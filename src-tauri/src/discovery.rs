use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures::{stream, StreamExt};
use if_addrs::IfAddr;
use ipnet::Ipv4Net;
use macaddr_ouidb::MacAddress;
use mdns_sd::{Receiver, ServiceDaemon, ServiceEvent};
use regex::Regex;
use tauri::ipc::Channel;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    time::timeout,
};

use crate::{
    background_process,
    models::{Device, NetworkInterface, ProfileStatus, ScanEvent, SshProfile},
};

const COMMON_PORTS: &[u16] = &[
    22, 53, 80, 139, 443, 445, 554, 1883, 3389, 5353, 5900, 8008, 8080, 8123, 9100,
];
const CONNECT_TIMEOUT: Duration = Duration::from_millis(220);
const MAX_SCAN_HOSTS: u64 = 1024;

#[derive(Clone, Debug, Default)]
struct ProbeInfo {
    ports: Vec<u16>,
    banners: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct ProtocolSignal {
    hostname: Option<String>,
    manufacturer: Option<String>,
    model: Option<String>,
    services: HashSet<String>,
    hints: Vec<String>,
}

impl ProtocolSignal {
    fn merge(&mut self, other: ProtocolSignal) {
        if self.hostname.is_none() {
            self.hostname = other.hostname;
        }
        if self.manufacturer.is_none() {
            self.manufacturer = other.manufacturer;
        }
        if self.model.is_none() {
            self.model = other.model;
        }
        self.services.extend(other.services);
        self.hints.extend(other.hints);
    }
}

pub fn interfaces() -> Result<Vec<NetworkInterface>, String> {
    let mut networks = Vec::new();
    let mut seen = HashSet::new();
    let addresses = if_addrs::get_if_addrs()
        .map_err(|error| format!("Could not inspect network interfaces: {error}"))?;

    for interface in addresses {
        let IfAddr::V4(address) = interface.addr else {
            continue;
        };
        if address.ip.is_loopback() || address.ip.is_link_local() {
            continue;
        }
        let prefix = prefix_from_netmask(address.netmask)?;
        // Avoid accidentally sweeping a large corporate or VPN range. A /24 is
        // enough for the common LAN case and still lets smaller subnets through.
        let scan_prefix = prefix.max(24);
        let network = Ipv4Net::new(address.ip, scan_prefix)
            .map_err(|error| format!("Invalid interface network: {error}"))?
            .trunc();
        let cidr = network.to_string();
        if seen.insert(cidr.clone()) {
            networks.push(NetworkInterface {
                name: interface.name,
                address: address.ip.to_string(),
                cidr,
            });
        }
    }
    networks.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(networks)
}

fn prefix_from_netmask(netmask: Ipv4Addr) -> Result<u8, String> {
    let bits = u32::from(netmask);
    let prefix = bits.leading_ones() as u8;
    let expected = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    if bits != expected {
        return Err(format!("Interface has a non-contiguous netmask: {netmask}"));
    }
    Ok(prefix)
}

pub async fn scan(cidr: &str, on_event: &Channel<ScanEvent>) -> Result<Vec<Device>, String> {
    let network: Ipv4Net = cidr
        .parse()
        .map_err(|_| "Enter a valid IPv4 network such as 192.168.1.0/24".to_string())?;
    let addresses = scan_addresses(network)?;
    let total = addresses.len();
    emit(on_event, ScanEvent::Started { total });

    // Passive service discovery runs alongside the address/port scan. These
    // protocols often provide the useful model name that a ping sweep cannot.
    let mdns_task = tokio::task::spawn_blocking(discover_mdns);
    let ssdp_task = tokio::spawn(discover_ssdp());

    let mut candidates: HashMap<Ipv4Addr, ProbeInfo> = HashMap::new();
    let mut announced = HashSet::new();
    let mut neighbors = HashMap::new();
    let mut probes = stream::iter(addresses)
        .map(|ip| async move { (ip, probe_host(ip).await) })
        .buffer_unordered(96);
    let now = now_ms();
    let mut completed = 0;

    while let Some((ip, probe)) = probes.next().await {
        completed += 1;
        if !probe.ports.is_empty() {
            candidates.insert(ip, probe.clone());
            if announced.insert(ip) {
                emit(
                    on_event,
                    ScanEvent::Device {
                        device: preliminary_device(ip, &probe, None, now),
                    },
                );
            }
        }

        // ARP/neighbor entries appear while probes are in flight. Reading them
        // periodically lets quiet devices show up before the full sweep ends.
        if completed % 16 == 0 || completed == total {
            neighbors = tokio::task::spawn_blocking(read_neighbor_table)
                .await
                .unwrap_or_default();
            for (neighbor_ip, mac) in &neighbors {
                if network.contains(neighbor_ip) {
                    let probe = candidates.entry(*neighbor_ip).or_default().clone();
                    if announced.insert(*neighbor_ip) {
                        emit(
                            on_event,
                            ScanEvent::Device {
                                device: preliminary_device(
                                    *neighbor_ip,
                                    &probe,
                                    Some(mac.clone()),
                                    now,
                                ),
                            },
                        );
                    }
                }
            }
        }

        if completed % 4 == 0 || completed == total {
            let percent = ((completed * 86) / total.max(1)).min(86) as u8;
            emit(
                on_event,
                ScanEvent::Progress {
                    completed,
                    total,
                    percent,
                    phase: format!("Searching addresses · {completed}/{total}"),
                },
            );
        }
    }

    emit(
        on_event,
        ScanEvent::Progress {
            completed: total,
            total,
            percent: 90,
            phase: "Identifying services and manufacturers".into(),
        },
    );

    let mut signals = mdns_task.await.unwrap_or_default();
    for (ip, signal) in ssdp_task.await.unwrap_or_default() {
        signals.entry(ip).or_default().merge(signal);
    }
    for ip in signals.keys().copied().collect::<Vec<_>>() {
        if network.contains(&ip) {
            candidates.entry(ip).or_default();
        }
    }
    for (ip, mac) in &neighbors {
        if network.contains(ip) {
            candidates.entry(*ip).or_default();
            if signals
                .get(ip)
                .and_then(|item| item.manufacturer.as_ref())
                .is_none()
            {
                if let Some(vendor) = manufacturer_for_mac(mac) {
                    signals.entry(*ip).or_default().manufacturer = Some(vendor);
                }
            }
        }
    }

    let candidate_total = candidates.len();
    let mut devices = Vec::with_capacity(candidate_total);
    for (index, (ip, probe)) in candidates.into_iter().enumerate() {
        let mut signal = signals.remove(&ip).unwrap_or_default();
        let dns_name =
            tokio::task::spawn_blocking(move || dns_lookup::lookup_addr(&IpAddr::V4(ip)).ok())
                .await
                .ok()
                .flatten()
                .filter(|name| name != &ip.to_string());
        if signal.hostname.is_none() {
            signal.hostname = dns_name;
        }
        if signal.hostname.is_none() && (probe.ports.contains(&139) || probe.ports.contains(&445)) {
            signal.hostname = netbios_name(ip).await;
            if signal.hostname.is_some() {
                signal.services.insert("NetBIOS".into());
            }
        }

        let mac = neighbors.get(&ip).cloned();
        if signal.manufacturer.is_none() {
            signal.manufacturer = mac.as_deref().and_then(manufacturer_for_mac);
        }
        let (device_type, confidence) = classify(&signal, &probe);
        let mut services: Vec<String> = signal.services.into_iter().collect();
        services.sort();
        let device = Device {
            ip: ip.to_string(),
            mac,
            hostname: signal.hostname,
            manufacturer: signal.manufacturer,
            model: signal.model,
            device_type: device_type.into(),
            confidence: confidence.into(),
            open_ports: probe.ports,
            services,
            reachable: true,
            last_seen_ms: now,
        };
        emit(
            on_event,
            ScanEvent::Device {
                device: device.clone(),
            },
        );
        devices.push(device);

        let identified = index + 1;
        let percent = 90 + ((identified * 10) / candidate_total.max(1)).min(10) as u8;
        emit(
            on_event,
            ScanEvent::Progress {
                completed: total,
                total,
                percent,
                phase: format!("Identifying devices · {identified}"),
            },
        );
    }

    devices.sort_by_key(|device| {
        device
            .ip
            .parse::<Ipv4Addr>()
            .map(u32::from)
            .unwrap_or(u32::MAX)
    });
    emit(
        on_event,
        ScanEvent::Progress {
            completed: total,
            total,
            percent: 100,
            phase: format!("Found {} devices", devices.len()),
        },
    );
    emit(
        on_event,
        ScanEvent::Finished {
            devices: devices.len(),
        },
    );
    Ok(devices)
}

fn emit(on_event: &Channel<ScanEvent>, event: ScanEvent) {
    // The receiver can disappear when the user changes views. That should not
    // turn an otherwise valid network scan into an error toast.
    let _ = on_event.send(event);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn preliminary_device(ip: Ipv4Addr, probe: &ProbeInfo, mac: Option<String>, now: u64) -> Device {
    let manufacturer = mac.as_deref().and_then(manufacturer_for_mac);
    let signal = ProtocolSignal {
        manufacturer: manufacturer.clone(),
        ..Default::default()
    };
    let (device_type, confidence) = classify(&signal, probe);
    Device {
        ip: ip.to_string(),
        mac,
        hostname: None,
        manufacturer,
        model: None,
        device_type: device_type.into(),
        confidence: confidence.into(),
        open_ports: probe.ports.clone(),
        services: Vec::new(),
        reachable: true,
        last_seen_ms: now,
    }
}

fn scan_addresses(network: Ipv4Net) -> Result<Vec<Ipv4Addr>, String> {
    let host_count = if network.prefix_len() >= 31 {
        1_u64 << (32 - network.prefix_len())
    } else {
        (1_u64 << (32 - network.prefix_len())) - 2
    };
    if host_count > MAX_SCAN_HOSTS {
        return Err(format!(
            "This app limits a scan to {MAX_SCAN_HOSTS} hosts. Choose a /22 or smaller range."
        ));
    }
    let first = u32::from(network.network());
    let last = u32::from(network.broadcast());
    let (start, end) = if network.prefix_len() >= 31 {
        (first, last)
    } else {
        (first + 1, last - 1)
    };
    Ok((start..=end).map(Ipv4Addr::from).collect())
}

async fn probe_host(ip: Ipv4Addr) -> ProbeInfo {
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await {
        let _ = socket
            .send_to(&[0], SocketAddr::new(IpAddr::V4(ip), 9))
            .await;
    }
    let checks = COMMON_PORTS.iter().copied().map(|port| async move {
        let address = SocketAddr::new(IpAddr::V4(ip), port);
        let mut socket = timeout(CONNECT_TIMEOUT, TcpStream::connect(address))
            .await
            .ok()?
            .ok()?;
        let banner = read_banner(&mut socket, port).await;
        Some((port, banner))
    });
    let mut probe = ProbeInfo::default();
    for (port, banner) in futures::future::join_all(checks)
        .await
        .into_iter()
        .flatten()
    {
        probe.ports.push(port);
        if let Some(banner) = banner {
            probe.banners.push(banner);
        }
    }
    probe.ports.sort_unstable();
    probe
}

async fn read_banner(socket: &mut TcpStream, port: u16) -> Option<String> {
    let limit = if port == 22 { 256 } else { 1024 };
    if matches!(port, 80 | 8008 | 8080 | 8123) {
        let _ = timeout(
            Duration::from_millis(120),
            socket.write_all(b"HEAD / HTTP/1.0\r\nHost: lantern.local\r\n\r\n"),
        )
        .await;
    }
    if port != 22 && !matches!(port, 80 | 8008 | 8080 | 8123) {
        return None;
    }
    let mut buffer = vec![0_u8; limit];
    let length = timeout(Duration::from_millis(220), socket.read(&mut buffer))
        .await
        .ok()?
        .ok()?;
    (length > 0).then(|| String::from_utf8_lossy(&buffer[..length]).into_owned())
}

fn read_neighbor_table() -> HashMap<Ipv4Addr, String> {
    let output = neighbor_command_output().unwrap_or_default();
    let ip_pattern = Regex::new(r"(?:\d{1,3}\.){3}\d{1,3}").expect("valid IPv4 regex");
    let mac_pattern =
        Regex::new(r"(?i)(?:[0-9a-f]{2}[:-]){5}[0-9a-f]{2}").expect("valid MAC regex");
    let mut neighbors = HashMap::new();
    for line in output.lines() {
        let (Some(ip_match), Some(mac_match)) = (ip_pattern.find(line), mac_pattern.find(line))
        else {
            continue;
        };
        let Ok(ip) = ip_match.as_str().parse::<Ipv4Addr>() else {
            continue;
        };
        let mac = mac_match.as_str().replace('-', ":").to_uppercase();
        if mac != "00:00:00:00:00:00" && mac != "FF:FF:FF:FF:FF:FF" {
            neighbors.insert(ip, mac);
        }
    }
    neighbors
}

fn run_command(program: &str, args: &[&str]) -> Option<String> {
    let output = background_process::command(program)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn neighbor_command_output() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        return run_command("arp", &["-a"]);
    }
    #[cfg(target_os = "macos")]
    {
        return run_command("arp", &["-an"]);
    }
    #[cfg(target_os = "linux")]
    {
        return run_command("ip", &["neigh", "show"]).or_else(|| run_command("arp", &["-an"]));
    }
    #[allow(unreachable_code)]
    run_command("arp", &["-an"])
}

fn manufacturer_for_mac(mac: &str) -> Option<String> {
    let address = mac.parse::<MacAddress>().ok()?;
    if address.is_local() {
        return Some("Private / randomized address".into());
    }
    address.oui().map(str::to_owned)
}

fn discover_mdns() -> HashMap<Ipv4Addr, ProtocolSignal> {
    const SERVICE_TYPES: &[(&str, &str)] = &[
        ("_workstation._tcp.local.", "Workstation"),
        ("_ssh._tcp.local.", "SSH"),
        ("_smb._tcp.local.", "SMB"),
        ("_http._tcp.local.", "HTTP"),
        ("_https._tcp.local.", "HTTPS"),
        ("_airplay._tcp.local.", "AirPlay"),
        ("_raop._tcp.local.", "AirPlay audio"),
        ("_googlecast._tcp.local.", "Google Cast"),
        ("_ipp._tcp.local.", "Printer"),
        ("_ipps._tcp.local.", "Printer"),
        ("_hap._tcp.local.", "HomeKit"),
        ("_home-assistant._tcp.local.", "Home Assistant"),
        ("_companion-link._tcp.local.", "Apple device"),
    ];

    let Ok(daemon) = ServiceDaemon::new() else {
        return HashMap::new();
    };
    let receivers: Vec<(&str, Receiver<ServiceEvent>)> = SERVICE_TYPES
        .iter()
        .filter_map(|(kind, label)| daemon.browse(kind).ok().map(|receiver| (*label, receiver)))
        .collect();
    let deadline = Instant::now() + Duration::from_millis(1450);
    let mut found: HashMap<Ipv4Addr, ProtocolSignal> = HashMap::new();
    while Instant::now() < deadline {
        for (label, receiver) in &receivers {
            while let Ok(event) = receiver.try_recv() {
                let ServiceEvent::ServiceResolved(info) = event else {
                    continue;
                };
                for ip in info.get_addresses_v4() {
                    let signal = found.entry(ip).or_default();
                    signal.services.insert((*label).into());
                    let hostname = info.get_hostname().trim_end_matches('.').to_string();
                    if !hostname.is_empty() {
                        signal.hostname.get_or_insert(hostname);
                    }
                    for key in ["model", "md", "am", "ty"] {
                        if let Some(value) = info.get_property_val_str(key) {
                            if !value.is_empty() {
                                signal.model.get_or_insert_with(|| value.to_string());
                                signal.hints.push(value.to_string());
                            }
                        }
                    }
                    for key in ["manufacturer", "mf", "vendor"] {
                        if let Some(value) = info.get_property_val_str(key) {
                            if !value.is_empty() {
                                signal.manufacturer.get_or_insert_with(|| value.to_string());
                            }
                        }
                    }
                    signal.hints.push(info.ty_domain.clone());
                }
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = daemon.shutdown();
    found
}

async fn discover_ssdp() -> HashMap<Ipv4Addr, ProtocolSignal> {
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await else {
        return HashMap::new();
    };
    let request = b"M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: ssdp:all\r\n\r\n";
    let _ = socket.send_to(request, "239.255.255.250:1900").await;
    let deadline = Instant::now() + Duration::from_millis(1300);
    let mut buffer = [0_u8; 4096];
    let mut found: HashMap<Ipv4Addr, ProtocolSignal> = HashMap::new();
    let mut descriptions = Vec::new();
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(Ok((length, source))) = timeout(remaining, socket.recv_from(&mut buffer)).await
        else {
            break;
        };
        let IpAddr::V4(ip) = source.ip() else {
            continue;
        };
        let headers = parse_headers(&String::from_utf8_lossy(&buffer[..length]));
        let signal = found.entry(ip).or_default();
        signal.services.insert("UPnP".into());
        for key in ["server", "st", "nt", "usn"] {
            if let Some(value) = headers.get(key) {
                signal.hints.push(value.clone());
            }
        }
        if let Some(location) = headers.get("location") {
            descriptions.push((ip, location.clone()));
        }
    }

    descriptions.sort();
    descriptions.dedup();
    let enriched: Vec<(Ipv4Addr, Option<ProtocolSignal>)> =
        stream::iter(descriptions.into_iter().take(24))
            .map(|(ip, location)| async move {
                let signal = fetch_upnp_description(ip, &location).await;
                (ip, signal)
            })
            .buffer_unordered(8)
            .collect()
            .await;
    for (ip, extra) in enriched {
        if let Some(extra) = extra {
            found.entry(ip).or_default().merge(extra);
        }
    }
    found
}

fn parse_headers(response: &str) -> HashMap<String, String> {
    response
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect()
}

async fn fetch_upnp_description(ip: Ipv4Addr, location: &str) -> Option<ProtocolSignal> {
    let rest = location.strip_prefix("http://")?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().ok()?),
        None => (authority, 80),
    };
    // Never follow an advertised URL off the responding LAN device.
    if host.parse::<Ipv4Addr>().ok()? != ip {
        return None;
    }
    let mut stream = timeout(Duration::from_millis(300), TcpStream::connect((ip, port)))
        .await
        .ok()?
        .ok()?;
    let request = format!("GET /{path} HTTP/1.0\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    timeout(
        Duration::from_millis(200),
        stream.write_all(request.as_bytes()),
    )
    .await
    .ok()?
    .ok()?;
    let mut body = Vec::new();
    timeout(
        Duration::from_millis(450),
        stream.take(32_768).read_to_end(&mut body),
    )
    .await
    .ok()?
    .ok()?;
    let xml = String::from_utf8_lossy(&body);
    let mut signal = ProtocolSignal::default();
    signal.hostname = xml_value(&xml, "friendlyName");
    signal.manufacturer = xml_value(&xml, "manufacturer");
    signal.model = xml_value(&xml, "modelName").or_else(|| xml_value(&xml, "modelNumber"));
    for tag in ["deviceType", "modelDescription", "modelName"] {
        if let Some(value) = xml_value(&xml, tag) {
            signal.hints.push(value);
        }
    }
    Some(signal)
}

fn xml_value(xml: &str, tag: &str) -> Option<String> {
    let pattern = Regex::new(&format!(r"(?is)<{tag}[^>]*>\s*([^<]+?)\s*</{tag}>")).ok()?;
    pattern
        .captures(xml)
        .and_then(|capture| capture.get(1))
        .map(|value| value.as_str().trim().to_string())
        .filter(|value| !value.is_empty())
}

async fn netbios_name(ip: Ipv4Addr) -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").await.ok()?;
    let query = netbios_query();
    socket.send_to(&query, (ip, 137)).await.ok()?;
    let mut data = [0_u8; 1024];
    let (length, _) = timeout(Duration::from_millis(350), socket.recv_from(&mut data))
        .await
        .ok()?
        .ok()?;
    parse_netbios_name(&data[..length])
}

fn netbios_query() -> Vec<u8> {
    let mut packet = vec![0x4c, 0x41, 0x00, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
    packet.push(32);
    packet.extend(std::iter::repeat_n(b'C', 32)); // encoded wildcard name
    packet.extend([0, 0, 0x21, 0, 1]);
    packet
}

fn parse_netbios_name(data: &[u8]) -> Option<String> {
    // NBSTAT answers finish with an array of 18-byte name records. Find a
    // workstation (suffix 0x00, not a group) and use its human-readable name.
    for chunk in data.windows(18) {
        if chunk[15] != 0 || chunk[16] & 0x80 != 0 {
            continue;
        }
        let name = String::from_utf8_lossy(&chunk[..15]).trim().to_string();
        if !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        {
            return Some(name);
        }
    }
    None
}

fn classify(signal: &ProtocolSignal, probe: &ProbeInfo) -> (&'static str, &'static str) {
    let evidence = [
        signal.hostname.as_deref().unwrap_or_default(),
        signal.manufacturer.as_deref().unwrap_or_default(),
        signal.model.as_deref().unwrap_or_default(),
        &signal.hints.join(" "),
        &signal
            .services
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(" "),
        &probe.banners.join(" "),
    ]
    .join(" ")
    .to_ascii_lowercase();

    let mut scores = [
        ("Raspberry Pi / Linux", 0_u8),
        ("macOS / Apple", 0),
        ("Windows", 0),
        ("Android", 0),
        ("Network device", 0),
        ("Linux / Unix", 0),
    ];
    add_score(
        &mut scores,
        "Raspberry Pi / Linux",
        &evidence,
        &["raspberry", "raspberry pi", "raspbian", "pihole", "pi-hole"],
        7,
    );
    add_score(
        &mut scores,
        "macOS / Apple",
        &evidence,
        &[
            "apple",
            "macbook",
            "imac",
            "iphone",
            "ipad",
            "airplay",
            "companion-link",
            "airport",
        ],
        6,
    );
    add_score(
        &mut scores,
        "Windows",
        &evidence,
        &[
            "microsoft",
            "windows",
            "desktop-",
            "laptop-",
            "microsoft-iis",
            "netbios",
        ],
        5,
    );
    add_score(
        &mut scores,
        "Android",
        &evidence,
        &[
            "android",
            "pixel",
            "galaxy",
            "google cast",
            "chromecast",
            "oneplus",
            "xiaomi",
        ],
        5,
    );
    add_score(
        &mut scores,
        "Network device",
        &evidence,
        &[
            "router",
            "gateway",
            "switch",
            "printer",
            "camera",
            "upnp",
            "synology",
            "qnap",
            "ubiquiti",
            "tp-link",
            "netgear",
            "espressif",
        ],
        5,
    );
    add_score(
        &mut scores,
        "Linux / Unix",
        &evidence,
        &[
            "ubuntu",
            "debian",
            "fedora",
            "linux",
            "openssh_",
            "home assistant",
        ],
        4,
    );

    if probe.ports.contains(&3389) {
        score(&mut scores, "Windows", 6);
    }
    if probe.ports.contains(&139) && probe.ports.contains(&445) {
        score(&mut scores, "Windows", 3);
    }
    if probe.ports.contains(&22) {
        score(&mut scores, "Linux / Unix", 2);
    }
    if probe
        .ports
        .iter()
        .any(|port| matches!(port, 53 | 554 | 9100))
    {
        score(&mut scores, "Network device", 3);
    }

    let (kind, points) = scores
        .into_iter()
        .max_by_key(|(_, points)| *points)
        .unwrap();
    if points == 0 {
        ("Unknown", "low")
    } else if points >= 6 {
        (kind, "high")
    } else if points >= 4 {
        (kind, "medium")
    } else {
        (kind, "low")
    }
}

fn add_score(
    scores: &mut [(&'static str, u8)],
    kind: &str,
    evidence: &str,
    hints: &[&str],
    points: u8,
) {
    if hints.iter().any(|hint| evidence.contains(hint)) {
        score(scores, kind, points);
    }
}

fn score(scores: &mut [(&'static str, u8)], kind: &str, points: u8) {
    if let Some((_, current)) = scores.iter_mut().find(|(name, _)| *name == kind) {
        *current = current.saturating_add(points);
    }
}

pub async fn profile_statuses(profiles: Vec<SshProfile>) -> Vec<ProfileStatus> {
    stream::iter(profiles)
        .map(|profile| async move {
            let started = Instant::now();
            let reachable = timeout(
                Duration::from_millis(850),
                TcpStream::connect((profile.host.as_str(), profile.port)),
            )
            .await
            .is_ok_and(|result| result.is_ok());
            ProfileStatus {
                id: profile.id,
                online: reachable,
                latency_ms: reachable.then(|| started.elapsed().as_millis() as u64),
            }
        })
        .buffer_unordered(32)
        .collect()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netmask_prefix_is_validated() {
        assert_eq!(
            prefix_from_netmask(Ipv4Addr::new(255, 255, 255, 0)).unwrap(),
            24
        );
        assert!(prefix_from_netmask(Ipv4Addr::new(255, 0, 255, 0)).is_err());
    }

    #[test]
    fn scan_range_is_bounded() {
        assert_eq!(
            scan_addresses("192.168.1.0/24".parse().unwrap())
                .unwrap()
                .len(),
            254
        );
        assert!(scan_addresses("10.0.0.0/8".parse().unwrap()).is_err());
    }

    #[test]
    fn classification_combines_multiple_signals() {
        let signal = ProtocolSignal {
            hostname: Some("studio-pi.local".into()),
            manufacturer: Some("Raspberry Pi Trading Ltd".into()),
            ..Default::default()
        };
        assert_eq!(
            classify(
                &signal,
                &ProbeInfo {
                    ports: vec![22],
                    banners: vec![]
                }
            )
            .0,
            "Raspberry Pi / Linux"
        );
        let windows = ProtocolSignal {
            hostname: Some("DESKTOP-FOO".into()),
            ..Default::default()
        };
        assert_eq!(
            classify(
                &windows,
                &ProbeInfo {
                    ports: vec![445, 3389],
                    banners: vec![]
                }
            )
            .0,
            "Windows"
        );
    }

    #[test]
    fn oui_lookup_handles_vendor_and_private_addresses() {
        assert!(manufacturer_for_mac("B8:27:EB:12:34:56").is_some());
        assert_eq!(
            manufacturer_for_mac("02:11:22:33:44:55").as_deref(),
            Some("Private / randomized address")
        );
    }

    #[test]
    fn upnp_xml_fields_are_extracted() {
        let xml = "<root><friendlyName>Living Room</friendlyName><manufacturer>Acme</manufacturer><modelName>Box 2</modelName></root>";
        assert_eq!(
            xml_value(xml, "friendlyName").as_deref(),
            Some("Living Room")
        );
        assert_eq!(xml_value(xml, "modelName").as_deref(), Some("Box 2"));
    }

    #[test]
    fn netbios_query_has_a_complete_question() {
        assert_eq!(netbios_query().len(), 50);
    }
}

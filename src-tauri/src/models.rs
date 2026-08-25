use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    pub name: String,
    pub address: String,
    pub cidr: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub ip: String,
    pub mac: Option<String>,
    pub hostname: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub device_type: String,
    pub confidence: String,
    pub open_ports: Vec<u16>,
    pub services: Vec<String>,
    pub reachable: bool,
    pub last_seen_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ScanEvent {
    Started {
        total: usize,
    },
    Progress {
        completed: usize,
        total: usize,
        percent: u8,
        phase: String,
    },
    Device {
        device: Device,
    },
    Finished {
        devices: usize,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub identity_file: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub last_connected_at: Option<u64>,
    #[serde(default)]
    pub has_saved_password: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileStatus {
    pub id: String,
    pub online: bool,
    pub latency_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TerminalEvent {
    Data { data: String },
    Exit { code: Option<u32> },
    Error { message: String },
}

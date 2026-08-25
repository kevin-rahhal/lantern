import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  Device,
  NetworkInterface,
  ProfileStatus,
  ScanEvent,
  SshProfile,
  TerminalEvent,
} from "./types";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

const PROFILE_KEY = "lantern-demo-profiles";
const mockSessions = new Map<string, (event: TerminalEvent) => void>();
const mockPasswords = new Set<string>();

const demoDevices: Device[] = [
  {
    ip: "192.168.1.12",
    mac: "B8:27:EB:7A:21:04",
    hostname: "studio-pi.local",
    manufacturer: "Raspberry Pi Trading Ltd",
    model: "Raspberry Pi 5",
    deviceType: "Raspberry Pi / Linux",
    confidence: "high",
    openPorts: [22, 80],
    services: ["SSH", "HTTP", "Workstation"],
    reachable: true,
    lastSeenMs: Date.now(),
  },
  {
    ip: "192.168.1.24",
    mac: "A4:83:E7:14:8C:31",
    hostname: "alex-macbook.local",
    manufacturer: "Apple, Inc.",
    model: "MacBook Pro",
    deviceType: "macOS / Apple",
    confidence: "high",
    openPorts: [22, 5900],
    services: ["SSH", "AirPlay", "Apple device"],
    reachable: true,
    lastSeenMs: Date.now(),
  },
  {
    ip: "192.168.1.38",
    mac: "30:5A:3A:88:5F:10",
    hostname: "DESKTOP-NOVA",
    manufacturer: "Microsoft Corporation",
    model: null,
    deviceType: "Windows",
    confidence: "high",
    openPorts: [445, 3389],
    services: ["SMB", "NetBIOS"],
    reachable: true,
    lastSeenMs: Date.now(),
  },
  {
    ip: "192.168.1.51",
    mac: "DA:3A:7B:81:9E:42",
    hostname: "pixel-9.local",
    manufacturer: "Private / randomized address",
    model: "Pixel 9",
    deviceType: "Android",
    confidence: "medium",
    openPorts: [],
    services: ["Google Cast"],
    reachable: true,
    lastSeenMs: Date.now(),
  },
];

function isDesktop(): boolean {
  return Boolean(window.__TAURI_INTERNALS__);
}

function readMockProfiles(): SshProfile[] {
  const saved = localStorage.getItem(PROFILE_KEY);
  if (saved) return (JSON.parse(saved) as SshProfile[]).map((profile) => ({ ...profile, hasSavedPassword: mockPasswords.has(profile.id) }));
  return [
    {
      id: "demo-studio-pi",
      name: "Studio Pi",
      host: "192.168.1.12",
      port: 22,
      username: "alex",
      identityFile: null,
      tags: ["lab"],
      lastConnectedAt: null,
      hasSavedPassword: false,
    },
  ];
}

export const api = {
  isDesktop,

  async interfaces(): Promise<NetworkInterface[]> {
    if (isDesktop()) return invoke("list_interfaces");
    return [{ name: "Wi-Fi (demo)", address: "192.168.1.7", cidr: "192.168.1.0/24" }];
  },

  async scan(cidr: string, onEvent: (event: ScanEvent) => void): Promise<Device[]> {
    if (isDesktop()) {
      const channel = new Channel<ScanEvent>();
      channel.onmessage = onEvent;
      return invoke("scan_network", { cidr, onEvent: channel });
    }
    onEvent({ kind: "started", total: 254 });
    for (let index = 0; index < demoDevices.length; index += 1) {
      await new Promise((resolve) => window.setTimeout(resolve, 180));
      const device = demoDevices[index];
      if (!device) continue;
      onEvent({ kind: "device", device });
      onEvent({
        kind: "progress",
        completed: Math.round(((index + 1) / demoDevices.length) * 254),
        total: 254,
        percent: Math.round(((index + 1) / demoDevices.length) * 92),
        phase: index === demoDevices.length - 1 ? "Identifying services and manufacturers" : `Searching addresses · ${Math.round(((index + 1) / demoDevices.length) * 254)}/254`,
      });
    }
    await new Promise((resolve) => window.setTimeout(resolve, 180));
    onEvent({ kind: "progress", completed: 254, total: 254, percent: 100, phase: `Found ${demoDevices.length} devices` });
    onEvent({ kind: "finished", devices: demoDevices.length });
    return demoDevices;
  },

  async loadProfiles(): Promise<SshProfile[]> {
    if (isDesktop()) return invoke("load_profiles");
    return readMockProfiles();
  },

  async saveProfile(profile: SshProfile): Promise<SshProfile[]> {
    if (isDesktop()) return invoke("save_profile", { profile });
    profile = { ...profile, hasSavedPassword: mockPasswords.has(profile.id) };
    const profiles = readMockProfiles();
    const index = profiles.findIndex((item) => item.id === profile.id);
    if (index >= 0) profiles[index] = profile;
    else profiles.push(profile);
    localStorage.setItem(PROFILE_KEY, JSON.stringify(profiles));
    return profiles;
  },

  async deleteProfile(id: string): Promise<SshProfile[]> {
    if (isDesktop()) return invoke("delete_profile", { id });
    mockPasswords.delete(id);
    const profiles = readMockProfiles().filter((profile) => profile.id !== id);
    localStorage.setItem(PROFILE_KEY, JSON.stringify(profiles));
    return profiles;
  },

  async savePassword(profileId: string, password: string): Promise<void> {
    if (isDesktop()) return invoke("save_password", { profileId, password });
    if (!password) throw new Error("Password cannot be empty");
    mockPasswords.add(profileId);
  },

  async deletePassword(profileId: string): Promise<void> {
    if (isDesktop()) return invoke("delete_password", { profileId });
    mockPasswords.delete(profileId);
  },

  async profileStatuses(profiles: SshProfile[]): Promise<ProfileStatus[]> {
    if (isDesktop()) return invoke("check_profile_statuses", { profiles });
    return profiles.map((profile) => ({
      id: profile.id,
      online: !profile.host.toLowerCase().includes("offline"),
      latencyMs: profile.host.toLowerCase().includes("offline") ? null : 8,
    }));
  },

  async sshAvailable(): Promise<boolean> {
    if (isDesktop()) return invoke("ssh_available");
    return true;
  },

  async startSsh(
    profile: SshProfile,
    cols: number,
    rows: number,
    onEvent: (event: TerminalEvent) => void,
  ): Promise<string> {
    if (isDesktop()) {
      const channel = new Channel<TerminalEvent>();
      channel.onmessage = onEvent;
      return invoke("start_ssh", { profile, cols, rows, onEvent: channel });
    }

    const id = crypto.randomUUID();
    mockSessions.set(id, onEvent);
    window.setTimeout(() => {
      onEvent({
        kind: "data",
        data: `\u001b[38;5;114mLantern browser preview\u001b[0m\r\nConnecting to ${profile.username}@${profile.host}…\r\n\r\nThe packaged desktop app opens the real SSH session here.\r\n${profile.username}@${profile.name.toLowerCase().replaceAll(" ", "-")}:~$ `,
      });
    }, 180);
    return id;
  },

  async writeSsh(sessionId: string, data: string): Promise<void> {
    if (isDesktop()) return invoke("write_ssh", { sessionId, data });
    const notify = mockSessions.get(sessionId);
    notify?.({ kind: "data", data });
  },

  async resizeSsh(sessionId: string, cols: number, rows: number): Promise<void> {
    if (isDesktop()) return invoke("resize_ssh", { sessionId, cols, rows });
  },

  async closeSsh(sessionId: string): Promise<void> {
    if (isDesktop()) await invoke("close_ssh", { sessionId });
    mockSessions.delete(sessionId);
  },
};

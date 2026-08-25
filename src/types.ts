export type DeviceKind =
  | "macOS / Apple"
  | "Windows"
  | "Android"
  | "Raspberry Pi / Linux"
  | "Linux / Unix"
  | "Network device"
  | "Unknown";

export interface NetworkInterface {
  name: string;
  address: string;
  cidr: string;
}

export interface Device {
  ip: string;
  mac: string | null;
  hostname: string | null;
  manufacturer: string | null;
  model: string | null;
  deviceType: DeviceKind;
  confidence: "high" | "medium" | "low";
  openPorts: number[];
  services: string[];
  reachable: boolean;
  lastSeenMs: number;
}

export type ScanEvent =
  | { kind: "started"; total: number }
  | { kind: "progress"; completed: number; total: number; percent: number; phase: string }
  | { kind: "device"; device: Device }
  | { kind: "finished"; devices: number };

export interface SshProfile {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  identityFile: string | null;
  tags: string[];
  lastConnectedAt: number | null;
  hasSavedPassword: boolean;
}

export interface ProfileStatus {
  id: string;
  online: boolean;
  latencyMs: number | null;
}

export type TerminalEvent =
  | { kind: "data"; data: string }
  | { kind: "exit"; code: number | null }
  | { kind: "error"; message: string };

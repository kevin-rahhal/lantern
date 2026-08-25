import "@xterm/xterm/css/xterm.css";
import "./style.css";
import { api } from "./api";
import { icon } from "./icons";
import type { FitAddon as XtermFitAddon } from "@xterm/addon-fit";
import type { Terminal as XtermTerminal } from "@xterm/xterm";
import type { Device, DeviceKind, NetworkInterface, ProfileStatus, ScanEvent, SshProfile } from "./types";

type AppView = "devices" | "profiles";
type View = AppView | "terminals";

interface TerminalTab {
  sessionId: string;
  profile: SshProfile;
  displayName: string;
  deviceKey: string;
  ordinal: number;
  terminal: XtermTerminal;
  fit: XtermFitAddon;
  host: HTMLDivElement;
  observer: ResizeObserver;
  ended: boolean;
  cleanup: () => void;
}

const terminalTabs = new Map<string, TerminalTab>();
const terminalSessionCounters = new Map<string, number>();
const terminalOpeningCounts = new Map<string, number>();
let activeTerminalId: string | null = null;
const NETWORK_STORAGE_KEY = "lantern-selected-network";
const SIDEBAR_STORAGE_KEY = "lantern-sidebar-collapsed";
const DEVICE_ALIAS_STORAGE_KEY = "lantern-device-aliases";

const state: {
  view: View;
  interfaces: NetworkInterface[];
  selectedCidr: string;
  devices: Device[];
  deviceAliases: Record<string, string>;
  profiles: SshProfile[];
  statuses: Map<string, ProfileStatus>;
  search: string;
  filter: "All" | DeviceKind;
  scanning: boolean;
  scanProgress: { percent: number; phase: string } | null;
  sidebarCollapsed: boolean;
  error: string | null;
  previousView: AppView;
} = {
  view: "devices",
  interfaces: [],
  selectedCidr: "",
  devices: [],
  deviceAliases: readDeviceAliases(),
  profiles: [],
  statuses: new Map(),
  search: "",
  filter: "All",
  scanning: false,
  scanProgress: null,
  sidebarCollapsed: localStorage.getItem(SIDEBAR_STORAGE_KEY) === "true",
  error: null,
  previousView: "devices",
};

const appRoot = document.querySelector<HTMLDivElement>("#app");
if (!appRoot) throw new Error("App mount point is missing");
const app: HTMLDivElement = appRoot;
const terminalMount = document.querySelector<HTMLDivElement>("#terminal-root");
if (!terminalMount) throw new Error("Terminal mount point is missing");
const terminalRoot: HTMLDivElement = terminalMount;

const escapeHtml = (value: string | number | null | undefined): string =>
  String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");

function initials(value: string): string {
  return value
    .split(/[\s.-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase())
    .join("");
}

function setError(error: unknown): void {
  state.error = error instanceof Error ? error.message : String(error);
  render();
}

function readDeviceAliases(): Record<string, string> {
  try {
    const stored = JSON.parse(localStorage.getItem(DEVICE_ALIAS_STORAGE_KEY) ?? "{}");
    if (!stored || typeof stored !== "object" || Array.isArray(stored)) return {};
    return Object.fromEntries(
      Object.entries(stored).filter((entry): entry is [string, string] => typeof entry[1] === "string"),
    );
  } catch {
    return {};
  }
}

function aliasKeys(device: Device): string[] {
  const keys = [`ip:${device.ip.trim().toLowerCase()}`];
  if (device.mac) keys.unshift(`mac:${device.mac.replaceAll(/[:-]/g, "").toLowerCase()}`);
  return keys;
}

function deviceAlias(device: Device): string | undefined {
  return aliasKeys(device).map((key) => state.deviceAliases[key]).find(Boolean);
}

function persistDeviceAlias(device: Device, name: string): void {
  const cleaned = name.trim();
  aliasKeys(device).forEach((key) => {
    if (cleaned) state.deviceAliases[key] = cleaned;
    else delete state.deviceAliases[key];
  });
  localStorage.setItem(DEVICE_ALIAS_STORAGE_KEY, JSON.stringify(state.deviceAliases));
}

function filteredDevices(): Device[] {
  const query = state.search.trim().toLowerCase();
  return state.devices.filter((device) => {
    const matchesType = state.filter === "All" || device.deviceType === state.filter;
    const savedName = savedProfileFor(device)?.name;
    const haystack = [device.ip, device.mac, device.hostname, device.manufacturer, device.model, device.deviceType, device.services.join(" "), savedName, deviceAlias(device)]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    return matchesType && (!query || haystack.includes(query));
  });
}

function savedProfileFor(device: Device): SshProfile | undefined {
  const addresses = [device.ip, device.hostname]
    .filter((value): value is string => Boolean(value))
    .map((value) => value.trim().toLowerCase());
  return state.profiles.find((profile) => addresses.includes(profile.host.trim().toLowerCase()));
}

function deviceName(device: Device): string {
  return deviceAlias(device) || savedProfileFor(device)?.name || device.hostname || device.model || device.manufacturer || "Unnamed device";
}

function layout(content: string): string {
  const demo = api.isDesktop() ? "" : '<span class="demo-pill">Browser preview</span>';
  return `
    <div class="shell ${state.sidebarCollapsed ? "sidebar-collapsed" : ""}">
      <aside class="sidebar">
        <div class="brand"><span class="brand-mark">${icon("network", 20)}</span><span class="brand-name">Lantern</span></div>
        <button class="sidebar-toggle" id="sidebar-toggle" aria-label="${state.sidebarCollapsed ? "Expand" : "Collapse"} navigation" title="${state.sidebarCollapsed ? "Expand" : "Collapse"} navigation">${icon("chevron", 15)}</button>
        <nav aria-label="Primary navigation">
          <button class="nav-item ${state.view === "devices" ? "active" : ""}" data-view="devices" title="Devices">${icon("radar")}<span>Devices</span></button>
          <button class="nav-item ${state.view === "profiles" ? "active" : ""}" data-view="profiles" title="SSH profiles">${icon("bookmark")}<span>SSH profiles</span></button>
          ${terminalTabs.size ? `<button class="nav-item ${state.view === "terminals" ? "active" : ""}" data-view="terminals" title="Terminals" data-terminal-count="${terminalTabs.size}">${icon("terminal")}<span>Terminals</span><small class="nav-count">${terminalTabs.size}</small></button>` : ""}
        </nav>
        <div class="sidebar-spacer"></div>
        <div class="sidebar-foot">
          ${demo}
          <span class="version">v0.1.0 MVP</span>
        </div>
      </aside>
      <main class="main">${content}</main>
    </div>
    ${state.error ? `<div class="toast" role="alert"><span>${escapeHtml(state.error)}</span><button id="dismiss-error" aria-label="Dismiss">${icon("x", 16)}</button></div>` : ""}
    <div id="modal-root"></div>
  `;
}

function deviceView(): string {
  const devices = filteredDevices();
  const types: Array<"All" | DeviceKind> = [
    "All",
    "macOS / Apple",
    "Windows",
    "Android",
    "Raspberry Pi / Linux",
    "Linux / Unix",
    "Network device",
    "Unknown",
  ];
  const interfaceOptions = state.interfaces.length
    ? state.interfaces.map((item) => `<option value="${escapeHtml(item.cidr)}" ${item.cidr === state.selectedCidr ? "selected" : ""}>${escapeHtml(item.name)} · ${escapeHtml(item.cidr)}</option>`).join("")
    : '<option value="">No IPv4 network found</option>';

  return layout(`
    <header class="topbar">
      <div><p class="eyebrow">LOCAL NETWORK</p><h1>Devices</h1><p class="subtitle">Find devices on your LAN and connect without leaving the app.</p></div>
      <button class="primary" id="scan" ${state.scanning || !state.selectedCidr ? "disabled" : ""}>${state.scanning ? '<span class="spinner"></span>Scanning…' : `${icon("refresh", 17)} Scan network`}</button>
    </header>
    <div class="scan-progress-host">${scanProgressMarkup()}</div>
    <section class="toolbar" aria-label="Device filters">
      <label class="search">${icon("search", 17)}<input id="search" value="${escapeHtml(state.search)}" placeholder="Search IP, MAC, hostname…" aria-label="Search devices" /></label>
      <label class="network-select"><span>Network</span><select id="network">${interfaceOptions}</select></label>
    </section>
    <div class="filter-row">${types.map((type) => `<button class="filter-chip ${state.filter === type ? "active" : ""}" data-filter="${escapeHtml(type)}">${escapeHtml(type === "Raspberry Pi / Linux" ? "Pi / Linux" : type)}</button>`).join("")}</div>
    ${devicePanel(devices)}
  `);
}

function scanProgressMarkup(): string {
  if (!state.scanning || !state.scanProgress) return "";
  return `<section class="scan-progress" aria-live="polite"><div class="scan-progress-copy"><span>${escapeHtml(state.scanProgress.phase)}</span><strong>${state.scanProgress.percent}%</strong></div><div class="progress-track"><i style="width:${state.scanProgress.percent}%"></i></div></section>`;
}

function devicePanel(devices = filteredDevices()): string {
  return `<section class="panel device-panel">
    <div class="panel-heading"><span>${devices.length} device${devices.length === 1 ? "" : "s"}</span><span class="muted">Names combine local services, hostnames, and MAC vendors</span></div>
    ${state.scanning && state.devices.length === 0 ? scanSkeleton() : deviceTable(devices)}
  </section>`;
}

function scanSkeleton(): string {
  return `<div class="skeleton-list">${Array.from({ length: 5 }, (_, index) => `<div class="skeleton-row"><i style="--delay:${index * 90}ms"></i><span></span><span></span><span></span></div>`).join("")}</div>`;
}

function deviceTable(devices: Device[]): string {
  if (devices.length === 0) {
    return `<div class="empty"><span class="empty-icon">${icon("radar", 28)}</span><h2>${state.devices.length ? "No matching devices" : "Ready to discover"}</h2><p>${state.devices.length ? "Try a broader search or another filter." : "Choose a local network and start a scan."}</p></div>`;
  }
  return `<div class="device-list" role="table">
    <div class="device-row table-head" role="row"><span>Device</span><span>Address</span><span>Type</span><span>Services</span><span></span></div>
    ${devices.map((device) => {
      const profile = savedProfileFor(device);
      const name = deviceName(device);
      const ssh = device.openPorts.includes(22);
      const serviceNames = device.services.length ? device.services : device.openPorts.map((port) => port === 22 ? "SSH" : String(port));
      const detail = [device.manufacturer, device.model].filter(Boolean).join(" · ");
      return `<div class="device-row ${profile ? "saved-device" : ""}" role="row">
        <div class="device-identity"><span class="avatar ${device.reachable ? "online" : ""}">${escapeHtml(initials(name) || "?")}</span><div><strong>${escapeHtml(name)}</strong><small>${escapeHtml(device.mac || "MAC unavailable")}</small></div></div>
        <div class="mono">${escapeHtml(device.ip)}</div>
        <div><span class="type-label">${escapeHtml(device.deviceType)}${deviceAlias(device) ? '<em class="saved-badge personal-badge">Named</em>' : profile ? '<em class="saved-badge">Saved</em>' : ""}</span><small class="confidence">${escapeHtml(detail || `${device.confidence} confidence`)}</small></div>
        <div class="port-list">${serviceNames.length ? serviceNames.slice(0, 4).map((service) => `<span>${escapeHtml(service)}</span>`).join("") : '<span class="muted">None detected</span>'}</div>
        <div class="row-actions"><button class="icon-button rename-device" data-ip="${escapeHtml(device.ip)}" aria-label="Rename ${escapeHtml(name)}" title="Give this device a personal name">${icon("edit", 16)}</button>${profile || ssh ? `<button class="ghost connect-device" data-profile-id="${escapeHtml(profile?.id)}" data-ip="${escapeHtml(device.ip)}" data-name="${escapeHtml(name)}">${icon("terminal", 16)} Connect</button>` : `<button class="icon-button save-device" data-ip="${escapeHtml(device.ip)}" data-name="${escapeHtml(name)}" aria-label="Save ${escapeHtml(name)} as SSH profile" title="Save SSH profile">${icon("plus", 17)}</button>`}</div>
      </div>`;
    }).join("")}
  </div>`;
}

function profilesView(): string {
  return layout(`
    <header class="topbar">
      <div><p class="eyebrow">QUICK CONNECT</p><h1>SSH profiles</h1><p class="subtitle">Saved hosts, current reachability, one-click terminals.</p></div>
      <button class="primary" id="new-profile">${icon("plus", 17)} New profile</button>
    </header>
    <section class="profiles-grid">
      ${state.profiles.length ? state.profiles.map(profileCard).join("") : `<div class="empty panel full"><span class="empty-icon">${icon("bookmark", 28)}</span><h2>No profiles yet</h2><p>Save a device or create a profile to get started.</p><button class="secondary" id="empty-new-profile">Create profile</button></div>`}
    </section>
    <section class="security-note"><span class="note-mark">✓</span><div><strong>Passwords are protected by your operating system</strong><p>Optional saved passwords live in Windows Credential Manager, macOS Keychain, or Linux Secret Service—not in Lantern’s profile files. Changed host keys are still rejected.</p></div></section>
  `);
}

function profileCard(profile: SshProfile): string {
  const status = state.statuses.get(profile.id);
  const statusClass = status?.online ? "online" : "offline";
  const statusText = status ? (status.online ? `${status.latencyMs ?? "—"} ms` : "Offline") : "Checking…";
  return `<article class="profile-card panel">
    <div class="profile-top"><span class="profile-avatar">${escapeHtml(initials(profile.name) || "SH")}</span><span class="status ${statusClass}"><i></i>${statusText}</span></div>
    <h2>${escapeHtml(profile.name)}</h2>
    <p class="profile-target mono">${escapeHtml(profile.username)}@${escapeHtml(profile.host)}:${escapeHtml(profile.port)}</p>
    <div class="tags">${profile.tags.map((tag) => `<span>${escapeHtml(tag)}</span>`).join("")}${profile.identityFile ? '<span>key</span>' : ""}${profile.hasSavedPassword ? '<span class="secure-tag">vault password</span>' : '<span>agent / prompt</span>'}</div>
    <div class="profile-actions"><button class="primary connect-profile" data-id="${escapeHtml(profile.id)}" ${status && !status.online ? "" : ""}>${icon("terminal", 16)} Connect</button><button class="icon-button edit-profile" data-id="${escapeHtml(profile.id)}" title="Edit profile">${icon("settings", 17)}</button></div>
  </article>`;
}

function render(): void {
  document.body.classList.toggle("sidebar-is-collapsed", state.sidebarCollapsed);
  app.innerHTML = state.view === "devices" ? deviceView() : state.view === "profiles" ? profilesView() : layout('<div class="terminal-placeholder" aria-hidden="true"></div>');
  bindCommon();
  if (state.view === "devices") bindDevices();
  else if (state.view === "profiles") bindProfiles();
  syncTerminalWorkspaceVisibility();
}

function bindCommon(): void {
  document.querySelector<HTMLButtonElement>("#sidebar-toggle")?.addEventListener("click", () => {
    state.sidebarCollapsed = !state.sidebarCollapsed;
    localStorage.setItem(SIDEBAR_STORAGE_KEY, String(state.sidebarCollapsed));
    render();
    if (state.view === "terminals" && activeTerminalId) activateTerminal(activeTerminalId);
  });
  document.querySelectorAll<HTMLButtonElement>("[data-view]").forEach((button) => {
    button.addEventListener("click", () => {
      const nextView = button.dataset.view as View;
      if (nextView === "terminals" && terminalTabs.size === 0) return;
      if (nextView === "terminals" && state.view !== "terminals") state.previousView = state.view as AppView;
      if (nextView !== "terminals") state.previousView = nextView;
      state.view = nextView;
      render();
      if (nextView === "terminals" && activeTerminalId) activateTerminal(activeTerminalId);
    });
  });
  document.querySelector<HTMLButtonElement>("#dismiss-error")?.addEventListener("click", () => {
    state.error = null;
    render();
  });
}

function bindDevices(): void {
  document.querySelector<HTMLInputElement>("#search")?.addEventListener("input", (event) => {
    state.search = (event.target as HTMLInputElement).value;
    refreshDevicePanel();
  });
  document.querySelector<HTMLSelectElement>("#network")?.addEventListener("change", (event) => {
    state.selectedCidr = (event.target as HTMLSelectElement).value;
    localStorage.setItem(NETWORK_STORAGE_KEY, state.selectedCidr);
  });
  document.querySelector<HTMLButtonElement>("#scan")?.addEventListener("click", scan);
  document.querySelectorAll<HTMLButtonElement>("[data-filter]").forEach((button) => {
    button.addEventListener("click", () => {
      state.filter = button.dataset.filter as typeof state.filter;
      render();
    });
  });
  bindDeviceActions();
}

function bindDeviceActions(): void {
  document.querySelectorAll<HTMLButtonElement>(".rename-device").forEach((button) => {
    button.addEventListener("click", () => {
      const device = state.devices.find((item) => item.ip === button.dataset.ip);
      if (device) openDeviceNameModal(device);
    });
  });
  document.querySelectorAll<HTMLButtonElement>(".connect-device").forEach((button) => {
    button.addEventListener("click", () => {
      const profile = state.profiles.find((item) => item.id === button.dataset.profileId);
      if (profile) void openTerminal(profile);
      else openProfileModal(undefined, button.dataset.ip, button.dataset.name, true);
    });
  });
  document.querySelectorAll<HTMLButtonElement>(".save-device").forEach((button) => {
    button.addEventListener("click", () => openProfileModal(undefined, button.dataset.ip, button.dataset.name));
  });
}

function openDeviceNameModal(device: Device): void {
  const root = document.querySelector<HTMLDivElement>("#modal-root");
  if (!root) return;
  const currentAlias = deviceAlias(device) ?? "";
  const detectedName = savedProfileFor(device)?.name || device.hostname || device.model || device.manufacturer || device.ip;
  root.innerHTML = `<div class="modal-backdrop"><section class="modal device-name-modal" role="dialog" aria-modal="true" aria-labelledby="device-name-title">
    <div class="modal-head"><div><p class="eyebrow">PERSONAL LABEL</p><h2 id="device-name-title">Name this device</h2></div><button class="icon-button" id="close-modal" aria-label="Close">${icon("x", 18)}</button></div>
    <form id="device-name-form">
      <label><span>Device name</span><input name="deviceName" required maxlength="80" value="${escapeHtml(currentAlias)}" placeholder="Living room media server" /></label>
      <p class="form-help">Currently detected as <strong>${escapeHtml(detectedName)}</strong>. Your personal name is stored on this computer and matched by MAC address whenever one is available.</p>
      <div class="modal-actions">${currentAlias ? '<button type="button" class="secondary" id="remove-device-name">Use detected name</button>' : ""}<span></span><button type="button" class="secondary" id="cancel-modal">Cancel</button><button type="submit" class="primary">Save name</button></div>
    </form>
  </section></div>`;
  const close = () => { root.innerHTML = ""; };
  root.querySelector("#close-modal")?.addEventListener("click", close);
  root.querySelector("#cancel-modal")?.addEventListener("click", close);
  root.querySelector(".modal-backdrop")?.addEventListener("click", (event) => {
    if (event.target === event.currentTarget) close();
  });
  root.querySelector("#remove-device-name")?.addEventListener("click", () => {
    persistDeviceAlias(device, "");
    close();
    refreshDevicePanel();
  });
  root.querySelector<HTMLFormElement>("#device-name-form")?.addEventListener("submit", (event) => {
    event.preventDefault();
    const form = new FormData(event.currentTarget as HTMLFormElement);
    persistDeviceAlias(device, String(form.get("deviceName") ?? ""));
    close();
    refreshDevicePanel();
  });
  root.querySelector<HTMLInputElement>('input[name="deviceName"]')?.focus();
}

function refreshDevicePanel(): void {
  const panel = document.querySelector<HTMLElement>(".device-panel");
  if (!panel) return;
  panel.outerHTML = devicePanel();
  bindDeviceActions();
}

function refreshScanProgress(): void {
  const host = document.querySelector<HTMLElement>(".scan-progress-host");
  if (host) host.innerHTML = scanProgressMarkup();
}

function mergeDevice(device: Device): void {
  const index = state.devices.findIndex((item) => item.ip === device.ip);
  if (index >= 0) state.devices[index] = device;
  else state.devices.push(device);
  state.devices.sort((left, right) => {
    const leftParts = left.ip.split(".").map(Number);
    const rightParts = right.ip.split(".").map(Number);
    return leftParts.reduce((value, part) => value * 256 + part, 0) - rightParts.reduce((value, part) => value * 256 + part, 0);
  });
}

function bindProfiles(): void {
  document.querySelector<HTMLButtonElement>("#new-profile")?.addEventListener("click", () => openProfileModal());
  document.querySelector<HTMLButtonElement>("#empty-new-profile")?.addEventListener("click", () => openProfileModal());
  document.querySelectorAll<HTMLButtonElement>(".connect-profile").forEach((button) => {
    button.addEventListener("click", () => {
      const profile = state.profiles.find((item) => item.id === button.dataset.id);
      if (profile) void openTerminal(profile);
    });
  });
  document.querySelectorAll<HTMLButtonElement>(".edit-profile").forEach((button) => {
    button.addEventListener("click", () => openProfileModal(state.profiles.find((item) => item.id === button.dataset.id)));
  });
}

async function scan(): Promise<void> {
  if (!state.selectedCidr || state.scanning) return;
  state.scanning = true;
  state.devices = [];
  state.scanProgress = { percent: 0, phase: "Preparing network scan" };
  state.error = null;
  render();
  try {
    const devices = await api.scan(state.selectedCidr, (event: ScanEvent) => {
      if (event.kind === "started") {
        state.scanProgress = { percent: 0, phase: `Searching ${event.total} addresses` };
        refreshScanProgress();
      } else if (event.kind === "progress") {
        state.scanProgress = { percent: event.percent, phase: event.phase };
        refreshScanProgress();
      } else if (event.kind === "device") {
        mergeDevice(event.device);
        refreshDevicePanel();
      } else if (event.kind === "finished") {
        state.scanProgress = { percent: 100, phase: `Found ${event.devices} devices` };
        refreshScanProgress();
      }
    });
    state.devices = devices;
    await new Promise((resolve) => window.setTimeout(resolve, 260));
  } catch (error) {
    setError(error);
  } finally {
    state.scanning = false;
    state.scanProgress = null;
    render();
  }
}

function openProfileModal(profile?: SshProfile, host = "", name = "", connectAfterSave = false): void {
  const root = document.querySelector<HTMLDivElement>("#modal-root");
  if (!root) return;
  const current: SshProfile = profile ?? {
    id: crypto.randomUUID(),
    name: name || "",
    host,
    port: 22,
    username: "",
    identityFile: null,
    tags: [],
    lastConnectedAt: null,
    hasSavedPassword: false,
  };
  root.innerHTML = `<div class="modal-backdrop"><section class="modal" role="dialog" aria-modal="true" aria-labelledby="profile-title">
    <div class="modal-head"><div><p class="eyebrow">SSH CONNECTION</p><h2 id="profile-title">${profile ? "Edit profile" : "New profile"}</h2></div><button class="icon-button" id="close-modal" aria-label="Close">${icon("x", 18)}</button></div>
    <form id="profile-form">
      <label><span>Name</span><input name="name" required value="${escapeHtml(current.name)}" placeholder="Studio Pi" /></label>
      <div class="form-row"><label class="grow"><span>Host or IP</span><input name="host" required value="${escapeHtml(current.host)}" placeholder="192.168.1.24" /></label><label class="port"><span>Port</span><input name="port" type="number" min="1" max="65535" required value="${current.port}" /></label></div>
      <label><span>Username</span><input name="username" required value="${escapeHtml(current.username)}" placeholder="alex" autocomplete="username" /></label>
      <label><span>Private key path <em>optional</em></span><input name="identityFile" value="${escapeHtml(current.identityFile)}" placeholder="~/.ssh/id_ed25519" /></label>
      <div class="credential-box">
        <label class="credential-toggle"><input name="storePassword" type="checkbox" ${current.hasSavedPassword ? "checked" : ""} /><span><strong>Save SSH password securely</strong><small>${current.hasSavedPassword ? "A password is stored in your OS credential vault." : "Protected by your OS credential vault, not Lantern’s files."}</small></span></label>
        <label class="password-field" ${current.hasSavedPassword ? "" : "hidden"}><span>${current.hasSavedPassword ? "Replace saved password" : "SSH password"} <em>${current.hasSavedPassword ? "leave blank to keep it" : ""}</em></span><input name="password" type="password" ${current.hasSavedPassword ? "" : "disabled required"} placeholder="${current.hasSavedPassword ? "Saved securely" : "Enter password"}" autocomplete="new-password" spellcheck="false" /></label>
      </div>
      <label><span>Tags <em>comma-separated</em></span><input name="tags" value="${escapeHtml(current.tags.join(", "))}" placeholder="home, production" /></label>
      <p class="form-help">Only SSH login passwords can be saved. Private-key passphrases continue to use your SSH agent or OpenSSH prompt.</p>
      <div class="modal-actions">${profile ? '<button type="button" class="danger" id="delete-profile">Delete</button>' : ""}<span></span><button type="button" class="secondary" id="cancel-modal">Cancel</button><button type="submit" class="primary">${connectAfterSave ? "Save & connect" : "Save profile"}</button></div>
    </form>
  </section></div>`;
  const close = () => { root.innerHTML = ""; };
  root.querySelector("#close-modal")?.addEventListener("click", close);
  root.querySelector("#cancel-modal")?.addEventListener("click", close);
  root.querySelector(".modal-backdrop")?.addEventListener("click", (event) => {
    if (event.target === event.currentTarget) close();
  });
  const passwordToggle = root.querySelector<HTMLInputElement>('input[name="storePassword"]');
  const passwordField = root.querySelector<HTMLElement>(".password-field");
  const passwordInput = root.querySelector<HTMLInputElement>('input[name="password"]');
  passwordToggle?.addEventListener("change", () => {
    if (!passwordField || !passwordInput) return;
    passwordField.hidden = !passwordToggle.checked;
    passwordInput.disabled = !passwordToggle.checked;
    passwordInput.required = passwordToggle.checked && !current.hasSavedPassword;
    if (!passwordToggle.checked) passwordInput.value = "";
    else passwordInput.focus();
  });
  root.querySelector("#delete-profile")?.addEventListener("click", async () => {
    state.profiles = await api.deleteProfile(current.id);
    close();
    await refreshStatuses();
    render();
  });
  root.querySelector<HTMLFormElement>("#profile-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const form = new FormData(event.currentTarget as HTMLFormElement);
    const storePassword = form.get("storePassword") === "on";
    const password = String(form.get("password") ?? "");
    const saved: SshProfile = {
      ...current,
      name: String(form.get("name") ?? "").trim(),
      host: String(form.get("host") ?? "").trim(),
      port: Number(form.get("port") ?? 22),
      username: String(form.get("username") ?? "").trim(),
      identityFile: String(form.get("identityFile") ?? "").trim() || null,
      tags: String(form.get("tags") ?? "").split(",").map((tag) => tag.trim()).filter(Boolean),
    };
    const targetChanged = current.username !== saved.username || current.host !== saved.host || current.port !== saved.port;
    if (storePassword && current.hasSavedPassword && targetChanged && !password) {
      passwordInput?.setCustomValidity("Re-enter the password after changing the username, host, or port.");
      passwordInput?.reportValidity();
      return;
    }
    try {
      state.profiles = await api.saveProfile(saved);
      if (storePassword && password) {
        await api.savePassword(saved.id, password);
        saved.hasSavedPassword = true;
      } else if (!storePassword && current.hasSavedPassword) {
        await api.deletePassword(saved.id);
        saved.hasSavedPassword = false;
      } else if (!targetChanged) {
        saved.hasSavedPassword = current.hasSavedPassword;
      }
      state.profiles = state.profiles.map((item) => item.id === saved.id ? { ...item, hasSavedPassword: saved.hasSavedPassword } : item);
      close();
      await refreshStatuses();
      render();
      if (connectAfterSave) await openTerminal(saved);
    } catch (error) {
      setError(error);
    }
  });
  passwordInput?.addEventListener("input", () => passwordInput.setCustomValidity(""));
  root.querySelector<HTMLInputElement>('input[name="name"]')?.focus();
}

function ensureTerminalWorkspace(): void {
  if (terminalRoot.querySelector(".terminal-workspace")) return;
  terminalRoot.innerHTML = `<section class="terminal-workspace" aria-label="SSH terminals">
    <header class="terminal-chrome">
      <div class="terminal-tabs" role="tablist" aria-label="Open terminals"></div>
      <button class="terminal-minimize" type="button" aria-label="Return to ${state.previousView}">${icon("minus", 17)}</button>
    </header>
    <div class="terminal-panes"></div>
  </section>`;
  terminalRoot.querySelector<HTMLButtonElement>(".terminal-minimize")?.addEventListener("click", () => {
    state.view = state.previousView;
    render();
  });
}

function terminalDeviceKey(profile: SshProfile): string {
  return `${profile.host.trim().toLowerCase()}:${profile.port}`;
}

function terminalBaseName(profile: SshProfile): string {
  const device = state.devices.find((item) => {
    const addresses = [item.ip, item.hostname].filter(Boolean).map((value) => value?.toLowerCase());
    return addresses.includes(profile.host.trim().toLowerCase());
  });
  return device ? deviceName(device) : profile.name;
}

function reserveTerminalOrdinal(deviceKey: string): number {
  const ordinal = (terminalSessionCounters.get(deviceKey) ?? 0) + 1;
  terminalSessionCounters.set(deviceKey, ordinal);
  terminalOpeningCounts.set(deviceKey, (terminalOpeningCounts.get(deviceKey) ?? 0) + 1);
  return ordinal;
}

function finishTerminalReservation(deviceKey: string): void {
  const remaining = (terminalOpeningCounts.get(deviceKey) ?? 1) - 1;
  if (remaining > 0) terminalOpeningCounts.set(deviceKey, remaining);
  else terminalOpeningCounts.delete(deviceKey);
  clearTerminalCounterIfUnused(deviceKey);
}

function clearTerminalCounterIfUnused(deviceKey: string): void {
  const stillOpen = [...terminalTabs.values()].some((tab) => tab.deviceKey === deviceKey);
  if (!stillOpen && !terminalOpeningCounts.has(deviceKey)) terminalSessionCounters.delete(deviceKey);
}

function syncTerminalWorkspaceVisibility(): void {
  terminalRoot.hidden = state.view !== "terminals" || terminalTabs.size === 0;
}

function refreshTerminalTabs(): void {
  ensureTerminalWorkspace();
  const tabList = terminalRoot.querySelector<HTMLDivElement>(".terminal-tabs");
  if (!tabList) return;
  tabList.innerHTML = "";
  terminalTabs.forEach((tab) => {
    const item = document.createElement("div");
    item.className = `terminal-tab-item${tab.sessionId === activeTerminalId ? " active" : ""}${tab.ended ? " ended" : ""}`;
    item.setAttribute("role", "presentation");

    const select = document.createElement("button");
    select.type = "button";
    select.className = "terminal-tab";
    select.setAttribute("role", "tab");
    select.setAttribute("aria-selected", String(tab.sessionId === activeTerminalId));
    select.title = `${tab.profile.username}@${tab.profile.host}:${tab.profile.port}`;
    select.innerHTML = '<span class="terminal-tab-status"></span><span class="terminal-tab-copy"><strong></strong><small></small></span>';
    const name = select.querySelector("strong");
    const target = select.querySelector("small");
    if (name) name.textContent = tab.displayName;
    if (target) target.textContent = `${tab.profile.username}@${tab.profile.host}`;
    select.addEventListener("click", () => activateTerminal(tab.sessionId));

    const close = document.createElement("button");
    close.type = "button";
    close.className = "terminal-tab-close";
    close.setAttribute("aria-label", `Close ${tab.displayName} terminal`);
    close.innerHTML = icon("x", 14);
    close.addEventListener("click", () => void closeTerminalTab(tab.sessionId));

    item.append(select, close);
    tabList.append(item);
  });
}

function activateTerminal(sessionId: string): void {
  const tab = terminalTabs.get(sessionId);
  if (!tab) return;
  activeTerminalId = sessionId;
  terminalTabs.forEach((item) => { item.host.hidden = item.sessionId !== sessionId; });
  refreshTerminalTabs();
  window.requestAnimationFrame(() => {
    try {
      tab.fit.fit();
      if (!tab.ended) void api.resizeSsh(tab.sessionId, tab.terminal.cols, tab.terminal.rows).catch(() => undefined);
    } catch {
      // The fit add-on can be briefly unavailable while the workspace is being revealed.
    }
    tab.terminal.focus();
  });
}

async function closeTerminalTab(sessionId: string): Promise<void> {
  const tab = terminalTabs.get(sessionId);
  if (!tab) return;
  const sessionIds = [...terminalTabs.keys()];
  const closedIndex = sessionIds.indexOf(sessionId);
  tab.cleanup();
  tab.observer.disconnect();
  tab.terminal.dispose();
  tab.host.remove();
  terminalTabs.delete(sessionId);
  clearTerminalCounterIfUnused(tab.deviceKey);

  if (activeTerminalId === sessionId) {
    const remainingIds = [...terminalTabs.keys()];
    activeTerminalId = remainingIds[Math.min(closedIndex, remainingIds.length - 1)] ?? null;
  }

  if (terminalTabs.size === 0) {
    activeTerminalId = null;
    terminalRoot.innerHTML = "";
    state.view = state.previousView;
    render();
  } else {
    render();
    if (activeTerminalId) activateTerminal(activeTerminalId);
  }

  try {
    if (!tab.ended) await api.closeSsh(sessionId);
    const updated = { ...tab.profile, lastConnectedAt: Date.now() };
    state.profiles = await api.saveProfile(updated);
  } catch (error) {
    setError(error);
  }
}

async function openTerminal(profile: SshProfile): Promise<void> {
  const available = await api.sshAvailable();
  if (!available) {
    setError("OpenSSH client was not found. Install the operating system’s OpenSSH client and try again.");
    return;
  }
  const [{ Terminal }, { FitAddon }] = await Promise.all([
    import("@xterm/xterm"),
    import("@xterm/addon-fit"),
  ]);
  ensureTerminalWorkspace();
  const panes = terminalRoot.querySelector<HTMLDivElement>(".terminal-panes");
  if (!panes) return;
  const host = document.createElement("div");
  host.className = "terminal-pane";
  host.hidden = true;
  panes.append(host);
  const terminal = new Terminal({
    cursorBlink: true,
    cursorStyle: "bar",
    fontFamily: '"SFMono-Regular", Consolas, "Liberation Mono", monospace',
    fontSize: 13,
    lineHeight: 1.22,
    scrollback: 5000,
    theme: { background: "#0b0f0e", foreground: "#d9e4df", cursor: "#87d6ad", selectionBackground: "#315442" },
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.open(host);

  const deviceKey = terminalDeviceKey(profile);
  const ordinal = reserveTerminalOrdinal(deviceKey);
  const baseName = terminalBaseName(profile);
  const displayName = ordinal === 1 ? baseName : `${baseName} (${ordinal})`;
  let reservationFinished = false;
  try {
    let ended = false;
    let closed = false;
    let sessionTab: TerminalTab | null = null;
    const sessionId = await api.startSsh(profile, 100, 30, (event) => {
      if (closed) return;
      if (event.kind === "data") terminal.write(event.data);
      if (event.kind === "error") terminal.writeln(`\r\n\u001b[31m${event.message}\u001b[0m`);
      if (event.kind === "exit") {
        ended = true;
        terminal.writeln(`\r\n\u001b[90mSession ended${event.code === null ? "" : ` (exit ${event.code})`}\u001b[0m`);
        if (sessionTab) {
          sessionTab.ended = true;
          refreshTerminalTabs();
        }
      }
    });
    let pending = "";
    let flushTimer = 0;
    const inputSubscription = terminal.onData((data) => {
      pending += data;
      if (!flushTimer) {
        flushTimer = window.setTimeout(() => {
          const input = pending;
          pending = "";
          flushTimer = 0;
          void api.writeSsh(sessionId, input).catch((error: unknown) => {
            if (!closed) setError(error);
          });
        }, 8);
      }
    });
    const observer = new ResizeObserver(() => {
      if (host.hidden) return;
      try {
        fit.fit();
        if (!ended) void api.resizeSsh(sessionId, terminal.cols, terminal.rows).catch(() => undefined);
      } catch {
        // Ignore transient measurements while changing tabs.
      }
    });
    observer.observe(host);
    const cleanup = () => {
      closed = true;
      inputSubscription.dispose();
      if (flushTimer) window.clearTimeout(flushTimer);
      pending = "";
    };
    sessionTab = { sessionId, profile, displayName, deviceKey, ordinal, terminal, fit, host, observer, ended, cleanup };
    terminalTabs.set(sessionId, sessionTab);
    finishTerminalReservation(deviceKey);
    reservationFinished = true;
    if (state.view !== "terminals") state.previousView = state.view as AppView;
    state.view = "terminals";
    activeTerminalId = sessionId;
    render();
    activateTerminal(sessionId);
  } catch (error) {
    if (!reservationFinished) finishTerminalReservation(deviceKey);
    terminal.dispose();
    host.remove();
    if (terminalTabs.size === 0) terminalRoot.innerHTML = "";
    setError(error);
  }
}

async function refreshStatuses(): Promise<void> {
  const statuses = await api.profileStatuses(state.profiles);
  state.statuses = new Map(statuses.map((status) => [status.id, status]));
}

async function init(): Promise<void> {
  render();
  try {
    [state.interfaces, state.profiles] = await Promise.all([api.interfaces(), api.loadProfiles()]);
    const rememberedNetwork = localStorage.getItem(NETWORK_STORAGE_KEY);
    state.selectedCidr = state.interfaces.some((item) => item.cidr === rememberedNetwork)
      ? rememberedNetwork ?? ""
      : state.interfaces[0]?.cidr ?? "";
    if (state.selectedCidr) localStorage.setItem(NETWORK_STORAGE_KEY, state.selectedCidr);
    render();
    await refreshStatuses();
    render();
  } catch (error) {
    setError(error);
  }
}

void init();

window.setInterval(() => {
  const modalOpen = Boolean(document.querySelector(".modal-backdrop"));
  if (state.view !== "profiles" || modalOpen || state.profiles.length === 0) return;
  void refreshStatuses()
    .then(render)
    .catch((error: unknown) => setError(error));
}, 20_000);

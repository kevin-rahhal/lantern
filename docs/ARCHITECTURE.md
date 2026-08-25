# Lantern architecture

Lantern is split into a small TypeScript view layer and a Rust application core. The priorities are low idle memory, fast startup, responsive interaction, straightforward native packaging, and honest network results.

```text
TypeScript + CSS UI
  ├─ progressive device list, search, filters, aliases
  ├─ saved profiles and reachability state
  └─ lazy xterm.js instances in compact tabs
                 │ typed Tauri commands and channels
Rust core
  ├─ discovery and multi-protocol enrichment
  ├─ JSON profile/alias/preferences persistence
  ├─ OS credential-vault adapter
  └─ OpenSSH processes in native pseudo-terminals
```

## UI boundary

`src/main.ts` owns presentation state and rendering. `src/api.ts` is the only bridge to native commands; it also provides deterministic sample data for browser-only interface work. The frontend cannot spawn programs or read arbitrary files.

The terminal emulator is loaded only when a session is opened. Each session has an independent xterm instance and PTY session ID, so multiple sessions to the same profile are isolated and numbered in the interface.

## Discovery pipeline

1. Enumerate non-loopback IPv4 interfaces and remember the user's last selection.
2. Keep automatic scans bounded; large networks default to the local `/24`, and manual ranges are capped at 1,024 candidates.
3. Probe candidates concurrently with short timeouts and publish progress events while the scan is running.
4. Merge the operating system ARP/neighbor cache with responding hosts to collect same-subnet MAC addresses.
5. Enrich candidates with reverse DNS, mDNS/DNS-SD, SSDP/UPnP descriptions, NetBIOS names, common service ports, and selected protocol banners.
6. Look up public MAC manufacturers from the packaged OUI database.
7. Score all available evidence to produce a best-effort device/OS label and confidence level.
8. Apply the user's persistent alias and mark devices that match saved SSH profiles.

Raw-packet fingerprinting is deliberately excluded: it would require elevated privileges, add deployment friction, and create a more intrusive scanner. Silent, sleeping, firewalled, routed, or privacy-randomized devices can remain unknown.

## Persistence and secrets

Profiles, personal device names, and UI preferences are stored in Tauri's per-user application-data directory as small JSON files. These files contain connection metadata but never saved passwords.

Optional SSH passwords are stored through `keyring-rs` in Windows Credential Manager, macOS Keychain, or Linux Secret Service. Credential entries are bound to the profile ID and connection target. Password strings are zeroized after retrieval where practical and are supplied only to an OpenSSH login-password prompt. Private-key passphrases remain the responsibility of the SSH agent or interactive OpenSSH prompt.

## SSH sessions

Each connection launches the operating system's `ssh` executable directly inside a native pseudo-terminal. Lantern streams output to xterm.js through a typed Tauri channel and sends batched keyboard input and resize events through narrow commands.

OpenSSH owns cryptography, host-key verification, `known_hosts`, configuration files, agents, and keys. Session shutdown treats an already-exited process as a successful close, avoiding a misleading error during normal teardown.

## Packaging

Tauri produces a per-user NSIS installer on Windows, DMGs on Apple silicon and Intel macOS, and AppImage plus Debian packages on Linux. GitHub Actions builds each target natively from a version tag and publishes the artifacts to a public release.

Release binaries are reproducible from the tag but are not yet signed with commercial platform certificates. Windows and macOS can therefore display an additional trust prompt. Signing can be added later by providing the platform credentials as encrypted repository secrets without changing application code.

## Project layout

```text
src/                             TypeScript UI, API adapter, styling
src-tauri/src/discovery.rs       discovery and identification pipeline
src-tauri/src/profiles.rs        profile, alias, and preference persistence
src-tauri/src/credentials.rs     operating-system credential vault
src-tauri/src/ssh.rs             PTY-backed OpenSSH lifecycle
src-tauri/src/lib.rs             narrow Tauri command surface
.github/workflows/ci.yml         source validation
.github/workflows/release.yml    native installer publication
```


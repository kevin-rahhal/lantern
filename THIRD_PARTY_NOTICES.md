# Third-party notices

Lantern is built with open-source software. The exact versions distributed in a release are recorded in `package-lock.json` and `src-tauri/Cargo.lock`.

## Major components

| Component | Purpose | License |
| --- | --- | --- |
| [Tauri](https://github.com/tauri-apps/tauri) | Lightweight desktop runtime and packaging | Apache-2.0 OR MIT |
| [xterm.js](https://github.com/xtermjs/xterm.js) | Embedded terminal renderer | MIT |
| [Tokio](https://github.com/tokio-rs/tokio) | Asynchronous networking runtime | MIT |
| [portable-pty](https://github.com/wezterm/wezterm) | Cross-platform pseudo-terminal support | MIT |
| [keyring-rs](https://github.com/hwchen/keyring-rs) | OS credential-vault integration | MIT OR Apache-2.0 |
| [mdns-sd](https://github.com/keepsimple1/mdns-sd) | mDNS/DNS-SD discovery | MIT OR Apache-2.0 |
| [macaddr-ouidb](https://crates.io/crates/macaddr-ouidb) | Offline MAC manufacturer lookup | Apache-2.0 |
| [TypeScript](https://github.com/microsoft/TypeScript) | Frontend language tooling | Apache-2.0 |
| [Vite](https://github.com/vitejs/vite) | Frontend build tooling | MIT |
| [esbuild](https://github.com/evanw/esbuild) | Cross-platform JavaScript compatibility transform | MIT |

The MAC manufacturer database packaged by `macaddr-ouidb` states that its source is Nmap's `nmap-mac-prefixes`, which in turn integrates the IEEE Registration Authority public assignment data. Credit belongs to the [IEEE Registration Authority](https://standards.ieee.org/products-programs/regauth/) and the [Nmap Project](https://nmap.org/). Lantern does not bundle or execute the Nmap scanner.

Other transitive dependencies use permissive licenses including MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0, CC0-1.0, 0BSD, and Unlicense. Their source distributions and license files are available from the package registries and upstream repositories identified by the two lockfiles.

OpenSSH is not bundled with Lantern. When you connect, Lantern launches the OpenSSH client installed by your operating system; that separate program remains governed by its own license.

Lantern's own source is licensed under the MIT License. See `LICENSE`.

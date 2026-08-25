# Contributing to Lantern

Thanks for helping make local-network work calmer and more approachable.

## Before you start

- Search existing issues before opening a new one.
- Use a feature request to discuss substantial behavior or interface changes first.
- Only scan networks and devices you own or are authorized to test.
- Never include passwords, private keys, public IP addresses, or identifying network captures in an issue.

## Development setup

Install Node.js 20.19 or newer, stable Rust, the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform, and an OpenSSH client.

```sh
npm ci
npm run desktop:dev
```

Before opening a pull request:

```sh
npm run check:version
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo check --manifest-path src-tauri/Cargo.toml
```

Keep changes focused, explain user-visible behavior, and add tests where practical. By contributing, you agree that your contribution is licensed under the project's MIT License.


# Arachnea Server

This directory is the Rust backend Cargo workspace. It contains the stream application backend, shared scraper primitives, DNS resolution, outbound HTTP helpers, and proxy routing crates.

## Crates

| Crate | Purpose |
| --- | --- |
| `arachnea-stream` | Backend executable crate, stream service facade, and Tauri application configuration/assets. |
| `arachnea-core` | Shared controller and persistence primitives, including generic REST and Tauri controller backends. |
| `arachnea-scrapyfy` | Generic YAML-driven scraper engine. |
| `arachnea-dns` | DNS resolver core with policy, cache, upstream transports, local records, blocklists, Smart DNS, DNSSEC-related features, recursive resolution, and optional UDP/TCP server support. |
| `arachnea-http` | Outbound HTTP client facade built on `rquest`, with optional Cloudflare-oriented engines such as Ghostwire, chaser-cf, and a Tauri/Wry solver path. |
| `arachnea-proxy` | Proxy routing core, proxy-chain support, outbound transports, egress pools, client connectors, and optional HTTP/HTTPS/SOCKS server listeners. |

## Runtime Assets

- `services/` contains source scraper configuration.
- `data/` contains local runtime data such as credentials.
- `crates/arachnea-stream/tauri.conf.json`, `crates/arachnea-stream/capabilities/`, and `crates/arachnea-stream/icons/` are used by the Tauri application build.
- `arachnea-stream` passes its Tauri context, embedded frontend assets, URI scheme, and API prefix into the generic Tauri controller from `arachnea-core`.
- DNS and proxy configuration examples live in `crates/arachnea-dns/config-sample/` and `crates/arachnea-proxy/config-sample/`.

## Install Prerequisites

Install Rust with `rustup`, Node.js with npm, Tauri's native platform requirements, and CMake. Reopen the shell after installing CMake and verify `cmake --version`.

Windows PowerShell:

```powershell
winget install --id Rustlang.Rustup -e
winget install --id Kitware.CMake -e
rustup default stable-msvc
```

Install Microsoft C++ Build Tools with `Desktop development with C++`. If `cmake` is not found after using the MSI installer, add `C:\Program Files\CMake\bin` to `PATH`. With Chocolatey, use `choco install cmake --installargs 'ADD_CMAKE_TO_PATH=System'`.

macOS:

```bash
xcode-select --install
brew install cmake
```

Linux Debian/Ubuntu:

```bash
sudo apt update
sudo apt install -y build-essential cmake pkg-config libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev
```

## Design Principles

- Keep reusable core logic independent from local server listeners.
- Keep shared crates independent from application crates; application-specific Tauri config and generated context stay in `arachnea-stream`.
- Make profiles expand into inspectable effective configuration instead of hidden behavior.
- Keep configuration types usable from code or `serde`; server crates may use TOML as their first file format.
- Preserve hostnames whenever the upstream protocol can carry them, and resolve locally only when required.
- Keep privacy, security, anti-censorship, and geolocation features explicit and composable rather than magical profile side effects.
- Avoid leaking secrets in logs, traces, errors, or documentation examples.
- Reject unsafe public proxy or recursive DNS exposure by default unless ACLs or explicit unsafe options are configured.

## Commands

- `cargo build` - Build the backend workspace.
- `cargo check --workspace` - Type-check the backend workspace.
- `cargo test --workspace` - Run existing backend tests.
- `cargo run -p arachnea-stream --bin arachnea` - Run the `arachnea` backend executable.
- `cd crates/arachnea-stream && cargo tauri build` - Build the Tauri desktop application and release bundles.
- `cargo build-doc` - Generate backend documentation without external dependencies.
- `cargo show-doc` - Generate backend documentation without external dependencies and open it in the browser.

## Cross-Platform Builds

Install only the targets you need:

```bash
rustup target add x86_64-pc-windows-msvc
rustup target add aarch64-pc-windows-msvc
rustup target add x86_64-apple-darwin aarch64-apple-darwin
rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
```

Useful checks from the workspace root:

```bash
cargo check -p arachnea-core --target x86_64-pc-windows-msvc
cargo check -p arachnea-stream --target x86_64-pc-windows-msvc
```

Tauri bundles are safest on native OS runners:

```bash
cd crates/arachnea-stream
cargo tauri build
cargo tauri build --target x86_64-pc-windows-msvc
cargo tauri build --target aarch64-pc-windows-msvc
cargo tauri build --target universal-apple-darwin
cargo tauri build --target x86_64-unknown-linux-gnu
```

For macOS/Linux to Windows MSVC Tauri builds, use Tauri's documented `cargo-xwin` runner:

```bash
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
cd crates/arachnea-stream
cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc
```

## DNS Quick Start

```powershell
cargo run -p arachnea-dns --example system_relay -- www.google.lu
cargo run -p arachnea-dns --features doh --example single_forwarder -- www.rtlplay.be
cargo run -p arachnea-dns --features doh,dnssec --example secure -- www.google.lu
cargo run -p arachnea-dns -- validate-config crates/arachnea-dns/config-sample/system_relay.toml
cargo run -p arachnea-dns -- serve --config crates/arachnea-dns/config-sample/system_relay.toml
```

## Proxy Quick Start

```powershell
cargo run -p arachnea-proxy --example tower_http_get -- example.com /
cargo run -p arachnea-proxy --example hyper_service -- example.com /
cargo run -p arachnea-proxy --features rquest --example rquest_loopback -- http://example.com/
cargo run -p arachnea-proxy -- validate-config crates/arachnea-proxy/config-sample/direct.toml
cargo run -p arachnea-proxy -- serve --config crates/arachnea-proxy/config-sample/direct.toml
```

The proxy binds to loopback by default. Public binds without ACLs are rejected unless `allow_unsafe_public_bind_without_acl = true` is configured explicitly.

## Documentation Map

- `README.md` gives the server workspace overview, shared commands, and quick starts.
- `TODO.md` tracks remaining DNS, HTTP, and proxy work imported into this workspace.
- `CHANGELOG.md` records meaningful server workspace changes.
- Crate `README.md` files hold crate-specific design context, capabilities, commands, and known gaps.
- Crate `AGENTS.md` files are AI-agent-only implementation instructions.

# Building Arachnea

This document covers build prerequisites, commands, and release packaging for
contributors. For a functional overview of the project, see the root
[`README.md`](../README.md).

## Project Structure

- `front/` - Vue.js frontend application.
- `server/` - Rust backend Cargo workspace.

## Rust Crates

- `arachnea-stream` (`server/crates/arachnea-stream/`) - Backend executable crate. It owns the `arachnea` binary, stream service logic, player resolvers, and Tauri application configuration/assets.
- `arachnea-core` (`server/crates/arachnea-core/`) - Shared backend primitives, including controller abstractions/backends and persistence/resource helpers. Its Tauri backend is parameterized by application-supplied context, assets, URI scheme, and API prefix.
- `arachnea-scrapyfy` (`server/crates/arachnea-scrapyfy/`) - Generic YAML-driven scraper engine, query models, actions, post-processors, and aggregation logic.
- `arachnea-dns` (`server/crates/arachnea-dns/`) - DNS resolver core with policies, cache, upstream transports, recursive resolution, and optional server support.
- `arachnea-http` (`server/crates/arachnea-http/`) - Outbound HTTP client facade with optional Cloudflare-oriented engines.
- `arachnea-proxy` (`server/crates/arachnea-proxy/`) - Proxy routing core, outbound transports, client connectors, and optional HTTP/HTTPS/SOCKS server listeners.

## Installation

Install Rust with `rustup`, Node.js with npm, and the platform build tools required by Tauri and native Rust dependencies. The workspace requires rustc >= 1.91.0 (locked `foyer@0.22.4+` dependency); run `rustup update` if the release tooling reports your toolchain as too old. After installation, open a new terminal and verify `rustc -V`, `cargo -V`, `node -v`, `npm -v`, and `cmake --version`.

Windows PowerShell:

```powershell
winget install --id Rustlang.Rustup -e
winget install --id Kitware.CMake -e
rustup default stable-msvc
```

Also install Microsoft C++ Build Tools with the `Desktop development with C++` workload. WebView2 is normally present on Windows 10 1803 and later, but install the Evergreen runtime if Tauri reports it missing. If CMake was installed with the MSI or Chocolatey instead of Winget, make sure `C:\Program Files\CMake\bin` is on `PATH`; the Chocolatey package supports `choco install cmake --installargs 'ADD_CMAKE_TO_PATH=System'`.

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

Use the official CMake download page if the distribution package is too old for a native dependency.

## Cargo Commands

The following commands are available for the Rust backend workspace in `server/`. Alias commands are defined in `server/.cargo/config.toml`.

- `cargo build` - Build the backend workspace.
- `cargo check --workspace` - Check the backend workspace.
- `cargo run -p arachnea-stream --bin arachnea` - Build and run the backend executable. Add `--server-public` to bind the server to `0.0.0.0` so the web interface and API are reachable through any local network hostname or IP (for example `http://localhost:8080/`).
- `cargo run -p arachnea-dns -- validate-config crates/arachnea-dns/config-sample/system_relay.toml` - Validate a DNS configuration sample.
- `cargo run -p arachnea-proxy -- validate-config crates/arachnea-proxy/config-sample/direct.toml` - Validate a proxy configuration sample.
- `cd server/crates/arachnea-stream && cargo tauri build` - Build the Tauri desktop application and generate release bundles.
- `cargo build-doc` - Generate backend documentation without external dependencies.
- `cargo show-doc` - Generate backend documentation without external dependencies and open it in the browser.

## Cross-Platform Build Commands

Rust cross-compilation starts by installing the target standard library

```bash
rustup target add <target-triple>
```

Then build with the Tauri CLI from `server/crates/arachnea-stream`:

```bash
cd server/crates/arachnea-stream
cargo tauri build
cargo tauri build --target x86_64-pc-windows-msvc
cargo tauri build --target aarch64-pc-windows-msvc
cargo tauri build --target universal-apple-darwin
cargo tauri build --target x86_64-unknown-linux-gnu
```

For macOS/Linux to Windows MSVC Tauri builds, use Tauri's documented `cargo-xwin` runner instead of plain `cargo tauri build --target ...`; plain cross-builds can fail in the Windows resource step, for example with a missing `llvm-rc`.

```bash
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
cd server/crates/arachnea-stream
cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc
```

## Local Cross-Platform Release Build

A Node-based release builder lives in `build-release/` and drives the Tauri CLI for every platform the current host can produce locally, without Docker — see `build-release/README.md` for the full documentation. It reads project paths and the platform list from `build-release/release-config.json`, resolves the version from `server/Cargo.toml`, builds with `cargo tauri build`, then assembles a `releases/release-<VERSION>/` folder with version-named installers, companion `*.sha256`/`*.md5` checksum files, the project CHANGELOG at its root, and a portable Windows `.zip` (executable + `services/`) for Windows platforms.

```bash
node build-release/install-tools.mjs                # install rust targets + cargo-xwin (+ Linux packages)
node build-release/release.mjs                      # build all platforms available on this host
node build-release/release.mjs -p windows            # build one family (osx = alias of darwin)
node build-release/release.mjs -p "darwin-*"         # build by wildcard pattern
node build-release/release.mjs -p darwin-universal -p windows   # several selectors (repeat or comma)
node build-release/release.mjs --list                # show which platforms this host can build
node build-release/release.mjs --version             # print the project version
node build-release/release.mjs --skip-build          # assemble only, reusing existing target/ artifacts
```

`-p/--platform` accepts exact ids (`darwin-arm64`), family names (`darwin`, `windows`, `linux`), `*` patterns (`darwin-*`), and the alias `osx` (maps to `darwin`). Multiple selectors can be repeated (`-p darwin -p windows-*`) or comma-separated (`-p "darwin,windows"`). Platform subfolders are named after the family part: `darwin-*` → `osx`, `windows-*` → `windows`, `linux-*` → `linux` (mapping centralized in `build-release/capabilities.mjs`).

The frontend is built **once** and shared by all targets: each `cargo tauri build` invocation is passed a `--config` override that disables `beforeBuildCommand` (which stays active for manual `cargo tauri build` runs outside this tooling). Use `--no-frontend-build` to reuse an existing `front/dist`. Every installer and portable Windows zip gets companion `*.sha256` and `*.md5` checksum files (formats `sha256sum`/`md5sum`). Only the outputs being rebuilt are cleaned: rebuilding a whole family (e.g. `-p osx`) clears that family folder, while rebuilding a single platform (e.g. `-p darwin-x86_64`) removes only that build's artifacts and checksums, leaving other platforms' files untouched.

Cross-compiling Windows NSIS installers from macOS/Linux requires a few native tools that `install-tools` provisions automatically: LLVM (`llvm-rc`, for cargo-xwin), CMake + Ninja + NASM (for BoringSSL-based dependencies), and `makensis` — for the latter a `makensis.exe` shim wrapping the native compiler is created in `~/.arachnea-cross-tools/bin/` because the Tauri NSIS bundler looks for the Windows-style executable name; spawned builds automatically get this directory on their PATH.

Example output layout:

```
releases/release-0.1.0/
  CHANGELOG.md
  osx/        arachnea_0.1.0_universal.dmg
  windows/    arachnea_0.1.0_x64-setup.exe
              arachnea_0.1.0_x64-portable.zip   # arachnea.exe + services/
  linux/      arachnea_0.1.0_amd64.deb
```

The cross-compilation capability matrix (which bundles each host can produce) is encoded in `build-release/capabilities.mjs`:

| Host \ Target bundle | macOS `.dmg` | Windows NSIS | Windows MSI | Linux `.deb`/`.rpm`/`.AppImage` |
|---|---|---|---|---|
| macOS | native | via `cargo-xwin` | not possible (WiX) | not possible |
| Windows | not possible | native | native | not possible (use WSL2) |
| Linux | not possible | via `cargo-xwin` | not possible (WiX) | native |

The Windows portable archive is the only generated zip; it ships the release executable plus the runtime `services/` folder read by the app in release mode, excluding local-only state such as `credentials.json` and the cache. The `data/` folder is not included and is created at runtime. The installer bundles (NSIS `.exe`, `.msi`, `.deb`, `.rpm`, `.AppImage`, `.dmg`) also embed the same `services/` folder through the Tauri `bundle.resources` setting; on Windows it is installed next to the executable (the application root in release mode), while on Linux/macOS it lands in the platform resource directory.

## NPM Commands

The following commands are available for the frontend project in `front/`.

- `npm install` - Install frontend dependencies.
- `npm run dev` - Start the Vite development server.
- `npm run build` - Run type checks and build the production bundle.
- `npm run preview` - Preview the production build locally.
- `npm run build-only` - Build the production bundle with Vite.
- `npm run type-check` - Run Vue TypeScript type checking.
- `npm run lint:css` - Run Stylelint on CSS and Vue style blocks.
- `npm run lint:css:fix` - Run Stylelint with automatic fixes.

## References

- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
- [Tauri CLI build options](https://v2.tauri.app/reference/cli/)
- [Tauri Windows installer and cargo-xwin notes](https://v2.tauri.app/distribute/windows-installer/)
- [Rustup cross-compilation](https://rust-lang.github.io/rustup/cross-compilation.html)
- [CMake downloads](https://cmake.org/download/)
- [Homebrew CMake formula](https://formulae.brew.sh/formula/cmake.html)
- [Winget install command](https://learn.microsoft.com/en-us/windows/package-manager/winget/install)

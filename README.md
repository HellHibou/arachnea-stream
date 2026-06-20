# Arachnea

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

Install Rust with `rustup`, Node.js with npm, and the platform build tools required by Tauri and native Rust dependencies. After installation, open a new terminal and verify `rustc -V`, `cargo -V`, `node -v`, `npm -v`, and `cmake --version`.

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
- `cargo run -p arachnea-stream --bin arachnea` - Build and run the backend executable.
- `cargo run -p arachnea-dns -- validate-config crates/arachnea-dns/config-sample/system_relay.toml` - Validate a DNS configuration sample.
- `cargo run -p arachnea-proxy -- validate-config crates/arachnea-proxy/config-sample/direct.toml` - Validate a proxy configuration sample.
- `cd server/crates/arachnea-stream && cargo tauri build` - Build the Tauri desktop application and generate release bundles.
- `cargo build-doc` - Generate backend documentation without external dependencies.
- `cargo show-doc` - Generate backend documentation without external dependencies and open it in the browser.

## Cross-Platform Build Commands

Rust cross-compilation starts by installing the target standard library, but `rustup target add` does not install the platform linker, C/C++ compiler, SDK, WebView, or CMake. Prefer native OS builders for release installers; use cross-compilation mainly for checks or when Tauri documents a supported runner.

Common target setup:

```bash
rustup target add x86_64-pc-windows-msvc
rustup target add aarch64-pc-windows-msvc
rustup target add x86_64-apple-darwin aarch64-apple-darwin
rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
```

Backend checks:

```bash
cd server
cargo check -p arachnea-core --target x86_64-pc-windows-msvc
cargo check -p arachnea-stream --target x86_64-pc-windows-msvc
```

Desktop bundles:

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

## Skills

The following Codex skills are provided by this project.

- `#build-yaml-source` - Create or update one Arachnea scraper YAML from one or more source URLs by analyzing the target website, reusing patterns from `server/services/*.yaml`, and checking supported scraper capabilities in `server/crates/arachnea-scrapyfy/src/scrapyfy/*`.
- `#update-readme` - Maintain all Arachnea README.md files by updating Cargo commands, NPM commands, crate summaries, and the list of project-related Codex skills.

## References

- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
- [Tauri CLI build options](https://v2.tauri.app/reference/cli/)
- [Tauri Windows installer and cargo-xwin notes](https://v2.tauri.app/distribute/windows-installer/)
- [Rustup cross-compilation](https://rust-lang.github.io/rustup/cross-compilation.html)
- [CMake downloads](https://cmake.org/download/)
- [Homebrew CMake formula](https://formulae.brew.sh/formula/cmake.html)
- [Winget install command](https://learn.microsoft.com/en-us/windows/package-manager/winget/install)

# Arachnea Release Builder

Local, zero-dependency Node tooling that builds the Arachnea Tauri application for every platform the current host can produce, without relying on CI. It drives the Tauri CLI (`cargo tauri build`) for installers, assembles versioned output folders under `releases/`, and provisions the native toolchain required for cross-compilation. When the host cannot natively bundle a Linux or macOS target (e.g. building on Windows or macOS), it can additionally build through a small Docker image (`docker.mjs`): a **portable** raw binary for every Docker-capable target, plus the Linux `.deb`/`.rpm`/`.AppImage` installers produced inside the container (one pass per bundle type; the `.AppImage` needs FUSE for linuxdeploy, unavailable under Docker Desktop).

## Files

| File | Role |
|---|---|
| `release-config.json` | Release configuration: project paths (including `frontendProject`), platform ids, Rust target triples, requested bundles, and portable archive config (`.zip` for Windows, `.tar.gz` for Linux/macOS). Single source of truth for what gets built. |
| `capabilities.mjs` | Cross-compilation capability matrix per host (which bundles each OS can actually produce), family/output-folder naming, selector aliases, architecture short names. |
| `lib.mjs` | Shared helpers: config loading, workspace version parsing, platform resolution, artifact discovery, checksums, zip/tar.gz creation. |
| `install-tools.mjs` | Installs everything required to build on this host (see below). |
| `release.mjs` | The builder CLI: builds platforms, assembles `releases/release-<VERSION>/`, writes checksums. |
| `docker.mjs` | Docker cross-build orchestration: locates/builds the Arachnea cross image and produces the `docker run` commands that compile raw Linux/macOS release binaries — and bundle the Linux installers through the in-image Tauri CLI — when the host cannot do it natively. |
| `docker/Dockerfile` | The cross image (derived from `joseluisq/rust-linux-darwin-builder`): adds the WebKitGTK 4.1 stack required to compile the Tauri app for `*-unknown-linux-gnu` targets, plus the Tauri CLI for in-image installer bundling (prebuilt binary on amd64, compiled from crates.io on arm64). |
| `package.json` | npm script aliases for the commands above. |

## Version source

The release version is read once from the `[workspace.package]` section of `server/Cargo.toml`. Keep it in sync with `tauri.conf.json`; every installer name and the output folder derive from it.

## Usage

```bash
node build-release/install-tools.mjs                # provision tools for all locally buildable platforms
node build-release/install-tools.mjs -p windows-*   # provision tools for matching platforms only

node build-release/release.mjs                      # build every platform available on this host
node build-release/release.mjs -p windows           # one family
node build-release/release.mjs -p "darwin-*"        # wildcard pattern
node build-release/release.mjs -p darwin-arm64      # exact platform id
node build-release/release.mjs -p osx -p linux-x86_64   # several selectors (repeat or comma)

node build-release/release.mjs --list               # show platforms + what this host can produce
node build-release/release.mjs --version            # print the project version
node build-release/release.mjs --skip-build         # assemble only, reusing existing target/ artifacts
node build-release/release.mjs --no-frontend-build  # reuse the configured frontend dist/ instead of building it
node build-release/release.mjs --continue-on-error   # continue after a platform build failure
node build-release/release.mjs --no-install         # skip tool provisioning/verification
node build-release/release.mjs --dry-run            # print the planned production (incl. Docker cross builds) and exit
```

Quote wildcard selectors (`-p "darwin-*"`): unquoted `*` is expanded by the shell.

When a required tool or Rust target is missing, `install-tools` asks for confirmation before installing it. This also applies when `release.mjs` invokes tool provisioning automatically; in a non-interactive terminal, the release stops rather than installing software without confirmation.

## Platform selectors

Accepted by `-p/--platform` in both scripts:

- exact platform id from `release-config.json`: `darwin-arm64`, `windows-x86_64`, ...
- family name (part before the first dash): `darwin`, `windows`, `linux`
- alias: `osx` (= `darwin`)
- wildcard pattern: `darwin-*`, `windows-*`, ...

## Output layout

Artifacts are grouped by family subfolder; each installer and every portable archive get companion `*.sha256` and `*.md5` files (`sha256sum`/`md5sum` format). `CHANGELOG.md` is copied at the release folder root only.

```
releases/release-0.1.0/
  CHANGELOG.md
  osx/        arachnea_0.1.0_universal.dmg (+ .sha256 / .md5)
              arachnea_0.1.0_x64-portable.tar.gz (+ .sha256 / .md5)   # via Docker cross-build
              arachnea_0.1.0_arm64-portable.tar.gz (+ .sha256 / .md5) # via Docker cross-build
  windows/    arachnea_0.1.0_x64-setup.exe (+ .sha256 / .md5)
              arachnea_0.1.0_x64-portable.zip (+ .sha256 / .md5)   # arachnea.exe + services/
  linux/      arachnea_0.1.0_amd64-portable.tar.gz (+ .sha256 / .md5)   # via Docker cross-build
              arachnea_0.1.0_arm64-portable.tar.gz (+ .sha256 / .md5)   # via Docker cross-build
```

### Cleanup behavior

Only what a run rebuilds is removed beforehand:

- rebuilding a full family (`-p osx`) clears that whole family folder;
- rebuilding a single platform (`-p darwin-x86_64`) removes only that build's artifacts and checksums, leaving other platforms' files untouched.

## Frontend builds

The frontend project path comes from `frontendProject` in `release-config.json`. It is compiled **once** per run and shared by every target: each `cargo tauri build` gets a `--config` override disabling `beforeBuildCommand` (which stays active for manual `cargo tauri build` runs outside this tooling). Bundle types are also passed through the same `--config` merge because the CLI restricts its `--bundles` flag to a host-dependent value list.

## Failure handling

By default, a platform build failure stops the release build. Pass `--continue-on-error` to continue with the remaining platforms instead. The final summary lists attempted targets with `✓` for success and `✗` for failure. Any failed target returns a non-zero process exit code, including when subsequent targets completed successfully.

## Cross-compilation matrix

Native installers per bundle type:

| Host \ Bundle | macOS `.dmg` | Windows NSIS | Windows MSI | Linux `.deb`/`.rpm`/`.AppImage` |
|---|---|---|---|---|
| macOS | native | via `cargo-xwin` | not possible (WiX) | via the Docker cross image |
| Windows | not possible | native | native | via the Docker cross image |
| Linux | not possible | via `cargo-xwin` | not possible (WiX) | native |

## Portable binaries and Linux installers via Docker

When a target cannot be produced natively on the current host, the builder
cross-compiles it inside the Arachnea Docker image (`docker.mjs` +
`docker/Dockerfile`, derived from `joseluisq/rust-linux-darwin-builder`):

- **Every Docker-capable target** keeps shipping the raw executable packaged as
  a portable `.tar.gz` (Windows portable `.zip` is host-only).
- **Linux targets additionally get their installers** (`.deb`, `.rpm`,
  `.AppImage`) bundled inside the container through the in-image Tauri CLI:
  one `cargo tauri build --target <triple>` pass **per bundle type** produces
  the release binary and one installer (the Tauri bundler crashes when asked
  for several types in a single process). The `.deb`/`.rpm` bundlers are pure
  Rust, and the AppImage one runs linuxdeploy with `--appimage-extract-and-run` +
  linuxdeploy downloads cached in `<workspace target>/.tauri-bundle-cache/<arch>/`.
  Each bundle is attempted independently: if one cannot be produced (e.g. the
  `.AppImage` under Docker Desktop, whose missing `/dev/fuse` breaks linuxdeploy
  internal sub-processes), the others and the portable archive are still shipped
  with a warning. AppImage builds work on a real Linux host or a Linux CI
  container.
- `darwin-x86_64` and `darwin-arm64`: compiled with osxcross; no installer is
  possible from a container (only the portable binary).

A platform is produced this way whenever `release-config.json` sets
`"build": "docker"` and declares a `portable` block and/or Linux bundles, and
the current host cannot natively produce them. With
`--force-use-docker-builder`, Docker-capable platforms go through the image
even when the host could build them natively. `--list` shows the chosen method
(`native`/`docker`), and `--dry-run` prints the exact `docker run` commands
without executing them.

The Docker path assumes Docker is available; run
`node build-release/install-tools.mjs` to build the cross image (asks for
confirmation, as it pulls the large Rust + osxcross base image). Note that
`.deb`/`.rpm`/`.AppImage` outputs link against the glibc/WebKitGTK versions of
the Debian base image, so they target distributions of same-or-newer vintage.

> **Portable caveats.** A Linux portable binary still links against
> `libwebkit2gtk-4.1`/GTK present on the machine that runs it — prefer the
> `.AppImage` (self-contained) when that matters. macOS portables are unsigned
> (an `aarch64-apple-darwin` binary must be ad-hoc signed before it will run,
> and downloaded files may trip Gatekeeper).

## What `install-tools` provisions

- rustup targets for every selected platform.
- `cargo-xwin` when a Windows target is built from a non-Windows host.
- LLVM tools (`clang`, `lld-link`, `llvm-rc`) through Homebrew on macOS or apt on Linux for cargo-xwin.
- CMake, Ninja and NASM — required by BoringSSL-based dependencies (`cmake -G Ninja`, `ASM_NASM`).
- `makensis`, plus a `makensis.exe` shim in `~/.arachnea-cross-tools/bin/` wrapping the native compiler: the Tauri NSIS bundler looks for the Windows-style executable name even on non-Windows hosts. Spawned builds automatically prepend that directory to PATH.
- Linux system packages (`libwebkit2gtk-4.1-dev`, ...) when running on Linux.
- The Arachnea Docker cross image (installs the WebKitGTK stack and the Tauri
  CLI on top of `joseluisq/rust-linux-darwin-builder`) whenever a Linux/macOS
  portable binary or a Linux installer bundle is selected on a host that cannot
  produce it natively.

## Portable archives

Two portable formats, each getting `.sha256`/`.md5` checksums:

- **Windows `.zip`**: the release `arachnea.exe` plus the runtime `services/`
  folder read by the app in release mode (the executable directory is the
  application root), excluding local-only state such as `credentials.json` and
  caches. The `data/` folder is created at first launch and is not shipped.
- **Linux/macOS `.tar.gz`**: the raw `arachnea` binary built through the Docker
  cross image (see "Portable binaries via Docker" above). It does not bundle the
  `services/` runtime data.

## Installer resources

The installer bundles (NSIS `.exe`, `.msi`, `.deb`, `.rpm`, `.AppImage`, `.dmg`)
embed the `services/` runtime folder through the Tauri `bundle.resources`
setting in `server/crates/arachnea-stream/tauri.conf.json`
(`"../../../server/services": "services/"`). On Windows the resources are
installed next to the executable, which is the application root the app reads
in release mode. On Linux/macOS they land in the platform resource directory
(`/usr/lib/...`, `.app/Contents/Resources`, ...), which the exe-directory
application root does not read — the portable archives remain the way to get a
self-contained app on those platforms.

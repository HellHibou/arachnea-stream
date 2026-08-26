# Arachnea Release Builder

Local, zero-dependency Node tooling that builds the Arachnea Tauri application for every platform the current host can produce, without Docker. It drives the Tauri CLI (`cargo tauri build`), assembles versioned output folders under `releases/`, and provisions the native toolchain required for cross-compilation.

## Files

| File | Role |
|---|---|
| `release-config.json` | Release configuration: project paths (including `frontendProject`), platform ids, Rust target triples, requested bundles, and portable-zip config. Single source of truth for what gets built. |
| `capabilities.mjs` | Cross-compilation capability matrix per host (which bundles each OS can actually produce), family/output-folder naming, selector aliases, architecture short names. |
| `lib.mjs` | Shared helpers: config loading, workspace version parsing, platform resolution, artifact discovery, checksums, zip creation. |
| `install-tools.mjs` | Installs everything required to build on this host (see below). |
| `release.mjs` | The builder CLI: builds platforms, assembles `releases/release-<VERSION>/`, writes checksums. |
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

Artifacts are grouped by family subfolder; each installer and the Windows portable zip get companion `*.sha256` and `*.md5` files (`sha256sum`/`md5sum` format). `CHANGELOG.md` is copied at the release folder root only.

```
releases/release-0.1.0/
  CHANGELOG.md
  osx/        arachnea_0.1.0_universal.dmg (+ .sha256 / .md5)
              arachnea_0.1.0_arm64.dmg     (+ .sha256 / .md5)
              arachnea_0.1.0_x64.dmg       (+ .sha256 / .md5)
  windows/    arachnea_0.1.0_x64-setup.exe (+ .sha256 / .md5)
              arachnea_0.1.0_x64-portable.zip (+ .sha256 / .md5)   # arachnea.exe + services/
  linux/      arachnea_0.1.0_amd64.deb     (+ .sha256 / .md5)
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

| Host \ Bundle | macOS `.dmg` | Windows NSIS | Windows MSI | Linux `.deb`/`.rpm`/`.AppImage` |
|---|---|---|---|---|
| macOS | native | via `cargo-xwin` | not possible (WiX) | not possible |
| Windows | not possible | native | native | not possible (use WSL2) |
| Linux | not possible | via `cargo-xwin` | not possible (WiX) | native |

## What `install-tools` provisions

- rustup targets for every selected platform.
- `cargo-xwin` when a Windows target is built from a non-Windows host.
- LLVM tools (`clang`, `lld-link`, `llvm-rc`) through Homebrew on macOS or apt on Linux for cargo-xwin.
- CMake, Ninja and NASM — required by BoringSSL-based dependencies (`cmake -G Ninja`, `ASM_NASM`).
- `makensis`, plus a `makensis.exe` shim in `~/.arachnea-cross-tools/bin/` wrapping the native compiler: the Tauri NSIS bundler looks for the Windows-style executable name even on non-Windows hosts. Spawned builds automatically prepend that directory to PATH.
- Linux system packages (`libwebkit2gtk-4.1-dev`, ...) when running on Linux.

## Windows portable archive

The only generated zip: the release `arachnea.exe` plus the runtime `services/` folder read by the app in release mode (the executable directory is the application root), excluding local-only state such as `credentials.json` and caches. The `data/` folder is created at first launch and is not shipped.

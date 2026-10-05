# Arachnea Release Builder

Local, zero-dependency Node tooling that builds the Arachnea Tauri application for every platform the current host can produce, without relying on CI. It drives the Tauri CLI (`cargo tauri build`) for installers, assembles versioned output folders under `releases/`, and provisions the native toolchain required for cross-compilation. When the host cannot natively bundle a Linux or macOS target (e.g. building on Windows or macOS), it can additionally build through a small Docker image (`docker.mjs`): a portable archive for every Docker-capable target, plus the Linux `.deb`/`.rpm`/`.AppImage` installers produced inside the container (one pass per bundle type; the `.AppImage` needs FUSE for linuxdeploy, unavailable under Docker Desktop). macOS targets additionally get a separate `-app.tar.gz` archive embedding a launchable `.app` bundle.

## Files

| File | Role |
|---|---|
| `release-config.json` | Release configuration: project paths (including `frontendProject`), platform ids, Rust target triples, requested bundles, and portable archive config (`.zip` for Windows, `.tar.gz` for Linux/macOS). Single source of truth for what gets built. |
| `build-config.json` | Build tool versions: minimum rustc (`minRustcVersion`), cross image tag (`crossImage`), in-image Tauri CLI (`tauriCliVersion`). Single source of truth for the `ensureRustcVersion()` guard (offers `rustup update` with confirmation when outdated) and the Docker image build. |
| `capabilities.mjs` | Cross-compilation capability matrix per host (which bundles each OS can actually produce), family/output-folder naming, selector aliases, host OS+CPU to platform id mapping (`hostPlatformSelector`), architecture short names. |
| `lib.mjs` | Shared helpers: config loading, workspace version parsing, platform resolution, artifact discovery, checksums, zip/tar.gz creation. |
| `install-tools.mjs` | Installs everything required to build on this host (see below). |
| `release.mjs` | The builder CLI: builds platforms, assembles `releases/release-<VERSION>/`, writes checksums. |
| `docker.mjs` | Docker cross-build orchestration: locates/builds the Arachnea cross image and produces the `docker run` commands that compile raw Linux/macOS release binaries — and bundle the Linux installers through the in-image Tauri CLI — when the host cannot do it natively. |
| `docker/Dockerfile` | The cross image (derived from `joseluisq/rust-linux-darwin-builder:1.89.0`, Debian 12 Bookworm): adds the WebKitGTK 4.1 stack required to compile the Tauri app for `*-unknown-linux-gnu` targets, plus the Tauri CLI for in-image installer bundling (prebuilt binary on amd64, compiled from crates.io on arm64). The 1.x base keeps the bundled crypto stack (libgcrypt/libgpg-error) compatible with older distros such as Linux Mint; the 2.x line (Debian 13 Trixie) produces AppImages failing with `undefined symbol: gpgrt_add_post_log_func`. |
| `package.json` | npm script aliases for the commands above. |

## Version source

The release version is read once from the `[workspace.package]` section of `server/Cargo.toml`. Keep it in sync with `tauri.conf.json`; every installer name and the output folder derive from it.

## Usage

```bash
node build-release/install-tools.mjs                # provision tools for every platform this host can build
node build-release/install-tools.mjs "windows-*"    # provision tools for matching platforms only

node build-release/release.mjs                      # build this host OS + CPU platform only
node build-release/release.mjs windows              # one family
node build-release/release.mjs "darwin-*"           # wildcard pattern
node build-release/release.mjs darwin-arm64         # exact platform id
node build-release/release.mjs osx linux-x86_64     # several targets (space- or comma-separated)
node build-release/release.mjs "*"                  # every configured platform

node build-release/release.mjs --list               # show platforms + what this host can produce
node build-release/release.mjs --version            # print the project version
node build-release/release.mjs --skip-build         # assemble only, reusing existing target/ artifacts
node build-release/release.mjs --no-frontend-build  # reuse the configured frontend dist/ instead of building it
node build-release/release.mjs --continue-on-error   # continue after a platform build failure
node build-release/release.mjs --no-install         # skip tool provisioning/verification
node build-release/release.mjs --dry-run            # print the planned production (incl. Docker cross builds) and exit
node build-release/release.mjs windows-* --package portable # produce only these packages for a family
node build-release/release.mjs --package "deb,portable"     # --package: repeatable and/or comma-separated
```

Quote wildcard selectors (`"darwin-*"`, `"*"`): unquoted `*` is expanded by the shell.
The repository-root `./build.sh` wrapper also recognizes the complete filename
list produced by an unquoted `*`, so both `./build.sh *` and `./build.sh "*"`
select every configured platform.

When a required tool or Rust target is missing, `install-tools` asks for confirmation before installing it. This also applies when `release.mjs` invokes tool provisioning automatically; in a non-interactive terminal, the release stops rather than installing software without confirmation.

## Platform selectors

Platform selectors are plain positional arguments in both scripts (`-p` and
`--platform` were removed); several targets may be listed, space- or
comma-separated (`release.mjs "darwin-*" windows-x86_64`). Accepted forms:

- exact platform id from `release-config.json`: `darwin-arm64`, `windows-x86_64`, ...
- family name (part before the first dash): `darwin`, `windows`, `linux`
- alias: `osx` (= `darwin`)
- wildcard pattern: `darwin-*`, `windows-*`, ...
- `*`: every configured platform

The defaults differ on purpose:

- `release.mjs` with no target builds the platform matching the **host OS and
  CPU** only (`windows-x86_64` on a 64-bit Windows host, `darwin-arm64` on
  Apple Silicon, `linux-arm64` on 64-bit ARM Linux, ...). Pass `"*"` to build
  everything this host can produce, or list the other targets explicitly.
- `install-tools.mjs` with no target provisions every platform this host can
  build (its historical default), so `node build-release/install-tools.mjs`
  still builds the Docker cross image.

Unknown selectors abort the run with `No platform matches: ...`. `-p` and
`--platform` now fail with an explicit message pointing at the new syntax.

## Package selection

By default, every package declared by each selected platform is produced
(installer bundles from `bundles`, plus the portable archive and, on macOS
targets, the `-app.tar.gz` archive). Pass `--package <name>` to narrow a run:

- repeatable and/or comma-separated: `release.mjs windows-* --package portable`,
  `release.mjs --package deb --package rpm`, `release.mjs --package "deb,portable"`;
- known names come from `release-config.json` (`dmg`, `nsis`, `msi`, `deb`,
  `rpm`, `appimage`) plus `portable` (portable archive) and `app` (macOS `.app`
  archive); `all` and `*` select every declared package (the default);
- an unknown name aborts with `Unknown package: ...` and the list of known
  names; a package no selected target can produce is reported as a warning;
- a platform left with nothing to produce is skipped without compiling;
- the production method stays based on the declared bundles: selecting
  `portable` alone does not turn a natively packaged platform into a Docker
  (or unbuildable) one;
- `--list` (`produces: ...`) and `--dry-run` (`packages=[...]`) reflect the
  selection.


## Output layout

Artifacts are grouped by family subfolder; each installer and every portable archive get companion `*.sha256` and `*.md5` files (`sha256sum`/`md5sum` format). `CHANGELOG.md` is copied at the release folder root only.

```
releases/release-0.1.0/
  CHANGELOG.md
  osx/        arachnea_0.1.0_universal.dmg (+ .sha256 / .md5)
              arachnea_0.1.0_x64-portable.tar.gz (+ .sha256 / .md5)   # via Docker cross-build
              arachnea_0.1.0_x64-app.tar.gz (+ .sha256 / .md5)        # Arachnéa.app bundle
              arachnea_0.1.0_arm64-portable.tar.gz (+ .sha256 / .md5) # via Docker cross-build
              arachnea_0.1.0_arm64-app.tar.gz (+ .sha256 / .md5)      # Arachnéa.app bundle
  windows/    arachnea_0.1.0_x64-setup.exe (+ .sha256 / .md5)
              arachnea_0.1.0_x64-portable.zip (+ .sha256 / .md5)   # arachnea.exe + services/
  linux/      arachnea_0.1.0_amd64-portable.tar.gz (+ .sha256 / .md5)   # via Docker cross-build
              arachnea_0.1.0_arm64-portable.tar.gz (+ .sha256 / .md5)   # via Docker cross-build
```

### Cleanup behavior

Only what a run rebuilds is removed beforehand:

- rebuilding a full family (`release.mjs osx`) clears that whole family folder;
- rebuilding a single platform (`release.mjs darwin-x86_64`) removes only that build's artifacts and checksums, leaving other platforms' files untouched.
- when `--package` narrows the run, a family folder is never cleared as a whole: only the selected packages' artifacts (and their checksums) are removed, so packages left out stay in place.

## Frontend builds

The frontend project path comes from `frontendProject` in `release-config.json`. It is compiled **once** per run and shared by every target: each `cargo tauri build` gets a `--config` override disabling `beforeBuildCommand` (which stays active for manual `cargo tauri build` runs outside this tooling). Before that shared build, `install-tools` ensures each workspace (`front/`, `front/public-app`, `front/admin-app`) has its `node_modules`: the app build scripts spawn their local `run-p` (from `npm-run-all2`), so a checkout without dependencies stops with `'run-p' is not recognized as an internal or external command`. Skipped with `--no-install`, `--skip-build` or `--no-frontend-build`. Bundle types are also passed through the same `--config` merge because the CLI restricts its `--bundles` flag to a host-dependent value list. Linux installer bundles (`*-unknown-linux-gnu` targets) additionally merge `productName` from `release-config.json` (`arachnea`) through the same `--config` override: the Tauri Debian bundler copies `productName` verbatim into the control `Package:` field, which rejects the accented `Arachnéa` from `tauri.conf.json` (`dpkg` only allows `[a-z0-9+.-]`) and stages resources under `/usr/lib/Arachnéa/`. macOS/Windows builds keep the `tauri.conf.json` display name untouched.

## Failure handling

By default, a platform build failure stops the release build. Pass `--continue-on-error` to continue with the remaining platforms instead. The final summary lists attempted targets with `✓` for success and `✗` for failure, and success lines append the packages that platform produced (`packages=[...]`, same names as `--dry-run`). Any failed target returns a non-zero process exit code, including when subsequent targets completed successfully.

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

- **Every Docker-capable target** keeps shipping a portable `.tar.gz` holding
  the raw executable (Windows portable `.zip` is host-only). macOS targets
  additionally get a separate `-app.tar.gz` archive embedding a
  `<productName>.app` bundle (see "Portable archives" below).
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
- `darwin-x86_64` and `darwin-arm64`: compiled with osxcross. No installer can
  be produced from the container — the Tauri CLI ignores macOS bundle types on
  a Linux host (`Wrong package type app for platform Linux`), so the `.app` is
  assembled host-side by `release.mjs` into its own archive. Assembly prefers
  the executable produced by the container (`target/docker-build/<arch>/…`)
  so a stale host-built binary in the plain target dir cannot shadow it.

A platform is produced this way whenever `release-config.json` sets
`"build": "docker"` and declares a `portable` block and/or Linux bundles, and
the current host cannot natively produce them. With
`--force-use-docker-builder`, Docker-capable platforms go through the image
even when the host could build them natively. `--list` shows the chosen method
(`native`/`docker`), and `--dry-run` prints the exact `docker run` commands
without executing them. The workspace requires rustc >= 1.91.0 (locked
`foyer@0.22.4+` dependency); both entry points fail fast with an actionable
error when the toolchain is older — run `rustup update`, then rebuild. When
`cargo` is missing entirely, both entry points instead offer to install Rust
through the official rustup script (`curl ... https://sh.rustup.rs | sh -s --
-y`, `wget` fallback) after a `[y/N]` confirmation, then continue in the same
run (`~/.cargo/bin` is added to the process PATH); refusing, a non-interactive
terminal, or a Windows host keep the plain "install Rust first" error, Rust on
Windows being installed through `winget` as documented in `server/README.md`.

The Docker path only covers platforms that resolve to `method: docker`; a run
whose platforms all build natively (e.g. the default `linux-x86_64` build on a
Linux host) never needs Docker. When a cross build does require it and the
`docker` CLI is missing on Linux, the tooling offers to install Docker Engine
itself after a `[y/N]` confirmation (official `get.docker.com` script through
`sudo`), adds the user to the `docker` group and re-launches the run through
`sg docker` so the new membership applies without logging out; Windows/macOS
keep the explicit "install Docker" error (Docker Desktop). To build the cross
image, run `node build-release/install-tools.mjs` (asks for confirmation, as it
pulls the large Rust + osxcross base image). The image is
built **for both architecture ports** (`linux/amd64` and `linux/arm64`)
through `docker buildx build --platform linux/amd64,linux/arm64`, so any Linux
target can run right away. Keeping both variants under the single
`arachnea-cross-builder:<version>` tag requires the Docker **containerd image
store** (Docker Desktop: "Use containerd for pulling and storing images");
with the classic image store only one variant can exist per tag, and the
tooling falls back to building just the missing variant with an explicit
warning. The first build is slow: the arm64 half compiles its Tauri CLI under
QEMU emulation (30-60 min, cached afterwards). If a variant is still missing
at build time, `release.mjs` offers to build it on the spot. Note that
`.deb`/`.rpm`/`.AppImage` outputs link against the glibc/WebKitGTK versions of
the Debian base image, so they target distributions of same-or-newer vintage.

> **Portable caveats.** A Linux portable binary still links against
> `libwebkit2gtk-4.1`/GTK present on the machine that runs it — prefer the
> `.AppImage` (self-contained) when that matters. macOS portables are unsigned
> (an `aarch64-apple-darwin` binary must be ad-hoc signed before it will run,
> and downloaded files may trip Gatekeeper).

## What `install-tools` provisions

- The Rust toolchain itself when `cargo` is missing: offered with a `[y/N]`
  confirmation through the official rustup installer on Linux/macOS (same
  guard in `release.mjs`, before any build step).
- Docker Engine on Linux when — and only when — a selected platform is
  produced through the cross image (`method: docker`): installed with the
  standard `[y/N]` confirmation via the official `get.docker.com` script,
  including `docker` group membership. Runs whose platforms all resolve to the
  native method never touch Docker.
- rustup targets for every selected platform.
- The host Tauri CLI (`cargo tauri`) at the `tauriCliVersion` from
  `build-config.json` (same version as the cross image) whenever a selected
  platform builds natively: offered with the standard `[y/N]` confirmation via
  `cargo install --locked tauri-cli@<version>` when missing or outdated, since
  `cargo tauri build` fails with `error: no such command: 'tauri'` without the
  `cargo-tauri` binary (Docker-built platforms use the in-image CLI instead).
- `cargo-xwin` when a Windows target is built from a non-Windows host.
- LLVM tools (`clang`, `lld-link`, `llvm-rc`) through Homebrew on macOS or apt on Linux for cargo-xwin.
- CMake, Ninja, NASM, Go, Perl and a C++ compiler — required by BoringSSL-based dependencies: BoringSSL's CMake runs `find_package(Perl REQUIRED)` and generates `err_data.c` through `go run`, NASM supplies the x86 assembly on Windows, and `cmake -G Ninja` drives the build. Installed through Homebrew on macOS and apt on Linux; on Windows through winget (`Kitware.CMake`, `Ninja-build.Ninja`, `NASM.NASM`, `GoLang.Go`, `StrawberryPerl.StrawberryPerl`) after the standard `[y/N]` confirmation, elevated through a UAC prompt when the shell is not administrator, with the resulting directories added to this run's PATH (Strawberry Perl's `c\bin` is left out — the GCC it ships makes the build misdetect the compiler). A `ninja`/`cl.exe` shipped inside Visual Studio counts as present, since CMake and rustc locate those through the VS installation; any tool left missing is reported with the matching winget id. The known install directories (Go, Strawberry `perl\bin`, NASM, CMake, `ninja-build`, winget's `Links` shims) are prepended to the run's PATH **before** that check, because the installers only refresh the *machine* PATH: a terminal started before them (VS Code's integrated shell) would otherwise report the tools as missing on every run and ask to install them again, and the build itself would not find `perl`/`go` either.
- `makensis`, plus a `makensis.exe` shim in `~/.arachnea-cross-tools/bin/` wrapping the native compiler: the Tauri NSIS bundler looks for the Windows-style executable name even on non-Windows hosts. Spawned builds automatically prepend that directory to PATH.
- The frontend workspace dependencies (`npm install` in `front/`, `front/public-app` and `front/admin-app`) whenever the run builds the frontend: the app build scripts spawn the local `run-p` binary from their `npm-run-all2` dependency, so a checkout without `node_modules` failed with `'run-p' is not recognized as an internal or external command`. A single `[y/N]` confirmation covers all pending workspaces, and workspaces whose `node_modules` is already up to date (lockfile marker newer than `package.json`) are skipped. Skipped entirely with `--skip-build`/`--no-frontend-build`.
- Linux system packages (`libwebkit2gtk-4.1-dev`, `libssl-dev`, `git`, `clang`, ...) when running on Linux: `dpkg-query` is checked first, so a run where everything is already installed skips the `sudo`/`apt-get update` round-trip (password prompt) instead of reinstalling idempotently.
- cargo's target directory: when `server/target/` sits on an unreliable mount (VirtualBox `vboxsf`, VMware `vmhgfs`, NFS/CIFS/SSHFS, exFAT/vFAT/NTFS — detected with `stat -f -c %T`), the run offers to redirect to `~/.cache/arachnea-target` via `CARGO_TARGET_DIR` (standard `[y/N]` confirmation). Build-script outputs written into such mounts silently vanish (`serde_core` `OUT_DIR/private.rs` missing even after `cargo clean`), so building there can never succeed; an explicit `CARGO_TARGET_DIR` is always respected.
- The Arachnea Docker cross image (installs the WebKitGTK stack and the Tauri
  CLI on top of `joseluisq/rust-linux-darwin-builder`) whenever a Linux/macOS
  portable binary or a Linux installer bundle is selected on a host that cannot
  produce it natively. The image is built for both architecture ports
  (`linux/amd64` + `linux/arm64`) through buildx, which requires the Docker
  containerd image store to keep both variants under the tag.

## Portable archives

Two portable formats, each getting `.sha256`/`.md5` checksums:

- **Windows `.zip`**: the release `arachnea.exe` plus the runtime `services/`
  folder read by the app in release mode (the executable directory is the
  application root), excluding local-only state such as `credentials.json` and
  caches. The `data/` folder is created at first launch and is not shipped.
- **Linux `.tar.gz`**: the raw `arachnea` binary built through the Docker cross
  image, plus the runtime `services/` folder next to it (the executable
  directory is the application root).
- **macOS `.tar.gz`**: same layout as Linux — the raw `arachnea` binary plus
  `services/` next to it.
- **macOS `-app.tar.gz`**: a second archive holding only an `Arachnéa.app`
  bundle (name read from `tauri.conf.json`) staged host-side by `release.mjs`
  with the same layout as the tauri-bundler `app` bundle —
  `Contents/Info.plist` (identifier, version, icon), `PkgInfo`,
  `Contents/MacOS/arachnea`, `Contents/Resources/{icon.icns, services/}`.
  The `services/` runtime folder is staged in `Contents/Resources/`, the
  Tauri `bundle.resources` location, which the application resource root
  resolution probes (`arachnea-core::application`). Writable `data/` lands in
  the per-user standard directory on packaged installs (`~/Library/Application
  Support/hell-hibou.arachnea` on macOS). The `.app` is unsigned
  (ad-hoc signing is still required on Apple Silicon before it runs, and
  downloaded files may trip Gatekeeper); the `.dmg` produced on a real macOS
  host remains the fully bundled alternative. A `.app` is a directory, not an
  executable: launch it with `open 'Arachnéa.app'` (or double-click), or run
  the inner binary directly (`Arachnéa.app/Contents/MacOS/arachnea`).

The POSIX modes of every `.tar.gz` are normalized after archiving
(`normalizeTarGzModes` in `lib.mjs`). TAR paths are compared as NFC-normalized
UTF-8 so the accented macOS app bundle name is recognized whether the host
filesystem records it in composed or decomposed Unicode form. Windows has no
permission bits, so the `bsdtar` shipped with it records every entry as
`0666`/`0777` — which produced a
**non-executable** `arachnea` in archives built on Windows: `arachnea-docker`'s
`test -x` aborted the image build, and anyone extracting the archive on Linux
got a binary that refused to run. Windows' `bsdtar` also rejects `--mode`, so
the tooling rewrites the modes inside the archive itself: files to `0644`,
directories to `0755`, and the release executable (plus
`Arachnéa.app/Contents/MacOS/arachnea` for the macOS `-app.tar.gz`) to `0755` —
the values macOS and GNU tar record, so the archive no longer depends on the
host that produced it. The builder stops with an explicit error when the
expected executable has no matching entry in the archive.

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

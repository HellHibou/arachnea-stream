# Arachnea Stream — Docker runtime

Multi-arch (`linux/amd64` + `linux/arm64`) runtime image for the prebuilt
Linux `arachnea` server binary. The image is kept as small as possible
(`debian:trixie-slim`, one `apt` layer, no build toolchain) and ships a
usable Chromium used by the `chaser-cf` Cloudflare solver, plus a single Xvfb
virtual display for headed Chromium on headless servers.

The `arachnea-stream` binary intentionally compiles the browser-backed
`chaser-cf` solver without the Ghostwire smart solver. This keeps automatic
Cloudflare handling on the `rquest` → Chromium path and avoids executing
Ghostwire in the long-lived server process.

One `Dockerfile` covers both architectures: Buildx injects `TARGETARCH` and
the portable archive declared for that port is copied in and extracted. Both
archives are read from the project's release folder (`<project>/releases`) via
the `releases` named build context, at paths expanded from a single
`ARACHNEA_VERSION` build arg — see [Release tree](#1-release-tree) and
[Build arguments](#build-arguments).

## Layout

```text
arachnea-docker/                    # main build context
  Dockerfile              # multi-arch runtime image (amd64 + arm64)
  docker-entrypoint.sh    # one Xvfb + CHROME_BIN wiring, then `arachnea`
  docker-compose.yml      # ready-to-run example (volume + port)
  build-image.sh          # version-resolving wrapper for `docker compose build`
  build-image.cmd         # same wrapper for Windows (cmd.exe)
  README.md               # this file
  .dockerignore           # keeps the build context minimal

<releases dir>/                     # `releases` named build context
  release-<version>/linux/arachnea-<version>-linux-amd64-portable.tar.gz  # ARACHNEA_AMD64_ARCHIVE
  release-<version>/linux/arachnea-<version>-linux-arm64-portable.tar.gz  # ARACHNEA_ARM64_ARCHIVE
```

The release tree is the `releases/` folder produced by
`node build-release/release.mjs` (Docker cross-build); each portable archive
contains the `arachnea` binary plus its `services/` YAML folder. It sits
outside `arachnea-docker/`, and Buildx refuses to `COPY` across build
contexts, so it travels as a *named* context (`releases`) instead of a path
inside the main context.

## 1. Release tree

Nothing is staged into `arachnea-docker/`: the archives are read straight from
the project's release folder (`<project>/releases`, output of
`node build-release/release.mjs`). `docker-compose.yml` declares it as a named
build context whose root comes from `ARACHNEA_RELEASES_DIR` in
`arachnea-docker/.env` (committed), relative to the compose file so it follows
the repository wherever it is checked out:

```yaml
additional_contexts:
  releases: "${ARACHNEA_RELEASES_DIR}"   # ../releases in .env = <project>/releases
```

`ARACHNEA_RELEASES_DIR` overrides that path when the archives live elsewhere
(already-built artifacts on a build host, CI cache, ...) — an environment
variable wins over `.env`:

```bash
ARACHNEA_RELEASES_DIR=/srv/arachnea/releases docker compose build
```

Expected layout inside that folder (this is what the two archive build args
expand to, with `<version>` = `ARACHNEA_VERSION`):

```text
releases/
  release-<version>/linux/arachnea-<version>-linux-amd64-portable.tar.gz
  release-<version>/linux/arachnea-<version>-linux-arm64-portable.tar.gz
```

## 2. Build (multi-arch)

A multi-arch manifest needs a `buildx` builder (Docker Desktop ships one):

```bash
# Local single-arch image (loads into `docker images`). The examples below run
# from the repository root, where the release folder is `./releases` (Compose
# resolves its own `../releases` relative to `arachnea-docker/`):
docker buildx build --platform linux/$(uname -m | sed 's/x86_64/amd64/;s/aarch64/arm64/') \
  --build-context releases="${ARACHNEA_RELEASES_DIR:-$PWD/releases}" \
  -t arachnea-stream:0.1.0 ./arachnea-docker --load

# Multi-arch manifest (both ports at once) — `docker compose build` declares
# the same named context and the same ARACHNEA_VERSION:
docker buildx build --platform linux/amd64,linux/arm64 \
  --build-context releases="${ARACHNEA_RELEASES_DIR:-$PWD/releases}" \
  --build-arg ARACHNEA_VERSION=0.1.0 \
  -t arachnea-stream:0.1.0 ./arachnea-docker --push  # or --load on containerd stores
```

The `releases` context is not optional: without it, `COPY --from=releases`
makes Buildx try to pull an image literally named `releases` and fail.
```

Or with Compose:

```bash
cd arachnea-docker
docker compose build
docker compose up -d
```

`docker compose build` tags the result from `ARACHNEA_VERSION` (same source as the
build args: `arachnea-docker/.env` or the environment), producing both
`arachnea-stream:<version>` and the `latest` alias. The versioned tag is also
the service `image:`, so `docker compose up` runs exactly the release just built.

Notes on multi-arch builds:

- Loading *both* platforms into the local image store requires the containerd
  image store (Docker Desktop default). On the classic overlay2 store, limit
  the build to the host platform (`--platform linux/$(uname -m | sed
  's/x86_64/amd64/;s/aarch64/arm64/')`) or push the manifest to a registry.
- Cross-building the foreign port goes through QEMU emulation (`binfmt`), so
  the first build is noticeably slower; the result is identical.
- A single `--platform linux/amd64,linux/arm64 ... --load` invocation produces
  one tag holding both variants (verified: `docker run --platform linux/amd64`
  reports `x86_64`, `--platform linux/arm64` reports `aarch64`). This needs the
  containerd image store; on the classic overlay2 store, `--load` only keeps
  one platform, so use `--push` with a registry instead.
- **Emulated amd64 cannot execute Chromium in this environment.** On Apple
  Silicon, Docker Desktop runs `linux/amd64` through Rosetta 2 (visible in the
  container: every process appears as `/rosetta/rosetta <binary> ...`).
  x86_64 Chromium refuses to start there: it pops a dialog through `xmessage`
  reading *"The hardware on this system lacks support for the sse3 instruction
  set. The upstream chromium project no longer supports this configuration"*
  (<http://crbug.com/1123353>). The emulated `/proc/cpuinfo` also keeps the host ARM feature
  list (`asimd`, `cpuid`, ...) with no `sse3`. This is an emulation limitation,
  not an image defect: the Dockerfile therefore only checks that
  `/usr/bin/chromium` and `/usr/bin/Xvfb` exist instead of executing the
  browser at build time. The `arachnea` server, the healthcheck and the whole
  REST API work on amd64 under Rosetta (verified: `GET /`, `GET
  /api/load_home` and `GET /api/search?query="test"` all answer `HTTP 200`);
  validate the amd64 *browser* path used by the `chaser-cf` solver on real
  x86_64 hardware or an amd64 CI runner.
- **One Xvfb only:** the entrypoint owns `DISPLAY=:99` and starts Xvfb before
  `arachnea`. The image defaults `CHASER_VIRTUAL_DISPLAY=0`, causing
  `chaser-cf` to launch its headed Chromium in that existing display. This is
  intentional: with `CHASER_VIRTUAL_DISPLAY=1`, chaser-cf always selects a
  free display from `:99` onward, starts a second Xvfb and changes `DISPLAY`
  itself. That duplicate display was unnecessary and made Chrome startup
  harder to diagnose. Set `CHASER_VIRTUAL_DISPLAY=1` only when chaser-cf must
  own the display; the entrypoint then skips its Xvfb.
- **Stale X lock purge:** on (re)start the entrypoint only reuses the display
  when an Xvfb process is actually alive (PID from the lock file, or a
  command-line scan). A stale `/tmp/.X99-lock` left by a dead Xvfb (`/tmp`
  survives `docker restart` while every process is gone) is removed with its
  socket and a fresh server is started, otherwise headed Chromium would
  inherit a dead `DISPLAY`. A warning is logged when the fresh server fails
  to come up.
- **Line endings on Windows checkouts:** the entrypoint is copied verbatim
  from the build context, so a CRLF working-tree copy ships a `#!/bin/sh\r`
  shebang and the container dies in a tini restart loop (`exec
  /usr/local/bin/docker-entrypoint.sh failed: No such file or directory`).
  The root `.gitattributes` pins `*.sh`/`Dockerfile` to LF on every checkout
  (`*.cmd`/`*.bat` stay CRLF), and the `Dockerfile` additionally strips a
  trailing CR from the entrypoint's first line at `COPY` time, so working
  trees checked out before that pinning still build a working image.

### Build scripts

`build-image.sh` (macOS/Linux) and `build-image.cmd` (Windows `cmd.exe`) wrap
`docker compose build` and resolve the release version to package:

```bash
cd arachnea-docker
./build-image.sh                  # highest release-* folder of the release tree
./build-image.sh --version=0.2.0  # one explicit release
./build-image.sh --no-cache       # force a full rebuild of every layer
```

```bat
build-image.cmd --version=0.2.0
```

`--version=<version>` (or `--version <version>`) sets `ARACHNEA_VERSION`.
Without it the script scans the release tree (`../releases`, or
`ARACHNEA_RELEASES_DIR` from the environment or `.env`, with the same
precedence as Compose) and picks the highest `release-*` folder — `sort -V` on
Unix, a `[version]` sort in PowerShell on Windows — so `0.9.0` < `0.10.0` <
`0.11.0`. The resolved version is exported for the build (the environment wins
over `.env`, so the image tag, the two archive paths and the stamped
`ENV ARACHNEA_VERSION` all follow it) and the two Linux portable archives are
checked first, so a missing release fails in the script with the expected path
instead of inside Buildx (`failed to compute cache key`). When neither
`--version` nor a `release-*` folder is available, the script stops with an
explicit error. The default `docker compose build` reuses the layer cache; any
other argument is forwarded to it, so `--no-cache` forces a full rebuild of
every layer instead of reusing a layer cached from a previous release
(`--progress=plain` or `--push` change the output). `-h`/`--help` prints the
usage. The script ends by printing the version-pinned command that starts the
image it just built (`ARACHNEA_VERSION=<version> docker compose up -d`, or
`set "ARACHNEA_VERSION=<version>" && docker compose up -d` on Windows).

### Build arguments

| Argument | Default | Effect |
|---|---|---|
| `ARACHNEA_VERSION` | **required** — `arachnea-docker/.env` (committed) or the environment | Release packaged into the image: it is expanded into the two archive paths below and stamped as `ENV ARACHNEA_VERSION` (logged by the entrypoint). Bumping a release only changes that value — an environment variable wins (`ARACHNEA_VERSION=0.2.0 docker compose build`). |
| `ARACHNEA_RELEASES_DIR` | `../releases` — `arachnea-docker/.env` or the environment | Root of the release tree, i.e. the `releases` named build context (`additional_contexts`); relative to the compose file. Environment wins (`ARACHNEA_RELEASES_DIR=/srv/arachnea/releases`). |
| `ARACHNEA_AMD64_ARCHIVE` | `release-${ARACHNEA_VERSION}/linux/arachnea-${ARACHNEA_VERSION}-linux-amd64-portable.tar.gz` | `linux/amd64` portable archive, relative to the `releases` build context. |
| `ARACHNEA_ARM64_ARCHIVE` | `release-${ARACHNEA_VERSION}/linux/arachnea-${ARACHNEA_VERSION}-linux-arm64-portable.tar.gz` | Same for `linux/arm64`. |

`docker-compose.yml` declares all three as build args — `ARACHNEA_VERSION` and
the two archive paths above — with **no default**: the version lives in exactly
one place, `arachnea-docker/.env` (committed, template in `.env.example`) or
the environment, and the two paths expand from it automatically (the same
`.env` also carries `ARACHNEA_RELEASES_DIR`, root of the `releases` context).
Compose fails fast with an explicit message when a variable is missing. Notes:

- The release tree is not part of the main context, so it must be passed as a
  named context: `additional_contexts.releases` in Compose,
  `--build-context releases=<dir>` on the CLI. An in-context `../releases/...`
  path or a symlink out of the context is rejected by Buildx.
- Both archives must exist even when a single port is built
  (`--platform linux/arm64`, or a reduced `platforms:` list), because the
  Dockerfile performs one `COPY` per port. The archive that is not needed by
  the port being built only lands in the builder cache, never in the image.
- Each `COPY` targets an explicit destination
  (`/tmp/bin/arachnea-<arch>-portable.tar.gz`), so the two archives do not need
  to share a file name; override `ARACHNEA_AMD64_ARCHIVE` /
  `ARACHNEA_ARM64_ARCHIVE` (e.g. through Compose `args`) for another naming.
- A misspelled path or a missing `release-<version>` folder fails immediately
  with `failed to compute cache key: "...": not found`. The extracted binary is
  then checked against the port being built through its ELF `e_machine` header
  (`3e00` x86_64, `b700` AArch64), so swapping the two args aborts the build
  instead of producing an image that dies later with `exec format error`.

## 3. Run

```bash
# Compose (recommended: persists /app/data, maps 8080, sets the seccomp
# option Chromium's sandbox needs — see the notes below):
cd arachnea-docker
docker compose up -d
open http://localhost:8080/

# Plain Docker:
docker run -d --name arachnea \
  -p 8080:8080 \
  -v arachnea-data:/app/data \
  --shm-size=256m \
  --security-opt seccomp=unconfined \
  arachnea-stream:0.1.0
```

Notes:

- `--shm-size=256m` (or the `shm_size:` Compose key) avoids Chromium tab
  crashes on the default 64M `/dev/shm`; the entrypoint also injects
  `--disable-dev-shm-usage` as a safety net.
- `--security-opt seccomp=unconfined` (`security_opt:` in the Compose file) is
  required for Chromium to start. Docker's default seccomp profile blocks the
  `clone`/`unshare` syscalls Chromium uses to create its user-namespace
  sandbox, so the browser aborts with
  `No usable sandbox! ... you can try using --no-sandbox` and `chaser-cf` fails
  every Cloudflare fetch with
  `Failed to initialize browser: Browser process exited with status
  ExitStatus(unix_wait_status(256))`. Unconfining seccomp restores a *working*
  sandbox, which is preferable to `--no-sandbox`: the browser keeps its
  process isolation, and the container's other hardening (non-root
  `arachnea` user, dropped capabilities, AppArmor, masked `/proc` paths) is
  unaffected. No `--privileged` and no host-side change is needed — this is
  what makes the image portable across Docker Desktop hosts (verified on
  Windows/WSL2 and Apple Silicon), unlike a host-side
  `kernel.unprivileged_userns_clone` toggle or a custom AppArmor profile, both
  of which are unsettable inside WSL2's managed VM anyway.
- State persists in `/app/data` (config, caches, Cloudflare session store):
  the data root is the executable directory in the portable layout, so the
  volume must be mounted there (override with `ARACHNEA_DATA_DIR` and mount
  the same path).
- The entrypoint pins the port and network mode in
  `$ARACHNEA_DATA_DIR/config.json` (`server_port`, `network_mode`) because
  `arachnea`'s argument parser compares option names only, so **any** value on
  the command line is rejected (`--server-port 8080` -> `unknown argument:
  8080`). Arguments placed after the image name are forwarded verbatim and
  must therefore be valueless flags such as `--no-tray` / `--server`.

### Network mode and published ports

`ARACHNEA_NETWORK` maps to the controller's client ACL
(`local` / `private` / `public`):

- `local` binds loopback only, so every request arriving through a published
  port answers `404`.
- `private` accepts peers whose address belongs to one of the container's own
  interface networks. Requests forwarded by the Docker host are *not*
  guaranteed to match (verified on Docker Desktop: `curl http://localhost:<published>/`
  answered `404`, while the same request issued from a sibling container on the
  same bridge answered `200`).
- `public` accepts any peer; it is the image default so `docker run -p ...`
  works out of the box.

To keep the service private while using `public`, bind the published port to
loopback (`-p 127.0.0.1:8080:8080`) and/or place the container behind a
reverse proxy. Use `private` only for container-to-container traffic on a
shared Docker network.

## Configuration

| Variable | Default | Effect |
|---|---|---|
| `ARACHNEA_PORT` | `8080` (`.env`) | REST server port. Drives both sides of the published port, the `server_port` pinned in `config.json`, and the healthcheck probe. |
| `ARACHNEA_NETWORK` | `public` (`.env`) | `local` (loopback) \| `private` (container networks) \| `public` (`--network`). Kept in `.env` because it must stay consistent with the published `ports:`. |
| `ARACHNEA_ROOT` | empty (`.env`) | Public URL prefix (no slashes, e.g. `arachnea`) behind a path-based reverse proxy: pinned as `entrypoint_root` in `config.json`, the app is served from `/<prefix>/`. Empty = served from `/`. |
| `ARACHNEA_DATA_DIR` | `/app/data` | Writable state directory (config + persistence; also `HOME`). |
| `CHROME_BIN` | `/usr/bin/chromium` | Chromium binary used by `chaser-cf`. |
| `CHASER_VIRTUAL_DISPLAY` | `0` | The entrypoint starts Xvfb on `DISPLAY=:99`; chaser-cf launches headed Chromium there. Set to `1` only to let chaser-cf create and own a separate Xvfb; the entrypoint then skips its own. |
| `CHASER_EXTRA_ARGS` | `--disable-dev-shm-usage` (ensured) | Extra Chromium flags, appended to the engine defaults (`--disable-gpu` for GPU-less hosts is the common one). The entrypoint appends `--disable-dev-shm-usage` when the value does not already contain it. |
| `DISPLAY` | `:99` | X display started by the entrypoint, unless `CHASER_VIRTUAL_DISPLAY=1` or `DISPLAY_SKIP_XVFB` is set. |

Docker option (Compose key, no environment variable):

| Option | Value | Effect |
|---|---|---|
| `security_opt` | `seccomp=unconfined` | Lets Chromium build its user-namespace sandbox; without it the browser refuses to start under Docker's default seccomp profile. See the run notes above. |

Extra arguments after the image name are forwarded to `arachnea`, e.g.
`docker run arachnea-stream --help`.

`ARACHNEA_PORT`, `ARACHNEA_NETWORK` and `ARACHNEA_ROOT` come from `.env`
(same fail-fast rule as the build variables) and are written to the persisted
`config.json` on every start, so changing them and restarting the container
is enough (the entrypoint also honours an already existing config file and
only overrides those three keys; an empty `ARACHNEA_ROOT` removes
`entrypoint_root`). `ARACHNEA_PORT` also drives the published `ports:` mapping,
so a port change moves both sides together.

## Image contents

- Base: `debian:trixie-slim` (glibc >= 2.38 required by the prebuilt binary,
  `TARGETARCH`-native).
- One `apt` layer: `chromium`, `xvfb`, `tini`, `jq`, `ca-certificates`,
  `libwebkit2gtk-4.1-0`, `libgtk-3-0`, `libayatana-appindicator3-1`,
  `libssl3` — the exact runtime `NEEDED` set of the `arachnea` binary plus
  the browser/display stack. No Node, no Rust, no X11 server beyond Xvfb.
  `bash`, `grep` and `awk` come from the Debian base and are not extra
  packages.
- App: `/app/arachnea` + `/app/services` (portable layout), non-root user.
- Healthcheck: `/bin/bash` HTTP probe of `127.0.0.1:$ARACHNEA_PORT` (no
  `curl`/`wget`; the base `/bin/sh` is `dash` and has no `/dev/tcp`), so the
  check stays valid in every network mode.

### Size profile

The runtime image is ~1.6 GB uncompressed (~0.6 GB compressed), dominated by
components that cannot be dropped without losing functionality:

| Component | Installed size | Why it stays |
|---|---|---|
| `chromium` + `chromium-common` | ~380 MB | the browser `chaser-cf` drives. |
| `libllvm19` + `mesa-libgallium` | ~150 MB | X11 GL stack: `Xvfb` exposes GLX and headed Chromium in virtual-display mode uses it. Removing the pair would break the solver path, so it is kept on purpose. |
| `libwebkit2gtk-4.1-0` + GTK/GStreamer/flite tree | ~250 MB | `NEEDED` libraries of the prebuilt `arachnea` binary (Tauri/Wry runtime). |
| `debian:trixie-slim` | ~117 MB | glibc >= 2.38 floor. |

A future `arachnea` release built without the Tauri GUI dependencies
(`--no-default-features`-style server build) would allow dropping the whole
WebKit/GTK tree, which is the only remaining large win.

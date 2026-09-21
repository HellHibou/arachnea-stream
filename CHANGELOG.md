# Changelog

All notable changes to the server workspace are recorded here.
## Unreleased

### <u>build-release</u>

#### Added

#### Changed

- Release tooling versions are now centralized in `build-release/build-config.json` (minimum rustc, cross image tag, in-image Tauri CLI) instead of constants scattered across `lib.mjs`/`docker.mjs`/`Dockerfile`; the Docker image build forwards the Tauri CLI version as a build arg.

#### Fixed

- Release tooling now checks the active rustc version before building: when it is older than the `minRustcVersion` floor in `build-config.json` (1.91.0, required by the locked `foyer@0.22.4+` dependency), it offers to run `rustup update` with confirmation instead of failing late inside `cargo tauri build`.

### <u>arachnea-docker</u>

#### Added

- Multi-arch (`linux/amd64` + `linux/arm64`) runtime image for the prebuilt portable Linux `arachnea` server: one Dockerfile where `TARGETARCH` selects the baked-in archive. The runtime stays minimal (Debian `trixie-slim` base, one `apt` layer, no build toolchain), ships Debian's `chromium` for the `chaser-cf` Cloudflare solver with one entrypoint-owned `Xvfb` on `:99` for its headed virtual display, runs as the non-root `arachnea` user (Chromium sandbox works without `--no-sandbox`/`--privileged`), persists state in `/app/data`, and uses `tini` as PID 1.
- Build configuration as Compose variables: `ARACHNEA_VERSION` and `ARACHNEA_RELEASES_DIR` live in the committed `arachnea-docker/.env` (template `.env.example`) or the environment, with no default and a fail-fast error when missing. The version expands into the two `ARACHNEA_AMD64_ARCHIVE` / `ARACHNEA_ARM64_ARCHIVE` paths (declared in `docker-compose.yml`, relative to the `releases` named build context), is stamped as `ENV ARACHNEA_VERSION`, and becomes the image tag: `image: arachnea-stream:${ARACHNEA_VERSION}` plus a `latest` alias via `build.tags`, so `docker compose up` runs the release just built. The archives are read straight from the release tree instead of a `bin/` staging folder; Buildx fails the build on a wrong path (`failed to compute cache key`), and the extracted binary's ELF `e_machine` is checked against the target port (`3e00` amd64 / `b700` arm64) so a swapped archive aborts the build instead of failing later with `exec format error`.
- `arachnea-docker/docker-entrypoint.sh`: wires `CHROME_BIN`, `CHASER_VIRTUAL_DISPLAY=0` and `CHASER_EXTRA_ARGS` (always ensuring `--disable-dev-shm-usage`), starts/reuses the one Xvfb display, and pins `ARACHNEA_PORT`/`ARACHNEA_NETWORK`/`ARACHNEA_ROOT` (all from the committed `.env`, empty root drops the key) into `/app/data/config.json` (the only reliable channel for those settings: `arachnea` rejects `--server-port 8080` with `unknown argument: 8080`). The published `ports:` mapping follows `ARACHNEA_PORT` on both sides.
- Stamped `HEALTHCHECK` (HTTP probe on `127.0.0.1:$ARACHNEA_PORT` via `/bin/bash`, valid in every `ARACHNEA_NETWORK` mode), Compose service (`ports`, `shm_size`), `.dockerignore`, `.gitignore`, and `README.md` (release-tree layout, named context, versioned tags, run notes).
- `arachnea-docker/build-image.sh` and `arachnea-docker/build-image.cmd`: build wrappers around `docker compose build` that resolve the release version to package. `--version=<version>` (or `--version <version>`) wins; otherwise the highest `release-*` folder of the release tree is picked (`../releases`, or `ARACHNEA_RELEASES_DIR` from the environment or `.env`, same precedence as Compose), and the script stops with an explicit error when neither is available. The resolved version is exported as `ARACHNEA_VERSION` (the environment wins over `.env`, so the image tag, the two archive paths and the stamped `ENV ARACHNEA_VERSION` all follow it), the two Linux portable archives are checked before the Docker build starts (clear message instead of a Buildx `failed to compute cache key`), any other argument is forwarded to `docker compose build` (e.g. `--no-cache` to rebuild every layer instead of reusing the cache of a previous release, `--progress=plain` or `--push` for the output), and the script ends by printing the version-pinned `docker compose up -d` line that starts the image just built.

#### Changed

#### Fixed

- The `apt` layer ends with a file-presence check rather than executing Chromium under foreign-architecture emulation, where Rosetta on Apple Silicon lacks Chromium's required SSE3 CPU feature.
- `arachnea-docker/docker-entrypoint.sh` no longer reuses `DISPLAY=:99` on the lock file alone: it verifies that an Xvfb process is actually alive, purges a stale `/tmp/.X99-lock` (leftover from a dead Xvfb, since `/tmp` survives `docker restart`) with its socket before starting a fresh server, and warns when the fresh server fails to come up instead of handing headed Chromium a dead `DISPLAY`.

### <u>arachnea-stream</u>

#### Added

#### Changed

#### Fixed

- `arachnea-stream` no longer compiles or initializes the Ghostwire smart Cloudflare solver. Automatic Cloudflare handling now goes from `rquest` directly to the Chromium-backed `chaser-cf` solver, preventing the native `SIGSEGV` that restarted the Docker container during Papadustream searches on Linux arm64.

### <u>arachnea-scrapyfy</u>

#### Added

#### Changed

- Ghostwire is no longer enabled by default. Consumers that still need the smart Cloudflare solver must explicitly enable the `ghostwire` feature.

#### Fixed

### <u>Other</u>

#### Added

#### Changed



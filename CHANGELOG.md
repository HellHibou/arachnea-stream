# Changelog

All notable changes to the server workspace are recorded here.
## Unreleased

### <u>build-release</u>

#### Added

#### Changed

- Release tooling versions are now centralized in `build-release/build-config.json` (minimum rustc, cross image tag, in-image Tauri CLI) instead of constants scattered across `lib.mjs`/`docker.mjs`/`Dockerfile`; the Docker image build forwards the Tauri CLI version as a build arg.

#### Fixed

- Release tooling now checks the active rustc version before building: when it is older than the `minRustcVersion` floor in `build-config.json` (1.91.0, required by the locked `foyer@0.22.4+` dependency), it offers to run `rustup update` with confirmation instead of failing late inside `cargo tauri build`.
- Portable `.tar.gz` archives built on Windows are now usable: `createArchive` / `createTarGz` normalize the POSIX modes recorded in the archive (`normalizeTarGzModes`), because Windows has no permission bits and its `bsdtar` stores every entry as `0666`/`0777` — the amd64 archive produced on a Windows host therefore held a **non-executable** `arachnea`. That broke `arachnea-docker`'s image build (`test -x /tmp/app/arachnea` aborted the `fetcher` stage with `exit code: 1` and no diagnostic, since the check only runs under `set -e`) and would also have handed Linux users an archive that does not run after extraction. Windows' `bsdtar` (3.8.8) rejects `--mode`, so the modes are rewritten in the archive itself — a pure-Node helper reading the gzipped tar with `node:zlib`, setting files to `0644`, directories to `0755` and the entries listed by the caller to `0755` (macOS and GNU tar values), which makes the archive independent of the build host, and recomputing each header checksum. `release.mjs` passes the release executable for the portable archive and `Arachnéa.app/Contents/MacOS/arachnea` for the macOS `.app` archive; a missing expected entry fails the build instead of shipping a silently broken archive.

### <u>arachnea-docker</u>

#### Added

- Multi-arch (`linux/amd64` + `linux/arm64`) runtime image for the prebuilt portable Linux `arachnea` server: one Dockerfile where `TARGETARCH` selects the baked-in archive. The runtime stays minimal (Debian `trixie-slim` base, one `apt` layer, no build toolchain), ships Debian's `chromium` for the `chaser-cf` Cloudflare solver with one entrypoint-owned `Xvfb` on `:99` for its headed virtual display, runs as the non-root `arachnea` user (Chromium's user-namespace sandbox still needs the container's seccomp profile unconfined — see the `Fixed` entry), persists state in `/app/data`, and uses `tini` as PID 1.
- Build configuration as Compose variables: `ARACHNEA_VERSION` and `ARACHNEA_RELEASES_DIR` live in the committed `arachnea-docker/.env` (template `.env.example`) or the environment, with no default and a fail-fast error when missing. The version expands into the two `ARACHNEA_AMD64_ARCHIVE` / `ARACHNEA_ARM64_ARCHIVE` paths (declared in `docker-compose.yml`, relative to the `releases` named build context), is stamped as `ENV ARACHNEA_VERSION`, and becomes the image tag: `image: arachnea-stream:${ARACHNEA_VERSION}` plus a `latest` alias via `build.tags`, so `docker compose up` runs the release just built. The archives are read straight from the release tree instead of a `bin/` staging folder; Buildx fails the build on a wrong path (`failed to compute cache key`), and the extracted binary's ELF `e_machine` is checked against the target port (`3e00` amd64 / `b700` arm64) so a swapped archive aborts the build instead of failing later with `exec format error`.
- `arachnea-docker/docker-entrypoint.sh`: wires `CHROME_BIN`, `CHASER_VIRTUAL_DISPLAY=0` and `CHASER_EXTRA_ARGS` (always ensuring `--disable-dev-shm-usage`), starts/reuses the one Xvfb display, and pins `ARACHNEA_PORT`/`ARACHNEA_NETWORK`/`ARACHNEA_ROOT` (all from the committed `.env`, empty root drops the key) into `/app/data/config.json` (the only reliable channel for those settings: `arachnea` rejects `--server-port 8080` with `unknown argument: 8080`). The published `ports:` mapping follows `ARACHNEA_PORT` on both sides.
- Stamped `HEALTHCHECK` (HTTP probe on `127.0.0.1:$ARACHNEA_PORT` via `/bin/bash`, valid in every `ARACHNEA_NETWORK` mode), Compose service (`ports`, `shm_size`), `.dockerignore`, `.gitignore`, and `README.md` (release-tree layout, named context, versioned tags, run notes).
- `arachnea-docker/build-image.sh` and `arachnea-docker/build-image.cmd`: build wrappers around `docker compose build` that resolve the release version to package. `--version=<version>` (or `--version <version>`) wins; otherwise the highest `release-*` folder of the release tree is picked (`../releases`, or `ARACHNEA_RELEASES_DIR` from the environment or `.env`, same precedence as Compose), and the script stops with an explicit error when neither is available. The resolved version is exported as `ARACHNEA_VERSION` (the environment wins over `.env`, so the image tag, the two archive paths and the stamped `ENV ARACHNEA_VERSION` all follow it), the two Linux portable archives are checked before the Docker build starts (clear message instead of a Buildx `failed to compute cache key`), any other argument is forwarded to `docker compose build` (e.g. `--no-cache` to rebuild every layer instead of reusing the cache of a previous release, `--progress=plain` or `--push` for the output), and the script ends by printing the version-pinned `docker compose up -d` line that starts the image just built.

#### Changed

#### Fixed

- `arachnea-docker` now keeps version resolution exclusively in `build-image.sh` / `build-image.cmd`: each wrapper selects the highest `release-*` directory when no version is passed, builds the versioned tag and its `latest` alias, while a bare `docker compose up -d` always starts the local `arachnea-stream:latest` image. The wrappers explicitly use the Buildx builder associated with the active Docker context and verify both tags in the local image store, preventing a globally selected `docker-container` builder from reporting success while retaining the image only in its private BuildKit cache. `pull_policy: never` prevents Compose from pulling or rebuilding a nonexistent `release-latest` archive pair.
- `docker-compose.yml` now sets `security_opt: [seccomp=unconfined]`. The image starts Chromium and Docker's *default* seccomp profile blocks the `clone`/`unshare` syscalls it needs to create its user-namespace sandbox, so the browser aborted with `No usable sandbox! ... you can try using --no-sandbox` and every Cloudflare fetch failed with `Failed to initialize browser: Browser process exited with status ExitStatus(unix_wait_status(256))` — reproduced on a Windows/WSL2 Docker Desktop host with the amd64 image, while the arm64 image on Apple Silicon was unaffected. Unconfining seccomp keeps Chromium's sandbox enabled (unlike the previously documented `CHASER_EXTRA_ARGS: "--no-sandbox"` fallback, which now also actually reaches the browser — see the `arachnea-http` entry) and needs no host-side configuration, which matters because WSL2's managed VM exposes neither `kernel.unprivileged_userns_clone` nor a settable AppArmor profile. `README.md` documents the required option for the Compose and plain-`docker run` paths, and the stale "no `--no-sandbox` needed, the sandbox works out of the box" claims in the `Dockerfile` and `docker-entrypoint.sh` comments were corrected.
- The `apt` layer ends with a file-presence check rather than executing Chromium under foreign-architecture emulation, where Rosetta on Apple Silicon lacks Chromium's required SSE3 CPU feature.
- `arachnea-docker/docker-entrypoint.sh` no longer reuses `DISPLAY=:99` on the lock file alone: it verifies that an Xvfb process is actually alive, purges a stale `/tmp/.X99-lock` (leftover from a dead Xvfb, since `/tmp` survives `docker restart`) with its socket before starting a fresh server, and warns when the fresh server fails to come up instead of handing headed Chromium a dead `DISPLAY`.
- An image built from a Windows working tree (`core.autocrlf=true`, no
  `.gitattributes`) never started: tini restart-looped with `exec
  /usr/local/bin/docker-entrypoint.sh failed: No such file or directory`
  because the copied entrypoint carried CRLF line endings (shebang
  `#!/bin/sh\r`). A root `.gitattributes` now pins `*.sh` and `Dockerfile`
  to LF on every checkout (`*.cmd`/`*.bat` stay CRLF), and the `Dockerfile`
  strips a trailing CR from the entrypoint's first line at `COPY` time so
  working trees checked out before that pinning still build a working image.

### <u>arachnea-stream</u>

#### Added

- `resolve_stream` responses can now expose an optional `subtitles` list (`{ lang?, label?, link }`) on `ResolvedPlayerStream`. Generic YAML-to-Rust conversion preserves resolver order, trims optional fields, discards tracks without a browser-consumable HTTP(S) or application-proxy `link`, and deduplicates exact `(lang, label, link)` entries. The field is omitted from JSON when empty, so existing resolvers keep their current payloads.
- Vidzy (`vidzy.cc` and `vidzy.live`) now extracts subtitle entries from `player.loadTracks`, retains only `kind: 'subtitles'` WebVTT tracks, and proxies each track with its required `Referer`, `Origin`, and `User-Agent` headers.
- Anime-Sama now exposes its daily release panels as selectable `subsections` under one daily-releases section, while Coflix adds its day, week, and month popular-content rails from the asynchronous `/ajax/movie/top` endpoints. Coflix preserves the source title, entry URL, and poster image without treating view counters as ratings or guessing a media type from its `/film/` URLs.
- The `arachnea-stream-hoster` group gains a StreamHG resolver (`streamhg.yaml`) covering the StreamHG-branded JW Player embeds served by rotating mirror domains such as `audinifer.com`, `vibuxer.com`, and `hanerix.com`, plus the `hgcloud.to` JavaScript router. Direct player recognition is content-based (the brand declared to the packed player setup), so no player mirror domain is required by the resolver; stable `hgcloud.to/e/<id>` entry URLs are mapped to a real StreamHG player mirror because the generic resolver deliberately does not execute their obfuscated redirect script. `resolve_stream` fetches the effective player page, unpacks the Dean Edwards Packer block, reads the page `links` object in the site's own priority (`hls4`, then `hls3`, then `hls2`) as ordered `stream_url` candidates whose order no longer depends on the key order the hoster emits, and returns the HLS manifest proxied with the effective embed-page `Referer`, the page title, the `#vplayer` poster, and the `/dl?op=get_slides` sprite VTT.

#### Changed

- The `arachnea-stream-hoster` Vidzy resolver is unified for `vidzy.cc` and `vidzy.live` (including optional `www.`). Its HLS decoder derives the XOR key from the request hostname, and all stream and subtitle proxy requests use the resolved request origin. The decoder returns its string through `document.write`, which is the supported `exec_js` string channel.
- The public Vue player propagates resolved subtitles through primary and fallback media sources, adds them as Video.js remote subtitle tracks, restores saved text-track preferences, and explicitly removes renderer-owned tracks on source changes and disposal.

#### Fixed

- Anime-Sama catalog cards now extract genres only once from their genre tags and exclude the decorative ellipsis tag, preventing duplicate or empty genre labels in search and home rails.
- Coflix search now parses the HTML returned by `/filter?keyword=…` instead of treating it as the retired JSON payload, restoring result titles, links, poster extraction, VF/VOSTFR language metadata, and next-page navigation.
- DoodStream-compatible embeds now return the generated CDN media URL instead of proxying their intermediate `/pass_md5/...` request. The resolver sends its JavaScript result through `document.write`, which is the `exec_js` string-return channel, extracts it before proxy wrapping, derives the media `Referer` from the resolved embed origin rather than hardcoding `playmogo.com`, and no longer exposes an unsupported storyboard.

- `arachnea-stream` no longer compiles or initializes the Ghostwire smart Cloudflare solver. Automatic Cloudflare handling now goes from `rquest` directly to the Chromium-backed `chaser-cf` solver, preventing the native `SIGSEGV` that restarted the Docker container during Papadustream searches on Linux arm64.

### <u>arachnea-scrapyfy</u>

#### Added

#### Changed

- Ghostwire is no longer enabled by default. Consumers that still need the smart Cloudflare solver must explicitly enable the `ghostwire` feature.

#### Fixed

### <u>arachnea-http</u>

#### Added

#### Changed

#### Fixed

- The `chaser-cf` engine builds its default Chromium flags with `ChaserConfig::add_extra_arg` instead of `with_extra_args`. The latter *replaces* the whole flag set, so the defaults appended after `ChaserConfig::from_env()` silently discarded the flags that function had just parsed from `CHASER_EXTRA_ARGS`, making the documented override a no-op — a container passing `--no-sandbox` still started Chromium without it and hit `No usable sandbox!`.

### <u>front-public</u>

#### Added

- Home and category rails can now render optional catalog subsections. The visible rail title combines parent and subsection labels, and accessible previous/next controls cycle through subsection rails with wraparound navigation.

#### Changed

- Home catalog normalization and multi-source merging preserve subsection boundaries and merge equivalent subsections only by their stable key under the same parent section.

#### Fixed

- Media-card details now render the language fact with its own label and value, instead of incorrectly rendering the genre a second time or an empty genre value when only a language is available.
- Pinned home rails now render the active subsection entries and retain the cyclic subsection controls, rather than rendering the empty parent item list.
- The previous subsection control now appears before the rail title, while the next control remains in the header actions.

### <u>Other</u>

#### Added

#### Changed



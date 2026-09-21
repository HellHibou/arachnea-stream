# Arachnea Stream — Docker runtime (linux/amd64 + linux/arm64)
#
# Multi-arch runtime image for the prebuilt Linux `arachnea` server binary
# (portable `arachnea-*-linux-*.tar.gz` layout: `arachnea` + `services/`).
# The image stays as small as possible: Debian slim base, no build toolchain,
# and a single `apt-get` layer installing only the runtime libraries the
# binary needs (GTK/WebKitGTK stack) plus a usable Chromium for the
# `chaser-cf` Cloudflare solver and the Xvfb virtual display it requires on
# headless servers.
#
# One Dockerfile covers both architectures: Docker Buildx sets TARGETARCH
# (amd64/arm64) and the portable archive declared for that port is copied in
# from the `releases` named build context. Those paths are build args
# (`ARACHNEA_AMD64_ARCHIVE` / `ARACHNEA_ARM64_ARCHIVE`) whose defaults expand
# `ARACHNEA_VERSION`, so bumping a release only changes that single arg. The
# Debian `chromium` package exists natively on both ports, so no per-arch
# package special-casing is needed.
#
# Two contexts are required:
#
#   ./                 main context: this Dockerfile + docker-entrypoint.sh
#   <release tree>     named context `releases`
#
# The release tree is the project's `releases/` folder produced by
# `node build-release/release.mjs`. It cannot live inside the main context
# (Buildx refuses to COPY across contexts), hence the named context.
# docker-compose.yml points it at `../releases` (relative to the compose file,
# i.e. `<project>/releases`) and lets `ARACHNEA_RELEASES_DIR` override it.
#
# Build (from the repository root):
#
#   docker buildx build --platform linux/amd64,linux/arm64 \
#     --build-context releases="$PWD/releases" \
#     -t arachnea-stream:latest ./arachnea-docker --load
#   # or push a multi-arch manifest:
#   docker buildx build --platform linux/amd64,linux/arm64 \
#     --build-context releases="$PWD/releases" \
#     -t <registry>/arachnea-stream:latest ./arachnea-docker --push
#   # a release bump only needs --build-arg ARACHNEA_VERSION=<version>
#
# Required layout inside the release tree (see README.md / docker-compose.yml):
#
#   release-0.1.0/linux/arachnea-0.1.0-linux-amd64-portable.tar.gz  # ARACHNEA_AMD64_ARCHIVE
#   release-0.1.0/linux/arachnea-0.1.0-linux-arm64-portable.tar.gz  # ARACHNEA_ARM64_ARCHIVE
#
# Runtime: see `docker-entrypoint.sh` for the Xvfb + CHROME_BIN wiring and the
# ensured Chrome flag (`--disable-dev-shm-usage`; `--no-sandbox` stays opt-in
# via `CHASER_EXTRA_ARGS` since the image runs as non-root).

# Debian Bookworm (glibc 2.36) is too old for the prebuilt `arachnea` binary:
# the release archives were linked against glibc >= 2.38, so the runtime base
# must provide at least that. Debian Trixie (glibc 2.41) ships native
# `chromium` + `libwebkit2gtk-4.1-0` packages on both amd64 and arm64, which
# keeps this single Dockerfile portable without downloading any out-of-band
# Chrome build.
ARG DEBIAN_TAG=trixie-slim
FROM --platform=$BUILDPLATFORM debian:${DEBIAN_TAG} AS fetcher

# Re-declare TARGETARCH in this stage (build args do not propagate across
# stages unless re-declared).
ARG TARGETARCH
# Release version packaged into the image: it expands the archive paths below
# (single source for a release bump) and is exported as an `ENV` for the
# entrypoint log line.
ARG ARACHNEA_VERSION=0.1.0
# Path of each portable archive inside the `releases` named build context (the
# `release-<version>/linux/` tree produced by `build-release/release.mjs`).
# Override them when a release is named differently.
ARG ARACHNEA_AMD64_ARCHIVE=release-${ARACHNEA_VERSION}/linux/arachnea-${ARACHNEA_VERSION}-linux-amd64-portable.tar.gz
ARG ARACHNEA_ARM64_ARCHIVE=release-${ARACHNEA_VERSION}/linux/arachnea-${ARACHNEA_VERSION}-linux-arm64-portable.tar.gz

# `releases` is a named build context (declared by docker-compose.yml through
# `build.additional_contexts`, or `--build-context releases=<dir>` on the CLI):
# Buildx refuses to COPY across contexts, so the release tree cannot be reached
# from the main context. One COPY per port, under a stable per-architecture
# name (the two archives do not need to share a file name); a wrong or missing
# path fails here at build time (`failed to compute cache key: ...: not found`)
# instead of resolving to a stale archive.
COPY --from=releases ${ARACHNEA_AMD64_ARCHIVE} /tmp/bin/arachnea-amd64-portable.tar.gz
COPY --from=releases ${ARACHNEA_ARM64_ARCHIVE} /tmp/bin/arachnea-arm64-portable.tar.gz

# Extract the archive for the port being built and check that it really is a
# binary of that port: the ELF `e_machine` field (bytes 18-19 of the header)
# is `3e00` for x86_64 and `b700` for AArch64. Swapped archive paths are
# otherwise only detected at runtime (`exec format error`).
RUN set -e; \
    case "$TARGETARCH" in \
      amd64) MACHINE="3e00" ;; \
      arm64) MACHINE="b700" ;; \
      *) echo "Unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    ARCHIVE="/tmp/bin/arachnea-${TARGETARCH}-portable.tar.gz"; \
    echo "Using portable archive: $ARCHIVE"; \
    mkdir -p /tmp/app; \
    tar -xzf "$ARCHIVE" -C /tmp/app; \
    test -x /tmp/app/arachnea; \
    test -d /tmp/app/services; \
    FOUND="$(od -An -tx1 -j 18 -N 2 /tmp/app/arachnea | tr -d ' \n')"; \
    if [ "$FOUND" != "$MACHINE" ]; then \
      echo "Wrong archive for linux/$TARGETARCH: $ARCHIVE holds an ELF e_machine=$FOUND binary (expected $MACHINE)" >&2; \
      exit 1; \
    fi

# ---- Runtime image ---------------------------------------------------------
FROM debian:${DEBIAN_TAG}

LABEL org.opencontainers.image.title="Arachnea Stream" \
      org.opencontainers.image.description="Arachnea Stream server (linux/amd64 + linux/arm64) with Chromium for the chaser-cf Cloudflare solver." \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"

# The REST controller listens on 8080 by default (`--server-port` overrides).
EXPOSE 8080

# Version of the `arachnea` binary baked into the image (informational only).
ARG ARACHNEA_VERSION=0.1.0
ENV ARACHNEA_VERSION=${ARACHNEA_VERSION} \
    # Port and network mode consumed by `docker-entrypoint.sh`.
    ARACHNEA_PORT=8080 \
    # `public` keeps the server reachable through published ports: Docker
    # host-forwarded traffic does not reliably originate from the container's
    # own subnets, so `private`/`local` answer 404 to it (see README).
    ARACHNEA_NETWORK=public \
    # chaser-cf auto-detects `CHROME_BIN`; the Chromium Debian package
    # installs both binaries, `chromium` first.
    CHROME_BIN=/usr/bin/chromium \
    # The entrypoint owns Xvfb on DISPLAY=:99. Keep chaser-cf internal
    # Xvfb disabled so it reuses that display instead of launching a second
    # server and mutating DISPLAY after the application has started.
    # Set CHASER_VIRTUAL_DISPLAY=1 only to let chaser-cf manage its own Xvfb.
    CHASER_VIRTUAL_DISPLAY=0 \
    DISPLAY=:99 \
    # Writable state, i.e. the application `data/` directory. `arachnea`
    # resolves its data root as the executable directory when the latter is
    # writable (portable layout), so the persisted configuration lives at
    # `$ARACHNEA_DATA_DIR/config.json` (see docker-entrypoint.sh). Mount a
    # named volume here to keep config, caches and Cloudflare sessions.
    ARACHNEA_DATA_DIR=/app/data

# Single apt layer: runtime libraries only, no build tools, no docs/man.
# - libwebkit2gtk-4.1-0 + libgtk-3-0 + libayatana-appindicator3-1: linked by
#   the `arachnea` binary (Tauri/Wry runtime deps, see `ldd` NEEDED list).
# - libssl3 + ca-certificates: TLS for the scraper HTTP stack.
# - chromium: real Chrome-compatible binary used by `chaser-cf`
#   (Cloudflare solver). The Debian package exists on both amd64/arm64.
# - xvfb: virtual display running headed Chromium on headless servers.
# - jq: merges `ARACHNEA_PORT`/`ARACHNEA_NETWORK` into the persisted
#   `$ARACHNEA_DATA_DIR/config.json` (the binary rejects any option value on
#   its command line: `--server-port 8080` fails with
#   `unknown argument: 8080`).
# - tini: minimal init reaping zombie Chrome/Xvfb child processes (PID 1).
#
# The layer ends with a presence check only (no execution): executing Chromium
# is not reliable for a foreign x86_64 target under Apple Silicon/Rosetta
# emulation (the emulated CPU lacks Chromium's SSE3 prerequisite). Chromium is
# exercised at runtime instead; see README for the architecture caveat.
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        ca-certificates \
        chromium \
        libayatana-appindicator3-1 \
        libgtk-3-0 \
        libssl3 \
        libwebkit2gtk-4.1-0 \
        jq \
        tini \
        xvfb \
    && rm -rf /var/lib/apt/lists/* \
    && test -x /usr/bin/chromium \
    && test -x /usr/bin/Xvfb

# Unprivileged runtime user: avoids the Chrome
# "Running as root without --no-sandbox is not supported" failure mode.
# `/tmp/.X11-unix` is created here because Xvfb otherwise logs
# "_XSERVTransmkdir: ERROR: euid != 0" on every start (harmless but noisy).
RUN set -e; \
    groupadd --system --gid 10001 arachnea; \
    useradd --system --uid 10001 --gid arachnea \
      --home-dir /app --shell /usr/sbin/nologin arachnea; \
    mkdir -p /app/data; \
    chown -R arachnea:arachnea /app; \
    mkdir -p /tmp/.X11-unix; \
    chmod 1777 /tmp/.X11-unix

# Portable app layout from the fetcher stage: `/app/arachnea` + `/app/services`.
COPY --from=fetcher --chown=arachnea:arachnea /tmp/app/arachnea /app/arachnea
COPY --from=fetcher --chown=arachnea:arachnea /tmp/app/services /app/services
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

USER arachnea
WORKDIR /app
# Writable application state (`<exe dir>/data`: config.json + persistence
# stores). Declared as a volume so `docker run -v`/Compose keep it across
# container recreation.
VOLUME ["/app/data"]

# HTTP probe on loopback: the in-image `/bin/sh` is `dash`, which has no
# `/dev/tcp`, so the check explicitly runs `/bin/bash` (already part of the
# Debian base, no extra package). Probing 127.0.0.1 keeps the check valid in
# every `ARACHNEA_NETWORK` mode (`local` rejects non-loopback peers with 404)
# and the 12-byte read only needs the status line, so the response body is
# never awaited.
HEALTHCHECK --interval=30s --timeout=5s --start-period=60s --retries=3 \
  CMD ["/bin/bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/${ARACHNEA_PORT:-8080} && printf 'GET / HTTP/1.0\\r\\nHost: 127.0.0.1\\r\\n\\r\\n' >&3 && head -c 12 <&3 | grep -q ' 200'"]

ENTRYPOINT ["tini", "--", "/usr/local/bin/docker-entrypoint.sh"]

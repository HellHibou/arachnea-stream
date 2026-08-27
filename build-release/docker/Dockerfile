# Arachnea cross-build image.
#
# Derived from joseluisq/rust-linux-darwin-builder, which already ships:
#   - a Rust toolchain (rustup + cargo) for the host architecture,
#   - osxcross (Clang + Apple macOS SDK 11.3) to cross-compile macOS binaries
#     (x86_64-apple-darwin and aarch64-apple-darwin) from a Linux container,
#   - GCC/Clang toolchains for the native Linux targets.
#
# This derivation adds the WebKitGTK/GUI development stack required to compile
# the Tauri application for the *-unknown-linux-gnu targets (which the base
# image does not ship) and updates rustup to the latest stable (see below).
# It also installs the Tauri CLI (`cargo tauri build`), used inside the image
# to produce the Linux installers (.deb/.rpm/.AppImage).
# macOS targets need no extra system package: wry on macOS links against the
# Apple frameworks provided by the osxcross SDK.
FROM joseluisq/rust-linux-darwin-builder:2.0.0-beta.1

# Linux GUI build dependencies for Tauri (WebKitGTK 4.1 stack). Kept in the
# image so crate builds can rely on pkg-config resolving the right flags.
#
# The release tooling (`build-release/docker.mjs`) runs this image with
# `--platform linux/amd64` for x86_64-unknown-linux-gnu targets and
# `--platform linux/arm64` for aarch64-unknown-linux-gnu ones: linking a
# GNU/Linux binary requires GTK/WebKitGTK/OpenSSL libraries for the *target*
# architecture, which is only guaranteed when the container architecture
# matches the target. These apt packages are therefore always native to the
# running container.
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-dev \
        libgtk-3-dev \
        librsvg2-dev \
        patchelf \
        xdg-utils \
        curl \
        ca-certificates \
        file \
        desktop-file-utils \
    && rm -rf /var/lib/apt/lists/*

# Tauri CLI (`cargo tauri build`) used by the Docker bundling step for the Linux
# installers (.deb/.rpm/.AppImage). Upstream only ships a prebuilt binary for
# x86_64-unknown-linux-gnu; the arm64 port compiles it from crates.io instead.
# The GNU target must be passed explicitly on arm64: the base image's cargo
# config defaults that port to a static musl build of cargo-install targets,
# which fails to link (mixed glibc/musl objects, missing libm symbols).
# `--root /usr/local` places `cargo-tauri` into /usr/local/bin so the plain
# `cargo tauri ...` form works.
ARG TARGETARCH
ARG TAURI_CLI_VERSION=2.11.4
RUN set -e; \
    case "$TARGETARCH" in \
      amd64) \
        curl -fsSL "https://github.com/tauri-apps/tauri/releases/download/tauri-cli-v${TAURI_CLI_VERSION}/cargo-tauri-x86_64-unknown-linux-gnu.tgz" \
          | tar -xz -C /usr/local/bin cargo-tauri ;; \
      arm64) \
        cargo install --locked --version "$TAURI_CLI_VERSION" \
          --target aarch64-unknown-linux-gnu --root /usr/local tauri-cli ;; \
      *) \
        echo "Unsupported TARGETARCH for tauri-cli installation: $TARGETARCH" >&2; exit 1 ;; \
    esac

# Bring Rust up to the latest stable and re-install the cross targets on it.
# The base image pins an older rustc (earlier 1.8x) that cannot compile the
# project's dependency graph (boa, cookie_store, time, ... require >= 1.88).
# `rustup` keeps /root/.cargo/config.toml untouched, so the osxcross compiler
# wrappers configured by the base image still drive macOS linking.
RUN rustup update stable \
    && rustup default stable \
    && rustup target add \
        x86_64-apple-darwin \
        aarch64-apple-darwin \
        x86_64-unknown-linux-gnu \
        aarch64-unknown-linux-gnu

# Point cc-rs at the osxcross Objective-C/C compilers for the darwin targets.
# Without these, cc-rs falls back to the host `cc` (GCC), which does not accept
# the clang flags passed for macOS (-arch / -mmacosx-version-min) and fails
# compiling any dependency shipping .m/.c sources (e.g. objc2, ring).
#
# IMPORTANT: cargo only forwards environment variables whose name is a valid
# identifier to build scripts — names containing a dash are dropped. cc-rs
# therefore reads the DASH-SUBSTITUTED form (`_` in place of `-`) that cargo
# does forward: `CC_x86_64_apple_darwin`.
#
# The wrappers below are named with the FULL osxcross triple prefix
# (`x86_64-apple-darwin22.4-clang`). This is essential: cmake derives every
# binutils executable (install_name_tool, dsymutil, nm, ar, ranlib, ...) by
# substituting the trailing compiler name, so a matching `-clang` basename makes
# cmake locate the existing osxcross `x86_64-apple-darwin22.4-*` tools.
# The backing compilers use the arch-VERSIONED osxcross driver names
# (`aarch64-apple-darwin22.4-clang`, ...): the historical unversioned names
# (`aarch64-apple-darwin-clang`) do not exist in every image architecture port,
# while the versioned tools ship identically under amd64 and arm64.

RUN set -e; \
    BIN=/usr/local/bin; \
    printf '#!/bin/sh\nexec /usr/local/osxcross/target/bin/o64-clang -fuse-ld=/usr/local/osxcross/target/bin/x86_64-apple-darwin22.4-ld "$@"\n' > "$BIN/x86_64-apple-darwin22.4-clang"; \
    printf '#!/bin/sh\nexec /usr/local/osxcross/target/bin/o64-clang++ -fuse-ld=/usr/local/osxcross/target/bin/x86_64-apple-darwin22.4-ld "$@"\n' > "$BIN/x86_64-apple-darwin22.4-clang++"; \
    printf '#!/bin/sh\nexec /usr/local/osxcross/target/bin/aarch64-apple-darwin22.4-clang -fuse-ld=/usr/local/osxcross/target/bin/aarch64-apple-darwin22.4-ld "$@"\n' > "$BIN/aarch64-apple-darwin22.4-clang"; \
    printf '#!/bin/sh\nexec /usr/local/osxcross/target/bin/aarch64-apple-darwin22.4-clang++ -fuse-ld=/usr/local/osxcross/target/bin/aarch64-apple-darwin22.4-ld "$@"\n' > "$BIN/aarch64-apple-darwin22.4-clang++"; \
    chmod +x "$BIN"/*-apple-darwin22.4-clang*

ENV CC_x86_64_apple_darwin=/usr/local/bin/x86_64-apple-darwin22.4-clang \
    CXX_x86_64_apple_darwin=/usr/local/bin/x86_64-apple-darwin22.4-clang++ \
    CC_aarch64_apple_darwin=/usr/local/bin/aarch64-apple-darwin22.4-clang \
    CXX_aarch64_apple_darwin=/usr/local/bin/aarch64-apple-darwin22.4-clang++

# Prefer the system OpenSSL shared objects at link time for the GNU/Linux
# targets: `boring-sys2` (static BoringSSL) emits -L directories whose
# `libssl.a`/`libcrypto.a` shadow the system libraries when the linker resolves
# bare `-lssl`/`-lcrypto` names, leaving a handful of OpenSSL 3-only symbols
# (SSL_read_ex, SSL_get1_peer_certificate, ERR_get_error_all, ...) undefined.
# Appending the absolute system objects makes resolution deterministic
# regardless of the search-path order emitted by dependency build scripts.
# The paths are native to whichever container architecture runs, matching the
# target triple they are scoped to above.
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-Clink-arg=/usr/lib/x86_64-linux-gnu/libssl.so -Clink-arg=/usr/lib/x86_64-linux-gnu/libcrypto.so" \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-Clink-arg=/usr/lib/aarch64-linux-gnu/libssl.so -Clink-arg=/usr/lib/aarch64-linux-gnu/libcrypto.so"

# AppImage bundling toolchain. linuxdeploy-plugin-appimage produces the
# SquashFS image through `mksquashfs`, which the base image does not ship; the
# AppImage bundler fails with `failed to run linuxdeploy` without it. Kept as a
# separate layer after the Tauri CLI one so the heavy apt/tauri-cli layers stay
# cached when this list changes.
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        squashfs-tools \
    && rm -rf /var/lib/apt/lists/*

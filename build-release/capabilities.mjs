// Cross-compilation capability matrix for Tauri desktop bundles.
//
// Map: build host (Node `process.platform`) -> Rust target triple -> bundles
// the host can produce locally, plus the runner needed for that target.
//
// Facts encoded here (Tauri v2 + native toolchains):
// - macOS (.app/.dmg): requires an Apple SDK / Xcode, so a macOS host only.
// - Windows NSIS (-setup.exe): cross-compilable from macOS/Linux through the
//   `cargo-xwin` runner (provides llvm-rc and the MSVC CRT crates).
// - Windows MSI: WiX only runs on Windows, so a Windows host only.
// - Linux (.deb/.rpm/.AppImage): requires dpkg/rpmbuild/appimagetool and the
//   WebKitGTK stack, so a Linux host only (or an equivalent container).
const HOST_CAPABILITIES = {
  darwin: {
    'x86_64-apple-darwin': { bundles: ['app', 'dmg'], runner: null },
    'aarch64-apple-darwin': { bundles: ['app', 'dmg'], runner: null },
    'universal-apple-darwin': {
      bundles: ['app', 'dmg'],
      runner: null,
      needsTargets: ['aarch64-apple-darwin', 'x86_64-apple-darwin'],
    },
    'x86_64-pc-windows-msvc': { bundles: ['nsis'], runner: 'cargo-xwin' },
    'aarch64-pc-windows-msvc': { bundles: ['nsis'], runner: 'cargo-xwin' },
    'x86_64-unknown-linux-gnu': { bundles: [], runner: null },
    'aarch64-unknown-linux-gnu': { bundles: [], runner: null },
  },
  win32: {
    'x86_64-apple-darwin': { bundles: [], runner: null },
    'aarch64-apple-darwin': { bundles: [], runner: null },
    'universal-apple-darwin': { bundles: [], runner: null },
    'x86_64-pc-windows-msvc': { bundles: ['nsis', 'msi'], runner: null },
    'aarch64-pc-windows-msvc': { bundles: ['nsis', 'msi'], runner: null },
    'x86_64-unknown-linux-gnu': { bundles: [], runner: null },
    'aarch64-unknown-linux-gnu': { bundles: [], runner: null },
  },
  linux: {
    'x86_64-apple-darwin': { bundles: [], runner: null },
    'aarch64-apple-darwin': { bundles: [], runner: null },
    'universal-apple-darwin': { bundles: [], runner: null },
    'x86_64-pc-windows-msvc': { bundles: ['nsis'], runner: 'cargo-xwin' },
    'aarch64-pc-windows-msvc': { bundles: ['nsis'], runner: 'cargo-xwin' },
    'x86_64-unknown-linux-gnu': { bundles: ['deb', 'rpm', 'appimage'], runner: null },
    'aarch64-unknown-linux-gnu': { bundles: ['deb', 'rpm', 'appimage'], runner: null },
  },
};

// Short architecture tokens used in release artifact file names.
const ARCH_SHORT = {
  'x86_64-apple-darwin': 'x64',
  'aarch64-apple-darwin': 'arm64',
  'universal-apple-darwin': 'universal',
  'x86_64-pc-windows-msvc': 'x64',
  'aarch64-pc-windows-msvc': 'arm64',
  'x86_64-unknown-linux-gnu': 'amd64',
  'aarch64-unknown-linux-gnu': 'arm64',
};

/**
 * Release subfolder name per platform family (the part of a platform id
 * before the first dash). Families missing from this map keep their raw name.
 */
const FAMILY_OUTPUT_NAMES = {
  darwin: 'darwin',
  windows: 'windows',
  linux: 'linux',
};

/**
 * Selector aliases accepted on the command line, mapped to the canonical
 * family they select (e.g. `osx` selects every `darwin-*` platform).
 */
const SELECTOR_ALIASES = {
  osx: 'darwin',
};

/**
 * Returns the release subfolder name for a platform id, using only the
 * family part before the first dash (`darwin-arm64` -> `darwin`,
 * `windows-x86_64` -> `windows`).
 *
 * @param {string} platformId - Platform identifier from `release-config.json`.
 * @returns {string} Output subfolder name.
 */
export function outputFolderName(platformId) {
  const family = platformId.split('-')[0];
  return FAMILY_OUTPUT_NAMES[family] ?? family;
}

/**
 * Normalizes a CLI selector through the centralized alias table
 * (`osx` -> `darwin`). Unknown selectors are returned unchanged.
 *
 * @param {string} selector - Raw selector from the command line.
 * @returns {string} Canonical selector.
 */
export function normalizeSelector(selector) {
  return SELECTOR_ALIASES[selector] ?? selector;
}

/**
 * Returns the family part of a platform id or selector
 * (`darwin-arm64` -> `darwin`).
 *
 * @param {string} idOrSelector - Platform id or selector.
 * @returns {string} Family token before the first dash.
 */
export function familyOf(idOrSelector) {
  return idOrSelector.split('-')[0];
}

/** Current build host normalized to a supported capability key or `null`. */
export function hostId() {
  return HOST_CAPABILITIES[process.platform] ? process.platform : null;
}

/**
 * Computes which of the requested bundles can actually be produced on this
 * host for the given platform entry.
 *
 * @param {object} platform - A platform entry from `release-config.json`.
 * @param {string[]} requested - Bundle types requested by the platform entry.
 * @returns {{ usable: string[], runner: string|null, needsTargets: string[] }}
 *   The locally producible bundles, the runner to use (or `null`), and the
 *   rustup targets that must be installed on this host.
 */
export function usableBundlesFor(platform, requested) {
  const host = hostId();
  const cap =
    (host && HOST_CAPABILITIES[host][platform.target]) || { bundles: [], runner: null, needsTargets: null };
  const usable = requested.filter((bundle) => cap.bundles.includes(bundle));
  return {
    usable,
    runner: cap.runner ?? null,
    needsTargets: cap.needsTargets ?? [platform.target],
  };
}

/** Returns a short architecture token for a Rust target triple. */
export function archShort(target) {
  return ARCH_SHORT[target] ?? target.split('-')[0];
}

/**
 * Rust targets that can be produced as a single raw executable through the
 * Arachnea cross-build image (`build-release/docker/Dockerfile`, derived from
 * `joseluisq/rust-linux-darwin-builder`). This does NOT create platform
 * installers (`.dmg`, `.deb`, ...): it only compiles the release binary that
 * the host-side portable step packages (`.tar.gz`/`.zip`).
 *
 * A platform entry that sets `"build": "docker"` together with a `portable`
 * block is produced this way whenever the current host cannot natively bundle
 * that target.
 */
const DOCKER_PORTABLE_TARGETS = {
  'x86_64-apple-darwin': true,
  'aarch64-apple-darwin': true,
  'x86_64-unknown-linux-gnu': true,
  'aarch64-unknown-linux-gnu': true,
};

/**
 * Returns `true` when the platform entry declares a portable artifact that must
 * be produced through the Docker cross-build image (the host cannot natively
 * bundle this target as an installer, but can still ship a raw binary).
 *
 * @param {object} platform - A platform entry from `release-config.json`.
 * @returns {boolean} Whether the platform should be built inside the container.
 */
export function needsDockerBuild(platform) {
  return !!(platform.portable && platform.build === 'docker' && DOCKER_PORTABLE_TARGETS[platform.target]);
}

/**
 * Resolves the production method for a platform entry.
 *
 * - `native`: the current host can natively bundle this target (e.g. `.dmg` on
 *   macOS, NSIS/MSI on Windows, Linux installers on Linux).
 * - `docker`: the raw release binary must be cross-compiled in the container
 *   and shipped as a portable archive.
 * - `none`: cannot be produced on this host at all.
 *
 * With `forceDocker`, platforms eligible for the Docker cross-build image
 * (`needsDockerBuild`) use it even when this host could natively bundle them;
 * targets not managed by the image still fall back to the native builder.
 *
 * @param {object} platform - A platform entry from `release-config.json`.
 * @param {string[]} usable - The bundles usable on this host (see `usableBundlesFor`).
 * @param {boolean} [forceDocker] - Force the Docker builder where supported.
 * @returns {'native'|'docker'|'none'}
 */
export function productionMethod(platform, usable, forceDocker = false) {
  if (forceDocker && needsDockerBuild(platform)) return 'docker';
  if (usable.length > 0) return 'native';
  return needsDockerBuild(platform) ? 'docker' : 'none';
}


// Docker cross-build orchestration for the Arachnea release tooling.
//
// Produces the raw release binary for Linux/macOS targets that the current host
// cannot natively bundle, by compiling inside the Arachnea cross image
// (build-release/docker/Dockerfile, derived from
// joseluisq/rust-linux-darwin-builder). The compiled executable lands in the
// mounted repository `target/<triple>/release/` and is then packaged on the
// host by the portable-archive step.
import { existsSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { ROOT, canRun, run, releaseDir } from './lib.mjs';

const RELEASE_DIR = releaseDir();

/** Tag of the locally-built Arachnea cross image. */
export const CROSS_IMAGE = 'arachnea-cross-builder:1.0.0';

/** Directory holding the cross image Dockerfile. */
export const DOCKERFILE_DIR = path.join(RELEASE_DIR, 'docker');

/** Returns `true` when the `docker` CLI is available on the PATH. */
export function dockerAvailable() {
  return canRun('docker');
}

/** Throws a contextualized error when Docker is not available. */
export function assertDocker() {
  if (!dockerAvailable()) {
    throw new Error(
      'Docker is required to produce the portable Linux/macOS binaries but is not ' +
        'available on the PATH. Install Docker (e.g. Docker Desktop on Windows/macOS) and retry.',
    );
  }
}

/** Returns the set of locally built image names (`repo:tag`). */
function dockerImageRefs() {
  const result = spawnSync('docker', ['images', '--format', '{{.Repository}}:{{.Tag}}'], { encoding: 'utf8' });
  if (result.status !== 0) return new Set();
  return new Set(result.stdout.trim().split(/\r?\n/).filter(Boolean));
}

/** Returns `true` when the cross image has already been built locally. */
export function crossImagePresent() {
  return dockerImageRefs().has(CROSS_IMAGE);
}

/**
 * Ensures the Arachnea cross image is built. Because this pulls a large base
 * image (Rust + osxcross + Apple SDK) on first use, it requires confirmation.
 */
export function ensureCrossImage() {
  assertDocker();
  if (crossImagePresent()) return;
  run('docker', ['build', '--tag', CROSS_IMAGE, DOCKERFILE_DIR]);
}

/** Converts an absolute host path into a Docker Desktop mount path. */
function toDockerMount(hostPath) {
  // Replace backslashes with forward slashes; keep the drive letter so Docker
  // Desktop on Windows can bind-mount e.g. `D:/repo` into the container.
  return path.resolve(hostPath).replace(/\\/g, '/');
}

/**
 * Resolves the container architecture a target must be compiled in. GNU/Linux
 * binaries link against GTK/WebKitGTK/OpenSSL for their own architecture, so
 * the container architecture has to match the Linux target triple. Other
 * targets (macOS via osxcross, Tauri's `cargo tauri build` host parts) use the
 * host architecture to avoid needless emulation.
 *
 * @param {object} platform - A platform entry.
 * @returns {string} `amd64` or `arm64`.
 */
export function containerArchFor(platform) {
  if (platform.target.startsWith('x86_64-unknown-linux')) return 'amd64';
  if (platform.target.startsWith('aarch64-unknown-linux')) return 'arm64';
  return process.arch === 'arm64' ? 'arm64' : 'amd64';
}

/**
 * Resolves the container platform for a target. Cross-compiling an
 * `aarch64-unknown-linux-gnu` binary needs an arm64 container (emulated when
 * the host is amd64), an `x86_64-unknown-linux-gnu` binary needs an amd64
 * container, whereas macOS targets are always compiled from the host-arch
 * container through osxcross (no QEMU needed).
 *
 * @param {object} platform - A platform entry.
 * @returns {string|null} `--platform` value, or `null` to keep the host default.
 */
function imagePlatformFor(platform) {
  if (!platform.target.includes('-unknown-linux')) return null;
  return `linux/${containerArchFor(platform)}`;
}

/**
 * Resolves the workspace-level cargo target directory root for a Tauri crate
 * (first parent owning a `Cargo.toml`, falling back to the crate itself).
 * Mirrors the walk performed by `targetBaseDirs()` in lib.mjs.
 *
 * @param {string} tauriDir - Absolute path of the Tauri crate on the host.
 * @returns {string} Absolute path of the workspace root directory.
 */
function workspaceRootFor(tauriDir) {
  let dir = path.dirname(tauriDir);
  while (true) {
    if (existsSync(path.join(dir, 'Cargo.toml'))) return dir;
    const parent = path.dirname(dir);
    if (parent === dir) return tauriDir;
    dir = parent;
  }
}

/**
 * Builds the `docker run` argument list that compiles the raw release binary
 * for the given platform inside the cross image. The repository is mounted at
 * `/io` and the Tauri crate is compiled from its workspace layout so cargo
 * places artifacts in `server/target/<triple>/release/` (visible to the host).
 *
 * Note: we deliberately keep the image's default `CARGO_HOME` (`/root/.cargo`)
 * because the darwin targets rely on its bundled `config.toml` to select the
 * osxcross linker wrappers — overriding it would break macOS linking (see the
 * joseluisq/rust-linux-darwin-builder docs). The compiled `target/` artifacts
 * persist in the mounted repository, so incremental builds survive runs.
 *
 * @param {object} platform - A platform entry.
 * @param {string} tauriDir - Absolute path of the Tauri crate on the host.
 * @param {boolean} [dryRun] - When true, return the args without validating Docker.
 * @returns {string[]} Arguments to pass to the `docker` executable.
 */
export function crossBuildArgs(platform, tauriDir, dryRun = false) {
  if (!dryRun) assertDocker();
  const repoMount = toDockerMount(ROOT);
  const crateRel = path.relative(ROOT, path.resolve(tauriDir)).replace(/\\/g, '/');
  const workaround = `cd /io/${crateRel} && cargo build --release --target ${platform.target}`;

  // Isolate cargo artifacts per container architecture. Build scripts and
  // proc-macros are compiled for the *container* architecture and stored in
  // the target dir root; without this split, runs from differently-arched
  // containers (e.g. amd64 for linux-x86_64, arm64 elsewhere) would overwrite
  // each other's host artifacts in the shared workspace `target/`.
  const contArch = containerArchFor(platform);
  const wsRel = path.relative(ROOT, workspaceRootFor(path.resolve(tauriDir))).replace(/\\/g, '/');
  const dockerTargetDir = ['/io', wsRel, 'target', '.docker-build', contArch].filter(Boolean).join('/');

  const args = ['run', '--rm', '--env', `CARGO_TARGET_DIR=${dockerTargetDir}`];
  const imagePlatform = imagePlatformFor(platform);
  if (imagePlatform) args.push('--platform', imagePlatform);
  args.push('--volume', `${repoMount}:/io`, CROSS_IMAGE, 'sh', '-c', workaround);
  return args;
}

/** Prints a human-readable description of the cross build command. */
export function describeCrossBuild(platform, tauriDir) {
  return `docker ${crossBuildArgs(platform, tauriDir, true).join(' ')}`;
}

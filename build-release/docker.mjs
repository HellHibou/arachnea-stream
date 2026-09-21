// Docker cross-build orchestration for the Arachnea release tooling.
//
// Produces, inside the Arachnea cross image (build-release/docker/Dockerfile,
// derived from joseluisq/rust-linux-darwin-builder), what the current host
// cannot natively produce:
// - macOS targets (osxcross): the raw release binary (`crossBuildArgs`),
//   packaged on the host as a portable archive embedding a `.app` bundle
//   (release.mjs) since the Tauri CLI ignores macOS bundle types on Linux.
// - Linux targets: `cargo tauri build` through the in-image Tauri CLI
//   (`crossBundleArgs`), producing the .deb/.rpm/.AppImage installers AND the
//   release binary (reused by the host-side portable step). Artifacts land in
//   the mounted repository `target/docker-build/<arch>/` and are assembled
//   host-side by release.mjs.
import { existsSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createInterface } from 'node:readline/promises';
import { ROOT, canRun, run, releaseDir, loadBuildConfig } from './lib.mjs';
import { dockerBundlesFor } from './capabilities.mjs';

const RELEASE_DIR = releaseDir();

/**
 * Tag of the locally-built Arachnea cross image (single source of truth:
 * `crossImage` in `build-config.json`; bump the tag whenever the Dockerfile
 * or its build context changes, see `build-release/AGENTS.md`).
 *
 * Read lazily through a function (instead of a module-level constant) so a
 * config edit is picked up without re-importing the module.
 *
 * @returns {string} The cross image tag, e.g. `arachnea-cross-builder:1.0.0`.
 */
export function crossImage() {
  return loadBuildConfig().crossImage;
}

/** Directory holding the cross image Dockerfile. */
export const DOCKERFILE_DIR = path.join(RELEASE_DIR, 'docker');

/** Returns `true` when the `docker` CLI is available on the PATH. */
export function dockerAvailable() {
  return canRun('docker');
}

/**
 * Probes the Docker daemon. The CLI can be on the PATH while the daemon is
 * unreachable (e.g. Docker Desktop not started on Windows), in which case every
 * command fails with a named-pipe/engine connection error.
 *
 * @returns {{ ok: boolean, stderr: string, status: number|null }} Probe result;
 *   `stderr` holds the daemon's error output when it does not respond.
 */
function dockerDaemonProbe() {
  const result = spawnSync('docker', ['version', '--format', '{{.Server.Version}}'], {
    encoding: 'utf8',
    windowsHide: true,
  });
  return { ok: result.status === 0, stderr: (result.stderr ?? '').trim(), status: result.status };
}

/**
 * Prompts the user to retry when the Docker daemon is not responding. Returns
 * `true` on confirmation. In a non-interactive terminal (no TTY) the prompt is
 * skipped and `false` is returned, so callers fall back to the contextualized
 * error instead of blocking on an unanswerable question.
 *
 * @returns {Promise<boolean>} `true` when the user confirms the retry.
 */
async function confirmDockerRetry() {
  if (!process.stdin.isTTY) return false;
  const rl = createInterface({ input: process.stdin, terminal: false });
  try {
    process.stdout.write('\n[install-tools] Docker daemon is not responding. Retry? [y/N] ');
    const answer = (await rl.question('')).trim().toLowerCase();
    return answer === 'y' || answer === 'yes';
  } finally {
    rl.close();
  }
}

/**
 * Ensures the Docker daemon is reachable, prompting to retry when it is not.
 *
 * The CLI can be on the PATH while the daemon is down (e.g. Docker Desktop not
 * started on Windows): commands then fail with a named-pipe/engine connection
 * error. When the probe fails, the contextualized error is shown and the user
 * is offered to retry — useful when Docker is starting up in the background.
 * A refusal (or a non-interactive terminal) throws the same error, unchanged.
 *
 * @returns {Promise<void>}
 */
export async function assertDocker() {
  if (!dockerAvailable()) {
    throw new Error(
      'Docker is required to produce the portable Linux/macOS binaries but is not ' +
        'available on the PATH. Install Docker (e.g. Docker Desktop on Windows/macOS) and retry.',
    );
  }
  while (true) {
    const probe = dockerDaemonProbe();
    if (probe.ok) return;
    const message =
      'The Docker daemon does not respond: Docker is probably not running ' +
      '(e.g. Docker Desktop is closed). Start Docker, check it with `docker version`, then retry.\n' +
      `Original error: ${probe.stderr || `docker exited with status ${probe.status ?? 'unknown'}`}`;
    console.warn(`\n[install-tools] ${message}`);
    if (await confirmDockerRetry()) continue;
    throw new Error(message);
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
  return dockerImageRefs().has(crossImage());
}

/**
 * Container platform string a target runs in: Linux targets pin their own
 * architecture; other targets (macOS via osxcross) run a host-arch container.
 */
function effectiveImagePlatform(platform) {
  return imagePlatformFor(platform) ?? `linux/${process.arch === 'arm64' ? 'arm64' : 'amd64'}`;
}

/**
 * Ensures a locally built variant of the cross image exists for the container
 * platform a target runs in. `crossImagePresent()` only checks the tag, but an
 * amd64 and an arm64 build can share it: without this probe, docker fails with
 * a misleading "pull access denied" registry error instead of pointing at the
 * missing local variant. When the variant is missing, the user is offered to
 * build it right away (both ports through buildx when the containerd image
 * store can hold them under the tag); a refusal — or a non-interactive
 * terminal — throws the contextualized error, unchanged in spirit.
 *
 * @param {object} platform - A platform entry.
 * @throws When no local image variant matches the required container platform
 *   and the on-the-spot build was not confirmed (or did not fix it).
 */
export async function assertCrossImageFor(platform) {
  await assertDocker();
  const plat = effectiveImagePlatform(platform);
  const image = crossImage();
  // Cheap one-shot container that only succeeds when the variant is stored
  // locally; otherwise docker attempts a pull and exits non-zero right away.
  const probe = () =>
    spawnSync('docker', ['run', '--rm', '--platform', plat, image, 'true'], {
      stdio: 'ignore',
    });
  let probeResult = probe();
  if (probeResult.status === 0) return;
  if (await confirmCrossImageBuild(plat)) {
    buildCrossImage(plat);
    probeResult = probe();
    if (probeResult.status === 0) return;
  }
  throw new Error(
    `The Arachnea cross image \`${image}\` has no ${plat} variant locally ` +
      `(docker exited with status ${probeResult.status}). Build both variants from the ` +
      'repository root with:\n' +
      `  docker buildx build --platform ${CROSS_IMAGE_PLATFORMS.join(',')} --tag ${image} --build-arg TAURI_CLI_VERSION=${tauriCliVersion()} build-release/docker\n` +
      'This keeps both variants under one tag with the containerd image store (Docker ' +
      'Desktop: "Use containerd for pulling and storing images"); the classic store holds ' +
      'a single variant per tag, so build the missing one only:\n' +
      `  docker build --platform ${plat} --tag ${image} --build-arg TAURI_CLI_VERSION=${tauriCliVersion()} build-release/docker`,
  );
}

/**
 * Ensures the Arachnea cross image is built — for BOTH architecture ports
 * (`linux/amd64` and `linux/arm64`) when the image store supports it, so any
 * Linux target can run right away. Because this pulls a large base image
 * (Rust + osxcross + Apple SDK) and the arm64 half compiles its Tauri CLI
 * under QEMU emulation, callers must have obtained user confirmation first
 * (see the `install-tools.mjs` flow).
 */
export async function ensureCrossImage() {
  await assertDocker();
  if (crossImagePresent()) return;
  buildCrossImage();
}

/**
 * Both container platforms the cross image ships (see
 * `build-release/docker/Dockerfile`): a Linux target always runs a container
 * matching its own architecture, macOS targets run a host-arch container.
 */
const CROSS_IMAGE_PLATFORMS = ['linux/amd64', 'linux/arm64'];

/**
 * Resolves the Docker image storage driver (`docker info`). `overlayfs`
 * identifies the containerd image store, which can hold BOTH architecture
 * variants of the cross image under one tag; the classic store (`overlay2`)
 * keeps a single variant per tag, so building one platform replaces the other.
 *
 * @returns {string|null} The storage driver name, or `null` when unknown.
 */
function dockerStorageDriver() {
  const result = spawnSync('docker', ['info', '--format', '{{.Driver}}'], { encoding: 'utf8' });
  return result.status === 0 ? (result.stdout ?? '').trim() : null;
}

/**
 * Tauri CLI version installed inside the cross image (single source of truth:
 * `tauriCliVersion` in `build-config.json`, forwarded to the Dockerfile as
 * `TAURI_CLI_VERSION` build arg).
 *
 * @returns {string} The Tauri CLI version, e.g. `2.11.4`.
 */
export function tauriCliVersion() {
  return loadBuildConfig().tauriCliVersion;
}

/**
 * Builds the cross image for both architecture ports through buildx. With the
 * containerd image store (the expected setup) the resulting manifest list
 * keeps the `linux/amd64` and `linux/arm64` variants together under the tag.
 * On the classic image store the multi-platform export is rejected, so only
 * the requested platform (default amd64) is built — replacing any other
 * variant previously stored under the tag, which the warning makes explicit.
 *
 * @param {string|null} [singlePlatform] - When the containerd store is not
 *   available, the platform to build (`null` = `linux/amd64`).
 */
export function buildCrossImage(singlePlatform = null) {
  const image = crossImage();
  const buildArgs = ['--build-arg', `TAURI_CLI_VERSION=${tauriCliVersion()}`];
  if (dockerStorageDriver() === 'overlayfs') {
    run('docker', [
      'buildx',
      'build',
      '--platform',
      CROSS_IMAGE_PLATFORMS.join(','),
      '--tag',
      image,
      ...buildArgs,
      DOCKERFILE_DIR,
    ]);
    return;
  }
  const plat = singlePlatform ?? CROSS_IMAGE_PLATFORMS[0];
  console.warn(
    `[docker] Classic Docker image store in use: only one variant can exist per tag. ` +
      `Building the ${plat} variant of \`${image}\`; any other variant stored under ` +
      'this tag is replaced. Enable "Use containerd for pulling and storing images" in ' +
      'Docker Desktop to keep both variants side by side.',
  );
  run('docker', ['build', '--platform', plat, '--tag', image, ...buildArgs, DOCKERFILE_DIR]);
}

/**
 * Prompts the user to build the missing cross image variant on the spot.
 * Returns `true` on confirmation. In a non-interactive terminal (no TTY) the
 * prompt is skipped and `false` is returned, so callers keep failing with the
 * contextualized error instead of blocking on an unanswerable question.
 *
 * @param {string} plat - The missing container platform (e.g. `linux/arm64`).
 * @returns {Promise<boolean>} `true` when the user confirms the build.
 */
async function confirmCrossImageBuild(plat) {
  if (!process.stdin.isTTY) return false;
  const rl = createInterface({ input: process.stdin, terminal: false });
  try {
    process.stdout.write(
      `\n[release] The ${plat} variant of \`${crossImage()}\` is missing. Build it now? ` +
        'First run pulls a large base image, and the arm64 half compiles its Tauri ' +
        'CLI under QEMU emulation (can take 30-60 min). [y/N] ',
    );
    const answer = (await rl.question('')).trim().toLowerCase();
    return answer === 'y' || answer === 'yes';
  } finally {
    rl.close();
  }
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
 * Builds the common `docker run` argument prefix for a cross build: repository
 * mounted at `/io`, cargo artifacts isolated per container architecture, and
 * the container platform pinned to the Linux target triple when required.
 *
 * @param {object} platform - A platform entry.
 * @param {string} tauriDir - Absolute path of the Tauri crate on the host.
 * @returns {{ args: string[], workspaceRel: string }} Argument list built up to
 *   the image reference, plus the repo-relative workspace root path.
 */
function crossRunBase(platform, tauriDir) {
  const repoMount = toDockerMount(ROOT);
  // Isolate cargo artifacts per container architecture. Build scripts and
  // proc-macros are compiled for the *container* architecture and stored in
  // the target dir root; without this split, runs from differently-arched
  // containers (e.g. amd64 for linux-x86_64, arm64 elsewhere) would overwrite
  // each other's host artifacts in the shared workspace `target/`.
  const contArch = containerArchFor(platform);
  const workspaceRel = path.relative(ROOT, workspaceRootFor(path.resolve(tauriDir))).replace(/\\/g, '/');
  const dockerTargetDir = ['/io', workspaceRel, 'target', 'docker-build', contArch].filter(Boolean).join('/');

  const args = ['run', '--rm', '--env', `CARGO_TARGET_DIR=${dockerTargetDir}`];
  const imagePlatform = imagePlatformFor(platform);
  if (imagePlatform) args.push('--platform', imagePlatform);
  args.push('--volume', `${repoMount}:/io`);
  return { args, workspaceRel };
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
export async function crossBuildArgs(platform, tauriDir, dryRun = false) {
  if (!dryRun) await assertDocker();
  const crateRel = path.relative(ROOT, path.resolve(tauriDir)).replace(/\\/g, '/');
  const workaround = `cd /io/${crateRel} && cargo build --release --target ${platform.target}`;
  return buildCrossRunArgs(platform, tauriDir, workaround, [], dryRun);
}

/** Prints a human-readable description of the raw-binary cross build command. */
export async function describeCrossBuild(platform, tauriDir) {
  const args = await crossBuildArgs(platform, tauriDir, true);
  return `docker ${args.join(' ')}`;
}

/**
 * Resolves the raw release executable produced inside the cross image for a
 * platform, when present. Docker builds write into
 * `<workspace target>/docker-build/<container arch>/<triple>/release/` (see
 * `crossRunBase`), which `targetBaseDirs()` lists last in `lib.mjs` — an old
 * host-built binary left in the plain `target/<triple>/release/` path could
 * otherwise shadow it when assembling portable archives.
 *
 * @param {object} platform - A platform entry produced through Docker.
 * @param {string} tauriDir - Absolute path of the Tauri crate on the host.
 * @param {string} exeName - Executable file name to look for.
 * @returns {string|null} Absolute executable path, or `null` when absent.
 */
export function dockerBuildExecutable(platform, tauriDir, exeName) {
  const contArch = containerArchFor(platform);
  const workspaceRoot = workspaceRootFor(path.resolve(tauriDir));
  const candidate = path.join(
    workspaceRoot,
    'target',
    'docker-build',
    contArch,
    platform.target,
    'release',
    exeName,
  );
  return existsSync(candidate) ? candidate : null;
}

/**
 * Host-side repo-relative cache directory for the AppImage bundler tool
 * downloads (linuxdeploy/AppRun/plugins), keyed by container architecture so
 * they persist across `docker run --rm` invocations.
 */
function crossBundleCacheDir(platform) {
  const wsRel = path.relative(ROOT, workspaceRootFor(path.resolve(ROOT, 'server'))).replace(/\\/g, '/');
  return `${wsRel}/target/.tauri-bundle-cache/${containerArchFor(platform)}`;
}

/**
 * Assembles the final `docker run` arguments from the base prefix, the in-image
 * shell command and optional additional environment variables.
 */
function buildCrossRunArgs(platform, tauriDir, command, extraEnv) {
  const { args } = crossRunBase(platform, tauriDir);
  for (const [name, value] of extraEnv) args.push('--env', `${name}=${value}`);
  args.push(crossImage(), 'sh', '-c', command);
  return args;
}

/**
 * Builds the `docker run` argument list that produces the Linux installers of a
 * platform (`.deb`, `.rpm`, `.AppImage`) inside the cross image through the
 * in-image Tauri CLI (`cargo tauri bundle`). The frontend is prebuilt by the
 * host into its configured dist folder (mounted through `/io`), so the in-image
 * build skips `beforeBuildCommand`; each pass uses the already compiled release
 * binary (from `crossBuildArgs`) and produces the requested installer, which
 * lands under `<workspace target>/docker-build/<arch>/<triple>/release/bundle/`.
 *
 * One process is spawned **per bundle type** (`bundles` is a single-element
 * list in practice, see release.mjs): requesting several types in one call
 * (`bundle.targets: ["deb","rpm","appimage"]`) is unreliable — the bundler
 * patches the binary in place and re-reads it between types, crashing with
 * "Could not read binary file" on the mounted volume for the second type.
 * Using `cargo tauri bundle` instead of `build` avoids re-linking the binary
 * between bundle types, saving time.
 *
 * @param {object} platform - A platform entry.
 * @param {string} tauriDir - Absolute path of the Tauri crate on the host.
 * @param {boolean} [dryRun] - When true, return the args without validating Docker.
 * @param {string[]} [bundles] - Bundle types to produce (default: the platform's
 *   Docker-producible bundles).
 * @returns {string[]|null} Arguments to pass to the `docker` executable, or
 *   `null` when no bundles are requested.
 */
export async function crossBundleArgs(platform, tauriDir, dryRun = false, bundles = null) {
  const requested = bundles ?? dockerBundlesFor(platform);
  if (requested.length === 0) return null;
  if (!dryRun) await assertDocker();
  const crateRel = path.relative(ROOT, path.resolve(tauriDir)).replace(/\\/g, '/');
  // JSON output never contains single quotes, so shell-quoting stays safe.
  const configJson = JSON.stringify({
    build: { beforeBuildCommand: null },
    bundle: { targets: requested },
  });
  // Use `cargo tauri bundle` instead of `build` to avoid recompilation/re-linking
  // between separate bundle types; the binary is already present.
  const command =
    `cd /io/${crateRel} && cargo tauri bundle --target ${platform.target} --config '${configJson}'`;
  const extraEnv = [
    ['XDG_CACHE_HOME', '/io/' + crossBundleCacheDir(platform) + '/.cache'],
  ];
  return buildCrossRunArgs(platform, tauriDir, command, extraEnv);
}

/**
 * Prints a human-readable description of the installer cross-bundling runs, one
 * `docker run` line per bundle type.
 *
 * @returns {string|null} Newline-separated descriptions, or `null` when the
 *   target has no Docker-producible bundles.
 */
export async function describeCrossBundling(platform, tauriDir) {
  const bundles = dockerBundlesFor(platform);
  if (bundles.length === 0) return null;
  const descriptions = [];
  for (const bundle of bundles) {
    const args = await crossBundleArgs(platform, tauriDir, true, [bundle]);
    descriptions.push(`docker ${args.join(' ')}`);
  }
  return descriptions.join('\n    ');
}

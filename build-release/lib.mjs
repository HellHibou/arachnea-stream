// Shared helpers for the Arachnea release tooling.
import {
  readFileSync,
  existsSync,
  mkdirSync,
  copyFileSync,
  readdirSync,
  statSync,
  rmSync,
  writeFileSync,
  openSync,
  readSync,
  closeSync,
} from 'node:fs';
import { createHash } from 'node:crypto';
import { gunzipSync, gzipSync } from 'node:zlib';
import { spawnSync, execFileSync } from 'node:child_process';
import { createInterface } from 'node:readline/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { usableBundlesFor, hostId, normalizeSelector, needsDockerBuild, productionMethod } from './capabilities.mjs';

const RELEASE_DIR = path.dirname(fileURLToPath(import.meta.url));

/**
 * Directory holding cross-compilation command shims (e.g. a `makensis.exe`
 * wrapper around the native `makensis`, required by the Tauri NSIS bundler).
 */
export const CROSS_TOOLS_BIN_DIR = path.join(os.homedir(), '.arachnea-cross-tools', 'bin');

/** Repository root (parent of `build-release/`). */
export const ROOT = path.resolve(RELEASE_DIR, '..');

/** Path of the release tooling directory. */
export const releaseDir = () => RELEASE_DIR;

/** Resolves a config-relative path against the release directory. */
export const resolveFromRelease = (p) => path.resolve(RELEASE_DIR, p);

/** Resolves a config-relative path against the repository root. */
export const resolveFromRoot = (p) => path.resolve(ROOT, p);

/** Loads `release-config.json`. */
export function loadConfig() {
  return JSON.parse(readFileSync(path.join(RELEASE_DIR, 'release-config.json'), 'utf8'));
}

/**
 * Reads the application version from the workspace `[workspace.package]`
 * section of `server/Cargo.toml` (single source of truth).
 *
 * @returns {string} The semantic version, e.g. `0.1.0`.
 */
export function parseWorkspaceVersion() {
  const config = loadConfig();
  const text = readFileSync(resolveFromRelease(config.versionSource), 'utf8');
  for (const section of text.split(/^\s*\[/m)) {
    if (section.startsWith('workspace.package]')) {
      const match = section.match(/^\s*version\s*=\s*"([^"]+)"/m);
      if (match) return match[1];
    }
  }
  throw new Error('Could not find `[workspace.package] version` in server/Cargo.toml');
}

/**
 * Tests a platform id against a selector. Accepted forms (after alias
 * normalization, e.g. `osx` -> `darwin`):
 * - a `*` wildcard pattern (`darwin-*`),
 * - an exact platform id (`darwin-arm64`),
 * - a bare family name matching every platform of that family
 *   (`darwin` matches `darwin-arm64`, `darwin-universal`, ...).
 *
 * @param {string} id - Platform identifier.
 * @param {string} selector - Exact id, family name, or wildcard pattern.
 * @returns {boolean} `true` when the id matches.
 */
function matchesSelector(id, selector) {
  if (selector.includes('*')) {
    const regex = new RegExp(
      `^${selector.split('*').map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('.*')}$`,
    );
    return regex.test(id);
  }
  return id === selector || id.startsWith(`${selector}-`);
}

/**
 * Merges common and platform-specific portable option lists, preserving order
 * and dropping duplicates (common values first, platform values after).
 *
 * @param {Array<string>} [common] - Common `portable` list from the config root.
 * @param {Array<string>} [local] - Platform-level `portable` list.
 * @returns {string[]} Merged, de-duplicated list.
 */
function mergePortableList(common = [], local = []) {
  return [...new Set([...common, ...local])];
}

/**
 * Enriches platform entries with the bundles usable on this host.
 *
 * @param {object} config - Loaded release configuration.
 * @param {string[]} [selectors] - When non-empty, keep only platforms whose id
 *   matches one of the selectors. Selectors are exact ids, bare family names
 *   (`darwin`, `windows`, `linux`, and aliases like `osx`), or `*` patterns
 *   like `darwin-*`. They may repeat or be comma-separated upstream.
 * @param {object} [options] - Resolution options.
 * @param {boolean} [options.forceUseDockerBuilder] - Route every Docker-capable
 *   platform through the cross-build image even when this host could produce
 *   it natively (targets not managed by the image stay native).
 * @returns {object[]} Platform entries with `usable`, `runner`, `needsTargets`
 *   and `buildable` fields added, in configuration order. Portable platforms
 *   get their `portable.include`/`portable.exclude` merged with the common
 *   `config.portable.include`/`.exclude` lists (platform values appended).
 */
export function resolvePlatforms(config, selectors = [], options = {}) {
  const platforms = config.platforms.map((platform) => {
    const { usable, runner, needsTargets } = usableBundlesFor(platform, platform.bundles);
    const method = productionMethod(platform, usable, options.forceUseDockerBuilder === true);
    let portable = platform.portable;
    if (portable) {
      const common = config.portable ?? {};
      portable = {
        ...portable,
        include: mergePortableList(common.include, portable.include),
        exclude: mergePortableList(common.exclude, portable.exclude),
      };
    }
    return {
      ...platform,
      usable,
      runner,
      needsTargets,
      method,
      needsDockerBuild: needsDockerBuild(platform),
      buildable: method !== 'none',
      portable,
    };
  });

  const normalized = (selectors ?? []).map((selector) => normalizeSelector(selector.trim()));
  if (normalized.length === 0) return platforms;

  const selected = new Set();
  const unmatched = [];
  for (const selector of normalized) {
    const matched = platforms.filter((platform) => matchesSelector(platform.id, selector));
    if (matched.length === 0) unmatched.push(selector);
    for (const platform of matched) selected.add(platform);
  }
  if (unmatched.length > 0) {
    const known = platforms.map((p) => p.id).join(', ');
    throw new Error(`No platform matches: ${unmatched.join(', ')}. Known platforms: ${known}`);
  }
  return platforms.filter((platform) => selected.has(platform));
}

/** Current host label for logging. */
export function hostLabel() {
  return hostId() ?? process.platform;
}

/** Runs a command synchronously, inheriting stdio. */
export function run(cmd, args, opts = {}) {
  console.log(`\n$ ${cmd} ${args.join(' ')}`);
  const result = spawnSync(cmd, args, { stdio: 'inherit', ...opts });
  if (result.error) throw new Error(`Failed to start ${cmd}: ${result.error.message}`);
  if (result.status !== 0) {
    throw new Error(`Command failed (exit ${result.status}): ${cmd} ${args.join(' ')}`);
  }
  return result;
}

/** Returns `true` when the given command exists on the PATH. */
export function canRun(cmd) {
  const where = process.platform === 'win32' ? 'where' : 'which';
  return spawnSync(where, [cmd], { stdio: 'ignore' }).status === 0;
}

/**
 * Resolves the absolute path of a command available on the PATH.
 *
 * @param {string} cmd - Command name to resolve.
 * @returns {string|null} Absolute path, or `null` when not found.
 */
export function commandPath(cmd) {
  const where = process.platform === 'win32' ? 'where' : 'which';
  const result = spawnSync(where, [cmd], { encoding: 'utf8' });
  if (result.status !== 0 || !result.stdout) return null;
  const first = result.stdout.trim().split(/\r?\n/)[0];
  return first || null;
}

/**
 * Loads `build-config.json` (single source of truth for toolchain floors and
 * build tool versions).
 */
export function loadBuildConfig() {
  return JSON.parse(readFileSync(path.join(RELEASE_DIR, 'build-config.json'), 'utf8'));
}

/**
 * Minimum rustc version required to compile the workspace dependency graph.
 * Bumped when a locked dependency raises its MSRV (e.g. `foyer@0.22.4+`
 * requires rustc 1.91.0 while earlier 0.22.x releases built with 1.85.0).
 *
 * @returns {string} Minimum `major.minor.patch` rustc version.
 */
export function minRustcVersion() {
  return loadBuildConfig().minRustcVersion;
}

/**
 * Compares two `major.minor.patch` version strings.
 *
 * @param {string} a - First version.
 * @param {string} b - Second version.
 * @returns {number} Negative when a < b, 0 when equal, positive when a > b.
 */
export function compareVersions(a, b) {
  const parts = (v) => v.split('.').map((n) => parseInt(n, 10) || 0);
  const [aParts, bParts] = [parts(a), parts(b)];
  for (let i = 0; i < Math.max(aParts.length, bParts.length); i += 1) {
    const diff = (aParts[i] ?? 0) - (bParts[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

/**
 * Reads the active rustc version (e.g. `rustc 1.90.0 (...)` -> `1.90.0`).
 *
 * @returns {string} The installed rustc version.
 */
export function rustcVersion() {
  if (!canRun('rustc')) {
    throw new Error('`rustc` is not installed. Install Rust first: https://rustup.rs');
  }
  const output = execFileSync('rustc', ['--version'], { encoding: 'utf8' }).trim();
  const match = output.match(/^rustc\s+(\d+\.\d+\.\d+)/);
  if (!match) throw new Error(`Could not parse rustc version from: ${output}`);
  return match[1];
}

/**
 * Asks a yes/no question on the terminal, following the `install-tools`
 * confirmation pattern (readline instead of a raw stdin read, explicit prompt
 * through stdout since readline ignores its own prompt without `terminal`).
 *
 * @param {string} question - Question to display (without the `[y/N]` suffix).
 * @returns {Promise<boolean>} `true` when the user answers `y`/`yes`.
 */
export async function confirmYesNo(question) {
  if (!process.stdin.isTTY) return false;
  const rl = createInterface({ input: process.stdin, terminal: false });
  try {
    process.stdout.write(`\n${question} [y/N] `);
    const answer = (await rl.question('')).trim().toLowerCase();
    return answer === 'y' || answer === 'yes';
  } finally {
    rl.close();
  }
}

/**
 * Ensures the active rustc meets the floor declared in `build-config.json`,
 * before any lengthy build step (frontend build, `cargo tauri build`) runs.
 * When the toolchain is outdated, prints the version error, then offers to
 * run `rustup update` (same confirmation pattern as `install-tools`): on
 * acceptance the update runs and the new version is re-checked; on refusal
 * (or in a non-interactive terminal) the original error is thrown unchanged.
 */
export async function ensureRustcVersion() {
  const { minRustcVersion: minimum } = loadBuildConfig();
  const outdated = () => compareVersions(rustcVersion(), minimum) < 0;
  if (!outdated()) return;
  const current = rustcVersion();
  if (await confirmYesNo(
    `[release] rustc ${current} is too old: this workspace requires rustc >= ${minimum}.\n` +
      'Run `rustup update` now to update the Rust toolchain?',
  )) {
    run('rustup', ['update']);
    if (!outdated()) return;
  }
  throw new Error(
    `rustc ${current} is too old: this workspace requires rustc >= ${minimum}. ` +
      `Update with \`rustup update\`, then rebuild.`,
  );
}

/** Recursively copies a directory tree, skipping excluded top-level entries. */
export function copyDir(src, dst, excluded = []) {
  if (!existsSync(src)) throw new Error(`Source directory not found: ${src}`);
  mkdirSync(dst, { recursive: true });
  for (const entry of readdirSync(src)) {
    if (excluded.includes(entry)) continue;
    const from = path.join(src, entry);
    const to = path.join(dst, entry);
    if (statSync(from).isDirectory()) {
      copyDir(from, to, []);
    } else {
      mkdirSync(path.dirname(to), { recursive: true });
      copyFileSync(from, to);
    }
  }
}

/** Copies a single file or a `.app`-style directory tree. */
export function copyArtifact(from, to) {
  mkdirSync(path.dirname(to), { recursive: true });
  if (statSync(from).isDirectory()) {
    copyDir(from, to, []);
  } else {
    copyFileSync(from, to);
  }
}

/**
 * Creates a `.zip` archive from a source directory using the best tool
 * available on the host (`zip`, `ditto` on macOS, PowerShell on Windows).
 *
 * @param {string} srcDir - Directory whose content is archived.
 * @param {string} zipPath - Absolute destination archive path.
 */
export function createZip(srcDir, zipPath) {
  mkdirSync(path.dirname(zipPath), { recursive: true });
  if (canRun('zip')) {
    run('zip', ['-r', zipPath, '.'], { cwd: srcDir });
    return;
  }
  if (process.platform === 'darwin' && canRun('ditto')) {
    run('ditto', ['-c', '-k', srcDir, zipPath]);
    return;
  }
  if (process.platform === 'win32') {
    const command = [
      'powershell',
      '-NoProfile',
      '-Command',
      `Compress-Archive -Path '${srcDir}\\*' -DestinationPath '${zipPath}' -Force`,
    ];
    run(command[0], command.slice(1));
    return;
  }
  throw new Error('No zip tool available. Run `node release/install-tools.mjs` or install `zip`.');
}

/**
 * Reads a NUL-terminated (or space-padded) string field of a tar header.
 * Latin-1 keeps the bytes intact for names that are not valid UTF-8, which is
 * all the archive-relative paths compared here need.
 *
 * @param {Uint8Array} header - The 512-byte header block.
 * @param {number} offset - Field offset inside the block.
 * @param {number} length - Field length in bytes.
 * @returns {string} The decoded field, without its terminator.
 */
function tarField(header, offset, length) {
  const field = Buffer.from(header.subarray(offset, offset + length));
  const end = field.indexOf(0);
  return (end === -1 ? field : field.subarray(0, end)).toString('latin1');
}

/**
 * Normalizes the POSIX modes recorded in a `.tar.gz` built on a host whose
 * filesystem cannot express them.
 *
 * Windows has no POSIX permission bits, so the `bsdtar` shipped with it records
 * every entry as `0666` (files) or `0777` (directories) — including the release
 * executable, which then fails `test -x` in `arachnea-docker`'s build and is not
 * runnable for anyone extracting the archive on Linux. Windows' `bsdtar` also
 * rejects `--mode`, so the modes are rewritten in the archive itself. Files are
 * pinned to `0644` and directories to `0755` — the values macOS and GNU tar
 * record — so the produced archive no longer depends on the build host, and
 * every path listed in `executables` gets `0755`.
 *
 * Entries are rewritten in place; the trailing padding is left untouched.
 *
 * @param {string} tarPath - Absolute path of the `.tar.gz` to rewrite.
 * @param {string[]} executables - Archive-relative paths that must be
 *   executable (leading `./` is ignored).
 * @throws {Error} When one of `executables` is missing from the archive.
 */
export function normalizeTarGzModes(tarPath, executables = []) {
  const wanted = new Set(
    executables.map((entry) => entry.replace(/^\.\//, '').replace(/\/+$/, '')),
  );
  const found = new Set();
  const archive = gunzipSync(readFileSync(tarPath));

  for (let offset = 0; offset + 512 <= archive.length; ) {
    const header = archive.subarray(offset, offset + 512);
    // Two zeroed blocks mark the end of the entry list.
    if (header.every((byte) => byte === 0)) break;
    const name = tarField(header, 0, 100);
    const prefix = tarField(header, 345, 155);
    const entry = (prefix ? `${prefix}/${name}` : name).replace(/^\.\//, '').replace(/\/+$/, '');
    const type = String.fromCharCode(header[156] || 0x30);
    const size = parseInt(tarField(header, 124, 12).trim() || '0', 8);
    if (type === '0' || type === '5') {
      const executable = wanted.has(entry);
      if (executable) found.add(entry);
      const mode = type === '5' || executable ? 0o755 : 0o644;
      header.write(`${mode.toString(8).padStart(7, '0')}\0`, 100, 8, 'latin1');
      // The checksum field must be blanked before the sum is computed, then
      // written as the conventional `6 octal digits + NUL + space`.
      header.fill(0x20, 148, 156);
      let sum = 0;
      for (const byte of header) sum += byte;
      header.write(`${sum.toString(8).padStart(6, '0')}\0 `, 148, 8, 'latin1');
    }
    offset += 512 + Math.ceil(size / 512) * 512;
  }

  const missing = [...wanted].filter((entry) => !found.has(entry));
  if (missing.length > 0) {
    throw new Error(
      `Cannot mark ${missing.join(', ')} executable in ${tarPath}: no matching entry ` +
        'in the archive. The staged release tree does not match the expected layout.',
    );
  }
  writeFileSync(tarPath, gzipSync(archive));
}

/**
 * Creates a `.tar.gz` archive from a source directory using `tar` (bsdtar on
 * Windows/macOS, GNU tar elsewhere), which is available on every supported
 * host. Used for portable Linux/macOS builds produced through Docker.
 *
 * The modes are normalized afterwards (see `normalizeTarGzModes`), because
 * bsdtar under Windows stores none of them.
 *
 * @param {string} srcDir - Directory whose content is archived (relative paths).
 * @param {string} tarPath - Absolute destination `.tar.gz` archive path.
 * @param {string[]} [executables] - Archive-relative paths to mark executable.
 */
export function createTarGz(srcDir, tarPath, executables = []) {
  mkdirSync(path.dirname(tarPath), { recursive: true });
  if (!canRun('tar')) throw new Error('`tar` is required to create the portable archive.');
  run('tar', ['-czf', tarPath, '.'], { cwd: srcDir });
  normalizeTarGzModes(tarPath, executables);
}

/**
 * Creates a portable archive whose format depends on the file extension:
 * `.tar.gz` uses `createTarGz`, anything else uses `createZip`.
 *
 * @param {string} srcDir - Directory whose content is archived.
 * @param {string} archivePath - Absolute destination archive path.
 * @param {string[]} [executables] - Archive-relative paths to mark executable
 *   (`.tar.gz` only; a `.zip` stores the mode differently and is not normalized).
 */
export function createArchive(srcDir, archivePath, executables = []) {
  if (archivePath.endsWith('.tar.gz')) {
    createTarGz(srcDir, archivePath, executables);
    return;
  }
  createZip(srcDir, archivePath);
}

/**
 * Lists the cargo output directories to search for a Tauri project: its own
 * `target/` folder and, when built inside a workspace, the workspace root
 * `target/` folder (where cargo actually places cross-target builds).
 *
 * @param {string} tauriDir - Absolute path of the Tauri crate.
 * @returns {string[]} Candidate `target/` directories.
 */
export function targetBaseDirs(tauriDir) {
  const bases = new Set([path.join(tauriDir, 'target')]);
  let dir = path.dirname(tauriDir);
  while (true) {
    // First parent owning a Cargo.toml is the workspace root.
    if (existsSync(path.join(dir, 'Cargo.toml'))) {
      bases.add(path.join(dir, 'target'));
      break;
    }
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  // Docker cross builds write into `<workspace target>/docker-build/<arch>/`
  // (one sub-directory per container architecture, see docker.mjs); include
  // the ones that already exist so bundle/exe lookups find those artifacts.
  for (const base of [...bases]) {
    const splitRoot = path.join(base, 'docker-build');
    if (!existsSync(splitRoot)) continue;
    for (const entry of readdirSync(splitRoot)) {
      bases.add(path.join(splitRoot, entry));
    }
  }
  return [...bases];
}

/**
 * Lists the bundle directories searched for a given bundle type, for
 * diagnostics when no artifact is found.
 *
 * @param {string} tauriDir - Absolute path of the Tauri crate.
 * @param {string} target - Rust target triple.
 * @param {string} bundleType - Bundle type (`dmg`, `nsis`, `msi`, ...).
 * @returns {string[]} Existing-or-not directories that will be searched.
 */
export function bundleSearchDirs(tauriDir, target, bundleType) {
  const dirs = [];
  for (const base of targetBaseDirs(tauriDir)) {
    dirs.push(path.join(base, target, 'release', 'bundle', bundleType));
    dirs.push(path.join(base, 'release', 'bundle', bundleType));
  }
  return dirs;
}

/**
 * Finds build artifacts of a bundle type in the cargo/Tauri bundle outputs.
 *
 * @param {string} tauriDir - Absolute path of the Tauri crate.
 * @param {string} target - Rust target triple.
 * @param {string} bundleType - Bundle type (`dmg`, `nsis`, `msi`, ...).
 * @returns {string[]} Matching artifact paths, newest first.
 */
export function findArtifacts(tauriDir, target, bundleType) {
  const roots = bundleSearchDirs(tauriDir, target, bundleType);
  const extensions = {
    app: '.app',
    dmg: '.dmg',
    msi: '.msi',
    nsis: '.exe',
    deb: '.deb',
    rpm: '.rpm',
    appimage: '.AppImage',
  };
  const suffix = extensions[bundleType];
  const found = [];
  for (const root of roots) {
    if (!existsSync(root)) continue;
    for (const entry of readdirSync(root)) {
      const full = path.join(root, entry);
      if (bundleType === 'app' && statSync(full).isDirectory() && entry.endsWith('.app')) {
        found.push(full);
        continue;
      }
      if (statSync(full).isFile() && entry.endsWith(suffix)) found.push(full);
    }
  }
  found.sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
  return found;
}

/**
 * Finds the raw release binary of the application for a target.
 *
 * @param {string} tauriDir - Absolute path of the Tauri crate.
 * @param {string} target - Rust target triple.
 * @param {string} exeName - Expected executable file name.
 * @returns {string|null} Executable path or `null`.
 */
export function findExecutable(tauriDir, target, exeName) {
  for (const base of targetBaseDirs(tauriDir)) {
    const candidates = [path.join(base, target, 'release', exeName), path.join(base, 'release', exeName)];
    for (const full of candidates) {
      if (existsSync(full)) return full;
    }
  }
  return null;
}

/**
 * Computes the lowercase hexadecimal digest of a file with the given Node
 * algorithm (`sha256`, `md5`, ...), streaming it in chunks to keep memory use
 * bounded for large installer bundles.
 *
 * @param {string} filePath - Absolute path of the file to hash.
 * @param {string} algorithm - Node crypto digest algorithm.
 * @returns {string} Hex digest of the requested length.
 */
export function fileDigestHex(filePath, algorithm) {
  const fd = openSync(filePath, 'r');
  const hash = createHash(algorithm);
  const buffer = Buffer.alloc(1024 * 1024);
  try {
    let bytesRead;
    while ((bytesRead = readSync(fd, buffer, 0, buffer.length, null)) > 0) {
      hash.update(buffer.subarray(0, bytesRead));
    }
  } finally {
    closeSync(fd);
  }
  return hash.digest('hex');
}

/** SHA-256 hex digest of a file (see `fileDigestHex`). */
export function sha256Hex(filePath) {
  return fileDigestHex(filePath, 'sha256');
}

/** MD5 hex digest of a file (see `fileDigestHex`). */
export function md5Hex(filePath) {
  return fileDigestHex(filePath, 'md5');
}

/**
 * Writes a `sha256sum`- or `md5sum`-compatible checksum file next to the given
 * file (`<file>.sha256` or `<file>.md5`) containing `<hash>  <basename>\n`.
 *
 * @param {string} filePath - Absolute path of the artifact to checksum.
 * @param {string} [algorithm] - `sha256` (default) or `md5`.
 * @returns {string} The path of the generated checksum file.
 */
export function writeChecksum(filePath, algorithm = 'sha256') {
  const hash = fileDigestHex(filePath, algorithm);
  const extension = algorithm === 'md5' ? 'md5' : 'sha256';
  const checksumPath = `${filePath}.${extension}`;
  writeFileSync(checksumPath, `${hash}  ${path.basename(filePath)}\n`);
  return checksumPath;
}

/** Common Homebrew LLVM and LLD bin directories used by cargo-xwin. */
const LLVM_BIN_DIRS = [
  '/opt/homebrew/opt/llvm/bin',
  '/usr/local/opt/llvm/bin',
  '/opt/homebrew/opt/lld/bin',
  '/usr/local/opt/lld/bin',
];

/**
 * Returns the process environment augmented with:
 * - the common Homebrew LLVM directories and the cross-tools shim directory on
 *   PATH (so `cargo-xwin` finds its LLVM tools and the NSIS bundler finds its
 *   `makensis.exe` shim without requiring a shell profile change);
 * - on Windows hosts with Visual Studio installed, the VS-provided LLVM bin
 *   (for `clang-cl`, required by `ring`/`aws-lc-sys` when building
 *   `aarch64-pc-windows-msvc`), the VS CMake Ninja directory (`ninja.exe`) and
 *   the MSVC toolchain bin dirs. Everything is optional: only existing
 *   directories are prepended.
 *
 * @returns {object} Environment suitable for `spawnSync`.
 */
export function buildEnvWithLlvm() {
  const pathSeparator = process.platform === 'win32' ? ';' : ':';
  const extraDirs = [...LLVM_BIN_DIRS, CROSS_TOOLS_BIN_DIR];

  if (process.platform === 'win32') {
    const vs = findVsInstallation();
    if (vs && vs.installationPath) {
      const toolset = msvcToolsetDir(vs.installationPath);
      if (toolset) {
        for (const host of ['Hostx64', 'Hostarm64']) {
          extraDirs.push(path.join(toolset, 'bin', host, 'arm64'));
          extraDirs.push(path.join(toolset, 'bin', host, 'x64'));
        }
      }
      const llvmBin = path.join(vs.installationPath, 'VC', 'Tools', 'Llvm', 'x64', 'bin');
      if (existsSync(llvmBin)) extraDirs.push(llvmBin);
      const ninjaDir = path.join(
        vs.installationPath,
        'Common7',
        'IDE',
        'CommonExtensions',
        'Microsoft',
        'CMake',
        'Ninja',
      );
      if (existsSync(ninjaDir)) extraDirs.push(ninjaDir);
    }
  }

  const present = extraDirs.filter((directory) => existsSync(directory));
  const currentEntries = (process.env.PATH || '').split(pathSeparator);
  const missing = present.filter((dir) => !currentEntries.includes(dir));
  const env = { ...process.env };
  if (missing.length === 0) return env;
  return { ...env, PATH: `${missing.join(pathSeparator)}${pathSeparator}${process.env.PATH}` };
}

/** Removes a directory tree if it exists. */
export function removeDir(dir) {
  if (existsSync(dir)) rmSync(dir, { recursive: true, force: true });
}

/**
 * Resolves the Visual Studio Installer `vswhere.exe` on a Windows host.
 *
 * @returns {string|null} Absolute path of `vswhere.exe`, or `null` when the
 *   Visual Studio Installer is not present.
 */
export function vsWhereExe() {
  if (process.platform !== 'win32') return null;
  const base = process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)';
  const candidate = path.join(base, 'Microsoft Visual Studio', 'Installer', 'vswhere.exe');
  return existsSync(candidate) ? candidate : null;
}

/**
 * Locates a Visual Studio installation through `vswhere`. Used to find the
 * MSVC toolset, LLVM/Clang and Ninja shipped inside Visual Studio so the
 * release builder can provision and use them.
 *
 * @returns {{ installationPath: string, installationVersion: string, instanceId: string }|null}
 *   The first VS instance found, or `null` when none is installed.
 */
export function findVsInstallation() {
  const vsWhere = vsWhereExe();
  if (!vsWhere) return null;
  const result = spawnSync(vsWhere, ['-all', '-products', '*', '-format', 'json'], { encoding: 'utf8' });
  if (result.status !== 0) return null;
  try {
    const list = JSON.parse(result.stdout);
    const instance = (list ?? []).find((entry) => entry && entry.installationPath);
    if (!instance) return null;
    return {
      installationPath: instance.installationPath,
      installationVersion: instance.installationVersion || '',
      instanceId: instance.instanceId || '',
    };
  } catch {
    return null;
  }
}

/**
 * Returns the most recent MSVC toolset directory of a Visual Studio
 * installation (e.g. `...\VC\Tools\MSVC\14.44.35207`).
 *
 * @param {string} vsPath - Visual Studio installation path.
 * @returns {string|null} The toolset directory, or `null` when none exists.
 */
export function msvcToolsetDir(vsPath) {
  const root = path.join(vsPath, 'VC', 'Tools', 'MSVC');
  if (!existsSync(root)) return null;
  const versions = readdirSync(root)
    .filter((name) => /^\d+\.\d+/.test(name))
    .sort((a, b) => {
      const va = a.split('.').map(Number);
      const vb = b.split('.').map(Number);
      for (let i = 0; i < Math.max(va.length, vb.length); i += 1) {
        const d = (va[i] ?? 0) - (vb[i] ?? 0);
        if (d !== 0) return d;
      }
      return 0;
    })
    .reverse();
  return versions.length ? path.join(root, versions[0]) : null;
}

/**
 * Detects whether the Windows host can compile C/C++ for the
 * `aarch64-pc-windows-msvc` Rust target natively. Two pieces must exist:
 * - the **MSVC ARM64 build tools** (`Hostx64\arm64\cl.exe` plus the `arm64`
 *   MSVC libraries) so `rustc` can link against the MSVC CRT;
 * - **clang-cl** (the "C++ Clang Compiler for Windows" component inside VS or
 *   an otherwise installed LLVM), because `ring` and `aws-lc-sys` require it
 *   to assemble/compile C for Windows ARM64.
 *
 * @returns {{ ok: boolean, missing: string[], vs: (object|null), toolset: (string|null), clangClPath: (string|null), msvcArm64ClPath: (string|null) }}
 */
export function windowsNativeMsvcArm64State() {
  const missing = [];
  const vs = findVsInstallation();
  let toolset = null;
  let clangClPath = null;
  let msvcArm64ClPath = null;

  if (vs && vs.installationPath) {
    toolset = msvcToolsetDir(vs.installationPath);
    if (!toolset) {
      missing.push('MSVC build tools (x64/x86)');
    } else {
      const clHostX64Arm64 = path.join(toolset, 'bin', 'Hostx64', 'arm64', 'cl.exe');
      const libArm64 = path.join(toolset, 'lib', 'arm64');
      if (!existsSync(clHostX64Arm64) || !existsSync(libArm64)) {
        missing.push('MSVC ARM64 build tools');
      } else {
        msvcArm64ClPath = clHostX64Arm64;
      }
    }
    const clangCl = path.join(vs.installationPath, 'VC', 'Tools', 'Llvm', 'x64', 'bin', 'clang-cl.exe');
    if (!existsSync(clangCl)) {
      missing.push('C++ Clang Compiler for Windows (clang-cl)');
    } else {
      clangClPath = clangCl;
    }
  } else {
    missing.push('Visual Studio Build Tools');
  }

  return { ok: missing.length === 0, missing, vs, toolset, clangClPath, msvcArm64ClPath };
}

/**
 * Detects whether the current Node process is running with Windows elevated
 * (Administrator) privileges.
 *
 * @returns {boolean} `true` when elevated on Windows.
 */
export function isElevated() {
  if (process.platform !== 'win32') return false;
  try {
    execFileSync('net', ['session'], { stdio: 'ignore', windowsHide: true });
    return true;
  } catch {
    return false;
  }
}

/**
 * Re-runs the given command with Windows elevation (UAC prompt) and waits for
 * the elevated process to finish, inheriting stdio. Used to launch the Visual
 * Studio installer, which requires elevation for `--quiet`/`--passive` (an
 * unelevated run fails with exit 5007 "should be run elevated from the
 * beginning").
 *
 * @param {string} exe - Absolute path of the executable to elevate.
 * @param {string[]} args - Command-line arguments.
 * @returns {number} Exit code of the elevated process.
 */
export function runElevatedSync(exe, args) {
  // `Start-Process -ArgumentList` with a PowerShell array joins the elements
  // without re-quoting, so paths containing spaces (e.g.
  // "C:\Program Files (x86)\...") would be split into separate arguments. The
  // reliable form is a single command-line string with every token that
  // contains whitespace wrapped in double quotes. The whole argument string is
  // then single-quoted for PowerShell (double single quotes escape one).
  const quoteToken = (token) => (/[\s"]/).test(token) ? `"${token.replace(/"/g, '""')}"` : token;
  const argString = args.map(quoteToken).join(' ');
  const psQuotedArgs = `'${argString.replace(/'/g, "''")}'`;
  // `Start-Process -Verb RunAs` triggers the UAC prompt. `-Wait` is unreliable
  // with `-Verb RunAs` (it can return before the elevated process exits), so
  // completion is polled with `HasExited` on the process handle returned by
  // `-PassThru` instead. `$ErrorActionPreference = 'Stop'` + a catch turns a
  // refused/cancelled elevation into a non-zero exit instead of `$p` staying
  // undefined.
  const script =
    `$ErrorActionPreference = 'Stop'; ` +
    `try { ` +
    `$p = Start-Process -FilePath ${JSON.stringify(exe)} -ArgumentList ${psQuotedArgs} ` +
    `-Verb RunAs -PassThru -ErrorAction Stop; ` +
    `while (-not $p.HasExited) { Start-Sleep -Milliseconds 500; $p.Refresh() } ` +
    `exit $p.ExitCode ` +
    `} catch { Write-Error $_.Exception.Message; exit 1 }`;
  const result = spawnSync(
    'powershell',
    ['-NoProfile', '-NonInteractive', '-WindowStyle', 'Hidden', '-Command', script],
    { stdio: 'inherit', windowsHide: true },
  );
  if (result.error) throw new Error(`Failed to elevate ${exe}: ${result.error.message}`);
  return result.status ?? 0;
}

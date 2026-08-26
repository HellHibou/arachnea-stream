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
import { spawnSync } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { usableBundlesFor, hostId, normalizeSelector } from './capabilities.mjs';

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
 * Enriches platform entries with the bundles usable on this host.
 *
 * @param {object} config - Loaded release configuration.
 * @param {string[]} [selectors] - When non-empty, keep only platforms whose id
 *   matches one of the selectors. Selectors are exact ids, bare family names
 *   (`darwin`, `windows`, `linux`, and aliases like `osx`), or `*` patterns
 *   like `darwin-*`. They may repeat or be comma-separated upstream.
 * @returns {object[]} Platform entries with `usable`, `runner`, `needsTargets`
 *   and `buildable` fields added, in configuration order.
 */
export function resolvePlatforms(config, selectors = []) {
  const platforms = config.platforms.map((platform) => {
    const { usable, runner, needsTargets } = usableBundlesFor(platform, platform.bundles);
    return { ...platform, usable, runner, needsTargets, buildable: usable.length > 0 };
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
 * Returns the process environment augmented with the common Homebrew LLVM
 * directories and the cross-tools shim directory on PATH (when present), so
 * `cargo-xwin` finds its LLVM tools and the NSIS bundler finds its
 * `makensis.exe` shim without requiring a shell profile change.
 *
 * @returns {object} Environment suitable for `spawnSync`.
 */
export function buildEnvWithLlvm() {
  const pathSeparator = process.platform === 'win32' ? ';' : ':';
  const extraDirs = [...LLVM_BIN_DIRS, CROSS_TOOLS_BIN_DIR].filter((directory) =>
    existsSync(directory),
  );
  const currentEntries = (process.env.PATH || '').split(pathSeparator);
  const missing = extraDirs.filter((dir) => !currentEntries.includes(dir));
  const env = { ...process.env };
  if (missing.length === 0) return env;
  return { ...env, PATH: `${missing.join(pathSeparator)}${pathSeparator}${process.env.PATH}` };
}

/** Removes a directory tree if it exists. */
export function removeDir(dir) {
  if (existsSync(dir)) rmSync(dir, { recursive: true, force: true });
}

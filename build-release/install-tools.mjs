// Toolchain installer for the Arachnea cross-platform release build.
//
// Ensures everything needed to build the requested platforms on the current
// host is available:
// - rustup targets (standard library for each target triple),
// - `cargo-xwin` (Windows MSVC linker/runner for non-Windows hosts),
// - Linux system packages required by Tauri/WebKitGTK bundles,
// - `zip` for the Windows portable archive.
//
// Usage:
//   node release/install-tools.mjs                 # all locally buildable platforms
//   node release/install-tools.mjs --platform <id> # tools for one platform
import { chmodSync, existsSync, mkdirSync, readSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { canRun, commandPath, run, loadConfig, resolvePlatforms, hostLabel, CROSS_TOOLS_BIN_DIR } from './lib.mjs';

function parseArgs(argv) {
  const options = { platforms: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--platform' || arg === '-p') {
      // Accept repeated flags and comma-separated selectors (`darwin-*` etc.).
      for (const part of argv[++i].split(',')) {
        const selector = part.trim();
        if (selector) options.platforms.push(selector);
      }
    } else if (arg === '--help' || arg === '-h') {
      options.help = true;
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }
  return options;
}

/** Prints the tool-install help text. */
function help() {
  console.log(`Usage: node build-release/install-tools.mjs [OPTIONS]

Installs the tools required to build the Arachnea release bundles on the
current host (${hostLabel()}):

  --platform, -p <sel>  Install tools only for matching platforms. Selectors
                        are exact ids (darwin-arm64), family names (darwin,
                        windows, linux, alias osx = darwin), or \`*\` patterns
                        (darwin-*). Repeat the flag or separate values with
                        commas. Default: all locally buildable platforms.
  --help, -h            Show this help.
`);
}

/** Requests confirmation before changing the host toolchain. */
function confirmInstallation(description) {
  if (!process.stdin.isTTY) {
    throw new Error(
      `Cannot install ${description} without confirmation in a non-interactive terminal. ` +
        'Run the command interactively.',
    );
  }

  process.stdout.write(`\n[install-tools] Install ${description}? [y/N] `);
  const buffer = Buffer.alloc(1024);
  const length = readSync(0, buffer, 0, buffer.length, null);
  const answer = buffer.toString('utf8', 0, length).trim().toLowerCase();
  if (answer !== 'y' && answer !== 'yes') {
    throw new Error(`Installation cancelled: ${description}.`);
  }
}

/** Installs rust targets for the given platform entries. */
function installRustTargets(platforms) {
  const targets = [...new Set(platforms.flatMap((p) => p.needsTargets))];
  if (targets.length === 0) return;
  if (!canRun('rustup')) throw new Error('`rustup` is not installed. Install Rust first: https://rustup.rs');
  const installed = spawnSync('rustup', ['target', 'list', '--installed'], { encoding: 'utf8' });
  if (installed.status !== 0) throw new Error('Could not list installed Rust targets.');
  const known = new Set(installed.stdout.trim().split(/\r?\n/).filter(Boolean));
  const missing = targets.filter((target) => !known.has(target));
  if (missing.length === 0) {
    console.log('\n[install-tools] Required Rust targets are already installed.');
    return;
  }
  confirmInstallation(`Rust targets: ${missing.join(', ')}`);
  run('rustup', ['target', 'add', ...missing]);
}

/** Installs `cargo-xwin` when a Windows target is built from a non-Windows host. */
function installCargoXwin(platforms) {
  const needsXwin =
    process.platform !== 'win32' && platforms.some((p) => p.target.includes('windows'));
  if (!needsXwin) return;
  if (!canRun('cargo')) throw new Error('`cargo` is not installed. Install Rust first: https://rustup.rs');
  if (canRun('cargo-xwin')) {
    console.log('\n[install-tools] cargo-xwin is already installed.');
    return;
  }
  console.log('\n[install-tools] Installing cargo-xwin (Windows MSVC cross-linker)...');
  confirmInstallation('cargo-xwin');
  run('cargo', ['install', 'cargo-xwin', '--locked']);
}

/** Installs Linux system packages required by Tauri bundles. */
function installLinuxSystemDeps() {
  if (process.platform !== 'linux') return;
  const packages = [
    'libwebkit2gtk-4.1-dev',
    'libappindicator3-dev',
    'librsvg2-dev',
    'patchelf',
    'xdg-utils',
    'zip',
  ];
  console.log('\n[install-tools] Installing Linux system packages...');
  confirmInstallation(`Linux system packages: ${packages.join(', ')}`);
  run('sudo', ['apt-get', 'update']);
  run('sudo', ['apt-get', 'install', '-y', ...packages]);
}

const LLVM_BIN_DIRS = [
  '/opt/homebrew/opt/llvm/bin',
  '/usr/local/opt/llvm/bin',
  '/opt/homebrew/opt/lld/bin',
  '/usr/local/opt/lld/bin',
];
const REQUIRED_LLVM_TOOLS = ['clang', 'lld-link', 'llvm-rc'];

function hasRequiredLlvmTools() {
  return REQUIRED_LLVM_TOOLS.every((tool) =>
    canRun(tool) || LLVM_BIN_DIRS.some((directory) => existsSync(path.join(directory, tool))),
  );
}

/**
 * Ensures the LLVM tools required by `cargo-xwin` are available when a Windows
 * target is built from a non-Windows host, installing them when missing.
 *
 * @param {object[]} platforms - Resolved platform entries.
 */
function ensureWindowsCrossLlvmTools(platforms) {
  if (process.platform === 'win32') return;
  if (!platforms.some((p) => p.target.includes('windows'))) return;

  if (hasRequiredLlvmTools()) {
    console.log(`\n[install-tools] LLVM tools (${REQUIRED_LLVM_TOOLS.join(', ')}) found.`);
    return;
  }

  if (process.platform === 'darwin' && canRun('brew')) {
    console.log('\n[install-tools] Installing LLVM and LLD through Homebrew for cargo-xwin...');
    confirmInstallation('Homebrew packages: llvm, lld');
    run('brew', ['install', 'llvm', 'lld']);
    return;
  }

  if (process.platform === 'linux') {
    console.log('\n[install-tools] Installing LLVM tools for cargo-xwin...');
    confirmInstallation('Linux packages: clang, lld, llvm');
    run('sudo', ['apt-get', 'update']);
    run('sudo', ['apt-get', 'install', '-y', 'clang', 'lld', 'llvm']);
    return;
  }

  console.warn(
    `[install-tools] Missing LLVM tools: ${REQUIRED_LLVM_TOOLS.join(', ')}. ` +
      'Install LLVM before building Windows targets.',
  );
}

/** Native build tools required by crates with C/C++ build scripts (e.g. BoringSSL). */
const NATIVE_BUILD_TOOLS = ['cmake', 'ninja', 'nasm'];

/**
 * Installs the native build tools required by some crates' build scripts
 * (`cmake`, `ninja`) when missing, through Homebrew on macOS or apt on Linux.
 */
function ensureNativeBuildTools() {
  const missing = NATIVE_BUILD_TOOLS.filter((tool) => !canRun(tool));
  if (missing.length === 0) {
    console.log(`\n[install-tools] Native build tools (${NATIVE_BUILD_TOOLS.join(', ')}) found.`);
    return;
  }

  if (process.platform === 'darwin' && canRun('brew')) {
    console.log(`\n[install-tools] Installing native build tools through Homebrew: ${missing.join(', ')}...`);
    confirmInstallation(`Homebrew packages: ${missing.join(', ')}`);
    run('brew', ['install', ...missing]);
    return;
  }
  if (process.platform === 'linux') {
    const packages = missing.map((tool) => (tool === 'ninja' ? 'ninja-build' : tool));
    console.log(`\n[install-tools] Installing native build tools: ${packages.join(', ')}...`);
    confirmInstallation(`Linux packages: ${packages.join(', ')}`);
    run('sudo', ['apt-get', 'install', '-y', ...packages]);
    return;
  }
  console.warn(`[install-tools] Missing native build tools: ${missing.join(', ')}. Install them before building.`);
}

/**
 * Ensures the NSIS compiler is available for cross builds: installs the native
 * `makensis` (Homebrew on macOS) and creates a `makensis.exe` shim in the
 * cross-tools directory, since the Tauri NSIS bundler looks for the
 * Windows-style executable name even on non-Windows hosts.
 *
 * @param {object[]} platforms - Resolved platform entries.
 */
function ensureMakensis(platforms) {
  if (process.platform === 'win32') return;
  if (!platforms.some((p) => p.target.includes('windows'))) return;

  if (!canRun('makensis')) {
    if (process.platform === 'darwin' && canRun('brew')) {
      console.log('\n[install-tools] Installing makensis through Homebrew (NSIS compiler)...');
      confirmInstallation('Homebrew package: makensis');
      run('brew', ['install', 'makensis']);
    } else {
      console.warn('[install-tools] `makensis` not found. Install NSIS before building Windows targets.');
      return;
    }
  }

  const native = commandPath('makensis');
  if (!native) {
    console.warn('[install-tools] Could not resolve the native `makensis` path; skipping shim creation.');
    return;
  }

  mkdirSync(CROSS_TOOLS_BIN_DIR, { recursive: true });
  const shimPath = path.join(CROSS_TOOLS_BIN_DIR, 'makensis.exe');
  writeFileSync(shimPath, `#!/bin/sh\nexec "${native}" "$@"\n`);
  chmodSync(shimPath, 0o755);
  console.log(`[install-tools] Created makensis.exe shim at ${shimPath} (-> ${native}).`);
}

/**
 * Installs every tool required to build the given platforms on this host.
 *
 * @param {object[]} platforms - Resolved platform entries (see `lib.mjs`).
 */
export function installTools(platforms) {
  const buildable = platforms.filter((p) => p.buildable);
  const skipped = platforms.filter((p) => !p.buildable);
  for (const platform of skipped) {
    console.warn(
      `[install-tools] Skipping \`${platform.id}\`: none of [${platform.bundles.join(', ')}] ` +
        `can be built on host ${hostLabel()} (bundles with installer types cannot be cross-produced).`,
    );
  }
  if (buildable.length === 0) {
    console.warn('[install-tools] No platform is buildable on this host.');
    return;
  }
  const targets = [...new Set(buildable.flatMap((p) => p.needsTargets))];
  console.log(`[install-tools] Host: ${hostLabel()}`);
  console.log(`[install-tools] Toolchain targets to ensure: ${targets.join(', ') || '(none)'}`);
  installRustTargets(buildable);
  installCargoXwin(buildable);
  ensureWindowsCrossLlvmTools(buildable);
  ensureNativeBuildTools();
  ensureMakensis(buildable);
  installLinuxSystemDeps();
  if (!canRun('zip')) {
    console.warn('[install-tools] `zip` not found. The Windows portable archive will use an alternative tool.');
  }
  console.log('[install-tools] Done.');
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    help();
    return;
  }
  const config = loadConfig();
  const platforms = resolvePlatforms(config, options.platforms);
  installTools(platforms);
}

// Only run the CLI when this file is the entry point, so that importing
// `installTools` from `release.mjs` does not trigger the installer.
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}

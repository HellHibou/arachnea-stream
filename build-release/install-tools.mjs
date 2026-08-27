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
import { chmodSync, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createInterface } from 'node:readline/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { canRun, commandPath, run, loadConfig, resolvePlatforms, hostLabel, CROSS_TOOLS_BIN_DIR } from './lib.mjs';
import { crossImagePresent, ensureCrossImage } from './docker.mjs';

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

/**
 * Requests confirmation before changing the host toolchain.
 *
 * Reads the answer through a readline interface instead of a raw synchronous
 * stdin read: `readSync(0, ...)` fails with EAGAIN when stdin sits in
 * non-blocking mode (e.g. IDE terminals), while readline handles it correctly.
 * The prompt is written explicitly through plain stdout because readline
 * ignores its own prompt string when running without `terminal: true`, which
 * would otherwise leave a silent wait with no visible question.
 */
async function confirmInstallation(description) {
  if (!process.stdin.isTTY) {
    throw new Error(
      `Cannot install ${description} without confirmation in a non-interactive terminal. ` +
        'Run the command interactively.',
    );
  }

  const rl = createInterface({ input: process.stdin, terminal: false });
  try {
    process.stdout.write(`\n[install-tools] Install ${description}? [y/N] `);
    const answer = (await rl.question('')).trim().toLowerCase();
    if (answer !== 'y' && answer !== 'yes') {
      throw new Error(`Installation cancelled: ${description}.`);
    }
  } catch (error) {
    if (error instanceof Error && error.message.startsWith('Installation cancelled')) throw error;
    // A closed stdin (EOF/Ctrl+D) or a stream failure counts as a refusal.
    throw new Error(`Installation cancelled: ${description}.`);
  } finally {
    rl.close();
  }
}

/** Installs rust targets for the given platform entries. */
async function installRustTargets(platforms) {
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
  await confirmInstallation(`Rust targets: ${missing.join(', ')}`);
  run('rustup', ['target', 'add', ...missing]);
}

/** Installs `cargo-xwin` when a Windows target is built from a non-Windows host. */
async function installCargoXwin(platforms) {
  const needsXwin =
    process.platform !== 'win32' && platforms.some((p) => p.target.includes('windows'));
  if (!needsXwin) return;
  if (!canRun('cargo')) throw new Error('`cargo` is not installed. Install Rust first: https://rustup.rs');
  if (canRun('cargo-xwin')) {
    console.log('\n[install-tools] cargo-xwin is already installed.');
    return;
  }
  console.log('\n[install-tools] Installing cargo-xwin (Windows MSVC cross-linker)...');
  await confirmInstallation('cargo-xwin');
  run('cargo', ['install', 'cargo-xwin', '--locked']);
}

/** Installs Linux system packages required by Tauri bundles. */
async function installLinuxSystemDeps() {
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
  await confirmInstallation(`Linux system packages: ${packages.join(', ')}`);
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
async function ensureWindowsCrossLlvmTools(platforms) {
  if (process.platform === 'win32') return;
  if (!platforms.some((p) => p.target.includes('windows'))) return;

  if (hasRequiredLlvmTools()) {
    console.log(`\n[install-tools] LLVM tools (${REQUIRED_LLVM_TOOLS.join(', ')}) found.`);
    return;
  }

  if (process.platform === 'darwin' && canRun('brew')) {
    console.log('\n[install-tools] Installing LLVM and LLD through Homebrew for cargo-xwin...');
    await confirmInstallation('Homebrew packages: llvm, lld');
    run('brew', ['install', 'llvm', 'lld']);
    return;
  }

  if (process.platform === 'linux') {
    console.log('\n[install-tools] Installing LLVM tools for cargo-xwin...');
    await confirmInstallation('Linux packages: clang, lld, llvm');
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
async function ensureNativeBuildTools() {
  const missing = NATIVE_BUILD_TOOLS.filter((tool) => !canRun(tool));
  if (missing.length === 0) {
    console.log(`\n[install-tools] Native build tools (${NATIVE_BUILD_TOOLS.join(', ')}) found.`);
    return;
  }

  if (process.platform === 'darwin' && canRun('brew')) {
    console.log(`\n[install-tools] Installing native build tools through Homebrew: ${missing.join(', ')}...`);
    await confirmInstallation(`Homebrew packages: ${missing.join(', ')}`);
    run('brew', ['install', ...missing]);
    return;
  }
  if (process.platform === 'linux') {
    const packages = missing.map((tool) => (tool === 'ninja' ? 'ninja-build' : tool));
    console.log(`\n[install-tools] Installing native build tools: ${packages.join(', ')}...`);
    await confirmInstallation(`Linux packages: ${packages.join(', ')}`);
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
async function ensureMakensis(platforms) {
  if (process.platform === 'win32') return;
  if (!platforms.some((p) => p.target.includes('windows'))) return;

  if (!canRun('makensis')) {
    if (process.platform === 'darwin' && canRun('brew')) {
      console.log('\n[install-tools] Installing makensis through Homebrew (NSIS compiler)...');
      await confirmInstallation('Homebrew package: makensis');
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
 * Ensures the Docker cross image is built when some selected platforms produce
 * anything through Docker (portable Linux/macOS binaries and/or the Linux
 * installers bundled inside the image). Because the first build pulls the
 * large base image (Rust + osxcross + Apple SDK), it requires confirmation
 * like every other host change.
 *
 * @param {object[]} platforms - Resolved platform entries.
 */
async function ensureDockerCrossBuild(platforms) {
  const buildPlatforms = platforms.filter((p) => p.needsDockerBuild);
  if (buildPlatforms.length === 0) return;
  if (crossImagePresent()) {
    console.log(`\n[install-tools] Docker cross image already built for: ${buildPlatforms.map((p) => p.id).join(', ')}.`);
    return;
  }
  console.log(`\n[install-tools] Building the Docker cross image for: ${buildPlatforms.map((p) => p.id).join(', ')}.`);
  await confirmInstallation('the Arach cross-build Docker image (first run pulls the base rust + osxcross image)');
  ensureCrossImage();
}

/** Installs every tool required to build the given platforms on this host.
 *
 * @param {object[]} platforms - Resolved platform entries (see `lib.mjs`).
 */
export async function installTools(platforms) {
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
  // Docker-built targets compile inside the container via osxcross/its own
  // toolchains, so only install host rustup targets for natively-built ones.
  await installRustTargets(buildable.filter((p) => p.method !== 'docker'));
  await installCargoXwin(buildable);
  await ensureWindowsCrossLlvmTools(buildable);
  await ensureNativeBuildTools();
  await ensureMakensis(buildable);
  await installLinuxSystemDeps();
  await ensureDockerCrossBuild(buildable);
  if (!canRun('zip')) {
    console.warn('[install-tools] `zip` not found. The Windows portable archive will use an alternative tool.');
  }
  console.log('[install-tools] Done.');
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    help();
    return;
  }
  const config = loadConfig();
  const platforms = resolvePlatforms(config, options.platforms);
  await installTools(platforms);
}

// Only run the CLI when this file is the entry point, so that importing
// `installTools` from `release.mjs` does not trigger the installer.
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}

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
import { canRun, commandPath, run, loadConfig, resolvePlatforms, hostLabel, CROSS_TOOLS_BIN_DIR, findVsInstallation, msvcToolsetDir, vsWhereExe, windowsNativeMsvcArm64State, isElevated, runElevatedSync, ensureRustcVersion } from './lib.mjs';
import { assertDocker, crossImagePresent, ensureCrossImage } from './docker.mjs';

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

/**
 * Resolves the Rust toolchain context the build will actually use: the `rustc`
 * at the head of PATH, its sysroot, and — when that rustc belongs to a rustup
 * toolchain — the toolchain name. Detection is sysroot-based rather than
 * `rustup target list`-based because a host can have several independent Rust
 * installs on PATH: `rustup target add` only manages rustup's own toolchains,
 * while `cargo build` uses whatever `cargo`/`rustc` precedes on PATH. Returning
 * a `toolchain` of `null` means the active rustc is a standalone install that
 * rustup cannot provision targets for.
 *
 * @returns {{ rustc: string, sysroot: string, toolchain: string|null }}
 */
function resolveRustContext() {
  const rustc = commandPath('rustc');
  if (!rustc) throw new Error('`rustc` is not installed. Install Rust first: https://rustup.rs');
  const res = spawnSync(rustc, ['--print', 'sysroot'], { encoding: 'utf8' });
  if (res.status !== 0) throw new Error('Could not read the Rust toolchain sysroot.');
  const sysroot = res.stdout.trim();

  const list = spawnSync('rustup', ['toolchain', 'list', '-v'], { encoding: 'utf8' });
  let toolchain = null;
  if (list.status === 0 && list.stdout) {
    const normalize = (p) => p.replace(/[\\/]+$/g, '').toLowerCase().replace(/\//g, '\\');
    const wanted = normalize(sysroot);
    for (const line of list.stdout.trim().split(/\r?\n/)) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      const tokens = trimmed.split(/\s+/);
      const name = tokens[0];
      const pathToken = tokens[tokens.length - 1];
      // Absolute path check (Windows drive or POSIX root): skip flag-only lines.
      if (!pathToken || !/^[A-Za-z]:[\\/]|^\//.test(pathToken)) continue;
      if (normalize(pathToken) === wanted) {
        toolchain = name;
        break;
      }
    }
  }
  return { rustc, sysroot, toolchain };
}

/**
 * `rustc` looks for the standard library of a target under
 * `<sysroot>/lib/rustlib/<target>/lib`; a missing directory is exactly what
 * produces "can't find crate for `core` / `std`". Checking it directly is the
 * source of truth, unlike `rustup target list` which reflects a different
 * (rustup-managed) install when several Rust toolchains coexist on PATH.
 *
 * @param {string} sysroot - The Rust toolchain sysroot to inspect.
 * @param {string} target - Rust target triple.
 * @returns {boolean} Whether the target's standard library is present.
 */
function targetStdInstalled(sysroot, target) {
  return existsSync(path.join(sysroot, 'lib', 'rustlib', target, 'lib'));
}

/**
 * Returns the subset of the given rustup targets whose standard library is not
 * present in the actual Rust toolchain that will perform the build.
 *
 * @param {string[]} targets - Rust target triples to check.
 * @returns {string[]} The targets missing from the toolchain in use.
 */
function missingRustTargets(targets) {
  const { sysroot } = resolveRustContext();
  return targets.filter((target) => !targetStdInstalled(sysroot, target));
}

/**
 * Installs the given missing Rust targets on the rustup toolchain that owns the
 * rustc in use, after explicit confirmation, then re-verifies them. Throws an
 * actionable error when the rustc in use is not managed by rustup, since such a
 * standalone toolchain cannot be provisioned by rustup.
 *
 * @param {string[]} missing - Rust target triples known to be missing.
 */
async function ensureTargetsInstalled(missing) {
  const context = resolveRustContext();
  if (!context.toolchain) {
    throw new Error(
      `The Rust toolchain that will build is not managed by rustup, so its missing ` +
        `target(s) (${missing.join(', ')}) cannot be installed automatically.\n` +
        `  rustc in use    : ${context.rustc}\n` +
        `  its sysroot     : ${context.sysroot}\n` +
        `This usually happens when a standalone Rust install (e.g. under "C:\\Program Files") ` +
        `precedes rustup's cargo on PATH. Either install the missing target(s) into that ` +
        `toolchain, or reorder PATH so rustup's bin directory comes first (rustup already ` +
        `installs targets there).`,
    );
  }
  await confirmInstallation(`Rust targets: ${missing.join(', ')}`);
  run('rustup', ['target', 'add', '--toolchain', context.toolchain, ...missing]);
  const stillMissing = missingRustTargets(missing);
  if (stillMissing.length > 0) {
    throw new Error(`rustup install reported success but these targets are still missing: ${stillMissing.join(', ')}`);
  }
}

/** Installs rust targets for the given platform entries. */
async function installRustTargets(platforms) {
  const targets = [...new Set(platforms.flatMap((p) => p.needsTargets))];
  if (targets.length === 0) return;
  const missing = missingRustTargets(targets);
  if (missing.length === 0) {
    console.log('\n[install-tools] Required Rust targets are already installed.');
    return;
  }
  await ensureTargetsInstalled(missing);
}

/**
 * Pre-flight guard run by `release.mjs` right before the native `cargo tauri
 * build` of one platform: detects whether its rustup target(s) are installed on
 * the actual Rust toolchain used by the build and, if not, installs them after
 * explicit confirmation so a missing target can never abort the build with a
 * `can't find crate for core/std` failure (e.g. `aarch64-pc-windows-msvc`). It
 * is a no-op when the targets are already present, so calling it for every
 * native build only costs a sysroot probe.
 *
 * @param {object} platform - Enriched platform entry whose native build is
 *   about to run (see `resolvePlatforms` for the `needsTargets` field).
 */
export async function ensurePlatformRustupTargets(platform) {
  const targets = [...new Set(platform.needsTargets ?? [])];
  if (targets.length === 0) return;
  const missing = missingRustTargets(targets);
  if (missing.length === 0) return;
  await ensureTargetsInstalled(missing);
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
 * Returns the VS-provided Ninja directory when `ninja.exe` exists inside the
 * Visual Studio CMake tooling (`Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja`).
 * The build environment augments PATH with this directory automatically.
 *
 * @returns {string|null} Directory containing `ninja.exe`, or `null`.
 */
function windowsVsNinjaDir() {
  if (process.platform !== 'win32') return null;
  const vs = findVsInstallation();
  if (!vs || !vs.installationPath) return null;
  const dir = path.join(vs.installationPath, 'Common7', 'IDE', 'CommonExtensions', 'Microsoft', 'CMake', 'Ninja');
  return existsSync(path.join(dir, 'ninja.exe')) ? dir : null;
}

/**
 * Installs the native build tools required by some crates' build scripts
 * (`cmake`, `ninja`) when missing, through Homebrew on macOS or apt on Linux.
 * On Windows, a `ninja` shipped inside Visual Studio CMake counts as present
 * because the build PATH already includes it.
 */
async function ensureNativeBuildTools() {
  let missing = NATIVE_BUILD_TOOLS.filter((tool) => !canRun(tool));
  if (process.platform === 'win32' && missing.includes('ninja') && windowsVsNinjaDir()) {
    console.log('[install-tools] ninja: found inside Visual Studio Build Tools (added to the build PATH).');
    missing = missing.filter((tool) => tool !== 'ninja');
  }
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
 * Adds the missing Visual Studio components (MSVC ARM64 build tools and the
 * C++ Clang Compiler for Windows) to the current VS installation through
 * `vs_installer.exe`, then re-verifies the ARM64 toolchain state.
 *
 * @param {object} state - Output of `windowsNativeMsvcArm64State()`.
 */
async function addVsComponents(state) {
  const components = [];
  if (!state.msvcArm64ClPath) {
    const vsVersion = (state.vs?.installationVersion || '').match(/^(\d+)\.(\d+)/);
    const toolsetName = state.toolset ? path.basename(state.toolset) : '';
    const msvcMajorMinor = toolsetName.split('.').slice(0, 2).join('.');
    if (!vsVersion || !msvcMajorMinor || !/^\d+\.\d+$/.test(msvcMajorMinor)) {
      throw new Error(
        'Could not derive the Visual Studio MSVC ARM64 component id from the current installation. ' +
          'Install the "MSVC v143 - VS 2022 C++ ARM64 build tools" component manually or ' +
          'reinstall Visual Studio Build Tools.',
      );
    }
    components.push(
      `Microsoft.VisualStudio.Component.VC.${msvcMajorMinor}.${vsVersion[1]}.${vsVersion[2]}.ARM64`,
    );
  }
  if (!state.clangClPath) components.push('Microsoft.VisualStudio.Component.VC.Llvm.Clang');
  if (components.length === 0) return;

  const vsWhere = vsWhereExe();
  const installer = vsWhere ? path.join(path.dirname(vsWhere), 'vs_installer.exe') : '';
  if (!installer || !existsSync(installer)) {
    throw new Error(
      'The Visual Studio installer (`vs_installer.exe`) was not found. Run the Visual Studio ' +
        `Installer manually to add the missing components: ${components.join(', ')}`,
    );
  }

  console.log(`\n[install-tools] Adding Visual Studio Build Tools components: ${components.join(', ')}...`);
  await confirmInstallation(`Visual Studio Build Tools components: ${components.join(', ')}`);
  const installerArgs = [
    'modify',
    '--installPath',
    state.vs.installationPath,
    '--add',
    ...components,
    '--quiet',
    '--norestart',
    // Skip the installer self-update / channel feed checks: those extra network
    // round-trips can be cancelled in constrained (elevated-from-unelevated)
    // contexts and are not needed to add components.
    '--noUpdateInstaller',
  ];
  // `setup.exe` with `--quiet`/`--passive` must run elevated (UAC); an
  // unelevated run exits 5007. When the current process is not elevated, re-run
  // the command through `Start-Process -Verb RunAs`. The installer is
  // synchronous here and returns 3010 when a reboot is required but the
  // operation itself succeeded; `--wait` is not a valid `modify` option on this
  // installer version.
  let status;
  if (isElevated()) {
    status = spawnSync(installer, installerArgs, { stdio: 'inherit' }).status;
  } else {
    const message = `\n[install-tools] Visual Studio needs administrator rights to modify components. ` +
      'A UAC prompt will appear — please accept it.';
    console.log(message);
    status = runElevatedSync(installer, installerArgs);
  }
  if (status !== 0 && status !== 3010) {
    throw new Error(`Visual Studio component installation failed (exit ${status}).`);
  }
  console.log('[install-tools] Visual Studio component installation finished.');

  const after = windowsNativeMsvcArm64State();
  if (!after.ok) {
    throw new Error(
      `Visual Studio reported a finished component installation but the following parts are ` +
        `still missing: ${after.missing.join(', ')}.\n` +
        'The Visual Studio installer can silently cancel a quiet modify when it is launched ' +
        'from an unelevated context. Open a terminal AS ADMINISTRATOR (right-click the terminal, ' +
        '"Run as administrator") and re-run this command, or finish the installation manually in ' +
        'the Visual Studio Installer UI (Modify > Individual components > check "MSVC v143 - VS 2022 ' +
        'C++ ARM64 build tools" and "C++ Clang Compiler for Windows", then Modify).',
    );
  }
  console.log('\n[install-tools] MSVC ARM64 toolchain and clang-cl are now installed.');
}

/**
 * Ensures the native Windows MSVC ARM64 C/C++ toolchain when a Windows host
 * natively builds `aarch64-pc-windows-msvc`. Two Visual Studio Build Tools
 * components are required:
 * - the **MSVC ARM64 build tools** (compiler + CRT libraries, used by rustc
 *   to link Windows ARM64 executables);
 * - **clang-cl** ("C++ Clang Compiler for Windows"), required by `ring` and
 *   `aws-lc-sys` to compile/assemble C for `aarch64-pc-windows-msvc`.
 *
 * Missing components are installed automatically after explicit confirmation;
 * when no Visual Studio installation exists at all, `winget` installs the
 * Build Tools (C++ workload) first.
 *
 * @param {object[]} platforms - Resolved platform entries.
 */
async function ensureWindowsArm64MsvcToolchain(platforms) {
  if (process.platform !== 'win32') return;
  const needsArm64 = platforms.some(
    (p) => p.buildable && p.method === 'native' && p.target === 'aarch64-pc-windows-msvc',
  );
  if (!needsArm64) return;

  const state = windowsNativeMsvcArm64State();
  if (state.ok) {
    console.log('\n[install-tools] MSVC ARM64 toolchain and clang-cl found (Windows ARM64 C/C++ build OK).');
    return;
  }
  console.log(`\n[install-tools] Missing Windows ARM64 C/C++ toolchain parts: ${state.missing.join(', ')}.`);

  if (state.vs && state.vs.installationPath) {
    await addVsComponents(state);
    return;
  }

  if (canRun('winget')) {
    await confirmInstallation('Visual Studio Build Tools 2022 (C++ workload) through winget');
    const override = [
      '--quiet',
      '--norestart',
      '--add', 'Microsoft.VisualStudio.Workload.VCTools',
      '--add', 'Microsoft.VisualStudio.Component.VC.Llvm.Clang',
    ].join(' ');
    run('winget', [
      'install', '--id', 'Microsoft.VisualStudio.2022.BuildTools',
      '--override', override,
      '--accept-source-agreements', '--accept-package-agreements',
      '--disable-interactivity', '--silent',
    ]);
  } else {
    throw new Error(
      'No Visual Studio Build Tools installation found and `winget` is not available. ' +
        'Install Visual Studio Build Tools 2022 with the "Desktop development with C++" workload ' +
        '(including the MSVC ARM64 build tools and the C++ Clang Compiler for Windows) before ' +
        'building aarch64-pc-windows-msvc.',
    );
  }

  // The VCTools workload only installs x64/x86 MSVC tools: provisioning the
  // ARM64 toolset through the same component adder is still necessary.
  const after = windowsNativeMsvcArm64State();
  if (!after.ok) await addVsComponents(after);
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
  // Fail fast (with a clear message) when Docker is missing or not running,
  // instead of prompting for an image build that cannot start.
  await assertDocker();
  if (crossImagePresent()) {
    console.log(`\n[install-tools] Docker cross image already built for: ${buildPlatforms.map((p) => p.id).join(', ')}.`);
    return;
  }
  console.log(`\n[install-tools] Building the Docker cross image for: ${buildPlatforms.map((p) => p.id).join(', ')}.`);
  await confirmInstallation(
    'the Arachnea cross-build Docker image (first run pulls the base rust + osxcross image and builds both amd64 and arm64 ports — the arm64 half compiles its Tauri CLI under QEMU emulation)',
  );
  await ensureCrossImage();
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
  // Fail fast before provisioning anything: an outdated rustc would reject the
  // locked dependency graph (e.g. foyer@0.22.4+ requires rustc >= 1.91.0).
  // Offers `rustup update` with confirmation when interactive.
  await ensureRustcVersion();
  console.log(`[install-tools] Toolchain targets to ensure: ${targets.join(', ') || '(none)'}`);
  // Docker-built targets compile inside the container via osxcross/its own
  // toolchains, so only install host rustup targets for natively-built ones.
  await installRustTargets(buildable.filter((p) => p.method !== 'docker'));
  await installCargoXwin(buildable);
  await ensureWindowsCrossLlvmTools(buildable);
  await ensureNativeBuildTools();
  await ensureWindowsArm64MsvcToolchain(buildable);
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

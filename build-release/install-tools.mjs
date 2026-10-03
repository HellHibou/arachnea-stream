// Toolchain installer for the Arachnea cross-platform release build.
//
// Ensures everything needed to build the requested platforms on the current
// host is available:
// - rustup targets (standard library for each target triple),
// - `cargo-xwin` (Windows MSVC linker/runner for non-Windows hosts),
// - host Tauri CLI (`cargo tauri`) for natively-built platforms,
// - frontend dependencies (`npm install` in `front/` workspaces, so `run-p`
//   and other local binaries exist before `release.mjs` runs `npm run build`),
// - Linux system packages required by Tauri/WebKitGTK bundles,
// - `zip` for the Windows portable archive.
//
// Usage:
//   node release/install-tools.mjs                 # all locally buildable platforms
//   node release/install-tools.mjs windows-*       # tools for matching platforms only
import { chmodSync, existsSync, mkdirSync, statSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createInterface } from 'node:readline/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { canRun, commandPath, run, loadConfig, resolvePlatforms, resolveFromRelease, hostLabel, CROSS_TOOLS_BIN_DIR, findVsInstallation, msvcToolsetDir, vsWhereExe, windowsNativeMsvcArm64State, isElevated, runElevatedSync, ensureRustcVersion, ensureRustToolchain, ensureTauriCli, ensureLocalTargetDir } from './lib.mjs';
import { assertDocker, crossImagePresent, ensureCrossImage } from './docker.mjs';

function parseArgs(argv) {
  const options = { targets: [] };
  for (const arg of argv) {
    // Removed flag: give the new syntax instead of a generic unknown-argument
    // error, so existing callers (scripts, CI) get an actionable message.
    if (arg === '--platform' || arg === '-p') {
      throw new Error(
        `The \`${arg}\` flag was removed: pass platform selectors as plain arguments, ` +
          'e.g. `node build-release/install-tools.mjs windows-*`.',
      );
    }
    if (arg === '--help' || arg === '-h') options.help = true;
    else if (arg.startsWith('-') && arg !== '-') throw new Error(`Unknown argument: ${arg}`);
    else {
      // Positional platform selectors. Several may be given, and a single
      // argument may hold a comma-separated list (`darwin,windows`).
      for (const part of arg.split(',')) {
        const selector = part.trim();
        if (selector) options.targets.push(selector);
      }
    }
  }
  return options;
}

/** Prints the tool-install help text. */
function help() {
  console.log(`Usage: node build-release/install-tools.mjs [OPTIONS] [TARGET...]

Installs the tools required to build the Arachnea release bundles on the
current host (${hostLabel()}):

  TARGET...             Provision tools only for matching platforms. Selectors
                        are exact ids (darwin-arm64), family names (darwin,
                        windows, linux), the alias osx (= darwin), \`*\` for every
                        platform, or trailing-\`*\` patterns (darwin-*). Several
                        targets may be listed, space- or comma-separated; quote
                        \`*\` so the shell keeps it.
                        Default: every platform this host can build.
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

/**
 * Ensures the host Tauri CLI (`cargo tauri`) when at least one buildable
 * platform uses the native method (host-side `cargo tauri build` needs the
 * `cargo-tauri` binary; Docker-built platforms use the in-image CLI instead).
 *
 * Must run after Rust provisioning (`ensureRustToolchain`/`ensureRustcVersion`):
 * the version probe shells out to `cargo`, and installing `tauri-cli` pins the
 * `tauriCliVersion` from `build-config.json` (same version as the cross image).
 */
async function ensureHostTauriCli(buildable) {
  if (!buildable.some((p) => p.method === 'native')) return;
  await ensureTauriCli();
}

/**
 * Returns `true` when the given Debian package is installed (`dpkg-query`
 * reports `install ok installed`). Returns `false` when `dpkg-query` itself
 * is unavailable, so callers fall back to the previous install flow.
 *
 * @param {string} pkg - Debian package name.
 * @returns {boolean} Whether the package is installed.
 */
function isDebPackageInstalled(pkg) {
  if (!canRun('dpkg-query')) return false;
  const result = spawnSync('dpkg-query', ['-W', '-f=${Status}', pkg], {
    encoding: 'utf8',
    windowsHide: true,
  });
  if (result.error || result.status !== 0) return false;
  return String(result.stdout ?? '').includes('install ok installed');
}

/** Installs Linux system packages required by Tauri bundles. */
async function installLinuxSystemDeps() {
  if (process.platform !== 'linux') return;
  const packages = [
    'libwebkit2gtk-4.1-dev',
    // Ayatana fork (Tauri's documented prerequisite): the legacy
    // `libappindicator3-dev` depends on `libappindicator3-1`, which apt reports
    // as conflicting with the `libayatana-appindicator3-1` shipped by Linux
    // Mint/Ubuntu, aborting the whole install.
    'libayatana-appindicator3-dev',
    'librsvg2-dev',
    'patchelf',
    'xdg-utils',
    'zip',
    // OpenSSL dev files (`openssl.pc` + headers): `openssl-sys` locates OpenSSL
    // through pkg-config, so a runtime-only `libssl3` install is not enough.
    'libssl-dev',
    'pkg-config',
    // `boring-sys2`'s build script applies BoringSSL patches through `git`
    // (`ensure_patches_applied`, first step of `build/main.rs`): without it the
    // script panics with `Os { code: 2, kind: NotFound }` at `main.rs:662`.
    'git',
    // `boring-sys2` generates its bindings through `bindgen`, which needs
    // `libclang` (bindgen requirements: `libclang-dev` on Debian; the `clang`
    // package as well to dump preprocessed inputs).
    'clang',
    'libclang-dev',
  ];
  console.log('\n[install-tools] Installing Linux system packages...');
  // Skip the sudo/apt round-trip (password prompt + `apt-get update`) when
  // every package is already present: `apt-get install -y` is idempotent, but
  // asking for it on each run is pure friction.
  const missing = packages.filter((pkg) => !isDebPackageInstalled(pkg));
  if (missing.length === 0) {
    console.log('[install-tools] Linux system packages already installed.');
    return;
  }
  await confirmInstallation(`Linux system packages: ${missing.join(', ')}`);
  run('sudo', ['apt-get', 'update']);
  run('sudo', ['apt-get', 'install', '-y', ...missing]);
}

/**
 * Workspace roots holding a `package.json` that must be installed before the
 * frontend build: the root `front/` project plus each app workspace (`front/`
 * has no npm workspaces field, so every directory installs its own deps).
 */
const FRONTEND_WORKSPACES = ['.', 'public-app', 'admin-app'];

/**
 * Returns `true` when a frontend workspace looks installed (its
 * `node_modules/.package-lock.json` exists and is newer than its
 * `package.json`), so repeat runs skip the `npm install` round-trip instead of
 * reinstalling idempotently.
 *
 * @param {string} dir - Absolute path of the workspace directory.
 * @returns {boolean} Whether dependencies look up to date.
 */
function frontendWorkspaceInstalled(dir) {
  try {
    const marker = path.join(dir, 'node_modules', '.package-lock.json');
    if (!existsSync(marker)) return false;
    const markerTime = statSync(marker).mtimeMs;
    for (const file of ['package.json', 'package-lock.json']) {
      const manifest = path.join(dir, file);
      if (existsSync(manifest) && statSync(manifest).mtimeMs > markerTime) return false;
    }
    return true;
  } catch {
    return false;
  }
}

/**
 * Ensures the frontend workspaces have their dependencies installed before
 * `release.mjs` runs `npm run build`: the app build scripts spawn `run-p` from
 * the local `npm-run-all2` dependency, so a checkout without `node_modules`
 * fails with `'run-p' is not recognized as an internal or external command`.
 * Each workspace installs after a single confirmation, and already-installed
 * ones are skipped (see `frontendWorkspaceInstalled`).
 */
async function ensureFrontendDeps() {
  const { frontendProject } = loadConfig();
  const root = resolveFromRelease(frontendProject);
  const pending = FRONTEND_WORKSPACES.map((workspace) => ({
    workspace,
    dir: path.join(root, workspace),
  })).filter(
    ({ dir }) => existsSync(path.join(dir, 'package.json')) && !frontendWorkspaceInstalled(dir),
  );
  if (pending.length === 0) return;
  if (!canRun('npm')) {
    throw new Error(
      '`npm` is not installed: install Node.js (with npm) before building the frontend ' +
        `(missing dependencies in: ${pending.map(({ workspace }) => workspace).join(', ')}).`,
    );
  }
  await confirmInstallation(
    `frontend dependencies via \`npm install\` in: ${pending.map(({ workspace }) => workspace).join(', ')}`,
  );
  const isWindows = process.platform === 'win32';
  for (const { workspace, dir } of pending) {
    console.log(`\n[install-tools] Installing frontend dependencies in ${workspace}...`);
    // Node >= 20 cannot `spawn` a Windows `.cmd` shim directly (EINVAL); pass
    // `shell: true` so npm.cmd is resolved through cmd.exe on Windows only.
    run(isWindows ? 'npm.cmd' : 'npm', ['install'], {
      cwd: dir,
      ...(isWindows ? { shell: true } : {}),
    });
  }
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
const NATIVE_BUILD_TOOLS = ['cmake', 'ninja', 'nasm', 'go', 'perl', 'c++'];

/**
 * winget ids of the Windows native build tools this tooling can install, keyed
 * by `NATIVE_BUILD_TOOLS` command name. `c++` has no entry: Windows' C++
 * compiler is MSVC `cl.exe`, which comes with the Visual Studio C++ workload
 * (see `windowsNativeMsvcArm64State`) rather than from a standalone package.
 */
const WINDOWS_WINGET_IDS = {
  cmake: 'Kitware.CMake',
  ninja: 'Ninja-build.Ninja',
  nasm: 'NASM.NASM',
  go: 'GoLang.Go',
  perl: 'StrawberryPerl.StrawberryPerl',
};

/**
 * Directories where the Windows native build tools are installed: the
 * `WINDOWS_WINGET_IDS` packages plus the winget "Links" shim directory used by
 * portable packages. Strawberry Perl's `c\bin` (a GCC toolchain) is deliberately
 * left out: having it on PATH makes CMake and the BoringSSL build misdetect the
 * compiler, and only `perl\bin` is needed for the `perl` command.
 *
 * @returns {string[]} Existing candidate directories.
 */
function windowsNativeToolDirs() {
  const programFiles = process.env.ProgramFiles || 'C:\\Program Files';
  const programFilesX86 = process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)';
  const localAppData = process.env.LOCALAPPDATA || '';
  return [
    path.join(programFiles, 'Go', 'bin'),
    'C:\\Strawberry\\perl\\bin',
    path.join(programFiles, 'NASM'),
    path.join(programFilesX86, 'NASM'),
    path.join(programFiles, 'CMake', 'bin'),
    path.join(programFilesX86, 'CMake', 'bin'),
    path.join(programFiles, 'ninja-build'),
    path.join(programFilesX86, 'ninja-build'),
    localAppData ? path.join(localAppData, 'Microsoft', 'WinGet', 'Links') : '',
  ].filter((dir) => dir && existsSync(dir));
}

/**
 * Prepends `windowsNativeToolDirs()` to the current `PATH` and returns the
 * directories that were added.
 *
 * The Go and Perl installers (and `winget` in general) update the *machine*
 * PATH, which only reaches shells started afterwards. A terminal opened before
 * the install therefore keeps probing the old PATH: the availability check sees
 * tools that are already installed as missing and offers to install them again
 * on every run. Augmenting PATH here keeps both that check and the build itself
 * accurate in such a shell (CMake runs `find_package(Perl REQUIRED)` and
 * BoringSSL spawns `go run`), without requiring a restart.
 *
 * @returns {string[]} Directories that were prepended.
 */
function prependWindowsNativeToolDirs() {
  if (process.platform !== 'win32') return [];
  const normalize = (dir) => dir.replace(/[\\/]+$/, '').toLowerCase();
  const current = (process.env.PATH || '').split(path.delimiter).map(normalize);
  const added = windowsNativeToolDirs().filter((dir) => !current.includes(normalize(dir)));
  if (added.length > 0) process.env.PATH = [...added, process.env.PATH].join(path.delimiter);
  return added;
}

/**
 * Returns `true` when a C++ compiler is available on Windows. There is no
 * `c++` command there: BoringSSL is compiled by MSVC `cl.exe`, which CMake and
 * rustc locate through the Visual Studio installation rather than PATH, so the
 * toolset location of the detected Visual Studio is checked before giving up.
 *
 * @returns {boolean} Whether an MSVC/clang-cl C++ compiler was found.
 */
function windowsCxxCompilerAvailable() {
  if (canRun('cl') || canRun('clang++')) return true;
  const vs = findVsInstallation();
  const toolset = vs?.installationPath ? msvcToolsetDir(vs.installationPath) : null;
  return Boolean(toolset && existsSync(path.join(toolset, 'bin', 'Hostx64', 'x64', 'cl.exe')));
}

/**
 * Returns `true` when a native build tool is available on this host.
 *
 * @param {string} tool - Command name from `NATIVE_BUILD_TOOLS`.
 * @returns {boolean} Whether the tool can be used for a build.
 */
function nativeBuildToolAvailable(tool) {
  // Windows has no `c++` command (see `windowsCxxCompilerAvailable`); `ninja`
  // and `go`/`perl` keep the plain command probe on every platform.
  if (tool === 'c++' && process.platform === 'win32') return windowsCxxCompilerAvailable();
  return canRun(tool);
}

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
 * (`cmake`, `ninja`, `nasm`, plus the BoringSSL build prerequisites `go`,
 * `perl` and a C++ compiler — BoringSSL's CMake runs `find_package(Perl
 * REQUIRED)` and generates `err_data.c` with `go run`) when missing, through
 * Homebrew on macOS, apt on Linux, and winget on Windows. On Windows, the
 * VS-provided `ninja` and MSVC `cl.exe` count as present because CMake and
 * rustc locate them through the Visual Studio installation; Go and Perl have no
 * such fallback (their absence fails the BoringSSL configure step), so they are
 * installed through winget, and a tool winget cannot provide is reported.
 */
async function ensureNativeBuildTools() {
  // A tool installed by an earlier run stays invisible in a shell started
  // before it (see `prependWindowsNativeToolDirs`): expose the known install
  // locations first, otherwise every run offers to reinstall what is already
  // there.
  const augmented = prependWindowsNativeToolDirs();
  if (augmented.length > 0) {
    console.log(`[install-tools] PATH augmented with installed tools: ${augmented.join(', ')}`);
  }
  let missing = NATIVE_BUILD_TOOLS.filter((tool) => !nativeBuildToolAvailable(tool));
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
    const packages = missing.map((tool) => {
      if (tool === 'ninja') return 'ninja-build';
      // `c++` is probed as a command but installed as the `g++` package.
      if (tool === 'c++') return 'g++';
      // The `go` and `perl` commands come from the `golang-go` and `perl`
      // packages (same names through Homebrew, hence no mapping needed there).
      if (tool === 'go') return 'golang-go';
      return tool;
    });
    console.log(`\n[install-tools] Installing native build tools: ${packages.join(', ')}...`);
    await confirmInstallation(`Linux packages: ${packages.join(', ')}`);
    run('sudo', ['apt-get', 'install', '-y', ...packages]);
    return;
  }
  if (process.platform === 'win32' && canRun('winget') && missing.some((tool) => WINDOWS_WINGET_IDS[tool])) {
    const installable = missing.filter((tool) => WINDOWS_WINGET_IDS[tool]);
    const packages = installable.map((tool) => WINDOWS_WINGET_IDS[tool]);
    console.log(`\n[install-tools] Installing native build tools through winget: ${packages.join(', ')}...`);
    await confirmInstallation(`winget packages: ${packages.join(', ')}`);
    const winget = commandPath('winget') || 'winget';
    // Go and Strawberry Perl install machine-wide MSIs, so winget needs an
    // elevated process: run it with inherited stdio when already elevated,
    // otherwise through `Start-Process -Verb RunAs` with the usual UAC prompt
    // (same handling as the Visual Studio installer).
    const elevated = isElevated();
    if (!elevated) {
      console.log('[install-tools] These packages install machine-wide: a UAC prompt will appear — please accept it.');
    }
    for (const id of packages) {
      const args = [
        'install',
        '--id', id,
        '--exact',
        '--accept-package-agreements',
        '--accept-source-agreements',
        // Silent install: the elevation prompt above is the interactive part.
        '--disable-interactivity',
      ];
      // A failing `winget install` (package already present but off PATH,
      // declined UAC, unavailable source, …) must not abort a build that may
      // still work: report it and let the availability re-check below decide.
      const result = elevated
        ? spawnSync(winget, args, { stdio: 'inherit' })
        : { status: runElevatedSync(winget, args) };
      if (result.error || result.status !== 0) {
        console.warn(
          `[install-tools] \`winget install ${id}\` failed (exit ${result.status ?? 'n/a'}). Install it manually before building.`,
        );
      }
    }
    // winget refreshed the machine PATH: expose the new directories to this run
    // too, then re-check instead of assuming the install succeeded.
    prependWindowsNativeToolDirs();
    missing = missing.filter((tool) => !nativeBuildToolAvailable(tool));
    if (missing.length === 0) {
      console.log(`\n[install-tools] Native build tools (${NATIVE_BUILD_TOOLS.join(', ')}) found.`);
      return;
    }
  }
  const windowsHint =
    process.platform === 'win32'
      ? ' Go: `winget install GoLang.Go`. Perl: `winget install StrawberryPerl.StrawberryPerl`.' +
        ' The C++ compiler comes with the Visual Studio "Desktop development with C++" workload.'
      : '';
  console.warn(
    `[install-tools] Missing native build tools: ${missing.join(', ')}. Install them before building.${windowsHint}`,
  );
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
 * Platforms are filtered on their *resolved* `method`, not the static
 * `needsDockerBuild` eligibility: a `build: docker` platform the host builds
 * natively (e.g. `linux-x86_64` on a Linux host, method `native`) must not
 * pull Docker into the run.
 *
 * @param {object[]} platforms - Resolved platform entries.
 */
async function ensureDockerCrossBuild(platforms) {
  const buildPlatforms = platforms.filter((p) => p.method === 'docker');
  if (buildPlatforms.length === 0) return;
  // Fail fast when Docker is missing or not running (on Linux a missing CLI
  // first offers the automatic Docker Engine install), instead of prompting
  // for an image build that cannot start.
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
 * @param {{frontendDeps?: boolean}} [options] - Set `frontendDeps: false` when
 *   the caller will not run the frontend build (`--skip-build`,
 *   `--no-frontend-build`), so the workspaces are left untouched.
 */
export async function installTools(platforms, options = {}) {
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
  // Redirect cargo's target directory off VirtualBox shared folders / network
  // mounts first: every `cargo` below (rustup probes, installs, builds)
  // inherits `process.env`, so this must run before anything shells out to
  // cargo. Offers `~/.cache/arachnea-target` with confirmation.
  await ensureLocalTargetDir();
  // Offer to install Rust itself first: the rustc floor check below and every
  // `cargo`/`rustup` step need a toolchain on the PATH. On refusal (or in a
  // non-interactive terminal) this throws the usual "install Rust first" error.
  await ensureRustToolchain();
  // Fail fast before provisioning anything: an outdated rustc would reject the
  // locked dependency graph (e.g. foyer@0.22.4+ requires rustc >= 1.91.0).
  // Offers `rustup update` with confirmation when interactive.
  await ensureRustcVersion();
  console.log(`[install-tools] Toolchain targets to ensure: ${targets.join(', ') || '(none)'}`);
  // Docker-built targets compile inside the container via osxcross/its own
  // toolchains, so only install host rustup targets for natively-built ones.
  await installRustTargets(buildable.filter((p) => p.method !== 'docker'));
  // Host `cargo tauri build` needs the `cargo-tauri` binary; `install-tools`
  // never installed it (the CLI version was only forwarded to the Docker
  // image), so native Linux runs failed with `no such command: 'tauri'`.
  await ensureHostTauriCli(buildable);
  // Frontend workspaces: the app build scripts spawn the local `run-p`, so a
  // checkout without `node_modules` fails the shared frontend build below.
  // Skipped only when this run never builds the frontend.
  if (options.frontendDeps !== false) await ensureFrontendDeps();
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
  const platforms = resolvePlatforms(config, options.targets);
  await installTools(platforms);
}

// Only run the CLI when this file is the entry point, so that importing
// `installTools` from `release.mjs` does not trigger the installer.
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}

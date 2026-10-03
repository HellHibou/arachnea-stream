// Arachnea cross-platform release builder (100% local, Node).
//
// Builds the requested platforms with `cargo tauri build`, assembles a
// `releases/release-<VERSION>/<platform>/` directory with version-named
// installer artifacts and copies the project CHANGELOG into the release
// folder. Windows platforms additionally produce a portable `.zip`
// (executable + runtime data).
import path from 'node:path';
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { installTools, ensurePlatformRustupTargets } from './install-tools.mjs';
import {
  ROOT,
  buildEnvWithLlvm,
  loadConfig,
  parseWorkspaceVersion,
  producedPackages,
  defaultSelectors,
  normalizePackages,
  resolvePlatforms,
  resolveFromRelease,
  run,
  hostLabel,
  copyDir,
  copyArtifact,
  findArtifacts,
  findExecutable,
  bundleSearchDirs,
  canRun,
  removeDir,
  createArchive,
  writeChecksum,
  fileDigestHex,
  ensureRustcVersion,
} from './lib.mjs';
import { APP_PACKAGE, PORTABLE_PACKAGE, archShort, outputFolderName, familyOf, dockerBundlesFor, hostPlatformSelector, knownPackageNames } from './capabilities.mjs';
import {
  assertCrossImageFor,
  assertDocker,
  crossBuildArgs,
  crossBundleArgs,
  describeCrossBuild,
  describeCrossBundling,
  dockerBuildExecutable,
  ensureCrossImage,
} from './docker.mjs';

function parseArgs(argv) {
  const options = {
    targets: [],
    packages: [],
    list: false,
    showVersion: false,
    noInstall: false,
    skipBuild: false,
    noFrontendBuild: false,
    continueOnError: false,
    dryRun: false,
    forceUseDockerBuilder: false,
    help: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    // Removed flag: give the new syntax instead of a generic unknown-argument
    // error, so existing callers (scripts, CI) get an actionable message.
    if (arg === '--platform' || arg === '-p') {
      throw new Error(
        `The \`${arg}\` flag was removed: pass platform selectors as plain arguments, ` +
          'e.g. `node build-release/release.mjs windows-*`.',
      );
    }
    if (arg === '--package') {
      // Value-taking option: package names to produce, repeatable and
      // comma-separated (`--package deb,portable`).
      const value = argv[i + 1];
      if (value === undefined || value.startsWith('-')) {
        throw new Error('Missing value for `--package` (e.g. `--package deb,portable`).');
      }
      options.packages.push(value);
      i += 1;
    }
    else if (arg === '--list') options.list = true;
    else if (arg === '--version') options.showVersion = true;
    else if (arg === '--no-install') options.noInstall = true;
    else if (arg === '--skip-build') options.skipBuild = true;
    else if (arg === '--no-frontend-build') options.noFrontendBuild = true;
    else if (arg === '--continue-on-error') options.continueOnError = true;
    else if (arg === '--dry-run') options.dryRun = true;
    else if (arg === '--force-use-docker-builder') options.forceUseDockerBuilder = true;
    else if (arg === '--help' || arg === '-h') options.help = true;
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

function help(config, version) {
  const ids = config.platforms.map((p) => p.id).join(', ');
  const hostSelector = hostPlatformSelector() ?? 'all platforms';
  const packageNames = knownPackageNames(config).join(', ');
  console.log(`Usage: node build-release/release.mjs [OPTIONS] [TARGET...]

Cross-platform release builder (host: ${hostLabel()}).

  TARGET...             Build only matching platforms. With no TARGET, only this
                        host platform is built (${hostSelector}). Selectors are
                        exact ids (darwin-arm64), family names (darwin, windows,
                        linux), the alias osx (= darwin), \`*\` for every platform,
                        or trailing-\`*\` patterns (darwin-*). Several targets may
                        be listed, space- or comma-separated; quote \`*\` so the
                        shell keeps it.
                        Available ids: ${ids}
  --package <name>      Only produce the named packages; repeat the flag or
                        separate values with commas. Known names: ${packageNames}
                        (\`all\`/\`*\` = every declared package). Default: every
                        package declared by each selected platform.
  --list                List platforms and whether they can be built on this host
                        (every configured platform unless TARGETs are given).
  --version             Print the project version (from server/Cargo.toml).
  --no-install          Do not install/verify tools before building.
  --skip-build          Reuse existing Tauri build artifacts (target/…); only assemble.
  --no-frontend-build   Do not build the frontend once upfront (reuse its configured dist/).
  --continue-on-error   Continue with remaining platforms after a platform build failure.
  --dry-run             Print the commands that would run (incl. Docker cross builds) and exit.
  --force-use-docker-builder
                        Build Docker-capable platforms (config "build": "docker"
                        declaring a portable block or Linux installers) inside
                        the cross image even when this host could produce them
                        natively. Non-Docker targets stay native.
  --help, -h            Show this help.

The frontend is built once and shared by all targets (each \`cargo tauri build\`
skips its \`beforeBuildCommand\` through a config override). On hosts that cannot
natively bundle a target, the Docker cross image compiles it and additionally
produces the Linux installers (.deb/.rpm/.AppImage) for Linux targets; the
binary is still packaged as a portable archive when configured, and macOS
targets additionally get a \`-app.tar.gz\` archive embedding a
\`<productName>.app\` bundle. Artifacts
follow the \`<product>-<version>-<os>-<arch>.<ext>\` scheme. Every installer gets
companion \`.sha256\` and \`.sha3\` checksum files; a portable archive's checksum file
lists the archive's own hash first, then the hash of every file inside the
archive.

Release version resolved from server/Cargo.toml: ${version}
Output directory: releases/release-<version>/ at the repository root.
`);
}

/** Builds a clean, version-named artifact file name for a bundle type. */
function artifactName(bundleType, platform, version, productName) {
  const os = familyOf(platform.id);
  const arch = archShort(platform.target);
  if (bundleType === 'nsis') return `${productName}-${version}-${os}-${arch}-setup.exe`;
  const ext = bundleType === 'appimage' ? 'AppImage' : bundleType;
  return `${productName}-${version}-${os}-${arch}.${ext}`;
}

/**
 * Debian-safe product name override for Linux installer bundles.
 *
 * The Tauri Debian bundler writes `settings.product_name()` verbatim into the
 * control `Package:` field, which rejects the accented `Arachnéa` from
 * `tauri.conf.json` (dpkg only allows `[a-z0-9+.-]`) and stages resources
 * under `/usr/lib/Arachnéa/`. The RPM name has the same ASCII requirement.
 * Overriding with the ASCII `release-config.json` productName (`arachnea`)
 * for `*-unknown-linux-gnu` targets keeps the display name untouched on
 * macOS/Windows while producing installable Linux packages.
 *
 * @param {object} platform - A platform entry from `release-config.json`.
 * @param {object} config - Release configuration.
 * @returns {string|null} Override product name, or `null` to keep `tauri.conf.json`.
 */
function linuxProductNameOverride(platform, config) {
  return platform.target.includes('-unknown-linux') ? config.productName : null;
}

/**
 * Writes the SHA-256 and SHA-3 checksum files for an artifact and records them
 * in the platform "produced" list.
 *
 * @param {string} filePath - Absolute path of the artifact to checksum.
 * @param {string[]} copied - List accumulating produced file names.
 */
function addChecksums(filePath, copied) {
  // SHA-256
  const hash256 = fileDigestHex(filePath, 'sha256');
  const out256 = `${filePath}.sha256`;
  writeFileSync(out256, `${hash256}  ${path.basename(filePath)}\n`);
  copied.push(path.basename(out256));

  // SHA-3 (SHA3-256)
  const hash3 = fileDigestHex(filePath, 'sha3-256');
  const out3 = `${filePath}.sha3`;
  writeFileSync(out3, `${hash3}  ${path.basename(filePath)}\n`);
  copied.push(path.basename(out3));
}

/**
 * Recursively collects the relative paths (forward slashes) of every file
 * below a staging directory.
 *
 * @param {string} dir - Directory to walk.
 * @returns {string[]} Relative file paths, POSIX-style.
 */
function listStagedFiles(dir) {
  const files = [];
  for (const entry of readdirSync(dir)) {
    const full = path.join(dir, entry);
    if (statSync(full).isDirectory()) {
      files.push(...listStagedFiles(full).map((rel) => path.posix.join(entry, rel)));
    } else {
      files.push(entry);
    }
  }
  return files;
}

/**
 * Writes one `.sha256` and one `.sha3` checksum file for a portable archive.
 * Each checksum file first lists the hash of the archive itself, then the hash
 * of every file staged inside the archive (its uncompressed content), so no
 * archive extraction is ever performed to compute them.
 *
 * @param {string} stagingDir - Directory holding the unpacked archive content.
 * @param {string} archivePath - Absolute path of the produced archive.
 * @param {string[]} copied - List accumulating produced file names.
 */
function addPortableChecksums(stagingDir, archivePath, copied) {
  // SHA-256
  const lines256 = [`${fileDigestHex(archivePath, 'sha256')}  ${path.basename(archivePath)}`];
  for (const rel of listStagedFiles(stagingDir)) {
    lines256.push(`${fileDigestHex(path.join(stagingDir, rel), 'sha256')}  ${rel}`);
  }
  const out256 = `${archivePath}.sha256`;
  writeFileSync(out256, `${lines256.join('\n')}\n`);
  copied.push(path.basename(out256));

  // SHA-3 (SHA3-256)
  const lines3 = [`${fileDigestHex(archivePath, 'sha3-256')}  ${path.basename(archivePath)}`];
  for (const rel of listStagedFiles(stagingDir)) {
    lines3.push(`${fileDigestHex(path.join(stagingDir, rel), 'sha3-256')}  ${rel}`);
  }
  const out3 = `${archivePath}.sha3`;
  writeFileSync(out3, `${lines3.join('\n')}\n`);
  copied.push(path.basename(out3));
}

/** File-name suffixes produced for each artifact (artifact + checksums). */
const ARTIFACT_SUFFIXES = ['', '.sha256', '.sha3'];

/** Portable archive extension: `.tar.gz` for Linux/macOS, `.zip` otherwise. */
function portableExt(platform) {
  return platform.portable && platform.portable.format === 'targz' ? '.tar.gz' : '.zip';
}

/** Version-named portable archive base blob (without extension/checksums). */
function portableBaseName(platform, version, productName) {
  return `${productName}-${version}-${familyOf(platform.id)}-${archShort(platform.target)}-portable`;
}

/** Full version-named portable archive file name. */
function portableArchiveName(platform, version, productName) {
  return `${portableBaseName(platform, version, productName)}${portableExt(platform)}`;
}

/**
 * Version-named macOS `.app` archive file name — a separate archive from the
 * portable one, holding the launchable `<productName>.app` bundle.
 */
function macAppArchiveName(platform, version, productName) {
  return `${productName}-${version}-${familyOf(platform.id)}-${archShort(platform.target)}-app.tar.gz`;
}

/**
 * Human-readable label of a produced package name (`portable` -> `portable
 * archive`, `app` -> `.app archive`); installer bundle types keep their name.
 */
function packageLabel(name) {
  if (name === PORTABLE_PACKAGE) return 'portable archive';
  if (name === APP_PACKAGE) return '.app archive';
  return name;
}

/**
 * Reads the Tauri configuration of the bundled crate (product name, bundle
 * identifier, ...) used to stage the macOS `.app` bundle.
 *
 * @param {object} config - Release configuration.
 * @returns {object} Parsed `tauri.conf.json`.
 */
function readTauriConf(config) {
  return JSON.parse(
    readFileSync(path.join(resolveFromRelease(config.tauriProject), 'tauri.conf.json'), 'utf8'),
  );
}

/**
 * Stages a macOS `.app` bundle inside the `.app` archive staging directory:
 * `<productName>.app/Contents/{Info.plist, PkgInfo, MacOS/<exe>, Resources/}`.
 * Replicates the file layout of the tauri-bundler `app` bundle (pure file
 * operations, no code signing) so the `-app.tar.gz` archive ships a launchable
 * application alongside the raw-binary portable archive. The optional runtime
 * data (`services/`, ...) is staged in `Contents/Resources/`, the Tauri
 * `bundle.resources` location, which the application resource root resolution
 * probes (see `arachnea-core::application`).
 *
 * The `.app` cannot be produced inside the Docker cross image: the Tauri CLI
 * ignores macOS bundle types on a Linux host ("Wrong package type app for
 * platform Linux").
 *
 * @param {string} portableDir - Staging directory holding the archive content.
 * @param {string} exe - Absolute path of the release executable to embed.
 * @param {object} platform - A platform entry with a `portable` block.
 * @param {object} config - Release configuration.
 * @param {string} version - Project version.
 * @returns {{resourcesDir: string, executable: string}} The directory receiving
 *   the runtime data (`Contents/Resources`), i.e. where the resource tree must
 *   be staged, and the archive-relative path of the embedded executable.
 */
function stageMacApp(portableDir, exe, platform, config, version) {
  const tauri = readTauriConf(config);
  const appName = `${tauri.productName}.app`;
  const contents = path.join(portableDir, appName, 'Contents');
  const macosDir = path.join(contents, 'MacOS');
  const resourcesDir = path.join(contents, 'Resources');
  mkdirSync(macosDir, { recursive: true });
  mkdirSync(resourcesDir, { recursive: true });

  const exeName = platform.portable.exeName || 'arachnea';
  const exeDest = path.join(macosDir, exeName);
  copyFileSync(exe, exeDest);
  chmodSync(exeDest, 0o755);

  const plist = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>${tauri.productName}</string>
	<key>CFBundleExecutable</key>
	<string>${exeName}</string>
	<key>CFBundleIconFile</key>
	<string>icon.icns</string>
	<key>CFBundleIdentifier</key>
	<string>${tauri.identifier}</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>${tauri.productName}</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>${version}</string>
	<key>CFBundleVersion</key>
	<string>${version}</string>
	<key>LSMinimumSystemVersion</key>
	<string>10.13</string>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
`;
  writeFileSync(path.join(contents, 'Info.plist'), plist);
  writeFileSync(path.join(contents, 'PkgInfo'), 'APPL????');

  const icon = path.join(resolveFromRelease(config.tauriProject), 'icons', 'icon.icns');
  if (existsSync(icon)) {
    copyFileSync(icon, path.join(resourcesDir, 'icon.icns'));
  } else {
    console.warn(`macOS app icon \`${icon}\` not found; the .app bundle ships without an icon.`);
  }
  return { resourcesDir, executable: `${appName}/Contents/MacOS/${exeName}` };
}

/**
 * Removes previous outputs for the platforms about to be built. When every
 * platform of a family is selected and no package selection narrows the run,
 * the whole family folder is cleared; otherwise only the files belonging to the
 * selected platforms and packages are removed (artifacts plus their
 * `.sha256`/`.sha3` checksums), leaving unselected packages in place.
 *
 * @param {object[]} selected - Enriched platform entries being rebuilt.
 * @param {object} config - Release configuration.
 * @param {string} version - Project version.
 * @param {string} releaseDir - Destination release version directory.
 * @param {Set<string>|null} [packageSelection] - Normalized `--package`
 *   selection; `null` (default) keeps the full-family cleanup behavior.
 */
function cleanPreviousOutputs(selected, config, version, releaseDir, packageSelection = null) {
  // All platform ids of the config, grouped by output family folder.
  const allByFolder = new Map();
  for (const platform of config.platforms) {
    const folder = outputFolderName(platform.id);
    if (!allByFolder.has(folder)) allByFolder.set(folder, new Set());
    allByFolder.get(folder).add(platform.id);
  }

  const selectedByFolder = new Map();
  for (const platform of selected) {
    const folder = outputFolderName(platform.id);
    if (!selectedByFolder.has(folder)) selectedByFolder.set(folder, new Set());
    selectedByFolder.get(folder).add(platform.id);
  }

  for (const [folder, selectedIds] of selectedByFolder) {
    const folderPath = path.join(releaseDir, folder);
    const familyIds = allByFolder.get(folder) ?? new Set();
    const fullFamilyRebuild =
      packageSelection === null && [...familyIds].every((id) => selectedIds.has(id));

    if (fullFamilyRebuild) {
      removeDir(folderPath);
      console.log(`[release] Cleaned ${folder}/ (full family rebuild).`);
      continue;
    }

    // Partial rebuild: remove only this build's artifacts and checksums.
    let removedAny = false;
    for (const platform of selected.filter((p) => outputFolderName(p.id) === folder)) {
      for (const bundleType of platform.bundles) {
        for (const suffix of ARTIFACT_SUFFIXES) {
          const name = `${artifactName(bundleType, platform, version, config.productName)}${suffix}`;
          const file = path.join(folderPath, name);
          if (existsSync(file)) {
            rmSync(file);
            removedAny = true;
          }
        }
      }
      if (platform.packages.portable || platform.packages.app) {
        const archiveNames = [];
        if (platform.packages.portable) {
          archiveNames.push(portableArchiveName(platform, version, config.productName));
        }
        if (platform.packages.app) {
          archiveNames.push(macAppArchiveName(platform, version, config.productName));
        }
        for (const name of archiveNames) {
          for (const suffix of ARTIFACT_SUFFIXES) {
            const file = path.join(folderPath, `${name}${suffix}`);
            if (existsSync(file)) {
              rmSync(file);
              removedAny = true;
            }
          }
        }
      }
    }
    if (removedAny) {
      console.log(`[release] Removed previous ${folder}/ outputs of: ${[...selectedIds].join(', ')}.`);
    }
  }
}

/**
 * Generates SHA256SUMS and SHA3SUMS files in a family directory, containing
 * checksums for every file (except the SUMS files themselves) in that directory.
 *
 * @param {string} familyDir - Absolute path to the family output directory.
 */
function generateFamilyChecksums(familyDir) {
  if (!existsSync(familyDir)) return;
  const files = readdirSync(familyDir)
    .filter((name) => name !== 'SHA256SUMS' && name !== 'SHA3SUMS' && name !== 'CHANGELOG.md')
    .map((name) => path.join(familyDir, name))
    .filter((full) => statSync(full).isFile());

  if (files.length === 0) return;

  // SHA-256
  const lines256 = files.map((file) => {
    const hash = fileDigestHex(file, 'sha256');
    return `${hash}  ${path.basename(file)}`;
  });
  const out256 = path.join(familyDir, 'SHA256SUMS');
  writeFileSync(out256, lines256.join('\n') + '\n');
  console.log(`[release] Generated SHA256SUMS in ${path.basename(familyDir)}/`);

  // SHA-3
  const lines3 = files.map((file) => {
    const hash = fileDigestHex(file, 'sha3-256');
    return `${hash}  ${path.basename(file)}`;
  });
  const out3 = path.join(familyDir, 'SHA3SUMS');
  writeFileSync(out3, lines3.join('\n') + '\n');
  console.log(`[release] Generated SHA3SUMS in ${path.basename(familyDir)}/`);
}

/**
 * Generates root SHA256SUMS and SHA3SUMS files at the release directory root.
 * These files contain checksums for every file located inside subdirectories
 * (family folders) only. Files directly at the root (CHANGELOG.md, etc.) are
 * ignored. Also excludes any file named SHA256SUMS or SHA3SUMS (regardless of
 * location) and CHANGELOG.md to avoid including checksum files themselves.
 *
 * @param {string} releaseDir - The release version directory.
 */
function generateRootChecksums(releaseDir) {
  if (!existsSync(releaseDir)) return;

  // Exclude these base names from checksumming.
  const EXCLUDED_NAMES = new Set(['SHA256SUMS', 'SHA3SUMS', 'CHANGELOG.md']);

  // Collect all files under releaseDir, using relative POSIX paths.
  const allFiles = [];
  function walk(dir, rel) {
    for (const entry of readdirSync(dir)) {
      const full = path.join(dir, entry);
      const relEntry = rel ? path.posix.join(rel, entry) : entry;
      if (statSync(full).isDirectory()) {
        walk(full, relEntry);
      } else {
        allFiles.push(relEntry);
      }
    }
  }
  walk(releaseDir, '');

  // Keep only files that are inside a subdirectory (i.e., contain a '/')
  // and whose base name is not in the exclusion set.
  const subdirFiles = allFiles.filter(
    (rel) => rel.includes('/') && !EXCLUDED_NAMES.has(path.basename(rel)),
  );

  if (subdirFiles.length === 0) {
    console.log('[release] No files in subdirectories; no root checksums generated.');
    return;
  }

  // SHA-256
  const lines256 = subdirFiles.map((rel) => {
    const abs = path.join(releaseDir, rel);
    const hash = fileDigestHex(abs, 'sha256');
    return `${hash}  ${rel}`;
  });
  const out256 = path.join(releaseDir, 'SHA256SUMS');
  writeFileSync(out256, lines256.join('\n') + '\n');
  console.log('[release] Generated root SHA256SUMS');

  // SHA-3
  const lines3 = subdirFiles.map((rel) => {
    const abs = path.join(releaseDir, rel);
    const hash = fileDigestHex(abs, 'sha3-256');
    return `${hash}  ${rel}`;
  });
  const out3 = path.join(releaseDir, 'SHA3SUMS');
  writeFileSync(out3, lines3.join('\n') + '\n');
  console.log('[release] Generated root SHA3SUMS');
}

/**
 * Builds and assembles one platform into `release-<VERSION>/<platform.id>/`.
 *
 * @param {object} platform - Enriched platform entry.
 * @param {object} config - Release configuration.
 * @param {string} version - Project version.
 * @param {string} releaseDir - Destination release version directory.
 * @param {boolean} skipBuild - When true, reuse existing Tauri artifacts instead
 *   of running `cargo tauri build`.
 */
async function buildPlatform(platform, config, version, releaseDir, tauriDir, skipBuild) {
  const label = `[${platform.id}] (${platform.target})`;
  if (platform.method === 'none') {
    console.warn(`${label} not buildable on ${hostLabel()} — installer types [${platform.bundles.join(', ')}] ` +
      'cannot be cross-produced here and no portable Docker build is configured. Skipping.');
    return;
  }

  // A platform whose every package was filtered out by `--package` produces
  // nothing: skip it instead of paying for a full (possibly Docker) compilation.
  const produced = producedPackages(platform);
  if (produced.length === 0) {
    console.warn(`${label} nothing to produce for the selected packages; skipping.`);
    return;
  }

  console.log(`\n===== Building ${label} (${platform.method}) =====`);
  console.log(`Packages: ${produced.map(packageLabel).join(', ')}`);
  if (platform.method === 'native') {
    console.log(`Bundles on this host: ${platform.usable.join(', ') || '(none)'}`);
  } else {
    const dockerBundles = dockerBundlesFor(platform);
    if (dockerBundles.length > 0) {
      console.log(
        `Installers from the cross image: ${dockerBundles.join(', ')}` +
          `${platform.packages.portable ? ', plus the portable archive' : ''}` +
          `${platform.packages.app ? ', plus the .app archive' : ''}`,
      );
    }
  }

  // 1. Build step. Native platforms use the host toolchain via the Tauri CLI;
  //    Docker platforms cross-compile the raw release binary in the container.
  if (!skipBuild && platform.method === 'native') {
    // Linux installer bundles (deb/rpm) need an ASCII product name: the Tauri
    // Debian bundler copies productName verbatim into the control `Package:`
    // field, which rejects the accented `Arachnéa` from tauri.conf.json.
    const linuxProductName = linuxProductNameOverride(platform, config);
    const args = [
      'tauri',
      'build',
      '--target',
      platform.target,
      // The frontend was already built once up front; skip beforeBuildCommand.
      // Bundle types go through the config merge because the CLI restricts the
      // `--bundles` flag to a host-dependent value list (e.g. mac hosts only
      // accept `app`/`dmg`/`ios`, blocking cross NSIS builds).
      '--config',
      JSON.stringify({
        ...(linuxProductName ? { productName: linuxProductName } : {}),
        build: { beforeBuildCommand: null },
        bundle: { targets: platform.usable },
      }),
    ];
    if (platform.runner) args.push('--runner', platform.runner);
    // Augment PATH so cargo-xwin finds llvm-rc installed by install-tools
    // even when the Homebrew LLVM directory is not on the shell PATH.
    const buildEnv = buildEnvWithLlvm();
    // ring invokes `clang` directly on Windows ARM64 while cargo-xwin supplies
    // clang-cl-only `/imsvc` flags. Keep clang-cl for CMake/BoringSSL and route
    // ring through a wrapper that translates those flags for the Unix driver.
    if (process.platform === 'darwin' && platform.target === 'aarch64-pc-windows-msvc') {
      const llvmBin = ['/opt/homebrew/opt/llvm/bin', '/usr/local/opt/llvm/bin']
        .find((dir) => existsSync(path.join(dir, 'clang')));
      if (!llvmBin) {
        throw new Error('Homebrew LLVM `clang` was not found for the macOS Windows ARM64 cross-build.');
      }
      buildEnv.ARACHNEA_XWIN_REAL_CLANG = path.join(llvmBin, 'clang');
      buildEnv.PATH = `${path.join(ROOT, 'build-release')}:${buildEnv.PATH}`;
    }
    run('cargo', args, { cwd: tauriDir, env: buildEnv });
  } else if (!skipBuild && platform.method === 'docker') {
    await assertDocker();
    await ensureCrossImage();
    // Tag presence is not enough: the image must exist for the container
    // platform this target runs in (see multi-platform builds).
    await assertCrossImageFor(platform);
    const dockerBundles = dockerBundlesFor(platform);

    // --- Split compilation and packaging steps ---
    if (dockerBundles.length > 0) {
      // Step 1: compile the raw binary once
      console.log(`${label} cross-compiling release binary in the Docker image: ${await describeCrossBuild(platform, tauriDir)}`);
      run('docker', await crossBuildArgs(platform, tauriDir));

      // Step 2: package each bundle type (the binary is already present).
      // Linux bundles use the ASCII productName override (see native build
      // above); macOS targets have no Docker bundles and keep `Arachnéa`.
      const linuxProductName = linuxProductNameOverride(platform, config);
      console.log(
        `${label} cross-building installers (${dockerBundles.join(', ')}) in the Docker image:\n    ${await describeCrossBundling(platform, tauriDir, linuxProductName)}`,
      );
      const failedBundles = [];
      for (const bundle of dockerBundles) {
        try {
          run('docker', await crossBundleArgs(platform, tauriDir, false, [bundle], linuxProductName));
        } catch (error) {
          failedBundles.push(bundle);
          console.warn(`${label} bundle \`${bundle}\` failed inside the Docker image:\n    ${error.message}`);
        }
      }
      if (failedBundles.length === dockerBundles.length) {
        throw new Error(`all Docker-produced bundles failed inside the image: ${dockerBundles.join(', ')}`);
      }
      if (failedBundles.length > 0) {
        console.warn(
          `${label} partial bundle production (continuing with the rest): ${failedBundles.join(', ')}`,
        );
      }
    } else {
      // No bundles, just compile the binary (e.g., macOS case)
      console.log(`${label} cross-building in the Docker image: ${await describeCrossBuild(platform, tauriDir)}`);
      run('docker', await crossBuildArgs(platform, tauriDir));
    }
    // --- end of split ---

  } else {
    console.log(`${label} --skip-build: reusing existing artifacts in ${tauriDir}/target.`);
  }

  const platformOutDir = path.join(releaseDir, outputFolderName(platform.id));
  const copied = [];

  // 2. Assemble version-named native bundles into the family folder. Both
  //    production methods may yield installers: native hosts produce
  //    `platform.usable`, the cross image produces `dockerBundlesFor`.
  const producedBundles = platform.method === 'native' ? platform.usable : dockerBundlesFor(platform);
  if (producedBundles.length > 0) {
    for (const bundleType of producedBundles) {
      const artifacts = findArtifacts(tauriDir, platform.target, bundleType);
      if (artifacts.length === 0) {
        console.warn(
          `${label} no \`${bundleType}\` artifact found. Searched:\n    ` +
            bundleSearchDirs(tauriDir, platform.target, bundleType).join('\n    '),
        );
        continue;
      }
      const targetName = artifactName(bundleType, platform, version, config.productName);
      const target = path.join(platformOutDir, targetName);
      copyArtifact(artifacts[0], target);
      console.log(`  bundled ${targetName}`);
      copied.push(targetName);
      if (statSync(target).isFile()) {
        addChecksums(target, copied);
      }
    }
  }

  // 3. Optional archives: the portable archive (raw executable + optional
  //    runtime data) and, for macOS targets, a separate `-app` archive
  //    embedding the executable in a launchable `<productName>.app` bundle.
  if (platform.packages.portable || platform.packages.app) {
    const exeName =
      platform.portable.exeName || (platform.target.includes('windows') ? 'arachnea.exe' : 'arachnea');
    // Docker-produced binaries live under `docker-build/<arch>/<triple>/release/`;
    // prefer them so a stray host-built binary in the plain target dir cannot
    // shadow the freshly cross-compiled one.
    const exe =
      platform.method === 'docker'
        ? (dockerBuildExecutable(platform, tauriDir, exeName) ??
          findExecutable(tauriDir, platform.target, exeName))
        : findExecutable(tauriDir, platform.target, exeName);
    if (!exe) {
      console.warn(`${label} release executable not found; skipping portable archive.`);
    } else {
      const sourceDir = resolveFromRelease(config.portableSource);
      const include = platform.portable.include || [];
      const exclude = platform.portable.exclude || [];
      // Copies the optional runtime data below `targetDir`, warning on misses.
      const copyRuntimeData = (targetDir) => {
        for (const rel of include) {
          const src = path.join(sourceDir, rel);
          if (!statSync(src, { throwIfNoEntry: false })) {
            console.warn(`${label} portable include \`${rel}\` not found in ${sourceDir}; skipped.`);
            continue;
          }
          copyDir(src, path.join(targetDir, rel), exclude);
        }
      };

      if (platform.packages.portable) {
        const portableName = portableArchiveName(platform, version, config.productName);
        const portableDir = path.join(releaseDir, `.tmp-${portableName}`);
        removeDir(portableDir);
        mkdirSync(portableDir, { recursive: true });
        copyFileSync(exe, path.join(portableDir, path.basename(exe)));
        copyRuntimeData(portableDir);
        const archivePath = path.join(platformOutDir, portableName);
        console.log(`${label} packaging portable archive ${path.basename(archivePath)}...`);
        // The executable is the only entry that must stay runnable once extracted.
        createArchive(portableDir, archivePath, [path.basename(exe)]);
        copied.push(path.basename(archivePath));
        // One checksum file per algorithm: archive hash first, then every inner
        // file's hash (computed on the staged originals, never via extraction).
        addPortableChecksums(portableDir, archivePath, copied);
        removeDir(portableDir);
      }

      // Separate macOS archive: a launchable `<productName>.app` bundle; the
      // runtime data is staged in `Contents/Resources/` (Tauri bundler
      // layout, probed by the runtime resource root resolution).
      if (platform.packages.app) {
        const appName = macAppArchiveName(platform, version, config.productName);
        const appDir = path.join(releaseDir, `.tmp-${appName}`);
        removeDir(appDir);
        mkdirSync(appDir, { recursive: true });
        const { resourcesDir, executable: macExecutable } = stageMacApp(
          appDir,
          exe,
          platform,
          config,
          version,
        );
        copyRuntimeData(resourcesDir);
        const appArchivePath = path.join(platformOutDir, appName);
        console.log(`${label} packaging macOS .app archive ${path.basename(appArchivePath)}...`);
        createArchive(appDir, appArchivePath, [macExecutable]);
        copied.push(path.basename(appArchivePath));
        addPortableChecksums(appDir, appArchivePath, copied);
        removeDir(appDir);
      }
    }
  }

  if (copied.length === 0) {
    console.warn(`${label} nothing was produced.`);
  }
}

/** Prints the result of every attempted platform build. */
function printBuildSummary(results) {
  console.log('\n===== Build summary =====');
  for (const result of results) {
    const color = result.success ? '\x1b[32m' : '\x1b[31m';
    const status = result.success ? '✓' : '✗';
    const detail = result.success ? 'success' : result.error;
    // Only successful builds produced artifacts; keep failed lines on the error.
    const packages = result.success
      ? ` packages=[${producedPackages(result.platform).join(', ')}]`
      : '';
    console.log(`  ${color}${status}\x1b[0m ${result.platform.target.padEnd(30)} ${detail}${packages}`);
  }
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const config = loadConfig();
  const version = parseWorkspaceVersion();

  if (options.help) {
    help(config, version);
    return;
  }
  if (options.showVersion) {
    console.log(version);
    return;
  }

  // With no target, only the platform matching this host OS+CPU is built;
  // `--list` keeps showing every configured platform (and what this host can
  // produce for each).
  const selectors =
    options.targets.length > 0 ? options.targets : options.list ? [] : defaultSelectors();
  if (options.targets.length === 0 && !options.list) {
    if (selectors.length === 0) {
      console.warn(
        `[release] Unsupported host ${process.platform}/${process.arch}: building every configured platform.`,
      );
    } else {
      console.log(`[release] No target given: building this host platform (${selectors.join(', ')}).`);
    }
  }
  // `--package` selection: validated once (throws on unknown names) and passed
  // to the platform resolution as a set; `null` keeps every declared package.
  const packages = normalizePackages(config, options.packages);
  const platforms = resolvePlatforms(config, selectors, {
    forceUseDockerBuilder: options.forceUseDockerBuilder,
    packages,
  });
  if (packages) {
    const produced = new Set(platforms.flatMap((p) => producedPackages(p)));
    const unmatched = [...packages].filter((name) => !produced.has(name));
    if (unmatched.length > 0) {
      console.warn(
        `[release] Packages not produced by the selected targets: ${unmatched.join(', ')}.`,
      );
    }
  }
  const tauriDir = resolveFromRelease(config.tauriProject);
  const releaseDir = path.join(ROOT, 'releases', `release-${version}`);

  if (options.list) {
    console.log(`Host: ${hostLabel()}`);
    console.log(`Version: ${version}\n`);
    for (const platform of platforms) {
      const produced = producedPackages(platform).map(packageLabel);
      const detail = !platform.buildable
        ? 'not buildable on this host'
        : produced.length > 0
          ? `available (${platform.method}; produces: ${produced.join(', ')})`
          : 'nothing to produce for the selected packages';
      console.log(
        `  ${platform.id.padEnd(20)} -> ${outputFolderName(platform.id).padEnd(10)} ` +
          `${platform.target.padEnd(30)} : ${detail}`,
      );
    }
    return;
  }

  const buildable = platforms.filter((p) => p.buildable && producedPackages(p).length > 0);

  if (options.dryRun) {
    console.log('DRY RUN — no artifact is produced. Planned production:\n');
    for (const platform of platforms) {
      const produced = producedPackages(platform);
      const method = platform.buildable ? platform.method : 'none';
      let command = '';
      if (!platform.buildable) command = '';
      else if (produced.length === 0) command = ' — nothing to produce for the selected packages';
      else if (platform.method === 'docker') {
        const bundleCommand = await describeCrossBundling(platform, tauriDir, linuxProductNameOverride(platform, config));
        if (bundleCommand) command += `\n        ${bundleCommand}`;
        if ((platform.packages.portable || platform.packages.app) && !bundleCommand) command += `\n        ${await describeCrossBuild(platform, tauriDir)}`;
      }
      console.log(`  ${platform.id.padEnd(20)} method=${method} packages=[${produced.join(', ')}]${command}`);
    }
    return;
  }

  if (buildable.length === 0) {
    const packageHint = packages
      ? ` Selected packages: ${[...packages].join(', ')} — none of them can be produced by these targets.`
      : '';
    console.error(`No platform can be built on host ${hostLabel()} with these targets.${packageHint}`);
    if (options.targets.length === 0) {
      console.error('Nothing to do. Run `node build-release/release.mjs --list` for details.');
    }
    process.exit(1);
  }

  if (!canRun('cargo')) {
    throw new Error('`cargo` is not installed. Install Rust first: https://rustup.rs');
  }

  if (!options.noInstall) {
    console.log('[release] Ensuring build tools...');
    await installTools(platforms);
  } else if (!options.skipBuild) {
    // installTools() enforces the rustc floor when it runs; without it, check
    // here so a stale toolchain still fails fast before the frontend build.
    // --skip-build only reassembles prebuilt artifacts without invoking cargo.
    // Offers `rustup update` with confirmation when interactive.
    await ensureRustcVersion();
  }

  // Build the frontend once, shared by every target. Each platform build later
  // runs `cargo tauri build` with a config override disabling `beforeBuildCommand`.
  if (!options.skipBuild && !options.noFrontendBuild) {
    console.log('\n[release] Building the frontend once for all targets...');
    const isWindows = process.platform === 'win32';
    // Node >= 20 cannot `spawn` a Windows `.cmd` shim directly (EINVAL); pass
    // `shell: true` so npm.cmd is resolved through cmd.exe on Windows only.
    run(isWindows ? 'npm.cmd' : 'npm', ['run', 'build'], {
      cwd: resolveFromRelease(config.frontendProject),
      ...(isWindows ? { shell: true } : {}),
    });
  } else {
    console.log(
      `[release] ${options.skipBuild ? '--skip-build' : '--no-frontend-build'}: reusing existing ${config.frontendProject}/dist.`,
    );
  }

  // Only clean what this run rebuilds (family folder or single-platform files).
  mkdirSync(releaseDir, { recursive: true });
  cleanPreviousOutputs(buildable, config, version, releaseDir, packages);

  // Remove existing root SHA256SUMS and SHA3SUMS before building.
  for (const sumFile of ['SHA256SUMS', 'SHA3SUMS']) {
    const sumPath = path.join(releaseDir, sumFile);
    if (existsSync(sumPath)) {
      rmSync(sumPath);
      console.log(`[release] Removed existing root ${sumFile}`);
    }
  }

  // Remove existing SUMS files in each family folder that will be rebuilt.
  const families = new Set(buildable.map((p) => outputFolderName(p.id)));
  for (const family of families) {
    const familyDir = path.join(releaseDir, family);
    for (const sumFile of ['SHA256SUMS', 'SHA3SUMS']) {
      const sumPath = path.join(familyDir, sumFile);
      if (existsSync(sumPath)) {
        rmSync(sumPath);
        console.log(`[release] Removed existing ${sumPath}`);
      }
    }
  }

  const results = [];
  for (const platform of buildable) {
    try {
      // Per-platform pre-flight guard: make sure this platform's rustup target
      // is installed before running the native build so a missing target (e.g.
      // `aarch64-pc-windows-msvc`) cannot abort `cargo tauri build` with a
      // `can't find crate for core/std` error. Missing targets are installed
      // automatically after explicit confirmation.
      if (platform.method === 'native' && !options.skipBuild) {
        await ensurePlatformRustupTargets(platform);
      }
      await buildPlatform(platform, config, version, releaseDir, tauriDir, options.skipBuild);
      results.push({ platform, success: true });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      results.push({ platform, success: false, error: message });
      console.error(`\n[release] ${platform.id} failed: ${message}`);
      if (!options.continueOnError) break;
    }
  }

  const failed = results.filter((result) => !result.success);
  if (failed.length > 0 && !options.continueOnError) {
    printBuildSummary(results);
    process.exitCode = 1;
    return;
  }

  // Copy the project CHANGELOG into the release folder root only.
  const changelog = resolveFromRelease(config.changelog);
  copyFileSync(changelog, path.join(releaseDir, 'CHANGELOG.md'));
  console.log('\n[release] Copied CHANGELOG.md into the release folder.');

  // Regenerate family checksums for each rebuilt family.
  for (const family of families) {
    const familyDir = path.join(releaseDir, family);
    generateFamilyChecksums(familyDir);
  }

  // Generate root SHA256SUMS and SHA3SUMS (after family SUMS are created).
  generateRootChecksums(releaseDir);

  console.log('\n[release] Done.');
  console.log(`Release directory : ${releaseDir}`);
  for (const platform of buildable) {
    const folder = outputFolderName(platform.id);
    console.log(`  ${platform.id}: ${path.join(releaseDir, folder)}`);
  }

  printBuildSummary(results);
  if (failed.length > 0) process.exitCode = 1;
}

main();

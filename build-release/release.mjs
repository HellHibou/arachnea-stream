// Arachnea cross-platform release builder (100% local, Node).
//
// Builds the requested platforms with `cargo tauri build`, assembles a
// `releases/release-<VERSION>/<platform>/` directory with version-named
// installer artifacts and copies the project CHANGELOG into the release
// folder. Windows platforms additionally produce a portable `.zip`
// (executable + runtime data).
import path from 'node:path';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
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
} from './lib.mjs';
import { archShort, outputFolderName, familyOf, dockerBundlesFor } from './capabilities.mjs';
import {
  assertCrossImageFor,
  assertDocker,
  crossBuildArgs,
  crossBundleArgs,
  describeCrossBuild,
  describeCrossBundling,
  ensureCrossImage,
} from './docker.mjs';

function parseArgs(argv) {
  const options = {
    platforms: [],
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
    if (arg === '--platform' || arg === '-p') {
      // Accept repeated flags and comma-separated selectors (`darwin-*` etc.).
      for (const part of argv[++i].split(',')) {
        const selector = part.trim();
        if (selector) options.platforms.push(selector);
      }
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
    else throw new Error(`Unknown argument: ${arg}`);
  }
  return options;
}

function help(config, version) {
  const ids = config.platforms.map((p) => p.id).join(', ');
  console.log(`Usage: node build-release/release.mjs [OPTIONS]

Cross-platform release builder (host: ${hostLabel()}).

  --platform, -p <sel>  Build only matching platforms. Selectors are exact ids
                        (darwin-arm64), family names (darwin, windows, linux,
                        alias osx = darwin), or \`*\` patterns (darwin-*).
                        Repeat the flag or separate values with commas.
                        Available ids: ${ids}
  --list                List platforms and whether they can be built on this host.
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
raw binary is still packaged as a portable archive when configured. Artifacts
follow the \`<product>-<version>-<os>-<arch>.<ext>\` scheme. Every installer gets
companion \`.sha256\`/\`.md5\` checksum files; a portable archive's checksum file
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
 * Writes the SHA-256 and MD5 checksum files for an artifact and records them
 * in the platform "produced" list.
 *
 * @param {string} filePath - Absolute path of the artifact to checksum.
 * @param {string[]} copied - List accumulating produced file names.
 */
function addChecksums(filePath, copied) {
  for (const algorithm of ['sha256', 'md5']) {
    const checksum = writeChecksum(filePath, algorithm);
    copied.push(path.basename(checksum));
  }
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
 * Writes one `.sha256` and one `.md5` checksum file for a portable archive.
 * Each checksum file first lists the hash of the archive itself, then the hash
 * of every file staged inside the archive (its uncompressed content), so no
 * archive extraction is ever performed to compute them.
 *
 * @param {string} stagingDir - Directory holding the unpacked archive content.
 * @param {string} archivePath - Absolute path of the produced archive.
 * @param {string[]} copied - List accumulating produced file names.
 */
function addPortableChecksums(stagingDir, archivePath, copied) {
  for (const algorithm of ['sha256', 'md5']) {
    const extension = algorithm === 'md5' ? 'md5' : 'sha256';
    const lines = [`${fileDigestHex(archivePath, algorithm)}  ${path.basename(archivePath)}`];
    for (const rel of listStagedFiles(stagingDir)) {
      lines.push(`${fileDigestHex(path.join(stagingDir, rel), algorithm)}  ${rel}`);
    }
    const out = `${archivePath}.${extension}`;
    writeFileSync(out, `${lines.join('\n')}\n`);
    copied.push(path.basename(out));
  }
}

/** File-name suffixes produced for each artifact (artifact + checksums). */
const ARTIFACT_SUFFIXES = ['', '.sha256', '.md5'];

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
 * Removes previous outputs for the platforms about to be built. When every
 * platform of a family is selected, the whole family folder is cleared;
 * otherwise only the files belonging to the selected platforms are removed
 * (artifacts plus their `.sha256`/`.md5` checksums).
 *
 * @param {object[]} selected - Enriched platform entries being rebuilt.
 * @param {object} config - Release configuration.
 * @param {string} version - Project version.
 * @param {string} releaseDir - Destination release version directory.
 */
function cleanPreviousOutputs(selected, config, version, releaseDir) {
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
    const fullFamilyRebuild = [...familyIds].every((id) => selectedIds.has(id));

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
      if (platform.portable) {
        const portableName = portableArchiveName(platform, version, config.productName);
        for (const suffix of ARTIFACT_SUFFIXES) {
          const file = path.join(folderPath, `${portableName}${suffix}`);
          if (existsSync(file)) {
            rmSync(file);
            removedAny = true;
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
 * Builds and assembles one platform into `release-<VERSION>/<platform.id>/`.
 *
 * @param {object} platform - Enriched platform entry.
 * @param {object} config - Release configuration.
 * @param {string} version - Project version.
 * @param {string} releaseDir - Destination release version directory.
 * @param {boolean} skipBuild - When true, reuse existing Tauri artifacts instead
 *   of running `cargo tauri build`.
 */
function buildPlatform(platform, config, version, releaseDir, tauriDir, skipBuild) {
  const label = `[${platform.id}] (${platform.target})`;
  if (platform.method === 'none') {
    console.warn(`${label} not buildable on ${hostLabel()} — installer types [${platform.bundles.join(', ')}] ` +
      'cannot be cross-produced here and no portable Docker build is configured. Skipping.');
    return;
  }

  console.log(`\n===== Building ${label} (${platform.method}) =====`);
  if (platform.method === 'native') {
    console.log(`Bundles on this host: ${platform.usable.join(', ')}`);
  } else {
    const dockerBundles = dockerBundlesFor(platform);
    if (dockerBundles.length > 0) {
      console.log(
        `Installers from the cross image: ${dockerBundles.join(', ')}` +
          `${platform.portable ? ', plus the portable archive' : ''}`,
      );
    }
  }

  // 1. Build step. Native platforms use the host toolchain via the Tauri CLI;
  //    Docker platforms cross-compile the raw release binary in the container.
  if (!skipBuild && platform.method === 'native') {
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
        build: { beforeBuildCommand: null },
        bundle: { targets: platform.usable },
      }),
    ];
    if (platform.runner) args.push('--runner', platform.runner);
    // Augment PATH so cargo-xwin finds llvm-rc installed by install-tools
    // even when the Homebrew LLVM directory is not on the shell PATH.
    run('cargo', args, { cwd: tauriDir, env: buildEnvWithLlvm() });
  } else if (!skipBuild && platform.method === 'docker') {
    assertDocker();
    ensureCrossImage();
    // Tag presence is not enough: the image must exist for the container
    // platform this target runs in (see multi-platform builds).
    assertCrossImageFor(platform);
    const dockerBundles = dockerBundlesFor(platform);
    if (dockerBundles.length > 0) {
      // Each `cargo tauri build` pass compiles the release binary AND one
      // installer type. One pass per type is required: bundling several types
      // in a single process crashes the Tauri bundler (see crossBundleArgs).
      console.log(
        `${label} cross-building installers (${dockerBundles.join(', ')}) in the Docker image:\n    ${describeCrossBundling(platform, tauriDir)}`,
      );
      // Try each bundle independently so a container without FUSE (e.g. Docker
      // Desktop on macOS) can still ship deb/rpm when only .AppImage fails.
      const failedBundles = [];
      for (const bundle of dockerBundles) {
        try {
          run('docker', crossBundleArgs(platform, tauriDir, false, [bundle]));
        } catch (error) {
          failedBundles.push(bundle);
          console.warn(`${label} bundle \`${bundle}\` failed inside the Docker image:\n    ${error.message}`);
        }
      }
      if (failedBundles.length === dockerBundles.length) {
        throw new Error(
          `all Docker-produced bundles failed inside the image: ${dockerBundles.join(', ')}`,
        );
      }
      if (failedBundles.length > 0) {
        console.warn(
          `${label} partial bundle production (continuing with the rest): ${failedBundles.join(', ')}`,
        );
      }
    } else {
      console.log(`${label} cross-building in the Docker image: ${describeCrossBuild(platform, tauriDir)}`);
      run('docker', crossBuildArgs(platform, tauriDir));
    }
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

  // 3. Optional portable archive: the raw executable + optional runtime data.
  if (platform.portable) {
    const portableName = portableArchiveName(platform, version, config.productName);
    const portableDir = path.join(releaseDir, `.tmp-${portableName}`);
    removeDir(portableDir);
    const exeName =
      platform.portable.exeName || (platform.target.includes('windows') ? 'arachnea.exe' : 'arachnea');
    const exe = findExecutable(tauriDir, platform.target, exeName);
    if (!exe) {
      console.warn(`${label} release executable not found; skipping portable archive.`);
    } else {
      mkdirSync(portableDir, { recursive: true });
      copyFileSync(exe, path.join(portableDir, path.basename(exe)));
      const sourceDir = resolveFromRelease(config.portableSource);
      const include = platform.portable.include || [];
      const exclude = platform.portable.exclude || [];
      for (const rel of include) {
        const src = path.join(sourceDir, rel);
        if (!statSync(src, { throwIfNoEntry: false })) {
          console.warn(`${label} portable include \`${rel}\` not found in ${sourceDir}; skipped.`);
          continue;
        }
        copyDir(src, path.join(portableDir, rel), exclude);
      }
      const archivePath = path.join(platformOutDir, portableName);
      console.log(`${label} packaging portable archive ${path.basename(archivePath)}...`);
      createArchive(portableDir, archivePath);
      copied.push(path.basename(archivePath));
      // One checksum file per algorithm: archive hash first, then every inner
      // file's hash (computed on the staged originals, never via extraction).
      addPortableChecksums(portableDir, archivePath, copied);
      removeDir(portableDir);
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
    const status = result.success ? '✓' : '✗';
    const detail = result.success ? 'success' : result.error;
    console.log(`  ${status} ${result.platform.target.padEnd(30)} ${detail}`);
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

  const platforms = resolvePlatforms(config, options.platforms, {
    forceUseDockerBuilder: options.forceUseDockerBuilder,
  });
  const tauriDir = resolveFromRelease(config.tauriProject);
  const releaseDir = path.join(ROOT, 'releases', `release-${version}`);

  if (options.list) {
    console.log(`Host: ${hostLabel()}`);
    console.log(`Version: ${version}\n`);
    for (const platform of platforms) {
      let produces;
      if (!platform.buildable) produces = 'nothing';
      else if (platform.method === 'native') produces = platform.usable.join(', ') || 'nothing';
      else {
        const parts = dockerBundlesFor(platform);
        if (platform.portable) parts.push('portable archive');
        produces = parts.join(', ') || 'portable archive';
      }
      const detail = platform.buildable
        ? `available (${platform.method}; produces: ${produces})`
        : 'not buildable on this host';
      console.log(
        `  ${platform.id.padEnd(20)} -> ${outputFolderName(platform.id).padEnd(10)} ` +
          `${platform.target.padEnd(30)} bundles=[${platform.bundles.join(', ')}] : ${detail}`,
      );
    }
    return;
  }

  const buildable = platforms.filter((p) => p.buildable);

  if (options.dryRun) {
    console.log('DRY RUN — no artifact is produced. Planned production:\n');
    for (const platform of platforms) {
      const method = platform.buildable ? platform.method : 'none';
      let command = '';
      if (!platform.buildable) command = '';
      else if (platform.method === 'docker') {
        const bundleCommand = describeCrossBundling(platform, tauriDir);
        if (bundleCommand) command += `\n        ${bundleCommand}`;
        if (platform.portable && !bundleCommand) command += `\n        ${describeCrossBuild(platform, tauriDir)}`;
      }
      console.log(`  ${platform.id.padEnd(20)} method=${method}${command}`);
    }
    return;
  }

  if (buildable.length === 0) {
    console.error(`No platform can be built on host ${hostLabel()} with these targets.`);
    if (options.platforms.length === 0) {
      console.error('Nothing to do. Run `node build-release/release.mjs --list` for details.');
    }
    process.exit(1);
  }

  if (!options.noInstall) {
    console.log('[release] Ensuring build tools...');
    await installTools(platforms);
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

  if (!canRun('cargo')) {
    throw new Error('`cargo` is not installed. Install Rust first: https://rustup.rs');
  }

  // Only clean what this run rebuilds (family folder or single-platform files).
  mkdirSync(releaseDir, { recursive: true });
  cleanPreviousOutputs(buildable, config, version, releaseDir);
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
      buildPlatform(platform, config, version, releaseDir, tauriDir, options.skipBuild);
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

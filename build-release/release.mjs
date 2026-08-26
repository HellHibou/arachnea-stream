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
  rmSync,
  statSync,
} from 'node:fs';
import { installTools } from './install-tools.mjs';
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
  createZip,
  writeChecksum,
} from './lib.mjs';
import { archShort, outputFolderName } from './capabilities.mjs';

function parseArgs(argv) {
  const options = {
    platforms: [],
    list: false,
    showVersion: false,
    noInstall: false,
    skipBuild: false,
    noFrontendBuild: false,
    continueOnError: false,
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
  --help, -h            Show this help.

The frontend is built once and shared by all targets (each \`cargo tauri build\`
skips its \`beforeBuildCommand\` through a config override). Every installer and
portable zip gets a companion \`.sha256\` checksum file.

Release version resolved from server/Cargo.toml: ${version}
Output directory: releases/release-<version>/ at the repository root.
`);
}

/** Builds a clean, version-named artifact file name for a bundle type. */
function artifactName(bundleType, platform, version, productName) {
  const arch = archShort(platform.target);
  if (bundleType === 'nsis') return `${productName}_${version}_${arch}-setup.exe`;
  if (bundleType === 'rpm') return `${productName}-${version}-1.${arch}.rpm`;
  if (bundleType === 'appimage') return `${productName}_${version}_${arch}.AppImage`;
  if (bundleType === 'app') return `${productName}_${version}_${arch}.app`;
  return `${productName}_${version}_${arch}.${bundleType}`;
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

/** File-name suffixes produced for each artifact (artifact + checksums). */
const ARTIFACT_SUFFIXES = ['', '.sha256', '.md5'];

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
        const zipName = `${config.productName}_${version}_${archShort(platform.target)}-portable.zip`;
        for (const suffix of ARTIFACT_SUFFIXES) {
          const file = path.join(folderPath, `${zipName}${suffix}`);
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
  if (!platform.buildable) {
    console.warn(`${label} not buildable on ${hostLabel()} — installer types [${platform.bundles.join(', ')}] ` +
      'cannot be cross-produced here. Skipping.');
    return;
  }

  console.log(`\n===== Building ${label} =====`);
  console.log(`Bundles on this host: ${platform.usable.join(', ')}`);

  // 1. Build the installers via the Tauri CLI (unless reusing existing artifacts).
  if (!skipBuild) {
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
  } else {
    console.log(`${label} --skip-build: reusing existing artifacts in ${tauriDir}/target.`);
  }

  // 2. Assemble version-named artifacts into the platform family folder.
  const platformOutDir = path.join(releaseDir, outputFolderName(platform.id));
  const copied = [];
  for (const bundleType of platform.usable) {
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

  // 3. Optional portable folder (Windows): executable + runtime data, zipped.
  if (platform.portable) {
    const portableName = `${config.productName}_${version}_${archShort(platform.target)}-portable`;
    const portableDir = path.join(releaseDir, `.tmp-${portableName}`);
    removeDir(portableDir);
    const exeName = platform.portable.exeName || (process.platform === 'win32' ? 'arachnea.exe' : 'arachnea');
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
      const zipPath = path.join(platformOutDir, `${portableName}.zip`);
      console.log(`${label} packaging portable archive ${path.basename(zipPath)}...`);
      createZip(portableDir, zipPath);
      copied.push(path.basename(zipPath));
      addChecksums(zipPath, copied);
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

function main() {
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

  const platforms = resolvePlatforms(config, options.platforms);
  const tauriDir = resolveFromRelease(config.tauriProject);
  const releaseDir = path.join(ROOT, 'releases', `release-${version}`);

  if (options.list) {
    console.log(`Host: ${hostLabel()}`);
    console.log(`Version: ${version}\n`);
    for (const platform of platforms) {
      const detail = platform.buildable
        ? `available (produces: ${platform.usable.join(', ')})`
        : 'not buildable on this host';
      console.log(
        `  ${platform.id.padEnd(20)} -> ${outputFolderName(platform.id).padEnd(10)} ` +
          `${platform.target.padEnd(30)} bundles=[${platform.bundles.join(', ')}] : ${detail}`,
      );
    }
    return;
  }

  const buildable = platforms.filter((p) => p.buildable);
  if (buildable.length === 0) {
    console.error(`No platform can be built on host ${hostLabel()} with these targets.`);
    if (options.platforms.length === 0) {
      console.error('Nothing to do. Run `node build-release/release.mjs --list` for details.');
    }
    process.exit(1);
  }

  if (!options.noInstall) {
    console.log('[release] Ensuring build tools...');
    installTools(platforms);
  }

  // Build the frontend once, shared by every target. Each platform build later
  // runs `cargo tauri build` with a config override disabling `beforeBuildCommand`.
  if (!options.skipBuild && !options.noFrontendBuild) {
    console.log('\n[release] Building the frontend once for all targets...');
    const npmCmd = process.platform === 'win32' ? 'npm.cmd' : 'npm';
    run(npmCmd, ['run', 'build'], { cwd: resolveFromRelease(config.frontendProject) });
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

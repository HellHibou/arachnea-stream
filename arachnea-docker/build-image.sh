#!/bin/bash
set -e

# Build the arachnea-docker image with `docker compose build`, resolving the
# release version to package.
#
# Usage:
#   ./build-image.sh [--version=<version>] [docker compose build options]
#
# The version is the ARACHNEA_VERSION build variable of docker-compose.yml.
# When --version is omitted, the highest `release-*` folder of the release tree
# is used (`../releases`, or ARACHNEA_RELEASES_DIR from the environment or
# `.env`); with no release folder and no --version argument the script stops
# with an error. Any other argument is forwarded to `docker compose build`,
# including `--no-cache` to force a full rebuild of the image.

usage() {
  cat <<'EOF'
Build the arachnea-docker image (docker compose build).

Usage:
  ./build-image.sh [--version=<version>] [docker compose build options]

Options:
  --version=<version>  Value of the ARACHNEA_VERSION build variable.
                       Default: highest `release-*` folder of the release tree.
  -h, --help           Show this help.

The release tree is `../releases`, or `ARACHNEA_RELEASES_DIR` (environment or
`.env`). Any other argument is forwarded to `docker compose build`: pass
`--no-cache` to force a full rebuild of every layer, `--progress=plain` or
`--push` to change the output.
EOF
}

die() {
  echo "Error: $*" >&2
  exit 1
}

# The build reads `.env`, the compose file and `../releases` relative to this
# folder, so always work from there.
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"

# --- Arguments --------------------------------------------------------------
# Rejects `--version=`, `--version --no-cache` and other empty/option values.
set_version() {
  [ -n "$1" ] && [ "${1#-}" = "$1" ] \
    || die "--version requires a value (--version=<version>)."
  VERSION="$1"
}

VERSION=""
EXTRA_ARGS=()

while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    --version=*)
      set_version "${1#--version=}"
      ;;
    --version)
      [ $# -ge 2 ] || die "--version requires a value (--version=<version>)."
      shift
      set_version "$1"
      ;;
    *)
      EXTRA_ARGS+=("$1")
      ;;
  esac
  shift
done

# --- Release tree root (environment > .env > ../releases) -------------------
RELEASES_DIR="${ARACHNEA_RELEASES_DIR:-}"
if [ -z "$RELEASES_DIR" ] && [ -f .env ]; then
  RELEASES_DIR="$(sed -n 's/^[[:space:]]*ARACHNEA_RELEASES_DIR[[:space:]]*=[[:space:]]*//p' .env | tail -n 1)"
  case "$RELEASES_DIR" in
    \"*\") RELEASES_DIR="${RELEASES_DIR#\"}"; RELEASES_DIR="${RELEASES_DIR%\"}" ;;
  esac
fi
RELEASES_DIR="${RELEASES_DIR:-../releases}"

# --- Version: --version argument, else the highest `release-*` folder -------
if [ -z "$VERSION" ]; then
  echo "No --version argument: looking for the highest release in $RELEASES_DIR/release-*..."
  VERSIONS=""
  for dir in "$RELEASES_DIR"/release-*/; do
    [ -d "$dir" ] || continue
    VERSIONS="$VERSIONS$(basename "$dir" | sed 's/^release-//')"$'\n'
  done
  if [ -n "$VERSIONS" ]; then
    # `sort -V` compares version numbers themselves (0.9.0 < 0.10.0).
    VERSION="$(printf '%s' "$VERSIONS" | sed '/^$/d' | sort -V | tail -n 1)"
  fi
  if [ -z "$VERSION" ]; then
    die "no --version argument and no \`release-*\` folder under $RELEASES_DIR.
       Build a release first (node build-release/release.mjs) or pass --version=<version>."
  fi
  echo "Highest release found: $VERSION"
fi

# --- Fail before Docker when the release tree lacks the archives -----------
RELEASE_DIR="$RELEASES_DIR/release-$VERSION"
for arch in amd64 arm64; do
  archive="$RELEASE_DIR/linux/arachnea-$VERSION-linux-$arch-portable.tar.gz"
  [ -f "$archive" ] || die "missing $archive
       (docker-compose.yml expands ARACHNEA_AMD64_ARCHIVE/ARACHNEA_ARM64_ARCHIVE
       to exactly these two paths and the Dockerfile copies both of them)."
done

command -v docker >/dev/null 2>&1 || die "docker is required but was not found in PATH."

# Compose otherwise follows the globally selected Buildx builder. A
# `docker-container` builder keeps a successful multi-platform result only in
# its BuildKit cache unless it is explicitly pushed/exported, so `docker images`
# remains empty and `docker compose up` tries to rebuild `release-latest`.
# The builder named after the active Docker context uses the local image store
# and makes both tags immediately available to Compose.
LOCAL_BUILDER="$(docker context show)"
docker buildx inspect "$LOCAL_BUILDER" >/dev/null 2>&1 \
  || die "no local Buildx builder found for Docker context '$LOCAL_BUILDER'."

# Exported: the environment wins over `.env`, so the image tag, the two archive
# paths and the stamped `ENV ARACHNEA_VERSION` all follow this version.
export ARACHNEA_VERSION="$VERSION"
echo "Building arachnea-stream:$VERSION from $RELEASE_DIR (ARACHNEA_VERSION=$VERSION)"
# `--no-cache` is not forced: pass it as an extra argument (see usage) to
# rebuild every layer instead of reusing the cache of a previous release.
docker compose build --builder "$LOCAL_BUILDER" "${EXTRA_ARGS[@]}"

docker image inspect "arachnea-stream:$VERSION" >/dev/null 2>&1 \
  || die "the build completed but arachnea-stream:$VERSION was not exported to the local Docker image store."
docker image inspect arachnea-stream:latest >/dev/null 2>&1 \
  || die "the build completed but arachnea-stream:latest was not exported to the local Docker image store."

echo
echo "Image arachnea-stream:$VERSION is ready. Start it with (from $SCRIPT_DIR):"
echo "docker compose up -d"

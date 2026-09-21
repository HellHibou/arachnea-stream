#!/bin/bash
set -e

# Change to the script directory
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
chmod +x "$SCRIPT_DIR/clang"

# Check if Node is already installed
if command -v node >/dev/null 2>&1; then
  echo "Node.js is already installed: $(node -v)"
else
  echo "Node.js is not installed. Installing the latest version..."

  # Get the latest version (Current)
  VERSION=$(curl -fsSL https://nodejs.org/dist/index.json | grep -o '"version":"v[0-9.]*"' | head -1 | cut -d'"' -f4 | sed 's/^v//')
  if [ -z "$VERSION" ]; then
    echo "Error: unable to retrieve Node.js version"
    exit 1
  fi
  echo "Latest version detected: v$VERSION"

  # Detect OS / architecture
  ARCH=$(uname -m)
  OS=$(uname -s | tr '[:upper:]' '[:lower:]')

  case $ARCH in
    x86_64)  NODE_ARCH="x64" ;;
    aarch64|arm64) NODE_ARCH="arm64" ;;
    *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
  esac

  case $OS in
    linux)  PLATFORM="linux" ;;
    darwin) PLATFORM="darwin" ;;
    *) echo "Unsupported OS: $OS"; exit 1 ;;
  esac

  FILENAME="node-v${VERSION}-${PLATFORM}-${NODE_ARCH}.tar.xz"
  URL="https://nodejs.org/dist/v${VERSION}/${FILENAME}"
  INSTALL_DIR="/usr/local"

  echo "Downloading $URL to $SCRIPT_DIR..."
  curl -fsSL "$URL" -o "$SCRIPT_DIR/$FILENAME"

  echo "Extracting to $INSTALL_DIR..."
  sudo tar -xJf "$SCRIPT_DIR/$FILENAME" -C "$INSTALL_DIR" --strip-components=1
  rm -f "$SCRIPT_DIR/$FILENAME"

  echo "Installation complete."
  echo "Installed version: $(node -v)"
  echo "npm: $(npm -v)"
fi

# === Launch node with all arguments passed to the script ===
node release.mjs $@

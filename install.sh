#!/bin/bash
SCRIPT_DIR="$(pwd)"

chmod +x "$0"
chmod +x "$SCRIPT_DIR/build.sh"
chmod +x "$SCRIPT_DIR/build-release/build-release.sh"

cd "$SCRIPT_DIR/front"
npm update

cd "$SCRIPT_DIR/front/admin-app"
npm update

cd "$SCRIPT_DIR/front/public-app"
npm update

cd "$SCRIPT_DIR"

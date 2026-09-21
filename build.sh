#!/bin/bash
SCRIPT_DIR="$(pwd)"

chmod +x "$0"

chmod +x $SCRIPT_DIR/build-release/build-release.sh
$SCRIPT_DIR/build-release/build-release.sh $*
cd "$SCRIPT_DIR"

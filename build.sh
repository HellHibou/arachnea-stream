#!/bin/bash
SCRIPT_DIR="$(pwd)"

$SCRIPT_DIR/build-release/build-release.sh $*
cd "$SCRIPT_DIR"

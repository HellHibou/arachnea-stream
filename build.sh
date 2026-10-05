#!/bin/bash
set -e

CALL_DIR="$(pwd)"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

chmod +x "$0"

chmod +x "$SCRIPT_DIR/build-release/build-release.sh"

# Shells expand an unquoted `*` before this script starts. When the received
# arguments contain every visible entry from the invocation directory, replace
# that expansion with the release builder's literal all-platform selector.
glob_entries=("$CALL_DIR"/*)
glob_matches=0
for argument in "$@"; do
  for entry in "${glob_entries[@]}"; do
    if [ "$argument" = "$(basename "$entry")" ]; then
      glob_matches=$((glob_matches + 1))
      break
    fi
  done
done

if [ "${#glob_entries[@]}" -gt 0 ] && [ "$glob_matches" -eq "${#glob_entries[@]}" ]; then
  forwarded_args=("*")
  for argument in "$@"; do
    expanded_entry=false
    for entry in "${glob_entries[@]}"; do
      if [ "$argument" = "$(basename "$entry")" ]; then
        expanded_entry=true
        break
      fi
    done
    if [ "$expanded_entry" = false ]; then
      forwarded_args+=("$argument")
    fi
  done
  exec "$SCRIPT_DIR/build-release/build-release.sh" "${forwarded_args[@]}"
fi

exec "$SCRIPT_DIR/build-release/build-release.sh" "$@"

#!/bin/sh
# Arachnea Stream container entrypoint.
#
# Starts the single Xvfb virtual display used by headed Chromium on headless
# servers, then execs the `arachnea` server binary with the port/network mode
# taken from the environment. chaser-cf reuses this display by default rather
# than starting a competing Xvfb instance of its own.
#
# Environment:
#   ARACHNEA_PORT     REST server port (default: 8080).
#   ARACHNEA_NETWORK  `local` | `private` | `public` (default: `public`, so
#                     host-forwarded traffic through published ports is
#                     accepted; see README for the tradeoff).
#   ARACHNEA_ROOT     Public URL prefix without slashes (default: empty, i.e.
#                     no prefix). Pinned as `entrypoint_root` in config.json;
#                     the app is then served from `/<prefix>/` and `GET /`
#                     redirects there.
#   ARACHNEA_DATA_DIR writable application `data/` directory (default:
#                     `<exe dir>/data`, i.e. `/app/data`). `arachnea` resolves
#                     its data root as the executable directory when the latter
#                     is writable (portable layout), so the persisted
#                     configuration is read from `$ARACHNEA_DATA_DIR/config.json`
#                     and everything else (caches, session stores) lives under
#                     that directory too. Mount the persistence volume here.
#   ARACHNEA_BIN      `arachnea` executable (default: /app/arachnea).
#   DISPLAY           X display used by Xvfb/chaser-cf (default: :99).
#   CHROME_BIN        Chromium binary used by chaser-cf
#                     (default: /usr/bin/chromium).
#   CHASER_EXTRA_ARGS extra Chromium flags appended to chaser-cf defaults.
#                     `--disable-dev-shm-usage` is always ensured (small
#                     `/dev/shm` in containers crashes Chromium tabs).
# Any extra CLI arguments are forwarded to `arachnea` verbatim; the binary
# only accepts valueless flags (`--no-tray`, `--desktop`, `--server`), so
# valued options must go through `$ARACHNEA_DATA_DIR/config.json` or the
# administration API.
set -eu

APP_BIN="${ARACHNEA_BIN:-/app/arachnea}"
APP_DIR="$(dirname "$APP_BIN")"
PORT="${ARACHNEA_PORT:-8080}"
NETWORK="${ARACHNEA_NETWORK:-public}"
# Public URL prefix without leading/trailing slash. An empty value means "no
# prefix": the key is dropped from config.json so the app serves `/` directly.
ROOT="$(printf '%s' "${ARACHNEA_ROOT:-}" | sed -e 's|^/*||' -e 's|/*$||')"
DATA_DIR="${ARACHNEA_DATA_DIR:-$APP_DIR/data}"
DISPLAY_NUM="${DISPLAY:-:99}"
CHROME_BIN="${CHROME_BIN:-/usr/bin/chromium}"

echo "Arachnea Stream ${ARACHNEA_VERSION:-unknown} starting (port=$PORT network=$NETWORK root=${ROOT:-/} chrome=$CHROME_BIN display=$DISPLAY_NUM data=$DATA_DIR)"

# Writable state: keep HOME/XDG on the volume so caches, Chromium profiles
# and the per-user data root fallback all survive container recreation.
mkdir -p "$DATA_DIR"
export HOME="$DATA_DIR"
export XDG_DATA_HOME="$DATA_DIR"
export XDG_CONFIG_HOME="$DATA_DIR/.config"
export XDG_CACHE_HOME="$DATA_DIR/.cache"
export CHROME_BIN="$CHROME_BIN"
export DISPLAY="$DISPLAY_NUM"

# Chromium crashes without `--disable-dev-shm-usage` when /dev/shm is small
# (default 64M in Docker). `--no-sandbox` stays opt-in: the image runs as a
# non-root user, so Chromium's sandbox works out of the box.
case " ${CHASER_EXTRA_ARGS:-} " in
  *" --disable-dev-shm-usage "*) ;;
  *) export CHASER_EXTRA_ARGS="${CHASER_EXTRA_ARGS:+$CHASER_EXTRA_ARGS }--disable-dev-shm-usage" ;;
esac

# Returns success when an Xvfb server is currently alive on the given display
# number (without the leading colon). The lock file alone is not proof: `/tmp`
# survives `docker restart` while every process is gone, so a leftover lock
# from a dead Xvfb must not be mistaken for a live display.
xvfb_display_alive() {
  dispnum="$1"
  lockpid=""
  if [ -f "/tmp/.X${dispnum}-lock" ]; then
    lockpid="$(cat "/tmp/.X${dispnum}-lock" 2>/dev/null || true)"
    lockpid="$(printf '%s' "$lockpid" | tr -d ' \t\r\n')"
  fi
  case "$lockpid" in
    ''|*[!0-9]*) ;;
    *)
      if [ -f "/proc/$lockpid/cmdline" ] \
        && tr '\0' ' ' < "/proc/$lockpid/cmdline" 2>/dev/null | grep -qF "Xvfb"; then
        return 0
      fi
      ;;
  esac
  for proc in /proc/[0-9]*; do
    if [ -f "$proc/cmdline" ] \
      && xvfb_cmdline_matches "$proc/cmdline" "$dispnum"; then
      return 0
    fi
  done
  return 1
}

# Returns success when a process command line belongs to Xvfb serving the
# given display number. A trailing space guards against prefix matches
# (`:99` must not match `:990`).
xvfb_cmdline_matches() {
  cmdline_file="$1"
  dispnum="$2"
  cmdline="$(tr '\0' ' ' < "$cmdline_file" 2>/dev/null || true)"
  cmdline="${cmdline} "
  case "$cmdline" in
    *Xvfb*":${dispnum} "*) return 0 ;;
    *) return 1 ;;
  esac
}

# This entrypoint owns the default Xvfb display. chaser-cf defaults to
# CHASER_VIRTUAL_DISPLAY=0 and launches headed Chromium against DISPLAY.
# When explicitly set to 1 chaser-cf starts its own Xvfb later, so skip this
# display to avoid duplicate virtual display servers.
if [ "${CHASER_VIRTUAL_DISPLAY:-0}" != "1" ] && [ -z "${DISPLAY_SKIP_XVFB:-}" ]; then
  DISP_NUM="${DISPLAY_NUM#:}"
  if [ -e "/tmp/.X${DISP_NUM}-lock" ] && ! xvfb_display_alive "$DISP_NUM"; then
    echo "Display $DISPLAY_NUM lock is stale (no live Xvfb), removing it..."
    rm -f "/tmp/.X${DISP_NUM}-lock" "/tmp/.X11-unix/X${DISP_NUM}"
  fi
  if [ ! -e "/tmp/.X${DISP_NUM}-lock" ]; then
    echo "Starting Xvfb on $DISPLAY_NUM..."
    Xvfb "$DISPLAY_NUM" -screen 0 1920x1080x24 -ac +extension GLX +render -noreset &
    XVFB_PID=$!
    # Wait for the X socket before launching the server.
    for _ in $(seq 1 50); do
      if [ -e "/tmp/.X${DISP_NUM}-lock" ]; then
        break
      fi
      sleep 0.1
    done
    sleep 0.4
    if ! xvfb_display_alive "$DISP_NUM"; then
      echo "WARNING: Xvfb on $DISPLAY_NUM does not appear to be running; headed Chromium (chaser-cf) will fail until the display is fixed." >&2
    fi
  else
    echo "Display $DISPLAY_NUM already active, reusing it."
  fi
  unset DISP_NUM
fi

# Server settings are pinned through the persisted JSON document instead of
# CLI flags: `arachnea`'s argument parser only matches option names, so any
# value on the command line fails with `unknown argument: <value>` (verified:
# `--server-port 9090` -> `Error: unknown argument: 9090`). The JSON keys map
# to CLI flags (`server_port` -> `--server-port`, `network_mode` ->
# `--network`, `entrypoint_root` -> `--entrypoint-root`) and the file is the
# same one the administration API rewrites, so both stay in sync.
CONFIG_FILE="$DATA_DIR/config.json"
mkdir -p "$(dirname "$CONFIG_FILE")"
if command -v jq >/dev/null 2>&1; then
  if [ -f "$CONFIG_FILE" ]; then
    TMP_CONFIG="$(mktemp)"
    if [ -n "$ROOT" ]; then
      jq --argjson port "$PORT" --arg network "$NETWORK" --arg root "$ROOT" \
        '. + {server_port: $port, network_mode: $network, entrypoint_root: $root}' \
        "$CONFIG_FILE" > "$TMP_CONFIG" && mv "$TMP_CONFIG" "$CONFIG_FILE"
    else
      jq --argjson port "$PORT" --arg network "$NETWORK" \
        '. + {server_port: $port, network_mode: $network} | del(.entrypoint_root)' \
        "$CONFIG_FILE" > "$TMP_CONFIG" && mv "$TMP_CONFIG" "$CONFIG_FILE"
    fi
  else
    if [ -n "$ROOT" ]; then
      jq -n --argjson port "$PORT" --arg network "$NETWORK" --arg root "$ROOT" \
        '{server_port: $port, network_mode: $network, entrypoint_root: $root}' > "$CONFIG_FILE"
    else
      jq -n --argjson port "$PORT" --arg network "$NETWORK" \
        '{server_port: $port, network_mode: $network}' > "$CONFIG_FILE"
    fi
  fi
  chmod 600 "$CONFIG_FILE"
  echo "Server settings pinned in $CONFIG_FILE (port=$PORT network=$NETWORK root=${ROOT:-/})"
else
  echo "WARNING: jq not available, cannot pin server port/network in $CONFIG_FILE" >&2
fi

# Extra container arguments are forwarded to `arachnea` verbatim; only
# valueless flags survive that parser (`--no-tray`, `--desktop`, `--server`).
# Empty "$@" is guarded so `docker run <image>` never forwards a stray word.
if [ "$#" -gt 0 ]; then
  exec "$APP_BIN" --no-tray "$@"
else
  exec "$APP_BIN" --no-tray
fi

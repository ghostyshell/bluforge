#!/bin/bash
# Window-level smoke test for the Linux desktop app.
#
# Launches the AppImage under an existing X display, waits for the real Tauri
# window, captures it, opens the command palette with the keyboard, and checks
# that the MCP server answers. This exercises the GTK/WebKit window, not just
# the web bundle.
#
# Usage: ui-smoke-linux.sh <appimage|app-binary> <out-dir>
set -euo pipefail

APP=${1:?usage: ui-smoke-linux.sh <appimage> <out-dir>}
OUT=${2:?}
: "${DISPLAY:?DISPLAY must be set (start Xvfb first)}"

for tool in xdotool import identify curl python3; do
    command -v "$tool" >/dev/null 2>&1 || { echo "missing $tool" >&2; exit 1; }
done

mkdir -p "$OUT"
# The MCP server is off by default. Enable it for the check; the config has
# serde defaults for every other field.
mkdir -p "$HOME/.config/keepsake-atelier"
printf '{"mcp_enabled":true,"mcp_port":8848}\n' \
    > "$HOME/.config/keepsake-atelier/config.json"

chmod +x "$APP"
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export LIBGL_ALWAYS_SOFTWARE=1
export GDK_BACKEND=x11

LOG="$OUT/app.log"
case "$APP" in
    *.AppImage) "$APP" --appimage-extract-and-run >"$LOG" 2>&1 & ;;
    *) "$APP" >"$LOG" 2>&1 & ;;
esac
PID=$!

cleanup() {
    kill "$PID" 2>/dev/null || true
    sleep 1
    kill -9 "$PID" 2>/dev/null || true
    pkill -f "keepsake-atelier-app" 2>/dev/null || true
}
trap cleanup EXIT

WID=""
for _ in $(seq 1 90); do
    WID=$(xdotool search --onlyvisible --name "Keepsake Atelier" 2>/dev/null | head -1 || true)
    [ -n "$WID" ] && break
    if ! kill -0 "$PID" 2>/dev/null; then
        echo "the app exited before opening a window" >&2
        tail -50 "$LOG" >&2 || true
        exit 1
    fi
    sleep 1
done
[ -n "$WID" ] || {
    echo "no Keepsake Atelier window after 90s" >&2
    tail -50 "$LOG" >&2 || true
    exit 1
}

xdotool getwindowgeometry --shell "$WID" > "$OUT/geometry.txt"
WIDTH=$(awk -F= '/^WIDTH=/{print $2}' "$OUT/geometry.txt")
HEIGHT=$(awk -F= '/^HEIGHT=/{print $2}' "$OUT/geometry.txt")
echo "window ${WIDTH}x${HEIGHT} (id $WID)"
[ "${WIDTH:-0}" -ge 900 ] || { echo "window too small: ${WIDTH:-0}" >&2; exit 1; }
[ "${HEIGHT:-0}" -ge 600 ] || { echo "window too short: ${HEIGHT:-0}" >&2; exit 1; }

# Let the first paint settle, then capture the screen. The root window avoids
# any reparenting differences between window managers.
sleep 4
import -window root "$OUT/shell.png"
MEAN=$(identify -format '%[fx:mean]' "$OUT/shell.png")
echo "mean brightness: $MEAN"
python3 - "$MEAN" <<'PY'
import sys
mean = float(sys.argv[1])
if mean < 0.02:
    raise SystemExit(f"the window rendered blank (mean {mean})")
PY

# MCP: initialize then list tools. Proves the backend behind the window is live.
mcp() {
    curl -sS -m 15 -X POST "http://127.0.0.1:8848/mcp" \
        -H 'content-type: application/json' -d "$1"
}
mcp '{"jsonrpc":"2.0","id":1,"method":"initialize"}' > "$OUT/mcp-init.json"
mcp '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' > "$OUT/mcp-tools.json"
python3 - "$OUT/mcp-init.json" "$OUT/mcp-tools.json" <<'PY'
import json, sys
init = json.load(open(sys.argv[1]))
tools = json.load(open(sys.argv[2]))
name = init["result"]["serverInfo"]["name"]
count = len(tools["result"]["tools"])
print(f"mcp: {name}, {count} tools")
if count < 10:
    raise SystemExit(f"expected at least 10 MCP tools, got {count}")
PY

# Keyboard: open the command palette, capture it, close it again. A scrim makes
# the frame measurably different from the shell capture.
changed_pixels() {
    compare -metric AE "$OUT/shell.png" "$1" null: 2>&1 | awk '{print $1}'
}

xdotool windowactivate --sync "$WID" 2>/dev/null ||
    xdotool windowfocus --sync "$WID" 2>/dev/null || true
sleep 1
echo "active window: $(xdotool getactivewindow 2>/dev/null || echo none)"
xdotool key --clearmodifiers ctrl+k || echo "xdotool key returned $?"
sleep 2
import -window root "$OUT/palette.png" || echo "palette capture returned $?"
CHANGED=$(changed_pixels "$OUT/palette.png")
echo "changed pixels after ctrl+k: $CHANGED"

if [ "${CHANGED:-0}" -le 2000 ]; then
    # No window manager, or the key was swallowed: click the top-bar trigger.
    eval "$(xdotool getwindowgeometry --shell "$WID")"
    xdotool mousemove $((X + WIDTH * 78 / 100)) $((Y + 29)) click 1
    sleep 2
    import -window root "$OUT/palette.png" || echo "palette capture returned $?"
    CHANGED=$(changed_pixels "$OUT/palette.png")
    echo "changed pixels after the click fallback: $CHANGED"
fi
[ "${CHANGED:-0}" -gt 2000 ] || { echo "the command palette did not open" >&2; exit 1; }
xdotool key --clearmodifiers Escape
sleep 1

kill -0 "$PID" 2>/dev/null || { echo "the app exited during the interaction" >&2; exit 1; }
echo "linux UI smoke passed"

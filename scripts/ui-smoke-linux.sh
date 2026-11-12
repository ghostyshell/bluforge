#!/bin/bash
# Window-level smoke test for the Linux desktop app.
#
# Launches the AppImage under an existing X display, waits for the real Tauri
# window, captures it, interacts with the rail, and checks that the MCP server
# answers with its tool list. This exercises the GTK/WebKit window, not just
# the web bundle.
#
# Usage: ui-smoke-linux.sh <appimage|app-binary> <out-dir>
set -euo pipefail

APP=${1:?usage: ui-smoke-linux.sh <appimage> <out-dir>}
OUT=${2:?}
: "${DISPLAY:?DISPLAY must be set (start Xvfb first)}"

for tool in xdotool import identify compare curl python3; do
    command -v "$tool" >/dev/null 2>&1 || { echo "missing $tool" >&2; exit 1; }
done

mkdir -p "$OUT"
# The MCP server is off by default. Enable it for the check; the config has
# serde defaults for every other field.
mkdir -p "$HOME/.config/keepsake-atelier"
# Keep launch deterministic: no 66 MB KEYDB download and no update check.
printf '{"mcp_enabled":true,"mcp_port":8848,"auto_update_keys":false,"auto_check_updates":false}\n' \
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
for _ in $(seq 1 120); do
    WID=$(xdotool search --onlyvisible --name "Keepsake" 2>/dev/null | head -1 || true)
    if [ -z "$WID" ]; then
        WID=$(xdotool search --onlyvisible --class "keepsake" 2>/dev/null | head -1 || true)
    fi
    [ -n "$WID" ] && break
    if ! kill -0 "$PID" 2>/dev/null; then
        echo "the app exited before opening a window" >&2
        tail -50 "$LOG" >&2 || true
        exit 1
    fi
    sleep 1
done
[ -n "$WID" ] || {
    echo "no Keepsake window after 120s" >&2
    tail -50 "$LOG" >&2 || true
    xdotool search --onlyvisible --name "." 2>/dev/null |
        while read -r w; do printf '%s %s\n' "$w" "$(xdotool getwindowname "$w" 2>/dev/null)"; done >&2
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
sleep 5
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

# Interaction: hover and click a rail section. A pointer-driven page change is
# the most portable proof that the webview accepts input; XTEST key delivery
# depends on focus and a window manager, so the palette stays best-effort.
changed_pixels() {
    compare -metric AE "$OUT/shell.png" "$1" null: 2>&1 | awk '{print $1}'
}

eval "$(xdotool getwindowgeometry --shell "$WID")"
xdotool windowactivate --sync "$WID" 2>/dev/null ||
    xdotool windowfocus --sync "$WID" 2>/dev/null || true
sleep 1

# The rail's second item ("Rip settings") sits near (60, 150) in the 1180x820
# window. Hover first, then click; the first click may only take focus.
# Window-relative moves avoid any frame offset from the window manager.
xdotool mousemove --window "$WID" 60 150
sleep 2
import -window root "$OUT/hover.png" || true
HOVER=$(changed_pixels "$OUT/hover.png")
echo "changed pixels after hovering the rail: $HOVER"

SECTION=0
for _ in 1 2 3; do
    xdotool click 1
    sleep 2
    import -window root "$OUT/section.png" || true
    SECTION=$(changed_pixels "$OUT/section.png")
    echo "changed pixels after clicking the rail: $SECTION"
    [ "${SECTION:-0}" -gt 2000 ] && break
done
[ "${SECTION:-0}" -gt 2000 ] || { echo "the rail click did not change the page" >&2; exit 1; }

# Best effort: the command palette, for the screenshot and the log.
xdotool mousemove --window "$WID" $((WIDTH / 2)) $((HEIGHT / 2))
xdotool click 1
sleep 1
xdotool key --clearmodifiers ctrl+k 2>/dev/null || true
sleep 2
import -window root "$OUT/palette.png" || true
xdotool key --clearmodifiers Escape 2>/dev/null || true
sleep 1

kill -0 "$PID" 2>/dev/null || { echo "the app exited during the interaction" >&2; exit 1; }
echo "linux UI smoke passed"

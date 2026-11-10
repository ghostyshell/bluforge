#!/bin/bash
# Build the current version, upload it to GoFile and file.kiwi, rewrite the
# download links, commit and clean up.
#
# This is the only script that writes release links. Run it by hand for a new
# version, or let scripts/refresh-release-links.sh call it weekly to renew the
# free-tier mirrors.
#
# Environment:
#   GOFILE_TOKEN       token string; overrides .gofile-io-token
#   KEEPSAKE_REPO      repository to operate on (default: this checkout)
#   SKIP_PUSH=1        commit but do not push
#   KEEP_DIST=1        keep dist/ after a fully successful upload
set -euo pipefail

SELF=$(cd "$(dirname "$0")" && pwd)
REPO=${KEEPSAKE_REPO:-$(cd "$SELF/.." && pwd)}
cd "$REPO"

log()  { printf '[%s] %s\n' "$(date '+%F %T')" "$*"; }
die()  { log "FAILED: $*"; exit 1; }
notify() { osascript -e "display notification \"$1\" with title \"Keepsake release\"" >/dev/null 2>&1 || true; }

for tool in python3 curl git shasum tar; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool not found on PATH ($PATH)"
done
command -v npx >/dev/null 2>&1 || die "npx not found on PATH ($PATH)"

V=$(python3 - <<'PY'
import json
print(json.load(open("app/src-tauri/tauri.conf.json"))["version"])
PY
)
[ -n "$V" ] || die "could not read the version from app/src-tauri/tauri.conf.json"

RUN=".release-${V}-$(date +%Y%m%d)"
LINKS="$RUN/links.txt"
DIST="dist"
mkdir -p "$RUN" "$DIST"
touch "$LINKS"

# One run at a time. A slept-through slot must not start a second upload.
LOCK=.release-publish.lock
if ! mkdir "$LOCK" 2>/dev/null; then
  pid=$(cat "$LOCK/pid" 2>/dev/null || true)
  if [ -n "${pid:-}" ] && kill -0 "$pid" 2>/dev/null; then
    log "another publish is already running (pid $pid) - exiting"
    exit 0
  fi
  log "clearing stale lock (pid ${pid:-unknown} is not running)"
  rm -rf "$LOCK"
  mkdir "$LOCK"
fi
printf '%s\n' "$$" > "$LOCK/pid"
trap 'rm -rf "$LOCK"' EXIT

# Old run directories are text and logs only. Keep a month.
find . -maxdepth 1 -name '.release-*' -type d -mtime +30 -exec rm -rf {} + 2>/dev/null || true

TOKEN=${GOFILE_TOKEN:-$(cat "$REPO/.gofile-io-token" 2>/dev/null || true)}
[ -n "$TOKEN" ] || die "no GoFile token: write .gofile-io-token or set GOFILE_TOKEN"

log "publishing Keepsake Atelier $V"

# --- 1. Build -------------------------------------------------------------------
log "building the DMG"
rm -f "$DIST"/*.dmg "$DIST"/*.tar.gz
( cd app && npx tauri build --bundles dmg \
    --config '{"bundle":{"createUpdaterArtifacts":false}}' ) \
  >> "$RUN/build.log" 2>&1 || die "tauri build failed - see $RUN/build.log"

log "building the CLI"
cargo build --release -p keepsake-atelier >> "$RUN/build.log" 2>&1 \
  || die "cargo build failed - see $RUN/build.log"

DMG_SRC=$(ls "$REPO"/target/release/bundle/dmg/*.dmg 2>/dev/null | head -1)
[ -n "$DMG_SRC" ] || die "the build did not produce a DMG"
DMG_NAME="Keepsake-Atelier-${V}-arm64.dmg"
CLI_NAME="keepsake-atelier-${V}-arm64.tar.gz"

cp "$DMG_SRC" "$DIST/$DMG_NAME"
tar -czf "$DIST/$CLI_NAME" -C "$REPO/target/release" keepsake-atelier

FILES=("$DMG_NAME" "$CLI_NAME")
log "built: ${FILES[*]}"

# --- 2. Upload ------------------------------------------------------------------
field() { grep -F "$1 |" "$LINKS" 2>/dev/null | tail -1 | cut -d'|' -f"$2" | sed 's/^ *//;s/ *$//' || true; }
row() {
  grep -v -F "$1 |" "$LINKS" > "$LINKS.tmp" 2>/dev/null || true
  mv "$LINKS.tmp" "$LINKS"
  printf '%s | %s | %s | %s | %s\n' "$1" "$2" "$3" "$4" "$5" >> "$LINKS"
}

human_size() {
  python3 - "$1" <<'PY'
import os, sys
b = os.path.getsize(sys.argv[1])
for unit in ("B", "KiB", "MiB", "GiB"):
    if b < 1024 or unit == "GiB":
        print(f"{b:.1f} {unit}" if unit != "B" else f"{b} B")
        break
    b /= 1024.0
PY
}

put_gofile() {
  local f=$1 url=""
  for attempt in 1 2 3; do
    url=$(curl -sS --max-time 3600 -X POST \
      "https://upload.gofile.io/contents/uploadfile" \
      -H "Authorization: Bearer $TOKEN" \
      -F "file=@$DIST/$f" 2>/dev/null \
      | python3 -c 'import json,sys
try:
    print(json.load(sys.stdin).get("data", {}).get("downloadPage", ""))
except Exception:
    print("")' 2>/dev/null) || url=""
    if [ -n "$url" ]; then printf '%s\n' "$url"; return 0; fi
    log "  gofile attempt $attempt/3 failed for $f; retrying in 30s"
    sleep 30
  done
  return 1
}

put_kiwi() {
  FORCE_COLOR=0 npx -y @file-kiwi/node "$DIST/$1" --title "$1" 2>&1 \
    | grep -Eo 'https://file\.kiwi/[A-Za-z0-9][A-Za-z0-9#_+/=-]*' | tail -1 || true
}

incomplete=0
for f in "${FILES[@]}"; do
  gf=$(field "$f" 2)
  kw=$(field "$f" 3)
  if [ -n "$gf" ] && [ "$gf" != "EMPTY" ] && [ -n "$kw" ] && [ "$kw" != "EMPTY" ]; then
    log "[skip] $f already uploaded today"
    continue
  fi
  size=$(human_size "$DIST/$f")
  sha=$(shasum -a 256 "$DIST/$f" | cut -d' ' -f1)
  log "[start] $f ($size)"
  if [ -z "$gf" ] || [ "$gf" = "EMPTY" ]; then
    gf=$(put_gofile "$f") || gf="EMPTY"
  fi
  if [ -z "$kw" ] || [ "$kw" = "EMPTY" ]; then
    kw=$(put_kiwi "$f")
  fi
  [ -n "$gf" ] && [ "$gf" != "EMPTY" ] || incomplete=1
  [ -n "$kw" ] && [ "$kw" != "EMPTY" ] || incomplete=1
  log "[done] $f | ${gf:-EMPTY} | ${kw:-EMPTY}"
  row "$f" "${gf:-EMPTY}" "${kw:-EMPTY}" "$size" "$sha"
done

# --- 3. Rewrite the links -------------------------------------------------------
python3 -I scripts/patch-release-links.py "$LINKS" \
  || die "could not patch the download links"

# --- 4. Commit and push ---------------------------------------------------------
if git diff --quiet -- README.md docs/downloads.json docs/sitemap.xml; then
  log "download links already current - nothing to commit"
else
  git add README.md docs/downloads.json docs/sitemap.xml
  git commit -q -m "docs: refresh ${V} download links (GoFile + file.kiwi)

Automated weekly refresh: the current version is rebuilt and re-uploaded to
GoFile (primary) and file.kiwi (backup). Free-tier links expire, so the docs
are repointed at the fresh mirrors." || die "git commit failed"
  if [ "${SKIP_PUSH:-0}" = "1" ]; then
    log "SKIP_PUSH=1: links committed locally; not pushed"
  else
    command -v gh >/dev/null 2>&1 && gh auth switch --user ghostyshell >/dev/null 2>&1 || true
    git -c credential.helper='!gh auth git-credential' push -q origin main \
      || die "git push failed - links are committed locally but not published"
    log "pushed refreshed ${V} links"
  fi
fi

# --- 5. Clean up ----------------------------------------------------------------
if [ "$incomplete" -ne 0 ]; then
  log "WARNING: at least one mirror is missing - dist/ kept so a re-run can finish"
  notify "Keepsake ${V} refresh finished with gaps"
else
  if [ "${KEEP_DIST:-0}" = "1" ]; then
    log "KEEP_DIST=1: dist/ kept"
  else
    log "removing local build artifacts"
    rm -rf "$DIST"
    rm -f "$DMG_SRC"
  fi
  notify "Keepsake ${V} download links refreshed"
fi

log "done"

#!/bin/bash
# Bundle the non-system dylibs into a Tauri .app and ad-hoc sign it.
#
# dylibbundler rewrites the linked libraries and their dependencies to
# @executable_path/../Resources/lib. libaacs and libbdplus are dlopen'ed by
# libbluray, not linked, so they are fixed from copies (never the Homebrew
# originals) and copied in. The app then runs on a Mac that has never seen
# Homebrew.
#
# Usage: scripts/bundle-app.sh "/path/to/Keepsake Atelier.app"
set -euo pipefail

APP=${1:?usage: bundle-app.sh "/path/to/Keepsake Atelier.app"}
# Resolve before changing directory: CI passes a path relative to the repo root.
APP=$(cd "$(dirname "$APP")" && pwd)/$(basename "$APP")
BIN="$APP/Contents/MacOS/keepsake-atelier-app"
RES="$APP/Contents/Resources"
LIB="$RES/lib"

[ -f "$BIN" ] || { echo "not an app bundle: $APP" >&2; exit 1; }
command -v dylibbundler >/dev/null 2>&1 || {
  echo "dylibbundler is missing (brew install dylibbundler)" >&2
  exit 1
}

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

rm -rf "$LIB"
mkdir -p "$LIB"

# libaacs and libbdplus are opened by name at runtime. Fix copies so their own
# dependencies also point into Resources/lib, then place them next to the app.
EXTRA=()
for name in libaacs libbdplus; do
  DIR=$(brew --prefix "$name" 2>/dev/null || true)
  if [ -n "$DIR" ] && [ -f "$DIR/lib/$name.0.dylib" ]; then
    cp "$DIR/lib/$name.0.dylib" "$TMP/$name.0.dylib"
    EXTRA+=(-x "$TMP/$name.0.dylib")
  else
    echo "warning: $name not installed; encrypted discs will not decrypt" >&2
  fi
done

cd "$RES"
dylibbundler -b -of -x "$BIN" "${EXTRA[@]}" \
  -d lib -p "@executable_path/../Resources/lib" -ns > "$TMP/bundle.log" 2>&1 \
  || { cat "$TMP/bundle.log" >&2; exit 1; }

for name in libaacs libbdplus; do
  if [ -f "$TMP/$name.0.dylib" ]; then
    cp "$TMP/$name.0.dylib" "$LIB/"
    install_name_tool -id "@executable_path/../Resources/lib/$name.0.dylib" "$LIB/$name.0.dylib"
  fi
done

# A stray Homebrew path means a clean Mac will fail at launch.
bad=""
for f in "$BIN" "$LIB"/*.dylib; do
  refs=$(otool -L "$f" 2>/dev/null | grep -E '/(opt/homebrew|usr/local)/' || true)
  if [ -n "$refs" ]; then
    bad="$bad\n$f: $refs"
  fi
done
if [ -n "$bad" ]; then
  printf 'unbundled library references:%b\n' "$bad" >&2
  exit 1
fi

# Loose dylibs in Resources/lib are not nested code for --deep; sign each one
# or dyld kills the app with "Code Signature Invalid".
for f in "$LIB"/*.dylib; do
  codesign --force --sign - "$f" >/dev/null 2>&1 || {
    echo "cannot sign $f" >&2
    exit 1
  }
done
codesign --force --sign - "$APP" >/dev/null 2>&1 || {
  echo "cannot sign the app" >&2
  exit 1
}
codesign --verify --deep --strict "$APP" 2>/dev/null || {
  echo "ad-hoc code signature failed" >&2
  exit 1
}

printf 'bundled %s libraries into %s\n' "$(ls "$LIB" | wc -l | tr -d ' ')" "$LIB"

#!/bin/bash
# Make the macOS CLI tarball self-contained.
#
# The CLI links libbluray and libgcrypt directly and libaacs/libbdplus are
# dlopen'ed. dylibbundler rewrites the linked libraries into lib/ and we add
# the two dlopen'ed ones, then sign everything. ffmpeg/ffprobe and the menu JAR
# sit next to the binary so the CLI works without Homebrew.
#
# Usage: bundle-cli.sh <binary> <out-dir> <resources-dir>
set -euo pipefail

BIN=${1:?usage: bundle-cli.sh <binary> <out-dir> <resources-dir>}
OUT=${2:?}
RES=${3:?}
PREFIX=${BLURAY_PREFIX:-}

[ -x "$BIN" ] || { echo "not a binary: $BIN" >&2; exit 1; }
[ -n "$PREFIX" ] && [ -f "$PREFIX/lib/libaacs.0.dylib" ] || {
    echo "BLURAY_PREFIX must point at the built disc libraries" >&2
    exit 1
}
for f in ffmpeg ffprobe 00000.jar ffmpeg-LICENSES.txt; do
    [ -f "$RES/$f" ] || { echo "missing resource: $RES/$f" >&2; exit 1; }
done

rm -rf "$OUT"
mkdir -p "$OUT/lib"
OUT=$(cd "$OUT" && pwd)
cp "$BIN" "$OUT/keepsake-atelier"
cp "$PREFIX/lib/libaacs.0.dylib" "$OUT/lib/"
cp "$PREFIX/lib/libbdplus.0.dylib" "$OUT/lib/"
install_name_tool -id "@executable_path/lib/libaacs.0.dylib" "$OUT/lib/libaacs.0.dylib"
install_name_tool -id "@executable_path/lib/libbdplus.0.dylib" "$OUT/lib/libbdplus.0.dylib"
cp "$RES/ffmpeg" "$RES/ffprobe" "$RES/00000.jar" "$RES/ffmpeg-LICENSES.txt" "$OUT/"

LOG=$(mktemp)
trap 'rm -f "$LOG"' EXIT
cd "$OUT"
dylibbundler -b -of -x keepsake-atelier -d lib -p "@executable_path/lib" \
    -x lib/libaacs.0.dylib -x lib/libbdplus.0.dylib > "$LOG" 2>&1 \
    || { cat "$LOG" >&2; exit 1; }

for f in keepsake-atelier lib/*.dylib; do
    codesign --force --sign - "$f" >/dev/null 2>&1 || { echo "cannot sign $f" >&2; exit 1; }
    m=$(otool -l "$f" | awk '/LC_BUILD_VERSION/{f=1} f&&/minos/{print $2; exit}')
    [ "$m" = "12.0" ] || { echo "unexpected minos $m for $f" >&2; exit 1; }
    refs=$(otool -L "$f" | tail -n +2 | grep -E '/(opt/homebrew|usr/local)/' || true)
    if [ -n "$PREFIX" ]; then
        refs="$refs$(otool -L "$f" | tail -n +2 | grep -F "$PREFIX" || true)"
    fi
    if [ -n "$refs" ]; then
        echo "unbundled reference in $f:" >&2
        echo "$refs" >&2
        exit 1
    fi
done
for f in ffmpeg ffprobe; do
    m=$(otool -l "$f" | awk '/LC_BUILD_VERSION/{f=1} f&&/minos/{print $2; exit}')
    [ "$m" = "12.0" ] || { echo "unexpected minos $m for $f" >&2; exit 1; }
done

echo "self-contained CLI in $OUT"
du -sh .

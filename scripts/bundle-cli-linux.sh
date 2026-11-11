#!/usr/bin/env bash
# Make the Linux CLI tarball self-contained.
#
# The CLI links libbluray directly, so the package needs the linked library
# closure, not just the binary. We copy every non-glibc dependency into lib/,
# give the binary an `$ORIGIN/lib` rpath and give each library `$ORIGIN`, then
# add the bundled ffmpeg, ffprobe and menu JAR beside the executable.
#
# Usage: bundle-cli-linux.sh <binary> <out-dir> <resources-dir>
set -euo pipefail

BIN=${1:?usage: bundle-cli-linux.sh <binary> <out-dir> <resources-dir>}
OUT=${2:?}
RES=${3:?}

[ -x "$BIN" ] || { echo "not a binary: $BIN" >&2; exit 1; }
for f in ffmpeg ffprobe 00000.jar ffmpeg-LICENSES.txt; do
    [ -f "$RES/$f" ] || { echo "missing resource: $RES/$f" >&2; exit 1; }
done
command -v patchelf >/dev/null 2>&1 || { echo "patchelf not found" >&2; exit 1; }

rm -rf "$OUT"
mkdir -p "$OUT/lib"
OUT=$(cd "$OUT" && pwd)

cp "$BIN" "$OUT/keepsake-atelier"
cp "$RES/ffmpeg" "$RES/ffprobe" "$RES/00000.jar" "$RES/ffmpeg-LICENSES.txt" "$OUT/"

# Libraries that must come from the host: the loader itself and the C runtime.
# Bundling glibc is unsupported and breaks on other distributions.
is_host_lib() {
    case "$1" in
        linux-vdso*|ld-linux*|libc.so*|libm.so*|libmvec.so*|libpthread.so*|libdl.so*|librt.so*|libutil.so*|libresolv.so*|libanl.so*|libnsl.so*|libcrypt.so*) return 0 ;;
        *) return 1 ;;
    esac
}

copy_closure() {
    local file=$1 soname path
    while IFS=$'\t' read -r soname path; do
        [ -n "$soname" ] || continue
        is_host_lib "$soname" && continue
        [ -f "$OUT/lib/$soname" ] && continue
        [ -f "$path" ] || continue
        cp -L "$path" "$OUT/lib/$soname"
        copy_closure "$path"
    done < <(ldd "$file" 2>/dev/null | awk '/=>/ { print $1 "\t" $3 }')
}

copy_dlopen() {
    local soname=$1 path
    path=$(ldconfig -p | awk -v s="$soname" '$1 == s { print $NF; exit }')
    [ -n "$path" ] || { echo "cannot find $soname" >&2; exit 1; }
    cp -L "$path" "$OUT/lib/$soname"
    copy_closure "$path"
}

copy_closure "$OUT/keepsake-atelier"
# libaacs and libbdplus are dlopen'ed by libbluray, so they never appear in the
# binary's dynamic section.
copy_dlopen libaacs.so.0
copy_dlopen libbdplus.so.0
# ffmpeg is statically linked against its codecs but may still need the
# C++ runtime and system libraries. Copy whatever resolves.
copy_closure "$RES/ffmpeg"

patchelf --set-rpath '$ORIGIN/lib' "$OUT/keepsake-atelier"
for so in "$OUT/lib"/*.so*; do
    [ -e "$so" ] || continue
    patchelf --set-rpath '$ORIGIN' "$so"
done

# Every linked library must resolve inside the package; anything still on the
# host means the tarball is not self-contained.
stray=$(ldd "$OUT/keepsake-atelier" \
    | awk -v lib="$OUT/lib/" '/=>/ && $3 !~ lib { print }' \
    | grep -Ev '(linux-vdso|ld-linux|libc\.so|libm\.so|libmvec\.so|libpthread\.so|libdl\.so|librt\.so|libutil\.so|libresolv\.so|libanl\.so|libnsl\.so|libcrypt\.so)' || true)
if [ -n "$stray" ]; then
    echo "unbundled reference in keepsake-atelier:" >&2
    echo "$stray" >&2
    exit 1
fi
for need in libbluray.so libaacs.so libbdplus.so; do
    ls "$OUT/lib/$need"* >/dev/null 2>&1 || {
        echo "missing bundled $need" >&2
        exit 1
    }
done

echo "self-contained CLI in $OUT"
du -sh "$OUT"

#!/usr/bin/env bash
# Build the macOS disc stack with a macOS 12.0 deployment target.
#
# Homebrew bottles currently target macOS 15, which is why the DMG refused to
# launch on 12 and 13. This builds the same libraries from source with
# MACOSX_DEPLOYMENT_TARGET=12.0:
#
#   libgpg-error, libgcrypt, libudfread, libaacs, libbdplus, libbluray
#
# libbluray is built without freetype, fontconfig and libxml2, which are only
# needed for its BD-J text and metadata paths; the app does not use libbluray's
# BD-J player. The result is a small, known dependency closure.
#
# Usage: scripts/build-disc-libs.sh [--force]
# Env:   KEEPSAKE_DISC_PREFIX  target prefix (default: vendor/disc/prefix)
#        MACOS_MIN             deployment target (default: 12.0)
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
BUILD=${KEEPSAKE_DISC_BUILD:-"$ROOT/vendor/disc"}
PREFIX=${KEEPSAKE_DISC_PREFIX:-"$BUILD/prefix"}
SRC="$BUILD/src"
MACOS_MIN=${MACOS_MIN:-12.0}
JOBS=$(sysctl -n hw.ncpu 2>/dev/null || echo 8)
FORCE=0
[ "${1:-}" = "--force" ] && FORCE=1

if [ "$(uname -s)" != "Darwin" ]; then
    echo "build-disc-libs.sh is macOS-only" >&2
    exit 1
fi

need() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "missing build tool: $1" >&2
        exit 1
    }
}
for t in cc make cmake meson ninja pkg-config curl tar python3; do need "$t"; done

# GNU bison: libaacs's parser uses %code, which Apple's BSD bison rejects.
if [ -x /opt/homebrew/opt/bison/bin/bison ]; then
    export PATH="/opt/homebrew/opt/bison/bin:$PATH"
fi
need bison
if ! bison --version 2>/dev/null | grep -q "GNU Bison"; then
    echo "GNU bison is required (brew install bison)" >&2
    exit 1
fi

export MACOSX_DEPLOYMENT_TARGET="$MACOS_MIN"
export CFLAGS="-O2 -mmacosx-version-min=$MACOS_MIN"
export CXXFLAGS="$CFLAGS"
export LDFLAGS="-mmacosx-version-min=$MACOS_MIN"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export PATH="$PREFIX/bin:$PATH"

mkdir -p "$SRC" "$PREFIX"

fetch() { # url outfile
    local url=$1 out=$2
    if [ -s "$out" ] && [ "$FORCE" = 0 ]; then
        echo "== $out already downloaded"
        return
    fi
    curl -fsSL --retry 3 -o "$out" "$url"
}
extract() { # archive dir
    local archive=$1 dir=$2
    if [ -d "$dir" ] && [ "$FORCE" = 0 ]; then
        return
    fi
    rm -rf "$dir"
    case "$archive" in
        *.tar.bz2) tar -xjf "$archive" -C "$SRC" ;;
        *.tar.xz) tar -xJf "$archive" -C "$SRC" ;;
        *.tar.gz) tar -xzf "$archive" -C "$SRC" ;;
        *) echo "unknown archive: $archive" >&2; exit 1 ;;
    esac
}

GPG_VER=1.50
GCRYPT_VER=1.11.0
UDFREAD_VER=1.2.0
AACS_VER=0.11.1
BDPLUS_VER=0.2.0
BLURAY_VER=1.5.0

fetch "https://gnupg.org/ftp/gcrypt/libgpg-error/libgpg-error-$GPG_VER.tar.bz2" "$SRC/libgpg-error-$GPG_VER.tar.bz2"
fetch "https://gnupg.org/ftp/gcrypt/libgcrypt/libgcrypt-$GCRYPT_VER.tar.bz2" "$SRC/libgcrypt-$GCRYPT_VER.tar.bz2"
fetch "https://code.videolan.org/videolan/libudfread/-/archive/$UDFREAD_VER/libudfread-$UDFREAD_VER.tar.gz" "$SRC/libudfread-$UDFREAD_VER.tar.gz"
fetch "https://downloads.videolan.org/pub/videolan/libaacs/$AACS_VER/libaacs-$AACS_VER.tar.bz2" "$SRC/libaacs-$AACS_VER.tar.bz2"
fetch "https://downloads.videolan.org/pub/videolan/libbdplus/$BDPLUS_VER/libbdplus-$BDPLUS_VER.tar.bz2" "$SRC/libbdplus-$BDPLUS_VER.tar.bz2"
fetch "https://downloads.videolan.org/pub/videolan/libbluray/$BLURAY_VER/libbluray-$BLURAY_VER.tar.xz" "$SRC/libbluray-$BLURAY_VER.tar.xz"

extract "$SRC/libgpg-error-$GPG_VER.tar.bz2" "$SRC/libgpg-error-$GPG_VER"
extract "$SRC/libgcrypt-$GCRYPT_VER.tar.bz2" "$SRC/libgcrypt-$GCRYPT_VER"
extract "$SRC/libudfread-$UDFREAD_VER.tar.gz" "$SRC/libudfread-$UDFREAD_VER"
extract "$SRC/libaacs-$AACS_VER.tar.bz2" "$SRC/libaacs-$AACS_VER"
extract "$SRC/libbdplus-$BDPLUS_VER.tar.bz2" "$SRC/libbdplus-$BDPLUS_VER"
extract "$SRC/libbluray-$BLURAY_VER.tar.xz" "$SRC/libbluray-$BLURAY_VER"

# libgpg-error 1.50 uses environ without a declaration, which Apple clang
# rejects in C99 mode.
python3 - "$SRC/libgpg-error-$GPG_VER/src/spawn-posix.c" <<'PY'
import sys
p = sys.argv[1]
s = open(p).read()
if "extern char **environ;" not in s:
    needle = "#include <unistd.h>"
    i = s.index(needle) + len(needle)
    open(p, "w").write(s[:i] + "\n\nextern char **environ;" + s[i:])
PY

if [ ! -f "$PREFIX/lib/libgpg-error.0.dylib" ] || [ "$FORCE" = 1 ]; then
    echo "== libgpg-error $GPG_VER"
    ( cd "$SRC/libgpg-error-$GPG_VER"
      ./configure --prefix="$PREFIX" --disable-doc --disable-tests
      make -j"$JOBS"
      make install )
fi

if [ ! -f "$PREFIX/lib/libgcrypt.20.dylib" ] || [ "$FORCE" = 1 ]; then
    echo "== libgcrypt $GCRYPT_VER"
    ( cd "$SRC/libgcrypt-$GCRYPT_VER"
      ./configure --prefix="$PREFIX" --disable-doc --disable-tests
      make -j"$JOBS"
      make install )
fi

if [ ! -f "$PREFIX/lib/libudfread.3.dylib" ] || [ "$FORCE" = 1 ]; then
    echo "== libudfread $UDFREAD_VER"
    ( cd "$SRC/libudfread-$UDFREAD_VER"
      rm -rf build
      meson setup build --prefix="$PREFIX" --libdir=lib \
          --default-library=shared --buildtype=release -Denable_docs=false
      meson compile -C build -j "$JOBS"
      meson install -C build )
fi

if [ ! -f "$PREFIX/lib/libaacs.0.dylib" ] || [ "$FORCE" = 1 ]; then
    echo "== libaacs $AACS_VER"
    ( cd "$SRC/libaacs-$AACS_VER"
      ./configure --prefix="$PREFIX"
      make -j"$JOBS"
      make install )
fi

if [ ! -f "$PREFIX/lib/libbdplus.0.dylib" ] || [ "$FORCE" = 1 ]; then
    echo "== libbdplus $BDPLUS_VER"
    ( cd "$SRC/libbdplus-$BDPLUS_VER"
      ./configure --prefix="$PREFIX"
      make -j"$JOBS"
      make install )
fi

if [ ! -f "$PREFIX/lib/libbluray.4.dylib" ] || [ "$FORCE" = 1 ]; then
    echo "== libbluray $BLURAY_VER"
    ( cd "$SRC/libbluray-$BLURAY_VER"
      rm -rf build
      meson setup build --prefix="$PREFIX" --libdir=lib \
          --default-library=shared --buildtype=release \
          -Dfontconfig=disabled -Dfreetype=disabled -Dlibxml2=disabled \
          -Dbdj_jar=disabled -Denable_tools=false -Denable_examples=false \
          -Denable_devtools=false
      meson compile -C build -j "$JOBS"
      meson install -C build )
fi

echo
echo "disc libraries in $PREFIX"
for f in libgpg-error.0.dylib libgcrypt.20.dylib libudfread.3.dylib \
         libaacs.0.dylib libbdplus.0.dylib libbluray.4.dylib; do
    [ -f "$PREFIX/lib/$f" ] || { echo "missing $f" >&2; exit 1; }
    printf '%-24s minos %s\n' "$f" \
        "$(otool -l "$PREFIX/lib/$f" | awk '/LC_BUILD_VERSION/{f=1} f&&/minos/{print $2; exit}')"
done

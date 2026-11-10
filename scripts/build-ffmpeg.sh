#!/usr/bin/env bash
# Build a self-contained ffmpeg for Keepsake Atelier.
#
# Includes:
#   - x264 (latest master) and x265 4.2, statically linked
#   - libdvdread 7.1.1 + libdvdnav 7.0.0, so ffmpeg has the dvdvideo demuxer
#   - VideoToolbox / AudioToolbox
#   - ffmpeg n9.0.2 with the native TrueHD, DTS(-HD), AC-3/E-AC-3 codecs (copy
#     and encode as ffmpeg supports; Atmos travels inside TrueHD/E-AC-3)
#
# Outputs:
#   vendor/ffmpeg/ffmpeg, vendor/ffmpeg/ffprobe   (build tree)
#   app/src-tauri/resources/ffmpeg, ffprobe       (bundled by Tauri)
#   app/src-tauri/resources/ffmpeg-LICENSES.txt
#
# Usage: scripts/build-ffmpeg.sh [--force]
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
BUILD="$ROOT/vendor/ffmpeg"
SRC="$BUILD/src"
PREFIX="$BUILD/prefix"
RES="$ROOT/app/src-tauri/resources"
JOBS=$(sysctl -n hw.ncpu 2>/dev/null || echo 8)
MACOS_MIN=${MACOS_MIN:-12.0}
FORCE=0
[ "${1:-}" = "--force" ] && FORCE=1

export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export PATH="$PREFIX/bin:$PATH"
export MACOSX_DEPLOYMENT_TARGET="$MACOS_MIN"
CFLAGS_EXTRA="-mmacosx-version-min=$MACOS_MIN"

need() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "missing build tool: $1" >&2
        exit 1
    }
}
for t in clang make cmake nasm meson pkg-config git curl tar; do need "$t"; done

mkdir -p "$SRC" "$PREFIX" "$RES"

clone() { # url dir [ref]
    local url=$1 dir=$2 ref=${3:-}
    if [ -d "$dir/.git" ] && [ "$FORCE" = 0 ]; then
        echo "== $dir already cloned"
        return
    fi
    rm -rf "$dir"
    if [ -n "$ref" ]; then
        git clone --depth 1 --branch "$ref" "$url" "$dir"
    else
        git clone --depth 1 "$url" "$dir"
    fi
}

fetch() { # url outfile
    local url=$1 out=$2
    if [ -s "$out" ] && [ "$FORCE" = 0 ]; then
        echo "== $out already downloaded"
        return
    fi
    curl -fL --retry 3 -o "$out" "$url"
}

# ---------------------------------------------------------------- x264
clone https://github.com/mirror/x264.git "$SRC/x264"
if [ ! -f "$PREFIX/lib/libx264.a" ] || [ "$FORCE" = 1 ]; then
    echo "== building x264"
    cd "$SRC/x264"
    ./configure --prefix="$PREFIX" \
        --enable-static --enable-pic --disable-cli --disable-opencl \
        --extra-cflags="$CFLAGS_EXTRA" --extra-ldflags="$CFLAGS_EXTRA"
    make -j"$JOBS"
    make install
fi

# ---------------------------------------------------------------- x265
clone https://bitbucket.org/multicoreware/x265_git.git "$SRC/x265" 4.2
if [ ! -f "$PREFIX/lib/libx265.a" ] || [ "$FORCE" = 1 ]; then
    echo "== building x265"
    cd "$SRC/x265"
    rm -rf build
    cmake -S source -B build -G Ninja \
        -DCMAKE_INSTALL_PREFIX="$PREFIX" \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOS_MIN" \
        -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
        -DENABLE_SHARED=OFF -DENABLE_CLI=OFF -DENABLE_ASSEMBLY=ON
    cmake --build build -j "$JOBS"
    cmake --install build
    # Some x265 installs omit the pkg-config file; ffmpeg needs it.
    if [ ! -f "$PREFIX/lib/pkgconfig/x265.pc" ]; then
        cat > "$PREFIX/lib/pkgconfig/x265.pc" <<EOF
prefix=$PREFIX
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include

Name: x265
Description: H.265/HEVC video encoder
Version: 4.2
Libs: -L\${libdir} -lx265
Libs.private: -lc++ -lm
Cflags: -I\${includedir}
EOF
    fi
fi

# ------------------------------------------------------- libdvdread/dvdnav
DVDREAD_VER=7.1.1
DVDNAV_VER=7.0.0
fetch "https://downloads.videolan.org/pub/videolan/libdvdread/$DVDREAD_VER/libdvdread-$DVDREAD_VER.tar.xz" \
    "$SRC/libdvdread-$DVDREAD_VER.tar.xz"
fetch "https://downloads.videolan.org/pub/videolan/libdvdnav/$DVDNAV_VER/libdvdnav-$DVDNAV_VER.tar.xz" \
    "$SRC/libdvdnav-$DVDNAV_VER.tar.xz"

meson_build() { # dir [extra options...]
    local dir=$1
    shift
    meson setup "$dir/_build" "$dir" --prefix="$PREFIX" \
        --default-library=static --buildtype=release \
        -Denable_docs=false "$@"
    meson compile -C "$dir/_build" -j "$JOBS"
    meson install -C "$dir/_build"
}

if [ ! -f "$PREFIX/lib/libdvdread.a" ] || [ "$FORCE" = 1 ]; then
    echo "== building libdvdread"
    rm -rf "$SRC/libdvdread-$DVDREAD_VER"
    tar -xJf "$SRC/libdvdread-$DVDREAD_VER.tar.xz" -C "$SRC"
    meson_build "$SRC/libdvdread-$DVDREAD_VER" -Dlibdvdcss=disabled
fi
if [ ! -f "$PREFIX/lib/libdvdnav.a" ] || [ "$FORCE" = 1 ]; then
    echo "== building libdvdnav"
    rm -rf "$SRC/libdvdnav-$DVDNAV_VER"
    tar -xJf "$SRC/libdvdnav-$DVDNAV_VER.tar.xz" -C "$SRC"
    meson_build "$SRC/libdvdnav-$DVDNAV_VER" -Denable_examples=false
fi

# ---------------------------------------------------------------- ffmpeg
clone https://github.com/FFmpeg/FFmpeg.git "$SRC/ffmpeg" n9.0.2
if [ ! -x "$PREFIX/bin/ffmpeg" ] || [ "$FORCE" = 1 ]; then
    echo "== building ffmpeg"
    cd "$SRC/ffmpeg"
    make distclean >/dev/null 2>&1 || true
    ./configure \
        --prefix="$PREFIX" \
        --pkg-config-flags=--static \
        --extra-cflags="-I$PREFIX/include $CFLAGS_EXTRA" \
        --extra-ldflags="-L$PREFIX/lib $CFLAGS_EXTRA" \
        --enable-gpl \
        --enable-libx264 \
        --enable-libx265 \
        --enable-libdvdread \
        --enable-libdvdnav \
        --enable-videotoolbox \
        --enable-audiotoolbox \
        --disable-vulkan --disable-xlib --disable-libxcb --disable-libxcb-shm \
        --disable-libxcb-xfixes --disable-libxcb-shape --disable-libdrm \
        --disable-vaapi --disable-vdpau --disable-sdl2 --disable-opencl \
        --disable-doc --disable-debug --disable-ffplay \
        --disable-shared --enable-static
    make -j"$JOBS"
    make install
fi

# ---------------------------------------------------------------- bundle
STAMP=$(date +%Y-%m-%d)
{
    echo "Built $STAMP for Keepsake Atelier"
    echo "ffmpeg:    n9.0.2 ($(git -C "$SRC/ffmpeg" rev-parse --short HEAD))"
    echo "x264:      master ($(git -C "$SRC/x264" rev-parse --short HEAD))"
    echo "x265:      4.2 ($(git -C "$SRC/x265" rev-parse --short HEAD))"
    echo "libdvdread:$DVDREAD_VER"
    echo "libdvdnav: $DVDNAV_VER"
    echo "configure: GPL, libx264, libx265, libdvdread, libdvdnav, videotoolbox, audiotoolbox"
} > "$BUILD/VERSIONS.txt"

cp -f "$PREFIX/bin/ffmpeg" "$RES/ffmpeg"
cp -f "$PREFIX/bin/ffprobe" "$RES/ffprobe"
chmod +x "$RES/ffmpeg" "$RES/ffprobe"
codesign --force --sign - "$RES/ffmpeg" >/dev/null 2>&1 || true
codesign --force --sign - "$RES/ffprobe" >/dev/null 2>&1 || true

{
    echo "Keepsake Atelier bundled ffmpeg"
    echo "================================="
    echo
    cat "$BUILD/VERSIONS.txt"
    echo
    echo "ffmpeg is GPLv2+ (--enable-gpl). x264 is GPLv2, x265 is GPLv2, and"
    echo "libdvdread/libdvdnav are GPLv2+. The sources are the projects listed"
    echo "above; keep the source offer with the app when distributing it."
    echo
    for f in "$SRC/ffmpeg/COPYING.GPLv2" "$SRC/x264/COPYING" "$SRC/x265/COPYING" \
             "$SRC/libdvdread-$DVDREAD_VER/COPYING" "$SRC/libdvdnav-$DVDNAV_VER/COPYING"; do
        [ -f "$f" ] && { echo "---- $f"; cat "$f"; echo; }
    done
} > "$RES/ffmpeg-LICENSES.txt"

echo
echo "done:"
"$RES/ffmpeg" -hide_banner -version | head -2
"$RES/ffmpeg" -hide_banner -demuxers 2>/dev/null | grep -w dvdvideo || echo "WARNING: dvdvideo missing"
"$RES/ffmpeg" -hide_banner -encoders 2>/dev/null | grep -E "libx264|libx265" | head -4
echo "bundled at $RES/ffmpeg and $RES/ffprobe"

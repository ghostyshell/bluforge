# The bundled ffmpeg

Keepsake Atelier ships its own `ffmpeg` and `ffprobe` so a rip does not depend on
whatever Homebrew has installed. The system ffmpeg 9.0.2 on this machine has no
`dvdvideo` demuxer, which is why DVD was blocked; the bundled build has it.

## What is in it

| Component | Version | Why |
| --- | --- | --- |
| ffmpeg | n9.0.2 | remux, encode, muxers/demuxers |
| x264 | master | H.264 encoding (`libx264`) |
| x265 | 4.2 | HEVC encoding (`libx265`) |
| libdvdread | 7.1.1 | `dvdvideo` demuxer, DVD `VIDEO_TS` |
| libdvdnav | 7.0.0 | DVD navigation for `dvdvideo` |
| VideoToolbox / AudioToolbox | system | GPU H.264/HEVC, hardware audio |

Configure flags:

```
--enable-gpl --enable-libx264 --enable-libx265
--enable-libdvdread --enable-libdvdnav
--enable-videotoolbox --enable-audiotoolbox
--disable-vulkan --disable-xlib --disable-libxcb --disable-libxcb-shm
--disable-libxcb-xfixes --disable-libxcb-shape --disable-libdrm
--disable-vaapi --disable-vdpau --disable-sdl2 --disable-opencl
--disable-doc --disable-debug --disable-ffplay
--disable-shared --enable-static
```

TrueHD, TrueHD+Atmos, DTS, DTS-HD MA, AC-3 and E-AC-3 are **native** ffmpeg
codecs; the build needs no extra library for them. ffmpeg does not *encode*
TrueHD/DTS/Atmos, so the rip path copies those tracks (`-c copy`), which is what
preserves Atmos and DTS-HD. AC-3/E-AC-3 can be encoded natively when a
conversion is requested.

## Rebuilding

```sh
brew install nasm cmake meson pkg-config     # once
scripts/build-ffmpeg.sh                      # ~15-30 min; --force to rebuild
```

The script clones the pinned refs, builds static x264/x265 and
libdvdread/libdvdnav into `vendor/ffmpeg/prefix`, builds ffmpeg, then copies:

```
app/src-tauri/resources/ffmpeg
app/src-tauri/resources/ffprobe
app/src-tauri/resources/ffmpeg-LICENSES.txt
```

`vendor/` and the two binaries are gitignored (they are build outputs). Run the
script before `npm run tauri build` so Tauri can bundle them. `tauri.conf.json`
maps them to `Contents/Resources/ffmpeg` and `.../ffprobe` in the app.

## How the app finds them

`core::binaries` resolves, in order:

1. `KEEPSAKE_FFMPEG` / `KEEPSAKE_FFPROBE`;
2. next to the running executable;
3. `../Resources/` inside a macOS `.app`;
4. `app/src-tauri/resources/` (dev tree) and `vendor/ffmpeg/` (dev tree);
5. `~/Library/Application Support/keepsake-atelier/bin/`;
6. `PATH`.

`keepsake-atelier doctor` prints the resolved path and whether the build has
`dvdvideo`, `libx264`, `libx265` and VideoToolbox.

## Licensing

The build is **GPLv2 or later** (`--enable-gpl`, x264 and x265 are GPLv2). The
app itself stays MIT and runs ffmpeg as a separate process, but when you
distribute a bundle containing this binary you must comply with the GPL for it:
ship the license texts (generated into `ffmpeg-LICENSES.txt`), the exact
versions/commits (in the same file), and make the corresponding source
available, or point to it:

- ffmpeg: https://github.com/FFmpeg/FFmpeg (tag `n9.0.2`)
- x264: https://github.com/mirror/x264
- x265: https://bitbucket.org/multicoreware/x265_git (tag `4.2`)
- libdvdread: https://code.videolan.org/videolan/libdvdread (7.1.1)
- libdvdnav: https://code.videolan.org/videolan/libdvdnav (7.0.0)

The build script and the license file are the source of truth for what was
shipped.

### Signing a release

The script ad-hoc signs the two helpers so local builds run. For a notarized
release, sign them with your Developer ID and the hardened runtime before
building the app, and keep them inside the app's signing scope:

```sh
codesign --force --options runtime --timestamp \
  --sign "Developer ID Application: ..." app/src-tauri/resources/ffmpeg
codesign --force --options runtime --timestamp \
  --sign "Developer ID Application: ..." app/src-tauri/resources/ffprobe
npm run tauri build
```

Otherwise Gatekeeper can reject the nested executables even when the `.app`
itself is notarized.

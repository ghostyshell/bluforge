# Platforms

Keepsake Atelier is macOS-first. The engine and CLI are portable Rust, but the
optical drive, native AACS and disc-burning layers talk to operating-system
interfaces, so each platform is at a different stage.

| Capability | macOS arm64 | macOS x64 | Linux x64 | Windows x64 |
| --- | --- | --- | --- | --- |
| Desktop app | yes | yes | not yet | not yet |
| CLI | yes | yes | yes | preview |
| Optical drive discovery | IOKit | IOKit | sysfs (`/dev/sr*`) | not yet |
| Native AACS key recovery | yes | yes | no (use KEYDB) | no (use KEYDB) |
| Rip and encode | yes | yes | yes, with a system ffmpeg | yes, with ffmpeg on `PATH` |
| Author to a BDMV folder | yes | yes | yes, with tsMuxeR | yes, with tsMuxeR |
| Author a UDF 2.50 ISO | yes (`newfs_udf`) | yes | no | no |
| Authored BD-J menu | yes | yes | yes (writer) | yes (writer) |
| DVD-Video | yes (bundled ffmpeg) | yes (bundled ffmpeg) | needs an ffmpeg with `dvdvideo` | needs an ffmpeg with `dvdvideo` |
| Verify, menu documents, KEYDB | yes | yes | yes | yes |
| Burn to disc | `hdiutil` | `hdiutil` | `growisofs` | no |
| MCP server | yes | yes | yes | preview |

## macOS

The reference platform. The app is a signed-by-you DMG, the bundled ffmpeg has
`dvdvideo`, libx264 and libx265, and the native AACS path uses IOKit and a
LibreDrive vendor read.

- **Apple Silicon:** the normal download.
- **Intel:** built in CI on an Intel runner (`x86_64-apple-darwin`). The same
  source, a separate DMG.

## Linux

The CLI builds against the distribution's libbluray, libaacs and libbdplus.

```sh
sudo apt install pkg-config libbluray-dev libaacs-dev libbdplus-dev ffmpeg
cargo build --release -p keepsake-atelier
./target/release/keepsake-atelier doctor
```

What works today, verified on Debian 12 (arm64) against an authored test disc:

- `disc`, `verify`, `menu parse` and `rip` from a mounted volume, BDMV folder
  or ISO.
- Drive discovery from `/sys/block/sr*`, mount matching from
  `/proc/self/mounts`, and `CDROMEJECT` for eject.
- `keys` import/search, libaacs decryption with a KEYDB entry.
- Authoring to a BDMV folder with tsMuxeR.
- Burning with growisofs when `dvd+rw-tools` is installed.

What does not work yet:

- Native AACS key recovery. Use a KEYDB entry or libaacs.
- UDF 2.50 ISO authoring. `newfs_udf` and `hdiutil` are macOS tools; write a
  BDMV folder instead and let your authoring tool make the image.
- The desktop app. Tauri builds on Linux, but the UI and device cards have not
  been validated there.

## Windows

The CLI is a preview. It is built with the GNU toolchain and MSYS2 libraries.

```powershell
# in an MSYS2 MINGW64 shell
pacman -S mingw-w64-x86_64-libbluray mingw-w64-x86_64-libaacs mingw-w64-x86_64-libbdplus mingw-w64-x86_64-pkg-config
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu -p keepsake-atelier
```

The release zip bundles the MSYS2 runtime DLLs the binary needs. Unpack it and
run `keepsake-atelier.exe doctor`.

What works: reading an ISO, BDMV folder or authored disc image; `disc`,
`verify`, `menu`, `rip` and `author` to a folder, provided ffmpeg and tsMuxeR
are on `PATH`.

What does not work yet:

- Drive discovery and eject. There is no WMI/DeviceIoControl backend.
- Native AACS and burning.
- UDF ISO authoring.
- The desktop app.

## Building everything

`.github/workflows/build.yml` is the cross-platform matrix:

- `macos-14` builds the Apple Silicon DMG and CLI.
- `macos-15-intel` builds the Intel DMG and CLI.
- `ubuntu-22.04` builds the Linux CLI.
- `windows-latest` builds the Windows CLI zip.

Run it from the Actions tab or `gh workflow run build.yml`. The local
`scripts/publish-release.sh` and the weekly launchd job still build the macOS
arm64 artifacts; use the workflow when you need the rest.

## Adding a platform

The shape of the work is the same for each:

1. `device.rs`: implement `list()` (drive discovery) and `eject()`.
2. `disc.rs`: add the library search paths and the raw-device naming.
3. `native.rs`: implement or explicitly disable native key recovery.
4. `udf.rs`: use a native UDF 2.50 writer, or keep folder-only authoring.
5. `burn.rs`: add the writer front-end.
6. `doctor.rs`: make the messages and capability matrix truthful.
7. Add a CI runner and publish the artifact.

Do not ship a platform that only compiles. The Linux build was checked with
`disc`, `verify`, `menu parse` and a real rip before it was added here.

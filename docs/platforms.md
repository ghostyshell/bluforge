# Platforms

Keepsake Atelier is macOS-first. The engine and CLI are portable Rust, but the
optical drive, native AACS and disc-burning layers talk to operating-system
interfaces, so each platform is at a different stage.

| Capability | macOS arm64 | macOS x64 | Linux x64 | Windows x64 |
| --- | --- | --- | --- | --- |
| Desktop app | yes | yes | preview (AppImage/deb) | preview (installer, CI smoke) |
| CLI | yes | yes | yes | yes (CI smoke) |
| Optical drive discovery | IOKit | IOKit | sysfs (`/dev/sr*`) | WMI (`Win32_CDROMDrive`) |
| Native AACS key recovery | yes | yes | no (use KEYDB) | no (KEYDB via bundled libaacs) |
| Rip and encode | yes | yes | yes, with a system ffmpeg | yes, with ffmpeg on `PATH` |
| Author to a BDMV folder | yes | yes | yes, with tsMuxeR | yes, with tsMuxeR |
| Author a UDF 2.50 ISO | yes (`newfs_udf`) | yes | yes (portable, BD-R) | yes (portable, BD-R) |
| Authored BD-J menu | yes | yes | yes (writer) | yes (writer) |
| DVD-Video | yes (bundled ffmpeg) | yes (bundled ffmpeg) | needs an ffmpeg with `dvdvideo` | yes (bundled ffmpeg) |
| Verify, menu documents, KEYDB | yes | yes | yes | yes |
| Burn to disc | `hdiutil` | `hdiutil` | `growisofs` | IMAPI2 (untested) |
| MCP server | yes | yes | yes | preview |

## macOS

The reference platform. The DMG bundles libbluray, libaacs, libbdplus and their
dependencies (fixed and re-signed with `scripts/bundle-app.sh`), the bundled
ffmpeg has `dvdvideo`, libx264 and libx265, and the native AACS path uses IOKit
and a LibreDrive vendor read. The CLI tarballs are not bundled and need the
Homebrew libraries.

- **Apple Silicon:** the normal download.
- **Intel:** built in CI on an Intel runner (`x86_64-apple-darwin`). The same
  source, a separate DMG. The published DMG was checked: x86_64 executable,
  bundled ffmpeg/ffprobe, the menu JAR and the app resources are present.
- Both CI DMGs were mounted and checked for a clean library closure (no
  `/opt/homebrew` or `/usr/local` references), and the arm64 build was launched
  against a real drive.

## Linux

The CLI builds against the distribution's libbluray, libaacs and libbdplus.

```sh
sudo apt install pkg-config libbluray-dev libaacs-dev libbdplus-dev ffmpeg
cargo build --release -p keepsake-atelier
./target/release/keepsake-atelier doctor
```

What works today, verified on Debian 12 (arm64) against an authored test disc
and with the x86_64 CI artifact under an Ubuntu 22.04 container:

- `disc`, `verify`, `menu parse` and `rip` from a mounted volume, BDMV folder
  or ISO.
- Drive discovery from `/sys/block/sr*`, mount matching from
  `/proc/self/mounts`, and `CDROMEJECT` for eject.
- `keys` import/search, libaacs decryption with a KEYDB entry.
- Authoring to a BDMV folder with tsMuxeR.
- Burning with growisofs when `dvd+rw-tools` is installed.
- A desktop app as an **AppImage** (bundles GTK/WebKit and the app) and a
  **.deb** (depends on `libwebkit2gtk-4.1-0`, `libgtk-3-0`, `libbluray2`,
  `libaacs0`, `libbdplus0` and `ffmpeg`). The build, packaged resources and
  dependency set are verified; the window has not been driven interactively on
  a Linux desktop here.

What does not work yet:

- Native AACS key recovery. Use a KEYDB entry or libaacs.
- BD-RE authoring. The portable writer builds a mastered UDF 2.50 image, which
  is the BD-R profile: it has no metadata partition. BD-RE still wants the
  macOS `newfs_udf` path.
- A full interactive pass on a real Linux desktop. The AppImage and deb build
  and package correctly, and the AppImage launches under Xvfb here without
  crashing; the headless WebKit surface did not paint for a screenshot, so
  pointer and keyboard behaviour still needs a monitor.

## Windows

The CLI is a preview. It is built with the GNU toolchain and MSYS2 libraries.

```powershell
# in an MSYS2 MINGW64 shell
pacman -S mingw-w64-x86_64-libbluray mingw-w64-x86_64-libaacs mingw-w64-x86_64-libbdplus mingw-w64-x86_64-pkg-config
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu -p keepsake-atelier
```

The release zip bundles the MSYS2 runtime DLLs the binary needs, including
libbluray and its dependencies. Unpack it and run
`keepsake-atelier.exe doctor`.

What works: reading an ISO, BDMV folder or authored disc image; `disc`,
`verify`, `menu`, `rip` and `author` to a folder, provided ffmpeg and tsMuxeR
are on `PATH`; and `burn` through IMAPI2.

There is also an **NSIS installer** built in CI with MSVC, statically linked
libbluray from vcpkg, and Windows ffmpeg/ffprobe in the bundle. It launches the
same desktop UI and can read an ISO or BDMV folder you point it at. It does not
bundle libaacs/libbdplus, so encrypted discs need the CLI zip (which does) or a
system install.

The app discovers drives through WMI (`Win32_CDROMDrive`), ejects through
`Shell.Application`, and understands drive letters as sources. It can open an
ISO or a BDMV folder. The installer bundles the MinGW libaacs and libbdplus
DLLs in `resources/windows-libs` and points `LIBAACS_PATH` and `LIBBDPLUS_PATH`
there, so a KEYDB entry can decrypt a disc. The CLI zip carries the same
closure next to the exe.

Burning uses Windows' own **IMAPI2** COM stack through PowerShell, so no extra
tool is needed. It picks the first compatible recorder, or the drive letter
passed as `--device D:`. The write path is implemented and unit-tested at the
script level, but has not been run on a physical writer here.

CI executes both artifacts on `windows-latest`: the zip runs `--version` and
`doctor` with its DLL closure, and the installer is silently installed, located,
and launched. The app has to stay alive for 15 seconds. That covers install and
startup, not a click-through of the UI.

Multi-title ISOs are written with the portable UDF 2.50 writer (pure Rust,
streams file data). The image is the BD-R profile; BD-RE media still wants the
macOS `newfs_udf` path.

What does not work yet:

- Native AACS key recovery.
- BD-RE authoring.
- A full interactive pass through the installer and UI on a Windows desktop.

## Building everything

`.github/workflows/build.yml` is the cross-platform matrix:

- `macos-15` builds the Apple Silicon DMG and CLI.
- `macos-15-intel` builds the Intel DMG and CLI.
- `ubuntu-22.04` builds the Linux CLI with an older glibc floor.
- `ubuntu-24.04` builds the Linux AppImage and deb.
- `windows-latest` builds the Windows CLI zip and the NSIS installer.

Run it from the Actions tab or `gh workflow run build.yml`. The local
`scripts/publish-release.sh` and the weekly launchd job still build the macOS
arm64 artifacts; use the workflow when you need the rest.

## Adding a platform

The shape of the work is the same for each:

1. `device.rs`: implement `list()` (drive discovery) and `eject()`.
2. `disc.rs`: add the library search paths and the raw-device naming.
3. `native.rs`: implement or explicitly disable native key recovery.
4. `udf.rs` / `udf_portable.rs`: native `newfs_udf`, or the portable writer.
5. `burn.rs`: add the writer front-end.
6. `doctor.rs`: make the messages and capability matrix truthful.
7. Add a CI runner and publish the artifact.

Do not ship a platform that only compiles. The Linux build was checked with
`disc`, `verify`, `menu parse` and a real rip before it was added here.

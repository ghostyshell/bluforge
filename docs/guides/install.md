# Install

## Requirements

- macOS 12 or newer on Apple Silicon or Intel, or an x86_64 Linux or Windows
  machine. The per-platform matrix is in [Platforms](../platforms.md).
- The macOS DMG is self-contained; the Linux and Windows builds install their
  own runtime pieces as described below.
- A USB Blu-ray writer or reader for disc work. DVD drives work for DVD titles.
- Free disk space: a lossless rip is roughly the size of the feature, and an
  authored ISO needs that much again.

## Download

Grab the latest DMG from one of the mirrors. The README and the
[project site](https://ghostyshell.github.io/keepsake-atelier/#downloads) always
carry the current links:

- **GoFile** is the primary mirror.
- **file.kiwi** is the encrypted backup mirror.

Links are refreshed weekly because GoFile expires free-tier files.

## Install

1. Open the DMG.
2. Drag **Keepsake Atelier** to **Applications**.
3. Eject the DMG.

The app is not notarized. On first launch macOS will refuse it. Do one of:

- Right-click the app, choose **Open**, then **Open** again in the dialog.
- Or clear the quarantine flag:

```sh
xattr -dr com.apple.quarantine "/Applications/Keepsake Atelier.app"
```

## CLI only

The downloads table also carries a CLI tarball. It contains the same engine
without the desktop shell:

```sh
tar -xzf keepsake-atelier-0.1.0-macos-arm64.tar.gz
./keepsake-atelier doctor
```

Every CLI package is self-contained. Unpack it and keep `keepsake-atelier` next
to `lib/`, `ffmpeg`, `ffprobe` and `00000.jar`; it finds all of them beside the
binary. No Homebrew libraries or system ffmpeg are needed. To put the command
on your `PATH`, add the unpacked directory:

```sh
export PATH="$PWD:$PATH"
```

## Linux

- **AppImage:** `chmod +x Keepsake-Atelier-0.1.0-linux-x64.AppImage` and run it.
  It carries WebKitGTK, GTK3 and ffmpeg.
- **deb:** `sudo apt install ./Keepsake-Atelier-0.1.0-linux-x64.deb`. It pulls
  `libwebkit2gtk-4.1-0`, `libgtk-3-0`, `libbluray2`, `libaacs0`, `libbdplus0`,
  `libgcrypt20` and `libgpg-error0`; ffmpeg is bundled.
- **CLI tarball:** unpack and run `./keepsake-atelier doctor`. The tarball
  carries libbluray, libaacs, libbdplus, libgcrypt, ffmpeg, ffprobe and the menu
  JAR; no distribution packages are needed.

Drive discovery uses sysfs and eject uses `CDROMEJECT`. Encrypted discs use the
native AACS path (`SG_IO` with a LibreDrive vendor read, then a KEYDB host
certificate) with libaacs as the fallback.

## Windows

- **Installer:** run the NSIS setup. SmartScreen may ask for **More info** then
  **Run anyway** because the build is not signed. The app carries ffmpeg and
  the libaacs/libbdplus DLL closure.
- **CLI zip:** unpack anywhere and run `keepsake-atelier.exe doctor`. libbluray
  is linked into the executable, and the zip carries libaacs, libbdplus, their
  dependency closure, ffmpeg, ffprobe and the menu JAR. tsMuxeR must be on
  `PATH` for authored menus.

Drives are found through WMI and ejected through `Shell.Application`. Native
AACS uses `IOCTL_SCSI_PASS_THROUGH_DIRECT`, and burning uses IMAPI2.

## Runtime libraries

The DMG is self-contained: libbluray, libaacs, libbdplus and their dependencies
are bundled in `Contents/Resources/lib`. The macOS CLI tarball keeps the same
libraries in `lib/`, and the Linux tarball and Windows zip carry their own
closure. None needs Homebrew.

`keepsake-atelier doctor` reports the exact library it is missing.

## First launch

The sidebar shows the app version, the data directory and the MCP state. Open
**System check** and run `doctor`:

```sh
keepsake-atelier doctor
```

You want to see:

- `ffmpeg` with `dvdvideo`, `libx264`, `libx265` and `videotoolbox`.
- `optical drive` naming your writer.
- `burning` reporting `hdiutil burn (native)`.
- `Authoring` reporting tsMuxeR if you plan to author.

## Authoring helper: tsMuxeR

Authoring uses tsMuxeR. Put a macOS build at:

```
~/Library/Application Support/keepsake-atelier/bin/tsMuxeR
```

and make it executable:

```sh
chmod +x "$HOME/Library/Application Support/keepsake-atelier/bin/tsMuxeR"
```

Alternatively install it on `PATH` as `tsMuxeR`, or point at it explicitly:

```sh
export KEEPSAKE_TSMUXER=/path/to/tsMuxeR
```

## Data directory

Everything the app keeps lives in:

```
~/Library/Application Support/keepsake-atelier
```

- `config.json` - output templates, defaults, MCP setting.
- `KEYDB.cfg` - AACS keys, if you use one.
- `menus/` - saved menu templates.
- `bin/` - the managed tsMuxeR.

The directory is private to your user. Nothing is uploaded. To start over, quit
the app and move the directory aside; it is recreated on next launch.

## Uninstall

Drag the app to the Trash. Remove the data directory only if you also want to
lose your keys and menu templates.

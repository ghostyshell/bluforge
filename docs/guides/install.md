# Install

## Requirements

- An Apple Silicon or Intel Mac. Linux and Windows CLI builds are covered in
  [Platforms](../platforms.md).
- macOS 12 or newer.
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
tar -xzf keepsake-atelier-0.1.0-arm64.tar.gz
sudo install -m 755 keepsake-atelier /usr/local/bin/
keepsake-atelier doctor
```

The tarball does not bundle libbluray or ffmpeg. Install the Homebrew libraries
(`libbluray libaacs libbdplus`) and point the CLI at a suitable ffmpeg build.
The app bundle carries one:

```sh
export KEEPSAKE_FFMPEG="/Applications/Keepsake Atelier.app/Contents/Resources/ffmpeg"
```

## Runtime libraries

The DMG is self-contained: libbluray, libaacs, libbdplus and their dependencies
are bundled in `Contents/Resources/lib`. The CLI tarballs are not bundled; they
need the Homebrew libraries:

```sh
brew install libbluray libaacs libbdplus
```

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

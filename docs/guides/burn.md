# Burn and verify

Burning writes an ISO to a Blu-ray writer. It erases the disc and cannot be
undone, so every path has a dry run and a confirmation.

## 1. Check the writer and media

```sh
keepsake-atelier devices
keepsake-atelier doctor
```

`devices` lists optical drives with BSD names and capacity. `doctor` should say
`hdiutil burn (native)`.

## 2. Ask what the image needs

```sh
hdiutil burn /path/movie.iso -sizequery
```

Or let the app show the image size next to the media type. A BD-25 image needs a
BD-25 or larger disc; a BD-50 needs a BD-50.

## 3. Dry run

```sh
keepsake-atelier burn /path/movie.iso
```

This prints the exact command and changes nothing. The default is
`hdiutil burn`, which picks the writer itself.

To use growisofs with an explicit device:

```sh
keepsake-atelier burn /path/movie.iso --device /dev/rdisk4
```

## 4. Write

```sh
keepsake-atelier burn /path/movie.iso --yes
```

In the app, the Burn button asks for confirmation. Over MCP, `burn_start`
requires `confirm=true`. There is no silent write path.

Options:

- `--speed N` sets the write speed.
- `--device /dev/rdiskN` switches to growisofs.
- hdiutil verifies by default after writing; keep that on for the first burn.

## 5. Read it back

Eject, reinsert, and verify the disc:

```sh
keepsake-atelier verify /Volumes/MY_DISC
```

For an authored disc, `verify` checks the navigation, playlists, clips and, if
present, the BDJO, JAR and menu rows.

## Media notes

- Use BD-RE for tests. A failed BD-R is a coaster.
- A BD-ROM (pressed movie) cannot be written. The drive will refuse it.
- Clean the disc if hdiutil fails at the same point twice.
- If the writer is mounted, unmount it first. The app checks this for you.

## If the write fails

- **No optical drive detected**: connect the writer, or pass `--device`.
- **Device is mounted**: eject the volume in Finder and retry.
- **hdiutil error -123**: media not writable or not blank enough.
- **growisofs cannot open the device**: use the raw node (`/dev/rdiskN`), not the
  block node, and make sure nothing else holds it.

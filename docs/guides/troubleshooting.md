# Troubleshooting

## The app will not open

macOS blocks unsigned apps. Right-click, **Open**, then confirm. Or:

```sh
xattr -dr com.apple.quarantine "/Applications/Keepsake Atelier.app"
```

## No drive detected

```sh
keepsake-atelier devices
keepsake-atelier doctor
```

- Reconnect the USB cable and try another port.
- Make sure the drive has power and the disc is seated.
- `doctor` should list an optical drive. If it does not, macOS is not exposing
  it; check `system_profiler SPCUSBDataType`.

## AACS or decryption errors

- Run `keepsake-atelier plan` and read the backend line.
- `keys status` shows whether a KEYDB entry exists.
- `keys native` needs a LibreDrive vendor read. A stock firmware failure is
  expected on some drives.
- libaacs with a valid KEYDB entry is enough for ripping even when native key
  recovery fails.

## tsMuxeR is missing

Authoring needs it at:

```
~/Library/Application Support/keepsake-atelier/bin/tsMuxeR
```

Make it executable, or set `KEEPSAKE_TSMUXER`. `doctor` reports what it found.

## Authoring fails or the ISO is empty

- Confirm the inputs are real video files with a video stream tsMuxeR can carry.
- Check free space: a UDF image is sparse until written, but the final size must
  fit.
- If the destination is an ISO, make sure no other process has it mounted.
- Run `verify` on the output; it names the missing playlist or clip.

## The menu does not appear on a player

Menus are opt-in and hardware playback is untested. Verify the tree first:

```sh
keepsake-atelier verify /Volumes/MY_DISC
```

Then read [Menus](menus.md) for what was verified and what was not.

## Burning fails

- A pressed BD-ROM cannot be written.
- Unmount the volume before writing.
- Use a blank BD-RE for tests.
- `hdiutil` error -123 means the media is not writable or is too small.

## DVD rip fails

- Try another title; some discs put copy protection on specific angles.
- Clean the disc.
- CSS-protected discs are not supported; the error names the failing step.

## Where the logs are

The app writes job output to the data directory. Long rips and authoring runs
keep their last lines there. Use **Open logs** in the app, or:

```sh
open "$HOME/Library/Application Support/keepsake-atelier"
```

## Reset

Quit the app, move the data directory aside, relaunch. This loses saved keys and
menu templates; copy them out first if you need them.

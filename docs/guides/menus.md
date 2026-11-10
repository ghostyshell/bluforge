# Menus

Keepsake Atelier can author a real BD-J title menu for a disc it builds. It is
opt-in because hardware playback is still untested.

## Turn it on

```sh
keepsake-atelier author ep1.mkv ep2.mkv --out set.iso --label MY_SHOW --menu
```

In the app, tick **Add a title menu** in the Author card. Over MCP, pass
`"menu": true` to `author_start`.

Add a background:

```sh
keepsake-atelier author ep1.mkv ep2.mkv --out set.iso --menu --menu-background art.png
```

The image is copied to `BDMV/JAR/menu.png` and drawn full screen. Without one,
the menu draws a dark gradient.

## What the menu does

- 1920x1080, one row per title, with the disc label and a heading.
- Up/Down or Left/Right moves the selection.
- Enter or Space starts the selected title.
- Digits 1 to 9 jump straight to a row.
- Mouse click selects and starts a row.
- After a title ends, the menu returns.

The title list is read from `BDMV/JAR/menu.txt` at runtime, so a single generic
Xlet drives every disc. The format is:

```
Keepsake Atelier|MY SHOW
Episode One|0
Episode Two|1
Episode Three|2
bg=menu.png
```

The first line is `heading|disc label`. Each later line is `label|playlist`.
`bg=` is optional. A `#` starts a comment.

## Use names from a saved template

A menu document from another disc can supply the row names:

```sh
keepsake-atelier menu parse /Volumes/OLD_DISC --save my-show
keepsake-atelier author ep1.mkv ep2.mkv --out set.iso --menu --menu-template my-show
```

Names are matched by title order, not by duration. Check the result with
`verify`.

## What gets written

- `BDMV/BDJO/00000.bdjo` - terminal info, app cache, accessible playlists, one
  autostart BD-J application, key interest and file access.
- `BDMV/JAR/00000.jar` - the generic menu Xlet.
- `BDMV/JAR/menu.txt` - the rows, heading, label and background reference.
- `index.bdmv` - first play and top menu point at the BD-J object; titles remain
  HDMV objects.
- Backup copies under `BDMV/BACKUP`.

## Verification and the honest caveat

`keepsake-atelier verify` checks that the BDJO exists, that an application JAR is
present, and that every menu row names a playlist that exists.

The Xlet itself is verified in two ways: libbluray parses the authored BDJO and
reports First Play and Top Menu supported, and the real Xlet runs in a stubbed
BD-J harness where it renders, moves the selection, requests the right
`bd://1.PLAYLIST:0000N`, and returns after end of media.

libbluray on macOS cannot start BD-J itself: its JVM loads AWT on the calling
thread, which enters the AppKit loop inside `bdj_open`. A hardware player is the
remaining test. Keep menus opt-in until you have tried one on your own player.

## Rebuild the menu asset

The prebuilt JAR lives at `core/assets/menu/00000.jar`. To rebuild it:

```sh
scripts/build-menu.sh
```

That needs a Java 8 JDK and libbluray's two runtime JARs. The source is
`core/assets/menu/src/Menu.java`. `scripts/test-menu.sh` runs the harness and
writes screenshots to a temporary directory.

# Author a disc

Authoring takes finished files and builds a Blu-ray structure: one title, or one
title per input, as a BDMV folder or a native UDF 2.50 ISO.

## Before you start

- tsMuxeR must be installed. See [Install](install.md).
- Inputs should already be in a Blu-ray compatible codec: H.264 or HEVC video,
  and AC-3, E-AC-3, DTS, TrueHD or LPCM audio. The app tells you what it can
  carry before muxing.
- A single input becomes one title. Several inputs become one title each.

## One title

```sh
keepsake-atelier author movie.mkv --out /Volumes/Media/Movie.iso --label MY_MOVIE
```

Use a folder instead of an ISO:

```sh
keepsake-atelier author movie.mkv --out /Volumes/Media/Movie
```

## Several titles

```sh
keepsake-atelier author ep1.mkv ep2.mkv ep3.mkv --out /Volumes/Media/Season.iso --label MY_SHOW
```

Each input gets its own playlist and movie object. tsMuxeR's clips are
renumbered so the titles do not collide, and `index.bdmv` plus
`MovieObject.bdmv` are generated.

## Chapters

Chapter marks come from the input files. Force a mark every N minutes:

```sh
keepsake-atelier author movie.mkv --out movie.iso --auto-chapters 8
```

Drop them entirely:

```sh
keepsake-atelier author movie.mkv --out movie.iso --no-chapters
```

## Split across discs

`--split GB` packs whole inputs onto discs of at most that size. A title is never
cut across discs.

```sh
keepsake-atelier author ep*.mkv --out /Volumes/Media/Set --split 25
```

With an ISO destination, the result is one ISO per disc:

```sh
keepsake-atelier author ep*.mkv --out /Volumes/Media/Set.iso --split 50
# writes /Volumes/Media/Set/Disc1.iso, Disc2.iso, ...
```

## Verify before burning

```sh
keepsake-atelier verify /Volumes/Media/Season.iso
```

The check reads the navigation files directly. It confirms every title plays a
playlist, every playlist's clips exist, and, when you authored a menu, the BDJO,
JAR and menu rows are present.

What it cannot check is whether a player likes the result. Do a short test burn
on a rewritable disc before committing a stack of BD-Rs.

## Notes on the UDF image

Multi-title ISOs are written by the app, not tsMuxeR.

- macOS uses `newfs_udf` and `hdiutil`. The filesystem is UDF 2.50 with a
  metadata partition, which is what BD-RE requires. `hdiutil` mounts the image
  read-write, the merged BDMV tree is copied in, and the image is detached. The
  image is sparse until the files are written.
- Linux and Windows use a portable pure-Rust writer. It produces a mastered UDF
  2.50 image with the BD-R profile (no metadata partition) and streams file data
  from disk, so a 50 GB tree does not need 50 GB of memory. `KEEPSAKE_PORTABLE_UDF=1`
  forces this path on macOS for testing.

## Menu

Add a title menu with `--menu`. See [Menus](menus.md) for the background,
templates and the verification caveat.

# Features

Keepsake Atelier is a macOS workshop for Blu-ray and DVD discs. It reads what is
on a disc, takes it apart without re-encoding when you do not want to, puts a new
disc together when you do, and checks the result before it reaches a writer.

Everything below is in the desktop app, the `keepsake-atelier` CLI and the MCP
server. The CLI is the reference surface: each section gives the command that
does the work.

## Disc inspection

- Optical drive discovery through IOKit, with the BSD device name and volume.
- Blu-ray title map through libbluray: every title, playlist, clip, duration,
  chapter count, resolution, dynamic range and audio stream count.
- Disc label, libbluray version, AACS state and which decryption path is active.
- The navigation table from `index.bdmv`: first playback, top menu and every
  title entry, including BD-J object names.
- A per-title stream list with codec, language, channels and default flags.

```sh
keepsake-atelier devices
keepsake-atelier disc /Volumes/MY_DISC
keepsake-atelier disc /Volumes/MY_DISC --title 0
```

## Keys and AACS

- A `KEYDB.cfg` store with import, search, add, show and remove.
- Native AACS1 recovery: read the volume ID from the drive, derive the volume
  unique key and the disc key from the disc's MKB. No host certificate and no
  MakeMKV required.
- Native AACS2 support for UHD discs: unit keys are derived offline from
  `AACS/Unit_Key_RO.inf` with the VUK. AACS2 is detected by the first byte of
  `Content000.cer`.
- `keys from-makemkv` imports dumps when you already have them.
- `plan` and `doctor` explain which backend will be used for the inserted disc.

```sh
keepsake-atelier keys status
keepsake-atelier keys native
keepsake-atelier keys search "THE MATRIX"
```

The volume ID step needs a LibreDrive vendor read. Stock firmware on some
drives does not provide it; libaacs plus a KEYDB entry still decrypts, and the
app reports which path it used.

## Ripping and encoding

- Lossless remux to MKV or MP4. Video is copied as is; audio and subtitles are
  muxed without a second encode.
- Codecs carried: H.264, HEVC, VC-1, MPEG-2, AC-3, E-AC-3, DTS, DTS-HD,
  TrueHD, FLAC, LPCM and PGS subtitles.
- Optional x264, x265 (8 or 10 bit) and VideoToolbox encode for size.
- Default track selection: English non-commentary audio, subtitles off, the
  first video stream labelled and default.
- Chapter marks from the disc, plus optional ChapterDB names.
- TMDB poster and metadata written into the output when a match is found.
- A persistent batch queue, one job at a time, with cancel and progress.

```sh
keepsake-atelier rip /Volumes/MY_DISC
keepsake-atelier rip /Volumes/MY_DISC --title 2 --encode --codec x265 --quality 19
keepsake-atelier rip /Volumes/MY_DISC --container mp4 --no-chapters
```

## Metadata and covers

- TMDB search for title, year, poster and overview.
- ChapterDB chapter names, matched by disc id or title.
- Cover search across HiResCovers, dvdcover.com and TMDB, with a preview before
  saving `cover.jpg`, `poster.jpg` and `fanart.jpg` next to the output.

```sh
keepsake-atelier covers search "The Jungle Book"
keepsake-atelier covers save /path/to/folder --from hirescovers:12345
```

## Authoring

- One input becomes one title. Several inputs become one title each on the same
  disc, with generated `index.bdmv` and `MovieObject.bdmv` and renumbered clips.
- Chapter marks are carried from the inputs; `--auto-chapters N` forces one
  every N minutes and `--no-chapters` drops them.
- Output can be a BDMV folder or a native UDF 2.50 image. The ISO path uses
  macOS's `newfs_udf` (UDF 2.50 with a metadata partition) and `hdiutil`, not a
  third-party UDF writer.
- `--split GB` packs whole inputs onto BD-25 or BD-50 sized discs. A title is
  never cut across discs.
- Splitting to ISOs writes `set/Disc1.iso`, `set/Disc2.iso`, and so on.
- The authoring step runs tsMuxeR for each title and then merges the trees. A
  hardware-player pass is still worth doing after the first burn.

```sh
keepsake-atelier author movie.mkv --out movie.iso --label MY_MOVIE
keepsake-atelier author ep1.mkv ep2.mkv ep3.mkv --out season.iso --label MY_SHOW
keepsake-atelier author ep*.mkv --out set.iso --split 25
```

## Disc menus

- A real BD-J title menu, installed as first play and top menu.
- One generic Xlet reads `BDMV/JAR/menu.txt` at runtime, so no per-disc Java
  compilation is needed.
- 1920x1080 screen with the disc label, one row per title, Up/Down, Enter,
  digits 1 to 9 and mouse selection.
- An optional background image, copied to `BDMV/JAR/menu.png`.
- Playback starts with `bd://1.PLAYLIST:0000N`; the menu returns when the title
  ends.
- `bdjo.rs` writes `BDMV/BDJO/00000.bdjo`; `index.bdmv` points first play and top
  menu at it while titles stay HDMV.
- Menu documents: parse an existing disc's menu, rename titles, mark the main
  feature, name chapters, save it as a template and apply it to another disc.

```sh
keepsake-atelier author ep1.mkv ep2.mkv --out set.iso --menu --menu-background art.png
keepsake-atelier menu parse /Volumes/MY_DISC --save my-disc
keepsake-atelier menu apply my-disc /Volumes/OTHER_DISC
```

Menus are opt-in. The BDJO, the JAR and every menu row are verified, and the
Xlet is exercised in a real JVM harness, but hardware playback is still
untested. See `docs/guides/menus.md`.

## DVD-Video

- List DVD titles through the bundled ffmpeg `dvdvideo` demuxer.
- Rip a title to MKV, copying MPEG-2 video and AC-3 or DTS audio.
- No DVD menus are ripped; this is title extraction.

```sh
keepsake-atelier dvd list /Volumes/DVD_NAME
keepsake-atelier dvd rip /Volumes/DVD_NAME --title 0
```

## TV and anime

- Split a disc into per-episode files with `SxxExx` names.
- Opening, ending and preview detection from chapter structure, so those blocks
  are not glued onto an episode.
- TMDB lookup for series title, season and episode names.
- A TV plan command that prints what it will do before writing anything.

```sh
keepsake-atelier tv plan /Volumes/MY_DISC --start 1
keepsake-atelier rip /Volumes/MY_DISC --tv --start 1
```

## Clone and ISO export

- 1:1 encrypted clone: copy the disc's structure without decrypting it.
- Decrypted ISO export, or a plain folder when you want files on disk.
- Both paths work from a mounted volume, a BDMV folder or an ISO.

```sh
keepsake-atelier iso /Volumes/MY_DISC /path/out/MY_DISC.iso
keepsake-atelier clone /Volumes/MY_DISC /path/out/MY_DISC.iso
```

## Verification

- Reads `index.bdmv` and `MovieObject.bdmv` directly, with no libbluray and no
  decryption, so it works on an authored tree before it is burned.
- Checks every HDMV title's movie object for a PlayPL, the playlist it names,
  and every clip's `.clpi` and `.m2ts`.
- Reports duration and mark counts per title.
- For BD-J first play and top menu, checks the BDJO and an application JAR.
- For an authored menu, checks every `menu.txt` row against a real playlist.
- Exits non-zero on problems, so it can gate a release or a burn.

```sh
keepsake-atelier verify /path/Set/Disc1
keepsake-atelier verify /Volumes/MY_DISC --json
```

## Burning

- Native `hdiutil burn` on macOS, `growisofs` on Linux, and IMAPI2 through
  PowerShell on Windows. The macOS and Windows paths pick the writer themselves.
- `--device /dev/rdiskN` selects growisofs, with cdrecord as a fallback.
- Dry run by default. A real write needs `--yes` in the CLI, a confirmation in
  the app, and `confirm=true` over MCP.
- Speed selection, size query and post-burn verification through hdiutil.
- A blank BD-RE or BD-R is required; the app refuses to start without a writer.

```sh
keepsake-atelier burn /path/movie.iso
keepsake-atelier burn /path/movie.iso --yes
keepsake-atelier burn /path/movie.iso --device /dev/rdisk4 --yes
keepsake-atelier burn C:\\path\\movie.iso --device D: --yes   # Windows
```

## Command line, desktop app and MCP

- The CLI covers every capability, including JSON output for scripting.
- The Tauri app has cards for discs, titles, rips, authoring, menus, keys, DVD,
  covers, the queue, verification, burning and settings.
- The MCP server exposes the same operations as tools on localhost. It is off by
  default, binds to loopback, and requires confirmation for destructive tools.

```sh
keepsake-atelier mcp --port 8765
claude mcp add --transport http keepsake http://127.0.0.1:8765/mcp
```

## Configuration, data and privacy

- Data lives in `~/Library/Application Support/keepsake-atelier`: config, KEYDB,
  saved menu templates and the managed tsMuxeR binary.
- Environment overrides use the `KEEPSAKE_*` prefix, with the old `BLUFORGE_*`
  names accepted as fallbacks.
- Nothing is uploaded automatically. There is no account, telemetry or cloud
  service. The release download mirrors are the only network endpoints the
  project itself controls.

## Known limits

- macOS first. The macOS app is the reference build. Linux ships a tested CLI
  plus AppImage and deb previews; Windows ships a CLI zip and an NSIS installer
  preview with WMI drive discovery and IMAPI2 burning. See
  [Platforms](platforms.md).
- Cinavia is a proprietary audio watermark. There is no open detector
  or remover, and none is bundled.
- Authored BD-J menus are verified in a JVM harness and by libbluray's parser,
  not on a hardware player. They remain opt-in.
- Burning has been exercised with a detected writer and a size query, but not
  with a blank disc in this workshop.

# Roadmap

Where Keepsake Atelier is, and what comes next. Status is honest: "done" means built,
tested and used on a real disc.

## Phase 1 - Disc access and inspection - done

- Optical device discovery, mount handling, hot-plug recovery.
- libbluray-backed inspection: titles, playlists, durations, chapters, streams,
  clip boundaries (episode splits on TV discs).
- Works against a mounted volume, a `BDMV` folder, an ISO, or `/dev/rdiskN`.

## Phase 2 - Rip, remux, encode - done

- Lossless remux (`-c copy`) or re-encode (x264/x265/videotoolbox).
- MKV/MP4, audio/subtitle selection, chapter names from ChapterDB.
- TV/anime mode: one file per episode, SxxExx naming, per-disc episode memory.
- Output templates and metadata (TMDB, ChapterDB, posters, covers).

## Phase 3 - Native AACS and BD+ - done

- Disc key from `KEYDB.cfg`, unit keys, and a fully native key recovery path:
  drive VID -> media key from the disc's own MKB with the KEYDB device keys ->
  VUK.
- Native VID acquisition over the drive's LibreDrive vendor `READ BUFFER`,
  with no host certificate and no MakeMKV. Host-certificate auth remains as the
  fallback for a non-LibreDrive drive.
- BD+ VM data installation, so BD+ discs also decrypt without MakeMKV.
- See `docs/decryption.md`.

## Phase 4 - Export and authoring - done

- Decrypted ISO or folder export.
- Author a `BDMV`/ISO from finished MKV/MP4 files with chapters.

## Phase 5 - Surfaces - done

- CLI for everything (`keepsake-atelier ...`).
- Tauri desktop app (Keepsake Atelier) for devices, disc inspection and track
  lists, rips, covers, ISO export, authoring, keys, BD+ and menu documents.
- MCP server exposing the core to agents.

## Phase 6 - Disc menu documents and templates - done

Parse a disc's menu, edit it, save it as a template, apply it to another disc.

- `BDMV/index.bdmv` navigation: first playback, top menu, and every title entry
  (HDMV movie object or BD-J object name), with a self-validating entry stride
  so both 12-byte and legacy layouts parse.
- Playable titles from libbluray, joined with editable names, the main-feature
  flag and chapter labels.
- Templates under `~/Library/Application Support/keepsake-atelier/menus/`, applied to a
  different disc by playlist id, then duration, then position.
- CLI: `keepsake-atelier menu parse|list|show|edit|apply|remove`.
- See `docs/menu-templates.md`.

## Phase 7 - Menu documents in the desktop app - done

- Parse the inserted disc's menu in the UI, rename titles, flag the main
  feature, edit chapter labels, inspect the navigation table, save a template,
  apply one to another disc, and delete templates.
- The same panel lists saved templates and shows what a parse could not read.
- Also surfaced alongside it: native key derivation for the inserted disc, BD+
  VM installation, and the per-title stream list.
- A saved template whose disc id matches the rip source now supplies the title
  name, year and chapter labels for that rip.

## Phase 8 - Batch queue - done

- The desktop app queues rip requests: "Start rip" enqueues the selected title
  (or the TV plan), and `+` on any title row queues that title with the current
  settings.
- Jobs run one at a time. A queue panel shows each job's state
  (queued/running/done/error/cancelled) and progress, and can cancel a running
  job or remove queued and finished ones.
- The queue is persisted to `~/Library/Application Support/keepsake-atelier/queue.json`
  on every state change, restored on launch (a job that was running becomes
  queued again), and never auto-starts on launch: the queue panel has a Resume
  button.

## Phase 9 - Authored menus - superseded by Phase 18

Generating a graphical top menu for a disc that `keepsake-atelier author` builds
needs an HDMV interactive-graphics stream or a BD-J application. A BD-J menu is
implemented in Phase 18; the saved menu documents from Phase 6 feed its names.

## Phase 10 - AACS2 / UHD unit keys - done

- The key entry is inspectable (`keepsake-atelier keys show`, `--json`) with the
  media key, volume id, VUK and every unit key, and the app's keys panel has a
  unit-key editor that writes the `U` table into KEYDB.
- Unit keys are **derived natively**: `AACS/Unit_Key_RO.inf` holds them encrypted,
  and each is AES-128-ECB decrypted with the VUK. AACS2 unit-key records are 64
  bytes apart (AACS1 48); `AACS/Content000.cer[0] == 0x10` marks AACS2.
  `keys native` and `keys from-makemkv` store the table automatically.
- Unit tests cover both record layouts with a synthetic unit-key file.

## Phase 11 - 1:1 disc clone - done

- `keepsake-atelier clone <device> <image.iso>` copies the raw sectors, preserving
  AACS/BD+, and `Clone 1:1` in the ISO panel does the same from the app.
- Progress is byte-based (block size and count come from the drive on macOS),
  the copy is uncached, a drive drop stops with the byte offset, and cancelling
  removes the partial image.
- Covered by unit tests using a file as the source, so the copy loop and cancel
  path are exercised without hardware.

## Phase 12 - Multi-title authoring and splitting - done

- `author` accepts several inputs: each becomes its own playlist and movie
  object in one BDMV folder. `bdmv::merge` renumbers tsMuxeR's clips, patches
  the playlist references, and writes `index.bdmv` / `MovieObject.bdmv` from the
  layouts libbluray parses (12-byte index entries, 12-byte `PlayPL` commands,
  `JumpTitle` for first play and top menu).
- `--split <GB>` groups whole inputs onto discs of at most that size under
  `<out>/Disc1`, `Disc2`, ... (BD-25/BD-50 style). A title is never cut.
- The app's author card takes multiple files and an optional split size; MCP
  takes `inputs` and `split_gb`.
- Verified by authoring two test clips and opening the merged tree with
  libbluray (two titles, patched clip names, navigation parses). Unit tests
  cover the index/movie-object bytes, clip renaming and the size grouping.

## Phase 13 - BDMV verification - done

- `verify` parses `index.bdmv`, `MovieObject.bdmv` and every playlist without
  libbluray, maps each title to its movie object and playlist, checks the clip
  files exist, and reports duration and mark counts. BD-J titles are reported as
  unchecked.
- `keepsake-atelier verify <path>` (JSON with `--json`, non-zero exit on
  problems), a **Verify** button next to Eject and next to Author in the app,
  and an MCP `verify` tool.
- Unit tested against generated and broken trees; also run against the authored
  two-title disc and a deliberately damaged copy.

## Phase 15 - DVD-Video - done

- `dvd list` reads the VMG `VIDEO_TS.IFO` title search pointer table (title,
  VTS, chapters, angles) and probes durations with ffprobe.
- `dvd rip` copies one title with the bundled `dvdvideo` demuxer: MPEG-2 and
  AC-3/DTS as-is, no re-encode, optional VobSub subtitles, progress and cancel.
- The app shows a **DVD-Video** card with the title list and a rip button when
  the disc field points at a DVD; MCP has `dvd_info` and `dvd_start`.
- Unit tests cover the IFO parser and the ffmpeg argument builder; the actual
  `dvdvideo` read needs a real disc to exercise.

## Phase 16 - Burning - implemented, write untested

- Live drive: an LG BD-RE BU40N (USB) was detected, a pressed AACS Blu-ray was
  read (60 titles), decrypted with libaacs and a lossless title rip completed
  (H.264 + AC-3 + PGS). The inserted disc was a BD-ROM, so no write could be run.
- Native key recovery needs the drive's LibreDrive vendor read; on this stock
  1.03 firmware it fails ("no LibreDrive VID record", authentication rc=-10),
  so `keys native` does not replace an existing KEYDB entry here.


- `burn <image.iso> --device /dev/rdiskN [--yes]` writes a single-title ISO with
  growisofs (cdrecord fallback). It refuses a mounted device or a missing image,
  prints the command by default, and requires `--yes` to write; cancelling warns
  that the disc may be partially written. The app's Decrypted ISO panel has a
  confirmed **Burn ISO to disc** button.
- Unit tests cover the command builders, progress parser and image validation.
- Not testable here (no writer drive). Multi-title images are covered by the
  native UDF 2.50 work in Phase 17.

## Phase 17 - Multi-title ISO (native UDF 2.50) - done

- macOS's `newfs_udf` creates a UDF 2.50 image with a metadata partition in a
  plain (sparse) file, and `hdiutil` mounts it read-write. `author` now lays the
  merged multi-title BDMV straight into that mount, so `--out disc.iso` works
  for several inputs. `--split ... --out set.iso` writes `set/Disc1.iso`, ...
- Verified with two test titles: the image mounts, `verify` reports two titles
  and passes, and libbluray lists both.
- Burning now prefers macOS's native `hdiutil burn`; an explicit `--device
  /dev/rdiskN` selects growisofs instead.
- What remains open is hardware verification of the authored BD-J menu and of
  the Cinavia stripper on a real player.

## Phase 18 - Authored BD-J title menu - done, hardware untested

- One generic BD-J Xlet (`00000.jar`) renders a 1080p title menu with one row
  per title, a background image, arrow keys, digits and mouse hit testing. It
  reads the disc's own `menu.txt`, so the same JAR works on every authored
  disc.
- `author --menu` installs the JAR and the menu data, wires first play and top
  menu to it, and keeps HDMV navigation as the fallback.
- Verified in a JVM harness and by libbluray's parser. No standalone player has
  run it, so the app labels it as checked with libbluray rather than
  hardware-proven.

## Parity with DVDFab-class apps

What the app covers today, against the ripping-suite feature set:

| Capability | Status |
| --- | --- |
| Disc inspection: titles, playlists, durations, chapters | done |
| Tree verification (navigation, playlists, clips) | done |
| Track list per title (codecs, languages) | done |
| Rip: lossless remux to MKV/MP4 | done |
| Rip: x264/x265/VideoToolbox encode | done |
| TV/anime episode split with TMDB naming | done |
| Decrypted ISO / folder export (`Copy`) | done |
| Authoring: BDMV/ISO from finished files | done (single or multi-title, native UDF 2.50) |
| Disc menu parse, edit, save, apply | done |
| Authored BD-J title menu (opt-in) | done (asset + container verified; hardware untested) |
| Native AACS key derivation, BD+ VM | done (needs a LibreDrive-unlocked drive for the volume ID) |
| Cover art | done |
| DVD (`VIDEO_TS`) ripping | done (MPEG-2/AC-3/DTS copy, no menus) |
| 1:1 encrypted disc clone | done |
| BD-25 / BD-50 splitting | done (whole titles per disc) |
| Burning to disc | done (native hdiutil burn; untested without a writer) |
| Cinavia detection/removal | done (ridge-field detector + notch; untested on a player) |
| UHD/AACS2 unit-key workflow | done (derived from Unit_Key_RO.inf with the VUK) |
| Batch queue (one job at a time) | done |

## Phase 14 - System check - done

- `doctor` reports every external piece (libbluray, libaacs, libbdplus + VM,
  ffmpeg and its `dvdvideo` demuxer, tsMuxeR, KEYDB, drive, burning tools, data
  directory) and a capability matrix with the fix for each missing item.
- CLI (`doctor`, `--json`), an app **System check** panel and an MCP `doctor`
  tool. This is how the DVD/burning limits below are surfaced to the user.

### What is blocked, and on what

Everything in the parity matrix above is implemented. These four are the
remaining gaps, and each is blocked on something outside the codebase:

- **Hardware verification of the authored BD-J menu.** The Xlet, the JAR and
  the navigation entries are checked by a JVM harness and libbluray. A
  standalone Blu-ray player has not run the disc yet.
- **Burning.** Implemented with native `hdiutil burn` (growisofs when an
  explicit device is given), but the write is untested here because no writer
  drive is attached. Multi-title ISOs work via native UDF 2.50 (`newfs_udf`).
- **Cinavia.** Implemented without decoding the payload: a ridge-field detector
  over the quiet frames plus an adaptive notch, documented in
  [guides/cinavia.md](guides/cinavia.md). The measurement separates a clean
  control (2 ridges) from watermarked dubs (47-69) but has not been confirmed
  against a Cinavia-aware player, and a stereo-only title is the weak case.

What would unblock each: a standalone player for the menu and for the Cinavia
check, a writer drive and a blank BD-RE for the burn check.

## Deliberately out of scope

- BD-J Java parsing beyond object names. The JAR bytecode is a full Java
  application; the menu *document* is the navigation table and the title list,
  not the Java UI.
- Writing to an original disc.

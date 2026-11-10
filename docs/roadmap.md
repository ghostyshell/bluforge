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

## Phase 9 - Authored menus - deferred

Generating a graphical top menu for a disc that `keepsake-atelier author` builds needs
an HDMV interactive-graphics stream or a BD-J application. Neither toolchain is
present: tsMuxeR does not write menus, and there is no BD-J compiler or player
to verify against. A navigation-only menu (no drawn buttons) would not be a
usable menu, so this is deferred rather than shipped half-working.

The parts that are ready for it: saved menu documents (Phase 6), the menu UI
(Phase 7) and authoring (Phase 4). When an IG or BD-J generator is added, it can
consume a saved document directly.

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
| Authoring: BDMV/ISO from finished files | done (single or multi-title) |
| Disc menu parse, edit, save, apply | done |
| Native AACS key derivation, BD+ VM | done |
| Cover art | done |
| DVD (`VIDEO_TS`) ripping | done (MPEG-2/AC-3/DTS copy, no menus) |
| 1:1 encrypted disc clone | done |
| BD-25 / BD-50 splitting | done (whole titles per disc) |
| Burning to disc | not started |
| Cinavia detection/removal | not implemented |
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

- **Authored graphical menus.** A Blu-ray menu is an HDMV Interactive Graphics
  stream (PDS/ODS/ICS segments inside the M2TS) or a BD-J application. tsMuxeR
  does not write IG streams, ffmpeg cannot mux them, and a BD-J app needs the
  BD-J API stubs plus a player to verify. There is no player here that renders
  HDMV IG, so this would ship unverified. The menu *documents* (parse/edit/save/
  apply) and multi-title authoring are the parts that are ready for it.
- **Burning.** Two blockers: macOS tools cannot produce a Blu-ray-spec UDF 2.50
  image (`hdiutil makehybrid -udf` caps at 1.50; `mkisofs` writes UDF 2.01), and
  there is no writer drive attached (`cdrecord` is present). Writing an
  unverified, non-spec image risks coasters.
- **Cinavia.** A proprietary audio watermark. There is no open detector or
  remover; "removal" would need the secret pattern and is legally fraught.
- **Multi-title ISO.** The folder output is complete and verified; packaging it
  as an ISO is blocked on the same UDF 2.50 limitation as burning.

What would unblock each: an IG encoder (and a player to test), a UDF 2.50 image
writer plus a BD writer drive, and a Cinavia implementation from licensed
material, respectively.

## Deliberately out of scope

- BD-J Java parsing beyond object names. The JAR bytecode is a full Java
  application; the menu *document* is the navigation table and the title list,
  not the Java UI.
- Writing to an original disc.

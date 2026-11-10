# Keepsake Atelier

A cross-platform desktop app for **ripping Blu-rays** and everything around them:
decrypted ISOs, MKV/MP4 remuxes, CPU- and GPU-encoded output, plus (later) Blu-ray
authoring, disc menus, and (later) burning.

The engine, CLI binary and crate names are still `keepsake-atelier`.

> **Iteration 1 (this)**: ripping first. Decrypted title streaming, lossless remux
> and re-encode, automatic AACS key refresh, and a desktop UI.

## Status

| Area | State |
| --- | --- |
| Device discovery (IOKit, optical detection) | ✅ |
| AACS key database (`KEYDB.cfg`) auto-fetch / store | ✅ |
| Key store: `keepsake-atelier keys add / list / show / remove / import` | ✅ |
| Disc inspection (titles, streams, HDR, AACS state) via libbluray | ✅ |
| Rip: MKV / MP4 remux (lossless) | ✅ |
| Rip: x264 / x265 / VideoToolbox encode | ✅ |
| Remux metadata: ChapterDB chapter names, TMDB poster, track titles | ✅ |
| Track defaults: English non-commentary audio, subtitles off | ✅ |
| Cover art: search/view/download from HiResCovers, dvdcover.com, TMDB | ✅ |
| Decrypted ISO / folder export | ✅ |
| 1:1 encrypted disc clone | ✅ |
| Multi-title ISO (UDF 2.50, native newfs_udf) | ✅ |
| Burn an ISO to disc (native hdiutil burn) | ✅ (untested without a writer) |
| DVD-Video (`VIDEO_TS`): title list + rip | ✅ |
| TV / anime: per-episode split, SxxExx titles, OP/ED/preview chapters | ✅ |
| Desktop UI (Tauri v2) | ✅ |
| MCP server (all features as tools, localhost, off by default) | ✅ |
| Blu-ray authoring (BDMV / ISO) | ✅ |
| Disc menu documents (parse, edit, save, apply to another disc) | ✅ |
| Batch rip queue (single titles, or **Queue all**) | ✅ |
| Authored menus (generate a menu for a disc you author) | 🚧 blocked on an IG/BD-J toolchain |
| Burning to disc | 🚧 blocked on a UDF 2.50 writer and a writer drive |
| Self-update wiring (Tauri updater) | ✅ (needs a signed release) |
| Windows / Linux | 🚧 (macOS first; non-macOS device layer stubbed) |

Phases and what is next: [`docs/roadmap.md`](docs/roadmap.md).

## Architecture

```
core/           keepsake-core - framework-agnostic engine (Rust)
  device.rs       IOKit device enumeration, optical detection
  keys.rs         KEYDB.cfg fetch + store + validation
  disc.rs         libbluray FFI (titles, decrypted title streaming)
  menu.rs         disc menu documents: index.bdmv navigation + editable templates
  bluray_shim.c   small C shim so Rust avoids libbluray struct layout churn
  rip.rs          ffmpeg orchestration (remux / encode) with progress
  iso.rs          decrypted ISO / backup-folder export
  config.rs       persisted settings
cli/            `keepsake-atelier` - headless CLI for testing the engine
app/            Tauri v2 desktop app (React + TypeScript frontend)
  src-tauri/      Rust commands bridging the UI to core
```

Decryption is Rust orchestration over libbluray plus an AACS/BD+ backend. The
AACS backend is **native `libaacs` by default**, driven by the local `KEYDB.cfg`
key store, so a keyed disc rips with no MakeMKV and no proprietary code. If
**MakeMKV** is installed its `libmmbd` is used only as a fallback: for a disc
whose key is not in KEYDB yet, and for BD+ (Homebrew's `libbdplus` ships no BD+
VM config). Native libaacs cannot read a *mounted* volume, so a rip unmounts it
and remounts it afterwards. `keepsake-atelier libs` and
`keepsake-atelier disc` print the chosen backend; force one with
`KEEPSAKE_DECRYPT=aacs|mmbd`, or override the library paths with
`LIBAACS_PATH` / `LIBBDPLUS_PATH`. ffmpeg is used only for muxing/encoding,
never decryption.

ffmpeg is bundled: `scripts/build-ffmpeg.sh` builds x264, x265, libdvdread and
libdvdnav into a static `ffmpeg`/`ffprobe`, and Tauri copies both into the app.
`KEEPSAKE_FFMPEG` overrides the bundled one. `doctor` reports which features the
resolved build has.

A keyless disc can be keyed from the drive itself with
`keepsake-atelier keys native` (read the volume id from the drive, derive from
the disc's own MKB, no MakeMKV); BD+ VM data for libbdplus installs with
`keepsake-atelier bdplus --install`. See
[`docs/decryption.md`](docs/decryption.md) for the full picture.

## Build & run

Prerequisites (macOS): Rust, Node, and the Homebrew libraries `libbluray
libaacs libbdplus`. The app ships its **own ffmpeg/ffprobe** (x264, x265,
libdvdread/libdvdnav, VideoToolbox); see [`docs/ffmpeg.md`](docs/ffmpeg.md) and
build it once with `scripts/build-ffmpeg.sh`.

```sh
# engine + CLI
cargo build --release
./target/release/keepsake-atelier devices          # find your drive / disc
./target/release/keepsake-atelier keys update       # refresh AACS keys
./target/release/keepsake-atelier keys add --from /Volumes/MY_DISC --vuk 0x... # key a disc
./target/release/keepsake-atelier keys list --keyed --limit 20
./target/release/keepsake-atelier disc /Volumes/MY_DISC   # inspect titles
./target/release/keepsake-atelier libs
./target/release/keepsake-atelier iso /Volumes/MY_DISC ~/movie.iso            # decrypted ISO
./target/release/keepsake-atelier verify /Volumes/MY_DISC            # check navigation/clips
./target/release/keepsake-atelier dvd list /Volumes/DVD              # DVD titles
./target/release/keepsake-atelier dvd rip /Volumes/DVD --title 1 --out ~/movie.mkv
./target/release/keepsake-atelier burn ~/movie.iso --device /dev/rdisk4        # dry run
./target/release/keepsake-atelier burn ~/movie.iso --device /dev/rdisk4 --yes  # write
./target/release/keepsake-atelier doctor                              # what is installed
./target/release/keepsake-atelier clone /dev/rdisk4 ~/movie-clone.iso         # 1:1, encrypted
./target/release/keepsake-atelier rip /Volumes/MY_DISC                        # main title -> ~/Downloads
./target/release/keepsake-atelier rip /dev/rdisk5 --title 3 ~/Movies/out.mkv  # explicit title + output

# desktop app
cd app && npm install
npm run tauri dev        # or: npm run tauri build
```

`BLURAY_PREFIX` overrides the libbluray prefix (default `/opt/homebrew`).
`KEEPSAKE_FFMPEG` points at a specific ffmpeg binary. The pre-rename
`BLUFORGE_*` names are still accepted for every `KEEPSAKE_*` setting.

## Testing without a drive

`scripts/mkbd.mjs` builds a tiny but spec-valid BDMV tree (index/MovieObject/
PLAYLIST/CLIPINF/STREAM plus an ffmpeg-made H.264+AC3 clip) so the whole pipeline
runs with no hardware:

```sh
node scripts/mkbd.mjs /tmp/testbd 8
./target/release/keepsake-atelier disc /tmp/testbd
./target/release/keepsake-atelier rip  /tmp/testbd /tmp/out.mkv --title 0
./target/release/keepsake-atelier iso  /tmp/testbd /tmp/out.iso --label SINTEL
```

## Remux metadata

Ripping a title also enriches the output, via two public services - both
reached keylessly, so there is nothing to configure:

* **Chapter names** from [ChapterDB](https://chapterdb.plex.tv). The disc's own
  chapter marks give the times; ChapterDB supplies the scene names, matched on
  title, chapter count and runtime. If the best entry is for a different cut the
  disc times are kept and the names are applied by position; with no match you
  get plain `Chapter NN`.
* **A poster** from TMDB, written next to the output as `<name>.poster.jpg` and
  attached inside the MKV as `cover.jpg`.

Track metadata is written on the way out: real language tags and descriptive
titles for video, audio and subtitles, the English **non-commentary** audio track
marked default, and every subtitle explicitly non-default so players start with
subtitles off.

Track selection: `--audio all|main|english` and `--subs all|english|none`
(defaults keep everything). Where a TrueHD / DTS-HD track is kept its embedded
lossy core is kept alongside it; nothing else is.

`--no-chapters` / `--no-poster` skip the lookups; `--movie "<title>"` overrides
the name used to search (default: the disc label). A failed lookup is never
fatal. All of these are controls in the app.

### Output template

Where files land is a template, editable in the app (`Output template`) and
stored in the config:

```
{title} ({year})/{title} ({year}) - {resolution} {hdr}
```

`/` creates sub-folders. Movie and release data (`{title}`, `{year}`, `{tmdb}`)
comes from TMDB; the rest comes from the disc (`{resolution}`, `{hdr}`,
`{video_codec}`, `{audio_lang}`, `{audio_codec}`, `{runtime}`, `{chapters}`,
`{playlist}`, `{index}`, `{disc}`). Unknown placeholders are kept verbatim so a
typo is visible, and empty ones collapse cleanly (`Title ()` -> `Title`). The UI
lists every placeholder and previews the rendered path live, using the same
renderer as the ripper.

## Disc menus

A disc's menu is parsed into a *menu document*: the navigation table from
`BDMV/index.bdmv` (first playback, top menu, and each title's HDMV movie object
or BD-J object name) plus the playable titles with their playlists, durations
and chapters.

```sh
keepsake-atelier menu parse /Volumes/MY_DISC --save my-disc   # parse + save a template
keepsake-atelier menu list                                     # saved templates
keepsake-atelier menu edit my-disc --title 51 --name "Feature" --main
keepsake-atelier menu apply my-disc /Volumes/OTHER_DISC        # reuse it elsewhere
```

Documents are JSON under `~/Library/Application Support/keepsake-atelier/menus/`, so
they can also be edited by hand. Applying a template matches titles by playlist
id, then duration, then position, and carries over the title names, the
main-feature flag and chapter labels. The target disc keeps its own navigation
objects.

A saved template whose disc id matches the inserted disc is used when ripping:
the selected title's name feeds `{title}` (and the TMDB query, with a trailing
`(1967)` split off as the year), and its chapter labels replace the ChapterDB
ones when the counts agree. See [`docs/menu-templates.md`](docs/menu-templates.md).

## Cover art

`covers` searches three sources and downloads the one you pick:

```sh
./target/release/keepsake-atelier covers search "Gladiator II" --source hirescovers
./target/release/keepsake-atelier covers get hirescovers 200648 --out ~/Downloads
./target/release/keepsake-atelier covers login     # store a HiResCovers member cookie
```

* **Bundled ffmpeg** - built by `scripts/build-ffmpeg.sh`; see
  [`docs/ffmpeg.md`](docs/ffmpeg.md) for the versions, flags, resolution order
  and GPL obligations. `doctor` shows whether the build has `dvdvideo`,
  `libx264`, `libx265` and VideoToolbox.

* **hirescovers** - case covers and disc labels. Its 204k-item manifest is read
  from `$HIRESCOVERS_DIR/state/gallery_items.jsonl`, so *searching* needs no
  login; the full-size original comes from `secure_image.php` with a member
  cookie and falls back to the public preview without one. Credentials go in the
  config (`hirescovers_user` / `hirescovers_pass`) or paste a cookie directly;
  the tooling's own `state/cookie_free.txt` is used if present.
* **dvdcover.com** - public WordPress REST; the free `single-free` render.
* **tmdb** - the movie poster.

A rip saves a front cover (and a disc label, when one exists) next to the output
as `<name> - cover.jpg` / `<name> - disc.jpg`, unless `--no-covers`. The app has
the same search with thumbnails, and a *Cover art next to the rip* toggle.

### Originals vs the free previews

HiResCovers serves the member original through `secure_image.php`; everything
else on the open web is a *derivative*. `covers inspect <file>` tells them apart
from the bytes alone - no image viewer needed:

```console
$ keepsake-atelier covers inspect cover.jpg
format  : jpeg 720x400
creator : "CREATOR: gd-jpeg v1.0 (using IJG JPEG v62), quality = 85"
verdict : watermarked preview (server re-encode)
  - COM creator is GD (PHP re-encode) - server-generated
  - only 720px on the long side (preview cap)
  - no EXIF/ICC/Adobe markers
  - 96 dpi (originals are 300)
```

The container is decomposed (SOF dimensions, JFIF density, APP0/1/2/13/14, COM)
and the verdict is: *original upload* > *downscaled derivative* > *watermarked
preview*. Downloads and the post-rip cover pick report which they got, and the
picker prefers the best available.

To get true originals, log in - either with credentials in the config
(`covers login`) or by pasting a browser cookie (`covers cookie "<k=v; ...>"`);
both verify the cookie against a known free pid before storing it. Without a
valid cookie the public previews are used and labelled as such.

## Authoring

Turn finished MKV/MP4 files back into a Blu-ray:

```bash
keepsake-atelier author "/path/Episode.mkv" --out "/path/Disc.iso" --label MY_DISC
keepsake-atelier author ep1.mkv ep2.mkv ep3.mkv --out "/path/Set" --label MY_SHOW
keepsake-atelier author ep*.mkv --out "/path/Set" --split 25      # BD-25 per disc
```

* Uses `tsMuxeR`; the app looks for it at
  `~/Library/Application Support/keepsake-atelier/bin/tsMuxeR` (override with
  `KEEPSAKE_TSMUXER`).
* One input writes a UDF 2.50 ISO with a complete BDMV tree. Several inputs
  are authored one title each: into a BDMV folder (`--out` a directory) or
  straight into a UDF 2.50 image (`--out disc.iso`, via macOS `newfs_udf` +
  `hdiutil`). Each input gets its own playlist, tsMuxeR's clips are renumbered,
  and `index.bdmv` / `MovieObject.bdmv` are written with one `PlayPL` object per
  title plus first-play and top-menu entries.
* `--split <GB>` packs whole inputs onto discs of at most that size (25 or 50)
  under `<out>/Disc1`, `Disc2`, ... A title is never cut across discs; an input
  larger than the limit still gets a disc of its own.
* Chapter marks are carried over from the input; `--auto-chapters N` forces one
  every N minutes and `--no-chapters` drops them.
* Single and multi-title authoring are validated with libbluray opening the
  authored tree. The generated navigation follows the libbluray parser layouts;
  a hardware-player pass is still worth doing.

In the app the Titles card has **Queue all** (every title of at least a minute,
skipping menus and trailers), and the keys panel has a searchable key store
with import and remove.

In the app: the **Author a disc** card (choose several files, optional split
size). Over MCP: `author_start` with `inputs` and `split_gb`.

### Verifying a tree

```bash
keepsake-atelier verify /path/Set/Disc1
keepsake-atelier verify /Volumes/MY_DISC --json
```

Reads `index.bdmv` and `MovieObject.bdmv` directly and checks that every HDMV
title's movie object plays a playlist that exists, and that every clip a
playlist references has its `.clpi` and `.m2ts`. It prints the title map and
per-title duration/mark count, exits non-zero on problems, and warns about BD-J
titles it cannot check. **Verify** in the app (next to Eject, and next to
Author) runs the same check.

## MCP server

Every capability is available as an MCP tool, served by the app itself on
localhost. It is **off by default** (no listening socket); enable it in the
sidebar (**MCP server**) or in the config:

```json
{ "mcp_enabled": true, "mcp_port": 8848 }
```

* Endpoint: `POST http://127.0.0.1:8848/mcp` (JSON-RPC 2.0, stateless HTTP)
* Health: `GET http://127.0.0.1:8848/`
* Bound to `127.0.0.1` only; the listener stops when the toggle is off or the
  port changes.

Client configuration:

```json
{
  "mcpServers": {
    "keepsake-atelier": { "type": "http", "url": "http://127.0.0.1:8848/mcp" }
  }
}
```

Tools:

| Tool | What it does |
| --- | --- |
| `status` | version, optical drives, key DB, port, output folder |
| `disc_info` | titles, streams, chapters for a disc / ISO / folder |
| `tv_plan` | split a TV disc into episodes (TMDB titles, stills) |
| `rip_start` | remux or encode, movie or TV; returns a job id |
| `job_status` / `job_cancel` | progress and cancellation for rips and exports |
| `iso_start` | decrypted ISO or BDMV folder export |
| `author_start` | author one or several files, optionally split per disc |
| `verify` | check a BDMV tree: navigation, playlists, clips |
| `dvd_info` / `dvd_start` | list DVD titles / rip one (MPEG-2 + AC-3/DTS copy) |
| `clone_start` | 1:1 encrypted disc copy to an ISO |
| `burn_start` | write an ISO to a writer drive (dry run unless confirmed) |
| `doctor` | installed tools and which capability each missing piece blocks |
| `covers_search` / `covers_download` | cover art from hirescovers / dvdcover / TMDB |
| `keys_update` | refresh KEYDB.cfg |
| `keys_list` | list keyed discs (id, name), optionally filtered |
| `keys_add` | add/replace a disc key (VUK and/or unit key) |
| `keys_remove` | drop a disc's key entry |
| `key_entry` / `keys_set_units` | inspect a disc entry / edit AACS2 unit keys |
| `menu_parse` / `menu_list` / `menu_show` / `menu_edit` / `menu_apply` / `menu_remove` | disc menu documents |
| `eject` | open the tray |
| `config_get` / `config_set` | read or patch settings |
| `preview_output` / `template_placeholders` | render the output template |

A stale `/dev/rdiskN` (USB drives re-enumerate) is resolved to the current
optical device automatically.

## Decryption backends

Two AACS implementations can drive libbluray. **Native `libaacs` is the
default**; MakeMKV's `libmmbd` is a fallback, not a requirement.

| Backend | When | Notes |
| --- | --- | --- |
| `libaacs` + KEYDB | default, whenever the disc is keyed | No proprietary code, no MakeMKV process. Decrypts from the disc's VUK/unit key in `KEYDB.cfg`; libaacs needs the raw device and an *unmounted* volume, which the rip arranges. |
| `libmmbd` (MakeMKV) | a disc that cannot be keyed natively, or BD+ | ABI-compatible; last-resort fallback. Force with `KEEPSAKE_DECRYPT=mmbd`. |

Key a disc so it never needs MakeMKV again:

```sh
keepsake-atelier keys add --from /Volumes/MY_DISC --vuk 0x<VUK> [--unit-key 0x<UK>] --name "My Disc"
keepsake-atelier keys show --from /Volumes/MY_DISC  # VUK + AACS2 unit keys
keepsake-atelier keys list --keyed
keepsake-atelier keys import /path/to/KEYDB.cfg     # merge a community database
```

AACS2 (UHD) discs need a unit-key table instead of a VUK alone. `keys show`
prints the entry and its unit keys; the app's keys panel has the same view with
a unit-key editor. Unit keys are imported, not derived, and the KEYDB entry must
already exist (key the disc first).

The VUK (and, for AHD/AACS2 discs, the unit key) come from the disc's `KEYDB.cfg`
entry, from `keepsake-atelier keys native` (VID read from the drive, media key from the
disc's own MKB), or from a one-off MakeMKV/FindVUK pass. `keepsake-atelier aacs <device>
--disc <mount>` reads the drive's AACS volume ID (LibreDrive vendor read, or the
standard command if the drive serves it) and seeds libaacs's cache.

## DVD-Video

The bundled ffmpeg has the `dvdvideo` demuxer, so a DVD can be read without
libbluray or any extra install:

```sh
keepsake-atelier dvd list /Volumes/MY_DVD          # title, VTS, chapters, duration
keepsake-atelier dvd rip /Volumes/MY_DVD --title 2 --out ~/movie.mkv
```

Titles come from the VMG `VIDEO_TS.IFO` title search pointer table. The rip
copies MPEG-2 video and AC-3/DTS audio (no re-encode); DVD subtitles are omitted
unless `--subs` is passed, and menus are not ripped. In the app, pointing the
disc field at a DVD shows a **DVD-Video** card with the title list and a rip
button; progress and cancel work like a Blu-ray rip. MCP: `dvd_info` and
`dvd_start`.

## Burning

A Blu-ray ISO (from `author --out disc.iso`, single or multi-title) can be
written to a writer drive:

```sh
keepsake-atelier burn ~/movie.iso          # native hdiutil burn, prints the command
keepsake-atelier burn ~/movie.iso --yes    # actually writes
keepsake-atelier burn ~/movie.iso --device /dev/rdisk4 --yes   # explicit growisofs
```

Without `--device` it uses macOS's own `hdiutil burn`, which picks the writer;
`--device /dev/rdiskN` selects growisofs (cdrecord fallback). The disc is erased
and there is no undo, so the CLI requires `--yes` and the app asks for
confirmation. The app has a **Burn ISO to disc** button in the Decrypted ISO
panel.

Multi-title discs are authored straight into a native UDF 2.50 image
(`newfs_udf` + `hdiutil`), so they can be burned too.

## System check

```sh
keepsake-atelier doctor          # or --json
```

Lists libbluray, libaacs, libbdplus (+ BD+ VM data), ffmpeg (and whether it has
the `dvdvideo` demuxer), tsMuxeR, KEYDB, the optical drive, the burning tools and
the data directory, then a capability matrix: Blu-ray rip, BD+ rip, native key
recovery, authoring, DVD, burning, verification. Each missing item names the
command that fixes it. The app has the same list under **System check**.

## Configuration & data

* Config: `~/Library/Application Support/keepsake-atelier/config.json`
* Keys:   `~/Library/Application Support/keepsake-atelier/KEYDB.cfg`
* Disc memory: `~/Library/Application Support/keepsake-atelier/tv_discs.json` - which
  episode numbers each disc held, so a later disc continues from there even when
  discs hold different counts (8 + 4 here). Override with `--start-episode`.

Keys refresh on launch (toggleable) and via the **Update** button.

## Self-update

The app uses `tauri-plugin-updater` against a `latest.json` release manifest.
Signing key: `~/.tauri/bluforge.key` (public key embedded in `tauri.conf.json`).
On macOS an update downloads the new `.app`/DMG and relaunches; releasing requires
setting `TAURI_SIGNING_PRIVATE_KEY` when running `npm run tauri build`.

## macOS note

macOS binds optical drives through its SCSI multimedia driver, which only
matches a device that reports SCSI peripheral type 5. Some USB-SATA bridges make
the drive enumerate as a generic type-0 mass-storage device - in practice this
happens when the drive sits behind a hub, and the drive then looks absent
(`drutil` and `system_profiler SPDiscBurning` show nothing). **Plug the drive
straight into the Mac.**

Reading a disc also needs the terminal (or the app) to have **Full Disk Access**,
otherwise TCC blocks `open()` on removable volumes, and raw `/dev/rdiskN` access
needs membership of the `operator` group.

## License

MIT - see [LICENSE](LICENSE).

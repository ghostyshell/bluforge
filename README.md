# BluForge

A cross-platform desktop app for **ripping Blu-rays** and everything around them:
decrypted ISOs, MKV/MP4 remuxes, CPU- and GPU-encoded output, plus (later) Blu-ray
authoring with custom menus and burning.

> **Iteration 1 (this)**: ripping first. Decrypted title streaming, lossless remux
> and re-encode, automatic AACS key refresh, and a desktop UI.

## Status

| Area | State |
| --- | --- |
| Device discovery (IOKit, optical detection) | ✅ |
| AACS key database (`KEYDB.cfg`) auto-fetch / store | ✅ |
| Disc inspection (titles, streams, HDR, AACS state) via libbluray | ✅ |
| Rip: MKV / MP4 remux (lossless) | ✅ |
| Rip: x264 / x265 / VideoToolbox encode | ✅ |
| Remux metadata: ChapterDB chapter names, TMDB poster, track titles | ✅ |
| Track defaults: English non-commentary audio, subtitles off | ✅ |
| Cover art: search/view/download from HiResCovers, dvdcover.com, TMDB | ✅ |
| Decrypted ISO / folder export | ✅ |
| Desktop UI (Tauri v2) | ✅ |
| Blu-ray authoring + menus + burning | 🚧 |
| Self-update wiring (Tauri updater) | ✅ (needs a signed release) |
| Windows / Linux | 🚧 (macOS first; non-macOS device layer stubbed) |

## Architecture

```
core/           bluforge-core — framework-agnostic engine (Rust)
  device.rs       IOKit device enumeration, optical detection
  keys.rs         KEYDB.cfg fetch + store + validation
  disc.rs         libbluray FFI (titles, decrypted title streaming)
  bluray_shim.c   small C shim so Rust avoids libbluray struct layout churn
  rip.rs          ffmpeg orchestration (remux / encode) with progress
  iso.rs          decrypted ISO / backup-folder export
  config.rs       persisted settings
cli/            `bluforge` — headless CLI for testing the engine
app/            Tauri v2 desktop app (React + TypeScript frontend)
  src-tauri/      Rust commands bridging the UI to core
```

Decryption is Rust orchestration over libbluray plus an AACS/BD+ backend: by
default `libaacs` + `libbdplus` from Homebrew. If **MakeMKV** is installed its
`libmmbd` is preferred instead, since it is ABI-compatible and additionally
handles LibreDrive, AACS bus encryption and BD+ (Homebrew's libbdplus ships no
BD+ VM config, and some drives reject libaacs's built-in host certificate).
`bluforge libs` prints the backend actually in use; override either choice with
`LIBAACS_PATH` / `LIBBDPLUS_PATH`. ffmpeg is used only for muxing/encoding,
never decryption.

## Build & run

Prerequisites (macOS, Homebrew): `ffmpeg libbluray libaacs libbdplus`, Rust, Node.

```sh
# engine + CLI
cargo build --release
./target/release/bluforge devices          # find your drive / disc
./target/release/bluforge keys update       # refresh AACS keys
./target/release/bluforge disc /Volumes/MY_DISC   # inspect titles
./target/release/bluforge libs
./target/release/bluforge iso /Volumes/MY_DISC ~/movie.iso            # decrypted ISO
./target/release/bluforge rip /Volumes/MY_DISC                        # main title -> ~/Downloads
./target/release/bluforge rip /dev/rdisk5 --title 3 ~/Movies/out.mkv  # explicit title + output

# desktop app
cd app && npm install
npm run tauri dev        # or: npm run tauri build
```

`BLURAY_PREFIX` overrides the libbluray prefix (default `/opt/homebrew`).
`BLUFORGE_FFMPEG` points at a specific ffmpeg binary.

## Testing without a drive

`scripts/mkbd.mjs` builds a tiny but spec-valid BDMV tree (index/MovieObject/
PLAYLIST/CLIPINF/STREAM plus an ffmpeg-made H.264+AC3 clip) so the whole pipeline
runs with no hardware:

```sh
node scripts/mkbd.mjs /tmp/testbd 8
./target/release/bluforge disc /tmp/testbd
./target/release/bluforge rip  /tmp/testbd /tmp/out.mkv --title 0
./target/release/bluforge iso  /tmp/testbd /tmp/out.iso --label SINTEL
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

## Cover art

`covers` searches three sources and downloads the one you pick:

```sh
./target/release/bluforge covers search "Gladiator II" --source hirescovers
./target/release/bluforge covers get hirescovers 200648 --out ~/Downloads
./target/release/bluforge covers login     # store a HiResCovers member cookie
```

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

## Configuration & data

* Config: `~/Library/Application Support/bluforge/config.json`
* Keys:   `~/Library/Application Support/bluforge/KEYDB.cfg`

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

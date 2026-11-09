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
| Key store: `bluforge keys add / list / show / remove / import` | ✅ |
| Disc inspection (titles, streams, HDR, AACS state) via libbluray | ✅ |
| Rip: MKV / MP4 remux (lossless) | ✅ |
| Rip: x264 / x265 / VideoToolbox encode | ✅ |
| Remux metadata: ChapterDB chapter names, TMDB poster, track titles | ✅ |
| Track defaults: English non-commentary audio, subtitles off | ✅ |
| Cover art: search/view/download from HiResCovers, dvdcover.com, TMDB | ✅ |
| Decrypted ISO / folder export | ✅ |
| TV / anime: per-episode split, SxxExx titles, OP/ED/preview chapters | ✅ |
| Desktop UI (Tauri v2) | ✅ |
| MCP server (all features as tools, localhost, off by default) | ✅ |
| Blu-ray authoring + menus + burning | 🚧 |
| Self-update wiring (Tauri updater) | ✅ (needs a signed release) |
| Windows / Linux | 🚧 (macOS first; non-macOS device layer stubbed) |

## Architecture

```
core/           bluforge-core - framework-agnostic engine (Rust)
  device.rs       IOKit device enumeration, optical detection
  keys.rs         KEYDB.cfg fetch + store + validation
  disc.rs         libbluray FFI (titles, decrypted title streaming)
  bluray_shim.c   small C shim so Rust avoids libbluray struct layout churn
  rip.rs          ffmpeg orchestration (remux / encode) with progress
  iso.rs          decrypted ISO / backup-folder export
  config.rs       persisted settings
cli/            `bluforge` - headless CLI for testing the engine
app/            Tauri v2 desktop app (React + TypeScript frontend)
  src-tauri/      Rust commands bridging the UI to core
```

Decryption is Rust orchestration over libbluray plus an AACS/BD+ backend. The
AACS backend is **native `libaacs` by default**, driven by the local `KEYDB.cfg`
key store, so a keyed disc rips with no MakeMKV and no proprietary code. If
**MakeMKV** is installed its `libmmbd` is used only as a fallback: for a disc
whose key is not in KEYDB yet, and for BD+ (Homebrew's `libbdplus` ships no BD+
VM config). Native libaacs cannot read a *mounted* volume, so a rip unmounts it
and remounts it afterwards. `bluforge libs` and `bluforge disc` print the chosen
backend; force one with `BLUFORGE_DECRYPT=aacs|mmbd`, or override the library
paths with `LIBAACS_PATH` / `LIBBDPLUS_PATH`. ffmpeg is used only for
muxing/encoding, never decryption.

A keyless disc can be keyed from the drive itself with `bluforge keys native`
(authenticate + derive from the MKB, no MakeMKV); BD+ VM data for libbdplus
installs with `bluforge bdplus --install`. See
[`docs/decryption.md`](docs/decryption.md) for the full picture.

## Build & run

Prerequisites (macOS, Homebrew): `ffmpeg libbluray libaacs libbdplus`, Rust, Node.

```sh
# engine + CLI
cargo build --release
./target/release/bluforge devices          # find your drive / disc
./target/release/bluforge keys update       # refresh AACS keys
./target/release/bluforge keys add --from /Volumes/MY_DISC --vuk 0x... # key a disc
./target/release/bluforge keys list --keyed --limit 20
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

### Originals vs the free previews

HiResCovers serves the member original through `secure_image.php`; everything
else on the open web is a *derivative*. `covers inspect <file>` tells them apart
from the bytes alone - no image viewer needed:

```console
$ bluforge covers inspect cover.jpg
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

Turn a finished MKV/MP4 back into a Blu-ray:

```bash
bluforge author "/path/Episode.mkv" --out "/path/Disc.iso" --label MY_DISC
```

* Uses `tsMuxeR`; the app looks for it at
  `~/Library/Application Support/bluforge/bin/tsMuxeR` (override with
  `BLUFORGE_TSMUXER`).
* Writes a UDF 2.50 image with a complete BDMV tree (index.bdmv,
  MovieObject.bdmv, PLAYLIST, CLIPINF, STREAM) - playable in a hardware player.
* Chapter marks are carried over from the input; `--auto-chapters N` forces one
  every N minutes and `--no-chapters` drops them.
* One file per disc for now: multi-episode discs need per-episode playlists
  (the next authoring step).

In the app: the **Author a disc** card. Over MCP: `author_start`.

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
    "bluforge": { "type": "http", "url": "http://127.0.0.1:8848/mcp" }
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
| `covers_search` / `covers_download` | cover art from hirescovers / dvdcover / TMDB |
| `keys_update` | refresh KEYDB.cfg |
| `keys_list` | list keyed discs (id, name) |
| `keys_add` | add/replace a disc key (VUK and/or unit key) |
| `keys_remove` | drop a disc's key entry |
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
| `libmmbd` (MakeMKV) | the disc is not in KEYDB yet, or BD+ | ABI-compatible; the fallback while a disc is still unkeyed. Force with `BLUFORGE_DECRYPT=mmbd`. |

Key a disc so it never needs MakeMKV again:

```sh
bluforge keys add --from /Volumes/MY_DISC --vuk 0x<VUK> [--unit-key 0x<UK>] --name "My Disc"
bluforge keys list --keyed
bluforge keys import /path/to/KEYDB.cfg     # merge a community database
```

The VUK (and, for AHD/AACS2 discs, the unit key) come from the disc's `KEYDB.cfg`
entry, or from a one-off MakeMKV/FindVUK pass. `bluforge aacs <device> --disc
<mount>` reads the drive's AACS volume ID and seeds libaacs's cache; on a drive
that rejects libaacs's own AGID (sense `05/6F/02`) the KEYDB entry is what makes
the disc decryptable.

## Configuration & data

* Config: `~/Library/Application Support/bluforge/config.json`
* Keys:   `~/Library/Application Support/bluforge/KEYDB.cfg`
* Disc memory: `~/Library/Application Support/bluforge/tv_discs.json` - which
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

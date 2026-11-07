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

Decryption is **pure Rust orchestration over libaacs / libbdplus / libbluray**
(no MakeMKV dependency). ffmpeg is used only for muxing/encoding, never decryption.

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

macOS 26+ no longer ships a CD/DVD SCSI peripheral driver, so drives must attach
directly (avoid unpowered hubs) and be readable as a normal block device. BluForge
enumerates via IOKit and reads discs through libbluray.

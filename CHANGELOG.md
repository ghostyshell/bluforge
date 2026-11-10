# Changelog

All notable changes to Keepsake Atelier. Newest first.

## Unreleased

### Changed

- The GitHub repository and the local folder are now `keepsake-atelier`
  (`github.com/ghostyshell/keepsake-atelier`). The Cargo `repository` field and
  the app's updater endpoint point at it. Crate and binary names remain
  `keepsake-atelier`.
- **The desktop app is now Keepsake Atelier**, with a new app icon and in-app
  logo. Product name, window title, bundle identifier
  (`click.keepsakeatelier.app`) and the docs were updated. The engine and CLI
  binary are still `keepsake-atelier`, and the data directory
  (`~/Library/Application Support/keepsake-atelier`) is unchanged so existing keys,
  menus and settings carry over.
- The desktop app now surfaces the menu documents (parse, edit, save, apply),
  native key derivation for the inserted disc, BD+ VM installation, and the
  per-title stream list.
- `plan_backend` tries the fully native key derivation even when MakeMKV is not
  installed, and `keepsake-atelier aacs` prints the VID obtained from the LibreDrive
  vendor read. MakeMKV's `discatt.dat` is now only a fallback for a drive whose
  LibreDrive vendor read fails.

### Added

- **Queue all and a key-store browser:** the Titles card can queue every real
  title (>= 1 minute) with a "queued" badge per row, and the keys panel searches
  the 189k-entry KEYDB (name or id, first 20), imports another KEYDB.cfg and
  removes entries. `keys list --search` and the MCP `keys_list` `search` filter
  do the same from the CLI/agents.
- **MCP `clone_start`:** the 1:1 encrypted disc copy is now a job tool too.
- **DVD-Video support:** `dvd list` reads the VMG title table from
  `VIDEO_TS.IFO`, and `dvd rip --title N` copies MPEG-2 + AC-3/DTS with the
  bundled `dvdvideo` demuxer (no re-encode, optional VobSub subtitles). The app
  shows a DVD-Video card with a title list and rip button; MCP has `dvd_info`
  and `dvd_start`. No menus are ripped.
- **Bundled ffmpeg/ffprobe:** `scripts/build-ffmpeg.sh` builds x264 (master),
  x265 4.2, libdvdread 7.1.1, libdvdnav 7.0.0 and ffmpeg n9.0.2 into a static
  binary, then copies it into the app bundle. Adds the `dvdvideo` demuxer (DVD
  is no longer toolchain-blocked), GPL x264/x265 encoders, VideoToolbox and the
  native TrueHD/DTS/Atmos/AC-3/E-AC-3 codecs. `core::binaries` prefers the
  bundled helper (env override, executable dir, `.app/Contents/Resources`, dev
  tree, managed dir, PATH), and `doctor` reports the resolved build's features.
  See `docs/ffmpeg.md` (including the GPLv2+ distribution obligations).
- **System check:** `keepsake-atelier doctor` (and an app **System check**
  panel, plus an MCP `doctor` tool) lists the installed stack, the ffmpeg
  `dvdvideo` demuxer, tsMuxeR, KEYDB, drive and burning tools, then a capability
  matrix with the fix for each missing item, so DVD/burning limits are
  diagnosable rather than mysterious.
- **BDMV verification:** `keepsake-atelier verify <path>` (and **Verify** in the
  app, plus an MCP `verify` tool) parses index/MovieObject and every playlist
  directly, maps each title to its playlist, checks the clip files exist, and
  reports duration and mark counts. Non-zero exit and per-title errors on
  problems; BD-J titles are reported as unchecked.
- **Multi-title authoring and BD-25/50 splitting:** `author` takes several
  inputs and writes one title each into a BDMV folder, renumbering clips and
  generating `index.bdmv`/`MovieObject.bdmv` (one `PlayPL` object per title,
  `JumpTitle` first play/top menu). `--split <GB>` (and a split field in the app)
  packs whole inputs onto discs of at most that size under `<out>/DiscN`, never
  cutting a title. MCP: `author_start` with `inputs` and `split_gb`.
- **Menu templates feed the rip:** when a saved template's disc id matches the
  source, the selected title's name becomes `{title}` (a trailing `(1967)` is
  split off as `{year}`), and its chapter labels replace the ChapterDB names
  when the counts match. Editing a template now changes the next rip.
- **Native AACS2 unit-key derivation:** `AACS/Unit_Key_RO.inf` is parsed (AACS1
  48-byte records, AACS2 64-byte; `Content000.cer[0] == 0x10` marks AACS2) and
  each encrypted unit key is AES-128-ECB decrypted with the VUK. `keys native`
  and `keys from-makemkv` now write the `U` table into KEYDB automatically, and
  the count is printed/displayed. Unit tested with a synthetic UKF for both
  layouts.
- **AACS2/UHD unit-key workflow:** `keepsake-atelier keys show [--disc|--from]
  [--json]` prints a disc's KEYDB entry including unit keys, and the app's keys
  panel shows the VUK/unit-key state with an editor that saves the `U` table.
  Unit keys are imported, not derived; the entry must already exist.
- **1:1 disc clone** (`keepsake-atelier clone`, and "Clone 1:1" in the app): a raw
  sector copy that keeps AACS/BD+ intact, for an archival image or a burner.
  Uncached reads, byte-based progress, cancel removes the partial file. Unit
  tested against a file source.
- **Batch rip queue** in the desktop app: "Start rip" enqueues the selected
  title (or TV plan), and `+` on a title row queues that title. Jobs run one at
  a time, with a queue panel for state, progress, cancel, resume and cleanup.
  The queue persists to `queue.json` and is restored on launch, but never
  auto-starts.
- **Disc menu documents** (`keepsake-atelier menu`): parse a disc's menu into an
  editable JSON document, save it as a named template, and apply it to a
  different disc. The document holds the `BDMV/index.bdmv` navigation table
  (first playback, top menu, HDMV movie objects and BD-J object names) and the
  playable titles with editable names, main-feature flag and chapter labels.
  `apply` matches by playlist id, then duration, then position. See
  `docs/menu-templates.md`; the roadmap is in `docs/roadmap.md`.
- **Native VID acquisition, no certificate and no MakeMKV.** A LibreDrive drive
  (this BU40N) decrypts the AACS volume id in its own firmware and serves it over
  vendor `READ BUFFER (0x3c)` buffer id `0x4410`: a 36-byte record
  `00 22 | 00 50 | <16-byte VID> | 16 zeros`. `bf_aacs_vid_libredrive()` issues
  that read, and `bf_native_key()` prefers it, so `keepsake-atelier keys native` now
  derives VID, MK, and VUK with **no host certificate at all**. The certificate
  path remains as the fallback for a non-LibreDrive drive.
  (`makemkvcon` sends zero `SEND KEY`/`REPORT KEY` and writes nothing to the
  drive; the mechanism was captured by launching it under lldb and breaking on
  `SCSITaskClass::SetCommandDescriptorBlock`.)
- **Native key derivation for an unkeyed disc** (`keepsake-atelier keys native`): the
  drive is read for the volume id (LibreDrive vendor read, or AGID + host
  certificate when needed), the media key is recovered from the disc's own MKB
  with the KEYDB device keys, and the VUK is derived. No MakeMKV, no pre-existing
  KEYDB entry.
- **`keepsake-atelier bdplus [--install]`**: install the public BD+ VM data into
  libbdplus's config dir, so BD+ discs also come off MakeMKV.
- **Native libaacs decryption is now the default.** A disc whose key is in
  `KEYDB.cfg` decrypts and rips with no MakeMKV. MakeMKV's `libmmbd` is used
  only as a fallback (unkeyed disc, or BD+). Force either backend with
  `KEEPSAKE_DECRYPT=aacs|mmbd`.
- **Key store:** `keepsake-atelier keys add|list|show|remove|import` over `KEYDB.cfg`,
  disc id from `AACS/Unit_Key_RO.inf`, with unit tests. MCP tools `keys_list`,
  `keys_add`, `keys_remove`. `keys update` now merges: an entry added locally
  and not yet in the community database survives a refresh.
- **`keepsake-atelier rip --retries N`** (default 2) re-runs a failed rip end to end,
  covering the transient `Unable decrypt unit` seen on the first read after a
  drive replug.
- `keepsake-atelier libs` and `keepsake-atelier disc` print the chosen decrypt backend; the rip
  logs it with the disc id and keyed flag.
- `docs/decryption.md`: the AACS split, the working setup, the traps, and the
  remaining gap (native AGID drive auth).

### Fixed

- `mount_point_for_device` accepted only an exact `/dev/...` string, so a
  `/dev/diskN` source failed to resolve its mount point. That made a keyed disc
  look unkeyed (routing to the native attempt and libmmbd instead of libaacs).
  It now matches on the BSD name regardless of `/dev/` or `r` prefix.

### Added

- `keepsake-atelier keys from-makemkv`: recover a disc key from MakeMKV's `discatt.dat`
  VID plus offline MKB derivation (`VUK = AES-G(MK, VID)`), bypassing the host
  certificate entirely. Also caches the VID/RDK where libaacs expects them, and
  `plan_backend` falls back to it when native auth is refused.

### Fixed

- `calc_pk` narrowed `dev_key_v_mask` with a logical shift; libaacs uses an
  arithmetic shift, without which the loop never converges and media-key
  derivation hangs.

### Changed

- The libaacs backend is handed a *mounted* volume (libaacs resolves the drive
  via statfs and manages unmount/remount itself); an unkeyed disc now routes to
  libaacs, not libmmbd. The native path links libaacs's inner crypto/MKB/MMC
  helpers (`libaacs.a`) so the AACS maths is not reimplemented.
- Native libaacs cannot read a mounted volume on this stack, so a rip now
  unmounts the disc, reads the raw `/dev/rdiskN`, and remounts afterwards. The
  backend is resolved before the unmount and threaded through nested opens.
- Workspace formatted with `cargo fmt`.

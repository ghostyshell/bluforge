# Changelog

All notable changes to Keepsake Atelier. Newest first.

## Unreleased

### Changed

- **The desktop app is now Keepsake Atelier**, with a new app icon and in-app
  logo. Product name, window title, bundle identifier
  (`click.keepsakeatelier.app`) and the docs were updated. The engine and CLI
  binary are still `bluforge`, and the data directory
  (`~/Library/Application Support/bluforge`) is unchanged so existing keys,
  menus and settings carry over.
- The desktop app now surfaces the menu documents (parse, edit, save, apply),
  native key derivation for the inserted disc, BD+ VM installation, and the
  per-title stream list.
- `plan_backend` tries the fully native key derivation even when MakeMKV is not
  installed, and `bluforge aacs` prints the VID obtained from the LibreDrive
  vendor read. MakeMKV's `discatt.dat` is now only a fallback for a drive whose
  LibreDrive vendor read fails.

### Added

- **Batch rip queue** in the desktop app: "Start rip" enqueues the selected
  title (or TV plan), and `+` on a title row queues that title. Jobs run one at
  a time, with a queue panel for state, progress, cancel and cleanup. In-memory
  only for now.
- **Disc menu documents** (`bluforge menu`): parse a disc's menu into an
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
  that read, and `bf_native_key()` prefers it, so `bluforge keys native` now
  derives VID, MK, and VUK with **no host certificate at all**. The certificate
  path remains as the fallback for a non-LibreDrive drive.
  (`makemkvcon` sends zero `SEND KEY`/`REPORT KEY` and writes nothing to the
  drive; the mechanism was captured by launching it under lldb and breaking on
  `SCSITaskClass::SetCommandDescriptorBlock`.)
- **Native key derivation for an unkeyed disc** (`bluforge keys native`): the
  drive is read for the volume id (LibreDrive vendor read, or AGID + host
  certificate when needed), the media key is recovered from the disc's own MKB
  with the KEYDB device keys, and the VUK is derived. No MakeMKV, no pre-existing
  KEYDB entry.
- **`bluforge bdplus [--install]`**: install the public BD+ VM data into
  libbdplus's config dir, so BD+ discs also come off MakeMKV.
- **Native libaacs decryption is now the default.** A disc whose key is in
  `KEYDB.cfg` decrypts and rips with no MakeMKV. MakeMKV's `libmmbd` is used
  only as a fallback (unkeyed disc, or BD+). Force either backend with
  `BLUFORGE_DECRYPT=aacs|mmbd`.
- **Key store:** `bluforge keys add|list|show|remove|import` over `KEYDB.cfg`,
  disc id from `AACS/Unit_Key_RO.inf`, with unit tests. MCP tools `keys_list`,
  `keys_add`, `keys_remove`. `keys update` now merges: an entry added locally
  and not yet in the community database survives a refresh.
- **`bluforge rip --retries N`** (default 2) re-runs a failed rip end to end,
  covering the transient `Unable decrypt unit` seen on the first read after a
  drive replug.
- `bluforge libs` and `bluforge disc` print the chosen decrypt backend; the rip
  logs it with the disc id and keyed flag.
- `docs/decryption.md`: the AACS split, the working setup, the traps, and the
  remaining gap (native AGID drive auth).

### Fixed

- `mount_point_for_device` accepted only an exact `/dev/...` string, so a
  `/dev/diskN` source failed to resolve its mount point. That made a keyed disc
  look unkeyed (routing to the native attempt and libmmbd instead of libaacs).
  It now matches on the BSD name regardless of `/dev/` or `r` prefix.

### Added

- `bluforge keys from-makemkv`: recover a disc key from MakeMKV's `discatt.dat`
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

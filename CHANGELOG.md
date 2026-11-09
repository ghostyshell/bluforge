# Changelog

All notable changes to BluForge. Newest first.

## Unreleased

### Added

- **Native key derivation for an unkeyed disc** (`bluforge keys native`): the
  drive is authenticated (AGID + host certificate) to read the volume id, the
  media key is recovered from the disc's own MKB with the KEYDB device keys, and
  the VUK is derived. No MakeMKV, no pre-existing KEYDB entry.
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

### Changed

- The libaacs backend is handed a *mounted* volume (libaacs resolves the drive
  via statfs and manages unmount/remount itself); an unkeyed disc now routes to
  libaacs, not libmmbd. The native path links libaacs's inner crypto/MKB/MMC
  helpers (`libaacs.a`) so the AACS maths is not reimplemented.
- Native libaacs cannot read a mounted volume on this stack, so a rip now
  unmounts the disc, reads the raw `/dev/rdiskN`, and remounts afterwards. The
  backend is resolved before the unmount and threaded through nested opens.
- Workspace formatted with `cargo fmt`.

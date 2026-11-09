# Decryption notes (AACS / BD+)

How BluForge decrypts, what is still not built, and the traps seen in practice.
Keep this in sync with `core/src/disc.rs` and `core/src/keys.rs`.

## The problem splits in two

AACS has two independent halves:

1. **Drive authentication** to obtain the 16-byte volume ID. Needs either a
   licensed host certificate with device keys, or LibreDrive (vendor-mode unlock
   or patched firmware). This is a credential, not an algorithm.
2. **Key derivation and AES decryption.** Disc ID is `SHA-1(AACS/Unit_Key_RO.inf)`.
   The VUK yields unit/title keys that drive AES-128-CBC over 6144-byte units
   (first 16 bytes clear, address-derived IV). Standard published math.

BluForge does **not** implement (1). It does (2) through libaacs, driven by the
local `KEYDB.cfg`.

## How BluForge decrypts today

- **Native libaacs is the default** backend. It reads the disc's VUK and unit
  key from `KEYDB.cfg` and never touches the drive's MMC, so role 1 is moot for a
  keyed disc.
- libaacs is only reliable against the **raw device** with the volume
  **unmounted** on this macOS stack; it has failed against a mounted volume.
  `disc::prepare_source` therefore unmounts the disc, opens `/dev/rdiskN`, and
  `disc::RemountGuard` remounts it on the way out. libmmbd tolerates a mounted
  device, so it only needs the raw device.
- The backend is resolved **before** the unmount and passed explicitly to
  `Disc::open_with_backend`. Re-resolving afterwards would miss the now-absent
  mount point and silently pick the wrong library. This is why the rip threads
  `DecryptBackend` into `open`/`split_clips`/`probe_sub_events` rather than each
  call site resolving on its own.
- MakeMKV's `libmmbd` is the fallback: a disc with no KEYDB entry, or BD+
  (Homebrew's `libbdplus` ships no BD+ VM config). Force either backend with
  `BLUFORGE_DECRYPT=aacs|mmbd`. `bluforge libs` / `disc` print the choice.

libaacs's own macOS MMC layer cannot even find the device ("Could not find
matching IOBDServices mounted @ /dev/rdiskN"), so it is driven entirely from
KEYDB. On the reference disc its output was the same size, and decoded
identically, to libmmbd's.

## Key store

`bluforge keys add|list|show|remove|import` manages `KEYDB.cfg` line entries
(`0x<discid> = <name> | D | <date> | M | ... | I | ... | V | <vuk> | U | <n>-<keys>`).
Disc id is `SHA-1(AACS/Unit_Key_RO.inf)`; `--from <mount>` derives it. The store
is what "retires MakeMKV": once a disc is keyed it rips natively and offline.

Back up `KEYDB.cfg` before bulk edits. `keys update` merges rather than
replaces, so an entry you added that the community database does not have yet
is preserved on refresh.

## Getting a key for a new disc

Because role 1 is unbuilt, a disc absent from KEYDB needs its VUK (and, for an
AHD/AACS2 disc, its unit key) from an external oracle: the community KEYDB that
ships with BluForge, or a one-off MakeMKV/FindVUK pass. Then:

```sh
bluforge keys add --from /Volumes/MY_DISC --vuk 0x<VUK> [--unit-key 0x<UK>]
```

After that the disc needs no MakeMKV.

## Traps seen in practice

- **The drive drops off the USB bus** under sustained reads (HL-DT-ST BD-RE
  BU40N in a bus-powered Archgon enclosure). Symptom: the device disappears
  (`drutil` reports "No Media Inserted", no `/dev/disk` node). It needs a
  physical replug; a powered hub is the real fix.
- **The first rip after a replug tends to fail** with `Unable decrypt unit`
  (libaacs) and then succeeds on retry. `bluforge rip` now re-runs the whole rip
  on failure by default (`--retries`, default 2), so this is handled without a
  manual second command.
- **`/dev/rdiskN` numbers change** when the drive re-enumerates; never cache the
  node, re-read it (`bluforge devices`).
- **Do not unmount unless the backend needs it**: libaacs does, libmmbd does not,
  and remounting a fragile drive can itself be slow.
- The `output is shorter than the title - disc read error?` warning fires on a
  genuine dropped read, and can also fire spuriously on a title whose PTS is
  compressed. Check the frame count, not just the warning.

Reference disc verified end to end: `HOUSE_OF_THE_DRAGON_S1_D1` (UHD, MKBv77,
bus-encryption flag set), ripped with zero MakeMKV.

## Not built / declined

- **Native AGID drive authentication** (extract MakeMKV's host cert + device
  keys, or a firmware/LibreDrive vendor unlock) so an unkeyed disc decrypts with
  no external key source at all. This is the remaining gap.
- **`keys from-makemkv`**: parse MakeMKV's `_private_data.tar` `hkd_*.bin` key
  cache. The format is opaque in practice; the community KEYDB is the practical
  source.

# Decryption notes (AACS / BD+)

How Keepsake Atelier decrypts, what is still not built, and the traps seen in practice.
Keep this in sync with `core/src/disc.rs` and `core/src/keys.rs`.

## The problem splits in two

AACS has two independent halves:

1. **Drive authentication** to obtain the 16-byte volume ID. Needs either a
   licensed host certificate with device keys, or LibreDrive (vendor-mode unlock
   or patched firmware). This is a credential, not an algorithm.
2. **Key derivation and AES decryption.** Disc ID is `SHA-1(AACS/Unit_Key_RO.inf)`.
   The VUK yields unit/title keys that drive AES-128-CBC over 6144-byte units
   (first 16 bytes clear, address-derived IV). Standard published math.

Keepsake Atelier does both:
1. through the drive's **LibreDrive vendor read**, which returns the already
   decrypted VID with no certificate and no `SEND KEY`/`REPORT KEY`
   (see "The VID, solved natively" below), with host-certificate
   authentication as the fallback for a drive that is not LibreDrive-enabled;
2. through libaacs, driven by the local `KEYDB.cfg`.

## How Keepsake Atelier decrypts today

- **Native libaacs is the default** backend. It reads the disc's VUK and unit
  key from `KEYDB.cfg` and never touches the drive's MMC, so role 1 is moot for a
  keyed disc.
- An **unkeyed** disc is keyed first, natively: `plan_backend` reads the VID
  from the drive, recovers the media key from the disc's own MKB with the KEYDB
  device keys, derives the VUK, and stores the entry. The rip then proceeds on
  libaacs like any keyed disc.
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
- MakeMKV's `libmmbd` is the last-resort fallback: a disc that still cannot be
  keyed, or BD+ (Homebrew's `libbdplus` ships no BD+ VM config). Force either
  backend with `KEEPSAKE_DECRYPT=aacs|mmbd`. `keepsake-atelier libs` / `disc` print the
  choice.

libaacs's own macOS MMC layer cannot even find the device ("Could not find
matching IOBDServices mounted @ /dev/rdiskN"), so it is driven entirely from
KEYDB. On the reference disc its output was the same size, and decoded
identically, to libmmbd's.

## Key store

`keepsake-atelier keys add|list|show|remove|import` manages `KEYDB.cfg` line entries
(`0x<discid> = <name> | D | <date> | M | ... | I | ... | V | <vuk> | U | <n>-<keys>`).
Disc id is `SHA-1(AACS/Unit_Key_RO.inf)`; `--from <mount>` derives it. The store
is what "retires MakeMKV": once a disc is keyed it rips natively and offline.

Back up `KEYDB.cfg` before bulk edits. `keys update` merges rather than
replaces, so an entry you added that the community database does not have yet
is preserved on refresh.

## Getting a key for a new disc

Preferred: derive it from the disc itself. No MakeMKV, no external oracle.

```sh
keepsake-atelier keys native --from /Volumes/MY_DISC   # read VID, derive, store
keepsake-atelier keys native --print                   # print only (default: mounted disc)
```

That runs the native path: the VID comes from the drive (LibreDrive vendor read,
or host-certificate authentication if the vendor read is unavailable), the media
key from the disc's own MKB and the KEYDB device keys, then VUK = AES-G(MK, VID).
The result is written to KEYDB, so the disc then rips with no MakeMKV.

On a LibreDrive drive this needs **no certificate at all**. On a non-LibreDrive
drive whose revocation list has rejected every published certificate, fall back
to an external oracle and store the key by hand:

```sh
keepsake-atelier keys add --from /Volumes/MY_DISC --vuk 0x<VUK> [--unit-key 0x<UK>]
```

## AACS2 / UHD unit keys

An AACS1 disc decrypts from the VUK. An AACS2 (UHD) disc is organised in units
(usually 6144-byte sectors) and needs the unit-key table in addition:

```
keepsake-atelier keys show --from /Volumes/UHD_DISC
keepsake-atelier keys add --from /Volumes/UHD_DISC --unit-key 0x<UK> ...
```

`keys show` prints the disc id, media key, volume id, VUK and every unit key
(`--json` for the raw entry). The app's keys panel shows the same and has a
unit-key editor (one hex key per line, saved in index order).

Unit keys are **derived**, not imported: the disc's `AACS/Unit_Key_RO.inf`
stores them encrypted, and each one is AES-128-ECB decrypted with the VUK
(AACS2 records are 64 bytes apart, AACS1 48; `AACS/Content000.cer[0] == 0x10`
marks AACS2). `keys native` and `keys from-makemkv` therefore write the `U`
table into KEYDB as part of the normal key recovery, with no extra tool.

## BD+ discs

BD+ is separate from AACS. libbdplus runs the BD+ VM but ships no VM data:

```sh
keepsake-atelier bdplus --install
```

unpacks the public `bdplus-vm0.bz2` into libbdplus's config dir; BD+ discs then
decrypt through libbdplus, no MakeMKV.

## The VID, solved natively (LibreDrive vendor read)

The standard `READ DISC STRUCTURE (0x80)` cannot supply the VID on this drive:
with an allocated AGID it returns SCSI `05/6F/02` (authentication required), and
it returns the same after MakeMKV has put the drive into LibreDrive mode, so
LibreDrive does not widen the standard command.

The actual mechanism was captured by launching `makemkvcon` **under lldb** (a
launch, not an attach, so no root and no SIP change) and breaking on
`SCSITaskClass::SetCommandDescriptorBlock` in Apple's `SCSITaskLib` plugin. The
complete drive conversation during a backup:

| opcode | command | count |
| --- | --- | --- |
| `0x12` | INQUIRY | 22 |
| `0x25` / `0x28` | READ CAPACITY / READ(10) | 8 / 250 |
| `0x3c` | **READ BUFFER** | 1050 |
| `0x46` | GET CONFIGURATION | 44 |
| `0x51` / `0x43` | MODE SENSE | 16 |
| `0xad` | READ DISC STRUCTURE | 36 |
| `0xbb` | SET CD SPEED | 12 |
| `0xa3` / `0xa4` | SEND KEY / REPORT KEY | **0 / 0** |

Zero `SEND KEY` and zero `REPORT KEY`, and no writes of any kind. The VID comes
from a vendor read:

    READ BUFFER (0x3c), mode 1, buffer id 0x4410, 36 bytes
      -> 00 22 | 00 50 | <16-byte VID> | 16 zero bytes
         ^len    ^type

Reading the whole buffer (9216 bytes) yields 256 identical records; the drive
ignores the buffer offset and returns the first record for any offset. So the
LibreDrive firmware decrypts the VID itself and serves it over READ BUFFER, which
is why revoked host certificates and `SEND KEY` never matter to MakeMKV.

`bf_aacs_vid_libredrive()` in `core/src/aacs_mmc.c` issues exactly that read.
`bf_native_key()` prefers it and only falls back to host-certificate
authentication when the vendor read fails. That closes the last MakeMKV
dependency: `keepsake-atelier keys native` needs no certificate and never runs
`makemkvcon`. Verified on the Danganronpa disc: the VID recovered this way is
`BA4E09AE31DE87E680D05E229E03F4AB`, matching KEYDB exactly.

## Fallback: MakeMKV's `discatt.dat` (non-LibreDrive drives)

Kept for a drive without LibreDrive firmware. MakeMKV writes what it read into
`discatt.dat`:

    04 00 00 80 00 00 00 10 <16-byte VID>
    04 00 00 84 00 00 00 20 <32-byte RDK>   (only if bus encryption / no LibreDrive)
    01 03 00 38 00 00 00 5C <92-byte drive cert>

The MK needs **no** host certificate (device keys + MKB, offline), and
`VUK = AES-G(MK, VID)`. So MakeMKV only has to supply the VID.

    keepsake-atelier keys from-makemkv [--from <mount>] [--print]

- runs `makemkvcon backup` just far enough to get `discatt.dat`, then kills it
  (`SDF_STOP` is set so MakeMKV does not switch the drive into LibreDrive, which
  would hide the RDK and can leave the drive needing a re-insert);
- derives the media key from the disc's own MKB with the KEYDB device keys;
- computes the VUK and writes `I/M/V` into `KEYDB.cfg`;
- caches `vid/<discid>` and `rdk/<sha1(drivecert)>/<discid>` where libaacs looks
  for them (the RDK is what lets libaacs read bus-encrypted/UHD discs without an
  HC).

`plan_backend` tries this only after the native path fails, so an unkeyed disc
still ends up on libaacs (MakeMKV is used once, per disc, never as the
decryptor).

### What this replaces

MakeMKV does four things; all four are now ours:

- **Media key**: ours (device keys + MKB, offline). MakeMKV not involved.
- **VUK**: ours (`AES-G(MK, VID)`). MakeMKV not involved.
- **RDK / bus encryption**: ours (read from `discatt.dat` when a drive needs it,
  cached where libaacs looks).
- **VID**: ours, from the drive's LibreDrive vendor READ BUFFER (or host
  authentication when the drive is not LibreDrive-enabled).

MakeMKV does **not** ship a host certificate. Its own log says
`Using LibreDrive mode (v06.3 ...)`, and the captured CDBs confirm it never sends
`SEND KEY`/`REPORT KEY`. That is also why scanning its process for a 92-byte
AACS host cert finds nothing that verifies, and why its `sdf.bin`
(raw-deflate + preset dictionary) holds only drive-tool scripts.

`makemkvcon` is now only a last-resort oracle for a non-LibreDrive drive, and
`KEEPSAKE_HOST_CERT` / `KEEPSAKE_HOST_PRIV` let a licensed certificate replace
even that.

## Traps seen in practice

- **The drive drops off the USB bus** under sustained reads (HL-DT-ST BD-RE
  BU40N in a bus-powered Archgon enclosure). Symptom: the device disappears
  (`drutil` reports "No Media Inserted", no `/dev/disk` node). It needs a
  physical replug; a powered hub is the real fix.
- **The first rip after a replug tends to fail** with `Unable decrypt unit`
  (libaacs) and then succeeds on retry. `keepsake-atelier rip` now re-runs the whole rip
  on failure by default (`--retries`, default 2), so this is handled without a
  manual second command.
- **`/dev/rdiskN` numbers change** when the drive re-enumerates; never cache the
  node, re-read it (`keepsake-atelier devices`).
- **A disc read right after authentication needs a moment.** Our MMC auth takes
  exclusive access, and on release the USB drive re-enumerates and the media
  briefly disappears. The native path remounts and waits for the volume to
  return before handing off.
- **libaacs must be handed a *mounted* path.** It resolves the BSD device from
  `statfs` and then unmounts/remounts around exclusive access itself. Handing it
  `/dev/rdiskN` (or a pre-unmounted volume) is why it reported "Could not find
  matching IOBDServices". libmmbd is the opposite: it wants the raw device with
  the volume unmounted.
- The `output is shorter than the title - disc read error?` warning fires on a
  genuine dropped read, and can also fire spuriously on a title whose PTS is
  compressed. Check the frame count, not just the warning.

Reference disc verified end to end: `HOUSE_OF_THE_DRAGON_S1_D1` (UHD, MKBv77,
bus-encryption flag set), ripped with zero MakeMKV.

## Not built / declined

- **AACS2/UHD from scratch.** AACS1 derives MK -> VUK and is handled; the AACS2
  MKB type and the read-data-key path are not. A UHD disc not in KEYDB still
  needs an external oracle.
- **`keys from-makemkv`**: parse MakeMKV's `_private_data.tar` (`hkd_*.bin` is a
  private container; the community KEYDB is the practical source).
- Vendor firmware / LibreDrive unlock is not needed given the above.

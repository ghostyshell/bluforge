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

Preferred: derive it from the disc itself. No MakeMKV, no external oracle.

```sh
bluforge keys native --from /Volumes/MY_DISC   # authenticate, derive, store
bluforge keys native --print                   # print only (default: mounted disc)
```

That runs the native path: drive authentication (AGID + host certificate) to
read the volume id, MKB -> media key using the KEYDB device keys, then media key
+ volume id -> VUK. The result is written to KEYDB, so the disc then rips with
no MakeMKV.

If the drive rejects the shipped host certificate (below), fall back to an
external oracle and store the key by hand:

```sh
bluforge keys add --from /Volumes/MY_DISC --vuk 0x<VUK> [--unit-key 0x<UK>]
```

## BD+ discs

BD+ is separate from AACS. libbdplus runs the BD+ VM but ships no VM data:

```sh
bluforge bdplus --install
```

unpacks the public `bdplus-vm0.bz2` into libbdplus's config dir; BD+ discs then
decrypt through libbdplus, no MakeMKV.

## Known limit: the host certificate is revoked on this drive

Authentication needs a host certificate + private key, carried in the community
KEYDB header (`| HC |`). The public ones are old: the shipped one is revoked in
MKBv72, and the newest published one (`0xffff80000217`) in MKBv82. This drive is
bus-encryption-capable and has already seen an AACS2 MKBv77 disc (a UHD rip), so
its host revocation list rejects **both** — native auth currently returns SCSI
`05/6F/00` at the `SEND KEY` step and the rip falls back to libmmbd.

So on this drive, keyless AACS1 decryption still needs a key; native auth works
mechanically and will succeed once a host cert newer than the drive's revocation
list is available (or on a drive that has not seen a v72+/AACS2 v77 disc). Until
then: `bluforge keys native` for the derivation attempt, and libmmbd as the
fallback (`plan_backend` does both automatically).

We now print the SCSI sense on auth failure, so the reason is never a guess.

## Bypass: recover the key from MakeMKV's VID

MakeMKV ships its own (unrevoked) host certificate and, during a backup, writes
what it authenticated for into `discatt.dat`:

   04 00 00 80 00 00 00 10 <16-byte VID>
   04 00 00 84 00 00 00 20 <32-byte RDK>   (only if bus encryption / no LibreDrive)
   01 03 00 38 00 00 00 5C <92-byte drive cert>

The MK needs **no** host certificate (device keys + MKB, offline), and
`VUK = AES-G(MK, VID)`. So MakeMKV only has to supply the VID.

    bluforge keys from-makemkv [--from <mount>] [--print]

- runs `makemkvcon backup` just far enough to get `discatt.dat`, then kills it
  (`SDF_STOP` is set so MakeMKV does not switch the drive into LibreDrive, which
  would hide the RDK and can leave the drive needing a re-insert);
- derives the media key from the disc's own MKB with the KEYDB device keys;
- computes the VUK and writes `I/M/V` into `KEYDB.cfg`;
- caches `vid/<discid>` and `rdk/<sha1(drivecert)>/<discid>` where libaacs looks
  for them (the RDK is what lets libaacs read bus-encrypted/UHD discs without an
  HC).

`plan_backend` tries this automatically after native auth fails, so an unkeyed
disc still ends up on libaacs (MakeMKV is used once, per disc, never as the
decryptor). Verified on the Danganronpa disc: the VID/MK/VUK recovered this way
match the reference values exactly.

### What this replaces, and the one thing it does not

MakeMKV does four things; three are now ours:

- **Media key** — ours (device keys + MKB, offline). MakeMKV not involved.
- **VUK** — ours (`AES-G(MK, VID)`). MakeMKV not involved.
- **RDK / bus encryption** — ours (read from `discatt.dat`, cached where libaacs
  looks; `SDF_STOP` keeps MakeMKV from hiding it by enabling LibreDrive).
- **VID** — still needs a non-revoked host certificate to authenticate. MakeMKV
  rides on *its own* certificate here, and that certificate is not extractable
  from the binaries as raw bytes: MakeMKV obfuscates its key tables (neither the
  public device keys nor the host cert appear verbatim, and no single-byte-XOR
  variant does either). So `makemkvcon` is kept purely as a VID oracle.

If/when a host cert newer than this drive's revocation list is available, put it
in the KEYDB header and `bluforge keys native` derives everything itself and even
`makemkvcon` goes away.

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

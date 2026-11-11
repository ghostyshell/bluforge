# Keys and AACS

## What the app needs

A Blu-ray disc can be AACS encrypted. Decryption needs a disc key, which is
either in a KEYDB entry or derived from the disc with the drive's volume ID.

```sh
keepsake-atelier keys status
keepsake-atelier plan
```

`plan` explains the backend for the inserted disc: native, MakeMKV fallback, or
libaacs with a KEYDB entry.

## KEYDB

The store is a standard `KEYDB.cfg` in the data directory. The app can import a
store, search it and add entries manually.

```sh
keepsake-atelier keys import /path/to/KEYDB.cfg
keepsake-atelier keys search "THE MATRIX"
keepsake-atelier keys show 0x0123456789ABCDEF
keepsake-atelier keys remove 0x0123456789ABCDEF
```

The file stays local. It is never uploaded.

## Native recovery

```sh
keepsake-atelier keys native
```

The command reads the volume ID from the drive, derives the volume unique key
from the disc's MKB, and stores the result. No MakeMKV required.

The volume ID step uses the LibreDrive vendor READ BUFFER when the firmware
provides it. If it does not, a host certificate from KEYDB is used for the
standard AACS authentication. The drive transport is IOKit on macOS, `SG_IO` on
Linux and `IOCTL_SCSI_PASS_THROUGH_DIRECT` on Windows.

If neither the vendor read nor a usable host certificate works, you get an
error such as:

```
native: no LibreDrive VID record in buffer 0x4410
native: drive authentication failed
```

That is a firmware limitation, not a key error. A LibreDrive-patched firmware
fixes it. Until then, libaacs with a KEYDB entry still decrypts normally.

## AACS2 and UHD

For AACS2 discs, the app derives unit keys offline from
`AACS/Unit_Key_RO.inf` with the VUK. AACS2 is detected by the first byte of
`Content000.cer` (0x10). The derived keys are stored in the same KEYDB format.

## MakeMKV dumps

If you already use MakeMKV, its dumps can be imported:

```sh
keepsake-atelier keys from-makemkv /path/to/dump
```

This is a fallback. It is not required, and the app does not call MakeMKV.

## Troubleshooting

- **Disc key present but playback fails**: check `plan` and make sure libaacs is
  using the same KEYDB (`keepsake-atelier libs`).
- **Volume not mounted**: mount the disc in Finder. The drive may be busy.
- **Disc ID mismatch**: the KEYDB entry may be for a different pressing. Search
  by label or add the key from a dump.
- **AACS2 unit key errors**: confirm the VUK entry is present and the disc has a
  readable `Unit_Key_RO.inf`.

# Vendored libaacs subset

`crypto.c`, `ec.c`, `mkb.c`, `util/strutl.c` and the headers are from
libaacs 0.11.1 (LGPL-2.1-or-later, see `COPYING`). They are compiled into
`keepsake-core` on Linux and Windows so native AACS key derivation does not
depend on libaacs shipping its internal, non-exported symbols.

Only the MKB parser and the AES/EC helpers are used. Logging is a no-op stub.
On macOS the Homebrew static `libaacs.a` is still linked, so this copy is not
compiled there.

// Native AACS: read the volume id from the drive, then derive the media key
// and VUK with the portable crypto half in aacs_key.c.
//
// The macOS build keeps the IOKit MMC layer below. Linux and Windows use the
// Rust SCSI transport in scsi.rs and call bf_media_key/bf_aesg directly.

#include "aacs_key.h"

#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern int bf_aacs_auth_vid(const char *bsd_name, const unsigned char *host_priv,
                            const unsigned char *host_cert, unsigned char out[16]);
extern int bf_aacs_vid_libredrive(const char *bsd_name, unsigned char out[16]);

// Returns BF_OK and fills out_vid/out_mk/out_vuk (16 bytes each).
int bf_native_key(const char *bsd_name,
                  const uint8_t *host_priv, const uint8_t *host_cert,
                  const uint8_t *mkb_data, size_t mkb_len,
                  const bf_dk_t *dks, int ndks,
                  uint8_t *out_vid, uint8_t *out_mk, uint8_t *out_vuk)
{
    uint8_t vid[16], mk[16];
    int rc;

    if (!bsd_name || !mkb_data || mkb_len < 16) {
        return BF_ERR_MKB;
    }

    // Preferred: the drive's LibreDrive firmware returns the decrypted VID over
    // vendor READ BUFFER, no AACS authentication required.
    rc = bf_aacs_vid_libredrive(bsd_name, vid);
    if (rc) {
        if (!host_priv || !host_cert) {
            fprintf(stderr, "native: no LibreDrive vendor VID and no host certificate\n");
            return BF_ERR_AUTH;
        }
        rc = bf_aacs_auth_vid(bsd_name, host_priv, host_cert, vid);
        if (rc) {
            fprintf(stderr, "native: drive authentication failed (rc=%d)\n", rc);
            return BF_ERR_AUTH;
        }
    }

    rc = bf_media_key(mkb_data, mkb_len, dks, ndks, mk);
    if (rc) {
        fprintf(stderr, "native: media key derivation failed (rc=%d)\n", rc);
        return rc;
    }

    memcpy(out_vid, vid, 16);
    memcpy(out_mk, mk, 16);
    bf_aesg(mk, vid, out_vuk);
    return BF_OK;
}

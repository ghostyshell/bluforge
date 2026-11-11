// Portable AACS key derivation. See aacs_key.h.
#include "aacs_key.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct mkb MKB;

extern int crypto_init(void);
extern int crypto_aes128d(const uint8_t *key, const uint8_t *data, uint8_t *dst);
extern int crypto_aesg3(const uint8_t *D, uint8_t *lsubk, uint8_t *rsubk, uint8_t *pk);
extern MKB *mkb_init(uint8_t *data, size_t len);
extern void mkb_close(MKB *mkb);
extern const uint8_t *mkb_subdiff_records(MKB *mkb, size_t *len);
extern const uint8_t *mkb_cvalues(MKB *mkb, size_t *len);
extern const uint8_t *mkb_mk_dv(MKB *mkb);

static uint32_t be32(const uint8_t *p)
{
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) | ((uint32_t)p[2] << 8) | p[3];
}

static uint32_t v_mask_of(uint32_t uv)
{
    uint32_t v = 0xffffffffu;
    while (!(uv & ~v)) {
        v <<= 1;
    }
    return v;
}

// walk a device key down the subset tree to a processing key (libaacs mk.c).
static int calc_pk(const uint8_t *dk, uint8_t *pk, uint32_t uv, uint32_t v_mask, uint32_t dev_key_v_mask)
{
    uint8_t l[16], r[16];
    if (crypto_aesg3(dk, l, r, pk)) {
        return -1;
    }
    while (dev_key_v_mask != v_mask) {
        int i;
        uint8_t cur[16];
        for (i = 31; i >= 0; i--) {
            if (!(dev_key_v_mask & (1u << i))) {
                break;
            }
        }
        if (i < 0 || !(uv & (1u << i))) {
            memcpy(cur, l, 16);
        } else {
            memcpy(cur, r, 16);
        }
        if (crypto_aesg3(cur, l, r, pk)) {
            return -1;
        }
        dev_key_v_mask = ((int32_t) dev_key_v_mask) >> 1;
    }
    return 0;
}

// aes128d(pk, cvalue) -> mk; mk[12..16] ^= uv; aes128d(mk, mk_dv) must equal VV.
static int validate_pk(const uint8_t *pk, const uint8_t *cvalue, const uint8_t *uv,
                       const uint8_t *mk_dv, uint8_t *mk)
{
    static const uint8_t vv[8] = { 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF };
    uint8_t dec[16];
    int a;

    if (crypto_aes128d(pk, cvalue, mk)) {
        return -1;
    }
    for (a = 0; a < 4; a++) {
        mk[a + 12] ^= uv[a];
    }
    if (crypto_aes128d(mk, mk_dv, dec)) {
        return -1;
    }
    return memcmp(dec, vv, 8) == 0 ? 0 : -1;
}

static int find_dk(const bf_dk_t *dks, int n, uint32_t node, uint32_t uv, uint32_t u_mask,
                   const bf_dk_t **out, uint32_t *out_vmask)
{
    int i;
    for (i = 0; i < n; i++) {
        uint32_t dk_u_mask, dk_v_mask;
        if (dks[i].node != node || !dks[i].uv) {
            continue;
        }
        dk_u_mask = 0xffffffffu << dks[i].u_shift;
        dk_v_mask = v_mask_of(dks[i].uv);
        if (u_mask == dk_u_mask && (uv & dk_v_mask) == (dks[i].uv & dk_v_mask)) {
            *out = &dks[i];
            *out_vmask = dk_v_mask;
            return 0;
        }
    }
    return -1;
}

static int media_key(MKB *m, const bf_dk_t *dks, int n, uint8_t *mk)
{
    const uint8_t *uvs, *cvalues, *mk_dv;
    size_t uvs_len = 0, cvalues_len = 0;
    unsigned num_uvs, idx;

    uvs = mkb_subdiff_records(m, &uvs_len);
    cvalues = mkb_cvalues(m, &cvalues_len);
    mk_dv = mkb_mk_dv(m);
    if (!uvs || !cvalues || !mk_dv) {
        return BF_ERR_MKB;
    }

    // uvs data = 1 leading byte + num_uvs * 5 bytes.
    if (uvs_len < 6) {
        return BF_ERR_MKB;
    }
    num_uvs = (unsigned)((uvs_len - 1) / 5);

    {
        int di;
        uint32_t last_node = 0xffffffffu;
        for (di = 0; di < n; di++) {
            uint32_t u_mask = 0, v_mask = 0, uv = 0;
            const bf_dk_t *dk;
            uint32_t dv_mask;
            uint8_t pk[16];

            if (dks[di].node == last_node) {
                continue;
            }
            last_node = dks[di].node;

            for (idx = 0; idx < num_uvs; idx++) {
                const uint8_t *p = uvs + 1 + 5 * idx;
                uint8_t shift = p[-1];
                uint32_t u = be32(p);
                if (!u) {
                    continue;
                }
                if (shift & 0xc0) { // revoked device
                    idx = num_uvs;
                    break;
                }
                u_mask = 0xffffffffu << shift;
                v_mask = v_mask_of(u);
                if (((dks[di].node & u_mask) == (u & u_mask)) &&
                    ((dks[di].node & v_mask) != (u & v_mask))) {
                    uv = u;
                    break;
                }
            }
            if (idx >= num_uvs || !uv) {
                continue;
            }
            if (find_dk(dks, n, dks[di].node, uv, u_mask, &dk, &dv_mask)) {
                continue;
            }
            if (calc_pk(dk->key, pk, uv, v_mask, dv_mask)) {
                continue;
            }
            if (validate_pk(pk, cvalues + idx * 16, uvs + 1 + 5 * idx, mk_dv, mk) == 0) {
                return BF_OK;
            }
        }
    }
    return BF_ERR_NOKEY;
}

// Media key from the MKB using device keys only (no drive, no host cert).
int bf_media_key(const uint8_t *mkb_data, size_t mkb_len, const bf_dk_t *dks, int ndks,
                 uint8_t out_mk[16])
{
    uint8_t *buf;
    MKB *m;
    int rc;
    if (!mkb_data || mkb_len < 16) {
        return BF_ERR_MKB;
    }
    crypto_init();
    buf = malloc(mkb_len);
    if (!buf) {
        return BF_ERR_MKB;
    }
    memcpy(buf, mkb_data, mkb_len);
    m = mkb_init(buf, mkb_len);
    if (!m) {
        free(buf);
        return BF_ERR_MKB;
    }
    rc = media_key(m, dks, ndks, out_mk);
    mkb_close(m);
    return rc;
}

// AES-G (AACS): AES-128-ECB decrypt of `data` under `key`, XOR `data`.
void bf_aesg(const uint8_t *key, const uint8_t *data, uint8_t out[16])
{
    int i;
    if (crypto_aes128d(key, data, out)) {
        return;
    }
    for (i = 0; i < 16; i++) {
        out[i] ^= data[i];
    }
}


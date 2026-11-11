// Portable AACS key derivation: MKB parsing, media key, AES-G.
//
// This is the crypto half of the native path. It has no OS dependencies, so
// macOS links it against Homebrew's static libaacs while Linux and Windows
// link it against the vendored libaacs crypto/MKB subset.

#ifndef KEEPSAKE_AACS_KEY_H
#define KEEPSAKE_AACS_KEY_H

#include <stddef.h>
#include <stdint.h>

typedef struct {
    uint8_t key[16];
    uint32_t node;
    uint32_t uv;
    uint8_t u_shift;
} bf_dk_t;

#if defined(_WIN32) && defined(BF_AACS_DLL)
#define BF_API __declspec(dllexport)
#else
#define BF_API
#endif

#define BF_OK 0
#define BF_ERR_AUTH 1
#define BF_ERR_MKB 2
#define BF_ERR_NOKEY 3

BF_API int bf_media_key(const uint8_t *mkb_data, size_t mkb_len,
                        const bf_dk_t *dks, int ndks, uint8_t out_mk[16]);
BF_API void bf_aesg(const uint8_t *key, const uint8_t *data, uint8_t out[16]);

#endif

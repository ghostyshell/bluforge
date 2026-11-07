// Thin C shim over libbluray so Rust does not have to mirror the (large,
// version-sensitive) BLURAY_TITLE_INFO / BLURAY_STREAM_INFO struct layouts.
#include <libbluray/bluray.h>
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    uint32_t index;
    uint32_t playlist;
    uint64_t duration_90k;
    uint32_t chapter_count;
    uint32_t clip_count;
    uint8_t  video_format;   /* bd_video_format_e, 0 if none */
    uint8_t  video_rate;     /* bd_video_rate_e */
    uint8_t  dynamic_range;  /* bd_dynamic_range_type_e */
    uint8_t  audio_count;
    int      is_main;
} bf_title_t;

struct bf_disc { BLURAY *bd; };
typedef struct bf_disc bf_disc_t;

bf_disc_t *bf_open(const char *path, const char *keyfile) {
    BLURAY *bd = bd_open(path, keyfile);
    if (!bd) return NULL;
    bf_disc_t *d = (bf_disc_t *)calloc(1, sizeof(bf_disc_t));
    if (!d) { bd_close(bd); return NULL; }
    d->bd = bd;
    return d;
}

void bf_close(bf_disc_t *d) {
    if (!d) return;
    bd_close(d->bd);
    free(d);
}

int bf_title_count(bf_disc_t *d) {
    return (int)bd_get_titles(d->bd, 0, 0);
}

int bf_main_title(bf_disc_t *d) {
    return (int)bd_get_main_title(d->bd);
}

/* Fill up to max titles, return count written. */
int bf_titles(bf_disc_t *d, bf_title_t *out, int max) {
    uint32_t n = bd_get_titles(d->bd, 0, 0);
    int main_idx = (int)bd_get_main_title(d->bd);
    int c = 0;
    for (uint32_t i = 0; i < n && c < max; i++) {
        BLURAY_TITLE_INFO *ti = bd_get_title_info(d->bd, i, 0);
        if (!ti) continue;
        memset(&out[c], 0, sizeof(out[c]));
        out[c].index = i;
        out[c].playlist = ti->playlist;
        out[c].duration_90k = ti->duration;
        out[c].chapter_count = ti->chapter_count;
        out[c].clip_count = ti->clip_count;
        if (ti->clip_count > 0) {
            const BLURAY_CLIP_INFO *cl = &ti->clips[0];
            out[c].audio_count = cl->audio_stream_count;
            if (cl->video_stream_count > 0) {
                out[c].video_format = cl->video_streams[0].format;
                out[c].video_rate = cl->video_streams[0].rate;
                out[c].dynamic_range = cl->video_streams[0].dynamic_range_type;
            }
        }
        out[c].is_main = (i == (uint32_t)main_idx) ? 1 : 0;
        c++;
        bd_free_title_info(ti);
    }
    return c;
}

const char *bf_disc_name(bf_disc_t *d) {
    const BLURAY_DISC_INFO *di = bd_get_disc_info(d->bd);
    if (!di) return "";
    if (di->disc_name) return di->disc_name;
    if (di->udf_volume_id) return di->udf_volume_id;
    return "";
}

int bf_has_aacs(bf_disc_t *d) {
    const BLURAY_DISC_INFO *di = bd_get_disc_info(d->bd);
    return di ? (di->aacs_detected | di->bdplus_detected) : 0;
}

/* 1 = AACS present and a usable decryption lib was found */
int bf_aacs_ready(bf_disc_t *d) {
    const BLURAY_DISC_INFO *di = bd_get_disc_info(d->bd);
    if (!di) return 0;
    if (di->aacs_detected && !di->libaacs_detected) return 0;
    return 1;
}

int bf_aacs_error(bf_disc_t *d) {
    const BLURAY_DISC_INFO *di = bd_get_disc_info(d->bd);
    return di ? di->aacs_error_code : 0;
}

/* 1 = disc detected */
int bf_detected(bf_disc_t *d) {
    const BLURAY_DISC_INFO *di = bd_get_disc_info(d->bd);
    return di ? di->bluray_detected : 0;
}

int bf_select_title(bf_disc_t *d, uint32_t title) {
    return bd_select_title(d->bd, title);
}

int bf_read(bf_disc_t *d, uint8_t *buf, int len) {
    return bd_read(d->bd, buf, len);
}

const char *bf_version(void) {
    static char v[32];
    int a = 0, b = 0, c = 0;
    bd_get_version(&a, &b, &c);
    snprintf(v, sizeof(v), "%d.%d.%d", a, b, c);
    return v;
}

// Thin C shim over libbluray so Rust does not have to mirror the (large,
// version-sensitive) BLURAY_TITLE_INFO / BLURAY_STREAM_INFO struct layouts.
#include <libbluray/bluray.h>
#include <libbluray/filesystem.h>
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/stat.h>
#include <unistd.h>

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

typedef struct {
    double   start_secs;
    uint32_t duration_90k;
    char     name[128];   /* empty when the playlist carries no chapter name */
} bf_chapter_t;

typedef struct {
    int     kind;         /* 0 = video, 1 = audio, 2 = PG subtitle */
    char    lang[4];      /* ISO 639-2, "" when unset */
    uint8_t coding_type;  /* BD_*_CODING_TYPE */
    uint8_t format;       /* bd_video_format_e / bd_audio_format_e */
    uint8_t rate;
    uint16_t pid;         /* mpeg-ts PID (used to dedupe view streams) */
} bf_stream_t;

/* Chapter entry marks (times + any authored names) of one title. */
int bf_title_chapters(bf_disc_t *d, uint32_t title, bf_chapter_t *out, int max) {
    BLURAY_TITLE_INFO *ti = bd_get_title_info(d->bd, title, 0);
    if (!ti) return 0;
    int c = 0;
    for (uint32_t i = 0; i < ti->chapter_count && c < max; i++) {
        const BLURAY_TITLE_CHAPTER *ch = &ti->chapters[i];
        memset(&out[c], 0, sizeof(out[c]));
        out[c].start_secs = ch->start / 90000.0;
        out[c].duration_90k = ch->duration;
        if (ch->chapter_name) {
            snprintf(out[c].name, sizeof(out[c].name), "%s", ch->chapter_name);
        }
        c++;
    }
    bd_free_title_info(ti);
    return c;
}

static void bf_put_streams(const BLURAY_STREAM_INFO *src, int n, int kind,
                           bf_stream_t *out, int *c, int max) {
    for (int i = 0; i < n && *c < max; i++) {
        memset(&out[*c], 0, sizeof(out[*c]));
        out[*c].kind = kind;
        memcpy(out[*c].lang, src[i].lang, sizeof(out[*c].lang) - 1);
        out[*c].coding_type = src[i].coding_type;
        out[*c].format = src[i].format;
        out[*c].rate = src[i].rate;
        out[*c].pid = src[i].pid;
        (*c)++;
    }
}

/* As bf_put_streams, but skips PIDs that are already listed (eg. a Dolby Vision
   view stream also reported as a secondary video stream). */
static void bf_put_streams_uniq(const BLURAY_STREAM_INFO *src, int n, int kind,
                                bf_stream_t *out, int *c, int max) {
    for (int i = 0; i < n && *c < max; i++) {
        int dup = 0;
        for (int j = 0; j < *c; j++) {
            if (out[j].pid == src[i].pid) {
                dup = 1;
                break;
            }
        }
        if (!dup) {
            bf_put_streams(&src[i], 1, kind, out, c, max);
        }
    }
}

/* Video, audio and PG subtitle streams of one title, in disc order. */
int bf_title_streams(bf_disc_t *d, uint32_t title, bf_stream_t *out, int max) {
    BLURAY_TITLE_INFO *ti = bd_get_title_info(d->bd, title, 0);
    if (!ti) return 0;
    int c = 0;
    if (ti->clip_count > 0) {
        const BLURAY_CLIP_INFO *cl = &ti->clips[0];
        /* Primary video, then any Dolby Vision / secondary video view (same
           PID order ffmpeg will use), deduped by PID, then audio and subs. */
        bf_put_streams(cl->video_streams, cl->video_stream_count, 0, out, &c, max);
        bf_put_streams_uniq(cl->sec_video_streams, cl->sec_video_stream_count, 0, out, &c, max);
        bf_put_streams_uniq(cl->dv_streams, cl->dv_stream_count, 0, out, &c, max);
        bf_put_streams(cl->audio_streams, cl->audio_stream_count, 1, out, &c, max);
        bf_put_streams(cl->pg_streams, cl->pg_stream_count, 2, out, &c, max);
    }
    bd_free_title_info(ti);
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

/* ------------------------------------------------------------------ */
/* Decrypted tree export (for building a decrypted ISO / backup folder) */
/* ------------------------------------------------------------------ */

static int bf_copy_file_dec(bf_disc_t *d, const char *path, const char *dest) {
    BD_FILE_H *f = bd_open_file_dec(d->bd, path);
    if (!f) { fprintf(stderr, "bf_export: cannot open %s\n", path); return -1; }
    FILE *o = fopen(dest, "wb");
    if (!o) { fprintf(stderr, "bf_export: cannot create %s\n", dest); f->close(f); return -1; }
    uint8_t *buf = (uint8_t *)malloc(6144);
    int rc = 0;
    int64_t total = 0;
    for (;;) {
        int64_t n = f->read(f, buf, 6144);
        if (n < 0) { fprintf(stderr, "bf_export: read error in %s after %lld bytes\n", path, (long long)total); rc = -1; break; }
        if (n == 0) break;
        if (fwrite(buf, 1, (size_t)n, o) != (size_t)n) { fprintf(stderr, "bf_export: write error on %s after %lld bytes\n", dest, (long long)total); rc = -1; break; }
        total += n;
    }
    free(buf);
    fclose(o);
    f->close(f);
    return rc;
}

static int bf_export_dir(bf_disc_t *d, const char *rel, const char *dest,
                         int depth, long *files) {
    BD_DIR_H *dir = bd_open_dir(d->bd, rel);
    if (!dir) { fprintf(stderr, "bf_export: cannot open dir '%s'\n", rel); return -1; }
    BD_DIRENT e;
    int rc = 0;
    while (dir->read(dir, &e) == 0) {
        if (!strcmp(e.d_name, ".") || !strcmp(e.d_name, "..")) continue;
        /* decrypted m2ts must not be AACS-decrypted again on re-open */
        if (depth == 0 && (!strcasecmp(e.d_name, "AACS") || !strcasecmp(e.d_name, "BDSVM")))
            continue;
        char child[1024], dchild[2048];
        if (rel[0]) {
            snprintf(child, sizeof(child), "%s/%s", rel, e.d_name);
        } else {
            snprintf(child, sizeof(child), "%s", e.d_name);
        }
        snprintf(dchild, sizeof(dchild), "%s/%s", dest, e.d_name);
        BD_DIR_H *sub = bd_open_dir(d->bd, child);
        if (sub) {
            sub->close(sub);
            mkdir(dchild, 0755);
            rc |= bf_export_dir(d, child, dchild, depth + 1, files);
        } else {
            if (bf_copy_file_dec(d, child, dchild) != 0) rc = -1;
            (*files)++;
        }
    }
    dir->close(dir);
    return rc;
}

/* Export the whole decrypted disc tree to dest_dir. 0 on success. */
int bf_export_decrypted(bf_disc_t *d, const char *dest_dir, long *out_files) {
    struct stat st;
    if (stat(dest_dir, &st) != 0) mkdir(dest_dir, 0755);

    /* The directory opener is source-dependent: the stdio reader for a mounted
     * volume wants ".", while libbluray's UDF reader (used for ISO images and
     * /dev/disk nodes) needs "" or "/". Probe for one that works. */
    static const char *roots[] = { ".", "", "/", NULL };
    const char *root = NULL;
    for (int i = 0; roots[i]; i++) {
        BD_DIR_H *t = bd_open_dir(d->bd, roots[i]);
        if (t) {
            t->close(t);
            root = roots[i];
            break;
        }
    }
    if (!root) {
        fprintf(stderr, "bf_export: no working root dir opener\n");
        return -1;
    }

    long files = 0;
    int rc = bf_export_dir(d, root, dest_dir, 0, &files);
    if (out_files) *out_files = files;
    return rc;
}


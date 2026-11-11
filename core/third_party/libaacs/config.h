/* Minimal libaacs build configuration for the vendored crypto/MKB subset. */
#ifndef KEEPSAKE_LIBAACS_CONFIG_H
#define KEEPSAKE_LIBAACS_CONFIG_H

#define HAVE_LIBGPG_ERROR 1

#ifndef _WIN32
#define HAVE_PTHREAD_H 1
#define HAVE_SYS_SELECT_H 1
#define HAVE_STRERROR_R 1
#endif

#endif

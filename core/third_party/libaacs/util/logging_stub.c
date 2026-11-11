/* Minimal logging sink for the vendored libaacs crypto/MKB subset. */
#include "logging.h"

uint32_t debug_mask = 0;

void bd_debug(const char *file, int line, uint32_t mask, const char *format, ...)
{
    (void)file; (void)line; (void)mask; (void)format;
}

#ifndef RARFS_SHIM_H
#define RARFS_SHIM_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Returns 1 to continue processing, -1 to abort. */
typedef int (*rarfs_process_cb)(size_t user_data, const unsigned char *data, size_t len);

void *rarfs_open(const char *path, int *err_out);
/* Copies current header's file name into buf (NUL-terminated, truncated to
 * buf_size-1). Returns 0 on success, unrar error code otherwise
 * (10 = ERAR_END_ARCHIVE). */
int rarfs_read_next_name(void *handle, char *buf, size_t buf_size);
/* RARProcessFile with RAR_SKIP for the current header. */
int rarfs_skip_current(void *handle);
/* RARProcessFile with RAR_TEST for the current header; data goes to the cb. */
int rarfs_process_current(void *handle);
void rarfs_set_callback(void *handle, rarfs_process_cb cb, size_t user_data);
int rarfs_close(void *handle);

#ifdef __cplusplus
}
#endif
#endif

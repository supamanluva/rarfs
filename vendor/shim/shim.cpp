#include "shim.h"
#include "../unrarsrc/dll.hpp"
#include <cstdint>
#include <cstring>

struct RarfsArchive {
    HANDLE h;
    rarfs_process_cb cb;
    size_t user_data;
    /* True once the current header's data has been consumed by
     * rarfs_skip_current/rarfs_process_current. RARReadHeaderEx in
     * RAR_OM_EXTRACT mode does not advance past file data, so
     * rarfs_read_next_name must skip an unconsumed file first. */
    bool consumed;
};

static int CALLBACK rarfs_callback(UINT msg, LPARAM user_data, LPARAM p1, LPARAM p2) {
    RarfsArchive *a = reinterpret_cast<RarfsArchive *>(static_cast<uintptr_t>(user_data));
    if (msg == UCM_PROCESSDATA && a && a->cb) {
        return a->cb(a->user_data, reinterpret_cast<const unsigned char *>(p1), static_cast<size_t>(p2));
    }
    return 1;
}

extern "C" {

void *rarfs_open(const char *path, int *err_out) {
    RAROpenArchiveDataEx data;
    std::memset(&data, 0, sizeof(data));
    data.ArcName = const_cast<char *>(path);
    data.OpenMode = RAR_OM_EXTRACT;
    HANDLE h = RAROpenArchiveEx(&data);
    if (err_out) *err_out = data.OpenResult;
    if (!h) return nullptr;
    RarfsArchive *a = new RarfsArchive{h, nullptr, 0, true};
    RARSetCallback(h, rarfs_callback, static_cast<LPARAM>(reinterpret_cast<uintptr_t>(a)));
    return a;
}

int rarfs_read_next_name(void *handle, char *buf, size_t buf_size) {
    RarfsArchive *a = static_cast<RarfsArchive *>(handle);
    if (!a->consumed) {
        int s = RARProcessFile(a->h, RAR_SKIP, nullptr, nullptr);
        if (s != 0) return s;
        a->consumed = true;
    }
    RARHeaderDataEx hd;
    std::memset(&hd, 0, sizeof(hd));
    int r = RARReadHeaderEx(a->h, &hd);
    if (r == 0) {
        a->consumed = false;
        if (buf && buf_size > 0) {
            std::strncpy(buf, hd.FileName, buf_size - 1);
            buf[buf_size - 1] = '\0';
        }
    }
    return r;
}

int rarfs_skip_current(void *handle) {
    RarfsArchive *a = static_cast<RarfsArchive *>(handle);
    int r = RARProcessFile(a->h, RAR_SKIP, nullptr, nullptr);
    if (r == 0) a->consumed = true;
    return r;
}

int rarfs_process_current(void *handle) {
    RarfsArchive *a = static_cast<RarfsArchive *>(handle);
    int r = RARProcessFile(a->h, RAR_TEST, nullptr, nullptr);
    if (r == 0) a->consumed = true;
    return r;
}

void rarfs_set_callback(void *handle, rarfs_process_cb cb, size_t user_data) {
    RarfsArchive *a = static_cast<RarfsArchive *>(handle);
    a->cb = cb;
    a->user_data = user_data;
}

int rarfs_close(void *handle) {
    RarfsArchive *a = static_cast<RarfsArchive *>(handle);
    int r = RARCloseArchive(a->h);
    delete a;
    return r;
}

}

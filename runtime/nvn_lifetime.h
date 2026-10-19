/* Opt-in observation of host reads borrowed from guest resources. No ownership changes. */
#ifndef NVN_LIFETIME_H
#define NVN_LIFETIME_H
#include <stdint.h>
#include <windows.h>
enum {
    NL_TEXTURE_INIT=1, NL_PACKAGED_SOURCE, NL_SNAPSHOT_READY, NL_TEXTURE_FINAL,
    NL_READ_BEGIN, NL_READ_END, NL_POOL_INIT, NL_POOL_FINAL_BEGIN, NL_POOL_FINAL_END,
    NL_RELEASE_BEGIN, NL_RELEASE_END, NL_SNAPSHOT_RELEASE, NL_TEXTURE_REJECTED
};
enum { NL_BORROWED=0, NL_OWNED=1, NL_CAPTURE=2 };
enum { NL_DECOMMIT=1, NL_RELEASE=2, NL_CLEAR=3 };
extern volatile LONG nvn_lifetime_state; /* -1 uninitialized, 0 off, 1 enabled */
int nvn_lifetime_enabled(void);
uint64_t nvn_lifetime_record(uint32_t event, uint64_t host, uint64_t guest,
    uint64_t pool, uint64_t addr, uint64_t size, uint64_t detail, uint64_t caller);
#define NVN_LIFE(event, host, guest, pool, addr, size, detail, caller) \
    (nvn_lifetime_state ? nvn_lifetime_record((event), (uint64_t)(uintptr_t)(host), \
     (guest), (pool), (uint64_t)(uintptr_t)(addr), (size), (detail), (caller)) : 0)
uint64_t nvn_lifetime_release_begin(uint64_t addr, uint64_t size, uint64_t kind, uint64_t caller);
void nvn_lifetime_release_end(uint64_t token, int success);
#endif

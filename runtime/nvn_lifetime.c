/* A fixed-width, allocation-free event stream. Disabled unless SSBU_NVN_LIFETIME names a NEW file.
 * WriteFile orders each complete record under one lock; it never waits on rendering or a GPU. */
#include "nvn_lifetime.h"
#include <stdio.h>
#include <stddef.h>
#include <string.h>
volatile LONG nvn_lifetime_state = -1;
static INIT_ONCE once = INIT_ONCE_STATIC_INIT;
static SRWLOCK lock = SRWLOCK_INIT;
static HANDLE output = INVALID_HANDLE_VALUE;
static uint64_t sequence;
typedef struct {
    uint64_t seq, tick, host, guest, pool, addr, size, detail, caller;
    uint32_t event, thread;
} LifeRecord;
typedef char record_size_must_be_80[sizeof(LifeRecord)==80 ? 1 : -1];
static BOOL CALLBACK initialize(PINIT_ONCE init, PVOID parameter, PVOID *context) {
    WCHAR path[1024]; DWORD n = GetEnvironmentVariableW(L"SSBU_NVN_LIFETIME", path, 1024);
    (void)init; (void)parameter; (void)context;
    if (!n) { InterlockedExchange(&nvn_lifetime_state,0); return TRUE; }
    if (n >= 1024) { InterlockedExchange(&nvn_lifetime_state,0); fprintf(stderr,"nvn lifetime: log path too long; disabled\n"); return TRUE; }
    output = CreateFileW(path, GENERIC_WRITE, FILE_SHARE_READ, NULL, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, NULL);
    if (output == INVALID_HANDLE_VALUE) { InterlockedExchange(&nvn_lifetime_state,0); fprintf(stderr,"nvn lifetime: cannot create a new log (%lu); disabled\n",GetLastError()); return TRUE; }
    struct { char magic[8]; uint32_t version, size; uint64_t frequency, reserved; } header = {{'N','V','N','L','I','F','E',0},1,sizeof(LifeRecord),0,0};
    LARGE_INTEGER frequency; QueryPerformanceFrequency(&frequency); header.frequency = frequency.QuadPart;
    DWORD written;
    if (!WriteFile(output,&header,sizeof header,&written,NULL) || written != sizeof header) {
        CloseHandle(output); output=INVALID_HANDLE_VALUE; InterlockedExchange(&nvn_lifetime_state,0); return TRUE;
    }
    InterlockedExchange(&nvn_lifetime_state,1);
    return TRUE;
}
int nvn_lifetime_enabled(void) {
    if (!nvn_lifetime_state) return 0;
    InitOnceExecuteOnce(&once,initialize,NULL,NULL);
    return nvn_lifetime_state > 0;
}
uint64_t nvn_lifetime_record(uint32_t event, uint64_t host, uint64_t guest,
    uint64_t pool, uint64_t addr, uint64_t size, uint64_t detail, uint64_t caller) {
    if (!nvn_lifetime_enabled()) return 0;
    AcquireSRWLockExclusive(&lock);
    uint64_t token=0;
    if (nvn_lifetime_state) {
        LARGE_INTEGER tick; QueryPerformanceCounter(&tick);
        LifeRecord r={++sequence,(uint64_t)tick.QuadPart,host,guest,pool,addr,size,detail,caller,event,GetCurrentThreadId()};
        DWORD written;
        if (WriteFile(output,&r,sizeof r,&written,NULL) && written==sizeof r) token=r.seq;
        else { InterlockedExchange(&nvn_lifetime_state,0); CloseHandle(output); output=INVALID_HANDLE_VALUE; }
    }
    ReleaseSRWLockExclusive(&lock);
    return token;
}
uint64_t nvn_lifetime_release_begin(uint64_t addr,uint64_t size,uint64_t kind,uint64_t caller) {
    if (!nvn_lifetime_enabled()) return 0;
    /* MEM_RELEASE has size 0. Resolve only for tracing, before VirtualFree changes the map. */
    if (!size) {
        MEMORY_BASIC_INFORMATION mi; uint64_t end=addr;
        if (VirtualQuery((const void *)(uintptr_t)addr,&mi,sizeof mi) && (uint64_t)(uintptr_t)mi.AllocationBase==addr) {
            do {
                uint64_t next=(uint64_t)(uintptr_t)mi.BaseAddress+mi.RegionSize;
                if (next<=end) break; end=next;
            } while (VirtualQuery((const void *)(uintptr_t)end,&mi,sizeof mi) && (uint64_t)(uintptr_t)mi.AllocationBase==addr);
            size=end-addr;
        }
    }
    return NVN_LIFE(NL_RELEASE_BEGIN,0,0,0,addr,size,kind,caller);
}
void nvn_lifetime_release_end(uint64_t token,int success) {
    if (token) (void)NVN_LIFE(NL_RELEASE_END,token,0,0,0,0,success!=0,0);
}

/* Exercises the event stream/analysis contract, not a game crash reproduction. */
#include "../runtime/nvn_lifetime.h"
#include <assert.h>
#include <string.h>
#include <stdio.h>
static uint64_t source;
static DWORD WINAPI release_source(void *unused) {
    (void)unused;
    uint64_t token=nvn_lifetime_release_begin(source,0,NL_RELEASE,0x1234);
    int success=VirtualFree((void *)(uintptr_t)source,0,MEM_RELEASE)!=0;
    assert(success); nvn_lifetime_release_end(token,success); return 0;
}
static uint64_t begin(uint64_t host,uint64_t address,uint64_t owned) {
    return NVN_LIFE(NL_READ_BEGIN,host,0,0x101,address,64,owned,0);
}
static void end(uint64_t host,uint64_t token) { (void)NVN_LIFE(NL_READ_END,host,0,0x101,0,0,token,0); }
int main(int argc,char **argv) {
    if(argc>1 && !strcmp(argv[1],"--disabled")) {
        assert(!NVN_LIFE(NL_POOL_INIT,0,1,1,0,0,0,0)); assert(!nvn_lifetime_state); return 0;
    }
    source=(uint64_t)(uintptr_t)VirtualAlloc(NULL,65536,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE);assert(source);
    (void)NVN_LIFE(NL_POOL_INIT,0,0x101,0x101,source,65536,0,0);
    (void)NVN_LIFE(NL_TEXTURE_INIT,0x201,0x301,0x101,source,64,0,0);
    (void)NVN_LIFE(NL_PACKAGED_SOURCE,0x201,0x301,0x101,source,64,0,0);
    (void)NVN_LIFE(NL_SNAPSHOT_READY,0x201,0x301,0x101,0x40000000,64,0,0);
    uint64_t read=begin(0x201,source,NL_BORROWED);
    HANDLE thread=CreateThread(NULL,0,release_source,NULL,0,NULL);assert(thread);
    assert(WaitForSingleObject(thread,10000)==WAIT_OBJECT_0);CloseHandle(thread);
    end(0x201,read);
    read=begin(0x201,0x40000000,NL_OWNED);end(0x201,read); /* captured bytes survive guest release */
    read=begin(0x201,source,NL_BORROWED);end(0x201,read); /* marker only; no invalid dereference */
    assert(VirtualAlloc((void *)(uintptr_t)source,65536,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE));
    (void)NVN_LIFE(NL_POOL_FINAL_BEGIN,0,0x101,0x101,source,65536,0,0);
    /* Reinitialization can interleave with the old finalizer's trailing marker. */
    (void)NVN_LIFE(NL_POOL_INIT,0,0x101,0x101,source,65536,0,0);
    (void)NVN_LIFE(NL_POOL_FINAL_END,0,0x101,0x101,0,0,0,0);
    read=begin(0x201,source,NL_BORROWED);end(0x201,read); /* old texture, reused pool address */
    (void)NVN_LIFE(NL_TEXTURE_INIT,0x202,0x302,0x101,source,64,0,0);
    read=begin(0x202,source,NL_BORROWED);end(0x202,read); /* new generation is valid */
    uint64_t failed=nvn_lifetime_release_begin(source,64,NL_DECOMMIT,0);
    nvn_lifetime_release_end(failed,0);
    read=begin(0x202,source,NL_BORROWED);end(0x202,read); /* failed frees do not invalidate */
    read=begin(0x201,0x40000000,NL_OWNED); /* marker only; tests cleanup during an outstanding read */
    (void)NVN_LIFE(NL_SNAPSHOT_RELEASE,0x201,0x301,0x101,0x40000000,64,0,0);
    end(0x201,read);
    read=begin(0x201,0x40000000,NL_OWNED);end(0x201,read);
    assert(VirtualFree((void *)(uintptr_t)source,0,MEM_RELEASE));
    puts("PASS lifecycle event markers with real cross-thread release, pool address reuse and owned snapshot distinction");
    return 0;
}

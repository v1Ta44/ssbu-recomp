/* SSBU 13.0.4 adapter. The verified route is pinned by the tag
 * net-delay-verified-20261018: run the original calculator, then set byte +65.
 * Core and frontend have no dependency on this file or guest memory. */
#include <windows.h>
#include <bcrypt.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <mutex>
#include <string>
#include "hook.h"
#define SSBU_NET_API
#include "yuzu_net.h"
#include "control.h"
#include "delay_api.h"
#include "bridge.h"
#include "preferences.h"

namespace {
const uint64_t target=0x71016ccae0ULL;
const unsigned target_size=400,field_offset=65;
const char *fingerprint="321ce8ab4a70fed64737950bdc8feac5b1eb258bfa39998b62db4fb3ebb98ecf";
typedef int (*LdnRead)(SsbuNetLdnSnapshot *);
const HookApi *hooks;
HookOrig original;
LdnRead ldn_read;
NdEngine *engine;
std::mutex trace_lock,settings_lock;
FILE *trace;
uint64_t last_caller,last_object;float last_metric;
wchar_t settings_path[32768];
volatile LONG started,preference_status,closing;
NdBridge *frontend;

bool memory_ok(uint64_t address,size_t length,bool write) {
    if(!address || address+length<address)return false;
    for(uint64_t p=address;p<address+length;){
        MEMORY_BASIC_INFORMATION m;
        if(!VirtualQuery((void *)(uintptr_t)p,&m,sizeof m) || m.State!=MEM_COMMIT || (m.Protect&(PAGE_GUARD|PAGE_NOACCESS)))return false;
        DWORD mode=m.Protect&0xff;
        bool writable=mode==PAGE_READWRITE || mode==PAGE_WRITECOPY || mode==PAGE_EXECUTE_READWRITE || mode==PAGE_EXECUTE_WRITECOPY;
        if(!writable && (write || (mode!=PAGE_READONLY && mode!=PAGE_EXECUTE_READ)))return false;
        uint64_t end=(uint64_t)(uintptr_t)m.BaseAddress+m.RegionSize;if(end<=p)return false;p=end;
    }
    return true;
}
bool verified_target() {
    if(!memory_ok(target,target_size,false))return false;
    BCRYPT_ALG_HANDLE alg=NULL;BCRYPT_HASH_HANDLE hash=NULL;
    unsigned char digest[32];char hex[65];
    bool ok=BCryptOpenAlgorithmProvider(&alg,BCRYPT_SHA256_ALGORITHM,NULL,0)>=0;
    if(ok)ok=BCryptCreateHash(alg,&hash,NULL,0,NULL,0,0)>=0;
    if(ok)ok=BCryptHashData(hash,(PUCHAR)(uintptr_t)target,target_size,0)>=0;
    if(ok)ok=BCryptFinishHash(hash,digest,sizeof digest,0)>=0;
    if(hash)BCryptDestroyHash(hash);if(alg)BCryptCloseAlgorithmProvider(alg,0);
    if(!ok)return false;
    for(int i=0;i<32;i++)snprintf(hex+i*2,3,"%02x",digest[i]);
    return !strcmp(hex,fingerprint);
}
NdSession context(SsbuNetLdnSnapshot *raw=NULL) {
    SsbuNetLdnSnapshot s={};s.size=sizeof s;s.abi=SSBU_NET_ABI;
    NdSession out={};
    if(ldn_read && ldn_read(&s)==SSBU_NET_OK){
        out.available=1;out.connected=s.room_state==SSBU_NET_JOINED;
        out.role=s.ldn_state==3?1:s.ldn_state==5?2:0;out.peers=s.node_count;
        out.connection=s.room_epoch;out.session=s.ldn_generation;
    }
    if(raw)*raw=s;return out;
}
int snapshot(NdSnapshot *out) {
    if(!engine || !out || out->size!=sizeof *out || out->abi!=ND_ABI)return 0;
    NdSession live=context();nd_session(engine,&live);return nd_snapshot(engine,out);
}
int select_frames(int frames) {
    std::lock_guard<std::mutex> settings(settings_lock);
    if(!nd_select(engine,frames))return 0;
    if(*settings_path)InterlockedExchange(&preference_status,nd_preferences::save(settings_path,frames)?1:-1);
    std::lock_guard<std::mutex> lock(trace_lock);
    if(trace){fprintf(trace,"# selection,%llu,%d\n",(unsigned long long)GetTickCount64(),frames);fflush(trace);}
    return 1;
}
const char *decision_name(int d) {
    static const char *const names[]={"auto_observe","applied","no_ldn_peer","session_changed","bad_value","memory_unavailable","not_installed"};
    return d>=0 && d<7?names[d]:"unknown";
}
struct WriteField { uint64_t address;NdSession session; };
int write_field(void *user,int frames) {
    WriteField *f=(WriteField *)user;
    NdSession live=context();
    if(!nd_session_active(&live) || live.connection!=f->session.connection || live.session!=f->session.session || live.role!=f->session.role ||
       !memory_ok(f->address,1,true))return 0;
    *(uint8_t *)(uintptr_t)f->address=(uint8_t)frames;return 1;
}
void replacement(Ctx *c,void *) {
    uint64_t object=c->x[0],caller=c->x[30];float metric;memcpy(&metric,&c->v[0],sizeof metric);
    NdSession before=context();
    NdSample sample={};sample.before=-1;sample.automatic=-1;
    bool readable=memory_ok(object,99,false);
    if(readable){
        sample.before=*(uint8_t *)(uintptr_t)(object+field_offset);
        memcpy(&sample.mode,(void *)(uintptr_t)(object+48),4);sample.object_state=*(uint8_t *)(uintptr_t)(object+98);
    }
    original(c); // Preserve all original register/flag/stack results.
    SsbuNetLdnSnapshot raw;NdSession after=context(&raw);
    sample.readable=readable && memory_ok(object+field_offset,1,false);
    if(sample.readable)sample.automatic=*(uint8_t *)(uintptr_t)(object+field_offset);
    if(InterlockedCompareExchange(&closing,0,0))return;
    WriteField write={object+field_offset,after};NdSnapshot result={};
    nd_evaluate(engine,&before,&after,&sample,write_field,&write,&result);
    std::lock_guard<std::mutex> lock(trace_lock);
    last_caller=caller;last_object=object;last_metric=metric;
    if(trace){
        fprintf(trace,"%llu,%llu,%d,%d,%d,%d,%s,%u,%u,%u,%llu,%llu,%llx,%llx,%u,%u,%.9g\n",
            (unsigned long long)GetTickCount64(),(unsigned long long)result.calls,result.selected,result.before,result.automatic,result.effective,decision_name(result.decision),
            raw.room_state,raw.ldn_state,raw.node_count,(unsigned long long)raw.room_epoch,(unsigned long long)raw.ldn_generation,
            (unsigned long long)caller,(unsigned long long)object,result.mode,result.object_state,(double)metric);
        if(result.calls%64==0)fflush(trace);
    }
}
int ui_preferences(){return InterlockedCompareExchange(&preference_status,0,0);}
void ui_log(const char *s){hooks->log(s);}
BOOL CALLBACK game_window(HWND window,LPARAM result) {
    DWORD pid=0;GetWindowThreadProcessId(window,&pid);wchar_t name[96];
    if(pid==GetCurrentProcessId() && GetClassNameW(window,name,96) && !wcscmp(name,L"ssbu_native_window")){
        *(HWND *)result=window;return FALSE;
    }
    return TRUE;
}
void start_frontend() {
    const char *mode=getenv("SSBU_NET_DELAY_UI");
    const char *old=getenv("SSBU_NET_DELAY_WINDOW");
    if((mode && !strcmp(mode,"off")) || (!mode && old && !strcmp(old,"0")))return;
    HMODULE self=NULL;
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,(LPCWSTR)(uintptr_t)&start_frontend,&self);
    wchar_t path[32768];DWORD n=GetModuleFileNameW(self,path,32768);if(!n || n>=32768)return;
    wchar_t *slash=wcsrchr(path,L'\\');if(!slash)return;*slash=0;
    std::wstring base=path;base+=L"\\..\\net_delay";
    const wchar_t *data=_wgetenv(L"SSBU_NET_DELAY_UI_DATA");
    std::wstring cache=data && *data?data:base+L"\\webview-data";
    HWND owner=NULL;EnumWindows(game_window,(LPARAM)&owner);
    if(!owner){hooks->log("net-delay: game window unavailable; control API remains active");return;}
    NdProvider provider={snapshot,select_frames,ui_preferences,ui_log};
    frontend=nd_bridge_start(&provider,(base+L"\\delay_panel.exe").c_str(),(base+L"\\ui").c_str(),cache.c_str(),
        (mode && !strcmp(mode,"hidden")) || (old && !strcmp(old,"hidden")),false,(uintptr_t)owner);
    if(!frontend)hooks->log("net-delay: optional frontend could not start; backend remains available");
}
void finish() {
    InterlockedExchange(&closing,1);
    if(frontend)nd_bridge_stop(frontend);
    std::lock_guard<std::mutex> lock(trace_lock);
    NdSnapshot state={};state.size=sizeof state;state.abi=ND_ABI;nd_snapshot(engine,&state);
    if(trace){fprintf(trace,"# summary,calls=%llu,writes=%llu\n",(unsigned long long)state.calls,(unsigned long long)state.writes);fclose(trace);trace=NULL;}
    /* Engine lives to process teardown: guest worker threads may still return
     * through this wrapper while the host exits. No callbacks outlive the UI. */
}
}
extern "C" __declspec(dllexport) int ssbu_delay_set_frames(int frames){return select_frames(frames);}
extern "C" __declspec(dllexport) int ssbu_delay_state(NdSnapshot *out){return snapshot(out);}
extern "C" __declspec(dllexport) int ssbu_delay_snapshot(DelaySnapshot *out) {
    if(!out || out->size!=sizeof *out || out->abi!=DELAY_ABI)return 0;
    NdSnapshot s={};s.size=sizeof s;s.abi=ND_ABI;if(!snapshot(&s))return 0;
    memset(out,0,sizeof *out);out->size=sizeof *out;out->abi=DELAY_ABI;
    out->selected=s.selected;out->installed=s.backend==ND_BACKEND_READY;out->decision=s.decision;
    out->before=s.before;out->automatic=s.automatic;out->effective=s.effective;out->calls=s.calls;out->writes=s.writes;
    out->mode=s.mode;out->object_state=s.object_state;out->room_state=s.live.connected?SSBU_NET_JOINED:SSBU_NET_IDLE;
    out->ldn_state=s.live.role==1?3:s.live.role==2?5:0;out->nodes=s.live.peers;
    out->room_epoch=s.live.connection;out->generation=s.live.session;
    {std::lock_guard<std::mutex> lock(trace_lock);out->caller=last_caller;out->object=last_object;out->input_metric=last_metric;}
    return 1;
}
extern "C" __declspec(dllexport) void mod_init(const HookApi *api) {
    if(InterlockedExchange(&started,1) || !api || api->version!=1 || api->size<sizeof *api)return;
    hooks=api;engine=nd_create();if(!engine){api->log("net-delay: backend allocation failed");return;}
    const wchar_t *path=_wgetenv(L"SSBU_NET_DELAY_CSV");if(path && *path)trace=_wfopen(path,L"wb");
    if(trace){fputs("# net-delay schema=1; automatic may reuse prior value; input_metric is not a measured RTT\nms,call,selected,before,automatic,effective,decision,room,ldn,nodes,room_epoch,ldn_generation,caller,object,mode,object_state,input_metric\n",trace);fflush(trace);}
    const wchar_t *config=_wgetenv(L"SSBU_NET_DELAY_CONFIG");
    if(config && *config && wcslen(config)<32768){wcscpy(settings_path,config);int saved;if(nd_preferences::load(settings_path,&saved)){nd_select(engine,saved);InterlockedExchange(&preference_status,1);}}
    const char *setting=getenv("SSBU_NET_DELAY");
    if(setting){
        InterlockedExchange(&preference_status,0);
        if(!strcmp(setting,"auto"))nd_select(engine,ND_AUTO);
        else {char *end;long n=strtol(setting,&end,10);if(*setting && !*end && n>=0 && n<=ND_MAX_FRAMES)nd_select(engine,(int)n);else {nd_select(engine,ND_AUTO);api->log("net-delay: invalid selection; using Auto");}}
    }
    ldn_read=(LdnRead)(void *)GetProcAddress(GetModuleHandleW(NULL),"ssbu_net_ldn_snapshot");
    if(!ldn_read){nd_backend(engine,ND_BACKEND_CONTEXT);api->log("net-delay: no LDN snapshot export; disabled");}
    else if(!verified_target()){nd_backend(engine,ND_BACKEND_VERSION);api->log("net-delay: unsupported target fingerprint; disabled without installing a hook");}
    else if(!api->install(target,replacement,NULL,&original)){nd_backend(engine,ND_BACKEND_HOOK);api->log("net-delay: hook installation failed");api->log(api->error());}
    else {nd_backend(engine,ND_BACKEND_READY);api->log("net-delay: 13.0.4 calculator entry 0x71016ccae0 verified; field +65; separated backend; LDN peer gate enabled");}
    atexit(finish);start_frontend();
}

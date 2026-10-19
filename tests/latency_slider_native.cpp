/* Executes the user's generated calculator, then compares the native wrapper.
 * The calculator body is generated into build/ by the Python runner. */
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "runtime.h"
#define SSBU_CTX_DEFINED
#include "hook.h"
#define SSBU_NET_API
#include "yuzu_net.h"
#include "control.h"
#define CHECK(x) do { if(!(x)){fprintf(stderr,"FAIL %s:%d %s\n",__FILE__,__LINE__,#x);exit(1);} } while(0)
static HookFn replacement;
static void *replacement_user;
static SsbuNetLdnSnapshot network;
static int tuning,session_change,read_error;
static int (*set_frames)(int);
static int (*get_status)(DelaySnapshot *);
static const uint64_t target=0x71016ccae0ULL;
#ifndef NO_LDN_EXPORT
extern "C" __declspec(dllexport)
#else
extern "C"
#endif
int ssbu_net_ldn_snapshot(SsbuNetLdnSnapshot *out) {
    if(read_error)return SSBU_NET_IO;
    CHECK(out && out->size==sizeof *out && out->abi==SSBU_NET_ABI);*out=network;return 0;
}
void f_71017392c0(Ctx *c) { c->x[0]=c->x[1]==0x2712?(uint32_t)tuning:300; }
void f_71016cd780(Ctx *c) { c->x[0]=3; }
#include "latency_target.generated.h"
static void original(Ctx *c) {
    f_71016ccae0(c);
    if(session_change==1)network.ldn_generation++;
    if(session_change==2)network.room_epoch++;
    if(session_change==3)network.ldn_state=4;
}
static int install(uint64_t address,HookFn hook,void *user,HookOrig *orig) {
    CHECK(address==target);replacement=hook;replacement_user=user;*orig=original;return 1;
}
static const char *error(void) {return "fixture error";}
static void log_line(const char *s){puts(s);}
static HookApi api={sizeof(HookApi),1,install,NULL,NULL,NULL,NULL,NULL,NULL,log_line,error};
static unsigned char object[256],stack_bytes[4096];
static void reset_network(unsigned room,unsigned ldn,unsigned nodes) {
    memset(&network,0,sizeof network);network.size=sizeof network;network.abi=SSBU_NET_ABI;
    network.room_state=room;network.ldn_state=ldn;network.node_count=nodes;network.room_epoch=77;network.ldn_generation=88;
}
static int check_case(int frames,int mode,float metric,int enabled,int change) {
    CHECK(set_frames(frames));reset_network(enabled?SSBU_NET_JOINED:SSBU_NET_IDLE,enabled?3:0,enabled?2:0);
    session_change=0;
    memset(object,0,sizeof object);object[65]=7;object[67]=2;object[64]=1;object[52]=6;object[98]=3;
    memcpy(object+48,&mode,sizeof mode);
    memset(stack_bytes,0x3c,sizeof stack_bytes);
    Ctx input={};for(int i=0;i<31;i++)input.x[i]=0xabc000+i;
    for(int i=0;i<32;i++){input.v[i].d[0]=0x123400+i;input.v[i].d[1]=0xabcd00+i;}
    input.x[0]=(uint64_t)(uintptr_t)object;input.x[31]=(uint64_t)(uintptr_t)(stack_bytes+sizeof stack_bytes-16);
    input.x[30]=0x71016d7e38ULL;input.N=1;input.C=1;input.V=1;
    memcpy(&input.v[0],&metric,4);
    unsigned char start[sizeof object],expected[sizeof object],start_stack[sizeof stack_bytes],expected_stack[sizeof stack_bytes];
    memcpy(start,object,sizeof object);memcpy(start_stack,stack_bytes,sizeof stack_bytes);
    Ctx plain=input;original(&plain);memcpy(expected,object,sizeof object);memcpy(expected_stack,stack_bytes,sizeof stack_bytes);
    int automatic=expected[65];
    memcpy(object,start,sizeof object);memcpy(stack_bytes,start_stack,sizeof stack_bytes);
    reset_network(enabled?SSBU_NET_JOINED:SSBU_NET_IDLE,enabled?3:0,enabled?2:0);session_change=change;
    Ctx wrapped=input;replacement(&wrapped,replacement_user);
    CHECK(!memcmp(&plain,&wrapped,sizeof plain));
    int apply=frames>=0 && enabled && !change && !read_error && automatic<=25;
    if(apply)expected[65]=(unsigned char)frames;
    CHECK(!memcmp(expected,object,sizeof object));CHECK(!memcmp(expected_stack,stack_bytes,sizeof stack_bytes));
    DelaySnapshot status={};status.size=sizeof status;status.abi=DELAY_ABI;CHECK(get_status(&status));
    CHECK(status.automatic==automatic && status.effective==expected[65]);
    CHECK((status.decision==DELAY_APPLIED)==apply);
    CHECK(status.caller==input.x[30] && status.object==(uint64_t)(uintptr_t)object);
    return apply;
}
int main(int argc,char **argv) {
    CHECK(argc>=3);
    void *image=VirtualAlloc((void *)0x7100000000ULL,0x6000000,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE);CHECK(image==(void *)0x7100000000ULL);
    FILE *elf=fopen(argv[2],"rb");CHECK(elf);
    CHECK(!fseek(elf,0x1000+0x16ccae0,SEEK_SET));CHECK(fread((void *)(uintptr_t)target,1,400,elf)==400);
    CHECK(!fseek(elf,0x1000+0x4471000,SEEK_SET));CHECK(fread((void *)0x7104471000ULL,1,8192,elf)==8192);fclose(elf);
    uint64_t manager=0x71052d0000ULL,service=0x71052d1000ULL;
    memcpy((void *)0x71052c3c58ULL,&manager,8);memcpy((void *)(uintptr_t)(manager+8),&service,8);
    if(argc>3 && !strcmp(argv[3],"--bad-fingerprint"))*(unsigned char *)(uintptr_t)target^=1;
    HMODULE plugin=LoadLibraryA(argv[1]);CHECK(plugin);
    void (*init)(const HookApi *)=(void (*)(const HookApi *))(void *)GetProcAddress(plugin,"mod_init");
    set_frames=(int (*)(int))(void *)GetProcAddress(plugin,"ssbu_delay_set_frames");
    get_status=(int (*)(DelaySnapshot *))(void *)GetProcAddress(plugin,"ssbu_delay_snapshot");CHECK(init && set_frames && get_status);
    init(&api);DelaySnapshot status={};status.size=sizeof status;status.abi=DELAY_ABI;CHECK(get_status(&status));
    if(argc>3 && (!strcmp(argv[3],"--bad-fingerprint") || !strcmp(argv[3],"--no-context"))) {CHECK(!replacement && !status.installed);puts("PASS unsupported fingerprint/missing context installs no hook");return 0;}
    CHECK(replacement && status.installed);CHECK(!get_status(NULL));DelaySnapshot invalid={};CHECK(!get_status(&invalid));
    CHECK(!set_frames(-2) && !set_frames(26) && !set_frames(1000000));
    if(argc>3 && !strcmp(argv[3],"--save-choice")){CHECK(set_frames(3));puts("saved=3");return 0;}
    if(argc>3 && !strcmp(argv[3],"--parse")){printf("selected=%d\n",status.selected);return 0;}
    int cases=0,writes=0;
    const float metrics[]={0.0f,10.0f,33.0f,100.0f,300.0f,1000.0f};
    for(int mode=0;mode<3;mode++)for(tuning=0;tuning<4;tuning++)for(float metric:metrics)for(int frames=-1;frames<=25;frames++){
        writes+=check_case(frames,mode,metric,1,0);cases++;
    }
    for(int frames=-1;frames<=25;frames++)for(int change=0;change<4;change++){
        writes+=check_case(frames,0,33.0f,change?1:0,change);cases++;
    }
    read_error=1;writes+=check_case(0,0,20.0f,1,0);cases++;read_error=0;
    for(unsigned ldn=0;ldn<=6;ldn++)for(unsigned nodes=0;nodes<=9;nodes++){
        reset_network(SSBU_NET_JOINED,ldn,nodes);CHECK(set_frames(1));session_change=0;
        memset(object,0,sizeof object);object[65]=5;
        Ctx c={};c.x[0]=(uint64_t)(uintptr_t)object;c.x[31]=(uint64_t)(uintptr_t)(stack_bytes+4000);
        replacement(&c,NULL);
        status.size=sizeof status;status.abi=DELAY_ABI;CHECK(get_status(&status));
        int apply=(ldn==3 || ldn==5) && nodes>=2 && nodes<=8;
        CHECK((status.decision==DELAY_APPLIED)==apply);cases++;writes+=apply;
    }
    CHECK(get_status(&status));CHECK(status.writes==(uint64_t)writes && status.calls==(uint64_t)cases);
    /* Some original branches retain the previous byte instead of recalculating.
     * Auto must stop overriding, not invent a fresh automatic value. */
    reset_network(SSBU_NET_JOINED,3,2);tuning=3;session_change=0;
    memset(object,0,sizeof object);object[65]=7;
    Ctx cached={};cached.x[0]=(uint64_t)(uintptr_t)object;cached.x[31]=(uint64_t)(uintptr_t)(stack_bytes+4000);
    CHECK(set_frames(2));replacement(&cached,NULL);CHECK(object[65]==2);
    cached.x[0]=(uint64_t)(uintptr_t)object;cached.x[31]=(uint64_t)(uintptr_t)(stack_bytes+4000);
    CHECK(set_frames(-1));replacement(&cached,NULL);CHECK(object[65]==2);
    cases+=2;writes++;CHECK(get_status(&status) && status.decision==DELAY_OBSERVE);
    CHECK(status.calls==(uint64_t)cases && status.writes==(uint64_t)writes);
    puts("PASS retained-value branch: Auto stops overrides without fabricating an automatic recalculation");
    CHECK(set_frames(-1));
    printf("PASS %d original-calculator comparisons/gates; %d manual writes; registers/flags/stack preserved; only byte +65 changes\n",cases,writes);
    return 0;
}

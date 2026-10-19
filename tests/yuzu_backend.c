/* One bounded loopback integration test. The room fixture implements the pinned
 * upstream wire format independently (no production packet codec is included).
 * It is not a substitute for running an actual Yuzu game. */
#include "yuzu_net_internal.h"
#include <enet/enet.h>
#include <zstd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define CHECK(x) do { if (!(x)) { fprintf(stderr,"FAIL line %d: %s\n",__LINE__,#x); ExitProcess(1); } } while(0)
static ENetHost *server;
static ENetPeer *client;
static const uint8_t own[4]={192,168,1,2}, remote[4]={192,168,1,3};
static LONG signals;
void os_signal_event(uint64_t key) { CHECK(key==123); InterlockedIncrement(&signals); }
typedef struct { uint8_t b[8192]; size_t n; } Packet;
static void bytes(Packet *p,const void *b,size_t n) { CHECK(p->n+n<=sizeof p->b); if(n)memcpy(p->b+p->n,b,n);p->n+=n; }
static void integer(Packet *p,uint64_t v,int n) { for(int i=n-1;i>=0;i--){uint8_t b=(uint8_t)(v>>(i*8));bytes(p,&b,1);} }
static void string(Packet *p,const char *s) { integer(p,strlen(s),4);bytes(p,s,strlen(s)); }
static void send_packet(const Packet *p) {
    ENetPacket *e=enet_packet_create(p->b,p->n,ENET_PACKET_FLAG_RELIABLE);
    CHECK(e && enet_peer_send(client,0,e)==0);enet_host_flush(server);
}
static void room_info(void) {
    Packet p={0};integer(&p,3,1);string(&p,"Fixture");string(&p,"Loopback only");integer(&p,8,4);integer(&p,24872,2);
    string(&p,"SSBU");string(&p,"host");integer(&p,2,4);
    for(int i=0;i<2;i++) {
        string(&p,i?"YuzuPeer":"PCPlayer");bytes(&p,i?remote:own,4);string(&p,"SSBU");integer(&p,0x01006a800016e000ULL,8);
        string(&p,"13.0.4");string(&p,i?"verified":"");string(&p,i?"Peer display":"");string(&p,i?"https://example.invalid/avatar.png":"");
    }
    send_packet(&p);
}
static int pump(Packet *out) {
    ENetEvent e;int r=enet_host_service(server,&e,5);CHECK(r>=0);
    if(!r)return 0;
    if(e.type==ENET_EVENT_TYPE_CONNECT){client=e.peer;return 0;}
    if(e.type!=ENET_EVENT_TYPE_RECEIVE)return 0;
    CHECK(e.channelID==0 && (e.packet->flags&ENET_PACKET_FLAG_RELIABLE));
    CHECK(e.packet->dataLength<=sizeof out->b);out->n=e.packet->dataLength;memcpy(out->b,e.packet->data,out->n);enet_packet_destroy(e.packet);
    if(out->b[0]==1) {
        /* Fixed Packet::Write vector: nickname, automatic IP, v1, password, token. */
        static const uint8_t join[]={1,0,0,0,8,'P','C','P','l','a','y','e','r',255,255,255,255,0,0,0,1,0,0,0,2,'p','w',0,0,0,12,'S','E','C','R','E','T','_','T','O','K','E','N'};
        CHECK(out->n==sizeof join && !memcmp(out->b,join,sizeof join));
        room_info();Packet ok={{2,192,168,1,2},5};send_packet(&ok);return 0;
    }
    return 1;
}
static Packet receive(unsigned id,int type) {
    ULONGLONG end=GetTickCount64()+2500;Packet p={0};
    while(GetTickCount64()<end)if(pump(&p) && p.b[0]==id && (type<0 || (p.n>1 && p.b[1]==type)))return p;
    CHECK(!"fixture receive timeout");return p;
}
static SsbuNetSnapshot snapshot;
static void wait_state(unsigned state) {
    ULONGLONG end=GetTickCount64()+2500;Packet p;
    do {pump(&p);snapshot.size=sizeof snapshot;snapshot.abi=SSBU_NET_ABI;CHECK(!ssbu_net_snapshot(&snapshot));if(snapshot.state==state)return;}while(GetTickCount64()<end);
    CHECK(!"room state timeout");
}
static void ldn(unsigned type,const void *data,size_t n) {
    Packet p={0};integer(&p,6,1);integer(&p,type,1);bytes(&p,remote,4);bytes(&p,own,4);integer(&p,0,1);integer(&p,n,4);bytes(&p,data,n);send_packet(&p);
}
static void node(uint8_t *p,const uint8_t *ip,int id,const char *name) {
    memset(p,0,64);for(int i=0;i<4;i++)p[i]=ip[3-i];p[4]=2;memcpy(p+6,ip,4);p[10]=(uint8_t)id;p[11]=1;memcpy(p+12,name,strlen(name));p[46]=1;
}
typedef struct { int op,fd,count,result; uint8_t info[0x480],filter[0x60],user[48],buf[64]; HANDLE started; } Job;
static DWORD WINAPI run_job(void *arg) {
    Job *j=arg;SetEvent(j->started);
    if(j->op==0)j->result=yn_ldn_scan(j->info,&j->count,1,j->filter);
    else if(j->op==1)j->result=yn_ldn_connect(j->info,j->user,1);
    else j->result=yn_socket_recv(j->fd,j->buf,sizeof j->buf,0,NULL);
    return 0;
}
static HANDLE start(Job *j) { j->started=CreateEvent(NULL,TRUE,FALSE,NULL);CHECK(j->started);HANDLE h=CreateThread(NULL,0,run_job,j,0,NULL);CHECK(h);CHECK(WaitForSingleObject(j->started,1000)==WAIT_OBJECT_0);return h; }
static void finish(HANDLE h,Job *j) { CHECK(WaitForSingleObject(h,2000)==WAIT_OBJECT_0);CloseHandle(h);CloseHandle(j->started); }
static void wait_nodes(unsigned n) {
    ULONGLONG end=GetTickCount64()+1500;uint8_t info[0x480];Packet p;
    while(GetTickCount64()<end){pump(&p);if(!yn_ldn_info(info) && info[0x67]==n)return;}CHECK(!"LDN nodes timeout");
}
static void check_ldn_context(unsigned state,unsigned nodes) {
    SsbuNetLdnSnapshot context={0};context.size=sizeof context;context.abi=SSBU_NET_ABI;
    CHECK(!ssbu_net_ldn_snapshot(&context));
    CHECK(context.ldn_state==state && context.node_count==nodes && !context.reserved);
    CHECK(context.room_epoch>0 && context.room_state==SSBU_NET_JOINED);
}
int main(int argc,char **argv) {
    if (argc==2 && !strcmp(argv[1],"--profile-probe")) {
        char profile[33]; ssbu_net_get_profile(profile);
        if (ssbu_net_validate_profile(profile)) return 2;
        fwrite(profile,1,strlen(profile),stdout); return 0;
    }
    CHECK(ssbu_net_ldn_snapshot(NULL)==SSBU_NET_BAD_ARGUMENT);
    SsbuNetLdnSnapshot invalid_context={0};CHECK(ssbu_net_ldn_snapshot(&invalid_context)==SSBU_NET_BAD_ARGUMENT);
    invalid_context.size=sizeof invalid_context;invalid_context.abi=SSBU_NET_ABI+1;
    CHECK(ssbu_net_ldn_snapshot(&invalid_context)==SSBU_NET_BAD_ARGUMENT);
    CHECK(ssbu_net_set_log_level(4)==SSBU_NET_BAD_ARGUMENT);
    CHECK(!ssbu_net_set_log_level(0));yn_log(1,"test","MUST_NOT_LOG_DISABLED");
    CHECK(!ssbu_net_set_log_level(1));yn_log(2,"test","MUST_NOT_LOG_DEBUG");
    yn_log(1,"test","logger enabled");CHECK(!ssbu_net_set_log_level(3));
    CHECK(SetEnvironmentVariableW(L"SSBU_NICKNAME",L"\x5c0f\x660e"));
    CHECK(!enet_initialize());ENetAddress address={0};CHECK(!enet_address_set_host_ip(&address,"127.0.0.1"));
    server=enet_host_create(&address,1,1,0,0);CHECK(server);CHECK(!enet_socket_get_address(server->socket,&address));
    CHECK(yn_ldn_initialize()==YN_LDN_ERROR(23));
    const char *name="\xe5\xb0\x8f\xe6\x98\x8e";char profile[33];ssbu_net_get_profile(profile);CHECK(!strcmp(name,profile));CHECK(!ssbu_net_set_profile(name));
    CHECK(ssbu_net_set_profile("\xff")==SSBU_NET_BAD_ARGUMENT);
    CHECK(ssbu_net_set_profile("   ")==SSBU_NET_BAD_ARGUMENT);
    CHECK(ssbu_net_set_profile("a\nb")==SSBU_NET_BAD_ARGUMENT);
    const uint8_t jpeg[]={255,216,255,217};uint8_t avatar[4];uint32_t fmt;
    CHECK(!ssbu_net_set_avatar(2,jpeg,4));CHECK(ssbu_net_get_avatar(&fmt,avatar,4)==4 && fmt==2 && !memcmp(jpeg,avatar,4));
    SsbuNetConfig c={0};c.size=sizeof c;c.abi=SSBU_NET_ABI;c.port=address.port;strcpy(c.host,"127.0.0.1");strcpy(c.nickname,"PCPlayer");strcpy(c.password,"pw");
    strcpy(c.token,"SECRET_TOKEN");
    strcpy(c.game_name,"SSBU");strcpy(c.game_version,"13.0.4");c.game_id=0x01006a800016e000ULL;
    CHECK(!ssbu_net_join(&c));Packet game=receive(4,-1);
    static const uint8_t game_vector[]={4,0,0,0,4,'S','S','B','U',1,0,0x6a,0x80,0,0x16,0xe0,0,0,0,0,6,'1','3','.','0','.','4'};
    CHECK(game.n==sizeof game_vector && !memcmp(game.b,game_vector,sizeof game_vector));
    wait_state(SSBU_NET_JOINED);CHECK(snapshot.member_count==2 && !memcmp(snapshot.ip,own,4));
    CHECK(!strcmp(snapshot.members[1].avatar_url,"https://example.invalid/avatar.png"));
    CHECK(ssbu_net_set_profile("other")==SSBU_NET_BUSY && ssbu_net_join(&c)==SSBU_NET_BUSY);
    check_ldn_context(0,0);
    puts("PASS room admission, fixed wire vectors, profile and avatar metadata");

    CHECK(!yn_ldn_initialize());CHECK(!yn_ldn_event(123));CHECK(!yn_ldn_open(0,0));
    uint8_t cfg[32]={0},security[68]={0},user[48]={0},info[0x480],peer_node[64];cfg[0]=0x42;cfg[18]=4;cfg[20]=1;memcpy(user,name,strlen(name));
    CHECK(!yn_ldn_create(cfg,security,user,NULL));CHECK(!yn_ldn_advertise("room-data",9));
    check_ldn_context(3,1);
    ldn(0,NULL,0);Packet scan=receive(6,1);CHECK(scan.n==15+0x480 && scan.b[0x26a+15]==9 && !memcmp(scan.b+15+0x68+12,name,strlen(name)));
    node(peer_node,remote,0,"YuzuUser");ldn(2,peer_node,64);Packet sync=receive(6,3);
    CHECK(sync.b[15+0x67]==2 && sync.b[15+0xa8+10]==1 && !memcmp(sync.b+15+0xa8+12,"YuzuUser",8));
    check_ldn_context(3,2);
    ldn(4,peer_node,64);wait_nodes(1);check_ldn_context(3,1);CHECK(signals>0);
    CHECK(!yn_ldn_disconnect(1));CHECK(!yn_ldn_open(0,1));CHECK(!yn_ldn_open(1,0));
    memset(info,0,sizeof info);info[0]=0x42;info[16]=0x77;info[0x20]=2;memcpy(info+0x22,remote,4);info[0x4b]=2;info[0x66]=4;info[0x67]=1;
    node(info+0x68,remote,0,"YuzuHost");
    Job j={0};j.filter[0]=0x42;j.filter[0x5c]=1;HANDLE h=start(&j);receive(6,0);ldn(1,info,sizeof info);finish(h,&j);
    CHECK(!j.result && j.count==1 && !memcmp(j.info,info,sizeof info));
    j.op=1;memcpy(j.user,user,sizeof user);h=start(&j);Packet connect=receive(6,2);
    CHECK(connect.n==79 && !memcmp(connect.b+15+12,name,strlen(name)) && !memcmp(connect.b+6,remote,4));
    memcpy(info+0xa8,connect.b+15,64);info[0xa8+10]=1;info[0x67]=2;ldn(3,info,sizeof info);finish(h,&j);
    CHECK(!j.result && yn_ldn_state()==5);
    /* A cancelled Connect must not clear a newly created session after waking. */
    CHECK(!yn_ldn_disconnect(0));h=start(&j);receive(6,2);
    CHECK(!yn_ldn_finalize());CHECK(!yn_ldn_initialize());CHECK(!yn_ldn_open(0,0));CHECK(!yn_ldn_create(cfg,security,user,NULL));
    finish(h,&j);CHECK(j.result && yn_ldn_state()==3);CHECK(!yn_ldn_info(info) && info[0x67]==1);
    puts("PASS LDN discovery, both host/station roles, user names and membership events");

    int fd=yn_socket_create(2,0);CHECK(fd>=YN_VFD_BASE);CHECK(yn_socket_create(1,6)==-93);
    struct sockaddr_in a={0};a.sin_family=AF_INET;a.sin_port=htons(4000);CHECK(!yn_socket_bind(fd,&a));
    a.sin_port=htons(5000);memcpy(&a.sin_addr,remote,4);CHECK(yn_socket_send(fd,"payload",7,0,&a)==7);
    Packet proxy=receive(5,-1);
    static const uint8_t header[]={5,1,192,168,1,2,0x0f,0xa0,1,192,168,1,3,0x13,0x88,0,0};
    CHECK(proxy.n>21 && !memcmp(proxy.b,header,sizeof header));
    char raw[64]={0};CHECK(ZSTD_decompress(raw,sizeof raw,proxy.b+21,proxy.n-21)==7 && !memcmp(raw,"payload",7));
    memcpy(proxy.b+2,remote,4);memcpy(proxy.b+9,own,4);proxy.b[6]=0x13;proxy.b[7]=0x88;proxy.b[13]=0x0f;proxy.b[14]=0xa0;
    send_packet(&proxy);
    ULONGLONG end=GetTickCount64()+1500;Packet ignored;
    while(!(yn_socket_poll(fd,1)&1) && GetTickCount64()<end)pump(&ignored);
    CHECK(yn_socket_poll(fd,1)&1);struct sockaddr_in src;
    CHECK(yn_socket_recv(fd,raw,sizeof raw,2,&src)==7 && !memcmp(&src.sin_addr,remote,4) && ntohs(src.sin_port)==5000);
    CHECK(yn_socket_recv(fd,raw,sizeof raw,0,&src)==7);CHECK(yn_socket_recv(fd,raw,sizeof raw,0x80,NULL)==-11);
    /* Malformed peer data must be dropped without poisoning the joined session. */
    uint64_t dropped=snapshot.dropped_packets;proxy.b[21]=0;send_packet(&proxy);end=GetTickCount64()+1500;
    do{pump(&ignored);ssbu_net_snapshot(&snapshot);}while(snapshot.dropped_packets==dropped && GetTickCount64()<end);
    CHECK(snapshot.dropped_packets>dropped && snapshot.state==SSBU_NET_JOINED);
    j.op=2;j.fd=fd;h=start(&j);CHECK(!yn_socket_close(fd));int next=yn_socket_create(2,0);CHECK(next!=fd);finish(h,&j);CHECK(j.result==-9);
    CHECK(yn_socket_poll(fd,1)==0x20);j.fd=next;h=start(&j);
    enet_peer_disconnect(client,0);enet_host_flush(server);wait_state(SSBU_NET_FAILED);
    SsbuNetLdnSnapshot lost={0};lost.size=sizeof lost;lost.abi=SSBU_NET_ABI;CHECK(!ssbu_net_ldn_snapshot(&lost));
    CHECK(lost.room_state==SSBU_NET_FAILED && !lost.node_count);
    puts("PASS copied LDN context, peer count transitions and disconnect invalidation");finish(h,&j);CHECK(j.result==-100);
    CHECK(yn_socket_poll(next,1)==0x18);ssbu_net_leave();CHECK(!yn_ldn_finalize());yn_socket_close_all();
    CHECK(!ssbu_net_set_avatar(0,NULL,0));CHECK(ssbu_net_get_avatar(NULL,NULL,0)==0);
    enet_host_destroy(server);enet_deinitialize();
    puts("PASS compressed datagrams, malformed packet bounds, stale descriptors and disconnect wakeup");
    for(int i=0;i<256;i++)yn_log(3,"test","rate limit probe=%d",i);
    yn_log(1,"test","lifecycle survives trace limit");
    uint64_t cursor=0;char tail[4096];int found=0;
    while(ssbu_net_read_log(&cursor,tail,sizeof tail)) {
        CHECK(!strstr(tail,"SECRET_TOKEN"));if(strstr(tail,"lifecycle survives trace limit"))found=1;
    }
    CHECK(found && cursor>0 && !ssbu_net_read_log(&cursor,tail,sizeof tail));
    return 0;
}

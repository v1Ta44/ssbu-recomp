/* Yuzu v1 room client. Protocol reference: yuzu-mainline 7ffac53c9e67,
 * src/network/room{,_member}.cpp, Citra/yuzu contributors, GPL-2.0-or-later.
 * Backend only: the worker owns ENet; callers exchange bounded copied state. */
#include "yuzu_net_internal.h"
#include "yuzu_packet.h"
#include <enet/enet.h>
#include <stdio.h>
#include <stdlib.h>
#include <limits.h>
#include <stdarg.h>

YnBackend ssbu_yn;
static INIT_ONCE once=INIT_ONCE_STATIC_INIT;
static SRWLOCK lifecycle=SRWLOCK_INIT;
static INIT_ONCE log_once=INIT_ONCE_STATIC_INIT;
static SRWLOCK log_lock=SRWLOCK_INIT;
static volatile LONG log_level=1;
static ULONGLONG log_started,log_window[4];
static unsigned log_count[4];
static uint64_t log_sequence,log_suppressed[4];
static char log_history[128][1100];
static BOOL CALLBACK log_initialize(PINIT_ONCE o,PVOID p,PVOID *c) {
    (void)o;(void)p;(void)c;
    wchar_t level[8];DWORD n=GetEnvironmentVariableW(L"SSBU_NET_LOG_LEVEL",level,8);
    if(n==1 && level[0]>=L'0' && level[0]<=L'3')log_level=level[0]-L'0';
    log_started=GetTickCount64();return TRUE;
}
int ssbu_net_set_log_level(uint32_t level) {
    if(level>3)return SSBU_NET_BAD_ARGUMENT;
    InitOnceExecuteOnce(&log_once,log_initialize,NULL,NULL);
    InterlockedExchange(&log_level,(LONG)level);return 0;
}
uint32_t ssbu_net_read_log(uint64_t *cursor,char *out,uint32_t capacity) {
    if(!cursor || !out || capacity<1100)return 0;
    InitOnceExecuteOnce(&log_once,log_initialize,NULL,NULL);
    AcquireSRWLockShared(&log_lock);
    uint64_t first=log_sequence>=128?log_sequence-127:1;
    uint64_t next=*cursor>=log_sequence?log_sequence+1:*cursor+1;
    if(next<first)next=first;
    uint32_t used=0;
    for(;next<=log_sequence;next++) {
        const char *line=log_history[(next-1)%128];size_t n=strlen(line);
        if(n>=capacity-used)break;
        memcpy(out+used,line,n);used+=(uint32_t)n;*cursor=next;
    }
    out[used]=0;ReleaseSRWLockShared(&log_lock);return used;
}
void yn_log(unsigned level,const char *component,const char *format,...) {
    InitOnceExecuteOnce(&log_once,log_initialize,NULL,NULL);
    if(!level || level>3 || level>(unsigned)InterlockedCompareExchange(&log_level,0,0))return;
    AcquireSRWLockExclusive(&log_lock);
    ULONGLONG now=GetTickCount64();
    /* Debug and packet traffic have independent budgets. Lifecycle lines still
     * pass when a busy game exhausts either budget. No payload is ever formatted. */
    if(level>=2) {
        if(now-log_window[level]>=1000){log_window[level]=now;log_count[level]=0;}
        if(log_count[level]++>=64){log_suppressed[level]++;ReleaseSRWLockExclusive(&log_lock);return;}
    }
    SYSTEMTIME t;GetSystemTime(&t);
    char body[768],line[1100];va_list args;va_start(args,format);vsnprintf(body,sizeof body,format,args);va_end(args);
    for(char *p=body;*p;p++)if((unsigned char)*p<32 || *p==127)*p='?';
    int n=snprintf(line,sizeof line,"[yuzu-net %04u-%02u-%02uT%02u:%02u:%02u.%03uZ +%llums pid=%lu tid=%lu #%llu L%u %s] %s [suppressed debug=%llu trace=%llu]\n",
        t.wYear,t.wMonth,t.wDay,t.wHour,t.wMinute,t.wSecond,t.wMilliseconds,
        (unsigned long long)(now-log_started),GetCurrentProcessId(),GetCurrentThreadId(),
        (unsigned long long)++log_sequence,level,component,body,
        (unsigned long long)log_suppressed[2],(unsigned long long)log_suppressed[3]);
    if(n>0){memcpy(log_history[(log_sequence-1)%128],line,strlen(line)+1);fwrite(line,1,(size_t)n<sizeof line?(size_t)n:sizeof line-1,stderr);}
    ReleaseSRWLockExclusive(&log_lock);
}

/* The Windows environment is UTF-16; getenv would use the machine's ANSI code
 * page, corrupting Chinese profile names and non-ASCII room passwords. */
static int env_utf8(const wchar_t *key,char *out,size_t capacity) {
    out[0]=0; DWORD n=GetEnvironmentVariableW(key,NULL,0);
    if (!n) return 0;
    if (n>capacity) return -1;
    wchar_t *wide=malloc((size_t)n*sizeof *wide); if (!wide) return -1;
    DWORD got=GetEnvironmentVariableW(key,wide,n);
    int ok=got && got<n && WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,wide,-1,out,(int)capacity,NULL,NULL);
    SecureZeroMemory(wide,(size_t)n*sizeof *wide); free(wide);
    if (!ok) { out[0]=0; return -1; }
    return 1;
}
static BOOL CALLBACK initialize(PINIT_ONCE o, PVOID p, PVOID *c) {
    (void)o; (void)p; (void)c;
    InitializeCriticalSection(&ssbu_yn.lock);
    InitializeConditionVariable(&ssbu_yn.changed);
    ssbu_yn.snapshot.size=sizeof ssbu_yn.snapshot; ssbu_yn.snapshot.abi=SSBU_NET_ABI;
    char name[33];
    if (env_utf8(L"SSBU_NICKNAME",name,sizeof name)>0 && !ssbu_net_validate_profile(name))
        memcpy(ssbu_yn.profile,name,strlen(name)+1);
    /* Missing/invalid identity stays empty. Never infer it from Windows or invent a player name. */
    return TRUE;
}
void yn_lock(void) { InitOnceExecuteOnce(&once,initialize,NULL,NULL); EnterCriticalSection(&ssbu_yn.lock); }
void yn_unlock(void) { LeaveCriticalSection(&ssbu_yn.lock); }
void yn_changed_locked(void) { ssbu_yn.activity++; ssbu_yn.snapshot.revision++; WakeAllConditionVariable(&ssbu_yn.changed); }
int yn_joined_locked(void) { return ssbu_yn.snapshot.state==SSBU_NET_JOINED && !ssbu_yn.stop; }
int yn_enabled(void) { yn_lock(); int v=ssbu_yn.enabled; yn_unlock(); return v; }
uint64_t yn_activity(void) { yn_lock(); uint64_t v=ssbu_yn.activity; yn_unlock(); return v; }
void yn_wait_activity(uint64_t previous,unsigned ms) {
    yn_lock(); if (ssbu_yn.activity==previous) SleepConditionVariableCS(&ssbu_yn.changed,&ssbu_yn.lock,ms); yn_unlock();
}
static void status_locked(unsigned state,unsigned error,const char *message) {
    static const char *names[]={"idle","connecting","joining","joined","failed"};
    if(state!=ssbu_yn.snapshot.state || error)
        yn_log(1,"room","state=%s->%s error=%u server_version=%u epoch=%llu reason=%s",
            names[ssbu_yn.snapshot.state],names[state],error,ssbu_yn.snapshot.server_version,(unsigned long long)ssbu_yn.epoch,message);
    ssbu_yn.snapshot.state=state; ssbu_yn.snapshot.error=error;
    snprintf(ssbu_yn.snapshot.message,sizeof ssbu_yn.snapshot.message,"%s",message);
    yn_changed_locked();
}
int yn_queue_locked(const void *data,size_t size) {
    if (size>YN_MAX_PACKET || ssbu_yn.send_bytes+ssbu_yn.transport_bytes+size>YN_QUEUE_BYTES || ssbu_yn.stop) {
        yn_log(2,"queue","send rejected bytes=%zu queued=%zu unacked=%zu stop=%d",size,ssbu_yn.send_bytes,ssbu_yn.transport_bytes,ssbu_yn.stop);return 0;
    }
    YnPacket *p=malloc(sizeof *p+size);
    if (!p) return 0;
    p->next=NULL; p->size=size; memcpy(p->bytes,data,size);
    if (ssbu_yn.send_tail) ssbu_yn.send_tail->next=p; else ssbu_yn.send_head=p;
    ssbu_yn.send_tail=p; ssbu_yn.send_bytes+=size;
    return 1;
}
static void clear_send_locked(void) {
    while (ssbu_yn.send_head) { YnPacket *p=ssbu_yn.send_head; ssbu_yn.send_head=p->next; free(p); }
    ssbu_yn.send_tail=NULL; ssbu_yn.send_bytes=0;
}
void yn_room_lost_locked(void) {
    if(ssbu_yn.snapshot.member_count)yn_log(1,"room","session ended epoch=%llu sent=%llu received=%llu dropped=%llu",
        (unsigned long long)ssbu_yn.epoch,(unsigned long long)ssbu_yn.snapshot.sent_packets,
        (unsigned long long)ssbu_yn.snapshot.received_packets,(unsigned long long)ssbu_yn.snapshot.dropped_packets);
    ssbu_yn.epoch++; clear_send_locked(); yn_ldn_lost_locked(); yn_socket_lost_locked();
    ssbu_yn.snapshot.member_count=0; memset(ssbu_yn.snapshot.ip,0,4); yn_changed_locked();
}
static int is_member_locked(const uint8_t *ip) {
    for (unsigned i=0;i<ssbu_yn.snapshot.member_count;i++)
        if (!memcmp(ip,ssbu_yn.snapshot.members[i].ip,4)) return 1;
    return 0;
}
static int room_info_locked(YnWire *w) {
    SsbuNetSnapshot *next=calloc(1,sizeof *next);
    if (!next) return 0;
    yw_get_string(w,next->room_name,sizeof next->room_name);
    yw_get_string(w,next->room_description,sizeof next->room_description);
    next->room_slots=(uint32_t)yw_read(w,4); next->room_port=(uint16_t)yw_read(w,2);
    char ignored[256]; yw_get_string(w,ignored,sizeof ignored); yw_get_string(w,ignored,sizeof ignored);
    next->member_count=(uint32_t)yw_read(w,4);
    if (next->member_count>SSBU_NET_MAX_MEMBERS) { free(next); return 0; }
    for (unsigned i=0;i<next->member_count && !w->bad;i++) {
        SsbuNetMember *m=&next->members[i];
        yw_get_string(w,m->nickname,sizeof m->nickname);
        const uint8_t *ip=yw_take(w,4); if (ip) memcpy(m->ip,ip,4);
        yw_get_string(w,m->game_name,sizeof m->game_name); m->game_id=yw_read(w,8);
        yw_get_string(w,m->game_version,sizeof m->game_version);
        yw_get_string(w,m->username,sizeof m->username); yw_get_string(w,m->display_name,sizeof m->display_name);
        yw_get_string(w,m->avatar_url,sizeof m->avatar_url);
    }
    if (w->bad || w->pos!=w->size) { free(next); return 0; }
    for (unsigned i=0;i<ssbu_yn.snapshot.member_count;i++) {
        unsigned j=0; for (;j<next->member_count;j++) if (!memcmp(ssbu_yn.snapshot.members[i].ip,next->members[j].ip,4)) break;
        if (j==next->member_count) {
            const uint8_t *ip=ssbu_yn.snapshot.members[i].ip;
            yn_log(1,"room","member left ip=%u.%u.%u.%u",ip[0],ip[1],ip[2],ip[3]);
            yn_ldn_member_left_locked(ip);
        }
    }
    yn_log(2,"room","member list count=%u slots=%u",next->member_count,next->room_slots);
    for(unsigned i=0;i<next->member_count;i++) {
        SsbuNetMember *m=&next->members[i];
        yn_log(2,"member","ip=%u.%u.%u.%u game=%016llx nickname_bytes=%zu verified=%d avatar_url_present=%d",
            m->ip[0],m->ip[1],m->ip[2],m->ip[3],(unsigned long long)m->game_id,strlen(m->nickname),!!m->username[0],!!m->avatar_url[0]);
    }
    memcpy(ssbu_yn.snapshot.room_name,next->room_name,sizeof next->room_name);
    memcpy(ssbu_yn.snapshot.room_description,next->room_description,sizeof next->room_description);
    ssbu_yn.snapshot.room_slots=next->room_slots; ssbu_yn.snapshot.room_port=next->room_port;
    ssbu_yn.snapshot.member_count=next->member_count;
    memcpy(ssbu_yn.snapshot.members,next->members,sizeof next->members);
    free(next); yn_changed_locked(); return 1;
}
static void received_locked(const uint8_t *data,size_t size) {
    if (!size || size>YN_MAX_PACKET) { ssbu_yn.snapshot.dropped_packets++; yn_log(2,"room","drop invalid packet size=%zu",size); return; }
    YnWire w={(uint8_t *)data,size,0,0}; unsigned id=(unsigned)yw_read(&w,1);
    ssbu_yn.snapshot.received_packets++;
    yn_log(3,"room","rx message=%u bytes=%zu",id,size);
    if (id==3) { if (!room_info_locked(&w)) goto malformed; return; }
    if (id==2 || id==24) {
        const uint8_t *ip=yw_take(&w,4);
        if (!ip || w.pos!=w.size || ssbu_yn.snapshot.state!=SSBU_NET_JOINING) goto malformed;
        if (!is_member_locked(ip)) goto malformed;
        memcpy(ssbu_yn.snapshot.ip,ip,4); status_locked(SSBU_NET_JOINED,0,"Joined Yuzu room");
        yn_log(1,"room","assigned ip=%u.%u.%u.%u moderator=%d",ip[0],ip[1],ip[2],ip[3],id==24);return;
    }
    if (id==5 || id==6) {
        if (!yn_joined_locked()) return;
        if (id==5) { yn_socket_receive_locked(data,size); return; }
        unsigned type=(unsigned)yw_read(&w,1);
        const uint8_t *src=yw_take(&w,4),*dst=yw_take(&w,4);
        unsigned broadcast=(unsigned)yw_read(&w,1);
        size_t n=(size_t)yw_read(&w,4); const uint8_t *payload=yw_take(&w,n);
        if (!src || !dst || !payload || w.bad || w.pos!=w.size || broadcast>1 || type>5) goto malformed;
        if (!is_member_locked(src) || !memcmp(src,ssbu_yn.snapshot.ip,4) || (!broadcast && memcmp(dst,ssbu_yn.snapshot.ip,4))) {
            yn_log(2,"ldn","ignore packet: source membership or destination mismatch type=%u",type);return;
        }
        yn_ldn_receive_locked(type,src,payload,n); return;
    }
    unsigned error=0; const char *message=NULL;
    switch (id) {
    case 8: error=SSBU_NET_NAME; message="Nickname rejected or already in use"; break;
    case 9: error=SSBU_NET_IP; message="Virtual IP collision"; break;
    case 10:
        ssbu_yn.snapshot.server_version=(uint32_t)yw_read(&w,4); if (w.bad) goto malformed;
        error=SSBU_NET_VERSION; message="Room protocol version mismatch"; break;
    case 11: error=SSBU_NET_PASSWORD; message="Wrong room password"; break;
    case 12: error=SSBU_NET_CLOSED; message="Room closed"; break;
    case 13: error=SSBU_NET_FULL; message="Room is full"; break;
    case 15: error=SSBU_NET_KICKED; message="Removed from room"; break;
    case 16: error=SSBU_NET_BANNED; message="Banned from room"; break;
    default: return; /* Chat/moderation are not required by this backend. */
    }
    ssbu_yn.stop=1; status_locked(SSBU_NET_FAILED,error,message); return;
malformed:
    yn_log(2,"room","drop malformed message=%u bytes=%zu offset=%zu",id,size,w.pos);
    ssbu_yn.snapshot.dropped_packets++;
    /* Bad control messages fail the handshake; bad peer packets are discarded. */
    if (ssbu_yn.snapshot.state!=SSBU_NET_JOINED) { ssbu_yn.stop=1; status_locked(SSBU_NET_FAILED,SSBU_NET_MALFORMED,"Malformed room response"); }
}
static void ENET_CALLBACK sent_packet_free(ENetPacket *packet) {
    yn_lock(); ssbu_yn.transport_bytes-=packet->dataLength; yn_changed_locked(); yn_unlock();
}
static DWORD WINAPI room_worker(void *unused) {
    (void)unused;
    ENetHost *host=NULL; ENetPeer *peer=NULL; int initialized=0;
    const char *phase="enet_initialize";
    yn_lock(); SsbuNetConfig config=ssbu_yn.config; yn_unlock();
    yn_log(1,"room","connect protocol=1 reference=7ffac53c9e67 host=%s port=%u game=%016llx",config.host,config.port,(unsigned long long)config.game_id);
    if (enet_initialize()!=0) goto io_error;
    initialized=1;phase="host_create";host=enet_host_create(NULL,1,1,0,0); if (!host) goto io_error;
    host->maximumPacketSize=YN_MAX_PACKET; host->maximumWaitingData=YN_QUEUE_BYTES;
    ENetAddress addr={0}; addr.port=config.port;
    phase="resolve_host";if (enet_address_set_host(&addr,config.host)!=0) goto io_error;
    yn_lock(); int stop=ssbu_yn.stop; yn_unlock(); if (stop) goto done;
    phase="host_connect";peer=enet_host_connect(host,&addr,1,0); if (!peer) goto io_error;
    enet_peer_timeout(peer,16,5000,10000);
    ULONGLONG deadline=GetTickCount64()+10000;
    ULONGLONG next_stats=GetTickCount64()+5000;
    for (;;) {
        yn_lock(); stop=ssbu_yn.stop; unsigned state=ssbu_yn.snapshot.state; yn_unlock(); if (stop) break;
        if (state!=SSBU_NET_JOINED && GetTickCount64()>deadline) {
            yn_lock(); status_locked(SSBU_NET_FAILED,SSBU_NET_TIMEOUT,"Room connection timed out"); yn_unlock(); break;
        }
        phase="host_service";ENetEvent event; int result=enet_host_service(host,&event,5);
        if (result<0) goto io_error;
        if (result>0) {
            if (event.type==ENET_EVENT_TYPE_CONNECT) {
                uint8_t bytes[8192]; YnWire w={bytes,sizeof bytes,0,0};
                yw_write(&w,1,1); yw_string(&w,config.nickname); yw_write(&w,0xffffffff,4);
                yw_write(&w,1,4); yw_string(&w,config.password); yw_string(&w,config.token);
                yn_lock(); status_locked(SSBU_NET_JOINING,0,"Waiting for room admission");
                if (w.bad || !yn_queue_locked(bytes,w.pos)) { ssbu_yn.stop=1; status_locked(SSBU_NET_FAILED,SSBU_NET_OVERFLOW,"Room send queue full"); }
                w.pos=0; w.bad=0; yw_write(&w,4,1); yw_string(&w,config.game_name); yw_write(&w,config.game_id,8); yw_string(&w,config.game_version);
                if (w.bad || !yn_queue_locked(bytes,w.pos)) ssbu_yn.stop=1;
                yn_unlock();
            } else if (event.type==ENET_EVENT_TYPE_RECEIVE) {
                yn_lock(); received_locked(event.packet->data,event.packet->dataLength); yn_unlock(); enet_packet_destroy(event.packet);
            } else if (event.type==ENET_EVENT_TYPE_DISCONNECT) {
                yn_log(1,"transport","disconnect event data=%u rtt_ms=%u lost_packets=%u",event.data,peer->roundTripTime,peer->packetsLost);
                yn_lock(); status_locked(SSBU_NET_FAILED,SSBU_NET_CLOSED,"Room connection lost"); yn_unlock(); break;
            }
        }
        yn_lock(); YnPacket *list=ssbu_yn.send_head; ssbu_yn.send_head=ssbu_yn.send_tail=NULL;
        ssbu_yn.transport_bytes+=ssbu_yn.send_bytes; ssbu_yn.send_bytes=0;
        if (list) yn_changed_locked(); yn_unlock();
        while (list) {
            YnPacket *p=list; list=p->next;
            yn_log(3,"room","tx message=%u bytes=%zu",p->bytes[0],p->size);
            phase="peer_send";
            ENetPacket *ep=enet_packet_create(p->bytes,p->size,ENET_PACKET_FLAG_RELIABLE);
            if (ep) ep->freeCallback=sent_packet_free;
            else { yn_lock(); ssbu_yn.transport_bytes-=p->size; yn_unlock(); }
            free(p);
            if (!ep || enet_peer_send(peer,0,ep)<0) {
                if (ep) enet_packet_destroy(ep);
                while (list) { p=list; list=p->next; yn_lock(); ssbu_yn.transport_bytes-=p->size; yn_unlock(); free(p); }
                goto io_error;
            }
            yn_lock(); ssbu_yn.snapshot.sent_packets++; yn_unlock();
        }
        enet_host_flush(host);
        if(GetTickCount64()>=next_stats) {
            yn_lock();yn_log(2,"stats","epoch=%llu state=%u ldn=%u members=%u sent=%llu received=%llu dropped=%llu queued=%zu unacked=%zu rtt_ms=%u lost_packets=%u",
                (unsigned long long)ssbu_yn.epoch,ssbu_yn.snapshot.state,ssbu_yn.ldn_state,ssbu_yn.snapshot.member_count,
                (unsigned long long)ssbu_yn.snapshot.sent_packets,(unsigned long long)ssbu_yn.snapshot.received_packets,
                (unsigned long long)ssbu_yn.snapshot.dropped_packets,ssbu_yn.send_bytes,ssbu_yn.transport_bytes,peer->roundTripTime,peer->packetsLost);yn_unlock();
            next_stats=GetTickCount64()+5000;
        }
    }
    goto done;
io_error:
    yn_log(1,"transport","failure phase=%s wsa_error=%d win_error=%lu (OS codes may be unset)",phase,WSAGetLastError(),GetLastError());
    yn_lock(); if (!ssbu_yn.stop) status_locked(SSBU_NET_FAILED,SSBU_NET_IO,"Room transport error"); yn_unlock();
done:
    if (peer) { enet_peer_disconnect_now(peer,0); enet_host_flush(host); }
    if (host) enet_host_destroy(host);
    if (initialized) enet_deinitialize();
    yn_lock(); yn_room_lost_locked(); SecureZeroMemory(ssbu_yn.config.password,sizeof ssbu_yn.config.password);
    SecureZeroMemory(ssbu_yn.config.token,sizeof ssbu_yn.config.token); yn_unlock();
    SecureZeroMemory(&config,sizeof config);
    return 0;
}
static int terminated(const char *s,size_t cap) { return memchr(s,0,cap)!=NULL; }
static int valid_nickname(const char *s) {
    size_t n=strlen(s); if (n<4 || n>20) return 0;
    for (;*s;s++) if (!( (*s>='a' && *s<='z') || (*s>='A' && *s<='Z') || (*s>='0' && *s<='9') || strchr(" ._-",*s))) return 0;
    return 1;
}
static void leave_locked_lifecycle(void) {
    yn_lock(); ssbu_yn.stop=1; yn_changed_locked(); HANDLE worker=ssbu_yn.worker; yn_unlock();
    if (worker) { WaitForSingleObject(worker,INFINITE); CloseHandle(worker); }
    yn_lock(); ssbu_yn.worker=NULL; ssbu_yn.enabled=0; yn_room_lost_locked(); status_locked(SSBU_NET_IDLE,0,"Disconnected"); yn_unlock();
}
int ssbu_net_join(const SsbuNetConfig *c) {
    if (!c || c->size!=sizeof *c || c->abi!=SSBU_NET_ABI ||
        !terminated(c->host,sizeof c->host) || !c->host[0] || !terminated(c->nickname,sizeof c->nickname) ||
        !terminated(c->password,sizeof c->password) || !terminated(c->token,sizeof c->token) ||
        !terminated(c->game_name,sizeof c->game_name) || !terminated(c->game_version,sizeof c->game_version) || !valid_nickname(c->nickname)) return SSBU_NET_BAD_ARGUMENT;
    AcquireSRWLockExclusive(&lifecycle); yn_lock();
    if ((ssbu_yn.snapshot.state>=SSBU_NET_CONNECTING && ssbu_yn.snapshot.state<=SSBU_NET_JOINED) || ssbu_yn.ldn_state>1) {
        yn_unlock(); ReleaseSRWLockExclusive(&lifecycle); return SSBU_NET_BUSY;
    }
    yn_unlock(); leave_locked_lifecycle(); yn_lock();
    ssbu_yn.config=*c; if (!ssbu_yn.config.port) ssbu_yn.config.port=24872;
    ssbu_yn.stop=0; ssbu_yn.enabled=1; ssbu_yn.snapshot.server_version=0; ssbu_yn.epoch++;
    status_locked(SSBU_NET_CONNECTING,0,"Connecting to Yuzu room");
    ssbu_yn.worker=CreateThread(NULL,0,room_worker,NULL,0,NULL);
    if (!ssbu_yn.worker) { ssbu_yn.enabled=0; status_locked(SSBU_NET_FAILED,SSBU_NET_IO,"Cannot start room worker"); }
    int r=ssbu_yn.worker?0:SSBU_NET_IO; yn_unlock(); ReleaseSRWLockExclusive(&lifecycle); return r;
}
void ssbu_net_leave(void) { AcquireSRWLockExclusive(&lifecycle); leave_locked_lifecycle(); ReleaseSRWLockExclusive(&lifecycle); }
int ssbu_net_snapshot(SsbuNetSnapshot *out) {
    if (!out || out->size!=sizeof *out || out->abi!=SSBU_NET_ABI) return SSBU_NET_BAD_ARGUMENT;
    yn_lock(); *out=ssbu_yn.snapshot; yn_unlock(); return 0;
}
int ssbu_net_ldn_snapshot(SsbuNetLdnSnapshot *out) {
    if (!out || out->size!=sizeof *out || out->abi!=SSBU_NET_ABI) return SSBU_NET_BAD_ARGUMENT;
    yn_lock();
    out->room_state=ssbu_yn.snapshot.state; out->ldn_state=ssbu_yn.ldn_state;
    out->node_count=(ssbu_yn.snapshot.state==SSBU_NET_JOINED && (ssbu_yn.ldn_state==3 || ssbu_yn.ldn_state==5))?ssbu_yn.network[0x67]:0;
    out->reserved=0; out->room_epoch=ssbu_yn.epoch; out->ldn_generation=ssbu_yn.ldn_generation;
    yn_unlock(); return SSBU_NET_OK;
}
static int profile_busy_locked(void) { return (ssbu_yn.snapshot.state>SSBU_NET_IDLE && ssbu_yn.snapshot.state<SSBU_NET_FAILED) || ssbu_yn.ldn_state>1; }
int ssbu_net_validate_profile(const char *s) {
    if (!s) return SSBU_NET_BAD_ARGUMENT;
    size_t n=strnlen(s,33);
    if (!n || n>32) return SSBU_NET_BAD_ARGUMENT;
    wchar_t wide[33]; WORD types[33];
    int length=MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,s,(int)n,wide,33);
    if (!length || !GetStringTypeW(CT_CTYPE1,wide,length,types)) return SSBU_NET_BAD_ARGUMENT;
    int visible=0;
    for (int i=0;i<length;i++) {
        if (types[i]&C1_CNTRL) return SSBU_NET_BAD_ARGUMENT;
        if (!(types[i]&C1_SPACE)) visible=1;
    }
    return visible ? 0 : SSBU_NET_BAD_ARGUMENT;
}
int ssbu_net_set_profile(const char *s) {
    if (ssbu_net_validate_profile(s)) return SSBU_NET_BAD_ARGUMENT;
    size_t n=strlen(s);
    yn_lock(); if (profile_busy_locked()) { yn_unlock(); return SSBU_NET_BUSY; }
    memset(ssbu_yn.profile,0,33); memcpy(ssbu_yn.profile,s,n); yn_changed_locked(); yn_unlock(); return 0;
}
void ssbu_net_get_profile(char out[33]) { if (out) { yn_lock(); memcpy(out,ssbu_yn.profile,33); yn_unlock(); } }
int ssbu_net_set_avatar(uint32_t format,const void *data,uint32_t size) {
    const uint8_t *p=data;
    if ((format==0 && size) || format>2 || size>SSBU_NET_AVATAR_MAX || (format && (!p || !size))) return SSBU_NET_BAD_ARGUMENT;
    if (format==1 && (size<8 || memcmp(p,"\x89PNG\r\n\x1a\n",8))) return SSBU_NET_BAD_ARGUMENT;
    if (format==2 && (size<4 || p[0]!=255 || p[1]!=216 || p[size-2]!=255 || p[size-1]!=217)) return SSBU_NET_BAD_ARGUMENT;
    uint8_t *copy=size?malloc(size):NULL; if (size && !copy) return SSBU_NET_IO;
    if (size) memcpy(copy,p,size);
    yn_lock(); if (profile_busy_locked()) { yn_unlock(); free(copy); return SSBU_NET_BUSY; }
    free(ssbu_yn.avatar); ssbu_yn.avatar=copy; ssbu_yn.avatar_size=size; ssbu_yn.avatar_format=format; yn_changed_locked(); yn_unlock(); return 0;
}
uint32_t ssbu_net_get_avatar(uint32_t *format,void *data,uint32_t capacity) {
    yn_lock(); uint32_t n=ssbu_yn.avatar_size; if (format) *format=ssbu_yn.avatar_format;
    if (data && capacity>=n && n) memcpy(data,ssbu_yn.avatar,n); yn_unlock(); return n;
}
void ssbu_net_boot(void) {
    atexit(ssbu_net_leave);
    SsbuNetConfig c={0}; c.size=sizeof c; c.abi=SSBU_NET_ABI;
    int host=env_utf8(L"SSBU_NET_HOST",c.host,sizeof c.host); if (!host) return;
    char port_text[16];
    if (host<0 || env_utf8(L"SSBU_NET_PORT",port_text,sizeof port_text)<0 ||
        env_utf8(L"SSBU_NET_NICKNAME",c.nickname,sizeof c.nickname)<0 ||
        env_utf8(L"SSBU_NET_PASSWORD",c.password,sizeof c.password)<0 ||
        env_utf8(L"SSBU_NET_TOKEN",c.token,sizeof c.token)<0) {
        SecureZeroMemory(&c,sizeof c); yn_log(1,"config","invalid or oversized environment configuration"); return;
    }
    if (*port_text) { char *end; long port=strtol(port_text,&end,10); if (*end || port<1 || port>65535) { SecureZeroMemory(&c,sizeof c); yn_log(1,"config","invalid SSBU_NET_PORT"); return; } c.port=(uint16_t)port; }
    if (!c.nickname[0]) strcpy(c.nickname,"PCPlayer");
    strcpy(c.game_name,"Super Smash Bros. Ultimate"); strcpy(c.game_version,"13.0.4"); c.game_id=0x01006a800016e000ULL;
    int r=ssbu_net_join(&c); SecureZeroMemory(&c,sizeof c);
    if (r) yn_log(1,"config","configuration rejected error=%d",r);
    else {
        /* Environment startup precedes guest socket creation. The public Join API
         * stays asynchronous; a frontend must wait for JOINED before local play. */
        ULONGLONG end=GetTickCount64()+12000;
        yn_lock();
        while (ssbu_yn.snapshot.state==SSBU_NET_CONNECTING || ssbu_yn.snapshot.state==SSBU_NET_JOINING) {
            ULONGLONG now=GetTickCount64(); if (now>=end) break;
            SleepConditionVariableCS(&ssbu_yn.changed,&ssbu_yn.lock,(DWORD)(end-now));
        }
        if (ssbu_yn.snapshot.state!=SSBU_NET_JOINED) {
            if (ssbu_yn.snapshot.state!=SSBU_NET_FAILED) {
                ssbu_yn.stop=1; status_locked(SSBU_NET_FAILED,SSBU_NET_TIMEOUT,"Startup admission timed out");
            }
            yn_log(1,"boot","admission failed error=%u reason=%s",ssbu_yn.snapshot.error,ssbu_yn.snapshot.message);
        } else yn_log(1,"boot","admission complete; guest networking may start");
        yn_unlock();
    }
}

/* Virtual datagrams carried by Yuzu ProxyPacket (mainline 7ffac53c9e67,
 * socket_proxy.cpp, Copyright 2022 yuzu, GPL-2.0-or-later). */
#include "yuzu_net_internal.h"
#include "yuzu_packet.h"
#include <zstd.h>
#include <stdlib.h>
#include <string.h>
#define SOCKET_COUNT 128
#define DATAGRAM_MAX 65507
typedef struct Datagram { struct Datagram *next; struct sockaddr_in src; size_t size; uint8_t bytes[1]; } Datagram;
typedef struct {
    int fd, protocol, nonblock, broadcast, reuse, bound, connected, shut_read, shut_write, lost;
    uint32_t recv_timeout, send_timeout, recv_limit, send_limit;
    uint64_t epoch;
    struct sockaddr_in local, peer;
    Datagram *head,*tail;
    size_t queued;
    unsigned packets;
} VirtualSocket;
static VirtualSocket sockets[SOCKET_COUNT];
static unsigned serial;
static uint64_t reset_sequence;
static size_t received_bytes;
static uint16_t next_port=49152;
static VirtualSocket *find(int fd) { for (unsigned i=0;i<SOCKET_COUNT;i++) if (sockets[i].fd==fd && fd>=YN_VFD_BASE) return &sockets[i]; return NULL; }
static void drain(VirtualSocket *s) {
    while (s->head) { Datagram *p=s->head; s->head=p->next; free(p); }
    received_bytes-=s->queued; s->tail=NULL; s->queued=0; s->packets=0;
}
void yn_socket_reset_locked(void) { reset_sequence++; for (unsigned i=0;i<SOCKET_COUNT;i++) drain(&sockets[i]); yn_changed_locked(); }
void yn_socket_lost_locked(void) {
    for (unsigned i=0;i<SOCKET_COUNT;i++) if(sockets[i].fd) { sockets[i].lost=1; drain(&sockets[i]); }
    yn_changed_locked();
}
void yn_socket_close_all(void) {
    yn_lock(); for (unsigned i=0;i<SOCKET_COUNT;i++) { drain(&sockets[i]); memset(&sockets[i],0,sizeof sockets[i]); }
    yn_changed_locked(); yn_unlock();
}
int yn_socket_create(int type,int protocol) {
    if (type!=SOCK_DGRAM || (protocol!=0 && protocol!=IPPROTO_UDP)) {
        yn_log(2,"socket","create unsupported type=%d protocol=%d errno=93",type,protocol);return -93;
    }
    yn_lock(); int fd=-24;
    if (!ssbu_yn.enabled || !yn_joined_locked()) fd=-100;
    else if (serial>=0x3fffffff-YN_VFD_BASE) fd=-24;
    else for (unsigned i=0;i<SOCKET_COUNT;i++) if (!sockets[i].fd) {
        VirtualSocket *s=&sockets[i]; memset(s,0,sizeof *s); fd=s->fd=YN_VFD_BASE+(int)++serial;
        s->protocol=protocol==0?0:3; s->epoch=ssbu_yn.epoch; s->recv_limit=s->send_limit=1024*1024;
        s->local.sin_family=AF_INET; break;
    }
    yn_log(2,"socket","create type=%d protocol=%d fd_or_error=%d epoch=%llu",type,protocol,fd,(unsigned long long)ssbu_yn.epoch);
    yn_unlock(); return fd;
}
int yn_socket_close(int fd) {
    yn_lock(); VirtualSocket *s=find(fd); int r=s?0:-9;
    if (s) { drain(s); memset(s,0,sizeof *s); yn_changed_locked(); }
    yn_log(2,"socket","close fd=%d result=%d",fd,r);yn_unlock(); return r;
}
static int status(VirtualSocket *s) {
    if (!s) return -9;
    if (s->lost || s->epoch!=ssbu_yn.epoch || !yn_joined_locked()) return -100;
    return 0;
}
static int bind_locked(VirtualSocket *s,const struct sockaddr_in *addr) {
    if (s->bound) return -22;
    struct sockaddr_in a=*addr;
    if (!a.sin_port) {
        unsigned tries=0;
        for (;tries<16384;tries++) {
            uint16_t p=next_port++; if (next_port<49152) next_port=49152;
            unsigned i=0; for (;i<SOCKET_COUNT;i++) if (sockets[i].fd && sockets[i].bound && sockets[i].local.sin_port==htons(p)) break;
            if (i==SOCKET_COUNT) { a.sin_port=htons(p); break; }
        }
        if (tries==16384) return -98;
    }
    for (unsigned i=0;i<SOCKET_COUNT;i++) {
        VirtualSocket *other=&sockets[i];
        if (other!=s && other->fd && !other->lost && other->bound && other->local.sin_port==a.sin_port && !(other->reuse && s->reuse)) return -98;
    }
    /* Binding is virtual; an address reported by the game is not a host NIC. */
    s->local=a; s->bound=1; return 0;
}
int yn_socket_bind(int fd,const struct sockaddr_in *addr) {
    if (!addr || addr->sin_family!=AF_INET) return -97;
    yn_lock(); VirtualSocket *s=find(fd); int r=status(s); if(!r)r=bind_locked(s,addr);
    const uint8_t *ip=(const uint8_t *)&addr->sin_addr;
    yn_log(2,"socket","bind fd=%d ip=%u.%u.%u.%u port=%u result=%d",fd,ip[0],ip[1],ip[2],ip[3],!r?ntohs(s->local.sin_port):ntohs(addr->sin_port),r);
    yn_unlock(); return r;
}
int yn_socket_connect(int fd,const struct sockaddr_in *addr) {
    if (!addr || addr->sin_family!=AF_INET) return -97;
    yn_lock(); VirtualSocket *s=find(fd); int r=status(s);
    if (!r) { s->peer=*addr; s->connected=1; } yn_unlock(); return r;
}
int yn_socket_name(int fd,struct sockaddr_in *addr,int peer) {
    yn_lock(); VirtualSocket *s=find(fd); int r=s?0:-9;
    if (s && peer && !s->connected) r=-107;
    if(!r)*addr=peer?s->peer:s->local; yn_unlock(); return r;
}
int yn_socket_fcntl(int fd,int cmd,int flags) {
    yn_lock(); VirtualSocket *s=find(fd); int r=s?0:-9;
    if (s) {
        if (cmd==3) r=s->nonblock?0x800:0;
        else if (cmd==4) s->nonblock=!!(flags&0x800);
        else r=-22;
    }
    yn_unlock(); return r;
}
int yn_socket_shutdown(int fd,int how) {
    if ((unsigned)how>2) return -22;
    yn_lock(); VirtualSocket *s=find(fd); int r=s?0:-9;
    if(s) { if (how!=1) s->shut_read=1; if (how!=0) s->shut_write=1; yn_changed_locked(); }
    yn_unlock(); return r;
}
int yn_socket_poll(int fd,int events) {
    yn_lock(); VirtualSocket *s=find(fd); int r=0;
    if(!s)r=0x20;
    else if(status(s))r=0x18;
    else {
        if(s->head || s->shut_read)r|=events&0x41;
        if(!s->shut_write && s->send_limit && ssbu_yn.send_bytes+ssbu_yn.transport_bytes<YN_QUEUE_BYTES)r|=events&4;
    }
    yn_unlock(); return r;
}
int yn_socket_recv(int fd,void *buf,size_t size,int flags,struct sockaddr_in *src) {
    if (!buf && size) return -22;
    if (flags&~(2|0x80)) return -95;
    yn_lock(); VirtualSocket *s=find(fd); int r=status(s);
    if(r){yn_unlock();return r;}
    ULONGLONG end=s->recv_timeout?GetTickCount64()+s->recv_timeout:0;
    uint64_t sequence=reset_sequence;
    for (;;) {
        s=find(fd); r=status(s); if(r)break;
        if(sequence!=reset_sequence){r=-104;break;}
        if(s->shut_read){r=0;break;}
        if(s->head) {
            Datagram *d=s->head; size_t n=d->size<size?d->size:size;
            if(n)memcpy(buf,d->bytes,n); if(src)*src=d->src; r=(int)n;
            if(!(flags&2)){s->head=d->next;if(!s->head)s->tail=NULL;s->queued-=d->size;received_bytes-=d->size;s->packets--;free(d);} break;
        }
        if (s->nonblock || (flags&0x80)) {r=-11;break;}
        DWORD ms=INFINITE;
        if(end){ULONGLONG now=GetTickCount64();if(now>=end){r=-11;break;}ms=(DWORD)(end-now);}
        SleepConditionVariableCS(&ssbu_yn.changed,&ssbu_yn.lock,ms);
    }
    yn_log(3,"socket","recv fd=%d capacity=%zu flags=0x%x result=%d",fd,size,flags,r);
    yn_unlock();return r;
}
int yn_socket_send(int fd,const void *buf,size_t size,int flags,const struct sockaddr_in *dst) {
    if(size>DATAGRAM_MAX)return -90;
    if(!buf && size)return -22;
    if(flags&~0x80)return -95;
    yn_lock(); VirtualSocket *s=find(fd);int r=status(s);
    if(r){yn_unlock();return r;}
    if(s->shut_write){yn_unlock();return -32;}
    if(!dst){if(!s->connected){yn_unlock();return -89;}dst=&s->peer;}
    if(dst->sin_family!=AF_INET){yn_unlock();return -97;}
    if(!s->bound){struct sockaddr_in a={0};a.sin_family=AF_INET;r=bind_locked(s,&a);if(r){yn_unlock();return r;}}
    const uint8_t *ip=(const uint8_t *)&dst->sin_addr;
    yn_log(3,"socket","send fd=%d dst=%u.%u.%u.%u:%u protocol=%d bytes=%zu",fd,ip[0],ip[1],ip[2],ip[3],ntohs(dst->sin_port),s->protocol,size);
    int broadcast=s->broadcast && ip[3]==255;
    if(ip[3]==255 && !s->broadcast){yn_unlock();return -13;}
    size_t capacity=ZSTD_compressBound(size)+32;uint8_t *bytes=malloc(capacity);
    if(!bytes){yn_unlock();return -105;}
    YnWire w={bytes,capacity,0,0};yw_write(&w,5,1);
    yw_write(&w,1,1);yw_bytes(&w,ssbu_yn.snapshot.ip,4);yw_write(&w,ntohs(s->local.sin_port),2);
    yw_write(&w,1,1);yw_bytes(&w,ip,4);yw_write(&w,ntohs(dst->sin_port),2);
    yw_write(&w,s->protocol,1);yw_write(&w,broadcast,1);
    size_t length_pos=w.pos;yw_write(&w,0,4);
    size_t compressed=ZSTD_compress(bytes+w.pos,capacity-w.pos,buf,size,3);
    if(ZSTD_isError(compressed))r=-105;
    else {
        size_t payload_pos=w.pos;w.pos=length_pos;yw_write(&w,compressed,4);w.pos=payload_pos+compressed;
        ULONGLONG end=s->send_timeout?GetTickCount64()+s->send_timeout:0;
        uint64_t sequence=reset_sequence;
        for (;;) {
            s=find(fd);r=status(s);if(r)break;
            if(sequence!=reset_sequence){r=-104;break;}
            if(s->shut_write){r=-32;break;}
            if(size>s->send_limit){r=-90;break;}
            if(ssbu_yn.send_bytes+ssbu_yn.transport_bytes+w.pos<=YN_QUEUE_BYTES){r=yn_queue_locked(bytes,w.pos)?(int)size:-105;break;}
            if(s->nonblock || (flags&0x80)){r=-11;break;}
            DWORD ms=INFINITE;
            if(end){ULONGLONG now=GetTickCount64();if(now>=end){r=-11;break;}ms=(DWORD)(end-now);}
            SleepConditionVariableCS(&ssbu_yn.changed,&ssbu_yn.lock,ms);
        }
    }
    yn_log(3,"socket","send complete fd=%d result=%d queued=%zu unacked=%zu",fd,r,ssbu_yn.send_bytes,ssbu_yn.transport_bytes);
    free(bytes);yn_unlock();return r;
}
void yn_socket_receive_locked(const uint8_t *bytes,size_t size) {
    const char *reason="invalid header or protocol";
    YnWire w={(uint8_t *)bytes,size,1,0};
    unsigned family=(unsigned)yw_read(&w,1);const uint8_t *src=yw_take(&w,4);uint16_t sport=(uint16_t)yw_read(&w,2);
    unsigned dfamily=(unsigned)yw_read(&w,1);const uint8_t *dst=yw_take(&w,4);uint16_t dport=(uint16_t)yw_read(&w,2);
    unsigned protocol=(unsigned)yw_read(&w,1),broadcast=(unsigned)yw_read(&w,1);
    size_t n=(size_t)yw_read(&w,4);const uint8_t *payload=yw_take(&w,n);
    if(w.bad || w.pos!=w.size || !src || !dst || !payload || family!=1 || dfamily!=1 || broadcast>1 || (protocol!=0 && protocol!=3))goto bad;
    reason="destination mismatch";if(!broadcast && memcmp(dst,ssbu_yn.snapshot.ip,4))goto bad;
    unsigned i=0;for(;i<ssbu_yn.snapshot.member_count;i++)if(!memcmp(src,ssbu_yn.snapshot.members[i].ip,4))break;
    reason="unknown or self source";if(i==ssbu_yn.snapshot.member_count || !memcmp(src,ssbu_yn.snapshot.ip,4))goto bad;
    reason="allocation failed";uint8_t *raw=malloc(DATAGRAM_MAX);if(!raw)goto bad;
    size_t rawsize=ZSTD_decompress(raw,DATAGRAM_MAX,payload,n);
    if(ZSTD_isError(rawsize)){reason="zstd decode failed or oversized datagram";free(raw);goto bad;}
    unsigned delivered=0;
    for(i=0;i<SOCKET_COUNT;i++) {
        VirtualSocket *s=&sockets[i];
        if(!s->fd || s->lost || s->epoch!=ssbu_yn.epoch || s->shut_read || !s->bound || s->protocol!=(int)protocol || ntohs(s->local.sin_port)!=dport || (broadcast && !s->broadcast))continue;
        if(s->connected && (s->peer.sin_port!=htons(sport) || memcmp(&s->peer.sin_addr,src,4)))continue;
        if(s->queued+rawsize>s->recv_limit || received_bytes+rawsize>YN_QUEUE_BYTES || s->packets>=256){ssbu_yn.snapshot.dropped_packets++;yn_log(2,"proxy","drop receive queue full fd=%d bytes=%zu queued=%zu packets=%u",s->fd,rawsize,s->queued,s->packets);continue;}
        Datagram *d=calloc(1,sizeof *d+rawsize);if(!d){ssbu_yn.snapshot.dropped_packets++;continue;}
        d->src.sin_family=AF_INET;d->src.sin_port=htons(sport);memcpy(&d->src.sin_addr,src,4);d->size=rawsize;
        if(rawsize)memcpy(d->bytes,raw,rawsize);
        if(s->tail)s->tail->next=d;else s->head=d;s->tail=d;s->queued+=rawsize;received_bytes+=rawsize;s->packets++;
        delivered++;
    }
    yn_log(3,"proxy","rx src=%u.%u.%u.%u:%u dst_port=%u protocol=%u broadcast=%u compressed=%zu raw=%zu delivered=%u",
        src[0],src[1],src[2],src[3],sport,dport,protocol,broadcast,n,rawsize,delivered);
    if(!delivered)yn_log(2,"proxy","not delivered: no eligible socket or full queues dst_port=%u protocol=%u broadcast=%u",dport,protocol,broadcast);
    free(raw);yn_changed_locked();return;
bad:ssbu_yn.snapshot.dropped_packets++;yn_log(2,"proxy","drop bytes=%zu reason=%s",size,reason);
}
int yn_socket_option(int fd,int set,int level,int name,void *value,uint32_t *size) {
    if(!value || !size)return -22;
    yn_lock();VirtualSocket *s=find(fd);int r=s?0:-9;
    if(!s){yn_unlock();return r;}
    if(level!=0xffff){yn_unlock();return -92;}
    uint32_t v=0;int timeout=name==0x1005 || name==0x1006;
    if(set) {
        if(timeout && *size>=16){int64_t tv[2];memcpy(tv,value,16);if(tv[0]<0 || tv[1]<0 || tv[1]>=1000000 || tv[0]>4294966){yn_unlock();return -22;}v=(uint32_t)(tv[0]*1000+(tv[1]+999)/1000);}
        else if(*size>=4)memcpy(&v,value,4);else{yn_unlock();return -22;}
    }
    switch(name) {
    case 0x20: if(set)s->broadcast=!!v;v=s->broadcast;break;
    case 4: if(set)s->reuse=!!v;v=s->reuse;break;
    case 0x1005:if(set)s->send_timeout=v;v=s->send_timeout;break;
    case 0x1006:if(set)s->recv_timeout=v;v=s->recv_timeout;break;
    case 0x1001:if(set)s->send_limit=v>YN_QUEUE_BYTES?YN_QUEUE_BYTES:v;v=s->send_limit;break;
    case 0x1002:if(set)s->recv_limit=v>YN_QUEUE_BYTES?YN_QUEUE_BYTES:v;v=s->recv_limit;break;
    case 0x1007:if(set)r=-22;v=s->lost?100:0;break;
    case 0x1008:if(set)r=-22;v=SOCK_DGRAM;break;
    case 0x800:case 8:case 0x100:case 0x80: v=0;break; /* NOSIGPIPE, KEEPALIVE, OOBINLINE, LINGER: irrelevant to UDP */
    default:r=-92;break;
    }
    if(!r && !set){
        if(timeout){int64_t tv[2]={v/1000,(v%1000)*1000};uint32_t n=*size<16?*size:16;memcpy(value,tv,n);*size=n;}
        else{uint32_t n=*size<4?*size:4;memcpy(value,&v,n);*size=n;}
    }
    yn_log(2,"socket","%s_option fd=%d level=0x%x name=0x%x value=%u result=%d",set?"set":"get",fd,level,name,v,r);
    yn_unlock();return r;
}

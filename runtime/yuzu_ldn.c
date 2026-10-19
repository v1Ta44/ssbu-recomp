/* LDN room interoperability. Wire layouts/state transitions reference yuzu
 * lan_discovery.cpp (2022 yuzu, GPL-2.0-or-later) and ldn_types.h (GPL-3.0-or-later),
 * mainline 7ffac53c9e67. Explicit byte offsets avoid host ABI padding. */
#include "yuzu_net_internal.h"
#include "yuzu_packet.h"
#include <stdlib.h>
#include <string.h>
#include <bcrypt.h>
#pragma comment(lib,"bcrypt.lib")
void os_signal_event(uint64_t key);
static uint32_t last_state,last_reason; /* protected by the backend lock */

static void notify_locked(void) {
    if(last_state!=ssbu_yn.ldn_state || last_reason!=ssbu_yn.disconnect_reason) {
        yn_log(1,"ldn","state=%u->%u disconnect_reason=%u generation=%llu nodes=%u event_attached=%d",
            last_state,ssbu_yn.ldn_state,ssbu_yn.disconnect_reason,(unsigned long long)ssbu_yn.ldn_generation,
            ssbu_yn.network[0x67],!!ssbu_yn.ldn_event);
        last_state=ssbu_yn.ldn_state;last_reason=ssbu_yn.disconnect_reason;
    }
    yn_changed_locked();
    if (ssbu_yn.ldn_event) os_signal_event(ssbu_yn.ldn_event);
}
static void cancel_waits_locked(void) {
    ssbu_yn.ldn_generation++; ssbu_yn.connecting=0; ssbu_yn.scanning=0; yn_changed_locked();
}
static int send_locked(unsigned type,const uint8_t *ip,const void *payload,size_t n) {
    uint8_t bytes[YN_LDN_SIZE+32]; YnWire w={bytes,sizeof bytes,0,0};
    if (!yn_joined_locked()) return 0;
    yw_write(&w,6,1); yw_write(&w,type,1); yw_bytes(&w,ssbu_yn.snapshot.ip,4);
    uint8_t zero[4]={0}; yw_bytes(&w,ip?ip:zero,4); yw_write(&w,ip?0:1,1);
    yw_write(&w,n,4); yw_bytes(&w,payload,n);
    int ok=!w.bad && yn_queue_locked(bytes,w.pos);
    const uint8_t *dst=ip?ip:zero;
    yn_log(ok?3:2,"ldn","tx type=%u dst=%u.%u.%u.%u broadcast=%d bytes=%zu queued=%d",type,dst[0],dst[1],dst[2],dst[3],!ip,n,ok);
    return ok;
}
static void node_ip(uint8_t out[4],const uint8_t *node) { for(int i=0;i<4;i++) out[i]=node[3-i]; }
static int node_is(const uint8_t *node,const uint8_t ip[4]) {
    uint8_t n[4]; node_ip(n,node); return !memcmp(n,ip,4);
}
static void make_node_locked(uint8_t *node,const uint8_t *user,int version) {
    memset(node,0,YN_NODE_SIZE);
    for (int i=0;i<4;i++) node[i]=ssbu_yn.snapshot.ip[3-i];
    node[4]=2; memcpy(node+6,ssbu_yn.snapshot.ip,4); node[11]=1;
    /* Preserve the name supplied by the game (already obtained from its profile).
     * Never replace a remote user's name with the local frontend nickname. */
    if (user && user[0]) { memcpy(node+12,user,32); node[44]=0; }
    else memcpy(node+12,ssbu_yn.profile,33);
    yn_put16(node+46,(unsigned)version);
}
static void update_locked(void) {
    unsigned count=0;
    for (unsigned i=0;i<8;i++) if (ssbu_yn.network[YN_NODE_OFF+i*64+11]) count++;
    ssbu_yn.network[0x67]=(uint8_t)count;
    yn_log(2,"ldn","host update nodes=%u max=%u advertise_bytes=%u",count,ssbu_yn.network[0x66],yn_le16(ssbu_yn.network+0x26a));
    for (unsigned i=1;i<8;i++) {
        uint8_t *node=ssbu_yn.network+YN_NODE_OFF+i*64;
        if (node[11]) { uint8_t ip[4]; node_ip(ip,node); send_locked(3,ip,ssbu_yn.network,YN_LDN_SIZE); }
    }
    notify_locked();
}
static int valid_network(const uint8_t *info) {
    if (!info[0x66] || info[0x66]>8 || !info[0x67] || info[0x67]>info[0x66] ||
        yn_le16(info+0x26a)>384 || info[0x26]>32 || !info[YN_NODE_OFF+11]) return 0;
    unsigned count=0;
    for (unsigned i=0;i<8;i++) if (info[YN_NODE_OFF+i*64+11]) {
        if (info[YN_NODE_OFF+i*64+10]!=i) return 0;
        count++;
    }
    return count==info[0x67];
}
void yn_ldn_lost_locked(void) {
    cancel_waits_locked();
    int active=ssbu_yn.ldn_state!=0;
    ssbu_yn.ldn_state=active?1:0; ssbu_yn.disconnect_reason=6;
    memset(ssbu_yn.host_ip,0,4); memset(ssbu_yn.network,0,sizeof ssbu_yn.network); ssbu_yn.scan_count=0; ssbu_yn.scanning=0;
    if (active) notify_locked();
}
void yn_ldn_member_left_locked(const uint8_t ip[4]) {
    if (ssbu_yn.ldn_state==3) {
        for (unsigned i=1;i<8;i++) {
            uint8_t *node=ssbu_yn.network+YN_NODE_OFF+i*64;
            if (node[11] && node_is(node,ip)) { node[11]=0; update_locked(); break; }
        }
    } else if (ssbu_yn.ldn_state==5 && !memcmp(ip,ssbu_yn.host_ip,4)) {
        ssbu_yn.ldn_state=4; ssbu_yn.disconnect_reason=6; memset(ssbu_yn.host_ip,0,4); yn_socket_reset_locked(); notify_locked();
    }
}
void yn_ldn_receive_locked(unsigned type,const uint8_t src[4],const uint8_t *data,size_t n) {
    yn_log(3,"ldn","rx type=%u src=%u.%u.%u.%u bytes=%zu state=%u",type,src[0],src[1],src[2],src[3],n,ssbu_yn.ldn_state);
    if (!ssbu_yn.ldn_state) return;
    if (type==0 && n==0 && ssbu_yn.ldn_state==3) { send_locked(1,src,ssbu_yn.network,YN_LDN_SIZE); return; }
    if (type==1 && ssbu_yn.scanning && n==YN_LDN_SIZE && valid_network(data) && node_is(data+YN_NODE_OFF,src)) {
        unsigned i=0; for (;i<ssbu_yn.scan_count;i++) if (!memcmp(ssbu_yn.scan[i]+0x20,data+0x20,6)) break;
        if (i<64) { memcpy(ssbu_yn.scan[i],data,n); if (i==ssbu_yn.scan_count) ssbu_yn.scan_count++; yn_changed_locked(); }
        return;
    }
    if (type==2 && ssbu_yn.ldn_state==3 && n==YN_NODE_SIZE && node_is(data,src)) {
        if (ssbu_yn.policy==1) { yn_log(2,"ldn","join ignored: RejectAll policy"); return; }
        unsigned free_slot=8;
        for (unsigned i=1;i<ssbu_yn.network[0x66];i++) {
            uint8_t *node=ssbu_yn.network+YN_NODE_OFF+i*64;
            if (node[11] && node_is(node,src)) { send_locked(3,src,ssbu_yn.network,YN_LDN_SIZE); return; }
            if (!node[11] && free_slot==8) free_slot=i;
        }
        if (free_slot==8) { yn_log(2,"ldn","join ignored: no free node slot"); return; }
        uint8_t *node=ssbu_yn.network+YN_NODE_OFF+free_slot*64;
        memcpy(node,data,64); node[10]=(uint8_t)free_slot; node[11]=1; node[44]=0;
        update_locked(); return;
    }
    if (type==3 && (ssbu_yn.ldn_state==4 || ssbu_yn.ldn_state==5) && !memcmp(src,ssbu_yn.host_ip,4) && n==YN_LDN_SIZE &&
        valid_network(data) && !memcmp(ssbu_yn.network,data,32) && node_is(data+YN_NODE_OFF,src)) {
        int present=0;
        for (unsigned i=1;i<8;i++) if (data[YN_NODE_OFF+i*64+11] && node_is(data+YN_NODE_OFF+i*64,ssbu_yn.snapshot.ip)) present=1;
        if (!present) { ssbu_yn.ldn_state=4; ssbu_yn.disconnect_reason=5; memset(ssbu_yn.host_ip,0,4); yn_socket_reset_locked(); notify_locked(); return; }
        int changed=ssbu_yn.ldn_state!=5 || memcmp(ssbu_yn.network,data,n);
        memcpy(ssbu_yn.network,data,n); ssbu_yn.ldn_state=5;
        if (changed) notify_locked(); return;
    }
    if (type==4 && ssbu_yn.ldn_state==3 && n==YN_NODE_SIZE && node_is(data,src)) { yn_ldn_member_left_locked(src); return; }
    if (type==5 && !n && (ssbu_yn.ldn_state==4 || ssbu_yn.ldn_state==5) && !memcmp(src,ssbu_yn.host_ip,4)) {
        ssbu_yn.ldn_state=4; ssbu_yn.disconnect_reason=3; memset(ssbu_yn.host_ip,0,4); yn_socket_reset_locked(); notify_locked(); return;
    }
    ssbu_yn.snapshot.dropped_packets++;
    yn_log(2,"ldn","drop type=%u bytes=%zu state=%u: unexpected state, layout or session",type,n,ssbu_yn.ldn_state);
}
uint32_t yn_ldn_initialize(void) {
    yn_lock(); uint32_t r=0;
    if (!yn_joined_locked()) r=YN_LDN_ERROR(23);
    else if (!ssbu_yn.ldn_state) { ssbu_yn.ldn_state=1; ssbu_yn.disconnect_reason=0; ssbu_yn.policy=0; yn_changed_locked(); }
    yn_log(1,"ldn","Initialize result=0x%x state=%u",r,ssbu_yn.ldn_state);
    last_state=ssbu_yn.ldn_state;last_reason=ssbu_yn.disconnect_reason;
    yn_unlock(); return r;
}
uint32_t yn_ldn_state(void) { yn_lock(); uint32_t s=ssbu_yn.ldn_state; yn_unlock(); return s; }
uint32_t yn_ldn_reason(void) { yn_lock(); uint32_t s=ssbu_yn.disconnect_reason; yn_unlock(); return s; }
uint32_t yn_ldn_event(uint64_t key) {
    yn_lock(); uint32_t r=ssbu_yn.ldn_state?0:YN_LDN_ERROR(32);
    if (!r) { ssbu_yn.ldn_event=key; if (ssbu_yn.ldn_state==3 || ssbu_yn.ldn_state==5) notify_locked(); }
    yn_unlock(); return r;
}
uint32_t yn_ldn_disconnect(int destroy) {
    yn_lock(); uint32_t r=0;
    if (destroy) {
        if (ssbu_yn.ldn_state!=3) r=YN_LDN_ERROR(32);
        else {
            for (unsigned i=1;i<8;i++) { const uint8_t *node=ssbu_yn.network+YN_NODE_OFF+i*64;
                if (node[11]) { uint8_t ip[4]; node_ip(ip,node); send_locked(5,ip,NULL,0); }
            }
            ssbu_yn.ldn_state=2; ssbu_yn.disconnect_reason=3;
        }
    } else {
        if (ssbu_yn.ldn_state!=4 && ssbu_yn.ldn_state!=5) r=YN_LDN_ERROR(32);
        else { if (ssbu_yn.host_ip[0]) send_locked(4,ssbu_yn.host_ip,ssbu_yn.own_node,64); ssbu_yn.ldn_state=4; ssbu_yn.disconnect_reason=1; }
    }
    if (!r) { cancel_waits_locked(); memset(ssbu_yn.host_ip,0,4); memset(ssbu_yn.network,0,sizeof ssbu_yn.network); yn_socket_reset_locked(); notify_locked(); }
    yn_unlock(); return r;
}
uint32_t yn_ldn_finalize(void) {
    yn_lock();
    if (ssbu_yn.ldn_state==3) yn_ldn_disconnect(1);
    /* A pending request may already occupy a slot on the remote host. */
    else if (ssbu_yn.ldn_state==5 || (ssbu_yn.ldn_state==4 && ssbu_yn.connecting)) yn_ldn_disconnect(0);
    cancel_waits_locked();
    ssbu_yn.ldn_state=0; ssbu_yn.ldn_event=0; ssbu_yn.scan_count=0; ssbu_yn.scanning=0;
    memset(ssbu_yn.host_ip,0,4);
    memset(ssbu_yn.network,0,sizeof ssbu_yn.network); yn_socket_reset_locked(); yn_changed_locked();
    yn_log(1,"ldn","Finalize state=0 generation=%llu",(unsigned long long)ssbu_yn.ldn_generation);
    last_state=0;last_reason=ssbu_yn.disconnect_reason;yn_unlock(); return 0;
}
uint32_t yn_ldn_open(int station,int close) {
    yn_lock(); uint32_t r=0;
    if (!ssbu_yn.ldn_state || !yn_joined_locked()) r=YN_LDN_ERROR(32);
    else if (close) {
        if (ssbu_yn.ldn_state==(station?5u:3u) || (station && ssbu_yn.ldn_state==4 && ssbu_yn.connecting)) yn_ldn_disconnect(!station);
        if (ssbu_yn.ldn_state!=(station?4u:2u)) r=YN_LDN_ERROR(32);
        else { cancel_waits_locked(); memset(ssbu_yn.host_ip,0,4); ssbu_yn.ldn_state=1; notify_locked(); }
    } else if (ssbu_yn.ldn_state!=1) r=YN_LDN_ERROR(32);
    else { ssbu_yn.ldn_state=station?4:2; ssbu_yn.disconnect_reason=0; notify_locked(); }
    yn_unlock(); return r;
}
uint32_t yn_ldn_create(const void *config,const void *security,const void *user,const void *parameter) {
    if (!config || !security || !user) return YN_LDN_ERROR(96);
    const uint8_t *cfg=config,*sec=security;
    if (!cfg[18] || cfg[18]>8 || yn_le16(sec+2)>64) return YN_LDN_ERROR(96);
    uint8_t random[32]; if (BCryptGenRandom(NULL,random,sizeof random,BCRYPT_USE_SYSTEM_PREFERRED_RNG)<0) return YN_LDN_ERROR(65);
    yn_lock(); uint32_t r=0;
    if (ssbu_yn.ldn_state!=2 || !yn_joined_locked()) r=YN_LDN_ERROR(32);
    else {
        cancel_waits_locked();
        uint8_t *info=ssbu_yn.network; memset(info,0,YN_LDN_SIZE); memcpy(info,cfg,16);
        memcpy(info+16,random,16); memcpy(info+0x50,random+16,16);
        if (parameter) { memcpy(info+0x50,parameter,16); memcpy(info+16,(const uint8_t *)parameter+16,16); }
        info[0x20]=2; memcpy(info+0x22,ssbu_yn.snapshot.ip,4);
        const char *ssid="YuzuFakeSsidForLdn"; info[0x26]=(uint8_t)strlen(ssid); memcpy(info+0x27,ssid,strlen(ssid));
        yn_put16(info+0x48,yn_le16(cfg+16)?yn_le16(cfg+16):6); info[0x4a]=2; info[0x4b]=2;
        memcpy(info+0x60,sec,2); info[0x62]=ssbu_yn.policy; info[0x66]=cfg[18];
        for (unsigned i=0;i<8;i++) info[YN_NODE_OFF+i*64+10]=(uint8_t)i;
        make_node_locked(info+YN_NODE_OFF,user,yn_le16(cfg+20));
        memcpy(ssbu_yn.network_config,cfg,32); ssbu_yn.ldn_state=3; ssbu_yn.disconnect_reason=0;
        yn_socket_reset_locked(); update_locked();
    }
    yn_unlock(); return r;
}
uint32_t yn_ldn_connect(const void *network,const void *user,int version) {
    if (!network || !user || !valid_network(network)) return YN_LDN_ERROR(96);
    yn_lock(); uint32_t r=0;
    if (ssbu_yn.ldn_state!=4 || !yn_joined_locked() || ssbu_yn.connecting) r=YN_LDN_ERROR(32);
    else {
        uint64_t generation=ssbu_yn.ldn_generation; ssbu_yn.connecting=1;
        memcpy(ssbu_yn.network,network,YN_LDN_SIZE); node_ip(ssbu_yn.host_ip,ssbu_yn.network+YN_NODE_OFF);
        yn_log(1,"ldn","Connect begin host=%u.%u.%u.%u version=%d generation=%llu",ssbu_yn.host_ip[0],ssbu_yn.host_ip[1],ssbu_yn.host_ip[2],ssbu_yn.host_ip[3],version,(unsigned long long)generation);
        make_node_locked(ssbu_yn.own_node,user,version); ssbu_yn.disconnect_reason=0; yn_socket_reset_locked();
        int requested=send_locked(2,ssbu_yn.host_ip,ssbu_yn.own_node,64);
        if (!requested) r=YN_LDN_ERROR(31);
        else {
            uint64_t epoch=ssbu_yn.epoch; ULONGLONG end=GetTickCount64()+4000;
            while (ssbu_yn.ldn_state==4 && !ssbu_yn.disconnect_reason && yn_joined_locked() && ssbu_yn.epoch==epoch && ssbu_yn.ldn_generation==generation) {
                ULONGLONG now=GetTickCount64(); if (now>=end) break;
                SleepConditionVariableCS(&ssbu_yn.changed,&ssbu_yn.lock,(DWORD)(end-now));
            }
            if (ssbu_yn.ldn_state!=5 || ssbu_yn.epoch!=epoch || !yn_joined_locked() || ssbu_yn.ldn_generation!=generation) r=YN_LDN_ERROR(66);
        }
        if (ssbu_yn.ldn_generation==generation) {
            ssbu_yn.connecting=0;
            if (r) {
                /* The reliable queue orders this after Connect, even if its reply is late.
                 * Cancellation cleans up before changing the generation; never touch a newer session. */
                if (requested && ssbu_yn.host_ip[0]) send_locked(4,ssbu_yn.host_ip,ssbu_yn.own_node,64);
                memset(ssbu_yn.host_ip,0,4); memset(ssbu_yn.network,0,sizeof ssbu_yn.network);
            }
        }
    }
    yn_log(1,"ldn","Connect end result=0x%x state=%u reason=%u",r,ssbu_yn.ldn_state,ssbu_yn.disconnect_reason);
    yn_unlock(); return r;
}
uint32_t yn_ldn_info(void *out) {
    if (!out) return YN_LDN_ERROR(96);
    yn_lock(); uint32_t r=(ssbu_yn.ldn_state==3 || ssbu_yn.ldn_state==5)?0:YN_LDN_ERROR(32);
    if (!r) memcpy(out,ssbu_yn.network,YN_LDN_SIZE); yn_unlock(); return r;
}
uint32_t yn_ldn_config(void *out) {
    if (!out) return YN_LDN_ERROR(96);
    yn_lock(); uint32_t r=(ssbu_yn.ldn_state==3 || ssbu_yn.ldn_state==5)?0:YN_LDN_ERROR(32);
    if (!r) {
        uint8_t *p=out; memset(p,0,32); memcpy(p,ssbu_yn.network,16); memcpy(p+16,ssbu_yn.network+0x48,2);
        p[18]=ssbu_yn.network[0x66]; memcpy(p+20,ssbu_yn.network+YN_NODE_OFF+46,2);
    }
    yn_unlock(); return r;
}
uint32_t yn_ldn_ip(void *ip,void *mask) {
    if (!ip || !mask) return YN_LDN_ERROR(96);
    yn_lock(); uint32_t r=yn_joined_locked() && ssbu_yn.ldn_state?0:YN_LDN_ERROR(33);
    if (!r) { for (int i=0;i<4;i++) ((uint8_t *)ip)[i]=ssbu_yn.snapshot.ip[3-i]; yn_put32(mask,0xffff0000); }
    yn_unlock(); return r;
}
uint32_t yn_ldn_security(void *out) {
    if (!out) return YN_LDN_ERROR(96);
    yn_lock(); uint32_t r=(ssbu_yn.ldn_state==3 || ssbu_yn.ldn_state==5)?0:YN_LDN_ERROR(32);
    if (!r) { memcpy(out,ssbu_yn.network+0x50,16); memcpy((uint8_t *)out+16,ssbu_yn.network+16,16); }
    yn_unlock(); return r;
}
uint32_t yn_ldn_advertise(const void *data,size_t n) {
    if (n>384) return YN_LDN_ERROR(10);
    if (n && !data) return YN_LDN_ERROR(96);
    yn_lock(); uint32_t r=ssbu_yn.ldn_state==3?0:YN_LDN_ERROR(32);
    if (!r) { memset(ssbu_yn.network+0x26c,0,384); if(n)memcpy(ssbu_yn.network+0x26c,data,n); yn_put16(ssbu_yn.network+0x26a,(unsigned)n); update_locked(); }
    yn_unlock(); return r;
}
static int matches(const uint8_t *n,const uint8_t *f) {
    uint32_t flags=yn_le32(f+0x5c);
    if ((flags&1) && memcmp(n,f,8)) return 0;
    if ((flags&2) && memcmp(n+16,f+16,16)) return 0;
    if ((flags&4) && yn_le32(f+32)!=n[0x4b]) return 0;
    if ((flags&8) && memcmp(n+32,f+36,6)) return 0;
    if ((flags&16) && (f[42]>32 || f[42]!=n[0x26] || memcmp(n+0x27,f+43,f[42]))) return 0;
    if ((flags&32) && memcmp(n+10,f+10,2)) return 0;
    return 1;
}
uint32_t yn_ldn_scan(void *out,int *count,int capacity,const void *filter) {
    if (!count || capacity<0 || (capacity && !out) || !filter) return YN_LDN_ERROR(96);
    *count=0; yn_lock();
    if (!ssbu_yn.ldn_state || !yn_joined_locked() || ssbu_yn.scanning) { yn_unlock(); return YN_LDN_ERROR(32); }
    ssbu_yn.scanning=1; ssbu_yn.scan_count=0; uint64_t epoch=ssbu_yn.epoch, generation=ssbu_yn.ldn_generation;
    yn_log(2,"ldn","Scan begin capacity=%d filter_flags=0x%x generation=%llu",capacity,yn_le32((const uint8_t *)filter+0x5c),(unsigned long long)generation);
    if (!send_locked(0,NULL,NULL,0)) { ssbu_yn.scanning=0; yn_unlock(); return YN_LDN_ERROR(31); }
    ULONGLONG end=GetTickCount64()+1000;
    while (yn_joined_locked() && ssbu_yn.epoch==epoch && ssbu_yn.scanning && ssbu_yn.ldn_generation==generation) {
        ULONGLONG now=GetTickCount64(); if (now>=end) break;
        SleepConditionVariableCS(&ssbu_yn.changed,&ssbu_yn.lock,(DWORD)(end-now));
    }
    uint32_t r=yn_joined_locked() && ssbu_yn.epoch==epoch && ssbu_yn.ldn_state && ssbu_yn.ldn_generation==generation?0:YN_LDN_ERROR(32);
    if (!r) for (unsigned i=0;i<ssbu_yn.scan_count && *count<capacity;i++)
        if (matches(ssbu_yn.scan[i],filter)) { memcpy((uint8_t *)out+(size_t)(*count)*YN_LDN_SIZE,ssbu_yn.scan[i],YN_LDN_SIZE); (*count)++; }
    yn_log(2,"ldn","Scan end result=0x%x received=%u matched=%d",r,ssbu_yn.scan_count,*count);
    if (ssbu_yn.ldn_generation==generation) ssbu_yn.scanning=0; yn_unlock(); return r;
}
uint32_t yn_ldn_policy(unsigned policy) {
    if (policy>1) return YN_LDN_ERROR(96); /* no silent fake blacklist/whitelist */
    yn_lock(); uint32_t r=ssbu_yn.ldn_state?0:YN_LDN_ERROR(32);
    if (!r) { ssbu_yn.policy=(uint8_t)policy; ssbu_yn.network[0x62]=(uint8_t)policy; }
    yn_unlock(); return r;
}
uint32_t yn_ldn_reject(uint32_t addr) {
    uint8_t ip[4]={(uint8_t)(addr>>24),(uint8_t)(addr>>16),(uint8_t)(addr>>8),(uint8_t)addr};
    yn_lock(); uint32_t r=ssbu_yn.ldn_state==3?0:YN_LDN_ERROR(32);
    if (!r) { send_locked(5,ip,NULL,0); yn_ldn_member_left_locked(ip); }
    yn_unlock(); return r;
}

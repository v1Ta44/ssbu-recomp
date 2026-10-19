/* Private backend interface. Functions ending _locked require ssbu_yn.lock.
 * Room, LDN and virtual sockets share one lock; condition waits release it. */
#ifndef SSBU_YUZU_NET_INTERNAL_H
#define SSBU_YUZU_NET_INTERNAL_H
#include <winsock2.h>
#include <windows.h>
#include "yuzu_net.h"
#define YN_MAX_PACKET (128 * 1024)
#define YN_QUEUE_BYTES (8 * 1024 * 1024)
#define YN_VFD_BASE 0x10000
#define YN_LDN_SIZE 0x480
#define YN_NODE_OFF 0x68
#define YN_NODE_SIZE 0x40
#define YN_LDN_ERROR(n) (203u | ((uint32_t)(n) << 9))
typedef struct YnPacket { struct YnPacket *next; size_t size; uint8_t bytes[1]; } YnPacket;
typedef struct {
    CRITICAL_SECTION lock;
    CONDITION_VARIABLE changed;
    HANDLE worker;
    int stop, enabled;
    uint64_t activity, epoch;
    SsbuNetConfig config;
    SsbuNetSnapshot snapshot;
    char profile[33];
    uint32_t avatar_format, avatar_size;
    uint8_t *avatar;
    YnPacket *send_head, *send_tail;
    size_t send_bytes, transport_bytes;
    uint32_t ldn_state, disconnect_reason;
    uint64_t ldn_event, ldn_generation;
    int connecting;
    uint8_t network[YN_LDN_SIZE], own_node[YN_NODE_SIZE], host_ip[4];
    uint8_t scan[64][YN_LDN_SIZE];
    unsigned scan_count;
    int scanning;
    uint8_t network_config[32], policy;
} YnBackend;
extern YnBackend ssbu_yn;
void yn_log(unsigned level,const char *component,const char *format,...);
void yn_lock(void);
void yn_unlock(void);
void yn_changed_locked(void);
int yn_joined_locked(void);
int yn_enabled(void);
int yn_queue_locked(const void *data, size_t size);
void yn_room_lost_locked(void);
void yn_ldn_receive_locked(unsigned type, const uint8_t src[4], const uint8_t *data, size_t size);
void yn_ldn_member_left_locked(const uint8_t ip[4]);
void yn_ldn_lost_locked(void);
void yn_socket_receive_locked(const uint8_t *bytes, size_t size);
void yn_socket_lost_locked(void);
void yn_socket_reset_locked(void);
uint64_t yn_activity(void);
void yn_wait_activity(uint64_t previous, unsigned ms);
/* LDN SDK adapter uses Result values, and copies fixed-size structures. */
uint32_t yn_ldn_initialize(void);
uint32_t yn_ldn_finalize(void);
uint32_t yn_ldn_state(void);
uint32_t yn_ldn_event(uint64_t key);
uint32_t yn_ldn_open(int station, int close);
uint32_t yn_ldn_create(const void *config, const void *security, const void *user, const void *parameter);
uint32_t yn_ldn_connect(const void *info, const void *user, int version);
uint32_t yn_ldn_disconnect(int destroy);
uint32_t yn_ldn_info(void *out);
uint32_t yn_ldn_config(void *out);
uint32_t yn_ldn_ip(void *ip, void *mask);
uint32_t yn_ldn_security(void *out);
uint32_t yn_ldn_advertise(const void *data, size_t size);
uint32_t yn_ldn_scan(void *out, int *count, int capacity, const void *filter);
uint32_t yn_ldn_policy(unsigned policy);
uint32_t yn_ldn_reject(uint32_t address);
uint32_t yn_ldn_reason(void);
/* Virtual socket API: fd >= YN_VFD_BASE, negative values are guest errno. */
int yn_socket_create(int type, int protocol);
int yn_socket_close(int fd);
void yn_socket_close_all(void);
int yn_socket_bind(int fd, const struct sockaddr_in *addr);
int yn_socket_connect(int fd, const struct sockaddr_in *addr);
int yn_socket_name(int fd, struct sockaddr_in *addr, int peer);
int yn_socket_recv(int fd, void *buf, size_t size, int flags, struct sockaddr_in *src);
int yn_socket_send(int fd, const void *buf, size_t size, int flags, const struct sockaddr_in *dst);
int yn_socket_fcntl(int fd, int cmd, int flags);
int yn_socket_poll(int fd, int events);
int yn_socket_option(int fd, int set, int level, int name, void *value, uint32_t *size);
int yn_socket_shutdown(int fd, int how);
static inline uint16_t yn_le16(const uint8_t *p) { return (uint16_t)(p[0] | p[1] << 8); }
static inline uint32_t yn_le32(const uint8_t *p) { return p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24; }
static inline void yn_put16(uint8_t *p, unsigned v) { p[0]=(uint8_t)v; p[1]=(uint8_t)(v>>8); }
static inline void yn_put32(uint8_t *p, uint32_t v) { for (int i=0;i<4;i++) p[i]=(uint8_t)(v>>(i*8)); }
#endif

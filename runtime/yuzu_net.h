/* PC backend ABI. No UI, guest pointers, or ENet types cross this boundary. */
#ifndef SSBU_YUZU_NET_H
#define SSBU_YUZU_NET_H
#include <stdint.h>
#include <stddef.h>
#ifndef SSBU_NET_API
#ifdef _WIN32
#define SSBU_NET_API __declspec(dllexport)
#else
#define SSBU_NET_API
#endif
#endif
#ifdef __cplusplus
extern "C" {
#endif
#define SSBU_NET_ABI 1
#define SSBU_NET_MAX_MEMBERS 254
#define SSBU_NET_AVATAR_MAX (256 * 1024)
enum { SSBU_NET_IDLE, SSBU_NET_CONNECTING, SSBU_NET_JOINING, SSBU_NET_JOINED, SSBU_NET_FAILED };
enum { SSBU_NET_OK, SSBU_NET_BAD_ARGUMENT, SSBU_NET_BUSY, SSBU_NET_IO, SSBU_NET_TIMEOUT,
       SSBU_NET_VERSION, SSBU_NET_PASSWORD, SSBU_NET_NAME, SSBU_NET_FULL, SSBU_NET_BANNED,
       SSBU_NET_KICKED, SSBU_NET_CLOSED, SSBU_NET_MALFORMED, SSBU_NET_OVERFLOW, SSBU_NET_IP };
typedef struct {
    uint32_t size, abi;
    char host[256];
    uint16_t port;                 /* host order; 0 selects 24872 */
    uint16_t reserved;
    char nickname[21];            /* Yuzu: 4..20 ASCII letters/digits/space/._- */
    char password[128];
    char token[4096];             /* optional server identity token; never logged */
    char game_name[128], game_version[32];
    uint64_t game_id;
} SsbuNetConfig;
typedef struct {
    char nickname[21], username[128], display_name[128], avatar_url[1024];
    uint8_t ip[4];
    char game_name[128], game_version[32];
    uint64_t game_id;
} SsbuNetMember;
typedef struct {
    uint32_t size, abi, state, error, server_version, member_count;
    uint64_t revision, sent_packets, received_packets, dropped_packets;
    uint8_t ip[4];
    char room_name[256], room_description[1024];
    uint32_t room_slots;
    uint16_t room_port;
    uint16_t reserved;
    char message[160];
    SsbuNetMember members[SSBU_NET_MAX_MEMBERS];
} SsbuNetSnapshot;
/* All functions are thread-safe. Join is asynchronous; poll a copied snapshot.
 * Leave joins the worker (normally <20ms; DNS can take longer). Call it off the UI thread.
 * Configuration/profile changes are rejected while joined or an LDN session is active.
 * Neither Join nor SetProfile writes a chat message. No callbacks into frontend code. */
SSBU_NET_API int ssbu_net_join(const SsbuNetConfig *config);
SSBU_NET_API void ssbu_net_leave(void);
SSBU_NET_API int ssbu_net_snapshot(SsbuNetSnapshot *out); /* set size and abi before call */
/* Small, copied context for optional local-wireless tools. Additive ABI: the
 * existing room snapshot and wire protocol do not change. No guest pointers. */
typedef struct {
    uint32_t size, abi, room_state, ldn_state, node_count, reserved;
    uint64_t room_epoch, ldn_generation;
} SsbuNetLdnSnapshot;
SSBU_NET_API int ssbu_net_ldn_snapshot(SsbuNetLdnSnapshot *out);

/* Diagnostics go to stderr (scripts/run.py --log captures them). Default 1;
 * SSBU_NET_LOG_LEVEL overrides at startup. 0 off, 1 lifecycle, 2 debug, 3 packets.
 * Levels 2/3 are rate limited; no passwords, tokens or payloads are printed. */
SSBU_NET_API int ssbu_net_set_log_level(uint32_t level);
/* Read retained diagnostic lines without callbacks. Start cursor at 0; pass at
 * least 1100 bytes of capacity. Returns copied bytes excluding NUL. Slow readers
 * resume at the oldest retained line (128 lines). Full logs remain on stderr. */
SSBU_NET_API uint32_t ssbu_net_read_log(uint64_t *cursor,char *out,uint32_t capacity);
SSBU_NET_API int ssbu_net_validate_profile(const char *utf8_name); /* 0 = valid, nonblank UTF-8, <=32 bytes */
SSBU_NET_API int ssbu_net_set_profile(const char *utf8_name); /* validation above; no generated identity */
SSBU_NET_API void ssbu_net_get_profile(char out_name[33]);
/* Local frontend avatar only. PNG/JPEG bytes are copied, not decoded. Remote member
 * avatars come from server avatar_url; Yuzu v1 has no client avatar upload message.
 * format: 0 clear, 1 PNG, 2 JPEG. Get returns required bytes, copies only if capacity fits. */
SSBU_NET_API int ssbu_net_set_avatar(uint32_t format, const void *bytes, uint32_t size);
SSBU_NET_API uint32_t ssbu_net_get_avatar(uint32_t *format, void *bytes, uint32_t capacity);
void ssbu_net_boot(void);         /* optional environment configuration, no frontend */
#ifdef __cplusplus
}
#endif
#endif

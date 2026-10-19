/* Local frontend/backend wire ABI. No pointers, HWNDs, network or game types.
 * Named-pipe ACL and bounded transfers are supplied by the Windows adapter. */
#ifndef NET_DELAY_UI_H
#define NET_DELAY_UI_H
#include "delay_api.h"
#define ND_UI_ABI 1u
#define ND_UI_CLASS L"SsbuNetDelayOverlay"
enum { ND_READ=1, ND_SELECT=2 };
typedef struct { uint32_t size,abi,operation; int32_t frames; } NdRequest;
typedef struct { uint32_t size,abi,ok; int32_t preferences; NdSnapshot state; } NdReply;
#endif

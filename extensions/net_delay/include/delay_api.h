/* Stable, renderer/network/game independent C boundary. */
#ifndef NET_DELAY_API_H
#define NET_DELAY_API_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#define ND_ABI 1u
#define ND_AUTO (-1)
#define ND_MAX_FRAMES 25

typedef enum {
    ND_AUTO_RESULT, ND_APPLIED, ND_NO_SESSION, ND_SESSION_CHANGED,
    ND_BAD_VALUE, ND_MEMORY, ND_UNAVAILABLE
} NdDecision;
typedef enum { ND_BACKEND_READY, ND_BACKEND_VERSION, ND_BACKEND_CONTEXT, ND_BACKEND_HOOK } NdBackend;
/* Opaque identities supplied by an adapter, not transport structures. */
typedef struct {
    uint32_t available, connected, role, peers;
    uint64_t connection, session;
} NdSession;
typedef struct {
    uint32_t size, abi;
    int32_t selected, backend, decision, before, automatic, effective;
    uint32_t mode, object_state;
    uint64_t calls, writes, selection_revision, applied_revision;
    NdSession live, last_session;
} NdSnapshot;
typedef struct {
    int32_t readable, before, automatic;
    uint32_t mode, object_state;
} NdSample;
typedef struct NdEngine NdEngine;
/* Called synchronously under the engine lock. No reentry into the engine.
 * Return 1 only when the adapter has written the value. */
typedef int (*NdWrite)(void *user, int frames);
NdEngine *nd_create(void);
void nd_destroy(NdEngine *engine); /* caller must stop its workers first */
int nd_select(NdEngine *engine, int frames);
int nd_snapshot(NdEngine *engine, NdSnapshot *out);
void nd_backend(NdEngine *engine, NdBackend backend);
void nd_session(NdEngine *engine, const NdSession *session);
int nd_evaluate(NdEngine *engine, const NdSession *before, const NdSession *after,
                const NdSample *sample, NdWrite write, void *user, NdSnapshot *result);
int nd_session_active(const NdSession *session);
#ifdef __cplusplus
}
#endif
#endif

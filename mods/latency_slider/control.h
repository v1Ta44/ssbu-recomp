/* Native experiment control/status. No game code or protocol payloads. */
#pragma once
#include <stdint.h>
enum { DELAY_AUTO=-1, DELAY_MAX=25, DELAY_ABI=1 };
enum DelayDecision {
    DELAY_OBSERVE, DELAY_APPLIED, DELAY_NO_SESSION, DELAY_SESSION_CHANGED,
    DELAY_BAD_VALUE, DELAY_MEMORY, DELAY_NOT_INSTALLED
};
struct DelaySnapshot {
    uint32_t size, abi;
    int32_t selected, installed, decision, before, automatic, effective;
    uint32_t room_state, ldn_state, nodes, mode, object_state;
    uint64_t calls, writes, room_epoch, generation, caller, object;
    float input_metric;
};
#ifdef __cplusplus
extern "C" {
#endif
__declspec(dllexport) int ssbu_delay_set_frames(int frames);
__declspec(dllexport) int ssbu_delay_snapshot(struct DelaySnapshot *out);
#ifdef __cplusplus
}
#endif

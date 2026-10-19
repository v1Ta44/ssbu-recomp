/* clang -I runtime tests/input_trace_test.c -o <test.exe>; <test.exe> <trace.csv> */
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include "input_trace.h"
int main(int argc, char **argv) {
    _putenv_s("SSBU_PAD_TRACE", argc > 1 ? argv[1] : "");
    unsigned char pad[40] = {0}, before[40];
    uint64_t buttons = 1; int axes[4] = {120, -240, 360, -480}; unsigned attributes = 3;
    memcpy(pad + 8, &buttons, 8); memcpy(pad + 16, axes, 16); memcpy(pad + 32, &attributes, 4);
    memcpy(before, pad, sizeof pad);
    input_trace_pad(1, pad); /* user's physical controller slot: player 2 */
    input_trace_pad(1, pad); /* unchanged polls must not duplicate rows */
    if (memcmp(before, pad, sizeof pad)) return 1;
    int right_y = 900; memcpy(pad + 28, &right_y, 4);
    input_trace_pad(1, pad); /* right stick alone must be captured */
    input_trace_pad(7, pad); /* player 8 is supported */
    memset(pad, 0, sizeof pad);
    input_trace_pad(1, pad); /* disconnect / release */
    input_trace_pad(8, pad); /* invalid id is ignored */
    return 0;
}

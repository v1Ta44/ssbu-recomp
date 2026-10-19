/* Optional diagnostic observation of the final pad state delivered to the guest.
 * Separate from the player-1 replay format; does not alter input. */
#ifndef SSBU_INPUT_TRACE_H
#define SSBU_INPUT_TRACE_H
static INIT_ONCE pad_trace_once = INIT_ONCE_STATIC_INIT;
static CRITICAL_SECTION pad_trace_lock;
static FILE *pad_trace_file;
static ULONGLONG pad_trace_started;
static unsigned char pad_trace_previous[8][32], pad_trace_seen[8];
static unsigned long long pad_trace_poll[8];
static BOOL CALLBACK pad_trace_init(PINIT_ONCE once, PVOID arg, PVOID *context) {
    (void)once; (void)arg; (void)context;
    const char *path = getenv("SSBU_PAD_TRACE");
    if (!path || !*path) return TRUE;
    InitializeCriticalSection(&pad_trace_lock);
    pad_trace_file = fopen(path, "wb");
    if (!pad_trace_file) { fprintf(stderr, "pad trace: cannot open %s\n", path); return TRUE; }
    pad_trace_started = GetTickCount64();
    fprintf(pad_trace_file, "# observed final Npad state; player numbers 1..8; not SSBU_PLAY format\n");
    fprintf(pad_trace_file, "time_ms,player,poll,buttons_hex,lx,ly,rx,ry,attributes\n");
    fflush(pad_trace_file);
    return TRUE;
}
static void input_trace_pad(unsigned id, const void *state) {
    InitOnceExecuteOnce(&pad_trace_once, pad_trace_init, NULL, NULL);
    if (!pad_trace_file || id >= 8) return;
    const unsigned char *data = (const unsigned char *)state + 8;
    EnterCriticalSection(&pad_trace_lock);
    unsigned long long poll = ++pad_trace_poll[id];
    if (!pad_trace_seen[id] || memcmp(pad_trace_previous[id], data, 32)) {
        unsigned long long buttons;
        int axes[4]; unsigned attributes;
        memcpy(&buttons, data, 8); memcpy(axes, data + 8, 16); memcpy(&attributes, data + 24, 4);
        fprintf(pad_trace_file, "%llu,%u,%llu,%llx,%d,%d,%d,%d,%u\n",
                (unsigned long long)(GetTickCount64() - pad_trace_started), id + 1, poll,
                buttons, axes[0], axes[1], axes[2], axes[3], attributes);
        fflush(pad_trace_file); /* preserve the final changes if this process faults */
        memcpy(pad_trace_previous[id], data, 32); pad_trace_seen[id] = 1;
    }
    LeaveCriticalSection(&pad_trace_lock);
}
#endif

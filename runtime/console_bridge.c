/* console_bridge.c: the network extension's C ABI (yuzu_net.h) for the external console (tools/console), over the named
 * pipe the console names in SSBU_CONSOLE_PIPE. One request line in, one JSON line out; the console polls.
 * Like the in-process window (yuzu_ui.c) the game waits at start until the console says "start": the room is joined
 * before the game creates its sockets. The console going away before that ends the run.
 * Requests (fields separated by tabs, strings as hex of their UTF-8 bytes, so a password may hold any character):
 *   poll                                          -> {"op":"poll", state, error, message, ip, room, members, counters, log}
 *   join <host> <port> <nickname> <password> <token>   -> {"op":"join","code":n}   (asynchronous; poll for state 3 = joined)
 *   leave | level <0..3> | start                  -> {"op":...,"code":n}
 * The password and the token arrive here only: never in the environment, the session files or a log. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "yuzu_net.h"
#include "mod_resources.h"

static HANDLE g_pipe, g_gate;
static volatile LONG g_decision, g_started;
static uint64_t g_log_cursor;
static SsbuNetSnapshot *g_snap;

typedef struct { char *b; size_t n, cap; } Out;
static void put(Out *o, const char *s, size_t n) {
    if (o->n + n + 1 > o->cap) {
        size_t cap = o->cap ? o->cap : 4096;
        while (cap < o->n + n + 1) cap *= 2;
        char *b = realloc(o->b, cap);
        if (!b) return;
        o->b = b; o->cap = cap;
    }
    memcpy(o->b + o->n, s, n); o->n += n; o->b[o->n] = 0;
}
static void putf(Out *o, const char *fmt, ...) {
    char t[512]; va_list a; va_start(a, fmt); int n = vsnprintf(t, sizeof t, fmt, a); va_end(a);
    if (n > 0) put(o, t, (size_t)n < sizeof t ? (size_t)n : sizeof t - 1);
}
static void jstr(Out *o, const char *s, size_t max) {           /* a JSON string; bytes >= 0x80 pass (the console reads lossy UTF-8) */
    put(o, "\"", 1);
    for (size_t i = 0; i < max && s[i]; i++) {
        unsigned char ch = (unsigned char)s[i];
        if (ch == '"' || ch == '\\') { char e[2] = {'\\', (char)ch}; put(o, e, 2); }
        else if (ch < 0x20 || ch == 0x7f) putf(o, "\\u%04x", ch);
        else put(o, (const char *)&ch, 1);
    }
    put(o, "\"", 1);
}
#define JSTR(o, field) jstr(o, field, sizeof field)

static int unhex(const char *h, char *out, size_t cap) {        /* 1 = decoded into a NUL-terminated string that fits */
    size_t n = strlen(h), k = 0;
    if (n % 2 || n / 2 >= cap) return 0;
    for (size_t i = 0; i < n; i += 2) {
        int v = 0;
        for (int j = 0; j < 2; j++) {
            char c = h[i + j];
            v = v * 16 + (c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : c >= 'A' && c <= 'F' ? c - 'A' + 10 : -1000);
        }
        if (v < 0) return 0;
        out[k++] = (char)v;
    }
    out[k] = 0;
    return 1;
}

static void answer_poll(Out *o) {
    SsbuNetSnapshot *s = g_snap;
    s->size = sizeof *s; s->abi = SSBU_NET_ABI;
    if (ssbu_net_snapshot(s)) { putf(o, "{\"op\":\"poll\",\"code\":%d}", SSBU_NET_BAD_ARGUMENT); return; }
    putf(o, "{\"op\":\"poll\",\"code\":0,\"bridge\":1,\"started\":%s,\"state\":%u,\"error\":%u,\"server_version\":%u,\"message\":",
         g_started ? "true" : "false", s->state, s->error, s->server_version);
    JSTR(o, s->message);
    putf(o, ",\"ip\":\"%u.%u.%u.%u\",\"revision\":%llu,\"sent\":%llu,\"received\":%llu,\"dropped\":%llu,\"room\":{\"name\":",
         s->ip[0], s->ip[1], s->ip[2], s->ip[3], (unsigned long long)s->revision, (unsigned long long)s->sent_packets,
         (unsigned long long)s->received_packets, (unsigned long long)s->dropped_packets);
    JSTR(o, s->room_name); put(o, ",\"description\":", 15); JSTR(o, s->room_description);
    putf(o, ",\"slots\":%u,\"port\":%u},\"members\":[", s->room_slots, s->room_port);
    for (uint32_t i = 0; i < s->member_count && i < SSBU_NET_MAX_MEMBERS; i++) {
        const SsbuNetMember *m = &s->members[i];
        put(o, i ? ",{\"nickname\":" : "{\"nickname\":", i ? 13 : 12); JSTR(o, m->nickname);
        put(o, ",\"username\":", 12); JSTR(o, m->username);
        put(o, ",\"display_name\":", 16); JSTR(o, m->display_name);
        put(o, ",\"game_name\":", 13); JSTR(o, m->game_name);
        put(o, ",\"game_version\":", 16); JSTR(o, m->game_version);
        putf(o, ",\"ip\":\"%u.%u.%u.%u\"}", m->ip[0], m->ip[1], m->ip[2], m->ip[3]);
    }
    put(o, "],\"log\":", 8);
    static char chunk[16384];
    uint32_t n = ssbu_net_read_log(&g_log_cursor, chunk, sizeof chunk);
    jstr(o, chunk, n < sizeof chunk ? n : sizeof chunk - 1);
    put(o, "}", 1);
}

static int do_join(char **f, int nf) {
    SsbuNetConfig c;
    memset(&c, 0, sizeof c);
    c.size = sizeof c; c.abi = SSBU_NET_ABI;
    int ok = nf >= 6 && unhex(f[1], c.host, sizeof c.host) && unhex(f[3], c.nickname, sizeof c.nickname) &&
             unhex(f[4], c.password, sizeof c.password) && unhex(f[5], c.token, sizeof c.token);
    long port = nf >= 3 ? strtol(f[2], NULL, 10) : -1;
    int r = SSBU_NET_BAD_ARGUMENT;
    if (ok && port >= 0 && port <= 65535 && c.host[0]) {
        c.port = (uint16_t)port;
        strcpy(c.game_name, "Super Smash Bros. Ultimate"); strcpy(c.game_version, "13.0.4"); c.game_id = 0x01006a800016e000ULL;
        r = ssbu_net_join(&c);
    }
    SecureZeroMemory(&c, sizeof c);
    return r;
}

static void handle(char *line, Out *o) {
    /* Backend-only extension: mods<TAB>UTF-8 JSON, one JSON response line. */
    if (!strncmp(line, "mods\t", 5)) {
        char *reply = ssbu_mods_request(line + 5);
        if (reply) { put(o, reply, strlen(reply)); ssbu_mods_free(reply); }
        else { const char *error = "{\"ok\":false,\"error\":{\"code\":\"out_of_memory\"}}"; put(o, error, strlen(error)); }
        return;
    }
    char *f[8]; int nf = 0;
    for (char *p = line; nf < 8; ) { f[nf++] = p; char *t = strchr(p, '\t'); if (!t) break; *t = 0; p = t + 1; }
    const char *op = f[0];
    if (!strcmp(op, "poll")) { answer_poll(o); return; }
    int code = -1;
    if (!strcmp(op, "join")) code = do_join(f, nf);
    else if (!strcmp(op, "leave")) { ssbu_net_leave(); code = 0; }
    else if (!strcmp(op, "level") && nf >= 2) code = ssbu_net_set_log_level((uint32_t)strtoul(f[1], NULL, 10));
    else if (!strcmp(op, "start")) {
        code = 0;
        if (!InterlockedExchange(&g_started, 1)) { InterlockedExchange(&g_decision, 1); SetEvent(g_gate); printf("console bridge: start\n"); }
    }
    put(o, "{\"op\":", 6); jstr(o, op, 16); putf(o, ",\"code\":%d}", code);
}

static DWORD WINAPI serve(void *unused) {
    (void)unused;
    static char line[16384];
    size_t used = 0;
    Out o = {0};
    for (;;) {
        char *nl = memchr(line, '\n', used);
        if (!nl) {
            DWORD got = 0;
            if (used >= sizeof line - 1 || !ReadFile(g_pipe, line + used, (DWORD)(sizeof line - 1 - used), &got, NULL) || !got) break;
            used += got;
            continue;
        }
        *nl = 0;
        if (nl > line && nl[-1] == '\r') nl[-1] = 0;
        o.n = 0;
        handle(line, &o);
        put(&o, "\n", 1);
        size_t rest = used - (size_t)(nl + 1 - line);
        SecureZeroMemory(line, (size_t)(nl + 1 - line));        /* a join line holds the password */
        memmove(line, nl + 1, rest); used = rest;
        DWORD wrote = 0;
        if (!o.b || !WriteFile(g_pipe, o.b, (DWORD)o.n, &wrote, NULL)) break;
    }
    SecureZeroMemory(line, sizeof line);
    free(o.b);
    CloseHandle(g_pipe); g_pipe = NULL;
    printf("console bridge: the console closed the connection%s\n", g_started ? " (the game and the room connection go on)" : "; the game does not start");
    SetEvent(g_gate);                                       /* before start: decision stays 0, the run ends */
    return 0;
}

/* boot.c: 1 = go on with the game, 0 = end the run (the console was closed before it said start) */
int console_bridge_start(void) {
    wchar_t name[256];
    DWORD n = GetEnvironmentVariableW(L"SSBU_CONSOLE_PIPE", name, 256);
    if (!n || n >= 256) return 1;
    ULONGLONG end = GetTickCount64() + 10000;
    for (;;) {
        g_pipe = CreateFileW(name, GENERIC_READ | GENERIC_WRITE, 0, NULL, OPEN_EXISTING, 0, NULL);
        if (g_pipe != INVALID_HANDLE_VALUE) break;
        DWORD e = GetLastError();
        if (GetTickCount64() > end) { printf("console bridge: the console's pipe cannot be opened (error %lu): the game starts without the network extension\n", e); g_pipe = NULL; return 1; }
        if (e == ERROR_PIPE_BUSY) WaitNamedPipeW(name, 500); else Sleep(100);
    }
    g_snap = calloc(1, sizeof *g_snap);
    g_gate = CreateEventW(NULL, TRUE, FALSE, NULL);
    if (!g_snap || !g_gate) { printf("console bridge: out of memory\n"); return 1; }
    atexit(ssbu_net_leave);
    HANDLE t = CreateThread(NULL, 0, serve, NULL, 0, NULL);
    if (!t) { printf("console bridge: no thread\n"); return 1; }
    CloseHandle(t);
    printf("console bridge: connected; the game waits until the console starts it\n");
    WaitForSingleObject(g_gate, INFINITE);
    return (int)g_decision;
}

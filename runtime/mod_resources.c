/* Independent native resource-package backend, loaded beside the host executable.
 * nnfs serves its sparse read-only archive view. No game binary or archive is rewritten.
 * The DLL owns parsing/cache/profile state; this file only adapts ownership and handles. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "mod_resources.h"
#if SSBU_ENABLE_RESOURCE_MODS
static SRWLOCK g_mod_lock = SRWLOCK_INIT, g_handle_lock = SRWLOCK_INIT;
static INIT_ONCE g_defaults_once = INIT_ONCE_STATIC_INIT;
static HMODULE g_mod_dll;
static char *(*backend_request)(const char *);
static void (*backend_free)(char *);
static uint64_t (*backend_prepare)(const char *);
static int64_t (*backend_read)(uint64_t, void *, size_t);
static uint64_t (*backend_size)(void);
static const char *(*backend_error)(void);
static char g_mod_error[1024], g_default_root[MAX_PATH * 4], g_profile[MAX_PATH * 4];
static uint64_t *g_handles;
static size_t g_handle_count;
static char *copy_text(const char *s) { size_t n = strlen(s) + 1; char *p = malloc(n); if (p) memcpy(p, s, n); return p; }
static void json_string(char *out, size_t cap, const char *text) {
    size_t n = 0; if (cap < 3) return; out[n++] = '"';
    for (const unsigned char *p = (const unsigned char *)text; *p && n + 7 < cap; ++p) {
        if (*p == '"' || *p == '\\') { out[n++] = '\\'; out[n++] = (char)*p; }
        else if (*p < 32) { n += (size_t)snprintf(out + n, cap - n, "\\u%04x", *p); }
        else out[n++] = (char)*p;
    }
    out[n++] = '"'; out[n] = 0;
}
static int native_utf8(const char *text, char *out, size_t cap) {
    wchar_t wide[MAX_PATH * 4];
    if (!MultiByteToWideChar(CP_ACP, MB_ERR_INVALID_CHARS, text, -1, wide, sizeof wide / sizeof *wide)) return 0;
    return WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, wide, -1, out, (int)cap, NULL, NULL) != 0;
}
static void json_native_path(char *out, size_t cap, const char *path) {
    char utf8[MAX_PATH * 12];
    if (!native_utf8(path, utf8, sizeof utf8)) { snprintf(out, cap, "null"); return; }
    json_string(out, cap, utf8);
}
static BOOL CALLBACK setup_defaults(PINIT_ONCE once, void *arg, void **context) {
    (void)once; (void)arg; (void)context;
    char exe[MAX_PATH * 4]; DWORD n = GetModuleFileNameA(NULL, exe, sizeof exe);
    if (!n || n >= sizeof exe) strcpy(exe, ".\\");
    char *p = strrchr(exe, '\\'); if (!p) p = strrchr(exe, '/'); if (p) p[1] = 0; else strcpy(exe, ".\\");
    snprintf(g_default_root, sizeof g_default_root, "%s..\\mods", exe);
    const char *profile = getenv("SSBU_MOD_PROFILE");
    if (profile && profile[0]) snprintf(g_profile, sizeof g_profile, "%s", profile);
    else snprintf(g_profile, sizeof g_profile, "%s..\\config\\mods.json", exe);
    return TRUE;
}
static void defaults(void) { InitOnceExecuteOnce(&g_defaults_once, setup_defaults, NULL, NULL); }
static int backend_call_ok(const char *req) {
    char *reply = backend_request(req); int ok = reply && strstr(reply, "\"ok\":true");
    if (!ok) snprintf(g_mod_error, sizeof g_mod_error, "%s", backend_error());
    if (reply) backend_free(reply); return ok;
}
static int load_backend(void) {
    AcquireSRWLockExclusive(&g_mod_lock);
    if (g_mod_dll) { ReleaseSRWLockExclusive(&g_mod_lock); return 1; }
    defaults();
    char path[MAX_PATH * 4]; DWORD n = GetModuleFileNameA(NULL, path, sizeof path);
    if (!n || n >= sizeof path) { strcpy(g_mod_error, "cannot locate the mod backend"); goto fail; }
    char *p = strrchr(path, '\\'); if (!p) { strcpy(g_mod_error, "invalid executable path"); goto fail; }
    snprintf(p + 1, sizeof path - (size_t)(p + 1 - path), "ssbu_mod_backend.dll");
    HMODULE dll = LoadLibraryExA(path, NULL, LOAD_WITH_ALTERED_SEARCH_PATH);
    if (!dll) { snprintf(g_mod_error, sizeof g_mod_error, "cannot load ssbu_mod_backend.dll (Windows error %lu)", GetLastError()); goto fail; }
#define GET(name, target) target = (void *)GetProcAddress(dll, name); if (!target) { snprintf(g_mod_error, sizeof g_mod_error, "backend is missing %s", name); FreeLibrary(dll); goto fail; }
    uint32_t (*version)(void); GET("sbm_api_version", version);
    if (version() != 1) { strcpy(g_mod_error, "unsupported resource backend ABI"); FreeLibrary(dll); goto fail; }
    GET("sbm_request", backend_request); GET("sbm_free", backend_free); GET("sbm_prepare", backend_prepare);
    GET("sbm_read", backend_read); GET("sbm_size", backend_size); GET("sbm_last_error", backend_error);
#undef GET
    char root[8192], cache[8192], profile[8192], req[26000], cache_path[MAX_PATH * 4];
    snprintf(cache_path, sizeof cache_path, "%s\\..\\cache\\mods", g_default_root);
    json_native_path(root, sizeof root, g_default_root); json_native_path(cache, sizeof cache, cache_path); json_native_path(profile, sizeof profile, g_profile);
    snprintf(req, sizeof req, "{\"op\":\"configure\",\"config\":{\"root\":%s,\"cache\":%s,\"profile\":%s}}", root, cache, profile);
    if (!backend_call_ok(req)) { FreeLibrary(dll); goto fail; }
    /* Load a saved profile, then apply explicit environment overrides. */
    snprintf(req, sizeof req, "{\"op\":\"init\",\"config\":{\"profile\":%s}}", profile);
    if (!backend_call_ok(req)) { FreeLibrary(dll); goto fail; }
    const char *env = getenv("SSBU_RESOURCE_MODS"), *ce = getenv("SSBU_MOD_CACHE"), *region = getenv("SSBU_MOD_REGION");
    if (env && env[0] && strcmp(env, "0")) {
        json_native_path(root, sizeof root, env); snprintf(req, sizeof req, "{\"op\":\"configure\",\"config\":{\"root\":%s}}", root);
        if (!backend_call_ok(req)) { FreeLibrary(dll); goto fail; }
    }
    if (ce && ce[0]) { json_native_path(cache, sizeof cache, ce); snprintf(req, sizeof req, "{\"op\":\"configure\",\"config\":{\"cache\":%s}}", cache); if (!backend_call_ok(req)) { FreeLibrary(dll); goto fail; } }
    if (region && region[0]) { json_string(root, sizeof root, region); snprintf(req, sizeof req, "{\"op\":\"configure\",\"config\":{\"region\":%s}}", root); if (!backend_call_ok(req)) { FreeLibrary(dll); goto fail; } }
    { /* Resolve the same saved language as ARCropolis; explicit region overrides win. */
        extern uint64_t ssbu_desired_language_code(void);
        extern const char *fs_user_root(void);
        uint64_t code = ssbu_desired_language_code(); char language[9] = {0}; memcpy(language, &code, 8);
        json_native_path(root, sizeof root, fs_user_root()); json_string(cache, sizeof cache, language);
        snprintf(req, sizeof req, "{\"op\":\"configure\",\"config\":{\"userdata\":%s,\"system_language\":%s}}", root, cache);
        if (!backend_call_ok(req)) { FreeLibrary(dll); goto fail; }
    }
    InterlockedExchangePointer((void *volatile *)&g_mod_dll, dll); g_mod_error[0] = 0; ReleaseSRWLockExclusive(&g_mod_lock); return 1;
fail:
    ReleaseSRWLockExclusive(&g_mod_lock); return 0;
}
uint32_t ssbu_mods_api_version(void) { return 1; }
char *ssbu_mods_request(const char *request) {
    if (!load_backend()) { char escaped[2200], reply[2400]; json_string(escaped, sizeof escaped, g_mod_error); snprintf(reply, sizeof reply, "{\"ok\":false,\"error\":{\"code\":\"backend_unavailable\",\"message\":%s}}", escaped); return copy_text(reply); }
    char *reply = backend_request(request); char *owned = reply ? copy_text(reply) : NULL;
    if (reply) backend_free(reply); return owned;
}
void ssbu_mods_free(char *reply) { free(reply); }
static int contains_handle(uint64_t h) {
    int found = 0; AcquireSRWLockShared(&g_handle_lock);
    for (size_t i = 0; i < g_handle_count; ++i) if (g_handles[i] == h) { found = 1; break; }
    ReleaseSRWLockShared(&g_handle_lock); return found;
}
int mod_resources_open(uint64_t h, const char *path) {
    const char *env = getenv("SSBU_RESOURCE_MODS"); if (env && !strcmp(env, "0")) return 1;
    defaults();
    if (!InterlockedCompareExchangePointer((void *volatile *)&g_mod_dll, NULL, NULL) && !(env && env[0]) && GetFileAttributesA(g_profile) == INVALID_FILE_ATTRIBUTES && GetFileAttributesA(g_default_root) == INVALID_FILE_ATTRIBUTES) return 1;
    if (!load_backend()) { fprintf(stderr, "mods: %s\n", g_mod_error); return 0; }
    char utf8[MAX_PATH * 12];
    if (!native_utf8(path, utf8, sizeof utf8)) { fprintf(stderr, "mods: archive path cannot be converted to UTF-8\n"); return 0; }
    uint64_t n = backend_prepare(utf8);
    if (!n) { fprintf(stderr, "mods: archive preparation failed: %s\n", backend_error()); return 0; }
    AcquireSRWLockExclusive(&g_handle_lock);
    uint64_t *next = realloc(g_handles, (g_handle_count + 1) * sizeof *next);
    if (!next) { ReleaseSRWLockExclusive(&g_handle_lock); return 0; }
    g_handles = next; g_handles[g_handle_count++] = h; ReleaseSRWLockExclusive(&g_handle_lock);
    printf("mods: read-only virtual archive ready (%llu bytes)\n", (unsigned long long)n); return 1;
}
void mod_resources_close(uint64_t h) {
    AcquireSRWLockExclusive(&g_handle_lock);
    for (size_t i = 0; i < g_handle_count; ++i) if (g_handles[i] == h) { g_handles[i] = g_handles[--g_handle_count]; break; }
    ReleaseSRWLockExclusive(&g_handle_lock);
}
int mod_resources_read(uint64_t h, int64_t offset, void *buffer, uint64_t size, uint64_t *read) {
    if (!contains_handle(h)) return 0;
    if (offset < 0 || size > SIZE_MAX || size > INT64_MAX) { *read = 0; return -1; }
    int64_t n = backend_read((uint64_t)offset, buffer, (size_t)size);
    *read = n < 0 ? 0 : (uint64_t)n;
    if (n < 0) fprintf(stderr, "mods: read failed: %s\n", backend_error());
    return n < 0 ? -1 : 1;
}
int mod_resources_size(uint64_t h, uint64_t *size) { if (!contains_handle(h)) return 0; *size = backend_size(); return 1; }

#else
/* Keep the IPC/C ABI usable in a vanilla build without loading any resource backend. */
uint32_t ssbu_mods_api_version(void) { return 1; }
char *ssbu_mods_request(const char *request) {
    (void)request;
    const char reply[] = "{\"ok\":false,\"error\":{\"code\":\"feature_disabled\",\"message\":\"Resource packages were disabled at build time; rebuild with --with-mods\"}}";
    char *owned = malloc(sizeof reply);
    if (owned) memcpy(owned, reply, sizeof reply);
    return owned;
}
void ssbu_mods_free(char *reply) { free(reply); }
int mod_resources_open(uint64_t handle, const char *path) { (void)handle; (void)path; return 1; }
void mod_resources_close(uint64_t handle) { (void)handle; }
int mod_resources_read(uint64_t handle, int64_t offset, void *buffer, uint64_t size, uint64_t *read) {
    (void)handle; (void)offset; (void)buffer; (void)size; (void)read; return 0;
}
int mod_resources_size(uint64_t handle, uint64_t *size) { (void)handle; (void)size; return 0; }
#endif

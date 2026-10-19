/* Resource package backend, ABI 1. UTF-8 JSON; no frontend implementation.
 * Game inputs are read-only. See docs/MOD_RESOURCES.md for operations and ownership. */
#ifndef SSBU_MOD_RESOURCES_H
#define SSBU_MOD_RESOURCES_H
#include <stdint.h>
#include <stddef.h>
/* Resource packages are opt-in at host build time. PC hook DLLs are independent. */
#ifndef SSBU_ENABLE_RESOURCE_MODS
#define SSBU_ENABLE_RESOURCE_MODS 0
#endif
#ifdef __cplusplus
extern "C" {
#endif
#if defined(_WIN32)
#define SSBU_MOD_EXPORT __declspec(dllexport)
#else
#define SSBU_MOD_EXPORT
#endif
/* Request returns owned JSON. Always release through ssbu_mods_free, including errors.
 * Operations are serialized. Configuration changes apply to the next game process. */
SSBU_MOD_EXPORT uint32_t ssbu_mods_api_version(void);
SSBU_MOD_EXPORT char *ssbu_mods_request(const char *request_utf8);
SSBU_MOD_EXPORT void ssbu_mods_free(char *response_utf8);
/* Host filesystem integration; the public frontend uses the request API above. */
int mod_resources_open(uint64_t handle, const char *host_path);
void mod_resources_close(uint64_t handle);
int mod_resources_read(uint64_t handle, int64_t offset, void *buffer, uint64_t size, uint64_t *read);
int mod_resources_size(uint64_t handle, uint64_t *size);
#ifdef __cplusplus
}
#endif
#endif

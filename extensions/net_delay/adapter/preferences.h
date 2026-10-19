#pragma once
#include <stdint.h>
namespace nd_preferences {
/* Strict UTF-8 format: version=1\nframes=auto|0..25\n. */
bool parse(const char *data,int *frames);
bool load(const wchar_t *path,int *frames);
bool save(const wchar_t *path,int frames);
}

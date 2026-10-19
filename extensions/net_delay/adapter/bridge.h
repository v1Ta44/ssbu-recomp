#pragma once
#include "delay_api.h"
struct NdBridge;
struct NdProvider {
    int (*read)(NdSnapshot *);
    int (*select)(int);
    int (*preferences)();
    void (*log)(const char *);
};
NdBridge *nd_bridge_start(const NdProvider *provider,const wchar_t *frontend,const wchar_t *assets,const wchar_t *data,bool hidden,bool selftest=false,uintptr_t owner=0);
void nd_bridge_stop(NdBridge *bridge);

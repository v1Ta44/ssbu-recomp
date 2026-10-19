#include "preferences.h"
#include "delay_api.h"
#include <windows.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

bool nd_preferences::parse(const char *data,int *frames) {
    if(!data || !frames)return false;
    char version[24]={},value[32]={};char tail=0;
    if(sscanf(data,"version=%23[^\n]\nframes=%31[^\n]\n %c",version,value,&tail)!=2 || strcmp(version,"1"))return false;
    if(!strcmp(value,"auto")){*frames=ND_AUTO;return true;}
    if(!*value)return false;
    for(const char *p=value;*p;p++)if(*p<'0' || *p>'9')return false;
    char *end;long n=strtol(value,&end,10);
    if(*end || n<0 || n>ND_MAX_FRAMES)return false;
    *frames=(int)n;return true;
}
bool nd_preferences::load(const wchar_t *path,int *frames) {
    if(!path || !*path)return false;
    FILE *f=_wfopen(path,L"rb");if(!f)return false;
    char bytes[128]={};size_t n=fread(bytes,1,sizeof bytes-1,f);bool ok=!ferror(f) && feof(f);fclose(f);
    if(!ok || memchr(bytes,0,n))return false;
    return parse(bytes,frames);
}
bool nd_preferences::save(const wchar_t *path,int frames) {
    if(!path || !*path || frames<ND_AUTO || frames>ND_MAX_FRAMES)return false;
    wchar_t temporary[32768];
    int n=_snwprintf(temporary,32768,L"%ls.%lu.tmp",path,GetCurrentProcessId());
    if(n<0 || n>=32768)return false;
    FILE *f=_wfopen(temporary,L"wb");if(!f)return false;
    char value[16];if(frames==ND_AUTO)strcpy(value,"auto");else snprintf(value,sizeof value,"%d",frames);
    bool ok=fprintf(f,"version=1\nframes=%s\n",value)>0;
    if(fflush(f))ok=false;if(fclose(f))ok=false;
    if(ok)ok=MoveFileExW(temporary,path,MOVEFILE_REPLACE_EXISTING|MOVEFILE_WRITE_THROUGH)!=0;
    if(!ok)DeleteFileW(temporary);
    return ok;
}

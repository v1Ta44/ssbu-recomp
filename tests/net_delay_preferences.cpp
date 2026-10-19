#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include "preferences.h"
#define CHECK(x) do{if(!(x)){fprintf(stderr,"FAIL line %d %s\n",__LINE__,#x);return 1;}}while(0)
int wmain(int argc,wchar_t **argv){
    CHECK(argc==2);int value=999;
    CHECK(nd_preferences::parse("version=1\nframes=auto\n",&value) && value==-1);
    CHECK(nd_preferences::parse("version=1\nframes=3\n",&value) && value==3);
    const char *bad[]={"","version=2\nframes=3\n","version=1\nframes=26\n","version=1\nframes=-1\n","version=1\nframes=3oops\n","version=1\nframes=auto\nextra=1\n","version=1\nframes=99999999999999999999999999999\n"};
    for(const char *text:bad)CHECK(!nd_preferences::parse(text,&value));
    for(int frames=-1;frames<=25;frames++){
        CHECK(nd_preferences::save(argv[1],frames));value=999;CHECK(nd_preferences::load(argv[1],&value) && value==frames);
    }
    CHECK(!nd_preferences::save(argv[1],26));CHECK(nd_preferences::load(argv[1],&value) && value==25);
    FILE *file=_wfopen(argv[1],L"wb");CHECK(file);const char invalid[]="version=1\nframes=3\0extra";
    fwrite(invalid,1,sizeof invalid,file);fclose(file);CHECK(!nd_preferences::load(argv[1],&value));
    CHECK(nd_preferences::save(argv[1],-1));
    puts("PASS preferences validation, 27 atomic save/reload round trips and corrupt-file fallback");return 0;
}

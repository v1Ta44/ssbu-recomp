/* Exercises the real Console frontend through the private pipe, with a fake
 * off-screen owner window. No game, renderer or interactive desktop needed. */
#include <windows.h>
#include <stdio.h>
#include <string>
#include "delay_api.h"
#include "delay_ui.h"
#include "bridge.h"
#define CHECK(x) do{if(!(x)){fprintf(stderr,"FAIL line %d %s\n",__LINE__,#x);return 1;}}while(0)
static NdEngine *engine;
static int reads(NdSnapshot *s){return nd_snapshot(engine,s);}
static int writer(void *,int){return 1;}
static int select(int value){
    if(!nd_select(engine,value))return 0;
    NdSession s={1,1,1,2,1,1};NdSample sample={1,4,4,0,0};
    nd_evaluate(engine,&s,&s,&sample,writer,NULL,NULL);return 1;
}
static int prefs(){return 1;}
static void log(const char *s){puts(s);}
static void pump(){MSG m;while(PeekMessageW(&m,NULL,0,0,PM_REMOVE)){TranslateMessage(&m);DispatchMessageW(&m);}Sleep(10);}
static BOOL CALLBACK matching(HWND w,LPARAM out){
    wchar_t name[96];if(GetClassNameW(w,name,96) && !wcscmp(name,ND_UI_CLASS)){*(HWND *)out=w;return FALSE;}return TRUE;
}
static HWND panel(){HWND result=NULL;EnumWindows(matching,(LPARAM)&result);return result;}
static bool aligned(HWND owner,HWND overlay){
    RECT r,client;POINT origin={0,0};GetClientRect(owner,&client);ClientToScreen(owner,&origin);GetWindowRect(overlay,&r);
    return abs((r.right+12)-(origin.x+client.right))<=2 && abs(r.top-origin.y-12)<=2;
}
int wmain(int argc,wchar_t **argv){
    SetProcessDPIAware();CHECK(argc==4);engine=nd_create();nd_backend(engine,ND_BACKEND_READY);
    NdSession session={1,1,1,2,1,1};nd_session(engine,&session);
    NdProvider provider={reads,select,prefs,log};
    WNDCLASSW cls={};cls.lpfnWndProc=DefWindowProcW;cls.hInstance=GetModuleHandleW(NULL);cls.lpszClassName=L"DelayOverlayOwnerFixture";
    CHECK(RegisterClassW(&cls));
    HWND owner=CreateWindowExW(WS_EX_TOOLWINDOW|WS_EX_NOACTIVATE,cls.lpszClassName,L"Off-screen fixture",WS_OVERLAPPEDWINDOW,-24000,-24000,1280,900,NULL,NULL,cls.hInstance,NULL);CHECK(owner);
    ShowWindow(owner,SW_SHOWNOACTIVATE);
    NdBridge *bridge=nd_bridge_start(&provider,argv[1],argv[2],argv[3],true,true,(uintptr_t)owner);CHECK(bridge);
    HWND overlay=NULL;ULONGLONG deadline=GetTickCount64()+10000;
    while(!(overlay=panel()) && GetTickCount64()<deadline)pump();CHECK(overlay);
    DWORD pid=0;GetWindowThreadProcessId(overlay,&pid);HANDLE process=OpenProcess(SYNCHRONIZE|PROCESS_QUERY_LIMITED_INFORMATION,FALSE,pid);CHECK(process);
    CHECK(GetWindow(overlay,GW_OWNER)==owner && !IsWindowVisible(overlay));
    deadline=GetTickCount64()+2000;while(!aligned(owner,overlay) && GetTickCount64()<deadline)pump();CHECK(aligned(owner,overlay));
    SetWindowPos(owner,NULL,-23000,-23000,1024,820,SWP_NOACTIVATE|SWP_NOZORDER);
    deadline=GetTickCount64()+2000;while(!aligned(owner,overlay) && GetTickCount64()<deadline)pump();CHECK(aligned(owner,overlay));
    // Reproduce the real launch: create WebView under a hidden HWND first.
    deadline=GetTickCount64()+12000;
    while(!GetPropW(overlay,L"NdPanelReady") && GetTickCount64()<deadline)pump();
    CHECK(GetPropW(overlay,L"NdPanelReady"));
    CHECK(!IsWindowVisible(overlay) && !GetPropW(overlay,L"NdWebViewVisible"));
    for(int cycle=0;cycle<3;cycle++){
        SendMessageW(overlay,(WM_APP+242),0,0);
        deadline=GetTickCount64()+1000;
        while((!IsWindowVisible(overlay) || !GetPropW(overlay,L"NdWebViewVisible")) && GetTickCount64()<deadline)pump();
        CHECK(IsWindowVisible(overlay) && GetPropW(overlay,L"NdWebViewVisible"));
        CHECK(aligned(owner,overlay));
        SendMessageW(overlay,WM_CLOSE,0,0);
        CHECK(!IsWindowVisible(overlay) && !GetPropW(overlay,L"NdWebViewVisible"));
    }
    SendMessageW(overlay,(WM_APP+242),0,0);
    ShowWindow(owner,SW_HIDE);
    deadline=GetTickCount64()+1000;
    while(IsWindowVisible(overlay) && GetTickCount64()<deadline)pump();
    CHECK(!IsWindowVisible(overlay) && !GetPropW(overlay,L"NdWebViewVisible"));
    ShowWindow(owner,SW_SHOWNOACTIVATE);
    deadline=GetTickCount64()+1000;
    while((!IsWindowVisible(overlay) || !GetPropW(overlay,L"NdWebViewVisible")) && GetTickCount64()<deadline)pump();
    CHECK(IsWindowVisible(overlay) && GetPropW(overlay,L"NdWebViewVisible"));
    puts("PASS owner hide/restore synchronizes native and browser visibility");
    puts("PASS hidden-start browser visibility, three expand/collapse cycles, no collapsed HWND or hit area");
    puts("PASS explicit owner, resize/move following and in-client bounds");
    bool changed=false,returned=false;deadline=GetTickCount64()+18000;
    while(GetTickCount64()<deadline){
        NdSnapshot state={};state.size=sizeof state;state.abi=ND_ABI;CHECK(nd_snapshot(engine,&state));
        if(state.selection_revision>=1)changed=true;
        if(state.selection_revision>=5 && state.selected==3 && state.writes>=4){returned=true;break;}
        pump();
    }
    deadline=GetTickCount64()+5000;while(WaitForSingleObject(process,0)!=WAIT_OBJECT_0 && GetTickCount64()<deadline)pump();
    CHECK(WaitForSingleObject(process,0)==WAIT_OBJECT_0);DWORD code;CHECK(GetExitCodeProcess(process,&code) && code==0);CloseHandle(process);
    nd_bridge_stop(bridge);CHECK(changed && returned);
    puts("PASS real WebView2 DOM -> isolated frontend -> private pipe -> backend -> rendered state");
    // A hidden live frontend has no arbitrary lifetime; it follows its owner.
    bridge=nd_bridge_start(&provider,argv[1],argv[2],argv[3],true,false,(uintptr_t)owner);CHECK(bridge);
    deadline=GetTickCount64()+10000;while(!(overlay=panel()) && GetTickCount64()<deadline)pump();CHECK(overlay);
    GetWindowThreadProcessId(overlay,&pid);process=OpenProcess(SYNCHRONIZE,FALSE,pid);CHECK(process);
    DestroyWindow(owner);deadline=GetTickCount64()+5000;
    while(WaitForSingleObject(process,0)!=WAIT_OBJECT_0 && GetTickCount64()<deadline)pump();
    CHECK(WaitForSingleObject(process,0)==WAIT_OBJECT_0);CloseHandle(process);nd_bridge_stop(bridge);
    puts("PASS owner close shuts down frontend without terminating game/backend");
    nd_destroy(engine);return 0;
}

/* Minimal in-process Win32 controller. The game waits for explicit Start after
 * room admission. UI operations use only the public backend ABI. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shellapi.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>
#include "yuzu_net.h"
#pragma comment(lib,"shell32.lib")

enum { ID_HOST=201,ID_PORT,ID_NICK,ID_PASSWORD,ID_TOKEN,ID_JOIN,ID_LEAVE,ID_START,ID_LEVEL,ID_LOG };
static HWND window,host,port,nick,password,token,join_button,leave_button,start_button,status_text,stats_text,members,logs,level_box;
static HFONT font;
static HANDLE start_gate,operation;
static volatile LONG busy,operation_result;
static int game_started,decision;
static uint64_t log_cursor;
static SsbuNetSnapshot snapshot;
static SsbuNetMember displayed[SSBU_NET_MAX_MEMBERS];
static unsigned displayed_count=~0u;
static wchar_t log_path[32768];
typedef struct { int leave;SsbuNetConfig config; } Operation;

static void set_text(HWND control,const wchar_t *text) {
    wchar_t old[1024];GetWindowTextW(control,old,1024);if(wcscmp(old,text))SetWindowTextW(control,text);
}
static int get_utf8(HWND control,char *out,int capacity) {
    int n=GetWindowTextLengthW(control);if(n>=capacity)return 0;
    wchar_t *text=calloc((size_t)n+1,sizeof *text);if(!text)return 0;
    GetWindowTextW(control,text,n+1);
    int ok=WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,text,-1,out,capacity,NULL,NULL)!=0;
    SecureZeroMemory(text,((size_t)n+1)*sizeof *text);free(text);return ok;
}
static const wchar_t *error_text(unsigned error) {
    static const wchar_t *text[]={L"",L"配置无效：检查地址和昵称（4–20 位英文字母、数字、空格或 ._-）。",L"游戏仍在本地无线会话中，请先在游戏里退出本地无线模式。",L"网络或系统错误，请查看日志。",L"连接超时，请检查地址、UDP 端口和网络。",L"服务器协议版本不匹配。",L"房间密码错误。",L"房间昵称不合法或已被占用。",L"服务器房间已满。",L"账号或地址被封禁。",L"已被房间移除。",L"服务器连接已断开。",L"服务器返回了无效数据。",L"发送队列已满。",L"虚拟 IP 冲突。"};
    return error<sizeof text/sizeof text[0]?text[error]:L"未知错误，请查看日志。";
}
static DWORD WINAPI perform(void *arg) {
    Operation *op=arg;int result=0;
    if(op->leave)ssbu_net_leave();else result=ssbu_net_join(&op->config);
    SecureZeroMemory(op,sizeof *op);free(op);
    InterlockedExchange(&operation_result,result);InterlockedExchange(&busy,0);return 0;
}
static void begin_operation(int leave) {
    if(InterlockedCompareExchange(&busy,0,0))return;
    Operation *op=calloc(1,sizeof *op);if(!op)return;
    op->leave=leave;SsbuNetConfig *c=&op->config;c->size=sizeof *c;c->abi=SSBU_NET_ABI;
    if(!leave) {
        wchar_t p[16],*end;GetWindowTextW(port,p,16);long number=wcstol(p,&end,10);
        if(!*p || *end || number<1 || number>65535 || !get_utf8(host,c->host,sizeof c->host) || !c->host[0] ||
            !get_utf8(nick,c->nickname,sizeof c->nickname) || !get_utf8(password,c->password,sizeof c->password) || !get_utf8(token,c->token,sizeof c->token)) {
            SecureZeroMemory(op,sizeof *op);free(op);MessageBoxW(window,L"请填写服务器地址和有效端口，检查字段是否过长。",L"连接配置",MB_OK|MB_ICONINFORMATION);return;
        }
        c->port=(uint16_t)number;strcpy(c->game_name,"Super Smash Bros. Ultimate");strcpy(c->game_version,"13.0.4");c->game_id=0x01006a800016e000ULL;
    }
    if(operation){CloseHandle(operation);operation=NULL;}
    InterlockedExchange(&operation_result,0);InterlockedExchange(&busy,1);
    operation=CreateThread(NULL,0,perform,op,0,NULL);
    if(!operation){SecureZeroMemory(op,sizeof *op);free(op);InterlockedExchange(&operation_result,SSBU_NET_IO);InterlockedExchange(&busy,0);}
}
static void refresh(void) {
    snapshot.size=sizeof snapshot;snapshot.abi=SSBU_NET_ABI;ssbu_net_snapshot(&snapshot);
    int working=InterlockedCompareExchange(&busy,0,0),active=snapshot.state>=SSBU_NET_CONNECTING && snapshot.state<=SSBU_NET_JOINED;
    EnableWindow(join_button,!working && !active);EnableWindow(leave_button,!working && snapshot.state!=SSBU_NET_IDLE);
    EnableWindow(start_button,!working && !game_started && snapshot.state==SSBU_NET_JOINED);
    HWND inputs[]={host,port,nick,password,token};for(unsigned i=0;i<5;i++)EnableWindow(inputs[i],!working && !active);
    static const wchar_t *states[]={L"未连接",L"连接服务器中…",L"等待房间准入…",L"已加入服务器房间",L"连接失败 / 已断开"};
    wchar_t text[1024];unsigned error=(unsigned)InterlockedCompareExchange(&operation_result,0,0);if(!error)error=snapshot.error;
    _snwprintf(text,1024,L"%ls%ls  %ls",working?L"正在处理…  ":L"",states[snapshot.state],error_text(error));text[1023]=0;set_text(status_text,text);
    _snwprintf(text,1024,L"虚拟 IP：%u.%u.%u.%u    成员：%u    发送：%llu  接收：%llu  丢弃：%llu",
        snapshot.ip[0],snapshot.ip[1],snapshot.ip[2],snapshot.ip[3],snapshot.member_count,
        (unsigned long long)snapshot.sent_packets,(unsigned long long)snapshot.received_packets,(unsigned long long)snapshot.dropped_packets);set_text(stats_text,text);
    if(displayed_count!=snapshot.member_count || memcmp(displayed,snapshot.members,snapshot.member_count*sizeof *displayed)) {
        SendMessageW(members,WM_SETREDRAW,FALSE,0);SendMessageW(members,LB_RESETCONTENT,0,0);
        for(unsigned i=0;i<snapshot.member_count;i++) {
            SsbuNetMember *m=&snapshot.members[i];wchar_t name[64]=L"?",version[64]=L"";
            MultiByteToWideChar(CP_UTF8,0,m->nickname,-1,name,64);MultiByteToWideChar(CP_UTF8,0,m->game_version,-1,version,64);
            _snwprintf(text,1024,L"%ls    %u.%u.%u.%u    %ls",name,m->ip[0],m->ip[1],m->ip[2],m->ip[3],version);SendMessageW(members,LB_ADDSTRING,0,(LPARAM)text);
        }
        displayed_count=snapshot.member_count;memcpy(displayed,snapshot.members,displayed_count*sizeof *displayed);
        SendMessageW(members,WM_SETREDRAW,TRUE,0);InvalidateRect(members,NULL,TRUE);
    }
    char chunk[16384];uint32_t n=ssbu_net_read_log(&log_cursor,chunk,sizeof chunk);
    if(n) {
        wchar_t wide[32768],decoded[16384];int used=0;
        int chars=MultiByteToWideChar(CP_UTF8,0,chunk,(int)n,decoded,16384);
        for(int i=0;i<chars && used<32765;i++){if(decoded[i]=='\n')wide[used++]=L'\r';wide[used++]=decoded[i];}wide[used]=0;
        int len=GetWindowTextLengthW(logs);
        if(len>60000){SendMessageW(logs,EM_SETSEL,0,30000);SendMessageW(logs,EM_REPLACESEL,FALSE,(LPARAM)L"");len=GetWindowTextLengthW(logs);}
        SendMessageW(logs,EM_SETSEL,len,len);SendMessageW(logs,EM_REPLACESEL,FALSE,(LPARAM)wide);SendMessageW(logs,EM_SCROLLCARET,0,0);
    }
}
static HWND control(const wchar_t *kind,const wchar_t *text,DWORD style,int x,int y,int w,int h,int id) {
    HWND c=CreateWindowExW(!wcscmp(kind,L"EDIT") || !wcscmp(kind,L"LISTBOX")?WS_EX_CLIENTEDGE:0,kind,text,WS_CHILD|WS_VISIBLE|style,x,y,w,h,window,(HMENU)(INT_PTR)id,GetModuleHandleW(NULL),NULL);
    SendMessageW(c,WM_SETFONT,(WPARAM)font,TRUE);return c;
}
static LRESULT CALLBACK procedure(HWND w,UINT message,WPARAM wp,LPARAM lp) {
    switch(message) {
    case WM_CREATE:
        window=w;font=CreateFontW(-16,0,0,0,FW_NORMAL,0,0,0,DEFAULT_CHARSET,0,0,CLEARTYPE_QUALITY,0,L"Microsoft YaHei UI");
        { char name[33]; wchar_t wide[33], label[160]; ssbu_net_get_profile(name);
          MultiByteToWideChar(CP_UTF8,0,name,-1,wide,33);
          swprintf(label,160,L"游戏内名字：%ls    头像：默认    ｜    先连接服务器，再启动游戏",wide);
          control(L"STATIC",label,0,16,14,765,24,0); }
        control(L"STATIC",L"服务器地址",0,16,51,90,24,0);host=control(L"EDIT",L"",ES_AUTOHSCROLL|WS_TABSTOP,110,47,435,28,ID_HOST);
        control(L"STATIC",L"UDP 端口",0,565,51,80,24,0);port=control(L"EDIT",L"24872",ES_NUMBER|WS_TABSTOP,650,47,130,28,ID_PORT);
        control(L"STATIC",L"房间昵称",0,16,89,90,24,0);nick=control(L"EDIT",L"vitas",ES_AUTOHSCROLL|WS_TABSTOP,110,85,245,28,ID_NICK);
        control(L"STATIC",L"房间密码",0,380,89,90,24,0);password=control(L"EDIT",L"",ES_PASSWORD|ES_AUTOHSCROLL|WS_TABSTOP,475,85,305,28,ID_PASSWORD);
        control(L"STATIC",L"Token 选填",0,16,127,95,24,0);token=control(L"EDIT",L"",ES_PASSWORD|ES_AUTOHSCROLL|WS_TABSTOP,110,123,670,28,ID_TOKEN);
        SendMessageW(host,EM_SETLIMITTEXT,255,0);SendMessageW(port,EM_SETLIMITTEXT,5,0);SendMessageW(nick,EM_SETLIMITTEXT,20,0);SendMessageW(password,EM_SETLIMITTEXT,127,0);SendMessageW(token,EM_SETLIMITTEXT,4095,0);
        join_button=control(L"BUTTON",L"连接",WS_TABSTOP,16,166,100,32,ID_JOIN);leave_button=control(L"BUTTON",L"断开",WS_TABSTOP,126,166,100,32,ID_LEAVE);
        start_button=control(L"BUTTON",L"启动游戏",WS_TABSTOP,236,166,125,32,ID_START);
        level_box=control(L"COMBOBOX",L"",CBS_DROPDOWNLIST|WS_TABSTOP,380,166,185,150,ID_LEVEL);
        const wchar_t *levels[]={L"日志：关闭",L"日志：状态",L"日志：调试",L"日志：收发详情"};for(int i=0;i<4;i++)SendMessageW(level_box,CB_ADDSTRING,0,(LPARAM)levels[i]);SendMessageW(level_box,CB_SETCURSEL,2,0);
        control(L"BUTTON",L"打开日志文件",WS_TABSTOP,590,166,190,32,ID_LOG);
        status_text=control(L"STATIC",L"",0,16,211,764,46,0);stats_text=control(L"STATIC",L"",0,16,257,764,24,0);
        control(L"STATIC",L"服务器成员（昵称 / 虚拟 IP / 游戏版本）",0,16,291,764,24,0);
        members=control(L"LISTBOX",L"",WS_VSCROLL|LBS_NOINTEGRALHEIGHT,16,318,764,92,0);
        control(L"STATIC",L"联机日志",0,16,421,764,23,0);logs=control(L"EDIT",L"",ES_READONLY|ES_MULTILINE|ES_AUTOVSCROLL|WS_VSCROLL,16,449,764,158,0);SendMessageW(logs,EM_SETLIMITTEXT,90000,0);
        control(L"STATIC",L"游戏里选择「本地无线」。断开后重新连接前，先退出游戏里的本地无线会话。",0,16,617,764,25,0);
        SetTimer(w,1,250,NULL);refresh();SetFocus(host);return 0;
    case WM_TIMER:refresh();return 0;
    case WM_COMMAND:
        switch(LOWORD(wp)) {
        case ID_JOIN:begin_operation(0);refresh();break;
        case ID_LEAVE:begin_operation(1);refresh();break;
        case ID_START:
            refresh();if(!game_started && snapshot.state==SSBU_NET_JOINED && !busy){game_started=decision=1;SetWindowTextW(start_button,L"游戏已启动");SetEvent(start_gate);refresh();}break;
        case ID_LEVEL:if(HIWORD(wp)==CBN_SELCHANGE)ssbu_net_set_log_level((uint32_t)SendMessageW(level_box,CB_GETCURSEL,0,0));break;
        case ID_LOG:if(log_path[0])ShellExecuteW(w,L"open",log_path,NULL,NULL,SW_SHOWNORMAL);break;
        }return 0;
    case WM_CLOSE:DestroyWindow(w);return 0;
    case WM_DESTROY:KillTimer(w,1);DeleteObject(font);if(!game_started)SetEvent(start_gate);PostQuitMessage(0);return 0;
    }
    return DefWindowProcW(w,message,wp,lp);
}
static DWORD WINAPI ui_thread(void *unused) {
    (void)unused;WNDCLASSW cls={0};cls.lpfnWndProc=procedure;cls.hInstance=GetModuleHandleW(NULL);cls.hCursor=LoadCursor(NULL,IDC_ARROW);cls.hbrBackground=(HBRUSH)(COLOR_BTNFACE+1);cls.lpszClassName=L"SsbuYuzuNetworkControl";
    if(!RegisterClassW(&cls)){SetEvent(start_gate);return 0;}
    RECT rect={0,0,800,653};DWORD style=WS_OVERLAPPED|WS_CAPTION|WS_SYSMENU|WS_MINIMIZEBOX;AdjustWindowRect(&rect,style,FALSE);
    window=CreateWindowExW(0,cls.lpszClassName,L"SSBU · Yuzu 联机实测",style,CW_USEDEFAULT,CW_USEDEFAULT,rect.right-rect.left,rect.bottom-rect.top,NULL,NULL,cls.hInstance,NULL);
    if(!window){SetEvent(start_gate);return 0;}ShowWindow(window,SW_SHOW);UpdateWindow(window);
    MSG msg;while(GetMessageW(&msg,NULL,0,0)>0){if(!IsDialogMessageW(window,&msg)){TranslateMessage(&msg);DispatchMessageW(&msg);}}return 0;
}
int ssbu_net_ui_start(void) {
    ssbu_net_set_log_level(2);
    atexit(ssbu_net_leave);GetEnvironmentVariableW(L"SSBU_NET_UI_LOG",log_path,32768);
    start_gate=CreateEvent(NULL,TRUE,FALSE,NULL);if(!start_gate)return 0;
    HANDLE thread=CreateThread(NULL,0,ui_thread,NULL,0,NULL);if(!thread){CloseHandle(start_gate);return 0;}
    WaitForSingleObject(start_gate,INFINITE);
    if(!decision){WaitForSingleObject(thread,INFINITE);if(operation)WaitForSingleObject(operation,INFINITE);}
    CloseHandle(thread);CloseHandle(start_gate);return decision;
}

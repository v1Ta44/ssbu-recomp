/* Console-style owned overlay. This executable knows only delay_ui.h, never
 * HookApi, game memory, Yuzu, Vulkan or game DLLs. */
#include <windows.h>
#include <shellapi.h>
#include <shlwapi.h>
#include <wrl.h>
#include <WebView2.h>
#include <stdio.h>
#include <stdlib.h>
#include <string>
#include <mutex>
#include "delay_ui.h"
using Microsoft::WRL::ComPtr;
using Microsoft::WRL::Callback;

namespace {
const UINT update_message=WM_APP+241,toggle_message=WM_APP+242;
const int no_pending=-99;
struct App {
    HWND window=NULL,owner=NULL;HANDLE parent=NULL,stop=NULL,wake=NULL,worker=NULL;
    DWORD parent_pid=0;std::wstring pipe,assets,data,capture,theme;
    bool hidden=false,preview=false,expanded=false,ready=false,capturing=false,fonts_ready=false,selftest=false,applied=false,f8_down=false,owner_requested=false,integration=false,load_failed=false;
    int test_stage=0,exit_code=0;ULONGLONG font_time=0,started=GetTickCount64();
    volatile LONG pending=no_pending;bool online=false,command_failed=false;
    NdReply reply={};std::mutex lock;
    ComPtr<ICoreWebView2Environment> environment;
    ComPtr<ICoreWebView2Controller> controller;
    ComPtr<ICoreWebView2> web;
} app;
bool io(HANDLE file,void *data,DWORD length,bool write) {
    OVERLAPPED op={};op.hEvent=CreateEventW(NULL,TRUE,FALSE,NULL);if(!op.hEvent)return false;
    DWORD done=0;BOOL ok=write?WriteFile(file,data,length,&done,&op):ReadFile(file,data,length,&done,&op);
    if(!ok && GetLastError()==ERROR_IO_PENDING){
        HANDLE events[]={app.stop,op.hEvent};DWORD result=WaitForMultipleObjects(2,events,FALSE,1000);
        if(result==WAIT_OBJECT_0+1)ok=GetOverlappedResult(file,&op,&done,FALSE);
        else {CancelIoEx(file,&op);GetOverlappedResult(file,&op,&done,TRUE);ok=FALSE;}
    }
    CloseHandle(op.hEvent);return ok && done==length;
}
bool exchange(const NdRequest &request,NdReply *reply) {
    HANDLE file=CreateFileW(app.pipe.c_str(),GENERIC_READ|GENERIC_WRITE,0,NULL,OPEN_EXISTING,FILE_FLAG_OVERLAPPED,NULL);
    if(file==INVALID_HANDLE_VALUE)return false;
    DWORD mode=PIPE_READMODE_MESSAGE;bool ok=SetNamedPipeHandleState(file,&mode,NULL,NULL)!=0;
    if(ok)ok=io(file,(void *)&request,sizeof request,true) && io(file,reply,sizeof *reply,false);
    CloseHandle(file);
    return ok && reply->size==sizeof *reply && reply->abi==ND_UI_ABI && reply->state.size==sizeof(NdSnapshot) && reply->state.abi==ND_ABI;
}
DWORD WINAPI worker(void *) {
    while(WaitForSingleObject(app.stop,0)!=WAIT_OBJECT_0){
        int desired=(int)InterlockedExchange(&app.pending,no_pending);
        NdRequest request={sizeof request,ND_UI_ABI,(uint32_t)(desired==no_pending?ND_READ:ND_SELECT),desired};NdReply reply={};
        bool online=exchange(request,&reply);
        {
            std::lock_guard<std::mutex> guard(app.lock);app.online=online;
            app.command_failed=online && request.operation==ND_SELECT && !reply.ok;
            if(online)app.reply=reply;
        }
        if(!online && desired!=no_pending)InterlockedCompareExchange(&app.pending,desired,no_pending);
        PostMessageW(app.window,update_message,0,0);
        HANDLE events[]={app.stop,app.wake};WaitForMultipleObjects(2,events,FALSE,250);
    }
    return 0;
}
void send_state() {
    if(!app.ready || !app.web)return;
    NdReply reply;bool online,failed;
    {std::lock_guard<std::mutex> guard(app.lock);reply=app.reply;online=app.online;failed=app.command_failed;}
    NdSnapshot &s=reply.state;
    bool active=s.live.available && s.live.connected && (s.live.role==1 || s.live.role==2) && s.live.peers>=2 && s.live.peers<=8;
    bool applied=s.decision==ND_APPLIED && s.effective==s.selected && s.applied_revision==s.selection_revision &&
        s.live.connection==s.last_session.connection && s.live.session==s.last_session.session && s.live.role==s.last_session.role;
    wchar_t json[1400];
    swprintf(json,1400,L"{\"type\":\"state\",\"connected\":%ls,\"selected\":%d,\"backend\":%d,\"decision\":%d,\"automatic\":%d,\"effective\":%d,\"calls\":%llu,\"writes\":%llu,\"active\":%ls,\"applied\":%ls,\"peers\":%u,\"selection_revision\":%llu,\"applied_revision\":%llu,\"preferences\":%d,\"command_failed\":%ls}",
        online?L"true":L"false",s.selected,s.backend,s.decision,s.automatic,s.effective,(unsigned long long)s.calls,(unsigned long long)s.writes,active?L"true":L"false",applied?L"true":L"false",s.live.peers,
        (unsigned long long)s.selection_revision,(unsigned long long)s.applied_revision,reply.preferences,failed?L"true":L"false");
    app.web->PostWebMessageAsJson(json);
}
void diagnostic(const wchar_t *event, HRESULT result=S_OK);
void refresh_visibility();
void bounds() {
    if(!app.controller)return;RECT rect;GetClientRect(app.window,&rect);app.controller->put_Bounds(rect);
}
void geometry(bool force=false) {
    RECT client={0,0,1280,720};POINT origin={0,0};
    if(app.owner){GetClientRect(app.owner,&client);ClientToScreen(app.owner,&origin);}
    int available_w=client.right-client.left,available_h=client.bottom-client.top;
    typedef UINT (WINAPI *GetDpi)(HWND);GetDpi dpi=(GetDpi)(void *)GetProcAddress(GetModuleHandleW(L"user32.dll"),"GetDpiForWindow");
    double scale=app.owner && dpi?(double)dpi(app.owner)/96.0:1.0;
    double fit=min((double)(available_w-24)/500.0,(double)(available_h-24)/720.0);
    if(fit<scale)scale=fit;if(scale<0.4)scale=0.4;
    int width=(int)(500*scale),height=(int)(720*scale);
    int x=origin.x+available_w-width-12,y=origin.y+12;
    if(app.preview){x=-30000;y=-30000;}
    RECT old;GetWindowRect(app.window,&old);
    bool changed=force || old.left!=x || old.top!=y || old.right-old.left!=width || old.bottom-old.top!=height;
    if(changed)SetWindowPos(app.window,NULL,x,y,width,height,SWP_NOZORDER|SWP_NOACTIVATE|SWP_NOOWNERZORDER);
    if(app.controller && changed){
        app.controller->put_ZoomFactor(scale);
        app.controller->NotifyParentWindowPositionChanged();
        bounds();
    }
}
void expand(bool value) {
    app.expanded=value;geometry(true);
    if(app.ready)app.web->PostWebMessageAsJson(value?L"{\"type\":\"expanded\",\"value\":true}":L"{\"type\":\"expanded\",\"value\":false}");
    if(!value && app.owner && !app.hidden && GetForegroundWindow()==app.window)SetForegroundWindow(app.owner);
    refresh_visibility();
}
BOOL CALLBACK find_owner(HWND w,LPARAM data) {
    DWORD pid=0;GetWindowThreadProcessId(w,&pid);if(pid!=app.parent_pid || GetWindow(w,GW_OWNER) || !IsWindowVisible(w))return TRUE;
    RECT r;GetClientRect(w,&r);LONG area=r.right*r.bottom;LONG *largest=(LONG *)data;
    if(area>*largest){*largest=area;app.owner=w;}return TRUE;
}
void preview_select(int frames) {
    std::lock_guard<std::mutex> guard(app.lock);
    NdSnapshot &s=app.reply.state;s.selected=frames;s.selection_revision++;app.reply.preferences=1;
    if(app.applied){s.calls++;s.decision=frames==-1?ND_AUTO_RESULT:ND_APPLIED;s.effective=frames==-1?4:frames;s.applied_revision=s.selection_revision;s.last_session=s.live;}
}
void capture() {
    app.capturing=true;ComPtr<IStream> stream;
    HRESULT hr=SHCreateStreamOnFileEx(app.capture.c_str(),STGM_CREATE|STGM_READWRITE|STGM_SHARE_EXCLUSIVE,FILE_ATTRIBUTE_NORMAL,TRUE,NULL,&stream);
    if(FAILED(hr)){app.exit_code=4;PostQuitMessage(4);return;}
    app.web->CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,stream.Get(),
        Callback<ICoreWebView2CapturePreviewCompletedHandler>([stream](HRESULT result)->HRESULT {
            diagnostic(L"capture-completed",result);stream->Commit(STGC_DEFAULT);app.exit_code=SUCCEEDED(result)?0:5;PostQuitMessage(app.exit_code);return S_OK;
        }).Get());
}
void fail(const wchar_t *text,HRESULT reason=E_FAIL) {
    OutputDebugStringW(text);app.exit_code=2;app.load_failed=true;diagnostic(text,reason);
    if(app.hidden || app.preview)PostQuitMessage(2);
    else {refresh_visibility();InvalidateRect(app.window,NULL,TRUE);}
}
void message(const wchar_t *text) {
    if(!wcscmp(text,L"ready")){app.ready=true;expand(app.expanded);send_state();app.web->ExecuteScript(L"document.fonts.ready.then(()=>window.chrome.webview.postMessage('fonts-ready'))",NULL);return;}
    if(!wcscmp(text,L"fonts-ready")){app.fonts_ready=true;app.font_time=GetTickCount64();SetPropW(app.window,L"NdPanelReady",(HANDLE)1);diagnostic(L"page-and-fonts-ready");return;}
    if(!wcscmp(text,L"hotkey")){if(!app.f8_down){app.f8_down=true;expand(!app.expanded);}return;}
    if(!wcscmp(text,L"expand")){expand(true);return;}
    if(!wcscmp(text,L"collapse")){expand(false);return;}
    if(!wcscmp(text,L"test-pass")){
        if(app.integration){
            BOOL visible=FALSE;app.controller->get_IsVisible(&visible);
            if(!IsWindowVisible(app.window) || !visible){fail(L"Visible-window regression failed");return;}
            app.capture=app.data+L"\\window-render.png";capture();
        }else {app.exit_code=0;PostQuitMessage(0);}
        return;
    }
    if(!wcsncmp(text,L"test-fail:",10)){fail(text);return;}
    if(!wcsncmp(text,L"select:",7)){
        const wchar_t *value=text+7;wchar_t *end;long n=wcstol(value,&end,10);
        if(!*value || *end || n<ND_AUTO || n>ND_MAX_FRAMES)return;
        if(app.preview){preview_select((int)n);send_state();}
        else {InterlockedExchange(&app.pending,(LONG)n);SetEvent(app.wake);}
    }
}
void diagnostic(const wchar_t *event, HRESULT result) {
    std::wstring path=app.data+L"\\frontend.log";
    FILE *file=_wfopen(path.c_str(),L"a, ccs=UTF-8");
    if(!file)return;
    BOOL visible=FALSE;if(app.controller)app.controller->get_IsVisible(&visible);
    RECT r={};if(app.window)GetClientRect(app.window,&r);
    fwprintf(file,L"%llu %ls hr=0x%08lx hwnd_visible=%d web_visible=%d ready=%d expanded=%d bounds=%ldx%ld\n",
        GetTickCount64(),event,(unsigned long)result,IsWindowVisible(app.window),visible,app.ready,app.expanded,r.right,r.bottom);
    fclose(file);
}
// A WebView created while its HWND is hidden starts with IsVisible=FALSE.
// HWND visibility alone does not restore that independent controller state.
void refresh_visibility() {
    bool eligible=app.preview;
    if(!app.preview && app.owner && !IsIconic(app.owner) && IsWindowVisible(app.owner)){
        HWND foreground=GetForegroundWindow();
        eligible=app.integration || foreground==app.owner || foreground==app.window || GetAncestor(foreground,GA_ROOT)==app.window;
    }
    bool show=app.expanded && eligible && !app.hidden;
    if(!show && app.controller){
        BOOL actual=FALSE;app.controller->get_IsVisible(&actual);
        if(actual){app.controller->put_IsVisible(FALSE);diagnostic(L"browser-hidden");}
    }
    if(show && !IsWindowVisible(app.window))ShowWindow(app.window,SW_SHOWNOACTIVATE);
    else if(!show && IsWindowVisible(app.window))ShowWindow(app.window,SW_HIDE);
    if(app.controller){
        BOOL wanted=IsWindowVisible(app.window) && app.ready && !app.load_failed;
        BOOL actual=FALSE;app.controller->get_IsVisible(&actual);
        if(actual!=wanted){app.controller->put_IsVisible(wanted);diagnostic(wanted?L"browser-shown":L"browser-hidden");}
        app.controller->get_IsVisible(&actual);
        SetPropW(app.window,L"NdWebViewVisible",(HANDLE)(uintptr_t)(actual?1:0));
    }
}
void init_webview() {
    diagnostic(L"startup");
    HRESULT hr=CreateCoreWebView2EnvironmentWithOptions(NULL,app.data.c_str(),NULL,
        Callback<ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler>([](HRESULT result,ICoreWebView2Environment *environment)->HRESULT {
            if(FAILED(result) || !environment){fail(L"WebView2 runtime unavailable",result);return S_OK;}
            app.environment=environment;
            HRESULT created=environment->CreateCoreWebView2Controller(app.window,
                Callback<ICoreWebView2CreateCoreWebView2ControllerCompletedHandler>([](HRESULT code,ICoreWebView2Controller *controller)->HRESULT {
                    if(FAILED(code) || !controller){fail(L"WebView2 controller unavailable",code);return S_OK;}
                    app.controller=controller;
                    if(FAILED(controller->get_CoreWebView2(&app.web)) || !app.web){fail(L"WebView2 content unavailable");return S_OK;}
                    bounds();diagnostic(L"controller-created");refresh_visibility();
                    ComPtr<ICoreWebView2Settings> settings;app.web->get_Settings(&settings);
                    settings->put_AreDevToolsEnabled(FALSE);settings->put_AreDefaultContextMenusEnabled(FALSE);settings->put_IsStatusBarEnabled(FALSE);
                    ComPtr<ICoreWebView2_3> folders;
                    if(FAILED(app.web.As(&folders))){fail(L"WebView2 virtual-host mapping unavailable");return S_OK;}
                    if(FAILED(folders->SetVirtualHostNameToFolderMapping(L"ssbu-delay.local",app.assets.c_str(),COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS))){fail(L"WebView2 local assets unavailable");return S_OK;}
                    if(!app.theme.empty()){
                        ComPtr<ICoreWebView2_13> v13;ComPtr<ICoreWebView2Profile> profile;
                        if(SUCCEEDED(app.web.As(&v13)) && SUCCEEDED(v13->get_Profile(&profile)))
                            profile->put_PreferredColorScheme(app.theme==L"light"?COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT:COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK);
                    }
                    EventRegistrationToken token;
                    app.web->add_WebMessageReceived(Callback<ICoreWebView2WebMessageReceivedEventHandler>([](ICoreWebView2 *,ICoreWebView2WebMessageReceivedEventArgs *args)->HRESULT {
                        LPWSTR source=NULL,text=NULL;args->get_Source(&source);
                        if(source && !wcscmp(source,L"https://ssbu-delay.local/index.html") && SUCCEEDED(args->TryGetWebMessageAsString(&text)) && text)message(text);
                        CoTaskMemFree(source);CoTaskMemFree(text);return S_OK;
                    }).Get(),&token);
                    app.web->add_NavigationStarting(Callback<ICoreWebView2NavigationStartingEventHandler>([](ICoreWebView2 *,ICoreWebView2NavigationStartingEventArgs *args)->HRESULT {
                        LPWSTR uri=NULL;args->get_Uri(&uri);if(!uri || wcscmp(uri,L"https://ssbu-delay.local/index.html"))args->put_Cancel(TRUE);CoTaskMemFree(uri);return S_OK;
                    }).Get(),&token);
                    app.web->add_NewWindowRequested(Callback<ICoreWebView2NewWindowRequestedEventHandler>([](ICoreWebView2 *,ICoreWebView2NewWindowRequestedEventArgs *args)->HRESULT {args->put_Handled(TRUE);return S_OK;}).Get(),&token);
                    app.web->add_NavigationCompleted(Callback<ICoreWebView2NavigationCompletedEventHandler>([](ICoreWebView2 *,ICoreWebView2NavigationCompletedEventArgs *args)->HRESULT {
                        BOOL success=FALSE;args->get_IsSuccess(&success);
                        if(!success)fail(L"WebView2 page navigation failed");else diagnostic(L"navigation-completed");
                        return S_OK;
                    }).Get(),&token);
                    app.web->add_ProcessFailed(Callback<ICoreWebView2ProcessFailedEventHandler>([](ICoreWebView2 *,ICoreWebView2ProcessFailedEventArgs *)->HRESULT {fail(L"WebView2 renderer process failed");return S_OK;}).Get(),&token);
                    if(FAILED(app.web->Navigate(L"https://ssbu-delay.local/index.html")))fail(L"WebView2 navigation could not start");
                    return S_OK;
                }).Get());
            if(FAILED(created))fail(L"WebView2 controller creation failed",created);return S_OK;
        }).Get());
    if(FAILED(hr))fail(L"WebView2 startup failed",hr);
}
void selftest() {
    if(!app.selftest || !app.fonts_ready)return;
    if(GetTickCount64()-app.font_time<650)return;
    // Drive the real DOM and WebMessage bridge, not a second UI implementation.
    NdReply current;{std::lock_guard<std::mutex> guard(app.lock);current=app.reply;}
    switch(app.test_stage){
    case 0:app.test_stage++;app.web->ExecuteScript(L"document.getElementById('manual').click()",NULL);break;
    case 1:if(current.state.selected!=3)return;app.test_stage++;app.web->ExecuteScript(L"document.getElementById('plus').click()",NULL);break;
    case 2:if(current.state.selected!=4)return;app.test_stage++;app.web->ExecuteScript(L"document.getElementById('minus').click()",NULL);break;
    case 3:if(current.state.selected!=3)return;app.test_stage++;app.web->ExecuteScript(L"document.getElementById('reset').click()",NULL);break;
    case 4:if(current.state.selected!=-1)return;app.test_stage++;app.web->ExecuteScript(L"(()=>{const r=document.getElementById('frames');r.dispatchEvent(new Event('pointerdown'));r.value='3';r.dispatchEvent(new Event('input'));setTimeout(()=>{if(r.value!=='3'){chrome.webview.postMessage('test-fail:slider-reset');return;}r.dispatchEvent(new Event('change'));},350);})()",NULL);break;
    case 5:if(current.state.selected!=3)return;app.test_stage++;app.web->ExecuteScript(L"setTimeout(()=>{const s=document.getElementById('selected').textContent;const css=getComputedStyle(document.querySelector('.btn'));if(s==='3'&&css.clipPath!=='none'){chrome.webview.postMessage('test-pass');}else chrome.webview.postMessage('test-fail:dom-or-console-style');},400)",NULL);break;
    }
}
LRESULT CALLBACK procedure(HWND window,UINT message_id,WPARAM wp,LPARAM lp) {
    switch(message_id){
    case WM_SIZE:bounds();return 0;
    case update_message:send_state();return 0;
    case toggle_message:expand(!app.expanded);return 0;
    case WM_TIMER:{
        if(!app.preview){
            if(app.parent && WaitForSingleObject(app.parent,0)==WAIT_OBJECT_0){DestroyWindow(window);return 0;}
            if(app.owner_requested && !IsWindow(app.owner)){DestroyWindow(window);return 0;}
            if(!IsWindow(app.owner)){
                LONG largest=0;app.owner=NULL;EnumWindows(find_owner,(LPARAM)&largest);
                if(app.owner)SetWindowLongPtrW(window,GWLP_HWNDPARENT,(LONG_PTR)app.owner);
            }
            bool active=false;
            if(app.owner && !IsIconic(app.owner) && IsWindowVisible(app.owner)){
                HWND fg=GetForegroundWindow();active=app.integration || fg==app.owner || fg==window || GetAncestor(fg,GA_ROOT)==window;
            }
            if(app.owner && !IsIconic(app.owner))geometry();
            bool key=(GetAsyncKeyState(VK_F8)&0x8000)!=0;
            if(active && key && !app.f8_down)expand(!app.expanded);app.f8_down=key;
            refresh_visibility();
        }
        if(app.ready){send_state();selftest();}
        if(!app.ready && !app.load_failed && GetTickCount64()-app.started>20000)fail(L"WebView2 page readiness timed out");
        if(app.preview && app.fonts_ready && !app.selftest && !app.capture.empty() && !app.capturing && GetTickCount64()-app.font_time>900)capture();
        if((app.preview || app.selftest || (app.hidden && !app.ready)) && GetTickCount64()-app.started>20000){app.exit_code=3;DestroyWindow(window);}
        return 0;}
    case WM_PAINT:{
        PAINTSTRUCT paint;HDC dc=BeginPaint(window,&paint);
        if(!app.ready || app.load_failed){
            RECT area;GetClientRect(window,&area);FillRect(dc,&area,GetSysColorBrush(COLOR_WINDOW));
            InflateRect(&area,-24,-24);SetBkMode(dc,TRANSPARENT);SetTextColor(dc,GetSysColor(COLOR_WINDOWTEXT));
            HFONT font=(HFONT)GetStockObject(DEFAULT_GUI_FONT);HGDIOBJ old=SelectObject(dc,font);
            DrawTextW(dc,app.load_failed?L"减延迟面板加载失败。\n请重启游戏；具体记录见本次运行的 interface-cache\\frontend.log。\n已选择的后端设置仍然保留。\n\n按 F8 或 Esc 隐藏。":L"正在加载减延迟设置…\n按 F8 隐藏。",-1,&area,DT_LEFT|DT_TOP|DT_WORDBREAK);
            SelectObject(dc,old);
        }
        EndPaint(window,&paint);return 0;}
    case WM_KEYDOWN:if(wp==VK_ESCAPE){expand(false);return 0;}break;
    case WM_CLOSE:expand(false);return 0;
    case WM_DESTROY:KillTimer(window,1);PostQuitMessage(app.exit_code);return 0;
    }
    return DefWindowProcW(window,message_id,wp,lp);
}
}
int WINAPI wWinMain(HINSTANCE instance,HINSTANCE,PWSTR,int) {
    int count;LPWSTR *args=CommandLineToArgvW(GetCommandLineW(),&count);
    for(int i=1;i<count;i++){
        std::wstring flag=args[i];
        if(flag==L"--preview")app.preview=true;
        else if(flag==L"--hidden")app.hidden=true;
        else if(flag==L"--expanded")app.expanded=true;
        else if(flag==L"--applied")app.applied=true;
        else if(flag==L"--integration-test"){app.selftest=app.integration=true;app.expanded=app.hidden=false;}
        else if(flag==L"--selftest"){app.selftest=app.preview=app.expanded=true;}
        else if(i+1<count){std::wstring value=args[++i];if(flag==L"--pipe")app.pipe=value;else if(flag==L"--pid")app.parent_pid=wcstoul(value.c_str(),NULL,10);else if(flag==L"--owner"){app.owner=(HWND)(uintptr_t)_wcstoui64(value.c_str(),NULL,10);app.owner_requested=true;}else if(flag==L"--assets")app.assets=value;else if(flag==L"--data")app.data=value;else if(flag==L"--capture")app.capture=value;else if(flag==L"--theme")app.theme=value;else {LocalFree(args);return 1;}}
        else {LocalFree(args);return 1;}
    }
    LocalFree(args);
    if(app.assets.empty() || app.data.empty() || (!app.preview && (!app.parent_pid || app.pipe.empty())))return 1;
    SetProcessDPIAware();CoInitializeEx(NULL,COINIT_APARTMENTTHREADED);
    if(!app.preview){app.parent=OpenProcess(SYNCHRONIZE,FALSE,app.parent_pid);if(!app.parent)return 1;}
    else {app.online=true;app.reply.ok=1;app.reply.state.size=sizeof(NdSnapshot);app.reply.state.abi=ND_ABI;app.reply.state.backend=ND_BACKEND_READY;app.reply.state.selected=app.applied?3:-1;app.reply.state.automatic=app.applied?4:-1;app.reply.state.effective=app.applied?3:-1;app.reply.state.calls=app.applied?120:0;app.reply.state.live.available=1;app.reply.state.live.connected=app.applied;app.reply.state.live.role=app.applied?1:0;app.reply.state.live.peers=app.applied?2:0;app.reply.state.last_session=app.reply.state.live;app.reply.state.decision=app.applied?ND_APPLIED:ND_AUTO_RESULT;}
    app.stop=CreateEventW(NULL,TRUE,FALSE,NULL);app.wake=CreateEventW(NULL,FALSE,FALSE,NULL);
    WNDCLASSW cls={};cls.lpfnWndProc=procedure;cls.hInstance=instance;cls.lpszClassName=ND_UI_CLASS;cls.hCursor=LoadCursor(NULL,IDC_ARROW);cls.hbrBackground=GetSysColorBrush(COLOR_WINDOW);
    RegisterClassW(&cls);
    if(app.owner_requested){DWORD pid=0;GetWindowThreadProcessId(app.owner,&pid);if(!IsWindow(app.owner) || pid!=app.parent_pid)return 1;}
    else if(!app.preview){LONG largest=0;EnumWindows(find_owner,(LPARAM)&largest);}
    app.window=CreateWindowExW(WS_EX_TOOLWINDOW,ND_UI_CLASS,L"SSBU · 联机减延迟",WS_POPUP|WS_CLIPCHILDREN|WS_CLIPSIBLINGS,-30000,-30000,500,720,app.owner,NULL,instance,NULL);
    if(!app.window)return 1;geometry(true);
    // Capture tests render off-screen without stealing focus or covering the desktop.
    if(app.preview && app.expanded)ShowWindow(app.window,SW_SHOWNOACTIVATE);
    CreateDirectoryW(app.data.c_str(),NULL);
    SetTimer(app.window,1,50,NULL);
    if(!app.preview)app.worker=CreateThread(NULL,0,worker,NULL,0,NULL);
    init_webview();MSG msg;
    while(GetMessageW(&msg,NULL,0,0)>0){TranslateMessage(&msg);DispatchMessageW(&msg);}
    SetEvent(app.stop);SetEvent(app.wake);if(app.worker){WaitForSingleObject(app.worker,INFINITE);CloseHandle(app.worker);}
    if(app.controller)app.controller->Close();app.web.Reset();app.controller.Reset();app.environment.Reset();
    if(app.parent)CloseHandle(app.parent);CloseHandle(app.stop);CloseHandle(app.wake);CoUninitialize();return app.exit_code;
}

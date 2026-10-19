#include "bridge.h"
#include "delay_ui.h"
#include <windows.h>
#include <sddl.h>
#include <bcrypt.h>
#include <stdio.h>
#include <new>
#include <string>

struct NdBridge { NdProvider provider;HANDLE stop,thread,process;DWORD pid;std::wstring name; };
namespace {
bool transfer(HANDLE pipe,HANDLE stop,void *bytes,DWORD length,bool write) {
    OVERLAPPED op={};op.hEvent=CreateEventW(NULL,TRUE,FALSE,NULL);if(!op.hEvent)return false;
    DWORD done=0;BOOL ok=write?WriteFile(pipe,bytes,length,&done,&op):ReadFile(pipe,bytes,length,&done,&op);
    if(!ok && GetLastError()==ERROR_IO_PENDING){
        HANDLE handles[]={stop,op.hEvent};DWORD result=WaitForMultipleObjects(2,handles,FALSE,1500);
        if(result==WAIT_OBJECT_0+1)ok=GetOverlappedResult(pipe,&op,&done,FALSE);
        else {CancelIoEx(pipe,&op);GetOverlappedResult(pipe,&op,&done,TRUE);ok=FALSE;}
    }
    CloseHandle(op.hEvent);return ok && done==length;
}
PSECURITY_DESCRIPTOR own_user_acl() {
    HANDLE token;DWORD size=0;
    if(!OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&token))return NULL;
    GetTokenInformation(token,TokenUser,NULL,0,&size);
    BYTE *buffer=new(std::nothrow) BYTE[size];
    if(!buffer){CloseHandle(token);return NULL;}
    bool ok=GetTokenInformation(token,TokenUser,buffer,size,&size)!=0;LPWSTR sid=NULL;
    if(ok)ok=ConvertSidToStringSidW(((TOKEN_USER *)buffer)->User.Sid,&sid)!=0;
    PSECURITY_DESCRIPTOR descriptor=NULL;
    if(ok){std::wstring text=L"D:P(A;;GA;;;";text+=sid;text+=L")";ConvertStringSecurityDescriptorToSecurityDescriptorW(text.c_str(),SDDL_REVISION_1,&descriptor,NULL);}
    if(sid)LocalFree(sid);delete[] buffer;CloseHandle(token);return descriptor;
}
DWORD WINAPI serve(void *arg) {
    NdBridge *bridge=(NdBridge *)arg;
    PSECURITY_DESCRIPTOR acl=own_user_acl();if(!acl){bridge->provider.log("net-delay: cannot create local frontend ACL");return 1;}
    SECURITY_ATTRIBUTES sa={sizeof sa,acl,FALSE};
    bool announced=false;
    while(WaitForSingleObject(bridge->stop,0)!=WAIT_OBJECT_0){
        HANDLE pipe=CreateNamedPipeW(bridge->name.c_str(),PIPE_ACCESS_DUPLEX|FILE_FLAG_OVERLAPPED|FILE_FLAG_FIRST_PIPE_INSTANCE,
                                     PIPE_TYPE_MESSAGE|PIPE_READMODE_MESSAGE|PIPE_WAIT|PIPE_REJECT_REMOTE_CLIENTS,1,sizeof(NdReply),sizeof(NdRequest),1000,&sa);
        if(pipe==INVALID_HANDLE_VALUE)break;
        OVERLAPPED connect={};connect.hEvent=CreateEventW(NULL,TRUE,FALSE,NULL);
        if(!connect.hEvent){CloseHandle(pipe);break;}
        BOOL joined=ConnectNamedPipe(pipe,&connect);DWORD err=joined?0:GetLastError();
        if(err==ERROR_PIPE_CONNECTED)joined=TRUE;
        else if(err==ERROR_IO_PENDING){
            HANDLE handles[]={bridge->stop,connect.hEvent};DWORD result=WaitForMultipleObjects(2,handles,FALSE,INFINITE);DWORD bytes;
            if(result==WAIT_OBJECT_0+1)joined=GetOverlappedResult(pipe,&connect,&bytes,FALSE);
            else {CancelIoEx(pipe,&connect);GetOverlappedResult(pipe,&connect,&bytes,TRUE);joined=FALSE;}
        }
        CloseHandle(connect.hEvent);
        ULONG client=0;NdRequest request={};NdReply reply={};reply.size=sizeof reply;reply.abi=ND_UI_ABI;
        if(joined && GetNamedPipeClientProcessId(pipe,&client) && client==bridge->pid &&
           transfer(pipe,bridge->stop,&request,sizeof request,false) && request.size==sizeof request && request.abi==ND_UI_ABI){
            if(!announced){bridge->provider.log("net-delay: Console frontend connected over local control API");announced=true;}
            if(request.operation==ND_READ)reply.ok=1;
            else if(request.operation==ND_SELECT)reply.ok=bridge->provider.select(request.frames);
            reply.preferences=bridge->provider.preferences();reply.state.size=sizeof reply.state;reply.state.abi=ND_ABI;
            if(!bridge->provider.read(&reply.state))reply.ok=0;
            transfer(pipe,bridge->stop,&reply,sizeof reply,true);
        }
        DisconnectNamedPipe(pipe);CloseHandle(pipe);
    }
    LocalFree(acl);return 0;
}
}
NdBridge *nd_bridge_start(const NdProvider *provider,const wchar_t *frontend,const wchar_t *assets,const wchar_t *data,bool hidden,bool selftest,uintptr_t owner) {
    NdBridge *b=new(std::nothrow) NdBridge{};if(!b)return NULL;b->provider=*provider;
    unsigned char nonce[16];if(BCryptGenRandom(NULL,nonce,sizeof nonce,BCRYPT_USE_SYSTEM_PREFERRED_RNG)<0){delete b;return NULL;}
    wchar_t name[160];int n=swprintf(name,160,L"\\\\.\\pipe\\ssbu-net-delay-%lu-",GetCurrentProcessId());
    for(int i=0;i<16;i++)swprintf(name+n+i*2,3,L"%02x",nonce[i]);b->name=name;
    b->stop=CreateEventW(NULL,TRUE,FALSE,NULL);
    std::wstring command=L"\"";command+=frontend;command+=L"\" --pipe \"";command+=b->name;command+=L"\" --pid ";
    wchar_t pid[32];swprintf(pid,32,L"%lu",GetCurrentProcessId());command+=pid;
    command+=L" --assets \"";command+=assets;command+=L"\" --data \"";command+=data;command+=L"\"";
    if(owner){wchar_t handle[40];swprintf(handle,40,L"%llu",(unsigned long long)owner);command+=L" --owner ";command+=handle;}
    if(hidden)command+=L" --hidden";
    if(selftest)command+=L" --integration-test";
    STARTUPINFOW si={};si.cb=sizeof si;si.dwFlags=STARTF_USESHOWWINDOW;si.wShowWindow=SW_HIDE;
    PROCESS_INFORMATION pi={};
    if(!b->stop || !CreateProcessW(frontend,&command[0],NULL,NULL,FALSE,CREATE_NO_WINDOW|CREATE_SUSPENDED,NULL,NULL,&si,&pi)){
        if(b->stop)CloseHandle(b->stop);delete b;return NULL;
    }
    b->pid=pi.dwProcessId;b->process=pi.hProcess;
    b->thread=CreateThread(NULL,0,serve,b,0,NULL);
    if(!b->thread){TerminateProcess(pi.hProcess,1);CloseHandle(pi.hThread);CloseHandle(pi.hProcess);CloseHandle(b->stop);delete b;return NULL;}
    ResumeThread(pi.hThread);CloseHandle(pi.hThread);
    return b;
}
void nd_bridge_stop(NdBridge *b) {
    if(!b)return;SetEvent(b->stop);WaitForSingleObject(b->thread,INFINITE);
    CloseHandle(b->thread);CloseHandle(b->stop);CloseHandle(b->process);delete b;
    /* The frontend observes parent death; no forced termination of a user's UI. */
}

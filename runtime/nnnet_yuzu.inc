/* Included after nnnet_c4a.inc. Keep offline handlers intact; only virtual FDs
 * and explicitly enabled room networking take these paths. */
#include "yuzu_net_internal.h"
#define YPTR(a) ((void *)(uintptr_t)(a))
static int yfd(uint64_t fd) { return fd>=YN_VFD_BASE && fd<=0x3fffffff; }
static void yret(Ctx *c,int r) {
    if(r<0){yn_log(r==-11?3:2,"sdk.socket","fd=%d errno=%d caller=0x%llx",(int)A0,-r,(unsigned long long)c->x[30]);so_fail(c,-r);}else RETI(r);
}
#define YLDN(call) do { uint32_t result=(call);yn_log(2,"sdk.ldn","%s result=0x%x caller=0x%llx",#call,result,(unsigned long long)c->x[30]);RET(result); } while(0)
static void y_socket(Ctx *c) {
    if(!yn_enabled()){socket_Socket(c);return;}
    so_need_init(c,"nn::socket::Socket");
    if((A0&255)!=2){so_fail(c,97);return;}
    yret(c,yn_socket_create((int)A1,(int)A2));
}
static void y_finalize(Ctx *c) { yn_socket_close_all();socket_Finalize(c); }
static void y_close(Ctx *c) { if(yfd(A0))yret(c,yn_socket_close((int)A0));else socket_Close(c); }
static void y_bind(Ctx *c) {
    if(!yfd(A0)){socket_Bind(c);return;}
    struct sockaddr_in a;if(!A1 || A2<8){so_fail(c,22);return;}
    if(!so_addr_in(A1,&a)){so_fail(c,97);return;}yret(c,yn_socket_bind((int)A0,&a));
}
static void y_bind_typed(Ctx *c) { if(!yfd(A0))socket_BindSockAddr(c);else y_bind(c); }
static void y_connect(Ctx *c) {
    if(!yfd(A0)){socket_Connect(c);return;}
    struct sockaddr_in a;if(!A1 || A2<8){so_fail(c,22);return;}
    if(!so_addr_in(A1,&a)){so_fail(c,97);return;}yret(c,yn_socket_connect((int)A0,&a));
}
static void y_listen(Ctx *c) { if(yfd(A0))so_fail(c,95);else socket_Listen(c); }
static void y_accept(Ctx *c) { if(yfd(A0))so_fail(c,95);else socket_Accept(c); }
static void y_shutdown(Ctx *c) { if(yfd(A0))yret(c,yn_socket_shutdown((int)A0,(int)A1));else socket_Shutdown(c); }
static void y_name(Ctx *c,int peer) {
    if(!yfd(A0)){so_name(c,peer);return;}
    if(!A1 || !A2 || !M32(A2)){so_fail(c,22);return;}
    struct sockaddr_in a;int r=yn_socket_name((int)A0,&a,peer);
    if(!r)M32(A2)=so_addr_out(A1,M32(A2),&a);yret(c,r);
}
static void y_sockname(Ctx *c){y_name(c,0);}
static void y_peername(Ctx *c){y_name(c,1);}
static void y_recv_impl(Ctx *c,int from) {
    if(!yfd(A0)){if(from)socket_RecvFrom(c);else socket_Recv(c);return;}
    if(A2>0x7fffffff){so_fail(c,14);return;}
    struct sockaddr_in a={0};int r=yn_socket_recv((int)A0,YPTR(A1),(size_t)A2,(int)A3,&a);
    if(r>=0 && from && A4 && A5)M32(A5)=so_addr_out(A4,M32(A5),&a);yret(c,r);
}
static void y_recv(Ctx *c){y_recv_impl(c,0);}
static void y_recvfrom(Ctx *c){y_recv_impl(c,1);}
static void y_send_impl(Ctx *c,int to) {
    if(!yfd(A0)){if(to)socket_SendTo(c);else socket_Send(c);return;}
    struct sockaddr_in a,*dst=NULL;
    if(to && A4 && A5){if(A5<8){so_fail(c,22);return;}if(!so_addr_in(A4,&a)){so_fail(c,97);return;}dst=&a;}
    yret(c,yn_socket_send((int)A0,YPTR(A1),(size_t)A2,(int)A3,dst));
}
static void y_send(Ctx *c){y_send_impl(c,0);}
static void y_sendto(Ctx *c){y_send_impl(c,1);}
static void y_fcntl(Ctx *c){if(yfd(A0))yret(c,yn_socket_fcntl((int)A0,(int)A1,(int)A2));else socket_Fcntl(c);}
static void y_getopt(Ctx *c){
    if(!yfd(A0)){socket_GetSockOpt(c);return;}
    if(!A4){so_fail(c,22);return;}uint32_t n=M32(A4);
    int r=yn_socket_option((int)A0,0,(int)A1,(int)A2,YPTR(A3),&n);if(!r)M32(A4)=n;yret(c,r);
}
static void y_setopt(Ctx *c){
    if(!yfd(A0)){socket_SetSockOpt(c);return;}
    uint32_t n=(uint32_t)A4;yret(c,yn_socket_option((int)A0,1,(int)A1,(int)A2,YPTR(A3),&n));
}
static void y_poll(Ctx *c) {
    uint64_t p=A0,n=A1;int timeout=(int)A2;
    if(!p || n>SO_MAX_FD || timeout<-1){if(n>SO_MAX_FD)so_fail(c,22);else socket_Poll(c);return;}
    int any=0;for(size_t i=0;i<n;i++)if(yfd((uint32_t)M32(p+i*8)))any=1;
    if(!any){socket_Poll(c);return;}
    struct {int fd;uint16_t events,revents;} native[SO_MAX_FD];unsigned index[SO_MAX_FD];
    ULONGLONG end=timeout>=0?GetTickCount64()+(unsigned)timeout:0;
    for(;;){
        uint64_t seq=yn_activity();unsigned count=0;int ready=0;
        for(size_t i=0;i<n;i++){
            int fd=(int)M32(p+i*8);M16(p+i*8+6)=0;if(fd<0)continue;
            if(yfd((uint32_t)fd)){int v=yn_socket_poll(fd,M16(p+i*8+4));M16(p+i*8+6)=(uint16_t)v;if(v)ready++;}
            else {index[count]=(unsigned)i;native[count].fd=fd;native[count].events=M16(p+i*8+4);native[count].revents=0;count++;}
        }
        if(count){A0=P(native);A1=count;A2=0;socket_Poll(c);int r=(int)A0;A0=p;A1=n;A2=(uint64_t)(int64_t)timeout;
            if(r<0){RETI(-1);return;}ready+=r;
            for(unsigned j=0;j<count;j++)M16(p+index[j]*8+6)=native[j].revents;
        }
        ULONGLONG now=GetTickCount64();if(ready || (timeout>=0 && now>=end)){RETI(ready);return;}
        DWORD ms=timeout<0?INFINITE:(DWORD)(end-now);if(count && ms>10)ms=10;
        yn_wait_activity(seq,ms);
    }
}
static void yl_init(Ctx *c){YLDN(yn_ldn_initialize());}
static void yl_final(Ctx *c){YLDN(yn_ldn_finalize());}
static void yl_state(Ctx *c){RET(yn_ldn_state());}
static void yl_event(Ctx *c){
    if(!A0){RET(YN_LDN_ERROR(96));return;}
    yn_lock();
    if(!ssbu_yn.ldn_state){yn_unlock();RET(YN_LDN_ERROR(32));return;}
    if(ssbu_yn.ldn_event!=A0)os_init_event(A0,0);
    uint32_t r=yn_ldn_event(A0);yn_unlock();RET(r);
}
static void yl_open_ap(Ctx *c){YLDN(yn_ldn_open(0,0));}
static void yl_close_ap(Ctx *c){YLDN(yn_ldn_open(0,1));}
static void yl_open_sta(Ctx *c){YLDN(yn_ldn_open(1,0));}
static void yl_close_sta(Ctx *c){YLDN(yn_ldn_open(1,1));}
static void yl_create(Ctx *c){YLDN(yn_ldn_create(YPTR(A0),YPTR(A1),YPTR(A2),NULL));}
static void yl_create_private(Ctx *c){YLDN(yn_ldn_create(YPTR(A0),YPTR(A1),YPTR(A3),YPTR(A2)));}
static void yl_connect(Ctx *c){YLDN(yn_ldn_connect(YPTR(A0),YPTR(A2),(int)A3));}
static void yl_connect_private(Ctx *c){
    if(!A0 || !A2 || !A3){RET(YN_LDN_ERROR(96));return;}
    uint8_t filter[0x60]={0},*found=malloc(64*YN_LDN_SIZE);if(!found){RET(YN_LDN_ERROR(31));return;}
    memcpy(filter,YPTR(A0),16);memcpy(filter+16,YPTR(A2+16),16);yn_put32(filter+0x5c,0x23);
    int count=0;uint32_t r=yn_ldn_scan(found,&count,64,filter);
    if(!r)r=count?yn_ldn_connect(found,YPTR(A3),(int)A4):YN_LDN_ERROR(97);
    free(found);RET(r);
}
static void yl_destroy(Ctx *c){YLDN(yn_ldn_disconnect(1));}
static void yl_disconnect(Ctx *c){YLDN(yn_ldn_disconnect(0));}
static void yl_info(Ctx *c){YLDN(yn_ldn_info(YPTR(A0)));}
static void yl_config(Ctx *c){YLDN(yn_ldn_config(YPTR(A0)));}
static void yl_ip(Ctx *c){YLDN(yn_ldn_ip(YPTR(A0),YPTR(A1)));}
static void yl_security(Ctx *c){YLDN(yn_ldn_security(YPTR(A0)));}
static void yl_advertise(Ctx *c){YLDN(yn_ldn_advertise(YPTR(A0),(size_t)A1));}
static void yl_scan(Ctx *c){YLDN(yn_ldn_scan(YPTR(A0),YPTR(A1),(int)A2,YPTR(A3)));}
static void yl_policy(Ctx *c){YLDN(yn_ldn_policy((unsigned)A0));}
static void yl_reject(Ctx *c){YLDN(yn_ldn_reject((uint32_t)A0));}
static void yl_reason(Ctx *c){RET(yn_ldn_reason());}
static void yl_controller(Ctx *c){RET(yn_ldn_state()?0:YN_LDN_ERROR(32));}
/* Registration table follows below. */

static void yn_register_sdk(void) {
    static const ImportDef defs[] = {
        {SOK("8FinalizeEv"), y_finalize},
        {SOK("6SocketEiii"), y_socket},
        {SOK("6SocketENS0_6FamilyENS0_4TypeENS0_8ProtocolE"), y_socket},
        {"nnsocketSocket", y_socket},
        {SOK("5CloseEi"), y_close},
        {"nnsocketClose", y_close},
        {SOK("4BindEiPK8sockaddrj"), y_bind},
        {SOK("4BindEiPKNS0_8SockAddrEj"), y_bind_typed},
        {"nnsocketBind", y_bind},
        {SOK("7ConnectEiPK8sockaddrj"), y_connect},
        {"nnsocketConnect", y_connect},
        {SOK("6ListenEii"), y_listen},
        {SOK("8ShutdownEii"), y_shutdown},
        {SOK("6AcceptEiP8sockaddrPj"), y_accept},
        {SOK("11GetSockNameEiP8sockaddrPj"), y_sockname},
        {"nnsocketGetSockName", y_sockname},
        {"nnsocketGetPeerName", y_peername},
        {SOK("4RecvEiPvmi"), y_recv},
        {"nnsocketRecv", y_recv},
        {SOK("8RecvFromEiPvmiP8sockaddrPj"), y_recvfrom},
        {SOK("8RecvFromEiPvmNS0_7MsgFlagEPNS0_8SockAddrEPj"), y_recvfrom},
        {SOK("4SendEiPKvmi"), y_send},
        {"nnsocketSend", y_send},
        {SOK("6SendToEiPKvmiPK8sockaddrj"), y_sendto},
        {SOK("6SendToEiPKvmNS0_7MsgFlagEPKNS0_8SockAddrEj"), y_sendto},
        {SOK("5FcntlEiiz"), y_fcntl},
        {"nnsocketFcntl", y_fcntl},
        {SOK("4PollEP6pollfdmi"), y_poll},
        {"nnsocketPoll", y_poll},
        {SOK("10GetSockOptEiiiPvPj"), y_getopt},
        {"nnsocketGetSockOpt", y_getopt},
        {SOK("10SetSockOptEiiiPKvj"), y_setopt},
        {SOK("10SetSockOptEiNS0_5LevelENS0_6OptionEPKvj"), y_setopt},
        {"nnsocketSetSockOpt", y_setopt},
        {LDNK("10InitializeEv"), yl_init},
        {LDNK("8GetStateEv"), yl_state},
        {LDNK("8FinalizeEv"), yl_final},
        {LDNK("22AttachStateChangeEventEPNS_2os15SystemEventTypeE"), yl_event},
        {LDNK("32SetWirelessControllerRestrictionENS0_29WirelessControllerRestrictionE"), yl_controller},
        {LDNK("11OpenStationEv"), yl_open_sta},
        {LDNK("12CloseStationEv"), yl_close_sta},
        {LDNK("15OpenAccessPointEv"), yl_open_ap},
        {LDNK("16CloseAccessPointEv"), yl_close_ap},
        {LDNK("10DisconnectEv"), yl_disconnect},
        {LDNK("14DestroyNetworkEv"), yl_destroy},
        {LDNK("14GetNetworkInfoEPNS0_11NetworkInfoE"), yl_info},
        {LDNK("16GetNetworkConfigEPNS0_13NetworkConfigE"), yl_config},
        {LDNK("14GetIpv4AddressEPNS0_11Ipv4AddressEPNS0_10SubnetMaskE"), yl_ip},
        {LDNK("20GetSecurityParameterEPNS0_17SecurityParameterE"), yl_security},
        {LDNK("16SetAdvertiseDataEPKvm"), yl_advertise},
        {LDNK("22SetStationAcceptPolicyENS0_12AcceptPolicyE"), yl_policy},
        {LDNK("6RejectENS0_11Ipv4AddressE"), yl_reject},
        {LDNK("4ScanEPNS0_11NetworkInfoEPiiRKNS0_10ScanFilterEi"), yl_scan},
        {LDNK("11ScanPrivateEPNS0_11NetworkInfoEPiiRKNS0_10ScanFilterEi"), yl_scan},
        {LDNK("13CreateNetworkERKNS0_13NetworkConfigERKNS0_14SecurityConfigERKNS0_10UserConfigE"), yl_create},
        {LDNK("20CreateNetworkPrivateERKNS0_13NetworkConfigERKNS0_14SecurityConfigERKNS0_17SecurityParameterERKNS0_10UserConfigEiPKNS0_12AddressEntryE"), yl_create_private},
        {LDNK("7ConnectERKNS0_11NetworkInfoERKNS0_14SecurityConfigERKNS0_10UserConfigEiNS0_13ConnectOptionE"), yl_connect},
        {LDNK("14ConnectPrivateERKNS0_13NetworkConfigERKNS0_14SecurityConfigERKNS0_17SecurityParameterERKNS0_10UserConfigEiNS0_13ConnectOptionE"), yl_connect_private},
        {LDNK("19GetDisconnectReasonEv"), yl_reason},
    };
    register_imports(defs, (int)(sizeof defs/sizeof defs[0]));
}

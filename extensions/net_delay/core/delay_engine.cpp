#include "delay_api.h"
#include <mutex>
#include <new>

struct NdEngine {
    std::mutex lock;
    NdSnapshot state;
    NdEngine() : state{} {
        state.size=sizeof state;state.abi=ND_ABI;state.selected=ND_AUTO;
        state.backend=ND_BACKEND_HOOK;state.decision=ND_UNAVAILABLE;
        state.before=state.automatic=state.effective=-1;
    }
};
extern "C" NdEngine *nd_create(void) { return new(std::nothrow) NdEngine; }
extern "C" void nd_destroy(NdEngine *e) { delete e; }
extern "C" int nd_select(NdEngine *e,int frames) {
    if(!e || frames<ND_AUTO || frames>ND_MAX_FRAMES)return 0;
    std::lock_guard<std::mutex> guard(e->lock);
    if(e->state.selected!=frames){e->state.selected=frames;e->state.selection_revision++;}
    return 1;
}
extern "C" int nd_snapshot(NdEngine *e,NdSnapshot *out) {
    if(!e || !out || out->size!=sizeof *out || out->abi!=ND_ABI)return 0;
    std::lock_guard<std::mutex> guard(e->lock);*out=e->state;return 1;
}
extern "C" void nd_backend(NdEngine *e,NdBackend backend) {
    if(!e)return;std::lock_guard<std::mutex> guard(e->lock);e->state.backend=backend;
}
extern "C" void nd_session(NdEngine *e,const NdSession *s) {
    if(!e || !s)return;std::lock_guard<std::mutex> guard(e->lock);e->state.live=*s;
}
extern "C" int nd_session_active(const NdSession *s) {
    return s && s->available && s->connected && (s->role==1 || s->role==2) && s->peers>=2 && s->peers<=8;
}
extern "C" int nd_evaluate(NdEngine *e,const NdSession *a,const NdSession *b,
                            const NdSample *sample,NdWrite write,void *user,NdSnapshot *result) {
    if(!e || !a || !b || !sample)return 0;
    std::lock_guard<std::mutex> guard(e->lock);
    NdSnapshot &s=e->state;
    s.calls++;s.live=s.last_session=*b;s.before=sample->before;
    s.automatic=s.effective=sample->readable?sample->automatic:-1;
    s.mode=sample->mode;s.object_state=sample->object_state;
    if(s.backend!=ND_BACKEND_READY)s.decision=ND_UNAVAILABLE;
    else if(!sample->readable)s.decision=ND_MEMORY;
    else if(s.selected==ND_AUTO)s.decision=ND_AUTO_RESULT;
    else if(!nd_session_active(a) || !nd_session_active(b))s.decision=ND_NO_SESSION;
    else if(a->connection!=b->connection || a->session!=b->session || a->role!=b->role)s.decision=ND_SESSION_CHANGED;
    else if(sample->automatic<0 || sample->automatic>ND_MAX_FRAMES)s.decision=ND_BAD_VALUE;
    else if(!write || !write(user,s.selected))s.decision=ND_MEMORY;
    else {s.decision=ND_APPLIED;s.effective=s.selected;s.writes++;s.applied_revision=s.selection_revision;}
    if(result)*result=s;
    return 1;
}

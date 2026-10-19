/* Pure backend tests: standard C++, no Windows, host, game or transport. */
#include "delay_api.h"
#include <atomic>
#include <stdio.h>
#include <stdlib.h>
#include <thread>
#include <vector>
#define CHECK(x) do{if(!(x)){fprintf(stderr,"FAIL line %d: %s\n",__LINE__,#x);exit(1);}}while(0)
static int writes,last;
static int accept(void *,int frames){writes++;last=frames;return 1;}
static int refuse(void *,int){return 0;}
static NdSnapshot read(NdEngine *e){NdSnapshot s={};s.size=sizeof s;s.abi=ND_ABI;CHECK(nd_snapshot(e,&s));return s;}
int main(){
    NdEngine *e=nd_create(),*independent=nd_create();CHECK(e && independent);
    CHECK(!nd_select(NULL,2) && !nd_select(e,-2) && !nd_select(e,26));
    CHECK(!nd_snapshot(e,NULL));NdSnapshot invalid={};CHECK(!nd_snapshot(e,&invalid));
    CHECK(read(e).selected==ND_AUTO && read(e).calls==0);
    NdSession active={1,1,1,2,7,8},after=active;
    NdSample sample={1,4,4,0,0};NdSnapshot result={};
    CHECK(nd_evaluate(e,&active,&after,&sample,accept,NULL,&result));CHECK(result.decision==ND_UNAVAILABLE && !writes);
    nd_backend(e,ND_BACKEND_READY);
    unsigned cases=1;
    for(int value=-1;value<=25;value++)for(unsigned role=0;role<=3;role++)for(unsigned peers=0;peers<=9;peers++){
        CHECK(nd_select(e,value));NdSnapshot previous=read(e);CHECK(nd_select(e,value));CHECK(read(e).selection_revision==previous.selection_revision);
        NdSession a=active,b=active;a.role=b.role=role;a.peers=b.peers=peers;
        int old=writes;CHECK(nd_evaluate(e,&a,&b,&sample,accept,NULL,&result));cases++;
        bool applies=value>=0 && (role==1 || role==2) && peers>=2 && peers<=8;
        CHECK((result.decision==ND_APPLIED)==applies);
        CHECK(writes==old+(applies?1:0));CHECK(result.effective==(applies?value:4));
        if(applies)CHECK(last==value && result.applied_revision==result.selection_revision);
    }
    CHECK(read(independent).selected==ND_AUTO && !read(independent).calls);
    CHECK(nd_select(e,3));
    for(unsigned kind=0;kind<3;kind++){
        after=active;if(kind==0)after.connection++;if(kind==1)after.session++;if(kind==2)after.role=2;
        nd_evaluate(e,&active,&after,&sample,accept,NULL,&result);CHECK(result.decision==ND_SESSION_CHANGED);cases++;
    }
    after=active;nd_evaluate(e,&active,&after,&sample,refuse,NULL,&result);CHECK(result.decision==ND_MEMORY && result.effective==4);cases++;
    sample.automatic=26;nd_evaluate(e,&active,&after,&sample,accept,NULL,&result);CHECK(result.decision==ND_BAD_VALUE);cases++;
    sample.readable=0;nd_evaluate(e,&active,&after,&sample,accept,NULL,&result);CHECK(result.decision==ND_MEMORY);cases++;
    sample.readable=1;sample.automatic=3;nd_select(e,ND_AUTO);int old=writes;
    nd_evaluate(e,&active,&after,&sample,accept,NULL,&result);CHECK(result.effective==3 && result.decision==ND_AUTO_RESULT && writes==old);cases++;
    std::atomic<int> failed(0);std::vector<std::thread> threads;
    for(int thread=0;thread<4;thread++)threads.emplace_back([&,thread](){
        for(int n=0;n<4000;n++){
            if(!nd_select(e,(n+thread)%27-1))failed++;
            NdSnapshot s={};s.size=sizeof s;s.abi=ND_ABI;
            if(!nd_snapshot(e,&s) || s.selected<-1 || s.selected>25)failed++;
        }
    });
    for(auto &t:threads)t.join();CHECK(!failed);
    CHECK(read(e).calls==cases);
    nd_destroy(e);nd_destroy(independent);
    printf("PASS %u backend decision cases; independent instances; 16000 concurrent selections/snapshots\n",cases);
}

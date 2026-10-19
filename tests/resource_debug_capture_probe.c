/* Exercise only the launcher's exception monitor; no game/runtime or GPU. */
#include <windows.h>
#include <stdio.h>
#include <string.h>
static void *page;
static LONG CALLBACK repair_write(EXCEPTION_POINTERS *ep) {
    if (ep->ExceptionRecord->ExceptionCode != EXCEPTION_ACCESS_VIOLATION) return EXCEPTION_CONTINUE_SEARCH;
    DWORD old;
    if (!VirtualProtect(page, 4096, PAGE_READWRITE, &old)) return EXCEPTION_CONTINUE_SEARCH;
    return EXCEPTION_CONTINUE_EXECUTION;
}
int main(int argc, char **argv) {
    SetErrorMode(SEM_NOGPFAULTERRORBOX | SEM_FAILCRITICALERRORS);
    if (argc > 1 && !strcmp(argv[1], "heap-failfast")) {
        EXCEPTION_RECORD record = {0};
        record.ExceptionCode = 0xc0000374u;
        record.ExceptionFlags = EXCEPTION_NONCONTINUABLE;
        record.ExceptionAddress = (void *)&main;
        RaiseFailFastException(&record, NULL, 0);
        return 9;
    }
    page = VirtualAlloc(NULL, 4096, MEM_RESERVE | MEM_COMMIT, PAGE_READONLY);
    if (!page) return 10;
    void *handler = AddVectoredExceptionHandler(1, repair_write);
    *(volatile int *)page = 42;
    if (*(int *)page != 42) return 11;
    RemoveVectoredExceptionHandler(handler);
    VirtualFree(page, 0, MEM_RELEASE);
    puts("handled access violation preserved");
    return 0;
}

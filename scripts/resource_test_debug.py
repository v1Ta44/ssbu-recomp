"""Capture fatal Windows child exceptions for manual resource tests.

Only the explicitly launched process is debugged. Normal first-chance access
violations (including write-watch) are passed to the child's handlers. Heap
corruption/fail-fast is captured on first chance, before Windows tears it down.
No registry, global debugger, or game files are changed.
"""
import ctypes as C
from ctypes import wintypes as W
from datetime import datetime
import json
import os
from pathlib import Path
import subprocess

DWORD = C.c_uint32
ULONG64 = C.c_uint64
HANDLE = C.c_void_p


class ExceptionRecord(C.Structure):
    _fields_ = [("code", DWORD), ("flags", DWORD), ("nested", HANDLE),
                ("address", HANDLE), ("count", DWORD), ("alignment", DWORD),
                ("parameters", ULONG64 * 15)]


class ExceptionDebugInfo(C.Structure):
    _fields_ = [("record", ExceptionRecord), ("first_chance", DWORD)]


class DebugPayload(C.Union):
    _fields_ = [("exception", ExceptionDebugInfo), ("raw", C.c_ubyte * 160),
                ("handles", HANDLE * 3), ("exit_code", DWORD)]


class DebugEvent(C.Structure):
    _fields_ = [("kind", DWORD), ("pid", DWORD), ("tid", DWORD), ("data", DebugPayload)]


class ExceptionPointers(C.Structure):
    _fields_ = [("record", C.POINTER(ExceptionRecord)), ("context", HANDLE)]


class DumpExceptionInfo(C.Structure):
    _pack_ = 4  # dbghelp.h minidump structures use 4-byte packing, including x64.
    _fields_ = [("tid", DWORD), ("pointers", C.POINTER(ExceptionPointers)),
                ("client_pointers", W.BOOL)]


class WindowsDebugCapture:
    def __init__(self, output):
        if os.name != "nt" or C.sizeof(HANDLE) != 8:
            raise RuntimeError("Manual exception capture requires 64-bit Windows Python")
        assert C.sizeof(ExceptionRecord) == 152 and C.sizeof(DebugEvent) == 176
        self.output = Path(output)
        self.output.mkdir(parents=True, exist_ok=True)
        self.kernel = C.WinDLL("kernel32", use_last_error=True)
        self.dbghelp = C.WinDLL("dbghelp", use_last_error=True)
        self.kernel.WaitForDebugEvent.argtypes = [C.POINTER(DebugEvent), DWORD]
        self.kernel.WaitForDebugEvent.restype = W.BOOL
        self.kernel.ContinueDebugEvent.argtypes = [DWORD, DWORD, DWORD]
        self.kernel.ContinueDebugEvent.restype = W.BOOL
        self.kernel.OpenThread.argtypes = [DWORD, W.BOOL, DWORD]
        self.kernel.OpenThread.restype = HANDLE
        self.kernel.CloseHandle.argtypes = [HANDLE]
        self.kernel.CloseHandle.restype = W.BOOL
        self.kernel.GetThreadContext.argtypes = [HANDLE, HANDLE]
        self.kernel.GetThreadContext.restype = W.BOOL
        self.dbghelp.MiniDumpWriteDump.argtypes = [HANDLE, DWORD, HANDLE, DWORD,
            C.POINTER(DumpExceptionInfo), HANDLE, HANDLE]
        self.dbghelp.MiniDumpWriteDump.restype = W.BOOL
        self.exceptions = []
        self.captured = set()

    def _dump(self, process, event):
        import msvcrt
        info = event.data.exception
        record = info.record
        suffix = f"{record.code:08x}-thread-{event.tid}-{'first' if info.first_chance else 'last'}"
        report = {"time": datetime.now().astimezone().isoformat(), "pid": event.pid,
                  "tid": event.tid, "code": f"0x{record.code:08x}",
                  "address": hex(record.address or 0), "first_chance": bool(info.first_chance),
                  "parameters": list(record.parameters[:min(record.count, 15)])}
        thread = self.kernel.OpenThread(0x0008 | 0x0040, False, event.tid)
        context_data = C.create_string_buffer(1232 + 16)
        context = (C.addressof(context_data) + 15) & ~15
        # CONTEXT_AMD64 | CONTEXT_FULL, at the platform-defined flags offset.
        C.c_uint32.from_address(context + 48).value = 0x0010000B
        pointers = None
        exception = None
        if thread:
            try:
                if self.kernel.GetThreadContext(thread, context):
                    pointers = ExceptionPointers(C.pointer(record), context)
                    exception = DumpExceptionInfo(event.tid, C.pointer(pointers), False)
                else:
                    report["context_error"] = C.get_last_error()
            finally:
                self.kernel.CloseHandle(thread)
        else:
            report["thread_error"] = C.get_last_error()
        target = self.output / ("fatal-" + suffix + ".dmp")
        # Thread stacks, their indirect references, and page metadata; no multi-GB
        # full-memory dump by default. The exact executable/map are saved separately.
        flags = 0x0004 | 0x0020 | 0x0040 | 0x0800 | 0x1000 | 0x20000
        with target.open("wb") as stream:
            ok = self.dbghelp.MiniDumpWriteDump(int(process._handle), event.pid,
                msvcrt.get_osfhandle(stream.fileno()), flags,
                C.byref(exception) if exception is not None else None, None, None)
            report["dump_ok"] = bool(ok)
            if not ok:
                report["dump_error"] = C.get_last_error()
        report["dump"] = target.name
        report["dump_bytes"] = target.stat().st_size
        self.exceptions.append(report)
        (self.output / "fatal-exceptions.json").write_text(
            json.dumps(self.exceptions, indent=2), encoding="utf-8")
        print("Captured fatal exception", report["code"], "in", target, flush=True)

    def run(self, command, *, cwd, env, log, on_start=None):
        # Popen and the event loop must run on this same thread, which owns the
        # debug relationship. Child processes are not debugged.
        process = subprocess.Popen(command, cwd=cwd, env=env, stdout=log,
            stderr=subprocess.STDOUT, creationflags=0x00000002)
        if on_start:
            on_start(process.pid)
        initial_breakpoint = True
        event = DebugEvent()
        try:
            while True:
                if not self.kernel.WaitForDebugEvent(C.byref(event), 1000):
                    error = C.get_last_error()
                    if error == 121:  # timeout
                        continue
                    raise C.WinError(error)
                status = 0x00010002  # DBG_CONTINUE (non-exception events)
                exited = event.kind == 5
                if event.kind == 1:
                    info = event.data.exception
                    code = info.record.code
                    first = bool(info.first_chance)
                    fatal = code in (0xC0000374, 0xC0000409, 0xC0000602) or not first
                    key = (code, info.record.address)
                    if fatal and key not in self.captured:
                        self.captured.add(key)
                        # Always release the exception even if writing diagnostics fails.
                        try:
                            self._dump(process, event)
                        except Exception as error:
                            (self.output / "capture-error.txt").write_text(repr(error), encoding="utf-8")
                            print("Fatal exception capture failed:", error, flush=True)
                    if initial_breakpoint and code == 0x80000003 and first:
                        initial_breakpoint = False
                    else:
                        status = 0x80010001  # DBG_EXCEPTION_NOT_HANDLED; preserve child behavior
                elif event.kind == 3:  # CREATE_PROCESS_DEBUG_EVENT: hFile, hProcess, hThread
                    # Debugger-owned process/thread event handles are closed at EXIT_PROCESS
                    # by Windows; hFile belongs to this event and must be closed here.
                    if event.data.handles[0]:
                        self.kernel.CloseHandle(event.data.handles[0])
                elif event.kind == 6:  # LOAD_DLL_DEBUG_EVENT: hFile
                    if event.data.handles[0]:
                        self.kernel.CloseHandle(event.data.handles[0])
                if not self.kernel.ContinueDebugEvent(event.pid, event.tid, status):
                    raise C.WinError(C.get_last_error())
                if exited:
                    return process.wait()
        except BaseException:
            # A failed diagnostic monitor must not leave an owned game process
            # frozen on a pending debug event when its console closes.
            if process.poll() is None:
                process.terminate()
                self.kernel.ContinueDebugEvent(event.pid, event.tid, 0x80010001)
                process.wait(timeout=10)
            raise

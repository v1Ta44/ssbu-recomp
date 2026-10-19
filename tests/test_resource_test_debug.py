"""One focused check of heap fail-fast capture and passing normal faults to the child.
Does not start the game, a window, or a GPU. Uses the repository's configured compiler.
"""
import json
import os
from pathlib import Path
import struct
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from resource_test_debug import WindowsDebugCapture


def main():
    state = json.loads((ROOT / "build/state.json").read_text(encoding="utf-8"))
    out = ROOT / "build/mod-adapt/gamebanana/bug03-heap/capture-check"
    out.mkdir(parents=True, exist_ok=True)
    exe = out / "capture_probe.exe"
    subprocess.run([state["tools"]["clang"], "-O1", "-g", str(ROOT / "tests/resource_debug_capture_probe.c"), "-o", str(exe)], check=True)
    normal = WindowsDebugCapture(out / "handled")
    with (out / "handled.log").open("wb") as log:
        code = normal.run([str(exe)], cwd=out, env=os.environ, log=log)
    assert code == 0 and not normal.exceptions, (code, normal.exceptions)
    assert b"handled access violation preserved" in (out / "handled.log").read_bytes()
    print("PASS normal first-chance access violation reaches the child's handler", flush=True)
    fatal = WindowsDebugCapture(out / "fatal")
    with (out / "fatal.log").open("wb") as log:
        code = fatal.run([str(exe), "heap-failfast"], cwd=out, env=os.environ, log=log)
    assert code == 0xc0000374, hex(code)
    assert len(fatal.exceptions) == 1, fatal.exceptions
    report = fatal.exceptions[0]
    assert report["code"] == "0xc0000374" and report["dump_ok"], report
    data = (fatal.output / report["dump"]).read_bytes()
    assert data[:4] == b"MDMP"
    count, table = struct.unpack_from("<II", data, 8)
    streams = {kind: (rva, size) for kind, size, rva in [struct.unpack_from("<III", data, table + 12 * i) for i in range(count)]}
    ex = streams[6][0]
    assert struct.unpack_from("<I", data, ex)[0] == report["tid"]
    assert struct.unpack_from("<I", data, ex + 8)[0] == 0xc0000374
    context_bytes, context_offset = struct.unpack_from("<II", data, ex + 160)
    assert context_bytes >= 1232 and context_offset, (context_bytes, context_offset)
    assert struct.unpack_from("<Q", data, context_offset + 248)[0], "exception context must contain RIP"
    print("PASS heap fail-fast produces a fresh matching exception dump and x64 thread context", flush=True)
    return 0

if __name__ == "__main__":
    sys.exit(main())

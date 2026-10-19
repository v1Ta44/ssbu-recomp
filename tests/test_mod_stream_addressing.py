"""Private-archive regression for native 32-bit music stream addressing.

Uses installed test-08 packages unchanged. Does not launch the game or play audio.
"""
import argparse
import json
from pathlib import Path
import struct

from test_mod_backend import Backend, VirtualFile, ax

ROOT = Path(__file__).resolve().parents[1]


def stream_records(arc):
    quick, count, indices, _ = struct.unpack_from("<4I", arc.t, 0x100)
    entries = 0x110 + quick * 12 + count * 8
    result = {}
    for i in range(count):
        packed, flags = struct.unpack_from("<QI", arc.t, entries + i * 12)
        index = struct.unpack_from("<I", arc.t, entries + count * 12 + (packed >> 40) * 4)[0]
        size, offset = struct.unpack_from("<QQ", arc.t, entries + count * 12 + indices * 4 + index * 16)
        result[packed & 0xffffffffff] = (offset, size, flags)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", default=str(ROOT / "build/mod-adapt/release-host/ssbu_mod_backend.dll"))
    args = parser.parse_args()
    source = Path(json.loads((ROOT / "build/state.json").read_text(encoding="utf-8"))["dump"]["romfs"]) / "data.arc"
    stamp = source.stat().st_size, source.stat().st_mtime_ns
    original = ax.Arc(str(source))
    originals = stream_records(original)
    backend = Backend(args.backend)
    backend.request({"op": "configure", "config": {
        "root": str(ROOT / "build/mod-adapt/gamebanana/packs"),
        "profile": str(ROOT / "build/mod-adapt/gamebanana/runs/streams/mods.json"),
        "cache": str(ROOT / "build/mod-adapt/gamebanana/cache"),
    }})
    backend.request({"op": "init"})
    plan = backend.request({"op": "plan"})["plan"]
    assert not plan["diagnostics"] and len(plan["entries"]) == 3
    backend.request({"op": "open", "path": str(source)})
    ax.open = lambda *a, **kw: VirtualFile(backend)
    virtual = ax.Arc(str(source))
    streams = stream_records(virtual)
    replaced = set()
    for entry in sorted(plan["entries"], key=lambda e: e["path"].endswith(".webm")):
        offset, size, _ = streams[entry["hash"]]
        disk = Path(entry["base"]["disk"])
        with disk.open("rb") as f:
            expected = f.read()
        assert size == len(expected)
        # Native music's stream base is read as u32 at 0x710373c958.
        # Reproduce that exact narrowing, including a read at a later position.
        native_base = offset & 0xffffffff if entry["path"].endswith(".nus3audio") else offset
        assert backend.read(native_base, 128) == expected[:128], (
            "native music base truncates a tail-appended stream", entry["path"], hex(offset), hex(native_base))
        assert offset + size <= 0xffffffff
        assert backend.read(offset, size) == expected
        for local in (size // 2, max(0, size - 257)):
            assert backend.read(native_base + local, min(257, size - local)) == expected[local:local + 257]
        replaced.add(entry["hash"])
    shift = virtual.file_off - original.file_off
    assert shift > 0
    preserved = 0
    for hash_value, (offset, size, flags) in originals.items():
        if hash_value in replaced:
            continue
        new_offset, new_size, new_flags = streams[hash_value]
        assert (new_size, new_flags) == (size, flags), hash_value
        assert new_offset == offset + shift and new_offset + size <= 0xffffffff, hash_value
        preserved += 1
    # Verify real bytes on both sides of the relocation, including original
    # movie/audio streams and ordinary/region-specific file sections.
    untouched = [(h, r) for h, r in originals.items() if h not in replaced]
    for _, (offset, size, _) in untouched[::max(1, len(untouched) // 5)] + untouched[-1:]:
        for local in (0, max(0, size - 257)):
            original.f.seek(offset + local)
            expected = original.f.read(min(257, size - local))
            assert backend.read(offset + shift + local, len(expected)) == expected
    for path in ("fighter/mario/model/body/c00/model.numdlb",
                 "fighter/palutena/model/body/c01/model.numdlb",
                 "sound/bank/fighter_voice/vc_palutena_c01.nus3audio"):
        assert virtual.get(path, 13) == original.get(path, 13), path
    assert (virtual.shared_off - original.shared_off) == shift
    assert stamp == (source.stat().st_size, source.stat().st_mtime_ns)
    print("PASS two BGM streams at native 32-bit base; movie/full bytes and seek/partial reads")
    print(f"PASS {preserved} original stream mappings/sizes/flags preserved; original payload sections relocated by {shift} bytes")
    print("PASS original stream tail/model/region voice bytes; original archive unchanged; no game or audio playback")


if __name__ == "__main__":
    main()

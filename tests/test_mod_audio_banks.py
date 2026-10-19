"""Private integration check for patch3audio bank identity and one-slot isolation.

Uses a downloaded patch without redistributing it. Never starts the game; the
Mario marker is intentionally invalid and lives only in isolated test fixtures.
"""
import argparse
import json
import os
from pathlib import Path
import struct

from test_mod_backend import Backend, VirtualFile, ax

ROOT = Path(__file__).resolve().parents[1]
NARRATION = "sound/bank/narration/vc_narration_common"
MARIO = "sound/bank/fighter_voice/vc_mario_c00"


def tracks(data):
    # Independent section reader: compare the actual names, TNIDs and sample
    # bytes rather than asking the Rust writer to validate its own output.
    assert data[:4] == b"NUS3" and data[8:12] == b"AUDI"
    assert struct.unpack_from("<I", data, 4)[0] + 8 == len(data)
    sections = {}
    position = 12
    while position < len(data):
        tag, size = struct.unpack_from("<4sI", data, position)
        sections[tag] = (position + 8, size)
        position += size + 8
    assert position == len(data)
    count = struct.unpack_from("<I", data, sections[b"INDX"][0])[0]
    result = {}
    for i in range(count):
        track_id = struct.unpack_from("<I", data, sections[b"TNID"][0] + 4 * i)[0]
        name_offset = struct.unpack_from("<I", data, sections[b"NMOF"][0] + 4 * i)[0]
        offset, size = struct.unpack_from("<2I", data, sections[b"ADOF"][0] + 8 * i)
        assert offset + size <= len(data)
        name = data[name_offset:data.index(b"\0", name_offset)].decode("utf-8")
        assert name not in result
        result[name] = (track_id, data[offset:offset + size])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", default=str(ROOT / "build/mod-adapt/release-host/ssbu_mod_backend.dll"))
    parser.add_argument("--patch", default=str(ROOT / "build/mod-adapt/gamebanana/packs/game-announcer" / (NARRATION + ".patch3audio")))
    args = parser.parse_args()
    state = json.loads((ROOT / "build/state.json").read_text(encoding="utf-8"))
    source = Path(state["dump"]["romfs"]) / "data.arc"
    stamp = (source.stat().st_size, source.stat().st_mtime_ns)
    original = ax.Arc(str(source))
    patch = Path(args.patch).read_bytes()
    fixture = ROOT / "build/mod-adapt/audio-bank-fixtures" / str(os.getpid())
    package = fixture / "packs/audio-scope"
    for name, data in [(NARRATION + ".patch3audio", patch), (MARIO + ".nus3audio", b"ISOLATED-VOICE-MARKER")]:
        path = package / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    backend = Backend(args.backend)
    backend.request({"op": "configure", "config": {"root": str(fixture / "packs"),
                    "cache": str(fixture / "cache"), "region": "zh_cn"}})
    backend.request({"op": "open", "path": str(source)})
    ax.open = lambda *args, **kwargs: VirtualFile(backend)
    virtual = ax.Arc(str(source))
    bank = original.get(NARRATION + ".nus3bank", 13)
    actual_bank = virtual.get(NARRATION + ".nus3bank", 13)
    assert actual_bank == bank, ("standalone narration bank changed", struct.unpack_from("<I", bank, bank.index(b"GRP ", 0x30) - 4)[0], struct.unpack_from("<I", actual_bank, actual_bank.index(b"GRP ", 0x30) - 4)[0])
    print("PASS standalone narration bank identity and all bank bytes preserved")
    old = tracks(original.get(NARRATION + ".nus3audio", 13))
    merged = tracks(virtual.get(NARRATION + ".nus3audio", 13))
    replacement = tracks(patch)
    assert set(merged) == set(old) | set(replacement)
    for name, (track_id, data) in old.items():
        assert merged[name][0] == track_id, name
        assert merged[name][1] == (replacement[name][1] if name in replacement else data), name
    assert set(replacement) == {"vc_narration_gameset"}
    print("PASS", len(old), "original track IDs retained; only GAME! sample replaced")
    bank = original.get(MARIO + ".nus3bank")
    actual_bank = virtual.get(MARIO + ".nus3bank")
    at = bank.index(b"GRP ", 0x30) - 4
    assert struct.unpack_from("<I", bank, at)[0] != struct.unpack_from("<I", actual_bank, at)[0]
    restored = bytearray(actual_bank)
    restored[at:at + 4] = bank[at:at + 4]
    assert restored == bank
    assert virtual.get(MARIO + ".nus3audio") == b"ISOLATED-VOICE-MARKER"
    for extension in (".nus3bank", ".nus3audio"):
        other = MARIO.replace("c00", "c01") + extension
        assert virtual.get(other) == original.get(other), other
    print("PASS shared Mario c00 bank still isolated; c01 bank and voice unchanged")
    assert stamp == (source.stat().st_size, source.stat().st_mtime_ns)
    print("PASS original game archive unchanged; no game/GPU session started")


if __name__ == "__main__":
    main()

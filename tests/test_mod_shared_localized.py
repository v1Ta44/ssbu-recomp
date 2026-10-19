"""Private regression: unshare a localized voice stored outside its load group.

Invalid marker data is isolated under build fixtures; never launch it in game.
"""
import argparse
import json
import os
from pathlib import Path
import struct

from test_mod_backend import Backend, VirtualFile, ax
from test_mod_localized_groups import check_group, info

ROOT = Path(__file__).resolve().parents[1]
STEM = "sound/bank/fighter_voice/vc_palutena_c00"


def stored(arc, row, variant):
    owner, did, _ = struct.unpack_from("<3I", arc.t, arc.o_i2d + (row[2] + variant) * 12)
    position, size, decompressed, flags = struct.unpack_from("<4I", arc.t, arc.o_data + did * 16)
    base = struct.unpack_from("<Q", arc.t, arc.o_folder_off + owner * 28)[0]
    arc.f.seek(arc.file_off + base + position * 4)
    return size, decompressed, flags, arc.f.read(size)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", default=str(ROOT / "build/mod-adapt/release-host/ssbu_mod_backend.dll"))
    args = parser.parse_args()
    state = json.loads((ROOT / "build/state.json").read_text(encoding="utf-8"))
    source = Path(state["dump"]["romfs"]) / "data.arc"
    stamp = source.stat().st_size, source.stat().st_mtime_ns
    original = ax.Arc(str(source))
    directory_hash = ax.hash40("fighter/palutena/c00")
    directory = next(i for i in range(original.n_folder) if struct.unpack_from("<Q", original.t, original.o_dirinfo + i * 52)[0] & 0xffffffffff == directory_hash)
    base, _, _, _, _, start, count, _, _, _ = struct.unpack_from("<3Q7I", original.t, original.o_dirinfo + directory * 52)
    old_group = (base >> 40) + 13
    rows = [struct.unpack_from("<4I", original.t, original.o_info + ii * 16) for ii in range(start, start + count)]
    paths = [STEM + extension for extension in (".nus3audio", ".nus3bank")]
    indices = {original.path_index_from_hash(ax.hash40(path)) for path in paths}
    for row in rows:
        if row[0] in indices:
            assert row[3] & 0x8000
            owner = struct.unpack_from("<I", original.t, original.o_i2d + (row[2] + 13) * 12)[0]
            assert owner != old_group, "fixture must borrow its voice from an external group"
    fixture = ROOT / "build/mod-adapt/shared-localized-fixtures" / str(os.getpid())
    path = fixture / "packs/test" / paths[0]
    path.parent.mkdir(parents=True, exist_ok=True)
    marker = b"PRIVATE-LOCALIZED-VOICE-MARKER" * 19
    path.write_bytes(marker)
    backend = Backend(args.backend)
    backend.request({"op": "configure", "config": {"root": str(fixture / "packs"), "cache": str(fixture / "cache"), "region": "zh_cn"}})
    backend.request({"op": "open", "path": str(source)})
    ax.open = lambda *args, **kwargs: VirtualFile(backend)
    virtual = ax.Arc(str(source))
    new_group = (struct.unpack_from("<Q", virtual.t, virtual.o_dirinfo + directory * 52)[0] >> 40) + 13
    for path in paths:
        pi = virtual.path_index_from_hash(ax.hash40(path))
        _, row = info(virtual, pi)
        assert row[3] & 0x8000 and not row[3] & 0x300000
        owner, _, bits = struct.unpack_from("<3I", virtual.t, virtual.o_i2d + (row[2] + 13) * 12)
        assert owner == new_group and bits & 0x01000000, "new slot must own its data rather than redirect to vanilla"
        original_row = next(r for r in rows if r[0] == pi)
        for variant in range(original.reg1 + 1):
            if variant != 13:
                assert stored(virtual, row, variant) == stored(original, original_row, variant), (path, variant)
    assert virtual.get(paths[0], 13) == marker
    old_bank = original.get(paths[1], 13)
    new_bank = virtual.get(paths[1], 13)
    at = old_bank.index(b"GRP ", 0x30) - 4
    assert old_bank[at:at + 4] != new_bank[at:at + 4]
    restored = bytearray(new_bank); restored[at:at + 4] = old_bank[at:at + 4]
    assert restored == old_bank
    check_group(virtual, new_group)
    # Verify the decoder's owned-file cursor, excluding untouched shared links.
    cursor = 0
    new_start, new_count = struct.unpack_from("<2I", virtual.t, virtual.o_dirinfo + directory * 52 + 32)
    for ii in range(new_start, new_start + new_count):
        row = struct.unpack_from("<4I", virtual.t, virtual.o_info + ii * 16)
        if not row[3] & 0x8000: continue
        owner, did, _ = struct.unpack_from("<3I", virtual.t, virtual.o_i2d + (row[2] + 13) * 12)
        if owner != new_group: continue
        position, size = struct.unpack_from("<2I", virtual.t, virtual.o_data + did * 16)
        assert position * 4 == cursor, "borrowed entries must not consume bytes from the next language"
        cursor += (size + 15) & ~15
    assert cursor == struct.unpack_from("<I", virtual.t, virtual.o_folder_off + new_group * 28 + 12)[0]
    for slot in ("c01", "c07"):
        for extension in (".nus3audio", ".nus3bank"):
            path = STEM.replace("c00", slot) + extension
            assert virtual.get(path, 13) == original.get(path, 13), path
    print("PASS external shared voice/bank become owned localized slots; native stream order and all other languages preserved")
    print("PASS companion bank isolated; Palutena c01/c07 voice and bank unchanged")
    assert stamp == (source.stat().st_size, source.stat().st_mtime_ns)
    print("PASS original archive unchanged; markers never installed for game testing")


if __name__ == "__main__":
    main()

"""Private-archive regression for costume EFF availability after a color change.

Never launches the game. The invalid EFF marker is confined to an isolated fixture.
"""
import argparse
import json
from pathlib import Path
import struct

from test_mod_backend import Backend, VirtualFile, ax, check_bulk_layout

ROOT = Path(__file__).resolve().parents[1]
EFFECT = "effect/fighter/palutena/ef_palutena_c00.eff"
BASE_EFFECT = "effect/fighter/palutena/ef_palutena.eff"


def directory_rows(arc):
    return [struct.unpack_from("<3Q7I", arc.t, arc.o_dirinfo + di * 52)
            for di in range(arc.n_folder)]


def dependency_files(arc, rows, di):
    seen, files, pending = set(), set(), [di]
    while pending:
        current = pending.pop()
        if current in seen:
            continue
        seen.add(current)
        row = rows[current]
        for ii in range(row[5], row[5] + row[6]):
            files.add(struct.unpack_from("<I", arc.t, arc.o_info + ii * 16)[0])
        for child in range(row[7], row[7] + row[8]):
            packed = struct.unpack_from("<Q", arc.t, arc.o_child + child * 8)[0]
            target = packed >> 40
            assert target < len(rows), "dependency must name an existing directory"
            assert packed & 0xffffffffff == rows[target][0] & 0xffffffffff
            pending.append(target)
    return files


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", default=str(ROOT / "build/mod-adapt/release-host/ssbu_mod_backend.dll"))
    args = parser.parse_args()
    state = json.loads((ROOT / "build/state.json").read_text(encoding="utf-8"))
    source = Path(state["dump"]["romfs"]) / "data.arc"
    stamp = source.stat().st_size, source.stat().st_mtime_ns
    original = ax.Arc(str(source))
    fixture = ROOT / "build/mod-adapt/effect-reload-fixtures"
    package = fixture / "packs/test"
    path = package / EFFECT
    path.parent.mkdir(parents=True, exist_ok=True)
    marker = b"PRIVATE-EFF-DEPENDENCY-MARKER" * 31
    path.write_bytes(marker)
    (package / "config.json").write_text(json.dumps({
        "new-dir-files": {"fighter/palutena/c00": [EFFECT]},
    }), encoding="utf-8")
    backend = Backend(args.backend)
    backend.request({"op": "configure", "config": {
        "root": str(fixture / "packs"), "cache": str(fixture / "cache"), "region": "zh_cn",
    }})
    backend.request({"op": "open", "path": str(source)})
    ax.open = lambda *a, **k: VirtualFile(backend)
    virtual = ax.Arc(str(source))
    rows = directory_rows(virtual)
    directories = {r[0] & 0xffffffffff: di for di, r in enumerate(rows)}
    pi = virtual.path_index_from_hash(ax.hash40(EFFECT))
    base_pi = virtual.path_index_from_hash(ax.hash40(BASE_EFFECT))
    assert pi is not None and pi >= original.n_path
    assert virtual.get(EFFECT) == marker
    # The effect registry enumerates this physical search folder even for c01.
    # A byte-readable individual file is insufficient: the requested directory
    # graph must retain it after every release, using one native data identity.
    owners = [di for di, row in enumerate(rows)
              if any(struct.unpack_from("<I", virtual.t, virtual.o_info + ii * 16)[0] == pi
                     for ii in range(row[5], row[5] + row[6]))]
    assert len(owners) == 1, ("one EFF owner, native reference counts", owners)
    assert base_pi in dependency_files(virtual, rows, owners[0]), "retain the vanilla effect library"
    for slot in range(8):
        di = directories[ax.hash40(f"fighter/palutena/c{slot:02}")]
        available = dependency_files(virtual, rows, di)
        assert pi in available, (
            "effect registry must never enumerate an unloaded EFF after a color change", slot)
    record = virtual.file_record(pi)
    assert record["info_flags"] & 0x7fff == 0x1000, "EFF uses native 4096-byte allocation alignment"
    assert virtual.get(BASE_EFFECT) == original.get(BASE_EFFECT), "vanilla EFF bytes must stay intact"
    for slot in (0, 1, 7):
        model = f"fighter/palutena/model/body/c{slot:02}/model.numdlb"
        assert virtual.get(model) == original.get(model), model
        for extension in (".nus3audio", ".nus3bank"):
            voice = f"sound/bank/fighter_voice/vc_palutena_c{slot:02}{extension}"
            assert virtual.get(voice, 13) == original.get(voice, 13), voice
    check_bulk_layout(virtual, source.stat().st_size)
    assert stamp == (source.stat().st_size, source.stat().st_mtime_ns)
    print("PASS every original Palutena color retains both effect libraries through native dependencies")
    print("PASS one EFF owner; native alignment and stream cursor; original EFF/models/voices/banks intact")
    print("PASS original game input unchanged; invalid EFF marker never installed for play")


if __name__ == "__main__":
    main()

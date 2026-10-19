"""Regression for a rebuilt costume waiting on an obsolete shared file group.

Uses a private archive read-only; invalid model markers stay in an isolated fixture.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

from test_mod_backend import Backend, VirtualFile, ax, check_bulk_layout, check_region_groups

ROOT = Path(__file__).resolve().parents[1]
COSTUME = "fighter/wolf/c04"
MODEL = "fighter/wolf/model/body/c04/model.numdlb"
ADDED = "fighter/wolf/model/body/c04/compat_extra.nutexb"
CANONICAL = "fighter/szerosuit/model/body/c00/alp_szerosuit_001_col.nutexb"
NONE = 0xffffff


def directories(archive):
    return {struct.unpack_from("<Q", archive.t, archive.o_dirinfo + i * 52)[0] & 0xffffffffff:
            struct.unpack_from("<3Q7I", archive.t, archive.o_dirinfo + i * 52)
            for i in range(archive.n_folder)}


def folder(archive, row):
    return struct.unpack_from("<Q5I", archive.t,
                              archive.o_folder_off + (row[0] >> 40) * 28)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", required=True)
    parser.add_argument("--arc")
    parser.add_argument("--fixture", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if not args.arc:
        state = json.loads((ROOT / "build/state.json").read_text(encoding="utf-8"))
        args.arc = str(Path(state["dump"]["romfs"]) / "data.arc")
    if args.fixture is None:
        with tempfile.TemporaryDirectory(prefix="ssbu-shared-group-wait-") as temp:
            subprocess.run([sys.executable, str(Path(__file__).resolve()), "--backend", args.backend,
                            "--arc", args.arc, "--fixture", temp], check=True)
        return
    source = Path(args.arc).resolve()
    stamp = source.stat().st_size, source.stat().st_mtime_ns
    original = ax.Arc(str(source))
    before_dirs = directories(original)
    before = before_dirs[ax.hash40(COSTUME)]
    assert folder(original, before)[-1] != NONE, "fixture requires a native shared-group redirect"
    package = args.fixture / "packs/wolf"
    expected = {MODEL: b"private-model-marker", ADDED: b"private-added-texture-marker",
                CANONICAL: b"private-canonical-texture-marker"}
    for path, data in expected.items():
        target = package / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    (package / "config.json").write_text(json.dumps({"new-dir-files": {COSTUME: [ADDED]}}),
                                          encoding="utf-8")
    hashes = {p.relative_to(package): hashlib.sha256(p.read_bytes()).hexdigest()
              for p in package.rglob("*") if p.is_file()}
    backend = Backend(Path(args.backend).resolve())
    backend.request({"op": "configure", "config": {
        "root": str(args.fixture / "packs"), "cache": str(args.fixture / "cache"),
        "profile": str(args.fixture / "profile.json"), "region": "zh_cn",
    }})
    backend.request({"op": "open", "path": str(source)})
    ax.open = lambda *a, **kw: VirtualFile(backend)
    virtual = ax.Arc(str(source))
    after_dirs = directories(virtual)
    after = after_dirs[ax.hash40(COSTUME)]
    assert folder(virtual, after)[-1] != NONE, "retain the original shared payload load"
    assert after[0] >> 40 != before[0] >> 40
    for path, data in expected.items():
        assert virtual.get(path, 13) == data
    # A canonical c00 replacement must not leave untouched c04 waiting for c00.
    for slot in range(8):
        path = f"fighter/szerosuit/c{slot:02}"
        h = ax.hash40(path)
        assert folder(original, before_dirs[h])[-1] != NONE
        assert folder(virtual, after_dirs[h])[-1] != NONE, path
    zero_suit = before_dirs[ax.hash40("fighter/szerosuit/c04")]
    for info in range(zero_suit[5], zero_suit[5] + zero_suit[6]):
        pi = struct.unpack_from("<I", original.t, original.o_info + info * 16)[0]
        h = original.file_path(pi)[0][0]
        assert virtual.get(h, 13) == original.get(h, 13), hex(h)
    assert backend.request({"op": "status"})["stats"]["shared_sources"] >= 1
    original_pi = original.path_index_from_hash(ax.hash40(CANONICAL))
    original_xi = original.file_path(original_pi)[0][1]
    private_source = f"__mods/shared_sources/{original_xi:06x}.bin"
    assert virtual.get(private_source, 13) == original.get(CANONICAL, 13)
    checked = 0
    for di, row in after_dirs.items():
        if row[-1] & (1 << 28):
            continue
        group = folder(virtual, row)[-1]
        if not virtual.n_folder_off1 <= group < virtual.n_folder_off1 + virtual.n_folder_off2:
            continue
        offset = struct.unpack_from("<Q5I", virtual.t, virtual.o_folder_off + group * 28)
        for ii in range(offset[3], offset[3] + offset[4]):
            pi, xi = struct.unpack_from("<2I", virtual.t, virtual.o_info + ii * 16)
            assert virtual.file_path(pi)[0][1] == xi, ("native shared wait has wrong identity", di, group, pi)
        checked += 1
    assert checked, "exercise actual native group waits"
    verified = 0
    # Preserve all untouched members, including those borrowed from shared and regional groups.
    for info in range(before[5], before[5] + before[6]):
        pi = struct.unpack_from("<I", original.t, original.o_info + info * 16)[0]
        h = original.file_path(pi)[0][0]
        if h == ax.hash40(MODEL):
            continue
        assert virtual.get(h, 13) == original.get(h, 13), hex(h)
        verified += 1
    for path in ("fighter/wolf/model/body/c00/model.numdlb",
                 "fighter/wolf/model/body/c05/model.numdlb",
                 "fighter/mario/model/body/c00/model.numdlb"):
        assert virtual.get(path, 13) == original.get(path, 13), path
    # A directory unrelated to the patch retains its original shared-group metadata.
    for path in ("fighter/mario/c00", "fighter/wolf/c05"):
        h = ax.hash40(path)
        old = folder(original, before_dirs[h])
        new = folder(virtual, after_dirs[h])
        assert new[:5] == old[:5] and new[-1] != NONE, path
        old_shared = struct.unpack_from("<Q5I", original.t, original.o_folder_off + old[-1] * 28)
        new_shared = struct.unpack_from("<Q5I", virtual.t, virtual.o_folder_off + new[-1] * 28)
        assert old_shared[:3] == new_shared[:3] and old_shared[4] == new_shared[4], path
    check_bulk_layout(virtual, source.stat().st_size)
    check_region_groups(original, virtual)
    assert hashes == {p.relative_to(package): hashlib.sha256(p.read_bytes()).hexdigest()
                      for p in package.rglob("*") if p.is_file()}
    assert stamp == (source.stat().st_size, source.stat().st_mtime_ns)
    print("PASS native group waits retain original sources;", verified,
          "untouched members, other colors and regional companions preserved; source files unchanged")


if __name__ == "__main__":
    main()

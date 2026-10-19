"""Private-archive regression for unedited native new-dir-files configs.

Never starts the game. Marker resources stay in temporary fixture packages.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

from test_mod_backend import Backend, VirtualFile, ax, check_bulk_layout

ROOT = Path(__file__).resolve().parents[1]
MODEL = "fighter/mario/model/body/c00/model.numdlb"
VANILLA = "fighter/mario/param/vl.prc"
ADDED = "fighter/mario/model/body/c00/compat_extra.nutexb"
ALIAS = "fighter/mario/model/body/c00/compat_alias.numdlb"
MISSING = "fighter/mario/model/body/c00/skin_ness_col.nutexb"
GROUP = "mod_compat/present"
ABSENT_GROUP = "mod_compat/absent"


def run_case(args):
    source = Path(args.arc).resolve()
    stamp = source.stat().st_size, source.stat().st_mtime_ns
    original = ax.Arc(str(source))
    fixture = args.fixture
    package = fixture / "packs/native-skin"
    package.mkdir(parents=True)
    if args.case == "missing-only":
        config = {"new-dir-files": {
            "fighter/mario/c00": [MISSING], ABSENT_GROUP: [VANILLA],
        }}
    elif args.case == "strict-sharing":
        config = {"share-to-vanilla": {MISSING: ALIAS}}
    else:
        key = "new_dir_files" if args.case == "underscore" else "new-dir-files"
        config = {
            "new-dir-infos": [GROUP],
            "share-to-vanilla": {MODEL: ALIAS},
            key: {GROUP: [MISSING, VANILLA, ADDED, ALIAS, MISSING],
                  ABSENT_GROUP: [VANILLA]},
        }
        target = package / ADDED
        target.parent.mkdir(parents=True)
        target.write_bytes(b"isolated-directory-compatibility-marker")
    config_path = package / "config.json"
    config_path.write_text(json.dumps(config), encoding="utf-8")
    before = {p.relative_to(package): hashlib.sha256(p.read_bytes()).hexdigest()
              for p in package.rglob("*") if p.is_file()}
    backend = Backend(Path(args.backend).resolve())
    backend.request({"op": "configure", "config": {
        "root": str(fixture / "packs"), "cache": str(fixture / "cache"),
        "profile": str(fixture / "mods.json"), "region": "us_en",
    }})
    plan = backend.request({"op": "plan"})["plan"]
    assert len(plan["entries"]) == (1 if args.case in ("hyphen", "underscore") else 0)
    if args.case == "strict-sharing":
        try:
            backend.request({"op": "open", "path": str(source)})
        except AssertionError as error:
            assert "shared source" in str(error) and "does not exist" in str(error), error
        else:
            raise AssertionError("missing required share source must remain fatal")
    else:
        result = backend.request({"op": "open", "path": str(source)})
        warnings = result["diagnostics"]
        assert len(warnings) == 2, warnings
        assert {d["code"] for d in warnings} == {"missing_directory_file", "missing_directory"}
        assert all(d["level"] == "warning" and str(config_path) in d["message"] for d in warnings)
        assert any(MISSING in d["message"] for d in warnings)
        assert backend.request({"op": "status"})["diagnostics"] == warnings
        # A fresh scan must not erase active archive diagnostics.
        backend.request({"op": "plan"})
        assert backend.request({"op": "status"})["diagnostics"] == warnings
        ax.open = lambda *a, **kw: VirtualFile(backend)
        virtual = ax.Arc(str(source))
        assert virtual.path_index_from_hash(ax.hash40(MISSING)) is None
        assert virtual.get(MODEL) == original.get(MODEL)
        assert virtual.get(VANILLA) == original.get(VANILLA)
        if args.case == "missing-only":
            assert result["archive"]["groups"] == 0
            assert result["archive"]["additions"] == 0
        else:
            assert virtual.get(ADDED) == (package / ADDED).read_bytes()
            assert virtual.get(ALIAS) == original.get(MODEL)
            directories = {struct.unpack_from("<Q", virtual.t, virtual.o_dirinfo + di * 52)[0]
                           & 0xffffffffff: di for di in range(virtual.n_folder)}
            assert ax.hash40(ABSENT_GROUP) not in directories
            row = struct.unpack_from("<3Q7I", virtual.t,
                                     virtual.o_dirinfo + directories[ax.hash40(GROUP)] * 52)
            members = set()
            for index in range(row[5], row[5] + row[6]):
                pi = struct.unpack_from("<I", virtual.t, virtual.o_info + index * 16)[0]
                members.add(virtual.file_path(pi)[0][0])
            assert members == {ax.hash40(p) for p in (VANILLA, ADDED, ALIAS)}, members
            check_bulk_layout(virtual, source.stat().st_size)
    after = {p.relative_to(package): hashlib.sha256(p.read_bytes()).hexdigest()
             for p in package.rglob("*") if p.is_file()}
    assert after == before, "native package files must remain byte-identical"
    assert stamp == (source.stat().st_size, source.stat().st_mtime_ns)
    print("PASS", args.case, "native config untouched; optional references skipped; required resources validated")

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", required=True)
    parser.add_argument("--arc")
    parser.add_argument("--fixture", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--case", choices=("hyphen", "underscore", "missing-only", "strict-sharing"))
    args = parser.parse_args()
    if not args.arc:
        state = json.loads((ROOT / "build/state.json").read_text(encoding="utf-8"))
        args.arc = str(Path(state["dump"]["romfs"]) / "data.arc")
    if args.fixture:
        run_case(args)
        return
    for case in ([args.case] if args.case else ("hyphen", "underscore", "missing-only", "strict-sharing")):
        # Own fixture cleanup in the parent, after the backend releases its read-only handles.
        with tempfile.TemporaryDirectory(prefix="ssbu-native-directory-") as folder:
            subprocess.run([sys.executable, str(Path(__file__).resolve()), "--backend", args.backend,
                            "--arc", args.arc, "--case", case, "--fixture", folder], check=True)


if __name__ == "__main__":
    main()

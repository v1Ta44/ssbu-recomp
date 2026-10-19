"""Private-archive regression for prebuilt; resources (no game or video playback)."""
import argparse
import json
from pathlib import Path
import tempfile
import subprocess
import sys

from test_mod_backend import Backend, VirtualFile, ax, contents

ROOT = Path(__file__).resolve().parents[1]
RESOURCE = "prebuilt:/movie/fighter/brave/c00/final_00.h264"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", required=True)
    parser.add_argument("--fixture", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.fixture is None:
        with tempfile.TemporaryDirectory(prefix="ssbu-prebuilt-") as folder:
            subprocess.run([sys.executable, str(Path(__file__).resolve()), "--backend", args.backend,
                            "--fixture", folder], check=True)
        return
    source = Path(json.loads((ROOT / "build/state.json").read_text(encoding="utf-8"))["dump"]["romfs"]) / "data.arc"
    stamp = source.stat().st_size, source.stat().st_mtime_ns
    original = ax.Arc(str(source))
    assert original.path_index_from_hash(ax.hash40(RESOURCE)) is not None
    root = args.fixture
    target = root / "packs/hero/prebuilt;/movie/fighter/brave/c00/final_00.h264"
    target.parent.mkdir(parents=True)
    expected = b"prebuilt regression replacement" * 43
    target.write_bytes(expected)
    backend = Backend(args.backend)
    backend.request({"op": "configure", "config": {
        "root": str(root / "packs"), "cache": str(root / "cache"),
        "profile": str(root / "mods.json"), "region": "us_en",
    }})
    plan = backend.request({"op": "plan"})["plan"]
    assert len(plan["entries"]) == 1 and not plan["diagnostics"]
    entry = plan["entries"][0]
    assert entry["path"] == RESOURCE and entry["hash"] == ax.hash40(RESOURCE)
    result = backend.request({"op": "open", "path": str(source)})
    assert result["archive"]["replacements"] == 1, result
    assert result["archive"]["additions"] == 0, result
    assert result["archive"]["streams"] == 0, result
    ax.open = lambda *a, **kw: VirtualFile(backend)
    virtual = ax.Arc(str(source))
    assert contents(virtual, RESOURCE) == expected
    for path in ("fighter/mario/model/body/c00/model.numdlb",
                 "fighter/palutena/model/body/c01/model.numdlb"):
        assert contents(virtual, path) == contents(original, path), path
    assert target.read_bytes() == expected
    assert stamp == (source.stat().st_size, source.stat().st_mtime_ns)
    print("PASS exact reported prebuilt; path replaces the existing archive file; no new file/stream")
    print("PASS independent reader sees replacement bytes; unrelated models, mod source and archive unchanged")


if __name__ == "__main__":
    main()

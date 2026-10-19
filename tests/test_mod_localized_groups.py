"""Private ARC regression for native localized load groups; no game is started.

Marker payloads are intentionally invalid assets and stay in isolated fixtures.
"""
import argparse
import json
import os
from pathlib import Path
import struct
import subprocess
import sys

from test_mod_backend import Backend, VirtualFile, ax

ROOT = Path(__file__).resolve().parents[1]


def info(arc, pi):
    xi = arc.file_path(pi)[0][1]
    directory, ii = struct.unpack_from("<2I", arc.t, arc.o_info_index + xi * 8)
    row = struct.unpack_from("<4I", arc.t, arc.o_info + (ii & 0xffffff) * 16)
    return directory, row


def selector(flags, region):
    return region if flags & 0x8000 else (2 if region <= 4 else 5) if flags & 0x10000 else 0


def read(arc, pi, region):
    return arc.read(arc.file_record(pi, selector(info(arc, pi)[1][3], region)))


def check_group(arc, folder):
    offset, _, compressed, start, count, _ = struct.unpack_from("<Q5I", arc.t, arc.o_folder_off + folder * 28)
    cursor = 0
    for did in range(start, start + count):
        position, size, _, _ = struct.unpack_from("<4I", arc.t, arc.o_data + did * 16)
        assert position * 4 == cursor, ("native localized stream cursor", folder, did)
        cursor += (size + 15) & ~15
    assert cursor == compressed, ("localized group span", folder)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", default=str(ROOT / "build/mod-adapt/release-host/ssbu_mod_backend.dll"))
    parser.add_argument("--region", choices=("us_en", "zh_cn"))
    args = parser.parse_args()
    if args.region is None:
        # Active views deliberately survive config changes until process restart.
        for region in ("us_en", "zh_cn"):
            subprocess.run([sys.executable, str(Path(__file__).resolve()), "--backend", args.backend,
                            "--region", region], check=True)
        return
    state = json.loads((ROOT / "build/state.json").read_text(encoding="utf-8"))
    source = Path(state["dump"]["romfs"]) / "data.arc"
    before = (source.stat().st_size, source.stat().st_mtime_ns)
    original = ax.Arc(str(source))
    fixtures = ROOT / "build/mod-adapt/localized-fixtures" / str(os.getpid())
    package = fixtures / "packs/test"
    paths = ["ui/message/msg_name.msbt", "sound/bank/narration/vc_narration_common.nus3audio",
             "ui/layout/info/info_center/info_center/layout.arc"]
    expected = {}
    for path in paths:
        pi = original.path_index_from_hash(ax.hash40(path))
        assert pi is not None, path
        expected[pi] = (path, b"LOCALIZED-MARKER-" + path.encode() * 37)
    # Also exercise the five-locale mapping; find a real canonical localized file
    # whose native directory uses that request class, without a path-name dump.
    for pi in range(original.n_path):
        directory, row = info(original, pi)
        if row[3] & 0x10000 and directory < original.n_folder:
            flags = struct.unpack_from("<I", original.t, original.o_dirinfo + directory * 52 + 48)[0]
            if flags >> 24 & 3 == 2:
                expected[pi] = ("0x%010x" % original.file_path(pi)[0][0], b"LOCALE-MARKER" * 31)
                break
    assert len(expected) == 4, "a real locale-group fixture is required"
    for _, (path, data) in expected.items():
        destination = package / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    backend = Backend(args.backend)
    ax.open = lambda *args, **kwargs: VirtualFile(backend)
    for region, name in [(2, "us_en")] if args.region == "us_en" else [(13, "zh_cn")]:
        backend.request({"op": "configure", "config": {"root": str(fixtures / "packs"),
                        "cache": str(fixtures / "cache"), "region": name}})
        backend.request({"op": "plan"})
        backend.request({"op": "open", "path": str(source)})
        virtual = ax.Arc(str(source))
        unchanged = 0
        for pi, (path, data) in expected.items():
            directory, old_row = info(original, pi)
            _, new_row = info(virtual, pi)
            assert new_row[3] & 0x18000 == old_row[3] & 0x18000, ("regional request class changed", path)
            selected = selector(old_row[3], region)
            base = struct.unpack_from("<Q", virtual.t, virtual.o_dirinfo + directory * 52)[0] >> 40
            record = virtual.file_record(pi, selected)
            assert record["folder_offset_index"] == base + selected, ("scheduler selects a different group", path)
            assert read(virtual, pi, region) == data, path
            check_group(virtual, base + selected)
            variants = original.reg1 if old_row[3] & 0x8000 else original.reg2
            for other in range(variants + 1):
                if other != selected:
                    actual = virtual.file_record(pi, other)
                    before_record = original.file_record(pi, other)
                    for key in ("comp_size", "decomp_size", "data_flags"):
                        assert actual[key] == before_record[key], ("inactive metadata changed", path, other, key)
                    # The region-zero sentinel may have a compression flag with
                    # no frame. Compare stored bytes without decoding that sentinel.
                    virtual.f.seek(actual["offset"]); original.f.seek(before_record["offset"])
                    assert virtual.f.read(actual["comp_size"]) == original.f.read(before_record["comp_size"]), ("inactive language changed", path, other)
            start, count = struct.unpack_from("<2I", original.t, original.o_dirinfo + directory * 52 + 32)
            for ii in range(start, start + count):
                neighbor = struct.unpack_from("<I", original.t, original.o_info + ii * 16)[0]
                if neighbor not in expected:
                    actual_bytes, expected_bytes = read(virtual, neighbor, region), read(original, neighbor, region)
                    assert actual_bytes == expected_bytes, ("neighbor changed", neighbor, name)
                    unchanged += 1
        print("PASS", name, "four localized replacements, native group streams, inactive languages and", unchanged, "unchanged neighbors")
    assert before == (source.stat().st_size, source.stat().st_mtime_ns)
    print("PASS original archive unchanged; invalid marker assets never installed for play")


if __name__ == "__main__":
    main()

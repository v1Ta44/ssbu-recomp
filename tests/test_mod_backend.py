"""One optional private-archive integration check; never starts or modifies the game.

Build the backend first. --arc defaults to SSBU_ARC or build/state.json.
Fixtures are deliberately invalid model/texture markers: do not install them for play.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import ctypes
import json
import os
from pathlib import Path
import struct
import sys

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / "tools" / "nro"))
import arc_extract as ax


class Backend:
    def __init__(self, path):
        self.dll = ctypes.CDLL(str(path))
        self.dll.sbm_request.argtypes = [ctypes.c_char_p]
        self.dll.sbm_request.restype = ctypes.c_void_p
        self.dll.sbm_free.argtypes = [ctypes.c_void_p]
        self.dll.sbm_read.argtypes = [ctypes.c_uint64, ctypes.c_void_p, ctypes.c_size_t]
        self.dll.sbm_read.restype = ctypes.c_int64
        self.dll.sbm_last_error.restype = ctypes.c_char_p
        self.dll.sbm_size.restype = ctypes.c_uint64

    def request(self, request):
        pointer = self.dll.sbm_request(json.dumps(request).encode("utf-8"))
        try:
            response = json.loads(ctypes.string_at(pointer))
        finally:
            self.dll.sbm_free(pointer)
        assert response["ok"], response
        return response["result"]

    def read(self, offset, size):
        buffer = ctypes.create_string_buffer(size)
        count = self.dll.sbm_read(offset, buffer, size)
        assert count >= 0, self.dll.sbm_last_error()
        return buffer.raw[:count]


class VirtualFile:
    def __init__(self, backend):
        self.backend = backend
        self.position = 0

    def seek(self, offset, whence=0):
        if whence == 0:
            self.position = offset
        elif whence == 1:
            self.position += offset
        else:
            self.position = self.backend.dll.sbm_size() + offset
        return self.position

    def read(self, size):
        data = self.backend.read(self.position, size)
        self.position += len(data)
        return data


def contents(archive, path):
    index = archive.path_index_from_hash(ax.hash40(path))
    assert index is not None, path
    return archive.read(archive.file_record(index))


def check_bulk_layout(archive, original_size):
    for directory in range(archive.n_folder):
        row = struct.unpack_from("<3Q7I", archive.t, archive.o_dirinfo + directory * 52)
        folder = row[0] >> 40
        offset = struct.unpack_from("<Q5I", archive.t, archive.o_folder_off + folder * 28)
        if archive.file_off + offset[0] < original_size:
            continue
        cursor = 0
        for slot, info in enumerate(range(row[5], row[5] + row[6])):
            _, _, mapping, flags = struct.unpack_from("<4I", archive.t, archive.o_info + info * 16)
            # Regional/localized entries consume their companion stream. Their
            # selector-zero metadata is not part of the native base-group cursor.
            if flags & 0x18000:
                continue
            owner, data_index, _ = struct.unpack_from("<3I", archive.t, archive.o_i2d + mapping * 12)
            if owner != folder:
                continue
            position, compressed, _, _ = struct.unpack_from("<4I", archive.t, archive.o_data + data_index * 16)
            assert data_index == offset[3] + slot, ("parallel FileData slots", directory, slot)
            assert position * 4 == cursor, ("native stream position", directory, slot)
            alignment = flags & 0x7fff
            assert alignment == 0 or alignment & (alignment - 1) == 0, ("allocation alignment", flags)
            cursor += (compressed + 15) & ~15
        assert cursor == offset[2], ("group padding", directory, cursor, offset[2])
    print("PASS native bulk-loader layout, padding and alignment")


def check_region_groups(original, virtual):
    # Native 13.0.4 scheduler (0x71035456d0) selects the companion at
    # DirInfo.folder + 1 + region_index / locale_index. It is not a standalone
    # lookup through the individual file's InfoToData. A readable model alone
    # does not prove that the adjacent NRO/language group can be scheduled.
    regions, locales = struct.unpack_from("<2B", original.t, 56)
    checked = 0
    for directory in range(original.n_folder):
        before = struct.unpack_from("<3Q7I", original.t, original.o_dirinfo + directory * 52)
        after = struct.unpack_from("<3Q7I", virtual.t, virtual.o_dirinfo + directory * 52)
        old_folder, new_folder = before[0] >> 40, after[0] >> 40
        if old_folder == new_folder:
            continue
        kind = (before[9] >> 24) & 3
        companions = regions if kind == 1 else locales if kind == 2 else 0
        for variant in range(1, companions + 1):
            expected = struct.unpack_from("<Q5I", original.t, original.o_folder_off + (old_folder + variant) * 28)
            actual = struct.unpack_from("<Q5I", virtual.t, virtual.o_folder_off + (new_folder + variant) * 28)
            # Byte ranges and group shape must remain those of the original
            # regional payload, including NROs unrelated to the replacement.
            assert actual[:3] == expected[:3], ("regional load group redirected", directory, variant, expected, actual)
            assert actual[4] == expected[4], ("regional file count", directory, variant)
            if expected[3] < original.n_data:
                assert actual[3] == expected[3], ("regional FileData range", directory, variant)
            checked += 1
    assert checked, "fixture must touch a group with regional companions"
    print("PASS native adjacent regional/localized load groups", checked)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arc", default=os.environ.get("SSBU_ARC"))
    parser.add_argument("--backend", default=str(REPO / "build/mod-adapt/cargo/debug/ssbu_mod_backend.dll"))
    args = parser.parse_args()
    if not args.arc:
        state = json.loads((REPO / "build/state.json").read_text(encoding="utf-8"))
        args.arc = str(Path(state["dump"]["romfs"]) / "data.arc")
    source_stamp = (Path(args.arc).stat().st_size, Path(args.arc).stat().st_mtime_ns)
    original = ax.Arc(args.arc)
    original_size = original.f.seek(0, 2)
    root = REPO / "build/mod-adapt/reader-fixtures" / str(os.getpid()) / "packs"
    package = root / "test"
    package.mkdir(parents=True, exist_ok=True)
    model = "fighter/mario/model/body/c00/model.numdlb"
    skeleton = "fighter/mario/model/body/c00/model.nusktb"
    addition = "fighter/mario/model/body/c00/new_extra.nutexb"
    alias = "fighter/mario/model/body/c00/shared_alias.numdlb"
    second_alias = "fighter/mario/model/body/c00/second_alias.numdlb"
    expected = {
        model: b"MODDED-LARGER-" + bytes(range(256)) * 40,
        skeleton: b"SKELETON-OVERRIDE-" + bytes(range(100)),
        addition: b"new-file-content",
    }
    for path, data in expected.items():
        destination = package / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    quick, hashes, _, _ = struct.unpack_from("<4I", original.t, 0x100)
    stream_table = 0x110 + quick * 12 + hashes * 8
    stream_hash = struct.unpack_from("<Q", original.t, stream_table)[0] & 0xffffffffff
    (package / f"0x{stream_hash:010x}").write_bytes(b"streamed-override")
    (package / "config.json").write_text(json.dumps({
        "share-to-vanilla": {model: alias, alias: second_alias},
        "new-dir-infos": ["mod_adapt/shared"],
        "new-dir-files": {"mod_adapt/shared": ["fighter/mario/param/vl.prc"]},
    }), encoding="utf-8")
    backend = Backend(args.backend)
    backend.request({"op": "configure", "config": {
        "root": str(root), "cache": str(root.parent / "cache"), "region": "us_en",
    }})
    backend.request({"op": "plan"})
    backend.request({"op": "open", "path": original.path})
    # Reuse the repository's independent extractor through its ordinary seek/read interface.
    ax.open = lambda *args, **kwargs: VirtualFile(backend)
    virtual = ax.Arc(original.path)
    expected[alias] = expected[model]
    expected[second_alias] = expected[model]
    for path, data in expected.items():
        assert contents(virtual, path) == data, path
        print("PASS resource", path, len(data))
    for path in ["fighter/mario/model/body/c01/model.nusktb",
                 "fighter/mario/model/body/c01/model.numdlb", "fighter/mario/param/vl.prc"]:
        assert contents(virtual, path) == contents(original, path), path
        print("PASS unchanged", path)
    virtual.f.seek(virtual.search_off)
    _, decompressed, compressed, _ = struct.unpack("<4I", virtual.f.read(16))
    search = virtual.zd.decompress(virtual.f.read(compressed), max_output_size=decompressed)
    _, folders, indices, paths = struct.unpack_from("<Q3I", search)
    assert len(search) == 20 + folders * 40 + indices * 12 + paths * 32
    lookup = 20 + folders * 40
    assert any(struct.unpack_from("<Q", search, lookup + i * 8)[0] & 0xffffffffff == ax.hash40(addition)
               for i in range(indices))
    print("PASS native search table")
    quick, hashes, indices, _ = struct.unpack_from("<4I", virtual.t, 0x100)
    entries = 0x110 + quick * 12 + hashes * 8
    for i in range(hashes):
        value, _ = struct.unpack_from("<QI", virtual.t, entries + i * 12)
        if value & 0xffffffffff == stream_hash:
            index = struct.unpack_from("<I", virtual.t, entries + hashes * 12 + (value >> 40) * 4)[0]
            size, offset = struct.unpack_from("<QQ", virtual.t, entries + hashes * 12 + indices * 4 + index * 16)
            assert backend.read(offset, size) == b"streamed-override"
            break
    else:
        raise AssertionError("missing stream")
    print("PASS stream offset and size")
    check_bulk_layout(virtual, original_size)
    check_region_groups(original, virtual)
    for offset in (0x40, 0x44, 0x50, 0x54):
        assert struct.unpack_from("<I", original.t, offset) == struct.unpack_from("<I", virtual.t, offset)
    assert original.t[original.end:] == virtual.t[virtual.end:]
    print("PASS preserved version/DLC auxiliary counts and opaque tail")
    record = virtual.file_record(virtual.path_index_from_hash(ax.hash40(model)))

    def chunk(number):
        offset = number * 37 % (len(expected[model]) - 64)
        assert backend.read(record["offset"] + offset, 64) == expected[model][offset:offset + 64]

    with ThreadPoolExecutor(max_workers=4) as pool:
        list(pool.map(chunk, range(20)))
    print("PASS concurrent partial reads")
    assert backend.request({"op": "set_package", "id": "test", "enabled": False})["restart_required"]
    assert contents(virtual, model) == expected[model]
    print("PASS configuration changes preserve the active archive until restart")
    assert source_stamp == (Path(args.arc).stat().st_size, Path(args.arc).stat().st_mtime_ns)
    print("PASS original archive unchanged")


if __name__ == "__main__":
    main()

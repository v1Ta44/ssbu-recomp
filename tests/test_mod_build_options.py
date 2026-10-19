"""Focused build/packaging checks; no game, private dump or Rust toolchain required."""
import hashlib
import os
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]


sys.path.insert(0, str(ROOT))
from scripts import package, setup


class BuildOptions(unittest.TestCase):
    def test_resumed_build_relinks_when_switch_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory) / "state.json"
            state.write_text(json.dumps({"done": {s: {"secs": 0} for s in setup.STEPS},
                                         "features": {"resource_mods": False}}), encoding="utf-8")
            linked = []

            def link(cx):
                linked.append(cx.a.with_mods)
                cx.state.setdefault("features", {})["resource_mods"] = cx.a.with_mods

            runners = {s: (lambda cx: None) for s in setup.STEPS}
            runners["link"] = link
            with patch.object(setup, "RUNNERS", runners), patch.object(setup, "say"), patch.object(setup, "how_to_run"):
                for flags in (["--with-mods"], []):
                    with patch.object(sys, "argv", ["setup.py", "--build", directory, *flags]):
                        self.assertEqual(setup.main(), 0)
                # The next identical build must remain resumable.
                with patch.object(sys, "argv", ["setup.py", "--build", directory]):
                    self.assertEqual(setup.main(), 0)
            self.assertEqual(linked, [True, False])

    def test_packaging_requires_feature_record_for_exact_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            exe = Path(directory) / "test.exe"
            exe.write_bytes(b"host fixture")
            stamp = exe.with_suffix(".build.json")
            with self.assertRaises(SystemExit):
                package.host_features(str(exe))
            for enabled in (False, True):
                stamp.write_text(json.dumps({"schema_version": 1, "resource_mods": enabled,
                                             "exe_sha256": hashlib.sha256(exe.read_bytes()).hexdigest()}), encoding="utf-8")
                self.assertEqual(package.host_features(str(exe)), {"resource_mods": enabled, "latency_slider": False})
            exe.write_bytes(b"replaced host")
            with self.assertRaises(SystemExit):
                package.host_features(str(exe))

    def test_disabled_package_needs_no_backend_stamp(self):
        with tempfile.TemporaryDirectory() as directory:
            exe = Path(directory) / "test.exe"
            for path in (exe, exe.with_suffix(".pdb"), exe.with_suffix(".map")):
                path.write_bytes(b"fixture")
            package.check_build_is_current(directory, str(exe), False, resource_mods=False)
            with self.assertRaises(SystemExit):
                package.check_build_is_current(directory, str(exe), False, resource_mods=True)

    def test_zip_accepts_license_epoch_and_preserves_mod_layout(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "package"
            license = root / "game/mod_backend_licenses/old/LICENSE"
            license.parent.mkdir(parents=True)
            license.write_bytes(b"license")
            os.utime(license, (1, 1))
            (root / "mods").mkdir()
            (root / "mods/README.txt").write_bytes(b"install here")
            (root / "romfs").mkdir()
            (root / "romfs/data.arc").write_bytes(b"must not redistribute in ZIP")
            archive = Path(directory) / "test.zip"
            package.zip_package(str(root), str(archive), "package")
            with zipfile.ZipFile(archive) as output:
                self.assertEqual(output.read("package/mods/README.txt"), b"install here")
                self.assertEqual(output.getinfo("package/game/mod_backend_licenses/old/LICENSE").date_time[0], 1980)
                self.assertNotIn("package/romfs/data.arc", output.namelist())

    @unittest.skipUnless(sys.platform == "win32", "Windows adapter")
    def test_disabled_adapter_ignores_mod_paths_and_preserves_reads(self):
        clang = os.environ.get("RECOMP_CLANG") or shutil.which("clang") or str(ROOT / "deps/llvm/bin/clang.exe")
        if not Path(clang).is_file():
            self.skipTest("clang is not installed")
        with tempfile.TemporaryDirectory() as directory:
            dll = Path(directory) / "disabled.dll"
            subprocess.run([clang, "-shared", str(ROOT / "runtime/mod_resources.c"), "-Wl,/EXPORT:mod_resources_open", "-Wl,/EXPORT:mod_resources_read",
                            "-Wl,/EXPORT:mod_resources_size", "-o", str(dll)], check=True,
                           capture_output=True)
            # A subprocess releases the Windows DLL before the temporary directory is removed.
            code = r'''
import ctypes, json, os, sys
os.environ["SSBU_RESOURCE_MODS"] = "Z:/definitely-missing/mods"
os.environ["SSBU_MOD_PROFILE"] = "Z:/definitely-missing/profile.json"
dll = ctypes.CDLL(sys.argv[1])
dll.ssbu_mods_request.restype = ctypes.c_void_p
dll.ssbu_mods_request.argtypes = [ctypes.c_char_p]
dll.ssbu_mods_free.argtypes = [ctypes.c_void_p]
reply = dll.ssbu_mods_request(b'{"op":"capabilities"}')
assert json.loads(ctypes.string_at(reply))["error"]["code"] == "feature_disabled"
dll.ssbu_mods_free(reply)
dll.mod_resources_open.argtypes = [ctypes.c_uint64, ctypes.c_char_p]
dll.mod_resources_read.argtypes = [ctypes.c_uint64, ctypes.c_int64, ctypes.c_void_p, ctypes.c_uint64, ctypes.POINTER(ctypes.c_uint64)]
dll.mod_resources_size.argtypes = [ctypes.c_uint64, ctypes.POINTER(ctypes.c_uint64)]
assert dll.mod_resources_open(123, b"missing/data.arc") == 1
count = ctypes.c_uint64(456)
assert dll.mod_resources_read(123, 0, None, 0, ctypes.byref(count)) == 0
assert count.value == 456
assert dll.mod_resources_size(123, ctypes.byref(count)) == 0
assert count.value == 456
'''
            result = subprocess.run([sys.executable, "-c", code, str(dll)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()

"""Build feature transitions, exact packaging records and native plugin loading."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from scripts import package, setup


class OptionalFeatures(unittest.TestCase):
    def test_each_feature_transition_relinks_and_invalidates_check(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory) / "state.json"
            state.write_text(json.dumps({"done": {s: {"secs": 0} for s in setup.STEPS},
                                         "features": {"resource_mods": False, "latency_slider": False}}))
            linked = []
            checked = []
            def link(cx):
                pair = (cx.a.with_mods, cx.a.with_latency_slider)
                linked.append(pair)
                self.assertNotIn("check", cx.state["done"])
                cx.state["features"] = dict(zip(("resource_mods", "latency_slider"), pair))
            runners = {s: (lambda cx: None) for s in setup.STEPS}
            runners.update(link=link, check=lambda cx: checked.append(1))
            cases = [([], (False, False)), (["--with-mods"], (True, False)),
                     (["--with-mods", "--with-latency-slider"], (True, True)),
                     (["--with-latency-slider"], (False, True)),
                     (["--without-mods", "--without-latency-slider"], (False, False))]
            with patch.object(setup, "RUNNERS", runners), patch.object(setup, "say"), patch.object(setup, "how_to_run"):
                for flags, _ in cases:
                    for repeat in range(2):
                        with patch.object(sys, "argv", ["setup.py", "--build", directory, *flags]):
                            self.assertEqual(setup.main(), 0)
            self.assertEqual(linked, [pair for _, pair in cases[1:]])
            self.assertEqual(len(checked), 4)

    def test_setup_builds_only_enabled_extension_dependencies(self):
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            for name in (setup.EXE, 'SDL3.dll', 'ssbu_glslang.dll', 'ssbu_game_eh_o1.dll'):
                (work / name).write_bytes(b'fixture')
            for resource, latency in ((False, False), (True, False), (False, True), (True, True)):
                commands = []
                cx = SimpleNamespace(a=SimpleNamespace(with_mods=resource, with_latency_slider=latency),
                                     work=str(work), build=str(work), bin=str(work / 'binaries/13.0.4'), tools={'clang': 'fixture-clang'}, state={},
                                     env=lambda **kwargs: kwargs, run=lambda step, argv, **kwargs: commands.append(argv))
                with patch('scripts.build_latency_slider.build_extension') as extension, patch.object(setup, 'say'):
                    setup.step_link(cx)
                self.assertEqual(extension.call_count, int(latency))
                self.assertEqual(any(any('build_mod_backend.py' in arg for arg in cmd) for cmd in commands), resource)
                host = next(cmd for cmd in commands if 'build.py' in cmd)
                self.assertIn('--with-mods' if resource else '--without-mods', host)
                self.assertIn('--with-latency-slider' if latency else '--without-latency-slider', host)
                self.assertEqual(cx.state['features'], dict(resource_mods=resource, latency_slider=latency))

    def test_1305_rejects_1304_delay_extension_before_building(self):
        from types import SimpleNamespace
        commands = []
        cx = SimpleNamespace(a=SimpleNamespace(with_mods=True, with_latency_slider=True),
                             state={'game_version': '13.0.5'},
                             run=lambda *args, **kwargs: commands.append(args))
        with patch('scripts.build_latency_slider.build_extension') as extension:
            with self.assertRaisesRegex(setup.Fail, '13.0.4 only'):
                setup.step_link(cx)
            extension.assert_not_called()
        self.assertEqual(commands, [])

    def test_packaging_omits_disabled_leftovers_and_rejects_modified_extension(self):
        from scripts.build_latency_slider import source_fingerprint
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            exe = work / "host.exe"
            exe.write_bytes(b"host")
            artifacts = {}
            for relative in ("plugins/latency_slider.dll", "net_delay/delay_panel.exe", "net_delay/WebView2-LICENSE.txt",
                             "net_delay/ui/index.html", "net_delay/ui/panel.css", "net_delay/ui/panel.js",
                             "net_delay/ui/console/style.css", "net_delay/ui/console/fonts/OFL-Saira.txt",
                             "net_delay/ui/console/fonts/OFL-SmileySans.txt"):
                path = work / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(relative.encode())
                artifacts[relative] = package.sha256(path)
            stamp = work / "net_delay/build.json"
            stamp.write_text(json.dumps(dict(schema_version=1, source_sha256=source_fingerprint(), artifacts_sha256=artifacts)))
            host = dict(schema_version=2, resource_mods=False, latency_slider=True,
                        exe_sha256=package.sha256(exe), latency_slider_sha256=package.sha256(stamp))
            exe.with_suffix(".build.json").write_text(json.dumps(host))
            self.assertEqual(package.host_features(str(exe)), dict(resource_mods=False, latency_slider=True))
            self.assertEqual(len(package.latency_files(directory, str(exe), True)), len(artifacts) + 1)
            (work / "plugins/latency_slider.dll").write_bytes(b"replaced")
            with self.assertRaises(SystemExit):
                package.latency_files(directory, str(exe), True)
            self.assertEqual(package.latency_files(directory, str(exe), False), [])
            host["latency_slider"] = 1
            exe.with_suffix(".build.json").write_text(json.dumps(host))
            with self.assertRaises(SystemExit):
                package.host_features(str(exe))

    @unittest.skipUnless(sys.platform == "win32", "Windows plugin loader")
    def test_native_loader_respects_build_switch_and_loads_only_once(self):
        clang = os.environ.get("RECOMP_CLANG", str(ROOT / "deps/llvm/bin/clang.exe"))
        if not Path(clang).is_file():
            self.skipTest("clang not installed")
        source = (ROOT / "fighter_sdk/hook_host.c").read_text()
        loader = source[source.index("#ifndef SSBU_ENABLE_NET_DELAY"):source.index("/* ---------------------------------------------------------------- SSBU_HOOK_COUNT")]
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            plugins = work / "plugins"
            plugins.mkdir()
            fixture = work / "plugin.c"
            fixture.write_text('#include <stdio.h>\n#include <stdlib.h>\n__declspec(dllexport) int ssbu_delay_state(void) {return 1;}\n'
                               '__declspec(dllexport) void mod_init(void *api) {FILE *f=fopen(getenv("SSBU_TEST_MARKER"),"ab"); if(f){fputc(49,f);fclose(f);}}\n')
            subprocess.run([clang, "-shared", str(fixture), "-o", str(plugins / "latency_slider.dll")], check=True, capture_output=True)
            harness = work / "host.c"
            harness.write_text('#include <windows.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n'
                               'typedef struct {int unused;} HookApi; static const HookApi g_hook_api={0}; static int g_hk_layers;\n'
                               + loader + '\nint main(void){hook_load_delay();hook_load_mods();return 0;}\n')
            marker = work / "loaded.txt"
            environment = dict(os.environ, SSBU_MODS=str(plugins), SSBU_TEST_MARKER=str(marker))
            for enabled in (0, 1):
                executable = work / ("host%d.exe" % enabled)
                subprocess.run([clang, "-DSSBU_ENABLE_NET_DELAY=%d" % enabled, str(harness), "-o", str(executable)], check=True, capture_output=True)
                subprocess.run([str(executable)], cwd=ROOT, env=environment, check=True, capture_output=True)
                self.assertEqual(marker.read_bytes() if marker.exists() else b"", b"1" if enabled else b"")


if __name__ == "__main__":
    unittest.main()

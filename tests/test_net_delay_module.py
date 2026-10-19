"""Verify independent backend, verified game adapter, real Console frontend and IPC."""
import argparse
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from build_latency_slider import OUT, compile_frontend, native_compile


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--clang", default="D:/pcsmash/deps/llvm/bin/clang.exe")
    args = ap.parse_args()
    work = OUT / "module-test"
    work.mkdir(parents=True, exist_ok=True)
    extension = ROOT / "extensions/net_delay"
    core = work / "net_delay_core.exe"
    native_compile([ROOT / "tests/net_delay_core.cpp", extension / "core/delay_engine.cpp"], core, [extension / "include"])
    subprocess.run([str(core)], check=True, timeout=10)
    preferences = work / "net_delay_preferences.exe"
    native_compile([ROOT / "tests/net_delay_preferences.cpp", extension / "adapter/preferences.cpp"],
                   preferences, [extension / "include", extension / "adapter"])
    subprocess.run([str(preferences), str(work / "settings.ini")], check=True, timeout=10)
    frontend = compile_frontend(args.clang)
    assets = frontend.parent / "ui"
    # Byte-for-byte reuse is the design contract, including all animation curves.
    for p in [ROOT / "tools/console/ui/style.css", *(ROOT / "tools/console/ui/fonts").glob("*")]:
        if p.is_file():
            assert p.read_bytes() == (assets / "console" / p.relative_to(ROOT / "tools/console/ui")).read_bytes()
    print("PASS Console CSS, motion definitions, fonts and font licenses reused byte for byte")
    # Layering checks are deliberate: porting the core never drags host headers in.
    forbidden = ("windows.h", "runtime.h", "hook.h", "yuzu", "Vulkan", "WebView2")
    for p in [extension / "core/delay_engine.cpp", extension / "include/delay_api.h"]:
        text = p.read_text(encoding="utf-8-sig")
        assert not any(word in text for word in forbidden), p
    for p in (extension / "ui").rglob("*"):
        if p.is_file():
            text = p.read_text(encoding="utf-8-sig")
            assert not any(word in text for word in ("#include \"runtime.h\"", "#include \"hook.h\"", "#include \"yuzu_net.h\"", "0x71016")), p
    print("PASS core/frontend dependency boundaries")
    subprocess.run([str(frontend), "--selftest", "--assets", str(assets), "--data", str(work / "webview-profile")], check=True, timeout=25)
    bridge = work / "net_delay_frontend_bridge.exe"
    native_compile([ROOT / "tests/net_delay_frontend_bridge.cpp", extension / "core/delay_engine.cpp", extension / "adapter/bridge.cpp"],
                   bridge, [extension / "include", extension / "adapter"], ["bcrypt.lib", "advapi32.lib", "user32.lib"])
    subprocess.run([str(bridge), str(frontend), str(assets), str(work / "bridge-webview-profile")], check=True, timeout=40)
    rendered = work / "bridge-webview-profile/window-render.png"
    assert rendered.stat().st_size > 1000
    print("PASS page capture after hidden-start HWND and browser both become visible")
    # Missing assets must report a real load failure instead of a silent black box.
    missing_assets = work / "missing-assets"
    missing_assets.mkdir(exist_ok=True)
    failed = subprocess.run([str(frontend), "--selftest", "--assets", str(missing_assets),
                             "--data", str(work / "failed-webview-profile")], timeout=25)
    assert failed.returncode == 2, failed.returncode
    error_log = (work / "failed-webview-profile/frontend.log").read_text(encoding="utf-8-sig")
    assert "page navigation failed" in error_log
    print("PASS missing page assets fail explicitly and write frontend diagnostics")
    previews = OUT / "previews"
    previews.mkdir(exist_ok=True)
    for theme, flags, name in (("dark", ["--expanded", "--applied"], "panel-dark"),
                               ("light", ["--expanded", "--applied"], "panel-light"),
                               ("dark", ["--expanded"], "panel-waiting")):
        subprocess.run([str(frontend), "--preview", *flags, "--theme", theme, "--assets", str(assets),
                        "--data", str(work / "preview-webview-profile"), "--capture", str(previews / (name + ".png"))], check=True, timeout=25)
        assert (previews / (name + ".png")).stat().st_size > 1000
    print(f"PASS frontend DOM controls and theme/state captures: {previews}")


if __name__ == "__main__":
    main()

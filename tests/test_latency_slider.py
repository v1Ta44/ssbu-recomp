"""Exercise the actual generated calculator through the native plugin, without a game or GPU."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from build_latency_slider import OUT, clean_environment, load_base, verify_elf, compile_plugin
from play_latency_slider import parse_frames


class ArgumentTests(unittest.TestCase):
    def test_frames(self):
        for text in ("auto", "0", "25", "12"):
            self.assertEqual(parse_frames(text), text)
        for text in ("", "-1", "26", "1.5", "Auto", " 2", "1x", "nan"):
            with self.assertRaises(argparse.ArgumentTypeError):
                parse_frames(text)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, required=True)
    args = parser.parse_args()
    base, state = load_base(args.base)
    elf = base / "build/binaries/13.0.4/main.elf"
    verify_elf(elf)
    work = OUT / "native-test"
    work.mkdir(parents=True, exist_ok=True)
    # Extract locally generated game content into an ignored test directory only.
    source = (base / "build/work/gen_eh_o2/c_01e6.c").read_text()
    match = re.search(r"^void f_71016ccae0\(Ctx \*c\) \{.*?(?=\nvoid f_|\Z)", source, re.M | re.S)
    if not match:
        raise RuntimeError("Expected generated calculator was not found")
    generated = match.group(0).strip() + "\n"
    (work / "latency_target.generated.h").write_text(generated)
    plugin = compile_plugin(state["tools"]["clang"], work / "plugins")
    exe = work / "latency_slider_native.exe"
    subprocess.run([state["tools"]["clang"], "-x", "c++", "-std=c++14", "-O1", "-fno-strict-aliasing", "-w",
                    "-I", str(ROOT / "runtime"), "-I", str(ROOT / "fighter_sdk/include"),
                    "-I", str(ROOT / "mods/latency_slider"), "-I", str(work),
                    str(ROOT / "tests/latency_slider_native.cpp"), "-luser32", "-o", str(exe)], check=True)
    env = clean_environment()
    env["SSBU_NET_DELAY_UI"] = "off"
    env["SSBU_NET_DELAY_CSV"] = str(work / "native.csv")
    for index, extra in enumerate(([], ["--bad-fingerprint"])):
        env["SSBU_NET_DELAY_CSV"] = str(work / f"case-{index}.csv")
        subprocess.run([str(exe), str(plugin), str(elf), *extra], check=True, env=env, timeout=20)
    missing = work / "latency_slider_no_context.exe"
    subprocess.run([state["tools"]["clang"], "-x", "c++", "-std=c++14", "-O1", "-fno-strict-aliasing", "-w",
                    "-DNO_LDN_EXPORT", "-I", str(ROOT / "runtime"), "-I", str(ROOT / "fighter_sdk/include"),
                    "-I", str(ROOT / "mods/latency_slider"), "-I", str(work),
                    str(ROOT / "tests/latency_slider_native.cpp"), "-luser32", "-o", str(missing)], check=True)
    env["SSBU_NET_DELAY_CSV"] = str(work / "missing-context.csv")
    subprocess.run([str(missing), str(plugin), str(elf), "--no-context"], check=True, env=env, timeout=10)
    env["SSBU_NET_DELAY_CSV"] = str(work / "parsing.csv")
    for text, expected in ((None, -1), ("auto", -1), ("0", 0), ("25", 25), ("-1", -1),
                           ("26", -1), ("2oops", -1), ("", -1), ("9999999999999999999999999999999", -1)):
        current = env.copy()
        if text is not None:
            current["SSBU_NET_DELAY"] = text
        result = subprocess.run([str(exe), str(plugin), str(elf), "--parse"], check=True, env=current,
                                capture_output=True, text=True, timeout=10)
        if f"selected={expected}" not in result.stdout:
            raise RuntimeError((text, result.stdout, result.stderr))
    settings = work / "adapter-choice.ini"
    current = env.copy()
    current["SSBU_NET_DELAY_CONFIG"] = str(settings)
    subprocess.run([str(exe), str(plugin), str(elf), "--save-choice"], check=True, env=current, timeout=10)
    result = subprocess.run([str(exe), str(plugin), str(elf), "--parse"], check=True, env=current, capture_output=True, text=True, timeout=10)
    assert "selected=3" in result.stdout
    current["SSBU_NET_DELAY"] = "auto"
    result = subprocess.run([str(exe), str(plugin), str(elf), "--parse"], check=True, env=current, capture_output=True, text=True, timeout=10)
    assert "selected=-1" in result.stdout
    assert "frames=3" in settings.read_text()
    print("PASS actual adapter saves/restores selection; per-launch override does not overwrite saved choice")
    print("PASS native configuration parsing and fail-closed version validation")
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(ArgumentTests)
    if not unittest.TextTestRunner().run(suite).wasSuccessful():
        raise RuntimeError("Argument validation failed")


if __name__ == "__main__":
    main()

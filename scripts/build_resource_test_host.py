"""Build only the manual resource-test host; never lift, link or rewrite game inputs.

The runtime snapshot can include local fixes outside the resource commit. Preserve
its directory structure: r1src wrappers must never replace root runtime sources.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
HOST = ROOT / "build/mod-adapt/host"


def write_changed(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists() or path.read_bytes() != data:
        # Generated inputs may be hardlinks from an earlier local staging script.
        # Replace the directory entry rather than writing through to another build.
        temporary = path.with_name(path.name + ".resource-stage.tmp")
        temporary.write_bytes(data)
        os.replace(temporary, path)


def stage_runtime(ref, baseline=None):
    # These are the files whose current working copies supply the resource fixes
    # and observation hooks. Other runtime files come from the chosen baseline.
    names = ("nvn_gl.c", "nvn_front.h", "nvn_stub.c", "nnsdk.c", "nnsdk2.c",
             "nvn_lifetime.c", "nvn_lifetime.h")
    staged = set()
    if baseline is not None:
        manifest = json.loads((baseline / "resource-host.build.json").read_text(encoding="utf-8"))
        actual = hashlib.sha256((baseline / "ssbu_mod_adapt.exe").read_bytes()).hexdigest()
        if actual != manifest["host_sha256"]:
            raise RuntimeError("Baseline host no longer matches its build fingerprint")
        for relative, expected in manifest["runtime_sources"].items():
            source = (baseline / relative).resolve()
            if not source.is_relative_to(baseline):
                raise RuntimeError("Baseline source escapes its directory")
            data = source.read_bytes()
            if hashlib.sha256(data).hexdigest() != expected:
                raise RuntimeError("Baseline source changed: " + relative)
            write_changed(HOST / relative, data)
            staged.add(relative)
    else:
        paths = subprocess.check_output(
            ["git", "ls-tree", "-r", "--name-only", ref, "runtime"],
            cwd=ROOT, encoding="utf-8").splitlines()
        for relative in paths:
            name = Path(relative).relative_to("runtime").as_posix()
            data = subprocess.check_output(["git", "show", ref + ":" + relative], cwd=ROOT)
            write_changed(HOST / name, data)
            staged.add(name)
    for name in names:
        write_changed(HOST / name, (ROOT / "runtime" / name).read_bytes())
        staged.add(name)
    return staged


def main():
    global HOST
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime-ref", default="HEAD")
    parser.add_argument("--host-dir", type=Path, default=HOST,
                        help="build a separate host without replacing the validated one")
    parser.add_argument("--baseline-host", type=Path,
                        help="copy and verify the source manifest of a validated resource-test host")
    args = parser.parse_args()
    HOST = (ROOT / args.host_dir).resolve()
    baseline = (ROOT / args.baseline_host).resolve() if args.baseline_host else None
    if baseline == HOST:
        raise RuntimeError("Separate baseline and destination host directories are required")
    baseline_hash = hashlib.sha256((baseline / "ssbu_mod_adapt.exe").read_bytes()).hexdigest() if baseline else None
    state = json.loads((ROOT / "build/state.json").read_text(encoding="utf-8"))
    inputs = [Path(state["dump"]["romfs"]) / "data.arc",
              ROOT / "build/work/ssbu_game_eh_o1.dll",
              ROOT / "build/binaries/13.0.4/main.elf"]
    before = {str(p): (p.stat().st_size, p.stat().st_mtime_ns) for p in inputs}
    HOST.mkdir(parents=True, exist_ok=True)
    runtime_paths = stage_runtime(args.runtime_ref, baseline)
    for p in (ROOT / "tools/lifter").glob("*.py"):
        write_changed(HOST / p.name, p.read_bytes())
    for p in (ROOT / "build/work").iterdir():
        if p.is_file() and (p.suffix in (".h", ".inc") or p.name in (
                "sdkdata.c", "sdklift_gen.c", "nro_registry.c", "ssbu_game_eh_o1.dll",
                "ssbu_game_eh_o1.lib", "ssbu_glslang.dll", "opus.dll")):
            if p.suffix in (".dll", ".lib"):
                if not (HOST / p.name).exists():
                    os.link(p, HOST / p.name)
            elif p.name not in runtime_paths:
                write_changed(HOST / p.name, p.read_bytes())
    shutil.copytree(ROOT / "build/work/fighter_sdk", HOST / "fighter_sdk", dirs_exist_ok=True)
    backend = baseline or ROOT / "build/mod-adapt/release-host"
    for name in ("ssbu_mod_backend.dll", "ssbu_mod_backend.build.json"):
        write_changed(HOST / name, (backend / name).read_bytes())
    tools = state["tools"]
    env = {k: v for k, v in os.environ.items() if not k.startswith(("RECOMP_", "SSBU_"))}
    env.update(PYTHONIOENCODING="utf-8", RECOMP_CLANG=tools["clang"],
               SDL3_ROOT=tools["sdl3"], VULKAN_HEADERS=tools["vulkan"],
               SSBU_NETWORK_DEPS=str(ROOT / "deps"),
               RECOMP_GEN=str(ROOT / "build/work/gen_eh_o2"), RECOMP_EXE="ssbu_mod_adapt.exe")
    # Resolve headers with the same include search as the actual compiler. This
    # catches an escaping wrapper even if all object timestamps appear current.
    includes = ["-I", str(HOST), "-I", env["RECOMP_GEN"],
                "-I", str(Path(tools["sdl3"]) / "include"),
                "-I", str(Path(tools["vulkan"]) / "include")]
    dependencies = {}
    for name in ("nvn_gl.c", "nvn_vk.c"):
        result = subprocess.run([tools["clang"], "-M", "-MT", "runtime", *includes, str(HOST / name)],
                                cwd=HOST, env=env, capture_output=True, encoding="utf-8", errors="replace", check=True)
        text = result.stdout.replace("\\\n", " ").replace("\\", "/").lower()
        expected = (HOST / "nvn_front.h").as_posix().lower()
        if expected not in text or "/build/work/nvn_front.h" in text:
            raise RuntimeError(name + " resolves a foreign texture layout")
        if name == "nvn_vk.c" and (HOST / "nvn_vk_draw.c").as_posix().lower() not in text:
            raise RuntimeError("Vulkan renderer escapes the staged source tree")
        dependencies[name] = result.stdout
    subprocess.run([sys.executable, str(HOST / "build.py"), "link", "--host-only", "--with-mods"],
                   cwd=HOST, env=env, check=True)
    assert before == {str(p): (p.stat().st_size, p.stat().st_mtime_ns) for p in inputs}, "game inputs changed"
    if baseline:
        assert baseline_hash == hashlib.sha256((baseline / "ssbu_mod_adapt.exe").read_bytes()).hexdigest(), "baseline host changed"
    manifest = {"baseline_host": str(baseline) if baseline else None,
                "baseline_host_sha256": baseline_hash, "lifetime_trace_version": 1,
                "runtime_overrides": ["nvn_gl.c", "nvn_front.h", "nvn_stub.c", "nnsdk.c", "nnsdk2.c", "nvn_lifetime.c", "nvn_lifetime.h"],
                "runtime_ref": subprocess.check_output(["git", "rev-parse", args.runtime_ref], cwd=ROOT, encoding="utf-8").strip(),
                "dependencies": dependencies,
                "host_sha256": hashlib.sha256((HOST / "ssbu_mod_adapt.exe").read_bytes()).hexdigest(),
                "runtime_sources": {p.relative_to(HOST).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                                    for p in sorted(HOST.rglob("*")) if p.is_file() and p.suffix in (".c", ".h", ".inc")
                                    and "fighter_sdk" not in p.parts}}
    (HOST / "resource-host.build.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    print("PASS runtime header/source dependencies agree; original game inputs unchanged")
    return 0


if __name__ == "__main__":
    sys.exit(main())

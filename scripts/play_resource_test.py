"""Launch a prepared GameBanana test group for manual play; no scripted input."""
import argparse
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / "build/mod-adapt/gamebanana"
HOST = ROOT / "build/mod-adapt/host"
CASES = json.loads((BASE / "cases.json").read_text(encoding="utf-8"))


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def backend_snapshot(source, destination, update):
    """Use a verified built backend pair; never mutate the regular test host."""
    metadata = (source / "ssbu_mod_backend.build.json").read_bytes()
    stamp = json.loads(metadata.decode("utf-8"))
    payload = (source / "ssbu_mod_backend.dll").read_bytes()
    if hashlib.sha256(payload).hexdigest() != stamp["dll_sha256"]:
        raise SystemExit("Backend DLL does not match its build fingerprint; finish the backend build before testing.")
    changed = False
    if source.resolve() != destination.resolve():
        for name, data in (("ssbu_mod_backend.dll", payload), ("ssbu_mod_backend.build.json", metadata)):
            target = destination / name
            if target.is_file() and target.read_bytes() == data:
                continue
            changed = True
            if update:
                temporary = target.with_name(target.name + f".lifetime-{os.getpid()}.tmp")
                temporary.write_bytes(data)
                os.replace(temporary, target)
    return stamp, changed


def main():
    global HOST
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("case", choices=CASES)
    parser.add_argument("--language", choices=["saved", "en-US", "zh-Hans"], default="saved")
    parser.add_argument("--host-dir", type=Path, default=HOST)
    parser.add_argument("--lifetime-trace", action="store_true",
                        help="use separate diagnostic evidence/save folders and record resource lifetimes")
    parser.add_argument("--describe", action="store_true", help="show the launch configuration without starting the game")
    args = parser.parse_args()
    HOST = (ROOT / args.host_dir).resolve()
    regular = BASE / "runs" / args.case
    out = BASE / "runs/lifetime" / args.case if args.lifetime_trace else regular
    ud = out / "userdata"
    save_source = regular / "userdata" if args.lifetime_trace and (regular / "userdata").is_dir() else ROOT / "userdata"
    if args.lifetime_trace:
        manifest = json.loads((HOST / "resource-host.build.json").read_text(encoding="utf-8"))
        if manifest.get("lifetime_trace_version") != 1:
            raise SystemExit("Use the lifecycle diagnostic entry with its separately built host")
        if digest(HOST / "ssbu_mod_adapt.exe") != manifest["host_sha256"]:
            raise SystemExit("Diagnostic host does not match its build fingerprint")
    state = json.loads((ROOT / "build/state.json").read_text(encoding="utf-8"))
    profile = regular / "mods.json"
    backend_source = Path(manifest["baseline_host"]) if args.lifetime_trace and manifest.get("baseline_host") else HOST
    stamp, backend_sync = backend_snapshot(backend_source, HOST, update=args.lifetime_trace and not args.describe)
    env = {k: v for k, v in os.environ.items() if not k.startswith(("RECOMP_", "SSBU_"))}
    env.update(
        SSBU_ROMFS=state["dump"]["romfs"],
        SSBU_USERDATA=str(ud),
        SSBU_NRO_BIN=os.path.relpath(ROOT / "build/work/nro_bin_eh_o2", HOST),
        SSBU_SHOW="1",
        SSBU_VOLUME="100",  # Test-copy names default to silent even with SSBU_SHOW=1.
        SSBU_RENDERER="vk",
        SSBU_CONFIG=str(ROOT / "build/mod-adapt/config/controllers.ini"),
        SSBU_GRAPHICS_CONFIG=str(ROOT / "build/mod-adapt/config/graphics.ini"),
        SSBU_VK_CACHE=str(ROOT / "build/cache/vk"),
        SSBU_VK_CONTAINERS=str(ROOT / "build/cache/vk_containers2"),
        SSBU_RESOURCE_MODS="0" if args.case == "baseline" else str(BASE / "packs"),
        SSBU_MOD_PROFILE=str(profile),
        SSBU_MOD_CACHE=str(BASE / "cache"),
    )
    for sub, var in [("fonts", "SSBU_FONTS"), ("zoneinfo", "SSBU_TZDIR")]:
        path = ROOT / "build/sysdata" / sub
        if path.is_dir():
            env[var] = str(path)
    if args.language != "saved":
        env["SSBU_LANGUAGE"] = args.language
        env["SSBU_MOD_REGION"] = "us_en" if args.language == "en-US" else "zh_cn"
    elf = ROOT / "build/binaries/13.0.4/main.elf"
    command = [str(HOST / "ssbu_mod_adapt.exe"), str(elf), "0"]
    capture_fatal = args.case == "skin" or args.lifetime_trace
    description = {"backend_source": str(backend_source), "backend_sync_required": backend_sync, "lifetime_trace_requested": args.lifetime_trace, "save_source": str(save_source), "userdata": str(ud), "evidence_root": str(out), "exception_capture": "child-debug-events-v1" if capture_fatal else "runtime-handler", "case": args.case, "packages": CASES[args.case], "command": command,
                   "environment": {k: v for k, v in env.items() if k.startswith("SSBU_")},
                   "backend": stamp}
    if args.describe:
        print(json.dumps(description, ensure_ascii=True, indent=2))
        return 0
    # Create each group's save only once; never reset a tester's progress on relaunch.
    if not ud.exists():
        shutil.copytree(save_source, ud, ignore=shutil.ignore_patterns("*.work.*", "*.journal.*"))
    session = out / datetime.now().strftime("%Y%m%d-%H%M%S-%f")
    shots = session / "shots"
    shots.mkdir(parents=True)
    env.update(SSBU_TRACE=str(session / "trace.txt"), SSBU_SHOT_DIR=str(shots),
               SSBU_SHOT_FIGHT="120,600", SSBU_REPORT_DIR=str(session))
    if args.lifetime_trace:
        env["SSBU_NVN_LIFETIME"] = str(session / "nvn-lifetime.bin")
    description["environment"] = {k: v for k, v in env.items() if k.startswith("SSBU_")}
    description["host_sha256"] = digest(HOST / "ssbu_mod_adapt.exe")
    inputs = [Path(state["dump"]["romfs"]) / "data.arc", elf, ROOT / "build/work/ssbu_game_eh_o1.dll"]
    description["game_inputs_before"] = {str(p): {"size": p.stat().st_size, "mtime_ns": p.stat().st_mtime_ns} for p in inputs}
    description["fixes_snapshot"] = (ROOT / "build/mod-adapt/retest-fixes.txt").read_text().strip()
    (session / "launch.json").write_text(json.dumps(description, ensure_ascii=False, indent=2), encoding="utf-8")
    print("Manual test group:", args.case, flush=True)
    print("Packages:", ", ".join(CASES[args.case]) or "vanilla baseline", flush=True)
    print("Evidence folder:", session, flush=True)
    captured = []
    if capture_fatal:
        from resource_test_debug import WindowsDebugCapture
        monitor = WindowsDebugCapture(session)
        # A new diagnostic session keeps its matching host symbols before launch.
        # Copies are local test artifacts and contain no game input files.
        symbols = session / "host-symbols"
        symbols.mkdir()
        for name in ("ssbu_mod_adapt.exe", "ssbu_mod_adapt.map", "ssbu_mod_adapt.pdb",
                     "ssbu_mod_backend.dll", "ssbu_mod_backend.build.json", "resource-host.build.json"):
            source = HOST / name
            if source.is_file():
                shutil.copy2(source, symbols / name)
        print("Diagnostic launch: fatal exception capture enabled", flush=True)
        with (session / "game.log").open("wb") as log:
            code = monitor.run(command, cwd=HOST, env=env, log=log,
                on_start=lambda pid: (session / "pid.txt").write_text(str(pid)))
        captured = monitor.exceptions
    else:
        with (session / "game.log").open("wb") as log:
            process = subprocess.Popen(command, cwd=HOST, env=env, stdout=log, stderr=subprocess.STDOUT)
            (session / "pid.txt").write_text(str(process.pid))
            code = process.wait()
    lifetime = {"requested": args.lifetime_trace, "created": False}
    if args.lifetime_trace:
        trace = session / "nvn-lifetime.bin"
        lifetime["created"] = trace.is_file()
        if trace.is_file():
            from analyze_nvn_lifetime import analyze
            try:
                result = analyze(trace)
                (session / "nvn-lifetime-analysis.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
                lifetime["findings"] = len(result["findings"])
            except (OSError, ValueError) as error:
                lifetime["analysis_error"] = str(error)
        else:
            print("Lifecycle trace was not produced; preserve this session for diagnosis", flush=True)
    after = {str(p): {"size": p.stat().st_size, "mtime_ns": p.stat().st_mtime_ns} for p in inputs}
    (session / "result.json").write_text(json.dumps({"exit_code": code, "game_inputs_after": after,
        "game_inputs_unchanged": after == description["game_inputs_before"],
        "fatal_exceptions": captured, "lifetime_trace": lifetime}, indent=2), encoding="utf-8")
    print("Game exit:", code, "Evidence:", session, flush=True)
    return code


if __name__ == "__main__":
    sys.exit(main())

"""Open the modular delay backend and Console-style in-game panel (saved choice or Auto)."""
import argparse
from datetime import datetime
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from build_latency_slider import OUT, clean_environment, digest, verify_elf


def parse_frames(text):
    if text == "auto":
        return text
    if not re.fullmatch(r"[0-9]+", text) or not 0 <= int(text) <= 25:
        raise argparse.ArgumentTypeError("Choose auto or an integer from 0 to 25")
    return str(int(text))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--frames", type=parse_frames, help="Override saved choice for this launch: auto or 0..25")
    parser.add_argument("--smoke-seconds", type=int, default=0, help="Hidden offline startup smoke; no server connection")
    parser.add_argument("--overlay-smoke", action="store_true", help="Also exercise the real hidden Console frontend and local IPC")
    args = parser.parse_args()
    if args.overlay_smoke and not args.smoke_seconds:
        parser.error("--overlay-smoke requires --smoke-seconds")
    manifest = json.loads((OUT / "build.json").read_text(encoding="utf-8"))
    for relative, expected in (manifest["artifacts_sha256"] | manifest["module_sha256"]).items():
        if digest(OUT / relative) != expected:
            raise RuntimeError(f"Build artifact changed; rebuild before using it: {relative}")
    base = Path(manifest["base"])
    state = json.loads((base / "build/state.json").read_text(encoding="utf-8-sig"))
    elf = base / "build/binaries/13.0.4/main.elf"
    verify_elf(elf)
    if args.smoke_seconds < 0 or args.smoke_seconds > 120:
        parser.error("Smoke duration must be 0..120 seconds")
    userdata = OUT / "userdata"
    if not userdata.exists():
        source = base / "build/userdata_yuzu"
        if source.is_dir():
            shutil.copytree(source, userdata)
        else:
            userdata.mkdir(parents=True)
    config = OUT / "config"
    config.mkdir(parents=True, exist_ok=True)
    graphics = config / "graphics.ini"
    if not graphics.exists():
        source = base / "build/config/graphics.ini"
        text = source.read_text(encoding="utf-8-sig") if source.exists() else ""
        text = re.sub(r"(?im)^\s*runahead\s*=.*$", "runahead = off", text)
        graphics.write_text(text + "\nrunahead = off\n", encoding="utf-8")
    for name in ("controllers.ini",):
        source = base / "build/config" / name
        if source.exists() and not (config / name).exists():
            shutil.copy2(source, config / name)
    logs = OUT / "runs" / datetime.now().strftime("%Y%m%d-%H%M%S-%f")
    logs.mkdir(parents=True)
    env = clean_environment()
    env.update(SSBU_ROMFS=state["dump"]["romfs"], SSBU_USERDATA=str(userdata), SSBU_NICKNAME="vitas",
               SSBU_NRO_BIN="nro_bin_eh_o2", SSBU_CONFIG=str(config / "controllers.ini"),
               SSBU_GRAPHICS_CONFIG=str(graphics), SSBU_RUNAHEAD="0", SSBU_MINIDUMP="1", SSBU_REPORT_DIR=str(logs),
               SSBU_MODS=str(Path(manifest["plugin"]).parent), SSBU_NET_DELAY_CSV=str(logs / "delay.csv"),
               SSBU_NET_DELAY_CONFIG=str(config / "online-delay.ini"), SSBU_NET_DELAY_UI_DATA=str(logs / "interface-cache"),
               SSBU_NET_DELAY_UI=("hidden" if args.overlay_smoke else "off") if args.smoke_seconds else "overlay", SSBU_NET_LOG_LEVEL="2",
               SSBU_TRACE=str(logs / "boot_trace.txt"))
    if args.frames is not None:
        env["SSBU_NET_DELAY"] = args.frames
    for sub, var in (("fonts", "SSBU_FONTS"), ("zoneinfo", "SSBU_TZDIR")):
        path = base / "build/sysdata" / sub
        if path.is_dir():
            env[var] = str(path)
    if args.smoke_seconds:
        env.update(SSBU_SHOW="0", SSBU_AUDIO_MUTE="1", SSBU_SKIP_MOVIES="1", SSBU_VK_PREWARM="0", SSBU_VK_PREPARE="off")
    else:
        env.update(SSBU_NET_UI="1", SSBU_SHOW="1", SSBU_NET_UI_LOG=str(logs / "game.log"))
    with (logs / "game.log").open("wb") as output:
        process = subprocess.Popen([manifest["host"], str(elf), str(args.smoke_seconds)], cwd=Path(manifest["host"]).parent,
                                   env=env, stdout=output, stderr=subprocess.STDOUT, creationflags=subprocess.CREATE_NO_WINDOW)
    (logs / "run.json").write_text(json.dumps(dict(pid=process.pid, frames=args.frames, smoke_seconds=args.smoke_seconds, overlay_smoke=args.overlay_smoke,
                                                    build=manifest["artifacts_sha256"], userdata=str(userdata)), indent=2), encoding="utf-8")
    print(f"Delay experiment PID: {process.pid}\nSelection: {args.frames or 'saved choice / Auto'}\nEvidence: {logs}")
    if args.smoke_seconds:
        try:
            code = process.wait(timeout=args.smoke_seconds + 45)
        except subprocess.TimeoutExpired:
            process.terminate()
            process.wait(timeout=10)
            raise RuntimeError("Startup smoke timed out; see the evidence directory")
        print(f"Smoke exit code: {code}")
        if code:
            raise RuntimeError("Startup smoke did not exit successfully")
        text = (logs / "game.log").read_text(encoding="utf-8", errors="replace")
        if "net-delay: 13.0.4 calculator entry" not in text:
            raise RuntimeError("Startup smoke never installed the verified hook")
        if args.overlay_smoke and "Console frontend connected over local control API" not in text:
            raise RuntimeError("Console frontend never connected to the backend")
        print("PASS hidden offline startup and verified hook installation; no online gameplay claim")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, RuntimeError) as error:
        print(f"Cannot launch delay experiment: {error}", file=sys.stderr)
        sys.exit(1)

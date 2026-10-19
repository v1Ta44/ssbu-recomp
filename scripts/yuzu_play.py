"""Open the in-process Yuzu control panel; the user joins a room then starts SSBU.
Game identity must be explicitly configured; the avatar stays at its default. No credentials are persisted.
"""
import argparse
import hashlib
from datetime import datetime
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
try:
    from .player_name import game_name
except ImportError:
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    from player_name import game_name

ROOT = Path(__file__).resolve().parents[1]

def prepare_diagnostics(build, exe, userdata, folder):
    """Preserve the inputs needed to inspect a future crash before starting it."""
    binaries = folder / 'binaries'
    binaries.mkdir(parents=True)
    identities = {}
    for name in (exe.name, exe.stem + '.pdb', exe.stem + '.map', 'ssbu_game_eh_o1.dll'):
        source = exe.parent / name
        if source.is_file():
            target = binaries / name
            shutil.copy2(source, target)
            with target.open('rb') as stream:
                identities[name] = hashlib.file_digest(stream, 'sha256').hexdigest()
    for name in ('save', 'cache'):
        source = userdata / name
        if source.is_dir():
            shutil.copytree(source, folder / 'userdata_before' / name)
    config = build / 'config'
    for name in ('graphics.ini', 'controllers.ini'):
        source = config / name
        if source.is_file():
            target = folder / 'config_before' / name
            target.parent.mkdir(exist_ok=True)
            shutil.copy2(source, target)
    manifest = dict(created=datetime.now().isoformat(), executable=str(exe),
                    userdata=str(userdata), binary_sha256=identities,
                    minidump='full', input_recording='input.rec: player-1 replay; pads.csv: observed inputs for all players, both sticks, not a replay file')
    (folder / 'manifest.json').write_text(json.dumps(manifest, indent=2), encoding='utf-8')
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--nickname', help='Required game identity, or use the name saved on the Console Start page')
    parser.add_argument('--userdata', type=Path, help='Existing test save directory; default: build/userdata_yuzu')
    parser.add_argument('--diagnostic', action='store_true', help='Save per-run evidence and a full memory dump if the game crashes')
    args = parser.parse_args()
    build = ROOT / 'build'
    try:
        player = game_name(build, args.nickname)
    except ValueError as error:
        parser.error(str(error))
    state = json.loads((build / 'state.json').read_text(encoding='utf-8'))
    exe = build / 'work' / ('ssbu_diagnostic.exe' if args.diagnostic else 'ssbu_native.exe')
    if not exe.is_file():
        raise RuntimeError('Build the host executable first: scripts/setup.py --only link')
    userdata = args.userdata.resolve() if args.userdata else build / 'userdata_yuzu'
    if not args.userdata and not userdata.exists() and (ROOT / 'userdata').is_dir():
        shutil.copytree(ROOT / 'userdata', userdata)
    userdata.mkdir(parents=True, exist_ok=True)
    (build / 'cache').mkdir(exist_ok=True)
    logs = build / 'logs'
    logs.mkdir(exist_ok=True)
    stamp = datetime.now().strftime('%Y%m%d-%H%M%S-%f')
    logfile = logs / ('yuzu-play-' + stamp + '.log')
    diagnostic = logs / ('yuzu-diagnostic-' + stamp) if args.diagnostic else None
    if diagnostic:
        prepare_diagnostics(build, exe, userdata, diagnostic)
        logfile = diagnostic / 'game.log'
    env = {k: v for k, v in os.environ.items() if not k.startswith(('RECOMP_', 'SSBU_'))}
    env.update(SSBU_ROMFS=state['dump']['romfs'], SSBU_USERDATA=str(userdata), SSBU_NICKNAME=player,
               SSBU_NET_UI='1', SSBU_NET_LOG_LEVEL='2', SSBU_NET_UI_LOG=str(logfile), SSBU_MINIDUMP='1')
    if diagnostic:
        env.update(SSBU_MINIDUMP='full', SSBU_REPORT_DIR=str(diagnostic), SSBU_SHOW='1',
                   SSBU_PAD_TRACE=str(diagnostic / 'pads.csv'),
                   SSBU_REC=str(diagnostic / 'input.rec'), SSBU_TRACE=str(diagnostic / 'boot_trace.txt'))
    for sub, var in (('fonts', 'SSBU_FONTS'), ('zoneinfo', 'SSBU_TZDIR')):
        path = build / 'sysdata' / sub
        if path.is_dir():
            env[var] = str(path)
    with logfile.open('wb') as stream:
        process = subprocess.Popen([str(exe), str(build / 'binaries' / '13.0.4' / 'main.elf'), '0'],
                                   cwd=build / 'work', env=env, stdout=stream, stderr=subprocess.STDOUT,
                                   creationflags=subprocess.CREATE_NO_WINDOW)
    if diagnostic:
        (diagnostic / 'process.json').write_text(json.dumps(dict(pid=process.pid)), encoding='utf-8')
        print(f'Diagnostic evidence: {diagnostic} (full dumps can take several GB)')
    print(f'Yuzu control panel opened. PID: {process.pid}\nProfile: {player}\nSaves: {userdata}\nLog: {logfile}')
    return 0

if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, RuntimeError) as error:
        print(f'Cannot open Yuzu control panel: {error}', file=sys.stderr)
        sys.exit(1)

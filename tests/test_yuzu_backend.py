"""Build/run the single loopback integration fixture, without a game or GPU.
Usage: .venv\Scripts\python.exe tests/test_yuzu_backend.py [--clang PATH]
"""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import re

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools' / 'lifter'))
import build_network

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--clang', default=str(ROOT / 'deps' / 'llvm' / 'bin' / 'clang.exe'))
    args = parser.parse_args()
    work = ROOT / 'build' / 'work'
    includes, objects = build_network.prepare(args.clang, work)
    out = work / 'yuzu_backend_test.exe'
    sources = [ROOT / 'tests' / 'yuzu_backend.c'] + [ROOT / 'runtime' / n for n in ('yuzu_net.c', 'yuzu_ldn.c', 'yuzu_socket.c')]
    subprocess.run([args.clang, '-O1', '-Wall', '-Wextra', '-Wno-deprecated-declarations', '-I', str(ROOT / 'runtime'),
                    *includes, *map(str, sources), *objects, '-lws2_32', '-lwinmm', '-ladvapi32', '-o', str(out)], check=True)
    # Exercise the actual native profile initializer in fresh processes, without a game.
    for value, expected in [(None, None), ('', None), ('   ', None), ('字'*11, None), ('udon','udon'), ('小明','小明'), ('a'*32,'a'*32)]:
        env = {k:v for k,v in os.environ.items() if k.upper() != 'SSBU_NICKNAME'}
        env['USERNAME'] = 'WINDOWS_NAME_MUST_NOT_BE_USED'
        if value is not None: env['SSBU_NICKNAME'] = value
        result = subprocess.run([str(out),'--profile-probe'], env=env, capture_output=True, timeout=3)
        assert result.returncode == (0 if expected is not None else 2), (value,result.returncode,result.stderr)
        assert result.stdout == (expected.encode('utf-8') if expected is not None else b''), (value,result.stdout)
    print('PASS explicit native identity; blank/oversized names never use the Windows account')
    # Reuse the existing protocol run to check useful logs and redaction.
    log = ROOT / 'build' / 'logs' / 'yuzu_backend_test.log'
    log.parent.mkdir(parents=True, exist_ok=True)
    with log.open('wb') as stream:
        subprocess.run([str(out)], check=True, timeout=15, stderr=stream)
    diagnostic = log.read_text(encoding='utf-8')
    for expected in ('state=joining->joined', 'Scan end', 'Connect end',
                     'zstd decode failed', 'disconnect event', 'lifecycle survives trace limit'):
        assert expected in diagnostic, f'missing diagnostic: {expected}'
    for secret in ('SECRET_TOKEN', 'avatar.png', 'payload', 'MUST_NOT_LOG'):
        assert secret not in diagnostic, f'unexpected log content: {secret}'
    assert re.search(r'suppressed debug=\d+ trace=[1-9]\d*', diagnostic)
    print(f'PASS diagnostic levels, rate limit and redaction; log: {log}')

if __name__ == '__main__':
    main()

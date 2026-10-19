#!/usr/bin/env python3
"""Make a test package of the built game: one folder that starts on another Windows PC without Python or build tools,
with a launcher that gives every run its own log folder and a report zip (tools/package/launch.ps1, README.txt).

    python scripts/package.py                     build/dist/ssbu-<date>-<commit>-<build id>/ ; data.arc hard-linked when the
                                                  output is on the same drive as the dump, otherwise copied
    python scripts/package.py --romfs none        without data.arc (on the other PC: romfs = <folder> in launcher/launcher.ini)
    python scripts/package.py --save <userdata folder> --cache all --zip

What goes in: the exe with its .pdb and .map, the game DLL, the script-module DLLs, SDL3 / glslang / opus and the MSVC
runtime DLLs they need, main.elf / main.npdm / sdk.elf, fonts and the DLC list, the shader-container store (cache/), the
launcher, the input sequence and save of the smoke test. manifest.json lists every file with size and sha256 and the
build id (a hash over the binaries), which every log, report and replay carries.

Every binary is also put in a symbol store (build/dist/symstore, the layout of a Microsoft symbol server: images by link
time stamp + image size, the .pdb by its GUID), so a minidump of any packaged build opens on this PC with
    WinDbg:  .sympath srv*<repo>\\build\\dist\\symstore   (and .exepath the same)      Visual Studio: add the folder as a symbol location
without looking for the package it came from. The package's files are hard links into the store (no second copy on disk).
"""
import argparse
import datetime
import glob
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import sys
import time
import zipfile

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TEMPLATES = os.path.join(REPO, 'tools', 'package')
VC_DLLS = ('msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll')       # imported by ssbu_glslang.dll and opus.dll


def pe_info(path):
    """Link time stamp, image size and the CodeView record (pdb GUID + age + name) of a PE file: the symbol-store keys."""
    with open(path, 'rb') as f:
        head = f.read(4096)
        lfanew = struct.unpack_from('<I', head, 0x3c)[0]
        if head[lfanew:lfanew + 4] != b'PE\0\0':
            return None
        _, nsec, stamp, _, _, opt_size, _ = struct.unpack_from('<HHIIIHH', head, lfanew + 4)
        opt = lfanew + 24
        pe32plus = struct.unpack_from('<H', head, opt)[0] == 0x20b
        size_of_image = struct.unpack_from('<I', head, opt + 56)[0]
        dirs = opt + (112 if pe32plus else 96)
        dbg_rva, dbg_size = struct.unpack_from('<II', head, dirs + 6 * 8)
        info = {'stamp': '%08x' % stamp, 'size_of_image': size_of_image, 'image_key': '%08X%x' % (stamp, size_of_image)}
        sections = [struct.unpack_from('<8sIIII', head, opt + opt_size + 40 * i) for i in range(nsec)]
        for _, vsize, va, rawsize, rawptr in sections:
            if dbg_rva and va <= dbg_rva < va + max(vsize, rawsize):
                f.seek(rawptr + dbg_rva - va)
                d = f.read(dbg_size)
                for k in range(0, len(d) - 27, 28):
                    typ, size_of_data, _, ptr = struct.unpack_from('<IIII', d, k + 12)
                    if typ == 2:                                            # IMAGE_DEBUG_TYPE_CODEVIEW
                        f.seek(ptr)
                        cv = f.read(size_of_data)
                        if cv[:4] == b'RSDS':
                            d1, d2, d3 = struct.unpack_from('<IHH', cv, 4)
                            age = struct.unpack_from('<I', cv, 20)[0]
                            info['pdb'] = os.path.basename(cv[24:].split(b'\0')[0].decode('utf-8', 'replace').replace('/', '\\').split('\\')[-1])
                            info['pdb_key'] = '%08X%04X%04X%s%x' % (d1, d2, d3, cv[12:20].hex().upper(), age)
                break
        return info


def sha256(path, progress=None):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for block in iter(lambda: f.read(1 << 22), b''):
            h.update(block)
    return h.hexdigest()


def copy_big(src, dst):
    """shutil.copy2 with a progress line (data.arc is 17 GB)."""
    total, done, shown = os.path.getsize(src), 0, -1
    with open(src, 'rb') as a, open(dst, 'wb') as b:
        for block in iter(lambda: a.read(1 << 24), b''):
            b.write(block)
            done += len(block)
            pct = done * 100 // max(total, 1)
            if pct // 5 != shown:
                shown = pct // 5
                print('\r  copying %s: %d%%' % (os.path.basename(src), pct), end='', flush=True)
    print()
    shutil.copystat(src, dst)


def link_or_copy(src, dst):
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    try:
        os.link(src, dst)
        return 'link'
    except OSError:
        (copy_big if os.path.getsize(src) > 256 << 20 else shutil.copy2)(src, dst)
        return 'copy'


def find_vc_runtime():
    """The redistributable MSVC runtime of the newest Visual Studio (may be shipped beside an application), else System32."""
    pats = [os.path.join(os.environ.get(v, ''), 'Microsoft Visual Studio', '*', '*', 'VC', 'Redist', 'MSVC', '*', 'x64', 'Microsoft.VC14*.CRT')
            for v in ('ProgramFiles', 'ProgramFiles(x86)')]
    dirs = sorted((d for p in pats for d in glob.glob(p) if all(os.path.isfile(os.path.join(d, n)) for n in VC_DLLS)),
                  key=lambda d: [int(x) if x.isdigit() else 0 for x in d.split(os.sep)[-3].split('.')])
    if dirs:
        return dirs[-1]
    sys32 = os.path.join(os.environ.get('SystemRoot', r'C:\Windows'), 'System32')
    return sys32 if all(os.path.isfile(os.path.join(sys32, n)) for n in VC_DLLS) else None


def git(*args):
    r = subprocess.run(['git', '-C', REPO] + list(args), capture_output=True, text=True, encoding='utf-8', errors='replace')
    return r.stdout.strip() if r.returncode == 0 else ''


def host_features(exe):
    """Read the feature of this exact binary, never guess from a leftover backend DLL."""
    path = os.path.splitext(exe)[0] + '.build.json'
    try:
        with open(path, encoding='utf-8') as file:
            stamp = json.load(file)
        if (stamp.get('schema_version') not in (1, 2) or type(stamp.get('resource_mods')) is not bool
                or (stamp.get('schema_version') == 2 and type(stamp.get('latency_slider')) is not bool)
                or stamp.get('exe_sha256') != sha256(exe)):
            raise ValueError('invalid feature record or executable hash mismatch')
    except (OSError, ValueError, AttributeError) as e:
        raise SystemExit('%s: %s; relink with scripts/setup.py --only link --with-mods (or --without-mods)' % (path, e))
    return {'resource_mods': stamp['resource_mods'], 'latency_slider': stamp.get('latency_slider', False)}


def latency_files(work, exe, enabled):
    """Only package exact extension artifacts bound to this host; ignore leftovers when disabled."""
    if not enabled:
        return []
    try:
        from .build_latency_slider import source_fingerprint
    except ImportError:
        from build_latency_slider import source_fingerprint
    base = Path(work).resolve()
    stamp_path = base / 'net_delay/build.json'
    try:
        with open(os.path.splitext(exe)[0] + '.build.json', encoding='utf-8') as file:
            host = json.load(file)
        with stamp_path.open(encoding='utf-8') as file:
            stamp = json.load(file)
        if host.get('latency_slider_sha256') != sha256(stamp_path):
            raise ValueError('extension build record differs from the linked host')
        if stamp.get('schema_version') != 1 or stamp.get('source_sha256') != source_fingerprint():
            raise ValueError('extension sources changed')
        artifacts = stamp['artifacts_sha256']
        required = {'plugins/latency_slider.dll', 'net_delay/delay_panel.exe', 'net_delay/WebView2-LICENSE.txt',
                    'net_delay/ui/index.html', 'net_delay/ui/panel.css', 'net_delay/ui/panel.js',
                    'net_delay/ui/console/style.css', 'net_delay/ui/console/fonts/OFL-Saira.txt',
                    'net_delay/ui/console/fonts/OFL-SmileySans.txt'}
        if not required.issubset(artifacts):
            raise ValueError('extension artifacts or licenses missing')
        files = []
        for relative, expected in artifacts.items():
            path = (base / relative).resolve()
            if (not path.is_relative_to(base) or not relative.startswith(('plugins/', 'net_delay/'))
                    or sha256(path) != expected):
                raise ValueError('extension artifact changed: ' + relative)
            files.append((str(path), 'game/' + relative))
        files.append((str(stamp_path), 'game/net_delay/build.json'))
        return files
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
        raise SystemExit('online delay extension: %s; rebuild with scripts/setup.py --only link --with-latency-slider' % error)


def check_build_is_current(work, exe, allow_stale, resource_mods=False):
    """The packaged exe must be linked from the runtime sources as they are in runtime/ (build/work holds copies)."""
    problems = []
    if resource_mods:
        try:
            from .build_mod_backend import source_fingerprint
        except ImportError:
            from build_mod_backend import source_fingerprint
        stamp_path = os.path.join(work, 'ssbu_mod_backend.build.json')
        try:
            with open(stamp_path, encoding='utf-8') as file:
                stamp = json.load(file)
            if stamp.get('source_sha256') != source_fingerprint(REPO):
                problems.append('the resource backend sources changed; run scripts/build_mod_backend.py')
            dll = os.path.join(work, 'ssbu_mod_backend.dll')
            if not os.path.isfile(dll) or stamp.get('dll_sha256') != sha256(dll):
                problems.append('the resource backend DLL does not match its build stamp')
        except (OSError, ValueError):
            problems.append('resource backend build stamp missing or invalid; run scripts/build_mod_backend.py')
    for src in sorted(glob.glob(os.path.join(REPO, 'runtime', '*'))):
        if not src.endswith(('.c', '.h', '.inc')):
            continue
        copy = os.path.join(work, os.path.basename(src))
        if not os.path.isfile(copy):
            continue
        with open(src, 'rb') as a, open(copy, 'rb') as b:
            if a.read() != b.read():
                problems.append('runtime/%s differs from build/work/%s (copy it and run the link step)' % (os.path.basename(src), os.path.basename(src)))
        if os.path.getmtime(copy) > os.path.getmtime(exe) + 1:
            problems.append('build/work/%s is newer than %s (run the link step)' % (os.path.basename(src), os.path.basename(exe)))
    for relative in ('fighter_sdk/hook_host.c', 'fighter_sdk/fighter_host.c', 'fighter_sdk/newmodule_host.c'):
        source, staged = os.path.join(REPO, relative), os.path.join(work, relative)
        if os.path.isfile(staged) and (Path(source).read_bytes() != Path(staged).read_bytes()
                                       or os.path.getmtime(staged) > os.path.getmtime(exe) + 1):
            problems.append('%s changed since the host build (run the setup link step)' % relative)
    for ext in ('.pdb', '.map'):
        side = exe[:-4] + ext
        if not os.path.isfile(side) or abs(os.path.getmtime(side) - os.path.getmtime(exe)) > 120:
            problems.append('%s is missing or not from the same link as the exe (tools/lifter/build.py writes it since 2026-10-15: run the link step)' % os.path.basename(side))
    if problems:
        print('\n'.join('  ' + p for p in problems))
        if not allow_stale:
            sys.exit('the build is not current; --allow-stale packages it anyway')


def zip_package(pkg, zpath, name):
    # Cargo's vendored license files can retain Unix-epoch timestamps. ZIP begins in 1980.
    with zipfile.ZipFile(zpath, 'w', zipfile.ZIP_DEFLATED, compresslevel=6, strict_timestamps=False) as z:
        for dp, dns, fns in os.walk(pkg):
            rel_dp = os.path.relpath(dp, pkg)
            for d in dns:
                if not os.listdir(os.path.join(dp, d)):
                    z.write(os.path.join(dp, d), os.path.join(name, rel_dp, d))
            for n in fns:
                if rel_dp == 'romfs' and n == 'data.arc':
                    continue
                z.write(os.path.join(dp, n), os.path.join(name, rel_dp, n))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--build', default=os.path.join(REPO, 'build'))
    ap.add_argument('--out', help='folder for the package and the symbol store (default <build>/dist)')
    ap.add_argument('--name', help='package folder name (default ssbu-<date>-<commit>-<build id>)')
    ap.add_argument('--exe', default='ssbu_native.exe', help='the exe in build/work (it is named ssbu_native.exe in the package: window and sound depend on the name)')
    ap.add_argument('--game-dll', default=os.environ.get('RECOMP_DLL', 'ssbu_game_eh_o1.dll'))
    ap.add_argument('--nro-bin', default='nro_bin_eh_o2', help='folder of the script-module DLLs in build/work')
    ap.add_argument('--romfs', choices=('auto', 'link', 'copy', 'none'), default='auto',
                    help='data.arc: auto = hard link when possible, else copy; none = leave it out')
    ap.add_argument('--cache', choices=('none', 'containers', 'all'), default='containers',
                    help='containers = the shader-container store (built from data.arc + main.elf, any GPU; saves the first start '
                         'building it); all = also the translated shaders (cache/vk spv, stage, pipes.bin: not tried on another GPU '
                         'yet; the driver pipeline cache is never included)')
    ap.add_argument('--save', default=REPO, help='a userdata folder whose save/ becomes the package\'s save (default: the repository, '
                                                 'i.e. <repo>/save; "none" = no save, the game starts fresh)')
    ap.add_argument('--smoke-save', default=None, help='userdata folder for SmokeTest.bat (default <build>/userdata; "none" = fresh save)')
    ap.add_argument('--config', action='store_true', help='include build/config/*.ini (default: the game writes its defaults on the first start)')
    ap.add_argument('--hash-romfs', action='store_true', help='also put the sha256 of data.arc in the manifest (reads 17 GB)')
    ap.add_argument('--zip', action='store_true', help='also write <package>.zip, without romfs (to hand the update of a build to another PC)')
    ap.add_argument('--console', default=os.path.join(REPO, 'build', 'console-target', 'release', 'ssbu-console.exe'),
                    help='the console exe (tools/console, cargo build --release); it becomes "SSBU Console.exe" at the package root')
    ap.add_argument('--public', action='store_true', help='portable public layout: no data.arc, caches, personal/test saves or local config; omit private manifest fields')
    ap.add_argument('--updater', default=os.path.join(REPO, 'build', 'updater-target', 'release', 'ssbu-updater.exe'), help='portable update/recovery helper')
    ap.add_argument('--force', action='store_true', help='replace a package folder of the same name')
    ap.add_argument('--allow-stale', action='store_true')
    a = ap.parse_args()
    if a.public:
        a.romfs, a.cache, a.save, a.smoke_save, a.config = "none", "none", "none", "none", False

    build = os.path.abspath(a.build)
    work = os.path.join(build, 'work')
    out_root = os.path.abspath(a.out or os.path.join(build, 'dist'))
    store = os.path.join(out_root, 'symstore')
    exe = os.path.join(work, a.exe)
    with open(os.path.join(build, 'state.json')) as f:
        state = json.load(f)
    if not os.path.isfile(exe):
        sys.exit('%s not found: build the game first (scripts/setup.py)' % exe)
    features = host_features(exe)
    check_build_is_current(work, exe, a.allow_stale, features['resource_mods'])
    delay_files = latency_files(work, exe, features['latency_slider'])
    t0 = time.time()

    # ---------------------------------------------------------------- what goes where
    files = []                                                       # (source, path in the package, kind)

    def add(src, rel, kind='data'):
        if not os.path.isfile(src):
            sys.exit('missing: %s' % src)
        files.append((src, rel.replace('\\', '/'), kind))

    empty_dirs = []                                                  # kept as they are (a save's empty stage/ folder)

    def add_tree(src_dir, rel_dir, kind='data', skip=lambda rel: False):
        for dp, dns, fns in os.walk(src_dir):
            dns[:] = [d for d in dns if not skip(os.path.relpath(os.path.join(dp, d), src_dir))]
            if not dns and not fns and dp != src_dir:
                empty_dirs.append(rel_dir + '/' + os.path.relpath(dp, src_dir).replace(os.sep, '/'))
            for n in sorted(fns):
                r = os.path.relpath(os.path.join(dp, n), src_dir)
                if not skip(r):
                    add(os.path.join(dp, n), rel_dir + '/' + r, kind)

    add(exe, 'game/ssbu_native.exe', 'pe')
    add(exe[:-4] + '.pdb', 'game/ssbu_native.pdb', 'pdb')
    add(exe[:-4] + '.map', 'game/ssbu_native.map', 'bin')
    add(os.path.splitext(exe)[0] + '.build.json', 'game/ssbu_native.build.json', 'bin')
    for n in (a.game_dll, 'SDL3.dll', 'ssbu_glslang.dll'):
        add(os.path.join(work, n), 'game/' + n, 'pe')
    for src, relative in delay_files:
        add(src, relative, 'pe' if src.endswith(('.exe', '.dll')) else 'bin')
    if features['resource_mods']:
        add(os.path.join(work, 'ssbu_mod_backend.dll'), 'game/ssbu_mod_backend.dll', 'pe')
        add(os.path.join(work, 'ssbu_mod_backend.build.json'), 'game/ssbu_mod_backend.build.json', 'bin')
        # Notices are required for a distributable package, even with --allow-stale.
        for n in ('dependencies.json', 'ssbu_mod_backend-GPL-3.0.txt'):
            if not os.path.isfile(os.path.join(work, 'mod_backend_licenses', n)):
                sys.exit('missing resource backend licenses: rebuild scripts/build_mod_backend.py')
        add_tree(os.path.join(work, 'mod_backend_licenses'), 'game/mod_backend_licenses', 'bin')
        if os.path.isfile(os.path.join(work, 'ssbu_mod_backend.pdb')):
            add(os.path.join(work, 'ssbu_mod_backend.pdb'), 'game/ssbu_mod_backend.pdb', 'bin')
        add(os.path.join(TEMPLATES, 'MODS.txt'), 'mods/README.txt', 'bin')
    opus = next((n for n in ('opus.dll', 'libopus-0.dll') if os.path.isfile(os.path.join(work, n))), None)
    if opus:
        add(os.path.join(work, opus), 'game/' + opus, 'pe')
    else:
        print('note: no opus.dll in build/work: the package plays no Opus music')
    nro = os.path.join(work, a.nro_bin)
    for n in sorted(os.listdir(nro)):
        if n.lower().endswith('.dll'):
            add(os.path.join(nro, n), 'game/%s/%s' % (a.nro_bin, n), 'pe')
    vc = find_vc_runtime()
    if not vc:
        sys.exit('the MSVC runtime DLLs (%s) were not found (Visual Studio redist folder or System32)' % ', '.join(VC_DLLS))
    for n in VC_DLLS:
        add(os.path.join(vc, n), 'game/' + n, 'pe')

    elf_dir = os.path.join(build, 'binaries', '13.0.4')
    for n in ('main.elf', 'main.npdm', 'sdk.elf'):
        add(os.path.join(elf_dir, n), 'binaries/13.0.4/' + n, 'bin')
    sysdata = os.path.join(build, 'sysdata')
    add_tree(os.path.join(sysdata, 'fonts'), 'sysdata/fonts', 'bin')
    for extra in ('zoneinfo',):
        if os.path.isdir(os.path.join(sysdata, extra)):
            add_tree(os.path.join(sysdata, extra), 'sysdata/' + extra, 'bin')
    if os.path.isfile(os.path.join(build, 'config', 'dlc.txt')):          # the add-on contents owned (runtime/nnmisc.c reads ..\config\dlc.txt)
        add(os.path.join(build, 'config', 'dlc.txt'), 'config/dlc.txt', 'bin')

    if a.cache in ('containers', 'all'):
        cont = os.path.join(build, 'cache', 'vk_containers2')
        if os.path.isfile(os.path.join(cont, 'store_source.txt')) and not os.path.isfile(os.path.join(cont, 'store_building.txt')):
            add_tree(cont, 'cache/vk_containers2', 'cache')
        else:
            print('note: no finished shader-container store in build/cache/vk_containers2: the first start on the other PC builds it')
    if a.cache == 'all':
        vk = os.path.join(build, 'cache', 'vk')
        for sub in ('spv', 'stage'):
            if os.path.isdir(os.path.join(vk, sub)):
                add_tree(os.path.join(vk, sub), 'cache/vk/' + sub, 'cache')
        for n in ('pipes.bin', 'late_containers.txt'):
            if os.path.isfile(os.path.join(vk, n)):
                add(os.path.join(vk, n), 'cache/vk/' + n, 'cache')

    no_work = lambda rel: '.work.' in rel or rel.endswith('.journal') or rel.split(os.sep)[0] == 'cache'
    if a.save != 'none':
        if os.path.isdir(os.path.join(a.save, 'save')):
            add_tree(os.path.join(a.save, 'save'), 'userdata/save', 'save', no_work)
        else:
            print('note: no save in %s: the package starts with a fresh save' % os.path.join(a.save, 'save'))
    smoke = a.smoke_save if a.smoke_save is not None else os.path.join(build, 'userdata')
    if smoke != 'none' and os.path.isdir(os.path.join(smoke, 'save')):
        add_tree(os.path.join(smoke, 'save'), 'tests/smoke_save/save', 'save', no_work)
    add(os.path.join(REPO, 'tests', 'seq_battle_play_fast.txt'), 'tests/seq_battle_play_fast.txt', 'bin')
    if a.config:
        for n in ('graphics.ini', 'controllers.ini'):
            if os.path.isfile(os.path.join(build, 'config', n)):
                add(os.path.join(build, 'config', n), 'config/' + n, 'config')

    add(os.path.join(TEMPLATES, 'launch.ps1'), 'launcher/launch.ps1', 'bin')
    add(os.path.join(TEMPLATES, 'launcher.ini'), 'launcher/launcher.ini', 'config')
    for n in ('Replay.bat', 'README.txt'):            # Replay.bat: launch.ps1 replays a report; play, smoke test and dump are the console's
        add(os.path.join(TEMPLATES, n), n, 'bin')
    # the console (tools/console, Rust + WebView2): the launcher with a window, at the root of the package
    console = os.path.abspath(a.console)
    if not os.path.isfile(console):
        sys.exit('%s not found: build it with  cargo build --release  in tools/console (CARGO_TARGET_DIR=build/console-target)' % console)
    newest_src = max(os.path.getmtime(os.path.join(dp, n)) for tool in ('console', 'updater') for dp, _, ns in os.walk(os.path.join(REPO, 'tools', tool))
                     if os.sep + 'gen' not in dp for n in ns if n.endswith(('.rs', '.html', '.css', '.js', '.toml', '.json', '.woff2')))
    if os.path.getmtime(console) + 1 < newest_src and not a.allow_stale:
        sys.exit('%s is older than its sources in tools/console (cargo build --release; --allow-stale packages it anyway)' % console)
    add(console, 'SSBU Console.exe', 'pe')
    add(os.path.abspath(a.updater), 'SSBU Updater.exe', 'pe')

    romfs_src = os.path.join(state['dump']['romfs'], 'data.arc')
    if not os.path.isfile(romfs_src):
        sys.exit('data.arc not found at %s (state.json "romfs")' % romfs_src)
    # everything else of the RomFS folder too: .nrr/*.nrr, the lists of the script modules the game may load. Without them
    # nn::ro refuses every module and the game waits on the loading screen before the first fight (2026-10-15, first
    # packages: only data.arc was packaged). data.arc itself is linked or copied below.
    romfs_dir = os.path.dirname(romfs_src)
    add_tree(romfs_dir, 'romfs', 'bin', lambda rel: rel == 'data.arc')
    romfs_other = sorted(rel for _, rel, _ in files if rel.startswith('romfs/'))
    if not any(r.startswith('romfs/.nrr/') and r.endswith('.nrr') for r in romfs_other):
        sys.exit('no .nrr/*.nrr in %s: the RomFS dump is incomplete (the game would refuse every script module)' % romfs_dir)

    # ---------------------------------------------------------------- the build id: what the binaries are
    print('hashing %d files...' % len(files))
    hashes, pe = {}, {}
    for src, rel, kind in files:
        if kind != 'cache':
            hashes[rel] = sha256(src)
        if kind == 'pe':
            pe[rel] = pe_info(src)
    build_id = hashlib.sha256('\n'.join('%s %s' % (r, hashes[r]) for r in sorted(hashes) if r.startswith(('game/', 'binaries/'))).encode()).hexdigest()[:12]
    commit = git('rev-parse', 'HEAD')
    dirty = bool(git('status', '--porcelain', '--untracked-files=no'))
    now = datetime.datetime.now()
    name = a.name or 'ssbu-%s-%s%s-%s' % (now.strftime('%Y%m%d-%H%M'), commit[:7] or 'nogit', '-dirty' if dirty else '', build_id[:8])
    pkg = os.path.join(out_root, name)
    if os.path.exists(pkg):
        if not a.force:
            sys.exit('%s exists (--force replaces it)' % pkg)
        shutil.rmtree(pkg)
    print('package %s (build %s)' % (pkg, build_id))

    # ---------------------------------------------------------------- the symbol store, then the package
    stored = 0
    for src, rel, kind in files:
        if kind not in ('pe', 'pdb'):
            continue
        base = os.path.basename(rel)
        if kind == 'pe':
            key = pe[rel]['image_key']
        else:
            key = pe['game/ssbu_native.exe'].get('pdb_key')
            if not key:
                continue
        dst = os.path.join(store, base, key, base)
        if not os.path.isfile(dst):
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            shutil.copy2(src, dst)
            stored += 1
    how = {'link': 0, 'copy': 0}
    for src, rel, kind in files:
        dst = os.path.join(pkg, rel)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        base = os.path.basename(rel)
        if kind in ('pe', 'pdb'):
            key = pe[rel]['image_key'] if kind == 'pe' else pe['game/ssbu_native.exe'].get('pdb_key')
            how[link_or_copy(os.path.join(store, base, key, base), dst)] += 1
        elif base.lower().endswith(('.txt', '.ini')) and kind in ('bin', 'config') and rel.startswith(('launcher/', 'README', 'mods/README')):
            with open(src, 'rb') as f:
                text = f.read()
            with open(dst, 'wb') as f:                                    # Notepad and PowerShell 5.1 read UTF-8 with a BOM
                f.write(text if text.startswith(b'\xef\xbb\xbf') else b'\xef\xbb\xbf' + text)
            hashes[rel] = sha256(dst)
        else:
            shutil.copy2(src, dst)
            how['copy'] += 1
    for d in ['config', 'userdata', 'logs', 'reports', 'cache', 'romfs'] + (['mods'] if features['resource_mods'] else []) + empty_dirs:
        os.makedirs(os.path.join(pkg, d), exist_ok=True)

    romfs = {'source': romfs_src, 'size': os.path.getsize(romfs_src), 'mtime': os.path.getmtime(romfs_src), 'included': a.romfs != 'none',
             'other_files': [r[len('romfs/'):] for r in romfs_other]}
    if a.romfs != 'none':
        dst = os.path.join(pkg, 'romfs', 'data.arc')
        if a.romfs == 'copy':
            copy_big(romfs_src, dst)
            romfs['how'] = 'copy'
        else:
            romfs['how'] = link_or_copy(romfs_src, dst) if a.romfs == 'auto' else (os.link(romfs_src, dst) or 'link')
        print('data.arc: %s' % romfs['how'])
    if a.hash_romfs:
        print('hashing data.arc...')
        romfs['sha256'] = sha256(romfs_src)

    manifest = {
        'build_id': build_id, 'name': name, 'created': now.isoformat(timespec='seconds'), 'packaged_on': socket.gethostname(),
        'git_commit': commit, 'git_dirty': dirty, 'git_branch': git('rev-parse', '--abbrev-ref', 'HEAD'),
        'git_subject': git('log', '-1', '--format=%s'), 'romfs': romfs, 'cache': a.cache,
        'pe': pe, 'symstore': store, 'features': features,
        'mod_resources': {'enabled': features['resource_mods'], 'root': 'mods' if features['resource_mods'] else None,
                          'profile': 'config/mods.json' if features['resource_mods'] else None,
                          'cache': 'cache/mods' if features['resource_mods'] else None},
        # checked by the launcher before every start (a damaged or half-copied package); the settings and the save are the
        # user's to change, the cache is rebuilt by the game: listed without the check
        'files': {rel: {'size': os.path.getsize(os.path.join(pkg, rel)), 'sha256': hashes.get(rel)}
                  for _, rel, kind in files if kind in ('pe', 'pdb', 'bin') or (kind == 'save' and rel.startswith('tests/'))},
        'other_files': sorted(rel for _, rel, kind in files if kind in ('config', 'save') and not rel.startswith('tests/')),
    }
    if a.public:
        manifest.pop('packaged_on', None)
        manifest.pop('symstore', None)
        for key in ('source', 'mtime', 'how'):
            manifest['romfs'].pop(key, None)
    with open(os.path.join(pkg, 'manifest.json'), 'w', encoding='utf-8') as f:
        json.dump(manifest, f, indent=1, ensure_ascii=False)
    os.makedirs(os.path.join(store, 'builds'), exist_ok=True)
    shutil.copy2(os.path.join(pkg, 'manifest.json'), os.path.join(store, 'builds', build_id + '.json'))

    if a.zip:
        zpath = pkg + '.zip'
        print('writing %s ...' % zpath)
        zip_package(pkg, zpath, name)
        print('zip: %.0f MB (no data.arc: put it into romfs/ or set romfs = in launcher/launcher.ini; romfs/.nrr is in the zip)' % (os.path.getsize(zpath) / 1048576.0))

    size = sum(os.path.getsize(os.path.join(pkg, r)) for _, r, _ in files)
    print('done in %.0f s: %d files, %.2f GB without data.arc (%d hard links into the store, %d copies), %d new files in %s'
          % (time.time() - t0, len(files), size / 1073741824.0, how['link'], how['copy'], stored, store))
    print('resource packages: %s' % ('enabled; install into <package>/mods/<package-name>/' if features['resource_mods'] else 'disabled at build time'))
    print('build id %s, commit %s%s' % (build_id, commit[:12], ' (uncommitted changes)' if dirty else ''))
    print('on the other PC: copy the whole folder, start "SSBU Console.exe" (its smoke test for a quick check); reports come back as reports\\*.zip;')
    print('here: Replay.bat <report.zip> in this package replays a run, WinDbg opens its .dmp with .sympath srv*%s' % store)
    return 0


if __name__ == '__main__':
    sys.exit(main())

#!/usr/bin/env python3
"""What a minidump from a test package is: the exception, the modules, and which packaged build they belong to.

    python scripts/dumpinfo.py <file.dmp> [--store build/dist/symstore]

For every module: base address, link stamp, image size, and whether the symbol store (scripts/package.py) has that exact
image, and for the exe its .pdb (by GUID): the files WinDbg / Visual Studio load with  .sympath srv*<store>. The build is
found in <store>/builds/*.json by the exe's link stamp. Host addresses of a crash report (ssbu_native_crash.txt) can be put
into a module with the table printed here.
"""
import argparse
import glob
import json
import os
import struct
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
STREAM_THREADS, STREAM_MODULES, STREAM_EXCEPTION, STREAM_SYSINFO = 3, 4, 6, 7


def read_string(d, rva):
    n = struct.unpack_from('<I', d, rva)[0]
    return d[rva + 4:rva + 4 + n].decode('utf-16-le', 'replace')


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('dump')
    ap.add_argument('--store', default=os.path.join(REPO, 'build', 'dist', 'symstore'))
    a = ap.parse_args()
    with open(a.dump, 'rb') as f:
        d = f.read()
    if d[:4] != b'MDMP':
        sys.exit('not a minidump: %s' % a.dump)
    nstreams, dir_rva = struct.unpack_from('<II', d, 8)
    streams = {}
    for i in range(nstreams):
        typ, size, rva = struct.unpack_from('<III', d, dir_rva + 12 * i)
        streams[typ] = (rva, size)

    if STREAM_SYSINFO in streams:
        rva = streams[STREAM_SYSINFO][0]
        arch, _, _, ncpu, ptype, major, minor, build = struct.unpack_from('<HHHBBIII', d, rva)
        print('system: Windows %d.%d build %d, %d processors' % (major, minor, build, ncpu))
    if STREAM_EXCEPTION in streams:
        rva = streams[STREAM_EXCEPTION][0]
        tid = struct.unpack_from('<I', d, rva)[0]
        code, flags, rec, addr, nparams = struct.unpack_from('<IIQQI', d, rva + 8)
        params = struct.unpack_from('<%dQ' % min(nparams, 15), d, rva + 8 + 32)
        print('exception 0x%08x at %016x on thread %d%s' % (code, addr, tid,
              ('  (%s address %016x)' % ('writing' if params[0] == 1 else 'reading', params[1])) if code == 0xC0000005 and nparams >= 2 else ''))
    else:
        print('no exception record (a freeze or manual dump: every thread as it was)')
    if STREAM_THREADS in streams:
        print('threads: %d' % struct.unpack_from('<I', d, streams[STREAM_THREADS][0])[0])

    builds = {}
    for p in glob.glob(os.path.join(a.store, 'builds', '*.json')):
        with open(p, encoding='utf-8') as f:
            m = json.load(f)
        exe = m.get('pe', {}).get('game/ssbu_native.exe')
        if exe:
            builds[exe['image_key']] = m

    rva = streams[STREAM_MODULES][0]
    n = struct.unpack_from('<I', d, rva)[0]
    print('\nmodules (%d):   base              end               stamp     in store' % n)
    found_build, missing = None, []
    for i in range(n):
        o = rva + 4 + 108 * i
        base, size, _, stamp, name_rva = struct.unpack_from('<QIIII', d, o)
        cv_size, cv_rva = struct.unpack_from('<II', d, o + 24 + 52)
        path = read_string(d, name_rva)
        name = os.path.basename(path.replace('\\', '/'))
        key = '%08X%x' % (stamp, size)
        have = os.path.isfile(os.path.join(a.store, name, key, name))
        pdb = ''
        if cv_size >= 24 and d[cv_rva:cv_rva + 4] == b'RSDS':
            d1, d2, d3 = struct.unpack_from('<IHH', d, cv_rva + 4)
            age = struct.unpack_from('<I', d, cv_rva + 20)[0]
            pdb_name = os.path.basename(d[cv_rva + 24:cv_rva + cv_size].split(b'\0')[0].decode('utf-8', 'replace').replace('\\', '/'))
            pkey = '%08X%04X%04X%s%x' % (d1, d2, d3, d[cv_rva + 12:cv_rva + 20].hex().upper(), age)
            if os.path.isdir(os.path.join(a.store, pdb_name)):
                pdb = '  pdb %s' % ('in store' if os.path.isfile(os.path.join(a.store, pdb_name, pkey, pdb_name)) else 'NOT in store (%s)' % pkey)
        ours = os.path.isdir(os.path.join(a.store, name))
        if ours and not have:
            missing.append(name)
        if name.lower() == 'ssbu_native.exe' and key in builds:
            found_build = builds[key]
        print('  %-26s %016x-%016x %08x  %s%s' % (name[:26], base, base + size, stamp, 'yes' if have else ('NO' if ours else '-'), pdb))
    print()
    if found_build:
        print('build %s (%s), commit %s%s, packaged %s on %s' % (found_build['build_id'], found_build['name'], found_build['git_commit'][:12],
              ' with uncommitted changes' if found_build.get('git_dirty') else '', found_build['created'], found_build['packaged_on']))
    else:
        print('the exe of this dump is not in %s/builds: not from a package made here' % a.store)
    if missing:
        print('modules of a packaged name but another build, missing from the store: %s' % ', '.join(missing))
    print('WinDbg: .sympath srv*%s ; .exepath srv*%s ; .reload ; !analyze -v ; ~*k' % (a.store, a.store))
    return 0


if __name__ == '__main__':
    sys.exit(main())

#!/usr/bin/env python3
"""Build signed portable full/file updates. Never includes data.arc or player-owned directories.

Example (no upload is performed):
  python scripts/release_update.py --package <old package> --overlay <new files> \
    --base <old package> --base <previous hotpatch installation> --sequence 1 \
    --version 2026.10.19.1 --build-id <unique id> --key <private pk8> --out build/dist/release

The offline ZIP and every mirror use the same Ed25519-signed file plan. The private key
stays outside the package; the public key must already be compiled into Console/updater.
"""
from __future__ import annotations
import argparse, datetime, hashlib, importlib.util, json, os, re, subprocess, sys, tempfile, zipfile
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]

def sha(path: Path) -> str:
    h = hashlib.sha256()
    with path.open('rb') as f:
        for data in iter(lambda: f.read(4 << 20), b''): h.update(data)
    return h.hexdigest()

def managed(path: str) -> bool:
    if not path or len(path) > 230 or any(c in path for c in '\\:\x00') or path.startswith('/'): return False
    for part in path.split('/'):
        stem = part.split('.')[0].upper()
        if not part or part in ('.', '..') or part.endswith(('.', ' ')) or any(ord(c) < 32 or c in '<>"|?*' for c in part) or stem in ('CON', 'PRN', 'AUX', 'NUL') or re.fullmatch(r'(?:COM|LPT)[0-9]', stem): return False
    p = path.lower()
    if p in ('ssbu console.exe', 'ssbu updater.exe', 'manifest.json', 'version.json', 'readme.txt', 'license.txt', 'replay.bat'): return True
    if p.startswith('romfs/.nrr/'): return p.endswith('.nrr')
    if p.startswith('binaries/'): return p.endswith(('.elf', '.npdm'))
    if p.startswith(('sysdata/fonts/', 'sysdata/zoneinfo/', 'licenses/', 'game/mod_backend_licenses/')): return True
    if p in ('launcher/launch.ps1', 'tests/seq_battle_play_fast.txt'): return True
    if p.startswith('game/'):
        t = p[5:]
        if '/' not in t: return t.endswith(('.exe', '.dll', '.pdb', '.map', '.build.json'))
        if t.startswith('nro_bin_eh_o2/'): return '/' not in t[len('nro_bin_eh_o2/'):] and t.endswith('.dll')
        if t == 'plugins/latency_slider.dll': return True
        if t in ('net_delay/build.json', 'net_delay/delay_panel.exe', 'net_delay/webview2-license.txt') or t.startswith('net_delay/ui/'): return True
    return False

def load(path: Path): return json.loads(path.read_text(encoding='utf-8-sig'))
def encoded(data) -> bytes: return json.dumps(data, ensure_ascii=False, indent=2).encode('utf-8')
def sign(data: dict, helper: Path, key: Path, key_id: str) -> bytes:
    with tempfile.TemporaryDirectory(prefix='ssbu-sign-') as folder:
        source = Path(folder)/'unsigned.json'; out = Path(folder)/'signed.json'; source.write_bytes(encoded(data))
        subprocess.run([str(helper), 'sign', '--key', str(key), '--key-id', key_id, '--input', str(source), '--output', str(out)], check=True, capture_output=True)
        return out.read_bytes()

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--package', type=Path, required=True)
    ap.add_argument('--overlay', type=Path)
    ap.add_argument('--base', action='append', type=Path, default=[])
    ap.add_argument('--sequence', type=int, required=True)
    ap.add_argument('--version', required=True)
    ap.add_argument('--build-id', required=True)
    ap.add_argument('--key', type=Path, required=True)
    ap.add_argument('--key-id', default='yuzu-20261019')
    ap.add_argument('--helper', type=Path, default=ROOT/'build/updater-target/release/ssbu-updater.exe')
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--notes', type=Path, required=True)
    ap.add_argument('--kind', choices=('patch','full','both'), default='both')
    ap.add_argument('--portable', action='store_true', help='also emit a fresh-use portable ZIP from the signed full package; no player data or data.arc')
    ap.add_argument('--force-file', action='append', default=[], help='include a managed file even when hashes match, e.g. for self-update verification')
    ap.add_argument('--remove', action='append', default=[])
    ap.add_argument('--sources', type=Path, help='JSON list of {name,kind,url,code,note}; URL may contain {filename}')
    args = ap.parse_args()
    if args.sequence < 1: ap.error('sequence must be positive')
    if args.kind != 'full' and not args.base: ap.error('patch builds require --base')
    helper=args.helper.resolve(); args.out.mkdir(parents=True,exist_ok=True)
    original=load(args.package/'manifest.json')
    source_paths={}
    for rel in original['files']:
        if managed(rel) and rel not in ('manifest.json','version.json'): source_paths[rel]=args.package/rel
    if args.overlay:
        for p in args.overlay.rglob('*'):
            if p.is_file():
                rel=p.relative_to(args.overlay).as_posix()
                if not managed(rel): raise ValueError(f'Overlay contains protected or unsupported path: {rel}')
                source_paths[rel]=p
    for rel in args.remove:
        if not managed(rel) or rel in ('manifest.json','version.json'):raise ValueError('Invalid removal: '+rel)
        source_paths.pop(rel,None)
    if any(p.name.lower()=='data.arc' for p in source_paths.values()):raise ValueError('data.arc must never be shipped')
    records={rel:{'size':p.stat().st_size,'sha256':sha(p)} for rel,p in sorted(source_paths.items())}
    host=load(source_paths['game/ssbu_native.build.json']);features={k:host[k] for k in ['resource_mods','latency_slider']}
    if host['exe_sha256']!=records['game/ssbu_native.exe']['sha256']:raise ValueError('Host feature record does not match executable')
    bases={}
    for folder in args.base:
        m=load(folder/'manifest.json');bid=m['build_id']
        if bid in bases:raise ValueError('Duplicate base build id: '+bid)
        base_host=load(folder/'game/ssbu_native.build.json')
        if any(base_host[k]!=features[k] for k in features):raise ValueError('Base feature combination differs: '+bid)
        base_records={}
        # Read actual files; mutable metadata is validated by build id, not a machine-specific timestamp hash.
        for rel in source_paths:
            path=folder/rel
            actual=sha(path) if path.is_file() else None
            declared=m.get('files',{}).get(rel,{}).get('sha256')
            if declared and not actual:raise ValueError('Base is incomplete; missing declared managed file: '+rel)
            if actual and declared and actual!=declared:raise ValueError('Base was locally modified: '+rel)
            if actual and not declared and rel not in ('manifest.json','version.json'):raise ValueError('Unmanaged file in base: '+rel)
            base_records[rel]=actual
        for rel in args.remove:base_records[rel]=sha(folder/rel)
        bases[bid]=base_records
    v={'schema':1,'channel':'yuzu-test','platform':'win-x64','sequence':args.sequence,'version':args.version,'build_id':args.build_id,'game_version':'13.0.4','features':features,'minimum_updater':1}
    generated={'version.json':encoded(v)}
    records['version.json']={'size':len(generated['version.json']),'sha256':hashlib.sha256(generated['version.json']).hexdigest()}
    for rec in bases.values():rec['version.json']=None
    spec=importlib.util.spec_from_file_location('package',ROOT/'scripts/package.py');pkg=importlib.util.module_from_spec(spec);spec.loader.exec_module(pkg)
    pe=dict(original.get('pe',{}))
    for rel in ['game/ssbu_native.exe','SSBU Console.exe','SSBU Updater.exe']:
        if rel in source_paths:pe[rel]=pkg.pe_info(source_paths[rel])
    pe={k:v for k,v in pe.items() if k in source_paths}
    manifest={'name':'yuzu-test-'+args.version,'build_id':args.build_id,'channel':'yuzu-test','sequence':args.sequence,'game_version':'13.0.4','features':features,'romfs':{'size':original.get('romfs',{}).get('size'),'included':False,'other_files':sorted(k[6:] for k in source_paths if k.startswith('romfs/'))},'cache':'none','files':records,'pe':pe}
    generated['manifest.json']=encoded(manifest)
    records={**records,'manifest.json':{'size':len(generated['manifest.json']),'sha256':hashlib.sha256(generated['manifest.json']).hexdigest()}}
    notes=args.notes.read_text(encoding='utf-8-sig').strip()
    sources=load(args.sources) if args.sources else []
    for s in sources:
        if s.get('kind') not in ('direct','browser') or not s.get('url','').startswith('https://'):raise ValueError('Sources must be direct/browser HTTPS entries')
    artifacts=[]
    for kind in (['patch','full'] if args.kind=='both' else [args.kind]):
        included={}
        for rel,rec in records.items():
            if kind=='full' or rel in generated or rel in args.force_file or any(before.get(rel)!=rec['sha256'] for before in bases.values()):included[rel]=rec
        changes=[{'path':rel,**rec,'before':{bid:before.get(rel) for bid,before in bases.items()}} for rel,rec in sorted(included.items())]
        removals=[{'path':rel,'before':{bid:before[rel] for bid,before in bases.items()}} for rel in args.remove]
        plan={**v,'kind':kind,'base_build_ids':sorted(bases) if kind=='patch' else [],'notes':notes,'files':changes,'remove':removals}
        envelope=sign(plan,helper,args.key,args.key_id)
        name=f'yuzu-test-{args.version}-{kind}.zip';out=args.out/name
        with zipfile.ZipFile(out,'w',zipfile.ZIP_DEFLATED,compresslevel=6,allowZip64=True,strict_timestamps=False) as z:
            z.writestr('update.json',envelope)
            for f in changes:
                rel=f['path'];member='payload/'+rel
                if rel in generated:z.writestr(member,generated[rel])
                else:z.write(source_paths[rel],member)
        with zipfile.ZipFile(out) as z:
            if z.testzip() is not None:raise ValueError('ZIP CRC failed')
            if any(Path(n).name.lower()=='data.arc' for n in z.namelist()):raise ValueError('Unexpected data.arc')
        if kind=='full' and args.portable:
            portable=args.out/f'yuzu-test-{args.version}-portable.zip'
            with zipfile.ZipFile(out) as source, zipfile.ZipFile(portable,'w',zipfile.ZIP_DEFLATED,compresslevel=6,allowZip64=True,strict_timestamps=False) as target:
                prefix=f'yuzu-test-{args.version}/'
                for item in source.infolist():
                    if item.filename=='update.json':target.writestr(prefix+'updates/installed.json',source.read(item))
                    else:
                        rel=item.filename.removeprefix('payload/')
                        with source.open(item) as a, target.open(prefix+rel,'w',force_zip64=True) as b:
                            import shutil
                            shutil.copyfileobj(a,b,4<<20)
                target.write(ROOT/'tools/package/launcher.ini',prefix+'launcher/launcher.ini')
            portable.with_suffix('.zip.sha256').write_text(sha(portable)+'  '+portable.name+'\n',encoding='utf-8')
        digest=sha(out);out.with_suffix('.zip.sha256').write_text(digest+'  '+out.name+'\n',encoding='utf-8')
        artifact={'id':kind,'kind':kind,'base_build_ids':sorted(bases) if kind=='patch' else [],'size':out.stat().st_size,'sha256':digest,'sources':[{**s,'url':s['url'].replace('{filename}',name)} for s in sources]}
        (args.out/(name+'.artifact.json')).write_bytes(encoded(artifact));artifacts.append(artifact)
        print(json.dumps({'archive':str(out),'files':len(changes),'size':out.stat().st_size,'sha256':digest}))
    feed={**v,'notes':notes,'published_at':datetime.datetime.now(datetime.timezone(datetime.timedelta(hours=8))).isoformat(timespec='seconds'),'artifacts':artifacts}
    (args.out/'update.json').write_bytes(sign(feed,helper,args.key,args.key_id))
    (args.out/'release-info.json').write_bytes(encoded({'version':v,'artifacts':artifacts,'published':False,'source_configured':bool(sources),'note':'No upload is performed by this script. Publish identical signed artifacts to verified sources before publishing update.json.'}))
if __name__=='__main__':main()

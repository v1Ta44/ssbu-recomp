"""One native stream/analysis check; no game, GPU, or game DLL is loaded."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[1]

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build-root',type=Path,default=ROOT)
    args=parser.parse_args()
    state=json.loads((args.build_root/'build/state.json').read_text(encoding='utf-8'))
    out=args.build_root/'build/mod-adapt/gamebanana/lifetime-followup/trace-check'
    out.mkdir(parents=True,exist_ok=True)
    exe=out/'nvn_lifetime_trace_test.exe'
    subprocess.run([state['tools']['clang'],'-O1','-Wall','-Wextra','-Werror',
                    str(ROOT/'runtime/nvn_lifetime.c'),str(ROOT/'tests/nvn_lifetime_trace_test.c'),
                    '-o',str(exe)],check=True)
    env=dict(os.environ);env.pop('SSBU_NVN_LIFETIME',None)
    subprocess.run([str(exe),'--disabled'],check=True,env=env,timeout=10)
    # Unique files preserve previous evidence, and exercise CREATE_NEW rather than truncation.
    import tempfile
    with tempfile.TemporaryDirectory(prefix='nvn-life-',dir=out) as directory:
        path=Path(directory)/'events.bin'
        subprocess.run([str(exe)],check=True,env=dict(env,SSBU_NVN_LIFETIME=str(path)),timeout=10)
        spec=importlib.util.spec_from_file_location('life',ROOT/'scripts/analyze_nvn_lifetime.py')
        module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
        result=module.analyze(path)
        kinds={r['kind'] for r in result['findings']}
        assert kinds=={'memory_release_overlaps_cpu_read','borrowed_read_after_memory_release',
                       'borrowed_read_after_pool_finalize','borrowed_read_after_pool_reuse',
                       'snapshot_read_after_release','snapshot_release_overlaps_cpu_read'},result
        assert not result['open_reads'] and not result['open_releases'] and not result['open_pool_finalizers'] and not result['partial_tail_bytes']
        assert all(r['texture']!='0x202' for r in result['findings']),result
        assert result['coverage']['pool_address_reinitializations']==1,result
        assert result['coverage']['read_threads']==1,result
        assert result['coverage']['valid_snapshot_reads_after_source_release']==2,result
        assert result['coverage']['valid_snapshot_reads_after_pool_finalize']==1,result
        assert result['coverage']['successful_memory_operations']=={'release': 1},result
        raw=path.read_bytes()
        truncated=Path(directory)/'truncated.bin';truncated.write_bytes(raw[:-7])
        prefix=module.analyze(truncated)
        assert prefix['partial_tail_bytes']==module.RECORD.size-7 and prefix['open_reads']==1
        (out/'events.bin').write_bytes(raw)
        (out/'analysis.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print('PASS read/release intervals, original pool generation after address reuse, owned-source distinction, failed-free exclusion, snapshot cleanup during reads and truncated prefix')
    return 0

if __name__=='__main__':
    raise SystemExit(main())

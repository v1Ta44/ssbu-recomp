#!/usr/bin/env python3
"""Read-only interface inventory. Outputs are local build data, never game code.
Run: python tools/audit/interfaces.py [--out build/interface-audit]
Source matches are candidates, not proof of the final runtime binding or behavior.
"""
import argparse
import ast
import collections
import datetime
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[2]
SCOPES = ('runtime/', 'fighter_sdk/', 'scripts/', 'tools/', 'mods/', 'tests/')
EXTS = {'.c', '.cpp', '.h', '.inc', '.py', '.ps1', '.bat'}
TOKEN = r'"(?:\\.|[^"\\])*"|[A-Za-z_]\w*(?:\(\s*"(?:\\.|[^"\\])*"\s*\))?'
PAIR = re.compile(r'\{\s*((?>(?:' + TOKEN + r')\s*)+)\s*,\s*([A-Za-z_]\w*)\s*\}')


def mask(s, strings=True):
    def sub(m):
        if not strings and m[0][0] in ('"', "'"):
            return m[0]
        return re.sub(r'[^\n]', ' ', m[0])
    return re.sub(r'"(?:\\.|[^"\\])*"|\x27(?:\\.|[^\x27\\])*\x27|/\*[\s\S]*?\*/|//[^\n]*', sub, s)


def end_at(s, start, left='{', right='}'):
    n = 1
    for i in range(start+1, len(s)):
        n += (s[i] == left) - (s[i] == right)
        if not n:
            return i
    return len(s)-1


def compact(s):
    return ' '.join(s.split())


def macro_defs(s):
    return {m[1]:(m[2],m[3]) for m in re.finditer(r'^\s*#define\s+(\w+)(?:\((\w+)\))?[ \t]+([^\n]+)', s, re.M) if '"' in m[3]}


def string_expr(s, defs, depth=0):
    if depth > 5:
        return None
    parts, pos = [], 0
    for m in re.finditer(TOKEN, s):
        if s[pos:m.start()].strip():
            return None
        token = m[0]
        if token.startswith('"'):
            value = ast.literal_eval(token)
        else:
            call = re.fullmatch(r'(\w+)\(\s*("(?:\\.|[^"\\])*")\s*\)', token)
            name = call[1] if call else token
            if name not in defs:
                return None
            arg, body = defs[name]
            if arg:
                if not call:
                    return None
                body = re.sub(r'\b'+arg+r'\b', lambda _:call[2], body)
            value = string_expr(body, defs, depth+1)
        if value is None:
            return None
        parts.append(value)
        pos = m.end()
    return ''.join(parts) if parts and not s[pos:].strip() else None


def c_functions(s, file):
    code, rows = mask(s), []
    depth, depths = 0, []
    for ch in code:
        depths.append(depth)
        depth += (ch=='{')-(ch=='}')
    for m in re.finditer(r'^[ \t]*([^\n;{}#=]*?\b([A-Za-z_]\w*)[ \t]*\()', code, re.M):
        name, prefix = m[2], s[m.start():m.start(2)]
        if name in {'if','for','while','switch','return','sizeof','defined','else','do'}:
            continue
        if re.search(r'\b(return|if|else|for|while|switch|typedef|case)\b|->|\.', mask(prefix)):
            continue
        op = m.end()-1
        end = end_at(code,op,'(',')')
        tail = end+1
        while tail<len(code) and code[tail].isspace():
            tail+=1
        if tail>=len(code) or code[tail] not in '{;':
            continue
        if (not prefix.strip() or depths[m.start()]) and not file.endswith(('.h','.cpp')):
            continue
        kind = 'definition' if code[tail]=='{' else 'declaration'
        finish = end_at(code,tail) if kind=='definition' else tail
        rows.append({'name':name,'file':file,'line':s.count('\n',0,m.start())+1,'signature':compact(s[m.start():end+1]),
                     'kind':kind,'static':bool(re.search(r'\bstatic\b',prefix)),
                     'exported':'dllexport' in prefix or 'SSBU_NET_API' in prefix,
                     '_body':s[tail+1:finish] if kind=='definition' else ''})
    return rows


def elf_symbols(p):
    d=p.read_bytes()
    if d[:6]!=b'\x7fELF\x02\x01':
        raise ValueError('Not ELF64 LE: '+str(p))
    so=struct.unpack_from('<Q',d,40)[0]
    ent,num=struct.unpack_from('<HH',d,58)
    sections=[struct.unpack_from('<IIQQQQIIQQ',d,so+i*ent) for i in range(num)]
    for sec in sections:
        if sec[1]!=11:
            continue
        st=sections[sec[6]]
        strings=d[st[4]:st[4]+st[5]]
        for off in range(sec[4],sec[4]+sec[5],sec[9] or 24):
            no,info,other,index,value,size=struct.unpack_from('<IBBHQQ',d,off)
            if no:
                yield {'name':strings[no:strings.index(0,no)].decode('utf8','replace'),'file':p.relative_to(ROOT).as_posix(),
                       'module':p.stem,'direction':'export' if index else 'import','address':hex(value),
                       'type':{0:'NOTYPE',1:'OBJECT',2:'FUNC',6:'TLS'}.get(info&15,str(info&15)),'size':size,'binding':info>>4}


def pe_exports(p):
    d=p.read_bytes()
    if d[:2]!=b'MZ':
        return
    pe=struct.unpack_from('<I',d,0x3c)[0]
    nsec=struct.unpack_from('<H',d,pe+6)[0]
    optsize=struct.unpack_from('<H',d,pe+20)[0]
    opt=pe+24
    magic=struct.unpack_from('<H',d,opt)[0]
    rva,size=struct.unpack_from('<II',d,opt+(112 if magic==0x20b else 96))
    if not rva:
        return
    sections=[struct.unpack_from('<8sIIIIIIHHI',d,opt+optsize+i*40) for i in range(nsec)]
    def raw(a):
        for s in sections:
            if s[2]<=a<s[2]+max(s[1],s[3]):
                return a-s[2]+s[4]
        raise ValueError('Unmapped RVA')
    base,nfn,nnames,funcs,names,ords=struct.unpack_from('<IIIIII',d,raw(rva)+16)
    for i in range(nnames):
        no=raw(struct.unpack_from('<I',d,raw(names)+4*i)[0])
        name=d[no:d.index(0,no)].decode('ascii','replace')
        ordinal=struct.unpack_from('<H',d,raw(ords)+2*i)[0]
        yield {'name':name,'file':p.relative_to(ROOT).as_posix(),'ordinal':base+ordinal,
               'rva':hex(struct.unpack_from('<I',d,raw(funcs)+4*ordinal)[0])}


def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--out',default='build/interface-audit')
    ap.add_argument('--no-build-symbols',action='store_true')
    args=ap.parse_args()
    out=ROOT/args.out
    out.mkdir(parents=True,exist_ok=True)
    tracked=subprocess.check_output(['git','ls-files'],cwd=ROOT,text=True).splitlines()
    files=sorted({f for f in tracked if f.startswith(SCOPES) and Path(f).suffix in EXTS} | {f for f in tracked if '/' not in f and f.endswith('.bat')}|{'tools/audit/interfaces.py'})
    texts={f:(ROOT/f).read_text(encoding='utf8',errors='replace') for f in files}
    defs={f:macro_defs(mask(s,False)) for f,s in texts.items()}
    candidates=collections.defaultdict(set)
    for dd in defs.values():
        for k,v in dd.items():
            candidates[k].add(v)
    unique={k:next(iter(v)) for k,v in candidates.items() if len(v)==1}
    funcs={f:c_functions(s,f) for f,s in texts.items() if Path(f).suffix in {'.c','.cpp','.h','.inc'}}
    handlers=collections.defaultdict(list)
    for rr in funcs.values():
        for r in rr:
            if r['kind']=='definition':
                handlers[r['name']].append(r)
    rows={k:[] for k in ('source_functions','callback_contracts','header_macros','guest_imports','environment','cli','dynamic_bindings','generated_symbols','binary_exports','configuration')}
    coverage=[]
    for f,s in texts.items():
        code=mask(s,False)
        line=lambda pos:s.count('\n',0,pos)+1
        coverage.append({'file':f,'sha256':hashlib.sha256(s.encode()).hexdigest(),'lines':len(s.splitlines())})
        clean=lambda r:{k:v for k,v in r.items() if not k.startswith('_')}
        for r in funcs.get(f,[]):
            if not r['static'] or f.endswith('.h') or r['exported']:
                rows['source_functions'].append(clean(r))
        if f in funcs:
            structural=mask(s)
            import_spans=[(m.end()-1,end_at(structural,m.end()-1)) for m in re.finditer(r'\bImportDef\s+\w+\s*(?:\[[^\]]*\])?\s*=\s*\{',structural)]
            for m in PAIR.finditer(code):
                name=string_expr(m[1],{**unique,**defs[f]})
                hh=handlers.get(m[2],[])
                in_import_table=any(a<=m.start()<b for a,b in import_spans)
                if not name or not (in_import_table or name.startswith('nvn') or any('Ctx' in r['signature'] for r in hh)):
                    continue
                local=[r for r in hh if r['file']==f] or hh
                body=local[0]['_body'] if local else ''
                rows['guest_imports'].append({'name':name,'handler':m[2],'file':f,'line':line(m.start()),
                    'kind':'nvn_dispatch' if name.startswith('nvn') and name!='nvnBootstrapLoader' else 'guest_import_candidate',
                    'handler_locations':[clean(r) for r in local],
                    'review_hint':'shared_constant_or_noop' if re.search(r'(?:ret[01]|keep|nop|zero|true|false|unsupported|generic)$',m[2],re.I) else 'inspect_implementation',
                    'handler_excerpt':compact(body)[:550]})
            for m in re.finditer(r'([^\n;{}]*?\(\s*\*\s*(\w+)\s*\)\s*\([^;{}]*\))\s*;',code):
                if f.endswith('.h') or 'typedef' in m[1]:
                    rows['callback_contracts'].append({'name':m[2],'file':f,'line':line(m.start()),'signature':compact(s[m.start(1):m.end(1)])})
            if f.endswith('.h'):
                for m in re.finditer(r'^\s*#define\s+(\w+)\(([^\n)]*)\)',code,re.M):
                    rows['header_macros'].append({'name':m[1],'file':f,'line':line(m.start()),'signature':m[1]+'('+m[2]+')'})
            for m in re.finditer(r'\b(GetProcAddress|hostfn_address|sdk_install|sdk_host_override|import_set_address|import_lookup)\s*\([^;\n]*',code):
                rows['dynamic_bindings'].append({'name':m[1],'file':f,'line':line(m.start()),'expression':compact(s[m.start():m.end()])})
        env_patterns=[r'\bgetenv\s*\(\s*"([A-Za-z_]\w*)"',r'\b(?:GetEnvironmentVariable[AW]?|env_utf8)\s*\(\s*L?"([A-Za-z_]\w*)"',r'(?:os\.environ(?:\.get)?|os\.getenv)\s*[\[(]\s*[\x27"]([A-Za-z_]\w*)[\x27"]',r'[\x27"]((?:SSBU|RECOMP)_[A-Z0-9_]+)[\x27"]']
        for pattern in env_patterns:
            for m in re.finditer(pattern,code):
                rows['environment'].append({'name':m[1],'file':f,'line':line(m.start()),'evidence':s.splitlines()[line(m.start())-1].strip()})
        if f in {'runtime/gfx_options.c','runtime/nnpad_sdl.c'}:
            for cm in re.finditer(r'^"([A-Za-z0-9_.-]+)\s*=\s*(.*?)\\r\\n"',s,re.M):
                rows['configuration'].append({'name':cm[1],'default':cm[2],'file':f,'line':line(cm.start()),'kind':'ini_default'})
        if f.endswith('.py'):
            tree=ast.parse(s)
            for n in ast.walk(tree):
                if isinstance(n,ast.Call) and isinstance(n.func,ast.Attribute) and n.func.attr in {'add_argument','add_parser'}:
                    flags=[a.value for a in n.args if isinstance(a,ast.Constant) and isinstance(a.value,str)]
                    rows['cli'].append({'file':f,'line':n.lineno,'name':', '.join(flags),'kind':n.func.attr,'options':{k.arg:ast.unparse(k.value) for k in n.keywords if k.arg}})
            if '__main__' in s or 'sys.argv' in code:
                rows['cli'].append({'file':f,'line':1,'name':f,'kind':'script_entry','description':(ast.get_docstring(tree) or '')[:1000],
                    'argv_evidence':[{'line':i+1,'text':t.strip()} for i,t in enumerate(s.splitlines()) if 'sys.argv' in t][:14]})
        elif f.endswith(('.bat','.ps1')):
            rows['cli'].append({'file':f,'line':1,'name':f,'kind':'shell_entry'})
    rows['environment']=list({(r['name'],r['file'],r['line']):r for r in rows['environment']}.values())
    extra=[]
    if not args.no_build_symbols:
        for name in ('main','sdk'):
            p=ROOT/f'build/binaries/13.0.4/{name}.elf'
            if p.exists():
                rows['generated_symbols'].extend(elf_symbols(p)); extra.append(p.relative_to(ROOT).as_posix())
        for p in sorted((ROOT/'build/work/gen_nro_eh_o2').glob('*/imports.c')):
            s=p.read_text(encoding='utf8',errors='replace')
            for m in re.finditer(r'\{\s*"([^"\n]+)"\s*,\s*(0x[0-9a-fA-F]+|\d+)(?:ULL|ull)?\s*,\s*([01])\s*\}',s):
                rows['generated_symbols'].append({'name':m[1],'file':p.relative_to(ROOT).as_posix(),'line':s.count('\n',0,m.start())+1,'module':p.parent.name,
                    'address':m[2],'direction':'export' if m[3]=='1' else 'import','type':'generated_DynSym'})
            extra.append(p.relative_to(ROOT).as_posix())
        for p in [ROOT/'build/work/ssbu_native.exe',*sorted((ROOT/'build/work').glob('ssbu_game*.dll')),*sorted((ROOT/'build/work/nro_bin_eh_o2').glob('*.dll'))]:
            if p.exists():
                rows['binary_exports'].extend(pe_exports(p)); extra.append(p.relative_to(ROOT).as_posix())
    summary={k:len(v) for k,v in rows.items()}
    summary.update(scanned_source_files=len(files),source_lines=sum(x['lines'] for x in coverage),
        environment_unique_names=len({r['name'] for r in rows['environment']}),guest_registration_unique_names=len({r['name'] for r in rows['guest_imports']}),
        cli_argument_records=sum(r['kind']=='add_argument' for r in rows['cli']),cli_scripts=sum(r['kind']=='script_entry' for r in rows['cli']),
        generated_modules=len({r['module'] for r in rows['generated_symbols']}))
    metadata={'schema':1,'date':datetime.datetime.now(datetime.timezone(datetime.timedelta(hours=8))).date().isoformat(),
        'git_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'summary':summary,'source_coverage':coverage,'local_build_inputs':extra,
        'limitations':['Lexical C/C++ index, not a compiler or call graph. Duplicate registrations do not imply effective runtime binding.',
        'C++ operators and macro-expanded definitions are referenced through canonical headers/macros; generated SDK wrappers absent locally are not invented.',
        'Environment occurrences include reads, writes and helper arguments; they need not affect the default executable.',
        'ELF symbols include both functions and data. Historical r1src, tests and mods remain identifiable by path.',
        'Constant/noop names are review hints, not proof of unimplemented behavior. No game or network tests are executed.']}
    (out/'interfaces.json').write_text(json.dumps({'metadata':metadata,**rows},ensure_ascii=False,indent=2)+'\n',encoding='utf8')
    for k,v in rows.items():
        (out/(k+'.jsonl')).write_text(''.join(json.dumps(r,ensure_ascii=False)+'\n' for r in v),encoding='utf8')
    (out/'SUMMARY.md').write_text('# 本地接口索引\n\n生成日期：'+metadata['date']+'。静态检查，未运行游戏。\n\n'+
        '\n'.join(f'- {k}: {v}' for k,v in summary.items())+'\n\n限制：\n\n'+'\n'.join('- '+s for s in metadata['limitations'])+'\n',encoding='utf8')
    search_rows=[{'category':k,**r} for k,v in rows.items() for r in v]
    payload=json.dumps(search_rows,ensure_ascii=False).replace('<',r'\u003c')
    template=(Path(__file__).with_suffix('.html')).read_text(encoding='utf-8-sig')
    (out/'index.html').write_text(template.replace('__INTERFACE_DATA__',payload),encoding='utf8')
    print(json.dumps(summary,ensure_ascii=False,indent=2))


if __name__=='__main__':
    main()

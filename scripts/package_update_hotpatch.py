#!/usr/bin/env python3
"""Wrap a signed update ZIP for older Consoles. Does not upload files or include signing keys."""
from pathlib import Path
import argparse,base64,hashlib,io,json,shutil,subprocess,zipfile
ROOT=Path(__file__).resolve().parents[1]
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 ap=argparse.ArgumentParser(description=__doc__)
 ap.add_argument('--update',type=Path,required=True)
 ap.add_argument('--helper',type=Path,default=ROOT/'build/updater-target/release/ssbu-updater.exe')
 ap.add_argument('--out',type=Path,required=True)
 ap.add_argument('--runtime-snapshot',type=Path,help='exact staged public runtime sources for a reused host binary')
 ap.add_argument('--host-provenance',type=Path)
 args=ap.parse_args();args.out.parent.mkdir(parents=True,exist_ok=True)
 prefix='FlickerHotpatch-20261019-Update';folder=args.out.parent/prefix
 if folder.exists():raise SystemExit('Choose a fresh destination; output folder exists: '+str(folder))
 folder.mkdir();shutil.copy2(args.update,folder/'Update.zip');shutil.copy2(args.helper,folder/'SSBU Updater.exe');shutil.copy2(ROOT/'LICENSE',folder/'LICENSE.txt')
 for name in ['Apply','Rollback','Recover']:
  text=(ROOT/'tools/package/update-bootstrap.cmd').read_text(encoding='utf-8-sig');(folder/(name+'.cmd')).write_bytes(text.replace('\r\n','\n').replace('\n','\r\n').encode('utf-8'))
 (folder/'使用说明.txt').write_text('''闪烁修复 + 便携更新功能热补丁（2026-10-19）

适用：yuzu-all-features-20261018-98528e1 原测试包，以及已安装上一版闪烁热补丁的完整测试包。
内容：地图表面闪烁修复、失焦继续运行、Console 手动更新及失败恢复。
保留资源模组、联网、延迟滑块；不覆盖存档、模组、配置、缓存和 data.arc。

1. 完全退出游戏、SSBU Console、手柄检测和延迟面板。
2. 将整个 FlickerHotpatch-20261019-Update 文件夹放进旧测试包根目录，与 game、SSBU Console.exe 并列。
3. 打开补丁文件夹，双击 Apply.cmd。完成后从原来的 SSBU Console.exe 启动。
4. 开始页会出现“版本与更新”。以后可直接用“从文件更新”选择发行的签名更新 ZIP。

本文件夹的 Update.zip 就是可导入 Console 的签名文件；外层 ZIP 用于首次手动安装。
Rollback.cmd 恢复本次安装前的程序。Recover.cmd 用于中断恢复。
保留包内 updates 文件夹，其中有安装备份和记录。
首次运行可能重新准备着色器，无需删除缓存。

在线检查入口已配置，但尚未发布签名通道时会提示暂不可用；离线导入不受影响。
源码、构建说明和 GPL 许可一并提供；不包含发行私钥。
''',encoding='utf-8-sig')
 source=folder/'source';source.mkdir();revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip();archive=subprocess.check_output(['git','archive','--format=zip',revision],cwd=ROOT);overlays=[]
 with zipfile.ZipFile(io.BytesIO(archive)) as original,zipfile.ZipFile(source/'ssbu-recomp-source.zip','w',zipfile.ZIP_DEFLATED,compresslevel=6,strict_timestamps=False) as z:
  for item in original.infolist():
   if item.is_dir():continue
   if item.filename.lower().endswith(('.pk8','.key','.pfx')):raise ValueError('Private-key-like tracked file; refusing source packaging')
   candidate=args.runtime_snapshot/Path(item.filename).name if args.runtime_snapshot and item.filename.startswith('runtime/') else None
   if candidate and candidate.is_file() and item.filename.endswith(('.c','.h','.inc')):z.writestr(item.filename,candidate.read_bytes());overlays.append(item.filename)
   else:z.writestr(item.filename,original.read(item))
 if args.host_provenance:shutil.copy2(args.host_provenance,source/'runtime-build-provenance.json')
 (source/'REBUILD.txt').write_text('Source revision: '+revision+'\nBuild helper first, then Console as documented in BUILDING.md. Public release key is included; private signing key is not.\nThis patch reuses the verified host binary with mods and latency slider enabled. Exact staged public runtime sources overlay the repository snapshot; runtime-build-provenance.json records their hashes. Generated game code/assets are excluded; use your own dump with the normal build process.\nUpdater/Console sources and Cargo locks match the revision. scripts/release_update.py builds signed updates; scripts/package_update_hotpatch.py builds this bootstrap wrapper.\n',encoding='utf-8')
 with zipfile.ZipFile(args.update) as z:plan=json.loads(base64.b64decode(json.loads(z.read('update.json'))['payload']))
 info={'source_revision':revision,'version':plan['version'],'build_id':plan['build_id'],'base_build_ids':plan['base_build_ids'],'inner_update_sha256':digest(args.update),'helper_sha256':digest(args.helper),'runtime_source_overlay':overlays,'published':False}
 (folder/'build-info.json').write_text(json.dumps(info,indent=2),encoding='utf-8')
 files={p.relative_to(folder).as_posix():digest(p) for p in folder.rglob('*') if p.is_file()}
 (folder/'SHA256SUMS.txt').write_text(''.join(h+'  '+n+'\n' for n,h in sorted(files.items())),encoding='utf-8')
 with zipfile.ZipFile(args.out,'w',zipfile.ZIP_DEFLATED,compresslevel=6,strict_timestamps=False) as z:
  for p in folder.rglob('*'):
   if p.is_file():z.write(p,p.relative_to(folder.parent))
 with zipfile.ZipFile(args.out) as z:assert z.testzip() is None
 result={'archive':str(args.out),'size':args.out.stat().st_size,'sha256':digest(args.out),**info}
 args.out.with_suffix('.zip.sha256').write_text(result['sha256']+'  '+args.out.name+'\n',encoding='utf-8')
 args.out.with_suffix('.release.json').write_text(json.dumps(result,indent=2),encoding='utf-8');print(json.dumps({k:v for k,v in result.items() if k!='runtime_source_overlay'}))
if __name__=='__main__':main()

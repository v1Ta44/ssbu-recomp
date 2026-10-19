use ssbu_updater::*;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
fn value(args: &[String], flag: &str) -> Result<String> {
    args.windows(2)
        .find(|a| a[0] == flag)
        .map(|a| a[1].clone())
        .ok_or_else(|| format!("缺少参数 {flag}"))
}
fn wait_parent(pid: u32) -> Result<()> {
    if pid == 0 {
        return Ok(());
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, WAIT_OBJECT_0},
            System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
        };
        let h = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if h.is_null() {
            return Ok(());
        }
        let status = unsafe { WaitForSingleObject(h, 30000) };
        unsafe { CloseHandle(h) };
        if status != WAIT_OBJECT_0 {
            return Err("控制台尚未退出，未安装更新".into());
        }
    }
    Ok(())
}
fn log(root: &Path, text: &str) {
    let _ = atomic_bytes(&root.join("updates/last-result.txt"), text.as_bytes());
}
fn restart(root: &Path) -> Result<()> {
    let mut c = Command::new(root.join("SSBU Console.exe"));
    c.current_dir(root);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    c.spawn().map_err(|e| e.to_string())?;
    Ok(())
}
fn run(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("keygen")=>{let private=PathBuf::from(value(args,"--key")?);if private.exists(){return Err("私钥已存在，未覆盖".into());}let(key,public)=generate_key()?;if let Some(p)=private.parent(){fs::create_dir_all(p).map_err(|e|e.to_string())?;}let mut f=std::fs::OpenOptions::new().write(true).create_new(true).open(&private).map_err(|e|e.to_string())?;use std::io::Write;f.write_all(&key).map_err(|e|e.to_string())?;f.sync_all().map_err(|e|e.to_string())?;let id=value(args,"--key-id")?;let trust=Trust{keys:std::collections::BTreeMap::from([(id,public)])};save_json(&PathBuf::from(value(args,"--public-out")?),&trust)?;println!("Signing key created; public trust record saved.");Ok(())},
        Some("sign")=>{let bytes=fs::read(value(args,"--input")?).map_err(|e|e.to_string())?;let key=fs::read(value(args,"--key")?).map_err(|e|e.to_string())?;let envelope=sign(&bytes,&key,&value(args,"--key-id")?)?;save_json(&PathBuf::from(value(args,"--output")?),&envelope)?;Ok(())},
        Some("install"|"rollback"|"recover")=>{
            let action=&args[0];let root=root_path(&PathBuf::from(value(args,"--root")?))?;
            let pid=args.windows(2).find(|a|a[0]=="--wait-pid").map(|a|a[1].parse::<u32>()).transpose().map_err(|e|e.to_string())?.unwrap_or(0);wait_parent(pid)?;
            let deadline=Instant::now()+Duration::from_secs(15);let lock=loop{match RootLock::acquire(&root){Ok(l)=>break l,Err(e)=>{if Instant::now()>=deadline{return Err(e);}std::thread::sleep(Duration::from_millis(200));}}};
            let trust=Trust::bundled();
            let result=(||{recover(&root)?;match action.as_str(){"install"=>{let stage=if let Ok(archive)=value(args,"--archive"){stage_archive(&root,&PathBuf::from(archive),&trust,|_,_|{})?}else{PathBuf::from(value(args,"--stage")?)};install(&root,&stage,&trust)},"rollback"=>rollback(&root),_=>Ok(())}})();
            match &result{Ok(())=>log(&root,&format!("{action}: success at {}",now())),Err(e)=>log(&root,&format!("{action}: {e}"))}
            drop(lock);if args.iter().any(|x|x=="--restart")&&!pending(&root){let _=restart(&root);}result
        },
        _=>Err("用法：ssbu-updater install --root <测试包> --archive <签名更新.zip>；rollback / recover --root <测试包>".into())
    }
}
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => println!("Update operation completed."),
        Err(e) => {
            if let Ok(r) = value(&args, "--root") {
                if let Ok(root) = root_path(&PathBuf::from(r)) {
                    log(&root, &e);
                }
            }
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

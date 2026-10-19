//! Resource-package management. Only installed copies and the user's profile are mutable.
use crate::{ini::Ini, paths::Layout};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::os::windows::ffi::OsStrExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CStr, CString},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
};

use windows_sys::Win32::Foundation::FreeLibrary;

pub static LOCK: Mutex<()> = Mutex::new(());
type Request = unsafe extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_char;
type Free = unsafe extern "C" fn(*mut std::ffi::c_char);
struct Backend {
    module: windows_sys::Win32::Foundation::HMODULE,
    request: Request,
    free: Free,
}
impl Drop for Backend {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.module);
        }
    }
}
impl Backend {
    fn open(path: &Path) -> Result<Self, String> {
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let module = unsafe {
            LoadLibraryExW(
                path.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        };
        if module.is_null() {
            return Err(format!(
                "资源后端无法加载：{}",
                std::io::Error::last_os_error()
            ));
        }
        unsafe {
            let request = GetProcAddress(module, b"sbm_request\0".as_ptr());
            let free = GetProcAddress(module, b"sbm_free\0".as_ptr());
            let version = GetProcAddress(module, b"sbm_api_version\0".as_ptr());
            if request.is_none() || free.is_none() || version.is_none() {
                FreeLibrary(module);
                return Err("资源后端接口不完整".into());
            }
            let version: unsafe extern "C" fn() -> u32 = std::mem::transmute(version.unwrap());
            if version() != 1 {
                FreeLibrary(module);
                return Err("资源后端版本不匹配".into());
            }
            Ok(Self {
                module,
                request: std::mem::transmute(request.unwrap()),
                free: std::mem::transmute(free.unwrap()),
            })
        }
    }
    fn call(&self, request: Value) -> Result<Value, String> {
        let request = CString::new(request.to_string()).map_err(|e| e.to_string())?;
        let reply = unsafe { (self.request)(request.as_ptr()) };
        if reply.is_null() {
            return Err("资源后端没有返回结果".into());
        }
        let parsed = serde_json::from_slice::<Value>(unsafe { CStr::from_ptr(reply).to_bytes() });
        unsafe {
            (self.free)(reply);
        }
        let parsed = parsed.map_err(|e| e.to_string())?;
        if parsed["ok"] != true {
            return Err(parsed["error"]["message"]
                .as_str()
                .unwrap_or("资源后端错误")
                .into());
        }
        Ok(parsed["result"].clone())
    }
}
fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&fs::read(path).map_err(|e| format!("{}：{e}", path.display()))?)
        .map_err(|e| e.to_string())
}
fn supports(l: &Layout) -> Result<(), String> {
    let record = read_json(&l.exe.with_extension("build.json"))
        .map_err(|_| "此构建没有模组支持记录，请使用重新打包的版本".to_string())?;
    // Version 2 adds the optional delay extension to the same hash-bound host record.
    if !matches!(record["schema_version"].as_u64(), Some(1) | Some(2)) {
        return Err("Console 不支持此版本的构建记录，请更新 Console".into());
    }
    if record["exe_sha256"].as_str()
        != Some(
            format!(
                "{:x}",
                Sha256::digest(fs::read(&l.exe).map_err(|e| e.to_string())?)
            )
            .as_str(),
        )
    {
        return Err("游戏程序与构建记录不一致，请重新打包".into());
    }
    if record["resource_mods"] != true {
        return Err("本测试包在构建时未启用资源模组".into());
    }
    if l.package {
        let manifest = read_json(&l.root.join("manifest.json"))?;
        if manifest["features"]["resource_mods"] != true {
            return Err("测试包的模组支持记录不一致，请重新打包".into());
        }
    }
    if !l.game_dir.join("ssbu_mod_backend.dll").is_file() {
        return Err("本测试包缺少资源后端，请完整复制测试包".into());
    }
    Ok(())
}
fn absolute(l: &Layout, value: &str) -> PathBuf {
    let p = PathBuf::from(value);
    if p.is_absolute() {
        p
    } else {
        l.game_dir.join(p)
    }
}
struct Session {
    backend: Backend,
    root: PathBuf,
    profile: PathBuf,
    disabled: bool,
}
fn session(l: &Layout) -> Result<Session, String> {
    supports(l)?;
    let ini = Ini::load(&l.launcher_ini());
    let profile = ini
        .get(Some("env"), "SSBU_MOD_PROFILE")
        .filter(|s| !s.is_empty())
        .map(|s| absolute(l, &s))
        .unwrap_or_else(|| l.config_dir().join("mods.json"));
    let backend = Backend::open(&l.game_dir.join("ssbu_mod_backend.dll"))?;
    let mut config =
        backend.call(json!({"op":"init", "config":{"profile":profile}}))?["config"].clone();
    for (field, default) in [
        ("root", "../mods"),
        ("cache", "../cache/mods"),
        ("userdata", "../userdata"),
    ] {
        config[field] = json!(absolute(l, config[field].as_str().unwrap_or(default)));
    }
    let resource_env = ini
        .get(Some("env"), "SSBU_RESOURCE_MODS")
        .unwrap_or_default();
    if !resource_env.is_empty() && resource_env != "0" {
        config["root"] = json!(absolute(l, &resource_env));
    }
    if let Some(cache) = ini
        .get(Some("env"), "SSBU_MOD_CACHE")
        .filter(|s| !s.is_empty())
    {
        config["cache"] = json!(absolute(l, &cache));
    }
    if let Some(region) = ini
        .get(Some("env"), "SSBU_MOD_REGION")
        .filter(|s| !s.is_empty())
    {
        config["region"] = json!(region);
    }
    if let Some(userdata) = ini.get(Some("run"), "userdata").filter(|s| !s.is_empty()) {
        config["userdata"] = json!(l.full(&userdata));
    }
    if let Some(language) = ini
        .get(Some("env"), "SSBU_LANGUAGE")
        .filter(|s| !s.is_empty())
    {
        config["system_language"] = json!(language);
    }
    config["profile"] = json!(profile);
    backend.call(json!({"op":"configure","config":config}))?;
    let root = PathBuf::from(config["root"].as_str().ok_or("模组目录无效")?);
    Ok(Session {
        backend,
        root,
        profile,
        disabled: resource_env == "0",
    })
}
fn portable(l: &Layout, path: &Path) -> Value {
    let normalized = path.components().fold(PathBuf::new(), |mut a, c| {
        if c == std::path::Component::ParentDir {
            a.pop();
        } else {
            a.push(c.as_os_str());
        }
        a
    });
    let text = normalized.to_string_lossy();
    let normalized = if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", unc))
    } else {
        PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
    };
    match normalized.strip_prefix(&l.root) {
        Ok(relative) => json!(Path::new("..").join(relative)),
        Err(_) => json!(path),
    }
}
fn persist(l: &Layout, s: &Session) -> Result<(), String> {
    let mut config = s.backend.call(json!({"op":"get_config"}))?["config"].clone();
    for field in ["root", "cache", "profile", "userdata"] {
        if let Some(path) = config[field].as_str() {
            config[field] = portable(l, Path::new(path));
        }
    }
    let parent = s.profile.parent().ok_or("配置路径无效")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = parent.join(format!("mods-{}.tmp", std::process::id()));
    fs::write(
        &temp,
        serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(&temp, &s.profile).map_err(|e| format!("配置没有保存：{e}"))
}
fn tags(paths: &[Value]) -> Vec<String> {
    let mut out = BTreeSet::new();
    for p in paths.iter().filter_map(Value::as_str) {
        let tag = if p.starts_with("stream;") || p.starts_with("stream:") {
            if p.contains("movie") || p.ends_with(".webm") {
                "视频"
            } else {
                "音乐"
            }
        } else if p.contains("/message/") || p.ends_with(".xmsbt") {
            "文字"
        } else if p.contains("fighter_voice") {
            "语音"
        } else if p.starts_with("sound/") {
            "音效"
        } else if p.starts_with("effect/") || p.contains("/effect/") {
            "特效"
        } else if p.contains("/motion/") || p.ends_with(".motdiff") || p.ends_with(".yml") {
            "动作"
        } else if p.contains("/param/")
            || p.ends_with(".prc")
            || p.ends_with(".prcx")
            || p.ends_with(".bin")
        {
            "参数"
        } else if p.starts_with("ui/") {
            "界面"
        } else if p.ends_with(".nutexb") || p.ends_with(".bntx") {
            "贴图"
        } else if p.starts_with("stage/") {
            "舞台"
        } else if p.contains("/model/") {
            "模型"
        } else if p.starts_with("camera/") {
            "镜头"
        } else {
            "其他资源"
        };
        out.insert(tag.into());
    }
    out.into_iter().collect()
}
fn inventory(l: &Layout, s: &Session) -> Result<Value, String> {
    let original = s.backend.call(json!({"op":"get_config"}))?["config"].clone();
    let (plan, load_error) = match s.backend.call(json!({"op":"scan"})) {
        Ok(value) => (value["plan"].clone(), None),
        Err(error) => {
            // Keep broken packages manageable: inspect their files with every package disabled.
            let mut inspection = original.clone();
            if s.root.is_dir() {
                for entry in fs::read_dir(&s.root).map_err(|e| e.to_string())? {
                    let entry = entry.map_err(|e| e.to_string())?;
                    if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                        let id = entry.file_name().to_string_lossy().into_owned();
                        if inspection["packages"][&id].is_null() {
                            inspection["packages"][&id] = json!({});
                        }
                        inspection["packages"][&id]["enabled"] = json!(false);
                    }
                }
            }
            s.backend
                .call(json!({"op":"configure","config":inspection}))?;
            let inspected = s.backend.call(json!({"op":"scan"}));
            s.backend
                .call(json!({"op":"configure","config":original}))?;
            (inspected?["plan"].clone(), Some(error))
        }
    };
    let mut conflicts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in plan["entries"].as_array().into_iter().flatten() {
        let owners: BTreeSet<_> = e["candidates"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| s["kind"] == "file")
            .filter_map(|s| s["package"].as_str())
            .collect();
        if owners.len() > 1 {
            for owner in &owners {
                conflicts
                    .entry(owner.to_string())
                    .or_default()
                    .push(e["path"].as_str().unwrap_or_default().into());
            }
        }
    }
    let config = s.backend.call(json!({"op":"get_config"}))?["config"].clone();
    let mut packages = vec![];
    for p in plan["packages"].as_array().into_iter().flatten() {
        let id = p["id"].as_str().unwrap_or_default();
        let paths = p["resources"].as_array().cloned().unwrap_or_default();
        let slots = p["costumes"].as_array().cloned().unwrap_or_default();
        let mut fighters = BTreeSet::new();
        let mut stages = BTreeSet::new();
        for key in slots.iter().filter_map(Value::as_str) {
            if let Some((f, _)) = key.split_once(':') {
                fighters.insert(f.to_string());
            }
        }
        for path in paths.iter().filter_map(Value::as_str) {
            let c: Vec<_> = path.split('/').collect();
            if c.len() > 1 && c[0] == "fighter" {
                fighters.insert(c[1].into());
            }
            if c.len() > 1 && c[0] == "stage" {
                stages.insert(c[1].to_string());
            }
        }
        let plugins = p["plugins"].as_array().cloned().unwrap_or_default();
        let slot_capable = plugins.is_empty()
            && !slots.is_empty()
            && !paths
                .iter()
                .any(|p| p.as_str().is_some_and(|s| s.starts_with("0x")));
        packages.push(json!({"id":id, "alias":config["packages"][id]["alias"].as_str().unwrap_or_default(),
            "enabled":config["packages"][id]["enabled"].as_bool().unwrap_or(true), "priority":p["priority"], "path":p["path"], "tags":tags(&paths),
            "fighters":fighters, "stages":stages, "costumes":slots, "slot_map":config["packages"][id]["slot_map"].as_object().cloned().unwrap_or_default(),
            "resource_count":paths.len(), "paths":paths.into_iter().take(100).collect::<Vec<_>>(), "plugins":plugins,
            "slot_capable":slot_capable, "conflicts":conflicts.get(id).cloned().unwrap_or_default()}));
    }
    let _ = l;
    Ok(
        json!({"supported":true,"reason":"此构建支持资源模组", "root":s.root,"profile":s.profile,
        "disabled_for_run":s.disabled,"packages":packages,"diagnostics":plan["diagnostics"],"load_error":load_error}),
    )
}
pub fn list(l: &Layout) -> Result<Value, String> {
    if let Err(reason) = supports(l) {
        return Ok(
            json!({"supported":false,"reason":reason,"packages":[],"root":l.root.join("mods")}),
        );
    }
    let s = session(l)?;
    inventory(l, &s)
}
pub fn update(l: &Layout, id: &str, patch: Value) -> Result<Value, String> {
    let s = session(l)?;
    package_path(&s.root, id)?;
    let mut request = json!({"op":"set_package","id":id});
    for key in ["alias", "enabled", "slot_map"] {
        if let Some(v) = patch.get(key) {
            request[key] = v.clone();
        }
    }
    s.backend.call(request)?;
    if patch.get("slot_map").is_some() {
        // Validate even a disabled package; no unsupported mapping can be saved silently.
        let enabled = s.backend.call(json!({"op":"get_config"}))?["config"]["packages"][id]
            ["enabled"]
            .as_bool()
            .unwrap_or(true);
        s.backend
            .call(json!({"op":"set_package","id":id,"enabled":true}))?;
        s.backend
            .call(json!({"op":"scan"}))
            .map_err(|e| format!("槽位调整没有保存：{e}"))?;
        s.backend
            .call(json!({"op":"set_package","id":id,"enabled":enabled}))?;
    }
    // Confirm the resulting plan before replacing the saved profile.
    if patch["enabled"] == true || patch.get("slot_map").is_some() {
        s.backend.call(json!({"op":"scan"}))?;
    }
    persist(l, &s)?;
    inventory(l, &s)
}
fn not_link(path: &Path) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    if fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .file_attributes()
        & 0x400
        != 0
    {
        return Err("不能操作链接或重解析目录，请使用真实文件夹".into());
    }
    Ok(())
}
fn package_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    if id.is_empty() || id.starts_with('.') || id.contains(['/', '\\', ':']) {
        return Err("模组名称无效".into());
    }
    not_link(root)?;
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let p = root.join(id);
    not_link(&p)?;
    let real = fs::canonicalize(&p).map_err(|e| e.to_string())?;
    if real.parent() != Some(root.as_path()) || !real.is_dir() {
        return Err("模组路径超出安装目录".into());
    }
    Ok(real)
}
pub fn delete(l: &Layout, id: &str) -> Result<Value, String> {
    let s = session(l)?;
    let source = package_path(&s.root, id)?;
    let trash = s.root.join(".trash");
    fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
    not_link(&trash)?;
    let destination = trash.join(format!(
        "{}-{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S-%f"),
        id
    ));
    fs::rename(source, destination).map_err(|e| format!("模组没有移入回收区：{e}"))?;
    inventory(l, &s)
}
fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
    not_link(source)?;
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        not_link(&path)?;
        let to = target.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &to)?;
        } else {
            fs::copy(path, to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn payload(path: &Path) -> bool {
    [
        "fighter",
        "sound",
        "ui",
        "stage",
        "effect",
        "camera",
        "stream;",
        "stream:",
        "config.json",
        "info.toml",
    ]
    .iter()
    .any(|s| path.join(s).exists())
        || fs::read_dir(path)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("0x"))
}
fn discover(path: &Path, depth: usize) -> Result<Vec<PathBuf>, String> {
    if depth > 7 {
        return Ok(vec![]);
    }
    if payload(path) {
        return Ok(vec![path.into()]);
    }
    let mut out = vec![];
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_dir()
            && !entry.file_name().to_string_lossy().starts_with('.')
        {
            out.extend(discover(&entry.path(), depth + 1)?);
        }
    }
    Ok(out)
}
fn extract(source: &Path, target: &Path) -> Result<(), String> {
    let file = fs::File::open(source).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("ZIP 无法读取：{e}"))?;
    if archive.len() > 100_000 {
        return Err("压缩包文件数量过多".into());
    }
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.enclosed_name().ok_or("压缩包包含越界路径")?;
        if name
            .components()
            .any(|c| c.as_os_str().to_string_lossy().contains([':', '\\']))
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
        {
            return Err("压缩包包含链接或无效路径".into());
        }
        total = total.checked_add(entry.size()).ok_or("压缩包过大")?;
        if total > 16 * 1024 * 1024 * 1024 {
            return Err("解压内容超过 16 GB，请手动解压后导入文件夹".into());
        }
        let to = target.join(name);
        if entry.is_dir() {
            fs::create_dir_all(to).map_err(|e| e.to_string())?;
        } else {
            fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
            let mut output = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(to)
                .map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut output).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
pub fn import(l: &Layout, source: &Path) -> Result<Value, String> {
    let s = session(l)?;
    fs::create_dir_all(&s.root).map_err(|e| e.to_string())?;
    not_link(&s.root)?;
    let root = fs::canonicalize(&s.root).map_err(|e| e.to_string())?;
    let source = fs::canonicalize(source).map_err(|e| e.to_string())?;
    if source.starts_with(&root) || root.starts_with(&source) {
        return Err("请选择模组安装目录以外的下载文件或文件夹".into());
    }
    let staging = root.join(format!(
        ".import-{}-{}",
        std::process::id(),
        chrono::Local::now()
            .timestamp_nanos_opt()
            .unwrap_or_default()
    ));
    fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let original_name = if source.is_dir() {
        source.file_name()
    } else {
        source.file_stem()
    }
    .and_then(|s| s.to_str())
    .unwrap_or("Imported Mod");
    if source.is_dir() {
        copy_tree(&source, &staging.join(original_name))?;
    } else if source
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("zip"))
    {
        extract(&source, &staging)?;
    } else {
        return Err("请选择 ZIP 压缩包或已经解压的模组文件夹".into());
    }
    let found = discover(&staging, 0)?;
    if found.is_empty() {
        return Err("没有找到模组资源目录（fighter、ui、sound、stage 等）".into());
    }
    let ready = staging.join(".ready");
    // A flat archive is already the package; move it to a separate ready folder.
    let flat = found.iter().any(|p| p == &staging);
    let ready = if flat {
        root.join(format!(
            ".import-ready-{}",
            chrono::Local::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
        ))
    } else {
        ready
    };
    fs::create_dir(&ready).map_err(|e| e.to_string())?;
    let mut imported = vec![];
    for path in found {
        let name = if path == staging {
            original_name
        } else {
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(original_name)
        };
        if name.starts_with('.') || name.contains(['/', '\\', ':']) {
            return Err("压缩包的模组目录名无效".into());
        }
        let mut id = name.to_string();
        let mut number = 2;
        while root.join(&id).exists() || ready.join(&id).exists() {
            id = format!("{name} ({number})");
            number += 1;
        }
        fs::rename(&path, ready.join(&id)).map_err(|e| e.to_string())?;
        s.backend
            .call(json!({"op":"set_package","id":id,"enabled":true,"alias":"","slot_map":{}}))?;
        imported.push(id);
    }
    // Validate all imported packages before publishing any of them.
    s.backend
        .call(json!({"op":"configure","config":{"root":ready}}))?;
    s.backend
        .call(json!({"op":"scan"}))
        .map_err(|e| format!("模组没有导入：{e}"))?;
    let mut moved = vec![];
    for id in &imported {
        if let Err(error) = fs::rename(ready.join(id), root.join(id)) {
            for previous in &moved {
                let _ = fs::rename(root.join(previous), ready.join(previous));
            }
            return Err(format!("模组没有导入：{error}"));
        }
        moved.push(id.clone());
    }
    s.backend
        .call(json!({"op":"configure","config":{"root":root}}))?;
    persist(l, &s)?;
    if flat {
        let _ = fs::remove_dir(&ready);
    }
    if staging.exists()
        && staging.parent() == Some(root.as_path())
        && staging
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".import-")
    {
        let _ = fs::remove_dir_all(&staging);
    }
    let mut out = inventory(l, &s)?;
    out["imported"] = json!(imported);
    Ok(out)
}
pub fn open(l: &Layout, id: Option<&str>) -> Result<Value, String> {
    let s = session(l)?;
    fs::create_dir_all(&s.root).map_err(|e| e.to_string())?;
    let path = if let Some(id) = id {
        package_path(&s.root, id)?
    } else {
        s.root
    };
    crate::win::open_folder(&path)?;
    Ok(json!(true))
}
pub fn pick(kind: &str) -> Result<Value, String> {
    use std::os::windows::process::CommandExt;
    let script = match kind {
        "folder" => "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description='选择已解压的模组文件夹'; if($d.ShowDialog() -eq 'OK'){[Console]::OutputEncoding=[Text.UTF8Encoding]::new(); [Console]::Write($d.SelectedPath)}",
        "zip" => "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.OpenFileDialog; $d.Filter='ZIP (*.zip)|*.zip'; $d.Title='导入模组'; if($d.ShowDialog() -eq 'OK'){[Console]::OutputEncoding=[Text.UTF8Encoding]::new(); [Console]::Write($d.FileName)}",
        _ => return Err("导入类型无效".into()),
    };
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-STA", "-Command", script])
        .creation_flags(crate::game::CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("文件选择窗口无法打开".into());
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(if path.is_empty() {
        Value::Null
    } else {
        json!(path)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_packaged_feature_records_and_rejects_mismatches() {
        let root = std::env::temp_dir().join(format!("ssbu-feature-record-test-{}", std::process::id()));
        let game = root.join("game");
        fs::create_dir_all(&game).unwrap();
        let exe = game.join("ssbu_native.exe");
        fs::write(&exe, b"host fixture").unwrap();
        fs::write(game.join("ssbu_mod_backend.dll"), b"backend fixture").unwrap();
        let l = Layout { root: root.clone(), game_dir: game, exe: exe.clone(), package: true };
        let manifest = root.join("manifest.json");
        fs::write(&manifest, json!({"features":{"resource_mods":true,"latency_slider":true}}).to_string()).unwrap();
        let mut record = json!({"schema_version":2,"resource_mods":true,"latency_slider":true,
            "exe_sha256":format!("{:x}", Sha256::digest(b"host fixture"))});
        let stamp = exe.with_extension("build.json");
        for schema in [1, 2] {
            record["schema_version"] = json!(schema);
            fs::write(&stamp, record.to_string()).unwrap();
            supports(&l).unwrap();
        }
        for schema in [json!(0), json!(3), json!("2")] {
            record["schema_version"] = schema;
            fs::write(&stamp, record.to_string()).unwrap();
            assert!(supports(&l).is_err());
        }
        record["schema_version"] = json!(2);
        record["resource_mods"] = json!(false);
        fs::write(&stamp, record.to_string()).unwrap();
        assert!(supports(&l).unwrap_err().contains("未启用"));
        record["resource_mods"] = json!(true);
        fs::write(&stamp, record.to_string()).unwrap();
        fs::write(&exe, b"changed host").unwrap();
        assert!(supports(&l).unwrap_err().contains("不一致"));
        fs::write(&exe, b"host fixture").unwrap();
        fs::write(&manifest, json!({"features":{"resource_mods":false}}).to_string()).unwrap();
        assert!(supports(&l).unwrap_err().contains("不一致"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn recognizes_contents_and_wrapper_layout() {
        let tags = tags(&[
            json!("fighter/sonic/model/body/c00/model.numdlb"),
            json!("sound/bank/fighter_voice/vc_sonic_c00.nus3audio"),
            json!("stream;/sound/bgm/test.nus3audio"),
        ]);
        assert!(
            tags.contains(&"模型".into())
                && tags.contains(&"语音".into())
                && tags.contains(&"音乐".into())
        );
        let root =
            std::env::temp_dir().join(format!("ssbu-console-import-test-{}", std::process::id()));
        let skin = root.join("ultimate/mods/Test Skin/fighter");
        fs::create_dir_all(&skin).unwrap();
        assert_eq!(
            discover(&root, 0).unwrap(),
            vec![skin.parent().unwrap().to_path_buf()]
        );
        assert!(package_path(&root, "../escape").is_err());
        assert!(package_path(&root, "C:escape").is_err());
        assert!(package_path(&root, ".trash").is_err());
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    #[ignore = "set SSBU_TEST_PACKAGE to a built package; no game is launched"]
    fn packaged_mod_support_uses_real_host_record() {
        let root = PathBuf::from(std::env::var("SSBU_TEST_PACKAGE").expect("SSBU_TEST_PACKAGE"));
        let game_dir = root.join("game");
        let l = Layout { exe: game_dir.join("ssbu_native.exe"), game_dir, root, package: true };
        let value = list(&l).unwrap();
        assert_eq!(value["supported"], true, "{value}");
    }
    #[test]
    #[ignore = "set SSBU_TEST_MOD_BACKEND to the built DLL; no game is launched"]
    fn installed_package_management_round_trip() {
        let backend =
            PathBuf::from(std::env::var("SSBU_TEST_MOD_BACKEND").expect("SSBU_TEST_MOD_BACKEND"));
        let root = std::env::temp_dir().join(format!(
            "ssbu-mod-management-{}-{}",
            std::process::id(),
            chrono::Local::now().timestamp_nanos_opt().unwrap()
        ));
        let package = root.join("package");
        let game = package.join("game");
        fs::create_dir_all(&game).unwrap();
        let exe = game.join("ssbu_native.exe");
        fs::write(&exe, b"host fixture").unwrap();
        fs::write(exe.with_extension("build.json"),json!({"schema_version":2,"resource_mods":true,"latency_slider":true,"exe_sha256":format!("{:x}",Sha256::digest(b"host fixture"))}).to_string()).unwrap();
        fs::copy(backend, game.join("ssbu_mod_backend.dll")).unwrap();
        let l = Layout {
            root: package.clone(),
            game_dir: game,
            exe: exe.clone(),
            package: true,
        };
        fs::write(package.join("manifest.json"), json!({"features":{"resource_mods":true,"latency_slider":true}}).to_string()).unwrap();
        let download = root.join("Downloaded Skin");
        let source = download.join("fighter/sonic/model/body/c00/model.numdlb");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"author's unmodified resource").unwrap();
        let imported = import(&l, &download).unwrap();
        let id = imported["imported"][0].as_str().unwrap();
        assert_eq!(id, "Downloaded Skin");
        let changed = update(&l, id, json!({"alias":"Test alias","enabled":false})).unwrap();
        assert_eq!(changed["packages"][0]["alias"], "Test alias");
        assert_eq!(changed["packages"][0]["enabled"], false);
        assert_eq!(changed["packages"][0]["resource_count"], 1);
        let changed = update(&l, id, json!({"slot_map":{"sonic:c00":2}})).unwrap();
        assert_eq!(changed["packages"][0]["slot_map"]["sonic:c00"], 2);
        assert!(package
            .join("mods")
            .join(id)
            .join("fighter/sonic/model/body/c00/model.numdlb")
            .is_file());
        assert_eq!(fs::read(&source).unwrap(), b"author's unmodified resource");
        let saved = read_json(&package.join("config/mods.json")).unwrap();
        assert_eq!(
            PathBuf::from(saved["root"].as_str().unwrap()),
            PathBuf::from("../mods")
        );
        // A broken active config must not make its disable/delete controls inaccessible.
        let broken = package.join("mods/Broken/config.json");
        fs::create_dir_all(broken.parent().unwrap()).unwrap();
        fs::write(&broken, b"invalid json").unwrap();
        let broken_list = list(&l).unwrap();
        assert!(broken_list["load_error"].is_string());
        assert_eq!(broken_list["packages"].as_array().unwrap().len(), 2);
        update(&l, "Broken", json!({"enabled":false})).unwrap();
        assert!(list(&l).unwrap()["load_error"].is_null());
        let deleted = delete(&l, id).unwrap();
        assert_eq!(deleted["packages"].as_array().unwrap().len(), 1);
        assert!(!package.join("mods").join(id).exists());
        assert_eq!(
            fs::read_dir(package.join("mods/.trash")).unwrap().count(),
            1
        );
        // Exercise ZIP wrapper discovery and ensure a traversal entry cannot escape extraction.
        let archive_path = root.join("music.zip");
        let mut archive = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
        archive
            .start_file(
                "ultimate/mods/Music/stream;/sound/bgm/test.nus3audio",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        std::io::Write::write_all(&mut archive, b"test stream").unwrap();
        archive.finish().unwrap();
        assert_eq!(import(&l, &archive_path).unwrap()["imported"][0], "Music");
        let bad = root.join("bad.zip");
        let mut archive = zip::ZipWriter::new(fs::File::create(&bad).unwrap());
        archive
            .start_file("../escape.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut archive, b"bad").unwrap();
        archive.finish().unwrap();
        let scratch = root.join("extract");
        fs::create_dir(&scratch).unwrap();
        assert!(extract(&bad, &scratch).is_err());
        assert!(!root.join("escape.txt").exists());
        // Build support is detected from the executable, not the presence of a stray backend DLL.
        fs::write(exe.with_extension("build.json"),json!({"schema_version":1,"resource_mods":false,"exe_sha256":format!("{:x}",Sha256::digest(b"host fixture"))}).to_string()).unwrap();
        assert_eq!(list(&l).unwrap()["supported"], false);
        fs::remove_dir_all(root).unwrap();
    }
}

//! Native backend for resource packages. The only writable outputs are profiles and
//! content-addressed patch cache blobs; original archives and mods are opened read-only.
mod archive;
mod inference;
mod packages;
mod patches;
mod paths;
mod slots;
use packages::{Config, Plan};
use serde_json::{json, Value};
use std::{
    ffi::{c_char, CStr, CString},
    sync::{
        atomic::{AtomicU32, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock, RwLock,
    },
};
struct State {
    config: Config,
    plan: Option<Plan>,
    active_config: Option<Config>,
    error: String,
}
static PREPARE_LOCK: Mutex<()> = Mutex::new(());
static PHASE: AtomicU32 = AtomicU32::new(0);
static DONE: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);
pub(crate) fn progress(phase: u32, done: usize, total: usize) {
    TOTAL.store(total, Ordering::Relaxed);
    DONE.store(done, Ordering::Relaxed);
    PHASE.store(phase, Ordering::Release);
}
fn progress_value() -> Value {
    let phase = PHASE.load(Ordering::Acquire);
    let name = ["idle", "scan", "materialize", "metadata", "ready", "failed"]
        .get(phase as usize)
        .copied()
        .unwrap_or("failed");
    json!({"phase":name,"done":DONE.load(Ordering::Relaxed),"total":TOTAL.load(Ordering::Relaxed)})
}

fn restart(s: &State) -> bool {
    matches!(PHASE.load(Ordering::Acquire), 1..=3)
        || s.active_config.as_ref().is_some_and(|c| c != &s.config)
}
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static ACTIVE: OnceLock<RwLock<Option<Arc<archive::VirtualArchive>>>> = OnceLock::new();
fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| {
        Mutex::new(State {
            config: Config::default(),
            plan: None,
            active_config: None,
            error: String::new(),
        })
    })
}
fn active() -> &'static RwLock<Option<Arc<archive::VirtualArchive>>> {
    ACTIVE.get_or_init(|| RwLock::new(None))
}
thread_local! {static ERROR:std::cell::RefCell<CString>=std::cell::RefCell::new(CString::new("").unwrap());}
fn set_error(e: &str) {
    ERROR.with(|v| *v.borrow_mut() = CString::new(e.replace('\0', " ")).unwrap());
}
fn merge(base: &mut Value, patch: &Value) {
    if let (Some(b), Some(p)) = (base.as_object_mut(), patch.as_object()) {
        for (k, v) in p {
            b.insert(k.clone(), v.clone());
        }
    }
}
fn config(v: Value) -> Result<Config, String> {
    let c: Config = serde_json::from_value(v).map_err(|e| e.to_string())?;
    if c.region != "auto" {
        paths::region(&c.region)?;
    }
    if !matches!(c.conflict_mode.as_str(), "priority" | "error") {
        return Err("invalid conflict_mode".into());
    }
    Ok(c)
}
fn request(v: Value) -> Result<Value, String> {
    let op = v.get("op").and_then(Value::as_str).ok_or("missing op")?;
    if op == "capabilities" {
        return Ok(
            json!({"api_version":1,"game_version":"13.0.4","archive_view":"read-only original with virtual metadata and groups","features":["replacement","uncompressed","addition","directories","stream","region","unshare","share","profiles","priority","conflicts","slot_mapping","prcx","prcxml","stdatx","stdatxml","stprmx","stprmxml","xmsbt","patch3audio","motion_list.yml","motdiff","bgm_property.bin"],"plugin_nro":false,"hot_reload":false}),
        );
    }
    if op == "open" {
        let path = v
            .get("path")
            .and_then(Value::as_str)
            .ok_or("missing archive path")?;
        prepare(path)?;
        let g = active().read().map_err(|e| e.to_string())?;
        return Ok(json!({"archive":g.as_ref().map(|a|&a.stats),"diagnostics":g.as_ref().map(|a|&a.diagnostics)}));
    }
    let mut s = state().lock().map_err(|e| e.to_string())?;
    match op {
        "init" | "configure" => {
            let patch = v.get("config").cloned().unwrap_or(json!({}));
            let mut next = serde_json::to_value(&s.config).map_err(|e| e.to_string())?;
            if op == "init" {
                let profile = patch
                    .get("profile")
                    .and_then(Value::as_str)
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| s.config.profile.clone());
                if profile.is_file() {
                    next = serde_json::from_slice(
                        &std::fs::read(&profile).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            merge(&mut next, &patch);
            s.config = config(next)?;
            s.plan = None;
            Ok(json!({"config":s.config,"restart_required":restart(&s)}))
        }
        "get_config" => Ok(json!({"config":s.config})),
        "set_package" => {
            let id = v
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing package id")?;
            if id.is_empty() || id.contains(['/', '\\']) || id == ".." {
                return Err("invalid package id".into());
            }
            let mut sel = s.config.packages.get(id).cloned().unwrap_or_default();
            if let Some(e) = v.get("enabled") {
                sel.enabled = e.as_bool().ok_or("enabled must be boolean")?;
            }
            if let Some(p) = v.get("priority") {
                sel.priority = p
                    .as_i64()
                    .ok_or("priority must be an integer")?
                    .try_into()
                    .map_err(|_| "priority out of range")?;
            }
            if let Some(alias) = v.get("alias") {
                sel.alias = alias
                    .as_str()
                    .ok_or("alias must be text")?
                    .trim()
                    .chars()
                    .take(120)
                    .collect();
            }
            if let Some(map) = v.get("slot_map") {
                sel.slot_map = serde_json::from_value(map.clone()).map_err(|e| e.to_string())?;
                slots::validate(&sel.slot_map)?;
            }
            s.config.packages.insert(id.into(), sel);
            s.plan = None;
            Ok(json!({"restart_required":restart(&s)}))
        }
        "save_profile" => {
            packages::save(&s.config)?;
            Ok(json!({"profile":s.config.profile,"restart_required":restart(&s)}))
        }
        "load_profile" => {
            let path = v
                .get("path")
                .and_then(Value::as_str)
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| s.config.profile.clone());
            let next = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            s.config = config(next)?;
            s.plan = None;
            Ok(json!({"config":s.config,"restart_required":restart(&s)}))
        }
        "scan" | "plan" => {
            let p = packages::scan(&s.config)?;
            let out = json!({"plan":p,"restart_required":restart(&s)});
            s.plan = Some(p);
            Ok(out)
        }
        "status" => {
            let a = active().read().map_err(|e| e.to_string())?;
            Ok(
                json!({"active":a.is_some(),"archive":a.as_ref().map(|a|&a.source),"stats":a.as_ref().map(|a|&a.stats),"diagnostics":a.as_ref().map(|a|&a.diagnostics),"active_config":s.active_config,"config":s.config,"restart_required":restart(&s),"progress":progress_value(),"error":s.error}),
            )
        }
        _ => Err(format!("unknown operation {op}")),
    }
}
fn prepare(path: &str) -> Result<u64, String> {
    let _serial = PREPARE_LOCK.lock().map_err(|e| e.to_string())?;
    {
        let a = active().read().map_err(|e| e.to_string())?;
        if let Some(a) = a.as_ref() {
            if std::fs::canonicalize(&a.source).map_err(|e| e.to_string())?
                != std::fs::canonicalize(path).map_err(|e| e.to_string())?
            {
                return Err("another archive is already active".into());
            }
            return Ok(a.size);
        }
    }
    let config = state().lock().map_err(|e| e.to_string())?.config.clone();
    let result = guard(|| {
        progress(1, 0, 0);
        let p = packages::scan(&config)?;
        progress(2, 0, p.entries.len());
        let a = archive::VirtualArchive::build(std::path::Path::new(path), &config, &p)?;
        for diagnostic in p.diagnostics.iter().chain(&a.diagnostics) {
            println!("mods: [{}] {}", diagnostic.code, diagnostic.message);
        }
        println!(
            "mods: {} replacements, {} additions, {} streams, {} patched inputs, {} groups",
            a.stats.replacements,
            a.stats.additions,
            a.stats.streams,
            a.stats.patches,
            a.stats.groups
        );
        let size = a.size;
        *active().write().map_err(|e| e.to_string())? = Some(Arc::new(a));
        let mut s = state().lock().map_err(|e| e.to_string())?;
        s.active_config = Some(config.clone());
        if s.config == config {
            s.plan = Some(p);
        }
        s.error.clear();
        progress(4, 0, 0);
        Ok(size)
    });
    if let Err(e) = &result {
        progress(5, 0, 0);
        if let Ok(mut s) = state().lock() {
            s.error = e.clone();
        }
    }
    result
}

fn guard<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
        Err(
            "resource parser rejected malformed input (panic contained at the backend boundary)"
                .into(),
        )
    })
}
unsafe fn text<'a>(p: *const c_char) -> Result<&'a str, String> {
    if p.is_null() {
        return Err("null string".into());
    }
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map_err(|e| e.to_string())
}
#[no_mangle]
pub extern "C" fn sbm_api_version() -> u32 {
    1
}
/// Returned UTF-8 JSON must be released with sbm_free on the same library instance.
#[no_mangle]
pub unsafe extern "C" fn sbm_request(p: *const c_char) -> *mut c_char {
    let result = guard(|| {
        let v: Value = serde_json::from_str(unsafe { text(p) }?).map_err(|e| e.to_string())?;
        request(v)
    });
    let out = match result {
        Ok(v) => {
            set_error("");
            json!({"ok":true,"result":v})
        }
        Err(e) => {
            set_error(&e);
            json!({"ok":false,"error":{"code":"mod_backend_error","message":e}})
        }
    };
    CString::new(out.to_string()).unwrap().into_raw()
}
#[no_mangle]
pub unsafe extern "C" fn sbm_free(p: *mut c_char) {
    if !p.is_null() {
        drop(unsafe { CString::from_raw(p) });
    }
}
#[no_mangle]
pub unsafe extern "C" fn sbm_prepare(p: *const c_char) -> u64 {
    match guard(|| prepare(unsafe { text(p) }?)) {
        Ok(n) => n,
        Err(e) => {
            set_error(&e);
            0
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn sbm_read(offset: u64, buffer: *mut u8, len: usize) -> i64 {
    if len > 0 && buffer.is_null() {
        set_error("null read buffer");
        return -1;
    }
    match guard(|| {
        let a = active()
            .read()
            .map_err(|e| e.to_string())?
            .clone()
            .ok_or("archive not prepared")?;
        let b = if len == 0 {
            &mut []
        } else {
            unsafe { std::slice::from_raw_parts_mut(buffer, len) }
        };
        a.read(offset, b)
    }) {
        Ok(n) => n as i64,
        Err(e) => {
            set_error(&e);
            -1
        }
    }
}
#[no_mangle]
pub extern "C" fn sbm_size() -> u64 {
    active()
        .read()
        .ok()
        .and_then(|a| a.as_ref().map(|a| a.size))
        .unwrap_or(0)
}
#[no_mangle]
pub extern "C" fn sbm_last_error() -> *const c_char {
    ERROR.with(|e| e.borrow().as_ptr())
}
#[cfg(test)]
mod tests;

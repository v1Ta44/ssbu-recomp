//! A run of the game: the session folder, the environment, the process, the log, the verdict and the report zip.
//! The same files as the PowerShell launcher wrote (tools/package/launch.ps1), so Replay.bat replays a console run:
//! session.json (env, build_id, save_snapshot ...), input.rec, save_before.zip, config_before\, fight_hash.txt.
use crate::bridge::Bridge;
use crate::ini::Ini;
use crate::paths::Layout;
use serde_json::{json, Map, Value};
use std::collections::VecDeque;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Run {
    pub session: PathBuf,
    pub name: String,
    pub mode: String,
    pub pid: u32,
    pub started: chrono::DateTime<chrono::Local>,
    pub t0: Instant,
    pub info: Map<String, Value>,
    pub child: Option<Child>,
    pub finished: Option<Value>,
}

pub struct Games {
    pub run: Mutex<Option<Run>>,
    pub log: Mutex<VecDeque<String>>,
    pub log_seq: Mutex<u64>,
}

impl Games {
    pub fn new() -> Games {
        Games { run: Mutex::new(None), log: Mutex::new(VecDeque::new()), log_seq: Mutex::new(0) }
    }
}

fn exit_meaning(code: i64) -> String {
    let t = match code {
        0 => "正常退出",
        1 => "加载失败 (或进程被外部结束)",
        2 => "找不到 data.arc, 或游戏代码陷阱",
        3 => "未翻译的指令",
        4 => "宿主异常 (崩溃; 见 ssbu_native_crash.txt 和 .dmp)",
        5 => "Vulkan 致命错误 / 没有 Vulkan 驱动 / SSBU_STRICT 停止",
        6 => "Vulkan 初始化失败 (例如 ssbu_glslang.dll)",
        7 => "缺少 main.npdm",
        8 => "模组资源加载失败：请查看日志中的 mods: 错误，并在模组页停用相关包",
        -1073741819 => "0xC0000005 访问冲突 (崩溃处理之外)",
        -1073740791 => "0xC0000409 快速失败 / 栈缓冲区溢出",
        -1073741571 => "0xC00000FD 栈溢出",
        -1073741515 => "0xC0000135 找不到 DLL",
        -1073741701 => "0xC000007B 映像错误 (32/64 位 DLL 混用)",
        -1073741510 => "0xC000013A 被 Ctrl+C / 控制台关闭",
        -1073741795 => "0xC000001D 非法指令 (CPU 缺少某个特性)",
        _ => return format!("退出码 0x{:08X}", code as u32),
    };
    t.to_string()
}

// ---------------------------------------------------------------- the package and this PC
pub fn manifest(l: &Layout) -> Option<Value> {
    let t = fs::read_to_string(l.root.join("manifest.json")).ok()?;
    serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()
}

pub fn build_id(l: &Layout) -> String {
    manifest(l).and_then(|m| m["build_id"].as_str().map(String::from)).unwrap_or_else(|| if l.package { "unknown".into() } else { "dev".into() })
}

pub struct Preflight {
    pub lines: Vec<String>,
    pub fatal: Vec<String>,
    pub package_bad: usize,
}

pub fn preflight(l: &Layout, romfs: &Path) -> Preflight {
    let mut lines = Vec::new();
    let mut fatal = Vec::new();
    if !l.exe.is_file() {
        fatal.push(format!("找不到游戏程序 {}", l.exe.display()));
    }
    let arc = romfs.join("data.arc");
    match fs::metadata(&arc) {
        Ok(m) => {
            lines.push(format!("data.arc: {}, {} bytes", arc.display(), m.len()));
            if let Some(want) = manifest(l).and_then(|m| m["romfs"]["size"].as_u64()) {
                if want != m.len() {
                    lines.push(format!("WARNING: data.arc 有 {} 字节, 本 build 用的是 {} 字节 (另一个游戏版本或损坏的副本)", m.len(), want));
                }
            }
        }
        Err(_) => fatal.push(format!("{} 里没有 data.arc (在 开始 页设置 RomFS 文件夹)", romfs.display())),
    }
    // .nrr\*.nrr: the lists of the script modules; without them the game waits for ever before the first fight
    let nrr = fs::read_dir(romfs.join(".nrr")).map(|d| d.filter_map(|e| e.ok()).filter(|e| e.path().extension().map_or(false, |x| x.eq_ignore_ascii_case("nrr"))).count()).unwrap_or(0);
    if nrr == 0 {
        fatal.push(format!("{}\\.nrr\\*.nrr 不存在: 游戏会拒绝所有脚本模块, 停在对战前的加载画面。请把 RomFS 里与 data.arc 并列的 .nrr 文件夹一起复制过来", romfs.display()));
    } else {
        lines.push(format!("RomFS .nrr: {} files", nrr));
    }
    let sys = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())).join("System32");
    if !sys.join("mfplat.dll").is_file() {
        fatal.push("缺少 Media Foundation (mfplat.dll; Windows N 版): 请安装媒体功能包".into());
    }
    if !sys.join("vulkan-1.dll").is_file() {
        lines.push("WARNING: 没有 Vulkan 驱动 (vulkan-1.dll); 游戏需要 Vulkan 1.3, 或在 画面 页把渲染器换成 OpenGL".into());
    }
    let root = l.root.to_string_lossy();
    if root.chars().any(|c| !(' '..='~').contains(&c)) {
        lines.push(format!("WARNING: 包的路径含有非 ASCII 字符 ({}); 游戏用 ANSI 路径打开文件, 系统代码页之外的字符会出错", root));
    }
    let mut bad = 0;
    if let Some(m) = manifest(l) {
        if let Some(files) = m["files"].as_object() {
            for (name, want) in files {
                let p = l.root.join(name.replace('/', "\\"));
                match fs::metadata(&p) {
                    Err(_) => {
                        lines.push(format!("package: MISSING  {}", name));
                        bad += 1;
                    }
                    Ok(md) if Some(md.len()) != want["size"].as_u64() => {
                        lines.push(format!("package: SIZE     {}: {} bytes, expected {}", name, md.len(), want["size"]));
                        bad += 1;
                    }
                    _ => {}
                }
            }
            lines.push(format!("package: {} files of the manifest checked (size): {} problem(s)", files.len(), bad));
            if bad > 0 {
                lines.push("WARNING: 包不完整或已损坏 (见上面几行): 请重新复制".into());
            }
        }
    } else if l.package {
        lines.push("package: manifest.json missing: the package cannot be checked".into());
    }
    Preflight { lines, fatal, package_bad: bad }
}

/// system.txt / events.txt: what Windows knows, collected by PowerShell (no Python on a tester's PC). Best effort.
const SYSTEM_PS: &str = r#"
$ErrorActionPreference='SilentlyContinue'
"collected $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss zzz') on $env:COMPUTERNAME"
$os=Get-CimInstance Win32_OperatingSystem; $cv=Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
"OS: {0} {1} (build {2}.{3}), memory {4:N1} GB, free {5:N1} GB" -f $os.Caption,$cv.DisplayVersion,$os.BuildNumber,$cv.UBR,($os.TotalVisibleMemorySize/1MB),($os.FreePhysicalMemory/1MB)
Get-CimInstance Win32_Processor | % { "CPU: {0}, {1} cores / {2} logical processors" -f $_.Name.Trim(),$_.NumberOfCores,$_.NumberOfLogicalProcessors }
Get-CimInstance Win32_VideoController | % { "GPU: {0}; driver {1} ({2}); {3}, {4} Hz" -f $_.Name,$_.DriverVersion,$(if($_.DriverDate){$_.DriverDate.ToString('yyyy-MM-dd')}),$_.VideoModeDescription,$_.CurrentRefreshRate }
$dll=Join-Path $env:SystemRoot 'System32\vulkan-1.dll'; if(Test-Path $dll){"Vulkan loader: "+(Get-Item $dll).VersionInfo.FileVersion}else{'Vulkan loader: NOT FOUND'}
foreach($k in 'HKLM:\SOFTWARE\Khronos\Vulkan\Drivers','HKLM:\SOFTWARE\Khronos\Vulkan\ImplicitLayers'){ if(Test-Path $k){ (Get-Item $k).Property | % { "  {0}: {1}" -f (Split-Path $k -Leaf),$_ } } }
Get-CimInstance Win32_PnPEntity -Filter "PNPClass='HIDClass' OR PNPClass='XnaComposite' OR PNPClass='XboxComposite'" | ? { $_.Name -match 'game|controller|gamepad|xbox|joystick|dualsense|dualshock|手柄|游戏' } | % { "controller: {0} ({1})" -f $_.Name,$_.Status }
'power: '+((powercfg /getactivescheme) -join ' ')
"locale: {0}, UI {1}, time zone {2}" -f (Get-Culture).Name,(Get-UICulture).Name,[TimeZoneInfo]::Local.Id
"#;

fn events_ps(since: &chrono::DateTime<chrono::Local>) -> String {
    format!(
        r#"$ErrorActionPreference='SilentlyContinue'; $since=[datetime]'{}'; $n=0
Get-WinEvent -FilterHashtable @{{LogName='Application';StartTime=$since;Id=1000,1001,1002,1005}} | ? {{ $_.Message -match 'ssbu' }} | % {{ $n++; "{{0}}  Application {{1}} #{{2}}: {{3}}" -f $_.TimeCreated.ToString('HH:mm:ss'),$_.ProviderName,$_.Id,(($_.Message -replace '\s+',' ')) }}
Get-WinEvent -FilterHashtable @{{LogName='System';StartTime=$since;Level=1,2,3}} -MaxEvents 200 | % {{ $n++; "{{0}}  System {{1}} #{{2}} {{3}}: {{4}}" -f $_.TimeCreated.ToString('HH:mm:ss'),$_.ProviderName,$_.Id,$_.LevelDisplayName,(($_.Message -replace '\s+',' ')) }}
if(-not $n){{ "no Windows errors or warnings since $($since.ToString('HH:mm:ss')) (Display 4101 = GPU driver reset, WHEA-Logger = hardware errors)" }}"#,
        since.format("%Y-%m-%dT%H:%M:%S")
    )
}

fn powershell_to(script: &str, out: &Path) {
    let r = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "[Console]::OutputEncoding=[Text.Encoding]::UTF8;", script])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .output();
    let text = match r {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(e) => format!("(PowerShell failed: {})", e),
    };
    let _ = fs::write(out, text);
}

// ---------------------------------------------------------------- files
fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for e in fs::read_dir(src)? {
        let e = e?;
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &dst.join(e.file_name()))?;
        } else {
            fs::copy(&p, dst.join(e.file_name()))?;
        }
    }
    Ok(())
}

fn dir_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                dir_files(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}

/// Zips `dir` into `zip`; `prefix` = the folder name inside the zip ("" = the files at the top).
pub fn zip_dir(dir: &Path, zip: &Path, prefix: &str) -> Result<(), String> {
    let f = fs::File::create(zip).map_err(|e| e.to_string())?;
    let mut w = zip::ZipWriter::new(f);
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
    let mut files = Vec::new();
    dir_files(dir, &mut files);
    for p in files {
        let rel = p.strip_prefix(dir).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
        let name = if prefix.is_empty() { rel } else { format!("{}/{}", prefix, rel) };
        w.start_file(name, opt).map_err(|e| e.to_string())?;
        let mut src = fs::File::open(&p).map_err(|e| e.to_string())?;
        std::io::copy(&mut src, &mut w).map_err(|e| e.to_string())?;
    }
    w.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn save_snapshot(userdata: &Path, zip: &Path, max_mb: u64) -> String {
    let save = userdata.join("save");
    if !save.is_dir() {
        return "save_before: no save yet (first start)".into();
    }
    let mut files = Vec::new();
    dir_files(&save, &mut files);
    let bytes: u64 = files.iter().filter_map(|p| fs::metadata(p).ok()).map(|m| m.len()).sum();
    if bytes > max_mb * 1024 * 1024 {
        return format!("save_before: not taken, the save is {} MB (save_snapshot_max_mb = {})", bytes / 1048576, max_mb);
    }
    match zip_dir(&save, zip, "save") {
        Ok(()) => format!("save_before: {} files, {:.1} MB -> save_before.zip", files.len(), bytes as f64 / 1048576.0),
        Err(e) => format!("save_before: not taken ({})", e),
    }
}

fn trim_sessions(logs: &Path, keep: usize) {
    let mut dirs: Vec<PathBuf> = fs::read_dir(logs).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !p.join("running.txt").exists()).collect()).unwrap_or_default();
    dirs.sort();
    if dirs.len() > keep {
        for d in &dirs[..dirs.len() - keep] {
            let _ = fs::remove_dir_all(d);
        }
    }
}

// ---------------------------------------------------------------- the verdict (the markers of launch.ps1 Get-Verdict)
const MARKERS: &[(&str, &str)] = &[
    ("crash", "HOST EXCEPTION"),
    ("mod_resources", "MOD RESOURCE ERROR"),
    ("guest_trap", "GUEST TRAP"),
    ("untranslated", "UNTRANSLATED INSTRUCTION"),
    ("device_lost", "nvn_vk_lost|VK_ERROR_DEVICE_LOST"),
    ("freeze", "NO FRAME PRESENTED FOR"),
    ("freeze_recovered", "frames reach the window again"),
    ("debug_end", r"debug end: \d+ fight frames reached"),
    ("movie", "input anchor 0 at poll"),
    ("time_limit", r"still running after \d+ s with"),
    ("vulkan_fatal", r"nvn_vk: FATAL: |nvn_vk: .+ failed: VK_ERROR"),
    ("nro_refused", "in no registered NRR: refused"),
];

pub fn verdict(code: i64, log: &Path, session: &Path) -> (String, Map<String, Value>, Vec<String>) {
    let mut found = Map::new();
    let text = fs::read(log).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    for (k, pat) in MARKERS {
        let re = regex::Regex::new(pat).unwrap();
        if let Some(line) = text.lines().find(|l| re.is_match(l)) {
            found.insert(k.to_string(), json!(line.trim()));
        }
    }
    if let Ok(trace) = fs::read(session.join("boot_trace.txt")) {
        let re = regex::Regex::new(r#"fs open "rom:/[^"]*" mode \d+ -> NOT FOUND"#).unwrap();
        let hits: Vec<String> = String::from_utf8_lossy(&trace).lines().filter(|l| re.is_match(l)).take(5).map(|l| l.trim().to_string()).collect();
        if !hits.is_empty() {
            found.insert("rom_missing".into(), json!(hits.join(" | ")));
        }
    }
    let dumps: Vec<String> = fs::read_dir(session).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".dmp")).collect()).unwrap_or_default();
    let mut result = if code != 0 { "error" } else { "ok" }.to_string();
    for k in ["crash", "guest_trap", "untranslated", "device_lost", "vulkan_fatal", "nro_refused", "rom_missing"] {
        if found.contains_key(k) {
            result = "error".into();
        }
    }
    if found.contains_key("freeze") && result == "ok" {
        result = "warning".into();
    }
    if dumps.iter().any(|n| n.contains("_manual_")) && result == "ok" {
        found.insert("manual_dump".into(), json!("写过手动 minidump: 测试者看到了问题"));
        result = "warning".into();
    }
    (result, found, dumps)
}

// ---------------------------------------------------------------- start
pub struct StartArgs {
    pub mode: String,                     // play | smoke | online
    pub extra_env: Vec<(String, String)>, // the debug panel's options for this run
    pub seconds: i64,
}

fn setting(ini: &Ini, k: &str, d: &str) -> String {
    ini.get(Some("run"), k).filter(|v| !v.is_empty()).unwrap_or_else(|| d.to_string())
}

fn apply_game_name(env: &mut Map<String, Value>, name: &str) {
    // Windows environment keys are case-insensitive. Debug overrides cannot replace the chosen identity.
    env.retain(|key, _| !key.eq_ignore_ascii_case("SSBU_NICKNAME"));
    env.insert("SSBU_NICKNAME".into(), json!(name));
}

pub fn start(l: &Layout, games: &Arc<Games>, bridge: &Arc<Bridge>, a: StartArgs, on_exit: impl FnOnce(Value) + Send + 'static) -> Result<Value, String> {
    let mut slot = games.run.lock().unwrap();
    if slot.as_ref().map_or(false, |r| r.finished.is_none()) {
        return Err("游戏已经在运行".into());
    }
    let ini = Ini::load(&l.launcher_ini());
    let player_name = crate::settings::validate_game_name(&crate::settings::configured_game_name(&ini))?;
    let now = chrono::Local::now();
    let computer: String = std::env::var("COMPUTERNAME").unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    let name = format!("{}_{}_{}", now.format("%Y%m%d-%H%M%S"), computer, a.mode);
    let session = l.logs().join(&name);
    fs::create_dir_all(&session).map_err(|e| format!("会话文件夹 {}: {}", session.display(), e))?;
    let _ = fs::create_dir_all(l.reports());
    let _ = fs::create_dir_all(l.root.join("cache"));
    let log = session.join("game.log");

    let romfs = {
        let r = ini.get(Some("run"), "romfs").unwrap_or_default();
        if r.is_empty() { l.default_romfs() } else { l.full(&r) }
    };
    let mut userdata = {
        let u = ini.get(Some("run"), "userdata").unwrap_or_default();
        if u.is_empty() { l.root.join("userdata") } else { l.full(&u) }
    };
    let mut seconds = if a.seconds >= 0 { a.seconds } else { setting(&ini, "seconds", "0").parse().unwrap_or(0) };
    let mut env: Map<String, Value> = Map::new();
    let scratch = l.root.join("tmp").join(&name);
    if a.mode == "smoke" {
        userdata = scratch.join("userdata");
        let _ = fs::create_dir_all(&userdata);
        let seed = l.root.join("tests").join("smoke_save");
        if seed.is_dir() {
            let _ = copy_dir(&seed, &userdata);
        }
        env.insert("SSBU_SHOT_DIR".into(), json!(session.to_string_lossy()));
        env.insert("SSBU_SHOT".into(), json!("600"));
        env.insert("SSBU_SHOT_EVERY".into(), json!("1200"));
        if seconds <= 0 {
            seconds = setting(&ini, "smoke_seconds", "90").parse().unwrap_or(90);
        }
    }
    let fs_seed = setting(&ini, "fixed_seed", "auto");
    if fs_seed == "auto" {
        let r = (now.timestamp_nanos_opt().unwrap_or(1) as u64 ^ std::process::id() as u64).wrapping_mul(6364136223846793005) % 2147483646 + 1;
        env.insert("SSBU_FIXED_SEED".into(), json!(r.to_string()));
    } else if fs_seed != "none" && fs_seed != "0" {
        env.insert("SSBU_FIXED_SEED".into(), json!(fs_seed));
    }
    if setting(&ini, "deterministic", "1") == "1" {
        env.insert("SSBU_DETERMINISTIC".into(), json!("1"));
    }
    env.insert("SSBU_ROMFS".into(), json!(romfs.to_string_lossy()));
    env.insert("SSBU_USERDATA".into(), json!(userdata.to_string_lossy()));
    env.insert("SSBU_FONTS".into(), json!(l.root.join("sysdata").join("fonts").to_string_lossy()));
    if l.root.join("sysdata").join("zoneinfo").is_dir() {
        env.insert("SSBU_TZDIR".into(), json!(l.root.join("sysdata").join("zoneinfo").to_string_lossy()));
    }
    env.insert("SSBU_REPORT_DIR".into(), json!(session.to_string_lossy()));
    env.insert("SSBU_TRACE".into(), json!(session.join("boot_trace.txt").to_string_lossy()));
    env.insert("SSBU_REC".into(), json!(session.join("input.rec").to_string_lossy()));
    env.insert("SSBU_MINIDUMP".into(), json!(setting(&ini, "minidump", "1")));
    if setting(&ini, "fight_hash", "1") == "1" {
        env.insert("SSBU_FIGHT_HASH".into(), json!(session.join("fight_hash.txt").to_string_lossy()));
    }
    for (k, v) in ini.entries(Some("env")) {
        env.insert(k, json!(v));
    }
    for (k, v) in &a.extra_env {
        if v.is_empty() {
            env.remove(k);
        } else {
            env.insert(k.clone(), json!(v));
        }
    }
    apply_game_name(&mut env, &player_name);
    let _ = fs::create_dir_all(&userdata);

    // before the run
    let pre = preflight(l, &romfs);
    let mut pre_text = pre.lines.clone();
    pre_text.extend(pre.fatal.iter().map(|f| format!("FATAL: {}", f)));
    let _ = fs::write(session.join("preflight.txt"), pre_text.join("\r\n"));
    {
        let out = session.join("system.txt");
        std::thread::spawn(move || powershell_to(SYSTEM_PS, &out));
    }
    if l.config_dir().is_dir() {
        let _ = copy_dir(&l.config_dir(), &session.join("config_before"));
    }
    let snap = save_snapshot(&userdata, &session.join("save_before.zip"), setting(&ini, "save_snapshot_max_mb", "200").parse().unwrap_or(200));
    let inherited: Map<String, Value> = std::env::vars().filter(|(k, _)| k.starts_with("SSBU_") || k.starts_with("RECOMP_")).map(|(k, v)| (k, json!(v))).collect();
    let m = manifest(l);
    let mut info = Map::new();
    info.insert("session".into(), json!(name));
    info.insert("mode".into(), json!(if a.mode == "online" { "play" } else { &a.mode }));
    info.insert("console_mode".into(), json!(a.mode));
    info.insert("launcher".into(), json!("ssbu-console"));
    info.insert("build_id".into(), json!(build_id(l)));
    info.insert("computer".into(), json!(std::env::var("COMPUTERNAME").unwrap_or_default()));
    info.insert("user".into(), json!(std::env::var("USERNAME").unwrap_or_default()));
    info.insert("started".into(), json!(now.to_rfc3339()));
    info.insert("package".into(), json!(l.root.to_string_lossy()));
    info.insert("seconds_limit".into(), json!(seconds));
    info.insert("git_commit".into(), m.as_ref().map(|m| m["git_commit"].clone()).unwrap_or(Value::Null));
    info.insert("package_problems".into(), json!(pre.package_bad));
    info.insert("replay_of".into(), Value::Null);
    info.insert("save_snapshot".into(), json!(snap));
    info.insert("env".into(), Value::Object(env.clone()));
    info.insert("inherited_env_removed".into(), Value::Object(inherited));
    if !pre.fatal.is_empty() {
        info.insert("result".into(), json!("not_started"));
        info.insert("problems".into(), json!(pre.fatal));
        let _ = fs::write(session.join("session.json"), serde_json::to_string_pretty(&info).unwrap());
        return Err(pre.fatal.join("\n"));
    }

    // the exclusion of logical processors: this process's mask is inherited by the game
    let exclude = setting(&ini, "exclude_cpus", "");
    let mut affinity = Value::Null;
    let mut restore: Option<usize> = None;
    if !exclude.is_empty() {
        if let Some((old, new)) = crate::win::exclude_cpus(&exclude) {
            affinity = json!(format!("{:x}", new));
            restore = Some(old);
        }
    }
    info.insert("affinity_mask".into(), affinity);
    let _ = fs::write(session.join("session.json"), serde_json::to_string_pretty(&info).unwrap());

    // the pipe the game connects to (console_bridge.c): network control, and the gate that holds the game before it boots
    let pipe = bridge.listen().map_err(|e| format!("命名管道: {}", e))?;
    let logf = fs::File::create(&log).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&l.exe);
    cmd.current_dir(&l.game_dir)
        .arg("..\\binaries\\13.0.4\\main.elf")
        .arg(seconds.to_string())
        .stdin(Stdio::null())
        .stdout(logf.try_clone().map_err(|e| e.to_string())?)
        .stderr(logf)
        .creation_flags(CREATE_NO_WINDOW);
    for (k, _) in std::env::vars() {
        if k.starts_with("SSBU_") || k.starts_with("RECOMP_") {
            cmd.env_remove(k);
        }
    }
    for (k, v) in &env {
        cmd.env(k, v.as_str().unwrap_or(""));
    }
    cmd.env("SSBU_CONSOLE_PIPE", &pipe);
    let spawned = cmd.spawn();
    if let Some(old) = restore {
        crate::win::set_affinity(old);
    }
    let child = match spawned {
        Ok(c) => c,
        Err(e) => {
            bridge.abandon();
            return Err(format!("游戏没有启动: {}", e));
        }
    };
    let pid = child.id();
    let _ = fs::write(session.join("running.txt"), format!("{}\r\n", pid));
    games.log.lock().unwrap().clear();
    *slot = Some(Run { session: session.clone(), name: name.clone(), mode: a.mode.clone(), pid, started: now, t0: Instant::now(), info, child: Some(child), finished: None });
    drop(slot);

    // offline runs pass the gate at once; an online run passes it when the room has admitted it (the UI says start)
    let b2 = bridge.clone();
    let auto_start = a.mode != "online";
    std::thread::spawn(move || {
        if b2.accept(Duration::from_secs(20)) && auto_start {
            let _ = b2.call("start");
        }
    });
    let g2 = games.clone();
    let l2 = l.clone();
    let b3 = bridge.clone();
    std::thread::spawn(move || tail_log(&g2, &log));
    let g3 = games.clone();
    std::thread::spawn(move || {
        let result = wait_and_finish(&l2, &g3, &b3, &scratch);
        on_exit(result);
    });
    Ok(json!({ "session": name, "pid": pid }))
}

/// The game's output, line by line, into the ring the debug panel reads (and pushes to the UI).
fn tail_log(g: &Arc<Games>, log: &Path) {
    let mut pos = 0u64;
    let mut rest = String::new();
    loop {
        let done = g.run.lock().unwrap().as_ref().map_or(true, |r| r.finished.is_some());
        if let Ok(mut f) = fs::File::open(log) {
            if f.seek(SeekFrom::Start(pos)).is_ok() {
                let mut buf = Vec::new();
                if f.read_to_end(&mut buf).is_ok() && !buf.is_empty() {
                    pos += buf.len() as u64;
                    rest.push_str(&String::from_utf8_lossy(&buf));
                    let mut ring = g.log.lock().unwrap();
                    let mut seq = g.log_seq.lock().unwrap();
                    while let Some(i) = rest.find('\n') {
                        let line: String = rest[..i].trim_end_matches('\r').chars().take(2000).collect();
                        rest.drain(..=i);
                        ring.push_back(line);
                        *seq += 1;
                        if ring.len() > 5000 {
                            ring.pop_front();
                        }
                    }
                }
            }
        }
        if done {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn wait_and_finish(l: &Layout, g: &Arc<Games>, bridge: &Arc<Bridge>, scratch: &Path) -> Value {
    let mut child = g.run.lock().unwrap().as_mut().and_then(|r| r.child.take()).expect("child");
    let status = child.wait();
    bridge.close();
    let code: i64 = status.ok().and_then(|s| s.code()).map(|c| c as i64).unwrap_or(-1);
    let (session, name, mode, started, t0, mut info) = {
        let r = g.run.lock().unwrap();
        let r = r.as_ref().unwrap();
        (r.session.clone(), r.name.clone(), r.mode.clone(), r.started, r.t0, r.info.clone())
    };
    let _ = fs::remove_file(session.join("running.txt"));
    std::thread::sleep(Duration::from_millis(300)); // the last lines of the log
    let (mut result, markers, dumps) = verdict(code, &session.join("game.log"), &session);
    if mode == "smoke" {
        let clean = result == "ok";
        result = if clean && markers.contains_key("time_limit") && markers.contains_key("movie") { "pass" } else { "fail" }.into();
    }
    powershell_to(&events_ps(&(started - chrono::Duration::seconds(5))), &session.join("events.txt"));
    if mode != "smoke" && l.config_dir().is_dir() {
        let _ = copy_dir(&l.config_dir(), &session.join("config_after"));
    }
    let fr = l.game_dir.join("fighter_report.txt");
    if fs::metadata(&fr).and_then(|m| m.modified()).map_or(false, |t| t >= std::time::SystemTime::now() - t0.elapsed()) {
        let _ = fs::copy(&fr, session.join("fighter_report.txt"));
    }
    let ended = chrono::Local::now();
    info.insert("ended".into(), json!(ended.to_rfc3339()));
    info.insert("duration_s".into(), json!((t0.elapsed().as_secs_f64() * 10.0).round() / 10.0));
    info.insert("exit_code".into(), json!(code));
    info.insert("exit_meaning".into(), json!(exit_meaning(code)));
    info.insert("result".into(), json!(result));
    info.insert("log_markers".into(), Value::Object(markers.clone()));
    info.insert("dumps".into(), json!(dumps));
    let _ = fs::write(session.join("session.json"), serde_json::to_string_pretty(&info).unwrap());

    let ini = Ini::load(&l.launcher_ini());
    let bad = ["error", "fail", "warning"].contains(&result.as_str());
    let zip_when = setting(&ini, "zip", "always");
    let mut zip = Value::Null;
    if zip_when == "always" || (zip_when == "error" && bad) {
        let z = l.reports().join(format!("{}.zip", name));
        match zip_dir(&session, &z, &name) {
            Ok(()) => zip = json!(z.to_string_lossy()),
            Err(e) => zip = json!(format!("(报告 zip 未能生成: {})", e)),
        }
    }
    let _ = fs::remove_dir_all(scratch);
    trim_sessions(&l.logs(), setting(&ini, "keep_sessions", "30").parse().unwrap_or(30));
    let summary = json!({
        "session": name, "mode": mode, "result": result, "exit_code": code, "exit_meaning": exit_meaning(code),
        "markers": markers, "dumps": dumps, "zip": zip, "duration_s": info["duration_s"],
    });
    if let Some(r) = g.run.lock().unwrap().as_mut() {
        r.finished = Some(summary.clone());
    }
    summary
}

pub fn status(g: &Games) -> Value {
    let r = g.run.lock().unwrap();
    match r.as_ref() {
        None => json!({ "state": "idle" }),
        Some(r) => json!({
            "state": if r.finished.is_some() { "ended" } else { "running" },
            "session": r.name, "mode": r.mode, "pid": r.pid,
            "uptime_s": r.t0.elapsed().as_secs(), "finished": r.finished,
            "session_dir": r.session.to_string_lossy(),
        }),
    }
}

pub fn log_since(g: &Games, after: u64) -> Value {
    let ring = g.log.lock().unwrap();
    let seq = *g.log_seq.lock().unwrap();
    let first = seq.saturating_sub(ring.len() as u64);
    let from = after.max(first);
    let lines: Vec<&String> = ring.iter().skip((from - first) as usize).collect();
    json!({ "seq": seq, "lines": lines })
}

pub fn kill(g: &Games) -> Result<(), String> {
    let pid = g.run.lock().unwrap().as_ref().filter(|r| r.finished.is_none()).map(|r| r.pid).ok_or("游戏没有在运行")?;
    crate::win::terminate(pid)
}

/// A minidump of the running game, written by the game's own helper (`--write-dump`, hostrt.c): the console does not
/// need debug rights of its own, and the file lands in the session folder where the report picks it up.
pub fn dump(l: &Layout, g: &Games, full: bool) -> Result<String, String> {
    let (pid, session) = g.run.lock().unwrap().as_ref().filter(|r| r.finished.is_none()).map(|r| (r.pid, r.session.clone())).ok_or("游戏没有在运行")?;
    let file = session.join(format!("ssbu_native_manual_{}.dmp", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    let out = Command::new(&l.exe)
        .current_dir(&l.game_dir)
        .args(["--write-dump", &pid.to_string(), "0", "0", if full { "1" } else { "0" }])
        .arg(&file)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() || !file.is_file() {
        return Err(format!("minidump 没有写成 (helper exit {:?})", out.status.code()));
    }
    let mb = fs::metadata(&file).map(|m| m.len() / 1048576).unwrap_or(0);
    let mut note = fs::OpenOptions::new().create(true).append(true).open(session.join("note.txt")).map_err(|e| e.to_string())?;
    let _ = writeln!(note, "{}: manual minidump (console) of process {}: {}\r", chrono::Local::now().format("%H:%M:%S"), pid, file.file_name().unwrap().to_string_lossy());
    Ok(format!("{} ({} MB)", file.display(), mb))
}

pub fn add_note(l: &Layout, session: &str, text: &str) -> Result<(), String> {
    if session.contains(['\\', '/', '.']) {
        return Err("bad session".into());
    }
    let dir = l.logs().join(session);
    let mut note = fs::OpenOptions::new().create(true).append(true).open(dir.join("note.txt")).map_err(|e| e.to_string())?;
    writeln!(note, "{}: {}\r", chrono::Local::now().format("%H:%M:%S"), text).map_err(|e| e.to_string())?;
    // the zip was made before the note: make it again so the note travels with the report
    let z = l.reports().join(format!("{}.zip", session));
    if z.is_file() {
        zip_dir(&dir, &z, session)?;
    }
    Ok(())
}

pub fn report(l: &Layout, session: &str) -> Result<String, String> {
    if session.contains(['\\', '/', '.']) {
        return Err("bad session".into());
    }
    let dir = l.logs().join(session);
    let z = l.reports().join(format!("{}.zip", session));
    let _ = fs::create_dir_all(l.reports());
    zip_dir(&dir, &z, session)?;
    Ok(z.to_string_lossy().into_owned())
}

pub fn sessions(l: &Layout) -> Value {
    let mut dirs: Vec<PathBuf> = fs::read_dir(l.logs()).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
    dirs.sort();
    dirs.reverse();
    let list: Vec<Value> = dirs
        .iter()
        .take(12)
        .map(|d| {
            let name = d.file_name().unwrap().to_string_lossy().into_owned();
            let info: Value = fs::read_to_string(d.join("session.json")).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or(Value::Null);
            let zip = l.reports().join(format!("{}.zip", name));
            json!({
                "name": name, "result": info["result"], "mode": info["console_mode"].as_str().or(info["mode"].as_str()),
                "started": info["started"], "duration_s": info["duration_s"], "exit_meaning": info["exit_meaning"],
                "running": d.join("running.txt").exists(), "zip": if zip.is_file() { json!(zip.to_string_lossy()) } else { Value::Null },
                "dir": d.to_string_lossy(),
            })
        })
        .collect();
    json!(list)
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn missing_name_blocks_every_mode_before_any_launch_side_effect() {
        let root = std::env::temp_dir().join(format!("ssbu-name-gate-{}", std::process::id()));
        let l = Layout { game_dir:root.join("game"),exe:root.join("game/does-not-exist.exe"),root:root.clone(),package:true };
        let games = Arc::new(Games::new());
        let bridge = Arc::new(Bridge::new());
        for mode in ["play", "online", "smoke"] {
            let result = start(&l,&games,&bridge,StartArgs { mode:mode.into(),extra_env:vec![("SSBU_NICKNAME".into(),"must-not-bypass".into())],seconds:0 }, |_| panic!("must not start"));
            assert!(result.err().unwrap().contains("开始页"));
        }
        assert!(!root.exists(), "blank names must not create sessions, saves, or a process");
    }
    #[test]
    fn chosen_name_overrides_case_insensitive_environment_variants() {
        let mut env = json!({"ssbu_nickname":"WindowsUser", "SSBU_NICKNAME":"Other", "SSBU_TEST":"keep"}).as_object().unwrap().clone();
        apply_game_name(&mut env,"udon");
        assert_eq!(env["SSBU_NICKNAME"],"udon");
        assert!(!env.contains_key("ssbu_nickname"));
        assert_eq!(env["SSBU_TEST"],"keep");
    }
}

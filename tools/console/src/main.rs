// SSBU Console: the package's launcher with a window. Starts the game (a session folder and a report per run, like the
// PowerShell launcher before it), edits graphics.ini / controllers.ini, drives the network extension of the running game
// over a pipe (runtime/console_bridge.c) and shows the game's log and run options for debugging.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod cover;
mod game;
mod ini;
mod mods;
mod pads;
mod paths;
mod settings;
mod win;
mod updates;

use bridge::{hex, Bridge};
use game::Games;
use pads::Pads;
use paths::Layout;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{Emitter, Manager, State};

struct App {
    layout: Option<Layout>,
    games: Arc<Games>,
    bridge: Arc<Bridge>,
    pads: Arc<Pads>,
    updates: Arc<updates::Updates>,
    _console_lock: Option<ssbu_updater::RootLock>,
}

type R = Result<Value, String>;

fn lay(a: &App) -> Result<&Layout, String> {
    a.layout.as_ref().ok_or_else(|| "找不到游戏: 控制台应放在包的根文件夹 (game\\ssbu_native.exe 旁边的上一级)".to_string())
}

#[tauri::command(async)]
fn overview(a: State<App>) -> R {
    let Some(l) = a.layout.as_ref() else {
        return Ok(json!({ "found": false }));
    };
    let ini = ini::Ini::load(&l.launcher_ini());
    let romfs = match ini.get(Some("run"), "romfs").filter(|s| !s.is_empty()) {
        Some(r) => l.full(&r),
        None => l.default_romfs(),
    };
    let pre = game::preflight(l, &romfs);
    Ok(json!({
        "found": true, "root": l.root.to_string_lossy(), "exe": l.exe.to_string_lossy(), "package": l.package,
        "build_id": game::build_id(l), "romfs": romfs.to_string_lossy(),
        "preflight": { "lines": pre.lines, "fatal": pre.fatal },
        "game": game::status(&a.games), "sessions": game::sessions(l),
    }))
}

#[tauri::command(async)]
fn graphics_get(a: State<App>) -> R {
    Ok(settings::graphics_get(lay(&a)?))
}
#[tauri::command(async)]
fn graphics_set(a: State<App>, values: serde_json::Map<String, Value>) -> R {
    settings::graphics_set(lay(&a)?, &values)
}
#[tauri::command(async)]
fn controllers_get(a: State<App>) -> R {
    a.updates.guard()?;
    let l = lay(&a)?;
    if !l.controllers_ini().is_file() && a.pads.running().is_none() && game::status(&a.games)["state"] != "running" {
        // the game writes its own commented file the first time SDL starts: one second of --pad-test does that
        let _ = std::process::Command::new(&l.exe)
            .current_dir(&l.game_dir)
            .args(["--pad-test", "--seconds", "1"])
            .env("SSBU_CONFIG", l.controllers_ini())
            .env_remove("SSBU_PAD_TEST")
            .creation_flags_no_window()
            .output();
    }
    Ok(settings::controllers_get(l))
}
#[tauri::command(async)]
fn controllers_set(a: State<App>, value: Value) -> R {
    settings::controllers_set(lay(&a)?, &value)
}
#[tauri::command(async)]
fn launcher_get(a: State<App>) -> R {
    Ok(settings::launcher_get(lay(&a)?))
}
#[tauri::command(async)]
fn launcher_set(a: State<App>, value: Value) -> R {
    settings::launcher_set(lay(&a)?, &value)
}

#[tauri::command(async)]
fn game_start(app: tauri::AppHandle, a: State<App>, mode: String, env: Vec<(String, String)>, seconds: Option<i64>) -> R {
    a.updates.guard()?;
    let _mods_lock = mods::LOCK.lock().map_err(|_| "模组管理正忙")?;
    let l = lay(&a)?;
    if !["play", "smoke", "online"].contains(&mode.as_str()) {
        return Err("unknown mode".into());
    }
    settings::validate_game_name(&settings::configured_game_name(&ini::Ini::load(&l.launcher_ini())))?;
    a.pads.stop(); // the pad tools hold SDL; the game would see the same pads twice
    let env: Vec<(String, String)> = env.into_iter().filter(|(k, _)| k.starts_with("SSBU_") && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).collect();
    let h = app.clone();
    game::start(l, &a.games, &a.bridge, game::StartArgs { mode, extra_env: env, seconds: seconds.unwrap_or(-1) }, move |summary| {
        let _ = h.emit("game-ended", summary);
    })
}
#[tauri::command(async)]
fn game_status(a: State<App>) -> R {
    let mut s = game::status(&a.games);
    s["bridge"] = json!(a.bridge.connected());
    Ok(s)
}
#[tauri::command(async)]
fn game_log(a: State<App>, after: u64) -> R {
    Ok(game::log_since(&a.games, after))
}
#[tauri::command(async)]
fn game_kill(a: State<App>) -> R {
    game::kill(&a.games).map(|_| json!(true))
}
#[tauri::command(async)]
fn game_dump(a: State<App>, full: bool) -> R {
    let l = lay(&a)?;
    let games = a.games.clone();
    let l = l.clone();
    // a full dump is several GB: off the UI thread (tauri commands run on a pool; this one may take minutes)
    std::thread::spawn(move || game::dump(&l, &games, full)).join().map_err(|_| "dump thread".to_string())?.map(|s| json!(s))
}
#[tauri::command(async)]
fn session_note(a: State<App>, session: String, text: String) -> R {
    game::add_note(lay(&a)?, &session, &text).map(|_| json!(true))
}
#[tauri::command(async)]
fn session_report(a: State<App>, session: String) -> R {
    game::report(lay(&a)?, &session).map(|s| json!(s))
}
#[tauri::command(async)]
fn cover_get(a: State<App>) -> R {
    cover::get(lay(&a)?)
}
#[tauri::command(async)]
fn cover_set(a: State<App>, data: Option<String>) -> R {
    cover::set(lay(&a)?, data.as_deref())
}
#[tauri::command(async)]
fn sessions(a: State<App>) -> R {
    Ok(game::sessions(lay(&a)?))
}
#[tauri::command(async)]
fn reveal(a: State<App>, path: String) -> R {
    let l = lay(&a)?;
    let p = std::path::PathBuf::from(&path);
    if !l.contains(&p) {
        return Err("只打开包里的文件夹".into());
    }
    if p.is_dir() {
        win::open_folder(&p)?;
    } else {
        win::reveal(&p);
    }
    Ok(json!(true))
}

// ---------------------------------------------------------------- resource packages
#[tauri::command(async)]
fn mods_list(a: State<App>) -> R {
    let _guard = mods::LOCK.lock().map_err(|_| "模组管理正忙")?;
    mods::list(lay(&a)?)
}
#[tauri::command(async)]
fn mods_change(a: State<App>, id: String, patch: Value) -> R {
    let _guard = mods::LOCK.lock().map_err(|_| "模组管理正忙")?;
    if game::status(&a.games)["state"] == "running" { return Err("请关闭游戏后再管理模组".into()); }
    mods::update(lay(&a)?, &id, patch)
}
#[tauri::command(async)]
fn mods_import(a: State<App>, path: String) -> R {
    let _guard = mods::LOCK.lock().map_err(|_| "模组管理正忙")?;
    if game::status(&a.games)["state"] == "running" { return Err("请关闭游戏后再导入模组".into()); }
    mods::import(lay(&a)?, std::path::Path::new(&path))
}
#[tauri::command(async)]
fn mods_delete(a: State<App>, id: String) -> R {
    let _guard = mods::LOCK.lock().map_err(|_| "模组管理正忙")?;
    if game::status(&a.games)["state"] == "running" { return Err("请关闭游戏后再删除模组".into()); }
    mods::delete(lay(&a)?, &id)
}
#[tauri::command(async)]
fn mods_open(a: State<App>, id: Option<String>) -> R {
    let _guard = mods::LOCK.lock().map_err(|_| "模组管理正忙")?;
    mods::open(lay(&a)?, id.as_deref())
}
#[tauri::command(async)]
fn mods_pick(kind: String) -> R { mods::pick(&kind) }

// ---------------------------------------------------------------- network (the running game's backend)
#[tauri::command(async)]
fn net_poll(a: State<App>) -> R {
    a.bridge.call("poll")
}
#[tauri::command(async)]
fn net_join(a: State<App>, host: String, port: u16, nickname: String, password: String, token: String) -> R {
    let line = format!("join\t{}\t{}\t{}\t{}\t{}", hex(host.trim()), port, hex(nickname.trim()), hex(&password), hex(&token));
    a.bridge.call(&line)
}
#[tauri::command(async)]
fn net_leave(a: State<App>) -> R {
    // leave joins the backend's worker (DNS may take a while): off the command pool's thread
    let b = a.bridge.clone();
    std::thread::spawn(move || b.call("leave")).join().map_err(|_| "leave thread".to_string())?
}
#[tauri::command(async)]
fn net_level(a: State<App>, level: u32) -> R {
    a.bridge.call(&format!("level\t{}", level.min(3)))
}
#[tauri::command(async)]
fn net_start(a: State<App>) -> R {
    a.bridge.call("start")
}

#[tauri::command(async)]
fn pad_tool(app: tauri::AppHandle, a: State<App>, kind: String, player: Option<u32>) -> R {
    a.updates.guard()?;
    let l = lay(&a)?;
    if game::status(&a.games)["state"] == "running" {
        return Err("游戏运行时手柄由游戏占用; 关闭游戏后再检测".into());
    }
    let h = app.clone();
    pads::run(l, &a.pads, &kind, player.unwrap_or(1), move |ev, payload| {
        let _ = h.emit(ev, payload);
    })
    .map(|_| json!(true))
}
#[tauri::command(async)]
fn pad_tool_stop(a: State<App>) -> R {
    a.pads.stop();
    Ok(json!(true))
}

trait NoWindow {
    fn creation_flags_no_window(&mut self) -> &mut Self;
}
impl NoWindow for std::process::Command {
    fn creation_flags_no_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(game::CREATE_NO_WINDOW)
    }
}

#[tauri::command(async)]
fn update_status(a: State<App>) -> R { a.updates.status(lay(&a)?) }
#[tauri::command(async)]
fn update_config(a: State<App>, config: updates::Config) -> R { updates::Updates::save_config(lay(&a)?, config) }
#[tauri::command(async)]
fn update_check(a: State<App>, automatic: bool) -> R { a.updates.check(lay(&a)?.clone(), automatic)?; Ok(json!(true)) }
#[tauri::command(async)]
fn update_import(a: State<App>, path: String) -> R { a.updates.import(lay(&a)?.clone(), path.into())?; Ok(json!(true)) }
#[tauri::command(async)]
fn update_download(a: State<App>, source: Option<String>) -> R { a.updates.download(lay(&a)?.clone(), source)?; Ok(json!(true)) }
#[tauri::command(async)]
fn update_cancel(a: State<App>) -> R { a.updates.cancel(); Ok(json!(true)) }
#[tauri::command(async)]
fn update_browser(a: State<App>, index: usize) -> R { a.updates.browser(index, lay(&a)?)?; Ok(json!(true)) }
#[tauri::command(async)]
fn update_pick() -> R { updates::pick() }
#[tauri::command(async)]
fn update_apply(app: tauri::AppHandle, a: State<App>, rollback: bool) -> R {
    a.updates.apply(lay(&a)?, &a.pads, &a.games, rollback)?;
    app.exit(0);
    Ok(json!(true))
}
fn main() {
    let layout = Layout::find();
    if let Some(l) = &layout {
        if l.package && ssbu_updater::pending(&l.root) {
            if updates::spawn_helper(&l.root, "recover", None, std::process::id(), true).is_ok() { return; }
        }
    }
    let console_lock = match &layout { Some(l) => match ssbu_updater::RootLock::acquire(&l.root) { Ok(lock) => Some(lock), Err(_) => return }, None => None };
    let updates = Arc::new(updates::Updates::new(layout.as_ref()));
    let app = App { layout, games: Arc::new(Games::new()), bridge: Arc::new(Bridge::new()), pads: Arc::new(Pads::new()), updates, _console_lock: console_lock };
    tauri::Builder::default()
        .manage(app)
        .invoke_handler(tauri::generate_handler![
            overview, graphics_get, graphics_set, controllers_get, controllers_set, launcher_get, launcher_set, game_start,
            game_status, game_log, game_kill, game_dump, session_note, session_report, sessions, reveal, net_poll, net_join,
            net_leave, net_level, net_start, pad_tool, pad_tool_stop, cover_get, cover_set, mods_list, mods_change, mods_import, mods_delete, mods_open, mods_pick,
            update_status, update_config, update_check, update_import, update_download, update_cancel, update_browser, update_pick, update_apply
        ])
        .on_window_event(|w, e| {
            if let tauri::WindowEvent::Destroyed = e {
                w.app_handle().state::<App>().pads.stop();
            }
        })
        .run(tauri::generate_context!())
        .expect("SSBU Console");
}

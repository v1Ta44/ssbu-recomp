//! graphics.ini (the keys the console controls) and controllers.ini (all of it), read and written for the UI as JSON.
//! Both files belong to the game: when they do not exist yet the console writes them the way the game would, by running
//! the exe once for a pad tool (nnpad_sdl.c writes controllers.ini) or, for graphics.ini, with only the console's keys
//! (gfx_options.c fills in its defaults for every key that is missing).
use crate::ini::Ini;
use crate::paths::Layout;
use serde_json::{json, Map, Value};

pub const GRAPHICS_KEYS: &[(&str, &str)] = &[
    ("renderer", "vulkan"),
    ("window", "windowed"),
    ("vsync", "auto"),
    ("latency", "normal"),
    ("resolution_scale", "1.0"),
    ("profile", "custom"),
];

pub fn graphics_get(l: &Layout) -> Value {
    let ini = Ini::load(&l.graphics_ini());
    let mut m = Map::new();
    for (k, def) in GRAPHICS_KEYS {
        m.insert(k.to_string(), json!(ini.get(None, k).unwrap_or_else(|| def.to_string()).to_ascii_lowercase()));
    }
    json!({ "values": m, "exists": ini.exists_text(), "path": l.graphics_ini().to_string_lossy() })
}

fn allowed(key: &str, v: &str) -> bool {
    match key {
        "renderer" => ["vulkan", "opengl"].contains(&v),
        "window" => ["windowed", "borderless", "fullscreen"].contains(&v),
        "vsync" => ["auto", "display", "clock", "off", "on", "mailbox"].contains(&v),
        "latency" => ["normal", "low", "lowest"].contains(&v),
        "profile" => ["custom", "low"].contains(&v),
        "resolution_scale" => v.parse::<f32>().map_or(false, |f| (0.25..=2.0).contains(&f)),
        _ => false,
    }
}

pub fn graphics_set(l: &Layout, values: &Map<String, Value>) -> Result<Value, String> {
    let mut ini = Ini::load(&l.graphics_ini());
    if !ini.exists_text() {
        ini.push_comment("; graphics.ini: written by the console with its keys only; the game adds the defaults of the others.");
        ini.push_comment("; Delete the file to get the game's full file with every key explained.");
    }
    for (k, v) in values {
        let v = v.as_str().ok_or_else(|| format!("{}: not a string", k))?.trim().to_ascii_lowercase();
        if !allowed(k, &v) {
            return Err(format!("graphics.ini: {} = {} is not a value the game reads", k, v));
        }
        ini.set(None, k, &v);
    }
    ini.save(&l.graphics_ini()).map_err(|e| format!("graphics.ini not written: {}", e))?;
    Ok(graphics_get(l))
}

// ---------------------------------------------------------------- controllers.ini
pub const PRO: &[&str] = &["A", "B", "X", "Y", "L", "R", "ZL", "ZR", "PLUS", "MINUS", "LEFT", "UP", "RIGHT", "DOWN", "STICK_L", "STICK_R"];
pub const KEYS: &[&str] = &[
    "A", "B", "X", "Y", "L", "R", "ZL", "ZR", "PLUS", "MINUS", "LEFT", "UP", "RIGHT", "DOWN", "STICK_L", "STICK_R",
    "LSTICK_LEFT", "LSTICK_UP", "LSTICK_RIGHT", "LSTICK_DOWN", "RSTICK_LEFT", "RSTICK_UP", "RSTICK_RIGHT", "RSTICK_DOWN",
];
const KB_DEFAULT: &[&str] = &[
    "Z", "X", "C", "V", "Q", "E", "A", "S", "ENTER", "BACKSPACE", "J", "I", "L", "K", "none", "none", "LEFT", "UP", "RIGHT",
    "DOWN", "none", "none", "none", "none",
];
const STICKS: &[&str] = &["LX", "LY", "RX", "RY"];
const DEV_SCALARS: &[&str] = &["layout", "deadzone", "outer_zone", "trigger_threshold", "rumble", "rumble_strength"];

pub fn controllers_get(l: &Layout) -> Value {
    let ini = Ini::load(&l.controllers_ini());
    let g = |k: &str, d: &str| ini.get(Some("general"), k).unwrap_or_else(|| d.to_string());
    let general = json!({
        "layout": g("layout", "auto").to_ascii_lowercase(),
        "deadzone": g("deadzone", "0.10"),
        "outer_zone": g("outer_zone", "0.98"),
        "trigger_threshold": g("trigger_threshold", "0.33"),
        "rumble": g("rumble", "on").to_ascii_lowercase(),
        "rumble_strength": g("rumble_strength", "1.0"),
        "ignore": ini.entries(Some("general")).into_iter().filter(|(k, _)| k.eq_ignore_ascii_case("ignore")).map(|(_, v)| v).collect::<Vec<_>>(),
    });
    let players: Vec<String> = (1..=8).map(|i| ini.get(Some("players"), &format!("player{}", i)).unwrap_or_else(|| "auto".into())).collect();
    let mut keys = Map::new();
    for (i, k) in KEYS.iter().enumerate() {
        keys.insert(k.to_string(), json!(ini.get(Some("keyboard"), k).unwrap_or_else(|| KB_DEFAULT[i].to_string())));
    }
    let keyboard = json!({ "player": ini.get(Some("keyboard"), "player").unwrap_or_else(|| "auto".into()), "keys": keys });
    let mut devices = Vec::new();
    for sec in ini.sections() {
        let Some(guid) = sec.strip_prefix("device ") else { continue };
        let guid = guid.trim().to_string();
        let mut d = Map::new();
        d.insert("guid".into(), json!(guid));
        let mut map = Map::new();
        let mut axis = Map::new();
        let mut invert = Map::new();
        for (k, v) in ini.entries(Some(&sec)) {
            let kl = k.to_ascii_lowercase();
            if kl == "name" || DEV_SCALARS.contains(&kl.as_str()) {
                d.insert(kl, json!(v));
            } else if let Some(b) = kl.strip_prefix("map.") {
                map.insert(b.to_ascii_uppercase(), json!(v));
            } else if let Some(a) = kl.strip_prefix("axis.") {
                axis.insert(a.to_ascii_uppercase(), json!(v));
            } else if let Some(a) = kl.strip_prefix("invert.") {
                invert.insert(a.to_ascii_uppercase(), json!(v));
            }
        }
        d.insert("map".into(), Value::Object(map));
        d.insert("axis".into(), Value::Object(axis));
        d.insert("invert".into(), Value::Object(invert));
        devices.push(Value::Object(d));
    }
    json!({ "general": general, "players": players, "keyboard": keyboard, "devices": devices,
            "exists": ini.exists_text(), "path": l.controllers_ini().to_string_lossy() })
}

fn clean(v: &Value) -> Result<String, String> {
    let s = match v {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => (if *b { "on" } else { "off" }).to_string(),
        _ => return Err("not a value".into()),
    };
    if s.contains(['\r', '\n', '[', ']']) || s.len() > 200 {
        return Err(format!("not a value for the file: {:?}", s));
    }
    Ok(s)
}
fn guid_ok(g: &str) -> bool {
    g.len() == 32 && g.chars().all(|c| c.is_ascii_hexdigit())
}
fn number_in(s: &str, lo: f32, hi: f32) -> bool {
    s.parse::<f32>().map_or(false, |f| (lo..=hi).contains(&f))
}

pub fn controllers_set(l: &Layout, c: &Value) -> Result<Value, String> {
    let mut ini = Ini::load(&l.controllers_ini());
    let gen = &c["general"];
    for (k, lo, hi) in [("deadzone", 0.0, 0.9), ("outer_zone", 0.1, 1.0), ("trigger_threshold", 0.01, 1.0), ("rumble_strength", 0.0, 2.0)] {
        let v = clean(&gen[k])?;
        if !number_in(&v, lo, hi) {
            return Err(format!("{} = {}: {} .. {}", k, v, lo, hi));
        }
        ini.set(Some("general"), k, &v);
    }
    let layout = clean(&gen["layout"])?;
    if !["auto", "positional", "labels"].contains(&layout.as_str()) {
        return Err(format!("layout = {}", layout));
    }
    ini.set(Some("general"), "layout", &layout);
    let rumble = clean(&gen["rumble"])?;
    ini.set(Some("general"), "rumble", if rumble == "off" { "off" } else { "on" });
    // ignore = lines: rewritten as a group (several lines with the same key)
    let ignores: Vec<String> = gen["ignore"].as_array().map(|a| a.iter().filter_map(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
    for g in &ignores {
        if !guid_ok(g) {
            return Err(format!("ignore = {}: a GUID is 32 hex digits", g));
        }
    }
    ini.remove_key(Some("general"), "ignore");
    for g in &ignores {
        ini.add(Some("general"), "ignore", g);
    }
    for (i, p) in c["players"].as_array().ok_or("players")?.iter().enumerate().take(8) {
        let p = clean(p)?;
        if !(p == "auto" || p == "none" || guid_ok(&p)) {
            return Err(format!("player{} = {}: auto, none or a device GUID", i + 1, p));
        }
        ini.set(Some("players"), &format!("player{}", i + 1), &p);
    }
    let kb = &c["keyboard"];
    let kp = clean(&kb["player"])?;
    if !(kp == "auto" || kp == "none" || kp.parse::<u8>().map_or(false, |n| (1..=8).contains(&n))) {
        return Err(format!("keyboard player = {}", kp));
    }
    ini.set(Some("keyboard"), "player", &kp);
    for k in KEYS {
        if let Some(v) = kb["keys"].get(*k) {
            let v = clean(v)?;
            if !key_ok(&v) {
                return Err(format!("keyboard {} = {}: not a key the game reads", k, v));
            }
            ini.set(Some("keyboard"), k, &v);
        }
    }
    // devices: each [device GUID] section is rewritten from the UI's object (keys left empty fall back to [general])
    let devs = c["devices"].as_array().cloned().unwrap_or_default();
    let keep: Vec<String> = devs.iter().filter_map(|d| d["guid"].as_str()).map(|s| s.to_ascii_lowercase()).collect();
    for sec in ini.sections() {
        if let Some(g) = sec.strip_prefix("device ") {
            if !keep.contains(&g.trim().to_ascii_lowercase()) {
                ini.remove_section(&sec);
            }
        }
    }
    for d in devs {
        let guid = d["guid"].as_str().unwrap_or("").trim().to_string();
        if !guid_ok(&guid) {
            return Err(format!("[device {}]: a GUID is 32 hex digits", guid));
        }
        let sec = format!("device {}", guid);
        ini.clear_section(&sec);
        let mut put = |k: &str, v: &str| ini.set(Some(&sec), k, v);
        if let Some(n) = d["name"].as_str().filter(|s| !s.trim().is_empty()) {
            put("name", &clean(&json!(n))?);
        } else {
            put("name", "pad");
        }
        for k in DEV_SCALARS {
            if let Some(v) = d.get(*k).filter(|v| !v.is_null() && v.as_str() != Some("")) {
                put(k, &clean(v)?);
            }
        }
        for b in PRO {
            if let Some(v) = d["map"].get(*b).filter(|v| v.as_str().map_or(false, |s| !s.trim().is_empty())) {
                let v = clean(v)?;
                if !v.split(',').all(|s| source_ok(s.trim())) {
                    return Err(format!("map.{} = {}: unknown source", b, v));
                }
                put(&format!("map.{}", b), &v);
            }
        }
        for a in STICKS {
            if let Some(v) = d["axis"].get(*a).filter(|v| v.as_str().map_or(false, |s| !s.trim().is_empty())) {
                let v = clean(v)?;
                if !source_ok(&v) {
                    return Err(format!("axis.{} = {}: unknown source", a, v));
                }
                put(&format!("axis.{}", a), &v);
            }
            if d["invert"].get(*a).and_then(|v| v.as_str()) == Some("on") {
                put(&format!("invert.{}", a), "on");
            }
        }
    }
    ini.save(&l.controllers_ini()).map_err(|e| format!("controllers.ini not written: {}", e))?;
    Ok(controllers_get(l))
}

/// The key names nnpad_sdl.c parse_vk understands.
pub fn key_ok(v: &str) -> bool {
    let u = v.to_ascii_uppercase();
    if u == "NONE" || (u.len() == 1 && u.chars().all(|c| c.is_ascii_alphanumeric())) {
        return true;
    }
    if ["LEFT", "UP", "RIGHT", "DOWN", "ENTER", "BACKSPACE", "SPACE", "TAB", "ESC", "SHIFT", "CTRL", "ALT"].contains(&u.as_str()) {
        return true;
    }
    if let Some(n) = u.strip_prefix('F') {
        return n.parse::<u8>().map_or(false, |n| (1..=12).contains(&n));
    }
    if let Some(n) = u.strip_prefix("NUM") {
        return n.len() == 1 && n.chars().all(|c| c.is_ascii_digit());
    }
    if let Some(h) = u.strip_prefix("0X") {
        return !h.is_empty() && h.len() <= 2 && h.chars().all(|c| c.is_ascii_hexdigit());
    }
    false
}

/// The source names nnpad_sdl.c src_parse understands.
pub fn source_ok(s: &str) -> bool {
    const GP: &[&str] = &[
        "south", "east", "west", "north", "back", "guide", "start", "leftstick", "rightstick", "leftshoulder", "rightshoulder",
        "dpup", "dpdown", "dpleft", "dpright", "misc1", "rightpaddle1", "leftpaddle1", "rightpaddle2", "leftpaddle2", "touchpad",
        "misc2", "misc3", "misc4", "misc5", "misc6",
    ];
    let s = s.to_ascii_lowercase();
    if s == "none" || GP.contains(&s.as_str()) {
        return true;
    }
    for ax in ["leftx", "lefty", "rightx", "righty", "lefttrigger", "righttrigger"] {
        if let Some(rest) = s.strip_prefix(ax) {
            if rest.is_empty() || rest == "+" || rest == "-" {
                return true;
            }
        }
    }
    let digits = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit());
    if let Some(n) = s.strip_prefix("button") {
        return digits(n);
    }
    if let Some(n) = s.strip_prefix("axis") {
        let n = n.trim_end_matches(['+', '-']);
        return digits(n) && s.len() - "axis".len() - n.len() <= 1;
    }
    if let Some(n) = s.strip_prefix("hat") {
        for d in ["up", "right", "down", "left"] {
            if let Some(num) = n.strip_suffix(d) {
                return digits(num);
            }
        }
    }
    false
}

/// A single game identity for every launch mode. An explicitly cleared new key never revives the legacy value.
pub fn configured_game_name(ini: &Ini) -> String {
    ini.get(Some("run"), "game_name").or_else(|| ini.get(Some("net"), "game_name")).unwrap_or_default()
}
pub fn validate_game_name(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() { return Err("请先在开始页填写游戏内名字".into()); }
    if value.len() > 32 { return Err("游戏内名字最多 32 字节，请缩短名字".into()); }
    if value.chars().any(char::is_control) { return Err("游戏内名字不能包含换行或控制字符".into()); }
    Ok(value.into())
}
static LAUNCHER_WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ---------------------------------------------------------------- launcher.ini (the run options of the package)
pub fn launcher_get(l: &Layout) -> Value {
    let ini = Ini::load(&l.launcher_ini());
    let g = |k: &str, d: &str| ini.get(Some("run"), k).filter(|v| !v.is_empty()).unwrap_or_else(|| d.to_string());
    let env: Vec<Value> = ini.entries(Some("env")).into_iter().map(|(k, v)| json!({ "key": k, "value": v })).collect();
    let net = |k: &str, d: &str| ini.get(Some("net"), k).filter(|v| !v.is_empty()).unwrap_or_else(|| d.to_string());
    json!({
        "game_name": configured_game_name(&ini),
        "romfs": ini.get(Some("run"), "romfs").unwrap_or_default(),
        "userdata": ini.get(Some("run"), "userdata").unwrap_or_default(),
        "romfs_default": l.default_romfs().to_string_lossy(),
        "fixed_seed": g("fixed_seed", "auto"),
        "deterministic": g("deterministic", "1"),
        "fight_hash": g("fight_hash", "1"),
        "minidump": g("minidump", "1"),
        "exclude_cpus": ini.get(Some("run"), "exclude_cpus").unwrap_or_default(),
        "zip": g("zip", "always"),
        "keep_sessions": g("keep_sessions", "30"),
        "smoke_seconds": g("smoke_seconds", "90"),
        "save_snapshot_max_mb": g("save_snapshot_max_mb", "200"),
        "env": env,
        "net": { "host": net("host", ""), "port": net("port", "24872"), "nickname": net("nickname", "") },
        "package": l.package,
    })
}

pub fn launcher_set(l: &Layout, v: &Value) -> Result<Value, String> {
    let _guard = LAUNCHER_WRITE.lock().map_err(|_| "设置正在保存，请重试")?;
    let mut ini = Ini::load(&l.launcher_ini());
    if let Some(value) = v.get("game_name") {
        let value = value.as_str().ok_or("游戏内名字必须是文字")?.trim();
        if !value.is_empty() { validate_game_name(value)?; }
        ini.set(Some("run"), "game_name", value);
    }
    for k in ["romfs", "userdata", "fixed_seed", "deterministic", "fight_hash", "minidump", "exclude_cpus", "zip", "keep_sessions", "smoke_seconds", "save_snapshot_max_mb"] {
        if let Some(x) = v.get(k) {
            ini.set(Some("run"), k, &clean(x)?);
        }
    }
    if let Some(env) = v["env"].as_array() {
        ini.clear_section("env");
        for e in env {
            let k = e["key"].as_str().unwrap_or("").trim();
            if k.is_empty() {
                continue;
            }
            if !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(format!("[env] {}: a variable name has letters, digits and _ only", k));
            }
            ini.add(Some("env"), k, &clean(&e["value"])?);
        }
    }
    if let Some(n) = v.get("net") {
        for k in ["host", "port", "nickname"] {
            if let Some(x) = n.get(k) {
                ini.set(Some("net"), k, &clean(x)?);
            }
        }
    }
    ini.save(&l.launcher_ini()).map_err(|e| format!("launcher.ini not written: {}", e))?;
    Ok(launcher_get(l))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn layout(tag: &str) -> Layout {
        let root = std::env::temp_dir().join(format!("ssbu_console_test_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("config")).unwrap();
        Layout { game_dir: root.join("game"), exe: root.join("game").join("ssbu_native.exe"), root, package: true }
    }
    #[test]
    fn controllers_round_trip_keeps_the_games_comments() {
        let l = layout("pads");
        let original = std::fs::read_to_string(r"D:\pcsmash\build\config\controllers.ini").unwrap_or_else(|_| "[general]\r\nlayout = auto\r\n".into());
        std::fs::write(l.controllers_ini(), &original).unwrap();
        let mut c = controllers_get(&l);
        c["general"]["deadzone"] = json!("0.15");
        c["general"]["ignore"] = json!(["03000000efbe00006d04000000000000"]);
        c["players"][1] = json!("030000005e0400008e02000000007801");
        c["keyboard"]["keys"]["A"] = json!("SPACE");
        c["devices"] = json!([{ "guid": "030000005e0400008e02000000007801", "name": "Pad", "layout": "labels", "deadzone": "",
            "map": { "A": "south, button7", "ZL": "leftshoulder" }, "axis": { "LX": "leftx" }, "invert": { "LY": "on" } }]);
        let back = controllers_set(&l, &c).unwrap();
        assert_eq!(back["general"]["deadzone"], "0.15");
        assert_eq!(back["general"]["ignore"][0], "03000000efbe00006d04000000000000");
        assert_eq!(back["players"][1], "030000005e0400008e02000000007801");
        assert_eq!(back["keyboard"]["keys"]["A"], "SPACE");
        let d = &back["devices"][0];
        assert_eq!(d["map"]["A"], "south, button7");
        assert_eq!(d["invert"]["LY"], "on");
        assert_eq!(d["layout"], "labels");
        assert!(d.get("deadzone").is_none());
        let text = std::fs::read_to_string(l.controllers_ini()).unwrap();
        for line in original.lines().filter(|x| x.starts_with(';')) {
            assert!(text.contains(line.trim_end()), "comment lost: {}", line);
        }
        let mut c2 = back.clone();
        c2["devices"] = json!([]);
        controllers_set(&l, &c2).unwrap();
        assert!(!std::fs::read_to_string(l.controllers_ini()).unwrap().contains("[device 030000005e0400008e02000000007801]"));
        let before = std::fs::read_to_string(l.controllers_ini()).unwrap();
        let mut bad = c2.clone();
        bad["keyboard"]["keys"]["B"] = json!("PAGEUP");
        assert!(controllers_set(&l, &bad).is_err());
        assert_eq!(before, std::fs::read_to_string(l.controllers_ini()).unwrap());
        let _ = std::fs::remove_dir_all(&l.root);
    }
    #[test]
    fn graphics_writes_only_its_keys_and_keeps_the_rest() {
        let l = layout("gfx");
        std::fs::write(l.graphics_ini(), "; c\r\nrenderer = vulkan\r\ngpu = RTX\r\nvsync = clock\r\n").unwrap();
        let mut v = Map::new();
        v.insert("vsync".into(), json!("display"));
        v.insert("resolution_scale".into(), json!("0.75"));
        graphics_set(&l, &v).unwrap();
        let t = std::fs::read_to_string(l.graphics_ini()).unwrap();
        assert!(t.contains("gpu = RTX") && t.contains("vsync = display") && t.contains("resolution_scale = 0.75") && t.starts_with("; c"));
        v.insert("resolution_scale".into(), json!("3"));
        assert!(graphics_set(&l, &v).is_err());
        let _ = std::fs::remove_dir_all(&l.root);
    }
    #[test]
    fn names_match_the_games_parsers() {
        for ok in ["south", "leftx+", "lefttrigger", "button12", "axis3-", "hat0left", "none"] {
            assert!(source_ok(ok), "{}", ok);
        }
        for bad in ["south+", "axis", "axis3+-", "hat0", "buttonx", "pageup"] {
            assert!(!source_ok(bad), "{}", bad);
        }
        for ok in ["Z", "7", "F12", "NUM3", "0x5B", "SPACE", "none"] {
            assert!(key_ok(ok), "{}", ok);
        }
        for bad in ["F13", "NUM", "0x", "PAGEUP", "ZZ"] {
            assert!(!key_ok(bad), "{}", bad);
        }
    }
    #[test]
    fn game_identity_migrates_and_cannot_fall_back_after_clear() {
        let l = layout("game_name");
        std::fs::create_dir_all(l.launcher_ini().parent().unwrap()).unwrap();
        std::fs::write(l.launcher_ini(), "[net]\ngame_name = udon\nnickname = RoomNick\n").unwrap();
        assert_eq!(launcher_get(&l)["game_name"], "udon");
        assert!(launcher_get(&l)["net"].get("game_name").is_none());
        assert_eq!(validate_game_name(" udon ").unwrap(), "udon");
        assert_eq!(validate_game_name("玩家").unwrap(), "玩家");
        assert!(validate_game_name("").is_err());
        assert!(validate_game_name(" 　").is_err());
        assert!(validate_game_name(&"字".repeat(11)).is_err());
        assert!(validate_game_name("a\nb").is_err());
        assert!(validate_game_name(&"a".repeat(32)).is_ok());
        launcher_set(&l, &json!({"game_name":"新名字"})).unwrap();
        assert_eq!(launcher_get(&l)["game_name"], "新名字");
        assert_eq!(launcher_get(&l)["net"]["nickname"], "RoomNick");
        launcher_set(&l, &json!({"game_name":""})).unwrap();
        assert_eq!(launcher_get(&l)["game_name"], "");
        assert!(validate_game_name(&configured_game_name(&Ini::load(&l.launcher_ini()))).is_err());
        let _ = std::fs::remove_dir_all(&l.root);
    }

}

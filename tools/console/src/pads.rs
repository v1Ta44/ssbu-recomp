//! The game's pad tools (nnpad_sdl.c), run as child processes: `--pad-test` lists the devices SDL sees and shows what is
//! pressed, `--pad-map N` asks for each button in turn and writes a [device GUID] section. Their output is parsed line by
//! line and sent to the UI as events; the console has no SDL of its own, so it sees the pads exactly as the game does.
use crate::game::CREATE_NO_WINDOW;
use crate::paths::Layout;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

pub struct Pads {
    child: Mutex<Option<(String, Child)>>,
}

impl Pads {
    pub fn new() -> Pads {
        Pads { child: Mutex::new(None) }
    }
    pub fn stop(&self) {
        if let Some((_, mut c)) = self.child.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
    pub fn running(&self) -> Option<String> {
        let mut g = self.child.lock().unwrap();
        if let Some((kind, c)) = g.as_mut() {
            if c.try_wait().ok().flatten().is_none() {
                return Some(kind.clone());
            }
        }
        *g = None;
        None
    }
}

fn device_line(name_line: &str, guid_line: &str) -> Option<Value> {
    // '  player 1: "Xbox Controller"'  +  '    GUID 0300...   type Xbox One   SDL mapping: yes   buttons 11 axes 6 hats 1   wired/unknown   rumble'
    let re1 = regex::Regex::new(r#"^\s*player (\d+): "(.*)"\s*$"#).unwrap();
    let re2 = regex::Regex::new(r"^\s*GUID ([0-9a-fA-F]{32})\s+type (.+?)\s+SDL mapping: (yes|NO[^b]*?)\s+buttons (\d+) axes (\d+) hats (\d+)\s+(.*)$").unwrap();
    let a = re1.captures(name_line)?;
    let b = re2.captures(guid_line)?;
    let tail = b[7].to_string();
    Some(json!({
        "player": a[1].parse::<u32>().unwrap_or(0), "name": &a[2], "guid": b[1].to_ascii_lowercase(), "type": &b[2],
        "mapped": &b[3] == "yes", "buttons": b[4].parse::<u32>().unwrap_or(0), "axes": b[5].parse::<u32>().unwrap_or(0),
        "hats": b[6].parse::<u32>().unwrap_or(0), "wireless": tail.contains("wireless"), "gyro": tail.contains("gyro"),
        "rumble": tail.contains("rumble"),
    }))
}

/// Starts a tool; every parsed line goes to `emit(kind, payload)`.
pub fn run(l: &Layout, pads: &Arc<Pads>, kind: &str, player: u32, emit: impl Fn(&str, Value) + Send + 'static) -> Result<(), String> {
    pads.stop();
    let mut cmd = Command::new(&l.exe);
    cmd.current_dir(&l.game_dir).creation_flags(CREATE_NO_WINDOW).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    for (k, _) in std::env::vars() {
        if k.starts_with("SSBU_") || k.starts_with("RECOMP_") {
            cmd.env_remove(k);
        }
    }
    cmd.env("SSBU_CONFIG", l.controllers_ini());
    match kind {
        "test" => cmd.args(["--pad-test", "--seconds", "900"]),
        "map" => cmd.args(["--pad-map".to_string(), player.clamp(1, 8).to_string()]),
        "init" => cmd.args(["--pad-test", "--seconds", "1"]),
        _ => return Err("unknown pad tool".into()),
    };
    let mut child = cmd.spawn().map_err(|e| format!("{}: {}", l.exe.display(), e))?;
    let out = child.stdout.take().unwrap();
    *pads.child.lock().unwrap() = Some((kind.to_string(), child));
    let kind = kind.to_string();
    std::thread::spawn(move || {
        let mut devices: Vec<Value> = Vec::new();
        let mut pending: Option<String> = None;
        let mut listing = false;
        let live = regex::Regex::new(r"^\s*player (\d+)\s+sources:(.*?)\s+->\s+Pro buttons:(.*?)\s+L\((-?\d+),(-?\d+)\) R\((-?\d+),(-?\d+)\)").unwrap();
        let ask = regex::Regex::new(r"^\s*(?:press (\S+)|move the (.+?))\s*\.\.\.\s?(.*)$").unwrap();
        // pad-map prints the prompt ("  press A        ... "), flushes, and finishes the line only when a button was
        // pressed: the reader works on chunks so the UI sees the prompt while the tool waits
        let mut reader = BufReader::new(out);
        let mut buf = String::new();
        let mut asked = String::new();
        loop {
            let chunk = match reader.fill_buf() {
                Ok(b) if !b.is_empty() => String::from_utf8_lossy(b).into_owned(),
                _ => break,
            };
            let n = chunk.len();
            reader.consume(n);
            buf.push_str(&chunk);
            let mut lines: Vec<String> = Vec::new();
            while let Some(i) = buf.find('\n') {
                lines.push(buf[..i].trim_end_matches('\r').to_string());
                buf.drain(..=i);
            }
            if kind == "map" && !buf.is_empty() {
                if let Some(c) = ask.captures(buf.trim_end()) {
                    let what = c.get(1).or(c.get(2)).map(|m| m.as_str().to_string()).unwrap_or_default();
                    if what != asked {
                        asked = what.clone();
                        emit("pad-map", json!({ "ask": what, "waiting": true }));
                    }
                }
            }
            for line in lines {
                if line.starts_with("pad-test: devices SDL sees") || (line.starts_with("pad-test: ") && line.ends_with("device(s)")) {
                    devices.clear();
                    listing = true;
                    emit("pad-devices", json!({ "devices": devices, "partial": true }));
                    continue;
                }
                if let Some(c) = live.captures(&line) {
                    listing = false;
                    let words = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
                    emit("pad-live", json!({
                        "player": c[1].parse::<u32>().unwrap_or(0), "sources": words(&c[2]), "buttons": words(&c[3]),
                        "l": [c[4].parse::<i32>().unwrap_or(0), c[5].parse::<i32>().unwrap_or(0)],
                        "r": [c[6].parse::<i32>().unwrap_or(0), c[7].parse::<i32>().unwrap_or(0)],
                    }));
                    continue;
                }
                if listing {
                    if let Some(prev) = pending.take() {
                        if let Some(d) = device_line(&prev, &line) {
                            devices.push(d);
                            emit("pad-devices", json!({ "devices": devices, "partial": false }));
                            continue;
                        }
                    }
                    if line.trim_start().starts_with("player ") && line.contains(": \"") {
                        pending = Some(line.clone());
                        continue;
                    }
                    if let Some(rest) = line.trim_start().strip_prefix("(ignored) ") {
                        emit("pad-ignored", json!({ "text": rest }));
                        continue;
                    }
                }
                if kind == "map" {
                    // a finished prompt line: "  press A        ... south" / "... skipped"; or a status line of the tool
                    if let Some(c) = ask.captures(&line) {
                        let what = c.get(1).or(c.get(2)).map(|m| m.as_str()).unwrap_or("");
                        emit("pad-map", json!({ "ask": what, "answer": c[3].trim() }));
                        asked.clear();
                    } else if !line.trim().is_empty() {
                        emit("pad-map", json!({ "text": line.trim().trim_start_matches("pad-map: ") }));
                    }
                }
            }
        }
        emit("pad-tool-end", json!({ "kind": kind }));
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn device_lines_as_print_devices_writes_them() {
        let a = r#"  player 2: "Xbox Series X Controller""#;
        let b = "    GUID 030000005e040000130b000000007801   type Xbox One   SDL mapping: yes   buttons 11 axes 6 hats 1   wireless   gyro+accel   rumble";
        let d = super::device_line(a, b).unwrap();
        assert_eq!(d["player"], 2);
        assert_eq!(d["guid"], "030000005e040000130b000000007801");
        assert_eq!(d["type"], "Xbox One");
        assert_eq!(d["mapped"], true);
        assert_eq!(d["wireless"], true);
        assert_eq!(d["rumble"], true);
        let b2 = "    GUID 03000000de280000ff11000000007801   type raw joystick   SDL mapping: NO (guessed layout; use --pad-map)   buttons 12 axes 4 hats 1   wired/unknown";
        let d2 = super::device_line(r#"  player 1: "USB Gamepad""#, b2).unwrap();
        assert_eq!(d2["mapped"], false);
        assert_eq!(d2["type"], "raw joystick");
        assert_eq!(d2["buttons"], 12);
    }
}

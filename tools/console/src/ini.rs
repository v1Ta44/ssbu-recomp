//! A line-preserving INI editor: comments, order and unknown keys stay as they are; only the keys that are set change.
//! The game's own readers (gfx_options.c, nnpad_sdl.c) and the old launcher read `key = value`, `;`/`#` comments.
use std::path::Path;

pub struct Ini {
    lines: Vec<String>,
}

fn kv(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    if t.is_empty() || t.starts_with(';') || t.starts_with('#') || t.starts_with('[') {
        return None;
    }
    let eq = t.find('=')?;
    if eq < 1 {
        return None;
    }
    Some((t[..eq].trim().to_string(), t[eq + 1..].trim().to_string()))
}

fn header(line: &str) -> Option<String> {
    let t = line.trim();
    if t.starts_with('[') {
        let end = t.find(']')?;
        return Some(t[1..end].trim().to_ascii_lowercase());
    }
    None
}

impl Ini {
    pub fn load(path: &Path) -> Ini {
        let text = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        Ini::parse(&text)
    }
    pub fn parse(text: &str) -> Ini {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut lines: Vec<String> = text.split('\n').map(|l| l.trim_end_matches('\r').to_string()).collect();
        if lines.last().map_or(false, |l| l.is_empty()) {
            lines.pop();
        }
        Ini { lines }
    }
    pub fn exists_text(&self) -> bool {
        !self.lines.is_empty()
    }
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut out = self.lines.join("\r\n");
        out.push_str("\r\n");
        let tmp = path.with_extension("ini.tmp");
        std::fs::write(&tmp, out.as_bytes())?;
        std::fs::rename(&tmp, path)
    }
    /// (line index, section) of every key line; `None` = any section.
    fn key_lines<'a>(&'a self, sec: Option<&'a str>) -> impl Iterator<Item = (usize, String, String)> + 'a {
        let mut cur = String::new();
        self.lines.iter().enumerate().filter_map(move |(i, l)| {
            if let Some(h) = header(l) {
                cur = h;
                return None;
            }
            let (k, v) = kv(l)?;
            match sec {
                Some(s) if !cur.eq_ignore_ascii_case(s) => None,
                _ => Some((i, k, v)),
            }
        })
    }
    /// The last value of the key (the game's readers let a later line win).
    pub fn get(&self, sec: Option<&str>, key: &str) -> Option<String> {
        self.key_lines(sec).filter(|(_, k, _)| k.eq_ignore_ascii_case(key)).map(|(_, _, v)| v).last()
    }
    pub fn entries(&self, sec: Option<&str>) -> Vec<(String, String)> {
        self.key_lines(sec).map(|(_, k, v)| (k, v)).collect()
    }
    pub fn set(&mut self, sec: Option<&str>, key: &str, value: &str) {
        let line = format!("{} = {}", key, value);
        let hits: Vec<usize> = self.key_lines(sec).filter(|(_, k, _)| k.eq_ignore_ascii_case(key)).map(|(i, _, _)| i).collect();
        if !hits.is_empty() {
            for i in hits {
                self.lines[i] = line.clone();
            }
            return;
        }
        match sec {
            None => self.lines.push(line),
            Some(s) => match self.section_end(s) {
                Some(at) => self.lines.insert(at, line),
                None => {
                    if self.lines.last().map_or(false, |l| !l.trim().is_empty()) {
                        self.lines.push(String::new());
                    }
                    self.lines.push(format!("[{}]", s));
                    self.lines.push(line);
                }
            },
        }
    }
    /// Removes every key line of the section (comments stay).
    pub fn clear_section(&mut self, sec: &str) {
        let drop: Vec<usize> = self.key_lines(Some(sec)).map(|(i, _, _)| i).collect();
        for i in drop.into_iter().rev() {
            self.lines.remove(i);
        }
    }
    /// The index after the section's last non-blank line, or None when there is no such section.
    fn section_end(&self, sec: &str) -> Option<usize> {
        let mut start = None;
        for (i, l) in self.lines.iter().enumerate() {
            if let Some(h) = header(l) {
                if start.is_some() {
                    break;
                }
                if h.eq_ignore_ascii_case(sec) {
                    start = Some(i);
                }
            }
        }
        let s = start?;
        let mut end = s + 1;
        for (i, l) in self.lines.iter().enumerate().skip(s + 1) {
            if header(l).is_some() {
                break;
            }
            if !l.trim().is_empty() {
                end = i + 1;
            }
        }
        Some(end)
    }
    pub fn push_comment(&mut self, text: &str) {
        self.lines.push(text.to_string());
    }
    /// Section names in file order (lower case, as `header` returns them; GUIDs are case-insensitive for the game).
    pub fn sections(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for l in &self.lines {
            if let Some(h) = header(l) {
                if !out.contains(&h) {
                    out.push(h);
                }
            }
        }
        out
    }
    pub fn remove_key(&mut self, sec: Option<&str>, key: &str) {
        let drop: Vec<usize> = self.key_lines(sec).filter(|(_, k, _)| k.eq_ignore_ascii_case(key)).map(|(i, _, _)| i).collect();
        for i in drop.into_iter().rev() {
            self.lines.remove(i);
        }
    }
    /// Appends a key line even when the key exists (keys that may repeat: ignore =, [env]).
    pub fn add(&mut self, sec: Option<&str>, key: &str, value: &str) {
        let line = format!("{} = {}", key, value);
        match sec.and_then(|s| self.section_end(s)) {
            Some(at) => self.lines.insert(at, line),
            None if sec.is_none() => self.lines.push(line),
            None => {
                self.set(sec, key, value);
            }
        }
    }
    /// Removes the header and the key lines of a section; its comment lines go with it up to the next header.
    pub fn remove_section(&mut self, sec: &str) {
        let mut i = 0;
        let mut inside = false;
        while i < self.lines.len() {
            if let Some(h) = header(&self.lines[i]) {
                inside = h.eq_ignore_ascii_case(sec);
            }
            if inside {
                self.lines.remove(i);
            } else {
                i += 1;
            }
        }
        while self.lines.len() >= 2 && self.lines.last().map_or(false, |l| l.trim().is_empty()) {
            self.lines.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn set_keeps_comments_and_order() {
        let mut ini = Ini::parse("; c\r\n[run]\r\na = 1\r\n\r\n[env]\r\n; x\r\nK = v\r\n");
        ini.set(Some("run"), "a", "2");
        ini.set(Some("run"), "b", "3");
        ini.set(Some("net"), "host", "h");
        assert_eq!(ini.get(Some("run"), "a").as_deref(), Some("2"));
        assert_eq!(ini.lines, vec!["; c", "[run]", "a = 2", "b = 3", "", "[env]", "; x", "K = v", "", "[net]", "host = h"]);
        ini.clear_section("env");
        assert_eq!(ini.entries(Some("env")).len(), 0);
        assert!(ini.lines.contains(&"; x".to_string()));
    }
}

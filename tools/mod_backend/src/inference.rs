//! Directory inference follows ARCropolis fs/inference.rs at bf2005b0 (GPL-3.0).
use crate::{packages::Plan, paths};
use std::collections::BTreeSet;
fn slot(s: &str) -> bool {
    let b = s.as_bytes();
    (3..=4).contains(&b.len()) && b[0] == b'c' && b[1..].iter().all(u8::is_ascii_digit)
}
fn extra(s: &str) -> bool {
    slot(s) && s[1..].parse::<u16>().is_ok_and(|n| n >= 8)
}
pub fn apply(p: &mut Plan) {
    for path in [
        "fighter/pacman/model/firehydrant/c00/firehydrant.lvd",
        "fighter/pickel/model/forge/c00/forge.lvd",
        "fighter/kamui/ryusensa/c00/motion_list.bin",
    ] {
        let h = paths::path_hash(path);
        if !p.rules.keep_shared.contains(&h) {
            p.rules.keep_shared.push(h);
        }
    }
    for entry in &p.entries {
        if entry.path.starts_with("fighter/samusd/model/") {
            for token in entry.path.split('/') {
                if slot(token) && !extra(token) {
                    p.rules
                        .preprocess
                        .entry(paths::hash(&format!("fighter/samusd/result/{token}")))
                        .or_insert(paths::hash(&format!("fighter/samusd/{token}")));
                }
            }
        }
    }

    let mut dirs: BTreeSet<String> = p.rules.directories.iter().cloned().collect();
    for e in &p.entries {
        let c: Vec<_> = e.path.split('/').collect();
        let mut group = None;
        if c.len() >= 4 && c[0] == "fighter" {
            if let Some((i, s)) = c.iter().enumerate().find(|(_, s)| extra(s)) {
                let f = c[1];
                group = match c[2] {
                    "model" | "motion" | "sound" | "effect" | "param" if i >= 3 => {
                        Some(format!("fighter/{f}/{s}"))
                    }
                    "camera" | "result" | "movie" if i == 3 => {
                        Some(format!("fighter/{f}/{}/{s}", c[2]))
                    }
                    "kirbycopy" if i >= 4 => Some(format!("fighter/{f}/kirbycopy/{s}")),
                    _ => None,
                };
                if c[2] == "model" && f == "kirby" && c.len() >= 6 {
                    if let Some(v) = c[3]
                        .strip_prefix("copy_")
                        .and_then(|v| v.strip_suffix("_cap"))
                    {
                        group = Some(format!("fighter/{v}/kirbycopy/{s}"));
                    }
                }
            }
        } else if c.len() >= 5 && c[0] == "camera" && c[1] == "fighter" && extra(c[3]) {
            group = Some(format!("fighter/{}/camera/{}", c[2], c[3]));
        } else if c.len() == 4
            && c[0] == "sound"
            && c[1] == "bank"
            && matches!(c[2], "fighter" | "fighter_voice")
        {
            let stem = c[3].rsplit_once('.').map(|(s, _)| s).unwrap_or(c[3]);
            if let Some(rest) = stem
                .strip_prefix("se_")
                .or_else(|| stem.strip_prefix("vc_"))
            {
                if let Some((name, s)) = rest.rsplit_once('_') {
                    if extra(s) {
                        group = Some(format!(
                            "fighter/{}/{s}",
                            name.strip_suffix("_cheer").unwrap_or(name)
                        ));
                    }
                }
            }
        } else if c.len() >= 4 && c[0] == "effect" && c[1] == "fighter" {
            let f = c[2];
            if c.len() >= 5 {
                if let Some(s) = c[3].strip_prefix("trail_") {
                    if slot(s) {
                        group = Some(format!("fighter/{f}/{s}"));
                    }
                }
            } else if let Some(rest) = c[3].strip_prefix(&format!("ef_{f}_")) {
                if let Some((s, _)) = rest.split_once('.') {
                    if slot(s) {
                        group = Some(format!("fighter/{f}/{s}"));
                    }
                }
            }
        } else if c.len() >= 4 && c[0] == "stage" {
            group = e.path.rsplit_once('/').map(|(s, _)| s.to_string());
        }
        if let Some(g) = group {
            dirs.insert(g.clone());
            p.rules
                .directory_files
                .entry(paths::path_hash(&g))
                .or_default()
                .push(e.hash);
        }
    }
    let clones: Vec<_> = dirs.iter().cloned().collect();
    for d in clones {
        let c: Vec<_> = d.split('/').collect();
        if c.len() == 3 && c[0] == "fighter" && extra(c[2]) {
            for sub in ["camera", "cmn"] {
                let key = format!("{d}/{sub}");
                if !p.rules.directory_bases.contains_key(&key) {
                    p.rules.optional_bases.push(key.clone());
                    p.rules
                        .directory_bases
                        .insert(key, format!("fighter/{}/c00/{sub}", c[1]));
                }
            }
        }
        if c.len() == 4 && c[0] == "fighter" && c[2] == "kirbycopy" && extra(c[3]) {
            for sub in ["bodymotion", "cmn", "sound"] {
                let key = format!("{d}/{sub}");
                if !p.rules.directory_bases.contains_key(&key) {
                    p.rules.optional_bases.push(key.clone());
                    p.rules
                        .directory_bases
                        .insert(key, format!("fighter/{}/kirbycopy/c00/{sub}", c[1]));
                }
            }
        }
        if c.len() == 3 && c[0] == "fighter" && c[1] == "kirby" && extra(c[2]) {
            for owner in [
                "mario", "luigi", "donkey", "link", "samus", "samusd", "yoshi", "fox", "pikachu",
                "ness", "captain", "purin", "peach", "pickel",
            ] {
                dirs.insert(format!("fighter/{owner}/kirbycopy/{}", c[2]));
            }
        }
    }
    for files in p.rules.directory_files.values_mut() {
        files.sort_unstable();
        files.dedup();
    }
    p.rules.directories = dirs.into_iter().collect();
}

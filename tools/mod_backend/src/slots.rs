//! Virtual costume routing. Disk paths and package bytes always retain the author's layout.
use serde_json::Value;
use std::collections::BTreeMap;

pub fn validate(map: &BTreeMap<String, u16>) -> Result<(), String> {
    for (key, to) in map {
        let (fighter, color) = key.split_once(':').ok_or("slot key must be fighter:c00")?;
        if fighter.is_empty()
            || !fighter
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
            || color
                .strip_prefix('c')
                .and_then(|s| s.parse::<u16>().ok())
                .is_none_or(|n| n > 255)
            || *to > 255
        {
            return Err("invalid costume mapping (valid range c00..c255)".into());
        }
    }
    Ok(())
}
fn color(s: &str) -> Option<u16> {
    let n = s.strip_prefix('c')?;
    if n.len() < 2 || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse::<u16>().ok().filter(|n| *n <= 255)
}
/// (fighter:cNN, range containing only the numeric costume index).
pub fn identify(path: &str) -> Option<(String, std::ops::Range<usize>)> {
    let parts: Vec<_> = path.split('/').collect();
    let fighter = if parts.first() == Some(&"fighter") && parts.len() > 2 {
        if parts.get(1) == Some(&"kirby") {
            parts
                .iter()
                .find_map(|s| s.strip_prefix("copy_").and_then(|s| s.strip_suffix("_cap")))
                .unwrap_or(parts[1])
        } else {
            parts[1]
        }
    } else if parts.first() == Some(&"camera")
        && parts.get(1) == Some(&"fighter")
        && parts.len() > 2
    {
        parts[2]
    } else if parts.first() == Some(&"effect")
        && parts.get(1) == Some(&"fighter")
        && parts.len() > 2
    {
        parts[2]
    } else {
        ""
    };
    if !fighter.is_empty() {
        let mut offset = 0;
        for p in &parts {
            if let Some(n) = color(p) {
                return Some((format!("{fighter}:c{n:02}"), offset + 1..offset + p.len()));
            }
            offset += p.len() + 1;
        }
        if let Some(at) = path.rfind("_c") {
            let start = at + 2;
            let len = path[start..]
                .bytes()
                .take_while(|b| b.is_ascii_digit())
                .count();
            let n = color(&path[at + 1..start + len])?;
            return Some((format!("{fighter}:c{n:02}"), start..start + len));
        }
    }
    let name = parts.last()?;
    let stem = name.split('.').next()?;
    let offset = path.len() - name.len();
    if path.starts_with("sound/bank/") {
        let (owner, n) = stem.rsplit_once("_c")?;
        let owner = owner
            .strip_prefix("vc_")
            .or_else(|| owner.strip_prefix("se_"))?;
        let owner = owner.strip_suffix("_cheer").unwrap_or(owner);
        let n = color(&format!("c{n}"))?;
        return Some((
            format!("{owner}:c{n:02}"),
            offset + stem.rfind("_c")? + 2..offset + stem.len(),
        ));
    }
    if path.starts_with("ui/") && stem.starts_with("chara_") {
        let (prefix, index) = stem.rsplit_once('_')?;
        let owner = prefix.strip_prefix("chara_")?.split_once('_')?.1;
        let n = index.parse::<u16>().ok().filter(|n| *n <= 255)?;
        return Some((
            format!("{owner}:c{n:02}"),
            offset + prefix.len() + 1..offset + stem.len(),
        ));
    }
    None
}
pub fn remap(path: &str, map: &BTreeMap<String, u16>) -> String {
    if let Some((key, range)) = identify(path) {
        if let Some(to) = map.get(&key) {
            return format!("{}{:02}{}", &path[..range.start], to, &path[range.end..]);
        }
    }
    path.into()
}
pub fn rules(value: Value, map: &BTreeMap<String, u16>) -> Result<Value, String> {
    if map.is_empty() {
        return Ok(value);
    }
    match value {
        Value::String(s) if s.starts_with("0x") || s.bytes().all(|b| b.is_ascii_digit()) => {
            Err("hashed config paths cannot be automatically moved between costume slots".into())
        }
        Value::Number(_) => {
            Err("hashed config paths cannot be automatically moved between costume slots".into())
        }
        Value::String(s) => Ok(Value::String(remap(&s, map))),
        Value::Array(a) => a
            .into_iter()
            .map(|v| rules(v, map))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(o) => o
            .into_iter()
            .map(|(k, v)| {
                if k.starts_with("0x") || k.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(
                        "hashed config paths cannot be automatically moved between costume slots"
                            .into(),
                    );
                }
                Ok((remap(&k, map), rules(v, map)?))
            })
            .collect::<Result<serde_json::Map<_, _>, String>>()
            .map(Value::Object),
        v => Ok(v),
    }
}

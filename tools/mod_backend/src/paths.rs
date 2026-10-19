use std::path::Path;
pub const MASK: u64 = 0xff_ffff_ffff;
pub const NONE: u32 = 0xff_ffff;
pub const REGIONS: [&str; 15] = [
    "none", "jp_ja", "us_en", "us_fr", "us_es", "eu_en", "eu_fr", "eu_es", "eu_de", "eu_nl",
    "eu_it", "eu_ru", "kr_ko", "zh_cn", "zh_tw",
];
pub fn hash(s: &str) -> u64 {
    let b = s.as_bytes();
    ((b.len() as u64) << 32) | crc32fast::hash(b) as u64
}
pub fn region(s: &str) -> Result<usize, String> {
    REGIONS
        .iter()
        .position(|r| r.eq_ignore_ascii_case(s))
        .filter(|r| *r > 0)
        .ok_or_else(|| format!("unknown region {s}"))
}
pub fn locale(r: usize) -> usize {
    match r {
        1 => 1,
        2..=4 => 2,
        5..=11 => 3,
        12 => 4,
        13..=14 => 5,
        _ => 0,
    }
}
pub fn normalize(s: &str) -> Result<(String, Option<usize>), String> {
    let mut s = s
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase()
        .replace(';', ":");
    if let Some(v) = s.strip_prefix("romfs/") {
        s = v.to_string();
    }
    let mut r = None;
    if let Some(i) = s.rfind('+') {
        if let Some(code) = s.get(i + 1..i + 6) {
            if let Ok(n) = region(code) {
                r = Some(n);
                s.replace_range(i..i + 6, "");
            }
        }
    }
    if s.ends_with(".mp4") {
        s.truncate(s.len() - 4);
        s.push_str(".webm");
    }
    if s.ends_with(".lua") {
        s.truncate(s.len() - 4);
        s.push_str(".lc");
    }
    // These are archive namespaces, not host drive letters. Keep prebuilt:/
    // in the hash: the original archive contains files under that exact prefix.
    let archive_path = s
        .strip_prefix("stream:/")
        .or_else(|| s.strip_prefix("prebuilt:/"))
        .unwrap_or(&s);
    if s.is_empty()
        || s.split('/').any(|c| c.is_empty() || c == "." || c == "..")
        || s.as_bytes().contains(&0)
        || archive_path.contains(':')
        || s.len() > 255
    {
        return Err(format!("invalid archive path {s:?}"));
    }
    Ok((s, r))
}
pub fn path_hash(s: &str) -> u64 {
    let base = s.rsplit('/').next().unwrap_or(s);
    if !base.contains('.') && base.starts_with("0x") {
        if let Ok(h) = u64::from_str_radix(&base[2..], 16) {
            return h & MASK;
        }
    }
    // Hash-named assets from the existing fighter tools contain a filename Hash40.
    let stem = base.split('.').next().unwrap_or(base);
    if stem.len() == 12 && stem.starts_with("0x") {
        if let Ok(n) = u64::from_str_radix(&stem[2..], 16) {
            if let Some(i) = s.rfind('/') {
                let len = (n >> 32) as usize;
                return (((i + 1 + len) as u64) << 32)
                    | (shift(crc32fast::hash(s[..i + 1].as_bytes()), len) ^ n as u32) as u64;
            }
        }
    }
    hash(s)
}
fn shift(mut crc: u32, len: usize) -> u32 {
    // CRC32 concatenation, exponentiation of the zero-byte linear operator.
    let mut op = [0u32; 32];
    for (i, v) in op.iter_mut().enumerate() {
        let mut x = 1u32 << i;
        for _ in 0..8 {
            x = (x >> 1) ^ if x & 1 != 0 { 0xedb88320 } else { 0 };
        }
        *v = x;
    }
    fn apply(op: &[u32; 32], mut x: u32) -> u32 {
        let mut y = 0;
        let mut i = 0;
        while x != 0 {
            if x & 1 != 0 {
                y ^= op[i];
            }
            x >>= 1;
            i += 1;
        }
        y
    }
    let mut n = len;
    while n > 0 {
        if n & 1 != 0 {
            crc = apply(&op, crc);
        }
        op = std::array::from_fn(|i| apply(&op, op[i]));
        n >>= 1;
    }
    crc
}
pub fn config_hash(v: &serde_json::Value) -> Result<u64, String> {
    if let Some(n) = v.as_u64() {
        if n <= MASK {
            return Ok(n);
        }
    }
    if let Some(s) = v.as_str() {
        if let Some(x) = s.strip_prefix("0x") {
            return u64::from_str_radix(x, 16)
                .map(|h| h & MASK)
                .map_err(|e| e.to_string());
        }
        return normalize(s).map(|(s, _)| path_hash(&s));
    }
    Err(format!("invalid Hash40 {v}"))
}
pub fn patch_target(path: &str) -> Option<(String, &'static str)> {
    let ext = Path::new(path).extension()?.to_str()?;
    let (target, kind) = match ext {
        "prcx" | "prcxml" => ("prc", "prc"),
        "stdatx" | "stdatxml" => ("stdat", "prc"),
        "stprmx" | "stprmxml" => ("stprm", "prc"),
        "xmsbt" => ("msbt", "msbt"),
        "patch3audio" => ("nus3audio", "audio"),
        "motdiff" => ("bin", "motion"),
        "yml" if path.ends_with("motion_list.yml") => ("bin", "motion"),
        "bin" if path.ends_with("bgm_property.bin") => ("bin", "bgm"),
        _ => return None,
    };
    Some((
        format!("{}.{}", path.rsplit_once('.').unwrap().0, target),
        kind,
    ))
}

#[derive(Clone, Debug)]
pub struct FolderSpec {
    pub hash: u64,
    pub name: Option<u64>,
    pub parent: Option<Box<FolderSpec>>,
}
#[derive(Clone, Debug)]
pub struct FileSpec {
    pub hash: u64,
    pub name: u64,
    pub ext: u64,
    pub parent: FolderSpec,
}
pub fn folder_spec(v: &serde_json::Value, depth: usize) -> Result<FolderSpec, String> {
    if depth > 64 {
        return Err("configuration folder nesting exceeds 64".into());
    }
    if let Some(s) = v.as_str() {
        if s.starts_with("0x") {
            return Ok(FolderSpec {
                hash: config_hash(v)?,
                name: None,
                parent: None,
            });
        }
        let s = normalize(s)?.0;
        let parent = s
            .rsplit_once('/')
            .map(|(p, _)| folder_spec(&serde_json::json!(p), depth + 1))
            .transpose()?
            .map(Box::new);
        let name = s.rsplit('/').next().unwrap();
        return Ok(FolderSpec {
            hash: path_hash(&s),
            name: Some(hash(name)),
            parent,
        });
    }
    let o = v
        .as_object()
        .ok_or("folder must be a path or structured folder")?;
    let name = config_hash(o.get("name").ok_or("folder name missing")?)?;
    let parent = folder_spec(o.get("parent").ok_or("folder parent missing")?, depth + 1)?;
    let h = hash40::Hash40(parent.hash)
        .join_path(hash40::Hash40(name))
        .0;
    Ok(FolderSpec {
        hash: h,
        name: Some(name),
        parent: Some(Box::new(parent)),
    })
}
pub fn file_spec(v: &serde_json::Value) -> Result<FileSpec, String> {
    if let Some(s) = v.as_str() {
        let s = normalize(s)?.0;
        let (p, n) = s
            .rsplit_once('/')
            .ok_or("shared file needs a parent directory")?;
        let (_, ext) = n.rsplit_once('.').ok_or("shared file needs an extension")?;
        return Ok(FileSpec {
            hash: path_hash(&s),
            name: name_hash(n),
            ext: hash(ext),
            parent: folder_spec(&serde_json::json!(p), 0)?,
        });
    }
    let o = v
        .as_object()
        .ok_or("shared file must be a path or structured file")?;
    let name = config_hash(
        o.get("file-name")
            .or_else(|| o.get("file_name"))
            .ok_or("shared file name missing")?,
    )?;
    let parent = folder_spec(o.get("parent").ok_or("shared file parent missing")?, 0)?;
    let ext = config_hash(o.get("extension").ok_or("shared file extension missing")?)?;
    let h = hash40::Hash40(parent.hash)
        .join_path(hash40::Hash40(name))
        .0;
    Ok(FileSpec {
        hash: h,
        name,
        ext,
        parent,
    })
}
pub fn name_hash(s: &str) -> u64 {
    let stem = s.split('.').next().unwrap_or(s);
    if stem.len() == 12 && stem.starts_with("0x") {
        if let Ok(h) = u64::from_str_radix(&stem[2..], 16) {
            return h & MASK;
        }
    }
    hash(s)
}

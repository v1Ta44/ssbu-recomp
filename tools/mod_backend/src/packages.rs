use crate::paths;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub root: PathBuf,
    pub cache: PathBuf,
    pub profile: PathBuf,
    pub region: String,
    pub userdata: PathBuf,
    pub system_language: String,
    pub conflict_mode: String,
    pub packages: BTreeMap<String, Selection>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            root: "../mods".into(),
            cache: "../cache/mods".into(),
            profile: "../config/mods.json".into(),
            region: "auto".into(),
            userdata: "../userdata".into(),
            system_language: "en-US".into(),
            conflict_mode: "priority".into(),
            packages: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Selection {
    pub enabled: bool,
    pub priority: i32,
    pub alias: String,
    pub slot_map: BTreeMap<String, u16>,
}
impl Default for Selection {
    fn default() -> Self {
        Self {
            enabled: true,
            priority: 0,
            alias: String::new(),
            slot_map: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Serialize)]
pub struct Package {
    pub id: String,
    pub path: PathBuf,
    pub enabled: bool,
    pub priority: i32,
    pub metadata: Value,
    pub files: usize,
    pub plugins: Vec<PathBuf>,
    pub resources: Vec<String>,
    pub costumes: Vec<String>,
}
#[derive(Clone, Serialize)]
pub struct Source {
    pub package: String,
    pub disk: PathBuf,
    pub size: u64,
    pub kind: String,
}
#[derive(Clone, Serialize)]
pub struct Entry {
    pub path: String,
    pub hash: u64,
    pub base: Option<Source>,
    pub patches: Vec<Source>,
    pub candidates: Vec<Source>,
}
#[derive(Default, Clone, Serialize)]
pub struct Rules {
    pub keep_shared: Vec<u64>,
    pub shares: BTreeMap<u64, Vec<Value>>,
    pub preprocess: BTreeMap<u64, u64>,
    pub directories: Vec<String>,
    pub directory_bases: BTreeMap<String, String>,
    #[serde(skip)]
    pub optional_bases: Vec<String>,
    pub directory_files: BTreeMap<u64, Vec<u64>>,
    #[serde(skip)]
    pub directory_file_sources: BTreeMap<(u64, u64), Vec<String>>,
}
#[derive(Clone, Serialize)]
pub struct Diagnostic {
    pub level: String,
    pub code: String,
    pub message: String,
}
#[derive(Clone, Serialize)]
pub struct Plan {
    pub packages: Vec<Package>,
    pub entries: Vec<Entry>,
    pub rules: Rules,
    pub diagnostics: Vec<Diagnostic>,
    pub region: String,
}
fn diag(out: &mut Vec<Diagnostic>, code: &str, msg: String) {
    out.push(Diagnostic {
        level: "warning".into(),
        code: code.into(),
        message: msg,
    });
}
fn walk(
    root: &Path,
    at: &Path,
    out: &mut Vec<PathBuf>,
    d: &mut Vec<Diagnostic>,
) -> Result<(), String> {
    let mut children = fs::read_dir(at)
        .map_err(|e| format!("{}: {e}", at.display()))?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    children.sort();
    for p in children {
        let meta = fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        let reparse = {
            use std::os::windows::fs::MetadataExt;
            meta.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let reparse = meta.file_type().is_symlink();
        if reparse {
            diag(
                d,
                "linked_path",
                format!("skipped linked path {}", p.display()),
            );
            continue;
        }
        let name = p.file_name().unwrap().to_string_lossy();
        if name.starts_with('.') || name == "src" || name == "__pycache__" {
            continue;
        }
        if meta.is_dir() {
            walk(root, &p, out, d)?;
        } else if meta.is_file() {
            out.push(
                p.strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_path_buf(),
            );
        }
    }
    Ok(())
}
fn merge_rules(v: Value, r: &mut Rules, source: &Path) -> Result<(), String> {
    let o = v.as_object().ok_or("config.json must be an object")?;
    for (key, v) in o {
        let key = key.replace('-', "_");
        match key.as_str() {
            "unshare_blacklist" | "keep_shared" => {
                for v in v.as_array().ok_or("keep-shared must be an array")? {
                    r.keep_shared.push(paths::config_hash(v)?);
                }
            }
            "preprocess_reshare" => {
                for (k, v) in v
                    .as_object()
                    .ok_or("preprocess-reshare must be an object")?
                {
                    r.preprocess
                        .insert(paths::config_hash(&json!(k))?, paths::config_hash(v)?);
                }
            }
            "share_to_vanilla" | "share_to_added" | "new_shared_files" => {
                for (k, v) in v.as_object().ok_or("share-to must be an object")? {
                    r.shares
                        .entry(paths::config_hash(&json!(k))?)
                        .or_default()
                        .extend(if let Some(a) = v.as_array() {
                            a.clone()
                        } else {
                            vec![v.clone()]
                        });
                }
            }
            "new_dir_infos" => {
                for p in v.as_array().ok_or("new-dir-infos must be an array")? {
                    r.directories
                        .push(paths::normalize(p.as_str().ok_or("directory must be a string")?)?.0);
                }
            }
            "new_dir_infos_base" => {
                for (k, v) in v
                    .as_object()
                    .ok_or("new-dir-infos-base must be an object")?
                {
                    r.directory_bases.insert(
                        paths::normalize(k)?.0,
                        paths::normalize(v.as_str().ok_or("base directory must be a string")?)?.0,
                    );
                }
            }
            "new_dir_files" => {
                for (k, v) in v.as_object().ok_or("new-dir-files must be an object")? {
                    let directory = paths::config_hash(&json!(k))?;
                    for v in v.as_array().ok_or("new-dir-files values must be arrays")? {
                        let file = paths::config_hash(v)?;
                        r.directory_files.entry(directory).or_default().push(file);
                        r.directory_file_sources
                            .entry((directory, file))
                            .or_default()
                            .push(format!("{}: directory {k}, asset {v}", source.display()));
                    }
                }
            }
            _ => return Err(format!("unrecognized ARCropolis config field {key}")),
        }
    }
    Ok(())
}
pub fn scan(c: &Config) -> Result<Plan, String> {
    let region = resolved_region(c)?;
    if !matches!(c.conflict_mode.as_str(), "priority" | "error") {
        return Err("conflict_mode must be priority or error".into());
    }
    let mut plan = Plan {
        packages: vec![],
        entries: vec![],
        rules: Rules::default(),
        diagnostics: vec![],
        region: paths::REGIONS[region].to_string(),
    };
    let mut entries = BTreeMap::<u64, Entry>::new();
    if !c.root.exists() {
        return Ok(plan);
    }
    for p in fs::read_dir(&c.root).map_err(|e| e.to_string())? {
        let p = p.map_err(|e| e.to_string())?;
        if !p.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        let id = p
            .file_name()
            .into_string()
            .map_err(|_| "package name is not UTF-8")?;
        if id.starts_with('.') {
            continue;
        }
        let select = c.packages.get(&id).cloned().unwrap_or_default();
        plan.packages.push(Package {
            id,
            path: p.path(),
            enabled: select.enabled,
            priority: select.priority,
            metadata: json!({}),
            files: 0,
            plugins: vec![],
            resources: vec![],
            costumes: vec![],
        });
    }
    // Apply low priority first. Equal priority uses a documented, stable folder order.
    plan.packages
        .sort_by(|a, b| a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)));
    for pkg in &mut plan.packages {
        let mut mapped_paths = BTreeMap::<String, String>::new();
        let selection = c.packages.get(&pkg.id).cloned().unwrap_or_default();
        crate::slots::validate(&selection.slot_map)?;
        let mut files = vec![];
        walk(&pkg.path, &pkg.path, &mut files, &mut plan.diagnostics)?;
        files.sort_by_key(|p| {
            (
                paths::normalize(&p.to_string_lossy())
                    .ok()
                    .and_then(|(_, r)| r)
                    .is_some(),
                p.clone(),
            )
        });
        for rel in files {
            let disk = pkg.path.join(&rel);
            let raw = rel
                .to_str()
                .ok_or("asset path is not UTF-8")?
                .replace('\\', "/");
            let base = rel
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase();
            if base.ends_with(".nro") || base.ends_with(".dll") {
                if pkg.enabled && !selection.slot_map.is_empty() {
                    return Err(format!("{}: executable plugin packages cannot be automatically moved between costume slots", pkg.id));
                }
                pkg.plugins.push(disk.clone());
                if pkg.enabled {
                    diag(
                        &mut plan.diagnostics,
                        "plugin_dependency",
                        format!(
                            "{} includes {}; executable plugins need their own native adaptation",
                            pkg.id, raw
                        ),
                    );
                }
                continue;
            }
            if !raw.contains('/') && base == "info.toml" {
                pkg.metadata =
                    json!({"info_toml":fs::read_to_string(&disk).map_err(|e|e.to_string())?});
                continue;
            }
            if raw.contains('/') || raw.starts_with("0x") {
                if let Ok((local, _)) = paths::normalize(&raw) {
                    if let Some((key, _)) = crate::slots::identify(&local) {
                        if !pkg.costumes.contains(&key) {
                            pkg.costumes.push(key);
                        }
                    }
                    pkg.resources.push(local);
                }
            }
            if !pkg.enabled {
                continue;
            }
            if base == "config.json" {
                let v = serde_json::from_slice(&fs::read(&disk).map_err(|e| e.to_string())?)
                    .map_err(|e| format!("{}: {e}", disk.display()))?;
                let v = crate::slots::rules(v, &selection.slot_map)?;
                merge_rules(v, &mut plan.rules, &disk)
                    .map_err(|e| format!("{}: {e}", disk.display()))?;
                continue;
            }
            if !raw.contains('/') && !raw.starts_with("0x") {
                continue;
            }
            if raw.starts_with("0x") && !selection.slot_map.is_empty() {
                return Err(format!(
                    "{}: hash-only assets cannot be automatically moved between costume slots",
                    pkg.id
                ));
            }
            let (local, r) = paths::normalize(&raw)
                .map_err(|e| format!("{}: {raw}: {e}", pkg.id))?;
            let mapped = crate::slots::remap(&local, &selection.slot_map);
            if !selection.slot_map.is_empty() {
                if let Some(previous) = mapped_paths.insert(mapped.clone(), local.clone()) {
                    if previous != local {
                        return Err(format!(
                            "{}: costume mapping sends {previous} and {local} to the same resource",
                            pkg.id
                        ));
                    }
                }
            }
            let local = mapped;
            if r.is_some_and(|r| r != region) {
                continue;
            }
            let (target, kind) = paths::patch_target(&local).unwrap_or((local.clone(), "file"));
            // ARCropolis skips malformed XMSBT files at discovery, while keeping the
            // rest of the package and valid patches for the same message table.
            if kind == "msbt" {
                if let Err(error) = crate::patches::validate_xmsbt(&disk) {
                    diag(
                        &mut plan.diagnostics,
                        "invalid_xmsbt",
                        format!("{}: skipped malformed XMSBT {}: {error}", pkg.id, raw),
                    );
                    continue;
                }
            }
            let h = paths::path_hash(&target);
            let src = Source {
                package: pkg.id.clone(),
                disk: disk.clone(),
                size: fs::metadata(&disk).map_err(|e| e.to_string())?.len(),
                kind: kind.into(),
            };
            let ent = entries.entry(h).or_insert_with(|| Entry {
                path: target.clone(),
                hash: h,
                base: None,
                patches: vec![],
                candidates: vec![],
            });
            if ent.path != target {
                return Err(format!("Hash40 collision: {} and {target}", ent.path));
            }
            ent.candidates.push(src.clone());
            if kind == "file" {
                if let Some(prev) = &ent.base {
                    if c.conflict_mode == "error" {
                        return Err(format!(
                            "resource conflict {target}: {} / {}",
                            prev.package, pkg.id
                        ));
                    }
                    diag(
                        &mut plan.diagnostics,
                        "resource_conflict",
                        format!("{target}: {} overrides {}", pkg.id, prev.package),
                    );
                }
                ent.base = Some(src);
            } else {
                ent.patches.push(src);
            }
            pkg.files += 1;
        }
    }
    plan.entries = entries.into_values().collect();
    crate::inference::apply(&mut plan);
    Ok(plan)
}
pub fn save(c: &Config) -> Result<(), String> {
    if c.profile.extension().is_none_or(|e| e != "json") {
        return Err("profiles must use a .json file outside the game inputs".into());
    }

    if let Some(p) = c.profile.parent() {
        fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let tmp = c
        .profile
        .with_extension(format!("{}.tmp", std::process::id()));
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        unsafe extern "system" {
            fn MoveFileExW(a: *const u16, b: *const u16, f: u32) -> i32;
        }
        let a: Vec<_> = tmp.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<_> = c.profile.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 9) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    #[cfg(not(windows))]
    fs::rename(tmp, &c.profile).map_err(|e| e.to_string())?;
    Ok(())
}

// SaveLanguageId byte and offset: ARCropolis utils.rs; read-only 13.0.4 save.
pub fn resolved_region(c: &Config) -> Result<usize, String> {
    if c.region != "auto" {
        return paths::region(&c.region);
    }
    let sys = c.system_language.to_ascii_lowercase();
    let us = matches!(sys.as_str(), "en-us" | "en-ca" | "fr-ca" | "es-419");
    let fallback = match sys.as_str() {
        "ja" => 1,
        "en-us" | "en-ca" => 2,
        "fr-ca" => 3,
        "es-419" => 4,
        "en-gb" => 5,
        "fr" => 6,
        "es" => 7,
        "de" => 8,
        "nl" => 9,
        "it" => 10,
        "ru" => 11,
        "ko" => 12,
        "zh-hans" | "zh-cn" => 13,
        "zh-hant" | "zh-tw" => 14,
        _ => 2,
    };
    use std::io::{Read, Seek, SeekFrom};
    if let Ok(mut f) = fs::File::open(c.userdata.join("save/save_data/system_data.bin")) {
        let mut b = [255];
        if f.seek(SeekFrom::Start(0x3c6098)).is_ok() && f.read_exact(&mut b).is_ok() {
            return Ok(match b[0] {
                0 => 1,
                1 => {
                    if us {
                        2
                    } else {
                        5
                    }
                }
                2 => {
                    if us {
                        3
                    } else {
                        6
                    }
                }
                3 => {
                    if us {
                        4
                    } else {
                        7
                    }
                }
                4 => 8,
                5 => 10,
                6 => 9,
                7 => 11,
                8 => 13,
                9 => 14,
                10 => 12,
                _ => fallback,
            });
        }
    }
    Ok(fallback)
}

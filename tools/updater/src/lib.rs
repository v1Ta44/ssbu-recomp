//! Signed portable updates. Only explicitly managed distribution files can be replaced.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ring::signature::{self, KeyPair};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(feature = "network")]
pub mod network;
pub type Result<T> = std::result::Result<T, String>;
pub const UPDATER_VERSION: u32 = 1;
pub const MAX_METADATA: u64 = 2 * 1024 * 1024;
pub const MAX_ARCHIVE: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_EXTRACTED: u64 = 8 * 1024 * 1024 * 1024;
pub const MAX_FILES: usize = 30000;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn unique() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(err)?;
    let mut sha = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(err)?;
        if n == 0 {
            break;
        }
        sha.update(&buf[..n]);
    }
    Ok(format!("{:x}", sha.finalize()))
}
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).map_err(err)?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(err)
}
pub fn save_json(path: &Path, value: &impl Serialize) -> Result<()> {
    atomic_bytes(path, &serde_json::to_vec_pretty(value).map_err(err)?)
}
pub fn atomic_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or("文件路径无效")?;
    fs::create_dir_all(parent).map_err(err)?;
    let temp = parent.join(format!(".update-{}.tmp", unique()));
    let result = (|| {
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(err)?;
        f.write_all(bytes).map_err(err)?;
        f.sync_all().map_err(err)?;
        drop(f);
        replace(&temp, path)
    })();
    if temp.exists() {
        let _ = fs::remove_file(&temp);
    }
    result
}
fn replace(src: &Path, dst: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let a: Vec<u16> = src.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<u16> = dst.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe {
            MoveFileExW(
                a.as_ptr(),
                b.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(format!(
                "无法替换 {}，请关闭占用它的程序：{}",
                dst.display(),
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(src, dst).map_err(err)
    }
}
fn copy_sync(src: &Path, dst: &Path) -> Result<()> {
    if let Some(p) = dst.parent() {
        fs::create_dir_all(p).map_err(err)?;
    }
    let mut input = File::open(src).map_err(err)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)
        .map_err(err)?;
    std::io::copy(&mut input, &mut output).map_err(err)?;
    output.sync_all().map_err(err)
}
fn replace_copy(src: &Path, dst: &Path) -> Result<()> {
    let parent = dst.parent().ok_or("路径错误")?;
    fs::create_dir_all(parent).map_err(err)?;
    let tmp = parent.join(format!(".update-{}.tmp", unique()));
    let result = (|| {
        copy_sync(src, &tmp)?;
        replace(&tmp, dst)
    })();
    if tmp.exists() {
        let _ = fs::remove_file(tmp);
    }
    result
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Trust {
    pub keys: BTreeMap<String, String>,
}
impl Trust {
    pub fn bundled() -> Self {
        serde_json::from_str(include_str!("../trust.json").trim_start_matches('\u{feff}'))
            .expect("release trust")
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub key_id: String,
    pub payload: String,
    pub signature: String,
}
pub fn verify<T: serde::de::DeserializeOwned>(envelope: &Envelope, trust: &Trust) -> Result<T> {
    let public = trust
        .keys
        .get(&envelope.key_id)
        .ok_or("更新签名的发布者不受信任，请升级更新器")?;
    let bytes = B64
        .decode(&envelope.payload)
        .map_err(|_| "更新清单编码损坏")?;
    if bytes.len() as u64 > MAX_METADATA {
        return Err("更新清单过大".into());
    }
    let key = B64.decode(public).map_err(err)?;
    let sig = B64.decode(&envelope.signature).map_err(err)?;
    signature::UnparsedPublicKey::new(&signature::ED25519, key)
        .verify(&bytes, &sig)
        .map_err(|_| "更新签名校验失败，文件可能损坏或来源不正确")?;
    serde_json::from_slice(&bytes).map_err(err)
}
pub fn sign(bytes: &[u8], key: &[u8], key_id: &str) -> Result<Envelope> {
    let pair = signature::Ed25519KeyPair::from_pkcs8(key).map_err(err)?;
    Ok(Envelope {
        key_id: key_id.into(),
        payload: B64.encode(bytes),
        signature: B64.encode(pair.sign(bytes).as_ref()),
    })
}
pub fn generate_key() -> Result<(Vec<u8>, String)> {
    let rng = ring::rand::SystemRandom::new();
    let pk = signature::Ed25519KeyPair::generate_pkcs8(&rng).map_err(err)?;
    let pair = signature::Ed25519KeyPair::from_pkcs8(pk.as_ref()).map_err(err)?;
    Ok((pk.as_ref().to_vec(), B64.encode(pair.public_key().as_ref())))
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Features {
    pub resource_mods: bool,
    pub latency_slider: bool,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Version {
    pub schema: u32,
    pub channel: String,
    pub platform: String,
    pub sequence: u64,
    pub version: String,
    pub build_id: String,
    pub game_version: String,
    pub features: Features,
    pub minimum_updater: u32,
}
impl Version {
    pub fn validate(&self) -> Result<()> {
        if self.schema != 1
            || self.channel != "yuzu-test"
            || self.platform != "win-x64"
            || self.game_version != "13.0.4"
            || self.minimum_updater > UPDATER_VERSION
            || self.sequence == 0
            || self.version.is_empty()
            || self.build_id.is_empty()
        {
            return Err("此更新的通道、平台、游戏版本或更新器版本不兼容".into());
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Change {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub before: BTreeMap<String, Option<String>>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Removal {
    pub path: String,
    pub before: BTreeMap<String, String>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Package {
    #[serde(flatten)]
    pub version: Version,
    pub kind: String,
    #[serde(default)]
    pub base_build_ids: Vec<String>,
    pub notes: String,
    pub files: Vec<Change>,
    #[serde(default)]
    pub remove: Vec<Removal>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Source {
    pub name: String,
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub note: String,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Artifact {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub base_build_ids: Vec<String>,
    pub size: u64,
    pub sha256: String,
    pub sources: Vec<Source>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Release {
    #[serde(flatten)]
    pub version: Version,
    pub notes: String,
    pub published_at: String,
    pub artifacts: Vec<Artifact>,
}
pub fn hex_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn relative(path: &str) -> Result<PathBuf> {
    if path.is_empty()
        || path.len() > 230
        || path.contains('\\')
        || path.contains(':')
        || path.starts_with('/')
        || path.contains('\0')
    {
        return Err(format!("更新文件路径无效：{path}"));
    }
    for c in path.split('/') {
        let stem = c.split('.').next().unwrap_or("").to_ascii_uppercase();
        if c.is_empty()
            || c == "."
            || c == ".."
            || c.ends_with(['.', ' '])
            || c.chars().any(|x| x < ' ' || "<>\"|?*".contains(x))
            || ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit())
        {
            return Err(format!("更新文件路径无效：{path}"));
        }
    }
    Ok(PathBuf::from(path))
}
pub fn managed(path: &str) -> bool {
    if relative(path).is_err() {
        return false;
    }
    let p = path.to_ascii_lowercase();
    if [
        "ssbu console.exe",
        "ssbu updater.exe",
        "manifest.json",
        "version.json",
        "readme.txt",
        "license.txt",
        "replay.bat",
    ]
    .contains(&p.as_str())
    {
        return true;
    }
    if p.starts_with("romfs/.nrr/") {
        return p.ends_with(".nrr");
    }
    if p.starts_with("binaries/") {
        return p.ends_with(".elf") || p.ends_with(".npdm");
    }
    if p.starts_with("sysdata/fonts/") || p.starts_with("sysdata/zoneinfo/") {
        return true;
    }
    if p.starts_with("licenses/") || p.starts_with("game/mod_backend_licenses/") {
        return true;
    }
    if p == "launcher/launch.ps1" || p == "tests/seq_battle_play_fast.txt" {
        return true;
    }
    if let Some(t) = p.strip_prefix("game/") {
        if !t.contains('/') {
            return [".exe", ".dll", ".pdb", ".map", ".build.json"]
                .iter()
                .any(|e| t.ends_with(e));
        }
        if let Some(t) = t.strip_prefix("nro_bin_eh_o2/") {
            return !t.contains('/') && t.ends_with(".dll");
        }
        if t.starts_with("plugins/") {
            return t == "plugins/latency_slider.dll";
        }
        if t.starts_with("net_delay/") {
            return t == "net_delay/build.json"
                || t == "net_delay/delay_panel.exe"
                || t == "net_delay/webview2-license.txt"
                || t.starts_with("net_delay/ui/");
        }
    }
    false
}
fn reparse(p: &Path) -> bool {
    let Ok(md) = fs::symlink_metadata(p) else {
        return false;
    };
    if md.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if md.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}
pub fn safe_path(root: &Path, path: &str) -> Result<PathBuf> {
    let rel = relative(path)?;
    let mut current = root.to_path_buf();
    if reparse(&current) {
        return Err("更新目录不能是链接或联接点".into());
    }
    for c in rel.components() {
        current.push(c);
        if reparse(&current) {
            return Err(format!("更新目标包含链接：{}", current.display()));
        }
    }
    Ok(current)
}
pub fn root_path(root: &Path) -> Result<PathBuf> {
    if !root.is_dir() || reparse(root) {
        return Err("请选择真实的测试包目录".into());
    }
    root.canonicalize().map_err(err)
}
/// One console/installer per portable directory, across processes. Never delete the lock file.
pub struct RootLock {
    _file: File,
}
impl RootLock {
    pub fn acquire(root: &Path) -> Result<Self> {
        let updates = safe_path(root, "updates")?;
        fs::create_dir_all(&updates).map_err(err)?;
        let path = safe_path(root, "updates/console.lock")?;
        let mut opts = OpenOptions::new();
        opts.read(true).write(true).create(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            opts.share_mode(0);
        }
        let file = opts
            .open(path)
            .map_err(|_| "此目录的控制台或更新程序正在运行，请先关闭它")?;
        Ok(Self { _file: file })
    }
}
pub fn ensure_space(root: &Path, needed: u64) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let path: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut available = 0u64;
        if unsafe {
            GetDiskFreeSpaceExW(
                path.as_ptr(),
                &mut available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(err(std::io::Error::last_os_error()));
        }
        if available < needed.saturating_add(64 * 1024 * 1024) {
            return Err("磁盘剩余空间不足，请腾出空间再更新".into());
        }
    }
    #[cfg(not(windows))]
    let _ = (root, needed);
    Ok(())
}
fn validate_package(p: &Package) -> Result<()> {
    p.version.validate()?;
    if !["patch", "full"].contains(&p.kind.as_str())
        || (p.kind == "patch" && p.base_build_ids.is_empty())
        || p.files.is_empty()
        || p.files.len() + p.remove.len() > MAX_FILES
    {
        return Err("更新包清单不完整".into());
    }
    let mut paths = HashSet::new();
    let mut total = 0u64;
    for f in &p.files {
        if !managed(&f.path)
            || !hex_hash(&f.sha256)
            || f.before.values().flatten().any(|h| !hex_hash(h))
        {
            return Err(format!("更新包包含不允许替换的文件或无效哈希：{}", f.path));
        }
        if !paths.insert(f.path.to_ascii_lowercase()) {
            return Err("更新包有重复文件".into());
        }
        total = total.checked_add(f.size).ok_or("更新包过大")?;
    }
    for f in &p.remove {
        if !managed(&f.path)
            || f.path == "manifest.json"
            || f.path == "version.json"
            || f.before.is_empty()
            || f.before.values().any(|h| !hex_hash(h))
            || !paths.insert(f.path.to_ascii_lowercase())
        {
            return Err("更新包删除清单无效".into());
        }
    }
    for p in &paths {
        let mut s = p.as_str();
        while let Some((parent, _)) = s.rsplit_once('/') {
            if paths.contains(parent) {
                return Err("更新包有文件与目录冲突".into());
            }
            s = parent;
        }
    }
    if !p.files.iter().any(|f| f.path == "manifest.json")
        || !p.files.iter().any(|f| f.path == "version.json")
    {
        return Err("更新包必须包含版本与发行文件清单".into());
    }
    if total > MAX_EXTRACTED {
        return Err("更新包解压后过大".into());
    }
    Ok(())
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Installed {
    pub build_id: String,
    pub sequence: u64,
    pub version: String,
    pub features: Features,
    pub game_version: String,
}
pub fn installed(root: &Path, trust: &Trust) -> Result<Installed> {
    let manifest: serde_json::Value = read_json(&safe_path(root, "manifest.json")?)?;
    let features: Features = read_json(&safe_path(root, "game/ssbu_native.build.json")?)?;
    let mut out = Installed {
        build_id: manifest["build_id"]
            .as_str()
            .ok_or("包清单缺少 build_id")?
            .into(),
        sequence: 0,
        version: manifest["name"].as_str().unwrap_or("旧测试包").into(),
        features,
        game_version: "13.0.4".into(),
    };
    let path = safe_path(root, "updates/installed.json")?;
    if path.exists() {
        let e: Envelope = read_json(&path)?;
        let p: Package = verify(&e, trust)?;
        validate_package(&p)?;
        if p.version.build_id != out.build_id {
            return Err("已安装版本记录与包清单不一致，请先恢复上次更新".into());
        }
        let v = safe_path(root, "version.json")?;
        let record = p
            .files
            .iter()
            .find(|f| f.path == "version.json")
            .ok_or("更新版本记录缺失")?;
        if hash_file(&v)? != record.sha256 {
            return Err("版本记录被修改，请先恢复上次更新".into());
        }
        out.sequence = p.version.sequence;
        out.version = p.version.version;
        out.game_version = p.version.game_version;
    }
    Ok(out)
}
pub fn stage_archive(
    root: &Path,
    archive: &Path,
    trust: &Trust,
    progress: impl Fn(u64, u64),
) -> Result<PathBuf> {
    let root = root_path(root)?;
    if fs::metadata(archive).map_err(err)?.len() > MAX_ARCHIVE {
        return Err("更新归档过大".into());
    }
    let mut zip = zip::ZipArchive::new(File::open(archive).map_err(err)?).map_err(err)?;
    if zip.len() > MAX_FILES + 1 {
        return Err("更新归档文件太多".into());
    }
    let e: Envelope = {
        let mut entry = zip
            .by_name("update.json")
            .map_err(|_| "这不是带签名的更新包，请选择发行页提供的更新文件")?;
        if entry.size() > MAX_METADATA {
            return Err("更新清单过大".into());
        }
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(MAX_METADATA + 1)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        if bytes.len() as u64 > MAX_METADATA {
            return Err("更新清单过大".into());
        }
        serde_json::from_slice(&bytes).map_err(err)?
    };
    let p: Package = verify(&e, trust)?;
    validate_package(&p)?;
    let expected: BTreeMap<String, &Change> = p
        .files
        .iter()
        .map(|f| (format!("payload/{}", f.path), f))
        .collect();
    let mut seen = HashSet::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let f = zip.by_index(i).map_err(err)?;
        let name = f.name().map_err(err)?.into_owned();
        relative(&name)?;
        if f.is_dir()
            || f.is_symlink()
            || f.enclosed_name().is_none()
            || !seen.insert(name.to_ascii_lowercase())
        {
            return Err("更新归档有重复路径、链接或目录条目".into());
        }
        if name == "update.json" {
            continue;
        }
        let record = expected.get(&name).ok_or("更新归档包含清单以外的文件")?;
        if record.size != f.size() {
            return Err("更新归档文件大小与签名清单不符".into());
        }
        total = total.checked_add(f.size()).ok_or("更新包过大")?;
    }
    if seen.len() != expected.len() + 1 || total > MAX_EXTRACTED {
        return Err("更新归档缺少文件或超出限制".into());
    }
    ensure_space(&root, total.saturating_mul(2))?;
    let rel = format!("updates/staging/{}", unique());
    let stage = safe_path(&root, &rel)?;
    fs::create_dir_all(&stage).map_err(err)?;
    let result = (|| {
        save_json(&stage.join("update.json"), &e)?;
        let mut done = 0u64;
        for (name, record) in &expected {
            let mut entry = zip.by_name(name).map_err(err)?;
            let dst = safe_path(&stage, &name)?;
            fs::create_dir_all(dst.parent().unwrap()).map_err(err)?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&dst)
                .map_err(err)?;
            let n = std::io::copy(&mut entry.by_ref().take(record.size + 1), &mut output)
                .map_err(err)?;
            output.sync_all().map_err(err)?;
            drop(output);
            if n != record.size || hash_file(&dst)? != record.sha256 {
                return Err(format!("更新文件校验失败：{}", record.path));
            }
            done += n;
            progress(done, total);
        }
        preflight(&root, &stage, trust, false)?;
        Ok(stage.clone())
    })();
    // Failed staging is retained for diagnosis; it never changes installed files.
    result
}
pub fn stage_package(stage: &Path, trust: &Trust) -> Result<(Envelope, Package)> {
    let e: Envelope = read_json(&stage.join("update.json"))?;
    let p: Package = verify(&e, trust)?;
    validate_package(&p)?;
    for f in &p.files {
        let path = safe_path(stage, &format!("payload/{}", f.path))?;
        if fs::metadata(&path).map_err(err)?.len() != f.size || hash_file(&path)? != f.sha256 {
            return Err(format!("暂存文件校验失败：{}", f.path));
        }
    }
    let v: Version = read_json(&safe_path(stage, "payload/version.json")?)?;
    let m: serde_json::Value = read_json(&safe_path(stage, "payload/manifest.json")?)?;
    if v != p.version || m["build_id"].as_str() != Some(p.version.build_id.as_str()) {
        return Err("更新包版本与发行清单不一致".into());
    }
    for f in &p.files {
        if f.path != "manifest.json"
            && (m["files"][&f.path]["sha256"].as_str() != Some(f.sha256.as_str())
                || m["files"][&f.path]["size"].as_u64() != Some(f.size))
        {
            return Err(format!("发行文件清单与签名不一致：{}", f.path));
        }
    }
    if p.remove.iter().any(|f| !m["files"][&f.path].is_null()) {
        return Err("发行清单仍引用待删除文件".into());
    }
    Ok((e, p))
}
fn old_managed(root: &Path, trust: &Trust) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    let m: serde_json::Value = read_json(&root.join("manifest.json"))?;
    if let Some(files) = m["files"].as_object() {
        for (path, v) in files {
            if managed(path) {
                if let Some(h) = v["sha256"].as_str() {
                    if hex_hash(h) {
                        result.insert(path.to_ascii_lowercase(), h.to_string());
                    }
                }
            }
        }
    }
    let saved = root.join("updates/installed.json");
    if saved.exists() {
        let envelope: Envelope = read_json(&saved)?;
        let p: Package = verify(&envelope, trust)?;
        for f in p.files {
            result.insert(f.path.to_ascii_lowercase(), f.sha256);
        }
    }
    Ok(result)
}
fn metadata_file(path: &str) -> bool {
    path == "manifest.json" || path == "version.json"
}
fn unlocked(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if !path.is_file() {
        return Err(format!("目标不是普通文件：{}", path.display()));
    }
    let mut opts = OpenOptions::new();
    opts.read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        opts.share_mode(0);
    }
    let f = opts.open(path).map_err(|_| {
        format!(
            "文件正在使用或不可写，请关闭游戏、其他控制台、手柄工具和延迟面板：{}",
            path.display()
        )
    })?;
    drop(f);
    Ok(())
}
pub fn preflight(root: &Path, stage: &Path, trust: &Trust, check_locks: bool) -> Result<Package> {
    let (_, p) = stage_package(stage, trust)?;
    let current = installed(root, trust)?;
    if p.version.sequence <= current.sequence {
        return Err("此更新不比当前版本新；回退请使用恢复上一版本".into());
    }
    if current.features != p.version.features || current.game_version != p.version.game_version {
        return Err("此更新的游戏版本或功能组合与当前包不同".into());
    }
    if p.kind == "patch" && !p.base_build_ids.contains(&current.build_id) {
        return Err("补丁的基础版本不匹配，请下载兼容的完整更新包".into());
    }
    let known = old_managed(root, trust)?;
    let mut size = 0u64;
    let changed: HashSet<String> = p
        .files
        .iter()
        .map(|f| f.path.to_ascii_lowercase())
        .chain(p.remove.iter().map(|f| f.path.to_ascii_lowercase()))
        .collect();
    for (name, hash) in &known {
        if !changed.contains(name) && !metadata_file(name) {
            let path = safe_path(root, name)?;
            if hash_file(&path)? != *hash {
                return Err(format!(
                    "当前发行文件已被修改或缺失：{name}，请先恢复匹配的基础版本"
                ));
            }
        }
    }

    for f in &p.files {
        let path = safe_path(root, &f.path)?;
        if check_locks {
            unlocked(&path)?;
        }
        let actual = if path.exists() {
            Some(hash_file(&path)?)
        } else {
            None
        };
        if metadata_file(&f.path) {
            continue;
        }
        let expected = if p.kind == "patch" {
            f.before
                .get(&current.build_id)
                .ok_or_else(|| format!("补丁缺少基础文件信息：{}", f.path))?
                .clone()
        } else {
            known.get(&f.path.to_ascii_lowercase()).cloned()
        };
        if actual != expected {
            return Err(format!(
                "{} 与基础版本不同或包含个人文件，已停止替换；请使用匹配的完整包",
                f.path
            ));
        }
        size = size
            .saturating_add(f.size)
            .saturating_add(fs::metadata(path).map(|m| m.len()).unwrap_or(0));
    }
    for f in &p.remove {
        let path = safe_path(root, &f.path)?;
        if check_locks {
            unlocked(&path)?;
        }
        let expected = f
            .before
            .get(&current.build_id)
            .ok_or("删除清单缺少当前基础版本")?;
        if hash_file(&path)? != *expected {
            return Err(format!("待删除文件已被修改：{}", f.path));
        }
    }
    ensure_space(root, size)?;
    Ok(p)
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Operation {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Journal {
    pub id: String,
    pub phase: String,
    pub build_id: String,
    pub stage: String,
    pub operations: Vec<Operation>,
    pub previous_installed: Option<Envelope>,
}
fn journal_path(root: &Path) -> Result<PathBuf> {
    safe_path(root, "updates/transaction.json")
}
pub fn journal(root: &Path) -> Result<Option<Journal>> {
    let p = journal_path(root)?;
    if p.exists() {
        read_json(&p).map(Some)
    } else {
        Ok(None)
    }
}
pub fn pending(root: &Path) -> bool {
    matches!(journal(root),Ok(Some(j)) if !["committed","rolled_back"].contains(&j.phase.as_str()))
}
fn backup_root(root: &Path, id: &str) -> Result<PathBuf> {
    if id.contains('/') || id.contains('\\') || relative(id).is_err() {
        return Err("更新备份标识无效".into());
    }
    safe_path(root, &format!("updates/backup/{id}"))
}
fn valid_journal(j: &Journal) -> Result<()> {
    if j.operations.is_empty() || j.operations.len() > MAX_FILES + 1 {
        return Err("更新恢复记录无效".into());
    }
    let mut paths = HashSet::new();
    for op in &j.operations {
        if !managed(&op.path)
            || !paths.insert(op.path.to_ascii_lowercase())
            || op.before.as_ref().is_some_and(|h| !hex_hash(h))
            || op.after.as_ref().is_some_and(|h| !hex_hash(h))
        {
            return Err("更新恢复路径或哈希无效".into());
        }
    }
    Ok(())
}
pub fn install(root: &Path, stage: &Path, trust: &Trust) -> Result<()> {
    install_with_hook(root, stage, trust, |_| Ok(()))
}
/// Hook is used by fault-injection tests after each complete replacement.
pub fn install_with_hook(
    root: &Path,
    stage: &Path,
    trust: &Trust,
    mut hook: impl FnMut(usize) -> Result<()>,
) -> Result<()> {
    let root = root_path(root)?;
    let stage = stage.canonicalize().map_err(err)?;
    let allowed = safe_path(&root, "updates/staging")?
        .canonicalize()
        .map_err(err)?;
    if !stage.starts_with(&allowed) || stage == allowed {
        return Err("更新暂存目录无效".into());
    }
    if let Some(j) = journal(&root)? {
        if !["committed", "rolled_back"].contains(&j.phase.as_str()) {
            return Err("上次更新尚未完成，请先恢复".into());
        }
    }
    let p = preflight(&root, &stage, trust, true)?;
    let id = unique();
    let backup = backup_root(&root, &id)?;
    fs::create_dir_all(&backup).map_err(err)?;
    let previous_installed = if root.join("updates/installed.json").exists() {
        Some(read_json(&root.join("updates/installed.json"))?)
    } else {
        None
    };
    let mut ops = Vec::new();
    for f in &p.files {
        let path = safe_path(&root, &f.path)?;
        ops.push(Operation {
            path: f.path.clone(),
            before: if path.exists() {
                Some(hash_file(&path)?)
            } else {
                None
            },
            after: Some(f.sha256.clone()),
        });
    }
    for f in &p.remove {
        ops.push(Operation {
            path: f.path.clone(),
            before: Some(hash_file(&safe_path(&root, &f.path)?)?),
            after: None,
        });
    }
    for op in &ops {
        if let Some(hash) = &op.before {
            let dst = safe_path(&backup, &op.path)?;
            copy_sync(&safe_path(&root, &op.path)?, &dst)?;
            if hash_file(&dst)? != *hash {
                return Err("备份校验失败，未开始替换".into());
            }
        }
    }
    if let Some(old) = journal(&root)? {
        let history = safe_path(&root, &format!("updates/history/{}.json", old.id))?;
        save_json(&history, &old)?;
    }
    let mut j = Journal {
        id,
        phase: "applying".into(),
        build_id: p.version.build_id.clone(),
        stage: stage
            .strip_prefix(&root)
            .map_err(err)?
            .to_string_lossy()
            .replace('\\', "/"),
        operations: ops,
        previous_installed,
    };
    save_json(&journal_path(&root)?, &j)?;
    let result: Result<()> = (|| {
        for (i, op) in j.operations.iter().enumerate() {
            let dst = safe_path(&root, &op.path)?;
            if op.after.is_some() {
                replace_copy(&safe_path(&stage, &format!("payload/{}", op.path))?, &dst)?;
            } else {
                fs::remove_file(&dst).map_err(err)?;
            }
            hook(i)?;
        }
        for op in &j.operations {
            let dst = safe_path(&root, &op.path)?;
            if let Some(hash) = &op.after {
                if hash_file(&dst)? != *hash {
                    return Err("安装后文件校验失败".into());
                }
            } else if dst.exists() {
                return Err("文件删除失败".into());
            }
        }
        let envelope: Envelope = read_json(&stage.join("update.json"))?;
        save_json(&safe_path(&root, "updates/installed.json")?, &envelope)?;
        j.phase = "committed".into();
        save_json(&journal_path(&root)?, &j)?;
        Ok(())
    })();
    if let Err(error) = result {
        match rollback_internal(&root, &mut j, false) {
            Ok(()) => Err(format!("更新未完成，已恢复原版本：{error}")),
            Err(recovery) => Err(format!(
                "更新中断：{error}。恢复也未完成，请重新运行更新器恢复：{recovery}"
            )),
        }
    } else {
        Ok(())
    }
}
fn rollback_internal(root: &Path, j: &mut Journal, check_current: bool) -> Result<()> {
    valid_journal(j)?;
    let backup = backup_root(root, &j.id)?;
    for op in &j.operations {
        if let Some(hash) = &op.before {
            let p = safe_path(&backup, &op.path)?;
            if hash_file(&p)? != *hash {
                return Err(format!("备份损坏，未执行恢复：{}", op.path));
            }
        }
        let dst = safe_path(root, &op.path)?;
        unlocked(&dst)?;
        if check_current {
            let actual = if dst.exists() {
                Some(hash_file(&dst)?)
            } else {
                None
            };
            if actual != op.after {
                return Err(format!(
                    "更新后的文件已被修改，停止恢复以免覆盖：{}",
                    op.path
                ));
            }
        }
    }
    j.phase = "rolling_back".into();
    save_json(&journal_path(root)?, j)?;
    for op in j.operations.iter().rev() {
        let dst = safe_path(root, &op.path)?;
        if op.before.is_some() {
            replace_copy(&safe_path(&backup, &op.path)?, &dst)?;
        } else if dst.exists() {
            fs::remove_file(dst).map_err(err)?;
        }
    }
    let saved = safe_path(root, "updates/installed.json")?;
    if let Some(e) = &j.previous_installed {
        save_json(&saved, e)?;
    } else if saved.exists() {
        fs::remove_file(saved).map_err(err)?;
    }
    j.phase = "rolled_back".into();
    save_json(&journal_path(root)?, j)?;
    Ok(())
}
pub fn rollback(root: &Path) -> Result<()> {
    let mut j = journal(root)?.ok_or("没有可恢复的更新")?;
    if j.phase != "committed" {
        return Err("当前没有已完成、可撤销的更新".into());
    }
    rollback_internal(root, &mut j, true)
}
pub fn recover(root: &Path) -> Result<()> {
    if let Some(mut j) = journal(root)? {
        if !["committed", "rolled_back"].contains(&j.phase.as_str()) {
            rollback_internal(root, &mut j, false)?;
        }
    }
    Ok(())
}

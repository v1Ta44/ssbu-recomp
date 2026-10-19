//! HTTPS sources are data, never executable update instructions. All mirrors use the same trust root.
use crate::*;
use reqwest::blocking::{Client, Response};
use std::time::Duration;
pub fn checked_url(value: &str) -> Result<reqwest::Url> {
    let u = reqwest::Url::parse(value).map_err(|_| "更新来源网址无效")?;
    if u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
    {
        return Err("更新来源必须是没有账号密码的 HTTPS 地址".into());
    }
    Ok(u)
}
fn client(timeout: u64) -> Result<Client> {
    Client::builder()
        .user_agent("SSBU-Console-Updater/1")
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(timeout))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 6 {
                attempt.error("重定向次数过多")
            } else if attempt.url().scheme() != "https" {
                attempt.error("禁止降级为非 HTTPS 下载")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(err)
}
fn response(c: &Client, url: &str) -> Result<Response> {
    c.get(checked_url(url)?)
        .send()
        .map_err(|e| format!("网络请求失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("来源暂时不可用：{e}"))
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CheckedRelease {
    pub envelope: Envelope,
    pub release: Release,
    pub source: String,
    pub failures: Vec<String>,
}
pub fn check(urls: &[String], trust: &Trust, observe: impl FnMut(&str)) -> Result<CheckedRelease> {
    if urls.is_empty() {
        return Err("尚未配置在线更新来源，可以从文件更新".into());
    }
    if urls.len() > 4 {
        return Err("最多设置四个清单入口".into());
    }
    let c = client(4)?;
    check_with_fetch(urls, trust, observe, |url| {
        let r = response(&c, url)?;
        if r.content_length().is_some_and(|n| n > MAX_METADATA) {
            return Err("版本清单过大".into());
        }
        let mut response = r;
        let start = std::time::Instant::now();
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            if start.elapsed() > Duration::from_secs(4) {
                return Err("清单读取超时".into());
            }
            let n = response.read(&mut chunk).map_err(err)?;
            if n == 0 {
                break;
            }
            if bytes.len() + n > MAX_METADATA as usize {
                return Err("版本清单过大".into());
            }
            bytes.extend_from_slice(&chunk[..n]);
        }
        Ok(bytes)
    })
}
#[doc(hidden)]
pub fn check_with_fetch(
    urls: &[String],
    trust: &Trust,
    mut observe: impl FnMut(&str),
    mut fetch: impl FnMut(&str) -> Result<Vec<u8>>,
) -> Result<CheckedRelease> {
    if urls.is_empty() || urls.len() > 4 {
        return Err("需要一到四个清单入口".into());
    }
    let mut best: Option<CheckedRelease> = None;
    let mut failures = Vec::new();
    for url in urls {
        observe(url);
        let result: Result<(Envelope, Release)> = (|| {
            checked_url(url)?;
            let bytes = fetch(url)?;
            if bytes.len() as u64 > MAX_METADATA {
                return Err("版本清单过大".into());
            }
            let e: Envelope = serde_json::from_slice(&bytes).map_err(err)?;
            let release: Release = verify(&e, trust)?;
            release.version.validate()?;
            if release.artifacts.is_empty() || release.artifacts.len() > 8 {
                return Err("更新清单缺少可用附件".into());
            }
            for a in &release.artifacts {
                if !["patch", "full"].contains(&a.kind.as_str())
                    || !hex_hash(&a.sha256)
                    || a.size > MAX_ARCHIVE
                    || a.size == 0
                    || a.sources.is_empty()
                {
                    return Err("更新附件信息无效".into());
                }
                for s in &a.sources {
                    checked_url(&s.url)?;
                    if !["direct", "browser"].contains(&s.kind.as_str()) {
                        return Err("更新来源类型无效".into());
                    }
                }
            }
            Ok((e, release))
        })();
        match result {
            Ok((envelope, release)) => {
                if best.as_ref().map_or(true, |old| {
                    release.version.sequence > old.release.version.sequence
                }) {
                    best = Some(CheckedRelease {
                        envelope,
                        release,
                        source: url.clone(),
                        failures: Vec::new(),
                    });
                }
            }
            Err(e) => failures.push(format!("{url}: {e}")),
        }
    }
    if let Some(mut b) = best {
        b.failures = failures;
        Ok(b)
    } else {
        Err(format!(
            "暂时无法检查更新，已有版本仍可正常使用。{}",
            failures.join("；")
        ))
    }
}
pub fn artifact<'a>(release: &'a Release, current: &Installed) -> Option<&'a Artifact> {
    release
        .artifacts
        .iter()
        .find(|a| a.kind == "patch" && a.base_build_ids.contains(&current.build_id))
        .or_else(|| release.artifacts.iter().find(|a| a.kind == "full"))
}
pub fn download(
    a: &Artifact,
    preferred: Option<&str>,
    dir: &Path,
    progress: impl Fn(u64, u64) -> bool,
) -> Result<PathBuf> {
    let c = client(20)?;
    download_with_fetch(a, preferred, dir, progress, |url| {
        let r = response(&c, url)?;
        if r.content_length().is_some_and(|n| n != a.size) {
            return Err("服务器返回的附件大小不符".into());
        }
        Ok(Box::new(r))
    })
}
#[doc(hidden)]
pub fn download_with_fetch(
    a: &Artifact,
    preferred: Option<&str>,
    dir: &Path,
    progress: impl Fn(u64, u64) -> bool,
    mut fetch: impl FnMut(&str) -> Result<Box<dyn Read>>,
) -> Result<PathBuf> {
    if a.size == 0 || a.size > MAX_ARCHIVE || !hex_hash(&a.sha256) {
        return Err("下载清单无效".into());
    }
    fs::create_dir_all(dir).map_err(err)?;
    ensure_space(dir, a.size)?;
    let mut sources: Vec<&Source> = a.sources.iter().filter(|s| s.kind == "direct").collect();
    if sources.is_empty() {
        return Err("此来源需要在浏览器下载后从文件更新".into());
    }
    sources.sort_by_key(|s| {
        if Some(s.url.as_str()) == preferred {
            0
        } else {
            1
        }
    });
    let mut failures = Vec::new();
    for source in sources {
        let path = dir.join(format!("{}.download", unique()));
        let result: Result<PathBuf> = (|| {
            checked_url(&source.url)?;
            let mut r = fetch(&source.url)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(err)?;
            let mut buf = [0u8; 65536];
            let mut done = 0u64;
            let mut sha = Sha256::new();
            let started = std::time::Instant::now();
            loop {
                if started.elapsed() > Duration::from_secs(1800) {
                    return Err("下载耗时过长，请使用浏览器下载后导入".into());
                }
                if !progress(done, a.size) {
                    return Err("下载已取消".into());
                }
                let n = r.read(&mut buf).map_err(err)?;
                if n == 0 {
                    break;
                }
                done = done.checked_add(n as u64).ok_or("下载大小异常")?;
                if done > a.size {
                    return Err("下载内容超过签名清单中的大小".into());
                }
                file.write_all(&buf[..n]).map_err(err)?;
                sha.update(&buf[..n]);
            }
            file.sync_all().map_err(err)?;
            if done != a.size || format!("{:x}", sha.finalize()) != a.sha256 {
                return Err("下载文件校验失败，可能来源尚未同步".into());
            }
            progress(done, a.size);
            Ok(path.clone())
        })();
        match result {
            Ok(path) => return Ok(path),
            Err(e) => {
                let _ = fs::remove_file(&path);
                if e == "下载已取消" {
                    return Err(e);
                }
                failures.push(format!("{}：{}", source.name, e));
            }
        }
    }
    Err(format!(
        "下载未完成，可选择其他来源或浏览器下载后导入。{}",
        failures.join("；")
    ))
}

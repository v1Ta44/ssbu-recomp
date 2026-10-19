//! Console orchestration. Network checks and file preparation never run on the UI thread.
use crate::{game, pads::Pads, paths::Layout};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ssbu_updater::{self as core, network, Envelope, Release, Trust};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
pub const HELPER: &[u8] = include_bytes!(env!("SSBU_UPDATER_EXE"));
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub startup_check: bool,
    pub manifest_urls: Vec<String>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            startup_check: false,
            manifest_urls: vec![
                "https://github.com/v1Ta44/ssbu-recomp/releases/download/yuzu-test/update.json"
                    .into(),
            ],
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct CheckState {
    last_attempt: u64,
    last_success: u64,
    last_source: String,
    highest_seen: u64,
    failures: u32,
    next_check: u64,
}
struct Inner {
    busy: bool,
    phase: String,
    message: String,
    done: u64,
    total: u64,
    latest: Option<network::CheckedRelease>,
    ready: Option<PathBuf>,
}
pub struct Updates {
    inner: Mutex<Inner>,
    cancel: AtomicBool,
}
impl Updates {
    pub fn new(l: Option<&Layout>) -> Self {
        let latest = l
            .and_then(|l| {
                core::read_json::<Envelope>(&l.root.join("updates/last-release.json")).ok()
            })
            .and_then(|e| {
                core::verify::<Release>(&e, &Trust::bundled())
                    .ok()
                    .map(|release| network::CheckedRelease {
                        envelope: e,
                        release,
                        source: String::new(),
                        failures: Vec::new(),
                    })
            });
        Self {
            inner: Mutex::new(Inner {
                busy: false,
                phase: "idle".into(),
                message: "可手动检查更新，或导入下载好的更新文件。".into(),
                done: 0,
                total: 0,
                latest,
                ready: None,
            }),
            cancel: AtomicBool::new(false),
        }
    }
    pub fn guard(&self) -> Result<(), String> {
        let i = self.inner.lock().unwrap();
        if i.phase == "installing" {
            Err("正在退出并更新，请稍候".into())
        } else {
            Ok(())
        }
    }
    fn begin(&self, phase: &str, message: &str) -> Result<(), String> {
        let mut i = self.inner.lock().unwrap();
        if i.busy {
            return Err("另一个更新操作尚未完成".into());
        }
        i.busy = true;
        i.phase = phase.into();
        i.message = message.into();
        i.done = 0;
        i.total = 0;
        if ["verifying", "downloading"].contains(&phase) {
            i.ready = None;
        }
        self.cancel.store(false, Ordering::Relaxed);
        Ok(())
    }
    fn finish(&self, result: Result<String, String>) {
        let mut i = self.inner.lock().unwrap();
        i.busy = false;
        match result {
            Ok(message) => {
                i.message = message;
                if i.phase != "ready" {
                    i.phase = "idle".into();
                }
            }
            Err(e) => {
                i.phase = "error".into();
                i.message = e;
            }
        }
    }
    fn progress(&self, done: u64, total: u64) -> bool {
        let mut i = self.inner.lock().unwrap();
        i.done = done;
        i.total = total;
        !self.cancel.load(Ordering::Relaxed)
    }
    pub fn config(l: &Layout) -> Config {
        core::read_json(&l.config_dir().join("updates.json")).unwrap_or_default()
    }
    pub fn save_config(l: &Layout, config: Config) -> Result<Value, String> {
        if config.manifest_urls.len() > 4 {
            return Err("最多设置四个在线清单来源".into());
        }
        for u in &config.manifest_urls {
            network::checked_url(u)?;
        }
        core::save_json(&core::safe_path(&l.root, "config/updates.json")?, &config)?;
        Ok(json!(config))
    }
    pub fn status(&self, l: &Layout) -> Result<Value, String> {
        let i = self.inner.lock().unwrap();
        let current = core::installed(&l.root, &Trust::bundled())?;
        let check: CheckState =
            core::read_json(&l.root.join("updates/check-state.json")).unwrap_or_default();
        let latest=i.latest.as_ref().map(|r|{let compatible=r.release.version.features==current.features&&r.release.version.game_version==current.game_version;let artifact=network::artifact(&r.release,&current);json!({"version":r.release.version,"notes":r.release.notes,"artifact":artifact,"compatible":compatible,"newer":r.release.version.sequence>current.sequence,"sources_delayed":r.release.version.sequence<check.highest_seen})});
        let ready=i.ready.as_ref().and_then(|s|core::read_json::<Envelope>(&s.join("update.json")).ok()).and_then(|e|core::verify::<core::Package>(&e,&Trust::bundled()).ok()).map(|p|json!({"version":p.version,"notes":p.notes,"files":p.files.len(),"bytes":p.files.iter().map(|f|f.size).sum::<u64>()}));
        let rollback = core::journal(&l.root)
            .ok()
            .flatten()
            .is_some_and(|j| j.phase == "committed");
        let result = fs::read_to_string(l.root.join("updates/last-result.txt")).unwrap_or_default();
        Ok(
            json!({"supported":l.package,"current":current,"phase":i.phase,"busy":i.busy,"message":i.message,"done":i.done,"total":i.total,"latest":latest,"ready":ready,"check":check,"config":Self::config(l),"rollback":rollback,"last_result":result.chars().take(4000).collect::<String>()}),
        )
    }
    pub fn check(self: &Arc<Self>, l: Layout, automatic: bool) -> Result<(), String> {
        if !l.package {
            return Err("开发目录不直接安装发行更新，请在测试包中使用".into());
        }
        let config = Self::config(&l);
        if automatic && !config.startup_check {
            return Ok(());
        }
        let mut state: CheckState =
            core::read_json(&l.root.join("updates/check-state.json")).unwrap_or_default();
        if core::now() < state.next_check {
            return Err("刚刚检查过，请稍后再试；也可以从文件更新。".into());
        }
        self.begin("checking", "正在检查更新来源…")?;
        let this = self.clone();
        std::thread::spawn(move || {
            let result = (|| {
                state.last_attempt = core::now();
                state.next_check = core::now() + 60;
                core::save_json(
                    &core::safe_path(&l.root, "updates/check-state.json")?,
                    &state,
                )?;
                match network::check(&config.manifest_urls, &Trust::bundled(), |_| {}) {
                    Ok(release) => {
                        if release.release.version.sequence < state.highest_seen {
                            return Err(
                                "来源仍是较旧版本，可能尚未同步；不会降级或显示为最新。".into()
                            );
                        }
                        let current = core::installed(&l.root, &Trust::bundled())?;
                        state.highest_seen =
                            state.highest_seen.max(release.release.version.sequence);
                        state.last_success = core::now();
                        state.last_source = release.source.clone();
                        state.failures = 0;
                        state.next_check = core::now() + if automatic { 3600 } else { 60 };
                        core::save_json(
                            &core::safe_path(&l.root, "updates/last-release.json")?,
                            &release.envelope,
                        )?;
                        let message = if release.release.version.sequence > current.sequence {
                            if network::artifact(&release.release, &current).is_some() {
                                "发现新版本，可选择来源下载。"
                            } else {
                                "发现新版本，但没有适用当前基础版本的附件，请等待兼容的完整更新包。"
                            }
                        } else if release.release.version.sequence < current.sequence {
                            "来源暂未同步到当前版本，保留现有版本。"
                        } else {
                            "当前已是此通道的最新版本。"
                        }
                        .to_string();
                        this.inner.lock().unwrap().latest = Some(release);
                        Ok(message)
                    }
                    Err(e) => {
                        state.failures = state.failures.saturating_add(1);
                        state.next_check = core::now()
                            + (60u64.saturating_mul(1u64 << state.failures.min(6))).min(3600);
                        Err(e)
                    }
                }
            })();
            let saved = core::safe_path(&l.root, "updates/check-state.json")
                .and_then(|p| core::save_json(&p, &state));
            this.finish(result.and_then(|m| saved.map(|_| m)));
        });
        Ok(())
    }
    fn stage(
        &self,
        l: &Layout,
        archive: &Path,
        expected: Option<&core::Version>,
    ) -> Result<String, String> {
        {
            let mut i = self.inner.lock().unwrap();
            i.phase = "verifying".into();
            i.message = "正在校验并准备更新文件…".into();
        }
        let stage = core::stage_archive(&l.root, archive, &Trust::bundled(), |d, t| {
            self.progress(d, t);
        })?;
        let (_, p) = core::stage_package(&stage, &Trust::bundled())?;
        if let Some(v) = expected {
            if p.version.build_id != v.build_id
                || p.version.sequence != v.sequence
                || p.version.channel != v.channel
                || p.version.features != v.features
            {
                return Err("下载包与所选版本清单不一致".into());
            }
        }
        let mut i = self.inner.lock().unwrap();
        i.ready = Some(stage);
        i.phase = "ready".into();
        Ok("更新文件已校验。关闭游戏和手柄工具后，点击退出并安装。".into())
    }
    pub fn import(self: &Arc<Self>, l: Layout, path: PathBuf) -> Result<(), String> {
        if !l.package {
            return Err("请在测试包中使用更新功能".into());
        }
        self.begin("verifying", "正在校验更新文件…")?;
        let this = self.clone();
        std::thread::spawn(move || {
            let r = this.stage(&l, &path, None);
            this.finish(r);
        });
        Ok(())
    }
    pub fn download(self: &Arc<Self>, l: Layout, source: Option<String>) -> Result<(), String> {
        let current = core::installed(&l.root, &Trust::bundled())?;
        let release = self
            .inner
            .lock()
            .unwrap()
            .latest
            .clone()
            .ok_or("请先检查更新")?;
        if release.release.version.features != current.features
            || release.release.version.sequence <= current.sequence
        {
            return Err("没有与当前包兼容的新版本".into());
        }
        let artifact = network::artifact(&release.release, &current)
            .ok_or("请下载兼容的完整更新包")?
            .clone();
        self.begin("downloading", "正在下载并校验更新…")?;
        let this = self.clone();
        std::thread::spawn(move || {
            let r = (|| {
                let download = core::safe_path(&l.root, "updates/downloads")?;
                let archive =
                    network::download(&artifact, source.as_deref(), &download, |d, t| {
                        this.progress(d, t)
                    })?;
                this.stage(&l, &archive, Some(&release.release.version))
            })();
            this.finish(r);
        });
        Ok(())
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn browser(&self, index: usize, l: &Layout) -> Result<(), String> {
        let current = core::installed(&l.root, &Trust::bundled())?;
        let i = self.inner.lock().unwrap();
        let r = i.latest.as_ref().ok_or("请先检查更新")?;
        let a = network::artifact(&r.release, &current).ok_or("没有适用的下载文件")?;
        let s = a.sources.get(index).ok_or("来源不存在")?;
        if s.kind != "browser" {
            return Err("该来源不是浏览器下载入口".into());
        }
        network::checked_url(&s.url)?;
        // A fixed executable and separately passed argument avoid shell interpolation of publisher URLs.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            Command::new("rundll32.exe")
                .args(["url.dll,FileProtocolHandler", &s.url])
                .creation_flags(game::CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn apply(
        &self,
        l: &Layout,
        pads: &Pads,
        games: &game::Games,
        rollback: bool,
    ) -> Result<(), String> {
        if !l.package {
            return Err("开发目录不直接安装更新".into());
        }
        if game::status(games)["state"] == "running" || pads.running().is_some() {
            return Err("请先关闭游戏、手柄工具和延迟面板，再安装更新".into());
        }
        self.begin("installing", "正在退出控制台并安装更新…")?;
        let result = (|| {
            let stage = self.inner.lock().unwrap().ready.clone();
            if !rollback {
                let s = stage.as_ref().ok_or("请先下载或导入更新文件")?;
                core::preflight(&l.root, s, &Trust::bundled(), false)?;
            }
            spawn_helper(
                &l.root,
                if rollback { "rollback" } else { "install" },
                stage.as_deref(),
                std::process::id(),
                true,
            )
        })();
        if result.is_err() {
            self.finish(Err(result.clone().unwrap_err()));
        }
        result
    }
}
pub fn spawn_helper(
    root: &Path,
    action: &str,
    stage: Option<&Path>,
    pid: u32,
    restart: bool,
) -> Result<(), String> {
    let folder = core::safe_path(root, &format!("updates/helpers/{}", core::unique()))?;
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let exe = folder.join("SSBU Updater.exe");
    core::atomic_bytes(&exe, HELPER)?;
    if core::hash_file(&exe)? != core::hash_bytes(HELPER) {
        return Err("更新辅助程序校验失败".into());
    }
    let mut command = Command::new(&exe);
    command
        .args([action, "--root"])
        .arg(root)
        .args(["--wait-pid", &pid.to_string()]);
    if let Some(s) = stage {
        command.arg("--stage").arg(s);
    }
    if restart {
        command.arg("--restart");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(game::CREATE_NO_WINDOW);
    }
    command
        .current_dir(root)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn pick() -> Result<Value, String> {
    use std::os::windows::process::CommandExt;
    let script="Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.OpenFileDialog; $d.Filter='Signed update (*.zip;*.ssbupatch)|*.zip;*.ssbupatch'; $d.Title='选择下载好的更新文件'; if($d.ShowDialog() -eq 'OK'){[Console]::OutputEncoding=[Text.UTF8Encoding]::new(); [Console]::Write($d.FileName)}";
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-STA", "-Command", script])
        .creation_flags(game::CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("无法打开文件选择窗口".into());
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(if path.is_empty() {
        Value::Null
    } else {
        json!(path)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_install_while_game_is_running() {
        let root =
            std::env::temp_dir().join(format!("ssbu-console-update-test-{}", core::unique()));
        let layout = Layout {
            root: root.clone(),
            game_dir: root.join("game"),
            exe: root.join("game/ssbu_native.exe"),
            package: true,
        };
        let games = game::Games::new();
        *games.run.lock().unwrap() = Some(game::Run {
            session: root.clone(),
            name: "test".into(),
            mode: "play".into(),
            pid: 0,
            started: chrono::Local::now(),
            t0: std::time::Instant::now(),
            info: serde_json::Map::new(),
            child: None,
            finished: None,
        });
        let updates = Updates::new(None);
        assert!(updates
            .apply(&layout, &Pads::new(), &games, false)
            .unwrap_err()
            .contains("关闭游戏"));
        assert!(!updates.inner.lock().unwrap().busy);
    }
    #[test]
    fn rejects_plain_http_update_sources() {
        let root =
            std::env::temp_dir().join(format!("ssbu-console-update-test-{}", core::unique()));
        let l = Layout {
            root: root.clone(),
            game_dir: root.join("game"),
            exe: root.join("game/ssbu_native.exe"),
            package: true,
        };
        assert!(Updates::save_config(
            &l,
            Config {
                startup_check: false,
                manifest_urls: vec!["http://example.test/feed".into()]
            }
        )
        .is_err());
        assert!(!root.exists());
    }
}

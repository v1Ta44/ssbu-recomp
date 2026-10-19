//! Where things are. A package (scripts/package.py) has game\ssbu_native.exe beside config\, logs\, reports\; the
//! development tree has build\work\ssbu_native.exe (with build\state.json). The exe runs in its own folder and finds
//! ..\binaries, ..\cache, ..\config by relative paths in both.
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Layout {
    pub root: PathBuf,
    pub game_dir: PathBuf,
    pub exe: PathBuf,
    pub package: bool,
}

impl Layout {
    pub fn find() -> Option<Layout> {
        let mut cands: Vec<PathBuf> = Vec::new();
        if let Ok(r) = std::env::var("SSBU_CONSOLE_ROOT") {
            cands.push(PathBuf::from(r));
        }
        if let Ok(exe) = std::env::current_exe() {
            for a in exe.ancestors().skip(1).take(7) {
                cands.push(a.to_path_buf());
                cands.push(a.join("build"));
            }
        }
        for c in cands {
            let pkg = c.join("game").join("ssbu_native.exe");
            if pkg.is_file() {
                return Some(Layout { game_dir: c.join("game"), exe: pkg, root: c, package: true });
            }
            let dev = c.join("work").join("ssbu_native.exe");
            if dev.is_file() && c.join("state.json").is_file() {
                return Some(Layout { game_dir: c.join("work"), exe: dev, root: c, package: false });
            }
        }
        None
    }
    pub fn config_dir(&self) -> PathBuf {
        self.root.join("config")
    }
    pub fn graphics_ini(&self) -> PathBuf {
        self.config_dir().join("graphics.ini")
    }
    pub fn controllers_ini(&self) -> PathBuf {
        self.config_dir().join("controllers.ini")
    }
    pub fn launcher_ini(&self) -> PathBuf {
        self.root.join("launcher").join("launcher.ini")
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn reports(&self) -> PathBuf {
        self.root.join("reports")
    }
    /// A path given in launcher.ini: relative ones are relative to the package root.
    pub fn full(&self, p: &str) -> PathBuf {
        let pb = PathBuf::from(p);
        if pb.is_absolute() {
            pb
        } else {
            self.root.join(pb)
        }
    }
    /// The RomFS folder when launcher.ini does not name one.
    pub fn default_romfs(&self) -> PathBuf {
        if !self.package {
            if let Ok(text) = std::fs::read_to_string(self.root.join("state.json")) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(r) = v["dump"]["romfs"].as_str() {
                        return PathBuf::from(r);
                    }
                }
            }
        }
        self.root.join("romfs")
    }
    /// Is `p` inside the package (the only folders the console opens in Explorer)?
    pub fn contains(&self, p: &Path) -> bool {
        let norm = |x: &Path| x.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
        let (r, q) = (norm(&self.root), norm(p));
        q.starts_with(&r) && !q.contains("..")
    }
}

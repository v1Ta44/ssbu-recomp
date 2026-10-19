use serde_json::json;
use ssbu_updater::*;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("ssbu-update-test-{}", unique()));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        if self.0.starts_with(std::env::temp_dir())
            && self
                .0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("ssbu-update-test-")
        {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
struct Fixture {
    temp: Temp,
    root: PathBuf,
    key: Vec<u8>,
    trust: Trust,
}
fn bytes(p: &Path) -> Vec<u8> {
    fs::read(p).unwrap()
}
fn write(p: &Path, b: &[u8]) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b).unwrap();
}
fn features() -> Features {
    Features {
        resource_mods: true,
        latency_slider: true,
    }
}
fn version(sequence: u64) -> Version {
    Version {
        schema: 1,
        channel: "yuzu-test".into(),
        platform: "win-x64".into(),
        sequence,
        version: format!("test.{sequence}"),
        build_id: format!("build-{sequence}"),
        game_version: "13.0.4".into(),
        features: features(),
        minimum_updater: 1,
    }
}
impl Fixture {
    fn new() -> Self {
        let temp = Temp::new();
        let root = temp.0.join("包 with spaces");
        fs::create_dir_all(&root).unwrap();
        let (key, public) = generate_key().unwrap();
        let trust = Trust {
            keys: BTreeMap::from([("test".into(), public)]),
        };
        let files = [
            ("SSBU Console.exe", b"old-console".to_vec()),
            ("game/ssbu_native.exe", b"old-game".to_vec()),
            (
                "game/ssbu_native.build.json",
                serde_json::to_vec(&features()).unwrap(),
            ),
        ];
        let mut map = serde_json::Map::new();
        for (n, b) in files {
            write(&root.join(n), &b);
            map.insert(n.into(), json!({"size":b.len(),"sha256":hash_bytes(&b)}));
        }
        write(
            &root.join("manifest.json"),
            &serde_json::to_vec(
                &json!({"build_id":"old-build","name":"old","romfs":{"size":17},"files":map}),
            )
            .unwrap(),
        );
        for p in [
            "userdata/save/user.bin",
            "mods/custom.bin",
            "config/graphics.ini",
            "cache/vk/custom.bin",
            "logs/log.txt",
            "reports/report.txt",
            "romfs/data.arc",
            "launcher/launcher.ini",
            "game/net_delay/webview-data/Preferences",
            "game/personal.dll",
        ] {
            write(&root.join(p), b"user-owned");
        }
        Self {
            temp,
            root,
            key,
            trust,
        }
    }
    fn contents(&self, sequence: u64) -> (Package, BTreeMap<String, Vec<u8>>) {
        let v = version(sequence);
        let current = installed(&self.root, &self.trust).unwrap();
        let mut payload: BTreeMap<String, Vec<u8>> = BTreeMap::from([
            (
                "SSBU Console.exe".into(),
                format!("new-console-{sequence}").into_bytes(),
            ),
            ("game/new.dll".into(), b"new-managed-file".to_vec()),
            ("version.json".into(), serde_json::to_vec(&v).unwrap()),
        ]);
        let mut manifest: serde_json::Value = read_json(&self.root.join("manifest.json")).unwrap();
        manifest["build_id"] = json!(v.build_id);
        for (n, b) in &payload {
            manifest["files"][n] = json!({"size":b.len(),"sha256":hash_bytes(b)});
        }
        payload.insert(
            "manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        let changes = payload
            .iter()
            .map(|(path, b)| Change {
                path: path.clone(),
                size: b.len() as u64,
                sha256: hash_bytes(b),
                before: BTreeMap::from([(
                    current.build_id.clone(),
                    if self.root.join(path).exists() {
                        Some(hash_file(&self.root.join(path)).unwrap())
                    } else {
                        None
                    },
                )]),
            })
            .collect();
        (
            Package {
                version: v,
                kind: "patch".into(),
                base_build_ids: vec![current.build_id],
                notes: "fix".into(),
                files: changes,
                remove: vec![],
            },
            payload,
        )
    }
    fn archive(
        &self,
        p: &Package,
        mut payload: BTreeMap<String, Vec<u8>>,
        extra: Option<(&str, &[u8])>,
    ) -> PathBuf {
        let zip = self.temp.0.join(format!("{}.zip", unique()));
        let mut w = zip::ZipWriter::new(fs::File::create(&zip).unwrap());
        let opt = zip::write::SimpleFileOptions::default();
        let envelope = sign(&serde_json::to_vec(p).unwrap(), &self.key, "test").unwrap();
        w.start_file("update.json", opt).unwrap();
        w.write_all(&serde_json::to_vec(&envelope).unwrap())
            .unwrap();
        for (name, b) in &mut payload {
            w.start_file(format!("payload/{name}"), opt).unwrap();
            w.write_all(b).unwrap();
        }
        if let Some((name, b)) = extra {
            w.start_file(name, opt).unwrap();
            w.write_all(b).unwrap();
        }
        w.finish().unwrap();
        zip
    }
    fn prepare(&self, sequence: u64) -> PathBuf {
        let (p, b) = self.contents(sequence);
        stage_archive(
            &self.root,
            &self.archive(&p, b, None),
            &self.trust,
            |_, _| {},
        )
        .unwrap()
    }
}
#[test]
fn patch_install_preserves_user_data_and_rollback_restores_exact_bytes() {
    let f = Fixture::new();
    let old = bytes(&f.root.join("manifest.json"));
    let link = f.temp.0.join("hardlink.exe");
    fs::hard_link(f.root.join("SSBU Console.exe"), &link).unwrap();
    let s = f.prepare(1);
    install(&f.root, &s, &f.trust).unwrap();
    assert_eq!(installed(&f.root, &f.trust).unwrap().sequence, 1);
    assert_eq!(bytes(&link), b"old-console");
    for p in [
        "userdata/save/user.bin",
        "mods/custom.bin",
        "config/graphics.ini",
        "cache/vk/custom.bin",
        "logs/log.txt",
        "reports/report.txt",
        "romfs/data.arc",
        "launcher/launcher.ini",
        "game/net_delay/webview-data/Preferences",
        "game/personal.dll",
    ] {
        assert_eq!(bytes(&f.root.join(p)), b"user-owned");
    }
    assert!(preflight(&f.root, &s, &f.trust, false).is_err());
    rollback(&f.root).unwrap();
    assert_eq!(bytes(&f.root.join("manifest.json")), old);
    assert_eq!(bytes(&f.root.join("SSBU Console.exe")), b"old-console");
    assert!(!f.root.join("game/new.dll").exists());
    assert!(!f.root.join("version.json").exists());
    assert_eq!(installed(&f.root, &f.trust).unwrap().sequence, 0);
}
#[test]
fn partial_failure_automatically_rolls_back() {
    let f = Fixture::new();
    let s = f.prepare(1);
    assert!(install_with_hook(&f.root, &s, &f.trust, |i| if i == 1 {
        Err("disk error".into())
    } else {
        Ok(())
    })
    .unwrap_err()
    .contains("已恢复"));
    assert_eq!(bytes(&f.root.join("SSBU Console.exe")), b"old-console");
    assert!(!pending(&f.root));
}
#[test]
fn interrupted_replacement_is_recovered_next_run() {
    let f = Fixture::new();
    let s = f.prepare(1);
    let result = std::panic::catch_unwind(|| {
        let _ = install_with_hook(&f.root, &s, &f.trust, |i| {
            if i == 1 {
                panic!("simulated process interruption");
            }
            Ok(())
        });
    });
    assert!(result.is_err());
    assert!(pending(&f.root));
    recover(&f.root).unwrap();
    assert_eq!(bytes(&f.root.join("SSBU Console.exe")), b"old-console");
    assert!(!f.root.join("game/new.dll").exists());
    assert!(!pending(&f.root));
}
#[test]
fn bad_signature_and_unknown_publisher_are_rejected() {
    let f = Fixture::new();
    let (p, b) = f.contents(1);
    let archive = f.archive(&p, b, None);
    let other = Trust {
        keys: BTreeMap::new(),
    };
    assert!(stage_archive(&f.root, &archive, &other, |_, _| {}).is_err());
    let mut e = sign(b"{}", &f.key, "test").unwrap();
    e.payload = "e30g".into();
    assert!(verify::<Package>(&e, &f.trust).is_err());
}
#[test]
fn corrupt_payload_and_extra_entries_are_rejected() {
    let f = Fixture::new();
    let (p, mut b) = f.contents(1);
    b.insert("SSBU Console.exe".into(), b"bad-console-1".to_vec());
    let a = f.archive(&p, b, None);
    assert!(stage_archive(&f.root, &a, &f.trust, |_, _| {}).is_err());
    let (p, b) = f.contents(1);
    for extra in [
        "../escape.dll",
        "C:/escape.dll",
        "payload/../config/graphics.ini",
        "payload/game/extra.dll",
    ] {
        assert!(stage_archive(
            &f.root,
            &f.archive(&p, b.clone(), Some((extra, b"bad"))),
            &f.trust,
            |_, _| {}
        )
        .is_err());
    }
}
#[test]
fn immutable_base_mismatch_and_wrong_features_do_not_replace_files() {
    let f = Fixture::new();
    let s = f.prepare(1);
    write(&f.root.join("SSBU Console.exe"), b"locally-modified");
    assert!(install(&f.root, &s, &f.trust).is_err());
    assert_eq!(bytes(&f.root.join("SSBU Console.exe")), b"locally-modified");
    let (p, mut b) = f.contents(1);
    let mut wrong = p.clone();
    wrong.version.features.resource_mods = false;
    b.insert(
        "version.json".into(),
        serde_json::to_vec(&wrong.version).unwrap(),
    );
    assert!(stage_archive(&f.root, &f.archive(&wrong, b, None), &f.trust, |_, _| {}).is_err());
}
#[test]
fn unchanged_managed_file_tampering_is_rejected() {
    let f = Fixture::new();
    let s = f.prepare(1);
    write(&f.root.join("game/ssbu_native.exe"), b"modified");
    assert!(install(&f.root, &s, &f.trust).is_err());
    assert_eq!(bytes(&f.root.join("SSBU Console.exe")), b"old-console");
}
#[test]
fn full_package_does_not_overwrite_unknown_personal_files() {
    let f = Fixture::new();
    let (mut p, b) = f.contents(1);
    p.kind = "full".into();
    p.base_build_ids.clear();
    let s = stage_archive(&f.root, &f.archive(&p, b, None), &f.trust, |_, _| {}).unwrap();
    write(&f.root.join("game/new.dll"), b"personal-file");
    assert!(install(&f.root, &s, &f.trust).is_err());
    assert_eq!(bytes(&f.root.join("game/new.dll")), b"personal-file");
}
#[test]
fn corrupted_backup_and_post_install_edits_refuse_rollback() {
    let f = Fixture::new();
    let s = f.prepare(1);
    install(&f.root, &s, &f.trust).unwrap();
    write(&f.root.join("SSBU Console.exe"), b"post-install-edit");
    assert!(rollback(&f.root).is_err());
    assert_eq!(
        bytes(&f.root.join("SSBU Console.exe")),
        b"post-install-edit"
    );
    write(&f.root.join("SSBU Console.exe"), b"new-console-1");
    let j = journal(&f.root).unwrap().unwrap();
    write(
        &f.root
            .join(format!("updates/backup/{}/SSBU Console.exe", j.id)),
        b"damaged",
    );
    assert!(rollback(&f.root).is_err());
    assert_eq!(bytes(&f.root.join("SSBU Console.exe")), b"new-console-1");
}
#[test]
fn repeated_updates_and_rollback_restore_previous_signed_version() {
    let f = Fixture::new();
    let s = f.prepare(1);
    install(&f.root, &s, &f.trust).unwrap();
    let next = f.prepare(2);
    install(&f.root, &next, &f.trust).unwrap();
    assert_eq!(installed(&f.root, &f.trust).unwrap().sequence, 2);
    rollback(&f.root).unwrap();
    assert_eq!(installed(&f.root, &f.trust).unwrap().sequence, 1);
    assert_eq!(bytes(&f.root.join("SSBU Console.exe")), b"new-console-1");
}
#[test]
fn paths_and_personal_directories_are_protected() {
    for p in [
        "../evil",
        "/evil",
        "C:/evil",
        "a\\b",
        "a:stream",
        "a/../b",
        "game/con.dll",
        "game/NUL",
        "game/a.",
        "game/a ",
        "game/a//b",
        "game/com1.exe",
    ] {
        assert!(relative(p).is_err(), "{p}");
    }
    for p in [
        "romfs/data.arc",
        "RomFS/Data.Arc",
        "userdata/save/a",
        "config/graphics.ini",
        "mods/custom.dll",
        "cache/vk/a",
        "logs/a",
        "reports/a",
        "launcher/launcher.ini",
        "game/net_delay/webview-data/Default/Preferences",
    ] {
        assert!(!managed(p), "{p}");
    }
    assert!(managed("romfs/.nrr/cross2.nrr"));
}
#[test]
fn signed_protected_path_and_oversized_package_are_rejected() {
    let f = Fixture::new();
    let (mut p, b) = f.contents(1);
    p.files[0].path = "config/graphics.ini".into();
    assert!(stage_archive(&f.root, &f.archive(&p, b, None), &f.trust, |_, _| {}).is_err());
    let (mut p, b) = f.contents(1);
    p.files[0].size = MAX_EXTRACTED + 1;
    assert!(stage_archive(&f.root, &f.archive(&p, b, None), &f.trust, |_, _| {}).is_err());
}
#[cfg(windows)]
#[test]
fn console_and_locked_executable_block_installation() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    let lock = RootLock::acquire(&f.root).unwrap();
    assert!(RootLock::acquire(&f.root).is_err());
    drop(lock);
    let s = f.prepare(1);
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(f.root.join("SSBU Console.exe"))
        .unwrap();
    assert!(install(&f.root, &s, &f.trust).is_err());
    drop(held);
    install(&f.root, &s, &f.trust).unwrap();
}

#[test]
fn concurrent_staging_identifiers_are_unique() {
    let workers: Vec<_> = (0..8)
        .map(|_| std::thread::spawn(|| (0..1000).map(|_| unique()).collect::<Vec<_>>()))
        .collect();
    let names: std::collections::HashSet<_> = workers
        .into_iter()
        .flat_map(|w| w.join().unwrap())
        .collect();
    assert_eq!(names.len(), 8000);
}

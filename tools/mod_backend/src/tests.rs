use super::*;
use std::{
    fs,
    io::Cursor,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static SERIAL: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "ssbu-mod-test-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn file(&self, p: &str, b: &[u8]) -> PathBuf {
        let p = self.0.join(p);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b).unwrap();
        p
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn source(path: PathBuf, kind: &str) -> packages::Source {
    packages::Source {
        package: "test".into(),
        size: fs::metadata(&path).unwrap().len(),
        disk: path,
        kind: kind.into(),
    }
}
#[test]
fn paths_hashes_regions_and_structured_shares() {
    let (p, r) = paths::normalize(r"RomFS\Fighter\Mario\model+us_en.nutexb").unwrap();
    assert_eq!((p.as_str(), r), ("fighter/mario/model.nutexb", Some(2)));
    assert_eq!(
        paths::normalize("stream;/movie/a.mp4").unwrap().0,
        "stream:/movie/a.webm"
    );
    assert_eq!(paths::normalize("ui/a.lua").unwrap().0, "ui/a.lc");
    assert!(paths::normalize("fighter/../data.arc").is_err());
    assert!(paths::normalize("a//b").is_err());
    let name = paths::hash("model.nusktb");
    assert_eq!(
        paths::path_hash(&format!("fighter/mario/0x{name:010x}.nusktb")),
        paths::hash("fighter/mario/model.nusktb")
    );
    let spec=paths::file_spec(&json!({"file-name":"model.nusktb","extension":"nusktb","parent":{"name":"mario","parent":"fighter"}})).unwrap();
    assert_eq!(spec.hash, paths::hash("fighter/mario/model.nusktb"));
}
#[test]
fn prebuilt_archive_namespace_and_scan_errors() {
    let expected = "prebuilt:/movie/fighter/brave/c00/final_00.h264";
    for path in [
        "prebuilt;/movie/fighter/brave/c00/final_00.h264",
        "prebuilt:/movie/fighter/brave/c00/final_00.h264",
        r"RomFS\PREBUILT;\movie\fighter\brave\c00\final_00.h264",
    ] {
        assert_eq!(paths::normalize(path).unwrap().0, expected);
    }
    assert_eq!(paths::path_hash(expected), paths::hash(expected));
    for path in ["C:/movie/a.h264", "unknown:/movie/a.h264", "prebuilt:/../a",
                 "prebuilt://a", "prebuilt:/movie/a:b", "stream:/movie/a:b"] {
        assert!(paths::normalize(path).is_err(), "{path}");
    }
    let t = Temp::new();
    t.file("packs/hero/prebuilt;/movie/fighter/brave/c00/final_00.h264", b"movie");
    t.file("packs/hero/fighter/brave/model/body/c00/model.numdlb", b"skin");
    let c = Config { root: t.0.join("packs"), region: "us_en".into(), ..Config::default() };
    let p = packages::scan(&c).unwrap();
    assert_eq!(p.entries.len(), 2);
    let entry = p.entries.iter().find(|e| e.path == expected).unwrap();
    assert_eq!(entry.hash, paths::hash(expected));
    assert_eq!(entry.base.as_ref().unwrap().package, "hero");
    t.file("packs/broken/unknown;/movie/a.h264", b"bad namespace");
    let error = match packages::scan(&c) { Ok(_) => panic!("invalid namespace accepted"), Err(e) => e };
    assert!(error.contains("broken: unknown;/movie/a.h264"), "{error}");
}
#[test]
fn scan_priority_region_patch_chain_and_profile() {
    let t = Temp::new();
    t.file("packs/a/ui/a.prc", b"base-a");
    t.file("packs/b/ui/a.prc", b"base-b");
    t.file("packs/b/ui/a+us_en.prc", b"regional-b");
    t.file("packs/b/ui/a+zh_cn.prc", b"other-region");
    t.file("packs/a/ui/a.prcxml", b"patch-a");
    t.file("packs/b/ui/a.prcx", b"patch-b");
    t.file("packs/disabled/ui/a.prc", b"disabled");
    let mut c = Config {
        root: t.0.join("packs"),
        profile: t.0.join("config/mods.json"),
        region: "us_en".into(),
        ..Config::default()
    };
    c.packages.insert(
        "disabled".into(),
        packages::Selection {
            enabled: false,
            priority: 999,
            ..Default::default()
        },
    );
    let p = packages::scan(&c).unwrap();
    assert_eq!(p.entries.len(), 1);
    let e = &p.entries[0];
    assert!(e.base.as_ref().unwrap().disk.ends_with("a+us_en.prc"));
    assert_eq!(
        e.patches
            .iter()
            .map(|p| p.package.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    c.packages.insert(
        "a".into(),
        packages::Selection {
            enabled: true,
            priority: 5,
            ..Default::default()
        },
    );
    assert_eq!(
        packages::scan(&c).unwrap().entries[0]
            .base
            .as_ref()
            .unwrap()
            .package,
        "a"
    );
    packages::save(&c).unwrap();
    let restored: Config = serde_json::from_slice(&fs::read(&c.profile).unwrap()).unwrap();
    assert_eq!(restored.packages["a"].priority, 5);
    c.conflict_mode = "error".into();
    assert!(packages::scan(&c).is_err());
}
#[test]
fn saved_language_and_package_config_forms() {
    let t = Temp::new();
    let mut save = vec![0; 0x3c6099];
    save[0x3c6098] = 8;
    t.file("userdata/save/save_data/system_data.bin", &save);
    t.file("packs/a/config.json",br#"{"share-to-vanilla":{"fighter/mario/a.bin":"fighter/mario/b.bin"},"keep-shared":["fighter/mario/a.bin"]}"#);
    let c = Config {
        root: t.0.join("packs"),
        userdata: t.0.join("userdata"),
        ..Config::default()
    };
    let p = packages::scan(&c).unwrap();
    assert_eq!(p.region, "zh_cn");
    assert_eq!(p.rules.shares[&paths::hash("fighter/mario/a.bin")].len(), 1);
}
fn prc_file(fields: &[(&str, i32)]) -> Vec<u8> {
    let value = prcx::ParamStruct(
        fields
            .iter()
            .map(|(k, v)| (hash40::Hash40::new(k), prcx::ParamKind::I32(*v)))
            .collect(),
    );
    let mut out = Cursor::new(vec![]);
    prcx::write_stream(&mut out, &value).unwrap();
    out.into_inner()
}
#[test]
fn parameter_patches_merge_and_preserve_other_fields() {
    let t = Temp::new();
    let p1 = t.file("a.prcx", &prc_file(&[("value", 7)]));
    let p2 = t.file("b.prcx", &prc_file(&[("value", 9)]));
    let base = prc_file(&[("value", 1), ("untouched", 33)]);
    let out = patches::apply("prc", base, &[source(p1, "prc"), source(p2, "prc")]).unwrap();
    let v = prcx::read_stream(&mut Cursor::new(out)).unwrap();
    assert_eq!(
        v.0.iter()
            .find(|(h, _)| *h == hash40::Hash40::new("value"))
            .unwrap()
            .1,
        prcx::ParamKind::I32(9)
    );
    assert_eq!(
        v.0.iter()
            .find(|(h, _)| *h == hash40::Hash40::new("untouched"))
            .unwrap()
            .1,
        prcx::ParamKind::I32(33)
    );
}
#[test]
fn audio_patch_preserves_ids_and_allocates_new_ids_without_collision() {
    use nus3audio::{AudioFile, Nus3audioFile};
    let t = Temp::new();
    let file = |files: Vec<AudioFile>| {
        let mut b = vec![];
        Nus3audioFile { files }.write(&mut b);
        b
    };
    let af = |id, name: &str, data: &[u8]| AudioFile {
        id,
        name: name.into(),
        data: data.into(),
    };
    let base = file(vec![af(0, "old", b"old"), af(17, "replace", b"before")]);
    let p = t.file(
        "audio.patch3audio",
        &file(vec![af(0, "replace", b"after"), af(1, "new", b"new")]),
    );
    let out = patches::apply("audio", base, &[source(p, "audio")]).unwrap();
    let v = Nus3audioFile::from_bytes(&out);
    let r = v.files.iter().find(|f| f.name == "replace").unwrap();
    assert_eq!(r.id, 17);
    assert_eq!(r.data, b"after");
    assert_eq!(v.files.iter().find(|f| f.name == "new").unwrap().id, 18);
    assert_eq!(
        v.files.iter().find(|f| f.name == "old").unwrap().data,
        b"old"
    );
}
#[test]
fn malformed_requests_and_parser_panics_are_contained() {
    let p = CString::new("not json").unwrap();
    let out = unsafe { sbm_request(p.as_ptr()) };
    let v: Value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { sbm_free(out) };
    assert_eq!(v["ok"], false);
    assert!(guard(|| patches::apply("audio", vec![0, 1], &[])).is_err());
    assert_eq!(unsafe { sbm_read(0, std::ptr::null_mut(), 1) }, -1);
}

#[test]
fn malformed_utf16_xmsbt_skips_only_the_bad_patch() {
    let t = Temp::new();
    // The downloaded Knuckles nameplate has a missing </entry>; the skin is usable.
    let xml = "<?xml version=\"1.0\" encoding=\"utf-16\"?><xmsbt><entry label=\"a\"><text>A</text></xmsbt>";
    let bytes: Vec<u8> = std::iter::once(0xfeffu16)
        .chain(xml.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect();
    t.file("packs/broken/ui/message/msg_name.xmsbt", &bytes);
    t.file("packs/broken/fighter/sonic/model.nutexb", b"skin");
    t.file(
        "packs/good/ui/message/msg_name.xmsbt",
        br#"<xmsbt><entry label="b"><text>B</text></entry></xmsbt>"#,
    );
    let plan = packages::scan(&Config {
        root: t.0.join("packs"),
        region: "us_en".into(),
        ..Config::default()
    })
    .unwrap();
    assert_eq!(plan.entries.len(), 2);
    let messages = plan
        .entries
        .iter()
        .find(|e| e.path == "ui/message/msg_name.msbt")
        .unwrap();
    assert_eq!(messages.patches.len(), 1);
    assert_eq!(messages.patches[0].package, "good");
    assert_eq!(plan.diagnostics.len(), 1);
    assert_eq!(plan.diagnostics[0].code, "invalid_xmsbt");
    assert!(plan.diagnostics[0].message.contains("broken"));
}

#[test]
fn costume_mapping_routes_resources_without_renaming_source_files() {
    let map = [("sonic:c00".to_string(), 3)].into_iter().collect();
    slots::validate(&map).unwrap();
    for (a, b) in [
        (
            "fighter/sonic/model/body/c00/model.numdlb",
            "fighter/sonic/model/body/c03/model.numdlb",
        ),
        (
            "sound/bank/fighter_voice/vc_sonic_c00.nus3audio",
            "sound/bank/fighter_voice/vc_sonic_c03.nus3audio",
        ),
        (
            "ui/replace/chara/chara_1/chara_1_sonic_00.bntx",
            "ui/replace/chara/chara_1/chara_1_sonic_03.bntx",
        ),
        (
            "effect/fighter/sonic/ef_sonic_c00.eff",
            "effect/fighter/sonic/ef_sonic_c03.eff",
        ),
        (
            "fighter/kirby/model/copy_sonic_cap/c00/model.numdlb",
            "fighter/kirby/model/copy_sonic_cap/c03/model.numdlb",
        ),
    ] {
        assert_eq!(slots::remap(a, &map), b);
    }
    assert_eq!(
        slots::remap("fighter/mario/model/body/c00/model.numdlb", &map),
        "fighter/mario/model/body/c00/model.numdlb"
    );
    let t = Temp::new();
    let source = t.file(
        "skin/fighter/sonic/model/body/c00/model.numdlb",
        b"original mod",
    );
    let mut c = packages::Config {
        root: t.0.clone(),
        ..Default::default()
    };
    c.packages.insert(
        "skin".into(),
        packages::Selection {
            slot_map: map,
            ..Default::default()
        },
    );
    let p = packages::scan(&c).unwrap();
    assert_eq!(
        p.entries[0].path,
        "fighter/sonic/model/body/c03/model.numdlb"
    );
    assert_eq!(p.entries[0].base.as_ref().unwrap().disk, source);
    assert_eq!(fs::read(source).unwrap(), b"original mod");
    t.file(
        "skin/fighter/sonic/model/body/c03/model.numdlb",
        b"another costume",
    );
    assert!(packages::scan(&c).err().unwrap().contains("same resource"));
}
#[test]
fn costume_mapping_rejects_unresolvable_hash_rules() {
    let map = [("sonic:c00".to_string(), 3)].into_iter().collect();
    assert!(slots::rules(json!({"keep-shared": ["0x123456"]}), &map).is_err());
    assert_eq!(
        slots::rules(json!({"keep-shared": ["fighter/sonic/c00"]}), &map).unwrap(),
        json!({"keep-shared": ["fighter/sonic/c03"]})
    );
}

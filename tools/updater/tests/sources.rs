#![cfg(feature = "network")]
use ssbu_updater::{network::*, *};
use std::{collections::BTreeMap, fs, io::Cursor};
fn version(n: u64) -> Version {
    Version {
        schema: 1,
        channel: "yuzu-test".into(),
        platform: "win-x64".into(),
        sequence: n,
        version: n.to_string(),
        build_id: n.to_string(),
        game_version: "13.0.4".into(),
        features: Features {
            resource_mods: true,
            latency_slider: true,
        },
        minimum_updater: 1,
    }
}
fn source(name: &str, kind: &str) -> Source {
    Source {
        name: name.into(),
        kind: kind.into(),
        url: format!("https://{name}.example/update.zip"),
        code: String::new(),
        note: String::new(),
    }
}
fn file() -> Artifact {
    Artifact {
        id: "patch".into(),
        kind: "patch".into(),
        base_build_ids: vec!["old".into()],
        size: 4,
        sha256: hash_bytes(b"good"),
        sources: vec![source("primary", "direct"), source("mirror", "direct")],
    }
}
fn release(n: u64, key: &[u8]) -> Vec<u8> {
    let r = Release {
        version: version(n),
        notes: "fix".into(),
        published_at: "2026-10-19".into(),
        artifacts: vec![file()],
    };
    serde_json::to_vec(&sign(&serde_json::to_vec(&r).unwrap(), key, "test").unwrap()).unwrap()
}
#[test]
fn failed_primary_and_stale_mirror_select_highest_valid_signed_release() {
    let (key, pubkey) = generate_key().unwrap();
    let trust = Trust {
        keys: BTreeMap::from([("test".into(), pubkey)]),
    };
    let urls = vec![
        "https://primary.example/feed".into(),
        "https://stale.example/feed".into(),
        "https://mirror.example/feed".into(),
    ];
    let checked = check_with_fetch(
        &urls,
        &trust,
        |_| {},
        |url| {
            if url.contains("primary") {
                Err("timeout".into())
            } else if url.contains("stale") {
                Ok(release(4, &key))
            } else {
                Ok(release(7, &key))
            }
        },
    )
    .unwrap();
    assert_eq!(checked.release.version.sequence, 7);
    assert_eq!(checked.failures.len(), 1);
    assert!(checked.source.contains("mirror"));
}
#[test]
fn all_failed_or_untrusted_sources_are_errors_not_latest() {
    let (_, pubkey) = generate_key().unwrap();
    let (key, _) = generate_key().unwrap();
    let trust = Trust {
        keys: BTreeMap::from([("test".into(), pubkey)]),
    };
    let urls = vec!["https://example.test/feed".into()];
    assert!(
        check_with_fetch(&urls, &trust, |_| {}, |_| Err("offline".into()))
            .unwrap_err()
            .contains("暂时无法检查")
    );
    assert!(check_with_fetch(&urls, &trust, |_| {}, |_| Ok(release(1, &key))).is_err());
}
#[test]
fn browser_sources_are_never_programmatically_downloaded() {
    let mut a = file();
    a.sources = vec![source("share", "browser")];
    let dir = std::env::temp_dir().join(format!("ssbu-update-net-test-{}", unique()));
    assert!(download_with_fetch(
        &a,
        None,
        &dir,
        |_, _| true,
        |_| panic!("must not fetch browser share")
    )
    .is_err());
    let _ = fs::remove_dir_all(dir);
}
#[test]
fn corrupted_direct_source_falls_back_without_weakening_hash_checks() {
    let a = file();
    let dir = std::env::temp_dir().join(format!("ssbu-update-net-test-{}", unique()));
    let path = download_with_fetch(
        &a,
        None,
        &dir,
        |_, _| true,
        |url| {
            Ok(Box::new(Cursor::new(if url.contains("primary") {
                b"bad!".to_vec()
            } else {
                b"good".to_vec()
            })))
        },
    )
    .unwrap();
    assert_eq!(fs::read(path).unwrap(), b"good");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    let _ = fs::remove_dir_all(dir);
}
#[test]
fn cancellation_does_not_silently_try_another_source() {
    let a = file();
    let dir = std::env::temp_dir().join(format!("ssbu-update-net-test-{}", unique()));
    let mut calls = 0;
    let cancelled = download_with_fetch(
        &a,
        None,
        &dir,
        |_, _| false,
        |_| {
            calls += 1;
            Ok(Box::new(Cursor::new(b"good".to_vec())))
        },
    )
    .unwrap_err();
    assert!(cancelled.contains("取消"), "{cancelled}");
    assert_eq!(calls, 1);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    let _ = fs::remove_dir_all(dir);
}
#[test]
fn credentials_and_non_https_urls_are_refused() {
    for u in [
        "http://example.test/file",
        "file:///C:/Windows/file",
        "https://user:pass@example.test/file",
        "https://example.test/file#fragment",
    ] {
        assert!(checked_url(u).is_err());
    }
    assert!(checked_url("https://example.test/a?token=public-download-id").is_ok());
}
#[test]
fn wrong_patch_base_selects_full_archive() {
    let mut r = Release {
        version: version(1),
        notes: String::new(),
        published_at: String::new(),
        artifacts: vec![file()],
    };
    let current = Installed {
        build_id: "another-build".into(),
        sequence: 0,
        version: "old".into(),
        features: version(1).features,
        game_version: "13.0.4".into(),
    };
    assert!(artifact(&r, &current).is_none());
    let mut full = file();
    full.kind = "full".into();
    r.artifacts.push(full);
    assert_eq!(artifact(&r, &current).unwrap().kind, "full");
}

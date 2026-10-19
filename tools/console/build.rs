fn main() {
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let helper = std::env::var_os("SSBU_UPDATER_EXE").map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../build/updater-target/release/ssbu-updater.exe"));
    if !helper.is_file() { panic!("Build tools/updater --release --no-default-features before building Console"); }
    println!("cargo:rerun-if-env-changed=SSBU_UPDATER_EXE");
    println!("cargo:rerun-if-changed={}", helper.display());
    println!("cargo:rustc-env=SSBU_UPDATER_EXE={}", helper.canonicalize().unwrap().display());
    tauri_build::build()
}

fn main() {
    let version_path = std::path::Path::new("../VERSION");
    println!("cargo:rerun-if-changed=../VERSION");
    let version = std::fs::read_to_string(version_path)
        .ok()
        .map(|s| s.trim().trim_start_matches(['v', 'V']).to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=SYLLABARY_VERSION={version}");

    println!("cargo:rerun-if-changed=assets/installer.rc");
    println!("cargo:rerun-if-changed=assets/installer.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let _ = embed_resource::compile("assets/installer.rc", embed_resource::NONE);
    }
}

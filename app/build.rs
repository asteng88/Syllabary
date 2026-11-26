fn main() {
    // Single source of truth for the application version: ../VERSION
    let version_path = std::path::Path::new("../VERSION");
    println!("cargo:rerun-if-changed=../VERSION");
    let version = std::fs::read_to_string(version_path)
        .ok()
        .map(|s| s.trim().trim_start_matches(['v', 'V']).to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=SYLLABARY_VERSION={version}");

    println!("cargo:rerun-if-changed=assets/icons/syllabary.rc");
    for name in [
        "syllabary.ico",
        "doc_wpx.ico",
        "doc_docx.ico",
        "doc_pdf.ico",
        "doc_gdoc.ico",
        "doc_md.ico",
        "doc_rtf.ico",
        "doc_txt.ico",
    ] {
        println!("cargo:rerun-if-changed=assets/icons/{name}");
    }
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let _ = embed_resource::compile("assets/icons/syllabary.rc", embed_resource::NONE);
    }
}

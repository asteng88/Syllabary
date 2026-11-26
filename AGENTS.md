# Syllabary Cross-Platform & Release Rules

## 1. Windows & Linux Compatibility (Required on Every Code Change)
Every change to the Syllabary codebase (`app/`) MUST remain compatible with **both Windows and Linux (including Fedora)**:

- **Platform-specific APIs & dependencies**:
  - Gate all Windows-only crates and code behind `#[cfg(windows)]` / `[target.'cfg(windows)'.dependencies]` / `[target.'cfg(windows)'.build-dependencies]` (including inside `app/build.rs`).
  - Gate Linux-only crates and code (such as `spellbook` Hunspell spell checking and XDG file associations) behind `#[cfg(target_os = "linux")]` / `[target.'cfg(target_os = "linux")'.dependencies]`.
  - Provide clean cross-platform fallbacks or `#[cfg_attr(..., expect(...))]` annotations so **neither Windows nor Linux produces compiler warnings**.
- **Paths, Fonts, & OS Tools**:
  - Never hardcode Windows-only paths (`C:\...`, backslashes) in runtime code; use `std::path::Path` / `PathBuf` and `dirs::*` (XDG-compliant on Linux).
  - Keep font discovery working when Windows system fonts are absent (bundled open-source font fallbacks in `fonts.rs` and Fedora/Debian system font paths).
  - Keep printing (`print.rs`: Win32 GDI on Windows, CUPS `lpstat`/`lp`/`lpr` + `xdg-open` on Linux), file associations (`file_assoc.rs`), and spell checking (`spell.rs`) working on both OSes.
- **Verification**:
  - Windows check: `cd app && cargo check`
  - Linux check/build (via Fedora WSL2):
    ```powershell
    wsl -- bash -lc 'cd /mnt/p/Syllabary/app && CARGO_TARGET_DIR=/tmp/syllabary-target cargo build --release && mkdir -p target/linux-release && cp /tmp/syllabary-target/release/syllabary target/linux-release/syllabary'
    ```

## 2. Git Upload & Release Compilation Prompt
Whenever the user is ready to **push/upload to Git** or **create a release** (e.g., mentions committing/pushing for a release, tagging a version, uploading to GitHub, or building release artifacts):

- **ALWAYS ask the user** whether they want to compile for:
  1. **Windows** (`app/target/release/syllabary.exe`)
  2. **Linux** (`app/target/linux-release/syllabary` via Fedora WSL2)
  3. **Both Windows and Linux**
- Wait for their choice and run the requested release build(s) before finalizing the upload/release.

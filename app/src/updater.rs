//! Direct GitHub Releases auto-updater for Syllabary.
//!
//! Queries the GitHub Releases API directly (no license keys or access codes),
//! compares semantic versions, streams verified downloads (byte length + SHA-256
//! digest when provided by GitHub), and replaces the running executable in-place
//! with automatic `.exe.old` cleanup on Windows/macOS/Linux.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};

/// Current application version from the repository root `VERSION` file (injected by `build.rs`).
pub const CURRENT_VERSION: &str = env!("SYLLABARY_VERSION");

/// Direct GitHub Releases API endpoint for Syllabary (no proxy or codes required).
pub const GITHUB_LATEST_API_URL: &str =
    "https://api.github.com/repos/asteng88/Syllabary/releases/latest";

/// Fallback GitHub Releases web page.
pub const GITHUB_RELEASES_PAGE: &str = "https://github.com/asteng88/Syllabary/releases";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub version: String,
    pub name: String,
    pub body: String,
    pub html_url: String,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub asset_size: u64,
    pub asset_sha256: Option<String>,
    pub is_newer: bool,
}

#[derive(Clone, Debug)]
pub enum CheckOutcome {
    UpdateAvailable(ReleaseInfo),
    UpToDate(String),
    NoReleases,
    Error(String),
}

/// Extracts an integer version tuple from a tag/version string like `"v1.2.1"` → `[1, 2, 1]`.
pub fn parse_version(version_str: &str) -> Vec<u64> {
    let clean = version_str.trim().trim_start_matches(['v', 'V']);
    let mut nums = Vec::new();
    let mut cur = String::new();
    for ch in clean.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            if let Ok(v) = cur.parse::<u64>() {
                nums.push(v);
            }
            cur.clear();
        }
    }
    if !cur.is_empty() {
        if let Ok(v) = cur.parse::<u64>() {
            nums.push(v);
        }
    }
    if nums.is_empty() {
        vec![0]
    } else {
        nums
    }
}

/// Returns `true` if `candidate` is strictly newer than `current` (with zero-padded comparison).
pub fn is_newer_version(current: &str, candidate: &str) -> bool {
    let v_curr = parse_version(current);
    let v_cand = parse_version(candidate);
    let max_len = v_curr.len().max(v_cand.len());
    for i in 0..max_len {
        let a = v_curr.get(i).copied().unwrap_or(0);
        let b = v_cand.get(i).copied().unwrap_or(0);
        if b != a {
            return b > a;
        }
    }
    false
}

/// Only allows update downloads over HTTPS with a non-empty host.
pub fn is_allowed_download_url(url: &str) -> bool {
    let trimmed = url.trim();
    if let Some(rest) = trimmed.strip_prefix("https://") {
        let host = rest.split('/').next().unwrap_or("");
        !host.is_empty() && !host.contains(char::is_whitespace)
    } else {
        false
    }
}

/// Extracts the expected 64-char hex SHA-256 digest for `asset_name` if GitHub's API provides one
/// (`"digest": "sha256:<hex>"`).
pub fn find_asset_sha256(assets: &[serde_json::Value], asset_name: &str) -> Option<String> {
    for asset in assets {
        if asset.get("name").and_then(|v| v.as_str()) != Some(asset_name) {
            continue;
        }
        let digest = asset.get("digest").and_then(|v| v.as_str())?.trim();
        let rest = digest
            .strip_prefix("sha256:")
            .or_else(|| digest.strip_prefix("SHA256:"))?
            .trim()
            .to_ascii_lowercase();
        if rest.len() == 64 && rest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(rest);
        }
    }
    None
}

/// Picks the best matching executable asset for the current OS from a GitHub release's `assets`.
pub fn find_executable_asset(
    assets: &[serde_json::Value],
) -> (Option<String>, Option<String>, u64) {
    #[cfg(target_os = "windows")]
    let (ext, os_hint) = (".exe", "windows");
    #[cfg(target_os = "macos")]
    let (ext, os_hint) = (".dmg", "macos");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let (ext, os_hint) = ("", "linux");

    // 1. Prioritize asset named with "syllabary" + matching extension/OS
    for asset in assets {
        let Some(name) = asset.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let low = name.to_ascii_lowercase();
        let ext_ok = if ext.is_empty() {
            low.contains(os_hint) && !low.ends_with(".exe") && !low.ends_with(".dmg")
        } else {
            low.ends_with(ext)
        };
        if ext_ok && low.contains("syllabary") {
            let url = asset
                .get("browser_download_url")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let size = asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            return (Some(name.to_string()), url, size);
        }
    }

    // 2. Fallback: any asset with matching extension
    if !ext.is_empty() {
        for asset in assets {
            let Some(name) = asset.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            if name.to_ascii_lowercase().ends_with(ext) {
                let url = asset
                    .get("browser_download_url")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let size = asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                return (Some(name.to_string()), url, size);
            }
        }
    }

    (None, None, 0)
}

/// Parses a GitHub Releases API JSON payload into `ReleaseInfo`.
pub fn parse_github_release(raw_json: &str, current_version: &str) -> Option<ReleaseInfo> {
    let data: serde_json::Value = serde_json::from_str(raw_json).ok()?;
    let tag_name = data.get("tag_name").and_then(|v| v.as_str())?.trim().to_string();
    if tag_name.is_empty() {
        return None;
    }
    let version = tag_name.trim_start_matches(['v', 'V']).to_string();
    let name = data
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&tag_name)
        .to_string();
    let body = data
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let html_url = data
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or(GITHUB_RELEASES_PAGE)
        .to_string();

    let assets = data
        .get("assets")
        .and_then(|v| v.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let (asset_name, asset_url, asset_size) = find_executable_asset(assets);
    let asset_sha256 = asset_name
        .as_deref()
        .and_then(|n| find_asset_sha256(assets, n));
    let is_newer = is_newer_version(current_version, &version);

    Some(ReleaseInfo {
        tag_name,
        version,
        name,
        body,
        html_url,
        asset_name,
        asset_url,
        asset_size,
        asset_sha256,
        is_newer,
    })
}

/// Directly queries GitHub Releases API for the latest Syllabary release.
pub fn check_for_updates(current_version: &str) -> CheckOutcome {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build();
    let agent: ureq::Agent = config.into();
    let resp = agent
        .get(GITHUB_LATEST_API_URL)
        .header("User-Agent", &format!("Syllabary/{current_version}"))
        .header("Accept", "application/vnd.github+json")
        .call();

    match resp {
        Ok(mut r) => match r.body_mut().read_to_string() {
            Ok(body) => match parse_github_release(&body, current_version) {
                Some(info) if info.is_newer => CheckOutcome::UpdateAvailable(info),
                Some(info) => CheckOutcome::UpToDate(info.version),
                None => CheckOutcome::Error("Could not parse GitHub release metadata.".into()),
            },
            Err(e) => CheckOutcome::Error(format!("Failed to read response: {e}")),
        },
        Err(ureq::Error::StatusCode(404)) => CheckOutcome::NoReleases,
        Err(e) => CheckOutcome::Error(format!("Could not reach GitHub: {e}")),
    }
}

/// Streams `asset_url` to `dest_path` with progress reporting, cancellation, byte-length check,
/// and SHA-256 verification (when `expected_sha256` is provided).
pub fn download_update_asset<F>(
    asset_url: &str,
    dest_path: &Path,
    expected_size: u64,
    expected_sha256: Option<&str>,
    cancel: &Arc<AtomicBool>,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(u64, u64),
{
    if !is_allowed_download_url(asset_url) {
        return Err("Refusing non-HTTPS update download URL.".into());
    }
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Could not create directory: {e}"))?;
    }

    let temp_file = dest_path.with_extension("tmp_download");
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_body(Some(Duration::from_secs(180)))
        .build();
    let agent: ureq::Agent = config.into();

    let mut resp = agent
        .get(asset_url)
        .header("User-Agent", &format!("Syllabary/{CURRENT_VERSION}"))
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(|e| format!("Download request failed: {e}"))?;

    let content_len = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(expected_size);

    let mut reader = resp.body_mut().as_reader();
    let mut file = File::create(&temp_file)
        .map_err(|e| format!("Could not create temporary update file: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 65_536];
    let mut downloaded = 0u64;

    let result: Result<(), String> = (|| {
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("Download cancelled.".into());
            }
            let n = reader
                .read(&mut buf)
                .map_err(|e| format!("Network read failed: {e}"))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .map_err(|e| format!("Disk write failed: {e}"))?;
            hasher.update(&buf[..n]);
            downloaded += n as u64;
            on_progress(downloaded, content_len);
        }
        file.flush().map_err(|e| format!("Disk flush failed: {e}"))?;
        drop(file);

        if downloaded == 0 {
            return Err("Downloaded file was empty.".into());
        }
        if content_len > 0 && downloaded != content_len {
            return Err(format!(
                "Incomplete download ({downloaded} of {content_len} bytes)."
            ));
        }
        if expected_size > 0 && downloaded != expected_size {
            return Err(format!(
                "Size mismatch ({downloaded} bytes, expected {expected_size})."
            ));
        }
        if let Some(exp_hex) = expected_sha256 {
            let actual = format!("{:x}", hasher.finalize());
            if !actual.eq_ignore_ascii_case(exp_hex.trim()) {
                return Err("SHA-256 verification failed.".into());
            }
        }
        fs::rename(&temp_file, dest_path)
            .map_err(|e| format!("Could not finalize downloaded file: {e}"))?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_file);
    }
    result
}

/// Replaces the currently running executable in-place on disk using the Capacity method:
/// 1. Rename `current_exe` → `current_exe.old` (permitted on Windows NTFS while running)
/// 2. Rename `new_exe` → `current_exe` (rolls back on failure)
/// 3. Schedule detached invisible cleanup of `.old` after exit.
pub fn replace_executable_in_place(new_exe_path: &Path) -> Result<PathBuf, String> {
    if !new_exe_path.exists() {
        return Err("Downloaded update executable does not exist.".into());
    }
    let current_exe = std::env::current_exe()
        .and_then(|p| p.canonicalize().or(Ok(p)))
        .map_err(|e| format!("Could not locate current executable: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(new_exe_path, fs::Permissions::from_mode(0o755));
    }

    let old_backup = old_backup_path(&current_exe);
    if old_backup.exists() {
        let _ = fs::remove_file(&old_backup);
    }

    fs::rename(&current_exe, &old_backup).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            "This installation is managed in a read-only system directory (such as an RPM or Flatpak). Please update Syllabary using your system package manager.".to_string()
        } else {
            format!("Could not move current executable aside: {e}")
        }
    })?;

    if let Err(e) = fs::rename(new_exe_path, &current_exe) {
        // Attempt rollback so the user is never left without an executable
        if !current_exe.exists() && old_backup.exists() {
            let _ = fs::rename(&old_backup, &current_exe);
        }
        return Err(format!("Could not move new executable into place: {e}"));
    }

    let _ = schedule_delete_old_backup(&old_backup);
    Ok(current_exe)
}

fn old_backup_path(exe: &Path) -> PathBuf {
    let mut os = exe.as_os_str().to_os_string();
    os.push(".old");
    PathBuf::from(os)
}

/// Spawns a detached invisible cleaner on Windows to delete `.exe.old` after Syllabary exits.
#[cfg(windows)]
pub fn schedule_delete_old_backup(old_path: &Path) -> bool {
    use std::process::{Command, Stdio};

    if !old_path.exists() {
        return false;
    }
    let Some(parent) = old_path.parent() else {
        return false;
    };
    let vbs_file = parent.join(format!("_clean_old_{}.vbs", std::process::id()));
    let esc_old = old_path.to_string_lossy().replace('"', "\"\"");
    let vbs_content = format!(
        "Set fso = CreateObject(\"Scripting.FileSystemObject\")\r\n\
         WScript.Sleep 1500\r\n\
         On Error Resume Next\r\n\
         For i = 1 To 25\r\n\
             fso.DeleteFile \"{esc_old}\", True\r\n\
             If Err.Number = 0 Then Exit For\r\n\
             Err.Clear\r\n\
             WScript.Sleep 500\r\n\
         Next\r\n\
         fso.DeleteFile WScript.ScriptFullName, True\r\n"
    );
    if fs::write(&vbs_file, vbs_content).is_err() {
        return false;
    }
    Command::new("wscript.exe")
        .arg(&vbs_file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(not(windows))]
pub fn schedule_delete_old_backup(old_path: &Path) -> bool {
    fs::remove_file(old_path).is_ok()
}

/// Silently deletes any `.old` executable backup left over from a previous update.
pub fn cleanup_old_executable_backup() {
    if let Ok(exe) = std::env::current_exe() {
        let old = old_backup_path(&exe);
        if old.exists() {
            let _ = fs::remove_file(&old);
        }
    }
}

/// Relaunches the newly installed executable and exits the current process.
pub fn relaunch_and_exit() -> ! {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).spawn();
    }
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_version_comparison_handles_prefixes_and_padding() {
        assert_eq!(parse_version("v1.2.3"), vec![1, 2, 3]);
        assert!(is_newer_version("1.0.0", "1.0.1"));
        assert!(is_newer_version("1.0.0", "v1.1"));
        assert!(is_newer_version("1.9.0", "1.10.0"));
        assert!(!is_newer_version("1.0.0", "v1.0.0"));
        assert!(!is_newer_version("1.2.0", "1.1.9"));
    }

    #[test]
    fn https_download_url_policy() {
        assert!(is_allowed_download_url(
            "https://github.com/asteng88/Syllabary/releases/download/v1.1.0/syllabary.exe"
        ));
        assert!(!is_allowed_download_url("http://example.com/syllabary.exe"));
        assert!(!is_allowed_download_url("file:///C:/syllabary.exe"));
        assert!(!is_allowed_download_url(""));
    }

    #[test]
    fn parses_github_release_payload_and_sha256_digest() {
        let digest = "a".repeat(64);
        let payload = serde_json::json!({
            "tag_name": "v1.2.0",
            "name": "Syllabary 1.2.0",
            "body": "- Improved DOCX & PDF accuracy",
            "html_url": "https://github.com/asteng88/Syllabary/releases/tag/v1.2.0",
            "assets": [
                {
                    "name": "notes.txt",
                    "browser_download_url": "https://github.com/asteng88/Syllabary/releases/download/v1.2.0/notes.txt",
                    "size": 123
                },
                {
                    "name": "syllabary.exe",
                    "browser_download_url": "https://github.com/asteng88/Syllabary/releases/download/v1.2.0/syllabary.exe",
                    "size": 30000000,
                    "digest": format!("sha256:{digest}")
                }
            ]
        });
        let info = parse_github_release(&payload.to_string(), "1.0.0").expect("release parsed");
        assert!(info.is_newer);
        assert_eq!(info.version, "1.2.0");
        #[cfg(target_os = "windows")]
        {
            assert_eq!(info.asset_name.as_deref(), Some("syllabary.exe"));
            assert_eq!(info.asset_sha256.as_deref(), Some(digest.as_str()));
            assert_eq!(info.asset_size, 30_000_000);
        }
    }
}

//! Queries GitHub Releases for the latest Syllabary release and streams verified downloads.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

pub const GITHUB_LATEST_API_URL: &str =
    "https://api.github.com/repos/asteng88/Syllabary/releases/latest";

#[derive(Clone, Debug)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub version: String,
    pub name: String,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub asset_size: u64,
    pub asset_sha256: Option<String>,
}

fn is_allowed_download_url(url: &str) -> bool {
    let trimmed = url.trim();
    if let Some(rest) = trimmed.strip_prefix("https://") {
        let host = rest.split('/').next().unwrap_or("");
        !host.is_empty() && !host.contains(char::is_whitespace)
    } else {
        false
    }
}

fn find_asset_sha256(assets: &[serde_json::Value], asset_name: &str) -> Option<String> {
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

/// Selects the best Syllabary application executable asset for the current OS,
/// deliberately skipping installer binaries (`*installer*`).
pub fn find_app_asset(assets: &[serde_json::Value]) -> (Option<String>, Option<String>, u64) {
    #[cfg(target_os = "windows")]
    let is_match = |low: &str| {
        low.contains("syllabary") && !low.contains("installer") && low.ends_with(".exe")
    };
    #[cfg(target_os = "linux")]
    let is_match = |low: &str| {
        low.contains("syllabary")
            && !low.contains("installer")
            && !low.ends_with(".exe")
            && !low.ends_with(".dmg")
            && !low.ends_with(".zip")
            && !low.ends_with(".tar.gz")
            && !low.ends_with(".sha256")
            && !low.ends_with(".txt")
            && (low.contains("linux") || low == "syllabary")
    };
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let is_match = |low: &str| {
        low.contains("syllabary") && !low.contains("installer") && low.ends_with(".dmg")
    };

    for asset in assets {
        let Some(name) = asset.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let low = name.to_ascii_lowercase();
        if is_match(&low) {
            let url = asset
                .get("browser_download_url")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let size = asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            return (Some(name.to_string()), url, size);
        }
    }

    // Secondary fallback on Windows: any non-installer .exe
    #[cfg(target_os = "windows")]
    for asset in assets {
        let Some(name) = asset.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let low = name.to_ascii_lowercase();
        if low.ends_with(".exe") && !low.contains("installer") {
            let url = asset
                .get("browser_download_url")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let size = asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            return (Some(name.to_string()), url, size);
        }
    }

    (None, None, 0)
}

pub fn fetch_latest_release() -> Result<ReleaseInfo, String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .build();
    let agent: ureq::Agent = config.into();

    let mut resp = agent
        .get(GITHUB_LATEST_API_URL)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "Syllabary-Installer")
        .call()
        .map_err(|e| format!("Could not reach GitHub Releases: {e}"))?;

    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Could not read GitHub response: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("Invalid GitHub JSON: {e}"))?;

    let tag_name = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if tag_name.is_empty() {
        return Err("No published releases found on GitHub yet.".into());
    }
    let version = tag_name.trim().trim_start_matches(['v', 'V']).to_string();
    let name = json
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&tag_name)
        .to_string();

    let assets = json
        .get("assets")
        .and_then(|v| v.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let (asset_name, asset_url, asset_size) = find_app_asset(assets);
    let asset_sha256 = asset_name
        .as_deref()
        .and_then(|n| find_asset_sha256(assets, n));

    Ok(ReleaseInfo {
        tag_name,
        version,
        name,
        asset_name,
        asset_url,
        asset_size,
        asset_sha256,
    })
}

pub fn download_verified<F>(
    url: &str,
    dest: &Path,
    expected_size: u64,
    expected_sha256: Option<&str>,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(u64, u64),
{
    if !is_allowed_download_url(url) {
        return Err("Refusing non-HTTPS download URL.".into());
    }

    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .build();
    let agent: ureq::Agent = config.into();

    let mut resp = agent
        .get(url)
        .header("User-Agent", "Syllabary-Installer")
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;

    let content_len = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(expected_size);

    let mut reader = resp.body_mut().as_reader();
    let mut file =
        File::create(dest).map_err(|e| format!("Could not create temp file: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Network read error: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("Disk write error: {e}"))?;
        hasher.update(&buf[..n]);
        downloaded += n as u64;
        on_progress(downloaded, content_len);
    }
    file.flush()
        .map_err(|e| format!("Could not flush downloaded file: {e}"))?;

    if expected_size > 0 && downloaded != expected_size {
        let _ = std::fs::remove_file(dest);
        return Err(format!(
            "Download size mismatch (expected {expected_size} bytes, got {downloaded} bytes)."
        ));
    }

    if let Some(want) = expected_sha256 {
        let got: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if !got.eq_ignore_ascii_case(want) {
            let _ = std::fs::remove_file(dest);
            return Err("SHA-256 verification failed! The downloaded file may be corrupted.".into());
        }
    }

    Ok(())
}

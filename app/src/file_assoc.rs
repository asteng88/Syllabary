//! Per-user file-type association registration (no administrator privileges required).
//!
//! On Windows, registers ProgIDs, `OpenWithProgids`, `Applications\syllabary.exe`,
//! `Software\Syllabary\Capabilities\FileAssociations`, and `Software\RegisteredApplications`
//! under `HKEY_CURRENT_USER`, then notifies Explorer via `SHChangeNotify`.

use std::collections::BTreeSet;

/// Metadata for a document extension Syllabary can open and associate with.
#[derive(Clone, Copy, Debug)]
pub struct FileTypeSpec {
    pub ext: &'static str,
    pub label: &'static str,
    pub prog_id: &'static str,
    pub description: &'static str,
    pub icon_index: u32,
}

pub const SUPPORTED_EXTENSIONS: &[FileTypeSpec] = &[
    FileTypeSpec {
        ext: "wpx",
        label: "Syllabary Document",
        prog_id: "Syllabary.Document.wpx",
        description: "Syllabary Document",
        icon_index: 1,
    },
    FileTypeSpec {
        ext: "docx",
        label: "Word Document",
        prog_id: "Syllabary.Document.docx",
        description: "Word Document (Syllabary)",
        icon_index: 2,
    },
    FileTypeSpec {
        ext: "odt",
        label: "OpenDocument Text",
        prog_id: "Syllabary.Document.odt",
        description: "OpenDocument Text (Syllabary)",
        icon_index: 1,
    },
    FileTypeSpec {
        ext: "pdf",
        label: "PDF Document",
        prog_id: "Syllabary.Document.pdf",
        description: "PDF Document (Syllabary)",
        icon_index: 3,
    },
    FileTypeSpec {
        ext: "gdoc",
        label: "Google Doc",
        prog_id: "Syllabary.Document.gdoc",
        description: "Google Doc (Syllabary)",
        icon_index: 4,
    },
    FileTypeSpec {
        ext: "md",
        label: "Markdown",
        prog_id: "Syllabary.Document.md",
        description: "Markdown Document (Syllabary)",
        icon_index: 5,
    },
    FileTypeSpec {
        ext: "rtf",
        label: "Rich Text Format",
        prog_id: "Syllabary.Document.rtf",
        description: "Rich Text Document (Syllabary)",
        icon_index: 6,
    },
    FileTypeSpec {
        ext: "txt",
        label: "Plain Text",
        prog_id: "Syllabary.Document.txt",
        description: "Text Document (Syllabary)",
        icon_index: 7,
    },
];

/// Default checked extensions for fresh installations.
pub fn default_selected_extensions() -> Vec<String> {
    SUPPORTED_EXTENSIONS.iter().map(|s| s.ext.to_string()).collect()
}

/// Normalizes a user extension list into canonical lowercase supported extensions without leading dots.
pub fn normalize_selected_extensions(exts: &[String]) -> Vec<&'static FileTypeSpec> {
    let want: BTreeSet<String> = exts
        .iter()
        .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
        .collect();
    SUPPORTED_EXTENSIONS
        .iter()
        .filter(|spec| want.contains(spec.ext))
        .collect()
}

/// Outcome of registering Syllabary as the default handler for selected extensions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplyResult {
    /// Extensions successfully registered in `HKCU\Software\Classes` & `Capabilities`.
    pub registered: Vec<String>,
    /// Extensions where Windows `UserChoice` currently locks another app as default;
    /// Syllabary is registered in `Default Apps` so the user can confirm them with one click.
    pub protected_by_windows: Vec<String>,
}

/// Registers Syllabary as the per-user default application for the given extensions.
#[cfg(windows)]
pub fn apply_default_associations(extensions: &[String]) -> Result<ApplyResult, String> {
    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

    let specs = normalize_selected_extensions(extensions);
    if specs.is_empty() {
        return Err("Check at least one file extension first.".into());
    }

    let exe_path = std::env::current_exe()
        .map_err(|e| format!("Could not determine executable path: {e}"))?;
    let exe_str = exe_path.to_string_lossy().into_owned();
    let exe_name = exe_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("syllabary.exe");

    let open_cmd = format!("\"{exe_str}\" \"%1\"");
    let app_icon = format!("\"{exe_str}\",0");

    // 1. Register Applications\<exe_name> for Windows "Open with" & Default Apps
    let app_base = format!("Software\\Classes\\Applications\\{exe_name}");
    win_reg_set_sz(&app_base, "FriendlyAppName", "Syllabary")?;
    win_reg_set_sz(&format!("{app_base}\\DefaultIcon"), "", &app_icon)?;
    win_reg_set_sz(&format!("{app_base}\\shell\\open\\command"), "", &open_cmd)?;

    // 2. Register Capabilities + RegisteredApplications (Windows 10/11 Default Apps contract)
    let caps_base = "Software\\Syllabary\\Capabilities";
    win_reg_set_sz(caps_base, "ApplicationName", "Syllabary")?;
    win_reg_set_sz(
        caps_base,
        "ApplicationDescription",
        "Fast, offline desktop word processor",
    )?;
    win_reg_set_sz("Software\\RegisteredApplications", "Syllabary", caps_base)?;

    let mut result = ApplyResult::default();

    // 3. Register each checked extension
    for spec in specs {
        let prog_base = format!("Software\\Classes\\{}", spec.prog_id);
        let icon_val = format!("\"{exe_str}\",{}", spec.icon_index);
        win_reg_set_sz(&prog_base, "", spec.description)?;
        win_reg_set_sz(&format!("{prog_base}\\DefaultIcon"), "", &icon_val)?;
        win_reg_set_sz(&format!("{prog_base}\\shell\\open\\command"), "", &open_cmd)?;

        let dot_ext = format!(".{}", spec.ext);
        let ext_base = format!("Software\\Classes\\{dot_ext}");
        win_reg_set_sz(&ext_base, "", spec.prog_id)?;
        win_reg_set_sz(&format!("{ext_base}\\OpenWithProgids"), spec.prog_id, "")?;

        win_reg_set_sz(&format!("{app_base}\\SupportedTypes"), &dot_ext, "")?;
        win_reg_set_sz(&format!("{caps_base}\\FileAssociations"), &dot_ext, spec.prog_id)?;

        // Check whether Windows UserChoice currently locks another ProgId for this extension
        let uc_key = format!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{dot_ext}\\UserChoice"
        );
        if let Some(current_prog_id) = win_reg_get_sz(&uc_key, "ProgId") {
            if !current_prog_id.eq_ignore_ascii_case(spec.prog_id) {
                result.protected_by_windows.push(dot_ext.clone());
            }
        }
        result.registered.push(dot_ext);
    }

    // 4. Notify Windows Shell to refresh file-type icons & handlers immediately
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }

    Ok(result)
}

#[cfg(not(windows))]
pub fn apply_default_associations(extensions: &[String]) -> Result<ApplyResult, String> {
    let specs = normalize_selected_extensions(extensions);
    if specs.is_empty() {
        return Err("Check at least one file extension first.".into());
    }
    Err("Automatic default-app registration is currently supported on Windows.".into())
}

/// Returns `true` if `ext` (with or without leading dot) is currently associated with Syllabary.
#[cfg(windows)]
pub fn is_registered_default(ext: &str) -> bool {
    let clean = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    let expected = format!("Syllabary.Document.{clean}");
    let uc_key = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\.{clean}\\UserChoice"
    );
    if let Some(uc) = win_reg_get_sz(&uc_key, "ProgId") {
        return uc.eq_ignore_ascii_case(&expected);
    }
    let classes_key = format!("Software\\Classes\\.{clean}");
    win_reg_get_sz(&classes_key, "")
        .map(|v| v.eq_ignore_ascii_case(&expected))
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn is_registered_default(_ext: &str) -> bool {
    false
}

/// Opens the OS Default Apps settings page so the user can confirm Windows-protected extensions.
#[cfg(windows)]
pub fn open_system_default_apps() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("cmd")
        .args(["/C", "start", "", "ms-settings:defaultapps"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open Windows Default Apps: {e}"))
}

#[cfg(not(windows))]
pub fn open_system_default_apps() -> Result<(), String> {
    Err("System Default Apps shortcut is only available on Windows.".into())
}

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn win_reg_set_sz(subkey: &str, value_name: &str, data: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };

    let subkey_w = to_wide(subkey);
    let name_w = to_wide(value_name);
    let data_w = to_wide(data);
    let data_bytes = unsafe {
        std::slice::from_raw_parts(data_w.as_ptr() as *const u8, data_w.len() * 2)
    };

    let mut hkey = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey_w.as_ptr()),
            Some(0),
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(format!("Registry create failed for HKCU\\{subkey}: {status:?}"));
    }

    let set_status = unsafe {
        RegSetValueExW(
            hkey,
            if value_name.is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(name_w.as_ptr())
            },
            Some(0),
            REG_SZ,
            Some(data_bytes),
        )
    };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if set_status != ERROR_SUCCESS {
        return Err(format!(
            "Registry write failed for HKCU\\{subkey}\\{value_name}: {set_status:?}"
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn win_reg_get_sz(subkey: &str, value_name: &str) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
        REG_VALUE_TYPE,
    };

    let subkey_w = to_wide(subkey);
    let name_w = to_wide(value_name);
    let mut hkey = HKEY::default();
    let open_status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey_w.as_ptr()),
            Some(0),
            KEY_QUERY_VALUE,
            &mut hkey,
        )
    };
    if open_status != ERROR_SUCCESS {
        return None;
    }

    let val_ptr = if value_name.is_empty() {
        PCWSTR::null()
    } else {
        PCWSTR(name_w.as_ptr())
    };
    let mut reg_type = REG_VALUE_TYPE(0);
    let mut byte_len: u32 = 0;
    let size_status = unsafe {
        RegQueryValueExW(hkey, val_ptr, None, Some(&mut reg_type), None, Some(&mut byte_len))
    };
    if size_status != ERROR_SUCCESS || byte_len == 0 {
        unsafe {
            let _ = RegCloseKey(hkey);
        }
        return None;
    }

    let mut buf = vec![0u8; byte_len as usize];
    let read_status = unsafe {
        RegQueryValueExW(
            hkey,
            val_ptr,
            None,
            Some(&mut reg_type),
            Some(buf.as_mut_ptr()),
            Some(&mut byte_len),
        )
    };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if read_status != ERROR_SUCCESS {
        return None;
    }

    let u16s: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&w| w != 0)
        .collect();
    Some(String::from_utf16_lossy(&u16s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_filters_and_deduplicates_extensions() {
        let input = vec![
            ".DOCX".to_string(),
            "odt".to_string(),
            "  .md ".to_string(),
            "docx".to_string(),
            "exe".to_string(),
        ];
        let specs = normalize_selected_extensions(&input);
        let exts: Vec<&str> = specs.iter().map(|s| s.ext).collect();
        assert_eq!(exts, vec!["docx", "odt", "md"]);
    }

    #[test]
    fn default_selected_includes_odt_and_core_formats() {
        let defs = default_selected_extensions();
        for expected in ["wpx", "docx", "odt", "pdf", "gdoc", "md", "rtf", "txt"] {
            assert!(defs.iter().any(|e| e == expected), "missing {expected}");
        }
    }
}

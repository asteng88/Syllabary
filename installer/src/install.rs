//! Cross-platform installation, desktop/menu integration, permissions, and uninstallation
//! for Syllabary on Windows and Linux (including Fedora).

use std::fs;
use std::path::{Path, PathBuf};

/// Embedded official 256×256 Syllabary app icon installed on Linux for menu & desktop entries.
#[cfg(target_os = "linux")]
const APP_ICON_PNG: &[u8] = include_bytes!("../../app/assets/icons/syllabary_256.png");

#[derive(Clone, Debug)]
pub struct InstallOptions {
    pub install_dir: PathBuf,
    pub create_desktop_shortcut: bool,
    pub register_app_menu: bool,
    pub register_file_associations: bool,
    #[cfg_attr(not(windows), expect(dead_code, reason = "Used on Windows to update HKCU PATH"))]
    pub add_to_path: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            install_dir: default_install_dir(),
            create_desktop_shortcut: true,
            register_app_menu: true,
            register_file_associations: false,
            add_to_path: true,
        }
    }
}

/// Standard per-user installation directory (no admin/root required):
/// - Windows: `%LOCALAPPDATA%\Programs\Syllabary`
/// - Linux:   `~/.local/bin`
pub fn default_install_dir() -> PathBuf {
    #[cfg(windows)]
    {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
            .join("Programs")
            .join("Syllabary")
    }
    #[cfg(target_os = "linux")]
    {
        dirs::executable_dir().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".local")
                .join("bin")
        })
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("Applications")
            .join("Syllabary")
    }
}

pub fn binary_filename() -> &'static str {
    #[cfg(windows)]
    {
        "syllabary.exe"
    }
    #[cfg(not(windows))]
    {
        "syllabary"
    }
}

pub fn installed_binary_path(install_dir: &Path) -> PathBuf {
    install_dir.join(binary_filename())
}

pub fn is_already_installed(install_dir: &Path) -> bool {
    installed_binary_path(install_dir).exists()
}

pub fn os_display_name() -> &'static str {
    #[cfg(windows)]
    {
        "Windows (x64)"
    }
    #[cfg(target_os = "linux")]
    {
        "Linux (Fedora / XDG)"
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        "macOS / Unix"
    }
}

/// Installs the downloaded (or local) Syllabary binary from `staged_bin` into `opts.install_dir`,
/// configures permissions, app menu / Office category entry, optional desktop shortcut,
/// optional file associations, and uninstall metadata.
#[cfg_attr(not(windows), expect(unused_variables, reason = "Version is written to Windows Uninstall registry metadata"))]
pub fn perform_install<F>(
    staged_bin: &Path,
    version: &str,
    opts: &InstallOptions,
    mut log: F,
) -> Result<PathBuf, String>
where
    F: FnMut(&str),
{
    log(&format!(
        "Creating install directory: {}",
        opts.install_dir.display()
    ));
    fs::create_dir_all(&opts.install_dir)
        .map_err(|e| format!("Could not create {}: {e}", opts.install_dir.display()))?;

    let target_bin = installed_binary_path(&opts.install_dir);

    // Copy/replace executable cleanly
    log(&format!("Installing binary to {}", target_bin.display()));
    if target_bin.exists() {
        let backup = target_bin.with_extension("old");
        let _ = fs::remove_file(&backup);
        if fs::rename(&target_bin, &backup).is_ok() {
            if let Err(e) = fs::copy(staged_bin, &target_bin) {
                let _ = fs::rename(&backup, &target_bin);
                return Err(format!("Could not install binary: {e}"));
            }
            let _ = fs::remove_file(&backup);
        } else {
            fs::copy(staged_bin, &target_bin)
                .map_err(|e| format!("Could not overwrite existing binary (is Syllabary open?): {e}"))?;
        }
    } else {
        fs::copy(staged_bin, &target_bin)
            .map_err(|e| format!("Could not copy binary to {}: {e}", target_bin.display()))?;
    }

    #[cfg(target_os = "linux")]
    install_linux_integration(&target_bin, opts, &mut log)?;

    #[cfg(windows)]
    install_windows_integration(&target_bin, version, opts, &mut log)?;

    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = version;

    log("Installation complete!");
    Ok(target_bin)
}

#[cfg(target_os = "linux")]
fn install_linux_integration<F>(
    target_bin: &Path,
    opts: &InstallOptions,
    log: &mut F,
) -> Result<(), String>
where
    F: FnMut(&str),
{
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    // 1. Ensure executable permissions (chmod 0755) + SELinux context on Fedora
    log("Setting executable permissions (chmod +x)...");
    fs::set_permissions(target_bin, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("Could not set executable permission: {e}"))?;
    let _ = Command::new("restorecon").arg(target_bin).status();

    let data_dir = dirs::data_local_dir()
        .ok_or_else(|| "Could not locate ~/.local/share (XDG_DATA_HOME).".to_string())?;

    // 2. Install Syllabary icon into XDG hicolor icon theme
    log("Installing application icon...");
    let icon_dir = data_dir
        .join("icons")
        .join("hicolor")
        .join("256x256")
        .join("apps");
    if fs::create_dir_all(&icon_dir).is_ok() {
        let _ = fs::write(icon_dir.join("syllabary.png"), APP_ICON_PNG);
        let _ = Command::new("gtk-update-icon-cache")
            .args(["-f", "-t"])
            .arg(data_dir.join("icons").join("hicolor"))
            .status();
    }

    let exe_str = target_bin.to_string_lossy();
    let desktop_content = format!(
        "[Desktop Entry]\n\
         Name=Syllabary\n\
         GenericName=Word Processor\n\
         Comment=Fast, offline desktop word processor and PDF/DOCX studio\n\
         Exec=\"{exe_str}\" %F\n\
         Icon=syllabary\n\
         Terminal=false\n\
         Type=Application\n\
         Categories=Office;WordProcessor;\n\
         MimeType=application/x-syllabary;application/vnd.openxmlformats-officedocument.wordprocessingml.document;application/vnd.oasis.opendocument.text;application/pdf;application/x-syllabary-gdoc;text/markdown;application/rtf;text/plain;\n\
         StartupWMClass=Syllabary\n"
    );

    // 3. Register in Applications → Office menu
    if opts.register_app_menu {
        log("Registering Syllabary in Applications → Office menu...");
        let apps_dir = data_dir.join("applications");
        fs::create_dir_all(&apps_dir)
            .map_err(|e| format!("Could not create {}: {e}", apps_dir.display()))?;
        let desktop_path = apps_dir.join("syllabary.desktop");
        fs::write(&desktop_path, &desktop_content)
            .map_err(|e| format!("Could not write {}: {e}", desktop_path.display()))?;
        let _ = fs::set_permissions(&desktop_path, fs::Permissions::from_mode(0o644));
        let _ = Command::new("update-desktop-database").arg(&apps_dir).status();
    }

    // 4. Register custom .wpx and .gdoc MIME types
    let mime_pkgs = data_dir.join("mime").join("packages");
    if fs::create_dir_all(&mime_pkgs).is_ok() {
        let mime_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-syllabary">
    <comment>Syllabary Document</comment>
    <glob pattern="*.wpx"/>
  </mime-type>
  <mime-type type="application/x-syllabary-gdoc">
    <comment>Google Doc Link</comment>
    <glob pattern="*.gdoc"/>
  </mime-type>
</mime-info>
"#;
        let _ = fs::write(mime_pkgs.join("syllabary.xml"), mime_xml);
        let _ = Command::new("update-mime-database")
            .arg(data_dir.join("mime"))
            .status();
    }

    // 5. Optional default file associations
    if opts.register_file_associations {
        log("Registering default file associations via xdg-mime...");
        for mime in [
            "application/x-syllabary",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "application/vnd.oasis.opendocument.text",
            "application/pdf",
            "application/x-syllabary-gdoc",
            "text/markdown",
            "application/rtf",
            "text/plain",
        ] {
            let _ = Command::new("xdg-mime")
                .args(["default", "syllabary.desktop", mime])
                .status();
        }
    }

    // 6. Optional Desktop shortcut (marked executable & trusted for GNOME/Fedora)
    if opts.create_desktop_shortcut {
        if let Some(desktop_dir) = dirs::desktop_dir() {
            if desktop_dir.exists() {
                log("Creating Desktop shortcut...");
                let shortcut = desktop_dir.join("Syllabary.desktop");
                if fs::write(&shortcut, &desktop_content).is_ok() {
                    let _ = fs::set_permissions(&shortcut, fs::Permissions::from_mode(0o755));
                    // Mark launcher trusted on GNOME (Fedora Workstation)
                    let _ = Command::new("gio")
                        .args(["set", &shortcut.to_string_lossy(), "metadata::trusted", "true"])
                        .status();
                }
            }
        }
    }

    Ok(())
}

#[cfg(windows)]
fn install_windows_integration<F>(
    target_bin: &Path,
    version: &str,
    opts: &InstallOptions,
    log: &mut F,
) -> Result<(), String>
where
    F: FnMut(&str),
{
    let exe_str = target_bin.to_string_lossy();
    let dir_str = opts.install_dir.to_string_lossy();

    // 1. Optional Start Menu shortcut
    if opts.register_app_menu {
        if let Some(roaming) = dirs::config_dir() {
            let start_menu = roaming
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs");
            let _ = fs::create_dir_all(&start_menu);
            let lnk = start_menu.join("Syllabary.lnk");
            log("Creating Start Menu shortcut...");
            let _ = create_windows_shortcut(&lnk, target_bin, &opts.install_dir);
        }
    }

    // 2. Optional Desktop shortcut
    if opts.create_desktop_shortcut {
        if let Some(desktop) = dirs::desktop_dir() {
            let lnk = desktop.join("Syllabary.lnk");
            log("Creating Desktop shortcut...");
            let _ = create_windows_shortcut(&lnk, target_bin, &opts.install_dir);
        }
    }

    // 3. Register in Windows Settings → Installed Apps (HKCU Uninstall key)
    log("Registering Syllabary in Windows Installed Apps...");
    let ps_reg = format!(
        "$k='HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Syllabary'; \
         New-Item -Path $k -Force | Out-Null; \
         Set-ItemProperty -Path $k -Name DisplayName -Value 'Syllabary'; \
         Set-ItemProperty -Path $k -Name DisplayVersion -Value '{}'; \
         Set-ItemProperty -Path $k -Name Publisher -Value 'Syllabary'; \
         Set-ItemProperty -Path $k -Name DisplayIcon -Value '{}'; \
         Set-ItemProperty -Path $k -Name InstallLocation -Value '{}'; \
         Set-ItemProperty -Path $k -Name NoModify -Value 1 -Type DWord; \
         Set-ItemProperty -Path $k -Name NoRepair -Value 1 -Type DWord;",
        ps_escape(version),
        ps_escape(&exe_str),
        ps_escape(&dir_str),
    );
    let _ = run_hidden_powershell(&ps_reg);

    // 4. Optional add to user PATH
    if opts.add_to_path {
        log("Adding install folder to user PATH...");
        let ps_path = format!(
            "$d='{}'; $p=[Environment]::GetEnvironmentVariable('Path','User'); \
             if (-not $p) {{ $p='' }}; \
             $parts = $p -split ';' | Where-Object {{ $_ -ne '' }}; \
             if ($parts -notcontains $d) {{ \
               $new = ($parts + $d) -join ';'; \
               [Environment]::SetEnvironmentVariable('Path', $new, 'User') \
             }}",
            ps_escape(&dir_str)
        );
        let _ = run_hidden_powershell(&ps_path);
    }

    // 5. Optional file associations (registers HKCU ProgIDs + OpenWith)
    if opts.register_file_associations {
        log("Registering default file associations (.wpx, .docx, .odt, .pdf, .md, .rtf, .txt)...");
        let ps_assoc = format!(
            "$exe='{}'; \
             $exts = @('.wpx','.docx','.odt','.pdf','.gdoc','.md','.rtf','.txt'); \
             foreach ($e in $exts) {{ \
               $clean = $e.TrimStart('.'); \
               $prog = 'Syllabary.Document.' + $clean; \
               $pb = 'HKCU:\\Software\\Classes\\' + $prog; \
               New-Item -Path ($pb + '\\shell\\open\\command') -Force | Out-Null; \
               Set-ItemProperty -Path ($pb + '\\shell\\open\\command') -Name '(default)' -Value ('\"' + $exe + '\" \"%1\"'); \
               New-Item -Path ('HKCU:\\Software\\Classes\\' + $e + '\\OpenWithProgids') -Force | Out-Null; \
               Set-ItemProperty -Path ('HKCU:\\Software\\Classes\\' + $e) -Name '(default)' -Value $prog; \
               Set-ItemProperty -Path ('HKCU:\\Software\\Classes\\' + $e + '\\OpenWithProgids') -Name $prog -Value ''; \
             }}",
            ps_escape(&exe_str)
        );
        let _ = run_hidden_powershell(&ps_assoc);
    }

    Ok(())
}

#[cfg(windows)]
fn ps_escape(s: &str) -> String {
    s.replace('\'', "''")
}

#[cfg(windows)]
fn run_hidden_powershell(script: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("PowerShell failed: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("PowerShell exited with {status}"))
    }
}

#[cfg(windows)]
fn create_windows_shortcut(lnk: &Path, target: &Path, workdir: &Path) -> Result<(), String> {
    let script = format!(
        "$ws = New-Object -ComObject WScript.Shell; \
         $sc = $ws.CreateShortcut('{}'); \
         $sc.TargetPath = '{}'; \
         $sc.WorkingDirectory = '{}'; \
         $sc.IconLocation = '{},0'; \
         $sc.Description = 'Syllabary — Fast, offline desktop word processor'; \
         $sc.Save()",
        ps_escape(&lnk.to_string_lossy()),
        ps_escape(&target.to_string_lossy()),
        ps_escape(&workdir.to_string_lossy()),
        ps_escape(&target.to_string_lossy()),
    );
    run_hidden_powershell(&script)
}

/// Removes an existing Syllabary installation, shortcuts, menu entries, and metadata.
pub fn uninstall(install_dir: &Path) -> Result<String, String> {
    let bin = installed_binary_path(install_dir);
    if bin.exists() {
        fs::remove_file(&bin)
            .map_err(|e| format!("Could not remove {} (close Syllabary first): {e}", bin.display()))?;
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(desktop) = dirs::desktop_dir() {
            let _ = fs::remove_file(desktop.join("Syllabary.desktop"));
        }
        if let Some(data_dir) = dirs::data_local_dir() {
            let _ = fs::remove_file(data_dir.join("applications").join("syllabary.desktop"));
            let _ = fs::remove_file(
                data_dir
                    .join("icons")
                    .join("hicolor")
                    .join("256x256")
                    .join("apps")
                    .join("syllabary.png"),
            );
            let _ = fs::remove_file(data_dir.join("mime").join("packages").join("syllabary.xml"));
            let _ = std::process::Command::new("update-desktop-database")
                .arg(data_dir.join("applications"))
                .status();
        }
    }

    #[cfg(windows)]
    {
        if let Some(desktop) = dirs::desktop_dir() {
            let _ = fs::remove_file(desktop.join("Syllabary.lnk"));
        }
        if let Some(roaming) = dirs::config_dir() {
            let _ = fs::remove_file(
                roaming
                    .join("Microsoft")
                    .join("Windows")
                    .join("Start Menu")
                    .join("Programs")
                    .join("Syllabary.lnk"),
            );
        }
        let _ = run_hidden_powershell(
            "Remove-Item -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Syllabary' -Recurse -Force -ErrorAction SilentlyContinue",
        );
    }

    Ok("Syllabary has been uninstalled.".into())
}

pub fn launch_syllabary(bin_path: &Path) -> Result<(), String> {
    std::process::Command::new(bin_path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not launch {}: {e}", bin_path.display()))
}

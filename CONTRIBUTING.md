# Contributing to Syllabary

Thank you for your interest in contributing to **Syllabary**! Whether you are reporting a bug, proposing a new feature, improving documentation, or submitting a pull request, your help is welcome.

---

## Development Setup

### Prerequisites
- **Rust toolchain** (1.85+ recommended via [rustup.rs](https://rustup.rs) or your Linux distro package manager)
- **Sibling crate:** `ModernRust` checked out adjacent to `Syllabary` (`../ModernRust`)
- **Linux (Fedora) system libraries:**
  ```bash
  sudo dnf install gcc pkgconf-pkg-config libxkbcommon-devel wayland-devel \
    libX11-devel libXcursor-devel libXrandr-devel libXi-devel \
    mesa-libGL-devel fontconfig-devel gtk3-devel dbus-devel \
    cups xdg-utils hunspell-en-US
  ```
- **Linux (Ubuntu / Debian) system libraries:**
  ```bash
  sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev \
    libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
    libgl1-mesa-dev libfontconfig1-dev libgtk-3-dev libdbus-1-dev \
    cups xdg-utils hunspell-en-us
  ```

### Building & Testing
```bash
# Run the main application
cd app
cargo run --release

# Run unit tests
cargo test

# Build the cross-platform installer
cd ../installer
cargo build --release
```

---

## Cross-Platform Guidelines (Windows & Linux)

Every contribution **must** remain compatible and warning-free on **both Windows and Linux (including Fedora)**:

1. **Gate OS-specific code & crates:**
   - Windows-only code/crates go behind `#[cfg(windows)]` / `[target.'cfg(windows)'.dependencies]`.
   - Linux-only code/crates (such as `spellbook` Hunspell spell checking and XDG desktop integration) go behind `#[cfg(target_os = "linux")]` / `[target.'cfg(target_os = "linux")'.dependencies]`.
2. **Avoid hardcoded OS paths:** Use `std::path::Path` / `PathBuf` and `dirs::*` (XDG-compliant on Linux).
3. **Zero compiler warnings:** Ensure `cargo check` and `cargo clippy` pass cleanly on both Windows and Linux.
4. **Offline-first privacy:** Never add telemetry, cloud dependencies, or required network calls outside of the opt-in GitHub Releases updater and installer.

---

## Submitting a Pull Request

1. Fork the repository and create a feature branch from `master`:
   ```bash
   git checkout -b feat/your-feature-name
   ```
2. Keep commits focused, clear, and well-tested.
3. Verify both `cargo check` and `cargo test` pass.
4. Open a Pull Request using the PR template and include screenshots for any UI changes.

---

## Reporting Issues

Please use the [GitHub Issue Templates](https://github.com/asteng88/Syllabary/issues/new/choose) and include:
- Syllabary version (shown in the window titlebar or **Help → About Syllabary**)
- Operating system & version (e.g., Windows 11, Fedora 45 Workstation, Ubuntu 24.04)
- Clear reproduction steps or a minimal sample file (if format-related)

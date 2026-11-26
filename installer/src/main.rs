#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod github;
mod install;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, Vec2};

const INSTALLER_ICON_PNG: &[u8] = include_bytes!("../assets/installer_256.png");
const INSTALLER_VERSION: &str = env!("SYLLABARY_VERSION");

fn load_window_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(INSTALLER_ICON_PNG).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

enum WorkerMsg {
    ReleaseLoaded(Result<github::ReleaseInfo, String>),
    Progress { downloaded: u64, total: u64, status: String },
    Log(String),
    InstallDone(Result<PathBuf, String>),
}

#[derive(PartialEq, Eq)]
enum Stage {
    Ready,
    Installing,
    Finished(PathBuf),
    Failed(String),
}

struct InstallerApp {
    opts: install::InstallOptions,
    release: Option<github::ReleaseInfo>,
    release_error: Option<String>,
    loading_release: bool,
    local_override: Option<PathBuf>,
    stage: Stage,
    progress_frac: f32,
    progress_label: String,
    logs: Vec<String>,
    launch_after: bool,
    uninstall_msg: Option<String>,
    rx: Receiver<WorkerMsg>,
    tx: mpsc::Sender<WorkerMsg>,
    icon_tex: Option<egui::TextureHandle>,
}

impl InstallerApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(20, 21, 26);
        visuals.window_fill = Color32::from_rgb(26, 27, 34);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(35, 37, 46);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(46, 49, 62);
        visuals.selection.bg_fill = Color32::from_rgb(16, 185, 129);
        cc.egui_ctx.set_visuals(visuals);

        let icon_tex = image::load_from_memory(INSTALLER_ICON_PNG).ok().map(|img| {
            let rgba = img.into_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let color_img = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
            cc.egui_ctx
                .load_texture("installer-icon", color_img, egui::TextureOptions::LINEAR)
        });

        let (tx, rx) = mpsc::channel();
        let app = Self {
            opts: install::InstallOptions::default(),
            release: None,
            release_error: None,
            loading_release: true,
            local_override: None,
            stage: Stage::Ready,
            progress_frac: 0.0,
            progress_label: String::new(),
            logs: Vec::new(),
            launch_after: true,
            uninstall_msg: None,
            rx,
            tx,
            icon_tex,
        };
        app.fetch_release_async(cc.egui_ctx.clone());
        app
    }

    fn fetch_release_async(&self, ctx: egui::Context) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let res = github::fetch_latest_release();
            let _ = tx.send(WorkerMsg::ReleaseLoaded(res));
            ctx.request_repaint();
        });
    }

    fn start_install(&mut self, ctx: egui::Context) {
        self.stage = Stage::Installing;
        self.progress_frac = 0.0;
        self.progress_label = "Preparing installation...".into();
        self.logs.clear();
        self.uninstall_msg = None;

        let tx = self.tx.clone();
        let opts = self.opts.clone();
        let release = self.release.clone();
        let local_override = self.local_override.clone();

        thread::spawn(move || {
            let result = (|| -> Result<PathBuf, String> {
                let (staged_path, version, cleanup_temp) = if let Some(local) = local_override {
                    let _ = tx.send(WorkerMsg::Log(format!(
                        "Using local binary: {}",
                        local.display()
                    )));
                    (local, INSTALLER_VERSION.to_string(), false)
                } else {
                    let rel = release.ok_or_else(|| {
                        "No GitHub release loaded. Pick a local binary or check your internet connection."
                            .to_string()
                    })?;
                    let url = rel.asset_url.as_deref().ok_or_else(|| {
                        format!(
                            "Release {} does not yet contain a {} binary asset.\nYou can browse for a local compiled binary instead.",
                            rel.tag_name,
                            install::os_display_name()
                        )
                    })?;
                    let asset_name = rel.asset_name.as_deref().unwrap_or("syllabary");
                    let _ = tx.send(WorkerMsg::Log(format!(
                        "Downloading {asset_name} ({}) from GitHub...",
                        rel.tag_name
                    )));

                    let temp_path = std::env::temp_dir().join(format!(
                        "syllabary-download-{}-{}",
                        std::process::id(),
                        install::binary_filename()
                    ));
                    github::download_verified(
                        url,
                        &temp_path,
                        rel.asset_size,
                        rel.asset_sha256.as_deref(),
                        |dl, total| {
                            let label = if total > 0 {
                                format!(
                                    "Downloading {:.1} MB / {:.1} MB...",
                                    dl as f64 / 1_048_576.0,
                                    total as f64 / 1_048_576.0
                                )
                            } else {
                                format!("Downloading {:.1} MB...", dl as f64 / 1_048_576.0)
                            };
                            let _ = tx.send(WorkerMsg::Progress {
                                downloaded: dl,
                                total,
                                status: label,
                            });
                            ctx.request_repaint();
                        },
                    )?;
                    if rel.asset_sha256.is_some() {
                        let _ = tx.send(WorkerMsg::Log("SHA-256 checksum verified.".into()));
                    }
                    (temp_path, rel.version, true)
                };

                let _ = tx.send(WorkerMsg::Progress {
                    downloaded: 1,
                    total: 1,
                    status: "Installing files and shortcuts...".into(),
                });
                ctx.request_repaint();

                let installed = install::perform_install(&staged_path, &version, &opts, |msg| {
                    let _ = tx.send(WorkerMsg::Log(msg.to_string()));
                    ctx.request_repaint();
                });

                if cleanup_temp {
                    let _ = std::fs::remove_file(&staged_path);
                }
                installed
            })();

            let _ = tx.send(WorkerMsg::InstallDone(result));
            ctx.request_repaint();
        });
    }

    fn poll_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                WorkerMsg::ReleaseLoaded(res) => {
                    self.loading_release = false;
                    match res {
                        Ok(rel) => {
                            self.release = Some(rel);
                            self.release_error = None;
                        }
                        Err(e) => {
                            self.release = None;
                            self.release_error = Some(e);
                        }
                    }
                }
                WorkerMsg::Progress {
                    downloaded,
                    total,
                    status,
                } => {
                    self.progress_frac = if total > 0 {
                        (downloaded as f32 / total as f32).clamp(0.0, 1.0)
                    } else {
                        0.5
                    };
                    self.progress_label = status;
                }
                WorkerMsg::Log(line) => self.logs.push(line),
                WorkerMsg::InstallDone(res) => match res {
                    Ok(bin) => {
                        self.progress_frac = 1.0;
                        self.stage = Stage::Finished(bin);
                    }
                    Err(err) => {
                        self.stage = Stage::Failed(err);
                    }
                },
            }
        }
    }
}

impl eframe::App for InstallerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_messages();
        let ctx = ui.ctx().clone();

        let accent = Color32::from_rgb(16, 185, 129);
        let muted = Color32::from_rgb(156, 163, 175);
        let card_bg = Color32::from_rgb(28, 30, 38);
        let card_stroke = Stroke::new(1.0, Color32::from_rgb(48, 52, 66));

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.global_style()).inner_margin(Margin::same(22)))
            .show(ui, |ui| {
                // ── HEADER ────────────────────────────────────────────────
                ui.horizontal(|ui| {
                    if let Some(tex) = &self.icon_tex {
                        ui.add(
                            egui::Image::new((tex.id(), Vec2::splat(60.0)))
                                .corner_radius(CornerRadius::same(12)),
                        );
                        ui.add_space(12.0);
                    }
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Syllabary Installer")
                                .size(22.0)
                                .strong()
                                .color(Color32::WHITE),
                        );
                        ui.label(
                            RichText::new(format!(
                                "Fast, offline desktop word processor  •  {}",
                                install::os_display_name()
                            ))
                            .size(12.5)
                            .color(muted),
                        );
                    });
                });

                ui.add_space(14.0);

                // ── RELEASE STATUS CARD ──────────────────────────────────
                egui::Frame::new()
                    .fill(card_bg)
                    .stroke(card_stroke)
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if let Some(local) = self.local_override.clone() {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Source: Local binary")
                                        .strong()
                                        .color(accent),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button("Use GitHub instead").clicked() {
                                            self.local_override = None;
                                        }
                                    },
                                );
                            });
                            ui.label(
                                RichText::new(local.display().to_string())
                                    .size(12.0)
                                    .color(muted),
                            );
                        } else if self.loading_release {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Checking GitHub for the latest Syllabary release...");
                            });
                        } else if let Some(rel) = &self.release {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("Latest Release: {}", rel.name))
                                        .strong()
                                        .color(Color32::WHITE),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button("Use local binary…").clicked() {
                                            if let Some(p) = rfd::FileDialog::new().pick_file() {
                                                self.local_override = Some(p);
                                            }
                                        }
                                    },
                                );
                            });
                            if let Some(asset) = &rel.asset_name {
                                let mb = rel.asset_size as f64 / 1_048_576.0;
                                let verified = if rel.asset_sha256.is_some() {
                                    "  •  SHA-256 verified"
                                } else {
                                    ""
                                };
                                ui.label(
                                    RichText::new(format!("{asset}  ({mb:.1} MB){verified}"))
                                        .size(12.0)
                                        .color(accent),
                                );
                            } else {
                                ui.label(
                                    RichText::new(format!(
                                        "No prebuilt {} asset found in {}. Click \"Use local binary…\" to install a local build.",
                                        install::os_display_name(),
                                        rel.tag_name
                                    ))
                                    .size(12.0)
                                    .color(Color32::from_rgb(251, 191, 36)),
                                );
                            }
                        } else if let Some(err) = &self.release_error {
                            ui.label(
                                RichText::new(err)
                                    .size(12.0)
                                    .color(Color32::from_rgb(248, 113, 113)),
                            );
                            ui.horizontal(|ui| {
                                if ui.small_button("Retry GitHub").clicked() {
                                    self.loading_release = true;
                                    self.release_error = None;
                                    self.fetch_release_async(ctx.clone());
                                }
                                if ui.small_button("Install from local binary…").clicked() {
                                    if let Some(p) = rfd::FileDialog::new().pick_file() {
                                        self.local_override = Some(p);
                                    }
                                }
                            });
                        }
                    });

                ui.add_space(12.0);

                // ── STAGE-SPECIFIC BODY ──────────────────────────────────
                match &self.stage {
                    Stage::Ready => {
                        let already = install::is_already_installed(&self.opts.install_dir);

                        egui::Frame::new()
                            .fill(card_bg)
                            .stroke(card_stroke)
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(Margin::same(14))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new("Installation Location").strong());
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    let mut path_str =
                                        self.opts.install_dir.to_string_lossy().into_owned();
                                    let resp = ui.add(
                                        egui::TextEdit::singleline(&mut path_str)
                                            .desired_width(ui.available_width() - 90.0),
                                    );
                                    if resp.changed() {
                                        self.opts.install_dir = PathBuf::from(path_str);
                                    }
                                    if ui.button("Browse…").clicked() {
                                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                            self.opts.install_dir = dir;
                                        }
                                    }
                                });

                                ui.add_space(10.0);
                                ui.label(RichText::new("Options").strong());
                                ui.add_space(4.0);

                                ui.checkbox(
                                    &mut self.opts.create_desktop_shortcut,
                                    "Create a shortcut on the Desktop",
                                );

                                #[cfg(target_os = "linux")]
                                ui.checkbox(
                                    &mut self.opts.register_app_menu,
                                    "Add Syllabary to Applications → Office menu (with icon & permissions)",
                                );
                                #[cfg(windows)]
                                ui.checkbox(
                                    &mut self.opts.register_app_menu,
                                    "Add Syllabary to the Start Menu & Installed Apps",
                                );

                                ui.checkbox(
                                    &mut self.opts.register_file_associations,
                                    "Register as default app for .wpx, .docx, .odt, .pdf, .md, .rtf, .txt",
                                );

                                #[cfg(windows)]
                                ui.checkbox(
                                    &mut self.opts.add_to_path,
                                    "Add install folder to user PATH",
                                );
                            });

                        if let Some(msg) = &self.uninstall_msg {
                            ui.add_space(8.0);
                            ui.label(RichText::new(msg).size(12.0).color(accent));
                        }

                        ui.add_space(16.0);

                        let can_install = self.local_override.is_some()
                            || self
                                .release
                                .as_ref()
                                .and_then(|r| r.asset_url.as_ref())
                                .is_some();

                        ui.horizontal(|ui| {
                            let label = if already {
                                "Update / Reinstall Syllabary"
                            } else {
                                "Install Syllabary"
                            };
                            let btn = egui::Button::new(
                                RichText::new(label)
                                    .size(14.0)
                                    .strong()
                                    .color(Color32::from_rgb(10, 15, 13)),
                            )
                            .fill(accent)
                            .corner_radius(CornerRadius::same(8))
                            .min_size(Vec2::new(210.0, 38.0));

                            if ui.add_enabled(can_install, btn).clicked() {
                                self.start_install(ctx.clone());
                            }

                            if already {
                                ui.add_space(8.0);
                                if ui
                                    .add(
                                        egui::Button::new("Uninstall")
                                            .min_size(Vec2::new(96.0, 38.0)),
                                    )
                                    .clicked()
                                {
                                    match install::uninstall(&self.opts.install_dir) {
                                        Ok(m) => self.uninstall_msg = Some(m),
                                        Err(e) => self.uninstall_msg = Some(e),
                                    }
                                }
                            }
                        });
                    }

                    Stage::Installing => {
                        ui.label(RichText::new(&self.progress_label).strong());
                        ui.add_space(6.0);
                        ui.add(
                            egui::ProgressBar::new(self.progress_frac)
                                .desired_height(14.0)
                                .show_percentage(),
                        );
                        ui.add_space(10.0);
                        egui::ScrollArea::vertical()
                            .max_height(150.0)
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                for line in &self.logs {
                                    ui.label(RichText::new(line).size(12.0).color(muted));
                                }
                            });
                    }

                    Stage::Finished(bin_path) => {
                        let bin_path = bin_path.clone();
                        egui::Frame::new()
                            .fill(card_bg)
                            .stroke(Stroke::new(1.0, accent))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(Margin::same(14))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(
                                    RichText::new("Syllabary is installed and ready to use!")
                                        .size(15.0)
                                        .strong()
                                        .color(accent),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(format!("Installed to: {}", bin_path.display()))
                                        .size(12.0)
                                        .color(muted),
                                );
                                ui.add_space(8.0);
                                ui.checkbox(
                                    &mut self.launch_after,
                                    "Launch Syllabary when closing the installer",
                                );
                            });

                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            let finish_btn = egui::Button::new(
                                RichText::new("Finish")
                                    .size(14.0)
                                    .strong()
                                    .color(Color32::from_rgb(10, 15, 13)),
                            )
                            .fill(accent)
                            .corner_radius(CornerRadius::same(8))
                            .min_size(Vec2::new(130.0, 36.0));
                            if ui.add(finish_btn).clicked() {
                                if self.launch_after {
                                    let _ = install::launch_syllabary(&bin_path);
                                }
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                    }

                    Stage::Failed(err) => {
                        let err = err.clone();
                        ui.label(
                            RichText::new("Installation Failed")
                                .size(15.0)
                                .strong()
                                .color(Color32::from_rgb(248, 113, 113)),
                        );
                        ui.add_space(4.0);
                        ui.label(RichText::new(err).size(12.5));
                        ui.add_space(12.0);
                        if ui.button("Back").clicked() {
                            self.stage = Stage::Ready;
                        }
                    }
                }
            });
    }
}

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("Syllabary Installer v{INSTALLER_VERSION}"))
        .with_inner_size([560.0, 470.0])
        .with_resizable(false);
    if let Some(icon) = load_window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Syllabary Installer",
        options,
        Box::new(|cc| Ok(Box::new(InstallerApp::new(cc)))),
    )
}

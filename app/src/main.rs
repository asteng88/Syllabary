#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod chrome;
mod dialogs;
mod doc;
mod docx;
mod pdf;
mod print;
mod spell;
mod editor;
mod fonts;
mod icons;
mod layout;
mod scrollbar;
mod theme;

use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let mut screenshot: Option<PathBuf> = None;
    let mut open: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--screenshot" {
            screenshot = args.next().map(PathBuf::from);
        } else if !a.starts_with("--") {
            open.push(PathBuf::from(a));
        }
    }

    // Same proportions as the original's maximised window (1899 × 1132).
    let size = if screenshot.is_some() { [1600.0, 954.0] } else { [1440.0, 900.0] };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Scribe Pro - [Untitled1]")
            .with_inner_size(size)
            .with_maximized(true)
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Scribe Pro",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, screenshot, open)))),
    )
}

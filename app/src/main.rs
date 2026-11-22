#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod chrome;
mod dialogs;
mod doc;
mod docx;
mod pdf;
mod pdf_import;
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
    let mut batch_manifest: Option<PathBuf> = None;
    let mut open: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--screenshot" {
            screenshot = args.next().map(PathBuf::from);
        } else if a == "--render-pdf-batch" {
            batch_manifest = args.next().map(PathBuf::from);
        } else if !a.starts_with("--") {
            open.push(PathBuf::from(a));
        }
    }

    if let Some(manifest_path) = batch_manifest {
        run_headless_pdf_batch(&manifest_path);
        return Ok(());
    }

    // Same proportions as the original's maximised window (1899 × 1132).
    let size = if screenshot.is_some() { [1600.0, 954.0] } else { [1440.0, 900.0] };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Syllabary - [Untitled1]")
            .with_inner_size(size)
            .with_maximized(true)
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Syllabary",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, screenshot, open)))),
    )
}

fn run_headless_pdf_batch(manifest_path: &std::path::Path) {
    let Ok(manifest_text) = std::fs::read_to_string(manifest_path) else {
        eprintln!("Failed to read batch manifest: {}", manifest_path.display());
        std::process::exit(1);
    };
    let ctx = egui::Context::default();
    let book = fonts::install(&ctx);

    for raw_line in manifest_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 {
            continue;
        }
        let pdf_path = PathBuf::from(parts[0]);
        let out_dir = PathBuf::from(parts[1]);
        let dpi: f32 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(150.0);
        let _ = std::fs::create_dir_all(&out_dir);

        let Ok(bytes) = std::fs::read(&pdf_path) else {
            let _ = std::fs::write(out_dir.join("error.txt"), "Failed to read PDF bytes");
            continue;
        };
        match pdf_import::read(&bytes) {
            Ok(doc) => {
                let mut pages_rgba = Vec::new();
                let mut out = ctx.run_ui(Default::default(), |_ui| {
                    pages_rgba = pdf::render_pages_rgba(&ctx, &doc, &book, dpi / layout::BASE_PPI);
                });
                out.textures_delta.clear();

                for (pi, (w, h, rgba)) in pages_rgba.iter().enumerate() {
                    if let Some(img) = image::RgbaImage::from_raw(*w, *h, rgba.clone()) {
                        let _ = img.save(out_dir.join(format!("page_{:03}.png", pi + 1)));
                    }
                }
                let (words, chars) = doc.counts(None);
                let body_txt = doc.plain_text();
                let hf_txt = doc.header_footer_plain_text();
                let full_txt = if hf_txt.is_empty() {
                    body_txt
                } else if body_txt.is_empty() {
                    hf_txt
                } else {
                    format!("{body_txt}\n{hf_txt}")
                };
                let meta = serde_json::json!({
                    "ok": true,
                    "pages": pages_rgba.len(),
                    "words": words,
                    "chars": chars,
                    "tables": doc.tables.len(),
                    "images": doc.images.len(),
                    "paras": doc.paras.len(),
                    "footer": doc.footer,
                    "page_footers": doc.page_footers.len(),
                    "page_width_in": doc.page.width,
                    "page_height_in": doc.page.height,
                    "plain_text": full_txt,
                });
                let _ = std::fs::write(out_dir.join("meta.json"), serde_json::to_string_pretty(&meta).unwrap_or_default());
            }
            Err(err) => {
                let meta = serde_json::json!({ "ok": false, "error": err });
                let _ = std::fs::write(out_dir.join("meta.json"), serde_json::to_string_pretty(&meta).unwrap_or_default());
            }
        }
    }
}

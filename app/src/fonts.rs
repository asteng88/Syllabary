//! Loads the computer's own installed fonts (no downloads) so documents can use
//! real Times New Roman, Arial, etc. with true bold and italic faces.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::collections::HashMap;
use std::sync::Arc;

use crate::doc::{bundled_folder, FONT_NAMES};
use std::path::PathBuf;

/// The fonts/ folder shipped next to the program (or found above it while developing).
pub fn bundled_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?.to_path_buf();
    for _ in 0..5 {
        let f = dir.join("fonts");
        if f.join("README.md").exists() || f.join("lora").exists() {
            return Some(f);
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

/// Files for (regular, bold, italic, bold-italic).
fn files_for(name: &str) -> [&'static str; 4] {
    match name {
        "Times New Roman" => ["times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf"],
        "Arial" => ["arial.ttf", "arialbd.ttf", "ariali.ttf", "arialbi.ttf"],
        "Calibri" => ["calibri.ttf", "calibrib.ttf", "calibrii.ttf", "calibriz.ttf"],
        "Cambria" => ["cambria.ttc", "cambriab.ttf", "cambriai.ttf", "cambriaz.ttf"],
        "Georgia" => ["georgia.ttf", "georgiab.ttf", "georgiai.ttf", "georgiaz.ttf"],
        "Garamond" => ["GARA.TTF", "GARABD.TTF", "GARAIT.TTF", ""],
        "Bookman Old Style" => ["BOOKOS.TTF", "BOOKOSB.TTF", "BOOKOSI.TTF", "BOOKOSBI.TTF"],
        "Palatino Linotype" => ["pala.ttf", "palab.ttf", "palai.ttf", "palabi.ttf"],
        "Segoe UI" => ["segoeui.ttf", "segoeuib.ttf", "segoeuii.ttf", "segoeuiz.ttf"],
        "Tahoma" => ["tahoma.ttf", "tahomabd.ttf", "", ""],
        "Trebuchet MS" => ["trebuc.ttf", "trebucbd.ttf", "trebucit.ttf", "trebucbi.ttf"],
        "Verdana" => ["verdana.ttf", "verdanab.ttf", "verdanai.ttf", "verdanaz.ttf"],
        "Courier New" => ["cour.ttf", "courbd.ttf", "couri.ttf", "courbi.ttf"],
        "Consolas" => ["consola.ttf", "consolab.ttf", "consolai.ttf", "consolaz.ttf"],
        _ => ["", "", "", ""],
    }
}

/// Which faces actually exist, so the layout knows when to fake italics.
#[derive(Default, Clone)]
pub struct FontBook {
    /// key: (font index, variant 0..4) → egui family name, or None when missing.
    pub faces: HashMap<(u8, u8), Option<String>>,
    pub ui_loaded: bool,
}

impl FontBook {
    /// Returns the family to use and whether italics must be synthesised.
    pub fn family(&self, font: u8, bold: bool, italic: bool) -> (FontFamily, bool) {
        let v = (bold as u8) | ((italic as u8) << 1);
        if let Some(Some(name)) = self.faces.get(&(font, v)) {
            return (FontFamily::Name(name.clone().into()), false);
        }
        // try without italic (fake it), then without bold
        if italic {
            if let Some(Some(name)) = self.faces.get(&(font, bold as u8)) {
                return (FontFamily::Name(name.clone().into()), true);
            }
        }
        if let Some(Some(name)) = self.faces.get(&(font, 0)) {
            return (FontFamily::Name(name.clone().into()), italic);
        }
        if font != 0 {
            return self.family(0, bold, italic);
        }
        (FontFamily::Proportional, italic)
    }
}

fn font_dir() -> std::path::PathBuf {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    std::path::Path::new(&windir).join("Fonts")
}

fn strip_ebdt_at(data: &mut [u8], dir_off: usize) {
    let Some(nt_bytes) = data.get(dir_off + 4..dir_off + 6) else { return };
    let num_tables = u16::from_be_bytes([nt_bytes[0], nt_bytes[1]]) as usize;
    for i in 0..num_tables {
        let rec = dir_off + 12 + i * 16;
        let Some(tag) = data.get(rec..rec + 4) else { break };
        if matches!(tag, b"EBDT" | b"EBLC" | b"EBSC") {
            data[rec..rec + 4].copy_from_slice(b"xxxx");
        }
    }
}

/// Strip legacy monochrome embedded-bitmap tables (EBDT/EBLC/EBSC) so epaint's
/// `color_fonts` feature does not mistake them for color emoji bitmaps and paint
/// them in untinted white on a white page.
fn strip_ebdt(mut data: Vec<u8>) -> Vec<u8> {
    if data.starts_with(b"ttcf") && data.len() >= 12 {
        let num_fonts = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize;
        for f in 0..num_fonts.min(64) {
            let off_pos = 12 + f * 4;
            let Some(ob) = data.get(off_pos..off_pos + 4) else { break };
            let dir_off = u32::from_be_bytes([ob[0], ob[1], ob[2], ob[3]]) as usize;
            strip_ebdt_at(&mut data, dir_off);
        }
    } else {
        strip_ebdt_at(&mut data, 0);
    }
    data
}

pub fn install(ctx: &egui::Context) -> FontBook {
    let mut defs = FontDefinitions::default();
    let mut book = FontBook::default();
    let dir = font_dir();

    let load = |file: &str| -> Option<FontData> {
        if file.is_empty() {
            return None;
        }
        std::fs::read(dir.join(file)).ok().map(strip_ebdt).map(FontData::from_owned)
    };
    let bundled = bundled_dir();
    let load_face = |fi: usize, v: usize| -> Option<FontData> {
        if let Some(folder) = bundled_folder(fi as u8) {
            let file = ["regular.ttf", "bold.ttf", "italic.ttf", "bolditalic.ttf"][v];
            return std::fs::read(bundled.as_ref()?.join(folder).join(file))
                .ok()
                .map(strip_ebdt)
                .map(FontData::from_owned);
        }
        load(files_for(FONT_NAMES[fi])[v])
    };

    // Symbol fallback for bullets, arrows and other special characters.
    let mut fallbacks: Vec<String> = Vec::new();
    for (key, file) in [("sym:segoe-symbol", "seguisym.ttf"), ("sym:segoe-ui", "segoeui.ttf")] {
        if let Some(d) = load(file) {
            defs.font_data.insert(key.into(), Arc::new(d));
            fallbacks.push(key.into());
        }
    }
    let egui_defaults: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();

    for (fi, name) in FONT_NAMES.iter().enumerate() {
        for v in 0..4u8 {
            let key = format!("{name}#{v}");
            match load_face(fi, v as usize) {
                Some(data) => {
                    defs.font_data.insert(key.clone(), Arc::new(data));
                    let mut chain = vec![key.clone()];
                    chain.extend(fallbacks.iter().cloned());
                    chain.extend(egui_defaults.iter().cloned());
                    defs.families.insert(FontFamily::Name(key.clone().into()), chain);
                    book.faces.insert((fi as u8, v), Some(key));
                }
                None => {
                    book.faces.insert((fi as u8, v), None);
                }
            }
        }
    }

    // Modern UI font: Segoe UI when present.
    if let Some(d) = load("segoeui.ttf") {
        defs.font_data.insert("ui:segoe".into(), Arc::new(d));
        if let Some(list) = defs.families.get_mut(&FontFamily::Proportional) {
            list.insert(0, "ui:segoe".into());
        }
        book.ui_loaded = true;
    }
    if let Some(d) = load("segoeuib.ttf") {
        defs.font_data.insert("ui:segoe-bold".into(), Arc::new(d));
        let mut chain = vec!["ui:segoe-bold".to_string()];
        chain.extend(egui_defaults.iter().cloned());
        defs.families.insert(FontFamily::Name("ui-bold".into()), chain);
    } else {
        defs.families.insert(FontFamily::Name("ui-bold".into()), egui_defaults.clone());
    }
    for f in &fallbacks {
        if let Some(list) = defs.families.get_mut(&FontFamily::Proportional) {
            list.push(f.clone());
        }
    }
    ctx.set_fonts(defs);
    book
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_font_renders_white_ebdt_bitmap_glyphs() {
        let ctx = egui::Context::default();
        let mut book = FontBook::default();
        let sample = "After The Prayer, The Circuit Overseer Slowly Stood Up 0123456789";
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            book = install(&ctx);
        });
        out.textures_delta.clear();
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            ctx.fonts_mut(|fonts| {
                for (fi, name) in FONT_NAMES.iter().enumerate() {
                    for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                        let (fam, _) = book.family(fi as u8, bold, italic);
                        for pt in [9.0, 10.0, 11.0, 12.0, 14.0, 16.0] {
                            let mut job = egui::text::LayoutJob::default();
                            job.append(
                                sample,
                                0.0,
                                egui::TextFormat {
                                    font_id: egui::FontId::new(pt * 96.0 / 72.0, fam.clone()),
                                    color: egui::Color32::BLACK,
                                    ..Default::default()
                                },
                            );
                            let g = fonts.layout_job(job);
                            let row = &g.rows[0].row;
                            let color_gl = row
                                .glyphs
                                .iter()
                                .filter(|gl| !gl.chr.is_whitespace() && gl.is_color)
                                .map(|gl| gl.chr)
                                .collect::<String>();
                            assert!(
                                color_gl.is_empty(),
                                "Font {fi} ({name}, bold={bold}, italic={italic}, {pt}pt) produced untinted color-bitmap glyphs: {color_gl:?}"
                            );
                        }
                    }
                }
            });
        });
        out.textures_delta.clear();
    }
}

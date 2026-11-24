//! Loads the computer's own installed fonts (no downloads) so documents can use
//! real Times New Roman, Arial, Aptos, etc. with true bold and italic faces.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::doc::{bundled_folder, FontCat, DEFAULT_FONT, FONT_NAMES};

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

/// Files for (regular, bold, italic, bold-italic) of the 14 core Windows fonts.
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

fn base_font_category(i: u16) -> FontCat {
    match i {
        0 | 3 | 4 | 5 | 6 | 7 | 14..=19 => FontCat::Serif,
        12 | 13 | 27 | 28 => FontCat::Mono,
        29 | 30 => FontCat::Script,
        _ => FontCat::Sans,
    }
}

pub struct CatalogEntry {
    pub name: &'static str,
    pub cat: FontCat,
    pub files: [Option<PathBuf>; 4],
    data: [OnceLock<Option<Arc<FontData>>>; 4],
    raw: [OnceLock<Option<Arc<Vec<u8>>>>; 4],
    metrics: [OnceLock<Option<(f32, f32)>>; 4],
}

impl CatalogEntry {
    fn load_raw(&self, v: usize) -> Option<Arc<Vec<u8>>> {
        self.raw[v]
            .get_or_init(|| {
                let path = self.files[v].as_ref()?;
                let bytes = std::fs::read(path).ok()?;
                Some(Arc::new(bytes))
            })
            .clone()
    }

    fn load_face(&self, v: usize) -> Option<Arc<FontData>> {
        self.data[v]
            .get_or_init(|| {
                let raw = self.load_raw(v)?;
                Some(Arc::new(FontData::from_owned(strip_ebdt((*raw).clone()))))
            })
            .clone()
    }

    fn load_word_metrics(&self, v: usize) -> Option<(f32, f32)> {
        *self.metrics[v].get_or_init(|| {
            let raw = self.load_raw(v)?;
            parse_word_line_metrics(&raw)
        })
    }
}

/// Parses Word-accurate single-line vertical metrics `(ascent_em, descent_em)` from raw
/// TrueType / OpenType / TTC font bytes, following Microsoft Word & WordCraft's rule:
/// `hhea` `(ascender + lineGap, |descender|)` with `lineGap` placed above ascent, falling
/// back to `OS/2` `(usWinAscent, usWinDescent)` when `hhea` is missing or out of range.
pub fn parse_word_line_metrics(data: &[u8]) -> Option<(f32, f32)> {
    let dir_off = if data.starts_with(b"ttcf") && data.len() >= 16 {
        u32_be(data, 12) as usize
    } else if data.len() >= 12 && matches!(&data[0..4], [0, 1, 0, 0] | b"OTTO" | b"true") {
        0
    } else {
        return None;
    };
    if dir_off + 12 > data.len() {
        return None;
    }
    let num_tables = u16_be(data, dir_off + 4) as usize;
    if num_tables == 0 || num_tables > 256 || dir_off + 12 + num_tables * 16 > data.len() {
        return None;
    }
    let mut head_loc = (0usize, 0usize);
    let mut hhea_loc = (0usize, 0usize);
    let mut os2_loc = (0usize, 0usize);
    for i in 0..num_tables {
        let r = dir_off + 12 + i * 16;
        let tag = &data[r..r + 4];
        let off = u32_be(data, r + 8) as usize;
        let len = u32_be(data, r + 12) as usize;
        if off.checked_add(len).is_none_or(|end| end > data.len()) {
            continue;
        }
        match tag {
            b"head" => head_loc = (off, len),
            b"hhea" => hhea_loc = (off, len),
            b"OS/2" => os2_loc = (off, len),
            _ => {}
        }
    }
    if head_loc.1 < 20 {
        return None;
    }
    let upem = u16_be(data, head_loc.0 + 18) as f32;
    if upem <= 0.0 {
        return None;
    }
    let ok = |a: f32, d: f32| a > 0.0 && a + d <= upem * 2.0;
    if hhea_loc.1 >= 10 {
        let asc = (i16_be(data, hhea_loc.0 + 4) as f32).max(0.0);
        let desc = (-(i16_be(data, hhea_loc.0 + 6) as f32)).max(0.0);
        let gap = (i16_be(data, hhea_loc.0 + 8) as f32).max(0.0);
        let a = asc + gap;
        if ok(a, desc) {
            return Some((a / upem, desc / upem));
        }
    }
    if os2_loc.1 >= 78 {
        let a = u16_be(data, os2_loc.0 + 74) as f32;
        let d = u16_be(data, os2_loc.0 + 76) as f32;
        if ok(a, d) {
            return Some((a / upem, d / upem));
        }
    }
    None
}

/// Returns Word-accurate `(ascent_em, descent_em)` line metrics for `(font, bold, italic)`,
/// falling back to regular face if the requested variant is unavailable.
pub fn word_line_metrics(font: u16, bold: bool, italic: bool) -> Option<(f32, f32)> {
    let entry = catalog().get(font as usize)?;
    let v = (bold as usize) | ((italic as usize) << 1);
    entry
        .load_word_metrics(v)
        .or_else(|| if italic { entry.load_word_metrics(bold as usize) } else { None })
        .or_else(|| entry.load_word_metrics(0))
}

/// Returns the raw TTF/OTF/TTC font bytes for `(font, bold, italic)` along with `(synth_bold, synth_italic)`.
/// Used by the vector PDF exporter (`krilla`) to embed real subsetted font faces.
pub fn font_file_bytes(font: u16, bold: bool, italic: bool) -> Option<(Arc<Vec<u8>>, bool, bool)> {
    let cat = catalog();
    let entry = cat.get(font as usize).or_else(|| cat.get(DEFAULT_FONT as usize))?;
    let v = (bold as usize) | ((italic as usize) << 1);
    if let Some(b) = entry.load_raw(v) {
        return Some((b, false, false));
    }
    if italic {
        if let Some(b) = entry.load_raw(bold as usize) {
            return Some((b, false, true));
        }
    }
    if bold {
        if let Some(b) = entry.load_raw((italic as usize) << 1) {
            return Some((b, true, false));
        }
    }
    if let Some(b) = entry.load_raw(0) {
        return Some((b, bold, italic));
    }
    if font != DEFAULT_FONT {
        return font_file_bytes(DEFAULT_FONT, bold, italic);
    }
    None
}

fn font_dir() -> PathBuf {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    Path::new(&windir).join("Fonts")
}

fn system_font_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![font_dir()];
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let lp = Path::new(&local);
        dirs.push(lp.join("Microsoft\\Windows\\Fonts"));
        dirs.push(lp.join("Microsoft\\FontCache\\4\\CloudFonts"));
    }
    for p in [
        "/System/Library/Fonts",
        "/System/Library/Fonts/Supplemental",
        "/Library/Fonts",
        "/usr/share/fonts",
        "/usr/local/share/fonts",
    ] {
        let pb = PathBuf::from(p);
        if pb.exists() {
            dirs.push(pb);
        }
    }
    dirs
}

fn collect_font_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 3 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_font_files(&path, depth + 1, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc") {
                out.push(path);
            }
        }
    }
}

fn u16_be(b: &[u8], o: usize) -> u16 {
    b.get(o..o + 2).map_or(0, |s| u16::from_be_bytes([s[0], s[1]]))
}

fn i16_be(b: &[u8], o: usize) -> i16 {
    b.get(o..o + 2).map_or(0, |s| i16::from_be_bytes([s[0], s[1]]))
}

fn u32_be(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn cmap_sub_has_glyph(cmap: &[u8], sub_off: usize, cp: u32) -> bool {
    if sub_off + 4 > cmap.len() {
        return false;
    }
    let fmt = u16_be(cmap, sub_off);
    if fmt == 4 {
        if cp > 0xFFFF || sub_off + 14 > cmap.len() {
            return false;
        }
        let cp16 = cp as u16;
        let seg_count_x2 = u16_be(cmap, sub_off + 6) as usize;
        let seg_count = seg_count_x2 / 2;
        let end_off = sub_off + 14;
        let start_off = end_off + seg_count_x2 + 2;
        let delta_off = start_off + seg_count_x2;
        let range_off = delta_off + seg_count_x2;
        if range_off + seg_count_x2 > cmap.len() {
            return false;
        }
        for i in 0..seg_count {
            let end = u16_be(cmap, end_off + i * 2);
            if end >= cp16 {
                let start = u16_be(cmap, start_off + i * 2);
                if start > cp16 {
                    return false;
                }
                let delta = i16_be(cmap, delta_off + i * 2);
                let ro = u16_be(cmap, range_off + i * 2) as usize;
                if ro == 0 {
                    return cp16.wrapping_add(delta as u16) != 0;
                }
                let glyph_off = range_off + i * 2 + ro + (cp16 - start) as usize * 2;
                let gid = u16_be(cmap, glyph_off);
                if gid == 0 {
                    return false;
                }
                return gid.wrapping_add(delta as u16) != 0;
            }
        }
        false
    } else if fmt == 12 {
        if sub_off + 16 > cmap.len() {
            return false;
        }
        let n_groups = (u32_be(cmap, sub_off + 12) as usize).min(100_000);
        for i in 0..n_groups {
            let g = sub_off + 16 + i * 12;
            if g + 12 > cmap.len() {
                break;
            }
            let s = u32_be(cmap, g);
            let e = u32_be(cmap, g + 4);
            let gid = u32_be(cmap, g + 8);
            if cp >= s && cp <= e {
                return gid.wrapping_add(cp - s) != 0;
            }
        }
        false
    } else {
        false
    }
}

fn classify_font(family: &str, fixed_pitch: bool, panose: [u8; 10]) -> FontCat {
    let l = family.to_ascii_lowercase();
    if fixed_pitch
        || panose[3] == 9
        || l.contains("mono")
        || l.contains("console")
        || l.contains("code")
        || l.contains("courier")
        || l.contains("typewriter")
        || l.starts_with("ocr ")
    {
        return FontCat::Mono;
    }
    if panose[0] == 3
        || l.contains("script")
        || l.contains("hand")
        || l.contains("brush")
        || l.contains("callig")
        || l.contains("corsiva")
        || l.contains("chiller")
        || l.contains("curlz")
        || l.contains("forte")
        || l.contains("mistral")
        || l.contains("papyrus")
        || l.contains("parchment")
        || l.contains("pristina")
        || l.contains("rage")
        || l.contains("vivaldi")
        || l.contains("marker")
        || l.contains("cookie")
        || l.contains("courgette")
        || l.contains("yellowtail")
        || l.contains("playball")
        || l.contains("sketch")
    {
        return FontCat::Script;
    }
    if l.contains("sans")
        || l.contains("gothic")
        || l.contains("arial")
        || l.contains("calibri")
        || l.contains("segoe")
        || l.contains("tahoma")
        || l.contains("verdana")
        || l.contains("trebuchet")
        || l.contains("aptos")
        || l.contains("corbel")
        || l.contains("candara")
        || l.contains("bahnschrift")
        || l.contains("dubai")
        || l.contains("ebrima")
        || l.contains("eras ")
        || l.contains("franklin")
        || l.contains("futura")
        || l.contains("gill")
        || l.contains("helvetica")
        || l.contains("impact")
        || l.contains("nirmala")
        || l.contains("quicksand")
        || l.contains("roboto")
        || l.contains("tw cen")
        || l.contains("agency")
        || l.contains("orbitron")
        || l.contains("russo")
        || l.contains("teko")
    {
        return FontCat::Sans;
    }
    if l.contains("serif")
        || l.contains("roman")
        || l.contains("garamond")
        || l.contains("georgia")
        || l.contains("cambria")
        || l.contains("palatino")
        || l.contains("book")
        || l.contains("antiqua")
        || l.contains("baskerville")
        || l.contains("bodoni")
        || l.contains("californian")
        || l.contains("calisto")
        || l.contains("centaur")
        || l.contains("century")
        || l.contains("colonna")
        || l.contains("constantia")
        || l.contains("elephant")
        || l.contains("engravers")
        || l.contains("felix")
        || l.contains("footlight")
        || l.contains("goudy")
        || l.contains("high tower")
        || l.contains("lucida bright")
        || l.contains("lucida fax")
        || l.contains("mincho")
        || l.contains("modern")
        || l.contains("perpetua")
        || l.contains("rockwell")
        || l.contains("sitka")
        || l.contains("sylfaen")
        || l.contains("cinzel")
        || l.contains("marcellus")
    {
        return FontCat::Serif;
    }
    if panose[0] == 2 {
        if (11..=15).contains(&panose[1]) {
            return FontCat::Sans;
        }
        if (2..=10).contains(&panose[1]) {
            return FontCat::Serif;
        }
    }
    FontCat::Sans
}

fn inspect_font_file(path: &Path) -> Option<(String, usize, FontCat)> {
    let meta = std::fs::metadata(path).ok()?;
    let file_len = meta.len();
    if file_len < 64 || file_len > 4_500_000 {
        return None;
    }
    let mut f = File::open(path).ok()?;
    let mut hdr = [0u8; 16];
    f.read_exact(&mut hdr).ok()?;
    let dir_off = if &hdr[0..4] == b"ttcf" {
        u32_be(&hdr, 12) as u64
    } else if matches!(&hdr[0..4], [0, 1, 0, 0] | b"OTTO" | b"true") {
        0
    } else {
        return None;
    };
    if dir_off + 12 > file_len {
        return None;
    }
    f.seek(SeekFrom::Start(dir_off + 4)).ok()?;
    let mut thdr = [0u8; 8];
    f.read_exact(&mut thdr).ok()?;
    let num_tables = u16_be(&thdr, 0) as usize;
    if num_tables == 0 || num_tables > 128 || dir_off + 12 + (num_tables as u64) * 16 > file_len {
        return None;
    }
    let mut recs = vec![0u8; num_tables * 16];
    f.read_exact(&mut recs).ok()?;

    let mut name_loc = (0u64, 0usize);
    let mut cmap_loc = (0u64, 0usize);
    let mut os2_loc = (0u64, 0usize);
    let mut post_loc = (0u64, 0usize);
    for i in 0..num_tables {
        let r = i * 16;
        let tag = &recs[r..r + 4];
        let off = u32_be(&recs, r + 8) as u64;
        let len = u32_be(&recs, r + 12) as usize;
        match tag {
            b"CBDT" | b"CBLC" | b"sbix" | b"SVG " | b"COLR" => return None,
            b"name" => name_loc = (off, len),
            b"cmap" => cmap_loc = (off, len),
            b"OS/2" => os2_loc = (off, len),
            b"post" => post_loc = (off, len),
            _ => {}
        }
    }
    if name_loc.0 == 0 || name_loc.1 < 6 || name_loc.0 + name_loc.1 as u64 > file_len {
        return None;
    }
    if cmap_loc.0 == 0 || cmap_loc.1 < 4 || cmap_loc.1 > 262_144 || cmap_loc.0 + cmap_loc.1 as u64 > file_len {
        return None;
    }

    // Verify Unicode cmap contains Latin 'A' and 'a' and is not a Symbol-encoded font.
    let mut cmap = vec![0u8; cmap_loc.1];
    f.seek(SeekFrom::Start(cmap_loc.0)).ok()?;
    f.read_exact(&mut cmap).ok()?;
    let c_num = u16_be(&cmap, 2) as usize;
    let mut has_ascii = false;
    for i in 0..c_num {
        let r = 4 + i * 8;
        if r + 8 > cmap.len() {
            break;
        }
        let pid = u16_be(&cmap, r);
        let eid = u16_be(&cmap, r + 2);
        let sub_off = u32_be(&cmap, r + 4) as usize;
        if pid == 3 && eid == 0 {
            return None;
        }
        if (pid == 3 && (eid == 1 || eid == 10)) || pid == 0 {
            if cmap_sub_has_glyph(&cmap, sub_off, 0x41) && cmap_sub_has_glyph(&cmap, sub_off, 0x61) {
                has_ascii = true;
            }
        }
    }
    if !has_ascii {
        return None;
    }

    // Read family (nameID 1) and subfamily (nameID 2).
    let name_read = name_loc.1.min(65_536);
    let mut name_buf = vec![0u8; name_read];
    f.seek(SeekFrom::Start(name_loc.0)).ok()?;
    f.read_exact(&mut name_buf).ok()?;
    let count = u16_be(&name_buf, 2) as usize;
    let str_off = u16_be(&name_buf, 4) as usize;
    let mut fam_en = String::new();
    let mut sub_en = String::new();
    let mut fam_any = String::new();
    let mut sub_any = String::new();
    for i in 0..count {
        let r = 6 + i * 12;
        if r + 12 > name_buf.len() {
            break;
        }
        let pid = u16_be(&name_buf, r);
        let eid = u16_be(&name_buf, r + 2);
        let lid = u16_be(&name_buf, r + 4);
        let nid = u16_be(&name_buf, r + 6);
        let len = u16_be(&name_buf, r + 8) as usize;
        let soff = str_off + u16_be(&name_buf, r + 10) as usize;
        if !matches!(nid, 1 | 2) || soff + len > name_buf.len() {
            continue;
        }
        let raw = &name_buf[soff..soff + len];
        if pid == 3 && eid == 1 && len % 2 == 0 {
            let mut s = String::with_capacity(len / 2);
            for ch in raw.chunks_exact(2) {
                if let Some(c) = char::from_u32(u16::from_be_bytes([ch[0], ch[1]]) as u32) {
                    s.push(c);
                }
            }
            let s = s.trim().to_string();
            if !s.is_empty() {
                if lid == 0x0409 {
                    if nid == 1 {
                        fam_en = s;
                    } else {
                        sub_en = s;
                    }
                } else if nid == 1 && fam_any.is_empty() {
                    fam_any = s;
                } else if nid == 2 && sub_any.is_empty() {
                    sub_any = s;
                }
            }
        } else if pid == 1 && eid == 0 && lid == 0 {
            if let Ok(s) = std::str::from_utf8(raw) {
                let s = s.trim().to_string();
                if nid == 1 && fam_any.is_empty() {
                    fam_any = s;
                } else if nid == 2 && sub_any.is_empty() {
                    sub_any = s;
                }
            }
        }
    }
    let fam = if !fam_en.is_empty() { fam_en } else { fam_any };
    let sub = if !sub_en.is_empty() { sub_en } else { sub_any };
    if fam.is_empty() || fam.starts_with('.') || fam.starts_with('@') || !fam.is_ascii() {
        return None;
    }
    let fl = fam.to_ascii_lowercase();
    if fl.contains("icons")
        || fl.contains("assets")
        || fl.contains("symbol")
        || fl.contains("dingbats")
        || fl.contains("wingdings")
        || fl.contains("webdings")
        || fl.contains("marlett")
        || fl.contains("hololens")
        || fl.contains("extb")
        || fl.contains("extg")
        || fl.contains("outlook")
        || fl.contains("specialty")
        || fl == "mt extra"
        || fl == "bookshelf"
    {
        return None;
    }

    let mut panose = [0u8; 10];
    if os2_loc.0 > 0 && os2_loc.1 >= 42 && os2_loc.0 + 42 <= file_len {
        let mut obuf = [0u8; 42];
        if f.seek(SeekFrom::Start(os2_loc.0)).is_ok() && f.read_exact(&mut obuf).is_ok() {
            panose.copy_from_slice(&obuf[32..42]);
        }
    }
    let mut fixed_pitch = false;
    if post_loc.0 > 0 && post_loc.1 >= 16 && post_loc.0 + 16 <= file_len {
        let mut pbuf = [0u8; 16];
        if f.seek(SeekFrom::Start(post_loc.0)).is_ok() && f.read_exact(&mut pbuf).is_ok() {
            fixed_pitch = u32_be(&pbuf, 12) != 0;
        }
    }

    let sl = sub.to_ascii_lowercase();
    let fam_has_bold = fl.ends_with(" bold") || fl.ends_with(" demi") || fl.ends_with(" semibold") || fl.ends_with(" black");
    let is_bold = !fam_has_bold && (sl.contains("bold") || sl.contains("demi"));
    let is_italic = sl.contains("italic") || sl.contains("oblique");
    let v = (is_bold as usize) | ((is_italic as usize) << 1);
    let cat = classify_font(&fam, fixed_pitch, panose);
    Some((fam, v, cat))
}

pub fn catalog() -> &'static [CatalogEntry] {
    static CATALOG: OnceLock<Vec<CatalogEntry>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let win_dir = font_dir();
        let bundled = bundled_dir();
        let mut entries: Vec<CatalogEntry> = Vec::with_capacity(255);

        // 0..31: keep the 31 built-in/bundled fonts at their exact indices.
        for (fi, &name) in FONT_NAMES.iter().enumerate() {
            let mut files: [Option<PathBuf>; 4] = [None, None, None, None];
            if let Some(folder) = bundled_folder(fi as u16) {
                if let Some(bdir) = bundled.as_ref() {
                    for (v, fname) in ["regular.ttf", "bold.ttf", "italic.ttf", "bolditalic.ttf"].iter().enumerate() {
                        let p = bdir.join(folder).join(fname);
                        if p.exists() {
                            files[v] = Some(p);
                        }
                    }
                }
            } else {
                let fnames = files_for(name);
                for (v, fname) in fnames.iter().enumerate() {
                    if !fname.is_empty() {
                        let p = win_dir.join(fname);
                        if p.exists() {
                            files[v] = Some(p);
                        }
                    }
                }
            }
            entries.push(CatalogEntry {
                name,
                cat: base_font_category(fi as u16),
                files,
                data: [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()],
                raw: [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()],
                metrics: [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()],
            });
        }

        // Scan system & Office CloudFonts directories for all other installed font families.
        let mut all_files = Vec::new();
        for d in system_font_dirs() {
            collect_font_files(&d, 0, &mut all_files);
        }

        struct Discovered {
            name: String,
            cat: FontCat,
            files: [Option<PathBuf>; 4],
        }
        let mut extra: BTreeMap<String, Discovered> = BTreeMap::new();

        for path in all_files {
            let Some((fam, v, cat)) = inspect_font_file(&path) else { continue };
            if let Some(existing) = entries.iter_mut().find(|e| e.name.eq_ignore_ascii_case(&fam)) {
                if existing.files[v].is_none() {
                    existing.files[v] = Some(path);
                }
                continue;
            }
            let key = fam.to_ascii_lowercase();
            let d = extra.entry(key).or_insert_with(|| Discovered {
                name: fam,
                cat,
                files: [None, None, None, None],
            });
            if d.files[v].is_none() {
                d.files[v] = Some(path);
            }
        }

        for (_, mut d) in extra {
            if entries.len() >= u16::MAX as usize {
                break;
            }
            if d.files[0].is_none() {
                d.files[0] = d.files[1].clone().or_else(|| d.files[2].clone()).or_else(|| d.files[3].clone());
            }
            if d.files[0].is_none() {
                continue;
            }
            let leaked_name: &'static str = Box::leak(d.name.into_boxed_str());
            entries.push(CatalogEntry {
                name: leaked_name,
                cat: d.cat,
                files: d.files,
                data: [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()],
                raw: [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()],
                metrics: [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()],
            });
        }

        entries
    })
}

/// Which faces actually exist, so the layout knows when to fake italics.
#[derive(Default, Clone)]
pub struct FontBook {
    /// key: (font index, variant 0..4) → egui family name, or None when missing.
    pub faces: HashMap<(u16, u8), Option<String>>,
    pub ui_loaded: bool,
}

impl FontBook {
    /// Returns the family to use and whether italics must be synthesised.
    pub fn family(&self, font: u16, bold: bool, italic: bool) -> (FontFamily, bool) {
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
        if font != DEFAULT_FONT {
            return self.family(DEFAULT_FONT, bold, italic);
        }
        (FontFamily::Proportional, italic)
    }
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

    // Symbol fallback for bullets, arrows and other special characters.
    let mut fallbacks: Vec<String> = Vec::new();
    for (key, file) in [("sym:win-emoji", "seguiemj.ttf"), ("sym:segoe-symbol", "seguisym.ttf"), ("sym:segoe-ui", "segoeui.ttf")] {
        if let Some(d) = load(file) {
            defs.font_data.insert(key.into(), Arc::new(d));
            fallbacks.push(key.into());
        }
    }
    for (key, abs_path) in [
        ("sym:mac-emoji", "/System/Library/Fonts/Apple Color Emoji.ttc"),
        ("sym:mac-symbols", "/System/Library/Fonts/Supplemental/Apple Symbols.ttf"),
        ("sym:mac-symbols-root", "/System/Library/Fonts/Apple Symbols.ttf"),
        ("sym:linux-noto-emoji", "/usr/share/fonts/truetype/noto/NotoEmoji-Regular.ttf"),
        ("sym:linux-noto-color", "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf"),
        ("sym:linux-noto-emoji-2", "/usr/share/fonts/noto/NotoEmoji-Regular.ttf"),
        ("sym:linux-symbola", "/usr/share/fonts/truetype/ancient-scripts/Symbola_hint.ttf"),
        ("sym:linux-dejavu", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
    ] {
        if let Ok(bytes) = std::fs::read(abs_path) {
            defs.font_data.insert(key.into(), Arc::new(FontData::from_owned(strip_ebdt(bytes))));
            fallbacks.push(key.into());
        }
    }
    let egui_defaults: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();

    for (fi, entry) in catalog().iter().enumerate() {
        let name = entry.name;
        for v in 0..4u8 {
            let key = format!("{name}#{v}");
            match entry.load_face(v as usize) {
                Some(data) => {
                    defs.font_data.insert(key.clone(), data);
                    let mut chain = vec![key.clone()];
                    chain.extend(fallbacks.iter().cloned());
                    chain.extend(egui_defaults.iter().cloned());
                    defs.families.insert(FontFamily::Name(key.clone().into()), chain);
                    book.faces.insert((fi as u16, v), Some(key));
                }
                None => {
                    book.faces.insert((fi as u16, v), None);
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
        defs.families.insert(FontFamily::Name("ui-bold".into()), egui_defaults);
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
                for (fi, entry) in catalog().iter().enumerate() {
                    let name = entry.name;
                    for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                        let (fam, _) = book.family(fi as u16, bold, italic);
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

    #[test]
    fn system_fonts_discovered() {
        let cat = catalog();
        assert!(cat.len() > FONT_NAMES.len(), "Expected system fonts beyond the 31 base fonts, got {}", cat.len());
    }

    #[test]
    fn word_hhea_line_metrics_are_in_typographic_range() {
        let m = word_line_metrics(DEFAULT_FONT, false, false).expect("default font has OpenType hhea/OS/2 metrics");
        let total = m.0 + m.1;
        assert!((1.0..=1.5).contains(&total), "unexpected Word single-line em ratio: {m:?} (sum={total})");
        let (bytes, _, _) = font_file_bytes(DEFAULT_FONT, false, false).expect("default font bytes readable");
        assert!(bytes.len() > 1024);
    }
}

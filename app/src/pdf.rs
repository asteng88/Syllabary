//! PDF export: lays the document out at 72 points per inch with the same line
//! breaks as the screen, then writes each word at its exact position using the
//! PDF standard fonts (Times, Helvetica, Courier). Written by hand; no internet.

use eframe::egui;

use crate::doc::{CharStyle, Document, ListKind};
use crate::fonts::FontBook;
use crate::layout;

/// Font family groups → PDF standard font base names (regular, bold, italic, bold italic).
const FAMILIES: [[&str; 4]; 3] = [
    ["Times-Roman", "Times-Bold", "Times-Italic", "Times-BoldItalic"],
    ["Helvetica", "Helvetica-Bold", "Helvetica-Oblique", "Helvetica-BoldOblique"],
    ["Courier", "Courier-Bold", "Courier-Oblique", "Courier-BoldOblique"],
];

fn family_of(font: u8) -> usize {
    match crate::doc::font_category(font) {
        crate::doc::FontCat::Serif => 0,
        crate::doc::FontCat::Mono => 2,
        _ => 1,
    }
}

fn font_res(s: &CharStyle) -> usize {
    family_of(s.font) * 4 + (s.bold as usize) + 2 * (s.italic as usize)
}

/// Unicode → Windows-1252 byte (the encoding the standard fonts use here).
fn win_ansi(c: char) -> u8 {
    let u = c as u32;
    if (0x20..0x7f).contains(&u) || (0xa0..=0xff).contains(&u) {
        return u as u8;
    }
    match c {
        '€' => 0x80,
        '‚' => 0x82,
        'ƒ' => 0x83,
        '„' => 0x84,
        '…' => 0x85,
        '†' => 0x86,
        '‡' => 0x87,
        'ˆ' => 0x88,
        '‰' => 0x89,
        'Š' => 0x8a,
        '‹' => 0x8b,
        'Œ' => 0x8c,
        'Ž' => 0x8e,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' | '◦' | '▪' => 0x95,
        '–' => 0x96,
        '—' | '─' => 0x97,
        '˜' => 0x98,
        '™' => 0x99,
        'š' => 0x9a,
        '›' => 0x9b,
        'œ' => 0x9c,
        'ž' => 0x9e,
        'Ÿ' => 0x9f,
        _ => b'?',
    }
}

fn pdf_string(text: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for c in text.chars() {
        let b = win_ansi(c);
        if b == b'(' || b == b')' || b == b'\\' {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
    out
}

fn rgb(c: [u8; 3]) -> String {
    format!("{:.3} {:.3} {:.3}", c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0)
}

/// Approximate Helvetica width (header/footer centring only).
fn approx_width(text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * size * 0.5
}

struct Page {
    ops: Vec<u8>,
}

impl Page {
    fn push(&mut self, s: &str) {
        self.ops.extend_from_slice(s.as_bytes());
    }
    fn text(&mut self, font: usize, size: f32, color: [u8; 3], x: f32, y: f32, txt: &str) {
        self.push(&format!("BT /F{} {:.2} Tf {} rg {:.2} {:.2} Td ", font + 1, size, rgb(color), x, y));
        let s = pdf_string(txt);
        self.ops.extend_from_slice(&s);
        self.push(" Tj ET\n");
    }
    fn rect(&mut self, color: [u8; 3], x: f32, y: f32, w: f32, h: f32) {
        self.push(&format!("{} rg {:.2} {:.2} {:.2} {:.2} re f\n", rgb(color), x, y, w, h));
    }
}

pub fn export(ctx: &egui::Context, doc: &Document, book: &FontBook) -> Vec<u8> {
    // 0.75 × 96 = 72 units per inch: one unit is one PDF point.
    let lay = layout::build(ctx, doc, book, 0.75, false);
    let pw = lay.page_size.x;
    let ph = lay.page_size.y;
    let mut pages: Vec<Page> = (0..lay.pages).map(|_| Page { ops: Vec::new() }).collect();
    let pg = doc.page;

    for (k, page) in pages.iter_mut().enumerate() {
        if pg.background != [255, 255, 255] {
            page.rect(pg.background, 0.0, 0.0, pw, ph);
        }
        let gray = [110, 110, 110];
        if !doc.header.is_empty() {
            let y = ph - lay.content_offset.y * 0.5 - 3.5;
            page.text(4, 10.0, gray, (pw - approx_width(&doc.header, 10.0)) / 2.0, y, &doc.header);
        }
        let mut foot = doc.footer.clone();
        if pg.page_numbers {
            let pn = format!("Page {}", k + 1);
            foot = if foot.is_empty() { pn } else { format!("{foot}    {pn}") };
        }
        if !foot.is_empty() {
            let bottom = ph - lay.content_offset.y - lay.content_size.y;
            page.text(4, 10.0, gray, (pw - approx_width(&foot, 10.0)) / 2.0, bottom * 0.5 - 3.5, &foot);
        }
    }

    for l in &lay.lines {
        let para = &doc.paras[l.para];
        let page = &mut pages[l.page];
        let ox = lay.content_offset.x + l.x;
        let oy = lay.content_offset.y + l.y;
        let Some(row) = l.galley.rows.first() else { continue };
        let glyphs = &row.row.glyphs;
        let gh = l.galley.rect.height();
        let n = (l.end - l.start).min(glyphs.len());

        // highlights first, so text sits on top
        let (hy0, hy1) = layout::highlight_pill_y(&l.galley, 0, n, gh);
        for (k, g) in glyphs.iter().enumerate().take(n) {
            let s = para.styles[l.start + k];
            if let Some(h) = s.highlight {

                let t = if n <= 1 { 0.5 } else { k as f32 / (n - 1) as f32 };
                let col = crate::doc::lerp_rgb(h, s.highlight_to.unwrap_or(h), t);
                page.rect(col, ox + g.pos.x, ph - (oy + hy1), g.advance_width + 0.3, (hy1 - hy0).max(1.0));
            }
        }

        // words: runs of non-space glyphs with the same style
        let mut k = 0;
        while k < n {
            let s = para.styles[l.start + k];
            if glyphs[k].chr.is_whitespace() {
                k += 1;
                continue;
            }
            let start = k;
            let mut word = String::new();
            while k < n && !glyphs[k].chr.is_whitespace() && para.styles[l.start + k] == s {
                word.push(glyphs[k].chr);
                k += 1;
            }
            let g0 = &glyphs[start];
            let mut size = s.size;
            let mut base = oy + g0.pos.y;
            if s.script != 0 {
                size *= 0.65;
                if s.script > 0 {
                    base -= s.size * 0.33;
                } else {
                    base += s.size * 0.12;
                }
            }
            page.text(font_res(&s), size, s.color, ox + g0.pos.x, ph - base, &word);
        }

        // underline and strikethrough as thin bars spanning spaces too
        let mut k = 0;
        while k < n {
            let s = para.styles[l.start + k];
            if !(s.underline || s.strike) {
                k += 1;
                continue;
            }
            let start = k;
            while k < n && para.styles[l.start + k].underline == s.underline && para.styles[l.start + k].strike == s.strike {
                k += 1;
            }
            let mut end = k;
            while end > start + 1 && glyphs[end - 1].chr.is_whitespace() {
                end -= 1;
            }
            let x0 = ox + glyphs[start].pos.x;
            let x1 = ox + glyphs[end - 1].pos.x + glyphs[end - 1].advance_width;
            let base = oy + glyphs[start].pos.y;
            let t = (s.size / 16.0).max(0.5);
            if s.underline {
                page.rect(s.color, x0, ph - (base + s.size * 0.12) - t, x1 - x0, t);
            }
            if s.strike {
                page.rect(s.color, x0, ph - (base - s.size * 0.28) - t / 2.0, x1 - x0, t);
            }
        }

        // bullet or number
        if let Some(m) = &l.marker {
            if let Some(mrow) = m.rows.first() {
                let s = para.style_at(0);
                let txt: String = mrow.row.glyphs.iter().map(|g| g.chr).collect();
                let base = oy + mrow.row.glyphs.first().map_or(gh * 0.8, |g| g.pos.y);
                let mx = lay.content_offset.x + l.marker_x;
                page.text(font_res(&CharStyle { bold: false, italic: false, ..s }), s.size, s.color, mx, ph - base, &txt);
            }
        }
        let _ = ListKind::None;
    }

    write_pdf(&pages, pw, ph, &doc.info.title, &doc.info.author)
}

fn write_pdf(pages: &[Page], pw: f32, ph: f32, title: &str, author: &str) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    let n_fonts = 12;
    // object numbers: 1 catalog, 2 pages, 3 info, 4..4+12 fonts, then per page: page + content
    let font0 = 4;
    let page0 = font0 + n_fonts;
    let obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(out.len());
        let num = offsets.len();
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>");
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", page0 + i * 2)).collect();
    obj(&mut out, &mut offsets, format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()).as_bytes());
    let mut info = b"<< /Producer (Scribe Pro) ".to_vec();
    if !title.is_empty() {
        info.extend_from_slice(b"/Title ");
        info.extend_from_slice(&pdf_string(title));
    }
    if !author.is_empty() {
        info.extend_from_slice(b" /Author ");
        info.extend_from_slice(&pdf_string(author));
    }
    info.extend_from_slice(b" >>");
    obj(&mut out, &mut offsets, &info);
    for fam in FAMILIES {
        for name in fam {
            obj(&mut out, &mut offsets, format!("<< /Type /Font /Subtype /Type1 /BaseFont /{name} /Encoding /WinAnsiEncoding >>").as_bytes());
        }
    }
    let fonts: Vec<String> = (0..n_fonts).map(|i| format!("/F{} {} 0 R", i + 1, font0 + i)).collect();
    for (i, p) in pages.iter().enumerate() {
        let content_num = page0 + i * 2 + 1;
        obj(
            &mut out,
            &mut offsets,
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] /Resources << /Font << {} >> >> /Contents {content_num} 0 R >>", fonts.join(" ")).as_bytes(),
        );
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(&p.ops, 6);
        let mut body = format!("<< /Length {} /Filter /FlateDecode >>\nstream\n", packed.len()).into_bytes();
        body.extend_from_slice(&packed);
        body.extend_from_slice(b"\nendstream");
        obj(&mut out, &mut offsets, &body);
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Pos;

    #[test]
    fn pdf_is_well_formed() {
        let ctx = egui::Context::default();
        let book = FontBook::default();
        let mut doc = Document::default();
        doc.insert_text(Pos::new(0, 0), &"Hello (world) – “quotes” ".repeat(300), CharStyle { underline: true, ..Default::default() });
        doc.page.page_numbers = true;
        let mut bytes = Vec::new();
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            bytes = export(&ctx, &doc, &book);
        });
        out.textures_delta.clear();
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.starts_with("%PDF-1.4"));
        assert!(s.trim_end().ends_with("%%EOF"));
        assert!(s.contains("/Count 2") || s.contains("/Count 3"), "several pages");
        // the xref offsets must point at the objects
        let x = s.rfind("startxref").unwrap();
        let xref: usize = s[x + 10..].lines().next().unwrap().trim().parse().unwrap();
        assert!(bytes[xref..].starts_with(b"xref"));
    }
}

#[cfg(test)]
mod sample {
    /// Writes shots/sample.pdf and shots/sample.docx from shots/lists.wpx for a visual check.
    #[test]
    #[ignore]
    fn write_sample() {
        let src = std::fs::read_to_string("shots/lists.wpx").unwrap();
        let doc = crate::doc::from_json(&src).unwrap();
        let ctx = eframe::egui::Context::default();
        let mut book = crate::fonts::FontBook::default();
        let mut bytes = Vec::new();
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            book = crate::fonts::install(&ctx);
        });
        out.textures_delta.clear();
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            bytes = super::export(&ctx, &doc, &book);
        });
        out.textures_delta.clear();
        std::fs::write("shots/sample.pdf", bytes).unwrap();
        std::fs::write("shots/sample.docx", crate::docx::write(&doc)).unwrap();
    }
}

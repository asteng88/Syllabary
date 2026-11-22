//! WYSIWYG Page Rasterizer and Calibrated sRGB PDF Exporter.
//!
//! Renders pages directly from the live `egui` glyph atlas and layout engine so every custom font
//! (Calibri, Lora, Playfair Display, Caveat, Dancing Script, system emojis, etc.), vertically
//! centered rounded highlight pill, Craft-style left-to-right color fade, paragraph shading, and
//! checklist checkbox matches the screen pixel-for-pixel. Also emits calibrated sRGB vector + text
//! streams so exported PDFs remain searchable and selectable.

use eframe::egui::{
    self,
    text::{LayoutJob, TextFormat},
    Color32, FontFamily, FontId, Galley,
};
use std::sync::Arc;

use crate::doc::{lerp_rgb, Align, CharStyle, Document, FooterSegment, ListKind};
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
    format!("{:.4} {:.4} {:.4}", c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0)
}

/// Approximate Helvetica width (header/footer centring only).
fn approx_width(text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * size * 0.5
}

// ── High-DPI WYSIWYG Software Page Canvas ───────────────────────────────

struct Canvas {
    w: u32,
    h: u32,
    /// Unmultiplied sRGB RGBA pixels (`w * h * 4`).
    rgba: Vec<u8>,
}

impl Canvas {
    fn new(w: u32, h: u32, bg: [u8; 3]) -> Self {
        let count = (w as usize) * (h as usize);
        let mut rgba = Vec::with_capacity(count * 4);
        for _ in 0..count {
            rgba.extend_from_slice(&[bg[0], bg[1], bg[2], 255]);
        }
        Self { w, h, rgba }
    }

    #[inline]
    fn blend_px(&mut self, x: i32, y: i32, col: [u8; 3], alpha: u8) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 || alpha == 0 {
            return;
        }
        let idx = ((y as usize) * (self.w as usize) + (x as usize)) * 4;
        if alpha == 255 {
            self.rgba[idx] = col[0];
            self.rgba[idx + 1] = col[1];
            self.rgba[idx + 2] = col[2];
            self.rgba[idx + 3] = 255;
        } else {
            let a = alpha as u32;
            let inv = 255 - a;
            self.rgba[idx] = ((col[0] as u32 * a + self.rgba[idx] as u32 * inv + 127) / 255) as u8;
            self.rgba[idx + 1] = ((col[1] as u32 * a + self.rgba[idx + 1] as u32 * inv + 127) / 255) as u8;
            self.rgba[idx + 2] = ((col[2] as u32 * a + self.rgba[idx + 2] as u32 * inv + 127) / 255) as u8;
            self.rgba[idx + 3] = 255;
        }
    }

    /// Signed distance to a rounded rectangle `[x0, y0]..[x1, y1]` with corner radius `r`.
    #[inline]
    fn sd_rounded_box(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
        let cx = (x0 + x1) * 0.5;
        let cy = (y0 + y1) * 0.5;
        let hw = (x1 - x0) * 0.5;
        let hh = (y1 - y0) * 0.5;
        let rad = r.min(hw).min(hh).max(0.0);
        let qx = (px - cx).abs() - (hw - rad);
        let qy = (py - cy).abs() - (hh - rad);
        let out_d = (qx.max(0.0).hypot(qy.max(0.0))) - rad;
        let in_d = qx.max(qy).min(0.0);
        out_d + in_d
    }

    /// Fills an anti-aliased rounded horizontal gradient pill fading from `c1` (left) to `c2` (right).
    #[expect(clippy::too_many_arguments, reason = "Pill coordinates, radius, and endpoint colors are all distinct parameters")]
    fn fill_gradient_pill(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32, c1: [u8; 3], c2: [u8; 3]) {
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let min_x = (x0.floor() as i32 - 1).max(0);
        let max_x = (x1.ceil() as i32 + 1).min(self.w as i32 - 1);
        let min_y = (y0.floor() as i32 - 1).max(0);
        let max_y = (y1.ceil() as i32 + 1).min(self.h as i32 - 1);
        let span_w = (x1 - x0).max(1.0);
        for py in min_y..=max_y {
            let fy = py as f32 + 0.5;
            for px in min_x..=max_x {
                let fx = px as f32 + 0.5;
                let sd = Self::sd_rounded_box(fx, fy, x0, y0, x1, y1, radius);
                if sd < 0.5 {
                    let cov = (0.5 - sd).clamp(0.0, 1.0);
                    let alpha = (cov * 255.0).round() as u8;
                    let t = ((fx - x0) / span_w).clamp(0.0, 1.0);
                    let col = if c1 == c2 { c1 } else { lerp_rgb(c1, c2, t) };
                    self.blend_px(px, py, col, alpha);
                }
            }
        }
    }

    /// Strokes an anti-aliased rounded rectangle border.
    #[expect(clippy::too_many_arguments, reason = "Rectangle bounds, radius, stroke width, and color are distinct")]
    fn stroke_rounded_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32, stroke_w: f32, col: [u8; 3]) {
        let min_x = (x0.floor() as i32 - 2).max(0);
        let max_x = (x1.ceil() as i32 + 2).min(self.w as i32 - 1);
        let min_y = (y0.floor() as i32 - 2).max(0);
        let max_y = (y1.ceil() as i32 + 2).min(self.h as i32 - 1);
        let half = stroke_w * 0.5;
        for py in min_y..=max_y {
            let fy = py as f32 + 0.5;
            for px in min_x..=max_x {
                let fx = px as f32 + 0.5;
                let sd = (Self::sd_rounded_box(fx, fy, x0 + half, y0 + half, x1 - half, y1 - half, (radius - half).max(0.0))).abs() - half;
                if sd < 0.5 {
                    let cov = (0.5 - sd).clamp(0.0, 1.0);
                    self.blend_px(px, py, col, (cov * 255.0).round() as u8);
                }
            }
        }
    }

    /// Draws an anti-aliased line segment from `(ax, ay)` to `(bx, by)`.
    fn draw_line(&mut self, ax: f32, ay: f32, bx: f32, by: f32, stroke_w: f32, col: [u8; 3]) {
        let pad = stroke_w + 2.0;
        let min_x = (ax.min(bx) - pad).floor().max(0.0) as i32;
        let max_x = (ax.max(bx) + pad).ceil().min(self.w as f32 - 1.0) as i32;
        let min_y = (ay.min(by) - pad).floor().max(0.0) as i32;
        let max_y = (ay.max(by) + pad).ceil().min(self.h as f32 - 1.0) as i32;
        let vx = bx - ax;
        let vy = by - ay;
        let len_sq = (vx * vx + vy * vy).max(1e-6);
        let half = stroke_w * 0.5;
        for py in min_y..=max_y {
            let fy = py as f32 + 0.5;
            for px in min_x..=max_x {
                let fx = px as f32 + 0.5;
                let t = (((fx - ax) * vx + (fy - ay) * vy) / len_sq).clamp(0.0, 1.0);
                let dx = fx - (ax + t * vx);
                let dy = fy - (ay + t * vy);
                let d = dx.hypot(dy) - half;
                if d < 0.5 {
                    let cov = (0.5 - d).clamp(0.0, 1.0);
                    self.blend_px(px, py, col, (cov * 255.0).round() as u8);
                }
            }
        }
    }

    /// Blits all glyphs and decoration lines from a `Galley` positioned at `(ox_pt, oy_pt)` in page points.
    fn blit_galley(&mut self, galley: &Galley, atlas: &egui::ColorImage, ox_pt: f32, oy_pt: f32, ppp: f32) {
        let Some(placed) = galley.rows.first() else { return };
        let row = &placed.row;
        let verts = &row.visuals.mesh.vertices;
        let atlas_w = atlas.width();
        let atlas_h = atlas.height();

        for g in &row.glyphs {
            let uv = g.uv_rect;
            if uv.is_nothing() {
                continue;
            }
            let u0 = uv.min[0] as usize;
            let v0 = uv.min[1] as usize;
            let u1 = (uv.max[0] as usize).min(atlas_w);
            let v1 = (uv.max[1] as usize).min(atlas_h);
            if u1 <= u0 || v1 <= v0 {
                continue;
            }
            let gw = u1 - u0;
            let gh = v1 - v0;

            let fv = g.first_vertex as usize;
            let (c_left, c_right, fake_italic) = if fv + 3 < verts.len() {
                let vl = verts[fv].color.to_srgba_unmultiplied();
                let vr = verts[fv + 1].color.to_srgba_unmultiplied();
                let italic_shear = (verts[fv].pos.x - verts[fv + 2].pos.x) > 0.1;
                ([vl[0], vl[1], vl[2]], [vr[0], vr[1], vr[2]], italic_shear)
            } else {
                ([0, 0, 0], [0, 0, 0], false)
            };

            let dst_x0 = ((ox_pt + placed.pos.x + g.pos.x + uv.offset.x) * ppp).round() as i32;
            let dst_y0 = ((oy_pt + placed.pos.y + g.pos.y + uv.offset.y) * ppp).round() as i32;

            for dy in 0..gh {
                let shear_x = if fake_italic {
                    (((gh.saturating_sub(1 + dy)) as f32 / gh.max(1) as f32) * (gh as f32 * 0.25)).round() as i32
                } else {
                    0
                };
                let py = dst_y0 + dy as i32;
                let row_off = (v0 + dy) * atlas_w;
                for dx in 0..gw {
                    let tex = atlas.pixels[row_off + u0 + dx];
                    let ta = tex.a();
                    if ta == 0 {
                        continue;
                    }
                    let px = dst_x0 + dx as i32 + shear_x;
                    if g.is_color {
                        let tc = tex.to_srgba_unmultiplied();
                        self.blend_px(px, py, [tc[0], tc[1], tc[2]], tc[3]);
                    } else {
                        let col = if c_left == c_right {
                            c_left
                        } else {
                            let t = dx as f32 / gw.saturating_sub(1).max(1) as f32;
                            lerp_rgb(c_left, c_right, t)
                        };
                        self.blend_px(px, py, col, ta);
                    }
                }
            }
        }
    }

    /// Bilinearly blits an RGBA image into `[x0, y0]..[x0 + dst_w, y0 + dst_h]` in pixel coordinates.
    #[expect(clippy::too_many_arguments, reason = "Source RGBA image dimensions and destination rectangle coordinates are distinct")]
    fn blit_rgba_scaled(&mut self, src_w: u32, src_h: u32, src_rgba: &[u8], x0: f32, y0: f32, dst_w: f32, dst_h: f32) {
        if src_w == 0 || src_h == 0 || dst_w < 1.0 || dst_h < 1.0 {
            return;
        }
        let min_x = (x0.round() as i32).max(0);
        let max_x = ((x0 + dst_w).round() as i32 - 1).min(self.w as i32 - 1);
        let min_y = (y0.round() as i32).max(0);
        let max_y = ((y0 + dst_h).round() as i32 - 1).min(self.h as i32 - 1);
        let sw_m1 = (src_w - 1) as f32;
        let sh_m1 = (src_h - 1) as f32;
        for py in min_y..=max_y {
            let v = ((py as f32 + 0.5 - y0) / dst_h).clamp(0.0, 1.0) * sh_m1;
            let v0 = (v.floor() as u32).min(src_h - 1);
            let v1 = (v0 + 1).min(src_h - 1);
            let fv = v - v0 as f32;
            for px in min_x..=max_x {
                let u = ((px as f32 + 0.5 - x0) / dst_w).clamp(0.0, 1.0) * sw_m1;
                let u0 = (u.floor() as u32).min(src_w - 1);
                let u1 = (u0 + 1).min(src_w - 1);
                let fu = u - u0 as f32;
                let p00 = ((v0 * src_w + u0) * 4) as usize;
                let p10 = ((v0 * src_w + u1) * 4) as usize;
                let p01 = ((v1 * src_w + u0) * 4) as usize;
                let p11 = ((v1 * src_w + u1) * 4) as usize;
                if p11 + 3 < src_rgba.len() {
                    let mut ch = [0u8; 4];
                    for c in 0..4 {
                        let top = src_rgba[p00 + c] as f32 * (1.0 - fu) + src_rgba[p10 + c] as f32 * fu;
                        let bot = src_rgba[p01 + c] as f32 * (1.0 - fu) + src_rgba[p11 + c] as f32 * fu;
                        ch[c] = (top * (1.0 - fv) + bot * fv).round().clamp(0.0, 255.0) as u8;
                    }
                    self.blend_px(px, py, [ch[0], ch[1], ch[2]], ch[3]);
                }
            }
        }
    }
}

/// Renders all pages of `doc` into high-DPI unmultiplied sRGB RGBA buffers `(width, height, rgba_bytes)`.
pub fn render_pages_rgba(ctx: &egui::Context, doc: &Document, book: &FontBook, dpi_scale: f32) -> Vec<(u32, u32, Vec<u8>)> {
    let scale = dpi_scale.clamp(1.0, 4.0);
    let lay = layout::build(ctx, doc, book, scale, false);
    let ppp = ctx.pixels_per_point().max(1.0);

    // Pre-shape header, footer & image caption galleys so their glyphs are present in the font atlas
    let pt = lay.ppi / 72.0;
    let hf_size = 10.0 * pt;
    let hf_col = Color32::from_rgb(95, 95, 95);
    type RichFooterLayout = (Option<(f32, f32, f32, f32, [u8; 3])>, Vec<(FooterSegment, Arc<Galley>)>);
    let (header_galley, rich_footer_galleys, footer_galleys, caption_galleys) = ctx.fonts_mut(|fonts| {
        let hg = if !doc.header.is_empty() {
            let (fam, fake_it) = book.family(0, false, doc.header_italic);
            let mut job = LayoutJob::default();
            job.append(
                &doc.header,
                0.0,
                TextFormat {
                    font_id: FontId::new(hf_size, fam),
                    color: hf_col,
                    italics: fake_it,
                    ..Default::default()
                },
            );
            Some(fonts.layout_job(job))
        } else {
            None
        };
        let mut rfgs: Vec<Option<RichFooterLayout>> = Vec::with_capacity(lay.pages);
        let mut fgs: Vec<Option<Arc<Galley>>> = Vec::with_capacity(lay.pages);
        for k in 0..lay.pages {
            if let Some(pf) = doc.resolved_page_footer(k, lay.pages) {
                let mut seg_gals = Vec::with_capacity(pf.segments.len());
                for seg in &pf.segments {
                    let mut job = LayoutJob::default();
                    for run in &seg.runs {
                        let st = run.style;
                        let (fam, fake_it) = book.family(st.font, st.bold, st.italic);
                        job.append(
                            &run.text,
                            0.0,
                            TextFormat {
                                font_id: FontId::new(st.size.max(4.5) * pt, fam),
                                color: Color32::from_rgb(st.color[0], st.color[1], st.color[2]),
                                italics: fake_it,
                                ..Default::default()
                            },
                        );
                    }
                    seg_gals.push((seg.clone(), fonts.layout_job(job)));
                }
                rfgs.push(Some((pf.rule, seg_gals)));
                fgs.push(None);
                continue;
            }
            rfgs.push(None);
            let mut foot = doc.footer.clone();
            if doc.page.page_numbers {
                let pn = format!("Page {}", k + 1);
                foot = if foot.is_empty() { pn } else { format!("{foot}    {pn}") };
            }
            if !foot.is_empty() {
                let (fam, fake_it) = book.family(0, false, doc.footer_italic);
                let mut job = LayoutJob::default();
                job.append(
                    &foot,
                    0.0,
                    TextFormat {
                        font_id: FontId::new(hf_size, fam),
                        color: hf_col,
                        italics: fake_it,
                        ..Default::default()
                    },
                );
                fgs.push(Some(fonts.layout_job(job)));
            } else {
                fgs.push(None);
            }
        }
        let mut cgs: std::collections::HashMap<u64, Arc<Galley>> = std::collections::HashMap::new();
        for img in &doc.images {
            if !img.caption.is_empty() {
                let mut job = LayoutJob::default();
                job.append(
                    &img.caption,
                    0.0,
                    TextFormat {
                        font_id: FontId::new(hf_size.max(8.5), FontFamily::Proportional),
                        color: Color32::from_rgb(78, 86, 102),
                        ..Default::default()
                    },
                );
                cgs.insert(img.id, fonts.layout_job(job));
            }
        }
        (hg, rfgs, fgs, cgs)
    });

    let atlas = ctx.fonts_mut(|f| f.image());
    let img_w = (lay.page_size.x * ppp).round().max(1.0) as u32;
    let img_h = (lay.page_size.y * ppp).round().max(1.0) as u32;

    let mut canvases: Vec<Canvas> = (0..lay.pages).map(|_| Canvas::new(img_w, img_h, doc.page.background)).collect();

    // 1. Header & Footer + Table cell backgrounds and borders on each page
    for (k, canvas) in canvases.iter_mut().enumerate() {
        if let Some(hg) = &header_galley {
            let gw = hg.rect.width();
            let gh = hg.rect.height();
            let hx = match doc.header_align {
                Align::Left | Align::Justify => lay.content_offset.x,
                Align::Center => lay.content_offset.x + (lay.content_size.x - gw) * 0.5,
                Align::Right => (lay.content_offset.x + lay.content_size.x - gw).max(lay.content_offset.x),
            };
            let hy = (lay.content_offset.y * 0.5 - gh * 0.5).max(3.0 * scale);
            canvas.blit_galley(hg, &atlas, hx, hy, ppp);
        }
        if let Some(Some((rule, seg_gals))) = rich_footer_galleys.get(k) {
            if let Some((x0, x1, y_bot, w_pt, col)) = *rule {
                let ry = (lay.page_size.y - y_bot * pt).clamp(2.0 * scale, lay.page_size.y - 2.0 * scale);
                let half_h = (w_pt * pt * ppp * 0.5).max(0.5);
                canvas.fill_gradient_pill(x0 * pt * ppp, ry * ppp - half_h, x1 * pt * ppp, ry * ppp + half_h, 0.0, col, col);
            }
            for (seg, fg) in seg_gals {
                let gw = fg.rect.width();
                let gh = fg.rect.height();
                let fx = match seg.align {
                    Align::Center => {
                        let mid = ((seg.x_pt + seg.max_x_pt) * 0.5) * pt;
                        (mid - gw * 0.5).clamp(2.0 * pt, (lay.page_size.x - gw - 2.0 * pt).max(0.0))
                    }
                    Align::Right => (seg.max_x_pt * pt - gw).max(2.0 * pt),
                    Align::Left | Align::Justify => (seg.x_pt * pt).min((lay.page_size.x - gw - 2.0 * pt).max(0.0)),
                };
                let ascent = fg
                    .rows
                    .first()
                    .and_then(|rw| rw.row.glyphs.first())
                    .map_or(gh * 0.78, |g| g.pos.y);
                let fy = (lay.page_size.y - seg.bottom_pt * pt - ascent)
                    .clamp(1.0 * pt, (lay.page_size.y - gh - 1.0 * pt).max(0.0));
                canvas.blit_galley(fg, &atlas, fx, fy, ppp);
            }
        } else if let Some(Some(fg)) = footer_galleys.get(k) {
            let gw = fg.rect.width();
            let gh = fg.rect.height();
            let fx = match doc.footer_align {
                Align::Left | Align::Justify => lay.content_offset.x,
                Align::Center => lay.content_offset.x + (lay.content_size.x - gw) * 0.5,
                Align::Right => (lay.content_offset.x + lay.content_size.x - gw).max(lay.content_offset.x),
            };
            let bottom_margin = (lay.page_size.y - lay.content_offset.y - lay.content_size.y).max(gh + 4.0 * scale);
            let fy = lay.page_size.y - (bottom_margin * 0.42 + gh * 0.5).clamp(gh + 3.0 * scale, 30.0 * pt + gh * 0.5);
            canvas.blit_galley(fg, &atlas, fx, fy, ppp);
        }

        // Table cell background shading first
        for cell in lay.cells.iter().filter(|c| c.page == k) {
            if let Some(rgb) = cell.shading {
                let x0 = (lay.content_offset.x + cell.rect.min.x) * ppp;
                let y0 = (lay.content_offset.y + cell.rect.min.y) * ppp;
                let x1 = (lay.content_offset.x + cell.rect.max.x) * ppp;
                let y1 = (lay.content_offset.y + cell.rect.max.y) * ppp;
                canvas.fill_gradient_pill(x0, y0, x1, y1, 0.0, rgb, rgb);
            }
        }
        // Table cell borders second
        for cell in lay.cells.iter().filter(|c| c.page == k) {
            let x0 = (lay.content_offset.x + cell.rect.min.x) * ppp;
            let y0 = (lay.content_offset.y + cell.rect.min.y) * ppp;
            let x1 = (lay.content_offset.x + cell.rect.max.x) * ppp;
            let y1 = (lay.content_offset.y + cell.rect.max.y) * ppp;
            if let Some(b) = cell.borders.top {
                if b.width_pt > 0.0 {
                    let hw = (b.width_pt * pt * ppp * 0.5).max(0.6);
                    canvas.fill_gradient_pill(x0, y0 - hw, x1, y0 + hw, 0.0, b.color, b.color);
                }
            }
            if let Some(b) = cell.borders.bottom {
                if b.width_pt > 0.0 {
                    let hw = (b.width_pt * pt * ppp * 0.5).max(0.6);
                    canvas.fill_gradient_pill(x0, y1 - hw, x1, y1 + hw, 0.0, b.color, b.color);
                }
            }
            if let Some(b) = cell.borders.left {
                if b.width_pt > 0.0 {
                    let hw = (b.width_pt * pt * ppp * 0.5).max(0.6);
                    canvas.fill_gradient_pill(x0 - hw, y0, x0 + hw, y1, 0.0, b.color, b.color);
                }
            }
            if let Some(b) = cell.borders.right {
                if b.width_pt > 0.0 {
                    let hw = (b.width_pt * pt * ppp * 0.5).max(0.6);
                    canvas.fill_gradient_pill(x1 - hw, y0, x1 + hw, y1, 0.0, b.color, b.color);
                }
            }
        }
    }

    // 2. Lines, paragraph shading, horizontal rules, rounded gradient pills, glyphs, underlines, and checklists
    for l in &lay.lines {
        let Some(canvas) = canvases.get_mut(l.page) else { continue };
        let para = &doc.paras[l.para];
        let pfmt = para.fmt;
        let ox = lay.content_offset.x + l.x;
        let oy = lay.content_offset.y + l.y;
        let content_x = lay.content_offset.x;
        let gh = l.galley.rect.height();

        // Paragraph background shading
        if let Some(sh) = pfmt.shading {
            let x0 = (content_x - 6.0 * scale) * ppp;
            let y0 = (oy - 1.0 * scale) * ppp;
            let x1 = (content_x + lay.content_size.x + 6.0 * scale) * ppp;
            let y1 = (oy + l.height + 1.0 * scale) * ppp;
            canvas.fill_gradient_pill(x0, y0, x1, y1, 4.0 * scale * ppp, sh, sh);
        }
        // Quote left accent bar
        if pfmt.style == 7 {
            let x0 = (ox - 12.0 * scale) * ppp;
            let y0 = oy * ppp;
            let x1 = x0 + 3.5 * scale * ppp;
            let y1 = (oy + l.height) * ppp;
            canvas.fill_gradient_pill(x0, y0, x1, y1, 1.5 * scale * ppp, [43, 87, 151], [43, 87, 151]);
        }
        // Horizontal rule divider
        if pfmt.hr && l.first_in_para {
            let y_mid = (oy + l.height * 0.5) * ppp;
            let hr_col = pfmt.hr_color.unwrap_or([185, 185, 185]);
            let half = if pfmt.hr_width_pt > 0.0 {
                (pfmt.hr_width_pt * pt * ppp * 0.5).max(0.5)
            } else {
                0.75 * scale * ppp
            };
            canvas.fill_gradient_pill(
                content_x * ppp,
                y_mid - half,
                (content_x + lay.content_size.x) * ppp,
                y_mid + half,
                0.0,
                hr_col,
                hr_col,
            );
        }

        // Embedded image block + caption
        if pfmt.image_id > 0 && l.first_in_para {
            let indent = pfmt.indent as f32 * 0.5 * lay.ppi;
            let cell_avail = if pfmt.table_id > 0 {
                lay.cells
                    .iter()
                    .find(|c| c.table_id == pfmt.table_id && c.row == pfmt.table_row && c.col == pfmt.table_col && c.page == l.page)
                    .map(|c| (c.rect.width() - 8.0).max(12.0))
            } else {
                None
            };
            let avail = cell_avail.unwrap_or_else(|| (lay.content_size.x - indent).max(lay.ppi * 0.25));
            if let Some((iw_pt, ih_pt, _cap_h, _tot_h)) = layout::image_box(doc, pfmt.image_id, avail, lay.ppi) {
                if let Some(doc_img) = doc.image(pfmt.image_id) {
                    let pad_top = if doc_img.seamless { 1.0 } else { 3.0 } * (lay.ppi / 72.0);
                    if let Some((sw, sh, srgba)) = doc_img.decode_rgba() {
                        canvas.blit_rgba_scaled(
                            sw,
                            sh,
                            &srgba,
                            ox * ppp,
                            (oy + pad_top) * ppp,
                            iw_pt * ppp,
                            ih_pt * ppp,
                        );
                    }
                    if let Some(cg) = caption_galleys.get(&pfmt.image_id) {
                        let cap_x = ox + (iw_pt - cg.rect.width()) * 0.5;
                        let cap_y = oy + pad_top + ih_pt + 5.0 * (lay.ppi / 72.0);
                        canvas.blit_galley(cg, &atlas, cap_x, cap_y, ppp);
                    }
                }
            }
        }

        // Vertically-centered solid and Craft-style left-to-right fading gradient highlight pills
        {
            let mut k = l.start;
            while k < l.end && k < para.styles.len() {
                let st = para.styles[k];
                if let Some(c1) = st.highlight {
                    let c2 = st.highlight_to.unwrap_or(c1);
                    let seg_start = k;
                    k += 1;
                    while k < l.end
                        && k < para.styles.len()
                        && para.styles[k].highlight == Some(c1)
                        && para.styles[k].highlight_to.unwrap_or(c1) == c2
                    {
                        k += 1;
                    }
                    let seg_end = k;
                    let rel_s = (seg_start - l.start).min(l.xs.len() - 1);
                    let rel_e = (seg_end - l.start).min(l.xs.len() - 1);
                    let x0 = ox + l.xs[rel_s];
                    let x1 = ox + l.xs[rel_e];
                    if x1 > x0 {
                        let (y0, y1) = layout::highlight_pill_y(&l.galley, rel_s, rel_e, gh);
                        let pad_x = (gh * 0.12).clamp(2.0 * scale, 5.0 * scale);
                        let radius = ((y1 - y0) * 0.22).clamp(3.5 * scale, 7.0 * scale);
                        canvas.fill_gradient_pill(
                            (x0 - pad_x) * ppp,
                            (oy + y0) * ppp,
                            (x1 + pad_x) * ppp,
                            (oy + y1) * ppp,
                            radius * ppp,
                            c1,
                            c2,
                        );
                    }
                    continue;
                }
                k += 1;
            }
        }

        // Blit shaped text glyphs from the atlas
        canvas.blit_galley(&l.galley, &atlas, ox, oy, ppp);

        // Underline and strikethrough bars (with exact link, checklist, and Craft gradient colors)
        if let Some(placed) = l.galley.rows.first() {
            let glyphs = &placed.row.glyphs;
            let verts = &placed.row.visuals.mesh.vertices;
            let n = (l.end - l.start).min(glyphs.len());
            let checked_checklist = pfmt.list == ListKind::Checklist && pfmt.checked;
            for (k, g) in glyphs.iter().enumerate().take(n) {
                let s = para.styles[l.start + k];
                let has_ul = s.underline || s.link > 0;
                let has_st = s.strike || checked_checklist;
                if !has_ul && !has_st {
                    continue;
                }
                let fv = g.first_vertex as usize;
                let (c1, c2) = if fv + 3 < verts.len() {
                    let vl = verts[fv].color.to_srgba_unmultiplied();
                    let vr = verts[fv + 1].color.to_srgba_unmultiplied();
                    ([vl[0], vl[1], vl[2]], [vr[0], vr[1], vr[2]])
                } else {
                    let base_c = if s.link > 0 && s.color == [0, 0, 0] {
                        [11, 87, 208]
                    } else if checked_checklist && s.color == [0, 0, 0] {
                        [110, 118, 129]
                    } else {
                        s.color
                    };
                    (base_c, base_c)
                };
                let gx0 = (ox + g.pos.x) * ppp;
                let gx1 = (ox + g.pos.x + g.advance_width) * ppp;
                let size_px = (s.size * lay.ppi / 72.0) * ppp;
                let t = (size_px / 16.0).max(1.0);
                let base_y = (oy + g.pos.y) * ppp;
                if has_ul {
                    let uy = base_y + size_px * 0.11;
                    canvas.fill_gradient_pill(gx0, uy, gx1, uy + t, 0.0, c1, c2);
                }
                if has_st {
                    let sy = base_y - size_px * 0.28;
                    canvas.fill_gradient_pill(gx0, sy - t * 0.5, gx1, sy + t * 0.5, 0.0, c1, c2);
                }
            }
        }

        // Checklist checkbox or bullet/number marker
        if pfmt.list == ListKind::Checklist && l.first_in_para {
            let box_s = (13.0 * scale).clamp(10.0, 56.0);
            let bx = (content_x + l.marker_x) * ppp;
            let by = (oy + (gh - box_s) * 0.5) * ppp;
            let bs = box_s * ppp;
            let rad = 3.0 * scale * ppp;
            if pfmt.checked {
                canvas.fill_gradient_pill(bx, by, bx + bs, by + bs, rad, [43, 87, 151], [43, 87, 151]);
                let p1 = (bx + bs * 0.22, by + bs * 0.52);
                let p2 = (bx + bs * 0.44, by + bs * 0.74);
                let p3 = (bx + bs * 0.78, by + bs * 0.28);
                let sw = 1.8 * scale * ppp;
                canvas.draw_line(p1.0, p1.1, p2.0, p2.1, sw, [255, 255, 255]);
                canvas.draw_line(p2.0, p2.1, p3.0, p3.1, sw, [255, 255, 255]);
            } else {
                canvas.stroke_rounded_rect(bx, by, bx + bs, by + bs, rad, 1.5 * scale * ppp, [115, 115, 115]);
            }
        } else if let Some(m) = &l.marker {
            canvas.blit_galley(m, &atlas, content_x + l.marker_x, oy, ppp);
        }
    }

    canvases.into_iter().map(|c| (c.w, c.h, c.rgba)).collect()
}

/// Renders a single page `page_idx` into `(width, height, rgba_bytes)`.
#[cfg(test)]
pub fn render_page_rgba(ctx: &egui::Context, doc: &Document, book: &FontBook, page_idx: usize, dpi_scale: f32) -> Option<(u32, u32, Vec<u8>)> {
    let mut pages = render_pages_rgba(ctx, doc, book, dpi_scale);
    if page_idx < pages.len() {
        Some(pages.swap_remove(page_idx))
    } else {
        None
    }
}

// ── Calibrated sRGB PDF Export (Vector + WYSIWYG Image XObject) ─────────

struct Page {
    ops: Vec<u8>,
    image: Option<(u32, u32, Vec<u8>)>,
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
    fn gradient_pill(&mut self, c1: [u8; 3], c2: [u8; 3], x: f32, y: f32, w: f32, h: f32) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let steps = if c1 == c2 { 1 } else { 16 };
        let sw = w / steps as f32;
        for i in 0..steps {
            let t = if steps <= 1 { 0.0 } else { i as f32 / (steps - 1) as f32 };
            let col = lerp_rgb(c1, c2, t);
            self.rect(col, x + i as f32 * sw, y, sw + 0.15, h);
        }
    }
}

pub fn export(ctx: &egui::Context, doc: &Document, book: &FontBook) -> Vec<u8> {
    export_page_range(ctx, doc, book, None)
}

/// Exports the document (or a `1`-based inclusive page range `Some((first_page, last_page))`) to PDF.
pub fn export_page_range(ctx: &egui::Context, doc: &Document, book: &FontBook, page_range: Option<(usize, usize)>) -> Vec<u8> {
    // Render high-DPI WYSIWYG page bitmaps (192 DPI base × screen scale)
    let rgba_pages = render_pages_rgba(ctx, doc, book, 2.0);

    // 0.75 × 96 = 72 units per inch: one unit is one PDF point.
    let lay = layout::build(ctx, doc, book, 0.75, false);
    let pw = lay.page_size.x;
    let ph = lay.page_size.y;
    let mut pages: Vec<Page> = (0..lay.pages)
        .map(|i| {
            let image = rgba_pages.get(i).map(|(w, h, rgba)| {
                let mut rgb_bytes = Vec::with_capacity((*w as usize) * (*h as usize) * 3);
                for &[r, g, b, _] in rgba.as_chunks::<4>().0 {
                    rgb_bytes.push(r);
                    rgb_bytes.push(g);
                    rgb_bytes.push(b);
                }
                (*w, *h, rgb_bytes)
            });
            Page { ops: Vec::new(), image }
        })
        .collect();
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
        if let Some(pf) = doc.resolved_page_footer(k, lay.pages) {
            if let Some((x0, x1, y_bot, w_pt, col)) = pf.rule {
                page.rect(col, x0, y_bot - w_pt * 0.5, (x1 - x0).max(1.0), w_pt.max(0.5));
            }
            for seg in &pf.segments {
                let total_txt: String = seg.runs.iter().map(|r| r.text.as_str()).collect();
                let dom_size = seg.runs.first().map_or(9.0, |r| r.style.size.max(4.5));
                let est_w = approx_width(&total_txt, dom_size);
                let mut cur_x = match seg.align {
                    Align::Center => ((seg.x_pt + seg.max_x_pt) * 0.5 - est_w * 0.5).max(2.0),
                    Align::Right => (seg.max_x_pt - est_w).max(2.0),
                    Align::Left | Align::Justify => seg.x_pt.max(2.0),
                };
                for run in &seg.runs {
                    let st = run.style;
                    let sz = st.size.max(4.5);
                    page.text(font_res(&st), sz, st.color, cur_x, seg.bottom_pt.max(2.0), &run.text);
                    cur_x += approx_width(&run.text, sz);
                }
            }
        } else {
            let mut foot = doc.footer.clone();
            if pg.page_numbers {
                let pn = format!("Page {}", k + 1);
                foot = if foot.is_empty() { pn } else { format!("{foot}    {pn}") };
            }
            if !foot.is_empty() {
                let bottom = ph - lay.content_offset.y - lay.content_size.y;
                let fw = approx_width(&foot, 10.0);
                let fx = match doc.footer_align {
                    Align::Left | Align::Justify => lay.content_offset.x,
                    Align::Center => (pw - fw) * 0.5,
                    Align::Right => (lay.content_offset.x + lay.content_size.x - fw).max(lay.content_offset.x),
                };
                let fid = if doc.footer_italic { 6 } else { 4 };
                let fy = (bottom * 0.42 - 3.5).clamp(6.0, 28.0);
                page.text(fid, 10.0, gray, fx, fy, &foot);
            }
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

        // Paragraph shading
        if let Some(sh) = para.fmt.shading {
            page.rect(sh, lay.content_offset.x - 4.5, ph - (oy + l.height + 0.75), lay.content_size.x + 9.0, l.height + 1.5);
        }

        // Contiguous highlight pill runs (interpolated across each highlight segment, not the whole line!)
        let mut hk = 0;
        while hk < n {
            let st = para.styles[l.start + hk];
            if let Some(c1) = st.highlight {
                let c2 = st.highlight_to.unwrap_or(c1);
                let seg_s = hk;
                hk += 1;
                while hk < n
                    && para.styles[l.start + hk].highlight == Some(c1)
                    && para.styles[l.start + hk].highlight_to.unwrap_or(c1) == c2
                {
                    hk += 1;
                }
                let seg_e = hk;
                let (hy0, hy1) = layout::highlight_pill_y(&l.galley, seg_s, seg_e, gh);
                let x0 = ox + glyphs[seg_s].pos.x - 2.2;
                let x1 = ox + glyphs[seg_e - 1].pos.x + glyphs[seg_e - 1].advance_width + 2.2;
                page.gradient_pill(c1, c2, x0, ph - (oy + hy1), (x1 - x0).max(1.0), (hy1 - hy0).max(1.0));
                continue;
            }
            hk += 1;
        }

        // Compute per-character text colors (supporting Craft-style left-to-right gradient text runs)
        let mut char_colors: Vec<[u8; 3]> = (0..n).map(|i| para.styles[l.start + i].color).collect();
        let mut gk = 0;
        while gk < n {
            let st = para.styles[l.start + gk];
            if let Some(c2) = st.color_to {
                if c2 != st.color {
                    let c1 = st.color;
                    let gs = gk;
                    gk += 1;
                    while gk < n && para.styles[l.start + gk].color == c1 && para.styles[l.start + gk].color_to == Some(c2) {
                        gk += 1;
                    }
                    let ge = gk;
                    let span = (ge - gs).saturating_sub(1).max(1) as f32;
                    for (idx, dst) in char_colors[gs..ge].iter_mut().enumerate() {
                        *dst = lerp_rgb(c1, c2, idx as f32 / span);
                    }
                    continue;
                }
            }
            gk += 1;
        }

        // Words: runs of non-space glyphs with the same style & resolved color
        let mut k = 0;
        while k < n {
            let s = para.styles[l.start + k];
            let col = char_colors[k];
            if glyphs[k].chr.is_whitespace() {
                k += 1;
                continue;
            }
            let start = k;
            let mut word = String::new();
            while k < n && !glyphs[k].chr.is_whitespace() && para.styles[l.start + k] == s && char_colors[k] == col {
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
            page.text(font_res(&s), size, col, ox + g0.pos.x, ph - base, &word);
        }

        // Underline and strikethrough bars
        let mut k = 0;
        while k < n {
            let s = para.styles[l.start + k];
            let col = char_colors[k];
            if !(s.underline || s.strike) {
                k += 1;
                continue;
            }
            let start = k;
            while k < n
                && para.styles[l.start + k].underline == s.underline
                && para.styles[l.start + k].strike == s.strike
                && char_colors[k] == col
            {
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
                page.rect(col, x0, ph - (base + s.size * 0.12) - t, x1 - x0, t);
            }
            if s.strike {
                page.rect(col, x0, ph - (base - s.size * 0.28) - t / 2.0, x1 - x0, t);
            }
        }

        // Bullet or number marker
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

    // Overlay the high-DPI WYSIWYG rasterized page image XObject (`/Im1 Do`) so visual appearance
    // in PDF viewers and CUPS printers is 100% identical to the screen while keeping the vector
    // text layer underneath selectable and searchable.
    for page in &mut pages {
        if page.image.is_some() {
            page.push(&format!("q {pw:.2} 0 0 {ph:.2} 0 0 cm /Im1 Do Q\n"));
        }
    }

    if let Some((first_1, last_1)) = page_range {
        let total = pages.len().max(1);
        let s = first_1.saturating_sub(1).min(total - 1);
        let e = last_1.clamp(1, total);
        if s < e {
            pages = pages.into_iter().skip(s).take(e - s).collect();
        }
    }

    write_pdf(&pages, pw, ph, &doc.info.title, &doc.info.author)
}

fn write_pdf(pages: &[Page], pw: f32, ph: f32, title: &str, author: &str) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    let n_fonts = 12;
    // Object numbers:
    // 1: Catalog, 2: Pages, 3: Info, 4: CalRGB sRGB ColorSpace, 5..17: 12 standard fonts,
    // then per page (3 objects): Page, Content stream, Image XObject.
    let cs_obj = 4;
    let font0 = 5;
    let page0 = font0 + n_fonts;
    let stride = 3;
    let obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(out.len());
        let num = offsets.len();
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>");
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", page0 + i * stride)).collect();
    obj(&mut out, &mut offsets, format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()).as_bytes());
    let mut info = b"<< /Producer (Syllabary) ".to_vec();
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
    // Object 4: Calibrated IEC 61966-2-1 sRGB color space
    obj(
        &mut out,
        &mut offsets,
        b"[/CalRGB << /WhitePoint [0.9505 1.0 1.0890] /Gamma [2.2 2.2 2.2] /Matrix [0.4124 0.2126 0.0193 0.3576 0.7152 0.1192 0.1805 0.0722 0.9505] >>]",
    );
    for fam in FAMILIES {
        for name in fam {
            obj(&mut out, &mut offsets, format!("<< /Type /Font /Subtype /Type1 /BaseFont /{name} /Encoding /WinAnsiEncoding >>").as_bytes());
        }
    }
    let fonts: Vec<String> = (0..n_fonts).map(|i| format!("/F{} {} 0 R", i + 1, font0 + i)).collect();
    for (i, p) in pages.iter().enumerate() {
        let content_num = page0 + i * stride + 1;
        let img_num = page0 + i * stride + 2;
        obj(
            &mut out,
            &mut offsets,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] /Resources << /ColorSpace << /DefaultRGB {cs_obj} 0 R /CSRGB {cs_obj} 0 R >> /Font << {} >> /XObject << /Im1 {img_num} 0 R >> >> /Contents {content_num} 0 R >>",
                fonts.join(" ")
            )
            .as_bytes(),
        );
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(&p.ops, 6);
        let mut body = format!("<< /Length {} /Filter /FlateDecode >>\nstream\n", packed.len()).into_bytes();
        body.extend_from_slice(&packed);
        body.extend_from_slice(b"\nendstream");
        obj(&mut out, &mut offsets, &body);

        if let Some((iw, ih, ref rgb_bytes)) = p.image {
            let packed_img = miniz_oxide::deflate::compress_to_vec_zlib(rgb_bytes, 6);
            let mut img_body = format!(
                "<< /Type /XObject /Subtype /Image /Width {iw} /Height {ih} /ColorSpace {cs_obj} 0 R /BitsPerComponent 8 /Interpolate true /Filter /FlateDecode /Length {} >>\nstream\n",
                packed_img.len()
            )
            .into_bytes();
            img_body.extend_from_slice(&packed_img);
            img_body.extend_from_slice(b"\nendstream");
            obj(&mut out, &mut offsets, &img_body);
        } else {
            obj(&mut out, &mut offsets, b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length 3 >>\nstream\n\xff\xff\xff\nendstream");
        }
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
        assert!(s.contains("/CalRGB"), "calibrated sRGB color space");
        let x = s.rfind("startxref").unwrap();
        let xref: usize = s[x + 10..].lines().next().unwrap().trim().parse().unwrap();
        assert!(bytes[xref..].starts_with(b"xref"));
    }

    #[test]
    fn wysiwyg_render_preserves_gradient_pill_and_text_colors() {
        let ctx = egui::Context::default();
        let book = FontBook::default();
        let mut doc = Document::default();
        doc.insert_text(Pos::new(0, 0), "Lead Designer: ", CharStyle::default());
        doc.insert_text(
            Pos::new(0, 15),
            "Andrew Thomas",
            CharStyle {
                color: [234, 88, 12],
                color_to: Some([219, 39, 119]),
                highlight: Some([254, 240, 138]),
                highlight_to: Some([251, 207, 232]),
                ..Default::default()
            },
        );
        let mut rendered = None;
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            rendered = render_page_rgba(&ctx, &doc, &book, 0, 1.5);
        });
        out.textures_delta.clear();
        let (w, h, rgba) = rendered.expect("page 0 rendered");
        assert!(w > 200 && h > 200);
        // Verify both warm yellow highlight pixels [254, 240, 138] and pink highlight pixels [251, 207, 232] exist
        let mut found_yellow = false;
        let mut found_pink = false;
        for &[r, g, b, _] in rgba.as_chunks::<4>().0 {
            if (r as i32 - 254).abs() <= 4 && (g as i32 - 240).abs() <= 8 && (b as i32 - 138).abs() <= 16 {
                found_yellow = true;
            }
            if (r as i32 - 251).abs() <= 4 && (g as i32 - 207).abs() <= 8 && (b as i32 - 232).abs() <= 16 {
                found_pink = true;
            }
        }
        assert!(found_yellow, "left end of gradient highlight pill must be warm yellow");
        assert!(found_pink, "right end of gradient highlight pill must be soft pink");
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

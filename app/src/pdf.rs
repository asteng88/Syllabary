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
    type RichFooterLayout = (Option<(f32, f32, f32, f32, [u8; 3])>, Vec<(FooterSegment, Arc<Galley>)>);
    let (header_galley, rich_footer_galleys, footer_galleys, caption_galleys) = ctx.fonts_mut(|fonts| {
        let hg = if !doc.header.is_empty() { Some(layout::hf_galley(fonts, book, doc, false, &doc.header, lay.ppi)) } else { None };
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
            let mut foot = crate::doc::update_footer_page_nums(&doc.footer, k + 1, lay.pages);
            if doc.page.page_numbers {
                let pn = format!("Page {}", k + 1);
                foot = if foot.is_empty() { pn } else { format!("{foot}    {pn}") };
            }
            if !foot.is_empty() {
                fgs.push(Some(layout::hf_galley(fonts, book, doc, true, &foot, lay.ppi)));
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
            let hy = layout::hf_y(doc, &lay, false, gh);
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
            let fy = layout::hf_y(doc, &lay, true, gh);
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
            let (bx0, bx1) = if pfmt.table_id > 0 { (0.0, lay.content_size.x) } else { layout::para_block_x(doc, &lay, l.para) };
            let x0 = (content_x + bx0 - 6.0 * scale) * ppp;
            let y0 = (oy - 1.0 * scale) * ppp;
            let x1 = (content_x + bx1 + 6.0 * scale) * ppp;
            let y1 = (oy + l.height + 1.0 * scale) * ppp;
            canvas.fill_gradient_pill(x0, y0, x1, y1, 4.0 * scale * ppp, sh, sh);
        }
        // Paragraph borders
        for (r, c) in layout::para_border_rects(doc, &lay, l) {
            let r = r.translate(egui::vec2(content_x, lay.content_offset.y));
            canvas.fill_gradient_pill(r.min.x * ppp, r.min.y * ppp, r.max.x * ppp, r.max.y * ppp, 0.0, c, c);
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
            let indent = doc.block_left_in(&pfmt).max(0.0) * lay.ppi + pfmt.right_in * lay.ppi;
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
        for (r, rad, c) in layout::leader_marks(l) {
            canvas.fill_gradient_pill((ox + r.min.x) * ppp, (oy + r.min.y) * ppp, (ox + r.max.x) * ppp, (oy + r.max.y) * ppp, rad * ppp, c, c);
        }

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



// ── True Vector PDF Export (krilla: embedded subsetted fonts, vector gradients, links & outline) ──

use std::collections::HashMap;
use krilla::action::{Action, LinkAction};
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb as krgb;
use krilla::destination::XyzDestination;
use krilla::geom::{PathBuilder, Point, Rect as KRect, Size, Transform};
use krilla::image::Image as KImage;
use krilla::metadata::Metadata;
use krilla::num::NormalizedF32;
use krilla::outline::{Outline, OutlineNode};
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, LinearGradient, Paint, SpreadMethod, Stop, Stroke as KStroke};
use krilla::surface::Surface;
use krilla::text::{Font as KFont, TextDirection};

fn kcolor(c: [u8; 3]) -> krgb::Color {
    krgb::Color::new(c[0], c[1], c[2])
}

fn fill_solid(c: [u8; 3]) -> Fill {
    Fill {
        paint: kcolor(c).into(),
        opacity: NormalizedF32::ONE,
        rule: FillRule::NonZero,
    }
}

fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Option<krilla::geom::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let mut pb = PathBuilder::new();
    pb.move_to(x, y);
    pb.line_to(x + w, y);
    pb.line_to(x + w, y + h);
    pb.line_to(x, y + h);
    pb.close();
    pb.finish()
}

fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<krilla::geom::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    if r <= 0.25 {
        return rect_path(x, y, w, h);
    }
    let k = r * 0.552_284_8;
    let x1 = x + w;
    let y1 = y + h;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x1 - r, y);
    pb.cubic_to(x1 - r + k, y, x1, y + r - k, x1, y + r);
    pb.line_to(x1, y1 - r);
    pb.cubic_to(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1);
    pb.line_to(x + r, y1);
    pb.cubic_to(x + r - k, y1, x, y1 - r + k, x, y1 - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn draw_rect(surface: &mut Surface<'_>, c: [u8; 3], x: f32, y: f32, w: f32, h: f32) {
    if let Some(path) = rect_path(x, y, w, h) {
        surface.set_stroke(None);
        surface.set_fill(Some(fill_solid(c)));
        surface.draw_path(&path);
    }
}

fn draw_gradient_pill(
    surface: &mut Surface<'_>,
    c1: [u8; 3],
    c2: [u8; 3],
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
) {
    let Some(path) = rounded_rect_path(x, y, w, h, radius) else {
        return;
    };
    surface.set_stroke(None);
    if c1 == c2 {
        surface.set_fill(Some(fill_solid(c1)));
    } else {
        let grad = LinearGradient {
            x1: x,
            y1: y,
            x2: x + w,
            y2: y,
            transform: Transform::identity(),
            spread_method: SpreadMethod::Pad,
            stops: vec![
                Stop {
                    offset: NormalizedF32::ZERO,
                    color: kcolor(c1).into(),
                    opacity: NormalizedF32::ONE,
                },
                Stop {
                    offset: NormalizedF32::ONE,
                    color: kcolor(c2).into(),
                    opacity: NormalizedF32::ONE,
                },
            ],
            anti_alias: true,
        };
        surface.set_fill(Some(Fill {
            paint: Paint::from(grad),
            opacity: NormalizedF32::ONE,
            rule: FillRule::NonZero,
        }));
    }
    surface.draw_path(&path);
}

struct FontCache {
    loaded: HashMap<(u16, bool, bool), Option<KFont>>,
    fallback: Option<KFont>,
}

impl FontCache {
    fn new() -> Self {
        let fallback = crate::fonts::font_file_bytes(crate::doc::DEFAULT_FONT, false, false)
            .and_then(|(bytes, _, _)| KFont::new(bytes.into(), 0));
        Self {
            loaded: HashMap::new(),
            fallback,
        }
    }

    fn get(&mut self, font: u16, bold: bool, italic: bool) -> Option<KFont> {
        if let Some(cached) = self.loaded.get(&(font, bold, italic)) {
            return cached.clone().or_else(|| self.fallback.clone());
        }
        let resolved = crate::fonts::font_file_bytes(font, bold, italic)
            .and_then(|(bytes, _, _)| KFont::new(bytes.into(), 0));
        self.loaded.insert((font, bold, italic), resolved.clone());
        resolved.or_else(|| self.fallback.clone())
    }
}

pub fn export(ctx: &egui::Context, doc: &Document, book: &FontBook) -> Vec<u8> {
    export_page_range(ctx, doc, book, None)
}

/// Exports the document (or a `1`-based inclusive page range `Some((first_page, last_page))`)
/// as a true vector PDF via `krilla` with embedded subsetted TrueType/OpenType fonts,
/// selectable full-Unicode text, vector gradients, clickable links, metadata, and heading outline.
pub fn export_page_range(
    ctx: &egui::Context,
    doc: &Document,
    book: &FontBook,
    page_range: Option<(usize, usize)>,
) -> Vec<u8> {
    // 0.75 × 96 = 72 units per inch: one layout unit is exactly one PDF point.
    let lay = layout::build(ctx, doc, book, 0.75, false);
    let pw = lay.page_size.x.max(1.0);
    let ph = lay.page_size.y.max(1.0);
    let total_pages = lay.pages.max(1);
    let (start_p, end_p) = match page_range {
        Some((f1, l1)) => {
            let s = f1.saturating_sub(1).min(total_pages - 1);
            let e = l1.clamp(1, total_pages);
            if s < e { (s, e) } else { (0, total_pages) }
        }
        None => (0, total_pages),
    };

    // Measure headers/footers with the exact egui font galleys for pixel-aligned centering
    let mut header_galley: Option<Arc<Galley>> = None;
    let mut footer_galleys: Vec<Option<Arc<Galley>>> = vec![None; total_pages];
    let mut caption_galleys: HashMap<u64, Arc<Galley>> = HashMap::new();
    ctx.fonts_mut(|fonts| {
        if !doc.header.is_empty() {
            header_galley = Some(layout::hf_galley(fonts, book, doc, false, &doc.header, lay.ppi));
        }
        for k in start_p..end_p {
            if doc.resolved_page_footer(k, total_pages).is_none() {
                let mut foot = crate::doc::update_footer_page_nums(&doc.footer, k + 1, total_pages);
                if doc.page.page_numbers {
                    let pn = format!("Page {}", k + 1);
                    foot = if foot.is_empty() { pn } else { format!("{foot}    {pn}") };
                }
                if !foot.is_empty() {
                    footer_galleys[k] = Some(layout::hf_galley(fonts, book, doc, true, &foot, lay.ppi));
                }
            }
        }
        for img in &doc.images {
            let cap = img.caption.trim();
            if !cap.is_empty() {
                let mut job = LayoutJob::default();
                let st = CharStyle {
                    font: crate::doc::DEFAULT_FONT,
                    size: 9.5,
                    italic: true,
                    color: [100, 108, 120],
                    ..CharStyle::default()
                };
                job.append(cap, 0.0, layout::text_format(book, &st, lay.ppi));
                caption_galleys.insert(img.id, fonts.layout_job(job));
            }
        }
    });

    let mut kdoc = krilla::Document::new();
    let mut meta = Metadata::new().producer("Syllabary".to_string());
    if !doc.info.title.is_empty() {
        meta = meta.title(doc.info.title.clone());
    }
    if !doc.info.author.is_empty() {
        meta = meta.authors(vec![doc.info.author.clone()]);
    }
    kdoc.set_metadata(meta);

    let mut fonts = FontCache::new();
    let mut outline = Outline::new();
    let mut has_outline = false;
    let mut img_cache: HashMap<u64, KImage> = HashMap::new();
    let pg = doc.page;

    for (out_page_idx, page_idx) in (start_p..end_p).enumerate() {
        let Some(settings) = PageSettings::from_wh(pw, ph).or_else(|| PageSettings::from_wh(612.0, 792.0)) else {
            break;
        };
        let mut page = kdoc.start_page_with(settings);
        let mut page_links: Vec<(KRect, String)> = Vec::new();
        {
            let mut surface = page.surface();

            // 1. Page background
            if pg.background != [255, 255, 255] {
                draw_rect(&mut surface, pg.background, 0.0, 0.0, pw, ph);
            }

            // 2. Table cell shading & borders
            for cell in lay.cells.iter().filter(|c| c.page == page_idx) {
                let x0 = lay.content_offset.x + cell.rect.min.x;
                let y0 = lay.content_offset.y + cell.rect.min.y;
                let cw = cell.rect.width();
                let ch = cell.rect.height();
                if let Some(sh) = cell.shading {
                    draw_rect(&mut surface, sh, x0, y0, cw, ch);
                }
                let b = cell.borders;
                if let Some(spec) = b.top {
                    let w = spec.width_pt.max(0.5);
                    draw_rect(&mut surface, spec.color, x0, y0 - w * 0.5, cw, w);
                }
                if let Some(spec) = b.bottom {
                    let w = spec.width_pt.max(0.5);
                    draw_rect(&mut surface, spec.color, x0, y0 + ch - w * 0.5, cw, w);
                }
                if let Some(spec) = b.left {
                    let w = spec.width_pt.max(0.5);
                    draw_rect(&mut surface, spec.color, x0 - w * 0.5, y0, w, ch);
                }
                if let Some(spec) = b.right {
                    let w = spec.width_pt.max(0.5);
                    draw_rect(&mut surface, spec.color, x0 + cw - w * 0.5, y0, w, ch);
                }
            }

            // 3. Lines on this page
            for l in lay.lines.iter().filter(|l| l.page == page_idx) {
                let para = &doc.paras[l.para];
                let pfmt = para.fmt;
                let ox = lay.content_offset.x + l.x;
                let oy = lay.content_offset.y + l.y;

                // Record Heading 1/2/3 bookmarks for PDF Outline
                if l.first_in_para && matches!(pfmt.style, 4..=6) {
                    let heading = para.string();
                    let trimmed = heading.trim();
                    if !trimmed.is_empty() {
                        has_outline = true;
                        outline.push_child(OutlineNode::new(
                            trimmed.to_string(),
                            XyzDestination::new(out_page_idx, Point::from_xy(ox, oy)),
                        ));
                    }
                }

                // Paragraph shading & borders
                if let Some(sh) = pfmt.shading {
                    if pfmt.table_id == 0 {
                        let (bx0, bx1) = layout::para_block_x(doc, &lay, l.para);
                        draw_rect(
                            &mut surface,
                            sh,
                            lay.content_offset.x + bx0 - 4.0,
                            oy - 0.5,
                            (bx1 - bx0) + 8.0,
                            l.height + 1.0,
                        );
                    }
                }
                for (r, col) in layout::para_border_rects(doc, &lay, l) {
                    draw_rect(
                        &mut surface,
                        col,
                        lay.content_offset.x + r.min.x,
                        lay.content_offset.y + r.min.y,
                        r.width(),
                        r.height(),
                    );
                }

                // Horizontal rule
                if pfmt.hr && l.first_in_para {
                    let hr_col = pfmt.hr_color.unwrap_or([180, 184, 192]);
                    let w = if pfmt.hr_width_pt > 0.0 { pfmt.hr_width_pt } else { 1.0 };
                    let y_mid = oy + l.height * 0.5;
                    draw_rect(&mut surface, hr_col, lay.content_offset.x, y_mid - w * 0.5, lay.content_size.x, w);
                }

                // Embedded image + caption
                if pfmt.image_id > 0 && l.first_in_para {
                    let indent = doc.block_left_in(&pfmt).max(0.0) * lay.ppi + pfmt.right_in * lay.ppi;
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
                            let pad_top = if doc_img.seamless { 1.0 } else { 3.0 };
                            let kimg = img_cache.entry(pfmt.image_id).or_insert_with(|| {
                                let (sw, sh, rgba) = doc_img.decode_rgba().unwrap_or((1, 1, vec![255, 255, 255, 255]));
                                KImage::from_rgba8(rgba, sw, sh)
                            });
                            if let Some(sz) = Size::from_wh(iw_pt, ih_pt) {
                                surface.push_transform(&Transform::from_translate(ox, oy + pad_top));
                                surface.draw_image(kimg.clone(), sz);
                                surface.pop();
                            }
                            if let Some(cg) = caption_galleys.get(&pfmt.image_id) {
                                if let Some(fnt) = fonts.get(crate::doc::DEFAULT_FONT, false, true) {
                                    let cap_x = ox + (iw_pt - cg.rect.width()) * 0.5;
                                    let cap_y = oy + pad_top + ih_pt + 13.0;
                                    surface.set_stroke(None);
                                    surface.set_fill(Some(fill_solid([100, 108, 120])));
                                    surface.draw_text(
                                        Point::from_xy(cap_x, cap_y),
                                        fnt,
                                        9.5,
                                        doc_img.caption.trim(),
                                        false,
                                        TextDirection::Auto,
                                    );
                                }
                            }
                        }
                    }
                    continue;
                }

                let Some(row) = l.galley.rows.first() else { continue };
                let glyphs = &row.row.glyphs;
                let gh = l.galley.rect.height();
                let n = (l.end - l.start).min(glyphs.len());

                // Contiguous highlight pills (solid & Craft left-to-right gradient)
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
                        let pad_x = (gh * 0.12).clamp(1.5, 4.0);
                        let x0 = ox + glyphs[seg_s].pos.x - pad_x;
                        let x1 = ox + glyphs[seg_e - 1].pos.x + glyphs[seg_e - 1].advance_width + pad_x;
                        let radius = ((hy1 - hy0) * 0.22).clamp(2.5, 5.5);
                        draw_gradient_pill(&mut surface, c1, c2, x0, oy + hy0, (x1 - x0).max(1.0), (hy1 - hy0).max(1.0), radius);
                        continue;
                    }
                    hk += 1;
                }

                // Tab leaders
                for (r, rad, c) in layout::leader_marks(l) {
                    draw_gradient_pill(&mut surface, c, c, ox + r.min.x, oy + r.min.y, r.width(), r.height(), rad);
                }

                // Resolve per-character colors (handling Craft left-to-right gradient text, links, checklists)
                let checked_checklist = pfmt.list == ListKind::Checklist && pfmt.checked;
                let mut char_colors: Vec<[u8; 3]> = (0..n)
                    .map(|i| {
                        let s = para.styles[l.start + i];
                        if s.link > 0 && s.color == [0, 0, 0] {
                            [11, 87, 208]
                        } else if checked_checklist && s.color == [0, 0, 0] {
                            [110, 118, 129]
                        } else {
                            s.color
                        }
                    })
                    .collect();
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

                // Draw text runs using real embedded subsetted TTF/OTF fonts
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
                    let mut size = s.size.max(1.0);
                    let mut base_y = oy + g0.pos.y;
                    if s.caps == 2 && para.text.get(l.start + start).is_some_and(|c| c.is_lowercase()) {
                        size *= 0.8;
                    }
                    if s.script != 0 {
                        size *= 0.65;
                        if s.script > 0 {
                            base_y -= s.size * 0.33;
                        } else {
                            base_y += s.size * 0.12;
                        }
                    }
                    if let Some(fnt) = fonts.get(s.font, s.bold, s.italic) {
                        surface.set_stroke(None);
                        surface.set_fill(Some(fill_solid(col)));
                        surface.draw_text(
                            Point::from_xy(ox + g0.pos.x, base_y),
                            fnt,
                            size,
                            &word,
                            false,
                            TextDirection::Auto,
                        );
                    }
                    if s.link > 0 {
                        if let Some(url) = doc.link_url(s.link) {
                            let gx0 = ox + g0.pos.x;
                            let gx1 = ox + glyphs[k - 1].pos.x + glyphs[k - 1].advance_width;
                            if let Some(r) = KRect::from_ltrb(gx0, oy, gx1.max(gx0 + 1.0), oy + l.height) {
                                page_links.push((r, url.to_string()));
                            }
                        }
                    }
                }

                // Underline and strikethrough bars
                let mut uk = 0;
                while uk < n {
                    let s = para.styles[l.start + uk];
                    let col = char_colors[uk];
                    let has_ul = s.underline || s.link > 0;
                    let has_st = s.strike || checked_checklist;
                    if !has_ul && !has_st {
                        uk += 1;
                        continue;
                    }
                    let start = uk;
                    while uk < n
                        && (para.styles[l.start + uk].underline || para.styles[l.start + uk].link > 0) == has_ul
                        && (para.styles[l.start + uk].strike || checked_checklist) == has_st
                        && char_colors[uk] == col
                    {
                        uk += 1;
                    }
                    let mut end = uk;
                    while end > start + 1 && glyphs[end - 1].chr.is_whitespace() {
                        end -= 1;
                    }
                    let x0 = ox + glyphs[start].pos.x;
                    let x1 = ox + glyphs[end - 1].pos.x + glyphs[end - 1].advance_width;
                    let base_y = oy + glyphs[start].pos.y;
                    let t = (s.size / 16.0).max(0.5);
                    if has_ul {
                        draw_rect(&mut surface, col, x0, base_y + s.size * 0.11, (x1 - x0).max(0.5), t);
                    }
                    if has_st {
                        draw_rect(&mut surface, col, x0, base_y - s.size * 0.28 - t * 0.5, (x1 - x0).max(0.5), t);
                    }
                }

                // Checklist checkbox or bullet/number marker
                if pfmt.list == ListKind::Checklist && l.first_in_para {
                    let bs = 9.5f32;
                    let bx = lay.content_offset.x + l.marker_x;
                    let by = oy + (gh - bs) * 0.5;
                    let rad = 2.0f32;
                    if pfmt.checked {
                        draw_gradient_pill(&mut surface, [43, 87, 151], [43, 87, 151], bx, by, bs, bs, rad);
                        let mut pb = PathBuilder::new();
                        pb.move_to(bx + bs * 0.22, by + bs * 0.52);
                        pb.line_to(bx + bs * 0.44, by + bs * 0.74);
                        pb.line_to(bx + bs * 0.78, by + bs * 0.28);
                        if let Some(p) = pb.finish() {
                            surface.set_fill(None);
                            surface.set_stroke(Some(KStroke {
                                paint: kcolor([255, 255, 255]).into(),
                                width: 1.3,
                                ..Default::default()
                            }));
                            surface.draw_path(&p);
                        }
                    } else if let Some(p) = rounded_rect_path(bx, by, bs, bs, rad) {
                        surface.set_fill(None);
                        surface.set_stroke(Some(KStroke {
                            paint: kcolor([115, 115, 115]).into(),
                            width: 1.0,
                            ..Default::default()
                        }));
                        surface.draw_path(&p);
                    }
                } else if let Some(m) = &l.marker {
                    if let Some(mrow) = m.rows.first() {
                        let s = para.style_at(0);
                        let txt: String = mrow.row.glyphs.iter().map(|g| g.chr).collect();
                        let base_y = oy + mrow.row.glyphs.first().map_or(gh * 0.8, |g| g.pos.y);
                        let mx = lay.content_offset.x + l.marker_x;
                        if let Some(fnt) = fonts.get(s.font, false, false) {
                            surface.set_stroke(None);
                            surface.set_fill(Some(fill_solid(s.color)));
                            surface.draw_text(
                                Point::from_xy(mx, base_y),
                                fnt,
                                s.size.max(1.0),
                                &txt,
                                false,
                                TextDirection::Auto,
                            );
                        }
                    }
                }
            }

            // 4. Header & Footer
            if let Some(hg) = &header_galley {
                let st = layout::hf_style(doc, false);
                let hw = hg.rect.width();
                let hx = match doc.header_align {
                    Align::Left | Align::Justify => lay.content_offset.x,
                    Align::Center => (pw - hw) * 0.5,
                    Align::Right => (lay.content_offset.x + lay.content_size.x - hw).max(lay.content_offset.x),
                };
                let top = layout::hf_y(doc, &lay, false, hg.rect.height());
                if let Some(fnt) = fonts.get(st.font, st.bold, st.italic) {
                    surface.set_stroke(None);
                    surface.set_fill(Some(fill_solid(st.color)));
                    surface.draw_text(
                        Point::from_xy(hx, top + st.size * 0.9),
                        fnt,
                        st.size.max(1.0),
                        &doc.header,
                        false,
                        TextDirection::Auto,
                    );
                }
            }
            if let Some(pf) = doc.resolved_page_footer(page_idx, total_pages) {
                if let Some((x0, x1, y_bot, w_pt, col)) = pf.rule {
                    let y_top = ph - y_bot;
                    draw_rect(&mut surface, col, x0, y_top - w_pt * 0.5, (x1 - x0).max(1.0), w_pt.max(0.5));
                }
                for seg in &pf.segments {
                    let total_txt: String = seg.runs.iter().map(|r| r.text.as_str()).collect();
                    let dom_size = seg.runs.first().map_or(9.0, |r| r.style.size.max(4.5));
                    let est_w = total_txt.chars().count() as f32 * dom_size * 0.5;
                    let mut cur_x = match seg.align {
                        Align::Center => ((seg.x_pt + seg.max_x_pt) * 0.5 - est_w * 0.5).max(2.0),
                        Align::Right => (seg.max_x_pt - est_w).max(2.0),
                        Align::Left | Align::Justify => seg.x_pt.max(2.0),
                    };
                    let base_y = ph - seg.bottom_pt.max(2.0);
                    for run in &seg.runs {
                        let st = run.style;
                        let sz = st.size.max(4.5);
                        if let Some(fnt) = fonts.get(st.font, st.bold, st.italic) {
                            surface.set_stroke(None);
                            surface.set_fill(Some(fill_solid(st.color)));
                            surface.draw_text(
                                Point::from_xy(cur_x, base_y),
                                fnt,
                                sz,
                                &run.text,
                                false,
                                TextDirection::Auto,
                            );
                        }
                        cur_x += run.text.chars().count() as f32 * sz * 0.5;
                    }
                }
            } else if let Some(fg) = &footer_galleys[page_idx] {
                let mut foot = crate::doc::update_footer_page_nums(&doc.footer, page_idx + 1, total_pages);
                if pg.page_numbers {
                    let pn = format!("Page {}", page_idx + 1);
                    foot = if foot.is_empty() { pn } else { format!("{foot}    {pn}") };
                }
                let st = layout::hf_style(doc, true);
                let fw = fg.rect.width();
                let fx = match doc.footer_align {
                    Align::Left | Align::Justify => lay.content_offset.x,
                    Align::Center => (pw - fw) * 0.5,
                    Align::Right => (lay.content_offset.x + lay.content_size.x - fw).max(lay.content_offset.x),
                };
                let top = layout::hf_y(doc, &lay, true, fg.rect.height());
                if let Some(fnt) = fonts.get(st.font, st.bold, st.italic) {
                    surface.set_stroke(None);
                    surface.set_fill(Some(fill_solid(st.color)));
                    surface.draw_text(
                        Point::from_xy(fx, top + st.size * 0.9),
                        fnt,
                        st.size.max(1.0),
                        &foot,
                        false,
                        TextDirection::Auto,
                    );
                }
            }

            surface.finish();
        }
        for (rect, url) in page_links {
            page.add_annotation(Annotation::new_link(
                LinkAnnotation::new(rect, Target::Action(Action::Link(LinkAction::new(url)))),
                None,
            ));
        }
        page.finish();
    }

    if has_outline {
        kdoc.set_outline(outline);
    }

    kdoc.finish().unwrap_or_default()
}

// ── Pixel-Accurate PDF Rendering via hayro (ArtCraft PdfCraft engine) ──

/// Returns the number of pages and `(width_pt, height_pt)` of each page in a raw PDF file.
pub fn hayro_pdf_info(pdf_bytes: Arc<Vec<u8>>) -> Option<Vec<(f32, f32)>> {
    let pdf = hayro::hayro_syntax::Pdf::new(pdf_bytes).ok()?;
    Some(pdf.pages().iter().map(|p| p.render_dimensions()).collect())
}

/// Renders a single page (`0`-based `page_idx`) of a raw PDF to `(width, height, unpremultiplied_rgba)`
/// using `hayro` (pure-Rust ISO 32000 PDF interpreter & `vello_cpu` rasterizer).
pub fn hayro_render_page_rgba(
    pdf_bytes: Arc<Vec<u8>>,
    page_idx: usize,
    scale: f32,
) -> Option<(u32, u32, Vec<u8>)> {
    let pdf = hayro::hayro_syntax::Pdf::new(pdf_bytes).ok()?;
    let page = pdf.pages().get(page_idx)?;
    let cache = hayro::RenderCache::new();
    let interp = hayro::hayro_interpret::InterpreterSettings::default();
    let rset = hayro::RenderSettings::default();
    let pset = hayro::PixmapSettings {
        x_scale: scale.clamp(0.25, 4.0),
        y_scale: scale.clamp(0.25, 4.0),
        bg_color: hayro::vello_cpu::color::palette::css::WHITE,
    };
    let pixmap = hayro::render(page, &cache, &interp, &rset, &pset);
    let w = pixmap.width() as u32;
    let h = pixmap.height() as u32;
    // With opaque WHITE bg_color every pixel has alpha = 255, so premultiplied == unpremultiplied RGBA.
    let rgba = pixmap.data_as_u8_slice().to_vec();
    Some((w, h, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Pos;

    #[test]
    fn pdf_is_well_formed() {
        let ctx = egui::Context::default();
        let mut book = FontBook::default();
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            book = crate::fonts::install(&ctx);
        });
        out.textures_delta.clear();
        let mut doc = Document::default();
        doc.insert_text(Pos::new(0, 0), &"Hello (world) – “quotes” ".repeat(300), CharStyle { underline: true, ..Default::default() });
        doc.page.page_numbers = true;
        let mut bytes = Vec::new();
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            bytes = export(&ctx, &doc, &book);
        });
        out.textures_delta.clear();
        assert!(bytes.starts_with(b"%PDF-1."), "krilla emits valid PDF header");
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.trim_end().ends_with("%%EOF"), "valid PDF EOF");
        // Verify with hayro that the exported vector PDF renders cleanly with multiple pages
        let arc_bytes = Arc::new(bytes);
        let info = hayro_pdf_info(arc_bytes.clone()).expect("hayro parses krilla PDF");
        assert!(info.len() >= 2, "expected multiple pages, got {}", info.len());
        let (w, h, rgba) = hayro_render_page_rgba(arc_bytes, 0, 1.0).expect("hayro renders page 0");
        assert!(w > 400 && h > 500 && rgba.len() == (w as usize) * (h as usize) * 4);
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

#[cfg(test)]
mod render_probe {
    /// Renders the files listed in SYLLABARY_RENDER (";"-separated) to PNG pages in SYLLABARY_OUT.
    #[test]
    #[ignore]
    fn render_files_to_png() {
        let list = std::env::var("SYLLABARY_RENDER").unwrap_or_default();
        let out_dir = std::env::var("SYLLABARY_OUT").unwrap_or_else(|_| ".".into());
        let ctx = eframe::egui::Context::default();
        let mut book = crate::fonts::FontBook::default();
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            book = crate::fonts::install(&ctx);
        });
        out.textures_delta.clear();
        for (fi, path) in list.split(';').filter(|p| !p.is_empty()).enumerate() {
            let bytes = std::fs::read(path).unwrap();
            let ext = std::path::Path::new(path)
                .extension()
                .unwrap()
                .to_string_lossy()
                .to_lowercase();
            let doc = match ext.as_str() {
                "docx" => crate::docx::read(&bytes).unwrap(),
                "pdf" => crate::pdf_import::read(&bytes).unwrap(),
                "rtf" => {
                    crate::doc::from_rtf(&bytes.iter().map(|&b| b as char).collect::<String>())
                }
                "md" => crate::doc::from_markdown(&crate::doc::decode_text(&bytes)),
                "gdoc" => crate::doc::from_gdoc(&crate::doc::decode_text(&bytes)).unwrap(),
                _ => crate::doc::from_plain(&crate::doc::decode_text(&bytes)),
            };
            let mut pages = Vec::new();
            let mut out = ctx.run_ui(Default::default(), |_ui| {
                pages = super::render_pages_rgba(&ctx, &doc, &book, 1.0);
            });
            out.textures_delta.clear();
            for (pi, (w, h, rgba)) in pages.iter().enumerate().take(3) {
                let img = image::RgbaImage::from_raw(*w, *h, rgba.clone()).unwrap();
                img.save(format!("{out_dir}/f{fi}_p{pi}.png")).unwrap();
            }
            println!("RENDERED {path} pages={}", pages.len());
        }
    }
}

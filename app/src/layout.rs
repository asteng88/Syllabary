//! Turns the document into positioned lines on pages (or continuous Pageless canvas).

use eframe::egui::{
    self,
    text::{LayoutJob, TextFormat, TextWrapping},
    Color32, FontId, Galley, Pos2, Rect, Stroke, Vec2,
};
use std::sync::Arc;

use crate::doc::{
    lerp_rgb, Align, CellBorders, CellVAlign, CharStyle, Document, ListKind, Para, TabKind,
    TabStop, TableCellSpec,
};
use crate::fonts::FontBook;

/// egui points per inch at 100 % zoom.
pub const BASE_PPI: f32 = 96.0;
/// Gap between pages on screen (points, not zoomed).
pub const PAGE_GAP: f32 = 28.0;
/// Hanging indent for bullets, numbers and checklists, in inches.
pub const LIST_HANG: f32 = 0.25;
/// One indent level, in inches.
pub const INDENT_STEP: f32 = 0.5;

#[derive(Clone)]
pub struct Line {
    pub para: usize,
    pub start: usize,
    pub end: usize,
    pub page: usize,
    /// Position of the line's left edge relative to the page's content box.
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub galley: Arc<Galley>,
    /// x of each char boundary (len = end - start + 1), relative to `x`.
    pub xs: Vec<f32>,
    pub first_in_para: bool,
    pub last_in_para: bool,
    pub marker: Option<Arc<Galley>>,
    pub marker_x: f32,
    /// Tab leaders to paint on this line.
    pub leaders: Vec<Leader>,
}

#[derive(Clone, Debug)]
pub struct LayoutCell {
    pub table_id: u32,
    pub row: u16,
    pub col: u16,
    pub page: usize,
    /// Cell rectangle relative to the page's content box.
    pub rect: Rect,
    pub shading: Option<[u8; 3]>,
    pub borders: CellBorders,
    pub wrap_text: bool,
}

pub struct PageLayout {
    pub lines: Vec<Line>,
    pub cells: Vec<LayoutCell>,
    pub pages: usize,
    pub ppi: f32,
    pub page_size: Vec2,
    pub content_offset: Vec2,
    pub content_size: Vec2,
    pub draft: bool,
}

pub fn color(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Paints a rounded horizontal pill that fades smoothly from `c1` (left) to `c2` (right).
pub fn paint_gradient_pill(painter: &egui::Painter, rect: Rect, c1: [u8; 3], c2: [u8; 3], radius: f32) {
    let w = rect.width();
    let h = rect.height();
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    if c1 == c2 {
        painter.rect_filled(rect, radius, color(c1));
        return;
    }
    let r = radius.min(w * 0.5).min(h * 0.5).max(0.0);
    let mut xs: Vec<f32> = Vec::with_capacity(16);
    if r <= 0.5 {
        xs.push(0.0);
        xs.push(w);
    } else {
        let steps = 6;
        for i in 0..=steps {
            xs.push(r * (i as f32 / steps as f32));
        }
        for i in 0..=steps {
            let dx = w - r + r * (i as f32 / steps as f32);
            if dx > xs.last().copied().unwrap_or(0.0) + 0.05 {
                xs.push(dx);
            }
        }
    }
    let mut mesh = egui::Mesh::default();
    let uv = egui::epaint::WHITE_UV;
    for &dx in &xs {
        let dy = if r > 0.5 && dx < r {
            let u = r - dx;
            r - (r * r - u * u).max(0.0).sqrt()
        } else if r > 0.5 && dx > w - r {
            let u = dx - (w - r);
            r - (r * r - u * u).max(0.0).sqrt()
        } else {
            0.0
        };
        let x = rect.min.x + dx;
        let t = (dx / w).clamp(0.0, 1.0);
        let col = color(lerp_rgb(c1, c2, t));
        mesh.vertices.push(egui::epaint::Vertex {
            pos: Pos2::new(x, rect.min.y + dy),
            uv,
            color: col,
        });
        mesh.vertices.push(egui::epaint::Vertex {
            pos: Pos2::new(x, rect.max.y - dy),
            uv,
            color: col,
        });
    }
    for i in 0..xs.len().saturating_sub(1) {
        let b = (i * 2) as u32;
        mesh.indices.extend_from_slice(&[b, b + 2, b + 3, b, b + 3, b + 1]);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Returns `(top_y, bottom_y)` relative to the line origin, vertically centered around the visible
/// glyph ink in `rel_start..rel_end` with symmetric top and bottom padding.
pub fn highlight_pill_y(galley: &Galley, rel_start: usize, rel_end: usize, line_h: f32) -> (f32, f32) {
    let gh = galley.rect.height().max(1.0);
    let pad_y = (gh * 0.16).clamp(2.5, 6.0);
    if let Some(row) = galley.rows.first() {
        let glyphs = &row.row.glyphs;
        let s = rel_start.min(glyphs.len());
        let e = rel_end.min(glyphs.len());
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for gl in &glyphs[s..e] {
            if !gl.chr.is_whitespace() && !gl.uv_rect.is_nothing() {
                let top = gl.pos.y + gl.uv_rect.offset.y;
                let bot = top + gl.uv_rect.size.y;
                min_y = min_y.min(top);
                max_y = max_y.max(bot);
            }
        }
        if min_y <= max_y && min_y.is_finite() && max_y.is_finite() {
            let mid_y = (min_y + max_y) * 0.5;
            let half_h = ((max_y - min_y) * 0.5 + pad_y).max(gh * 0.42);
            return (mid_y - half_h, mid_y + half_h);
        }
    }
    (0.5, (line_h - 0.5).max(1.0))
}

pub fn text_format(book: &FontBook, s: &CharStyle, ppi: f32) -> TextFormat {
    let (family, fake_italic) = book.family(s.font, s.bold, s.italic);
    let mut size = s.size * ppi / 72.0;
    if s.script != 0 {
        size *= 0.65;
    }
    let is_link = s.link > 0;
    let col = if is_link && s.color == [0, 0, 0] {
        Color32::from_rgb(11, 87, 208)
    } else {
        color(s.color)
    };
    let line_w = (size / 16.0).max(1.0);
    TextFormat {
        font_id: FontId::new(size.max(1.0), family),
        color: col,
        // All highlights (solid and gradient) are painted as vertically-centered pills in editor.rs
        background: Color32::TRANSPARENT,
        italics: fake_italic,
        underline: if s.underline || is_link { Stroke::new(line_w, col) } else { Stroke::NONE },
        strikethrough: if s.strike { Stroke::new(line_w, col) } else { Stroke::NONE },
        valign: if s.script > 0 { egui::Align::TOP } else { egui::Align::BOTTOM },
        ..Default::default()
    }
}

fn single_row(galley: &Arc<Galley>, row: usize, para: &Para, start: usize, end: usize) -> Arc<Galley> {
    let mut g: Galley = (**galley).clone();
    let mut placed = g.rows[row].clone();
    placed.pos = Pos2::new(0.0, 0.0);

    // Apply Craft-style left-to-right fading text color across glyph/underline/strikethrough vertices
    if end > start && end <= para.styles.len() && para.styles[start..end].iter().any(|s| s.is_gradient_color()) {
        let row_mut = Arc::make_mut(&mut placed.row);
        let mut k = start;
        while k < end {
            let st = para.styles[k];
            if let Some(c2) = st.color_to {
                if c2 != st.color {
                    let c1 = st.color;
                    let seg_start = k;
                    k += 1;
                    while k < end && para.styles[k].color == c1 && para.styles[k].color_to == Some(c2) {
                        k += 1;
                    }
                    let seg_end = k;
                    let x0 = row_mut.glyphs.get(seg_start - start).map_or(0.0, |gl| gl.pos.x);
                    let x1 = row_mut
                        .glyphs
                        .get(seg_end - start - 1)
                        .map_or(x0 + 1.0, |gl| gl.pos.x + gl.advance_width);
                    let span_w = (x1 - x0).max(1.0);
                    for v in &mut row_mut.visuals.mesh.vertices {
                        if v.pos.x >= x0 - 1.5 && v.pos.x <= x1 + 1.5 {
                            let t = ((v.pos.x - x0) / span_w).clamp(0.0, 1.0);
                            let rgb = lerp_rgb(c1, c2, t);
                            v.color = Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], v.color.a());
                        }
                    }
                    continue;
                }
            }
            k += 1;
        }
    }

    let size = placed.row.size;
    let mesh_bounds = placed.row.visuals.mesh_bounds;
    g.num_vertices = placed.row.visuals.mesh.vertices.len();
    g.num_indices = placed.row.visuals.mesh.indices.len();
    placed.ends_with_newline = false;
    g.rows = vec![placed];
    g.rect = Rect::from_min_size(Pos2::ZERO, size);
    g.mesh_bounds = mesh_bounds;
    Arc::new(g)
}

/// Computes `(img_w, img_h, caption_h, total_h)` in layout points for an embedded image paragraph.
pub fn image_box(doc: &Document, image_id: u64, avail_w: f32, ppi: f32) -> Option<(f32, f32, f32, f32)> {
    let img = doc.image(image_id)?;
    let zoom = (ppi / BASE_PPI).max(0.25);
    let min_w = if img.seamless { 6.0 * zoom } else { 24.0 * zoom };
    let min_h = if img.seamless { 4.0 * zoom } else { 16.0 * zoom };
    let raw_w = (img.width_in * ppi).max(min_w);
    let img_w = raw_w.min(avail_w.max(min_w));
    let scale = if raw_w > 0.0 { img_w / raw_w } else { 1.0 };
    let img_h = (img.height_in * ppi * scale).max(min_h);
    let (cap_h, pad_v) = if !img.caption.trim().is_empty() {
        (22.0 * zoom, 10.0 * zoom)
    } else if img.seamless {
        (0.0, 2.0 * zoom)
    } else {
        (6.0 * zoom, 8.0 * zoom)
    };
    let total_h = img_h + cap_h + pad_v;
    Some((img_w, img_h, cap_h, total_h))
}

/// A tab leader (dots / dashes / line) painted across the gap a tab opened.
#[derive(Clone, Copy, Debug)]
pub struct Leader {
    /// Start / end x relative to the line's `x`.
    pub x0: f32,
    pub x1: f32,
    /// Baseline y relative to the line's top.
    pub baseline: f32,
    /// 1 = dots, 2 = dashes, 3 = solid line, 4 = middle dots
    pub kind: u8,
    pub color: [u8; 3],
    /// Font size in layout points (zoomed).
    pub size: f32,
}

/// Character style used to draw the header (`footer == false`) or footer text.
pub fn hf_style(doc: &Document, footer: bool) -> CharStyle {
    let (st, italic) = if footer {
        (doc.footer_style, doc.footer_italic)
    } else {
        (doc.header_style, doc.header_italic)
    };
    match st {
        Some(s) => CharStyle {
            italic: s.italic || italic,
            link: 0,
            ..s
        },
        None => CharStyle {
            font: 0,
            size: 10.0,
            color: [95, 95, 95],
            italic,
            ..CharStyle::default()
        },
    }
}

/// Header / footer galley in the document's own font, size and color.
pub fn hf_galley(
    fonts: &mut egui::epaint::text::FontsView<'_>,
    book: &FontBook,
    doc: &Document,
    footer: bool,
    text: &str,
    ppi: f32,
) -> Arc<Galley> {
    let st = hf_style(doc, footer);
    let mut job = LayoutJob::default();
    job.append(text, 0.0, text_format(book, &st, ppi));
    fonts.layout_job(job)
}

/// Top y of the header text and of the footer text on a page of height `page_h` (layout points).
pub fn hf_y(doc: &Document, lay: &PageLayout, footer: bool, galley_h: f32) -> f32 {
    let ppi = lay.ppi;
    let scale = ppi / BASE_PPI;
    if footer {
        if doc.footer_dist_in > 0.0 {
            (lay.page_size.y - doc.footer_dist_in * ppi - galley_h).max(0.0)
        } else {
            let bottom_margin = (lay.page_size.y - lay.content_offset.y - lay.content_size.y)
                .max(galley_h + 4.0 * scale);
            let pt = ppi / 72.0;
            lay.page_size.y
                - (bottom_margin * 0.42 + galley_h * 0.5)
                    .clamp(galley_h + 3.0 * scale, 30.0 * pt + galley_h * 0.5)
        }
    } else if doc.header_dist_in > 0.0 {
        doc.header_dist_in * ppi
    } else {
        (lay.content_offset.y * 0.5 - galley_h * 0.5).max(3.0 * scale)
    }
}

/// Filled marks `(rect, corner radius, color)` relative to the line origin that draw the
/// line's tab leaders, so the screen and PDF renderers paint identical dots / dashes / rules.
pub fn leader_marks(line: &Line) -> Vec<(Rect, f32, [u8; 3])> {
    let mut out = Vec::new();
    for ld in &line.leaders {
        let y = ld.baseline;
        let w = (ld.size / 16.0).max(0.8);
        match ld.kind {
            3 => out.push((
                Rect::from_min_max(Pos2::new(ld.x0, y + 0.5), Pos2::new(ld.x1, y + 0.5 + w)),
                0.0,
                ld.color,
            )),
            2 => {
                let step = ld.size * 0.5;
                let mut x = ld.x0 + step * 0.25;
                let yy = y - ld.size * 0.25;
                while x + step * 0.5 <= ld.x1 {
                    out.push((
                        Rect::from_min_max(
                            Pos2::new(x, yy - w * 0.5),
                            Pos2::new(x + step * 0.5, yy + w * 0.5),
                        ),
                        0.0,
                        ld.color,
                    ));
                    x += step;
                }
            }
            _ => {
                let step = ld.size * 0.3;
                let r = (ld.size * 0.05).max(0.6);
                let cy = if ld.kind == 4 {
                    y - ld.size * 0.3
                } else {
                    y - r
                };
                let mut x = ld.x0 + step * 0.5;
                while x + r <= ld.x1 - step * 0.3 {
                    out.push((
                        Rect::from_center_size(Pos2::new(x, cy), Vec2::splat(r * 2.0)),
                        r,
                        ld.color,
                    ));
                    x += step;
                }
            }
        }
    }
    out
}

/// Horizontal extent `(x0, x1)` of paragraph `pi`'s text block relative to the content box,
/// used for paragraph shading and borders (from the left indent to the right indent).
pub fn para_block_x(doc: &Document, lay: &PageLayout, pi: usize) -> (f32, f32) {
    let f = doc.paras[pi].fmt;
    let x0 = doc.block_left_in(&f) * lay.ppi;
    let x1 = lay.content_size.x - f.right_in * lay.ppi;
    (x0, x1.max(x0 + 4.0))
}

/// Filled rects `(rect, color)` relative to the content box origin that draw the paragraph
/// borders crossing `line`. Consecutive paragraphs with identical borders form one box.
pub fn para_border_rects(doc: &Document, lay: &PageLayout, line: &Line) -> Vec<(Rect, [u8; 3])> {
    let pi = line.para;
    let f = doc.paras[pi].fmt;
    let b = f.pborders;
    if b.is_empty() || f.table_id > 0 {
        return Vec::new();
    }
    let same = |q: usize| {
        let g = doc.paras[q].fmt;
        g.pborders == b && g.left_in == f.left_in && g.right_in == f.right_in && g.table_id == 0
    };
    let opens = line.first_in_para && (pi == 0 || !same(pi - 1));
    let closes = line.last_in_para && (pi + 1 >= doc.paras.len() || !same(pi + 1));
    let pt = lay.ppi / 72.0;
    let (bx0, bx1) = para_block_x(doc, lay, pi);
    let pad = 4.0 * pt;
    let x0 = bx0 - pad;
    let x1 = bx1 + pad;
    let mut y0 = line.y;
    let mut y1 = line.y + line.height;
    if line.first_in_para {
        y0 -= if opens { 1.0 * pt } else { f.space_before * pt };
    }
    if line.last_in_para {
        y1 += if closes { 1.0 * pt } else { f.space_after * pt };
    }
    let mut out = Vec::new();
    let wpx = |s: &crate::doc::BorderSpec| (s.width_pt * pt).max(0.75);
    if let (Some(s), true) = (b.top, opens) {
        out.push((
            Rect::from_min_max(Pos2::new(x0, y0 - wpx(&s)), Pos2::new(x1, y0)),
            s.color,
        ));
    }
    if let (Some(s), true) = (b.bottom, closes) {
        out.push((
            Rect::from_min_max(Pos2::new(x0, y1), Pos2::new(x1, y1 + wpx(&s))),
            s.color,
        ));
    }
    if let Some(s) = b.left {
        out.push((
            Rect::from_min_max(Pos2::new(x0 - wpx(&s), y0), Pos2::new(x0, y1)),
            s.color,
        ));
    }
    if let Some(s) = b.right {
        out.push((
            Rect::from_min_max(Pos2::new(x1, y0), Pos2::new(x1 + wpx(&s), y1)),
            s.color,
        ));
    }
    out
}

/// Builds the layout job for `para.text[from..to]` with the given leading space after each tab.
fn build_job(
    book: &FontBook,
    para: &Para,
    from: usize,
    to: usize,
    leads: &[(usize, f32)],
    ppi: f32,
    style_for: &dyn Fn(&CharStyle) -> CharStyle,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    if from >= to {
        let s = if para.text.is_empty() {
            para.end_style
        } else {
            para.style_at(from)
        };
        job.append(" ", 0.0, text_format(book, &style_for(&s), ppi));
        return job;
    }
    let mut buf = String::new();
    let mut cur: Option<(CharStyle, bool)> = None;
    let mut pending_lead = 0.0f32;
    let flush =
        |job: &mut LayoutJob, buf: &mut String, cur: &Option<(CharStyle, bool)>, lead: &mut f32| {
            if buf.is_empty() {
                return;
            }
            if let Some((s, small)) = cur {
                let mut tf = text_format(book, s, ppi);
                if *small {
                    tf.font_id.size *= 0.8;
                }
                if s.spacing_pt != 0.0 {
                    tf.extra_letter_spacing = s.spacing_pt * ppi / 72.0;
                }
                job.append(buf, *lead, tf);
            }
            *lead = 0.0;
            buf.clear();
        };
    for i in from..to {
        let raw = para.text[i];
        let st = style_for(&para.styles[i]);
        let mut ch = if raw == '\t' { ' ' } else { raw };
        let mut small = false;
        if st.caps != 0 && ch.is_lowercase() {
            let mut up = ch.to_uppercase();
            if let (Some(u), None) = (up.next(), up.next()) {
                ch = u;
                small = st.caps == 2;
            }
        }
        let key = (st, small);
        let lead_here = if i > from && para.text[i - 1] == '\t' {
            leads.iter().find(|(t, _)| *t == i - 1).map(|(_, l)| *l)
        } else {
            None
        };
        if cur != Some(key) || lead_here.is_some() {
            flush(&mut job, &mut buf, &cur, &mut pending_lead);
            cur = Some(key);
            if let Some(l) = lead_here {
                pending_lead = l;
            }
        }
        buf.push(ch);
    }
    flush(&mut job, &mut buf, &cur, &mut pending_lead);
    job
}

/// For each char in `from..to`, its `(row, glyph)` in `galley` (None for consumed newlines).
fn char_glyph_map(
    galley: &Galley,
    para: &Para,
    from: usize,
    to: usize,
) -> Vec<Option<(usize, usize)>> {
    let mut map = vec![None; to.saturating_sub(from)];
    if from >= to {
        return map;
    }
    let mut c = from;
    for (r, placed) in galley.rows.iter().enumerate() {
        for g in 0..placed.row.glyphs.len() {
            if c < to {
                map[c - from] = Some((r, g));
            }
            c += 1;
        }
        if placed.ends_with_newline && para.text.get(c) == Some(&'\n') {
            c += 1;
        }
    }
    map
}

/// Next tab stop strictly right of `x` (layout points from the column's left edge).
fn next_tab_stop(
    x: f32,
    tabs: &[TabStop],
    hang_stop: Option<f32>,
    default_px: f32,
    ppi: f32,
) -> (f32, TabKind, u8) {
    let mut best: Option<(f32, TabKind, u8)> = tabs
        .iter()
        .find(|t| t.pos_in * ppi > x + 0.5)
        .map(|t| (t.pos_in * ppi, t.kind, t.leader));
    if let Some(h) = hang_stop {
        if h > x + 0.5 && best.is_none_or(|b| h < b.0) {
            best = Some((h, TabKind::Left, 0));
        }
    }
    best.unwrap_or_else(|| {
        let last = tabs.last().map_or(0.0, |t| t.pos_in * ppi);
        let base = x.max(last);
        let step = default_px.max(4.0);
        (
            (((base + 0.5) / step).floor() + 1.0) * step,
            TabKind::Left,
            0,
        )
    })
}

struct Shaped {
    galley: Arc<Galley>,
    /// `(row, leader)` pairs with x relative to the galley's left edge.
    leaders: Vec<(usize, Leader)>,
}

/// Lays out `para.text[from..to]` in a column starting `row_x` from the column's left edge,
/// resolving tab stops (left / center / right / decimal, with leaders) by iterative relayout.
#[allow(clippy::too_many_arguments)]
fn shape_range(
    fonts: &mut egui::epaint::text::FontsView<'_>,
    doc: &Document,
    book: &FontBook,
    para: &Para,
    from: usize,
    to: usize,
    row_x: f32,
    width: f32,
    wrap: bool,
    hang_stop: Option<f32>,
    ppi: f32,
    style_for: &dyn Fn(&CharStyle) -> CharStyle,
) -> Shaped {
    let fmt = para.fmt;
    let tab_chars: Vec<usize> = (from..to).filter(|&i| para.text[i] == '\t').collect();
    let mut leads: Vec<(usize, f32)> = tab_chars.iter().map(|&t| (t, 0.0)).collect();
    let make = |fonts: &mut egui::epaint::text::FontsView<'_>, leads: &[(usize, f32)]| {
        let mut job = build_job(book, para, from, to, leads, ppi, style_for);
        job.wrap = TextWrapping {
            max_width: if wrap { width.max(8.0) } else { f32::INFINITY },
            ..Default::default()
        };
        job.justify = fmt.align == Align::Justify;
        job.keep_trailing_whitespace = true;
        fonts.layout_job(job)
    };
    let mut galley = make(fonts, &leads);
    let mut leaders = Vec::new();
    if tab_chars.is_empty() {
        return Shaped { galley, leaders };
    }
    let default_px = doc.default_tab_in.max(0.05) * ppi;
    for _ in 0..8 {
        let map = char_glyph_map(&galley, para, from, to);
        let mut next: Vec<(usize, f32)> = Vec::with_capacity(tab_chars.len());
        let mut lds = Vec::new();
        for &ti in &tab_chars {
            let old = leads
                .iter()
                .find(|(t, _)| *t == ti)
                .map_or(0.0, |(_, l)| *l);
            let Some((r, g)) = map[ti - from] else {
                next.push((ti, old));
                continue;
            };
            let glyphs = &galley.rows[r].row.glyphs;
            let gl = &glyphs[g];
            let tab_start = gl.pos.x;
            let tab_end = gl.pos.x + gl.advance_width;
            let (stop, kind, leader) = next_tab_stop(
                row_x + tab_start,
                fmt.tabs.as_slice(),
                hang_stop,
                default_px,
                ppi,
            );
            // Width of the text that follows, up to the next tab or the row's end.
            let seg_start = glyphs.get(g + 1).map_or(tab_end + old, |n| n.pos.x);
            let mut seg_end = seg_start;
            let mut dec_x: Option<f32> = None;
            let mut ci = ti + 1;
            for n in glyphs.iter().skip(g + 1) {
                if para.text.get(ci) == Some(&'\t') {
                    break;
                }
                if dec_x.is_none() && (n.chr == '.' || n.chr == ',') {
                    dec_x = Some(n.pos.x);
                }
                if !n.chr.is_whitespace() {
                    seg_end = n.pos.x + n.advance_width;
                }
                ci += 1;
            }
            let seg_w = (seg_end - seg_start).max(0.0);
            let target_rel = match kind {
                TabKind::Left => stop,
                TabKind::Right => stop - seg_w,
                TabKind::Center => stop - seg_w * 0.5,
                TabKind::Decimal => stop - dec_x.map_or(seg_w, |d| d - seg_start),
            } - row_x;
            let mut lead = (target_rel - tab_end).max(0.0);
            let limit = if wrap { width } else { f32::INFINITY };
            if kind == TabKind::Left && tab_end + lead > limit + 1.0 {
                lead = 0.0;
            } else if kind != TabKind::Left && tab_end + lead + seg_w > limit + 2.0 {
                lead = (limit - seg_w - tab_end).max(0.0);
            }
            next.push((ti, lead));
            if leader > 0 && lead > 2.0 {
                let st = para.styles[ti];
                lds.push((
                    r,
                    Leader {
                        x0: tab_start,
                        x1: tab_end + lead,
                        baseline: gl.pos.y,
                        kind: leader,
                        color: st.color,
                        size: st.size * ppi / 72.0,
                    },
                ));
            }
        }
        let converged = next
            .iter()
            .zip(&leads)
            .all(|(a, b)| (a.1 - b.1).abs() < 0.5);
        leaders = lds;
        if converged {
            break;
        }
        leads = next;
        galley = make(fonts, &leads);
    }
    Shaped { galley, leaders }
}

/// Word-accurate natural single-line height in layout points, using `hhea` / `OS/2` metrics
/// from the actual OpenType font face (matching Microsoft Word & WordCraft).
fn word_natural_line_h(para: &Para, start: usize, end: usize, pt: f32, fallback: f32) -> f32 {
    let mut best = 0.0f32;
    let slice = if start < end && end <= para.styles.len() {
        &para.styles[start..end]
    } else {
        std::slice::from_ref(&para.end_style)
    };
    for st in slice {
        if let Some((a, d)) = crate::fonts::word_line_metrics(st.font, st.bold, st.italic) {
            let h = st.size * pt * (a + d);
            if h > best {
                best = h;
            }
        }
    }
    if best > 0.0 {
        best
    } else {
        fallback
    }
}

#[allow(clippy::too_many_arguments)]
fn shape_para_lines(
    fonts: &mut egui::epaint::text::FontsView<'_>,
    doc: &Document,
    book: &FontBook,
    pi: usize,
    base_x: f32,
    base_avail: f32,
    ppi: f32,
    wrap_text: bool,
) -> Vec<Line> {
    let para = &doc.paras[pi];
    let fmt = para.fmt;
    let pt = ppi / 72.0;
    let (left_in, first_in) = doc.indents_in(&fmt);
    let left = left_in * ppi;
    let first = first_in * ppi;
    let right = fmt.right_in * ppi;
    let rest_w = (base_avail - left - right).max(8.0);
    let mut out = Vec::new();

    let line_h = |natural: f32| -> f32 {
        if fmt.line_exact_pt > 0.0 {
            fmt.line_exact_pt * pt
        } else {
            let h = natural.max(1.0) * fmt.line_spacing;
            if fmt.line_min_pt > 0.0 {
                h.max(fmt.line_min_pt * pt)
            } else {
                h
            }
        }
    };

    if fmt.image_id > 0 {
        let block_left = (left + first.min(0.0)).max(0.0);
        let avail = (base_avail - block_left - right).max(8.0);
        if let Some((img_w, _img_h, _cap_h, total_h)) = image_box(doc, fmt.image_id, avail, ppi) {
            let mut job = LayoutJob::default();
            job.append(" ", 0.0, text_format(book, &para.end_style, ppi));
            let galley = fonts.layout_job(job);
            let offset = match fmt.align {
                Align::Left | Align::Justify => 0.0,
                Align::Center => ((avail - img_w) * 0.5).max(0.0),
                Align::Right => (avail - img_w).max(0.0),
            };
            out.push(Line {
                para: pi,
                start: 0,
                end: para.len(),
                page: 0,
                x: base_x + block_left + offset,
                y: 0.0,
                height: total_h,
                galley: single_row(&galley, 0, para, 0, para.len()),
                xs: vec![0.0, img_w],
                first_in_para: true,
                last_in_para: true,
                marker: None,
                marker_x: base_x + block_left,
                leaders: Vec::new(),
            });
            return out;
        }
    }

    let checked_checklist = fmt.list == ListKind::Checklist && fmt.checked;
    let style_for = move |s: &CharStyle| -> CharStyle {
        if checked_checklist {
            CharStyle {
                strike: true,
                color: if s.color == [0, 0, 0] { [110, 118, 129] } else { s.color },
                ..*s
            }
        } else {
            *s
        }
    };

    // List marker (bullet / number / checkbox) and where the first line's text starts.
    let mut first_x = left + first;
    let mut marker_x = left + first;
    let mut hang_stop = if first < -0.5 { Some(left) } else { None };
    let marker = if fmt.list != ListKind::None {
        let s = para.style_at(0);
        let text = doc.list_marker(pi);
        let lv = doc.list_level(&fmt);
        let mut ms = CharStyle {
            underline: false,
            strike: false,
            highlight: None,
            highlight_to: None,
            link: 0,
            caps: 0,
            spacing_pt: 0.0,
            ..s
        };
        if let Some(lv) = lv {
            if let Some(f) = lv.font {
                ms.font = f;
            }
            if let Some(c) = lv.color {
                ms.color = c;
            }
            if let Some(sz) = lv.size {
                ms.size = sz;
            }
            if let Some(b) = lv.bold {
                ms.bold = b;
            }
        }
        let mut mj = LayoutJob::default();
        mj.append(
            if text.is_empty() { " " } else { &text },
            0.0,
            text_format(book, &ms, ppi),
        );
        let mg = fonts.layout_job(mj);
        let mw = mg.rect.width();
        let anchor = left + first;
        marker_x = match lv.map_or(Align::Left, |l| l.align) {
            Align::Right => anchor - mw,
            Align::Center => anchor - mw * 0.5,
            _ => anchor,
        };
        let m_end = marker_x + mw;
        first_x = match lv.map_or(0, |l| l.suffix) {
            1 => m_end + ms.size * pt * 0.28,
            2 => m_end,
            _ => {
                if fmt.list_id == 0 || left > m_end + 0.5 {
                    left.max(m_end + 2.0 * pt)
                } else {
                    next_tab_stop(
                        m_end,
                        fmt.tabs.as_slice(),
                        None,
                        doc.default_tab_in.max(0.05) * ppi,
                        ppi,
                    )
                    .0
                }
            }
        };
        hang_stop = None;
        Some(mg)
    } else {
        None
    };

    let n = para.len();
    let first_w = (base_avail - right - first_x).max(8.0);
    // Shape the first line at its own indent, then the rest of the paragraph at the left indent.
    // Each part: (shaped, first char, row x, width, only its first row is used)
    let mut parts: Vec<(Shaped, usize, f32, f32, bool)> = Vec::new();
    if n == 0 {
        let sh = shape_range(
            fonts, doc, book, para, 0, 0, first_x, first_w, wrap_text, hang_stop, ppi, &style_for,
        );
        parts.push((sh, 0, first_x, first_w, false));
    } else if (first_x - left).abs() < 0.5 {
        let sh = shape_range(
            fonts, doc, book, para, 0, n, left, rest_w, wrap_text, hang_stop, ppi, &style_for,
        );
        parts.push((sh, 0, left, rest_w, false));
    } else {
        let sh = shape_range(
            fonts, doc, book, para, 0, n, first_x, first_w, wrap_text, hang_stop, ppi, &style_for,
        );
        let mut consumed = sh.galley.rows.first().map_or(n, |r| r.row.glyphs.len());
        let more = sh.galley.rows.len() > 1;
        if sh.galley.rows.first().is_some_and(|r| r.ends_with_newline)
            && para.text.get(consumed) == Some(&'\n')
        {
            consumed += 1;
        }
        parts.push((sh, 0, first_x, first_w, true));
        if more {
            let from = consumed.min(n);
            let sh2 = shape_range(
                fonts, doc, book, para, from, n, left, rest_w, wrap_text, None, ppi, &style_for,
            );
            parts.push((sh2, from, left, rest_w, false));
        }
    }

    let empty = n == 0;
    let total_rows: usize = parts
        .iter()
        .map(|(s, _, _, _, only)| if *only { 1 } else { s.galley.rows.len() })
        .sum();
    let mut row_no = 0usize;
    for (sh, from, row_x, avail, only_first) in &parts {
        let galley = &sh.galley;
        let part_empty = empty || *from >= n;
        let nrows = if *only_first { 1 } else { galley.rows.len() };
        let mut char_cursor = *from;
        for r in 0..nrows {
            let placed = &galley.rows[r];
            let glyphs = &placed.row.glyphs;
            let count = if part_empty { 0 } else { glyphs.len() };
            let start = char_cursor.min(n);
            let end = (char_cursor + count).min(n);
            char_cursor += count;
            if placed.ends_with_newline && para.text.get(char_cursor) == Some(&'\n') {
                char_cursor += 1;
            }
            let mut trimmed = 0.0f32;
            for g in glyphs.iter().rev() {
                if !g.chr.is_whitespace() {
                    trimmed = g.pos.x + g.advance_width;
                    break;
                }
            }
            let offset = match fmt.align {
                Align::Left | Align::Justify => 0.0,
                Align::Center => ((avail - trimmed) / 2.0).max(0.0),
                Align::Right => (avail - trimmed).max(0.0),
            };
            let mut xs = Vec::with_capacity(end - start + 1);
            for k in 0..(end - start) {
                xs.push(glyphs.get(k).map_or(0.0, |g| g.pos.x));
            }
            let end_x = if end > start {
                glyphs
                    .get(end - start - 1)
                    .map_or(0.0, |g| g.pos.x + g.advance_width)
            } else {
                0.0
            };
            xs.push(end_x);
            let leaders = sh
                .leaders
                .iter()
                .filter(|(lr, _)| *lr == r)
                .map(|(_, l)| *l)
                .collect();
            let is_first = row_no == 0;
            let nat_h = word_natural_line_h(para, start, end, pt, placed.row.size.y);
            out.push(Line {
                para: pi,
                start,
                end,
                page: 0,
                x: base_x + row_x + offset,
                y: 0.0,
                height: line_h(nat_h),
                galley: single_row(galley, r, para, start, end),
                xs,
                first_in_para: is_first,
                last_in_para: row_no + 1 == total_rows,
                marker: if is_first { marker.clone() } else { None },
                marker_x: base_x + marker_x,
                leaders,
            });
            row_no += 1;
        }
    }
    out
}

pub fn build(ctx: &egui::Context, doc: &Document, book: &FontBook, zoom: f32, draft: bool) -> PageLayout {
    let draft = draft || doc.pageless;
    let ppi = BASE_PPI * zoom;
    let pg = &doc.page;
    let page_size = Vec2::new(pg.width * ppi, pg.height * ppi);
    let (content_offset, content_size) = if draft {
        (Vec2::new(0.5 * ppi, 0.4 * ppi), Vec2::new((pg.width - pg.margin_left - pg.margin_right) * ppi, f32::INFINITY))
    } else {
        (
            Vec2::new(pg.margin_left * ppi, pg.margin_top * ppi),
            Vec2::new((pg.width - pg.margin_left - pg.margin_right) * ppi, (pg.height - pg.margin_top - pg.margin_bottom) * ppi),
        )
    };
    let pt = ppi / 72.0;
    let mut lines: Vec<Line> = Vec::new();
    let mut cells: Vec<LayoutCell> = Vec::new();
    let mut page = 0usize;
    let mut y = 0.0f32;

    ctx.fonts_mut(|fonts| {
        let mut pi = 0usize;
        while pi < doc.paras.len() {
            let tid = doc.paras[pi].fmt.table_id;
            if tid == 0 {
                let fmt = doc.paras[pi].fmt;
                if fmt.page_break_before && pi > 0 && !draft {
                    page += 1;
                    y = 0.0;
                }
                if y > 0.0 {
                    y += fmt.space_before * pt;
                }
                let para_lines = shape_para_lines(fonts, doc, book, pi, 0.0, content_size.x, ppi, true);
                let para_total_h: f32 = para_lines.iter().map(|l| l.height).sum();
                if !draft && y > 0.0 {
                    // keep_lines: keep all lines of this paragraph together on one page
                    let break_keep_lines = fmt.keep_lines
                        && y + para_total_h > content_size.y
                        && para_total_h <= content_size.y;
                    // keep_next: keep this paragraph with at least the first line of the next paragraph
                    let break_keep_next = if fmt.keep_next
                        && pi + 1 < doc.paras.len()
                        && doc.paras[pi + 1].fmt.table_id == 0
                        && !doc.paras[pi + 1].fmt.page_break_before
                    {
                        let next_lines = shape_para_lines(fonts, doc, book, pi + 1, 0.0, content_size.x, ppi, true);
                        let next_first_h = next_lines.first().map_or(0.0, |l| l.height);
                        let needed = para_total_h
                            + fmt.space_after * pt
                            + doc.paras[pi + 1].fmt.space_before * pt
                            + next_first_h;
                        y + needed > content_size.y && needed <= content_size.y
                    } else {
                        false
                    };
                    if break_keep_lines || break_keep_next {
                        page += 1;
                        y = 0.0;
                    }
                }
                // Widow / orphan control (Word `w:widowControl`, Google Docs `avoidWidowAndOrphan`)
                let mut force_break_at: Option<usize> = None;
                if !draft && fmt.widow_control && para_lines.len() >= 2 && y > 0.0 {
                    let mut sim_y = y;
                    let mut fit = 0usize;
                    for l in &para_lines {
                        if sim_y + l.height > content_size.y {
                            break;
                        }
                        sim_y += l.height;
                        fit += 1;
                    }
                    if fit < para_lines.len() {
                        if fit <= 1 && para_total_h <= content_size.y {
                            // Prevent orphan (1 line alone at bottom of page)
                            page += 1;
                            y = 0.0;
                        } else if para_lines.len() - fit == 1 && fit >= 3 {
                            // Prevent widow (1 line alone at top of next page)
                            force_break_at = Some(fit - 1);
                        }
                    }
                }
                for (li, mut l) in para_lines.into_iter().enumerate() {
                    if !draft
                        && y > 0.0
                        && (force_break_at == Some(li) || y + l.height > content_size.y)
                    {
                        page += 1;
                        y = 0.0;
                    }
                    l.page = page;
                    l.y = y;
                    y += l.height;
                    lines.push(l);
                }
                y += fmt.space_after * pt;
                pi += 1;
                continue;
            }

            // Group contiguous paragraphs belonging to table `tid`
            let mut tbl_end = pi + 1;
            while tbl_end < doc.paras.len() && doc.paras[tbl_end].fmt.table_id == tid {
                tbl_end += 1;
            }
            if doc.paras[pi].fmt.page_break_before && pi > 0 && !draft {
                page += 1;
                y = 0.0;
            }

            let tbl_opt = doc.table(tid);
            let tbl_indent_x = tbl_opt.map_or(0.0, |t| (t.indent_in * ppi).max(0.0));
            let max_para_row = doc.paras[pi..tbl_end]
                .iter()
                .map(|p| p.fmt.table_row as usize)
                .max()
                .unwrap_or(0)
                + 1;
            let num_rows = tbl_opt.map_or(max_para_row, |t| t.rows.len().max(max_para_row));

            // Compute horizontal scale if table exceeds content_size.x
            let raw_tbl_w = tbl_opt
                .and_then(|t| {
                    let grid_sum: f32 = t.grid_cols_in.iter().sum();
                    if grid_sum > 0.0 {
                        Some(grid_sum * ppi)
                    } else {
                        t.rows.first().map(|r| r.cells.iter().map(|c| c.width_in * ppi).sum())
                    }
                })
                .unwrap_or(content_size.x);
            let avail_tbl_w = (content_size.x - tbl_indent_x).max(40.0 * zoom);
            let col_scale = if raw_tbl_w > avail_tbl_w + 1.0 { avail_tbl_w / raw_tbl_w } else { 1.0 };

            // Cache leading repeating header rows (`w:tblHeader`) to re-emit on page breaks.
            let mut header_rows: Vec<(f32, Vec<Line>, Vec<LayoutCell>)> = Vec::new();
            let mut collecting_headers = true;

            for r_idx in 0..num_rows {
                let row_spec = tbl_opt.and_then(|t| t.rows.get(r_idx));
                let is_header_row = row_spec.is_some_and(|rs| rs.is_header);
                if !is_header_row {
                    collecting_headers = false;
                }
                let max_para_col = doc.paras[pi..tbl_end]
                    .iter()
                    .filter(|p| p.fmt.table_row as usize == r_idx)
                    .map(|p| p.fmt.table_col as usize + 1)
                    .max()
                    .unwrap_or(1);
                let num_cols = row_spec.map_or(max_para_col, |rs| rs.cells.len().max(max_para_col));

                struct ShapedCell {
                    col: u16,
                    x: f32,
                    w: f32,
                    pad_t: f32,
                    pad_b: f32,
                    text_h: f32,
                    natural_h: f32,
                    spec: TableCellSpec,
                    lines: Vec<Line>,
                }

                let mut shaped_cells: Vec<ShapedCell> = Vec::with_capacity(num_cols);
                let mut cell_x = tbl_indent_x;
                for c_idx in 0..num_cols {
                    let spec = row_spec
                        .and_then(|rs| rs.cells.get(c_idx))
                        .cloned()
                        .unwrap_or_else(|| TableCellSpec {
                            width_in: (content_size.x / ppi) / (num_cols.max(1) as f32),
                            ..TableCellSpec::default()
                        });
                    let cell_w = (spec.width_in * ppi * col_scale).max(12.0 * zoom);
                    let pad_l = spec.pad_left_in * ppi * col_scale;
                    let pad_r = spec.pad_right_in * ppi * col_scale;
                    let pad_t = spec.pad_top_in * ppi;
                    let pad_b = spec.pad_bottom_in * ppi;
                    let inner_avail = (cell_w - pad_l - pad_r).max(6.0 * zoom);

                    let mut cell_lines: Vec<Line> = Vec::new();
                    let mut local_y = pad_t;
                    for p_idx in pi..tbl_end {
                        let pfmt = doc.paras[p_idx].fmt;
                        if pfmt.table_row as usize == r_idx && pfmt.table_col as usize == c_idx {
                            local_y += pfmt.space_before * pt;
                            let p_lines = shape_para_lines(fonts, doc, book, p_idx, cell_x + pad_l, inner_avail, ppi, spec.wrap_text);
                            for mut l in p_lines {
                                l.y = local_y;
                                local_y += l.height;
                                cell_lines.push(l);
                            }
                            local_y += pfmt.space_after * pt;
                        }
                    }
                    let text_h = (local_y - pad_t).max(0.0);
                    let natural_h = (local_y + pad_b).max(4.0 * zoom);
                    shaped_cells.push(ShapedCell {
                        col: c_idx as u16,
                        x: cell_x,
                        w: cell_w,
                        pad_t,
                        pad_b,
                        text_h,
                        natural_h,
                        spec,
                        lines: cell_lines,
                    });
                    cell_x += cell_w;
                }

                let min_row_h = row_spec.map_or(0.0, |rs| rs.min_height_pt * pt);
                let row_h = shaped_cells
                    .iter()
                    .map(|sc| sc.natural_h)
                    .fold(min_row_h, f32::max);

                if !draft && y + row_h > content_size.y && y > 0.0 {
                    page += 1;
                    y = 0.0;
                    if !is_header_row && !header_rows.is_empty() {
                        let hdr_total: f32 = header_rows.iter().map(|(h, _, _)| *h).sum();
                        if hdr_total + row_h <= content_size.y {
                            for (hh, h_lines, h_cells) in &header_rows {
                                for hl in h_lines {
                                    let mut c = hl.clone();
                                    c.page = page;
                                    c.y += y;
                                    lines.push(c);
                                }
                                for hc in h_cells {
                                    let mut c = hc.clone();
                                    c.page = page;
                                    c.rect = c.rect.translate(Vec2::new(0.0, y));
                                    cells.push(c);
                                }
                                y += *hh;
                            }
                        }
                    }
                }

                let mut row_hdr_lines = Vec::new();
                let mut row_hdr_cells = Vec::new();
                for sc in shaped_cells {
                    let dy = match sc.spec.v_align {
                        CellVAlign::Top => 0.0,
                        CellVAlign::Center => ((row_h - sc.text_h) * 0.5 - sc.pad_t).max(-sc.pad_t),
                        CellVAlign::Bottom => (row_h - sc.pad_b - sc.text_h - sc.pad_t).max(-sc.pad_t),
                    };
                    for mut l in sc.lines {
                        if collecting_headers && is_header_row {
                            let mut rel = l.clone();
                            rel.y += dy;
                            row_hdr_lines.push(rel);
                        }
                        l.page = page;
                        l.y += y + dy;
                        lines.push(l);
                    }
                    let cell_rel = LayoutCell {
                        table_id: tid,
                        row: r_idx as u16,
                        col: sc.col,
                        page,
                        rect: Rect::from_min_size(Pos2::new(sc.x, 0.0), Vec2::new(sc.w, row_h)),
                        shading: sc.spec.shading,
                        borders: sc.spec.borders,
                        wrap_text: sc.spec.wrap_text,
                    };
                    if collecting_headers && is_header_row {
                        row_hdr_cells.push(cell_rel.clone());
                    }
                    let mut placed_cell = cell_rel;
                    placed_cell.rect = placed_cell.rect.translate(Vec2::new(0.0, y));
                    cells.push(placed_cell);
                }
                if collecting_headers && is_header_row {
                    header_rows.push((row_h, row_hdr_lines, row_hdr_cells));
                }

                y += row_h;
            }

            pi = tbl_end;
        }
    });

    lines.sort_by_key(|l| (l.para, l.start, l.page));
    let pages = page + 1;
    let page_size = if draft { Vec2::new(page_size.x, (y + content_offset.y * 2.0).max(page_size.y)) } else { page_size };
    PageLayout { lines, cells, pages, ppi, page_size, content_offset, content_size, draft }
}

impl PageLayout {
    /// Top-left of page `k` relative to the document canvas origin.
    pub fn page_origin(&self, k: usize, canvas_w: f32) -> Pos2 {
        let x = ((canvas_w - self.page_size.x) / 2.0).max(PAGE_GAP);
        Pos2::new(x, PAGE_GAP + k as f32 * (self.page_size.y + PAGE_GAP))
    }
    pub fn canvas_size(&self, min_w: f32) -> Vec2 {
        Vec2::new((self.page_size.x + 2.0 * PAGE_GAP).max(min_w), PAGE_GAP + self.pages as f32 * (self.page_size.y + PAGE_GAP))
    }
    pub fn line_origin(&self, line: &Line, canvas_min: Pos2, canvas_w: f32) -> Pos2 {
        let po = self.page_origin(line.page, canvas_w);
        canvas_min + po.to_vec2() + self.content_offset + Vec2::new(line.x, line.y)
    }
    /// Index of the line containing doc position (para, c).
    pub fn line_of(&self, p: usize, c: usize) -> usize {
        // binary search for first line of para
        let first = self.lines.partition_point(|l| l.para < p);
        let mut i = first;
        while i < self.lines.len() && self.lines[i].para == p {
            let l = &self.lines[i];
            if c < l.end || l.last_in_para || (c == l.end && i + 1 < self.lines.len() && self.lines[i + 1].para != p) {
                return i;
            }
            i += 1;
        }
        first.min(self.lines.len().saturating_sub(1))
    }
    pub fn x_in_line(&self, li: usize, c: usize) -> f32 {
        let l = &self.lines[li];
        let k = c.saturating_sub(l.start).min(l.xs.len() - 1);
        l.x + l.xs[k]
    }
    /// Char index nearest to x (relative to content box) in a line.
    pub fn char_at_x(&self, li: usize, x: f32) -> usize {
        let l = &self.lines[li];
        let rel = x - l.x;
        let n = l.end - l.start;
        for k in 0..n {
            let mid = (l.xs[k] + l.xs[k + 1]) / 2.0;
            if rel < mid {
                return l.start + k;
            }
        }
        // don't place caret after the trailing space of a wrapped line
        if !l.last_in_para && n > 0 {
            return l.end - 1;
        }
        l.end
    }
}

/// Measures the natural unwrapped content width (in inches, including horizontal cell padding)
/// needed to fit all cells in the column corresponding to `(table_id, ref_row, ref_col)`.
pub fn measure_column_autofit_width_in(
    ctx: &egui::Context,
    doc: &Document,
    book: &FontBook,
    table_id: u32,
    ref_row: usize,
    ref_col: usize,
) -> Option<f32> {
    let tbl = doc.table(table_id)?;
    let ppi = BASE_PPI;
    let avail_page_w = (doc.page.width - doc.page.margin_left - doc.page.margin_right).max(1.5);
    let target_grid: usize = tbl
        .rows
        .get(ref_row)
        .map_or(ref_col, |r| {
            let gs: usize = r.cells.iter().take(ref_col).map(|c| c.grid_span.max(1) as usize).sum();
            let sp = r.cells.get(ref_col).map_or(1, |c| c.grid_span.max(1) as usize);
            gs + sp - 1
        });

    let mut max_w_in = 0.55_f32;
    ctx.fonts_mut(|fonts| {
        for (r_idx, r) in tbl.rows.iter().enumerate() {
            let mut g_cur = 0usize;
            for (c_idx, cell) in r.cells.iter().enumerate() {
                let sp = cell.grid_span.max(1) as usize;
                let matches_col = if sp == 1 {
                    g_cur == target_grid
                } else {
                    r_idx == ref_row && c_idx == ref_col
                };
                g_cur += sp;
                if !matches_col {
                    continue;
                }
                let pad_x = cell.pad_left_in + cell.pad_right_in + 0.08;
                for (pi, p) in doc.paras.iter().enumerate() {
                    if p.fmt.table_id == table_id && p.fmt.table_row as usize == r_idx && p.fmt.table_col as usize == c_idx {
                        let lines = shape_para_lines(fonts, doc, book, pi, 0.0, 10_000.0, ppi, false);
                        for l in lines {
                            let w_pts =
                                l.xs.last()
                                    .copied()
                                    .unwrap_or(0.0)
                                    .max(l.galley.rect.width());
                            let w_in = ((l.x + w_pts) / ppi) + pad_x + p.fmt.right_in.max(0.0);
                            max_w_in = max_w_in.max(w_in);
                        }
                    }
                }
            }
        }
    });
    Some(max_w_in.clamp(0.45, avail_page_w))
}

/// Measures the exact natural height (in typographic points, 1/72 in) needed to fit the contents
/// of `row` in `table_id` at its current column widths.
pub fn measure_row_autofit_height_pt(
    ctx: &egui::Context,
    doc: &Document,
    book: &FontBook,
    table_id: u32,
    row: usize,
) -> Option<f32> {
    let tbl = doc.table(table_id)?;
    let row_spec = tbl.rows.get(row)?;
    let ppi = BASE_PPI;
    let pt = ppi / 72.0;
    let mut max_h_px = 14.0 * pt;
    ctx.fonts_mut(|fonts| {
        for (c_idx, spec) in row_spec.cells.iter().enumerate() {
            let cell_w = (spec.width_in * ppi).max(12.0);
            let pad_l = spec.pad_left_in * ppi;
            let pad_r = spec.pad_right_in * ppi;
            let pad_t = spec.pad_top_in * ppi;
            let pad_b = spec.pad_bottom_in * ppi;
            let inner_avail = (cell_w - pad_l - pad_r).max(6.0);
            let mut local_y = pad_t;
            for (pi, p) in doc.paras.iter().enumerate() {
                if p.fmt.table_id == table_id && p.fmt.table_row as usize == row && p.fmt.table_col as usize == c_idx {
                    local_y += p.fmt.space_before * pt;
                    let p_lines = shape_para_lines(fonts, doc, book, pi, 0.0, inner_avail, ppi, spec.wrap_text);
                    for l in p_lines {
                        local_y += l.height;
                    }
                    local_y += p.fmt.space_after * pt;
                }
            }
            max_h_px = max_h_px.max(local_y + pad_b);
        }
    });
    Some((max_h_px / pt).clamp(12.0, 720.0))
}

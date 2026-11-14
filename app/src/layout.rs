//! Turns the document into positioned lines on pages (or continuous Pageless canvas).

use eframe::egui::{
    self,
    text::{LayoutJob, TextFormat, TextWrapping},
    Color32, FontId, Galley, Pos2, Rect, Stroke, Vec2,
};
use std::sync::Arc;

use crate::doc::{lerp_rgb, Align, CellBorders, CellVAlign, CharStyle, Document, ListKind, Para, TableCellSpec};
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
    let hang = if fmt.list != ListKind::None { LIST_HANG * ppi } else { 0.0 };
    let indent = fmt.indent as f32 * INDENT_STEP * ppi + hang;
    let avail = (base_avail - indent).max(8.0);
    let mut out = Vec::new();

    if fmt.image_id > 0 {
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
                x: base_x + indent + offset,
                y: 0.0,
                height: total_h,
                galley: single_row(&galley, 0, para, 0, para.len()),
                xs: vec![0.0, img_w],
                first_in_para: true,
                last_in_para: true,
                marker: None,
                marker_x: base_x + indent,
            });
            return out;
        }
    }

    let checked_checklist = fmt.list == ListKind::Checklist && fmt.checked;
    let style_for = |s: &CharStyle| -> CharStyle {
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

    let mut job = LayoutJob::default();
    let empty = para.text.is_empty();
    if empty {
        job.append(" ", 0.0, text_format(book, &style_for(&para.end_style), ppi));
    } else {
        for (a, b, s) in para.runs() {
            let text: String = para.text[a..b].iter().collect();
            job.append(&text, 0.0, text_format(book, &style_for(&s), ppi));
        }
    }
    job.wrap = TextWrapping {
        max_width: if wrap_text { avail } else { f32::INFINITY },
        ..Default::default()
    };
    job.justify = fmt.align == Align::Justify;
    job.keep_trailing_whitespace = true;
    let galley = fonts.layout_job(job);

    let marker = if fmt.list != ListKind::None {
        let s = para.style_at(0);
        let text = doc.list_marker(pi);
        let mut mj = LayoutJob::default();
        mj.append(
            &text,
            0.0,
            text_format(
                book,
                &CharStyle { underline: false, strike: false, highlight: None, highlight_to: None, link: 0, ..s },
                ppi,
            ),
        );
        Some(fonts.layout_job(mj))
    } else {
        None
    };

    let nrows = galley.rows.len();
    let mut char_cursor = 0usize;
    for r in 0..nrows {
        let placed = &galley.rows[r];
        let glyphs = &placed.row.glyphs;
        let count = if empty { 0 } else { glyphs.len() };
        let start = char_cursor.min(para.len());
        let end = (char_cursor + count).min(para.len());
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
            xs.push(offset + glyphs.get(k).map_or(0.0, |g| g.pos.x));
        }
        let end_x = if end > start {
            glyphs.get(end - start - 1).map_or(0.0, |g| g.pos.x + g.advance_width)
        } else {
            0.0
        };
        xs.push(offset + end_x);
        let h = placed.row.size.y.max(1.0) * fmt.line_spacing;
        out.push(Line {
            para: pi,
            start,
            end,
            page: 0,
            x: base_x + indent + offset,
            y: 0.0,
            height: h,
            galley: single_row(&galley, r, para, start, end),
            xs: xs.iter().map(|v| v - offset).collect(),
            first_in_para: r == 0,
            last_in_para: r + 1 == nrows,
            marker: if r == 0 { marker.clone() } else { None },
            marker_x: base_x + indent - hang,
        });
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
                for mut l in para_lines {
                    if !draft && y + l.height > content_size.y && y > 0.0 {
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

            for r_idx in 0..num_rows {
                let row_spec = tbl_opt.and_then(|t| t.rows.get(r_idx));
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
                }

                for sc in shaped_cells {
                    let dy = match sc.spec.v_align {
                        CellVAlign::Top => 0.0,
                        CellVAlign::Center => ((row_h - sc.text_h) * 0.5 - sc.pad_t).max(-sc.pad_t),
                        CellVAlign::Bottom => (row_h - sc.pad_b - sc.text_h - sc.pad_t).max(-sc.pad_t),
                    };
                    for mut l in sc.lines {
                        l.page = page;
                        l.y += y + dy;
                        lines.push(l);
                    }
                    cells.push(LayoutCell {
                        table_id: tid,
                        row: r_idx as u16,
                        col: sc.col,
                        page,
                        rect: Rect::from_min_size(Pos2::new(sc.x, y), Vec2::new(sc.w, row_h)),
                        shading: sc.spec.shading,
                        borders: sc.spec.borders,
                        wrap_text: sc.spec.wrap_text,
                    });
                }

                y += row_h;
            }

            pi = tbl_end;
        }
    });

    lines.sort_by_key(|l| (l.para, l.start));
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
                            let w_pts = l.xs.last().copied().unwrap_or(0.0).max(l.galley.rect.width());
                            let indent_in = (p.fmt.indent as f32) * INDENT_STEP
                                + if p.fmt.list != ListKind::None { LIST_HANG } else { 0.0 };
                            let w_in = (w_pts / ppi) + pad_x + indent_in;
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

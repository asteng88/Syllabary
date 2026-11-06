//! Turns the document into positioned lines on pages.

use eframe::egui::{
    self,
    text::{LayoutJob, TextFormat, TextWrapping},
    Color32, FontId, Galley, Pos2, Rect, Stroke, Vec2,
};
use std::sync::Arc;

use crate::doc::{Align, CharStyle, Document, ListKind};
use crate::fonts::FontBook;

/// egui points per inch at 100 % zoom.
pub const BASE_PPI: f32 = 96.0;
/// Gap between pages on screen (points, not zoomed).
pub const PAGE_GAP: f32 = 28.0;
/// Hanging indent for bullets and numbers, in inches.
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
    pub last_in_para: bool,
    pub marker: Option<Arc<Galley>>,
    pub marker_x: f32,
}

pub struct PageLayout {
    pub lines: Vec<Line>,
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

pub fn text_format(book: &FontBook, s: &CharStyle, ppi: f32) -> TextFormat {
    let (family, fake_italic) = book.family(s.font, s.bold, s.italic);
    let mut size = s.size * ppi / 72.0;
    if s.script != 0 {
        size *= 0.65;
    }
    let col = color(s.color);
    let line_w = (size / 16.0).max(1.0);
    TextFormat {
        font_id: FontId::new(size.max(1.0), family),
        color: col,
        background: s.highlight.map(color).unwrap_or(Color32::TRANSPARENT),
        italics: fake_italic,
        underline: if s.underline { Stroke::new(line_w, col) } else { Stroke::NONE },
        strikethrough: if s.strike { Stroke::new(line_w, col) } else { Stroke::NONE },
        valign: if s.script > 0 { egui::Align::TOP } else { egui::Align::BOTTOM },
        ..Default::default()
    }
}

fn single_row(galley: &Arc<Galley>, row: usize) -> Arc<Galley> {
    let mut g: Galley = (**galley).clone();
    let mut placed = g.rows[row].clone();
    placed.pos = Pos2::new(0.0, 0.0);
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

pub fn build(ctx: &egui::Context, doc: &Document, book: &FontBook, zoom: f32, draft: bool) -> PageLayout {
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
    let mut page = 0usize;
    let mut y = 0.0f32;

    ctx.fonts_mut(|fonts| {
        for (pi, para) in doc.paras.iter().enumerate() {
            let fmt = para.fmt;
            if fmt.page_break_before && pi > 0 && !draft {
                page += 1;
                y = 0.0;
            }
            if y > 0.0 {
                y += fmt.space_before * pt;
            }
            let hang = if fmt.list != ListKind::None { LIST_HANG * ppi } else { 0.0 };
            let indent = fmt.indent as f32 * INDENT_STEP * ppi + hang;
            let avail = (content_size.x - indent).max(20.0);

            let mut job = LayoutJob::default();
            let empty = para.text.is_empty();
            if empty {
                job.append(" ", 0.0, text_format(book, &para.end_style, ppi));
            } else {
                for (a, b, s) in para.runs() {
                    let text: String = para.text[a..b].iter().collect();
                    job.append(&text, 0.0, text_format(book, &s, ppi));
                }
            }
            job.wrap = TextWrapping { max_width: avail, ..Default::default() };
            job.justify = fmt.align == Align::Justify;
            job.keep_trailing_whitespace = true;
            let galley = fonts.layout_job(job);

            // marker for bullets / numbers
            let marker = if fmt.list != ListKind::None {
                let s = para.style_at(0);
                let text = match fmt.list {
                    ListKind::Bullet => ["•", "◦", "▪"][(fmt.indent as usize) % 3].to_string(),
                    _ => format!("{}.", doc.list_number(pi)),
                };
                let mut mj = LayoutJob::default();
                mj.append(&text, 0.0, text_format(book, &CharStyle { underline: false, strike: false, highlight: None, ..s }, ppi));
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
                // trimmed row width for alignment
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
                    xs.push(offset + glyphs.get(k).map(|g| g.pos.x).unwrap_or(0.0));
                }
                let end_x = if end > start {
                    glyphs.get(end - start - 1).map(|g| g.pos.x + g.advance_width).unwrap_or(0.0)
                } else {
                    0.0
                };
                xs.push(offset + end_x);
                let h = placed.row.size.y.max(1.0) * fmt.line_spacing;
                if !draft && y + h > content_size.y && y > 0.0 {
                    page += 1;
                    y = 0.0;
                }
                lines.push(Line {
                    para: pi,
                    start,
                    end,
                    page,
                    x: indent + offset,
                    y,
                    height: h,
                    galley: single_row(&galley, r),
                    xs: xs.iter().map(|v| v - offset).collect(),
                    last_in_para: r + 1 == nrows,
                    marker: if r == 0 { marker.clone() } else { None },
                    marker_x: indent - hang,
                });
                y += h;
            }
            y += fmt.space_after * pt;
        }
    });

    let pages = page + 1;
    let page_size = if draft { Vec2::new(page_size.x, (y + content_offset.y * 2.0).max(page_size.y)) } else { page_size };
    PageLayout { lines, pages, ppi, page_size, content_offset, content_size, draft }
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

//! One open document (a tab): caret, selection, undo, Google Docs markdown auto-format,
//! interactive checklists, links, comments, suggesting/viewing modes, and the page view.

use eframe::egui::{self, pos2, vec2, Color32, Event, Key, Modifiers, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use std::path::PathBuf;
use std::rc::Rc;

use crate::doc::*;
use crate::fonts::FontBook;
use crate::icons::{self, Icon};
use crate::layout::{self, PageLayout, PAGE_GAP};
use crate::theme::Theme;

pub const UNDO_DEPTH: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DocMode {
    Editing,
    Suggesting,
    Viewing,
}

use serde::{Deserialize, Serialize};

impl DocMode {
    pub fn label(self) -> &'static str {
        match self {
            DocMode::Editing => "Editing",
            DocMode::Suggesting => "Suggesting",
            DocMode::Viewing => "Viewing",
        }
    }
    pub fn icon(self) -> Icon {
        match self {
            DocMode::Editing => Icon::EditMode,
            DocMode::Suggesting => Icon::SuggestMode,
            DocMode::Viewing => Icon::ViewMode,
        }
    }
}

#[derive(Clone)]
struct Snapshot {
    doc: Document,
    caret: Pos,
    anchor: Pos,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Typing,
    Delete,
    Other,
}

#[derive(Clone, Copy, PartialEq)]
struct LayoutKey {
    rev: u64,
    zoom: f32,
    draft: bool,
    ppp: f32,
    fonts_rev: u64,
}

/// Active viewing/editing mode for an opened PDF document.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PdfMode {
    /// Clean, pixel-accurate `hayro` rendering (including any in-place edits, with no editing chrome).
    #[default]
    Read,
    /// In-place text editing directly on top of the exact `hayro` PDF pages.
    Edit,
    /// Converted reflowable Syllabary word-processor document view.
    Reflow,
}

/// Exclusive drag operation on a selected PDF text block (move position or resize width).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum PdfBlockDrag {
    #[default]
    None,
    Move {
        id: u64,
        start_ptr: Pos2,
        start_x_pt: f32,
        start_y_pt: f32,
    },
    ResizeWidth {
        id: u64,
        start_ptr_x: f32,
        start_w_pt: f32,
    },
}

impl PdfBlockDrag {
    fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }

    fn block_id(self) -> Option<u64> {
        match self {
            Self::None => None,
            Self::Move { id, .. } | Self::ResizeWidth { id, .. } => Some(id),
        }
    }
}

/// Pixel-accurate PDF page viewer & in-place text editor backed by `hayro` and `krilla`.
pub struct PdfExactView {
    pub bytes: std::sync::Arc<Vec<u8>>,
    pub page_dims_pt: Vec<(f32, f32)>,
    pub mode: PdfMode,
    pub blocks: Vec<crate::pdf_import::PdfTextBlock>,
    pub selected_block: Option<u64>,
    pub adding_text_box: bool,
    pub undo: Vec<Vec<crate::pdf_import::PdfTextBlock>>,
    pub redo: Vec<Vec<crate::pdf_import::PdfTextBlock>>,
    next_block_id: u64,
    pub editing_checkpointed: Option<u64>,
    drag: PdfBlockDrag,
    textures: std::collections::HashMap<(usize, u32), egui::TextureHandle>,
}

impl PdfExactView {
    pub fn is_exact(&self) -> bool {
        matches!(self.mode, PdfMode::Read | PdfMode::Edit)
    }

    pub fn has_edits(&self) -> bool {
        self.blocks.iter().any(|b| b.modified || b.deleted || b.is_added)
    }

    pub fn checkpoint(&mut self) {
        self.undo.push(self.blocks.clone());
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.blocks, prev));
            self.editing_checkpointed = None;
            if let Some(sel) = self.selected_block {
                if !self.blocks.iter().any(|b| b.id == sel && !b.deleted) {
                    self.selected_block = None;
                }
            }
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.blocks, next));
            self.editing_checkpointed = None;
            if let Some(sel) = self.selected_block {
                if !self.blocks.iter().any(|b| b.id == sel && !b.deleted) {
                    self.selected_block = None;
                }
            }
            true
        } else {
            false
        }
    }

    /// Replaces the backing PDF bytes after saving an edited PDF so Read mode renders
    /// the freshly baked PDF directly via `hayro`.
    pub fn reload_saved(&mut self, bytes: Vec<u8>) {
        let arc = std::sync::Arc::new(bytes);
        if let Some(dims) = crate::pdf::hayro_pdf_info(arc.clone()) {
            if !dims.is_empty() {
                self.page_dims_pt = dims;
            }
        }
        let fresh_blocks = crate::pdf_import::extract_editable_blocks(&arc).unwrap_or_default();
        self.next_block_id = fresh_blocks.iter().map(|b| b.id).max().unwrap_or(0) + 1;
        self.bytes = arc;
        self.blocks = fresh_blocks;
        self.textures.clear();
        self.selected_block = None;
        self.adding_text_box = false;
        self.editing_checkpointed = None;
        self.drag = PdfBlockDrag::None;
        self.undo.clear();
        self.redo.clear();
    }
}

pub struct Tab {
    pub id: u64,
    pub doc: Document,
    pub name: String,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub caret: Pos,
    pub anchor: Pos,
    desired_x: Option<f32>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: EditKind,
    last_edit_time: f64,
    pub rev: u64,
    layout: Option<(LayoutKey, Rc<PageLayout>)>,
    pub pending: Option<CharStyle>,
    pub scroll_to_caret: bool,
    pub scroll_to_page: Option<usize>,
    pub pending_scroll: Option<Vec2>,
    pub canvas_x0: f32,
    pub canvas_w: f32,
    pub saved_rev: u64,
    pub current_page: usize,
    ctx_pos: Option<Pos>,
    pub versions: Vec<DocVersion>,
    pub selected_image: Option<u64>,
    pub selected_image_rect: Option<(u64, usize, Rect)>,
    pub active_table_cell_rect: Option<(u32, u16, u16, Rect, Rect)>,
    pub selected_cells: Vec<(u32, u16, u16)>,
    image_textures: std::collections::HashMap<u64, (usize, egui::TextureHandle)>,
    pub drag: CanvasDrag,
    pub pdf_view: Option<PdfExactView>,
}

/// Exclusive pointer drag interaction active on the document canvas.
/// Replaces four separate `Option<...>` fields so image resize, table column/row resize,
/// and multi-cell selection can never overlap or leave stale partial state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum CanvasDrag {
    #[default]
    Idle,
    ResizingImage {
        id: u64,
        handle: u8,
        start_pos: Pos2,
        w0_in: f32,
        h0_in: f32,
    },
    ResizingTableCol {
        table_id: u32,
        row: usize,
        col: usize,
        start_x: f32,
        start_w_in: f32,
    },
    ResizingTableRow {
        table_id: u32,
        row: usize,
        start_y: f32,
        start_h_pt: f32,
    },
    SelectingCells {
        table_id: u32,
        anchor_row: u16,
        anchor_col: u16,
    },
}

impl CanvasDrag {
    pub fn is_idle(self) -> bool {
        matches!(self, Self::Idle)
    }

    pub fn is_resizing_image(self) -> bool {
        matches!(self, Self::ResizingImage { .. })
    }

    pub fn resizing_image_id(self) -> Option<u64> {
        match self {
            Self::ResizingImage { id, .. } => Some(id),
            _ => None,
        }
    }

    pub fn resizing_image_handle(self) -> Option<(u64, u8)> {
        match self {
            Self::ResizingImage { id, handle, .. } => Some((id, handle)),
            _ => None,
        }
    }
}

pub struct ViewOpts<'a> {
    pub zoom: f32,
    pub draft: bool,
    pub show_marks: bool,
    pub margin_guides: bool,
    pub block_cursor: bool,
    pub header_footer: bool,
    pub matches: &'a [(Pos, Pos)],
    pub theme: &'a Theme,
    pub book: &'a FontBook,
    pub fonts_rev: u64,
    pub speller: Option<&'a crate::spell::Speller>,
    pub mode: DocMode,
    pub shortcuts: ShortcutPreset,
}

pub struct SmartCorrect<'a> {
    pub enabled: bool,
    pub quotes: bool,
    pub markdown: bool,
    pub auto_links: bool,
    pub table: &'a [(String, String)],
}

/// What the editor asks the app to do (menu-level actions triggered by keys or canvas interactions).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    New,
    Open,
    Save,
    SaveAs,
    Close,
    Find,
    Replace,
    GoTo,
    Print,
    NextTab,
    ZoomIn,
    ZoomOut,
    Zoom100,
    Spell,
    Cut,
    Copy,
    Paste,
    PastePlain,
    Link,
    Comment,
    Footnote,
    WordCount,
    Dictionary,
    CompactMode,
    SearchMenus,
    VersionHistory,
    AtMenu,
    Checklist,
    BulletList,
    NumberList,
    DuplicatePara,
    OpenComments,
    InsertImage,
    ImageProps(u64),
    PageSetup,
}

impl Tab {
    pub fn new(id: u64, name: String, doc: Document) -> Self {
        let initial_ver = DocVersion {
            name: "Initial version".into(),
            timestamp: chrono::Local::now().format("%b %-d, %-I:%M %p").to_string(),
            words: doc.counts(None).1,
            snapshot_json: to_json(&doc),
        };
        Self {
            id,
            doc,
            name,
            path: None,
            dirty: false,
            caret: Pos::default(),
            anchor: Pos::default(),
            desired_x: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: EditKind::Other,
            last_edit_time: 0.0,
            rev: 1,
            layout: None,
            pending: None,
            scroll_to_caret: false,
            scroll_to_page: None,
            pending_scroll: None,
            canvas_x0: 0.0,
            canvas_w: 0.0,
            saved_rev: 1,
            current_page: 0,
            ctx_pos: None,
            versions: vec![initial_ver],
            selected_image: None,
            selected_image_rect: None,
            active_table_cell_rect: None,
            selected_cells: Vec::new(),
            image_textures: Default::default(),
            drag: CanvasDrag::Idle,
            pdf_view: None,
        }
    }

    /// Attaches raw PDF bytes for pixel-accurate `hayro` rendering and extracts positioned
    /// text blocks for in-place editing (`PdfMode::Read` ↔ `PdfMode::Edit`).
    pub fn attach_pdf_exact_view(&mut self, bytes: Vec<u8>) {
        let arc = std::sync::Arc::new(bytes);
        if let Some(dims) = crate::pdf::hayro_pdf_info(arc.clone()) {
            if !dims.is_empty() {
                let blocks = crate::pdf_import::extract_editable_blocks(&arc).unwrap_or_default();
                let next_block_id = blocks.iter().map(|b| b.id).max().unwrap_or(0) + 1;
                self.pdf_view = Some(PdfExactView {
                    bytes: arc,
                    page_dims_pt: dims,
                    mode: PdfMode::Read,
                    blocks,
                    selected_block: None,
                    adding_text_box: false,
                    undo: Vec::new(),
                    redo: Vec::new(),
                    next_block_id,
                    editing_checkpointed: None,
                    drag: PdfBlockDrag::None,
                    textures: std::collections::HashMap::new(),
                });
            }
        }
    }

    pub fn pdf_mode(&self) -> Option<PdfMode> {
        self.pdf_view.as_ref().map(|pv| pv.mode)
    }

    pub fn set_pdf_mode(&mut self, mode: PdfMode) {
        if let Some(pv) = self.pdf_view.as_mut() {
            pv.mode = mode;
            if mode != PdfMode::Edit {
                pv.selected_block = None;
                pv.adding_text_box = false;
                pv.editing_checkpointed = None;
                pv.drag = PdfBlockDrag::None;
            }
        }
    }

    pub fn pdf_undo(&mut self) -> bool {
        if let Some(pv) = self.pdf_view.as_mut() {
            if pv.mode == PdfMode::Edit && pv.undo() {
                self.rev += 1;
                self.dirty = pv.has_edits();
                return true;
            }
        }
        false
    }

    pub fn pdf_redo(&mut self) -> bool {
        if let Some(pv) = self.pdf_view.as_mut() {
            if pv.mode == PdfMode::Edit && pv.redo() {
                self.rev += 1;
                self.dirty = pv.has_edits();
                return true;
            }
        }
        false
    }

    pub fn save_version(&mut self, name: impl Into<String>) {
        let v = DocVersion {
            name: name.into(),
            timestamp: chrono::Local::now().format("%b %-d, %-I:%M:%S %p").to_string(),
            words: self.doc.counts(None).1,
            snapshot_json: to_json(&self.doc),
        };
        self.versions.push(v);
    }

    pub fn title(&self) -> String {
        if self.dirty {
            format!("{} •", self.name)
        } else {
            self.name.clone()
        }
    }

    pub fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }
    pub fn selection(&self) -> (Pos, Pos) {
        order(self.anchor, self.caret)
    }
    pub fn selected_text(&self) -> String {
        let (a, b) = self.selection();
        self.doc.text_range(a, b)
    }

    // ── undo ────────────────────────────────────────────────
    pub fn checkpoint(&mut self, kind: EditKind, now: f64) {
        let coalesce = kind != EditKind::Other && kind == self.last_edit && now - self.last_edit_time < 1.5 && !self.undo.is_empty();
        if !coalesce {
            self.undo.push(Snapshot { doc: self.doc.clone(), caret: self.caret, anchor: self.anchor });
            if self.undo.len() > UNDO_DEPTH {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_edit = kind;
        self.last_edit_time = now;
    }
    pub fn changed(&mut self) {
        if let Some(p) = self.doc.paras.get(self.caret.p) {
            let fmt = p.fmt;
            if fmt.table_id > 0 {
                self.doc
                    .evaluate_table_formulas_live(fmt.table_id, (fmt.table_row as usize, fmt.table_col as usize));
            }
        }
        self.rev += 1;
        self.dirty = true;
        self.scroll_to_caret = true;
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            self.redo.push(Snapshot { doc: self.doc.clone(), caret: self.caret, anchor: self.anchor });
            self.doc = s.doc;
            self.caret = self.doc.clamp(s.caret);
            self.anchor = self.doc.clamp(s.anchor);
            self.last_edit = EditKind::Other;
            self.changed();
        }
    }
    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            self.undo.push(Snapshot { doc: self.doc.clone(), caret: self.caret, anchor: self.anchor });
            self.doc = s.doc;
            self.caret = self.doc.clamp(s.caret);
            self.anchor = self.doc.clamp(s.anchor);
            self.last_edit = EditKind::Other;
            self.changed();
        }
    }

    // ── editing ─────────────────────────────────────────────
    pub fn typing_style(&self) -> CharStyle {
        if let Some(p) = self.pending {
            return p;
        }
        if self.has_selection() {
            let (a, _) = self.selection();
            return self.doc.style_at(self.doc.next(a));
        }
        self.doc.style_at(self.caret)
    }

    pub fn delete_selection(&mut self) -> bool {
        if !self.has_selection() {
            return false;
        }
        let (a, b) = self.selection();
        self.caret = self.doc.delete_range(a, b);
        self.anchor = self.caret;
        true
    }

    pub fn insert(&mut self, text: &str, now: f64) {
        let mut style = self.typing_style();
        if text == " " || text == "\n" {
            style.link = 0;
        }
        self.checkpoint(EditKind::Typing, now);
        self.delete_selection();
        self.caret = self.doc.insert_text(self.caret, text, style);
        self.anchor = self.caret;
        self.desired_x = None;
        self.changed();
    }

    pub fn type_char(&mut self, s: &str, now: f64, sc: &SmartCorrect) {
        let mut text = s.to_string();
        if sc.quotes && (s == "\"" || s == "'") {
            let prev = if self.caret.c > 0 { Some(self.doc.paras[self.caret.p].text[self.caret.c - 1]) } else { None };
            let opening = prev.is_none_or(|c| c.is_whitespace() || "([{“‘—–".contains(c));
            text = match (s, opening) {
                ("\"", true) => "“",
                ("\"", false) => "”",
                (_, true) => "‘",
                _ => "’",
            }
            .to_string();
        }
        self.insert(&text, now);
        if text == " " && sc.markdown && self.try_markdown_prefix(now) {
            return;
        }
        let mut chars = text.chars();
        if let (Some(ch), None) = (chars.next(), chars.next()) {
            if ch == ' ' || ".,;:!?)".contains(ch) {
                if sc.auto_links {
                    self.try_auto_link();
                }
                if sc.enabled {
                    self.smart_correct(sc.table);
                }
            }
        }
    }

    /// Google Docs "Automatically detect Markdown" prefix rules on Space.
    fn try_markdown_prefix(&mut self, now: f64) -> bool {
        let p = self.caret.p;
        let c = self.caret.c;
        let line_prefix: String = self.doc.paras[p].text[..c].iter().collect();
        let (style_idx, list_kind, checked) = match line_prefix.as_str() {
            "# " => (Some(4), None, false),
            "## " => (Some(5), None, false),
            "### " => (Some(6), None, false),
            "> " => (Some(7), None, false),
            "* " | "- " => (None, Some(ListKind::Bullet), false),
            "1. " => (None, Some(ListKind::Number), false),
            "[] " | "[ ] " => (None, Some(ListKind::Checklist), false),
            "[x] " | "[X] " => (None, Some(ListKind::Checklist), true),
            _ => return false,
        };
        self.doc.delete_range(Pos::new(p, 0), Pos::new(p, c));
        self.caret = Pos::new(p, 0);
        self.anchor = self.caret;
        if let Some(si) = style_idx {
            self.apply_named_style(si, now);
        }
        if let Some(lk) = list_kind {
            self.doc.paras[p].fmt.list = lk;
            self.doc.paras[p].fmt.checked = checked;
            self.changed();
        }
        true
    }

    fn try_auto_link(&mut self) {
        let p = self.caret.p;
        let end = self.caret.c.saturating_sub(1);
        let t = &self.doc.paras[p].text;
        let mut start = end;
        while start > 0 && !t[start - 1].is_whitespace() {
            start -= 1;
        }
        if start >= end {
            return;
        }
        let word: String = t[start..end].iter().collect();
        if word.starts_with("https://") || word.starts_with("http://") || word.starts_with("www.") {
            let url = if word.starts_with("www.") { format!("https://{word}") } else { word };
            let lid = self.doc.add_link(&url);
            for s in &mut self.doc.paras[p].styles[start..end] {
                s.link = lid;
                s.underline = true;
                if s.color == [0, 0, 0] {
                    s.color = [11, 87, 208];
                }
            }
            self.changed();
        }
    }

    fn smart_correct(&mut self, table: &[(String, String)]) {
        let p = self.caret.p;
        let end = self.caret.c.saturating_sub(1); // position of the trigger char
        let t = &self.doc.paras[p].text;
        let mut start = end;
        while start > 0 && !t[start - 1].is_whitespace() {
            start -= 1;
        }
        if start >= end {
            return;
        }
        let word: String = t[start..end].iter().collect();
        for (from, to) in table {
            if *from == word {
                let style = self.doc.paras[p].styles[start];
                self.doc.delete_range(Pos::new(p, start), Pos::new(p, end));
                let after = self.doc.insert_text(Pos::new(p, start), to, style);
                self.caret = Pos::new(p, after.c + 1);
                self.anchor = self.caret;
                self.changed();
                return;
            }
        }
    }

    pub fn enter(&mut self, now: f64) {
        self.checkpoint(EditKind::Other, now);
        self.delete_selection();
        // Enter on a table formula (e.g. "=SUM(ABOVE)") evaluates the formula immediately in the cell.
        let p_str = self.doc.paras[self.caret.p].string();
        if self.doc.paras[self.caret.p].fmt.table_id > 0 && p_str.trim_start().starts_with('=') {
            let tid = self.doc.paras[self.caret.p].fmt.table_id;
            self.doc.evaluate_table_formulas(tid);
            let end_c = self.doc.paras.get(self.caret.p).map_or(0, |p| p.len());
            self.caret = self.doc.clamp(Pos::new(self.caret.p, end_c));
            self.anchor = self.caret;
            self.changed();
            return;
        }
        // Enter on "---" turns it into a horizontal divider line.
        if (p_str == "---" || p_str == "***") && self.caret.c == p_str.len() {
            let p = self.caret.p;
            self.doc.delete_range(Pos::new(p, 0), Pos::new(p, p_str.len()));
            self.doc.paras[p].fmt.hr = true;
            self.caret = self.doc.split(Pos::new(p, 0));
            self.anchor = self.caret;
            self.changed();
            return;
        }
        // Enter on an empty list item ends the list.
        let para = &self.doc.paras[self.caret.p];
        if para.text.is_empty() && para.fmt.list != ListKind::None {
            self.doc.paras[self.caret.p].fmt.list = ListKind::None;
            self.doc.paras[self.caret.p].fmt.checked = false;
            self.changed();
            return;
        }
        let mut style = self.typing_style();
        style.link = 0;
        self.caret = self.doc.split(self.caret);
        if self.doc.paras[self.caret.p].text.is_empty() && self.doc.paras[self.caret.p].fmt.style == 0 {
            self.doc.paras[self.caret.p].end_style = style;
        }
        self.anchor = self.caret;
        self.changed();
    }

    pub fn backspace(&mut self, word: bool, now: f64) {
        if let Some(img_id) = self.selected_image {
            self.remove_image(img_id, now);
            return;
        }
        if self.selected_cells.len() > 1 {
            self.checkpoint(EditKind::Delete, now);
            let cells = self.selected_cells.clone();
            let mut tid_eval = 0u32;
            for (tid, r, c) in cells {
                tid_eval = tid;
                self.doc.set_cell_text(tid, r as usize, c as usize, "");
            }
            if tid_eval > 0 {
                self.doc.evaluate_table_formulas(tid_eval);
            }
            self.changed();
            return;
        }
        self.checkpoint(EditKind::Delete, now);
        if !self.delete_selection() {
            let para = &self.doc.paras[self.caret.p];
            if self.caret.c == 0 && (para.fmt.list != ListKind::None || para.fmt.indent > 0 || para.fmt.hr || para.fmt.image_id > 0) {
                let f = &mut self.doc.paras[self.caret.p].fmt;
                if f.image_id > 0 {
                    f.image_id = 0;
                    self.selected_image = None;
                } else if f.hr {
                    f.hr = false;
                } else if f.list != ListKind::None {
                    f.list = ListKind::None;
                    f.checked = false;
                } else {
                    f.indent -= 1;
                }
            } else {
                let from = if word { self.doc.word_left(self.caret) } else { self.doc.prev(self.caret) };
                let cur_f = self.doc.paras[self.caret.p].fmt;
                let prev_f = self.doc.paras[from.p].fmt;
                let crosses_cell = from.p != self.caret.p
                    && (cur_f.table_id > 0 || prev_f.table_id > 0)
                    && (cur_f.table_id != prev_f.table_id
                        || cur_f.table_row != prev_f.table_row
                        || cur_f.table_col != prev_f.table_col);
                if !crosses_cell {
                    self.caret = self.doc.delete_range(from, self.caret);
                }
            }
            self.anchor = self.caret;
        }
        self.changed();
    }

    pub fn delete_forward(&mut self, word: bool, now: f64) {
        if let Some(img_id) = self.selected_image {
            self.remove_image(img_id, now);
            return;
        }
        if self.selected_cells.len() > 1 {
            self.checkpoint(EditKind::Delete, now);
            let cells = self.selected_cells.clone();
            let mut tid_eval = 0u32;
            for (tid, r, c) in cells {
                tid_eval = tid;
                self.doc.set_cell_text(tid, r as usize, c as usize, "");
            }
            if tid_eval > 0 {
                self.doc.evaluate_table_formulas(tid_eval);
            }
            self.changed();
            return;
        }
        self.checkpoint(EditKind::Delete, now);
        if !self.delete_selection() {
            if self.caret.c == 0 && self.doc.paras[self.caret.p].fmt.image_id > 0 {
                self.doc.paras[self.caret.p].fmt.image_id = 0;
                self.selected_image = None;
            } else {
                let to = if word { self.doc.word_right(self.caret) } else { self.doc.next(self.caret) };
                let cur_f = self.doc.paras[self.caret.p].fmt;
                let next_f = self.doc.paras[to.p].fmt;
                let crosses_cell = to.p != self.caret.p
                    && (cur_f.table_id > 0 || next_f.table_id > 0)
                    && (cur_f.table_id != next_f.table_id
                        || cur_f.table_row != next_f.table_row
                        || cur_f.table_col != next_f.table_col);
                if !crosses_cell {
                    self.caret = self.doc.delete_range(self.caret, to);
                }
                self.anchor = self.caret;
            }
        }
        self.changed();
    }

    pub fn insert_image(&mut self, img: DocImage, now: f64) {
        self.checkpoint(EditKind::Other, now);
        self.delete_selection();
        let img_id = img.id;
        self.doc.images.push(img);
        if !self.doc.paras[self.caret.p].text.is_empty()
            || self.doc.paras[self.caret.p].fmt.hr
            || self.doc.paras[self.caret.p].fmt.image_id > 0
        {
            self.caret = self.doc.split(self.caret);
        }
        let img_p = self.caret.p;
        self.doc.paras[img_p].fmt.image_id = img_id;
        self.doc.paras[img_p].fmt.align = Align::Center;
        if img_p + 1 == self.doc.paras.len() {
            self.doc.split(Pos::new(img_p, 0));
        }
        self.caret = Pos::new(img_p, 0);
        self.anchor = self.caret;
        self.selected_image = Some(img_id);
        self.changed();
    }

    pub fn remove_image(&mut self, img_id: u64, now: f64) {
        self.checkpoint(EditKind::Delete, now);
        for p in &mut self.doc.paras {
            if p.fmt.image_id == img_id {
                p.fmt.image_id = 0;
            }
        }
        self.selected_image = None;
        self.selected_image_rect = None;
        self.changed();
    }

    pub fn page_break(&mut self, now: f64) {
        self.checkpoint(EditKind::Other, now);
        self.delete_selection();
        self.caret = self.doc.split(self.caret);
        self.doc.paras[self.caret.p].fmt.page_break_before = true;
        self.anchor = self.caret;
        self.changed();
    }

    pub fn insert_hr(&mut self, now: f64) {
        self.checkpoint(EditKind::Other, now);
        self.delete_selection();
        if !self.doc.paras[self.caret.p].text.is_empty() {
            self.caret = self.doc.split(self.caret);
        }
        self.doc.paras[self.caret.p].fmt.hr = true;
        self.caret = self.doc.split(self.caret);
        self.anchor = self.caret;
        self.changed();
    }

    pub fn duplicate_para(&mut self, now: f64) {
        self.checkpoint(EditKind::Other, now);
        let p = self.caret.p;
        let cloned = self.doc.paras[p].clone();
        self.doc.paras.insert(p + 1, cloned);
        self.caret = Pos::new(p + 1, self.caret.c);
        self.anchor = self.caret;
        self.changed();
    }

    pub fn select_all(&mut self) {
        self.anchor = Pos::new(0, 0);
        self.caret = self.doc.end();
        self.pending = None;
    }

    /// Change character formatting of the selection, of all selected table cells, or of the next typed text.
    pub fn apply_char(&mut self, now: f64, f: &dyn Fn(&mut CharStyle)) {
        if !self.selected_cells.is_empty() && !self.has_selection() {
            self.checkpoint(EditKind::Other, now);
            let sel_cells = self.selected_cells.clone();
            for p in &mut self.doc.paras {
                if sel_cells.contains(&(p.fmt.table_id, p.fmt.table_row, p.fmt.table_col)) {
                    for s in &mut p.styles {
                        f(s);
                    }
                    f(&mut p.end_style);
                }
            }
            self.changed();
            self.scroll_to_caret = false;
        } else if self.has_selection() {
            self.checkpoint(EditKind::Other, now);
            let (a, b) = self.selection();
            self.doc.restyle(a, b, f);
            self.changed();
            self.scroll_to_caret = false;
        } else {
            let mut s = self.typing_style();
            f(&mut s);
            let para = &mut self.doc.paras[self.caret.p];
            if para.text.is_empty() {
                f(&mut para.end_style);
                self.rev += 1;
            }
            self.pending = Some(s);
        }
    }

    /// Toggle a boolean attribute based on the current state.
    pub fn toggle(&mut self, now: f64, get: fn(&CharStyle) -> bool, set: fn(&mut CharStyle, bool)) {
        let on = !get(&self.typing_style());
        self.apply_char(now, &move |s| set(s, on));
    }

    pub fn apply_para(&mut self, now: f64, f: &dyn Fn(&mut ParaFmt)) {
        self.checkpoint(EditKind::Other, now);
        if !self.selected_cells.is_empty() && !self.has_selection() {
            let sel_cells = self.selected_cells.clone();
            for p in &mut self.doc.paras {
                if sel_cells.contains(&(p.fmt.table_id, p.fmt.table_row, p.fmt.table_col)) {
                    f(&mut p.fmt);
                }
            }
        } else {
            for p in self.doc.para_range(self.anchor, self.caret) {
                f(&mut self.doc.paras[p].fmt);
            }
        }
        self.changed();
        self.scroll_to_caret = false;
    }

    pub fn apply_named_style(&mut self, idx: usize, now: f64) {
        let styles = named_styles();
        let Some(ns) = styles.get(idx) else { return };
        self.checkpoint(EditKind::Other, now);
        for p in self.doc.para_range(self.anchor, self.caret) {
            let para = &mut self.doc.paras[p];
            let keep_break = para.fmt.page_break_before;
            para.fmt = ParaFmt { style: idx, page_break_before: keep_break, ..ns.fmt };
            for s in para.styles.iter_mut() {
                // keep emphasis the writer added, reset the rest to the style's look
                let (b, i, u, lk) = (s.bold && !ns.chr.bold, s.italic && !ns.chr.italic, s.underline, s.link);
                *s = ns.chr;
                s.bold |= b;
                s.italic |= i;
                s.underline |= u;
                s.link = lk;
            }
            para.end_style = ns.chr;
        }
        self.pending = None;
        self.changed();
        self.scroll_to_caret = false;
    }

    pub fn clear_formatting(&mut self, now: f64) {
        let p = self.caret.p;
        let style_idx = self.doc.paras[p].fmt.style;
        let base = named_styles().get(style_idx).map(|s| s.chr).unwrap_or_default();
        self.apply_char(now, &move |s| *s = base);
    }

    pub fn change_case(&mut self, kind: CaseChange, now: f64) {
        if !self.has_selection() {
            let (a, b) = self.doc.word_at(self.caret);
            self.anchor = a;
            self.caret = b;
        }
        self.checkpoint(EditKind::Other, now);
        let (a, b) = self.selection();
        crate::doc::change_case(&mut self.doc, a, b, kind);
        self.changed();
    }

    pub fn paste(&mut self, text: &str, clip: Option<&(String, Fragment)>, now: f64) {
        self.checkpoint(EditKind::Other, now);
        self.delete_selection();
        let norm = text.replace("\r\n", "\n");
        match clip {
            Some((t, frag)) if t.replace("\r\n", "\n") == norm => {
                self.caret = self.doc.insert_fragment(self.caret, frag);
            }
            _ => {
                let style = self.doc.style_at(self.caret);
                self.caret = self.doc.insert_text(self.caret, &norm, style);
            }
        }
        self.anchor = self.caret;
        self.changed();
    }

    pub fn paste_plain(&mut self, text: &str, now: f64) {
        self.checkpoint(EditKind::Other, now);
        self.delete_selection();
        let norm = text.replace("\r\n", "\n");
        let mut style = self.doc.style_at(self.caret);
        style.link = 0;
        self.caret = self.doc.insert_text(self.caret, &norm, style);
        self.anchor = self.caret;
        self.changed();
    }

    pub fn copy(&self) -> Option<(String, Fragment)> {
        if !self.has_selection() {
            return None;
        }
        let (a, b) = self.selection();
        Some((self.doc.text_range(a, b), self.doc.fragment(a, b)))
    }

    pub fn current_style_name(&self) -> &'static str {
        let idx = self.doc.paras[self.doc.clamp(self.caret).p].fmt.style;
        named_styles().get(idx).map_or("Default Text", |s| s.name)
    }

    pub fn set_caret(&mut self, pos: Pos, extend: bool) {
        self.caret = self.doc.clamp(pos);
        if !extend {
            self.anchor = self.caret;
            self.selected_cells.clear();
        }
        self.pending = None;
        self.scroll_to_caret = true;
    }

    /// Selects an entire row `row` in `table_id`.
    pub fn select_table_row(&mut self, table_id: u32, row: usize) {
        self.selected_cells = self.doc.table_row_cells(table_id, row);
        if let Some(pi) = self
            .doc
            .paras
            .iter()
            .position(|p| p.fmt.table_id == table_id && p.fmt.table_row as usize == row)
        {
            self.caret = Pos::new(pi, 0);
            self.anchor = self.caret;
            self.pending = None;
            self.scroll_to_caret = false;
        }
    }

    /// Selects an entire column `col` in `table_id`.
    pub fn select_table_col(&mut self, table_id: u32, col: usize) {
        self.selected_cells = self.doc.table_col_cells(table_id, col);
        if let Some(pi) = self
            .doc
            .paras
            .iter()
            .position(|p| p.fmt.table_id == table_id && p.fmt.table_col as usize == col)
        {
            self.caret = Pos::new(pi, 0);
            self.anchor = self.caret;
            self.pending = None;
            self.scroll_to_caret = false;
        }
    }

    // ── layout ──────────────────────────────────────────────
    pub fn layout(&mut self, ctx: &egui::Context, opts: &ViewOpts) -> Rc<PageLayout> {
        let key = LayoutKey { rev: self.rev, zoom: opts.zoom, draft: opts.draft || self.doc.pageless, ppp: ctx.pixels_per_point(), fonts_rev: opts.fonts_rev };
        if let Some((k, l)) = &self.layout {
            if *k == key {
                return l.clone();
            }
        }
        let l = Rc::new(layout::build(ctx, &self.doc, opts.book, opts.zoom, opts.draft));
        self.layout = Some((key, l.clone()));
        l
    }

    /// Misspelled ranges in a paragraph, leaving out the word being typed.
    pub fn spell_errors(&self, sp: &crate::spell::Speller, p: usize) -> Vec<(usize, usize)> {
        let para = &self.doc.paras[p];
        let caret = self.doc.clamp(self.caret);
        sp.errors(&para.text)
            .into_iter()
            .filter(|(a, b)| !(caret.p == p && *a <= caret.c && caret.c <= *b && !self.has_selection() && caret.c == para.len().min(*b) && *b == caret.c))
            .collect()
    }

    fn context_menu(&mut self, resp: &egui::Response, opts: &ViewOpts, cmds: &mut Vec<Cmd>, now: f64) {
        let has_sel = self.has_selection();
        let pos = self.ctx_pos.unwrap_or(self.caret);
        let pos = self.doc.clamp(pos);
        // misspelled word under the click
        let mut wrong: Option<(Pos, Pos, String)> = None;
        if let Some(sp) = opts.speller {
            for (a, b) in self.spell_errors(sp, pos.p) {
                if a <= pos.c && pos.c <= b {
                    let w: String = self.doc.paras[pos.p].text[a..b].iter().collect();
                    wrong = Some((Pos::new(pos.p, a), Pos::new(pos.p, b), w));
                }
            }
        }
        let kp = opts.shortcuts;
        let ctx_img_id = self.doc.paras[pos.p].fmt.image_id;
        resp.context_menu(|ui| {
            crate::chrome::fit_menu(
                ui,
                &[
                    ("Image size & caption…", ""),
                    ("Cut", "Ctrl+X"),
                    ("Copy", "Ctrl+C"),
                    ("Paste", "Ctrl+V"),
                    ("Paste without formatting", kp.paste_plain()),
                    ("Comment", "Ctrl+Alt+M"),
                    ("Insert link", "Ctrl+K"),
                    ("Insert image from file…", ""),
                    ("Insert smart chip / building block", "@"),
                    ("Define word", kp.dictionary()),
                    ("Select All", "Ctrl+A"),
                    ("Clear formatting", kp.clear_format()),
                    ("Capitalization", "▸"),
                ],
            );
            if ctx_img_id > 0 {
                if ui.button(egui::RichText::new("Image size & caption…").strong()).clicked() {
                    self.selected_image = Some(ctx_img_id);
                    cmds.push(Cmd::ImageProps(ctx_img_id));
                    ui.close();
                }
                if ui.button("Remove image").clicked() {
                    self.remove_image(ctx_img_id, now);
                    ui.close();
                }
                ui.separator();
            }
            if let (Some((a, b, w)), Some(sp)) = (&wrong, opts.speller) {
                let sugg = sp.suggest(w);
                if sugg.is_empty() {
                    ui.label(egui::RichText::new("No spelling suggestions").weak());
                }
                for s in sugg {
                    if ui.button(egui::RichText::new(&s).strong()).clicked() {
                        self.checkpoint(EditKind::Other, now);
                        let style = self.doc.paras[a.p].styles.get(a.c).copied().unwrap_or_default();
                        self.doc.delete_range(*a, *b);
                        let e = self.doc.insert_text(*a, &s, style);
                        self.set_caret(e, false);
                        self.changed();
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Ignore All").clicked() {
                    sp.ignore(w);
                    self.rev += 1;
                    ui.close();
                }
                if ui.button("Add to Personal Dictionary").clicked() {
                    sp.add(w);
                    self.rev += 1;
                    ui.close();
                }
                ui.separator();
            }
            if ui.add_enabled(has_sel, egui::Button::new("Cut").shortcut_text("Ctrl+X")).clicked() {
                cmds.push(Cmd::Cut);
                ui.close();
            }
            if ui.add_enabled(has_sel, egui::Button::new("Copy").shortcut_text("Ctrl+C")).clicked() {
                cmds.push(Cmd::Copy);
                ui.close();
            }
            if ui.add(egui::Button::new("Paste").shortcut_text("Ctrl+V")).clicked() {
                cmds.push(Cmd::Paste);
                ui.close();
            }
            if ui.add(egui::Button::new("Paste without formatting").shortcut_text(kp.paste_plain())).clicked() {
                cmds.push(Cmd::PastePlain);
                ui.close();
            }
            ui.separator();
            if ui.add(egui::Button::new("Comment").shortcut_text("Ctrl+Alt+M")).clicked() {
                cmds.push(Cmd::Comment);
                ui.close();
            }
            if ui.add(egui::Button::new("Insert link").shortcut_text("Ctrl+K")).clicked() {
                cmds.push(Cmd::Link);
                ui.close();
            }
            if ui.button("Insert image from file…").clicked() {
                cmds.push(Cmd::InsertImage);
                ui.close();
            }
            if ui.add(egui::Button::new("Insert smart chip / building block").shortcut_text("@")).clicked() {
                cmds.push(Cmd::AtMenu);
                ui.close();
            }
            if ui.add(egui::Button::new("Define word").shortcut_text(kp.dictionary())).clicked() {
                cmds.push(Cmd::Dictionary);
                ui.close();
            }
            ui.separator();
            if ui.add(egui::Button::new("Select All").shortcut_text("Ctrl+A")).clicked() {
                self.select_all();
                ui.close();
            }
            if ui.add(egui::Button::new("Clear formatting").shortcut_text(kp.clear_format())).clicked() {
                self.clear_formatting(now);
                ui.close();
            }
            ui.menu_button("Capitalization", |ui| {
                crate::chrome::fit_menu(
                    ui,
                    &[
                        ("UPPERCASE", ""),
                        ("lowercase", ""),
                        ("Title Case", ""),
                        ("Sentence case", ""),
                    ],
                );
                for (l, k) in [("UPPERCASE", CaseChange::Upper), ("lowercase", CaseChange::Lower), ("Title Case", CaseChange::Title), ("Sentence case", CaseChange::Sentence)] {
                    if ui.button(l).clicked() {
                        self.change_case(k, now);
                        ui.close();
                    }
                }
            });
        });
    }

    pub fn cached_pages(&self) -> usize {
        self.layout.as_ref().map_or(1, |(_, l)| l.pages)
    }

    pub fn first_pos_on_page(&self, page: usize) -> Option<Pos> {
        let (_, l) = self.layout.as_ref()?;
        let line = l.lines.iter().find(|ln| ln.page >= page)?;
        Some(Pos::new(line.para, line.start))
    }

    fn hit(&self, lay: &PageLayout, canvas: Rect, p: Pos2) -> Pos {
        if lay.lines.is_empty() {
            return Pos::default();
        }
        let local = p - canvas.min;
        let stride = lay.page_size.y + PAGE_GAP;
        let k = (((local.y - PAGE_GAP) / stride).floor().max(0.0) as usize).min(lay.pages - 1);
        let po = lay.page_origin(k, canvas.width());
        let cx = local.x - po.x - lay.content_offset.x;
        let cy = local.y - po.y - lay.content_offset.y;

        // If the click lands inside a table cell on page `k`, prefer lines belonging to that cell
        let hit_cell = lay
            .cells
            .iter()
            .find(|c| c.page == k && c.rect.contains(pos2(cx, cy)))
            .map(|c| (c.table_id, c.row, c.col));

        let mut best_li: Option<usize> = None;
        let mut best_score = (f32::INFINITY, f32::INFINITY);
        for (i, l) in lay.lines.iter().enumerate() {
            if l.page != k {
                continue;
            }
            if let Some((tid, r, c)) = hit_cell {
                let pfmt = self.doc.paras[l.para].fmt;
                if pfmt.table_id != tid || pfmt.table_row != r || pfmt.table_col != c {
                    continue;
                }
            }
            let dy = if cy < l.y {
                l.y - cy
            } else if cy > l.y + l.height {
                cy - (l.y + l.height)
            } else {
                0.0
            };
            let lx1 = l.x + l.xs.last().copied().unwrap_or(0.0);
            let dx = if cx < l.x {
                l.x - cx
            } else if cx > lx1 {
                cx - lx1
            } else {
                0.0
            };
            let score = (dy, dx);
            if score < best_score {
                best_score = score;
                best_li = Some(i);
            }
        }
        let li = best_li.unwrap_or_else(|| {
            lay.lines
                .iter()
                .position(|l| l.page >= k)
                .unwrap_or(lay.lines.len() - 1)
        });
        let c = lay.char_at_x(li, cx);
        Pos::new(lay.lines[li].para, c)
    }

    fn move_vert(&mut self, lay: &PageLayout, lines: isize, extend: bool) {
        let li = lay.line_of(self.caret.p, self.caret.c);
        let x = self.desired_x.unwrap_or_else(|| lay.x_in_line(li, self.caret.c));
        let cur_fmt = self.doc.paras[self.caret.p].fmt;

        // Inside a table cell, Up/Down must move vertically within the cell and then to the
        // same column in the row above/below — NEVER sideways to an adjacent column!
        if cur_fmt.table_id > 0 && lines.abs() == 1 {
            let tid = cur_fmt.table_id;
            let r = cur_fmt.table_row as usize;
            let c = cur_fmt.table_col as usize;
            let cell_lines: Vec<usize> = lay
                .lines
                .iter()
                .enumerate()
                .filter(|(_, l)| {
                    let f = self.doc.paras[l.para].fmt;
                    f.table_id == tid && f.table_row as usize == r && f.table_col as usize == c
                })
                .map(|(i, _)| i)
                .collect();
            if let Some(pos_in_cell) = cell_lines.iter().position(|&idx| idx == li) {
                let next_in_cell = pos_in_cell as isize + lines;
                if next_in_cell >= 0 && (next_in_cell as usize) < cell_lines.len() {
                    let tli = cell_lines[next_in_cell as usize];
                    let pos = Pos::new(lay.lines[tli].para, lay.char_at_x(tli, x));
                    self.set_caret(pos, extend);
                    self.desired_x = Some(x);
                    return;
                }
            }
            if let Some(tbl) = self.doc.table(tid) {
                let target_row = r as isize + lines;
                if target_row >= 0 && (target_row as usize) < tbl.rows.len() {
                    let tr = target_row as usize;
                    let max_col = tbl.rows[tr].cells.len().saturating_sub(1);
                    let tc = c.min(max_col);
                    if let Some(tp) = self.doc.table_cell_para(tid, tr, tc) {
                        let tli = lay.line_of(tp, 0);
                        let pos = Pos::new(lay.lines[tli].para, lay.char_at_x(tli, x));
                        self.set_caret(pos, extend);
                        self.desired_x = Some(x);
                        return;
                    }
                } else if target_row < 0 {
                    // Move to paragraph immediately before the table
                    let first_tbl_p = self.doc.paras.iter().position(|p| p.fmt.table_id == tid).unwrap_or(0);
                    if first_tbl_p > 0 {
                        let prev_p = first_tbl_p - 1;
                        let tli = lay.line_of(prev_p, self.doc.paras[prev_p].len());
                        let pos = Pos::new(lay.lines[tli].para, lay.char_at_x(tli, x));
                        self.set_caret(pos, extend);
                        self.desired_x = Some(x);
                        return;
                    }
                } else {
                    // Move to paragraph immediately after the table
                    let last_tbl_p = self.doc.paras.iter().rposition(|p| p.fmt.table_id == tid).unwrap_or(self.caret.p);
                    if last_tbl_p + 1 < self.doc.paras.len() {
                        let next_p = last_tbl_p + 1;
                        let tli = lay.line_of(next_p, 0);
                        let pos = Pos::new(lay.lines[tli].para, lay.char_at_x(tli, x));
                        self.set_caret(pos, extend);
                        self.desired_x = Some(x);
                        return;
                    }
                }
            }
        }

        let target = (li as isize + lines).clamp(0, lay.lines.len() as isize - 1) as usize;
        let pos = if target == li && lines < 0 {
            Pos::new(self.caret.p, 0)
        } else if target == li && lines > 0 {
            self.doc.end()
        } else {
            Pos::new(lay.lines[target].para, lay.char_at_x(target, x))
        };
        self.set_caret(pos, extend);
        self.desired_x = Some(x);
    }

    /// Moves cell-to-cell in a table on `Tab` / `Shift+Tab` using exact `(row, col)` grid coordinates.
    /// Pressing `Tab` in the very last cell automatically appends a new row and moves into its first cell.
    pub fn tab_in_table(&mut self, shift: bool, now: f64) -> bool {
        let pfmt = self.doc.paras[self.caret.p].fmt;
        if pfmt.table_id == 0 {
            return false;
        }
        let tid = pfmt.table_id;
        let r = pfmt.table_row as usize;
        let c = pfmt.table_col as usize;
        self.doc.evaluate_table_formulas(tid);
        self.selected_cells.clear();

        let Some(tbl) = self.doc.table(tid) else {
            return true;
        };
        let nrows = tbl.rows.len();
        if nrows == 0 {
            return true;
        }
        let cur_cols = tbl.rows.get(r).map_or(1, |row| row.cells.len()).max(1);

        if !shift {
            let target = if c + 1 < cur_cols {
                Some((r, c + 1))
            } else if r + 1 < nrows {
                Some((r + 1, 0))
            } else {
                None
            };
            if let Some((tr, tc)) = target {
                if let Some(tp) = self.doc.table_cell_para(tid, tr, tc) {
                    let end_c = self.doc.paras[tp].len();
                    self.set_caret(Pos::new(tp, end_c), false);
                }
            } else {
                // At the very last cell of the table: append a new row and jump into its first cell!
                self.checkpoint(EditKind::Other, now);
                if let Some(new_pos) = self.doc.table_insert_row(tid, r, false) {
                    self.set_caret(new_pos, false);
                    self.changed();
                }
            }
        } else {
            let target = if c > 0 {
                Some((r, c - 1))
            } else if r > 0 {
                let prev_cols = tbl.rows[r - 1].cells.len().max(1);
                Some((r - 1, prev_cols - 1))
            } else {
                None
            };
            if let Some((tr, tc)) = target {
                if let Some(tp) = self.doc.table_cell_para(tid, tr, tc) {
                    let end_c = self.doc.paras[tp].len();
                    self.set_caret(Pos::new(tp, end_c), false);
                }
            }
        }
        self.rev += 1;
        true
    }

    // ── keyboard ────────────────────────────────────────────
    #[expect(clippy::too_many_arguments, reason = "Keyboard input handler dispatches over layout, clipboard, command queue, and active mode")]
    pub fn handle_keys_mode(
        &mut self,
        ui: &egui::Ui,
        lay: &PageLayout,
        sc: &SmartCorrect,
        clip: &mut Option<(String, Fragment)>,
        cmds: &mut Vec<Cmd>,
        view_h: f32,
        mode: DocMode,
        shortcuts: ShortcutPreset,
    ) {
        let now = ui.input(|i| i.time);
        let events = ui.input(|i| i.events.clone());
        let lines_per_view = ((view_h / (lay.ppi * 0.2)).max(3.0)) as isize;
        for ev in events {
            match ev {
                Event::Text(t) => {
                    if mode == DocMode::Viewing {
                        continue;
                    }
                    let m = ui.input(|i| i.modifiers);
                    if (m.alt || m.command) && t == "/" {
                        cmds.push(Cmd::SearchMenus);
                        continue;
                    }
                    if !(m.ctrl && !m.alt) && !t.chars().any(|c| c.is_control()) {
                        let prev_ch = if self.caret.c > 0 { Some(self.doc.paras[self.caret.p].text[self.caret.c - 1]) } else { None };
                        if t == "@" && (prev_ch.is_none() || prev_ch == Some(' ')) {
                            cmds.push(Cmd::AtMenu);
                            continue;
                        }
                        if mode == DocMode::Suggesting {
                            let start_pos = self.caret;
                            let mut st = self.typing_style();
                            st.highlight = Some([187, 247, 208]);
                            self.pending = Some(st);
                            self.type_char(&t, now, sc);
                            self.record_suggestion(start_pos, self.caret, &t);
                        } else {
                            self.type_char(&t, now, sc);
                        }
                    }
                }
                Event::Copy => {
                    if let Some(c) = self.copy() {
                        ui.ctx().copy_text(c.0.clone());
                        *clip = Some(c);
                    }
                }
                Event::Cut => {
                    if mode == DocMode::Viewing {
                        continue;
                    }
                    if let Some(c) = self.copy() {
                        ui.ctx().copy_text(c.0.clone());
                        *clip = Some(c);
                        self.checkpoint(EditKind::Other, now);
                        self.delete_selection();
                        self.changed();
                    }
                }
                Event::Paste(s) => {
                    if mode == DocMode::Viewing {
                        continue;
                    }
                    if ui.input(|i| i.modifiers.shift) && matches!(shortcuts, ShortcutPreset::GoogleDocs | ShortcutPreset::WordPro) {
                        self.paste_plain(&s, now);
                    } else {
                        self.paste(&s, clip.as_ref(), now);
                    }
                }
                Event::Key { key, pressed: true, modifiers, .. } => {
                    self.key_mode(key, modifiers, now, lay, lines_per_view, cmds, mode, shortcuts);
                }
                _ => {}
            }
        }
    }

    fn record_suggestion(&mut self, a: Pos, b: Pos, typed: &str) {
        if a.p != b.p {
            return;
        }
        if let Some(last) = self.doc.comments.last_mut() {
            if last.is_suggestion && !last.resolved && last.para == a.p && last.end == a.c {
                last.end = b.c;
                last.quote.push_str(typed);
                last.text = format!("Add: “{}”", last.quote);
                return;
            }
        }
        let id = self.doc.comments.len() as u64 + 1;
        self.doc.comments.push(DocComment {
            id,
            para: a.p,
            start: a.c,
            end: b.c,
            quote: typed.to_string(),
            author: "Reviewer (Suggestion)".into(),
            text: format!("Add: “{typed}”"),
            timestamp: chrono::Local::now().format("%-I:%M %p").to_string(),
            resolved: false,
            replies: Vec::new(),
            is_suggestion: true,
        });
    }

    #[expect(clippy::too_many_arguments, reason = "Key press dispatcher requires layout, command queue, document mode, and shortcut preset")]
    fn key_mode(
        &mut self,
        key: Key,
        m: Modifiers,
        now: f64,
        lay: &PageLayout,
        lpv: isize,
        cmds: &mut Vec<Cmd>,
        mode: DocMode,
        shortcuts: ShortcutPreset,
    ) {
        self.last_edit_time = now;
        let cmd = m.command || m.ctrl;
        let shift = m.shift;
        let alt = m.alt;
        if key != Key::ArrowUp && key != Key::ArrowDown && key != Key::PageUp && key != Key::PageDown {
            self.desired_x = None;
        }
        let ro = mode == DocMode::Viewing;
        match key {
            Key::Slash if alt || cmd => cmds.push(Cmd::SearchMenus),
            Key::ArrowLeft => {
                let pos = if self.has_selection() && !shift {
                    self.selection().0
                } else if cmd {
                    self.doc.word_left(self.caret)
                } else {
                    self.doc.prev(self.caret)
                };
                self.set_caret(pos, shift);
            }
            Key::ArrowRight => {
                let pos = if self.has_selection() && !shift {
                    self.selection().1
                } else if cmd {
                    self.doc.word_right(self.caret)
                } else {
                    self.doc.next(self.caret)
                };
                self.set_caret(pos, shift);
            }
            Key::ArrowUp if cmd => {
                let p = if self.caret.c == 0 && self.caret.p > 0 { self.caret.p - 1 } else { self.caret.p };
                self.set_caret(Pos::new(p, 0), shift);
            }
            Key::ArrowDown if cmd => {
                let p = (self.caret.p + 1).min(self.doc.paras.len() - 1);
                let pos = if p == self.caret.p { Pos::new(p, self.doc.paras[p].len()) } else { Pos::new(p, 0) };
                self.set_caret(pos, shift);
            }
            Key::ArrowUp => self.move_vert(lay, -1, shift),
            Key::ArrowDown => self.move_vert(lay, 1, shift),
            Key::PageUp => self.move_vert(lay, -lpv, shift),
            Key::PageDown => self.move_vert(lay, lpv, shift),
            Key::Home => {
                let pos = if cmd {
                    Pos::new(0, 0)
                } else {
                    let li = lay.line_of(self.caret.p, self.caret.c);
                    Pos::new(self.caret.p, lay.lines[li].start)
                };
                self.set_caret(pos, shift);
            }
            Key::End => {
                let pos = if cmd {
                    self.doc.end()
                } else {
                    let li = lay.line_of(self.caret.p, self.caret.c);
                    let l = &lay.lines[li];
                    let c = if l.last_in_para { l.end } else { l.end.saturating_sub(1).max(l.start) };
                    Pos::new(self.caret.p, c)
                };
                self.set_caret(pos, shift);
            }
            Key::Backspace if !ro => self.backspace(cmd, now),
            Key::Delete if !ro => self.delete_forward(cmd, now),
            Key::Enter if cmd && !ro => self.page_break(now),
            Key::Enter if !ro => self.enter(now),
            Key::Tab if cmd => cmds.push(Cmd::NextTab),
            Key::Tab if !ro => {
                let pfmt = self.doc.paras[self.caret.p].fmt;
                if pfmt.table_id > 0 {
                    self.tab_in_table(shift, now);
                } else if pfmt.list != ListKind::None || (self.has_selection() && self.anchor.p != self.caret.p) {
                    self.apply_para(now, &move |f| {
                        if shift {
                            f.indent = f.indent.saturating_sub(1);
                        } else {
                            f.indent = (f.indent + 1).min(8);
                        }
                    });
                } else if !shift {
                    self.insert("\t", now);
                }
            }
            Key::A if cmd && !shift && !alt => self.select_all(),
            Key::Z if cmd && shift && !ro => self.redo(),
            Key::Z if cmd && !ro => self.undo(),
            Key::Y if cmd && shift && shortcuts == ShortcutPreset::GoogleDocs => cmds.push(Cmd::Dictionary),
            Key::Y if cmd && !shift && !alt && !ro => self.redo(),
            Key::V if cmd && alt && !ro && shortcuts == ShortcutPreset::MsWord => cmds.push(Cmd::PastePlain),
            // Bulleted list in WordPro (Ctrl+Shift+B) vs Bold (Ctrl+B)
            Key::B if cmd && shift && !ro && shortcuts == ShortcutPreset::WordPro => cmds.push(Cmd::BulletList),
            Key::B if cmd && !shift && !ro => self.toggle(now, |s| s.bold, |s, v| s.bold = v),
            Key::I if cmd && !shift && !ro => self.toggle(now, |s| s.italic, |s, v| s.italic = v),
            Key::U if cmd && !shift && !ro => self.toggle(now, |s| s.underline, |s, v| s.underline = v),
            // Strikethrough: Alt+Shift+5 (Google Docs), Ctrl+Shift+X (MS Word), Ctrl+Shift+K (WordPro)
            Key::Num5 if alt && shift && !ro && shortcuts == ShortcutPreset::GoogleDocs => self.toggle(now, |s| s.strike, |s, v| s.strike = v),
            Key::X if cmd && shift && !ro && shortcuts == ShortcutPreset::MsWord => self.toggle(now, |s| s.strike, |s, v| s.strike = v),
            Key::K if cmd && shift && !ro && shortcuts == ShortcutPreset::WordPro => self.toggle(now, |s| s.strike, |s, v| s.strike = v),
            // Lists:
            Key::Num7 if cmd && shift && !ro && matches!(shortcuts, ShortcutPreset::GoogleDocs | ShortcutPreset::MsWord) => cmds.push(Cmd::NumberList),
            Key::Num8 if cmd && shift && !ro && shortcuts == ShortcutPreset::GoogleDocs => cmds.push(Cmd::BulletList),
            Key::Num9 if cmd && shift && !ro => cmds.push(Cmd::Checklist),
            Key::L if cmd && shift && !ro && shortcuts == ShortcutPreset::MsWord => cmds.push(Cmd::BulletList),
            // Paragraph styles:
            Key::Num0 if cmd && alt && !ro && shortcuts == ShortcutPreset::GoogleDocs => self.apply_named_style(0, now),
            Key::N if cmd && shift && !ro && shortcuts == ShortcutPreset::MsWord => self.apply_named_style(0, now),
            Key::N if cmd && shift && !ro && shortcuts == ShortcutPreset::WordPro => cmds.push(Cmd::NumberList),
            Key::Num0 if alt && !cmd && !shift && !ro && shortcuts == ShortcutPreset::WordPro => self.apply_named_style(0, now),
            Key::Num1 if ((cmd && alt && matches!(shortcuts, ShortcutPreset::GoogleDocs | ShortcutPreset::MsWord)) || (alt && !cmd && shortcuts == ShortcutPreset::WordPro)) && !ro => {
                self.apply_named_style(4, now);
            }
            Key::Num2 if ((cmd && alt && matches!(shortcuts, ShortcutPreset::GoogleDocs | ShortcutPreset::MsWord)) || (alt && !cmd && shortcuts == ShortcutPreset::WordPro)) && !ro => {
                self.apply_named_style(5, now);
            }
            Key::Num3 if ((cmd && alt && matches!(shortcuts, ShortcutPreset::GoogleDocs | ShortcutPreset::MsWord)) || (alt && !cmd && shortcuts == ShortcutPreset::WordPro)) && !ro => {
                self.apply_named_style(6, now);
            }
            // Font size step & Superscript / Subscript:
            Key::Period if cmd && shift && !ro && matches!(shortcuts, ShortcutPreset::GoogleDocs | ShortcutPreset::MsWord) => {
                self.apply_char(now, &|s| s.size = (s.size + 1.0).min(MAX_SIZE));
            }
            Key::Comma if cmd && shift && !ro && matches!(shortcuts, ShortcutPreset::GoogleDocs | ShortcutPreset::MsWord) => {
                self.apply_char(now, &|s| s.size = (s.size - 1.0).max(MIN_SIZE));
            }
            Key::Period if cmd && !shift && !ro && matches!(shortcuts, ShortcutPreset::GoogleDocs | ShortcutPreset::WordPro) => {
                let cur = self.typing_style().script;
                let nk = if cur == 1 { 0 } else { 1 };
                self.apply_char(now, &move |s| s.script = nk);
            }
            Key::Comma if cmd && !shift && !ro && matches!(shortcuts, ShortcutPreset::GoogleDocs | ShortcutPreset::WordPro) => {
                let cur = self.typing_style().script;
                let nk = if cur == -1 { 0 } else { -1 };
                self.apply_char(now, &move |s| s.script = nk);
            }
            Key::Equals | Key::Plus if cmd && shift && !ro && shortcuts == ShortcutPreset::MsWord => {
                let cur = self.typing_style().script;
                let nk = if cur == 1 { 0 } else { 1 };
                self.apply_char(now, &move |s| s.script = nk);
            }
            Key::Equals if cmd && !shift && !ro && shortcuts == ShortcutPreset::MsWord => {
                let cur = self.typing_style().script;
                let nk = if cur == -1 { 0 } else { -1 };
                self.apply_char(now, &move |s| s.script = nk);
            }
            Key::F4 if cmd && shortcuts == ShortcutPreset::WordPro => cmds.push(Cmd::Close),
            Key::F4 if shift && !cmd && !alt && !ro && shortcuts == ShortcutPreset::WordPro => {
                self.apply_char(now, &|s| s.size = (s.size - SIZE_STEP).max(MIN_SIZE));
            }
            Key::F4 if !shift && !cmd && !alt && !ro && shortcuts == ShortcutPreset::WordPro => {
                self.apply_char(now, &|s| s.size = (s.size + SIZE_STEP).min(MAX_SIZE));
            }
            // Alignment:
            Key::L if cmd && ((shift && shortcuts == ShortcutPreset::GoogleDocs) || (!shift && shortcuts != ShortcutPreset::MsWord)) && !ro => {
                self.apply_para(now, &|f| f.align = Align::Left);
            }
            Key::L if cmd && !shift && !ro && shortcuts == ShortcutPreset::MsWord => {
                self.apply_para(now, &|f| f.align = Align::Left);
            }
            Key::E if cmd && !ro => self.apply_para(now, &|f| f.align = Align::Center),
            Key::R if cmd && !ro => self.apply_para(now, &|f| f.align = Align::Right),
            Key::J if cmd && !ro => self.apply_para(now, &|f| f.align = Align::Justify),
            // Clear formatting:
            Key::Backslash if cmd && !ro && shortcuts == ShortcutPreset::GoogleDocs => self.clear_formatting(now),
            Key::Space if cmd && !ro && matches!(shortcuts, ShortcutPreset::MsWord | ShortcutPreset::WordPro) => self.clear_formatting(now),
            // Indent / Outdent:
            Key::CloseBracket if cmd && !ro && matches!(shortcuts, ShortcutPreset::GoogleDocs | ShortcutPreset::WordPro) => {
                self.apply_para(now, &|f| f.indent = (f.indent + 1).min(8));
            }
            Key::OpenBracket if cmd && !ro && matches!(shortcuts, ShortcutPreset::GoogleDocs | ShortcutPreset::WordPro) => {
                self.apply_para(now, &|f| f.indent = f.indent.saturating_sub(1));
            }
            Key::M if cmd && shift && !alt && !ro && shortcuts == ShortcutPreset::MsWord => {
                self.apply_para(now, &|f| f.indent = f.indent.saturating_sub(1));
            }
            Key::M if cmd && !shift && !alt && !ro && shortcuts == ShortcutPreset::MsWord => {
                self.apply_para(now, &|f| f.indent = (f.indent + 1).min(8));
            }
            Key::K if cmd && !shift && !ro => cmds.push(Cmd::Link),
            Key::M if cmd && alt => cmds.push(Cmd::Comment),
            Key::F if cmd && alt && !ro => cmds.push(Cmd::Footnote),
            // Word count: Ctrl+Shift+C (Google Docs), Ctrl+Shift+G (MS Word), Ctrl+Shift+W (WordPro)
            Key::C if cmd && shift && shortcuts == ShortcutPreset::GoogleDocs => cmds.push(Cmd::WordCount),
            Key::G if cmd && shift && shortcuts == ShortcutPreset::MsWord => cmds.push(Cmd::WordCount),
            Key::W if cmd && shift && shortcuts == ShortcutPreset::WordPro => cmds.push(Cmd::WordCount),
            Key::F if cmd && shift => cmds.push(Cmd::CompactMode),
            Key::H if cmd && alt && shift => cmds.push(Cmd::VersionHistory),
            Key::D if cmd && !ro => cmds.push(Cmd::DuplicatePara),
            Key::N if cmd && !shift => cmds.push(Cmd::New),
            Key::O if cmd => cmds.push(Cmd::Open),
            Key::S if cmd && shift && matches!(shortcuts, ShortcutPreset::GoogleDocs | ShortcutPreset::WordPro) => cmds.push(Cmd::SaveAs),
            Key::F12 if !cmd && !shift && !alt && shortcuts == ShortcutPreset::MsWord => cmds.push(Cmd::SaveAs),
            Key::S if cmd && !shift => cmds.push(Cmd::Save),
            Key::W if cmd && !shift && matches!(shortcuts, ShortcutPreset::GoogleDocs | ShortcutPreset::MsWord) => cmds.push(Cmd::Close),
            Key::F if cmd && !shift && !alt => {
                if shortcuts == ShortcutPreset::WordPro {
                    cmds.push(Cmd::Replace);
                } else {
                    cmds.push(Cmd::Find);
                }
            }
            Key::H if cmd && !shift && !alt => cmds.push(Cmd::Replace),
            Key::G if cmd && !shift => cmds.push(Cmd::GoTo),
            Key::P if cmd => cmds.push(Cmd::Print),
            Key::Equals | Key::Plus if cmd && shortcuts != ShortcutPreset::MsWord => cmds.push(Cmd::ZoomIn),
            Key::Minus if cmd => cmds.push(Cmd::ZoomOut),
            Key::Num0 if cmd && !alt => cmds.push(Cmd::Zoom100),
            Key::F2 if cmd && shortcuts == ShortcutPreset::WordPro => cmds.push(Cmd::Spell),
            Key::F2 if alt && !cmd && !shift && shortcuts == ShortcutPreset::WordPro => cmds.push(Cmd::Dictionary),
            Key::F7 if shift && !cmd && !alt && shortcuts == ShortcutPreset::MsWord => cmds.push(Cmd::Dictionary),
            Key::F7 if !shift && !cmd && !alt && matches!(shortcuts, ShortcutPreset::GoogleDocs | ShortcutPreset::MsWord) => cmds.push(Cmd::Spell),
            _ => {}
        }
    }

    // ── drawing ─────────────────────────────────────────────
    /// Renders an opened PDF pixel-accurately via `hayro` (on-demand per visible page),
    /// supporting explicit `Read` (clean final view) and `Edit` (in-place text editing) modes.
    /// Editing controls live cleanly in the right-hand PDF Editor side panel.
    fn show_pdf_exact(&mut self, ui: &mut egui::Ui, opts: &ViewOpts) -> (egui::Response, Option<Rect>) {
        let th = opts.theme;
        let is_edit = self.pdf_view.as_ref().is_some_and(|pv| pv.mode == PdfMode::Edit);

        // Escape cancels add-text-box or deselects active block
        if is_edit && ui.input(|i| i.key_pressed(Key::Escape)) {
            if let Some(pv) = self.pdf_view.as_mut() {
                pv.adding_text_box = false;
                pv.selected_block = None;
                pv.editing_checkpointed = None;
                pv.drag = PdfBlockDrag::None;
            }
        }

        let adding_box = self.pdf_view.as_ref().is_some_and(|pv| pv.adding_text_box);
        let avail_w = ui.available_width();
        let px_per_pt = (layout::BASE_PPI / 72.0) * opts.zoom;
        let ppp = ui.ctx().pixels_per_point();
        let raster_scale = (px_per_pt * ppp).clamp(0.5, 4.0);
        let scale_key = (raster_scale * 100.0).round() as u32;

        let mut total_h = PAGE_GAP;
        let mut max_w = avail_w;
        let mut page_rects: Vec<(usize, Vec2, f32)> = Vec::new();
        if let Some(pv) = &self.pdf_view {
            for (idx, &(w_pt, h_pt)) in pv.page_dims_pt.iter().enumerate() {
                let sz = vec2(w_pt * px_per_pt, h_pt * px_per_pt);
                max_w = max_w.max(sz.x + PAGE_GAP * 2.0);
                page_rects.push((idx, sz, total_h));
                total_h += sz.y + PAGE_GAP;
            }
        }

        let (canvas, resp) = ui.allocate_exact_size(vec2(max_w, total_h.max(200.0)), Sense::click());
        let resp = if adding_box {
            resp.on_hover_cursor(egui::CursorIcon::Crosshair)
        } else {
            resp
        };
        self.canvas_x0 = canvas.min.x;
        self.canvas_w = canvas.width();
        let clip = ui.clip_rect();
        let painter = ui.painter();
        let hover_pos = ui.input(|i| i.pointer.hover_pos());

        let mut page_screen_rects: Vec<(usize, Rect)> = Vec::with_capacity(page_rects.len());
        if let Some(pv) = self.pdf_view.as_mut() {
            // Evict textures from previous zoom levels to keep GPU memory bounded
            pv.textures.retain(|&(_, sk), _| sk == scale_key);
            for (idx, sz, top_y) in page_rects {
                let x = canvas.min.x + ((canvas.width() - sz.x) * 0.5).max(PAGE_GAP);
                let rect = Rect::from_min_size(pos2(x, canvas.min.y + top_y), sz);
                page_screen_rects.push((idx, rect));
                if !rect.intersects(clip.expand(sz.y * 0.5)) {
                    continue;
                }
                // Soft page drop shadow
                painter.rect_filled(rect.translate(vec2(2.0, 3.0)), 2.0, Color32::from_black_alpha(28));
                painter.rect_filled(rect, 0.0, Color32::WHITE);

                let tex = pv.textures.entry((idx, scale_key)).or_insert_with(|| {
                    let (rw, rh, rgba) = crate::pdf::hayro_render_page_rgba(pv.bytes.clone(), idx, raster_scale)
                        .unwrap_or((1, 1, vec![255, 255, 255, 255]));
                    let img = egui::ColorImage::from_rgba_unmultiplied([rw as usize, rh as usize], &rgba);
                    ui.ctx().load_texture(
                        format!("pdf_exact_{}_{}_{}", self.id, idx, scale_key),
                        img,
                        egui::TextureOptions::LINEAR,
                    )
                });
                painter.image(
                    tex.id(),
                    rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_gray(205)), StrokeKind::Outside);
            }
        }

        // Composite background patches + wrapped/aligned/justified edited text on top of the exact PDF pages
        let mut clicked_block: Option<u64> = None;
        let mut active_edit_rect: Option<(u64, Rect)> = None;
        if let Some(pv) = self.pdf_view.as_ref() {
            let sel = pv.selected_block;
            for &(page_idx, page_rect) in &page_screen_rects {
                if !page_rect.intersects(clip.expand(40.0)) {
                    continue;
                }
                for b in pv.blocks.iter().filter(|b| b.page == page_idx) {
                    let bx = page_rect.min.x + b.x_pt * px_per_pt;
                    let by = page_rect.min.y + b.y_pt * px_per_pt;
                    let bw = (b.w_pt * px_per_pt).max(32.0);
                    let bh = (b.h_pt * px_per_pt).max(14.0);
                    let brect = Rect::from_min_size(pos2(bx, by), vec2(bw, bh));

                    // Paint background patch over modified or deleted original PDF text at its original location & width
                    if !b.is_added && (b.modified || b.deleted) {
                        let ox = page_rect.min.x + b.orig_x_pt * px_per_pt;
                        let oy = page_rect.min.y + b.orig_y_pt * px_per_pt;
                        let ow = (b.orig_w_pt * px_per_pt).max(16.0);
                        let orig_rect = Rect::from_min_size(pos2(ox, oy), vec2(ow, bh));
                        let pad_x = 1.5 * px_per_pt;
                        let pad_y = 1.0 * px_per_pt;
                        let patch = Rect::from_min_max(
                            pos2((orig_rect.min.x - pad_x).max(page_rect.min.x), (orig_rect.min.y - pad_y).max(page_rect.min.y)),
                            pos2((orig_rect.max.x + pad_x).min(page_rect.max.x), (orig_rect.max.y + pad_y).min(page_rect.max.y)),
                        );
                        painter.rect_filled(patch, 0.0, Color32::from_rgb(b.bg_color[0], b.bg_color[1], b.bg_color[2]));
                    }

                    if b.deleted {
                        continue;
                    }

                    let (fam, _) = opts.book.family(b.font, b.bold, b.italic);
                    let fid = egui::FontId::new((b.size_pt * px_per_pt).max(6.0), fam);
                    let col = Color32::from_rgb(b.color[0], b.color[1], b.color[2]);

                    // Measure wrapped height so multi-line & justified text boxes expand vertically
                    let line_h_px = (b.size_pt * 1.28 * px_per_pt).max(10.0);
                    let wrapped_paras: Vec<Vec<Vec<String>>> = if b.modified || b.is_added || (is_edit && sel == Some(b.id)) {
                        ui.fonts_mut(|fonts| {
                            let space_w = fonts
                                .layout_no_wrap(" ".to_string(), fid.clone(), col)
                                .rect
                                .width()
                                .max(fid.size * 0.25);
                            b.text
                                .split('\n')
                                .map(|para| {
                                    let words: Vec<&str> = para.split_whitespace().collect();
                                    if words.is_empty() {
                                        return vec![Vec::new()];
                                    }
                                    let mut lines: Vec<Vec<String>> = Vec::new();
                                    let mut cur: Vec<String> = Vec::new();
                                    let mut cur_w = 0.0f32;
                                    for w in words {
                                        let ww = fonts
                                            .layout_no_wrap(w.to_string(), fid.clone(), col)
                                            .rect
                                            .width();
                                        let add = if cur.is_empty() { ww } else { space_w + ww };
                                        if !cur.is_empty() && cur_w + add > bw {
                                            lines.push(std::mem::take(&mut cur));
                                            cur_w = ww;
                                            cur.push(w.to_string());
                                        } else {
                                            cur_w += add;
                                            cur.push(w.to_string());
                                        }
                                    }
                                    if !cur.is_empty() {
                                        lines.push(cur);
                                    }
                                    lines
                                })
                                .collect()
                        })
                    } else {
                        Vec::new()
                    };
                    let total_lines = wrapped_paras.iter().map(|p| p.len()).sum::<usize>().max(1);
                    let rendered_h = (total_lines as f32 * line_h_px).max(bh);
                    let hit_rect = Rect::from_min_size(brect.min, vec2(bw, rendered_h));

                    let is_sel = is_edit && sel == Some(b.id);
                    if is_sel {
                        let edit_w = bw.clamp(40.0 * px_per_pt, (page_rect.max.x - brect.min.x).max(40.0 * px_per_pt));
                        let edit_h = (rendered_h + 6.0).max(bh + 4.0);
                        active_edit_rect = Some((b.id, Rect::from_min_size(brect.min, vec2(edit_w, edit_h))));
                    } else if b.modified || b.is_added {
                        // Layout galleys inside `ui.fonts_mut` first, then paint outside the lock
                        // (calling `painter.text` inside `ui.fonts_mut` re-entrantly locks Fonts and panics).
                        let mut line_y = brect.min.y;
                        let mut paint_galleys: Vec<(Pos2, std::sync::Arc<egui::Galley>)> = Vec::new();
                        ui.fonts_mut(|fonts| {
                            let space_w = fonts
                                .layout_no_wrap(" ".to_string(), fid.clone(), col)
                                .rect
                                .width()
                                .max(fid.size * 0.25);
                            for para_lines in &wrapped_paras {
                                let last_idx = para_lines.len().saturating_sub(1);
                                for (li, words) in para_lines.iter().enumerate() {
                                    if !words.is_empty() {
                                        let is_last = li == last_idx;
                                        if b.align == Align::Justify && !is_last && words.len() >= 2 {
                                            let word_galleys: Vec<std::sync::Arc<egui::Galley>> = words
                                                .iter()
                                                .map(|w| fonts.layout_no_wrap(w.clone(), fid.clone(), col))
                                                .collect();
                                            let total_ww: f32 =
                                                word_galleys.iter().map(|g| g.rect.width()).sum();
                                            let gap = ((bw - total_ww) / (words.len() - 1) as f32)
                                                .max(space_w * 0.5);
                                            let mut cx = brect.min.x;
                                            for g in word_galleys {
                                                let ww = g.rect.width();
                                                paint_galleys.push((pos2(cx, line_y), g));
                                                cx += ww + gap;
                                            }
                                        } else {
                                            let line_str = words.join(" ");
                                            let g = fonts.layout_no_wrap(line_str, fid.clone(), col);
                                            let lw = g.rect.width();
                                            let lx = match b.align {
                                                Align::Left | Align::Justify => brect.min.x,
                                                Align::Center => brect.min.x + ((bw - lw) * 0.5).max(0.0),
                                                Align::Right => brect.min.x + (bw - lw).max(0.0),
                                            };
                                            paint_galleys.push((pos2(lx, line_y), g));
                                        }
                                    }
                                    line_y += line_h_px;
                                }
                            }
                        });
                        for (gpos, galley) in paint_galleys {
                            painter.galley(gpos, galley, col);
                        }
                    }

                    if is_edit && !is_sel {
                        let hov = hover_pos.is_some_and(|hp| hit_rect.expand(2.0).contains(hp));
                        if hov {
                            painter.rect_filled(hit_rect.expand(1.5), 2.0, th.accent.gamma_multiply(0.10));
                            painter.rect_stroke(
                                hit_rect.expand(1.5),
                                2.0,
                                Stroke::new(1.2, th.accent),
                                StrokeKind::Outside,
                            );
                        } else if b.modified || b.is_added {
                            painter.rect_stroke(
                                hit_rect.expand(1.0),
                                2.0,
                                Stroke::new(0.8, th.accent.gamma_multiply(0.35)),
                                StrokeKind::Outside,
                            );
                        }
                        if resp.clicked() && !adding_box {
                            if let Some(pp) = resp.interact_pointer_pos() {
                                if hit_rect.expand(2.5).contains(pp) {
                                    clicked_block = Some(b.id);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Handle clicks on the PDF page in Edit mode (select block, add text box, or deselect)
        let mut just_added_block: Option<u64> = None;
        let is_dragging_block = self.pdf_view.as_ref().is_some_and(|pv| pv.drag.is_active());
        if is_edit && resp.clicked() && !is_dragging_block {
            if let Some(id) = clicked_block {
                if let Some(pv) = self.pdf_view.as_mut() {
                    pv.selected_block = Some(id);
                    pv.editing_checkpointed = None;
                }
            } else if let Some(pp) = resp.interact_pointer_pos() {
                if adding_box {
                    if let Some(&(page_idx, page_rect)) = page_screen_rects.iter().find(|(_, r)| r.contains(pp)) {
                        let x_pt = ((pp.x - page_rect.min.x) / px_per_pt).max(0.0);
                        let y_pt = ((pp.y - page_rect.min.y) / px_per_pt).max(0.0);
                        if let Some(pv) = self.pdf_view.as_mut() {
                            pv.checkpoint();
                            let id = pv.next_block_id;
                            pv.next_block_id += 1;
                            pv.blocks.push(crate::pdf_import::PdfTextBlock {
                                id,
                                page: page_idx,
                                x_pt,
                                y_pt,
                                orig_x_pt: x_pt,
                                orig_y_pt: y_pt,
                                orig_w_pt: 200.0,
                                w_pt: 200.0,
                                h_pt: 16.0,
                                original_text: String::new(),
                                text: String::new(),
                                font: crate::doc::DEFAULT_FONT,
                                size_pt: 12.0,
                                bold: false,
                                italic: false,
                                align: Align::Left,
                                color: [0, 0, 0],
                                bg_color: [255, 255, 255],
                                deleted: false,
                                is_added: true,
                                modified: true,
                            });
                            pv.selected_block = Some(id);
                            pv.adding_text_box = false;
                            pv.editing_checkpointed = Some(id);
                            just_added_block = Some(id);
                            self.rev += 1;
                            self.dirty = true;
                        }
                    }
                } else if let Some(pv) = self.pdf_view.as_mut() {
                    pv.selected_block = None;
                    pv.editing_checkpointed = None;
                }
            }
        }

        // Render live inline TextEdit + floating Move handle + right-edge Width resize handle
        if is_edit && self.pdf_view.as_ref().is_some_and(|pv| pv.selected_block.is_some()) {
            if let Some((sel_id, edit_rect)) = active_edit_rect {
                let mut text_changed = false;
                let mut geom_changed = false;
                if let Some(pv) = self.pdf_view.as_mut() {
                    if let Some(idx) = pv.blocks.iter().position(|b| b.id == sel_id) {
                        let (page_idx, font_idx, size_pt, bold, italic, align, color, bg_color, cur_x_pt, cur_y_pt, cur_w_pt) = {
                            let b = &pv.blocks[idx];
                            (b.page, b.font, b.size_pt, b.bold, b.italic, b.align, b.color, b.bg_color, b.x_pt, b.y_pt, b.w_pt)
                        };
                        let (pw_pt, ph_pt) = pv.page_dims_pt.get(page_idx).copied().unwrap_or((612.0, 792.0));
                        let (fam, _) = opts.book.family(font_idx, bold, italic);
                        let fid = egui::FontId::new((size_pt * px_per_pt).max(8.0), fam);
                        let bg_c = Color32::from_rgb(bg_color[0], bg_color[1], bg_color[2]);
                        let fg_c = Color32::from_rgb(color[0], color[1], color[2]);

                        // 1. Floating "⋮⋮ Move" handle directly above the text box
                        let handle_h = 18.0;
                        let handle_w = 64.0;
                        let handle_y = (edit_rect.min.y - handle_h - 3.0).max(clip.min.y + 2.0);
                        let handle_rect = Rect::from_min_size(pos2(edit_rect.min.x - 1.5, handle_y), vec2(handle_w, handle_h));
                        let handle_id = egui::Id::unique(("pdf_block_move", self.id, sel_id));
                        let handle_resp = ui
                            .interact(handle_rect, handle_id, Sense::click_and_drag())
                            .on_hover_text("Drag to move this text box anywhere on the page")
                            .on_hover_cursor(if matches!(pv.drag, PdfBlockDrag::Move { .. }) {
                                egui::CursorIcon::Grabbing
                            } else {
                                egui::CursorIcon::Grab
                            });

                        // 2. Right-edge Width resize handle
                        let resize_h = edit_rect.height().clamp(18.0, 34.0);
                        let resize_rect = Rect::from_center_size(
                            pos2(edit_rect.max.x + 3.0, edit_rect.center().y),
                            vec2(8.0, resize_h),
                        );
                        let resize_id = egui::Id::unique(("pdf_block_resize_w", self.id, sel_id));
                        let resize_resp = ui
                            .interact(resize_rect, resize_id, Sense::click_and_drag())
                            .on_hover_text("Drag left or right to resize text box width")
                            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);

                        if handle_resp.drag_started() {
                            if let Some(ptr) = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())) {
                                pv.checkpoint();
                                pv.drag = PdfBlockDrag::Move {
                                    id: sel_id,
                                    start_ptr: ptr,
                                    start_x_pt: cur_x_pt,
                                    start_y_pt: cur_y_pt,
                                };
                            }
                        } else if resize_resp.drag_started() {
                            if let Some(ptr) = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())) {
                                pv.checkpoint();
                                pv.drag = PdfBlockDrag::ResizeWidth {
                                    id: sel_id,
                                    start_ptr_x: ptr.x,
                                    start_w_pt: cur_w_pt,
                                };
                            }
                        }

                        match pv.drag {
                            PdfBlockDrag::Move {
                                id,
                                start_ptr,
                                start_x_pt,
                                start_y_pt,
                            } if id == sel_id => {
                                if ui.input(|i| i.pointer.primary_down()) {
                                    if let Some(cur_ptr) = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())) {
                                        let dx_pt = (cur_ptr.x - start_ptr.x) / px_per_pt.max(0.1);
                                        let dy_pt = (cur_ptr.y - start_ptr.y) / px_per_pt.max(0.1);
                                        let nx = (start_x_pt + dx_pt).clamp(0.0, (pw_pt - 20.0).max(0.0));
                                        let ny = (start_y_pt + dy_pt).clamp(0.0, (ph_pt - 14.0).max(0.0));
                                        let b = &mut pv.blocks[idx];
                                        if (b.x_pt - nx).abs() > 0.05 || (b.y_pt - ny).abs() > 0.05 {
                                            b.x_pt = nx;
                                            b.y_pt = ny;
                                            b.modified = true;
                                            geom_changed = true;
                                        }
                                    }
                                } else {
                                    pv.drag = PdfBlockDrag::None;
                                }
                            }
                            PdfBlockDrag::ResizeWidth {
                                id,
                                start_ptr_x,
                                start_w_pt,
                            } if id == sel_id => {
                                if ui.input(|i| i.pointer.primary_down()) {
                                    if let Some(cur_ptr) = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())) {
                                        let dx_pt = (cur_ptr.x - start_ptr_x) / px_per_pt.max(0.1);
                                        let max_w = (pw_pt - cur_x_pt).max(40.0);
                                        let nw = (start_w_pt + dx_pt).clamp(40.0, max_w);
                                        let b = &mut pv.blocks[idx];
                                        if (b.w_pt - nw).abs() > 0.05 {
                                            b.w_pt = nw;
                                            b.modified = true;
                                            geom_changed = true;
                                        }
                                    }
                                } else {
                                    pv.drag = PdfBlockDrag::None;
                                }
                            }
                            _ => {}
                        }

                        ui.painter().rect_filled(edit_rect.expand(1.5), 2.0, bg_c);
                        ui.painter().rect_stroke(
                            edit_rect.expand(1.5),
                            2.0,
                            Stroke::new(1.5, th.accent),
                            StrokeKind::Outside,
                        );
                        // Paint Move pill
                        let handle_bg = if handle_resp.hovered() || matches!(pv.drag, PdfBlockDrag::Move { .. }) {
                            th.accent
                        } else {
                            th.accent.gamma_multiply(0.88)
                        };
                        ui.painter().rect_filled(handle_rect, 4.0, handle_bg);
                        ui.painter().text(
                            handle_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "⋮⋮ Move",
                            egui::FontId::proportional(10.5),
                            Color32::WHITE,
                        );
                        // Paint Right-Edge Width resize handle
                        let r_bg = if resize_resp.hovered() || matches!(pv.drag, PdfBlockDrag::ResizeWidth { .. }) {
                            th.accent
                        } else {
                            Color32::WHITE
                        };
                        ui.painter().rect_filled(resize_rect, 3.0, r_bg);
                        ui.painter().rect_stroke(resize_rect, 3.0, Stroke::new(1.5, th.accent), StrokeKind::Inside);

                        // Live geometry badge while moving or resizing
                        if pv.drag.block_id() == Some(sel_id) {
                            let b = &pv.blocks[idx];
                            let badge_txt = match pv.drag {
                                PdfBlockDrag::Move { .. } => format!("X:{:.0}  Y:{:.0} pt", b.x_pt, b.y_pt),
                                PdfBlockDrag::ResizeWidth { .. } => format!("W:{:.0} pt", b.w_pt),
                                PdfBlockDrag::None => String::new(),
                            };
                            let coord_rect = Rect::from_min_size(
                                pos2(handle_rect.max.x + 6.0, handle_rect.min.y),
                                vec2(96.0, handle_h),
                            );
                            ui.painter().rect_filled(coord_rect, 4.0, Color32::from_black_alpha(185));
                            ui.painter().text(
                                coord_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                badge_txt,
                                egui::FontId::monospace(10.0),
                                Color32::WHITE,
                            );
                        }

                        let h_align = match align {
                            Align::Center => egui::Align::Center,
                            Align::Right => egui::Align::Max,
                            Align::Left | Align::Justify => egui::Align::Min,
                        };
                        let prev_text = pv.blocks[idx].text.clone();
                        let te_id = egui::Id::unique(("pdf_inline_edit", self.id, sel_id));
                        let te_resp = ui.put(
                            edit_rect,
                            egui::TextEdit::multiline(&mut pv.blocks[idx].text)
                                .id(te_id)
                                .font(fid)
                                .text_color(fg_c)
                                .background_color(bg_c)
                                .align(egui::Align2::new(h_align, egui::Align::TOP))
                                .frame(egui::Frame::NONE)
                                .desired_rows(1),
                        );
                        if !te_resp.has_focus() && (clicked_block == Some(sel_id) || just_added_block == Some(sel_id)) {
                            te_resp.request_focus();
                        }
                        if te_resp.changed() {
                            if pv.editing_checkpointed != Some(sel_id) {
                                let mut snap = pv.blocks.clone();
                                snap[idx].text = prev_text;
                                pv.undo.push(snap);
                                if pv.undo.len() > 64 {
                                    pv.undo.remove(0);
                                }
                                pv.redo.clear();
                                pv.editing_checkpointed = Some(sel_id);
                            }
                            let b = &mut pv.blocks[idx];
                            b.modified = b.is_added || b.text != b.original_text;
                            text_changed = true;
                        }
                    }
                }
                if text_changed || geom_changed {
                    self.rev += 1;
                    self.dirty = true;
                }
            }
        }

        (resp, None)
    }

    /// Draws the pages and handles mouse input. Returns the caret's screen rect.
    pub fn show(&mut self, ui: &mut egui::Ui, opts: &ViewOpts, focus_id: egui::Id, cmds: &mut Vec<Cmd>) -> (egui::Response, Option<Rect>) {
        if self.pdf_view.as_ref().is_some_and(|pv| pv.is_exact()) {
            return self.show_pdf_exact(ui, opts);
        }
        if let Some(pv) = self.pdf_view.as_mut() {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                if ui.small_button("Back to Exact PDF (Read)").clicked() {
                    pv.mode = PdfMode::Read;
                }
                if ui.small_button("Edit on Exact PDF").clicked() {
                    pv.mode = PdfMode::Edit;
                }
                ui.label(egui::RichText::new("Reflowable document mode").weak().small());
            });
        }
        let lay = self.layout(ui.ctx(), opts);
        let avail_w = ui.available_width();
        let size = lay.canvas_size(avail_w);
        let (canvas, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        self.canvas_x0 = canvas.min.x;
        self.canvas_w = canvas.width();
        let th = opts.theme;
        let now = ui.input(|i| i.time);

        // Pre-compute interactive checklist checkbox rects & embedded image rects
        let mut checkbox_rects: Vec<(usize, Rect)> = Vec::new();
        let mut image_rects: Vec<(usize, u64, Rect, Rect)> = Vec::new();
        for l in &lay.lines {
            if l.first_in_para {
                let pfmt = self.doc.paras[l.para].fmt;
                if pfmt.list == ListKind::Checklist {
                    let o = lay.line_origin(l, canvas.min, canvas.width());
                    let content_x = o.x - l.x;
                    let gh = l.galley.rect.height();
                    let box_s = (13.0 * opts.zoom).clamp(10.0, 22.0);
                    let bx = content_x + l.marker_x;
                    let by = o.y + (gh - box_s) * 0.5;
                    checkbox_rects.push((l.para, Rect::from_min_size(pos2(bx, by), vec2(box_s, box_s)).expand(3.0)));
                }
                if pfmt.image_id > 0 {
                    let o = lay.line_origin(l, canvas.min, canvas.width());
                    let indent = self.doc.block_left_in(&pfmt).max(0.0) * lay.ppi;
                    let avail =
                        (lay.content_size.x - indent - pfmt.right_in * lay.ppi).max(lay.ppi * 0.8);
                    if let Some((img_w, img_h, _cap_h, total_h)) =
                        layout::image_box(&self.doc, pfmt.image_id, avail, lay.ppi)
                    {
                        let pad_top = 4.0 * (lay.ppi / 72.0);
                        let img_r = Rect::from_min_size(pos2(o.x, o.y + pad_top), vec2(img_w, img_h));
                        let full_r = Rect::from_min_size(pos2(o.x, o.y), vec2(img_w, total_h));
                        image_rects.push((l.para, pfmt.image_id, img_r, full_r));
                    }
                }
            }
        }
        self.selected_image_rect = self.selected_image.and_then(|sel_id| {
            image_rects
                .iter()
                .find(|(_, id, _, _)| *id == sel_id)
                .map(|&(pi, id, _, full_r)| (id, pi, full_r))
        });

        let caret_clamped = self.doc.clamp(self.caret);
        let caret_fmt = self.doc.paras[caret_clamped.p].fmt;
        self.active_table_cell_rect = if caret_fmt.table_id > 0 {
            let tid = caret_fmt.table_id;
            let crow = caret_fmt.table_row;
            let ccol = caret_fmt.table_col;
            lay.cells
                .iter()
                .find(|c| c.table_id == tid && c.row == crow && c.col == ccol)
                .map(|active_c| {
                    let page_origin = canvas.min + lay.page_origin(active_c.page, canvas.width()).to_vec2() + lay.content_offset;
                    let cell_r = active_c.rect.translate(page_origin.to_vec2());
                    let mut tbl_r = cell_r;
                    for c in lay.cells.iter().filter(|x| x.table_id == tid && x.page == active_c.page) {
                        tbl_r = tbl_r.union(c.rect.translate(page_origin.to_vec2()));
                    }
                    (tid, crow, ccol, cell_r, tbl_r)
                })
        } else {
            None
        };

        let hover_pos = ui.input(|i| i.pointer.hover_pos());
        let mut hover_handle: Option<(u64, u8, Pos2)> = None;
        if opts.mode != DocMode::Viewing {
            if let Some(hp) = hover_pos {
                for &(_, img_id, img_r, _) in &image_rects {
                    if self.selected_image == Some(img_id) || img_r.expand(10.0).contains(hp) {
                        for (hi, hpt) in image_handle_points(img_r).into_iter().enumerate() {
                            if Rect::from_center_size(hpt, vec2(12.0, 12.0)).contains(hp) {
                                hover_handle = Some((img_id, hi as u8, hp));
                                break;
                            }
                        }
                    }
                    if hover_handle.is_some() {
                        break;
                    }
                }
            }
        }

        // Pre-compute screen-space table cell rects, border resize edges, and row/column header handles
        let mut cell_screen_rects: Vec<(u32, u16, u16, Rect)> = Vec::with_capacity(lay.cells.len());
        for c in &lay.cells {
            let page_origin = canvas.min + lay.page_origin(c.page, canvas.width()).to_vec2() + lay.content_offset;
            let cr = c.rect.translate(page_origin.to_vec2());
            cell_screen_rects.push((c.table_id, c.row, c.col, cr));
        }

        // Row & Column selector header pills for the active table
        let mut col_header_rects: Vec<(u32, u16, Rect, String)> = Vec::new();
        let mut row_header_rects: Vec<(u32, u16, Rect, String)> = Vec::new();
        if let Some((active_tid, _, _, _, tbl_r)) = self.active_table_cell_rect {
            // Column headers along the top of the table (from row 0 cells, or first visible row on this page)
            let min_row = cell_screen_rects
                .iter()
                .filter(|(t, _, _, _)| *t == active_tid)
                .map(|(_, r, _, _)| *r)
                .min()
                .unwrap_or(0);
            for &(t, r, c, cr) in &cell_screen_rects {
                if t == active_tid && r == min_row {
                    let hr = Rect::from_min_max(pos2(cr.min.x + 0.5, tbl_r.min.y - 16.0), pos2(cr.max.x - 0.5, tbl_r.min.y - 2.0));
                    let mut col_idx = c as usize;
                    let mut lbl = String::new();
                    loop {
                        lbl.insert(0, (b'A' + (col_idx % 26) as u8) as char);
                        if col_idx < 26 {
                            break;
                        }
                        col_idx = col_idx / 26 - 1;
                    }
                    col_header_rects.push((t, c, hr, lbl));
                }
            }
            // Row headers along the left of the table (from col 0 cells)
            for &(t, r, c, cr) in &cell_screen_rects {
                if t == active_tid && c == 0 {
                    let hr = Rect::from_min_max(pos2(tbl_r.min.x - 19.0, cr.min.y + 0.5), pos2(tbl_r.min.x - 2.0, cr.max.y - 0.5));
                    row_header_rects.push((t, r, hr, format!("{}", r + 1)));
                }
            }
        }

        // Detect hover over table column border (vertical edge) or row border (horizontal edge), or row/column header
        let mut hover_col_edge: Option<(u32, usize, usize, f32, f32, f32)> = None; // (tid, row, col, x_screen, y0, y1)
        let mut hover_row_edge: Option<(u32, usize, f32, f32, f32)> = None; // (tid, row, y_screen, x0, x1)
        let mut hover_col_hdr: Option<(u32, usize)> = None;
        let mut hover_row_hdr: Option<(u32, usize)> = None;
        if opts.mode != DocMode::Viewing && hover_handle.is_none() && !self.drag.is_resizing_image() {
            if let Some(hp) = hover_pos {
                for &(t, c, hr, _) in &col_header_rects {
                    if hr.expand(1.5).contains(hp) {
                        hover_col_hdr = Some((t, c as usize));
                        break;
                    }
                }
                if hover_col_hdr.is_none() {
                    for &(t, r, hr, _) in &row_header_rects {
                        if hr.expand(1.5).contains(hp) {
                            hover_row_hdr = Some((t, r as usize));
                            break;
                        }
                    }
                }
                if hover_col_hdr.is_none() && hover_row_hdr.is_none() {
                    let tol = 3.5_f32;
                    for &(t, r, c, cr) in &cell_screen_rects {
                        if hp.y >= cr.min.y - 2.0 && hp.y <= cr.max.y + 2.0 && (hp.x - cr.max.x).abs() <= tol {
                            let mut y0 = cr.min.y;
                            let mut y1 = cr.max.y;
                            for &(t2, _, _, cr2) in &cell_screen_rects {
                                if t2 == t && (cr2.max.x - cr.max.x).abs() < 2.0 {
                                    y0 = y0.min(cr2.min.y);
                                    y1 = y1.max(cr2.max.y);
                                }
                            }
                            hover_col_edge = Some((t, r as usize, c as usize, cr.max.x, y0, y1));
                            break;
                        }
                        if hp.x >= cr.min.x - 2.0 && hp.x <= cr.max.x + 2.0 && (hp.y - cr.max.y).abs() <= tol {
                            let mut x0 = cr.min.x;
                            let mut x1 = cr.max.x;
                            for &(t2, r2, _, cr2) in &cell_screen_rects {
                                if t2 == t && r2 == r {
                                    x0 = x0.min(cr2.min.x);
                                    x1 = x1.max(cr2.max.x);
                                }
                            }
                            hover_row_edge = Some((t, r as usize, cr.max.y, x0, x1));
                            break;
                        }
                    }
                }
            }
        }

        // Double-click on a cell's right border -> auto-size column width to contents!
        // Double-click on a cell's bottom border -> auto-size row height to contents!
        let mut handled_table_edge = false;
        if opts.mode != DocMode::Viewing && resp.double_clicked() {
            if let Some((tid, r_idx, c_idx, ..)) = hover_col_edge {
                if let Some(auto_w) = layout::measure_column_autofit_width_in(ui.ctx(), &self.doc, opts.book, tid, r_idx, c_idx) {
                    self.checkpoint(EditKind::Other, now);
                    self.doc.table_resize_col(tid, r_idx, c_idx, auto_w);
                    self.drag = CanvasDrag::Idle;
                    self.changed();
                    self.scroll_to_caret = false;
                    handled_table_edge = true;
                }
            } else if let Some((tid, r_idx, ..)) = hover_row_edge {
                if let Some(auto_h_pt) = layout::measure_row_autofit_height_pt(ui.ctx(), &self.doc, opts.book, tid, r_idx) {
                    self.checkpoint(EditKind::Other, now);
                    self.doc.table_resize_row(tid, r_idx, auto_h_pt);
                    self.drag = CanvasDrag::Idle;
                    self.changed();
                    self.scroll_to_caret = false;
                    handled_table_edge = true;
                }
            }
        }

        // Start or continue dragging a table column border or row border
        if !handled_table_edge
            && self.drag.is_idle()
            && opts.mode != DocMode::Viewing
            && (ui.input(|i| i.pointer.primary_pressed()) || resp.drag_started())
        {
            if let Some(hp) = hover_pos {
                if let Some((tid, r_idx, c_idx, ..)) = hover_col_edge {
                    let start_w = self
                        .doc
                        .table(tid)
                        .and_then(|t| t.rows.get(r_idx))
                        .and_then(|r| r.cells.get(c_idx))
                        .map_or(1.5, |c| c.width_in);
                    self.checkpoint(EditKind::Other, now);
                    self.drag = CanvasDrag::ResizingTableCol {
                        table_id: tid,
                        row: r_idx,
                        col: c_idx,
                        start_x: hp.x,
                        start_w_in: start_w,
                    };
                    handled_table_edge = true;
                } else if let Some((tid, r_idx, ..)) = hover_row_edge {
                    let pt = lay.ppi / 72.0;
                    let start_h_pt = cell_screen_rects
                        .iter()
                        .find(|(t, r, _, _)| *t == tid && *r as usize == r_idx)
                        .map_or(24.0, |(_, _, _, cr)| cr.height() / pt.max(0.1));
                    self.checkpoint(EditKind::Other, now);
                    self.drag = CanvasDrag::ResizingTableRow {
                        table_id: tid,
                        row: r_idx,
                        start_y: hp.y,
                        start_h_pt,
                    };
                    handled_table_edge = true;
                }
            }
        }
        match self.drag {
            CanvasDrag::ResizingTableCol {
                table_id: tid,
                row: r_idx,
                col: c_idx,
                start_x,
                start_w_in: start_w,
            } => {
                if ui.input(|i| i.pointer.primary_down()) {
                    if let Some(cur_pt) = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())) {
                        let dx_in = (cur_pt.x - start_x) / lay.ppi.max(1.0);
                        let max_w_in = (self.doc.page.width - self.doc.page.margin_left - self.doc.page.margin_right).max(1.0);
                        let new_w = (start_w + dx_in).clamp(0.4, max_w_in);
                        self.doc.table_resize_col(tid, r_idx, c_idx, new_w);
                        self.rev += 1;
                        self.dirty = true;
                    }
                    handled_table_edge = true;
                } else {
                    self.drag = CanvasDrag::Idle;
                }
            }
            CanvasDrag::ResizingTableRow {
                table_id: tid,
                row: r_idx,
                start_y,
                start_h_pt,
            } => {
                if ui.input(|i| i.pointer.primary_down()) {
                    if let Some(cur_pt) = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())) {
                        let pt = (lay.ppi / 72.0).max(0.1);
                        let dy_pt = (cur_pt.y - start_y) / pt;
                        let new_h_pt = (start_h_pt + dy_pt).clamp(12.0, 600.0);
                        self.doc.table_resize_row(tid, r_idx, new_h_pt);
                        self.rev += 1;
                        self.dirty = true;
                    }
                    handled_table_edge = true;
                } else {
                    self.drag = CanvasDrag::Idle;
                }
            }
            _ => {}
        }

        // Clicking a column header pill or row header pill selects the entire column or row!
        let mut clicked_table_hdr = false;
        if !handled_table_edge && !self.drag.is_resizing_image() && (resp.clicked() || ui.input(|i| i.pointer.primary_pressed())) {
            if let Some((tid, col_idx)) = hover_col_hdr {
                self.select_table_col(tid, col_idx);
                clicked_table_hdr = true;
            } else if let Some((tid, row_idx)) = hover_row_hdr {
                self.select_table_row(tid, row_idx);
                clicked_table_hdr = true;
            }
        }

        // Start or continue dragging an image resize handle
        if self.drag.is_idle()
            && opts.mode != DocMode::Viewing
            && (ui.input(|i| i.pointer.primary_pressed()) || resp.drag_started())
        {
            if let Some((img_id, hi, start_pt)) = hover_handle {
                if let Some(img) = self.doc.image(img_id) {
                    let (w0, h0) = (img.width_in, img.height_in);
                    self.checkpoint(EditKind::Other, now);
                    self.selected_image = Some(img_id);
                    self.drag = CanvasDrag::ResizingImage {
                        id: img_id,
                        handle: hi,
                        start_pos: start_pt,
                        w0_in: w0,
                        h0_in: h0,
                    };
                }
            }
        }
        if let CanvasDrag::ResizingImage {
            id: img_id,
            handle: hi,
            start_pos: start_pt,
            w0_in: w0,
            h0_in: h0,
        } = self.drag
        {
            if ui.input(|i| i.pointer.primary_down()) {
                if let Some(cur_pt) = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())) {
                    let dx_in = (cur_pt.x - start_pt.x) / lay.ppi.max(1.0);
                    let dy_in = (cur_pt.y - start_pt.y) / lay.ppi.max(1.0);
                    let max_w_in = (self.doc.page.width - self.doc.page.margin_left - self.doc.page.margin_right).max(1.0);
                    let aspect = (w0 / h0.max(0.05)).max(0.05);
                    let (new_w, new_h) = match hi {
                        0 => {
                            let nw = (w0 - dx_in).clamp(0.5, max_w_in);
                            (nw, (nw / aspect).clamp(0.4, 20.0))
                        }
                        1 => {
                            let nw = (w0 + dx_in).clamp(0.5, max_w_in);
                            (nw, (nw / aspect).clamp(0.4, 20.0))
                        }
                        2 => {
                            let nw = (w0 + dx_in).clamp(0.5, max_w_in);
                            (nw, (nw / aspect).clamp(0.4, 20.0))
                        }
                        3 => {
                            let nw = (w0 - dx_in).clamp(0.5, max_w_in);
                            (nw, (nw / aspect).clamp(0.4, 20.0))
                        }
                        4 => (w0, (h0 - dy_in).clamp(0.4, 20.0)),
                        5 => (w0, (h0 + dy_in).clamp(0.4, 20.0)),
                        6 => ((w0 - dx_in).clamp(0.5, max_w_in), h0),
                        _ => ((w0 + dx_in).clamp(0.5, max_w_in), h0),
                    };
                    if let Some(img) = self.doc.image_mut(img_id) {
                        if (img.width_in - new_w).abs() > 0.005 || (img.height_in - new_h).abs() > 0.005 {
                            img.width_in = new_w;
                            img.height_in = new_h;
                            self.rev += 1;
                            self.dirty = true;
                        }
                    }
                }
            } else {
                self.drag = CanvasDrag::Idle;
            }
        }

        let over_checkbox = hover_pos.is_some_and(|hp| checkbox_rects.iter().any(|(_, r)| r.contains(hp)));
        let over_image = hover_pos.is_some_and(|hp| image_rects.iter().any(|(_, _, _, fr)| fr.contains(hp)));
        let cursor_icon = match self.drag {
            CanvasDrag::ResizingImage { handle, .. } => handle_cursor(handle),
            CanvasDrag::ResizingTableCol { .. } => egui::CursorIcon::ResizeHorizontal,
            CanvasDrag::ResizingTableRow { .. } => egui::CursorIcon::ResizeVertical,
            CanvasDrag::Idle | CanvasDrag::SelectingCells { .. } => {
                if let Some((_, hi, _)) = hover_handle {
                    handle_cursor(hi)
                } else if hover_col_edge.is_some() {
                    egui::CursorIcon::ResizeHorizontal
                } else if hover_row_edge.is_some() {
                    egui::CursorIcon::ResizeVertical
                } else if hover_col_hdr.is_some() || hover_row_hdr.is_some() || over_checkbox || over_image {
                    egui::CursorIcon::PointingHand
                } else {
                    egui::CursorIcon::Text
                }
            }
        };
        let resp = resp.on_hover_cursor(cursor_icon);

        // mouse
        if resp.clicked() || resp.drag_started() || resp.double_clicked() || resp.triple_clicked() {
            ui.memory_mut(|m| m.request_focus(focus_id));
        }
        let mut toggled_checkbox = false;
        if resp.clicked() && opts.mode != DocMode::Viewing && !self.drag.is_resizing_image() {
            if let Some(pp) = resp.interact_pointer_pos() {
                if let Some((pi, _)) = checkbox_rects.iter().find(|(_, r)| r.contains(pp)) {
                    self.checkpoint(EditKind::Other, now);
                    self.doc.paras[*pi].fmt.checked = !self.doc.paras[*pi].fmt.checked;
                    self.changed();
                    self.scroll_to_caret = false;
                    toggled_checkbox = true;
                }
            }
        }
        let mut clicked_image = false;
        if !toggled_checkbox && !self.drag.is_resizing_image() && hover_handle.is_none() {
            if let Some(pp) = resp.interact_pointer_pos() {
                if let Some(&(pi, img_id, _, _)) = image_rects.iter().find(|(_, _, _, fr)| fr.expand(4.0).contains(pp)) {
                    if resp.clicked() || resp.double_clicked() || ui.input(|i| i.pointer.primary_pressed()) {
                        self.selected_image = Some(img_id);
                        self.caret = Pos::new(pi, 0);
                        self.anchor = self.caret;
                        self.scroll_to_caret = false;
                        if resp.double_clicked() {
                            cmds.push(Cmd::ImageProps(img_id));
                        }
                        clicked_image = true;
                    }
                } else if ui.input(|i| i.pointer.primary_pressed()) || resp.clicked() {
                    self.selected_image = None;
                }
            }
        }
        let mut clicked_hf_margin = false;
        if !toggled_checkbox && !clicked_image && !handled_table_edge && !lay.draft && opts.mode != DocMode::Viewing && resp.double_clicked() {
            if let Some(pp) = resp.interact_pointer_pos() {
                for k in 0..lay.pages {
                    let r = Rect::from_min_size(canvas.min + lay.page_origin(k, canvas.width()).to_vec2(), lay.page_size);
                    if r.contains(pp) {
                        let content_top = r.min.y + lay.content_offset.y;
                        let content_bot = content_top + lay.content_size.y;
                        if pp.y < content_top - 4.0 || pp.y > content_bot + 4.0 {
                            cmds.push(Cmd::PageSetup);
                            clicked_hf_margin = true;
                        }
                        break;
                    }
                }
            }
        }
        if !toggled_checkbox
            && !clicked_image
            && !clicked_hf_margin
            && !handled_table_edge
            && !clicked_table_hdr
            && matches!(self.drag, CanvasDrag::Idle | CanvasDrag::SelectingCells { .. })
        {
            if let Some(pp) = resp.interact_pointer_pos() {
                ui.memory_mut(|m| m.request_focus(focus_id));
                self.last_edit_time = now;
                let pos = self.hit(&lay, canvas, pp);
                let shift = ui.input(|i| i.modifiers.shift);
                let hit_cell_coord = cell_screen_rects
                    .iter()
                    .find(|(_, _, _, cr)| cr.contains(pp))
                    .map(|&(t, r, c, _)| (t, r, c));
                if resp.triple_clicked() {
                    self.anchor = Pos::new(pos.p, 0);
                    self.caret = Pos::new(pos.p, self.doc.paras[pos.p].len());
                    self.selected_cells.clear();
                } else if resp.double_clicked() {
                    let (a, b) = self.doc.word_at(pos);
                    self.anchor = a;
                    self.caret = b;
                    self.selected_cells.clear();
                } else if ui.input(|i| i.pointer.primary_pressed()) {
                    self.set_caret(pos, shift);
                    self.drag = match hit_cell_coord {
                        Some((table_id, anchor_row, anchor_col)) => CanvasDrag::SelectingCells {
                            table_id,
                            anchor_row,
                            anchor_col,
                        },
                        None => CanvasDrag::Idle,
                    };
                } else if resp.dragged() {
                    if let (
                        CanvasDrag::SelectingCells {
                            table_id: t0,
                            anchor_row: r0,
                            anchor_col: c0,
                        },
                        Some((t1, r1, c1)),
                    ) = (self.drag, hit_cell_coord)
                    {
                        if t0 == t1 && (r0 != r1 || c0 != c1) {
                            let mut sel = Vec::new();
                            for r in r0.min(r1)..=r0.max(r1) {
                                for c in c0.min(c1)..=c0.max(c1) {
                                    sel.push((t0, r, c));
                                }
                            }
                            self.selected_cells = sel;
                            self.caret = pos;
                            self.anchor = pos;
                            self.pending = None;
                        } else {
                            self.selected_cells.clear();
                            self.caret = pos;
                            self.pending = None;
                        }
                    } else {
                        self.selected_cells.clear();
                        self.caret = pos;
                        self.pending = None;
                    }
                }
                self.desired_x = None;
                self.scroll_to_caret = false;
            }
        }
        if !ui.input(|i| i.pointer.primary_down()) && matches!(self.drag, CanvasDrag::SelectingCells { .. }) {
            self.drag = CanvasDrag::Idle;
        }

        // right click: move the caret there unless it lands inside the selection
        if resp.secondary_clicked() {
            if let Some(pp) = ui.input(|i| i.pointer.interact_pos()) {
                let pos = self.hit(&lay, canvas, pp);
                let (a, b) = self.selection();
                if !(self.has_selection() && a <= pos && pos <= b) {
                    self.set_caret(pos, false);
                    self.scroll_to_caret = false;
                }
                self.ctx_pos = Some(pos);
                ui.memory_mut(|m| m.request_focus(focus_id));
            }
        }
        self.context_menu(&resp, opts, cmds, now);

        let painter = ui.painter_at(ui.clip_rect());
        let clip = ui.clip_rect();
        let mut spell_cache: std::collections::HashMap<usize, Vec<(usize, usize)>> = Default::default();
        let page_bg = layout::color(self.doc.page.background);
        let page_bg = if th.dark && self.doc.page.background == [255, 255, 255] { Color32::from_rgb(250, 250, 248) } else { page_bg };

        // pages
        let mut page_rects = Vec::with_capacity(lay.pages);
        for k in 0..lay.pages {
            let o = canvas.min + lay.page_origin(k, canvas.width()).to_vec2();
            let r = Rect::from_min_size(o, lay.page_size);
            page_rects.push(r);
            if !r.expand(12.0).intersects(clip) {
                continue;
            }
            if !self.doc.pageless {
                // soft shadow
                for (spread, alpha) in [(6.0, 10u8), (3.0, 18), (1.5, 28)] {
                    painter.rect_filled(r.translate(vec2(0.0, spread * 0.6)).expand(spread * 0.5), 3.0, Color32::from_black_alpha(alpha));
                }
            }
            painter.rect_filled(r, if self.doc.pageless { 8.0 } else { 2.0 }, page_bg);
            painter.rect_stroke(r, if self.doc.pageless { 8.0 } else { 2.0 }, Stroke::new(1.0, th.page_border), StrokeKind::Inside);
            let content = Rect::from_min_size(r.min + lay.content_offset, vec2(lay.content_size.x, if lay.draft { r.height() - lay.content_offset.y * 2.0 } else { lay.content_size.y }));
            if opts.margin_guides && !self.doc.pageless {
                let g = Stroke::new(1.0, th.guide);
                painter.line_segment([pos2(content.min.x, r.min.y + 12.0), pos2(content.min.x, r.max.y - 12.0)], g);
                painter.line_segment([pos2(content.max.x, r.min.y + 12.0), pos2(content.max.x, r.max.y - 12.0)], g);
                if !lay.draft {
                    painter.line_segment([pos2(r.min.x + 12.0, content.min.y), pos2(r.max.x - 12.0, content.min.y)], Stroke::new(1.0, th.guide.gamma_multiply(0.6)));
                    painter.line_segment([pos2(r.min.x + 12.0, content.max.y), pos2(r.max.x - 12.0, content.max.y)], Stroke::new(1.0, th.guide.gamma_multiply(0.6)));
                }
            }
            if opts.header_footer && !lay.draft {
                let pt = lay.ppi / 72.0;
                if let Some(hdr_txt) = self.doc.resolved_header_text(k, lay.pages) {
                    let gal = ui.ctx().fonts_mut(|f| {
                        layout::hf_galley(f, opts.book, &self.doc, false, &hdr_txt, lay.ppi)
                    });
                    let gw = gal.rect.width();
                    let gh = gal.rect.height();
                    let hx = match self.doc.header_align {
                        Align::Left | Align::Justify => content.min.x,
                        Align::Center => content.center().x - gw * 0.5,
                        Align::Right => (content.max.x - gw).max(content.min.x),
                    };
                    let hy = r.min.y + layout::hf_y(&self.doc, &lay, false, gh);
                    painter.galley(pos2(hx, hy), gal, Color32::BLACK);
                }
                if let Some(pf) = self.doc.resolved_page_footer(k, lay.pages) {
                    if let Some((x0, x1, y_bot, w_pt, col)) = pf.rule {
                        let ry = (r.max.y - y_bot * pt).clamp(r.min.y + 2.0, r.max.y - 2.0);
                        painter.line_segment(
                            [pos2(r.min.x + x0 * pt, ry), pos2(r.min.x + x1 * pt, ry)],
                            Stroke::new((w_pt * pt).max(0.8), layout::color(col)),
                        );
                    }
                    for seg in &pf.segments {
                        let mut job = egui::text::LayoutJob::default();
                        for run in &seg.runs {
                            let st = run.style;
                            let (fam, fake_it) = opts.book.family(st.font, st.bold, st.italic);
                            let ul_stroke = if st.underline {
                                Stroke::new((st.size * pt / 14.0).max(0.8), layout::color(st.color))
                            } else {
                                Stroke::NONE
                            };
                            let st_stroke = if st.strike {
                                Stroke::new((st.size * pt / 14.0).max(0.8), layout::color(st.color))
                            } else {
                                Stroke::NONE
                            };
                            job.append(
                                &run.text,
                                0.0,
                                egui::text::TextFormat {
                                    font_id: egui::FontId::new(st.size.max(4.5) * pt, fam),
                                    color: layout::color(st.color),
                                    background: st.highlight.map_or(Color32::TRANSPARENT, layout::color),
                                    italics: fake_it,
                                    underline: ul_stroke,
                                    strikethrough: st_stroke,
                                    ..Default::default()
                                },
                            );
                        }
                        let gal = ui.ctx().fonts_mut(|f| f.layout_job(job));
                        let gw = gal.rect.width();
                        let gh = gal.rect.height();
                        let fx = match seg.align {
                            Align::Center => {
                                let mid = r.min.x + ((seg.x_pt + seg.max_x_pt) * 0.5) * pt;
                                (mid - gw * 0.5).clamp(r.min.x + 2.0 * pt, (r.max.x - gw - 2.0 * pt).max(r.min.x))
                            }
                            Align::Right => (r.min.x + seg.max_x_pt * pt - gw).max(r.min.x + 2.0 * pt),
                            Align::Left | Align::Justify => {
                                (r.min.x + seg.x_pt * pt).min((r.max.x - gw - 2.0 * pt).max(r.min.x))
                            }
                        };
                        let ascent = gal
                            .rows
                            .first()
                            .and_then(|rw| rw.row.glyphs.first())
                            .map_or(gh * 0.78, |g| g.pos.y);
                        let fy = (r.max.y - seg.bottom_pt * pt - ascent)
                            .clamp(r.min.y + 1.0 * pt, (r.max.y - gh - 1.0 * pt).max(r.min.y));
                        painter.galley(pos2(fx, fy), gal, Color32::BLACK);
                    }
                } else if let Some(foot) = self.doc.resolved_footer_text(k, lay.pages) {
                    let gal = ui.ctx().fonts_mut(|f| {
                        layout::hf_galley(f, opts.book, &self.doc, true, &foot, lay.ppi)
                    });
                    let gw = gal.rect.width();
                    let gh = gal.rect.height();
                    let fx = match self.doc.footer_align {
                        Align::Left | Align::Justify => content.min.x,
                        Align::Center => content.center().x - gw * 0.5,
                        Align::Right => (content.max.x - gw).max(content.min.x),
                    };
                    let fy = r.min.y + layout::hf_y(&self.doc, &lay, true, gh);
                    painter.galley(pos2(fx, fy), gal, Color32::BLACK);
                }
            }

            // Table cells: background shading first, multi-cell selection highlight, then crisp borders
            let pt = lay.ppi / 72.0;
            for cell in lay.cells.iter().filter(|c| c.page == k) {
                let cr = cell.rect.translate(content.min.to_vec2());
                if let Some(rgb) = cell.shading {
                    painter.rect_filled(cr, 0.0, layout::color(rgb));
                }
                if self.selected_cells.contains(&(cell.table_id, cell.row, cell.col)) {
                    painter.rect_filled(cr, 0.0, th.accent.gamma_multiply(0.18));
                    painter.rect_stroke(cr, 0.0, Stroke::new(1.5, th.accent), StrokeKind::Inside);
                }
            }
            for cell in lay.cells.iter().filter(|c| c.page == k) {
                let cr = cell.rect.translate(content.min.to_vec2());
                if let Some(b) = cell.borders.top {
                    if b.width_pt > 0.0 {
                        let w = (b.width_pt * pt).max(1.0);
                        painter.line_segment([cr.left_top(), cr.right_top()], Stroke::new(w, layout::color(b.color)));
                    }
                }
                if let Some(b) = cell.borders.bottom {
                    if b.width_pt > 0.0 {
                        let w = (b.width_pt * pt).max(1.0);
                        painter.line_segment([cr.left_bottom(), cr.right_bottom()], Stroke::new(w, layout::color(b.color)));
                    }
                }
                if let Some(b) = cell.borders.left {
                    if b.width_pt > 0.0 {
                        let w = (b.width_pt * pt).max(1.0);
                        painter.line_segment([cr.left_top(), cr.left_bottom()], Stroke::new(w, layout::color(b.color)));
                    }
                }
                if let Some(b) = cell.borders.right {
                    if b.width_pt > 0.0 {
                        let w = (b.width_pt * pt).max(1.0);
                        painter.line_segment([cr.right_top(), cr.right_bottom()], Stroke::new(w, layout::color(b.color)));
                    }
                }
                if self
                    .doc
                    .table(cell.table_id)
                    .and_then(|t| t.rows.get(cell.row as usize))
                    .and_then(|r| r.cells.get(cell.col as usize))
                    .and_then(|cs| cs.formula.as_ref())
                    .is_some()
                {
                    painter.text(
                        pos2(cr.max.x - 3.0 * opts.zoom, cr.min.y + 2.0 * opts.zoom),
                        egui::Align2::RIGHT_TOP,
                        "∑",
                        egui::FontId::proportional((8.5 * opts.zoom).clamp(7.0, 13.0)),
                        th.accent.gamma_multiply(0.75),
                    );
                }
            }
        }

        // lines
        let (sa, sb) = self.selection();
        let has_sel = self.has_selection();
        let sel_col = th.selection;
        let mut caret_rect = None;
        let caret = self.doc.clamp(self.caret);
        let caret_line = lay.line_of(caret.p, caret.c);
        let mut visible_page = None;
        for (li, l) in lay.lines.iter().enumerate() {
            let o = lay.line_origin(l, canvas.min, canvas.width());
            let lr = Rect::from_min_size(o, vec2(l.xs.last().copied().unwrap_or(0.0).max(4.0), l.height));
            if li == caret_line {
                let idx = (caret.c - l.start).min(l.xs.len() - 1);
                let x = o.x + l.xs[idx];
                let gh = l.galley.rect.height();
                let w = if opts.block_cursor {
                    let default_w = (gh * 0.55).max(8.0);
                    if idx + 1 < l.xs.len() {
                        let cw = l.xs[idx + 1] - l.xs[idx];
                        if cw > 2.0 {
                            cw.clamp((gh * 0.45).max(6.0), (gh * 0.85).max(10.0))
                        } else {
                            default_w
                        }
                    } else {
                        default_w
                    }
                } else {
                    1.6
                };
                caret_rect = Some(Rect::from_min_size(pos2(x, o.y), vec2(w, gh)));
            }
            if lr.max.y < clip.min.y || lr.min.y > clip.max.y {
                continue;
            }
            if visible_page.is_none() && lr.center().y > clip.min.y + 40.0 {
                visible_page = Some(l.page);
            }
            let gh = l.galley.rect.height();
            let pfmt = self.doc.paras[l.para].fmt;
            let content_x = o.x - l.x;

            // Paragraph background shading (e.g. Code blocks / Callouts)
            if let Some(sh) = pfmt.shading {
                let (bx0, bx1) = if pfmt.table_id > 0 {
                    (0.0, lay.content_size.x)
                } else {
                    layout::para_block_x(&self.doc, &lay, l.para)
                };
                let sh_rect = Rect::from_min_size(
                    pos2(content_x + bx0 - 6.0, o.y - 1.0),
                    vec2(bx1 - bx0 + 12.0, l.height + 2.0),
                );
                painter.rect_filled(sh_rect, 4.0, layout::color(sh));
            }
            // Paragraph borders (Word box / top / bottom rules)
            for (r, c) in layout::para_border_rects(&self.doc, &lay, l) {
                painter.rect_filled(
                    r.translate(vec2(content_x, o.y - l.y)),
                    0.0,
                    layout::color(c),
                );
            }
            // Quote left accent bar
            if pfmt.style == 7 {
                painter.rect_filled(Rect::from_min_size(pos2(o.x - 12.0, o.y), vec2(3.5, l.height)), 1.5, th.accent);
            }
            // Horizontal rule divider
            if matches!(pfmt.block_kind(), crate::doc::BlockKind::HorizontalRule) && l.first_in_para {
                let y_mid = o.y + l.height * 0.5;
                let hr_col = pfmt.hr_color.map_or(Color32::from_gray(185), layout::color);
                let hr_w = if pfmt.hr_width_pt > 0.0 {
                    (pfmt.hr_width_pt * (lay.ppi / 72.0)).max(0.8)
                } else {
                    1.5
                };
                painter.line_segment([pos2(content_x, y_mid), pos2(content_x + lay.content_size.x, y_mid)], Stroke::new(hr_w, hr_col));
            }

            // Embedded image block + caption + interactive resize handles
            if pfmt.image_id > 0 && l.first_in_para {
                let indent =
                    self.doc.block_left_in(&pfmt).max(0.0) * lay.ppi + pfmt.right_in * lay.ppi;
                let cell_avail = if pfmt.table_id > 0 {
                    lay.cells
                        .iter()
                        .find(|c| c.table_id == pfmt.table_id && c.row == pfmt.table_row && c.col == pfmt.table_col && c.page == l.page)
                        .map(|c| (c.rect.width() - 8.0).max(12.0))
                } else {
                    None
                };
                let avail = cell_avail.unwrap_or_else(|| (lay.content_size.x - indent).max(lay.ppi * 0.25));
                if let Some((img_w, img_h, _cap_h, total_h)) = layout::image_box(&self.doc, pfmt.image_id, avail, lay.ppi) {
                    let scale = lay.ppi / 72.0;
                    let seamless = self.doc.image(pfmt.image_id).is_some_and(|im| im.seamless);
                    let pad_top = if seamless { 1.0 * scale } else { 3.0 * scale };
                    let img_r = Rect::from_min_size(pos2(o.x, o.y + pad_top), vec2(img_w, img_h));
                    let full_r = Rect::from_min_size(pos2(o.x, o.y), vec2(img_w, total_h));
                    let tex_opt = if let Some(doc_img) = self.doc.image(pfmt.image_id) {
                        let b64_len = doc_img.png_b64.len();
                        let needs_load = self.image_textures.get(&pfmt.image_id).is_none_or(|(len, _)| *len != b64_len);
                        if needs_load {
                            if let Some((w, h, rgba)) = doc_img.decode_rgba() {
                                let cimg = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                                let handle = ui.ctx().load_texture(
                                    format!("doc-img-{}-{b64_len}", pfmt.image_id),
                                    cimg,
                                    egui::TextureOptions::LINEAR,
                                );
                                self.image_textures.insert(pfmt.image_id, (b64_len, handle));
                            }
                        }
                        self.image_textures.get(&pfmt.image_id).map(|(_, h)| h.id())
                    } else {
                        None
                    };
                    let is_sel = self.selected_image == Some(pfmt.image_id);
                    let is_hov = hover_pos.is_some_and(|hp| full_r.expand(4.0).contains(hp));
                    if !seamless || is_sel || is_hov {
                        painter.rect_filled(img_r.translate(vec2(0.0, 1.5)).expand(1.0), 4.0, Color32::from_black_alpha(16));
                    }
                    if let Some(tid) = tex_opt {
                        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                        painter.image(tid, img_r, uv, Color32::WHITE);
                    } else {
                        painter.rect_filled(img_r, 4.0, Color32::from_gray(235));
                    }
                    if !seamless || is_sel || is_hov {
                        painter.rect_stroke(img_r, 3.0, Stroke::new(1.0, Color32::from_black_alpha(28)), StrokeKind::Inside);
                    }
                    if let Some(doc_img) = self.doc.image(pfmt.image_id) {
                        if !doc_img.caption.is_empty() {
                            let cap_font = egui::FontId::new((10.0 * scale).max(8.5), egui::FontFamily::Proportional);
                            let cap_y = img_r.max.y + 5.0 * scale;
                            painter.text(
                                pos2(img_r.center().x, cap_y),
                                egui::Align2::CENTER_TOP,
                                &doc_img.caption,
                                cap_font,
                                Color32::from_rgb(78, 86, 102),
                            );
                        } else if is_sel || is_hov {
                            let cap_font = egui::FontId::new((9.5 * scale).max(8.5), egui::FontFamily::Proportional);
                            let cap_y = img_r.max.y + 4.0 * scale;
                            painter.text(
                                pos2(img_r.center().x, cap_y),
                                egui::Align2::CENTER_TOP,
                                "+ Add a caption in the bar below (or double-click image)…",
                                cap_font,
                                th.accent.gamma_multiply(0.85),
                            );
                        }
                        if is_sel || is_hov || self.drag.resizing_image_id() == Some(pfmt.image_id) {
                            let stroke_w = if is_sel { 2.0 } else { 1.2 };
                            painter.rect_stroke(img_r, 2.0, Stroke::new(stroke_w, th.accent), StrokeKind::Outside);
                            for (hi, hpt) in image_handle_points(img_r).into_iter().enumerate() {
                                let hr = Rect::from_center_size(hpt, vec2(8.5, 8.5));
                                let active_h = self.drag.resizing_image_handle() == Some((pfmt.image_id, hi as u8))
                                    || hover_handle.is_some_and(|(id, idx, _)| id == pfmt.image_id && idx == hi as u8);
                                painter.rect_filled(hr, 2.0, if active_h { th.accent } else { Color32::WHITE });
                                painter.rect_stroke(hr, 2.0, Stroke::new(1.5, th.accent), StrokeKind::Inside);
                            }
                            if is_sel || self.drag.resizing_image_id() == Some(pfmt.image_id) {
                                let dim_txt = format!("{:.2} × {:.2} in", doc_img.width_in, doc_img.height_in);
                                let badge_rect = Rect::from_min_size(
                                    pos2((img_r.max.x - 94.0).max(img_r.min.x + 4.0), (img_r.max.y - 24.0).max(img_r.min.y + 4.0)),
                                    vec2(88.0, 18.0),
                                );
                                painter.rect_filled(badge_rect, 4.0, Color32::from_black_alpha(175));
                                painter.text(
                                    badge_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    dim_txt,
                                    egui::FontId::proportional(10.5),
                                    Color32::WHITE,
                                );
                            }
                        }
                    }
                }
            }

            // Active comment highlights on this paragraph
            for cm in &self.doc.comments {
                if !cm.resolved && cm.para == l.para && cm.start < l.end.max(l.start + 1) && cm.end >= l.start {
                    let s = cm.start.max(l.start) - l.start;
                    let e = cm.end.min(l.end) - l.start;
                    let x0 = o.x + l.xs[s];
                    let x1 = (o.x + l.xs[e]).max(x0 + 6.0);
                    let col = if cm.is_suggestion {
                        Color32::from_rgba_unmultiplied(134, 239, 172, 95)
                    } else {
                        Color32::from_rgba_unmultiplied(253, 224, 71, 95)
                    };
                    painter.rect_filled(Rect::from_min_max(pos2(x0, o.y), pos2(x1, o.y + gh)), 2.0, col);
                    if l.first_in_para {
                        let badge = Rect::from_min_size(pos2(content_x + lay.content_size.x + 10.0, o.y), vec2(18.0, 18.0));
                        painter.rect_filled(badge, 4.0, th.active);
                        icons::draw(&painter, badge.shrink(3.0), Icon::Comment, th.accent, th.accent);
                        if resp.clicked() {
                            if let Some(pos) = resp.interact_pointer_pos() {
                                if badge.expand(3.0).contains(pos) {
                                    cmds.push(Cmd::OpenComments);
                                }
                            }
                        }
                    }
                }
            }

            // Vertically-centered solid and Craft-style left-to-right fading gradient highlight pills
            {
                let para = &self.doc.paras[l.para];
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
                        let x0 = o.x + l.xs[rel_s];
                        let x1 = o.x + l.xs[rel_e];
                        if x1 > x0 {
                            let (y0, y1) = layout::highlight_pill_y(&l.galley, rel_s, rel_e, gh);
                            let pad_x = (gh * 0.12).clamp(2.0, 5.0);
                            let radius = ((y1 - y0) * 0.22).clamp(3.5, 7.0);
                            let hl_rect = Rect::from_min_max(pos2(x0 - pad_x, o.y + y0), pos2(x1 + pad_x, o.y + y1));
                            layout::paint_gradient_pill(&painter, hl_rect, c1, c2, radius);
                        }
                        continue;
                    }
                    k += 1;
                }
            }

            // find matches highlight
            for (ma, mb) in opts.matches {
                if ma.p == l.para && ma.c < l.end.max(l.start + 1) && mb.c > l.start {
                    let s = ma.c.max(l.start) - l.start;
                    let e = mb.c.min(l.end) - l.start;
                    painter.rect_filled(Rect::from_min_max(pos2(o.x + l.xs[s], o.y), pos2(o.x + l.xs[e], o.y + gh)), 2.0, th.find_highlight);
                }
            }
            if has_sel && l.para >= sa.p && l.para <= sb.p {
                let lp = l.para;
                let s_raw = if lp == sa.p { sa.c } else { 0 };
                let e_raw = if lp == sb.p { sb.c } else { usize::MAX };
                let s = s_raw.max(l.start);
                let e = e_raw.min(l.end);
                let continues = e_raw > l.end && l.last_in_para;
                if s <= l.end && (s < e || continues) {
                    let x0 = o.x + l.xs[s - l.start];
                    let x1 = o.x + l.xs[e.max(s) - l.start] + if continues { 5.0 } else { 0.0 };
                    if x1 > x0 {
                        painter.rect_filled(Rect::from_min_max(pos2(x0, o.y), pos2(x1, o.y + gh)), 0.0, sel_col);
                    }
                }
            }
            if let crate::doc::BlockKind::TableCell {
                table_id,
                row,
                col,
            } = pfmt.block_kind()
            {
                if let Some(cell) = lay
                    .cells
                    .iter()
                    .find(|c| c.table_id == table_id && c.row == row && c.col == col && c.page == l.page)
                {
                    if !cell.wrap_text {
                        let page_origin = canvas.min + lay.page_origin(l.page, canvas.width()).to_vec2() + lay.content_offset;
                        let cell_cr = cell.rect.translate(page_origin.to_vec2()).intersect(clip);
                        painter.with_clip_rect(cell_cr).galley(o, l.galley.clone(), Color32::BLACK);
                    } else {
                        painter.galley(o, l.galley.clone(), Color32::BLACK);
                    }
                } else {
                    painter.galley(o, l.galley.clone(), Color32::BLACK);
                }
            } else {
                painter.galley(o, l.galley.clone(), Color32::BLACK);
            }
            for (r, rad, c) in layout::leader_marks(l) {
                painter.rect_filled(r.translate(o.to_vec2()), rad, layout::color(c));
            }
            if let Some(sp) = opts.speller {
                let errs = spell_cache.entry(l.para).or_insert_with(|| self.spell_errors(sp, l.para));
                for &(a, b) in errs.iter() {
                    if a < l.end && b > l.start {
                        let s = a.max(l.start) - l.start;
                        let e = b.min(l.end) - l.start;
                        squiggle(&painter, o.x + l.xs[s], o.x + l.xs[e], o.y + gh - 1.5);
                    }
                }
            }
            if pfmt.list == ListKind::Checklist && l.first_in_para {
                let box_s = (13.0 * opts.zoom).clamp(10.0, 22.0);
                let bx = content_x + l.marker_x;
                let by = o.y + (gh - box_s) * 0.5;
                let br = Rect::from_min_size(pos2(bx, by), vec2(box_s, box_s));
                if pfmt.checked {
                    painter.rect_filled(br, 3.0, th.accent);
                    let p1 = pos2(br.min.x + box_s * 0.22, br.min.y + box_s * 0.52);
                    let p2 = pos2(br.min.x + box_s * 0.44, br.min.y + box_s * 0.74);
                    let p3 = pos2(br.min.x + box_s * 0.78, br.min.y + box_s * 0.28);
                    painter.add(egui::Shape::line(vec![p1, p2, p3], Stroke::new(1.8, Color32::WHITE)));
                } else {
                    painter.rect_stroke(br, 3.0, Stroke::new(1.5, Color32::from_gray(115)), StrokeKind::Inside);
                }
            } else if let Some(m) = &l.marker {
                painter.galley(pos2(content_x + l.marker_x, o.y), m.clone(), Color32::BLACK);
            }
            if opts.show_marks && l.last_in_para {
                let x = o.x + l.xs.last().copied().unwrap_or(0.0) + 2.0;
                painter.text(pos2(x, o.y + gh * 0.5), egui::Align2::LEFT_CENTER, "¶", egui::FontId::proportional((gh * 0.6).max(6.0)), th.accent.gamma_multiply(0.7));
            }
            if opts.show_marks && l.start == 0 && self.doc.paras[l.para].fmt.page_break_before {
                let y = o.y - 3.0;
                let cx0 = o.x - l.x;
                painter.text(pos2(cx0, y), egui::Align2::LEFT_BOTTOM, "— page break —", egui::FontId::proportional(9.0), th.accent.gamma_multiply(0.7));
            }
        }
        if let Some(vp) = visible_page {
            self.current_page = vp;
        } else if let Some(l) = lay.lines.get(caret_line) {
            self.current_page = l.page;
        }

        // Paint active table row/column selection headers & resize edge highlight guides
        if opts.mode != DocMode::Viewing {
            for &(t, c, hr, ref lbl) in &col_header_rects {
                let col_sel = !self.selected_cells.is_empty()
                    && self
                        .selected_cells
                        .iter()
                        .all(|&(st, _, sc)| st == t && sc == c)
                    && self.selected_cells.len() >= self.doc.table(t).map_or(1, |tb| tb.rows.len());
                let hov = hover_col_hdr == Some((t, c as usize));
                let bg = if col_sel || hov { th.accent } else { th.surface_elevated };
                let fg = if col_sel || hov { Color32::WHITE } else { th.subtle };
                painter.rect_filled(hr, 3.0, bg);
                painter.rect_stroke(hr, 3.0, Stroke::new(1.0, if col_sel || hov { th.accent } else { th.border }), StrokeKind::Inside);
                painter.text(hr.center(), egui::Align2::CENTER_CENTER, lbl, egui::FontId::proportional(9.5), fg);
            }
            for &(t, r, hr, ref lbl) in &row_header_rects {
                let row_sel = !self.selected_cells.is_empty()
                    && self
                        .selected_cells
                        .iter()
                        .all(|&(st, sr, _)| st == t && sr == r)
                    && self.selected_cells.len()
                        >= self
                            .doc
                            .table(t)
                            .and_then(|tb| tb.rows.get(r as usize))
                            .map_or(1, |rw| rw.cells.len());
                let hov = hover_row_hdr == Some((t, r as usize));
                let bg = if row_sel || hov { th.accent } else { th.surface_elevated };
                let fg = if row_sel || hov { Color32::WHITE } else { th.subtle };
                painter.rect_filled(hr, 3.0, bg);
                painter.rect_stroke(hr, 3.0, Stroke::new(1.0, if row_sel || hov { th.accent } else { th.border }), StrokeKind::Inside);
                painter.text(hr.center(), egui::Align2::CENTER_CENTER, lbl, egui::FontId::proportional(9.5), fg);
            }
            if let Some((_, _, _, x_scr, y0, y1)) = hover_col_edge {
                painter.line_segment([pos2(x_scr, y0), pos2(x_scr, y1)], Stroke::new(2.4, th.accent));
            }
            if let Some((_, _, y_scr, x0, x1)) = hover_row_edge {
                painter.line_segment([pos2(x0, y_scr), pos2(x1, y_scr)], Stroke::new(2.4, th.accent));
            }
            if let CanvasDrag::ResizingTableCol {
                table_id: tid,
                row: r_idx,
                col: c_idx,
                ..
            } = self.drag
            {
                if let Some(&(_, _, _, cr)) = cell_screen_rects
                    .iter()
                    .find(|&&(t, r, c, _)| t == tid && r as usize == r_idx && c as usize == c_idx)
                {
                    painter.line_segment([pos2(cr.max.x, cr.min.y - 8.0), pos2(cr.max.x, cr.max.y + 8.0)], Stroke::new(2.4, th.accent));
                    let w_in = self
                        .doc
                        .table(tid)
                        .and_then(|tb| tb.rows.get(r_idx))
                        .and_then(|rw| rw.cells.get(c_idx))
                        .map_or(1.0, |cs| cs.width_in);
                    let badge = Rect::from_center_size(pos2(cr.max.x, cr.min.y - 12.0), vec2(58.0, 17.0));
                    painter.rect_filled(badge, 4.0, Color32::from_black_alpha(185));
                    painter.text(
                        badge.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("{w_in:.2} in"),
                        egui::FontId::proportional(10.0),
                        Color32::WHITE,
                    );
                }
            }
            if let CanvasDrag::ResizingTableRow {
                table_id: tid,
                row: r_idx,
                ..
            } = self.drag
            {
                if let Some(&(_, _, _, cr)) = cell_screen_rects
                    .iter()
                    .find(|&&(t, r, _, _)| t == tid && r as usize == r_idx)
                {
                    painter.line_segment([pos2(cr.min.x - 8.0, cr.max.y), pos2(cr.max.x + 8.0, cr.max.y)], Stroke::new(2.4, th.accent));
                    let h_pt = self
                        .doc
                        .table(tid)
                        .and_then(|tb| tb.rows.get(r_idx))
                        .map_or(0.0, |rw| rw.min_height_pt);
                    let badge = Rect::from_center_size(pos2(cr.max.x + 34.0, cr.max.y), vec2(58.0, 17.0));
                    painter.rect_filled(badge, 4.0, Color32::from_black_alpha(185));
                    painter.text(
                        badge.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("{h_pt:.0} pt"),
                        egui::FontId::proportional(10.0),
                        Color32::WHITE,
                    );
                }
            }
        }

        // caret
        let focused = ui.memory(|m| m.has_focus(focus_id) || m.focused().is_none());
        if let Some(cr) = caret_rect {
            if focused && opts.mode != DocMode::Viewing && self.selected_image.is_none() {
                let t = ui.input(|i| i.time);
                let since = (t - self.last_edit_time.min(t)).max(0.0);
                let on_duration = 0.53_f32;
                let off_duration = 0.53_f32;
                let total_duration = on_duration + off_duration;
                let time_in_cycle = (since % (total_duration as f64)) as f32;
                let wake_in = if time_in_cycle < on_duration {
                    if opts.block_cursor {
                        let left_col = Color32::from_rgba_unmultiplied(105, 112, 122, 175);
                        let right_col = Color32::from_rgba_unmultiplied(220, 224, 230, 55);
                        let mut mesh = egui::Mesh::default();
                        let uv = egui::epaint::WHITE_UV;
                        mesh.vertices.push(egui::epaint::Vertex { pos: cr.left_top(), uv, color: left_col });
                        mesh.vertices.push(egui::epaint::Vertex { pos: cr.right_top(), uv, color: right_col });
                        mesh.vertices.push(egui::epaint::Vertex { pos: cr.right_bottom(), uv, color: right_col });
                        mesh.vertices.push(egui::epaint::Vertex { pos: cr.left_bottom(), uv, color: left_col });
                        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
                        painter.add(egui::Shape::mesh(mesh));
                        painter.rect_stroke(cr, 0.0, Stroke::new(1.0, Color32::from_gray(128)), StrokeKind::Inside);
                    } else {
                        let caret_col = if opts.mode == DocMode::Suggesting { Color32::from_rgb(22, 163, 74) } else { th.caret };
                        painter.rect_filled(Rect::from_min_size(cr.min, vec2(1.6, cr.height())), 0.0, caret_col);
                    }
                    on_duration - time_in_cycle
                } else {
                    total_duration - time_in_cycle
                };
                ui.ctx().request_repaint_after_secs(wake_in + 0.005);
            }
        }
        if let Some(pg) = self.scroll_to_page.take() {
            if let Some(r) = page_rects.get(pg) {
                ui.scroll_to_rect(Rect::from_min_size(r.min, vec2(r.width(), 40.0)), Some(egui::Align::TOP));
            }
        } else if self.scroll_to_caret {
            if let Some(cr) = caret_rect {
                ui.scroll_to_rect(cr.expand2(vec2(40.0, 30.0)), None);
            }
            self.scroll_to_caret = false;
        }
        let _ = Vec2::ZERO;
        (resp, caret_rect)
    }
}

fn squiggle(p: &egui::Painter, x0: f32, x1: f32, y: f32) {
    let col = Color32::from_rgb(220, 38, 38);
    let step = 2.0;
    let mut pts = Vec::new();
    let mut x = x0;
    let mut up = false;
    while x <= x1 {
        pts.push(pos2(x, if up { y - 1.2 } else { y + 1.2 }));
        up = !up;
        x += step;
    }
    if pts.len() >= 2 {
        p.add(egui::Shape::line(pts, Stroke::new(1.0, col)));
    }
}

fn image_handle_points(r: Rect) -> [Pos2; 8] {
    [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        pos2(r.center().x, r.min.y),
        pos2(r.center().x, r.max.y),
        pos2(r.min.x, r.center().y),
        pos2(r.max.x, r.center().y),
    ]
}

fn handle_cursor(idx: u8) -> egui::CursorIcon {
    match idx {
        0 | 2 => egui::CursorIcon::ResizeNwSe,
        1 | 3 => egui::CursorIcon::ResizeNeSw,
        4 | 5 => egui::CursorIcon::ResizeVertical,
        _ => egui::CursorIcon::ResizeHorizontal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sc() -> SmartCorrect<'static> {
        static TABLE: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
        SmartCorrect {
            enabled: true,
            quotes: true,
            markdown: true,
            auto_links: true,
            table: TABLE.get_or_init(crate::doc::smartcorrect_table),
        }
    }

    #[test]
    fn typing_enter_backspace_undo() {
        let mut t = Tab::new(1, "Untitled1".into(), Document::default());
        for ch in "Hello teh ".chars() {
            t.type_char(&ch.to_string(), 0.0, &sc());
        }
        assert_eq!(t.doc.plain_text(), "Hello the "); // SmartCorrect from ltscorrt.reg
        t.enter(10.0);
        t.type_char("\"", 20.0, &sc());
        t.type_char("x", 20.0, &sc());
        assert_eq!(t.doc.paras.len(), 2);
        assert_eq!(t.doc.paras[1].string(), "“x");
        t.backspace(false, 30.0);
        t.backspace(false, 30.1);
        t.backspace(false, 30.2);
        assert_eq!(t.doc.paras.len(), 1);
        t.undo();
        assert_eq!(t.doc.paras.len(), 2);
        t.redo();
        assert_eq!(t.doc.paras.len(), 1);
    }

    #[test]
    fn bold_toggle_applies_to_selection_and_typing() {
        let mut t = Tab::new(1, "a".into(), Document::default());
        t.insert("word one", 0.0);
        t.anchor = Pos::new(0, 0);
        t.caret = Pos::new(0, 4);
        t.toggle(1.0, |s| s.bold, |s, v| s.bold = v);
        assert!(t.doc.paras[0].styles[0].bold);
        assert!(!t.doc.paras[0].styles[5].bold);
        t.set_caret(Pos::new(0, 8), false);
        t.toggle(2.0, |s| s.italic, |s, v| s.italic = v);
        t.insert("!", 3.0);
        assert!(t.doc.paras[0].styles[8].italic);
    }

    #[test]
    fn markdown_auto_format_and_checklists() {
        let mut t = Tab::new(1, "Untitled1".into(), Document::default());
        for ch in "# Heading".chars() {
            t.type_char(&ch.to_string(), 0.0, &sc());
        }
        assert_eq!(t.doc.paras[0].fmt.style, 4); // Heading 1
        assert_eq!(t.doc.paras[0].string(), "Heading");
        t.enter(1.0);
        for ch in "[] Buy milk".chars() {
            t.type_char(&ch.to_string(), 2.0, &sc());
        }
        assert_eq!(t.doc.paras[1].fmt.list, ListKind::Checklist);
        assert!(!t.doc.paras[1].fmt.checked);
        assert_eq!(t.doc.paras[1].string(), "Buy milk");
    }

    #[test]
    fn layout_paginates_and_maps_positions() {
        let ctx = egui::Context::default();
        let book = crate::fonts::FontBook::default();
        let mut doc = Document::default();
        let long = "lorem ipsum dolor sit amet ".repeat(400);
        doc.insert_text(Pos::new(0, 0), &long, CharStyle::default());
        let mut pages = 0;
        let mut ok = false;
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            let lay = crate::layout::build(&ctx, &doc, &book, 1.0, false);
            pages = lay.pages;
            // every char boundary must be reachable
            let total: usize = lay.lines.iter().map(|l| l.end - l.start).sum();
            ok = total == doc.paras[0].len();
            let li = lay.line_of(0, 500);
            assert!(lay.lines[li].start <= 500 && 500 <= lay.lines[li].end);
        });
        out.textures_delta.clear();
        assert!(ok, "glyphs map 1:1 to characters");
        assert!(pages >= 2, "long text flows onto more pages");
    }

    #[test]
    fn shortcut_presets_dispatch_expected_actions() {
        let mut t = Tab::new(1, "Untitled1".into(), Document::default());
        t.insert("hello", 0.0);
        let ctx = egui::Context::default();
        let book = crate::fonts::FontBook::default();
        let mut cmds = Vec::new();
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            let lay = crate::layout::build(&ctx, &t.doc, &book, 1.0, false);
            let m_ctrl = Modifiers { ctrl: true, ..Default::default() };
            let m_ctrl_shift = Modifiers { ctrl: true, shift: true, ..Default::default() };
            let m_none = Modifiers::default();

            // MS Word: Ctrl+E centers paragraph, F12 saves as
            t.key_mode(Key::E, m_ctrl, 1.0, &lay, 20, &mut cmds, DocMode::Editing, ShortcutPreset::MsWord);
            assert_eq!(t.doc.paras[0].fmt.align, Align::Center);
            t.key_mode(Key::F12, m_none, 2.0, &lay, 20, &mut cmds, DocMode::Editing, ShortcutPreset::MsWord);
            assert!(matches!(cmds.pop(), Some(Cmd::SaveAs)));

            // WordPro: Ctrl+R right-aligns, Ctrl+F2 opens spellcheck
            t.key_mode(Key::R, m_ctrl, 3.0, &lay, 20, &mut cmds, DocMode::Editing, ShortcutPreset::WordPro);
            assert_eq!(t.doc.paras[0].fmt.align, Align::Right);
            t.key_mode(Key::F2, m_ctrl, 4.0, &lay, 20, &mut cmds, DocMode::Editing, ShortcutPreset::WordPro);
            assert!(matches!(cmds.pop(), Some(Cmd::Spell)));

            // Google Docs: Ctrl+Shift+L left-aligns, Ctrl+Shift+C opens word count
            t.key_mode(Key::L, m_ctrl_shift, 5.0, &lay, 20, &mut cmds, DocMode::Editing, ShortcutPreset::GoogleDocs);
            assert_eq!(t.doc.paras[0].fmt.align, Align::Left);
            t.key_mode(Key::C, m_ctrl_shift, 6.0, &lay, 20, &mut cmds, DocMode::Editing, ShortcutPreset::GoogleDocs);
            assert!(matches!(cmds.pop(), Some(Cmd::WordCount)));
        });
        out.textures_delta.clear();
    }

    #[test]
    fn highlight_pill_is_vertically_centered_around_glyphs() {
        let ctx = egui::Context::default();
        let mut book = crate::fonts::FontBook::default();
        let mut doc = Document::default();
        doc.insert_text(
            Pos::new(0, 0),
            "Andrew Thomas",
            CharStyle {
                highlight: Some([254, 240, 138]),
                highlight_to: Some([253, 186, 116]),
                ..CharStyle::default()
            },
        );
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            book = crate::fonts::install(&ctx);
        });
        out.textures_delta.clear();
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            let lay = crate::layout::build(&ctx, &doc, &book, 1.0, false);
            let l = &lay.lines[0];
            let (y0, y1) = crate::layout::highlight_pill_y(&l.galley, l.start, l.end, l.height);
            let glyphs = &l.galley.rows[0].row.glyphs;
            let ink_top = glyphs
                .iter()
                .filter(|g| !g.chr.is_whitespace() && !g.uv_rect.is_nothing())
                .map(|g| g.pos.y + g.uv_rect.offset.y)
                .fold(f32::INFINITY, f32::min);
            let ink_bot = glyphs
                .iter()
                .filter(|g| !g.chr.is_whitespace() && !g.uv_rect.is_nothing())
                .map(|g| g.pos.y + g.uv_rect.offset.y + g.uv_rect.size.y)
                .fold(f32::NEG_INFINITY, f32::max);
            let pill_mid = (y0 + y1) * 0.5;
            let ink_mid = (ink_top + ink_bot) * 0.5;
            assert!(
                (pill_mid - ink_mid).abs() < 0.05,
                "Expected pill midpoint {pill_mid} to match glyph ink midpoint {ink_mid}"
            );
            assert!(y0 < ink_top && y1 > ink_bot, "Expected equal padding above {ink_top} ({y0}) and below {ink_bot} ({y1})");
        });
        out.textures_delta.clear();
    }

    #[test]
    fn image_insert_resize_caption_and_roundtrip() {
        let mut raw_png = Vec::new();
        let rgba = image::RgbaImage::from_pixel(40, 20, image::Rgba([43, 87, 151, 255]));
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(&mut std::io::Cursor::new(&mut raw_png), image::ImageFormat::Png)
            .expect("encode test png");

        let img = DocImage::from_file_bytes(1, "diagram.png".into(), &raw_png, 6.5).expect("decode test png");
        assert_eq!(img.natural_size, [40, 20]);
        assert!((img.aspect_ratio() - 2.0).abs() < 0.01);

        let mut t = Tab::new(1, "Untitled1".into(), Document::default());
        t.insert_image(img, 1.0);
        assert_eq!(t.selected_image, Some(1));
        assert_eq!(t.doc.paras[0].fmt.image_id, 1);

        if let Some(im) = t.doc.image_mut(1) {
            im.width_in = 3.2;
            im.height_in = 1.6;
            im.caption = "Figure 1: System Architecture".into();
        }
        let json = crate::doc::to_json(&t.doc);
        let restored = crate::doc::from_json(&json).expect("roundtrip json");
        let r_img = restored.image(1).expect("restored image");
        assert_eq!(r_img.caption, "Figure 1: System Architecture");
        assert!((r_img.width_in - 3.2).abs() < 0.001);
        assert!((r_img.height_in - 1.6).abs() < 0.001);
        let (dw, dh, drgba) = r_img.decode_rgba().expect("decode restored rgba");
        assert_eq!((dw, dh), (40, 20));
        assert_eq!(&drgba[0..4], &[43, 87, 151, 255]);
    }

    #[test]
    fn table_tab_arrows_and_backspace_stay_inside_cells() {
        let mut t = Tab::new(1, "TableTest".into(), Document::default());
        let first_pos = t.doc.insert_table(Pos::new(0, 0), 2, 2);
        let tid = t.doc.paras[first_pos.p].fmt.table_id;
        assert!(tid > 0);
        t.set_caret(first_pos, false);

        // Populate cells (0,0)="A1", (0,1)="B1", (1,0)="A2", (1,1)="B2" via Tab navigation
        t.insert("A1", 1.0);
        assert!(t.tab_in_table(false, 2.0));
        assert_eq!((t.doc.paras[t.caret.p].fmt.table_row, t.doc.paras[t.caret.p].fmt.table_col), (0, 1));
        t.insert("B1", 2.1);

        assert!(t.tab_in_table(false, 3.0));
        assert_eq!((t.doc.paras[t.caret.p].fmt.table_row, t.doc.paras[t.caret.p].fmt.table_col), (1, 0));
        t.insert("A2", 3.1);

        assert!(t.tab_in_table(false, 4.0));
        assert_eq!((t.doc.paras[t.caret.p].fmt.table_row, t.doc.paras[t.caret.p].fmt.table_col), (1, 1));
        t.insert("B2", 4.1);

        // Tab on the very last cell (1,1) appends row 2 and moves into (2,0)
        assert!(t.tab_in_table(false, 5.0));
        assert_eq!(t.doc.table(tid).unwrap().rows.len(), 3);
        assert_eq!((t.doc.paras[t.caret.p].fmt.table_row, t.doc.paras[t.caret.p].fmt.table_col), (2, 0));

        // Shift+Tab moves back to (1,1) -> (1,0) -> (0,1)
        assert!(t.tab_in_table(true, 6.0));
        assert_eq!((t.doc.paras[t.caret.p].fmt.table_row, t.doc.paras[t.caret.p].fmt.table_col), (1, 1));
        assert!(t.tab_in_table(true, 7.0));
        assert_eq!((t.doc.paras[t.caret.p].fmt.table_row, t.doc.paras[t.caret.p].fmt.table_col), (1, 0));
        assert!(t.tab_in_table(true, 8.0));
        assert_eq!((t.doc.paras[t.caret.p].fmt.table_row, t.doc.paras[t.caret.p].fmt.table_col), (0, 1));

        // Backspace at col 0 of cell (0,1) MUST NOT merge into (0,0) or jump caret
        t.set_caret(Pos::new(t.caret.p, 0), false);
        t.backspace(false, 9.0);
        assert_eq!((t.doc.paras[t.caret.p].fmt.table_row, t.doc.paras[t.caret.p].fmt.table_col), (0, 1));
        assert_eq!(t.doc.cell_text(tid, 0, 0), "A1");
        assert_eq!(t.doc.cell_text(tid, 0, 1), "B1");

        // Delete at end of cell (0,0) MUST NOT swallow (0,1)
        let p00 = t.doc.table_cell_para(tid, 0, 0).unwrap();
        t.set_caret(Pos::new(p00, 2), false);
        t.delete_forward(false, 10.0);
        assert_eq!((t.doc.paras[t.caret.p].fmt.table_row, t.doc.paras[t.caret.p].fmt.table_col), (0, 0));
        assert_eq!(t.doc.cell_text(tid, 0, 0), "A1");
        assert_eq!(t.doc.cell_text(tid, 0, 1), "B1");

        // ArrowDown from (0,0) must move vertically to (1,0), NOT sideways to (0,1)
        let ctx = egui::Context::default();
        let book = crate::fonts::FontBook::default();
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            let lay = layout::build(&ctx, &t.doc, &book, 1.0, false);
            t.move_vert(&lay, 1, false);
        });
        out.textures_delta.clear();
        assert_eq!((t.doc.paras[t.caret.p].fmt.table_row, t.doc.paras[t.caret.p].fmt.table_col), (1, 0));

        // ArrowUp from (1,0) must move straight back to (0,0)
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            let lay = layout::build(&ctx, &t.doc, &book, 1.0, false);
            t.move_vert(&lay, -1, false);
        });
        out.textures_delta.clear();
        assert_eq!((t.doc.paras[t.caret.p].fmt.table_row, t.doc.paras[t.caret.p].fmt.table_col), (0, 0));

        // Cross-cell range deletion clears text in both cells without destroying cell paragraphs
        let p01 = t.doc.table_cell_para(tid, 0, 1).unwrap();
        t.anchor = Pos::new(p00, 1);
        t.caret = Pos::new(p01, 1);
        t.backspace(false, 11.0);
        assert_eq!(t.doc.cell_text(tid, 0, 0), "A");
        assert_eq!(t.doc.cell_text(tid, 0, 1), "1");
        assert!(t.doc.table_cell_para(tid, 0, 1).is_some());
    }
}

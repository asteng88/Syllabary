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
        }
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
        // Enter on "---" turns it into a horizontal divider line.
        let p_str = self.doc.paras[self.caret.p].string();
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
        self.checkpoint(EditKind::Delete, now);
        if !self.delete_selection() {
            let para = &self.doc.paras[self.caret.p];
            if self.caret.c == 0 && (para.fmt.list != ListKind::None || para.fmt.indent > 0 || para.fmt.hr) {
                let f = &mut self.doc.paras[self.caret.p].fmt;
                if f.hr {
                    f.hr = false;
                } else if f.list != ListKind::None {
                    f.list = ListKind::None;
                    f.checked = false;
                } else {
                    f.indent -= 1;
                }
            } else {
                let from = if word { self.doc.word_left(self.caret) } else { self.doc.prev(self.caret) };
                self.caret = self.doc.delete_range(from, self.caret);
            }
            self.anchor = self.caret;
        }
        self.changed();
    }

    pub fn delete_forward(&mut self, word: bool, now: f64) {
        self.checkpoint(EditKind::Delete, now);
        if !self.delete_selection() {
            let to = if word { self.doc.word_right(self.caret) } else { self.doc.next(self.caret) };
            self.caret = self.doc.delete_range(self.caret, to);
            self.anchor = self.caret;
        }
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

    /// Change character formatting of the selection, or of the next typed text.
    pub fn apply_char(&mut self, now: f64, f: &dyn Fn(&mut CharStyle)) {
        if self.has_selection() {
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
        for p in self.doc.para_range(self.anchor, self.caret) {
            f(&mut self.doc.paras[p].fmt);
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
        }
        self.pending = None;
        self.scroll_to_caret = true;
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
        resp.context_menu(|ui| {
            crate::chrome::fit_menu(
                ui,
                &[
                    ("Cut", "Ctrl+X"),
                    ("Copy", "Ctrl+C"),
                    ("Paste", "Ctrl+V"),
                    ("Paste without formatting", kp.paste_plain()),
                    ("Comment", "Ctrl+Alt+M"),
                    ("Insert link", "Ctrl+K"),
                    ("Insert smart chip / building block", "@"),
                    ("Define word", kp.dictionary()),
                    ("Select All", "Ctrl+A"),
                    ("Clear formatting", kp.clear_format()),
                    ("Capitalization", "▸"),
                ],
            );
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
        let first = lay.lines.partition_point(|l| l.page < k);
        let last = lay.lines.partition_point(|l| l.page <= k);
        let li = if first >= last {
            // empty page (after a page break) → nearest line before it
            first.min(lay.lines.len() - 1)
        } else {
            let mut best = first;
            for i in first..last {
                if lay.lines[i].y <= cy {
                    best = i;
                } else {
                    break;
                }
            }
            best
        };
        let c = lay.char_at_x(li, cx);
        Pos::new(lay.lines[li].para, c)
    }

    fn move_vert(&mut self, lay: &PageLayout, lines: isize, extend: bool) {
        let li = lay.line_of(self.caret.p, self.caret.c);
        let x = self.desired_x.unwrap_or_else(|| lay.x_in_line(li, self.caret.c));
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
                let para = &self.doc.paras[self.caret.p];
                if para.fmt.list != ListKind::None || (self.has_selection() && self.anchor.p != self.caret.p) {
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
    /// Draws the pages and handles mouse input. Returns the caret's screen rect.
    pub fn show(&mut self, ui: &mut egui::Ui, opts: &ViewOpts, focus_id: egui::Id, cmds: &mut Vec<Cmd>) -> (egui::Response, Option<Rect>) {
        let lay = self.layout(ui.ctx(), opts);
        let avail_w = ui.available_width();
        let size = lay.canvas_size(avail_w);
        let (canvas, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        self.canvas_x0 = canvas.min.x;
        self.canvas_w = canvas.width();
        let th = opts.theme;
        let now = ui.input(|i| i.time);

        // Pre-compute interactive checklist checkbox rects so clicks on a checkbox toggle it directly
        let mut checkbox_rects: Vec<(usize, Rect)> = Vec::new();
        for l in &lay.lines {
            if l.first_in_para && self.doc.paras[l.para].fmt.list == ListKind::Checklist {
                let o = lay.line_origin(l, canvas.min, canvas.width());
                let content_x = o.x - l.x;
                let gh = l.galley.rect.height();
                let box_s = (13.0 * opts.zoom).clamp(10.0, 22.0);
                let bx = content_x + l.marker_x;
                let by = o.y + (gh - box_s) * 0.5;
                checkbox_rects.push((l.para, Rect::from_min_size(pos2(bx, by), vec2(box_s, box_s)).expand(3.0)));
            }
        }

        let hover_pos = ui.input(|i| i.pointer.hover_pos());
        let over_checkbox = hover_pos.is_some_and(|hp| checkbox_rects.iter().any(|(_, r)| r.contains(hp)));
        let resp = resp.on_hover_cursor(if over_checkbox { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Text });

        // mouse
        if resp.clicked() || resp.drag_started() || resp.double_clicked() || resp.triple_clicked() {
            ui.memory_mut(|m| m.request_focus(focus_id));
        }
        let mut toggled_checkbox = false;
        if resp.clicked() && opts.mode != DocMode::Viewing {
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
        if !toggled_checkbox {
            if let Some(pp) = resp.interact_pointer_pos() {
                ui.memory_mut(|m| m.request_focus(focus_id));
                self.last_edit_time = now;
                let pos = self.hit(&lay, canvas, pp);
                let shift = ui.input(|i| i.modifiers.shift);
                if resp.triple_clicked() {
                    self.anchor = Pos::new(pos.p, 0);
                    self.caret = Pos::new(pos.p, self.doc.paras[pos.p].len());
                } else if resp.double_clicked() {
                    let (a, b) = self.doc.word_at(pos);
                    self.anchor = a;
                    self.caret = b;
                } else if ui.input(|i| i.pointer.primary_pressed()) {
                    self.set_caret(pos, shift);
                } else if resp.dragged() {
                    self.caret = pos;
                    self.pending = None;
                }
                self.desired_x = None;
                self.scroll_to_caret = false;
            }
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
                let hf = egui::FontId::new(10.0 * lay.ppi / 72.0, egui::FontFamily::Proportional);
                let gray = Color32::from_gray(110);
                if !self.doc.header.is_empty() {
                    painter.text(pos2(r.center().x, r.min.y + lay.content_offset.y * 0.5), egui::Align2::CENTER_CENTER, &self.doc.header, hf.clone(), gray);
                }
                let mut foot = self.doc.footer.clone();
                if self.doc.page.page_numbers {
                    let pn = format!("Page {}", k + 1);
                    foot = if foot.is_empty() { pn } else { format!("{foot}    {pn}") };
                }
                if !foot.is_empty() {
                    painter.text(pos2(r.center().x, r.max.y - (lay.page_size.y - lay.content_offset.y - lay.content_size.y) * 0.5), egui::Align2::CENTER_CENTER, foot, hf, gray);
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
                let sh_rect = Rect::from_min_size(pos2(content_x - 6.0, o.y - 1.0), vec2(lay.content_size.x + 12.0, l.height + 2.0));
                painter.rect_filled(sh_rect, 4.0, layout::color(sh));
            }
            // Quote left accent bar
            if pfmt.style == 7 {
                painter.rect_filled(Rect::from_min_size(pos2(o.x - 12.0, o.y), vec2(3.5, l.height)), 1.5, th.accent);
            }
            // Horizontal rule divider
            if pfmt.hr && l.first_in_para {
                let y_mid = o.y + l.height * 0.5;
                painter.line_segment([pos2(content_x, y_mid), pos2(content_x + lay.content_size.x, y_mid)], Stroke::new(1.5, Color32::from_gray(185)));
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
            painter.galley(o, l.galley.clone(), Color32::BLACK);
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

        // caret
        let focused = ui.memory(|m| m.has_focus(focus_id) || m.focused().is_none());
        if let Some(cr) = caret_rect {
            if focused && opts.mode != DocMode::Viewing {
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
}

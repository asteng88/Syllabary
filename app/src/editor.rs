//! One open document (a tab): caret, selection, undo and the on-screen page view.

use eframe::egui::{self, pos2, vec2, Color32, Event, Key, Modifiers, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use std::path::PathBuf;
use std::rc::Rc;

use crate::doc::*;
use crate::fonts::FontBook;
use crate::layout::{self, PageLayout, PAGE_GAP};
use crate::theme::Theme;

pub const UNDO_DEPTH: usize = 100;

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
}

pub struct ViewOpts<'a> {
    pub zoom: f32,
    pub draft: bool,
    pub show_marks: bool,
    pub margin_guides: bool,
    pub header_footer: bool,
    pub matches: &'a [(Pos, Pos)],
    pub theme: &'a Theme,
    pub book: &'a FontBook,
    pub fonts_rev: u64,
    pub speller: Option<&'a crate::spell::Speller>,
}

pub struct SmartCorrect<'a> {
    pub enabled: bool,
    pub quotes: bool,
    pub table: &'a [(String, String)],
}

/// What the editor asks the app to do (menu-level actions triggered by keys).
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
}

impl Tab {
    pub fn new(id: u64, name: String, doc: Document) -> Self {
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
        }
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
            return self.doc.style_at(self.doc.next(a)).clone();
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
        let style = self.typing_style();
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
            let opening = prev.map(|c| c.is_whitespace() || "([{“‘—–".contains(c)).unwrap_or(true);
            text = match (s, opening) {
                ("\"", true) => "“",
                ("\"", false) => "”",
                (_, true) => "‘",
                _ => "’",
            }
            .to_string();
        }
        self.insert(&text, now);
        if sc.enabled && text.chars().count() == 1 {
            let ch = text.chars().next().unwrap();
            if ch == ' ' || ".,;:!?)".contains(ch) {
                self.smart_correct(sc.table);
            }
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
        // strip leading punctuation like quotes
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
        // Enter on an empty list item ends the list.
        let para = &self.doc.paras[self.caret.p];
        if para.text.is_empty() && para.fmt.list != ListKind::None {
            self.doc.paras[self.caret.p].fmt.list = ListKind::None;
            self.changed();
            return;
        }
        let style = self.typing_style();
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
            if self.caret.c == 0 && (para.fmt.list != ListKind::None || para.fmt.indent > 0) {
                // first backspace removes the bullet / indent, like modern editors
                let f = &mut self.doc.paras[self.caret.p].fmt;
                if f.list != ListKind::None {
                    f.list = ListKind::None;
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
                let (b, i, u) = (s.bold && !ns.chr.bold, s.italic && !ns.chr.italic, s.underline);
                *s = ns.chr;
                s.bold |= b;
                s.italic |= i;
                s.underline |= u;
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

    pub fn copy(&self) -> Option<(String, Fragment)> {
        if !self.has_selection() {
            return None;
        }
        let (a, b) = self.selection();
        Some((self.doc.text_range(a, b), self.doc.fragment(a, b)))
    }

    pub fn current_style_name(&self) -> &'static str {
        let idx = self.doc.paras[self.doc.clamp(self.caret).p].fmt.style;
        named_styles().get(idx).map(|s| s.name).unwrap_or("Default Text")
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
        let key = LayoutKey { rev: self.rev, zoom: opts.zoom, draft: opts.draft, ppp: ctx.pixels_per_point(), fonts_rev: opts.fonts_rev };
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
        resp.context_menu(|ui| {
            ui.set_min_width(200.0);
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
                if ui.button("Add to Dictionary").clicked() {
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
            ui.separator();
            if ui.add(egui::Button::new("Select All").shortcut_text("Ctrl+A")).clicked() {
                self.select_all();
                ui.close();
            }
            if ui.button("Clear Formatting").clicked() {
                self.clear_formatting(now);
                ui.close();
            }
            ui.menu_button("Change Case", |ui| {
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
        self.layout.as_ref().map(|(_, l)| l.pages).unwrap_or(1)
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
    pub fn handle_keys(&mut self, ui: &egui::Ui, lay: &PageLayout, sc: &SmartCorrect, clip: &mut Option<(String, Fragment)>, cmds: &mut Vec<Cmd>, view_h: f32) {
        let now = ui.input(|i| i.time);
        let events = ui.input(|i| i.events.clone());
        let lines_per_view = ((view_h / (lay.ppi * 0.2)).max(3.0)) as isize;
        for ev in events {
            match ev {
                Event::Text(t) => {
                    let m = ui.input(|i| i.modifiers);
                    if !(m.ctrl && !m.alt) && !t.chars().any(|c| c.is_control()) {
                        self.type_char(&t, now, sc);
                    }
                }
                Event::Copy => {
                    if let Some(c) = self.copy() {
                        ui.ctx().copy_text(c.0.clone());
                        *clip = Some(c);
                    }
                }
                Event::Cut => {
                    if let Some(c) = self.copy() {
                        ui.ctx().copy_text(c.0.clone());
                        *clip = Some(c);
                        self.checkpoint(EditKind::Other, now);
                        self.delete_selection();
                        self.changed();
                    }
                }
                Event::Paste(s) => {
                    self.paste(&s, clip.as_ref(), now);
                }
                Event::Key { key, pressed: true, modifiers, .. } => {
                    self.key(key, modifiers, now, lay, lines_per_view, cmds);
                }
                _ => {}
            }
        }
    }

    fn key(&mut self, key: Key, m: Modifiers, now: f64, lay: &PageLayout, lpv: isize, cmds: &mut Vec<Cmd>) {
        let cmd = m.command;
        let shift = m.shift;
        if key != Key::ArrowUp && key != Key::ArrowDown && key != Key::PageUp && key != Key::PageDown {
            self.desired_x = None;
        }
        match key {
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
            Key::Backspace => self.backspace(cmd, now),
            Key::Delete => self.delete_forward(cmd, now),
            Key::Enter if cmd => self.page_break(now),
            Key::Enter => self.enter(now),
            Key::Tab if cmd => cmds.push(Cmd::NextTab),
            Key::Tab => {
                let para = &self.doc.paras[self.caret.p];
                if para.fmt.list != ListKind::None || (self.has_selection() && self.anchor.p != self.caret.p) {
                    let shift = shift;
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
            Key::A if cmd => self.select_all(),
            Key::Z if cmd && shift => self.redo(),
            Key::Z if cmd => self.undo(),
            Key::Y if cmd => self.redo(),
            Key::B if cmd => self.toggle(now, |s| s.bold, |s, v| s.bold = v),
            Key::I if cmd => self.toggle(now, |s| s.italic, |s, v| s.italic = v),
            Key::U if cmd => self.toggle(now, |s| s.underline, |s, v| s.underline = v),
            Key::L if cmd => self.apply_para(now, &|f| f.align = Align::Left),
            Key::E if cmd => self.apply_para(now, &|f| f.align = Align::Center),
            Key::R if cmd => self.apply_para(now, &|f| f.align = Align::Right),
            Key::J if cmd => self.apply_para(now, &|f| f.align = Align::Justify),
            Key::Space if cmd => self.clear_formatting(now),
            Key::F4 if shift => self.apply_char(now, &|s| s.size = (s.size - SIZE_STEP).max(MIN_SIZE)),
            Key::F4 => self.apply_char(now, &|s| s.size = (s.size + SIZE_STEP).min(MAX_SIZE)),
            Key::CloseBracket if cmd => self.apply_char(now, &|s| s.size = (s.size + 1.0).min(MAX_SIZE)),
            Key::OpenBracket if cmd => self.apply_char(now, &|s| s.size = (s.size - 1.0).max(MIN_SIZE)),
            Key::N if cmd => cmds.push(Cmd::New),
            Key::O if cmd => cmds.push(Cmd::Open),
            Key::S if cmd && shift => cmds.push(Cmd::SaveAs),
            Key::S if cmd => cmds.push(Cmd::Save),
            Key::W if cmd => cmds.push(Cmd::Close),
            Key::F if cmd => cmds.push(Cmd::Find),
            Key::H if cmd => cmds.push(Cmd::Replace),
            Key::G if cmd => cmds.push(Cmd::GoTo),
            Key::P if cmd => cmds.push(Cmd::Print),
            Key::Equals | Key::Plus if cmd => cmds.push(Cmd::ZoomIn),
            Key::Minus if cmd => cmds.push(Cmd::ZoomOut),
            Key::Num0 if cmd => cmds.push(Cmd::Zoom100),
            Key::F2 if cmd => cmds.push(Cmd::Spell),
            Key::F7 => cmds.push(Cmd::Spell),
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
        let resp = resp.on_hover_cursor(egui::CursorIcon::Text);
        self.canvas_x0 = canvas.min.x;
        self.canvas_w = canvas.width();
        let th = opts.theme;

        // mouse
        if resp.clicked() || resp.drag_started() || resp.double_clicked() || resp.triple_clicked() {
            ui.memory_mut(|m| m.request_focus(focus_id));
        }
        if let Some(pp) = resp.interact_pointer_pos() {
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
        self.context_menu(&resp, opts, cmds, ui.input(|i| i.time));

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
            // soft shadow
            for (i, a) in [(6.0, 10u8), (3.0, 18), (1.5, 28)].iter().enumerate() {
                let _ = i;
                painter.rect_filled(r.translate(vec2(0.0, a.0 * 0.6)).expand(a.0 * 0.5), 3.0, Color32::from_black_alpha(a.1));
            }
            painter.rect_filled(r, 2.0, page_bg);
            painter.rect_stroke(r, 2.0, Stroke::new(1.0, th.page_border), StrokeKind::Inside);
            let content = Rect::from_min_size(r.min + lay.content_offset, vec2(lay.content_size.x, if lay.draft { r.height() - lay.content_offset.y * 2.0 } else { lay.content_size.y }));
            if opts.margin_guides {
                // light margin guides like the original's vertical margin lines
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
                    let pn = format!("Page {}", k + 1); // string 10678 "Page %d"
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
                let x = o.x + l.xs[(caret.c - l.start).min(l.xs.len() - 1)];
                let gh = l.galley.rect.height();
                caret_rect = Some(Rect::from_min_size(pos2(x, o.y), vec2(1.0, gh)));
            }
            if lr.max.y < clip.min.y || lr.min.y > clip.max.y {
                continue;
            }
            if visible_page.is_none() && lr.center().y > clip.min.y + 40.0 {
                visible_page = Some(l.page);
            }
            let gh = l.galley.rect.height();
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
                let errs = spell_cache.entry(l.para).or_insert_with(|| self.spell_errors(sp, l.para)).clone();
                for (a, b) in errs {
                    if a < l.end && b > l.start {
                        let s = a.max(l.start) - l.start;
                        let e = b.min(l.end) - l.start;
                        squiggle(&painter, o.x + l.xs[s], o.x + l.xs[e], o.y + gh - 1.5);
                    }
                }
            }
            if let Some(m) = &l.marker {
                let mo = lay.line_origin(l, canvas.min, canvas.width());
                let content_x = mo.x - l.x;
                painter.galley(pos2(content_x + l.marker_x, o.y), m.clone(), Color32::BLACK);
            }
            if opts.show_marks && l.last_in_para {
                let x = o.x + *l.xs.last().unwrap() + 2.0;
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
        let focused = ui.memory(|m| m.has_focus(focus_id));
        if let Some(cr) = caret_rect {
            if focused {
                let t = ui.input(|i| i.time);
                let since = t - self.last_edit_time.min(t);
                let on = since < 0.6 || ((t * 1.0 / 0.53) as i64) % 2 == 0;
                if on {
                    painter.rect_filled(Rect::from_min_size(cr.min, vec2(1.6, cr.height())), 0.0, th.caret);
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(530));
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
        SmartCorrect { enabled: true, quotes: true, table: TABLE.get_or_init(crate::doc::smartcorrect_table) }
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
}

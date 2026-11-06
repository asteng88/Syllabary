//! The main window: state, actions, files, and the frame loop.

use eframe::egui::{self, vec2, Color32, Frame, Margin, RichText, Sense, Stroke};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::dialogs::{self, Dialog, FindState};
use crate::doc::{self, *};
use crate::editor::{Cmd, EditKind, SmartCorrect, Tab, ViewOpts};
use crate::fonts::{self, FontBook};
use crate::icons::{self, Icon};
use crate::layout::{self, BASE_PPI};
use crate::theme::Theme;

pub const ZOOM_MIN: f32 = 0.25; // the person's decision
pub const ZOOM_MAX: f32 = 2.0;
pub const ZOOM_START: f32 = 1.0; // "Standard (100%)"
pub const AUTOSAVE_MIN_DEFAULT: u32 = 10;
pub const APP_NAME: &str = "Scribe Pro";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZoomMode {
    Percent,
    PageWidth,
    MarginWidth,
    FullPage,
}

#[derive(Clone, Debug)]
pub enum Action {
    New,
    Open,
    OpenPath(PathBuf),
    Save,
    SaveAs,
    ExportRtf,
    ExportTxt,
    ExportDocx,
    ExportPdf,
    Print,
    LineSpacing(f32),
    FormatPainter,
    ToggleNav,
    ToggleSpell,
    GoToPara(usize),
    Close(usize),
    CloseAll,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Dialog(Dialog),
    SetZoom(f32),
    ZoomMode(ZoomMode),
    ZoomIn,
    ZoomOut,
    Draft(bool),
    ToggleDark,
    PageBreak,
    InsertText(String),
    List(ListKind),
    Bold,
    Italic,
    Underline,
    Strike,
    Script(i8),
    Grow,
    Shrink,
    ClearFormat,
    Font(u8),
    Size(f32),
    Color([u8; 3]),
    Highlight(Option<[u8; 3]>),
    Align(Align),
    Indent(i8),
    Style(usize),
    Case(CaseChange),
    SelectTab(usize),
    NextTab,
    PageEdit(PageEdit),
}

#[derive(Clone, Copy, Debug)]
pub enum PageEdit {
    Size(f32, f32),
    Swap,
    Numbers(bool),
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub dark: bool,
    pub smartcorrect: bool,
    pub smart_quotes: bool,
    pub autosave_min: u32,
    pub recent: Vec<PathBuf>,
    pub margin_guides: bool,
    pub user_corrections: Vec<(String, String)>,
    pub removed_corrections: Vec<String>,
    pub spellcheck: bool,
    pub recent_fonts: Vec<u8>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dark: false,
            smartcorrect: true,
            smart_quotes: true,
            autosave_min: AUTOSAVE_MIN_DEFAULT,
            recent: Vec::new(),
            margin_guides: true,
            user_corrections: Vec::new(),
            removed_corrections: Vec::new(),
            spellcheck: true,
            recent_fonts: Vec::new(),
        }
    }
}

fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("ScribePro"))
}
pub fn recovery_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("ScribePro").join("recovery"))
}

pub struct App {
    pub tabs: Vec<Tab>,
    pub active: usize,
    next_id: u64,
    untitled: usize,
    pub book: FontBook,
    pub zoom: f32,
    pub zoom_mode: ZoomMode,
    pub draft: bool,
    pub show_toolbar: bool,
    pub show_status: bool,
    pub show_ruler: bool,
    pub show_marks: bool,
    pub show_hf: bool,
    pub clean_screen: bool,
    pub cfg: Config,
    pub theme: Theme,
    pub sc_table: Vec<(String, String)>,
    pub dialog: Dialog,
    pub find: FindState,
    clip: Option<(String, Fragment)>,
    actions: Vec<Action>,
    screenshot: Option<PathBuf>,
    shot_done: Arc<AtomicBool>,
    frame: u64,
    toast: Option<(String, f64)>,
    last_autosave: f64,
    title: String,
    pub confirm_close: Option<usize>,
    pub confirm_quit: bool,
    force_quit: bool,
    pub doc_id: egui::Id,
    view_size: egui::Vec2,
    fonts_rev: u64,
    pub speller: crate::spell::Speller,
    pub show_nav: bool,
    pub font_search: String,
    pub demo_done: bool,
    pub painter_style: Option<(CharStyle, ParaFmt)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>, open: Vec<PathBuf>) -> Self {
        let book = fonts::install(&cc.egui_ctx);
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let cfg = if screenshot.is_some() { Config::default() } else { load_config() };
        let theme = if cfg.dark { Theme::dark() } else { Theme::light() };
        theme.apply(&cc.egui_ctx);
        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            untitled: 0,
            book,
            zoom: ZOOM_START,
            zoom_mode: ZoomMode::Percent,
            draft: false,
            show_toolbar: true,
            show_status: true,
            show_ruler: true,
            show_marks: false,
            show_hf: true,
            clean_screen: false,
            sc_table: Vec::new(),
            cfg,
            theme,
            dialog: Dialog::None,
            find: FindState::default(),
            clip: None,
            actions: Vec::new(),
            screenshot,
            shot_done: Arc::new(AtomicBool::new(false)),
            frame: 0,
            toast: None,
            last_autosave: 0.0,
            title: String::new(),
            confirm_close: None,
            confirm_quit: false,
            force_quit: false,
            doc_id: egui::Id::unique("document-canvas"),
            view_size: vec2(1200.0, 800.0),
            fonts_rev: 1,
            speller: crate::spell::Speller::new(),
            show_nav: false,
            font_search: String::new(),
            demo_done: false,
            painter_style: None,
        };
        app.rebuild_sc_table();
        for p in open {
            app.open_path(&p);
        }
        if app.tabs.is_empty() {
            app.new_doc();
        }
        if app.screenshot.is_none() {
            if let Some(dir) = recovery_dir() {
                let found: Vec<PathBuf> = std::fs::read_dir(&dir)
                    .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().map(|e| e == FILE_EXT).unwrap_or(false)).collect())
                    .unwrap_or_default();
                if !found.is_empty() {
                    app.dialog = Dialog::Recover(found);
                }
            }
        }
        cc.egui_ctx.memory_mut(|m| m.request_focus(app.doc_id));
        app
    }

    pub fn rebuild_sc_table(&mut self) {
        let mut t: Vec<(String, String)> = doc::smartcorrect_table().into_iter().filter(|(a, _)| !self.cfg.removed_corrections.contains(a)).collect();
        for (a, b) in &self.cfg.user_corrections {
            t.retain(|(x, _)| x != a);
            t.push((a.clone(), b.clone()));
        }
        self.sc_table = t;
    }

    pub fn save_config(&self) {
        if self.screenshot.is_some() {
            return;
        }
        if let Some(d) = config_dir() {
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::write(d.join("settings.json"), serde_json::to_string_pretty(&self.cfg).unwrap_or_default());
        }
    }

    pub fn tab(&mut self) -> &mut Tab {
        let i = self.active.min(self.tabs.len() - 1);
        &mut self.tabs[i]
    }
    pub fn tab_ref(&self) -> &Tab {
        &self.tabs[self.active.min(self.tabs.len() - 1)]
    }

    fn new_doc(&mut self) {
        // "Untitled" + counter in base 10, as FUN_0044e7a6 builds it → "Untitled1"
        self.untitled += 1;
        let name = format!("Untitled{}", self.untitled);
        let t = Tab::new(self.next_id, name, Document::default());
        self.next_id += 1;
        self.tabs.push(t);
        self.active = self.tabs.len() - 1;
    }

    pub fn open_path(&mut self, path: &Path) {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.toast(format!("Couldn't open {}: {e}", path.display()));
                return;
            }
        };
        let text = String::from_utf8_lossy(&bytes).to_string();
        let doc = match ext.as_str() {
            "wpx" => match doc::from_json(&text) {
                Ok(d) => d,
                Err(e) => {
                    self.toast(format!("That file isn't a readable document ({e})."));
                    return;
                }
            },
            "rtf" => doc::from_rtf(&text),
            "docx" => match crate::docx::read(&bytes) {
                Ok(d) => d,
                Err(e) => {
                    self.toast(format!("Couldn't read that Word file: {e}"));
                    return;
                }
            },
            "doc" => {
                self.toast("Old Word (.doc) files aren't supported. Save it as .docx or Rich Text first.".into());
                return;
            }
            "lwp" | "sam" | "mwp" => {
                self.toast("Files from the 1999 program use a closed format. Save them as Rich Text (RTF) there first.".into());
                return;
            }
            _ => doc::from_plain(&text),
        };
        if self.tabs.len() == 1 && !self.tabs[0].dirty && self.tabs[0].path.is_none() && self.tabs[0].doc.plain_text().is_empty() {
            self.tabs.clear();
        }
        let mut t = Tab::new(self.next_id, path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Document".into()), doc);
        self.next_id += 1;
        if ext == "wpx" || ext == "rtf" || ext == "txt" || ext == "docx" {
            t.path = Some(path.to_path_buf());
        }
        self.tabs.push(t);
        self.active = self.tabs.len() - 1;
        self.add_recent(path);
    }

    pub fn open_recovered(&mut self, path: &Path) {
        if let Ok(s) = std::fs::read_to_string(path) {
            if let Ok(d) = doc::from_json(&s) {
                self.untitled += 1;
                let mut t = Tab::new(self.next_id, format!("Recovered{}", self.untitled), d);
                self.next_id += 1;
                t.dirty = true;
                if self.tabs.len() == 1 && !self.tabs[0].dirty && self.tabs[0].doc.plain_text().is_empty() {
                    self.tabs.clear();
                }
                self.tabs.push(t);
                self.active = self.tabs.len() - 1;
            }
        }
        let _ = std::fs::remove_file(path);
    }

    fn add_recent(&mut self, path: &Path) {
        let p = path.to_path_buf();
        self.cfg.recent.retain(|r| r != &p);
        self.cfg.recent.insert(0, p);
        self.cfg.recent.truncate(8);
        self.save_config();
    }

    fn write_doc(doc: &Document, path: &Path) -> std::io::Result<()> {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let data = match ext.as_str() {
            "rtf" => doc::to_rtf(doc).into_bytes(),
            "txt" => doc.plain_text().replace('\n', "\r\n").into_bytes(),
            "docx" => crate::docx::write(doc),
            _ => doc::to_json(doc).into_bytes(),
        };
        std::fs::write(path, data)
    }

    pub fn save(&mut self, idx: usize, save_as: bool) -> bool {
        let path = if save_as || self.tabs[idx].path.is_none() {
            let base = self.tabs[idx].name.split('.').next().unwrap_or("Document").to_string();
            match rfd::FileDialog::new()
                .set_file_name(format!("{base}.{FILE_EXT}"))
                .add_filter("Scribe Pro document", &[FILE_EXT])
                .add_filter("Word document", &["docx"])
                .add_filter("Rich Text", &["rtf"])
                .add_filter("Plain text", &["txt"])
                .save_file()
            {
                Some(p) => p,
                None => return false,
            }
        } else {
            self.tabs[idx].path.clone().unwrap()
        };
        match Self::write_doc(&self.tabs[idx].doc, &path) {
            Ok(()) => {
                let t = &mut self.tabs[idx];
                t.path = Some(path.clone());
                t.name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                t.dirty = false;
                t.saved_rev = t.rev;
                let id = t.id;
                remove_recovery(id);
                self.add_recent(&path);
                self.toast(format!("Saved {}", path.display()));
                true
            }
            Err(e) => {
                self.toast(format!("Couldn't save: {e}"));
                false
            }
        }
    }

    fn export_pdf(&mut self, ctx: &egui::Context, print: bool) {
        let t = self.tab_ref();
        let base = t.name.split('.').next().unwrap_or("Document").to_string();
        let bytes = crate::pdf::export(ctx, &t.doc, &self.book);
        let path = if print {
            let dir = std::env::temp_dir().join("ScribePro");
            let _ = std::fs::create_dir_all(&dir);
            dir.join(format!("{base} (print).pdf"))
        } else {
            match rfd::FileDialog::new().set_file_name(format!("{base}.pdf")).add_filter("PDF", &["pdf"]).save_file() {
                Some(p) => p,
                None => return,
            }
        };
        match std::fs::write(&path, bytes) {
            Ok(()) if print => {
                // open in the computer's own PDF viewer, where it can be printed
                let _ = std::process::Command::new("explorer").arg(&path).spawn();
                self.toast("Opened a print-ready PDF in your PDF viewer — print from there.".into());
            }
            Ok(()) => self.toast(format!("Exported to {}", path.display())),
            Err(e) => self.toast(format!("Couldn't write the PDF: {e}")),
        }
    }

    fn export(&mut self, ext: &str) {
        let base = self.tab_ref().name.split('.').next().unwrap_or("Document").to_string();
        let filter = match ext {
            "rtf" => "Rich Text",
            "docx" => "Word document",
            _ => "Plain text",
        };
        if let Some(p) = rfd::FileDialog::new().set_file_name(format!("{base}.{ext}")).add_filter(filter, &[ext]).save_file() {
            match Self::write_doc(&self.tab_ref().doc, &p) {
                Ok(()) => self.toast(format!("Exported to {}", p.display())),
                Err(e) => self.toast(format!("Couldn't export: {e}")),
            }
        }
    }

    pub fn close_tab(&mut self, idx: usize, force: bool) {
        if idx >= self.tabs.len() {
            return;
        }
        if self.tabs[idx].dirty && !force {
            self.confirm_close = Some(idx);
            return;
        }
        remove_recovery(self.tabs[idx].id);
        self.tabs.remove(idx);
        if self.tabs.is_empty() {
            self.new_doc();
        }
        if idx < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len() - 1);
    }

    pub fn save_then_close(&mut self, idx: usize) {
        if self.save(idx, false) {
            self.close_tab(idx, true);
        }
    }

    pub fn toast(&mut self, msg: String) {
        self.toast = Some((msg, -1.0));
    }

    pub fn act(&mut self, a: Action) {
        self.actions.push(a);
    }

    pub fn effective_zoom(&self) -> f32 {
        let pg = self.tab_ref().doc.page;
        let w = (self.view_size.x - 2.0 * layout::PAGE_GAP - 16.0).max(100.0);
        let h = (self.view_size.y - 2.0 * layout::PAGE_GAP - 24.0).max(100.0);
        let z = match self.zoom_mode {
            ZoomMode::Percent => self.zoom,
            ZoomMode::PageWidth => w / (pg.width * BASE_PPI),
            ZoomMode::MarginWidth => w / ((pg.width - pg.margin_left - pg.margin_right + 0.4) * BASE_PPI),
            ZoomMode::FullPage => (w / (pg.width * BASE_PPI)).min(h / (pg.height * BASE_PPI)),
        };
        (z.clamp(ZOOM_MIN, ZOOM_MAX) * 100.0).round() / 100.0
    }

    pub fn force_quit(&mut self, ctx: &egui::Context) {
        self.force_quit = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn run_actions(&mut self, ctx: &egui::Context) {
        let actions = std::mem::take(&mut self.actions);
        let now = ctx.input(|i| i.time);
        for a in actions {
            let mut refocus = true;
            match a {
                Action::New => self.new_doc(),
                Action::Open => {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Documents", &[FILE_EXT, "docx", "rtf", "txt"]).add_filter("All files", &["*"]).pick_file() {
                        self.open_path(&p);
                    }
                }
                Action::OpenPath(p) => self.open_path(&p),
                Action::Save => {
                    let i = self.active;
                    self.save(i, false);
                }
                Action::SaveAs => {
                    let i = self.active;
                    self.save(i, true);
                }
                Action::ExportRtf => self.export("rtf"),
                Action::ExportTxt => self.export("txt"),
                Action::ExportDocx => self.export("docx"),
                Action::ExportPdf => self.export_pdf(ctx, false),
                Action::Print => self.export_pdf(ctx, true),
                Action::LineSpacing(ls) => self.tab().apply_para(now, &move |f| f.line_spacing = ls),
                Action::FormatPainter => {
                    if self.painter_style.is_some() {
                        self.painter_style = None;
                    } else {
                        let t = self.tab_ref();
                        let st = t.typing_style();
                        let f = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                        self.painter_style = Some((st, f));
                        self.toast("Format painter: select the text to paint.".into());
                    }
                }
                Action::ToggleNav => self.show_nav = !self.show_nav,
                Action::ToggleSpell => {
                    self.cfg.spellcheck = !self.cfg.spellcheck;
                    self.save_config();
                }
                Action::GoToPara(p) => {
                    let t = self.tab();
                    t.set_caret(Pos::new(p, 0), false);
                }
                Action::Close(i) => self.close_tab(i, false),
                Action::CloseAll => {
                    for i in (0..self.tabs.len()).rev() {
                        if !self.tabs[i].dirty {
                            self.close_tab(i, true);
                        }
                    }
                    if let Some(i) = self.tabs.iter().position(|t| t.dirty) {
                        self.active = i;
                        self.confirm_close = Some(i);
                    }
                }
                Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Action::Undo => self.tab().undo(),
                Action::Redo => self.tab().redo(),
                Action::Cut | Action::Copy => {
                    let cut = matches!(a, Action::Cut);
                    let t = self.tab();
                    if let Some(c) = t.copy() {
                        ctx.copy_text(c.0.clone());
                        if cut {
                            t.checkpoint(EditKind::Other, now);
                            t.delete_selection();
                            t.changed();
                        }
                        self.clip = Some(c);
                    }
                }
                Action::Paste => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
                Action::SelectAll => self.tab().select_all(),
                Action::Dialog(d) => {
                    if let Dialog::Find(replace) = d {
                        let sel = self.tab_ref().selected_text();
                        if !sel.is_empty() && !sel.contains('\n') && sel.chars().count() < 80 {
                            self.find.query = sel;
                        }
                        self.find.focus = true;
                        self.find.show_replace = replace;
                    }
                    self.dialog = d;
                    refocus = false;
                }
                Action::SetZoom(z) => {
                    self.zoom_mode = ZoomMode::Percent;
                    self.zoom = z.clamp(ZOOM_MIN, ZOOM_MAX);
                }
                Action::ZoomMode(m) => self.zoom_mode = m,
                Action::ZoomIn | Action::ZoomOut => {
                    let z = self.effective_zoom();
                    let steps = [0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];
                    let nz = if matches!(a, Action::ZoomIn) {
                        steps.iter().copied().find(|s| *s > z + 0.001).unwrap_or(ZOOM_MAX)
                    } else {
                        steps.iter().rev().copied().find(|s| *s < z - 0.001).unwrap_or(ZOOM_MIN)
                    };
                    self.zoom_mode = ZoomMode::Percent;
                    self.zoom = nz;
                }
                Action::Draft(d) => self.draft = d,
                Action::ToggleDark => {
                    self.cfg.dark = !self.cfg.dark;
                    self.theme = if self.cfg.dark { Theme::dark() } else { Theme::light() };
                    self.theme.apply(ctx);
                    self.save_config();
                }
                Action::PageBreak => self.tab().page_break(now),
                Action::InsertText(s) => self.tab().insert(&s, now),
                Action::List(k) => {
                    let t = self.tab();
                    let cur = t.doc.paras[t.doc.clamp(t.caret).p].fmt.list;
                    let nk = if cur == k { ListKind::None } else { k };
                    t.apply_para(now, &move |f| f.list = nk);
                }
                Action::Bold => self.tab().toggle(now, |s| s.bold, |s, v| s.bold = v),
                Action::Italic => self.tab().toggle(now, |s| s.italic, |s, v| s.italic = v),
                Action::Underline => self.tab().toggle(now, |s| s.underline, |s, v| s.underline = v),
                Action::Strike => self.tab().toggle(now, |s| s.strike, |s, v| s.strike = v),
                Action::Script(k) => {
                    let cur = self.tab_ref().typing_style().script;
                    let nk = if cur == k { 0 } else { k };
                    self.tab().apply_char(now, &move |s| s.script = nk);
                }
                Action::Grow => self.tab().apply_char(now, &|s| s.size = (s.size + SIZE_STEP).min(MAX_SIZE)),
                Action::Shrink => self.tab().apply_char(now, &|s| s.size = (s.size - SIZE_STEP).max(MIN_SIZE)),
                Action::ClearFormat => self.tab().clear_formatting(now),
                Action::Font(f) => {
                    self.tab().apply_char(now, &move |s| s.font = f);
                    self.cfg.recent_fonts.retain(|x| *x != f);
                    self.cfg.recent_fonts.insert(0, f);
                    self.cfg.recent_fonts.truncate(4);
                    self.save_config();
                }
                Action::Size(z) => self.tab().apply_char(now, &move |s| s.size = z.clamp(MIN_SIZE, MAX_SIZE)),
                Action::Color(c) => self.tab().apply_char(now, &move |s| s.color = c),
                Action::Highlight(h) => self.tab().apply_char(now, &move |s| s.highlight = h),
                Action::Align(al) => self.tab().apply_para(now, &move |f| f.align = al),
                Action::Indent(d) => self.tab().apply_para(now, &move |f| f.indent = (f.indent as i8 + d).clamp(0, 8) as u8),
                Action::Style(i) => self.tab().apply_named_style(i, now),
                Action::Case(k) => self.tab().change_case(k, now),
                Action::SelectTab(i) => self.active = i.min(self.tabs.len() - 1),
                Action::NextTab => self.active = (self.active + 1) % self.tabs.len(),
                Action::PageEdit(pe) => {
                    let t = self.tab();
                    t.checkpoint(EditKind::Other, now);
                    match pe {
                        PageEdit::Size(w, h) => {
                            t.doc.page.width = w;
                            t.doc.page.height = h;
                        }
                        PageEdit::Swap => {
                            let pg = &mut t.doc.page;
                            std::mem::swap(&mut pg.width, &mut pg.height);
                        }
                        PageEdit::Numbers(on) => t.doc.page.page_numbers = on,
                    }
                    t.changed();
                    t.scroll_to_caret = false;
                }
            }
            if refocus {
                ctx.memory_mut(|m| m.request_focus(self.doc_id));
            }
        }
    }

    fn handle_cmds(&mut self, cmds: Vec<Cmd>) {
        for c in cmds {
            let a = match c {
                Cmd::New => Action::New,
                Cmd::Open => Action::Open,
                Cmd::Save => Action::Save,
                Cmd::SaveAs => Action::SaveAs,
                Cmd::Close => Action::Close(self.active),
                Cmd::Find => Action::Dialog(Dialog::Find(false)),
                Cmd::Replace => Action::Dialog(Dialog::Find(true)),
                Cmd::GoTo => Action::Dialog(Dialog::GoTo(self.tab_ref().current_page + 1)),
                Cmd::Print => Action::Print,
                Cmd::Spell => Action::Dialog(Dialog::Spell(0)),
                Cmd::Cut => Action::Cut,
                Cmd::Copy => Action::Copy,
                Cmd::Paste => Action::Paste,
                Cmd::NextTab => Action::NextTab,
                Cmd::ZoomIn => Action::ZoomIn,
                Cmd::ZoomOut => Action::ZoomOut,
                Cmd::Zoom100 => Action::SetZoom(1.0),
            };
            self.actions.push(a);
        }
    }

    fn autosave(&mut self, now: f64) {
        if self.screenshot.is_some() || self.cfg.autosave_min == 0 {
            return;
        }
        if self.last_autosave == 0.0 {
            self.last_autosave = now.max(0.001);
            return;
        }
        if now - self.last_autosave < self.cfg.autosave_min as f64 * 60.0 {
            return;
        }
        self.last_autosave = now;
        let Some(dir) = recovery_dir() else { return };
        let _ = std::fs::create_dir_all(&dir);
        for t in &self.tabs {
            if t.dirty {
                let _ = std::fs::write(dir.join(format!("recovery-{}.{FILE_EXT}", t.id)), doc::to_json(&t.doc));
            }
        }
    }
}

fn load_config() -> Config {
    config_dir().and_then(|d| std::fs::read_to_string(d.join("settings.json")).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn remove_recovery(id: u64) {
    if let Some(d) = recovery_dir() {
        let _ = std::fs::remove_file(d.join(format!("recovery-{id}.{FILE_EXT}")));
    }
}

// ───────────────────────── small widgets ─────────────────────────

pub fn icon_button(ui: &mut egui::Ui, th: &Theme, icon: Icon, tip: &str, selected: bool, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let bg = if selected {
        th.active
    } else if resp.hovered() && enabled {
        th.hover
    } else {
        Color32::TRANSPARENT
    };
    p.rect_filled(rect, 6.0, bg);
    let col = if !enabled {
        th.icon.gamma_multiply(0.35)
    } else if selected {
        th.accent
    } else {
        th.icon
    };
    let acc = if enabled { th.accent } else { th.accent.gamma_multiply(0.35) };
    icons::draw(p, rect.shrink(5.0), icon, col, acc);
    resp.on_hover_text(tip)
}

pub fn sep(ui: &mut egui::Ui, th: &Theme) {
    let (r, _) = ui.allocate_exact_size(vec2(7.0, 26.0), Sense::hover());
    ui.painter().line_segment([r.center_top() + vec2(0.0, 4.0), r.center_bottom() - vec2(0.0, 4.0)], Stroke::new(1.0, th.border));
}

pub const PALETTE: &[[u8; 3]] = &[
    [0, 0, 0],
    [55, 65, 81],
    [107, 114, 128],
    [156, 163, 175],
    [255, 255, 255],
    [185, 28, 28],
    [220, 38, 38],
    [234, 88, 12],
    [217, 119, 6],
    [202, 138, 4],
    [22, 163, 74],
    [21, 128, 61],
    [13, 148, 136],
    [8, 145, 178],
    [37, 99, 235],
    [29, 78, 216],
    [79, 70, 229],
    [124, 58, 237],
    [192, 38, 211],
    [219, 39, 119],
];
pub const HIGHLIGHTS: &[[u8; 3]] = &[[254, 240, 138], [187, 247, 208], [191, 219, 254], [251, 207, 232], [254, 215, 170], [221, 214, 254], [229, 231, 235]];

pub fn swatch(ui: &mut egui::Ui, c: [u8; 3], selected: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
    ui.painter().rect_filled(r.shrink(1.0), 4.0, layout::color(c));
    let st = if selected || resp.hovered() { Stroke::new(2.0, Color32::from_rgb(37, 99, 235)) } else { Stroke::new(1.0, Color32::from_gray(200)) };
    ui.painter().rect_stroke(r.shrink(1.0), 4.0, st, egui::StrokeKind::Inside);
    resp
}

// ───────────────────────── frame loop ─────────────────────────

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frame += 1;
        let now = ctx.input(|i| i.time);

        // screenshot mode: draw ~120 frames, save the picture, exit
        if let Some(path) = self.screenshot.clone() {
            ctx.request_repaint();
            if self.frame == 120 {
                let done = self.shot_done.clone();
                ctx.request_screenshot(move |img| {
                    let [w, h] = img.size;
                    let mut buf = Vec::with_capacity(w * h * 4);
                    for p in &img.pixels {
                        buf.extend_from_slice(&[p.r(), p.g(), p.b(), 255]);
                    }
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Some(im) = image::RgbaImage::from_raw(w as u32, h as u32, buf) {
                        let _ = im.save(&path);
                    }
                    done.store(true, Ordering::SeqCst);
                });
            }
            if self.shot_done.load(Ordering::SeqCst) || self.frame > 900 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // closing the window with unsaved work
        if ctx.input(|i| i.viewport().close_requested()) && !self.force_quit && self.tabs.iter().any(|t| t.dirty) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_quit = true;
        }

        // window title, like "Word Pro - [Untitled1]"
        let title = format!("{APP_NAME} - [{}]", self.tab_ref().title());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        let th = self.theme.clone();

        egui::Panel::top("menubar").frame(Frame::new().fill(th.chrome).inner_margin(Margin::symmetric(6, 2))).show(ui, |ui| {
            let r = egui::MenuBar::new().ui(ui, |ui| {
                self.menus(ui);
            });
            // the original's menu bar is named "Application"
            ui.interact(r.response.rect, egui::Id::unique("menubar-a11y"), Sense::hover()).widget_info(|| egui::WidgetInfo::labeled(egui::Role::MenuBar, true, "Application"));
        });

        if self.show_toolbar && !self.clean_screen {
            egui::Panel::top("toolbar")
                .frame(Frame::new().fill(th.toolbar).inner_margin(Margin::symmetric(8, 4)).stroke(Stroke::new(1.0, th.border)))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        self.toolbar(ui, &th);
                    });
                });
        }

        if !self.clean_screen {
            egui::Panel::top("tabs").frame(Frame::new().fill(th.chrome).inner_margin(Margin { left: 8, right: 8, top: 4, bottom: 0 })).show(ui, |ui| {
                self.tab_strip(ui, &th);
            });
        }

        if self.show_status && !self.clean_screen {
            egui::Panel::bottom("status").frame(Frame::new().fill(th.chrome).inner_margin(Margin::symmetric(10, 3)).stroke(Stroke::new(1.0, th.border))).show(ui, |ui| {
                self.status_bar(ui, &th);
            });
        }

        if self.show_nav && !self.clean_screen {
            egui::Panel::left("nav")
                .resizable(true)
                .default_size(230.0)
                .frame(Frame::new().fill(th.chrome).inner_margin(Margin::symmetric(10, 8)).stroke(Stroke::new(1.0, th.border)))
                .show(ui, |ui| {
                    self.nav_pane(ui, &th);
                });
        }

        let mut cmds = Vec::new();
        egui::CentralPanel::default().frame(Frame::new().fill(th.workspace)).show(ui, |ui| {
            let full = ui.available_rect_before_wrap();
            self.view_size = full.size();
            let zoom = self.effective_zoom();
            if self.show_ruler && !self.clean_screen && !self.draft {
                self.ruler(ui, &th, zoom);
            }
            // ctrl + wheel zooms the page
            let zd = ui.input(|i| i.zoom_delta());
            if zd != 1.0 && ui.rect_contains_pointer(full) {
                self.zoom_mode = ZoomMode::Percent;
                self.zoom = (zoom * zd).clamp(ZOOM_MIN, ZOOM_MAX);
            }
            let zoom = self.effective_zoom();
            let doc_id = self.doc_id;
            let book = self.book.clone();
            let active = self.active;
            let matches = if matches!(self.dialog, Dialog::Find(_)) { self.find.matches(&self.tabs[active].doc, self.tabs[active].rev) } else { Vec::new() };
            let table = std::mem::take(&mut self.sc_table);
            let sc = SmartCorrect { enabled: self.cfg.smartcorrect, quotes: self.cfg.smart_quotes, table: &table };
            let focused = ui.memory(|m| m.focused());
            let doc_has_keys = (focused.is_none() || focused == Some(doc_id)) && self.confirm_close.is_none() && !self.confirm_quit;
            let view_h = ui.available_height();
            let tab_id = self.tabs[active].id;
            let mut clip = self.clip.take();
            {
                let opts = ViewOpts {
                    zoom,
                    draft: self.draft,
                    show_marks: self.show_marks,
                    margin_guides: self.cfg.margin_guides,
                    header_footer: self.show_hf,
                    matches: &matches,
                    theme: &th,
                    book: &book,
                    fonts_rev: self.fonts_rev,
                    speller: if self.cfg.spellcheck && self.speller.available() { Some(&self.speller) } else { None },
                };
                let tab = &mut self.tabs[active];
                if doc_has_keys {
                    let lay = tab.layout(&ctx, &opts);
                    tab.handle_keys(ui, &lay, &sc, &mut clip, &mut cmds, view_h);
                }
                use crate::scrollbar::{scrollbar, SIZE};
                let area = ui.available_rect_before_wrap();
                // the original's MDI client pane is called "Workspace"
                ui.interact(area, egui::Id::unique("workspace"), Sense::hover()).widget_info(|| egui::WidgetInfo::labeled(egui::Role::Pane, true, "Workspace"));
                let doc_rect = egui::Rect::from_min_max(area.min, area.max - vec2(SIZE, SIZE));
                let vbar = egui::Rect::from_min_max(egui::pos2(doc_rect.max.x, area.min.y), egui::pos2(area.max.x, doc_rect.max.y));
                let hbar = egui::Rect::from_min_max(egui::pos2(area.min.x, doc_rect.max.y), egui::pos2(doc_rect.max.x, area.max.y));
                let corner = egui::Rect::from_min_max(doc_rect.max, area.max);
                let mut sa = egui::ScrollArea::both()
                    .id_salt(("doc", tab_id))
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
                if let Some(o) = tab.pending_scroll.take() {
                    sa = sa.scroll_offset(o);
                }
                let out = ui.scope_builder(egui::UiBuilder::new().max_rect(doc_rect), |ui| sa.show(ui, |ui| tab.show(ui, &opts, doc_id, &mut cmds))).inner;
                let off = out.state.clamped_offset();
                let (content, view) = (out.content_size, out.inner_rect.size());
                let sid = egui::Id::unique(("sb", tab_id));
                if let Some(y) = scrollbar(ui, vbar, true, off.y, content.y, view.y, &th, sid.with("v")) {
                    tab.pending_scroll = Some(vec2(off.x, y));
                    ctx.request_repaint();
                }
                if let Some(x) = scrollbar(ui, hbar, false, off.x, content.x, view.x, &th, sid.with("h")) {
                    tab.pending_scroll = Some(vec2(x, off.y));
                    ctx.request_repaint();
                }
                ui.painter().rect_filled(corner, 0.0, th.chrome);
                // format painter: paint the copied look onto the next selection made with the mouse
                if let Some((st, f)) = self.painter_style {
                    if ui.input(|i| i.pointer.primary_released()) && tab.has_selection() {
                        tab.checkpoint(EditKind::Other, ui.input(|i| i.time));
                        let (a, b) = tab.selection();
                        tab.doc.restyle(a, b, &move |s| *s = st);
                        for p in tab.doc.para_range(a, b) {
                            let keep = tab.doc.paras[p].fmt.page_break_before;
                            tab.doc.paras[p].fmt = ParaFmt { page_break_before: keep, ..f };
                        }
                        tab.changed();
                        tab.scroll_to_caret = false;
                        self.painter_style = None;
                    }
                }
            }
            self.clip = clip;
            self.sc_table = table;
            if doc_has_keys && focused.is_none() {
                ui.memory_mut(|m| m.request_focus(doc_id));
            }
        });
        let doc_id = self.doc_id;
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(doc_id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false });
        });
        self.handle_cmds(cmds);

        dialogs::show(self, &ctx);

        if let Some((msg, t0)) = self.toast.clone() {
            let t0 = if t0 < 0.0 { now } else { t0 };
            self.toast = Some((msg.clone(), t0));
            if now - t0 > 4.0 {
                self.toast = None;
            } else {
                egui::Area::new(egui::Id::unique("toast")).anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -48.0)).show(&ctx, |ui| {
                    Frame::new().fill(Color32::from_rgb(31, 41, 55)).corner_radius(8).inner_margin(Margin::symmetric(14, 8)).show(ui, |ui| {
                        ui.label(RichText::new(msg).color(Color32::WHITE));
                    });
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }

        self.run_actions(&ctx);
        self.autosave(now);
    }

    fn on_exit(&mut self) {
        if self.screenshot.is_none() {
            for t in &self.tabs {
                remove_recovery(t.id);
            }
        }
    }
}

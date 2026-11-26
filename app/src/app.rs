//! The main window: state, actions, files, Google Docs modes, smart chips, building blocks, and the frame loop.

use eframe::egui::{self, pos2, vec2, Color32, Frame, Margin, RichText, Sense, Stroke};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::dialogs::{self, Dialog, FindState};
use crate::doc::{self, *};
use crate::editor::{Cmd, DocMode, EditKind, SmartCorrect, Tab, ViewOpts};
use crate::fonts::{self, FontBook};
use crate::icons::{self, Icon};
use crate::layout::{self, BASE_PPI};
use crate::theme::Theme;

pub const ZOOM_MIN: f32 = 0.25; // the person's decision
pub const ZOOM_MAX: f32 = 2.0;
pub const ZOOM_START: f32 = 1.0; // "Standard (100%)"
pub const AUTOSAVE_MIN_DEFAULT: u32 = 10;
pub const APP_NAME: &str = "Syllabary";

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
    MakeCopy,
    ExportRtf,
    ExportTxt,
    ExportDocx,
    ExportOdt,
    ExportGdoc,
    ExportPdf,
    ExportMd,
    ExportHtml,
    Print,
    LineSpacing(f32),
    SpaceBefore(f32),
    SpaceAfter(f32),
    FormatPainter,
    ToggleNav,
    ToggleComments,
    ToggleHistory,
    OpenNameVersion,
    ToggleSpell,
    TogglePageless,
    ToggleStar,
    ToggleCompact,
    ToggleLiveWordCount,
    SetMode(DocMode),
    GoToPara(usize),
    Close(usize),
    CloseAll,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PastePlain,
    DuplicatePara,
    SelectAll,
    Dialog(Dialog),
    SetZoom(f32),
    ZoomMode(ZoomMode),
    ZoomIn,
    ZoomOut,
    Draft(bool),
    ToggleDark,
    CheckForUpdates,
    PageBreak,
    HorizontalRule,
    InsertText(String),
    SystemEmojiPicker,
    InsertFootnote,
    InsertToc,
    InsertBuildingBlock(&'static str),
    InsertSmartChip(&'static str),
    InsertImage,
    #[allow(dead_code)]
    InsertImagePath(PathBuf),
    ReplaceImage(u64),
    InsertLink { text: String, url: String },
    RemoveLink(Pos),
    OpenLink(String),
    AddComment(String),
    ResolveComment(u64),
    AcceptSuggestion(u64),
    RejectSuggestion(u64),
    DeleteComment(u64),
    List(ListKind),
    BulletPreset(ListKind, u8),
    ParaShading(Option<[u8; 3]>),
    Bold,
    Italic,
    Underline,
    Strike,
    Script(i8),
    Grow,
    Shrink,
    StepFontSize(f32),
    ClearFormat,
    Font(u16),
    Size(f32),
    Color([u8; 3]),
    ColorGradient([u8; 3], [u8; 3]),
    Highlight(Option<[u8; 3]>),
    HighlightGradient([u8; 3], [u8; 3]),
    Align(Align),
    Indent(i8),
    Style(usize),
    Case(CaseChange),
    SelectTab(usize),
    NextTab,
    PageEdit(PageEdit),
    InsertTable {
        rows: usize,
        cols: usize,
        header_shading: Option<[u8; 3]>,
    },
    TableInsertRow {
        above: bool,
    },
    TableDeleteRow,
    TableInsertCol {
        left: bool,
    },
    TableDeleteCol,
    TableMergeRight,
    TableMergeRow,
    TableSplitCell,
    TableCellShading(Option<[u8; 3]>),
    TableRowShading(Option<[u8; 3]>),
    TableCellVAlign(CellVAlign),
    TableCellHAlign(Align),
    TableToggleWrap,
    TableSelectRow,
    TableSelectCol,
    TableAutoFitCol,
    TableAutoFitRow,
    TableCellFormula(Option<String>),
    OpenTableMath,
    SetPdfMode(crate::editor::PdfMode),
    OpenHeadersFooters,
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
    pub markdown_auto: bool,
    pub auto_links: bool,
    pub live_word_count: bool,
    pub autosave_min: u32,
    pub recent: Vec<PathBuf>,
    pub margin_guides: bool,
    pub block_cursor: bool,
    pub user_corrections: Vec<(String, String)>,
    pub removed_corrections: Vec<String>,
    pub spellcheck: bool,
    pub recent_fonts: Vec<u16>,
    pub shortcut_preset: ShortcutPreset,
    pub assoc_extensions: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dark: false,
            smartcorrect: true,
            smart_quotes: true,
            markdown_auto: true,
            auto_links: true,
            live_word_count: false,
            autosave_min: AUTOSAVE_MIN_DEFAULT,
            recent: Vec::new(),
            margin_guides: true,
            block_cursor: false,
            user_corrections: Vec::new(),
            removed_corrections: Vec::new(),
            spellcheck: true,
            recent_fonts: Vec::new(),
            shortcut_preset: ShortcutPreset::default(),
            assoc_extensions: crate::file_assoc::default_selected_extensions(),
        }
    }
}

fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("Syllabary"))
}
pub fn recovery_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Syllabary").join("recovery"))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UpdatePhase {
    #[default]
    Idle,
    Prompt,
    Downloading,
    ReadyToRestart(String),
    Error(String),
}

#[derive(Default)]
pub struct UpdateState {
    pub check_rx: Option<(bool, std::sync::mpsc::Receiver<crate::updater::CheckOutcome>)>,
    pub progress: Arc<std::sync::Mutex<(u64, u64)>>,
    pub cancel: Arc<AtomicBool>,
    pub download_rx: Option<std::sync::mpsc::Receiver<Result<PathBuf, String>>>,
    pub release: Option<crate::updater::ReleaseInfo>,
    pub phase: UpdatePhase,
}

/// Explicit close/quit confirmation lifecycle (replaces `confirm_close`, `confirm_quit`, `force_quit`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CloseFlow {
    #[default]
    None,
    ConfirmTab(usize),
    ConfirmQuit,
    ForceQuit,
}

impl CloseFlow {
    pub fn is_confirming(self) -> bool {
        matches!(self, CloseFlow::ConfirmTab(_) | CloseFlow::ConfirmQuit)
    }
}

/// Background document-loading result delivered back to the UI thread.
pub struct LoadedDocument {
    pub path: PathBuf,
    pub ext: String,
    pub doc: Document,
    pub pdf_view: Option<(Arc<Vec<u8>>, Vec<(f32, f32)>, Vec<crate::pdf_import::PdfTextBlock>)>,
}

/// Progress or completion message sent by the background document loader thread.
pub enum DocLoadMsg {
    Progress { frac: f32, status: String },
    Done(Result<LoadedDocument, String>),
}

/// Live state of an in-progress background document load so the UI stays 100% responsive.
pub struct DocLoadJob {
    pub file_name: String,
    pub progress: f32,
    pub status: String,
    pub started_at: std::time::Instant,
    pub show_immediately: bool,
    pub rx: std::sync::mpsc::Receiver<DocLoadMsg>,
}

/// Explicit mutually-exclusive right-hand studio inspector drawer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RightPanel {
    #[default]
    None,
    Comments,
    VersionHistory,
    HeadersFooters,
    PageSetup,
    Settings,
}

impl RightPanel {
    pub fn toggle(&mut self, target: RightPanel) {
        *self = if *self == target { RightPanel::None } else { target };
    }
}

pub struct App {
    pub egui_ctx: egui::Context,
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
    pub compact_mode: bool,
    pub cfg: Config,
    pub theme: Theme,
    pub sc_table: Vec<(String, String)>,
    pub dialog: Dialog,
    pub find: FindState,
    clip: Option<(String, Fragment)>,
    pub paste_plain_next: bool,
    actions: Vec<Action>,
    screenshot: Option<PathBuf>,
    shot_done: Arc<AtomicBool>,
    frame: u64,
    pub toasts: modernrust::ToastStack,
    last_autosave: f64,
    title: String,
    pub close_flow: CloseFlow,
    pub doc_id: egui::Id,
    view_size: egui::Vec2,
    fonts_rev: u64,
    pub speller: crate::spell::Speller,
    pub show_nav: bool,
    pub right_panel: RightPanel,
    pub history_sel_idx: usize,
    pub history_ver_name: String,
    pub mode: DocMode,
    pub font_search: String,
    pub demo_done: bool,
    pub painter_style: Option<(CharStyle, ParaFmt)>,
    pub editing_title: bool,
    pub comment_reply_draft: (u64, String),
    pub wc_metric: u8,
    pub table_panel_collapsed: bool,
    pub pdf_panel_collapsed: bool,
    pub update: UpdateState,
    pub assoc_status_msg: Option<(bool, String)>,
    pub doc_load: Option<DocLoadJob>,
    pub doc_load_queue: Vec<PathBuf>,
    pub splash: crate::splash::SplashState,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>, open: Vec<PathBuf>) -> Self {
        let book = fonts::install(&cc.egui_ctx);
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let mut cfg = if screenshot.is_some() { Config::default() } else { load_config() };
        if std::env::var("SYLLABARY_DARK").is_ok() || std::env::var("SCRIBE_DARK").is_ok() {
            cfg.dark = true;
        }
        let theme = if cfg.dark { Theme::dark() } else { Theme::light() };
        theme.apply(&cc.egui_ctx);
        let show_splash = (screenshot.is_none() && std::env::var("SYLLABARY_NO_SPLASH").is_err())
            || std::env::var("SYLLABARY_SHOW_SPLASH").is_ok();
        let mut app = Self {
            egui_ctx: cc.egui_ctx.clone(),
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
            compact_mode: false,
            sc_table: Vec::new(),
            cfg,
            theme,
            dialog: Dialog::None,
            find: FindState::default(),
            clip: None,
            paste_plain_next: false,
            actions: Vec::new(),
            screenshot,
            shot_done: Arc::new(AtomicBool::new(false)),
            frame: 0,
            toasts: modernrust::ToastStack::default(),
            last_autosave: 0.0,
            title: String::new(),
            close_flow: CloseFlow::None,
            doc_id: egui::Id::unique("document-canvas"),
            view_size: vec2(1200.0, 800.0),
            fonts_rev: 1,
            speller: crate::spell::Speller::new(),
            show_nav: false,
            right_panel: RightPanel::None,
            history_sel_idx: 0,
            history_ver_name: String::new(),
            mode: DocMode::Editing,
            font_search: String::new(),
            demo_done: false,
            painter_style: None,
            editing_title: false,
            comment_reply_draft: (0, String::new()),
            wc_metric: 0,
            table_panel_collapsed: false,
            pdf_panel_collapsed: false,
            update: UpdateState::default(),
            assoc_status_msg: None,
            doc_load: None,
            doc_load_queue: Vec::new(),
            splash: crate::splash::SplashState::new(show_splash),
        };
        crate::updater::cleanup_old_executable_backup();
        if app.screenshot.is_none()
            && std::env::var("SYLLABARY_NO_UPDATE_CHECK").is_err()
        {
            app.start_update_check(false);
        }
        app.rebuild_sc_table();
        if app.screenshot.is_some() {
            for p in open {
                app.open_path(&p);
            }
        } else {
            for p in open {
                app.open_path_async(&p);
            }
        }
        if app.tabs.is_empty() {
            app.new_doc();
        }
        if std::env::var("SYLLABARY_DEMO_DOC").is_ok() || std::env::var("SCRIBE_DEMO_DOC").is_ok() {
            app.show_nav = true;
            let t = app.tab();
            t.name = "Product Architecture Brief".to_string();
            t.doc.starred = true;
            t.doc.summary = "Editorial specification & visual language overview for Syllabary.".to_string();
            t.doc.paras.clear();
            let push_str = |p: &mut Para, s: &str, st: CharStyle| {
                for ch in s.chars() {
                    p.text.push(ch);
                    p.styles.push(st);
                }
            };
            let s0 = CharStyle { font: DEFAULT_FONT, size: 22.0, bold: true, color: [28, 35, 51], ..CharStyle::default() };
            let mut p0 = Para::new(s0, ParaFmt { style: 2, space_after: 6.0, ..ParaFmt::default() });
            push_str(&mut p0, "Syllabary — Editorial Studio", s0);

            let s1 = CharStyle { font: DEFAULT_FONT, size: 12.0, color: [71, 85, 105], ..CharStyle::default() };
            let mut p1 = Para::new(s1, ParaFmt { space_after: 10.0, ..ParaFmt::default() });
            push_str(&mut p1, "Crafted for focus, typographic precision, and effortless offline writing.", s1);

            let s2 = CharStyle { font: DEFAULT_FONT, size: 13.0, ..CharStyle::default() };
            let mut p2 = Para::new(s2, ParaFmt { space_after: 10.0, ..ParaFmt::default() });
            push_str(&mut p2, "Lead Designer: ", s2);
            let pill_style = CharStyle {
                font: DEFAULT_FONT,
                size: 13.0,
                bold: true,
                color: [234, 88, 12],
                color_to: Some([219, 39, 119]),
                highlight: Some([254, 240, 138]),
                highlight_to: Some([254, 202, 202]),
                ..CharStyle::default()
            };
            push_str(&mut p2, "Andrew Thomas", pill_style);

            let s3 = CharStyle { font: DEFAULT_FONT, size: 16.0, bold: true, ..CharStyle::default() };
            let mut p3 = Para::new(s3, ParaFmt { style: 4, space_before: 6.0, space_after: 4.0, ..ParaFmt::default() });
            push_str(&mut p3, "1. Visual Architecture", s3);

            let s_body = CharStyle { font: DEFAULT_FONT, size: 12.0, ..CharStyle::default() };
            let mut p4 = Para::new(s_body, ParaFmt { space_after: 8.0, ..ParaFmt::default() });
            push_str(
                &mut p4,
                "A unified studio header and floating formatting island replace stacked legacy toolbars, keeping focus on your writing.",
                s_body,
            );

            let s_h2 = CharStyle { font: DEFAULT_FONT, size: 14.0, bold: true, ..CharStyle::default() };
            let mut p5 = Para::new(s_h2, ParaFmt { style: 5, space_before: 4.0, space_after: 4.0, ..ParaFmt::default() });
            push_str(&mut p5, "2. Studio Capabilities", s_h2);

            let mut p6 = Para::new(s_body, ParaFmt { list: ListKind::Checklist, checked: true, ..ParaFmt::default() });
            push_str(&mut p6, "Unified studio header with sculpted menu capsule & command search", s_body);

            let mut p7 = Para::new(s_body, ParaFmt { list: ListKind::Checklist, checked: false, ..ParaFmt::default() });
            push_str(&mut p7, "Handcrafted duotone vector icons across Light and Dark studio themes", s_body);

            t.doc.paras.extend([p0, p1, p2, p3, p4, p5, p6, p7]);
            t.dirty = false;
        }
        if std::env::var("SYLLABARY_DESIGN_SHEET").is_ok() || std::env::var("SCRIBE_DESIGN_SHEET").is_ok() {
            app.dialog = Dialog::DesignSheet;
        }
        if app.screenshot.is_none() {
            if let Some(dir) = recovery_dir() {
                let found: Vec<PathBuf> = std::fs::read_dir(&dir)
                    .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == FILE_EXT)).collect())
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
        let text = doc::decode_text(&bytes);
        let doc = match ext.as_str() {
            "wpx" => match doc::from_json(&text) {
                Ok(d) => d,
                Err(e) => {
                    self.toast(format!("That file isn't a readable document ({e})."));
                    return;
                }
            },
            "gdoc" => match doc::from_gdoc(&text) {
                Ok(d) => d,
                Err(e) => {
                    self.toast(format!("Couldn't read that Google Docs file: {e}"));
                    return;
                }
            },
            // RTF is 7-bit with 8-bit escapes and raw \bin data: keep every byte as-is
            "rtf" => doc::from_rtf(&bytes.iter().map(|&b| b as char).collect::<String>()),
            "md" | "markdown" => doc::from_markdown(&text),
            "docx" => match crate::docx::read(&bytes) {
                Ok(d) => d,
                Err(e) => {
                    self.toast(format!("Couldn't read that Word file: {e}"));
                    return;
                }
            },
            "odt" => match crate::odt::read(&bytes) {
                Ok(d) => d,
                Err(e) => {
                    self.toast(format!("Couldn't read that OpenDocument file: {e}"));
                    return;
                }
            },
            "pdf" => match crate::pdf_import::read(&bytes) {
                Ok(d) => d,
                Err(e) => {
                    self.toast(format!("Couldn't read that PDF file: {e}"));
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
        let mut t = Tab::new(
            self.next_id,
            path.file_name().map_or_else(|| "Document".into(), |n| n.to_string_lossy().to_string()),
            doc,
        );
        if ext == "pdf" {
            t.attach_pdf_exact_view(bytes);
        }
        self.next_id += 1;
        if matches!(ext.as_str(), "wpx" | "gdoc" | "rtf" | "txt" | "docx" | "odt" | "md" | "markdown" | "pdf") {
            t.path = Some(path.to_path_buf());
        }
        self.tabs.push(t);
        self.active = self.tabs.len() - 1;
        self.add_recent(path);
    }

    /// Opens a document on a background thread with live progress reporting so large files
    /// (especially multi-page PDFs) never freeze the UI or trigger OS "(Not Responding)" warnings.
    pub fn open_path_async(&mut self, path: &Path) {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ext == "doc" {
            self.toast("Old Word (.doc) files aren't supported. Save it as .docx or Rich Text first.".into());
            return;
        }
        if matches!(ext.as_str(), "lwp" | "sam" | "mwp") {
            self.toast("Files from the 1999 program use a closed format. Save them as Rich Text (RTF) there first.".into());
            return;
        }

        if self.doc_load.is_some() {
            self.doc_load_queue.push(path.to_path_buf());
            return;
        }

        let file_name = path
            .file_name()
            .map_or_else(|| "Document".into(), |n| n.to_string_lossy().to_string());
        let file_len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let show_immediately = ext == "pdf" || file_len >= 256 * 1024;
        let initial_status = if file_len >= 1024 * 1024 {
            format!("Reading {file_name} ({:.1} MB)…", file_len as f64 / 1_048_576.0)
        } else {
            format!("Reading {file_name}…")
        };

        let (tx, rx) = std::sync::mpsc::channel::<DocLoadMsg>();
        let ctx = self.egui_ctx.clone();
        let path_buf = path.to_path_buf();
        let ext_clone = ext.clone();
        let status_for_thread = initial_status.clone();

        std::thread::spawn(move || {
            let send_prog = |frac: f32, msg: &str| {
                let _ = tx.send(DocLoadMsg::Progress {
                    frac: frac.clamp(0.0, 1.0),
                    status: msg.to_string(),
                });
                ctx.request_repaint();
            };

            send_prog(0.03, &status_for_thread);

            let res = (|| -> Result<LoadedDocument, String> {
                let bytes = std::fs::read(&path_buf)
                    .map_err(|e| format!("Couldn't open {}: {e}", path_buf.display()))?;

                let (doc, pdf_view) = match ext_clone.as_str() {
                    "pdf" => {
                        let doc = crate::pdf_import::read_with_progress(&bytes, &mut |f, s| {
                            // First pass maps 0.05..0.58 of total bar
                            send_prog(0.05 + f * 0.53, s);
                        })
                        .map_err(|e| format!("Couldn't read that PDF file: {e}"))?;

                        let arc = Arc::new(bytes);
                        send_prog(0.60, "Preparing PDF page renderer…");
                        let pdf_view = if let Some(dims) = crate::pdf::hayro_pdf_info(arc.clone()) {
                            if !dims.is_empty() {
                                let blocks = crate::pdf_import::extract_editable_blocks_with_progress(
                                    &arc,
                                    &mut |f, s| {
                                        // Second pass maps 0.62..0.96 of total bar
                                        send_prog(0.62 + f * 0.34, s);
                                    },
                                )
                                .unwrap_or_default();
                                Some((arc, dims, blocks))
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        (doc, pdf_view)
                    }
                    "docx" => {
                        send_prog(0.30, "Parsing Word document (.docx)…");
                        let doc = crate::docx::read(&bytes)
                            .map_err(|e| format!("Couldn't read that Word file: {e}"))?;
                        (doc, None)
                    }
                    "odt" => {
                        send_prog(0.30, "Parsing OpenDocument (.odt)…");
                        let doc = crate::odt::read(&bytes)
                            .map_err(|e| format!("Couldn't read that OpenDocument file: {e}"))?;
                        (doc, None)
                    }
                    "wpx" => {
                        send_prog(0.35, "Parsing Syllabary document…");
                        let text = doc::decode_text(&bytes);
                        let doc = doc::from_json(&text)
                            .map_err(|e| format!("That file isn't a readable document ({e})."))?;
                        (doc, None)
                    }
                    "gdoc" => {
                        send_prog(0.35, "Parsing Google Docs file…");
                        let text = doc::decode_text(&bytes);
                        let doc = doc::from_gdoc(&text)
                            .map_err(|e| format!("Couldn't read that Google Docs file: {e}"))?;
                        (doc, None)
                    }
                    "rtf" => {
                        send_prog(0.35, "Parsing Rich Text document…");
                        let raw: String = bytes.iter().map(|&b| b as char).collect();
                        (doc::from_rtf(&raw), None)
                    }
                    "md" | "markdown" => {
                        send_prog(0.35, "Parsing Markdown document…");
                        let text = doc::decode_text(&bytes);
                        (doc::from_markdown(&text), None)
                    }
                    _ => {
                        send_prog(0.35, "Decoding text document…");
                        let text = doc::decode_text(&bytes);
                        (doc::from_plain(&text), None)
                    }
                };

                send_prog(0.98, "Opening document…");
                Ok(LoadedDocument {
                    path: path_buf,
                    ext: ext_clone,
                    doc,
                    pdf_view,
                })
            })();

            let _ = tx.send(DocLoadMsg::Done(res));
            ctx.request_repaint();
        });

        self.doc_load = Some(DocLoadJob {
            file_name,
            progress: 0.03,
            status: initial_status,
            started_at: std::time::Instant::now(),
            show_immediately,
            rx,
        });
        self.egui_ctx.request_repaint();
    }

    /// Polls the background document loader each frame and opens the tab once ready.
    fn poll_doc_load(&mut self, ctx: &egui::Context) {
        let mut finished: Option<Result<LoadedDocument, String>> = None;
        if let Some(job) = self.doc_load.as_mut() {
            while let Ok(msg) = job.rx.try_recv() {
                match msg {
                    DocLoadMsg::Progress { frac, status } => {
                        job.progress = frac;
                        job.status = status;
                    }
                    DocLoadMsg::Done(res) => {
                        finished = Some(res);
                        break;
                    }
                }
            }
            if finished.is_none() {
                // Keep repainting smoothly so the spinner & progress bar animate
                ctx.request_repaint_after(std::time::Duration::from_millis(33));
            }
        }

        if let Some(res) = finished {
            self.doc_load = None;
            match res {
                Ok(loaded) => {
                    if self.tabs.len() == 1
                        && !self.tabs[0].dirty
                        && self.tabs[0].path.is_none()
                        && self.tabs[0].doc.plain_text().is_empty()
                    {
                        self.tabs.clear();
                    }
                    let mut t = Tab::new(
                        self.next_id,
                        loaded
                            .path
                            .file_name()
                            .map_or_else(|| "Document".into(), |n| n.to_string_lossy().to_string()),
                        loaded.doc,
                    );
                    if let Some((bytes, dims, blocks)) = loaded.pdf_view {
                        t.attach_prepared_pdf_view(bytes, dims, blocks);
                    }
                    self.next_id += 1;
                    if matches!(
                        loaded.ext.as_str(),
                        "wpx" | "gdoc" | "rtf" | "txt" | "docx" | "odt" | "md" | "markdown" | "pdf"
                    ) {
                        t.path = Some(loaded.path.clone());
                    }
                    self.tabs.push(t);
                    self.active = self.tabs.len() - 1;
                    self.add_recent(&loaded.path);
                }
                Err(err) => {
                    self.toast(err);
                }
            }
            if !self.doc_load_queue.is_empty() {
                let next = self.doc_load_queue.remove(0);
                self.open_path_async(&next);
            }
        }
    }

    /// Renders a sleek, non-blocking progress card while a large document loads in the background.
    fn draw_doc_loading_overlay(&self, ctx: &egui::Context, th: &Theme) {
        let Some(job) = &self.doc_load else { return };
        if !job.show_immediately && job.started_at.elapsed().as_millis() < 120 {
            return;
        }

        let screen = ctx.content_rect();
        // Subtle translucent backdrop scrim
        egui::Area::new(egui::Id::unique("doc-loading-scrim"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (_, _) = ui.allocate_exact_size(screen.size(), Sense::click());
                let scrim = if th.dark {
                    Color32::from_black_alpha(115)
                } else {
                    Color32::from_black_alpha(55)
                };
                ui.painter().rect_filled(screen, 0.0, scrim);
            });

        // Centered progress card
        egui::Area::new(egui::Id::unique("doc-loading-card"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(th.chrome)
                    .stroke(Stroke::new(1.0, th.border))
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(Margin::symmetric(24, 20))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(if th.dark { 140 } else { 45 }),
                    })
                    .show(ui, |ui| {
                        ui.set_width(380.0);
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(format!("Opening {}", job.file_name))
                                    .size(15.0)
                                    .strong()
                                    .color(th.text),
                            );
                        });
                        ui.add_space(12.0);
                        ui.add(
                            egui::ProgressBar::new(job.progress)
                                .desired_height(12.0)
                                .show_percentage()
                                .animate(true),
                        );
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&job.status).size(12.0).color(th.subtle));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let secs = job.started_at.elapsed().as_secs();
                                if secs >= 2 {
                                    ui.label(
                                        RichText::new(format!("{secs}s"))
                                            .size(11.5)
                                            .color(th.subtle),
                                    );
                                }
                            });
                        });
                    });
            });
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
            "md" | "markdown" => doc::to_markdown(doc).into_bytes(),
            "html" | "htm" => doc::to_html(doc).into_bytes(),
            "docx" => crate::docx::write(doc),
            "odt" => crate::odt::write(doc),
            "gdoc" => doc::to_gdoc(doc).into_bytes(),
            _ => doc::to_json(doc).into_bytes(),
        };
        std::fs::write(path, data)
    }

    pub fn save(&mut self, idx: usize, save_as: bool) -> bool {
        let path = if let Some(p) = self.tabs[idx].path.clone().filter(|_| !save_as) {
            p
        } else {
            let base = self.tabs[idx].name.split('.').next().unwrap_or("Document").to_string();
            let default_ext = self.tabs[idx]
                .path
                .as_ref()
                .and_then(|p| p.extension())
                .map(|e| e.to_string_lossy().to_lowercase())
                .filter(|e| matches!(e.as_str(), "wpx" | "gdoc" | "docx" | "odt" | "pdf" | "md" | "rtf" | "txt"))
                .unwrap_or_else(|| FILE_EXT.to_string());
            match rfd::FileDialog::new()
                .set_file_name(format!("{base}.{default_ext}"))
                .add_filter("Syllabary document", &[FILE_EXT])
                .add_filter("PDF document", &["pdf"])
                .add_filter("Google Docs", &["gdoc"])
                .add_filter("Word document", &["docx"])
                .add_filter("OpenDocument Text", &["odt"])
                .add_filter("Markdown", &["md"])
                .add_filter("Rich Text", &["rtf"])
                .add_filter("Plain text", &["txt"])
                .save_file()
            {
                Some(p) => p,
                None => return false,
            }
        };
        let is_pdf = path
            .extension()
            .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("pdf"));
        let mut saved_exact_pdf: Option<Vec<u8>> = None;
        let res = if is_pdf {
            let exact_res = self.tabs[idx].pdf_view.as_ref().and_then(|pv| {
                if pv.is_exact() {
                    if pv.has_edits() {
                        Some(crate::pdf::export_edited_pdf(
                            pv.bytes.clone(),
                            &pv.page_dims_pt,
                            &pv.blocks,
                        ))
                    } else {
                        Some(Ok((*pv.bytes).clone()))
                    }
                } else {
                    None
                }
            });
            match exact_res {
                Some(Ok(bytes)) => {
                    saved_exact_pdf = Some(bytes.clone());
                    std::fs::write(&path, bytes)
                }
                Some(Err(err)) => Err(std::io::Error::other(err)),
                None => {
                    let bytes = crate::pdf::export(&self.egui_ctx, &self.tabs[idx].doc, &self.book);
                    std::fs::write(&path, bytes)
                }
            }
        } else {
            Self::write_doc(&self.tabs[idx].doc, &path)
        };
        match res {
            Ok(()) => {
                let t = &mut self.tabs[idx];
                if let Some(new_bytes) = saved_exact_pdf {
                    if let Some(pv) = t.pdf_view.as_mut() {
                        pv.reload_saved(new_bytes);
                    }
                }
                t.path = Some(path.clone());
                t.name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                t.dirty = false;
                t.saved_rev = t.rev;
                t.save_version("Saved to disk");
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

    fn export_pdf(&mut self, ctx: &egui::Context) {
        let t = self.tab_ref();
        let base = t.name.split('.').next().unwrap_or("Document").to_string();
        let Some(path) = rfd::FileDialog::new().set_file_name(format!("{base}.pdf")).add_filter("PDF", &["pdf"]).save_file() else {
            return;
        };
        let bytes = if let Some(pv) = t.pdf_view.as_ref().filter(|pv| pv.is_exact()) {
            if pv.has_edits() {
                match crate::pdf::export_edited_pdf(pv.bytes.clone(), &pv.page_dims_pt, &pv.blocks) {
                    Ok(b) => b,
                    Err(e) => {
                        self.toast(format!("Couldn't export PDF: {e}"));
                        return;
                    }
                }
            } else {
                (*pv.bytes).clone()
            }
        } else {
            crate::pdf::export(ctx, &t.doc, &self.book)
        };
        match std::fs::write(&path, bytes) {
            Ok(()) => self.toast(format!("Exported to {}", path.display())),
            Err(e) => self.toast(format!("Couldn't write the PDF: {e}")),
        }
    }

    fn print_doc(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "windows")]
        {
            let doc = self.tab_ref().doc.clone();
            let base = self.tab_ref().name.split('.').next().unwrap_or("Document").to_string();
            match crate::print::print_native_windows(ctx, &doc, &self.book, &base) {
                crate::print::PrintOutcome::Printed(msg) => self.toast(msg),
                crate::print::PrintOutcome::Cancelled => {}
                crate::print::PrintOutcome::Error(err) => self.toast(err),
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = ctx;
            let pages = self.tab_ref().cached_pages().max(1);
            self.dialog = Dialog::Print(crate::print::PrintJobConfig::discover(pages));
        }
    }

    fn export(&mut self, ext: &str) {
        let base = self.tab_ref().name.split('.').next().unwrap_or("Document").to_string();
        let filter = match ext {
            "rtf" => "Rich Text",
            "docx" => "Word document",
            "odt" => "OpenDocument Text",
            "gdoc" => "Google Docs",
            "md" => "Markdown",
            "html" => "Web Page (HTML)",
            _ => "Plain text",
        };
        if let Some(p) = rfd::FileDialog::new().set_file_name(format!("{base}.{ext}")).add_filter(filter, &[ext]).save_file() {
            match Self::write_doc(&self.tab_ref().doc, &p) {
                Ok(()) => self.toast(format!("Downloaded {}", p.display())),
                Err(e) => self.toast(format!("Couldn't export: {e}")),
            }
        }
    }

    pub fn close_tab(&mut self, idx: usize, force: bool) {
        if idx >= self.tabs.len() {
            return;
        }
        if self.tabs[idx].dirty && !force {
            self.close_flow = CloseFlow::ConfirmTab(idx);
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

    /// Returns the active ModernRust Fluent palette matching Syllabary's Light / Dark mode.
    pub fn mr_palette(&self) -> modernrust::ThemePalette {
        if self.cfg.dark {
            modernrust::ThemePreset::ObsidianPaper.palette()
        } else {
            modernrust::ThemePreset::FluentLight.palette()
        }
    }

    pub fn toast(&mut self, msg: String) {
        let lower = msg.to_ascii_lowercase();
        let (title, role) = if lower.starts_with("saved")
            || lower.starts_with("exported")
            || lower.starts_with("restored")
            || lower.starts_with("inserted")
            || lower.starts_with("created")
        {
            ("Success", modernrust::Role::Success)
        } else if lower.contains("couldn't")
            || lower.contains("failed")
            || lower.contains("error")
        {
            ("Error", modernrust::Role::Danger)
        } else if lower.contains("place the cursor")
            || lower.contains("already")
            || lower.contains("unsaved")
        {
            ("Notice", modernrust::Role::Warning)
        } else {
            ("Syllabary", modernrust::Role::Info)
        };
        let now = self.egui_ctx.input(|i| i.time);
        self.toasts.push(title, msg, role, now);
    }

    pub fn start_update_check(&mut self, manual: bool) {
        if self.update.check_rx.is_some() {
            if manual {
                self.toast("Already checking for updates…".into());
            }
            return;
        }
        if manual {
            self.toast("Checking GitHub for updates…".into());
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let outcome = crate::updater::check_for_updates(crate::updater::CURRENT_VERSION);
            let _ = tx.send(outcome);
            ctx.request_repaint();
        });
        self.update.check_rx = Some((manual, rx));
    }

    pub fn start_update_download(&mut self) {
        let Some(rel) = self.update.release.clone() else {
            return;
        };
        let Some(url) = rel.asset_url.clone() else {
            self.update.phase = UpdatePhase::Error(
                "No downloadable executable asset was found for this OS in the GitHub release.".into(),
            );
            return;
        };
        let dest = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(format!("update_{}.exe", rel.version))))
            .unwrap_or_else(|| std::env::temp_dir().join(format!("syllabary_update_{}.exe", rel.version)));

        self.update.cancel.store(false, Ordering::Relaxed);
        if let Ok(mut p) = self.update.progress.lock() {
            *p = (0, rel.asset_size);
        }
        self.update.phase = UpdatePhase::Downloading;

        let (tx, rx) = std::sync::mpsc::channel();
        let prog = self.update.progress.clone();
        let cancel = self.update.cancel.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let res = crate::updater::download_update_asset(
                &url,
                &dest,
                rel.asset_size,
                rel.asset_sha256.as_deref(),
                &cancel,
                |down, tot| {
                    if let Ok(mut p) = prog.lock() {
                        *p = (down, tot);
                    }
                    ctx.request_repaint();
                },
            )
            .map(|()| dest);
            let _ = tx.send(res);
            ctx.request_repaint();
        });
        self.update.download_rx = Some(rx);
    }

    pub fn poll_updater(&mut self, ctx: &egui::Context) {
        if let Some((manual, rx)) = &self.update.check_rx {
            match rx.try_recv() {
                Ok(outcome) => {
                    let manual = *manual;
                    self.update.check_rx = None;
                    match outcome {
                        crate::updater::CheckOutcome::UpdateAvailable(info) => {
                            self.update.release = Some(info);
                            self.update.phase = UpdatePhase::Prompt;
                            self.dialog = Dialog::Update;
                        }
                        crate::updater::CheckOutcome::UpToDate(latest) => {
                            if manual {
                                self.toast(format!(
                                    "Syllabary v{} is up to date (latest: v{latest}).",
                                    crate::updater::CURRENT_VERSION
                                ));
                            }
                        }
                        crate::updater::CheckOutcome::NoReleases => {
                            if manual {
                                self.toast("No GitHub releases published yet for Syllabary.".into());
                            }
                        }
                        crate::updater::CheckOutcome::Error(e) => {
                            if manual {
                                self.toast(format!("Update check failed: {e}"));
                            }
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.update.check_rx = None;
                }
            }
        }
        if let Some(rx) = &self.update.download_rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.update.download_rx = None;
                    match res {
                        Ok(dest_file) => {
                            let ver = self
                                .update
                                .release
                                .as_ref()
                                .map_or_else(|| "latest".into(), |r| r.version.clone());
                            match crate::updater::replace_executable_in_place(&dest_file) {
                                Ok(_) => {
                                    self.update.phase = UpdatePhase::ReadyToRestart(ver);
                                }
                                Err(e) => {
                                    self.update.phase = UpdatePhase::Error(format!(
                                        "{e}\n\nDownloaded update saved to:\n{}",
                                        dest_file.display()
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            if !self.update.cancel.load(Ordering::Relaxed) {
                                self.update.phase = UpdatePhase::Error(e);
                            } else {
                                self.update.phase = UpdatePhase::Idle;
                                self.dialog = Dialog::None;
                            }
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(80));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.update.download_rx = None;
                }
            }
        }
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
        self.close_flow = CloseFlow::ForceQuit;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn insert_smart_chip(&mut self, kind: &str, now: f64) {
        let today = chrono::Local::now();
        let (label, bg, fg): (String, [u8; 3], [u8; 3]) = match kind {
            "today" => (format!(" 📅 {} ", today.format("%b %-d, %Y")), [219, 234, 254], [30, 64, 175]),
            "tomorrow" => {
                let tmr = today + chrono::Duration::days(1);
                (format!(" 📅 {} ", tmr.format("%b %-d, %Y")), [219, 234, 254], [30, 64, 175])
            }
            "yesterday" => {
                let yst = today - chrono::Duration::days(1);
                (format!(" 📅 {} ", yst.format("%b %-d, %Y")), [229, 231, 235], [55, 65, 81])
            }
            "status_not_started" => (" ⚪ Not started ▾ ".into(), [229, 231, 235], [55, 65, 81]),
            "status_in_progress" => (" 🔵 In progress ▾ ".into(), [191, 219, 254], [29, 78, 216]),
            "status_review" => (" 🟡 Under review ▾ ".into(), [254, 240, 138], [161, 98, 7]),
            "status_approved" => (" 🟢 Approved ▾ ".into(), [187, 247, 208], [21, 128, 61]),
            "priority_low" => (" ▫ Low priority ▾ ".into(), [229, 231, 235], [55, 65, 81]),
            "priority_med" => (" ▪ Medium priority ▾ ".into(), [254, 215, 170], [194, 65, 12]),
            "priority_high" => (" 🔥 High priority ▾ ".into(), [251, 207, 232], [185, 28, 28]),
            "person" => {
                let author = if self.tab_ref().doc.info.author.is_empty() { "Team Member".to_string() } else { self.tab_ref().doc.info.author.clone() };
                (format!(" 👤 @{author} "), [221, 214, 254], [91, 33, 182])
            }
            "timer" => (" ⏱ 25:00 Focus ".into(), [229, 231, 235], [31, 41, 55]),
            _ => (format!(" @{kind} "), [229, 231, 235], [31, 41, 55]),
        };
        let t = self.tab();
        t.checkpoint(EditKind::Other, now);
        t.delete_selection();
        let base = t.typing_style();
        let chip_style = CharStyle {
            font: 1, // Arial / clean sans for Google Docs chip look
            size: (base.size - 0.5).max(9.0),
            bold: true,
            color: fg,
            highlight: Some(bg),
            ..base
        };
        let pos = t.doc.insert_text(t.caret, &label, chip_style);
        let pos = t.doc.insert_text(pos, " ", base);
        t.set_caret(pos, false);
        t.changed();
    }

    fn insert_building_block(&mut self, kind: &str, now: f64) {
        let today = chrono::Local::now().format("%B %-d, %Y").to_string();
        let t = self.tab();
        t.checkpoint(EditKind::Other, now);
        t.delete_selection();
        let styles = named_styles();
        if !t.doc.paras[t.caret.p].text.is_empty() {
            t.caret = t.doc.split(t.caret);
        }
        let mut p = t.caret.p;
        let add_line = |doc: &mut Document, p: &mut usize, style_idx: usize, list: ListKind, checked: bool, text: &str| {
            let ns = &styles[style_idx];
            doc.paras[*p].fmt = ParaFmt { style: style_idx, list, checked, ..ns.fmt };
            doc.paras[*p].end_style = ns.chr;
            let end = doc.insert_text(Pos::new(*p, 0), text, ns.chr);
            let next = doc.split(end);
            *p = next.p;
        };
        match kind {
            "meeting" => {
                add_line(&mut t.doc, &mut p, 4, ListKind::None, false, &format!("📅 Meeting Notes — {today}"));
                add_line(&mut t.doc, &mut p, 0, ListKind::Bullet, false, "Attendees: @Team");
                add_line(&mut t.doc, &mut p, 5, ListKind::None, false, "Agenda & Discussion");
                add_line(&mut t.doc, &mut p, 0, ListKind::Bullet, false, "Key updates and milestones");
                add_line(&mut t.doc, &mut p, 0, ListKind::Bullet, false, "Open questions & blockers");
                add_line(&mut t.doc, &mut p, 5, ListKind::None, false, "Action Items");
                add_line(&mut t.doc, &mut p, 0, ListKind::Checklist, false, "Share meeting summary with stakeholders");
                add_line(&mut t.doc, &mut p, 0, ListKind::Checklist, false, "Follow up on next deliverables");
            }
            "email" => {
                add_line(&mut t.doc, &mut p, 5, ListKind::None, false, "✉️ Email Draft");
                add_line(&mut t.doc, &mut p, 0, ListKind::None, false, "To: recipient@example.com");
                add_line(&mut t.doc, &mut p, 0, ListKind::None, false, "Cc: ");
                add_line(&mut t.doc, &mut p, 0, ListKind::None, false, "Subject: Project update & next steps");
                t.doc.paras[p].fmt.hr = true;
                let next = t.doc.split(Pos::new(p, 0));
                p = next.p;
                add_line(&mut t.doc, &mut p, 0, ListKind::None, false, "Hi everyone,");
                add_line(&mut t.doc, &mut p, 0, ListKind::None, false, "Here is a quick summary of our progress this week:");
                add_line(&mut t.doc, &mut p, 0, ListKind::None, false, "Best regards,");
            }
            "roadmap" => {
                add_line(&mut t.doc, &mut p, 4, ListKind::None, false, "🗺️ Project Roadmap");
                add_line(&mut t.doc, &mut p, 0, ListKind::Number, false, "Phase 1: Discovery & Specification   [ 🟢 Approved ]");
                add_line(&mut t.doc, &mut p, 0, ListKind::Number, false, "Phase 2: Core Implementation          [ 🔵 In progress ]");
                add_line(&mut t.doc, &mut p, 0, ListKind::Number, false, "Phase 3: QA & Release Readiness       [ ⚪ Not started ]");
            }
            "checklist" => {
                add_line(&mut t.doc, &mut p, 5, ListKind::None, false, "✅ Launch Readiness Checklist");
                add_line(&mut t.doc, &mut p, 0, ListKind::Checklist, true, "Complete design review and sign-off");
                add_line(&mut t.doc, &mut p, 0, ListKind::Checklist, false, "Run automated test suite & verify formatting");
                add_line(&mut t.doc, &mut p, 0, ListKind::Checklist, false, "Prepare release notes & documentation");
            }
            "code" => {
                let ns = &styles[10];
                t.doc.paras[p].fmt = ns.fmt;
                t.doc.paras[p].end_style = ns.chr;
                let end = t.doc.insert_text(Pos::new(p, 0), "fn main() {\n    println!(\"Hello, Syllabary!\");\n}", ns.chr);
                p = end.p;
            }
            "table_2x2" => {
                let first_cell = t.doc.insert_table(Pos::new(p, 0), 2, 2);
                p = first_cell.p;
            }
            "table_3x3" => {
                let first_cell = t.doc.insert_table(Pos::new(p, 0), 3, 3);
                p = first_cell.p;
            }
            _ => {}
        }
        t.set_caret(Pos::new(p, 0), false);
        t.changed();
    }

    fn insert_toc(&mut self, now: f64) {
        let styles = named_styles();
        let mut heads: Vec<(usize, usize, String)> = Vec::new();
        for (i, p) in self.tab_ref().doc.paras.iter().enumerate() {
            let level = match styles.get(p.fmt.style).map(|s| s.name) {
                Some("Title") => 0,
                Some("Heading 1") => 1,
                Some("Heading 2") => 2,
                Some("Heading 3") => 3,
                _ => continue,
            };
            let txt = p.string();
            if !txt.trim().is_empty() && txt.trim() != "Table of Contents" {
                heads.push((i, level, txt));
            }
        }
        if heads.is_empty() {
            self.toast("Add headings (Heading 1–3) first to generate a Table of Contents.".into());
            return;
        }
        let t = self.tab();
        t.checkpoint(EditKind::Other, now);
        if !t.doc.paras[t.caret.p].text.is_empty() {
            t.caret = t.doc.split(t.caret);
        }
        let mut p = t.caret.p;
        let h_ns = &styles[5];
        t.doc.paras[p].fmt = ParaFmt { style: 5, ..h_ns.fmt };
        let end = t.doc.insert_text(Pos::new(p, 0), "Table of Contents", h_ns.chr);
        p = t.doc.split(end).p;
        for (target_p, level, title) in heads {
            let lid = t.doc.add_link(&format!("#heading:{target_p}"));
            let indent_spaces = "    ".repeat(level.saturating_sub(1));
            let line_txt = format!("{indent_spaces}• {title}");
            let st = CharStyle {
                font: 1,
                size: 11.5,
                color: [11, 87, 208],
                underline: true,
                link: lid,
                ..CharStyle::default()
            };
            let end = t.doc.insert_text(Pos::new(p, 0), &line_txt, st);
            p = t.doc.split(end).p;
        }
        t.set_caret(Pos::new(p, 0), false);
        t.changed();
        self.toast("Inserted clickable Table of Contents.".into());
    }

    fn run_actions(&mut self, ctx: &egui::Context) {
        let actions = std::mem::take(&mut self.actions);
        let now = ctx.input(|i| i.time);
        for a in actions {
            let mut refocus = true;
            match a {
                Action::New => self.new_doc(),
                Action::Open => {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Documents", &[FILE_EXT, "gdoc", "docx", "odt", "pdf", "md", "rtf", "txt"])
                        .add_filter("OpenDocument Text", &["odt"])
                        .add_filter("PDF documents", &["pdf"])
                        .add_filter("Google Docs", &["gdoc"])
                        .add_filter("All files", &["*"])
                        .pick_file()
                    {
                        self.open_path_async(&p);
                    }
                }
                Action::OpenPath(p) => self.open_path_async(&p),
                Action::Save => {
                    let i = self.active;
                    self.save(i, false);
                }
                Action::SaveAs => {
                    let i = self.active;
                    self.save(i, true);
                }
                Action::MakeCopy => {
                    let cur = self.tab_ref();
                    let copy_name = format!("Copy of {}", cur.name);
                    let copy_doc = cur.doc.clone();
                    let mut t = Tab::new(self.next_id, copy_name, copy_doc);
                    self.next_id += 1;
                    t.dirty = true;
                    self.tabs.push(t);
                    self.active = self.tabs.len() - 1;
                    self.toast("Created a copy of the document in a new tab.".into());
                }
                Action::ExportRtf => self.export("rtf"),
                Action::ExportTxt => self.export("txt"),
                Action::ExportDocx => self.export("docx"),
                Action::ExportOdt => self.export("odt"),
                Action::ExportGdoc => self.export("gdoc"),
                Action::ExportMd => self.export("md"),
                Action::ExportHtml => self.export("html"),
                Action::ExportPdf => self.export_pdf(ctx),
                Action::Print => self.print_doc(ctx),
                Action::LineSpacing(ls) => self.tab().apply_para(now, &move |f| f.line_spacing = ls),
                Action::SpaceBefore(sb) => self.tab().apply_para(now, &move |f| f.space_before = sb.max(0.0)),
                Action::SpaceAfter(sa) => self.tab().apply_para(now, &move |f| f.space_after = sa.max(0.0)),
                Action::FormatPainter => {
                    if self.painter_style.is_some() {
                        self.painter_style = None;
                    } else {
                        let t = self.tab_ref();
                        let st = t.typing_style();
                        let f = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                        self.painter_style = Some((st, f));
                        self.toast("Paint format: select text to apply the copied formatting.".into());
                    }
                }
                Action::ToggleNav => self.show_nav = !self.show_nav,
                Action::ToggleComments => self.right_panel.toggle(RightPanel::Comments),
                Action::ToggleHistory => self.right_panel.toggle(RightPanel::VersionHistory),
                Action::OpenNameVersion => {
                    self.right_panel = RightPanel::VersionHistory;
                    if self.history_ver_name.trim().is_empty() {
                        self.history_ver_name = "Milestone".into();
                    }
                }
                Action::ToggleSpell => {
                    self.cfg.spellcheck = !self.cfg.spellcheck;
                    self.save_config();
                }
                Action::TogglePageless => {
                    let t = self.tab();
                    t.doc.pageless = !t.doc.pageless;
                    t.rev += 1;
                    let on = t.doc.pageless;
                    self.toast(if on { "Switched to Pageless view.".into() } else { "Switched to Pages view.".into() });
                }
                Action::ToggleStar => {
                    let t = self.tab();
                    t.doc.starred = !t.doc.starred;
                    t.dirty = true;
                }
                Action::ToggleCompact => self.compact_mode = !self.compact_mode,
                Action::ToggleLiveWordCount => {
                    self.cfg.live_word_count = !self.cfg.live_word_count;
                    self.save_config();
                }
                Action::SetMode(m) => {
                    self.mode = m;
                    if m == DocMode::Suggesting {
                        self.right_panel = RightPanel::Comments;
                    }
                    self.toast(format!("Mode: {}", m.label()));
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
                        self.close_flow = CloseFlow::ConfirmTab(i);
                    }
                }
                Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Action::Undo => {
                    if !self.tab().pdf_undo() {
                        self.tab().undo();
                    }
                }
                Action::Redo => {
                    if !self.tab().pdf_redo() {
                        self.tab().redo();
                    }
                }
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
                Action::Paste => {
                    self.paste_plain_next = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                }
                Action::PastePlain => {
                    self.paste_plain_next = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                }
                Action::DuplicatePara => self.tab().duplicate_para(now),
                Action::SelectAll => self.tab().select_all(),
                Action::Dialog(d) => {
                    if matches!(d, Dialog::PageSetup) {
                        self.right_panel = RightPanel::PageSetup;
                        continue;
                    }
                    if matches!(d, Dialog::Prefs) {
                        self.right_panel = RightPanel::Settings;
                        continue;
                    }
                    if let Dialog::Find(replace) = &d {
                        let sel = self.tab_ref().selected_text();
                        if !sel.is_empty() && !sel.contains('\n') && sel.chars().count() < 80 {
                            self.find.query = sel;
                        }
                        self.find.focus = true;
                        self.find.show_replace = *replace;
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
                    self.fonts_rev = self.fonts_rev.wrapping_add(1);
                    ctx.request_repaint();
                    self.save_config();
                }
                Action::CheckForUpdates => {
                    self.start_update_check(true);
                }
                Action::PageBreak => self.tab().page_break(now),
                Action::HorizontalRule => self.tab().insert_hr(now),
                Action::InsertText(s) => self.tab().insert(&s, now),
                Action::SystemEmojiPicker => {
                    self.dialog = Dialog::None;
                    ctx.memory_mut(|m| m.request_focus(self.doc_id));
                    if !open_system_emoji_picker() {
                        self.dialog = Dialog::Symbol;
                        self.toast("Opened Syllabary Emoji & Symbol Library.".into());
                    }
                }
                Action::InsertFootnote => {
                    let t = self.tab();
                    t.checkpoint(EditKind::Other, now);
                    let fn_num = t.doc.paras.iter().filter(|p| p.string().starts_with('[') && p.string().contains("] Footnote:")).count() + 1;
                    let mut sup = t.typing_style();
                    sup.script = 1;
                    sup.bold = true;
                    sup.color = [11, 87, 208];
                    t.caret = t.doc.insert_text(t.caret, &format!("[{fn_num}]"), sup);
                    let foot_para = Para {
                        text: format!("[{fn_num}] Footnote: ").chars().collect(),
                        styles: vec![
                            CharStyle { size: 9.5, color: [95, 99, 104], ..CharStyle::default() };
                            format!("[{fn_num}] Footnote: ").chars().count()
                        ],
                        fmt: ParaFmt { space_before: 4.0, ..ParaFmt::default() },
                        end_style: CharStyle { size: 9.5, color: [95, 99, 104], ..CharStyle::default() },
                    };
                    t.doc.paras.push(foot_para);
                    let last_p = t.doc.paras.len() - 1;
                    let last_c = t.doc.paras[last_p].len();
                    t.set_caret(Pos::new(last_p, last_c), false);
                    t.changed();
                    self.toast(format!("Inserted footnote [{fn_num}] — type your footnote text at the bottom."));
                }
                Action::InsertToc => self.insert_toc(now),
                Action::InsertBuildingBlock(k) => self.insert_building_block(k, now),
                Action::InsertSmartChip(k) => self.insert_smart_chip(k, now),
                Action::InsertImage => {
                    if let Some(p) = rfd::FileDialog::new()
                        .set_title("Insert Image from File")
                        .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "ico", "tga"])
                        .add_filter("All files", &["*"])
                        .pick_file()
                    {
                        self.insert_image_from_path(&p, now);
                    }
                }
                Action::InsertImagePath(p) => {
                    self.insert_image_from_path(&p, now);
                }
                Action::ReplaceImage(img_id) => {
                    if let Some(p) = rfd::FileDialog::new()
                        .set_title("Replace Image from File")
                        .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "ico", "tga"])
                        .add_filter("All files", &["*"])
                        .pick_file()
                    {
                        self.replace_image_from_path(img_id, &p, now);
                    }
                }
                Action::InsertLink { text, url } => {
                    let t = self.tab();
                    t.checkpoint(EditKind::Other, now);
                    let lid = t.doc.add_link(&url);
                    if t.has_selection() {
                        let (a, b) = t.selection();
                        if !text.is_empty() && text != t.doc.text_range(a, b) {
                            let mut st = t.doc.style_at(a);
                            st.link = lid;
                            st.underline = true;
                            st.color = [11, 87, 208];
                            t.doc.delete_range(a, b);
                            let end = t.doc.insert_text(a, &text, st);
                            t.set_caret(end, false);
                        } else {
                            t.doc.restyle(a, b, &move |s| {
                                s.link = lid;
                                s.underline = true;
                                s.color = [11, 87, 208];
                            });
                        }
                    } else if let Some((a, b, _)) = t.doc.link_bounds(t.caret) {
                        let mut st = t.doc.style_at(a);
                        st.link = lid;
                        st.underline = true;
                        st.color = [11, 87, 208];
                        t.doc.delete_range(a, b);
                        let lbl = if text.is_empty() { url.clone() } else { text };
                        let end = t.doc.insert_text(a, &lbl, st);
                        t.set_caret(end, false);
                    } else {
                        let lbl = if text.is_empty() { url.clone() } else { text };
                        let mut st = t.typing_style();
                        st.link = lid;
                        st.underline = true;
                        st.color = [11, 87, 208];
                        let end = t.doc.insert_text(t.caret, &lbl, st);
                        t.set_caret(end, false);
                    }
                    t.changed();
                }
                Action::RemoveLink(pos) => {
                    let t = self.tab();
                    if let Some((a, b, _)) = t.doc.link_bounds(pos) {
                        t.checkpoint(EditKind::Other, now);
                        t.doc.restyle(a, b, &|s| {
                            s.link = 0;
                            s.underline = false;
                            if s.color == [11, 87, 208] {
                                s.color = [0, 0, 0];
                            }
                        });
                        t.changed();
                    }
                }
                Action::OpenLink(url) => {
                    if let Some(p_str) = url.strip_prefix("#heading:") {
                        if let Ok(p) = p_str.parse::<usize>() {
                            let t = self.tab();
                            t.set_caret(Pos::new(p, 0), false);
                            self.toast("Jumped to heading.".into());
                        }
                    } else if let Some(bname) = url.strip_prefix("#bookmark:") {
                        if let Some((_, p)) = self.tab_ref().doc.bookmarks.iter().find(|(n, _)| n == bname).cloned() {
                            let t = self.tab();
                            t.set_caret(Pos::new(p, 0), false);
                            self.toast(format!("Jumped to bookmark “{bname}”."));
                        }
                    } else {
                        ctx.copy_text(url.clone());
                        self.toast(format!("Copied link URL to clipboard: {url}"));
                    }
                }
                Action::AddComment(text) => {
                    if !text.trim().is_empty() {
                        let t = self.tab();
                        let (a, b) = if t.has_selection() { t.selection() } else { t.doc.word_at(t.caret) };
                        let quote = t.doc.text_range(a, b);
                        let id = t.doc.comments.len() as u64 + 1;
                        let author = if t.doc.info.author.is_empty() { "You".to_string() } else { t.doc.info.author.clone() };
                        t.doc.comments.push(DocComment {
                            id,
                            para: a.p,
                            start: a.c,
                            end: if a.p == b.p { b.c } else { t.doc.paras[a.p].len() },
                            quote: if quote.trim().is_empty() { t.doc.paras[a.p].string() } else { quote },
                            author,
                            text: text.trim().to_string(),
                            timestamp: chrono::Local::now().format("%-I:%M %p").to_string(),
                            resolved: false,
                            replies: Vec::new(),
                            is_suggestion: false,
                        });
                        t.dirty = true;
                        t.rev += 1;
                        self.right_panel = RightPanel::Comments;
                    }
                }
                Action::ResolveComment(id) => {
                    let t = self.tab();
                    if let Some(c) = t.doc.comments.iter_mut().find(|c| c.id == id) {
                        c.resolved = true;
                        t.dirty = true;
                        t.rev += 1;
                    }
                }
                Action::AcceptSuggestion(id) => {
                    let t = self.tab();
                    if let Some(c) = t.doc.comments.iter().find(|c| c.id == id).cloned() {
                        t.checkpoint(EditKind::Other, now);
                        let p = c.para.min(t.doc.paras.len().saturating_sub(1));
                        let end = c.end.min(t.doc.paras[p].len());
                        let start = c.start.min(end);
                        t.doc.restyle(Pos::new(p, start), Pos::new(p, end), &|s| {
                            if s.highlight == Some([187, 247, 208]) {
                                s.highlight = None;
                            }
                        });
                        if let Some(cm) = t.doc.comments.iter_mut().find(|x| x.id == id) {
                            cm.resolved = true;
                        }
                        t.changed();
                        self.toast("Accepted suggested edit.".into());
                    }
                }
                Action::RejectSuggestion(id) => {
                    let t = self.tab();
                    if let Some(c) = t.doc.comments.iter().find(|c| c.id == id).cloned() {
                        t.checkpoint(EditKind::Other, now);
                        let p = c.para.min(t.doc.paras.len().saturating_sub(1));
                        let end = c.end.min(t.doc.paras[p].len());
                        let start = c.start.min(end);
                        t.doc.delete_range(Pos::new(p, start), Pos::new(p, end));
                        if let Some(cm) = t.doc.comments.iter_mut().find(|x| x.id == id) {
                            cm.resolved = true;
                        }
                        t.changed();
                        self.toast("Rejected suggested edit.".into());
                    }
                }
                Action::DeleteComment(id) => {
                    let t = self.tab();
                    t.doc.comments.retain(|c| c.id != id);
                    t.dirty = true;
                    t.rev += 1;
                }
                Action::List(k) => {
                    let t = self.tab();
                    let cur = t.doc.paras[t.doc.clamp(t.caret).p].fmt.list;
                    let nk = if cur == k { ListKind::None } else { k };
                    t.apply_para(now, &move |f| {
                        f.list = nk;
                        if nk == ListKind::None {
                            f.checked = false;
                        }
                    });
                }
                Action::BulletPreset(k, preset) => {
                    let t = self.tab();
                    t.apply_para(now, &move |f| {
                        f.list = k;
                        f.bullet_preset = preset;
                    });
                }
                Action::ParaShading(sh) => {
                    let t = self.tab();
                    t.apply_para(now, &move |f| f.shading = sh);
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
                Action::StepFontSize(delta) => self.tab().apply_char(now, &move |s| s.size = (s.size + delta).clamp(MIN_SIZE, MAX_SIZE)),
                Action::ClearFormat => self.tab().clear_formatting(now),
                Action::Font(f) => {
                    self.tab().apply_char(now, &move |s| s.font = f);
                    self.cfg.recent_fonts.retain(|x| *x != f);
                    self.cfg.recent_fonts.insert(0, f);
                    self.cfg.recent_fonts.truncate(4);
                    self.save_config();
                }
                Action::Size(z) => self.tab().apply_char(now, &move |s| s.size = z.clamp(MIN_SIZE, MAX_SIZE)),
                Action::Color(c) => self.tab().apply_char(now, &move |s| {
                    s.color = c;
                    s.color_to = None;
                }),
                Action::ColorGradient(c1, c2) => self.tab().apply_char(now, &move |s| {
                    s.color = c1;
                    s.color_to = Some(c2);
                }),
                Action::Highlight(h) => self.tab().apply_char(now, &move |s| {
                    s.highlight = h;
                    s.highlight_to = None;
                }),
                Action::HighlightGradient(c1, c2) => self.tab().apply_char(now, &move |s| {
                    s.highlight = Some(c1);
                    s.highlight_to = Some(c2);
                }),
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
                Action::InsertTable {
                    rows,
                    cols,
                    header_shading,
                } => {
                    let t = self.tab();
                    t.checkpoint(EditKind::Other, now);
                    let pos = t.doc.insert_table(t.caret, rows, cols);
                    let tid = t.doc.paras[pos.p].fmt.table_id;
                    if let Some(shd) = header_shading {
                        t.doc.table_set_row_shading(tid, 0, Some(shd));
                        let luma = 0.299 * (shd[0] as f32) + 0.587 * (shd[1] as f32) + 0.114 * (shd[2] as f32);
                        let hdr_color = if luma < 155.0 { [255, 255, 255] } else { [0, 0, 0] };
                        for p in &mut t.doc.paras {
                            if p.fmt.table_id == tid && p.fmt.table_row == 0 {
                                p.end_style.bold = true;
                                p.end_style.color = hdr_color;
                            }
                        }
                    }
                    t.set_caret(pos, false);
                    t.changed();
                    self.toast(format!("Inserted {cols} × {rows} table."));
                }
                Action::TableInsertRow { above } => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        t.checkpoint(EditKind::Other, now);
                        if let Some(np) = t.doc.table_insert_row(fmt.table_id, fmt.table_row as usize, above) {
                            t.set_caret(np, false);
                            t.changed();
                        }
                    }
                }
                Action::TableDeleteRow => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        t.checkpoint(EditKind::Other, now);
                        if let Some(np) = t.doc.table_delete_row(fmt.table_id, fmt.table_row as usize) {
                            t.set_caret(np, false);
                            t.changed();
                        }
                    }
                }
                Action::TableInsertCol { left } => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        t.checkpoint(EditKind::Other, now);
                        if let Some(np) = t.doc.table_insert_col(fmt.table_id, fmt.table_col as usize, left) {
                            t.set_caret(np, false);
                            t.changed();
                        }
                    }
                }
                Action::TableDeleteCol => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        t.checkpoint(EditKind::Other, now);
                        if let Some(np) = t.doc.table_delete_col(fmt.table_id, fmt.table_col as usize) {
                            t.set_caret(np, false);
                            t.changed();
                        }
                    }
                }
                Action::TableMergeRight => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        t.checkpoint(EditKind::Other, now);
                        if t.doc.table_merge_right(fmt.table_id, fmt.table_row as usize, fmt.table_col as usize) {
                            t.changed();
                        }
                    }
                }
                Action::TableMergeRow => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        t.checkpoint(EditKind::Other, now);
                        if t.doc.table_merge_row(fmt.table_id, fmt.table_row as usize) {
                            t.changed();
                        }
                    }
                }
                Action::TableSplitCell => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        t.checkpoint(EditKind::Other, now);
                        if t.doc.table_split_cell(fmt.table_id, fmt.table_row as usize, fmt.table_col as usize) {
                            t.changed();
                        }
                    }
                }
                Action::TableCellShading(shd) => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        t.checkpoint(EditKind::Other, now);
                        if !t.selected_cells.is_empty() {
                            let sel = t.selected_cells.clone();
                            for (tid, r, c) in sel {
                                t.doc.table_set_cell_shading(tid, r as usize, c as usize, shd);
                            }
                        } else {
                            t.doc
                                .table_set_cell_shading(fmt.table_id, fmt.table_row as usize, fmt.table_col as usize, shd);
                        }
                        t.changed();
                    }
                }
                Action::TableRowShading(shd) => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        t.checkpoint(EditKind::Other, now);
                        t.doc.table_set_row_shading(fmt.table_id, fmt.table_row as usize, shd);
                        t.changed();
                    }
                }
                Action::TableCellVAlign(va) => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        t.checkpoint(EditKind::Other, now);
                        if !t.selected_cells.is_empty() {
                            let sel = t.selected_cells.clone();
                            for (tid, r, c) in sel {
                                t.doc.table_set_cell_valign(tid, r as usize, c as usize, va);
                            }
                        } else {
                            t.doc
                                .table_set_cell_valign(fmt.table_id, fmt.table_row as usize, fmt.table_col as usize, va);
                        }
                        t.changed();
                    }
                }
                Action::TableCellHAlign(al) => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        t.checkpoint(EditKind::Other, now);
                        if !t.selected_cells.is_empty() {
                            let sel = t.selected_cells.clone();
                            for (tid, r, c) in sel {
                                t.doc.table_set_cell_halign(tid, r as usize, c as usize, al);
                            }
                        } else {
                            t.doc
                                .table_set_cell_halign(fmt.table_id, fmt.table_row as usize, fmt.table_col as usize, al);
                        }
                        t.changed();
                    }
                }
                Action::TableToggleWrap => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        let cur_wrap = t
                            .doc
                            .table(fmt.table_id)
                            .and_then(|tb| tb.rows.get(fmt.table_row as usize))
                            .and_then(|rw| rw.cells.get(fmt.table_col as usize))
                            .is_none_or(|c| c.wrap_text);
                        let new_wrap = !cur_wrap;
                        t.checkpoint(EditKind::Other, now);
                        if !t.selected_cells.is_empty() {
                            let sel = t.selected_cells.clone();
                            for (tid, r, c) in sel {
                                t.doc.table_set_cell_wrap(tid, r as usize, c as usize, new_wrap);
                            }
                        } else {
                            t.doc
                                .table_set_cell_wrap(fmt.table_id, fmt.table_row as usize, fmt.table_col as usize, new_wrap);
                        }
                        t.changed();
                    }
                }
                Action::TableSelectRow => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        t.select_table_row(fmt.table_id, fmt.table_row as usize);
                    }
                }
                Action::TableSelectCol => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        t.select_table_col(fmt.table_id, fmt.table_col as usize);
                    }
                }
                Action::TableAutoFitCol => {
                    let fmt = {
                        let t = self.tab_ref();
                        t.doc.paras[t.doc.clamp(t.caret).p].fmt
                    };
                    if fmt.table_id > 0 {
                        let auto_w = layout::measure_column_autofit_width_in(
                            ctx,
                            &self.tab_ref().doc,
                            &self.book,
                            fmt.table_id,
                            fmt.table_row as usize,
                            fmt.table_col as usize,
                        );
                        if let Some(w_in) = auto_w {
                            let t = self.tab();
                            t.checkpoint(EditKind::Other, now);
                            t.doc
                                .table_resize_col(fmt.table_id, fmt.table_row as usize, fmt.table_col as usize, w_in);
                            t.changed();
                        }
                    }
                }
                Action::TableAutoFitRow => {
                    let fmt = {
                        let t = self.tab_ref();
                        t.doc.paras[t.doc.clamp(t.caret).p].fmt
                    };
                    if fmt.table_id > 0 {
                        let auto_h = layout::measure_row_autofit_height_pt(ctx, &self.tab_ref().doc, &self.book, fmt.table_id, fmt.table_row as usize);
                        if let Some(h_pt) = auto_h {
                            let t = self.tab();
                            t.checkpoint(EditKind::Other, now);
                            t.doc.table_resize_row(fmt.table_id, fmt.table_row as usize, h_pt);
                            t.changed();
                        }
                    }
                }
                Action::TableCellFormula(f_opt) => {
                    let t = self.tab();
                    let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
                    if fmt.table_id > 0 {
                        t.checkpoint(EditKind::Other, now);
                        t.doc
                            .table_set_cell_formula(fmt.table_id, fmt.table_row as usize, fmt.table_col as usize, f_opt);
                        let end_c = t.doc.paras.get(t.caret.p).map_or(0, |p| p.len());
                        t.set_caret(Pos::new(t.caret.p, end_c), false);
                        t.changed();
                    }
                }
                Action::SetPdfMode(m) => {
                    self.tab().set_pdf_mode(m);
                    self.toast(match m {
                        crate::editor::PdfMode::Read => "PDF Read mode".into(),
                        crate::editor::PdfMode::Edit => "PDF Edit mode — click any text on the page to edit".into(),
                        crate::editor::PdfMode::Reflow => "Switched to Reflowable document mode".into(),
                    });
                }
                Action::OpenHeadersFooters => {
                    self.show_hf = true;
                    self.right_panel = RightPanel::HeadersFooters;
                    refocus = false;
                }
                Action::OpenTableMath => {
                    let fmt = {
                        let t = self.tab_ref();
                        t.doc.paras[t.doc.clamp(t.caret).p].fmt
                    };
                    if fmt.table_id > 0 {
                        let r = fmt.table_row as usize;
                        let c = fmt.table_col as usize;
                        let mut op = TableMathOp::Sum;
                        let mut use_above = r > 0;
                        let mut use_left = r == 0 && c > 0;
                        let mut chosen = Vec::new();
                        let mut keep_sig_figs = true;
                        let mut expr: Option<String> = None;
                        if let Some(existing) = self
                            .tab_ref()
                            .doc
                            .table(fmt.table_id)
                            .and_then(|tb| tb.rows.get(r))
                            .and_then(|rw| rw.cells.get(c))
                            .and_then(|cl| cl.formula.clone())
                        {
                            let s = existing
                                .trim()
                                .strip_prefix('=')
                                .unwrap_or(existing.trim())
                                .trim();
                            // arithmetic formulas reopen in the arithmetic editor
                            let first = s
                                .split('(')
                                .next()
                                .unwrap_or("")
                                .trim()
                                .to_ascii_uppercase();
                            let aggregate = matches!(
                                first.as_str(),
                                "SUM" | "TOTAL" | "SUBTOTAL" | "AVERAGE" | "AVG" | "MEDIAN"
                            ) && s.find('(').is_some()
                                && s.ends_with(')')
                                && s.matches('(').count() == 1;
                            if !aggregate {
                                expr = Some(s.to_string());
                            } else if let (Some(op_i), Some(cl_i)) = (s.find('('), s.rfind(')')) {
                                let op_n = s[..op_i].trim().to_ascii_uppercase();
                                op = match op_n.as_str() {
                                    "SUBTOTAL" => TableMathOp::Subtotal,
                                    "AVERAGE" | "AVG" => TableMathOp::Average,
                                    "MEDIAN" => TableMathOp::Median,
                                    _ => TableMathOp::Sum,
                                };
                                let inside = s[op_i + 1..cl_i].trim();
                                let (args_s, sig) = if let Some((l, r_s)) = inside.rsplit_once(';') {
                                    (l.trim(), !r_s.trim().eq_ignore_ascii_case("RAW"))
                                } else {
                                    (inside, true)
                                };
                                keep_sig_figs = sig;
                                if args_s.eq_ignore_ascii_case("ABOVE") {
                                    use_above = true;
                                    use_left = false;
                                } else if args_s.eq_ignore_ascii_case("LEFT") {
                                    use_left = true;
                                    use_above = false;
                                } else {
                                    use_above = false;
                                    use_left = false;
                                    for tok in args_s.split(',') {
                                        if let Some(rc) = parse_cell_coord(tok) {
                                            chosen.push(rc);
                                        }
                                    }
                                }
                            }
                        }
                        self.dialog = Dialog::TableMath {
                            table_id: fmt.table_id,
                            row: r,
                            col: c,
                            op,
                            use_above,
                            use_left,
                            chosen,
                            keep_sig_figs,
                            expr,
                            picking_target: false,
                        };
                        refocus = false;
                    } else {
                        self.toast("Place the cursor inside a table cell first to add a calculation.".into());
                    }
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
                Cmd::PastePlain => Action::PastePlain,
                Cmd::NextTab => Action::NextTab,
                Cmd::ZoomIn => Action::ZoomIn,
                Cmd::ZoomOut => Action::ZoomOut,
                Cmd::Zoom100 => Action::SetZoom(1.0),
                Cmd::Link => {
                    let t = self.tab_ref();
                    let (txt, url) = if let Some((a, b, lid)) = t.doc.link_bounds(t.caret) {
                        (t.doc.text_range(a, b), t.doc.link_url(lid).unwrap_or("").to_string())
                    } else {
                        (t.selected_text(), String::new())
                    };
                    Action::Dialog(Dialog::Link { text: txt, url })
                }
                Cmd::Comment => Action::Dialog(Dialog::Comment(String::new())),
                Cmd::Footnote => Action::InsertFootnote,
                Cmd::WordCount => Action::Dialog(Dialog::WordCount),
                Cmd::Dictionary => Action::Dialog(Dialog::Dictionary),
                Cmd::CompactMode => Action::ToggleCompact,
                Cmd::SearchMenus => Action::Dialog(Dialog::SearchMenus(String::new())),
                Cmd::VersionHistory => Action::ToggleHistory,
                Cmd::AtMenu => Action::Dialog(Dialog::AtMenu(String::new())),
                Cmd::Checklist => Action::List(ListKind::Checklist),
                Cmd::BulletList => Action::List(ListKind::Bullet),
                Cmd::NumberList => Action::List(ListKind::Number),
                Cmd::DuplicatePara => Action::DuplicatePara,
                Cmd::OpenComments => Action::ToggleComments,
                Cmd::InsertImage => Action::InsertImage,
                Cmd::ImageProps(id) => Action::Dialog(Dialog::ImageProps(id)),
                Cmd::PageSetup => Action::OpenHeadersFooters,
            };
            self.actions.push(a);
        }
    }

    pub fn insert_image_from_path(&mut self, path: &Path, now: f64) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.toast(format!("Couldn't read image {}: {e}", path.display()));
                return;
            }
        };
        let name = path
            .file_name()
            .map_or_else(|| "Image".into(), |n| n.to_string_lossy().to_string());
        let next_id = self.tab_ref().doc.next_image_id();
        let max_w_in = {
            let pg = &self.tab_ref().doc.page;
            (pg.width - pg.margin_left - pg.margin_right).max(1.5)
        };
        match DocImage::from_file_bytes(next_id, name.clone(), &bytes, max_w_in) {
            Ok(img) => {
                self.tab().insert_image(img, now);
                self.toast(format!("Inserted image “{name}” — drag handles to resize or add a caption below."));
            }
            Err(e) => self.toast(e),
        }
    }

    pub fn replace_image_from_path(&mut self, img_id: u64, path: &Path, now: f64) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.toast(format!("Couldn't read image {}: {e}", path.display()));
                return;
            }
        };
        let name = path
            .file_name()
            .map_or_else(|| "Image".into(), |n| n.to_string_lossy().to_string());
        let max_w_in = {
            let pg = &self.tab_ref().doc.page;
            (pg.width - pg.margin_left - pg.margin_right).max(1.5)
        };
        match DocImage::from_file_bytes(img_id, name.clone(), &bytes, max_w_in) {
            Ok(new_img) => {
                let t = self.tab();
                t.checkpoint(EditKind::Other, now);
                if let Some(existing) = t.doc.image_mut(img_id) {
                    let keep_cap = existing.caption.clone();
                    let keep_w = existing.width_in;
                    let aspect = new_img.aspect_ratio();
                    *existing = new_img;
                    existing.caption = keep_cap;
                    existing.width_in = keep_w;
                    existing.height_in = (keep_w / aspect).clamp(0.4, 20.0);
                }
                t.changed();
                self.toast(format!("Replaced image with “{name}”."));
            }
            Err(e) => self.toast(e),
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
    config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("settings.json")).ok())
        .or_else(|| {
            dirs::config_dir()
                .and_then(|d| std::fs::read_to_string(d.join("ScribePro").join("settings.json")).ok())
        })
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn remove_recovery(id: u64) {
    if let Some(d) = recovery_dir() {
        let _ = std::fs::remove_file(d.join(format!("recovery-{id}.{FILE_EXT}")));
    }
}

/// Invokes the operating system's native Emoji Picker (Windows `Win+.`, macOS Character Viewer `Ctrl+Cmd+Space`,
/// or Linux `emote` / `plasma-emojier` / `gnome-characters` / `ibus emoji`).
pub fn open_system_emoji_picker() -> bool {
    #[cfg(target_os = "windows")]
    {
        #[link(name = "user32")]
        extern "system" {
            fn keybd_event(bVk: u8, bScan: u8, dwFlags: u32, dwExtraInfo: usize);
        }
        const VK_LWIN: u8 = 0x5B;
        const VK_OEM_PERIOD: u8 = 0xBE;
        const KEYEVENTF_KEYUP: u32 = 0x0002;
        // SAFETY: `keybd_event` is a standard User32 API that synthesizes virtual-key events
        // into the thread's input stream using scalar arguments and no raw pointers.
        unsafe {
            keybd_event(VK_LWIN, 0, 0, 0);
            keybd_event(VK_OEM_PERIOD, 0, 0, 0);
            keybd_event(VK_OEM_PERIOD, 0, KEYEVENTF_KEYUP, 0);
            keybd_event(VK_LWIN, 0, KEYEVENTF_KEYUP, 0);
        }
        true
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("osascript")
            .arg("-e")
            .arg("tell application \"System Events\" to keystroke \" \" using {command down, control down}")
            .spawn()
            .is_ok()
    }
    #[cfg(target_os = "linux")]
    {
        for (cmd, args) in [
            ("emote", &[][..]),
            ("plasma-emojier", &[][..]),
            ("gnome-characters", &[][..]),
            ("ibus", &["emoji"][..]),
        ] {
            if std::process::Command::new(cmd).args(args).spawn().is_ok() {
                return true;
            }
        }
        false
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

// ───────────────────────── small widgets ─────────────────────────

pub fn icon_button(ui: &mut egui::Ui, th: &Theme, icon: Icon, tip: &str, selected: bool, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let hovered = resp.hovered() && enabled;
    let bg = if selected {
        th.accent_soft
    } else if hovered {
        th.hover
    } else {
        Color32::TRANSPARENT
    };
    p.rect_filled(rect, 7.0, bg);
    if selected {
        p.rect_stroke(rect, 7.0, Stroke::new(1.0, th.accent.gamma_multiply(0.45)), egui::StrokeKind::Inside);
    }
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
    let (r, _) = ui.allocate_exact_size(vec2(7.0, 22.0), Sense::hover());
    ui.painter().line_segment([r.center_top() + vec2(0.0, 3.0), r.center_bottom() - vec2(0.0, 3.0)], Stroke::new(1.0, th.border));
}

pub const PALETTE: &[[u8; 3]] = &[
    [0, 0, 0],
    [67, 67, 67],
    [102, 102, 102],
    [153, 153, 153],
    [255, 255, 255],
    [152, 0, 0],
    [255, 0, 0],
    [255, 153, 0],
    [255, 255, 0],
    [0, 255, 0],
    [0, 255, 255],
    [74, 134, 232],
    [0, 0, 255],
    [153, 0, 255],
    [255, 0, 255],
    [204, 65, 37],
    [224, 102, 102],
    [246, 178, 107],
    [255, 217, 102],
    [147, 196, 125],
    [118, 165, 175],
    [109, 158, 235],
    [111, 168, 220],
    [142, 124, 195],
    [194, 123, 160],
];
pub const HIGHLIGHTS: &[[u8; 3]] = &[
    [254, 240, 138],
    [187, 247, 208],
    [191, 219, 254],
    [251, 207, 232],
    [254, 215, 170],
    [221, 214, 254],
    [165, 243, 252],
    [229, 231, 235],
];

/// Craft-inspired left-to-right fading text color presets `(name, left_rgb, right_rgb)`.
pub const CRAFT_GRADIENT_TEXT: &[(&str, [u8; 3], [u8; 3])] = &[
    ("Sunset (Orange → Rose)", [234, 88, 12], [219, 39, 119]),
    ("Ocean (Blue → Cyan)", [37, 99, 235], [6, 182, 212]),
    ("Berry (Violet → Pink)", [124, 58, 237], [236, 72, 153]),
    ("Forest (Emerald → Teal)", [5, 150, 105], [13, 148, 136]),
    ("Twilight (Indigo → Purple)", [29, 78, 216], [147, 51, 234]),
    ("Fire (Crimson → Amber)", [220, 38, 38], [217, 119, 6]),
    ("Aurora (Teal → Royal)", [13, 148, 136], [37, 99, 235]),
    ("Dusk (Slate → Indigo)", [30, 41, 59], [79, 70, 229]),
];

/// Craft-inspired left-to-right fading highlight presets `(name, left_rgb, right_rgb)`.
pub const CRAFT_GRADIENT_HIGHLIGHTS: &[(&str, [u8; 3], [u8; 3])] = &[
    ("Sunset (Peach → Rose)", [254, 215, 170], [251, 207, 232]),
    ("Sunrise (Lemon → Coral)", [254, 240, 138], [254, 202, 202]),
    ("Mint (Mint → Aqua)", [187, 247, 208], [165, 243, 252]),
    ("Ocean (Aqua → Periwinkle)", [165, 243, 252], [199, 210, 254]),
    ("Twilight (Sky → Lavender)", [191, 219, 254], [233, 213, 255]),
    ("Berry (Rose → Lavender)", [251, 207, 232], [221, 214, 254]),
    ("Meadow (Lemon → Mint)", [254, 240, 138], [187, 247, 208]),
    ("Glow Fade (Lemon → Cream)", [254, 240, 138], [255, 253, 242]),
];

pub fn swatch(ui: &mut egui::Ui, c: [u8; 3], selected: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
    ui.painter().rect_filled(r.shrink(1.0), 10.0, layout::color(c));
    let st = if selected || resp.hovered() { Stroke::new(2.0, Color32::from_rgb(11, 87, 208)) } else { Stroke::new(1.0, Color32::from_gray(200)) };
    ui.painter().rect_stroke(r.shrink(1.0), 10.0, st, egui::StrokeKind::Inside);
    resp
}

pub fn gradient_swatch(ui: &mut egui::Ui, name: &str, c1: [u8; 3], c2: [u8; 3], selected: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(54.0, 22.0), Sense::click());
    let inner = r.shrink(1.5);
    layout::paint_gradient_pill(ui.painter(), inner, c1, c2, 6.0);
    let st = if selected || resp.hovered() {
        Stroke::new(2.0, Color32::from_rgb(11, 87, 208))
    } else {
        Stroke::new(1.0, Color32::from_gray(195))
    };
    ui.painter().rect_stroke(inner, 6.0, st, egui::StrokeKind::Inside);
    let short = name.split(" (").next().unwrap_or(name);
    ui.painter().text(
        inner.center(),
        egui::Align2::CENTER_CENTER,
        short,
        egui::FontId::proportional(10.5),
        Color32::from_rgb(45, 50, 60),
    );
    resp.on_hover_text(name)
}

pub fn text_gradient_swatch(ui: &mut egui::Ui, name: &str, c1: [u8; 3], c2: [u8; 3], selected: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(54.0, 22.0), Sense::click());
    let inner = r.shrink(1.5);
    ui.painter().rect_filled(inner, 6.0, Color32::from_rgb(248, 250, 252));
    let bar = egui::Rect::from_min_max(egui::pos2(inner.min.x + 3.0, inner.max.y - 3.5), egui::pos2(inner.max.x - 3.0, inner.max.y - 1.5));
    layout::paint_gradient_pill(ui.painter(), bar, c1, c2, 1.0);
    let st = if selected || resp.hovered() {
        Stroke::new(2.0, Color32::from_rgb(11, 87, 208))
    } else {
        Stroke::new(1.0, Color32::from_gray(195))
    };
    ui.painter().rect_stroke(inner, 6.0, st, egui::StrokeKind::Inside);
    let short = name.split(" (").next().unwrap_or(name);
    let mut job = egui::text::LayoutJob::default();
    let n = short.chars().count().max(1);
    for (idx, ch) in short.chars().enumerate() {
        let t = if n <= 1 { 0.5 } else { idx as f32 / (n - 1) as f32 };
        let col = layout::color(doc::lerp_rgb(c1, c2, t));
        job.append(
            &ch.to_string(),
            0.0,
            egui::text::TextFormat {
                font_id: egui::FontId::proportional(10.5),
                color: col,
                ..Default::default()
            },
        );
    }
    let g = ui.fonts_mut(|f| f.layout_job(job));
    let pos = egui::pos2(inner.center().x - g.size().x * 0.5, inner.center().y - g.size().y * 0.5 - 1.0);
    ui.painter().galley(pos, g, Color32::BLACK);
    resp.on_hover_text(name)
}

pub fn icon_text_color_button(ui: &mut egui::Ui, th: &Theme, tip: &str, c1: [u8; 3], c2: Option<[u8; 3]>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    let p = ui.painter();
    let bg = if resp.hovered() { th.hover } else { Color32::TRANSPARENT };
    p.rect_filled(rect, 6.0, bg);
    icons::draw(p, rect.shrink(5.0), Icon::TextColor, th.icon, layout::color(c1));
    if let Some(b) = c2 {
        if b != c1 {
            let inner = rect.shrink(5.0);
            let bar = egui::Rect::from_min_max(
                egui::pos2(inner.min.x + inner.width() * 0.12, inner.min.y + inner.height() * 0.78),
                egui::pos2(inner.min.x + inner.width() * 0.88, inner.min.y + inner.height() * 0.94),
            );
            layout::paint_gradient_pill(p, bar, c1, b, 1.5);
        }
    }
    resp.on_hover_text(tip)
}

pub fn icon_highlight_button(ui: &mut egui::Ui, th: &Theme, tip: &str, c1: Option<[u8; 3]>, c2: Option<[u8; 3]>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    let p = ui.painter();
    let bg = if resp.hovered() { th.hover } else { Color32::TRANSPARENT };
    p.rect_filled(rect, 6.0, bg);
    let base_col = c1.map_or(Color32::from_rgb(250, 204, 21), layout::color);
    icons::draw(p, rect.shrink(5.0), Icon::Highlight, th.icon, base_col);
    if let (Some(a), Some(b)) = (c1, c2) {
        if a != b {
            let inner = rect.shrink(5.0);
            let bar = egui::Rect::from_min_max(
                egui::pos2(inner.min.x + inner.width() * 0.12, inner.min.y + inner.height() * 0.78),
                egui::pos2(inner.min.x + inner.width() * 0.88, inner.min.y + inner.height() * 0.94),
            );
            layout::paint_gradient_pill(p, bar, a, b, 1.5);
        }
    }
    resp.on_hover_text(tip)
}

// ───────────────────────── frame loop ─────────────────────────

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frame += 1;
        let now = ctx.input(|i| i.time);
        self.poll_updater(&ctx);
        self.poll_doc_load(&ctx);

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
        if ctx.input(|i| i.viewport().close_requested()) && self.close_flow != CloseFlow::ForceQuit && self.tabs.iter().any(|t| t.dirty) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_flow = CloseFlow::ConfirmQuit;
        }

        // Esc exits clean screen
        if self.clean_screen && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.clean_screen = false;
        }

        // Drag-and-drop image or document files from OS file explorer
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for df in dropped {
            let p = df.path();
            if !p.as_os_str().is_empty() {
                let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "ico" | "tga") {
                    self.insert_image_from_path(p, now);
                } else {
                    self.open_path_async(p);
                }
            }
        }

        // window title, like "Syllabary v0.2.1 - [Untitled1]"
        let title = format!("{APP_NAME} v{} - [{}]", crate::updater::CURRENT_VERSION, self.tab_ref().title());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        let th = self.theme.clone();

        if !self.clean_screen && !self.compact_mode {
            egui::Panel::top("menubar")
                .frame(
                    Frame::new()
                        .fill(th.chrome)
                        .inner_margin(Margin { left: 14, right: 14, top: 7, bottom: 3 }),
                )
                .show(ui, |ui| {
                    self.docs_header(ui, &th);
                });
        }

        if self.show_toolbar && !self.clean_screen {
            egui::Panel::top("toolbar")
                .max_size(140.0)
                .frame(
                    Frame::new()
                        .fill(th.chrome)
                        .stroke(Stroke::new(1.0, th.border.gamma_multiply(0.65)))
                        .inner_margin(Margin::symmetric(14, 6)),
                )
                .show(ui, |ui| {
                    Frame::new()
                        .fill(th.toolbar)
                        .stroke(Stroke::new(1.0, th.border))
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 2],
                            blur: 8,
                            spread: 0,
                            color: if th.dark {
                                Color32::from_black_alpha(65)
                            } else {
                                Color32::from_black_alpha(12)
                            },
                        })
                        .corner_radius(egui::CornerRadius::same(12))
                        .inner_margin(Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(2.5, 4.0);
                                self.toolbar(ui, &th);
                            });
                        });
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
                .default_size(245.0)
                .frame(Frame::new().fill(th.chrome).inner_margin(Margin::symmetric(10, 8)).stroke(Stroke::new(1.0, th.border)))
                .show(ui, |ui| {
                    self.nav_pane(ui, &th);
                });
        }

        if !self.clean_screen {
            match self.right_panel {
                RightPanel::Comments => {
                    egui::Panel::right("comments")
                        .resizable(true)
                        .default_size(280.0)
                        .frame(Frame::new().fill(th.chrome).inner_margin(Margin::symmetric(10, 8)).stroke(Stroke::new(1.0, th.border)))
                        .show(ui, |ui| {
                            self.comments_pane(ui, &th);
                        });
                }
                RightPanel::VersionHistory => {
                    egui::Panel::right("version-history")
                        .resizable(true)
                        .default_size(290.0)
                        .min_size(240.0)
                        .max_size(420.0)
                        .frame(Frame::new().fill(th.chrome).inner_margin(Margin::symmetric(10, 8)).stroke(Stroke::new(1.0, th.border)))
                        .show(ui, |ui| {
                            self.history_pane(ui, &th, now);
                        });
                }
                RightPanel::HeadersFooters => {
                    egui::Panel::right("headers-footers-panel")
                        .resizable(true)
                        .default_size(310.0)
                        .min_size(260.0)
                        .max_size(440.0)
                        .frame(Frame::new().fill(th.chrome).inner_margin(Margin::symmetric(12, 10)).stroke(Stroke::new(1.0, th.border)))
                        .show(ui, |ui| {
                            self.headers_footers_pane(ui, &th, now);
                        });
                }
                RightPanel::PageSetup => {
                    egui::Panel::right("page-setup-panel")
                        .resizable(true)
                        .default_size(300.0)
                        .min_size(250.0)
                        .max_size(420.0)
                        .frame(Frame::new().fill(th.chrome).inner_margin(Margin::symmetric(12, 10)).stroke(Stroke::new(1.0, th.border)))
                        .show(ui, |ui| {
                            self.page_setup_pane(ui, &th, now);
                        });
                }
                RightPanel::Settings => {
                    egui::Panel::right("settings-panel")
                        .resizable(true)
                        .default_size(340.0)
                        .min_size(280.0)
                        .max_size(460.0)
                        .frame(Frame::new().fill(th.chrome).inner_margin(Margin::symmetric(12, 10)).stroke(Stroke::new(1.0, th.border)))
                        .show(ui, |ui| {
                            self.settings_pane(ui, &th);
                        });
                }
                RightPanel::None => {}
            }
        }

        // Docked Right-Hand Table Studio Panel (pops out when caret is inside a table, without overlapping the document or scrollbar)
        let active_tbl_info = if matches!(self.dialog, Dialog::None) && !self.clean_screen && self.tab_ref().selected_image.is_none() {
            let t = self.tab_ref();
            t.active_table_cell_rect.map(|(tid, crow, ccol, cell_r, tbl_r)| {
                let r = crow as usize;
                let c = ccol as usize;
                let cell_spec = t
                    .doc
                    .table(tid)
                    .and_then(|tb| tb.rows.get(r))
                    .and_then(|rw| rw.cells.get(c))
                    .cloned()
                    .unwrap_or_default();
                let can_merge_right = t
                    .doc
                    .table(tid)
                    .and_then(|tb| tb.rows.get(r))
                    .is_some_and(|rw| c + 1 < rw.cells.len());
                let can_merge_row = t
                    .doc
                    .table(tid)
                    .and_then(|tb| tb.rows.get(r))
                    .is_some_and(|rw| rw.cells.len() > 1);
                let h_align = t.doc.paras.get(t.doc.clamp(t.caret).p).map_or(Align::Left, |p| p.fmt.align);
                let sel_count = t.selected_cells.len();
                (tid, r, c, cell_r, tbl_r, cell_spec, h_align, can_merge_right, can_merge_row, sel_count)
            })
        } else {
            None
        };
        if let Some((_tid, r, c, _cell_r, _tbl_r, cell_spec, cur_halign, can_merge_right, can_merge_row, sel_count)) = active_tbl_info {
            let coord_lbl = if sel_count > 1 {
                format!("{sel_count} cells")
            } else {
                format!("Cell {}", cell_coord_label(r, c))
            };
            if !self.table_panel_collapsed {
                egui::Panel::right("table-studio-panel")
                    .resizable(true)
                    .default_size(244.0)
                    .min_size(216.0)
                    .max_size(340.0)
                    .frame(Frame::new().fill(th.chrome).inner_margin(Margin::symmetric(12, 10)).stroke(Stroke::new(1.0, th.border)))
                    .show(ui, |ui| {
                    egui::ScrollArea::vertical().id_salt("table-studio-scroll").show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let (ir, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                            icons::draw(ui.painter(), ir, Icon::Table, th.accent, th.accent);
                            ui.label(RichText::new("Table").size(13.0).color(th.text).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(coord_lbl).size(11.5).color(th.accent).strong());
                            });
                        });
                        if let Some(f) = &cell_spec.formula {
                            ui.label(RichText::new(f).size(11.0).color(th.subtle).monospace());
                        }
                        ui.separator();

                        // Shading
                        ui.label(RichText::new("Cell Shading").size(11.5).color(th.subtle).strong());
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
                            for &(name, rgb_opt) in dialogs::TABLE_SHADING_SWATCHES {
                                let sel = cell_spec.shading == rgb_opt;
                                let (sr, sresp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                                let fill = rgb_opt.map_or(Color32::WHITE, |rgb| Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                                ui.painter().rect_filled(sr.shrink(1.0), 3.0, fill);
                                if rgb_opt.is_none() {
                                    ui.painter().line_segment(
                                        [sr.shrink(4.0).left_bottom(), sr.shrink(4.0).right_top()],
                                        Stroke::new(1.2, Color32::from_rgb(220, 38, 38)),
                                    );
                                }
                                let st = if sel { Stroke::new(1.8, th.accent) } else { Stroke::new(1.0, th.border) };
                                ui.painter().rect_stroke(sr.shrink(1.0), 3.0, st, egui::StrokeKind::Inside);
                                if sresp.on_hover_text(format!("Shade cell(s): {name}")).clicked() {
                                    self.act(Action::TableCellShading(rgb_opt));
                                }
                            }
                            if ui
                                .small_button("Apply Row")
                                .on_hover_text("Apply current cell's shading across the entire row")
                                .clicked()
                            {
                                self.act(Action::TableRowShading(cell_spec.shading));
                            }
                        });
                        ui.separator();

                        // Alignment & Wrap
                        ui.label(RichText::new("Alignment & Wrap").size(11.5).color(th.subtle).strong());
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if icon_button(ui, &th, Icon::AlignLeft, "Align cell text left", cur_halign == Align::Left, true).clicked() {
                                self.act(Action::TableCellHAlign(Align::Left));
                            }
                            if icon_button(ui, &th, Icon::AlignCenter, "Center cell text horizontally", cur_halign == Align::Center, true).clicked() {
                                self.act(Action::TableCellHAlign(Align::Center));
                            }
                            if icon_button(ui, &th, Icon::AlignRight, "Align cell text right", cur_halign == Align::Right, true).clicked() {
                                self.act(Action::TableCellHAlign(Align::Right));
                            }
                            ui.separator();
                            if ui
                                .selectable_label(cell_spec.wrap_text, RichText::new("↩ Wrap").size(11.5))
                                .on_hover_text("Toggle text wrapping in cell(s)")
                                .clicked()
                            {
                                self.act(Action::TableToggleWrap);
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(RichText::new("Vertical:").size(11.0).color(th.subtle));
                            for (va, lbl, tip) in [
                                (CellVAlign::Top, "Top", "Align cell text to top"),
                                (CellVAlign::Center, "Mid", "Center cell text vertically"),
                                (CellVAlign::Bottom, "Bot", "Align cell text to bottom"),
                            ] {
                                if ui
                                    .selectable_label(cell_spec.v_align == va, RichText::new(lbl).size(11.5))
                                    .on_hover_text(tip)
                                    .clicked()
                                {
                                    self.act(Action::TableCellVAlign(va));
                                }
                            }
                        });
                        ui.separator();

                        // Selection, Auto-Fit, Merge & Split
                        ui.label(RichText::new("Cells & Merge").size(11.5).color(th.subtle).strong());
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                            if ui
                                .small_button("Sel Row")
                                .on_hover_text("Select the entire current row (or click row number on left)")
                                .clicked()
                            {
                                self.act(Action::TableSelectRow);
                            }
                            if ui
                                .small_button("Sel Col")
                                .on_hover_text("Select the entire current column (or click column letter on top)")
                                .clicked()
                            {
                                self.act(Action::TableSelectCol);
                            }
                            if ui
                                .small_button("↔ Fit")
                                .on_hover_text("Auto-fit column width & row height to contents (or double-click cell edge)")
                                .clicked()
                            {
                                self.act(Action::TableAutoFitCol);
                                self.act(Action::TableAutoFitRow);
                            }
                            if ui
                                .add_enabled(can_merge_right, egui::Button::new(RichText::new("Merge →").size(11.0)))
                                .on_hover_text("Merge this cell with the cell to its right")
                                .clicked()
                            {
                                self.act(Action::TableMergeRight);
                            }
                            if ui
                                .add_enabled(can_merge_row, egui::Button::new(RichText::new("Merge Row").size(11.0)))
                                .on_hover_text("Merge all cells in this row into a single banner cell")
                                .clicked()
                            {
                                self.act(Action::TableMergeRow);
                            }
                            if ui
                                .add_enabled(cell_spec.grid_span > 1, egui::Button::new(RichText::new("Split").size(11.0)))
                                .on_hover_text("Split this merged cell back into individual columns")
                                .clicked()
                            {
                                self.act(Action::TableSplitCell);
                            }
                        });
                        ui.separator();

                        // Rows & Columns
                        ui.label(RichText::new("Rows & Columns").size(11.5).color(th.subtle).strong());
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                            if ui.small_button("+Row ↑").on_hover_text("Insert row above").clicked() {
                                self.act(Action::TableInsertRow { above: true });
                            }
                            if ui.small_button("+Row ↓").on_hover_text("Insert row below").clicked() {
                                self.act(Action::TableInsertRow { above: false });
                            }
                            if ui.small_button("−Row").on_hover_text("Delete current row").clicked() {
                                self.act(Action::TableDeleteRow);
                            }
                            if ui.small_button("+Col →").on_hover_text("Insert column to the right").clicked() {
                                self.act(Action::TableInsertCol { left: false });
                            }
                            if ui.small_button("−Col").on_hover_text("Delete current column").clicked() {
                                self.act(Action::TableDeleteCol);
                            }
                        });
                        ui.separator();

                        // Cell Math
                        ui.label(RichText::new("Formulas").size(11.5).color(th.subtle).strong());
                        let dir = if r > 0 { "ABOVE" } else { "LEFT" };
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                            for (lbl, fname, tip) in [
                                ("∑ Sum", "SUM", "Calculate Sum (Total) keeping input significant figures"),
                                ("Subtotal", "SUBTOTAL", "Calculate section Subtotal keeping input significant figures"),
                                ("Avg", "AVERAGE", "Calculate Average keeping input significant figures"),
                                ("Median", "MEDIAN", "Calculate Median keeping input significant figures"),
                            ] {
                                if ui.small_button(lbl).on_hover_text(tip).clicked() {
                                    self.act(Action::TableCellFormula(Some(format!("={fname}({dir})"))));
                                }
                            }
                            if ui
                                .button(RichText::new("∑ Math…").size(11.0).color(th.accent).strong())
                                .on_hover_text("Choose specific cells for Sum, Subtotal, Average, or Median")
                                .clicked()
                            {
                                self.act(Action::OpenTableMath);
                            }
                        });
                    });
                });
            }

            // Small docked tab on the pop-out edge to hide/show the Table Studio panel without overlapping the document or scrollbar
            egui::Panel::right("table-studio-tab")
                .resizable(false)
                .default_size(18.0)
                .min_size(18.0)
                .max_size(18.0)
                .frame(Frame::new().fill(th.workspace).inner_margin(Margin::ZERO))
                .show(ui, |ui| {
                    let avail = ui.available_rect_before_wrap();
                    let tab_h = 68.0_f32.min(avail.height().max(24.0));
                    let tab_rect = egui::Rect::from_center_size(
                        pos2(avail.center().x, avail.center().y),
                        vec2(18.0, tab_h),
                    );
                    let resp = ui.interact(tab_rect, egui::Id::unique("table-studio-toggle-tab"), Sense::click());
                    let bg = if resp.hovered() { th.hover } else { th.chrome };
                    let cr = egui::CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 };
                    ui.painter().rect_filled(tab_rect, cr, bg);
                    ui.painter().rect_stroke(tab_rect, cr, Stroke::new(1.0, th.border), egui::StrokeKind::Inside);
                    let glyph = if self.table_panel_collapsed { "‹" } else { "›" };
                    let glyph_col = if resp.hovered() || self.table_panel_collapsed { th.accent } else { th.text };
                    ui.painter().text(
                        tab_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        glyph,
                        egui::FontId::proportional(15.0),
                        glyph_col,
                    );
                    let tip = if self.table_panel_collapsed { "Show Table panel" } else { "Hide Table panel" };
                    if resp.on_hover_text(tip).clicked() {
                        self.table_panel_collapsed = !self.table_panel_collapsed;
                    }
                });
        }

        // Contextual right-hand PDF Editor side panel when editing an exact PDF
        let pdf_edit_active = !self.clean_screen
            && self.tab_ref().pdf_mode() == Some(crate::editor::PdfMode::Edit);
        if pdf_edit_active {
            if !self.pdf_panel_collapsed {
                egui::Panel::right("pdf-studio-panel")
                    .resizable(true)
                    .default_size(248.0)
                    .min_size(220.0)
                    .max_size(340.0)
                    .frame(
                        Frame::new()
                            .fill(th.chrome)
                            .inner_margin(Margin::symmetric(12, 10))
                            .stroke(Stroke::new(1.0, th.border)),
                    )
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("pdf-studio-scroll")
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 8.0;

                                let (page_count, adding_box, sel_id, can_undo, can_redo, sel_info) = {
                                    let pv = self.tab_ref().pdf_view.as_ref();
                                    match pv {
                                        Some(pv) => {
                                            let info = pv.selected_block.and_then(|id| {
                                                pv.blocks.iter().find(|b| b.id == id && !b.deleted).map(|b| {
                                                    let (pw, ph) = pv
                                                        .page_dims_pt
                                                        .get(b.page)
                                                        .copied()
                                                        .unwrap_or((612.0, 792.0));
                                                    (
                                                        b.size_pt, b.bold, b.italic, b.align, b.color,
                                                        b.x_pt, b.y_pt, b.w_pt, pw, ph, b.page,
                                                    )
                                                })
                                            });
                                            (
                                                pv.page_dims_pt.len(),
                                                pv.adding_text_box,
                                                pv.selected_block,
                                                !pv.undo.is_empty(),
                                                !pv.redo.is_empty(),
                                                info,
                                            )
                                        }
                                        None => (0, false, None, false, false, None),
                                    }
                                };

                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("PDF Editor").strong().size(14.0));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(
                                            RichText::new(format!(
                                                "{page_count} pg{}",
                                                if page_count == 1 { "" } else { "s" }
                                            ))
                                            .small()
                                            .weak(),
                                        );
                                    });
                                });
                                ui.separator();

                                // Insert new text box
                                let add_btn = ui.add_sized(
                                    vec2(ui.available_width(), 28.0),
                                    egui::Button::new(
                                        RichText::new(if adding_box {
                                            "Click on PDF Page to Place…"
                                        } else {
                                            "+ Add Text Box"
                                        })
                                        .strong(),
                                    )
                                    .selected(adding_box),
                                );
                                if add_btn
                                    .on_hover_text("Click anywhere on a PDF page to insert a new resizable text box")
                                    .clicked()
                                {
                                    if let Some(pv) = self.tab().pdf_view.as_mut() {
                                        pv.adding_text_box = !pv.adding_text_box;
                                        if pv.adding_text_box {
                                            pv.selected_block = None;
                                        }
                                    }
                                }

                                ui.horizontal(|ui| {
                                    if ui.add_enabled(can_undo, egui::Button::new("Undo")).clicked() {
                                        self.tab().pdf_undo();
                                    }
                                    if ui.add_enabled(can_redo, egui::Button::new("Redo")).clicked() {
                                        self.tab().pdf_redo();
                                    }
                                });

                                ui.separator();

                                if let (
                                    Some(id),
                                    Some((
                                        cur_sz,
                                        cur_bold,
                                        cur_italic,
                                        cur_align,
                                        cur_col,
                                        cur_x,
                                        cur_y,
                                        cur_w,
                                        pw,
                                        ph,
                                        pg_idx,
                                    )),
                                ) = (sel_id, sel_info)
                                {
                                    ui.label(
                                        RichText::new(format!("Selected Text Box (Page {})", pg_idx + 1))
                                            .size(11.5)
                                            .color(th.subtle)
                                            .strong(),
                                    );

                                    // Typography: Size, Bold, Italic
                                    let mut new_sz = cur_sz;
                                    let mut toggle_bold = false;
                                    let mut toggle_italic = false;
                                    ui.horizontal(|ui| {
                                        if ui.small_button("A−").clicked() {
                                            new_sz = (cur_sz - 1.0).clamp(6.0, 96.0);
                                        }
                                        ui.add(
                                            egui::DragValue::new(&mut new_sz)
                                                .speed(0.25)
                                                .range(6.0..=96.0)
                                                .fixed_decimals(0)
                                                .suffix(" pt"),
                                        );
                                        if ui.small_button("A+").clicked() {
                                            new_sz = (cur_sz + 1.0).clamp(6.0, 96.0);
                                        }
                                        if ui.selectable_label(cur_bold, RichText::new("B").strong()).clicked() {
                                            toggle_bold = true;
                                        }
                                        if ui.selectable_label(cur_italic, RichText::new("I").italics()).clicked() {
                                            toggle_italic = true;
                                        }
                                    });

                                    // Alignment (Left, Center, Right, Justify)
                                    let mut new_align = None;
                                    ui.label(RichText::new("Alignment").size(11.5).color(th.subtle));
                                    ui.horizontal(|ui| {
                                        for (al, lbl) in [
                                            (Align::Left, "Left"),
                                            (Align::Center, "Center"),
                                            (Align::Right, "Right"),
                                            (Align::Justify, "Justify"),
                                        ] {
                                            if ui.selectable_label(cur_align == al, lbl).clicked() {
                                                new_align = Some(al);
                                            }
                                        }
                                    });

                                    // Text Color swatches
                                    let mut new_color = None;
                                    ui.label(RichText::new("Text Color").size(11.5).color(th.subtle));
                                    ui.horizontal(|ui| {
                                        for sw in [
                                            [0u8, 0, 0],
                                            [68, 72, 82],
                                            [24, 72, 168],
                                            [185, 28, 28],
                                            [22, 101, 52],
                                            [255, 255, 255],
                                        ] {
                                            let (sr, sresp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                                            ui.painter().rect_filled(sr, 4.0, Color32::from_rgb(sw[0], sw[1], sw[2]));
                                            let stroke = if sw == cur_col {
                                                Stroke::new(2.0, th.accent)
                                            } else {
                                                Stroke::new(1.0, th.border)
                                            };
                                            ui.painter().rect_stroke(sr, 4.0, stroke, egui::StrokeKind::Inside);
                                            if sresp.clicked() {
                                                new_color = Some(sw);
                                            }
                                        }
                                    });

                                    // Position & Width
                                    ui.label(RichText::new("Position & Size").size(11.5).color(th.subtle));
                                    let mut nx = cur_x;
                                    let mut ny = cur_y;
                                    let mut nw = cur_w;
                                    let mut pos_or_w_changed = false;
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("X:").weak().small());
                                        if ui
                                            .add(
                                                egui::DragValue::new(&mut nx)
                                                    .speed(0.5)
                                                    .range(0.0..=(pw - 20.0).max(0.0))
                                                    .fixed_decimals(0)
                                                    .suffix(" pt"),
                                            )
                                            .changed()
                                        {
                                            pos_or_w_changed = true;
                                        }
                                        ui.label(RichText::new("Y:").weak().small());
                                        if ui
                                            .add(
                                                egui::DragValue::new(&mut ny)
                                                    .speed(0.5)
                                                    .range(0.0..=(ph - 14.0).max(0.0))
                                                    .fixed_decimals(0)
                                                    .suffix(" pt"),
                                            )
                                            .changed()
                                        {
                                            pos_or_w_changed = true;
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Width:").weak().small());
                                        if ui
                                            .add(
                                                egui::DragValue::new(&mut nw)
                                                    .speed(1.0)
                                                    .range(40.0..=(pw - nx).max(40.0))
                                                    .fixed_decimals(0)
                                                    .suffix(" pt"),
                                            )
                                            .changed()
                                        {
                                            pos_or_w_changed = true;
                                        }
                                    });
                                    ui.label(
                                        RichText::new("Tip: drag ⋮⋮ Move or the right-edge handle directly on the page.")
                                            .weak()
                                            .small(),
                                    );

                                    let mut delete_blk = false;
                                    ui.add_space(4.0);
                                    if ui
                                        .add_sized(
                                            vec2(ui.available_width(), 26.0),
                                            egui::Button::new(RichText::new("Delete Text Block").color(Color32::from_rgb(185, 28, 28))),
                                        )
                                        .clicked()
                                    {
                                        delete_blk = true;
                                    }

                                    let sz_changed = (new_sz - cur_sz).abs() > 0.01;
                                    if sz_changed
                                        || toggle_bold
                                        || toggle_italic
                                        || new_align.is_some()
                                        || new_color.is_some()
                                        || pos_or_w_changed
                                        || delete_blk
                                    {
                                        let tab = self.tab();
                                        if let Some(pv) = tab.pdf_view.as_mut() {
                                            if !pos_or_w_changed || pv.editing_checkpointed != Some(id) {
                                                pv.checkpoint();
                                                if pos_or_w_changed {
                                                    pv.editing_checkpointed = Some(id);
                                                }
                                            }
                                            if let Some(b) = pv.blocks.iter_mut().find(|b| b.id == id) {
                                                if delete_blk {
                                                    b.deleted = true;
                                                    b.modified = true;
                                                    pv.selected_block = None;
                                                } else {
                                                    if sz_changed {
                                                        b.size_pt = new_sz;
                                                    }
                                                    if toggle_bold {
                                                        b.bold = !b.bold;
                                                    }
                                                    if toggle_italic {
                                                        b.italic = !b.italic;
                                                    }
                                                    if let Some(al) = new_align {
                                                        b.align = al;
                                                    }
                                                    if let Some(c) = new_color {
                                                        b.color = c;
                                                    }
                                                    if pos_or_w_changed {
                                                        b.x_pt = nx;
                                                        b.y_pt = ny;
                                                        b.w_pt = nw;
                                                    }
                                                    b.modified = true;
                                                }
                                            }
                                        }
                                        tab.rev += 1;
                                        tab.dirty = true;
                                    }
                                } else {
                                    ui.label(
                                        RichText::new("Click any text on the PDF page to edit, move, resize, or align it — or click + Add Text Box above.")
                                            .weak()
                                            .small(),
                                    );
                                }

                                ui.add_space(8.0);
                                ui.separator();
                                ui.label(RichText::new("Document").size(11.5).color(th.subtle).strong());
                                if ui
                                    .add_sized(
                                        vec2(ui.available_width(), 26.0),
                                        egui::Button::new("Convert to Reflowable Doc"),
                                    )
                                    .on_hover_text("Switch to full word-processor reflow editing")
                                    .clicked()
                                {
                                    self.act(Action::SetPdfMode(crate::editor::PdfMode::Reflow));
                                }
                            });
                    });
            }

            // Small docked tab on the right edge to hide/show the PDF Editor panel
            let mr = self.mr_palette();
            egui::Panel::right("pdf-studio-tab")
                .resizable(false)
                .default_size(18.0)
                .min_size(18.0)
                .max_size(18.0)
                .frame(Frame::new().fill(th.workspace).inner_margin(Margin::ZERO))
                .show(ui, |ui| {
                    modernrust::edge_collapse_tab(ui, &mr, &mut self.pdf_panel_collapsed, "PDF Editor panel");
                });
        }

        let mut cmds = Vec::new();
        let mut caret_screen_rect = None;
        egui::CentralPanel::default().frame(Frame::new().fill(th.workspace)).show(ui, |ui| {
            let full = ui.available_rect_before_wrap();
            self.view_size = full.size();
            let zoom = self.effective_zoom();
            if self.show_ruler && !self.clean_screen && !self.draft && !self.tab_ref().doc.pageless {
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
            let sc = SmartCorrect {
                enabled: self.cfg.smartcorrect,
                quotes: self.cfg.smart_quotes,
                markdown: self.cfg.markdown_auto,
                auto_links: self.cfg.auto_links,
                table: &table,
            };
            let focused = ui.memory(|m| m.focused());
            let doc_has_keys = (focused.is_none() || focused == Some(doc_id)) && !self.close_flow.is_confirming() && !self.editing_title;
            if doc_has_keys && focused.is_none() && matches!(self.dialog, Dialog::None) {
                ui.memory_mut(|m| {
                    m.request_focus(doc_id);
                    m.set_focus_lock_filter(doc_id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false });
                });
            }
            let view_h = ui.available_height();
            let tab_id = self.tabs[active].id;
            let mut clip = if self.paste_plain_next { None } else { self.clip.take() };
            {
                let opts = ViewOpts {
                    zoom,
                    draft: self.draft,
                    show_marks: self.show_marks,
                    margin_guides: self.cfg.margin_guides,
                    block_cursor: self.cfg.block_cursor,
                    header_footer: self.show_hf,
                    matches: &matches,
                    theme: &th,
                    book: &book,
                    fonts_rev: self.fonts_rev,
                    speller: if self.cfg.spellcheck && self.speller.available() { Some(&self.speller) } else { None },
                    mode: self.mode,
                    shortcuts: self.cfg.shortcut_preset,
                };
                let tab = &mut self.tabs[active];
                let exact_pdf = tab.pdf_view.as_ref().is_some_and(|pv| pv.is_exact());
                if exact_pdf {
                    let (save_k, save_as_k, undo_k, redo_k, zoom_in_k, zoom_out_k, zoom_0_k) = ui.input(|i| {
                        let cmd = i.modifiers.command;
                        let sh = i.modifiers.shift;
                        (
                            cmd && !sh && i.key_pressed(egui::Key::S),
                            cmd && sh && i.key_pressed(egui::Key::S),
                            doc_has_keys && cmd && !sh && i.key_pressed(egui::Key::Z),
                            doc_has_keys && cmd && (i.key_pressed(egui::Key::Y) || (sh && i.key_pressed(egui::Key::Z))),
                            cmd && (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals)),
                            cmd && i.key_pressed(egui::Key::Minus),
                            cmd && i.key_pressed(egui::Key::Num0),
                        )
                    });
                    if save_k {
                        cmds.push(Cmd::Save);
                    } else if save_as_k {
                        cmds.push(Cmd::SaveAs);
                    } else if undo_k {
                        tab.pdf_undo();
                    } else if redo_k {
                        tab.pdf_redo();
                    } else if zoom_in_k {
                        cmds.push(Cmd::ZoomIn);
                    } else if zoom_out_k {
                        cmds.push(Cmd::ZoomOut);
                    } else if zoom_0_k {
                        cmds.push(Cmd::Zoom100);
                    }
                } else if doc_has_keys {
                    let lay = tab.layout(&ctx, &opts);
                    tab.handle_keys_mode(ui, &lay, &sc, &mut clip, &mut cmds, view_h, self.mode, self.cfg.shortcut_preset);
                }
                self.paste_plain_next = false;
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
                caret_screen_rect = out.inner.1;
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
            if clip.is_some() {
                self.clip = clip;
            }
            self.sc_table = table;
            if doc_has_keys && focused.is_none() {
                ui.memory_mut(|m| m.request_focus(doc_id));
            }
        });

        // Floating Google Docs Link Preview Card when caret sits on a link
        let active_link = {
            let t = self.tab_ref();
            t.doc.link_bounds(t.caret).and_then(|(a, b, lid)| {
                t.doc.link_url(lid).map(|u| (a, b, u.to_string(), t.doc.text_range(a, b)))
            })
        };
        if let (Some((la, _lb, url, ltxt)), Some(cr)) = (active_link, caret_screen_rect) {
            egui::Area::new(egui::Id::unique("link-popover"))
                .fixed_pos(pos2(cr.min.x.max(20.0), cr.max.y + 6.0))
                .show(&ctx, |ui| {
                    Frame::new()
                        .fill(if th.dark { Color32::from_rgb(40, 42, 46) } else { Color32::WHITE })
                        .corner_radius(8)
                        .stroke(Stroke::new(1.0, th.border))
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (ir, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                                icons::draw(ui.painter(), ir, Icon::Link, th.accent, th.accent);
                                let disp = if url.len() > 36 { format!("{}…", &url[..36]) } else { url.clone() };
                                if ui.link(RichText::new(disp).color(th.accent).strong()).on_hover_text("Jump to or copy link").clicked() {
                                    self.act(Action::OpenLink(url.clone()));
                                }
                                ui.separator();
                                if ui.small_button("Copy").on_hover_text("Copy link").clicked() {
                                    ctx.copy_text(url.clone());
                                    self.toast("Link copied to clipboard.".into());
                                }
                                if ui.small_button("Edit").on_hover_text("Edit link").clicked() {
                                    self.act(Action::Dialog(Dialog::Link { text: ltxt.clone(), url: url.clone() }));
                                }
                                if ui.small_button("Remove").on_hover_text("Remove link").clicked() {
                                    self.act(Action::RemoveLink(la));
                                }
                            });
                        });
                });
        }

        // Floating Editorial Studio Image Bar (Resize + Inline Caption + Alignment) when an image is selected
        let active_img_info = {
            let t = self.tab_ref();
            t.selected_image_rect.and_then(|(img_id, para_idx, full_r)| {
                t.doc.image(img_id).cloned().map(|img| {
                    let align = t.doc.paras.get(para_idx).map_or(Align::Center, |p| p.fmt.align);
                    let max_w_in = (t.doc.page.width - t.doc.page.margin_left - t.doc.page.margin_right).max(1.0);
                    (img, para_idx, full_r, align, max_w_in)
                })
            })
        };
        if let Some((img_snap, para_idx, full_r, cur_align, max_w_in)) = active_img_info {
            if !matches!(self.dialog, Dialog::ImageProps(_)) && !self.clean_screen {
                let screen = ctx.content_rect();
                let bar_w = 500.0_f32;
                let bar_x = (full_r.center().x - bar_w * 0.5).clamp(screen.min.x + 16.0, (screen.max.x - bar_w - 16.0).max(screen.min.x + 16.0));
                let bar_y = if full_r.max.y + 84.0 < screen.max.y - 28.0 {
                    full_r.max.y + 8.0
                } else {
                    (full_r.min.y - 78.0).max(screen.min.y + 84.0)
                };
                let mut w_in = img_snap.width_in;
                let mut h_in = img_snap.height_in;
                let mut caption = img_snap.caption.clone();
                let mut align = cur_align;
                let mut open_props = false;
                let mut delete_img = false;
                let nat_aspect = img_snap.aspect_ratio();

                egui::Area::new(egui::Id::unique("image-studio-bar"))
                    .fixed_pos(pos2(bar_x, bar_y))
                    .order(egui::Order::Foreground)
                    .show(&ctx, |ui| {
                        Frame::new()
                            .fill(th.surface_elevated)
                            .corner_radius(10)
                            .stroke(Stroke::new(1.0, th.border))
                            .shadow(egui::epaint::Shadow {
                                offset: [0, 4],
                                blur: 12,
                                spread: 0,
                                color: Color32::from_black_alpha(if th.dark { 90 } else { 28 }),
                            })
                            .inner_margin(Margin::symmetric(10, 7))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 5.0;
                                    // Row 1: Inline Caption Editor + Details + Delete
                                    ui.horizontal(|ui| {
                                        let (ir, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                                        icons::draw(ui.painter(), ir, Icon::Image, th.accent, th.accent);
                                        ui.label(RichText::new("Caption:").size(11.5).color(th.subtle).strong());
                                        ui.add(
                                            egui::TextEdit::singleline(&mut caption)
                                                .id_salt(("img-inline-cap", img_snap.id))
                                                .desired_width(275.0)
                                                .hint_text("Type a caption below image (e.g. Figure 1)…"),
                                        );
                                        if ui.small_button("Size & Caption…").on_hover_text("Open full image size & caption dialog").clicked() {
                                            open_props = true;
                                        }
                                        if icon_button(ui, &th, Icon::Trash, "Remove image (Delete)", false, true).clicked() {
                                            delete_img = true;
                                        }
                                    });
                                    // Row 2: Quick Resize presets, +/- step, 1:1 aspect reset, and Alignment
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Resize:").size(11.5).color(th.subtle).strong());
                                        for (lbl, frac) in [("25%", 0.25_f32), ("50%", 0.50), ("75%", 0.75), ("100%", 1.0)] {
                                            let active_p = ((w_in / max_w_in) - frac).abs() < 0.04;
                                            if ui.selectable_label(active_p, RichText::new(lbl).size(11.5)).on_hover_text(format!("Scale image to {lbl} of page width")).clicked() {
                                                w_in = (max_w_in * frac).clamp(0.5, max_w_in);
                                                h_in = (w_in / nat_aspect).clamp(0.4, 20.0);
                                            }
                                        }
                                        if ui.small_button("−").on_hover_text("Shrink image (-0.25 in)").clicked() {
                                            w_in = (w_in - 0.25).clamp(0.5, max_w_in);
                                            h_in = (w_in / nat_aspect).clamp(0.4, 20.0);
                                        }
                                        ui.label(RichText::new(format!("{w_in:.2}×{h_in:.2}\"")).size(11.0).color(th.subtle).monospace());
                                        if ui.small_button("+").on_hover_text("Enlarge image (+0.25 in)").clicked() {
                                            w_in = (w_in + 0.25).clamp(0.5, max_w_in);
                                            h_in = (w_in / nat_aspect).clamp(0.4, 20.0);
                                        }
                                        if ui.small_button("1:1").on_hover_text("Reset aspect ratio").clicked() {
                                            h_in = (w_in / nat_aspect).clamp(0.4, 20.0);
                                        }
                                        ui.separator();
                                        if icon_button(ui, &th, Icon::AlignLeft, "Align image left", align == Align::Left, true).clicked() {
                                            align = Align::Left;
                                        }
                                        if icon_button(ui, &th, Icon::AlignCenter, "Center image", align == Align::Center, true).clicked() {
                                            align = Align::Center;
                                        }
                                        if icon_button(ui, &th, Icon::AlignRight, "Align image right", align == Align::Right, true).clicked() {
                                            align = Align::Right;
                                        }
                                    });
                                });
                            });
                    });

                if (w_in - img_snap.width_in).abs() > 0.002
                    || (h_in - img_snap.height_in).abs() > 0.002
                    || caption != img_snap.caption
                    || align != cur_align
                {
                    let kind = if caption != img_snap.caption { EditKind::Typing } else { EditKind::Other };
                    let t = self.tab();
                    t.checkpoint(kind, now);
                    if let Some(img) = t.doc.image_mut(img_snap.id) {
                        img.width_in = w_in;
                        img.height_in = h_in;
                        img.caption = caption;
                    }
                    if let Some(p) = t.doc.paras.get_mut(para_idx) {
                        p.fmt.align = align;
                    }
                    t.changed();
                    t.scroll_to_caret = false;
                }
                if open_props {
                    self.act(Action::Dialog(Dialog::ImageProps(img_snap.id)));
                } else if delete_img {
                    self.tab().remove_image(img_snap.id, now);
                }
            }
        }

        // Google Docs floating "Display word count while typing" pill in the bottom-left
        if self.cfg.live_word_count && !self.clean_screen {
            let t = self.tab_ref();
            let (chars, words) = t.doc.counts(None);
            let pages = t.cached_pages();
            let chars_no_spaces = t.doc.plain_text().chars().filter(|c| !c.is_whitespace()).count();
            let label = match self.wc_metric % 4 {
                0 => if t.has_selection() {
                    let sw = t.doc.counts(Some(t.selection())).1;
                    format!("{sw} of {words} words ▾")
                } else {
                    format!("{words} words ▾")
                },
                1 => format!("{chars} characters ▾"),
                2 => format!("{chars_no_spaces} chars (no spaces) ▾"),
                _ => format!("{pages} pages ▾"),
            };
            egui::Area::new(egui::Id::unique("live-wc-pill"))
                .anchor(egui::Align2::LEFT_BOTTOM, vec2(18.0, -36.0))
                .show(&ctx, |ui| {
                    Frame::new()
                        .fill(if th.dark { Color32::from_rgb(40, 42, 46) } else { Color32::WHITE })
                        .corner_radius(16)
                        .stroke(Stroke::new(1.0, th.border))
                        .inner_margin(Margin::symmetric(12, 5))
                        .show(ui, |ui| {
                            if ui.add(egui::Button::new(RichText::new(label).size(12.5).color(th.text)).frame(false)).on_hover_text("Click to cycle metric (Words / Characters / Pages)").clicked() {
                                self.wc_metric = (self.wc_metric + 1) % 4;
                            }
                        });
                });
        }

        let doc_id = self.doc_id;
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(doc_id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false });
        });
        self.handle_cmds(cmds);

        dialogs::show(self, &ctx);
        self.draw_doc_loading_overlay(&ctx, &th);
        self.splash.show(&ctx);

        let mr_pal = self.mr_palette();
        self.toasts.show(&ctx, &mr_pal);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_does_not_collapse_canvas_when_unmaximizing_window() {
        let ctx = egui::Context::default();
        let th = Theme::light();
        let book = crate::fonts::FontBook::default();
        let mut app = App {
            egui_ctx: ctx.clone(),
            tabs: vec![Tab::new(1, "Untitled1".into(), Document::default())],
            active: 0,
            next_id: 2,
            untitled: 1,
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
            compact_mode: false,
            sc_table: Vec::new(),
            cfg: Config::default(),
            theme: th.clone(),
            dialog: Dialog::None,
            find: FindState::default(),
            clip: None,
            paste_plain_next: false,
            actions: Vec::new(),
            screenshot: None,
            shot_done: Arc::new(AtomicBool::new(false)),
            frame: 0,
            toasts: modernrust::ToastStack::default(),
            last_autosave: 0.0,
            title: String::new(),
            close_flow: CloseFlow::None,
            doc_id: egui::Id::unique("test-canvas"),
            view_size: vec2(1200.0, 800.0),
            fonts_rev: 1,
            speller: crate::spell::Speller::new(),
            show_nav: false,
            right_panel: RightPanel::None,
            history_sel_idx: 0,
            history_ver_name: String::new(),
            mode: DocMode::Editing,
            font_search: String::new(),
            demo_done: false,
            painter_style: None,
            editing_title: false,
            comment_reply_draft: (0, String::new()),
            wc_metric: 0,
            table_panel_collapsed: false,
            pdf_panel_collapsed: false,
            update: UpdateState::default(),
            assoc_status_msg: None,
            doc_load: None,
            doc_load_queue: Vec::new(),
            splash: crate::splash::SplashState::new(false),
        };

        // Simulate fullscreen -> windowed -> narrow window resize sequence
        for (w, h) in [(1920.0, 1080.0), (1100.0, 720.0), (780.0, 560.0)] {
            let mut toolbar_h = 0.0f32;
            let mut central_h = 0.0f32;
            for _ in 0..2 {
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(w, h))),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(raw, |ui| {
                    egui::Panel::top("menubar").show(ui, |ui| {
                        app.docs_header(ui, &th);
                    });
                    let tb = egui::Panel::top("toolbar")
                        .max_size(140.0)
                        .show(ui, |ui| {
                            Frame::new()
                                .inner_margin(Margin::symmetric(10, 5))
                                .show(ui, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = vec2(2.5, 4.0);
                                        app.toolbar(ui, &th);
                                    });
                                });
                        });
                    toolbar_h = tb.response.rect.height();
                    egui::Panel::bottom("status").show(ui, |ui| {
                        app.status_bar(ui, &th);
                    });
                    let cp = egui::CentralPanel::default().show(ui, |ui| {
                        central_h = ui.available_height();
                    });
                    let _ = cp;
                });
                out.textures_delta.clear();
            }
            assert!(
                toolbar_h <= 145.0,
                "Toolbar blew up vertically at {w}x{h}: height={toolbar_h}"
            );
            assert!(
                central_h >= h * 0.5,
                "Central document canvas collapsed at {w}x{h}: central_h={central_h}, toolbar_h={toolbar_h}"
            );
        }
    }

    #[test]
    fn explicit_state_machines_enforce_exclusivity_and_invariants() {
        // 1. CloseFlow: single exclusive lifecycle instead of 3 booleans/Options
        assert!(!CloseFlow::None.is_confirming());
        assert!(CloseFlow::ConfirmTab(0).is_confirming());
        assert!(CloseFlow::ConfirmQuit.is_confirming());
        assert!(!CloseFlow::ForceQuit.is_confirming());

        // 2. RightPanel: Comments and VersionHistory are mutually exclusive
        let mut panel = RightPanel::None;
        panel.toggle(RightPanel::Comments);
        assert_eq!(panel, RightPanel::Comments);
        panel.toggle(RightPanel::VersionHistory);
        assert_eq!(panel, RightPanel::VersionHistory);
        panel.toggle(RightPanel::VersionHistory);
        assert_eq!(panel, RightPanel::None);

        // 3. BlockKind: derived classification without breaking ParaFmt JSON serialization
        let text_fmt = crate::doc::ParaFmt::default();
        assert_eq!(text_fmt.block_kind(), crate::doc::BlockKind::Text);

        let hr_fmt = crate::doc::ParaFmt {
            hr: true,
            ..Default::default()
        };
        assert_eq!(hr_fmt.block_kind(), crate::doc::BlockKind::HorizontalRule);

        let img_fmt = crate::doc::ParaFmt {
            image_id: 42,
            ..Default::default()
        };
        assert_eq!(img_fmt.block_kind(), crate::doc::BlockKind::Image(42));

        let cell_fmt = crate::doc::ParaFmt {
            table_id: 7,
            table_row: 2,
            table_col: 3,
            ..Default::default()
        };
        assert_eq!(
            cell_fmt.block_kind(),
            crate::doc::BlockKind::TableCell {
                table_id: 7,
                row: 2,
                col: 3,
            }
        );

        // 4. CanvasDrag: exclusive pointer drag state
        let idle = crate::editor::CanvasDrag::default();
        assert!(idle.is_idle());
        assert!(!idle.is_resizing_image());
        assert_eq!(idle.resizing_image_id(), None);

        let img_drag = crate::editor::CanvasDrag::ResizingImage {
            id: 9,
            handle: 2,
            start_pos: egui::pos2(10.0, 20.0),
            w0_in: 3.0,
            h0_in: 2.0,
        };
        assert!(!img_drag.is_idle());
        assert!(img_drag.is_resizing_image());
        assert_eq!(img_drag.resizing_image_id(), Some(9));
        assert_eq!(img_drag.resizing_image_handle(), Some((9, 2)));
    }

    #[test]
    fn table_math_target_reassignment_prevents_self_reference_and_updates_direction() {
        let mut r = 2usize;
        let mut c = 1usize;
        let mut use_above = false;
        let mut use_left = false;
        let mut chosen = vec![(0, 1), (1, 1), (2, 2)];
        let mut picking = true;

        // Reassign target to (1, 1) which was in `chosen` -> stripped automatically
        crate::dialogs::set_table_math_target(
            1,
            1,
            &mut r,
            &mut c,
            &mut use_above,
            &mut use_left,
            &mut chosen,
            &mut picking,
        );
        assert_eq!((r, c), (1, 1));
        assert!(!chosen.contains(&(1, 1)));
        assert_eq!(chosen, vec![(0, 1), (2, 2)]);
        assert!(!picking);

        // Moving to top row (0, 2) while `use_above` is active flips to `use_left`
        use_above = true;
        crate::dialogs::set_table_math_target(
            0,
            2,
            &mut r,
            &mut c,
            &mut use_above,
            &mut use_left,
            &mut chosen,
            &mut picking,
        );
        assert_eq!((r, c), (0, 2));
        assert!(!use_above);
        assert!(use_left);
    }

    #[test]
    fn pdf_mode_state_machine_and_undo_redo() {
        let ctx = egui::Context::default();
        let mut book = FontBook::default();
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            book = crate::fonts::install(&ctx);
        });
        out.textures_delta.clear();

        let mut doc = Document::default();
        doc.insert_text(Pos::new(0, 0), "Hello PDF", CharStyle::default());
        let mut bytes = Vec::new();
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            bytes = crate::pdf::export(&ctx, &doc, &book);
        });
        out.textures_delta.clear();

        let mut tab = Tab::new(1, "sample.pdf".into(), doc);
        tab.attach_pdf_exact_view(bytes);
        assert_eq!(tab.pdf_mode(), Some(crate::editor::PdfMode::Read));

        tab.set_pdf_mode(crate::editor::PdfMode::Edit);
        assert_eq!(tab.pdf_mode(), Some(crate::editor::PdfMode::Edit));

        {
            let pv = tab.pdf_view.as_mut().expect("pdf_view attached");
            assert!(!pv.blocks.is_empty());
            pv.checkpoint();
            pv.blocks[0].text = "Updated PDF".into();
            pv.blocks[0].modified = true;
            assert!(pv.has_edits());
        }
        tab.dirty = true;

        assert!(tab.pdf_undo());
        assert_eq!(tab.pdf_view.as_ref().unwrap().blocks[0].text, "Hello PDF");
        assert!(!tab.dirty);

        assert!(tab.pdf_redo());
        assert_eq!(tab.pdf_view.as_ref().unwrap().blocks[0].text, "Updated PDF");
        assert!(tab.dirty);

        tab.set_pdf_mode(crate::editor::PdfMode::Reflow);
        assert_eq!(tab.pdf_mode(), Some(crate::editor::PdfMode::Reflow));
        tab.set_pdf_mode(crate::editor::PdfMode::Read);
        assert_eq!(tab.pdf_mode(), Some(crate::editor::PdfMode::Read));

        // Regression test: rendering modified/justified PDF text blocks after deselecting
        // (clicking/double-clicking outside the text box) must not panic from re-entrant fonts lock.
        tab.set_pdf_mode(crate::editor::PdfMode::Edit);
        {
            let pv = tab.pdf_view.as_mut().unwrap();
            pv.blocks[0].text = "First justified line with enough words to wrap across multiple lines cleanly.".into();
            pv.blocks[0].align = Align::Justify;
            pv.blocks[0].w_pt = 160.0;
            pv.blocks[0].modified = true;
            pv.selected_block = Some(pv.blocks[0].id);
        }
        let th = Theme::light();
        let opts = crate::editor::ViewOpts {
            zoom: 1.0,
            draft: false,
            show_marks: false,
            margin_guides: false,
            block_cursor: false,
            header_footer: false,
            matches: &[],
            theme: &th,
            book: &book,
            fonts_rev: 1,
            speller: None,
            mode: DocMode::Editing,
            shortcuts: ShortcutPreset::default(),
        };
        let doc_id = egui::Id::unique("pdf-deselect-test");
        let mut cmds = Vec::new();
        // Frame 1: selected
        let mut out = ctx.run_ui(Default::default(), |ui| {
            tab.show(ui, &opts, doc_id, &mut cmds);
        });
        out.textures_delta.clear();
        // Frame 2: deselected (as after clicking/double-clicking outside the text box) -> paints justified galleys
        tab.pdf_view.as_mut().unwrap().selected_block = None;
        let mut out = ctx.run_ui(Default::default(), |ui| {
            tab.show(ui, &opts, doc_id, &mut cmds);
        });
        out.textures_delta.clear();
        // Frame 3: Read mode with justified modified text
        tab.set_pdf_mode(crate::editor::PdfMode::Read);
        let mut out = ctx.run_ui(Default::default(), |ui| {
            tab.show(ui, &opts, doc_id, &mut cmds);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn independent_header_and_footer_scopes_and_tokens() {
        use crate::doc::{from_json, to_json, HfScope};

        let mut doc = Document::default();
        doc.info.title = "Annual Plan".into();

        // Header: different first page + odd pages only for subsequent pages
        doc.header_different_first = true;
        doc.header_first = "Cover — {title}".into();
        doc.header = "Running Header (Odd)".into();
        doc.header_scope = HfScope::OddOnly;

        // Footer: completely independent (even pages only + Page X of Y + Strictly Confidential)
        doc.footer_different_first = false;
        doc.footer = "Strictly Confidential • Page {page} of {pages}".into();
        doc.footer_scope = HfScope::EvenOnly;

        // Page 1 (idx 0): first-page header, no footer (since 1 is odd and footer is EvenOnly)
        assert_eq!(doc.resolved_header_text(0, 4).as_deref(), Some("Cover — Annual Plan"));
        assert_eq!(doc.resolved_footer_text(0, 4), None);

        // Page 2 (idx 1): no header (2 is even, header is OddOnly), footer shows Page 2 of 4
        assert_eq!(doc.resolved_header_text(1, 4), None);
        assert_eq!(
            doc.resolved_footer_text(1, 4).as_deref(),
            Some("Strictly Confidential • Page 2 of 4")
        );

        // Page 3 (idx 2): odd running header, no footer
        assert_eq!(doc.resolved_header_text(2, 4).as_deref(), Some("Running Header (Odd)"));
        assert_eq!(doc.resolved_footer_text(2, 4), None);

        // Page 4 (idx 3): no header, footer shows Page 4 of 4
        assert_eq!(doc.resolved_header_text(3, 4), None);
        assert_eq!(
            doc.resolved_footer_text(3, 4).as_deref(),
            Some("Strictly Confidential • Page 4 of 4")
        );

        // FirstOnly scope check
        doc.header_different_first = false;
        doc.header_scope = HfScope::FirstOnly;
        assert_eq!(doc.resolved_header_text(0, 4).as_deref(), Some("Running Header (Odd)"));
        assert_eq!(doc.resolved_header_text(1, 4), None);

        // JSON (.wpx) round-trip preserves all independent header/footer fields
        let json = to_json(&doc);
        let reloaded = from_json(&json).expect("wpx json roundtrip");
        assert_eq!(reloaded.header_scope, HfScope::FirstOnly);
        assert_eq!(reloaded.footer_scope, HfScope::EvenOnly);
        assert_eq!(reloaded.header_first, "Cover — {title}");
        assert_eq!(reloaded.footer, "Strictly Confidential • Page {page} of {pages}");
    }
}

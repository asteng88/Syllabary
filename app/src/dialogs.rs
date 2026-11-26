//! Dialog boxes: Find & Replace, Word Count, Page setup (Pages & Pageless), Font, Paragraph,
//! Go To, Date/Time, Special Characters & Emoji, Preferences, Personal Dictionary / Substitutions,
//! Document details, Version history, Search the menus (`Alt+/`), `@` Smart Chips & Building Blocks,
//! Insert Link, Add Comment, Bookmarks, Citations, Dictionary, Rename, Keyboard Shortcuts, About.

use eframe::egui::{self, vec2, RichText};
use std::path::PathBuf;

use crate::app::{gradient_swatch, swatch, text_gradient_swatch, Action, App, PageEdit, ZoomMode, CRAFT_GRADIENT_HIGHLIGHTS, CRAFT_GRADIENT_TEXT, HIGHLIGHTS, PALETTE};
use crate::doc::*;
use crate::editor::{DocMode, EditKind};
use crate::icons::{draw as draw_icon, Icon};

#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    None,
    Find(bool),
    WordCount,
    PageSetup,
    Paragraph,
    GoTo(usize),
    DateTime,
    Symbol,
    Prefs,
    About,
    Update,
    Shortcuts,
    DocProps,
    SmartCorrect(String, String),
    Font,
    Recover(Vec<PathBuf>),
    Spell(usize),
    SearchMenus(String),
    AtMenu(String),
    Link { text: String, url: String },
    Comment(String),
    Bookmark(String),
    Citations(CitationEntry),
    Dictionary,
    Rename(String),
    #[cfg_attr(target_os = "windows", expect(dead_code, reason = "Constructed on macOS and Linux; Windows uses native PrintDlgW directly"))]
    Print(crate::print::PrintJobConfig),
    DesignSheet,
    ImageProps(u64),
    InsertTable {
        rows: usize,
        cols: usize,
        header_shading: Option<[u8; 3]>,
    },
    TableMath {
        table_id: u32,
        row: usize,
        col: usize,
        op: TableMathOp,
        use_above: bool,
        use_left: bool,
        chosen: Vec<(usize, usize)>,
        keep_sig_figs: bool,
        /// Some(expression) = arithmetic mode (`B2*C2`, `(D2+D3)/B2`, …)
        expr: Option<String>,
        /// When true, clicking any cell in the dialog grid reassigns `(row, col)` to that cell.
        picking_target: bool,
    },
}

struct FindCache {
    rev: u64,
    query: String,
    mode: FindMode,
    case: bool,
    matches: Vec<(Pos, Pos)>,
}

pub struct FindState {
    pub query: String,
    pub replace: String,
    pub mode: FindMode,
    pub case: bool,
    pub focus: bool,
    pub show_replace: bool,
    cache: Option<FindCache>,
}

impl Default for FindState {
    fn default() -> Self {
        Self { query: String::new(), replace: String::new(), mode: FindMode::Containing, case: false, focus: false, show_replace: true, cache: None }
    }
}

impl FindState {
    pub fn matches(&mut self, doc: &Document, rev: u64) -> Vec<(Pos, Pos)> {
        if let Some(c) = &self.cache {
            if c.rev == rev && c.query == self.query && c.mode == self.mode && c.case == self.case {
                return c.matches.clone();
            }
        }
        let matches = find_all(doc, &self.query, self.mode, self.case);
        self.cache = Some(FindCache {
            rev,
            query: self.query.clone(),
            mode: self.mode,
            case: self.case,
            matches: matches.clone(),
        });
        matches
    }
}

const SYMBOLS: &str = "©®™§¶†‡•·–—…‘’“”«»°±×÷≠≤≥≈∞√∑∏∫πΩµ€£¥¢½¼¾⅓⅔→←↑↓↔⇒✓✗★☆♥♦♣♠☎✉✂✎αβγδεθλμσφψω¹²³ªº¿¡ß";

fn window(title: &str) -> egui::Window<'_> {
    egui::Window::new(title).collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(egui::pos2(700.0, 360.0))
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    confirmations(app, ctx);
    let dialog = app.dialog.clone();
    let mut open = true;
    let now = ctx.input(|i| i.time);
    match dialog {
        Dialog::None => {}
        Dialog::DesignSheet => design_sheet_dialog(ctx, &mut open),
        Dialog::Find(_) => find_dialog(app, ctx, &mut open),
        Dialog::Spell(_) => spell_dialog(app, ctx, &mut open, now),
        Dialog::WordCount => {
            let t = app.tab_ref();
            let (chars, words) = t.doc.counts(None);
            let sel = if t.has_selection() { Some(t.doc.counts(Some(t.selection()))) } else { None };
            let paras = t.doc.paras.iter().filter(|p| !p.text.is_empty()).count();
            let pages = t.cached_pages();
            let pageless = t.doc.pageless;
            let text = t.doc.plain_text();
            let chars_no_spaces = text.chars().filter(|c| !c.is_whitespace()).count();
            let sentences = text.split(['.', '!', '?']).filter(|s| s.chars().any(|c| c.is_alphanumeric())).count();
            let mut live_wc = app.cfg.live_word_count;
            window("Word count").open(&mut open).show(ctx, |ui| {
                ui.set_min_width(360.0);
                if let Some((c, w)) = sel {
                    ui.label(RichText::new(format!("Selection: {w} of {words} words ({c} of {chars} characters)")).strong());
                    ui.add_space(4.0);
                }
                egui::Grid::new("wc").num_columns(2).striped(true).spacing(vec2(36.0, 6.0)).show(ui, |ui| {
                    ui.label("Pages");
                    ui.label(if pageless { "Pageless".to_string() } else { pages.to_string() });
                    ui.end_row();
                    ui.label("Words");
                    ui.label(if let Some((_, w)) = sel { format!("{w} of {words}") } else { words.to_string() });
                    ui.end_row();
                    ui.label("Characters");
                    ui.label(if let Some((c, _)) = sel { format!("{c} of {chars}") } else { chars.to_string() });
                    ui.end_row();
                    ui.label("Characters excluding spaces");
                    ui.label(chars_no_spaces.to_string());
                    ui.end_row();
                    ui.label("Paragraphs");
                    ui.label(paras.to_string());
                    ui.end_row();
                    ui.label("Sentences");
                    ui.label(sentences.to_string());
                    ui.end_row();
                    ui.label("Reading time");
                    ui.label(format!("~{} min", ((words as f32) / 230.0).ceil().max(if words > 0 { 1.0 } else { 0.0 })));
                    ui.end_row();
                });
                ui.add_space(8.0);
                ui.separator();
                if ui.checkbox(&mut live_wc, "Display word count while typing").changed() {
                    app.cfg.live_word_count = live_wc;
                    app.save_config();
                }
            });
        }
        Dialog::PageSetup => page_setup(app, ctx, &mut open, now),
        Dialog::Paragraph => {
            let t = app.tab_ref();
            let mut f = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
            let orig = f;
            window("Custom spacing & paragraph options").open(&mut open).show(ctx, |ui| {
                egui::Grid::new("para").num_columns(2).spacing(vec2(16.0, 8.0)).show(ui, |ui| {
                    ui.label("Alignment");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut f.align, Align::Left, "Left");
                        ui.selectable_value(&mut f.align, Align::Center, "Center");
                        ui.selectable_value(&mut f.align, Align::Right, "Right");
                        ui.selectable_value(&mut f.align, Align::Justify, "Justify");
                    });
                    ui.end_row();
                    ui.label("Indent level");
                    ui.add(egui::DragValue::new(&mut f.indent).range(0..=8).suffix(" × 0.5 in"));
                    ui.end_row();
                    ui.label("Space before");
                    ui.add(egui::DragValue::new(&mut f.space_before).range(0.0..=72.0).speed(0.5).suffix(" pt"));
                    ui.end_row();
                    ui.label("Space after");
                    ui.add(egui::DragValue::new(&mut f.space_after).range(0.0..=72.0).speed(0.5).suffix(" pt"));
                    ui.end_row();
                    ui.label("Line spacing");
                    egui::ComboBox::from_id_salt("ls").selected_text(format!("{:.2}", f.line_spacing)).show_ui(ui, |ui| {
                        for v in [1.0f32, 1.15, 1.5, 2.0, 2.5, 3.0] {
                            ui.selectable_value(&mut f.line_spacing, v, format!("{v:.2}"));
                        }
                    });
                    ui.end_row();
                    ui.label("Page break before");
                    ui.checkbox(&mut f.page_break_before, "");
                    ui.end_row();
                });
            });
            if f != orig {
                let t = app.tab();
                t.checkpoint(EditKind::Typing, now);
                for p in t.doc.para_range(t.anchor, t.caret) {
                    let s = t.doc.paras[p].fmt.style;
                    t.doc.paras[p].fmt = ParaFmt { style: s, ..f };
                }
                t.changed();
                t.scroll_to_caret = false;
            }
        }
        Dialog::GoTo(mut page) => {
            let pages = app.tab_ref().cached_pages();
            let mut go = false;
            window("Go To Page").open(&mut open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Page");
                    let r = ui.add(egui::DragValue::new(&mut page).range(1..=pages.max(1)));
                    ui.label(format!("of {pages}"));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        go = true;
                    }
                });
                if ui.button("Go").clicked() {
                    go = true;
                }
            });
            app.dialog = Dialog::GoTo(page);
            if go {
                let t = app.tab();
                t.scroll_to_page = Some(page - 1);
                if let Some(p) = t.first_pos_on_page(page - 1) {
                    t.set_caret(p, false);
                    t.scroll_to_caret = false;
                }
                open = false;
            }
        }
        Dialog::DateTime => {
            let n = chrono::Local::now();
            let formats = [
                n.format("%B %-d, %Y").to_string(),
                n.format("%-m/%-d/%Y").to_string(),
                n.format("%-d %B %Y").to_string(),
                n.format("%A, %B %-d, %Y").to_string(),
                n.format("%Y-%m-%d").to_string(),
                n.format("%-I:%M %p").to_string(),
                n.format("%H:%M").to_string(),
                n.format("%B %-d, %Y %-I:%M %p").to_string(),
            ];
            window("Insert Date / Time").open(&mut open).show(ctx, |ui| {
                ui.label(RichText::new("Click a format to insert it (or use @today for a date chip)").weak());
                for f in formats {
                    if ui.add(egui::Button::new(&f).min_size(vec2(260.0, 0.0))).clicked() {
                        app.act(Action::InsertText(f.clone()));
                    }
                }
            });
        }
        Dialog::Symbol => {
            const EMOJI_GROUPS: &[(&str, &[&str])] = &[
                (
                    "Smileys & People",
                    &[
                        "😀", "😃", "😄", "😁", "😆", "😅", "😂", "🤣", "😊", "😇", "🙂", "😉",
                        "😍", "🥰", "😘", "😋", "😎", "🥳", "🤩", "🤔", "🤗", "😴", "🙌", "👏",
                        "👍", "👎", "👋", "🤝", "🙏", "💪", "🧠", "👀", "🎉", "🎊", "🎈", "🏆",
                    ],
                ),
                (
                    "Hearts, Stars & Status",
                    &[
                        "❤️", "🧡", "💛", "💚", "💙", "💜", "🤍", "💖", "✨", "🌟", "⭐", "🔥",
                        "✅", "❌", "⚠️", "📌", "📍", "📎", "📝", "📄", "📂", "📅", "📊", "📈",
                        "📉", "🔍", "🔒", "🔑", "💬", "📢", "💡", "⚙️", "🚀", "⏳", "⏱️", "🔖",
                    ],
                ),
                (
                    "Nature, Food & Travel",
                    &[
                        "🌞", "🌈", "🌊", "🌸", "🌲", "🍀", "🐶", "🐱", "🦊", "🦁", "☕", "🍵",
                        "🍎", "🍕", "🎂", "🥂", "✈️", "🚗", "🏠", "🌍", "⛰️", "🏖️", "🧭", "💎",
                    ],
                ),
            ];
            let mut open_sys = false;
            window("Emoji & Special Characters").open(&mut open).show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(RichText::new("😊 Open System Emoji Library (Windows / macOS / Linux)").strong())
                        .on_hover_text("Opens the operating system's native Emoji Picker at the cursor")
                        .clicked()
                    {
                        open_sys = true;
                    }
                });
                ui.label(RichText::new("Or click any emoji or special symbol below to insert it directly into your document:").weak().small());
                ui.separator();
                egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                    for (group_name, list) in EMOJI_GROUPS {
                        ui.label(RichText::new(*group_name).small().strong());
                        egui::Grid::new(*group_name).spacing(vec2(3.0, 3.0)).show(ui, |ui| {
                            for (i, em) in list.iter().enumerate() {
                                if ui.add(egui::Button::new(RichText::new(*em).size(18.0)).min_size(vec2(32.0, 32.0))).clicked() {
                                    app.act(Action::InsertText((*em).to_string()));
                                }
                                if i % 12 == 11 {
                                    ui.end_row();
                                }
                            }
                        });
                        ui.add_space(4.0);
                    }
                    ui.label(RichText::new("Symbols, Math & Arrows").small().strong());
                    egui::Grid::new("sym").spacing(vec2(3.0, 3.0)).show(ui, |ui| {
                        for (i, ch) in SYMBOLS.chars().enumerate() {
                            if ui.add(egui::Button::new(RichText::new(ch.to_string()).size(17.0)).min_size(vec2(32.0, 32.0))).clicked() {
                                app.act(Action::InsertText(ch.to_string()));
                            }
                            if i % 12 == 11 {
                                ui.end_row();
                            }
                        }
                    });
                });
            });
            if open_sys {
                open = false;
                app.act(Action::SystemEmojiPicker);
            }
        }
        Dialog::Font => {
            let st = app.tab_ref().typing_style();
            let book = app.book.clone();
            window("Font & Color").open(&mut open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        let th = app.theme.clone();
                        let recent = app.cfg.recent_fonts.clone();
                        if let Some(f) = crate::chrome::font_list(ui, &th, &book, st.font, &mut app.font_search, &recent, false, 260.0) {
                            app.act(Action::Font(f));
                        }
                    });
                    ui.vertical(|ui| {
                        let mut size = st.size;
                        ui.horizontal(|ui| {
                            ui.label("Size");
                            if ui.add(egui::DragValue::new(&mut size).range(MIN_SIZE..=MAX_SIZE).speed(0.5).suffix(" pt")).changed() {
                                app.act(Action::Size(size));
                            }
                        });
                        let mut b = st.bold;
                        if ui.checkbox(&mut b, "Bold").changed() {
                            app.act(Action::Bold);
                        }
                        let mut it = st.italic;
                        if ui.checkbox(&mut it, "Italic").changed() {
                            app.act(Action::Italic);
                        }
                        let mut u = st.underline;
                        if ui.checkbox(&mut u, "Underline").changed() {
                            app.act(Action::Underline);
                        }
                        let mut s = st.strike;
                        if ui.checkbox(&mut s, "Strikethrough").changed() {
                            app.act(Action::Strike);
                        }
                        ui.horizontal(|ui| {
                            let mut sup = st.script > 0;
                            if ui.checkbox(&mut sup, "Superscript").changed() {
                                app.act(Action::Script(1));
                            }
                            let mut sub = st.script < 0;
                            if ui.checkbox(&mut sub, "Subscript").changed() {
                                app.act(Action::Script(-1));
                            }
                        });
                    });
                });
                ui.separator();
                ui.label("Text colour (Solid)");
                ui.horizontal_wrapped(|ui| {
                    for c in PALETTE {
                        if swatch(ui, *c, st.color == *c && !st.is_gradient_color()).clicked() {
                            app.act(Action::Color(*c));
                        }
                    }
                });
                ui.label("Craft gradient text (Left → Right)");
                egui::Grid::new("dlg-craft-text-grad").spacing(vec2(6.0, 5.0)).show(ui, |ui| {
                    for (i, (name, c1, c2)) in CRAFT_GRADIENT_TEXT.iter().enumerate() {
                        let sel = st.color == *c1 && st.color_to == Some(*c2);
                        if text_gradient_swatch(ui, name, *c1, *c2, sel).clicked() {
                            app.act(Action::ColorGradient(*c1, *c2));
                        }
                        if i % 4 == 3 {
                            ui.end_row();
                        }
                    }
                });
                ui.label("Highlight (Solid)");
                ui.horizontal(|ui| {
                    for c in HIGHLIGHTS {
                        if swatch(ui, *c, st.highlight == Some(*c) && !st.is_gradient_highlight()).clicked() {
                            app.act(Action::Highlight(Some(*c)));
                        }
                    }
                    if ui.small_button("None").clicked() {
                        app.act(Action::Highlight(None));
                    }
                });
                ui.label("Craft gradient highlight (Left → Right)");
                egui::Grid::new("dlg-craft-grad").spacing(vec2(6.0, 5.0)).show(ui, |ui| {
                    for (i, (name, c1, c2)) in CRAFT_GRADIENT_HIGHLIGHTS.iter().enumerate() {
                        let sel = st.highlight == Some(*c1) && st.highlight_to == Some(*c2);
                        if gradient_swatch(ui, name, *c1, *c2, sel).clicked() {
                            app.act(Action::HighlightGradient(*c1, *c2));
                        }
                        if i % 4 == 3 {
                            ui.end_row();
                        }
                    }
                });
                let cur_from = st.highlight.unwrap_or(HIGHLIGHTS[0]);
                let cur_to = st.highlight_to.or(st.highlight).unwrap_or(HIGHLIGHTS[3]);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Custom Left → Right:").small().weak());
                    for c in HIGHLIGHTS {
                        if swatch(ui, *c, st.is_gradient_highlight() && st.highlight == Some(*c)).clicked() {
                            app.act(Action::HighlightGradient(*c, cur_to));
                        }
                    }
                    ui.label("→");
                    for c in HIGHLIGHTS {
                        if swatch(ui, *c, st.is_gradient_highlight() && st.highlight_to == Some(*c)).clicked() {
                            app.act(Action::HighlightGradient(cur_from, *c));
                        }
                    }
                });
                ui.separator();
                let (fam, _) = book.family(st.font, st.bold, st.italic);
                let mut rt = RichText::new("The quick brown fox jumps over the lazy dog").font(egui::FontId::new(st.size.clamp(8.0, 36.0) * 1.333, fam.clone())).color(crate::layout::color(st.color));
                if st.underline {
                    rt = rt.underline();
                }
                if st.strike {
                    rt = rt.strikethrough();
                }
                if let Some(c1) = st.highlight {
                    if !st.is_gradient_highlight() {
                        rt = rt.background_color(crate::layout::color(c1));
                    }
                }
                egui::Frame::new().fill(egui::Color32::WHITE).corner_radius(6).inner_margin(10).show(ui, |ui| {
                    ui.set_min_width(420.0);
                    let resp = ui.label(rt);
                    if let (Some(c1), Some(c2)) = (st.highlight, st.highlight_to) {
                        if c1 != c2 {
                            crate::layout::paint_gradient_pill(ui.painter(), resp.rect.expand2(vec2(3.0, 1.0)), c1, c2, 4.0);
                            let mut rt2 = RichText::new("The quick brown fox jumps over the lazy dog")
                                .font(egui::FontId::new(st.size.clamp(8.0, 36.0) * 1.333, fam))
                                .color(crate::layout::color(st.color));
                            if st.underline {
                                rt2 = rt2.underline();
                            }
                            if st.strike {
                                rt2 = rt2.strikethrough();
                            }
                            ui.put(resp.rect, egui::Label::new(rt2));
                        }
                    }
                });
            });
        }
        Dialog::Prefs => {
            // Handled via RightPanel::Settings in app.rs.
            open = false;
        }
        Dialog::SmartCorrect(mut from, mut to) => {
            let mut remove: Option<String> = None;
            let mut add = false;
            let table = app.sc_table.clone();
            window("Personal dictionary & Substitutions").open(&mut open).show(ctx, |ui| {
                ui.label("Automatic text substitutions (replace text on the left with text on the right):");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut from).hint_text("Replace").desired_width(130.0));
                    ui.label("→");
                    ui.add(egui::TextEdit::singleline(&mut to).hint_text("With").desired_width(160.0));
                    if ui.add_enabled(!from.trim().is_empty() && !to.is_empty(), egui::Button::new("Add")).clicked() {
                        add = true;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    egui::Grid::new("scg").num_columns(3).striped(true).show(ui, |ui| {
                        for (a, b) in &table {
                            ui.label(a);
                            ui.label(b);
                            if ui.small_button("Remove").clicked() {
                                remove = Some(a.clone());
                            }
                            ui.end_row();
                        }
                    });
                });
            });
            if add {
                let f = from.trim().to_string();
                app.cfg.removed_corrections.retain(|x| x != &f);
                app.cfg.user_corrections.retain(|(x, _)| x != &f);
                app.cfg.user_corrections.push((f, to.clone()));
                from.clear();
                to.clear();
                app.rebuild_sc_table();
                app.save_config();
            }
            if let Some(r) = remove {
                app.cfg.user_corrections.retain(|(x, _)| x != &r);
                if !app.cfg.removed_corrections.contains(&r) {
                    app.cfg.removed_corrections.push(r);
                }
                app.rebuild_sc_table();
                app.save_config();
            }
            if app.dialog != Dialog::None {
                app.dialog = Dialog::SmartCorrect(from, to);
            }
        }
        Dialog::DocProps => {
            let t = app.tab();
            let mut info = t.doc.info.clone();
            let mut summary = t.doc.summary.clone();
            let (chars, words) = t.doc.counts(None);
            let name = t.name.clone();
            let path = t.path.clone();
            window("Document details").open(&mut open).show(ctx, |ui| {
                egui::Grid::new("dp").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                    ui.label("Title");
                    ui.add(egui::TextEdit::singleline(&mut info.title).desired_width(300.0));
                    ui.end_row();
                    ui.label("Author");
                    ui.add(egui::TextEdit::singleline(&mut info.author).desired_width(300.0));
                    ui.end_row();
                    ui.label("Subject");
                    ui.add(egui::TextEdit::singleline(&mut info.subject).desired_width(300.0));
                    ui.end_row();
                    ui.label("Keywords");
                    ui.add(egui::TextEdit::singleline(&mut info.keywords).desired_width(300.0));
                    ui.end_row();
                    ui.label("Summary");
                    ui.add(egui::TextEdit::multiline(&mut summary).desired_width(300.0).desired_rows(2));
                    ui.end_row();
                    ui.label("Notes");
                    ui.add(egui::TextEdit::multiline(&mut info.notes).desired_width(300.0).desired_rows(3));
                    ui.end_row();
                    ui.label("Location");
                    ui.label(path.map_or(name, |p| p.display().to_string()));
                    ui.end_row();
                    ui.label("Statistics");
                    ui.label(format!("{words} words, {chars} characters"));
                    ui.end_row();
                });
            });
            let t = app.tab();
            if info != t.doc.info || summary != t.doc.summary {
                t.doc.info = info;
                t.doc.summary = summary;
                t.dirty = true;
            }
        }
        Dialog::Recover(files) => {
            let mut choice = None;
            egui::Modal::new(egui::Id::unique("recover")).show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.heading("Recover documents");
                ui.label(format!("A timed save copy of {} unsaved document(s) was found from last time. Do you want to open them?", files.len()));
                ui.horizontal(|ui| {
                    if ui.button("Open them").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Discard").clicked() {
                        choice = Some(false);
                    }
                });
            });
            match choice {
                Some(true) => {
                    for f in &files {
                        app.open_recovered(f);
                    }
                    open = false;
                }
                Some(false) => {
                    for f in &files {
                        let _ = std::fs::remove_file(f);
                    }
                    open = false;
                }
                None => {}
            }
        }
        Dialog::About => {
            let mut close = false;
            window("About Syllabary").open(&mut open).show(ctx, |ui| {
                ui.heading(format!("Syllabary v{} — Offline Studio Edition", crate::updater::CURRENT_VERSION));
                ui.label("A fast, offline desktop word processor featuring the Google Docs menu structure, Material Design 3 pill toolbar, and modern productivity tools.");
                ui.add_space(6.0);
                ui.label("• Search the menus (Alt+/) & @ Smart Chips / Building Blocks");
                ui.label("• Checklists, Markdown auto-formatting, Pageless mode & Version History");
                ui.label("• Comments, Suggesting mode, Links, Bookmarks, Citations & Live Word Count");
                ui.label("• Works 100% offline on this computer — no cloud account or internet connection needed.");
                ui.add_space(8.0);
                if ui.button("Check for Updates…").clicked() {
                    app.act(Action::CheckForUpdates);
                    close = true;
                }
            });
            if close {
                open = false;
            }
        }
        Dialog::Update => {
            let phase = app.update.phase.clone();
            let rel = app.update.release.clone();
            let mut close = false;
            let mut start_dl = false;
            let mut cancel_dl = false;
            let mut restart_now = false;
            let mut open_url: Option<String> = None;

            window("Syllabary Update").open(&mut open).show(ctx, |ui| {
                ui.set_min_width(460.0);
                match phase {
                    crate::app::UpdatePhase::Prompt => {
                        if let Some(r) = &rel {
                            ui.heading(format!("New Version Available: v{}", r.version));
                            ui.label(
                                RichText::new(format!(
                                    "You are currently running Syllabary v{}.",
                                    crate::updater::CURRENT_VERSION
                                ))
                                .weak(),
                            );
                            ui.add_space(8.0);
                            egui::Frame::new()
                                .fill(egui::Color32::from_rgba_unmultiplied(25, 135, 84, 24))
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(25, 135, 84)))
                                .corner_radius(6)
                                .inner_margin(egui::Margin::symmetric(10, 8))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new("100% Data Preservation Guaranteed")
                                            .strong()
                                            .color(egui::Color32::from_rgb(25, 135, 84)),
                                    );
                                    ui.label("Updating replaces the executable in-place on disk. Your documents, settings, and history stay untouched.");
                                });
                            ui.add_space(8.0);
                            ui.label(RichText::new("What's New in this Release:").strong());
                            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                                let notes = if r.body.trim().is_empty() {
                                    "No changelog provided."
                                } else {
                                    r.body.trim()
                                };
                                ui.label(notes);
                            });
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if ui.button("Later").clicked() {
                                    close = true;
                                }
                                if ui.button("View on GitHub").clicked() {
                                    open_url = Some(r.html_url.clone());
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if r.asset_url.is_some()
                                        && ui
                                            .button(
                                                RichText::new(format!("Download & Update to v{}", r.version))
                                                    .strong(),
                                            )
                                            .clicked()
                                    {
                                        start_dl = true;
                                    }
                                });
                            });
                        }
                    }
                    crate::app::UpdatePhase::Downloading => {
                        let ver = rel.as_ref().map_or("latest", |r| r.version.as_str());
                        ui.heading(format!("Downloading Syllabary v{ver}…"));
                        let (down, tot) = app
                            .update
                            .progress
                            .lock()
                            .map(|p| *p)
                            .unwrap_or((0, 0));
                        let frac = if tot > 0 {
                            (down as f32 / tot as f32).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        ui.add(egui::ProgressBar::new(frac).show_percentage().animate(true));
                        let status = if tot > 0 {
                            format!(
                                "{:.1} MB / {:.1} MB",
                                down as f64 / (1024.0 * 1024.0),
                                tot as f64 / (1024.0 * 1024.0)
                            )
                        } else if down > 0 {
                            format!("{:.1} MB downloaded", down as f64 / (1024.0 * 1024.0))
                        } else {
                            "Connecting to GitHub…".into()
                        };
                        ui.label(RichText::new(status).weak());
                        ui.add_space(8.0);
                        if ui.button("Cancel").clicked() {
                            cancel_dl = true;
                        }
                    }
                    crate::app::UpdatePhase::ReadyToRestart(ver) => {
                        ui.heading(format!("Updated to Syllabary v{ver}!"));
                        ui.label("The new executable has been installed in-place. Restart Syllabary now (or whenever you finish your document) to use the new version.");
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("Later").clicked() {
                                close = true;
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(RichText::new("Restart Now").strong()).clicked() {
                                    restart_now = true;
                                }
                            });
                        });
                    }
                    crate::app::UpdatePhase::Error(msg) => {
                        ui.heading("Update Failed");
                        ui.label(msg);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Close").clicked() {
                                close = true;
                            }
                            if ui.button("Open GitHub Releases").clicked() {
                                open_url = Some(crate::updater::GITHUB_RELEASES_PAGE.to_string());
                            }
                        });
                    }
                    crate::app::UpdatePhase::Idle => {
                        close = true;
                    }
                }
            });

            if close {
                open = false;
            }
            if let Some(u) = open_url {
                app.act(Action::OpenLink(u));
            }
            if start_dl {
                app.start_update_download();
            }
            if cancel_dl {
                app.update.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                open = false;
            }
            if restart_now {
                crate::updater::relaunch_and_exit();
            }
        }
        Dialog::Shortcuts => {
            let mut kp = app.cfg.shortcut_preset;
            window("Keyboard shortcuts").open(&mut open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Shortcut keys:").strong());
                    for p in ShortcutPreset::ALL {
                        ui.selectable_value(&mut kp, p, p.label());
                    }
                });
                ui.separator();
                let script_pair = format!("{} / {}", kp.superscript(), kp.subscript());
                let size_pair = format!("{} / {}", kp.grow_font(), kp.shrink_font());
                let styles_row = format!("{} / {} / {} / {}", kp.style_normal(), kp.style_h1(), kp.style_h2(), kp.style_h3());
                let align_row = format!("{} / {} / {} / {}", kp.align_left(), kp.align_center(), kp.align_right(), kp.align_justify());
                let indent_row = format!("{} / {}", kp.indent(), kp.outdent());
                let list_row = format!("{} / {} / {}", kp.number_list(), kp.bullet_list(), kp.checklist());
                let rows: &[(&str, &str)] = &[
                    ("Alt+/", "Search the menus (command palette)"),
                    ("@", "Insert Smart Chips, Date chips & Building Blocks"),
                    ("Ctrl+N / Ctrl+O / Ctrl+S", "New, Open, Save"),
                    (kp.save_as(), "Save As"),
                    (kp.close_tab(), "Close tab"),
                    ("Ctrl+P", "Export / Print PDF"),
                    ("Ctrl+Z / Ctrl+Y", "Undo / Redo"),
                    ("Ctrl+X / C / V", "Cut / Copy / Paste"),
                    (kp.paste_plain(), "Paste without formatting"),
                    (kp.replace(), "Find and replace"),
                    ("Ctrl+B / I / U", "Bold / Italic / Underline"),
                    (kp.strikethrough(), "Strikethrough"),
                    (&script_pair, "Superscript / Subscript"),
                    (&size_pair, "Increase / Decrease font size"),
                    (&styles_row, "Normal text / Heading 1 / Heading 2 / Heading 3"),
                    (&align_row, "Align Left / Center / Right / Justify"),
                    (&indent_row, "Increase / Decrease indent"),
                    (&list_row, "Numbered list / Bulleted list / Checklist"),
                    (kp.clear_format(), "Clear formatting"),
                    ("Ctrl+K", "Insert or edit link"),
                    ("Ctrl+Alt+M", "Add comment"),
                    ("Ctrl+Alt+F", "Insert footnote"),
                    (kp.spellcheck(), "Spelling and grammar check"),
                    (kp.word_count(), "Word count"),
                    (kp.dictionary(), "Dictionary / Define word"),
                    ("Ctrl+Shift+F", "Compact mode (hide menus)"),
                    ("Ctrl+Alt+Shift+H", "Version history"),
                ];
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    egui::Grid::new("ks").num_columns(2).striped(true).spacing(vec2(24.0, 5.0)).show(ui, |ui| {
                        for (k, d) in rows {
                            ui.label(RichText::new(*k).strong());
                            ui.label(*d);
                            ui.end_row();
                        }
                    });
                });
            });
            if kp != app.cfg.shortcut_preset {
                app.cfg.shortcut_preset = kp;
                app.save_config();
            }
        }
        Dialog::SearchMenus(mut query) => {
            search_menus_dialog(app, ctx, &mut open, &mut query);
            if open && matches!(app.dialog, Dialog::SearchMenus(_)) {
                app.dialog = Dialog::SearchMenus(query);
            }
        }
        Dialog::AtMenu(mut query) => {
            at_menu_dialog(app, ctx, &mut open, &mut query);
            if open && matches!(app.dialog, Dialog::AtMenu(_)) {
                app.dialog = Dialog::AtMenu(query);
            }
        }
        Dialog::Link { mut text, mut url } => {
            link_dialog(app, ctx, &mut open, &mut text, &mut url);
            if open && matches!(app.dialog, Dialog::Link { .. }) {
                app.dialog = Dialog::Link { text, url };
            }
        }
        Dialog::Comment(mut text) => {
            let mut add = false;
            let mut cancel = false;
            window("Add comment").open(&mut open).show(ctx, |ui| {
                ui.set_min_width(340.0);
                let quote = {
                    let t = app.tab_ref();
                    if t.has_selection() {
                        let (a, b) = t.selection();
                        t.doc.text_range(a, b)
                    } else {
                        String::new()
                    }
                };
                if !quote.is_empty() {
                    let short: String = quote.chars().take(70).collect();
                    ui.label(RichText::new(format!("“{short}”")).italics().weak());
                }
                let r = ui.add(egui::TextEdit::multiline(&mut text).desired_rows(3).desired_width(320.0).hint_text("Comment or note..."));
                r.request_focus();
                if ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.ctrl) && !text.trim().is_empty() {
                    add = true;
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(!text.trim().is_empty(), egui::Button::new("Comment")).clicked() {
                        add = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            if add {
                app.act(Action::AddComment(text));
                open = false;
            } else if cancel {
                open = false;
            } else if open && matches!(app.dialog, Dialog::Comment(_)) {
                app.dialog = Dialog::Comment(text);
            }
        }
        Dialog::Bookmark(mut name) => {
            let mut add = false;
            let existing = app.tab_ref().doc.bookmarks.clone();
            let mut jump = None;
            window("Bookmarks").open(&mut open).show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.label("Add a bookmark at the current paragraph so links can jump directly to it:");
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(220.0).hint_text("Bookmark name"));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !name.trim().is_empty() {
                        add = true;
                    }
                    if ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Add")).clicked() {
                        add = true;
                    }
                });
                if !existing.is_empty() {
                    ui.separator();
                    ui.label(RichText::new("Existing bookmarks in document:").weak().small());
                    for (bname, pi) in &existing {
                        ui.horizontal(|ui| {
                            if ui.button(format!("🔖 {bname} (¶{})", pi + 1)).clicked() {
                                jump = Some(*pi);
                            }
                        });
                    }
                }
            });
            if add {
                let bname = name.trim().to_string();
                let t = app.tab();
                let pi = t.doc.clamp(t.caret).p;
                t.doc.bookmarks.retain(|(n, _)| n != &bname);
                t.doc.bookmarks.push((bname.clone(), pi));
                t.dirty = true;
                app.show_nav = true;
                app.toast(format!("Added bookmark “{bname}”"));
                open = false;
            } else if let Some(pi) = jump {
                app.act(Action::GoToPara(pi));
                open = false;
            } else if open && matches!(app.dialog, Dialog::Bookmark(_)) {
                app.dialog = Dialog::Bookmark(name);
            }
        }
        Dialog::Citations(mut entry) => {
            citations_dialog(app, ctx, &mut open, &mut entry);
            if open && matches!(app.dialog, Dialog::Citations(_)) {
                app.dialog = Dialog::Citations(entry);
            }
        }
        Dialog::Dictionary => {
            dictionary_dialog(app, ctx, &mut open, now);
        }
        Dialog::Rename(mut name) => {
            let mut commit = false;
            let mut cancel = false;
            window("Rename document").open(&mut open).show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.label("Enter a new document title:");
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(300.0));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !name.trim().is_empty() {
                    commit = true;
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(!name.trim().is_empty(), egui::Button::new("OK")).clicked() {
                        commit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            if commit {
                let t = app.tab();
                t.name = name.trim().to_string();
                t.doc.info.title = t.name.clone();
                t.dirty = true;
                open = false;
            } else if cancel {
                open = false;
            } else if open && matches!(app.dialog, Dialog::Rename(_)) {
                app.dialog = Dialog::Rename(name);
            }
        }
        Dialog::Print(mut cfg) => {
            print_dialog(app, ctx, &mut open, &mut cfg);
            if open && matches!(app.dialog, Dialog::Print(_)) {
                app.dialog = Dialog::Print(cfg);
            }
        }
        Dialog::ImageProps(img_id) => {
            image_props_dialog(app, ctx, &mut open, img_id, now);
        }
        Dialog::InsertTable {
            mut rows,
            mut cols,
            mut header_shading,
        } => {
            insert_table_dialog(app, ctx, &mut open, &mut rows, &mut cols, &mut header_shading);
            if open && matches!(app.dialog, Dialog::InsertTable { .. }) {
                app.dialog = Dialog::InsertTable {
                    rows,
                    cols,
                    header_shading,
                };
            }
        }
        Dialog::TableMath {
            table_id,
            mut row,
            mut col,
            mut op,
            mut use_above,
            mut use_left,
            mut chosen,
            mut keep_sig_figs,
            mut expr,
            mut picking_target,
        } => {
            table_math_dialog(
                app,
                ctx,
                &mut open,
                table_id,
                &mut row,
                &mut col,
                &mut op,
                &mut use_above,
                &mut use_left,
                &mut chosen,
                &mut keep_sig_figs,
                &mut expr,
                &mut picking_target,
            );
            if open && matches!(app.dialog, Dialog::TableMath { .. }) {
                app.dialog = Dialog::TableMath {
                    table_id,
                    row,
                    col,
                    op,
                    use_above,
                    use_left,
                    chosen,
                    keep_sig_figs,
                    expr,
                    picking_target,
                };
            }
        }
    }
    if !open {
        app.dialog = Dialog::None;
        ctx.memory_mut(|m| m.request_focus(app.doc_id));
    }
}

fn search_menus_dialog(app: &mut App, ctx: &egui::Context, open: &mut bool, query: &mut String) {
    let kp = app.cfg.shortcut_preset;
    let items: &[(Icon, &str, &str, Action)] = &[
        // File
        (Icon::New, "File > New document", "Ctrl+N", Action::New),
        (Icon::Open, "File > Open…", "Ctrl+O", Action::Open),
        (Icon::CopyDoc, "File > Make a copy", "", Action::MakeCopy),
        (Icon::Save, "File > Save", "Ctrl+S", Action::Save),
        (Icon::Save, "File > Save As", kp.save_as(), Action::SaveAs),
        (Icon::Pdf, "File > Download as PDF (.pdf)", "", Action::ExportPdf),
        (Icon::Save, "File > Download as Google Docs (.gdoc)", "", Action::ExportGdoc),
        (Icon::Save, "File > Download as Microsoft Word (.docx)", "", Action::ExportDocx),
        (Icon::Save, "File > Download as OpenDocument Text (.odt)", "", Action::ExportOdt),
        (Icon::Save, "File > Download as Rich Text (.rtf)", "", Action::ExportRtf),
        (Icon::Save, "File > Download as Plain Text (.txt)", "", Action::ExportTxt),
        (Icon::Save, "File > Download as Markdown (.md)", "", Action::ExportMd),
        (Icon::Save, "File > Download as Web Page (.html)", "", Action::ExportHtml),
        (Icon::EditMode, "File > Rename document…", "", Action::Dialog(Dialog::Rename(app.tab_ref().title()))),
        (Icon::EditMode, "File > Version history > Name current version…", "", Action::OpenNameVersion),
        (Icon::History, "File > Version history > See version history", "Ctrl+Alt+Shift+H", Action::ToggleHistory),
        (Icon::PageSetup, "File > Document details", "", Action::Dialog(Dialog::DocProps)),
        (Icon::Pageless, "File > Page setup (Pages / Pageless)", "", Action::Dialog(Dialog::PageSetup)),
        (Icon::Print, "File > Print…", "Ctrl+P", Action::Print),
        (Icon::Star, "File > Star / unstar document", "", Action::ToggleStar),
        // Edit
        (Icon::Undo, "Edit > Undo", "Ctrl+Z", Action::Undo),
        (Icon::Redo, "Edit > Redo", "Ctrl+Y", Action::Redo),
        (Icon::Cut, "Edit > Cut", "Ctrl+X", Action::Cut),
        (Icon::Copy, "Edit > Copy", "Ctrl+C", Action::Copy),
        (Icon::Paste, "Edit > Paste", "Ctrl+V", Action::Paste),
        (Icon::Paste, "Edit > Paste without formatting", kp.paste_plain(), Action::PastePlain),
        (Icon::EditMode, "Edit > Select all", "Ctrl+A", Action::SelectAll),
        (Icon::CopyDoc, "Edit > Duplicate paragraph", "Ctrl+D", Action::DuplicatePara),
        (Icon::Find, "Edit > Find", "Ctrl+F", Action::Dialog(Dialog::Find(false))),
        (Icon::Find, "Edit > Find and replace", kp.replace(), Action::Dialog(Dialog::Find(true))),
        (Icon::PageBreak, "Edit > Go to page…", "Ctrl+G", Action::Dialog(Dialog::GoTo(1))),
        // View
        (Icon::EditMode, "View > Mode: Editing", "", Action::SetMode(DocMode::Editing)),
        (Icon::SuggestMode, "View > Mode: Suggesting", "", Action::SetMode(DocMode::Suggesting)),
        (Icon::ViewMode, "View > Mode: Viewing", "", Action::SetMode(DocMode::Viewing)),
        (Icon::NavPane, "View > Show document outline", "", Action::ToggleNav),
        (Icon::Comment, "View > Show comments & suggestions", "", Action::ToggleComments),
        (Icon::Pageless, "View > Toggle Pageless / Pages mode", "", Action::TogglePageless),
        (Icon::ZoomIn, "View > Zoom: Fit page width", "", Action::ZoomMode(ZoomMode::PageWidth)),
        (Icon::ZoomIn, "View > Zoom: Fit margin width", "", Action::ZoomMode(ZoomMode::MarginWidth)),
        (Icon::ZoomOut, "View > Zoom: Full page", "", Action::ZoomMode(ZoomMode::FullPage)),
        (Icon::ZoomIn, "View > Zoom in", "Ctrl+=", Action::ZoomIn),
        (Icon::ZoomOut, "View > Zoom out", "Ctrl+-", Action::ZoomOut),
        (Icon::ChevronUp, "View > Compact mode (hide menus)", "Ctrl+Shift+F", Action::ToggleCompact),
        (Icon::Moon, "View > Toggle Dark / Light theme", "", Action::ToggleDark),
        (Icon::DocsLogo, "View > Icon design sheet", "", Action::Dialog(Dialog::DesignSheet)),
        // Insert
        (Icon::Image, "Insert > Image from file…", "", Action::InsertImage),
        (Icon::Table, "Insert > Table… (custom rows & columns)", "", Action::Dialog(Dialog::InsertTable { rows: 3, cols: 3, header_shading: None })),
        (Icon::Table, "Insert > Table (2×2)", "", Action::InsertBuildingBlock("table_2x2")),
        (Icon::Table, "Insert > Table (3×3)", "", Action::InsertBuildingBlock("table_3x3")),
        (Icon::Table, "Insert > Table > Calculate Sum / Math…", "", Action::OpenTableMath),
        (Icon::Table, "Insert > Table > Select row", "", Action::TableSelectRow),
        (Icon::Table, "Insert > Table > Select column", "", Action::TableSelectCol),
        (Icon::Table, "Insert > Table > Auto-fit column width", "", Action::TableAutoFitCol),
        (Icon::Table, "Insert > Table > Auto-fit row height", "", Action::TableAutoFitRow),
        (Icon::Table, "Insert > Table > Toggle text wrap", "", Action::TableToggleWrap),
        (Icon::Table, "Insert > Table > Merge right", "", Action::TableMergeRight),
        (Icon::Table, "Insert > Table > Merge row", "", Action::TableMergeRow),
        (Icon::Table, "Insert > Table > Split cell", "", Action::TableSplitCell),
        (Icon::Table, "Insert > Table > Insert row above", "", Action::TableInsertRow { above: true }),
        (Icon::Table, "Insert > Table > Insert row below", "", Action::TableInsertRow { above: false }),
        (Icon::Table, "Insert > Table > Insert column left", "", Action::TableInsertCol { left: true }),
        (Icon::Table, "Insert > Table > Insert column right", "", Action::TableInsertCol { left: false }),
        (Icon::Table, "Insert > Table > Delete row", "", Action::TableDeleteRow),
        (Icon::Table, "Insert > Table > Delete column", "", Action::TableDeleteCol),
        (Icon::BuildingBlock, "Insert > Building block: Meeting notes", "", Action::InsertBuildingBlock("meeting")),
        (Icon::BuildingBlock, "Insert > Building block: Email draft", "", Action::InsertBuildingBlock("email")),
        (Icon::BuildingBlock, "Insert > Building block: Project roadmap", "", Action::InsertBuildingBlock("roadmap")),
        (Icon::BuildingBlock, "Insert > Building block: Launch checklist", "", Action::InsertBuildingBlock("checklist")),
        (Icon::BuildingBlock, "Insert > Building block: Code block", "", Action::InsertBuildingBlock("code")),
        (Icon::SmartChip, "Insert > Smart chip: Today's date (@today)", "", Action::InsertSmartChip("today")),
        (Icon::SmartChip, "Insert > Smart chip: Tomorrow's date (@tomorrow)", "", Action::InsertSmartChip("tomorrow")),
        (Icon::SmartChip, "Insert > Smart chip: Person (@person)", "", Action::InsertSmartChip("person")),
        (Icon::SmartChip, "Insert > Smart chip: Status — Not started", "", Action::InsertSmartChip("status_not_started")),
        (Icon::SmartChip, "Insert > Smart chip: Status — In progress", "", Action::InsertSmartChip("status_in_progress")),
        (Icon::SmartChip, "Insert > Smart chip: Status — In review", "", Action::InsertSmartChip("status_review")),
        (Icon::SmartChip, "Insert > Smart chip: Status — Approved", "", Action::InsertSmartChip("status_approved")),
        (Icon::SmartChip, "Insert > Smart chip: Priority — High", "", Action::InsertSmartChip("priority_high")),
        (Icon::SmartChip, "Insert > Smart chip: Timer", "", Action::InsertSmartChip("timer")),
        (Icon::HorizontalRule, "Insert > Horizontal line", "", Action::HorizontalRule),
        (Icon::Footnote, "Insert > Footnote", "Ctrl+Alt+F", Action::InsertFootnote),
        (Icon::Symbol, "Insert > Emoji (System Library)", "", Action::SystemEmojiPicker),
        (Icon::Symbol, "Insert > Special characters & emoji", "", Action::Dialog(Dialog::Symbol)),
        (Icon::SmartChip, "Insert > Date & time…", "", Action::Dialog(Dialog::DateTime)),
        (Icon::PageBreak, "Insert > Page break", "Ctrl+Enter", Action::PageBreak),
        (Icon::Link, "Insert > Link", "Ctrl+K", Action::Dialog(Dialog::Link { text: String::new(), url: String::new() })),
        (Icon::Comment, "Insert > Comment", "Ctrl+Alt+M", Action::Dialog(Dialog::Comment(String::new()))),
        (Icon::Bookmark, "Insert > Bookmark", "", Action::Dialog(Dialog::Bookmark(String::new()))),
        (Icon::NavPane, "Insert > Table of contents", "", Action::InsertToc),
        // Format
        (Icon::Bold, "Format > Text > Bold", "Ctrl+B", Action::Bold),
        (Icon::Italic, "Format > Text > Italic", "Ctrl+I", Action::Italic),
        (Icon::Underline, "Format > Text > Underline", "Ctrl+U", Action::Underline),
        (Icon::Strike, "Format > Text > Strikethrough", kp.strikethrough(), Action::Strike),
        (Icon::Superscript, "Format > Text > Superscript", kp.superscript(), Action::Script(1)),
        (Icon::Subscript, "Format > Text > Subscript", kp.subscript(), Action::Script(-1)),
        (Icon::Plus, "Format > Text > Increase font size", kp.grow_font(), Action::StepFontSize(1.0)),
        (Icon::Minus, "Format > Text > Decrease font size", kp.shrink_font(), Action::StepFontSize(-1.0)),
        (Icon::TextColor, "Format > Text > Capitalization: UPPERCASE", "", Action::Case(CaseChange::Upper)),
        (Icon::TextColor, "Format > Text > Capitalization: lowercase", "", Action::Case(CaseChange::Lower)),
        (Icon::TextColor, "Format > Text > Capitalization: Title Case", "", Action::Case(CaseChange::Title)),
        (Icon::TextColor, "Format > Text > Capitalization: Sentence case", "", Action::Case(CaseChange::Sentence)),
        (Icon::TextColor, "Format > Text > Font…", "", Action::Dialog(Dialog::Font)),
        (Icon::EditMode, "Format > Paragraph styles > Normal text", kp.style_normal(), Action::Style(0)),
        (Icon::EditMode, "Format > Paragraph styles > Title", "", Action::Style(2)),
        (Icon::EditMode, "Format > Paragraph styles > Subtitle", "", Action::Style(3)),
        (Icon::EditMode, "Format > Paragraph styles > Heading 1", kp.style_h1(), Action::Style(4)),
        (Icon::EditMode, "Format > Paragraph styles > Heading 2", kp.style_h2(), Action::Style(5)),
        (Icon::EditMode, "Format > Paragraph styles > Heading 3", kp.style_h3(), Action::Style(6)),
        (Icon::EditMode, "Format > Paragraph styles > Quote", "", Action::Style(7)),
        (Icon::AlignLeft, "Format > Align & indent > Left", kp.align_left(), Action::Align(Align::Left)),
        (Icon::AlignCenter, "Format > Align & indent > Center", kp.align_center(), Action::Align(Align::Center)),
        (Icon::AlignRight, "Format > Align & indent > Right", kp.align_right(), Action::Align(Align::Right)),
        (Icon::AlignJustify, "Format > Align & indent > Justified", kp.align_justify(), Action::Align(Align::Justify)),
        (Icon::Indent, "Format > Align & indent > Increase indent", "Ctrl+]", Action::Indent(1)),
        (Icon::Outdent, "Format > Align & indent > Decrease indent", "Ctrl+[", Action::Indent(-1)),
        (Icon::LineSpacing, "Format > Line & paragraph spacing…", "", Action::Dialog(Dialog::Paragraph)),
        (Icon::LineSpacing, "Format > Line spacing: Single (1.0)", "", Action::LineSpacing(1.0)),
        (Icon::LineSpacing, "Format > Line spacing: 1.15", "", Action::LineSpacing(1.15)),
        (Icon::LineSpacing, "Format > Line spacing: 1.5", "", Action::LineSpacing(1.5)),
        (Icon::LineSpacing, "Format > Line spacing: Double (2.0)", "", Action::LineSpacing(2.0)),
        (Icon::Checklist, "Format > Bullets & numbering > Checklist", kp.checklist(), Action::List(ListKind::Checklist)),
        (Icon::Bullets, "Format > Bullets & numbering > Bulleted list", kp.bullet_list(), Action::List(ListKind::Bullet)),
        (Icon::Numbers, "Format > Bullets & numbering > Numbered list", kp.number_list(), Action::List(ListKind::Number)),
        (Icon::PageSetup, "Format > Page orientation (Portrait / Landscape)", "", Action::PageEdit(PageEdit::Swap)),
        (Icon::Painter, "Format > Paint format (Format painter)", "Ctrl+Alt+C", Action::FormatPainter),
        (Icon::Clear, "Format > Clear formatting", kp.clear_format(), Action::ClearFormat),
        // Tools
        (Icon::Spell, "Tools > Spelling and grammar check", kp.spellcheck(), Action::Dialog(Dialog::Spell(0))),
        (Icon::Spell, "Tools > Toggle spellcheck as you type", "", Action::ToggleSpell),
        (Icon::WordCount, "Tools > Word count", kp.word_count(), Action::Dialog(Dialog::WordCount)),
        (Icon::WordCount, "Tools > Display live word count while typing", "", Action::ToggleLiveWordCount),
        (Icon::Citation, "Tools > Citations & Bibliography", "", Action::Dialog(Dialog::Citations(CitationEntry::default()))),
        (Icon::Dictionary, "Tools > Dictionary / Define word", kp.dictionary(), Action::Dialog(Dialog::Dictionary)),
        (Icon::EditMode, "Tools > SmartCorrect / Auto-substitutions…", "", Action::Dialog(Dialog::SmartCorrect(String::new(), String::new()))),
        (Icon::Preferences, "Tools > Settings / Preferences", "", Action::Dialog(Dialog::Prefs)),
        // Window & Help
        (Icon::New, "Window > Next document tab", "Ctrl+Tab", Action::NextTab),
        (Icon::Close, "Window > Close all tabs", "", Action::CloseAll),
        (Icon::Shortcuts, "Help > Keyboard shortcuts", "Ctrl+/", Action::Dialog(Dialog::Shortcuts)),
        (Icon::About, "Help > Check for Updates…", "", Action::CheckForUpdates),
        (Icon::About, "Help > About Syllabary", "", Action::Dialog(Dialog::About)),
    ];

    let mut trigger: Option<Action> = None;
    window("Search the menus (Alt+/)").open(open).show(ctx, |ui| {
        ui.set_min_width(440.0);
        let r = ui.add(egui::TextEdit::singleline(query).desired_width(420.0).hint_text("Type a menu command (e.g. checklist, pageless, heading, word count, table)..."));
        r.request_focus();
        let q = query.trim().to_lowercase();
        let filtered: Vec<_> = items
            .iter()
            .filter(|(_, label, sc, _)| q.is_empty() || label.to_lowercase().contains(&q) || sc.to_lowercase().contains(&q))
            .collect();
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some((_, _, _, act)) = filtered.first() {
                trigger = Some((*act).clone());
            }
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            for (ic, label, sc, act) in filtered.into_iter().take(40) {
                let (rect, resp) = ui.allocate_exact_size(vec2(410.0, 26.0), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, app.theme.hover);
                }
                let ic_r = egui::Rect::from_center_size(egui::pos2(rect.left() + 14.0, rect.center().y), vec2(15.0, 15.0));
                draw_icon(ui.painter(), ic_r, *ic, app.theme.icon, app.theme.accent);
                ui.painter().text(
                    egui::pos2(rect.left() + 30.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    *label,
                    egui::FontId::proportional(12.5),
                    app.theme.text,
                );
                if !sc.is_empty() {
                    ui.painter().text(
                        egui::pos2(rect.right() - 8.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        *sc,
                        egui::FontId::proportional(11.0),
                        app.theme.subtle,
                    );
                }
                if resp.clicked() {
                    trigger = Some(act.clone());
                }
            }
        });
    });
    if let Some(act) = trigger {
        *open = false;
        app.act(act);
    }
}

fn at_menu_dialog(app: &mut App, ctx: &egui::Context, open: &mut bool, query: &mut String) {
    let entries: &[(Icon, &str, &str, Action)] = &[
        (Icon::SmartChip, "Today's date chip (@today)", "Smart Chip", Action::InsertSmartChip("today")),
        (Icon::SmartChip, "Tomorrow's date chip (@tomorrow)", "Smart Chip", Action::InsertSmartChip("tomorrow")),
        (Icon::SmartChip, "Yesterday's date chip (@yesterday)", "Smart Chip", Action::InsertSmartChip("yesterday")),
        (Icon::SmartChip, "Status chip: Not started", "Dropdown", Action::InsertSmartChip("status_not_started")),
        (Icon::SmartChip, "Status chip: In progress", "Dropdown", Action::InsertSmartChip("status_in_progress")),
        (Icon::SmartChip, "Status chip: Under review", "Dropdown", Action::InsertSmartChip("status_review")),
        (Icon::SmartChip, "Status chip: Approved ✓", "Dropdown", Action::InsertSmartChip("status_approved")),
        (Icon::SmartChip, "Priority chip: High", "Dropdown", Action::InsertSmartChip("priority_high")),
        (Icon::SmartChip, "Priority chip: Medium", "Dropdown", Action::InsertSmartChip("priority_med")),
        (Icon::SmartChip, "Priority chip: Low", "Dropdown", Action::InsertSmartChip("priority_low")),
        (Icon::SmartChip, "Person chip (@Owner)", "Smart Chip", Action::InsertSmartChip("person")),
        (Icon::SmartChip, "Stopwatch / Timer chip (⏱ 25:00)", "Smart Chip", Action::InsertSmartChip("timer")),
        (Icon::BuildingBlock, "Meeting notes", "Building Block", Action::InsertBuildingBlock("meeting")),
        (Icon::BuildingBlock, "Email draft", "Building Block", Action::InsertBuildingBlock("email")),
        (Icon::BuildingBlock, "Project roadmap", "Building Block", Action::InsertBuildingBlock("roadmap")),
        (Icon::BuildingBlock, "Launch checklist", "Building Block", Action::InsertBuildingBlock("checklist")),
        (Icon::BuildingBlock, "Code block", "Building Block", Action::InsertBuildingBlock("code")),
        (Icon::Image, "Image from file…", "Insert", Action::InsertImage),
        (Icon::Table, "Table (2×2)", "Insert", Action::InsertBuildingBlock("table_2x2")),
        (Icon::Table, "Table (3×3)", "Insert", Action::InsertBuildingBlock("table_3x3")),
        (Icon::Checklist, "Checklist", "Format", Action::List(ListKind::Checklist)),
        (Icon::Bullets, "Bulleted list", "Format", Action::List(ListKind::Bullet)),
        (Icon::Numbers, "Numbered list", "Format", Action::List(ListKind::Number)),
        (Icon::HorizontalRule, "Horizontal line / divider", "Insert", Action::HorizontalRule),
        (Icon::EditMode, "Heading 1", "Style", Action::Style(4)),
        (Icon::EditMode, "Heading 2", "Style", Action::Style(5)),
        (Icon::EditMode, "Heading 3", "Style", Action::Style(6)),
        (Icon::NavPane, "Table of contents", "Insert", Action::InsertToc),
    ];

    let mut chosen: Option<Action> = None;
    window("@ Insert Smart Chips & Building Blocks").open(open).show(ctx, |ui| {
        ui.set_min_width(380.0);
        let r = ui.add(egui::TextEdit::singleline(query).desired_width(360.0).hint_text("Search @menu (e.g. today, meeting, status, code, checklist)..."));
        r.request_focus();
        let q = query.trim().to_lowercase();
        let filtered: Vec<_> = entries
            .iter()
            .filter(|(_, label, cat, _)| q.is_empty() || label.to_lowercase().contains(&q) || cat.to_lowercase().contains(&q))
            .collect();
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some((_, _, _, act)) = filtered.first() {
                chosen = Some((*act).clone());
            }
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
            for (ic, label, cat, act) in filtered {
                let (rect, resp) = ui.allocate_exact_size(vec2(355.0, 26.0), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, app.theme.hover);
                }
                let ic_r = egui::Rect::from_center_size(egui::pos2(rect.left() + 14.0, rect.center().y), vec2(15.0, 15.0));
                draw_icon(ui.painter(), ic_r, *ic, app.theme.accent, app.theme.accent);
                ui.painter().text(
                    egui::pos2(rect.left() + 30.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    *label,
                    egui::FontId::proportional(12.5),
                    app.theme.text,
                );
                ui.painter().text(
                    egui::pos2(rect.right() - 8.0, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    *cat,
                    egui::FontId::proportional(11.0),
                    app.theme.subtle,
                );
                if resp.clicked() {
                    chosen = Some(act.clone());
                }
            }
        });
    });
    if let Some(act) = chosen {
        // If the user typed '@' in the document to open this menu, remove the trailing '@' first
        let now = ctx.input(|i| i.time);
        let t = app.tab();
        let c = t.doc.clamp(t.caret);
        if c.c > 0 && t.doc.paras[c.p].text.get(c.c - 1) == Some(&'@') {
            t.checkpoint(EditKind::Typing, now);
            let prev = Pos::new(c.p, c.c - 1);
            t.doc.delete_range(prev, c);
            t.set_caret(prev, false);
            t.changed();
        }
        *open = false;
        app.act(act);
    }
}

fn link_dialog(app: &mut App, ctx: &egui::Context, open: &mut bool, text: &mut String, url: &mut String) {
    let mut apply = false;
    let mut remove = false;
    let mut cancel = false;
    let headings: Vec<(usize, String)> = app
        .tab_ref()
        .doc
        .paras
        .iter()
        .enumerate()
        .filter(|(_, p)| matches!(p.fmt.style, 2..=6) && !p.text.is_empty())
        .map(|(i, p)| (i, p.string()))
        .collect();
    let bookmarks = app.tab_ref().doc.bookmarks.clone();

    window("Insert / Edit Link (Ctrl+K)").open(open).show(ctx, |ui| {
        ui.set_min_width(380.0);
        egui::Grid::new("lnk").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
            ui.label("Text");
            ui.add(egui::TextEdit::singleline(text).desired_width(300.0).hint_text("Display text"));
            ui.end_row();
            ui.label("Link");
            let r = ui.add(egui::TextEdit::singleline(url).desired_width(300.0).hint_text("Paste a link or select a heading below"));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !url.trim().is_empty() {
                apply = true;
            }
            ui.end_row();
        });
        if !headings.is_empty() || !bookmarks.is_empty() {
            ui.separator();
            ui.label(RichText::new("Headings and bookmarks in this document:").weak().small());
            egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                for (pi, h) in &headings {
                    if ui.selectable_label(*url == format!("#heading:{pi}"), format!("§ {h}")).clicked() {
                        *url = format!("#heading:{pi}");
                        if text.trim().is_empty() {
                            *text = h.clone();
                        }
                    }
                }
                for (bname, _) in &bookmarks {
                    if ui.selectable_label(*url == format!("#bookmark:{bname}"), format!("🔖 {bname}")).clicked() {
                        *url = format!("#bookmark:{bname}");
                        if text.trim().is_empty() {
                            *text = bname.clone();
                        }
                    }
                }
            });
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!url.trim().is_empty(), egui::Button::new("Apply")).clicked() {
                apply = true;
            }
            if ui.button("Remove link").clicked() {
                remove = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if remove {
        let c = app.tab_ref().doc.clamp(app.tab_ref().caret);
        app.act(Action::RemoveLink(c));
        *open = false;
    } else if cancel {
        *open = false;
    } else if apply {
        let u = if !url.contains("://") && !url.starts_with('#') && !url.starts_with("mailto:") {
            format!("https://{}", url.trim())
        } else {
            url.trim().to_string()
        };
        let t_str = text.trim().to_string();
        *open = false;
        app.act(Action::InsertLink { text: t_str, url: u });
    }
}

fn citations_dialog(app: &mut App, ctx: &egui::Context, open: &mut bool, entry: &mut CitationEntry) {
    let mut add = false;
    let mut cite_idx: Option<usize> = None;
    let mut insert_bib = false;
    let list = app.tab_ref().doc.citations.clone();
    window("Citations & Bibliography").open(open).show(ctx, |ui| {
        ui.set_min_width(420.0);
        ui.label(RichText::new("Add a citation source (APA / MLA style):").strong());
        egui::Grid::new("cit_g").num_columns(2).spacing(vec2(8.0, 5.0)).show(ui, |ui| {
            ui.label("Author(s)");
            ui.add(egui::TextEdit::singleline(&mut entry.author).desired_width(280.0).hint_text("e.g. Smith, J."));
            ui.end_row();
            ui.label("Title");
            ui.add(egui::TextEdit::singleline(&mut entry.title).desired_width(280.0).hint_text("Book, article, or website title"));
            ui.end_row();
            ui.label("Year");
            ui.add(egui::TextEdit::singleline(&mut entry.year).desired_width(120.0).hint_text("2026"));
            ui.end_row();
            ui.label("Publisher / Source");
            ui.add(egui::TextEdit::singleline(&mut entry.source).desired_width(280.0).hint_text("Journal, publisher, or URL"));
            ui.end_row();
        });
        if ui.add_enabled(!entry.author.trim().is_empty() && !entry.title.trim().is_empty(), egui::Button::new("+ Add citation source")).clicked() {
            add = true;
        }
        ui.separator();
        ui.label(RichText::new(format!("Sources in document ({})", list.len())).strong());
        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
            for (i, c) in list.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("• {} ({}). {}.", c.author, c.year, c.title));
                    if ui.small_button("Cite").clicked() {
                        cite_idx = Some(i);
                    }
                });
            }
        });
        if !list.is_empty() {
            ui.add_space(4.0);
            if ui.button("Insert Bibliography at end of document").clicked() {
                insert_bib = true;
            }
        }
    });
    if add {
        app.tab().doc.citations.push(entry.clone());
        app.tab().dirty = true;
        *entry = CitationEntry::default();
    }
    if let Some(i) = cite_idx {
        if let Some(c) = list.get(i) {
            let short_author = c.author.split(',').next().unwrap_or(&c.author).trim();
            app.act(Action::InsertText(format!(" ({short_author}, {})", c.year)));
        }
    }
    if insert_bib {
        let now = ctx.input(|i| i.time);
        let styles = named_styles();
        let t = app.tab();
        t.checkpoint(EditKind::Other, now);
        let h_ns = &styles[4];
        let mut h = Para::new(h_ns.chr, ParaFmt { style: 4, ..h_ns.fmt });
        for ch in "References".chars() {
            h.text.push(ch);
            h.styles.push(h_ns.chr);
        }
        t.doc.paras.push(h);
        for c in &list {
            let st = CharStyle::default();
            let mut p = Para::new(st, ParaFmt::default());
            let line = if c.source.is_empty() {
                format!("{} ({}). {}.", c.author, c.year, c.title)
            } else {
                format!("{} ({}). {}. {}.", c.author, c.year, c.title, c.source)
            };
            for ch in line.chars() {
                p.text.push(ch);
                p.styles.push(st);
            }
            t.doc.paras.push(p);
        }
        t.changed();
        app.toast("Inserted References section".to_string());
        *open = false;
    }
}

fn word_bounds_at(doc: &Document, pos: Pos) -> (Pos, Pos) {
    let c = doc.clamp(pos);
    let text = &doc.paras[c.p].text;
    let mut a = c.c.min(text.len());
    let mut b = a;
    while a > 0 && (text[a - 1].is_alphanumeric() || text[a - 1] == '\'') {
        a -= 1;
    }
    while b < text.len() && (text[b].is_alphanumeric() || text[b] == '\'') {
        b += 1;
    }
    (Pos::new(c.p, a), Pos::new(c.p, b))
}

fn dictionary_dialog(app: &mut App, ctx: &egui::Context, open: &mut bool, now: f64) {
    let word = {
        let t = app.tab_ref();
        if t.has_selection() {
            let (a, b) = t.selection();
            t.doc.text_range(a, b).trim().to_string()
        } else {
            let (a, b) = word_bounds_at(&t.doc, t.caret);
            t.doc.text_range(a, b).trim().to_string()
        }
    };
    let mut replace_with: Option<String> = None;
    window("Dictionary (Ctrl+Shift+Y)").open(open).show(ctx, |ui| {
        ui.set_min_width(340.0);
        if word.is_empty() {
            ui.label(RichText::new("Select a word or place the cursor inside a word in the document to inspect it.").weak());
        } else {
            ui.heading(&word);
            let chars = word.chars().count();
            let vowels = word.chars().filter(|c| "aeiouyAEIOUY".contains(*c)).count().max(1);
            ui.label(RichText::new(format!("{chars} letters • ~{vowels} syllable(s)")).weak());
            ui.separator();
            let misspelled = app.speller.available() && !app.speller.errors(&word.chars().collect::<Vec<_>>()).is_empty();
            if misspelled {
                ui.label(RichText::new("⚠ Not found in system dictionary").color(egui::Color32::from_rgb(220, 38, 38)));
            } else {
                ui.label(RichText::new("✓ Valid spelling in dictionary").color(egui::Color32::from_rgb(22, 163, 74)));
            }
            let sugg = app.speller.suggest(&word);
            if !sugg.is_empty() {
                ui.add_space(4.0);
                ui.label("Related / Suggested words (click to replace):");
                ui.horizontal_wrapped(|ui| {
                    for s in sugg {
                        if ui.button(&s).clicked() {
                            replace_with = Some(s);
                        }
                    }
                });
            }
        }
    });
    if let Some(rep) = replace_with {
        let t = app.tab();
        let (a, b) = if t.has_selection() {
            t.selection()
        } else {
            word_bounds_at(&t.doc, t.caret)
        };
        t.checkpoint(EditKind::Other, now);
        let style = t.doc.paras[a.p].styles.get(a.c).copied().unwrap_or_default();
        t.doc.delete_range(a, b);
        let e = t.doc.insert_text(a, &rep, style);
        t.set_caret(e, false);
        t.changed();
        *open = false;
    }
}

fn print_dialog(app: &mut App, ctx: &egui::Context, open: &mut bool, cfg: &mut crate::print::PrintJobConfig) {
    let mut do_print = false;
    let mut do_preview = false;
    let mut do_save_pdf = false;
    let mut cancel = false;

    window("Print Document (Ctrl+P)").open(open).show(ctx, |ui| {
        ui.set_min_width(430.0);
        egui::Grid::new("print_cfg_grid").num_columns(2).spacing(vec2(14.0, 8.0)).show(ui, |ui| {
            ui.label(RichText::new("Destination Printer").strong());
            ui.horizontal(|ui| {
                let current_name = cfg
                    .printers
                    .get(cfg.selected_printer)
                    .cloned()
                    .unwrap_or_else(|| "Default System Printer".into());
                egui::ComboBox::from_id_salt("print_dest_combo")
                    .selected_text(current_name)
                    .width(230.0)
                    .show_ui(ui, |ui| {
                        for (i, name) in cfg.printers.iter().enumerate() {
                            ui.selectable_value(&mut cfg.selected_printer, i, name);
                        }
                    });
                if ui.button("Refresh").on_hover_text("Re-scan native system printers").clicked() {
                    let refreshed = crate::print::PrintJobConfig::discover(cfg.total_pages);
                    cfg.printers = refreshed.printers;
                    cfg.selected_printer = refreshed.selected_printer;
                }
            });
            ui.end_row();

            ui.label(RichText::new("Pages").strong());
            ui.horizontal(|ui| {
                ui.radio_value(&mut cfg.all_pages, true, format!("All ({})", cfg.total_pages));
                ui.radio_value(&mut cfg.all_pages, false, "Range:");
                ui.add_enabled(
                    !cfg.all_pages,
                    egui::DragValue::new(&mut cfg.from_page).range(1..=cfg.total_pages.max(1)).speed(0.1),
                );
                ui.label("to");
                ui.add_enabled(
                    !cfg.all_pages,
                    egui::DragValue::new(&mut cfg.to_page).range(1..=cfg.total_pages.max(1)).speed(0.1),
                );
            });
            ui.end_row();

            ui.label(RichText::new("Copies").strong());
            ui.add(egui::DragValue::new(&mut cfg.copies).range(1..=99).speed(0.1));
            ui.end_row();

            ui.label(RichText::new("Color Mode").strong());
            ui.horizontal(|ui| {
                ui.selectable_value(&mut cfg.color, true, "Color (Calibrated sRGB)");
                ui.selectable_value(&mut cfg.color, false, "Black & White");
            });
            ui.end_row();

            ui.label(RichText::new("Two-Sided").strong());
            egui::ComboBox::from_id_salt("print_duplex_combo")
                .selected_text(cfg.duplex.label())
                .width(230.0)
                .show_ui(ui, |ui| {
                    for mode in [
                        crate::print::DuplexMode::OneSided,
                        crate::print::DuplexMode::LongEdge,
                        crate::print::DuplexMode::ShortEdge,
                    ] {
                        ui.selectable_value(&mut cfg.duplex, mode, mode.label());
                    }
                });
            ui.end_row();
        });

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button(RichText::new("  Print  ").strong()).clicked() {
                do_print = true;
            }
            if ui.button("System Preview…").clicked() {
                do_preview = true;
            }
            if ui.button("Save as PDF…").clicked() {
                do_save_pdf = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });

    if do_print {
        let base = app.tab_ref().name.split('.').next().unwrap_or("Document").to_string();
        let doc = app.tab_ref().doc.clone();
        match crate::print::submit_cups_print_job(ctx, &doc, &app.book, &base, cfg) {
            crate::print::PrintOutcome::Printed(msg) => app.toast(msg),
            crate::print::PrintOutcome::Cancelled => {}
            crate::print::PrintOutcome::Error(err) => app.toast(err),
        }
        *open = false;
    } else if do_preview {
        let base = app.tab_ref().name.split('.').next().unwrap_or("Document").to_string();
        let doc = app.tab_ref().doc.clone();
        match crate::print::open_system_print_preview(ctx, &doc, &app.book, &base) {
            crate::print::PrintOutcome::Printed(msg) => app.toast(msg),
            crate::print::PrintOutcome::Cancelled => {}
            crate::print::PrintOutcome::Error(err) => app.toast(err),
        }
        *open = false;
    } else if do_save_pdf {
        app.act(Action::ExportPdf);
        *open = false;
    } else if cancel {
        *open = false;
    }
}

fn confirmations(app: &mut App, ctx: &egui::Context) {
    let mr = app.mr_palette();
    match app.close_flow {
        crate::app::CloseFlow::None | crate::app::CloseFlow::ForceQuit => {}
        crate::app::CloseFlow::ConfirmTab(idx) => {
            let name = app.tabs.get(idx).map(|t| t.name.clone()).unwrap_or_default();
            let msg = format!("\"{name}\" has changes that haven't been saved.");
            let mut outcome = modernrust::PromptOutcome::Pending;
            egui::Modal::new(egui::Id::unique("confirm-close"))
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    ui.set_max_width(400.0);
                    outcome = modernrust::PromptBox::new("Save changes?", &msg)
                        .role(modernrust::Role::Warning)
                        .confirm_label("Save")
                        .secondary_label("Don't save")
                        .cancel_label("Cancel")
                        .show_inline(ui, &mr);
                });
            match outcome {
                modernrust::PromptOutcome::Confirm => {
                    app.close_flow = crate::app::CloseFlow::None;
                    app.save_then_close(idx);
                }
                modernrust::PromptOutcome::Secondary => {
                    app.close_flow = crate::app::CloseFlow::None;
                    app.close_tab(idx, true);
                }
                modernrust::PromptOutcome::Cancel => app.close_flow = crate::app::CloseFlow::None,
                modernrust::PromptOutcome::Pending => {}
            }
        }
        crate::app::CloseFlow::ConfirmQuit => {
            let n = app.tabs.iter().filter(|t| t.dirty).count();
            let msg = format!("{n} document(s) have changes that haven't been saved.");
            let mut outcome = modernrust::PromptOutcome::Pending;
            egui::Modal::new(egui::Id::unique("confirm-quit"))
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    ui.set_max_width(420.0);
                    outcome = modernrust::PromptBox::new("Unsaved documents", &msg)
                        .role(modernrust::Role::Warning)
                        .confirm_label("Save all")
                        .secondary_label("Quit without saving")
                        .cancel_label("Cancel")
                        .show_inline(ui, &mr);
                });
            match outcome {
                modernrust::PromptOutcome::Confirm => {
                    app.close_flow = crate::app::CloseFlow::None;
                    let mut ok = true;
                    for i in 0..app.tabs.len() {
                        if app.tabs[i].dirty && !app.save(i, false) {
                            ok = false;
                            break;
                        }
                    }
                    if ok {
                        app.force_quit(ctx);
                    }
                }
                modernrust::PromptOutcome::Secondary => {
                    app.force_quit(ctx);
                }
                modernrust::PromptOutcome::Cancel => app.close_flow = crate::app::CloseFlow::None,
                modernrust::PromptOutcome::Pending => {}
            }
        }
    }
}

fn find_dialog(app: &mut App, ctx: &egui::Context, open: &mut bool) {
    let now = ctx.input(|i| i.time);
    let active = app.active;
    let rev = app.tabs[active].rev;
    let matches = app.find.matches(&app.tabs[active].doc, rev);
    let (sel_a, sel_b) = app.tabs[active].selection();
    let cur = matches.iter().position(|m| m.0 == sel_a && m.1 == sel_b);
    let mut next = false;
    let mut prev = false;
    let mut replace_one = false;
    let mut replace_all = false;
    let title = if app.find.show_replace { "Find and replace" } else { "Find in document" };
    egui::Window::new(title).id(egui::Id::unique("find-window")).open(open).collapsible(false).resizable(false).anchor(egui::Align2::RIGHT_TOP, vec2(-30.0, 150.0)).show(ctx, |ui| {
        let fs = &mut app.find;
        egui::Grid::new("fr").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
            ui.label("Find");
            let r = ui.add(egui::TextEdit::singleline(&mut fs.query).desired_width(240.0).hint_text("Find in document"));
            if fs.focus {
                r.request_focus();
                fs.focus = false;
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                next = true;
                r.request_focus();
            }
            ui.end_row();
            if fs.show_replace {
                ui.label("Replace with");
                ui.add(egui::TextEdit::singleline(&mut fs.replace).desired_width(240.0));
                ui.end_row();
            }
            ui.label("Match");
            egui::ComboBox::from_id_salt("find-mode").selected_text(fs.mode.label()).width(240.0).show_ui(ui, |ui| {
                for m in [FindMode::Containing, FindMode::WholeWords, FindMode::StartingWith, FindMode::EndingWith] {
                    ui.selectable_value(&mut fs.mode, m, m.label());
                }
            });
            ui.end_row();
            ui.label("");
            ui.checkbox(&mut fs.case, "Match case");
            ui.end_row();
        });
        let status = if fs.query.is_empty() {
            String::new()
        } else if matches.is_empty() {
            "0 of 0".into()
        } else if let Some(c) = cur {
            format!("{} of {}", c + 1, matches.len())
        } else {
            format!("{} matches", matches.len())
        };
        ui.label(RichText::new(status).weak());
        ui.horizontal(|ui| {
            if ui.button("◀ Previous").clicked() {
                prev = true;
            }
            if ui.button("Next ▶").clicked() {
                next = true;
            }
            if fs.show_replace {
                if ui.add_enabled(!matches.is_empty(), egui::Button::new("Replace")).clicked() {
                    replace_one = true;
                }
                if ui.add_enabled(!matches.is_empty(), egui::Button::new("Replace all")).clicked() {
                    replace_all = true;
                }
            } else if ui.small_button("More options…").clicked() {
                fs.show_replace = true;
            }
        });
    });
    if matches.is_empty() {
        return;
    }
    let tab = &mut app.tabs[active];
    if replace_all {
        tab.checkpoint(EditKind::Other, now);
        let rep = app.find.replace.clone();
        for (a, b) in matches.iter().rev() {
            let style = tab.doc.paras[a.p].styles.get(a.c).copied().unwrap_or_default();
            let p = tab.doc.delete_range(*a, *b);
            tab.doc.insert_text(p, &rep, style);
        }
        tab.anchor = tab.doc.clamp(tab.caret);
        tab.caret = tab.anchor;
        tab.changed();
        let n = matches.len();
        app.toast(format!("Replaced {n} match{}", if n == 1 { "" } else { "es" }));
        return;
    }
    if replace_one {
        if let Some(c) = cur {
            let (a, b) = matches[c];
            tab.checkpoint(EditKind::Other, now);
            let style = tab.doc.paras[a.p].styles.get(a.c).copied().unwrap_or_default();
            let p = tab.doc.delete_range(a, b);
            let e = tab.doc.insert_text(p, &app.find.replace, style);
            tab.set_caret(e, false);
            tab.changed();
        }
        next = true;
    }
    if next || prev {
        let matches = app.find.matches(&tab.doc, tab.rev);
        if matches.is_empty() {
            return;
        }
        let (a, b) = tab.selection();
        let pick = if prev {
            matches.iter().rev().find(|m| m.1 <= a && m.0 < a).or(matches.last()).copied()
        } else {
            matches.iter().find(|m| m.0 >= b && !(m.0 == a && m.1 == b)).or(matches.first()).copied()
        };
        if let Some((s, e)) = pick {
            tab.anchor = s;
            tab.caret = e;
            tab.scroll_to_caret = true;
        }
    }
}

/// Check Spelling: walks through the misspellings from the caret onwards.
fn spell_dialog(app: &mut App, ctx: &egui::Context, open: &mut bool, now: f64) {
    if !app.speller.available() {
        window("Spelling and grammar check").open(open).show(ctx, |ui| {
            ui.label("This computer has no English spelling dictionary installed.");
            #[cfg(windows)]
            ui.label("Add one in Windows Settings › Time & language › Language.");
            #[cfg(target_os = "linux")]
            ui.label("Install a Hunspell dictionary (e.g. `sudo dnf install hunspell-en-US` on Fedora or `sudo apt install hunspell-en-us` on Ubuntu/Debian).");
            #[cfg(not(any(windows, target_os = "linux")))]
            ui.label("Install a system spelling dictionary for your language.");
        });
        return;
    }
    let tab = &app.tabs[app.active];
    let start = tab.selection().0;
    let mut found: Option<(Pos, Pos)> = None;
    let n = tab.doc.paras.len();
    'outer: for k in 0..=n {
        let p = (start.p + k) % n;
        for (a, b) in app.speller.errors(&tab.doc.paras[p].text) {
            if k == 0 && a < start.c {
                continue;
            }
            if k == n && p == start.p && a >= start.c {
                break;
            }
            found = Some((Pos::new(p, a), Pos::new(p, b)));
            break 'outer;
        }
    }
    let mut action: Option<(u8, String)> = None;
    let word = found.map(|(a, b)| tab.doc.text_range(a, b)).unwrap_or_default();
    let sugg = if found.is_some() { app.speller.suggest(&word) } else { Vec::new() };
    let context = found.map(|(a, _)| tab.doc.paras[a.p].string()).unwrap_or_default();
    window("Spelling and grammar check").open(open).show(ctx, |ui| {
        ui.set_min_width(360.0);
        if !app.speller.language.is_empty() {
            ui.label(RichText::new(format!("Dictionary: {}", app.speller.language)).weak().small());
        }
        match found {
            None => {
                ui.label(RichText::new("✓ Document looks good! No spelling suggestions.").strong());
                if ui.button("Close").clicked() {
                    action = Some((9, String::new()));
                }
            }
            Some(_) => {
                ui.label("Change:");
                ui.label(RichText::new(&word).size(18.0).color(egui::Color32::from_rgb(220, 38, 38)).strong());
                let short: String = context.chars().take(140).collect();
                ui.label(RichText::new(short).weak().italics());
                ui.separator();
                ui.label("To:");
                if sugg.is_empty() {
                    ui.label(RichText::new("(no suggestions)").weak());
                }
                ui.horizontal_wrapped(|ui| {
                    for s in &sugg {
                        if ui.button(s).clicked() {
                            action = Some((1, s.clone()));
                        }
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Ignore").clicked() {
                        action = Some((2, String::new()));
                    }
                    if ui.button("Ignore all").clicked() {
                        action = Some((3, String::new()));
                    }
                    if ui.button("Add to personal dictionary").clicked() {
                        action = Some((4, String::new()));
                    }
                });
            }
        }
    });
    let tab = &mut app.tabs[app.active];
    match (action, found) {
        (Some((9, _)), _) => *open = false,
        (Some((1, s)), Some((a, b))) => {
            tab.checkpoint(EditKind::Other, now);
            let style = tab.doc.paras[a.p].styles.get(a.c).copied().unwrap_or_default();
            tab.doc.delete_range(a, b);
            let e = tab.doc.insert_text(a, &s, style);
            tab.set_caret(e, false);
            tab.changed();
        }
        (Some((2, _)), Some((_, b))) => tab.set_caret(b, false),
        (Some((3, _)), Some(_)) => {
            app.speller.ignore(&word);
            tab.rev += 1;
        }
        (Some((4, _)), Some(_)) => {
            app.speller.add(&word);
            tab.rev += 1;
        }
        (None, Some((a, b))) if tab.anchor != a || tab.caret != b => {
            tab.anchor = a;
            tab.caret = b;
            tab.scroll_to_caret = true;
        }
        _ => {}
    }
}

fn page_setup(app: &mut App, ctx: &egui::Context, open: &mut bool, now: f64) {
    let t = app.tab_ref();
    let mut pg = t.doc.page;
    let mut pageless = t.doc.pageless;
    let mut header = t.doc.header.clone();
    let mut footer = t.doc.footer.clone();
    let mut header_align = t.doc.header_align;
    let mut footer_align = t.doc.footer_align;
    let mut header_italic = t.doc.header_italic;
    let mut footer_italic = t.doc.footer_italic;
    window("Page setup").open(open).show(ctx, |ui| {
        ui.set_min_width(380.0);
        // Google Docs Pages / Pageless tabs
        ui.horizontal(|ui| {
            ui.selectable_value(&mut pageless, false, "📄 Pages");
            ui.selectable_value(&mut pageless, true, "📜 Pageless");
        });
        ui.separator();
        if pageless {
            ui.label(RichText::new("Pageless format").strong());
            ui.label("The pageless format allows your document to flow continuously without page breaks, making wide tables and headings easier to read.");
            ui.add_space(8.0);
            ui.heading("Background color");
            ui.horizontal(|ui| {
                for c in [[255u8, 255, 255], [255, 253, 245], [250, 247, 240], [245, 248, 255], [244, 250, 246], [255, 247, 250], [248, 248, 248]] {
                    if swatch(ui, c, pg.background == c).clicked() {
                        pg.background = c;
                    }
                }
            });
        } else {
            ui.heading("Paper size and margins");
            let land = pg.width > pg.height;
            egui::Grid::new("ps").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                ui.label("Paper size");
                let cur = PAGE_SIZES.iter().find(|(_, w, h)| {
                    let (pw, ph) = if land { (*h, *w) } else { (*w, *h) };
                    (pw - pg.width).abs() < 0.01 && (ph - pg.height).abs() < 0.01
                });
                egui::ComboBox::from_id_salt("psz").selected_text(cur.map_or("Custom", |c| c.0)).width(220.0).show_ui(ui, |ui| {
                    for (name, w, h) in PAGE_SIZES {
                        if ui.selectable_label(cur.is_some_and(|c| c.0 == *name), *name).clicked() {
                            if land {
                                pg.width = *h;
                                pg.height = *w;
                            } else {
                                pg.width = *w;
                                pg.height = *h;
                            }
                        }
                    }
                });
                ui.end_row();
                ui.label("Orientation");
                ui.horizontal(|ui| {
                    if ui.selectable_label(!land, "Portrait").clicked() && land {
                        std::mem::swap(&mut pg.width, &mut pg.height);
                    }
                    if ui.selectable_label(land, "Landscape").clicked() && !land {
                        std::mem::swap(&mut pg.width, &mut pg.height);
                    }
                });
                ui.end_row();
                let max_lr = (pg.width / 2.0 - 0.5).max(0.0);
                let max_tb = (pg.height / 2.0 - 0.5).max(0.0);
                for (label, v, max) in [("Top margin", &mut pg.margin_top, max_tb), ("Bottom margin", &mut pg.margin_bottom, max_tb), ("Left margin", &mut pg.margin_left, max_lr), ("Right margin", &mut pg.margin_right, max_lr)] {
                    ui.label(label);
                    ui.add(egui::DragValue::new(v).range(0.0..=max).speed(0.05).fixed_decimals(2).suffix(" in"));
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.heading("Headers and footers");
            egui::Grid::new("hf").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                ui.label("Header");
                ui.add(egui::TextEdit::singleline(&mut header).desired_width(260.0).hint_text("Text at the top of every page"));
                ui.end_row();
                ui.label("Header style");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut header_align, Align::Left, "Left");
                    ui.selectable_value(&mut header_align, Align::Center, "Center");
                    ui.selectable_value(&mut header_align, Align::Right, "Right");
                    ui.separator();
                    ui.checkbox(&mut header_italic, "Italic");
                });
                ui.end_row();
                ui.label("Footer");
                ui.add(egui::TextEdit::singleline(&mut footer).desired_width(260.0).hint_text("Text at the bottom of every page"));
                ui.end_row();
                ui.label("Footer style");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut footer_align, Align::Left, "Left");
                    ui.selectable_value(&mut footer_align, Align::Center, "Center");
                    ui.selectable_value(&mut footer_align, Align::Right, "Right");
                    ui.separator();
                    ui.checkbox(&mut footer_italic, "Italic");
                });
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut pg.page_numbers, "Page numbers (\"Page 1\")");
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.heading("Page color");
            ui.horizontal(|ui| {
                for c in [[255u8, 255, 255], [255, 253, 245], [250, 247, 240], [245, 248, 255], [244, 250, 246], [255, 247, 250], [248, 248, 248]] {
                    if swatch(ui, c, pg.background == c).clicked() {
                        pg.background = c;
                    }
                }
            });
        }
    });
    let t = app.tab();
    if pg != t.doc.page
        || pageless != t.doc.pageless
        || header != t.doc.header
        || footer != t.doc.footer
        || header_align != t.doc.header_align
        || footer_align != t.doc.footer_align
        || header_italic != t.doc.header_italic
        || footer_italic != t.doc.footer_italic
    {
        let footer_text_changed = footer != t.doc.footer;
        let footer_align_changed = footer_align != t.doc.footer_align;
        let footer_italic_changed = footer_italic != t.doc.footer_italic;
        let page_nums_changed = pg.page_numbers != t.doc.page.page_numbers;
        t.checkpoint(EditKind::Typing, now);
        t.doc.page = pg;
        t.doc.pageless = pageless;
        t.doc.header = header;
        t.doc.footer = footer;
        t.doc.header_align = header_align;
        t.doc.footer_align = footer_align;
        t.doc.header_italic = header_italic;
        t.doc.footer_italic = footer_italic;
        if footer_text_changed || page_nums_changed {
            t.doc.page_footers.clear();
        } else if footer_align_changed || footer_italic_changed {
            let pw_pt = t.doc.page.width * 72.0;
            let ml_pt = t.doc.page.margin_left * 72.0;
            let mr_pt = t.doc.page.margin_right * 72.0;
            for pf in &mut t.doc.page_footers {
                if pf.segments.len() == 1 {
                    let seg = &mut pf.segments[0];
                    if footer_align_changed {
                        seg.align = footer_align;
                        match footer_align {
                            Align::Left | Align::Justify => {
                                seg.x_pt = ml_pt;
                                seg.max_x_pt = pw_pt - mr_pt;
                            }
                            Align::Center => {
                                seg.x_pt = ml_pt;
                                seg.max_x_pt = pw_pt - mr_pt;
                            }
                            Align::Right => {
                                seg.x_pt = ml_pt;
                                seg.max_x_pt = pw_pt - mr_pt;
                            }
                        }
                    }
                    if footer_italic_changed {
                        for r in &mut seg.runs {
                            r.style.italic = footer_italic;
                        }
                    }
                }
            }
        }
        t.changed();
        t.scroll_to_caret = false;
    }
}

fn design_sheet_dialog(ctx: &egui::Context, open: &mut bool) {
    let light = crate::theme::Theme::light();
    let dark = crate::theme::Theme::dark();
    egui::Window::new("Syllabary — Studio UI & Duotone Icon Design Sheet (Light & Dark Themes)")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .open(open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for (title, th) in [("☀ LIGHT STUDIO THEME", &light), ("☾ DARK STUDIO THEME", &dark)] {
                    egui::Frame::new()
                        .fill(th.workspace)
                        .stroke(egui::Stroke::new(1.0, th.border))
                        .corner_radius(10)
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(740.0);
                                // Theme Header & Sample Controls Strip
                                egui::Frame::new()
                                    .fill(th.surface_elevated)
                                    .stroke(egui::Stroke::new(1.0, th.border))
                                    .corner_radius(8)
                                    .inner_margin(egui::Margin::symmetric(10, 6))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            let (lr, _) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::hover());
                                            draw_icon(ui.painter(), lr, Icon::DocsLogo, th.icon, th.accent);
                                            ui.label(RichText::new(title).size(13.0).color(th.text).strong());
                                            ui.add_space(6.0);
                                            for (lbl, col) in [
                                                ("Accent", th.accent),
                                                ("Soft", th.accent_soft),
                                                ("Surface", th.surface_subtle),
                                                ("Hover", th.hover),
                                            ] {
                                                let (sr, _) = ui.allocate_exact_size(vec2(48.0, 18.0), egui::Sense::hover());
                                                ui.painter().rect_filled(sr, 5.0, col);
                                                ui.painter().rect_stroke(sr, 5.0, egui::Stroke::new(1.0, th.border), egui::StrokeKind::Inside);
                                                let tc = if lbl == "Accent" { egui::Color32::WHITE } else { th.text };
                                                ui.painter().text(sr.center(), egui::Align2::CENTER_CENTER, lbl, egui::FontId::proportional(9.5), tc);
                                            }
                                            let (mr, _) = ui.allocate_exact_size(vec2(170.0, 22.0), egui::Sense::hover());
                                            ui.painter().rect_filled(mr, 6.0, th.hover);
                                            let ib = egui::Rect::from_center_size(egui::pos2(mr.min.x + 12.0, mr.center().y), vec2(16.0, 16.0));
                                            ui.painter().rect_filled(ib, 4.0, th.accent_soft);
                                            draw_icon(ui.painter(), ib.shrink(2.0), Icon::Save, th.accent, th.accent);
                                            ui.painter().text(
                                                egui::pos2(mr.min.x + 24.0, mr.center().y),
                                                egui::Align2::LEFT_CENTER,
                                                "Save Document",
                                                egui::FontId::proportional(11.0),
                                                th.text,
                                            );
                                            let sp = egui::Rect::from_min_max(egui::pos2(mr.max.x - 42.0, mr.min.y + 3.0), egui::pos2(mr.max.x - 4.0, mr.max.y - 3.0));
                                            ui.painter().rect_filled(sp, 3.5, th.surface_elevated);
                                            ui.painter().text(sp.center(), egui::Align2::CENTER_CENTER, "Ctrl+S", egui::FontId::proportional(9.5), th.subtle);
                                        });
                                    });
                                ui.add_space(6.0);

                                // All 76 Duotone Icons Grid (10 columns × 8 rows)
                                let cols = 10;
                                let cell_w = 69.5;
                                let cell_h = 58.0;
                                egui::Grid::new(("icon-sheet-grid", th.dark))
                                    .spacing(vec2(4.5, 4.5))
                                    .show(ui, |ui| {
                                        for (idx, (ic, name, _cat)) in Icon::ALL.iter().enumerate() {
                                            let (rect, resp) = ui.allocate_exact_size(vec2(cell_w, cell_h), egui::Sense::hover());
                                            let p = ui.painter();
                                            let bg = if resp.hovered() { th.hover } else { th.surface_elevated };
                                            p.rect_filled(rect, 7.0, bg);
                                            p.rect_stroke(
                                                rect,
                                                7.0,
                                                egui::Stroke::new(1.0, if idx % 9 == 0 { th.accent.gamma_multiply(0.45) } else { th.border }),
                                                egui::StrokeKind::Inside,
                                            );
                                            let badge = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.min.y + 21.0), vec2(28.0, 28.0));
                                            p.rect_filled(badge, 7.0, if idx % 9 == 0 { th.accent_soft } else { th.surface_subtle });
                                            let ic_rect = badge.shrink(4.0);
                                            draw_icon(p, ic_rect, *ic, th.icon, th.accent);
                                            p.text(
                                                egui::pos2(rect.center().x, rect.max.y - 9.0),
                                                egui::Align2::CENTER_CENTER,
                                                *name,
                                                egui::FontId::proportional(9.0),
                                                th.subtle,
                                            );
                                            if idx % cols == cols - 1 {
                                                ui.end_row();
                                            }
                                        }
                                    });
                            });
                        });
                }
            });
        });
}

fn image_props_dialog(app: &mut App, ctx: &egui::Context, open: &mut bool, img_id: u64, now: f64) {
    let Some(img_snapshot) = app.tab_ref().doc.image(img_id).cloned() else {
        *open = false;
        return;
    };
    let para_idx = app
        .tab_ref()
        .doc
        .paras
        .iter()
        .position(|p| p.fmt.image_id == img_id);
    let cur_align = para_idx
        .map_or(Align::Center, |pi| app.tab_ref().doc.paras[pi].fmt.align);
    let max_w_in = {
        let pg = &app.tab_ref().doc.page;
        (pg.width - pg.margin_left - pg.margin_right).max(1.0)
    };
    let nat_aspect = img_snapshot.aspect_ratio();

    let mut w_in = img_snapshot.width_in;
    let mut h_in = img_snapshot.height_in;
    let mut caption = img_snapshot.caption.clone();
    let mut align = cur_align;
    let mut replace_clicked = false;
    let mut delete_clicked = false;
    let mut done_clicked = false;

    window("Image Size & Caption").open(open).show(ctx, |ui| {
        ui.set_min_width(400.0);
        ui.horizontal(|ui| {
            let (ir, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
            draw_icon(ui.painter(), ir, Icon::Image, app.theme.icon, app.theme.accent);
            ui.label(RichText::new(&img_snapshot.name).strong());
            ui.label(
                RichText::new(format!(
                    "({} × {} px)",
                    img_snapshot.natural_size[0], img_snapshot.natural_size[1]
                ))
                .weak()
                .small(),
            );
        });
        ui.add_space(6.0);

        ui.label(RichText::new("Caption").strong());
        let cap_resp = ui.add(
            egui::TextEdit::singleline(&mut caption)
                .desired_width(385.0)
                .hint_text("Add a caption below the image (e.g. Figure 1: Architecture overview)…"),
        );
        if cap_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            done_clicked = true;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Quick prefix:").weak().small());
            if ui.small_button("Figure 1: ").clicked() && !caption.starts_with("Figure ") {
                caption = format!("Figure 1: {caption}");
            }
            if ui.small_button("Photo: ").clicked() && !caption.starts_with("Photo: ") {
                caption = format!("Photo: {caption}");
            }
            if !caption.is_empty() && ui.small_button("Clear caption").clicked() {
                caption.clear();
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        ui.label(RichText::new("Dimensions & Scale").strong());
        ui.horizontal(|ui| {
            ui.label("Width:");
            let w_changed = ui
                .add(
                    egui::DragValue::new(&mut w_in)
                        .speed(0.05)
                        .range(0.5..=max_w_in)
                        .fixed_decimals(2)
                        .suffix(" in"),
                )
                .changed();
            if w_changed {
                h_in = (w_in / nat_aspect).clamp(0.4, 20.0);
            }
            ui.add_space(10.0);
            ui.label("Height:");
            let h_changed = ui
                .add(
                    egui::DragValue::new(&mut h_in)
                        .speed(0.05)
                        .range(0.4..=20.0)
                        .fixed_decimals(2)
                        .suffix(" in"),
                )
                .changed();
            if h_changed {
                w_in = (h_in * nat_aspect).clamp(0.5, max_w_in);
            }
            if ui.small_button("1:1 Ratio").on_hover_text("Reset height to match original aspect ratio").clicked() {
                h_in = (w_in / nat_aspect).clamp(0.4, 20.0);
            }
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Page width preset:").weak().small());
            for (lbl, frac) in [("25%", 0.25_f32), ("50%", 0.50), ("75%", 0.75), ("100%", 1.0)] {
                if ui.small_button(lbl).clicked() {
                    w_in = (max_w_in * frac).clamp(0.5, max_w_in);
                    h_in = (w_in / nat_aspect).clamp(0.4, 20.0);
                }
            }
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Alignment:").strong());
            ui.selectable_value(&mut align, Align::Left, "Left");
            ui.selectable_value(&mut align, Align::Center, "Center");
            ui.selectable_value(&mut align, Align::Right, "Right");
        });

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Replace from file…").clicked() {
                replace_clicked = true;
            }
            if ui.button(RichText::new("Remove image").color(egui::Color32::from_rgb(220, 38, 38))).clicked() {
                delete_clicked = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Done").clicked() {
                    done_clicked = true;
                }
            });
        });
    });

    if (w_in - img_snapshot.width_in).abs() > 0.002
        || (h_in - img_snapshot.height_in).abs() > 0.002
        || caption != img_snapshot.caption
        || align != cur_align
    {
        let t = app.tab();
        t.checkpoint(EditKind::Other, now);
        if let Some(img) = t.doc.image_mut(img_id) {
            img.width_in = w_in;
            img.height_in = h_in;
            img.caption = caption;
        }
        if let Some(pi) = para_idx {
            t.doc.paras[pi].fmt.align = align;
        }
        t.changed();
        t.scroll_to_caret = false;
    }

    if replace_clicked {
        app.act(Action::ReplaceImage(img_id));
    } else if delete_clicked {
        app.tab().remove_image(img_id, now);
        *open = false;
    } else if done_clicked {
        *open = false;
    }
}

pub const TABLE_SHADING_SWATCHES: &[(&str, Option<[u8; 3]>)] = &[
    ("None", None),
    ("Pale Yellow", Some([255, 255, 204])),
    ("Teal Banner", Some([60, 127, 139])),
    ("Gold Banner", Some([193, 134, 38])),
    ("Crimson Banner", Some([191, 47, 19])),
    ("Soft Blue", Some([219, 234, 254])),
    ("Soft Green", Some([220, 252, 231])),
    ("Soft Rose", Some([255, 228, 230])),
    ("Soft Slate", Some([241, 245, 249])),
];

fn insert_table_dialog(
    app: &mut App,
    ctx: &egui::Context,
    open: &mut bool,
    rows: &mut usize,
    cols: &mut usize,
    header_shading: &mut Option<[u8; 3]>,
) {
    let mut insert_clicked = false;
    let mut cancel_clicked = false;
    window("Insert Table").open(open).show(ctx, |ui| {
        ui.set_min_width(380.0);
        ui.label(RichText::new("Dimensions").strong());
        ui.horizontal(|ui| {
            ui.label("Columns:");
            ui.add(egui::DragValue::new(cols).range(1..=12).speed(0.1));
            ui.add_space(14.0);
            ui.label("Rows:");
            ui.add(egui::DragValue::new(rows).range(1..=64).speed(0.2));
        });

        ui.add_space(6.0);
        ui.label(RichText::new("Quick Grid Picker (click a cell to set size):").weak().small());
        let max_preview_r = 6usize;
        let max_preview_c = 8usize;
        egui::Grid::new("tbl-grid-picker").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
            for r in 1..=max_preview_r {
                for c in 1..=max_preview_c {
                    let active = r <= *rows && c <= *cols;
                    let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 18.0), egui::Sense::click());
                    let fill = if active {
                        if r == 1 {
                            header_shading.map_or(app.theme.accent.gamma_multiply(0.35), |rgb| {
                                egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
                            })
                        } else {
                            app.theme.accent_soft
                        }
                    } else {
                        app.theme.surface_subtle
                    };
                    let stroke_col = if active || resp.hovered() { app.theme.accent } else { app.theme.border };
                    ui.painter().rect_filled(rect, 3.0, fill);
                    ui.painter()
                        .rect_stroke(rect, 3.0, egui::Stroke::new(1.0, stroke_col), egui::StrokeKind::Inside);
                    if resp.on_hover_text(format!("{c} × {r} table")).clicked() {
                        *rows = r;
                        *cols = c;
                    }
                }
                ui.end_row();
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        ui.label(RichText::new("Header Row Shading (Optional)").strong());
        ui.horizontal_wrapped(|ui| {
            for &(name, rgb_opt) in TABLE_SHADING_SWATCHES {
                let selected = *header_shading == rgb_opt;
                let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::click());
                let fill = rgb_opt.map_or(egui::Color32::WHITE, |c| egui::Color32::from_rgb(c[0], c[1], c[2]));
                ui.painter().rect_filled(rect.shrink(2.0), 5.0, fill);
                if rgb_opt.is_none() {
                    ui.painter().line_segment(
                        [rect.shrink(5.0).left_bottom(), rect.shrink(5.0).right_top()],
                        egui::Stroke::new(1.3, egui::Color32::from_rgb(220, 38, 38)),
                    );
                }
                let st = if selected {
                    egui::Stroke::new(2.0, app.theme.accent)
                } else {
                    egui::Stroke::new(1.0, app.theme.border)
                };
                ui.painter().rect_stroke(rect.shrink(2.0), 5.0, st, egui::StrokeKind::Inside);
                if resp.on_hover_text(name).clicked() {
                    *header_shading = rgb_opt;
                }
            }
        });

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new(format!("  Insert {} × {} Table  ", *cols, *rows)).strong())
                .clicked()
            {
                insert_clicked = true;
            }
            if ui.button("Cancel").clicked() {
                cancel_clicked = true;
            }
        });
    });

    if insert_clicked {
        let r = *rows;
        let c = *cols;
        let shd = *header_shading;
        *open = false;
        app.act(Action::InsertTable {
            rows: r,
            cols: c,
            header_shading: shd,
        });
    } else if cancel_clicked {
        *open = false;
    }
}

/// Updates the Table Cell Math target cell while preventing self-references and
/// keeping `ABOVE`/`LEFT` sensible when moving to row 0 or column 0.
pub(crate) fn set_table_math_target(
    new_row: usize,
    new_col: usize,
    target_row: &mut usize,
    target_col: &mut usize,
    use_above: &mut bool,
    use_left: &mut bool,
    chosen: &mut Vec<(usize, usize)>,
    picking_target: &mut bool,
) {
    *target_row = new_row;
    *target_col = new_col;
    chosen.retain(|&rc| rc != (new_row, new_col));
    if *use_above && new_row == 0 {
        *use_above = false;
        *use_left = new_col > 0;
    } else if *use_left && new_col == 0 {
        *use_left = false;
        *use_above = new_row > 0;
    }
    *picking_target = false;
}

#[expect(clippy::too_many_arguments, reason = "Table math dialog binds target cell, operation, range mode, chosen cells, and sig-fig option")]
fn table_math_dialog(
    app: &mut App,
    ctx: &egui::Context,
    open: &mut bool,
    table_id: u32,
    target_row: &mut usize,
    target_col: &mut usize,
    op: &mut TableMathOp,
    use_above: &mut bool,
    use_left: &mut bool,
    chosen: &mut Vec<(usize, usize)>,
    keep_sig_figs: &mut bool,
    expr: &mut Option<String>,
    picking_target: &mut bool,
) {
    let Some(tbl_snap) = app.tab_ref().doc.table(table_id).cloned() else {
        *open = false;
        return;
    };
    if tbl_snap.rows.is_empty() {
        *open = false;
        return;
    }
    *target_row = (*target_row).min(tbl_snap.rows.len() - 1);
    let max_col = tbl_snap.rows[*target_row].cells.len().saturating_sub(1);
    *target_col = (*target_col).min(max_col);

    let target_label = cell_coord_label(*target_row, *target_col);
    let mut apply_formula: Option<Option<String>> = None;
    let mut cancel_clicked = false;

    let args_part = if *use_above {
        "ABOVE".to_string()
    } else if *use_left {
        "LEFT".to_string()
    } else {
        chosen
            .iter()
            .map(|&(r, c)| cell_coord_label(r, c))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let suffix = if *keep_sig_figs { "" } else { ";RAW" };
    let built_formula = if let Some(e) = expr.as_ref() {
        let e = e.trim().trim_start_matches('=').trim();
        if e.is_empty() {
            String::new()
        } else {
            format!("={e}")
        }
    } else if args_part.is_empty() {
        String::new()
    } else {
        format!("={}({}{})", op.name(), args_part, suffix)
    };
    let preview_val = if built_formula.is_empty() {
        "—".to_string()
    } else {
        app.tab_ref()
            .doc
            .preview_table_formula(table_id, *target_row, *target_col, &built_formula)
            .unwrap_or_else(|| {
                if expr.is_some() {
                    "(incomplete formula)".to_string()
                } else {
                    "0".to_string()
                }
            })
    };
    let formula_ok = !built_formula.is_empty()
        && (expr.is_none()
            || app
                .tab_ref()
                .doc
                .preview_table_formula(table_id, *target_row, *target_col, &built_formula)
                .is_some());

    window(&format!("Table Cell Math — Cell {target_label}"))
        .open(open)
        .show(ctx, |ui| {
            ui.set_min_width(440.0);
            ui.label(
                RichText::new("Calculate Sum (Total), Subtotal, Average or Median, or add, subtract, multiply and divide across cells.")
                    .weak()
                    .small(),
            );
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Target Cell:").strong());
                egui::ComboBox::from_id_salt("tbl-math-target")
                    .selected_text(format!("🎯 {target_label}"))
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for (r_idx, row_spec) in tbl_snap.rows.iter().enumerate() {
                            for (c_idx, _cell_spec) in row_spec.cells.iter().enumerate() {
                                let coord = cell_coord_label(r_idx, c_idx);
                                let raw_txt = app.tab_ref().doc.cell_text(table_id, r_idx, c_idx);
                                let short_txt: String = raw_txt.trim().chars().take(12).collect();
                                let item_lbl = if short_txt.is_empty() {
                                    coord
                                } else {
                                    format!("{coord}: {short_txt}")
                                };
                                if ui
                                    .selectable_label((r_idx, c_idx) == (*target_row, *target_col), item_lbl)
                                    .clicked()
                                {
                                    set_table_math_target(
                                        r_idx,
                                        c_idx,
                                        target_row,
                                        target_col,
                                        use_above,
                                        use_left,
                                        chosen,
                                        picking_target,
                                    );
                                }
                            }
                        }
                    });
                if ui
                    .selectable_label(*picking_target, "🎯 Pick Target in Grid")
                    .on_hover_text("Click any cell in the grid below to move the target there (or right-click any cell anytime)")
                    .clicked()
                {
                    *picking_target = !*picking_target;
                }
            });
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Calculation:").strong());
                for candidate in [
                    TableMathOp::Sum,
                    TableMathOp::Subtotal,
                    TableMathOp::Average,
                    TableMathOp::Median,
                ] {
                    if ui.selectable_label(expr.is_none() && *op == candidate, candidate.label()).clicked() {
                        *op = candidate;
                        *expr = None;
                    }
                }
                if ui
                    .selectable_label(expr.is_some(), "Arithmetic (+ − × ÷)")
                    .on_hover_text("Build a formula such as B2*C2, (D2+D3)/B2 or SUM(ABOVE)*1.07")
                    .clicked()
                    && expr.is_none()
                {
                    *expr = Some(String::new());
                }
            });

            if let Some(e) = expr.as_mut() {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Formula:").strong());
                    ui.label(RichText::new("=").monospace());
                    ui.add(egui::TextEdit::singleline(e).font(egui::TextStyle::Monospace).desired_width(260.0).hint_text("B2*C2"));
                });
                ui.horizontal(|ui| {
                    for (label, ins) in [("+", "+"), ("−", "-"), ("×", "*"), ("÷", "/"), ("(", "("), (")", ")"), ("SUM(ABOVE)", "SUM(ABOVE)")] {
                        if ui.button(RichText::new(label).monospace()).clicked() {
                            e.push_str(ins);
                        }
                    }
                    if ui.button("⌫").on_hover_text("Delete the last character").clicked() {
                        e.pop();
                    }
                });
                ui.label(RichText::new("Click cells below to insert their references (A1 is the top-left cell). Empty cells count as 0.").weak().small());
            }

            ui.add_space(6.0);
            if expr.is_none() { ui.horizontal(|ui| {
                ui.label(RichText::new("Input Cells:").strong());
                if ui
                    .selectable_label(*use_above, "Column Above (ABOVE)")
                    .on_hover_text("Use numeric cells above this cell in the same column")
                    .clicked()
                {
                    *use_above = true;
                    *use_left = false;
                }
                if ui
                    .selectable_label(*use_left, "Row to Left (LEFT)")
                    .on_hover_text("Use numeric cells to the left of this cell in the same row")
                    .clicked()
                {
                    *use_left = true;
                    *use_above = false;
                }
                if ui
                    .selectable_label(!*use_above && !*use_left, "Chosen Cells (Pick Below)")
                    .on_hover_text("Click any combination of cells in the grid below")
                    .clicked()
                {
                    if *use_above {
                        chosen.clear();
                        for r in 0..*target_row {
                            chosen.push((r, *target_col));
                        }
                    } else if *use_left {
                        chosen.clear();
                        for c in 0..*target_col {
                            chosen.push((*target_row, c));
                        }
                    }
                    *use_above = false;
                    *use_left = false;
                }
            }); }

            ui.add_space(4.0);
            if *picking_target {
                ui.label(
                    RichText::new("Click any cell in the table preview below to set it as the Target Cell:")
                        .color(app.theme.accent)
                        .strong()
                        .small(),
                );
            } else {
                ui.label(
                    RichText::new("Click cells below to include/exclude them • Right-click any cell to set it as the Target Cell:")
                        .weak()
                        .small(),
                );
            }
            egui::ScrollArea::both()
                .id_salt("tbl-math-cell-picker")
                .max_height(180.0)
                .show(ui, |ui| {
                    egui::Grid::new("tbl-math-grid").spacing(vec2(6.0, 4.0)).show(ui, |ui| {
                        for (r_idx, row_spec) in tbl_snap.rows.iter().enumerate() {
                            for (c_idx, _cell_spec) in row_spec.cells.iter().enumerate() {
                                let coord = cell_coord_label(r_idx, c_idx);
                                let raw_txt = app.tab_ref().doc.cell_text(table_id, r_idx, c_idx);
                                let short_txt: String = raw_txt.trim().chars().take(12).collect();
                                let is_target = (r_idx, c_idx) == (*target_row, *target_col);
                                let display = if is_target {
                                    format!("🎯 {coord} (Target)")
                                } else if short_txt.is_empty() {
                                    format!("{coord}: —")
                                } else {
                                    format!("{coord}: {short_txt}")
                                };
                                let is_selected = if *picking_target {
                                    is_target
                                } else if is_target {
                                    false
                                } else if let Some(e) = expr.as_ref() {
                                    e.to_ascii_uppercase().contains(&coord)
                                } else if *use_above {
                                    c_idx == *target_col && r_idx < *target_row
                                } else if *use_left {
                                    r_idx == *target_row && c_idx < *target_col
                                } else {
                                    chosen.contains(&(r_idx, c_idx))
                                };
                                let tip = if *picking_target {
                                    "Click to set as Target Cell"
                                } else if is_target {
                                    "Current Target Cell (result will be written here)"
                                } else {
                                    "Click to include/exclude • Right-click to set as Target Cell"
                                };
                                let btn = ui
                                    .add_enabled(*picking_target || !is_target, egui::Button::new(&display).selected(is_selected))
                                    .on_hover_text(tip);
                                if (*picking_target && btn.clicked()) || btn.secondary_clicked() {
                                    set_table_math_target(
                                        r_idx,
                                        c_idx,
                                        target_row,
                                        target_col,
                                        use_above,
                                        use_left,
                                        chosen,
                                        picking_target,
                                    );
                                } else if btn.clicked() && expr.is_some() {
                                    if let Some(e) = expr.as_mut() {
                                        // two references in a row need an operator between them
                                        if e.trim_end().chars().last().is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == ')') {
                                            e.push('+');
                                        }
                                        e.push_str(&coord);
                                    }
                                } else if btn.clicked() {
                                    if *use_above {
                                        chosen.clear();
                                        for rr in 0..*target_row {
                                            chosen.push((rr, *target_col));
                                        }
                                        *use_above = false;
                                    } else if *use_left {
                                        chosen.clear();
                                        for cc in 0..*target_col {
                                            chosen.push((*target_row, cc));
                                        }
                                        *use_left = false;
                                    }
                                    if let Some(pos) = chosen.iter().position(|&x| x == (r_idx, c_idx)) {
                                        chosen.remove(pos);
                                    } else {
                                        chosen.push((r_idx, c_idx));
                                        chosen.sort();
                                    }
                                }
                            }
                            ui.end_row();
                        }
                    });
                });

            ui.add_space(6.0);
            if expr.is_none() {
                ui.checkbox(
                    keep_sig_figs,
                    "Keep maximum significant figures / precision from input cells (e.g. 10 + 5.23 = 15.23)",
                );
            }

            ui.add_space(6.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Formula:").weak());
                ui.label(RichText::new(if built_formula.is_empty() { "(select cells)" } else { &built_formula }).monospace());
                ui.add_space(16.0);
                ui.label(RichText::new("Result Preview:").weak());
                ui.label(RichText::new(&preview_val).strong().size(15.0).color(app.theme.accent));
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(formula_ok, egui::Button::new(RichText::new("  Apply Calculation  ").strong()))
                    .clicked()
                {
                    apply_formula = Some(Some(built_formula.clone()));
                }
                if ui.button("Clear Formula").clicked() {
                    apply_formula = Some(None);
                }
                if ui.button("Cancel").clicked() {
                    cancel_clicked = true;
                }
            });
        });

    if let Some(f_opt) = apply_formula {
        let t = app.tab();
        t.checkpoint(EditKind::Other, ctx.input(|i| i.time));
        t.doc.table_set_cell_formula(table_id, *target_row, *target_col, f_opt);
        if let Some(pi) = t.doc.paras.iter().position(|p| {
            p.fmt.table_id == table_id
                && p.fmt.table_row as usize == *target_row
                && p.fmt.table_col as usize == *target_col
        }) {
            let end_c = t.doc.paras[pi].len();
            t.set_caret(Pos::new(pi, end_c), false);
        }
        t.changed();
        *open = false;
    } else if cancel_clicked {
        *open = false;
    }
}

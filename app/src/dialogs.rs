//! Dialog boxes: Find & Replace, Word Count, Page setup (Pages & Pageless), Font, Paragraph,
//! Go To, Date/Time, Special Characters & Emoji, Preferences, Personal Dictionary / Substitutions,
//! Document details, Version history, Search the menus (`Alt+/`), `@` Smart Chips & Building Blocks,
//! Insert Link, Add Comment, Bookmarks, Citations, Dictionary, Rename, Keyboard Shortcuts, About.

use eframe::egui::{self, vec2, RichText};
use std::path::PathBuf;

use crate::app::{gradient_swatch, swatch, text_gradient_swatch, Action, App, CRAFT_GRADIENT_HIGHLIGHTS, CRAFT_GRADIENT_TEXT, HIGHLIGHTS, PALETTE};
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
    VersionHistory(usize, String),
    Citations(CitationEntry),
    Dictionary,
    Rename(String),
    DesignSheet,
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
            let mut cfg = app.cfg.clone();
            window("Preferences").open(&mut open).show(ctx, |ui| {
                ui.set_min_width(380.0);
                ui.heading("General");
                ui.checkbox(&mut cfg.smart_quotes, "Use smart quotes (“ ”)");
                ui.checkbox(&mut cfg.auto_links, "Automatically detect links");
                ui.checkbox(&mut cfg.markdown_auto, "Automatically detect Markdown (# headings, * lists, [] checklists, ---)");
                ui.checkbox(&mut cfg.smartcorrect, "Automatically correct spelling and substitutions");
                ui.checkbox(&mut cfg.live_word_count, "Display word count while typing");
                ui.add_space(6.0);
                ui.heading("Shortcut keys");
                ui.horizontal(|ui| {
                    ui.label("Preset:");
                    for p in ShortcutPreset::ALL {
                        ui.selectable_value(&mut cfg.shortcut_preset, p, p.label());
                    }
                });
                ui.add_space(6.0);
                ui.heading("View & Appearance");
                ui.checkbox(&mut cfg.dark, "Dark theme");
                ui.checkbox(&mut cfg.margin_guides, "Show margin guides on the page");
                ui.horizontal(|ui| {
                    ui.label("Cursor:");
                    ui.selectable_value(&mut cfg.block_cursor, false, "Line cursor");
                    ui.selectable_value(&mut cfg.block_cursor, true, "Block cursor");
                });
                ui.add_space(6.0);
                ui.heading("Timed save");
                ui.horizontal(|ui| {
                    ui.label("Keep a recovery copy every");
                    ui.add(egui::DragValue::new(&mut cfg.autosave_min).range(0..=120));
                    ui.label("minutes (0 = off)");
                });
                ui.label(RichText::new("Recovery copies stay on this computer only.").weak());
                ui.add_space(6.0);
                ui.heading("Privacy");
                ui.label("Scribe Pro works 100% offline: it never connects to the internet.");
            });
            if cfg.dark != app.cfg.dark {
                app.act(Action::ToggleDark);
                cfg.dark = app.cfg.dark;
            }
            let changed = cfg.margin_guides != app.cfg.margin_guides
                || cfg.block_cursor != app.cfg.block_cursor
                || cfg.smartcorrect != app.cfg.smartcorrect
                || cfg.smart_quotes != app.cfg.smart_quotes
                || cfg.markdown_auto != app.cfg.markdown_auto
                || cfg.auto_links != app.cfg.auto_links
                || cfg.live_word_count != app.cfg.live_word_count
                || cfg.autosave_min != app.cfg.autosave_min
                || cfg.shortcut_preset != app.cfg.shortcut_preset;
            if changed {
                app.cfg.margin_guides = cfg.margin_guides;
                app.cfg.block_cursor = cfg.block_cursor;
                app.cfg.smartcorrect = cfg.smartcorrect;
                app.cfg.smart_quotes = cfg.smart_quotes;
                app.cfg.markdown_auto = cfg.markdown_auto;
                app.cfg.auto_links = cfg.auto_links;
                app.cfg.live_word_count = cfg.live_word_count;
                app.cfg.autosave_min = cfg.autosave_min;
                app.cfg.shortcut_preset = cfg.shortcut_preset;
                app.save_config();
                ctx.memory_mut(|m| m.request_focus(app.doc_id));
            }
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
            window("About Scribe Pro").open(&mut open).show(ctx, |ui| {
                ui.heading("Scribe Pro — Google Docs Edition");
                ui.label("A fast, offline desktop word processor featuring the Google Docs menu structure, Material Design 3 pill toolbar, and modern productivity tools.");
                ui.add_space(6.0);
                ui.label("• Search the menus (Alt+/) & @ Smart Chips / Building Blocks");
                ui.label("• Checklists, Markdown auto-formatting, Pageless mode & Version History");
                ui.label("• Comments, Suggesting mode, Links, Bookmarks, Citations & Live Word Count");
                ui.label("• Works 100% offline on this computer — no cloud account or internet connection needed.");
            });
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
        Dialog::VersionHistory(mut sel_idx, mut ver_name) => {
            version_history_dialog(app, ctx, &mut open, &mut sel_idx, &mut ver_name, now);
            if open && matches!(app.dialog, Dialog::VersionHistory(_, _)) {
                app.dialog = Dialog::VersionHistory(sel_idx, ver_name);
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
    }
    if !open {
        app.dialog = Dialog::None;
        ctx.memory_mut(|m| m.request_focus(app.doc_id));
    }
}

fn search_menus_dialog(app: &mut App, ctx: &egui::Context, open: &mut bool, query: &mut String) {
    let kp = app.cfg.shortcut_preset;
    let items: &[(Icon, &str, &str, Action)] = &[
        (Icon::New, "File > New document", "Ctrl+N", Action::New),
        (Icon::CopyDoc, "File > Make a copy", "", Action::MakeCopy),
        (Icon::Save, "File > Save", "Ctrl+S", Action::Save),
        (Icon::Save, "File > Save As", kp.save_as(), Action::SaveAs),
        (Icon::Pdf, "File > Download as PDF (.pdf)", "Ctrl+P", Action::ExportPdf),
        (Icon::Save, "File > Download as Google Docs (.gdoc)", "", Action::ExportGdoc),
        (Icon::Save, "File > Download as Microsoft Word (.docx)", "", Action::ExportDocx),
        (Icon::Save, "File > Download as Markdown (.md)", "", Action::ExportMd),
        (Icon::Save, "File > Download as Web Page (.html)", "", Action::ExportHtml),
        (Icon::History, "File > Version history", "Ctrl+Alt+Shift+H", Action::Dialog(Dialog::VersionHistory(0, String::new()))),
        (Icon::Pageless, "File > Page setup (Pages / Pageless)", "", Action::Dialog(Dialog::PageSetup)),
        (Icon::PageSetup, "File > Document details", "", Action::Dialog(Dialog::DocProps)),
        (Icon::Undo, "Edit > Undo", "Ctrl+Z", Action::Undo),
        (Icon::Redo, "Edit > Redo", "Ctrl+Y", Action::Redo),
        (Icon::Paste, "Edit > Paste without formatting", kp.paste_plain(), Action::PastePlain),
        (Icon::Find, "Edit > Find and replace", kp.replace(), Action::Dialog(Dialog::Find(true))),
        (Icon::EditMode, "View > Mode: Editing", "", Action::SetMode(DocMode::Editing)),
        (Icon::SuggestMode, "View > Mode: Suggesting", "", Action::SetMode(DocMode::Suggesting)),
        (Icon::ViewMode, "View > Mode: Viewing", "", Action::SetMode(DocMode::Viewing)),
        (Icon::NavPane, "View > Show document outline", "", Action::ToggleNav),
        (Icon::Comment, "View > Show comments & suggestions", "", Action::ToggleComments),
        (Icon::Pageless, "View > Toggle Pageless mode", "", Action::TogglePageless),
        (Icon::ChevronUp, "View > Compact mode (hide menus)", "Ctrl+Shift+F", Action::ToggleCompact),
        (Icon::Table, "Insert > Table (2×2)", "", Action::InsertBuildingBlock("table_2x2")),
        (Icon::Table, "Insert > Table (3×3)", "", Action::InsertBuildingBlock("table_3x3")),
        (Icon::BuildingBlock, "Insert > Building block: Meeting notes", "", Action::InsertBuildingBlock("meeting")),
        (Icon::BuildingBlock, "Insert > Building block: Email draft", "", Action::InsertBuildingBlock("email")),
        (Icon::BuildingBlock, "Insert > Building block: Project roadmap", "", Action::InsertBuildingBlock("roadmap")),
        (Icon::BuildingBlock, "Insert > Building block: Launch checklist", "", Action::InsertBuildingBlock("checklist")),
        (Icon::BuildingBlock, "Insert > Building block: Code block", "", Action::InsertBuildingBlock("code")),
        (Icon::SmartChip, "Insert > Smart chip: Today's date (@today)", "", Action::InsertSmartChip("today")),
        (Icon::SmartChip, "Insert > Smart chip: Status — In progress", "", Action::InsertSmartChip("status_in_progress")),
        (Icon::SmartChip, "Insert > Smart chip: Status — Approved", "", Action::InsertSmartChip("status_approved")),
        (Icon::HorizontalRule, "Insert > Horizontal line", "", Action::HorizontalRule),
        (Icon::Footnote, "Insert > Footnote", "Ctrl+Alt+F", Action::InsertFootnote),
        (Icon::Symbol, "Insert > Emoji (System Library)", "", Action::SystemEmojiPicker),
        (Icon::Symbol, "Insert > Special characters & emoji", "", Action::Dialog(Dialog::Symbol)),
        (Icon::PageBreak, "Insert > Page break", "Ctrl+Enter", Action::PageBreak),
        (Icon::Link, "Insert > Link", "Ctrl+K", Action::Dialog(Dialog::Link { text: String::new(), url: String::new() })),
        (Icon::Comment, "Insert > Comment", "Ctrl+Alt+M", Action::Dialog(Dialog::Comment(String::new()))),
        (Icon::Bookmark, "Insert > Bookmark", "", Action::Dialog(Dialog::Bookmark(String::new()))),
        (Icon::NavPane, "Insert > Table of contents", "", Action::InsertToc),
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
        (Icon::Checklist, "Format > Bullets & numbering > Checklist", kp.checklist(), Action::List(ListKind::Checklist)),
        (Icon::Bullets, "Format > Bullets & numbering > Bulleted list", kp.bullet_list(), Action::List(ListKind::Bullet)),
        (Icon::Numbers, "Format > Bullets & numbering > Numbered list", kp.number_list(), Action::List(ListKind::Number)),
        (Icon::Clear, "Format > Clear formatting", kp.clear_format(), Action::ClearFormat),
        (Icon::Spell, "Tools > Spelling and grammar check", kp.spellcheck(), Action::Dialog(Dialog::Spell(0))),
        (Icon::WordCount, "Tools > Word count", kp.word_count(), Action::Dialog(Dialog::WordCount)),
        (Icon::Citation, "Tools > Citations & Bibliography", "", Action::Dialog(Dialog::Citations(CitationEntry::default()))),
        (Icon::Dictionary, "Tools > Dictionary / Define word", kp.dictionary(), Action::Dialog(Dialog::Dictionary)),
        (Icon::PageSetup, "Tools > Preferences", "", Action::Dialog(Dialog::Prefs)),
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
            for (ic, label, sc, act) in filtered.into_iter().take(25) {
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

fn version_history_dialog(app: &mut App, ctx: &egui::Context, open: &mut bool, sel_idx: &mut usize, ver_name: &mut String, now: f64) {
    let mut save_ver = false;
    let mut restore_idx: Option<usize> = None;
    let versions = app.tab_ref().versions.clone();
    window("Version history").open(open).show(ctx, |ui| {
        ui.set_min_width(520.0);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(ver_name).desired_width(240.0).hint_text("Name current version (e.g. Draft 1)"));
            if ui.button("Save named version").clicked() {
                save_ver = true;
            }
        });
        ui.separator();
        if versions.is_empty() {
            ui.label(RichText::new("No named versions saved yet. Click 'Save named version' above or save the document (Ctrl+S) to record a snapshot.").weak());
        } else {
            let idx = (*sel_idx).min(versions.len() - 1);
            *sel_idx = idx;
            ui.columns(2, |cols| {
                cols[0].label(RichText::new("Saved versions").strong());
                egui::ScrollArea::vertical().id_salt("vh_list").max_height(260.0).show(&mut cols[0], |ui| {
                    for (i, v) in versions.iter().enumerate().rev() {
                        let lbl = format!("{} — {} ({} words)", v.name, v.timestamp, v.words);
                        if ui.selectable_label(i == *sel_idx, lbl).clicked() {
                            *sel_idx = i;
                        }
                    }
                });
                let v = &versions[*sel_idx];
                cols[1].label(RichText::new(format!("Preview: {} ({})", v.name, v.timestamp)).strong());
                let preview = from_json(&v.snapshot_json).map(|d| d.plain_text()).unwrap_or_default();
                egui::ScrollArea::vertical().id_salt("vh_prev").max_height(220.0).show(&mut cols[1], |ui| {
                    ui.label(if preview.is_empty() { "(Empty document)" } else { &preview });
                });
                cols[1].add_space(6.0);
                if cols[1].button("Restore this version").clicked() {
                    restore_idx = Some(*sel_idx);
                }
            });
        }
    });
    if save_ver {
        let name = if ver_name.trim().is_empty() {
            format!("Version {}", app.tab_ref().versions.len() + 1)
        } else {
            ver_name.trim().to_string()
        };
        ver_name.clear();
        let t = app.tab();
        let (_, words) = t.doc.counts(None);
        let snap = to_json(&t.doc);
        let ts = chrono::Local::now().format("%b %-d, %-I:%M %p").to_string();
        t.versions.push(DocVersion { name: name.clone(), timestamp: ts, words, snapshot_json: snap });
        *sel_idx = t.versions.len().saturating_sub(1);
        app.toast(format!("Saved version “{name}”"));
    }
    if let Some(i) = restore_idx {
        if let Some(v) = versions.get(i) {
            if let Ok(restored) = from_json(&v.snapshot_json) {
                let t = app.tab();
                t.checkpoint(EditKind::Other, now);
                t.doc = restored;
                t.set_caret(Pos::new(0, 0), false);
                t.changed();
                app.toast(format!("Restored version “{}”", v.name));
                *open = false;
            }
        }
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

fn confirmations(app: &mut App, ctx: &egui::Context) {
    if let Some(idx) = app.confirm_close {
        let name = app.tabs.get(idx).map(|t| t.name.clone()).unwrap_or_default();
        let mut choice = 0;
        egui::Modal::new(egui::Id::unique("confirm-close")).show(ctx, |ui| {
            ui.set_max_width(380.0);
            ui.heading("Save changes?");
            ui.label(format!("\"{name}\" has changes that haven't been saved."));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = 1;
                }
                if ui.button("Don't save").clicked() {
                    choice = 2;
                }
                if ui.button("Cancel").clicked() {
                    choice = 3;
                }
            });
        });
        match choice {
            1 => {
                app.confirm_close = None;
                app.save_then_close(idx);
            }
            2 => {
                app.confirm_close = None;
                app.close_tab(idx, true);
            }
            3 => app.confirm_close = None,
            _ => {}
        }
    }
    if app.confirm_quit {
        let n = app.tabs.iter().filter(|t| t.dirty).count();
        let mut choice = 0;
        egui::Modal::new(egui::Id::unique("confirm-quit")).show(ctx, |ui| {
            ui.set_max_width(400.0);
            ui.heading("Unsaved documents");
            ui.label(format!("{n} document(s) have changes that haven't been saved."));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Save all").clicked() {
                    choice = 1;
                }
                if ui.button("Quit without saving").clicked() {
                    choice = 2;
                }
                if ui.button("Cancel").clicked() {
                    choice = 3;
                }
            });
        });
        match choice {
            1 => {
                app.confirm_quit = false;
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
            2 => {
                app.confirm_quit = false;
                app.force_quit(ctx);
            }
            3 => app.confirm_quit = false,
            _ => {}
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
            ui.label("Add one in Windows Settings › Time & language › Language.");
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
                ui.label("Footer");
                ui.add(egui::TextEdit::singleline(&mut footer).desired_width(260.0).hint_text("Text at the bottom of every page"));
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
    if pg != t.doc.page || pageless != t.doc.pageless || header != t.doc.header || footer != t.doc.footer {
        t.checkpoint(EditKind::Typing, now);
        t.doc.page = pg;
        t.doc.pageless = pageless;
        t.doc.header = header;
        t.doc.footer = footer;
        t.changed();
        t.scroll_to_caret = false;
    }
}

fn design_sheet_dialog(ctx: &egui::Context, open: &mut bool) {
    let light = crate::theme::Theme::light();
    let dark = crate::theme::Theme::dark();
    egui::Window::new("Scribe Pro — Studio UI & Duotone Icon Design Sheet (Light & Dark Themes)")
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

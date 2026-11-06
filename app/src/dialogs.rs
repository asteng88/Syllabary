//! Dialog boxes: Find & Replace, Word Count, Page setup, Font, Paragraph, Go To,
//! Date/Time, Symbol, Preferences, SmartCorrect, Document properties, About.

use eframe::egui::{self, vec2, RichText};
use std::path::PathBuf;

use crate::app::{swatch, Action, App, HIGHLIGHTS, PALETTE};
use crate::doc::*;
use crate::editor::EditKind;

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
}

pub struct FindState {
    pub query: String,
    pub replace: String,
    pub mode: FindMode,
    pub case: bool,
    pub focus: bool,
    pub show_replace: bool,
    cache: Option<(u64, String, FindMode, bool, Vec<(Pos, Pos)>)>,
}

impl Default for FindState {
    fn default() -> Self {
        Self { query: String::new(), replace: String::new(), mode: FindMode::Containing, case: false, focus: false, show_replace: true, cache: None }
    }
}

impl FindState {
    pub fn matches(&mut self, doc: &Document, rev: u64) -> Vec<(Pos, Pos)> {
        if let Some((r, q, m, c, v)) = &self.cache {
            if *r == rev && *q == self.query && *m == self.mode && *c == self.case {
                return v.clone();
            }
        }
        let v = find_all(doc, &self.query, self.mode, self.case);
        self.cache = Some((rev, self.query.clone(), self.mode, self.case, v.clone()));
        v
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
        Dialog::Find(_) => find_dialog(app, ctx, &mut open),
        Dialog::Spell(_) => spell_dialog(app, ctx, &mut open, now),
        Dialog::WordCount => {
            let t = app.tab_ref();
            let (chars, words) = t.doc.counts(None);
            let sel = if t.has_selection() { Some(t.doc.counts(Some(t.selection()))) } else { None };
            let paras = t.doc.paras.iter().filter(|p| !p.text.is_empty()).count();
            let pages = t.cached_pages();
            let text = t.doc.plain_text();
            let sentences = text.split(|c| c == '.' || c == '!' || c == '?').filter(|s| s.chars().any(|c| c.is_alphanumeric())).count();
            window("Word Count").open(&mut open).show(ctx, |ui| {
                ui.set_min_width(340.0);
                // wording from the original's strings 15723 / 15724
                ui.label(format!("The document contains {chars} characters and {words} words."));
                if let Some((c, w)) = sel {
                    ui.label(format!("The selected text contains {c} characters and {w} words."));
                }
                ui.add_space(6.0);
                egui::Grid::new("wc").num_columns(2).spacing(vec2(24.0, 4.0)).show(ui, |ui| {
                    ui.label("Pages");
                    ui.label(pages.to_string());
                    ui.end_row();
                    ui.label("Paragraphs");
                    ui.label(paras.to_string());
                    ui.end_row();
                    ui.label("Sentences");
                    ui.label(sentences.to_string());
                    ui.end_row();
                    ui.label("Average words per sentence");
                    ui.label(if sentences > 0 { format!("{:.1}", words as f32 / sentences as f32) } else { "–".into() });
                    ui.end_row();
                    ui.label("Reading time");
                    ui.label(format!("about {} min", ((words as f32) / 230.0).ceil().max(if words > 0 { 1.0 } else { 0.0 })));
                    ui.end_row();
                });
            });
        }
        Dialog::PageSetup => page_setup(app, ctx, &mut open, now),
        Dialog::Paragraph => {
            let t = app.tab_ref();
            let mut f = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
            let orig = f;
            window("Paragraph Spacing").open(&mut open).show(ctx, |ui| {
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
            window("Go To").open(&mut open).show(ctx, |ui| {
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
            window("Insert Date/Time").open(&mut open).show(ctx, |ui| {
                ui.label(RichText::new("Click a format to insert it").weak());
                for f in formats {
                    if ui.add(egui::Button::new(&f).min_size(vec2(260.0, 0.0))).clicked() {
                        app.act(Action::InsertText(f.clone()));
                    }
                }
            });
        }
        Dialog::Symbol => {
            window("Insert Symbol").open(&mut open).show(ctx, |ui| {
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
                ui.label("Text colour");
                ui.horizontal_wrapped(|ui| {
                    for c in PALETTE {
                        if swatch(ui, *c, st.color == *c).clicked() {
                            app.act(Action::Color(*c));
                        }
                    }
                });
                ui.label("Highlight");
                ui.horizontal(|ui| {
                    for c in HIGHLIGHTS {
                        if swatch(ui, *c, st.highlight == Some(*c)).clicked() {
                            app.act(Action::Highlight(Some(*c)));
                        }
                    }
                    if ui.small_button("None").clicked() {
                        app.act(Action::Highlight(None));
                    }
                });
                ui.separator();
                let (fam, _) = book.family(st.font, st.bold, st.italic);
                let mut rt = RichText::new("The quick brown fox jumps over the lazy dog").font(egui::FontId::new(st.size.clamp(8.0, 36.0) * 1.333, fam)).color(crate::layout::color(st.color));
                if st.underline {
                    rt = rt.underline();
                }
                if st.strike {
                    rt = rt.strikethrough();
                }
                egui::Frame::new().fill(egui::Color32::WHITE).corner_radius(6).inner_margin(10).show(ui, |ui| {
                    ui.set_min_width(420.0);
                    ui.label(rt);
                });
            });
        }
        Dialog::Prefs => {
            let mut cfg = app.cfg.clone();
            window("Preferences").open(&mut open).show(ctx, |ui| {
                ui.heading("General");
                ui.checkbox(&mut cfg.dark, "Dark theme");
                ui.checkbox(&mut cfg.margin_guides, "Show margin guides on the page");
                ui.add_space(6.0);
                ui.heading("Typing");
                ui.checkbox(&mut cfg.smartcorrect, "SmartCorrect common misspellings as you type");
                ui.checkbox(&mut cfg.smart_quotes, "Curly “smart” quotes");
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
                ui.label("This app works completely offline: it never connects to the internet,\nhas no scanner (TWAIN) support and no sharing features.");
            });
            if cfg.dark != app.cfg.dark {
                app.act(Action::ToggleDark);
                cfg.dark = app.cfg.dark;
            }
            let changed = cfg.margin_guides != app.cfg.margin_guides || cfg.smartcorrect != app.cfg.smartcorrect || cfg.smart_quotes != app.cfg.smart_quotes || cfg.autosave_min != app.cfg.autosave_min;
            if changed {
                app.cfg.margin_guides = cfg.margin_guides;
                app.cfg.smartcorrect = cfg.smartcorrect;
                app.cfg.smart_quotes = cfg.smart_quotes;
                app.cfg.autosave_min = cfg.autosave_min;
                app.save_config();
            }
        }
        Dialog::SmartCorrect(mut from, mut to) => {
            let mut remove: Option<String> = None;
            let mut add = false;
            let table = app.sc_table.clone();
            window("SmartCorrect Setup").open(&mut open).show(ctx, |ui| {
                ui.label("Words on the left are replaced by the right as soon as you type a space or punctuation.");
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
            let (chars, words) = t.doc.counts(None);
            let name = t.name.clone();
            let path = t.path.clone();
            window("Document Properties").open(&mut open).show(ctx, |ui| {
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
                    ui.label("Notes");
                    ui.add(egui::TextEdit::multiline(&mut info.notes).desired_width(300.0).desired_rows(3));
                    ui.end_row();
                    ui.label("File");
                    ui.label(path.map(|p| p.display().to_string()).unwrap_or(name));
                    ui.end_row();
                    ui.label("Statistics");
                    ui.label(format!("{words} words, {chars} characters"));
                    ui.end_row();
                });
            });
            let t = app.tab();
            if info != t.doc.info {
                t.doc.info = info;
                t.dirty = true;
            }
        }
        Dialog::Recover(files) => {
            let mut choice = None;
            egui::Modal::new(egui::Id::unique("recover")).show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.heading("Recover documents");
                // the original's message 15508, in plain words
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
                ui.heading("Scribe Pro 1.0");
                ui.label("A sleek, modern word processor modelled on a classic of the 1990s.");
                ui.add_space(4.0);
                ui.label("• Works entirely on this computer — no internet connection, ever.");
                ui.label("• No scanner (TWAIN), web or team-sharing features.");
                ui.label("• Documents are saved as .wpx files; export to Rich Text or plain text.");
            });
        }
        Dialog::Shortcuts => {
            let rows = [
                ("Ctrl+N / Ctrl+O / Ctrl+S", "New, open, save"),
                ("Ctrl+W", "Close document tab"),
                ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
                ("Ctrl+X / C / V", "Cut / copy / paste"),
                ("Ctrl+B / I / U", "Bold / italic / underline"),
                ("Ctrl+L / E / R / J", "Left / center / right / justify"),
                ("Ctrl+Space", "Normal (clear formatting)"),
                ("F4 / Shift+F4", "Enlarge / reduce text"),
                ("Ctrl+Enter", "Page break"),
                ("Tab / Shift+Tab", "Indent / outdent a list"),
                ("Ctrl+F / Ctrl+H", "Find / replace"),
                ("Ctrl+G", "Go to page"),
                ("Ctrl+= / Ctrl+- / Ctrl+0", "Zoom in / out / 100%"),
                ("Ctrl+wheel", "Zoom"),
                ("Ctrl+Tab", "Next document tab"),
            ];
            window("Keyboard Shortcuts").open(&mut open).show(ctx, |ui| {
                egui::Grid::new("ks").num_columns(2).striped(true).spacing(vec2(24.0, 4.0)).show(ui, |ui| {
                    for (k, d) in rows {
                        ui.label(RichText::new(k).strong());
                        ui.label(d);
                        ui.end_row();
                    }
                });
            });
        }
    }
    if !open {
        app.dialog = Dialog::None;
        ctx.memory_mut(|m| m.request_focus(app.doc_id));
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
    let title = if app.find.show_replace { "Find & Replace Text" } else { "Find Text" };
    egui::Window::new(title).id(egui::Id::unique("find-window")).open(open).collapsible(false).resizable(false).anchor(egui::Align2::RIGHT_TOP, vec2(-30.0, 150.0)).show(ctx, |ui| {
        let fs = &mut app.find;
        egui::Grid::new("fr").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
            ui.label("Find");
            let r = ui.add(egui::TextEdit::singleline(&mut fs.query).desired_width(240.0).hint_text("Text to find"));
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
            "No matches".into()
        } else if let Some(c) = cur {
            format!("{} of {} matches", c + 1, matches.len())
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
                if ui.add_enabled(!matches.is_empty(), egui::Button::new("Replace All")).clicked() {
                    replace_all = true;
                }
            } else if ui.small_button("Replace…").clicked() {
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
        window("Check Spelling").open(open).show(ctx, |ui| {
            ui.label("This computer has no English spelling dictionary installed.");
            ui.label("Add one in Windows Settings › Time & language › Language.");
        });
        return;
    }
    // first misspelling at or after the caret (wrapping round once)
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
    window("Check Spelling").open(open).show(ctx, |ui| {
        ui.set_min_width(360.0);
        if !app.speller.language.is_empty() {
            ui.label(RichText::new(format!("Dictionary: {}", app.speller.language)).weak().small());
        }
        match found {
            None => {
                ui.label(RichText::new("✓ No spelling mistakes found.").strong());
                if ui.button("Close").clicked() {
                    action = Some((9, String::new()));
                }
            }
            Some(_) => {
                ui.label("Not in the dictionary:");
                ui.label(RichText::new(&word).size(18.0).color(egui::Color32::from_rgb(220, 38, 38)).strong());
                let short: String = context.chars().take(140).collect();
                ui.label(RichText::new(short).weak().italics());
                ui.separator();
                ui.label("Suggestions:");
                if sugg.is_empty() {
                    ui.label(RichText::new("(none)").weak());
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
                    if ui.button("Skip").clicked() {
                        action = Some((2, String::new()));
                    }
                    if ui.button("Ignore All").clicked() {
                        action = Some((3, String::new()));
                    }
                    if ui.button("Add to Dictionary").clicked() {
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
        (None, Some((a, b))) => {
            // show the word selected in the page
            if tab.anchor != a || tab.caret != b {
                tab.anchor = a;
                tab.caret = b;
                tab.scroll_to_caret = true;
            }
        }
        _ => {}
    }
}

fn page_setup(app: &mut App, ctx: &egui::Context, open: &mut bool, now: f64) {
    let t = app.tab_ref();
    let mut pg = t.doc.page;
    let mut header = t.doc.header.clone();
    let mut footer = t.doc.footer.clone();
    window("Page Properties").open(open).show(ctx, |ui| {
        ui.heading("Size and margin");
        let land = pg.width > pg.height;
        egui::Grid::new("ps").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
            ui.label("Page size");
            let cur = PAGE_SIZES.iter().find(|(_, w, h)| {
                let (pw, ph) = if land { (*h, *w) } else { (*w, *h) };
                (pw - pg.width).abs() < 0.01 && (ph - pg.height).abs() < 0.01
            });
            egui::ComboBox::from_id_salt("psz").selected_text(cur.map(|c| c.0).unwrap_or("Custom")).width(220.0).show_ui(ui, |ui| {
                for (name, w, h) in PAGE_SIZES {
                    if ui.selectable_label(cur.map(|c| c.0 == *name).unwrap_or(false), *name).clicked() {
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
        ui.heading("Header and footer");
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
        ui.heading("Background");
        ui.horizontal(|ui| {
            for c in [[255u8, 255, 255], [255, 253, 245], [250, 247, 240], [245, 248, 255], [244, 250, 246], [255, 247, 250], [248, 248, 248]] {
                if swatch(ui, c, pg.background == c).clicked() {
                    pg.background = c;
                }
            }
        });
    });
    let t = app.tab();
    if pg != t.doc.page || header != t.doc.header || footer != t.doc.footer {
        t.checkpoint(EditKind::Typing, now);
        t.doc.page = pg;
        t.doc.header = header;
        t.doc.footer = footer;
        t.changed();
        t.scroll_to_caret = false;
    }
}

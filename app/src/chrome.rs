//! Menus (names and order from the original's menu resource 1002, without scanning,
//! internet or sharing), the modern toolbar, document tabs, status bar and ruler.

use eframe::egui::{self, vec2, Color32, FontFamily, FontId, Frame, Margin, RichText, Sense, Stroke};

use crate::app::{icon_button, sep, swatch, Action, App, PageEdit, ZoomMode, HIGHLIGHTS, PALETTE, ZOOM_MAX, ZOOM_MIN};
use crate::dialogs::Dialog;
use crate::doc::*;
use crate::icons::{self, Icon};
use crate::layout::{self, BASE_PPI};
use crate::theme::Theme;

fn item(ui: &mut egui::Ui, label: &str, sc: &str) -> bool {
    let b = if sc.is_empty() { egui::Button::new(label) } else { egui::Button::new(label).shortcut_text(sc) };
    ui.add(b).clicked()
}

impl App {
    pub fn menus(&mut self, ui: &mut egui::Ui) {
        let has_sel = self.tab_ref().has_selection();
        let can_undo = self.tab_ref().can_undo();
        let can_redo = self.tab_ref().can_redo();

        ui.menu_button("File", |ui| {
            if item(ui, "New Document", "Ctrl+N") {
                self.act(Action::New);
            }
            if item(ui, "Open…", "Ctrl+O") {
                self.act(Action::Open);
            }
            if item(ui, "Close", "Ctrl+W") {
                let i = self.active;
                self.act(Action::Close(i));
            }
            ui.separator();
            if item(ui, "Save", "Ctrl+S") {
                self.act(Action::Save);
            }
            if item(ui, "Save As…", "Ctrl+Shift+S") {
                self.act(Action::SaveAs);
            }
            ui.menu_button("Export", |ui| {
                if item(ui, "PDF (.pdf)…", "") {
                    self.act(Action::ExportPdf);
                }
                if item(ui, "Word (.docx)…", "") {
                    self.act(Action::ExportDocx);
                }
                if item(ui, "Rich Text (.rtf)…", "") {
                    self.act(Action::ExportRtf);
                }
                if item(ui, "Plain Text (.txt)…", "") {
                    self.act(Action::ExportTxt);
                }
            });
            if item(ui, "Print…", "Ctrl+P") {
                self.act(Action::Print);
            }
            ui.separator();
            ui.menu_button("Document Properties", |ui| {
                if item(ui, "Document…", "") {
                    self.act(Action::Dialog(Dialog::DocProps));
                }
                if item(ui, "Page Setup…", "") {
                    self.act(Action::Dialog(Dialog::PageSetup));
                }
            });
            ui.menu_button("User Setup", |ui| {
                if item(ui, "Preferences…", "") {
                    self.act(Action::Dialog(Dialog::Prefs));
                }
                if item(ui, "SmartCorrect Setup…", "") {
                    self.act(Action::Dialog(Dialog::SmartCorrect(String::new(), String::new())));
                }
            });
            if !self.cfg.recent.is_empty() {
                ui.separator();
                ui.label(RichText::new("Recent documents").small().weak());
                for (i, p) in self.cfg.recent.clone().iter().enumerate() {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    if ui.add(egui::Button::new(format!("{}  {}", i + 1, name))).on_hover_text(p.display().to_string()).clicked() {
                        self.act(Action::OpenPath(p.clone()));
                    }
                }
            }
            ui.separator();
            if item(ui, "Exit", "Alt+F4") {
                self.act(Action::Quit);
            }
        });

        ui.menu_button("Edit", |ui| {
            if ui.add_enabled(can_undo, egui::Button::new("Undo").shortcut_text("Ctrl+Z")).clicked() {
                self.act(Action::Undo);
            }
            if ui.add_enabled(can_redo, egui::Button::new("Redo").shortcut_text("Ctrl+Y")).clicked() {
                self.act(Action::Redo);
            }
            ui.separator();
            if ui.add_enabled(has_sel, egui::Button::new("Cut").shortcut_text("Ctrl+X")).clicked() {
                self.act(Action::Cut);
            }
            if ui.add_enabled(has_sel, egui::Button::new("Copy").shortcut_text("Ctrl+C")).clicked() {
                self.act(Action::Copy);
            }
            if item(ui, "Paste", "Ctrl+V") {
                self.act(Action::Paste);
            }
            if item(ui, "Select All", "Ctrl+A") {
                self.act(Action::SelectAll);
            }
            ui.separator();
            if item(ui, "Go To…", "Ctrl+G") {
                let p = self.tab_ref().current_page + 1;
                self.act(Action::Dialog(Dialog::GoTo(p)));
            }
            if item(ui, "Find & Replace Text…", "Ctrl+H") {
                self.act(Action::Dialog(Dialog::Find(true)));
            }
            ui.menu_button("Proofing Tools", |ui| {
                if item(ui, "Quick Find…", "Ctrl+F") {
                    self.act(Action::Dialog(Dialog::Find(false)));
                }
                if item(ui, "Check Spelling…", "F7") {
                    self.act(Action::Dialog(Dialog::Spell(0)));
                }
                if item(ui, "Word Count…", "") {
                    self.act(Action::Dialog(Dialog::WordCount));
                }
                ui.separator();
                let mut sp = self.cfg.spellcheck;
                if ui.checkbox(&mut sp, "Check spelling as you type").changed() {
                    self.act(Action::ToggleSpell);
                }
                let mut on = self.cfg.smartcorrect;
                if ui.checkbox(&mut on, "SmartCorrect while typing").changed() {
                    self.cfg.smartcorrect = on;
                    self.save_config();
                }
            });
        });

        ui.menu_button("View", |ui| {
            if ui.add(egui::Button::new("Layout").selected(!self.draft)).clicked() {
                self.act(Action::Draft(false));
            }
            if ui.add(egui::Button::new("Draft").selected(self.draft)).clicked() {
                self.act(Action::Draft(true));
            }
            ui.separator();
            if ui.add(egui::Button::new("Full Page").selected(self.zoom_mode == ZoomMode::FullPage)).clicked() {
                self.act(Action::ZoomMode(ZoomMode::FullPage));
            }
            if ui.add(egui::Button::new("Standard (100%)").shortcut_text("Ctrl+0").selected(self.zoom_mode == ZoomMode::Percent && self.zoom == 1.0)).clicked() {
                self.act(Action::SetZoom(1.0));
            }
            ui.menu_button("Zoom To", |ui| {
                if ui.add(egui::Button::new("Margin Width").selected(self.zoom_mode == ZoomMode::MarginWidth)).clicked() {
                    self.act(Action::ZoomMode(ZoomMode::MarginWidth));
                }
                if ui.add(egui::Button::new("Page Width").selected(self.zoom_mode == ZoomMode::PageWidth)).clicked() {
                    self.act(Action::ZoomMode(ZoomMode::PageWidth));
                }
                for z in [25, 50, 75, 150, 200] {
                    let sel = self.zoom_mode == ZoomMode::Percent && (self.zoom * 100.0).round() as i32 == z;
                    if ui.add(egui::Button::new(format!("{z}%")).selected(sel)).clicked() {
                        self.act(Action::SetZoom(z as f32 / 100.0));
                    }
                }
                ui.separator();
                if item(ui, "Zoom In", "Ctrl+=") {
                    self.act(Action::ZoomIn);
                }
                if item(ui, "Zoom Out", "Ctrl+-") {
                    self.act(Action::ZoomOut);
                }
            });
            ui.separator();
            if ui.add(egui::Button::new("Navigation Pane").selected(self.show_nav)).clicked() {
                self.act(Action::ToggleNav);
            }
            ui.menu_button("Show/Hide", |ui| {
                ui.checkbox(&mut self.show_toolbar, "Toolbar");
                ui.checkbox(&mut self.show_status, "Status Bar");
                ui.checkbox(&mut self.show_ruler, "Ruler");
                ui.checkbox(&mut self.show_hf, "Headers & Footers");
                ui.checkbox(&mut self.show_marks, "Paragraph Marks");
                if ui.checkbox(&mut self.cfg.margin_guides, "Margin Guides").changed() {
                    self.save_config();
                }
                ui.checkbox(&mut self.clean_screen, "Clean Screen");
            });
            ui.separator();
            let label = if self.cfg.dark { "Light Theme" } else { "Dark Theme" };
            if item(ui, label, "") {
                self.act(Action::ToggleDark);
            }
        });

        ui.menu_button("Create", |ui| {
            if item(ui, "Page Break", "Ctrl+Enter") {
                self.act(Action::PageBreak);
            }
            if item(ui, "Bulleted List", "") {
                self.act(Action::List(ListKind::Bullet));
            }
            if item(ui, "Numbered List", "") {
                self.act(Action::List(ListKind::Number));
            }
            ui.separator();
            if item(ui, "Date/Time…", "") {
                self.act(Action::Dialog(Dialog::DateTime));
            }
            if item(ui, "Symbol…", "") {
                self.act(Action::Dialog(Dialog::Symbol));
            }
        });

        ui.menu_button("Page", |ui| {
            if item(ui, "Page Properties…", "") {
                self.act(Action::Dialog(Dialog::PageSetup));
            }
            if item(ui, "Header/Footer…", "") {
                self.act(Action::Dialog(Dialog::PageSetup));
            }
            ui.separator();
            let cur = self.tab_ref().doc.page;
            let land = cur.width > cur.height;
            ui.menu_button("Page Size", |ui| {
                for (name, w, h) in PAGE_SIZES {
                    let (pw, ph) = if land { (*h, *w) } else { (*w, *h) };
                    let sel = (cur.width - pw).abs() < 0.01 && (cur.height - ph).abs() < 0.01;
                    if ui.add(egui::Button::new(*name).selected(sel)).clicked() {
                        self.act(Action::PageEdit(PageEdit::Size(pw, ph)));
                    }
                }
            });
            ui.menu_button("Orientation", |ui| {
                if ui.add(egui::Button::new("Portrait").selected(!land)).clicked() && land {
                    self.act(Action::PageEdit(PageEdit::Swap));
                }
                if ui.add(egui::Button::new("Landscape").selected(land)).clicked() && !land {
                    self.act(Action::PageEdit(PageEdit::Swap));
                }
            });
            if item(ui, "Margins…", "") {
                self.act(Action::Dialog(Dialog::PageSetup));
            }
            ui.separator();
            let mut pn = cur.page_numbers;
            if ui.checkbox(&mut pn, "Page Numbers in Footer").changed() {
                self.act(Action::PageEdit(PageEdit::Numbers(pn)));
            }
            ui.separator();
            if item(ui, "Insert Page Break", "Ctrl+Enter") {
                self.act(Action::PageBreak);
            }
        });

        ui.menu_button("Text", |ui| {
            if item(ui, "Font & Color…", "") {
                self.act(Action::Dialog(Dialog::Font));
            }
            if item(ui, "Paragraph Spacing…", "") {
                self.act(Action::Dialog(Dialog::Paragraph));
            }
            ui.menu_button("Bullets & Numbers", |ui| {
                if item(ui, "Bullets", "") {
                    self.act(Action::List(ListKind::Bullet));
                }
                if item(ui, "Numbers", "") {
                    self.act(Action::List(ListKind::Number));
                }
            });
            ui.menu_button("Named Styles", |ui| {
                for (i, s) in named_styles().iter().enumerate() {
                    if item(ui, s.name, "") {
                        self.act(Action::Style(i));
                    }
                }
            });
            ui.separator();
            if item(ui, "Normal", "Ctrl+Space") {
                self.act(Action::ClearFormat);
            }
            ui.menu_button("Attributes", |ui| {
                if item(ui, "Bold", "Ctrl+B") {
                    self.act(Action::Bold);
                }
                if item(ui, "Italic", "Ctrl+I") {
                    self.act(Action::Italic);
                }
                if item(ui, "Underline", "Ctrl+U") {
                    self.act(Action::Underline);
                }
                if item(ui, "Strikethrough", "") {
                    self.act(Action::Strike);
                }
                if item(ui, "Superscript", "") {
                    self.act(Action::Script(1));
                }
                if item(ui, "Subscript", "") {
                    self.act(Action::Script(-1));
                }
                ui.separator();
                if item(ui, "Enlarge Text", "F4") {
                    self.act(Action::Grow);
                }
                if item(ui, "Reduce Text", "Shift+F4") {
                    self.act(Action::Shrink);
                }
            });
            ui.menu_button("Alignment", |ui| {
                for (l, a, k) in [("Left", Align::Left, "Ctrl+L"), ("Center", Align::Center, "Ctrl+E"), ("Right", Align::Right, "Ctrl+R"), ("Full Justify", Align::Justify, "Ctrl+J")] {
                    if item(ui, l, k) {
                        self.act(Action::Align(a));
                    }
                }
                ui.separator();
                if item(ui, "Indent", "Tab") {
                    self.act(Action::Indent(1));
                }
                if item(ui, "Outdent", "Shift+Tab") {
                    self.act(Action::Indent(-1));
                }
            });
            ui.menu_button("Line Spacing", |ui| {
                for v in [1.0f32, 1.15, 1.5, 2.0, 2.5, 3.0] {
                    if item(ui, &format!("{v:.2}"), "") {
                        self.act(Action::LineSpacing(v));
                    }
                }
            });
            if ui.add(egui::Button::new("Format Painter").selected(self.painter_style.is_some())).clicked() {
                self.act(Action::FormatPainter);
            }
            ui.menu_button("Change Case", |ui| {
                if item(ui, "UPPERCASE", "") {
                    self.act(Action::Case(CaseChange::Upper));
                }
                if item(ui, "lowercase", "") {
                    self.act(Action::Case(CaseChange::Lower));
                }
                if item(ui, "Title Case", "") {
                    self.act(Action::Case(CaseChange::Title));
                }
                if item(ui, "Sentence case", "") {
                    self.act(Action::Case(CaseChange::Sentence));
                }
            });
            ui.separator();
            ui.menu_button("Insert Other", |ui| {
                if item(ui, "Date/Time…", "") {
                    self.act(Action::Dialog(Dialog::DateTime));
                }
                if item(ui, "Symbol…", "") {
                    self.act(Action::Dialog(Dialog::Symbol));
                }
            });
        });

        ui.menu_button("Window", |ui| {
            if item(ui, "New Document Tab", "Ctrl+N") {
                self.act(Action::New);
            }
            if item(ui, "Close Tab", "Ctrl+W") {
                let i = self.active;
                self.act(Action::Close(i));
            }
            if item(ui, "Close All Tabs", "") {
                self.act(Action::CloseAll);
            }
            if item(ui, "Next Tab", "Ctrl+Tab") {
                self.act(Action::NextTab);
            }
            ui.separator();
            let titles: Vec<String> = self.tabs.iter().map(|t| t.title()).collect();
            for (i, t) in titles.iter().enumerate() {
                if ui.add(egui::Button::new(format!("{}  {}", i + 1, t)).selected(i == self.active)).clicked() {
                    self.act(Action::SelectTab(i));
                }
            }
        });

        ui.menu_button("Help", |ui| {
            if item(ui, "Keyboard Shortcuts", "") {
                self.act(Action::Dialog(Dialog::Shortcuts));
            }
            ui.separator();
            if item(ui, "About Scribe Pro…", "") {
                self.act(Action::Dialog(Dialog::About));
            }
        });
    }

    /// Modern replacement for the SmartIcons bar.
    pub fn toolbar(&mut self, ui: &mut egui::Ui, th: &Theme) {
        let t = self.tab_ref();
        let st = t.typing_style();
        let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
        let has_sel = t.has_selection();
        let (can_undo, can_redo) = (t.can_undo(), t.can_redo());
        let style_name = t.current_style_name();

        macro_rules! b {
            ($icon:expr, $tip:expr, $sel:expr, $en:expr, $act:expr) => {
                if icon_button(ui, th, $icon, $tip, $sel, $en).clicked() {
                    self.act($act);
                }
            };
        }
        b!(Icon::New, "New document (Ctrl+N)", false, true, Action::New);
        b!(Icon::Open, "Open (Ctrl+O)", false, true, Action::Open);
        b!(Icon::Save, "Save (Ctrl+S)", false, true, Action::Save);
        b!(Icon::Print, "Print (Ctrl+P)", false, true, Action::Print);
        b!(Icon::Pdf, "Export as PDF", false, true, Action::ExportPdf);
        sep(ui, th);
        b!(Icon::Undo, "Undo (Ctrl+Z)", false, can_undo, Action::Undo);
        b!(Icon::Redo, "Redo (Ctrl+Y)", false, can_redo, Action::Redo);
        sep(ui, th);
        b!(Icon::Cut, "Cut (Ctrl+X)", false, has_sel, Action::Cut);
        b!(Icon::Copy, "Copy (Ctrl+C)", false, has_sel, Action::Copy);
        b!(Icon::Paste, "Paste (Ctrl+V)", false, true, Action::Paste);
        b!(Icon::Painter, "Format painter: copy formatting to the next selection", self.painter_style.is_some(), true, Action::FormatPainter);
        sep(ui, th);

        let styles = named_styles();
        egui::ComboBox::from_id_salt("style-combo").width(112.0).selected_text(style_name).show_ui(ui, |ui| {
            for (i, s) in styles.iter().enumerate() {
                let (fam, _) = self.book.family(s.chr.font, s.chr.bold, s.chr.italic);
                let size = (s.chr.size * 0.9).clamp(11.0, 20.0);
                if ui.selectable_label(s.name == style_name, RichText::new(s.name).font(FontId::new(size, fam))).clicked() {
                    self.act(Action::Style(i));
                }
            }
        });
        // font picker: the current font shown in its own typeface; opens a searchable, grouped list
        let r = font_button(ui, th, &self.book, st.font);
        if std::env::var("SCRIBE_DEMO_FONTS").is_ok() && ui.input(|i| i.time) > 0.5 && !self.demo_done {
            egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&r));
            self.demo_done = true;
        }
        let mut picked = None;
        let opened_now = r.clicked();
        egui::Popup::menu(&r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            let recent = self.cfg.recent_fonts.clone();
            if let Some(f) = font_list(ui, th, &self.book, st.font, &mut self.font_search, &recent, opened_now, 420.0) {
                picked = Some(f);
                ui.close();
            }
        });
        if let Some(f) = picked {
            self.font_search.clear();
            self.act(Action::Font(f));
        }
        let size_txt = if st.size.fract() == 0.0 { format!("{}", st.size as i32) } else { format!("{:.1}", st.size) };
        egui::ComboBox::from_id_salt("size-combo").width(46.0).selected_text(size_txt).height(360.0).show_ui(ui, |ui| {
            for s in SIZE_LIST {
                if ui.selectable_label((st.size - s).abs() < 0.01, format!("{}", *s as i32)).clicked() {
                    self.act(Action::Size(*s));
                }
            }
        });
        sep(ui, th);
        b!(Icon::Bold, "Bold (Ctrl+B)", st.bold, true, Action::Bold);
        b!(Icon::Italic, "Italic (Ctrl+I)", st.italic, true, Action::Italic);
        b!(Icon::Underline, "Underline (Ctrl+U)", st.underline, true, Action::Underline);
        b!(Icon::Strike, "Strikethrough", st.strike, true, Action::Strike);
        let r = icon_button(ui, th, Icon::TextColor, "Text colour", false, true);
        egui::Popup::menu(&r).show(|ui| {
            ui.label("Text colour");
            egui::Grid::new("pal").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
                for (i, c) in PALETTE.iter().enumerate() {
                    if swatch(ui, *c, st.color == *c).clicked() {
                        self.act(Action::Color(*c));
                        ui.close();
                    }
                    if i % 5 == 4 {
                        ui.end_row();
                    }
                }
            });
        });
        let r = icon_button(ui, th, Icon::Highlight, "Highlight", st.highlight.is_some(), true);
        egui::Popup::menu(&r).show(|ui| {
            ui.label("Highlight");
            ui.horizontal(|ui| {
                for c in HIGHLIGHTS {
                    if swatch(ui, *c, st.highlight == Some(*c)).clicked() {
                        self.act(Action::Highlight(Some(*c)));
                        ui.close();
                    }
                }
            });
            if ui.button("No highlight").clicked() {
                self.act(Action::Highlight(None));
                ui.close();
            }
        });
        b!(Icon::Clear, "Normal — clear formatting (Ctrl+Space)", false, true, Action::ClearFormat);
        sep(ui, th);
        b!(Icon::AlignLeft, "Align left (Ctrl+L)", fmt.align == Align::Left, true, Action::Align(Align::Left));
        b!(Icon::AlignCenter, "Center (Ctrl+E)", fmt.align == Align::Center, true, Action::Align(Align::Center));
        b!(Icon::AlignRight, "Align right (Ctrl+R)", fmt.align == Align::Right, true, Action::Align(Align::Right));
        b!(Icon::AlignJustify, "Full justify (Ctrl+J)", fmt.align == Align::Justify, true, Action::Align(Align::Justify));
        sep(ui, th);
        b!(Icon::Bullets, "Bullets", fmt.list == ListKind::Bullet, true, Action::List(ListKind::Bullet));
        b!(Icon::Numbers, "Numbers", fmt.list == ListKind::Number, true, Action::List(ListKind::Number));
        b!(Icon::Outdent, "Outdent (Shift+Tab)", false, fmt.indent > 0, Action::Indent(-1));
        b!(Icon::Indent, "Indent (Tab)", false, true, Action::Indent(1));
        let r = icon_button(ui, th, Icon::LineSpacing, "Line spacing", false, true);
        egui::Popup::menu(&r).show(|ui| {
            for v in [1.0f32, 1.15, 1.5, 2.0, 2.5, 3.0] {
                if ui.selectable_label((fmt.line_spacing - v).abs() < 0.01, format!("{v:.2}")).clicked() {
                    self.act(Action::LineSpacing(v));
                    ui.close();
                }
            }
        });
        sep(ui, th);
        b!(Icon::PageBreak, "Insert page break (Ctrl+Enter)", false, true, Action::PageBreak);
        b!(Icon::Symbol, "Insert symbol", false, true, Action::Dialog(Dialog::Symbol));
        sep(ui, th);
        b!(Icon::Find, "Find & replace (Ctrl+F)", matches!(self.dialog, Dialog::Find(_)), true, Action::Dialog(Dialog::Find(true)));
        b!(Icon::Spell, "Check spelling (F7)", matches!(self.dialog, Dialog::Spell(_)), self.speller.available(), Action::Dialog(Dialog::Spell(0)));
        b!(Icon::WordCount, "Word count", false, true, Action::Dialog(Dialog::WordCount));
        b!(Icon::NavPane, "Navigation pane (headings)", self.show_nav, true, Action::ToggleNav);
        b!(Icon::PageSetup, "Page setup", false, true, Action::Dialog(Dialog::PageSetup));
        if icon_button(ui, th, Icon::Marks, "Show paragraph marks", self.show_marks, true).clicked() {
            self.show_marks = !self.show_marks;
        }
        sep(ui, th);
        let icon = if self.cfg.dark { Icon::Sun } else { Icon::Moon };
        b!(icon, "Switch light / dark theme", false, true, Action::ToggleDark);
    }

    pub fn tab_strip(&mut self, ui: &mut egui::Ui, th: &Theme) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let mut close = None;
            let mut select = None;
            for (i, t) in self.tabs.iter().enumerate() {
                let active = i == self.active;
                let fill = if active { th.workspace } else { Color32::TRANSPARENT };
                let fr = Frame::new()
                    .fill(fill)
                    .corner_radius(egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 })
                    .inner_margin(Margin { left: 12, right: 6, top: 5, bottom: 5 })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let txt = RichText::new(t.title()).color(if active { th.text } else { th.subtle });
                            if ui.add(egui::Label::new(txt).sense(Sense::click())).clicked() {
                                select = Some(i);
                            }
                            let (r, cr) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                            if cr.hovered() {
                                ui.painter().rect_filled(r, 4.0, th.hover);
                            }
                            icons::draw(ui.painter(), r.shrink(4.0), Icon::Close, th.subtle, th.accent);
                            if cr.on_hover_text("Close").clicked() {
                                close = Some(i);
                            }
                        });
                    });
                if active {
                    let name = t.name.clone();
                    ui.interact(fr.response.rect, egui::Id::unique("doc-title-a11y"), Sense::hover()).widget_info(move || egui::WidgetInfo::labeled(egui::Role::TitleBar, true, name.clone()));
                }
            }
            let (r, pr) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
            if pr.hovered() {
                ui.painter().rect_filled(r, 6.0, th.hover);
            }
            icons::draw(ui.painter(), r.shrink(5.0), Icon::New, th.subtle, th.accent);
            if pr.on_hover_text("New document").clicked() {
                self.act(Action::New);
            }
            if let Some(i) = select {
                self.act(Action::SelectTab(i));
            }
            if let Some(i) = close {
                self.act(Action::Close(i));
            }
        });
    }

    pub fn status_bar(&mut self, ui: &mut egui::Ui, th: &Theme) {
        let zoom = self.effective_zoom();
        let t = self.tab_ref();
        let pages = t.cached_pages();
        let st = t.typing_style();
        let (chars, words) = t.doc.counts(None);
        let sel_words = if t.has_selection() {
            let (a, b) = t.selection();
            Some(t.doc.counts(Some((a, b))).1)
        } else {
            None
        };
        let style = t.current_style_name();
        let page = t.current_page + 1;
        let path = t.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "Not saved yet".into());
        let font = FONT_NAMES[st.font as usize % FONT_NAMES.len()];
        let draft = self.draft;
        ui.horizontal(|ui| {
            let small = |s: String| RichText::new(s).size(12.5).color(th.subtle);
            // " Pg %d" — the original's page indicator (FUN_00452923)
            ui.label(small(format!("Pg {page} of {pages}")));
            ui.separator();
            match sel_words {
                Some(sw) => ui.label(small(format!("{sw} of {words} words"))),
                None => ui.label(small(format!("{words} words"))),
            };
            ui.label(small(format!("{chars} characters")));
            ui.separator();
            ui.label(small(format!("{font} {}", st.size)));
            ui.separator();
            ui.label(small(style.to_string()));
            ui.separator();
            ui.label(small(path));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new(format!("{}%", (zoom * 100.0).round() as i32)).font(FontId::new(12.5, FontFamily::Proportional)).color(th.text));
                if icon_button(ui, th, Icon::ZoomIn, "Zoom in (Ctrl+=)", false, zoom < ZOOM_MAX).clicked() {
                    self.act(Action::ZoomIn);
                }
                let mut z = zoom * 100.0;
                let s = ui.add_sized([140.0, 18.0], egui::Slider::new(&mut z, (ZOOM_MIN * 100.0)..=(ZOOM_MAX * 100.0)).show_value(false));
                if s.changed() {
                    self.act(Action::SetZoom((z / 5.0).round() * 5.0 / 100.0));
                }
                if icon_button(ui, th, Icon::ZoomOut, "Zoom out (Ctrl+-)", false, zoom > ZOOM_MIN).clicked() {
                    self.act(Action::ZoomOut);
                }
                ui.separator();
                if icon_button(ui, th, Icon::PageWidth, "Page width", self.zoom_mode == ZoomMode::PageWidth, true).clicked() {
                    self.act(Action::ZoomMode(ZoomMode::PageWidth));
                }
                if icon_button(ui, th, Icon::FullPage, "Full page", self.zoom_mode == ZoomMode::FullPage, true).clicked() {
                    self.act(Action::ZoomMode(ZoomMode::FullPage));
                }
                ui.separator();
                if ui.selectable_label(draft, small("Draft".into())).clicked() {
                    self.act(Action::Draft(true));
                }
                if ui.selectable_label(!draft, small("Layout".into())).clicked() {
                    self.act(Action::Draft(false));
                }
            });
        });
    }

    pub fn ruler(&mut self, ui: &mut egui::Ui, th: &Theme, zoom: f32) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, th.chrome);
        p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, th.border));
        let t = self.tab_ref();
        let pg = t.doc.page;
        let ppi = BASE_PPI * zoom;
        let page_w = pg.width * ppi;
        let (cx0, cw) = if t.canvas_w > 0.0 { (t.canvas_x0, t.canvas_w) } else { (rect.min.x, rect.width()) };
        let x0 = cx0 + ((cw - page_w) / 2.0).max(layout::PAGE_GAP);
        let band = egui::Rect::from_min_max(egui::pos2(x0, rect.min.y + 4.0), egui::pos2(x0 + page_w, rect.max.y - 4.0));
        p.rect_filled(band, 2.0, if th.dark { Color32::from_rgb(58, 62, 72) } else { Color32::from_rgb(226, 230, 236) });
        let ml = x0 + pg.margin_left * ppi;
        let mr = x0 + (pg.width - pg.margin_right) * ppi;
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(ml, band.min.y), egui::pos2(mr, band.max.y)), 0.0, if th.dark { Color32::from_rgb(30, 33, 39) } else { Color32::WHITE });
        let from = -(pg.margin_left * 8.0).floor() as i32;
        let to = ((pg.width - pg.margin_left) * 8.0).ceil() as i32;
        for k in from..=to {
            let x = ml + k as f32 * ppi / 8.0;
            if x < band.min.x || x > band.max.x {
                continue;
            }
            if k % 8 == 0 {
                if k != 0 {
                    p.text(egui::pos2(x, band.center().y), egui::Align2::CENTER_CENTER, format!("{}", k.abs() / 8), FontId::proportional(9.5), th.subtle);
                }
            } else if ppi / 8.0 > 3.0 || k % 2 == 0 {
                let h = if k % 4 == 0 { 6.0 } else if k % 2 == 0 { 4.0 } else { 2.5 };
                p.line_segment([egui::pos2(x, band.center().y - h / 2.0), egui::pos2(x, band.center().y + h / 2.0)], Stroke::new(1.0, th.subtle.gamma_multiply(0.7)));
            }
        }
        let f = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
        let ix = ml + f.indent as f32 * layout::INDENT_STEP * ppi;
        for x in [ix, mr] {
            let tri = vec![egui::pos2(x - 5.0, band.max.y), egui::pos2(x + 5.0, band.max.y), egui::pos2(x, band.max.y - 6.0)];
            p.add(egui::Shape::convex_polygon(tri, th.accent, Stroke::NONE));
        }
    }

    /// Headings of the document, click one to jump there.
    pub fn nav_pane(&mut self, ui: &mut egui::Ui, th: &Theme) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Navigation").strong().color(th.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (r, cr) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                if cr.hovered() {
                    ui.painter().rect_filled(r, 4.0, th.hover);
                }
                icons::draw(ui.painter(), r.shrink(4.0), Icon::Close, th.subtle, th.accent);
                if cr.on_hover_text("Close").clicked() {
                    self.act(Action::ToggleNav);
                }
            });
        });
        ui.add_space(4.0);
        let styles = named_styles();
        let t = self.tab_ref();
        let caret_p = t.doc.clamp(t.caret).p;
        let mut heads: Vec<(usize, usize, String)> = Vec::new();
        for (i, p) in t.doc.paras.iter().enumerate() {
            let level = match styles.get(p.fmt.style).map(|s| s.name) {
                Some("Title") => 0,
                Some("Heading 1") => 1,
                Some("Heading 2") => 2,
                Some("Heading 3") => 3,
                _ => continue,
            };
            let text = p.string();
            if !text.trim().is_empty() {
                heads.push((i, level, text));
            }
        }
        // the heading the caret is under
        let current = heads.iter().rev().find(|h| h.0 <= caret_p).map(|h| h.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if heads.is_empty() {
                ui.label(RichText::new("Headings you add (Title, Heading 1–3) appear here so you can jump between sections.").weak().size(12.5));
            }
            for (i, level, text) in heads {
                ui.horizontal(|ui| {
                    ui.add_space(level.saturating_sub(1) as f32 * 14.0);
                    let short: String = if text.chars().count() > 40 { text.chars().take(38).collect::<String>() + "…" } else { text };
                    let rt = if level <= 1 { RichText::new(short).strong() } else { RichText::new(short) };
                    if ui.selectable_label(current == Some(i), rt).clicked() {
                        self.act(Action::GoToPara(i));
                    }
                });
            }
        });
    }
}

/// Toolbar button that shows the current font name in that font.
pub fn font_button(ui: &mut egui::Ui, th: &Theme, book: &crate::fonts::FontBook, font: u8) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(176.0, 28.0), Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let bg = if open { th.active } else if resp.hovered() { th.hover } else { ui.visuals().widgets.inactive.weak_bg_fill };
    ui.painter().rect_filled(rect, 6.0, bg);
    let (fam, _) = book.family(font, false, false);
    let name = FONT_NAMES[font as usize % FONT_NAMES.len()];
    let text_rect = egui::Rect::from_min_max(rect.min + vec2(10.0, 0.0), rect.max - vec2(26.0, 0.0));
    ui.painter_at(text_rect).text(egui::pos2(text_rect.min.x, rect.center().y), egui::Align2::LEFT_CENTER, name, FontId::new(15.0, fam), th.text);
    let c = egui::pos2(rect.max.x - 14.0, rect.center().y);
    ui.painter().add(egui::Shape::line(vec![c + vec2(-4.0, -2.0), c + vec2(0.0, 2.0), c + vec2(4.0, -2.0)], Stroke::new(1.5, th.subtle)));
    resp.on_hover_text("Font")
}

/// Searchable font list grouped by kind, each name drawn in its own typeface.
pub fn font_list(ui: &mut egui::Ui, th: &Theme, book: &crate::fonts::FontBook, current: u8, search: &mut String, recent: &[u8], focus: bool, max_h: f32) -> Option<u8> {
    let mut picked = None;
    let width = 300.0;
    ui.set_width(width);
    let r = ui.add(egui::TextEdit::singleline(search).hint_text("Search fonts").desired_width(width));
    if focus {
        r.request_focus();
    }
    let q = search.trim().to_lowercase();
    let available = |i: u8| book.faces.get(&(i, 0)).map(|f| f.is_some()).unwrap_or(false);
    let row = |ui: &mut egui::Ui, i: u8| -> bool {
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 32.0), Sense::click());
        if i == current {
            ui.painter().rect_filled(rect, 6.0, th.active);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 6.0, th.hover);
        }
        let (fam, _) = book.family(i, false, false);
        let name = FONT_NAMES[i as usize];
        let bundled = crate::doc::bundled_folder(i).is_some();
        let text_clip = egui::Rect::from_min_max(rect.min, rect.max - vec2(if bundled { 70.0 } else { 30.0 }, 0.0));
        let g = ui.painter().layout_no_wrap(name.to_string(), FontId::new(17.0, fam), th.text);
        let baseline = g.rows.first().and_then(|r| r.row.glyphs.first()).map(|gl| gl.pos.y).unwrap_or(g.size().y * 0.8);
        ui.painter_at(text_clip).galley(egui::pos2(rect.min.x + 12.0, rect.center().y + 6.0 - baseline), g, th.text);
        if bundled {
            ui.painter().text(egui::pos2(rect.max.x - 10.0, rect.center().y), egui::Align2::RIGHT_CENTER, "included", FontId::proportional(10.5), th.accent);
        }
        if i == current {
            ui.painter().text(egui::pos2(rect.max.x - if bundled { 62.0 } else { 12.0 }, rect.center().y), egui::Align2::RIGHT_CENTER, "✓", FontId::proportional(13.0), th.accent);
        }
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    };
    let header = |ui: &mut egui::Ui, t: &str| {
        ui.add_space(6.0);
        ui.label(RichText::new(t.to_uppercase()).size(10.5).color(th.subtle).strong());
    };
    egui::ScrollArea::vertical().max_height(max_h).id_salt("font-list").show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        if q.is_empty() {
            let rec: Vec<u8> = recent.iter().copied().filter(|i| (*i as usize) < FONT_NAMES.len() && available(*i)).collect();
            if !rec.is_empty() {
                header(ui, "Recently used");
                for i in rec {
                    if row(ui, i) {
                        picked = Some(i);
                    }
                }
            }
        }
        let mut any = false;
        for cat in FontCat::ALL {
            let mut items: Vec<u8> = (0..FONT_NAMES.len() as u8)
                .filter(|i| font_category(*i) == cat && available(*i) && (q.is_empty() || FONT_NAMES[*i as usize].to_lowercase().contains(&q)))
                .collect();
            items.sort_by_key(|i| FONT_NAMES[*i as usize]);
            if items.is_empty() {
                continue;
            }
            any = true;
            header(ui, cat.label());
            for i in items {
                if row(ui, i) {
                    picked = Some(i);
                }
            }
        }
        if !any {
            ui.label(RichText::new("No fonts match.").weak());
        }
    });
    picked
}

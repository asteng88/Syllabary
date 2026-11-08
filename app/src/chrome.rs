//! Google Docs menu bar (File, Edit, View, Insert, Format, Tools, Window, Help) with
//! vector menu icons, the Google Docs header & pill-shaped toolbar, document tabs,
//! outline/summary sidebar, comments/suggestions sidebar, status bar and ruler.

use eframe::egui::{self, pos2, vec2, Color32, FontFamily, FontId, Frame, Margin, RichText, Sense, Stroke};

use crate::app::{
    gradient_swatch, icon_button, icon_highlight_button, icon_text_color_button, sep, swatch, text_gradient_swatch, Action, App, PageEdit, ZoomMode,
    CRAFT_GRADIENT_HIGHLIGHTS, CRAFT_GRADIENT_TEXT, HIGHLIGHTS, PALETTE, ZOOM_MAX, ZOOM_MIN,
};
use crate::dialogs::Dialog;
use crate::doc::*;
use crate::editor::DocMode;
use crate::icons::{self, Icon};
use crate::layout::{self, BASE_PPI};
use crate::theme::Theme;

/// Computes the natural content width of a menu entry (icon badge + label + gap + shortcut pill + padding).
pub fn entry_width(ui: &egui::Ui, label: &str, sc: &str) -> f32 {
    let lw = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), FontId::proportional(13.0), Color32::WHITE).size().x);
    let sw = if sc.is_empty() {
        0.0
    } else {
        22.0 + ui.fonts_mut(|f| f.layout_no_wrap(sc.to_string(), FontId::proportional(11.0), Color32::WHITE).size().x)
    };
    36.0 + lw + sw + 10.0
}

/// Constrains a menu's width to fit its widest menu entry plus 10 pixels.
pub fn fit_menu(ui: &mut egui::Ui, entries: &[(&str, &str)]) -> f32 {
    let widest = entries
        .iter()
        .map(|(l, sc)| entry_width(ui, l, sc))
        .fold(0.0f32, f32::max);
    let w = (widest + 10.0).ceil();
    ui.set_width(w);
    w
}

/// Studio menu item with rounded icon badge, crisp label, and right-aligned shortcut pill.
fn mitem_en(ui: &mut egui::Ui, th: &Theme, enabled: bool, icon: Option<Icon>, label: &str, sc: &str) -> bool {
    let item_w = (entry_width(ui, label, sc) + 10.0).ceil();
    let max_w = ui.max_rect().width();
    let w = if max_w.is_finite() && max_w < 500.0 {
        max_w.max(item_w)
    } else {
        item_w
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    let label_owned = label.to_string();
    resp.widget_info(move || egui::WidgetInfo::labeled(egui::Role::MenuItem, enabled, label_owned.clone()));
    let p = ui.painter();
    let hovered = resp.hovered() && enabled;
    if hovered {
        p.rect_filled(rect, 7.0, th.hover);
    }
    let fg = if enabled { th.text } else { th.subtle.gamma_multiply(0.50) };
    let ic_col = if !enabled {
        th.icon.gamma_multiply(0.38)
    } else if hovered {
        th.accent
    } else {
        th.icon
    };
    let acc_col = if enabled { th.accent } else { th.accent.gamma_multiply(0.38) };
    if let Some(ic) = icon {
        let badge = egui::Rect::from_center_size(pos2(rect.min.x + 15.0, rect.center().y), vec2(20.0, 20.0));
        p.rect_filled(
            badge,
            5.0,
            if hovered { th.accent_soft } else { th.surface_subtle },
        );
        icons::draw(p, badge.shrink(2.5), ic, ic_col, acc_col);
    }
    p.text(pos2(rect.min.x + 32.0, rect.center().y), egui::Align2::LEFT_CENTER, label, FontId::proportional(13.0), fg);
    if !sc.is_empty() {
        let sc_galley = p.layout_no_wrap(sc.to_string(), FontId::proportional(11.0), th.subtle);
        let sc_w = sc_galley.size().x;
        let pill_r = egui::Rect::from_min_max(
            pos2(rect.max.x - sc_w - 15.0, rect.center().y - 9.0),
            pos2(rect.max.x - 5.0, rect.center().y + 9.0),
        );
        if sc.len() > 1 {
            p.rect_filled(pill_r, 4.0, if hovered { th.surface_elevated } else { th.surface_subtle });
        }
        p.galley(
            pos2(pill_r.center().x - sc_w * 0.5, rect.center().y - sc_galley.size().y * 0.5),
            sc_galley,
            th.subtle,
        );
    }
    let clicked = enabled && resp.clicked();
    if clicked {
        ui.close();
    }
    clicked
}

fn mitem(ui: &mut egui::Ui, th: &Theme, icon: Icon, label: &str, sc: &str) -> bool {
    mitem_en(ui, th, true, Some(icon), label, sc)
}

fn mitem_plain(ui: &mut egui::Ui, th: &Theme, label: &str, sc: &str) -> bool {
    mitem_en(ui, th, true, None, label, sc)
}

fn mcheck(ui: &mut egui::Ui, th: &Theme, checked: bool, label: &str, sc: &str) -> bool {
    mitem_en(ui, th, true, if checked { Some(Icon::Check) } else { None }, label, sc)
}

impl App {
    /// Unified Studio Header: Scribe crest, inline editable document title, star, status pill,
    /// integrated document tab pills, command search trigger, history, comments, mode switcher, and theme pill.
    pub fn docs_header(&mut self, ui: &mut egui::Ui, th: &Theme) {
        let starred = self.tab_ref().doc.starred;
        let dirty = self.tab_ref().dirty;
        let pageless = self.tab_ref().doc.pageless;
        let open_comments = self.tab_ref().doc.comments.iter().filter(|c| !c.resolved).count();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (logo_r, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
            icons::draw(ui.painter(), logo_r, Icon::DocsLogo, th.icon, th.accent);

            if self.editing_title {
                let mut name = self.tab_ref().name.clone();
                let te = ui.add(egui::TextEdit::singleline(&mut name).desired_width(200.0).font(FontId::proportional(15.0)));
                if te.changed() && !name.trim().is_empty() {
                    let t = self.tab();
                    t.name = name.trim().to_string();
                    t.dirty = true;
                }
                if te.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)) {
                    self.editing_title = false;
                }
            } else {
                let active_name = self.tab_ref().name.clone();
                let title_txt = RichText::new(&active_name).size(15.0).color(th.text).strong();
                let btn = ui
                    .add(egui::Button::new(title_txt).frame(false))
                    .on_hover_text("Click to rename document");
                ui.interact(btn.rect, egui::Id::unique("doc-title-a11y"), Sense::hover())
                    .widget_info(move || egui::WidgetInfo::labeled(egui::Role::TitleBar, true, active_name.clone()));
                if btn.clicked() {
                    self.editing_title = true;
                }
            }

            let star_ic = if starred { Icon::StarFilled } else { Icon::Star };
            if icon_button(ui, th, star_ic, if starred { "Starred document (click to unstar)" } else { "Star this document" }, false, true).clicked() {
                self.act(Action::ToggleStar);
            }

            // Status pill badge
            let (status_txt, status_col, status_bg) = if dirty {
                ("Edited", th.accent, th.accent_soft)
            } else {
                ("Saved", th.subtle, th.surface_subtle)
            };
            Frame::new()
                .fill(status_bg)
                .corner_radius(10)
                .inner_margin(Margin::symmetric(8, 2))
                .show(ui, |ui| {
                    ui.label(RichText::new(status_txt).size(11.0).color(status_col));
                });

            if pageless {
                Frame::new()
                    .fill(th.accent_soft)
                    .corner_radius(10)
                    .inner_margin(Margin::symmetric(8, 2))
                    .show(ui, |ui| {
                        ui.label(RichText::new("Pageless").size(11.0).color(th.accent).strong());
                    });
            }

            // Integrated Document Tabs & New Document button right in the header
            sep(ui, th);
            let mut close_tab = None;
            let mut select_tab = None;
            if self.tabs.len() > 1 {
                for (i, t) in self.tabs.iter().enumerate() {
                    let active = i == self.active;
                    let fill = if active { th.surface_elevated } else { th.surface_subtle };
                    let stroke = if active {
                        Stroke::new(1.0, th.accent.gamma_multiply(0.45))
                    } else {
                        Stroke::new(1.0, th.border)
                    };
                    Frame::new()
                        .fill(fill)
                        .stroke(stroke)
                        .corner_radius(8)
                        .inner_margin(Margin { left: 9, right: 5, top: 3, bottom: 3 })
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                let txt = RichText::new(t.title()).size(12.0).color(if active { th.text } else { th.subtle });
                                if ui.add(egui::Label::new(txt).sense(Sense::click())).clicked() {
                                    select_tab = Some(i);
                                }
                                let (r, cr) = ui.allocate_exact_size(vec2(15.0, 15.0), Sense::click());
                                if cr.hovered() {
                                    ui.painter().rect_filled(r, 4.0, th.hover);
                                }
                                icons::draw(ui.painter(), r.shrink(3.5), Icon::Close, th.subtle, th.accent);
                                if cr.on_hover_text("Close tab").clicked() {
                                    close_tab = Some(i);
                                }
                            });
                        });
                }
            }
            if icon_button(ui, th, Icon::New, "New document tab (Ctrl+N)", false, true).clicked() {
                self.act(Action::New);
            }
            if let Some(i) = select_tab {
                self.act(Action::SelectTab(i));
            }
            if let Some(i) = close_tab {
                self.act(Action::Close(i));
            }

            // Sculpted Studio Menu Capsule integrated directly into the header bar
            ui.add_space(4.0);
            Frame::new()
                .fill(th.surface_subtle)
                .stroke(Stroke::new(1.0, th.border))
                .corner_radius(9)
                .inner_margin(Margin::symmetric(6, 2))
                .show(ui, |ui| {
                    let r = egui::MenuBar::new().ui(ui, |ui| {
                        self.menus(ui, th);
                    });
                    // the original's menu bar is named "Application"
                    ui.interact(r.response.rect, egui::Id::unique("menubar-a11y"), Sense::hover())
                        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::MenuBar, true, "Application"));
                });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                // Tactile Light / Dark Theme Capsule
                let (theme_ic, theme_lbl) = if self.cfg.dark {
                    (Icon::Sun, "Light")
                } else {
                    (Icon::Moon, "Dark")
                };
                let (t_rect, t_resp) = ui.allocate_exact_size(vec2(66.0, 26.0), Sense::click());
                let t_bg = if t_resp.hovered() { th.hover } else { th.surface_subtle };
                ui.painter().rect_filled(t_rect, 13.0, t_bg);
                ui.painter().rect_stroke(t_rect, 13.0, Stroke::new(1.0, th.border), egui::StrokeKind::Inside);
                let ic_r = egui::Rect::from_center_size(pos2(t_rect.min.x + 14.0, t_rect.center().y), vec2(14.0, 14.0));
                icons::draw(ui.painter(), ic_r, theme_ic, th.icon, th.accent);
                ui.painter().text(
                    pos2(t_rect.min.x + 25.0, t_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    theme_lbl,
                    FontId::proportional(12.0),
                    th.text,
                );
                if t_resp.on_hover_text("Switch between Light and Dark studio themes").clicked() {
                    self.act(Action::ToggleDark);
                }

                // Mode selector pill (Editing / Suggesting / Viewing)
                let mode_r = combo_box_button(ui, th, 96.0, self.mode.label(), FontId::proportional(12.5), "Document mode (Editing / Suggesting / Viewing)");
                egui::Popup::menu(&mode_r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    fit_menu(
                        ui,
                        &[
                            ("Editing — Edit document directly", "✓"),
                            ("Suggesting — Edits become suggestions", "✓"),
                            ("Viewing — Read or print final document", "✓"),
                        ],
                    );
                    for (m, desc) in [
                        (DocMode::Editing, "Edit document directly"),
                        (DocMode::Suggesting, "Edits become suggestions"),
                        (DocMode::Viewing, "Read or print final document"),
                    ] {
                        if mitem(ui, th, m.icon(), &format!("{} — {}", m.label(), desc), if self.mode == m { "✓" } else { "" }) {
                            self.act(Action::SetMode(m));
                        }
                    }
                });

                let cm_tip = if open_comments > 0 {
                    format!("Comments & suggestions ({open_comments} open)")
                } else {
                    "Open comment history (Ctrl+Alt+M)".into()
                };
                if icon_button(ui, th, Icon::Comment, &cm_tip, self.show_comments, true).clicked() {
                    self.act(Action::ToggleComments);
                }
                if open_comments > 0 {
                    ui.label(RichText::new(format!("{open_comments}")).size(11.5).color(th.accent).strong());
                }

                if icon_button(ui, th, Icon::History, "Version history (Ctrl+Alt+Shift+H)", false, true).clicked() {
                    self.act(Action::Dialog(Dialog::VersionHistory(0, String::new())));
                }

                // Quick Command Search Pill (Alt+/)
                let (s_rect, s_resp) = ui.allocate_exact_size(vec2(158.0, 26.0), Sense::click());
                let s_bg = if s_resp.hovered() { th.hover } else { th.surface_subtle };
                ui.painter().rect_filled(s_rect, 13.0, s_bg);
                ui.painter().rect_stroke(s_rect, 13.0, Stroke::new(1.0, th.border), egui::StrokeKind::Inside);
                let s_ic = egui::Rect::from_center_size(pos2(s_rect.min.x + 14.0, s_rect.center().y), vec2(13.5, 13.5));
                icons::draw(ui.painter(), s_ic, Icon::SearchMenus, th.subtle, th.accent);
                ui.painter().text(
                    pos2(s_rect.min.x + 26.0, s_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "Commands…",
                    FontId::proportional(12.0),
                    th.subtle,
                );
                ui.painter().text(
                    pos2(s_rect.max.x - 10.0, s_rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    "Alt+/",
                    FontId::proportional(10.5),
                    th.subtle.gamma_multiply(0.85),
                );
                if s_resp.on_hover_text("Search all commands and menus (Alt+/)").clicked() {
                    self.act(Action::Dialog(Dialog::SearchMenus(String::new())));
                }
            });
        });
    }

    pub fn menus(&mut self, ui: &mut egui::Ui, th: &Theme) {
        let has_sel = self.tab_ref().has_selection();
        let can_undo = self.tab_ref().can_undo();
        let can_redo = self.tab_ref().can_redo();
        let pageless = self.tab_ref().doc.pageless;
        let kp = self.cfg.shortcut_preset;

        // Style top-level menu triggers as borderless studio pills
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.spacing_mut().button_padding = vec2(9.0, 4.0);
        ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        ui.visuals_mut().widgets.inactive.bg_fill = Color32::TRANSPARENT;
        ui.visuals_mut().widgets.hovered.weak_bg_fill = th.hover;
        ui.visuals_mut().widgets.hovered.bg_fill = th.hover;
        ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::NONE;
        ui.visuals_mut().widgets.open.weak_bg_fill = th.accent_soft;
        ui.visuals_mut().widgets.open.bg_fill = th.accent_soft;
        ui.visuals_mut().widgets.open.bg_stroke = Stroke::new(1.0, th.accent.gamma_multiply(0.35));

        // ── FILE ──
        ui.menu_button("File", |ui| {
            fit_menu(
                ui,
                &[
                    ("New", "▸"),
                    ("Open…", "Ctrl+O"),
                    ("Make a copy", ""),
                    ("Open recent", "▸"),
                    ("Save", "Ctrl+S"),
                    ("Save As…", kp.save_as()),
                    ("Download", "▸"),
                    ("Rename…", ""),
                    ("Close tab", kp.close_tab()),
                    ("Version history", "▸"),
                    ("Details…", ""),
                    ("Page setup…", ""),
                    ("Print…", "Ctrl+P"),
                    ("Exit", "Alt+F4"),
                ],
            );
            ui.menu_button("New", |ui| {
                fit_menu(
                    ui,
                    &[
                        ("Document", "Ctrl+N"),
                        ("From a template / building block", ""),
                        ("Meeting notes", ""),
                        ("Project roadmap", ""),
                        ("Email draft", ""),
                        ("Launch checklist", ""),
                    ],
                );
                if mitem(ui, th, Icon::New, "Document", "Ctrl+N") {
                    self.act(Action::New);
                }
                ui.separator();
                ui.label(RichText::new("From a template / building block").small().weak());
                if mitem(ui, th, Icon::BuildingBlock, "Meeting notes", "") {
                    self.act(Action::New);
                    self.act(Action::InsertBuildingBlock("meeting"));
                }
                if mitem(ui, th, Icon::BuildingBlock, "Project roadmap", "") {
                    self.act(Action::New);
                    self.act(Action::InsertBuildingBlock("roadmap"));
                }
                if mitem(ui, th, Icon::BuildingBlock, "Email draft", "") {
                    self.act(Action::New);
                    self.act(Action::InsertBuildingBlock("email"));
                }
                if mitem(ui, th, Icon::Checklist, "Launch checklist", "") {
                    self.act(Action::New);
                    self.act(Action::InsertBuildingBlock("checklist"));
                }
            });
            if mitem(ui, th, Icon::Open, "Open…", "Ctrl+O") {
                self.act(Action::Open);
            }
            if mitem(ui, th, Icon::CopyDoc, "Make a copy", "") {
                self.act(Action::MakeCopy);
            }
            if !self.cfg.recent.is_empty() {
                ui.menu_button("Open recent", |ui| {
                    let recent_items: Vec<(std::path::PathBuf, String)> = self
                        .cfg
                        .recent
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            (p.clone(), format!("{}  {}", i + 1, name))
                        })
                        .collect();
                    let recent_refs: Vec<(&str, &str)> = recent_items.iter().map(|(_, lbl)| (lbl.as_str(), "")).collect();
                    fit_menu(ui, &recent_refs);
                    for (p, lbl) in recent_items {
                        if mitem(ui, th, Icon::Open, &lbl, "") {
                            self.act(Action::OpenPath(p));
                        }
                    }
                });
            }
            ui.separator();
            if mitem(ui, th, Icon::Save, "Save", "Ctrl+S") {
                self.act(Action::Save);
            }
            if mitem(ui, th, Icon::Save, "Save As…", kp.save_as()) {
                self.act(Action::SaveAs);
            }
            ui.menu_button("Download", |ui| {
                fit_menu(
                    ui,
                    &[
                        ("Google Docs (.gdoc)", ""),
                        ("Microsoft Word (.docx)", ""),
                        ("PDF Document (.pdf)", ""),
                        ("Rich Text Format (.rtf)", ""),
                        ("Plain Text (.txt)", ""),
                        ("Markdown (.md)", ""),
                        ("Web Page (.html)", ""),
                    ],
                );
                if mitem(ui, th, Icon::New, "Google Docs (.gdoc)", "") {
                    self.act(Action::ExportGdoc);
                }
                if mitem(ui, th, Icon::New, "Microsoft Word (.docx)", "") {
                    self.act(Action::ExportDocx);
                }
                if mitem(ui, th, Icon::Pdf, "PDF Document (.pdf)", "") {
                    self.act(Action::ExportPdf);
                }
                if mitem(ui, th, Icon::New, "Rich Text Format (.rtf)", "") {
                    self.act(Action::ExportRtf);
                }
                if mitem(ui, th, Icon::New, "Plain Text (.txt)", "") {
                    self.act(Action::ExportTxt);
                }
                if mitem(ui, th, Icon::New, "Markdown (.md)", "") {
                    self.act(Action::ExportMd);
                }
                if mitem(ui, th, Icon::New, "Web Page (.html)", "") {
                    self.act(Action::ExportHtml);
                }
            });
            ui.separator();
            if mitem(ui, th, Icon::EditMode, "Rename…", "") {
                let cur_name = self.tab_ref().name.clone();
                self.act(Action::Dialog(Dialog::Rename(cur_name)));
            }
            if mitem(ui, th, Icon::Close, "Close tab", kp.close_tab()) {
                let i = self.active;
                self.act(Action::Close(i));
            }
            ui.separator();
            ui.menu_button("Version history", |ui| {
                fit_menu(
                    ui,
                    &[
                        ("Name current version…", ""),
                        ("See version history", "Ctrl+Alt+Shift+H"),
                    ],
                );
                if mitem(ui, th, Icon::EditMode, "Name current version…", "") {
                    self.act(Action::Dialog(Dialog::VersionHistory(0, "Milestone".into())));
                }
                if mitem(ui, th, Icon::History, "See version history", "Ctrl+Alt+Shift+H") {
                    self.act(Action::Dialog(Dialog::VersionHistory(0, String::new())));
                }
            });
            ui.separator();
            if mitem(ui, th, Icon::WordCount, "Details…", "") {
                self.act(Action::Dialog(Dialog::DocProps));
            }
            if mitem(ui, th, Icon::PageSetup, "Page setup…", "") {
                self.act(Action::Dialog(Dialog::PageSetup));
            }
            if mitem(ui, th, Icon::Print, "Print…", "Ctrl+P") {
                self.act(Action::Print);
            }
            ui.separator();
            if mitem(ui, th, Icon::Close, "Exit", "Alt+F4") {
                self.act(Action::Quit);
            }
        });

        // ── EDIT ──
        ui.menu_button("Edit", |ui| {
            fit_menu(
                ui,
                &[
                    ("Undo", "Ctrl+Z"),
                    ("Redo", "Ctrl+Y"),
                    ("Cut", "Ctrl+X"),
                    ("Copy", "Ctrl+C"),
                    ("Paste", "Ctrl+V"),
                    ("Paste without formatting", kp.paste_plain()),
                    ("Select all", "Ctrl+A"),
                    ("Duplicate paragraph", "Ctrl+D"),
                    ("Find and replace", kp.replace()),
                    ("Find", "Ctrl+F"),
                    ("Go to page…", "Ctrl+G"),
                ],
            );
            if mitem_en(ui, th, can_undo, Some(Icon::Undo), "Undo", "Ctrl+Z") {
                self.act(Action::Undo);
            }
            if mitem_en(ui, th, can_redo, Some(Icon::Redo), "Redo", "Ctrl+Y") {
                self.act(Action::Redo);
            }
            ui.separator();
            if mitem_en(ui, th, has_sel, Some(Icon::Cut), "Cut", "Ctrl+X") {
                self.act(Action::Cut);
            }
            if mitem_en(ui, th, has_sel, Some(Icon::Copy), "Copy", "Ctrl+C") {
                self.act(Action::Copy);
            }
            if mitem(ui, th, Icon::Paste, "Paste", "Ctrl+V") {
                self.act(Action::Paste);
            }
            if mitem(ui, th, Icon::Paste, "Paste without formatting", kp.paste_plain()) {
                self.act(Action::PastePlain);
            }
            ui.separator();
            if mitem(ui, th, Icon::Check, "Select all", "Ctrl+A") {
                self.act(Action::SelectAll);
            }
            if mitem(ui, th, Icon::CopyDoc, "Duplicate paragraph", "Ctrl+D") {
                self.act(Action::DuplicatePara);
            }
            ui.separator();
            if mitem(ui, th, Icon::Find, "Find and replace", kp.replace()) {
                self.act(Action::Dialog(Dialog::Find(true)));
            }
            if mitem(ui, th, Icon::Find, "Find", "Ctrl+F") {
                self.act(Action::Dialog(Dialog::Find(false)));
            }
            if mitem(ui, th, Icon::PageSetup, "Go to page…", "Ctrl+G") {
                let p = self.tab_ref().current_page + 1;
                self.act(Action::Dialog(Dialog::GoTo(p)));
            }
        });

        // ── VIEW ──
        ui.menu_button("View", |ui| {
            let theme_lbl = if self.cfg.dark { "Light theme" } else { "Dark theme" };
            fit_menu(
                ui,
                &[
                    ("Mode", "▸"),
                    ("Pageless", ""),
                    ("Show print layout", ""),
                    ("Show ruler", ""),
                    ("Show outline", "Ctrl+Alt+A"),
                    ("Show comments", ""),
                    ("Show non-printing characters (¶)", ""),
                    ("Show margin guides", ""),
                    ("Show headers & footers", ""),
                    ("Show status bar", ""),
                    ("Zoom", "▸"),
                    ("Compact controls", "Ctrl+Shift+F"),
                    ("Full screen", ""),
                    (theme_lbl, ""),
                    ("UI & Icon Design Sheet…", ""),
                ],
            );
            ui.menu_button("Mode", |ui| {
                fit_menu(ui, &[("Editing", "✓"), ("Suggesting", "✓"), ("Viewing", "✓")]);
                for m in [DocMode::Editing, DocMode::Suggesting, DocMode::Viewing] {
                    if mitem(ui, th, m.icon(), m.label(), if self.mode == m { "✓" } else { "" }) {
                        self.act(Action::SetMode(m));
                    }
                }
            });
            ui.separator();
            if mcheck(ui, th, pageless, "Pageless", "") {
                self.act(Action::TogglePageless);
            }
            if mcheck(ui, th, !self.draft && !pageless, "Show print layout", "") {
                if pageless {
                    self.act(Action::TogglePageless);
                }
                self.act(Action::Draft(false));
            }
            if mcheck(ui, th, self.show_ruler, "Show ruler", "") {
                self.show_ruler = !self.show_ruler;
            }
            if mcheck(ui, th, self.show_nav, "Show outline", "Ctrl+Alt+A") {
                self.act(Action::ToggleNav);
            }
            if mcheck(ui, th, self.show_comments, "Show comments", "") {
                self.act(Action::ToggleComments);
            }
            if mcheck(ui, th, self.show_marks, "Show non-printing characters (¶)", "") {
                self.show_marks = !self.show_marks;
            }
            if mcheck(ui, th, self.cfg.margin_guides, "Show margin guides", "") {
                self.cfg.margin_guides = !self.cfg.margin_guides;
                self.save_config();
            }
            if mcheck(ui, th, self.show_hf, "Show headers & footers", "") {
                self.show_hf = !self.show_hf;
            }
            if mcheck(ui, th, self.show_status, "Show status bar", "") {
                self.show_status = !self.show_status;
            }
            ui.separator();
            ui.menu_button("Zoom", |ui| {
                fit_menu(
                    ui,
                    &[
                        ("Fit (Page width)", ""),
                        ("Margin width", ""),
                        ("Full page", ""),
                        ("100%", "Ctrl+0"),
                        ("Zoom in", "Ctrl+="),
                        ("Zoom out", "Ctrl+-"),
                    ],
                );
                if mcheck(ui, th, self.zoom_mode == ZoomMode::PageWidth, "Fit (Page width)", "") {
                    self.act(Action::ZoomMode(ZoomMode::PageWidth));
                }
                if mcheck(ui, th, self.zoom_mode == ZoomMode::MarginWidth, "Margin width", "") {
                    self.act(Action::ZoomMode(ZoomMode::MarginWidth));
                }
                if mcheck(ui, th, self.zoom_mode == ZoomMode::FullPage, "Full page", "") {
                    self.act(Action::ZoomMode(ZoomMode::FullPage));
                }
                ui.separator();
                for z in [50, 75, 90, 100, 125, 150, 200] {
                    let sel = self.zoom_mode == ZoomMode::Percent && (self.zoom * 100.0).round() as i32 == z;
                    if mcheck(ui, th, sel, &format!("{z}%"), if z == 100 { "Ctrl+0" } else { "" }) {
                        self.act(Action::SetZoom(z as f32 / 100.0));
                    }
                }
                ui.separator();
                if mitem(ui, th, Icon::ZoomIn, "Zoom in", "Ctrl+=") {
                    self.act(Action::ZoomIn);
                }
                if mitem(ui, th, Icon::ZoomOut, "Zoom out", "Ctrl+-") {
                    self.act(Action::ZoomOut);
                }
            });
            if mcheck(ui, th, self.compact_mode, "Compact controls", "Ctrl+Shift+F") {
                self.act(Action::ToggleCompact);
            }
            if mcheck(ui, th, self.clean_screen, "Full screen", "") {
                self.clean_screen = !self.clean_screen;
            }
            ui.separator();
            let theme_ic = if self.cfg.dark { Icon::Sun } else { Icon::Moon };
            if mitem(ui, th, theme_ic, theme_lbl, "") {
                self.act(Action::ToggleDark);
            }
            if mitem(ui, th, Icon::Painter, "UI & Icon Design Sheet…", "") {
                self.act(Action::Dialog(Dialog::DesignSheet));
            }
        });

        // ── INSERT (Google Docs Insert menu!) ──
        ui.menu_button("Insert", |ui| {
            fit_menu(
                ui,
                &[
                    ("Table", "▸"),
                    ("Building blocks", "▸"),
                    ("Smart chips", "▸"),
                    ("Horizontal line", ""),
                    ("Footnote", "Ctrl+Alt+F"),
                    ("Emoji (System Library)", ""),
                    ("Special characters & emoji…", ""),
                    ("Date & time…", ""),
                    ("Link", "Ctrl+K"),
                    ("Comment", "Ctrl+Alt+M"),
                    ("Bookmark…", ""),
                    ("Page break", "Ctrl+Enter"),
                    ("Headers & footers", "▸"),
                    ("Page numbers", "▸"),
                    ("Table of contents", ""),
                ],
            );
            ui.menu_button("Table", |ui| {
                fit_menu(ui, &[("2 × 2 Table", ""), ("3 × 3 Tracker Table", "")]);
                if mitem(ui, th, Icon::Table, "2 × 2 Table", "") {
                    self.act(Action::InsertBuildingBlock("table_2x2"));
                }
                if mitem(ui, th, Icon::Table, "3 × 3 Tracker Table", "") {
                    self.act(Action::InsertBuildingBlock("table_3x3"));
                }
            });
            ui.menu_button("Building blocks", |ui| {
                fit_menu(
                    ui,
                    &[
                        ("Meeting notes", "@meeting"),
                        ("Email draft", "@email"),
                        ("Project roadmap", "@roadmap"),
                        ("Launch checklist", "@checklist"),
                        ("Code block", "@code"),
                    ],
                );
                if mitem(ui, th, Icon::BuildingBlock, "Meeting notes", "@meeting") {
                    self.act(Action::InsertBuildingBlock("meeting"));
                }
                if mitem(ui, th, Icon::BuildingBlock, "Email draft", "@email") {
                    self.act(Action::InsertBuildingBlock("email"));
                }
                if mitem(ui, th, Icon::BuildingBlock, "Project roadmap", "@roadmap") {
                    self.act(Action::InsertBuildingBlock("roadmap"));
                }
                if mitem(ui, th, Icon::Checklist, "Launch checklist", "@checklist") {
                    self.act(Action::InsertBuildingBlock("checklist"));
                }
                if mitem(ui, th, Icon::BuildingBlock, "Code block", "@code") {
                    self.act(Action::InsertBuildingBlock("code"));
                }
            });
            ui.menu_button("Smart chips", |ui| {
                fit_menu(
                    ui,
                    &[
                        ("Today's date", "@today"),
                        ("Tomorrow's date", "@tomorrow"),
                        ("Person chip", "@person"),
                        ("Dropdown status chips", ""),
                        ("⚪ Status: Not started", ""),
                        ("🔵 Status: In progress", ""),
                        ("🟡 Status: Under review", ""),
                        ("🟢 Status: Approved", ""),
                        ("🔥 Priority: High", ""),
                        ("⏱ Stopwatch / Focus timer", ""),
                    ],
                );
                if mitem(ui, th, Icon::SmartChip, "Today's date", "@today") {
                    self.act(Action::InsertSmartChip("today"));
                }
                if mitem(ui, th, Icon::SmartChip, "Tomorrow's date", "@tomorrow") {
                    self.act(Action::InsertSmartChip("tomorrow"));
                }
                if mitem(ui, th, Icon::SmartChip, "Person chip", "@person") {
                    self.act(Action::InsertSmartChip("person"));
                }
                ui.separator();
                ui.label(RichText::new("Dropdown status chips").small().weak());
                if mitem_plain(ui, th, "⚪ Status: Not started", "") {
                    self.act(Action::InsertSmartChip("status_not_started"));
                }
                if mitem_plain(ui, th, "🔵 Status: In progress", "") {
                    self.act(Action::InsertSmartChip("status_in_progress"));
                }
                if mitem_plain(ui, th, "🟡 Status: Under review", "") {
                    self.act(Action::InsertSmartChip("status_review"));
                }
                if mitem_plain(ui, th, "🟢 Status: Approved", "") {
                    self.act(Action::InsertSmartChip("status_approved"));
                }
                ui.separator();
                if mitem_plain(ui, th, "🔥 Priority: High", "") {
                    self.act(Action::InsertSmartChip("priority_high"));
                }
                if mitem_plain(ui, th, "⏱ Stopwatch / Focus timer", "") {
                    self.act(Action::InsertSmartChip("timer"));
                }
            });
            ui.separator();
            if mitem(ui, th, Icon::HorizontalRule, "Horizontal line", "") {
                self.act(Action::HorizontalRule);
            }
            if mitem(ui, th, Icon::Footnote, "Footnote", "Ctrl+Alt+F") {
                self.act(Action::InsertFootnote);
            }
            if mitem(ui, th, Icon::Symbol, "Emoji (System Library)", "") {
                self.act(Action::SystemEmojiPicker);
            }
            if mitem(ui, th, Icon::Symbol, "Special characters & emoji…", "") {
                self.act(Action::Dialog(Dialog::Symbol));
            }
            if mitem(ui, th, Icon::History, "Date & time…", "") {
                self.act(Action::Dialog(Dialog::DateTime));
            }
            ui.separator();
            if mitem(ui, th, Icon::Link, "Link", "Ctrl+K") {
                let txt = self.tab_ref().selected_text();
                self.act(Action::Dialog(Dialog::Link { text: txt, url: String::new() }));
            }
            if mitem(ui, th, Icon::Comment, "Comment", "Ctrl+Alt+M") {
                self.act(Action::Dialog(Dialog::Comment(String::new())));
            }
            if mitem(ui, th, Icon::Bookmark, "Bookmark…", "") {
                self.act(Action::Dialog(Dialog::Bookmark(String::new())));
            }
            ui.separator();
            if mitem(ui, th, Icon::PageBreak, "Page break", "Ctrl+Enter") {
                self.act(Action::PageBreak);
            }
            ui.menu_button("Headers & footers", |ui| {
                fit_menu(ui, &[("Header…", ""), ("Footer…", "")]);
                if mitem(ui, th, Icon::PageSetup, "Header…", "") {
                    self.act(Action::Dialog(Dialog::PageSetup));
                }
                if mitem(ui, th, Icon::PageSetup, "Footer…", "") {
                    self.act(Action::Dialog(Dialog::PageSetup));
                }
            });
            ui.menu_button("Page numbers", |ui| {
                fit_menu(ui, &[("Page numbers in footer", ""), ("Page setup…", "")]);
                let pn = self.tab_ref().doc.page.page_numbers;
                if mcheck(ui, th, pn, "Page numbers in footer", "") {
                    self.act(Action::PageEdit(PageEdit::Numbers(!pn)));
                }
                if mitem(ui, th, Icon::PageSetup, "Page setup…", "") {
                    self.act(Action::Dialog(Dialog::PageSetup));
                }
            });
            if mitem(ui, th, Icon::NavPane, "Table of contents", "") {
                self.act(Action::InsertToc);
            }
        });

        // ── FORMAT (Google Docs Format menu!) ──
        ui.menu_button("Format", |ui| {
            let pageless_lbl = if pageless { "Switch to Pages format" } else { "Switch to Pageless format" };
            fit_menu(
                ui,
                &[
                    ("Text", "▸"),
                    ("Paragraph styles", "▸"),
                    ("Align & indent", "▸"),
                    ("Line & paragraph spacing", "▸"),
                    ("Bullets & numbering", "▸"),
                    ("Headers & footers…", ""),
                    ("Page orientation", "▸"),
                    (pageless_lbl, ""),
                    ("Paint format", ""),
                    ("Clear formatting", kp.clear_format()),
                ],
            );
            ui.menu_button("Text", |ui| {
                fit_menu(
                    ui,
                    &[
                        ("Bold", "Ctrl+B"),
                        ("Italic", "Ctrl+I"),
                        ("Underline", "Ctrl+U"),
                        ("Strikethrough", kp.strikethrough()),
                        ("Superscript", kp.superscript()),
                        ("Subscript", kp.subscript()),
                        ("Size", "▸"),
                        ("Capitalization", "▸"),
                        ("Font & color…", ""),
                    ],
                );
                if mitem(ui, th, Icon::Bold, "Bold", "Ctrl+B") {
                    self.act(Action::Bold);
                }
                if mitem(ui, th, Icon::Italic, "Italic", "Ctrl+I") {
                    self.act(Action::Italic);
                }
                if mitem(ui, th, Icon::Underline, "Underline", "Ctrl+U") {
                    self.act(Action::Underline);
                }
                if mitem(ui, th, Icon::Strike, "Strikethrough", kp.strikethrough()) {
                    self.act(Action::Strike);
                }
                if mitem(ui, th, Icon::Superscript, "Superscript", kp.superscript()) {
                    self.act(Action::Script(1));
                }
                if mitem(ui, th, Icon::Subscript, "Subscript", kp.subscript()) {
                    self.act(Action::Script(-1));
                }
                ui.separator();
                ui.menu_button("Size", |ui| {
                    fit_menu(
                        ui,
                        &[
                            ("Increase font size", kp.grow_font()),
                            ("Decrease font size", kp.shrink_font()),
                        ],
                    );
                    if mitem(ui, th, Icon::Plus, "Increase font size", kp.grow_font()) {
                        self.act(Action::Grow);
                    }
                    if mitem(ui, th, Icon::Minus, "Decrease font size", kp.shrink_font()) {
                        self.act(Action::Shrink);
                    }
                });
                ui.menu_button("Capitalization", |ui| {
                    fit_menu(
                        ui,
                        &[
                            ("lowercase", ""),
                            ("UPPERCASE", ""),
                            ("Title Case", ""),
                            ("Sentence case", ""),
                        ],
                    );
                    if mitem_plain(ui, th, "lowercase", "") {
                        self.act(Action::Case(CaseChange::Lower));
                    }
                    if mitem_plain(ui, th, "UPPERCASE", "") {
                        self.act(Action::Case(CaseChange::Upper));
                    }
                    if mitem_plain(ui, th, "Title Case", "") {
                        self.act(Action::Case(CaseChange::Title));
                    }
                    if mitem_plain(ui, th, "Sentence case", "") {
                        self.act(Action::Case(CaseChange::Sentence));
                    }
                });
                if mitem(ui, th, Icon::TextColor, "Font & color…", "") {
                    self.act(Action::Dialog(Dialog::Font));
                }
            });

            ui.menu_button("Paragraph styles", |ui| {
                fit_menu(
                    ui,
                    &[
                        ("Default Text", kp.style_normal()),
                        ("Heading 1", kp.style_h1()),
                        ("Heading 2", kp.style_h2()),
                        ("Heading 3", kp.style_h3()),
                        ("Number List", ""),
                        ("Borders and shading", "▸"),
                    ],
                );
                for (i, s) in named_styles().iter().enumerate() {
                    let sc = match s.name {
                        "Default Text" => kp.style_normal(),
                        "Heading 1" => kp.style_h1(),
                        "Heading 2" => kp.style_h2(),
                        "Heading 3" => kp.style_h3(),
                        _ => "",
                    };
                    if mitem_plain(ui, th, s.name, sc) {
                        self.act(Action::Style(i));
                    }
                }
                ui.separator();
                ui.menu_button("Borders and shading", |ui| {
                    fit_menu(
                        ui,
                        &[
                            ("Soft gray callout box", ""),
                            ("Soft blue note box", ""),
                            ("Soft amber warning box", ""),
                            ("Remove paragraph shading", ""),
                        ],
                    );
                    if mitem_plain(ui, th, "Soft gray callout box", "") {
                        self.act(Action::ParaShading(Some([241, 245, 249])));
                    }
                    if mitem_plain(ui, th, "Soft blue note box", "") {
                        self.act(Action::ParaShading(Some([239, 246, 255])));
                    }
                    if mitem_plain(ui, th, "Soft amber warning box", "") {
                        self.act(Action::ParaShading(Some([254, 252, 232])));
                    }
                    if mitem_plain(ui, th, "Remove paragraph shading", "") {
                        self.act(Action::ParaShading(None));
                    }
                });
            });

            ui.menu_button("Align & indent", |ui| {
                fit_menu(
                    ui,
                    &[
                        ("Left", kp.align_left()),
                        ("Center", kp.align_center()),
                        ("Right", kp.align_right()),
                        ("Justified", kp.align_justify()),
                        ("Increase indent", kp.indent()),
                        ("Decrease indent", kp.outdent()),
                    ],
                );
                if mitem(ui, th, Icon::AlignLeft, "Left", kp.align_left()) {
                    self.act(Action::Align(Align::Left));
                }
                if mitem(ui, th, Icon::AlignCenter, "Center", kp.align_center()) {
                    self.act(Action::Align(Align::Center));
                }
                if mitem(ui, th, Icon::AlignRight, "Right", kp.align_right()) {
                    self.act(Action::Align(Align::Right));
                }
                if mitem(ui, th, Icon::AlignJustify, "Justified", kp.align_justify()) {
                    self.act(Action::Align(Align::Justify));
                }
                ui.separator();
                if mitem(ui, th, Icon::Indent, "Increase indent", kp.indent()) {
                    self.act(Action::Indent(1));
                }
                if mitem(ui, th, Icon::Outdent, "Decrease indent", kp.outdent()) {
                    self.act(Action::Indent(-1));
                }
            });

            ui.menu_button("Line & paragraph spacing", |ui| {
                let fmt = self.tab_ref().doc.paras[self.tab_ref().doc.clamp(self.tab_ref().caret).p].fmt;
                let sb_lbl = if fmt.space_before > 0.0 { "Remove space before paragraph" } else { "Add space before paragraph" };
                let sa_lbl = if fmt.space_after > 0.0 { "Remove space after paragraph" } else { "Add space after paragraph" };
                fit_menu(
                    ui,
                    &[
                        ("Single (1.00)", ""),
                        ("Double (2.00)", ""),
                        (sb_lbl, ""),
                        (sa_lbl, ""),
                        ("Custom spacing…", ""),
                    ],
                );
                for (lbl, v) in [("Single (1.00)", 1.0f32), ("1.15", 1.15), ("1.5", 1.5), ("Double (2.00)", 2.0)] {
                    if mcheck(ui, th, (fmt.line_spacing - v).abs() < 0.01, lbl, "") {
                        self.act(Action::LineSpacing(v));
                    }
                }
                ui.separator();
                if fmt.space_before > 0.0 {
                    if mitem_plain(ui, th, "Remove space before paragraph", "") {
                        self.act(Action::SpaceBefore(0.0));
                    }
                } else if mitem_plain(ui, th, "Add space before paragraph", "") {
                    self.act(Action::SpaceBefore(10.0));
                }
                if fmt.space_after > 0.0 {
                    if mitem_plain(ui, th, "Remove space after paragraph", "") {
                        self.act(Action::SpaceAfter(0.0));
                    }
                } else if mitem_plain(ui, th, "Add space after paragraph", "") {
                    self.act(Action::SpaceAfter(10.0));
                }
                ui.separator();
                if mitem(ui, th, Icon::LineSpacing, "Custom spacing…", "") {
                    self.act(Action::Dialog(Dialog::Paragraph));
                }
            });

            ui.menu_button("Bullets & numbering", |ui| {
                fit_menu(
                    ui,
                    &[
                        ("Checklist", kp.checklist()),
                        ("Bulleted list menu", "▸"),
                        ("Numbered list menu", "▸"),
                    ],
                );
                if mitem(ui, th, Icon::Checklist, "Checklist", kp.checklist()) {
                    self.act(Action::List(ListKind::Checklist));
                }
                ui.menu_button("Bulleted list menu", |ui| {
                    fit_menu(
                        ui,
                        &[
                            ("•  ◦  ▪  Disc / Circle", kp.bullet_list()),
                            ("❖  ➢  ▪  Diamond / Arrow", ""),
                            ("➔  ➢  •  Arrow", ""),
                            ("★  ☆  •  Star", ""),
                        ],
                    );
                    if mitem(ui, th, Icon::Bullets, "•  ◦  ▪  Disc / Circle", kp.bullet_list()) {
                        self.act(Action::BulletPreset(ListKind::Bullet, 0));
                    }
                    if mitem(ui, th, Icon::Bullets, "❖  ➢  ▪  Diamond / Arrow", "") {
                        self.act(Action::BulletPreset(ListKind::Bullet, 1));
                    }
                    if mitem(ui, th, Icon::Bullets, "➔  ➢  •  Arrow", "") {
                        self.act(Action::BulletPreset(ListKind::Bullet, 2));
                    }
                    if mitem(ui, th, Icon::Bullets, "★  ☆  •  Star", "") {
                        self.act(Action::BulletPreset(ListKind::Bullet, 3));
                    }
                });
                ui.menu_button("Numbered list menu", |ui| {
                    fit_menu(
                        ui,
                        &[
                            ("1.  a.  i.  Standard", kp.number_list()),
                            ("1)  a)  i)  Parenthesis", ""),
                            ("I.  A.  1.  Roman", ""),
                            ("A.  B.  C.  Uppercase", ""),
                        ],
                    );
                    if mitem(ui, th, Icon::Numbers, "1.  a.  i.  Standard", kp.number_list()) {
                        self.act(Action::BulletPreset(ListKind::Number, 0));
                    }
                    if mitem(ui, th, Icon::Numbers, "1)  a)  i)  Parenthesis", "") {
                        self.act(Action::BulletPreset(ListKind::Number, 1));
                    }
                    if mitem(ui, th, Icon::Numbers, "I.  A.  1.  Roman", "") {
                        self.act(Action::BulletPreset(ListKind::Number, 2));
                    }
                    if mitem(ui, th, Icon::Numbers, "A.  B.  C.  Uppercase", "") {
                        self.act(Action::BulletPreset(ListKind::Number, 3));
                    }
                });
            });

            ui.separator();
            if mitem(ui, th, Icon::PageSetup, "Headers & footers…", "") {
                self.act(Action::Dialog(Dialog::PageSetup));
            }
            ui.menu_button("Page orientation", |ui| {
                let mut orient_entries: Vec<(&str, &str)> = vec![("Portrait", ""), ("Landscape", "")];
                for (name, _, _) in PAGE_SIZES {
                    orient_entries.push((name, ""));
                }
                fit_menu(ui, &orient_entries);
                let cur = self.tab_ref().doc.page;
                let land = cur.width > cur.height;
                if mcheck(ui, th, !land, "Portrait", "") && land {
                    self.act(Action::PageEdit(PageEdit::Swap));
                }
                if mcheck(ui, th, land, "Landscape", "") && !land {
                    self.act(Action::PageEdit(PageEdit::Swap));
                }
                ui.separator();
                for (name, w, h) in PAGE_SIZES {
                    let (pw, ph) = if land { (*h, *w) } else { (*w, *h) };
                    let sel = (cur.width - pw).abs() < 0.01 && (cur.height - ph).abs() < 0.01;
                    if mcheck(ui, th, sel, name, "") {
                        self.act(Action::PageEdit(PageEdit::Size(pw, ph)));
                    }
                }
            });
            if mitem(ui, th, Icon::Pageless, pageless_lbl, "") {
                self.act(Action::TogglePageless);
            }
            ui.separator();
            if mitem(ui, th, Icon::Painter, "Paint format", "") {
                self.act(Action::FormatPainter);
            }
            if mitem(ui, th, Icon::Clear, "Clear formatting", kp.clear_format()) {
                self.act(Action::ClearFormat);
            }
        });

        // ── TOOLS (Google Docs Tools menu!) ──
        ui.menu_button("Tools", |ui| {
            fit_menu(
                ui,
                &[
                    ("Spelling and grammar", "▸"),
                    ("Word count", kp.word_count()),
                    ("Display word count while typing", ""),
                    ("Review suggested edits", ""),
                    ("Citations…", ""),
                    ("Dictionary (Define word)…", kp.dictionary()),
                    ("Preferences…", ""),
                    ("Substitutions (SmartCorrect)…", ""),
                ],
            );
            ui.menu_button("Spelling and grammar", |ui| {
                fit_menu(
                    ui,
                    &[
                        ("Spelling and grammar check", kp.spellcheck()),
                        ("Show spelling suggestions", ""),
                    ],
                );
                if mitem(ui, th, Icon::Spell, "Spelling and grammar check", kp.spellcheck()) {
                    self.act(Action::Dialog(Dialog::Spell(0)));
                }
                if mcheck(ui, th, self.cfg.spellcheck, "Show spelling suggestions", "") {
                    self.act(Action::ToggleSpell);
                }
            });
            if mitem(ui, th, Icon::WordCount, "Word count", kp.word_count()) {
                self.act(Action::Dialog(Dialog::WordCount));
            }
            if mcheck(ui, th, self.cfg.live_word_count, "Display word count while typing", "") {
                self.act(Action::ToggleLiveWordCount);
            }
            ui.separator();
            if mitem(ui, th, Icon::Comment, "Review suggested edits", "") {
                self.act(Action::ToggleComments);
            }
            if mitem(ui, th, Icon::Citation, "Citations…", "") {
                self.act(Action::Dialog(Dialog::Citations(CitationEntry::default())));
            }
            if mitem(ui, th, Icon::Dictionary, "Dictionary (Define word)…", kp.dictionary()) {
                self.act(Action::Dialog(Dialog::Dictionary));
            }
            ui.separator();
            if mitem(ui, th, Icon::PageSetup, "Preferences…", "") {
                self.act(Action::Dialog(Dialog::Prefs));
            }
            if mitem(ui, th, Icon::EditMode, "Substitutions (SmartCorrect)…", "") {
                self.act(Action::Dialog(Dialog::SmartCorrect(String::new(), String::new())));
            }
        });

        // ── WINDOW ──
        ui.menu_button("Window", |ui| {
            let titles: Vec<String> = self
                .tabs
                .iter()
                .enumerate()
                .map(|(i, t)| format!("{}  {}", i + 1, t.title()))
                .collect();
            let mut win_entries: Vec<(&str, &str)> = vec![
                ("New Document Tab", "Ctrl+N"),
                ("Duplicate Tab", ""),
                ("Close Tab", kp.close_tab()),
                ("Close All Tabs", ""),
                ("Next Tab", "Ctrl+Tab"),
            ];
            for lbl in &titles {
                win_entries.push((lbl.as_str(), ""));
            }
            fit_menu(ui, &win_entries);
            if mitem(ui, th, Icon::New, "New Document Tab", "Ctrl+N") {
                self.act(Action::New);
            }
            if mitem(ui, th, Icon::CopyDoc, "Duplicate Tab", "") {
                self.act(Action::MakeCopy);
            }
            if mitem(ui, th, Icon::Close, "Close Tab", kp.close_tab()) {
                let i = self.active;
                self.act(Action::Close(i));
            }
            if mitem_plain(ui, th, "Close All Tabs", "") {
                self.act(Action::CloseAll);
            }
            if mitem_plain(ui, th, "Next Tab", "Ctrl+Tab") {
                self.act(Action::NextTab);
            }
            ui.separator();
            for (i, lbl) in titles.iter().enumerate() {
                if mcheck(ui, th, i == self.active, lbl, "") {
                    self.act(Action::SelectTab(i));
                }
            }
        });

        // ── HELP ──
        ui.menu_button("Help", |ui| {
            fit_menu(
                ui,
                &[
                    ("Search the menus", "Alt+/"),
                    ("Keyboard shortcuts", "Ctrl+/"),
                    ("About Scribe Pro…", ""),
                ],
            );
            if mitem(ui, th, Icon::SearchMenus, "Search the menus", "Alt+/") {
                self.act(Action::Dialog(Dialog::SearchMenus(String::new())));
            }
            ui.separator();
            if mitem(ui, th, Icon::PageSetup, "Keyboard shortcuts", "Ctrl+/") {
                self.act(Action::Dialog(Dialog::Shortcuts));
            }
            ui.separator();
            if mitem(ui, th, Icon::DocsLogo, "About Scribe Pro…", "") {
                self.act(Action::Dialog(Dialog::About));
            }
        });
    }

    /// Google Docs pill-shaped toolbar with Search the menus, Undo/Redo, Print, Spellcheck,
    /// Paint format, Zoom, Styles, Font, [-] [Size] [+], Bold/Italic/Underline/Color/Highlight,
    /// Link/Comment/@SmartChip, Align, Line spacing, Checklist/Bullets/Numbers, Indent, and Clear formatting.
    pub fn toolbar(&mut self, ui: &mut egui::Ui, th: &Theme) {
        let t = self.tab_ref();
        let st = t.typing_style();
        let fmt = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
        let (can_undo, can_redo) = (t.can_undo(), t.can_redo());
        let style_name = t.current_style_name();
        let zoom = self.effective_zoom();
        let kp = self.cfg.shortcut_preset;

        macro_rules! b {
            ($icon:expr, $tip:expr, $sel:expr, $en:expr, $act:expr) => {
                if icon_button(ui, th, $icon, $tip, $sel, $en).clicked() {
                    self.act($act);
                }
            };
        }

        // 1. Search the menus (magnifying glass on the far left of Google Docs' toolbar)
        b!(
            Icon::SearchMenus,
            "Search the menus (Alt+/)",
            matches!(self.dialog, Dialog::SearchMenus(_)),
            true,
            Action::Dialog(Dialog::SearchMenus(String::new()))
        );
        b!(Icon::Undo, "Undo (Ctrl+Z)", false, can_undo, Action::Undo);
        b!(Icon::Redo, "Redo (Ctrl+Y)", false, can_redo, Action::Redo);
        b!(Icon::Print, "Print (Ctrl+P)", false, true, Action::Print);
        b!(Icon::Spell, &format!("Spelling and grammar check ({})", kp.spellcheck()), matches!(self.dialog, Dialog::Spell(_)), self.speller.available(), Action::Dialog(Dialog::Spell(0)));
        b!(Icon::Painter, "Paint format", self.painter_style.is_some(), true, Action::FormatPainter);

        // Zoom dropdown
        let zoom_lbl = match self.zoom_mode {
            ZoomMode::PageWidth => "Fit".to_string(),
            _ => format!("{}%", (zoom * 100.0).round() as i32),
        };
        let zoom_r = combo_box_button(ui, th, 68.0, &zoom_lbl, FontId::proportional(13.0), "Zoom");
        egui::Popup::menu(&zoom_r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            let widest = ["Fit", "50%", "75%", "90%", "100%", "125%", "150%", "200%"]
                .iter()
                .map(|l| ui.fonts_mut(|f| f.layout_no_wrap((*l).to_string(), FontId::proportional(13.0), Color32::WHITE).size().x) + 16.0)
                .fold(0.0f32, f32::max);
            ui.set_width((widest + 10.0).ceil());
            if ui.selectable_label(self.zoom_mode == ZoomMode::PageWidth, "Fit").clicked() {
                self.act(Action::ZoomMode(ZoomMode::PageWidth));
                ui.close();
            }
            ui.separator();
            for z in [50, 75, 90, 100, 125, 150, 200] {
                let sel = self.zoom_mode == ZoomMode::Percent && (self.zoom * 100.0).round() as i32 == z;
                if ui.selectable_label(sel, format!("{z}%")).clicked() {
                    self.act(Action::SetZoom(z as f32 / 100.0));
                    ui.close();
                }
            }
        });
        sep(ui, th);

        // Styles dropdown
        let styles = named_styles();
        let style_r = combo_box_button(ui, th, 114.0, style_name, FontId::proportional(13.0), "Styles");
        egui::Popup::menu(&style_r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            let widest = styles
                .iter()
                .map(|s| {
                    let (fam, _) = self.book.family(s.chr.font, s.chr.bold, s.chr.italic);
                    let size = (s.chr.size * 0.9).clamp(11.0, 20.0);
                    ui.fonts_mut(|f| f.layout_no_wrap(s.name.to_string(), FontId::new(size, fam), Color32::WHITE).size().x) + 16.0
                })
                .fold(0.0f32, f32::max);
            ui.set_width((widest + 10.0).ceil());
            for (i, s) in styles.iter().enumerate() {
                let (fam, _) = self.book.family(s.chr.font, s.chr.bold, s.chr.italic);
                let size = (s.chr.size * 0.9).clamp(11.0, 20.0);
                if ui.selectable_label(s.name == style_name, RichText::new(s.name).font(FontId::new(size, fam))).clicked() {
                    self.act(Action::Style(i));
                    ui.close();
                }
            }
        });
        sep(ui, th);

        // Font picker: the current font shown in its own typeface; opens a searchable, grouped list
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
        sep(ui, th);

            // Google Docs [-][ 12 ][+] Font Size control inside a unified studio capsule
            Frame::new()
                .fill(th.surface_subtle)
                .stroke(Stroke::new(1.0, th.border))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(2, 1))
                .show(ui, |inner_ui| {
                    inner_ui.spacing_mut().item_spacing.x = 1.0;
                    if icon_button(inner_ui, th, Icon::Minus, &format!("Decrease font size ({})", kp.shrink_font()), false, st.size > MIN_SIZE).clicked() {
                        self.act(Action::StepFontSize(-1.0));
                    }
                    let size_txt = if st.size.fract() == 0.0 { format!("{}", st.size as i32) } else { format!("{:.1}", st.size) };
                    let size_r = combo_box_button(inner_ui, th, 44.0, &size_txt, FontId::proportional(13.0), "Font size");
                    egui::Popup::menu(&size_r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                        let widest = SIZE_LIST
                            .iter()
                            .map(|s| ui.fonts_mut(|f| f.layout_no_wrap(format!("{}", *s as i32), FontId::proportional(13.0), Color32::WHITE).size().x) + 16.0)
                            .fold(0.0f32, f32::max);
                        ui.set_width((widest + 10.0).ceil());
                        egui::ScrollArea::vertical().max_height(320.0).id_salt("size-list").show(ui, |ui| {
                            for s in SIZE_LIST {
                                if ui.selectable_label((st.size - s).abs() < 0.01, format!("{}", *s as i32)).clicked() {
                                    self.act(Action::Size(*s));
                                    ui.close();
                                }
                            }
                        });
                    });
                    if icon_button(inner_ui, th, Icon::Plus, &format!("Increase font size ({})", kp.grow_font()), false, st.size < MAX_SIZE).clicked() {
                        self.act(Action::StepFontSize(1.0));
                    }
                });
            sep(ui, th);

        // Bold, Italic, Underline, Strikethrough, Text color, Highlight color
        b!(Icon::Bold, "Bold (Ctrl+B)", st.bold, true, Action::Bold);
        b!(Icon::Italic, "Italic (Ctrl+I)", st.italic, true, Action::Italic);
        b!(Icon::Underline, "Underline (Ctrl+U)", st.underline, true, Action::Underline);
        b!(Icon::Strike, &format!("Strikethrough ({})", kp.strikethrough()), st.strike, true, Action::Strike);
        let r = icon_text_color_button(ui, th, "Text color (Solid & Craft Gradient)", st.color, st.color_to);
        egui::Popup::menu(&r).show(|ui| {
            ui.set_min_width(236.0);
            ui.label(RichText::new("Solid text color").small().strong());
            egui::Grid::new("pal").spacing(vec2(5.0, 5.0)).show(ui, |ui| {
                for (i, c) in PALETTE.iter().enumerate() {
                    if swatch(ui, *c, st.color == *c && !st.is_gradient_color()).clicked() {
                        self.act(Action::Color(*c));
                        ui.close();
                    }
                    if i % 5 == 4 {
                        ui.end_row();
                    }
                }
            });
            ui.separator();
            ui.label(RichText::new("Craft gradient text (Left → Right)").small().strong());
            egui::Grid::new("craft-text-grad-grid").spacing(vec2(6.0, 5.0)).show(ui, |ui| {
                for (i, (name, c1, c2)) in CRAFT_GRADIENT_TEXT.iter().enumerate() {
                    let sel = st.color == *c1 && st.color_to == Some(*c2);
                    if text_gradient_swatch(ui, name, *c1, *c2, sel).clicked() {
                        self.act(Action::ColorGradient(*c1, *c2));
                        ui.close();
                    }
                    if i % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
            ui.separator();
            ui.label(RichText::new("Custom text fade (Left → Right)").small().strong());
            let cur_from = st.color;
            let cur_to = st.color_to.unwrap_or([37, 99, 235]);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Left:").small().weak());
                for c in &PALETTE[5..15] {
                    if swatch(ui, *c, st.is_gradient_color() && st.color == *c).clicked() {
                        self.act(Action::ColorGradient(*c, cur_to));
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Right:").small().weak());
                for c in &PALETTE[5..15] {
                    if swatch(ui, *c, st.is_gradient_color() && st.color_to == Some(*c)).clicked() {
                        self.act(Action::ColorGradient(cur_from, *c));
                    }
                }
            });
        });
        let r = icon_highlight_button(ui, th, "Highlight color (Solid & Craft Gradient)", st.highlight, st.highlight_to);
        egui::Popup::menu(&r).show(|ui| {
            ui.set_min_width(236.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Solid highlight").small().strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("None").clicked() {
                        self.act(Action::Highlight(None));
                        ui.close();
                    }
                });
            });
            ui.horizontal(|ui| {
                for c in HIGHLIGHTS {
                    if swatch(ui, *c, st.highlight == Some(*c) && !st.is_gradient_highlight()).clicked() {
                        self.act(Action::Highlight(Some(*c)));
                        ui.close();
                    }
                }
            });
            ui.separator();
            ui.label(RichText::new("Craft gradient fade (Left → Right)").small().strong());
            egui::Grid::new("craft-grad-grid").spacing(vec2(6.0, 5.0)).show(ui, |ui| {
                for (i, (name, c1, c2)) in CRAFT_GRADIENT_HIGHLIGHTS.iter().enumerate() {
                    let sel = st.highlight == Some(*c1) && st.highlight_to == Some(*c2);
                    if gradient_swatch(ui, name, *c1, *c2, sel).clicked() {
                        self.act(Action::HighlightGradient(*c1, *c2));
                        ui.close();
                    }
                    if i % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
            ui.separator();
            ui.label(RichText::new("Custom fade (Left → Right)").small().strong());
            let cur_from = st.highlight.unwrap_or(HIGHLIGHTS[0]);
            let cur_to = st.highlight_to.or(st.highlight).unwrap_or(HIGHLIGHTS[3]);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Left:").small().weak());
                for c in HIGHLIGHTS {
                    if swatch(ui, *c, st.is_gradient_highlight() && st.highlight == Some(*c)).clicked() {
                        self.act(Action::HighlightGradient(*c, cur_to));
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Right:").small().weak());
                for c in HIGHLIGHTS {
                    if swatch(ui, *c, st.is_gradient_highlight() && st.highlight_to == Some(*c)).clicked() {
                        self.act(Action::HighlightGradient(cur_from, *c));
                    }
                }
            });
        });
        sep(ui, th);

        // Link, Comment, @ Smart Chip / Building Block
        let cur_sel_txt = self.tab_ref().selected_text();
        if icon_button(ui, th, Icon::Link, "Insert link (Ctrl+K)", st.link > 0, true).clicked() {
            self.act(Action::Dialog(Dialog::Link { text: cur_sel_txt, url: String::new() }));
        }
        b!(Icon::Comment, "Add comment (Ctrl+Alt+M)", false, true, Action::Dialog(Dialog::Comment(String::new())));
        b!(Icon::SmartChip, "Insert smart chip / building block (@)", false, true, Action::Dialog(Dialog::AtMenu(String::new())));
        sep(ui, th);

        // Align, Line spacing, Checklist, Bullets, Numbers, Indent
        b!(Icon::AlignLeft, &format!("Left align ({})", kp.align_left()), fmt.align == Align::Left, true, Action::Align(Align::Left));
        b!(Icon::AlignCenter, &format!("Center align ({})", kp.align_center()), fmt.align == Align::Center, true, Action::Align(Align::Center));
        b!(Icon::AlignRight, &format!("Right align ({})", kp.align_right()), fmt.align == Align::Right, true, Action::Align(Align::Right));
        b!(Icon::AlignJustify, &format!("Justify ({})", kp.align_justify()), fmt.align == Align::Justify, true, Action::Align(Align::Justify));
        let r = icon_button(ui, th, Icon::LineSpacing, "Line & paragraph spacing", false, true);
        egui::Popup::menu(&r).show(|ui| {
            let sb_lbl = if fmt.space_before > 0.0 { "Remove space before paragraph" } else { "Add space before paragraph" };
            let sa_lbl = if fmt.space_after > 0.0 { "Remove space after paragraph" } else { "Add space after paragraph" };
            fit_menu(
                ui,
                &[
                    ("Single (1.00)", ""),
                    ("Double (2.00)", ""),
                    (sb_lbl, ""),
                    (sa_lbl, ""),
                    ("Custom spacing…", ""),
                ],
            );
            for (lbl, v) in [("Single (1.00)", 1.0f32), ("1.15", 1.15), ("1.5", 1.5), ("Double (2.00)", 2.0)] {
                if ui.selectable_label((fmt.line_spacing - v).abs() < 0.01, lbl).clicked() {
                    self.act(Action::LineSpacing(v));
                    ui.close();
                }
            }
            ui.separator();
            if fmt.space_before > 0.0 {
                if ui.button("Remove space before paragraph").clicked() {
                    self.act(Action::SpaceBefore(0.0));
                    ui.close();
                }
            } else if ui.button("Add space before paragraph").clicked() {
                self.act(Action::SpaceBefore(10.0));
                ui.close();
            }
            if fmt.space_after > 0.0 {
                if ui.button("Remove space after paragraph").clicked() {
                    self.act(Action::SpaceAfter(0.0));
                    ui.close();
                }
            } else if ui.button("Add space after paragraph").clicked() {
                self.act(Action::SpaceAfter(10.0));
                ui.close();
            }
            ui.separator();
            if ui.button("Custom spacing…").clicked() {
                self.act(Action::Dialog(Dialog::Paragraph));
                ui.close();
            }
        });
        sep(ui, th);
        b!(Icon::Checklist, &format!("Checklist ({})", kp.checklist()), fmt.list == ListKind::Checklist, true, Action::List(ListKind::Checklist));
        b!(Icon::Bullets, &format!("Bulleted list ({})", kp.bullet_list()), fmt.list == ListKind::Bullet, true, Action::List(ListKind::Bullet));
        b!(Icon::Numbers, &format!("Numbered list ({})", kp.number_list()), fmt.list == ListKind::Number, true, Action::List(ListKind::Number));
        b!(Icon::Outdent, &format!("Decrease indent ({})", kp.outdent()), false, fmt.indent > 0, Action::Indent(-1));
        b!(Icon::Indent, &format!("Increase indent ({})", kp.indent()), false, true, Action::Indent(1));
        sep(ui, th);
        b!(Icon::Clear, &format!("Clear formatting ({})", kp.clear_format()), false, true, Action::ClearFormat);
        b!(Icon::NavPane, "Show document outline (Ctrl+Alt+A)", self.show_nav, true, Action::ToggleNav);
        if icon_button(ui, th, Icon::Marks, "Show non-printing characters (¶)", self.show_marks, true).clicked() {
            self.show_marks = !self.show_marks;
        }
        let chev = if self.compact_mode { Icon::ChevronDown } else { Icon::ChevronUp };
        b!(chev, "Hide the menus (Ctrl+Shift+F)", self.compact_mode, true, Action::ToggleCompact);
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
        let pageless = t.doc.pageless;
        let path = t.path.as_ref().map(|p| p.display().to_string());
        let font = FONT_NAMES[st.font as usize % FONT_NAMES.len()];
        let draft = self.draft;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let pill = |ui: &mut egui::Ui, text: String, strong_accent: bool| {
                Frame::new()
                    .fill(if strong_accent { th.accent_soft } else { th.surface_subtle })
                    .corner_radius(6)
                    .inner_margin(Margin::symmetric(7, 2))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(text)
                                .size(11.8)
                                .color(if strong_accent { th.accent } else { th.subtle }),
                        );
                    });
            };
            // " Pg %d" — the original's page indicator (FUN_00452923)
            if pageless {
                pill(ui, "Pageless".into(), true);
            } else {
                pill(ui, format!("Pg {page} of {pages}"), false);
            }
            let wc_str = match sel_words {
                Some(sw) => format!("{sw} of {words} words · {chars} chars"),
                None => format!("{words} words · {chars} chars"),
            };
            pill(ui, wc_str, false);
            pill(ui, format!("{font} {} pt · {style}", st.size), false);
            if let Some(p_str) = path {
                ui.label(RichText::new(p_str).size(11.5).color(th.subtle.gamma_multiply(0.85)));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let pct_txt = format!("{}%", (zoom * 100.0).round() as i32);
                if ui
                    .add(egui::Button::new(RichText::new(pct_txt).font(FontId::new(12.0, FontFamily::Proportional)).color(th.text)).frame(false))
                    .on_hover_text("Click to reset zoom to 100% (Ctrl+0)")
                    .clicked()
                {
                    self.act(Action::SetZoom(1.0));
                }
                if icon_button(ui, th, Icon::ZoomIn, "Zoom in (Ctrl+=)", false, zoom < ZOOM_MAX).clicked() {
                    self.act(Action::ZoomIn);
                }
                // Bespoke precision zoom track (replaces stock egui::Slider)
                let (zr, zresp) = ui.allocate_exact_size(vec2(96.0, 20.0), Sense::click_and_drag());
                let zp = ui.painter();
                let track = egui::Rect::from_min_max(pos2(zr.min.x + 5.0, zr.center().y - 2.0), pos2(zr.max.x - 5.0, zr.center().y + 2.0));
                zp.rect_filled(track, 2.0, th.surface_subtle);
                zp.rect_stroke(track, 2.0, Stroke::new(1.0, th.border), egui::StrokeKind::Inside);
                let frac = ((zoom - ZOOM_MIN) / (ZOOM_MAX - ZOOM_MIN)).clamp(0.0, 1.0);
                let tx = track.min.x + frac * track.width();
                let active_track = egui::Rect::from_min_max(track.min, pos2(tx, track.max.y));
                zp.rect_filled(active_track, 2.0, th.accent);
                let thumb_c = pos2(tx, track.center().y);
                let thumb_r = if zresp.hovered() || zresp.dragged() { 6.5 } else { 5.5 };
                zp.circle_filled(thumb_c, thumb_r, th.accent);
                zp.circle_filled(thumb_c, 2.3, Color32::WHITE);
                if (zresp.clicked() || zresp.dragged()) && track.width() > 1.0 {
                    if let Some(pos) = zresp.interact_pointer_pos() {
                        let t_val = ((pos.x - track.min.x) / track.width()).clamp(0.0, 1.0);
                        let raw_z = (ZOOM_MIN + t_val * (ZOOM_MAX - ZOOM_MIN)) * 100.0;
                        self.act(Action::SetZoom((raw_z / 5.0).round() * 5.0 / 100.0));
                    }
                }
                zresp.on_hover_text("Drag to adjust zoom level");

                if icon_button(ui, th, Icon::ZoomOut, "Zoom out (Ctrl+-)", false, zoom > ZOOM_MIN).clicked() {
                    self.act(Action::ZoomOut);
                }
                sep(ui, th);
                if icon_button(ui, th, Icon::PageWidth, "Page width", self.zoom_mode == ZoomMode::PageWidth, true).clicked() {
                    self.act(Action::ZoomMode(ZoomMode::PageWidth));
                }
                if icon_button(ui, th, Icon::FullPage, "Full page", self.zoom_mode == ZoomMode::FullPage, true).clicked() {
                    self.act(Action::ZoomMode(ZoomMode::FullPage));
                }
                sep(ui, th);

                // Segmented View Switcher Capsule: [ Pages | Draft | Pageless ]
                Frame::new()
                    .fill(th.surface_subtle)
                    .stroke(Stroke::new(1.0, th.border))
                    .corner_radius(7)
                    .inner_margin(Margin::symmetric(3, 2))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let seg_btn = |ui: &mut egui::Ui, label: &str, active: bool| -> bool {
                            let col = if active { th.accent } else { th.subtle };
                            let rt = if active {
                                RichText::new(label).size(11.5).color(col).strong()
                            } else {
                                RichText::new(label).size(11.5).color(col)
                            };
                            let b = egui::Button::new(rt)
                                .fill(if active { th.surface_elevated } else { Color32::TRANSPARENT })
                                .stroke(if active { Stroke::new(1.0, th.border) } else { Stroke::NONE })
                                .corner_radius(5);
                            ui.add(b).clicked()
                        };
                        if seg_btn(ui, "Pageless", pageless) && !pageless {
                            self.act(Action::TogglePageless);
                        }
                        if seg_btn(ui, "Draft", draft && !pageless) {
                            if pageless {
                                self.act(Action::TogglePageless);
                            }
                            self.act(Action::Draft(true));
                        }
                        if seg_btn(ui, "Pages", !draft && !pageless) {
                            if pageless {
                                self.act(Action::TogglePageless);
                            }
                            self.act(Action::Draft(false));
                        }
                    });
            });
        });
    }

    pub fn ruler(&mut self, ui: &mut egui::Ui, th: &Theme, zoom: f32) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, th.workspace);
        let t = self.tab_ref();
        let pg = t.doc.page;
        let ppi = BASE_PPI * zoom;
        let page_w = pg.width * ppi;
        let (cx0, cw) = if t.canvas_w > 0.0 { (t.canvas_x0, t.canvas_w) } else { (rect.min.x, rect.width()) };
        let x0 = cx0 + ((cw - page_w) / 2.0).max(layout::PAGE_GAP);
        let band = egui::Rect::from_min_max(egui::pos2(x0, rect.min.y + 3.0), egui::pos2(x0 + page_w, rect.max.y - 3.0));
        p.rect_filled(band, 4.0, th.surface_subtle);
        let ml = x0 + pg.margin_left * ppi;
        let mr = x0 + (pg.width - pg.margin_right) * ppi;
        p.rect_filled(
            egui::Rect::from_min_max(egui::pos2(ml, band.min.y), egui::pos2(mr, band.max.y)),
            2.0,
            th.surface_elevated,
        );
        p.rect_stroke(band, 4.0, Stroke::new(1.0, th.border), egui::StrokeKind::Inside);
        let from = -(pg.margin_left * 8.0).floor() as i32;
        let to = ((pg.width - pg.margin_left) * 8.0).ceil() as i32;
        for k in from..=to {
            let x = ml + k as f32 * ppi / 8.0;
            if x < band.min.x || x > band.max.x {
                continue;
            }
            if k % 8 == 0 {
                if k != 0 {
                    p.text(egui::pos2(x, band.center().y), egui::Align2::CENTER_CENTER, format!("{}", k.abs() / 8), FontId::proportional(9.0), th.subtle);
                }
            } else if ppi / 8.0 > 3.0 || k % 2 == 0 {
                let h = if k % 4 == 0 { 5.5 } else if k % 2 == 0 { 3.5 } else { 2.0 };
                p.line_segment([egui::pos2(x, band.center().y - h / 2.0), egui::pos2(x, band.center().y + h / 2.0)], Stroke::new(1.0, th.subtle.gamma_multiply(0.55)));
            }
        }
        let f = t.doc.paras[t.doc.clamp(t.caret).p].fmt;
        let ix = ml + f.indent as f32 * layout::INDENT_STEP * ppi;
        for x in [ix, mr] {
            let tri = vec![egui::pos2(x - 4.5, band.max.y), egui::pos2(x + 4.5, band.max.y), egui::pos2(x, band.max.y - 5.5)];
            p.add(egui::Shape::convex_polygon(tri, th.accent, Stroke::NONE));
        }
    }

    /// Google Docs Document Outline & Summary sidebar.
    pub fn nav_pane(&mut self, ui: &mut egui::Ui, th: &Theme) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Document Outline").strong().color(th.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (r, cr) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                if cr.hovered() {
                    ui.painter().rect_filled(r, 4.0, th.hover);
                }
                icons::draw(ui.painter(), r.shrink(4.0), Icon::Close, th.subtle, th.accent);
                if cr.on_hover_text("Close outline").clicked() {
                    self.act(Action::ToggleNav);
                }
            });
        });
        ui.add_space(4.0);

        // Google Docs Document Summary box
        ui.label(RichText::new("SUMMARY").size(10.5).color(th.subtle).strong());
        let mut summary = self.tab_ref().doc.summary.clone();
        if ui
            .add(egui::TextEdit::multiline(&mut summary).hint_text("+ Add document summary…").desired_rows(2).desired_width(ui.available_width()))
            .changed()
        {
            let t = self.tab();
            t.doc.summary = summary;
            t.dirty = true;
        }
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(RichText::new("OUTLINE").size(10.5).color(th.subtle).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("+ Insert TOC").on_hover_text("Insert clickable Table of Contents into document").clicked() {
                    self.act(Action::InsertToc);
                }
            });
        });

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
        let bookmarks = t.doc.bookmarks.clone();
        let current = heads.iter().rev().find(|h| h.0 <= caret_p).map(|h| h.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if heads.is_empty() {
                ui.label(RichText::new("Headings you add (Title, Heading 1–3) appear here so you can jump between sections.").weak().size(12.0));
            }
            for (i, level, text) in heads {
                let is_cur = current == Some(i);
                let indent_px = level.saturating_sub(1) as f32 * 12.0;
                let row_w = ui.available_width();
                let (rect, resp) = ui.allocate_exact_size(vec2(row_w, 26.0), Sense::click());
                let p = ui.painter();
                if is_cur {
                    p.rect_filled(rect, 6.0, th.accent_soft);
                    let bar = egui::Rect::from_min_max(pos2(rect.min.x + 2.0, rect.min.y + 5.0), pos2(rect.min.x + 5.0, rect.max.y - 5.0));
                    p.rect_filled(bar, 1.5, th.accent);
                } else if resp.hovered() {
                    p.rect_filled(rect, 6.0, th.hover);
                }
                let short: String = if text.chars().count() > 32 {
                    text.chars().take(30).collect::<String>() + "…"
                } else {
                    text
                };
                let font_id = FontId::proportional(if level == 0 { 13.0 } else if level == 1 { 12.5 } else { 12.0 });
                let col = if is_cur {
                    th.accent
                } else if level <= 1 {
                    th.text
                } else {
                    th.subtle
                };
                p.text(
                    pos2(rect.min.x + 10.0 + indent_px, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    short,
                    font_id,
                    col,
                );
                if resp.clicked() {
                    self.act(Action::GoToPara(i));
                }
            }
            if !bookmarks.is_empty() {
                ui.separator();
                ui.label(RichText::new("BOOKMARKS").size(10.5).color(th.subtle).strong());
                for (bname, bp) in bookmarks {
                    if ui.selectable_label(caret_p == bp, format!("🔖 {bname}")).clicked() {
                        self.act(Action::GoToPara(bp));
                    }
                }
            }
        });
    }

    /// Google Docs Right Sidebar: Comments & Suggested Edits.
    pub fn comments_pane(&mut self, ui: &mut egui::Ui, th: &Theme) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Comments & Suggestions").strong().color(th.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (r, cr) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                if cr.hovered() {
                    ui.painter().rect_filled(r, 4.0, th.hover);
                }
                icons::draw(ui.painter(), r.shrink(4.0), Icon::Close, th.subtle, th.accent);
                if cr.on_hover_text("Close comments").clicked() {
                    self.act(Action::ToggleComments);
                }
                if ui.small_button("+ New").on_hover_text("Add a comment (Ctrl+Alt+M)").clicked() {
                    self.act(Action::Dialog(Dialog::Comment(String::new())));
                }
            });
        });
        ui.add_space(6.0);

        let comments = self.tab_ref().doc.comments.clone();
        let active_list: Vec<_> = comments.iter().filter(|c| !c.resolved).collect();
        let resolved_list: Vec<_> = comments.iter().filter(|c| c.resolved).collect();

        let mut add_reply: Option<(u64, String)> = None;

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if active_list.is_empty() {
                ui.label(RichText::new("No open comments or suggested edits.\nSelect text and press Ctrl+Alt+M to add a comment, or switch to Suggesting mode.").weak().size(12.0));
            }
            for cm in active_list {
                let card_bg = if th.dark { Color32::from_rgb(38, 41, 46) } else { Color32::WHITE };
                Frame::new()
                    .fill(card_bg)
                    .corner_radius(8)
                    .stroke(Stroke::new(1.0, th.border))
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&cm.author).strong().size(12.5));
                            ui.label(RichText::new(&cm.timestamp).weak().size(11.0));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if cm.is_suggestion {
                                    if ui.small_button("✗ Reject").on_hover_text("Reject suggested edit").clicked() {
                                        self.act(Action::RejectSuggestion(cm.id));
                                    }
                                    if ui.small_button("✓ Accept").on_hover_text("Accept suggested edit").clicked() {
                                        self.act(Action::AcceptSuggestion(cm.id));
                                    }
                                } else if ui.small_button("✓ Resolve").on_hover_text("Mark as resolved").clicked() {
                                    self.act(Action::ResolveComment(cm.id));
                                }
                            });
                        });
                        if !cm.quote.is_empty() {
                            let q_short: String = cm.quote.chars().take(50).collect();
                            if ui
                                .add(egui::Button::new(RichText::new(format!("“{q_short}”")).italics().size(11.5).color(th.accent)).frame(false))
                                .on_hover_text("Click to jump to commented text")
                                .clicked()
                            {
                                self.act(Action::GoToPara(cm.para));
                            }
                        }
                        ui.label(RichText::new(&cm.text).size(13.0));
                        for (rauth, rtxt, rtime) in &cm.replies {
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(rauth).strong().size(11.5));
                                ui.label(RichText::new(rtime).weak().size(10.5));
                            });
                            ui.label(RichText::new(rtxt).size(12.0));
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let is_cur = self.comment_reply_draft.0 == cm.id;
                            let mut draft = if is_cur { self.comment_reply_draft.1.clone() } else { String::new() };
                            let te = ui.add(egui::TextEdit::singleline(&mut draft).hint_text("Reply…").desired_width(150.0));
                            if te.changed() {
                                self.comment_reply_draft = (cm.id, draft.clone());
                            }
                            if ui.small_button("Reply").clicked() && !draft.trim().is_empty() {
                                add_reply = Some((cm.id, draft.trim().to_string()));
                                self.comment_reply_draft = (0, String::new());
                            }
                        });
                    });
                ui.add_space(6.0);
            }

            if !resolved_list.is_empty() {
                ui.separator();
                ui.collapsing(format!("Resolved ({})", resolved_list.len()), |ui| {
                    for cm in resolved_list {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{}: {}", cm.author, cm.text)).weak().size(11.5));
                            if ui.small_button("Delete").clicked() {
                                self.act(Action::DeleteComment(cm.id));
                            }
                        });
                    }
                });
            }
        });

        if let Some((cid, rtxt)) = add_reply {
            let t = self.tab();
            let author = if t.doc.info.author.is_empty() { "You".to_string() } else { t.doc.info.author.clone() };
            let ts = chrono::Local::now().format("%-I:%M %p").to_string();
            if let Some(c) = t.doc.comments.iter_mut().find(|c| c.id == cid) {
                c.replies.push((author, rtxt, ts));
                t.dirty = true;
            }
        }
    }
}

/// Vertically centers a single-row galley inside `rect` by the actual cap-height of its first visible glyph,
/// so fonts with large internal ascent/descent metrics (such as Palatino Linotype or Segoe UI) are always
/// visually centered rather than pushed toward the top or bottom edge.
fn center_galley_y(rect: egui::Rect, g: &egui::Galley) -> f32 {
    if let Some(gl) = g.rows.first().and_then(|r| r.row.glyphs.iter().find(|gl| !gl.uv_rect.is_nothing())) {
        let cap_top = gl.pos.y + gl.uv_rect.offset.y;
        let cap_bot = gl.pos.y;
        (rect.center().y - (cap_top + cap_bot) * 0.5).round()
    } else {
        (rect.center().y - g.size().y * 0.5).round()
    }
}

/// Uniform 26px-tall studio dropdown capsule with vertically centered text and a vector chevron.
pub fn combo_box_button(ui: &mut egui::Ui, th: &Theme, width: f32, text: &str, font_id: FontId, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let bg = if open {
        th.accent_soft
    } else if resp.hovered() {
        th.hover
    } else {
        th.surface_subtle
    };
    ui.painter().rect_filled(rect, 7.0, bg);
    let border_col = if open {
        th.accent.gamma_multiply(0.50)
    } else if resp.hovered() {
        th.border
    } else {
        th.border.gamma_multiply(0.65)
    };
    ui.painter().rect_stroke(rect, 7.0, Stroke::new(1.0, border_col), egui::StrokeKind::Inside);
    let text_rect = egui::Rect::from_min_max(rect.min + vec2(8.0, 0.0), rect.max - vec2(19.0, 0.0));
    let fg = if open { th.accent } else { th.text };
    let g = ui.painter().layout_no_wrap(text.to_string(), font_id, fg);
    let y = center_galley_y(rect, &g);
    ui.painter_at(text_rect).galley(egui::pos2(text_rect.min.x, y), g, fg);
    let c = egui::pos2(rect.max.x - 10.5, rect.center().y);
    let chev_col = if open { th.accent } else { th.subtle };
    ui.painter().add(egui::Shape::line(vec![c + vec2(-3.2, -1.6), c + vec2(0.0, 1.8), c + vec2(3.2, -1.6)], Stroke::new(1.5, chev_col)));
    resp.on_hover_text(tip)
}

/// Toolbar button that shows the current font name in that font.
pub fn font_button(ui: &mut egui::Ui, th: &Theme, book: &crate::fonts::FontBook, font: u8) -> egui::Response {
    let (fam, _) = book.family(font, false, false);
    let name = FONT_NAMES[font as usize % FONT_NAMES.len()];
    combo_box_button(ui, th, 154.0, name, FontId::new(14.5, fam), "Font")
}

/// Searchable font list grouped by kind, each name drawn in its own typeface.
#[expect(clippy::too_many_arguments, reason = "Immediate-mode font list popup takes UI, theme, font book, and search/selection state")]
pub fn font_list(ui: &mut egui::Ui, th: &Theme, book: &crate::fonts::FontBook, current: u8, search: &mut String, recent: &[u8], focus: bool, max_h: f32) -> Option<u8> {
    let mut picked = None;
    let width = 300.0;
    ui.set_width(width);
    let r = ui.add(egui::TextEdit::singleline(search).hint_text("Search fonts").desired_width(width));
    if focus {
        r.request_focus();
    }
    let q = search.trim().to_lowercase();
    let available = |i: u8| book.faces.get(&(i, 0)).is_some_and(|f| f.is_some());
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
        let y = center_galley_y(rect, &g);
        ui.painter_at(text_clip).galley(egui::pos2(rect.min.x + 12.0, y), g, th.text);
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

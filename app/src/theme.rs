use eframe::egui::{self, Color32, CornerRadius, Stroke};

#[derive(Clone)]
pub struct Theme {
    pub dark: bool,
    pub accent: Color32,
    pub workspace: Color32,
    pub chrome: Color32,
    pub toolbar: Color32,
    pub border: Color32,
    pub page_border: Color32,
    pub guide: Color32,
    pub selection: Color32,
    pub find_highlight: Color32,
    pub caret: Color32,
    pub text: Color32,
    pub subtle: Color32,
    pub icon: Color32,
    pub hover: Color32,
    pub active: Color32,
}

impl Theme {
    pub fn light() -> Self {
        Self {
            dark: false,
            accent: Color32::from_rgb(37, 99, 235),
            workspace: Color32::from_rgb(228, 231, 236),
            chrome: Color32::from_rgb(249, 250, 251),
            toolbar: Color32::from_rgb(255, 255, 255),
            border: Color32::from_rgb(222, 225, 230),
            page_border: Color32::from_rgb(214, 218, 224),
            guide: Color32::from_rgb(214, 220, 232),
            selection: Color32::from_rgba_unmultiplied(56, 132, 255, 70),
            find_highlight: Color32::from_rgba_unmultiplied(255, 200, 0, 110),
            caret: Color32::from_rgb(17, 24, 39),
            text: Color32::from_rgb(31, 41, 55),
            subtle: Color32::from_rgb(107, 114, 128),
            icon: Color32::from_rgb(55, 65, 81),
            hover: Color32::from_rgb(237, 241, 247),
            active: Color32::from_rgb(219, 231, 254),
        }
    }
    pub fn dark() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(96, 165, 250),
            workspace: Color32::from_rgb(24, 26, 31),
            chrome: Color32::from_rgb(32, 35, 41),
            toolbar: Color32::from_rgb(38, 41, 48),
            border: Color32::from_rgb(52, 56, 64),
            page_border: Color32::from_rgb(60, 64, 72),
            guide: Color32::from_rgb(205, 212, 226),
            selection: Color32::from_rgba_unmultiplied(56, 132, 255, 80),
            find_highlight: Color32::from_rgba_unmultiplied(255, 200, 0, 110),
            caret: Color32::from_rgb(17, 24, 39),
            text: Color32::from_rgb(229, 231, 235),
            subtle: Color32::from_rgb(156, 163, 175),
            icon: Color32::from_rgb(209, 213, 219),
            hover: Color32::from_rgb(52, 56, 66),
            active: Color32::from_rgb(37, 58, 99),
        }
    }

    pub fn apply(&self, ctx: &egui::Context) {
        let mut style = (*ctx.global_style()).clone();
        let mut v = if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = self.chrome;
        v.window_fill = self.toolbar;
        v.extreme_bg_color = if self.dark { Color32::from_rgb(28, 30, 36) } else { Color32::WHITE };
        v.selection.bg_fill = self.accent.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, self.accent);
        v.hyperlink_color = self.accent;
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.window_stroke = Stroke::new(1.0, self.border);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open, &mut v.widgets.noninteractive] {
            w.corner_radius = CornerRadius::same(6);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        v.widgets.inactive.weak_bg_fill = if self.dark { Color32::from_rgb(46, 50, 58) } else { Color32::from_rgb(243, 244, 246) };
        v.widgets.inactive.bg_fill = v.widgets.inactive.weak_bg_fill;
        v.widgets.hovered.weak_bg_fill = self.hover;
        v.widgets.hovered.bg_fill = self.hover;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, self.border);
        v.widgets.active.weak_bg_fill = self.active;
        v.widgets.active.bg_fill = self.active;
        style.visuals = v;
        style.spacing.item_spacing = egui::vec2(6.0, 5.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.menu_margin = egui::Margin::same(6);
        style.spacing.interact_size.y = 24.0;
        ctx.set_theme(if self.dark { egui::Theme::Dark } else { egui::Theme::Light });
        ctx.set_style_of(egui::Theme::Light, style.clone());
        ctx.set_style_of(egui::Theme::Dark, style);
    }
}

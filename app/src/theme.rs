use eframe::egui::{self, epaint::Shadow, Color32, CornerRadius, Stroke};

#[derive(Clone)]
pub struct Theme {
    pub dark: bool,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub workspace: Color32,
    pub chrome: Color32,
    pub toolbar: Color32,
    pub surface_elevated: Color32,
    pub surface_subtle: Color32,
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
            accent: Color32::from_rgb(53, 89, 224),
            accent_soft: Color32::from_rgba_unmultiplied(53, 89, 224, 28),
            workspace: Color32::from_rgb(241, 243, 247),
            chrome: Color32::from_rgb(248, 249, 252),
            toolbar: Color32::from_rgb(255, 255, 255),
            surface_elevated: Color32::from_rgb(255, 255, 255),
            surface_subtle: Color32::from_rgb(240, 243, 248),
            border: Color32::from_rgb(222, 227, 235),
            page_border: Color32::from_rgb(212, 218, 227),
            guide: Color32::from_rgb(218, 224, 235),
            selection: Color32::from_rgba_unmultiplied(53, 89, 224, 58),
            find_highlight: Color32::from_rgba_unmultiplied(250, 204, 21, 120),
            caret: Color32::from_rgb(28, 35, 51),
            text: Color32::from_rgb(24, 28, 36),
            subtle: Color32::from_rgb(100, 108, 122),
            icon: Color32::from_rgb(56, 63, 76),
            hover: Color32::from_rgb(233, 238, 246),
            active: Color32::from_rgb(219, 229, 252),
        }
    }

    pub fn dark() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(122, 156, 255),
            accent_soft: Color32::from_rgba_unmultiplied(122, 156, 255, 38),
            workspace: Color32::from_rgb(16, 18, 22),
            chrome: Color32::from_rgb(22, 25, 31),
            toolbar: Color32::from_rgb(29, 33, 41),
            surface_elevated: Color32::from_rgb(33, 38, 47),
            surface_subtle: Color32::from_rgb(25, 28, 35),
            border: Color32::from_rgb(48, 54, 66),
            page_border: Color32::from_rgb(58, 65, 78),
            guide: Color32::from_rgb(205, 212, 226),
            selection: Color32::from_rgba_unmultiplied(96, 140, 255, 82),
            find_highlight: Color32::from_rgba_unmultiplied(250, 204, 21, 115),
            caret: Color32::from_rgb(17, 24, 39),
            text: Color32::from_rgb(235, 238, 245),
            subtle: Color32::from_rgb(148, 157, 173),
            icon: Color32::from_rgb(208, 214, 226),
            hover: Color32::from_rgb(41, 47, 58),
            active: Color32::from_rgb(44, 62, 107),
        }
    }

    pub fn apply(&self, ctx: &egui::Context) {
        let mut style = (*ctx.global_style()).clone();
        let mut v = if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = self.chrome;
        v.window_fill = self.surface_elevated;
        v.extreme_bg_color = if self.dark { Color32::from_rgb(19, 21, 27) } else { Color32::WHITE };
        v.faint_bg_color = self.surface_subtle;
        v.selection.bg_fill = self.accent.gamma_multiply(0.32);
        v.selection.stroke = Stroke::new(1.0, self.accent);
        v.hyperlink_color = self.accent;
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(10);
        v.window_stroke = Stroke::new(1.0, self.border);
        v.popup_shadow = Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: if self.dark {
                Color32::from_black_alpha(130)
            } else {
                Color32::from_black_alpha(28)
            },
        };
        v.window_shadow = Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 0,
            color: if self.dark {
                Color32::from_black_alpha(150)
            } else {
                Color32::from_black_alpha(36)
            },
        };
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
            &mut v.widgets.noninteractive,
        ] {
            w.corner_radius = CornerRadius::same(7);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.inactive.weak_bg_fill = self.surface_subtle;
        v.widgets.inactive.bg_fill = self.surface_subtle;
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.hovered.weak_bg_fill = self.hover;
        v.widgets.hovered.bg_fill = self.hover;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, self.border);
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.active.weak_bg_fill = self.active;
        v.widgets.active.bg_fill = self.active;
        v.widgets.active.bg_stroke = Stroke::new(1.0, self.accent.gamma_multiply(0.55));
        v.widgets.active.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.open.weak_bg_fill = self.active;
        v.widgets.open.bg_fill = self.active;
        v.widgets.open.bg_stroke = Stroke::new(1.0, self.accent.gamma_multiply(0.45));
        style.visuals = v;
        style.spacing.item_spacing = egui::vec2(6.0, 5.0);
        style.spacing.button_padding = egui::vec2(9.0, 4.5);
        style.spacing.menu_margin = egui::Margin::same(7);
        style.spacing.interact_size.y = 25.0;
        ctx.set_theme(if self.dark { egui::Theme::Dark } else { egui::Theme::Light });
        ctx.set_style_of(egui::Theme::Light, style.clone());
        ctx.set_style_of(egui::Theme::Dark, style);
    }
}

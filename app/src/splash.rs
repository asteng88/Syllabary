//! Animated studio splash screen for Syllabary.
//!
//! Renders an obsidian editorial studio card with animated amber-to-magenta aurora glows,
//! fine typographic registration marks, sweeping glass sheen, the Syllabary emblem,
//! live startup progress bar, and author signature (`asteng88 - studio88retro.com`).

use std::time::Instant;

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Mesh, Pos2, Rect, Sense, Shape,
    Stroke, TextureHandle, TextureOptions,
};

const APP_ICON_PNG: &[u8] = include_bytes!("../assets/icons/syllabary_256.png");

const HOLD_SECS: f32 = 2.35;
const FADE_OUT_SECS: f32 = 0.35;

pub struct SplashState {
    active: bool,
    started_at: Instant,
    dismissed_at: Option<Instant>,
    icon_tex: Option<TextureHandle>,
}

impl SplashState {
    pub fn new(show_on_launch: bool) -> Self {
        Self {
            active: show_on_launch,
            started_at: Instant::now(),
            dismissed_at: None,
            icon_tex: None,
        }
    }

    pub fn replay(&mut self) {
        self.active = true;
        self.started_at = Instant::now();
        self.dismissed_at = None;
    }

    fn ensure_icon(&mut self, ctx: &egui::Context) -> Option<&TextureHandle> {
        if self.icon_tex.is_none() {
            if let Ok(img) = image::load_from_memory(APP_ICON_PNG) {
                let rgba = img.into_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let color_img = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                self.icon_tex =
                    Some(ctx.load_texture("splash-app-icon", color_img, TextureOptions::LINEAR));
            }
        }
        self.icon_tex.as_ref()
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.active {
            return;
        }

        let elapsed = self.started_at.elapsed().as_secs_f32();

        // Allow instant smooth dismiss via Esc / Enter / Space
        if self.dismissed_at.is_none()
            && ctx.input(|i| {
                i.key_pressed(egui::Key::Escape)
                    || i.key_pressed(egui::Key::Enter)
                    || i.key_pressed(egui::Key::Space)
            })
        {
            self.dismissed_at = Some(Instant::now());
        }

        // Compute fade-in and fade-out alpha
        let fade_in = ease_out_cubic((elapsed / 0.32).clamp(0.0, 1.0));
        let fade_out = if let Some(dis) = self.dismissed_at {
            let t = (dis.elapsed().as_secs_f32() / 0.22).clamp(0.0, 1.0);
            if t >= 1.0 {
                self.active = false;
                return;
            }
            1.0 - ease_out_cubic(t)
        } else if elapsed >= HOLD_SECS {
            let t = ((elapsed - HOLD_SECS) / FADE_OUT_SECS).clamp(0.0, 1.0);
            if t >= 1.0 {
                self.active = false;
                return;
            }
            1.0 - ease_out_cubic(t)
        } else {
            1.0
        };

        let alpha = (fade_in * fade_out).clamp(0.0, 1.0);
        let progress = ease_out_cubic((elapsed / (HOLD_SECS - 0.35)).clamp(0.0, 1.0));
        let lift_y = (1.0 - fade_in) * 14.0;

        // Keep animating smoothly at 60fps while visible
        ctx.request_repaint();

        let screen = ctx.content_rect();
        let icon_id = self.ensure_icon(ctx).map(|t| t.id());

        let mut click_dismiss = false;
        let mut open_website = false;

        // Fullscreen translucent studio scrim
        egui::Area::new(egui::Id::unique("syllabary-splash-scrim"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (_, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
                if resp.clicked() {
                    click_dismiss = true;
                }
                let scrim_a = (175.0 * alpha) as u8;
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(scrim_a));
            });

        // Centered studio splash card
        let card_size = vec2(620.0, 356.0);
        let card_center = screen.center() + vec2(0.0, lift_y);
        let card_rect = Rect::from_center_size(card_center, card_size);

        egui::Area::new(egui::Id::unique("syllabary-splash-card"))
            .order(egui::Order::Foreground)
            .fixed_pos(card_rect.min)
            .show(ctx, |ui| {
                let cr = CornerRadius::same(20);
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(15, 16, 22, (255.0 * alpha) as u8))
                    .corner_radius(cr)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 14],
                        blur: 36,
                        spread: 2,
                        color: Color32::from_black_alpha((165.0 * alpha) as u8),
                    })
                    .show(ui, |ui| {
                let (rect, card_resp) = ui.allocate_exact_size(card_size, Sense::click());
                let p = ui.painter().with_clip_rect(rect.expand(4.0));

                // 1. Subtle inner top luminous sheen (rounded to match card corners)
                let card_p = ui.painter().with_clip_rect(rect);
                card_p.rect_filled(
                    rect,
                    cr,
                    Color32::from_rgba_unmultiplied(16, 17, 24, (255.0 * alpha) as u8),
                );
                let top_sheen = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + rect.height() * 0.48));
                card_p.rect_filled(
                    top_sheen,
                    CornerRadius { nw: 20, ne: 20, sw: 0, se: 0 },
                    Color32::from_rgba_unmultiplied(28, 30, 44, (120.0 * alpha) as u8),
                );

                // 3. Animated radial aurora glows (Amber Gold + Studio Magenta + Indigo)
                let wave = (elapsed * 1.4).sin();
                let wave2 = (elapsed * 1.1 + 1.3).cos();

                paint_soft_glow(
                    &card_p,
                    pos2(rect.left() + 125.0 + wave * 18.0, rect.top() + 115.0 + wave2 * 10.0),
                    190.0,
                    Color32::from_rgb(245, 158, 11),
                    0.18 * alpha,
                );
                paint_soft_glow(
                    &card_p,
                    pos2(rect.right() - 95.0 - wave2 * 22.0, rect.bottom() - 85.0 + wave * 12.0),
                    220.0,
                    Color32::from_rgb(236, 72, 153),
                    0.16 * alpha,
                );
                paint_soft_glow(
                    &card_p,
                    pos2(rect.right() - 120.0, rect.top() + 55.0),
                    150.0,
                    Color32::from_rgb(99, 102, 241),
                    0.10 * alpha,
                );

                // 4. Subtle editorial typographic grid & print registration marks
                let grid_col = Color32::from_rgba_unmultiplied(255, 255, 255, (10.0 * alpha) as u8);
                let step = 32.0;
                let mut gx = rect.left() + step;
                while gx < rect.right() {
                    card_p.line_segment(
                        [pos2(gx, rect.top()), pos2(gx, rect.bottom())],
                        Stroke::new(1.0, grid_col),
                    );
                    gx += step;
                }
                let mut gy = rect.top() + step;
                while gy < rect.bottom() {
                    card_p.line_segment(
                        [pos2(rect.left(), gy), pos2(rect.right(), gy)],
                        Stroke::new(1.0, grid_col),
                    );
                    gy += step;
                }

                // Corner registration brackets
                let mark_col = Color32::from_rgba_unmultiplied(255, 255, 255, (42.0 * alpha) as u8);
                let inset = 16.0;
                let arm = 10.0;
                for (cx, cy, dx, dy) in [
                    (rect.left() + inset, rect.top() + inset, 1.0, 1.0),
                    (rect.right() - inset, rect.top() + inset, -1.0, 1.0),
                    (rect.left() + inset, rect.bottom() - inset, 1.0, -1.0),
                    (rect.right() - inset, rect.bottom() - inset, -1.0, -1.0),
                ] {
                    card_p.line_segment(
                        [pos2(cx, cy), pos2(cx + dx * arm, cy)],
                        Stroke::new(1.0, mark_col),
                    );
                    card_p.line_segment(
                        [pos2(cx, cy), pos2(cx, cy + dy * arm)],
                        Stroke::new(1.0, mark_col),
                    );
                }

                // 5. Diagonal glass light sweep on entrance
                let sweep_t = ((elapsed - 0.12) / 0.95).clamp(0.0, 1.0);
                if sweep_t > 0.0 && sweep_t < 1.0 {
                    let sx = rect.left() - 140.0 + (rect.width() + 280.0) * ease_out_cubic(sweep_t);
                    let sweep_a = ((1.0 - (sweep_t * 2.0 - 1.0).abs()) * 24.0 * alpha) as u8;
                    let poly = vec![
                        pos2(sx, rect.top()),
                        pos2(sx + 80.0, rect.top()),
                        pos2(sx - 40.0, rect.bottom()),
                        pos2(sx - 120.0, rect.bottom()),
                    ];
                    card_p.add(Shape::convex_polygon(
                        poly,
                        Color32::from_rgba_unmultiplied(255, 255, 255, sweep_a),
                        Stroke::NONE,
                    ));
                }

                // 6. Hero Syllabary emblem with glowing halo
                let icon_scale = 0.88 + 0.12 * fade_in;
                let icon_sz = 108.0 * icon_scale;
                let icon_center = pos2(rect.left() + 102.0, rect.top() + 132.0);
                let icon_rect = Rect::from_center_size(icon_center, vec2(icon_sz, icon_sz));

                paint_soft_glow(
                    &card_p,
                    icon_center,
                    76.0,
                    Color32::from_rgb(245, 158, 11),
                    0.24 * alpha,
                );
                if let Some(tex_id) = icon_id {
                    card_p.image(
                        tex_id,
                        icon_rect,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::from_white_alpha((255.0 * alpha) as u8),
                    );
                }

                // 7. Editorial Wordmark, Studio Badge & Gradient Pill
                let tx = rect.left() + 178.0;
                card_p.text(
                    pos2(tx, rect.top() + 72.0),
                    Align2::LEFT_TOP,
                    "OFFLINE STUDIO EDITION",
                    FontId::proportional(11.0),
                    Color32::from_rgba_unmultiplied(251, 191, 36, (230.0 * alpha) as u8),
                );

                let title_galley = card_p.layout_no_wrap(
                    "Syllabary".to_string(),
                    FontId::proportional(40.0),
                    Color32::from_rgba_unmultiplied(250, 248, 243, (255.0 * alpha) as u8),
                );
                let title_w = title_galley.size().x;
                card_p.galley(
                    pos2(tx, rect.top() + 88.0),
                    title_galley,
                    Color32::from_rgba_unmultiplied(250, 248, 243, (255.0 * alpha) as u8),
                );

                // Version pill next to wordmark
                let ver_text = format!("v{}", crate::updater::CURRENT_VERSION);
                let pill_rect = Rect::from_min_size(
                    pos2(tx + title_w + 12.0, rect.top() + 104.0),
                    vec2(58.0, 22.0),
                );
                card_p.rect_filled(
                    pill_rect,
                    CornerRadius::same(11),
                    Color32::from_rgba_unmultiplied(255, 255, 255, (22.0 * alpha) as u8),
                );
                card_p.rect_stroke(
                    pill_rect,
                    CornerRadius::same(11),
                    Stroke::new(
                        1.0,
                        Color32::from_rgba_unmultiplied(245, 158, 11, (110.0 * alpha) as u8),
                    ),
                    egui::StrokeKind::Inside,
                );
                card_p.text(
                    pill_rect.center(),
                    Align2::CENTER_CENTER,
                    ver_text,
                    FontId::proportional(11.5),
                    Color32::from_rgba_unmultiplied(252, 211, 77, (245.0 * alpha) as u8),
                );

                // Signature amber-to-magenta accent rule
                let rule_rect = Rect::from_min_size(pos2(tx, rect.top() + 144.0), vec2(170.0, 4.0));
                paint_horizontal_gradient(
                    &card_p,
                    rule_rect,
                    Color32::from_rgba_unmultiplied(245, 158, 11, (245.0 * alpha) as u8),
                    Color32::from_rgba_unmultiplied(236, 72, 153, (245.0 * alpha) as u8),
                );

                // Tagline
                card_p.text(
                    pos2(tx, rect.top() + 160.0),
                    Align2::LEFT_TOP,
                    "Fast, 100% Offline Desktop Word Processor",
                    FontId::proportional(14.5),
                    Color32::from_rgba_unmultiplied(226, 232, 240, (235.0 * alpha) as u8),
                );
                card_p.text(
                    pos2(tx, rect.top() + 183.0),
                    Align2::LEFT_TOP,
                    "Native PDF, DOCX, ODT, Markdown & RTF Studio",
                    FontId::proportional(12.0),
                    Color32::from_rgba_unmultiplied(148, 163, 184, (215.0 * alpha) as u8),
                );

                // 8. Animated Amber-to-Magenta Progress Bar & Startup Milestone
                let bar_left = rect.left() + 44.0;
                let bar_right = rect.right() - 44.0;
                let bar_y = rect.bottom() - 92.0;

                let status_text = if progress < 0.35 {
                    "Initializing typography & layout engine…"
                } else if progress < 0.72 {
                    "Loading 17 bundled open-source fonts…"
                } else if progress < 0.96 {
                    "Preparing offline studio workspace…"
                } else {
                    "Ready."
                };

                card_p.text(
                    pos2(bar_left, bar_y - 18.0),
                    Align2::LEFT_CENTER,
                    status_text,
                    FontId::proportional(11.5),
                    Color32::from_rgba_unmultiplied(156, 163, 175, (220.0 * alpha) as u8),
                );
                card_p.text(
                    pos2(bar_right, bar_y - 18.0),
                    Align2::RIGHT_CENTER,
                    format!("{}%", (progress * 100.0).round() as u32),
                    FontId::proportional(11.5),
                    Color32::from_rgba_unmultiplied(251, 191, 36, (225.0 * alpha) as u8),
                );

                let track_rect =
                    Rect::from_min_max(pos2(bar_left, bar_y), pos2(bar_right, bar_y + 5.0));
                card_p.rect_filled(
                    track_rect,
                    CornerRadius::same(3),
                    Color32::from_rgba_unmultiplied(255, 255, 255, (24.0 * alpha) as u8),
                );

                let fill_w = (track_rect.width() * progress).max(6.0);
                let fill_rect = Rect::from_min_size(track_rect.min, vec2(fill_w, 5.0));
                paint_horizontal_gradient(
                    &card_p,
                    fill_rect,
                    Color32::from_rgba_unmultiplied(245, 158, 11, (255.0 * alpha) as u8),
                    Color32::from_rgba_unmultiplied(236, 72, 153, (255.0 * alpha) as u8),
                );
                // Bright leading spark dot
                card_p.circle_filled(
                    pos2(fill_rect.right(), fill_rect.center().y),
                    3.5,
                    Color32::from_rgba_unmultiplied(255, 245, 235, (235.0 * alpha) as u8),
                );

                // 9. Hairline footer divider & Author Signature (`asteng88 - studio88retro.com`)
                let div_y = rect.bottom() - 56.0;
                card_p.line_segment(
                    [pos2(bar_left, div_y), pos2(bar_right, div_y)],
                    Stroke::new(
                        1.0,
                        Color32::from_rgba_unmultiplied(255, 255, 255, (22.0 * alpha) as u8),
                    ),
                );

                let foot_y = rect.bottom() - 28.0;
                let prefix_g = card_p.layout_no_wrap(
                    "Created by ".to_string(),
                    FontId::proportional(12.5),
                    Color32::from_rgba_unmultiplied(148, 163, 184, (225.0 * alpha) as u8),
                );
                let prefix_w = prefix_g.size().x;
                card_p.galley(
                    pos2(bar_left, foot_y - prefix_g.size().y * 0.5),
                    prefix_g,
                    Color32::WHITE,
                );

                let author_g = card_p.layout_no_wrap(
                    "asteng88".to_string(),
                    FontId::proportional(13.0),
                    Color32::from_rgba_unmultiplied(250, 248, 243, (250.0 * alpha) as u8),
                );
                let author_w = author_g.size().x;
                card_p.galley(
                    pos2(bar_left + prefix_w, foot_y - author_g.size().y * 0.5),
                    author_g,
                    Color32::WHITE,
                );

                let sep_g = card_p.layout_no_wrap(
                    "  —  ".to_string(),
                    FontId::proportional(12.5),
                    Color32::from_rgba_unmultiplied(100, 116, 139, (220.0 * alpha) as u8),
                );
                let sep_w = sep_g.size().x;
                card_p.galley(
                    pos2(bar_left + prefix_w + author_w, foot_y - sep_g.size().y * 0.5),
                    sep_g,
                    Color32::WHITE,
                );

                // Clickable studio88retro.com link
                let site_x = bar_left + prefix_w + author_w + sep_w;
                let site_g = card_p.layout_no_wrap(
                    "studio88retro.com".to_string(),
                    FontId::proportional(13.0),
                    Color32::from_rgba_unmultiplied(251, 191, 36, (245.0 * alpha) as u8),
                );
                let site_rect = Rect::from_min_size(
                    pos2(site_x, foot_y - site_g.size().y * 0.5 - 2.0),
                    vec2(site_g.size().x + 4.0, site_g.size().y + 4.0),
                );
                let site_resp = ui.interact(
                    site_rect,
                    egui::Id::unique("splash-studio88-link"),
                    Sense::click(),
                );
                let site_col = if site_resp.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    Color32::from_rgba_unmultiplied(253, 224, 71, (255.0 * alpha) as u8)
                } else {
                    Color32::from_rgba_unmultiplied(251, 191, 36, (240.0 * alpha) as u8)
                };
                card_p.text(
                    pos2(site_x, foot_y),
                    Align2::LEFT_CENTER,
                    "studio88retro.com",
                    FontId::proportional(13.0),
                    site_col,
                );
                if site_resp.hovered() {
                    card_p.line_segment(
                        [
                            pos2(site_x, foot_y + 8.0),
                            pos2(site_x + site_g.size().x, foot_y + 8.0),
                        ],
                        Stroke::new(1.0, site_col),
                    );
                }
                if site_resp.clicked() {
                    open_website = true;
                }

                // Right-aligned dismiss hint
                card_p.text(
                    pos2(bar_right, foot_y),
                    Align2::RIGHT_CENTER,
                    "Click anywhere to continue",
                    FontId::proportional(11.0),
                    Color32::from_rgba_unmultiplied(100, 116, 139, (195.0 * alpha) as u8),
                );

                // 10. Crisp luminous glass outer border
                p.rect_stroke(
                    rect,
                    cr,
                    Stroke::new(
                        1.2,
                        Color32::from_rgba_unmultiplied(255, 255, 255, (42.0 * alpha) as u8),
                    ),
                    egui::StrokeKind::Inside,
                );

                if card_resp.clicked() && !open_website {
                    click_dismiss = true;
                }
                });
            });

        if open_website {
            ctx.open_url(egui::OpenUrl::new_tab("https://studio88retro.com"));
        } else if click_dismiss && self.dismissed_at.is_none() {
            self.dismissed_at = Some(Instant::now());
        }
    }
}

fn ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t.clamp(0.0, 1.0);
    1.0 - inv * inv * inv
}

fn paint_horizontal_gradient(p: &egui::Painter, rect: Rect, left: Color32, right: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), left);
    mesh.colored_vertex(rect.right_top(), right);
    mesh.colored_vertex(rect.right_bottom(), right);
    mesh.colored_vertex(rect.left_bottom(), left);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(Shape::mesh(mesh));
}

fn paint_soft_glow(p: &egui::Painter, center: Pos2, radius: f32, color: Color32, strength: f32) {
    // Layered concentric radial discs for a smooth, banding-free aurora bloom
    let steps = 8;
    for i in 0..steps {
        let frac = (i + 1) as f32 / steps as f32;
        let r = radius * frac;
        let falloff = (1.0 - frac).powi(2);
        let a = (255.0 * strength * falloff * 0.42).clamp(0.0, 255.0) as u8;
        if a > 0 {
            p.circle_filled(
                center,
                r,
                Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a),
            );
        }
    }
}

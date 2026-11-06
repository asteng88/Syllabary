//! Slim modern scroll bars with the same five parts as the original's
//! (small/large decrement, thumb, large/small increment), each labelled for
//! screen readers exactly like the Windows scroll bar the original used.

use eframe::egui::{self, pos2, vec2, Id, Rect, Role, Sense, Stroke, Ui, WidgetInfo};

use crate::theme::Theme;

pub const SIZE: f32 = 16.0;
const SMALL_STEP: f32 = 40.0;

pub fn scrollbar(ui: &mut Ui, rect: Rect, vertical: bool, offset: f32, content: f32, view: f32, th: &Theme, id: Id) -> Option<f32> {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, th.chrome);
    let bar = ui.interact(rect, id.with("bar"), Sense::hover());
    let name = if vertical { "Vertical Scroll Bar" } else { "Horizontal Scroll Bar" };
    bar.widget_info(|| WidgetInfo::labeled(Role::ScrollBar, true, name));

    let max = (content - view).max(0.0);
    let (len, a0) = if vertical { (rect.height(), rect.min.y) } else { (rect.width(), rect.min.x) };
    let along = |s: f32, e: f32| -> Rect {
        if vertical {
            Rect::from_min_max(pos2(rect.min.x, s), pos2(rect.max.x, e))
        } else {
            Rect::from_min_max(pos2(s, rect.min.y), pos2(e, rect.max.y))
        }
    };
    let track_s = a0 + SIZE;
    let track_e = a0 + len - SIZE;
    let track_len = (track_e - track_s).max(1.0);
    let thumb_len = if max <= 0.0 { track_len } else { (track_len * view / content).clamp(24.0_f32.min(track_len), track_len) };
    let travel = (track_len - thumb_len).max(0.0);
    let thumb_s = track_s + if max > 0.0 { travel * (offset / max).clamp(0.0, 1.0) } else { 0.0 };
    let thumb_e = thumb_s + thumb_len;

    let parts = [
        ("Back by small amount", along(a0, track_s)),
        ("Back by large amount", along(track_s, thumb_s)),
        ("Thumb", along(thumb_s, thumb_e)),
        ("Forward by large amount", along(thumb_e, track_e)),
        ("Forward by small amount", along(track_e, a0 + len)),
    ];
    let page = view * 0.9;
    let mut new = offset;
    let now = ui.input(|i| i.time);
    let press_start = ui.input(|i| i.pointer.press_start_time()).unwrap_or(now);
    let pressed_now = ui.input(|i| i.pointer.primary_pressed());
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    for (k, (label, r)) in parts.iter().enumerate() {
        let r = if r.width() <= 0.0 || r.height() <= 0.0 { Rect::from_min_size(r.min, vec2(0.5, 0.5)) } else { *r };
        let resp = ui.interact(r, id.with(k), Sense::click_and_drag());
        let label = *label;
        resp.widget_info(|| WidgetInfo::labeled(Role::Button, max > 0.0, label));
        let step = match k {
            0 => -SMALL_STEP,
            1 => -page,
            3 => page,
            4 => SMALL_STEP,
            _ => 0.0,
        };
        if k == 2 {
            if resp.dragged() && travel > 0.0 {
                let d = if vertical { resp.drag_delta().y } else { resp.drag_delta().x };
                new += d * max / travel;
            }
            let col = if resp.dragged() {
                th.subtle
            } else if resp.hovered() {
                th.subtle.gamma_multiply(0.8)
            } else {
                th.subtle.gamma_multiply(0.45)
            };
            if max > 0.0 {
                p.rect_filled(r.shrink(4.0), 4.0, col);
            }
        } else if resp.is_pointer_button_down_on() {
            if pressed_now {
                new += step;
            } else if now - press_start > 0.35 {
                new += step * dt * if k == 0 || k == 4 { 12.0 } else { 6.0 };
                ui.ctx().request_repaint();
            }
        }
        if k == 0 || k == 4 {
            let c = r.center();
            let col = if resp.hovered() { th.text } else { th.subtle };
            let s = 3.5;
            let pts = match (vertical, k == 0) {
                (true, true) => [pos2(c.x - s, c.y + s / 2.0), pos2(c.x, c.y - s / 2.0), pos2(c.x + s, c.y + s / 2.0)],
                (true, false) => [pos2(c.x - s, c.y - s / 2.0), pos2(c.x, c.y + s / 2.0), pos2(c.x + s, c.y - s / 2.0)],
                (false, true) => [pos2(c.x + s / 2.0, c.y - s), pos2(c.x - s / 2.0, c.y), pos2(c.x + s / 2.0, c.y + s)],
                (false, false) => [pos2(c.x - s / 2.0, c.y - s), pos2(c.x + s / 2.0, c.y), pos2(c.x - s / 2.0, c.y + s)],
            };
            p.add(egui::Shape::line(pts.to_vec(), Stroke::new(1.4, col)));
        }
    }
    let new = new.clamp(0.0, max);
    if (new - offset).abs() > 0.01 {
        Some(new)
    } else {
        None
    }
}

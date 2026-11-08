//! Whisper-quiet modern pill scrollbars with full screen-reader accessibility.
//! Keeps the five accessible regions (`Role::ScrollBar` and `Role::Button`) while
//! hiding visual clutter when content fits within the viewport.

use eframe::egui::{pos2, vec2, Id, Rect, Role, Sense, Ui, WidgetInfo};

use crate::theme::Theme;

pub const SIZE: f32 = 12.0;
const SMALL_STEP: f32 = 40.0;

#[expect(clippy::too_many_arguments, reason = "Immediate-mode scrollbar widget parameters")]
pub fn scrollbar(ui: &mut Ui, rect: Rect, vertical: bool, offset: f32, content: f32, view: f32, th: &Theme, id: Id) -> Option<f32> {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, th.workspace);
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
    let thumb_len = if max <= 0.5 { track_len } else { (track_len * view / content).clamp(32.0_f32.min(track_len), track_len) };
    let travel = (track_len - thumb_len).max(0.0);
    let thumb_s = track_s + if max > 0.5 { travel * (offset / max).clamp(0.0, 1.0) } else { 0.0 };
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
        resp.widget_info(|| WidgetInfo::labeled(Role::Button, max > 0.5, label));
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
            if max > 0.5 {
                let col = if resp.dragged() {
                    th.accent
                } else if resp.hovered() || bar.hovered() {
                    th.subtle.gamma_multiply(0.75)
                } else {
                    th.subtle.gamma_multiply(0.38)
                };
                let inset = if resp.hovered() || resp.dragged() { 2.5 } else { 3.5 };
                p.rect_filled(r.shrink(inset), 5.0, col);
            }
        } else if max > 0.5 && resp.is_pointer_button_down_on() {
            if pressed_now {
                new += step;
            } else if now - press_start > 0.35 {
                new += step * dt * if k == 0 || k == 4 { 12.0 } else { 6.0 };
                ui.ctx().request_repaint();
            }
        }
    }
    let new = new.clamp(0.0, max);
    if (new - offset).abs() > 0.01 {
        Some(new)
    } else {
        None
    }
}

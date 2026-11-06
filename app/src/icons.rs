//! Flat, line-style toolbar icons drawn as vector shapes (no bitmaps).

use eframe::egui::{epaint::CubicBezierShape, pos2, vec2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    New,
    Open,
    Save,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Find,
    Bold,
    Italic,
    Underline,
    Strike,
    TextColor,
    Highlight,
    AlignLeft,
    AlignCenter,
    AlignRight,
    AlignJustify,
    Bullets,
    Numbers,
    Outdent,
    Indent,
    PageBreak,
    Symbol,
    Marks,
    ZoomIn,
    ZoomOut,
    FullPage,
    PageWidth,
    WordCount,
    PageSetup,
    Clear,
    Moon,
    Sun,
    Close,
    Print,
    Painter,
    LineSpacing,
    Spell,
    NavPane,
    Pdf,
}

pub fn draw(p: &Painter, r: Rect, icon: Icon, col: Color32, accent: Color32) {
    let s = r.width().min(r.height());
    let c = r.center();
    let u = s / 20.0; // design grid: 20 × 20
    let at = |x: f32, y: f32| -> Pos2 { pos2(c.x + (x - 10.0) * u, c.y + (y - 10.0) * u) };
    let st = Stroke::new(1.5 * u.max(0.8), col);
    let acc = Stroke::new(1.5 * u.max(0.8), accent);
    let line = |a: (f32, f32), b: (f32, f32), stroke: Stroke| p.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
    let poly = |pts: &[(f32, f32)], stroke: Stroke, closed: bool| {
        let v: Vec<Pos2> = pts.iter().map(|q| at(q.0, q.1)).collect();
        if closed {
            p.add(Shape::closed_line(v, stroke));
        } else {
            p.add(Shape::line(v, stroke));
        }
    };
    let rrect = |x0: f32, y0: f32, x1: f32, y1: f32, rad: f32, stroke: Stroke| {
        p.rect_stroke(Rect::from_min_max(at(x0, y0), at(x1, y1)), rad * u, stroke, eframe::egui::StrokeKind::Middle);
    };
    let text = |t: &str, size: f32, color: Color32, family: FontFamily, dy: f32| {
        p.text(at(10.0, 10.0 + dy), eframe::egui::Align2::CENTER_CENTER, t, FontId::new(size * u, family), color);
    };
    let lines_icon = |xs: [(f32, f32); 4]| {
        for (i, (a, b)) in xs.iter().enumerate() {
            let yy = 4.5 + i as f32 * 3.7;
            line((*a, yy), (*b, yy), st);
        }
    };
    let arc = |cx: f32, cy: f32, rad: f32, a0: f32, a1: f32, stroke: Stroke| {
        let n = 18;
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let t = a0 + (a1 - a0) * i as f32 / n as f32;
                at(cx + rad * t.cos(), cy + rad * t.sin())
            })
            .collect();
        p.add(Shape::line(pts, stroke));
    };
    match icon {
        Icon::New => {
            poly(&[(5.0, 2.5), (11.5, 2.5), (15.5, 6.5), (15.5, 17.5), (5.0, 17.5)], st, true);
            poly(&[(11.5, 2.5), (11.5, 6.5), (15.5, 6.5)], st, false);
            line((10.25, 9.0), (10.25, 15.0), acc);
            line((7.25, 12.0), (13.25, 12.0), acc);
        }
        Icon::Open => {
            poly(&[(2.5, 5.0), (7.5, 5.0), (9.0, 6.5), (15.5, 6.5), (15.5, 8.5)], st, false);
            poly(&[(2.5, 5.0), (2.5, 16.0), (15.0, 16.0), (17.5, 9.0), (5.0, 9.0), (2.5, 16.0)], st, false);
            p.add(Shape::convex_polygon(
                vec![at(5.4, 9.8), at(16.4, 9.8), at(14.4, 15.2), at(3.6, 15.2)],
                accent.gamma_multiply(0.25),
                Stroke::NONE,
            ));
        }
        Icon::Save => {
            poly(&[(3.0, 3.0), (14.0, 3.0), (17.0, 6.0), (17.0, 17.0), (3.0, 17.0)], st, true);
            rrect(6.0, 3.0, 13.0, 7.5, 0.5, st);
            rrect(6.0, 11.0, 14.0, 17.0, 0.5, acc);
        }
        Icon::Undo => {
            arc(11.0, 11.0, 5.5, -std::f32::consts::FRAC_PI_2 * 1.0, std::f32::consts::FRAC_PI_2, st);
            line((11.0, 5.5), (4.5, 5.5), st);
            poly(&[(7.5, 2.5), (4.5, 5.5), (7.5, 8.5)], st, false);
            line((11.0, 16.5), (7.0, 16.5), st);
        }
        Icon::Redo => {
            arc(9.0, 11.0, 5.5, std::f32::consts::FRAC_PI_2, std::f32::consts::PI * 1.5, st);
            line((9.0, 5.5), (15.5, 5.5), st);
            poly(&[(12.5, 2.5), (15.5, 5.5), (12.5, 8.5)], st, false);
            line((9.0, 16.5), (13.0, 16.5), st);
        }
        Icon::Cut => {
            p.circle_stroke(at(6.0, 14.5), 2.6 * u, st);
            p.circle_stroke(at(14.0, 14.5), 2.6 * u, st);
            line((7.6, 12.5), (14.0, 2.5), st);
            line((12.4, 12.5), (6.0, 2.5), st);
        }
        Icon::Copy => {
            rrect(7.0, 6.5, 16.5, 17.5, 1.5, st);
            poly(&[(13.0, 6.5), (13.0, 2.5), (3.5, 2.5), (3.5, 13.5), (7.0, 13.5)], st, false);
        }
        Icon::Paste => {
            rrect(3.5, 4.0, 14.5, 17.5, 1.5, st);
            rrect(6.5, 2.5, 11.5, 5.5, 1.0, st);
            p.rect_filled(Rect::from_min_max(at(9.5, 9.0), at(17.5, 18.5)), 1.0 * u, accent.gamma_multiply(0.2));
            rrect(9.5, 9.0, 17.5, 18.5, 1.0, acc);
        }
        Icon::Find => {
            p.circle_stroke(at(8.5, 8.5), 5.0 * u, st);
            line((12.2, 12.2), (17.0, 17.0), Stroke::new(st.width * 1.4, col));
        }
        Icon::Bold => text("B", 15.0, col, FontFamily::Name("ui-bold".into()), 0.5),
        Icon::Italic => {
            line((12.5, 3.5), (7.5, 16.5), st);
            line((9.5, 3.5), (15.5, 3.5), st);
            line((4.5, 16.5), (10.5, 16.5), st);
        }
        Icon::Underline => {
            poly(&[(5.5, 3.0), (5.5, 9.5)], st, false);
            poly(&[(14.5, 3.0), (14.5, 9.5)], st, false);
            arc(10.0, 9.5, 4.5, 0.0, std::f32::consts::PI, st);
            line((4.0, 17.0), (16.0, 17.0), acc);
        }
        Icon::Strike => {
            text("S", 15.0, col, FontFamily::Proportional, 0.0);
            line((3.5, 10.5), (16.5, 10.5), acc);
        }
        Icon::TextColor => {
            poly(&[(5.0, 13.5), (10.0, 2.5), (15.0, 13.5)], st, false);
            line((7.0, 9.5), (13.0, 9.5), st);
            p.rect_filled(Rect::from_min_max(at(3.0, 15.5), at(17.0, 18.5)), 0.8 * u, accent);
        }
        Icon::Highlight => {
            poly(&[(6.0, 12.0), (12.5, 3.0), (16.0, 6.0), (9.5, 14.5)], st, true);
            poly(&[(6.0, 12.0), (4.5, 15.0), (7.5, 15.5), (9.5, 14.5)], st, false);
            p.rect_filled(Rect::from_min_max(at(3.0, 16.5), at(17.0, 19.0)), 0.8 * u, accent);
        }
        Icon::AlignLeft => lines_icon([(3.0, 17.0), (3.0, 12.0), (3.0, 17.0), (3.0, 10.0)]),
        Icon::AlignCenter => lines_icon([(3.0, 17.0), (6.0, 14.0), (3.0, 17.0), (7.0, 13.0)]),
        Icon::AlignRight => lines_icon([(3.0, 17.0), (8.0, 17.0), (3.0, 17.0), (10.0, 17.0)]),
        Icon::AlignJustify => lines_icon([(3.0, 17.0), (3.0, 17.0), (3.0, 17.0), (3.0, 17.0)]),
        Icon::Bullets => {
            for i in 0..3 {
                let yy = 5.0 + i as f32 * 5.0;
                p.circle_filled(at(4.5, yy), 1.4 * u, accent);
                line((8.0, yy), (17.0, yy), st);
            }
        }
        Icon::Numbers => {
            for i in 0..3 {
                let yy = 5.0 + i as f32 * 5.0;
                p.text(at(4.5, yy), eframe::egui::Align2::CENTER_CENTER, format!("{}", i + 1), FontId::proportional(6.5 * u), accent);
                line((8.0, yy), (17.0, yy), st);
            }
        }
        Icon::Outdent | Icon::Indent => {
            line((3.0, 4.0), (17.0, 4.0), st);
            line((9.0, 8.0), (17.0, 8.0), st);
            line((9.0, 12.0), (17.0, 12.0), st);
            line((3.0, 16.0), (17.0, 16.0), st);
            if icon == Icon::Indent {
                poly(&[(3.0, 7.0), (6.0, 10.0), (3.0, 13.0)], acc, false);
            } else {
                poly(&[(6.0, 7.0), (3.0, 10.0), (6.0, 13.0)], acc, false);
            }
        }
        Icon::PageBreak => {
            poly(&[(4.0, 2.5), (4.0, 7.0), (16.0, 7.0), (16.0, 2.5)], st, false);
            poly(&[(4.0, 17.5), (4.0, 13.0), (16.0, 13.0), (16.0, 17.5)], st, false);
            for i in 0..4 {
                let x = 2.5 + i as f32 * 4.5;
                line((x, 10.0), (x + 2.5, 10.0), acc);
            }
        }
        Icon::Symbol => text("Ω", 15.0, col, FontFamily::Proportional, 0.0),
        Icon::Marks => text("¶", 16.0, col, FontFamily::Proportional, 0.0),
        Icon::ZoomIn | Icon::ZoomOut => {
            p.circle_stroke(at(8.5, 8.5), 5.5 * u, st);
            line((12.5, 12.5), (17.0, 17.0), st);
            line((6.0, 8.5), (11.0, 8.5), acc);
            if icon == Icon::ZoomIn {
                line((8.5, 6.0), (8.5, 11.0), acc);
            }
        }
        Icon::FullPage => {
            rrect(5.0, 2.5, 15.0, 17.5, 1.0, st);
            for i in 0..3 {
                let yy = 6.5 + i as f32 * 3.0;
                line((7.5, yy), (12.5, yy), acc);
            }
        }
        Icon::PageWidth => {
            rrect(4.0, 3.0, 16.0, 17.0, 1.0, st);
            line((1.5, 10.0), (18.5, 10.0), acc);
            poly(&[(3.5, 8.0), (1.5, 10.0), (3.5, 12.0)], acc, false);
            poly(&[(16.5, 8.0), (18.5, 10.0), (16.5, 12.0)], acc, false);
        }
        Icon::WordCount => {
            text("123", 8.5, col, FontFamily::Name("ui-bold".into()), -2.5);
            line((3.0, 15.5), (17.0, 15.5), acc);
        }
        Icon::PageSetup => {
            rrect(4.0, 2.5, 16.0, 17.5, 1.0, st);
            rrect(6.5, 5.0, 13.5, 15.0, 0.0, Stroke::new(st.width * 0.7, accent));
        }
        Icon::Clear => {
            text("A", 13.0, col, FontFamily::Proportional, -1.0);
            line((12.5, 12.5), (17.5, 17.5), acc);
            line((17.5, 12.5), (12.5, 17.5), acc);
        }
        Icon::Moon => {
            let pts = [at(13.0, 3.0), at(4.0, 4.0), at(5.0, 17.0), at(16.5, 13.0)];
            p.add(CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, st));
            let pts2 = [at(13.0, 3.0), at(9.0, 6.0), at(10.0, 13.0), at(16.5, 13.0)];
            p.add(CubicBezierShape::from_points_stroke(pts2, false, Color32::TRANSPARENT, st));
        }
        Icon::Sun => {
            p.circle_stroke(at(10.0, 10.0), 3.5 * u, st);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (sn, cs) = a.sin_cos();
                p.line_segment([at(10.0 + cs * 5.5, 10.0 + sn * 5.5), at(10.0 + cs * 7.5, 10.0 + sn * 7.5)], st);
            }
        }
        Icon::Print => {
            poly(&[(5.5, 7.0), (5.5, 2.5), (14.5, 2.5), (14.5, 7.0)], st, false);
            rrect(2.5, 7.0, 17.5, 14.0, 1.5, st);
            rrect(5.5, 11.5, 14.5, 17.5, 0.5, acc);
            p.circle_filled(at(14.8, 9.4), 0.9 * u, accent);
        }
        Icon::Painter => {
            rrect(3.0, 2.5, 15.0, 7.5, 1.2, st);
            poly(&[(15.0, 5.0), (17.0, 5.0), (17.0, 10.0), (10.0, 10.0), (10.0, 12.0)], st, false);
            rrect(8.8, 12.0, 11.2, 18.0, 0.8, acc);
        }
        Icon::LineSpacing => {
            for i in 0..3 {
                let yy = 5.0 + i as f32 * 5.0;
                line((9.0, yy), (17.5, yy), st);
            }
            line((4.5, 3.0), (4.5, 17.0), acc);
            poly(&[(2.5, 5.0), (4.5, 3.0), (6.5, 5.0)], acc, false);
            poly(&[(2.5, 15.0), (4.5, 17.0), (6.5, 15.0)], acc, false);
        }
        Icon::Spell => {
            p.text(at(9.0, 7.5), eframe::egui::Align2::CENTER_CENTER, "abc", FontId::new(7.5 * u, FontFamily::Name("ui-bold".into())), col);
            poly(&[(5.5, 13.5), (8.5, 16.5), (15.0, 10.5)], acc, false);
        }
        Icon::NavPane => {
            rrect(2.5, 3.0, 17.5, 17.0, 1.5, st);
            line((7.5, 3.0), (7.5, 17.0), st);
            for i in 0..3 {
                let yy = 6.5 + i as f32 * 3.5;
                line((9.5, yy), (15.5, yy), acc);
            }
        }
        Icon::Pdf => {
            poly(&[(5.0, 2.5), (11.5, 2.5), (15.5, 6.5), (15.5, 17.5), (5.0, 17.5)], st, true);
            p.text(at(10.25, 12.0), eframe::egui::Align2::CENTER_CENTER, "PDF", FontId::new(5.0 * u, FontFamily::Name("ui-bold".into())), accent);
        }
        Icon::Close => {
            line((5.5, 5.5), (14.5, 14.5), st);
            line((14.5, 5.5), (5.5, 14.5), st);
        }
    }
    let _ = vec2(0.0, 0.0);
}

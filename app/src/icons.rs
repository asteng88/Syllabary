//! Hand-crafted duotone vector icon suite for Scribe Pro.
//! Every icon combines soft translucent accent fills, smooth cubic Bezier curves,
//! and precision vector geometry tuned for both Light and Dark themes.

use eframe::egui::{self, epaint::CubicBezierShape, pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    New,
    Open,
    Save,
    Print,
    Pdf,
    Cut,
    Copy,
    Paste,
    PastePlain,
    Undo,
    Redo,
    Find,
    Replace,
    Spell,
    AutoCorrect,
    Bold,
    Italic,
    Underline,
    Strike,
    Superscript,
    Subscript,
    TextColor,
    Highlight,
    AlignLeft,
    AlignCenter,
    AlignRight,
    AlignJustify,
    LineSpacing,
    ParaSpacing,
    Bullets,
    Numbers,
    Checklist,
    Indent,
    Outdent,
    Rule,
    PageBreak,
    DateTime,
    Symbol,
    Clear,
    Marks,
    Draft,
    NavPane,
    ZoomIn,
    ZoomOut,
    PageWidth,
    FullPage,
    Sun,
    Moon,
    Close,
    FormatPainter,
    Link,
    Comment,
    SmartChip,
    TableOfContents,
    HeaderFooter,
    PageNumber,
    WordCount,
    PageSetup,
    Rename,
    Trash,
    Star,
    StarFilled,
    VersionHistory,
    SearchMenus,
    EditMode,
    SuggestMode,
    ViewMode,
    Bookmark,
    Citation,
    Dictionary,
    Preferences,
    Shortcuts,
    About,
    Quote,
    Code,
    Pageless,
    Columns,
    Summary,
    ChevronUp,
    ChevronDown,
    DocsLogo,
    Check,
    Plus,
    Minus,
    Table,
    BuildingBlock,
    Footnote,
    Image,
}

#[expect(non_upper_case_globals, reason = "Alias constants match enum variant naming")]
impl Icon {
    pub const History: Self = Self::VersionHistory;
    pub const CopyDoc: Self = Self::Copy;
    pub const HorizontalRule: Self = Self::Rule;
    pub const Painter: Self = Self::FormatPainter;

    pub const ALL: &'static [(Self, &'static str, &'static str)] = &[
        // Brand & Files
        (Self::DocsLogo, "Scribe Crest", "Brand & Files"),
        (Self::New, "New Document", "Brand & Files"),
        (Self::Open, "Open File", "Brand & Files"),
        (Self::Save, "Save", "Brand & Files"),
        (Self::Print, "Print", "Brand & Files"),
        (Self::Pdf, "Export PDF", "Brand & Files"),
        (Self::Rename, "Rename", "Brand & Files"),
        (Self::Trash, "Delete / Reset", "Brand & Files"),
        (Self::Star, "Star", "Brand & Files"),
        (Self::StarFilled, "Starred", "Brand & Files"),
        (Self::VersionHistory, "History", "Brand & Files"),
        // Editing & Clipboard
        (Self::Undo, "Undo", "Editing"),
        (Self::Redo, "Redo", "Editing"),
        (Self::Cut, "Cut", "Editing"),
        (Self::Copy, "Copy", "Editing"),
        (Self::Paste, "Paste", "Editing"),
        (Self::PastePlain, "Paste Plain", "Editing"),
        (Self::Find, "Find", "Editing"),
        (Self::Replace, "Find & Replace", "Editing"),
        (Self::FormatPainter, "Paint Format", "Editing"),
        (Self::Clear, "Clear Format", "Editing"),
        // Typography
        (Self::Bold, "Bold", "Typography"),
        (Self::Italic, "Italic", "Typography"),
        (Self::Underline, "Underline", "Typography"),
        (Self::Strike, "Strikethrough", "Typography"),
        (Self::Superscript, "Superscript", "Typography"),
        (Self::Subscript, "Subscript", "Typography"),
        (Self::TextColor, "Text Color", "Typography"),
        (Self::Highlight, "Highlight", "Typography"),
        (Self::Quote, "Block Quote", "Typography"),
        (Self::Code, "Code Span", "Typography"),
        // Paragraph & Lists
        (Self::AlignLeft, "Align Left", "Paragraph"),
        (Self::AlignCenter, "Align Center", "Paragraph"),
        (Self::AlignRight, "Align Right", "Paragraph"),
        (Self::AlignJustify, "Justify", "Paragraph"),
        (Self::LineSpacing, "Line Spacing", "Paragraph"),
        (Self::ParaSpacing, "Para Spacing", "Paragraph"),
        (Self::Checklist, "Checklist", "Paragraph"),
        (Self::Bullets, "Bulleted List", "Paragraph"),
        (Self::Numbers, "Numbered List", "Paragraph"),
        (Self::Indent, "Indent", "Paragraph"),
        (Self::Outdent, "Outdent", "Paragraph"),
        // Insert & Rich Elements
        (Self::Link, "Insert Link", "Insert"),
        (Self::Comment, "Add Comment", "Insert"),
        (Self::SmartChip, "Smart Chip", "Insert"),
        (Self::Bookmark, "Bookmark", "Insert"),
        (Self::TableOfContents, "Contents", "Insert"),
        (Self::HeaderFooter, "Header/Footer", "Insert"),
        (Self::PageNumber, "Page Number", "Insert"),
        (Self::PageBreak, "Page Break", "Insert"),
        (Self::Rule, "Divider Line", "Insert"),
        (Self::DateTime, "Date & Time", "Insert"),
        (Self::Symbol, "Emoji & Symbols", "Insert"),
        // Review & Tools
        (Self::Spell, "Spell Check", "Tools"),
        (Self::AutoCorrect, "Substitutions", "Tools"),
        (Self::WordCount, "Word Count", "Tools"),
        (Self::Dictionary, "Dictionary", "Tools"),
        (Self::Citation, "Citations", "Tools"),
        (Self::SearchMenus, "Command Search", "Tools"),
        (Self::EditMode, "Editing Mode", "Tools"),
        (Self::SuggestMode, "Suggesting", "Tools"),
        (Self::ViewMode, "Viewing Mode", "Tools"),
        // View & Layout
        (Self::NavPane, "Outline Pane", "View & Chrome"),
        (Self::Summary, "Summary", "View & Chrome"),
        (Self::PageSetup, "Page Setup", "View & Chrome"),
        (Self::Pageless, "Pageless", "View & Chrome"),
        (Self::Draft, "Draft View", "View & Chrome"),
        (Self::Columns, "Columns", "View & Chrome"),
        (Self::Marks, "Paragraph Marks", "View & Chrome"),
        (Self::ZoomIn, "Zoom In", "View & Chrome"),
        (Self::ZoomOut, "Zoom Out", "View & Chrome"),
        (Self::PageWidth, "Fit Width", "View & Chrome"),
        (Self::FullPage, "Full Page", "View & Chrome"),
        (Self::Preferences, "Preferences", "View & Chrome"),
        (Self::Shortcuts, "Shortcuts", "View & Chrome"),
        (Self::About, "About", "View & Chrome"),
        (Self::Sun, "Light Mode", "View & Chrome"),
        (Self::Moon, "Dark Mode", "View & Chrome"),
        (Self::ChevronUp, "Collapse", "View & Chrome"),
        (Self::ChevronDown, "Expand", "View & Chrome"),
        (Self::Close, "Close", "View & Chrome"),
        (Self::Check, "Checkmark", "View & Chrome"),
        (Self::Plus, "Increase / Add", "View & Chrome"),
        (Self::Minus, "Decrease", "View & Chrome"),
        (Self::Table, "Table Grid", "Insert"),
        (Self::BuildingBlock, "Building Block", "Insert"),
        (Self::Footnote, "Footnote", "Insert"),
    ];
}

#[inline]
fn bezier_open(pts: [Pos2; 4], stroke: Stroke) -> Shape {
    Shape::CubicBezier(CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, stroke))
}

#[inline]
fn bezier_closed(pts: [Pos2; 4], fill: Color32, stroke: Stroke) -> Shape {
    Shape::CubicBezier(CubicBezierShape::from_points_stroke(pts, true, fill, stroke))
}

/// Draws a duotone page sheet with a folded top-right corner.
fn draw_page_sheet(p: &Painter, pt: impl Fn(f32, f32) -> Pos2, u: f32, fill: Color32, stroke: Stroke, accent: Color32) {
    let body = vec![
        pt(0.18, 0.08),
        pt(0.62, 0.08),
        pt(0.82, 0.28),
        pt(0.82, 0.92),
        pt(0.18, 0.92),
    ];
    p.add(Shape::convex_polygon(body.clone(), fill, Stroke::NONE));
    p.add(Shape::closed_line(body, stroke));
    let flap = vec![pt(0.62, 0.08), pt(0.62, 0.28), pt(0.82, 0.28)];
    p.add(Shape::convex_polygon(flap.clone(), accent.gamma_multiply(0.24), Stroke::NONE));
    p.add(Shape::line(flap, Stroke::new(u * 0.95, accent)));
}

pub fn draw(p: &Painter, r: Rect, icon: Icon, fg: Color32, accent: Color32) {
    let s = r.width().min(r.height());
    let u = (s / 15.5).max(1.25);
    let st = Stroke::new(u, fg);
    let st_thin = Stroke::new(u * 0.82, fg);
    let st_bold = Stroke::new(u * 1.22, fg);
    let ac = Stroke::new(u, accent);
    let ac_bold = Stroke::new(u * 1.18, accent);
    let tint = accent.gamma_multiply(0.20);
    let tint_strong = accent.gamma_multiply(0.32);
    let fg_tint = fg.gamma_multiply(0.14);

    let pt = |x: f32, y: f32| -> Pos2 { pos2(r.min.x + x * r.width(), r.min.y + y * r.height()) };
    let line = |p: &Painter, a: (f32, f32), b: (f32, f32), stroke: Stroke| {
        p.line_segment([pt(a.0, a.1), pt(b.0, b.1)], stroke);
    };
    let poly = |p: &Painter, pts: &[(f32, f32)], stroke: Stroke, closed: bool| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| pt(x, y)).collect();
        p.add(if closed { Shape::closed_line(v, stroke) } else { Shape::line(v, stroke) });
    };
    let fill_poly = |p: &Painter, pts: &[(f32, f32)], fill: Color32, stroke: Stroke| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| pt(x, y)).collect();
        p.add(Shape::convex_polygon(v, fill, stroke));
    };

    match icon {
        Icon::DocsLogo => {
            // Bespoke Scribe Studio crest: rounded indigo squircle with fountain nib & page lines
            let badge = r.shrink(s * 0.04);
            p.rect_filled(badge, s * 0.24, accent);
            let inner_glow = Rect::from_min_max(badge.min + vec2(1.0, 1.0), pos2(badge.max.x - 1.0, badge.center().y));
            p.rect_filled(inner_glow, s * 0.22, Color32::from_white_alpha(38));
            // Crisp white fountain-pen nib & page lines
            let w_col = Color32::WHITE;
            let w_soft = Color32::from_white_alpha(200);
            let nib = [
                pt(0.50, 0.18),
                pt(0.70, 0.46),
                pt(0.60, 0.78),
                pt(0.40, 0.78),
                pt(0.30, 0.46),
            ];
            p.add(Shape::convex_polygon(nib.to_vec(), Color32::from_white_alpha(45), Stroke::new(u * 0.95, w_col)));
            p.line_segment([pt(0.50, 0.19), pt(0.50, 0.50)], Stroke::new(u * 0.85, w_col));
            p.circle_filled(pt(0.50, 0.53), u * 0.75, w_col);
            p.line_segment([pt(0.34, 0.82), pt(0.66, 0.82)], Stroke::new(u * 0.95, w_soft));
        }
        Icon::New => {
            draw_page_sheet(p, pt, u, tint, st, accent);
            // Sparkle / plus badge in accent
            p.circle_filled(pt(0.50, 0.58), s * 0.18, accent.gamma_multiply(0.18));
            line(p, (0.50, 0.44), (0.50, 0.72), ac_bold);
            line(p, (0.36, 0.58), (0.64, 0.58), ac_bold);
        }
        Icon::Open => {
            // Layered duotone folder with front leaf
            let back = Rect::from_min_max(pt(0.08, 0.24), pt(0.88, 0.84));
            p.rect_filled(back, 2.0, tint);
            poly(
                p,
                &[(0.08, 0.82), (0.08, 0.20), (0.38, 0.20), (0.47, 0.31), (0.88, 0.31), (0.88, 0.44)],
                st,
                false,
            );
            fill_poly(
                p,
                &[(0.08, 0.84), (0.20, 0.44), (0.94, 0.44), (0.84, 0.84)],
                tint_strong,
                Stroke::new(u, accent),
            );
        }
        Icon::Save => {
            let body = vec![
                pt(0.12, 0.12),
                pt(0.74, 0.12),
                pt(0.88, 0.26),
                pt(0.88, 0.88),
                pt(0.12, 0.88),
            ];
            p.add(Shape::convex_polygon(body.clone(), tint, Stroke::NONE));
            p.add(Shape::closed_line(body, st));
            let shutter = Rect::from_min_max(pt(0.28, 0.12), pt(0.68, 0.38));
            p.rect_filled(shutter, 1.5, fg_tint);
            p.rect_stroke(shutter, 1.5, st_thin, egui::StrokeKind::Inside);
            let label = Rect::from_min_max(pt(0.24, 0.54), pt(0.76, 0.88));
            p.rect_filled(label, 2.0, tint_strong);
            p.rect_stroke(label, 2.0, ac, egui::StrokeKind::Inside);
            line(p, (0.34, 0.68), (0.66, 0.68), ac);
        }
        Icon::Print => {
            // Top paper sheet
            let top_sheet = Rect::from_min_max(pt(0.26, 0.10), pt(0.74, 0.36));
            p.rect_filled(top_sheet, 1.5, tint);
            poly(p, &[(0.26, 0.36), (0.26, 0.10), (0.74, 0.10), (0.74, 0.36)], st_thin, false);
            // Printer carriage
            let body = Rect::from_min_max(pt(0.10, 0.36), pt(0.90, 0.72));
            p.rect_filled(body, 3.0, fg_tint);
            p.rect_stroke(body, 3.0, st, egui::StrokeKind::Inside);
            p.circle_filled(pt(0.76, 0.48), u * 0.75, accent);
            // Output paper with text lines
            let out_sheet = Rect::from_min_max(pt(0.26, 0.58), pt(0.74, 0.90));
            p.rect_filled(out_sheet, 1.5, tint_strong);
            p.rect_stroke(out_sheet, 1.5, ac, egui::StrokeKind::Inside);
            line(p, (0.35, 0.70), (0.65, 0.70), ac);
            line(p, (0.35, 0.80), (0.56, 0.80), ac);
        }
        Icon::Pdf => {
            draw_page_sheet(p, pt, u, tint, st, accent);
            // Stylized ribbon / Bezier trefoil curve signature for PDF
            let pill = Rect::from_min_max(pt(0.10, 0.52), pt(0.78, 0.82));
            p.rect_filled(pill, 3.0, accent);
            // Crisp vector "PDF" micro-glyphs inside the ribbon
            let w_st = Stroke::new(u * 0.78, Color32::WHITE);
            // P
            poly(p, &[(0.19, 0.75), (0.19, 0.59), (0.29, 0.59), (0.29, 0.67), (0.19, 0.67)], w_st, false);
            // D
            poly(p, &[(0.36, 0.59), (0.45, 0.59), (0.48, 0.63), (0.48, 0.71), (0.45, 0.75), (0.36, 0.75)], w_st, true);
            // F
            poly(p, &[(0.56, 0.75), (0.56, 0.59), (0.68, 0.59)], w_st, false);
            line(p, (0.56, 0.67), (0.65, 0.67), w_st);
        }
        Icon::Cut => {
            p.circle_filled(pt(0.27, 0.74), s * 0.15, tint_strong);
            p.circle_filled(pt(0.73, 0.74), s * 0.15, tint_strong);
            p.circle_stroke(pt(0.27, 0.74), s * 0.15, ac);
            p.circle_stroke(pt(0.73, 0.74), s * 0.15, ac);
            line(p, (0.38, 0.64), (0.80, 0.14), st);
            line(p, (0.62, 0.64), (0.20, 0.14), st);
            p.circle_filled(pt(0.50, 0.48), u * 0.7, accent);
        }
        Icon::Copy => {
            let back = Rect::from_min_max(pt(0.14, 0.12), pt(0.68, 0.72));
            p.rect_filled(back, 2.5, fg_tint);
            p.rect_stroke(back, 2.5, st_thin, egui::StrokeKind::Inside);
            let front = Rect::from_min_max(pt(0.32, 0.28), pt(0.86, 0.88));
            p.rect_filled(front, 2.5, tint_strong);
            p.rect_stroke(front, 2.5, ac, egui::StrokeKind::Inside);
            line(p, (0.43, 0.48), (0.75, 0.48), st_thin);
            line(p, (0.43, 0.61), (0.75, 0.61), st_thin);
            line(p, (0.43, 0.74), (0.64, 0.74), st_thin);
        }
        Icon::Paste => {
            let board = Rect::from_min_max(pt(0.16, 0.20), pt(0.84, 0.90));
            p.rect_filled(board, 3.0, tint);
            p.rect_stroke(board, 3.0, st, egui::StrokeKind::Inside);
            let clip = Rect::from_min_max(pt(0.34, 0.10), pt(0.66, 0.27));
            p.rect_filled(clip, 2.5, tint_strong);
            p.rect_stroke(clip, 2.5, ac, egui::StrokeKind::Inside);
            line(p, (0.30, 0.46), (0.70, 0.46), ac);
            line(p, (0.30, 0.60), (0.70, 0.60), st_thin);
            line(p, (0.30, 0.74), (0.56, 0.74), st_thin);
        }
        Icon::PastePlain => {
            let board = Rect::from_min_max(pt(0.16, 0.20), pt(0.84, 0.90));
            p.rect_filled(board, 3.0, tint);
            p.rect_stroke(board, 3.0, st, egui::StrokeKind::Inside);
            let clip = Rect::from_min_max(pt(0.34, 0.10), pt(0.66, 0.27));
            p.rect_filled(clip, 2.5, tint_strong);
            p.rect_stroke(clip, 2.5, ac, egui::StrokeKind::Inside);
            // Crisp vector "T" inside clipboard
            line(p, (0.34, 0.44), (0.66, 0.44), ac_bold);
            line(p, (0.50, 0.44), (0.50, 0.78), ac_bold);
            line(p, (0.42, 0.78), (0.58, 0.78), ac);
        }
        Icon::Undo => {
            p.add(bezier_open(
                [pt(0.20, 0.40), pt(0.56, 0.34), pt(0.88, 0.42), pt(0.82, 0.78)],
                st,
            ));
            p.add(bezier_open(
                [pt(0.82, 0.78), pt(0.78, 0.84), pt(0.56, 0.84), pt(0.42, 0.82)],
                ac,
            ));
            poly(p, &[(0.38, 0.22), (0.16, 0.40), (0.38, 0.58)], ac_bold, false);
        }
        Icon::Redo => {
            p.add(bezier_open(
                [pt(0.80, 0.40), pt(0.44, 0.34), pt(0.12, 0.42), pt(0.18, 0.78)],
                st,
            ));
            p.add(bezier_open(
                [pt(0.18, 0.78), pt(0.22, 0.84), pt(0.44, 0.84), pt(0.58, 0.82)],
                ac,
            ));
            poly(p, &[(0.62, 0.22), (0.84, 0.40), (0.62, 0.58)], ac_bold, false);
        }
        Icon::Find | Icon::SearchMenus => {
            let c = pt(0.42, 0.42);
            let rad = s * 0.27;
            p.circle_filled(c, rad, tint);
            p.circle_stroke(c, rad, st);
            // Subtle lens reflection arc
            p.add(bezier_open([pt(0.29, 0.34), pt(0.32, 0.26), pt(0.40, 0.24), pt(0.47, 0.27)], ac));
            line(p, (0.62, 0.62), (0.88, 0.88), ac_bold);
        }
        Icon::Replace => {
            let c = pt(0.36, 0.36);
            p.circle_filled(c, s * 0.22, tint);
            p.circle_stroke(c, s * 0.22, st);
            line(p, (0.52, 0.52), (0.68, 0.68), st);
            // Curved swap arrows in accent
            p.add(bezier_open([pt(0.56, 0.24), pt(0.78, 0.24), pt(0.88, 0.42), pt(0.82, 0.62)], ac));
            poly(p, &[(0.72, 0.56), (0.82, 0.65), (0.91, 0.54)], ac, false);
            line(p, (0.18, 0.82), (0.56, 0.82), ac);
            poly(p, &[(0.28, 0.72), (0.16, 0.82), (0.28, 0.92)], ac, false);
        }
        Icon::Spell => {
            // Vector "A" on top-left + bold accent checkmark badge on bottom-right
            poly(p, &[(0.14, 0.62), (0.32, 0.14), (0.50, 0.62)], st, false);
            line(p, (0.20, 0.46), (0.44, 0.46), st);
            p.circle_filled(pt(0.66, 0.68), s * 0.24, tint_strong);
            poly(p, &[(0.48, 0.69), (0.62, 0.83), (0.88, 0.50)], ac_bold, false);
        }
        Icon::AutoCorrect => {
            // Magic wand with duotone sparkle stars
            line(p, (0.14, 0.86), (0.68, 0.32), st_bold);
            line(p, (0.56, 0.44), (0.72, 0.28), ac_bold);
            // Sparkle 1 (top-right)
            fill_poly(
                p,
                &[(0.76, 0.08), (0.81, 0.21), (0.94, 0.26), (0.81, 0.31), (0.76, 0.44), (0.71, 0.31), (0.58, 0.26), (0.71, 0.21)],
                tint_strong,
                ac,
            );
            // Sparkle 2 (top-left)
            line(p, (0.28, 0.14), (0.28, 0.34), ac);
            line(p, (0.18, 0.24), (0.38, 0.24), ac);
            // Sparkle 3 (bottom-right)
            line(p, (0.80, 0.64), (0.80, 0.82), ac);
            line(p, (0.71, 0.73), (0.89, 0.73), ac);
        }
        Icon::Bold => {
            // Precision geometric serif 'B' with right-rounded duotone counter bowls
            let r_top = egui::CornerRadius { nw: 0, sw: 0, ne: (s * 0.16) as u8, se: (s * 0.16) as u8 };
            let r_bot = egui::CornerRadius { nw: 0, sw: 0, ne: (s * 0.18) as u8, se: (s * 0.18) as u8 };
            let top_bowl = Rect::from_min_max(pt(0.28, 0.16), pt(0.68, 0.49));
            let bot_bowl = Rect::from_min_max(pt(0.28, 0.49), pt(0.74, 0.84));
            p.rect_filled(top_bowl, r_top, tint);
            p.rect_filled(bot_bowl, r_bot, tint_strong);
            p.rect_stroke(top_bowl, r_top, st_bold, egui::StrokeKind::Inside);
            p.rect_stroke(bot_bowl, r_bot, st_bold, egui::StrokeKind::Inside);
            line(p, (0.20, 0.16), (0.34, 0.16), st_bold);
            line(p, (0.20, 0.84), (0.34, 0.84), st_bold);
            line(p, (0.28, 0.16), (0.28, 0.84), ac_bold);
        }
        Icon::Italic => {
            // Editorial serif Italic 'I'
            line(p, (0.38, 0.16), (0.74, 0.16), st_bold);
            line(p, (0.58, 0.16), (0.42, 0.84), ac_bold);
            line(p, (0.26, 0.84), (0.62, 0.84), st_bold);
        }
        Icon::Underline => {
            // Classic proportioned 'U' with straight stems + semicircular bowl + accent underline
            line(p, (0.20, 0.14), (0.34, 0.14), st);
            line(p, (0.66, 0.14), (0.80, 0.14), st);
            line(p, (0.27, 0.14), (0.27, 0.48), st_bold);
            line(p, (0.73, 0.14), (0.73, 0.48), st_bold);
            p.add(bezier_open(
                [pt(0.27, 0.47), pt(0.27, 0.74), pt(0.73, 0.74), pt(0.73, 0.47)],
                st_bold,
            ));
            line(p, (0.18, 0.86), (0.82, 0.86), ac_bold);
        }
        Icon::Strike => {
            // Smooth cubic-Bezier 'S' with accent strikethrough bar
            p.add(bezier_open(
                [pt(0.70, 0.24), pt(0.58, 0.10), pt(0.26, 0.14), pt(0.28, 0.34)],
                st_bold,
            ));
            p.add(bezier_open(
                [pt(0.28, 0.34), pt(0.30, 0.50), pt(0.70, 0.50), pt(0.72, 0.68)],
                st_bold,
            ));
            p.add(bezier_open(
                [pt(0.72, 0.68), pt(0.74, 0.88), pt(0.40, 0.90), pt(0.28, 0.76)],
                st_bold,
            ));
            line(p, (0.12, 0.50), (0.88, 0.50), ac_bold);
        }
        Icon::Superscript => {
            // Crisp vector 'X' + elevated accent '2'
            line(p, (0.14, 0.32), (0.56, 0.84), st_bold);
            line(p, (0.56, 0.32), (0.14, 0.84), st_bold);
            // Vector '2' in top-right
            p.add(bezier_open([pt(0.64, 0.22), pt(0.66, 0.10), pt(0.88, 0.10), pt(0.86, 0.26)], ac));
            poly(p, &[(0.86, 0.26), (0.64, 0.46), (0.90, 0.46)], ac, false);
        }
        Icon::Subscript => {
            // Crisp vector 'X' + lowered accent '2'
            line(p, (0.14, 0.20), (0.56, 0.72), st_bold);
            line(p, (0.56, 0.20), (0.14, 0.72), st_bold);
            // Vector '2' in bottom-right
            p.add(bezier_open([pt(0.64, 0.62), pt(0.66, 0.50), pt(0.88, 0.50), pt(0.86, 0.66)], ac));
            poly(p, &[(0.86, 0.66), (0.64, 0.86), (0.90, 0.86)], ac, false);
        }
        Icon::TextColor => {
            // Precision vector 'A' + rounded color pill bar beneath
            poly(p, &[(0.22, 0.68), (0.50, 0.12), (0.78, 0.68)], st_bold, false);
            line(p, (0.31, 0.50), (0.69, 0.50), st);
            let bar = Rect::from_min_max(pt(0.14, 0.77), pt(0.86, 0.92));
            p.rect_filled(bar, 2.5, accent);
        }
        Icon::Highlight => {
            // Angled chisel-tip marker pen + luminous highlight swatch bar
            fill_poly(
                p,
                &[(0.58, 0.12), (0.82, 0.34), (0.48, 0.66), (0.26, 0.44)],
                tint_strong,
                st,
            );
            fill_poly(
                p,
                &[(0.26, 0.44), (0.48, 0.66), (0.34, 0.72), (0.20, 0.72)],
                accent,
                Stroke::new(u * 0.8, fg),
            );
            let bar = Rect::from_min_max(pt(0.12, 0.80), pt(0.88, 0.94));
            p.rect_filled(bar, 2.5, accent);
        }
        Icon::AlignLeft => {
            line(p, (0.12, 0.20), (0.88, 0.20), st);
            line(p, (0.12, 0.40), (0.64, 0.40), ac_bold);
            line(p, (0.12, 0.60), (0.88, 0.60), st);
            line(p, (0.12, 0.80), (0.54, 0.80), ac_bold);
        }
        Icon::AlignCenter => {
            line(p, (0.12, 0.20), (0.88, 0.20), st);
            line(p, (0.26, 0.40), (0.74, 0.40), ac_bold);
            line(p, (0.12, 0.60), (0.88, 0.60), st);
            line(p, (0.30, 0.80), (0.70, 0.80), ac_bold);
        }
        Icon::AlignRight => {
            line(p, (0.12, 0.20), (0.88, 0.20), st);
            line(p, (0.36, 0.40), (0.88, 0.40), ac_bold);
            line(p, (0.12, 0.60), (0.88, 0.60), st);
            line(p, (0.46, 0.80), (0.88, 0.80), ac_bold);
        }
        Icon::AlignJustify => {
            line(p, (0.12, 0.20), (0.88, 0.20), st);
            line(p, (0.12, 0.40), (0.88, 0.40), ac_bold);
            line(p, (0.12, 0.60), (0.88, 0.60), st);
            line(p, (0.12, 0.80), (0.88, 0.80), ac_bold);
        }
        Icon::LineSpacing => {
            for y in [0.22, 0.50, 0.78] {
                line(p, (0.46, y), (0.90, y), st);
            }
            line(p, (0.22, 0.16), (0.22, 0.84), ac);
            poly(p, &[(0.10, 0.30), (0.22, 0.15), (0.34, 0.30)], ac, false);
            poly(p, &[(0.10, 0.70), (0.22, 0.85), (0.34, 0.70)], ac, false);
        }
        Icon::ParaSpacing => {
            let blk1 = Rect::from_min_max(pt(0.44, 0.16), pt(0.90, 0.40));
            let blk2 = Rect::from_min_max(pt(0.44, 0.60), pt(0.90, 0.84));
            p.rect_filled(blk1, 2.0, tint);
            p.rect_stroke(blk1, 2.0, st_thin, egui::StrokeKind::Inside);
            p.rect_filled(blk2, 2.0, tint);
            p.rect_stroke(blk2, 2.0, st_thin, egui::StrokeKind::Inside);
            line(p, (0.22, 0.14), (0.22, 0.86), ac);
            poly(p, &[(0.10, 0.28), (0.22, 0.14), (0.34, 0.28)], ac, false);
            poly(p, &[(0.10, 0.72), (0.22, 0.86), (0.34, 0.72)], ac, false);
        }
        Icon::Bullets => {
            for y in [0.22, 0.50, 0.78] {
                p.circle_filled(pt(0.20, y), u * 1.15, accent);
                line(p, (0.40, y), (0.88, y), st);
            }
        }
        Icon::Numbers => {
            // Crisp vector 1, 2, 3 instead of blurry tiny font text
            // 1
            poly(p, &[(0.14, 0.18), (0.21, 0.14), (0.21, 0.30)], ac, false);
            line(p, (0.15, 0.30), (0.27, 0.30), ac);
            // 2
            poly(p, &[(0.14, 0.43), (0.26, 0.43), (0.26, 0.50), (0.14, 0.58), (0.27, 0.58)], ac, false);
            // 3
            poly(p, &[(0.14, 0.70), (0.26, 0.70), (0.20, 0.77), (0.26, 0.85), (0.14, 0.85)], ac, false);
            for y in [0.22, 0.50, 0.78] {
                line(p, (0.42, y), (0.88, y), st);
            }
        }
        Icon::Checklist => {
            // Checked box on top
            let b1 = Rect::from_min_max(pt(0.10, 0.16), pt(0.34, 0.40));
            p.rect_filled(b1, 2.5, tint_strong);
            p.rect_stroke(b1, 2.5, ac, egui::StrokeKind::Inside);
            poly(p, &[(0.15, 0.28), (0.21, 0.35), (0.31, 0.20)], ac_bold, false);
            line(p, (0.46, 0.28), (0.88, 0.28), st);
            // Unchecked box on bottom
            let b2 = Rect::from_min_max(pt(0.10, 0.58), pt(0.34, 0.82));
            p.rect_filled(b2, 2.5, fg_tint);
            p.rect_stroke(b2, 2.5, st_thin, egui::StrokeKind::Inside);
            line(p, (0.46, 0.70), (0.88, 0.70), st);
        }
        Icon::Indent => {
            line(p, (0.12, 0.18), (0.88, 0.18), st);
            line(p, (0.48, 0.40), (0.88, 0.40), st);
            line(p, (0.48, 0.60), (0.88, 0.60), st);
            line(p, (0.12, 0.82), (0.88, 0.82), st);
            line(p, (0.10, 0.50), (0.36, 0.50), ac_bold);
            poly(p, &[(0.24, 0.36), (0.38, 0.50), (0.24, 0.64)], ac_bold, false);
        }
        Icon::Outdent => {
            line(p, (0.12, 0.18), (0.88, 0.18), st);
            line(p, (0.48, 0.40), (0.88, 0.40), st);
            line(p, (0.48, 0.60), (0.88, 0.60), st);
            line(p, (0.12, 0.82), (0.88, 0.82), st);
            line(p, (0.38, 0.50), (0.12, 0.50), ac_bold);
            poly(p, &[(0.26, 0.36), (0.12, 0.50), (0.26, 0.64)], ac_bold, false);
        }
        Icon::Rule => {
            line(p, (0.16, 0.22), (0.84, 0.22), st_thin);
            let bar = Rect::from_min_max(pt(0.08, 0.44), pt(0.92, 0.56));
            p.rect_filled(bar, 2.0, accent);
            line(p, (0.16, 0.78), (0.84, 0.78), st_thin);
        }
        Icon::PageBreak => {
            let top_p = Rect::from_min_max(pt(0.18, 0.08), pt(0.82, 0.38));
            let bot_p = Rect::from_min_max(pt(0.18, 0.62), pt(0.82, 0.92));
            p.rect_filled(top_p, 2.0, tint);
            poly(p, &[(0.18, 0.38), (0.18, 0.10), (0.82, 0.10), (0.82, 0.38)], st, false);
            p.rect_filled(bot_p, 2.0, tint);
            poly(p, &[(0.18, 0.62), (0.18, 0.90), (0.82, 0.90), (0.82, 0.62)], st, false);
            for x in [0.10, 0.34, 0.58, 0.82] {
                line(p, (x, 0.50), (x + 0.10, 0.50), ac_bold);
            }
        }
        Icon::DateTime => {
            let cal = Rect::from_min_max(pt(0.12, 0.20), pt(0.88, 0.88));
            p.rect_filled(cal, 3.0, tint);
            p.rect_stroke(cal, 3.0, st, egui::StrokeKind::Inside);
            let hdr = Rect::from_min_max(pt(0.12, 0.20), pt(0.88, 0.40));
            p.rect_filled(hdr, 3.0, tint_strong);
            line(p, (0.12, 0.40), (0.88, 0.40), st);
            line(p, (0.32, 0.10), (0.32, 0.26), ac_bold);
            line(p, (0.68, 0.10), (0.68, 0.26), ac_bold);
            p.circle_filled(pt(0.36, 0.58), u * 0.8, accent);
            p.circle_filled(pt(0.64, 0.58), u * 0.8, fg);
            p.circle_filled(pt(0.36, 0.74), u * 0.8, fg);
            p.circle_filled(pt(0.64, 0.74), u * 0.8, accent);
        }
        Icon::Symbol => {
            // Smiley emoji / special symbol duotone face
            let c = pt(0.50, 0.50);
            p.circle_filled(c, s * 0.39, tint);
            p.circle_stroke(c, s * 0.39, st);
            p.circle_filled(pt(0.36, 0.40), u * 0.85, accent);
            p.circle_filled(pt(0.64, 0.40), u * 0.85, accent);
            p.add(bezier_open(
                [pt(0.32, 0.58), pt(0.40, 0.74), pt(0.60, 0.74), pt(0.68, 0.58)],
                ac_bold,
            ));
        }
        Icon::Clear => {
            // Diagonal Italic 'T' with accent slash badge
            line(p, (0.26, 0.18), (0.78, 0.18), st_bold);
            line(p, (0.54, 0.18), (0.36, 0.82), st_bold);
            line(p, (0.16, 0.84), (0.84, 0.20), ac_bold);
        }
        Icon::Marks => {
            // Crisp vector Pilcrow (¶) with filled bowl
            let bowl = Rect::from_min_max(pt(0.22, 0.14), pt(0.56, 0.52));
            p.rect_filled(bowl, s * 0.18, tint_strong);
            p.add(bezier_open(
                [pt(0.54, 0.14), pt(0.16, 0.14), pt(0.16, 0.52), pt(0.54, 0.52)],
                ac_bold,
            ));
            line(p, (0.54, 0.14), (0.54, 0.86), st_bold);
            line(p, (0.72, 0.14), (0.72, 0.86), st_bold);
            line(p, (0.46, 0.14), (0.80, 0.14), st);
        }
        Icon::Draft => {
            let box_r = Rect::from_min_max(pt(0.12, 0.18), pt(0.88, 0.82));
            p.rect_filled(box_r, 3.0, tint);
            for (i, y) in [0.26, 0.42, 0.58, 0.74].iter().enumerate() {
                let x1 = if i % 2 == 0 { 0.84 } else { 0.66 };
                line(p, (0.16, *y), (x1, *y), if i == 0 { ac_bold } else { st });
            }
        }
        Icon::NavPane => {
            let outer = r.shrink(s * 0.12);
            p.rect_filled(outer, 2.5, fg_tint);
            let left_pane = Rect::from_min_max(outer.min, pos2(pt(0.42, 0.0).x, outer.max.y));
            p.rect_filled(left_pane, 2.5, tint_strong);
            p.rect_stroke(outer, 2.5, st, egui::StrokeKind::Inside);
            line(p, (0.42, 0.12), (0.42, 0.88), ac);
            line(p, (0.20, 0.32), (0.34, 0.32), ac);
            line(p, (0.20, 0.50), (0.34, 0.50), ac);
            line(p, (0.20, 0.68), (0.34, 0.68), ac);
        }
        Icon::ZoomIn => {
            let c = pt(0.42, 0.42);
            p.circle_filled(c, s * 0.28, tint);
            p.circle_stroke(c, s * 0.28, st);
            line(p, (0.62, 0.62), (0.88, 0.88), st_bold);
            line(p, (0.42, 0.27), (0.42, 0.57), ac_bold);
            line(p, (0.27, 0.42), (0.57, 0.42), ac_bold);
        }
        Icon::ZoomOut => {
            let c = pt(0.42, 0.42);
            p.circle_filled(c, s * 0.28, tint);
            p.circle_stroke(c, s * 0.28, st);
            line(p, (0.62, 0.62), (0.88, 0.88), st_bold);
            line(p, (0.27, 0.42), (0.57, 0.42), ac_bold);
        }
        Icon::PageWidth => {
            let sheet = r.shrink2(vec2(s * 0.12, s * 0.18));
            p.rect_filled(sheet, 2.5, tint);
            p.rect_stroke(sheet, 2.5, st, egui::StrokeKind::Inside);
            line(p, (0.22, 0.50), (0.78, 0.50), ac_bold);
            poly(p, &[(0.34, 0.36), (0.20, 0.50), (0.34, 0.64)], ac_bold, false);
            poly(p, &[(0.66, 0.36), (0.80, 0.50), (0.66, 0.64)], ac_bold, false);
        }
        Icon::FullPage => {
            let sheet = r.shrink2(vec2(s * 0.20, s * 0.10));
            p.rect_filled(sheet, 2.5, tint);
            p.rect_stroke(sheet, 2.5, st, egui::StrokeKind::Inside);
            line(p, (0.32, 0.34), (0.68, 0.34), ac);
            line(p, (0.32, 0.50), (0.68, 0.50), st_thin);
            line(p, (0.32, 0.66), (0.56, 0.66), st_thin);
        }
        Icon::Sun => {
            let c = r.center();
            p.circle_filled(c, s * 0.22, tint_strong);
            p.circle_stroke(c, s * 0.22, ac_bold);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * s * 0.31, c + d * s * 0.44], st);
            }
        }
        Icon::Moon => {
            // Precision mathematical crescent moon with soft duotone fill
            let c_outer = pt(0.50, 0.50);
            let r_outer = s * 0.37;
            let c_inner = pt(0.65, 0.36);
            let r_inner = s * 0.31;
            let mut crescent = Vec::with_capacity(34);
            for i in 0..=18 {
                let a = 0.65 + (i as f32 / 18.0) * 4.45;
                crescent.push(c_outer + vec2(a.cos(), a.sin()) * r_outer);
            }
            for i in 0..=14 {
                let a = 4.65 - (i as f32 / 14.0) * 3.55;
                crescent.push(c_inner + vec2(a.cos(), a.sin()) * r_inner);
            }
            p.add(Shape::Path(egui::epaint::PathShape {
                points: crescent.clone(),
                closed: true,
                fill: tint_strong,
                stroke: Stroke::NONE.into(),
            }));
            p.add(Shape::closed_line(crescent, ac_bold));
        }
        Icon::Close => {
            line(p, (0.24, 0.24), (0.76, 0.76), st);
            line(p, (0.76, 0.24), (0.24, 0.76), st);
        }
        Icon::FormatPainter => {
            let roller = Rect::from_min_max(pt(0.14, 0.14), pt(0.72, 0.40));
            p.rect_filled(roller, 3.0, tint_strong);
            p.rect_stroke(roller, 3.0, ac, egui::StrokeKind::Inside);
            poly(p, &[(0.72, 0.27), (0.86, 0.27), (0.86, 0.52), (0.48, 0.52), (0.48, 0.64)], st, false);
            let handle = Rect::from_min_max(pt(0.40, 0.64), pt(0.56, 0.90));
            p.rect_filled(handle, 2.0, fg_tint);
            p.rect_stroke(handle, 2.0, st, egui::StrokeKind::Inside);
        }
        Icon::Link => {
            let left = Rect::from_min_max(pt(0.08, 0.30), pt(0.54, 0.70));
            let right = Rect::from_min_max(pt(0.46, 0.30), pt(0.92, 0.70));
            p.rect_filled(left, s * 0.20, tint);
            p.rect_filled(right, s * 0.20, tint);
            p.rect_stroke(left, s * 0.20, st, egui::StrokeKind::Inside);
            p.rect_stroke(right, s * 0.20, st, egui::StrokeKind::Inside);
            line(p, (0.34, 0.50), (0.66, 0.50), ac_bold);
        }
        Icon::Comment => {
            let bubble = Rect::from_min_max(pt(0.10, 0.14), pt(0.90, 0.72));
            p.rect_filled(bubble, 3.5, tint);
            poly(
                p,
                &[
                    (0.14, 0.14),
                    (0.86, 0.14),
                    (0.86, 0.70),
                    (0.46, 0.70),
                    (0.24, 0.88),
                    (0.26, 0.70),
                    (0.14, 0.70),
                ],
                st,
                true,
            );
            line(p, (0.50, 0.28), (0.50, 0.56), ac_bold);
            line(p, (0.36, 0.42), (0.64, 0.42), ac_bold);
        }
        Icon::SmartChip => {
            // Precision geometric '@' symbol inside a soft duotone circle
            let c = pt(0.50, 0.50);
            p.circle_filled(c, s * 0.39, tint);
            p.circle_stroke(c, s * 0.16, ac_bold);
            line(p, (0.66, 0.34), (0.66, 0.58), ac_bold);
            // Outer circular arc from right-bottom around to bottom-right
            let mut arc = Vec::with_capacity(28);
            for i in 0..=26 {
                let a = -0.35 - (i as f32 / 26.0) * 5.25;
                arc.push(c + vec2(a.cos(), a.sin()) * (s * 0.38));
            }
            p.add(Shape::line(arc, st));
            p.add(bezier_open(
                [pt(0.66, 0.56), pt(0.66, 0.70), pt(0.86, 0.70), pt(0.86, 0.50)],
                ac,
            ));
            line(p, (0.86, 0.50), (0.85, 0.38), st);
        }
        Icon::TableOfContents => {
            for (i, y) in [0.24, 0.50, 0.76].iter().enumerate() {
                let st_row = if i == 0 { ac_bold } else { st };
                line(p, (0.12, *y), (0.54, *y), st_row);
                p.circle_filled(pt(0.68, *y), u * 0.65, accent);
                line(p, (0.78, *y), (0.88, *y), ac);
            }
        }
        Icon::HeaderFooter => {
            let sheet = r.shrink(s * 0.12);
            p.rect_filled(sheet, 2.5, tint);
            let hdr = Rect::from_min_max(sheet.min, pos2(sheet.max.x, pt(0.0, 0.32).y));
            let ftr = Rect::from_min_max(pos2(sheet.min.x, pt(0.0, 0.68).y), sheet.max);
            p.rect_filled(hdr, 2.5, tint_strong);
            p.rect_filled(ftr, 2.5, tint_strong);
            p.rect_stroke(sheet, 2.5, st, egui::StrokeKind::Inside);
            line(p, (0.12, 0.32), (0.88, 0.32), ac);
            line(p, (0.12, 0.68), (0.88, 0.68), ac);
        }
        Icon::PageNumber => {
            draw_page_sheet(p, pt, u, tint, st, accent);
            // Vector '#' octothorpe at bottom center
            line(p, (0.42, 0.48), (0.38, 0.78), ac);
            line(p, (0.58, 0.48), (0.54, 0.78), ac);
            line(p, (0.32, 0.58), (0.64, 0.58), ac);
            line(p, (0.30, 0.68), (0.62, 0.68), ac);
        }
        Icon::WordCount => {
            draw_page_sheet(p, pt, u, tint, st, accent);
            // Analytics / word count bars inside page
            let b1 = Rect::from_min_max(pt(0.30, 0.62), pt(0.40, 0.80));
            let b2 = Rect::from_min_max(pt(0.46, 0.44), pt(0.56, 0.80));
            let b3 = Rect::from_min_max(pt(0.62, 0.52), pt(0.72, 0.80));
            p.rect_filled(b1, 1.0, accent);
            p.rect_filled(b2, 1.0, accent);
            p.rect_filled(b3, 1.0, fg);
        }
        Icon::PageSetup => {
            let sheet = r.shrink(s * 0.12);
            p.rect_filled(sheet, 2.5, tint);
            p.rect_stroke(sheet, 2.5, st, egui::StrokeKind::Inside);
            let margins = r.shrink(s * 0.28);
            p.rect_filled(margins, 1.5, tint_strong);
            p.rect_stroke(margins, 1.5, ac, egui::StrokeKind::Inside);
        }
        Icon::Rename => {
            // Tilted fountain/edit pencil over an input baseline
            fill_poly(
                p,
                &[(0.64, 0.14), (0.84, 0.34), (0.38, 0.80), (0.18, 0.82), (0.20, 0.62)],
                tint_strong,
                st,
            );
            line(p, (0.54, 0.24), (0.74, 0.44), ac);
            line(p, (0.14, 0.90), (0.86, 0.90), ac_bold);
        }
        Icon::Trash => {
            let bin = Rect::from_min_max(pt(0.24, 0.26), pt(0.76, 0.88));
            p.rect_filled(bin, 2.0, tint);
            line(p, (0.14, 0.26), (0.86, 0.26), st_bold);
            poly(p, &[(0.36, 0.26), (0.36, 0.14), (0.64, 0.14), (0.64, 0.26)], ac, false);
            poly(p, &[(0.24, 0.26), (0.28, 0.88), (0.72, 0.88), (0.76, 0.26)], st, false);
            line(p, (0.42, 0.40), (0.42, 0.74), ac);
            line(p, (0.58, 0.40), (0.58, 0.74), ac);
        }
        Icon::Star | Icon::StarFilled => {
            let c = r.center();
            let mut pts = Vec::with_capacity(10);
            for k in 0..10 {
                let a = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 5.0;
                let rad = if k % 2 == 0 { s * 0.42 } else { s * 0.18 };
                pts.push(c + vec2(a.cos() * rad, a.sin() * rad));
            }
            if icon == Icon::StarFilled {
                let gold = Color32::from_rgb(245, 166, 35);
                p.add(Shape::convex_polygon(pts.clone(), gold.gamma_multiply(0.35), Stroke::NONE));
                p.add(Shape::closed_line(pts, Stroke::new(u * 1.15, gold)));
            } else {
                p.add(Shape::closed_line(pts, st));
            }
        }
        Icon::VersionHistory => {
            let c = r.center();
            p.circle_filled(c, s * 0.36, tint);
            p.circle_stroke(c, s * 0.36, st);
            poly(p, &[(0.50, 0.26), (0.50, 0.52), (0.68, 0.62)], ac_bold, false);
            poly(p, &[(0.12, 0.24), (0.14, 0.44), (0.34, 0.40)], ac, false);
        }
        Icon::EditMode => {
            fill_poly(
                p,
                &[(0.66, 0.12), (0.88, 0.34), (0.36, 0.86), (0.14, 0.86), (0.14, 0.64)],
                tint_strong,
                st,
            );
            line(p, (0.54, 0.24), (0.76, 0.46), ac);
        }
        Icon::SuggestMode => {
            let card = Rect::from_min_max(pt(0.12, 0.16), pt(0.88, 0.84));
            p.rect_filled(card, 3.0, tint);
            poly(p, &[(0.14, 0.38), (0.14, 0.16), (0.86, 0.16), (0.86, 0.84), (0.58, 0.84)], st, false);
            poly(p, &[(0.14, 0.72), (0.48, 0.38), (0.62, 0.52), (0.28, 0.86), (0.14, 0.86)], ac, true);
        }
        Icon::ViewMode => {
            // Eye with duotone iris
            let top_arc = [pt(0.08, 0.50), pt(0.30, 0.18), pt(0.70, 0.18), pt(0.92, 0.50)];
            let bot_arc = [pt(0.92, 0.50), pt(0.70, 0.82), pt(0.30, 0.82), pt(0.08, 0.50)];
            p.add(bezier_closed(top_arc, tint, Stroke::NONE));
            p.add(bezier_closed(bot_arc, tint, Stroke::NONE));
            p.add(bezier_open(top_arc, st));
            p.add(bezier_open(bot_arc, st));
            p.circle_filled(r.center(), s * 0.16, tint_strong);
            p.circle_stroke(r.center(), s * 0.16, ac_bold);
            p.circle_filled(r.center(), s * 0.06, accent);
        }
        Icon::Bookmark => {
            fill_poly(
                p,
                &[(0.24, 0.12), (0.76, 0.12), (0.76, 0.88), (0.50, 0.68), (0.24, 0.88)],
                tint_strong,
                st,
            );
            line(p, (0.34, 0.26), (0.66, 0.26), ac_bold);
        }
        Icon::Citation | Icon::Quote => {
            // Sculpted editorial quotation marks with duotone badge frame
            let card = r.shrink(s * 0.10);
            p.rect_filled(card, 3.5, tint);
            for dx in [0.0, 0.34] {
                let head = pt(0.33 + dx, 0.40);
                p.circle_filled(head, s * 0.11, accent);
                p.add(bezier_open(
                    [
                        pt(0.41 + dx, 0.40),
                        pt(0.43 + dx, 0.58),
                        pt(0.34 + dx, 0.68),
                        pt(0.24 + dx, 0.72),
                    ],
                    ac_bold,
                ));
            }
        }
        Icon::Dictionary => {
            let book = Rect::from_min_max(pt(0.18, 0.12), pt(0.82, 0.88));
            p.rect_filled(book, 2.5, tint);
            p.rect_stroke(book, 2.5, st, egui::StrokeKind::Inside);
            let spine = Rect::from_min_max(pt(0.18, 0.12), pt(0.34, 0.88));
            p.rect_filled(spine, 2.0, tint_strong);
            line(p, (0.34, 0.12), (0.34, 0.88), ac);
            line(p, (0.46, 0.34), (0.70, 0.34), ac_bold);
            line(p, (0.46, 0.50), (0.70, 0.50), st_thin);
        }
        Icon::Preferences => {
            // Sliders / tuning controls (cleaner and sharper at small sizes than a 6-spoke cog)
            for (i, (y, kx)) in [(0.24, 0.36), (0.50, 0.66), (0.76, 0.42)].iter().enumerate() {
                line(p, (0.12, *y), (0.88, *y), st);
                let knob = pt(*kx, *y);
                p.circle_filled(knob, s * 0.12, if i == 1 { accent } else { tint_strong });
                p.circle_stroke(knob, s * 0.12, if i == 1 { ac_bold } else { st });
            }
        }
        Icon::Shortcuts => {
            let kb = Rect::from_min_max(pt(0.08, 0.22), pt(0.92, 0.80));
            p.rect_filled(kb, 3.0, tint);
            p.rect_stroke(kb, 3.0, st, egui::StrokeKind::Inside);
            for x in [0.24, 0.42, 0.58, 0.76] {
                p.circle_filled(pt(x, 0.40), u * 0.75, fg);
            }
            line(p, (0.28, 0.64), (0.72, 0.64), ac_bold);
        }
        Icon::About => {
            let c = r.center();
            p.circle_filled(c, s * 0.39, tint);
            p.circle_stroke(c, s * 0.39, st);
            p.circle_filled(pt(0.50, 0.31), u * 0.9, accent);
            line(p, (0.50, 0.46), (0.50, 0.74), ac_bold);
        }
        Icon::Code => {
            poly(p, &[(0.34, 0.26), (0.12, 0.50), (0.34, 0.74)], st_bold, false);
            poly(p, &[(0.66, 0.26), (0.88, 0.50), (0.66, 0.74)], st_bold, false);
            line(p, (0.56, 0.20), (0.44, 0.80), ac_bold);
        }
        Icon::Pageless => {
            let wave = Rect::from_min_max(pt(0.14, 0.14), pt(0.86, 0.86));
            p.rect_filled(wave, 3.5, tint);
            line(p, (0.16, 0.14), (0.84, 0.14), st);
            line(p, (0.16, 0.86), (0.84, 0.86), st);
            line(p, (0.24, 0.36), (0.76, 0.36), ac_bold);
            line(p, (0.24, 0.52), (0.76, 0.52), st_thin);
            line(p, (0.24, 0.68), (0.60, 0.68), st_thin);
        }
        Icon::Columns => {
            let c1 = Rect::from_min_max(pt(0.12, 0.16), pt(0.46, 0.84));
            let c2 = Rect::from_min_max(pt(0.54, 0.16), pt(0.88, 0.84));
            p.rect_filled(c1, 2.0, tint_strong);
            p.rect_filled(c2, 2.0, tint);
            p.rect_stroke(c1, 2.0, ac, egui::StrokeKind::Inside);
            p.rect_stroke(c2, 2.0, st, egui::StrokeKind::Inside);
        }
        Icon::Summary => {
            draw_page_sheet(p, pt, u, tint, st, accent);
            line(p, (0.30, 0.44), (0.70, 0.44), ac_bold);
            line(p, (0.30, 0.58), (0.70, 0.58), st_thin);
            line(p, (0.30, 0.72), (0.54, 0.72), st_thin);
        }
        Icon::ChevronUp => {
            poly(p, &[(0.22, 0.64), (0.50, 0.34), (0.78, 0.64)], st_bold, false);
        }
        Icon::ChevronDown => {
            poly(p, &[(0.22, 0.36), (0.50, 0.66), (0.78, 0.36)], st_bold, false);
        }
        Icon::Check => {
            p.circle_filled(r.center(), s * 0.38, tint_strong);
            poly(p, &[(0.24, 0.52), (0.43, 0.72), (0.78, 0.30)], ac_bold, false);
        }
        Icon::Plus => {
            line(p, (0.50, 0.22), (0.50, 0.78), st_bold);
            line(p, (0.22, 0.50), (0.78, 0.50), st_bold);
        }
        Icon::Minus => {
            line(p, (0.22, 0.50), (0.78, 0.50), st_bold);
        }
        Icon::Table => {
            let tbl = r.shrink(s * 0.12);
            p.rect_filled(tbl, 2.5, tint);
            let hdr = Rect::from_min_max(tbl.min, pos2(tbl.max.x, pt(0.0, 0.38).y));
            p.rect_filled(hdr, 2.5, tint_strong);
            p.rect_stroke(tbl, 2.5, st, egui::StrokeKind::Inside);
            line(p, (0.12, 0.38), (0.88, 0.38), ac);
            line(p, (0.12, 0.63), (0.88, 0.63), st_thin);
            line(p, (0.50, 0.12), (0.50, 0.88), st_thin);
        }
        Icon::BuildingBlock => {
            // Isometric/modular stacked studio blocks
            let b1 = Rect::from_min_max(pt(0.14, 0.14), pt(0.56, 0.48));
            let b2 = Rect::from_min_max(pt(0.14, 0.54), pt(0.48, 0.86));
            let b3 = Rect::from_min_max(pt(0.54, 0.42), pt(0.88, 0.86));
            p.rect_filled(b1, 2.5, tint_strong);
            p.rect_stroke(b1, 2.5, ac, egui::StrokeKind::Inside);
            p.rect_filled(b2, 2.5, tint);
            p.rect_stroke(b2, 2.5, st, egui::StrokeKind::Inside);
            p.rect_filled(b3, 2.5, tint);
            p.rect_stroke(b3, 2.5, st, egui::StrokeKind::Inside);
        }
        Icon::Footnote => {
            draw_page_sheet(p, pt, u, tint, st, accent);
            line(p, (0.30, 0.68), (0.70, 0.68), ac);
            // Asterisk / footnote reference mark
            line(p, (0.38, 0.42), (0.38, 0.58), ac_bold);
            line(p, (0.30, 0.50), (0.46, 0.50), ac_bold);
        }
        Icon::Image => {
            let frame = Rect::from_min_max(pt(0.12, 0.16), pt(0.88, 0.84));
            p.rect_filled(frame, 3.0, tint);
            // Mountain landscape silhouette
            fill_poly(
                p,
                &[(0.14, 0.82), (0.38, 0.50), (0.56, 0.82)],
                tint_strong,
                Stroke::NONE,
            );
            fill_poly(
                p,
                &[(0.48, 0.82), (0.70, 0.56), (0.86, 0.82)],
                tint_strong,
                Stroke::NONE,
            );
            poly(
                p,
                &[(0.15, 0.80), (0.38, 0.50), (0.56, 0.70), (0.70, 0.56), (0.85, 0.80)],
                ac,
                false,
            );
            // Warm sun disc
            p.circle_filled(pt(0.66, 0.34), s * 0.085, accent);
            p.rect_stroke(frame, 3.0, st, egui::StrokeKind::Inside);
        }
    }
}

//! HTML export that reproduces the page as laid out in the editor: page size and margins,
//! fonts, sizes, colors, highlights, indents, list markers, tab stops, borders, shading,
//! tables, images, headers and footers.

use crate::doc::*;

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\n' => o.push_str("<br>"),
            c => o.push(c),
        }
    }
    o
}

fn css_rgb(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn font_stack(f: u16) -> String {
    let generic = match font_category(f) {
        FontCat::Serif => "serif",
        FontCat::Mono => "monospace",
        FontCat::Script => "cursive",
        FontCat::Sans => "sans-serif",
    };
    format!("'{}',{generic}", font_name(f).replace('\'', ""))
}

fn char_css(s: &CharStyle) -> String {
    let mut css = format!(
        "font-family:{};font-size:{}pt;",
        font_stack(s.font),
        trim_num(s.size)
    );
    if s.bold {
        css.push_str("font-weight:bold;");
    }
    if s.italic {
        css.push_str("font-style:italic;");
    }
    let mut deco = Vec::new();
    if s.underline || s.link > 0 {
        deco.push("underline");
    }
    if s.strike {
        deco.push("line-through");
    }
    if !deco.is_empty() {
        css.push_str(&format!("text-decoration:{};", deco.join(" ")));
    }
    match s.caps {
        1 => css.push_str("text-transform:uppercase;"),
        2 => css.push_str("font-variant:small-caps;"),
        _ => {}
    }
    if s.spacing_pt != 0.0 {
        css.push_str(&format!("letter-spacing:{}pt;", trim_num(s.spacing_pt)));
    }
    if s.script != 0 {
        css.push_str(&format!(
            "vertical-align:{};font-size:{}pt;",
            if s.script > 0 { "super" } else { "sub" },
            trim_num(s.size * 0.65)
        ));
    }
    match (s.color_to, s.color) {
        (Some(c2), c1) if c2 != c1 => css.push_str(&format!(
            "background-image:linear-gradient(90deg,{},{});-webkit-background-clip:text;background-clip:text;-webkit-text-fill-color:transparent;",
            css_rgb(c1),
            css_rgb(c2)
        )),
        (_, c) => {
            let c = if s.link > 0 && c == [0, 0, 0] { [11, 87, 208] } else { c };
            css.push_str(&format!("color:{};", css_rgb(c)));
        }
    }
    match (s.highlight, s.highlight_to) {
        (Some(a), Some(b)) if a != b => css.push_str(&format!(
            "background:linear-gradient(90deg,{},{});",
            css_rgb(a),
            css_rgb(b)
        )),
        (Some(a), _) => css.push_str(&format!("background-color:{};", css_rgb(a))),
        _ => {}
    }
    css
}

fn trim_num(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn border_css(side: &str, b: Option<BorderSpec>) -> String {
    match b {
        Some(b) => format!(
            "border-{side}:{}pt solid {};",
            trim_num(b.width_pt),
            css_rgb(b.color)
        ),
        None => String::new(),
    }
}

fn image_tag(img: &DocImage, extra: &str) -> String {
    let mime = match base64_decode(&img.png_b64).as_deref() {
        Some([0xFF, 0xD8, ..]) => "image/jpeg",
        Some([b'G', b'I', b'F', ..]) => "image/gif",
        _ => "image/png",
    };
    format!(
        "<img src=\"data:{mime};base64,{}\" alt=\"{}\" style=\"width:{}in;height:{}in;max-width:100%;{extra}\">",
        img.png_b64,
        esc(&img.name),
        trim_num(img.width_in),
        trim_num(img.height_in)
    )
}

fn para_html(doc: &Document, pi: usize, out: &mut String) {
    let p = &doc.paras[pi];
    let f = p.fmt;
    let (left, first) = doc.indents_in(&f);
    let align = match f.align {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
        Align::Justify => "justify",
    };
    let line_height = if f.line_exact_pt > 0.0 {
        format!("{}pt", trim_num(f.line_exact_pt))
    } else {
        // the editor's single spacing is the font's natural line height (≈ 1.15 em)
        trim_num(f.line_spacing * 1.15)
    };
    let mut css = format!(
        "margin:{}pt {}in {}pt {}in;text-indent:{}in;text-align:{align};line-height:{line_height};",
        trim_num(f.space_before),
        trim_num(f.right_in),
        trim_num(f.space_after),
        trim_num(left),
        trim_num(first)
    );
    if f.line_min_pt > 0.0 {
        css.push_str(&format!("min-height:{}pt;", trim_num(f.line_min_pt)));
    }
    if let Some(shd) = f.shading {
        css.push_str(&format!(
            "background-color:{};padding:1pt 4pt;",
            css_rgb(shd)
        ));
    }
    if f.page_break_before {
        css.push_str("break-before:page;");
    }
    css.push_str(&border_css("top", f.pborders.top));
    css.push_str(&border_css("bottom", f.pborders.bottom));
    css.push_str(&border_css("left", f.pborders.left));
    css.push_str(&border_css("right", f.pborders.right));
    if !f.pborders.is_empty() {
        css.push_str("padding:1pt 4pt;");
    }
    if p.text.contains(&'\t') {
        css.push_str(&format!(
            "white-space:pre-wrap;tab-size:{}in;",
            trim_num(doc.default_tab_in)
        ));
    }
    if f.hr && p.text.is_empty() {
        let c = f.hr_color.unwrap_or([203, 213, 225]);
        out.push_str(&format!(
            "<hr style=\"border:0;border-top:{}pt solid {};margin:{}pt 0 {}pt 0;\">\n",
            trim_num(if f.hr_width_pt > 0.0 {
                f.hr_width_pt
            } else {
                0.75
            }),
            css_rgb(c),
            trim_num(f.space_before + 6.0),
            trim_num(f.space_after + 6.0)
        ));
        return;
    }
    let tag = match named_styles().get(f.style).map(|s| s.name) {
        Some("Title") | Some("Heading 1") => "h1",
        Some("Subtitle") | Some("Heading 2") => "h2",
        Some("Heading 3") => "h3",
        Some("Quote") => "blockquote",
        _ => "p",
    };
    if f.style == 7 {
        css.push_str("border-left:3.5px solid #2b5797;padding-left:9px;");
    }
    out.push_str(&format!("<{tag} style=\"{css}\">"));
    if f.image_id > 0 {
        if let Some(img) = doc.image(f.image_id) {
            out.push_str(&image_tag(img, "vertical-align:bottom;"));
            if !img.caption.trim().is_empty() {
                out.push_str(&format!("<br><span style=\"font-style:italic;color:#5a606c;font-size:10.5pt;\">{}</span>", esc(img.caption.trim())));
            }
        }
    }
    if f.list != ListKind::None {
        let marker = doc.list_marker(pi);
        let mut ms = CharStyle {
            underline: false,
            strike: false,
            highlight: None,
            highlight_to: None,
            link: 0,
            ..p.style_at(0)
        };
        if let Some(lv) = doc.list_level(&f) {
            if let Some(font) = lv.font {
                ms.font = font;
            }
            if let Some(c) = lv.color {
                ms.color = c;
            }
            if let Some(sz) = lv.size {
                ms.size = sz;
            }
        }
        let hang = (-first).max(0.0);
        let shown = if f.list == ListKind::Checklist {
            if f.checked { "☑" } else { "☐" }.to_string()
        } else {
            marker
        };
        out.push_str(&format!(
            "<span style=\"{}display:inline-block;min-width:{}in;text-indent:0;\">{}</span>",
            char_css(&ms),
            trim_num(hang),
            esc(&shown)
        ));
    }
    if p.text.is_empty() {
        out.push_str(&format!(
            "<span style=\"{}\">&#8203;</span>",
            char_css(&p.end_style)
        ));
    }
    let checked = f.list == ListKind::Checklist && f.checked;
    for (a, b, s) in p.runs() {
        let s = if checked {
            CharStyle {
                strike: true,
                color: if s.color == [0, 0, 0] {
                    [110, 118, 129]
                } else {
                    s.color
                },
                ..s
            }
        } else {
            s
        };
        let txt: String = p.text[a..b].iter().collect();
        let span = format!("<span style=\"{}\">{}</span>", char_css(&s), esc(&txt));
        let span = match s.script {
            1 => format!("<sup>{span}</sup>"),
            -1 => format!("<sub>{span}</sub>"),
            _ => span,
        };
        if let Some(url) = doc.link_url(s.link) {
            out.push_str(&format!("<a href=\"{}\">{span}</a>", esc(url)));
        } else {
            out.push_str(&span);
        }
    }
    out.push_str(&format!("</{tag}>\n"));
}

pub fn to_html(doc: &Document) -> String {
    let pg = doc.page;
    let title = if doc.info.title.is_empty() {
        "Document"
    } else {
        &doc.info.title
    };
    let body_font = doc
        .paras
        .first()
        .map_or(CharStyle::default(), |p| p.style_at(0));
    let mut out = format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><meta name=\"generator\" content=\"Syllabary\"><title>{}</title>\n<style>\n\
@page {{ size: {w}in {h}in; margin: {mt}in {mr}in {mb}in {ml}in; }}\n\
html {{ background: #e8eaed; }}\n\
body {{ margin: 0; font-family: {ff}; font-size: {fs}pt; color: #000; }}\n\
.page {{ box-sizing: border-box; width: {w}in; min-height: {h}in; margin: 24px auto; padding: {mt}in {mr}in {mb}in {ml}in; background: {bg}; box-shadow: 0 1px 3px rgba(0,0,0,.25); position: relative; }}\n\
.page p, .page h1, .page h2, .page h3, .page blockquote {{ font-size: inherit; font-weight: normal; }}\n\
.hdr, .ftr {{ position: absolute; left: {ml}in; right: {mr}in; font-size: 9pt; color: #555; }}\n\
.hdr {{ top: 0.45in; }} .ftr {{ bottom: 0.45in; }}\n\
table.doc {{ border-collapse: collapse; table-layout: fixed; }}\n\
table.doc td {{ vertical-align: top; overflow-wrap: anywhere; }}\n\
a {{ color: inherit; }}\n\
@media print {{ html {{ background: none; }} .page {{ margin: 0; box-shadow: none; width: auto; min-height: 0; padding: 0; }} .hdr, .ftr {{ display: none; }} }}\n\
</style></head><body>\n<div class=\"page\">\n",
        esc(title),
        w = trim_num(pg.width),
        h = trim_num(pg.height),
        mt = trim_num(pg.margin_top),
        mr = trim_num(pg.margin_right),
        mb = trim_num(pg.margin_bottom),
        ml = trim_num(pg.margin_left),
        ff = font_stack(body_font.font),
        fs = trim_num(body_font.size),
        bg = css_rgb(pg.background)
    );
    let hf_align = |a: Align| match a {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
        Align::Justify => "justify",
    };
    if !doc.header.is_empty() {
        out.push_str(&format!(
            "<div class=\"hdr\" style=\"text-align:{};{}{}\">{}</div>\n",
            hf_align(doc.header_align),
            char_css(&crate::layout::hf_style(doc, false)),
            if doc.header_dist_in > 0.0 {
                format!("top:{}in;", trim_num(doc.header_dist_in))
            } else {
                String::new()
            },
            esc(&doc.header)
        ));
    }
    let mut pi = 0;
    while pi < doc.paras.len() {
        let p = &doc.paras[pi];
        if p.fmt.table_id == 0 {
            para_html(doc, pi, &mut out);
            pi += 1;
            continue;
        }
        let tid = p.fmt.table_id;
        let mut end = pi + 1;
        while end < doc.paras.len() && doc.paras[end].fmt.table_id == tid {
            end += 1;
        }
        if let Some(tbl) = doc.table(tid) {
            out.push_str(&format!(
                "<table class=\"doc\" style=\"margin-left:{}in;\"><colgroup>",
                trim_num(tbl.indent_in)
            ));
            for w in &tbl.grid_cols_in {
                out.push_str(&format!("<col style=\"width:{}in\">", trim_num(*w)));
            }
            out.push_str("</colgroup>\n");
            for (r, row) in tbl.rows.iter().enumerate() {
                out.push_str(&format!(
                    "<tr style=\"height:{}pt\">",
                    trim_num(row.min_height_pt)
                ));
                for (c, cell) in row.cells.iter().enumerate() {
                    let mut css = format!(
                        "width:{}in;padding:{}in {}in {}in {}in;vertical-align:{};",
                        trim_num(cell.width_in),
                        trim_num(cell.pad_top_in),
                        trim_num(cell.pad_right_in),
                        trim_num(cell.pad_bottom_in),
                        trim_num(cell.pad_left_in),
                        match cell.v_align {
                            CellVAlign::Top => "top",
                            CellVAlign::Center => "middle",
                            CellVAlign::Bottom => "bottom",
                        }
                    );
                    css.push_str(&border_css("top", cell.borders.top));
                    css.push_str(&border_css("bottom", cell.borders.bottom));
                    css.push_str(&border_css("left", cell.borders.left));
                    css.push_str(&border_css("right", cell.borders.right));
                    if let Some(s) = cell.shading {
                        css.push_str(&format!("background-color:{};", css_rgb(s)));
                    }
                    if !cell.wrap_text {
                        css.push_str("white-space:nowrap;");
                    }
                    let span = if cell.grid_span > 1 {
                        format!(" colspan=\"{}\"", cell.grid_span)
                    } else {
                        String::new()
                    };
                    out.push_str(&format!("<td{span} style=\"{css}\">"));
                    for q in pi..end {
                        let qf = doc.paras[q].fmt;
                        if qf.table_row as usize == r && qf.table_col as usize == c {
                            para_html(doc, q, &mut out);
                        }
                    }
                    out.push_str("</td>");
                }
                out.push_str("</tr>\n");
            }
            out.push_str("</table>\n");
        } else {
            for q in pi..end {
                para_html(doc, q, &mut out);
            }
        }
        pi = end;
    }
    if !doc.footer.is_empty() || pg.page_numbers {
        let text = if doc.footer.is_empty() {
            "1".to_string()
        } else {
            doc.footer.clone()
        };
        out.push_str(&format!(
            "<div class=\"ftr\" style=\"text-align:{};{}{}\">{}</div>\n",
            hf_align(if doc.footer.is_empty() {
                Align::Center
            } else {
                doc.footer_align
            }),
            char_css(&crate::layout::hf_style(doc, true)),
            if doc.footer_dist_in > 0.0 {
                format!("bottom:{}in;", trim_num(doc.footer_dist_in))
            } else {
                String::new()
            },
            esc(&text)
        ));
    }
    out.push_str("</div>\n</body></html>\n");
    out
}

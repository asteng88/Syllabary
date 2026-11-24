//! Markdown (CommonMark + GitHub tables / task lists / strikethrough) reader and writer.

use crate::doc::*;

const CODE_BG: [u8; 3] = [241, 245, 249];

fn code_font() -> u16 {
    font_index("Consolas")
}

fn is_code_font(f: u16) -> bool {
    font_category(f) == FontCat::Mono
}

// ───────────────────────── writer ─────────────────────────

/// Escapes characters that would otherwise start Markdown syntax.
fn md_escape(s: &str, at_line_start: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().enumerate() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '~' => {
                out.push('\\');
                out.push(c);
            }
            '#' | '+' | '-' | '=' if i == 0 && at_line_start => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

fn inline_md(doc: &Document, p: &Para, style_bold: bool, style_italic: bool) -> String {
    let mut out = String::new();
    let runs = p.runs();
    for (k, (a, b, s)) in runs.iter().enumerate() {
        let raw: String = p.text[*a..*b].iter().collect();
        let mono = is_code_font(s.font);
        let mut parts: Vec<String> = Vec::new();
        for (li, line) in raw.split('\n').enumerate() {
            if li > 0 {
                parts.push("  \n".into());
            }
            if line.is_empty() {
                continue;
            }
            let body = if mono {
                let ticks = if line.contains('`') { "``" } else { "`" };
                // CommonMark strips one space from each side when both are present
                if line.starts_with([' ', '`']) || line.ends_with([' ', '`']) {
                    format!("{ticks} {line} {ticks}")
                } else {
                    format!("{ticks}{line}{ticks}")
                }
            } else {
                let at_start = k == 0 && li == 0 && out.is_empty();
                let mut e = md_escape(line, at_start).replace('\t', "&#9;");
                if at_start || li > 0 {
                    let n = e.chars().take_while(|c| *c == ' ').count();
                    if n > 0 {
                        e = format!("{}{}", "&#32;".repeat(n), &e[n..]);
                    }
                }
                e
            };
            // keep surrounding spaces outside the emphasis markers (CommonMark flanking rules)
            let lead: String = body.chars().take_while(|c| *c == ' ').collect();
            let trail: String = body.chars().rev().take_while(|c| *c == ' ').collect();
            let core = body.trim_matches(' ').to_string();
            if core.is_empty() {
                parts.push(body);
                continue;
            }
            let mut seg = core;
            if s.script > 0 {
                seg = format!("<sup>{seg}</sup>");
            } else if s.script < 0 {
                seg = format!("<sub>{seg}</sub>");
            }
            if s.underline && s.link == 0 {
                seg = format!("<u>{seg}</u>");
            }
            if s.strike {
                seg = format!("~~{seg}~~");
            }
            if s.italic && !style_italic {
                seg = format!("*{seg}*");
            }
            if s.bold && !style_bold {
                seg = format!("**{seg}**");
            }
            if s.highlight.is_some() && !mono {
                seg = format!("<mark>{seg}</mark>");
            }
            if let Some(url) = doc.link_url(s.link) {
                seg = format!("[{seg}]({})", url.replace(' ', "%20").replace(')', "%29"));
            }
            parts.push(format!("{lead}{seg}{trail}"));
        }
        out.push_str(&parts.concat());
    }
    out
}

fn image_md(img: &DocImage) -> String {
    let alt = if img.caption.trim().is_empty() {
        img.name.clone()
    } else {
        img.caption.trim().to_string()
    };
    let mime = match base64_decode(&img.png_b64).as_deref() {
        Some([0xFF, 0xD8, ..]) => "image/jpeg",
        _ => "image/png",
    };
    format!(
        "![{}](data:{mime};base64,{})",
        alt.replace([']', '['], ""),
        img.png_b64
    )
}

pub fn to_markdown(doc: &Document) -> String {
    let styles = named_styles();
    let mut out = String::new();
    let mut pi = 0;
    while pi < doc.paras.len() {
        let p = &doc.paras[pi];
        if p.fmt.table_id > 0 {
            let tid = p.fmt.table_id;
            let mut end = pi + 1;
            while end < doc.paras.len() && doc.paras[end].fmt.table_id == tid {
                end += 1;
            }
            let nrows = doc.paras[pi..end]
                .iter()
                .map(|x| x.fmt.table_row as usize + 1)
                .max()
                .unwrap_or(1);
            let ncols = doc
                .table(tid)
                .map(|t| t.rows.iter().map(|r| r.cells.len()).max().unwrap_or(1))
                .unwrap_or_else(|| {
                    doc.paras[pi..end]
                        .iter()
                        .map(|x| x.fmt.table_col as usize + 1)
                        .max()
                        .unwrap_or(1)
                })
                .max(1);
            for r in 0..nrows {
                out.push('|');
                for c in 0..ncols {
                    let cell: Vec<String> = doc.paras[pi..end]
                        .iter()
                        .filter(|cp| {
                            cp.fmt.table_row as usize == r && cp.fmt.table_col as usize == c
                        })
                        .map(|cp| {
                            inline_md(doc, cp, r == 0, false)
                                .replace("  \n", "<br>")
                                .replace('\n', " ")
                        })
                        .collect();
                    out.push(' ');
                    out.push_str(&cell.join("<br>"));
                    out.push_str(" |");
                }
                out.push('\n');
                if r == 0 {
                    out.push('|');
                    for c in 0..ncols {
                        let al = doc.paras[pi..end]
                            .iter()
                            .find(|cp| cp.fmt.table_col as usize == c && cp.fmt.table_row == 1)
                            .map(|cp| cp.fmt.align);
                        out.push_str(match al {
                            Some(Align::Center) => " :---: |",
                            Some(Align::Right) => " ---: |",
                            _ => " --- |",
                        });
                    }
                    out.push('\n');
                }
            }
            out.push('\n');
            pi = end;
            continue;
        }
        if p.fmt.hr && p.text.is_empty() {
            out.push_str("---\n\n");
            pi += 1;
            continue;
        }
        if p.fmt.image_id > 0 {
            if let Some(img) = doc.image(p.fmt.image_id) {
                out.push_str(&image_md(img));
                out.push_str("\n\n");
                pi += 1;
                continue;
            }
        }
        let sname = styles.get(p.fmt.style).map_or("Default Text", |s| s.name);
        // Code blocks: consecutive Code-style paragraphs become one fenced block.
        if sname == "Code"
            || (!p.text.is_empty()
                && p.styles.iter().all(|s| is_code_font(s.font))
                && p.fmt.shading.is_some())
        {
            out.push_str("```\n");
            while pi < doc.paras.len() {
                let q = &doc.paras[pi];
                let qn = styles.get(q.fmt.style).map_or("", |s| s.name);
                if !(qn == "Code"
                    || (!q.text.is_empty()
                        && q.styles.iter().all(|s| is_code_font(s.font))
                        && q.fmt.shading.is_some()))
                    || q.fmt.table_id > 0
                {
                    break;
                }
                out.push_str(&q.string());
                out.push('\n');
                pi += 1;
            }
            out.push_str("```\n\n");
            continue;
        }
        let (style_bold, style_italic) = styles
            .get(p.fmt.style)
            .map_or((false, false), |ns| (ns.chr.bold, ns.chr.italic));
        let mut prefix = String::new();
        match sname {
            "Title" | "Heading 1" => prefix.push_str("# "),
            "Subtitle" | "Heading 2" => prefix.push_str("## "),
            "Heading 3" => prefix.push_str("### "),
            "Quote" => prefix.push_str("> "),
            _ => {}
        }
        let is_heading = prefix.starts_with('#');
        if p.fmt.list != ListKind::None {
            let indent = "    ".repeat(p.fmt.indent as usize);
            match p.fmt.list {
                ListKind::Bullet => prefix.push_str(&format!("{indent}- ")),
                ListKind::Number => {
                    let marker = doc.list_marker(pi);
                    let n: String = marker.chars().filter(|c| c.is_ascii_digit()).collect();
                    let n = if n.is_empty() {
                        doc.list_number(pi).to_string()
                    } else {
                        n
                    };
                    prefix.push_str(&format!("{indent}{n}. "));
                }
                ListKind::Checklist => prefix.push_str(&format!(
                    "{indent}- [{}] ",
                    if p.fmt.checked { "x" } else { " " }
                )),
                ListKind::None => {}
            }
        }
        let body = inline_md(doc, p, style_bold || is_heading, style_italic);
        if body.is_empty() && p.fmt.list == ListKind::None && prefix.is_empty() {
            // an intentionally empty paragraph
            out.push_str("&nbsp;\n\n");
            pi += 1;
            continue;
        }
        let body = if sname == "Quote" {
            body.replace("  \n", "  \n> ")
        } else {
            body
        };
        out.push_str(&prefix);
        out.push_str(&body);
        out.push('\n');
        let next_is_list = doc
            .paras
            .get(pi + 1)
            .is_some_and(|np| np.fmt.list != ListKind::None && np.fmt.table_id == 0);
        let next_is_quote = doc
            .paras
            .get(pi + 1)
            .is_some_and(|np| styles.get(np.fmt.style).is_some_and(|s| s.name == "Quote"));
        if p.fmt.list != ListKind::None && next_is_list {
            // tight list
        } else if sname == "Quote" && next_is_quote {
            out.push_str(">\n");
        } else {
            out.push('\n');
        }
        pi += 1;
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}


// ───────────────────────── reader (pulldown-cmark CommonMark + GFM) ─────────────────────────

use pulldown_cmark::{Alignment, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

fn heading_style(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 4,
        HeadingLevel::H2 => 5,
        _ => 6,
    }
}

/// Paragraph spacing that matches how Markdown is usually rendered.
const MD_GAP: f32 = 8.0;

#[derive(Clone)]
struct ListState {
    kind: ListKind,
    list_id: u16,
}

#[derive(Default)]
struct TableBuilder {
    aligns: Vec<Align>,
    rows: Vec<Vec<Vec<(char, CharStyle)>>>,
    cur_row: Vec<Vec<(char, CharStyle)>>,
    cur_cell: Vec<(char, CharStyle)>,
    in_head: bool,
    in_cell: bool,
}

fn push_styled_str(target_text: &mut Vec<char>, target_styles: &mut Vec<CharStyle>, s: &str, st: CharStyle) {
    for c in s.chars() {
        target_text.push(c);
        target_styles.push(st);
    }
}

fn push_code_block(doc: &mut Document, code: &str) {
    let st = CharStyle {
        font: code_font(),
        size: 10.0,
        ..CharStyle::default()
    };
    let trimmed = code.strip_suffix('\n').unwrap_or(code);
    let lines: Vec<&str> = if trimmed.is_empty() {
        Vec::new()
    } else {
        trimmed.split('\n').collect()
    };
    let n = lines.len().max(1);
    for (k, l) in lines.iter().enumerate() {
        let mut p = Para::new(
            st,
            ParaFmt {
                style: 10,
                shading: Some(CODE_BG),
                space_after: if k + 1 == n { MD_GAP } else { 0.0 },
                ..ParaFmt::default()
            },
        );
        push_styled_str(&mut p.text, &mut p.styles, l, st);
        doc.paras.push(p);
    }
    if lines.is_empty() {
        doc.paras.push(Para::new(
            st,
            ParaFmt {
                style: 10,
                shading: Some(CODE_BG),
                space_after: MD_GAP,
                ..ParaFmt::default()
            },
        ));
    }
}

fn add_image(doc: &mut Document, alt: &str, src: &str) -> Option<u64> {
    let b64 = src.split_once("base64,").map(|(_, b)| b.trim().to_string())?;
    let bytes = base64_decode(&b64)?;
    let img = image::load_from_memory(&bytes).ok()?;
    let id = doc.images.iter().map(|i| i.id).max().unwrap_or(0) + 1;
    let (w, h) = (img.width().max(1), img.height().max(1));
    let max_w = (doc.page.width - doc.page.margin_left - doc.page.margin_right).max(1.0);
    let w_in = (w as f32 / 96.0).min(max_w);
    doc.images.push(DocImage {
        id,
        name: if alt.is_empty() {
            format!("Image {id}")
        } else {
            alt.to_string()
        },
        natural_size: [w, h],
        width_in: w_in,
        height_in: w_in * h as f32 / w as f32,
        caption: String::new(),
        png_b64: b64,
        seamless: false,
    });
    Some(id)
}

fn flush_table(doc: &mut Document, tb: TableBuilder) {
    if tb.rows.is_empty() {
        return;
    }
    let ncols = tb
        .rows
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(1)
        .max(tb.aligns.len())
        .max(1);
    let tid = doc.tables.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    let avail = (doc.page.width - doc.page.margin_left - doc.page.margin_right).max(1.0);
    let col_w = avail / ncols as f32;
    let border = BorderSpec {
        width_pt: 0.5,
        color: [203, 213, 225],
    };
    let mut specs = Vec::with_capacity(tb.rows.len());
    for (r, row) in tb.rows.into_iter().enumerate() {
        let mut cells = Vec::with_capacity(ncols);
        for c in 0..ncols {
            cells.push(TableCellSpec {
                width_in: col_w,
                shading: (r == 0).then_some([248, 250, 252]),
                borders: CellBorders::all(border),
                pad_left_in: 0.08,
                pad_right_in: 0.08,
                pad_top_in: 0.04,
                pad_bottom_in: 0.04,
                ..TableCellSpec::default()
            });
            let cell_chars = row.get(c).cloned().unwrap_or_default();
            let base = CharStyle {
                bold: r == 0,
                ..CharStyle::default()
            };
            let fmt = ParaFmt {
                table_id: tid,
                table_row: r as u16,
                table_col: c as u16,
                align: tb.aligns.get(c).copied().unwrap_or(Align::Left),
                ..ParaFmt::default()
            };
            let mut p = Para::new(base, fmt);
            for (ch, mut st) in cell_chars {
                if r == 0 {
                    st.bold = true;
                }
                p.text.push(ch);
                p.styles.push(st);
            }
            doc.paras.push(p);
        }
        specs.push(TableRowSpec {
            cells,
            min_height_pt: 0.0,
            grid_after: 0,
            cant_split: false,
            is_header: r == 0,
        });
    }
    doc.tables.push(DocTable {
        id: tid,
        grid_cols_in: vec![col_w; ncols],
        rows: specs,
        indent_in: 0.0,
    });
}

/// Parses CommonMark + GitHub Flavored Markdown (tables, strikethrough, task lists,
/// footnotes, superscript/subscript, reference links, nested lists/blockquotes, and
/// inline HTML tags `<mark>`, `<u>`, `<sup>`, `<sub>`, `<br>`) into a `Document`.
pub fn from_markdown(src: &str) -> Document {
    let mut doc = Document {
        paras: Vec::new(),
        ..Default::default()
    };
    let styles = named_styles();
    let base = CharStyle::default();

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_SUPERSCRIPT);
    opts.insert(Options::ENABLE_SUBSCRIPT);

    let parser = Parser::new_ext(src, opts);

    let mut list_stack: Vec<ListState> = Vec::new();
    let mut quote_depth: usize = 0;
    let mut style_stack: Vec<CharStyle> = vec![base];
    let mut cur_para: Option<Para> = None;
    let mut in_code_block = false;
    let mut code_buf = String::new();
    let mut table: Option<TableBuilder> = None;
    let mut pending_image: Option<(String, String)> = None;
    let mut pending_item_open = false;

    let flush_para = |doc: &mut Document, cur_para: &mut Option<Para>| {
        if let Some(p) = cur_para.take() {
            doc.paras.push(p);
        }
    };

    let open_item_para = |list_stack: &[ListState],
                          quote_depth: usize,
                          styles: &[NamedStyle],
                          base: CharStyle|
     -> (Para, CharStyle) {
        let lvl = list_stack.len().saturating_sub(1).min(8) as u8;
        let (kind, lid) = list_stack
            .last()
            .map_or((ListKind::Bullet, 0), |ls| (ls.kind, ls.list_id));
        let mut fmt = ParaFmt {
            list: kind,
            indent: lvl,
            list_id: lid,
            ..ParaFmt::default()
        };
        let chr = if quote_depth > 0 {
            fmt.style = 7;
            fmt.left_in += (quote_depth.saturating_sub(1) as f32) * 0.25;
            styles[7].chr
        } else {
            base
        };
        (Para::new(chr, fmt), chr)
    };

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    if table.is_some() {
                        // cell text flows directly into TableBuilder
                    } else if pending_item_open && cur_para.is_some() {
                        pending_item_open = false;
                    } else {
                        flush_para(&mut doc, &mut cur_para);
                        let (fmt, chr) = if quote_depth > 0 {
                            let mut f = ParaFmt {
                                style: 7,
                                ..styles[7].fmt
                            };
                            if quote_depth > 1 {
                                f.left_in += ((quote_depth - 1) as f32) * 0.25;
                            }
                            (f, styles[7].chr)
                        } else if !list_stack.is_empty() {
                            let lvl = list_stack.len().saturating_sub(1).min(8) as u8;
                            (
                                ParaFmt {
                                    left_in: (lvl as f32 + 1.0) * 0.5,
                                    space_after: MD_GAP,
                                    ..ParaFmt::default()
                                },
                                base,
                            )
                        } else {
                            (
                                ParaFmt {
                                    space_after: MD_GAP,
                                    ..ParaFmt::default()
                                },
                                base,
                            )
                        };
                        cur_para = Some(Para::new(chr, fmt));
                        style_stack.push(chr);
                    }
                }
                Tag::Heading { level, .. } => {
                    flush_para(&mut doc, &mut cur_para);
                    let idx = heading_style(level);
                    let fmt = ParaFmt {
                        style: idx,
                        keep_next: true,
                        ..styles[idx].fmt
                    };
                    let chr = styles[idx].chr;
                    cur_para = Some(Para::new(chr, fmt));
                    style_stack.push(chr);
                }
                Tag::BlockQuote(_) => {
                    flush_para(&mut doc, &mut cur_para);
                    quote_depth += 1;
                }
                Tag::CodeBlock(_) => {
                    flush_para(&mut doc, &mut cur_para);
                    in_code_block = true;
                    code_buf.clear();
                }
                Tag::List(start_opt) => {
                    if cur_para.is_some() {
                        flush_para(&mut doc, &mut cur_para);
                        pending_item_open = false;
                    }
                    let lvl = list_stack.len().min(8);
                    let (kind, list_id) = match start_opt {
                        Some(start) => {
                            let lid = if start != 1 {
                                let def_id = doc.list_defs.len() as u16 + 1;
                                let levels = (0..9)
                                    .map(|n| ListLevel {
                                        fmt: NumFmt::Decimal,
                                        text: format!("%{}.", n + 1),
                                        start: if n == lvl { start as u32 } else { 1 },
                                        left_in: n as f32 * 0.5 + 0.25,
                                        first_in: -0.25,
                                        ..ListLevel::default()
                                    })
                                    .collect();
                                doc.list_defs.push(ListDef { id: def_id, levels });
                                def_id
                            } else {
                                0
                            };
                            (ListKind::Number, lid)
                        }
                        None => (ListKind::Bullet, 0),
                    };
                    list_stack.push(ListState { kind, list_id });
                }
                Tag::Item => {
                    flush_para(&mut doc, &mut cur_para);
                    let (p, chr) = open_item_para(&list_stack, quote_depth, &styles, base);
                    cur_para = Some(p);
                    style_stack.push(chr);
                    pending_item_open = true;
                }
                Tag::Table(aligns) => {
                    flush_para(&mut doc, &mut cur_para);
                    let mapped = aligns
                        .into_iter()
                        .map(|a| match a {
                            Alignment::Center => Align::Center,
                            Alignment::Right => Align::Right,
                            Alignment::Left | Alignment::None => Align::Left,
                        })
                        .collect();
                    table = Some(TableBuilder {
                        aligns: mapped,
                        ..Default::default()
                    });
                }
                Tag::TableHead => {
                    if let Some(tb) = table.as_mut() {
                        tb.in_head = true;
                        tb.cur_row.clear();
                    }
                }
                Tag::TableRow => {
                    if let Some(tb) = table.as_mut() {
                        tb.cur_row.clear();
                    }
                }
                Tag::TableCell => {
                    if let Some(tb) = table.as_mut() {
                        tb.in_cell = true;
                        tb.cur_cell.clear();
                        let mut st = base;
                        if tb.in_head {
                            st.bold = true;
                        }
                        style_stack.push(st);
                    }
                }
                Tag::FootnoteDefinition(label) => {
                    flush_para(&mut doc, &mut cur_para);
                    let mut p = Para::new(
                        CharStyle { size: 9.5, ..base },
                        ParaFmt {
                            space_after: 4.0,
                            left_in: 0.25,
                            first_in: -0.25,
                            ..ParaFmt::default()
                        },
                    );
                    let prefix = format!("[{label}] ");
                    push_styled_str(
                        &mut p.text,
                        &mut p.styles,
                        &prefix,
                        CharStyle {
                            size: 9.5,
                            bold: true,
                            ..base
                        },
                    );
                    cur_para = Some(p);
                    style_stack.push(CharStyle { size: 9.5, ..base });
                }
                Tag::Strong => {
                    let mut st = *style_stack.last().unwrap_or(&base);
                    st.bold = true;
                    style_stack.push(st);
                }
                Tag::Emphasis => {
                    let mut st = *style_stack.last().unwrap_or(&base);
                    st.italic = true;
                    style_stack.push(st);
                }
                Tag::Strikethrough => {
                    let mut st = *style_stack.last().unwrap_or(&base);
                    st.strike = true;
                    style_stack.push(st);
                }
                Tag::Superscript => {
                    let mut st = *style_stack.last().unwrap_or(&base);
                    st.script = 1;
                    style_stack.push(st);
                }
                Tag::Subscript => {
                    let mut st = *style_stack.last().unwrap_or(&base);
                    st.script = -1;
                    style_stack.push(st);
                }
                Tag::Link { dest_url, .. } => {
                    let mut st = *style_stack.last().unwrap_or(&base);
                    st.link = doc.add_link(&dest_url);
                    style_stack.push(st);
                }
                Tag::Image { dest_url, .. } => {
                    pending_image = Some((dest_url.to_string(), String::new()));
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => {
                    if table.is_none() && (list_stack.is_empty() || !pending_item_open) {
                        flush_para(&mut doc, &mut cur_para);
                        if style_stack.len() > 1 {
                            style_stack.pop();
                        }
                    }
                }
                TagEnd::Heading(_) => {
                    flush_para(&mut doc, &mut cur_para);
                    if style_stack.len() > 1 {
                        style_stack.pop();
                    }
                }
                TagEnd::BlockQuote(_) => {
                    flush_para(&mut doc, &mut cur_para);
                    quote_depth = quote_depth.saturating_sub(1);
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    push_code_block(&mut doc, &code_buf);
                    code_buf.clear();
                }
                TagEnd::List(_) => {
                    flush_para(&mut doc, &mut cur_para);
                    list_stack.pop();
                    if list_stack.is_empty() {
                        if let Some(last) = doc.paras.last_mut() {
                            last.fmt.space_after = MD_GAP;
                        }
                    }
                }
                TagEnd::Item => {
                    flush_para(&mut doc, &mut cur_para);
                    pending_item_open = false;
                    if style_stack.len() > 1 {
                        style_stack.pop();
                    }
                }
                TagEnd::TableHead => {
                    if let Some(tb) = table.as_mut() {
                        tb.in_head = false;
                        if !tb.cur_row.is_empty() {
                            let row = std::mem::take(&mut tb.cur_row);
                            tb.rows.push(row);
                        }
                    }
                }
                TagEnd::TableRow => {
                    if let Some(tb) = table.as_mut() {
                        let row = std::mem::take(&mut tb.cur_row);
                        tb.rows.push(row);
                    }
                }
                TagEnd::TableCell => {
                    if let Some(tb) = table.as_mut() {
                        tb.in_cell = false;
                        let cell = std::mem::take(&mut tb.cur_cell);
                        tb.cur_row.push(cell);
                        if style_stack.len() > 1 {
                            style_stack.pop();
                        }
                    }
                }
                TagEnd::Table => {
                    if let Some(tb) = table.take() {
                        flush_table(&mut doc, tb);
                    }
                }
                TagEnd::FootnoteDefinition => {
                    flush_para(&mut doc, &mut cur_para);
                    if style_stack.len() > 1 {
                        style_stack.pop();
                    }
                }
                TagEnd::Strong
                | TagEnd::Emphasis
                | TagEnd::Strikethrough
                | TagEnd::Superscript
                | TagEnd::Subscript
                | TagEnd::Link => {
                    if style_stack.len() > 1 {
                        style_stack.pop();
                    }
                }
                TagEnd::Image => {
                    if let Some((url, alt)) = pending_image.take() {
                        if let Some(id) = add_image(&mut doc, &alt, &url) {
                            let standalone = cur_para.as_ref().is_some_and(|p| p.text.is_empty());
                            if standalone {
                                cur_para = None;
                            } else {
                                flush_para(&mut doc, &mut cur_para);
                            }
                            doc.paras.push(Para::new(
                                base,
                                ParaFmt {
                                    image_id: id,
                                    space_after: MD_GAP,
                                    ..ParaFmt::default()
                                },
                            ));
                        } else if !alt.is_empty() {
                            let st = *style_stack.last().unwrap_or(&base);
                            if let Some(tb) = table.as_mut().filter(|t| t.in_cell) {
                                for c in alt.chars() {
                                    tb.cur_cell.push((c, st));
                                }
                            } else if let Some(p) = cur_para.as_mut() {
                                push_styled_str(&mut p.text, &mut p.styles, &alt, st);
                            }
                        }
                    }
                }
                _ => {}
            },
            Event::TaskListMarker(checked) => {
                if let Some(p) = cur_para.as_mut() {
                    p.fmt.list = ListKind::Checklist;
                    p.fmt.checked = checked;
                }
            }
            Event::Text(text) => {
                if let Some((_, ref mut alt)) = pending_image {
                    alt.push_str(&text);
                } else if in_code_block {
                    code_buf.push_str(&text);
                } else {
                    let st = *style_stack.last().unwrap_or(&base);
                    if let Some(tb) = table.as_mut().filter(|t| t.in_cell) {
                        for c in text.chars() {
                            tb.cur_cell.push((c, st));
                        }
                    } else {
                        if cur_para.is_none() {
                            cur_para = Some(Para::new(
                                st,
                                ParaFmt {
                                    space_after: MD_GAP,
                                    ..ParaFmt::default()
                                },
                            ));
                        }
                        if let Some(p) = cur_para.as_mut() {
                            push_styled_str(&mut p.text, &mut p.styles, &text, st);
                        }
                    }
                }
            }
            Event::Code(code) => {
                if let Some((_, ref mut alt)) = pending_image {
                    alt.push_str(&code);
                } else {
                    let mut st = *style_stack.last().unwrap_or(&base);
                    st.font = code_font();
                    st.highlight = Some(CODE_BG);
                    if let Some(tb) = table.as_mut().filter(|t| t.in_cell) {
                        for c in code.chars() {
                            tb.cur_cell.push((c, st));
                        }
                    } else if let Some(p) = cur_para.as_mut() {
                        push_styled_str(&mut p.text, &mut p.styles, &code, st);
                    }
                }
            }
            Event::SoftBreak => {
                if let Some((_, ref mut alt)) = pending_image {
                    alt.push(' ');
                } else {
                    let st = *style_stack.last().unwrap_or(&base);
                    if let Some(tb) = table.as_mut().filter(|t| t.in_cell) {
                        tb.cur_cell.push((' ', st));
                    } else if let Some(p) = cur_para.as_mut() {
                        p.text.push(' ');
                        p.styles.push(st);
                    }
                }
            }
            Event::HardBreak => {
                let st = *style_stack.last().unwrap_or(&base);
                if let Some(tb) = table.as_mut().filter(|t| t.in_cell) {
                    tb.cur_cell.push(('\n', st));
                } else if let Some(p) = cur_para.as_mut() {
                    p.text.push('\n');
                    p.styles.push(st);
                }
            }
            Event::Rule => {
                flush_para(&mut doc, &mut cur_para);
                doc.paras.push(Para::new(
                    base,
                    ParaFmt {
                        hr: true,
                        space_after: MD_GAP,
                        ..ParaFmt::default()
                    },
                ));
            }
            Event::FootnoteReference(label) => {
                let mut st = *style_stack.last().unwrap_or(&base);
                st.script = 1;
                let s = format!("[{label}]");
                if let Some(tb) = table.as_mut().filter(|t| t.in_cell) {
                    for c in s.chars() {
                        tb.cur_cell.push((c, st));
                    }
                } else if let Some(p) = cur_para.as_mut() {
                    push_styled_str(&mut p.text, &mut p.styles, &s, st);
                }
            }
            Event::InlineHtml(html) | Event::Html(html) => {
                let tag = html.trim().to_ascii_lowercase();
                match tag.as_str() {
                    "<u>" => {
                        let mut st = *style_stack.last().unwrap_or(&base);
                        st.underline = true;
                        style_stack.push(st);
                    }
                    "<mark>" => {
                        let mut st = *style_stack.last().unwrap_or(&base);
                        st.highlight = Some([254, 240, 138]);
                        style_stack.push(st);
                    }
                    "<sup>" => {
                        let mut st = *style_stack.last().unwrap_or(&base);
                        st.script = 1;
                        style_stack.push(st);
                    }
                    "<sub>" => {
                        let mut st = *style_stack.last().unwrap_or(&base);
                        st.script = -1;
                        style_stack.push(st);
                    }
                    "</u>" | "</mark>" | "</sup>" | "</sub>" => {
                        if style_stack.len() > 1 {
                            style_stack.pop();
                        }
                    }
                    "<br>" | "<br/>" | "<br />" => {
                        let st = *style_stack.last().unwrap_or(&base);
                        if let Some(tb) = table.as_mut().filter(|t| t.in_cell) {
                            tb.cur_cell.push(('\n', st));
                        } else if let Some(p) = cur_para.as_mut() {
                            p.text.push('\n');
                            p.styles.push(st);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    flush_para(&mut doc, &mut cur_para);
    if doc.paras.is_empty() {
        doc.paras.push(Para::new(base, ParaFmt::default()));
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_joins_wrapped_lines_and_reads_blocks() {
        let md = "# Title\n\nFirst line\nsecond line  \nhard break\n\n- a\n- b\n    - nested\n\n3. three\n4. four\n\n```\nfn main() {}\n```\n\n| A | B |\n|---|--:|\n| 1 | 2 |\n";
        let d = from_markdown(md);
        assert_eq!(d.paras[0].fmt.style, 4);
        assert_eq!(d.paras[1].string(), "First line second line\nhard break");
        assert_eq!(d.paras[2].fmt.list, ListKind::Bullet);
        assert_eq!(d.paras[4].fmt.indent, 1);
        assert_eq!(d.list_marker(5), "3.");
        assert_eq!(d.paras[7].string(), "fn main() {}");
        assert_eq!(d.tables.len(), 1);
        assert!(d.paras[8].styles[0].bold);
    }

    #[test]
    fn markdown_roundtrip_escapes_and_emphasis() {
        let md = "Some **bold** and *it* and a [link](https://x.y) with 2 \\* stars\n";
        let d = from_markdown(md);
        assert_eq!(
            d.paras[0].string(),
            "Some bold and it and a link with 2 * stars"
        );
        let back = to_markdown(&d);
        let d2 = from_markdown(&back);
        assert_eq!(d2.paras[0].string(), d.paras[0].string());
        assert!(d2.paras[0].styles[5].bold);
    }

    #[test]
    fn markdown_gfm_reference_links_tasklists_escaped_pipes_and_keep_next() {
        let md = "## Section\n\nSee [Rust][r], ~~old~~, <mark>hi</mark>, and note[^1].\n\n- [x] Done\n- [ ] Todo\n\n| Expr | Val |\n|---|---|\n| a \\| b | 1 |\n\n[r]: https://www.rust-lang.org\n\n[^1]: Footnote body\n";
        let d = from_markdown(md);
        assert!(d.paras[0].fmt.keep_next, "headings set keep_next");
        assert!(d.links.iter().any(|u| u == "https://www.rust-lang.org"), "reference link resolved");
        assert!(d.paras.iter().any(|p| p.fmt.list == ListKind::Checklist && p.fmt.checked && p.string() == "Done"));
        assert!(d.paras.iter().any(|p| p.fmt.list == ListKind::Checklist && !p.fmt.checked && p.string() == "Todo"));
        assert!(d.paras.iter().any(|p| p.fmt.table_id > 0 && p.string() == "a | b"), "escaped pipe inside GFM table cell");
        assert!(d.tables[0].rows[0].is_header, "GFM table header row marked as repeating header");
        assert!(d.paras.iter().any(|p| p.string().contains("Footnote body")));
    }
}

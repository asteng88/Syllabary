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

// ───────────────────────── reader ─────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Block {
    Para,
    Heading(usize),
    Quote,
    Item(ListKind, u8, bool, u32),
}

fn heading_style(level: usize) -> usize {
    match level {
        1 => 4,
        2 => 5,
        _ => 6,
    }
}

/// Paragraph spacing that matches how Markdown is usually rendered.
const MD_GAP: f32 = 8.0;

pub fn from_markdown(src: &str) -> Document {
    let mut doc = Document {
        paras: Vec::new(),
        ..Default::default()
    };
    let styles = named_styles();
    let normalized = src.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let base = CharStyle::default();
    let mut li = 0usize;
    // the open paragraph-like block: (kind, collected text lines)
    let mut open: Option<(Block, Vec<String>)> = None;
    let mut list_starts: Vec<(u8, u32)> = Vec::new();
    let mut list_cols: Vec<usize> = Vec::new();

    let flush = |doc: &mut Document,
                 open: &mut Option<(Block, Vec<String>)>,
                 list_starts: &mut Vec<(u8, u32)>| {
        let Some((kind, ls)) = open.take() else {
            return;
        };
        // join lines: soft wraps become spaces, hard breaks ("  " or "\\" endings) become line breaks
        let mut text = String::new();
        for (k, l) in ls.iter().enumerate() {
            let hard = l.ends_with("  ") || l.ends_with('\\');
            let t = if l.ends_with('\\') {
                &l[..l.len() - 1]
            } else {
                l.trim_end()
            };
            text.push_str(t.trim_start());
            if k + 1 < ls.len() {
                text.push(if hard { '\n' } else { ' ' });
            }
        }
        let (fmt, chr) = match kind {
            Block::Para => (
                ParaFmt {
                    space_after: MD_GAP,
                    ..ParaFmt::default()
                },
                base,
            ),
            Block::Heading(l) => {
                let i = heading_style(l);
                (
                    ParaFmt {
                        style: i,
                        ..styles[i].fmt
                    },
                    styles[i].chr,
                )
            }
            Block::Quote => (
                ParaFmt {
                    style: 7,
                    ..styles[7].fmt
                },
                styles[7].chr,
            ),
            Block::Item(k, lvl, checked, start) => {
                let mut f = ParaFmt {
                    list: k,
                    indent: lvl,
                    checked,
                    ..ParaFmt::default()
                };
                // a later item of the same list keeps the list's numbering
                let prev_same = doc
                    .paras
                    .iter()
                    .rev()
                    .take_while(|q| q.fmt.list != ListKind::None && q.fmt.indent >= lvl)
                    .find(|q| q.fmt.indent == lvl)
                    .map(|q| (q.fmt.list, q.fmt.list_id));
                if let Some((ListKind::Number, lid)) = prev_same {
                    if k == ListKind::Number {
                        f.list_id = lid;
                    }
                } else if k == ListKind::Number && start != 1 {
                    // ordered lists that start elsewhere keep their first number
                    let def_id = doc.list_defs.len() as u16 + 1;
                    if !list_starts.iter().any(|(l, s)| *l == lvl && *s == start) {
                        let levels = (0..9)
                            .map(|n| ListLevel {
                                fmt: NumFmt::Decimal,
                                text: format!("%{}.", n + 1),
                                start: if n == lvl as usize { start } else { 1 },
                                left_in: n as f32 * 0.5 + 0.25,
                                first_in: -0.25,
                                ..ListLevel::default()
                            })
                            .collect();
                        doc.list_defs.push(ListDef { id: def_id, levels });
                        list_starts.push((lvl, start));
                    }
                    f.list_id = doc.list_defs.len() as u16;
                }
                (f, base)
            }
        };
        let mut p = Para::new(chr, fmt);
        let imgs_before = doc.images.len();
        parse_inline(doc, &mut p, &text, chr);
        let new_imgs: Vec<u64> = doc.images[imgs_before..].iter().map(|i| i.id).collect();
        if new_imgs.is_empty() {
            doc.paras.push(p);
            return;
        }
        // images are blocks: drop the placeholders and put each image in its own paragraph
        let keep: Vec<usize> = (0..p.text.len())
            .filter(|&i| p.text[i] != '\u{FFFC}')
            .collect();
        p.text = keep.iter().map(|&i| p.text[i]).collect();
        p.styles = keep.iter().map(|&i| p.styles[i]).collect();
        let only_images = p.text.iter().all(|c| c.is_whitespace());
        if !only_images {
            doc.paras.push(p.clone());
        }
        for id in new_imgs {
            let mut ip = Para::new(
                chr,
                ParaFmt {
                    space_after: MD_GAP,
                    ..ParaFmt::default()
                },
            );
            ip.fmt.image_id = id;
            doc.paras.push(ip);
        }
    };

    while li < lines.len() {
        let raw = lines[li];
        let line = raw.trim_end_matches('\n');
        let trimmed = line.trim();
        let lead = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .map(|c| if c == '\t' { 4 } else { 1 })
            .sum::<usize>();

        // blank line
        if trimmed.is_empty() {
            flush(&mut doc, &mut open, &mut list_starts);
            li += 1;
            continue;
        }
        // fenced code block
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            flush(&mut doc, &mut open, &mut list_starts);
            let fence = &trimmed[..3];
            li += 1;
            let mut code: Vec<&str> = Vec::new();
            while li < lines.len() && !lines[li].trim_start().starts_with(fence) {
                code.push(lines[li]);
                li += 1;
            }
            li += 1;
            push_code_block(&mut doc, &code);
            continue;
        }
        // indented code (not continuing a paragraph or list)
        if lead >= 4
            && open.is_none()
            && !matches!(
                doc.paras.last().map(|p| p.fmt.list),
                Some(ListKind::Bullet | ListKind::Number | ListKind::Checklist)
            )
        {
            let mut code: Vec<&str> = Vec::new();
            while li < lines.len()
                && (lines[li].starts_with("    ")
                    || lines[li].starts_with('\t')
                    || lines[li].trim().is_empty())
            {
                let l = lines[li];
                code.push(
                    l.strip_prefix("    ")
                        .or_else(|| l.strip_prefix('\t'))
                        .unwrap_or(""),
                );
                li += 1;
            }
            while code.last().is_some_and(|l| l.trim().is_empty()) {
                code.pop();
            }
            push_code_block(&mut doc, &code);
            continue;
        }
        // thematic break
        let compact: String = trimmed.chars().filter(|c| *c != ' ').collect();
        if compact.len() >= 3
            && (compact.chars().all(|c| c == '-')
                || compact.chars().all(|c| c == '*')
                || compact.chars().all(|c| c == '_'))
        {
            // "---" right under paragraph text is a setext heading
            if compact.starts_with('-')
                && matches!(open, Some((Block::Para, _)))
                && !trimmed.contains(' ')
            {
                if let Some((_, ls)) = open.take() {
                    open = Some((Block::Heading(2), ls));
                }
                flush(&mut doc, &mut open, &mut list_starts);
                li += 1;
                continue;
            }
            flush(&mut doc, &mut open, &mut list_starts);
            let mut p = Para::new(
                base,
                ParaFmt {
                    space_after: MD_GAP,
                    ..ParaFmt::default()
                },
            );
            p.fmt.hr = true;
            doc.paras.push(p);
            li += 1;
            continue;
        }
        // setext H1
        if compact.len() >= 2
            && compact.chars().all(|c| c == '=')
            && matches!(open, Some((Block::Para, _)))
        {
            if let Some((_, ls)) = open.take() {
                open = Some((Block::Heading(1), ls));
            }
            flush(&mut doc, &mut open, &mut list_starts);
            li += 1;
            continue;
        }
        // ATX heading
        if lead < 4 && trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            let rest = &trimmed[level..];
            if (1..=6).contains(&level) && (rest.is_empty() || rest.starts_with(' ')) {
                flush(&mut doc, &mut open, &mut list_starts);
                let text = rest.trim().trim_end_matches('#').trim_end().to_string();
                open = Some((Block::Heading(level), vec![text]));
                flush(&mut doc, &mut open, &mut list_starts);
                li += 1;
                continue;
            }
        }
        // GFM table
        if trimmed.starts_with('|') && li + 1 < lines.len() && is_table_sep(lines[li + 1].trim()) {
            flush(&mut doc, &mut open, &mut list_starts);
            let header = split_row(trimmed);
            let aligns: Vec<Align> = split_row(lines[li + 1].trim())
                .iter()
                .map(|c| {
                    let c = c.trim();
                    match (c.starts_with(':'), c.ends_with(':')) {
                        (true, true) => Align::Center,
                        (false, true) => Align::Right,
                        _ => Align::Left,
                    }
                })
                .collect();
            li += 2;
            let mut rows = vec![header];
            while li < lines.len() && lines[li].trim().starts_with('|') {
                rows.push(split_row(lines[li].trim()));
                li += 1;
            }
            push_table(&mut doc, &rows, &aligns);
            continue;
        }
        // block quote
        if lead < 4 && trimmed.starts_with('>') {
            let text = trimmed.trim_start_matches('>').trim_start_matches(' ');
            match open.as_mut() {
                Some((Block::Quote, ls)) => {
                    if text.is_empty() {
                        flush(&mut doc, &mut open, &mut list_starts);
                    } else {
                        ls.push(text.to_string());
                    }
                }
                _ => {
                    flush(&mut doc, &mut open, &mut list_starts);
                    if !text.is_empty() {
                        open = Some((Block::Quote, vec![text.to_string()]));
                    }
                }
            }
            li += 1;
            continue;
        }
        // list item
        if let Some((kind, checked, start, content)) = list_item(trimmed) {
            if !(matches!(open, Some((Block::Para, _))) && kind == ListKind::Number && start != 1) {
                flush(&mut doc, &mut open, &mut list_starts);
                // nesting follows the indentation columns of the enclosing items
                while list_cols.last().is_some_and(|&c| c > lead) {
                    list_cols.pop();
                }
                if list_cols.last() != Some(&lead) {
                    list_cols.push(lead);
                }
                let lvl = (list_cols.len() - 1).min(8) as u8;
                open = Some((
                    Block::Item(kind, lvl, checked, start),
                    vec![content.to_string()],
                ));
                li += 1;
                continue;
            }
        }
        // paragraph text (or a lazy continuation of the open block)
        if lead == 0 && !matches!(open, Some((Block::Item(..), _))) {
            list_cols.clear();
        }
        match open.as_mut() {
            Some((kind, ls)) if !matches!(kind, Block::Heading(_)) => {
                ls.push(line.trim_start().to_string())
            }
            _ => {
                flush(&mut doc, &mut open, &mut list_starts);
                open = Some((Block::Para, vec![line.trim_start().to_string()]));
            }
        }
        li += 1;
    }
    flush(&mut doc, &mut open, &mut list_starts);
    // tight lists: no gap between items of the same list
    for i in 0..doc.paras.len() {
        if doc.paras[i].fmt.list != ListKind::None {
            let next_list = doc
                .paras
                .get(i + 1)
                .is_some_and(|q| q.fmt.list != ListKind::None);
            doc.paras[i].fmt.space_after = if next_list { 0.0 } else { MD_GAP };
        }
    }
    // "&nbsp;" paragraphs written for empty paragraphs
    for p in doc.paras.iter_mut() {
        if p.text.len() == 1 && p.text[0] == '\u{a0}' {
            p.text.clear();
            p.styles.clear();
        }
    }
    if doc.paras.is_empty() {
        doc.paras.push(Para::new(base, ParaFmt::default()));
    }
    doc
}

fn push_code_block(doc: &mut Document, code: &[&str]) {
    let st = CharStyle {
        font: code_font(),
        size: 10.0,
        ..CharStyle::default()
    };
    let n = code.len().max(1);
    for (k, l) in code.iter().enumerate() {
        let mut p = Para::new(
            st,
            ParaFmt {
                style: 10,
                shading: Some(CODE_BG),
                space_after: if k + 1 == n { MD_GAP } else { 0.0 },
                ..ParaFmt::default()
            },
        );
        for c in l.chars() {
            p.text.push(c);
            p.styles.push(st);
        }
        doc.paras.push(p);
    }
    if code.is_empty() {
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

fn is_table_sep(l: &str) -> bool {
    let l = l.trim();
    l.contains('-') && l.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

fn split_row(l: &str) -> Vec<String> {
    let inner = l.trim().trim_start_matches('|');
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut esc = false;
    for c in inner.chars() {
        if esc {
            if c != '|' {
                cur.push('\\');
            }
            cur.push(c);
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == '|' {
            cells.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

fn push_table(doc: &mut Document, rows: &[Vec<String>], aligns: &[Align]) {
    let ncols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let avail = (doc.page.width - doc.page.margin_left - doc.page.margin_right).max(2.0);
    let col_w = avail / ncols as f32;
    let tid = doc.next_table_id();
    let mut specs = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        let mut cells = Vec::new();
        for c in 0..ncols {
            cells.push(TableCellSpec {
                width_in: col_w,
                grid_span: 1,
                shading: if r == 0 { Some([243, 244, 246]) } else { None },
                borders: CellBorders::all(BorderSpec {
                    width_pt: 0.75,
                    color: [208, 215, 222],
                }),
                v_align: CellVAlign::Top,
                pad_left_in: 0.08,
                pad_right_in: 0.08,
                pad_top_in: 0.04,
                pad_bottom_in: 0.04,
                formula: None,
                wrap_text: true,
            });
            let raw = row.get(c).map_or("", String::as_str);
            for part in raw.split("<br>") {
                let st = CharStyle {
                    bold: r == 0,
                    ..CharStyle::default()
                };
                let fmt = ParaFmt {
                    table_id: tid,
                    table_row: r as u16,
                    table_col: c as u16,
                    align: aligns.get(c).copied().unwrap_or(Align::Left),
                    ..ParaFmt::default()
                };
                let mut p = Para::new(st, fmt);
                parse_inline(doc, &mut p, part, st);
                doc.paras.push(p);
            }
        }
        specs.push(TableRowSpec {
            cells,
            min_height_pt: 0.0,
            grid_after: 0,
        });
    }
    doc.tables.push(DocTable {
        id: tid,
        grid_cols_in: vec![col_w; ncols],
        rows: specs,
        indent_in: 0.0,
    });
    if let Some(last) = doc.paras.last_mut() {
        last.fmt.space_after = 0.0;
    }
    // spacing after the table comes from an empty separator paragraph only if text follows
}

/// `- item`, `* item`, `+ item`, `1. item`, `1) item`, `- [ ] task` → (kind, checked, start, content)
fn list_item(t: &str) -> Option<(ListKind, bool, u32, &str)> {
    for m in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(m) {
            if let Some(r) = rest.strip_prefix("[ ] ") {
                return Some((ListKind::Checklist, false, 1, r));
            }
            if let Some(r) = rest
                .strip_prefix("[x] ")
                .or_else(|| rest.strip_prefix("[X] "))
            {
                return Some((ListKind::Checklist, true, 1, r));
            }
            return Some((ListKind::Bullet, false, 1, rest));
        }
    }
    if t == "-" || t == "*" || t == "+" {
        return Some((ListKind::Bullet, false, 1, ""));
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if (1..=9).contains(&digits) {
        let rest = &t[digits..];
        if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some((ListKind::Number, false, t[..digits].parse().unwrap_or(1), r));
        }
    }
    None
}

fn add_image(doc: &mut Document, alt: &str, src: &str) -> bool {
    let Some(b64) = src.split_once("base64,").map(|(_, b)| b.trim().to_string()) else {
        return false;
    };
    let Some(bytes) = base64_decode(&b64) else {
        return false;
    };
    let Ok(img) = image::load_from_memory(&bytes) else {
        return false;
    };
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
    true
}

fn find_closing(text: &str, from: usize, delim: &str) -> Option<usize> {
    let mut i = from;
    while let Some(off) = text[i..].find(delim) {
        let at = i + off;
        if at > from && !text[..at].ends_with('\\') {
            return Some(at);
        }
        i = at + delim.len();
        if i >= text.len() {
            break;
        }
    }
    None
}

fn parse_inline(doc: &mut Document, p: &mut Para, text: &str, base: CharStyle) {
    let push = |p: &mut Para, s: &str, st: CharStyle| {
        for ch in s.chars() {
            p.text.push(ch);
            p.styles.push(st);
        }
        p.end_style = st;
    };
    let mut i = 0usize;
    while i < text.len() {
        let rest = &text[i..];
        // backslash escape
        if let Some(r) = rest.strip_prefix('\\') {
            if let Some(c) = r.chars().next() {
                if c.is_ascii_punctuation() {
                    push(p, &c.to_string(), base);
                    i += 1 + c.len_utf8();
                    continue;
                }
            }
        }
        // code span
        if rest.starts_with('`') {
            let ticks = rest.chars().take_while(|c| *c == '`').count();
            let fence = "`".repeat(ticks);
            if let Some(end) = rest[ticks..].find(&fence) {
                let raw_code = &rest[ticks..ticks + end];
                let code = if raw_code.len() >= 2
                    && raw_code.starts_with(' ')
                    && raw_code.ends_with(' ')
                    && !raw_code.trim().is_empty()
                {
                    &raw_code[1..raw_code.len() - 1]
                } else {
                    raw_code
                };
                let st = CharStyle {
                    font: code_font(),
                    size: (base.size - 1.0).max(8.0),
                    highlight: Some(CODE_BG),
                    ..base
                };
                push(p, code, st);
                i += ticks * 2 + end;
                continue;
            }
        }
        // image ![alt](src)
        if rest.starts_with("![") {
            if let Some(cb) = rest.find("](") {
                if let Some(cp) = rest[cb + 2..].find(')') {
                    let alt = &rest[2..cb];
                    let src = rest[cb + 2..cb + 2 + cp]
                        .trim()
                        .trim_matches('<')
                        .trim_matches('>');
                    if add_image(doc, alt, src) {
                        push(p, "\u{FFFC}", base);
                    } else {
                        push(
                            p,
                            alt,
                            CharStyle {
                                italic: true,
                                ..base
                            },
                        );
                    }
                    i += cb + 3 + cp;
                    continue;
                }
            }
        }
        // link [label](url)
        if rest.starts_with('[') {
            if let Some(cb) = find_closing(rest, 1, "](") {
                if let Some(cp) = rest[cb + 2..].find(')') {
                    let label = &rest[1..cb];
                    let url = rest[cb + 2..cb + 2 + cp]
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .trim_matches('<')
                        .trim_matches('>');
                    let lid = doc.add_link(url);
                    let st = CharStyle { link: lid, ..base };
                    parse_inline(doc, p, label, st);
                    i += cb + 3 + cp;
                    continue;
                }
            }
        }
        // autolink <https://…>
        if rest.starts_with("<http://")
            || rest.starts_with("<https://")
            || rest.starts_with("<mailto:")
        {
            if let Some(end) = rest.find('>') {
                let url = &rest[1..end];
                let lid = doc.add_link(url);
                push(p, url, CharStyle { link: lid, ..base });
                i += end + 1;
                continue;
            }
        }
        // inline HTML the writer produces
        let tags: [(&str, &str, fn(CharStyle) -> CharStyle); 7] = [
            ("<u>", "</u>", |s| CharStyle {
                underline: true,
                ..s
            }),
            ("<sup>", "</sup>", |s| CharStyle { script: 1, ..s }),
            ("<sub>", "</sub>", |s| CharStyle { script: -1, ..s }),
            ("<mark>", "</mark>", |s| CharStyle {
                highlight: Some([255, 240, 138]),
                ..s
            }),
            ("<b>", "</b>", |s| CharStyle { bold: true, ..s }),
            ("<i>", "</i>", |s| CharStyle { italic: true, ..s }),
            ("<s>", "</s>", |s| CharStyle { strike: true, ..s }),
        ];
        let mut matched = false;
        for (open, close, f) in tags {
            if rest.starts_with(open) {
                if let Some(end) = rest.find(close) {
                    parse_inline(doc, p, &rest[open.len()..end], f(base));
                    i += end + close.len();
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            continue;
        }
        if rest.starts_with("<br>") || rest.starts_with("<br/>") || rest.starts_with("<br />") {
            push(p, "\n", base);
            i += rest.find('>').unwrap_or(3) + 1;
            continue;
        }
        if let Some(r) = rest.strip_prefix("&nbsp;") {
            let _ = r;
            push(p, "\u{a0}", base);
            i += 6;
            continue;
        }
        if let Some(num) = rest.strip_prefix("&#") {
            if let Some(end) = num.find(';').filter(|&e| e <= 7) {
                let code = if let Some(h) = num[..end].strip_prefix(['x', 'X']) {
                    u32::from_str_radix(h, 16).ok()
                } else {
                    num[..end].parse().ok()
                };
                if let Some(c) = code.and_then(char::from_u32) {
                    push(p, &c.to_string(), base);
                    i += 2 + end + 1;
                    continue;
                }
            }
        }
        if rest.starts_with("&amp;")
            || rest.starts_with("&lt;")
            || rest.starts_with("&gt;")
            || rest.starts_with("&quot;")
        {
            let end = rest.find(';').unwrap_or(0);
            let c = match &rest[..=end] {
                "&amp;" => "&",
                "&lt;" => "<",
                "&gt;" => ">",
                _ => "\"",
            };
            push(p, c, base);
            i += end + 1;
            continue;
        }
        // emphasis: ***, **, __, ~~, *, _
        let mut handled = false;
        for (delim, f) in [
            (
                "***",
                (|s: CharStyle| CharStyle {
                    bold: true,
                    italic: true,
                    ..s
                }) as fn(CharStyle) -> CharStyle,
            ),
            ("___", |s| CharStyle {
                bold: true,
                italic: true,
                ..s
            }),
            ("**", |s| CharStyle { bold: true, ..s }),
            ("__", |s| CharStyle { bold: true, ..s }),
            ("~~", |s| CharStyle { strike: true, ..s }),
            ("*", |s| CharStyle { italic: true, ..s }),
            ("_", |s| CharStyle { italic: true, ..s }),
        ] {
            if !rest.starts_with(delim) {
                continue;
            }
            // left-flanking: next char is not whitespace; '_' must not be intraword
            let after = rest[delim.len()..].chars().next();
            if after.is_none_or(char::is_whitespace) {
                continue;
            }
            if delim.starts_with('_') && text[..i].chars().last().is_some_and(char::is_alphanumeric)
            {
                continue;
            }
            if let Some(end) = find_closing(rest, delim.len(), delim) {
                let inner = &rest[delim.len()..end];
                if inner.ends_with(' ') {
                    continue;
                }
                if delim.starts_with('_')
                    && rest[end + delim.len()..]
                        .chars()
                        .next()
                        .is_some_and(char::is_alphanumeric)
                {
                    continue;
                }
                parse_inline(doc, p, inner, f(base));
                i += end + delim.len();
                handled = true;
                break;
            }
        }
        if handled {
            continue;
        }
        let ch = rest.chars().next().unwrap_or(' ');
        push(p, &ch.to_string(), base);
        i += ch.len_utf8();
    }
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
}

//! Google Docs REST API (v1) JSON reader: named styles, text styles with inheritance, lists with
//! their glyphs, indents, tab stops, borders, shading, tables, inline images (data URIs),
//! headers / footers, footnotes, page breaks and document style.

use serde_json::Value;
use std::collections::HashMap;

use crate::doc::*;

fn mag(v: Option<&Value>) -> Option<f32> {
    v?.get("magnitude")?.as_f64().map(|m| m as f32)
}

fn rgb(c: Option<&Value>) -> Option<[u8; 3]> {
    let rgb = c?.get("color")?.get("rgbColor")?;
    let ch = |k: &str| {
        (rgb.get(k).and_then(Value::as_f64).unwrap_or(0.0) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Some([ch("red"), ch("green"), ch("blue")])
}

/// Google Docs' built-in named styles (used when the JSON omits `namedStyles`).
fn default_named(kind: &str) -> (CharStyle, ParaFmt) {
    let arial = font_index("Arial");
    let base = CharStyle {
        font: arial,
        size: 11.0,
        ..CharStyle::default()
    };
    let pf = ParaFmt {
        line_spacing: 1.15,
        ..ParaFmt::default()
    };
    let grey = [0x66, 0x66, 0x66];
    match kind {
        "TITLE" => (
            CharStyle { size: 26.0, ..base },
            ParaFmt {
                space_after: 3.0,
                style: 2,
                ..pf
            },
        ),
        "SUBTITLE" => (
            CharStyle {
                size: 15.0,
                color: grey,
                ..base
            },
            ParaFmt {
                space_after: 16.0,
                style: 3,
                ..pf
            },
        ),
        "HEADING_1" => (
            CharStyle { size: 20.0, ..base },
            ParaFmt {
                space_before: 20.0,
                space_after: 6.0,
                style: 4,
                ..pf
            },
        ),
        "HEADING_2" => (
            CharStyle { size: 16.0, ..base },
            ParaFmt {
                space_before: 18.0,
                space_after: 6.0,
                style: 5,
                ..pf
            },
        ),
        "HEADING_3" => (
            CharStyle {
                size: 14.0,
                color: [0x43, 0x43, 0x43],
                ..base
            },
            ParaFmt {
                space_before: 16.0,
                space_after: 4.0,
                style: 6,
                ..pf
            },
        ),
        "HEADING_4" => (
            CharStyle {
                size: 12.0,
                color: grey,
                ..base
            },
            ParaFmt {
                space_before: 14.0,
                space_after: 4.0,
                style: 6,
                ..pf
            },
        ),
        "HEADING_5" => (
            CharStyle {
                size: 11.0,
                color: grey,
                ..base
            },
            ParaFmt {
                space_before: 12.0,
                space_after: 4.0,
                style: 6,
                ..pf
            },
        ),
        "HEADING_6" => (
            CharStyle {
                size: 11.0,
                italic: true,
                color: grey,
                ..base
            },
            ParaFmt {
                space_before: 12.0,
                space_after: 4.0,
                style: 6,
                ..pf
            },
        ),
        // Syllabary's own style names (written into .gdoc exports)
        "BODY_SINGLE" | "QUOTE" | "BULLET_1" | "NUMBER_LIST" | "CODE" => {
            let idx = match kind {
                "BODY_SINGLE" => 1,
                "QUOTE" => 7,
                "BULLET_1" => 8,
                "NUMBER_LIST" => 9,
                _ => 10,
            };
            let ns = &named_styles()[idx];
            (
                ns.chr,
                ParaFmt {
                    style: idx,
                    ..ns.fmt
                },
            )
        }
        _ => (base, pf),
    }
}

fn apply_text_style(st: &mut CharStyle, ts: &Value, doc: &mut Document) {
    if let Some(b) = ts.get("bold").and_then(Value::as_bool) {
        st.bold = b;
    }
    if let Some(i) = ts.get("italic").and_then(Value::as_bool) {
        st.italic = i;
    }
    if let Some(u) = ts.get("underline").and_then(Value::as_bool) {
        st.underline = u;
    }
    if let Some(s) = ts.get("strikethrough").and_then(Value::as_bool) {
        st.strike = s;
    }
    if let Some(s) = ts.get("smallCaps").and_then(Value::as_bool) {
        st.caps = if s { 2 } else { 0 };
    }
    if let Some(bo) = ts.get("baselineOffset").and_then(Value::as_str) {
        st.script = match bo {
            "SUPERSCRIPT" => 1,
            "SUBSCRIPT" => -1,
            _ => 0,
        };
    }
    if let Some(sz) = mag(ts.get("fontSize")) {
        st.size = sz.clamp(1.0, 400.0);
    }
    if let Some(wff) = ts.get("weightedFontFamily") {
        if let Some(ff) = wff.get("fontFamily").and_then(Value::as_str) {
            st.font = font_index(ff);
        }
        if let Some(w) = wff.get("weight").and_then(Value::as_i64) {
            if w >= 600 {
                st.bold = true;
            }
        }
    }
    if let Some(fg) = ts.get("foregroundColor") {
        if let Some(c) = rgb(Some(fg)) {
            st.color = c;
        }
    }
    if let Some(c2) = rgb(ts.get("foregroundColorTo")) {
        st.color_to = Some(c2);
    }
    if let Some(bg) = ts.get("backgroundColor") {
        st.highlight = rgb(Some(bg)).filter(|_| bg.get("color").is_some());
    }
    if let Some(bg2) = rgb(ts.get("backgroundColorTo")) {
        st.highlight_to = Some(bg2);
    }
    if let Some(link) = ts.get("link") {
        if let Some(url) = link.get("url").and_then(Value::as_str) {
            st.link = doc.add_link(url);
        }
    }
}

fn border(b: Option<&Value>) -> Option<BorderSpec> {
    let b = b?;
    let w = mag(b.get("width")).unwrap_or(0.0);
    if w <= 0.0 {
        return None;
    }
    Some(BorderSpec {
        width_pt: w,
        color: rgb(b.get("color")).unwrap_or([0, 0, 0]),
    })
}

fn apply_para_style(f: &mut ParaFmt, ps: &Value) {
    if let Some(al) = ps.get("alignment").and_then(Value::as_str) {
        f.align = match al {
            "CENTER" => Align::Center,
            "END" | "RIGHT" => Align::Right,
            "JUSTIFIED" => Align::Justify,
            _ => Align::Left,
        };
    }
    if let Some(ls) = ps.get("lineSpacing").and_then(Value::as_f64) {
        f.line_spacing = (ls as f32 / 100.0).clamp(0.3, 10.0);
    }
    if let Some(v) = mag(ps.get("spaceAbove")) {
        f.space_before = v.max(0.0);
    }
    if let Some(v) = mag(ps.get("spaceBelow")) {
        f.space_after = v.max(0.0);
    }
    let start = mag(ps.get("indentStart"));
    let first = mag(ps.get("indentFirstLine"));
    if let Some(s) = start {
        f.indent = 0;
        f.left_in = s / 72.0;
    }
    if let Some(fl) = first {
        // Docs measures the first line from the margin, not from the start indent
        f.first_in = fl / 72.0 - f.left_in;
    } else if start.is_some() {
        f.first_in = 0.0;
    }
    if let Some(e) = mag(ps.get("indentEnd")) {
        f.right_in = e / 72.0;
    }
    if let Some(pbb) = ps.get("pageBreakBefore").and_then(Value::as_bool) {
        f.page_break_before = pbb;
    }
    if let Some(shd) = ps.get("shading").and_then(|s| s.get("backgroundColor")) {
        f.shading = rgb(Some(shd)).filter(|c| *c != [255, 255, 255]);
    }
    if let Some(tabs) = ps.get("tabStops").and_then(Value::as_array) {
        f.tabs = TabStops::default();
        for t in tabs {
            if let Some(off) = mag(t.get("offset")) {
                let kind = match t.get("alignment").and_then(Value::as_str) {
                    Some("CENTER") => TabKind::Center,
                    Some("END") => TabKind::Right,
                    _ => TabKind::Left,
                };
                f.tabs.set(TabStop {
                    pos_in: off / 72.0,
                    kind,
                    leader: 0,
                });
            }
        }
    }
    for (k, slot) in [
        ("borderTop", 0),
        ("borderBottom", 1),
        ("borderLeft", 2),
        ("borderRight", 3),
    ] {
        if let Some(b) = ps.get(k) {
            let spec = border(Some(b));
            match slot {
                0 => f.pborders.top = spec,
                1 => f.pborders.bottom = spec,
                2 => f.pborders.left = spec,
                _ => f.pborders.right = spec,
            }
        }
    }
}

struct Ctx<'a> {
    named: HashMap<String, (CharStyle, ParaFmt)>,
    lists: &'a serde_json::Map<String, Value>,
    list_ids: HashMap<String, u16>,
    footnotes: Option<&'a serde_json::Map<String, Value>>,
    notes: Vec<String>,
    pending_break: bool,
}

fn list_def_for(doc: &mut Document, ctx: &mut Ctx, list_id: &str) -> Option<u16> {
    if let Some(&id) = ctx.list_ids.get(list_id) {
        return Some(id);
    }
    let levels_json = ctx
        .lists
        .get(list_id)?
        .get("listProperties")?
        .get("nestingLevels")?
        .as_array()?;
    let mut levels = Vec::new();
    for nl in levels_json {
        let glyph_type = nl.get("glyphType").and_then(Value::as_str).unwrap_or("");
        let symbol = nl.get("glyphSymbol").and_then(Value::as_str);
        let fmt = match (symbol, glyph_type) {
            (Some(_), _) => NumFmt::Bullet,
            (None, "DECIMAL") => NumFmt::Decimal,
            (None, "ZERO_DECIMAL") => NumFmt::DecimalZero,
            (None, "ALPHA") => NumFmt::LowerLetter,
            (None, "UPPER_ALPHA") => NumFmt::UpperLetter,
            (None, "ROMAN") => NumFmt::LowerRoman,
            (None, "UPPER_ROMAN") => NumFmt::UpperRoman,
            (None, "NONE") => NumFmt::None,
            _ => NumFmt::Bullet,
        };
        let text = if let Some(sym) = symbol {
            sym.to_string()
        } else {
            // "%0." → "%1." (Docs counts levels from zero)
            let f = nl
                .get("glyphFormat")
                .and_then(Value::as_str)
                .unwrap_or("%0.");
            let mut out = String::new();
            let mut it = f.chars().peekable();
            while let Some(c) = it.next() {
                if c == '%' {
                    if let Some(d) = it.peek().and_then(|d| d.to_digit(10)) {
                        it.next();
                        out.push('%');
                        out.push(char::from_digit(d + 1, 10).unwrap_or('1'));
                        continue;
                    }
                }
                out.push(c);
            }
            out
        };
        let left = mag(nl.get("indentStart")).unwrap_or(36.0) / 72.0;
        let first_abs = mag(nl.get("indentFirstLine")).unwrap_or(18.0) / 72.0;
        let mut lvl = ListLevel {
            fmt,
            text: if fmt == NumFmt::Bullet && text.is_empty() {
                "●".into()
            } else {
                text
            },
            start: nl.get("startNumber").and_then(Value::as_u64).unwrap_or(1) as u32,
            left_in: left,
            first_in: first_abs - left,
            align: match nl.get("bulletAlignment").and_then(Value::as_str) {
                Some("CENTER") => Align::Center,
                Some("END") => Align::Right,
                _ => Align::Left,
            },
            ..ListLevel::default()
        };
        if let Some(ts) = nl.get("textStyle") {
            let mut st = CharStyle::default();
            apply_text_style(&mut st, ts, doc);
            if ts.get("weightedFontFamily").is_some() {
                lvl.font = Some(st.font);
            }
            if ts
                .get("foregroundColor")
                .and_then(|c| c.get("color"))
                .is_some()
            {
                lvl.color = Some(st.color);
            }
            if ts.get("fontSize").is_some() {
                lvl.size = Some(st.size);
            }
        }
        levels.push(lvl);
    }
    let id = doc.list_defs.len() as u16 + 1;
    doc.list_defs.push(ListDef { id, levels });
    ctx.list_ids.insert(list_id.to_string(), id);
    Some(id)
}

/// Converts one `paragraph` structural element; `cell` places it in a table cell.
fn read_paragraph(doc: &mut Document, ctx: &mut Ctx, p_obj: &Value, cell: Option<(u32, u16, u16)>) {
    let ps = p_obj.get("paragraphStyle");
    let kind = ps
        .and_then(|p| p.get("namedStyleType"))
        .and_then(Value::as_str)
        .unwrap_or("NORMAL_TEXT");
    let (mut base, mut fmt) = ctx
        .named
        .get(kind)
        .copied()
        .unwrap_or_else(|| default_named(kind));
    if let Some(ps) = ps {
        apply_para_style(&mut fmt, ps);
    }
    if std::mem::take(&mut ctx.pending_break) {
        fmt.page_break_before = true;
    }
    if let Some(bul) = p_obj.get("bullet") {
        let lid = bul.get("listId").and_then(Value::as_str).unwrap_or("");
        let lvl = bul
            .get("nestingLevel")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(8) as u8;
        if let Some(id) = list_def_for(doc, ctx, lid) {
            let lv = doc
                .list_def(id)
                .and_then(|d| d.levels.get(lvl as usize).cloned())
                .unwrap_or_default();
            fmt.list = if lv.fmt == NumFmt::Bullet {
                ListKind::Bullet
            } else {
                ListKind::Number
            };
            fmt.list_id = id;
            fmt.indent = lvl;
            // paragraph indents in Docs repeat the list's; keep the list level's own values
            if ps.and_then(|p| p.get("indentStart")).is_none() {
                fmt.left_in = 0.0;
                fmt.first_in = 0.0;
            }
        } else {
            // Syllabary-written .gdoc bodies name their lists by kind
            fmt.list = if lid.contains("number") || lid.contains("decimal") {
                ListKind::Number
            } else if lid.contains("check") {
                ListKind::Checklist
            } else {
                ListKind::Bullet
            };
            fmt.indent = lvl;
            fmt.checked = bul.get("checked").and_then(Value::as_bool).unwrap_or(false);
            fmt.bullet_preset = bul.get("bulletPreset").and_then(Value::as_u64).unwrap_or(0) as u8;
            fmt.left_in = 0.0;
            fmt.first_in = 0.0;
        }
        if let Some(ts) = bul.get("textStyle") {
            apply_text_style(&mut base, ts, doc);
        }
    }
    if let Some((tid, r, c)) = cell {
        fmt.table_id = tid;
        fmt.table_row = r;
        fmt.table_col = c;
    } else if let Some(tcr) = p_obj.get("tableCellRef") {
        fmt.table_id = tcr.get("tableId").and_then(Value::as_u64).unwrap_or(0) as u32;
        fmt.table_row = tcr.get("row").and_then(Value::as_u64).unwrap_or(0) as u16;
        fmt.table_col = tcr.get("col").and_then(Value::as_u64).unwrap_or(0) as u16;
    }
    let mut p = Para::new(base, fmt);
    let mut paras_out: Vec<Para> = Vec::new();
    if let Some(elems) = p_obj.get("elements").and_then(Value::as_array) {
        for el in elems {
            if el.get("horizontalRule").is_some() {
                p.fmt.hr = true;
                continue;
            }
            if el.get("pageBreak").is_some() {
                ctx.pending_break = true;
                continue;
            }
            if let Some(fr) = el.get("footnoteReference") {
                let id = fr.get("footnoteId").and_then(Value::as_str).unwrap_or("");
                let text = ctx
                    .footnotes
                    .and_then(|f| f.get(id))
                    .and_then(|f| f.get("content"))
                    .map(plain_text_of)
                    .unwrap_or_default();
                ctx.notes.push(text);
                let mut st = p.styles.last().copied().unwrap_or(base);
                st.script = 1;
                for ch in ctx.notes.len().to_string().chars() {
                    p.text.push(ch);
                    p.styles.push(st);
                }
                continue;
            }
            if let Some(ioe) = el.get("inlineObjectElement") {
                if let Some(id) = ioe
                    .get("inlineObjectId")
                    .and_then(Value::as_str)
                    .and_then(|oid| image_id_of(doc, oid))
                {
                    if !p.text.is_empty() {
                        let f = ParaFmt {
                            image_id: 0,
                            ..p.fmt
                        };
                        paras_out.push(std::mem::replace(
                            &mut p,
                            Para::new(
                                base,
                                ParaFmt {
                                    page_break_before: false,
                                    ..f
                                },
                            ),
                        ));
                    }
                    p.fmt.image_id = id;
                    p.fmt.list = ListKind::None;
                }
                continue;
            }
            let Some(tr) = el.get("textRun") else {
                continue;
            };
            let raw = tr.get("content").and_then(Value::as_str).unwrap_or("");
            let mut st = base;
            if let Some(ts) = tr.get("textStyle") {
                apply_text_style(&mut st, ts, doc);
            }
            for ch in raw.chars() {
                match ch {
                    '\n' => {}
                    '\u{000B}' => {
                        p.text.push('\n');
                        p.styles.push(st);
                    }
                    c => {
                        p.text.push(c);
                        p.styles.push(st);
                    }
                }
            }
            p.end_style = st;
        }
    }
    paras_out.push(p);
    for q in paras_out {
        doc.paras.push(q);
    }
}

fn plain_text_of(content: &Value) -> String {
    let mut out = String::new();
    for item in content.as_array().into_iter().flatten() {
        if let Some(els) = item
            .get("paragraph")
            .and_then(|p| p.get("elements"))
            .and_then(Value::as_array)
        {
            for el in els {
                if let Some(t) = el
                    .get("textRun")
                    .and_then(|t| t.get("content"))
                    .and_then(Value::as_str)
                {
                    out.push_str(&t.replace(['\n', '\u{000B}'], " "));
                }
            }
        }
    }
    out.trim().to_string()
}

fn image_id_of(doc: &Document, oid: &str) -> Option<u64> {
    let idx = doc
        .images
        .iter()
        .position(|im| im.name == format!("\u{0}{oid}") || format!("kix.img_{}", im.id) == oid)?;
    Some(doc.images[idx].id)
}

fn read_content(doc: &mut Document, ctx: &mut Ctx, content: &[Value]) {
    for item in content {
        if let Some(p_obj) = item.get("paragraph") {
            read_paragraph(doc, ctx, p_obj, None);
        } else if let Some(t) = item.get("table") {
            read_table(doc, ctx, t);
        } else if let Some(toc) = item
            .get("tableOfContents")
            .and_then(|t| t.get("content"))
            .and_then(Value::as_array)
        {
            read_content(doc, ctx, toc);
        } else if item.get("sectionBreak").is_some() {
            let st = item
                .get("sectionBreak")
                .and_then(|s| s.get("sectionStyle"))
                .and_then(|s| s.get("sectionType"))
                .and_then(Value::as_str);
            if st == Some("NEXT_PAGE") && !doc.paras.is_empty() {
                ctx.pending_break = true;
            }
        }
    }
}

fn read_table(doc: &mut Document, ctx: &mut Ctx, t: &Value) {
    let tid = doc.tables.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    let ncols = t.get("columns").and_then(Value::as_u64).unwrap_or(1).max(1) as usize;
    let content_w = (doc.page.width - doc.page.margin_left - doc.page.margin_right).max(1.0);
    let col_props = t
        .get("tableStyle")
        .and_then(|s| s.get("tableColumnProperties"))
        .and_then(Value::as_array);
    let mut grid: Vec<f32> = (0..ncols)
        .map(|c| {
            col_props
                .and_then(|cp| cp.get(c))
                .and_then(|p| mag(p.get("width")))
                .map_or(0.0, |w| w / 72.0)
        })
        .collect();
    let fixed: f32 = grid.iter().sum();
    let evenly = grid.iter().filter(|w| **w <= 0.0).count();
    if evenly > 0 {
        let rest = ((content_w - fixed) / evenly as f32).max(0.3);
        for w in grid.iter_mut().filter(|w| **w <= 0.0) {
            *w = rest;
        }
    }
    let mut rows = Vec::new();
    for (r, row) in t
        .get("tableRows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let mut cells = Vec::new();
        let mut gcol = 0usize;
        for (c, cell) in row
            .get("tableCells")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let cs = cell.get("tableCellStyle");
            let span = cs
                .and_then(|s| s.get("columnSpan"))
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .max(1) as usize;
            let width: f32 = grid.iter().skip(gcol).take(span).sum();
            gcol += span;
            let pad = |k: &str, d: f32| cs.and_then(|s| mag(s.get(k))).map_or(d, |v| v / 72.0);
            let thin = Some(BorderSpec {
                width_pt: 1.0,
                color: [0, 0, 0],
            });
            let b = |k: &str| match cs.and_then(|s| s.get(k)) {
                Some(v) => border(Some(v)),
                None => thin,
            };
            let spec = TableCellSpec {
                width_in: width.max(0.2),
                grid_span: span as u16,
                shading: cs
                    .and_then(|s| rgb(s.get("backgroundColor")))
                    .filter(|c| *c != [255, 255, 255]),
                borders: CellBorders {
                    top: b("borderTop"),
                    bottom: b("borderBottom"),
                    left: b("borderLeft"),
                    right: b("borderRight"),
                },
                v_align: match cs
                    .and_then(|s| s.get("contentAlignment"))
                    .and_then(Value::as_str)
                {
                    Some("MIDDLE") => CellVAlign::Center,
                    Some("BOTTOM") => CellVAlign::Bottom,
                    _ => CellVAlign::Top,
                },
                pad_left_in: pad("paddingLeft", 5.0 / 72.0),
                pad_right_in: pad("paddingRight", 5.0 / 72.0),
                pad_top_in: pad("paddingTop", 5.0 / 72.0),
                pad_bottom_in: pad("paddingBottom", 5.0 / 72.0),
                formula: None,
                wrap_text: true,
            };
            cells.push(spec);
            let before = doc.paras.len();
            if let Some(content) = cell.get("content").and_then(Value::as_array) {
                for item in content {
                    if let Some(p_obj) = item.get("paragraph") {
                        read_paragraph(doc, ctx, p_obj, Some((tid, r as u16, c as u16)));
                    }
                }
            }
            if doc.paras.len() == before {
                let (base, f) = default_named("NORMAL_TEXT");
                doc.paras.push(Para::new(
                    base,
                    ParaFmt {
                        table_id: tid,
                        table_row: r as u16,
                        table_col: c as u16,
                        ..f
                    },
                ));
            }
        }
        let min_h = row
            .get("tableRowStyle")
            .and_then(|s| mag(s.get("minRowHeight")))
            .unwrap_or(0.0);
        rows.push(TableRowSpec {
            cells,
            min_height_pt: min_h,
            grid_after: 0,
        });
    }
    doc.tables.push(DocTable {
        id: tid,
        grid_cols_in: grid,
        rows,
        indent_in: 0.0,
    });
}

/// The first non-empty text run's style in a header / footer.
fn hf_run_style(map: Option<&serde_json::Map<String, Value>>, id: Option<&str>) -> Option<Value> {
    let content = map?.get(id?)?.get("content")?.as_array()?;
    content.iter().find_map(|i| {
        i.get("paragraph")?
            .get("elements")?
            .as_array()?
            .iter()
            .find_map(|e| {
                let tr = e.get("textRun")?;
                let txt = tr.get("content")?.as_str()?;
                if txt.trim().is_empty() {
                    None
                } else {
                    Some(tr.get("textStyle").cloned().unwrap_or(Value::Null))
                }
            })
    })
}

fn hf_text(
    map: Option<&serde_json::Map<String, Value>>,
    id: Option<&str>,
) -> Option<(String, Align)> {
    let hf = map?.get(id?)?;
    let content = hf.get("content")?;
    let align = content
        .as_array()
        .and_then(|a| a.iter().find_map(|i| i.get("paragraph")))
        .and_then(|p| p.get("paragraphStyle"))
        .and_then(|s| s.get("alignment"))
        .and_then(Value::as_str)
        .map_or(Align::Left, |a| match a {
            "CENTER" => Align::Center,
            "END" => Align::Right,
            _ => Align::Left,
        });
    Some((plain_text_of(content), align))
}

/// Reads a Google Docs API document (`body.content`) into a `Document`.
pub fn from_api(v: &Value) -> Document {
    // Documents with tabs keep their body inside the first tab.
    let root = if v.get("body").is_some() {
        v
    } else {
        v.get("tabs")
            .and_then(Value::as_array)
            .and_then(|t| t.first())
            .and_then(|t| t.get("documentTab"))
            .unwrap_or(v)
    };
    let mut doc = Document {
        paras: Vec::new(),
        ..Default::default()
    };
    let mut hdr_ts: Option<Value> = None;
    let mut ftr_ts: Option<Value> = None;
    if let Some(t) = v.get("title").and_then(Value::as_str) {
        doc.info.title = t.to_string();
    }
    if let Some(ds) = root.get("documentStyle") {
        if let Some(ps) = ds.get("pageSize") {
            if let Some(w) = mag(ps.get("width")) {
                doc.page.width = w / 72.0;
            }
            if let Some(h) = mag(ps.get("height")) {
                doc.page.height = h / 72.0;
            }
        }
        for (k, slot) in [
            ("marginTop", 0),
            ("marginBottom", 1),
            ("marginLeft", 2),
            ("marginRight", 3),
        ] {
            if let Some(m) = mag(ds.get(k)) {
                let m = m / 72.0;
                match slot {
                    0 => doc.page.margin_top = m,
                    1 => doc.page.margin_bottom = m,
                    2 => doc.page.margin_left = m,
                    _ => doc.page.margin_right = m,
                }
            }
        }
        if let Some(bg) = rgb(ds.get("background")) {
            doc.page.background = bg;
        }
        if let Some(pn) = ds.get("pageNumbers").and_then(Value::as_bool) {
            doc.page.page_numbers = pn;
        }
        if let Some(pl) = ds.get("pageless").and_then(Value::as_bool) {
            doc.pageless = pl;
        }
        if ds
            .get("documentFormat")
            .and_then(|f| f.get("documentMode"))
            .and_then(Value::as_str)
            == Some("PAGELESS")
        {
            doc.pageless = true;
        }
        let headers = root.get("headers").and_then(Value::as_object);
        let footers = root.get("footers").and_then(Value::as_object);
        hdr_ts = hf_run_style(headers, ds.get("defaultHeaderId").and_then(Value::as_str));
        ftr_ts = hf_run_style(footers, ds.get("defaultFooterId").and_then(Value::as_str));
        if let Some(m) = mag(ds.get("marginHeader")) {
            doc.header_dist_in = m / 72.0;
        }
        if let Some(m) = mag(ds.get("marginFooter")) {
            doc.footer_dist_in = m / 72.0;
        }
        // Syllabary's own exports store these directly
        doc.header_style = ds
            .get("headerStyle")
            .and_then(|x| serde_json::from_value(x.clone()).ok())
            .or(doc.header_style);
        doc.footer_style = ds
            .get("footerStyle")
            .and_then(|x| serde_json::from_value(x.clone()).ok())
            .or(doc.footer_style);
        if let Some(d) = ds.get("headerDist").and_then(Value::as_f64) {
            doc.header_dist_in = d as f32;
        }
        if let Some(d) = ds.get("footerDist").and_then(Value::as_f64) {
            doc.footer_dist_in = d as f32;
        }
        if let Some((t, a)) = hf_text(headers, ds.get("defaultHeaderId").and_then(Value::as_str)) {
            doc.header = t;
            doc.header_align = a;
        }
        if let Some((t, a)) = hf_text(footers, ds.get("defaultFooterId").and_then(Value::as_str)) {
            doc.footer = t;
            doc.footer_align = a;
        }
        for (k, target) in [("headerText", 0), ("footerText", 1)] {
            if let Some(t) = ds.get(k).and_then(Value::as_str) {
                if target == 0 {
                    doc.header = t.to_string();
                } else {
                    doc.footer = t.to_string();
                }
            }
        }
        if let Some(a) = ds
            .get("headerAlign")
            .and_then(|x| serde_json::from_value(x.clone()).ok())
        {
            doc.header_align = a;
        }
        if let Some(a) = ds
            .get("footerAlign")
            .and_then(|x| serde_json::from_value(x.clone()).ok())
        {
            doc.footer_align = a;
        }
        if let Some(i) = ds.get("headerItalic").and_then(Value::as_bool) {
            doc.header_italic = i;
        }
        if let Some(i) = ds.get("footerItalic").and_then(Value::as_bool) {
            doc.footer_italic = i;
        }
    }
    for (_, sw, sh) in PAGE_SIZES {
        if (doc.page.width - sw).abs() < 0.02 && (doc.page.height - sh).abs() < 0.02 {
            doc.page.width = *sw;
            doc.page.height = *sh;
        }
    }
    // Named styles: NORMAL_TEXT is the base of every other style.
    let mut named: HashMap<String, (CharStyle, ParaFmt)> = HashMap::new();
    let styles_json = root
        .get("namedStyles")
        .and_then(|n| n.get("styles"))
        .and_then(Value::as_array);
    let normal_json = styles_json.and_then(|s| {
        s.iter()
            .find(|x| x.get("namedStyleType").and_then(Value::as_str) == Some("NORMAL_TEXT"))
    });
    let mut normal = default_named("NORMAL_TEXT");
    if let Some(nj) = normal_json {
        if let Some(ts) = nj.get("textStyle") {
            apply_text_style(&mut normal.0, ts, &mut doc);
        }
        if let Some(ps) = nj.get("paragraphStyle") {
            apply_para_style(&mut normal.1, ps);
        }
    }
    for kind in [
        "NORMAL_TEXT",
        "TITLE",
        "SUBTITLE",
        "HEADING_1",
        "HEADING_2",
        "HEADING_3",
        "HEADING_4",
        "HEADING_5",
        "HEADING_6",
    ] {
        let (dc, dp) = default_named(kind);
        let (mut c, mut p) = if kind == "NORMAL_TEXT" {
            normal
        } else {
            (
                CharStyle {
                    font: normal.0.font,
                    ..dc
                },
                ParaFmt {
                    line_spacing: normal.1.line_spacing,
                    ..dp
                },
            )
        };
        if let Some(sj) = styles_json.and_then(|s| {
            s.iter()
                .find(|x| x.get("namedStyleType").and_then(Value::as_str) == Some(kind))
        }) {
            if kind != "NORMAL_TEXT" {
                c = normal.0;
                p = ParaFmt {
                    style: dp.style,
                    ..normal.1
                };
                if let Some(ts) = sj.get("textStyle") {
                    apply_text_style(&mut c, ts, &mut doc);
                }
                if let Some(ps) = sj.get("paragraphStyle") {
                    apply_para_style(&mut p, ps);
                }
            }
        }
        named.insert(kind.to_string(), (c, p));
    }
    // Syllabary's own .gdoc exports describe every run explicitly against the editor's styles.
    if v.get("format").and_then(Value::as_str) == Some("google-docs-document")
        && styles_json.is_none()
    {
        let app = named_styles();
        let pick = |idx: usize| {
            (
                app[idx].chr,
                ParaFmt {
                    style: idx,
                    ..app[idx].fmt
                },
            )
        };
        named.insert(
            "NORMAL_TEXT".into(),
            (CharStyle::default(), ParaFmt::default()),
        );
        for (kind, idx) in [
            ("BODY_SINGLE", 1),
            ("TITLE", 2),
            ("SUBTITLE", 3),
            ("HEADING_1", 4),
            ("HEADING_2", 5),
            ("HEADING_3", 6),
            ("HEADING_4", 6),
            ("HEADING_5", 6),
            ("HEADING_6", 6),
            ("QUOTE", 7),
            ("BULLET_1", 8),
            ("NUMBER_LIST", 9),
            ("CODE", 10),
        ] {
            named.insert(kind.into(), pick(idx));
        }
    }
    // Inline images: only embedded data URIs can be shown offline.
    let mut titles: Vec<(u64, Option<String>)> = Vec::new();
    if let Some(iobjs) = root.get("inlineObjects").and_then(Value::as_object) {
        for (oid, obj) in iobjs {
            let Some(emb) = obj
                .get("inlineObjectProperties")
                .and_then(|p| p.get("embeddedObject"))
            else {
                continue;
            };
            let uri = emb
                .get("imageProperties")
                .and_then(|p| p.get("contentUri"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let Some(b64) = uri.split_once("base64,").map(|(_, b)| b.to_string()) else {
                continue;
            };
            let id = oid
                .strip_prefix("kix.img_")
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(doc.images.len() as u64 + 1);
            let w_in = emb
                .get("size")
                .and_then(|s| mag(s.get("width")))
                .map_or(3.0, |m| m / 72.0);
            let h_in = emb
                .get("size")
                .and_then(|s| mag(s.get("height")))
                .map_or(2.0, |m| m / 72.0);
            let nat_json = emb.get("naturalSize").and_then(|n| {
                Some([
                    n.get("width")?.as_u64()? as u32,
                    n.get("height")?.as_u64()? as u32,
                ])
            });
            let nat = nat_json.unwrap_or_else(|| {
                base64_decode(&b64)
                    .and_then(|b| image::load_from_memory(&b).ok())
                    .map_or([(w_in * 96.0) as u32, (h_in * 96.0) as u32], |i| {
                        [i.width(), i.height()]
                    })
            });
            titles.push((
                id,
                emb.get("title").and_then(Value::as_str).map(str::to_string),
            ));
            doc.images.push(DocImage {
                id,
                name: format!("\u{0}{oid}"),
                natural_size: [nat[0].max(1), nat[1].max(1)],
                width_in: w_in,
                height_in: h_in,
                caption: emb
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                png_b64: b64,
                seamless: false,
            });
        }
    }
    // header / footer text takes NORMAL_TEXT plus its own run formatting
    if let Some((base, _)) = named.get("NORMAL_TEXT").copied() {
        for (ts, footer) in [(hdr_ts.take(), false), (ftr_ts.take(), true)] {
            if let Some(ts) = ts {
                let mut st = base;
                apply_text_style(&mut st, &ts, &mut doc);
                st.link = 0;
                if footer {
                    doc.footer_style = Some(st);
                } else {
                    doc.header_style = Some(st);
                }
            }
        }
    }
    let empty = serde_json::Map::new();
    let lists = root
        .get("lists")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let mut ctx = Ctx {
        named,
        lists,
        list_ids: HashMap::new(),
        footnotes: root.get("footnotes").and_then(Value::as_object),
        notes: Vec::new(),
        pending_break: false,
    };
    if let Some(content) = root
        .get("body")
        .and_then(|b| b.get("content"))
        .and_then(Value::as_array)
    {
        read_content(&mut doc, &mut ctx, content);
    }
    // image names carried the object id while linking; restore readable names
    for im in doc.images.iter_mut() {
        if im.name.starts_with('\u{0}') {
            im.name = titles
                .iter()
                .find(|(id, _)| *id == im.id)
                .and_then(|(_, t)| t.clone())
                .unwrap_or_else(|| format!("Image {}", im.id));
        }
    }
    // Syllabary .gdoc exports also carry their table geometry
    if let Some(tbls) = v
        .get("tables")
        .and_then(|x| serde_json::from_value::<Vec<DocTable>>(x.clone()).ok())
    {
        if !tbls.is_empty() {
            doc.tables = tbls;
        }
    }
    // Docs' "collapse lists" spacing mode: no extra space between items of the same list
    for i in 1..doc.paras.len() {
        let (a, b) = (doc.paras[i - 1].fmt, doc.paras[i].fmt);
        if a.list_id > 0
            && a.list != ListKind::None
            && b.list != ListKind::None
            && a.list_id == b.list_id
            && a.table_id == b.table_id
        {
            doc.paras[i - 1].fmt.space_after = 0.0;
            doc.paras[i].fmt.space_before = 0.0;
        }
    }
    if !ctx.notes.is_empty() {
        let (base, _) = ctx
            .named
            .get("NORMAL_TEXT")
            .copied()
            .unwrap_or_else(|| default_named("NORMAL_TEXT"));
        let ns = CharStyle {
            size: (base.size - 1.0).max(7.0),
            ..base
        };
        let mut sep = Para::new(
            ns,
            ParaFmt {
                space_before: 12.0,
                ..ParaFmt::default()
            },
        );
        sep.fmt.pborders.top = Some(BorderSpec {
            width_pt: 0.5,
            color: [0, 0, 0],
        });
        sep.fmt.right_in = (doc.page.width - doc.page.margin_left - doc.page.margin_right) * 0.66;
        doc.paras.push(sep);
        for (k, t) in ctx.notes.iter().enumerate() {
            let mut p = Para::new(ns, ParaFmt::default());
            for ch in (k + 1).to_string().chars() {
                p.text.push(ch);
                p.styles.push(CharStyle { script: 1, ..ns });
            }
            for ch in format!(" {t}").chars() {
                p.text.push(ch);
                p.styles.push(ns);
            }
            doc.paras.push(p);
        }
    }
    if doc.paras.is_empty() {
        let (base, f) = default_named("NORMAL_TEXT");
        doc.paras.push(Para::new(base, f));
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_named_styles_lists_and_tables() {
        let json = serde_json::json!({
            "title": "T",
            "namedStyles": { "styles": [
                { "namedStyleType": "NORMAL_TEXT", "textStyle": { "fontSize": { "magnitude": 11, "unit": "PT" }, "weightedFontFamily": { "fontFamily": "Georgia" } }, "paragraphStyle": { "lineSpacing": 115 } },
                { "namedStyleType": "HEADING_1", "textStyle": { "fontSize": { "magnitude": 20, "unit": "PT" } } }
            ]},
            "lists": { "kix.abc": { "listProperties": { "nestingLevels": [
                { "glyphType": "DECIMAL", "glyphFormat": "%0.", "indentStart": { "magnitude": 36, "unit": "PT" }, "indentFirstLine": { "magnitude": 18, "unit": "PT" }, "startNumber": 3 }
            ]}}},
            "body": { "content": [
                { "paragraph": { "paragraphStyle": { "namedStyleType": "HEADING_1" }, "elements": [ { "textRun": { "content": "Head\n" } } ] } },
                { "paragraph": { "bullet": { "listId": "kix.abc" }, "paragraphStyle": { "namedStyleType": "NORMAL_TEXT" }, "elements": [ { "textRun": { "content": "Item\u{000B}more\n", "textStyle": { "bold": true } } } ] } },
                { "table": { "columns": 2, "tableRows": [ { "tableCells": [
                    { "content": [ { "paragraph": { "elements": [ { "textRun": { "content": "A\n" } } ] } } ] },
                    { "content": [ { "paragraph": { "elements": [ { "textRun": { "content": "B\n" } } ] } } ] }
                ] } ] } }
            ]}
        });
        let d = from_api(&json);
        assert_eq!(font_name(d.paras[0].styles[0].font), "Georgia");
        assert_eq!(d.paras[0].styles[0].size, 20.0);
        assert_eq!(d.paras[1].string(), "Item\nmore");
        assert!(d.paras[1].styles[0].bold);
        assert_eq!(d.list_marker(1), "3.");
        assert!((d.indents_in(&d.paras[1].fmt).0 - 0.5).abs() < 0.01);
        assert_eq!(d.tables.len(), 1);
        assert_eq!(d.paras[3].fmt.table_col, 1);
    }
}

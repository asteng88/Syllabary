//! Rich Text Format reader and writer.
//!
//! The reader follows the RTF 1.9 spec closely enough to reproduce what Word / WordPad / TextEdit
//! show: font & color tables, stylesheet, paragraph indents / tab stops / borders / exact line
//! spacing, character formatting (caps, letter spacing, shading), Word list tables, real tables
//! (`\trowd … \cell … \row`), pictures (PNG / JPEG), hyperlink fields, headers / footers,
//! document info and page setup. The writer emits the same constructs so other apps render the
//! document the way the editor does.

use std::collections::HashMap;

use crate::doc::*;

// ───────────────────────── tokenizer ─────────────────────────

#[derive(Debug, Clone)]
enum RTok {
    Open,
    Close,
    Word(String, Option<i32>),
    Sym(char),
    Hex(u8),
    Text(char),
    Bin(Vec<u8>),
}

fn tokenize(src: &str) -> Vec<RTok> {
    let b: Vec<char> = src.chars().collect();
    let mut out = Vec::with_capacity(b.len() / 2);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            '{' => {
                out.push(RTok::Open);
                i += 1;
            }
            '}' => {
                out.push(RTok::Close);
                i += 1;
            }
            '\\' => {
                i += 1;
                let Some(&c2) = b.get(i) else { break };
                if c2.is_ascii_alphabetic() {
                    let start = i;
                    while i < b.len() && b[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let word: String = b[start..i].iter().collect();
                    let ns = i;
                    if i < b.len() && (b[i] == '-' || b[i].is_ascii_digit()) {
                        i += 1;
                        while i < b.len() && b[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    let num = if i > ns {
                        b[ns..i]
                            .iter()
                            .collect::<String>()
                            .parse::<i64>()
                            .ok()
                            .map(|v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
                    } else {
                        None
                    };
                    if i < b.len() && b[i] == ' ' {
                        i += 1;
                    }
                    if word == "bin" {
                        let n = num.unwrap_or(0).max(0) as usize;
                        let end = (i + n).min(b.len());
                        out.push(RTok::Bin(
                            b[i..end].iter().map(|&c| c as u32 as u8).collect(),
                        ));
                        i = end;
                        continue;
                    }
                    out.push(RTok::Word(word, num));
                } else if c2 == '\'' {
                    let hex: String = b[(i + 1).min(b.len())..(i + 3).min(b.len())]
                        .iter()
                        .collect();
                    i += 3;
                    if let Ok(v) = u8::from_str_radix(&hex, 16) {
                        out.push(RTok::Hex(v));
                    }
                } else if c2 == '\r' || c2 == '\n' {
                    // "\<newline>" is a paragraph mark
                    out.push(RTok::Word("par".into(), None));
                    i += 1;
                } else {
                    out.push(RTok::Sym(c2));
                    i += 1;
                }
            }
            '\r' | '\n' => i += 1,
            _ => {
                out.push(RTok::Text(c));
                i += 1;
            }
        }
    }
    out
}

pub fn cp1252_char(v: u8) -> char {
    cp1252(v)
}

fn cp1252(v: u8) -> char {
    const HI: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    if (0x80..0xa0).contains(&v) {
        HI[(v - 0x80) as usize]
    } else {
        v as char
    }
}

/// Mac Roman upper half (for `\mac` documents).
fn mac_roman(v: u8) -> char {
    const HI: &str = "ÄÅÇÉÑÖÜáàâäãåçéèêëíìîïñóòôöõúùûü†°¢£§•¶ß®©™´¨≠ÆØ∞±≤≥¥µ∂∑∏π∫ªºΩæø¿¡¬√ƒ≈∆«»… ÀÃÕŒœ–—“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔ\u{F8FF}ÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";
    if v < 0x80 {
        v as char
    } else {
        HI.chars().nth((v - 0x80) as usize).unwrap_or('?')
    }
}

// ───────────────────────── reader ─────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Dest {
    Body,
    Skip,
    FontTbl,
    ColorTbl,
    StyleSheet,
    Info(u8),
    Pict,
    Header,
    Footer,
    FldInst,
    ListTable,
    ListOverride,
    ListText,
    LevelText,
    LevelNumbers,
    Legacy,
    /// {\*\sycaption …} / {\*\syimgname …}: image metadata written by Syllabary
    ImgMeta(u8),
}

#[derive(Clone, Copy)]
struct GState {
    chr: CharStyle,
    dest: Dest,
    uc: usize,
    hidden: bool,
    /// font table index of the current font (for symbol-charset mapping)
    font_num: i32,
}

#[derive(Default, Clone)]
struct RtfFont {
    name: String,
    charset: i32,
}

#[derive(Default)]
struct PictState {
    hex: Vec<u8>,
    hi_nibble: Option<u8>,
    kind: u8, // 1 png, 2 jpeg, 0 unsupported
    picw: i32,
    pich: i32,
    goalw: i32,
    goalh: i32,
    scalex: i32,
    scaley: i32,
}

#[derive(Default, Clone)]
struct RowDef {
    left: i32,
    gaph: i32,
    cells: Vec<TableCellSpec>,
    rights: Vec<i32>,
    /// borders / shading accumulating for the next \cellx
    pending: TableCellSpec,
    height: i32,
}

/// Which border the next \brdrs / \brdrw / \brdrcf applies to.
#[derive(Clone, Copy, PartialEq)]
enum BorderTarget {
    None,
    Para(u8),
    Cell(u8),
}

struct ListLevelBuild {
    lvl: ListLevel,
    font_num: Option<i32>,
    text_raw: Vec<u32>,
}

pub fn from_rtf(src: &str) -> Document {
    let toks = tokenize(src);
    let mut doc = Document {
        paras: Vec::new(),
        ..Default::default()
    };
    // RTF's own defaults: Letter, 1.25" side margins, 1" top / bottom, 12pt
    doc.page.margin_left = 1.25;
    doc.page.margin_right = 1.25;
    let named = named_styles();

    let mut fonts: HashMap<i32, RtfFont> = HashMap::new();
    let mut deff = 0i32;
    let mut mac = false;
    let mut colors: Vec<Option<[u8; 3]>> = Vec::new();
    let mut cur_rgb = [0u8; 3];
    let mut rgb_set = false;
    let mut font_entry = RtfFont::default();
    let mut font_entry_num = 0i32;

    // stylesheet
    let mut style_defs: HashMap<i32, (ParaFmt, CharStyle, String)> = HashMap::new();
    let mut cstyle_defs: HashMap<i32, CharStyle> = HashMap::new();
    let mut ss_num: Option<i32> = None;
    let mut ss_is_char = false;
    let mut ss_name = String::new();
    let mut ss_depth = 0usize;

    // lists
    let mut lists: HashMap<i32, Vec<ListLevel>> = HashMap::new(); // \listid → levels
    let mut cur_list_levels: Vec<ListLevel> = Vec::new();
    let mut cur_listid: Option<i32> = None;
    let mut cur_level: Option<ListLevelBuild> = None;
    let mut overrides: HashMap<i32, i32> = HashMap::new(); // \ls → \listid
    let mut ov_listid: Option<i32> = None;
    let mut ov_ls: Option<i32> = None;
    let mut ls_to_def: HashMap<i32, u16> = HashMap::new();
    let mut level_depth: Option<usize> = None;
    let mut sy_list: Option<i32> = None;

    // document text state
    let plain = |fonts: &HashMap<i32, RtfFont>, deff: i32| CharStyle {
        font: fonts.get(&deff).map_or(0, |f| font_index(&f.name)),
        size: 12.0,
        ..CharStyle::default()
    };
    let mut gs = GState {
        chr: CharStyle {
            font: 0,
            size: 12.0,
            ..CharStyle::default()
        },
        dest: Dest::Body,
        uc: 1,
        hidden: false,
        font_num: 0,
    };
    let mut stack: Vec<GState> = Vec::new();
    let mut pf = ParaFmt::default();
    let mut para = Para::new(gs.chr, pf);
    let mut skip_chars = 0usize;
    let mut pending_tab_kind = TabKind::Left;
    let mut pending_leader = 0u8;
    let mut border_target = BorderTarget::None;
    let mut sl: Option<(i32, i32)> = None; // (\sl, \slmult)
    let mut ls: Option<i32> = None;
    let mut ilvl = 0u8;
    let mut marker_text = String::new();
    let mut pending_page_break = false;
    let mut sect_continuous = false;

    // fields
    let mut fldinst = String::new();
    let mut field_link_stack: Vec<u16> = Vec::new();
    let mut fldrslt_depth: Option<usize> = None;
    let mut field_page = false;
    let mut meta_text = String::new();
    let mut last_field_page = false;

    // header / footer / info text
    let mut hf_text = String::new();
    let mut hf_align = Align::Left;
    let mut hf_italic = false;
    let mut hf_first: Option<CharStyle> = None;
    let mut hf_styled = false;
    let app_file = src.contains("\\generator Syllabary");
    let mut info_text = String::new();

    // pictures
    let mut pict = PictState::default();

    // tables
    let mut in_tbl = false;
    let mut row_def = RowDef::default();
    let mut cur_table: Option<(u32, Vec<TableRowSpec>, Vec<f32>)> = None;
    let mut cell_idx = 0u16;
    let mut row_idx = 0u16;
    let mut next_tbl = 1u32;
    let mut nowrap_seen = false;

    // legacy (older Syllabary RTF) markers
    let mut legacy_tables: Option<Vec<DocTable>> = None;
    let mut legacy_b64 = String::new();
    let mut legacy_img: Option<(u64, f32, f32)> = None;

    let finish_para = |doc: &mut Document,
                       para: &mut Para,
                       pf: &ParaFmt,
                       marker_text: &mut String,
                       in_tbl: bool,
                       cell_idx: u16,
                       row_idx: u16,
                       cur_table: &Option<(u32, Vec<TableRowSpec>, Vec<f32>)>,
                       end: CharStyle| {
        let mut p = std::mem::replace(para, Para::new(end, ParaFmt::default()));
        p.fmt = ParaFmt {
            image_id: p.fmt.image_id,
            page_break_before: p.fmt.page_break_before || pf.page_break_before,
            ..*pf
        };
        p.end_style = end;
        if in_tbl {
            if let Some((tid, _, _)) = cur_table {
                p.fmt.table_id = *tid;
                p.fmt.table_row = row_idx;
                p.fmt.table_col = cell_idx;
            }
        } else {
            p.fmt.table_id = 0;
        }
        // Old-style list text ({\pntext …} / {\listtext …}) without a list table
        if p.fmt.list == ListKind::None && !marker_text.trim().is_empty() {
            let m = marker_text.trim();
            p.fmt.list = if m.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) {
                ListKind::Number
            } else {
                ListKind::Bullet
            };
        }
        marker_text.clear();
        doc.paras.push(p);
    };

    let color_at = |colors: &Vec<Option<[u8; 3]>>, n: Option<i32>| -> Option<[u8; 3]> {
        colors
            .get(n.unwrap_or(0).max(0) as usize)
            .copied()
            .flatten()
    };

    let mut i = 0usize;
    while i < toks.len() {
        let tok = toks[i].clone();
        i += 1;
        match tok {
            RTok::Open => {
                stack.push(gs);
                // "{\*\dest …}": unknown ignorable destinations are skipped
                if matches!(toks.get(i), Some(RTok::Sym('*'))) {
                    let known = matches!(
                        toks.get(i + 1),
                        Some(RTok::Word(w, _)) if matches!(w.as_str(), "sycaption" | "syimgname" | "fldinst" | "listtable" | "listoverridetable" | "shppict" | "cs" | "syllabarytbls" | "scribetbls" | "pn" | "listtext" | "pntext")
                    );
                    if !known {
                        gs.dest = Dest::Skip;
                    }
                }
                if gs.dest == Dest::StyleSheet {
                    ss_depth += 1;
                    if ss_depth == 1 {
                        ss_num = Some(0);
                        ss_is_char = false;
                        ss_name.clear();
                        pf = ParaFmt::default();
                        gs.chr = plain(&fonts, deff);
                    }
                }
            }
            RTok::Close => {
                let closing = gs.dest;
                match closing {
                    Dest::FontTbl if !font_entry.name.is_empty() => {
                        fonts.insert(font_entry_num, std::mem::take(&mut font_entry));
                    }
                    Dest::StyleSheet if ss_depth == 1 => {
                        if let Some(n) = ss_num.take() {
                            let name = ss_name.trim().trim_end_matches(';').trim().to_string();
                            if ss_is_char {
                                cstyle_defs.insert(n, gs.chr);
                            } else {
                                let mut f = pf;
                                f.style = style_index_for(&name);
                                style_defs.insert(n, (f, gs.chr, name));
                            }
                        }
                        pf = ParaFmt::default();
                    }
                    Dest::Pict => {
                        if stack.last().is_none_or(|g| g.dest != Dest::Pict) {
                            if let Some(img) = finish_pict(&mut doc, &pict) {
                                insert_image_para(&mut doc, &mut para, pf, img, gs.chr);
                            }
                            pict = PictState::default();
                        }
                    }
                    Dest::Header | Dest::Footer
                        if stack.last().is_none_or(|g| g.dest != closing) =>
                    {
                        let t = hf_text.trim().to_string();
                        // older Syllabary files used the editor's default header look
                        let st = if !app_file || hf_styled {
                            hf_first.take()
                        } else {
                            None
                        };
                        hf_first = None;
                        hf_styled = false;
                        if closing == Dest::Header {
                            if !t.is_empty() {
                                doc.header = t;
                                doc.header_align = hf_align;
                                doc.header_italic = hf_italic;
                                doc.header_style = st;
                            }
                        } else {
                            if field_page {
                                doc.page.page_numbers = true;
                            }
                            if !t.is_empty() || field_page {
                                doc.footer = t;
                                doc.footer_align = hf_align;
                                doc.footer_italic = hf_italic;
                                doc.footer_style = st;
                            }
                        }
                        hf_text.clear();
                        field_page = false;
                    }
                    Dest::Info(k) if stack.last().is_none_or(|g| g.dest != closing) => {
                        let t = info_text.trim().to_string();
                        match k {
                            1 => doc.info.title = t,
                            2 => doc.info.author = t,
                            3 => doc.info.subject = t,
                            4 => doc.info.keywords = t,
                            5 => doc.info.notes = t,
                            _ => {}
                        }
                        info_text.clear();
                    }
                    Dest::ImgMeta(k) if stack.last().is_none_or(|g| g.dest != closing) => {
                        if let Some(im) = doc.images.last_mut() {
                            if k == 1 {
                                im.caption = std::mem::take(&mut meta_text);
                            } else {
                                im.name = std::mem::take(&mut meta_text);
                            }
                        }
                    }
                    Dest::FldInst if stack.last().is_none_or(|g| g.dest != Dest::FldInst) => {
                        let inst = fldinst.trim().to_string();
                        last_field_page = false;
                        let upper = inst.to_ascii_uppercase();
                        if let Some(rest) =
                            upper.strip_prefix("HYPERLINK").map(|_| inst[9..].trim())
                        {
                            let url = rest.split('"').nth(1).map_or_else(
                                || rest.split_whitespace().next().unwrap_or("").to_string(),
                                |u| u.to_string(),
                            );
                            field_link_stack.push(if url.is_empty() {
                                0
                            } else {
                                doc.add_link(&url)
                            });
                        } else {
                            if upper.starts_with("PAGE") || upper.starts_with("NUMPAGES") {
                                field_page = true;
                                last_field_page = true;
                            }
                            field_link_stack.push(0);
                        }
                        fldinst.clear();
                    }
                    _ => {}
                }
                if closing == Dest::StyleSheet {
                    ss_depth = ss_depth.saturating_sub(1);
                }
                if let Some(d) = fldrslt_depth {
                    if stack.len() <= d {
                        fldrslt_depth = None;
                        field_link_stack.pop();
                    }
                }
                if level_depth == Some(stack.len()) {
                    level_depth = None;
                    if let Some(lb) = cur_level.take() {
                        cur_list_levels.push(finish_level(lb, &fonts));
                        if let Some(id) = cur_listid {
                            lists.insert(id, cur_list_levels.clone());
                        }
                    }
                }
                if let Some(prev) = stack.pop() {
                    gs = prev;
                    if closing == Dest::FontTbl && prev.dest != Dest::FontTbl {
                        // the default font applies from here on
                        if let Some(f) = fonts.get(&deff) {
                            gs.chr.font = font_index(&f.name);
                            gs.font_num = deff;
                        }
                    }
                }
            }
            RTok::Bin(bytes) => {
                if gs.dest == Dest::Pict {
                    pict.hex.extend(bytes);
                }
            }
            RTok::Hex(v) => {
                if skip_chars > 0 {
                    skip_chars -= 1;
                    continue;
                }
                let charset = fonts.get(&gs.font_num).map_or(0, |f| f.charset);
                let ch = if mac { mac_roman(v) } else { cp1252(v) };
                match gs.dest {
                    Dest::Pict => push_pict_hex(&mut pict, v),
                    Dest::LevelText => {
                        if let Some(lb) = cur_level.as_mut() {
                            lb.text_raw.push(v as u32);
                        }
                    }
                    Dest::Body if !gs.hidden => {
                        let ch = if charset == 2
                            && matches!(
                                v,
                                0xA7 | 0xA8 | 0xB7 | 0xD8 | 0xFC | 0x6F | 0x6C | 0x6E | 0x76 | 0x75
                            ) {
                            let fname = fonts
                                .get(&gs.font_num)
                                .map_or("Symbol".to_string(), |f| f.name.clone());
                            map_symbol_bullet(
                                &char::from_u32(0xF000 + v as u32).unwrap_or('•').to_string(),
                                &fname,
                            )
                            .0
                            .chars()
                            .next()
                            .unwrap_or(ch)
                        } else {
                            ch
                        };
                        push_char(&mut para, ch, gs.chr);
                    }
                    Dest::Header | Dest::Footer => {
                        if hf_first.is_none() && !ch.is_whitespace() {
                            hf_first = Some(gs.chr);
                        }
                        hf_text.push(ch)
                    }
                    Dest::Info(_) => info_text.push(ch),
                    Dest::FldInst => fldinst.push(ch),
                    Dest::ImgMeta(_) => meta_text.push(ch),
                    Dest::ListText => marker_text.push(ch),
                    Dest::FontTbl => font_entry.name.push(ch),
                    Dest::StyleSheet => ss_name.push(ch),
                    _ => {}
                }
            }
            RTok::Text(c) => {
                if skip_chars > 0 {
                    skip_chars -= 1;
                    continue;
                }
                // raw 8-bit characters (files are read byte-per-char) use the document code page
                let c = if (0x80..=0xFF).contains(&(c as u32)) && gs.dest != Dest::Pict {
                    if mac {
                        mac_roman(c as u32 as u8)
                    } else {
                        cp1252(c as u32 as u8)
                    }
                } else {
                    c
                };
                match gs.dest {
                    Dest::Body if !gs.hidden => push_char(&mut para, c, gs.chr),
                    Dest::Pict => {
                        if let Some(v) = c.to_digit(16) {
                            match pict.hi_nibble.take() {
                                Some(h) => pict.hex.push(h << 4 | v as u8),
                                None => pict.hi_nibble = Some(v as u8),
                            }
                        }
                    }
                    Dest::FontTbl => {
                        if c == ';' {
                            fonts.insert(font_entry_num, std::mem::take(&mut font_entry));
                        } else {
                            font_entry.name.push(c);
                        }
                    }
                    Dest::ColorTbl => {
                        if c == ';' {
                            colors.push(if rgb_set { Some(cur_rgb) } else { None });
                            cur_rgb = [0; 3];
                            rgb_set = false;
                        }
                    }
                    Dest::StyleSheet => ss_name.push(c),
                    Dest::Header | Dest::Footer => {
                        if hf_first.is_none() && !c.is_whitespace() {
                            hf_first = Some(gs.chr);
                        }
                        hf_text.push(c)
                    }
                    Dest::Info(_) => info_text.push(c),
                    Dest::FldInst => fldinst.push(c),
                    Dest::ListText => marker_text.push(c),
                    Dest::LevelText => {
                        if c != ';' {
                            if let Some(lb) = cur_level.as_mut() {
                                lb.text_raw.push(c as u32);
                            }
                        }
                    }
                    Dest::Legacy => legacy_b64.push(c),
                    Dest::ImgMeta(_) => meta_text.push(c),
                    _ => {}
                }
            }
            RTok::Sym(c) => {
                let text_dest = matches!(
                    gs.dest,
                    Dest::Body | Dest::Header | Dest::Footer | Dest::Info(_)
                );
                let ch = match c {
                    '\\' | '{' | '}' => Some(c),
                    '~' => Some('\u{a0}'),
                    '_' => Some('\u{2011}'),
                    '-' => None,
                    _ => None,
                };
                if let Some(ch) = ch {
                    if skip_chars > 0 {
                        skip_chars -= 1;
                    } else if text_dest {
                        match gs.dest {
                            Dest::Body if !gs.hidden => push_char(&mut para, ch, gs.chr),
                            Dest::Header | Dest::Footer => {
                                if hf_first.is_none() && !ch.is_whitespace() {
                                    hf_first = Some(gs.chr);
                                }
                                hf_text.push(ch)
                            }
                            Dest::Info(_) => info_text.push(ch),
                            _ => {}
                        }
                    } else if gs.dest == Dest::FldInst {
                        fldinst.push(ch);
                    }
                }
            }
            RTok::Word(w, n) => {
                let on = n != Some(0);
                if skip_chars > 0 && !matches!(w.as_str(), "par" | "line" | "tab" | "cell" | "row")
                {
                    skip_chars -= 1;
                    continue;
                }
                skip_chars = 0;
                // destinations
                match w.as_str() {
                    "fonttbl" => {
                        gs.dest = Dest::FontTbl;
                        continue;
                    }
                    "colortbl" => {
                        gs.dest = Dest::ColorTbl;
                        colors.clear();
                        continue;
                    }
                    "stylesheet" => {
                        gs.dest = Dest::StyleSheet;
                        ss_depth = 0;
                        continue;
                    }
                    "info" => {
                        gs.dest = Dest::Info(0);
                        continue;
                    }
                    "title" if matches!(gs.dest, Dest::Info(_)) => {
                        gs.dest = Dest::Info(1);
                        info_text.clear();
                        continue;
                    }
                    "author" if matches!(gs.dest, Dest::Info(_)) => {
                        gs.dest = Dest::Info(2);
                        info_text.clear();
                        continue;
                    }
                    "subject" if matches!(gs.dest, Dest::Info(_)) => {
                        gs.dest = Dest::Info(3);
                        info_text.clear();
                        continue;
                    }
                    "keywords" if matches!(gs.dest, Dest::Info(_)) => {
                        gs.dest = Dest::Info(4);
                        info_text.clear();
                        continue;
                    }
                    "doccomm" if matches!(gs.dest, Dest::Info(_)) => {
                        gs.dest = Dest::Info(5);
                        info_text.clear();
                        continue;
                    }
                    "operator" | "company" | "manager" | "category" | "comment" | "hlinkbase"
                    | "creatim" | "revtim" | "printim" | "buptim"
                        if matches!(gs.dest, Dest::Info(_)) =>
                    {
                        gs.dest = Dest::Skip;
                        continue;
                    }
                    "pict" => {
                        gs.dest = Dest::Pict;
                        pict = PictState {
                            scalex: 100,
                            scaley: 100,
                            ..PictState::default()
                        };
                        continue;
                    }
                    "shppict" => continue,
                    "nonshppict" | "themedata" | "colorschememapping" | "latentstyles"
                    | "datastore" | "xmlnstbl" | "rsidtbl" | "generator" | "mmathPr"
                    | "filetbl" | "revtbl" | "fchars" | "lchars" | "pgdsctbl" | "bkmkstart"
                    | "bkmkend" | "annotation" | "atnid" | "atnauthor" | "footnote" | "object"
                    | "objdata" | "txe" | "sp" | "shpinst" | "do" => {
                        gs.dest = Dest::Skip;
                        continue;
                    }
                    "header" | "headerr" => {
                        gs.dest = Dest::Header;
                        hf_first = None;
                        hf_text.clear();
                        hf_align = Align::Left;
                        hf_italic = false;
                        continue;
                    }
                    "footer" | "footerr" => {
                        gs.dest = Dest::Footer;
                        hf_first = None;
                        hf_text.clear();
                        hf_align = Align::Left;
                        hf_italic = false;
                        continue;
                    }
                    "headerl" | "headerf" | "footerl" | "footerf" => {
                        gs.dest = Dest::Skip;
                        continue;
                    }
                    "fldinst" => {
                        gs.dest = Dest::FldInst;
                        fldinst.clear();
                        continue;
                    }
                    "fldrslt" => {
                        fldrslt_depth = Some(stack.len());
                        if last_field_page && matches!(gs.dest, Dest::Header | Dest::Footer) {
                            // the page number is drawn by the editor, not the cached result
                            gs.dest = Dest::Skip;
                        }
                        if let Some(&l) = field_link_stack.last() {
                            if l > 0 {
                                gs.chr.link = l;
                            }
                        }
                        continue;
                    }
                    "listtable" => {
                        gs.dest = Dest::ListTable;
                        continue;
                    }
                    "listoverridetable" => {
                        gs.dest = Dest::ListOverride;
                        continue;
                    }
                    "listtext" | "pntext" => {
                        gs.dest = Dest::ListText;
                        continue;
                    }
                    "pn" => {
                        gs.dest = Dest::Skip;
                        continue;
                    }
                    "sycaption" => {
                        gs.dest = Dest::ImgMeta(1);
                        meta_text.clear();
                        continue;
                    }
                    "syimgname" => {
                        gs.dest = Dest::ImgMeta(2);
                        meta_text.clear();
                        continue;
                    }
                    "syseamless" => {
                        if let Some(im) = doc.images.last_mut() {
                            im.seamless = true;
                        }
                        continue;
                    }
                    "syllabarytbls" | "scribetbls" => {
                        gs.dest = Dest::Legacy;
                        legacy_b64.clear();
                        legacy_tables = Some(Vec::new());
                        continue;
                    }
                    _ => {}
                }
                match gs.dest {
                    Dest::Skip
                    | Dest::Legacy
                    | Dest::ImgMeta(_)
                    | Dest::Info(_)
                    | Dest::FldInst
                    | Dest::ListText => {
                        if gs.dest == Dest::ListText && w == "tab" {
                            marker_text.push('\t');
                        }
                        if w == "u" && matches!(gs.dest, Dest::Info(_) | Dest::ImgMeta(_)) {
                            if let Some(ch) = n.and_then(|v| {
                                char::from_u32(if v < 0 { (v + 65536) as u32 } else { v as u32 })
                            }) {
                                if matches!(gs.dest, Dest::ImgMeta(_)) {
                                    meta_text.push(ch);
                                } else {
                                    info_text.push(ch);
                                }
                                skip_chars = gs.uc;
                            }
                        }
                        continue;
                    }
                    Dest::FontTbl => {
                        match w.as_str() {
                            "f" => {
                                if !font_entry.name.is_empty() {
                                    fonts.insert(font_entry_num, std::mem::take(&mut font_entry));
                                }
                                font_entry_num = n.unwrap_or(0);
                            }
                            "fcharset" => font_entry.charset = n.unwrap_or(0),
                            _ => {}
                        }
                        continue;
                    }
                    Dest::ColorTbl => {
                        match w.as_str() {
                            "red" => cur_rgb[0] = n.unwrap_or(0).clamp(0, 255) as u8,
                            "green" => cur_rgb[1] = n.unwrap_or(0).clamp(0, 255) as u8,
                            "blue" => cur_rgb[2] = n.unwrap_or(0).clamp(0, 255) as u8,
                            _ => {}
                        }
                        rgb_set = true;
                        continue;
                    }
                    Dest::Pict => {
                        match w.as_str() {
                            "pngblip" => pict.kind = 1,
                            "jpegblip" => pict.kind = 2,
                            "emfblip" | "wmetafile" | "macpict" | "dibitmap" | "wbitmap" => {
                                pict.kind = 0
                            }
                            "picw" => pict.picw = n.unwrap_or(0),
                            "pich" => pict.pich = n.unwrap_or(0),
                            "picwgoal" => pict.goalw = n.unwrap_or(0),
                            "pichgoal" => pict.goalh = n.unwrap_or(0),
                            "picscalex" => pict.scalex = n.unwrap_or(100),
                            "picscaley" => pict.scaley = n.unwrap_or(100),
                            _ => {}
                        }
                        continue;
                    }
                    Dest::ListTable | Dest::LevelText | Dest::LevelNumbers => {
                        match w.as_str() {
                            "list" => {
                                cur_list_levels.clear();
                                cur_listid = None;
                            }
                            "listid" => {
                                cur_listid = n;
                                if let Some(id) = n {
                                    lists.insert(id, cur_list_levels.clone());
                                }
                            }
                            "listlevel" => {
                                level_depth = Some(stack.len());
                                cur_level = Some(ListLevelBuild {
                                    lvl: ListLevel {
                                        left_in: 0.0,
                                        first_in: 0.0,
                                        text: String::new(),
                                        ..ListLevel::default()
                                    },
                                    font_num: None,
                                    text_raw: Vec::new(),
                                })
                            }
                            "levelnfc" | "levelnfcn" => {
                                if let Some(lb) = cur_level.as_mut() {
                                    lb.lvl.fmt = match n.unwrap_or(0) {
                                        0 => NumFmt::Decimal,
                                        1 => NumFmt::UpperRoman,
                                        2 => NumFmt::LowerRoman,
                                        3 => NumFmt::UpperLetter,
                                        4 => NumFmt::LowerLetter,
                                        22 => NumFmt::DecimalZero,
                                        23 => NumFmt::Bullet,
                                        255 => NumFmt::None,
                                        _ => NumFmt::Decimal,
                                    }
                                }
                            }
                            "levelstartat" => {
                                if let Some(lb) = cur_level.as_mut() {
                                    lb.lvl.start = n.unwrap_or(1).max(0) as u32;
                                }
                            }
                            "leveljc" | "leveljcn" => {
                                if let Some(lb) = cur_level.as_mut() {
                                    lb.lvl.align = match n.unwrap_or(0) {
                                        1 => Align::Center,
                                        2 => Align::Right,
                                        _ => Align::Left,
                                    }
                                }
                            }
                            "levelfollow" => {
                                if let Some(lb) = cur_level.as_mut() {
                                    lb.lvl.suffix = n.unwrap_or(0).clamp(0, 2) as u8;
                                }
                            }
                            "leveltext" => gs.dest = Dest::LevelText,
                            "levelnumbers" => gs.dest = Dest::LevelNumbers,
                            "li" | "lin" => {
                                if let Some(lb) = cur_level.as_mut() {
                                    lb.lvl.left_in = n.unwrap_or(0) as f32 / 1440.0;
                                }
                            }
                            "fi" => {
                                if let Some(lb) = cur_level.as_mut() {
                                    lb.lvl.first_in = n.unwrap_or(0) as f32 / 1440.0;
                                }
                            }
                            "f" => {
                                if let Some(lb) = cur_level.as_mut() {
                                    lb.font_num = n;
                                }
                            }
                            "u" if gs.dest == Dest::LevelText => {
                                if let (Some(lb), Some(v)) = (cur_level.as_mut(), n) {
                                    lb.text_raw.push(if v < 0 {
                                        (v + 65536) as u32
                                    } else {
                                        v as u32
                                    });
                                    skip_chars = gs.uc;
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }
                    Dest::ListOverride => {
                        match w.as_str() {
                            "listoverride" => {
                                ov_listid = None;
                                ov_ls = None;
                            }
                            "listid" => ov_listid = n,
                            "ls" => ov_ls = n,
                            _ => {}
                        }
                        if let (Some(l), Some(s)) = (ov_listid, ov_ls) {
                            overrides.insert(s, l);
                        }
                        continue;
                    }
                    _ => {}
                }

                // ----- formatting & text (body, stylesheet, header/footer) -----
                let in_text = matches!(
                    gs.dest,
                    Dest::Body | Dest::Header | Dest::Footer | Dest::StyleSheet
                );
                if !in_text {
                    continue;
                }
                let tw_in = |v: Option<i32>| v.unwrap_or(0) as f32 / 1440.0;
                match w.as_str() {
                    // document
                    "ansi" => mac = false,
                    "mac" => mac = true,
                    "deff" => deff = n.unwrap_or(0),
                    "uc" => gs.uc = n.unwrap_or(1).max(0) as usize,
                    "paperw" => doc.page.width = (tw_in(n) * 100.0).round() / 100.0,
                    "paperh" => doc.page.height = (tw_in(n) * 100.0).round() / 100.0,
                    "margl" | "marglsxn" => doc.page.margin_left = snap(tw_in(n)),
                    "margr" | "margrsxn" => doc.page.margin_right = snap(tw_in(n)),
                    "margt" | "margtsxn" => doc.page.margin_top = snap(tw_in(n)),
                    "margb" | "margbsxn" => doc.page.margin_bottom = snap(tw_in(n)),
                    "pgwsxn" => doc.page.width = (tw_in(n) * 100.0).round() / 100.0,
                    "pghsxn" => doc.page.height = (tw_in(n) * 100.0).round() / 100.0,
                    "headery" => {
                        if n.unwrap_or(0) > 0 {
                            doc.header_dist_in = tw_in(n);
                        }
                    }
                    "footery" => {
                        if n.unwrap_or(0) > 0 {
                            doc.footer_dist_in = tw_in(n);
                        }
                    }
                    "syhfstyle" => hf_styled = true,
                    "deftab" => {
                        if n.unwrap_or(0) > 36 {
                            doc.default_tab_in = tw_in(n);
                        }
                    }
                    "pgbg" => {
                        if let Some(c) = color_at(&colors, n) {
                            doc.page.background = c;
                        }
                    }
                    "pgnum" => doc.page.page_numbers = on,
                    "pageless" => doc.pageless = on,
                    "sbknone" => sect_continuous = true,
                    "sbkpage" | "sbkodd" | "sbkeven" | "sbkcol" => sect_continuous = false,
                    "sectd" => sect_continuous = false,
                    "sect" if gs.dest == Dest::Body => {
                        if !sect_continuous {
                            pending_page_break = true;
                        }
                    }
                    // stylesheet entry ids
                    "s" if gs.dest == Dest::StyleSheet => ss_num = n,
                    "cs" | "ds" | "ts" if gs.dest == Dest::StyleSheet => {
                        ss_num = n;
                        ss_is_char = w == "cs";
                        if w != "cs" {
                            ss_num = None;
                        }
                    }
                    "sbasedon" if gs.dest == Dest::StyleSheet => {
                        if let Some((bf, bc, _)) = n.and_then(|b| style_defs.get(&b)) {
                            pf = ParaFmt {
                                style: pf.style,
                                ..*bf
                            };
                            gs.chr = *bc;
                        }
                    }
                    // paragraph
                    "pard" => {
                        pf = ParaFmt::default();
                        sl = None;
                        ls = None;
                        sy_list = None;
                        ilvl = 0;
                        in_tbl = false;
                        border_target = BorderTarget::None;
                    }
                    "s" => {
                        let k = n.unwrap_or(0);
                        if let Some((sfmt, schr, _)) = style_defs.get(&k) {
                            pf = *sfmt;
                            gs.chr = *schr;
                        } else if style_defs.is_empty() && (k as usize) < named.len() {
                            // older Syllabary files numbered styles by the editor's style list
                            pf = ParaFmt {
                                style: k as usize,
                                ..named[k as usize].fmt
                            };
                            gs.chr = named[k as usize].chr;
                        }
                    }
                    "cs" => {
                        if let Some(c) = n.and_then(|k| cstyle_defs.get(&k)) {
                            gs.chr = CharStyle {
                                link: gs.chr.link,
                                ..*c
                            };
                        }
                    }
                    "ql" => set_align(gs.dest, &mut pf, &mut hf_align, Align::Left),
                    "qc" => set_align(gs.dest, &mut pf, &mut hf_align, Align::Center),
                    "qr" => set_align(gs.dest, &mut pf, &mut hf_align, Align::Right),
                    "qj" | "qd" => set_align(gs.dest, &mut pf, &mut hf_align, Align::Justify),
                    "li" | "lin" => {
                        pf.indent = 0;
                        pf.left_in = tw_in(n);
                    }
                    "ri" | "rin" => pf.right_in = tw_in(n),
                    "fi" => pf.first_in = tw_in(n),
                    "sb" => pf.space_before = (n.unwrap_or(0) as f32 / 20.0).max(0.0),
                    "sa" => pf.space_after = (n.unwrap_or(0) as f32 / 20.0).max(0.0),
                    "sbauto" if on => pf.space_before = 14.0,
                    "saauto" if on => pf.space_after = 14.0,
                    "sl" => {
                        sl = Some((n.unwrap_or(0), sl.map_or(0, |s| s.1)));
                        apply_sl(&mut pf, sl);
                    }
                    "slmult" => {
                        sl = Some((sl.map_or(0, |s| s.0), n.unwrap_or(0)));
                        apply_sl(&mut pf, sl);
                    }
                    "pagebb" => pf.page_break_before = on,
                    "keepn" | "keep" | "widctlpar" | "nowidctlpar" | "hyphpar" => {}
                    "intbl" => in_tbl = true,
                    "itap" => in_tbl = n.unwrap_or(1) > 0,
                    "tqr" => pending_tab_kind = TabKind::Right,
                    "tqc" => pending_tab_kind = TabKind::Center,
                    "tqdec" => pending_tab_kind = TabKind::Decimal,
                    "tldot" => pending_leader = 1,
                    "tlhyph" => pending_leader = 2,
                    "tlul" | "tlth" => pending_leader = 3,
                    "tlmdot" => pending_leader = 4,
                    "tx" | "tb" => {
                        if w == "tx" {
                            pf.tabs.set(TabStop {
                                pos_in: tw_in(n),
                                kind: pending_tab_kind,
                                leader: pending_leader,
                            });
                        }
                        pending_tab_kind = TabKind::Left;
                        pending_leader = 0;
                    }
                    "cbpat" => pf.shading = color_at(&colors, n).filter(|c| *c != [255, 255, 255]),
                    "brdrt" => border_target = BorderTarget::Para(0),
                    "brdrb" => border_target = BorderTarget::Para(1),
                    "brdrl" => border_target = BorderTarget::Para(2),
                    "brdrr" => border_target = BorderTarget::Para(3),
                    "box" => border_target = BorderTarget::Para(4),
                    "clbrdrt" => border_target = BorderTarget::Cell(0),
                    "clbrdrb" => border_target = BorderTarget::Cell(1),
                    "clbrdrl" => border_target = BorderTarget::Cell(2),
                    "clbrdrr" => border_target = BorderTarget::Cell(3),
                    "brdrs" | "brdrth" | "brdrsh" | "brdrdb" | "brdrdot" | "brdrdash"
                    | "brdrhair" | "brdrw" | "brdrcf" | "brdrnone" | "brdrnil" => {
                        let none = matches!(w.as_str(), "brdrnone" | "brdrnil");
                        let upd = |b: &mut Option<BorderSpec>| {
                            if none {
                                *b = None;
                                return;
                            }
                            let mut s = b.unwrap_or(BorderSpec {
                                width_pt: 0.5,
                                color: [0, 0, 0],
                            });
                            match w.as_str() {
                                "brdrw" => {
                                    s.width_pt = (n.unwrap_or(10) as f32 / 20.0).clamp(0.25, 12.0)
                                }
                                "brdrcf" => s.color = color_at(&colors, n).unwrap_or([0, 0, 0]),
                                "brdrth" => s.width_pt = s.width_pt.max(1.0),
                                "brdrdb" => s.width_pt = s.width_pt.max(1.5),
                                _ => {}
                            }
                            *b = Some(s);
                        };
                        match border_target {
                            BorderTarget::Para(k) => {
                                let b = &mut pf.pborders;
                                if k == 4 {
                                    for side in
                                        [&mut b.top, &mut b.bottom, &mut b.left, &mut b.right]
                                    {
                                        upd(side);
                                    }
                                } else {
                                    upd(match k {
                                        0 => &mut b.top,
                                        1 => &mut b.bottom,
                                        2 => &mut b.left,
                                        _ => &mut b.right,
                                    });
                                }
                            }
                            BorderTarget::Cell(k) => {
                                let b = &mut row_def.pending.borders;
                                upd(match k {
                                    0 => &mut b.top,
                                    1 => &mut b.bottom,
                                    2 => &mut b.left,
                                    _ => &mut b.right,
                                });
                            }
                            BorderTarget::None => {}
                        }
                    }
                    "ls" => ls = n,
                    "sylist" => sy_list = n,
                    "ilvl" => ilvl = n.unwrap_or(0).clamp(0, 8) as u8,
                    // legacy Syllabary paragraph markers
                    "bpreset" => pf.bullet_preset = n.unwrap_or(0).clamp(0, 255) as u8,
                    "tblid" => pf.table_id = n.unwrap_or(0).max(0) as u32,
                    "tblrow" => pf.table_row = n.unwrap_or(0).max(0) as u16,
                    "tblcol" => pf.table_col = n.unwrap_or(0).max(0) as u16,
                    "imgid" => legacy_img = Some((n.unwrap_or(0).max(0) as u64, 2.0, 1.5)),
                    "imgw" => {
                        if let Some(l) = legacy_img.as_mut() {
                            l.1 = tw_in(n).clamp(0.05, 40.0);
                        }
                    }
                    "imgh" => {
                        if let Some(l) = legacy_img.as_mut() {
                            l.2 = tw_in(n).clamp(0.05, 40.0);
                        }
                    }
                    // tables
                    "trowd" => {
                        row_def = RowDef {
                            gaph: 108,
                            ..RowDef::default()
                        };
                        border_target = BorderTarget::None;
                    }
                    "trgaph" => row_def.gaph = n.unwrap_or(108),
                    "trleft" => row_def.left = n.unwrap_or(0),
                    "trrh" => row_def.height = n.unwrap_or(0),
                    "clcbpat" => {
                        row_def.pending.shading =
                            color_at(&colors, n).filter(|c| *c != [255, 255, 255])
                    }
                    "clvertalc" => row_def.pending.v_align = CellVAlign::Center,
                    "clvertalb" => row_def.pending.v_align = CellVAlign::Bottom,
                    "clvertalt" => row_def.pending.v_align = CellVAlign::Top,
                    "clNoWrap" => nowrap_seen = true,
                    "clpadl" => row_def.pending.pad_left_in = tw_in(n),
                    "clpadr" => row_def.pending.pad_right_in = tw_in(n),
                    "clpadt" => row_def.pending.pad_top_in = tw_in(n),
                    "clpadb" => row_def.pending.pad_bottom_in = tw_in(n),
                    "cellx" => {
                        let right = n.unwrap_or(0);
                        let prev = row_def.rights.last().copied().unwrap_or(row_def.left);
                        let pending = std::mem::take(&mut row_def.pending);
                        let mut spec = pending.clone();
                        spec.width_in = ((right - prev).max(30)) as f32 / 1440.0;
                        spec.grid_span = 1;
                        spec.wrap_text = !nowrap_seen;
                        nowrap_seen = false;
                        if spec.pad_left_in == 0.0 && spec.pad_right_in == 0.0 {
                            spec.pad_left_in = row_def.gaph.max(0) as f32 / 1440.0;
                            spec.pad_right_in = row_def.gaph.max(0) as f32 / 1440.0;
                        }
                        row_def.cells.push(spec);
                        row_def.rights.push(right);
                        border_target = BorderTarget::None;
                    }
                    "cell" | "nestcell" if gs.dest == Dest::Body => {
                        if cur_table.is_none() {
                            cur_table = Some((next_tbl, Vec::new(), Vec::new()));
                            next_tbl += 1;
                            row_idx = 0;
                        }
                        in_tbl = true;
                        finish_para(
                            &mut doc,
                            &mut para,
                            &pf,
                            &mut marker_text,
                            true,
                            cell_idx,
                            row_idx,
                            &cur_table,
                            gs.chr,
                        );
                        cell_idx += 1;
                    }
                    "row" | "nestrow" if gs.dest == Dest::Body => {
                        if let Some((_, rows, grid)) = cur_table.as_mut() {
                            let mut cells = row_def.cells.clone();
                            while cells.len() < cell_idx as usize {
                                cells.push(TableCellSpec {
                                    width_in: 1.5,
                                    ..TableCellSpec::default()
                                });
                            }
                            cells.truncate((cell_idx as usize).max(1));
                            if grid.len() < cells.len() {
                                *grid = cells.iter().map(|c| c.width_in).collect();
                            }
                            rows.push(TableRowSpec {
                                cells,
                                min_height_pt: (row_def.height.abs() as f32 / 20.0),
                                grid_after: 0,
                                ..TableRowSpec::default()
                            });
                        }
                        cell_idx = 0;
                        row_idx += 1;
                        in_tbl = false;
                    }
                    // character
                    "plain" => {
                        let link = gs.chr.link;
                        gs.chr = plain(&fonts, deff);
                        gs.chr.link = link;
                        gs.hidden = false;
                        gs.font_num = deff;
                    }
                    "f" => {
                        let k = n.unwrap_or(0);
                        gs.font_num = k;
                        if let Some(f) = fonts.get(&k) {
                            if f.charset != 2 {
                                gs.chr.font = font_index(&f.name);
                            }
                        }
                    }
                    "fs" => gs.chr.size = (n.unwrap_or(24) as f32 / 2.0).clamp(1.0, 400.0),
                    "b" => gs.chr.bold = on,
                    "i" => {
                        gs.chr.italic = on;
                        if matches!(gs.dest, Dest::Header | Dest::Footer) {
                            hf_italic = on;
                        }
                    }
                    "ul" | "uld" | "uldash" | "uldashd" | "uldashdd" | "uldb" | "ulth" | "ulw"
                    | "ulwave" | "ulhwave" | "ululdbwave" | "ulthd" | "ulthdash" | "ulldash" => {
                        gs.chr.underline = on
                    }
                    "ulnone" => gs.chr.underline = false,
                    "strike" | "striked" => gs.chr.strike = on,
                    "super" => gs.chr.script = 1,
                    "sub" => gs.chr.script = -1,
                    "nosupersub" => gs.chr.script = 0,
                    "up" => gs.chr.script = if n.unwrap_or(6) > 0 { 1 } else { 0 },
                    "dn" => gs.chr.script = if n.unwrap_or(6) > 0 { -1 } else { 0 },
                    "caps" => gs.chr.caps = if on { 1 } else { 0 },
                    "scaps" => gs.chr.caps = if on { 2 } else { 0 },
                    "expndtw" => gs.chr.spacing_pt = n.unwrap_or(0) as f32 / 20.0,
                    "expnd" => gs.chr.spacing_pt = n.unwrap_or(0) as f32 / 4.0,
                    "v" => gs.hidden = on,
                    "cf" => gs.chr.color = color_at(&colors, n).unwrap_or([0, 0, 0]),
                    "cfto" => gs.chr.color_to = color_at(&colors, n),
                    "highlight" | "chcbpat" | "cb" => {
                        gs.chr.highlight = if n.unwrap_or(0) == 0 {
                            None
                        } else {
                            color_at(&colors, n).filter(|c| *c != [255, 255, 255])
                        }
                    }
                    "hlto" => {
                        gs.chr.highlight_to = if n.unwrap_or(0) == 0 {
                            None
                        } else {
                            color_at(&colors, n)
                        }
                    }
                    // special characters
                    "par" | "line" | "tab" | "page" | "emdash" | "endash" | "bullet" | "lquote"
                    | "rquote" | "ldblquote" | "rdblquote" | "emspace" | "enspace" | "qmspace"
                    | "zwj" | "zwnj" | "u" | "chpgn" => {
                        let ch: Option<char> = match w.as_str() {
                            "tab" => Some('\t'),
                            "emdash" => Some('—'),
                            "endash" => Some('–'),
                            "bullet" => Some('•'),
                            "lquote" => Some('‘'),
                            "rquote" => Some('’'),
                            "ldblquote" => Some('“'),
                            "rdblquote" => Some('”'),
                            "emspace" => Some('\u{2003}'),
                            "enspace" => Some('\u{2002}'),
                            "qmspace" => Some('\u{2005}'),
                            "zwj" => Some('\u{200D}'),
                            "zwnj" => Some('\u{200C}'),
                            "u" => {
                                skip_chars = gs.uc;
                                n.and_then(|v| {
                                    char::from_u32(if v < 0 {
                                        (v + 65536) as u32
                                    } else {
                                        v as u32
                                    })
                                })
                            }
                            "line" => Some('\n'),
                            _ => None,
                        };
                        match gs.dest {
                            Dest::Header | Dest::Footer => match w.as_str() {
                                "chpgn" => field_page = true,
                                "par" | "line" => hf_text.push(' '),
                                _ => {
                                    if let Some(c) = ch {
                                        hf_text.push(if c == '\t' { ' ' } else { c });
                                    }
                                }
                            },
                            Dest::StyleSheet => {
                                if let Some(c) = ch {
                                    ss_name.push(c);
                                }
                            }
                            Dest::Body => match w.as_str() {
                                "par" => {
                                    // a lone \par inside a table cell starts another paragraph in that cell
                                    if !in_tbl && cur_table.is_some() {
                                        if let Some((tid, rows, grid)) = cur_table.take() {
                                            doc.tables.push(DocTable {
                                                id: tid,
                                                grid_cols_in: grid,
                                                rows,
                                                indent_in: row_def.left.max(0) as f32 / 1440.0,
                                            });
                                        }
                                    }
                                    if in_tbl && cur_table.is_none() {
                                        cur_table = Some((next_tbl, Vec::new(), Vec::new()));
                                        next_tbl += 1;
                                        row_idx = 0;
                                    }
                                    apply_list(
                                        &mut pf,
                                        ls,
                                        ilvl,
                                        &overrides,
                                        &lists,
                                        &mut ls_to_def,
                                        &mut doc.list_defs,
                                    );
                                    apply_sy_list(&mut pf, sy_list, ilvl);
                                    if pending_page_break {
                                        para.fmt.page_break_before = true;
                                        pending_page_break = false;
                                    }
                                    let keep_pb = para.fmt.page_break_before;
                                    let mut fmt = pf;
                                    fmt.page_break_before = fmt.page_break_before || keep_pb;
                                    if let Some((id, w_in, h_in)) = legacy_img.take() {
                                        if id > 0 && !doc.images.iter().any(|im| im.id == id) {
                                            doc.images.push(DocImage {
                                                id,
                                                name: format!("Image {id}"),
                                                natural_size: [
                                                    (w_in * 96.0) as u32,
                                                    (h_in * 96.0) as u32,
                                                ],
                                                width_in: w_in,
                                                height_in: h_in,
                                                caption: String::new(),
                                                png_b64: String::new(),
                                                seamless: false,
                                            });
                                        }
                                        para.fmt.image_id = id;
                                    }
                                    finish_para(
                                        &mut doc,
                                        &mut para,
                                        &fmt,
                                        &mut marker_text,
                                        in_tbl,
                                        cell_idx,
                                        row_idx,
                                        &cur_table,
                                        gs.chr,
                                    );
                                }
                                "page" => {
                                    if para.text.is_empty() {
                                        para.fmt.page_break_before = true;
                                    } else {
                                        pending_page_break = true;
                                    }
                                }
                                _ => {
                                    if let Some(c) = ch {
                                        if !gs.hidden {
                                            push_char(&mut para, c, gs.chr);
                                        }
                                    }
                                }
                            },
                            _ => {}
                        }
                    }
                    _ => {}
                }
                // keep a paragraph's list identity once it has text
                if w == "ls" || w == "ilvl" || w == "sylist" {
                    apply_list(
                        &mut pf,
                        ls,
                        ilvl,
                        &overrides,
                        &lists,
                        &mut ls_to_def,
                        &mut doc.list_defs,
                    );
                    apply_sy_list(&mut pf, sy_list, ilvl);
                }
            }
        }
    }
    // last paragraph (RTF often omits the final \par)
    if !para.text.is_empty() || para.fmt.image_id > 0 {
        apply_list(
            &mut pf,
            ls,
            ilvl,
            &overrides,
            &lists,
            &mut ls_to_def,
            &mut doc.list_defs,
        );
        let end = gs.chr;
        finish_para(
            &mut doc,
            &mut para,
            &pf,
            &mut marker_text,
            false,
            0,
            0,
            &None,
            end,
        );
    }
    if let Some((tid, rows, grid)) = cur_table.take() {
        doc.tables.push(DocTable {
            id: tid,
            grid_cols_in: grid,
            rows,
            indent_in: row_def.left.max(0) as f32 / 1440.0,
        });
    }
    // Older Syllabary files stored tables as JSON and list markers inline.
    if let Some(bytes) = legacy_tables.and(base64_decode(legacy_b64.trim())) {
        if let Ok(tbls) = serde_json::from_slice::<Vec<DocTable>>(&bytes) {
            if !tbls.is_empty() {
                doc.tables = tbls;
            }
        }
    }
    for p in doc.paras.iter_mut() {
        legacy_list_marker(p);
        if p.text.len() == 1 && p.text[0] == ' ' && p.fmt.list == ListKind::None {
            p.end_style = p.styles[0];
            p.text.clear();
            p.styles.clear();
        }
    }
    // Table paragraphs must reference existing tables; drop orphans to plain paragraphs.
    let ids: Vec<u32> = doc.tables.iter().map(|t| t.id).collect();
    for p in doc.paras.iter_mut() {
        if p.fmt.table_id > 0 && !ids.contains(&p.fmt.table_id) {
            p.fmt.table_id = 0;
        }
    }
    if doc.paras.is_empty() {
        doc.paras
            .push(Para::new(plain(&fonts, deff), ParaFmt::default()));
    }
    if !(1.0..=40.0).contains(&doc.page.width) || !(1.0..=40.0).contains(&doc.page.height) {
        doc.page.width = 8.5;
        doc.page.height = 11.0;
    }
    for (_, sw, sh) in PAGE_SIZES {
        if (doc.page.width - sw).abs() < 0.02 && (doc.page.height - sh).abs() < 0.02 {
            doc.page.width = *sw;
            doc.page.height = *sh;
        }
    }
    doc
}

/// Lists written by Syllabary keep their editor look (built-in markers, checklists).
fn apply_sy_list(pf: &mut ParaFmt, sy: Option<i32>, ilvl: u8) {
    let Some(k) = sy else { return };
    pf.list = match k {
        2 => ListKind::Number,
        3 | 4 => ListKind::Checklist,
        _ => ListKind::Bullet,
    };
    pf.checked = k == 4;
    pf.list_id = 0;
    pf.indent = ilvl;
    pf.left_in = 0.0;
    pf.first_in = 0.0;
}

fn snap(v: f32) -> f32 {
    let v = v.clamp(0.0, 10.0);
    let s = (v * 100.0).round() / 100.0;
    if (v - s).abs() < 0.0005 {
        s
    } else {
        v
    }
}

fn set_align(dest: Dest, pf: &mut ParaFmt, hf: &mut Align, a: Align) {
    if matches!(dest, Dest::Header | Dest::Footer) {
        *hf = a;
    } else {
        pf.align = a;
    }
}

fn apply_sl(pf: &mut ParaFmt, sl: Option<(i32, i32)>) {
    let Some((v, mult)) = sl else { return };
    pf.line_exact_pt = 0.0;
    pf.line_min_pt = 0.0;
    pf.line_spacing = 1.0;
    if v == 0 || v == 1000 {
        return;
    }
    if mult == 1 {
        pf.line_spacing = (v.abs() as f32 / 240.0).clamp(0.3, 10.0);
    } else if v < 0 {
        pf.line_exact_pt = (-v) as f32 / 20.0;
    } else {
        pf.line_min_pt = v as f32 / 20.0;
    }
}

fn push_char(p: &mut Para, c: char, st: CharStyle) {
    if (c as u32) < 0x20 && c != '\t' && c != '\n' {
        return;
    }
    p.text.push(c);
    p.styles.push(st);
}

fn push_pict_hex(p: &mut PictState, v: u8) {
    p.hex.push(v);
}

fn finish_level(lb: ListLevelBuild, fonts: &HashMap<i32, RtfFont>) -> ListLevel {
    let mut lv = lb.lvl;
    // \leveltext: first code is the length, then text; codes < 10 are level placeholders
    let raw = &lb.text_raw;
    let len = raw.first().copied().unwrap_or(0) as usize;
    let take = if len == 0 { raw.len() } else { len };
    let mut text = String::new();
    for &c in raw.iter().skip(1).take(take) {
        if c < 10 && lv.fmt != NumFmt::Bullet {
            text.push('%');
            text.push(char::from_digit(c + 1, 10).unwrap_or('1'));
        } else if let Some(ch) = char::from_u32(c) {
            text.push(ch);
        }
    }
    let font_name = lb
        .font_num
        .and_then(|f| fonts.get(&f))
        .map(|f| f.name.clone())
        .unwrap_or_default();
    if lv.fmt == NumFmt::Bullet {
        let src = if text.is_empty() {
            "•".to_string()
        } else {
            text
        };
        let (mapped, did) = map_symbol_bullet(&src, &font_name);
        lv.text = if mapped.trim().is_empty() {
            "•".into()
        } else {
            mapped
        };
        lv.font = if did || font_name.is_empty() {
            None
        } else {
            Some(font_index(&font_name))
        };
    } else {
        lv.text = if text.is_empty() { "%1.".into() } else { text };
    }
    lv
}

fn apply_list(
    pf: &mut ParaFmt,
    ls: Option<i32>,
    ilvl: u8,
    overrides: &HashMap<i32, i32>,
    lists: &HashMap<i32, Vec<ListLevel>>,
    ls_to_def: &mut HashMap<i32, u16>,
    defs: &mut Vec<ListDef>,
) {
    let Some(ls) = ls else { return };
    let Some(levels) = overrides.get(&ls).and_then(|lid| lists.get(lid)) else {
        return;
    };
    if levels.is_empty() {
        return;
    }
    let id = *ls_to_def.entry(ls).or_insert_with(|| {
        let id = defs.len() as u16 + 1;
        defs.push(ListDef {
            id,
            levels: levels.clone(),
        });
        id
    });
    let lv = levels
        .get(ilvl as usize)
        .or_else(|| levels.last())
        .cloned()
        .unwrap_or_default();
    pf.list = if lv.fmt == NumFmt::Bullet {
        ListKind::Bullet
    } else {
        ListKind::Number
    };
    pf.list_id = id;
    pf.indent = ilvl;
    if pf.left_in == 0.0 && pf.first_in == 0.0 {
        pf.left_in = lv.left_in;
        pf.first_in = lv.first_in;
    }
}

/// Older Syllabary RTF wrote list markers inline ("•\t", "1.\t", "☐\t").
fn legacy_list_marker(p: &mut Para) {
    if p.fmt.list != ListKind::None || p.fmt.first_in >= 0.0 || p.text.len() < 2 {
        return;
    }
    let tab = match p.text.iter().position(|&c| c == '\t') {
        Some(t) if t <= 5 => t,
        _ => return,
    };
    let head: String = p.text[..tab].iter().collect();
    let kind = match head.as_str() {
        "•" => Some((ListKind::Bullet, false)),
        "☐" => Some((ListKind::Checklist, false)),
        "☑" => Some((ListKind::Checklist, true)),
        h if h.len() >= 2
            && h.ends_with('.')
            && h[..h.len() - 1].chars().all(|c| c.is_ascii_digit()) =>
        {
            Some((ListKind::Number, false))
        }
        _ => None,
    };
    if let Some((k, checked)) = kind {
        p.fmt.list = k;
        p.fmt.checked = checked;
        p.fmt.indent = (((p.fmt.left_in - 0.25).max(0.0) / 0.5).round() as u8).min(8);
        p.fmt.left_in = 0.0;
        p.fmt.first_in = 0.0;
        p.text.drain(0..=tab);
        p.styles.drain(0..=tab);
    }
}

fn finish_pict(doc: &mut Document, pict: &PictState) -> Option<DocImage> {
    if pict.kind == 0 || pict.hex.is_empty() {
        return None;
    }
    let img = image::load_from_memory(&pict.hex).ok()?;
    let nat = [img.width().max(1), img.height().max(1)];
    // \picwgoal / \pichgoal are twips; otherwise show the bitmap at 96 dpi (15 twips per pixel)
    let w_tw = if pict.goalw > 0 {
        pict.goalw
    } else {
        (nat[0] * 15) as i32
    };
    let h_tw = if pict.goalh > 0 {
        pict.goalh
    } else {
        (nat[1] * 15) as i32
    };
    let _ = (pict.picw, pict.pich);
    let w_in = (w_tw as f32 / 1440.0 * pict.scalex.max(1) as f32 / 100.0).clamp(0.05, 40.0);
    let h_in = (h_tw as f32 / 1440.0 * pict.scaley.max(1) as f32 / 100.0).clamp(0.05, 40.0);
    let id = doc.images.iter().map(|i| i.id).max().unwrap_or(0) + 1;
    Some(DocImage {
        id,
        name: format!("Image {id}"),
        natural_size: nat,
        width_in: w_in,
        height_in: h_in,
        caption: String::new(),
        png_b64: base64_encode(&pict.hex),
        seamless: false,
    })
}

/// Places an image as its own block paragraph, splitting off any text already collected.
fn insert_image_para(
    doc: &mut Document,
    para: &mut Para,
    pf: ParaFmt,
    img: DocImage,
    st: CharStyle,
) {
    let id = img.id;
    doc.images.push(img);
    if !para.text.is_empty() || para.fmt.image_id > 0 {
        let done = std::mem::replace(
            para,
            Para::new(
                st,
                ParaFmt {
                    page_break_before: false,
                    ..pf
                },
            ),
        );
        doc.paras.push(Para {
            fmt: ParaFmt {
                image_id: done.fmt.image_id,
                ..pf
            },
            ..done
        });
    }
    para.fmt.image_id = id;
}

fn style_index_for(name: &str) -> usize {
    let want = match name
        .to_ascii_lowercase()
        .replace([' ', '-', '_'], "")
        .as_str()
    {
        "title" => "Title",
        "subtitle" => "Subtitle",
        "heading1" => "Heading 1",
        "heading2" => "Heading 2",
        "heading3" | "heading4" | "heading5" | "heading6" => "Heading 3",
        "quote" | "blockquote" | "intensequote" => "Quote",
        "code" | "htmlpreformatted" => "Code",
        "bodysingle" => "Body Single",
        _ => return 0,
    };
    named_styles()
        .iter()
        .position(|s| s.name == want)
        .unwrap_or(0)
}

// ───────────────────────── writer ─────────────────────────

struct RtfOut {
    fonts: Vec<u16>,
    colors: Vec<[u8; 3]>,
}

impl RtfOut {
    fn font(&mut self, f: u16) -> usize {
        if let Some(i) = self.fonts.iter().position(|&x| x == f) {
            return i;
        }
        self.fonts.push(f);
        self.fonts.len() - 1
    }
    /// 1-based color table index (0 = auto).
    fn color(&mut self, c: [u8; 3]) -> usize {
        if let Some(i) = self.colors.iter().position(|&x| x == c) {
            return i + 1;
        }
        self.colors.push(c);
        self.colors.len()
    }
}

fn rtf_text(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\t' => out.push_str("\\tab "),
            '\n' => out.push_str("\\line "),
            '\u{a0}' => out.push_str("\\~"),
            '\u{2011}' => out.push_str("\\_"),
            c if (c as u32) < 0x20 => {}
            c if (c as u32) < 0x80 => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{}?", *u as i16));
                }
            }
        }
    }
}

fn char_props(o: &mut RtfOut, s: &CharStyle) -> String {
    let mut r = format!("\\f{}\\fs{}", o.font(s.font), (s.size * 2.0).round() as i32);
    if s.bold {
        r.push_str("\\b");
    }
    if s.italic {
        r.push_str("\\i");
    }
    if s.underline {
        r.push_str("\\ul");
    }
    if s.strike {
        r.push_str("\\strike");
    }
    match s.caps {
        1 => r.push_str("\\caps"),
        2 => r.push_str("\\scaps"),
        _ => {}
    }
    if s.spacing_pt != 0.0 {
        r.push_str(&format!(
            "\\expndtw{}",
            (s.spacing_pt * 20.0).round() as i32
        ));
    }
    if s.script > 0 {
        r.push_str("\\super");
    } else if s.script < 0 {
        r.push_str("\\sub");
    }
    if s.color != [0, 0, 0] {
        r.push_str(&format!("\\cf{}", o.color(s.color)));
    }
    if let Some(c2) = s.color_to {
        r.push_str(&format!("\\cfto{}", o.color(c2)));
    }
    if let Some(h) = s.highlight {
        r.push_str(&format!("\\chshdng0\\chcbpat{}", o.color(h)));
    }
    if let Some(h2) = s.highlight_to {
        r.push_str(&format!("\\hlto{}", o.color(h2)));
    }
    r
}

fn border_word(o: &mut RtfOut, tag: &str, b: Option<BorderSpec>) -> String {
    match b {
        Some(b) if b.width_pt > 0.0 => format!(
            "\\{tag}\\brdrs\\brdrw{}\\brdrcf{}",
            (b.width_pt * 20.0).round().max(2.0) as i32,
            o.color(b.color)
        ),
        _ => String::new(),
    }
}

fn level_text_rtf(lv: &ListLevel) -> (String, String) {
    // returns (\leveltext group body, \levelnumbers body)
    let mut chars: Vec<String> = Vec::new();
    let mut numbers = String::new();
    let mut it = lv.text.chars().peekable();
    while let Some(c) = it.next() {
        if c == '%' {
            if let Some(d) = it.peek().and_then(|d| d.to_digit(10)) {
                it.next();
                numbers.push_str(&format!("\\'{:02x}", chars.len() + 1));
                chars.push(format!("\\'{:02x}", d.saturating_sub(1)));
                continue;
            }
        }
        let mut s = String::new();
        rtf_text(&mut s, &c.to_string());
        chars.push(s);
    }
    (
        format!("\\'{:02x}{};", chars.len(), chars.concat()),
        format!("{numbers};"),
    )
}

pub fn to_rtf(doc: &Document) -> String {
    let mut o = RtfOut {
        fonts: vec![DEFAULT_FONT],
        colors: Vec::new(),
    };
    let mut body = String::new();
    let pg = doc.page;
    let tw = |inch: f32| (inch * 1440.0).round() as i32;

    // ---- lists: one \list per imported definition plus the built-in looks ----
    let mut list_table = String::new();
    let mut override_table = String::new();
    let mut ls_of_def: HashMap<u16, usize> = HashMap::new();
    let mut ls_of_builtin: HashMap<(u8, u8), usize> = HashMap::new();
    let mut next_ls = 1usize;
    let mut emit_list = |levels: &[ListLevel],
                         o: &mut RtfOut,
                         list_table: &mut String,
                         override_table: &mut String|
     -> usize {
        let ls = next_ls;
        next_ls += 1;
        let list_id = 1000 + ls;
        list_table.push_str("{\\list\\listtemplateid");
        list_table.push_str(&(2000 + ls).to_string());
        for lv in levels.iter().take(9) {
            let nfc = match lv.fmt {
                NumFmt::Decimal => 0,
                NumFmt::UpperRoman => 1,
                NumFmt::LowerRoman => 2,
                NumFmt::UpperLetter => 3,
                NumFmt::LowerLetter => 4,
                NumFmt::DecimalZero => 22,
                NumFmt::Bullet => 23,
                NumFmt::None => 255,
            };
            let jc = match lv.align {
                Align::Center => 1,
                Align::Right => 2,
                _ => 0,
            };
            let (lt, ln) = level_text_rtf(lv);
            let font = lv
                .font
                .map(|f| format!("\\f{}", o.font(f)))
                .unwrap_or_default();
            list_table.push_str(&format!(
                "{{\\listlevel\\levelnfc{nfc}\\levelnfcn{nfc}\\leveljc{jc}\\leveljcn{jc}\\levelfollow{}\\levelstartat{}{{\\leveltext {lt}}}{{\\levelnumbers {ln}}}{font}\\fi{}\\li{}\\lin{}}}",
                lv.suffix,
                lv.start,
                tw(lv.first_in),
                tw(lv.left_in),
                tw(lv.left_in)
            ));
        }
        list_table.push_str(&format!("{{\\listname ;}}\\listid{list_id}}}"));
        override_table.push_str(&format!(
            "{{\\listoverride\\listid{list_id}\\listoverridecount0\\ls{ls}}}"
        ));
        ls
    };
    for def in &doc.list_defs {
        let ls = emit_list(&def.levels, &mut o, &mut list_table, &mut override_table);
        ls_of_def.insert(def.id, ls);
    }

    // ---- header / footer ----
    let align_word = |a: Align| match a {
        Align::Left => "\\ql",
        Align::Center => "\\qc",
        Align::Right => "\\qr",
        Align::Justify => "\\qj",
    };
    if !doc.header.is_empty() {
        let hs = doc
            .header_style
            .map(|st| {
                format!(
                    "\\syhfstyle{}",
                    char_props(&mut o, &CharStyle { link: 0, ..st })
                )
            })
            .unwrap_or_default();
        body.push_str(&format!(
            "{{\\header\\pard\\plain{}{hs}{} ",
            align_word(doc.header_align),
            if doc.header_italic { "\\i" } else { "" }
        ));
        rtf_text(&mut body, &doc.header);
        body.push_str("\\par}\n");
    }
    if !doc.footer.is_empty() || pg.page_numbers {
        let al = if doc.footer.is_empty() {
            Align::Center
        } else {
            doc.footer_align
        };
        let fs = doc
            .footer_style
            .map(|st| {
                format!(
                    "\\syhfstyle{}",
                    char_props(&mut o, &CharStyle { link: 0, ..st })
                )
            })
            .unwrap_or_default();
        body.push_str(&format!(
            "{{\\footer\\pard\\plain{}{fs}{} ",
            align_word(al),
            if doc.footer_italic { "\\i" } else { "" }
        ));
        rtf_text(&mut body, &doc.footer);
        if pg.page_numbers {
            if !doc.footer.is_empty() {
                body.push_str("\\tab ");
            }
            body.push_str("{\\field{\\*\\fldinst PAGE }{\\fldrslt 1}}");
        }
        body.push_str("\\par}\n");
    }

    // ---- paragraphs ----
    let named = named_styles();
    let mut prev_list: Option<(u8, u8, usize)> = None;
    let mut pi = 0usize;
    while pi < doc.paras.len() {
        let p = &doc.paras[pi];
        if p.fmt.table_id > 0 {
            let tid = p.fmt.table_id;
            let mut end = pi + 1;
            while end < doc.paras.len() && doc.paras[end].fmt.table_id == tid {
                end += 1;
            }
            if let Some(tbl) = doc.table(tid) {
                for (r, row) in tbl.rows.iter().enumerate() {
                    let mut rowd = format!(
                        "\\trowd\\trgaph{}\\trleft{}",
                        tw(row.cells.first().map_or(0.075, |c| c.pad_left_in)),
                        tw(tbl.indent_in)
                    );
                    if row.min_height_pt > 0.0 {
                        rowd.push_str(&format!(
                            "\\trrh{}",
                            (row.min_height_pt * 20.0).round() as i32
                        ));
                    }
                    let mut x = tw(tbl.indent_in);
                    for c in &row.cells {
                        rowd.push_str(&border_word(&mut o, "clbrdrt", c.borders.top));
                        rowd.push_str(&border_word(&mut o, "clbrdrl", c.borders.left));
                        rowd.push_str(&border_word(&mut o, "clbrdrb", c.borders.bottom));
                        rowd.push_str(&border_word(&mut o, "clbrdrr", c.borders.right));
                        if let Some(s) = c.shading {
                            rowd.push_str(&format!("\\clcbpat{}", o.color(s)));
                        }
                        if !c.wrap_text {
                            rowd.push_str("\\clNoWrap");
                        }
                        rowd.push_str(match c.v_align {
                            CellVAlign::Top => "\\clvertalt",
                            CellVAlign::Center => "\\clvertalc",
                            CellVAlign::Bottom => "\\clvertalb",
                        });
                        rowd.push_str(&format!(
                            "\\clpadl{}\\clpadfl3\\clpadr{}\\clpadfr3\\clpadt{}\\clpadft3\\clpadb{}\\clpadfb3",
                            tw(c.pad_left_in),
                            tw(c.pad_right_in),
                            tw(c.pad_top_in),
                            tw(c.pad_bottom_in)
                        ));
                        x += tw(c.width_in);
                        rowd.push_str(&format!("\\cellx{x}"));
                    }
                    body.push_str(&rowd);
                    body.push('\n');
                    for c in 0..row.cells.len() {
                        let cell_paras: Vec<usize> = (pi..end)
                            .filter(|&q| {
                                doc.paras[q].fmt.table_row as usize == r
                                    && doc.paras[q].fmt.table_col as usize == c
                            })
                            .collect();
                        if cell_paras.is_empty() {
                            body.push_str("\\pard\\intbl\\cell\n");
                        }
                        for (k, &q) in cell_paras.iter().enumerate() {
                            write_para(
                                doc,
                                q,
                                &mut o,
                                &mut body,
                                &named,
                                true,
                                &ls_of_def,
                                &mut ls_of_builtin,
                                &mut emit_list,
                                &mut list_table,
                                &mut override_table,
                                &mut prev_list,
                            );
                            body.push_str(if k + 1 == cell_paras.len() {
                                "\\cell\n"
                            } else {
                                "\\par\n"
                            });
                        }
                    }
                    body.push_str(&rowd);
                    body.push_str("\\row\n");
                }
                body.push_str("\\pard\n");
            }
            prev_list = None;
            pi = end;
            continue;
        }
        write_para(
            doc,
            pi,
            &mut o,
            &mut body,
            &named,
            false,
            &ls_of_def,
            &mut ls_of_builtin,
            &mut emit_list,
            &mut list_table,
            &mut override_table,
            &mut prev_list,
        );
        body.push_str("\\par\n");
        pi += 1;
    }

    // ---- stylesheet, info and page setup (these may add fonts / colors) ----
    let mut head = String::from("{\\stylesheet{\\s0\\ql Normal;}");
    for (k, ns) in named.iter().enumerate().skip(1) {
        let mut esc = String::new();
        rtf_text(&mut esc, ns.name);
        head.push_str(&format!(
            "{{\\s{k}\\sbasedon0\\snext0{}\\sb{}\\sa{} {} {esc};}}",
            align_word(ns.fmt.align),
            (ns.fmt.space_before * 20.0).round() as i32,
            (ns.fmt.space_after * 20.0).round() as i32,
            char_props(&mut o, &ns.chr)
        ));
    }
    head.push_str("}\n");
    if !list_table.is_empty() {
        head.push_str(&format!(
            "{{\\*\\listtable{list_table}}}\n{{\\*\\listoverridetable{override_table}}}\n"
        ));
    }
    let info = &doc.info;
    if !(info.title.is_empty()
        && info.author.is_empty()
        && info.subject.is_empty()
        && info.keywords.is_empty()
        && info.notes.is_empty())
    {
        head.push_str("{\\info");
        for (tag, v) in [
            ("title", &info.title),
            ("author", &info.author),
            ("subject", &info.subject),
            ("keywords", &info.keywords),
            ("doccomm", &info.notes),
        ] {
            if !v.is_empty() {
                head.push_str(&format!("{{\\{tag} "));
                rtf_text(&mut head, v);
                head.push('}');
            }
        }
        head.push_str("}\n");
    }
    let pgbg = if pg.background != [255, 255, 255] {
        format!("\\pgbg{}", o.color(pg.background))
    } else {
        String::new()
    };
    head.push_str(&format!(
        "\\paperw{}\\paperh{}\\margl{}\\margr{}\\margt{}\\margb{}\\deftab{}{}{}{}{}\n",
        tw(pg.width),
        tw(pg.height),
        tw(pg.margin_left),
        tw(pg.margin_right),
        tw(pg.margin_top),
        tw(pg.margin_bottom),
        tw(doc.default_tab_in.max(0.05)),
        if pg.width > pg.height {
            "\\landscape"
        } else {
            ""
        },
        pgbg,
        if pg.page_numbers { "\\pgnum1" } else { "" },
        if doc.pageless { "\\pageless1" } else { "" }
    ));
    if doc.header_dist_in > 0.0 {
        head.push_str(&format!("\\headery{}", tw(doc.header_dist_in)));
    }
    if doc.footer_dist_in > 0.0 {
        head.push_str(&format!("\\footery{}", tw(doc.footer_dist_in)));
    }

    let mut out = String::from("{\\rtf1\\ansi\\ansicpg1252\\deff0\\deflang1033\\uc1");
    out.push_str("{\\fonttbl");
    for (i, &f) in o.fonts.iter().enumerate() {
        let name = font_name(f);
        let fam = match font_category(f) {
            FontCat::Serif => "\\froman",
            FontCat::Mono => "\\fmodern",
            FontCat::Script => "\\fscript",
            FontCat::Sans => "\\fswiss",
        };
        let mut esc = String::new();
        rtf_text(&mut esc, name);
        out.push_str(&format!("{{\\f{i}{fam}\\fcharset0 {esc};}}"));
    }
    out.push_str("}\n{\\colortbl;");
    for c in &o.colors {
        out.push_str(&format!("\\red{}\\green{}\\blue{};", c[0], c[1], c[2]));
    }
    out.push_str("}\n{\\*\\generator Syllabary;}\n");
    out.push_str(&head);
    out.push_str("\\sectd\\sbknone\n");
    out.push_str(&body);
    out.push('}');
    out
}

#[allow(clippy::too_many_arguments)]
fn write_para(
    doc: &Document,
    pi: usize,
    o: &mut RtfOut,
    out: &mut String,
    named: &[NamedStyle],
    in_table: bool,
    ls_of_def: &HashMap<u16, usize>,
    ls_of_builtin: &mut HashMap<(u8, u8), usize>,
    emit_list: &mut dyn FnMut(&[ListLevel], &mut RtfOut, &mut String, &mut String) -> usize,
    list_table: &mut String,
    override_table: &mut String,
    prev_list: &mut Option<(u8, u8, usize)>,
) {
    let p = &doc.paras[pi];
    let f = p.fmt;
    out.push_str("\\pard\\plain");
    if in_table {
        out.push_str("\\intbl");
    }
    if f.style > 0 && f.style < named.len() {
        out.push_str(&format!("\\s{}", f.style));
    }
    if f.page_break_before {
        out.push_str("\\pagebb");
    }
    out.push_str(match f.align {
        Align::Left => "\\ql",
        Align::Center => "\\qc",
        Align::Right => "\\qr",
        Align::Justify => "\\qj",
    });
    let (left, first) = doc.indents_in(&f);
    out.push_str(&format!(
        "\\li{0}\\lin{0}\\ri{1}\\rin{1}\\fi{2}",
        (left * 1440.0).round() as i32,
        (f.right_in * 1440.0).round() as i32,
        (first * 1440.0).round() as i32
    ));
    out.push_str(&format!(
        "\\sb{}\\sa{}",
        (f.space_before * 20.0).round() as i32,
        (f.space_after * 20.0).round() as i32
    ));
    if f.line_exact_pt > 0.0 {
        out.push_str(&format!(
            "\\sl-{}\\slmult0",
            (f.line_exact_pt * 20.0).round() as i32
        ));
    } else if f.line_min_pt > 0.0 {
        out.push_str(&format!(
            "\\sl{}\\slmult0",
            (f.line_min_pt * 20.0).round() as i32
        ));
    } else if (f.line_spacing - 1.0).abs() > 0.001 {
        out.push_str(&format!(
            "\\sl{}\\slmult1",
            (f.line_spacing * 240.0).round() as i32
        ));
    }
    for t in f.tabs.as_slice() {
        out.push_str(match t.kind {
            TabKind::Left => "",
            TabKind::Center => "\\tqc",
            TabKind::Right => "\\tqr",
            TabKind::Decimal => "\\tqdec",
        });
        out.push_str(match t.leader {
            1 => "\\tldot",
            2 => "\\tlhyph",
            3 => "\\tlul",
            4 => "\\tlmdot",
            _ => "",
        });
        out.push_str(&format!("\\tx{}", (t.pos_in * 1440.0).round() as i32));
    }
    out.push_str(&border_word(o, "brdrt", f.pborders.top));
    out.push_str(&border_word(o, "brdrl", f.pborders.left));
    if f.hr {
        let c = f.hr_color.unwrap_or([203, 213, 225]);
        out.push_str(&format!(
            "\\brdrb\\brdrs\\brdrw{}\\brdrcf{}",
            ((if f.hr_width_pt > 0.0 {
                f.hr_width_pt
            } else {
                0.75
            }) * 20.0)
                .round() as i32,
            o.color(c)
        ));
    } else {
        out.push_str(&border_word(o, "brdrb", f.pborders.bottom));
    }
    out.push_str(&border_word(o, "brdrr", f.pborders.right));
    if let Some(shd) = f.shading {
        out.push_str(&format!("\\cbpat{}", o.color(shd)));
    }
    if f.bullet_preset > 0 {
        out.push_str(&format!("\\bpreset{}", f.bullet_preset));
    }
    // list
    if f.list != ListKind::None {
        let ls =
            if let (true, Some(&ls)) = (f.list != ListKind::Checklist, ls_of_def.get(&f.list_id)) {
                ls
            } else {
                let kind = match f.list {
                    ListKind::Bullet => 0u8,
                    ListKind::Number => 1,
                    _ => 2,
                };
                let preset = if kind == 2 {
                    f.checked as u8
                } else {
                    f.bullet_preset % 4
                };
                match *prev_list {
                    Some((k, pr, ls)) if k == kind && pr == preset => ls,
                    _ => {
                        // numbers restart for each separate list, like the editor
                        let levels: Vec<ListLevel> = (0..9)
                            .map(|lvl| {
                                let (fmt, text) = builtin_rtf_level(kind, preset, lvl);
                                ListLevel {
                                    fmt,
                                    text,
                                    left_in: lvl as f32 * 0.5 + 0.25,
                                    first_in: -0.25,
                                    ..ListLevel::default()
                                }
                            })
                            .collect();
                        if kind == 1 || !ls_of_builtin.contains_key(&(kind, preset)) {
                            let ls = emit_list(&levels, o, list_table, override_table);
                            ls_of_builtin.insert((kind, preset), ls);
                            ls
                        } else {
                            ls_of_builtin[&(kind, preset)]
                        }
                    }
                }
            };
        *prev_list = Some((
            match f.list {
                ListKind::Bullet => 0,
                ListKind::Number => 1,
                _ => 2,
            },
            if f.list == ListKind::Checklist {
                f.checked as u8
            } else {
                f.bullet_preset % 4
            },
            ls,
        ));
        out.push_str(&format!("\\ls{}\\ilvl{}", ls, f.indent.min(8)));
        if f.list_id == 0 || f.list == ListKind::Checklist {
            let k = match f.list {
                ListKind::Bullet => 1,
                ListKind::Number => 2,
                _ if f.checked => 4,
                _ => 3,
            };
            out.push_str(&format!("\\sylist{k}"));
        }
        let marker = doc.list_marker(pi);
        let ms = p.style_at(0);
        out.push_str(&format!(
            "{{\\listtext\\pard\\plain{} ",
            char_props(
                o,
                &CharStyle {
                    underline: false,
                    strike: false,
                    highlight: None,
                    link: 0,
                    ..ms
                }
            )
        ));
        rtf_text(out, &marker);
        out.push_str("\\tab}");
    } else {
        *prev_list = None;
    }
    // image
    if f.image_id > 0 {
        if let Some(img) = doc.image(f.image_id) {
            let data = base64_decode(&img.png_b64)
                .filter(|b| !b.is_empty() && image::load_from_memory(b).is_ok());
            if data.is_none() {
                out.push_str(&format!(
                    "\\imgid{}\\imgw{}\\imgh{} ",
                    img.id,
                    (img.width_in * 1440.0).round() as i32,
                    (img.height_in * 1440.0).round() as i32
                ));
            }
            if let Some(bytes) = data {
                let blip = if bytes.starts_with(&[0xFF, 0xD8]) {
                    "\\jpegblip"
                } else {
                    "\\pngblip"
                };
                out.push_str(&format!(
                    "{{\\*\\shppict{{\\pict{blip}\\picw{}\\pich{}\\picwgoal{}\\pichgoal{}\n",
                    img.natural_size[0],
                    img.natural_size[1],
                    (img.width_in * 1440.0).round() as i32,
                    (img.height_in * 1440.0).round() as i32
                ));
                for chunk in bytes.chunks(64) {
                    for b in chunk {
                        out.push_str(&format!("{b:02x}"));
                    }
                    out.push('\n');
                }
                out.push_str("}}");
            }
            out.push_str("{\\*\\syimgname ");
            rtf_text(out, &img.name);
            out.push('}');
            if !img.caption.is_empty() {
                out.push_str("{\\*\\sycaption ");
                rtf_text(out, &img.caption);
                out.push('}');
            }
            if img.seamless {
                out.push_str("{\\*\\syseamless}");
            }
        }
    }
    let runs = p.runs();
    if runs.is_empty() {
        out.push_str(&format!("{{{} }}", char_props(o, &p.end_style)));
        out.push_str(&char_props(o, &p.end_style));
        out.push(' ');
    }
    for (a, b, s) in runs {
        let text: String = p.text[a..b].iter().collect();
        let linked = s.link > 0 && (s.link as usize) <= doc.links.len();
        if linked {
            out.push_str("{\\field{\\*\\fldinst HYPERLINK \"");
            rtf_text(out, &doc.links[s.link as usize - 1]);
            out.push_str("\"}{\\fldrslt ");
        }
        out.push('{');
        out.push_str(&char_props(o, &s));
        out.push(' ');
        rtf_text(out, &text);
        out.push('}');
        if linked {
            out.push_str("}}");
        }
    }
    // paragraph-mark formatting (so empty paragraphs keep their height)
    if !p.runs().is_empty() {
        out.push_str(&char_props(o, &p.end_style));
        out.push(' ');
    }
}

fn builtin_rtf_level(kind: u8, preset: u8, lvl: usize) -> (NumFmt, String) {
    match kind {
        0 => {
            let sets: [[&str; 3]; 4] = [
                ["•", "◦", "▪"],
                ["❖", "➢", "▪"],
                ["➔", "➢", "•"],
                ["★", "☆", "•"],
            ];
            (
                NumFmt::Bullet,
                sets[(preset % 4) as usize][lvl % 3].to_string(),
            )
        }
        2 => (
            NumFmt::Bullet,
            if preset == 1 {
                "☑".into()
            } else {
                "☐".into()
            },
        ),
        _ => {
            let n = lvl + 1;
            let (fmt, close) = match (preset % 4, lvl % 3) {
                (0, 0) => (NumFmt::Decimal, "."),
                (0, 1) => (NumFmt::LowerLetter, "."),
                (0, _) => (NumFmt::LowerRoman, "."),
                (1, 0) => (NumFmt::Decimal, ")"),
                (1, 1) => (NumFmt::LowerLetter, ")"),
                (1, _) => (NumFmt::LowerRoman, ")"),
                (2, 0) => (NumFmt::UpperRoman, "."),
                (2, 1) => (NumFmt::UpperLetter, "."),
                (2, _) => (NumFmt::Decimal, "."),
                _ => (NumFmt::UpperLetter, "."),
            };
            (fmt, format!("%{n}{close}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtf_roundtrip_keeps_formatting() {
        let mut d = Document::default();
        d.insert_text(
            Pos::new(0, 0),
            "Hello world",
            CharStyle {
                bold: true,
                size: 14.0,
                color: [200, 0, 0],
                ..Default::default()
            },
        );
        d.paras[0].fmt.align = Align::Center;
        d.paras[0].fmt.left_in = 0.5;
        d.paras[0].fmt.first_in = -0.25;
        d.paras[0].fmt.tabs.set(TabStop {
            pos_in: 3.0,
            kind: TabKind::Right,
            leader: 1,
        });
        let back = from_rtf(&to_rtf(&d));
        assert_eq!(back.plain_text(), "Hello world");
        let p = &back.paras[0];
        assert!(p.styles[0].bold);
        assert_eq!(p.styles[0].size, 14.0);
        assert_eq!(p.styles[0].color, [200, 0, 0]);
        assert_eq!(p.fmt.align, Align::Center);
        assert!((p.fmt.left_in - 0.5).abs() < 0.01);
        assert!((p.fmt.first_in + 0.25).abs() < 0.01);
        assert_eq!(p.fmt.tabs.as_slice()[0].kind, TabKind::Right);
        assert_eq!(p.fmt.tabs.as_slice()[0].leader, 1);
    }

    #[test]
    fn rtf_reads_word_lists_tables_and_line_breaks() {
        let src = r#"{\rtf1\ansi\deff0{\fonttbl{\f0\froman Times New Roman;}{\f1\fnil\fcharset2 Symbol;}}
{\colortbl;\red0\green0\blue0;\red255\green0\blue0;}
{\*\listtable{\list\listtemplateid1{\listlevel\levelnfc23\levelstartat1{\leveltext\'01\'b7;}{\levelnumbers;}\f1\fi-360\li720}{\listname ;}\listid7}}
{\*\listoverridetable{\listoverride\listid7\listoverridecount0\ls1}}
\pard\plain\ls1\ilvl0\fi-360\li720{\listtext\f1\'b7\tab}First item\par
\pard\plain Line one\line line two\par
\trowd\trgaph108\cellx2000\cellx4000\pard\intbl A\cell B\cell\row
\pard\plain {\cf2 red}\par}"#;
        let d = from_rtf(src);
        assert_eq!(d.paras[0].string(), "First item");
        assert_eq!(d.paras[0].fmt.list, ListKind::Bullet);
        assert_eq!(d.list_marker(0), "•");
        assert_eq!(d.paras[1].string(), "Line one\nline two");
        assert_eq!(d.tables.len(), 1);
        assert_eq!(d.paras[2].string(), "A");
        assert_eq!(d.paras[3].fmt.table_col, 1);
        assert_eq!(d.paras.last().unwrap().styles[0].color, [255, 0, 0]);
    }
}

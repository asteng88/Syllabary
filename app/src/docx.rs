//! Word (.docx) files: writer and reader for text, character formatting (incl. caps, letter
//! spacing, theme colors), paragraph indents / tab stops / borders / exact line spacing,
//! multi-level numbering, styles (paragraph, character and table), tables, images, headers,
//! footers, footnotes, page size and margins.
//! The zip container is written and read by hand (deflate via miniz_oxide).

use std::collections::HashMap;

use crate::doc::*;

// ───────────────────────── zip ─────────────────────────

fn zip(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let crc = crc32fast::hash(data);
        let packed = miniz_oxide::deflate::compress_to_vec(data, 6);
        let off = out.len() as u32;
        let nb = name.as_bytes();
        let le16 = |v: u16| v.to_le_bytes();
        let le32 = |v: u32| v.to_le_bytes();
        out.extend_from_slice(&le32(0x0403_4b50));
        out.extend_from_slice(&le16(20));
        out.extend_from_slice(&le16(0));
        out.extend_from_slice(&le16(8));
        out.extend_from_slice(&le16(0));
        out.extend_from_slice(&le16(0x21));
        out.extend_from_slice(&le32(crc));
        out.extend_from_slice(&le32(packed.len() as u32));
        out.extend_from_slice(&le32(data.len() as u32));
        out.extend_from_slice(&le16(nb.len() as u16));
        out.extend_from_slice(&le16(0));
        out.extend_from_slice(nb);
        out.extend_from_slice(&packed);

        central.extend_from_slice(&le32(0x0201_4b50));
        central.extend_from_slice(&le16(20));
        central.extend_from_slice(&le16(20));
        central.extend_from_slice(&le16(0));
        central.extend_from_slice(&le16(8));
        central.extend_from_slice(&le16(0));
        central.extend_from_slice(&le16(0x21));
        central.extend_from_slice(&le32(crc));
        central.extend_from_slice(&le32(packed.len() as u32));
        central.extend_from_slice(&le32(data.len() as u32));
        central.extend_from_slice(&le16(nb.len() as u16));
        central.extend_from_slice(&[0; 12]);
        central.extend_from_slice(&le32(off));
        central.extend_from_slice(nb);
    }
    let cd_off = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&[0; 2]);
    out
}

fn unzip(data: &[u8]) -> Result<HashMap<String, Vec<u8>>, String> {
    let rd16 = |o: usize| -> Option<u16> { data.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])) };
    let rd32 = |o: usize| -> Option<u32> { data.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) };
    let mut eocd = None;
    let lo = data.len().saturating_sub(65_557);
    for i in (lo..data.len().saturating_sub(21)).rev() {
        if rd32(i) == Some(0x0605_4b50) {
            eocd = Some(i);
            break;
        }
    }
    let eocd = eocd.ok_or("this isn't a Word document (no zip directory)")?;
    let count = rd16(eocd + 10).ok_or("bad zip")? as usize;
    let mut p = rd32(eocd + 16).ok_or("bad zip")? as usize;
    let mut files = HashMap::new();
    for _ in 0..count {
        if rd32(p) != Some(0x0201_4b50) {
            break;
        }
        let method = rd16(p + 10).unwrap_or(0);
        let csize = rd32(p + 20).unwrap_or(0) as usize;
        let nlen = rd16(p + 28).unwrap_or(0) as usize;
        let elen = rd16(p + 30).unwrap_or(0) as usize;
        let clen = rd16(p + 32).unwrap_or(0) as usize;
        let loff = rd32(p + 42).unwrap_or(0) as usize;
        let name = String::from_utf8_lossy(data.get(p + 46..p + 46 + nlen).unwrap_or(&[])).to_string();
        p += 46 + nlen + elen + clen;
        if rd32(loff) != Some(0x0403_4b50) {
            continue;
        }
        let ln = rd16(loff + 26).unwrap_or(0) as usize;
        let le = rd16(loff + 28).unwrap_or(0) as usize;
        let start = loff + 30 + ln + le;
        let Some(raw) = data.get(start..start + csize) else { continue };
        let bytes = match method {
            0 => raw.to_vec(),
            8 => match miniz_oxide::inflate::decompress_to_vec(raw) {
                Ok(v) => v,
                Err(_) => continue,
            },
            _ => continue,
        };
        files.insert(name, bytes);
    }
    Ok(files)
}

// ───────────────────────── write ─────────────────────────

// ───────────────────────── write ─────────────────────────

/// Namespace for Syllabary-only extras. It is declared `mc:Ignorable`, so Word opens the file
/// cleanly and simply skips these attributes and elements.
const SY_NS: &str = "http://schemas.syllabary.app/wordml/2026";

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 && c != '\t' => {}
            c => o.push(c),
        }
    }
    o
}

fn hex(c: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

fn style_id(idx: usize) -> Option<&'static str> {
    match named_styles().get(idx).map(|s| s.name) {
        Some("Body Single") => Some("BodySingle"),
        Some("Title") => Some("Title"),
        Some("Subtitle") => Some("Subtitle"),
        Some("Heading 1") => Some("Heading1"),
        Some("Heading 2") => Some("Heading2"),
        Some("Heading 3") => Some("Heading3"),
        Some("Quote") => Some("Quote"),
        Some("Bullet 1") => Some("Bullet1"),
        Some("Number List") => Some("NumberList"),
        Some("Code") => Some("Code"),
        _ => None,
    }
}

fn tw(v: f32) -> i32 {
    (v * 1440.0).round() as i32
}

fn align_val(a: Align) -> &'static str {
    match a {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
        Align::Justify => "both",
    }
}

/// `w:rPr` with children in the order the WordprocessingML schema requires.
fn run_props(s: &CharStyle) -> String {
    let f = esc(font_name(s.font));
    let mut r = format!(
        "<w:rPr><w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:eastAsia=\"{f}\" w:cs=\"{f}\"/>"
    );
    if s.bold {
        r.push_str("<w:b/><w:bCs/>");
    }
    if s.italic {
        r.push_str("<w:i/><w:iCs/>");
    }
    match s.caps {
        1 => r.push_str("<w:caps/>"),
        2 => r.push_str("<w:smallCaps/>"),
        _ => {}
    }
    if s.strike {
        r.push_str("<w:strike/>");
    }
    if let Some(c2) = s.color_to {
        r.push_str(&format!(
            "<w:color w:val=\"{}\" sy:colorTo=\"{}\"/>",
            hex(s.color),
            hex(c2)
        ));
    } else if s.color != [0, 0, 0] {
        r.push_str(&format!("<w:color w:val=\"{}\"/>", hex(s.color)));
    }
    if s.spacing_pt != 0.0 {
        r.push_str(&format!(
            "<w:spacing w:val=\"{}\"/>",
            (s.spacing_pt * 20.0).round() as i32
        ));
    }
    r.push_str(&format!(
        "<w:sz w:val=\"{0}\"/><w:szCs w:val=\"{0}\"/>",
        (s.size * 2.0).round() as i32
    ));
    if s.underline {
        r.push_str("<w:u w:val=\"single\"/>");
    }
    if let Some(h) = s.highlight {
        if let Some(h2) = s.highlight_to {
            r.push_str(&format!(
                "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\" sy:fillTo=\"{}\"/>",
                hex(h),
                hex(h2)
            ));
        } else {
            r.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(h)));
        }
    }
    if s.script > 0 {
        r.push_str("<w:vertAlign w:val=\"superscript\"/>");
    } else if s.script < 0 {
        r.push_str("<w:vertAlign w:val=\"subscript\"/>");
    }
    r.push_str("</w:rPr>");
    r
}

fn border_xml(side: &str, b: Option<BorderSpec>) -> String {
    match b {
        Some(b) if b.width_pt > 0.0 => format!(
            "<w:{side} w:val=\"single\" w:sz=\"{}\" w:space=\"1\" w:color=\"{}\"/>",
            (b.width_pt * 8.0).round().clamp(2.0, 96.0) as i32,
            hex(b.color)
        ),
        _ => String::new(),
    }
}

/// Numbering instances used while writing: built-in list looks and imported list definitions.
struct NumPlan {
    /// abstractNum XML blocks
    abstracts: String,
    /// w:num XML blocks
    nums: String,
    next_num: u32,
    /// (list kind, preset) → abstractNum id for built-in lists
    builtin_abs: HashMap<(u8, u8), u32>,
    /// ListDef id → numId
    def_num: HashMap<u16, u32>,
}

impl NumPlan {
    fn new(doc: &Document) -> Self {
        let mut plan = Self {
            abstracts: String::new(),
            nums: String::new(),
            next_num: 1,
            builtin_abs: HashMap::new(),
            def_num: HashMap::new(),
        };
        let mut next_abs = 0u32;
        // Built-in looks: bullets (4 presets), numbers (4 presets), checklists (unchecked/checked)
        for kind in 0u8..3 {
            let presets: &[u8] = if kind == 2 { &[0, 1] } else { &[0, 1, 2, 3] };
            for &preset in presets {
                let aid = next_abs;
                next_abs += 1;
                plan.builtin_abs.insert((kind, preset), aid);
                plan.abstracts.push_str(&format!(
                    "<w:abstractNum w:abstractNumId=\"{aid}\"><w:multiLevelType w:val=\"hybridMultilevel\"/>"
                ));
                for lvl in 0..9usize {
                    let (fmt, text) = builtin_level(kind, preset, lvl);
                    plan.abstracts.push_str(&format!(
                        "<w:lvl w:ilvl=\"{lvl}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{fmt}\"/><w:lvlText w:val=\"{}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
                        esc(&text),
                        720 * lvl + 360
                    ));
                }
                plan.abstracts.push_str("</w:abstractNum>");
            }
        }
        // Imported multi-level lists keep their exact numbering definitions.
        for def in &doc.list_defs {
            let aid = next_abs;
            next_abs += 1;
            plan.abstracts.push_str(&format!(
                "<w:abstractNum w:abstractNumId=\"{aid}\"><w:multiLevelType w:val=\"multilevel\"/>"
            ));
            for (lvl, lv) in def.levels.iter().enumerate().take(9) {
                let fmt = match lv.fmt {
                    NumFmt::Bullet => "bullet",
                    NumFmt::Decimal => "decimal",
                    NumFmt::DecimalZero => "decimalZero",
                    NumFmt::LowerLetter => "lowerLetter",
                    NumFmt::UpperLetter => "upperLetter",
                    NumFmt::LowerRoman => "lowerRoman",
                    NumFmt::UpperRoman => "upperRoman",
                    NumFmt::None => "none",
                };
                let suff = match lv.suffix {
                    1 => "<w:suff w:val=\"space\"/>",
                    2 => "<w:suff w:val=\"nothing\"/>",
                    _ => "",
                };
                let first = if lv.first_in < 0.0 {
                    format!("w:hanging=\"{}\"", tw(-lv.first_in))
                } else {
                    format!("w:firstLine=\"{}\"", tw(lv.first_in))
                };
                let mut rpr = String::new();
                if let Some(f) = lv.font {
                    let n = esc(font_name(f));
                    rpr.push_str(&format!(
                        "<w:rFonts w:ascii=\"{n}\" w:hAnsi=\"{n}\" w:hint=\"default\"/>"
                    ));
                }
                if let Some(b) = lv.bold {
                    rpr.push_str(if b { "<w:b/>" } else { "<w:b w:val=\"0\"/>" });
                }
                if let Some(c) = lv.color {
                    rpr.push_str(&format!("<w:color w:val=\"{}\"/>", hex(c)));
                }
                if let Some(sz) = lv.size {
                    rpr.push_str(&format!("<w:sz w:val=\"{}\"/>", (sz * 2.0).round() as i32));
                }
                let rpr = if rpr.is_empty() {
                    String::new()
                } else {
                    format!("<w:rPr>{rpr}</w:rPr>")
                };
                plan.abstracts.push_str(&format!(
                    "<w:lvl w:ilvl=\"{lvl}\"><w:start w:val=\"{}\"/><w:numFmt w:val=\"{fmt}\"/>{suff}<w:lvlText w:val=\"{}\"/><w:lvlJc w:val=\"{}\"/><w:pPr><w:ind w:left=\"{}\" {first}/></w:pPr>{rpr}</w:lvl>",
                    lv.start,
                    esc(&lv.text),
                    align_val(lv.align),
                    tw(lv.left_in)
                ));
            }
            plan.abstracts.push_str("</w:abstractNum>");
            let nid = plan.next_num;
            plan.next_num += 1;
            plan.nums.push_str(&format!(
                "<w:num w:numId=\"{nid}\"><w:abstractNumId w:val=\"{aid}\"/></w:num>"
            ));
            plan.def_num.insert(def.id, nid);
        }
        plan
    }

    /// A fresh numbering instance (numbers restart at 1, like separate lists in the editor).
    fn new_builtin_num(&mut self, kind: u8, preset: u8) -> u32 {
        let preset = if kind == 2 { preset.min(1) } else { preset % 4 };
        let aid = self.builtin_abs.get(&(kind, preset)).copied().unwrap_or(0);
        let nid = self.next_num;
        self.next_num += 1;
        let mut ov = String::new();
        if kind == 1 {
            for l in 0..9 {
                ov.push_str(&format!(
                    "<w:lvlOverride w:ilvl=\"{l}\"><w:startOverride w:val=\"1\"/></w:lvlOverride>"
                ));
            }
        }
        self.nums.push_str(&format!(
            "<w:num w:numId=\"{nid}\"><w:abstractNumId w:val=\"{aid}\"/>{ov}</w:num>"
        ));
        nid
    }
}

/// Number format and label text of a built-in list level, matching `Document::list_marker`.
fn builtin_level(kind: u8, preset: u8, lvl: usize) -> (&'static str, String) {
    match kind {
        0 => {
            let sets: [[&str; 3]; 4] = [
                ["•", "◦", "▪"],
                ["❖", "➢", "▪"],
                ["➔", "➢", "•"],
                ["★", "☆", "•"],
            ];
            ("bullet", sets[(preset % 4) as usize][lvl % 3].to_string())
        }
        2 => (
            "bullet",
            if preset == 1 {
                "☑".into()
            } else {
                "☐".into()
            },
        ),
        _ => {
            let n = lvl + 1;
            let (fmt, close) = match (preset % 4, lvl % 3) {
                (0, 0) => ("decimal", "."),
                (0, 1) => ("lowerLetter", "."),
                (0, _) => ("lowerRoman", "."),
                (1, 0) => ("decimal", ")"),
                (1, 1) => ("lowerLetter", ")"),
                (1, _) => ("lowerRoman", ")"),
                (2, 0) => ("upperRoman", "."),
                (2, 1) => ("upperLetter", "."),
                (2, _) => ("decimal", "."),
                _ => ("upperLetter", "."),
            };
            (fmt, format!("%{n}{close}"))
        }
    }
}

struct WriteCtx {
    plan: NumPlan,
    /// numId of the built-in list run currently open (kind, preset, numId)
    open_list: Option<(u8, u8, u32)>,
    next_drawing_id: u32,
}

fn write_para_xml(
    doc: &Document,
    p: &Para,
    prev: Option<&Para>,
    ctx: &mut WriteCtx,
    body: &mut String,
) {
    let f = p.fmt;
    body.push_str("<w:p><w:pPr>");
    if let Some(id) = style_id(f.style) {
        body.push_str(&format!("<w:pStyle w:val=\"{id}\"/>"));
    }
    if f.page_break_before {
        body.push_str("<w:pageBreakBefore/>");
    }
    // numbering
    let mut list_ext = String::new();
    if f.list != ListKind::None {
        let lvl = f.indent.min(8);
        let num_id = if let (true, Some(&nid)) = (
            f.list_id > 0 && f.list != ListKind::Checklist,
            ctx.plan.def_num.get(&f.list_id),
        ) {
            nid
        } else {
            let kind = match f.list {
                ListKind::Bullet => 0u8,
                ListKind::Number => 1,
                _ => 2,
            };
            let preset = if kind == 2 {
                f.checked as u8
            } else {
                f.bullet_preset
            };
            let continues =
                prev.is_some_and(|q| q.fmt.list != ListKind::None && q.fmt.table_id == f.table_id);
            match ctx.open_list {
                Some((k, pr, nid)) if continues && k == kind && (pr == preset || kind == 2) => nid,
                _ => {
                    let nid = ctx.plan.new_builtin_num(kind, preset);
                    ctx.open_list = Some((kind, preset, nid));
                    nid
                }
            }
        };
        body.push_str(&format!(
            "<w:numPr><w:ilvl w:val=\"{lvl}\"/><w:numId w:val=\"{num_id}\"/></w:numPr>"
        ));
        if f.list_id == 0 || f.list == ListKind::Checklist {
            let kind = match f.list {
                ListKind::Bullet => "bullet",
                ListKind::Number => "number",
                _ if f.checked => "checked",
                _ => "check",
            };
            list_ext = format!(
                "<sy:list sy:kind=\"{kind}\" sy:preset=\"{}\"/>",
                f.bullet_preset
            );
        }
    } else {
        ctx.open_list = None;
    }
    // borders (an editor horizontal rule is a bottom border tagged for round-trip)
    let mut bdr = String::new();
    bdr.push_str(&border_xml("top", f.pborders.top));
    bdr.push_str(&border_xml("left", f.pborders.left));
    if f.hr {
        let w = if f.hr_width_pt > 0.0 {
            f.hr_width_pt
        } else {
            0.75
        };
        bdr.push_str(&format!(
            "<w:bottom w:val=\"single\" w:sz=\"{}\" w:space=\"1\" w:color=\"{}\"/>",
            (w * 8.0).round().max(2.0) as i32,
            hex(f.hr_color.unwrap_or([203, 213, 225]))
        ));
    } else {
        bdr.push_str(&border_xml("bottom", f.pborders.bottom));
    }
    bdr.push_str(&border_xml("right", f.pborders.right));
    if !bdr.is_empty() {
        body.push_str(&format!("<w:pBdr>{bdr}</w:pBdr>"));
    }
    if let Some(shd) = f.shading {
        body.push_str(&format!(
            "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>",
            hex(shd)
        ));
    }
    if !f.tabs.is_empty() {
        body.push_str("<w:tabs>");
        for t in f.tabs.as_slice() {
            let val = match t.kind {
                TabKind::Left => "left",
                TabKind::Center => "center",
                TabKind::Right => "right",
                TabKind::Decimal => "decimal",
            };
            let leader = match t.leader {
                1 => " w:leader=\"dot\"",
                2 => " w:leader=\"hyphen\"",
                3 => " w:leader=\"underscore\"",
                4 => " w:leader=\"middleDot\"",
                _ => "",
            };
            body.push_str(&format!(
                "<w:tab w:val=\"{val}\"{leader} w:pos=\"{}\"/>",
                tw(t.pos_in)
            ));
        }
        body.push_str("</w:tabs>");
    }
    let line = if f.line_exact_pt > 0.0 {
        format!(
            "w:line=\"{}\" w:lineRule=\"exact\"",
            (f.line_exact_pt * 20.0).round() as i32
        )
    } else if f.line_min_pt > 0.0 {
        format!(
            "w:line=\"{}\" w:lineRule=\"atLeast\"",
            (f.line_min_pt * 20.0).round() as i32
        )
    } else {
        format!(
            "w:line=\"{}\" w:lineRule=\"auto\"",
            (f.line_spacing * 240.0).round() as i32
        )
    };
    body.push_str(&format!(
        "<w:spacing w:before=\"{}\" w:after=\"{}\" {line}/>",
        (f.space_before * 20.0).round() as i32,
        (f.space_after * 20.0).round() as i32,
    ));
    // indents: lists without explicit indents take theirs from the numbering level
    let explicit = f.left_in != 0.0 || f.first_in != 0.0 || f.right_in != 0.0;
    if f.list == ListKind::None || explicit {
        let (left, first) = if f.list == ListKind::None || f.list_id == 0 {
            doc.indents_in(&f)
        } else {
            (f.left_in, f.first_in)
        };
        let first_attr = if first < 0.0 {
            format!(" w:hanging=\"{}\"", tw(-first))
        } else if first > 0.0 {
            format!(" w:firstLine=\"{}\"", tw(first))
        } else {
            String::new()
        };
        if left != 0.0 || f.right_in != 0.0 || !first_attr.is_empty() {
            body.push_str(&format!(
                "<w:ind w:left=\"{}\" w:right=\"{}\"{first_attr}/>",
                tw(left),
                tw(f.right_in)
            ));
        }
    }
    body.push_str(&format!(
        "<w:jc w:val=\"{}\"/>{}",
        align_val(f.align),
        run_props(&p.end_style)
    ));
    body.push_str(&list_ext);
    if f.hr {
        body.push_str("<sy:hr/>");
    }
    body.push_str("</w:pPr>");
    if f.image_id > 0 {
        if let Some(img) = doc.image(f.image_id) {
            let cx = (img.width_in * 914_400.0).round() as i64;
            let cy = (img.height_in * 914_400.0).round() as i64;
            let did = ctx.next_drawing_id;
            ctx.next_drawing_id += 1;
            body.push_str(&format!(
                "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:docPr id=\"{did}\" name=\"{}\" descr=\"{}\" sy:imageId=\"{}\" sy:naturalW=\"{}\" sy:naturalH=\"{}\"{}/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{did}\" name=\"{}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"rIdImg{}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>",
                esc(&img.name),
                esc(&img.caption),
                img.id,
                img.natural_size[0],
                img.natural_size[1],
                if img.seamless { " sy:seamless=\"1\"" } else { "" },
                esc(&img.name),
                img.id
            ));
        }
    }
    for (a, b, s) in p.runs() {
        let linked = s.link > 0 && (s.link as usize) <= doc.links.len();
        if linked {
            body.push_str(&format!(
                "<w:hyperlink r:id=\"rIdLink{}\" w:history=\"1\">",
                s.link
            ));
        }
        body.push_str("<w:r>");
        body.push_str(&run_props(&s));
        let mut buf = String::new();
        let flush = |buf: &mut String, body: &mut String| {
            if !buf.is_empty() {
                body.push_str(&format!("<w:t xml:space=\"preserve\">{}</w:t>", esc(buf)));
                buf.clear();
            }
        };
        for &c in &p.text[a..b] {
            match c {
                '\t' => {
                    flush(&mut buf, body);
                    body.push_str("<w:tab/>");
                }
                '\n' | '\u{2028}' | '\u{0B}' => {
                    flush(&mut buf, body);
                    body.push_str("<w:br/>");
                }
                c => buf.push(c),
            }
        }
        flush(&mut buf, body);
        body.push_str("</w:r>");
        if linked {
            body.push_str("</w:hyperlink>");
        }
    }
    body.push_str("</w:p>");
}

/// Header / footer part with an optional PAGE field.
fn hf_part(
    tag: &str,
    text: &str,
    align: Align,
    italic: bool,
    page_field: bool,
    style: Option<CharStyle>,
) -> String {
    let rpr = match style {
        Some(s) => run_props(&CharStyle {
            italic: s.italic || italic,
            link: 0,
            highlight: None,
            highlight_to: None,
            color_to: None,
            ..s
        }),
        None if italic => "<w:rPr><w:i/></w:rPr>".to_string(),
        None => String::new(),
    };
    let rpr = rpr.as_str();
    let mut runs = String::new();
    if !text.is_empty() {
        runs.push_str(&format!(
            "<w:r>{rpr}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
            esc(text)
        ));
    }
    if page_field {
        if !text.is_empty() {
            runs.push_str(&format!("<w:r>{rpr}<w:tab/></w:r>"));
        }
        runs.push_str(&format!(
            "<w:r>{rpr}<w:fldChar w:fldCharType=\"begin\"/></w:r><w:r>{rpr}<w:instrText xml:space=\"preserve\"> PAGE </w:instrText></w:r><w:r>{rpr}<w:fldChar w:fldCharType=\"separate\"/></w:r><w:r>{rpr}<w:t>1</w:t></w:r><w:r>{rpr}<w:fldChar w:fldCharType=\"end\"/></w:r>"
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:{tag} xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:p><w:pPr><w:jc w:val=\"{}\"/></w:pPr>{runs}</w:p></w:{tag}>",
        align_val(align)
    )
}

/// Image container format from its magic bytes: (extension, content type).
fn image_kind(bytes: &[u8]) -> (&'static str, &'static str) {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        ("jpeg", "image/jpeg")
    } else if bytes.starts_with(b"GIF8") {
        ("gif", "image/gif")
    } else if bytes.starts_with(b"BM") {
        ("bmp", "image/bmp")
    } else {
        ("png", "image/png")
    }
}

pub fn write(doc: &Document) -> Vec<u8> {
    let mut body = String::new();
    let mut extra_rels = String::new();
    for (li, url) in doc.links.iter().enumerate() {
        extra_rels.push_str(&format!(
            "<Relationship Id=\"rIdLink{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
            li + 1,
            esc(url)
        ));
    }
    let mut media_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut media_types: Vec<(&'static str, &'static str)> = Vec::new();
    for img in &doc.images {
        if let Some(bytes) = base64_decode(&img.png_b64) {
            let (ext, ct) = image_kind(&bytes);
            if !media_types.iter().any(|(e, _)| *e == ext) {
                media_types.push((ext, ct));
            }
            extra_rels.push_str(&format!(
                "<Relationship Id=\"rIdImg{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image{}.{ext}\"/>",
                img.id, img.id
            ));
            media_files.push((format!("word/media/image{}.{ext}", img.id), bytes));
        }
    }

    let mut ctx = WriteCtx {
        plan: NumPlan::new(doc),
        open_list: None,
        next_drawing_id: 1,
    };
    let mut pi = 0;
    while pi < doc.paras.len() {
        let p = &doc.paras[pi];
        let prev = pi.checked_sub(1).map(|q| &doc.paras[q]);
        if p.fmt.table_id > 0 {
            let tid = p.fmt.table_id;
            let mut tbl_end = pi + 1;
            while tbl_end < doc.paras.len() && doc.paras[tbl_end].fmt.table_id == tid {
                tbl_end += 1;
            }
            ctx.open_list = None;
            if let Some(tbl) = doc.table(tid) {
                body.push_str("<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/>");
                if tbl.indent_in.abs() > 0.001 {
                    body.push_str(&format!("<w:tblInd w:w=\"{}\" w:type=\"dxa\"/>", tw(tbl.indent_in)));
                }
                body.push_str("<w:tblLayout w:type=\"fixed\"/><w:tblLook w:val=\"0000\" w:firstRow=\"0\" w:lastRow=\"0\" w:firstColumn=\"0\" w:lastColumn=\"0\" w:noHBand=\"1\" w:noVBand=\"1\"/></w:tblPr><w:tblGrid>");
                for &cw in &tbl.grid_cols_in {
                    body.push_str(&format!("<w:gridCol w:w=\"{}\"/>", tw(cw)));
                }
                body.push_str("</w:tblGrid>");
                for (r_idx, row_spec) in tbl.rows.iter().enumerate() {
                    body.push_str("<w:tr>");
                    if row_spec.grid_after > 0 || row_spec.min_height_pt > 0.0 {
                        body.push_str("<w:trPr>");
                        if row_spec.grid_after > 0 {
                            body.push_str(&format!(
                                "<w:gridAfter w:val=\"{}\"/>",
                                row_spec.grid_after
                            ));
                        }
                        if row_spec.min_height_pt > 0.0 {
                            body.push_str(&format!(
                                "<w:trHeight w:val=\"{}\"/>",
                                (row_spec.min_height_pt * 20.0).round() as i32
                            ));
                        }
                        body.push_str("</w:trPr>");
                    }
                    for (c_idx, cell_spec) in row_spec.cells.iter().enumerate() {
                        body.push_str("<w:tc><w:tcPr>");
                        body.push_str(&format!("<w:tcW w:w=\"{}\" w:type=\"dxa\"/>", tw(cell_spec.width_in)));
                        if cell_spec.grid_span > 1 {
                            body.push_str(&format!("<w:gridSpan w:val=\"{}\"/>", cell_spec.grid_span));
                        }
                        body.push_str("<w:tcBorders>");
                        for (side, b_opt) in [
                            ("top", cell_spec.borders.top),
                            ("left", cell_spec.borders.left),
                            ("bottom", cell_spec.borders.bottom),
                            ("right", cell_spec.borders.right),
                        ] {
                            match b_opt {
                                Some(b) if b.width_pt > 0.0 => body.push_str(&format!(
                                    "<w:{side} w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>",
                                    (b.width_pt * 8.0).round().max(1.0) as i32,
                                    hex(b.color)
                                )),
                                _ => body.push_str(&format!("<w:{side} w:val=\"nil\"/>")),
                            }
                        }
                        body.push_str("</w:tcBorders>");
                        if let Some(shd) = cell_spec.shading {
                            body.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(shd)));
                        }
                        if !cell_spec.wrap_text {
                            body.push_str("<w:noWrap/>");
                        }
                        body.push_str(&format!(
                            "<w:tcMar><w:top w:w=\"{}\" w:type=\"dxa\"/><w:left w:w=\"{}\" w:type=\"dxa\"/><w:bottom w:w=\"{}\" w:type=\"dxa\"/><w:right w:w=\"{}\" w:type=\"dxa\"/></w:tcMar>",
                            tw(cell_spec.pad_top_in),
                            tw(cell_spec.pad_left_in),
                            tw(cell_spec.pad_bottom_in),
                            tw(cell_spec.pad_right_in)
                        ));
                        let va = match cell_spec.v_align {
                            CellVAlign::Top => "top",
                            CellVAlign::Center => "center",
                            CellVAlign::Bottom => "bottom",
                        };
                        body.push_str(&format!("<w:vAlign w:val=\"{va}\"/></w:tcPr>"));
                        let mut wrote_any = false;
                        let mut prev_cell: Option<&Para> = None;
                        for cp in &doc.paras[pi..tbl_end] {
                            if cp.fmt.table_row as usize == r_idx
                                && cp.fmt.table_col as usize == c_idx
                            {
                                write_para_xml(doc, cp, prev_cell, &mut ctx, &mut body);
                                prev_cell = Some(cp);
                                wrote_any = true;
                            }
                        }
                        if !wrote_any {
                            body.push_str("<w:p/>");
                        }
                        body.push_str("</w:tc>");
                    }
                    body.push_str("</w:tr>");
                }
                body.push_str("</w:tbl>");
            } else {
                for (k, cp) in doc.paras[pi..tbl_end].iter().enumerate() {
                    let pv = if k == 0 {
                        None
                    } else {
                        Some(&doc.paras[pi + k - 1])
                    };
                    write_para_xml(doc, cp, pv, &mut ctx, &mut body);
                }
            }
            ctx.open_list = None;
            pi = tbl_end;
            continue;
        }
        write_para_xml(
            doc,
            p,
            prev.filter(|q| q.fmt.table_id == 0),
            &mut ctx,
            &mut body,
        );
        pi += 1;
    }

    // header / footer parts
    let pg = doc.page;
    let mut hf_rels = String::new();
    let mut hf_refs = String::new();
    let mut hf_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut hf_types = String::new();
    if !doc.header.is_empty() {
        hf_files.push((
            "word/header1.xml".into(),
            hf_part(
                "hdr",
                &doc.header,
                doc.header_align,
                doc.header_italic,
                false,
                doc.header_style,
            )
            .into_bytes(),
        ));
        hf_rels.push_str("<Relationship Id=\"rIdHdr1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" Target=\"header1.xml\"/>");
        hf_refs.push_str("<w:headerReference w:type=\"default\" r:id=\"rIdHdr1\"/>");
        hf_types.push_str("<Override PartName=\"/word/header1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>");
    }
    if !doc.footer.is_empty() || pg.page_numbers {
        let align = if doc.footer.is_empty() {
            Align::Center
        } else {
            doc.footer_align
        };
        hf_files.push((
            "word/footer1.xml".into(),
            hf_part(
                "ftr",
                &doc.footer,
                align,
                doc.footer_italic,
                pg.page_numbers,
                doc.footer_style,
            )
            .into_bytes(),
        ));
        hf_rels.push_str("<Relationship Id=\"rIdFtr1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer\" Target=\"footer1.xml\"/>");
        hf_refs.push_str("<w:footerReference w:type=\"default\" r:id=\"rIdFtr1\"/>");
        hf_types.push_str("<Override PartName=\"/word/footer1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\"/>");
    }

    let orient = if pg.width > pg.height {
        " w:orient=\"landscape\""
    } else {
        ""
    };
    let hf_ext = format!(
        " sy:hdrStyled=\"{}\" sy:ftrStyled=\"{}\" sy:headerDist=\"{}\" sy:footerDist=\"{}\"",
        doc.header_style.is_some() as u8,
        doc.footer_style.is_some() as u8,
        doc.header_dist_in,
        doc.footer_dist_in
    );
    let hdr_tw = if doc.header_dist_in > 0.0 {
        tw(doc.header_dist_in)
    } else {
        720
    };
    let ftr_tw = if doc.footer_dist_in > 0.0 {
        tw(doc.footer_dist_in)
    } else {
        720
    };
    body.push_str(&format!(
        "<w:sectPr sy:app=\"1\" sy:pgNum=\"{}\" sy:pageless=\"{}\" sy:headerText=\"{}\" sy:footerText=\"{}\" sy:headerAlign=\"{}\" sy:footerAlign=\"{}\" sy:headerItalic=\"{}\" sy:footerItalic=\"{}\"{hf_ext}>{hf_refs}<w:pgSz w:w=\"{}\" w:h=\"{}\"{orient}/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"{hdr_tw}\" w:footer=\"{ftr_tw}\" w:gutter=\"0\"/><w:cols w:space=\"720\"/></w:sectPr>",
        pg.page_numbers as u8,
        doc.pageless as u8,
        esc(&doc.header),
        esc(&doc.footer),
        align_val(doc.header_align),
        align_val(doc.footer_align),
        doc.header_italic as u8,
        doc.footer_italic as u8,
        tw(pg.width),
        tw(pg.height),
        tw(pg.margin_top),
        tw(pg.margin_right),
        tw(pg.margin_bottom),
        tw(pg.margin_left)
    ));
    let bg_xml = if pg.background != [255, 255, 255] {
        format!("<w:background w:color=\"{}\"/>", hex(pg.background))
    } else {
        String::new()
    };
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" xmlns:sy=\"{SY_NS}\" mc:Ignorable=\"sy\">{bg_xml}<w:body>{body}</w:body></w:document>"
    );

    let mut styles = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" xmlns:sy=\"");
    styles.push_str(SY_NS);
    styles.push_str("\" mc:Ignorable=\"sy\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:eastAsia=\"Calibri\" w:cs=\"Calibri\"/><w:sz w:val=\"24\"/><w:szCs w:val=\"24\"/><w:lang w:val=\"en-US\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style><w:style w:type=\"character\" w:default=\"1\" w:styleId=\"DefaultParagraphFont\"><w:name w:val=\"Default Paragraph Font\"/><w:uiPriority w:val=\"1\"/><w:semiHidden/><w:unhideWhenUsed/></w:style><w:style w:type=\"table\" w:default=\"1\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/><w:tblPr><w:tblInd w:w=\"0\" w:type=\"dxa\"/><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>");
    for (i, ns) in named_styles().iter().enumerate() {
        if let Some(id) = style_id(i) {
            let outline = match id {
                "Heading1" => "<w:outlineLvl w:val=\"0\"/>",
                "Heading2" => "<w:outlineLvl w:val=\"1\"/>",
                "Heading3" => "<w:outlineLvl w:val=\"2\"/>",
                _ => "",
            };
            let keep = if id.starts_with("Heading") || id == "Title" || id == "Subtitle" {
                "<w:keepNext/>"
            } else {
                ""
            };
            let name = if id.starts_with("Heading") {
                ns.name.to_lowercase()
            } else {
                ns.name.to_string()
            };
            styles.push_str(&format!(
                "<w:style w:type=\"paragraph\" w:styleId=\"{id}\"><w:name w:val=\"{name}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr>{keep}<w:spacing w:before=\"{}\" w:after=\"{}\"/><w:jc w:val=\"{}\"/>{outline}</w:pPr>{}</w:style>",
                (ns.fmt.space_before * 20.0).round() as i32,
                (ns.fmt.space_after * 20.0).round() as i32,
                align_val(ns.fmt.align),
                run_props(&ns.chr)
            ));
        }
    }
    styles.push_str("</w:styles>");

    let numbering = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">{}{}</w:numbering>",
        ctx.plan.abstracts, ctx.plan.nums
    );
    let settings = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">{}<w:defaultTabStop w:val=\"{}\"/><w:characterSpacingControl w:val=\"doNotCompress\"/><w:compat><w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/></w:compat></w:settings>",
        if pg.background != [255, 255, 255] { "<w:displayBackgroundShape/>" } else { "" },
        tw(doc.default_tab_in.max(0.05))
    );

    let core = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title><dc:creator>{}</dc:creator><dc:subject>{}</dc:subject><cp:keywords>{}</cp:keywords><dc:description>{}</dc:description></cp:coreProperties>",
        esc(&doc.info.title),
        esc(&doc.info.author),
        esc(&doc.info.subject),
        esc(&doc.info.keywords),
        esc(&doc.info.notes)
    );
    let mut media_defaults = String::new();
    for (ext, ct) in &media_types {
        media_defaults.push_str(&format!(
            "<Default Extension=\"{ext}\" ContentType=\"{ct}\"/>"
        ));
    }
    let types = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>{media_defaults}<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/><Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>{hf_types}<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>"
    );
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>";
    let doc_rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/><Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/>{hf_rels}{extra_rels}</Relationships>"
    );
    let mut entries: Vec<(&str, Vec<u8>)> = vec![
        ("[Content_Types].xml", types.into_bytes()),
        ("_rels/.rels", rels.as_bytes().to_vec()),
        ("word/document.xml", document.into_bytes()),
        ("word/_rels/document.xml.rels", doc_rels.into_bytes()),
        ("word/styles.xml", styles.into_bytes()),
        ("word/numbering.xml", numbering.into_bytes()),
        ("word/settings.xml", settings.into_bytes()),
        ("docProps/core.xml", core.into_bytes()),
    ];
    for (fname, bytes) in &hf_files {
        entries.push((fname.as_str(), bytes.clone()));
    }
    for (fname, bytes) in &media_files {
        entries.push((fname.as_str(), bytes.clone()));
    }
    zip(&entries)
}

// ───────────────────────── read ─────────────────────────

enum Tok {
    Open(String, HashMap<String, String>, bool),
    Close(String),
    Text(String),
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut o = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        o.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(j) = rest.find(';') else { break };
        let ent = &rest[1..j];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => o.push(c),
            None => o.push_str(&rest[..=j]),
        }
        rest = &rest[j + 1..];
    }
    o.push_str(rest);
    o
}

fn tokens(xml: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let b = xml.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'<' {
            let Some(end) = xml[i..].find('>') else { break };
            let tag = &xml[i + 1..i + end];
            i += end + 1;
            if tag.starts_with('?') || tag.starts_with('!') {
                continue;
            }
            if let Some(name) = tag.strip_prefix('/') {
                out.push(Tok::Close(name.trim().to_string()));
                continue;
            }
            let selfclose = tag.ends_with('/');
            let tag = tag.trim_end_matches('/');
            let mut parts = tag.splitn(2, char::is_whitespace);
            let name = parts.next().unwrap_or("").to_string();
            let mut attrs = HashMap::new();
            let mut rest = parts.next().unwrap_or("");
            while let Some(eq) = rest.find('=') {
                let key = rest[..eq].trim().to_string();
                let after = rest[eq + 1..].trim_start();
                let q = after.chars().next().unwrap_or('"');
                let Some(close) = after[1..].find(q) else { break };
                attrs.insert(key, unescape(&after[1..1 + close]));
                rest = &after[close + 2..];
            }
            out.push(Tok::Open(name, attrs, selfclose));
        } else {
            let end = xml[i..].find('<').map_or(b.len(), |e| i + e);
            out.push(Tok::Text(unescape(&xml[i..end])));
            i = end;
        }
    }
    out
}

type Attrs = HashMap<String, String>;

fn on(a: &Attrs) -> bool {
    !matches!(
        a.get("w:val").map(|s| s.as_str()),
        Some("0" | "false" | "none" | "off")
    )
}

/// Attribute in either the WordprocessingML namespace or the app's own (older files used `w:`).
fn ext_attr<'a>(a: &'a Attrs, key: &str) -> Option<&'a String> {
    a.get(&format!("sy:{key}"))
        .or_else(|| a.get(&format!("w:{key}")))
}

fn num(a: &Attrs, k: &str) -> Option<f32> {
    a.get(k).and_then(|v| v.trim().parse::<f32>().ok())
}

/// Word's exact highlighter colors.
fn highlight_color(name: &str) -> Option<[u8; 3]> {
    Some(match name {
        "yellow" => [255, 255, 0],
        "green" | "brightGreen" => [0, 255, 0],
        "cyan" | "turquoise" => [0, 255, 255],
        "magenta" | "pink" => [255, 0, 255],
        "blue" => [0, 0, 255],
        "red" => [255, 0, 0],
        "darkBlue" => [0, 0, 128],
        "darkCyan" | "teal" => [0, 128, 128],
        "darkGreen" => [0, 128, 0],
        "darkMagenta" | "violet" => [128, 0, 128],
        "darkRed" => [128, 0, 0],
        "darkYellow" => [128, 128, 0],
        "darkGray" => [128, 128, 128],
        "lightGray" => [192, 192, 192],
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        _ => return None,
    })
}

fn parse_hex(v: &str) -> Option<[u8; 3]> {
    if v.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(v, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

/// Theme data from word/theme/theme1.xml.
#[derive(Default)]
struct Theme {
    major: Option<u16>,
    minor: Option<u16>,
    colors: HashMap<String, [u8; 3]>,
}

impl Theme {
    fn parse(xml: &str) -> Self {
        let mut th = Theme::default();
        let mut in_major = false;
        let mut in_minor = false;
        let mut in_clr = false;
        let mut cur_clr = String::new();
        for t in tokens(xml) {
            match t {
                Tok::Open(n, _, sc) if n == "a:majorFont" => in_major = !sc,
                Tok::Close(n) if n == "a:majorFont" => in_major = false,
                Tok::Open(n, _, sc) if n == "a:minorFont" => in_minor = !sc,
                Tok::Close(n) if n == "a:minorFont" => in_minor = false,
                Tok::Open(n, _, sc) if n == "a:clrScheme" => in_clr = !sc,
                Tok::Close(n) if n == "a:clrScheme" => in_clr = false,
                Tok::Open(n, a, _) if n == "a:latin" && (in_major || in_minor) => {
                    if let Some(tf) = a.get("typeface").filter(|s| !s.is_empty()) {
                        let fi = font_index(tf);
                        if in_major && th.major.is_none() {
                            th.major = Some(fi);
                        } else if in_minor && th.minor.is_none() {
                            th.minor = Some(fi);
                        }
                    }
                }
                Tok::Open(n, a, _) if in_clr => {
                    if let Some(name) = n.strip_prefix("a:") {
                        match name {
                            "srgbClr" => {
                                if let Some(c) = a.get("val").and_then(|v| parse_hex(v)) {
                                    th.colors.insert(cur_clr.clone(), c);
                                }
                            }
                            "sysClr" => {
                                if let Some(c) = a.get("lastClr").and_then(|v| parse_hex(v)) {
                                    th.colors.insert(cur_clr.clone(), c);
                                }
                            }
                            _ => cur_clr = name.to_string(),
                        }
                    }
                }
                _ => {}
            }
        }
        th
    }

    fn color(&self, a: &Attrs) -> Option<[u8; 3]> {
        let name = a.get("w:themeColor")?;
        let key = match name.as_str() {
            "text1" | "dark1" => "dk1",
            "background1" | "light1" => "lt1",
            "text2" | "dark2" => "dk2",
            "background2" | "light2" => "lt2",
            "hyperlink" => "hlink",
            "followedHyperlink" => "folHlink",
            other => other,
        };
        let mut c = *self.colors.get(key)?;
        if let Some(sh) = a
            .get("w:themeShade")
            .and_then(|v| u8::from_str_radix(v, 16).ok())
        {
            let f = sh as f32 / 255.0;
            c = c.map(|v| (v as f32 * f).round() as u8);
        }
        if let Some(ti) = a
            .get("w:themeTint")
            .and_then(|v| u8::from_str_radix(v, 16).ok())
        {
            let f = ti as f32 / 255.0;
            c = c.map(|v| (v as f32 * f + 255.0 * (1.0 - f)).round() as u8);
        }
        Some(c)
    }
}

/// Applies one run-property element. `hidden` is set by `w:vanish`.
fn apply_rpr(s: &mut CharStyle, hidden: &mut bool, name: &str, a: &Attrs, th: &Theme) {
    match name {
        "w:b" => s.bold = on(a),
        "w:i" => s.italic = on(a),
        "w:u" => s.underline = on(a),
        "w:strike" | "w:dstrike" => s.strike = on(a),
        "w:caps" => s.caps = if on(a) { 1 } else { 0 },
        "w:smallCaps" => {
            if on(a) {
                s.caps = 2;
            } else if s.caps == 2 {
                s.caps = 0;
            }
        }
        "w:vanish" => *hidden = on(a),
        "w:spacing" => {
            if let Some(v) = num(a, "w:val") {
                s.spacing_pt = (v / 20.0).clamp(-10.0, 50.0);
            }
        }
        "w:sz" => {
            if let Some(v) = num(a, "w:val") {
                s.size = (v / 2.0).clamp(1.0, 400.0);
            }
        }
        "w:rFonts" => {
            let theme_font = a
                .get("w:asciiTheme")
                .or_else(|| a.get("w:hAnsiTheme"))
                .and_then(|t| {
                    let tl = t.to_ascii_lowercase();
                    if tl.starts_with("major") {
                        Some(th.major.unwrap_or(3))
                    } else if tl.starts_with("minor") {
                        Some(th.minor.unwrap_or(2))
                    } else {
                        None
                    }
                });
            if let Some(f) = theme_font {
                s.font = f;
            } else if let Some(f) = a
                .get("w:ascii")
                .or_else(|| a.get("w:hAnsi"))
                .filter(|f| !f.is_empty())
            {
                s.font = font_index(f);
            }
        }
        "w:color" => {
            if let Some(c) = a
                .get("w:val")
                .and_then(|v| parse_hex(v))
                .or_else(|| th.color(a))
            {
                s.color = c;
            } else if a.get("w:val").is_some_and(|v| v == "auto") {
                s.color = [0, 0, 0];
            }
            if let Some(c2) = ext_attr(a, "colorTo").and_then(|v| parse_hex(v)) {
                s.color_to = Some(c2);
            }
        }
        "w:highlight" => s.highlight = a.get("w:val").and_then(|v| highlight_color(v)),
        "w:shd" => {
            if let Some(c) = a.get("w:fill").and_then(|v| parse_hex(v)) {
                if c != [255, 255, 255] {
                    s.highlight = Some(c);
                }
            }
            if let Some(c2) = ext_attr(a, "fillTo").and_then(|v| parse_hex(v)) {
                s.highlight_to = Some(c2);
            }
        }
        "w:vertAlign" => {
            s.script = match a.get("w:val").map(|s| s.as_str()) {
                Some("superscript") => 1,
                Some("subscript") => -1,
                _ => 0,
            }
        }
        _ => {}
    }
}

fn parse_border(a: &Attrs) -> Option<BorderSpec> {
    let val = a.get("w:val").map_or("single", |v| v.as_str());
    if matches!(val, "none" | "nil" | "0" | "false") {
        return None;
    }
    let sz = num(a, "w:sz").unwrap_or(4.0);
    if sz <= 0.0 {
        return None;
    }
    let mut width_pt = (sz / 8.0).clamp(0.25, 12.0);
    if matches!(val, "double" | "thinThickSmallGap" | "thickThinSmallGap") {
        width_pt *= 2.5;
    }
    let color = a
        .get("w:color")
        .and_then(|v| parse_hex(v))
        .unwrap_or([0, 0, 0]);
    Some(BorderSpec { width_pt, color })
}

/// Paragraph-level extras that live outside `ParaFmt` while reading.
#[derive(Clone, Default)]
struct PExtra {
    contextual: Option<bool>,
    num_id: Option<String>,
    ilvl: Option<u8>,
}

fn tab_kind(v: &str) -> Option<TabKind> {
    Some(match v {
        "left" | "start" | "num" => TabKind::Left,
        "center" => TabKind::Center,
        "right" | "end" => TabKind::Right,
        "decimal" => TabKind::Decimal,
        _ => return None,
    })
}

/// Applies one paragraph-property element (direct, style or default). Border children
/// (`w:top`, `w:bottom`, ...) only count inside `w:pBdr`, signalled by `in_pbdr`.
fn apply_ppr(fmt: &mut ParaFmt, ex: &mut PExtra, n: &str, a: &Attrs, in_pbdr: bool) {
    match n {
        "w:jc" => {
            fmt.align = match a.get("w:val").map(|s| s.as_str()) {
                Some("center") => Align::Center,
                Some("right") | Some("end") => Align::Right,
                Some("both")
                | Some("distribute")
                | Some("lowKashida")
                | Some("mediumKashida")
                | Some("highKashida") => Align::Justify,
                _ => Align::Left,
            };
        }
        "w:spacing" => {
            if a.get("w:beforeAutospacing")
                .is_some_and(|v| v == "1" || v == "true" || v == "on")
            {
                fmt.space_before = 14.0;
            } else if let Some(v) = num(a, "w:before") {
                fmt.space_before = (v / 20.0).clamp(0.0, 400.0);
            }
            if a.get("w:afterAutospacing")
                .is_some_and(|v| v == "1" || v == "true" || v == "on")
            {
                fmt.space_after = 14.0;
            } else if let Some(v) = num(a, "w:after") {
                fmt.space_after = (v / 20.0).clamp(0.0, 400.0);
            }
            if let Some(v) = num(a, "w:line") {
                match a.get("w:lineRule").map_or("auto", |s| s.as_str()) {
                    "exact" => {
                        fmt.line_exact_pt = (v.abs() / 20.0).max(1.0);
                        fmt.line_min_pt = 0.0;
                        fmt.line_spacing = 1.0;
                    }
                    "atLeast" => {
                        fmt.line_min_pt = (v / 20.0).max(0.0);
                        fmt.line_exact_pt = 0.0;
                        fmt.line_spacing = 1.0;
                    }
                    _ => {
                        fmt.line_spacing = (v / 240.0).clamp(0.3, 10.0);
                        fmt.line_exact_pt = 0.0;
                        fmt.line_min_pt = 0.0;
                    }
                }
            }
        }
        "w:ind" => {
            let left = num(a, "w:left").or_else(|| num(a, "w:start"));
            let right = num(a, "w:right").or_else(|| num(a, "w:end"));
            if let Some(v) = left {
                fmt.indent = 0;
                fmt.left_in = v / 1440.0;
            }
            if let Some(v) = right {
                fmt.right_in = v / 1440.0;
            }
            if let Some(v) = num(a, "w:hanging") {
                fmt.first_in = -v / 1440.0;
            } else if let Some(v) = num(a, "w:firstLine") {
                fmt.first_in = v / 1440.0;
            }
        }
        "w:tab" => {
            let Some(pos) = num(a, "w:pos") else { return };
            let val = a.get("w:val").map_or("left", |s| s.as_str());
            if val == "clear" {
                fmt.tabs.remove_at(pos / 1440.0);
            } else if let Some(kind) = tab_kind(val) {
                let leader = match a.get("w:leader").map(|s| s.as_str()) {
                    Some("dot") => 1,
                    Some("hyphen") => 2,
                    Some("underscore") | Some("heavy") => 3,
                    Some("middleDot") => 4,
                    _ => 0,
                };
                fmt.tabs.set(TabStop {
                    pos_in: pos / 1440.0,
                    kind,
                    leader,
                });
            }
        }
        "w:contextualSpacing" => ex.contextual = Some(on(a)),
        "w:numId" => ex.num_id = a.get("w:val").cloned(),
        "w:ilvl" => ex.ilvl = a.get("w:val").and_then(|v| v.parse().ok()),
        "w:shd" => {
            fmt.shading = a
                .get("w:fill")
                .and_then(|v| parse_hex(v))
                .filter(|c| *c != [255, 255, 255]);
        }
        "w:top" if in_pbdr => fmt.pborders.top = parse_border(a),
        "w:bottom" if in_pbdr => fmt.pborders.bottom = parse_border(a),
        "w:left" | "w:start" if in_pbdr => fmt.pborders.left = parse_border(a),
        "w:right" | "w:end" if in_pbdr => fmt.pborders.right = parse_border(a),
        "w:pageBreakBefore" => fmt.page_break_before = on(a),
        _ => {}
    }
}

/// Parsed `word/numbering.xml`: list levels per numbering instance.
#[derive(Default)]
struct Numbering {
    abstracts: HashMap<String, Vec<ListLevel>>,
    /// numId → (abstractNumId, per-level overrides (start, full level))
    nums: HashMap<String, (String, Vec<(usize, Option<u32>, Option<ListLevel>)>)>,
}

fn num_fmt(v: &str) -> NumFmt {
    match v {
        "bullet" => NumFmt::Bullet,
        "decimalZero" => NumFmt::DecimalZero,
        "lowerLetter" => NumFmt::LowerLetter,
        "upperLetter" => NumFmt::UpperLetter,
        "lowerRoman" => NumFmt::LowerRoman,
        "upperRoman" => NumFmt::UpperRoman,
        "none" => NumFmt::None,
        _ => NumFmt::Decimal,
    }
}

impl Numbering {
    fn parse(xml: &str, th: &Theme) -> Self {
        let mut nb = Numbering::default();
        let mut cur_abs: Option<String> = None;
        let mut cur_num: Option<String> = None;
        let mut ov_lvl: Option<usize> = None;
        let mut lvl: Option<(usize, ListLevel, String)> = None; // (index, level, marker font name)
        let mut in_lvl_ppr = false;
        let mut in_lvl_rpr = false;
        let finish_level = |lvl: (usize, ListLevel, String)| -> (usize, ListLevel) {
            let (i, mut lv, font) = lvl;
            if lv.fmt == NumFmt::Bullet {
                let (txt, mapped) = map_symbol_bullet(&lv.text, &font);
                lv.text = if txt.trim().is_empty() {
                    "•".into()
                } else {
                    txt
                };
                if mapped {
                    lv.font = None;
                }
            } else {
                let (txt, mapped) = map_symbol_bullet(&lv.text, &font);
                if mapped {
                    lv.text = txt;
                    lv.font = None;
                }
            }
            (i, lv)
        };
        for t in tokens(xml) {
            match t {
                Tok::Open(n, a, sc) => match n.as_str() {
                    "w:abstractNum" if !sc => {
                        let id = a.get("w:abstractNumId").cloned().unwrap_or_default();
                        nb.abstracts.insert(id.clone(), Vec::new());
                        cur_abs = Some(id);
                    }
                    "w:num" if !sc => cur_num = a.get("w:numId").cloned(),
                    "w:abstractNumId" if cur_num.is_some() => {
                        if let (Some(nid), Some(aid)) = (cur_num.clone(), a.get("w:val")) {
                            nb.nums.insert(nid, (aid.clone(), Vec::new()));
                        }
                    }
                    "w:lvlOverride" if cur_num.is_some() && !sc => {
                        ov_lvl = a.get("w:ilvl").and_then(|v| v.parse().ok())
                    }
                    "w:startOverride" => {
                        if let (Some(nid), Some(l)) = (cur_num.as_ref(), ov_lvl) {
                            let start = a.get("w:val").and_then(|v| v.parse().ok());
                            if let Some((_, ovs)) = nb.nums.get_mut(nid) {
                                ovs.push((l, start, None));
                            }
                        }
                    }
                    "w:lvl" if !sc => {
                        let i = a
                            .get("w:ilvl")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0usize);
                        lvl = Some((
                            i,
                            ListLevel {
                                left_in: 0.0,
                                first_in: 0.0,
                                text: String::new(),
                                ..ListLevel::default()
                            },
                            String::new(),
                        ));
                    }
                    "w:pPr" if lvl.is_some() => in_lvl_ppr = !sc,
                    "w:rPr" if lvl.is_some() => in_lvl_rpr = !sc,
                    _ => {
                        let Some((_, lv, font)) = lvl.as_mut() else {
                            continue;
                        };
                        if in_lvl_ppr {
                            if n == "w:ind" {
                                if let Some(v) = num(&a, "w:left").or_else(|| num(&a, "w:start")) {
                                    lv.left_in = v / 1440.0;
                                }
                                if let Some(v) = num(&a, "w:hanging") {
                                    lv.first_in = -v / 1440.0;
                                } else if let Some(v) = num(&a, "w:firstLine") {
                                    lv.first_in = v / 1440.0;
                                }
                            }
                        } else if in_lvl_rpr {
                            match n.as_str() {
                                "w:rFonts" => {
                                    if let Some(f) = a.get("w:ascii").or_else(|| a.get("w:hAnsi")) {
                                        *font = f.clone();
                                        lv.font = Some(font_index(f));
                                    }
                                }
                                "w:color" => {
                                    lv.color = a
                                        .get("w:val")
                                        .and_then(|v| parse_hex(v))
                                        .or_else(|| th.color(&a))
                                }
                                "w:sz" => lv.size = num(&a, "w:val").map(|v| v / 2.0),
                                "w:b" => lv.bold = Some(on(&a)),
                                _ => {}
                            }
                        } else {
                            match n.as_str() {
                                "w:start" => {
                                    lv.start =
                                        a.get("w:val").and_then(|v| v.parse().ok()).unwrap_or(1)
                                }
                                "w:numFmt" => {
                                    lv.fmt =
                                        num_fmt(a.get("w:val").map_or("decimal", |s| s.as_str()))
                                }
                                "w:lvlText" => {
                                    lv.text = a.get("w:val").cloned().unwrap_or_default()
                                }
                                "w:lvlJc" => {
                                    lv.align = match a.get("w:val").map(|s| s.as_str()) {
                                        Some("center") => Align::Center,
                                        Some("right") | Some("end") => Align::Right,
                                        _ => Align::Left,
                                    }
                                }
                                "w:suff" => {
                                    lv.suffix = match a.get("w:val").map(|s| s.as_str()) {
                                        Some("space") => 1,
                                        Some("nothing") => 2,
                                        _ => 0,
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                },
                Tok::Close(n) => match n.as_str() {
                    "w:pPr" => in_lvl_ppr = false,
                    "w:rPr" => in_lvl_rpr = false,
                    "w:lvl" => {
                        if let Some(l) = lvl.take() {
                            let (i, lv) = finish_level(l);
                            if let Some(nid) = cur_num.as_ref() {
                                if let Some((_, ovs)) = nb.nums.get_mut(nid) {
                                    ovs.push((i, None, Some(lv)));
                                }
                            } else if let Some(levels) =
                                cur_abs.as_ref().and_then(|aid| nb.abstracts.get_mut(aid))
                            {
                                while levels.len() <= i.min(8) {
                                    levels.push(ListLevel::default());
                                }
                                levels[i.min(8)] = lv;
                            }
                        }
                        in_lvl_ppr = false;
                        in_lvl_rpr = false;
                    }
                    "w:abstractNum" => cur_abs = None,
                    "w:num" => cur_num = None,
                    "w:lvlOverride" => ov_lvl = None,
                    _ => {}
                },
                Tok::Text(_) => {}
            }
        }
        nb
    }

    /// The list levels of numbering instance `num_id` with overrides applied.
    fn levels(&self, num_id: &str) -> Option<Vec<ListLevel>> {
        let (aid, ovs) = self.nums.get(num_id)?;
        let mut levels = self.abstracts.get(aid).cloned().unwrap_or_default();
        for (l, start, lvl) in ovs {
            let l = (*l).min(8);
            while levels.len() <= l {
                levels.push(ListLevel::default());
            }
            if let Some(lv) = lvl {
                levels[l] = lv.clone();
            }
            if let Some(s) = start {
                levels[l].start = *s;
            }
        }
        if levels.is_empty() {
            None
        } else {
            Some(levels)
        }
    }
}

/// Paragraph, run and table formatting from styles.xml.
#[derive(Default)]
struct Styles {
    default_pfmt: ParaFmt,
    default_ex: PExtra,
    base: CharStyle,
    para: HashMap<String, (ParaFmt, PExtra, CharStyle)>,
    chars: HashMap<String, Vec<(String, Attrs)>>,
    tables: HashMap<String, TableStyle>,
    default_table: Option<String>,
}

#[derive(Clone, Default)]
struct TableStyle {
    pfmt_ops: Vec<(String, Attrs, bool)>,
    rpr_ops: Vec<(String, Attrs)>,
    borders: Vec<(String, Attrs)>,
    cell_mar: Vec<(String, Attrs)>,
}

impl Styles {
    fn parse(xml: &str, th: &Theme) -> Self {
        struct RawStyle {
            id: String,
            stype: String,
            is_default: bool,
            based_on: Option<String>,
            ppr: Vec<(String, Attrs, bool)>,
            rpr: Vec<(String, Attrs)>,
            borders: Vec<(String, Attrs)>,
            cell_mar: Vec<(String, Attrs)>,
        }
        let mut st = Styles {
            base: CharStyle {
                font: th.minor.unwrap_or(0),
                size: 10.0,
                ..CharStyle::default()
            },
            ..Default::default()
        };
        let mut def_ppr: Vec<(String, Attrs, bool)> = Vec::new();
        let mut def_rpr: Vec<(String, Attrs)> = Vec::new();
        let mut in_rpr_def = false;
        let mut in_ppr_def = false;
        let mut cur: Option<RawStyle> = None;
        let mut in_ppr = false;
        let mut in_rpr = false;
        let mut in_pbdr = false;
        let mut in_tbl_pr = false;
        let mut in_tbl_borders = false;
        let mut in_cell_mar = false;
        let mut in_cond = 0usize; // inside w:tblStylePr (conditional formats are skipped)
        let mut raw: Vec<RawStyle> = Vec::new();
        for t in tokens(xml) {
            match t {
                Tok::Open(n, a, sc) => match n.as_str() {
                    "w:rPrDefault" => in_rpr_def = !sc,
                    "w:pPrDefault" => in_ppr_def = !sc,
                    "w:style" if !sc => {
                        cur = Some(RawStyle {
                            id: a.get("w:styleId").cloned().unwrap_or_default(),
                            stype: a
                                .get("w:type")
                                .cloned()
                                .unwrap_or_else(|| "paragraph".into()),
                            is_default: matches!(
                                a.get("w:default").map(|s| s.as_str()),
                                Some("1" | "true")
                            ),
                            based_on: None,
                            ppr: Vec::new(),
                            rpr: Vec::new(),
                            borders: Vec::new(),
                            cell_mar: Vec::new(),
                        });
                    }
                    "w:tblStylePr" if !sc => in_cond += 1,
                    n if is_skipped_container(n) && !sc => in_cond += 1,
                    _ if in_cond > 0 => {}
                    "w:basedOn" if cur.is_some() && !in_ppr && !in_rpr => {
                        if let (Some(rs), Some(b)) = (cur.as_mut(), a.get("w:val")) {
                            rs.based_on = Some(b.clone());
                        }
                    }
                    "w:pPr" => in_ppr = !sc,
                    "w:rPr" => in_rpr = !sc,
                    "w:pBdr" if in_ppr => in_pbdr = !sc,
                    "w:tblPr" if cur.is_some() => in_tbl_pr = !sc,
                    "w:tblBorders" if in_tbl_pr => in_tbl_borders = !sc,
                    "w:tblCellMar" if in_tbl_pr => in_cell_mar = !sc,
                    _ => {
                        if in_rpr_def && in_rpr {
                            def_rpr.push((n, a));
                        } else if in_ppr_def && in_ppr {
                            def_ppr.push((n, a, in_pbdr));
                        } else if let Some(rs) = cur.as_mut() {
                            if in_tbl_borders {
                                rs.borders.push((n, a));
                            } else if in_cell_mar {
                                rs.cell_mar.push((n, a));
                            } else if in_rpr {
                                rs.rpr.push((n, a));
                            } else if in_ppr {
                                rs.ppr.push((n, a, in_pbdr));
                            }
                        }
                    }
                },
                Tok::Close(n) => match n.as_str() {
                    "w:rPrDefault" => in_rpr_def = false,
                    "w:pPrDefault" => in_ppr_def = false,
                    n if n == "w:tblStylePr" || is_skipped_container(n) => {
                        in_cond = in_cond.saturating_sub(1)
                    }
                    "w:pPr" => {
                        in_ppr = false;
                        in_pbdr = false;
                    }
                    "w:rPr" => in_rpr = false,
                    "w:pBdr" => in_pbdr = false,
                    "w:tblPr" => in_tbl_pr = false,
                    "w:tblBorders" => in_tbl_borders = false,
                    "w:tblCellMar" => in_cell_mar = false,
                    "w:style" => {
                        if let Some(rs) = cur.take() {
                            raw.push(rs);
                        }
                        in_ppr = false;
                        in_rpr = false;
                        in_tbl_pr = false;
                        in_cond = 0;
                    }
                    _ => {}
                },
                Tok::Text(_) => {}
            }
        }
        let mut hidden = false;
        for (n, a) in &def_rpr {
            apply_rpr(&mut st.base, &mut hidden, n, a, th);
        }
        for (n, a, b) in &def_ppr {
            apply_ppr(&mut st.default_pfmt, &mut st.default_ex, n, a, *b);
        }
        let by_id: HashMap<&str, &RawStyle> = raw.iter().map(|s| (s.id.as_str(), s)).collect();
        fn chain_of<'a>(
            rs: &'a RawStyle,
            by_id: &HashMap<&str, &'a RawStyle>,
        ) -> Vec<&'a RawStyle> {
            let mut chain: Vec<&RawStyle> = Vec::new();
            let mut c = Some(rs);
            while let Some(s) = c {
                if chain.len() >= 12 || chain.iter().any(|x| x.id == s.id) {
                    break;
                }
                chain.push(s);
                c = s.based_on.as_deref().and_then(|b| by_id.get(b).copied());
            }
            chain.reverse();
            chain
        }
        // The default paragraph style (Normal) applies to every paragraph without a style.
        if let Some(norm) = raw.iter().find(|s| {
            s.stype == "paragraph" && (s.is_default || s.id.eq_ignore_ascii_case("Normal"))
        }) {
            for anc in chain_of(norm, &by_id) {
                for (n, a, b) in &anc.ppr {
                    apply_ppr(&mut st.default_pfmt, &mut st.default_ex, n, a, *b);
                }
                for (n, a) in &anc.rpr {
                    apply_rpr(&mut st.base, &mut hidden, n, a, th);
                }
            }
        }
        let normal_id = raw
            .iter()
            .find(|s| {
                s.stype == "paragraph" && (s.is_default || s.id.eq_ignore_ascii_case("Normal"))
            })
            .map(|s| s.id.clone());
        for rs in &raw {
            let chain = chain_of(rs, &by_id);
            match rs.stype.as_str() {
                "paragraph" => {
                    let mut f = st.default_pfmt;
                    let mut ex = st.default_ex.clone();
                    let mut c = st.base;
                    let idx = style_index(&rs.id);
                    for anc in &chain {
                        if Some(&anc.id) == normal_id.as_ref() {
                            continue; // already folded into the defaults
                        }
                        for (n, a, b) in &anc.ppr {
                            apply_ppr(&mut f, &mut ex, n, a, *b);
                        }
                        for (n, a) in &anc.rpr {
                            apply_rpr(&mut c, &mut hidden, n, a, th);
                        }
                    }
                    if idx != 0 {
                        f.style = idx;
                    }
                    st.para.insert(rs.id.clone(), (f, ex, c));
                }
                "character" => {
                    st.chars.insert(
                        rs.id.clone(),
                        chain.iter().flat_map(|s| s.rpr.iter().cloned()).collect(),
                    );
                }
                "table" => {
                    let mut ts = TableStyle::default();
                    for anc in &chain {
                        ts.pfmt_ops.extend(anc.ppr.iter().cloned());
                        ts.rpr_ops.extend(anc.rpr.iter().cloned());
                        ts.borders.extend(anc.borders.iter().cloned());
                        ts.cell_mar.extend(anc.cell_mar.iter().cloned());
                    }
                    if rs.is_default {
                        st.default_table = Some(rs.id.clone());
                    }
                    st.tables.insert(rs.id.clone(), ts);
                }
                _ => {}
            }
        }
        st
    }
}

/// Header / footer part → (text, alignment, italic, bare PAGE field, style of the first text run).
fn parse_hf_part(
    xml_bytes: &[u8],
    styles: &Styles,
    theme: &Theme,
    footer: bool,
) -> (String, Align, bool, bool, Option<CharStyle>) {
    let xml = String::from_utf8_lossy(xml_bytes);
    // Word's built-in "Header" / "Footer" paragraph styles apply when a paragraph names none
    let default_style = styles
        .para
        .get(if footer { "Footer" } else { "Header" })
        .map_or(styles.base, |s| s.2);
    let mut para_base = default_style;
    let mut run = default_style;
    let mut first_style: Option<CharStyle> = None;
    let mut in_ppr = false;
    let mut in_rpr = false;
    let mut in_run = false;
    let mut hidden = false;
    let mut paras: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut align = Align::Center;
    let mut align_set = false;
    let mut italic = false;
    let mut in_t = false;
    let mut page_field = false;
    let mut page_kept = false;
    let mut field_depth = 0usize;
    let mut in_field_result = false;
    let mut field_is_page = false;
    let mut skip = 0usize; // mc:Fallback
    for t in tokens(&xml) {
        match t {
            Tok::Open(n, a, sc) => match n.as_str() {
                "mc:Fallback" if !sc => skip += 1,
                _ if skip > 0 => {}
                "w:jc" if !align_set => {
                    align_set = true;
                    align = match a.get("w:val").map(|s| s.as_str()) {
                        Some("right") | Some("end") => Align::Right,
                        Some("center") => Align::Center,
                        Some("both") | Some("distribute") => Align::Justify,
                        _ => Align::Left,
                    };
                }
                "w:p" => {
                    para_base = default_style;
                }
                "w:pPr" => in_ppr = !sc,
                "w:rPr" => in_rpr = !sc,
                "w:r" => {
                    in_run = !sc;
                    run = para_base;
                    hidden = false;
                }
                "w:pStyle" if in_ppr => {
                    if let Some(st) = a.get("w:val").and_then(|v| styles.para.get(v)) {
                        para_base = st.2;
                    }
                }
                "w:rStyle" if in_rpr && in_run => {
                    if let Some(ops) = a.get("w:val").and_then(|v| styles.chars.get(v)) {
                        for (on_, oa) in ops {
                            apply_rpr(&mut run, &mut hidden, on_, oa, theme);
                        }
                    }
                }
                _ if in_rpr && in_run => apply_rpr(&mut run, &mut hidden, &n, &a, theme),
                "w:fldSimple" => {
                    let instr = a
                        .get("w:instr")
                        .map_or(String::new(), |s| s.trim().to_ascii_uppercase());
                    if instr.starts_with("PAGE") || instr.starts_with("NUMPAGES") {
                        page_field = true;
                        if !sc {
                            field_is_page = true;
                            in_field_result = true;
                        }
                    }
                }
                "w:fldChar" => match a.get("w:fldCharType").map(|s| s.as_str()) {
                    Some("begin") => {
                        field_depth += 1;
                        field_is_page = false;
                    }
                    Some("separate") => in_field_result = field_is_page,
                    Some("end") => {
                        field_depth = field_depth.saturating_sub(1);
                        in_field_result = false;
                        field_is_page = false;
                    }
                    _ => {}
                },
                "w:tab" | "w:ptab" => {
                    if !text.is_empty() {
                        text.push_str("   ");
                    }
                }
                "w:t" => in_t = !sc,
                _ => {}
            },
            Tok::Close(n) => match n.as_str() {
                "mc:Fallback" => skip = skip.saturating_sub(1),
                "w:t" => in_t = false,
                "w:pPr" => in_ppr = false,
                "w:rPr" => in_rpr = false,
                "w:r" => in_run = false,
                "w:fldSimple" => {
                    in_field_result = false;
                    field_is_page = false;
                }
                "w:p" => {
                    let s = text.trim().to_string();
                    if !s.is_empty() {
                        paras.push(s);
                    }
                    text.clear();
                }
                _ => {}
            },
            Tok::Text(s) if in_t && skip == 0 => {
                if first_style.is_none() && !s.trim().is_empty() && !hidden {
                    first_style = Some(run);
                }
                if hidden {
                    continue;
                }
                if in_field_result && field_is_page {
                    // keep "Page 1" wording so the editor's page counter can update it
                    let lower = text.to_ascii_lowercase();
                    let lt = lower.trim_end();
                    if lt.ends_with("page") || lt.ends_with(" of") {
                        text.push_str(&s);
                        page_kept = true;
                    }
                } else {
                    text.push_str(&s);
                }
            }
            Tok::Text(s) => {
                if field_depth > 0 && s.trim().to_ascii_uppercase().starts_with("PAGE") {
                    field_is_page = true;
                    page_field = true;
                }
            }
        }
    }
    // "Page 1 of 2" text is updated per page by the editor; only a bare PAGE field needs page numbers
    if let Some(st) = first_style {
        italic = st.italic;
    }
    (
        paras.join("  "),
        align,
        italic,
        page_field && !page_kept,
        first_style,
    )
}

#[derive(Clone, Default)]
struct PendingCell {
    spec: TableCellSpec,
    grid_start: usize,
    ovr_top: Option<Option<BorderSpec>>,
    ovr_bottom: Option<Option<BorderSpec>>,
    ovr_left: Option<Option<BorderSpec>>,
    ovr_right: Option<Option<BorderSpec>>,
}

#[derive(Clone, Default)]
struct PendingRow {
    cells: Vec<PendingCell>,
    min_height_pt: f32,
    grid_after: u16,
}

struct CurTable {
    id: u32,
    grid_cols_in: Vec<f32>,
    rows: Vec<PendingRow>,
    indent_in: f32,
    tbl_top: Option<BorderSpec>,
    tbl_bottom: Option<BorderSpec>,
    tbl_left: Option<BorderSpec>,
    tbl_right: Option<BorderSpec>,
    tbl_inside_h: Option<BorderSpec>,
    tbl_inside_v: Option<BorderSpec>,
    def_pad_left_in: f32,
    def_pad_right_in: f32,
    def_pad_top_in: f32,
    def_pad_bottom_in: f32,
    in_tbl_pr: bool,
    in_tbl_borders: bool,
    in_tbl_cell_mar: bool,
    cur_row: Option<PendingRow>,
    cur_row_idx: u16,
    cur_col_idx: u16,
    cur_grid_col: usize,
    cur_cell: Option<PendingCell>,
    cur_cell_has_tc_w: bool,
    cur_cell_para_count: usize,
    in_tr_pr: bool,
    in_tc_pr: bool,
    in_tc_borders: bool,
    in_tc_mar: bool,
    /// Paragraph / run defaults from the table style.
    pfmt: ParaFmt,
    pex: PExtra,
    base: CharStyle,
}

impl CurTable {
    fn new(id: u32, pfmt: ParaFmt, pex: PExtra, base: CharStyle) -> Self {
        Self {
            id,
            grid_cols_in: Vec::new(),
            rows: Vec::new(),
            indent_in: 0.0,
            tbl_top: None,
            tbl_bottom: None,
            tbl_left: None,
            tbl_right: None,
            tbl_inside_h: None,
            tbl_inside_v: None,
            def_pad_left_in: 108.0 / 1440.0,
            def_pad_right_in: 108.0 / 1440.0,
            def_pad_top_in: 0.0,
            def_pad_bottom_in: 0.0,
            in_tbl_pr: false,
            in_tbl_borders: false,
            in_tbl_cell_mar: false,
            cur_row: None,
            cur_row_idx: 0,
            cur_col_idx: 0,
            cur_grid_col: 0,
            cur_cell: None,
            cur_cell_has_tc_w: false,
            cur_cell_para_count: 0,
            in_tr_pr: false,
            in_tc_pr: false,
            in_tc_borders: false,
            in_tc_mar: false,
            pfmt,
            pex,
            base,
        }
    }

    fn set_border(&mut self, side: &str, b: Option<BorderSpec>) {
        match side {
            "w:top" => self.tbl_top = b,
            "w:bottom" => self.tbl_bottom = b,
            "w:left" | "w:start" => self.tbl_left = b,
            "w:right" | "w:end" => self.tbl_right = b,
            "w:insideH" => self.tbl_inside_h = b,
            "w:insideV" => self.tbl_inside_v = b,
            _ => {}
        }
    }

    fn set_cell_mar(&mut self, side: &str, a: &Attrs) {
        if let Some(w) = num(a, "w:w") {
            let inches = (w / 1440.0).max(0.0);
            match side {
                "w:left" | "w:start" => self.def_pad_left_in = inches,
                "w:right" | "w:end" => self.def_pad_right_in = inches,
                "w:top" => self.def_pad_top_in = inches,
                "w:bottom" => self.def_pad_bottom_in = inches,
                _ => {}
            }
        }
    }

    fn finish(self) -> DocTable {
        let num_rows = self.rows.len();
        let num_grid_cols = self.grid_cols_in.len().max(1);
        let mut final_rows = Vec::with_capacity(num_rows);
        for (r_idx, prow) in self.rows.into_iter().enumerate() {
            let mut final_cells = Vec::with_capacity(prow.cells.len());
            for pcell in prow.cells {
                let g_start = pcell.grid_start;
                let g_end = g_start + pcell.spec.grid_span.max(1) as usize;
                let def_top = if r_idx == 0 { self.tbl_top } else { self.tbl_inside_h };
                let def_bottom = if r_idx + 1 == num_rows { self.tbl_bottom } else { self.tbl_inside_h };
                let def_left = if g_start == 0 { self.tbl_left } else { self.tbl_inside_v };
                let def_right = if g_end >= num_grid_cols && prow.grid_after == 0 {
                    self.tbl_right
                } else {
                    self.tbl_inside_v
                };
                let borders = CellBorders {
                    top: pcell.ovr_top.unwrap_or(def_top),
                    bottom: pcell.ovr_bottom.unwrap_or(def_bottom),
                    left: pcell.ovr_left.unwrap_or(def_left),
                    right: pcell.ovr_right.unwrap_or(def_right),
                };
                let mut spec = pcell.spec;
                spec.borders = borders;
                final_cells.push(spec);
            }
            final_rows.push(TableRowSpec {
                cells: final_cells,
                min_height_pt: prow.min_height_pt,
                grid_after: prow.grid_after,
            });
        }
        DocTable {
            id: self.id,
            grid_cols_in: self.grid_cols_in,
            rows: final_rows,
            indent_in: self.indent_in,
        }
    }
}

fn style_index(id: &str) -> usize {
    let want = match id
        .to_ascii_lowercase()
        .replace([' ', '-', '_'], "")
        .as_str()
    {
        "bodysingle" => "Body Single",
        "title" => "Title",
        "subtitle" => "Subtitle",
        "heading1" => "Heading 1",
        "heading2" => "Heading 2",
        "heading3" | "heading4" | "heading5" | "heading6" => "Heading 3",
        "quote" | "intensequote" | "blockquote" => "Quote",
        "bullet1" | "listbullet" => "Bullet 1",
        "numberlist" | "listnumber" => "Number List",
        "code" | "htmlpreformatted" | "macrotext" => "Code",
        _ => return 0,
    };
    named_styles().iter().position(|s| s.name == want).unwrap_or(0)
}

/// Containers whose content must not be shown: alternate fallbacks, deleted / moved-away
/// text, and the *previous* formatting recorded by tracked changes.
fn is_skipped_container(n: &str) -> bool {
    matches!(
        n,
        "mc:Fallback"
            | "w:del"
            | "w:moveFrom"
            | "w:pPrChange"
            | "w:rPrChange"
            | "w:sectPrChange"
            | "w:tblPrChange"
            | "w:tblPrExChange"
            | "w:trPrChange"
            | "w:tcPrChange"
            | "w:tblGridChange"
            | "w:numberingChange"
    )
}

fn part_path(target: &str) -> String {
    if let Some(t) = target.strip_prefix('/') {
        t.to_string()
    } else if target.starts_with("word/") {
        target.to_string()
    } else {
        format!("word/{}", target.trim_start_matches("./"))
    }
}

/// Plain text of footnotes / endnotes keyed by id.
fn parse_notes(xml: &str, tag: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut cur: Option<String> = None;
    let mut text = String::new();
    let mut in_t = false;
    for t in tokens(xml) {
        match t {
            Tok::Open(n, a, sc) if n == tag && !sc => {
                let sep = a.get("w:type").is_some_and(|v| v != "normal");
                cur = if sep { None } else { a.get("w:id").cloned() };
                text.clear();
            }
            Tok::Open(n, _, sc) if n == "w:t" => in_t = !sc,
            Tok::Open(n, _, _) if n == "w:tab" && cur.is_some() => text.push(' '),
            Tok::Close(n) if n == "w:t" => in_t = false,
            Tok::Close(n)
                if n == "w:p" && cur.is_some() && !text.is_empty() && !text.ends_with(' ') =>
            {
                text.push(' ')
            }
            Tok::Close(n) if n == tag => {
                if let Some(id) = cur.take() {
                    out.insert(id, text.trim().to_string());
                }
            }
            Tok::Text(s) if in_t && cur.is_some() => text.push_str(&s),
            _ => {}
        }
    }
    out
}

/// State saved while reading a text box's paragraphs nested inside another paragraph.
struct Saved {
    para: Option<Para>,
    para_base: CharStyle,
    mark: CharStyle,
    run: CharStyle,
    in_run: bool,
    run_hidden: bool,
    pex: PExtra,
    style_id: String,
}

pub fn read(data: &[u8]) -> Result<Document, String> {
    let files = unzip(data)?;
    // The main part is named by the package relationships (usually word/document.xml).
    let main_path = files
        .get("_rels/.rels")
        .and_then(|r| {
            let r = String::from_utf8_lossy(r);
            tokens(&r).into_iter().find_map(|t| match t {
                Tok::Open(n, a, _)
                    if n.ends_with("Relationship")
                        && a.get("Type")
                            .is_some_and(|t| t.ends_with("/officeDocument")) =>
                {
                    a.get("Target")
                        .map(|t| t.trim_start_matches('/').to_string())
                }
                _ => None,
            })
        })
        .filter(|p| files.contains_key(p))
        .unwrap_or_else(|| "word/document.xml".into());
    let xml = files
        .get(&main_path)
        .ok_or("this file has no Word document inside")?;
    let xml = String::from_utf8_lossy(xml);
    let rels_path = main_path.rfind('/').map_or_else(
        || format!("_rels/{main_path}.rels"),
        |i| format!("{}/_rels/{}.rels", &main_path[..i], &main_path[i + 1..]),
    );

    let mut rel_targets: HashMap<String, String> = HashMap::new();
    if let Some(rels_xml) = files.get(&rels_path) {
        for t in tokens(&String::from_utf8_lossy(rels_xml)) {
            if let Tok::Open(n, a, _) = t {
                if n.ends_with("Relationship") {
                    if let (Some(id), Some(target)) = (a.get("Id"), a.get("Target")) {
                        rel_targets.insert(id.clone(), target.clone());
                    }
                }
            }
        }
    }
    let theme = rel_targets
        .values()
        .find(|t| t.contains("theme"))
        .and_then(|t| files.get(&part_path(t)))
        .or_else(|| files.get("word/theme/theme1.xml"))
        .map(|b| Theme::parse(&String::from_utf8_lossy(b)))
        .unwrap_or_default();
    let styles = files
        .get("word/styles.xml")
        .map(|b| Styles::parse(&String::from_utf8_lossy(b), &theme))
        .unwrap_or_else(|| Styles {
            base: CharStyle {
                font: theme.minor.unwrap_or(0),
                size: 10.0,
                ..CharStyle::default()
            },
            ..Default::default()
        });
    let numbering = files
        .get("word/numbering.xml")
        .map(|b| Numbering::parse(&String::from_utf8_lossy(b), &theme))
        .unwrap_or_default();
    let footnotes = files
        .get("word/footnotes.xml")
        .map(|b| parse_notes(&String::from_utf8_lossy(b), "w:footnote"))
        .unwrap_or_default();
    let endnotes = files
        .get("word/endnotes.xml")
        .map(|b| parse_notes(&String::from_utf8_lossy(b), "w:endnote"))
        .unwrap_or_default();

    let mut doc = Document {
        paras: Vec::new(),
        ..Default::default()
    };
    if let Some(set) = files.get("word/settings.xml") {
        for t in tokens(&String::from_utf8_lossy(set)) {
            if let Tok::Open(n, a, _) = t {
                if n == "w:defaultTabStop" {
                    if let Some(v) = num(&a, "w:val") {
                        if v > 36.0 {
                            doc.default_tab_in = v / 1440.0;
                        }
                    }
                }
            }
        }
    }

    let base = styles.base;
    let named = named_styles();
    let mut list_ids: HashMap<String, u16> = HashMap::new();
    let mut notes_out: Vec<(usize, String)> = Vec::new();

    let mut para: Option<Para> = None;
    let mut in_ppr = false;
    let mut in_pbdr = false;
    let mut in_tabs = false;
    let mut in_rpr = false;
    let mut in_run = false;
    let mut in_text = false;
    let mut in_drawing = false;
    let mut run_hidden = false;
    let mut legacy_list: Option<(String, u8)> = None; // app-written list markers: (kind, preset)
    let mut is_app_file = false;
    let mut cur_link: u16 = 0;
    let mut para_base = base;
    let mut mark = base;
    let mut run = base;
    let mut pex = PExtra::default();
    let mut direct_ind: Option<Attrs> = None;
    let mut ppr_seen = false;
    let mut style_id = String::new();
    let mut pending_break = false;
    let mut in_sect = false;
    let mut sect_in_ppr = false;
    let mut header_type_ok = false;
    let mut app_hdr_styled = false;
    let mut app_ftr_styled = false;
    let mut footer_type_ok = false;
    let mut draw_cx: i64 = 0;
    let mut draw_cy: i64 = 0;
    let mut draw_id: u64 = 0;
    let mut draw_name = String::new();
    let mut draw_caption = String::new();
    let mut draw_nat_w: u32 = 0;
    let mut draw_nat_h: u32 = 0;
    let mut draw_seamless = false;
    let mut draw_rid = String::new();
    let mut cur_tbl: Option<CurTable> = None;
    let mut tbl_depth = 0usize;
    let mut next_tbl_id: u32 = 1;
    let mut skip_depth = 0usize; // inside mc:Fallback / w:del-like containers we ignore
    let mut saved: Vec<Saved> = Vec::new();
    // floating shape anchor: (horizontal rel, offset pt, vertical rel, offset pt)
    let mut anchor: Option<(String, f32, String, f32)> = None;
    let mut pos_axis: Option<char> = None;
    let mut in_pos_offset = false;
    // text boxes to place once page margins are known: (first para, end para, anchor, width pt)
    let mut tb_fixups: Vec<(usize, usize, (String, f32, String, f32), f32)> = Vec::new();
    let mut tb_open: Vec<(usize, Option<(String, f32, String, f32)>, f32)> = Vec::new();
    // (style id, contextual spacing) of the last finished paragraph, for contextual spacing
    let mut last_meta: Option<(String, bool)> = None;

    // Pushes a finished paragraph, honouring Word's "don't add space between paragraphs of
    // the same style" (contextualSpacing).
    let push_para = |doc: &mut Document,
                     mut p: Para,
                     sid: &str,
                     ctx: bool,
                     last: &mut Option<(String, bool)>| {
        if let Some((lsid, lctx)) = last.as_ref() {
            let same_table = doc
                .paras
                .last()
                .is_some_and(|q| q.fmt.table_id == p.fmt.table_id);
            if lsid == sid && same_table {
                if *lctx {
                    if let Some(prev) = doc.paras.last_mut() {
                        prev.fmt.space_after = 0.0;
                    }
                }
                if ctx {
                    p.fmt.space_before = 0.0;
                }
            }
        }
        *last = Some((sid.to_string(), ctx));
        doc.paras.push(p);
    };

    for t in tokens(&xml) {
        // Skip alternate (fallback) content and nested-table structure we flatten.
        match &t {
            Tok::Open(n, _, sc) if is_skipped_container(n) => {
                if !sc {
                    skip_depth += 1;
                }
                continue;
            }
            Tok::Close(n) if is_skipped_container(n) => {
                skip_depth = skip_depth.saturating_sub(1);
                continue;
            }
            _ if skip_depth > 0 => continue,
            _ => {}
        }
        if tbl_depth > 1 {
            let structural = |n: &str| {
                matches!(
                    n,
                    "w:tbl"
                        | "w:tblPr"
                        | "w:tblGrid"
                        | "w:gridCol"
                        | "w:tr"
                        | "w:trPr"
                        | "w:tc"
                        | "w:tcPr"
                        | "w:tblBorders"
                        | "w:tcBorders"
                        | "w:tblCellMar"
                        | "w:tcMar"
                )
            };
            match &t {
                Tok::Open(n, _, sc) if n == "w:tbl" && !sc => {
                    tbl_depth += 1;
                    continue;
                }
                Tok::Close(n) if n == "w:tbl" => {
                    tbl_depth -= 1;
                    continue;
                }
                Tok::Open(n, _, _) | Tok::Close(n) if structural(n) => continue,
                _ => {}
            }
        }
        match t {
            Tok::Open(n, a, selfclose) => match n.as_str() {
                "w:background" => {
                    if let Some(c) = a.get("w:color").and_then(|v| parse_hex(v)) {
                        doc.page.background = c;
                    }
                }
                "w:txbxContent" if !selfclose => {
                    tb_open.push((doc.paras.len(), anchor.clone(), draw_cx as f32 / 12_700.0));
                    saved.push(Saved {
                        para: para.take(),
                        para_base,
                        mark,
                        run,
                        in_run,
                        run_hidden,
                        pex: pex.clone(),
                        style_id: style_id.clone(),
                    });
                    in_run = false;
                    in_text = false;
                }
                "w:tbl" => {
                    if !selfclose {
                        tbl_depth += 1;
                        if tbl_depth == 1 {
                            let tid = next_tbl_id;
                            next_tbl_id += 1;
                            // the default table style (Table Normal) applies until w:tblStyle says otherwise
                            let mut tb = CurTable::new(
                                tid,
                                styles.default_pfmt,
                                styles.default_ex.clone(),
                                base,
                            );
                            if let Some(ts) = styles
                                .default_table
                                .as_ref()
                                .and_then(|id| styles.tables.get(id))
                            {
                                for (n, a) in &ts.cell_mar {
                                    tb.set_cell_mar(n, a);
                                }
                            }
                            cur_tbl = Some(tb);
                        }
                    }
                }
                "w:tblPr" => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.in_tbl_pr = !selfclose;
                    }
                }
                "w:tblStyle" if cur_tbl.as_ref().is_some_and(|tb| tb.in_tbl_pr) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        let val = a.get("w:val").map_or("", |v| v.as_str());
                        if let Some(ts) = styles.tables.get(val) {
                            let mut hidden = false;
                            for (n, a, b) in &ts.pfmt_ops {
                                apply_ppr(&mut tb.pfmt, &mut tb.pex, n, a, *b);
                            }
                            for (n, a) in &ts.rpr_ops {
                                apply_rpr(&mut tb.base, &mut hidden, n, a, &theme);
                            }
                            for (n, a) in &ts.borders {
                                tb.set_border(n, parse_border(a));
                            }
                            for (n, a) in &ts.cell_mar {
                                tb.set_cell_mar(n, a);
                            }
                        } else if val.to_ascii_lowercase().contains("grid") {
                            let b = Some(BorderSpec {
                                width_pt: 0.5,
                                color: [0, 0, 0],
                            });
                            for side in [
                                "w:top",
                                "w:bottom",
                                "w:left",
                                "w:right",
                                "w:insideH",
                                "w:insideV",
                            ] {
                                tb.set_border(side, b);
                            }
                        }
                    }
                }
                "w:tblInd" if cur_tbl.as_ref().is_some_and(|tb| tb.in_tbl_pr) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        if let Some(w) = num(&a, "w:w") {
                            tb.indent_in = w / 1440.0;
                        }
                    }
                }
                "w:tblBorders" if cur_tbl.as_ref().is_some_and(|tb| tb.in_tbl_pr) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.in_tbl_borders = !selfclose;
                    }
                }
                "w:tblCellMar" if cur_tbl.as_ref().is_some_and(|tb| tb.in_tbl_pr) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.in_tbl_cell_mar = !selfclose;
                    }
                }
                "w:gridCol" if cur_tbl.is_some() => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        let w = num(&a, "w:w").unwrap_or(2880.0);
                        tb.grid_cols_in.push((w / 1440.0).max(0.05));
                    }
                }
                "w:tr" if cur_tbl.is_some() => {
                    if !selfclose {
                        if let Some(tb) = cur_tbl.as_mut() {
                            tb.cur_row_idx = tb.rows.len() as u16;
                            tb.cur_col_idx = 0;
                            tb.cur_grid_col = 0;
                            tb.cur_row = Some(PendingRow::default());
                        }
                    }
                }
                "w:trPr" if cur_tbl.is_some() => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.in_tr_pr = !selfclose;
                    }
                }
                "w:gridBefore" if cur_tbl.as_ref().is_some_and(|tb| tb.in_tr_pr) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.cur_grid_col = a.get("w:val").and_then(|v| v.parse().ok()).unwrap_or(0);
                    }
                }
                "w:gridAfter" if cur_tbl.as_ref().is_some_and(|tb| tb.in_tr_pr) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        if let Some(r) = tb.cur_row.as_mut() {
                            r.grid_after = a.get("w:val").and_then(|v| v.parse().ok()).unwrap_or(0);
                        }
                    }
                }
                "w:trHeight" if cur_tbl.as_ref().is_some_and(|tb| tb.in_tr_pr) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        if let Some(r) = tb.cur_row.as_mut() {
                            if let Some(v) = num(&a, "w:val") {
                                r.min_height_pt = (v / 20.0).max(0.0);
                            }
                        }
                    }
                }
                "w:tc" if cur_tbl.is_some() => {
                    if !selfclose {
                        if let Some(tb) = cur_tbl.as_mut() {
                            let def_w = tb.grid_cols_in.get(tb.cur_grid_col).copied().unwrap_or(2.0);
                            tb.cur_cell = Some(PendingCell {
                                spec: TableCellSpec {
                                    width_in: def_w,
                                    grid_span: 1,
                                    shading: None,
                                    borders: CellBorders::default(),
                                    v_align: CellVAlign::Top,
                                    pad_left_in: tb.def_pad_left_in,
                                    pad_right_in: tb.def_pad_right_in,
                                    pad_top_in: tb.def_pad_top_in,
                                    pad_bottom_in: tb.def_pad_bottom_in,
                                    formula: None,
                                    wrap_text: true,
                                },
                                grid_start: tb.cur_grid_col,
                                ovr_top: None,
                                ovr_bottom: None,
                                ovr_left: None,
                                ovr_right: None,
                            });
                            tb.cur_cell_has_tc_w = false;
                            tb.cur_cell_para_count = 0;
                        }
                    }
                }
                "w:tcPr" if cur_tbl.is_some() => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.in_tc_pr = !selfclose;
                    }
                }
                "w:tcBorders" if cur_tbl.as_ref().is_some_and(|tb| tb.in_tc_pr) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.in_tc_borders = !selfclose;
                    }
                }
                "w:tcMar" if cur_tbl.as_ref().is_some_and(|tb| tb.in_tc_pr) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.in_tc_mar = !selfclose;
                    }
                }
                _ if cur_tbl.as_ref().is_some_and(|tb| tb.in_tbl_borders) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.set_border(&n, parse_border(&a));
                    }
                }
                _ if cur_tbl.as_ref().is_some_and(|tb| tb.in_tbl_cell_mar) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.set_cell_mar(&n, &a);
                    }
                }
                _ if cur_tbl.as_ref().is_some_and(|tb| tb.in_tc_borders) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        if let Some(cell) = tb.cur_cell.as_mut() {
                            let b = parse_border(&a);
                            match n.as_str() {
                                "w:top" => cell.ovr_top = Some(b),
                                "w:bottom" => cell.ovr_bottom = Some(b),
                                "w:left" | "w:start" => cell.ovr_left = Some(b),
                                "w:right" | "w:end" => cell.ovr_right = Some(b),
                                _ => {}
                            }
                        }
                    }
                }
                _ if cur_tbl.as_ref().is_some_and(|tb| tb.in_tc_mar) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        if let Some(cell) = tb.cur_cell.as_mut() {
                            if let Some(w) = num(&a, "w:w") {
                                let inches = (w / 1440.0).max(0.0);
                                match n.as_str() {
                                    "w:left" | "w:start" => cell.spec.pad_left_in = inches,
                                    "w:right" | "w:end" => cell.spec.pad_right_in = inches,
                                    "w:top" => cell.spec.pad_top_in = inches,
                                    "w:bottom" => cell.spec.pad_bottom_in = inches,
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                _ if cur_tbl.as_ref().is_some_and(|tb| tb.in_tc_pr) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        if let Some(cell) = tb.cur_cell.as_mut() {
                            match n.as_str() {
                                "w:tcW" => {
                                    if let Some(w) = num(&a, "w:w") {
                                        if w > 0.0 && a.get("w:type").is_none_or(|t| t == "dxa") {
                                            cell.spec.width_in = w / 1440.0;
                                            tb.cur_cell_has_tc_w = true;
                                        }
                                    }
                                }
                                "w:gridSpan" => {
                                    if let Some(sp) = a.get("w:val").and_then(|v| v.parse::<u16>().ok()) {
                                        cell.spec.grid_span = sp.max(1);
                                        if !tb.cur_cell_has_tc_w {
                                            let g0 = cell.grid_start;
                                            let g1 = (g0 + cell.spec.grid_span as usize).min(tb.grid_cols_in.len());
                                            if g1 > g0 {
                                                cell.spec.width_in = tb.grid_cols_in[g0..g1].iter().sum();
                                            }
                                        }
                                    }
                                }
                                "w:shd" => {
                                    if let Some(c) = a.get("w:fill").and_then(|v| parse_hex(v)) {
                                        if c != [255, 255, 255] {
                                            cell.spec.shading = Some(c);
                                        }
                                    }
                                }
                                "w:vAlign" => {
                                    cell.spec.v_align = match a.get("w:val").map(|s| s.as_str()) {
                                        Some("center") => CellVAlign::Center,
                                        Some("bottom") => CellVAlign::Bottom,
                                        _ => CellVAlign::Top,
                                    };
                                }
                                "w:noWrap" => cell.spec.wrap_text = !on(&a),
                                _ => {}
                            }
                        }
                    }
                }
                "w:p" => {
                    let (mut pfmt, ex, pbase) = match cur_tbl.as_ref() {
                        Some(tb) => (tb.pfmt, tb.pex.clone(), tb.base),
                        None => (styles.default_pfmt, styles.default_ex.clone(), base),
                    };
                    if let Some(tb) = cur_tbl.as_mut() {
                        pfmt.table_id = tb.id;
                        pfmt.table_row = tb.cur_row_idx;
                        pfmt.table_col = tb.cur_col_idx;
                        tb.cur_cell_para_count += 1;
                    }
                    pfmt.page_break_before = std::mem::take(&mut pending_break);
                    para = Some(Para::new(pbase, pfmt));
                    para_base = pbase;
                    mark = pbase;
                    pex = ex;
                    direct_ind = None;
                    ppr_seen = false;
                    legacy_list = None;
                    style_id.clear();
                    if selfclose {
                        if let Some(p) = para.take() {
                            push_para(&mut doc, p, "", false, &mut last_meta);
                        }
                    }
                }
                "w:pPr" => in_ppr = !selfclose,
                "w:pBdr" if in_ppr => in_pbdr = !selfclose,
                "w:tabs" if in_ppr => in_tabs = !selfclose,
                "w:rPr" => in_rpr = !selfclose,
                "w:hyperlink" => {
                    if !selfclose {
                        if let Some(url) = a.get("r:id").and_then(|rid| rel_targets.get(rid)) {
                            cur_link = doc.add_link(url);
                        }
                    }
                }
                "w:r" => {
                    in_run = !selfclose;
                    run = para_base;
                    run_hidden = false;
                    if cur_link > 0 {
                        run.link = cur_link;
                    }
                }
                "w:drawing" | "w:pict" | "w:object" if in_run => {
                    in_drawing = !selfclose;
                    draw_cx = 0;
                    draw_cy = 0;
                    draw_id = 0;
                    draw_name.clear();
                    draw_caption.clear();
                    draw_nat_w = 0;
                    draw_nat_h = 0;
                    draw_seamless = false;
                    draw_rid.clear();
                }
                "wp:anchor" if in_drawing => {
                    anchor = Some((String::new(), 0.0, String::new(), 0.0));
                }
                "wp:positionH" | "wp:positionV" if in_drawing => {
                    let axis = if n == "wp:positionH" { 'h' } else { 'v' };
                    pos_axis = Some(axis);
                    if let Some(an) = anchor.as_mut() {
                        let rel = a.get("relativeFrom").cloned().unwrap_or_default();
                        if axis == 'h' {
                            an.0 = rel;
                        } else {
                            an.2 = rel;
                        }
                    }
                }
                "wp:posOffset" if in_drawing => in_pos_offset = !selfclose,
                "wp:extent" if in_drawing => {
                    draw_cx = a.get("cx").and_then(|v| v.parse().ok()).unwrap_or(0);
                    draw_cy = a.get("cy").and_then(|v| v.parse().ok()).unwrap_or(0);
                }
                "wp:docPr" if in_drawing => {
                    draw_id = ext_attr(&a, "imageId")
                        .or_else(|| a.get("id"))
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    draw_name = a.get("name").cloned().unwrap_or_default();
                    draw_caption = a.get("descr").cloned().unwrap_or_default();
                    draw_nat_w = ext_attr(&a, "naturalW")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    draw_nat_h = ext_attr(&a, "naturalH")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    draw_seamless = a.get("sy:seamless").is_some_and(|v| v == "1");
                }
                "a:blip" if in_drawing && draw_rid.is_empty() => {
                    draw_rid = a.get("r:embed").cloned().unwrap_or_default();
                }
                "v:imagedata" if in_drawing && draw_rid.is_empty() => {
                    draw_rid = a.get("r:id").cloned().unwrap_or_default();
                }
                "v:shape" if in_drawing => {
                    // style="width:120pt;height:80pt"
                    if let Some(style) = a.get("style") {
                        for part in style.split(';') {
                            let mut kv = part.splitn(2, ':');
                            let (k, v) = (
                                kv.next().unwrap_or("").trim(),
                                kv.next().unwrap_or("").trim(),
                            );
                            let pt = if let Some(x) = v.strip_suffix("pt") {
                                x.parse::<f32>().ok()
                            } else if let Some(x) = v.strip_suffix("in") {
                                x.parse::<f32>().ok().map(|i| i * 72.0)
                            } else {
                                None
                            };
                            if let Some(pt) = pt {
                                if k == "width" {
                                    draw_cx = (pt * 12_700.0) as i64;
                                } else if k == "height" {
                                    draw_cy = (pt * 12_700.0) as i64;
                                }
                            }
                        }
                    }
                }
                "w:t" => in_text = !selfclose,
                "w:sectPr" => {
                    in_sect = !selfclose;
                    sect_in_ppr = in_ppr;
                    if ext_attr(&a, "pgNum").is_some_and(|v| v == "1" || v == "true") {
                        doc.page.page_numbers = true;
                    }
                    if a.contains_key("sy:app") || a.contains_key("w:headerText") {
                        is_app_file = true;
                    }
                    if ext_attr(&a, "pageless").is_some_and(|v| v == "1" || v == "true") {
                        doc.pageless = true;
                    }
                    let al = |v: &str| match v {
                        "left" => Align::Left,
                        "right" => Align::Right,
                        "both" => Align::Justify,
                        _ => Align::Center,
                    };
                    if let Some(h) = ext_attr(&a, "headerText") {
                        doc.header = h.clone();
                    }
                    if let Some(f) = ext_attr(&a, "footerText") {
                        doc.footer = f.clone();
                    }
                    if let Some(ha) = ext_attr(&a, "headerAlign") {
                        doc.header_align = al(ha);
                    }
                    if let Some(fa) = ext_attr(&a, "footerAlign") {
                        doc.footer_align = al(fa);
                    }
                    if let Some(hi) = ext_attr(&a, "headerItalic") {
                        doc.header_italic = hi == "1" || hi == "true";
                    }
                    if let Some(fi) = ext_attr(&a, "footerItalic") {
                        doc.footer_italic = fi == "1" || fi == "true";
                    }
                    app_hdr_styled = a.get("sy:hdrStyled").is_some_and(|v| v == "1");
                    app_ftr_styled = a.get("sy:ftrStyled").is_some_and(|v| v == "1");
                    if let Some(v) = a.get("sy:headerDist").and_then(|v| v.parse::<f32>().ok()) {
                        doc.header_dist_in = v;
                    }
                    if let Some(v) = a.get("sy:footerDist").and_then(|v| v.parse::<f32>().ok()) {
                        doc.footer_dist_in = v;
                    }
                }
                "w:type" if in_sect && sect_in_ppr => {
                    // a section break inside a paragraph starts the next section on a new page
                    if !matches!(a.get("w:val").map(|s| s.as_str()), Some("continuous")) {
                        pending_break = true;
                    }
                }
                "w:headerReference" | "w:footerReference" if in_sect && !sect_in_ppr => {
                    let ty = a.get("w:type").map_or("default", |s| s.as_str());
                    let is_header = n == "w:headerReference";
                    let have_default = if is_header {
                        header_type_ok
                    } else {
                        footer_type_ok
                    };
                    let part = a
                        .get("r:id")
                        .and_then(|rid| rel_targets.get(rid))
                        .and_then(|t| files.get(&part_path(t)));
                    if is_app_file {
                        // Syllabary files keep their text in sy: attributes; only the style comes from the part
                        if let Some(bytes) = part.filter(|_| {
                            if is_header {
                                app_hdr_styled
                            } else {
                                app_ftr_styled
                            }
                        }) {
                            let st = parse_hf_part(bytes, &styles, &theme, !is_header).4;
                            if is_header {
                                doc.header_style = st;
                            } else {
                                doc.footer_style = st;
                            }
                        }
                    } else if ty == "default" || !have_default {
                        if let Some(bytes) = part {
                            let (txt, al, it, page, st) =
                                parse_hf_part(bytes, &styles, &theme, !is_header);
                            if page && !is_header {
                                doc.page.page_numbers = true;
                            }
                            if !txt.is_empty() || ty == "default" {
                                if is_header {
                                    doc.header = txt;
                                    doc.header_align = al;
                                    doc.header_italic = it;
                                    doc.header_style = st;
                                    header_type_ok |= ty == "default";
                                } else {
                                    doc.footer = txt;
                                    doc.footer_align = al;
                                    doc.footer_italic = it;
                                    doc.footer_style = st;
                                    footer_type_ok |= ty == "default";
                                }
                            }
                        }
                    }
                }
                "w:pgSz" if in_sect => {
                    let w = num(&a, "w:w");
                    let h = num(&a, "w:h");
                    if let (Some(w), Some(h)) = (w, h) {
                        let mut pw = (w / 1440.0 * 100.0).round() / 100.0;
                        let mut ph = (h / 1440.0 * 100.0).round() / 100.0;
                        for &(_, sw, sh) in PAGE_SIZES {
                            if (pw - sw).abs() < 0.02 && (ph - sh).abs() < 0.02 {
                                pw = sw;
                                ph = sh;
                                break;
                            } else if (pw - sh).abs() < 0.02 && (ph - sw).abs() < 0.02 {
                                pw = sh;
                                ph = sw;
                                break;
                            }
                        }
                        doc.page.width = pw;
                        doc.page.height = ph;
                    }
                }
                "w:pgMar" if in_sect => {
                    let g = |k: &str| {
                        num(&a, k).map(|v| {
                            let raw = v.abs() / 1440.0;
                            let snapped = (raw * 100.0).round() / 100.0;
                            if (raw - snapped).abs() < 0.0005 { snapped } else { raw }
                        })
                    };
                    if let Some(v) = g("w:top") {
                        doc.page.margin_top = v;
                    }
                    if let Some(v) = g("w:bottom") {
                        doc.page.margin_bottom = v;
                    }
                    if let Some(v) = g("w:left") {
                        doc.page.margin_left = v;
                    }
                    if let Some(v) = g("w:right") {
                        doc.page.margin_right = v;
                    }
                    if !is_app_file {
                        if let Some(v) = g("w:header") {
                            doc.header_dist_in = v;
                        }
                        if let Some(v) = g("w:footer") {
                            doc.footer_dist_in = v;
                        }
                    }
                }
                _ if in_sect => {}
                _ if in_rpr && in_run => {
                    if n == "w:rStyle" {
                        if let Some(ops) = a.get("w:val").and_then(|v| styles.chars.get(v)) {
                            for (op_n, op_a) in ops {
                                apply_rpr(&mut run, &mut run_hidden, op_n, op_a, &theme);
                            }
                        }
                    } else {
                        apply_rpr(&mut run, &mut run_hidden, &n, &a, &theme);
                    }
                }
                _ if in_rpr && in_ppr => {
                    let mut h = false;
                    if n == "w:rStyle" {
                        if let Some(ops) = a.get("w:val").and_then(|v| styles.chars.get(v)) {
                            for (op_n, op_a) in ops {
                                apply_rpr(&mut mark, &mut h, op_n, op_a, &theme);
                            }
                        }
                    } else {
                        apply_rpr(&mut mark, &mut h, &n, &a, &theme);
                    }
                }
                _ if in_ppr => {
                    let Some(p) = para.as_mut() else { continue };
                    match n.as_str() {
                        "w:pStyle" => {
                            let val = a.get("w:val").map_or("", |v| v.as_str());
                            style_id = val.to_string();
                            let keep = p.fmt;
                            let idx = style_index(val);
                            if let Some((sfmt, sex, schr)) = styles.para.get(val) {
                                p.fmt = ParaFmt {
                                    page_break_before: keep.page_break_before
                                        || sfmt.page_break_before,
                                    table_id: keep.table_id,
                                    table_row: keep.table_row,
                                    table_col: keep.table_col,
                                    ..*sfmt
                                };
                                pex = sex.clone();
                                para_base = *schr;
                                mark = *schr;
                            } else if idx != 0 {
                                p.fmt = ParaFmt {
                                    style: idx,
                                    page_break_before: keep.page_break_before,
                                    table_id: keep.table_id,
                                    table_row: keep.table_row,
                                    table_col: keep.table_col,
                                    ..named[idx].fmt
                                };
                                para_base = named[idx].chr;
                                mark = named[idx].chr;
                            }
                        }
                        "w:ind" => {
                            direct_ind = Some(a.clone());
                            apply_ppr(&mut p.fmt, &mut pex, &n, &a, false);
                        }
                        "w:preset" => {
                            legacy_list = Some((
                                "legacy".into(),
                                a.get("w:val").and_then(|v| v.parse().ok()).unwrap_or(0),
                            ));
                        }
                        "sy:list" => {
                            legacy_list = Some((
                                a.get("sy:kind").cloned().unwrap_or_else(|| "bullet".into()),
                                a.get("sy:preset").and_then(|v| v.parse().ok()).unwrap_or(0),
                            ));
                        }
                        "sy:hr" => {
                            p.fmt.hr = true;
                            if let Some(b) = p.fmt.pborders.bottom.take() {
                                p.fmt.hr_color = Some(b.color);
                                p.fmt.hr_width_pt = b.width_pt;
                            }
                        }
                        "w:tab" if in_tabs => apply_ppr(&mut p.fmt, &mut pex, &n, &a, false),
                        _ => apply_ppr(&mut p.fmt, &mut pex, &n, &a, in_pbdr),
                    }
                }
                "w:tab" | "w:ptab" if in_run => {
                    if let Some(p) = para.as_mut() {
                        if !run_hidden {
                            p.text.push('\t');
                            p.styles.push(run);
                        }
                    }
                }
                "w:noBreakHyphen" if in_run => {
                    if let Some(p) = para.as_mut() {
                        p.text.push('\u{2011}');
                        p.styles.push(run);
                    }
                }
                "w:sym" if in_run => {
                    if let (Some(p), Some(code)) = (
                        para.as_mut(),
                        a.get("w:char")
                            .and_then(|c| u32::from_str_radix(c, 16).ok()),
                    ) {
                        let font = a.get("w:font").map_or("Symbol", |s| s.as_str());
                        let raw: String =
                            char::from_u32(code).map(String::from).unwrap_or_default();
                        let (txt, _) = map_symbol_bullet(&raw, font);
                        for c in txt.chars() {
                            p.text.push(c);
                            p.styles.push(run);
                        }
                    }
                }
                "w:footnoteReference" | "w:endnoteReference" if in_run => {
                    let id = a.get("w:id").cloned().unwrap_or_default();
                    let body = if n == "w:footnoteReference" {
                        footnotes.get(&id)
                    } else {
                        endnotes.get(&id)
                    };
                    if let (Some(p), Some(body)) = (para.as_mut(), body) {
                        let k = notes_out.len() + 1;
                        let st = CharStyle { script: 1, ..run };
                        for c in k.to_string().chars() {
                            p.text.push(c);
                            p.styles.push(st);
                        }
                        notes_out.push((k, body.clone()));
                    }
                }
                "w:br" | "w:cr" if in_run => {
                    let ty = a.get("w:type").map_or("textWrapping", |s| s.as_str());
                    if ty == "page" {
                        // continue the same paragraph formatting on the next page
                        if let Some(p) = para.as_mut() {
                            let fmt = p.fmt;
                            if p.text.is_empty() {
                                p.fmt.page_break_before = true;
                            } else {
                                let done = std::mem::replace(
                                    p,
                                    Para::new(
                                        run,
                                        ParaFmt {
                                            page_break_before: true,
                                            ..fmt
                                        },
                                    ),
                                );
                                push_para(
                                    &mut doc,
                                    done,
                                    &style_id,
                                    pex.contextual.unwrap_or(false),
                                    &mut last_meta,
                                );
                                if let Some(tb) = cur_tbl.as_mut() {
                                    tb.cur_cell_para_count += 1;
                                }
                            }
                        }
                    } else if let Some(p) = para.as_mut() {
                        p.text.push('\n');
                        p.styles.push(run);
                    }
                }
                _ => {}
            },
            Tok::Close(n) => match n.as_str() {
                "w:txbxContent" => {
                    if let Some(p) = para.take() {
                        push_para(&mut doc, p, &style_id, false, &mut last_meta);
                    }
                    if let Some((start, Some(an), w)) = tb_open.pop() {
                        if doc.paras.len() > start {
                            tb_fixups.push((start, doc.paras.len(), an, w));
                        }
                    }
                    if let Some(s) = saved.pop() {
                        para = s.para;
                        para_base = s.para_base;
                        mark = s.mark;
                        run = s.run;
                        in_run = s.in_run;
                        run_hidden = s.run_hidden;
                        pex = s.pex;
                        style_id = s.style_id;
                    }
                }
                "w:tblPr" => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.in_tbl_pr = false;
                        tb.in_tbl_borders = false;
                        tb.in_tbl_cell_mar = false;
                    }
                }
                "w:tblBorders" => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.in_tbl_borders = false;
                    }
                }
                "w:tblCellMar" => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.in_tbl_cell_mar = false;
                    }
                }
                "w:trPr" => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.in_tr_pr = false;
                    }
                }
                "w:tcPr" => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.in_tc_pr = false;
                        tb.in_tc_borders = false;
                        tb.in_tc_mar = false;
                    }
                }
                "w:tcBorders" => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.in_tc_borders = false;
                    }
                }
                "w:tcMar" => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        tb.in_tc_mar = false;
                    }
                }
                "w:tc" => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        if tb.cur_cell_para_count == 0 {
                            let pfmt = ParaFmt {
                                table_id: tb.id,
                                table_row: tb.cur_row_idx,
                                table_col: tb.cur_col_idx,
                                ..tb.pfmt
                            };
                            doc.paras.push(Para::new(tb.base, pfmt));
                        }
                        if let Some(cell) = tb.cur_cell.take() {
                            tb.cur_grid_col += cell.spec.grid_span.max(1) as usize;
                            tb.cur_col_idx += 1;
                            if let Some(row) = tb.cur_row.as_mut() {
                                row.cells.push(cell);
                            }
                        }
                    }
                }
                "w:tr" => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        if let Some(row) = tb.cur_row.take() {
                            tb.rows.push(row);
                        }
                    }
                }
                "w:tbl" => {
                    tbl_depth = tbl_depth.saturating_sub(1);
                    if tbl_depth == 0 {
                        if let Some(tb) = cur_tbl.take() {
                            doc.tables.push(tb.finish());
                        }
                        last_meta = None;
                    }
                }
                "w:pBdr" => in_pbdr = false,
                "w:tabs" => in_tabs = false,
                "w:pPr" => {
                    in_ppr = false;
                    in_pbdr = false;
                    in_tabs = false;
                    if let Some(p) = para.as_mut() {
                        p.end_style = mark;
                        ppr_seen = true;
                        resolve_list(
                            &mut doc.list_defs,
                            &numbering,
                            &mut list_ids,
                            p,
                            &pex,
                            direct_ind.as_ref(),
                            legacy_list.as_ref(),
                        );
                    }
                }
                "w:sectPr" => {
                    in_sect = false;
                    sect_in_ppr = false;
                }
                "w:drawing" | "w:pict" | "w:object" => {
                    if !in_drawing {
                        continue;
                    }
                    in_drawing = false;
                    let Some(img_bytes) = rel_targets
                        .get(&draw_rid)
                        .and_then(|t| files.get(&part_path(t)))
                    else {
                        continue;
                    };
                    let Ok(dimg) = image::load_from_memory(img_bytes) else {
                        continue;
                    };
                    let Some(p) = para.as_mut() else { continue };
                    let mut id = if draw_id > 0 {
                        draw_id
                    } else {
                        doc.images.len() as u64 + 1
                    };
                    while doc.images.iter().any(|im| im.id == id) {
                        id += 1;
                    }
                    let nat = if draw_nat_w > 0 && draw_nat_h > 0 {
                        [draw_nat_w, draw_nat_h]
                    } else {
                        [dimg.width().max(1), dimg.height().max(1)]
                    };
                    let width_in = if draw_cx > 0 {
                        (draw_cx as f32 / 914_400.0).clamp(0.05, 40.0)
                    } else {
                        nat[0] as f32 / 96.0
                    };
                    let height_in = if draw_cy > 0 {
                        (draw_cy as f32 / 914_400.0).clamp(0.05, 40.0)
                    } else {
                        nat[1] as f32 / 96.0
                    };
                    // keep non-PNG/JPEG formats displayable by re-encoding them as PNG
                    let bytes = if img_bytes.starts_with(&[0x89, b'P', b'N', b'G'])
                        || img_bytes.starts_with(&[0xFF, 0xD8])
                    {
                        img_bytes.clone()
                    } else {
                        let mut out = std::io::Cursor::new(Vec::new());
                        if dimg.write_to(&mut out, image::ImageFormat::Png).is_err() {
                            continue;
                        }
                        out.into_inner()
                    };
                    doc.images.push(DocImage {
                        id,
                        name: if draw_name.is_empty() {
                            format!("Image {id}")
                        } else {
                            draw_name.clone()
                        },
                        natural_size: nat,
                        width_in,
                        height_in,
                        caption: draw_caption.clone(),
                        png_b64: base64_encode(&bytes),
                        seamless: draw_seamless,
                    });
                    // An image is its own block: split text before it into a separate paragraph.
                    if !p.text.is_empty() || p.fmt.image_id > 0 {
                        let fmt = ParaFmt {
                            image_id: 0,
                            page_break_before: false,
                            ..p.fmt
                        };
                        let done = std::mem::replace(p, Para::new(run, fmt));
                        push_para(&mut doc, done, &style_id, false, &mut last_meta);
                        if let Some(tb) = cur_tbl.as_mut() {
                            tb.cur_cell_para_count += 1;
                        }
                    }
                    if let Some(p) = para.as_mut() {
                        p.fmt.image_id = id;
                        p.fmt.list = ListKind::None;
                    }
                }
                "w:hyperlink" => cur_link = 0,
                "wp:posOffset" => in_pos_offset = false,
                "wp:positionH" | "wp:positionV" => pos_axis = None,
                "wp:anchor" => anchor = None,
                "w:rPr" => in_rpr = false,
                "w:r" => {
                    in_run = false;
                    run_hidden = false;
                }
                "w:t" => in_text = false,
                "w:p" => {
                    if let Some(mut p) = para.take() {
                        if !ppr_seen {
                            resolve_list(
                                &mut doc.list_defs,
                                &numbering,
                                &mut list_ids,
                                &mut p,
                                &pex,
                                None,
                                None,
                            );
                        }
                        push_para(
                            &mut doc,
                            p,
                            &style_id,
                            pex.contextual.unwrap_or(false),
                            &mut last_meta,
                        );
                    }
                }
                _ => {}
            },
            Tok::Text(s) if in_pos_offset => {
                if let (Some(an), Ok(v)) = (anchor.as_mut(), s.trim().parse::<f32>()) {
                    if pos_axis == Some('h') {
                        an.1 = v / 12_700.0;
                    } else {
                        an.3 = v / 12_700.0;
                    }
                }
            }
            Tok::Text(s) => {
                if in_text && in_run && !run_hidden {
                    // text after an image stays in the image's paragraph (shown under it)
                    if let Some(p) = para.as_mut() {
                        for c in s.chars() {
                            p.text.push(c);
                            p.styles.push(run);
                        }
                    }
                }
            }
        }
    }
    if let Some(p) = para.take() {
        doc.paras.push(p);
    }
    if let Some(tb) = cur_tbl.take() {
        doc.tables.push(tb.finish());
    }
    // Floating text boxes (cover pages, letterheads) keep their anchored position.
    let pg = doc.page;
    let content_w_pt = (pg.width - pg.margin_left - pg.margin_right) * 72.0;
    for (start, end, (rel_h, off_h, rel_v, off_v), w_pt) in tb_fixups {
        let left_pt = match rel_h.as_str() {
            "page" | "leftMargin" | "insideMargin" => off_h - pg.margin_left * 72.0,
            "margin" | "column" | "character" => off_h,
            _ => 0.0,
        }
        .max(0.0);
        let right_pt = if w_pt > 0.0 {
            (content_w_pt - left_pt - w_pt).max(0.0)
        } else {
            0.0
        };
        for p in &mut doc.paras[start..end] {
            if p.fmt.table_id == 0 && left_pt + right_pt < content_w_pt - 36.0 {
                p.fmt.left_in += left_pt / 72.0;
                p.fmt.right_in += right_pt / 72.0;
            }
        }
        // vertical offset applies when nothing visible precedes the box on its page
        let top_pt = match rel_v.as_str() {
            "page" | "topMargin" => off_v - pg.margin_top * 72.0,
            "margin" => off_v,
            _ => -1.0,
        };
        let page_start = doc.paras[..start]
            .iter()
            .rposition(|q| q.fmt.page_break_before)
            .unwrap_or(0);
        let blank_before = doc.paras[page_start..start]
            .iter()
            .all(|q| q.text.iter().all(|c| c.is_whitespace()) && q.fmt.image_id == 0);
        if top_pt > 0.0 && blank_before {
            let used: f32 = doc.paras[page_start..start]
                .iter()
                .map(|q| q.fmt.space_before + q.fmt.space_after + q.end_style.size * 1.2)
                .sum();
            doc.paras[start].fmt.space_before =
                (top_pt - used).max(doc.paras[start].fmt.space_before);
        }
    }
    // Footnotes / endnotes are listed after the body, like a printed page's notes.
    if !notes_out.is_empty() {
        let note_style = CharStyle {
            size: (base.size - 2.0).max(7.0),
            ..base
        };
        let mut sep = Para::new(
            note_style,
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
        for (k, body) in notes_out {
            let mut p = Para::new(
                note_style,
                ParaFmt {
                    space_after: 2.0,
                    ..ParaFmt::default()
                },
            );
            for c in k.to_string().chars() {
                p.text.push(c);
                p.styles.push(CharStyle {
                    script: 1,
                    ..note_style
                });
            }
            p.text.push(' ');
            p.styles.push(note_style);
            for c in body.chars() {
                p.text.push(c);
                p.styles.push(note_style);
            }
            doc.paras.push(p);
        }
    }
    if doc.paras.is_empty() {
        doc.paras.push(Para::new(CharStyle::default(), ParaFmt::default()));
    }
    if !(1.0..=40.0).contains(&doc.page.width) || !(1.0..=40.0).contains(&doc.page.height) {
        doc.page = PageSetup::default();
    }
    if let Some(core) = files.get("docProps/core.xml") {
        let core = String::from_utf8_lossy(core);
        let mut cur = String::new();
        for t in tokens(&core) {
            match t {
                Tok::Open(n, _, _) => cur = n,
                Tok::Text(s) => match cur.as_str() {
                    "dc:title" => doc.info.title = s,
                    "dc:creator" => doc.info.author = s,
                    "dc:subject" => doc.info.subject = s,
                    "cp:keywords" => doc.info.keywords = s,
                    "dc:description" => doc.info.notes = s,
                    _ => {}
                },
                Tok::Close(_) => cur.clear(),
            }
        }
    }
    Ok(doc)
}

/// Turns a paragraph's numbering (`w:numPr`, direct or from its style) into list formatting.
fn resolve_list(
    defs: &mut Vec<ListDef>,
    numbering: &Numbering,
    list_ids: &mut HashMap<String, u16>,
    p: &mut Para,
    pex: &PExtra,
    direct_ind: Option<&Attrs>,
    legacy: Option<&(String, u8)>,
) {
    let Some(num_id) = pex.num_id.as_ref() else {
        return;
    };
    let lvl = pex.ilvl.unwrap_or(0).min(8);
    if num_id == "0" {
        p.fmt.list = ListKind::None;
        return;
    }
    // Lists written by Syllabary carry their editor look; restore it exactly.
    if let Some((kind, preset)) = legacy {
        p.fmt.list_id = 0;
        p.fmt.indent = lvl;
        p.fmt.bullet_preset = *preset;
        p.fmt.left_in = 0.0;
        p.fmt.first_in = 0.0;
        p.fmt.list = match kind.as_str() {
            "number" => ListKind::Number,
            "check" => {
                p.fmt.checked = false;
                ListKind::Checklist
            }
            "checked" => {
                p.fmt.checked = true;
                ListKind::Checklist
            }
            "legacy" => match num_id.as_str() {
                "2" => ListKind::Number,
                "3" => ListKind::Checklist,
                "4" => {
                    p.fmt.checked = true;
                    ListKind::Checklist
                }
                _ => ListKind::Bullet,
            },
            _ => ListKind::Bullet,
        };
        return;
    }
    let Some(levels) = numbering.levels(num_id) else {
        return;
    };
    let id = match list_ids.get(num_id) {
        Some(&id) => id,
        None => {
            let id = defs.len() as u16 + 1;
            defs.push(ListDef {
                id,
                levels: levels.clone(),
            });
            list_ids.insert(num_id.clone(), id);
            id
        }
    };
    let lv = levels
        .get(lvl as usize)
        .or_else(|| levels.last())
        .cloned()
        .unwrap_or_default();
    if lv.fmt == NumFmt::None && lv.text.trim().is_empty() {
        p.fmt.list = ListKind::None;
    } else {
        p.fmt.list = if lv.fmt == NumFmt::Bullet {
            ListKind::Bullet
        } else {
            ListKind::Number
        };
    }
    p.fmt.list_id = id;
    p.fmt.indent = lvl;
    // Numbering indents beat style indents; direct paragraph indents beat both.
    p.fmt.left_in = 0.0;
    p.fmt.first_in = 0.0;
    if let Some(a) = direct_ind {
        p.fmt.left_in = lv.left_in;
        p.fmt.first_in = lv.first_in;
        let mut ex = PExtra::default();
        apply_ppr(&mut p.fmt, &mut ex, "w:ind", a, false);
        if p.fmt.left_in == 0.0 && p.fmt.first_in == 0.0 {
            // explicit zero indents: keep them explicit with a hairline so the level's are not used
            p.fmt.left_in = 0.0001;
        }
    }
    if p.fmt.list == ListKind::None {
        // "no number" levels still indent like the list
        if direct_ind.is_none() {
            p.fmt.left_in = lv.left_in;
            p.fmt.first_in = lv.first_in;
        }
        p.fmt.list_id = 0;
        p.fmt.indent = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn docx_roundtrip() {
        let mut d = Document::default();
        d.insert_text(Pos::new(0, 0), "Title here\nBody & <text>\tend", CharStyle { bold: true, size: 14.0, color: [200, 0, 0], ..Default::default() });
        d.paras[1].fmt.list = ListKind::Number;
        d.paras[1].fmt.align = Align::Center;
        d.paras[0].fmt.style = 4;
        d.info.title = "My doc".into();
        let bytes = write(&d);
        let back = read(&bytes).unwrap();
        assert_eq!(back.plain_text(), d.plain_text());
        assert!(back.paras[0].styles[0].bold);
        assert_eq!(back.paras[0].styles[0].size, 14.0);
        assert_eq!(back.paras[0].styles[0].color, [200, 0, 0]);
        assert_eq!(back.paras[1].fmt.list, ListKind::Number);
        assert_eq!(back.paras[1].fmt.align, Align::Center);
        assert_eq!(back.paras[0].fmt.style, 4);
        assert_eq!(back.info.title, "My doc");
    }

    #[test]
    fn docx_preserves_theme_normal_and_system_fonts() {
        let theme = br#"<?xml version="1.0" encoding="UTF-8"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <a:themeElements>
    <a:fontScheme name="Custom">
      <a:majorFont><a:latin typeface="Cambria"/></a:majorFont>
      <a:minorFont><a:latin typeface="Georgia"/></a:minorFont>
    </a:fontScheme>
  </a:themeElements>
</a:theme>"#;
        let styles = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:docDefaults>
    <w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi" w:hAnsiTheme="minorHAnsi"/><w:sz w:val="22"/></w:rPr></w:rPrDefault>
  </w:docDefaults>
  <w:style w:type="paragraph" w:default="1" w:styleId="Normal">
    <w:name w:val="Normal"/>
    <w:rPr><w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman"/><w:sz w:val="24"/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/>
    <w:basedOn w:val="Normal"/>
    <w:rPr><w:rFonts w:asciiTheme="majorHAnsi" w:hAnsiTheme="majorHAnsi"/><w:sz w:val="32"/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Title">
    <w:name w:val="Title"/>
    <w:basedOn w:val="Heading1"/>
    <w:rPr><w:b/></w:rPr>
  </w:style>
</w:styles>"#;
        let document = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Heading Title</w:t></w:r></w:p>
    <w:p><w:r><w:rPr><w:rFonts w:cs="Arial"/></w:rPr><w:t>Body in Normal</w:t></w:r></w:p>
    <w:p><w:r><w:rPr><w:rFonts w:asciiTheme="minorHAnsi"/></w:rPr><w:t>Theme Minor</w:t></w:r></w:p>
  </w:body>
</w:document>"#;
        let z = zip(&[
            ("word/theme/theme1.xml", theme.to_vec()),
            ("word/styles.xml", styles.to_vec()),
            ("word/document.xml", document.to_vec()),
        ]);
        let doc = read(&z).unwrap();
        // Title inherits majorHAnsi (Cambria) and 16pt from Heading1, plus bold from Title
        assert_eq!(font_name(doc.paras[0].styles[0].font), "Cambria");
        assert_eq!(doc.paras[0].styles[0].size, 16.0);
        assert!(doc.paras[0].styles[0].bold);
        // Normal paragraph inherits Times New Roman 12pt from Normal (and w:cs="Arial" does not clobber Latin font)
        assert_eq!(font_name(doc.paras[1].styles[0].font), "Times New Roman");
        assert_eq!(doc.paras[1].styles[0].size, 12.0);
        // Explicit minorHAnsi resolves to Georgia from theme1.xml
        assert_eq!(font_name(doc.paras[2].styles[0].font), "Georgia");
    }

    #[test]
    fn header_footer_style_and_distance_roundtrip() {
        let mut d = Document::default();
        d.insert_text(Pos::new(0, 0), "Body", CharStyle::default());
        d.header = "Quarterly Report".into();
        d.footer = "Page 1 of 1".into();
        d.header_style = Some(CharStyle {
            font: font_index("Georgia"),
            size: 14.0,
            bold: true,
            color: [20, 60, 120],
            ..CharStyle::default()
        });
        d.footer_style = Some(CharStyle {
            font: font_index("Arial"),
            size: 8.0,
            color: [90, 90, 90],
            ..CharStyle::default()
        });
        d.header_dist_in = 0.4;
        d.footer_dist_in = 0.3;
        for back in [
            read(&write(&d)).unwrap(),
            crate::rtf::from_rtf(&crate::rtf::to_rtf(&d)),
        ] {
            assert_eq!(back.header, "Quarterly Report");
            let hs = back.header_style.expect("header style");
            assert_eq!(font_name(hs.font), "Georgia");
            assert_eq!(hs.size, 14.0);
            assert!(hs.bold);
            assert_eq!(hs.color, [20, 60, 120]);
            let fs = back.footer_style.expect("footer style");
            assert_eq!(font_name(fs.font), "Arial");
            assert_eq!(fs.size, 8.0);
            assert!((back.header_dist_in - 0.4).abs() < 0.01);
            assert!((back.footer_dist_in - 0.3).abs() < 0.01);
        }
    }
}

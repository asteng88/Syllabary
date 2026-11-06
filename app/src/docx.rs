//! Word (.docx) files: a small writer and reader for text, character formatting,
//! alignment, spacing, indents, lists, headings, page size and margins.
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
        Some("Title") => Some("Title"),
        Some("Subtitle") => Some("Subtitle"),
        Some("Heading 1") => Some("Heading1"),
        Some("Heading 2") => Some("Heading2"),
        Some("Heading 3") => Some("Heading3"),
        Some("Quote") => Some("Quote"),
        _ => None,
    }
}

fn run_props(s: &CharStyle) -> String {
    let f = esc(FONT_NAMES[s.font as usize % FONT_NAMES.len()]);
    let mut r = format!("<w:rPr><w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:cs=\"{f}\"/>");
    if s.bold {
        r.push_str("<w:b/>");
    }
    if s.italic {
        r.push_str("<w:i/>");
    }
    if s.strike {
        r.push_str("<w:strike/>");
    }
    if s.color != [0, 0, 0] {
        r.push_str(&format!("<w:color w:val=\"{}\"/>", hex(s.color)));
    }
    r.push_str(&format!("<w:sz w:val=\"{0}\"/><w:szCs w:val=\"{0}\"/>", (s.size * 2.0).round() as i32));
    if let Some(h) = s.highlight {
        r.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(h)));
    }
    if s.underline {
        r.push_str("<w:u w:val=\"single\"/>");
    }
    if s.script > 0 {
        r.push_str("<w:vertAlign w:val=\"superscript\"/>");
    } else if s.script < 0 {
        r.push_str("<w:vertAlign w:val=\"subscript\"/>");
    }
    r.push_str("</w:rPr>");
    r
}

pub fn write(doc: &Document) -> Vec<u8> {
    let mut body = String::new();
    for p in &doc.paras {
        body.push_str("<w:p><w:pPr>");
        if let Some(id) = style_id(p.fmt.style) {
            body.push_str(&format!("<w:pStyle w:val=\"{id}\"/>"));
        }
        if p.fmt.page_break_before {
            body.push_str("<w:pageBreakBefore/>");
        }
        match p.fmt.list {
            ListKind::Bullet => body.push_str(&format!("<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"1\"/></w:numPr>", p.fmt.indent.min(8))),
            ListKind::Number => body.push_str(&format!("<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"2\"/></w:numPr>", p.fmt.indent.min(8))),
            ListKind::None => {}
        }
        body.push_str(&format!(
            "<w:spacing w:before=\"{}\" w:after=\"{}\" w:line=\"{}\" w:lineRule=\"auto\"/>",
            (p.fmt.space_before * 20.0).round() as i32,
            (p.fmt.space_after * 20.0).round() as i32,
            (p.fmt.line_spacing * 240.0).round() as i32
        ));
        if p.fmt.list == ListKind::None && p.fmt.indent > 0 {
            body.push_str(&format!("<w:ind w:left=\"{}\"/>", p.fmt.indent as i32 * 720));
        }
        let jc = match p.fmt.align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "both",
        };
        body.push_str(&format!("<w:jc w:val=\"{jc}\"/>{}</w:pPr>", run_props(&p.end_style)));
        for (a, b, s) in p.runs() {
            body.push_str("<w:r>");
            body.push_str(&run_props(&s));
            let text: String = p.text[a..b].iter().collect();
            for (i, part) in text.split('\t').enumerate() {
                if i > 0 {
                    body.push_str("<w:tab/>");
                }
                if !part.is_empty() {
                    body.push_str(&format!("<w:t xml:space=\"preserve\">{}</w:t>", esc(part)));
                }
            }
            body.push_str("</w:r>");
        }
        body.push_str("</w:p>");
    }
    let pg = doc.page;
    let tw = |v: f32| (v * 1440.0).round() as i32;
    let orient = if pg.width > pg.height { " w:orient=\"landscape\"" } else { "" };
    body.push_str(&format!(
        "<w:sectPr><w:pgSz w:w=\"{}\" w:h=\"{}\"{orient}/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr>",
        tw(pg.width),
        tw(pg.height),
        tw(pg.margin_top),
        tw(pg.margin_right),
        tw(pg.margin_bottom),
        tw(pg.margin_left)
    ));
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>"
    );

    let mut styles = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Times New Roman\" w:hAnsi=\"Times New Roman\"/><w:sz w:val=\"24\"/></w:rPr></w:rPrDefault></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>");
    for (i, ns) in named_styles().iter().enumerate() {
        if let Some(id) = style_id(i) {
            let outline = match id {
                "Heading1" => "<w:outlineLvl w:val=\"0\"/>",
                "Heading2" => "<w:outlineLvl w:val=\"1\"/>",
                "Heading3" => "<w:outlineLvl w:val=\"2\"/>",
                _ => "",
            };
            let name = if id.starts_with("Heading") { ns.name.to_lowercase() } else { ns.name.to_string() };
            styles.push_str(&format!(
                "<w:style w:type=\"paragraph\" w:styleId=\"{id}\"><w:name w:val=\"{name}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/>{outline}</w:pPr>{}</w:style>",
                run_props(&ns.chr)
            ));
        }
    }
    styles.push_str("</w:styles>");

    let mut numbering = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">");
    for (aid, bullet) in [(0, true), (1, false)] {
        numbering.push_str(&format!("<w:abstractNum w:abstractNumId=\"{aid}\">"));
        for lvl in 0..9 {
            let (fmt, text) = if bullet { ("bullet", ["•", "◦", "▪"][lvl % 3].to_string()) } else { ("decimal", format!("%{}.", lvl + 1)) };
            numbering.push_str(&format!(
                "<w:lvl w:ilvl=\"{lvl}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{fmt}\"/><w:lvlText w:val=\"{text}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
                720 * (lvl + 1) - 360
            ));
        }
        numbering.push_str("</w:abstractNum>");
    }
    numbering.push_str("<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num><w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num></w:numbering>");

    let core = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:creator>{}</dc:creator><dc:subject>{}</dc:subject><cp:keywords>{}</cp:keywords></cp:coreProperties>",
        esc(&doc.info.title),
        esc(&doc.info.author),
        esc(&doc.info.subject),
        esc(&doc.info.keywords)
    );
    let types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>";
    let doc_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/></Relationships>";
    zip(&[
        ("[Content_Types].xml", types.as_bytes().to_vec()),
        ("_rels/.rels", rels.as_bytes().to_vec()),
        ("word/document.xml", document.into_bytes()),
        ("word/_rels/document.xml.rels", doc_rels.as_bytes().to_vec()),
        ("word/styles.xml", styles.into_bytes()),
        ("word/numbering.xml", numbering.into_bytes()),
        ("docProps/core.xml", core.into_bytes()),
    ])
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
            let end = xml[i..].find('<').map(|e| i + e).unwrap_or(b.len());
            out.push(Tok::Text(unescape(&xml[i..end])));
            i = end;
        }
    }
    out
}

fn on(a: &HashMap<String, String>) -> bool {
    !matches!(a.get("w:val").map(|s| s.as_str()), Some("0" | "false" | "none" | "off"))
}

fn highlight_color(name: &str) -> Option<[u8; 3]> {
    Some(match name {
        "yellow" => [254, 240, 138],
        "green" | "brightGreen" => [187, 247, 208],
        "cyan" | "turquoise" => [191, 219, 254],
        "magenta" | "pink" => [251, 207, 232],
        "red" => [254, 202, 202],
        "lightGray" | "darkGray" => [229, 231, 235],
        "none" => return None,
        _ => [254, 240, 138],
    })
}

fn parse_hex(v: &str) -> Option<[u8; 3]> {
    if v.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(v, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

fn apply_rpr(s: &mut CharStyle, name: &str, a: &HashMap<String, String>) {
    match name {
        "w:b" => s.bold = on(a),
        "w:i" => s.italic = on(a),
        "w:u" => s.underline = on(a),
        "w:strike" | "w:dstrike" => s.strike = on(a),
        "w:sz" => {
            if let Some(v) = a.get("w:val").and_then(|v| v.parse::<f32>().ok()) {
                s.size = (v / 2.0).clamp(MIN_SIZE, MAX_SIZE);
            }
        }
        "w:rFonts" => {
            if let Some(f) = a.get("w:ascii").or(a.get("w:hAnsi")) {
                s.font = font_index(f);
            } else if a.contains_key("w:asciiTheme") {
                s.font = 2; // theme body font → Calibri
            }
        }
        "w:color" => {
            if let Some(c) = a.get("w:val").and_then(|v| parse_hex(v)) {
                s.color = c;
            }
        }
        "w:highlight" => s.highlight = a.get("w:val").and_then(|v| highlight_color(v)),
        "w:shd" => {
            if let Some(c) = a.get("w:fill").and_then(|v| parse_hex(v)) {
                if c != [255, 255, 255] {
                    s.highlight = Some(c);
                }
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

fn style_index(id: &str) -> usize {
    let want = match id.to_ascii_lowercase().replace(' ', "").as_str() {
        "title" => "Title",
        "subtitle" => "Subtitle",
        "heading1" => "Heading 1",
        "heading2" => "Heading 2",
        "heading3" | "heading4" | "heading5" | "heading6" => "Heading 3",
        "quote" | "intensequote" => "Quote",
        _ => return 0,
    };
    named_styles().iter().position(|s| s.name == want).unwrap_or(0)
}

pub fn read(data: &[u8]) -> Result<Document, String> {
    let files = unzip(data)?;
    let xml = files.get("word/document.xml").ok_or("this file has no Word document inside")?;
    let xml = String::from_utf8_lossy(xml);

    // document defaults (font and size)
    let mut base = CharStyle { font: 2, size: 11.0, ..CharStyle::default() };
    if let Some(st) = files.get("word/styles.xml") {
        let st = String::from_utf8_lossy(st);
        let mut in_defaults = false;
        for t in tokens(&st) {
            match t {
                Tok::Open(n, a, _) if in_defaults => apply_rpr(&mut base, &n, &a),
                Tok::Open(n, _, _) if n == "w:rPrDefault" => in_defaults = true,
                Tok::Close(n) if n == "w:rPrDefault" => in_defaults = false,
                _ => {}
            }
        }
    }
    // which list ids are bullets
    let mut bullet_num: HashMap<String, bool> = HashMap::new();
    if let Some(nb) = files.get("word/numbering.xml") {
        let nb = String::from_utf8_lossy(nb);
        let mut abs_bullet: HashMap<String, bool> = HashMap::new();
        let mut cur_abs = String::new();
        let mut cur_lvl0 = false;
        let mut cur_num = String::new();
        for t in tokens(&nb) {
            match t {
                Tok::Open(n, a, _) if n == "w:abstractNum" => cur_abs = a.get("w:abstractNumId").cloned().unwrap_or_default(),
                Tok::Open(n, a, _) if n == "w:lvl" => cur_lvl0 = a.get("w:ilvl").map(|v| v == "0").unwrap_or(false),
                Tok::Open(n, a, _) if n == "w:numFmt" && cur_lvl0 => {
                    abs_bullet.insert(cur_abs.clone(), a.get("w:val").map(|v| v == "bullet").unwrap_or(false));
                }
                Tok::Open(n, a, _) if n == "w:num" => cur_num = a.get("w:numId").cloned().unwrap_or_default(),
                Tok::Open(n, a, _) if n == "w:abstractNumId" => {
                    let b = a.get("w:val").and_then(|v| abs_bullet.get(v)).copied().unwrap_or(true);
                    bullet_num.insert(cur_num.clone(), b);
                }
                _ => {}
            }
        }
    }

    let mut doc = Document { paras: Vec::new(), ..Default::default() };
    let styles = named_styles();
    let mut para: Option<Para> = None;
    let mut in_ppr = false;
    let mut in_rpr = false;
    let mut in_run = false;
    let mut in_text = false;
    let mut para_base = base;
    let mut mark = base;
    let mut run = base;
    let mut list_num: Option<String> = None;
    let mut list_lvl = 0u8;
    let mut pending_break = false;
    let mut in_sect = false;

    for t in tokens(&xml) {
        match t {
            Tok::Open(n, a, selfclose) => match n.as_str() {
                "w:p" => {
                    let mut p = Para::new(base, ParaFmt { space_after: 0.0, ..ParaFmt::default() });
                    p.fmt.page_break_before = std::mem::take(&mut pending_break);
                    para = Some(p);
                    para_base = base;
                    mark = base;
                    list_num = None;
                    list_lvl = 0;
                    if selfclose {
                        if let Some(p) = para.take() {
                            doc.paras.push(p);
                        }
                    }
                }
                "w:pPr" => in_ppr = !selfclose,
                "w:rPr" => in_rpr = !selfclose,
                "w:r" => {
                    in_run = !selfclose;
                    run = para_base;
                }
                "w:t" => in_text = !selfclose,
                "w:sectPr" => in_sect = true,
                "w:pgSz" if in_sect => {
                    let w = a.get("w:w").and_then(|v| v.parse::<f32>().ok());
                    let h = a.get("w:h").and_then(|v| v.parse::<f32>().ok());
                    if let (Some(w), Some(h)) = (w, h) {
                        doc.page.width = w / 1440.0;
                        doc.page.height = h / 1440.0;
                    }
                }
                "w:pgMar" if in_sect => {
                    let g = |k: &str| a.get(k).and_then(|v| v.parse::<f32>().ok()).map(|v| v / 1440.0);
                    if let Some(v) = g("w:top") {
                        doc.page.margin_top = v.abs();
                    }
                    if let Some(v) = g("w:bottom") {
                        doc.page.margin_bottom = v.abs();
                    }
                    if let Some(v) = g("w:left") {
                        doc.page.margin_left = v;
                    }
                    if let Some(v) = g("w:right") {
                        doc.page.margin_right = v;
                    }
                }
                _ if in_rpr && in_run => apply_rpr(&mut run, &n, &a),
                _ if in_rpr && in_ppr => apply_rpr(&mut mark, &n, &a),
                _ if in_ppr => {
                    let Some(p) = para.as_mut() else { continue };
                    match n.as_str() {
                        "w:pStyle" => {
                            let idx = a.get("w:val").map(|v| style_index(v)).unwrap_or(0);
                            if idx != 0 {
                                p.fmt = ParaFmt { style: idx, page_break_before: p.fmt.page_break_before, ..styles[idx].fmt };
                                para_base = styles[idx].chr;
                                mark = styles[idx].chr;
                            } else if a.get("w:val").map(|v| v.to_lowercase().contains("list")).unwrap_or(false) {
                                p.fmt.list = ListKind::Bullet;
                            }
                        }
                        "w:jc" => {
                            p.fmt.align = match a.get("w:val").map(|s| s.as_str()) {
                                Some("center") => Align::Center,
                                Some("right") | Some("end") => Align::Right,
                                Some("both") | Some("distribute") => Align::Justify,
                                _ => Align::Left,
                            }
                        }
                        "w:spacing" => {
                            if let Some(v) = a.get("w:before").and_then(|v| v.parse::<f32>().ok()) {
                                p.fmt.space_before = (v / 20.0).clamp(0.0, 72.0);
                            }
                            if let Some(v) = a.get("w:after").and_then(|v| v.parse::<f32>().ok()) {
                                p.fmt.space_after = (v / 20.0).clamp(0.0, 72.0);
                            }
                            if a.get("w:lineRule").map(|r| r == "auto").unwrap_or(true) {
                                if let Some(v) = a.get("w:line").and_then(|v| v.parse::<f32>().ok()) {
                                    p.fmt.line_spacing = (v / 240.0).clamp(1.0, 3.0);
                                }
                            }
                        }
                        "w:ind" => {
                            if let Some(v) = a.get("w:left").or(a.get("w:start")).and_then(|v| v.parse::<f32>().ok()) {
                                p.fmt.indent = (v / 720.0).round().clamp(0.0, 8.0) as u8;
                            }
                        }
                        "w:pageBreakBefore" => p.fmt.page_break_before = on(&a),
                        "w:ilvl" => list_lvl = a.get("w:val").and_then(|v| v.parse().ok()).unwrap_or(0),
                        "w:numId" => list_num = a.get("w:val").cloned(),
                        _ => {}
                    }
                }
                "w:tab" if in_run => {
                    if let Some(p) = para.as_mut() {
                        p.text.push('\t');
                        p.styles.push(run);
                    }
                }
                "w:br" | "w:cr" if in_run => {
                    if a.get("w:type").map(|t| t == "page").unwrap_or(false) {
                        if let Some(p) = para.take() {
                            doc.paras.push(p);
                        }
                        let mut np = Para::new(para_base, ParaFmt::default());
                        np.fmt.page_break_before = true;
                        para = Some(np);
                    } else if let Some(p) = para.as_mut() {
                        // a line break inside a paragraph becomes a new paragraph
                        let fmt = p.fmt;
                        let done = std::mem::replace(p, Para::new(run, ParaFmt { page_break_before: false, ..fmt }));
                        doc.paras.push(done);
                    }
                }
                _ => {}
            },
            Tok::Close(n) => match n.as_str() {
                "w:pPr" => {
                    in_ppr = false;
                    if let (Some(p), Some(id)) = (para.as_mut(), list_num.as_ref()) {
                        if id != "0" {
                            p.fmt.list = if *bullet_num.get(id).unwrap_or(&true) { ListKind::Bullet } else { ListKind::Number };
                            p.fmt.indent = list_lvl.min(8);
                        }
                    }
                    if let Some(p) = para.as_mut() {
                        p.end_style = mark;
                    }
                }
                "w:rPr" => in_rpr = false,
                "w:r" => in_run = false,
                "w:t" => in_text = false,
                "w:sectPr" => in_sect = false,
                "w:p" => {
                    if let Some(p) = para.take() {
                        doc.paras.push(p);
                    }
                }
                _ => {}
            },
            Tok::Text(s) => {
                if in_text && in_run {
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
    if doc.paras.is_empty() {
        doc.paras.push(Para::new(CharStyle::default(), ParaFmt::default()));
    }
    if !(1.0..=40.0).contains(&doc.page.width) || !(1.0..=40.0).contains(&doc.page.height) {
        doc.page = PageSetup::default();
    }
    // document properties
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
                    _ => {}
                },
                Tok::Close(_) => cur.clear(),
            }
        }
    }
    Ok(doc)
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
}

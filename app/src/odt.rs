//! OpenDocument Text (`.odt`, ISO/IEC 26300 / ODF 1.2) reader and writer.
//!
//! Supports LibreOffice Writer, OpenOffice, Google Docs, and Microsoft Word `.odt` files:
//! - Headings (`text:h`), paragraphs (`text:p`), inline spans (`text:span`), links (`text:a`)
//! - Bold, italic, underline, strikethrough, font family, font size, text color, highlight, alignment
//! - Bullet & numbered lists (`text:list` / `text:list-item`)
//! - Tables (`table:table` / `table:table-row` / `table:table-cell`)
//! - Embedded images (`draw:image` / `Pictures/...`)
//! - Page setup dimensions & margins, headers/footers (`styles.xml`), and metadata (`meta.xml`)

use std::collections::HashMap;

use crate::doc::{
    font_index, font_name, Align, CharStyle, DocImage, DocTable, Document, ListKind, PageSetup,
    Para, ParaFmt, TableCellSpec, TableRowSpec, DEFAULT_FONT,
};

// ── Minimal ZIP Reader & Writer (ODF-compliant: uncompressed `mimetype` first) ──

fn read_u16_le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn read_u32_le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn unzip_all(data: &[u8]) -> Result<HashMap<String, Vec<u8>>, String> {
    if data.len() < 22 {
        return Err("File is too small to be a valid .odt archive.".into());
    }
    // Locate End of Central Directory (0x06054b50)
    let search_start = data.len().saturating_sub(65_557);
    let mut eocd = None;
    for i in (search_start..=data.len() - 22).rev() {
        if read_u32_le(data, i) == 0x0605_4b50 {
            eocd = Some(i);
            break;
        }
    }
    let eocd = eocd.ok_or("Missing ZIP end-of-central-directory in .odt file.")?;
    let cd_size = read_u32_le(data, eocd + 12) as usize;
    let cd_off = read_u32_le(data, eocd + 16) as usize;
    if cd_off + cd_size > data.len() {
        return Err("Corrupt ZIP central directory in .odt file.".into());
    }

    let mut files = HashMap::new();
    let mut pos = cd_off;
    let end = cd_off + cd_size;
    while pos + 46 <= end && read_u32_le(data, pos) == 0x0201_4b50 {
        let method = read_u16_le(data, pos + 10);
        let comp_size = read_u32_le(data, pos + 20) as usize;
        let name_len = read_u16_le(data, pos + 28) as usize;
        let extra_len = read_u16_le(data, pos + 30) as usize;
        let comment_len = read_u16_le(data, pos + 32) as usize;
        let lfh_off = read_u32_le(data, pos + 42) as usize;
        if pos + 46 + name_len > data.len() {
            break;
        }
        let name = String::from_utf8_lossy(&data[pos + 46..pos + 46 + name_len]).to_string();
        pos += 46 + name_len + extra_len + comment_len;

        if name.ends_with('/') || lfh_off + 30 > data.len() || read_u32_le(data, lfh_off) != 0x0403_4b50 {
            continue;
        }
        let l_name_len = read_u16_le(data, lfh_off + 26) as usize;
        let l_extra_len = read_u16_le(data, lfh_off + 28) as usize;
        let data_start = lfh_off + 30 + l_name_len + l_extra_len;
        if data_start + comp_size > data.len() {
            continue;
        }
        let raw = &data[data_start..data_start + comp_size];
        let content = match method {
            0 => raw.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec(raw)
                .map_err(|e| format!("Failed to decompress {name}: {e:?}"))?,
            _ => continue,
        };
        files.insert(name, content);
    }
    Ok(files)
}

struct ZipEntry {
    name: String,
    data: Vec<u8>,
    store_uncompressed: bool,
}

fn build_zip(entries: &[ZipEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();

    for e in entries {
        let crc = crc32fast::hash(&e.data);
        let (method, payload): (u16, Vec<u8>) = if e.store_uncompressed {
            (0, e.data.clone())
        } else {
            (8, miniz_oxide::deflate::compress_to_vec(&e.data, 6))
        };
        let lfh_off = out.len() as u32;
        let name_b = e.name.as_bytes();

        // Local file header
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&(e.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name_b);
        out.extend_from_slice(&payload);

        // Central directory header
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        central.extend_from_slice(&(e.data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&lfh_off.to_le_bytes());
        central.extend_from_slice(name_b);
    }

    let cd_off = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);

    // End of central directory
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

// ── Lightweight XML DOM Parser & Escaper ────────────────────────────────

#[derive(Clone, Debug)]
struct XmlNode {
    tag: String,
    attrs: HashMap<String, String>,
    children: Vec<XmlChild>,
}

#[derive(Clone, Debug)]
enum XmlChild {
    Elem(XmlNode),
    Text(String),
}

impl XmlNode {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(|s| s.as_str())
    }

    fn find(&self, tag: &str) -> Option<&XmlNode> {
        for ch in &self.children {
            if let XmlChild::Elem(n) = ch {
                if n.tag == tag {
                    return Some(n);
                }
            }
        }
        None
    }

    fn find_deep(&self, tag: &str) -> Option<&XmlNode> {
        if self.tag == tag {
            return Some(self);
        }
        for ch in &self.children {
            if let XmlChild::Elem(n) = ch {
                if let Some(hit) = n.find_deep(tag) {
                    return Some(hit);
                }
            }
        }
        None
    }

    fn text_content(&self) -> String {
        let mut out = String::new();
        for ch in &self.children {
            match ch {
                XmlChild::Text(t) => out.push_str(t),
                XmlChild::Elem(n) => {
                    if n.tag == "text:s" {
                        let count = n.attr("text:c").and_then(|c| c.parse::<usize>().ok()).unwrap_or(1);
                        for _ in 0..count {
                            out.push(' ');
                        }
                    } else if n.tag == "text:tab" {
                        out.push('\t');
                    } else if n.tag == "text:line-break" {
                        out.push('\n');
                    } else {
                        out.push_str(&n.text_content());
                    }
                }
            }
        }
        out
    }
}

fn unescape_xml(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some(rel) = s[i..].find(';') {
                let ent = &s[i + 1..i + rel];
                let ch = match ent {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                        u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32)
                    }
                    _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
                    _ => None,
                };
                if let Some(c) = ch {
                    out.push(c);
                    i += rel + 1;
                    continue;
                }
            }
        }
        let c = s[i..].chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

fn parse_xml(src: &str) -> Option<XmlNode> {
    let bytes = src.as_bytes();
    let mut pos = 0usize;
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;

    while pos < bytes.len() {
        if bytes[pos] == b'<' {
            if src[pos..].starts_with("<!--") {
                pos = src[pos..].find("-->").map_or(bytes.len(), |k| pos + k + 3);
                continue;
            }
            if src[pos..].starts_with("<?") {
                pos = src[pos..].find("?>").map_or(bytes.len(), |k| pos + k + 2);
                continue;
            }
            if src[pos..].starts_with("<![CDATA[") {
                if let Some(k) = src[pos..].find("]]>") {
                    let txt = &src[pos + 9..pos + k];
                    if let Some(top) = stack.last_mut() {
                        top.children.push(XmlChild::Text(txt.to_string()));
                    }
                    pos = pos + k + 3;
                    continue;
                }
            }
            if src[pos..].starts_with("<!") {
                pos = src[pos..].find('>').map_or(bytes.len(), |k| pos + k + 1);
                continue;
            }
            if src[pos..].starts_with("</") {
                let end = src[pos..].find('>').map_or(bytes.len(), |k| pos + k);
                if let Some(node) = stack.pop() {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(XmlChild::Elem(node));
                    } else {
                        root = Some(node);
                    }
                }
                pos = (end + 1).min(bytes.len());
                continue;
            }
            // Start tag
            pos += 1;
            let name_start = pos;
            while pos < bytes.len() && !matches!(bytes[pos], b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>') {
                pos += 1;
            }
            let tag = src[name_start..pos].to_string();
            let mut attrs = HashMap::new();
            let mut self_closing = false;
            loop {
                while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t' | b'\r' | b'\n') {
                    pos += 1;
                }
                if pos >= bytes.len() {
                    break;
                }
                if bytes[pos] == b'>' {
                    pos += 1;
                    break;
                }
                if src[pos..].starts_with("/>") {
                    self_closing = true;
                    pos += 2;
                    break;
                }
                let a_start = pos;
                while pos < bytes.len() && !matches!(bytes[pos], b'=' | b' ' | b'\t' | b'\r' | b'\n' | b'>' | b'/') {
                    pos += 1;
                }
                let attr_name = src[a_start..pos].to_string();
                while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t' | b'\r' | b'\n') {
                    pos += 1;
                }
                let mut attr_val = String::new();
                if pos < bytes.len() && bytes[pos] == b'=' {
                    pos += 1;
                    while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t' | b'\r' | b'\n') {
                        pos += 1;
                    }
                    if pos < bytes.len() && (bytes[pos] == b'"' || bytes[pos] == b'\'') {
                        let q = bytes[pos];
                        pos += 1;
                        let v_start = pos;
                        while pos < bytes.len() && bytes[pos] != q {
                            pos += 1;
                        }
                        attr_val = unescape_xml(&src[v_start..pos]);
                        if pos < bytes.len() {
                            pos += 1;
                        }
                    }
                }
                if !attr_name.is_empty() {
                    attrs.insert(attr_name, attr_val);
                }
            }
            let node = XmlNode {
                tag,
                attrs,
                children: Vec::new(),
            };
            if self_closing {
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(XmlChild::Elem(node));
                } else {
                    root = Some(node);
                }
            } else {
                stack.push(node);
            }
        } else {
            let t_start = pos;
            while pos < bytes.len() && bytes[pos] != b'<' {
                pos += 1;
            }
            let txt = unescape_xml(&src[t_start..pos]);
            if !txt.is_empty() {
                if let Some(top) = stack.last_mut() {
                    top.children.push(XmlChild::Text(txt));
                }
            }
        }
    }
    root.or_else(|| stack.into_iter().next())
}

// ── Unit / Color Helpers ────────────────────────────────────────────────

fn parse_length_in(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(v) = s.strip_suffix("in") {
        v.trim().parse::<f32>().ok()
    } else if let Some(v) = s.strip_suffix("cm") {
        v.trim().parse::<f32>().ok().map(|x| x / 2.54)
    } else if let Some(v) = s.strip_suffix("mm") {
        v.trim().parse::<f32>().ok().map(|x| x / 25.4)
    } else if let Some(v) = s.strip_suffix("pt") {
        v.trim().parse::<f32>().ok().map(|x| x / 72.0)
    } else {
        None
    }
}

fn parse_pt(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(v) = s.strip_suffix("pt") {
        v.trim().parse::<f32>().ok()
    } else if let Some(v) = s.strip_suffix("px") {
        v.trim().parse::<f32>().ok().map(|x| x * 0.75)
    } else {
        parse_length_in(s).map(|x| x * 72.0)
    }
}

fn parse_hex_rgb(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some([r, g, b])
}

fn fmt_hex_rgb(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

// ── Style Resolution ────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct OdfStyleDef {
    parent: Option<String>,
    align: Option<Align>,
    space_before: Option<f32>,
    space_after: Option<f32>,
    left_in: Option<f32>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    size: Option<f32>,
    font: Option<u16>,
    color: Option<[u8; 3]>,
    highlight: Option<[u8; 3]>,
    shading: Option<[u8; 3]>,
}

fn collect_styles(node: &XmlNode, map: &mut HashMap<String, OdfStyleDef>) {
    if node.tag == "style:style" {
        if let Some(name) = node.attr("style:name") {
            let mut def = OdfStyleDef {
                parent: node.attr("style:parent-style-name").map(|s| s.to_string()),
                ..Default::default()
            };
            if let Some(pp) = node.find("style:paragraph-properties") {
                if let Some(al) = pp.attr("fo:text-align") {
                    def.align = Some(match al {
                        "center" => Align::Center,
                        "end" | "right" => Align::Right,
                        "justify" => Align::Justify,
                        _ => Align::Left,
                    });
                }
                if let Some(v) = pp.attr("fo:margin-top").and_then(parse_pt) {
                    def.space_before = Some(v);
                }
                if let Some(v) = pp.attr("fo:margin-bottom").and_then(parse_pt) {
                    def.space_after = Some(v);
                }
                if let Some(v) = pp.attr("fo:margin-left").and_then(parse_length_in) {
                    def.left_in = Some(v);
                }
                if let Some(bg) = pp.attr("fo:background-color").and_then(parse_hex_rgb) {
                    def.shading = Some(bg);
                }
            }
            if let Some(tp) = node.find("style:text-properties") {
                if let Some(fw) = tp.attr("fo:font-weight") {
                    def.bold = Some(fw == "bold" || fw.parse::<u32>().is_ok_and(|w| w >= 600));
                }
                if let Some(fs) = tp.attr("fo:font-style") {
                    def.italic = Some(fs == "italic" || fs == "oblique");
                }
                if let Some(ul) = tp.attr("style:text-underline-style") {
                    def.underline = Some(ul != "none");
                }
                if let Some(st) = tp.attr("style:text-line-through-style") {
                    def.strike = Some(st != "none");
                }
                if let Some(sz) = tp.attr("fo:font-size").and_then(parse_pt) {
                    def.size = Some(sz.clamp(6.0, 96.0));
                }
                if let Some(fnm) = tp.attr("style:font-name").or_else(|| tp.attr("fo:font-family")) {
                    let clean = fnm.trim_matches('\'').trim_matches('"');
                    if !clean.is_empty() {
                        def.font = Some(font_index(clean));
                    }
                }
                if let Some(col) = tp.attr("fo:color").and_then(parse_hex_rgb) {
                    def.color = Some(col);
                }
                if let Some(bg) = tp.attr("fo:background-color").and_then(parse_hex_rgb) {
                    def.highlight = Some(bg);
                }
            }
            map.insert(name.to_string(), def);
        }
    }
    for ch in &node.children {
        if let XmlChild::Elem(n) = ch {
            collect_styles(n, map);
        }
    }
}

fn apply_style_chain(
    name: &str,
    styles: &HashMap<String, OdfStyleDef>,
    cs: &mut CharStyle,
    pf: &mut ParaFmt,
    depth: u8,
) {
    if depth > 8 {
        return;
    }
    let Some(def) = styles.get(name) else {
        return;
    };
    if let Some(parent) = &def.parent {
        apply_style_chain(parent, styles, cs, pf, depth + 1);
    }
    if let Some(a) = def.align {
        pf.align = a;
    }
    if let Some(v) = def.space_before {
        pf.space_before = v;
    }
    if let Some(v) = def.space_after {
        pf.space_after = v;
    }
    if let Some(v) = def.left_in {
        pf.left_in = v;
    }
    if let Some(v) = def.shading {
        pf.shading = Some(v);
    }
    if let Some(v) = def.bold {
        cs.bold = v;
    }
    if let Some(v) = def.italic {
        cs.italic = v;
    }
    if let Some(v) = def.underline {
        cs.underline = v;
    }
    if let Some(v) = def.strike {
        cs.strike = v;
    }
    if let Some(v) = def.size {
        cs.size = v;
    }
    if let Some(v) = def.font {
        cs.font = v;
    }
    if let Some(v) = def.color {
        cs.color = v;
    }
    if let Some(v) = def.highlight {
        cs.highlight = Some(v);
    }
}

// ── Public ODT Reader ───────────────────────────────────────────────────

/// Parses an OpenDocument Text (`.odt`) archive into an editable Syllabary `Document`.
pub fn read(data: &[u8]) -> Result<Document, String> {
    let files = unzip_all(data)?;
    let content_bytes = files
        .get("content.xml")
        .ok_or("Not a valid .odt file (missing content.xml).")?;
    let content_str = String::from_utf8_lossy(content_bytes);
    let content_root = parse_xml(&content_str).ok_or("Couldn't parse content.xml in .odt file.")?;

    let mut doc = Document::default();
    doc.paras.clear();
    let mut styles: HashMap<String, OdfStyleDef> = HashMap::new();

    // 1. Metadata (`meta.xml`)
    if let Some(mb) = files.get("meta.xml") {
        if let Some(mr) = parse_xml(&String::from_utf8_lossy(mb)) {
            if let Some(t) = mr.find_deep("dc:title") {
                doc.info.title = t.text_content().trim().to_string();
            }
            if let Some(a) = mr.find_deep("dc:creator") {
                doc.info.author = a.text_content().trim().to_string();
            }
            if let Some(s) = mr.find_deep("dc:subject") {
                doc.info.subject = s.text_content().trim().to_string();
            }
        }
    }

    // 2. Styles, page layout, headers & footers (`styles.xml`)
    if let Some(sb) = files.get("styles.xml") {
        if let Some(sr) = parse_xml(&String::from_utf8_lossy(sb)) {
            collect_styles(&sr, &mut styles);
            if let Some(plp) = sr.find_deep("style:page-layout-properties") {
                let mut pg = PageSetup::default();
                if let Some(w) = plp.attr("fo:page-width").and_then(parse_length_in) {
                    pg.width = w.clamp(2.0, 24.0);
                }
                if let Some(h) = plp.attr("fo:page-height").and_then(parse_length_in) {
                    pg.height = h.clamp(2.0, 36.0);
                }
                if let Some(m) = plp.attr("fo:margin-top").and_then(parse_length_in) {
                    pg.margin_top = m.clamp(0.0, 4.0);
                }
                if let Some(m) = plp.attr("fo:margin-bottom").and_then(parse_length_in) {
                    pg.margin_bottom = m.clamp(0.0, 4.0);
                }
                if let Some(m) = plp.attr("fo:margin-left").and_then(parse_length_in) {
                    pg.margin_left = m.clamp(0.0, 4.0);
                }
                if let Some(m) = plp.attr("fo:margin-right").and_then(parse_length_in) {
                    pg.margin_right = m.clamp(0.0, 4.0);
                }
                doc.page = pg;
            }
            if let Some(hdr) = sr.find_deep("style:header") {
                let txt = hdr.text_content().trim().to_string();
                if !txt.is_empty() {
                    doc.header = txt;
                }
            }
            if let Some(ftr) = sr.find_deep("style:footer") {
                let txt = ftr.text_content().trim().to_string();
                if !txt.is_empty() {
                    doc.footer = txt;
                }
            }
        }
    }

    // 3. Automatic styles & body (`content.xml`)
    collect_styles(&content_root, &mut styles);
    let mut next_img_id = 1u64;
    let mut next_tbl_id = 1u32;

    if let Some(body) = content_root.find_deep("office:text") {
        read_block_children(
            body,
            &styles,
            &files,
            &mut doc,
            &mut next_img_id,
            &mut next_tbl_id,
            ListKind::None,
            0,
        );
    }

    if doc.paras.is_empty() {
        doc.paras.push(Para::new(CharStyle::default(), ParaFmt::default()));
    }
    Ok(doc)
}

#[allow(clippy::too_many_arguments)]
fn read_block_children(
    parent: &XmlNode,
    styles: &HashMap<String, OdfStyleDef>,
    files: &HashMap<String, Vec<u8>>,
    doc: &mut Document,
    next_img_id: &mut u64,
    next_tbl_id: &mut u32,
    list_kind: ListKind,
    list_level: u8,
) {
    for ch in &parent.children {
        let XmlChild::Elem(node) = ch else {
            continue;
        };
        match node.tag.as_str() {
            "text:p" | "text:h" => {
                let mut cs = CharStyle::default();
                let mut pf = ParaFmt::default();
                if node.tag == "text:h" {
                    let lvl = node
                        .attr("text:outline-level")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1)
                        .clamp(1, 3);
                    pf.style = 3 + lvl; // 4=H1, 5=H2, 6=H3
                    cs.bold = true;
                    cs.size = match lvl {
                        1 => 20.0,
                        2 => 16.0,
                        _ => 13.0,
                    };
                }
                if let Some(sn) = node.attr("text:style-name") {
                    apply_style_chain(sn, styles, &mut cs, &mut pf, 0);
                }
                if list_kind != ListKind::None {
                    pf.list = list_kind;
                    pf.indent = list_level.saturating_sub(1);
                }
                let mut para = Para::new(cs, pf);
                read_inline_children(node, cs, styles, files, doc, &mut para, next_img_id);
                doc.paras.push(para);
            }
            "text:list" => {
                let kind = node
                    .attr("text:style-name")
                    .map(|sn| {
                        let lo = sn.to_ascii_lowercase();
                        if lo.contains("num") || lo.contains("order") {
                            ListKind::Number
                        } else {
                            ListKind::Bullet
                        }
                    })
                    .unwrap_or(ListKind::Bullet);
                for item in &node.children {
                    if let XmlChild::Elem(li) = item {
                        if li.tag == "text:list-item" {
                            read_block_children(
                                li,
                                styles,
                                files,
                                doc,
                                next_img_id,
                                next_tbl_id,
                                kind,
                                list_level.saturating_add(1).min(4),
                            );
                        }
                    }
                }
            }
            "table:table" => {
                read_table(node, styles, files, doc, next_img_id, next_tbl_id);
            }
            "text:section" => {
                read_block_children(node, styles, files, doc, next_img_id, next_tbl_id, list_kind, list_level);
            }
            _ => {}
        }
    }
}

fn read_inline_children(
    node: &XmlNode,
    cur_style: CharStyle,
    styles: &HashMap<String, OdfStyleDef>,
    files: &HashMap<String, Vec<u8>>,
    doc: &mut Document,
    para: &mut Para,
    next_img_id: &mut u64,
) {
    for ch in &node.children {
        match ch {
            XmlChild::Text(t) => {
                for c in t.chars() {
                    para.text.push(c);
                    para.styles.push(cur_style);
                }
            }
            XmlChild::Elem(el) => match el.tag.as_str() {
                "text:span" => {
                    let mut st = cur_style;
                    let mut dummy_pf = ParaFmt::default();
                    if let Some(sn) = el.attr("text:style-name") {
                        apply_style_chain(sn, styles, &mut st, &mut dummy_pf, 0);
                    }
                    read_inline_children(el, st, styles, files, doc, para, next_img_id);
                }
                "text:a" => {
                    let mut st = cur_style;
                    if let Some(href) = el.attr("xlink:href") {
                        doc.links.push(href.to_string());
                        st.link = doc.links.len() as u16;
                        st.underline = true;
                        st.color = [17, 85, 204];
                    }
                    read_inline_children(el, st, styles, files, doc, para, next_img_id);
                }
                "text:s" => {
                    let count = el.attr("text:c").and_then(|c| c.parse::<usize>().ok()).unwrap_or(1);
                    for _ in 0..count {
                        para.text.push(' ');
                        para.styles.push(cur_style);
                    }
                }
                "text:tab" => {
                    para.text.push('\t');
                    para.styles.push(cur_style);
                }
                "text:line-break" => {
                    para.text.push('\n');
                    para.styles.push(cur_style);
                }
                "draw:frame" => {
                    let w_in = el.attr("svg:width").and_then(parse_length_in).unwrap_or(3.0).clamp(0.4, 7.5);
                    let h_in = el.attr("svg:height").and_then(parse_length_in).unwrap_or(2.0).clamp(0.3, 9.0);
                    if let Some(img_el) = el.find_deep("draw:image") {
                        if let Some(href) = img_el.attr("xlink:href") {
                            let key = href.trim_start_matches("./");
                            if let Some(raw) = files.get(key) {
                                let id = *next_img_id;
                                *next_img_id += 1;
                                let nat = image::load_from_memory(raw)
                                    .map(|im| [im.width(), im.height()])
                                    .unwrap_or([300, 200]);
                                doc.images.push(DocImage {
                                    id,
                                    name: key.rsplit('/').next().unwrap_or("image").to_string(),
                                    natural_size: nat,
                                    caption: String::new(),
                                    width_in: w_in,
                                    height_in: h_in,
                                    png_b64: crate::doc::base64_encode(raw),
                                    seamless: false,
                                });
                                para.fmt.image_id = id;
                            }
                        }
                    }
                }
                _ => {
                    read_inline_children(el, cur_style, styles, files, doc, para, next_img_id);
                }
            },
        }
    }
}

fn read_table(
    tbl_node: &XmlNode,
    styles: &HashMap<String, OdfStyleDef>,
    files: &HashMap<String, Vec<u8>>,
    doc: &mut Document,
    next_img_id: &mut u64,
    next_tbl_id: &mut u32,
) {
    let tid = *next_tbl_id;
    *next_tbl_id += 1;
    let mut rows_spec = Vec::new();
    let mut row_idx = 0usize;

    for ch in &tbl_node.children {
        let XmlChild::Elem(row_el) = ch else {
            continue;
        };
        if row_el.tag != "table:table-row" {
            continue;
        }
        let mut cell_specs = Vec::new();
        let mut col_idx = 0usize;
        for cch in &row_el.children {
            let XmlChild::Elem(cell_el) = cch else {
                continue;
            };
            if cell_el.tag != "table:table-cell" {
                continue;
            }
            let mut cs = CharStyle::default();
            let mut pf = ParaFmt {
                table_id: tid,
                table_row: row_idx as u16,
                table_col: col_idx as u16,
                ..ParaFmt::default()
            };
            if let Some(sn) = cell_el.attr("table:style-name") {
                apply_style_chain(sn, styles, &mut cs, &mut pf, 0);
            }
            let cell_shading = pf.shading.take();
            let mut added_para = false;
            for pch in &cell_el.children {
                if let XmlChild::Elem(pe) = pch {
                    if pe.tag == "text:p" || pe.tag == "text:h" {
                        let mut pcs = cs;
                        let mut ppf = pf;
                        if let Some(sn) = pe.attr("text:style-name") {
                            apply_style_chain(sn, styles, &mut pcs, &mut ppf, 0);
                        }
                        let mut para = Para::new(pcs, ppf);
                        read_inline_children(pe, pcs, styles, files, doc, &mut para, next_img_id);
                        doc.paras.push(para);
                        added_para = true;
                    }
                }
            }
            if !added_para {
                doc.paras.push(Para::new(cs, pf));
            }
            cell_specs.push(TableCellSpec {
                shading: cell_shading,
                ..TableCellSpec::default()
            });
            col_idx += 1;
        }
        if !cell_specs.is_empty() {
            rows_spec.push(TableRowSpec {
                cells: cell_specs,
                ..TableRowSpec::default()
            });
            row_idx += 1;
        }
    }

    if !rows_spec.is_empty() {
        let max_cols = rows_spec.iter().map(|r| r.cells.len()).max().unwrap_or(1).max(1);
        let col_w = ((doc.page.width - doc.page.margin_left - doc.page.margin_right).max(2.0)) / max_cols as f32;
        for rw in &mut rows_spec {
            for c in &mut rw.cells {
                c.width_in = col_w;
            }
        }
        doc.tables.push(DocTable {
            id: tid,
            rows: rows_spec,
            grid_cols_in: vec![col_w; max_cols],
            indent_in: 0.0,
        });
    }
}

// ── Public ODT Writer ───────────────────────────────────────────────────

/// Serializes a Syllabary `Document` into a standards-compliant OpenDocument Text (`.odt`) archive.
pub fn write(doc: &Document) -> Vec<u8> {
    let mut entries: Vec<ZipEntry> = Vec::new();
    let mut manifest_items = String::new();

    // 1. `mimetype` MUST be the first entry, stored uncompressed (ODF 1.2 §3.3)
    entries.push(ZipEntry {
        name: "mimetype".into(),
        data: b"application/vnd.oasis.opendocument.text".to_vec(),
        store_uncompressed: true,
    });

    // 2. Embed images in `Pictures/`
    let mut img_hrefs: HashMap<u64, String> = HashMap::new();
    for img in &doc.images {
        let Some(raw) = crate::doc::base64_decode(&img.png_b64) else {
            continue;
        };
        let href = format!("Pictures/img_{}.png", img.id);
        manifest_items.push_str(&format!(
            "  <manifest:file-entry manifest:full-path=\"{href}\" manifest:media-type=\"image/png\"/>\n"
        ));
        entries.push(ZipEntry {
            name: href.clone(),
            data: raw,
            store_uncompressed: true,
        });
        img_hrefs.insert(img.id, href);
    }

    // 3. `meta.xml`
    let meta_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <office:document-meta xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" office:version=\"1.2\">\n\
         <office:meta>\n\
           <meta:generator>Syllabary</meta:generator>\n\
           <dc:title>{}</dc:title>\n\
           <dc:creator>{}</dc:creator>\n\
           <dc:subject>{}</dc:subject>\n\
         </office:meta>\n\
         </office:document-meta>\n",
        escape_xml(&doc.info.title),
        escape_xml(&doc.info.author),
        escape_xml(&doc.info.subject),
    );
    entries.push(ZipEntry {
        name: "meta.xml".into(),
        data: meta_xml.into_bytes(),
        store_uncompressed: false,
    });

    // 4. `styles.xml` (page geometry + headers/footers)
    let pg = doc.page;
    let hdr_xml = if doc.header.trim().is_empty() {
        String::new()
    } else {
        format!("<style:header><text:p>{}</text:p></style:header>", escape_xml(&doc.header))
    };
    let ftr_xml = if doc.footer.trim().is_empty() {
        String::new()
    } else {
        format!("<style:footer><text:p>{}</text:p></style:footer>", escape_xml(&doc.footer))
    };
    let styles_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <office:document-styles xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
         xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" office:version=\"1.2\">\n\
         <office:automatic-styles>\n\
           <style:page-layout style:name=\"pm1\">\n\
             <style:page-layout-properties fo:page-width=\"{:.3}in\" fo:page-height=\"{:.3}in\" \
             fo:margin-top=\"{:.3}in\" fo:margin-bottom=\"{:.3}in\" \
             fo:margin-left=\"{:.3}in\" fo:margin-right=\"{:.3}in\"/>\n\
           </style:page-layout>\n\
         </office:automatic-styles>\n\
         <office:master-styles>\n\
           <style:master-page style:name=\"Standard\" style:page-layout-name=\"pm1\">{hdr_xml}{ftr_xml}</style:master-page>\n\
         </office:master-styles>\n\
         </office:document-styles>\n",
        pg.width, pg.height, pg.margin_top, pg.margin_bottom, pg.margin_left, pg.margin_right
    );
    entries.push(ZipEntry {
        name: "styles.xml".into(),
        data: styles_xml.into_bytes(),
        store_uncompressed: false,
    });

    // 5. `content.xml`
    let mut auto_styles = String::new();
    let mut text_style_map: HashMap<(u16, u32, bool, bool, bool, bool, [u8; 3], Option<[u8; 3]>), String> = HashMap::new();
    let mut para_style_map: HashMap<(u8, i32, i32, i32), String> = HashMap::new();
    let mut body_xml = String::new();

    let mut get_tstyle = |st: &CharStyle, auto: &mut String| -> String {
        let key = (
            st.font,
            (st.size * 10.0).round() as u32,
            st.bold,
            st.italic,
            st.underline,
            st.strike,
            st.color,
            st.highlight,
        );
        if let Some(name) = text_style_map.get(&key) {
            return name.clone();
        }
        let name = format!("T{}", text_style_map.len() + 1);
        let fname = escape_xml(font_name(if st.font == 0 { DEFAULT_FONT } else { st.font }));
        let mut attrs = format!(
            "style:font-name=\"{fname}\" fo:font-family=\"{fname}\" fo:font-size=\"{:.1}pt\" fo:color=\"{}\"",
            st.size.max(6.0),
            fmt_hex_rgb(st.color)
        );
        if st.bold {
            attrs.push_str(" fo:font-weight=\"bold\"");
        }
        if st.italic {
            attrs.push_str(" fo:font-style=\"italic\"");
        }
        if st.underline {
            attrs.push_str(" style:text-underline-style=\"solid\"");
        }
        if st.strike {
            attrs.push_str(" style:text-line-through-style=\"solid\"");
        }
        if let Some(hl) = st.highlight {
            attrs.push_str(&format!(" fo:background-color=\"{}\"", fmt_hex_rgb(hl)));
        }
        auto.push_str(&format!(
            "  <style:style style:name=\"{name}\" style:family=\"text\"><style:text-properties {attrs}/></style:style>\n"
        ));
        text_style_map.insert(key, name.clone());
        name
    };

    let mut get_pstyle = |pf: &ParaFmt, auto: &mut String| -> String {
        let al_code = match pf.align {
            Align::Left => 0u8,
            Align::Center => 1,
            Align::Right => 2,
            Align::Justify => 3,
        };
        let key = (
            al_code,
            (pf.space_before * 10.0).round() as i32,
            (pf.space_after * 10.0).round() as i32,
            (pf.left_in * 100.0).round() as i32,
        );
        if let Some(name) = para_style_map.get(&key) {
            return name.clone();
        }
        let name = format!("P{}", para_style_map.len() + 1);
        let al_str = match pf.align {
            Align::Left => "start",
            Align::Center => "center",
            Align::Right => "end",
            Align::Justify => "justify",
        };
        auto.push_str(&format!(
            "  <style:style style:name=\"{name}\" style:family=\"paragraph\">\
             <style:paragraph-properties fo:text-align=\"{al_str}\" \
             fo:margin-top=\"{:.1}pt\" fo:margin-bottom=\"{:.1}pt\" fo:margin-left=\"{:.3}in\"/>\
             </style:style>\n",
            pf.space_before.max(0.0),
            pf.space_after.max(0.0),
            pf.left_in.max(0.0)
        ));
        para_style_map.insert(key, name.clone());
        name
    };

    let emit_para_inner = |para: &Para,
                           doc: &Document,
                           img_hrefs: &HashMap<u64, String>,
                           auto: &mut String,
                           get_tstyle: &mut dyn FnMut(&CharStyle, &mut String) -> String|
     -> String {
        let mut inner = String::new();
        if para.fmt.image_id > 0 {
            if let (Some(img), Some(href)) = (doc.image(para.fmt.image_id), img_hrefs.get(&para.fmt.image_id)) {
                inner.push_str(&format!(
                    "<draw:frame svg:width=\"{:.3}in\" svg:height=\"{:.3}in\" text:anchor-type=\"as-char\">\
                     <draw:image xlink:href=\"{href}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>\
                     </draw:frame>",
                    img.width_in, img.height_in
                ));
            }
        }
        for (a, b, st) in para.runs() {
            let txt: String = para.text[a..b].iter().collect();
            let tname = get_tstyle(&st, auto);
            let escaped = escape_xml(&txt).replace('\t', "<text:tab/>");
            if st.link > 0 {
                if let Some(url) = doc.links.get((st.link - 1) as usize) {
                    inner.push_str(&format!(
                        "<text:a xlink:type=\"simple\" xlink:href=\"{}\"><text:span text:style-name=\"{tname}\">{escaped}</text:span></text:a>",
                        escape_xml(url)
                    ));
                    continue;
                }
            }
            inner.push_str(&format!("<text:span text:style-name=\"{tname}\">{escaped}</text:span>"));
        }
        inner
    };

    let mut i = 0usize;
    while i < doc.paras.len() {
        let pf = doc.paras[i].fmt;
        if pf.table_id > 0 {
            let tid = pf.table_id;
            let mut tbl_paras: HashMap<(u16, u16), Vec<usize>> = HashMap::new();
            let mut max_r = 0u16;
            let mut max_c = 0u16;
            while i < doc.paras.len() && doc.paras[i].fmt.table_id == tid {
                let r = doc.paras[i].fmt.table_row;
                let c = doc.paras[i].fmt.table_col;
                max_r = max_r.max(r);
                max_c = max_c.max(c);
                tbl_paras.entry((r, c)).or_default().push(i);
                i += 1;
            }
            let (nrows, ncols) = doc.table(tid).map_or(
                (max_r as usize + 1, max_c as usize + 1),
                |tb| (tb.rows.len(), tb.rows.iter().map(|r| r.cells.len()).max().unwrap_or(1)),
            );
            body_xml.push_str(&format!("  <table:table table:name=\"Table{tid}\">\n"));
            for _ in 0..ncols {
                body_xml.push_str("    <table:table-column/>\n");
            }
            for r in 0..nrows {
                body_xml.push_str("    <table:table-row>\n");
                for c in 0..ncols {
                    body_xml.push_str("      <table:table-cell office:value-type=\"string\">\n");
                    if let Some(pidxs) = tbl_paras.get(&(r as u16, c as u16)) {
                        for &pi in pidxs {
                            let p = &doc.paras[pi];
                            let pn = get_pstyle(&p.fmt, &mut auto_styles);
                            let inner = emit_para_inner(p, doc, &img_hrefs, &mut auto_styles, &mut get_tstyle);
                            body_xml.push_str(&format!("        <text:p text:style-name=\"{pn}\">{inner}</text:p>\n"));
                        }
                    } else {
                        body_xml.push_str("        <text:p/>\n");
                    }
                    body_xml.push_str("      </table:table-cell>\n");
                }
                body_xml.push_str("    </table:table-row>\n");
            }
            body_xml.push_str("  </table:table>\n");
            continue;
        }

        if pf.list != ListKind::None {
            let lkind = pf.list;
            let lname = if lkind == ListKind::Number { "LNum" } else { "LBul" };
            body_xml.push_str(&format!("  <text:list text:style-name=\"{lname}\">\n"));
            while i < doc.paras.len() && doc.paras[i].fmt.table_id == 0 && doc.paras[i].fmt.list == lkind {
                let p = &doc.paras[i];
                let pn = get_pstyle(&p.fmt, &mut auto_styles);
                let inner = emit_para_inner(p, doc, &img_hrefs, &mut auto_styles, &mut get_tstyle);
                body_xml.push_str(&format!("    <text:list-item><text:p text:style-name=\"{pn}\">{inner}</text:p></text:list-item>\n"));
                i += 1;
            }
            body_xml.push_str("  </text:list>\n");
            continue;
        }

        let p = &doc.paras[i];
        let pn = get_pstyle(&p.fmt, &mut auto_styles);
        let inner = emit_para_inner(p, doc, &img_hrefs, &mut auto_styles, &mut get_tstyle);
        if matches!(p.fmt.style, 4..=6) {
            let lvl = p.fmt.style - 3;
            body_xml.push_str(&format!(
                "  <text:h text:style-name=\"{pn}\" text:outline-level=\"{lvl}\">{inner}</text:h>\n"
            ));
        } else {
            body_xml.push_str(&format!("  <text:p text:style-name=\"{pn}\">{inner}</text:p>\n"));
        }
        i += 1;
    }

    let content_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <office:document-content \
         xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
         xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
         xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" \
         xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
         xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" \
         office:version=\"1.2\">\n\
         <office:automatic-styles>\n\
           <text:list-style style:name=\"LBul\"><text:list-level-style-bullet text:level=\"1\" text:bullet-char=\"•\"/></text:list-style>\n\
           <text:list-style style:name=\"LNum\"><text:list-level-style-number text:level=\"1\" style:num-suffix=\".\" style:num-format=\"1\"/></text:list-style>\n\
         {auto_styles}\
         </office:automatic-styles>\n\
         <office:body>\n\
         <office:text>\n\
         {body_xml}\
         </office:text>\n\
         </office:body>\n\
         </office:document-content>\n"
    );
    entries.push(ZipEntry {
        name: "content.xml".into(),
        data: content_xml.into_bytes(),
        store_uncompressed: false,
    });

    // 6. `META-INF/manifest.xml`
    let manifest_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.2\">\n\
           <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.text\"/>\n\
           <manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\n\
           <manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>\n\
           <manifest:file-entry manifest:full-path=\"meta.xml\" manifest:media-type=\"text/xml\"/>\n\
         {manifest_items}\
         </manifest:manifest>\n"
    );
    entries.push(ZipEntry {
        name: "META-INF/manifest.xml".into(),
        data: manifest_xml.into_bytes(),
        store_uncompressed: false,
    });

    build_zip(&entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Pos;

    #[test]
    fn odt_roundtrip_preserves_text_styles_lists_tables_and_metadata() {
        let mut doc = Document::default();
        doc.info.title = "OpenDocument Test".into();
        doc.info.author = "Syllabary".into();
        doc.header = "Confidential Header".into();
        doc.footer = "Page 1".into();

        // Heading 1
        doc.paras[0].fmt.style = 4;
        doc.insert_text(Pos::new(0, 0), "Project Overview", CharStyle { bold: true, size: 20.0, ..Default::default() });
        doc.split(Pos::new(0, doc.paras[0].len()));

        // Styled body paragraph
        doc.paras[1].fmt.style = 0;
        doc.paras[1].fmt.align = Align::Center;
        doc.insert_text(Pos::new(1, 0), "Bold & Italic", CharStyle { bold: true, italic: true, color: [180, 20, 20], ..Default::default() });
        doc.split(Pos::new(1, doc.paras[1].len()));

        // Bullet item
        doc.paras[2].fmt.align = Align::Left;
        doc.paras[2].fmt.list = ListKind::Bullet;
        doc.insert_text(Pos::new(2, 0), "First bullet item", CharStyle::default());

        // Table
        let _ = doc.insert_table(Pos::new(2, doc.paras[2].len()), 2, 2);
        let tid = doc.tables.last().map(|t| t.id).unwrap_or(1);
        if let Some(pi) = doc.paras.iter().position(|p| p.fmt.table_id == tid && p.fmt.table_row == 0 && p.fmt.table_col == 0) {
            doc.insert_text(Pos::new(pi, 0), "Header A", CharStyle::default());
        }
        if let Some(pi) = doc.paras.iter().position(|p| p.fmt.table_id == tid && p.fmt.table_row == 1 && p.fmt.table_col == 1) {
            doc.insert_text(Pos::new(pi, 0), "Value 42", CharStyle::default());
        }

        let bytes = write(&doc);
        // ODF spec requires ZIP starting with uncompressed `mimetype`
        assert!(bytes.starts_with(b"PK\x03\x04"));
        assert!(String::from_utf8_lossy(&bytes[..100]).contains("application/vnd.oasis.opendocument.text"));

        let reloaded = read(&bytes).expect("odt::read parses written .odt");
        assert_eq!(reloaded.info.title, "OpenDocument Test");
        assert_eq!(reloaded.info.author, "Syllabary");
        assert_eq!(reloaded.header, "Confidential Header");
        assert_eq!(reloaded.footer, "Page 1");
        let plain = reloaded.plain_text();
        assert!(plain.contains("Project Overview"), "missing heading: {plain}");
        assert!(plain.contains("Bold & Italic"), "missing body: {plain}");
        assert!(plain.contains("First bullet item"), "missing list: {plain}");
        assert!(plain.contains("Header A") && plain.contains("Value 42"), "missing table: {plain}");
        assert!(reloaded.paras.iter().any(|p| p.fmt.list == ListKind::Bullet));
        assert!(!reloaded.tables.is_empty());
    }
}

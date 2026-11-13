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

fn run_props(s: &CharStyle) -> String {
    let f = esc(font_name(s.font));
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
    if let Some(c2) = s.color_to {
        r.push_str(&format!("<w:color w:val=\"{}\" w:colorTo=\"{}\"/>", hex(s.color), hex(c2)));
    } else if s.color != [0, 0, 0] {
        r.push_str(&format!("<w:color w:val=\"{}\"/>", hex(s.color)));
    }
    r.push_str(&format!("<w:sz w:val=\"{0}\"/><w:szCs w:val=\"{0}\"/>", (s.size * 2.0).round() as i32));
    if let Some(h) = s.highlight {
        if let Some(h2) = s.highlight_to {
            r.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\" w:fillTo=\"{}\"/>", hex(h), hex(h2)));
        } else {
            r.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(h)));
        }
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

fn write_para_xml(doc: &Document, p: &Para, body: &mut String) {
    body.push_str("<w:p><w:pPr>");
    if let Some(id) = style_id(p.fmt.style) {
        body.push_str(&format!("<w:pStyle w:val=\"{id}\"/>"));
    }
    if p.fmt.page_break_before {
        body.push_str("<w:pageBreakBefore/>");
    }
    if p.fmt.hr {
        body.push_str("<w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"CBD5E1\"/></w:pBdr>");
    }
    if let Some(shd) = p.fmt.shading {
        body.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(shd)));
    }
    match p.fmt.list {
        ListKind::Bullet => body.push_str(&format!(
            "<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"1\"/><w:preset w:val=\"{}\"/></w:numPr>",
            p.fmt.indent.min(8),
            p.fmt.bullet_preset
        )),
        ListKind::Number => body.push_str(&format!(
            "<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"2\"/><w:preset w:val=\"{}\"/></w:numPr>",
            p.fmt.indent.min(8),
            p.fmt.bullet_preset
        )),
        ListKind::Checklist => {
            let nid = if p.fmt.checked { 4 } else { 3 };
            body.push_str(&format!(
                "<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{nid}\"/><w:preset w:val=\"{}\"/></w:numPr>",
                p.fmt.indent.min(8),
                p.fmt.bullet_preset
            ));
        }
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
    if p.fmt.image_id > 0 {
        if let Some(img) = doc.image(p.fmt.image_id) {
            let cx = (img.width_in * 914_400.0).round() as i64;
            let cy = (img.height_in * 914_400.0).round() as i64;
            body.push_str(&format!(
                "<w:r><w:drawing><wp:inline><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{}\" name=\"{}\" descr=\"{}\" w:naturalW=\"{}\" w:naturalH=\"{}\"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed=\"rIdImg{}\"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>",
                img.id,
                esc(&img.name),
                esc(&img.caption),
                img.natural_size[0],
                img.natural_size[1],
                img.id
            ));
        }
    }
    for (a, b, s) in p.runs() {
        if s.link > 0 && (s.link as usize) <= doc.links.len() {
            body.push_str(&format!("<w:hyperlink r:id=\"rIdLink{}\">", s.link));
        }
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
        if s.link > 0 && (s.link as usize) <= doc.links.len() {
            body.push_str("</w:hyperlink>");
        }
    }
    body.push_str("</w:p>");
}

pub fn write(doc: &Document) -> Vec<u8> {
    let mut body = String::new();
    let mut extra_rels = String::new();
    for (li, url) in doc.links.iter().enumerate() {
        let rid = format!("rIdLink{}", li + 1);
        extra_rels.push_str(&format!(
            "<Relationship Id=\"{rid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
            esc(url)
        ));
    }
    let mut media_files: Vec<(String, Vec<u8>)> = Vec::new();
    for img in &doc.images {
        let rid = format!("rIdImg{}", img.id);
        let fname = format!("word/media/image{}.png", img.id);
        extra_rels.push_str(&format!(
            "<Relationship Id=\"{rid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image{}.png\"/>",
            img.id
        ));
        if let Some(bytes) = base64_decode(&img.png_b64) {
            media_files.push((fname, bytes));
        }
    }

    let tw = |v: f32| (v * 1440.0).round() as i32;
    let mut pi = 0;
    while pi < doc.paras.len() {
        let p = &doc.paras[pi];
        if p.fmt.table_id > 0 {
            let tid = p.fmt.table_id;
            let mut tbl_end = pi + 1;
            while tbl_end < doc.paras.len() && doc.paras[tbl_end].fmt.table_id == tid {
                tbl_end += 1;
            }
            if let Some(tbl) = doc.table(tid) {
                body.push_str("<w:tbl><w:tblPr><w:tblLayout w:type=\"fixed\"/>");
                if tbl.indent_in.abs() > 0.001 {
                    body.push_str(&format!("<w:tblInd w:w=\"{}\" w:type=\"dxa\"/>", tw(tbl.indent_in)));
                }
                body.push_str("</w:tblPr><w:tblGrid>");
                for &cw in &tbl.grid_cols_in {
                    body.push_str(&format!("<w:gridCol w:w=\"{}\"/>", tw(cw)));
                }
                body.push_str("</w:tblGrid>");
                for (r_idx, row_spec) in tbl.rows.iter().enumerate() {
                    body.push_str("<w:tr><w:trPr>");
                    if row_spec.grid_after > 0 {
                        body.push_str(&format!("<w:gridAfter w:val=\"{}\"/>", row_spec.grid_after));
                    }
                    if row_spec.min_height_pt > 0.0 {
                        body.push_str(&format!("<w:trHeight w:val=\"{}\"/>", (row_spec.min_height_pt * 20.0).round() as i32));
                    }
                    body.push_str("</w:trPr>");
                    for (c_idx, cell_spec) in row_spec.cells.iter().enumerate() {
                        body.push_str("<w:tc><w:tcPr>");
                        body.push_str(&format!("<w:tcW w:w=\"{}\" w:type=\"dxa\"/>", tw(cell_spec.width_in)));
                        if cell_spec.grid_span > 1 {
                            body.push_str(&format!("<w:gridSpan w:val=\"{}\"/>", cell_spec.grid_span));
                        }
                        body.push_str("<w:tcBorders>");
                        for (side, b_opt) in [
                            ("top", cell_spec.borders.top),
                            ("bottom", cell_spec.borders.bottom),
                            ("left", cell_spec.borders.left),
                            ("right", cell_spec.borders.right),
                        ] {
                            if let Some(b) = b_opt {
                                if b.width_pt > 0.0 {
                                    body.push_str(&format!(
                                        "<w:{side} w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>",
                                        (b.width_pt * 8.0).round().max(1.0) as i32,
                                        hex(b.color)
                                    ));
                                } else {
                                    body.push_str(&format!("<w:{side} w:val=\"nil\"/>"));
                                }
                            } else {
                                body.push_str(&format!("<w:{side} w:val=\"nil\"/>"));
                            }
                        }
                        body.push_str("</w:tcBorders>");
                        if let Some(shd) = cell_spec.shading {
                            body.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(shd)));
                        }
                        body.push_str(&format!(
                            "<w:tcMar><w:top w:w=\"{}\" w:type=\"dxa\"/><w:bottom w:w=\"{}\" w:type=\"dxa\"/><w:left w:w=\"{}\" w:type=\"dxa\"/><w:right w:w=\"{}\" w:type=\"dxa\"/></w:tcMar>",
                            tw(cell_spec.pad_top_in),
                            tw(cell_spec.pad_bottom_in),
                            tw(cell_spec.pad_left_in),
                            tw(cell_spec.pad_right_in)
                        ));
                        let va = match cell_spec.v_align {
                            CellVAlign::Top => "top",
                            CellVAlign::Center => "center",
                            CellVAlign::Bottom => "bottom",
                        };
                        body.push_str(&format!("<w:vAlign w:val=\"{va}\"/>"));
                        if !cell_spec.wrap_text {
                            body.push_str("<w:noWrap/>");
                        }
                        body.push_str("</w:tcPr>");
                        let mut wrote_any = false;
                        for cp in &doc.paras[pi..tbl_end] {
                            if cp.fmt.table_row as usize == r_idx && cp.fmt.table_col as usize == c_idx {
                                write_para_xml(doc, cp, &mut body);
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
                for cp in &doc.paras[pi..tbl_end] {
                    write_para_xml(doc, cp, &mut body);
                }
            }
            pi = tbl_end;
            continue;
        }
        write_para_xml(doc, p, &mut body);
        pi += 1;
    }
    let pg = doc.page;
    let orient = if pg.width > pg.height { " w:orient=\"landscape\"" } else { "" };
    let pg_nums = if pg.page_numbers { " w:pgNum=\"1\"" } else { "" };
    let pageless_attr = if doc.pageless { " w:pageless=\"1\"" } else { "" };
    let h_align = match doc.header_align {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
        Align::Justify => "both",
    };
    let f_align = match doc.footer_align {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
        Align::Justify => "both",
    };
    body.push_str(&format!(
        "<w:sectPr{pg_nums}{pageless_attr} w:headerText=\"{}\" w:footerText=\"{}\" w:headerAlign=\"{h_align}\" w:footerAlign=\"{f_align}\" w:headerItalic=\"{}\" w:footerItalic=\"{}\"><w:pgSz w:w=\"{}\" w:h=\"{}\"{orient}/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr>",
        esc(&doc.header),
        esc(&doc.footer),
        if doc.header_italic { "1" } else { "0" },
        if doc.footer_italic { "1" } else { "0" },
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
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">{bg_xml}<w:body>{body}</w:body></w:document>"
    );

    let mut styles = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\"/><w:sz w:val=\"24\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>");
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
    numbering.push_str("<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num><w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num><w:num w:numId=\"3\"><w:abstractNumId w:val=\"0\"/></w:num><w:num w:numId=\"4\"><w:abstractNumId w:val=\"0\"/></w:num></w:numbering>");

    let core = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:creator>{}</dc:creator><dc:subject>{}</dc:subject><cp:keywords>{}</cp:keywords><dc:description>{}</dc:description></cp:coreProperties>",
        esc(&doc.info.title),
        esc(&doc.info.author),
        esc(&doc.info.subject),
        esc(&doc.info.keywords),
        esc(&doc.info.notes)
    );
    let types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>";
    let doc_rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>{extra_rels}</Relationships>"
    );
    let mut entries: Vec<(&str, Vec<u8>)> = vec![
        ("[Content_Types].xml", types.as_bytes().to_vec()),
        ("_rels/.rels", rels.as_bytes().to_vec()),
        ("word/document.xml", document.into_bytes()),
        ("word/_rels/document.xml.rels", doc_rels.into_bytes()),
        ("word/styles.xml", styles.into_bytes()),
        ("word/numbering.xml", numbering.into_bytes()),
        ("docProps/core.xml", core.into_bytes()),
    ];
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

fn apply_rpr(s: &mut CharStyle, name: &str, a: &HashMap<String, String>, theme_major: Option<u8>, theme_minor: Option<u8>) {
    match name {
        "w:b" => s.bold = on(a),
        "w:i" => s.italic = on(a),
        "w:u" => s.underline = on(a),
        "w:strike" | "w:dstrike" => s.strike = on(a),
        "w:sz" => {
            if let Some(v) = a.get("w:val").and_then(|v| v.parse::<f32>().ok()) {
                s.size = (v / 2.0).clamp(1.0, MAX_SIZE);
            }
        }
        "w:rFonts" => {
            if let Some(f) = a.get("w:ascii").or_else(|| a.get("w:hAnsi")).filter(|f| !f.is_empty()) {
                s.font = font_index(f);
            } else if let Some(th) = a.get("w:asciiTheme").or_else(|| a.get("w:hAnsiTheme")) {
                let tl = th.to_ascii_lowercase();
                if tl.starts_with("major") {
                    s.font = theme_major.unwrap_or(2);
                } else if tl.starts_with("minor") {
                    s.font = theme_minor.unwrap_or(2);
                }
            }
        }
        "w:color" => {
            if let Some(c) = a.get("w:val").and_then(|v| parse_hex(v)) {
                s.color = c;
            }
            if let Some(c2) = a.get("w:colorTo").and_then(|v| parse_hex(v)) {
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
            if let Some(c2) = a.get("w:fillTo").and_then(|v| parse_hex(v)) {
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

fn parse_border(a: &HashMap<String, String>) -> Option<BorderSpec> {
    let val = a.get("w:val").map_or("single", |v| v.as_str());
    if matches!(val, "none" | "nil" | "0" | "false") {
        return None;
    }
    let sz = a.get("w:sz").and_then(|v| v.parse::<f32>().ok()).unwrap_or(4.0);
    if sz <= 0.0 {
        return None;
    }
    let width_pt = (sz / 8.0).clamp(0.25, 12.0);
    let color = a.get("w:color").and_then(|v| parse_hex(v)).unwrap_or([0, 0, 0]);
    Some(BorderSpec { width_pt, color })
}

fn parse_hf_part(xml_bytes: &[u8]) -> (String, Align, bool) {
    let xml = String::from_utf8_lossy(xml_bytes);
    let mut text = String::new();
    let mut align = Align::Center;
    let mut italic = false;
    let mut in_t = false;
    for t in tokens(&xml) {
        match t {
            Tok::Open(n, a, sc) => match n.as_str() {
                "w:jc" => {
                    align = match a.get("w:val").map(|s| s.as_str()) {
                        Some("right") | Some("end") => Align::Right,
                        Some("center") => Align::Center,
                        Some("both") | Some("distribute") => Align::Justify,
                        _ => Align::Left,
                    };
                }
                "w:i" => {
                    if on(&a) {
                        italic = true;
                    }
                }
                "w:t" => in_t = !sc,
                _ => {}
            },
            Tok::Close(n) if n == "w:t" => in_t = false,
            Tok::Text(s) if in_t => text.push_str(&s),
            _ => {}
        }
    }
    (text.trim().to_string(), align, italic)
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
}

impl CurTable {
    fn new(id: u32) -> Self {
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
    let want = match id.to_ascii_lowercase().replace([' ', '-', '_'], "").as_str() {
        "bodysingle" | "bodytext" => "Body Single",
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

pub fn read(data: &[u8]) -> Result<Document, String> {
    let files = unzip(data)?;
    let xml = files.get("word/document.xml").ok_or("this file has no Word document inside")?;
    let xml = String::from_utf8_lossy(xml);

    // Parse relationships (hyperlinks + embedded media images)
    let mut rel_targets: HashMap<String, String> = HashMap::new();
    if let Some(rels_xml) = files.get("word/_rels/document.xml.rels") {
        let rels_str = String::from_utf8_lossy(rels_xml);
        for t in tokens(&rels_str) {
            if let Tok::Open(n, a, _) = t {
                if n.ends_with("Relationship") {
                    if let (Some(id), Some(target)) = (a.get("Id"), a.get("Target")) {
                        rel_targets.insert(id.clone(), target.clone());
                    }
                }
            }
        }
    }

    // Parse theme fonts (majorFont / minorFont) from word/theme/theme1.xml
    let mut theme_major: Option<u8> = None;
    let mut theme_minor: Option<u8> = None;
    if let Some(th_xml) = files.get("word/theme/theme1.xml") {
        let th_str = String::from_utf8_lossy(th_xml);
        let mut in_major = false;
        let mut in_minor = false;
        for t in tokens(&th_str) {
            match t {
                Tok::Open(n, _, sc) if n == "a:majorFont" => in_major = !sc,
                Tok::Close(n) if n == "a:majorFont" => in_major = false,
                Tok::Open(n, _, sc) if n == "a:minorFont" => in_minor = !sc,
                Tok::Close(n) if n == "a:minorFont" => in_minor = false,
                Tok::Open(n, a, _) if n == "a:latin" && (in_major || in_minor) => {
                    if let Some(tf) = a.get("typeface").filter(|s| !s.is_empty()) {
                        let fi = font_index(tf);
                        if in_major && theme_major.is_none() {
                            theme_major = Some(fi);
                        } else if in_minor && theme_minor.is_none() {
                            theme_minor = Some(fi);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let apply_ppr_tok = |fmt: &mut ParaFmt, n: &str, a: &HashMap<String, String>| match n {
        "w:jc" => {
            fmt.align = match a.get("w:val").map(|s| s.as_str()) {
                Some("center") => Align::Center,
                Some("right") | Some("end") => Align::Right,
                Some("both") | Some("distribute") => Align::Justify,
                _ => Align::Left,
            };
        }
        "w:spacing" => {
            if let Some(v) = a.get("w:before").and_then(|v| v.parse::<f32>().ok()) {
                fmt.space_before = (v / 20.0).clamp(0.0, 72.0);
            }
            if let Some(v) = a.get("w:after").and_then(|v| v.parse::<f32>().ok()) {
                fmt.space_after = (v / 20.0).clamp(0.0, 72.0);
            }
            if a.get("w:lineRule").is_none_or(|r| r == "auto") {
                if let Some(v) = a.get("w:line").and_then(|v| v.parse::<f32>().ok()) {
                    fmt.line_spacing = (v / 240.0).clamp(1.0, 3.0);
                }
            }
        }
        "w:ind" => {
            if let Some(v) = a.get("w:left").or_else(|| a.get("w:start")).and_then(|v| v.parse::<f32>().ok()) {
                fmt.indent = (v / 720.0).round().clamp(0.0, 8.0) as u8;
            }
        }
        _ => {}
    };

    // document defaults (font, size, paragraph spacing) + custom style definitions
    let mut base = CharStyle { font: theme_minor.unwrap_or(2), size: 11.0, ..CharStyle::default() };
    let mut default_pfmt = ParaFmt { space_after: 0.0, ..ParaFmt::default() };
    let mut custom_styles: HashMap<String, (ParaFmt, CharStyle)> = HashMap::new();
    let mut char_styles: HashMap<String, Vec<(String, HashMap<String, String>)>> = HashMap::new();
    if let Some(st) = files.get("word/styles.xml") {
        struct RawStyle {
            id: String,
            stype: String,
            is_default: bool,
            based_on: Option<String>,
            ppr: Vec<(String, HashMap<String, String>)>,
            rpr: Vec<(String, HashMap<String, String>)>,
        }
        let st = String::from_utf8_lossy(st);
        let mut in_rpr_def = false;
        let mut in_ppr_def = false;
        let mut cur_style: Option<RawStyle> = None;
        let mut in_style_ppr = false;
        let mut in_style_rpr = false;
        let mut raw_styles: Vec<RawStyle> = Vec::new();
        for t in tokens(&st) {
            match t {
                Tok::Open(n, _, _) if n == "w:rPrDefault" => in_rpr_def = true,
                Tok::Close(n) if n == "w:rPrDefault" => in_rpr_def = false,
                Tok::Open(n, _, _) if n == "w:pPrDefault" => in_ppr_def = true,
                Tok::Close(n) if n == "w:pPrDefault" => in_ppr_def = false,
                Tok::Open(n, a, _) if in_rpr_def => apply_rpr(&mut base, &n, &a, theme_major, theme_minor),
                Tok::Open(n, a, _) if in_ppr_def => apply_ppr_tok(&mut default_pfmt, &n, &a),
                Tok::Open(n, a, selfclose) if n == "w:style" => {
                    let stype = a.get("w:type").cloned().unwrap_or_else(|| "paragraph".into());
                    if !selfclose && (stype == "paragraph" || stype == "character") {
                        let sid = a.get("w:styleId").cloned().unwrap_or_default();
                        let is_default = matches!(a.get("w:default").map(|s| s.as_str()), Some("1" | "true"));
                        cur_style = Some(RawStyle {
                            id: sid,
                            stype,
                            is_default,
                            based_on: None,
                            ppr: Vec::new(),
                            rpr: Vec::new(),
                        });
                    }
                }
                Tok::Close(n) if n == "w:style" => {
                    if let Some(rs) = cur_style.take() {
                        raw_styles.push(rs);
                    }
                    in_style_ppr = false;
                    in_style_rpr = false;
                }
                Tok::Open(n, a, _) if cur_style.is_some() && !in_style_ppr && !in_style_rpr && n == "w:basedOn" => {
                    if let (Some(rs), Some(b)) = (cur_style.as_mut(), a.get("w:val")) {
                        rs.based_on = Some(b.clone());
                    }
                }
                Tok::Open(n, _, sc) if cur_style.is_some() && n == "w:pPr" => in_style_ppr = !sc,
                Tok::Close(n) if cur_style.is_some() && n == "w:pPr" => in_style_ppr = false,
                Tok::Open(n, _, sc) if cur_style.is_some() && n == "w:rPr" => in_style_rpr = !sc,
                Tok::Close(n) if cur_style.is_some() && n == "w:rPr" => in_style_rpr = false,
                Tok::Open(n, a, _) if in_style_rpr => {
                    if let Some(rs) = cur_style.as_mut() {
                        rs.rpr.push((n, a));
                    }
                }
                Tok::Open(n, a, _) if in_style_ppr => {
                    if let Some(rs) = cur_style.as_mut() {
                        rs.ppr.push((n, a));
                    }
                }
                _ => {}
            }
        }

        // Apply default paragraph style (Normal / w:default="1") to default_pfmt and base.
        if let Some(norm) = raw_styles
            .iter()
            .find(|s| s.stype == "paragraph" && (s.is_default || s.id.eq_ignore_ascii_case("Normal")))
        {
            for (n, a) in &norm.ppr {
                apply_ppr_tok(&mut default_pfmt, n, a);
            }
            for (n, a) in &norm.rpr {
                apply_rpr(&mut base, n, a, theme_major, theme_minor);
            }
        }

        let by_id: HashMap<&str, &RawStyle> = raw_styles.iter().map(|s| (s.id.as_str(), s)).collect();
        for rs in &raw_styles {
            let mut chain: Vec<&RawStyle> = Vec::new();
            let mut cur = Some(rs);
            while let Some(s) = cur {
                if chain.len() >= 10 || chain.iter().any(|x| x.id == s.id) {
                    break;
                }
                chain.push(s);
                cur = s.based_on.as_deref().and_then(|bid| by_id.get(bid).copied());
            }
            if rs.stype == "paragraph" {
                let idx = style_index(&rs.id);
                let mut s_fmt = default_pfmt;
                let mut s_chr = base;
                if idx != 0 {
                    s_fmt.style = idx;
                }
                for anc in chain.iter().rev() {
                    for (n, a) in &anc.ppr {
                        apply_ppr_tok(&mut s_fmt, n, a);
                    }
                    for (n, a) in &anc.rpr {
                        apply_rpr(&mut s_chr, n, a, theme_major, theme_minor);
                    }
                }
                custom_styles.insert(rs.id.clone(), (s_fmt, s_chr));
            } else if rs.stype == "character" {
                let mut ops = Vec::new();
                for anc in chain.iter().rev() {
                    ops.extend(anc.rpr.iter().cloned());
                }
                char_styles.insert(rs.id.clone(), ops);
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
                Tok::Open(n, a, _) if n == "w:lvl" => cur_lvl0 = a.get("w:ilvl").is_some_and(|v| v == "0"),
                Tok::Open(n, a, _) if n == "w:numFmt" && cur_lvl0 => {
                    abs_bullet.insert(cur_abs.clone(), a.get("w:val").is_some_and(|v| v == "bullet"));
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
    let mut in_pbdr = false;
    let mut in_rpr = false;
    let mut in_run = false;
    let mut in_text = false;
    let mut in_drawing = false;
    let mut cur_link: u16 = 0;
    let mut para_base = base;
    let mut mark = base;
    let mut run = base;
    let mut list_num: Option<String> = None;
    let mut list_lvl = 0u8;
    let mut list_preset = 0u8;
    let mut pending_break = false;
    let mut in_sect = false;
    let mut draw_cx: i64 = 0;
    let mut draw_cy: i64 = 0;
    let mut draw_id: u64 = 0;
    let mut draw_name = String::new();
    let mut draw_caption = String::new();
    let mut draw_nat_w: u32 = 0;
    let mut draw_nat_h: u32 = 0;
    let mut draw_rid = String::new();
    let mut cur_tbl: Option<CurTable> = None;
    let mut next_tbl_id: u32 = 1;

    for t in tokens(&xml) {
        match t {
            Tok::Open(n, a, selfclose) => match n.as_str() {
                "w:background" => {
                    if let Some(c) = a.get("w:color").and_then(|v| parse_hex(v)) {
                        doc.page.background = c;
                    }
                }
                "w:tbl" => {
                    if !selfclose {
                        let tid = next_tbl_id;
                        next_tbl_id += 1;
                        cur_tbl = Some(CurTable::new(tid));
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
                        if val.to_ascii_lowercase().contains("grid") {
                            let b = Some(BorderSpec { width_pt: 0.5, color: [0, 0, 0] });
                            tb.tbl_top = b;
                            tb.tbl_bottom = b;
                            tb.tbl_left = b;
                            tb.tbl_right = b;
                            tb.tbl_inside_h = b;
                            tb.tbl_inside_v = b;
                        }
                    }
                }
                "w:tblInd" if cur_tbl.as_ref().is_some_and(|tb| tb.in_tbl_pr) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        if let Some(w) = a.get("w:w").and_then(|v| v.parse::<f32>().ok()) {
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
                        let w = a.get("w:w").and_then(|v| v.parse::<f32>().ok()).unwrap_or(2880.0);
                        tb.grid_cols_in.push((w / 1440.0).max(0.1));
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
                            if let Some(v) = a.get("w:val").and_then(|v| v.parse::<f32>().ok()) {
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
                        let b = parse_border(&a);
                        match n.as_str() {
                            "w:top" => tb.tbl_top = b,
                            "w:bottom" => tb.tbl_bottom = b,
                            "w:left" => tb.tbl_left = b,
                            "w:right" => tb.tbl_right = b,
                            "w:insideH" => tb.tbl_inside_h = b,
                            "w:insideV" => tb.tbl_inside_v = b,
                            _ => {}
                        }
                    }
                }
                _ if cur_tbl.as_ref().is_some_and(|tb| tb.in_tbl_cell_mar) => {
                    if let Some(tb) = cur_tbl.as_mut() {
                        if let Some(w) = a.get("w:w").and_then(|v| v.parse::<f32>().ok()) {
                            let inches = (w / 1440.0).max(0.0);
                            match n.as_str() {
                                "w:left" | "w:start" => tb.def_pad_left_in = inches,
                                "w:right" | "w:end" => tb.def_pad_right_in = inches,
                                "w:top" => tb.def_pad_top_in = inches,
                                "w:bottom" => tb.def_pad_bottom_in = inches,
                                _ => {}
                            }
                        }
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
                            if let Some(w) = a.get("w:w").and_then(|v| v.parse::<f32>().ok()) {
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
                                    if let Some(w) = a.get("w:w").and_then(|v| v.parse::<f32>().ok()) {
                                        if w > 0.0 {
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
                                "w:noWrap" => {
                                    cell.spec.wrap_text = !on(&a);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "w:p" => {
                    let mut pfmt = default_pfmt;
                    if let Some(tb) = cur_tbl.as_mut() {
                        pfmt.table_id = tb.id;
                        pfmt.table_row = tb.cur_row_idx;
                        pfmt.table_col = tb.cur_col_idx;
                        pfmt.space_after = 0.0;
                        pfmt.line_spacing = 1.0;
                        tb.cur_cell_para_count += 1;
                    }
                    let mut p = Para::new(base, pfmt);
                    p.fmt.page_break_before = std::mem::take(&mut pending_break);
                    para = Some(p);
                    para_base = base;
                    mark = base;
                    list_num = None;
                    list_lvl = 0;
                    list_preset = 0;
                    if selfclose {
                        if let Some(p) = para.take() {
                            doc.paras.push(p);
                        }
                    }
                }
                "w:pPr" => in_ppr = !selfclose,
                "w:pBdr" if in_ppr => in_pbdr = !selfclose,
                "w:rPr" => in_rpr = !selfclose,
                "w:hyperlink" => {
                    if !selfclose {
                        if let Some(rid) = a.get("r:id") {
                            if let Some(url) = rel_targets.get(rid) {
                                cur_link = doc.add_link(url);
                            }
                        }
                    }
                }
                "w:r" => {
                    in_run = !selfclose;
                    run = para_base;
                    if cur_link > 0 {
                        run.link = cur_link;
                    }
                }
                "w:drawing" if in_run => {
                    in_drawing = !selfclose;
                    draw_cx = 0;
                    draw_cy = 0;
                    draw_id = 0;
                    draw_name.clear();
                    draw_caption.clear();
                    draw_nat_w = 0;
                    draw_nat_h = 0;
                    draw_rid.clear();
                }
                "wp:extent" if in_drawing => {
                    draw_cx = a.get("cx").and_then(|v| v.parse().ok()).unwrap_or(0);
                    draw_cy = a.get("cy").and_then(|v| v.parse().ok()).unwrap_or(0);
                }
                "wp:docPr" if in_drawing => {
                    draw_id = a.get("id").and_then(|v| v.parse().ok()).unwrap_or(0);
                    draw_name = a.get("name").cloned().unwrap_or_default();
                    draw_caption = a.get("descr").cloned().unwrap_or_default();
                    draw_nat_w = a.get("w:naturalW").and_then(|v| v.parse().ok()).unwrap_or(0);
                    draw_nat_h = a.get("w:naturalH").and_then(|v| v.parse().ok()).unwrap_or(0);
                }
                "a:blip" if in_drawing => {
                    draw_rid = a.get("r:embed").cloned().unwrap_or_default();
                }
                "w:t" => in_text = !selfclose,
                "w:sectPr" => {
                    in_sect = !selfclose;
                    if a.get("w:pgNum").is_some_and(|v| v == "1" || v == "true") {
                        doc.page.page_numbers = true;
                    }
                    if a.get("w:pageless").is_some_and(|v| v == "1" || v == "true") {
                        doc.pageless = true;
                    }
                    if let Some(h) = a.get("w:headerText") {
                        doc.header = h.clone();
                    }
                    if let Some(f) = a.get("w:footerText") {
                        doc.footer = f.clone();
                    }
                    if let Some(ha) = a.get("w:headerAlign") {
                        doc.header_align = match ha.as_str() {
                            "left" => Align::Left,
                            "right" => Align::Right,
                            "both" => Align::Justify,
                            _ => Align::Center,
                        };
                    }
                    if let Some(fa) = a.get("w:footerAlign") {
                        doc.footer_align = match fa.as_str() {
                            "left" => Align::Left,
                            "right" => Align::Right,
                            "both" => Align::Justify,
                            _ => Align::Center,
                        };
                    }
                    if let Some(hi) = a.get("w:headerItalic") {
                        doc.header_italic = hi == "1" || hi == "true";
                    }
                    if let Some(fi) = a.get("w:footerItalic") {
                        doc.footer_italic = fi == "1" || fi == "true";
                    }
                }
                "w:headerReference" if in_sect => {
                    if let Some(rid) = a.get("r:id") {
                        if let Some(target) = rel_targets.get(rid) {
                            let path = if target.starts_with("word/") {
                                target.clone()
                            } else {
                                format!("word/{}", target.trim_start_matches('/'))
                            };
                            if let Some(bytes) = files.get(&path) {
                                let (txt, al, it) = parse_hf_part(bytes);
                                if !txt.is_empty() {
                                    doc.header = txt;
                                    doc.header_align = al;
                                    doc.header_italic = it;
                                }
                            }
                        }
                    }
                }
                "w:footerReference" if in_sect => {
                    if let Some(rid) = a.get("r:id") {
                        if let Some(target) = rel_targets.get(rid) {
                            let path = if target.starts_with("word/") {
                                target.clone()
                            } else {
                                format!("word/{}", target.trim_start_matches('/'))
                            };
                            if let Some(bytes) = files.get(&path) {
                                let (txt, al, it) = parse_hf_part(bytes);
                                if !txt.is_empty() {
                                    doc.footer = txt;
                                    doc.footer_align = al;
                                    doc.footer_italic = it;
                                }
                            }
                        }
                    }
                }
                "w:pgSz" if in_sect => {
                    let w = a.get("w:w").and_then(|v| v.parse::<f32>().ok());
                    let h = a.get("w:h").and_then(|v| v.parse::<f32>().ok());
                    if let (Some(w), Some(h)) = (w, h) {
                        let mut pw = (w / 1440.0 * 100.0).round() / 100.0;
                        let mut ph = (h / 1440.0 * 100.0).round() / 100.0;
                        for &(_, sw, sh) in PAGE_SIZES {
                            if (pw - sw).abs() < 0.01 && (ph - sh).abs() < 0.01 {
                                pw = sw;
                                ph = sh;
                                break;
                            } else if (pw - sh).abs() < 0.01 && (ph - sw).abs() < 0.01 {
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
                        a.get(k).and_then(|v| v.parse::<f32>().ok()).map(|v| {
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
                }
                _ if in_rpr && in_run => {
                    if n == "w:rStyle" {
                        if let Some(ops) = a.get("w:val").and_then(|v| char_styles.get(v)) {
                            for (op_n, op_a) in ops {
                                apply_rpr(&mut run, op_n, op_a, theme_major, theme_minor);
                            }
                        }
                    } else {
                        apply_rpr(&mut run, &n, &a, theme_major, theme_minor);
                    }
                }
                _ if in_rpr && in_ppr => {
                    if n == "w:rStyle" {
                        if let Some(ops) = a.get("w:val").and_then(|v| char_styles.get(v)) {
                            for (op_n, op_a) in ops {
                                apply_rpr(&mut mark, op_n, op_a, theme_major, theme_minor);
                            }
                        }
                    } else {
                        apply_rpr(&mut mark, &n, &a, theme_major, theme_minor);
                    }
                }
                _ if in_ppr => {
                    let Some(p) = para.as_mut() else { continue };
                    match n.as_str() {
                        "w:bottom" if in_pbdr => {
                            if on(&a) {
                                p.fmt.hr = true;
                            }
                        }
                        "w:shd" => {
                            if let Some(c) = a.get("w:fill").and_then(|v| parse_hex(v)) {
                                if c != [255, 255, 255] {
                                    p.fmt.shading = Some(c);
                                }
                            }
                        }
                        "w:pStyle" => {
                            let val = a.get("w:val").map_or("", |v| v.as_str());
                            let idx = style_index(val);
                            if let Some((sfmt, schr)) = custom_styles.get(val) {
                                p.fmt = ParaFmt {
                                    style: idx,
                                    page_break_before: p.fmt.page_break_before,
                                    table_id: p.fmt.table_id,
                                    table_row: p.fmt.table_row,
                                    table_col: p.fmt.table_col,
                                    ..*sfmt
                                };
                                para_base = *schr;
                                mark = *schr;
                            } else if idx != 0 {
                                p.fmt = ParaFmt {
                                    style: idx,
                                    page_break_before: p.fmt.page_break_before,
                                    table_id: p.fmt.table_id,
                                    table_row: p.fmt.table_row,
                                    table_col: p.fmt.table_col,
                                    ..styles[idx].fmt
                                };
                                para_base = styles[idx].chr;
                                mark = styles[idx].chr;
                            } else if val.to_lowercase().contains("list") {
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
                            if a.get("w:lineRule").is_none_or(|r| r == "auto") {
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
                        "w:preset" => list_preset = a.get("w:val").and_then(|v| v.parse().ok()).unwrap_or(0),
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
                    if a.get("w:type").is_some_and(|t| t == "page") {
                        if let Some(p) = para.take() {
                            doc.paras.push(p);
                        }
                        let mut np = Para::new(para_base, ParaFmt::default());
                        np.fmt.page_break_before = true;
                        para = Some(np);
                    } else if let Some(p) = para.as_mut() {
                        // a line break inside a paragraph becomes a new paragraph (preserving cell membership if in a table)
                        let fmt = p.fmt;
                        let done = std::mem::replace(p, Para::new(run, ParaFmt { page_break_before: false, ..fmt }));
                        doc.paras.push(done);
                        if let Some(tb) = cur_tbl.as_mut() {
                            tb.cur_cell_para_count += 1;
                        }
                    }
                }
                _ => {}
            },
            Tok::Close(n) => match n.as_str() {
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
                                space_after: 0.0,
                                line_spacing: 1.0,
                                ..default_pfmt
                            };
                            doc.paras.push(Para::new(base, pfmt));
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
                    if let Some(tb) = cur_tbl.take() {
                        doc.tables.push(tb.finish());
                    }
                }
                "w:pBdr" => in_pbdr = false,
                "w:pPr" => {
                    in_ppr = false;
                    if let (Some(p), Some(id)) = (para.as_mut(), list_num.as_ref()) {
                        if id == "3" {
                            p.fmt.list = ListKind::Checklist;
                            p.fmt.checked = false;
                            p.fmt.indent = list_lvl.min(8);
                            p.fmt.bullet_preset = list_preset;
                        } else if id == "4" {
                            p.fmt.list = ListKind::Checklist;
                            p.fmt.checked = true;
                            p.fmt.indent = list_lvl.min(8);
                            p.fmt.bullet_preset = list_preset;
                        } else if id != "0" {
                            p.fmt.list = if *bullet_num.get(id).unwrap_or(&true) { ListKind::Bullet } else { ListKind::Number };
                            p.fmt.indent = list_lvl.min(8);
                            p.fmt.bullet_preset = list_preset;
                        }
                    }
                    if let Some(p) = para.as_mut() {
                        p.end_style = mark;
                    }
                }
                "w:drawing" => {
                    in_drawing = false;
                    if let Some(p) = para.as_mut() {
                        let target_path = rel_targets
                            .get(&draw_rid)
                            .map_or_else(String::new, |t| if t.starts_with("word/") { t.clone() } else { format!("word/{}", t.trim_start_matches('/')) });
                        if let Some(img_bytes) = files.get(&target_path) {
                            let id = if draw_id > 0 { draw_id } else { doc.images.len() as u64 + 1 };
                            let width_in = if draw_cx > 0 { (draw_cx as f32 / 914_400.0).clamp(0.5, 20.0) } else { 3.0 };
                            let height_in = if draw_cy > 0 { (draw_cy as f32 / 914_400.0).clamp(0.5, 20.0) } else { 2.0 };
                            let nat = if draw_nat_w > 0 && draw_nat_h > 0 {
                                [draw_nat_w, draw_nat_h]
                            } else if let Ok(dimg) = image::load_from_memory(img_bytes) {
                                [dimg.width().max(1), dimg.height().max(1)]
                            } else {
                                [(width_in * 96.0).round() as u32, (height_in * 96.0).round() as u32]
                            };
                            doc.images.push(DocImage {
                                id,
                                name: if draw_name.is_empty() { format!("Image {id}") } else { draw_name.clone() },
                                natural_size: nat,
                                width_in,
                                height_in,
                                caption: draw_caption.clone(),
                                png_b64: base64_encode(img_bytes),
                            });
                            p.fmt.image_id = id;
                        }
                    }
                }
                "w:hyperlink" => cur_link = 0,
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
                    "dc:description" => doc.info.notes = s,
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
}

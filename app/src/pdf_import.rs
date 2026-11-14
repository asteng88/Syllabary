//! Native PDF importer: parses PDF 1.0–1.7+ files (xref tables, cross-reference streams,
//! object streams, Flate/ASCII85/ASCIIHex/RunLength filters, PNG/TIFF predictors, ToUnicode CMaps,
//! WinAnsi/MacRoman/Differences encodings, graphics state, text operators, vector decorations,
//! and embedded images) into an editable Scribe Pro `Document`.

use std::collections::{BTreeMap, HashMap};

use image::ImageEncoder;

use crate::doc::{
    font_index, lerp_rgb, Align, BorderSpec, CellBorders, CellVAlign, CharStyle, DocImage, DocTable,
    Document, FooterRun, FooterSegment, ListKind, PageFooter, PageSetup, Para, ParaFmt,
    TableCellSpec, TableRowSpec,
};

// ── PDF Object Model ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum Obj {
    Null,
    Bool,
    Int(i64),
    Real(f64),
    Name(String),
    Str(Vec<u8>),
    Array(Vec<Obj>),
    Dict(BTreeMap<String, Obj>),
    Stream { dict: BTreeMap<String, Obj>, data: Vec<u8> },
    Ref(u32),
}

impl Obj {
    fn as_int(&self) -> Option<i64> {
        match self {
            Obj::Int(i) => Some(*i),
            Obj::Real(r) => Some(*r as i64),
            _ => None,
        }
    }

    fn as_f32(&self) -> Option<f32> {
        match self {
            Obj::Int(i) => Some(*i as f32),
            Obj::Real(r) => Some(*r as f32),
            _ => None,
        }
    }

    fn as_name(&self) -> Option<&str> {
        match self {
            Obj::Name(s) => Some(s.as_str()),
            _ => None,
        }
    }

    fn as_str_bytes(&self) -> Option<&[u8]> {
        match self {
            Obj::Str(b) => Some(b.as_slice()),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&[Obj]> {
        match self {
            Obj::Array(a) => Some(a.as_slice()),
            _ => None,
        }
    }

    fn as_dict(&self) -> Option<&BTreeMap<String, Obj>> {
        match self {
            Obj::Dict(d) | Obj::Stream { dict: d, .. } => Some(d),
            _ => None,
        }
    }
}

// ── Byte Lexer ──────────────────────────────────────────────────────────

struct Parser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos: pos.min(data.len()) }
    }

    fn skip_ws_and_comments(&mut self) {
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            if matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x00 | 0x0c) {
                self.pos += 1;
            } else if b == b'%' {
                while self.pos < self.data.len() && !matches!(self.data[self.pos], b'\r' | b'\n') {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws_and_comments();
        self.data.get(self.pos).copied()
    }

    fn starts_with(&self, pat: &[u8]) -> bool {
        self.data[self.pos..].starts_with(pat)
    }

    fn read_word(&mut self) -> String {
        self.skip_ws_and_comments();
        let start = self.pos;
        while self.pos < self.data.len() && !is_delim_or_ws(self.data[self.pos]) {
            self.pos += 1;
        }
        String::from_utf8_lossy(&self.data[start..self.pos]).to_string()
    }

    fn read_obj(&mut self) -> Result<Obj, String> {
        self.skip_ws_and_comments();
        let Some(&b) = self.data.get(self.pos) else {
            return Err("unexpected end of PDF".into());
        };
        match b {
            b'<' => {
                if self.starts_with(b"<<") {
                    self.read_dict_or_stream()
                } else {
                    Ok(Obj::Str(self.read_hex_string()))
                }
            }
            b'(' => Ok(Obj::Str(self.read_lit_string())),
            b'[' => self.read_array(),
            b'/' => Ok(Obj::Name(self.read_name())),
            b't' if self.starts_with(b"true") => {
                self.pos += 4;
                Ok(Obj::Bool)
            }
            b'f' if self.starts_with(b"false") => {
                self.pos += 5;
                Ok(Obj::Bool)
            }
            b'n' if self.starts_with(b"null") => {
                self.pos += 4;
                Ok(Obj::Null)
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => Ok(self.read_number_or_ref()),
            _ => {
                let w = self.read_word();
                if w.is_empty() {
                    self.pos += 1;
                }
                Ok(Obj::Null)
            }
        }
    }

    fn read_number_or_ref(&mut self) -> Obj {
        let save = self.pos;
        let first = self.read_word();
        if let Ok(n1) = first.parse::<i64>() {
            let after_first = self.pos;
            self.skip_ws_and_comments();
            if self.pos < self.data.len() && self.data[self.pos].is_ascii_digit() {
                let second = self.read_word();
                if second.parse::<u16>().is_ok() {
                    self.skip_ws_and_comments();
                    if self.pos < self.data.len() && self.data[self.pos] == b'R' {
                        let next_is_delim = self
                            .data
                            .get(self.pos + 1)
                            .is_none_or(|&c| is_delim_or_ws(c));
                        if next_is_delim && n1 >= 0 {
                            self.pos += 1;
                            return Obj::Ref(n1 as u32);
                        }
                    }
                }
            }
            self.pos = after_first;
            return Obj::Int(n1);
        }
        if let Ok(r) = first.parse::<f64>() {
            return Obj::Real(r);
        }
        self.pos = save + 1;
        Obj::Null
    }

    fn read_name(&mut self) -> String {
        self.pos += 1; // skip '/'
        let mut out = Vec::new();
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            if is_delim_or_ws(b) {
                break;
            }
            if b == b'#' && self.pos + 2 < self.data.len() {
                if let (Some(h), Some(l)) = (
                    hex_val(self.data[self.pos + 1]),
                    hex_val(self.data[self.pos + 2]),
                ) {
                    out.push((h << 4) | l);
                    self.pos += 3;
                    continue;
                }
            }
            out.push(b);
            self.pos += 1;
        }
        String::from_utf8_lossy(&out).to_string()
    }

    fn read_lit_string(&mut self) -> Vec<u8> {
        self.pos += 1; // skip '('
        let mut out = Vec::new();
        let mut depth = 1usize;
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            self.pos += 1;
            match b {
                b'(' => {
                    depth += 1;
                    out.push(b'(');
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return out;
                    }
                    out.push(b')');
                }
                b'\\' => {
                    let Some(&esc) = self.data.get(self.pos) else {
                        break;
                    };
                    self.pos += 1;
                    match esc {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        b'(' | b')' | b'\\' => out.push(esc),
                        b'\r' => {
                            if self.data.get(self.pos) == Some(&b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {}
                        b'0'..=b'7' => {
                            let mut val = (esc - b'0') as u16;
                            for _ in 0..2 {
                                if let Some(&d) = self.data.get(self.pos) {
                                    if (b'0'..=b'7').contains(&d) {
                                        val = val * 8 + (d - b'0') as u16;
                                        self.pos += 1;
                                        continue;
                                    }
                                }
                                break;
                            }
                            out.push((val & 0xff) as u8);
                        }
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }
        out
    }

    fn read_hex_string(&mut self) -> Vec<u8> {
        self.pos += 1; // skip '<'
        let mut nibbles = Vec::new();
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            self.pos += 1;
            if b == b'>' {
                break;
            }
            if let Some(v) = hex_val(b) {
                nibbles.push(v);
            }
        }
        let mut out = Vec::with_capacity(nibbles.len().div_ceil(2));
        for pair in nibbles.chunks(2) {
            let hi = pair[0];
            let lo = pair.get(1).copied().unwrap_or(0);
            out.push((hi << 4) | lo);
        }
        out
    }

    fn read_array(&mut self) -> Result<Obj, String> {
        self.pos += 1; // skip '['
        let mut items = Vec::new();
        loop {
            match self.peek() {
                None => break,
                Some(b']') => {
                    self.pos += 1;
                    break;
                }
                Some(_) => items.push(self.read_obj()?),
            }
        }
        Ok(Obj::Array(items))
    }

    fn read_dict_or_stream(&mut self) -> Result<Obj, String> {
        self.pos += 2; // skip '<<'
        let mut dict = BTreeMap::new();
        loop {
            self.skip_ws_and_comments();
            if self.pos >= self.data.len() {
                break;
            }
            if self.starts_with(b">>") {
                self.pos += 2;
                break;
            }
            if self.data[self.pos] != b'/' {
                // Recover from unexpected token inside dict
                self.pos += 1;
                continue;
            }
            let key = self.read_name();
            let val = self.read_obj()?;
            dict.insert(key, val);
        }
        let save = self.pos;
        self.skip_ws_and_comments();
        if self.starts_with(b"stream") {
            let mut after_kw = self.pos + 6;
            while matches!(self.data.get(after_kw), Some(b' ' | b'\t')) {
                after_kw += 1;
            }
            let next = self.data.get(after_kw).copied();
            if matches!(next, Some(b'\r' | b'\n')) {
                self.pos = after_kw;
                if self.data.get(self.pos) == Some(&b'\r') {
                    self.pos += 1;
                }
                if self.data.get(self.pos) == Some(&b'\n') {
                    self.pos += 1;
                }
                let start = self.pos;
                let explicit_len = dict.get("Length").and_then(Obj::as_int).and_then(|n| usize::try_from(n).ok());
                let end = if let Some(len) = explicit_len.filter(|&l| start + l <= self.data.len()) {
                    self.pos = start + len;
                    start + len
                } else {
                    let rel = find_bytes(&self.data[start..], b"endstream").unwrap_or(self.data.len() - start);
                    let mut e = start + rel;
                    while e > start && matches!(self.data[e - 1], b'\r' | b'\n') {
                        e -= 1;
                    }
                    self.pos = start + rel;
                    e
                };
                self.skip_ws_and_comments();
                if self.starts_with(b"endstream") {
                    self.pos += 9;
                }
                return Ok(Obj::Stream { dict, data: self.data[start..end].to_vec() });
            }
        }
        self.pos = save;
        Ok(Obj::Dict(dict))
    }
}

fn is_delim_or_ws(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\t' | b'\r' | b'\n' | 0x00 | 0x0c | b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn rfind_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

// ── Stream Filters & Predictors ─────────────────────────────────────────

fn decode_stream(dict: &BTreeMap<String, Obj>, raw: &[u8], doc: &PdfDoc<'_>) -> Result<Vec<u8>, String> {
    let mut filters: Vec<String> = Vec::new();
    if let Some(f) = dict.get("Filter") {
        let resolved = doc.resolve(f);
        if let Some(name) = resolved.as_name() {
            filters.push(name.to_string());
        } else if let Some(arr) = resolved.as_array() {
            for item in arr {
                if let Some(name) = doc.resolve(item).as_name() {
                    filters.push(name.to_string());
                }
            }
        }
    }
    let mut cur = raw.to_vec();
    for (idx, f) in filters.iter().enumerate() {
        cur = match f.as_str() {
            "FlateDecode" | "Fl" => {
                let decompressed = miniz_oxide::inflate::decompress_to_vec_zlib(&cur)
                    .or_else(|_| miniz_oxide::inflate::decompress_to_vec(&cur))
                    .map_err(|e| format!("FlateDecode error: {e:?}"))?;
                apply_decode_parms(&decompressed, dict, idx, doc)
            }
            "ASCIIHexDecode" | "AHx" => decode_ascii_hex(&cur),
            "ASCII85Decode" | "A85" => decode_ascii85(&cur),
            "RunLengthDecode" | "RL" => decode_run_length(&cur),
            "DCTDecode" | "DCT" => return Ok(cur),
            other => return Err(format!("Unsupported PDF stream filter: /{other}")),
        };
    }
    Ok(cur)
}

fn apply_decode_parms(
    data: &[u8],
    dict: &BTreeMap<String, Obj>,
    filter_idx: usize,
    doc: &PdfDoc<'_>,
) -> Vec<u8> {
    let Some(parms_obj) = dict.get("DecodeParms") else {
        return data.to_vec();
    };
    let resolved = doc.resolve(parms_obj);
    let arr_elem = resolved.as_array().and_then(|arr| arr.get(filter_idx)).map(|o| doc.resolve(o));
    let parms = if let Some(d) = resolved.as_dict() {
        Some(d)
    } else if let Some(ref elem) = arr_elem {
        elem.as_dict()
    } else {
        None
    };
    let Some(p) = parms else {
        return data.to_vec();
    };
    let predictor = p.get("Predictor").and_then(Obj::as_int).unwrap_or(1);
    if predictor <= 1 {
        return data.to_vec();
    }
    let columns = p.get("Columns").and_then(Obj::as_int).unwrap_or(1).max(1) as usize;
    let colors = p.get("Colors").and_then(Obj::as_int).unwrap_or(1).max(1) as usize;
    let bpc = p.get("BitsPerComponent").and_then(Obj::as_int).unwrap_or(8).max(1) as usize;
    let bpp = (colors * bpc).div_ceil(8).max(1);
    let row_bytes = (columns * colors * bpc).div_ceil(8);

    if predictor >= 10 {
        // PNG predictor: each row has 1 filter byte + row_bytes
        let stride = row_bytes + 1;
        let mut out = Vec::with_capacity(data.len());
        let mut prev = vec![0u8; row_bytes];
        for row in data.chunks(stride) {
            if row.is_empty() {
                break;
            }
            let filter_type = row[0];
            let src = &row[1..];
            let mut cur = vec![0u8; src.len()];
            for i in 0..src.len() {
                let left = if i >= bpp { cur[i - bpp] } else { 0 };
                let up = prev.get(i).copied().unwrap_or(0);
                let up_left = if i >= bpp { prev.get(i - bpp).copied().unwrap_or(0) } else { 0 };
                cur[i] = match filter_type {
                    0 => src[i],
                    1 => src[i].wrapping_add(left),
                    2 => src[i].wrapping_add(up),
                    3 => src[i].wrapping_add(((left as u16 + up as u16) / 2) as u8),
                    4 => src[i].wrapping_add(paeth(left, up, up_left)),
                    _ => src[i],
                };
            }
            out.extend_from_slice(&cur);
            prev = cur;
        }
        out
    } else if predictor == 2 {
        // TIFF Predictor 2 (horizontal differencing)
        let mut out = data.to_vec();
        for row in out.chunks_mut(row_bytes) {
            for i in bpp..row.len() {
                row[i] = row[i].wrapping_add(row[i - bpp]);
            }
        }
        out
    } else {
        data.to_vec()
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let a = a as i32;
    let b = b as i32;
    let c = c as i32;
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}

fn decode_ascii_hex(data: &[u8]) -> Vec<u8> {
    let mut nibbles = Vec::new();
    for &b in data {
        if b == b'>' {
            break;
        }
        if let Some(v) = hex_val(b) {
            nibbles.push(v);
        }
    }
    nibbles
        .chunks(2)
        .map(|p| (p[0] << 4) | p.get(1).copied().unwrap_or(0))
        .collect()
}

fn decode_ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut group = [0u32; 5];
    let mut n = 0usize;
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        i += 1;
        if b == b'~' && data.get(i) == Some(&b'>') {
            break;
        }
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'z' && n == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if (b'!'..=b'u').contains(&b) {
            group[n] = (b - b'!') as u32;
            n += 1;
            if n == 5 {
                let v = (((group[0] * 85 + group[1]) * 85 + group[2]) * 85 + group[3]) * 85 + group[4];
                out.extend_from_slice(&v.to_be_bytes());
                n = 0;
            }
        }
    }
    if n > 1 {
        for slot in &mut group[n..] {
            *slot = 84;
        }
        let v = (((group[0] * 85 + group[1]) * 85 + group[2]) * 85 + group[3]) * 85 + group[4];
        let bytes = v.to_be_bytes();
        out.extend_from_slice(&bytes[..(n - 1)]);
    }
    out
}

fn decode_run_length(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let len = data[i];
        i += 1;
        if len == 128 {
            break;
        } else if len < 128 {
            let count = len as usize + 1;
            let end = (i + count).min(data.len());
            out.extend_from_slice(&data[i..end]);
            i = end;
        } else {
            let count = 257 - len as usize;
            if let Some(&b) = data.get(i) {
                out.extend(std::iter::repeat_n(b, count));
                i += 1;
            } else {
                break;
            }
        }
    }
    out
}

// ── PDF Document & XRef Resolution ──────────────────────────────────────

#[derive(Clone, Copy)]
enum XrefEntry {
    Offset(usize),
    Compressed { stm_num: u32, index: usize },
}

struct PdfDoc<'a> {
    data: &'a [u8],
    xref: HashMap<u32, XrefEntry>,
    cache: std::cell::RefCell<HashMap<u32, Obj>>,
    objstm_cache: std::cell::RefCell<HashMap<u32, Vec<(u32, Obj)>>>,
    trailer: BTreeMap<String, Obj>,
}

impl<'a> PdfDoc<'a> {
    fn open(data: &'a [u8]) -> Result<Self, String> {
        let head = &data[..data.len().min(1024)];
        if find_bytes(head, b"%PDF-").is_none() {
            return Err("This file doesn't have a PDF header.".into());
        }
        let tail_start = data.len().saturating_sub(2048);
        let tail = &data[tail_start..];
        let sx_rel = rfind_bytes(tail, b"startxref").ok_or("Missing PDF startxref marker.")?;
        let mut p = Parser::new(data, tail_start + sx_rel + 9);
        let startxref = p
            .read_word()
            .parse::<usize>()
            .map_err(|_| "Invalid PDF startxref offset.")?;

        let mut doc = Self {
            data,
            xref: HashMap::new(),
            cache: std::cell::RefCell::new(HashMap::new()),
            objstm_cache: std::cell::RefCell::new(HashMap::new()),
            trailer: BTreeMap::new(),
        };
        doc.load_xref_chain(startxref)?;
        if doc.trailer.contains_key("Encrypt") {
            return Err("Encrypted or password-protected PDFs aren't supported yet.".into());
        }
        Ok(doc)
    }

    fn load_xref_chain(&mut self, mut offset: usize) -> Result<(), String> {
        let mut visited = Vec::new();
        while !visited.contains(&offset) {
            visited.push(offset);
            if offset >= self.data.len() {
                if visited.len() == 1 {
                    if let Some(xr) = find_classic_xref_near(self.data, self.data.len()) {
                        offset = xr;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            let mut p = Parser::new(self.data, offset);
            p.skip_ws_and_comments();
            if !p.starts_with(b"xref") {
                // Check if this points to a valid `<num> <gen> obj` XRef stream; if not, heal drifted `xref` offset!
                let saved_pos = p.pos;
                let w0 = p.read_word();
                let w1 = p.read_word();
                let w2 = p.read_word();
                let looks_like_xref_stm = !w0.is_empty()
                    && w0.bytes().all(|b| b.is_ascii_digit())
                    && !w1.is_empty()
                    && w1.bytes().all(|b| b.is_ascii_digit())
                    && w2 == "obj";
                p.pos = saved_pos;
                if !looks_like_xref_stm {
                    if let Some(xr) = find_classic_xref_near(self.data, offset) {
                        p = Parser::new(self.data, xr);
                        p.skip_ws_and_comments();
                    }
                }
            }
            if p.starts_with(b"xref") {
                p.pos += 4;
                self.parse_classic_xref(&mut p)?;
                p.skip_ws_and_comments();
                if p.starts_with(b"trailer") {
                    p.pos += 7;
                    let tr = p.read_obj()?;
                    if let Some(d) = tr.as_dict() {
                        for (k, v) in d {
                            self.trailer.entry(k.clone()).or_insert_with(|| v.clone());
                        }
                        if let Some(prev) = d.get("Prev").and_then(Obj::as_int) {
                            offset = usize::try_from(prev).unwrap_or(usize::MAX);
                            continue;
                        }
                    }
                }
                break;
            } else {
                // Cross-reference stream (PDF 1.5+)
                let _obj_num = p.read_word();
                let _gen = p.read_word();
                let _kw = p.read_word();
                let obj = p.read_obj()?;
                let Obj::Stream { dict, data } = obj else {
                    return Err("Invalid PDF cross-reference table.".into());
                };
                for (k, v) in &dict {
                    self.trailer.entry(k.clone()).or_insert_with(|| v.clone());
                }
                self.parse_xref_stream(&dict, &data)?;
                if let Some(prev) = dict.get("Prev").and_then(Obj::as_int) {
                    offset = usize::try_from(prev).unwrap_or(usize::MAX);
                    continue;
                }
                break;
            }
        }
        Ok(())
    }

    fn parse_classic_xref(&mut self, p: &mut Parser<'a>) -> Result<(), String> {
        loop {
            p.skip_ws_and_comments();
            if p.starts_with(b"trailer") || p.pos >= self.data.len() {
                break;
            }
            let first = p
                .read_word()
                .parse::<u32>()
                .map_err(|_| "Malformed PDF xref section.")?;
            let count = p
                .read_word()
                .parse::<u32>()
                .map_err(|_| "Malformed PDF xref count.")?;
            for i in 0..count {
                let off_s = p.read_word();
                let _gen_s = p.read_word();
                let kind = p.read_word();
                if kind == "n" {
                    if let Ok(off) = off_s.parse::<usize>() {
                        self.xref.entry(first + i).or_insert(XrefEntry::Offset(off));
                    }
                }
            }
        }
        Ok(())
    }

    fn parse_xref_stream(&mut self, dict: &BTreeMap<String, Obj>, raw: &[u8]) -> Result<(), String> {
        let bytes = decode_stream(dict, raw, self)?;
        let w_arr = dict
            .get("W")
            .and_then(Obj::as_array)
            .ok_or("Missing /W in XRef stream.")?;
        if w_arr.len() < 3 {
            return Err("Invalid /W in XRef stream.".into());
        }
        let w0 = w_arr[0].as_int().unwrap_or(0).max(0) as usize;
        let w1 = w_arr[1].as_int().unwrap_or(0).max(0) as usize;
        let w2 = w_arr[2].as_int().unwrap_or(0).max(0) as usize;
        let stride = w0 + w1 + w2;
        if stride == 0 {
            return Ok(());
        }
        let size = dict.get("Size").and_then(Obj::as_int).unwrap_or(0).max(0) as u32;
        let index_vals: Vec<u32> = if let Some(idx) = dict.get("Index").and_then(Obj::as_array) {
            idx.iter().filter_map(|o| o.as_int().map(|v| v.max(0) as u32)).collect()
        } else {
            vec![0, size]
        };

        let mut pos = 0usize;
        for pair in index_vals.chunks(2) {
            if pair.len() < 2 {
                break;
            }
            let start = pair[0];
            let count = pair[1];
            for i in 0..count {
                if pos + stride > bytes.len() {
                    break;
                }
                let f0 = if w0 == 0 {
                    1
                } else {
                    read_be_uint(&bytes[pos..pos + w0])
                };
                let f1 = read_be_uint(&bytes[pos + w0..pos + w0 + w1]);
                let f2 = read_be_uint(&bytes[pos + w0 + w1..pos + stride]);
                pos += stride;
                let obj_num = start + i;
                match f0 {
                    1 => {
                        self.xref.entry(obj_num).or_insert(XrefEntry::Offset(f1 as usize));
                    }
                    2 => {
                        self.xref.entry(obj_num).or_insert(XrefEntry::Compressed {
                            stm_num: f1 as u32,
                            index: f2 as usize,
                        });
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn get_obj(&self, num: u32) -> Obj {
        if let Some(cached) = self.cache.borrow().get(&num) {
            return cached.clone();
        }
        let obj = match self.xref.get(&num).copied() {
            Some(XrefEntry::Offset(off)) => {
                let mut p = Parser::new(self.data, off);
                let n_ok = p.read_word().parse::<u32>() == Ok(num);
                let _g = p.read_word();
                let kw_ok = p.read_word() == "obj";
                if n_ok && kw_ok {
                    p.read_obj().unwrap_or(Obj::Null)
                } else if let Some(repaired) = self.find_obj_body_offset(num, Some(off)) {
                    Parser::new(self.data, repaired).read_obj().unwrap_or(Obj::Null)
                } else {
                    Obj::Null
                }
            }
            Some(XrefEntry::Compressed { stm_num, index }) => self.get_compressed_obj(stm_num, index),
            None => {
                if let Some(repaired) = self.find_obj_body_offset(num, None) {
                    Parser::new(self.data, repaired).read_obj().unwrap_or(Obj::Null)
                } else {
                    Obj::Null
                }
            }
        };
        self.cache.borrow_mut().insert(num, obj.clone());
        obj
    }

    /// Scans raw PDF bytes to locate `<num> <gen> obj` when a generator wrote drifted xref offsets.
    fn find_obj_body_offset(&self, num: u32, hint: Option<usize>) -> Option<usize> {
        let needle_0 = format!("{num} 0 obj");
        let needle_b = needle_0.as_bytes();
        let check_window = |start: usize, end: usize| -> Option<usize> {
            let slice = &self.data[start..end];
            let mut rel = 0usize;
            while rel + needle_b.len() <= slice.len() {
                let Some(pos) = find_bytes(&slice[rel..], needle_b) else {
                    break;
                };
                let abs = start + rel + pos;
                let prev_ok = abs == 0 || is_delim_or_ws(self.data[abs - 1]);
                let after = abs + needle_b.len();
                let next_ok = after >= self.data.len() || is_delim_or_ws(self.data[after]);
                if prev_ok && next_ok {
                    return Some(after);
                }
                rel += pos + 1;
            }
            None
        };
        if let Some(h) = hint {
            let w0 = h.saturating_sub(1024);
            let w1 = (h + 1024).min(self.data.len());
            if let Some(found) = check_window(w0, w1) {
                return Some(found);
            }
        }
        check_window(0, self.data.len())
    }

    fn get_compressed_obj(&self, stm_num: u32, index: usize) -> Obj {
        if let Some(items) = self.objstm_cache.borrow().get(&stm_num) {
            return items.get(index).map_or(Obj::Null, |(_, o)| o.clone());
        }
        let stm = self.get_obj(stm_num);
        let Obj::Stream { dict, data } = stm else {
            return Obj::Null;
        };
        let Ok(decoded) = decode_stream(&dict, &data, self) else {
            return Obj::Null;
        };
        let n = dict.get("N").and_then(Obj::as_int).unwrap_or(0).max(0) as usize;
        let first = dict.get("First").and_then(Obj::as_int).unwrap_or(0).max(0) as usize;
        let mut hdr = Parser::new(&decoded, 0);
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            let onum = hdr.read_word().parse::<u32>().unwrap_or(0);
            let off = hdr.read_word().parse::<usize>().unwrap_or(0);
            entries.push((onum, off));
        }
        let mut parsed = Vec::with_capacity(n);
        for (onum, off) in entries {
            let mut op = Parser::new(&decoded, first + off);
            let obj = op.read_obj().unwrap_or(Obj::Null);
            parsed.push((onum, obj));
        }
        let res = parsed.get(index).map_or(Obj::Null, |(_, o)| o.clone());
        self.objstm_cache.borrow_mut().insert(stm_num, parsed);
        res
    }

    fn resolve(&self, obj: &Obj) -> Obj {
        let mut cur = obj.clone();
        for _ in 0..16 {
            if let Obj::Ref(n) = cur {
                cur = self.get_obj(n);
            } else {
                break;
            }
        }
        cur
    }

    fn dict_get(&self, dict: &BTreeMap<String, Obj>, key: &str) -> Obj {
        dict.get(key).map_or(Obj::Null, |o| self.resolve(o))
    }
}

fn find_classic_xref_near(data: &[u8], hint: usize) -> Option<usize> {
    let lo = hint.saturating_sub(8192);
    let hi = (hint + 8192).min(data.len());
    for (a, b) in [(lo, hi), (0, data.len())] {
        let slice = &data[a..b];
        let mut best: Option<usize> = None;
        let mut idx = 0usize;
        while idx + 4 <= slice.len() {
            if &slice[idx..idx + 4] == b"xref" {
                let abs = a + idx;
                let before_ok = abs == 0 || matches!(data[abs - 1], b'\r' | b'\n' | b' ' | b'\t');
                let after_ok = abs + 4 == data.len() || matches!(data[abs + 4], b'\r' | b'\n' | b' ' | b'\t');
                if before_ok && after_ok && best.is_none_or(|prev| abs.abs_diff(hint) < prev.abs_diff(hint)) {
                    best = Some(abs);
                }
                idx += 4;
            } else {
                idx += 1;
            }
        }
        if best.is_some() {
            return best;
        }
    }
    None
}

fn read_be_uint(bytes: &[u8]) -> u64 {
    let mut v = 0u64;
    for &b in bytes {
        v = (v << 8) | (b as u64);
    }
    v
}

// ── String & Encoding Tables ────────────────────────────────────────────

fn decode_pdf_text_string(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xfe, 0xff]) {
        return decode_utf16be(&bytes[2..]);
    }
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(&bytes[3..]).to_string();
    }
    bytes.iter().map(|&b| winansi_char(b)).collect()
}

fn decode_utf16be(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[hi, lo]| u16::from_be_bytes([hi, lo]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn winansi_char(b: u8) -> char {
    if !(0x80..0xa0).contains(&b) {
        return b as char;
    }
    match b {
        0x80 => '€',
        0x82 => '‚',
        0x83 => 'ƒ',
        0x84 => '„',
        0x85 => '…',
        0x86 => '†',
        0x87 => '‡',
        0x88 => 'ˆ',
        0x89 => '‰',
        0x8a => 'Š',
        0x8b => '‹',
        0x8c => 'Œ',
        0x8e => 'Ž',
        0x91 => '‘',
        0x92 => '’',
        0x93 => '“',
        0x94 => '”',
        0x95 => '•',
        0x96 => '–',
        0x97 => '—',
        0x98 => '˜',
        0x99 => '™',
        0x9a => 'š',
        0x9b => '›',
        0x9c => 'œ',
        0x9e => 'ž',
        0x9f => 'Ÿ',
        _ => b as char,
    }
}

fn macroman_char(b: u8) -> char {
    if b < 0x80 {
        return b as char;
    }
    const TABLE: [char; 128] = [
        'Ä', 'Å', 'Ç', 'É', 'Ñ', 'Ö', 'Ü', 'á', 'à', 'â', 'ä', 'ã', 'å', 'ç', 'é', 'è',
        'ê', 'ë', 'í', 'ì', 'î', 'ï', 'ñ', 'ó', 'ò', 'ô', 'ö', 'õ', 'ú', 'ù', 'û', 'ü',
        '†', '°', '¢', '£', '§', '•', '¶', 'ß', '®', '©', '™', '´', '¨', '≠', 'Æ', 'Ø',
        '∞', '±', '≤', '≥', '¥', 'µ', '∂', '∑', '∏', 'π', '∫', 'ª', 'º', 'Ω', 'æ', 'ø',
        '¿', '¡', '¬', '√', 'ƒ', '≈', '∆', '«', '»', '…', '\u{00a0}', 'À', 'Ã', 'Õ', 'Œ', 'œ',
        '–', '—', '“', '”', '‘', '’', '÷', '◊', 'ÿ', 'Ÿ', '⁄', '€', '‹', '›', 'ﬁ', 'ﬂ',
        '‡', '·', '‚', '„', '‰', 'Â', 'Ê', 'Á', 'Ë', 'È', 'Í', 'Î', 'Ï', 'Ì', 'Ó', 'Ô',
        '\u{f8ff}', 'Ò', 'Ú', 'Û', 'Ù', 'ı', 'ˆ', '˜', '¯', '˘', '˙', '˚', '¸', '˝', '˛', 'ˇ',
    ];
    TABLE[(b - 0x80) as usize]
}

fn glyph_name_to_str(name: &str) -> Option<&'static str> {
    Some(match name {
        "space" | "nonbreakingspace" | "nbspace" => " ",
        "exclam" => "!",
        "quotedbl" => "\"",
        "numbersign" => "#",
        "dollar" => "$",
        "percent" => "%",
        "ampersand" => "&",
        "quotesingle" => "'",
        "parenleft" => "(",
        "parenright" => ")",
        "asterisk" => "*",
        "plus" => "+",
        "comma" => ",",
        "hyphen" | "minus" | "sfthyphen" => "-",
        "period" => ".",
        "slash" => "/",
        "colon" => ":",
        "semicolon" => ";",
        "less" => "<",
        "equal" => "=",
        "greater" => ">",
        "question" => "?",
        "at" => "@",
        "bracketleft" => "[",
        "backslash" => "\\",
        "bracketright" => "]",
        "asciicircum" => "^",
        "underscore" => "_",
        "grave" => "`",
        "braceleft" => "{",
        "bar" => "|",
        "braceright" => "}",
        "asciitilde" => "~",
        "bullet" => "•",
        "endash" => "–",
        "emdash" => "—",
        "quoteleft" => "‘",
        "quoteright" => "’",
        "quotedblleft" => "“",
        "quotedblright" => "”",
        "quotedblbase" => "„",
        "quotesinglbase" => "‚",
        "ellipsis" => "…",
        "dagger" => "†",
        "daggerdbl" => "‡",
        "Euro" => "€",
        "trademark" => "™",
        "copyright" => "©",
        "registered" => "®",
        "degree" => "°",
        "plusminus" => "±",
        "multiply" => "×",
        "divide" => "÷",
        "fi" => "fi",
        "fl" => "fl",
        "ff" => "ff",
        "ffi" => "ffi",
        "ffl" => "ffl",
        _ => return None,
    })
}

// ── Font & ToUnicode CMap ───────────────────────────────────────────────

#[derive(Clone, Default)]
struct PdfFont {
    font_idx: u8,
    bold: bool,
    italic: bool,
    two_byte: bool,
    mac_roman: bool,
    differences: HashMap<u8, String>,
    to_unicode: HashMap<u32, String>,
    widths: HashMap<u32, f32>,
    default_width: Option<f32>,
}

impl PdfFont {
    fn decode_bytes(&self, bytes: &[u8]) -> String {
        let mut out = String::new();
        if self.two_byte {
            for pair in bytes.chunks(2) {
                let code = if pair.len() == 2 {
                    u16::from_be_bytes([pair[0], pair[1]]) as u32
                } else {
                    pair[0] as u32
                };
                if let Some(s) = self.to_unicode.get(&code) {
                    for ch in s.chars() {
                        if !matches!(ch, '\u{200b}' | '\u{feff}') {
                            out.push(ch);
                        }
                    }
                } else if (0x20..0x7f).contains(&code) {
                    out.push(code as u8 as char);
                }
            }
            return out;
        }
        for &b in bytes {
            if let Some(s) = self.to_unicode.get(&(b as u32)) {
                for ch in s.chars() {
                    if !matches!(ch, '\u{200b}' | '\u{feff}') {
                        out.push(ch);
                    }
                }
            } else if let Some(s) = self.differences.get(&b) {
                out.push_str(s);
            } else if self.mac_roman {
                out.push(macroman_char(b));
            } else {
                out.push(winansi_char(b));
            }
        }
        out
    }

    fn measure_bytes(
        &self,
        bytes: &[u8],
        font_size: f32,
        char_space: f32,
        word_space: f32,
        horiz_scale: f32,
    ) -> f32 {
        let mut total = 0.0f32;
        if self.two_byte {
            for pair in bytes.chunks(2) {
                let code = if pair.len() == 2 {
                    u16::from_be_bytes([pair[0], pair[1]]) as u32
                } else {
                    pair[0] as u32
                };
                let decoded = self.to_unicode.get(&code).map(String::as_str).unwrap_or("");
                let w_thousandths = self
                    .widths
                    .get(&code)
                    .copied()
                    .or(self.default_width)
                    .unwrap_or_else(|| approx_str_thousandths(decoded));
                let is_space = decoded == " " || code == 0x20;
                total += (w_thousandths / 1000.0) * font_size
                    + char_space
                    + if is_space { word_space } else { 0.0 };
            }
        } else {
            for &b in bytes {
                let code = b as u32;
                let w_thousandths = self
                    .widths
                    .get(&code)
                    .copied()
                    .or(self.default_width)
                    .unwrap_or_else(|| approx_char_thousandths(winansi_char(b)));
                total += (w_thousandths / 1000.0) * font_size
                    + char_space
                    + if b == b' ' { word_space } else { 0.0 };
            }
        }
        total * (horiz_scale / 100.0)
    }
}

fn approx_char_thousandths(ch: char) -> f32 {
    match ch {
        '\u{200b}' | '\u{feff}' => 0.0,
        ' ' | '\u{00a0}' => 250.0,
        'i' | 'l' | 'I' | '!' | '|' | '.' | ',' | ':' | ';' | '\'' | '’' | '‘' => 260.0,
        'f' | 'j' | 'r' | 't' | '(' | ')' | '[' | ']' | '-' => 330.0,
        'm' | 'w' => 780.0,
        'M' | 'W' | '—' => 860.0,
        c if c.is_ascii_uppercase() => 600.0,
        _ => 500.0,
    }
}

fn approx_str_thousandths(s: &str) -> f32 {
    if s.is_empty() {
        return 500.0;
    }
    s.chars().map(approx_char_thousandths).sum()
}

fn load_font(doc: &PdfDoc<'_>, font_obj: &Obj) -> PdfFont {
    let resolved = doc.resolve(font_obj);
    let Some(dict) = resolved.as_dict() else {
        return PdfFont::default();
    };
    let base_name = doc
        .dict_get(dict, "BaseFont")
        .as_name()
        .unwrap_or("Helvetica")
        .to_string();
    let lower = base_name.to_ascii_lowercase();
    let bold = lower.contains("bold")
        || lower.contains("black")
        || lower.contains("heavy")
        || lower.contains("semibold")
        || lower.contains("demi");
    let italic = lower.contains("italic") || lower.contains("oblique") || lower.contains("slant");
    let stripped = strip_font_style_suffix(&base_name);
    let font_idx = font_index(stripped);

    let subtype = doc.dict_get(dict, "Subtype").as_name().unwrap_or("").to_string();
    let enc_obj = doc.dict_get(dict, "Encoding");
    let mut mac_roman = false;
    let mut two_byte = subtype == "Type0";
    let mut differences = HashMap::new();

    if let Some(enc_name) = enc_obj.as_name() {
        mac_roman = enc_name == "MacRomanEncoding";
        if enc_name.starts_with("Identity-") {
            two_byte = true;
        }
    } else if let Some(enc_dict) = enc_obj.as_dict() {
        if let Some(base) = doc.dict_get(enc_dict, "BaseEncoding").as_name() {
            mac_roman = base == "MacRomanEncoding";
        }
        if let Some(diffs) = doc.dict_get(enc_dict, "Differences").as_array() {
            let mut code = 0u16;
            for item in diffs {
                let item = doc.resolve(item);
                if let Some(n) = item.as_int() {
                    code = n.clamp(0, 255) as u16;
                } else if let Some(gname) = item.as_name() {
                    if code < 256 {
                        if let Some(s) = glyph_name_to_str(gname) {
                            differences.insert(code as u8, s.to_string());
                        } else if gname.len() == 1 {
                            differences.insert(code as u8, gname.to_string());
                        }
                    }
                    code += 1;
                }
            }
        }
    }

    let mut widths = HashMap::new();
    let mut default_width = None;
    let first_char = doc.dict_get(dict, "FirstChar").as_int().unwrap_or(0).max(0) as u32;
    if let Some(w_arr) = doc.dict_get(dict, "Widths").as_array() {
        for (i, w_obj) in w_arr.iter().enumerate() {
            if let Some(w) = doc.resolve(w_obj).as_f32() {
                widths.insert(first_char + i as u32, w);
            }
        }
    }
    if two_byte {
        if let Some(desc_arr) = doc.dict_get(dict, "DescendantFonts").as_array() {
            if let Some(first_desc) = desc_arr.first() {
                if let Some(cid_dict) = doc.resolve(first_desc).as_dict() {
                    if let Some(dw) = doc.dict_get(cid_dict, "DW").as_f32() {
                        default_width = Some(dw);
                    }
                    if let Some(w_arr) = doc.dict_get(cid_dict, "W").as_array() {
                        parse_cid_widths(doc, w_arr, &mut widths);
                    }
                }
            }
        }
    }

    let mut to_unicode = HashMap::new();
    if let Obj::Stream { dict: tu_dict, data: tu_raw } = doc.dict_get(dict, "ToUnicode") {
        if let Ok(cmap_bytes) = decode_stream(&tu_dict, &tu_raw, doc) {
            parse_to_unicode_cmap(&cmap_bytes, &mut to_unicode);
        }
    }

    PdfFont {
        font_idx,
        bold,
        italic,
        two_byte,
        mac_roman,
        differences,
        to_unicode,
        widths,
        default_width,
    }
}

fn parse_cid_widths(doc: &PdfDoc<'_>, arr: &[Obj], widths: &mut HashMap<u32, f32>) {
    let mut i = 0;
    while i + 1 < arr.len() {
        let Some(c_first) = doc.resolve(&arr[i]).as_int().map(|n| n.max(0) as u32) else {
            i += 1;
            continue;
        };
        let second = doc.resolve(&arr[i + 1]);
        if let Some(sub) = second.as_array() {
            for (k, w_obj) in sub.iter().enumerate() {
                if let Some(w) = doc.resolve(w_obj).as_f32() {
                    widths.insert(c_first + k as u32, w);
                }
            }
            i += 2;
        } else if i + 2 < arr.len() {
            if let (Some(c_last), Some(w)) = (
                second.as_int().map(|n| n.max(0) as u32),
                doc.resolve(&arr[i + 2]).as_f32(),
            ) {
                for code in c_first..=c_last.min(c_first + 4096) {
                    widths.insert(code, w);
                }
            }
            i += 3;
        } else {
            break;
        }
    }
}

fn strip_font_style_suffix(name: &str) -> &str {
    let mut s = name.trim();
    if s.len() > 7 && s.as_bytes()[6] == b'+' && s[..6].bytes().all(|b| b.is_ascii_uppercase()) {
        s = &s[7..];
    }
    if let Some(idx) = s.find(['-', ',']) {
        s = &s[..idx];
    }
    loop {
        let trimmed = s.trim_end();
        let lower = trimmed.to_ascii_lowercase();
        let mut stripped = None;
        for suf in [" bold italic", " bold oblique", " bolditalic", " bold", " italic", " oblique", " regular"] {
            if lower.ends_with(suf) && trimmed.len() > suf.len() {
                stripped = Some(&trimmed[..trimmed.len() - suf.len()]);
                break;
            }
        }
        if let Some(next) = stripped {
            s = next;
        } else {
            return trimmed;
        }
    }
}

fn parse_to_unicode_cmap(data: &[u8], map: &mut HashMap<u32, String>) {
    let text = String::from_utf8_lossy(data);
    let tokens = cmap_tokens(&text);
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == "beginbfchar" {
            i += 1;
            while i + 1 < tokens.len() && tokens[i] != "endbfchar" {
                if let (Some(src), Some(dst)) = (parse_hex_u32(&tokens[i]), parse_cmap_dst(&tokens[i + 1])) {
                    map.insert(src, dst);
                }
                i += 2;
            }
        } else if tokens[i] == "beginbfrange" {
            i += 1;
            while i + 2 < tokens.len() && tokens[i] != "endbfrange" {
                let start = parse_hex_u32(&tokens[i]);
                let end = parse_hex_u32(&tokens[i + 1]);
                i += 2;
                if i < tokens.len() && tokens[i] == "[" {
                    i += 1;
                    let mut code = start.unwrap_or(0);
                    let max_code = end.unwrap_or(code);
                    while i < tokens.len() && tokens[i] != "]" {
                        if code <= max_code {
                            if let Some(s) = parse_cmap_dst(&tokens[i]) {
                                map.insert(code, s);
                            }
                            code += 1;
                        }
                        i += 1;
                    }
                    if i < tokens.len() && tokens[i] == "]" {
                        i += 1;
                    }
                } else if i < tokens.len() {
                    if let (Some(s), Some(e), Some(base_bytes)) = (start, end, parse_hex_bytes(&tokens[i])) {
                        let count = (e.saturating_sub(s)).min(4096);
                        for offset in 0..=count {
                            let mut cur = base_bytes.clone();
                            add_be_offset(&mut cur, offset);
                            let decoded = decode_utf16be(&cur);
                            if !decoded.is_empty() {
                                map.insert(s + offset, decoded);
                            }
                        }
                    }
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
    }
}

fn cmap_tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
        } else if b[i] == b'%' {
            while i < b.len() && b[i] != b'\n' && b[i] != b'\r' {
                i += 1;
            }
        } else if b[i] == b'<' && b.get(i + 1) == Some(&b'<') {
            out.push("<<".to_string());
            i += 2;
        } else if b[i] == b'>' && b.get(i + 1) == Some(&b'>') {
            out.push(">>".to_string());
            i += 2;
        } else if b[i] == b'<' {
            let start = i;
            i += 1;
            while i < b.len() && b[i] != b'>' {
                i += 1;
            }
            if i < b.len() {
                i += 1;
            }
            out.push(String::from_utf8_lossy(&b[start..i]).to_string());
        } else if b[i] == b'(' {
            let start = i;
            i += 1;
            let mut depth = 1i32;
            while i < b.len() && depth > 0 {
                if b[i] == b'\\' {
                    i = (i + 2).min(b.len());
                    continue;
                }
                if b[i] == b'(' {
                    depth += 1;
                } else if b[i] == b')' {
                    depth -= 1;
                }
                i += 1;
            }
            out.push(String::from_utf8_lossy(&b[start..i]).to_string());
        } else if matches!(b[i], b'[' | b']' | b'>' | b')') {
            out.push((b[i] as char).to_string());
            i += 1;
        } else {
            let start = i;
            while i < b.len()
                && !b[i].is_ascii_whitespace()
                && !matches!(b[i], b'<' | b'>' | b'[' | b']' | b'(' | b')' | b'%')
            {
                i += 1;
            }
            out.push(String::from_utf8_lossy(&b[start..i]).to_string());
        }
    }
    out
}

fn parse_hex_bytes(tok: &str) -> Option<Vec<u8>> {
    let inner = tok.strip_prefix('<')?.strip_suffix('>')?;
    let mut nibs = Vec::new();
    for &b in inner.as_bytes() {
        if let Some(v) = hex_val(b) {
            nibs.push(v);
        }
    }
    Some(
        nibs.chunks(2)
            .map(|p| (p[0] << 4) | p.get(1).copied().unwrap_or(0))
            .collect(),
    )
}

fn parse_hex_u32(tok: &str) -> Option<u32> {
    let bytes = parse_hex_bytes(tok)?;
    let mut v = 0u32;
    for b in bytes {
        v = (v << 8) | (b as u32);
    }
    Some(v)
}

fn parse_cmap_dst(tok: &str) -> Option<String> {
    let bytes = parse_hex_bytes(tok)?;
    if bytes.is_empty() {
        return None;
    }
    if bytes.len() == 1 {
        Some((bytes[0] as char).to_string())
    } else {
        Some(decode_utf16be(&bytes))
    }
}

fn add_be_offset(bytes: &mut [u8], mut offset: u32) {
    for b in bytes.iter_mut().rev() {
        let sum = (*b as u32) + (offset & 0xff);
        *b = (sum & 0xff) as u8;
        offset = (offset >> 8) + (sum >> 8);
        if offset == 0 {
            break;
        }
    }
}

// ── Content Stream Interpreter ──────────────────────────────────────────

#[derive(Clone, Debug)]
struct TextSpan {
    page: usize,
    x: f32,
    y: f32,
    width: f32,
    font: u8,
    size: f32,
    bold: bool,
    italic: bool,
    color: [u8; 3],
    text: String,
    underline: bool,
    strike: bool,
    highlight: Option<[u8; 3]>,
}

#[derive(Clone, Debug)]
struct FillRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [u8; 3],
    alpha: f32,
    path_idx: usize,
}

impl FillRect {
    fn blended_color(&self, bg: [u8; 3]) -> [u8; 3] {
        if self.alpha >= 0.99 {
            self.color
        } else {
            lerp_rgb(bg, self.color, self.alpha.clamp(0.0, 1.0))
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct StyledSeg {
    coord: f32, // y for horizontal, x for vertical
    a0: f32,    // min_x or min_y
    a1: f32,    // max_x or max_y
    width_pt: f32,
    color: [u8; 3],
    path_idx: usize,
}

#[derive(Clone, Debug)]
struct VecPath {
    subpaths: Vec<(Vec<(f32, f32)>, bool)>,
    bbox: [f32; 4], // [min_x, min_y, max_x, max_y] in PDF page coords
    fill: Option<([u8; 3], f32, bool)>,
    stroke: Option<([u8; 3], f32, f32)>,
    has_non_axis_geom: bool,
}

#[derive(Clone, Debug)]
struct PlacedImage {
    page: usize,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    png_bytes: Vec<u8>,
    seamless: bool,
}

#[derive(Clone)]
struct GState {
    ctm: [f32; 6],
    fill_rgb: [u8; 3],
    stroke_rgb: [u8; 3],
    fill_alpha: f32,
    stroke_alpha: f32,
    line_width: f32,
    font_name: String,
    font_size: f32,
    char_space: f32,
    word_space: f32,
    horiz_scale: f32,
    leading: f32,
    rise: f32,
    render_mode: i32,
}

impl Default for GState {
    fn default() -> Self {
        Self {
            ctm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            fill_rgb: [0, 0, 0],
            stroke_rgb: [0, 0, 0],
            fill_alpha: 1.0,
            stroke_alpha: 1.0,
            line_width: 1.0,
            font_name: String::new(),
            font_size: 12.0,
            char_space: 0.0,
            word_space: 0.0,
            horiz_scale: 100.0,
            leading: 0.0,
            rise: 0.0,
            render_mode: 0,
        }
    }
}

#[derive(Clone)]
enum CsTok {
    Num(f32),
    Name(String),
    Str(Vec<u8>),
    Array(Vec<CsTok>),
    Op(String),
}

fn tokenize_content(data: &[u8]) -> Vec<CsTok> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x00 | 0x0c) {
            i += 1;
        } else if b == b'%' {
            while i < data.len() && !matches!(data[i], b'\r' | b'\n') {
                i += 1;
            }
        } else if b == b'(' {
            let mut p = Parser::new(data, i);
            let bytes = p.read_lit_string();
            i = p.pos;
            out.push(CsTok::Str(bytes));
        } else if b == b'<' {
            if data.get(i + 1) == Some(&b'<') {
                let mut p = Parser::new(data, i);
                let _ = p.read_dict_or_stream();
                i = p.pos;
            } else {
                let mut p = Parser::new(data, i);
                let bytes = p.read_hex_string();
                i = p.pos;
                out.push(CsTok::Str(bytes));
            }
        } else if b == b'[' {
            i += 1;
            let mut arr = Vec::new();
            while i < data.len() {
                while i < data.len() && matches!(data[i], b' ' | b'\t' | b'\r' | b'\n' | 0x00 | 0x0c) {
                    i += 1;
                }
                if i >= data.len() {
                    break;
                }
                if data[i] == b']' {
                    i += 1;
                    break;
                }
                if data[i] == b'(' {
                    let mut p = Parser::new(data, i);
                    arr.push(CsTok::Str(p.read_lit_string()));
                    i = p.pos;
                } else if data[i] == b'<' {
                    let mut p = Parser::new(data, i);
                    arr.push(CsTok::Str(p.read_hex_string()));
                    i = p.pos;
                } else if data[i] == b'/' {
                    let mut p = Parser::new(data, i);
                    arr.push(CsTok::Name(p.read_name()));
                    i = p.pos;
                } else {
                    let start = i;
                    while i < data.len() && !is_delim_or_ws(data[i]) {
                        i += 1;
                    }
                    let w = String::from_utf8_lossy(&data[start..i]);
                    if let Ok(n) = w.parse::<f32>() {
                        arr.push(CsTok::Num(n));
                    } else if i == start {
                        i += 1;
                    }
                }
            }
            out.push(CsTok::Array(arr));
        } else if b == b'/' {
            let mut p = Parser::new(data, i);
            let name = p.read_name();
            i = p.pos;
            out.push(CsTok::Name(name));
        } else {
            let start = i;
            while i < data.len() && !is_delim_or_ws(data[i]) {
                i += 1;
            }
            if i == start {
                i += 1;
                continue;
            }
            let w = String::from_utf8_lossy(&data[start..i]).to_string();
            if w == "BI" {
                // Skip inline image header + binary data safely up to whitespace + EI + whitespace
                while i + 1 < data.len() {
                    if data[i] == b'I'
                        && data[i + 1] == b'D'
                        && (i == 0 || is_delim_or_ws(data[i - 1]))
                        && data.get(i + 2).is_none_or(|&c| matches!(c, b' ' | b'\t' | b'\r' | b'\n' | 0x00))
                    {
                        i += 3;
                        break;
                    }
                    i += 1;
                }
                while i + 1 < data.len() {
                    if data[i] == b'E'
                        && data[i + 1] == b'I'
                        && matches!(data[i.saturating_sub(1)], b' ' | b'\t' | b'\r' | b'\n' | 0x00)
                        && data.get(i + 2).is_none_or(|&c| is_delim_or_ws(c))
                    {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            if let Ok(n) = w.parse::<f32>() {
                out.push(CsTok::Num(n));
            } else {
                out.push(CsTok::Op(w));
            }
        }
    }
    out
}

fn mat_mul(a: [f32; 6], b: [f32; 6]) -> [f32; 6] {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn mat_pt(m: [f32; 6], x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

fn cmyk_to_rgb(c: f32, m: f32, y: f32, k: f32) -> [u8; 3] {
    let r = (1.0 - c) * (1.0 - k);
    let g = (1.0 - m) * (1.0 - k);
    let b = (1.0 - y) * (1.0 - k);
    [
        (r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (b.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

fn f_to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

// ── Page Extraction ─────────────────────────────────────────────────────

struct PageInfo {
    media_box: [f32; 4],
    resources: BTreeMap<String, Obj>,
    contents: Vec<u8>,
}

fn collect_pages(
    doc: &PdfDoc<'_>,
    node_obj: &Obj,
    inh_box: [f32; 4],
    inh_res: BTreeMap<String, Obj>,
    out: &mut Vec<PageInfo>,
) {
    let resolved = doc.resolve(node_obj);
    let Some(dict) = resolved.as_dict() else {
        return;
    };
    let crop_obj = doc.dict_get(dict, "CropBox");
    let media_obj = doc.dict_get(dict, "MediaBox");
    let box_vals = crop_obj
        .as_array()
        .or_else(|| media_obj.as_array())
        .and_then(|a| {
            if a.len() >= 4 {
                Some([
                    a[0].as_f32()?,
                    a[1].as_f32()?,
                    a[2].as_f32()?,
                    a[3].as_f32()?,
                ])
            } else {
                None
            }
        })
        .unwrap_or(inh_box);
    let res = doc
        .dict_get(dict, "Resources")
        .as_dict()
        .cloned()
        .unwrap_or(inh_res);

    let kind = doc.dict_get(dict, "Type").as_name().unwrap_or("").to_string();
    if kind == "Pages" || dict.contains_key("Kids") {
        if let Some(kids) = doc.dict_get(dict, "Kids").as_array() {
            for k in kids {
                collect_pages(doc, k, box_vals, res.clone(), out);
            }
        }
        return;
    }

    let mut contents = Vec::new();
    match doc.dict_get(dict, "Contents") {
        Obj::Stream { dict: cd, data: cr } => {
            if let Ok(dec) = decode_stream(&cd, &cr, doc) {
                contents.extend_from_slice(&dec);
            }
        }
        Obj::Array(arr) => {
            for item in arr {
                if let Obj::Stream { dict: cd, data: cr } = doc.resolve(&item) {
                    if let Ok(dec) = decode_stream(&cd, &cr, doc) {
                        contents.extend_from_slice(&dec);
                        contents.push(b'\n');
                    }
                }
            }
        }
        _ => {}
    }

    out.push(PageInfo { media_box: box_vals, resources: res, contents });
}

#[derive(Clone, Debug)]
struct PageGeometry {
    pw: f32,
    ph: f32,
    fill_rects: Vec<FillRect>,
    h_segs: Vec<StyledSeg>,
    v_segs: Vec<StyledSeg>,
    vec_paths: Vec<VecPath>,
    consumed_paths: Vec<bool>,
}

fn merge_resources(
    doc: &PdfDoc<'_>,
    parent: &BTreeMap<String, Obj>,
    child_dict: &BTreeMap<String, Obj>,
) -> BTreeMap<String, Obj> {
    let Some(c_res) = doc.dict_get(child_dict, "Resources").as_dict().cloned() else {
        return parent.clone();
    };
    let mut merged = parent.clone();
    for (k, v) in c_res {
        if let (Some(p_sub), Some(c_sub)) = (
            merged.get(&k).and_then(|o| doc.resolve(o).as_dict().cloned()),
            doc.resolve(&v).as_dict().cloned(),
        ) {
            let mut sub = p_sub;
            for (sk, sv) in c_sub {
                sub.insert(sk, sv);
            }
            merged.insert(k, Obj::Dict(sub));
        } else {
            merged.insert(k, v);
        }
    }
    merged
}

fn eval_cubic(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    let b0 = u * u * u;
    let b1 = 3.0 * u * u * t;
    let b2 = 3.0 * u * t * t;
    let b3 = t * t * t;
    (
        b0 * p0.0 + b1 * p1.0 + b2 * p2.0 + b3 * p3.0,
        b0 * p0.1 + b1 * p1.1 + b2 * p2.1 + b3 * p3.1,
    )
}

#[allow(clippy::too_many_arguments)]
fn interpret_stream(
    doc: &PdfDoc<'_>,
    page_idx: usize,
    resources: &BTreeMap<String, Obj>,
    content_bytes: &[u8],
    init_gs: GState,
    fill_rects: &mut Vec<FillRect>,
    h_segs: &mut Vec<StyledSeg>,
    v_segs: &mut Vec<StyledSeg>,
    vec_paths: &mut Vec<VecPath>,
    page_spans: &mut Vec<TextSpan>,
    page_imgs: &mut Vec<PlacedImage>,
    depth: usize,
) {
    let mut fonts: HashMap<String, PdfFont> = HashMap::new();
    if let Some(fdict) = doc.dict_get(resources, "Font").as_dict() {
        for (name, fobj) in fdict {
            fonts.insert(name.clone(), load_font(doc, fobj));
        }
    }
    let xobjects = doc
        .dict_get(resources, "XObject")
        .as_dict()
        .cloned()
        .unwrap_or_default();
    let ext_gstates = doc
        .dict_get(resources, "ExtGState")
        .as_dict()
        .cloned()
        .unwrap_or_default();

    let toks = tokenize_content(content_bytes);
    let mut stack: Vec<CsTok> = Vec::new();
    let mut gs = init_gs;
    let mut gs_stack: Vec<GState> = Vec::new();
    let mut tm = [1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut tlm = tm;

    let mut pending_rects: Vec<[f32; 4]> = Vec::new();
    let mut subpaths: Vec<(Vec<(f32, f32)>, bool)> = Vec::new();
    let mut cur_pts: Vec<(f32, f32)> = Vec::new();
    let mut has_curves = false;

    let pop_num = |st: &mut Vec<CsTok>| -> f32 {
        while let Some(t) = st.pop() {
            if let CsTok::Num(n) = t {
                return n;
            }
        }
        0.0
    };

    for tok in toks {
        let CsTok::Op(op) = tok else {
            stack.push(tok);
            continue;
        };
        match op.as_str() {
            "q" => gs_stack.push(gs.clone()),
            "Q" => {
                if let Some(prev) = gs_stack.pop() {
                    gs = prev;
                }
            }
            "cm" => {
                let f = pop_num(&mut stack);
                let e = pop_num(&mut stack);
                let d = pop_num(&mut stack);
                let c = pop_num(&mut stack);
                let b = pop_num(&mut stack);
                let a = pop_num(&mut stack);
                gs.ctm = mat_mul([a, b, c, d, e, f], gs.ctm);
            }
            "w" => {
                gs.line_width = pop_num(&mut stack).abs().max(0.15);
            }
            "gs" => {
                if let Some(CsTok::Name(gname)) = stack.pop() {
                    if let Some(gobj) = ext_gstates.get(&gname) {
                        if let Some(gd) = doc.resolve(gobj).as_dict() {
                            if let Some(ca) = doc.dict_get(gd, "ca").as_f32() {
                                gs.fill_alpha = ca.clamp(0.0, 1.0);
                            }
                            if let Some(sca) = doc.dict_get(gd, "CA").as_f32() {
                                gs.stroke_alpha = sca.clamp(0.0, 1.0);
                            }
                            if let Some(lw) = doc.dict_get(gd, "LW").as_f32() {
                                gs.line_width = lw.abs().max(0.15);
                            }
                        }
                    }
                }
            }
            "rg" => {
                let b = f_to_u8(pop_num(&mut stack));
                let g = f_to_u8(pop_num(&mut stack));
                let r = f_to_u8(pop_num(&mut stack));
                gs.fill_rgb = [r, g, b];
            }
            "RG" => {
                let b = f_to_u8(pop_num(&mut stack));
                let g = f_to_u8(pop_num(&mut stack));
                let r = f_to_u8(pop_num(&mut stack));
                gs.stroke_rgb = [r, g, b];
            }
            "g" => {
                let v = f_to_u8(pop_num(&mut stack));
                gs.fill_rgb = [v, v, v];
            }
            "G" => {
                let v = f_to_u8(pop_num(&mut stack));
                gs.stroke_rgb = [v, v, v];
            }
            "k" => {
                let k = pop_num(&mut stack);
                let y = pop_num(&mut stack);
                let m = pop_num(&mut stack);
                let c = pop_num(&mut stack);
                gs.fill_rgb = cmyk_to_rgb(c, m, y, k);
            }
            "K" => {
                let k = pop_num(&mut stack);
                let y = pop_num(&mut stack);
                let m = pop_num(&mut stack);
                let c = pop_num(&mut stack);
                gs.stroke_rgb = cmyk_to_rgb(c, m, y, k);
            }
            "sc" | "scn" => {
                let nums: Vec<f32> = stack
                    .iter()
                    .filter_map(|t| if let CsTok::Num(n) = t { Some(*n) } else { None })
                    .collect();
                if nums.len() >= 4 {
                    let n = nums.len();
                    gs.fill_rgb = cmyk_to_rgb(nums[n - 4], nums[n - 3], nums[n - 2], nums[n - 1]);
                } else if nums.len() == 3 {
                    gs.fill_rgb = [f_to_u8(nums[0]), f_to_u8(nums[1]), f_to_u8(nums[2])];
                } else if nums.len() == 1 {
                    let v = f_to_u8(nums[0]);
                    gs.fill_rgb = [v, v, v];
                }
            }
            "SC" | "SCN" => {
                let nums: Vec<f32> = stack
                    .iter()
                    .filter_map(|t| if let CsTok::Num(n) = t { Some(*n) } else { None })
                    .collect();
                if nums.len() >= 4 {
                    let n = nums.len();
                    gs.stroke_rgb = cmyk_to_rgb(nums[n - 4], nums[n - 3], nums[n - 2], nums[n - 1]);
                } else if nums.len() == 3 {
                    gs.stroke_rgb = [f_to_u8(nums[0]), f_to_u8(nums[1]), f_to_u8(nums[2])];
                } else if nums.len() == 1 {
                    let v = f_to_u8(nums[0]);
                    gs.stroke_rgb = [v, v, v];
                }
            }
            "BT" => {
                tm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
                tlm = tm;
            }
            "ET" => {}
            "Tr" => {
                gs.render_mode = pop_num(&mut stack).round() as i32;
            }
            "Tf" => {
                let sz = pop_num(&mut stack);
                if let Some(CsTok::Name(name)) = stack.pop() {
                    gs.font_name = name;
                }
                gs.font_size = sz.abs().max(1.0);
            }
            "Tc" => gs.char_space = pop_num(&mut stack),
            "Tw" => gs.word_space = pop_num(&mut stack),
            "Tz" => gs.horiz_scale = pop_num(&mut stack).max(1.0),
            "TL" => gs.leading = pop_num(&mut stack),
            "Ts" => gs.rise = pop_num(&mut stack),
            "Tm" => {
                let f = pop_num(&mut stack);
                let e = pop_num(&mut stack);
                let d = pop_num(&mut stack);
                let c = pop_num(&mut stack);
                let b = pop_num(&mut stack);
                let a = pop_num(&mut stack);
                tm = [a, b, c, d, e, f];
                tlm = tm;
            }
            "Td" => {
                let ty = pop_num(&mut stack);
                let tx = pop_num(&mut stack);
                tlm = mat_mul([1.0, 0.0, 0.0, 1.0, tx, ty], tlm);
                tm = tlm;
            }
            "TD" => {
                let ty = pop_num(&mut stack);
                let tx = pop_num(&mut stack);
                gs.leading = -ty;
                tlm = mat_mul([1.0, 0.0, 0.0, 1.0, tx, ty], tlm);
                tm = tlm;
            }
            "T*" => {
                tlm = mat_mul([1.0, 0.0, 0.0, 1.0, 0.0, -gs.leading], tlm);
                tm = tlm;
            }
            "Tj" | "'" | "\"" => {
                if op == "'" || op == "\"" {
                    tlm = mat_mul([1.0, 0.0, 0.0, 1.0, 0.0, -gs.leading], tlm);
                    tm = tlm;
                }
                let raw = match stack.pop() {
                    Some(CsTok::Str(b)) => b,
                    _ => Vec::new(),
                };
                if op == "\"" {
                    gs.char_space = pop_num(&mut stack);
                    gs.word_space = pop_num(&mut stack);
                }
                let font = fonts.get(&gs.font_name);
                let text = font.map_or_else(|| decode_pdf_text_string(&raw), |f| f.decode_bytes(&raw));
                emit_text_span(page_idx, &gs, &mut tm, font, &raw, text, page_spans);
            }
            "TJ" => {
                if let Some(CsTok::Array(items)) = stack.pop() {
                    let font = fonts.get(&gs.font_name);
                    let mut combined = String::new();
                    let mut raw_concat = Vec::new();
                    let mut extra_kern_pts = 0.0f32;
                    let eff_size = gs.font_size
                        * (tm[0].hypot(tm[1])).max(0.01)
                        * (gs.ctm[0].hypot(gs.ctm[1])).max(0.01);
                    for item in items {
                        match item {
                            CsTok::Str(b) => {
                                let s = font.map_or_else(|| decode_pdf_text_string(&b), |f| f.decode_bytes(&b));
                                combined.push_str(&s);
                                raw_concat.extend_from_slice(&b);
                            }
                            CsTok::Num(adj) => {
                                let shift_pts = -(adj / 1000.0) * eff_size * (gs.horiz_scale / 100.0);
                                extra_kern_pts += shift_pts;
                                if adj < -180.0 && !combined.ends_with(' ') && !combined.is_empty() {
                                    combined.push(' ');
                                }
                            }
                            _ => {}
                        }
                    }
                    emit_text_span(page_idx, &gs, &mut tm, font, &raw_concat, combined, page_spans);
                    if let Some(last) = page_spans.last_mut() {
                        last.width = (last.width + extra_kern_pts).max(0.0);
                    }
                    tm[4] += extra_kern_pts / (gs.ctm[0].hypot(gs.ctm[1])).max(0.01);
                }
            }
            "re" => {
                if cur_pts.len() >= 2 {
                    subpaths.push((std::mem::take(&mut cur_pts), false));
                } else {
                    cur_pts.clear();
                }
                let h = pop_num(&mut stack);
                let w = pop_num(&mut stack);
                let y = pop_num(&mut stack);
                let x = pop_num(&mut stack);
                pending_rects.push([x, y, w, h]);
            }
            "m" => {
                if cur_pts.len() >= 2 {
                    subpaths.push((std::mem::take(&mut cur_pts), false));
                } else {
                    cur_pts.clear();
                }
                let y = pop_num(&mut stack);
                let x = pop_num(&mut stack);
                cur_pts.push((x, y));
            }
            "l" => {
                let y = pop_num(&mut stack);
                let x = pop_num(&mut stack);
                cur_pts.push((x, y));
            }
            "c" => {
                let y3 = pop_num(&mut stack);
                let x3 = pop_num(&mut stack);
                let y2 = pop_num(&mut stack);
                let x2 = pop_num(&mut stack);
                let y1 = pop_num(&mut stack);
                let x1 = pop_num(&mut stack);
                if let Some(&p0) = cur_pts.last() {
                    has_curves = true;
                    for step in 1..=12 {
                        cur_pts.push(eval_cubic(p0, (x1, y1), (x2, y2), (x3, y3), step as f32 / 12.0));
                    }
                } else {
                    cur_pts.push((x3, y3));
                }
            }
            "v" => {
                let y3 = pop_num(&mut stack);
                let x3 = pop_num(&mut stack);
                let y2 = pop_num(&mut stack);
                let x2 = pop_num(&mut stack);
                if let Some(&p0) = cur_pts.last() {
                    has_curves = true;
                    for step in 1..=12 {
                        cur_pts.push(eval_cubic(p0, p0, (x2, y2), (x3, y3), step as f32 / 12.0));
                    }
                } else {
                    cur_pts.push((x3, y3));
                }
            }
            "y" => {
                let y3 = pop_num(&mut stack);
                let x3 = pop_num(&mut stack);
                let y1 = pop_num(&mut stack);
                let x1 = pop_num(&mut stack);
                if let Some(&p0) = cur_pts.last() {
                    has_curves = true;
                    for step in 1..=12 {
                        cur_pts.push(eval_cubic(p0, (x1, y1), (x3, y3), (x3, y3), step as f32 / 12.0));
                    }
                } else {
                    cur_pts.push((x3, y3));
                }
            }
            "h" => {
                if cur_pts.len() >= 2 {
                    subpaths.push((std::mem::take(&mut cur_pts), true));
                } else {
                    cur_pts.clear();
                }
            }
            "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "S" | "s" => {
                let close_cur = matches!(op.as_str(), "b" | "b*" | "s");
                if cur_pts.len() >= 2 {
                    subpaths.push((std::mem::take(&mut cur_pts), close_cur));
                } else {
                    cur_pts.clear();
                }
                let do_fill = matches!(op.as_str(), "f" | "F" | "f*" | "B" | "B*" | "b" | "b*");
                let do_stroke = matches!(op.as_str(), "S" | "s" | "B" | "B*" | "b" | "b*");
                let even_odd = matches!(op.as_str(), "f*" | "B*" | "b*");
                let ctm_scale = (gs.ctm[0].hypot(gs.ctm[1])).max(0.1);
                let eff_lw = (gs.line_width * ctm_scale).clamp(0.25, 18.0);
                let eff_stroke_col = lerp_rgb([255, 255, 255], gs.stroke_rgb, gs.stroke_alpha);

                // Emit pending `re` rectangles
                for [rx, ry, rw, rh] in pending_rects.drain(..) {
                    let (p00x, p00y) = mat_pt(gs.ctm, rx, ry);
                    let (p10x, p10y) = mat_pt(gs.ctm, rx + rw, ry);
                    let (p11x, p11y) = mat_pt(gs.ctm, rx + rw, ry + rh);
                    let (p01x, p01y) = mat_pt(gs.ctm, rx, ry + rh);
                    let min_x = p00x.min(p10x).min(p11x).min(p01x);
                    let max_x = p00x.max(p10x).max(p11x).max(p01x);
                    let min_y = p00y.min(p10y).min(p11y).min(p01y);
                    let max_y = p00y.max(p10y).max(p11y).max(p01y);
                    let bw = max_x - min_x;
                    let bh = max_y - min_y;
                    let rotated = (p00y - p10y).abs() > 1.0 || (p00x - p01x).abs() > 1.0;

                    let pidx = vec_paths.len();
                    vec_paths.push(VecPath {
                        subpaths: vec![(vec![(p00x, p00y), (p10x, p10y), (p11x, p11y), (p01x, p01y)], true)],
                        bbox: [min_x, min_y, max_x, max_y],
                        fill: do_fill.then_some((gs.fill_rgb, gs.fill_alpha, even_odd)),
                        stroke: do_stroke.then_some((gs.stroke_rgb, gs.stroke_alpha, eff_lw)),
                        has_non_axis_geom: rotated,
                    });

                    if !rotated && do_fill && gs.fill_alpha > 0.02 {
                        let fcol = lerp_rgb([255, 255, 255], gs.fill_rgb, gs.fill_alpha);
                        fill_rects.push(FillRect {
                            x: min_x,
                            y: min_y,
                            w: bw,
                            h: bh,
                            color: gs.fill_rgb,
                            alpha: gs.fill_alpha,
                            path_idx: pidx,
                        });
                        // Thin filled rectangles frequently act as table borders or horizontal rules
                        if bh <= 3.6 && bw >= 8.0 && fcol != [255, 255, 255] {
                            h_segs.push(StyledSeg {
                                coord: min_y + bh * 0.5,
                                a0: min_x,
                                a1: max_x,
                                width_pt: bh.max(0.4),
                                color: fcol,
                                path_idx: pidx,
                            });
                        } else if bw <= 3.6 && bh >= 6.0 && fcol != [255, 255, 255] {
                            v_segs.push(StyledSeg {
                                coord: min_x + bw * 0.5,
                                a0: min_y,
                                a1: max_y,
                                width_pt: bw.max(0.4),
                                color: fcol,
                                path_idx: pidx,
                            });
                        }
                    }
                    if !rotated && do_stroke && gs.stroke_alpha > 0.02 {
                        if bw >= 8.0 {
                            h_segs.push(StyledSeg {
                                coord: min_y,
                                a0: min_x,
                                a1: max_x,
                                width_pt: eff_lw,
                                color: eff_stroke_col,
                                path_idx: pidx,
                            });
                            h_segs.push(StyledSeg {
                                coord: max_y,
                                a0: min_x,
                                a1: max_x,
                                width_pt: eff_lw,
                                color: eff_stroke_col,
                                path_idx: pidx,
                            });
                        }
                        if bh >= 6.0 {
                            v_segs.push(StyledSeg {
                                coord: min_x,
                                a0: min_y,
                                a1: max_y,
                                width_pt: eff_lw,
                                color: eff_stroke_col,
                                path_idx: pidx,
                            });
                            v_segs.push(StyledSeg {
                                coord: max_x,
                                a0: min_y,
                                a1: max_y,
                                width_pt: eff_lw,
                                color: eff_stroke_col,
                                path_idx: pidx,
                            });
                        }
                    }
                }

                // Emit general subpaths (`m`, `l`, `c`, `v`, `y`, `h`)
                if !subpaths.is_empty() {
                    let mut xformed: Vec<(Vec<(f32, f32)>, bool)> = Vec::with_capacity(subpaths.len());
                    let mut min_x = f32::INFINITY;
                    let mut max_x = f32::NEG_INFINITY;
                    let mut min_y = f32::INFINITY;
                    let mut max_y = f32::NEG_INFINITY;
                    let mut non_axis = has_curves;

                    for (pts, closed) in subpaths.drain(..) {
                        let mut tpts = Vec::with_capacity(pts.len());
                        for &(ux, uy) in &pts {
                            let (px, py) = mat_pt(gs.ctm, ux, uy);
                            min_x = min_x.min(px);
                            max_x = max_x.max(px);
                            min_y = min_y.min(py);
                            max_y = max_y.max(py);
                            tpts.push((px, py));
                        }
                        for w in tpts.windows(2) {
                            let dx = (w[1].0 - w[0].0).abs();
                            let dy = (w[1].1 - w[0].1).abs();
                            if dx > 1.5 && dy > 1.5 {
                                non_axis = true;
                            }
                        }
                        if closed && tpts.len() >= 2 {
                            let first = tpts[0];
                            let last = *tpts.last().unwrap();
                            if (first.0 - last.0).abs() > 1.5 && (first.1 - last.1).abs() > 1.5 {
                                non_axis = true;
                            }
                        }
                        xformed.push((tpts, closed));
                    }

                    if min_x.is_finite() {
                        let pidx = vec_paths.len();
                        let bw = max_x - min_x;
                        let bh = max_y - min_y;

                        // Check if this is an axis-aligned filled quad (`m l l l h f`)
                        if !non_axis && do_fill && gs.fill_alpha > 0.02 && xformed.len() == 1 && xformed[0].0.len() >= 3 && xformed[0].0.len() <= 5 {
                            if bw > 0.4 && bh > 0.4 {
                                let fcol = lerp_rgb([255, 255, 255], gs.fill_rgb, gs.fill_alpha);
                                fill_rects.push(FillRect {
                                    x: min_x,
                                    y: min_y,
                                    w: bw,
                                    h: bh,
                                    color: gs.fill_rgb,
                                    alpha: gs.fill_alpha,
                                    path_idx: pidx,
                                });
                                if bh <= 3.6 && bw >= 8.0 && fcol != [255, 255, 255] {
                                    h_segs.push(StyledSeg {
                                        coord: min_y + bh * 0.5,
                                        a0: min_x,
                                        a1: max_x,
                                        width_pt: bh.max(0.4),
                                        color: fcol,
                                        path_idx: pidx,
                                    });
                                } else if bw <= 3.6 && bh >= 6.0 && fcol != [255, 255, 255] {
                                    v_segs.push(StyledSeg {
                                        coord: min_x + bw * 0.5,
                                        a0: min_y,
                                        a1: max_y,
                                        width_pt: bw.max(0.4),
                                        color: fcol,
                                        path_idx: pidx,
                                    });
                                }
                            }
                        }

                        if do_stroke && gs.stroke_alpha > 0.02 {
                            for (tpts, closed) in &xformed {
                                let mut iter_segs: Vec<((f32, f32), (f32, f32))> =
                                    tpts.windows(2).map(|w| (w[0], w[1])).collect();
                                if *closed && tpts.len() >= 3 {
                                    iter_segs.push((*tpts.last().unwrap(), tpts[0]));
                                }
                                for ((ax0, ay0), (ax1, ay1)) in iter_segs {
                                    if (ay0 - ay1).abs() < 1.5 && (ax1 - ax0).abs() > 2.0 {
                                        let y_mid = (ay0 + ay1) * 0.5;
                                        let x_min = ax0.min(ax1);
                                        let x_max = ax0.max(ax1);
                                        fill_rects.push(FillRect {
                                            x: x_min,
                                            y: y_mid - eff_lw * 0.5,
                                            w: x_max - x_min,
                                            h: eff_lw.max(0.8),
                                            color: gs.stroke_rgb,
                                            alpha: gs.stroke_alpha,
                                            path_idx: pidx,
                                        });
                                        if x_max - x_min >= 8.0 {
                                            h_segs.push(StyledSeg {
                                                coord: y_mid,
                                                a0: x_min,
                                                a1: x_max,
                                                width_pt: eff_lw,
                                                color: eff_stroke_col,
                                                path_idx: pidx,
                                            });
                                        }
                                    } else if (ax0 - ax1).abs() < 1.5 && (ay1 - ay0).abs() >= 6.0 {
                                        v_segs.push(StyledSeg {
                                            coord: (ax0 + ax1) * 0.5,
                                            a0: ay0.min(ay1),
                                            a1: ay0.max(ay1),
                                            width_pt: eff_lw,
                                            color: eff_stroke_col,
                                            path_idx: pidx,
                                        });
                                    }
                                }
                            }
                        }

                        vec_paths.push(VecPath {
                            subpaths: xformed,
                            bbox: [min_x, min_y, max_x, max_y],
                            fill: do_fill.then_some((gs.fill_rgb, gs.fill_alpha, even_odd)),
                            stroke: do_stroke.then_some((gs.stroke_rgb, gs.stroke_alpha, eff_lw)),
                            has_non_axis_geom: non_axis,
                        });
                    }
                }
                has_curves = false;
            }
            "n" | "W" | "W*" => {
                if op == "n" {
                    pending_rects.clear();
                    subpaths.clear();
                    cur_pts.clear();
                    has_curves = false;
                }
            }
            "Do" => {
                if let Some(CsTok::Name(xname)) = stack.pop() {
                    if let Some(xobj) = xobjects.get(&xname) {
                        if let Obj::Stream { dict: xd, data: xr } = doc.resolve(xobj) {
                            let subtype = doc.dict_get(&xd, "Subtype").as_name().unwrap_or("").to_string();
                            if subtype == "Image" {
                                let (p00x, p00y) = mat_pt(gs.ctm, 0.0, 0.0);
                                let (p10x, p10y) = mat_pt(gs.ctm, 1.0, 0.0);
                                let (p01x, p01y) = mat_pt(gs.ctm, 0.0, 1.0);
                                let (p11x, p11y) = mat_pt(gs.ctm, 1.0, 1.0);
                                let min_x = p00x.min(p10x).min(p01x).min(p11x);
                                let max_x = p00x.max(p10x).max(p01x).max(p11x);
                                let min_y = p00y.min(p10y).min(p01y).min(p11y);
                                let max_y = p00y.max(p10y).max(p01y).max(p11y);
                                let iw = max_x - min_x;
                                let ih = max_y - min_y;
                                if iw > 2.0 && ih > 2.0 {
                                    if let Some(img) = extract_image(doc, &xd, &xr, page_idx, min_x, min_y, iw, ih, gs.fill_rgb) {
                                        page_imgs.push(img);
                                    }
                                }
                            } else if subtype == "Form" && depth < 8 {
                                if let Ok(form_bytes) = decode_stream(&xd, &xr, doc) {
                                    let mut child_gs = gs.clone();
                                    if let Some(m_arr) = doc.dict_get(&xd, "Matrix").as_array() {
                                        if m_arr.len() >= 6 {
                                            if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
                                                m_arr[0].as_f32(),
                                                m_arr[1].as_f32(),
                                                m_arr[2].as_f32(),
                                                m_arr[3].as_f32(),
                                                m_arr[4].as_f32(),
                                                m_arr[5].as_f32(),
                                            ) {
                                                child_gs.ctm = mat_mul([a, b, c, d, e, f], child_gs.ctm);
                                            }
                                        }
                                    }
                                    let child_res = merge_resources(doc, resources, &xd);
                                    interpret_stream(
                                        doc,
                                        page_idx,
                                        &child_res,
                                        &form_bytes,
                                        child_gs,
                                        fill_rects,
                                        h_segs,
                                        v_segs,
                                        vec_paths,
                                        page_spans,
                                        page_imgs,
                                        depth + 1,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        stack.clear();
    }
}

fn interpret_page(
    doc: &PdfDoc<'_>,
    page_idx: usize,
    page: &PageInfo,
    is_scribe_export: bool,
    spans_out: &mut Vec<TextSpan>,
    images_out: &mut Vec<PlacedImage>,
    page_bg_out: &mut Option<[u8; 3]>,
) -> PageGeometry {
    let bx0 = page.media_box[0].min(page.media_box[2]);
    let by0 = page.media_box[1].min(page.media_box[3]);
    let pw = (page.media_box[2] - page.media_box[0]).abs().max(72.0);
    let ph = (page.media_box[3] - page.media_box[1]).abs().max(72.0);

    let mut fill_rects: Vec<FillRect> = Vec::new();
    let mut h_segs: Vec<StyledSeg> = Vec::new();
    let mut v_segs: Vec<StyledSeg> = Vec::new();
    let mut vec_paths: Vec<VecPath> = Vec::new();
    let mut page_spans: Vec<TextSpan> = Vec::new();
    let mut page_imgs: Vec<PlacedImage> = Vec::new();

    let mut init_gs = GState::default();
    if bx0.abs() > 0.1 || by0.abs() > 0.1 {
        init_gs.ctm = [1.0, 0.0, 0.0, 1.0, -bx0, -by0];
    }

    interpret_stream(
        doc,
        page_idx,
        &page.resources,
        &page.contents,
        init_gs,
        &mut fill_rects,
        &mut h_segs,
        &mut v_segs,
        &mut vec_paths,
        &mut page_spans,
        &mut page_imgs,
        0,
    );

    let mut consumed_paths = vec![false; vec_paths.len()];

    // Detect full-page background color and mark full-page background rects consumed
    let mut cur_bg = page_bg_out.unwrap_or([255, 255, 255]);
    for r in &fill_rects {
        if r.w >= pw * 0.88 && r.h >= ph * 0.88 {
            if r.path_idx < consumed_paths.len() {
                consumed_paths[r.path_idx] = true;
            }
            if page_idx == 0 && page_bg_out.is_none() {
                let c = r.blended_color([255, 255, 255]);
                if c != [255, 255, 255] {
                    *page_bg_out = Some(c);
                    cur_bg = c;
                }
            }
        }
    }

    // Apply underline, strikethrough, and tight inline highlight rectangles to spans on this page
    for r in &fill_rects {
        if r.w >= pw * 0.88 && r.h >= ph * 0.88 {
            continue;
        }
        let eff_c = r.blended_color(cur_bg);
        // Check thin underline / strikethrough bars first
        let mut used_as_decor = false;
        for sp in &mut page_spans {
            let overlap_x = ((sp.x + sp.width).min(r.x + r.w) - sp.x.max(r.x)).max(0.0);
            if overlap_x < (sp.width * 0.5).min(6.0) {
                continue;
            }
            if r.h <= (sp.size * 0.25).max(2.5) && r.w <= sp.width * 1.45 + 14.0 {
                let mid_y = r.y + r.h * 0.5;
                let dy = mid_y - sp.y;
                if dy >= -sp.size * 0.35 && dy <= sp.size * 0.08 {
                    sp.underline = true;
                    used_as_decor = true;
                } else if dy > sp.size * 0.08 && dy <= sp.size * 0.55 {
                    sp.strike = true;
                    used_as_decor = true;
                }
            }
        }
        if used_as_decor {
            if r.path_idx < consumed_paths.len() {
                consumed_paths[r.path_idx] = true;
            }
            continue;
        }

        // Tight inline highlight / badge pill behind text spans (requiring text width >= 58% of rect width
        // so chart bars and wide table cells are not misclassified as inline text highlights)
        if eff_c != cur_bg && r.w < pw * 0.45 && r.w <= 220.0 {
            let mut sum_ov_w = 0.0f32;
            let mut matched_indices = Vec::new();
            for (si, sp) in page_spans.iter().enumerate() {
                if r.h >= sp.size * 0.65
                    && r.h <= sp.size * 2.25
                    && r.y <= sp.y + sp.size * 0.4
                    && r.y + r.h >= sp.y + sp.size * 0.2
                {
                    let ov = ((sp.x + sp.width).min(r.x + r.w) - sp.x.max(r.x)).max(0.0);
                    if ov >= (sp.width * 0.5).min(6.0) {
                        sum_ov_w += ov;
                        matched_indices.push(si);
                    }
                }
            }
            if !matched_indices.is_empty() && sum_ov_w >= r.w * 0.58 {
                for si in matched_indices {
                    page_spans[si].highlight = Some(eff_c);
                }
                if r.path_idx < consumed_paths.len() {
                    consumed_paths[r.path_idx] = true;
                }
            }
        }
    }

    // Skip full-page WYSIWYG overlay images ONLY on Scribe Pro's own `/Im1` hybrid exports
    let has_text = page_spans.iter().any(|s| !s.text.trim().is_empty());
    for img in page_imgs {
        let is_full_page = img.w >= pw * 0.92 && img.h >= ph * 0.92;
        if !(is_scribe_export && has_text && is_full_page) {
            images_out.push(img);
        }
    }

    spans_out.extend(page_spans);
    PageGeometry {
        pw,
        ph,
        fill_rects,
        h_segs,
        v_segs,
        vec_paths,
        consumed_paths,
    }
}

fn emit_text_span(
    page: usize,
    gs: &GState,
    tm: &mut [f32; 6],
    font: Option<&PdfFont>,
    raw_bytes: &[u8],
    text: String,
    out: &mut Vec<TextSpan>,
) {
    let combined = mat_mul(*tm, gs.ctm);
    let (x, y) = mat_pt(combined, 0.0, gs.rise);
    let ctm_scale = (gs.ctm[0].hypot(gs.ctm[1])).max(0.01);
    let tm_scale = (tm[0].hypot(tm[1])).max(0.01);
    let eff_size = (gs.font_size * tm_scale * ctm_scale).clamp(4.0, 144.0);
    let adv_pts = if let Some(f) = font {
        f.measure_bytes(
            raw_bytes,
            eff_size,
            gs.char_space * ctm_scale,
            gs.word_space * ctm_scale,
            gs.horiz_scale,
        )
    } else {
        (approx_str_thousandths(&text) / 1000.0) * eff_size * (gs.horiz_scale / 100.0)
    };
    tm[4] += adv_pts / ctm_scale;
    if text.is_empty() || gs.render_mode == 3 {
        return;
    }
    let (font_idx, mut bold, italic) = font.map_or((2, false, false), |f| (f.font_idx, f.bold, f.italic));
    if gs.render_mode == 2 {
        bold = true;
    }
    let color = if gs.render_mode == 1 { gs.stroke_rgb } else { gs.fill_rgb };
    out.push(TextSpan {
        page,
        x,
        y,
        width: adv_pts.max(0.0),
        font: font_idx,
        size: eff_size,
        bold,
        italic,
        color,
        text,
        underline: false,
        strike: false,
        highlight: None,
    });
}

fn stream_has_dct(doc: &PdfDoc<'_>, dict: &BTreeMap<String, Obj>) -> bool {
    let f = doc.dict_get(dict, "Filter");
    if let Some(name) = f.as_name() {
        return name == "DCTDecode" || name == "DCT";
    }
    if let Some(arr) = f.as_array() {
        return arr
            .iter()
            .any(|o| matches!(doc.resolve(o).as_name(), Some("DCTDecode" | "DCT")));
    }
    false
}

fn resolve_color_channels_and_palette(
    doc: &PdfDoc<'_>,
    cs_obj: &Obj,
) -> Option<(u8, Option<(u8, Vec<u8>)>)> {
    let cs = doc.resolve(cs_obj);
    if let Some(name) = cs.as_name() {
        return match name {
            "DeviceGray" | "G" => Some((1, None)),
            "DeviceRGB" | "RGB" => Some((3, None)),
            "DeviceCMYK" | "CMYK" => Some((4, None)),
            _ => None,
        };
    }
    let arr = cs.as_array()?;
    let tag = doc.resolve(arr.first()?).as_name()?.to_string();
    match tag.as_str() {
        "CalRGB" | "Lab" => Some((3, None)),
        "CalGray" => Some((1, None)),
        "ICCBased" => {
            let stm = doc.resolve(arr.get(1)?);
            if let Obj::Stream { dict, .. } = stm {
                let n = doc.dict_get(&dict, "N").as_int().unwrap_or(3).clamp(1, 4) as u8;
                Some((n, None))
            } else {
                Some((3, None))
            }
        }
        "Separation" | "DeviceN" => Some((1, None)),
        "Indexed" | "I" => {
            let (base_ch, _) = resolve_color_channels_and_palette(doc, arr.get(1)?)?;
            let lookup_obj = doc.resolve(arr.get(3)?);
            let pal_bytes = match lookup_obj {
                Obj::Str(b) => b,
                Obj::Stream { dict, data } => decode_stream(&dict, &data, doc).ok()?,
                _ => return None,
            };
            Some((1, Some((base_ch, pal_bytes))))
        }
        _ => None,
    }
}

fn unpack_bits(data: &[u8], iw: u32, ih: u32, bpc: i64, comps: usize) -> Vec<u8> {
    if bpc == 8 {
        return data.to_vec();
    }
    let samples_per_row = iw as usize * comps;
    let row_stride = (samples_per_row * bpc as usize).div_ceil(8);
    let mut out = Vec::with_capacity(samples_per_row * ih as usize);
    for r in 0..ih as usize {
        let row = &data[(r * row_stride).min(data.len())..((r + 1) * row_stride).min(data.len())];
        for s in 0..samples_per_row {
            let val = match bpc {
                1 => {
                    let byte = row.get(s / 8).copied().unwrap_or(0);
                    let bit = (byte >> (7 - (s % 8))) & 1;
                    bit * 255
                }
                2 => {
                    let byte = row.get(s / 4).copied().unwrap_or(0);
                    let v = (byte >> (6 - 2 * (s % 4))) & 3;
                    v * 85
                }
                4 => {
                    let byte = row.get(s / 2).copied().unwrap_or(0);
                    let v = if s % 2 == 0 { byte >> 4 } else { byte & 0x0f };
                    v * 17
                }
                16 => row.get(s * 2).copied().unwrap_or(0),
                _ => 0,
            };
            out.push(val);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn extract_image(
    doc: &PdfDoc<'_>,
    dict: &BTreeMap<String, Obj>,
    raw: &[u8],
    page: usize,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fill_rgb: [u8; 3],
) -> Option<PlacedImage> {
    let smask_alpha: Option<(u32, u32, Vec<u8>)> = match doc.dict_get(dict, "SMask") {
        Obj::Stream { dict: sd, data: sr } => {
            let sw = doc.dict_get(&sd, "Width").as_int().unwrap_or(0) as u32;
            let sh = doc.dict_get(&sd, "Height").as_int().unwrap_or(0) as u32;
            let sbpc = doc.dict_get(&sd, "BitsPerComponent").as_int().unwrap_or(8);
            decode_stream(&sd, &sr, doc)
                .ok()
                .filter(|_| sw > 0 && sh > 0)
                .map(|dec| (sw, sh, unpack_bits(&dec, sw, sh, sbpc, 1)))
        }
        _ => None,
    };

    if stream_has_dct(doc, dict) {
        let jpeg_bytes = decode_stream(dict, raw, doc).ok()?;
        if let Some((sw, sh, alpha)) = smask_alpha {
            let mut rgba_img = image::load_from_memory(&jpeg_bytes).ok()?.to_rgba8();
            let (iw, ih) = (rgba_img.width(), rgba_img.height());
            for py in 0..ih {
                let sy = ((py as u64 * sh as u64) / ih.max(1) as u64).min(sh.saturating_sub(1) as u64) as u32;
                for px in 0..iw {
                    let sx = ((px as u64 * sw as u64) / iw.max(1) as u64).min(sw.saturating_sub(1) as u64) as u32;
                    let a = alpha.get((sy * sw + sx) as usize).copied().unwrap_or(255);
                    rgba_img.get_pixel_mut(px, py).0[3] = a;
                }
            }
            let mut png_bytes = Vec::new();
            image::DynamicImage::ImageRgba8(rgba_img)
                .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
                .ok()?;
            return Some(PlacedImage { page, x, y, w, h, png_bytes, seamless: true });
        }
        return Some(PlacedImage {
            page,
            x,
            y,
            w,
            h,
            png_bytes: jpeg_bytes,
            seamless: false,
        });
    }

    let iw = doc.dict_get(dict, "Width").as_int()? as u32;
    let ih = doc.dict_get(dict, "Height").as_int()? as u32;
    if iw == 0 || ih == 0 || iw > 8192 || ih > 8192 {
        return None;
    }

    let is_mask = matches!(dict.get("ImageMask"), Some(Obj::Bool));
    let bpc = doc.dict_get(dict, "BitsPerComponent").as_int().unwrap_or(if is_mask { 1 } else { 8 });
    if !matches!(bpc, 1 | 2 | 4 | 8 | 16) {
        return None;
    }

    let decoded = decode_stream(dict, raw, doc).ok()?;

    if is_mask && bpc == 1 && !dict.contains_key("ColorSpace") {
        let bits = unpack_bits(&decoded, iw, ih, 1, 1);
        let mut rgba = Vec::with_capacity((iw * ih * 4) as usize);
        for &v in bits.iter().take((iw * ih) as usize) {
            let alpha = 255u8.saturating_sub(v);
            rgba.extend_from_slice(&[fill_rgb[0], fill_rgb[1], fill_rgb[2], alpha]);
        }
        while rgba.len() < (iw * ih * 4) as usize {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        }
        let img_buf = image::RgbaImage::from_raw(iw, ih, rgba)?;
        let mut png_bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img_buf)
            .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .ok()?;
        return Some(PlacedImage { page, x, y, w, h, png_bytes, seamless: true });
    }

    let cs_obj = doc.dict_get(dict, "ColorSpace");
    let (channels, palette) = resolve_color_channels_and_palette(doc, &cs_obj)?;
    let unpacked = unpack_bits(&decoded, iw, ih, bpc, channels as usize);
    let pixel_count = (iw as usize) * (ih as usize);
    if unpacked.len() < pixel_count * (channels as usize) {
        return None;
    }

    let mut rgba = Vec::with_capacity(pixel_count * 4);
    let mut has_transparency = false;
    if let Some((base_ch, pal)) = palette {
        let bch = base_ch as usize;
        for i in 0..pixel_count {
            let idx = unpacked[i] as usize;
            let base = idx * bch;
            let rgb = match base_ch {
                1 => {
                    let g = pal.get(base).copied().unwrap_or(0);
                    [g, g, g]
                }
                4 => cmyk_to_rgb(
                    pal.get(base).copied().unwrap_or(0) as f32 / 255.0,
                    pal.get(base + 1).copied().unwrap_or(0) as f32 / 255.0,
                    pal.get(base + 2).copied().unwrap_or(0) as f32 / 255.0,
                    pal.get(base + 3).copied().unwrap_or(0) as f32 / 255.0,
                ),
                _ => [
                    pal.get(base).copied().unwrap_or(0),
                    pal.get(base + 1).copied().unwrap_or(0),
                    pal.get(base + 2).copied().unwrap_or(0),
                ],
            };
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    } else {
        match channels {
            1 => {
                for &g in &unpacked[..pixel_count] {
                    rgba.extend_from_slice(&[g, g, g, 255]);
                }
            }
            3 => {
                for px in unpacked[..pixel_count * 3].chunks_exact(3) {
                    rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
            4 => {
                for px in unpacked[..pixel_count * 4].chunks_exact(4) {
                    let rgb = cmyk_to_rgb(
                        px[0] as f32 / 255.0,
                        px[1] as f32 / 255.0,
                        px[2] as f32 / 255.0,
                        px[3] as f32 / 255.0,
                    );
                    rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
            }
            _ => return None,
        }
    }

    if let Some((sw, sh, alpha)) = smask_alpha {
        for py in 0..ih {
            let sy = ((py as u64 * sh as u64) / ih.max(1) as u64).min(sh.saturating_sub(1) as u64) as u32;
            for px in 0..iw {
                let sx = ((px as u64 * sw as u64) / iw.max(1) as u64).min(sw.saturating_sub(1) as u64) as u32;
                let a = alpha.get((sy * sw + sx) as usize).copied().unwrap_or(255);
                if a < 250 {
                    has_transparency = true;
                }
                rgba[((py * iw + px) * 4 + 3) as usize] = a;
            }
        }
    }

    let img_buf = image::RgbaImage::from_raw(iw, ih, rgba)?;
    let mut png_bytes = Vec::new();
    image::DynamicImage::ImageRgba8(img_buf)
        .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .ok()?;
    Some(PlacedImage {
        page,
        x,
        y,
        w,
        h,
        png_bytes,
        seamless: has_transparency,
    })
}

fn placed_image_to_doc_image(id: u64, img: &PlacedImage) -> Option<DocImage> {
    let width_in = (img.w / 72.0).clamp(0.08, 18.0);
    let height_in = (img.h / 72.0).clamp(0.06, 36.0);
    DocImage::from_exact_bytes(
        id,
        format!("pdf_img_{id}.png"),
        &img.png_bytes,
        width_in,
        height_in,
        img.seamless,
    )
    .ok()
}

// ── Layout Reconstruction -> Document ───────────────────────────────────

#[derive(Clone)]
struct LineGroup {
    y: f32,
    min_x: f32,
    max_x: f32,
    size: f32,
    spans: Vec<TextSpan>,
}

struct DetectedTable {
    top_y: f32,
    bot_y: f32,
    left_x: f32,
    right_x: f32,
    est_h_pt: f32,
    doc_table: DocTable,
    paras: Vec<Para>,
}

#[derive(Clone, Copy)]
struct StyledRule {
    y: f32,
    x0: f32,
    x1: f32,
    width_pt: f32,
    color: [u8; 3],
}

enum PageItemKind {
    Table(DetectedTable),
    Rule { width_pt: f32, color: [u8; 3] },
    Image(PlacedImage),
    Para(Para),
}

struct PageItem {
    top_y: f32,
    bot_y: f32,
    est_h_pt: f32,
    kind: PageItemKind,
}

/// Parses raw PDF bytes into an editable Scribe Pro `Document`.
pub fn read(data: &[u8]) -> Result<Document, String> {
    let pdf = PdfDoc::open(data)?;
    let root_obj = pdf
        .trailer
        .get("Root")
        .ok_or("Missing PDF /Root catalog.")?;
    let root = pdf.resolve(root_obj);
    let root_dict = root.as_dict().ok_or("Invalid PDF catalog.")?;
    let pages_obj = pdf.dict_get(root_dict, "Pages");

    let mut pages = Vec::new();
    collect_pages(&pdf, &pages_obj, [0.0, 0.0, 612.0, 792.0], BTreeMap::new(), &mut pages);
    if pages.is_empty() {
        return Err("This PDF has no pages.".into());
    }

    let mut doc = Document::default();
    doc.paras.clear();

    let mut is_scribe_export = false;
    if let Some(info_obj) = pdf.trailer.get("Info") {
        if let Some(info_dict) = pdf.resolve(info_obj).as_dict() {
            if let Some(b) = pdf.dict_get(info_dict, "Producer").as_str_bytes() {
                if decode_pdf_text_string(b).contains("Scribe Pro") {
                    is_scribe_export = true;
                }
            }
            if let Some(b) = pdf.dict_get(info_dict, "Title").as_str_bytes() {
                doc.info.title = decode_pdf_text_string(b);
            }
            if let Some(b) = pdf.dict_get(info_dict, "Author").as_str_bytes() {
                doc.info.author = decode_pdf_text_string(b);
            }
            if let Some(b) = pdf.dict_get(info_dict, "Subject").as_str_bytes() {
                doc.info.subject = decode_pdf_text_string(b);
            }
        }
    }

    let p0 = &pages[0].media_box;
    let pw = (p0[2] - p0[0]).abs().max(72.0);
    let ph = (p0[3] - p0[1]).abs().max(72.0);
    doc.page = PageSetup {
        width: (pw / 72.0 * 100.0).round() / 100.0,
        height: (ph / 72.0 * 100.0).round() / 100.0,
        ..PageSetup::default()
    };

    let mut all_spans = Vec::new();
    let mut all_images = Vec::new();
    let mut page_bg = None;
    let mut page_geoms = Vec::with_capacity(pages.len());
    for (idx, p) in pages.iter().enumerate() {
        page_geoms.push(interpret_page(
            &pdf,
            idx,
            p,
            is_scribe_export,
            &mut all_spans,
            &mut all_images,
            &mut page_bg,
        ));
    }
    if let Some(bg) = page_bg {
        doc.page.background = bg;
    }

    let mut imgs_by_page: Vec<Vec<PlacedImage>> = vec![Vec::new(); pages.len()];
    for img in all_images {
        if img.page < imgs_by_page.len() {
            imgs_by_page[img.page].push(img);
        }
    }

    // Reconstruct tables, standalone rules, banner shading, and vector artwork per page
    let mut next_table_id = 1u32;
    let mut tables_by_page: Vec<Vec<DetectedTable>> = Vec::with_capacity(pages.len());
    let mut rules_by_page: Vec<Vec<StyledRule>> = Vec::with_capacity(pages.len());
    let mut lines_by_page: Vec<Vec<LineGroup>> = Vec::with_capacity(pages.len());
    let mut page_is_full_scan = vec![false; pages.len()];
    let mut all_pages_full_scan = !pages.is_empty();

    for page_idx in 0..pages.len() {
        let p_spans: Vec<TextSpan> = all_spans.iter().filter(|s| s.page == page_idx).cloned().collect();
        let mut consumed_spans = vec![false; p_spans.len()];
        let pg_w = page_geoms[page_idx].pw;
        let pg_h = page_geoms[page_idx].ph;

        // Detect full-page scan / layered MRC cover pages (e.g. scanned magazine covers with cutout layers)
        let has_full_page_scan = imgs_by_page[page_idx]
            .iter()
            .any(|im| im.w >= pg_w * 0.85 && im.h >= pg_h * 0.85);
        page_is_full_scan[page_idx] = has_full_page_scan;
        all_pages_full_scan &= has_full_page_scan;

        if has_full_page_scan {
            // Render unconsumed vector logos/artwork into `imgs_by_page` and composite all overlapping layers
            // onto the full-page scan so layered scans never fragment across multiple pages.
            extract_unconsumed_vector_graphics(
                &mut page_geoms[page_idx],
                page_idx,
                &p_spans,
                &mut consumed_spans,
                &mut imgs_by_page[page_idx],
                doc.page.background,
            );
            let remaining_cover_spans: Vec<TextSpan> = p_spans
                .iter()
                .enumerate()
                .filter_map(|(i, s)| (!consumed_spans[i]).then(|| s.clone()))
                .collect();
            composite_overlapping_page_images(
                &mut imgs_by_page[page_idx],
                pg_w,
                pg_h,
                if remaining_cover_spans.len() <= 60 {
                    &remaining_cover_spans
                } else {
                    &[]
                },
            );
            tables_by_page.push(Vec::new());
            rules_by_page.push(Vec::new());
            // Keep large text layers editable if > 60 spans; otherwise cover text was composited on the scan
            if remaining_cover_spans.len() > 60 {
                lines_by_page.push(group_spans_into_lines(remaining_cover_spans));
            } else {
                lines_by_page.push(Vec::new());
            }
            continue;
        }

        let page_tables = detect_page_tables(
            &mut page_geoms[page_idx],
            &p_spans,
            &mut consumed_spans,
            &mut next_table_id,
            doc.page.background,
        );

        let page_rules = collect_standalone_rules(&mut page_geoms[page_idx], &page_tables, doc.page.background);

        // Pre-mark wide banner shading rectangles behind remaining text lines so they stay paragraph shading
        mark_banner_rects_consumed(&mut page_geoms[page_idx], &p_spans, &consumed_spans, doc.page.background);

        // Rasterize remaining unconsumed vector artwork (logos, icons, seals, bar/pie/line charts, diagrams)
        extract_unconsumed_vector_graphics(
            &mut page_geoms[page_idx],
            page_idx,
            &p_spans,
            &mut consumed_spans,
            &mut imgs_by_page[page_idx],
            doc.page.background,
        );

        composite_overlapping_page_images(&mut imgs_by_page[page_idx], pg_w, pg_h, &[]);

        let remaining_spans: Vec<TextSpan> = p_spans
            .into_iter()
            .enumerate()
            .filter_map(|(i, s)| (!consumed_spans[i]).then_some(s))
            .collect();
        let page_lines = group_spans_into_lines(remaining_spans);
        tables_by_page.push(page_tables);
        rules_by_page.push(page_rules);
        lines_by_page.push(page_lines);
    }

    // Extract repeating and bottom-pinned page footers before margin inference so footers sit at their exact
    // PDF Y coordinates near the bottom of each page rather than floating up into sparse body content.
    let max_footer_top_y = extract_pdf_page_footers(
        &mut doc,
        &page_geoms,
        &page_is_full_scan,
        &mut lines_by_page,
        &mut rules_by_page,
        &tables_by_page,
        &imgs_by_page,
    );

    // Infer page margins from lines, tables, and images across pages (supporting full-bleed & tight forms)
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_top_y = f32::NEG_INFINITY;
    let mut min_bot_y = f32::INFINITY;

    for page_lines in &lines_by_page {
        for l in page_lines {
            if !l.spans.iter().all(|s| s.text.trim().is_empty()) {
                min_x = min_x.min(l.min_x);
                max_x = max_x.max(l.max_x);
                max_top_y = max_top_y.max(l.y + l.size * 0.92);
                min_bot_y = min_bot_y.min(l.y - l.size * 0.22);
            }
        }
    }
    for page_tables in &tables_by_page {
        for t in page_tables {
            min_x = min_x.min(t.left_x);
            max_x = max_x.max(t.right_x);
            max_top_y = max_top_y.max(t.top_y);
            min_bot_y = min_bot_y.min(t.bot_y);
        }
    }
    for page_imgs in &imgs_by_page {
        for img in page_imgs {
            min_x = min_x.min(img.x);
            max_x = max_x.max(img.x + img.w);
            max_top_y = max_top_y.max(img.y + img.h);
            min_bot_y = min_bot_y.min(img.y);
        }
    }

    if all_pages_full_scan {
        doc.page.margin_left = 0.01;
        doc.page.margin_right = 0.01;
        doc.page.margin_top = 0.01;
        doc.page.margin_bottom = 0.01;
    } else {
        if min_x.is_finite() && min_x < pw * 0.36 {
            let ml = (((min_x.max(0.0) - 3.0).max(4.0) / 72.0) * 20.0).floor() / 20.0;
            let mr = if max_x.is_finite() && (pw - max_x) < pw * 0.36 {
                ((((pw - max_x).max(0.0) - 3.0).max(4.0) / 72.0) * 20.0).floor() / 20.0
            } else {
                ml
            };
            let sym_m = ml.min(mr).clamp(0.06, 1.75);
            doc.page.margin_left = sym_m;
            doc.page.margin_right = sym_m;
        }
        if max_top_y.is_finite() && (ph - max_top_y) < 130.0 {
            let mt = ((((ph - max_top_y).max(0.0) - 2.0).max(4.0) / 72.0) * 20.0).floor() / 20.0;
            doc.page.margin_top = mt.clamp(0.06, 1.65);
        }
        if min_bot_y.is_finite() && min_bot_y < 76.0 {
            let mb = (((min_bot_y.max(0.0) - 4.0).max(4.0) / 72.0) * 20.0).floor() / 20.0;
            doc.page.margin_bottom = mb.clamp(0.06, 1.0);
        } else {
            doc.page.margin_bottom = doc.page.margin_top.clamp(0.45, 0.85);
        }
        if max_footer_top_y > 0.0 {
            let footer_zone_in = (((max_footer_top_y + 4.0) / 72.0) * 20.0).ceil() / 20.0;
            let max_safe_mb = if min_bot_y.is_finite() {
                ((((min_bot_y - 4.0).max(4.0)) / 72.0) * 20.0).floor() / 20.0
            } else {
                1.25
            };
            if footer_zone_in > doc.page.margin_bottom {
                doc.page.margin_bottom = footer_zone_in.min(max_safe_mb).clamp(0.06, 1.25);
            }
        }
    }

    let content_left = doc.page.margin_left * 72.0;
    let content_right = pw - doc.page.margin_right * 72.0;
    let content_w = (content_right - content_left).max(72.0);
    let page_content_top = ph - doc.page.margin_top * 72.0;
    let avail_h_pt = (ph - (doc.page.margin_top + doc.page.margin_bottom) * 72.0).max(120.0);

    let mut next_img_id = 1u64;
    let mut any_content_emitted = false;

    for page_idx in 0..pages.len() {
        let geom = &page_geoms[page_idx];
        let mut page_tables = std::mem::take(&mut tables_by_page[page_idx]);
        let page_rules = std::mem::take(&mut rules_by_page[page_idx]);
        let mut page_lines = std::mem::take(&mut lines_by_page[page_idx]);
        let mut page_imgs = std::mem::take(&mut imgs_by_page[page_idx]);

        // Combine side-by-side images or small left/right logo + header text into borderless tables
        build_side_by_side_media_tables(
            &mut doc,
            &mut page_imgs,
            &mut page_lines,
            &mut page_tables,
            &mut next_table_id,
            &mut next_img_id,
            content_left,
            content_right,
            content_w,
        );

        // Pair independent staggered two-column text zones (e.g. two-column articles, dual summary boxes, split form headers)
        pair_two_column_text_blocks(
            &mut page_lines,
            &mut page_tables,
            geom,
            content_left,
            content_right,
            content_w,
            doc.page.background,
            &mut next_table_id,
        );

        for tbl in &mut page_tables {
            let indent_pt = (tbl.left_x - content_left).max(0.0);
            tbl.doc_table.indent_in = if indent_pt > 5.0 {
                ((indent_pt / 72.0) * 20.0).round() / 20.0
            } else {
                0.0
            };
        }

        let mut items: Vec<PageItem> = Vec::new();
        for tbl in page_tables {
            items.push(PageItem {
                top_y: tbl.top_y,
                bot_y: tbl.bot_y,
                est_h_pt: tbl.est_h_pt,
                kind: PageItemKind::Table(tbl),
            });
        }
        for r in page_rules {
            if (r.x1 - r.x0) >= content_w * 0.55 {
                items.push(PageItem {
                    top_y: r.y + r.width_pt * 0.5,
                    bot_y: r.y - r.width_pt * 0.5,
                    est_h_pt: 14.0,
                    kind: PageItemKind::Rule {
                        width_pt: r.width_pt,
                        color: r.color,
                    },
                });
            }
        }
        for img in page_imgs {
            items.push(PageItem {
                top_y: img.y + img.h,
                bot_y: img.y,
                est_h_pt: img.h + if img.seamless { 2.0 } else { 8.0 },
                kind: PageItemKind::Image(img),
            });
        }

        // Convert remaining text lines into either multi-column borderless tables or merged prose paragraphs
        build_line_items(
            &page_lines,
            geom,
            content_left,
            content_right,
            content_w,
            doc.page.background,
            &mut next_table_id,
            &mut items,
        );

        items.sort_by(|a, b| b.top_y.total_cmp(&a.top_y));
        if items.is_empty() {
            let page_has_footer = doc
                .page_footers
                .get(page_idx)
                .is_some_and(|pf| !pf.segments.is_empty() || pf.rule.is_some());
            if page_has_footer || (pages.len() > 1 && page_idx > 0) {
                doc.paras.push(Para::new(
                    CharStyle::default(),
                    ParaFmt {
                        page_break_before: any_content_emitted,
                        ..ParaFmt::default()
                    },
                ));
                any_content_emitted = true;
            }
            continue;
        }

        // Compute inter-item vertical gaps and scale proportionally if needed so dense single pages never overflow
        let mut raw_gaps = vec![0.0f32; items.len()];
        raw_gaps[0] = (page_content_top - items[0].top_y).clamp(0.0, 96.0);
        let mut running_bot = items[0].bot_y;
        let mut max_raw_gap = raw_gaps[0];
        for idx in 1..items.len() {
            let g = (running_bot - items[idx].top_y).max(0.0);
            raw_gaps[idx] = g.min(340.0);
            max_raw_gap = max_raw_gap.max(raw_gaps[idx]);
            running_bot = running_bot.min(items[idx].bot_y);
        }

        let est_natural_h: f32 = items.iter().map(|it| it.est_h_pt).sum();
        let sum_raw_gaps: f32 = raw_gaps.iter().sum();
        let avail_frac = if max_raw_gap > 130.0 { 0.89 } else { 0.92 };
        let budget_for_gaps = (avail_h_pt * avail_frac - est_natural_h).max(0.0);
        let gap_scale = if sum_raw_gaps > budget_for_gaps && sum_raw_gaps > 0.5 {
            (budget_for_gaps / sum_raw_gaps).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let dense_row_scale = if est_natural_h > avail_h_pt * 0.93 {
            (avail_h_pt * 0.90 / est_natural_h.max(1.0)).clamp(0.65, 1.0)
        } else {
            1.0
        };

        let mut page_has_emitted = false;
        for (idx, item) in items.into_iter().enumerate() {
            let gap_pt = ((raw_gaps[idx] * gap_scale) * 2.0).round() / 2.0;
            let pb = any_content_emitted && !page_has_emitted;

            match item.kind {
                PageItemKind::Table(mut tbl) => {
                    if tbl.paras.is_empty() {
                        continue;
                    }
                    if dense_row_scale < 0.999 {
                        for row in &mut tbl.doc_table.rows {
                            row.min_height_pt *= dense_row_scale;
                            for c in &mut row.cells {
                                c.pad_top_in *= dense_row_scale;
                                c.pad_bottom_in *= dense_row_scale;
                            }
                        }
                    }
                    if gap_pt >= 3.0 {
                        if !pb {
                            if let Some(prev_p) = doc.paras.last_mut() {
                                if prev_p.fmt.table_id == 0 {
                                    prev_p.fmt.space_after += gap_pt;
                                } else if gap_pt >= 7.0 {
                                    doc.paras.push(Para::new(
                                        CharStyle { size: 2.0, ..CharStyle::default() },
                                        ParaFmt {
                                            space_after: (gap_pt - 3.0).max(0.0),
                                            ..ParaFmt::default()
                                        },
                                    ));
                                }
                            }
                        } else if gap_pt >= 10.0 {
                            doc.paras.push(Para::new(
                                CharStyle { size: 2.0, ..CharStyle::default() },
                                ParaFmt {
                                    page_break_before: true,
                                    space_after: (gap_pt - 3.0).max(0.0),
                                    ..ParaFmt::default()
                                },
                            ));
                            page_has_emitted = true;
                            any_content_emitted = true;
                        }
                    }
                    if any_content_emitted && !page_has_emitted {
                        tbl.paras[0].fmt.page_break_before = true;
                    }
                    page_has_emitted = true;
                    any_content_emitted = true;
                    doc.tables.push(tbl.doc_table);
                    doc.paras.extend(tbl.paras);
                }
                PageItemKind::Rule { width_pt, color } => {
                    doc.paras.push(Para::new(
                        CharStyle::default(),
                        ParaFmt {
                            hr: true,
                            hr_color: Some(color),
                            hr_width_pt: width_pt,
                            space_before: if page_has_emitted { gap_pt } else { 0.0 },
                            page_break_before: pb,
                            ..ParaFmt::default()
                        },
                    ));
                    page_has_emitted = true;
                    any_content_emitted = true;
                }
                PageItemKind::Image(img) => {
                    let id = next_img_id;
                    if let Some(mut dimg) = placed_image_to_doc_image(id, &img) {
                        next_img_id += 1;
                        if !all_pages_full_scan && dense_row_scale < 0.999 {
                            dimg.width_in *= dense_row_scale;
                            dimg.height_in *= dense_row_scale;
                        }
                        let h_cap_frac = if all_pages_full_scan { 0.985 } else { 0.94 };
                        let max_img_h_in = (avail_h_pt * h_cap_frac / 72.0).max(1.0);
                        if dimg.height_in > max_img_h_in {
                            let s = max_img_h_in / dimg.height_in;
                            dimg.height_in *= s;
                            dimg.width_in *= s;
                        }
                        let max_img_w_in = (content_w / 72.0).max(1.0);
                        if dimg.width_in > max_img_w_in {
                            let s = max_img_w_in / dimg.width_in;
                            dimg.width_in *= s;
                            dimg.height_in *= s;
                        }
                        let (align, indent) = infer_box_align_and_indent(
                            img.x,
                            img.x + img.w,
                            content_left,
                            content_right,
                            content_w,
                        );
                        doc.images.push(dimg);
                        doc.paras.push(Para::new(
                            CharStyle::default(),
                            ParaFmt {
                                image_id: id,
                                align,
                                indent,
                                space_before: if page_has_emitted { gap_pt } else { 0.0 },
                                page_break_before: pb,
                                ..ParaFmt::default()
                            },
                        ));
                        page_has_emitted = true;
                        any_content_emitted = true;
                    }
                }
                PageItemKind::Para(mut p) => {
                    if dense_row_scale < 0.92 {
                        p.fmt.line_spacing = (p.fmt.line_spacing.max(0.95) * dense_row_scale.max(0.84)).clamp(0.82, 1.5);
                    }
                    p.fmt.page_break_before = pb;
                    if page_has_emitted {
                        p.fmt.space_before = gap_pt;
                    } else if gap_pt >= 12.0 {
                        // Preserve intentional top-of-page offset on subsequent pages via a tiny spacer
                        doc.paras.push(Para::new(
                            CharStyle { size: 2.0, ..CharStyle::default() },
                            ParaFmt {
                                page_break_before: pb,
                                space_after: (gap_pt - 3.0).max(0.0),
                                ..ParaFmt::default()
                            },
                        ));
                        p.fmt.page_break_before = false;
                    }
                    page_has_emitted = true;
                    any_content_emitted = true;
                    doc.paras.push(p);
                }
            }
        }
    }

    if doc.paras.is_empty() {
        doc.paras.push(Para::new(CharStyle::default(), ParaFmt::default()));
    }

    Ok(doc)
}

#[expect(clippy::too_many_arguments, reason = "Per-page lines, rules, tables, and images are inspected together to separate true footers from body content")]
fn extract_pdf_page_footers(
    doc: &mut Document,
    page_geoms: &[PageGeometry],
    page_is_full_scan: &[bool],
    lines_by_page: &mut [Vec<LineGroup>],
    rules_by_page: &mut [Vec<StyledRule>],
    tables_by_page: &[Vec<DetectedTable>],
    imgs_by_page: &[Vec<PlacedImage>],
) -> f32 {
    let num_pages = page_geoms.len();
    if num_pages == 0 {
        return 0.0;
    }

    // Pass 1: Discover repeating bottom-zone line signatures across multiple non-scan pages
    let non_scan_count = page_is_full_scan.iter().filter(|&&s| !s).count();
    let mut sig_pages: HashMap<(i32, String), Vec<usize>> = HashMap::new();
    if non_scan_count >= 2 {
        for p in 0..num_pages {
            if page_is_full_scan[p] {
                continue;
            }
            let pg_h = page_geoms[p].ph;
            let min_obs_y = min_page_obstacle_bot_y(&tables_by_page[p], &imgs_by_page[p]);
            for lg in lines_by_page[p].iter().rev().take(3) {
                if lg.y > (pg_h * 0.16).max(116.0) || lg.y + lg.size * 0.55 > min_obs_y + 2.0 {
                    break;
                }
                let norm = normalize_footer_signature(lg);
                if norm.chars().count() < 2 || norm.len() > 180 {
                    continue;
                }
                let yb = (lg.y / 10.0).round() as i32;
                let entry = sig_pages.entry((yb, norm)).or_default();
                if !entry.contains(&p) {
                    entry.push(p);
                }
            }
        }
    }

    let is_repeating_sig = |y: f32, norm: &str| -> bool {
        if non_scan_count < 2 || norm.chars().count() < 2 {
            return false;
        }
        let yb = (y / 10.0).round() as i32;
        for dy in -1..=1 {
            if let Some(ps) = sig_pages.get(&(yb + dy, norm.to_string())) {
                if ps.len() >= 2 {
                    return true;
                }
            }
        }
        false
    };

    // Pass 2: Extract footer lines and optional footer divider rule per page
    let mut page_footers = Vec::with_capacity(num_pages);
    let mut max_footer_top_y = 0.0f32;
    let mut any_footer_found = false;

    for p in 0..num_pages {
        if page_is_full_scan[p] {
            page_footers.push(PageFooter::default());
            continue;
        }
        let pg_w = page_geoms[p].pw;
        let pg_h = page_geoms[p].ph;
        let min_obs_y = min_page_obstacle_bot_y(&tables_by_page[p], &imgs_by_page[p]);
        let mut extracted_lines: Vec<LineGroup> = Vec::new();

        while extracted_lines.len() < 3 && !lines_by_page[p].is_empty() {
            let last_idx = lines_by_page[p].len() - 1;
            let lg = &lines_by_page[p][last_idx];
            let raw_txt = line_group_plain_text(lg);
            if raw_txt.trim().is_empty() {
                break;
            }
            // Must sit in the bottom margin zone and below all tables and images on the page
            let max_y_zone = if extracted_lines.is_empty() {
                (pg_h * 0.16).max(122.0)
            } else {
                48.0
            };
            if lg.y > max_y_zone || lg.y + lg.size * 0.55 > min_obs_y + 2.0 {
                break;
            }
            // If we already popped a lower footer line, any upper stacked footer line must sit close above it
            if let Some(prev_extracted) = extracted_lines.last() {
                let stack_gap = lg.y - (prev_extracted.y + prev_extracted.size * 0.85);
                if stack_gap > 22.0 {
                    break;
                }
            }

            let prev_body_bot = if last_idx > 0 {
                let prev = &lines_by_page[p][last_idx - 1];
                prev.y - prev.size * 0.22
            } else {
                f32::INFINITY
            };
            let nearest_above_y = prev_body_bot.min(min_obs_y);
            let gap_above = nearest_above_y - (lg.y + lg.size * 0.80);

            let has_divider_rule_above = rules_by_page[p].iter().any(|r| {
                (r.x1 - r.x0) >= pg_w * 0.45
                    && r.y >= lg.y + lg.size * 0.60
                    && r.y <= lg.y + lg.size + 18.0
                    && r.y <= nearest_above_y + 2.5
            });

            // Guard 1: Do not detach the final wrapped line of a multi-line body prose paragraph
            let is_prose_continuation = if last_idx > 0 && !has_divider_rule_above {
                let prev = &lines_by_page[p][last_idx - 1];
                gap_above < lg.size * 0.85
                    && (prev.size - lg.size).abs() <= 0.8
                    && (prev.min_x - lg.min_x).abs() <= 6.0
                    && (prev.max_x - prev.min_x) >= pg_w * 0.52
            } else {
                false
            };

            let segs = split_footer_line_into_segments(lg, pg_w);
            // Guard 2: Do not detach 3+ column body/form rows unless ultra-tiny bottom-edge legal print
            let is_multicol_body_row = segs.len() >= 3 && lg.size >= 6.5 && gap_above < 18.0 && !has_divider_rule_above;

            if is_prose_continuation || is_multicol_body_row {
                break;
            }

            let norm = normalize_footer_signature(lg);
            let crit_repeating = is_repeating_sig(lg.y, &norm);
            let crit_explicit = looks_like_explicit_footer_text(&raw_txt)
                && (gap_above >= 14.0 || lg.y <= 48.0 || has_divider_rule_above || last_idx == 0);
            let crit_bottom_strip = lg.y <= 26.5
                && (last_idx == 0 || gap_above >= 6.0 || lg.size <= 8.5 || has_divider_rule_above);
            let crit_separated_bar = lg.y <= 58.0
                && (gap_above >= 20.0 || has_divider_rule_above)
                && (lg.size <= 9.2
                    || lg.spans.iter().filter(|s| !s.text.trim().is_empty()).all(|s| s.italic)
                    || is_muted_footer_color(lg, doc.page.background)
                    || looks_like_contact_or_copyright_footer(&raw_txt)
                    || (segs.len() == 2 && segs[1].x_pt - segs[0].max_x_pt >= 72.0));

            if crit_repeating || crit_explicit || crit_bottom_strip || crit_separated_bar {
                if let Some(popped) = lines_by_page[p].pop() {
                    extracted_lines.push(popped);
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if extracted_lines.is_empty() {
            page_footers.push(PageFooter::default());
            continue;
        }

        any_footer_found = true;
        extracted_lines.reverse(); // Top-to-bottom within footer zone
        let mut segments = Vec::new();
        let mut top_y = 0.0f32;
        for lg in &extracted_lines {
            top_y = top_y.max(lg.y + lg.size * 0.90);
            segments.extend(split_footer_line_into_segments(lg, pg_w));
        }

        // Check for a standalone horizontal divider rule right above the footer and below remaining body content
        let rem_body_bot = lines_by_page[p]
            .last()
            .map_or(f32::INFINITY, |l| l.y - l.size * 0.22)
            .min(min_obs_y);
        let mut footer_rule = None;
        if let Some(r_idx) = rules_by_page[p].iter().position(|r| {
            (r.x1 - r.x0) >= pg_w * 0.45
                && r.y >= top_y + 0.5
                && r.y <= top_y + 18.0
                && r.y <= rem_body_bot + 1.5
        }) {
            let r = rules_by_page[p].remove(r_idx);
            top_y = top_y.max(r.y + r.width_pt * 0.5);
            footer_rule = Some((r.x0, r.x1, r.y, r.width_pt, r.color));
        }

        max_footer_top_y = max_footer_top_y.max(top_y);
        page_footers.push(PageFooter {
            segments,
            rule: footer_rule,
        });
    }

    if !any_footer_found {
        return 0.0;
    }

    // Populate `doc.footer`, `doc.footer_align`, and `doc.footer_italic` from the primary footer so
    // the Page Setup / Headers & Footers dialog displays it and DOCX/JSON metadata stays populated.
    if let Some(rep_pf) = page_footers.iter().find(|pf| !pf.segments.is_empty()) {
        let mut seg_texts = Vec::new();
        let mut all_italic = true;
        let mut any_non_ws = false;
        for seg in &rep_pf.segments {
            let s: String = seg.runs.iter().map(|r| r.text.as_str()).collect();
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                seg_texts.push(trimmed.to_string());
            }
            for r in &seg.runs {
                if !r.text.trim().is_empty() {
                    any_non_ws = true;
                    all_italic &= r.style.italic;
                }
            }
        }
        doc.footer = seg_texts.join("    ");
        doc.footer_align = if rep_pf.segments.len() == 1 {
            rep_pf.segments[0].align
        } else {
            Align::Center
        };
        doc.footer_italic = any_non_ws && all_italic;
    }

    doc.page_footers = page_footers;
    max_footer_top_y
}

fn min_page_obstacle_bot_y(tables: &[DetectedTable], imgs: &[PlacedImage]) -> f32 {
    let mut min_y = f32::INFINITY;
    for t in tables {
        min_y = min_y.min(t.bot_y);
    }
    for im in imgs {
        min_y = min_y.min(im.y);
    }
    min_y
}

fn line_group_plain_text(lg: &LineGroup) -> String {
    let mut out = String::new();
    let mut prev_end: Option<f32> = None;
    for sp in &lg.spans {
        if let Some(pe) = prev_end {
            if sp.x - pe > (sp.size * 0.15).max(0.95) && !out.ends_with(' ') && !sp.text.starts_with(' ') {
                out.push(' ');
            }
        }
        out.push_str(&sp.text);
        prev_end = Some(sp.x + sp.width);
    }
    out
}

fn normalize_footer_signature(lg: &LineGroup) -> String {
    let raw = line_group_plain_text(lg);
    let mut out = String::with_capacity(raw.len());
    let mut prev_digit = false;
    let mut prev_space = true;
    for ch in raw.trim().chars() {
        if ch.is_ascii_digit() {
            if !prev_digit {
                out.push('#');
                prev_digit = true;
            }
            prev_space = false;
        } else if ch.is_whitespace() {
            prev_digit = false;
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            prev_digit = false;
            prev_space = false;
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
        }
    }
    out.trim().to_string()
}

fn split_footer_line_into_segments(lg: &LineGroup, pg_w: f32) -> Vec<FooterSegment> {
    let mut clusters: Vec<Vec<&TextSpan>> = Vec::new();
    let mut prev_end: Option<f32> = None;
    for sp in &lg.spans {
        if sp.text.is_empty() {
            continue;
        }
        let split_tol = (sp.size * 3.0).max(24.0);
        if let Some(pe) = prev_end {
            if sp.x - pe > split_tol {
                clusters.push(vec![sp]);
            } else if let Some(last) = clusters.last_mut() {
                last.push(sp);
            }
        } else {
            clusters.push(vec![sp]);
        }
        prev_end = Some(sp.x + sp.width);
    }

    let page_mid = pg_w * 0.5;
    let mut segments = Vec::new();
    for cl in clusters {
        if cl.iter().all(|s| s.text.trim().is_empty()) {
            continue;
        }
        let x0 = cl.first().map_or(lg.min_x, |s| s.x);
        let x1 = cl.last().map_or(lg.max_x, |s| s.x + s.width);
        let baseline_y = cl.iter().map(|s| s.y).sum::<f32>() / cl.len() as f32;
        let seg_mid = (x0 + x1) * 0.5;
        let align = if (seg_mid - page_mid).abs() <= 26.0 && x0 > 36.0 && (pg_w - x1) > 36.0 {
            Align::Center
        } else if seg_mid > pg_w * 0.60 && (pg_w - x1) < x0 * 0.55 {
            Align::Right
        } else {
            Align::Left
        };

        let mut runs: Vec<FooterRun> = Vec::new();
        let mut pe_run: Option<f32> = None;
        for sp in cl {
            let st = span_to_char_style(sp);
            let mut txt = String::new();
            if let Some(pe) = pe_run {
                let gap = sp.x - pe;
                if gap > (sp.size * 0.15).max(0.95)
                    && !runs.last().is_some_and(|r| r.text.ends_with(' '))
                    && !sp.text.starts_with(' ')
                {
                    let n_sp = if gap > (sp.size * 1.4).max(10.0) {
                        ((gap / (sp.size * 0.34)).round() as usize).clamp(2, 8)
                    } else {
                        1
                    };
                    for _ in 0..n_sp {
                        txt.push(' ');
                    }
                }
            }
            for ch in sp.text.chars() {
                if ch != '\r' && ch != '\n' {
                    txt.push(ch);
                }
            }
            if !txt.is_empty() {
                if let Some(last_r) = runs.last_mut() {
                    if last_r.style == st {
                        last_r.text.push_str(&txt);
                    } else {
                        runs.push(FooterRun { text: txt, style: st });
                    }
                } else {
                    runs.push(FooterRun { text: txt, style: st });
                }
            }
            pe_run = Some(sp.x + sp.width);
        }

        if !runs.is_empty() {
            segments.push(FooterSegment {
                runs,
                x_pt: x0,
                max_x_pt: x1,
                bottom_pt: baseline_y.max(2.0),
                align,
            });
        }
    }
    segments
}

fn looks_like_explicit_footer_text(txt: &str) -> bool {
    let t = txt.trim();
    if t.is_empty() || t.len() > 96 {
        return false;
    }
    let tl = t.to_ascii_lowercase();
    for kw in ["page ", "pg. ", "pg "] {
        if let Some(idx) = tl.find(kw) {
            if idx == 0 || tl.as_bytes().get(idx - 1).is_some_and(|b| b.is_ascii_whitespace() || *b == b'|' || *b == b'-') {
                let rest = tl[idx + kw.len()..].trim_start();
                if rest.bytes().next().is_some_and(|b| b.is_ascii_digit()) {
                    return true;
                }
            }
        }
    }
    let stripped = t.trim_matches(|c: char| c == '-' || c == '–' || c == '—' || c == '[' || c == ']' || c.is_whitespace());
    if !stripped.is_empty() && stripped.len() <= 4 && stripped.bytes().all(|b| b.is_ascii_digit()) && t.contains(['-', '–', '—', '[']) {
        return true;
    }
    if (tl.starts_with("printed ")
        || tl.starts_with("printed:")
        || tl.starts_with("generated on ")
        || tl.starts_with("generated by "))
        && t.bytes().any(|b| b.is_ascii_digit())
    {
        return true;
    }
    false
}

fn looks_like_contact_or_copyright_footer(txt: &str) -> bool {
    let t = txt.trim();
    if t.is_empty() || t.len() > 150 {
        return false;
    }
    let tl = t.to_ascii_lowercase();
    if t.contains('©') || tl.starts_with("copyright ") || tl.contains("all rights reserved") {
        return true;
    }
    let has_sep = t.contains(" | ") || t.contains(" • ") || t.contains(" · ");
    let has_web_or_phone = tl.contains(".com")
        || tl.contains(".org")
        || tl.contains(".net")
        || tl.contains(".gov")
        || tl.contains(".io")
        || tl.contains("www.")
        || tl.contains("http")
        || t.bytes().filter(|b| b.is_ascii_digit()).count() >= 5;
    has_sep && has_web_or_phone
}

fn is_muted_footer_color(lg: &LineGroup, bg: [u8; 3]) -> bool {
    let bg_lum = (bg[0] as i32 + bg[1] as i32 + bg[2] as i32) / 3;
    if bg_lum < 200 {
        return false;
    }
    let mut any = false;
    for sp in &lg.spans {
        if sp.text.trim().is_empty() {
            continue;
        }
        any = true;
        let lum = (sp.color[0] as i32 + sp.color[1] as i32 + sp.color[2] as i32) / 3;
        if lum < 75 {
            return false;
        }
    }
    any
}

fn infer_box_align_and_indent(
    x0: f32,
    x1: f32,
    content_left: f32,
    content_right: f32,
    content_w: f32,
) -> (Align, u8) {
    let w = (x1 - x0).max(0.0);
    if w >= content_w * 0.88 {
        return (Align::Center, 0);
    }
    let left_gap = (x0 - content_left).max(0.0);
    let right_gap = (content_right - x1).max(0.0);
    let mid_diff = ((x0 + x1) * 0.5 - (content_left + content_right) * 0.5).abs();
    if mid_diff <= 18.0 && left_gap > 16.0 && right_gap > 16.0 {
        (Align::Center, 0)
    } else if right_gap <= 20.0 && left_gap > 32.0 {
        (Align::Right, 0)
    } else {
        let indent = if left_gap > 20.0 {
            (left_gap / 36.0).round().clamp(0.0, 6.0) as u8
        } else {
            0
        };
        (Align::Left, indent)
    }
}

fn group_spans_into_lines(mut spans: Vec<TextSpan>) -> Vec<LineGroup> {
    spans.sort_by(|a, b| b.y.total_cmp(&a.y).then_with(|| a.x.total_cmp(&b.x)));
    let mut page_lines: Vec<LineGroup> = Vec::new();
    for sp in spans {
        let tol = (sp.size * 0.42).max(2.8);
        if let Some(lg) = page_lines.iter_mut().find(|l| (l.y - sp.y).abs() <= tol) {
            lg.min_x = lg.min_x.min(sp.x);
            lg.max_x = lg.max_x.max(sp.x + sp.width);
            lg.size = lg.size.max(sp.size);
            lg.spans.push(sp);
        } else {
            page_lines.push(LineGroup {
                y: sp.y,
                min_x: sp.x,
                max_x: sp.x + sp.width,
                size: sp.size,
                spans: vec![sp],
            });
        }
    }
    for lg in &mut page_lines {
        lg.spans.sort_by(|a, b| a.x.total_cmp(&b.x));
        lg.min_x = lg.spans.first().map_or(lg.min_x, |s| s.x);
        lg.max_x = lg.spans.last().map_or(lg.max_x, |s| s.x + s.width);
    }
    page_lines
}

fn cluster_coords(mut vals: Vec<f32>, tol: f32) -> Vec<f32> {
    vals.sort_by(|a, b| a.total_cmp(b));
    let mut clusters: Vec<(f32, usize)> = Vec::new();
    for v in vals {
        if let Some(last) = clusters.last_mut() {
            if (v - last.0).abs() <= tol {
                let n = last.1 as f32;
                last.0 = (last.0 * n + v) / (n + 1.0);
                last.1 += 1;
                continue;
            }
        }
        clusters.push((v, 1));
    }
    clusters.into_iter().map(|(v, _)| v).collect()
}

fn edge_border_spec(
    segs: &[StyledSeg],
    coord: f32,
    a0: f32,
    a1: f32,
    tol: f32,
    consumed_paths: &mut [bool],
) -> Option<BorderSpec> {
    let edge_len = (a1 - a0).max(1.0);
    let mut intervals: Vec<(f32, f32)> = Vec::new();
    let mut w_sum = 0.0f32;
    let mut r_sum = 0.0f32;
    let mut g_sum = 0.0f32;
    let mut b_sum = 0.0f32;
    let mut weight_tot = 0.0f32;
    let mut matched_paths = Vec::new();

    for seg in segs {
        if (seg.coord - coord).abs() <= tol {
            let s0 = seg.a0.max(a0);
            let s1 = seg.a1.min(a1);
            if s1 - s0 > 0.5 {
                let len = s1 - s0;
                intervals.push((s0, s1));
                w_sum += seg.width_pt * len;
                r_sum += seg.color[0] as f32 * len;
                g_sum += seg.color[1] as f32 * len;
                b_sum += seg.color[2] as f32 * len;
                weight_tot += len;
                matched_paths.push(seg.path_idx);
            }
        }
    }
    if intervals.is_empty() || weight_tot <= 0.0 {
        return None;
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut covered = 0.0f32;
    let mut cur = intervals[0];
    for &(s0, s1) in &intervals[1..] {
        if s0 <= cur.1 + 1.5 {
            cur.1 = cur.1.max(s1);
        } else {
            covered += cur.1 - cur.0;
            cur = (s0, s1);
        }
    }
    covered += cur.1 - cur.0;

    if covered >= edge_len * 0.38 || (edge_len < 24.0 && covered >= 8.0) {
        for pidx in matched_paths {
            if pidx < consumed_paths.len() {
                consumed_paths[pidx] = true;
            }
        }
        Some(BorderSpec {
            width_pt: (w_sum / weight_tot).clamp(0.35, 6.0),
            color: [
                (r_sum / weight_tot).round().clamp(0.0, 255.0) as u8,
                (g_sum / weight_tot).round().clamp(0.0, 255.0) as u8,
                (b_sum / weight_tot).round().clamp(0.0, 255.0) as u8,
            ],
        })
    } else {
        None
    }
}

fn detect_page_tables(
    geom: &mut PageGeometry,
    spans: &[TextSpan],
    consumed: &mut [bool],
    next_table_id: &mut u32,
    page_bg: [u8; 3],
) -> Vec<DetectedTable> {
    let mut out = Vec::new();

    // ── Part A: Grid tables with vertical border segments ───────────────────
    let mut valid_v: Vec<StyledSeg> = geom
        .v_segs
        .iter()
        .copied()
        .filter(|s| (s.a1 - s.a0) >= 6.0)
        .collect();
    valid_v.sort_by(|a, b| a.a0.total_cmp(&b.a0));

    let mut zones: Vec<(f32, f32)> = Vec::new();
    for s in &valid_v {
        if let Some(last) = zones.last_mut() {
            if s.a0 <= last.1 + 16.0 {
                last.0 = last.0.min(s.a0);
                last.1 = last.1.max(s.a1);
                continue;
            }
        }
        zones.push((s.a0, s.a1));
    }

    for (zy0, zy1) in zones {
        let zone_v: Vec<StyledSeg> = valid_v
            .iter()
            .copied()
            .filter(|s| s.a1 >= zy0 - 2.0 && s.a0 <= zy1 + 2.0)
            .collect();
        let raw_xs: Vec<f32> = zone_v.iter().map(|s| s.coord).collect();
        let cand_xs = cluster_coords(raw_xs, 3.5);
        let col_xs: Vec<f32> = cand_xs
            .into_iter()
            .filter(|&cx| {
                let total_len: f32 = zone_v
                    .iter()
                    .filter(|s| (s.coord - cx).abs() <= 4.0)
                    .map(|s| s.a1 - s.a0)
                    .sum();
                total_len >= 14.0
            })
            .collect();
        if col_xs.len() < 2 {
            continue;
        }
        let tbl_x0 = col_xs[0];
        let tbl_x1 = *col_xs.last().unwrap_or(&tbl_x0);
        let tbl_w = tbl_x1 - tbl_x0;
        if tbl_w < 90.0 {
            continue;
        }
        // Skip decorative full-page single-box outer borders that lack internal columns/dividers
        if col_xs.len() == 2 && tbl_w >= geom.pw * 0.80 && (zy1 - zy0) >= geom.ph * 0.65 {
            let internal_h = geom
                .h_segs
                .iter()
                .filter(|h| h.coord > zy0 + 12.0 && h.coord < zy1 - 12.0 && (h.a1.min(tbl_x1) - h.a0.max(tbl_x0)) >= tbl_w * 0.5)
                .count();
            if internal_h == 0 {
                continue;
            }
        }

        let matching_h: Vec<StyledSeg> = geom
            .h_segs
            .iter()
            .copied()
            .filter(|h| {
                h.coord >= zy0 - 28.0
                    && h.coord <= zy1 + 28.0
                    && (h.a1.min(tbl_x1) - h.a0.max(tbl_x0)) >= 20.0
            })
            .collect();
        if matching_h.len() < 2 {
            continue;
        }

        let mut raw_ys: Vec<f32> = vec![zy0, zy1];
        for h in &matching_h {
            raw_ys.push(h.coord);
        }
        for r in &geom.fill_rects {
            if r.w >= geom.pw * 0.88 && r.h >= geom.ph * 0.88 {
                continue;
            }
            let ov_x = (tbl_x1.min(r.x + r.w) - tbl_x0.max(r.x)).max(0.0);
            if r.h >= 8.0 && ov_x >= tbl_w * 0.5 && r.y >= zy0 - 28.0 && r.y + r.h <= zy1 + 28.0 {
                raw_ys.push(r.y);
                raw_ys.push(r.y + r.h);
            }
        }

        let mut row_ys = cluster_coords(raw_ys, 3.5);
        row_ys.sort_by(|a, b| b.total_cmp(a));
        if row_ys.len() < 2 {
            continue;
        }

        let mut row_bands: Vec<(f32, f32)> = Vec::new();
        let mut cur_top = row_ys[0];
        for &ry in &row_ys[1..] {
            if cur_top - ry >= 6.0 {
                row_bands.push((cur_top, ry));
                cur_top = ry;
            }
        }
        if row_bands.is_empty() || (row_bands.len() < 2 && col_xs.len() < 3) {
            continue;
        }

        for v in &zone_v {
            if v.path_idx < geom.consumed_paths.len() {
                geom.consumed_paths[v.path_idx] = true;
            }
        }
        for h in &matching_h {
            if h.path_idx < geom.consumed_paths.len() {
                geom.consumed_paths[h.path_idx] = true;
            }
        }

        let raw_table_w_in = ((col_xs[col_xs.len() - 1] - col_xs[0]) / 72.0).max(0.4);
        let mut grid_cols_in: Vec<f32> = col_xs
            .windows(2)
            .map(|w| ((w[1] - w[0]) / 72.0).max(0.08))
            .collect();
        let sum_cols_in: f32 = grid_cols_in.iter().sum();
        if sum_cols_in > raw_table_w_in + 0.03 && sum_cols_in > 0.1 {
            let scale_w = raw_table_w_in / sum_cols_in;
            for cw in &mut grid_cols_in {
                *cw *= scale_w;
            }
        }
        let num_grid_cols = grid_cols_in.len();
        let tid = *next_table_id;
        *next_table_id += 1;

        let mut rows_spec: Vec<TableRowSpec> = Vec::with_capacity(row_bands.len());
        let mut tbl_paras: Vec<Para> = Vec::new();
        let mut est_tbl_h = 0.0f32;

        for (r_idx, &(y_top, y_bot)) in row_bands.iter().enumerate() {
            let row_h = (y_top - y_bot).max(6.0);
            let mut cells_spec: Vec<TableCellSpec> = Vec::new();
            let mut g0 = 0usize;
            let mut c_idx = 0u16;
            let mut max_cell_est_h = 0.0f32;

            while g0 < num_grid_cols {
                let mut g1 = g0 + 1;
                while g1 < num_grid_cols {
                    let bx = col_xs[g1];
                    let has_v_divider = zone_v.iter().any(|v| {
                        (v.coord - bx).abs() <= 4.5
                            && (v.a1.min(y_top - 1.5) - v.a0.max(y_bot + 1.5)) >= (row_h * 0.35).min(6.0)
                    });
                    if has_v_divider {
                        break;
                    }
                    let straddles_bx = spans.iter().enumerate().any(|(si, sp)| {
                        !consumed[si]
                            && !sp.text.trim().is_empty()
                            && sp.y + sp.size * 0.3 >= y_bot - 2.0
                            && sp.y + sp.size * 0.3 <= y_top + 2.0
                            && sp.x < bx - 1.5
                            && sp.x + sp.width > bx + 1.5
                    });
                    if !straddles_bx && (bx - col_xs[g0]) >= 28.0 && (col_xs[g1 + 1] - bx) >= 28.0 {
                        let mut max_l_end = f32::NEG_INFINITY;
                        let mut min_r_start = f32::INFINITY;
                        for (si, sp) in spans.iter().enumerate() {
                            if consumed[si] || sp.text.trim().is_empty() {
                                continue;
                            }
                            let my = sp.y + sp.size * 0.3;
                            if my >= y_bot - 2.0 && my <= y_top + 2.0 {
                                if sp.x >= col_xs[g0] - 4.0 && sp.x + sp.width <= bx + 2.0 {
                                    max_l_end = max_l_end.max(sp.x + sp.width);
                                } else if sp.x >= bx - 2.0 && sp.x < col_xs[g1 + 1] - 2.0 {
                                    min_r_start = min_r_start.min(sp.x);
                                }
                            }
                        }
                        if max_l_end.is_finite() && min_r_start.is_finite() && (min_r_start - max_l_end) >= 20.0 {
                            break;
                        }
                    }
                    g1 += 1;
                }

                let cx0 = col_xs[g0];
                let cx1 = col_xs[g1];
                let cell_w_pt = (cx1 - cx0).max(12.0);
                let width_in: f32 = grid_cols_in[g0..g1].iter().sum();

                let borders = CellBorders {
                    top: edge_border_spec(&geom.h_segs, y_top, cx0, cx1, 4.0, &mut geom.consumed_paths),
                    bottom: edge_border_spec(&geom.h_segs, y_bot, cx0, cx1, 4.0, &mut geom.consumed_paths),
                    left: edge_border_spec(&geom.v_segs, cx0, y_bot, y_top, 4.5, &mut geom.consumed_paths),
                    right: edge_border_spec(&geom.v_segs, cx1, y_bot, y_top, 4.5, &mut geom.consumed_paths),
                };

                let cell_area = cell_w_pt * row_h;
                let mut shading: Option<[u8; 3]> = None;
                let mut best_cov = 0.0f32;
                let mut best_path_idx: Option<usize> = None;
                for r in &geom.fill_rects {
                    if r.w >= geom.pw * 0.88 && r.h >= geom.ph * 0.88 {
                        continue;
                    }
                    let eff_c = r.blended_color(page_bg);
                    if eff_c == page_bg {
                        continue;
                    }
                    let ox = (cx1.min(r.x + r.w) - cx0.max(r.x)).max(0.0);
                    let oy = (y_top.min(r.y + r.h) - y_bot.max(r.y)).max(0.0);
                    let cov = (ox * oy) / cell_area.max(1.0);
                    if cov >= 0.42 && cov > best_cov {
                        best_cov = cov;
                        shading = Some(eff_c);
                        best_path_idx = Some(r.path_idx);
                    }
                }
                if let Some(pidx) = best_path_idx {
                    if pidx < geom.consumed_paths.len() {
                        geom.consumed_paths[pidx] = true;
                    }
                }

                let left_pad = if g0 == 0 { 8.0 } else { 2.5 };
                let right_pad = if g1 == num_grid_cols { 8.0 } else { 2.5 };
                let mut cell_spans = Vec::new();
                for (si, sp) in spans.iter().enumerate() {
                    if consumed[si] {
                        continue;
                    }
                    let mid_y = sp.y + sp.size * 0.3;
                    let anchor_x = sp.x + sp.width.min(6.0) * 0.5;
                    if mid_y >= y_bot - 2.5
                        && mid_y <= y_top + 2.5
                        && anchor_x >= cx0 - left_pad
                        && anchor_x <= cx1 + right_pad
                    {
                        consumed[si] = true;
                        let mut adjusted = sp.clone();
                        if shading.is_some() && adjusted.highlight == shading {
                            adjusted.highlight = None;
                        }
                        adjusted.color = ensure_legible_color(adjusted.color, shading, adjusted.highlight, page_bg);
                        cell_spans.push(adjusted);
                    }
                }

                let cell_lines = group_spans_into_lines(cell_spans);
                let mut v_align = CellVAlign::Top;
                if !cell_lines.is_empty() {
                    let top_txt = cell_lines[0].y + cell_lines[0].size * 0.84;
                    let last_cl = &cell_lines[cell_lines.len() - 1];
                    let bot_txt = last_cl.y - last_cl.size * 0.18;
                    let top_gap = (y_top - top_txt).max(0.0);
                    let bot_gap = (bot_txt - y_bot).max(0.0);
                    if top_gap >= 2.2 && bot_gap >= 2.2 && (top_gap - bot_gap).abs() <= row_h * 0.32 {
                        v_align = CellVAlign::Center;
                    }
                }

                let cell_est_h = populate_table_cell_paras(
                    &cell_lines,
                    cx0,
                    cx1,
                    cell_w_pt,
                    tid,
                    r_idx as u16,
                    c_idx,
                    &mut tbl_paras,
                );
                let h_pad = (width_in * 0.07).clamp(0.012, 0.055);
                let v_pad = ((row_h / 72.0) * 0.08).clamp(0.006, 0.02);
                max_cell_est_h = max_cell_est_h.max(cell_est_h + v_pad * 144.0 + 1.0);

                cells_spec.push(TableCellSpec {
                    width_in,
                    grid_span: (g1 - g0) as u16,
                    shading,
                    borders,
                    v_align,
                    pad_left_in: h_pad,
                    pad_right_in: h_pad,
                    pad_top_in: v_pad,
                    pad_bottom_in: v_pad,
                    ..TableCellSpec::default()
                });

                g0 = g1;
                c_idx += 1;
            }

            let min_h = (row_h * 0.90).clamp(4.0, 180.0);
            est_tbl_h += max_cell_est_h.max(min_h);
            rows_spec.push(TableRowSpec {
                cells: cells_spec,
                min_height_pt: min_h,
                grid_after: 0,
            });
        }

        let top_y = row_bands.first().map_or(zy1, |b| b.0);
        let bot_y = row_bands.last().map_or(zy0, |b| b.1);
        out.push(DetectedTable {
            top_y,
            bot_y,
            left_x: tbl_x0,
            right_x: tbl_x1,
            est_h_pt: est_tbl_h,
            doc_table: DocTable {
                id: tid,
                grid_cols_in,
                rows: rows_spec,
                indent_in: 0.0,
            },
            paras: tbl_paras,
        });
    }

    // ── Part B: Horizontal-rule & zebra-shaded borderless-column tables ─────
    detect_hrule_and_zebra_tables(geom, spans, consumed, next_table_id, page_bg, &mut out);

    out
}

fn populate_table_cell_paras(
    cell_lines: &[LineGroup],
    cx0: f32,
    cx1: f32,
    cell_w_pt: f32,
    tid: u32,
    row_idx: u16,
    col_idx: u16,
    tbl_paras: &mut Vec<Para>,
) -> f32 {
    if cell_lines.is_empty() {
        tbl_paras.push(Para::new(
            CharStyle { size: 8.0, ..CharStyle::default() },
            ParaFmt {
                table_id: tid,
                table_row: row_idx,
                table_col: col_idx,
                ..ParaFmt::default()
            },
        ));
        return 8.0;
    }

    let mut cur_cell_para: Option<Para> = None;
    let mut prev_cl: Option<&LineGroup> = None;
    let mut est_h = 0.0f32;
    let mut emitted_any = false;

    for cl in cell_lines {
        let line_txt = join_line_text(&cl.spans);
        if line_txt.trim().is_empty() {
            continue;
        }
        est_h += cl.size * 1.24;
        let align = if cell_w_pt >= 46.0 {
            infer_cell_alignment(cl, cx0 + 2.5, cx1 - 2.5, (cell_w_pt - 5.0).max(20.0))
        } else {
            Align::Left
        };
        let (raw_list, checked, raw_strip) = detect_list_prefix(&line_txt);
        let (list_kind, strip_chars) = if raw_list == ListKind::Number {
            (ListKind::None, 0)
        } else {
            (raw_list, raw_strip)
        };
        let merge = prev_cl.is_some_and(|p| {
            cur_cell_para.as_ref().is_some_and(|cp| cp.fmt.align == align)
                && should_merge_lines(p, cl, cx0, cell_w_pt, raw_list)
        });
        if !merge {
            if let Some(mut p) = cur_cell_para.take() {
                if let Some(prev) = prev_cl {
                    let extra_gap = (prev.y - cl.y) - prev.size.max(cl.size) * 1.28;
                    if extra_gap >= 3.0 {
                        let sa = (extra_gap.min(24.0) * 2.0).round() / 2.0;
                        p.fmt.space_after = sa;
                        est_h += sa;
                    }
                }
                tbl_paras.push(p);
                emitted_any = true;
            }
            let first_st = span_to_char_style(&cl.spans[0]);
            cur_cell_para = Some(Para::new(
                first_st,
                ParaFmt {
                    align,
                    list: list_kind,
                    checked,
                    table_id: tid,
                    table_row: row_idx,
                    table_col: col_idx,
                    ..ParaFmt::default()
                },
            ));
        } else if let Some(ref mut p) = cur_cell_para {
            if let Some(prev) = prev_cl {
                let ls = ((prev.y - cl.y) / (cl.size * 1.20)).clamp(0.95, 1.65);
                p.fmt.line_spacing = ls;
            }
            if !p.text.is_empty() && !p.text.ends_with(&[' ', '-']) {
                let st = p.end_style;
                p.text.push(' ');
                p.styles.push(st);
            }
        }
        if let Some(ref mut p) = cur_cell_para {
            append_line_spans(p, &cl.spans, strip_chars, false, cell_w_pt);
        }
        prev_cl = Some(cl);
    }

    if let Some(p) = cur_cell_para.take() {
        tbl_paras.push(p);
        emitted_any = true;
    }
    if !emitted_any {
        tbl_paras.push(Para::new(
            CharStyle { size: 8.0, ..CharStyle::default() },
            ParaFmt {
                table_id: tid,
                table_row: row_idx,
                table_col: col_idx,
                ..ParaFmt::default()
            },
        ));
    }
    est_h.max(10.0)
}

fn infer_cell_alignment(lg: &LineGroup, left: f32, right: f32, avail_w: f32) -> Align {
    let line_w = (lg.max_x - lg.min_x).max(0.0);
    if line_w >= avail_w * 0.85 {
        return Align::Left;
    }
    let left_gap = (lg.min_x - left).max(0.0);
    let right_gap = (right - lg.max_x).max(0.0);
    let mid_diff = ((lg.min_x + lg.max_x) * 0.5 - (left + right) * 0.5).abs();
    if right_gap <= 14.0 && left_gap >= 14.0 {
        Align::Right
    } else if mid_diff <= 10.0 && left_gap >= 10.0 && right_gap >= 10.0 {
        Align::Center
    } else {
        Align::Left
    }
}

fn split_line_into_chunks(lg: &LineGroup, min_gap_pt: f32) -> Vec<(f32, f32, Vec<TextSpan>)> {
    let mut chunks: Vec<(f32, f32, Vec<TextSpan>)> = Vec::new();
    for sp in &lg.spans {
        if sp.text.trim().is_empty() {
            continue;
        }
        let thresh = (lg.size * 1.65).max(min_gap_pt);
        if let Some(last) = chunks.last_mut() {
            if sp.x - last.1 <= thresh {
                last.1 = last.1.max(sp.x + sp.width);
                last.2.push(sp.clone());
                continue;
            }
        }
        chunks.push((sp.x, sp.x + sp.width, vec![sp.clone()]));
    }
    chunks
}

fn detect_hrule_and_zebra_tables(
    geom: &mut PageGeometry,
    spans: &[TextSpan],
    consumed: &mut [bool],
    next_table_id: &mut u32,
    page_bg: [u8; 3],
    tables_out: &mut Vec<DetectedTable>,
) {
    let min_band_w = geom.pw * 0.36;
    // Gather horizontal rules and row-like shading bands not already inside Part A tables
    let mut intervals: Vec<(f32, f32, f32, f32)> = Vec::new(); // (y0, y1, x0, x1)
    for h in &geom.h_segs {
        if (h.path_idx < geom.consumed_paths.len() && geom.consumed_paths[h.path_idx])
            || (h.a1 - h.a0) < min_band_w
        {
            continue;
        }
        if tables_out.iter().any(|t| h.coord >= t.bot_y - 4.0 && h.coord <= t.top_y + 4.0) {
            continue;
        }
        intervals.push((h.coord - 2.0, h.coord + 2.0, h.a0, h.a1));
    }
    for r in &geom.fill_rects {
        if (r.path_idx < geom.consumed_paths.len() && geom.consumed_paths[r.path_idx])
            || r.w < min_band_w
            || r.h < 9.0
            || r.h > 44.0
        {
            continue;
        }
        if r.blended_color(page_bg) == page_bg {
            continue;
        }
        if tables_out.iter().any(|t| r.y + r.h >= t.bot_y - 2.0 && r.y <= t.top_y + 2.0) {
            continue;
        }
        intervals.push((r.y, r.y + r.h, r.x, r.x + r.w));
    }
    if intervals.len() < 2 {
        return;
    }

    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut zones: Vec<(f32, f32, f32, f32, usize)> = Vec::new();
    for &(y0, y1, x0, x1) in &intervals {
        if let Some(last) = zones.last_mut() {
            if y0 <= last.1 + 38.0 {
                last.0 = last.0.min(y0);
                last.1 = last.1.max(y1);
                last.2 = last.2.min(x0);
                last.3 = last.3.max(x1);
                last.4 += 1;
                continue;
            }
        }
        zones.push((y0, y1, x0, x1, 1));
    }

    for (raw_y0, raw_y1, zx0, zx1, prim_count) in zones {
        if prim_count < 2 || (zx1 - zx0) < min_band_w {
            continue;
        }
        // Expand slightly to catch header labels immediately above a top rule or totals just below a rule
        let mut zy0 = raw_y0 - 20.0;
        let mut zy1 = raw_y1 + 20.0;
        for t in tables_out.iter() {
            if t.bot_y >= raw_y1 && zy1 > t.bot_y - 2.0 {
                zy1 = t.bot_y - 2.0;
            }
            if t.top_y <= raw_y0 && zy0 < t.top_y + 2.0 {
                zy0 = t.top_y + 2.0;
            }
        }
        if zy1 - zy0 < 20.0 {
            continue;
        }

        let mut cand_span_indices = Vec::new();
        let mut cand_spans = Vec::new();
        for (si, sp) in spans.iter().enumerate() {
            if consumed[si] || sp.text.trim().is_empty() {
                continue;
            }
            let mid_y = sp.y + sp.size * 0.3;
            if mid_y >= zy0 && mid_y <= zy1 && sp.x + sp.width >= zx0 - 8.0 && sp.x <= zx1 + 8.0 {
                cand_span_indices.push(si);
                cand_spans.push(sp.clone());
            }
        }
        let mut zone_lines = group_spans_into_lines(cand_spans);
        if zone_lines.len() < 2 {
            continue;
        }

        // Trim leading/trailing single-chunk prose lines that sit outside [raw_y0 - 3.0, raw_y1 + 3.0]
        while zone_lines.len() >= 2 {
            let first = &zone_lines[0];
            if first.y > raw_y1 + 3.0 && split_line_into_chunks(first, 11.0).len() < 2 {
                zone_lines.remove(0);
            } else {
                break;
            }
        }
        while zone_lines.len() >= 2 {
            let last = &zone_lines[zone_lines.len() - 1];
            if last.y + last.size * 0.5 < raw_y0 - 3.0 && split_line_into_chunks(last, 11.0).len() < 2 {
                zone_lines.pop();
            } else {
                break;
            }
        }
        if zone_lines.len() < 2 {
            continue;
        }

        let line_chunks: Vec<Vec<(f32, f32, Vec<TextSpan>)>> = zone_lines
            .iter()
            .map(|lg| split_line_into_chunks(lg, 11.0))
            .collect();
        let multi_col_lines = line_chunks.iter().filter(|c| c.len() >= 2).count();
        if multi_col_lines < 2 || multi_col_lines * 2 < zone_lines.len() {
            continue;
        }

        // Cluster chunk X intervals across all multi-chunk lines to discover columns
        let mut all_x_ivs: Vec<(f32, f32)> = Vec::new();
        for chs in &line_chunks {
            if chs.len() >= 2 {
                for &(x0, x1, _) in chs {
                    all_x_ivs.push((x0, x1));
                }
            }
        }
        all_x_ivs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut col_ivs: Vec<(f32, f32)> = Vec::new();
        for (x0, x1) in all_x_ivs {
            if let Some(last) = col_ivs.last_mut() {
                if x0 <= last.1 + 6.0 {
                    last.1 = last.1.max(x1);
                    continue;
                }
            }
            col_ivs.push((x0, x1));
        }

        // If a multi-chunk line has two chunks landing inside the same merged col_iv, split that col_iv at the gap
        for chs in &line_chunks {
            if chs.len() < 2 {
                continue;
            }
            for pair in chs.windows(2) {
                let split_x = (pair[0].1 + pair[1].0) * 0.5;
                if let Some(pos) = col_ivs
                    .iter()
                    .position(|&(c0, c1)| pair[0].0 >= c0 - 1.0 && pair[1].1 <= c1 + 1.0 && pair[1].0 > pair[0].1 + 6.0)
                {
                    let (old_c0, old_c1) = col_ivs[pos];
                    if split_x - old_c0 >= 14.0 && old_c1 - split_x >= 14.0 {
                        col_ivs[pos] = (old_c0, split_x - 2.0);
                        col_ivs.insert(pos + 1, (split_x + 2.0, old_c1));
                    }
                }
            }
        }

        if col_ivs.len() < 2 || col_ivs.len() > 8 {
            continue;
        }

        let tbl_x0 = zx0.min(col_ivs[0].0);
        let tbl_x1 = zx1.max(col_ivs[col_ivs.len() - 1].1);
        let num_cols = col_ivs.len();
        let mut col_bounds = Vec::with_capacity(num_cols + 1);
        col_bounds.push(tbl_x0);
        for i in 0..num_cols - 1 {
            col_bounds.push((col_ivs[i].1 + col_ivs[i + 1].0) * 0.5);
        }
        col_bounds.push(tbl_x1);

        let grid_cols_in: Vec<f32> = col_bounds
            .windows(2)
            .map(|w| ((w[1] - w[0]) / 72.0).max(0.20))
            .collect();

        // Mark spans contained in kept `zone_lines` as consumed
        let top_line_y = zone_lines[0].y + zone_lines[0].size;
        let bot_line_y = zone_lines[zone_lines.len() - 1].y - 4.0;
        for &si in &cand_span_indices {
            let sp = &spans[si];
            if sp.y <= top_line_y + 1.0 && sp.y >= bot_line_y - 1.0 {
                consumed[si] = true;
            }
        }

        let tid = *next_table_id;
        *next_table_id += 1;
        let mut rows_spec = Vec::with_capacity(zone_lines.len());
        let mut tbl_paras = Vec::new();
        let mut est_tbl_h = 0.0f32;

        let n_rows = zone_lines.len();
        let mut row_tops = vec![0.0f32; n_rows];
        let mut row_bots = vec![0.0f32; n_rows];
        for i in 0..n_rows {
            row_tops[i] = if i == 0 {
                (zone_lines[0].y + zone_lines[0].size * 1.15).max(raw_y1.min(zone_lines[0].y + zone_lines[0].size * 1.6))
            } else {
                let prev_bot = zone_lines[i - 1].y - zone_lines[i - 1].size * 0.25;
                let cur_top = zone_lines[i].y + zone_lines[i].size * 0.95;
                (prev_bot + cur_top) * 0.5
            };
            row_bots[i] = if i + 1 == n_rows {
                (zone_lines[i].y - zone_lines[i].size * 0.40).min(raw_y0.max(zone_lines[i].y - zone_lines[i].size * 0.9))
            } else {
                let cur_bot = zone_lines[i].y - zone_lines[i].size * 0.25;
                let next_top = zone_lines[i + 1].y + zone_lines[i + 1].size * 0.95;
                (cur_bot + next_top) * 0.5
            };
        }

        for r_idx in 0..n_rows {
            let y_top = row_tops[r_idx];
            let y_bot = row_bots[r_idx];
            let row_h = (y_top - y_bot).max(10.0);
            let lg = &zone_lines[r_idx];
            let chs = &line_chunks[r_idx];

            // Assign each chunk to the best-overlapping column (allowing wide single-chunk rows to span columns)
            let is_banner_row = chs.len() == 1 && (chs[0].1 - chs[0].0) > (col_bounds[1] - col_bounds[0]) + 18.0;
            let mut cells_spec = Vec::new();

            if is_banner_row {
                let top_b = if r_idx == 0 {
                    edge_border_spec(&geom.h_segs, y_top, tbl_x0, tbl_x1, 5.5, &mut geom.consumed_paths)
                } else {
                    None
                };
                let bot_b = edge_border_spec(&geom.h_segs, y_bot, tbl_x0, tbl_x1, 5.5, &mut geom.consumed_paths);
                let shading = find_cell_rect_shading(geom, tbl_x0, tbl_x1, y_bot, y_top, page_bg);
                let adjusted_spans: Vec<TextSpan> = lg
                    .spans
                    .iter()
                    .map(|sp| {
                        let mut s = sp.clone();
                        if shading.is_some() && s.highlight == shading {
                            s.highlight = None;
                        }
                        s.color = ensure_legible_color(s.color, shading, s.highlight, page_bg);
                        s
                    })
                    .collect();
                let single_lg = [LineGroup {
                    y: lg.y,
                    min_x: lg.min_x,
                    max_x: lg.max_x,
                    size: lg.size,
                    spans: adjusted_spans,
                }];
                let cell_est = populate_table_cell_paras(
                    &single_lg,
                    tbl_x0,
                    tbl_x1,
                    tbl_x1 - tbl_x0,
                    tid,
                    r_idx as u16,
                    0,
                    &mut tbl_paras,
                );
                let min_h = (row_h * 0.88).clamp(10.0, 96.0);
                est_tbl_h += (cell_est + 3.0).max(min_h);
                cells_spec.push(TableCellSpec {
                    width_in: grid_cols_in.iter().sum(),
                    grid_span: num_cols as u16,
                    shading,
                    borders: CellBorders {
                        top: top_b,
                        bottom: bot_b,
                        left: None,
                        right: None,
                    },
                    v_align: CellVAlign::Center,
                    pad_left_in: 0.05,
                    pad_right_in: 0.05,
                    pad_top_in: 0.02,
                    pad_bottom_in: 0.02,
                    ..TableCellSpec::default()
                });
            } else {
                let mut col_buckets: Vec<Vec<TextSpan>> = vec![Vec::new(); num_cols];
                for (cx0, cx1, cspans) in chs {
                    let mid = (cx0 + cx1) * 0.5;
                    let mut best_c = 0usize;
                    let mut best_dist = f32::INFINITY;
                    for c in 0..num_cols {
                        let b0 = col_bounds[c];
                        let b1 = col_bounds[c + 1];
                        let dist = if mid >= b0 && mid <= b1 {
                            0.0
                        } else if mid < b0 {
                            b0 - mid
                        } else {
                            mid - b1
                        };
                        if dist < best_dist {
                            best_dist = dist;
                            best_c = c;
                        }
                    }
                    col_buckets[best_c].extend(cspans.iter().cloned());
                }

                let mut max_cell_est = 0.0f32;
                for (c, bucket) in col_buckets.into_iter().enumerate() {
                    let bx0 = col_bounds[c];
                    let bx1 = col_bounds[c + 1];
                    let top_b = if r_idx == 0 {
                        edge_border_spec(&geom.h_segs, y_top, bx0, bx1, 5.5, &mut geom.consumed_paths)
                    } else {
                        None
                    };
                    let bot_b = edge_border_spec(&geom.h_segs, y_bot, bx0, bx1, 5.5, &mut geom.consumed_paths);
                    let shading = find_cell_rect_shading(geom, bx0, bx1, y_bot, y_top, page_bg);
                    let adjusted: Vec<TextSpan> = bucket
                        .into_iter()
                        .map(|mut s| {
                            if shading.is_some() && s.highlight == shading {
                                s.highlight = None;
                            }
                            s.color = ensure_legible_color(s.color, shading, s.highlight, page_bg);
                            s
                        })
                        .collect();
                    let clines = group_spans_into_lines(adjusted);
                    let cell_est = populate_table_cell_paras(
                        &clines,
                        bx0,
                        bx1,
                        bx1 - bx0,
                        tid,
                        r_idx as u16,
                        c as u16,
                        &mut tbl_paras,
                    );
                    max_cell_est = max_cell_est.max(cell_est + 3.0);
                    cells_spec.push(TableCellSpec {
                        width_in: grid_cols_in[c],
                        grid_span: 1,
                        shading,
                        borders: CellBorders {
                            top: top_b,
                            bottom: bot_b,
                            left: None,
                            right: None,
                        },
                        v_align: CellVAlign::Center,
                        pad_left_in: 0.05,
                        pad_right_in: 0.05,
                        pad_top_in: 0.02,
                        pad_bottom_in: 0.02,
                        ..TableCellSpec::default()
                    });
                }
                let min_h = (row_h * 0.88).clamp(10.0, 96.0);
                est_tbl_h += max_cell_est.max(min_h);
            }

            rows_spec.push(TableRowSpec {
                cells: cells_spec,
                min_height_pt: (row_h * 0.88).clamp(10.0, 96.0),
                grid_after: 0,
            });
        }

        let top_y = row_tops.first().copied().unwrap_or(raw_y1);
        let bot_y = row_bots.last().copied().unwrap_or(raw_y0);
        tables_out.push(DetectedTable {
            top_y,
            bot_y,
            left_x: tbl_x0,
            right_x: tbl_x1,
            est_h_pt: est_tbl_h,
            doc_table: DocTable {
                id: tid,
                grid_cols_in,
                rows: rows_spec,
                indent_in: 0.0,
            },
            paras: tbl_paras,
        });
    }
}

fn find_cell_rect_shading(
    geom: &mut PageGeometry,
    cx0: f32,
    cx1: f32,
    y_bot: f32,
    y_top: f32,
    page_bg: [u8; 3],
) -> Option<[u8; 3]> {
    let cell_area = ((cx1 - cx0) * (y_top - y_bot)).max(1.0);
    let mut best_cov = 0.0f32;
    let mut best_col = None;
    let mut best_pidx = None;
    for r in &geom.fill_rects {
        if r.w >= geom.pw * 0.88 && r.h >= geom.ph * 0.88 {
            continue;
        }
        let eff_c = r.blended_color(page_bg);
        if eff_c == page_bg {
            continue;
        }
        let ox = (cx1.min(r.x + r.w) - cx0.max(r.x)).max(0.0);
        let oy = (y_top.min(r.y + r.h) - y_bot.max(r.y)).max(0.0);
        let cov = (ox * oy) / cell_area;
        if cov >= 0.40 && cov > best_cov {
            best_cov = cov;
            best_col = Some(eff_c);
            best_pidx = Some(r.path_idx);
        }
    }
    if let Some(pidx) = best_pidx {
        if pidx < geom.consumed_paths.len() {
            geom.consumed_paths[pidx] = true;
        }
    }
    best_col
}

fn collect_standalone_rules(
    geom: &mut PageGeometry,
    page_tables: &[DetectedTable],
    page_bg: [u8; 3],
) -> Vec<StyledRule> {
    let mut out: Vec<StyledRule> = Vec::new();
    for h in &geom.h_segs {
        if h.path_idx < geom.consumed_paths.len() && geom.consumed_paths[h.path_idx] {
            continue;
        }
        if (h.a1 - h.a0) >= geom.pw * 0.45
            && h.color != page_bg
            && !page_tables.iter().any(|t| h.coord >= t.bot_y - 4.0 && h.coord <= t.top_y + 4.0)
        {
            if h.path_idx < geom.consumed_paths.len() {
                geom.consumed_paths[h.path_idx] = true;
            }
            if !out.iter().any(|r| (r.y - h.coord).abs() <= 4.0) {
                out.push(StyledRule {
                    y: h.coord,
                    x0: h.a0,
                    x1: h.a1,
                    width_pt: h.width_pt.clamp(0.5, 6.0),
                    color: h.color,
                });
            }
        }
    }
    out
}

fn mark_banner_rects_consumed(
    geom: &mut PageGeometry,
    spans: &[TextSpan],
    consumed_spans: &[bool],
    page_bg: [u8; 3],
) {
    let unconsumed: Vec<TextSpan> = spans
        .iter()
        .enumerate()
        .filter_map(|(i, s)| (!consumed_spans[i]).then(|| s.clone()))
        .collect();
    let lines = group_spans_into_lines(unconsumed);
    for lg in &lines {
        let mid_y = lg.y + lg.size * 0.35;
        for r in &geom.fill_rects {
            if r.w >= geom.pw * 0.88 && r.h >= geom.ph * 0.88 {
                continue;
            }
            if r.blended_color(page_bg) == page_bg {
                continue;
            }
            if r.w >= geom.pw * 0.42
                && r.h >= lg.size * 0.8
                && r.h <= lg.size * 3.8
                && mid_y >= r.y - 2.0
                && mid_y <= r.y + r.h + 2.0
                && r.path_idx < geom.consumed_paths.len()
            {
                geom.consumed_paths[r.path_idx] = true;
            }
        }
    }
}

// ── Pure-Rust Anti-Aliased Vector Artwork & Chart Rasterizer ────────────

fn extract_unconsumed_vector_graphics(
    geom: &mut PageGeometry,
    page_idx: usize,
    spans: &[TextSpan],
    consumed_spans: &mut [bool],
    page_imgs: &mut Vec<PlacedImage>,
    page_bg: [u8; 3],
) {
    let mut cand_indices = Vec::new();
    for (pidx, vp) in geom.vec_paths.iter().enumerate() {
        if geom.consumed_paths.get(pidx).copied().unwrap_or(true) {
            continue;
        }
        let has_vis_fill = vp.fill.is_some_and(|(c, a, _)| a > 0.02 && lerp_rgb(page_bg, c, a) != page_bg);
        let has_vis_stroke = vp.stroke.is_some_and(|(_, a, _)| a > 0.02);
        if !has_vis_fill && !has_vis_stroke {
            continue;
        }
        let [x0, y0, x1, y1] = vp.bbox;
        if x1 < 0.0 || y1 < 0.0 || x0 > geom.pw || y0 > geom.ph {
            continue;
        }
        let bw = x1 - x0;
        let bh = y1 - y0;
        if bw < 2.2 && bh < 2.2 {
            continue;
        }
        // Skip full-page decorative border frames
        if bw >= geom.pw * 0.72 && bh >= geom.ph * 0.55 && !vp.has_non_axis_geom && vp.subpaths.len() <= 2 {
            continue;
        }
        cand_indices.push(pidx);
    }
    if cand_indices.is_empty() {
        return;
    }

    // Cluster nearby vector primitives into connected figures (logos, seals, charts, diagrams)
    let n = cand_indices.len();
    let mut visited = vec![false; n];
    let mut clusters: Vec<(Vec<usize>, [f32; 4])> = Vec::new();

    for start in 0..n {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut group = vec![cand_indices[start]];
        let mut bb = geom.vec_paths[cand_indices[start]].bbox;
        let mut changed = true;
        while changed {
            changed = false;
            for j in 0..n {
                if visited[j] {
                    continue;
                }
                let jb = geom.vec_paths[cand_indices[j]].bbox;
                let pad = 15.0;
                if jb[0] <= bb[2] + pad && jb[2] >= bb[0] - pad && jb[1] <= bb[3] + pad && jb[3] >= bb[1] - pad {
                    visited[j] = true;
                    group.push(cand_indices[j]);
                    bb[0] = bb[0].min(jb[0]);
                    bb[1] = bb[1].min(jb[1]);
                    bb[2] = bb[2].max(jb[2]);
                    bb[3] = bb[3].max(jb[3]);
                    changed = true;
                }
            }
        }
        clusters.push((group, bb));
    }

    let mut font_ctx: Option<(eframe::egui::Context, crate::fonts::FontBook)> = None;

    for (group, mut bb) in clusters {
        let max_lw = group
            .iter()
            .filter_map(|&pi| geom.vec_paths[pi].stroke.map(|(_, _, lw)| lw))
            .fold(1.0f32, f32::max);
        let pad = (max_lw * 0.6 + 1.5).min(8.0);
        bb[0] = (bb[0] - pad).max(0.0);
        bb[1] = (bb[1] - pad).max(0.0);
        bb[2] = (bb[2] + pad).min(geom.pw);
        bb[3] = (bb[3] + pad).min(geom.ph);
        let gw = bb[2] - bb[0];
        let gh = bb[3] - bb[1];
        let any_non_axis = group.iter().any(|&pi| geom.vec_paths[pi].has_non_axis_geom);
        if gw < 5.0 || gh < 5.0 || (gh > geom.ph * 0.68 && group.len() <= 5 && !any_non_axis) {
            continue;
        }

        // Check overlapping unconsumed text spans:
        // - Skip simple 1-2 box callout frames wrapping wide body prose so prose stays editable text
        // - Absorb short labels inside multi-primitive charts/seals/diagrams/flowcharts so labels render in place
        let mut overlapping_spans = Vec::new();
        let mut max_span_w = 0.0f32;
        let mut total_chars = 0usize;
        for (si, sp) in spans.iter().enumerate() {
            if consumed_spans[si] || sp.text.trim().is_empty() {
                continue;
            }
            let cx = sp.x + sp.width * 0.5;
            let cy = sp.y + sp.size * 0.35;
            if cx >= bb[0] - 8.0 && cx <= bb[2] + 8.0 && cy >= bb[1] - 10.0 && cy <= bb[3] + 8.0 {
                max_span_w = max_span_w.max(sp.width);
                total_chars += sp.text.chars().count();
                overlapping_spans.push(si);
            }
        }

        if group.len() <= 2 && !any_non_axis && (max_span_w > 130.0 || total_chars > 42) {
            continue;
        }

        let max_label_chars = if group.len() >= 8 { 110 } else { 42 };
        let mut absorbed_spans: Vec<TextSpan> = Vec::new();
        if (group.len() >= 3 || any_non_axis) && !overlapping_spans.is_empty() && max_span_w <= gw.max(140.0) {
            for si in overlapping_spans {
                let sp = &spans[si];
                if sp.text.chars().count() <= max_label_chars && sp.width <= gw + 28.0 {
                    consumed_spans[si] = true;
                    bb[0] = bb[0].min((sp.x - 2.0).max(0.0));
                    bb[1] = bb[1].min((sp.y - sp.size * 0.32).max(0.0));
                    bb[2] = bb[2].max((sp.x + sp.width + 4.0).min(geom.pw));
                    bb[3] = bb[3].max((sp.y + sp.size * 1.05).min(geom.ph));
                    absorbed_spans.push(sp.clone());
                }
            }
        } else if !overlapping_spans.is_empty() {
            // Simple 1-2 rect shape colliding with text: leave text editable and skip box
            continue;
        }

        for &pi in &group {
            if pi < geom.consumed_paths.len() {
                geom.consumed_paths[pi] = true;
            }
        }

        let final_w = (bb[2] - bb[0]).max(4.0);
        let final_h = (bb[3] - bb[1]).max(4.0);
        if let Some(png_bytes) = rasterize_vector_cluster(
            &geom.vec_paths,
            &group,
            &absorbed_spans,
            bb,
            &mut font_ctx,
        ) {
            page_imgs.push(PlacedImage {
                page: page_idx,
                x: bb[0],
                y: bb[1],
                w: final_w,
                h: final_h,
                png_bytes,
                seamless: true,
            });
        }
    }
}

fn blend_rgba_src_over(px: &mut [u8], col: [u8; 3], alpha: f32) {
    let sa = alpha.clamp(0.0, 1.0);
    if sa <= 0.002 {
        return;
    }
    let da = px[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 1e-5 {
        return;
    }
    let inv = 1.0 - sa;
    px[0] = ((col[0] as f32 * sa + px[0] as f32 * da * inv) / out_a).round().clamp(0.0, 255.0) as u8;
    px[1] = ((col[1] as f32 * sa + px[1] as f32 * da * inv) / out_a).round().clamp(0.0, 255.0) as u8;
    px[2] = ((col[2] as f32 * sa + px[2] as f32 * da * inv) / out_a).round().clamp(0.0, 255.0) as u8;
    px[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

fn rasterize_vector_cluster(
    all_paths: &[VecPath],
    indices: &[usize],
    labels: &[TextSpan],
    bb: [f32; 4],
    font_ctx: &mut Option<(eframe::egui::Context, crate::fonts::FontBook)>,
) -> Option<Vec<u8>> {
    let gw = (bb[2] - bb[0]).max(2.0);
    let gh = (bb[3] - bb[1]).max(2.0);
    let mut scale = 3.0f32; // 216 DPI
    if gw * scale * gh * scale > 1600.0 * 1600.0 {
        scale = ((1600.0 * 1600.0) / (gw * gh)).sqrt().clamp(1.0, 3.0);
    }
    let px_w = (gw * scale).ceil().clamp(2.0, 2048.0) as usize;
    let px_h = (gh * scale).ceil().clamp(2.0, 2048.0) as usize;
    let mut rgba = vec![0u8; px_w * px_h * 4];

    let to_px = |x: f32, y: f32| -> (f32, f32) { ((x - bb[0]) * scale, (bb[3] - y) * scale) };

    for &pidx in indices {
        let vp = &all_paths[pidx];

        // 1. Scanline polygon fill (2x2 supersampled horizontally/vertically)
        if let Some((fcol, falpha, even_odd)) = vp.fill {
            if falpha > 0.01 {
                let mut edges: Vec<(f32, f32, f32, f32)> = Vec::new();
                let mut min_py = px_h as f32;
                let mut max_py = 0.0f32;
                for (pts, _) in &vp.subpaths {
                    if pts.len() < 3 {
                        continue;
                    }
                    for i in 0..pts.len() {
                        let (ax, ay) = to_px(pts[i].0, pts[i].1);
                        let next = pts[(i + 1) % pts.len()];
                        let (bx, by) = to_px(next.0, next.1);
                        min_py = min_py.min(ay.min(by));
                        max_py = max_py.max(ay.max(by));
                        if (ay - by).abs() > 1e-4 {
                            edges.push((ax, ay, bx, by));
                        }
                    }
                }
                if !edges.is_empty() {
                    let y_start = (min_py.floor() as isize).max(0) as usize;
                    let y_end = (max_py.ceil() as usize).min(px_h.saturating_sub(1));
                    let mut xings: Vec<(f32, i32)> = Vec::new();
                    let mut row_cov = vec![0.0f32; px_w];

                    for py in y_start..=y_end {
                        row_cov.fill(0.0);
                        for sub in [0.25f32, 0.75f32] {
                            let sy = py as f32 + sub;
                            xings.clear();
                            for &(ax, ay, bx, by) in &edges {
                                let (y0, y1, dir) = if ay < by { (ay, by, 1) } else { (by, ay, -1) };
                                if sy >= y0 && sy < y1 {
                                    let t = (sy - ay) / (by - ay);
                                    xings.push((ax + t * (bx - ax), dir));
                                }
                            }
                            if xings.len() < 2 {
                                continue;
                            }
                            xings.sort_by(|a, b| a.0.total_cmp(&b.0));

                            if even_odd {
                                for pair in xings.chunks_exact(2) {
                                    let x_l = pair[0].0.max(0.0);
                                    let x_r = pair[1].0.min(px_w as f32);
                                    if x_r > x_l {
                                        let p0 = (x_l.floor() as usize).min(px_w - 1);
                                        let p1 = (x_r.ceil() as usize).min(px_w);
                                        for (px_i, cov_slot) in row_cov.iter_mut().enumerate().take(p1).skip(p0) {
                                            let c0 = x_l.max(px_i as f32);
                                            let c1 = x_r.min((px_i + 1) as f32);
                                            if c1 > c0 {
                                                *cov_slot += (c1 - c0) * 0.5;
                                            }
                                        }
                                    }
                                }
                            } else {
                                let mut wind = 0i32;
                                let mut span_start = 0.0f32;
                                for &(xx, dir) in &xings {
                                    if wind == 0 {
                                        span_start = xx;
                                    }
                                    wind += dir;
                                    if wind == 0 {
                                        let x_l = span_start.max(0.0);
                                        let x_r = xx.min(px_w as f32);
                                        if x_r > x_l {
                                            let p0 = (x_l.floor() as usize).min(px_w - 1);
                                            let p1 = (x_r.ceil() as usize).min(px_w);
                                            for (px_i, cov_slot) in row_cov.iter_mut().enumerate().take(p1).skip(p0) {
                                                let c0 = x_l.max(px_i as f32);
                                                let c1 = x_r.min((px_i + 1) as f32);
                                                if c1 > c0 {
                                                    *cov_slot += (c1 - c0) * 0.5;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        let row_off = py * px_w * 4;
                        for (px_i, &cov) in row_cov.iter().enumerate() {
                            if cov > 0.005 {
                                blend_rgba_src_over(&mut rgba[row_off + px_i * 4..row_off + px_i * 4 + 4], fcol, falpha * cov.min(1.0));
                            }
                        }
                    }
                }
            }
        }

        // 2. Anti-aliased capsule stroke
        if let Some((scol, salpha, lw_pt)) = vp.stroke {
            if salpha > 0.01 {
                let sw = (lw_pt * scale).max(0.85);
                let half = sw * 0.5;
                let aa_pad = half + 1.5;
                for (pts, closed) in &vp.subpaths {
                    if pts.len() < 2 {
                        continue;
                    }
                    let seg_count = if *closed && pts.len() >= 3 { pts.len() } else { pts.len() - 1 };
                    for i in 0..seg_count {
                        let (ax, ay) = to_px(pts[i].0, pts[i].1);
                        let next = pts[(i + 1) % pts.len()];
                        let (bx, by) = to_px(next.0, next.1);
                        let min_x = ((ax.min(bx) - aa_pad).floor() as isize).max(0) as usize;
                        let max_x = ((ax.max(bx) + aa_pad).ceil() as isize).clamp(0, px_w as isize - 1) as usize;
                        let min_y = ((ay.min(by) - aa_pad).floor() as isize).max(0) as usize;
                        let max_y = ((ay.max(by) + aa_pad).ceil() as isize).clamp(0, px_h as isize - 1) as usize;
                        let vx = bx - ax;
                        let vy = by - ay;
                        let len_sq = (vx * vx + vy * vy).max(1e-5);
                        for py in min_y..=max_y {
                            let fy = py as f32 + 0.5;
                            let row_off = py * px_w * 4;
                            for px_i in min_x..=max_x {
                                let fx = px_i as f32 + 0.5;
                                let t = (((fx - ax) * vx + (fy - ay) * vy) / len_sq).clamp(0.0, 1.0);
                                let d = (fx - (ax + t * vx)).hypot(fy - (ay + t * vy)) - half;
                                if d < 0.5 {
                                    let cov = (0.5 - d).clamp(0.0, 1.0);
                                    blend_rgba_src_over(
                                        &mut rgba[row_off + px_i * 4..row_off + px_i * 4 + 4],
                                        scol,
                                        salpha * cov,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. Blit absorbed chart / diagram / seal text labels onto the graphic buffer
    if !labels.is_empty() {
        blit_spans_onto_rgba(&mut rgba, px_w, px_h, bb, scale, labels, font_ctx);
    }

    let mut png_bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png_bytes)
        .write_image(&rgba, px_w as u32, px_h as u32, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(png_bytes)
}

fn blit_spans_onto_rgba(
    rgba: &mut [u8],
    px_w: usize,
    px_h: usize,
    bb: [f32; 4],
    scale: f32,
    labels: &[TextSpan],
    font_ctx: &mut Option<(eframe::egui::Context, crate::fonts::FontBook)>,
) {
    if labels.is_empty() {
        return;
    }
    let (ctx, book) = font_ctx.get_or_insert_with(|| {
        let c = eframe::egui::Context::default();
        let b = crate::fonts::install(&c);
        let mut warm = c.run_ui(Default::default(), |_ui| {});
        warm.textures_delta.clear();
        (c, b)
    });
    let mut galleys: Vec<(f32, f32, [u8; 3], std::sync::Arc<eframe::egui::Galley>)> = Vec::new();
    let mut out = ctx.run_ui(Default::default(), |_ui| {
        galleys = ctx.fonts_mut(|fonts| {
            labels
                .iter()
                .map(|sp| {
                    let (fam, fake_it) = book.family(sp.font, sp.bold, sp.italic);
                    let mut job = eframe::egui::text::LayoutJob::default();
                    job.append(
                        &sp.text,
                        0.0,
                        eframe::egui::TextFormat {
                            font_id: eframe::egui::FontId::new((sp.size * scale).max(6.0), fam),
                            color: eframe::egui::Color32::from_rgb(sp.color[0], sp.color[1], sp.color[2]),
                            italics: fake_it,
                            ..Default::default()
                        },
                    );
                    let dx = (sp.x - bb[0]) * scale;
                    let dy = (bb[3] - (sp.y + sp.size * 0.90)) * scale;
                    (dx, dy, sp.color, fonts.layout_job(job))
                })
                .collect()
        });
    });
    out.textures_delta.clear();
    let atlas = ctx.fonts_mut(|f| f.image());
    let atlas_w = atlas.width();
    let atlas_h = atlas.height();
    for (ox, oy, col, galley) in galleys {
        if let Some(placed) = galley.rows.first() {
            for g in &placed.row.glyphs {
                let uv = g.uv_rect;
                if uv.is_nothing() {
                    continue;
                }
                let u0 = uv.min[0] as usize;
                let v0 = uv.min[1] as usize;
                let u1 = (uv.max[0] as usize).min(atlas_w);
                let v1 = (uv.max[1] as usize).min(atlas_h);
                if u1 <= u0 || v1 <= v0 {
                    continue;
                }
                let dst_x0 = (ox + placed.pos.x + g.pos.x + uv.offset.x).round() as isize;
                let dst_y0 = (oy + placed.pos.y + g.pos.y + uv.offset.y).round() as isize;
                for dy in 0..(v1 - v0) {
                    let py = dst_y0 + dy as isize;
                    if py < 0 || py >= px_h as isize {
                        continue;
                    }
                    let row_off = (py as usize) * px_w * 4;
                    let a_row = (v0 + dy) * atlas_w;
                    for dx in 0..(u1 - u0) {
                        let px_i = dst_x0 + dx as isize;
                        if px_i < 0 || px_i >= px_w as isize {
                            continue;
                        }
                        let ta = atlas.pixels[a_row + u0 + dx].a();
                        if ta > 0 {
                            let dst = (px_i as usize) * 4;
                            blend_rgba_src_over(&mut rgba[row_off + dst..row_off + dst + 4], col, ta as f32 / 255.0);
                        }
                    }
                }
            }
        }
    }
}

/// Composites overlapping placed images (e.g. MRC foreground/background scan layers, magazine covers,
/// or layered vector logos over photos) in painter's draw order into unified seamless RGBA PNGs.
fn composite_overlapping_page_images(
    page_imgs: &mut Vec<PlacedImage>,
    pg_w: f32,
    pg_h: f32,
    overlay_spans: &[TextSpan],
) {
    if page_imgs.is_empty() {
        return;
    }
    let has_full_scan = page_imgs
        .iter()
        .any(|im| im.w >= pg_w * 0.84 && im.h >= pg_h * 0.84);

    if page_imgs.len() == 1 && overlay_spans.is_empty() {
        return;
    }

    let n = page_imgs.len();
    let mut comp_of = vec![usize::MAX; n];
    let mut comps: Vec<Vec<usize>> = Vec::new();

    if has_full_scan {
        comps.push((0..n).collect());
    } else {
        for start in 0..n {
            if comp_of[start] != usize::MAX {
                continue;
            }
            let cid = comps.len();
            comp_of[start] = cid;
            let mut members = vec![start];
            let mut changed = true;
            while changed {
                changed = false;
                for j in 0..n {
                    if comp_of[j] != usize::MAX {
                        continue;
                    }
                    let b = &page_imgs[j];
                    let overlaps_any = members.iter().any(|&mi| {
                        let a = &page_imgs[mi];
                        let ox = ((a.x + a.w).min(b.x + b.w) - a.x.max(b.x)).max(0.0);
                        let oy = ((a.y + a.h).min(b.y + b.h) - a.y.max(b.y)).max(0.0);
                        let ov_area = ox * oy;
                        let min_area = (a.w * a.h).min(b.w * b.h).max(1.0);
                        ov_area >= min_area * 0.16
                    });
                    if overlaps_any {
                        comp_of[j] = cid;
                        members.push(j);
                        changed = true;
                    }
                }
            }
            members.sort_unstable(); // Preserve original PDF painter's draw order!
            comps.push(members);
        }
    }

    if comps.iter().all(|c| c.len() <= 1) && overlay_spans.is_empty() {
        return;
    }

    let mut font_ctx: Option<(eframe::egui::Context, crate::fonts::FontBook)> = None;
    let orig = std::mem::take(page_imgs);
    let mut out_imgs = Vec::with_capacity(comps.len());

    for (ci, members) in comps.into_iter().enumerate() {
        let attach_spans = if ci == 0 && !overlay_spans.is_empty() {
            overlay_spans
        } else {
            &[]
        };
        if members.len() == 1 && attach_spans.is_empty() {
            out_imgs.push(orig[members[0]].clone());
            continue;
        }

        let page_num = orig[members[0]].page;
        let mut ux0 = f32::INFINITY;
        let mut uy0 = f32::INFINITY;
        let mut ux1 = f32::NEG_INFINITY;
        let mut uy1 = f32::NEG_INFINITY;
        for &mi in &members {
            let im = &orig[mi];
            ux0 = ux0.min(im.x);
            uy0 = uy0.min(im.y);
            ux1 = ux1.max(im.x + im.w);
            uy1 = uy1.max(im.y + im.h);
        }
        for sp in attach_spans {
            ux0 = ux0.min(sp.x - 2.0);
            uy0 = uy0.min(sp.y - sp.size * 0.30);
            ux1 = ux1.max(sp.x + sp.width + 4.0);
            uy1 = uy1.max(sp.y + sp.size * 1.05);
        }
        ux0 = ux0.clamp(0.0, pg_w);
        uy0 = uy0.clamp(0.0, pg_h);
        ux1 = ux1.clamp((ux0 + 2.0).min(pg_w), pg_w);
        uy1 = uy1.clamp((uy0 + 2.0).min(pg_h), pg_h);
        let uw = (ux1 - ux0).max(2.0);
        let uh = (uy1 - uy0).max(2.0);

        let mut scale = 2.25f32;
        if uw * scale * uh * scale > 1800.0 * 1800.0 {
            scale = ((1800.0 * 1800.0) / (uw * uh)).sqrt().clamp(1.0, 2.25);
        }
        let px_w = (uw * scale).ceil().clamp(2.0, 2200.0) as usize;
        let px_h = (uh * scale).ceil().clamp(2.0, 2200.0) as usize;
        let mut rgba = vec![0u8; px_w * px_h * 4];

        for &mi in &members {
            let im = &orig[mi];
            let Ok(decoded) = image::load_from_memory(&im.png_bytes) else {
                continue;
            };
            let src = decoded.to_rgba8();
            let sw = src.width() as usize;
            let sh = src.height() as usize;
            if sw == 0 || sh == 0 {
                continue;
            }
            let src_raw = src.as_raw();
            let dx0_f = (im.x - ux0) * scale;
            let dy0_f = (uy1 - (im.y + im.h)) * scale;
            let dw_f = (im.w * scale).max(1.0);
            let dh_f = (im.h * scale).max(1.0);
            let x_start = (dx0_f.floor() as isize).max(0) as usize;
            let x_end = ((dx0_f + dw_f).ceil() as isize).clamp(0, px_w as isize) as usize;
            let y_start = (dy0_f.floor() as isize).max(0) as usize;
            let y_end = ((dy0_f + dh_f).ceil() as isize).clamp(0, px_h as isize) as usize;

            for py in y_start..y_end {
                let v = (((py as f32 + 0.5) - dy0_f) / dh_f).clamp(0.0, 0.999999);
                let sy = ((v * sh as f32) as usize).min(sh - 1);
                let dst_row = py * px_w * 4;
                let src_row = sy * sw * 4;
                for px_i in x_start..x_end {
                    let u = (((px_i as f32 + 0.5) - dx0_f) / dw_f).clamp(0.0, 0.999999);
                    let sx = ((u * sw as f32) as usize).min(sw - 1);
                    let s_off = src_row + sx * 4;
                    let sa = src_raw[s_off + 3];
                    if sa > 0 {
                        let d_off = dst_row + px_i * 4;
                        blend_rgba_src_over(
                            &mut rgba[d_off..d_off + 4],
                            [src_raw[s_off], src_raw[s_off + 1], src_raw[s_off + 2]],
                            sa as f32 / 255.0,
                        );
                    }
                }
            }
        }

        if !attach_spans.is_empty() {
            blit_spans_onto_rgba(
                &mut rgba,
                px_w,
                px_h,
                [ux0, uy0, ux1, uy1],
                scale,
                attach_spans,
                &mut font_ctx,
            );
        }

        let mut png_bytes = Vec::new();
        if image::codecs::png::PngEncoder::new(&mut png_bytes)
            .write_image(&rgba, px_w as u32, px_h as u32, image::ExtendedColorType::Rgba8)
            .is_ok()
        {
            out_imgs.push(PlacedImage {
                page: page_num,
                x: ux0,
                y: uy0,
                w: uw,
                h: uh,
                png_bytes,
                seamless: true,
            });
        } else {
            for &mi in &members {
                out_imgs.push(orig[members[0].max(mi)].clone());
            }
        }
    }

    *page_imgs = out_imgs;
}

#[allow(clippy::too_many_arguments)]
fn build_side_by_side_media_tables(
    doc: &mut Document,
    page_imgs: &mut Vec<PlacedImage>,
    page_lines: &mut Vec<LineGroup>,
    page_tables: &mut Vec<DetectedTable>,
    next_table_id: &mut u32,
    next_img_id: &mut u64,
    content_left: f32,
    content_right: f32,
    content_w: f32,
) {
    if page_imgs.is_empty() {
        return;
    }
    page_imgs.sort_by(|a, b| b.y.total_cmp(&a.y).then_with(|| a.x.total_cmp(&b.x)));

    // 1. Pair side-by-side images sharing the same horizontal band (e.g. dual dashboard charts)
    let mut used_img = vec![false; page_imgs.len()];
    for i in 0..page_imgs.len() {
        if used_img[i] || page_imgs[i].w > content_w * 0.62 {
            continue;
        }
        let mut group = vec![i];
        let mut gy0 = page_imgs[i].y;
        let mut gy1 = page_imgs[i].y + page_imgs[i].h;
        for j in (i + 1)..page_imgs.len() {
            if used_img[j] || page_imgs[j].w > content_w * 0.62 {
                continue;
            }
            let jy0 = page_imgs[j].y;
            let jy1 = jy0 + page_imgs[j].h;
            let ov_y = (gy1.min(jy1) - gy0.max(jy0)).max(0.0);
            let min_h = (gy1 - gy0).min(jy1 - jy0).max(1.0);
            let x_disjoint = group.iter().all(|&gi| {
                let a = &page_imgs[gi];
                let b = &page_imgs[j];
                (a.x + a.w).min(b.x + b.w) - a.x.max(b.x) <= 4.0
            });
            if ov_y >= min_h * 0.45 && x_disjoint && group.len() < 4 {
                group.push(j);
                gy0 = gy0.min(jy0);
                gy1 = gy1.max(jy1);
            }
        }
        if group.len() >= 2 {
            group.sort_by(|&a, &b| page_imgs[a].x.total_cmp(&page_imgs[b].x));

            // Check if unconsumed text lines sit in the same horizontal band [gy0 - 4, gy1 + 6]
            let mut band_line_indices = Vec::new();
            for (li, lg) in page_lines.iter().enumerate() {
                let ly0 = lg.y - lg.size * 0.25;
                let ly1 = lg.y + lg.size * 0.95;
                if ly1 >= gy0 - 4.0 && ly0 <= gy1 + 6.0 {
                    band_line_indices.push(li);
                }
            }

            if !band_line_indices.is_empty() {
                // Support common 3-part form headers: [Left Logo | Middle Title Text | Right Box/Seal]
                let i0 = &page_imgs[group[0]];
                let i1 = &page_imgs[group[1]];
                let all_between = group.len() == 2
                    && band_line_indices.iter().all(|&li| {
                        page_lines[li].min_x >= i0.x + i0.w + 3.0 && page_lines[li].max_x <= i1.x - 3.0
                    });
                if !all_between {
                    // Leave images unpaired here so Step 2 can pair a logo cleanly with its adjacent text
                    continue;
                }
                for &gi in &group {
                    used_img[gi] = true;
                }
                let mut mid_lines = Vec::with_capacity(band_line_indices.len());
                for &li in band_line_indices.iter().rev() {
                    mid_lines.push(page_lines.remove(li));
                }
                mid_lines.reverse();

                let left_x = i0.x.min(content_left);
                let right_x = (i1.x + i1.w).max(content_right);
                let s0 = (i0.x + i0.w + mid_lines.iter().map(|l| l.min_x).fold(f32::INFINITY, f32::min)) * 0.5;
                let s1 = (mid_lines.iter().map(|l| l.max_x).fold(f32::NEG_INFINITY, f32::max) + i1.x) * 0.5;
                let grid_cols_in = vec![
                    ((s0 - left_x) / 72.0).max(0.35),
                    ((s1 - s0) / 72.0).max(0.50),
                    ((right_x - s1) / 72.0).max(0.35),
                ];
                let tid = *next_table_id;
                *next_table_id += 1;
                let mut paras = Vec::new();
                if let Some(dimg0) = placed_image_to_doc_image(*next_img_id, &page_imgs[group[0]]) {
                    let id0 = *next_img_id;
                    *next_img_id += 1;
                    doc.images.push(dimg0);
                    paras.push(Para::new(
                        CharStyle::default(),
                        ParaFmt {
                            image_id: id0,
                            align: Align::Left,
                            table_id: tid,
                            table_row: 0,
                            table_col: 0,
                            ..ParaFmt::default()
                        },
                    ));
                }
                let txt_est = populate_table_cell_paras(
                    &mid_lines,
                    s0,
                    s1,
                    (s1 - s0).max(40.0),
                    tid,
                    0,
                    1,
                    &mut paras,
                );
                if let Some(dimg1) = placed_image_to_doc_image(*next_img_id, &page_imgs[group[1]]) {
                    let id1 = *next_img_id;
                    *next_img_id += 1;
                    doc.images.push(dimg1);
                    paras.push(Para::new(
                        CharStyle::default(),
                        ParaFmt {
                            image_id: id1,
                            align: Align::Right,
                            table_id: tid,
                            table_row: 0,
                            table_col: 2,
                            ..ParaFmt::default()
                        },
                    ));
                }
                let band_top = gy1.max(mid_lines[0].y + mid_lines[0].size);
                let band_bot = gy0.min(mid_lines[mid_lines.len() - 1].y - 3.0);
                let cells = grid_cols_in
                    .iter()
                    .map(|&w_in| TableCellSpec {
                        width_in: w_in,
                        grid_span: 1,
                        shading: None,
                        borders: CellBorders::default(),
                        v_align: CellVAlign::Center,
                        pad_left_in: 0.02,
                        pad_right_in: 0.02,
                        pad_top_in: 0.01,
                        pad_bottom_in: 0.01,
                        ..TableCellSpec::default()
                    })
                    .collect();
                page_tables.push(DetectedTable {
                    top_y: band_top,
                    bot_y: band_bot,
                    left_x,
                    right_x,
                    est_h_pt: (band_top - band_bot).max(txt_est) + 2.0,
                    doc_table: DocTable {
                        id: tid,
                        grid_cols_in,
                        rows: vec![TableRowSpec {
                            cells,
                            min_height_pt: (band_top - band_bot).clamp(12.0, 180.0),
                            grid_after: 0,
                        }],
                        indent_in: 0.0,
                    },
                    paras,
                });
                continue;
            }

            for &gi in &group {
                used_img[gi] = true;
            }
            let left_x = page_imgs[group[0]].x;
            let right_x = page_imgs[*group.last().unwrap()].x + page_imgs[*group.last().unwrap()].w;
            let tid = *next_table_id;
            *next_table_id += 1;

            let mut bounds = Vec::with_capacity(group.len() + 1);
            bounds.push(left_x);
            for k in 0..group.len() - 1 {
                bounds.push((page_imgs[group[k]].x + page_imgs[group[k]].w + page_imgs[group[k + 1]].x) * 0.5);
            }
            bounds.push(right_x);
            let grid_cols_in: Vec<f32> = bounds.windows(2).map(|w| ((w[1] - w[0]) / 72.0).max(0.3)).collect();

            let mut cells = Vec::with_capacity(group.len());
            let mut paras = Vec::with_capacity(group.len());
            for (c, &gi) in group.iter().enumerate() {
                let id = *next_img_id;
                let align = infer_box_align_and_indent(
                    page_imgs[gi].x,
                    page_imgs[gi].x + page_imgs[gi].w,
                    bounds[c],
                    bounds[c + 1],
                    bounds[c + 1] - bounds[c],
                )
                .0;
                if let Some(dimg) = placed_image_to_doc_image(id, &page_imgs[gi]) {
                    *next_img_id += 1;
                    doc.images.push(dimg);
                    paras.push(Para::new(
                        CharStyle::default(),
                        ParaFmt {
                            image_id: id,
                            align,
                            table_id: tid,
                            table_row: 0,
                            table_col: c as u16,
                            ..ParaFmt::default()
                        },
                    ));
                } else {
                    paras.push(Para::new(
                        CharStyle::default(),
                        ParaFmt {
                            table_id: tid,
                            table_row: 0,
                            table_col: c as u16,
                            ..ParaFmt::default()
                        },
                    ));
                }
                cells.push(TableCellSpec {
                    width_in: grid_cols_in[c],
                    grid_span: 1,
                    shading: None,
                    borders: CellBorders::default(),
                    v_align: CellVAlign::Center,
                    pad_left_in: 0.02,
                    pad_right_in: 0.02,
                    pad_top_in: 0.01,
                    pad_bottom_in: 0.01,
                    ..TableCellSpec::default()
                });
            }
            page_tables.push(DetectedTable {
                top_y: gy1,
                bot_y: gy0,
                left_x,
                right_x,
                est_h_pt: (gy1 - gy0) + 4.0,
                doc_table: DocTable {
                    id: tid,
                    grid_cols_in,
                    rows: vec![TableRowSpec {
                        cells,
                        min_height_pt: (gy1 - gy0).clamp(12.0, 360.0),
                        grid_after: 0,
                    }],
                    indent_in: 0.0,
                },
                paras,
            });
        }
    }

    // 2. Pair a single small/medium image (e.g. company logo) with side-by-side non-overlapping text lines
    for i in 0..page_imgs.len() {
        if used_img[i] || page_imgs[i].w > content_w * 0.46 || page_imgs[i].h > 160.0 {
            continue;
        }
        let img = &page_imgs[i];
        let iy0 = img.y;
        let iy1 = img.y + img.h;
        let ix0 = img.x;
        let ix1 = img.x + img.w;

        let mut side_line_indices = Vec::new();
        let mut all_right = true;
        let mut all_left = true;
        for (li, lg) in page_lines.iter().enumerate() {
            let ly0 = lg.y - lg.size * 0.25;
            let ly1 = lg.y + lg.size * 0.95;
            if ly1 >= iy0 - 4.0 && ly0 <= iy1 + 6.0 {
                if lg.min_x >= ix1 + 6.0 {
                    all_left = false;
                    side_line_indices.push(li);
                } else if lg.max_x <= ix0 - 6.0 {
                    all_right = false;
                    side_line_indices.push(li);
                } else {
                    side_line_indices.clear();
                    break;
                }
            }
        }

        if !side_line_indices.is_empty() && (all_right || all_left) {
            used_img[i] = true;
            let mut matched_lines = Vec::with_capacity(side_line_indices.len());
            for &li in side_line_indices.iter().rev() {
                matched_lines.push(page_lines.remove(li));
            }
            matched_lines.reverse();

            let txt_min_x = matched_lines.iter().map(|l| l.min_x).fold(f32::INFINITY, f32::min);
            let txt_max_x = matched_lines.iter().map(|l| l.max_x).fold(f32::NEG_INFINITY, f32::max);
            let band_top = iy1.max(matched_lines[0].y + matched_lines[0].size);
            let band_bot = iy0.min(matched_lines[matched_lines.len() - 1].y - 3.0);

            let left_x = ix0.min(txt_min_x).min(content_left);
            let right_x = ix1.max(txt_max_x).max(content_right);
            let split_x = if all_right {
                (ix1 + txt_min_x) * 0.5
            } else {
                (txt_max_x + ix0) * 0.5
            };
            let w0_in = ((split_x - left_x) / 72.0).max(0.35);
            let w1_in = ((right_x - split_x) / 72.0).max(0.35);
            let grid_cols_in = vec![w0_in, w1_in];

            let tid = *next_table_id;
            *next_table_id += 1;
            let (img_col, txt_col) = if all_right { (0u16, 1u16) } else { (1u16, 0u16) };
            let (txt_x0, txt_x1) = if all_right {
                (split_x, right_x)
            } else {
                (left_x, split_x)
            };

            let mut paras = Vec::new();
            let id = *next_img_id;
            if let Some(dimg) = placed_image_to_doc_image(id, img) {
                *next_img_id += 1;
                doc.images.push(dimg);
                let img_align = if all_right { Align::Left } else { Align::Right };
                paras.push(Para::new(
                    CharStyle::default(),
                    ParaFmt {
                        image_id: id,
                        align: img_align,
                        table_id: tid,
                        table_row: 0,
                        table_col: img_col,
                        ..ParaFmt::default()
                    },
                ));
            }
            let txt_est = populate_table_cell_paras(
                &matched_lines,
                txt_x0,
                txt_x1,
                txt_x1 - txt_x0,
                tid,
                0,
                txt_col,
                &mut paras,
            );
            paras.sort_by_key(|p| p.fmt.table_col);

            let row_h = (band_top - band_bot).max(txt_est).max(img.h + 2.0);
            let cells = vec![
                TableCellSpec {
                    width_in: w0_in,
                    grid_span: 1,
                    shading: None,
                    borders: CellBorders::default(),
                    v_align: CellVAlign::Center,
                    pad_left_in: 0.02,
                    pad_right_in: 0.04,
                    pad_top_in: 0.01,
                    pad_bottom_in: 0.01,
                    ..TableCellSpec::default()
                },
                TableCellSpec {
                    width_in: w1_in,
                    grid_span: 1,
                    shading: None,
                    borders: CellBorders::default(),
                    v_align: CellVAlign::Center,
                    pad_left_in: 0.04,
                    pad_right_in: 0.02,
                    pad_top_in: 0.01,
                    pad_bottom_in: 0.01,
                    ..TableCellSpec::default()
                },
            ];
            page_tables.push(DetectedTable {
                top_y: band_top,
                bot_y: band_bot,
                left_x,
                right_x,
                est_h_pt: row_h + 2.0,
                doc_table: DocTable {
                    id: tid,
                    grid_cols_in,
                    rows: vec![TableRowSpec {
                        cells,
                        min_height_pt: (band_top - band_bot).clamp(12.0, 180.0),
                        grid_after: 0,
                    }],
                    indent_in: 0.0,
                },
                paras,
            });
        }
    }

    let mut remaining = Vec::new();
    for (idx, img) in std::mem::take(page_imgs).into_iter().enumerate() {
        if !used_img[idx] {
            remaining.push(img);
        }
    }
    *page_imgs = remaining;
}

/// Pairs vertically overlapping independent two-column text blocks (such as two-column academic papers,
/// side-by-side statement summary boxes, or split left/right form headers with staggered baselines)
/// into a 1-row, 2-column borderless `DetectedTable` so they render side-by-side instead of interleaving.
#[allow(clippy::too_many_arguments)]
fn pair_two_column_text_blocks(
    page_lines: &mut Vec<LineGroup>,
    page_tables: &mut Vec<DetectedTable>,
    geom: &PageGeometry,
    content_left: f32,
    content_right: f32,
    content_w: f32,
    page_bg: [u8; 3],
    next_table_id: &mut u32,
) {
    if page_lines.len() < 4 || content_w < 180.0 {
        return;
    }

    let candidate_fracs = [0.50f32, 0.475, 0.525, 0.45, 0.55, 0.40, 0.60, 0.34, 0.66];
    let gutter_half = 2.4f32;

    let mut consumed_line = vec![false; page_lines.len()];
    let mut s = 0usize;
    while s + 3 < page_lines.len() {
        if consumed_line[s] || find_banner_shading(&page_lines[s], geom, content_w, page_bg).is_some() {
            s += 1;
            continue;
        }

        let mut best_match: Option<(usize, f32)> = None; // (end_exclusive, split_x)
        for &frac in &candidate_fracs {
            let split_x = content_left + content_w * frac;
            let mut e = s;
            let mut left_lines = 0usize;
            let mut right_lines = 0usize;
            let mut left_only = 0usize;
            let mut right_only = 0usize;
            let mut l_top = f32::NEG_INFINITY;
            let mut l_bot = f32::INFINITY;
            let mut r_top = f32::NEG_INFINITY;
            let mut r_bot = f32::INFINITY;

            while e < page_lines.len() && !consumed_line[e] {
                let lg = &page_lines[e];
                if find_banner_shading(lg, geom, content_w, page_bg).is_some() {
                    break;
                }
                if e > s {
                    let prev_y = page_lines[e - 1].y;
                    if prev_y - lg.y > 42.0 {
                        break;
                    }
                }
                // No span may straddle the vertical gutter [split_x - gutter_half, split_x + gutter_half]
                let crosses_gutter = lg.spans.iter().any(|sp| {
                    !sp.text.trim().is_empty()
                        && sp.x < split_x + gutter_half
                        && (sp.x + sp.width) > split_x - gutter_half
                });
                if crosses_gutter {
                    break;
                }
                let has_l = lg
                    .spans
                    .iter()
                    .any(|sp| !sp.text.trim().is_empty() && sp.x + sp.width <= split_x - gutter_half);
                let has_r = lg
                    .spans
                    .iter()
                    .any(|sp| !sp.text.trim().is_empty() && sp.x >= split_x + gutter_half);
                if !has_l && !has_r {
                    break;
                }
                if has_l {
                    left_lines += 1;
                    l_top = l_top.max(lg.y + lg.size * 0.92);
                    l_bot = l_bot.min(lg.y - lg.size * 0.25);
                }
                if has_r {
                    right_lines += 1;
                    r_top = r_top.max(lg.y + lg.size * 0.92);
                    r_bot = r_bot.min(lg.y - lg.size * 0.25);
                }
                if has_l && !has_r {
                    left_only += 1;
                } else if has_r && !has_l {
                    right_only += 1;
                }
                e += 1;
            }

            let ov_y = (l_top.min(r_top) - l_bot.max(r_bot)).max(0.0);
            if e - s >= 4
                && left_lines >= 2
                && right_lines >= 2
                && (left_only + right_only) >= 1
                && (l_top - r_top).abs() <= 32.0
                && ov_y >= 12.0
                && best_match.is_none_or(|(best_e, _)| e > best_e)
            {
                best_match = Some((e, split_x));
            }
        }

        let Some((e, split_x)) = best_match else {
            s += 1;
            continue;
        };

        let mut left_spans = Vec::new();
        let mut right_spans = Vec::new();
        for (idx, flag) in consumed_line.iter_mut().enumerate().take(e).skip(s) {
            *flag = true;
            for sp in &page_lines[idx].spans {
                if sp.text.trim().is_empty() {
                    continue;
                }
                let mut adj = sp.clone();
                adj.color = ensure_legible_color(adj.color, None, adj.highlight, page_bg);
                if sp.x + sp.width <= split_x {
                    left_spans.push(adj);
                } else {
                    right_spans.push(adj);
                }
            }
        }

        let left_grouped = group_spans_into_lines(left_spans);
        let right_grouped = group_spans_into_lines(right_spans);
        if left_grouped.is_empty() || right_grouped.is_empty() {
            s = e;
            continue;
        }

        let l_min_x = left_grouped.iter().map(|l| l.min_x).fold(f32::INFINITY, f32::min);
        let l_max_x = left_grouped.iter().map(|l| l.max_x).fold(f32::NEG_INFINITY, f32::max);
        let r_min_x = right_grouped.iter().map(|l| l.min_x).fold(f32::INFINITY, f32::min);
        let r_max_x = right_grouped.iter().map(|l| l.max_x).fold(f32::NEG_INFINITY, f32::max);

        let left_x = if (l_min_x - content_left).abs() <= 24.0 {
            content_left.min(l_min_x)
        } else {
            l_min_x
        };
        let right_x = if (content_right - r_max_x).abs() <= 28.0 {
            content_right.max(r_max_x)
        } else {
            r_max_x
        };
        let eff_split = ((l_max_x + r_min_x) * 0.5).clamp(left_x + 36.0, right_x - 36.0);
        let w0_in = ((eff_split - left_x) / 72.0).max(0.4);
        let w1_in = ((right_x - eff_split) / 72.0).max(0.4);

        let tid = *next_table_id;
        *next_table_id += 1;
        let mut paras = Vec::new();
        let est0 = populate_table_cell_paras(
            &left_grouped,
            left_x,
            eff_split - 4.0,
            (eff_split - left_x - 4.0).max(30.0),
            tid,
            0,
            0,
            &mut paras,
        );
        let est1 = populate_table_cell_paras(
            &right_grouped,
            eff_split + 4.0,
            right_x,
            (right_x - eff_split - 4.0).max(30.0),
            tid,
            0,
            1,
            &mut paras,
        );

        let top_y = page_lines[s].y + page_lines[s].size * 0.92;
        let bot_y = page_lines[s..e]
            .iter()
            .map(|l| l.y - l.size * 0.25)
            .fold(f32::INFINITY, f32::min);
        let natural_span = (top_y - bot_y).max(12.0);
        let est_h = est0.max(est1).max(natural_span * 0.85);

        let cells = vec![
            TableCellSpec {
                width_in: w0_in,
                grid_span: 1,
                shading: None,
                borders: CellBorders::default(),
                v_align: CellVAlign::Top,
                pad_left_in: 0.0,
                pad_right_in: 0.08,
                pad_top_in: 0.01,
                pad_bottom_in: 0.01,
                ..TableCellSpec::default()
            },
            TableCellSpec {
                width_in: w1_in,
                grid_span: 1,
                shading: None,
                borders: CellBorders::default(),
                v_align: CellVAlign::Top,
                pad_left_in: 0.08,
                pad_right_in: 0.0,
                pad_top_in: 0.01,
                pad_bottom_in: 0.01,
                ..TableCellSpec::default()
            },
        ];

        page_tables.push(DetectedTable {
            top_y,
            bot_y,
            left_x,
            right_x,
            est_h_pt: est_h,
            doc_table: DocTable {
                id: tid,
                grid_cols_in: vec![w0_in, w1_in],
                rows: vec![TableRowSpec {
                    cells,
                    min_height_pt: 0.0,
                    grid_after: 0,
                }],
                indent_in: 0.0,
            },
            paras,
        });

        s = e;
    }

    if consumed_line.iter().any(|&c| c) {
        let mut kept = Vec::with_capacity(page_lines.len());
        for (idx, lg) in std::mem::take(page_lines).into_iter().enumerate() {
            if !consumed_line[idx] {
                kept.push(lg);
            }
        }
        *page_lines = kept;
    }
}

#[allow(clippy::too_many_arguments)]
fn build_line_items(
    lines: &[LineGroup],
    geom: &PageGeometry,
    content_left: f32,
    content_right: f32,
    content_w: f32,
    page_bg: [u8; 3],
    next_table_id: &mut u32,
    items_out: &mut Vec<PageItem>,
) {
    let mut prev_line: Option<&LineGroup> = None;
    let mut cur_para: Option<(Para, f32, f32, f32)> = None; // (para, top_y, bot_y, est_h)

    let mut i = 0usize;
    while i < lines.len() {
        let lg = &lines[i];
        let line_text = join_line_text(&lg.spans);
        if line_text.trim().is_empty() {
            i += 1;
            continue;
        }

        // Check if this line (and any consecutive multi-column lines directly below it) has wide column gaps
        // such as left+right headers ("Missing Reports        Leesville - Raleigh, NC"), resume role+date rows,
        // or dual signature blocks.
        let col_gap_thresh = (lg.size * 3.1).max(24.0);
        let chunks = split_line_into_chunks(lg, col_gap_thresh);
        if chunks.len() >= 2 && chunks.len() <= 5 {
            // Group consecutive compatible multi-column lines with matching chunk count into one borderless table
            let n_cols = chunks.len();
            let mut run_end = i + 1;
            let mut run_chunks = vec![chunks];
            while run_end < lines.len() {
                let nlg = &lines[run_end];
                let prev_lg = &lines[run_end - 1];
                if prev_lg.y - nlg.y > prev_lg.size.max(nlg.size) * 2.3 {
                    break;
                }
                let n_thresh = (nlg.size * 3.1).max(24.0);
                let n_ch = split_line_into_chunks(nlg, n_thresh);
                if n_ch.len() != n_cols {
                    break;
                }
                // Check column X alignment compatibility
                let compat = (0..n_cols).all(|c| {
                    let (a0, a1, _) = &run_chunks[0][c];
                    let (b0, b1, _) = &n_ch[c];
                    (a0 - b0).abs() <= 28.0 || (a1 - b1).abs() <= 28.0
                });
                if !compat {
                    break;
                }
                run_chunks.push(n_ch);
                run_end += 1;
            }

            // Guard against splitting a single isolated justified prose line with wide inter-word justification
            let is_justified_prose_line = run_end == i + 1
                && line_text.split_whitespace().count() >= 9
                && (lg.max_x - lg.min_x) >= content_w * 0.82
                && max_internal_gap(&lg.spans) < 42.0;

            if !is_justified_prose_line {
                if let Some((p, ty, by, eh)) = cur_para.take() {
                    items_out.push(PageItem {
                        top_y: ty,
                        bot_y: by,
                        est_h_pt: eh,
                        kind: PageItemKind::Para(p),
                    });
                }
                prev_line = None;

                let tbl = build_borderless_multicol_table(
                    &lines[i..run_end],
                    &run_chunks,
                    geom,
                    content_left,
                    content_right,
                    content_w,
                    page_bg,
                    next_table_id,
                );
                items_out.push(PageItem {
                    top_y: tbl.top_y,
                    bot_y: tbl.bot_y,
                    est_h_pt: tbl.est_h_pt,
                    kind: PageItemKind::Table(tbl),
                });
                i = run_end;
                continue;
            }
        }

        let align = infer_alignment(lg, content_left, content_right, content_w);
        let (list_kind, checked, strip_chars) = detect_list_prefix(&line_text);
        let banner_shading = find_banner_shading(lg, geom, content_w, page_bg);

        let merge = prev_line.is_some_and(|p| {
            cur_para
                .as_ref()
                .is_some_and(|(cp, _, _, _)| cp.fmt.align == align && cp.fmt.shading == banner_shading)
                && should_merge_lines(p, lg, content_left, content_w, list_kind)
        });

        let adjusted_spans: Vec<TextSpan> = lg
            .spans
            .iter()
            .map(|sp| {
                let mut s = sp.clone();
                if banner_shading.is_some() && s.highlight == banner_shading {
                    s.highlight = None;
                }
                s.color = ensure_legible_color(s.color, banner_shading, s.highlight, page_bg);
                s
            })
            .collect();

        let line_top = lg.y + lg.size * 0.90;
        let line_bot = lg.y - lg.size * 0.22;
        let line_est_h = lg.size * 1.24;

        if !merge {
            if let Some((p, ty, by, eh)) = cur_para.take() {
                items_out.push(PageItem {
                    top_y: ty,
                    bot_y: by,
                    est_h_pt: eh,
                    kind: PageItemKind::Para(p),
                });
            }
            let first_style = span_to_char_style(&adjusted_spans[0]);
            let indent = if list_kind == ListKind::None && lg.min_x > content_left + 18.0 && align == Align::Left {
                ((lg.min_x - content_left) / 36.0).round().clamp(0.0, 6.0) as u8
            } else {
                0
            };
            let fmt = ParaFmt {
                align,
                indent,
                list: list_kind,
                checked,
                shading: banner_shading,
                ..ParaFmt::default()
            };
            cur_para = Some((Para::new(first_style, fmt), line_top, line_bot, line_est_h));
        } else if let Some((ref mut p, _, ref mut by, ref mut eh)) = cur_para {
            if let Some(prev) = prev_line {
                let ls = ((prev.y - lg.y) / (lg.size * 1.20)).clamp(0.95, 1.80);
                p.fmt.line_spacing = ls;
                *eh += lg.size * 1.20 * ls;
            } else {
                *eh += line_est_h;
            }
            *by = (*by).min(line_bot);
            if !p.text.is_empty() && !p.text.ends_with(&[' ', '-']) {
                let st = p.end_style;
                p.text.push(' ');
                p.styles.push(st);
            }
        }

        if let Some((ref mut p, _, _, _)) = cur_para {
            append_line_spans(p, &adjusted_spans, strip_chars, false, content_w);
        }
        prev_line = Some(lg);
        i += 1;
    }

    if let Some((p, ty, by, eh)) = cur_para.take() {
        items_out.push(PageItem {
            top_y: ty,
            bot_y: by,
            est_h_pt: eh,
            kind: PageItemKind::Para(p),
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn build_borderless_multicol_table(
    run_lines: &[LineGroup],
    run_chunks: &[Vec<(f32, f32, Vec<TextSpan>)>],
    geom: &PageGeometry,
    content_left: f32,
    content_right: f32,
    content_w: f32,
    page_bg: [u8; 3],
    next_table_id: &mut u32,
) -> DetectedTable {
    let n_cols = run_chunks[0].len();
    let mut col_min = vec![f32::INFINITY; n_cols];
    let mut col_max = vec![f32::NEG_INFINITY; n_cols];
    for chs in run_chunks {
        for c in 0..n_cols {
            col_min[c] = col_min[c].min(chs[c].0);
            col_max[c] = col_max[c].max(chs[c].1);
        }
    }

    let left_x = if (col_min[0] - content_left).abs() <= 24.0 {
        content_left.min(col_min[0])
    } else {
        col_min[0]
    };
    let right_x = if (content_right - col_max[n_cols - 1]).abs() <= 28.0 {
        content_right.max(col_max[n_cols - 1])
    } else {
        col_max[n_cols - 1]
    };

    let mut bounds = Vec::with_capacity(n_cols + 1);
    bounds.push(left_x);
    for c in 0..n_cols - 1 {
        let mid = (col_max[c] + col_min[c + 1]) * 0.5;
        bounds.push(mid.max(bounds[c] + 14.0));
    }
    bounds.push(right_x.max(bounds[n_cols - 1] + 14.0));

    let grid_cols_in: Vec<f32> = bounds.windows(2).map(|w| ((w[1] - w[0]) / 72.0).max(0.20)).collect();
    let tid = *next_table_id;
    *next_table_id += 1;

    let mut rows = Vec::with_capacity(run_lines.len());
    let mut paras = Vec::new();
    let mut est_h = 0.0f32;

    for (r_idx, (lg, chs)) in run_lines.iter().zip(run_chunks.iter()).enumerate() {
        let row_shading = find_banner_shading(lg, geom, content_w, page_bg);
        let mut cells = Vec::with_capacity(n_cols);
        let mut row_est = 0.0f32;

        for c in 0..n_cols {
            let bx0 = bounds[c];
            let bx1 = bounds[c + 1];
            let (cx0, cx1, ref cspans) = chs[c];
            let adjusted: Vec<TextSpan> = cspans
                .iter()
                .map(|sp| {
                    let mut s = sp.clone();
                    if row_shading.is_some() && s.highlight == row_shading {
                        s.highlight = None;
                    }
                    s.color = ensure_legible_color(s.color, row_shading, s.highlight, page_bg);
                    s
                })
                .collect();

            let align = if c + 1 == n_cols && (right_x - cx1) <= 22.0 && (cx0 - bx0) >= 10.0 {
                Align::Right
            } else if c > 0 && ((cx0 + cx1) * 0.5 - (bx0 + bx1) * 0.5).abs() <= 10.0 && (cx0 - bx0) >= 12.0 {
                Align::Center
            } else {
                Align::Left
            };

            let first_st = adjusted.first().map_or_else(CharStyle::default, span_to_char_style);
            let mut p = Para::new(
                first_st,
                ParaFmt {
                    align,
                    table_id: tid,
                    table_row: r_idx as u16,
                    table_col: c as u16,
                    ..ParaFmt::default()
                },
            );
            append_line_spans(&mut p, &adjusted, 0, false, bx1 - bx0);
            paras.push(p);
            row_est = row_est.max(lg.size * 1.26);

            cells.push(TableCellSpec {
                width_in: grid_cols_in[c],
                grid_span: 1,
                shading: row_shading,
                borders: CellBorders::default(),
                v_align: CellVAlign::Center,
                pad_left_in: if c == 0 { 0.0 } else { 0.03 },
                pad_right_in: if c + 1 == n_cols { 0.0 } else { 0.03 },
                pad_top_in: 0.01,
                pad_bottom_in: 0.01,
                ..TableCellSpec::default()
            });
        }

        let min_h = if r_idx + 1 < run_lines.len() {
            ((lg.y - run_lines[r_idx + 1].y) * 0.90).clamp(lg.size * 1.15, 42.0)
        } else {
            lg.size * 1.22
        };
        est_h += row_est.max(min_h);
        rows.push(TableRowSpec {
            cells,
            min_height_pt: min_h,
            grid_after: 0,
        });
    }

    let top_y = run_lines[0].y + run_lines[0].size * 0.92;
    let last_lg = &run_lines[run_lines.len() - 1];
    let bot_y = last_lg.y - last_lg.size * 0.25;
    let indent_pt = (left_x - content_left).max(0.0);
    let indent_in = if indent_pt > 5.0 {
        ((indent_pt / 72.0) * 20.0).round() / 20.0
    } else {
        0.0
    };
    DetectedTable {
        top_y,
        bot_y,
        left_x,
        right_x,
        est_h_pt: est_h,
        doc_table: DocTable {
            id: tid,
            grid_cols_in,
            rows,
            indent_in,
        },
        paras,
    }
}

fn find_banner_shading(lg: &LineGroup, geom: &PageGeometry, content_w: f32, page_bg: [u8; 3]) -> Option<[u8; 3]> {
    let mid_y = lg.y + lg.size * 0.35;
    for r in &geom.fill_rects {
        if r.w >= geom.pw * 0.88 && r.h >= geom.ph * 0.88 {
            continue;
        }
        let eff_c = r.blended_color(page_bg);
        if eff_c == page_bg {
            continue;
        }
        if r.w >= content_w * 0.48
            && r.h >= lg.size * 0.8
            && r.h <= lg.size * 3.8
            && mid_y >= r.y - 2.0
            && mid_y <= r.y + r.h + 2.0
        {
            return Some(eff_c);
        }
    }
    None
}

fn ensure_legible_color(
    color: [u8; 3],
    shading: Option<[u8; 3]>,
    highlight: Option<[u8; 3]>,
    page_bg: [u8; 3],
) -> [u8; 3] {
    let bg = highlight.or(shading).unwrap_or(page_bg);
    let text_very_light = color[0] >= 235 && color[1] >= 235 && color[2] >= 235;
    let bg_light = bg[0] >= 215 && bg[1] >= 215 && bg[2] >= 215;
    if text_very_light && bg_light {
        [30, 30, 30]
    } else {
        color
    }
}

fn max_internal_gap(spans: &[TextSpan]) -> f32 {
    let mut max_g = 0.0f32;
    let mut prev_end: Option<f32> = None;
    for sp in spans {
        if sp.text.trim().is_empty() {
            continue;
        }
        if let Some(pe) = prev_end {
            max_g = max_g.max(sp.x - pe);
        }
        prev_end = Some(sp.x + sp.width);
    }
    max_g
}

fn should_merge_lines(
    prev: &LineGroup,
    curr: &LineGroup,
    col_left: f32,
    col_w: f32,
    curr_list: ListKind,
) -> bool {
    if curr_list != ListKind::None {
        return false;
    }
    let dy = prev.y - curr.y;
    let max_sz = curr.size.max(prev.size);
    if dy <= 0.0 || dy > max_sz * 1.45 {
        return false;
    }
    if (prev.size - curr.size).abs() > 1.5 {
        return false;
    }
    if curr.min_x - prev.min_x > 12.0 || prev.min_x - curr.min_x > 28.0 {
        return false;
    }
    let curr_txt = join_line_text(&curr.spans);
    let ct = curr_txt.trim_start();
    if ct.starts_with('(') && ct.find(") ").is_some_and(|idx| (2..=4).contains(&idx)) {
        return false;
    }
    if max_internal_gap(&prev.spans) > prev.size * 2.3 || max_internal_gap(&curr.spans) > curr.size * 2.3 {
        return false;
    }
    let prev_reach = prev.max_x - col_left;
    prev_reach >= col_w * 0.66
}

fn join_line_text(spans: &[TextSpan]) -> String {
    let mut out = String::new();
    let mut prev_end: Option<f32> = None;
    for sp in spans {
        if let Some(pe) = prev_end {
            let gap = sp.x - pe;
            if gap > (sp.size * 0.15).max(0.95) && !out.ends_with(' ') && !sp.text.starts_with(' ') {
                out.push(' ');
            }
        }
        out.push_str(&sp.text);
        prev_end = Some(sp.x + sp.width);
    }
    out
}

fn append_line_spans(
    para: &mut Para,
    spans: &[TextSpan],
    mut skip_chars: usize,
    preserve_wide_gaps: bool,
    avail_w_pt: f32,
) {
    let mut prev_end: Option<f32> = None;
    let total_text_w: f32 = spans.iter().map(|s| s.width).sum();
    for sp in spans {
        let mut text = sp.text.as_str();
        let st = span_to_char_style(sp);
        if let Some(pe) = prev_end {
            let gap = sp.x - pe;
            if gap > (sp.size * 0.15).max(0.95) && !para.text.ends_with(&[' ']) && !text.starts_with(' ') {
                let n_spaces = if preserve_wide_gaps && gap > (sp.size * 2.5).max(18.0) && skip_chars == 0 {
                    let room_pt = (avail_w_pt * 0.92 - total_text_w).max(sp.size);
                    let capped_gap = gap.min(room_pt);
                    ((capped_gap / (sp.size * 0.34)).round() as usize).clamp(2, 36)
                } else {
                    1
                };
                for _ in 0..n_spaces {
                    if skip_chars > 0 {
                        skip_chars -= 1;
                    } else {
                        para.text.push(' ');
                        para.styles.push(st);
                    }
                }
            }
        }
        if skip_chars > 0 {
            let char_count = text.chars().count();
            if skip_chars >= char_count {
                skip_chars -= char_count;
                prev_end = Some(sp.x + sp.width);
                continue;
            }
            let byte_idx = text.char_indices().nth(skip_chars).map_or(text.len(), |(i, _)| i);
            text = &text[byte_idx..];
            skip_chars = 0;
        }
        for ch in text.chars() {
            if ch == '\r' || ch == '\n' {
                continue;
            }
            para.text.push(ch);
            para.styles.push(st);
        }
        para.end_style = st;
        prev_end = Some(sp.x + sp.width);
    }
}

fn span_to_char_style(sp: &TextSpan) -> CharStyle {
    CharStyle {
        font: sp.font,
        size: (sp.size * 2.0).round() / 2.0,
        bold: sp.bold,
        italic: sp.italic,
        underline: sp.underline,
        strike: sp.strike,
        color: sp.color,
        highlight: sp.highlight,
        ..CharStyle::default()
    }
}

fn infer_alignment(lg: &LineGroup, left: f32, right: f32, content_w: f32) -> Align {
    let line_w = (lg.max_x - lg.min_x).max(0.0);
    if line_w > content_w * 0.85 || max_internal_gap(&lg.spans) > lg.size * 2.3 {
        return Align::Left;
    }
    let left_gap = (lg.min_x - left).abs();
    let right_gap = (right - lg.max_x).abs();
    let mid_diff = ((lg.min_x + lg.max_x) * 0.5 - (left + right) * 0.5).abs();
    if mid_diff < 16.0 && left_gap > 20.0 && right_gap > 20.0 {
        Align::Center
    } else if right_gap < 16.0 && left_gap > 36.0 {
        Align::Right
    } else {
        Align::Left
    }
}

fn detect_list_prefix(line: &str) -> (ListKind, bool, usize) {
    let trimmed = line.trim_start();
    let leading = line.chars().count() - trimmed.chars().count();
    if let Some(rest) = trimmed
        .strip_prefix("• ")
        .or_else(|| trimmed.strip_prefix("- "))
        .or_else(|| trimmed.strip_prefix("* "))
    {
        let _ = rest;
        return (ListKind::Bullet, false, leading + 2);
    }
    if trimmed.starts_with("☑ ") || trimmed.starts_with("✓ ") {
        return (ListKind::Checklist, true, leading + 2);
    }
    if trimmed.starts_with("☐ ") {
        return (ListKind::Checklist, false, leading + 2);
    }
    let mut digits = 0usize;
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
        } else {
            break;
        }
    }
    if digits > 0 && digits <= 3 {
        let after = &trimmed[digits..];
        if after.starts_with(". ") || after.starts_with(") ") {
            return (ListKind::Number, false, leading + digits + 2);
        }
    }
    (ListKind::None, false, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Pos;
    use crate::fonts::FontBook;
    use eframe::egui;

    #[test]
    fn pdf_roundtrip_preserves_text_and_formatting() {
        let ctx = egui::Context::default();
        let book = FontBook::default();
        let mut doc = Document::default();
        doc.info.title = "Quarterly Report".into();
        doc.info.author = "Scribe Pro".into();
        doc.insert_text(Pos::new(0, 0), "Hello ", CharStyle::default());
        doc.insert_text(
            Pos::new(0, 6),
            "PDF World",
            CharStyle { bold: true, color: [180, 30, 30], ..Default::default() },
        );

        let mut bytes = Vec::new();
        let mut out = ctx.run_ui(Default::default(), |_ui| {
            bytes = crate::pdf::export(&ctx, &doc, &book);
        });
        out.textures_delta.clear();

        let imported = read(&bytes).expect("import exported PDF");
        assert_eq!(imported.info.title, "Quarterly Report");
        assert_eq!(imported.info.author, "Scribe Pro");
        let plain = imported.plain_text();
        assert!(plain.contains("Hello"), "got: {plain:?}");
        assert!(plain.contains("PDF World"), "got: {plain:?}");
        assert!(imported.images.is_empty(), "WYSIWYG overlay /Im1 should not be imported as a user image");
    }

    #[test]
    fn pdf_import_rejects_non_pdf_bytes() {
        let err = read(b"not a pdf at all").expect_err("should reject non-PDF");
        assert!(err.contains("PDF"));
    }

    #[test]
    fn pdf_import_parses_tounicode_cmap() {
        let cmap = b"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def % dict header
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
2 beginbfchar
<0003> <0048>
<0004> <0069>
endbfchar
1 beginbfrange
<0010> <0012> <0041>
endbfrange
endcmap";
        let mut map = HashMap::new();
        parse_to_unicode_cmap(cmap, &mut map);
        assert_eq!(map.get(&3).map(String::as_str), Some("H"));
        assert_eq!(map.get(&4).map(String::as_str), Some("i"));
        assert_eq!(map.get(&0x10).map(String::as_str), Some("A"));
        assert_eq!(map.get(&0x12).map(String::as_str), Some("C"));
    }

    fn build_minimal_pdf(objects: &[(u32, &str)]) -> Vec<u8> {
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0usize; objects.len() + 1];
        for &(id, body) in objects {
            offsets[id as usize] = out.len();
            out.extend_from_slice(format!("{id} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        let xref_pos = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for off in offsets.iter().skip(1) {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    #[test]
    fn pdf_import_extracts_form_xobjects_and_multipage_breaks() {
        let form1_stream = "BT /F1 12 Tf 72 720 Td (First Page Inside Form XObject) Tj ET";
        let form2_stream = "BT /F1 12 Tf 72 720 Td (Second Page Inside Form XObject) Tj ET";
        let c1 = "q /Fm1 Do Q";
        let c2 = "q /Fm2 Do Q";
        let pdf = build_minimal_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /XObject << /Fm1 5 0 R >> >> /Contents 6 0 R >>",
            ),
            (
                4,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /XObject << /Fm2 7 0 R >> >> /Contents 8 0 R >>",
            ),
            (
                5,
                &format!(
                    "<< /Type /XObject /Subtype /Form /BBox [0 0 612 792] /Resources << /Font << /F1 9 0 R >> >> /Length {} >>\nstream\n{form1_stream}\nendstream",
                    form1_stream.len()
                ),
            ),
            (6, &format!("<< /Length {} >>\nstream\n{c1}\nendstream", c1.len())),
            (
                7,
                &format!(
                    "<< /Type /XObject /Subtype /Form /BBox [0 0 612 792] /Resources << /Font << /F1 9 0 R >> >> /Length {} >>\nstream\n{form2_stream}\nendstream",
                    form2_stream.len()
                ),
            ),
            (8, &format!("<< /Length {} >>\nstream\n{c2}\nendstream", c2.len())),
            (9, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        ]);

        let imported = read(&pdf).expect("import Form XObject PDF");
        let plain = imported.plain_text();
        assert!(plain.contains("First Page Inside Form XObject"), "got: {plain:?}");
        assert!(plain.contains("Second Page Inside Form XObject"), "got: {plain:?}");
        assert!(
            imported.paras.iter().skip(1).any(|p| p.fmt.page_break_before),
            "second page should set page_break_before"
        );
    }

    #[test]
    fn pdf_import_reconstructs_bordered_table_and_shaded_banner() {
        let stream = "\
0.15 0.45 0.50 rg 72 700 468 24 re f \
0 0 0 RG \
72 700 m 540 700 l S \
72 676 m 540 676 l S \
72 652 m 540 652 l S \
72 652 m 72 700 l S \
220 652 m 220 700 l S \
540 652 m 540 700 l S \
BT 1 1 1 rg /F1 12 Tf 80 708 Td (Section Banner) Tj ET \
BT 0 0 0 rg /F1 11 Tf 80 684 Td (7:00) Tj 228 684 Td (Opening Song) Tj ET \
BT 0 0 0 rg /F1 11 Tf 80 660 Td (7:05) Tj 228 660 Td (Comments) Tj ET";
        let pdf = build_minimal_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            ),
            (4, &format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len())),
            (5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        ]);

        let imported = read(&pdf).expect("import bordered table PDF");
        assert_eq!(imported.tables.len(), 1, "expected 1 reconstructed table");
        assert_eq!(imported.tables[0].rows.len(), 3, "banner + 2 schedule rows");
        assert_eq!(imported.tables[0].rows[0].cells[0].grid_span, 2, "banner spans 2 cols");
        assert!(imported.tables[0].rows[0].cells[0].shading.is_some(), "banner shading preserved");
        let plain = imported.plain_text();
        assert!(plain.contains("Section Banner"), "got: {plain:?}");
        assert!(plain.contains("Opening Song"), "got: {plain:?}");
    }

    #[test]
    fn pdf_import_rasterizes_vector_chart_and_detects_hrule_table() {
        let stream = "\
0.2 0.2 0.2 RG 1.2 w 60 560 m 60 660 l S 60 560 m 240 560 l S \
0.1 0.4 0.8 rg 80 561 24 65 re f 0.8 0.3 0.2 rg 120 561 24 85 re f 0.2 0.6 0.4 rg 160 561 24 45 re f \
BT 0 0 0 rg /F1 9 Tf 82 630 Td (65%) Tj 122 650 Td (85%) Tj ET \
0 0 0 RG 1 w 60 480 m 540 480 l S 60 456 m 540 456 l S 60 432 m 540 432 l S \
BT 0 0 0 rg /F1 10 Tf 68 464 Td (Item) Tj 252 0 Td (Qty) Tj 160 0 Td (Total) Tj ET \
BT 0 0 0 rg /F1 10 Tf 68 440 Td (CloudCompute) Tj 256 0 Td (3) Tj 151 0 Td ($450.00) Tj ET";
        let pdf = build_minimal_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            ),
            (4, &format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len())),
            (5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        ]);

        let imported = read(&pdf).expect("import vector chart + hrule table PDF");
        assert!(!imported.images.is_empty(), "vector bar chart should be rasterized into seamless DocImage");
        assert!(imported.images[0].seamless);
        assert_eq!(imported.tables.len(), 1, "horizontal-rule invoice table should be reconstructed");
        assert_eq!(imported.tables[0].grid_cols_in.len(), 3);
        assert!(imported.plain_text().contains("CloudCompute"));
    }

    #[test]
    fn pdf_import_repairs_drifted_xref_and_pairs_staggered_two_columns() {
        let stream = "\
BT 0 0 0 rg /F1 11 Tf 60 710 Td (Account Summary) Tj ET \
BT 0 0 0 rg /F1 11 Tf 340 706 Td (Payment Information) Tj ET \
BT 0 0 0 rg /F1 10 Tf 60 692 Td (Previous Balance: $410.00) Tj ET \
BT 0 0 0 rg /F1 10 Tf 340 688 Td (New Balance: $525.00) Tj ET \
BT 0 0 0 rg /F1 10 Tf 60 676 Td (Purchases: $115.00) Tj ET \
BT 0 0 0 rg /F1 10 Tf 340 671 Td (Minimum Due: $25.00) Tj ET";
        let mut pdf = build_minimal_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            ),
            (4, &format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len())),
            (5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        ]);
        // Inject extra bytes right before `3 0 obj` so xref offsets for objects 3..5 drift, mimicking broken tax/form writers
        if let Some(pos) = pdf.windows(7).position(|w| w == b"3 0 obj") {
            pdf.splice(pos..pos, *b"% drifted comment\r\n");
        }

        let imported = read(&pdf).expect("self-heal drifted xref offsets");
        assert_eq!(imported.tables.len(), 1, "staggered left/right zones should pair into a 2-col table");
        assert_eq!(imported.tables[0].grid_cols_in.len(), 2);
        let plain = imported.plain_text();
        assert!(plain.contains("Account Summary") && plain.contains("Minimum Due: $25.00"), "got: {plain:?}");
    }

    #[test]
    fn pdf_import_extracts_repeating_sparse_page_footers_at_exact_bottom() {
        let p1 = "BT /F1 14 Tf 1 0 0 1 50 720 Tm (October 2026 Missing Reports) Tj /F2 9 Tf 1 0 0 1 515 9 Tm (Printed 2026/11/12) Tj ET";
        let p2 = "BT /F1 12 Tf 1 0 0 1 50 710 Tm (Group 02 Summary) Tj /F2 9 Tf 1 0 0 1 515 9 Tm (Printed 2026/11/12) Tj ET";
        let pdf = build_minimal_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 7 0 R /F2 8 0 R >> >> /Contents 5 0 R >>",
            ),
            (
                4,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 7 0 R /F2 8 0 R >> >> /Contents 6 0 R >>",
            ),
            (5, &format!("<< /Length {} >>\nstream\n{p1}\nendstream", p1.len())),
            (6, &format!("<< /Length {} >>\nstream\n{p2}\nendstream", p2.len())),
            (7, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>"),
            (8, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Oblique >>"),
        ]);

        let imported = read(&pdf).expect("import sparse multi-page PDF with repeating bottom footer");
        assert_eq!(imported.footer, "Printed 2026/11/12");
        assert_eq!(imported.footer_align, Align::Right);
        assert!(imported.footer_italic, "Helvetica-Oblique footer should be marked italic");
        assert_eq!(imported.page_footers.len(), 2);
        assert!(
            !imported.plain_text().contains("Printed 2026/11/12"),
            "footer must not be duplicated inside body paragraphs where it would float up on sparse pages"
        );
        for pf in &imported.page_footers {
            assert_eq!(pf.segments.len(), 1);
            assert_eq!(pf.segments[0].align, Align::Right);
            assert!((pf.segments[0].bottom_pt - 9.0).abs() < 1.0);
        }
    }

    #[test]
    fn pdf_import_preserves_colored_footer_runs_split_segments_and_divider_rule() {
        let s1 = "\
0.75 0.75 0.75 RG 0.8 w 72 54 m 540 54 l S \
BT 0.4 0.4 0.4 rg /F1 10 Tf 1 0 0 1 162 40 Tm (101 Townsend Street | ) Tj \
1.0 0.53 0.38 rg 1 0 0 1 268 40 Tm (cloudflare.com) Tj \
0.4 0.4 0.4 rg 1 0 0 1 342 40 Tm ( | +1 650 319 8930) Tj ET \
BT 0 0 0 rg /F1 12 Tf 1 0 0 1 72 700 Tm (Domain Ownership Letter) Tj ET";
        let pdf1 = build_minimal_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            ),
            (4, &format!("<< /Length {} >>\nstream\n{s1}\nendstream", s1.len())),
            (5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        ]);
        let doc1 = read(&pdf1).expect("import colored centered footer with divider rule");
        assert_eq!(doc1.page_footers.len(), 1);
        let pf1 = &doc1.page_footers[0];
        assert!(pf1.rule.is_some(), "divider rule right above footer should attach to PageFooter");
        assert_eq!(pf1.segments.len(), 1);
        assert_eq!(pf1.segments[0].align, Align::Center);
        assert!(
            pf1.segments[0].runs.len() >= 3 && pf1.segments[0].runs.iter().any(|r| r.style.color[0] > 220 && r.text.contains("cloudflare.com")),
            "orange brand color run should be preserved in footer: {:?}",
            pf1.segments[0].runs
        );

        let s2 = "\
BT 0 0 0 rg /F1 12 Tf 1 0 0 1 54 700 Tm (Statement Summary) Tj \
/F1 8 Tf 1 0 0 1 16 12 Tm (STMT_02232026) Tj 1 0 0 1 536 12 Tm (Page 1 of 1) Tj ET";
        let pdf2 = build_minimal_pdf(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            ),
            (4, &format!("<< /Length {} >>\nstream\n{s2}\nendstream", s2.len())),
            (5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        ]);
        let doc2 = read(&pdf2).expect("import split left+right footer");
        assert_eq!(doc2.page_footers.len(), 1);
        let pf2 = &doc2.page_footers[0];
        assert_eq!(pf2.segments.len(), 2, "left batch ID and right Page 1 of 1 should split into 2 segments");
        assert_eq!(pf2.segments[0].align, Align::Left);
        assert_eq!(pf2.segments[1].align, Align::Right);
    }
}

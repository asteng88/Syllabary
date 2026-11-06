//! Document model: paragraphs of styled characters, page setup and editing operations.

use serde::{Deserialize, Serialize};

/// Fonts offered in the font list. Index 0 is the default (see MECHANICS.md).
pub const FONT_NAMES: &[&str] = &[
    "Times New Roman",
    "Arial",
    "Calibri",
    "Cambria",
    "Georgia",
    "Garamond",
    "Bookman Old Style",
    "Palatino Linotype",
    "Segoe UI",
    "Tahoma",
    "Trebuchet MS",
    "Verdana",
    "Courier New",
    "Consolas",
    // bundled free fonts (SIL Open Font License), shipped in the fonts/ folder
    "Lora",
    "Merriweather",
    "Libre Baskerville",
    "EB Garamond",
    "Playfair Display",
    "Source Serif 4",
    "Open Sans",
    "Roboto",
    "Lato",
    "Inter",
    "Montserrat",
    "Nunito",
    "Work Sans",
    "JetBrains Mono",
    "Source Code Pro",
    "Caveat",
    "Dancing Script",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontCat {
    Serif,
    Sans,
    Mono,
    Script,
}

impl FontCat {
    pub const ALL: [FontCat; 4] = [FontCat::Serif, FontCat::Sans, FontCat::Mono, FontCat::Script];
    pub fn label(self) -> &'static str {
        match self {
            FontCat::Serif => "Serif",
            FontCat::Sans => "Sans Serif",
            FontCat::Mono => "Monospace",
            FontCat::Script => "Handwriting",
        }
    }
}

pub fn font_category(i: u8) -> FontCat {
    match i {
        0 | 3 | 4 | 5 | 6 | 7 | 14..=19 => FontCat::Serif,
        12 | 13 | 27 | 28 => FontCat::Mono,
        29 | 30 => FontCat::Script,
        _ => FontCat::Sans,
    }
}

/// Folder name under fonts/ for a bundled font, None for the computer's own fonts.
pub fn bundled_folder(i: u8) -> Option<&'static str> {
    const DIRS: [&str; 17] = [
        "lora",
        "merriweather",
        "libre-baskerville",
        "eb-garamond",
        "playfair-display",
        "source-serif-4",
        "open-sans",
        "roboto",
        "lato",
        "inter",
        "montserrat",
        "nunito",
        "work-sans",
        "jetbrains-mono",
        "source-code-pro",
        "caveat",
        "dancing-script",
    ];
    (i as usize).checked_sub(14).and_then(|k| DIRS.get(k).copied())
}

pub const DEFAULT_FONT: u8 = 0; // Times New Roman
pub const DEFAULT_SIZE: f32 = 12.0; // points
pub const MIN_SIZE: f32 = 6.0;
pub const MAX_SIZE: f32 = 96.0;
pub const SIZE_STEP: f32 = 2.0;
pub const SIZE_LIST: &[f32] = &[
    8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 48.0, 72.0,
];

pub fn font_index(name: &str) -> u8 {
    if let Some(i) = FONT_NAMES.iter().position(|f| f.eq_ignore_ascii_case(name)) {
        return i as u8;
    }
    // Fallback rule from the original (function 0x58ac4f): fixed pitch → Courier New,
    // sans-serif → Arial, everything else → Times New Roman.
    let l = name.to_ascii_lowercase();
    if l.contains("mono") || l.contains("courier") || l.contains("console") || l.contains("code") {
        12
    } else if l.contains("sans") || l.contains("helvet") || l.contains("swiss") {
        1
    } else {
        0
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct CharStyle {
    pub font: u8,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: [u8; 3],
    pub highlight: Option<[u8; 3]>,
    /// 1 = superscript, -1 = subscript
    pub script: i8,
}

impl Default for CharStyle {
    fn default() -> Self {
        Self {
            font: DEFAULT_FONT,
            size: DEFAULT_SIZE,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            color: [0, 0, 0],
            highlight: None,
            script: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ListKind {
    None,
    Bullet,
    Number,
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct ParaFmt {
    pub align: Align,
    pub list: ListKind,
    pub indent: u8,
    pub space_before: f32,
    pub space_after: f32,
    pub line_spacing: f32,
    pub style: usize,
    pub page_break_before: bool,
}

impl Default for ParaFmt {
    fn default() -> Self {
        Self {
            align: Align::Left,
            list: ListKind::None,
            indent: 0,
            space_before: 0.0,
            space_after: 0.0,
            line_spacing: 1.0,
            style: 0,
            page_break_before: false,
        }
    }
}

/// Named paragraph styles ("Default Text" is the original's default).
pub struct NamedStyle {
    pub name: &'static str,
    pub chr: CharStyle,
    pub fmt: ParaFmt,
}

pub fn named_styles() -> Vec<NamedStyle> {
    let base = CharStyle::default();
    let pf = ParaFmt::default();
    let arial = |size: f32, bold: bool| CharStyle { font: 1, size, bold, ..base };
    vec![
        NamedStyle { name: "Default Text", chr: base, fmt: pf },
        NamedStyle { name: "Body Single", chr: base, fmt: ParaFmt { space_after: 6.0, ..pf } },
        NamedStyle {
            name: "Title",
            chr: arial(26.0, true),
            fmt: ParaFmt { align: Align::Center, space_after: 12.0, ..pf },
        },
        NamedStyle {
            name: "Subtitle",
            chr: CharStyle { color: [90, 90, 100], ..arial(15.0, false) },
            fmt: ParaFmt { align: Align::Center, space_after: 12.0, ..pf },
        },
        NamedStyle {
            name: "Heading 1",
            chr: arial(18.0, true),
            fmt: ParaFmt { space_before: 12.0, space_after: 6.0, ..pf },
        },
        NamedStyle {
            name: "Heading 2",
            chr: arial(14.0, true),
            fmt: ParaFmt { space_before: 10.0, space_after: 4.0, ..pf },
        },
        NamedStyle {
            name: "Heading 3",
            chr: CharStyle { italic: true, ..arial(12.0, true) },
            fmt: ParaFmt { space_before: 8.0, space_after: 2.0, ..pf },
        },
        NamedStyle {
            name: "Quote",
            chr: CharStyle { italic: true, color: [70, 70, 80], ..base },
            fmt: ParaFmt { indent: 1, space_before: 6.0, space_after: 6.0, ..pf },
        },
        NamedStyle { name: "Bullet 1", chr: base, fmt: ParaFmt { list: ListKind::Bullet, ..pf } },
        NamedStyle { name: "Number List", chr: base, fmt: ParaFmt { list: ListKind::Number, ..pf } },
        NamedStyle {
            name: "Code",
            chr: CharStyle { font: 12, size: 10.0, ..base },
            fmt: ParaFmt { space_after: 0.0, ..pf },
        },
    ]
}

#[derive(Clone, Debug, PartialEq)]
pub struct Para {
    pub text: Vec<char>,
    pub styles: Vec<CharStyle>,
    pub fmt: ParaFmt,
    /// Style used when the paragraph is empty or the caret sits at its start.
    pub end_style: CharStyle,
}

impl Para {
    pub fn new(style: CharStyle, fmt: ParaFmt) -> Self {
        Self { text: Vec::new(), styles: Vec::new(), fmt, end_style: style }
    }
    pub fn len(&self) -> usize {
        self.text.len()
    }
    pub fn string(&self) -> String {
        self.text.iter().collect()
    }
    /// Style for text typed at `c`.
    pub fn style_at(&self, c: usize) -> CharStyle {
        if self.text.is_empty() {
            self.end_style
        } else if c == 0 {
            self.styles[0]
        } else {
            self.styles[(c - 1).min(self.styles.len() - 1)]
        }
    }
    /// Runs of equally styled text.
    pub fn runs(&self) -> Vec<(usize, usize, CharStyle)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.text.len() {
            let s = self.styles[i];
            let mut j = i + 1;
            while j < self.text.len() && self.styles[j] == s {
                j += 1;
            }
            out.push((i, j, s));
            i = j;
        }
        out
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct PageSetup {
    pub width: f32,
    pub height: f32,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub margin_right: f32,
    pub background: [u8; 3],
    pub page_numbers: bool,
}

impl Default for PageSetup {
    fn default() -> Self {
        // Letter with 1 inch margins, measured from the original's page.
        Self {
            width: 8.5,
            height: 11.0,
            margin_top: 1.0,
            margin_bottom: 1.0,
            margin_left: 1.0,
            margin_right: 1.0,
            background: [255, 255, 255],
            page_numbers: false,
        }
    }
}

pub const PAGE_SIZES: &[(&str, f32, f32)] = &[
    ("Letter (8.5 × 11 in)", 8.5, 11.0),
    ("Legal (8.5 × 14 in)", 8.5, 14.0),
    ("A4 (210 × 297 mm)", 8.27, 11.69),
    ("A5 (148 × 210 mm)", 5.83, 8.27),
    ("Executive (7.25 × 10.5 in)", 7.25, 10.5),
];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocInfo {
    pub title: String,
    pub author: String,
    pub subject: String,
    pub keywords: String,
    pub notes: String,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Pos {
    pub p: usize,
    pub c: usize,
}

impl Pos {
    pub fn new(p: usize, c: usize) -> Self {
        Self { p, c }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub paras: Vec<Para>,
    pub page: PageSetup,
    pub header: String,
    pub footer: String,
    pub info: DocInfo,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            paras: vec![Para::new(CharStyle::default(), ParaFmt::default())],
            page: PageSetup::default(),
            header: String::new(),
            footer: String::new(),
            info: DocInfo::default(),
        }
    }
}

/// A rich fragment kept for internal copy/paste.
#[derive(Clone, Debug, Default)]
pub struct Fragment {
    pub paras: Vec<Para>,
}

pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '’' || c == '_'
}

impl Document {
    pub fn clamp(&self, pos: Pos) -> Pos {
        let p = pos.p.min(self.paras.len() - 1);
        Pos::new(p, pos.c.min(self.paras[p].len()))
    }
    pub fn end(&self) -> Pos {
        let p = self.paras.len() - 1;
        Pos::new(p, self.paras[p].len())
    }

    pub fn style_at(&self, pos: Pos) -> CharStyle {
        let pos = self.clamp(pos);
        self.paras[pos.p].style_at(pos.c)
    }

    /// Insert text (may contain '\n' for new paragraphs). Returns the position after it.
    pub fn insert_text(&mut self, pos: Pos, text: &str, style: CharStyle) -> Pos {
        let mut pos = self.clamp(pos);
        for ch in text.chars() {
            match ch {
                '\r' => {}
                '\n' => pos = self.split(pos),
                c => {
                    let para = &mut self.paras[pos.p];
                    para.text.insert(pos.c, c);
                    para.styles.insert(pos.c, style);
                    pos.c += 1;
                }
            }
        }
        pos
    }

    /// Split the paragraph at `pos` (Enter). Returns the start of the new paragraph.
    pub fn split(&mut self, pos: Pos) -> Pos {
        let pos = self.clamp(pos);
        let style = self.paras[pos.p].style_at(pos.c);
        let para = &mut self.paras[pos.p];
        let tail_text: Vec<char> = para.text.split_off(pos.c);
        let tail_styles: Vec<CharStyle> = para.styles.split_off(pos.c);
        let mut fmt = para.fmt;
        fmt.page_break_before = false;
        // A heading followed by Enter continues with Default Text, like most modern editors.
        let styles = named_styles();
        if tail_text.is_empty() && matches!(styles.get(fmt.style).map(|s| s.name), Some("Title" | "Subtitle" | "Heading 1" | "Heading 2" | "Heading 3")) {
            fmt = ParaFmt::default();
            let np = Para::new(styles[0].chr, fmt);
            self.paras.insert(pos.p + 1, np);
            return Pos::new(pos.p + 1, 0);
        }
        let np = Para { text: tail_text, styles: tail_styles, fmt, end_style: style };
        self.paras.insert(pos.p + 1, np);
        Pos::new(pos.p + 1, 0)
    }

    pub fn delete_range(&mut self, a: Pos, b: Pos) -> Pos {
        let (a, b) = order(self.clamp(a), self.clamp(b));
        if a == b {
            return a;
        }
        if a.p == b.p {
            let para = &mut self.paras[a.p];
            if a.c == 0 && b.c == para.len() {
                para.end_style = para.style_at(0);
            }
            para.text.drain(a.c..b.c);
            para.styles.drain(a.c..b.c);
        } else {
            let tail = self.paras[b.p].clone();
            let first = &mut self.paras[a.p];
            if a.c == 0 && first.len() > 0 {
                first.end_style = first.style_at(0);
            }
            first.text.truncate(a.c);
            first.styles.truncate(a.c);
            first.text.extend_from_slice(&tail.text[b.c..]);
            first.styles.extend_from_slice(&tail.styles[b.c..]);
            self.paras.drain(a.p + 1..=b.p);
        }
        a
    }

    pub fn text_range(&self, a: Pos, b: Pos) -> String {
        let (a, b) = order(self.clamp(a), self.clamp(b));
        let mut s = String::new();
        for p in a.p..=b.p {
            let para = &self.paras[p];
            let from = if p == a.p { a.c } else { 0 };
            let to = if p == b.p { b.c } else { para.len() };
            s.extend(&para.text[from..to]);
            if p != b.p {
                s.push('\n');
            }
        }
        s
    }

    pub fn fragment(&self, a: Pos, b: Pos) -> Fragment {
        let (a, b) = order(self.clamp(a), self.clamp(b));
        let mut paras = Vec::new();
        for p in a.p..=b.p {
            let para = &self.paras[p];
            let from = if p == a.p { a.c } else { 0 };
            let to = if p == b.p { b.c } else { para.len() };
            paras.push(Para {
                text: para.text[from..to].to_vec(),
                styles: para.styles[from..to].to_vec(),
                fmt: para.fmt,
                end_style: para.style_at(from),
            });
        }
        Fragment { paras }
    }

    pub fn insert_fragment(&mut self, pos: Pos, frag: &Fragment) -> Pos {
        let mut pos = self.clamp(pos);
        for (i, fp) in frag.paras.iter().enumerate() {
            if i > 0 {
                pos = self.split(pos);
                self.paras[pos.p].fmt = fp.fmt;
            }
            let para = &mut self.paras[pos.p];
            for (k, ch) in fp.text.iter().enumerate() {
                para.text.insert(pos.c, *ch);
                para.styles.insert(pos.c, fp.styles[k]);
                pos.c += 1;
            }
        }
        pos
    }

    /// Apply a change to every character's style in the range.
    pub fn restyle(&mut self, a: Pos, b: Pos, f: &dyn Fn(&mut CharStyle)) {
        let (a, b) = order(self.clamp(a), self.clamp(b));
        for p in a.p..=b.p {
            let para = &mut self.paras[p];
            let from = if p == a.p { a.c } else { 0 };
            let to = if p == b.p { b.c } else { para.len() };
            for s in &mut para.styles[from..to] {
                f(s);
            }
            if para.text.is_empty() || (from == 0 && to == para.len()) {
                f(&mut para.end_style);
            }
        }
    }

    pub fn para_range(&self, a: Pos, b: Pos) -> std::ops::RangeInclusive<usize> {
        let (a, b) = order(self.clamp(a), self.clamp(b));
        a.p..=b.p
    }

    pub fn word_at(&self, pos: Pos) -> (Pos, Pos) {
        let pos = self.clamp(pos);
        let t = &self.paras[pos.p].text;
        if t.is_empty() {
            return (pos, pos);
        }
        let mut c = pos.c.min(t.len() - 1);
        if !is_word_char(t[c]) && c > 0 && is_word_char(t[c - 1]) {
            c -= 1;
        }
        let kind = is_word_char(t[c]);
        let mut s = c;
        while s > 0 && is_word_char(t[s - 1]) == kind {
            s -= 1;
        }
        let mut e = c;
        while e < t.len() && is_word_char(t[e]) == kind {
            e += 1;
        }
        // include a following space like most editors
        while e < t.len() && t[e] == ' ' && kind {
            e += 1;
        }
        (Pos::new(pos.p, s), Pos::new(pos.p, e))
    }

    pub fn word_left(&self, pos: Pos) -> Pos {
        let pos = self.clamp(pos);
        if pos.c == 0 {
            if pos.p == 0 {
                return pos;
            }
            return Pos::new(pos.p - 1, self.paras[pos.p - 1].len());
        }
        let t = &self.paras[pos.p].text;
        let mut c = pos.c;
        while c > 0 && !is_word_char(t[c - 1]) {
            c -= 1;
        }
        while c > 0 && is_word_char(t[c - 1]) {
            c -= 1;
        }
        Pos::new(pos.p, c)
    }

    pub fn word_right(&self, pos: Pos) -> Pos {
        let pos = self.clamp(pos);
        let t = &self.paras[pos.p].text;
        if pos.c >= t.len() {
            if pos.p + 1 >= self.paras.len() {
                return pos;
            }
            return Pos::new(pos.p + 1, 0);
        }
        let mut c = pos.c;
        while c < t.len() && is_word_char(t[c]) {
            c += 1;
        }
        while c < t.len() && !is_word_char(t[c]) {
            c += 1;
        }
        Pos::new(pos.p, c)
    }

    pub fn prev(&self, pos: Pos) -> Pos {
        if pos.c > 0 {
            Pos::new(pos.p, pos.c - 1)
        } else if pos.p > 0 {
            Pos::new(pos.p - 1, self.paras[pos.p - 1].len())
        } else {
            pos
        }
    }
    pub fn next(&self, pos: Pos) -> Pos {
        if pos.c < self.paras[pos.p].len() {
            Pos::new(pos.p, pos.c + 1)
        } else if pos.p + 1 < self.paras.len() {
            Pos::new(pos.p + 1, 0)
        } else {
            pos
        }
    }

    pub fn plain_text(&self) -> String {
        self.paras.iter().map(|p| p.string()).collect::<Vec<_>>().join("\n")
    }

    /// (characters, words) — the counts behind "The document contains %ld characters and %ld words."
    pub fn counts(&self, range: Option<(Pos, Pos)>) -> (usize, usize) {
        let text = match range {
            Some((a, b)) => self.text_range(a, b),
            None => self.plain_text(),
        };
        let chars = text.chars().filter(|c| *c != '\n').count();
        let words = text.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).count();
        (chars, words)
    }

    /// Number for a numbered list paragraph.
    pub fn list_number(&self, p: usize) -> usize {
        let fmt = self.paras[p].fmt;
        let mut n = 1;
        let mut i = p;
        while i > 0 {
            i -= 1;
            let f = self.paras[i].fmt;
            if f.list == ListKind::Number && f.indent == fmt.indent {
                n += 1;
            } else if f.list != ListKind::None && f.indent > fmt.indent {
                continue;
            } else {
                break;
            }
        }
        n
    }
}

pub fn order(a: Pos, b: Pos) -> (Pos, Pos) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

// ───────────────────────── Find ─────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FindMode {
    Containing,
    WholeWords,
    StartingWith,
    EndingWith,
}

impl FindMode {
    pub fn label(self) -> &'static str {
        match self {
            FindMode::WholeWords => "Whole words only",
            FindMode::StartingWith => "Words starting with",
            FindMode::EndingWith => "Words ending with",
            FindMode::Containing => "Words containing",
        }
    }
}

pub fn find_all(doc: &Document, needle: &str, mode: FindMode, case: bool) -> Vec<(Pos, Pos)> {
    let n: Vec<char> = if case { needle.chars().collect() } else { needle.to_lowercase().chars().collect() };
    let mut out = Vec::new();
    if n.is_empty() {
        return out;
    }
    for (pi, para) in doc.paras.iter().enumerate() {
        let hay: Vec<char> = if case {
            para.text.clone()
        } else {
            para.text.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect()
        };
        if hay.len() < n.len() {
            continue;
        }
        let mut i = 0;
        while i + n.len() <= hay.len() {
            if hay[i..i + n.len()] == n[..] {
                let before_ok = i == 0 || !is_word_char(hay[i - 1]);
                let after_ok = i + n.len() == hay.len() || !is_word_char(hay[i + n.len()]);
                let ok = match mode {
                    FindMode::Containing => true,
                    FindMode::WholeWords => before_ok && after_ok,
                    FindMode::StartingWith => before_ok,
                    FindMode::EndingWith => after_ok,
                };
                if ok {
                    out.push((Pos::new(pi, i), Pos::new(pi, i + n.len())));
                    i += n.len();
                    continue;
                }
            }
            i += 1;
        }
    }
    out
}

// ───────────────────────── Case changes ─────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseChange {
    Upper,
    Lower,
    Title,
    Sentence,
}

pub fn change_case(doc: &mut Document, a: Pos, b: Pos, kind: CaseChange) {
    let (a, b) = order(doc.clamp(a), doc.clamp(b));
    for p in a.p..=b.p {
        let para = &mut doc.paras[p];
        let from = if p == a.p { a.c } else { 0 };
        let to = if p == b.p { b.c } else { para.len() };
        let mut prev_word = from > 0 && is_word_char(para.text[from - 1]);
        let mut sentence_start = from == 0 || {
            let mut k = from;
            while k > 0 && para.text[k - 1] == ' ' {
                k -= 1;
            }
            k == 0 || matches!(para.text[k - 1], '.' | '!' | '?')
        };
        for i in from..to {
            let c = para.text[i];
            let nc = match kind {
                CaseChange::Upper => c.to_uppercase().next().unwrap_or(c),
                CaseChange::Lower => c.to_lowercase().next().unwrap_or(c),
                CaseChange::Title => {
                    if !prev_word {
                        c.to_uppercase().next().unwrap_or(c)
                    } else {
                        c.to_lowercase().next().unwrap_or(c)
                    }
                }
                CaseChange::Sentence => {
                    if sentence_start && c.is_alphabetic() {
                        sentence_start = false;
                        c.to_uppercase().next().unwrap_or(c)
                    } else {
                        c.to_lowercase().next().unwrap_or(c)
                    }
                }
            };
            if matches!(c, '.' | '!' | '?') {
                sentence_start = true;
            }
            prev_word = is_word_char(c);
            para.text[i] = nc;
        }
    }
}

// ───────────────────────── Saving ─────────────────────────

#[derive(Serialize, Deserialize)]
struct FileRun {
    t: String,
    s: CharStyle,
}

#[derive(Serialize, Deserialize)]
struct FilePara {
    fmt: ParaFmt,
    end: CharStyle,
    runs: Vec<FileRun>,
}

#[derive(Serialize, Deserialize)]
struct FileDoc {
    format: String,
    version: u32,
    page: PageSetup,
    #[serde(default)]
    header: String,
    #[serde(default)]
    footer: String,
    #[serde(default)]
    info: DocInfo,
    paras: Vec<FilePara>,
}

pub const FILE_EXT: &str = "wpx";

pub fn to_json(doc: &Document) -> String {
    let fd = FileDoc {
        format: "scribe-pro-document".into(),
        version: 1,
        page: doc.page,
        header: doc.header.clone(),
        footer: doc.footer.clone(),
        info: doc.info.clone(),
        paras: doc
            .paras
            .iter()
            .map(|p| FilePara {
                fmt: p.fmt,
                end: p.end_style,
                runs: p.runs().into_iter().map(|(a, b, s)| FileRun { t: p.text[a..b].iter().collect(), s }).collect(),
            })
            .collect(),
    };
    serde_json::to_string_pretty(&fd).unwrap_or_default()
}

pub fn from_json(s: &str) -> Result<Document, String> {
    let fd: FileDoc = serde_json::from_str(s).map_err(|e| e.to_string())?;
    let mut paras = Vec::new();
    for fp in fd.paras {
        let mut p = Para::new(fp.end, fp.fmt);
        for r in fp.runs {
            for ch in r.t.chars() {
                p.text.push(ch);
                p.styles.push(r.s);
            }
        }
        paras.push(p);
    }
    if paras.is_empty() {
        paras.push(Para::new(CharStyle::default(), ParaFmt::default()));
    }
    Ok(Document { paras, page: fd.page, header: fd.header, footer: fd.footer, info: fd.info })
}

pub fn from_plain(s: &str) -> Document {
    let mut d = Document::default();
    let st = CharStyle::default();
    d.insert_text(Pos::new(0, 0), &s.replace("\r\n", "\n"), st);
    d
}

// ───────────────────────── RTF export / import ─────────────────────────

pub fn to_rtf(doc: &Document) -> String {
    let mut colors: Vec<[u8; 3]> = vec![[0, 0, 0]];
    for p in &doc.paras {
        for s in p.styles.iter().chain(std::iter::once(&p.end_style)) {
            if !colors.contains(&s.color) {
                colors.push(s.color);
            }
            if let Some(h) = s.highlight {
                if !colors.contains(&h) {
                    colors.push(h);
                }
            }
        }
    }
    let mut out = String::from("{\\rtf1\\ansi\\deff0{\\fonttbl");
    for (i, f) in FONT_NAMES.iter().enumerate() {
        out.push_str(&format!("{{\\f{i} {f};}}"));
    }
    out.push_str("}{\\colortbl;");
    for c in &colors {
        out.push_str(&format!("\\red{}\\green{}\\blue{};", c[0], c[1], c[2]));
    }
    out.push('}');
    let pg = doc.page;
    let tw = |inch: f32| (inch * 1440.0).round() as i32;
    out.push_str(&format!(
        "\\paperw{}\\paperh{}\\margl{}\\margr{}\\margt{}\\margb{}\n",
        tw(pg.width),
        tw(pg.height),
        tw(pg.margin_left),
        tw(pg.margin_right),
        tw(pg.margin_top),
        tw(pg.margin_bottom)
    ));
    for (pi, p) in doc.paras.iter().enumerate() {
        out.push_str("\\pard");
        if p.fmt.page_break_before {
            out.push_str("\\pagebb");
        }
        out.push_str(match p.fmt.align {
            Align::Left => "\\ql",
            Align::Center => "\\qc",
            Align::Right => "\\qr",
            Align::Justify => "\\qj",
        });
        let li = p.fmt.indent as i32 * 720 + if p.fmt.list != ListKind::None { 360 } else { 0 };
        if li > 0 {
            out.push_str(&format!("\\li{li}"));
        }
        if p.fmt.list != ListKind::None {
            out.push_str("\\fi-360");
        }
        out.push_str(&format!("\\sb{}\\sa{}\\sl{}\\slmult1 ", (p.fmt.space_before * 20.0) as i32, (p.fmt.space_after * 20.0) as i32, (p.fmt.line_spacing * 240.0) as i32));
        match p.fmt.list {
            ListKind::Bullet => out.push_str("\\u8226?\\tab "),
            ListKind::Number => out.push_str(&format!("{}.\\tab ", doc.list_number(pi))),
            ListKind::None => {}
        }
        let runs = p.runs();
        if runs.is_empty() {
            let s = p.end_style;
            out.push_str(&format!("{{\\f{}\\fs{} }}", s.font, (s.size * 2.0) as i32));
        }
        for (a, b, s) in runs {
            out.push('{');
            out.push_str(&format!("\\f{}\\fs{}", s.font, (s.size * 2.0).round() as i32));
            if s.bold {
                out.push_str("\\b");
            }
            if s.italic {
                out.push_str("\\i");
            }
            if s.underline {
                out.push_str("\\ul");
            }
            if s.strike {
                out.push_str("\\strike");
            }
            if s.script > 0 {
                out.push_str("\\super");
            }
            if s.script < 0 {
                out.push_str("\\sub");
            }
            let ci = colors.iter().position(|c| *c == s.color).unwrap_or(0) + 1;
            out.push_str(&format!("\\cf{ci}"));
            if let Some(h) = s.highlight {
                let hi = colors.iter().position(|c| *c == h).unwrap_or(0) + 1;
                out.push_str(&format!("\\highlight{hi}"));
            }
            out.push(' ');
            for ch in &p.text[a..b] {
                match ch {
                    '\\' => out.push_str("\\\\"),
                    '{' => out.push_str("\\{"),
                    '}' => out.push_str("\\}"),
                    '\t' => out.push_str("\\tab "),
                    c if (*c as u32) < 128 => out.push(*c),
                    c => {
                        let mut buf = [0u16; 2];
                        for u in c.encode_utf16(&mut buf) {
                            out.push_str(&format!("\\u{}?", *u as i16));
                        }
                    }
                }
            }
            out.push('}');
        }
        out.push_str("\\par\n");
    }
    out.push('}');
    out
}

/// A small RTF reader: keeps text, bold/italic/underline, size, font names and alignment.
pub fn from_rtf(src: &str) -> Document {
    let mut doc = Document { paras: vec![Para::new(CharStyle::default(), ParaFmt::default())], ..Default::default() };
    let mut fonts: std::collections::HashMap<i32, u8> = Default::default();
    let mut stack: Vec<(CharStyle, bool)> = Vec::new();
    let mut st = CharStyle::default();
    let mut skip = false;
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut pos = Pos::new(0, 0);
    let mut in_fonttbl = false;
    let mut font_num = 0;
    let mut font_name = String::new();
    let mut uc_skip = 0usize;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '{' => {
                stack.push((st, skip));
                i += 1;
                if i < chars.len() && chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '*' {
                    skip = true;
                }
            }
            '}' => {
                if in_fonttbl && !font_name.is_empty() {
                    fonts.insert(font_num, font_index(font_name.trim().trim_end_matches(';')));
                    font_name.clear();
                }
                if let Some((s, k)) = stack.pop() {
                    st = s;
                    skip = k;
                }
                if in_fonttbl && stack.len() <= 1 {
                    in_fonttbl = false;
                    skip = false;
                }
                i += 1;
            }
            '\\' => {
                i += 1;
                if i >= chars.len() {
                    break;
                }
                let c2 = chars[i];
                if !c2.is_ascii_alphabetic() {
                    i += 1;
                    match c2 {
                        '\\' | '{' | '}' if !skip => pos = doc.insert_text(pos, &c2.to_string(), st),
                        '\'' => {
                            let hex: String = chars[i..(i + 2).min(chars.len())].iter().collect();
                            i += 2;
                            if uc_skip > 0 {
                                uc_skip -= 1;
                            } else if !skip {
                                if let Ok(v) = u8::from_str_radix(&hex, 16) {
                                    let ch = cp1252(v);
                                    pos = doc.insert_text(pos, &ch.to_string(), st);
                                }
                            }
                        }
                        '~' if !skip => pos = doc.insert_text(pos, "\u{a0}", st),
                        '*' => skip = true,
                        _ => {}
                    }
                    continue;
                }
                let start = i;
                while i < chars.len() && chars[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let nstart = i;
                if i < chars.len() && (chars[i] == '-' || chars[i].is_ascii_digit()) {
                    i += 1;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let num: Option<i32> = if i > nstart { chars[nstart..i].iter().collect::<String>().parse().ok() } else { None };
                if i < chars.len() && chars[i] == ' ' {
                    i += 1;
                }
                let on = num != Some(0);
                match word.as_str() {
                    "fonttbl" => {
                        in_fonttbl = true;
                        skip = true;
                    }
                    "colortbl" | "stylesheet" | "info" | "pict" | "header" | "footer" | "listtable" | "listoverridetable" | "generator" => skip = true,
                    "f" if in_fonttbl => font_num = num.unwrap_or(0),
                    "f" => st.font = *fonts.get(&num.unwrap_or(0)).unwrap_or(&0),
                    "fs" => st.size = (num.unwrap_or(24) as f32 / 2.0).clamp(MIN_SIZE, MAX_SIZE),
                    "b" => st.bold = on,
                    "i" => st.italic = on,
                    "ul" => st.underline = on,
                    "ulnone" => st.underline = false,
                    "strike" => st.strike = on,
                    "super" => st.script = 1,
                    "sub" => st.script = -1,
                    "nosupersub" => st.script = 0,
                    "plain" => st = CharStyle::default(),
                    "par" | "line" if !skip => pos = doc.split(pos),
                    "tab" if !skip => pos = doc.insert_text(pos, "\t", st),
                    "qc" => doc.paras[pos.p].fmt.align = Align::Center,
                    "qr" => doc.paras[pos.p].fmt.align = Align::Right,
                    "qj" => doc.paras[pos.p].fmt.align = Align::Justify,
                    "ql" => doc.paras[pos.p].fmt.align = Align::Left,
                    "pard" => doc.paras[pos.p].fmt = ParaFmt::default(),
                    "u" if !skip => {
                        if let Some(n) = num {
                            let v = if n < 0 { (n + 65536) as u32 } else { n as u32 };
                            if let Some(ch) = char::from_u32(v) {
                                pos = doc.insert_text(pos, &ch.to_string(), st);
                            }
                            uc_skip = 1;
                            if i < chars.len() && chars[i] == '?' {
                                i += 1;
                                uc_skip = 0;
                            }
                        }
                    }
                    "emdash" if !skip => pos = doc.insert_text(pos, "—", st),
                    "endash" if !skip => pos = doc.insert_text(pos, "–", st),
                    "bullet" if !skip => pos = doc.insert_text(pos, "•", st),
                    "lquote" if !skip => pos = doc.insert_text(pos, "‘", st),
                    "rquote" if !skip => pos = doc.insert_text(pos, "’", st),
                    "ldblquote" if !skip => pos = doc.insert_text(pos, "“", st),
                    "rdblquote" if !skip => pos = doc.insert_text(pos, "”", st),
                    _ => {}
                }
            }
            '\r' | '\n' => i += 1,
            _ => {
                if in_fonttbl {
                    font_name.push(c);
                } else if !skip {
                    if uc_skip > 0 {
                        uc_skip -= 1;
                    } else {
                        pos = doc.insert_text(pos, &c.to_string(), st);
                    }
                }
                i += 1;
            }
        }
    }
    // drop trailing empty paragraph created by the last \par
    if doc.paras.len() > 1 && doc.paras.last().map(|p| p.text.is_empty()).unwrap_or(false) {
        doc.paras.pop();
    }
    doc
}

fn cp1252(v: u8) -> char {
    const HI: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    if (0x80..0xa0).contains(&v) {
        HI[(v - 0x80) as usize]
    } else {
        v as char
    }
}

// ───────────────────────── SmartCorrect ─────────────────────────

pub fn smartcorrect_table() -> Vec<(String, String)> {
    include_str!("smartcorrect.txt")
        .lines()
        .filter_map(|l| {
            let mut it = l.splitn(2, '\t');
            Some((it.next()?.to_string(), it.next()?.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn insert_and_delete() {
        let mut d = Document::default();
        let p = d.insert_text(Pos::new(0, 0), "Hello\nWorld", CharStyle::default());
        assert_eq!(d.paras.len(), 2);
        assert_eq!(p, Pos::new(1, 5));
        d.delete_range(Pos::new(0, 3), Pos::new(1, 2));
        assert_eq!(d.plain_text(), "Helrld");
        assert_eq!(d.counts(None), (6, 1));
    }
    #[test]
    fn rtf_roundtrip() {
        let mut d = Document::default();
        d.insert_text(Pos::new(0, 0), "Café {x}\nLine two", CharStyle { bold: true, ..Default::default() });
        let r = to_rtf(&d);
        let back = from_rtf(&r);
        assert_eq!(back.plain_text(), d.plain_text());
        assert!(back.paras[0].styles[0].bold);
    }
    #[test]
    fn find_modes() {
        let d = from_plain("cat concat cats");
        assert_eq!(find_all(&d, "cat", FindMode::Containing, false).len(), 3);
        assert_eq!(find_all(&d, "cat", FindMode::WholeWords, false).len(), 1);
        assert_eq!(find_all(&d, "cat", FindMode::StartingWith, false).len(), 2);
        assert_eq!(find_all(&d, "cat", FindMode::EndingWith, false).len(), 2);
    }
}

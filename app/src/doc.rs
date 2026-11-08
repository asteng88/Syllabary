//! Document model: paragraphs of styled characters, page setup, links, comments,
//! smart chips, building blocks, checklists, and export/import operations.

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

pub const DEFAULT_FONT: u8 = 2; // Calibri
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
    /// Optional right-hand end color for a Craft-style left-to-right fading text color.
    #[serde(default)]
    pub color_to: Option<[u8; 3]>,
    pub highlight: Option<[u8; 3]>,
    /// Optional right-hand end color for a Craft-style left-to-right fading highlight.
    #[serde(default)]
    pub highlight_to: Option<[u8; 3]>,
    /// 1 = superscript, -1 = subscript
    pub script: i8,
    /// 0 = no link; >= 1 = 1-based index into `Document::links`
    #[serde(default)]
    pub link: u16,
}

impl CharStyle {
    pub fn is_gradient_color(&self) -> bool {
        matches!(self.color_to, Some(b) if b != self.color)
    }

    pub fn is_gradient_highlight(&self) -> bool {
        matches!((self.highlight, self.highlight_to), (Some(a), Some(b)) if a != b)
    }
}

pub fn lerp_rgb(c1: [u8; 3], c2: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
    [mix(c1[0], c2[0]), mix(c1[1], c2[1]), mix(c1[2], c2[2])]
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
            color_to: None,
            highlight: None,
            highlight_to: None,
            script: 0,
            link: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum ShortcutPreset {
    #[default]
    GoogleDocs,
    MsWord,
    WordPro,
}

impl ShortcutPreset {
    pub const ALL: [ShortcutPreset; 3] = [
        ShortcutPreset::MsWord,
        ShortcutPreset::WordPro,
        ShortcutPreset::GoogleDocs,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ShortcutPreset::MsWord => "MS Word",
            ShortcutPreset::WordPro => "WordPro",
            ShortcutPreset::GoogleDocs => "Google Docs",
        }
    }

    pub fn save_as(self) -> &'static str {
        match self {
            ShortcutPreset::MsWord => "F12",
            ShortcutPreset::WordPro | ShortcutPreset::GoogleDocs => "Ctrl+Shift+S",
        }
    }

    pub fn close_tab(self) -> &'static str {
        match self {
            ShortcutPreset::WordPro => "Ctrl+F4",
            ShortcutPreset::MsWord | ShortcutPreset::GoogleDocs => "Ctrl+W",
        }
    }

    pub fn paste_plain(self) -> &'static str {
        match self {
            ShortcutPreset::MsWord => "Ctrl+Alt+V",
            ShortcutPreset::WordPro | ShortcutPreset::GoogleDocs => "Ctrl+Shift+V",
        }
    }

    pub fn replace(self) -> &'static str {
        match self {
            ShortcutPreset::WordPro => "Ctrl+F",
            ShortcutPreset::MsWord | ShortcutPreset::GoogleDocs => "Ctrl+H",
        }
    }

    pub fn strikethrough(self) -> &'static str {
        match self {
            ShortcutPreset::GoogleDocs => "Alt+Shift+5",
            ShortcutPreset::MsWord => "Ctrl+Shift+X",
            ShortcutPreset::WordPro => "Ctrl+Shift+K",
        }
    }

    pub fn superscript(self) -> &'static str {
        match self {
            ShortcutPreset::MsWord => "Ctrl+Shift+=",
            ShortcutPreset::WordPro | ShortcutPreset::GoogleDocs => "Ctrl+.",
        }
    }

    pub fn subscript(self) -> &'static str {
        match self {
            ShortcutPreset::MsWord => "Ctrl+=",
            ShortcutPreset::WordPro | ShortcutPreset::GoogleDocs => "Ctrl+,",
        }
    }

    pub fn grow_font(self) -> &'static str {
        match self {
            ShortcutPreset::WordPro => "F4",
            ShortcutPreset::MsWord | ShortcutPreset::GoogleDocs => "Ctrl+Shift+.",
        }
    }

    pub fn shrink_font(self) -> &'static str {
        match self {
            ShortcutPreset::WordPro => "Shift+F4",
            ShortcutPreset::MsWord | ShortcutPreset::GoogleDocs => "Ctrl+Shift+,",
        }
    }

    pub fn align_left(self) -> &'static str {
        match self {
            ShortcutPreset::GoogleDocs => "Ctrl+Shift+L",
            ShortcutPreset::MsWord | ShortcutPreset::WordPro => "Ctrl+L",
        }
    }

    pub fn align_center(self) -> &'static str {
        match self {
            ShortcutPreset::GoogleDocs => "Ctrl+Shift+E",
            ShortcutPreset::MsWord | ShortcutPreset::WordPro => "Ctrl+E",
        }
    }

    pub fn align_right(self) -> &'static str {
        match self {
            ShortcutPreset::GoogleDocs => "Ctrl+Shift+R",
            ShortcutPreset::MsWord | ShortcutPreset::WordPro => "Ctrl+R",
        }
    }

    pub fn align_justify(self) -> &'static str {
        match self {
            ShortcutPreset::GoogleDocs => "Ctrl+Shift+J",
            ShortcutPreset::MsWord | ShortcutPreset::WordPro => "Ctrl+J",
        }
    }

    pub fn indent(self) -> &'static str {
        match self {
            ShortcutPreset::MsWord => "Ctrl+M",
            ShortcutPreset::WordPro | ShortcutPreset::GoogleDocs => "Ctrl+]",
        }
    }

    pub fn outdent(self) -> &'static str {
        match self {
            ShortcutPreset::MsWord => "Ctrl+Shift+M",
            ShortcutPreset::WordPro | ShortcutPreset::GoogleDocs => "Ctrl+[",
        }
    }

    pub fn bullet_list(self) -> &'static str {
        match self {
            ShortcutPreset::GoogleDocs => "Ctrl+Shift+8",
            ShortcutPreset::MsWord => "Ctrl+Shift+L",
            ShortcutPreset::WordPro => "Ctrl+Shift+B",
        }
    }

    pub fn number_list(self) -> &'static str {
        match self {
            ShortcutPreset::GoogleDocs | ShortcutPreset::MsWord => "Ctrl+Shift+7",
            ShortcutPreset::WordPro => "Ctrl+Shift+N",
        }
    }

    pub fn checklist(self) -> &'static str {
        "Ctrl+Shift+9"
    }

    pub fn clear_format(self) -> &'static str {
        match self {
            ShortcutPreset::GoogleDocs => "Ctrl+\\",
            ShortcutPreset::MsWord | ShortcutPreset::WordPro => "Ctrl+Space",
        }
    }

    pub fn style_normal(self) -> &'static str {
        match self {
            ShortcutPreset::GoogleDocs => "Ctrl+Alt+0",
            ShortcutPreset::MsWord => "Ctrl+Shift+N",
            ShortcutPreset::WordPro => "Alt+0",
        }
    }

    pub fn style_h1(self) -> &'static str {
        match self {
            ShortcutPreset::WordPro => "Alt+1",
            ShortcutPreset::GoogleDocs | ShortcutPreset::MsWord => "Ctrl+Alt+1",
        }
    }

    pub fn style_h2(self) -> &'static str {
        match self {
            ShortcutPreset::WordPro => "Alt+2",
            ShortcutPreset::GoogleDocs | ShortcutPreset::MsWord => "Ctrl+Alt+2",
        }
    }

    pub fn style_h3(self) -> &'static str {
        match self {
            ShortcutPreset::WordPro => "Alt+3",
            ShortcutPreset::GoogleDocs | ShortcutPreset::MsWord => "Ctrl+Alt+3",
        }
    }

    pub fn spellcheck(self) -> &'static str {
        match self {
            ShortcutPreset::WordPro => "Ctrl+F2",
            ShortcutPreset::GoogleDocs | ShortcutPreset::MsWord => "F7",
        }
    }

    pub fn word_count(self) -> &'static str {
        match self {
            ShortcutPreset::GoogleDocs => "Ctrl+Shift+C",
            ShortcutPreset::MsWord => "Ctrl+Shift+G",
            ShortcutPreset::WordPro => "Ctrl+Shift+W",
        }
    }

    pub fn dictionary(self) -> &'static str {
        match self {
            ShortcutPreset::GoogleDocs => "Ctrl+Shift+Y",
            ShortcutPreset::MsWord => "Shift+F7",
            ShortcutPreset::WordPro => "Alt+F2",
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
    Checklist,
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
    #[serde(default)]
    pub checked: bool,
    #[serde(default)]
    pub bullet_preset: u8,
    #[serde(default)]
    pub hr: bool,
    #[serde(default)]
    pub shading: Option<[u8; 3]>,
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
            checked: false,
            bullet_preset: 0,
            hr: false,
            shading: None,
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
            fmt: ParaFmt { space_after: 2.0, shading: Some([241, 245, 249]), ..pf },
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
    ("Tabloid (11 × 17 in)", 11.0, 17.0),
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocComment {
    pub id: u64,
    pub para: usize,
    pub start: usize,
    pub end: usize,
    pub quote: String,
    pub author: String,
    pub text: String,
    pub timestamp: String,
    pub resolved: bool,
    #[serde(default)]
    pub replies: Vec<(String, String, String)>,
    #[serde(default)]
    pub is_suggestion: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocVersion {
    pub name: String,
    pub timestamp: String,
    pub words: usize,
    pub snapshot_json: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CitationEntry {
    pub author: String,
    pub title: String,
    pub year: String,
    pub source: String,
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
    pub links: Vec<String>,
    pub comments: Vec<DocComment>,
    pub bookmarks: Vec<(String, usize)>,
    pub summary: String,
    pub starred: bool,
    pub pageless: bool,
    pub citations: Vec<CitationEntry>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            paras: vec![Para::new(CharStyle::default(), ParaFmt::default())],
            page: PageSetup::default(),
            header: String::new(),
            footer: String::new(),
            info: DocInfo::default(),
            links: Vec::new(),
            comments: Vec::new(),
            bookmarks: Vec::new(),
            summary: String::new(),
            starred: false,
            pageless: false,
            citations: Vec::new(),
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

fn to_roman(mut n: usize) -> String {
    let table = [
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut s = String::new();
    for (v, sym) in table {
        while n >= v {
            s.push_str(sym);
            n -= v;
        }
    }
    if s.is_empty() { "i".into() } else { s }
}

fn to_alpha(n: usize) -> String {
    let idx = n.saturating_sub(1) % 26;
    ((b'a' + idx as u8) as char).to_string()
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

    pub fn add_link(&mut self, url: &str) -> u16 {
        let u = url.trim();
        if u.is_empty() {
            return 0;
        }
        if let Some(idx) = self.links.iter().position(|x| x == u) {
            return (idx + 1) as u16;
        }
        self.links.push(u.to_string());
        self.links.len() as u16
    }

    pub fn link_url(&self, id: u16) -> Option<&str> {
        if id == 0 {
            None
        } else {
            self.links.get((id - 1) as usize).map(|s| s.as_str())
        }
    }

    /// If `pos` is inside or immediately adjacent to a link run in the same paragraph, returns `(start, end, link_id)`.
    pub fn link_bounds(&self, pos: Pos) -> Option<(Pos, Pos, u16)> {
        let pos = self.clamp(pos);
        let para = &self.paras[pos.p];
        if para.text.is_empty() {
            return None;
        }
        let idx = if pos.c < para.len() && para.styles[pos.c].link > 0 {
            pos.c
        } else if pos.c > 0 && para.styles[pos.c - 1].link > 0 {
            pos.c - 1
        } else {
            return None;
        };
        let lid = para.styles[idx].link;
        let mut s = idx;
        while s > 0 && para.styles[s - 1].link == lid {
            s -= 1;
        }
        let mut e = idx + 1;
        while e < para.len() && para.styles[e].link == lid {
            e += 1;
        }
        Some((Pos::new(pos.p, s), Pos::new(pos.p, e), lid))
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
        let mut style = self.paras[pos.p].style_at(pos.c);
        style.link = 0;
        let para = &mut self.paras[pos.p];
        let tail_text: Vec<char> = para.text.split_off(pos.c);
        let tail_styles: Vec<CharStyle> = para.styles.split_off(pos.c);
        let mut fmt = para.fmt;
        fmt.page_break_before = false;
        fmt.hr = false;
        fmt.checked = false;
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
                para.end_style.link = 0;
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

    /// Marker string for any list kind, supporting Google Docs bullet & numbering presets.
    pub fn list_marker(&self, p: usize) -> String {
        let fmt = self.paras[p].fmt;
        let lvl = (fmt.indent as usize) % 3;
        match fmt.list {
            ListKind::None => String::new(),
            ListKind::Checklist => if fmt.checked { "☑".into() } else { "☐".into() },
            ListKind::Bullet => {
                let sets: [[&str; 3]; 4] = [
                    ["•", "◦", "▪"],
                    ["❖", "➢", "▪"],
                    ["➔", "➢", "•"],
                    ["★", "☆", "•"],
                ];
                sets[(fmt.bullet_preset as usize) % sets.len()][lvl].to_string()
            }
            ListKind::Number => {
                let n = self.list_number(p);
                match fmt.bullet_preset % 4 {
                    0 => match lvl {
                        0 => format!("{n}."),
                        1 => format!("{}.", to_alpha(n)),
                        _ => format!("{}.", to_roman(n)),
                    },
                    1 => match lvl {
                        0 => format!("{n})"),
                        1 => format!("{})", to_alpha(n)),
                        _ => format!("{})", to_roman(n)),
                    },
                    2 => match lvl {
                        0 => format!("{}.", to_roman(n).to_uppercase()),
                        1 => format!("{}.", to_alpha(n).to_uppercase()),
                        _ => format!("{n}."),
                    },
                    _ => format!("{}.", to_alpha(n).to_uppercase()),
                }
            }
        }
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
    #[serde(default)]
    links: Vec<String>,
    #[serde(default)]
    comments: Vec<DocComment>,
    #[serde(default)]
    bookmarks: Vec<(String, usize)>,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    starred: bool,
    #[serde(default)]
    pageless: bool,
    #[serde(default)]
    citations: Vec<CitationEntry>,
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
        links: doc.links.clone(),
        comments: doc.comments.clone(),
        bookmarks: doc.bookmarks.clone(),
        summary: doc.summary.clone(),
        starred: doc.starred,
        pageless: doc.pageless,
        citations: doc.citations.clone(),
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
    Ok(Document {
        paras,
        page: fd.page,
        header: fd.header,
        footer: fd.footer,
        info: fd.info,
        links: fd.links,
        comments: fd.comments,
        bookmarks: fd.bookmarks,
        summary: fd.summary,
        starred: fd.starred,
        pageless: fd.pageless,
        citations: fd.citations,
    })
}

pub fn from_plain(s: &str) -> Document {
    let mut d = Document::default();
    let st = CharStyle::default();
    d.insert_text(Pos::new(0, 0), &s.replace("\r\n", "\n"), st);
    d
}

// ───────────────────────── Google Docs (.gdoc) export / import ─────────────────────────

pub fn to_gdoc(doc: &Document) -> String {
    let styles = named_styles();
    let title = if doc.info.title.is_empty() { "Untitled document" } else { &doc.info.title };
    let mut content = Vec::new();
    for p in &doc.paras {
        if p.fmt.hr {
            content.push(serde_json::json!({
                "paragraph": {
                    "elements": [{ "horizontalRule": {} }],
                    "paragraphStyle": { "namedStyleType": "NORMAL_TEXT" }
                }
            }));
            continue;
        }
        let named_style = match styles.get(p.fmt.style).map(|s| s.name) {
            Some("Title") => "TITLE",
            Some("Subtitle") => "SUBTITLE",
            Some("Heading 1") => "HEADING_1",
            Some("Heading 2") => "HEADING_2",
            Some("Heading 3") => "HEADING_3",
            _ => "NORMAL_TEXT",
        };
        let alignment = match p.fmt.align {
            Align::Left => "START",
            Align::Center => "CENTER",
            Align::Right => "END",
            Align::Justify => "JUSTIFIED",
        };
        let mut elements = Vec::new();
        for (a, b, s) in p.runs() {
            let mut txt: String = p.text[a..b].iter().collect();
            if b == p.text.len() {
                txt.push('\n');
            }
            let font_name = FONT_NAMES[s.font as usize % FONT_NAMES.len()];
            let baseline = if s.script > 0 {
                "SUPERSCRIPT"
            } else if s.script < 0 {
                "SUBSCRIPT"
            } else {
                "NONE"
            };
            let mut ts = serde_json::json!({
                "bold": s.bold,
                "italic": s.italic,
                "underline": s.underline,
                "strikethrough": s.strike,
                "baselineOffset": baseline,
                "fontSize": { "magnitude": s.size, "unit": "PT" },
                "weightedFontFamily": { "fontFamily": font_name },
                "foregroundColor": {
                    "color": {
                        "rgbColor": {
                            "red": s.color[0] as f32 / 255.0,
                            "green": s.color[1] as f32 / 255.0,
                            "blue": s.color[2] as f32 / 255.0,
                        }
                    }
                }
            });
            if let Some(hl) = s.highlight {
                ts["backgroundColor"] = serde_json::json!({
                    "color": {
                        "rgbColor": {
                            "red": hl[0] as f32 / 255.0,
                            "green": hl[1] as f32 / 255.0,
                            "blue": hl[2] as f32 / 255.0,
                        }
                    }
                });
            }
            if let Some(url) = doc.link_url(s.link) {
                ts["link"] = serde_json::json!({ "url": url });
            }
            elements.push(serde_json::json!({
                "textRun": {
                    "content": txt,
                    "textStyle": ts
                }
            }));
        }
        if elements.is_empty() {
            elements.push(serde_json::json!({
                "textRun": {
                    "content": "\n",
                    "textStyle": {}
                }
            }));
        }
        let mut para_obj = serde_json::json!({
            "elements": elements,
            "paragraphStyle": {
                "namedStyleType": named_style,
                "alignment": alignment,
                "lineSpacing": p.fmt.line_spacing * 100.0,
                "spaceAbove": { "magnitude": p.fmt.space_before, "unit": "PT" },
                "spaceBelow": { "magnitude": p.fmt.space_after, "unit": "PT" },
                "indentStart": { "magnitude": p.fmt.indent as f32 * 36.0, "unit": "PT" }
            }
        });
        if p.fmt.list != ListKind::None {
            let list_id = match p.fmt.list {
                ListKind::Bullet => "kix.bullet",
                ListKind::Number => "kix.number",
                ListKind::Checklist => "kix.checklist",
                ListKind::None => "",
            };
            para_obj["bullet"] = serde_json::json!({
                "listId": list_id,
                "nestingLevel": p.fmt.indent,
                "checked": p.fmt.checked
            });
        }
        content.push(serde_json::json!({ "paragraph": para_obj }));
    }

    let mut val: serde_json::Value = serde_json::from_str(&to_json(doc)).unwrap_or_else(|_| serde_json::json!({}));
    val["format"] = serde_json::json!("google-docs-document");
    val["title"] = serde_json::json!(title);
    val["doc_id"] = serde_json::json!("scribe-pro-gdoc");
    val["body"] = serde_json::json!({ "content": content });
    serde_json::to_string_pretty(&val).unwrap_or_default()
}

pub fn from_gdoc(s: &str) -> Result<Document, String> {
    let v: serde_json::Value = serde_json::from_str(s).map_err(|e| e.to_string())?;
    // 1. Native Scribe Pro / hybrid .gdoc with lossless `paras` array
    if v.get("paras").and_then(|p| p.as_array()).is_some() {
        return from_json(s);
    }
    // 2. Google Docs REST API v1 JSON structure (`body.content`)
    if let Some(content) = v.get("body").and_then(|b| b.get("content")).and_then(|c| c.as_array()) {
        let mut doc = Document { paras: Vec::new(), ..Default::default() };
        if let Some(t) = v.get("title").and_then(|t| t.as_str()) {
            doc.info.title = t.to_string();
        }
        let styles = named_styles();
        for item in content {
            let Some(p_obj) = item.get("paragraph") else { continue };
            let mut fmt = ParaFmt::default();
            let mut base = CharStyle::default();
            if let Some(ps) = p_obj.get("paragraphStyle") {
                if let Some(nst) = ps.get("namedStyleType").and_then(|x| x.as_str()) {
                    let style_idx = match nst {
                        "TITLE" => Some(2),
                        "SUBTITLE" => Some(3),
                        "HEADING_1" => Some(4),
                        "HEADING_2" => Some(5),
                        "HEADING_3" | "HEADING_4" | "HEADING_5" | "HEADING_6" => Some(6),
                        _ => None,
                    };
                    if let Some(idx) = style_idx {
                        fmt = ParaFmt { style: idx, ..styles[idx].fmt };
                        base = styles[idx].chr;
                    }
                }
                if let Some(al) = ps.get("alignment").and_then(|x| x.as_str()) {
                    fmt.align = match al {
                        "CENTER" => Align::Center,
                        "END" | "RIGHT" => Align::Right,
                        "JUSTIFIED" => Align::Justify,
                        _ => Align::Left,
                    };
                }
                if let Some(ls) = ps.get("lineSpacing").and_then(|x| x.as_f64()) {
                    fmt.line_spacing = (ls as f32 / 100.0).clamp(1.0, 3.0);
                }
                if let Some(sa) = ps.get("spaceAbove").and_then(|x| x.get("magnitude")).and_then(|x| x.as_f64()) {
                    fmt.space_before = (sa as f32).clamp(0.0, 72.0);
                }
                if let Some(sb) = ps.get("spaceBelow").and_then(|x| x.get("magnitude")).and_then(|x| x.as_f64()) {
                    fmt.space_after = (sb as f32).clamp(0.0, 72.0);
                }
                if let Some(ind) = ps.get("indentStart").and_then(|x| x.get("magnitude")).and_then(|x| x.as_f64()) {
                    fmt.indent = ((ind as f32 / 36.0).round() as i32).clamp(0, 8) as u8;
                }
            }
            if let Some(bul) = p_obj.get("bullet") {
                let lid = bul.get("listId").and_then(|x| x.as_str()).unwrap_or("");
                fmt.list = if lid.contains("number") || lid.contains("decimal") {
                    ListKind::Number
                } else if lid.contains("check") {
                    ListKind::Checklist
                } else {
                    ListKind::Bullet
                };
                if let Some(nl) = bul.get("nestingLevel").and_then(|x| x.as_u64()) {
                    fmt.indent = (nl as u8).min(8);
                }
                if let Some(chk) = bul.get("checked").and_then(|x| x.as_bool()) {
                    fmt.checked = chk;
                }
            }
            let mut p = Para::new(base, fmt);
            if let Some(elems) = p_obj.get("elements").and_then(|e| e.as_array()) {
                for el in elems {
                    if el.get("horizontalRule").is_some() {
                        p.fmt.hr = true;
                        continue;
                    }
                    let Some(tr) = el.get("textRun") else { continue };
                    let raw = tr.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    let txt = raw.strip_suffix('\n').unwrap_or(raw);
                    let mut st = base;
                    if let Some(ts) = tr.get("textStyle") {
                        if let Some(b) = ts.get("bold").and_then(|x| x.as_bool()) {
                            st.bold = b;
                        }
                        if let Some(i) = ts.get("italic").and_then(|x| x.as_bool()) {
                            st.italic = i;
                        }
                        if let Some(u) = ts.get("underline").and_then(|x| x.as_bool()) {
                            st.underline = u;
                        }
                        if let Some(s) = ts.get("strikethrough").and_then(|x| x.as_bool()) {
                            st.strike = s;
                        }
                        if let Some(bo) = ts.get("baselineOffset").and_then(|x| x.as_str()) {
                            st.script = match bo {
                                "SUPERSCRIPT" => 1,
                                "SUBSCRIPT" => -1,
                                _ => 0,
                            };
                        }
                        if let Some(sz) = ts.get("fontSize").and_then(|x| x.get("magnitude")).and_then(|x| x.as_f64()) {
                            st.size = (sz as f32).clamp(MIN_SIZE, MAX_SIZE);
                        }
                        if let Some(ff) = ts.get("weightedFontFamily").and_then(|x| x.get("fontFamily")).and_then(|x| x.as_str()) {
                            st.font = font_index(ff);
                        }
                        let parse_rgb = |c_obj: &serde_json::Value| -> Option<[u8; 3]> {
                            let rgb = c_obj.get("color")?.get("rgbColor")?;
                            let r = (rgb.get("red").and_then(|x| x.as_f64()).unwrap_or(0.0) * 255.0).round() as u8;
                            let g = (rgb.get("green").and_then(|x| x.as_f64()).unwrap_or(0.0) * 255.0).round() as u8;
                            let b = (rgb.get("blue").and_then(|x| x.as_f64()).unwrap_or(0.0) * 255.0).round() as u8;
                            Some([r, g, b])
                        };
                        if let Some(fg) = ts.get("foregroundColor").and_then(parse_rgb) {
                            st.color = fg;
                        }
                        if let Some(bg) = ts.get("backgroundColor").and_then(parse_rgb) {
                            st.highlight = Some(bg);
                        }
                        if let Some(url) = ts.get("link").and_then(|x| x.get("url")).and_then(|x| x.as_str()) {
                            st.link = doc.add_link(url);
                            st.underline = true;
                            if st.color == [0, 0, 0] {
                                st.color = [11, 87, 208];
                            }
                        }
                    }
                    for ch in txt.chars() {
                        p.text.push(ch);
                        p.styles.push(st);
                    }
                    p.end_style = st;
                }
            }
            doc.paras.push(p);
        }
        if doc.paras.is_empty() {
            doc.paras.push(Para::new(CharStyle::default(), ParaFmt::default()));
        }
        return Ok(doc);
    }
    // 3. Google Drive for Desktop shortcut .gdoc ({"url": ..., "doc_id": ..., "email": ...})
    if v.get("url").is_some() || v.get("doc_id").is_some() {
        let mut doc = Document::default();
        let doc_id = v.get("doc_id").and_then(|x| x.as_str()).unwrap_or("");
        let url = v.get("url").and_then(|x| x.as_str()).map_or_else(|| format!("https://docs.google.com/document/d/{doc_id}/edit"), |u| u.to_string());
        let email = v.get("email").and_then(|x| x.as_str()).unwrap_or("");
        doc.info.author = email.to_string();
        doc.info.notes = format!("Google Docs ID: {doc_id}\nURL: {url}");
        let lid = doc.add_link(&url);
        let link_style = CharStyle {
            color: [11, 87, 208],
            underline: true,
            link: lid,
            ..CharStyle::default()
        };
        let pos = doc.insert_text(Pos::new(0, 0), &url, link_style);
        doc.split(pos);
        return Ok(doc);
    }
    Err("unrecognized Google Docs (.gdoc) structure".into())
}

// ───────────────────────── Markdown & HTML export / import ─────────────────────────

pub fn to_markdown(doc: &Document) -> String {
    let mut out = String::new();
    let styles = named_styles();
    for (pi, p) in doc.paras.iter().enumerate() {
        if p.fmt.hr {
            out.push_str("---\n\n");
            continue;
        }
        let sname = styles.get(p.fmt.style).map_or("Default Text", |s| s.name);
        match sname {
            "Title" => out.push_str("# "),
            "Subtitle" => out.push_str("## "),
            "Heading 1" => out.push_str("# "),
            "Heading 2" => out.push_str("## "),
            "Heading 3" => out.push_str("### "),
            "Quote" => out.push_str("> "),
            _ => {}
        }
        let indent = "  ".repeat(p.fmt.indent as usize);
        match p.fmt.list {
            ListKind::Bullet => out.push_str(&format!("{indent}- ")),
            ListKind::Number => out.push_str(&format!("{indent}{}. ", doc.list_number(pi))),
            ListKind::Checklist => out.push_str(&format!("{indent}- [{}] ", if p.fmt.checked { "x" } else { " " })),
            ListKind::None => {}
        }
        for (a, b, s) in p.runs() {
            let txt: String = p.text[a..b].iter().collect();
            let mut seg = txt;
            if s.bold {
                seg = format!("**{seg}**");
            }
            if s.italic {
                seg = format!("*{seg}*");
            }
            if s.strike {
                seg = format!("~~{seg}~~");
            }
            if let Some(url) = doc.link_url(s.link) {
                seg = format!("[{seg}]({url})");
            }
            out.push_str(&seg);
        }
        out.push('\n');
        if p.fmt.list == ListKind::None {
            out.push('\n');
        }
    }
    out
}

pub fn from_markdown(src: &str) -> Document {
    let mut doc = Document { paras: Vec::new(), ..Default::default() };
    let styles = named_styles();
    for raw_line in src.replace("\r\n", "\n").lines() {
        let trimmed = raw_line.trim_end();
        if trimmed == "---" || trimmed == "***" {
            let mut p = Para::new(CharStyle::default(), ParaFmt::default());
            p.fmt.hr = true;
            doc.paras.push(p);
            continue;
        }
        let mut fmt = ParaFmt::default();
        let mut base = CharStyle::default();
        let mut text = trimmed;
        if let Some(rest) = text.strip_prefix("### ") {
            fmt = ParaFmt { style: 6, ..styles[6].fmt };
            base = styles[6].chr;
            text = rest;
        } else if let Some(rest) = text.strip_prefix("## ") {
            fmt = ParaFmt { style: 5, ..styles[5].fmt };
            base = styles[5].chr;
            text = rest;
        } else if let Some(rest) = text.strip_prefix("# ") {
            fmt = ParaFmt { style: 4, ..styles[4].fmt };
            base = styles[4].chr;
            text = rest;
        } else if let Some(rest) = text.strip_prefix("> ") {
            fmt = ParaFmt { style: 7, ..styles[7].fmt };
            base = styles[7].chr;
            text = rest;
        } else if let Some(rest) = text.strip_prefix("- [x] ").or_else(|| text.strip_prefix("- [X] ")) {
            fmt.list = ListKind::Checklist;
            fmt.checked = true;
            text = rest;
        } else if let Some(rest) = text.strip_prefix("- [ ] ").or_else(|| text.strip_prefix("[] ")) {
            fmt.list = ListKind::Checklist;
            fmt.checked = false;
            text = rest;
        } else if let Some(rest) = text.strip_prefix("- ").or_else(|| text.strip_prefix("* ")) {
            fmt.list = ListKind::Bullet;
            text = rest;
        } else if let Some(dot) = text.find(". ") {
            if dot > 0 && text[..dot].chars().all(|c| c.is_ascii_digit()) {
                fmt.list = ListKind::Number;
                text = &text[dot + 2..];
            }
        }
        let mut p = Para::new(base, fmt);
        for ch in text.chars() {
            p.text.push(ch);
            p.styles.push(base);
        }
        doc.paras.push(p);
    }
    if doc.paras.is_empty() {
        doc.paras.push(Para::new(CharStyle::default(), ParaFmt::default()));
    }
    doc
}

pub fn to_html(doc: &Document) -> String {
    let mut out = String::from("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>");
    out.push_str(if doc.info.title.is_empty() { "Document" } else { &doc.info.title });
    out.push_str("</title><style>body{max-width:816px;margin:40px auto;padding:0 48px;font-family:Calibri,sans-serif;line-height:1.4;color:#1f1f1f;}</style></head><body>\n");
    for p in &doc.paras {
        if p.fmt.hr {
            out.push_str("<hr>\n");
            continue;
        }
        let align = match p.fmt.align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "justify",
        };
        out.push_str(&format!("<p style=\"text-align:{align};\">"));
        if p.fmt.list == ListKind::Checklist {
            out.push_str(if p.fmt.checked { "☑ " } else { "☐ " });
        } else if p.fmt.list == ListKind::Bullet {
            out.push_str("• ");
        }
        for (a, b, s) in p.runs() {
            let txt: String = p.text[a..b]
                .iter()
                .map(|c| match c {
                    '&' => "&amp;".to_string(),
                    '<' => "&lt;".to_string(),
                    '>' => "&gt;".to_string(),
                    _ => c.to_string(),
                })
                .collect();
            let mut span = txt;
            if s.bold {
                span = format!("<b>{span}</b>");
            }
            if s.italic {
                span = format!("<i>{span}</i>");
            }
            if s.underline {
                span = format!("<u>{span}</u>");
            }
            if s.strike {
                span = format!("<s>{span}</s>");
            }
            if let Some(c2) = s.color_to {
                if c2 != s.color {
                    let c1 = s.color;
                    span = format!(
                        "<span style=\"background:linear-gradient(90deg,rgb({},{},{}),rgb({},{},{}));-webkit-background-clip:text;-webkit-text-fill-color:transparent;\">{span}</span>",
                        c1[0], c1[1], c1[2], c2[0], c2[1], c2[2]
                    );
                }
            }
            if let (Some(c1), Some(c2)) = (s.highlight, s.highlight_to) {
                if c1 != c2 {
                    span = format!(
                        "<mark style=\"background:linear-gradient(90deg,rgb({},{},{}),rgb({},{},{}));border-radius:3px;padding:0 2px;\">{span}</mark>",
                        c1[0], c1[1], c1[2], c2[0], c2[1], c2[2]
                    );
                } else {
                    span = format!("<mark style=\"background-color:rgb({},{},{});\">{span}</mark>", c1[0], c1[1], c1[2]);
                }
            } else if let Some(c1) = s.highlight {
                span = format!("<mark style=\"background-color:rgb({},{},{});\">{span}</mark>", c1[0], c1[1], c1[2]);
            }
            if let Some(url) = doc.link_url(s.link) {
                span = format!("<a href=\"{url}\">{span}</a>");
            }
            out.push_str(&span);
        }
        out.push_str("</p>\n");
    }
    out.push_str("</body></html>\n");
    out
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
    let mut out = format!("{{\\rtf1\\ansi\\deff{DEFAULT_FONT}{{\\fonttbl");
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
            ListKind::Checklist => out.push_str(if p.fmt.checked { "\\u9745?\\tab " } else { "\\u9744?\\tab " }),
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
                    "f" => st.font = *fonts.get(&num.unwrap_or(0)).unwrap_or(&DEFAULT_FONT),
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
    if doc.paras.len() > 1 && doc.paras.last().is_some_and(|p| p.text.is_empty()) {
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
    #[test]
    fn gdoc_roundtrip_and_api_parse() {
        let mut d = Document::default();
        d.info.title = "Roadmap".into();
        d.insert_text(Pos::new(0, 0), "Google Docs Title\nSecond line", CharStyle { bold: true, ..Default::default() });
        let g = to_gdoc(&d);
        let back = from_gdoc(&g).unwrap();
        assert_eq!(back.plain_text(), d.plain_text());
        assert_eq!(back.info.title, "Roadmap");
        assert!(back.paras[0].styles[0].bold);

        // Also test pure Google Docs REST API v1 JSON without `paras`
        let mut v: serde_json::Value = serde_json::from_str(&g).unwrap();
        v.as_object_mut().unwrap().remove("paras");
        let api_back = from_gdoc(&v.to_string()).unwrap();
        assert_eq!(api_back.plain_text(), d.plain_text());
        assert!(api_back.paras[0].styles[0].bold);

        // Also test Google Drive shortcut .gdoc
        let shortcut = r#"{"url": "https://docs.google.com/open?id=123", "doc_id": "123", "email": "user@example.com"}"#;
        let sc_doc = from_gdoc(shortcut).unwrap();
        assert!(sc_doc.plain_text().contains("https://docs.google.com/open?id=123"));
        assert_eq!(sc_doc.links.len(), 1);
    }
}

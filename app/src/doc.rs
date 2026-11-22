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

pub fn font_count() -> usize {
    crate::fonts::catalog().len()
}

pub fn font_name(i: u8) -> &'static str {
    let cat = crate::fonts::catalog();
    cat.get(i as usize).map_or(FONT_NAMES[DEFAULT_FONT as usize], |e| e.name)
}

pub fn font_category(i: u8) -> FontCat {
    let cat = crate::fonts::catalog();
    cat.get(i as usize).map_or(FontCat::Sans, |e| e.cat)
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
    let mut cleaned = name.trim().trim_matches(|c| c == '"' || c == '\'' || c == ';').trim();
    if cleaned.len() > 7 && cleaned.as_bytes()[6] == b'+' && cleaned[..6].bytes().all(|b| b.is_ascii_uppercase()) {
        cleaned = &cleaned[7..];
    }
    let cat = crate::fonts::catalog();
    if let Some(i) = cat.iter().position(|e| e.name.eq_ignore_ascii_case(cleaned)) {
        return i as u8;
    }
    // Try stripping common weight/style suffixes (e.g. "Cambria Math" -> "Cambria", "Arial MT" -> "Arial")
    for suffix in [
        " Regular",
        " Light",
        " Semilight",
        " SemiLight",
        " Semibold",
        " SemiBold",
        " Demi",
        " Demibold",
        " Medium",
        " Black",
        " Heavy",
        " Narrow",
        " Condensed",
        " Display",
        " Text",
        " Math",
        " MT",
        " PS",
        " PSMT",
        " Roman",
    ] {
        if cleaned.len() > suffix.len() && cleaned[cleaned.len() - suffix.len()..].eq_ignore_ascii_case(suffix) {
            let base = cleaned[..cleaned.len() - suffix.len()].trim();
            if let Some(i) = cat.iter().position(|e| e.name.eq_ignore_ascii_case(base)) {
                return i as u8;
            }
        }
    }
    let l = cleaned.to_ascii_lowercase();
    match l.replace([' ', '-', '_'], "").as_str() {
        "times" | "timesnewroman" | "timesnewromanps" | "timesnewromanpsmt" => return 0,
        "helvetica" | "helveticaneue" | "arial" | "arialmt" => return 1,
        "aptos" | "aptosdisplay" | "aptosnarrow" | "calibri" | "calibrilight" => return 2,
        "cambria" | "cambriamath" => return 3,
        "georgia" => return 4,
        "garamond" => return 5,
        "palatino" | "palatinolinotype" | "bookantiqua" => return 7,
        "segoeui" | "segoeuilight" | "segoeuisemibold" | "lucidagrande" => return 8,
        "tahoma" => return 9,
        "trebuchet" | "trebuchetms" => return 10,
        "verdana" => return 11,
        "courier" | "couriernew" | "couriernewpsmt" => return 12,
        "consolas" => return 13,
        _ => {}
    }
    // Fallback rule: fixed pitch → Courier New, sans-serif → Arial, script → Caveat, else → Times New Roman.
    if l.contains("mono") || l.contains("courier") || l.contains("console") || l.contains("code") || l.contains("typewriter") {
        12
    } else if l.contains("sans")
        || l.contains("helvet")
        || l.contains("swiss")
        || l.contains("gothic")
        || l.contains("grotesk")
        || l.contains("aptos")
        || l.contains("calibri")
        || l.contains("segoe")
        || l.contains("tahoma")
        || l.contains("verdana")
        || l.contains("arial")
        || l.contains("corbel")
        || l.contains("candara")
    {
        1
    } else if l.contains("script") || l.contains("hand") || l.contains("cursive") || l.contains("brush") {
        29
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
pub struct BorderSpec {
    pub width_pt: f32,
    pub color: [u8; 3],
}

impl Default for BorderSpec {
    fn default() -> Self {
        Self { width_pt: 0.5, color: [0, 0, 0] }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct CellBorders {
    pub top: Option<BorderSpec>,
    pub bottom: Option<BorderSpec>,
    pub left: Option<BorderSpec>,
    pub right: Option<BorderSpec>,
}

impl CellBorders {
    pub fn all(spec: BorderSpec) -> Self {
        Self {
            top: Some(spec),
            bottom: Some(spec),
            left: Some(spec),
            right: Some(spec),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum CellVAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

fn default_grid_span() -> u16 {
    1
}

fn default_cell_pad_x() -> f32 {
    0.075
}

fn default_cell_wrap() -> bool {
    true
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum TableMathOp {
    #[default]
    Sum,
    Subtotal,
    Average,
    Median,
}

impl TableMathOp {
    pub fn name(self) -> &'static str {
        match self {
            TableMathOp::Sum => "SUM",
            TableMathOp::Subtotal => "SUBTOTAL",
            TableMathOp::Average => "AVERAGE",
            TableMathOp::Median => "MEDIAN",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TableMathOp::Sum => "Sum (Total)",
            TableMathOp::Subtotal => "Subtotal",
            TableMathOp::Average => "Average",
            TableMathOp::Median => "Median",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedCellNumber {
    pub value: f64,
    pub decimal_places: usize,
    pub sig_figs: usize,
    pub currency: Option<char>,
}

/// Converts a 0-based `(row, col)` table coordinate into spreadsheet notation (e.g. `(0, 0)` -> `"A1"`).
pub fn cell_coord_label(row: usize, col: usize) -> String {
    let mut c = col;
    let mut col_str = String::new();
    loop {
        col_str.insert(0, (b'A' + (c % 26) as u8) as char);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    format!("{col_str}{}", row + 1)
}

/// Parses spreadsheet coordinate notation (e.g. `"B3"`) into 0-based `(row, col)`.
pub fn parse_cell_coord(s: &str) -> Option<(usize, usize)> {
    let s = s.trim();
    if s.len() < 2 {
        return None;
    }
    let split = s.find(|c: char| c.is_ascii_digit())?;
    if split == 0 {
        return None;
    }
    let col_part = &s[..split];
    let row_part = &s[split..];
    if !col_part.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut col = 0usize;
    for ch in col_part.chars() {
        col = col * 26 + (ch.to_ascii_uppercase() as u8 - b'A' + 1) as usize;
    }
    let row_1based: usize = row_part.parse().ok()?;
    if row_1based == 0 || col == 0 {
        return None;
    }
    Some((row_1based - 1, col - 1))
}

/// Parses a numeric value from a table cell's text along with its decimal precision and significant figures.
pub fn parse_cell_number(text: &str) -> Option<ParsedCellNumber> {
    let mut s = text.trim();
    if s.is_empty() || s.starts_with('=') {
        return None;
    }
    // Allow optional label prefix before a colon (e.g. "Subtotal: 15.23" or "Total: $10.50")
    if let Some((_lbl, rhs)) = s.rsplit_once(':') {
        let rhs_trim = rhs.trim();
        if !rhs_trim.is_empty() && rhs_trim.chars().any(|c| c.is_ascii_digit()) {
            s = rhs_trim;
        }
    }
    let mut currency = None;
    let mut neg = false;
    for _ in 0..3 {
        s = s.trim_start();
        if let Some(rest) = s.strip_prefix('-') {
            neg = !neg;
            s = rest;
        } else if let Some(rest) = s.strip_prefix('+') {
            s = rest;
        } else if let Some(ch) = s.chars().next() {
            if matches!(ch, '$' | '€' | '£' | '¥') {
                currency = Some(ch);
                s = &s[ch.len_utf8()..];
            } else {
                break;
            }
        }
    }
    s = s.trim_end();
    if let Some(rest) = s.strip_suffix('%') {
        s = rest.trim_end();
    }
    let cleaned: String = s.chars().filter(|&c| c != ',' && c != '_').collect();
    if cleaned.is_empty() || !cleaned.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let raw_val: f64 = cleaned.parse().ok()?;
    let value = if neg { -raw_val } else { raw_val };
    let decimal_places = cleaned.split_once('.').map_or(0, |(_, frac)| frac.len());
    let digits_only: String = cleaned.chars().filter(|c| c.is_ascii_digit()).collect();
    let trimmed_leading = digits_only.trim_start_matches('0');
    let sig_figs = trimmed_leading.len().max(1);
    Some(ParsedCellNumber {
        value,
        decimal_places,
        sig_figs,
        currency,
    })
}

/// Formats a calculated table cell result (`val`), preserving the maximum decimal precision / significant figures
/// of the chosen input cells when `keep_sig_figs` is true (e.g. `10` + `5.23` -> `15.23`).
pub fn format_table_math_value(val: f64, inputs: &[ParsedCellNumber], keep_sig_figs: bool) -> String {
    if inputs.is_empty() || !val.is_finite() {
        return "0".to_string();
    }
    let curr = inputs.iter().find_map(|x| x.currency);
    let num_str = if keep_sig_figs {
        let max_dec = inputs.iter().map(|x| x.decimal_places).max().unwrap_or(0);
        let max_sf = inputs.iter().map(|x| x.sig_figs).max().unwrap_or(1);
        if max_dec > 0 {
            let factor = 10f64.powi(max_dec as i32);
            let rounded = (val * factor).round() / factor;
            let clean = if rounded.abs() < 1e-12 { 0.0 } else { rounded };
            format!("{clean:.max_dec$}")
        } else if (val - val.round()).abs() < 1e-9 {
            format!("{:.0}", val.round())
        } else {
            let abs_v = val.abs().max(1e-12);
            let mag = abs_v.log10().floor() as i32;
            let sf = max_sf.max(2) as i32;
            let dec = (sf - 1 - mag).clamp(0, 8) as usize;
            let factor = 10f64.powi(dec as i32);
            let rounded = (val * factor).round() / factor;
            format!("{rounded:.dec$}")
        }
    } else {
        let factor = 10_000.0;
        let rounded = (val * factor).round() / factor;
        if (rounded - rounded.round()).abs() < 1e-9 {
            format!("{:.0}", rounded.round())
        } else {
            let s = format!("{rounded:.4}");
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        }
    };
    if let Some(c) = curr {
        if let Some(rest) = num_str.strip_prefix('-') {
            format!("-{c}{rest}")
        } else {
            format!("{c}{num_str}")
        }
    } else {
        num_str
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableCellSpec {
    pub width_in: f32,
    #[serde(default = "default_grid_span")]
    pub grid_span: u16,
    #[serde(default)]
    pub shading: Option<[u8; 3]>,
    #[serde(default)]
    pub borders: CellBorders,
    #[serde(default)]
    pub v_align: CellVAlign,
    #[serde(default = "default_cell_pad_x")]
    pub pad_left_in: f32,
    #[serde(default = "default_cell_pad_x")]
    pub pad_right_in: f32,
    #[serde(default)]
    pub pad_top_in: f32,
    #[serde(default)]
    pub pad_bottom_in: f32,
    /// Optional cell formula (e.g. `"=SUM(ABOVE)"`, `"=SUBTOTAL(B1:B3)"`, `"=AVERAGE(B1,B2)"`, `"=MEDIAN(ABOVE)"`).
    #[serde(default)]
    pub formula: Option<String>,
    /// Whether text inside this cell wraps to the cell's inner width (`true` by default).
    #[serde(default = "default_cell_wrap")]
    pub wrap_text: bool,
}

impl Default for TableCellSpec {
    fn default() -> Self {
        Self {
            width_in: 2.0,
            grid_span: 1,
            shading: None,
            borders: CellBorders::all(BorderSpec::default()),
            v_align: CellVAlign::Top,
            pad_left_in: 0.075,
            pad_right_in: 0.075,
            pad_top_in: 0.0,
            pad_bottom_in: 0.0,
            formula: None,
            wrap_text: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct TableRowSpec {
    pub cells: Vec<TableCellSpec>,
    #[serde(default)]
    pub min_height_pt: f32,
    #[serde(default)]
    pub grid_after: u16,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct DocTable {
    pub id: u32,
    pub grid_cols_in: Vec<f32>,
    pub rows: Vec<TableRowSpec>,
    #[serde(default)]
    pub indent_in: f32,
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
    pub hr_color: Option<[u8; 3]>,
    #[serde(default)]
    pub hr_width_pt: f32,
    #[serde(default)]
    pub shading: Option<[u8; 3]>,
    /// 0 = normal text paragraph; >= 1 = `DocImage::id` embedded in this paragraph block.
    #[serde(default)]
    pub image_id: u64,
    /// 0 = normal paragraph; >= 1 = `DocTable::id` that this paragraph belongs to.
    #[serde(default)]
    pub table_id: u32,
    #[serde(default)]
    pub table_row: u16,
    #[serde(default)]
    pub table_col: u16,
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
            hr_color: None,
            hr_width_pt: 0.0,
            shading: None,
            image_id: 0,
            table_id: 0,
            table_row: 0,
            table_col: 0,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocImage {
    pub id: u64,
    pub name: String,
    pub natural_size: [u32; 2],
    pub width_in: f32,
    pub height_in: f32,
    #[serde(default)]
    pub caption: String,
    pub png_b64: String,
    #[serde(default)]
    pub seamless: bool,
}

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(B64_ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_ALPHABET[(n & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |b: u8| -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let clean: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if clean.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        if chunk.len() < 2 {
            return None;
        }
        let v0 = val(chunk[0])? as u32;
        let v1 = val(chunk[1])? as u32;
        let c2 = chunk.get(2).copied().unwrap_or(b'=');
        let c3 = chunk.get(3).copied().unwrap_or(b'=');
        let v2 = if c2 == b'=' { 0 } else { val(c2)? as u32 };
        let v3 = if c3 == b'=' { 0 } else { val(c3)? as u32 };
        let n = (v0 << 18) | (v1 << 12) | (v2 << 6) | v3;
        out.push(((n >> 16) & 0xff) as u8);
        if c2 != b'=' {
            out.push(((n >> 8) & 0xff) as u8);
        }
        if c3 != b'=' {
            out.push((n & 0xff) as u8);
        }
    }
    Some(out)
}

impl DocImage {
    /// Decodes an image file (PNG, JPEG, BMP, ICO, TGA) from raw bytes and creates a `DocImage`
    /// scaled to fit comfortably within `max_width_in` inches while preserving aspect ratio.
    pub fn from_file_bytes(id: u64, name: String, bytes: &[u8], max_width_in: f32) -> Result<Self, String> {
        let dyn_img = image::load_from_memory(bytes).map_err(|e| format!("Unsupported or corrupt image: {e}"))?;
        let w = dyn_img.width().max(1);
        let h = dyn_img.height().max(1);
        let mut png_bytes = Vec::new();
        dyn_img
            .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .map_err(|e| format!("Failed to encode image: {e}"))?;
        let png_b64 = base64_encode(&png_bytes);
        let limit_w = max_width_in.clamp(1.5, 6.5).min(4.6);
        let raw_w = (w as f32 / 96.0).clamp(1.2, limit_w);
        let aspect = h as f32 / w as f32;
        let width_in = raw_w;
        let height_in = (width_in * aspect).clamp(0.4, 8.5);
        Ok(Self {
            id,
            name,
            natural_size: [w, h],
            width_in,
            height_in,
            caption: String::new(),
            png_b64,
            seamless: false,
        })
    }

    /// Creates a `DocImage` preserving exact physical dimensions `(width_in, height_in)` in inches.
    pub fn from_exact_bytes(
        id: u64,
        name: String,
        bytes: &[u8],
        width_in: f32,
        height_in: f32,
        seamless: bool,
    ) -> Result<Self, String> {
        let dyn_img = image::load_from_memory(bytes).map_err(|e| format!("Unsupported or corrupt image: {e}"))?;
        let w = dyn_img.width().max(1);
        let h = dyn_img.height().max(1);
        let mut png_bytes = Vec::new();
        dyn_img
            .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .map_err(|e| format!("Failed to encode image: {e}"))?;
        Ok(Self {
            id,
            name,
            natural_size: [w, h],
            width_in: width_in.clamp(0.06, 10.5),
            height_in: height_in.clamp(0.05, 13.5),
            caption: String::new(),
            png_b64: base64_encode(&png_bytes),
            seamless,
        })
    }

    pub fn aspect_ratio(&self) -> f32 {
        self.natural_size[0].max(1) as f32 / self.natural_size[1].max(1) as f32
    }

    pub fn decode_rgba(&self) -> Option<(u32, u32, Vec<u8>)> {
        let png_bytes = base64_decode(&self.png_b64)?;
        let img = image::load_from_memory(&png_bytes).ok()?.to_rgba8();
        let (w, h) = (img.width(), img.height());
        Some((w, h, img.into_raw()))
    }
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FooterRun {
    pub text: String,
    pub style: CharStyle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FooterSegment {
    pub runs: Vec<FooterRun>,
    /// Horizontal start X in PDF points from the left edge of the page.
    pub x_pt: f32,
    /// Horizontal end X in PDF points from the left edge of the page.
    pub max_x_pt: f32,
    /// Typographic baseline distance from the bottom edge of the page in PDF points.
    pub bottom_pt: f32,
    /// Horizontal anchor (`Left` anchors at `x_pt`, `Center` at midpoint, `Right` at `max_x_pt`).
    pub align: Align,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PageFooter {
    pub segments: Vec<FooterSegment>,
    /// Optional horizontal divider rule above the footer: `(x0_pt, x1_pt, y_from_bottom_pt, width_pt, color)`.
    #[serde(default)]
    pub rule: Option<(f32, f32, f32, f32, [u8; 3])>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub paras: Vec<Para>,
    pub page: PageSetup,
    pub header: String,
    pub footer: String,
    pub header_align: Align,
    pub footer_align: Align,
    pub header_italic: bool,
    pub footer_italic: bool,
    pub page_footers: Vec<PageFooter>,
    pub info: DocInfo,
    pub links: Vec<String>,
    pub comments: Vec<DocComment>,
    pub bookmarks: Vec<(String, usize)>,
    pub summary: String,
    pub starred: bool,
    pub pageless: bool,
    pub citations: Vec<CitationEntry>,
    pub images: Vec<DocImage>,
    pub tables: Vec<DocTable>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            paras: vec![Para::new(CharStyle::default(), ParaFmt::default())],
            page: PageSetup::default(),
            header: String::new(),
            footer: String::new(),
            header_align: Align::Center,
            footer_align: Align::Center,
            header_italic: false,
            footer_italic: false,
            page_footers: Vec::new(),
            info: DocInfo::default(),
            links: Vec::new(),
            comments: Vec::new(),
            bookmarks: Vec::new(),
            summary: String::new(),
            starred: false,
            pageless: false,
            citations: Vec::new(),
            images: Vec::new(),
            tables: Vec::new(),
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

    pub fn image(&self, id: u64) -> Option<&DocImage> {
        if id == 0 {
            None
        } else {
            self.images.iter().find(|img| img.id == id)
        }
    }

    pub fn image_mut(&mut self, id: u64) -> Option<&mut DocImage> {
        if id == 0 {
            None
        } else {
            self.images.iter_mut().find(|img| img.id == id)
        }
    }

    pub fn next_image_id(&self) -> u64 {
        self.images.iter().map(|i| i.id).max().unwrap_or(0) + 1
    }

    pub fn table(&self, id: u32) -> Option<&DocTable> {
        if id == 0 {
            None
        } else {
            self.tables.iter().find(|t| t.id == id)
        }
    }

    pub fn table_mut(&mut self, id: u32) -> Option<&mut DocTable> {
        if id == 0 {
            None
        } else {
            self.tables.iter_mut().find(|t| t.id == id)
        }
    }

    pub fn next_table_id(&self) -> u32 {
        self.tables.iter().map(|t| t.id).max().unwrap_or(0) + 1
    }

    /// Inserts a `rows × cols` table at `pos` and returns the position at the first cell `(0, 0)`.
    pub fn insert_table(&mut self, pos: Pos, rows: usize, cols: usize) -> Pos {
        let rows = rows.clamp(1, 64);
        let cols = cols.clamp(1, 16);
        let pos = self.clamp(pos);
        let avail_w = (self.page.width - self.page.margin_left - self.page.margin_right).max(2.0);
        let col_w = ((avail_w / cols as f32) * 1440.0).round() / 1440.0;
        let tid = self.next_table_id();
        let grid_cols_in = vec![col_w; cols];
        let mut table_rows = Vec::with_capacity(rows);
        for _ in 0..rows {
            let mut cells = Vec::with_capacity(cols);
            for _ in 0..cols {
                cells.push(TableCellSpec {
                    width_in: col_w,
                    grid_span: 1,
                    shading: None,
                    borders: CellBorders::all(BorderSpec::default()),
                    v_align: CellVAlign::Top,
                    pad_left_in: 0.075,
                    pad_right_in: 0.075,
                    pad_top_in: 0.025,
                    pad_bottom_in: 0.025,
                    formula: None,
                    wrap_text: true,
                });
            }
            table_rows.push(TableRowSpec {
                cells,
                min_height_pt: 26.0,
                grid_after: 0,
            });
        }
        self.tables.push(DocTable {
            id: tid,
            grid_cols_in,
            rows: table_rows,
            indent_in: 0.0,
        });

        let insert_idx = if self.paras[pos.p].text.is_empty() && self.paras[pos.p].fmt.table_id == 0 && self.paras[pos.p].fmt.image_id == 0 {
            pos.p
        } else {
            pos.p + 1
        };
        let mut cell_paras = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            for c in 0..cols {
                let fmt = ParaFmt {
                    table_id: tid,
                    table_row: r as u16,
                    table_col: c as u16,
                    ..ParaFmt::default()
                };
                cell_paras.push(Para::new(CharStyle::default(), fmt));
            }
        }
        if insert_idx == pos.p && self.paras[pos.p].text.is_empty() && self.paras[pos.p].fmt.table_id == 0 {
            self.paras.splice(pos.p..=pos.p, cell_paras);
        } else {
            self.paras.splice(insert_idx..insert_idx, cell_paras);
        }
        // Ensure a trailing non-table paragraph after the table
        let after_idx = insert_idx + rows * cols;
        if after_idx >= self.paras.len() || self.paras[after_idx].fmt.table_id != 0 {
            self.paras.insert(after_idx, Para::new(CharStyle::default(), ParaFmt::default()));
        }
        Pos::new(insert_idx, 0)
    }

    /// Returns the combined text of all paragraphs in `(table_id, row, col)`.
    pub fn cell_text(&self, table_id: u32, row: usize, col: usize) -> String {
        let mut parts = Vec::new();
        for p in &self.paras {
            if p.fmt.table_id == table_id && p.fmt.table_row as usize == row && p.fmt.table_col as usize == col {
                parts.push(p.string());
            }
        }
        parts.join("\n")
    }

    /// Replaces the text of `(table_id, row, col)` while preserving its character style and paragraph formatting.
    pub fn set_cell_text(&mut self, table_id: u32, row: usize, col: usize, text: &str) {
        let mut first_idx = None;
        let mut extra_indices = Vec::new();
        for (pi, p) in self.paras.iter().enumerate() {
            if p.fmt.table_id == table_id && p.fmt.table_row as usize == row && p.fmt.table_col as usize == col {
                if first_idx.is_none() {
                    first_idx = Some(pi);
                } else {
                    extra_indices.push(pi);
                }
            }
        }
        for pi in extra_indices.into_iter().rev() {
            self.paras.remove(pi);
        }
        if let Some(pi) = first_idx {
            let st = self.paras[pi].styles.first().copied().unwrap_or(self.paras[pi].end_style);
            self.paras[pi].text = text.chars().collect();
            self.paras[pi].styles = vec![st; self.paras[pi].text.len()];
            self.paras[pi].end_style = st;
        }
    }

    /// Sets or clears the background shading for a single cell `(table_id, row, col)`.
    pub fn table_set_cell_shading(&mut self, table_id: u32, row: usize, col: usize, shading: Option<[u8; 3]>) {
        if let Some(tbl) = self.table_mut(table_id) {
            if let Some(r) = tbl.rows.get_mut(row) {
                if let Some(c) = r.cells.get_mut(col) {
                    c.shading = shading;
                }
            }
        }
    }

    /// Sets or clears the background shading for an entire row `(table_id, row)`.
    pub fn table_set_row_shading(&mut self, table_id: u32, row: usize, shading: Option<[u8; 3]>) {
        if let Some(tbl) = self.table_mut(table_id) {
            if let Some(r) = tbl.rows.get_mut(row) {
                for c in &mut r.cells {
                    c.shading = shading;
                }
            }
        }
    }

    /// Sets the vertical alignment (`Top`, `Center`, `Bottom`) for `(table_id, row, col)`.
    pub fn table_set_cell_valign(&mut self, table_id: u32, row: usize, col: usize, v_align: CellVAlign) {
        if let Some(tbl) = self.table_mut(table_id) {
            if let Some(r) = tbl.rows.get_mut(row) {
                if let Some(c) = r.cells.get_mut(col) {
                    c.v_align = v_align;
                }
            }
        }
    }

    /// Sets the horizontal alignment (`Left`, `Center`, `Right`, `Justify`) for all paragraphs in `(table_id, row, col)`.
    pub fn table_set_cell_halign(&mut self, table_id: u32, row: usize, col: usize, align: Align) {
        for p in &mut self.paras {
            if p.fmt.table_id == table_id && p.fmt.table_row as usize == row && p.fmt.table_col as usize == col {
                p.fmt.align = align;
            }
        }
    }

    /// Sets whether text wraps inside `(table_id, row, col)`.
    pub fn table_set_cell_wrap(&mut self, table_id: u32, row: usize, col: usize, wrap: bool) {
        if let Some(tbl) = self.table_mut(table_id) {
            if let Some(r) = tbl.rows.get_mut(row) {
                if let Some(c) = r.cells.get_mut(col) {
                    c.wrap_text = wrap;
                }
            }
        }
    }

    /// Resizes a cell `(row, col)` (or its spanned grid column) in `table_id` to `new_width_in` inches,
    /// updating `grid_cols_in` and syncing all rows' cell widths accordingly.
    pub fn table_resize_col(&mut self, table_id: u32, row: usize, col: usize, new_width_in: f32) {
        let Some(tbl) = self.table_mut(table_id) else { return };
        let new_w = ((new_width_in.clamp(0.35, 16.0) * 1440.0).round()) / 1440.0;
        // Ensure grid_cols_in is populated
        if tbl.grid_cols_in.is_empty() {
            if let Some(first_row) = tbl.rows.first() {
                for c in &first_row.cells {
                    let sp = c.grid_span.max(1) as usize;
                    for _ in 0..sp {
                        tbl.grid_cols_in.push((c.width_in / sp as f32).max(0.25));
                    }
                }
            }
        }
        // Determine which grid columns `(row, col)` spans
        let (g_start, span) = tbl
            .rows
            .get(row)
            .map_or((col, 1usize), |r| {
                let gs: usize = r.cells.iter().take(col).map(|c| c.grid_span.max(1) as usize).sum();
                let sp = r.cells.get(col).map_or(1, |c| c.grid_span.max(1) as usize);
                (gs, sp)
            });
        let target_grid = (g_start + span - 1).min(tbl.grid_cols_in.len().saturating_sub(1));
        if span <= 1 {
            if let Some(gw) = tbl.grid_cols_in.get_mut(target_grid) {
                *gw = new_w;
            }
        } else if target_grid < tbl.grid_cols_in.len() {
            let prefix_sum: f32 = tbl.grid_cols_in[g_start..target_grid].iter().sum();
            tbl.grid_cols_in[target_grid] = (new_w - prefix_sum).max(0.25);
        }
        // Sync every row's cell widths from grid_cols_in
        let grid = tbl.grid_cols_in.clone();
        for r in &mut tbl.rows {
            let mut g_cur = 0usize;
            for c in &mut r.cells {
                let sp = c.grid_span.max(1) as usize;
                let g_end = (g_cur + sp).min(grid.len());
                if g_end > g_cur {
                    let w_sum: f32 = grid[g_cur..g_end].iter().sum();
                    c.width_in = ((w_sum * 1440.0).round()) / 1440.0;
                }
                g_cur += sp;
            }
        }
    }

    /// Resizes `row` in `table_id` to a minimum height in points (`0.0` auto-fits tightly to cell contents).
    pub fn table_resize_row(&mut self, table_id: u32, row: usize, new_min_height_pt: f32) {
        if let Some(tbl) = self.table_mut(table_id) {
            if let Some(r) = tbl.rows.get_mut(row) {
                r.min_height_pt = ((new_min_height_pt.clamp(0.0, 720.0) * 20.0).round()) / 20.0;
            }
        }
    }

    /// Returns all `(table_id, row, col)` coordinates in row `row` of `table_id`.
    pub fn table_row_cells(&self, table_id: u32, row: usize) -> Vec<(u32, u16, u16)> {
        let Some(tbl) = self.table(table_id) else { return Vec::new() };
        let Some(r) = tbl.rows.get(row) else { return Vec::new() };
        (0..r.cells.len()).map(|c| (table_id, row as u16, c as u16)).collect()
    }

    /// Returns all `(table_id, row, col)` coordinates in column `col` of `table_id`.
    pub fn table_col_cells(&self, table_id: u32, col: usize) -> Vec<(u32, u16, u16)> {
        let Some(tbl) = self.table(table_id) else { return Vec::new() };
        let mut out = Vec::with_capacity(tbl.rows.len());
        for (r_idx, r) in tbl.rows.iter().enumerate() {
            if col < r.cells.len() {
                out.push((table_id, r_idx as u16, col as u16));
            }
        }
        out
    }

    /// Merges `(table_id, row, col)` with the adjacent cell to its right (`col + 1`).
    pub fn table_merge_right(&mut self, table_id: u32, row: usize, col: usize) -> bool {
        let can_merge = self
            .table(table_id)
            .and_then(|t| t.rows.get(row))
            .is_some_and(|r| col + 1 < r.cells.len());
        if !can_merge {
            return false;
        }
        if let Some(tbl) = self.table_mut(table_id) {
            if let Some(r) = tbl.rows.get_mut(row) {
                let right = r.cells.remove(col + 1);
                r.cells[col].width_in += right.width_in;
                r.cells[col].grid_span = r.cells[col].grid_span.max(1) + right.grid_span.max(1);
                r.cells[col].borders.right = right.borders.right;
            }
        }
        let left_txt = self.cell_text(table_id, row, col);
        let right_txt = self.cell_text(table_id, row, col + 1);
        let mut remove_paras = Vec::new();
        for (pi, p) in self.paras.iter_mut().enumerate() {
            if p.fmt.table_id == table_id && p.fmt.table_row as usize == row {
                let c = p.fmt.table_col as usize;
                if c == col + 1 {
                    if right_txt.trim().is_empty() || left_txt.trim().is_empty() {
                        remove_paras.push(pi);
                    } else {
                        p.fmt.table_col = col as u16;
                    }
                } else if c > col + 1 {
                    p.fmt.table_col -= 1;
                }
            }
        }
        for pi in remove_paras.into_iter().rev() {
            if self.paras.len() > 1 {
                self.paras.remove(pi);
            }
        }
        if left_txt.trim().is_empty() && !right_txt.trim().is_empty() {
            self.set_cell_text(table_id, row, col, &right_txt);
        }
        true
    }

    /// Merges all cells in `(table_id, row)` into a single full-width banner cell.
    pub fn table_merge_row(&mut self, table_id: u32, row: usize) -> bool {
        let mut merged_any = false;
        while self.table_merge_right(table_id, row, 0) {
            merged_any = true;
        }
        merged_any
    }

    /// Splits a merged cell (`grid_span > 1`) at `(table_id, row, col)` back into individual grid cells.
    pub fn table_split_cell(&mut self, table_id: u32, row: usize, col: usize) -> bool {
        let Some(tbl) = self.table_mut(table_id) else { return false };
        let Some(r) = tbl.rows.get_mut(row) else { return false };
        let Some(cell) = r.cells.get(col).cloned() else { return false };
        let span = cell.grid_span as usize;
        if span <= 1 {
            return false;
        }
        let grid_start: usize = r.cells[..col].iter().map(|c| c.grid_span.max(1) as usize).sum();
        let mut split_cells = Vec::with_capacity(span);
        for k in 0..span {
            let w = tbl
                .grid_cols_in
                .get(grid_start + k)
                .copied()
                .unwrap_or(cell.width_in / span as f32);
            split_cells.push(TableCellSpec {
                width_in: w,
                grid_span: 1,
                shading: cell.shading,
                borders: CellBorders::all(BorderSpec::default()),
                v_align: cell.v_align,
                pad_left_in: cell.pad_left_in,
                pad_right_in: cell.pad_right_in,
                pad_top_in: cell.pad_top_in,
                pad_bottom_in: cell.pad_bottom_in,
                formula: if k == 0 { cell.formula.clone() } else { None },
                wrap_text: cell.wrap_text,
            });
        }
        r.cells.splice(col..=col, split_cells);

        let shift = (span - 1) as u16;
        let mut insert_after = 0usize;
        for (pi, p) in self.paras.iter_mut().enumerate() {
            if p.fmt.table_id == table_id && p.fmt.table_row as usize == row {
                if p.fmt.table_col as usize == col {
                    insert_after = pi + 1;
                } else if p.fmt.table_col as usize > col {
                    p.fmt.table_col += shift;
                }
            }
        }
        let mut new_paras = Vec::with_capacity(span - 1);
        for k in 1..span {
            new_paras.push(Para::new(
                CharStyle::default(),
                ParaFmt {
                    table_id,
                    table_row: row as u16,
                    table_col: (col + k) as u16,
                    ..ParaFmt::default()
                },
            ));
        }
        let ins = insert_after.min(self.paras.len());
        self.paras.splice(ins..ins, new_paras);
        true
    }

    /// Inserts a new row above or below `ref_row` in `table_id` and returns the position of the first cell in the new row.
    pub fn table_insert_row(&mut self, table_id: u32, ref_row: usize, above: bool) -> Option<Pos> {
        let (new_row_idx, new_cells_count, template_cells) = {
            let tbl = self.table_mut(table_id)?;
            let idx = if above { ref_row.min(tbl.rows.len()) } else { (ref_row + 1).min(tbl.rows.len()) };
            let tpl = tbl
                .rows
                .get(ref_row.min(tbl.rows.len().saturating_sub(1)))
                .map_or_else(
                    || {
                        tbl.grid_cols_in
                            .iter()
                            .map(|&w| TableCellSpec {
                                width_in: w,
                                ..TableCellSpec::default()
                            })
                            .collect()
                    },
                    |r| {
                        r.cells
                            .iter()
                            .map(|c| TableCellSpec {
                                shading: None,
                                formula: None,
                                borders: CellBorders::all(BorderSpec::default()),
                                ..c.clone()
                            })
                            .collect::<Vec<_>>()
                    },
                );
            let ncols = tpl.len().max(1);
            tbl.rows.insert(
                idx,
                TableRowSpec {
                    cells: tpl.clone(),
                    min_height_pt: 26.0,
                    grid_after: 0,
                },
            );
            (idx, ncols, tpl)
        };
        let _ = template_cells;
        let mut insert_pi = None;
        let mut last_table_pi = 0usize;
        for (pi, p) in self.paras.iter_mut().enumerate() {
            if p.fmt.table_id == table_id {
                last_table_pi = pi + 1;
                if p.fmt.table_row as usize >= new_row_idx {
                    if insert_pi.is_none() {
                        insert_pi = Some(pi);
                    }
                    p.fmt.table_row += 1;
                }
            }
        }
        let target_pi = insert_pi.unwrap_or(last_table_pi).min(self.paras.len());
        let mut new_paras = Vec::with_capacity(new_cells_count);
        for c in 0..new_cells_count {
            new_paras.push(Para::new(
                CharStyle::default(),
                ParaFmt {
                    table_id,
                    table_row: new_row_idx as u16,
                    table_col: c as u16,
                    ..ParaFmt::default()
                },
            ));
        }
        self.paras.splice(target_pi..target_pi, new_paras);
        self.evaluate_table_formulas(table_id);
        Some(Pos::new(target_pi, 0))
    }

    /// Deletes `row` from `table_id` (if the table has more than 1 row).
    pub fn table_delete_row(&mut self, table_id: u32, row: usize) -> Option<Pos> {
        {
            let tbl = self.table_mut(table_id)?;
            if tbl.rows.len() <= 1 || row >= tbl.rows.len() {
                return None;
            }
            tbl.rows.remove(row);
        }
        self.paras
            .retain(|p| !(p.fmt.table_id == table_id && p.fmt.table_row as usize == row));
        let mut fallback_pi = 0usize;
        for (pi, p) in self.paras.iter_mut().enumerate() {
            if p.fmt.table_id == table_id {
                if p.fmt.table_row as usize > row {
                    p.fmt.table_row -= 1;
                }
                if p.fmt.table_row as usize == row.saturating_sub(1) && p.fmt.table_col == 0 {
                    fallback_pi = pi;
                }
            }
        }
        self.evaluate_table_formulas(table_id);
        Some(Pos::new(fallback_pi.min(self.paras.len().saturating_sub(1)), 0))
    }

    /// Inserts a new column to the left or right of `ref_col` in `table_id`.
    pub fn table_insert_col(&mut self, table_id: u32, ref_col: usize, left: bool) -> Option<Pos> {
        let avail_w = (self.page.width - self.page.margin_left - self.page.margin_right).max(2.0);
        let new_col_idx = {
            let tbl = self.table_mut(table_id)?;
            let idx = if left {
                ref_col.min(tbl.grid_cols_in.len())
            } else {
                (ref_col + 1).min(tbl.grid_cols_in.len())
            };
            let new_ncols = tbl.grid_cols_in.len() + 1;
            let col_w = ((avail_w / new_ncols as f32) * 1440.0).round() / 1440.0;
            tbl.grid_cols_in = vec![col_w; new_ncols];
            for r in &mut tbl.rows {
                let c_ins = idx.min(r.cells.len());
                r.cells.insert(
                    c_ins,
                    TableCellSpec {
                        width_in: col_w,
                        grid_span: 1,
                        shading: None,
                        borders: CellBorders::all(BorderSpec::default()),
                        v_align: CellVAlign::Top,
                        pad_left_in: 0.075,
                        pad_right_in: 0.075,
                        pad_top_in: 0.025,
                        pad_bottom_in: 0.025,
                        formula: None,
                        wrap_text: true,
                    },
                );
                let row_cols = r.cells.len().max(1);
                for c in &mut r.cells {
                    c.width_in = (avail_w / row_cols as f32) * (c.grid_span.max(1) as f32);
                }
            }
            idx
        };

        let num_rows = self.table(table_id).map_or(0, |t| t.rows.len());
        for r_idx in 0..num_rows {
            let mut ins_pi = None;
            let mut row_end_pi = 0usize;
            for (pi, p) in self.paras.iter_mut().enumerate() {
                if p.fmt.table_id == table_id && p.fmt.table_row as usize == r_idx {
                    row_end_pi = pi + 1;
                    if p.fmt.table_col as usize >= new_col_idx {
                        if ins_pi.is_none() {
                            ins_pi = Some(pi);
                        }
                        p.fmt.table_col += 1;
                    }
                }
            }
            let target_pi = ins_pi.unwrap_or(row_end_pi).min(self.paras.len());
            self.paras.insert(
                target_pi,
                Para::new(
                    CharStyle::default(),
                    ParaFmt {
                        table_id,
                        table_row: r_idx as u16,
                        table_col: new_col_idx as u16,
                        ..ParaFmt::default()
                    },
                ),
            );
        }
        self.evaluate_table_formulas(table_id);
        let first_pi = self
            .paras
            .iter()
            .position(|p| p.fmt.table_id == table_id && p.fmt.table_col as usize == new_col_idx)
            .unwrap_or(0);
        Some(Pos::new(first_pi, 0))
    }

    /// Deletes column `col` from `table_id` (if the table has more than 1 column).
    pub fn table_delete_col(&mut self, table_id: u32, col: usize) -> Option<Pos> {
        let avail_w = (self.page.width - self.page.margin_left - self.page.margin_right).max(2.0);
        {
            let tbl = self.table_mut(table_id)?;
            if tbl.grid_cols_in.len() <= 1 {
                return None;
            }
            let rm_grid = col.min(tbl.grid_cols_in.len() - 1);
            tbl.grid_cols_in.remove(rm_grid);
            let new_ncols = tbl.grid_cols_in.len().max(1);
            let col_w = ((avail_w / new_ncols as f32) * 1440.0).round() / 1440.0;
            tbl.grid_cols_in = vec![col_w; new_ncols];
            for r in &mut tbl.rows {
                if r.cells.len() > 1 && col < r.cells.len() {
                    r.cells.remove(col);
                }
                let row_cols = r.cells.len().max(1);
                for c in &mut r.cells {
                    c.width_in = avail_w / row_cols as f32;
                }
            }
        }
        self.paras
            .retain(|p| !(p.fmt.table_id == table_id && p.fmt.table_col as usize == col));
        let mut fallback_pi = 0usize;
        for (pi, p) in self.paras.iter_mut().enumerate() {
            if p.fmt.table_id == table_id {
                if p.fmt.table_col as usize > col {
                    p.fmt.table_col -= 1;
                }
                if p.fmt.table_col as usize == col.saturating_sub(1) {
                    fallback_pi = pi;
                }
            }
        }
        self.evaluate_table_formulas(table_id);
        Some(Pos::new(fallback_pi.min(self.paras.len().saturating_sub(1)), 0))
    }

    /// Attaches a math formula (e.g. `"=SUM(ABOVE)"`, `"=AVERAGE(B1,B2)"`) to `(table_id, row, col)` and immediately evaluates it.
    pub fn table_set_cell_formula(&mut self, table_id: u32, row: usize, col: usize, formula: Option<String>) {
        if let Some(tbl) = self.table_mut(table_id) {
            if let Some(r) = tbl.rows.get_mut(row) {
                if let Some(c) = r.cells.get_mut(col) {
                    c.formula = formula.filter(|s| !s.trim().is_empty());
                }
            }
        }
        self.evaluate_table_formulas(table_id);
    }

    /// Evaluates all cell formulas (`SUM`, `SUBTOTAL`, `AVERAGE`, `MEDIAN`) in `table_id` in row-major order.
    /// Also promotes any cell text typed as `=SUM(...)`, `=SUBTOTAL(...)`, `=AVERAGE(...)`, or `=MEDIAN(...)` into `cell.formula`.
    pub fn evaluate_table_formulas(&mut self, table_id: u32) {
        let Some(tbl_snap) = self.table(table_id).cloned() else { return };
        let nrows = tbl_snap.rows.len();

        // First pass: promote any inline typed `=SUM(...)` / `=SUBTOTAL(...)` / `=AVERAGE(...)` / `=MEDIAN(...)` into `cell.formula`
        let mut promoted = Vec::new();
        for r_idx in 0..nrows {
            let ncols = tbl_snap.rows[r_idx].cells.len();
            for c_idx in 0..ncols {
                let raw = self.cell_text(table_id, r_idx, c_idx);
                let trimmed = raw.trim();
                if let Some(rest) = trimmed.strip_prefix('=') {
                    let up = rest.trim_start().to_ascii_uppercase();
                    if up.starts_with("SUM(")
                        || up.starts_with("TOTAL(")
                        || up.starts_with("SUBTOTAL(")
                        || up.starts_with("AVERAGE(")
                        || up.starts_with("AVG(")
                        || up.starts_with("MEDIAN(")
                    {
                        promoted.push((r_idx, c_idx, format!("={}", rest.trim())));
                    }
                }
            }
        }
        if !promoted.is_empty() {
            if let Some(tbl) = self.table_mut(table_id) {
                for (r_idx, c_idx, f_str) in promoted {
                    if let Some(cell) = tbl.rows.get_mut(r_idx).and_then(|r| r.cells.get_mut(c_idx)) {
                        cell.formula = Some(f_str);
                    }
                }
            }
        }

        let Some(tbl_snap) = self.table(table_id).cloned() else { return };
        for r_idx in 0..nrows {
            let ncols = tbl_snap.rows[r_idx].cells.len();
            for c_idx in 0..ncols {
                let Some(formula) = tbl_snap.rows[r_idx].cells[c_idx].formula.as_deref() else {
                    continue;
                };
                if let Some(formatted) = self.eval_single_formula(table_id, &tbl_snap, r_idx, c_idx, formula) {
                    self.set_cell_text(table_id, r_idx, c_idx, &formatted);
                }
            }
        }
    }

    /// Re-evaluates all formula cells in `table_id` except `skip_cell` (used while the user is actively typing in `skip_cell`).
    pub fn evaluate_table_formulas_live(&mut self, table_id: u32, skip_cell: (usize, usize)) {
        let Some(tbl_snap) = self.table(table_id).cloned() else { return };
        let nrows = tbl_snap.rows.len();
        for r_idx in 0..nrows {
            let ncols = tbl_snap.rows[r_idx].cells.len();
            for c_idx in 0..ncols {
                if (r_idx, c_idx) == skip_cell {
                    continue;
                }
                let Some(formula) = tbl_snap.rows[r_idx].cells[c_idx].formula.as_deref() else {
                    continue;
                };
                if let Some(formatted) = self.eval_single_formula(table_id, &tbl_snap, r_idx, c_idx, formula) {
                    self.set_cell_text(table_id, r_idx, c_idx, &formatted);
                }
            }
        }
    }

    /// Computes a live preview of what `formula` would produce in `(table_id, target_row, target_col)`.
    pub fn preview_table_formula(&self, table_id: u32, target_row: usize, target_col: usize, formula: &str) -> Option<String> {
        let tbl = self.table(table_id)?;
        self.eval_single_formula(table_id, tbl, target_row, target_col, formula)
    }

    fn eval_single_formula(
        &self,
        table_id: u32,
        tbl: &DocTable,
        target_row: usize,
        target_col: usize,
        formula: &str,
    ) -> Option<String> {
        let s = formula.trim().strip_prefix('=').unwrap_or(formula.trim()).trim();
        let open_paren = s.find('(')?;
        let close_paren = s.rfind(')')?;
        if close_paren < open_paren {
            return None;
        }
        let op_name = s[..open_paren].trim().to_ascii_uppercase();
        let op = match op_name.as_str() {
            "SUM" | "TOTAL" => TableMathOp::Sum,
            "SUBTOTAL" => TableMathOp::Subtotal,
            "AVERAGE" | "AVG" => TableMathOp::Average,
            "MEDIAN" => TableMathOp::Median,
            _ => return None,
        };
        let inside = s[open_paren + 1..close_paren].trim();
        let (args_str, keep_sig_figs) = if let Some((lhs, rhs)) = inside.rsplit_once(';') {
            let flag = !rhs.trim().eq_ignore_ascii_case("RAW");
            (lhs.trim(), flag)
        } else {
            (inside, true)
        };

        let is_cell_op = |r: usize, c: usize, check_op: &str| -> bool {
            tbl.rows
                .get(r)
                .and_then(|row| row.cells.get(c))
                .and_then(|cell| cell.formula.as_deref())
                .is_some_and(|f| {
                    let clean = f.trim().strip_prefix('=').unwrap_or(f.trim()).trim_start().to_ascii_uppercase();
                    clean.starts_with(check_op)
                })
        };

        let mut coords: Vec<(usize, usize)> = Vec::new();
        for token in args_str.split(',') {
            let tok = token.trim().to_ascii_uppercase();
            if tok.is_empty() {
                continue;
            }
            if tok == "ABOVE" {
                match op {
                    TableMathOp::Subtotal => {
                        let mut r = target_row;
                        let mut section_rows = Vec::new();
                        while r > 0 {
                            r -= 1;
                            if is_cell_op(r, target_col, "SUBTOTAL(") || is_cell_op(r, target_col, "SUM(") {
                                break;
                            }
                            section_rows.push((r, target_col));
                        }
                        section_rows.reverse();
                        coords.extend(section_rows);
                    }
                    TableMathOp::Sum => {
                        let subtotals_above: Vec<(usize, usize)> = (0..target_row)
                            .filter(|&r| is_cell_op(r, target_col, "SUBTOTAL("))
                            .map(|r| (r, target_col))
                            .collect();
                        if !subtotals_above.is_empty() {
                            coords.extend(subtotals_above);
                        } else {
                            for r in 0..target_row {
                                if !is_cell_op(r, target_col, "SUM(") {
                                    coords.push((r, target_col));
                                }
                            }
                        }
                    }
                    TableMathOp::Average | TableMathOp::Median => {
                        for r in 0..target_row {
                            if !is_cell_op(r, target_col, "SUBTOTAL(") && !is_cell_op(r, target_col, "SUM(") {
                                coords.push((r, target_col));
                            }
                        }
                    }
                }
            } else if tok == "LEFT" {
                for c in 0..target_col {
                    if !is_cell_op(target_row, c, "SUBTOTAL(") && !is_cell_op(target_row, c, "SUM(") {
                        coords.push((target_row, c));
                    }
                }
            } else if let Some((a_str, b_str)) = tok.split_once(':') {
                if let (Some((r0, c0)), Some((r1, c1))) = (parse_cell_coord(a_str), parse_cell_coord(b_str)) {
                    for r in r0.min(r1)..=r0.max(r1) {
                        for c in c0.min(c1)..=c0.max(c1) {
                            if (r, c) != (target_row, target_col) {
                                if op == TableMathOp::Subtotal && (is_cell_op(r, c, "SUBTOTAL(") || is_cell_op(r, c, "SUM(")) {
                                    continue;
                                }
                                coords.push((r, c));
                            }
                        }
                    }
                }
            } else if let Some((r, c)) = parse_cell_coord(&tok) {
                if (r, c) != (target_row, target_col) && !coords.contains(&(r, c)) {
                    coords.push((r, c));
                }
            }
        }

        let mut inputs = Vec::new();
        for (r, c) in coords {
            let txt = self.cell_text(table_id, r, c);
            if let Some(parsed) = parse_cell_number(&txt) {
                inputs.push(parsed);
            }
        }
        if inputs.is_empty() {
            return Some("0".to_string());
        }
        let val = match op {
            TableMathOp::Sum | TableMathOp::Subtotal => inputs.iter().map(|x| x.value).sum::<f64>(),
            TableMathOp::Average => inputs.iter().map(|x| x.value).sum::<f64>() / (inputs.len() as f64),
            TableMathOp::Median => {
                let mut vals: Vec<f64> = inputs.iter().map(|x| x.value).collect();
                vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let mid = vals.len() / 2;
                if vals.len().is_multiple_of(2) {
                    (vals[mid - 1] + vals[mid]) * 0.5
                } else {
                    vals[mid]
                }
            }
        };
        Some(format_table_math_value(val, &inputs, keep_sig_figs))
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
        fmt.image_id = 0;
        // A heading followed by Enter continues with Default Text, like most modern editors.
        let styles = named_styles();
        if tail_text.is_empty() && matches!(styles.get(fmt.style).map(|s| s.name), Some("Title" | "Subtitle" | "Heading 1" | "Heading 2" | "Heading 3")) {
            fmt = ParaFmt {
                table_id: fmt.table_id,
                table_row: fmt.table_row,
                table_col: fmt.table_col,
                ..ParaFmt::default()
            };
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

    /// Resolves the positioned rich footer for `page_idx` (`0`-based), if any.
    pub fn resolved_page_footer(&self, page_idx: usize, total_pages: usize) -> Option<std::borrow::Cow<'_, PageFooter>> {
        if self.page_footers.is_empty() {
            return None;
        }
        if let Some(pf) = self.page_footers.get(page_idx) {
            if !pf.segments.is_empty() || pf.rule.is_some() {
                // If total_pages changed due to user edits, keep "Page X of Y" totals accurate on existing pages too
                if total_pages != self.page_footers.len()
                    && pf.segments.iter().any(|s| s.runs.iter().any(|r| r.text.to_ascii_lowercase().contains("page ")))
                {
                    let mut cloned = pf.clone();
                    for seg in &mut cloned.segments {
                        for run in &mut seg.runs {
                            run.text = update_footer_page_nums(&run.text, page_idx + 1, total_pages);
                        }
                    }
                    return Some(std::borrow::Cow::Owned(cloned));
                }
                return Some(std::borrow::Cow::Borrowed(pf));
            }
            return None;
        }
        // Extra pages added after import: inherit the last non-empty page footer template
        let template = self
            .page_footers
            .iter()
            .rev()
            .find(|pf| !pf.segments.is_empty() || pf.rule.is_some())?;
        let mut cloned = template.clone();
        for seg in &mut cloned.segments {
            for run in &mut seg.runs {
                run.text = update_footer_page_nums(&run.text, page_idx + 1, total_pages);
            }
        }
        Some(std::borrow::Cow::Owned(cloned))
    }

    /// Concatenated text of headers and footers across the document (used for full-document search/indexing).
    pub fn header_footer_plain_text(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.header.trim().is_empty() {
            parts.push(self.header.clone());
        }
        if !self.page_footers.is_empty() {
            for pf in &self.page_footers {
                for seg in &pf.segments {
                    let s: String = seg.runs.iter().map(|r| r.text.as_str()).collect();
                    if !s.trim().is_empty() {
                        parts.push(s);
                    }
                }
            }
        } else if !self.footer.trim().is_empty() {
            parts.push(self.footer.clone());
        }
        parts.join("\n")
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

/// Updates `"Page N"` or `"Page N of M"` patterns inside a footer string when page count changes.
fn update_footer_page_nums(s: &str, page_1: usize, total_pages: usize) -> String {
    let lower = s.to_ascii_lowercase();
    let Some(idx) = lower.find("page ") else {
        return s.to_string();
    };
    let after_kw = idx + 5;
    let bytes = s.as_bytes();
    let mut i = after_kw;
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    let d1_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == d1_start {
        return s.to_string();
    }
    let d1_end = i;
    // Optional " of M"
    let rest_lower = &lower[d1_end..];
    if let Some(of_rel) = rest_lower.strip_prefix(" of ") {
        let mut j = 0;
        while j < of_rel.len() && of_rel.as_bytes()[j] == b' ' {
            j += 1;
        }
        let d2_start = j;
        while j < of_rel.len() && of_rel.as_bytes()[j].is_ascii_digit() {
            j += 1;
        }
        if j > d2_start {
            let total_end = d1_end + 4 + j;
            return format!("{}{} of {}{}", &s[..d1_start], page_1, total_pages.max(1), &s[total_end..]);
        }
    }
    format!("{}{}{}", &s[..d1_start], page_1, &s[d1_end..])
}

fn default_hf_align() -> Align {
    Align::Center
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
    #[serde(default = "default_hf_align")]
    header_align: Align,
    #[serde(default = "default_hf_align")]
    footer_align: Align,
    #[serde(default)]
    header_italic: bool,
    #[serde(default)]
    footer_italic: bool,
    #[serde(default)]
    page_footers: Vec<PageFooter>,
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
    #[serde(default)]
    images: Vec<DocImage>,
    #[serde(default)]
    tables: Vec<DocTable>,
    paras: Vec<FilePara>,
}

pub const FILE_EXT: &str = "wpx";

pub fn to_json(doc: &Document) -> String {
    let fd = FileDoc {
        format: "syllabary-document".into(),
        version: 1,
        page: doc.page,
        header: doc.header.clone(),
        footer: doc.footer.clone(),
        header_align: doc.header_align,
        footer_align: doc.footer_align,
        header_italic: doc.header_italic,
        footer_italic: doc.footer_italic,
        page_footers: doc.page_footers.clone(),
        info: doc.info.clone(),
        links: doc.links.clone(),
        comments: doc.comments.clone(),
        bookmarks: doc.bookmarks.clone(),
        summary: doc.summary.clone(),
        starred: doc.starred,
        pageless: doc.pageless,
        citations: doc.citations.clone(),
        images: doc.images.clone(),
        tables: doc.tables.clone(),
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
        header_align: fd.header_align,
        footer_align: fd.footer_align,
        header_italic: fd.header_italic,
        footer_italic: fd.footer_italic,
        page_footers: fd.page_footers,
        info: fd.info,
        links: fd.links,
        comments: fd.comments,
        bookmarks: fd.bookmarks,
        summary: fd.summary,
        starred: fd.starred,
        pageless: fd.pageless,
        citations: fd.citations,
        images: fd.images,
        tables: fd.tables,
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
    let mut inline_objects = serde_json::Map::new();
    for img in &doc.images {
        inline_objects.insert(
            format!("kix.img_{}", img.id),
            serde_json::json!({
                "objectId": format!("kix.img_{}", img.id),
                "inlineObjectProperties": {
                    "embeddedObject": {
                        "title": img.name,
                        "description": img.caption,
                        "size": {
                            "width": { "magnitude": img.width_in * 72.0, "unit": "PT" },
                            "height": { "magnitude": img.height_in * 72.0, "unit": "PT" }
                        },
                        "naturalSize": {
                            "width": img.natural_size[0],
                            "height": img.natural_size[1]
                        },
                        "imageProperties": {
                            "contentUri": format!("data:image/png;base64,{}", img.png_b64)
                        }
                    }
                }
            }),
        );
    }
    for p in &doc.paras {
        if p.fmt.hr {
            content.push(serde_json::json!({
                "paragraph": {
                    "elements": [{ "horizontalRule": {} }],
                    "paragraphStyle": {
                        "namedStyleType": "NORMAL_TEXT",
                        "pageBreakBefore": p.fmt.page_break_before
                    }
                }
            }));
            continue;
        }
        let named_style = match styles.get(p.fmt.style).map(|s| s.name) {
            Some("Body Single") => "BODY_SINGLE",
            Some("Title") => "TITLE",
            Some("Subtitle") => "SUBTITLE",
            Some("Heading 1") => "HEADING_1",
            Some("Heading 2") => "HEADING_2",
            Some("Heading 3") => "HEADING_3",
            Some("Quote") => "QUOTE",
            Some("Bullet 1") => "BULLET_1",
            Some("Number List") => "NUMBER_LIST",
            Some("Code") => "CODE",
            _ => "NORMAL_TEXT",
        };
        let alignment = match p.fmt.align {
            Align::Left => "START",
            Align::Center => "CENTER",
            Align::Right => "END",
            Align::Justify => "JUSTIFIED",
        };
        let mut elements = Vec::new();
        if p.fmt.image_id > 0 {
            elements.push(serde_json::json!({
                "inlineObjectElement": {
                    "inlineObjectId": format!("kix.img_{}", p.fmt.image_id)
                }
            }));
        }
        for (a, b, s) in p.runs() {
            let mut txt: String = p.text[a..b].iter().collect();
            if b == p.text.len() {
                txt.push('\n');
            }
            let font_name = font_name(s.font);
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
            if let Some(c2) = s.color_to {
                ts["foregroundColorTo"] = serde_json::json!({
                    "color": {
                        "rgbColor": {
                            "red": c2[0] as f32 / 255.0,
                            "green": c2[1] as f32 / 255.0,
                            "blue": c2[2] as f32 / 255.0,
                        }
                    }
                });
            }
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
            if let Some(hl2) = s.highlight_to {
                ts["backgroundColorTo"] = serde_json::json!({
                    "color": {
                        "rgbColor": {
                            "red": hl2[0] as f32 / 255.0,
                            "green": hl2[1] as f32 / 255.0,
                            "blue": hl2[2] as f32 / 255.0,
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
            let font_name = font_name(p.end_style.font);
            let baseline = if p.end_style.script > 0 {
                "SUPERSCRIPT"
            } else if p.end_style.script < 0 {
                "SUBSCRIPT"
            } else {
                "NONE"
            };
            elements.push(serde_json::json!({
                "textRun": {
                    "content": "\n",
                    "textStyle": {
                        "bold": p.end_style.bold,
                        "italic": p.end_style.italic,
                        "underline": p.end_style.underline,
                        "strikethrough": p.end_style.strike,
                        "baselineOffset": baseline,
                        "fontSize": { "magnitude": p.end_style.size, "unit": "PT" },
                        "weightedFontFamily": { "fontFamily": font_name },
                        "foregroundColor": {
                            "color": {
                                "rgbColor": {
                                    "red": p.end_style.color[0] as f32 / 255.0,
                                    "green": p.end_style.color[1] as f32 / 255.0,
                                    "blue": p.end_style.color[2] as f32 / 255.0,
                                }
                            }
                        }
                    }
                }
            }));
        }
        let mut pstyle = serde_json::json!({
            "namedStyleType": named_style,
            "alignment": alignment,
            "lineSpacing": p.fmt.line_spacing * 100.0,
            "spaceAbove": { "magnitude": p.fmt.space_before, "unit": "PT" },
            "spaceBelow": { "magnitude": p.fmt.space_after, "unit": "PT" },
            "indentStart": { "magnitude": p.fmt.indent as f32 * 36.0, "unit": "PT" },
            "pageBreakBefore": p.fmt.page_break_before
        });
        if let Some(shd) = p.fmt.shading {
            pstyle["shading"] = serde_json::json!({
                "backgroundColor": {
                    "color": {
                        "rgbColor": {
                            "red": shd[0] as f32 / 255.0,
                            "green": shd[1] as f32 / 255.0,
                            "blue": shd[2] as f32 / 255.0,
                        }
                    }
                }
            });
        }
        let mut para_obj = serde_json::json!({
            "elements": elements,
            "paragraphStyle": pstyle
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
                "checked": p.fmt.checked,
                "bulletPreset": p.fmt.bullet_preset
            });
        }
        if p.fmt.table_id > 0 {
            para_obj["tableCellRef"] = serde_json::json!({
                "tableId": p.fmt.table_id,
                "row": p.fmt.table_row,
                "col": p.fmt.table_col
            });
        }
        content.push(serde_json::json!({ "paragraph": para_obj }));
    }

    let pg = doc.page;
    let doc_style = serde_json::json!({
        "pageSize": {
            "width": { "magnitude": pg.width * 72.0, "unit": "PT" },
            "height": { "magnitude": pg.height * 72.0, "unit": "PT" }
        },
        "marginTop": { "magnitude": pg.margin_top * 72.0, "unit": "PT" },
        "marginBottom": { "magnitude": pg.margin_bottom * 72.0, "unit": "PT" },
        "marginLeft": { "magnitude": pg.margin_left * 72.0, "unit": "PT" },
        "marginRight": { "magnitude": pg.margin_right * 72.0, "unit": "PT" },
        "background": {
            "color": {
                "rgbColor": {
                    "red": pg.background[0] as f32 / 255.0,
                    "green": pg.background[1] as f32 / 255.0,
                    "blue": pg.background[2] as f32 / 255.0,
                }
            }
        },
        "pageNumbers": pg.page_numbers,
        "pageless": doc.pageless,
        "headerText": doc.header,
        "footerText": doc.footer,
        "headerAlign": doc.header_align,
        "footerAlign": doc.footer_align,
        "headerItalic": doc.header_italic,
        "footerItalic": doc.footer_italic
    });

    let mut val: serde_json::Value = serde_json::from_str(&to_json(doc)).unwrap_or_else(|_| serde_json::json!({}));
    val["format"] = serde_json::json!("google-docs-document");
    val["title"] = serde_json::json!(title);
    val["doc_id"] = serde_json::json!("syllabary-gdoc");
    val["documentStyle"] = doc_style;
    val["inlineObjects"] = serde_json::Value::Object(inline_objects);
    val["body"] = serde_json::json!({ "content": content });
    serde_json::to_string_pretty(&val).unwrap_or_default()
}

pub fn from_gdoc(s: &str) -> Result<Document, String> {
    let v: serde_json::Value = serde_json::from_str(s).map_err(|e| e.to_string())?;
    // 1. Native Syllabary / hybrid .gdoc with lossless `paras` array
    if v.get("paras").and_then(|p| p.as_array()).is_some() {
        return from_json(s);
    }
    // 2. Google Docs REST API v1 JSON structure (`body.content`)
    if let Some(content) = v.get("body").and_then(|b| b.get("content")).and_then(|c| c.as_array()) {
        let mut doc = Document { paras: Vec::new(), ..Default::default() };
        if let Some(t) = v.get("title").and_then(|t| t.as_str()) {
            doc.info.title = t.to_string();
        }
        let parse_rgb = |c_obj: &serde_json::Value| -> Option<[u8; 3]> {
            let rgb = c_obj.get("color")?.get("rgbColor")?;
            let r = (rgb.get("red").and_then(|x| x.as_f64()).unwrap_or(0.0) * 255.0).round() as u8;
            let g = (rgb.get("green").and_then(|x| x.as_f64()).unwrap_or(0.0) * 255.0).round() as u8;
            let b = (rgb.get("blue").and_then(|x| x.as_f64()).unwrap_or(0.0) * 255.0).round() as u8;
            Some([r, g, b])
        };
        if let Some(ds) = v.get("documentStyle") {
            let pt_to_in = |obj: &serde_json::Value, key: &str| -> Option<f32> {
                obj.get(key)?.get("magnitude")?.as_f64().map(|m| (m as f32) / 72.0)
            };
            if let Some(ps) = ds.get("pageSize") {
                if let Some(w) = pt_to_in(ps, "width") {
                    doc.page.width = w;
                }
                if let Some(h) = pt_to_in(ps, "height") {
                    doc.page.height = h;
                }
            }
            if let Some(mt) = pt_to_in(ds, "marginTop") {
                doc.page.margin_top = mt;
            }
            if let Some(mb) = pt_to_in(ds, "marginBottom") {
                doc.page.margin_bottom = mb;
            }
            if let Some(ml) = pt_to_in(ds, "marginLeft") {
                doc.page.margin_left = ml;
            }
            if let Some(mr) = pt_to_in(ds, "marginRight") {
                doc.page.margin_right = mr;
            }
            if let Some(bg) = ds.get("background").and_then(parse_rgb) {
                doc.page.background = bg;
            }
            if let Some(pn) = ds.get("pageNumbers").and_then(|x| x.as_bool()) {
                doc.page.page_numbers = pn;
            }
            if let Some(pl) = ds.get("pageless").and_then(|x| x.as_bool()) {
                doc.pageless = pl;
            }
            if let Some(ht) = ds.get("headerText").and_then(|x| x.as_str()) {
                doc.header = ht.to_string();
            }
            if let Some(ft) = ds.get("footerText").and_then(|x| x.as_str()) {
                doc.footer = ft.to_string();
            }
            if let Some(ha) = ds.get("headerAlign").and_then(|x| serde_json::from_value(x.clone()).ok()) {
                doc.header_align = ha;
            }
            if let Some(fa) = ds.get("footerAlign").and_then(|x| serde_json::from_value(x.clone()).ok()) {
                doc.footer_align = fa;
            }
            if let Some(hi) = ds.get("headerItalic").and_then(|x| x.as_bool()) {
                doc.header_italic = hi;
            }
            if let Some(fi) = ds.get("footerItalic").and_then(|x| x.as_bool()) {
                doc.footer_italic = fi;
            }
        }
        if let Some(tbls) = v.get("tables").and_then(|x| serde_json::from_value::<Vec<DocTable>>(x.clone()).ok()) {
            doc.tables = tbls;
        }
        if let Some(iobjs) = v.get("inlineObjects").and_then(|x| x.as_object()) {
            for (oid, obj) in iobjs {
                let id_num = oid
                    .strip_prefix("kix.img_")
                    .and_then(|n| n.parse::<u64>().ok())
                    .unwrap_or(doc.images.len() as u64 + 1);
                if let Some(emb) = obj.get("inlineObjectProperties").and_then(|p| p.get("embeddedObject")) {
                    let name = emb.get("title").and_then(|x| x.as_str()).unwrap_or("Image").to_string();
                    let caption = emb.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let w_in = emb
                        .get("size")
                        .and_then(|s| s.get("width"))
                        .and_then(|w| w.get("magnitude"))
                        .and_then(|m| m.as_f64())
                        .map_or(3.0, |m| (m as f32) / 72.0);
                    let h_in = emb
                        .get("size")
                        .and_then(|s| s.get("height"))
                        .and_then(|h| h.get("magnitude"))
                        .and_then(|m| m.as_f64())
                        .map_or(2.0, |m| (m as f32) / 72.0);
                    let nw = emb.get("naturalSize").and_then(|s| s.get("width")).and_then(|x| x.as_u64()).unwrap_or(100) as u32;
                    let nh = emb.get("naturalSize").and_then(|s| s.get("height")).and_then(|x| x.as_u64()).unwrap_or(100) as u32;
                    let uri = emb
                        .get("imageProperties")
                        .and_then(|p| p.get("contentUri"))
                        .and_then(|u| u.as_str())
                        .unwrap_or("");
                    let b64 = uri.strip_prefix("data:image/png;base64,").unwrap_or(uri).to_string();
                    if !b64.is_empty() {
                        doc.images.push(DocImage {
                            id: id_num,
                            name,
                            natural_size: [nw.max(1), nh.max(1)],
                            width_in: w_in,
                            height_in: h_in,
                            caption,
                            png_b64: b64,
                            seamless: false,
                        });
                    }
                }
            }
        }
        let styles = named_styles();
        for item in content {
            let Some(p_obj) = item.get("paragraph") else { continue };
            let mut fmt = ParaFmt::default();
            let mut base = CharStyle::default();
            if let Some(ps) = p_obj.get("paragraphStyle") {
                if let Some(nst) = ps.get("namedStyleType").and_then(|x| x.as_str()) {
                    let style_idx = match nst {
                        "BODY_SINGLE" => Some(1),
                        "TITLE" => Some(2),
                        "SUBTITLE" => Some(3),
                        "HEADING_1" => Some(4),
                        "HEADING_2" => Some(5),
                        "HEADING_3" | "HEADING_4" | "HEADING_5" | "HEADING_6" => Some(6),
                        "QUOTE" => Some(7),
                        "BULLET_1" => Some(8),
                        "NUMBER_LIST" => Some(9),
                        "CODE" => Some(10),
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
                if let Some(pbb) = ps.get("pageBreakBefore").and_then(|x| x.as_bool()) {
                    fmt.page_break_before = pbb;
                }
                if let Some(shd) = ps.get("shading").and_then(|s| s.get("backgroundColor")).and_then(parse_rgb) {
                    fmt.shading = Some(shd);
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
                if let Some(bp) = bul.get("bulletPreset").and_then(|x| x.as_u64()) {
                    fmt.bullet_preset = bp as u8;
                }
            }
            if let Some(tcr) = p_obj.get("tableCellRef") {
                fmt.table_id = tcr.get("tableId").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                fmt.table_row = tcr.get("row").and_then(|x| x.as_u64()).unwrap_or(0) as u16;
                fmt.table_col = tcr.get("col").and_then(|x| x.as_u64()).unwrap_or(0) as u16;
            }
            let mut p = Para::new(base, fmt);
            if let Some(elems) = p_obj.get("elements").and_then(|e| e.as_array()) {
                for el in elems {
                    if el.get("horizontalRule").is_some() {
                        p.fmt.hr = true;
                        continue;
                    }
                    if let Some(ioe) = el.get("inlineObjectElement") {
                        if let Some(oid) = ioe.get("inlineObjectId").and_then(|x| x.as_str()) {
                            if let Some(id_num) = oid.strip_prefix("kix.img_").and_then(|n| n.parse::<u64>().ok()) {
                                p.fmt.image_id = id_num;
                            }
                        }
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
                        if let Some(fg) = ts.get("foregroundColor").and_then(parse_rgb) {
                            st.color = fg;
                        }
                        if let Some(fg2) = ts.get("foregroundColorTo").and_then(parse_rgb) {
                            st.color_to = Some(fg2);
                        }
                        if let Some(bg) = ts.get("backgroundColor").and_then(parse_rgb) {
                            st.highlight = Some(bg);
                        }
                        if let Some(bg2) = ts.get("backgroundColorTo").and_then(parse_rgb) {
                            st.highlight_to = Some(bg2);
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
    let fmt_para_inline = |p: &Para| -> String {
        let mut s_out = String::new();
        for (a, b, s) in p.runs() {
            let txt: String = p.text[a..b].iter().collect();
            let mut seg = txt.replace('|', "\\|");
            if s.bold && !styles.get(p.fmt.style).is_some_and(|ns| ns.chr.bold) {
                seg = format!("**{seg}**");
            }
            if s.italic && !styles.get(p.fmt.style).is_some_and(|ns| ns.chr.italic) {
                seg = format!("*{seg}*");
            }
            if s.strike {
                seg = format!("~~{seg}~~");
            }
            if let Some(url) = doc.link_url(s.link) {
                seg = format!("[{seg}]({url})");
            }
            s_out.push_str(&seg);
        }
        s_out
    };
    let mut pi = 0;
    while pi < doc.paras.len() {
        let p = &doc.paras[pi];
        if p.fmt.table_id > 0 {
            let tid = p.fmt.table_id;
            let mut tbl_end = pi + 1;
            while tbl_end < doc.paras.len() && doc.paras[tbl_end].fmt.table_id == tid {
                tbl_end += 1;
            }
            let max_row = doc.paras[pi..tbl_end].iter().map(|x| x.fmt.table_row).max().unwrap_or(0) as usize;
            let max_col = doc.table(tid).map_or_else(
                || doc.paras[pi..tbl_end].iter().map(|x| x.fmt.table_col).max().unwrap_or(0) as usize,
                |t| t.grid_cols_in.len().max(1) - 1,
            );
            let ncols = max_col + 1;
            for r in 0..=max_row {
                out.push('|');
                for c in 0..ncols {
                    let mut cell_parts = Vec::new();
                    for cp in &doc.paras[pi..tbl_end] {
                        if cp.fmt.table_row as usize == r && cp.fmt.table_col as usize == c {
                            cell_parts.push(fmt_para_inline(cp));
                        }
                    }
                    out.push(' ');
                    out.push_str(&cell_parts.join("<br>"));
                    out.push_str(" |");
                }
                out.push('\n');
                if r == 0 {
                    out.push('|');
                    for _ in 0..ncols {
                        out.push_str(" --- |");
                    }
                    out.push('\n');
                }
            }
            out.push('\n');
            pi = tbl_end;
            continue;
        }
        if p.fmt.hr {
            out.push_str("---\n\n");
            pi += 1;
            continue;
        }
        if p.fmt.image_id > 0 {
            if let Some(img) = doc.image(p.fmt.image_id) {
                let cap = if img.caption.trim().is_empty() { &img.name } else { img.caption.trim() };
                out.push_str(&format!("![{cap}]({})\n\n", img.name));
                pi += 1;
                continue;
            }
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
            if s.bold && !styles.get(p.fmt.style).is_some_and(|ns| ns.chr.bold) {
                seg = format!("**{seg}**");
            }
            if s.italic && !styles.get(p.fmt.style).is_some_and(|ns| ns.chr.italic) {
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
        let next_is_list = doc.paras.get(pi + 1).is_some_and(|np| np.fmt.list != ListKind::None);
        if p.fmt.list == ListKind::None || !next_is_list {
            out.push('\n');
        }
        pi += 1;
    }
    out
}

pub fn from_markdown(src: &str) -> Document {
    let mut doc = Document { paras: Vec::new(), ..Default::default() };
    let styles = named_styles();
    let normalized = src.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let mut li = 0usize;
    while li < lines.len() {
        let raw_line = lines[li];
        let trimmed = raw_line.trim_end();
        if trimmed.is_empty() {
            // A single blank line after a paragraph in Markdown is the paragraph separator;
            // consecutive blank lines represent intentional empty paragraphs.
            let mut blanks = 0usize;
            while li < lines.len() && lines[li].trim_end().is_empty() {
                blanks += 1;
                li += 1;
            }
            for _ in 1..blanks {
                doc.paras.push(Para::new(CharStyle::default(), ParaFmt::default()));
            }
            continue;
        }
        // GFM pipe table (`| col1 | col2 |`)
        if trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() >= 2 {
            let mut tbl_lines = Vec::new();
            while li < lines.len() {
                let tl = lines[li].trim();
                if tl.starts_with('|') && tl.ends_with('|') && tl.len() >= 2 {
                    tbl_lines.push(tl);
                    li += 1;
                } else {
                    break;
                }
            }
            // Filter out separator row (`| --- | --- |`)
            let data_rows: Vec<Vec<String>> = tbl_lines
                .into_iter()
                .filter(|row| {
                    let inner = &row[1..row.len() - 1];
                    !inner.split('|').all(|c| {
                        let ct = c.trim();
                        !ct.is_empty() && ct.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
                    })
                })
                .map(|row| {
                    let inner = &row[1..row.len() - 1];
                    inner.split('|').map(|c| c.trim().replace("\\|", "|")).collect()
                })
                .collect();
            if !data_rows.is_empty() {
                let ncols = data_rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
                let avail_w = (doc.page.width - doc.page.margin_left - doc.page.margin_right).max(2.0);
                let col_w = avail_w / ncols as f32;
                let tid = doc.next_table_id();
                let mut rows_spec = Vec::with_capacity(data_rows.len());
                for (r_idx, r_cells) in data_rows.iter().enumerate() {
                    let mut cells_spec = Vec::with_capacity(ncols);
                    for c_idx in 0..ncols {
                        cells_spec.push(TableCellSpec {
                            width_in: col_w,
                            grid_span: 1,
                            shading: None,
                            borders: CellBorders::all(BorderSpec::default()),
                            v_align: CellVAlign::Top,
                            pad_left_in: 0.075,
                            pad_right_in: 0.075,
                            pad_top_in: 0.02,
                            pad_bottom_in: 0.02,
                            formula: None,
                            wrap_text: true,
                        });
                        let raw_cell = r_cells.get(c_idx).map_or("", String::as_str);
                        for part in raw_cell.split("<br>") {
                            let fmt = ParaFmt {
                                table_id: tid,
                                table_row: r_idx as u16,
                                table_col: c_idx as u16,
                                ..ParaFmt::default()
                            };
                            let mut p = Para::new(CharStyle::default(), fmt);
                            parse_md_inline(&mut doc, &mut p, part, CharStyle::default());
                            doc.paras.push(p);
                        }
                    }
                    rows_spec.push(TableRowSpec {
                        cells: cells_spec,
                        min_height_pt: 0.0,
                        grid_after: 0,
                    });
                }
                doc.tables.push(DocTable {
                    id: tid,
                    grid_cols_in: vec![col_w; ncols],
                    rows: rows_spec,
                    indent_in: 0.0,
                });
                if li < lines.len() && lines[li].trim().is_empty() {
                    li += 1;
                }
                continue;
            }
        }
        li += 1;
        if trimmed == "---" || trimmed == "***" {
            let mut p = Para::new(CharStyle::default(), ParaFmt::default());
            p.fmt.hr = true;
            doc.paras.push(p);
            continue;
        }
        let mut fmt = ParaFmt::default();
        let mut base = CharStyle::default();
        let leading_spaces = trimmed.chars().take_while(|&c| c == ' ').count();
        let list_indent = (leading_spaces / 2).min(8) as u8;
        let unindented = &trimmed[leading_spaces..];
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
        } else if let Some(rest) = unindented.strip_prefix("- [x] ").or_else(|| unindented.strip_prefix("- [X] ")) {
            fmt.list = ListKind::Checklist;
            fmt.checked = true;
            fmt.indent = list_indent;
            text = rest;
        } else if let Some(rest) = unindented.strip_prefix("- [ ] ").or_else(|| unindented.strip_prefix("[] ")) {
            fmt.list = ListKind::Checklist;
            fmt.checked = false;
            fmt.indent = list_indent;
            text = rest;
        } else if let Some(rest) = unindented.strip_prefix("- ").or_else(|| unindented.strip_prefix("* ")) {
            fmt.list = ListKind::Bullet;
            fmt.indent = list_indent;
            text = rest;
        } else if let Some(dot) = unindented.find(". ") {
            if dot > 0 && unindented[..dot].chars().all(|c| c.is_ascii_digit()) {
                fmt.list = ListKind::Number;
                fmt.indent = list_indent;
                text = &unindented[dot + 2..];
            }
        }
        let mut p = Para::new(base, fmt);
        parse_md_inline(&mut doc, &mut p, text, base);
        doc.paras.push(p);
    }
    if doc.paras.is_empty() {
        doc.paras.push(Para::new(CharStyle::default(), ParaFmt::default()));
    }
    doc
}

fn parse_md_inline(doc: &mut Document, p: &mut Para, text: &str, base: CharStyle) {
    let mut i = 0usize;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        // Link: [label](url)
        if bytes[i] == b'[' {
            if let Some(close_br) = text[i + 1..].find("](") {
                let label_end = i + 1 + close_br;
                let url_start = label_end + 2;
                if let Some(close_par) = text[url_start..].find(')') {
                    let url_end = url_start + close_par;
                    let label = &text[i + 1..label_end];
                    let url = &text[url_start..url_end];
                    let lid = doc.add_link(url);
                    let mut st = base;
                    st.link = lid;
                    st.underline = true;
                    if st.color == [0, 0, 0] {
                        st.color = [11, 87, 208];
                    }
                    parse_md_inline(doc, p, label, st);
                    i = url_end + 1;
                    continue;
                }
            }
        }
        // Bold+Italic: ***...***
        if text[i..].starts_with("***") {
            if let Some(end) = text[i + 3..].find("***") {
                if end > 0 {
                    let mut st = base;
                    st.bold = true;
                    st.italic = true;
                    parse_md_inline(doc, p, &text[i + 3..i + 3 + end], st);
                    i += 6 + end;
                    continue;
                }
            }
        }
        // Bold: **...**
        if text[i..].starts_with("**") {
            if let Some(end) = text[i + 2..].find("**") {
                if end > 0 {
                    let mut st = base;
                    st.bold = true;
                    parse_md_inline(doc, p, &text[i + 2..i + 2 + end], st);
                    i += 4 + end;
                    continue;
                }
            }
        }
        // Strikethrough: ~~...~~
        if text[i..].starts_with("~~") {
            if let Some(end) = text[i + 2..].find("~~") {
                if end > 0 {
                    let mut st = base;
                    st.strike = true;
                    parse_md_inline(doc, p, &text[i + 2..i + 2 + end], st);
                    i += 4 + end;
                    continue;
                }
            }
        }
        // Italic: *...*
        if bytes[i] == b'*' {
            if let Some(end) = text[i + 1..].find('*') {
                if end > 0 {
                    let mut st = base;
                    st.italic = true;
                    parse_md_inline(doc, p, &text[i + 1..i + 1 + end], st);
                    i += 2 + end;
                    continue;
                }
            }
        }
        if let Some(ch) = text[i..].chars().next() {
            p.text.push(ch);
            p.styles.push(base);
            p.end_style = base;
            i += ch.len_utf8();
        } else {
            break;
        }
    }
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
        if p.fmt.image_id > 0 {
            if let Some(img) = doc.image(p.fmt.image_id) {
                out.push_str(&format!(
                    "<figure style=\"text-align:{align};margin:12px 0;\"><img src=\"data:image/png;base64,{}\" style=\"width:{:.2}in;height:{:.2}in;max-width:100%;\" alt=\"{}\">",
                    img.png_b64,
                    img.width_in,
                    img.height_in,
                    img.name
                ));
                if !img.caption.trim().is_empty() {
                    out.push_str(&format!("<figcaption style=\"font-style:italic;color:#5a606c;font-size:10.5pt;margin-top:4px;\">{}</figcaption>", img.caption.trim()));
                }
                out.push_str("</figure>\n");
                continue;
            }
        }
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
    if doc.page.background != [255, 255, 255] && !colors.contains(&doc.page.background) {
        colors.push(doc.page.background);
    }
    for p in &doc.paras {
        if let Some(shd) = p.fmt.shading {
            if !colors.contains(&shd) {
                colors.push(shd);
            }
        }
        for s in p.styles.iter().chain(std::iter::once(&p.end_style)) {
            if !colors.contains(&s.color) {
                colors.push(s.color);
            }
            if let Some(c2) = s.color_to {
                if !colors.contains(&c2) {
                    colors.push(c2);
                }
            }
            if let Some(h) = s.highlight {
                if !colors.contains(&h) {
                    colors.push(h);
                }
            }
            if let Some(h2) = s.highlight_to {
                if !colors.contains(&h2) {
                    colors.push(h2);
                }
            }
        }
    }
    let mut out = format!("{{\\rtf1\\ansi\\deff{DEFAULT_FONT}{{\\fonttbl");
    for i in 0..font_count() {
        let f = font_name(i as u8);
        out.push_str(&format!("{{\\f{i} {f};}}"));
    }
    out.push_str("}{\\colortbl;");
    for c in &colors {
        out.push_str(&format!("\\red{}\\green{}\\blue{};", c[0], c[1], c[2]));
    }
    out.push('}');
    let pg = doc.page;
    let tw = |inch: f32| (inch * 1440.0).round() as i32;
    let pgbg_ci = if pg.background != [255, 255, 255] {
        colors.iter().position(|c| *c == pg.background).map_or(0, |i| i + 1)
    } else {
        0
    };
    out.push_str(&format!(
        "\\paperw{}\\paperh{}\\margl{}\\margr{}\\margt{}\\margb{}{}{}{}\n",
        tw(pg.width),
        tw(pg.height),
        tw(pg.margin_left),
        tw(pg.margin_right),
        tw(pg.margin_top),
        tw(pg.margin_bottom),
        if pgbg_ci > 0 { format!("\\pgbg{pgbg_ci}") } else { String::new() },
        if pg.page_numbers { "\\pgnum1" } else { "" },
        if doc.pageless { "\\pageless1" } else { "" }
    ));
    if !doc.tables.is_empty() {
        if let Ok(tbl_json) = serde_json::to_vec(&doc.tables) {
            out.push_str(&format!("{{\\*\\syllabarytbls {}}}", base64_encode(&tbl_json)));
        }
    }
    for (pi, p) in doc.paras.iter().enumerate() {
        out.push_str("\\pard");
        if p.fmt.table_id > 0 {
            out.push_str(&format!("\\tblid{}\\tblrow{}\\tblcol{}", p.fmt.table_id, p.fmt.table_row, p.fmt.table_col));
        }
        if p.fmt.style > 0 {
            out.push_str(&format!("\\s{}", p.fmt.style));
        }
        if p.fmt.page_break_before {
            out.push_str("\\pagebb");
        }
        if p.fmt.hr {
            out.push_str("\\brdrb");
        }
        if let Some(shd) = p.fmt.shading {
            let si = colors.iter().position(|c| *c == shd).unwrap_or(0) + 1;
            out.push_str(&format!("\\cbpat{si}"));
        }
        if p.fmt.bullet_preset > 0 {
            out.push_str(&format!("\\bpreset{}", p.fmt.bullet_preset));
        }
        if p.fmt.image_id > 0 {
            if let Some(img) = doc.image(p.fmt.image_id) {
                out.push_str(&format!(
                    "\\imgid{}\\imgw{}\\imgh{}\\imgcap{}",
                    img.id,
                    (img.width_in * 1440.0).round() as i32,
                    (img.height_in * 1440.0).round() as i32,
                    if img.caption.trim().is_empty() { 0 } else { 1 }
                ));
            }
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
        out.push_str(&format!(
            "\\sb{}\\sa{}\\sl{}\\slmult1 ",
            (p.fmt.space_before * 20.0).round() as i32,
            (p.fmt.space_after * 20.0).round() as i32,
            (p.fmt.line_spacing * 240.0).round() as i32
        ));
        match p.fmt.list {
            ListKind::Bullet => out.push_str("\\u8226?\\tab "),
            ListKind::Number => out.push_str(&format!("{}.\\tab ", doc.list_number(pi))),
            ListKind::Checklist => out.push_str(if p.fmt.checked { "\\u9745?\\tab " } else { "\\u9744?\\tab " }),
            ListKind::None => {}
        }
        let runs = p.runs();
        if runs.is_empty() {
            let s = p.end_style;
            let ci = colors.iter().position(|c| *c == s.color).unwrap_or(0) + 1;
            out.push_str(&format!(
                "{{\\f{}\\fs{}{}{}{}{}\\cf{} }}",
                s.font,
                (s.size * 2.0).round() as i32,
                if s.bold { "\\b" } else { "" },
                if s.italic { "\\i" } else { "" },
                if s.underline { "\\ul" } else { "" },
                if s.strike { "\\strike" } else { "" },
                ci
            ));
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
            if let Some(c2) = s.color_to {
                let ci2 = colors.iter().position(|c| *c == c2).unwrap_or(0) + 1;
                out.push_str(&format!("\\cfto{ci2}"));
            }
            if let Some(h) = s.highlight {
                let hi = colors.iter().position(|c| *c == h).unwrap_or(0) + 1;
                out.push_str(&format!("\\highlight{hi}"));
            }
            if let Some(h2) = s.highlight_to {
                let hi2 = colors.iter().position(|c| *c == h2).unwrap_or(0) + 1;
                out.push_str(&format!("\\hlto{hi2}"));
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

/// RTF reader: parses page setup, font & color tables, paragraph styles/spacing/indents/lists,
/// and character styling (fonts, sizes, colors, highlights, gradients, scripts).
pub fn from_rtf(src: &str) -> Document {
    let mut doc = Document { paras: vec![Para::new(CharStyle::default(), ParaFmt::default())], ..Default::default() };
    let styles = named_styles();
    let mut fonts: std::collections::HashMap<i32, u8> = Default::default();
    let mut colors: Vec<[u8; 3]> = vec![[0, 0, 0]]; // index 0 is auto/black
    let mut stack: Vec<(CharStyle, bool)> = Vec::new();
    let mut st = CharStyle::default();
    let mut skip = false;
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut pos = Pos::new(0, 0);
    let mut in_fonttbl = false;
    let mut in_colortbl = false;
    let mut in_scribetbls = false;
    let mut scribetbls_b64 = String::new();
    let mut cur_rgb: [u8; 3] = [0, 0, 0];
    let mut has_rgb = false;
    let mut font_num = 0;
    let mut font_name = String::new();
    let mut uc_skip = 0usize;
    let mut raw_li: Vec<i32> = vec![0];
    let mut raw_fi: Vec<i32> = vec![0];
    let mut raw_img_dims: Vec<(u64, f32, f32, bool)> = vec![(0, 0.0, 0.0, false)];

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
                if in_colortbl {
                    if has_rgb {
                        colors.push(cur_rgb);
                        cur_rgb = [0, 0, 0];
                        has_rgb = false;
                    }
                    in_colortbl = false;
                }
                if in_scribetbls {
                    if let Some(bytes) = base64_decode(scribetbls_b64.trim()) {
                        if let Ok(tbls) = serde_json::from_slice::<Vec<DocTable>>(&bytes) {
                            doc.tables = tbls;
                        }
                    }
                    scribetbls_b64.clear();
                    in_scribetbls = false;
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
                    "colortbl" => {
                        in_colortbl = true;
                        skip = true;
                        colors.clear();
                    }
                    "red" if in_colortbl => {
                        cur_rgb[0] = num.unwrap_or(0).clamp(0, 255) as u8;
                        has_rgb = true;
                    }
                    "green" if in_colortbl => {
                        cur_rgb[1] = num.unwrap_or(0).clamp(0, 255) as u8;
                        has_rgb = true;
                    }
                    "blue" if in_colortbl => {
                        cur_rgb[2] = num.unwrap_or(0).clamp(0, 255) as u8;
                        has_rgb = true;
                    }
                    "stylesheet" | "info" | "pict" | "header" | "footer" | "listtable" | "listoverridetable" | "generator" => skip = true,
                    "paperw" => {
                        if let Some(n) = num {
                            doc.page.width = ((n as f32 / 1440.0 * 100.0).round() / 100.0).clamp(1.0, 40.0);
                        }
                    }
                    "paperh" => {
                        if let Some(n) = num {
                            doc.page.height = ((n as f32 / 1440.0 * 100.0).round() / 100.0).clamp(1.0, 40.0);
                        }
                    }
                    "margl" => {
                        if let Some(n) = num {
                            let raw = (n as f32 / 1440.0).clamp(0.0, 10.0);
                            let snapped = (raw * 100.0).round() / 100.0;
                            doc.page.margin_left = if (raw - snapped).abs() < 0.0005 { snapped } else { raw };
                        }
                    }
                    "margr" => {
                        if let Some(n) = num {
                            let raw = (n as f32 / 1440.0).clamp(0.0, 10.0);
                            let snapped = (raw * 100.0).round() / 100.0;
                            doc.page.margin_right = if (raw - snapped).abs() < 0.0005 { snapped } else { raw };
                        }
                    }
                    "margt" => {
                        if let Some(n) = num {
                            let raw = (n as f32 / 1440.0).clamp(0.0, 10.0);
                            let snapped = (raw * 100.0).round() / 100.0;
                            doc.page.margin_top = if (raw - snapped).abs() < 0.0005 { snapped } else { raw };
                        }
                    }
                    "margb" => {
                        if let Some(n) = num {
                            let raw = (n as f32 / 1440.0).clamp(0.0, 10.0);
                            let snapped = (raw * 100.0).round() / 100.0;
                            doc.page.margin_bottom = if (raw - snapped).abs() < 0.0005 { snapped } else { raw };
                        }
                    }
                    "pgbg" => {
                        if let Some(n) = num {
                            if let Some(c) = colors.get(n as usize) {
                                doc.page.background = *c;
                            }
                        }
                    }
                    "pgnum" => doc.page.page_numbers = on,
                    "pageless" => doc.pageless = on,
                    "f" if in_fonttbl => font_num = num.unwrap_or(0),
                    "f" => {
                        st.font = *fonts.get(&num.unwrap_or(0)).unwrap_or(&DEFAULT_FONT);
                        doc.paras[pos.p].end_style = st;
                    }
                    "fs" => {
                        st.size = (num.unwrap_or(24) as f32 / 2.0).clamp(1.0, MAX_SIZE);
                        doc.paras[pos.p].end_style = st;
                    }
                    "b" => {
                        st.bold = on;
                        doc.paras[pos.p].end_style = st;
                    }
                    "i" => {
                        st.italic = on;
                        doc.paras[pos.p].end_style = st;
                    }
                    "ul" => {
                        st.underline = on;
                        doc.paras[pos.p].end_style = st;
                    }
                    "ulnone" => {
                        st.underline = false;
                        doc.paras[pos.p].end_style = st;
                    }
                    "strike" => {
                        st.strike = on;
                        doc.paras[pos.p].end_style = st;
                    }
                    "super" => st.script = 1,
                    "sub" => st.script = -1,
                    "nosupersub" => st.script = 0,
                    "cf" => {
                        let idx = num.unwrap_or(0).max(0) as usize;
                        st.color = colors.get(idx).copied().unwrap_or([0, 0, 0]);
                        doc.paras[pos.p].end_style = st;
                    }
                    "cfto" => {
                        let idx = num.unwrap_or(0).max(0) as usize;
                        st.color_to = colors.get(idx).copied();
                        doc.paras[pos.p].end_style = st;
                    }
                    "highlight" | "chcbpat" => {
                        let idx = num.unwrap_or(0).max(0) as usize;
                        st.highlight = if idx == 0 { None } else { colors.get(idx).copied() };
                        doc.paras[pos.p].end_style = st;
                    }
                    "hlto" => {
                        let idx = num.unwrap_or(0).max(0) as usize;
                        st.highlight_to = if idx == 0 { None } else { colors.get(idx).copied() };
                        doc.paras[pos.p].end_style = st;
                    }
                    "cbpat" => {
                        let idx = num.unwrap_or(0).max(0) as usize;
                        doc.paras[pos.p].fmt.shading = if idx == 0 { None } else { colors.get(idx).copied() };
                    }
                    "plain" => st = CharStyle::default(),
                    "par" | "line" if !skip => {
                        pos = doc.split(pos);
                        raw_li.push(0);
                        raw_fi.push(0);
                        raw_img_dims.push((0, 0.0, 0.0, false));
                    }
                    "tab" if !skip => pos = doc.insert_text(pos, "\t", st),
                    "qc" => doc.paras[pos.p].fmt.align = Align::Center,
                    "qr" => doc.paras[pos.p].fmt.align = Align::Right,
                    "qj" => doc.paras[pos.p].fmt.align = Align::Justify,
                    "ql" => doc.paras[pos.p].fmt.align = Align::Left,
                    "s" => {
                        let idx = num.unwrap_or(0).max(0) as usize;
                        if idx < styles.len() {
                            doc.paras[pos.p].fmt = ParaFmt { style: idx, ..styles[idx].fmt };
                            doc.paras[pos.p].end_style = styles[idx].chr;
                        }
                    }
                    "pagebb" => doc.paras[pos.p].fmt.page_break_before = true,
                    "brdrb" => doc.paras[pos.p].fmt.hr = true,
                    "bpreset" => doc.paras[pos.p].fmt.bullet_preset = num.unwrap_or(0).clamp(0, 255) as u8,
                    "syllabarytbls" | "scribetbls" => {
                        in_scribetbls = true;
                        skip = true;
                    }
                    "tblid" => doc.paras[pos.p].fmt.table_id = num.unwrap_or(0).max(0) as u32,
                    "tblrow" => doc.paras[pos.p].fmt.table_row = num.unwrap_or(0).max(0) as u16,
                    "tblcol" => doc.paras[pos.p].fmt.table_col = num.unwrap_or(0).max(0) as u16,
                    "imgid" => {
                        let id = num.unwrap_or(0).max(0) as u64;
                        doc.paras[pos.p].fmt.image_id = id;
                        if let Some(slot) = raw_img_dims.get_mut(pos.p) {
                            slot.0 = id;
                        }
                    }
                    "imgw" => {
                        if let Some(slot) = raw_img_dims.get_mut(pos.p) {
                            slot.1 = (num.unwrap_or(2880) as f32 / 1440.0).clamp(0.5, 20.0);
                        }
                    }
                    "imgh" => {
                        if let Some(slot) = raw_img_dims.get_mut(pos.p) {
                            slot.2 = (num.unwrap_or(2160) as f32 / 1440.0).clamp(0.5, 20.0);
                        }
                    }
                    "imgcap" => {
                        if let Some(slot) = raw_img_dims.get_mut(pos.p) {
                            slot.3 = on;
                        }
                    }
                    "sb" => {
                        if let Some(n) = num {
                            doc.paras[pos.p].fmt.space_before = (n as f32 / 20.0).clamp(0.0, 72.0);
                        }
                    }
                    "sa" => {
                        if let Some(n) = num {
                            doc.paras[pos.p].fmt.space_after = (n as f32 / 20.0).clamp(0.0, 72.0);
                        }
                    }
                    "sl" => {
                        if let Some(n) = num {
                            doc.paras[pos.p].fmt.line_spacing = (n.abs() as f32 / 240.0).clamp(1.0, 3.0);
                        }
                    }
                    "li" => {
                        if let Some(n) = num {
                            if let Some(slot) = raw_li.get_mut(pos.p) {
                                *slot = n;
                            }
                            doc.paras[pos.p].fmt.indent = ((n as f32 / 720.0).floor().max(0.0) as u8).min(8);
                        }
                    }
                    "fi" => {
                        if let Some(n) = num {
                            if let Some(slot) = raw_fi.get_mut(pos.p) {
                                *slot = n;
                            }
                        }
                    }
                    "pard" => {
                        doc.paras[pos.p].fmt = ParaFmt::default();
                        if let Some(slot) = raw_li.get_mut(pos.p) {
                            *slot = 0;
                        }
                        if let Some(slot) = raw_fi.get_mut(pos.p) {
                            *slot = 0;
                        }
                    }
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
                if in_colortbl && c == ';' {
                    colors.push(cur_rgb);
                    cur_rgb = [0, 0, 0];
                    has_rgb = false;
                } else if in_fonttbl {
                    font_name.push(c);
                } else if in_scribetbls {
                    scribetbls_b64.push(c);
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
    // Reconstruct list markers (\u8226?\tab, \u9744?\tab, \u9745?\tab, N.\tab) and empty-para padding
    for (pi, p) in doc.paras.iter_mut().enumerate() {
        let li = raw_li.get(pi).copied().unwrap_or(0);
        let fi = raw_fi.get(pi).copied().unwrap_or(0);
        if fi < 0 || p.text.first().is_some_and(|&c| c == '•' || c == '☐' || c == '☑') {
            if p.text.len() >= 2 && p.text[1] == '\t' {
                match p.text[0] {
                    '•' => {
                        p.fmt.list = ListKind::Bullet;
                        p.text.drain(0..2);
                        p.styles.drain(0..2);
                    }
                    '☐' => {
                        p.fmt.list = ListKind::Checklist;
                        p.fmt.checked = false;
                        p.text.drain(0..2);
                        p.styles.drain(0..2);
                    }
                    '☑' => {
                        p.fmt.list = ListKind::Checklist;
                        p.fmt.checked = true;
                        p.text.drain(0..2);
                        p.styles.drain(0..2);
                    }
                    _ => {}
                }
            } else if let Some(tab_pos) = p.text.iter().position(|&c| c == '\t') {
                if tab_pos >= 2 && p.text[tab_pos - 1] == '.' && p.text[..tab_pos - 1].iter().all(|c| c.is_ascii_digit()) {
                    p.fmt.list = ListKind::Number;
                    p.text.drain(0..=tab_pos);
                    p.styles.drain(0..=tab_pos);
                }
            }
            if p.fmt.list != ListKind::None {
                p.fmt.indent = (((li - 360).max(0) as f32 / 720.0).round() as u8).min(8);
            }
        }
        // Empty paragraphs emitted as `{\fN\fsM }` have a single space placeholder
        if p.text.len() == 1 && p.text[0] == ' ' {
            p.end_style = p.styles[0];
            p.text.clear();
            p.styles.clear();
        }
        if let Some(&(id, w_in, h_in, has_cap)) = raw_img_dims.get(pi) {
            if id > 0 && w_in > 0.0 && h_in > 0.0 && !doc.images.iter().any(|im| im.id == id) {
                let para_cap: String = p.text.iter().collect();
                doc.images.push(DocImage {
                    id,
                    name: format!("Image {id}"),
                    natural_size: [(w_in * 96.0).round() as u32, (h_in * 96.0).round() as u32],
                    width_in: w_in,
                    height_in: h_in,
                    caption: if !para_cap.is_empty() {
                        para_cap
                    } else if has_cap {
                        format!("Image {id}")
                    } else {
                        String::new()
                    },
                    png_b64: String::new(),
                    seamless: false,
                });
            }
        }
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

    fn make_reference_document(seed: usize) -> Document {
        let styles = named_styles();
        let mut d = Document { paras: Vec::new(), ..Default::default() };
        d.info.title = format!("Syllabary Layout Benchmark Document #{seed:02}");
        d.info.author = "Layout Verification Suite".into();
        d.info.subject = format!("Format Fidelity Test {seed}");

        // Page geometry variations across the 50 documents:
        // Letter, Legal, A4, Executive, Portrait & Landscape, custom margins, page numbers, backgrounds.
        let (_, mut pw, mut ph) = PAGE_SIZES[seed % PAGE_SIZES.len()];
        if seed % 7 == 3 {
            std::mem::swap(&mut pw, &mut ph); // Landscape
        }
        let margin_step = [0.75, 1.0, 1.25, 0.85, 1.1][seed % 5];
        d.page = PageSetup {
            width: pw,
            height: ph,
            margin_top: margin_step,
            margin_bottom: margin_step,
            margin_left: margin_step,
            margin_right: margin_step,
            background: if seed.is_multiple_of(11) { [252, 250, 245] } else { [255, 255, 255] },
            page_numbers: seed.is_multiple_of(2),
        };
        d.header = if seed.is_multiple_of(3) { format!("Benchmark Header #{seed}") } else { String::new() };
        d.footer = if seed.is_multiple_of(4) { format!("Confidential — Doc {seed}") } else { String::new() };

        // 1. Title / Heading paragraph
        let title_style_idx = match seed % 4 {
            0 => 2, // Title
            1 => 4, // Heading 1
            2 => 5, // Heading 2
            _ => 6, // Heading 3
        };
        let mut p_title = Para::new(styles[title_style_idx].chr, ParaFmt { style: title_style_idx, ..styles[title_style_idx].fmt });
        let title_txt = format!("Document {:02}: Cross-Format Native Layout Verification", seed + 1);
        for ch in title_txt.chars() {
            p_title.text.push(ch);
            p_title.styles.push(styles[title_style_idx].chr);
        }
        d.paras.push(p_title);

        // 2. Multi-run styled body paragraph with wrapping, alignment, line spacing, colors, highlights, gradients, and script
        let align = match seed % 4 {
            0 => Align::Left,
            1 => Align::Center,
            2 => Align::Right,
            _ => Align::Justify,
        };
        let line_spacing = [1.0, 1.15, 1.5, 2.0][seed % 4];
        let space_before = (seed % 4) as f32 * 3.0;
        let space_after = ((seed + 1) % 5) as f32 * 3.0;
        let font_idx = (seed % 14) as u8; // system/core fonts 0..14
        let font_size = [10.0, 11.0, 12.0, 14.0, 16.0][seed % 5];
        let base_chr = CharStyle { font: font_idx, size: font_size, ..CharStyle::default() };
        let mut p_body = Para::new(
            base_chr,
            ParaFmt {
                align,
                line_spacing,
                space_before,
                space_after,
                indent: (seed % 3) as u8,
                shading: if seed.is_multiple_of(6) { Some([241, 245, 249]) } else { None },
                ..ParaFmt::default()
            },
        );
        let seg1 = "The quick brown fox jumps over the lazy dog across multiple wrapped lines to verify exact glyph advance widths, kerning, and margin boundaries. ";
        for ch in seg1.chars() {
            p_body.text.push(ch);
            p_body.styles.push(base_chr);
        }
        let bold_italic_chr = CharStyle {
            bold: true,
            italic: seed.is_multiple_of(2),
            underline: seed.is_multiple_of(3),
            strike: seed.is_multiple_of(5),
            color: [180, 35, 24],
            color_to: if seed.is_multiple_of(4) { Some([37, 99, 235]) } else { None },
            highlight: Some([254, 240, 138]),
            highlight_to: if seed.is_multiple_of(4) { Some([251, 207, 232]) } else { None },
            ..base_chr
        };
        for ch in "Highlighted critical clause with exact typographic metrics. ".chars() {
            p_body.text.push(ch);
            p_body.styles.push(bold_italic_chr);
        }
        let script_chr = CharStyle {
            script: if seed.is_multiple_of(2) { 1 } else { -1 },
            ..base_chr
        };
        for ch in "E=mc2".chars() {
            p_body.text.push(ch);
            p_body.styles.push(script_chr);
        }
        p_body.end_style = base_chr;
        d.paras.push(p_body);

        // 3. List items (Bullet, Numbered, and Checklist with nesting levels)
        let list_kind = match seed % 3 {
            0 => ListKind::Bullet,
            1 => ListKind::Number,
            _ => ListKind::Checklist,
        };
        for item_idx in 0..3 {
            let mut p_list = Para::new(
                base_chr,
                ParaFmt {
                    list: list_kind,
                    indent: (item_idx % 2) as u8,
                    checked: (seed + item_idx).is_multiple_of(2),
                    space_after: 2.0,
                    ..ParaFmt::default()
                },
            );
            let item_txt = format!("Structured list item #{} at indent level {} for document {}", item_idx + 1, item_idx % 2, seed + 1);
            for ch in item_txt.chars() {
                p_list.text.push(ch);
                p_list.styles.push(base_chr);
            }
            d.paras.push(p_list);
        }

        // 4. Optional Horizontal Rule or Page Break + Secondary Section
        if seed.is_multiple_of(5) {
            let mut p_hr = Para::new(CharStyle::default(), ParaFmt::default());
            p_hr.fmt.hr = true;
            d.paras.push(p_hr);
        }

        // 5. Multi-column Table with cell shading, borders, vertical alignment, and cell text (every 2nd document)
        if seed.is_multiple_of(2) {
            let last_p = d.paras.len() - 1;
            let last_c = d.paras[last_p].len();
            let first_cell_pos = d.insert_table(Pos::new(last_p, last_c), 3, 3);
            if d.paras.last().is_some_and(|p| p.fmt.table_id == 0 && p.text.is_empty()) {
                d.paras.pop();
            }
            let tid = d.paras[first_cell_pos.p].fmt.table_id;
            if let Some(tbl) = d.table_mut(tid) {
                let banner_rgb = match seed % 6 {
                    0 => [60, 127, 139],
                    2 => [193, 134, 38],
                    _ => [191, 47, 19],
                };
                for (c_idx, cell) in tbl.rows[0].cells.iter_mut().enumerate() {
                    cell.shading = Some(banner_rgb);
                    cell.v_align = CellVAlign::Center;
                    if c_idx == 1 {
                        cell.borders.bottom = Some(BorderSpec { width_pt: 1.5, color: [30, 41, 59] });
                    }
                }
                if let Some(row1) = tbl.rows.get_mut(1) {
                    if let Some(c0) = row1.cells.get_mut(0) {
                        c0.v_align = CellVAlign::Center;
                    }
                    if let Some(c1) = row1.cells.get_mut(1) {
                        c1.shading = Some([255, 255, 204]);
                    }
                }
            }
            for r in 0..3u16 {
                for c in 0..3u16 {
                    if let Some(pi) = d
                        .paras
                        .iter()
                        .position(|p| p.fmt.table_id == tid && p.fmt.table_row == r && p.fmt.table_col == c)
                    {
                        let txt = if r == 0 {
                            format!("Header Col {}", c + 1)
                        } else {
                            format!("Row {} Cell {} (#{seed})", r, c + 1)
                        };
                        let st = if r == 0 {
                            CharStyle {
                                font: font_idx,
                                size: font_size,
                                bold: true,
                                color: [255, 255, 255],
                                ..CharStyle::default()
                            }
                        } else {
                            base_chr
                        };
                        d.paras[pi].fmt.align = if c == 0 && r > 0 { Align::Center } else { Align::Left };
                        d.paras[pi].end_style = st;
                        for ch in txt.chars() {
                            d.paras[pi].text.push(ch);
                            d.paras[pi].styles.push(st);
                        }
                    }
                }
            }
        }

        // 6. Optional Embedded Resized Image Block (every 4th document)
        if seed.is_multiple_of(4) {
            let mut img_buf = image::RgbaImage::new(32, 20);
            for (x, y, px) in img_buf.enumerate_pixels_mut() {
                *px = image::Rgba([(x * 7) as u8, (y * 11) as u8, (seed * 5) as u8, 255]);
            }
            let mut raw_png = Vec::new();
            let _ = image::DynamicImage::ImageRgba8(img_buf).write_to(&mut std::io::Cursor::new(&mut raw_png), image::ImageFormat::Png);
            if let Ok(mut doc_img) = DocImage::from_file_bytes(1, format!("fig_{seed}.png"), &raw_png, 3.0) {
                doc_img.width_in = 2.5 + (seed % 3) as f32 * 0.25;
                doc_img.height_in = 1.5 + (seed % 2) as f32 * 0.25;
                doc_img.caption = format!("Figure {}: Calibrated layout test diagram", seed + 1);
                d.images.push(doc_img);
                let mut p_img = Para::new(CharStyle::default(), ParaFmt::default());
                p_img.fmt.image_id = 1;
                p_img.fmt.align = if seed.is_multiple_of(8) { Align::Center } else { Align::Left };
                for ch in format!("Figure {}: Calibrated layout test diagram", seed + 1).chars() {
                    p_img.text.push(ch);
                    p_img.styles.push(CharStyle::default());
                }
                d.paras.push(p_img);
            }
        }

        // 6. Concluding paragraph (with optional explicit page break to test multi-page pagination)
        let mut p_end = Para::new(
            base_chr,
            ParaFmt {
                page_break_before: seed.is_multiple_of(3),
                align: Align::Left,
                space_before: 6.0,
                space_after: 4.0,
                ..ParaFmt::default()
            },
        );
        let lid = d.add_link("https://example.com/syllabary-spec");
        let link_chr = CharStyle {
            color: [11, 87, 208],
            underline: true,
            link: lid,
            ..base_chr
        };
        for ch in "Reference link: ".chars() {
            p_end.text.push(ch);
            p_end.styles.push(base_chr);
        }
        for ch in "Open Specification".chars() {
            p_end.text.push(ch);
            p_end.styles.push(link_chr);
        }
        p_end.end_style = base_chr;
        d.paras.push(p_end);

        d
    }

    fn assert_pinpoint_layout_match(ctx: &eframe::egui::Context, book: &crate::fonts::FontBook, ref_doc: &Document, loaded_doc: &Document, format_name: &str, doc_idx: usize) {
        let ref_layout = crate::layout::build(ctx, ref_doc, book, 1.0, false);
        let loaded_layout = crate::layout::build(ctx, loaded_doc, book, 1.0, false);

        assert_eq!(
            loaded_layout.pages, ref_layout.pages,
            "[{format_name} doc #{doc_idx}] page count mismatch: got {}, expected {}",
            loaded_layout.pages, ref_layout.pages
        );
        assert!(
            (loaded_layout.page_size.x - ref_layout.page_size.x).abs() < 0.1
                && (loaded_layout.page_size.y - ref_layout.page_size.y).abs() < 0.1,
            "[{format_name} doc #{doc_idx}] page_size mismatch: got {:?}, expected {:?}",
            loaded_layout.page_size,
            ref_layout.page_size
        );
        assert!(
            (loaded_layout.content_size.x - ref_layout.content_size.x).abs() < 0.1
                && (loaded_layout.content_size.y - ref_layout.content_size.y).abs() < 0.1,
            "[{format_name} doc #{doc_idx}] content_size mismatch: got {:?}, expected {:?}",
            loaded_layout.content_size,
            ref_layout.content_size
        );
        assert!(
            (loaded_layout.content_offset.x - ref_layout.content_offset.x).abs() < 0.1
                && (loaded_layout.content_offset.y - ref_layout.content_offset.y).abs() < 0.1,
            "[{format_name} doc #{doc_idx}] margin content_offset mismatch: got {:?}, expected {:?}",
            loaded_layout.content_offset,
            ref_layout.content_offset
        );
        assert_eq!(
            loaded_layout.lines.len(),
            ref_layout.lines.len(),
            "[{format_name} doc #{doc_idx}] positioned line count mismatch: got {}, expected {}",
            loaded_layout.lines.len(),
            ref_layout.lines.len()
        );
        for (li, (la, lb)) in ref_layout.lines.iter().zip(loaded_layout.lines.iter()).enumerate() {
            assert_eq!(
                (lb.para, lb.start, lb.end, lb.page),
                (la.para, la.start, la.end, la.page),
                "[{format_name} doc #{doc_idx} line #{li}] wrap/page span mismatch"
            );
            assert!(
                (lb.x - la.x).abs() < 0.15 && (lb.y - la.y).abs() < 0.15 && (lb.height - la.height).abs() < 0.15,
                "[{format_name} doc #{doc_idx} line #{li}] geometric coordinate mismatch: got (x={:.2}, y={:.2}, h={:.2}), expected (x={:.2}, y={:.2}, h={:.2})",
                lb.x,
                lb.y,
                lb.height,
                la.x,
                la.y,
                la.height
            );
            assert_eq!(
                lb.marker.is_some(),
                la.marker.is_some(),
                "[{format_name} doc #{doc_idx} line #{li}] list marker presence mismatch"
            );
            if la.marker.is_some() {
                assert!(
                    (lb.marker_x - la.marker_x).abs() < 0.15,
                    "[{format_name} doc #{doc_idx} line #{li}] list marker_x mismatch"
                );
            }
        }
        assert_eq!(
            loaded_layout.cells.len(),
            ref_layout.cells.len(),
            "[{format_name} doc #{doc_idx}] table cell count mismatch: got {}, expected {}",
            loaded_layout.cells.len(),
            ref_layout.cells.len()
        );
        for (ci, (ca, cb)) in ref_layout.cells.iter().zip(loaded_layout.cells.iter()).enumerate() {
            assert_eq!(
                (cb.row, cb.col, cb.page, cb.shading),
                (ca.row, ca.col, ca.page, ca.shading),
                "[{format_name} doc #{doc_idx} cell #{ci}] table cell metadata/shading mismatch"
            );
            assert!(
                (cb.rect.min.x - ca.rect.min.x).abs() < 0.25
                    && (cb.rect.min.y - ca.rect.min.y).abs() < 0.25
                    && (cb.rect.width() - ca.rect.width()).abs() < 0.25
                    && (cb.rect.height() - ca.rect.height()).abs() < 0.25,
                "[{format_name} doc #{doc_idx} cell #{ci}] table cell rect mismatch: got {:?}, expected {:?}",
                cb.rect,
                ca.rect
            );
        }
    }

    #[test]
    fn chairman_outline_docx_matches_native_pdf_layout() {
        let docx_path = std::path::Path::new(
            r"C:\Users\asten\.gemini\antigravity\brain\d16bac67-640a-4a09-ac26-df3d189a982a\.user_uploaded\media_1794148162920_68d202b0.docx",
        );
        if !docx_path.exists() {
            return;
        }
        let raw = std::fs::read(docx_path).expect("read Chairman Outline docx");
        let doc = crate::docx::read(&raw).expect("parse Chairman Outline docx");

        // Must parse both tables (2-column header banner table + 3-column schedule table)
        assert_eq!(doc.tables.len(), 2, "Expected 2 tables in Chairman Outline.docx");
        assert_eq!(doc.tables[0].rows.len(), 1);
        assert_eq!(doc.tables[0].rows[0].cells.len(), 2);
        assert_eq!(doc.tables[1].rows.len(), 21);

        // Must parse external footer part (word/footer1.xml) with right alignment and italics
        assert_eq!(doc.footer, "Printed by NWS Desktop 2026/11/08");
        assert_eq!(doc.footer_align, Align::Right);
        assert!(doc.footer_italic);

        // Build layout and verify it fits on 1 single page just like the native PDF
        let ctx = eframe::egui::Context::default();
        let book = crate::fonts::install(&ctx);
        let mut out = ctx.run_ui(eframe::egui::RawInput::default(), |_| {});
        out.textures_delta.clear();

        let lay = crate::layout::build(&ctx, &doc, &book, 1.0, false);
        assert_eq!(lay.pages, 1, "Chairman Outline must fit on 1 single page (got {} pages)", lay.pages);

        // Verify colored section banner cells (#3C7F8B teal, #C18626 gold, #BF2F13 crimson) and pale yellow song/prayer cells (#FFFFCC)
        assert!(lay.cells.iter().any(|c| c.shading == Some([60, 127, 139])), "Missing teal Treasures banner cell");
        assert!(lay.cells.iter().any(|c| c.shading == Some([193, 134, 38])), "Missing gold Field Ministry banner cell");
        assert!(lay.cells.iter().any(|c| c.shading == Some([191, 47, 19])), "Missing crimson Living as Christians banner cell");
        assert!(lay.cells.iter().any(|c| c.shading == Some([255, 255, 204])), "Missing pale yellow song/prayer cell");

        // Verify spacer rows have left/right borders suppressed (`None`)
        let spacer_row_3: Vec<_> = lay.cells.iter().filter(|c| c.table_id == 2 && c.row == 3).collect();
        assert!(!spacer_row_3.is_empty());
        for sc in spacer_row_3 {
            assert!(sc.borders.left.is_none() && sc.borders.right.is_none(), "Spacer row should have nil left/right borders");
        }

        // Also verify round-tripping Chairman Outline through .docx, .wpx, .gdoc, and .rtf preserves its 1-page table layout!
        let re_docx = crate::docx::read(&crate::docx::write(&doc)).expect("re-read docx");
        assert_pinpoint_layout_match(&ctx, &book, &doc, &re_docx, "ChairmanOutline .docx", 0);

        let re_wpx = from_json(&to_json(&doc)).expect("re-read wpx");
        assert_pinpoint_layout_match(&ctx, &book, &doc, &re_wpx, "ChairmanOutline .wpx", 0);

        let re_gdoc = from_gdoc(&to_gdoc(&doc)).expect("re-read gdoc");
        assert_pinpoint_layout_match(&ctx, &book, &doc, &re_gdoc, "ChairmanOutline .gdoc", 0);

        let re_rtf = from_rtf(&to_rtf(&doc));
        assert_pinpoint_layout_match(&ctx, &book, &doc, &re_rtf, "ChairmanOutline .rtf", 0);

        // Render high-DPI WYSIWYG page image to artifact directory for visual side-by-side inspection
        let pages = crate::pdf::render_pages_rgba(&ctx, &doc, &book, 2.0);
        assert_eq!(pages.len(), 1);
        let (w, h, rgba) = &pages[0];
        if let Some(img) = image::RgbaImage::from_raw(*w, *h, rgba.clone()) {
            let out_path = std::path::Path::new(
                r"C:\Users\asten\.gemini\antigravity\brain\d16bac67-640a-4a09-ac26-df3d189a982a\chairman_outline_scribepro.png",
            );
            let _ = img.save(out_path);
        }
    }

    #[test]
    fn layout_fidelity_50_docs_per_filetype() {
        // Initialize headless egui context for deterministic layout & WYSIWYG rasterization
        let ctx = eframe::egui::Context::default();
        let book = crate::fonts::install(&ctx);
        let mut out = ctx.run_ui(eframe::egui::RawInput::default(), |_| {});
        out.textures_delta.clear();

        for i in 0..50 {
            let ref_doc = make_reference_document(i);

            // 1. Native Syllabary (.wpx) — 50 documents
            let wpx_str = to_json(&ref_doc);
            let wpx_doc = from_json(&wpx_str).expect("wpx parse");
            assert_pinpoint_layout_match(&ctx, &book, &ref_doc, &wpx_doc, ".wpx", i);

            // 2. Microsoft Word (.docx) — 50 documents
            let docx_bytes = crate::docx::write(&ref_doc);
            let docx_doc = crate::docx::read(&docx_bytes).expect("docx parse");
            assert_pinpoint_layout_match(&ctx, &book, &ref_doc, &docx_doc, ".docx", i);

            // 3. Google Docs (.gdoc hybrid & pure Google Docs REST API v1 `body.content`) — 50 documents
            let gdoc_str = to_gdoc(&ref_doc);
            let gdoc_hybrid = from_gdoc(&gdoc_str).expect("gdoc hybrid parse");
            assert_pinpoint_layout_match(&ctx, &book, &ref_doc, &gdoc_hybrid, ".gdoc (hybrid)", i);

            let mut pure_api_json: serde_json::Value = serde_json::from_str(&gdoc_str).expect("gdoc json");
            pure_api_json.as_object_mut().unwrap().remove("paras");
            let gdoc_api_doc = from_gdoc(&pure_api_json.to_string()).expect("gdoc pure API v1 parse");
            assert_pinpoint_layout_match(&ctx, &book, &ref_doc, &gdoc_api_doc, ".gdoc (API v1)", i);

            // 4. Rich Text Format (.rtf) — 50 documents
            let rtf_str = to_rtf(&ref_doc);
            let rtf_doc = from_rtf(&rtf_str);
            assert_pinpoint_layout_match(&ctx, &book, &ref_doc, &rtf_doc, ".rtf", i);

            // 5. Markdown (.md) — 50 documents (structural + idempotent layout convergence)
            let md_str1 = to_markdown(&ref_doc);
            let md_doc1 = from_markdown(&md_str1);
            let md_str2 = to_markdown(&md_doc1);
            let md_doc2 = from_markdown(&md_str2);
            assert_eq!(md_str1, md_str2, "[.md doc #{i}] markdown serialization should be idempotent");
            assert_pinpoint_layout_match(&ctx, &book, &md_doc1, &md_doc2, ".md", i);

            // 6. Plain Text (.txt) — 50 documents (exact line/paragraph & layout convergence)
            let txt_str1 = ref_doc.plain_text();
            let txt_doc1 = from_plain(&txt_str1);
            let txt_str2 = txt_doc1.plain_text();
            let txt_doc2 = from_plain(&txt_str2);
            assert_eq!(txt_str1, txt_str2, "[.txt doc #{i}] plain text roundtrip mismatch");
            assert_pinpoint_layout_match(&ctx, &book, &txt_doc1, &txt_doc2, ".txt", i);

            // Verify WYSIWYG PDF pixel rasterizer on representative documents across formats
            if i % 10 == 0 {
                let pages_ref = crate::pdf::render_pages_rgba(&ctx, &ref_doc, &book, 1.0);
                let pages_docx = crate::pdf::render_pages_rgba(&ctx, &docx_doc, &book, 1.0);
                let pages_gdoc = crate::pdf::render_pages_rgba(&ctx, &gdoc_api_doc, &book, 1.0);
                let pages_rtf = crate::pdf::render_pages_rgba(&ctx, &rtf_doc, &book, 1.0);
                assert_eq!(pages_ref.len(), pages_docx.len());
                assert_eq!(pages_ref.len(), pages_gdoc.len());
                assert_eq!(pages_ref.len(), pages_rtf.len());
                for p_idx in 0..pages_ref.len() {
                    assert_eq!(pages_ref[p_idx].0, pages_docx[p_idx].0);
                    assert_eq!(pages_ref[p_idx].1, pages_docx[p_idx].1);
                    let diff_docx = pages_ref[p_idx]
                        .2
                        .iter()
                        .zip(pages_docx[p_idx].2.iter())
                        .filter(|(a, b)| a != b)
                        .count();
                    assert_eq!(
                        diff_docx, 0,
                        "[doc #{i} page #{p_idx}] .docx rendered RGBA pixels differ in {diff_docx} bytes from reference!"
                    );
                    let diff_gdoc = pages_ref[p_idx]
                        .2
                        .iter()
                        .zip(pages_gdoc[p_idx].2.iter())
                        .filter(|(a, b)| a != b)
                        .count();
                    assert_eq!(
                        diff_gdoc, 0,
                        "[doc #{i} page #{p_idx}] .gdoc (API v1) rendered RGBA pixels differ in {diff_gdoc} bytes from reference!"
                    );
                }
            }
        }
    }

    #[test]
    fn table_features_shading_merge_align_and_math_sig_figs() {
        let mut doc = Document::default();
        let pos = doc.insert_table(Pos::new(0, 0), 6, 3);
        let tid = doc.paras[pos.p].fmt.table_id;
        assert!(tid > 0);

        // 1. Shade & align cells, merge row 0 into a banner, and split/re-merge
        doc.table_set_row_shading(tid, 0, Some([60, 127, 139]));
        doc.set_cell_text(tid, 0, 0, "Quarterly Budget Summary");
        assert!(doc.table_merge_row(tid, 0));
        assert_eq!(doc.table(tid).unwrap().rows[0].cells.len(), 1);
        assert_eq!(doc.table(tid).unwrap().rows[0].cells[0].grid_span, 3);
        assert!(doc.table_split_cell(tid, 0, 0));
        assert_eq!(doc.table(tid).unwrap().rows[0].cells.len(), 3);
        assert!(doc.table_merge_right(tid, 0, 0));
        assert_eq!(doc.table(tid).unwrap().rows[0].cells[0].grid_span, 2);

        // 2. Vertical and Horizontal cell alignment
        doc.table_set_cell_valign(tid, 1, 1, CellVAlign::Center);
        doc.table_set_cell_halign(tid, 1, 1, Align::Right);
        assert_eq!(doc.table(tid).unwrap().rows[1].cells[1].v_align, CellVAlign::Center);

        // 3. Exact user example: 10 and 5.23 -> Sum = 15.23 (keeping significant figures / precision of greater)
        doc.set_cell_text(tid, 1, 1, "10");
        doc.set_cell_text(tid, 2, 1, "5.23");
        doc.table_set_cell_formula(tid, 3, 1, Some("=SUM(B2, B3)".into()));
        assert_eq!(doc.cell_text(tid, 3, 1), "15.23");

        // 4. Subtotal, Average, Median, and Grand Total (SUM(ABOVE) summing SUBTOTALs)
        doc.table_set_cell_formula(tid, 3, 1, Some("=SUBTOTAL(ABOVE)".into()));
        assert_eq!(doc.cell_text(tid, 3, 1), "15.23");
        doc.set_cell_text(tid, 4, 1, "4.77");
        doc.table_set_cell_formula(tid, 5, 1, Some("=AVERAGE(B2, B3)".into()));
        assert_eq!(doc.cell_text(tid, 5, 1), "7.62");
        doc.table_set_cell_formula(tid, 5, 2, Some("=MEDIAN(B2, B3, B5)".into()));
        assert_eq!(doc.cell_text(tid, 5, 2), "5.23");

        // 5. Live recalculation when input cell changes
        doc.set_cell_text(tid, 1, 1, "20");
        doc.evaluate_table_formulas(tid);
        assert_eq!(doc.cell_text(tid, 3, 1), "25.23");
    }

    #[test]
    fn table_valign_row_col_select_resize_autofit_and_wrap() {
        let ctx = eframe::egui::Context::default();
        let book = crate::fonts::install(&ctx);
        let mut out = ctx.run_ui(eframe::egui::RawInput::default(), |_| {});
        out.textures_delta.clear();

        let mut doc = Document::default();
        let pos = doc.insert_table(Pos::new(0, 0), 3, 3);
        let tid = doc.paras[pos.p].fmt.table_id;

        // 1. Vertical alignment (`Top`, `Center`, `Bottom`) must produce distinct y positions inside the cell
        doc.table_resize_row(tid, 0, 50.0); // 50pt row height
        doc.set_cell_text(tid, 0, 0, "Top Cell");
        doc.set_cell_text(tid, 0, 1, "Mid Cell");
        doc.set_cell_text(tid, 0, 2, "Bot Cell");
        doc.table_set_cell_valign(tid, 0, 0, CellVAlign::Top);
        doc.table_set_cell_valign(tid, 0, 1, CellVAlign::Center);
        doc.table_set_cell_valign(tid, 0, 2, CellVAlign::Bottom);

        let lay = crate::layout::build(&ctx, &doc, &book, 1.0, false);
        let y_top = lay
            .lines
            .iter()
            .find(|l| doc.paras[l.para].fmt.table_id == tid && doc.paras[l.para].fmt.table_row == 0 && doc.paras[l.para].fmt.table_col == 0)
            .expect("top cell line")
            .y;
        let y_mid = lay
            .lines
            .iter()
            .find(|l| doc.paras[l.para].fmt.table_id == tid && doc.paras[l.para].fmt.table_row == 0 && doc.paras[l.para].fmt.table_col == 1)
            .expect("mid cell line")
            .y;
        let y_bot = lay
            .lines
            .iter()
            .find(|l| doc.paras[l.para].fmt.table_id == tid && doc.paras[l.para].fmt.table_row == 0 && doc.paras[l.para].fmt.table_col == 2)
            .expect("bot cell line")
            .y;
        assert!(
            y_top + 8.0 < y_mid && y_mid + 8.0 < y_bot,
            "Expected Top ({y_top:.1}) < Mid ({y_mid:.1}) < Bot ({y_bot:.1})"
        );

        // 2. Row and Column selection helpers
        assert_eq!(doc.table_row_cells(tid, 1), vec![(tid, 1, 0), (tid, 1, 1), (tid, 1, 2)]);
        assert_eq!(doc.table_col_cells(tid, 2), vec![(tid, 0, 2), (tid, 1, 2), (tid, 2, 2)]);

        // 3. Wrap text (`true` vs `false`)
        doc.table_resize_col(tid, 1, 0, 0.8); // narrow 0.8 in column
        doc.set_cell_text(tid, 1, 0, "This is a long sentence inside a narrow table cell that wraps onto multiple lines");
        doc.table_set_cell_wrap(tid, 1, 0, true);
        let lay_wrapped = crate::layout::build(&ctx, &doc, &book, 1.0, false);
        let wrapped_line_count = lay_wrapped
            .lines
            .iter()
            .filter(|l| doc.paras[l.para].fmt.table_id == tid && doc.paras[l.para].fmt.table_row == 1 && doc.paras[l.para].fmt.table_col == 0)
            .count();
        assert!(wrapped_line_count >= 2, "Expected wrapped cell to span >= 2 lines, got {wrapped_line_count}");

        doc.table_set_cell_wrap(tid, 1, 0, false);
        let lay_unwrapped = crate::layout::build(&ctx, &doc, &book, 1.0, false);
        let unwrapped_line_count = lay_unwrapped
            .lines
            .iter()
            .filter(|l| doc.paras[l.para].fmt.table_id == tid && doc.paras[l.para].fmt.table_row == 1 && doc.paras[l.para].fmt.table_col == 0)
            .count();
        assert_eq!(unwrapped_line_count, 1, "Expected unwrapped cell (`wrap_text = false`) to stay on 1 single line");

        // 4. Double-click edge Auto-Fit Column Width & Row Height
        let auto_w = crate::layout::measure_column_autofit_width_in(&ctx, &doc, &book, tid, 1, 0).expect("autofit width");
        assert!(auto_w > 1.8, "Expected autofit column width ({auto_w:.2} in) to expand to fit long sentence");
        doc.table_resize_col(tid, 1, 0, auto_w);
        assert!((doc.table(tid).unwrap().rows[1].cells[0].width_in - auto_w).abs() < 0.01);

        let auto_h = crate::layout::measure_row_autofit_height_pt(&ctx, &doc, &book, tid, 0).expect("autofit height");
        doc.table_resize_row(tid, 0, auto_h);
        assert!(auto_h < 30.0, "Expected autofit row height ({auto_h:.1} pt) to shrink from 50pt down to tight 1-line height");
    }
}

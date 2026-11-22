//! Arithmetic for table cell formulas: `=B2*C2`, `=(B1-B2)/B3`, `=SUM(ABOVE)*1.07`, `=PRODUCT(B2:D2)`.
//!
//! Grammar (case-insensitive):
//! ```text
//! expr   := term (('+' | '-') term)*
//! term   := unary (('*' | '×' | 'x' between operands | '/' | '÷') unary)*
//! unary  := ('-' | '+') unary | power
//! power  := atom ('^' unary)?
//! atom   := number | percent | cell | func '(' args ')' | '(' expr ')'
//! ```
//! Cell references use the table's letters-and-numbers grid (`A1` is the top-left cell).

use crate::doc::{parse_cell_coord, ParsedCellNumber};

/// Functions understood inside expressions. Aggregates reuse the table's own evaluator.
pub const FUNCTIONS: &[&str] = &[
    "SUM", "TOTAL", "SUBTOTAL", "AVERAGE", "AVG", "MEDIAN", "PRODUCT", "MIN", "MAX", "ROUND", "ABS",
];

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Cell(usize, usize),
    Func(String, String), // name, raw argument text
    Op(char),
    LParen,
    RParen,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MathError {
    /// The text isn't a formula this evaluator understands.
    Syntax,
    DivByZero,
}

/// Values a formula can see: cell contents and aggregate results.
pub trait CellSource {
    /// Parsed number in cell `(row, col)`, or None for empty / non-numeric cells.
    fn number(&self, row: usize, col: usize) -> Option<ParsedCellNumber>;
    /// Result of an aggregate call such as `SUM(ABOVE)` evaluated for the target cell.
    fn aggregate(&self, call: &str) -> Option<ParsedCellNumber>;
    /// Cells named by function arguments (`A1:B3`, `B2,C4`, `ABOVE`, `LEFT`).
    fn arg_cells(&self, args: &str) -> Vec<(usize, usize)>;
}

fn tokenize(src: &str) -> Result<Vec<Tok>, MathError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == ',')
            {
                i += 1;
            }
            let txt: String = chars[start..i].iter().filter(|c| **c != ',').collect();
            let mut v: f64 = txt.parse().map_err(|_| MathError::Syntax)?;
            if chars.get(i) == Some(&'%') {
                v /= 100.0;
                i += 1;
            }
            out.push(Tok::Num(v));
            continue;
        }
        if matches!(c, '$' | '€' | '£' | '¥') {
            i += 1; // currency symbols on constants are decoration
            continue;
        }
        if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let word: String = chars[start..i]
                .iter()
                .collect::<String>()
                .to_ascii_uppercase();
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if chars.get(j) == Some(&'(') {
                if !FUNCTIONS.contains(&word.as_str()) {
                    return Err(MathError::Syntax);
                }
                // capture the balanced argument text
                let mut depth = 0usize;
                let mut k = j;
                while k < chars.len() {
                    match chars[k] {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                if k >= chars.len() {
                    return Err(MathError::Syntax);
                }
                let args: String = chars[j + 1..k].iter().collect();
                out.push(Tok::Func(word, args));
                i = k + 1;
                continue;
            }
            // "x" between two operands is a multiplication sign (e.g. "=B2 x C2")
            if word == "X"
                && matches!(
                    out.last(),
                    Some(Tok::Num(_) | Tok::Cell(..) | Tok::RParen | Tok::Func(..))
                )
            {
                out.push(Tok::Op('*'));
                continue;
            }
            let (r, c) = parse_cell_coord(&word).ok_or(MathError::Syntax)?;
            out.push(Tok::Cell(r, c));
            continue;
        }
        let op = match c {
            '+' | '-' | '*' | '/' | '^' => c,
            '×' | '·' => '*',
            '÷' => '/',
            '−' | '–' => '-',
            '(' => {
                out.push(Tok::LParen);
                i += 1;
                continue;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
                continue;
            }
            _ => return Err(MathError::Syntax),
        };
        out.push(Tok::Op(op));
        i += 1;
    }
    if out.is_empty() {
        return Err(MathError::Syntax);
    }
    Ok(out)
}

struct Parser<'a, S: CellSource> {
    toks: Vec<Tok>,
    pos: usize,
    src: &'a S,
    /// Numbers read from cells (their decimals and currency shape the result's format).
    inputs: Vec<ParsedCellNumber>,
}

impl<S: CellSource> Parser<'_, S> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn expr(&mut self) -> Result<f64, MathError> {
        let mut v = self.term()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.pos += 1;
            let r = self.term()?;
            v = if op == '+' { v + r } else { v - r };
        }
        Ok(v)
    }

    fn term(&mut self) -> Result<f64, MathError> {
        let mut v = self.unary()?;
        loop {
            match self.peek().cloned() {
                Some(Tok::Op(op @ ('*' | '/'))) => {
                    self.pos += 1;
                    let r = self.unary()?;
                    if op == '*' {
                        v *= r;
                    } else {
                        if r == 0.0 {
                            return Err(MathError::DivByZero);
                        }
                        v /= r;
                    }
                }
                // implicit multiplication: "2(B1+B2)" or "(A1)(A2)"
                Some(Tok::LParen) => {
                    let r = self.unary()?;
                    v *= r;
                }
                _ => return Ok(v),
            }
        }
    }

    fn unary(&mut self) -> Result<f64, MathError> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                Ok(-self.unary()?)
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<f64, MathError> {
        let base = self.atom()?;
        if let Some(Tok::Op('^')) = self.peek() {
            self.pos += 1;
            let e = self.unary()?;
            return Ok(base.powf(e));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<f64, MathError> {
        let tok = self.peek().cloned().ok_or(MathError::Syntax)?;
        self.pos += 1;
        match tok {
            Tok::Num(v) => Ok(v),
            Tok::Cell(r, c) => Ok(match self.src.number(r, c) {
                Some(n) => {
                    let v = n.value;
                    self.inputs.push(n);
                    v
                }
                None => 0.0, // empty cells count as zero, like a spreadsheet
            }),
            Tok::LParen => {
                let v = self.expr()?;
                if self.peek() != Some(&Tok::RParen) {
                    return Err(MathError::Syntax);
                }
                self.pos += 1;
                Ok(v)
            }
            Tok::Func(name, args) => self.func(&name, &args),
            _ => Err(MathError::Syntax),
        }
    }

    fn func(&mut self, name: &str, args: &str) -> Result<f64, MathError> {
        match name {
            "PRODUCT" | "MIN" | "MAX" => {
                let mut vals = Vec::new();
                for (r, c) in self.src.arg_cells(args) {
                    if let Some(n) = self.src.number(r, c) {
                        vals.push(n.value);
                        self.inputs.push(n);
                    }
                }
                // plain numbers / nested expressions are allowed too: PRODUCT(B2, 1.5)
                for part in split_args(args) {
                    if self.src.arg_cells(&part).is_empty() && !part.trim().is_empty() {
                        vals.push(self.sub_expr(&part)?);
                    }
                }
                if vals.is_empty() {
                    return Ok(0.0);
                }
                Ok(match name {
                    "PRODUCT" => vals.iter().product(),
                    "MIN" => vals.iter().copied().fold(f64::INFINITY, f64::min),
                    _ => vals.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                })
            }
            "ROUND" => {
                let parts = split_args(args);
                let v = self.sub_expr(parts.first().ok_or(MathError::Syntax)?)?;
                let d = parts
                    .get(1)
                    .map_or(Ok(0.0), |p| self.sub_expr(p))?
                    .round()
                    .clamp(0.0, 10.0) as i32;
                let f = 10f64.powi(d);
                Ok((v * f).round() / f)
            }
            "ABS" => Ok(self.sub_expr(args)?.abs()),
            _ => {
                let n = self
                    .src
                    .aggregate(&format!("{name}({args})"))
                    .ok_or(MathError::Syntax)?;
                let v = n.value;
                self.inputs.push(n);
                Ok(v)
            }
        }
    }

    fn sub_expr(&mut self, text: &str) -> Result<f64, MathError> {
        let toks = tokenize(text)?;
        let mut p = Parser {
            toks,
            pos: 0,
            src: self.src,
            inputs: Vec::new(),
        };
        let v = p.expr()?;
        if p.pos != p.toks.len() {
            return Err(MathError::Syntax);
        }
        self.inputs.extend(p.inputs);
        Ok(v)
    }
}

fn split_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in args.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' | ';' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

/// Evaluates `formula` (with or without a leading `=`), returning the value and the cell numbers
/// it read (for formatting).
pub fn evaluate<S: CellSource>(
    formula: &str,
    src: &S,
) -> Result<(f64, Vec<ParsedCellNumber>), MathError> {
    let body = formula.trim().strip_prefix('=').unwrap_or(formula.trim());
    let toks = tokenize(body)?;
    let mut p = Parser {
        toks,
        pos: 0,
        src,
        inputs: Vec::new(),
    };
    let v = p.expr()?;
    if p.pos != p.toks.len() || !v.is_finite() {
        return Err(MathError::Syntax);
    }
    Ok((v, p.inputs))
}

/// True when `text` (starting with `=`) is an arithmetic formula this module can evaluate.
pub fn looks_like_expression(text: &str) -> bool {
    let Some(body) = text.trim().strip_prefix('=') else {
        return false;
    };
    let Ok(toks) = tokenize(body) else {
        return false;
    };
    // must reference a cell or a function, or combine numbers with an operator
    toks.iter()
        .any(|t| matches!(t, Tok::Cell(..) | Tok::Func(..)))
        || toks.iter().any(|t| matches!(t, Tok::Op(_)))
}

/// Formats an arithmetic result: money and decimals follow the cells it used; whole numbers stay
/// whole; other fractions show up to four decimals.
pub fn format_result(v: f64, inputs: &[ParsedCellNumber]) -> String {
    let curr = inputs.iter().find_map(|x| x.currency);
    let max_dec = inputs.iter().map(|x| x.decimal_places).max().unwrap_or(0);
    let num = if max_dec > 0 {
        let s = format!(
            "{:.*}",
            max_dec,
            (v * 10f64.powi(max_dec as i32)).round() / 10f64.powi(max_dec as i32)
        );
        if s.starts_with('-') && s.trim_start_matches(['-', '0', '.']).is_empty() {
            s[1..].to_string()
        } else {
            s
        }
    } else if (v - v.round()).abs() < 1e-9 {
        format!("{:.0}", v.round())
    } else {
        let s = format!("{:.4}", (v * 10_000.0).round() / 10_000.0);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    match (curr, num.strip_prefix('-')) {
        (Some(c), Some(rest)) => format!("-{c}{rest}"),
        (Some(c), None) => format!("{c}{num}"),
        (None, _) => num,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::parse_cell_number;
    use std::collections::HashMap;

    struct Grid(HashMap<(usize, usize), &'static str>);

    impl CellSource for Grid {
        fn number(&self, r: usize, c: usize) -> Option<ParsedCellNumber> {
            self.0.get(&(r, c)).and_then(|t| parse_cell_number(t))
        }
        fn aggregate(&self, call: &str) -> Option<ParsedCellNumber> {
            // SUM over column B rows 1..3 for the test
            if call.starts_with("SUM") {
                parse_cell_number("60")
            } else {
                None
            }
        }
        fn arg_cells(&self, args: &str) -> Vec<(usize, usize)> {
            args.split(',')
                .filter_map(|a| parse_cell_coord(a.trim()))
                .collect()
        }
    }

    fn grid() -> Grid {
        Grid(HashMap::from([
            ((0, 1), "10"),
            ((1, 1), "20"),
            ((2, 1), "30"),
            ((0, 2), "$12.50"),
            ((1, 2), "4"),
            ((2, 2), "0"),
        ]))
    }

    fn eval(f: &str) -> String {
        match evaluate(f, &grid()) {
            Ok((v, inputs)) => format_result(v, &inputs),
            Err(MathError::DivByZero) => "#DIV/0!".into(),
            Err(MathError::Syntax) => "ERR".into(),
        }
    }

    #[test]
    fn arithmetic_across_cells() {
        assert_eq!(eval("=B1+B2"), "30");
        assert_eq!(eval("=B3-B1"), "20");
        assert_eq!(eval("=B2*B3"), "600");
        assert_eq!(eval("=B3/B2"), "1.5");
        assert_eq!(eval("=B1/3"), "3.3333");
        assert_eq!(eval("=(B1+B2)*B3"), "900");
        assert_eq!(eval("=B1+B2*B3"), "610");
        assert_eq!(eval("=-B1+5"), "-5");
        assert_eq!(eval("=C1*C2"), "$50.00");
        assert_eq!(eval("=C1 x C2"), "$50.00");
        assert_eq!(eval("=B3÷B1"), "3");
        assert_eq!(eval("=B1*10%"), "1");
        assert_eq!(eval("=SUM(ABOVE)*2"), "120");
        assert_eq!(eval("=PRODUCT(B1,B2)"), "200");
        assert_eq!(eval("=ROUND(B1/3, 2)"), "3.33");
        assert_eq!(eval("=B1/C3"), "#DIV/0!");
        assert_eq!(eval("=hello"), "ERR");
        assert_eq!(eval("=B1+"), "ERR");
        assert!(looks_like_expression("=A1*B2"));
        assert!(!looks_like_expression("=hello world"));
        assert!(!looks_like_expression("no formula"));
    }
}

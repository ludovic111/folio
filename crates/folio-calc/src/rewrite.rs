//! Rewriting formulas as text: copy and fill, inserted and deleted rows and columns, renamed
//! sheets. Only the references change; spacing, case and everything else are kept. A leading
//! `=` is accepted and kept.

use crate::addr::{MAX_COLS, MAX_ROWS, Part, RefBody, SheetRange, order, quote_sheet_name};
use crate::lexer::{RefTok, Tok, lex};

/// Rewrites each reference with `f`, which returns the new text of the reference body
/// (`None` turns the reference into `#REF!`), or leaves it alone when `f` is not applicable.
fn rewrite(formula: &str, f: &mut dyn FnMut(&RefTok, &str, &str) -> Option<String>) -> String {
    let (lead, body) = match formula.strip_prefix('=') {
        Some(rest) => ("=", rest),
        None => ("", formula),
    };
    let mut out = String::with_capacity(formula.len() + 8);
    out.push_str(lead);
    for token in lex(body) {
        let text = &body[token.start..token.end];
        match &token.tok {
            Tok::Ref(r) => {
                let prefix = &text[..r.prefix_len];
                let rest = &text[r.prefix_len..];
                match f(r, prefix, rest) {
                    Some(new) => out.push_str(&new),
                    None => out.push_str(text),
                }
            }
            _ => out.push_str(text),
        }
    }
    out
}

fn moved(x: u32, d: i64, abs: bool, max: u32) -> Option<u32> {
    if abs {
        return Some(x);
    }
    let n = x as i64 + d;
    (0..max as i64).contains(&n).then_some(n as u32)
}

fn move_part(p: Part, drow: i64, dcol: i64) -> Option<Part> {
    Some(Part {
        row: moved(p.row, drow, p.row_abs, MAX_ROWS)?,
        col: moved(p.col, dcol, p.col_abs, MAX_COLS)?,
        ..p
    })
}

/// Moves a formula as when copying it `drow` rows down and `dcol` columns right: relative
/// references move, `$`-anchored parts stay, and references pushed off the grid become `#REF!`.
pub fn translate(formula: &str, drow: i64, dcol: i64) -> String {
    rewrite(formula, &mut |r, prefix, _| {
        let body = match r.body {
            RefBody::Cell(p) => move_part(p, drow, dcol).map(RefBody::Cell),
            RefBody::Area(a, b) => match (move_part(a, drow, dcol), move_part(b, drow, dcol)) {
                (Some(a), Some(b)) => {
                    let (a, b) = order(a, b);
                    Some(RefBody::Area(a, b))
                }
                _ => None,
            },
            RefBody::Cols(a, b) => match (move_part(a, 0, dcol), move_part(b, 0, dcol)) {
                (Some(a), Some(b)) => {
                    let (a, b) = order(a, b);
                    Some(RefBody::Cols(a, b))
                }
                _ => None,
            },
            RefBody::Rows(a, b) => match (move_part(a, drow, 0), move_part(b, drow, 0)) {
                (Some(a), Some(b)) => {
                    let (a, b) = order(a, b);
                    Some(RefBody::Rows(a, b))
                }
                _ => None,
            },
        };
        Some(match body {
            Some(body) => format!("{prefix}{}", body.render()),
            None => format!("{prefix}#REF!"),
        })
    })
}

/// Which way rows or columns are inserted or deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Rows,
    Cols,
}

/// Shifts one coordinate for an insertion (`count > 0`) or deletion (`count < 0`) at `at`.
fn shift_one(x: u32, at: u32, count: i64, max: u32) -> Option<u32> {
    if count >= 0 {
        if x < at {
            return Some(x);
        }
        let n = x as i64 + count;
        return (n < max as i64).then_some(n as u32);
    }
    let k = -count;
    let end = at as i64 + k;
    if x < at {
        Some(x)
    } else if (x as i64) >= end {
        Some((x as i64 - k) as u32)
    } else {
        None
    }
}

/// Shifts a span [s, e] for an insertion or deletion; `None` when it disappears.
fn shift_span(s: u32, e: u32, at: u32, count: i64, max: u32) -> Option<(u32, u32)> {
    if count >= 0 {
        let s2 = shift_one(s, at, count, max)?;
        let e2 = if e < at { e } else { (e as i64 + count).min(max as i64 - 1) as u32 };
        return Some((s2, e2));
    }
    let k = -count;
    let band_end = at as i64 + k; // first row after the deleted band
    let (s64, e64) = (s as i64, e as i64);
    if e64 < at as i64 {
        return Some((s, e));
    }
    if s64 >= band_end {
        return Some(((s64 - k) as u32, (e64 - k) as u32));
    }
    if s64 >= at as i64 && e64 < band_end {
        return None;
    }
    let s2 = if s64 < at as i64 { s } else { at };
    let e2 = if e64 >= band_end { (e64 - k) as u32 } else { at - 1 };
    Some((s2, e2))
}

/// Adjusts a formula after `count` rows or columns were inserted (`count > 0`) or deleted
/// (`count < 0`) at index `at` (0-based) of sheet `target`. `own_sheet` is the name of the
/// formula's sheet, which unqualified references belong to. References inside a deleted band
/// become `#REF!`; ranges grow when rows are inserted inside them and shrink when part of them
/// is deleted. Anchors (`$`) do not matter here: inserted rows move everything below them.
pub fn adjust_for_insert(formula: &str, own_sheet: &str, target: &str, axis: Axis, at: u32, count: i64) -> String {
    if count == 0 {
        return formula.to_string();
    }
    let target = target.to_lowercase();
    let own = own_sheet.to_lowercase();
    rewrite(formula, &mut |r, prefix, _| {
        let sheet = r.sheet.as_ref().map_or_else(|| own.clone(), |s| s.to_lowercase());
        if sheet != target {
            return None;
        }
        let rows = axis == Axis::Rows;
        let body = match r.body {
            RefBody::Cell(p) => {
                if rows {
                    shift_one(p.row, at, count, MAX_ROWS).map(|row| RefBody::Cell(Part { row, ..p }))
                } else {
                    shift_one(p.col, at, count, MAX_COLS).map(|col| RefBody::Cell(Part { col, ..p }))
                }
            }
            RefBody::Area(a, b) => {
                if rows {
                    shift_span(a.row, b.row, at, count, MAX_ROWS)
                        .map(|(s, e)| RefBody::Area(Part { row: s, ..a }, Part { row: e, ..b }))
                } else {
                    shift_span(a.col, b.col, at, count, MAX_COLS)
                        .map(|(s, e)| RefBody::Area(Part { col: s, ..a }, Part { col: e, ..b }))
                }
            }
            RefBody::Cols(a, b) => {
                if rows {
                    return None;
                }
                shift_span(a.col, b.col, at, count, MAX_COLS).map(|(s, e)| RefBody::Cols(Part { col: s, ..a }, Part { col: e, ..b }))
            }
            RefBody::Rows(a, b) => {
                if !rows {
                    return None;
                }
                shift_span(a.row, b.row, at, count, MAX_ROWS).map(|(s, e)| RefBody::Rows(Part { row: s, ..a }, Part { row: e, ..b }))
            }
        };
        Some(match body {
            Some(body) if body == r.body => return None,
            Some(body) => format!("{prefix}{}", body.render()),
            None => format!("{prefix}#REF!"),
        })
    })
}

/// Renames a sheet in every reference to it (names compare without case); the new name is
/// quoted when it needs quotes.
pub fn rename_sheet(formula: &str, old: &str, new: &str) -> String {
    let old = old.to_lowercase();
    rewrite(formula, &mut |r, _, rest| match &r.sheet {
        Some(s) if s.to_lowercase() == old => Some(format!("{}!{rest}", quote_sheet_name(new))),
        _ => None,
    })
}

/// Every reference in a formula, in order (for colouring them in the editor).
pub fn references(formula: &str) -> Vec<SheetRange> {
    let body = formula.strip_prefix('=').unwrap_or(formula);
    lex(body)
        .into_iter()
        .filter_map(|t| match t.tok {
            Tok::Ref(r) => Some(SheetRange { sheet: r.sheet, range: r.body.range() }),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translating() {
        assert_eq!(translate("A1+$B$2+C$3+$D4", 1, 1), "B2+$B$2+D$3+$D5");
        assert_eq!(translate("SUM(A1:B2)", 2, 0), "SUM(A3:B4)");
        assert_eq!(translate("SUM(A:A)", 5, 1), "SUM(B:B)");
        assert_eq!(translate("SUM(1:2)", 1, 5), "SUM(2:3)");
        assert_eq!(translate("A1", -1, 0), "#REF!");
        assert_eq!(translate("Sheet2!A1 + 'My sheet'!B2", 0, -1), "Sheet2!#REF! + 'My sheet'!A2");
        assert_eq!(translate("=a1 *  2", 1, 0), "=A2 *  2");
        assert_eq!(translate("\"A1\" & A1", 1, 0), "\"A1\" & A2");
        assert_eq!(translate("LOG10(A1)", 0, 1), "LOG10(B1)");
        assert_eq!(translate("$A1:A$5", 10, 0), "$A$5:A11");
    }

    #[test]
    fn inserting_and_deleting() {
        let rows = |f: &str, at, n| adjust_for_insert(f, "Sheet1", "Sheet1", Axis::Rows, at, n);
        let cols = |f: &str, at, n| adjust_for_insert(f, "Sheet1", "Sheet1", Axis::Cols, at, n);
        assert_eq!(rows("A1+A5", 2, 3), "A1+A8");
        assert_eq!(rows("SUM(A1:A10)", 4, 2), "SUM(A1:A12)");
        assert_eq!(rows("SUM(A3:A10)", 0, 1), "SUM(A4:A11)");
        assert_eq!(rows("SUM(A1:A10)", 10, 2), "SUM(A1:A10)");
        assert_eq!(rows("$A$5", 0, 1), "$A$6");
        assert_eq!(rows("A5", 4, -1), "#REF!");
        assert_eq!(rows("A6", 4, -1), "A5");
        assert_eq!(rows("SUM(A1:A10)", 2, -3), "SUM(A1:A7)");
        assert_eq!(rows("SUM(A3:A10)", 1, -4), "SUM(A2:A6)");
        assert_eq!(rows("SUM(A3:A4)", 2, -2), "SUM(#REF!)");
        assert_eq!(rows("SUM(A1:A3)", 0, -1), "SUM(A1:A2)");
        assert_eq!(rows("SUM(A:A)", 0, 5), "SUM(A:A)");
        assert_eq!(rows("SUM(3:5)", 0, 1), "SUM(4:6)");
        assert_eq!(cols("C1+SUM(A:D)", 1, 1), "D1+SUM(A:E)");
        assert_eq!(cols("B1", 1, -1), "#REF!");
        assert_eq!(adjust_for_insert("Data!A5+A5", "Sheet1", "data", Axis::Rows, 0, 1), "Data!A6+A5");
        assert_eq!(adjust_for_insert("A5", "Other", "Sheet1", Axis::Rows, 0, 1), "A5");
    }

    #[test]
    fn renaming() {
        assert_eq!(rename_sheet("Sales!A1+sales!B2+Other!C3", "Sales", "Q1 Sales"), "'Q1 Sales'!A1+'Q1 Sales'!B2+Other!C3");
        assert_eq!(rename_sheet("'My sheet'!A1:B2", "my sheet", "Data"), "Data!A1:B2");
        assert_eq!(rename_sheet("A1+\"Sales!A1\"", "Sales", "X"), "A1+\"Sales!A1\"");
        assert_eq!(rename_sheet("Data!B2*2", "Data", "Raw"), "Raw!B2*2");
    }

    #[test]
    fn listing() {
        let refs: Vec<String> = references("=SUM(A1:B2, 'My sheet'!C3) + Data!D:D + \"E5\"").iter().map(|r| r.to_string()).collect();
        assert_eq!(refs, ["A1:B2", "'My sheet'!C3", "Data!D:D"]);
        assert!(references("1+2").is_empty());
    }
}

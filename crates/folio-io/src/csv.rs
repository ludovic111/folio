//! Comma- (or semicolon-, tab-) separated values: one sheet of values, no formatting.
//!
//! In: the separator is guessed (`,` `;` tab `|`), the text is UTF-8 (with or without a byte
//! order mark), UTF-16 with a mark, or else Windows-1252 (a superset of Latin-1). Each field is
//! read like typing it in a cell (numbers, dates, percentages and currencies become values with
//! a format), except codes with leading zeros (`00123`), long digit strings and `+` phone
//! numbers, which stay text, and formulas, which stay text too (a CSV carries values; reading
//! `=…` as a formula is how CSV injection works). With `;` as separator, numbers with a decimal
//! comma (`1.234,50`) are read as numbers.
//!
//! Out: the first chosen sheet as it shows (formatted values), from A1 to its last used cell,
//! RFC 4180 quoting, CRLF line ends, and a UTF-8 byte order mark so Excel reads accents right.

use folio_calc::{Addr, Input};
use folio_core::{Document, PageKind};

use crate::{Format, Imported};

pub const FORMAT: Format = Format {
    id: "csv",
    name: "CSV",
    extensions: &["csv", "tsv"],
    kinds: &["sheet"],
    import: true,
    export: true,
    apps: &["Any spreadsheet", "Databases and scripts", "Bank and shop exports"],
    notes: "One sheet of values: formulas are written as their results and read as text, formats, charts and other sheets are left out. Opening guesses the separator and the text encoding.",
};

/// Text from bytes: UTF-8 (BOM or not), UTF-16 with a BOM, else Windows-1252.
fn decode(bytes: &[u8]) -> (String, Option<&'static str>) {
    if let Some(rest) = bytes.strip_prefix(b"\xef\xbb\xbf") {
        return (String::from_utf8_lossy(rest).into_owned(), None);
    }
    let utf16 = |le: bool| -> String {
        let units: Vec<u16> = bytes[2..].as_chunks::<2>().0.iter().map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        String::from_utf16_lossy(&units)
    };
    if bytes.starts_with(&[0xff, 0xfe]) {
        return (utf16(true), None);
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        return (utf16(false), None);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), None),
        Err(_) => (bytes.iter().map(|&b| cp1252(b)).collect(), Some("The file isn't UTF-8: it was read as Windows-1252 (Latin-1); check accented letters.")),
    }
}

fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9f => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

/// The separator the first lines agree on.
fn sniff_delimiter(text: &str) -> u8 {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(50).collect();
    let mut best = (b',', 0usize);
    for d in *b",;\t|" {
        let counts: Vec<usize> = lines
            .iter()
            .map(|l| {
                let mut quoted = false;
                l.bytes()
                    .filter(|&c| {
                        if c == b'"' {
                            quoted = !quoted;
                        }
                        c == d && !quoted
                    })
                    .count()
            })
            .collect();
        // The most common count among lines, and how many lines have it.
        let mut freq: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for c in &counts {
            *freq.entry(*c).or_default() += 1;
        }
        if let Some((&mode, &n)) = freq.iter().filter(|(c, _)| **c > 0).max_by_key(|(c, n)| (**n, **c)) {
            let score = n * 1000 + mode;
            if n * 10 >= lines.len() * 6 && score > best.1 {
                best = (d, score);
            }
        }
    }
    best.0
}

/// `1.234,50` or `12,5` to `1234.50` / `12.5` (when the file uses `;`, so commas are decimals).
fn decimal_comma(f: &str) -> Option<String> {
    let t = f.trim();
    let (sign, body) = t.strip_prefix('-').map(|b| ("-", b)).unwrap_or(("", t));
    let (int, frac) = body.split_once(',')?;
    if frac.is_empty() || !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let groups: Vec<&str> = int.split(['.', ' ', '\u{a0}']).collect();
    let ok = !int.is_empty() && groups.iter().all(|g| !g.is_empty() && g.chars().all(|c| c.is_ascii_digit())) && (groups.len() == 1 || groups[0].len() <= 3 && groups[1..].iter().all(|g| g.len() == 3));
    ok.then(|| format!("{sign}{}.{frac}", groups.concat()))
}

/// What to type in the cell for a field.
fn field_input(f: &str, semicolons: bool) -> String {
    let t = f.trim();
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    let keep_text = t.starts_with('=') && t.len() > 1
        || t.len() > 1 && t.starts_with('0') && digits(t)
        || digits(t) && t.len() > 15
        || t.starts_with('+') && digits(&t[1..]) && t.len() > 6;
    if !keep_text
        && semicolons
        && let Some(n) = decimal_comma(f)
    {
        return n;
    }
    match folio_calc::parse_input(f).0 {
        Input::Text(s) if s == f => f.to_string(),
        Input::Number(_) | Input::Bool(_) if !keep_text => f.to_string(),
        _ => format!("'{f}"),
    }
}

pub fn import(bytes: &[u8], title: &str) -> Result<Imported, String> {
    let (text, note) = decode(bytes);
    let delim = sniff_delimiter(&text);
    let mut rdr = ::csv::ReaderBuilder::new().delimiter(delim).has_headers(false).flexible(true).from_reader(text.as_bytes());
    let mut doc = Document::empty(title);
    let name = super::xlsx::safe_sheet_name(title, &[]);
    let i = doc.add_page(PageKind::Sheet, Some(&name), None).map_err(|e| e.0)?;
    let mut warnings: Vec<String> = note.map(|n| vec![n.to_string()]).unwrap_or_default();
    let mut widths: Vec<usize> = vec![];
    {
        let sheet = doc.page_mut(i).sheet_mut().unwrap();
        for (r, rec) in rdr.records().enumerate() {
            let rec = match rec {
                Ok(rec) => rec,
                Err(e) => {
                    warnings.push(format!("Stopped at line {}: {e}.", r + 1));
                    break;
                }
            };
            if r as u32 >= folio_calc::MAX_ROWS {
                warnings.push("The file has more rows than a sheet holds (1,048,576): the rest was left out.".into());
                break;
            }
            for (c, f) in rec.iter().enumerate() {
                if f.is_empty() || c as u32 >= folio_calc::MAX_COLS {
                    continue;
                }
                if widths.len() <= c {
                    widths.resize(c + 1, 0);
                }
                widths[c] = widths[c].max(f.chars().count());
                sheet.set_input(Addr::new(r as u32, c as u32), &field_input(f, delim == b';'));
            }
        }
        // Columns wide enough for their longest field (up to a limit).
        for (c, w) in widths.iter().enumerate() {
            let px = (*w as f32 * 7.0 + 16.0).min(400.0);
            if px > folio_core::sheet::COL_W {
                sheet.cols.insert(c as u32, px);
            }
        }
    }
    folio_core::recalc::Calc::new().sync(&mut doc);
    Ok(Imported { doc, warnings, format: "csv" })
}

pub fn export(doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    export_with(doc, pages, b',')
}

/// Tab-separated values (`.tsv`).
pub fn export_tsv(doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    export_with(doc, pages, b'\t')
}

pub fn export_with(doc: &Document, pages: &[usize], delimiter: u8) -> Result<(Vec<u8>, Vec<String>), String> {
    let (keep, mut warnings) = crate::pages_of_kind(doc, pages, PageKind::Sheet, if delimiter == b'\t' { "TSV" } else { "CSV" });
    let Some(&first) = keep.first() else {
        return Err("Nothing to write: a CSV file holds one sheet, and none of the chosen pages is a sheet.".into());
    };
    if keep.len() > 1 {
        let others: Vec<String> = keep[1..].iter().map(|i| format!("\"{}\"", doc.pages[*i].name)).collect();
        warnings.push(format!("A CSV file holds one sheet: wrote \"{}\", left out {} (export them one at a time).", doc.pages[first].name, others.join(", ")));
    }
    let sheet = doc.pages[first].sheet().unwrap();
    if !sheet.charts.is_empty() {
        warnings.push(format!("\"{}\": charts were left out (CSV holds values only).", doc.pages[first].name));
    }
    let mut out: Vec<u8> = b"\xef\xbb\xbf".to_vec();
    {
        let mut w = ::csv::WriterBuilder::new().delimiter(delimiter).terminator(::csv::Terminator::CRLF).quote_style(::csv::QuoteStyle::Necessary).from_writer(&mut out);
        if let Some(used) = sheet.used_range() {
            for row in 0..=used.end.row {
                let rec: Vec<String> = (0..=used.end.col).map(|col| sheet.display(Addr::new(row, col))).collect();
                w.write_record(&rec).map_err(|e| e.to_string())?;
            }
        }
        w.flush().map_err(|e| e.to_string())?;
    }
    Ok((out, warnings))
}

#[cfg(test)]
mod tests {
    use folio_calc::Value;

    use super::*;

    fn a(s: &str) -> Addr {
        Addr::parse(s).unwrap()
    }

    #[test]
    fn reads_values_and_keeps_codes() {
        let csv = "Name,Code,Amount,Date,Share,Note\r\nAnn,00123,\"1,234.50\",2026-10-06,12%,\"says \"\"hi\"\"\"\r\nBob,7,3,10/6/2026,5%,=1+1\r\n";
        let im = import(csv.as_bytes(), "people").unwrap();
        let s = im.doc.pages[0].sheet().unwrap();
        assert_eq!(im.doc.pages[0].name, "people");
        assert_eq!(s.value(a("B2")), Value::Text("00123".into()));
        assert_eq!(s.value(a("C2")), Value::Number(1234.5));
        assert_eq!(s.value(a("B3")), Value::Number(7.0));
        assert_eq!(s.display(a("D2")), "2026-10-06");
        assert_eq!(s.value(a("E2")), Value::Number(0.12));
        assert_eq!(s.value(a("F2")), Value::Text("says \"hi\"".into()));
        assert_eq!(s.value(a("F3")), Value::Text("=1+1".into()));
    }

    #[test]
    fn semicolons_tabs_and_latin1() {
        let im = import("a;b\n1,5;2.000,25\n".as_bytes(), "t").unwrap();
        let s = im.doc.pages[0].sheet().unwrap();
        assert_eq!(s.value(a("A2")), Value::Number(1.5));
        assert_eq!(s.value(a("B2")), Value::Number(2000.25));
        let im = import("x\ty\n3\t4\n".as_bytes(), "t").unwrap();
        assert_eq!(im.doc.pages[0].sheet().unwrap().value(a("B2")), Value::Number(4.0));
        let im = import(b"caf\xe9,\x80\n", "t").unwrap();
        assert_eq!(im.doc.pages[0].sheet().unwrap().value(a("A1")), Value::Text("café".into()));
        assert_eq!(im.doc.pages[0].sheet().unwrap().value(a("B1")), Value::Text("€".into()));
        assert_eq!(im.warnings.len(), 1);
        let im = import(b"\xef\xbb\xbfh\xc3\xa9,2\n", "t").unwrap();
        assert_eq!(im.doc.pages[0].sheet().unwrap().value(a("A1")), Value::Text("hé".into()));
    }

    #[test]
    fn writes_shown_values_with_quotes() {
        let mut doc = Document::empty("t");
        let i = doc.add_page(PageKind::Sheet, Some("S"), None).unwrap();
        doc.add_page(PageKind::Sheet, Some("Other"), None).unwrap();
        {
            let s = doc.page_mut(i).sheet_mut().unwrap();
            s.set_input(a("A1"), "Name, full");
            s.set_input(a("B1"), "12%");
            s.set_input(a("A2"), "=B1*2");
            s.set_input(a("C3"), "say \"x\"");
        }
        folio_core::recalc::Calc::new().sync(&mut doc);
        let (bytes, warnings) = export(&doc, &[0, 1]).unwrap();
        assert_eq!(warnings.len(), 1);
        let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
        assert_eq!(text, "\"Name, full\",12%,\r\n0.24,,\r\n,,\"say \"\"x\"\"\"\r\n");
        let (tsv, _) = export_tsv(&doc, &[0]).unwrap();
        assert!(String::from_utf8_lossy(&tsv).contains("Name, full\t12%\t"));
        // And back.
        let back = import(text.as_bytes(), "t").unwrap();
        assert_eq!(back.doc.pages[0].sheet().unwrap().value(a("B1")), Value::Number(0.12));
    }
}

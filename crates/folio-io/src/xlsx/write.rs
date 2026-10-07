//! Writing XLSX with `rust_xlsxwriter`: cells typed, formulas with their results, formats,
//! sizes, frozen panes, the filter and native Excel charts.

use std::collections::HashMap;

use folio_calc::{Input, Range, Value};
use folio_core::{Align, CellFormat, ChartKind, Document, PageKind, Sheet};
use rust_xlsxwriter::{
    Chart as XlChart, ChartType, Color, DocProperties, FilterCondition, FilterCriteria, Format as XlFormat, FormatAlign, FormatBorder, FormatPattern, FormatUnderline, Formula, Workbook, Worksheet,
};

use super::num_text;

/// Excel's limits on sheet names: 31 characters, unique, not "History".
fn excel_sheet_name(name: &str, taken: &[String]) -> String {
    let mut s: String = name.chars().filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\')).collect();
    s = s.trim_matches('\'').to_string();
    if s.is_empty() || s.eq_ignore_ascii_case("history") {
        s = format!("{s} sheet").trim().to_string();
    }
    let cut = |s: &str, n: usize| s.chars().take(n).collect::<String>();
    s = cut(&s, 31);
    let is_taken = |n: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if !is_taken(&s) {
        return s;
    }
    (2..).map(|i| format!("{} {i}", cut(&s, 28))).find(|n| !is_taken(n)).unwrap()
}

fn color(hex: &str) -> Option<Color> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    u32::from_str_radix(h, 16).ok().map(Color::RGB)
}

fn xl_format(f: &CellFormat) -> XlFormat {
    let mut x = XlFormat::new();
    if let Some(n) = &f.number {
        x = x.set_num_format(n);
    }
    if f.bold {
        x = x.set_bold();
    }
    if f.italic {
        x = x.set_italic();
    }
    if f.underline {
        x = x.set_underline(FormatUnderline::Single);
    }
    if f.strike {
        x = x.set_font_strikethrough();
    }
    if let Some(c) = f.color.as_deref().and_then(color) {
        x = x.set_font_color(c);
    }
    if let Some(c) = f.fill.as_deref().and_then(color) {
        x = x.set_pattern(FormatPattern::Solid).set_background_color(c);
    }
    if let Some(a) = f.align {
        x = x.set_align(match a {
            Align::Left => FormatAlign::Left,
            Align::Center => FormatAlign::Center,
            Align::Right => FormatAlign::Right,
            Align::Justify => FormatAlign::Justify,
        });
    }
    if f.wrap {
        x = x.set_text_wrap();
    }
    if let Some(s) = f.size {
        x = x.set_font_size(s);
    }
    for side in f.border.chars() {
        x = match side {
            't' => x.set_border_top(FormatBorder::Thin),
            'r' => x.set_border_right(FormatBorder::Thin),
            'b' => x.set_border_bottom(FormatBorder::Thin),
            'l' => x.set_border_left(FormatBorder::Thin),
            _ => x,
        };
    }
    x
}

/// Function names and cell references in upper case (Excel reads files that way), sheet
/// names quoted the way Excel wants and renamed to their Excel names.
fn excel_formula(f: &str, renames: &[(String, String)]) -> String {
    let b: Vec<char> = f.chars().collect();
    let mut out = String::with_capacity(f.len());
    let mut i = 0;
    let is_ref = |t: &str| {
        let t: String = t.chars().filter(|c| *c != '$').collect();
        let letters = t.chars().take_while(|c| c.is_ascii_alphabetic()).count();
        (1..=3).contains(&letters) && t.len() > letters && t[letters..].chars().all(|c| c.is_ascii_digit())
    };
    while i < b.len() {
        let c = b[i];
        if c == '"' || c == '\'' {
            let q = c;
            out.push(c);
            i += 1;
            while i < b.len() {
                out.push(b[i]);
                if b[i] == q {
                    if b.get(i + 1) == Some(&q) {
                        out.push(q);
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_' || b[i] == '.' || b[i] == '$') {
                i += 1;
            }
            let tok: String = b[start..i].iter().collect();
            let next = b.get(i).copied();
            if next == Some('(') || (next != Some('!') && (is_ref(&tok) || tok.eq_ignore_ascii_case("true") || tok.eq_ignore_ascii_case("false"))) {
                out.push_str(&tok.to_uppercase());
            } else {
                out.push_str(&tok);
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    for (old, new) in renames {
        out = folio_calc::rename_sheet(&out, old, new);
    }
    out
}

fn result_text(v: &Value) -> String {
    match v {
        Value::Empty => "0".into(),
        Value::Number(n) => num_text(*n),
        Value::Text(s) => s.clone(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Value::Error(folio_calc::ErrorKind::Circular) => "#VALUE!".into(),
        Value::Error(e) => e.code().into(),
    }
}

/// Excel's longest cell text.
const MAX_TEXT: usize = 32_767;

fn cut_text(s: &str, long: &mut usize) -> String {
    if s.chars().count() > MAX_TEXT {
        *long += 1;
        s.chars().take(MAX_TEXT).collect()
    } else {
        s.to_string()
    }
}

/// The cell (and pixel offset inside it) where a point `x` px from the left falls.
fn cell_at(px: f32, size: impl Fn(u32) -> f32) -> (u32, u32) {
    let mut left = px.max(0.0);
    let mut i = 0u32;
    while i < 16_000 {
        let s = size(i);
        if left < s || s <= 0.0 && left <= 0.0 {
            break;
        }
        left -= s;
        i += 1;
    }
    (i, left.max(0.0) as u32)
}

/// How spreadsheets read a chart's range (the rules of `ChartData::from_grid`): whether the
/// first row names the series and whether the first column labels the categories.
fn chart_layout(grid: &[Vec<(Value, String)>], headers: bool, series_in_rows: bool) -> (bool, bool) {
    let rows = grid.len();
    let cols = grid.iter().map(Vec::len).max().unwrap_or(0);
    let at = |r: usize, c: usize| -> Option<&(Value, String)> { if series_in_rows { grid.get(c).and_then(|row| row.get(r)) } else { grid.get(r).and_then(|row| row.get(c)) } };
    let (rows, cols) = if series_in_rows { (cols, rows) } else { (rows, cols) };
    if rows == 0 || cols == 0 {
        return (false, false);
    }
    let is_text = |v: Option<&(Value, String)>| matches!(v.map(|x| &x.0), Some(Value::Text(_)));
    let header_row = headers && (0..cols).any(|c| is_text(at(0, c))) || headers && rows > 1 && (0..cols).all(|c| !matches!(at(0, c).map(|x| &x.0), Some(Value::Number(_))));
    let first = if header_row { 1 } else { 0 };
    let label_col = headers && cols > 1 && (first..rows).any(|r| is_text(at(r, 0)));
    (header_row, label_col)
}

/// A folio chart as a native Excel chart reading the same cells.
fn xl_chart(doc: &Document, chart: &folio_core::Chart, excel_names: &HashMap<usize, String>) -> Result<XlChart, String> {
    let (pi, range) = folio_core::links::resolve(doc, &chart.source).map_err(|e| e.0)?;
    let xl_sheet = excel_names.get(&pi).ok_or_else(|| format!("its data is on \"{}\", which isn't exported", doc.pages[pi].name))?;
    let sheet = doc.pages[pi].sheet().ok_or("its source isn't a sheet")?;
    let r = sheet.clip(range);
    let grid = sheet.grid(r);
    if grid.is_empty() || sheet.used_range().is_none() {
        return Err("its range is empty".into());
    }
    let (header_row, label_col) = chart_layout(&grid, chart.headers, chart.series_in_rows);
    let ty = match (chart.kind, chart.stacked) {
        (ChartKind::Column, false) => ChartType::Column,
        (ChartKind::Column, true) => ChartType::ColumnStacked,
        (ChartKind::Bar, false) => ChartType::Bar,
        (ChartKind::Bar, true) => ChartType::BarStacked,
        (ChartKind::Line, _) => ChartType::Line,
        (ChartKind::Area, false) => ChartType::Area,
        (ChartKind::Area, true) => ChartType::AreaStacked,
        (ChartKind::Pie, _) => ChartType::Pie,
        (ChartKind::Scatter, _) => ChartType::ScatterStraightWithMarkers,
    };
    let mut c = XlChart::new(ty);
    let s = xl_sheet.as_str();
    // Work in "series lines" (columns, or rows when series are in rows) and "points".
    let (r0, c0, r1, c1) = (r.start.row, r.start.col as u16, r.end.row, r.end.col as u16);
    let first_point = if header_row { 1 } else { 0 };
    let first_line = if label_col { 1 } else { 0 };
    let (lines, points) = if chart.series_in_rows { ((r1 - r0 + 1) as usize, (c1 - c0 + 1) as usize) } else { ((c1 - c0 + 1) as usize, (r1 - r0 + 1) as usize) };
    if first_point >= points || first_line >= lines {
        return Err("its range has no numbers".into());
    }
    // (row0, col0, row1, col1) of a line's points, and the cell naming it.
    let line_range = |l: usize| -> (u32, u16, u32, u16) {
        if chart.series_in_rows { (r0 + l as u32, c0 + first_point as u16, r0 + l as u32, c1) } else { (r0 + first_point as u32, c0 + l as u16, r1, c0 + l as u16) }
    };
    let name_cell = |l: usize| -> (u32, u16) { if chart.series_in_rows { (r0 + l as u32, c0) } else { (r0, c0 + l as u16) } };
    let mut series_lines: Vec<usize> = (first_line..lines).collect();
    let mut categories = if label_col { Some(line_range(0)) } else { None };
    // Scatter charts plot the first column against the others.
    if chart.kind == ChartKind::Scatter && categories.is_none() && series_lines.len() > 1 {
        categories = Some(line_range(series_lines.remove(0)));
    }
    for (n, l) in series_lines.into_iter().enumerate() {
        let (a, b, cc, d) = line_range(l);
        let ser = c.add_series();
        ser.set_values((s, a, b, cc, d));
        if let Some((a, b, cc, d)) = categories {
            ser.set_categories((s, a, b, cc, d));
        }
        if header_row {
            let (nr, nc) = name_cell(l);
            ser.set_name((s, nr, nc));
        } else {
            ser.set_name(format!("Series {}", n + 1).as_str());
        }
    }
    if !chart.title.is_empty() {
        c.title().set_name(&chart.title);
    }
    if !chart.legend {
        c.legend().set_hidden();
    }
    Ok(c)
}

fn filter_condition(cond: &str) -> Option<FilterCondition> {
    let cond = cond.trim();
    let (crit, rest) = [("<>", FilterCriteria::NotEqualTo), (">=", FilterCriteria::GreaterThanOrEqualTo), ("<=", FilterCriteria::LessThanOrEqualTo), (">", FilterCriteria::GreaterThan), ("<", FilterCriteria::LessThan), ("=", FilterCriteria::EqualTo)]
        .into_iter()
        .find_map(|(op, c)| cond.strip_prefix(op).map(|r| (c, r)))
        .unwrap_or((FilterCriteria::EqualTo, cond));
    Some(match rest.trim().parse::<f64>() {
        Ok(n) => FilterCondition::new().add_custom_filter(crit, n),
        Err(_) => FilterCondition::new().add_custom_filter(crit, rest.trim()),
    })
}

fn write_sheet(ws: &mut Worksheet, sheet: &Sheet, renames: &[(String, String)], warnings: &mut Vec<String>, page: &str) -> Result<(), String> {
    let e = |e: rust_xlsxwriter::XlsxError| format!("Couldn't write \"{page}\": {e}");
    let mut formats: HashMap<String, XlFormat> = HashMap::new();
    let mut long = 0usize;
    // folio's rows are 24 px tall by default; Excel's 20.
    ws.set_default_row_height_pixels(folio_core::sheet::ROW_H as u32);
    for (a, c) in sheet.cells.iter() {
        let (row, col) = (a.row, a.col as u16);
        let fmt = if c.format.is_default() {
            None
        } else {
            let key = serde_json::to_string(&c.format).unwrap_or_default();
            Some(formats.entry(key).or_insert_with(|| xl_format(&c.format)).clone())
        };
        match folio_calc::parse_input(&c.input).0 {
            Input::Empty => {
                if let Some(f) = &fmt {
                    ws.write_blank(row, col, f).map_err(e)?;
                }
            }
            Input::Number(n) => {
                match &fmt {
                    Some(f) => ws.write_number_with_format(row, col, n, f),
                    None => ws.write_number(row, col, n),
                }
                .map_err(e)?;
            }
            Input::Text(t) => {
                let t = cut_text(&t, &mut long);
                match &fmt {
                    Some(f) => ws.write_string_with_format(row, col, &t, f),
                    None => ws.write_string(row, col, &t),
                }
                .map_err(e)?;
            }
            Input::Bool(b) => {
                match &fmt {
                    Some(f) => ws.write_boolean_with_format(row, col, b, f),
                    None => ws.write_boolean(row, col, b),
                }
                .map_err(e)?;
            }
            Input::Formula(f) => {
                let formula = Formula::new(excel_formula(&f, renames)).set_result(cut_text(&result_text(&c.value), &mut long));
                match &fmt {
                    Some(fm) => ws.write_formula_with_format(row, col, formula, fm),
                    None => ws.write_formula(row, col, formula),
                }
                .map_err(e)?;
            }
        }
    }
    if long > 0 {
        warnings.push(format!("\"{page}\": {long} cells had more text than Excel holds (32,767 characters) and were cut."));
    }
    // Sizes: used columns without a width keep folio's default, which is wider than Excel's.
    let last_col = sheet.used_range().map(|r| r.end.col).unwrap_or(0);
    for col in 0..=last_col.max(sheet.cols.keys().last().copied().unwrap_or(0)) {
        let px = sheet.col_width(col);
        if px <= 0.0 {
            ws.set_column_hidden(col as u16).map_err(e)?;
        } else if sheet.cols.contains_key(&col) || col <= last_col {
            ws.set_column_width_pixels(col as u16, px.round() as u32).map_err(e)?;
        }
    }
    for (row, px) in &sheet.rows {
        if *px <= 0.0 {
            ws.set_row_hidden(*row).map_err(e)?;
        } else {
            ws.set_row_height_pixels(*row, px.round() as u32).map_err(e)?;
        }
    }
    if sheet.freeze_rows > 0 || sheet.freeze_cols > 0 {
        ws.set_freeze_panes(sheet.freeze_rows, sheet.freeze_cols as u16).map_err(e)?;
    }
    if !sheet.gridlines {
        ws.set_screen_gridlines(false);
    }
    if let Some(f) = &sheet.filter
        && let Some(r) = Range::parse(&f.range)
    {
        ws.autofilter(r.start.row, r.start.col as u16, r.end.row, r.end.col as u16).map_err(e)?;
        for (dc, rule) in &f.rules {
            let col = r.start.col as u16 + *dc as u16;
            let cond = match (&rule.values, &rule.condition) {
                (Some(vals), _) if !vals.is_empty() => Some(vals.iter().fold(FilterCondition::new(), |c, v| c.add_list_filter(v.as_str()))),
                (_, Some(cond)) => filter_condition(cond),
                _ => None,
            };
            if let Some(cond) = cond {
                ws.filter_column(col, &cond).map_err(e)?;
            }
        }
        for row in sheet.hidden_rows() {
            ws.set_row_hidden(row).map_err(e)?;
        }
    }
    Ok(())
}

pub fn export(doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    let (keep, mut warnings) = crate::pages_of_kind(doc, pages, PageKind::Sheet, "XLSX");
    if keep.is_empty() {
        return Err("Nothing to write: an Excel workbook holds sheets, and none of the chosen pages is a sheet.".into());
    }
    // Excel names for the exported sheets; formulas follow (every sheet re-quoted, even those
    // keeping their name).
    let mut taken = vec![];
    let mut excel_names: HashMap<usize, String> = HashMap::new();
    for &i in &keep {
        let n = excel_sheet_name(&doc.pages[i].name, &taken);
        if n != doc.pages[i].name {
            warnings.push(format!("\"{}\" is called \"{n}\" in the workbook (Excel's sheet names are shorter).", doc.pages[i].name));
        }
        taken.push(n.clone());
        excel_names.insert(i, n);
    }
    let renames: Vec<(String, String)> = doc.sheets().into_iter().map(|(i, name)| (name.to_string(), excel_names.get(&i).cloned().unwrap_or_else(|| name.to_string()))).collect();
    let exported: std::collections::HashSet<String> = keep.iter().map(|i| doc.pages[*i].name.to_lowercase()).collect();

    let mut wb = Workbook::new();
    let mut props = DocProperties::new().set_title(&doc.title);
    if !doc.meta.author.is_empty() {
        props = props.set_author(&doc.meta.author);
    }
    wb.set_properties(&props);
    let mut missing_refs = 0usize;
    for &i in &keep {
        let page = &doc.pages[i];
        let sheet = page.sheet().unwrap();
        let ws = wb.add_worksheet();
        ws.set_name(&excel_names[&i]).map_err(|e| format!("Couldn't name the sheet \"{}\": {e}", page.name))?;
        write_sheet(ws, sheet, &renames, &mut warnings, &page.name)?;
        for (_, c) in sheet.cells.iter() {
            if c.is_formula() && folio_calc::references(&c.input).iter().any(|r| r.sheet.as_ref().is_some_and(|s| !exported.contains(&s.to_lowercase()))) {
                missing_refs += 1;
            }
        }
        for ch in &sheet.charts {
            match xl_chart(doc, &ch.chart, &excel_names) {
                Ok(mut xc) => {
                    xc.set_width(ch.w.max(40.0).round() as u32).set_height(ch.h.max(30.0).round() as u32);
                    let (col, dx) = cell_at(ch.x, |c| sheet.col_width(c));
                    let (row, dy) = cell_at(ch.y, |r| sheet.row_height(r));
                    ws.insert_chart_with_offset(row, col as u16, &xc, dx, dy).map_err(|e| format!("Couldn't place a chart on \"{}\": {e}", page.name))?;
                }
                Err(why) => warnings.push(format!("A chart on \"{}\" was left out: {why}.", page.name)),
            }
        }
    }
    if missing_refs > 0 {
        warnings.push(format!("{missing_refs} formulas read sheets that aren't in this export; Excel will show #REF! for them."));
    }
    let bytes = wb.save_to_buffer().map_err(|e| format!("Couldn't write the workbook: {e}"))?;
    Ok((bytes, warnings))
}

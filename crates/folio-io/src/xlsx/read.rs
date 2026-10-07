//! Reading XLSX: cells (inputs and Excel's saved results), styles, sizes, frozen panes,
//! filters and charts, straight from the package's XML.

use std::collections::{BTreeMap, HashMap};

use folio_calc::{Addr, ErrorKind, Range, SheetRange, Value};
use folio_core::sheet::{Filter, FilterRule, SheetChart};
use folio_core::{Align, CellFormat, Chart, Document, Id, PageKind, Sheet};

use super::package::{El, INDEXED, Package, apply_tint, chart_plot, chart_title, hex_color, theme_colors};
use super::{FormulaCheck, builtin_num_format, clean_formula, num_text, safe_sheet_name, text_input};
use crate::Imported;

/// SpreadsheetML colour (`rgb`, `theme` + `tint`, `indexed`) to `#rrggbb`.
fn color_of(el: &El, theme: &[String]) -> Option<String> {
    let base = if let Some(rgb) = el.attr("rgb") {
        hex_color(rgb)?
    } else if let Some(t) = el.attr("theme") {
        theme.get(t.parse::<usize>().ok()?)?.clone()
    } else if let Some(i) = el.attr("indexed") {
        let i: usize = i.parse().ok()?;
        match i {
            64 => return None, // system foreground: automatic
            _ => INDEXED.get(i)?.to_string(),
        }
    } else {
        return None;
    };
    Some(match el.attr_f64("tint") {
        Some(t) if t != 0.0 => apply_tint(&base, t),
        _ => base,
    })
}

#[derive(Clone, Default)]
struct Font {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    color: Option<String>,
    size: Option<f32>,
}

/// The cell formats of `styles.xml`, one per `cellXfs` entry.
struct Styles {
    xfs: Vec<CellFormat>,
}

impl Styles {
    fn load(x: Option<&El>, theme: &[String]) -> Styles {
        let Some(x) = x else { return Styles { xfs: vec![] } };
        let mut num_fmts: HashMap<u32, String> = HashMap::new();
        if let Some(n) = x.child("numFmts") {
            for f in n.children("numFmt") {
                if let (Some(id), Some(code)) = (f.attr("numFmtId").and_then(|v| v.parse().ok()), f.attr("formatCode")) {
                    num_fmts.insert(id, code.to_string());
                }
            }
        }
        let fonts: Vec<Font> = x
            .child("fonts")
            .map(|f| {
                f.children("font")
                    .map(|f| {
                        let on = |n: &str| f.child(n).is_some_and(|e| e.attr_bool("val", true));
                        Font {
                            bold: on("b"),
                            italic: on("i"),
                            underline: f.child("u").is_some_and(|u| u.attr("val") != Some("none")),
                            strike: on("strike"),
                            color: f.child("color").and_then(|c| color_of(c, theme)),
                            size: f.child("sz").and_then(|s| s.attr_f64("val")).map(|v| v as f32),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let default_font = fonts.first().cloned().unwrap_or_default();
        let fills: Vec<Option<String>> = x
            .child("fills")
            .map(|f| {
                f.children("fill")
                    .map(|f| {
                        let p = f.child("patternFill")?;
                        if p.attr("patternType") != Some("solid") {
                            return None;
                        }
                        p.child("fgColor").and_then(|c| color_of(c, theme)).or_else(|| p.child("bgColor").and_then(|c| color_of(c, theme)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let borders: Vec<String> = x
            .child("borders")
            .map(|b| {
                b.children("border")
                    .map(|b| {
                        let has = |n: &str| b.child(n).and_then(|s| s.attr("style")).is_some_and(|s| s != "none");
                        let mut out = String::new();
                        for (side, names) in [('t', ["top", "top"]), ('r', ["right", "end"]), ('b', ["bottom", "bottom"]), ('l', ["left", "start"])] {
                            if names.iter().any(|n| has(n)) {
                                out.push(side);
                            }
                        }
                        out
                    })
                    .collect()
            })
            .unwrap_or_default();
        let default_color = default_font.color.clone().unwrap_or_else(|| "#000000".into());
        let xfs = x
            .child("cellXfs")
            .map(|c| {
                c.children("xf")
                    .map(|xf| {
                        let idx = |k: &str| xf.attr(k).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
                        let font = fonts.get(idx("fontId")).cloned().unwrap_or_default();
                        let nid = idx("numFmtId") as u32;
                        let number = num_fmts.get(&nid).cloned().or_else(|| builtin_num_format(nid).map(str::to_string)).filter(|c| !c.eq_ignore_ascii_case("general")).map(|c| unescape_code(&c));
                        let al = xf.child("alignment");
                        let align = al.and_then(|a| a.attr("horizontal")).and_then(|h| match h {
                            "left" => Some(Align::Left),
                            "center" | "centerContinuous" => Some(Align::Center),
                            "right" => Some(Align::Right),
                            "justify" | "distributed" => Some(Align::Justify),
                            _ => None,
                        });
                        CellFormat {
                            number,
                            bold: font.bold && !default_font.bold,
                            italic: font.italic,
                            underline: font.underline,
                            strike: font.strike,
                            color: font.color.filter(|c| *c != default_color),
                            fill: fills.get(idx("fillId")).cloned().flatten(),
                            align,
                            wrap: al.is_some_and(|a| a.attr_bool("wrapText", false)),
                            size: font.size.filter(|s| Some(*s) != default_font.size),
                            border: borders.get(idx("borderId")).cloned().unwrap_or_default(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Styles { xfs }
    }

    fn get(&self, s: Option<&str>) -> CellFormat {
        s.and_then(|s| s.parse::<usize>().ok()).and_then(|i| self.xfs.get(i)).cloned().unwrap_or_default()
    }
}

/// Excel's column width (characters of the default font, padding included) to pixels.
fn width_px(w: f64) -> f32 {
    (((256.0 * w + (128.0f64 / 7.0).trunc()) / 256.0) * 7.0).trunc().max(0.0) as f32
}

/// A cell reference like `B12` to an address.
fn addr_of(r: &str) -> Option<Addr> {
    Addr::parse(r)
}

/// A string item's text: plain `<t>` or rich runs `<r><t>`; phonetic runs (`rPh`) are not text.
fn si_text(si: &El) -> String {
    let mut s = String::new();
    for k in si.elements() {
        match k.name.as_str() {
            "t" => s.push_str(&k.text()),
            "r" => {
                for t in k.children("t") {
                    s.push_str(&t.text());
                }
            }
            _ => {}
        }
    }
    s
}

fn shared_strings(x: &El) -> Vec<String> {
    x.children("si").map(si_text).collect()
}

/// A sheet as read, before names are made safe.
struct RawSheet {
    name: String,
    sheet: Sheet,
    /// Formula cells with the result Excel saved.
    cached: Vec<(Addr, Value)>,
    /// Charts: (anchor in pixels, chart part path).
    charts: Vec<([f32; 4], String)>,
}

struct Ctx<'a> {
    sst: &'a [String],
    styles: &'a Styles,
    date1904: bool,
    warnings: &'a mut Warnings,
}

#[derive(Default)]
struct Warnings {
    list: Vec<String>,
    merged: usize,
    images: usize,
    shapes: usize,
    comments: usize,
    cond: usize,
    validation: usize,
    hyperlinks: usize,
    tables: usize,
    pivots: usize,
    arrays: usize,
    data_tables: usize,
    combo: usize,
    chart_gaps: usize,
    external: usize,
}

fn read_sheet(pkg: &mut Package, path: &str, name: &str, cx: &mut Ctx) -> Result<RawSheet, String> {
    let x = pkg.xml(path).ok_or_else(|| format!("The sheet \"{name}\" is missing from the workbook ({path})."))?;
    let rels = pkg.rels(path);
    let mut sheet = Sheet::default();
    let mut cached = vec![];

    // Column widths. Excel's default is 8.43 characters (64 px) where folio's is 96 px, so
    // every used column without its own width gets Excel's default to look the same.
    let fmt_pr = x.child("sheetFormatPr");
    let default_col_px = fmt_pr
        .and_then(|f| f.attr_f64("defaultColWidth").map(width_px).or_else(|| f.attr_f64("baseColWidth").map(|b| width_px(b + 5.0 / 7.0))))
        .unwrap_or(64.0);
    let default_row_px = fmt_pr.and_then(|f| f.attr_f64("defaultRowHeight")).map(|h| (h * 4.0 / 3.0) as f32);
    let mut col_px: BTreeMap<u32, f32> = BTreeMap::new();
    if let Some(cols) = x.child("cols") {
        for c in cols.children("col") {
            let (Some(min), Some(max)) = (c.attr_f64("min"), c.attr_f64("max")) else { continue };
            let px = if c.attr_bool("hidden", false) { Some(0.0) } else { c.attr_f64("width").map(width_px) };
            if let Some(px) = px {
                // `max` is often 16384 for "the rest of the sheet": keep that cheap.
                for col in (min as u32).max(1)..=(max as u32).min(min as u32 + 1024) {
                    col_px.insert(col - 1, px);
                }
            }
        }
    }

    // Frozen panes, gridlines.
    if let Some(view) = x.path(&["sheetViews", "sheetView"]) {
        if !view.attr_bool("showGridLines", true) {
            sheet.gridlines = false;
        }
        if let Some(pane) = view.child("pane")
            && matches!(pane.attr("state"), Some("frozen") | Some("frozenSplit"))
        {
            sheet.freeze_cols = pane.attr_f64("xSplit").unwrap_or(0.0) as u32;
            sheet.freeze_rows = pane.attr_f64("ySplit").unwrap_or(0.0) as u32;
        }
    }

    // Cells.
    let mut shared: HashMap<String, (Addr, String)> = HashMap::new();
    let mut max_col = 0u32;
    let mut hidden_rows = vec![];
    let mut row_px = BTreeMap::new();
    if let Some(data) = x.child("sheetData") {
        let mut next_row = 0u32;
        for row in data.children("row") {
            let r = row.attr_f64("r").map(|v| v as u32 - 1).unwrap_or(next_row);
            next_row = r + 1;
            if let Some(ht) = row.attr_f64("ht") {
                let px = (ht * 4.0 / 3.0) as f32;
                row_px.insert(r, px);
                // Rows at the sheet's default height take folio's; rows sized to fit their text
                // only when taller than folio's default.
                let custom = row.attr_bool("customHeight", false) && default_row_px.is_none_or(|d| (d - px).abs() > 0.5);
                if (px - folio_core::sheet::ROW_H).abs() > 0.5 && (custom || px > folio_core::sheet::ROW_H) {
                    sheet.rows.insert(r, px.round());
                }
            }
            if row.attr_bool("hidden", false) {
                hidden_rows.push(r);
                row_px.insert(r, 0.0);
            }
            let row_style = if row.attr_bool("customFormat", false) { row.attr("s") } else { None };
            let mut next_col = 0u32;
            for c in row.children("c") {
                let a = c.attr("r").and_then(addr_of).unwrap_or(Addr::new(r, next_col));
                next_col = a.col + 1;
                max_col = max_col.max(a.col);
                let mut format = cx.styles.get(c.attr("s").or(row_style));
                let t = c.attr("t").unwrap_or("n");
                let v = c.child("v").map(|v| v.text());
                // Excel's saved value.
                let value = match t {
                    "s" => v.as_deref().and_then(|i| i.trim().parse::<usize>().ok()).and_then(|i| cx.sst.get(i)).map(|s| Value::Text(s.clone())).unwrap_or(Value::Empty),
                    "str" => v.map(Value::Text).unwrap_or(Value::Empty),
                    "inlineStr" => c.child("is").map(|is| Value::Text(si_text(is))).unwrap_or(Value::Empty),
                    "b" => v.map(|v| Value::Bool(v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"))).unwrap_or(Value::Empty),
                    "e" => v.map(|v| ErrorKind::parse(&v).map(Value::Error).unwrap_or(Value::Error(ErrorKind::Value))).unwrap_or(Value::Empty),
                    "d" => v.and_then(|v| iso_to_serial(&v)).map(Value::Number).unwrap_or(Value::Empty),
                    _ => v.and_then(|v| v.trim().parse::<f64>().ok()).map(Value::Number).unwrap_or(Value::Empty),
                };
                let value = match value {
                    Value::Number(n) if cx.date1904 && format.number.as_deref().is_some_and(folio_calc::is_date_format) => Value::Number(n + 1462.0),
                    v => v,
                };
                if t == "d" && format.number.is_none() {
                    format.number = Some("yyyy-mm-dd".into());
                }
                // The formula, shared ones expanded.
                let mut formula = None;
                if let Some(f) = c.child("f") {
                    let ft = f.attr("t").unwrap_or("normal");
                    let text = f.text();
                    match ft {
                        "shared" => {
                            let si = f.attr("si").unwrap_or("").to_string();
                            if !text.trim().is_empty() {
                                shared.insert(si, (a, text.clone()));
                                formula = Some(text);
                            } else if let Some((anchor, base)) = shared.get(&si) {
                                formula = Some(folio_calc::translate(base, a.row as i64 - anchor.row as i64, a.col as i64 - anchor.col as i64));
                            }
                        }
                        "dataTable" => cx.warnings.data_tables += 1,
                        "array" => {
                            if f.attr("ref").and_then(Range::parse).is_some_and(|r| r.area() > 1) {
                                cx.warnings.arrays += 1;
                            }
                            if !text.trim().is_empty() {
                                formula = Some(text);
                            }
                        }
                        _ => {
                            if !text.trim().is_empty() {
                                formula = Some(text);
                            }
                        }
                    }
                }
                let input = match &formula {
                    Some(f) => {
                        let f = clean_formula(f);
                        if f.contains('[') {
                            cx.warnings.external += 1;
                        }
                        format!("={f}")
                    }
                    None => match &value {
                        Value::Empty => String::new(),
                        Value::Number(n) => num_text(*n),
                        Value::Text(s) => text_input(s),
                        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
                        Value::Error(e) => format!("={}", e.code()),
                    },
                };
                if formula.is_some() && c.child("v").is_some() {
                    cached.push((a, value.clone()));
                }
                if input.is_empty() && format.is_default() {
                    continue;
                }
                sheet.cells.insert(a, folio_core::Cell { input, value, format });
            }
        }
    }

    // Used columns without a width of their own take Excel's default width.
    if let Some(used) = sheet.used_range() {
        max_col = max_col.max(used.end.col);
    }
    for col in 0..=max_col {
        let px = col_px.get(&col).copied().unwrap_or(default_col_px);
        if (px - folio_core::sheet::COL_W).abs() > 0.5 {
            sheet.cols.insert(col, px);
        }
    }
    for (col, px) in col_px.range(max_col + 1..) {
        if (*px - folio_core::sheet::COL_W).abs() > 0.5 {
            sheet.cols.insert(*col, *px);
        }
    }

    // Merged cells: folio has none; the top-left cell keeps the content.
    if let Some(m) = x.child("mergeCells") {
        cx.warnings.merged += m.children("mergeCell").count();
    }

    // The filter.
    if let Some(af) = x.child("autoFilter")
        && let Some(r) = af.attr("ref").and_then(Range::parse)
    {
        let mut rules = BTreeMap::new();
        for fc in af.children("filterColumn") {
            let Some(col) = fc.attr_f64("colId").map(|v| v as u32) else { continue };
            if let Some(f) = fc.child("filters") {
                let values: Vec<String> = f.children("filter").filter_map(|v| v.attr("val").map(str::to_string)).collect();
                if !values.is_empty() {
                    rules.insert(col, FilterRule { values: Some(values), condition: None });
                }
            } else if let Some(cf) = fc.child("customFilters")
                && let Some(c) = cf.child("customFilter")
            {
                let op = match c.attr("operator").unwrap_or("equal") {
                    "lessThan" => "<",
                    "lessThanOrEqual" => "<=",
                    "greaterThan" => ">",
                    "greaterThanOrEqual" => ">=",
                    "notEqual" => "<>",
                    _ => "=",
                };
                rules.insert(col, FilterRule { values: None, condition: Some(format!("{op}{}", c.attr("val").unwrap_or(""))) });
            }
        }
        sheet.filter = Some(Filter { range: r.a1(), rules });
    }
    // Hidden rows stay hidden, except those the filter hides (folio hides them itself).
    let filtered = sheet.filter.as_ref().and_then(|f| Range::parse(&f.range));
    for r in hidden_rows {
        if !filtered.is_some_and(|f| r > f.start.row && r <= f.end.row) {
            sheet.rows.insert(r, 0.0);
        }
    }

    // Things folio doesn't carry on sheets.
    cx.warnings.cond += x.children("conditionalFormatting").count();
    cx.warnings.validation += x.child("dataValidations").map(|d| d.children("dataValidation").count()).unwrap_or(0);
    cx.warnings.hyperlinks += x.child("hyperlinks").map(|h| h.children("hyperlink").count()).unwrap_or(0);
    for r in &rels {
        match r.kind.as_str() {
            "comments" => {
                if let Some(c) = pkg.xml(&r.target) {
                    cx.warnings.comments += c.all("comment").len();
                }
            }
            "table" => cx.warnings.tables += 1,
            "pivotTable" => cx.warnings.pivots += 1,
            _ => {}
        }
    }

    // Charts (and pictures, which sheets don't hold) from the drawing.
    let mut charts = vec![];
    if let Some(d) = x.child("drawing").and_then(|d| d.attr_ns("id"))
        && let Some(drel) = rels.iter().find(|r| r.id == d)
        && let Some(dx) = pkg.xml(&drel.target)
    {
        let drels = pkg.rels(&drel.target);
        for anchor in dx.elements() {
            let rect = anchor_rect(anchor, &col_px, default_col_px, &row_px, default_row_px.unwrap_or(20.0));
            if let Some(c) = anchor.find("chart")
                && let Some(id) = c.attr_ns("id")
                && let Some(cr) = drels.iter().find(|r| r.id == id)
            {
                charts.push((rect, cr.target.clone()));
            } else if anchor.find("pic").is_some() {
                cx.warnings.images += 1;
            } else if anchor.find("sp").is_some() || anchor.find("cxnSp").is_some() {
                cx.warnings.shapes += 1;
            }
        }
    }
    Ok(RawSheet { name: name.to_string(), sheet, cached, charts })
}

/// An anchor's box in pixels on the grid.
fn anchor_rect(anchor: &El, cols: &BTreeMap<u32, f32>, default_col: f32, rows: &BTreeMap<u32, f32>, default_row: f32) -> [f32; 4] {
    let emu = |v: Option<f64>| v.unwrap_or(0.0) as f32 / 9525.0;
    let point = |m: &El| -> (f32, f32) {
        let num = |n: &str| m.child(n).map(|e| e.text().trim().parse::<f64>().unwrap_or(0.0));
        let col = num("col").unwrap_or(0.0) as u32;
        let row = num("row").unwrap_or(0.0) as u32;
        let x: f32 = (0..col).map(|c| cols.get(&c).copied().unwrap_or(default_col)).sum::<f32>() + emu(num("colOff"));
        let y: f32 = (0..row).map(|r| rows.get(&r).copied().unwrap_or(default_row)).sum::<f32>() + emu(num("rowOff"));
        (x, y)
    };
    let ext = |a: &El| a.child("ext").map(|e| (emu(e.attr_f64("cx")), emu(e.attr_f64("cy"))));
    match anchor.name.as_str() {
        "twoCellAnchor" => {
            let (x0, y0) = anchor.child("from").map(point).unwrap_or((0.0, 0.0));
            let (x1, y1) = anchor.child("to").map(point).unwrap_or((x0 + 480.0, y0 + 288.0));
            [x0, y0, (x1 - x0).max(40.0), (y1 - y0).max(30.0)]
        }
        "oneCellAnchor" => {
            let (x0, y0) = anchor.child("from").map(point).unwrap_or((0.0, 0.0));
            let (w, h) = ext(anchor).unwrap_or((480.0, 288.0));
            [x0, y0, w, h]
        }
        _ => {
            let pos = anchor.child("pos");
            let (w, h) = ext(anchor).unwrap_or((480.0, 288.0));
            [emu(pos.and_then(|p| p.attr_f64("x"))), emu(pos.and_then(|p| p.attr_f64("y"))), w, h]
        }
    }
}

/// `2026-10-06` or `2026-10-06T14:30:00` to a serial.
fn iso_to_serial(s: &str) -> Option<f64> {
    let s = s.trim();
    let (date, time) = s.split_once('T').unwrap_or((s, ""));
    let mut p = date.split('-');
    let (y, m, d) = (p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?);
    let mut serial = folio_calc::date_to_serial(y, m, d);
    if !time.is_empty() {
        let t: Vec<f64> = time.trim_end_matches('Z').split(':').filter_map(|v| v.parse().ok()).collect();
        serial += folio_calc::time_fraction(*t.first().unwrap_or(&0.0) as u32, *t.get(1).unwrap_or(&0.0) as u32, *t.get(2).unwrap_or(&0.0));
    }
    Some(serial)
}

/// A chart part as a folio chart over one range (`None` with a reason when it can't be).
fn read_chart(x: &El, sheet_names: &HashMap<String, String>, warnings: &mut Warnings) -> Result<Chart, String> {
    let chart = x.child("chart").ok_or("no chart element")?;
    let (kind, stacked, first, combo) = chart_plot(chart)?;
    if combo {
        warnings.combo += 1;
    }
    // Every reference the series use, on one sheet.
    let mut sheet: Option<String> = None;
    let mut bbox: Option<Range> = None;
    let mut has_names = false;
    let mut has_cats = false;
    let mut series_in_rows = None;
    let mut parts: Vec<Range> = vec![];
    let mut add = |f: &str, sheet: &mut Option<String>, bbox: &mut Option<Range>| -> bool {
        let Some(sr) = SheetRange::parse(f.trim()) else { return false };
        parts.push(sr.range);
        let name = sr.sheet.clone().unwrap_or_default();
        match sheet {
            None => *sheet = Some(name),
            Some(s) if !s.eq_ignore_ascii_case(&name) => return false,
            _ => {}
        }
        *bbox = Some(match bbox {
            None => sr.range,
            Some(b) => Range::new(
                Addr::new(b.start.row.min(sr.range.start.row), b.start.col.min(sr.range.start.col)),
                Addr::new(b.end.row.max(sr.range.end.row), b.end.col.max(sr.range.end.col)),
            ),
        });
        true
    };
    let reff = |e: Option<&El>| -> Option<String> { e.and_then(|e| e.find("f")).map(|f| f.text()) };
    for ser in first.children("ser") {
        if let Some(f) = reff(ser.child("tx")) {
            has_names |= add(&f, &mut sheet, &mut bbox);
        }
        if let Some(f) = reff(ser.child("cat").or(ser.child("xVal"))) {
            has_cats |= add(&f, &mut sheet, &mut bbox);
        }
        if let Some(f) = reff(ser.child("val").or(ser.child("yVal"))) {
            if series_in_rows.is_none()
                && let Some(sr) = SheetRange::parse(f.trim())
            {
                series_in_rows = Some(sr.range.rows() == 1 && sr.range.cols() > 1);
            }
            add(&f, &mut sheet, &mut bbox);
        }
    }
    let (Some(sheet), Some(bbox)) = (sheet, bbox) else { return Err("its data isn't in cells".into()) };
    // One range stands for the chart: lines inside it the chart didn't use become series too.
    let rows = series_in_rows.unwrap_or(false);
    let lines = |r: &Range| if rows { r.start.row..=r.end.row } else { r.start.col..=r.end.col };
    let used: std::collections::BTreeSet<u32> = parts.iter().flat_map(lines).collect();
    if lines(&bbox).any(|l| !used.contains(&l)) {
        warnings.chart_gaps += 1;
    }
    let sheet = sheet_names.get(&sheet.to_lowercase()).cloned().unwrap_or(sheet);
    let mut c = Chart::new(kind, SheetRange { sheet: Some(sheet), range: bbox }.to_string());
    c.series_in_rows = series_in_rows.unwrap_or(false);
    c.headers = has_names || has_cats;
    c.stacked = stacked;
    c.legend = chart.child("legend").is_some();
    c.title = chart_title(chart);
    Ok(c)
}

pub fn import(bytes: &[u8], title: &str) -> Result<Imported, String> {
    if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0]) {
        return Err("This is an Excel 97–2003 workbook (.xls) or a password-protected one; folio opens .xlsx: save it as .xlsx from Excel, Numbers or LibreOffice first.".into());
    }
    let mut pkg = Package::open(bytes).map_err(|_| "This file isn't an Excel workbook (it isn't a zip package).".to_string())?;
    let wb_path = pkg.main_part().unwrap_or_else(|| "xl/workbook.xml".into());
    let wb = pkg.xml(&wb_path).ok_or("This file isn't an Excel workbook (no workbook part).")?;
    let wb_rels = pkg.rels(&wb_path);
    let theme: Vec<String> = match wb_rels.iter().find(|r| r.kind == "theme").and_then(|r| pkg.xml(&r.target)) {
        Some(t) => {
            let c = theme_colors(&t);
            let get = |n: &str, d: &str| c.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()).unwrap_or_else(|| d.to_string());
            // SpreadsheetML's theme indexes swap light and dark: lt1, dk1, lt2, dk2, accents.
            vec![
                get("lt1", "#ffffff"),
                get("dk1", "#000000"),
                get("lt2", "#e7e6e6"),
                get("dk2", "#44546a"),
                get("accent1", "#4472c4"),
                get("accent2", "#ed7d31"),
                get("accent3", "#a5a5a5"),
                get("accent4", "#ffc000"),
                get("accent5", "#5b9bd5"),
                get("accent6", "#70ad47"),
                get("hlink", "#0563c1"),
                get("folHlink", "#954f72"),
            ]
        }
        None => ["#ffffff", "#000000", "#e7e6e6", "#44546a", "#4472c4", "#ed7d31", "#a5a5a5", "#ffc000", "#5b9bd5", "#70ad47", "#0563c1", "#954f72"].map(String::from).to_vec(),
    };
    let sst = wb_rels.iter().find(|r| r.kind == "sharedStrings").and_then(|r| pkg.xml(&r.target)).map(|x| shared_strings(&x)).unwrap_or_default();
    let styles_x = wb_rels.iter().find(|r| r.kind == "styles").and_then(|r| pkg.xml(&r.target));
    let styles = Styles::load(styles_x.as_ref(), &theme);
    let date1904 = wb.child("workbookPr").is_some_and(|p| p.attr_bool("date1904", false));

    let mut warnings = Warnings::default();
    let mut raw = vec![];
    let mut hidden = vec![];
    let mut chartsheets = 0;
    {
        let mut cx = Ctx { sst: &sst, styles: &styles, date1904, warnings: &mut warnings };
        for s in wb.child("sheets").map(|s| s.children("sheet").collect::<Vec<_>>()).unwrap_or_default() {
            let name = s.attr("name").unwrap_or("Sheet").to_string();
            let Some(rel) = s.attr_ns("id").and_then(|id| wb_rels.iter().find(|r| r.id == id)) else { continue };
            if rel.kind == "chartsheet" {
                chartsheets += 1;
                continue;
            }
            if rel.kind != "worksheet" {
                continue;
            }
            if matches!(s.attr("state"), Some("hidden") | Some("veryHidden")) {
                hidden.push(name.clone());
            }
            let target = rel.target.clone();
            raw.push(read_sheet(&mut pkg, &target, &name, &mut cx)?);
        }
    }
    if raw.is_empty() {
        return Err("This workbook has no worksheets folio can read.".into());
    }

    // Page names folio accepts; formulas follow renamed sheets.
    let mut doc = Document::empty(title);
    let mut names: HashMap<String, String> = HashMap::new();
    let mut taken: Vec<String> = vec![];
    for r in &raw {
        let safe = safe_sheet_name(&r.name, &taken);
        taken.push(safe.clone());
        names.insert(r.name.to_lowercase(), safe);
    }
    let renamed: Vec<(String, String)> = raw.iter().filter_map(|r| names.get(&r.name.to_lowercase()).filter(|n| **n != r.name).map(|n| (r.name.clone(), n.clone()))).collect();
    if !renamed.is_empty() {
        warnings.list.push(format!(
            "Renamed sheets folio's formulas can't name as they were: {}.",
            renamed.iter().map(|(a, b)| format!("\"{a}\" → \"{b}\"")).collect::<Vec<_>>().join(", ")
        ));
    }
    let mut checks: Vec<(usize, Vec<(Addr, Value)>)> = vec![];
    let mut sheet_charts: Vec<(usize, Vec<([f32; 4], String)>)> = vec![];
    for r in raw {
        let name = names[&r.name.to_lowercase()].clone();
        let i = doc.add_page(PageKind::Sheet, Some(&name), None).map_err(|e| e.0)?;
        let mut sheet = r.sheet;
        if !renamed.is_empty() {
            let formulas: Vec<Addr> = sheet.cells.iter().filter(|(_, c)| c.is_formula()).map(|(a, _)| *a).collect();
            for a in formulas {
                if let Some(c) = sheet.cells.get_mut(&a) {
                    let mut f = c.input[1..].to_string();
                    for (old, new) in &renamed {
                        f = folio_calc::rename_sheet(&f, old, new);
                    }
                    c.input = format!("={f}");
                }
            }
        }
        *doc.page_mut(i).sheet_mut().unwrap() = sheet;
        checks.push((i, r.cached));
        sheet_charts.push((i, r.charts));
    }

    // Charts, now that page names are final.
    for (i, list) in sheet_charts {
        for (rect, path) in list {
            let Some(cx) = pkg.xml(&path) else { continue };
            match read_chart(&cx, &names, &mut warnings) {
                Ok(chart) => {
                    let s = doc.page_mut(i).sheet_mut().unwrap();
                    s.charts.push(SheetChart { id: Id::new(), chart, x: rect[0], y: rect[1], w: rect[2], h: rect[3] });
                }
                Err(why) => warnings.list.push(format!("A chart on \"{}\" was left out: {why}.", doc.pages[i].name)),
            }
        }
    }

    // Values from folio's engine, compared with Excel's.
    let mut check = FormulaCheck::default();
    folio_core::recalc::Calc::new().sync(&mut doc);
    for (i, cached) in &checks {
        let s = doc.pages[*i].sheet().unwrap();
        for (a, v) in cached {
            if let Some(c) = s.cell(*a) {
                check.compare(&doc.pages[*i].name, *a, &c.input, v, &c.value);
            }
        }
    }

    let w = &mut warnings;
    let mut out = std::mem::take(&mut w.list);
    out.extend(check.warnings("Excel"));
    let mut note = |n: usize, one: &str, many: &str| {
        if n == 1 {
            out.push(one.to_string());
        } else if n > 1 {
            out.push(many.replace("{n}", &n.to_string()));
        }
    };
    note(w.merged, "A merged range was split: folio has no merged cells, the top-left cell keeps the content.", "{n} merged ranges were split: folio has no merged cells, each top-left cell keeps the content.");
    note(w.images, "A picture on a sheet was left out (sheets don't hold pictures in folio; put it in a document or on a slide).", "{n} pictures on sheets were left out (sheets don't hold pictures in folio; put them in a document or on a slide).");
    note(w.shapes, "A drawn shape or text box on a sheet was left out.", "{n} drawn shapes or text boxes on sheets were left out.");
    note(w.comments, "A cell comment was left out.", "{n} cell comments were left out.");
    note(w.cond, "Conditional formatting was left out (one rule set).", "Conditional formatting was left out ({n} rule sets).");
    note(w.validation, "A data validation rule was left out.", "{n} data validation rules were left out.");
    note(w.hyperlinks, "A cell hyperlink was left out (the cell keeps its text).", "{n} cell hyperlinks were left out (cells keep their text).");
    note(w.tables, "An Excel table became plain cells (formulas naming it by its columns show #NAME?).", "{n} Excel tables became plain cells (formulas naming them by their columns show #NAME?).");
    note(w.pivots, "A pivot table became plain values (it no longer updates).", "{n} pivot tables became plain values (they no longer update).");
    note(w.arrays, "An array formula keeps only its first cell's formula; the other cells hold its values.", "{n} array formulas keep only their first cell's formula; the other cells hold their values.");
    note(w.data_tables, "A what-if data table became plain values.", "{n} what-if data table cells became plain values.");
    note(w.combo, "A combination chart became one kind of chart.", "{n} combination charts became one kind of chart.");
    note(w.chart_gaps, "A chart now also shows the cells between its labels and its values (folio charts read one range).", "{n} charts now also show the cells between their labels and their values (folio charts read one range).");
    note(w.external, "A formula reads another workbook: folio can't, it shows #REF!.", "{n} formulas read other workbooks: folio can't, they show #REF!.");
    note(chartsheets, "A chart sheet was left out (folio's charts sit on sheets).", "{n} chart sheets were left out (folio's charts sit on sheets).");
    if !hidden.is_empty() {
        out.push(format!("Hidden sheets are shown in folio: {}.", hidden.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(", ")));
    }
    if date1904 {
        out.push("The workbook counts dates from 1904 (old Mac Excel); dates were moved to folio's 1900 count and look the same.".into());
    }
    if let Some(dn) = wb.child("definedNames") {
        let names: Vec<String> = dn.children("definedName").filter_map(|d| d.attr("name")).filter(|n| !n.starts_with("_xlnm.") && !n.starts_with("_xlfn.")).map(str::to_string).collect();
        if !names.is_empty() {
            let shown: Vec<String> = names.iter().take(6).map(|n| format!("\"{n}\"")).collect();
            out.push(format!(
                "Named ranges aren't supported yet ({}{}): formulas that use them show #NAME?.",
                shown.join(", "),
                if names.len() > 6 { format!(" and {} more", names.len() - 6) } else { String::new() }
            ));
        }
    }
    Ok(Imported { doc, warnings: out, format: "xlsx" })
}

/// Excel escapes characters that need no escaping (`yyyy\-mm\-dd`): written plainly, codes
/// match folio's presets and survive other formats.
pub(crate) fn unescape_code(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut chars = code.chars();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                // Decimal points and commas affect numeric formatting unless escaped.
                if quoted || !matches!(n, '-' | '/' | ' ' | ':' | '(' | ')') {
                    out.push(c);
                }
                out.push(n);
            } else {
                out.push(c);
            }
        } else {
            if c == '"' {
                quoted = !quoted;
            }
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod format_code_tests {
    #[test]
    fn normalizes_date_separators_without_changing_numeric_literals() {
        for (input, expected) in [(r"yyyy\-mm\-dd", "yyyy-mm-dd"), (r"0\,000", r"0\,000"), (r"0\.00", r"0\.00"), (r#"0 "a\-b""#, r#"0 "a\-b""#), (r"0\\-", r"0\\-")] {
            assert_eq!(super::unescape_code(input), expected);
        }
    }
}

//! `sheet.*`: cells, formulas, formats, rows and columns, sorting, filters, fills, charts.

use std::sync::Arc;

use folio_calc::{Addr, Range, Value};
use folio_core::sheet::{Filter, FilterRule, FormatPatch, SheetChart};
use folio_core::{Chart, Id, PageKind};
use serde_json::json;

use super::util::{self, page_of};
use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

/// Cells `sheet.read` returns at most.
const READ_MAX: usize = 2000;

fn addr(s: &str) -> CmdResult<Addr> {
    Addr::parse(s.trim()).ok_or_else(|| format!("\"{s}\" isn't a cell like B3."))
}

fn range(s: &str) -> CmdResult<Range> {
    Range::parse(s.trim()).ok_or_else(|| format!("\"{s}\" isn't a range like B2:D9."))
}

fn col(s: &str) -> CmdResult<u32> {
    let t = s.trim();
    folio_calc::col_index(t).or_else(|| t.parse::<u32>().ok().filter(|n| *n >= 1).map(|n| n - 1)).ok_or_else(|| format!("\"{s}\" isn't a column like C."))
}

/// A JSON value as typed text: numbers and booleans as written, strings as they are.
fn typed(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => if *b { "TRUE".into() } else { "FALSE".into() },
        other => other.to_string(),
    }
}

pub fn value_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Empty => serde_json::Value::Null,
        Value::Number(n) => json!(n),
        Value::Text(t) => json!(t),
        Value::Bool(b) => json!(b),
        Value::Error(e) => json!(e.code()),
    }
}

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    if cx.spec.name == "sheet.functions" {
        return functions(s, &a);
    }
    if cx.spec.name == "sheet.numberFormats" {
        return Ok(json!(folio_calc::PRESETS.iter().map(|(label, code)| json!({ "label": label, "code": code, "example": folio_calc::format_value(&Value::Number(if code.contains('y') || code.contains('h') { 46301.5625 } else { -1234.5678 }), Some(code)) })).collect::<Vec<_>>()));
    }
    let doc = s.doc()?;
    let pi = page_of(s, &doc, &a, Some(PageKind::Sheet))?;
    let page_id = doc.pages[pi].id.clone();
    let sh = doc.pages[pi].sheet().unwrap();
    let label = cx.label();
    let src = cx.source;
    match cx.spec.name {
        "sheet.read" => {
            let r = match a.opt_str("range") {
                Some(r) => sh.clip(range(r)?),
                None => match sh.used_range() {
                    Some(r) => r,
                    None => return Ok(json!({ "name": doc.pages[pi].name, "range": null, "rows": [], "charts": sh.charts })),
                },
            };
            let inputs = a.bool_or("inputs", false);
            let total = r.rows() as usize * r.cols() as usize;
            let max_rows = (READ_MAX / r.cols().max(1) as usize).max(1);
            let rows: Vec<Vec<serde_json::Value>> = (r.start.row..=r.end.row)
                .take(max_rows)
                .map(|row| {
                    (r.start.col..=r.end.col)
                        .map(|c| {
                            let at = Addr::new(row, c);
                            match sh.cell(at) {
                                None => serde_json::Value::Null,
                                Some(cell) if inputs => json!(cell.input),
                                Some(cell) => {
                                    if cell.format.number.is_some() || matches!(cell.value, Value::Error(_)) { json!(cell.display()) } else { value_json(&cell.value) }
                                }
                            }
                        })
                        .collect()
                })
                .collect();
            let formulas: Vec<serde_json::Value> = if inputs {
                vec![]
            } else {
                sh.cells.iter().filter(|(at, c)| r.contains(**at) && c.is_formula()).take(300).map(|(at, c)| json!({ "cell": at.a1(), "formula": c.input })).collect()
            };
            let truncated = total > max_rows * r.cols() as usize;
            Ok(json!({
                "name": doc.pages[pi].name,
                "range": r.a1(),
                "rows": rows,
                "formulas": formulas,
                "truncated": truncated,
                "charts": sh.charts,
                "filter": sh.filter,
                "frozen": { "rows": sh.freeze_rows, "columns": sh.freeze_cols },
            }))
        }
        "sheet.set" => {
            let at = addr(a.str("cell")?)?;
            let input = typed(a.get("value").unwrap_or(&serde_json::Value::Null));
            s.edit(label, src, a.coalesce(), |d| {
                d.page_mut(pi).sheet_mut().unwrap().set_input(at, &input);
                Ok(())
            })?;
            cell_result(s, pi, at)
        }
        "sheet.setRange" => {
            let at = addr(a.str("at")?)?;
            let rows = a.array("values").cloned().unwrap_or_default();
            let mut count = 0;
            let grid: Vec<Vec<String>> = rows
                .iter()
                .map(|r| match r {
                    serde_json::Value::Array(cells) => cells.iter().map(typed).collect(),
                    other => vec![typed(other)],
                })
                .collect();
            s.edit(label, src, None, |d| {
                let sh = d.page_mut(pi).sheet_mut().unwrap();
                for (dr, row) in grid.iter().enumerate() {
                    for (dc, v) in row.iter().enumerate() {
                        sh.set_input(Addr::new(at.row + dr as u32, at.col + dc as u32), v);
                        count += 1;
                    }
                }
                Ok(())
            })?;
            let end = Addr::new(at.row + grid.len().saturating_sub(1) as u32, at.col + grid.iter().map(Vec::len).max().unwrap_or(1).saturating_sub(1) as u32);
            let errors = errors_in(s, pi, Range { start: at, end });
            Ok(json!({ "range": Range { start: at, end }.a1(), "cells": count, "errors": errors }))
        }
        "sheet.clear" => {
            let r = range(a.str("range")?)?;
            let (contents, formats) = match a.opt_str("what").unwrap_or("contents") {
                "contents" => (true, false),
                "formats" => (false, true),
                "all" => (true, true),
                w => return Err(format!("what is contents, formats or all, not \"{w}\".")),
            };
            s.edit(label, src, None, |d| {
                d.page_mut(pi).sheet_mut().unwrap().clear(r, contents, formats);
                Ok(())
            })?;
            Ok(json!({ "range": r.a1() }))
        }
        "sheet.format" => {
            let r = range(a.str("range")?)?;
            let patch = FormatPatch {
                number: a.opt_str("number").map(str::to_string),
                bold: a.opt_bool("bold"),
                italic: a.opt_bool("italic"),
                underline: a.opt_bool("underline"),
                strike: a.opt_bool("strike"),
                color: util::color(&a, "color")?,
                fill: util::color(&a, "fill")?,
                align: a.opt_str("align").map(str::to_string),
                wrap: a.opt_bool("wrap"),
                size: a.opt_f64("size").map(|v| v as f32),
                border: a.opt_str("border").map(|b| if b == "none" { String::new() } else { b.to_string() }),
            };
            if patch == FormatPatch::default() {
                return Err("Say what to change: number, bold, italic, underline, strike, color, fill, align, wrap, size or border.".into());
            }
            s.edit(label, src, a.coalesce(), |d| {
                d.page_mut(pi).sheet_mut().unwrap().format(r, &patch);
                Ok(())
            })?;
            Ok(json!({ "range": r.a1() }))
        }
        "sheet.insertRows" | "sheet.deleteRows" | "sheet.insertColumns" | "sheet.deleteColumns" => {
            let rows = cx.spec.name.ends_with("Rows");
            let at = if rows { (a.opt_i64("at").unwrap_or(1).max(1) - 1) as u32 } else { col(a.str("at")?)? };
            let n = a.opt_i64("count").unwrap_or(1).clamp(1, 100_000);
            let count = if cx.spec.name.starts_with("sheet.insert") { n } else { -n };
            s.edit(label, src, None, |d| d.insert_lines(pi, rows, at, count))?;
            Ok(json!({ "at": if rows { json!(at + 1) } else { json!(folio_calc::col_name(at)) }, "count": count }))
        }
        "sheet.sort" => {
            let r = sh.clip(range(a.str("range")?)?);
            let keys: Vec<(u32, bool)> = match a.array("by") {
                Some(list) if !list.is_empty() => list
                    .iter()
                    .map(|k| {
                        let c = k.get("column").and_then(|v| v.as_str()).ok_or("each sort key needs \"column\"")?;
                        let desc = k.get("descending").and_then(|v| v.as_bool()).unwrap_or(false) || k.get("order").and_then(|v| v.as_str()) == Some("desc");
                        Ok::<_, String>((col(c)?, !desc))
                    })
                    .collect::<Result<_, _>>()?,
                _ => vec![(r.start.col, true)],
            };
            for (c, _) in &keys {
                if *c < r.start.col || *c > r.end.col {
                    return Err(format!("Column {} is outside {}.", folio_calc::col_name(*c), r.a1()));
                }
            }
            // A header: first row all text while the next has a number.
            let header = a.opt_bool("header").unwrap_or_else(|| {
                let first: Vec<Value> = (r.start.col..=r.end.col).map(|c| sh.value(Addr::new(r.start.row, c))).collect();
                let second: Vec<Value> = (r.start.col..=r.end.col).map(|c| sh.value(Addr::new(r.start.row + 1, c))).collect();
                first.iter().all(|v| matches!(v, Value::Text(_) | Value::Empty)) && first.iter().any(|v| matches!(v, Value::Text(_))) && second.iter().any(|v| matches!(v, Value::Number(_)))
                    || first.iter().all(|v| matches!(v, Value::Text(_))) && r.rows() > 1 && keys.iter().any(|(c, _)| matches!(sh.value(Addr::new(r.start.row + 1, *c)), Value::Number(_)))
            });
            s.edit(label, src, None, |d| {
                d.page_mut(pi).sheet_mut().unwrap().sort(r, &keys, header);
                Ok(())
            })?;
            Ok(json!({ "range": r.a1(), "header": header }))
        }
        "sheet.filter" => {
            let clear = a.bool_or("clear", false);
            let column = a.opt_str("column").map(col).transpose()?;
            let current = sh.filter.clone();
            let r = match a.opt_str("range") {
                Some(r) => range(r)?,
                None => current.as_ref().and_then(|f| Range::parse(&f.range)).or(sh.used_range()).ok_or("The sheet is empty.")?,
            };
            let rule = FilterRule { values: a.array("values").map(|v| v.iter().map(typed).collect()), condition: a.opt_str("condition").map(str::to_string) };
            let next = if clear && column.is_none() {
                None
            } else {
                let mut f = current.filter(|f| f.range == r.a1()).unwrap_or(Filter { range: r.a1(), rules: Default::default() });
                if let Some(c) = column {
                    if c < r.start.col || c > r.end.col {
                        return Err(format!("Column {} is outside {}.", folio_calc::col_name(c), r.a1()));
                    }
                    if clear || (rule.values.is_none() && rule.condition.is_none()) {
                        f.rules.remove(&(c - r.start.col));
                    } else {
                        f.rules.insert(c - r.start.col, rule);
                    }
                }
                Some(f)
            };
            let hidden = s.edit(label, src, None, |d| {
                let sh = d.page_mut(pi).sheet_mut().unwrap();
                sh.filter = next;
                Ok(sh.hidden_rows().len())
            })?;
            Ok(json!({ "range": r.a1(), "hiddenRows": hidden }))
        }
        "sheet.fill" => {
            let r = range(a.str("range")?)?;
            let down = match a.opt_str("direction").unwrap_or("down") {
                "down" => true,
                "right" => false,
                d => return Err(format!("direction is down or right, not \"{d}\".")),
            };
            s.edit(label, src, None, |d| {
                d.page_mut(pi).sheet_mut().unwrap().fill(r, down);
                Ok(())
            })?;
            Ok(json!({ "range": r.a1() }))
        }
        "sheet.copy" => {
            let from = range(a.str("from")?)?;
            let from = if from.rows() as u64 * from.cols() as u64 > 1_000_000 { sh.clip(from) } else { from };
            let to = addr(a.str("to")?)?;
            let tp = match a.opt_str("toPage") {
                Some(p) => {
                    let i = doc.page_index(p).ok_or_else(|| format!("No page \"{p}\"."))?;
                    if doc.pages[i].kind() != PageKind::Sheet {
                        return Err(format!("\"{p}\" isn't a sheet."));
                    }
                    i
                }
                None => pi,
            };
            let mv = a.bool_or("move", false);
            let formats = a.bool_or("formats", true);
            let block = sh.copy_block(from, to);
            s.edit(label, src, None, |d| {
                if mv {
                    d.page_mut(pi).sheet_mut().unwrap().clear(from, true, formats);
                }
                d.page_mut(tp).sheet_mut().unwrap().paste(&block, to, formats);
                Ok(())
            })?;
            let end = Addr::new(to.row + from.rows() - 1, to.col + from.cols() - 1);
            Ok(json!({ "to": Range { start: to, end }.a1(), "page": doc.pages[tp].name }))
        }
        "sheet.resize" => {
            let cols: Vec<(u32, f32)> = match a.object("columns") {
                Some(m) => m.iter().map(|(k, v)| Ok::<_, String>((col(k)?, v.as_f64().ok_or("widths are numbers of pixels")? as f32))).collect::<Result<_, _>>()?,
                None => vec![],
            };
            let rows: Vec<(u32, f32)> = match a.object("rows") {
                Some(m) => m.iter().map(|(k, v)| Ok::<_, String>((k.parse::<u32>().map_err(|_| format!("\"{k}\" isn't a row number"))?.max(1) - 1, v.as_f64().ok_or("heights are numbers of pixels")? as f32))).collect::<Result<_, _>>()?,
                None => vec![],
            };
            let fit = a.opt_str("fit").map(|f| range(&if f.contains(':') { f.to_string() } else { format!("{f}:{f}") })).transpose()?;
            s.edit(label, src, a.coalesce(), |d| {
                let sh = d.page_mut(pi).sheet_mut().unwrap();
                for (c, w) in cols {
                    sh.cols.insert(c, w.clamp(16.0, 2000.0));
                }
                for (r, h) in rows {
                    sh.rows.insert(r, h.clamp(12.0, 1000.0));
                }
                if let Some(f) = fit {
                    for c in f.start.col..=f.end.col.min(f.start.col + 200) {
                        let widest = sh.cells.iter().filter(|(at, _)| at.col == c).map(|(_, cell)| cell.display().chars().count()).max().unwrap_or(0);
                        sh.cols.insert(c, (widest as f32 * 7.5 + 18.0).clamp(40.0, 600.0));
                    }
                }
                Ok(())
            })?;
            Ok(json!({ "ok": true }))
        }
        "sheet.freeze" => {
            let (r, c) = (a.opt_i64("rows").unwrap_or(0).clamp(0, 100) as u32, a.opt_i64("columns").unwrap_or(0).clamp(0, 50) as u32);
            s.edit(label, src, None, |d| {
                let sh = d.page_mut(pi).sheet_mut().unwrap();
                sh.freeze_rows = r;
                sh.freeze_cols = c;
                Ok(())
            })?;
            Ok(json!({ "rows": r, "columns": c }))
        }
        "sheet.setGridlines" => {
            let on = a.opt_bool("on").unwrap_or(true);
            s.edit(label, src, None, |d| {
                d.page_mut(pi).sheet_mut().unwrap().gridlines = on;
                Ok(())
            })?;
            Ok(json!({ "gridlines": on }))
        }
        "sheet.evaluate" => {
            let f = a.str("formula")?.trim();
            let f = f.strip_prefix('=').unwrap_or(f).to_string();
            let at = a.opt_str("cell").map(addr).transpose()?.unwrap_or(Addr::new(0, 0));
            let v = s.read(|ed| ed.calc().sheet_index(&page_id).map(|si| ed.calc().engine().evaluate(si, at, &f)))?.unwrap_or(Value::Empty);
            Ok(json!({ "value": value_json(&v), "text": folio_calc::format_value(&v, None) }))
        }
        "sheet.find" => {
            let needle = a.str("text")?.to_lowercase();
            let in_f = a.bool_or("inFormulas", true);
            let hits: Vec<serde_json::Value> = sh
                .cells
                .iter()
                .filter(|(_, c)| c.display().to_lowercase().contains(&needle) || in_f && c.is_formula() && c.input.to_lowercase().contains(&needle))
                .take(500)
                .map(|(at, c)| json!({ "cell": at.a1(), "value": c.display(), "input": c.input }))
                .collect();
            Ok(json!({ "count": hits.len(), "cells": hits }))
        }
        "sheet.addChart" => {
            let r = range(a.str("range")?)?;
            let source = folio_calc::SheetRange { sheet: Some(doc.pages[pi].name.clone()), range: r }.to_string();
            let mut chart = Chart::new(super::doc::chart_kind(&a)?, source);
            chart.title = a.opt_str("title").unwrap_or("").to_string();
            chart.series_in_rows = a.bool_or("seriesInRows", false);
            // Right of the data by default.
            let used = sh.used_range().unwrap_or(r);
            let default_x: f32 = (0..=used.end.col).map(|c| sh.col_width(c)).sum::<f32>() + 24.0;
            let default_y: f32 = (0..r.start.row).map(|rw| sh.row_height(rw)).sum::<f32>();
            let c = SheetChart {
                id: Id::new(),
                chart,
                x: a.opt_f64("x").map(|v| v as f32).unwrap_or(default_x),
                y: a.opt_f64("y").map(|v| v as f32).unwrap_or(default_y),
                w: a.opt_f64("w").map(|v| v as f32).unwrap_or(480.0).max(80.0),
                h: a.opt_f64("h").map(|v| v as f32).unwrap_or(300.0).max(60.0),
            };
            let id = c.id.clone();
            s.edit(label, src, None, |d| {
                d.page_mut(pi).sheet_mut().unwrap().charts.push(c);
                Ok(())
            })?;
            Ok(json!({ "chart": id }))
        }
        "sheet.updateChart" => {
            let id = a.str("chart")?.to_string();
            let ci = sh.charts.iter().position(|c| c.id == id.as_str()).ok_or_else(|| format!("No chart \"{id}\" on this sheet (sheet.read lists them)."))?;
            let kind = a.opt_str("kind").map(|_| super::doc::chart_kind(&a)).transpose()?;
            let source = a.opt_str("range").map(range).transpose()?.map(|r| folio_calc::SheetRange { sheet: Some(doc.pages[pi].name.clone()), range: r }.to_string());
            s.edit(label, src, a.coalesce(), |d| {
                let c = &mut d.page_mut(pi).sheet_mut().unwrap().charts[ci];
                if let Some(k) = kind {
                    c.chart.kind = k;
                }
                if let Some(src) = &source {
                    c.chart.source = src.clone();
                }
                if let Some(t) = a.opt_str("title") {
                    c.chart.title = t.to_string();
                }
                if let Some(v) = a.opt_bool("seriesInRows") {
                    c.chart.series_in_rows = v;
                }
                if let Some(v) = a.opt_bool("legend") {
                    c.chart.legend = v;
                }
                if let Some(v) = a.opt_bool("stacked") {
                    c.chart.stacked = v;
                }
                for (k, slot) in [("x", &mut c.x), ("y", &mut c.y), ("w", &mut c.w), ("h", &mut c.h)] {
                    if let Some(v) = a.opt_f64(k) {
                        *slot = v as f32;
                    }
                }
                c.w = c.w.max(80.0);
                c.h = c.h.max(60.0);
                Ok(())
            })?;
            Ok(json!({ "chart": id }))
        }
        "sheet.removeChart" => {
            let id = a.str("chart")?.to_string();
            let ci = sh.charts.iter().position(|c| c.id == id.as_str()).ok_or_else(|| format!("No chart \"{id}\" on this sheet."))?;
            s.edit(label, src, None, |d| {
                d.page_mut(pi).sheet_mut().unwrap().charts.remove(ci);
                Ok(())
            })?;
            Ok(json!({ "removed": id }))
        }
        _ => Err(super::unhandled(cx)),
    }
}

/// One cell as the answer to a change: what it holds and shows.
fn cell_result(s: &Session, pi: usize, at: Addr) -> CmdResult {
    let doc = s.doc()?;
    let sh = doc.pages[pi].sheet().unwrap();
    let cell = sh.cell(at).cloned().unwrap_or_default();
    Ok(json!({ "cell": at.a1(), "input": cell.input, "value": value_json(&cell.value), "shows": cell.display() }))
}

/// Cells showing an error in a range (for the agent to notice its mistakes).
fn errors_in(s: &Session, pi: usize, r: Range) -> Vec<serde_json::Value> {
    let Ok(doc) = s.doc() else { return vec![] };
    let sh = doc.pages[pi].sheet().unwrap();
    sh.cells.iter().filter(|(at, c)| r.contains(**at) && matches!(c.value, Value::Error(_))).take(50).map(|(at, c)| json!({ "cell": at.a1(), "input": c.input, "error": c.display() })).collect()
}

fn functions(s: &Session, a: &Args) -> CmdResult {
    let list = s.read(|ed| ed.calc().engine().functions()).unwrap_or_else(|_| folio_calc::Engine::new().functions());
    let q = a.opt_str("search").map(str::to_lowercase);
    let cat = a.opt_str("category").map(str::to_lowercase);
    Ok(json!(list
        .into_iter()
        .filter(|f| q.as_ref().is_none_or(|q| f.name.to_lowercase().contains(q) || f.summary.to_lowercase().contains(q)))
        .filter(|f| cat.as_ref().is_none_or(|c| f.category.to_lowercase() == *c))
        .map(|f| json!({ "name": f.name, "syntax": f.syntax, "summary": f.summary, "category": f.category }))
        .collect::<Vec<_>>()))
}

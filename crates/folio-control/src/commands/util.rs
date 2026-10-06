//! Helpers the command families share: finding pages, blocks, slides and shapes from
//! parameters, reading positions and formatting.

use folio_core::text::{Flow, ParaPatch, StylePatch};
use folio_core::{Align, Block, Document, ListKind, PageKind, ParaStyle, Pos};
use serde_json::{Value, json};

use crate::registry::Args;
use crate::session::{CmdResult, Session};

/// The page a command acts on: `page` if given, else the window's page when it is of `kind`,
/// else the first page of `kind`. Checks the kind.
pub fn page_of(s: &Session, doc: &Document, a: &Args, kind: Option<PageKind>) -> CmdResult<usize> {
    if let Some(key) = a.get("page") {
        let key = match key {
            Value::String(k) => k.clone(),
            Value::Number(n) => n.to_string(),
            other => return Err(format!("`page` is a page id, name or number, not {other}")),
        };
        let i = doc.page_index(&key).ok_or_else(|| doc.page(&key).err().map(|e| e.0).unwrap_or_default())?;
        if let Some(k) = kind
            && doc.pages[i].kind() != k
        {
            return Err(format!("\"{}\" is a {}, not a {}.", doc.pages[i].name, noun(doc.pages[i].kind()), noun(k)));
        }
        return Ok(i);
    }
    let shown = s.ui_state().page.and_then(|id| doc.page_index(&id));
    if let Some(i) = shown
        && kind.is_none_or(|k| doc.pages[i].kind() == k)
    {
        return Ok(i);
    }
    match kind {
        Some(k) => doc.pages.iter().position(|p| p.kind() == k).ok_or_else(|| format!("The file has no {}. Add one with page.add kind={}.", noun(k), k.id())),
        None => {
            if doc.pages.is_empty() {
                Err("The file has no pages. Add one with page.add.".into())
            } else {
                Ok(0)
            }
        }
    }
}

pub fn noun(k: PageKind) -> &'static str {
    match k {
        PageKind::Doc => "document",
        PageKind::Sheet => "sheet",
        PageKind::Deck => "deck",
    }
}

/// A block index from an index or an id.
pub fn block_of(flow: &Flow, v: &Value) -> CmdResult<usize> {
    let i = match v {
        Value::Number(n) => n.as_i64().ok_or("a block index is a whole number")?,
        Value::String(s) => match s.parse::<i64>() {
            Ok(n) => n,
            Err(_) => return flow.iter().position(|b| b.id() == s.as_str()).ok_or_else(|| format!("No block with id \"{s}\" (doc.read lists them).")),
        },
        other => return Err(format!("a block is an index or an id, not {other}")),
    };
    if i < 0 || i as usize >= flow.len() {
        return Err(format!("Block {i} doesn't exist: the page has {} blocks (0 to {}).", flow.len(), flow.len().saturating_sub(1)));
    }
    Ok(i as usize)
}

/// A position from `{"block": 2, "offset": 5, "cell": [0, 1]}`.
pub fn pos_of(flow: &Flow, v: &Value) -> CmdResult<Pos> {
    let o = v.as_object().ok_or("a position is {\"block\": 0, \"offset\": 0}")?;
    let block = block_of(flow, o.get("block").ok_or("a position needs \"block\"")?)?;
    let offset = o.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let cell = match o.get("cell") {
        Some(Value::Array(rc)) if rc.len() == 2 => Some((rc[0].as_u64().unwrap_or(0) as usize, rc[1].as_u64().unwrap_or(0) as usize)),
        Some(Value::Null) | None => None,
        Some(other) => return Err(format!("\"cell\" is [row, col], not {other}")),
    };
    if cell.is_some() && !matches!(flow[block], Block::Table(_)) {
        return Err(format!("Block {block} isn't a table (\"cell\" only goes with tables)."));
    }
    let p = folio_core::text::clamp(flow, Pos { block, cell, offset });
    Ok(p)
}

pub fn pos_json(p: Pos) -> Value {
    match p.cell {
        Some((r, c)) => json!({ "block": p.block, "offset": p.offset, "cell": [r, c] }),
        None => json!({ "block": p.block, "offset": p.offset }),
    }
}

/// Character formatting from the shared parameters.
pub fn style_patch(a: &Args) -> StylePatch {
    let s = |k: &str| a.opt_str(k).map(str::to_string);
    StylePatch {
        bold: a.opt_bool("bold"),
        italic: a.opt_bool("italic"),
        underline: a.opt_bool("underline"),
        strike: a.opt_bool("strike"),
        code: a.opt_bool("code"),
        superscript: a.opt_bool("superscript"),
        subscript: a.opt_bool("subscript"),
        color: s("color"),
        highlight: s("highlight"),
        link: s("link"),
        size: a.opt_f64("size").map(|v| v as f32),
        font: s("font"),
        note: None,
        comment: None,
    }
}

/// Paragraph settings from the shared parameters.
pub fn para_patch(a: &Args) -> CmdResult<ParaPatch> {
    let style = match a.opt_str("style") {
        Some(st) => Some(ParaStyle::parse(st).ok_or_else(|| format!("Unknown paragraph style \"{st}\": normal, title, subtitle, heading1, heading2, heading3, quote, code or caption."))?),
        None => None,
    };
    let align = match a.opt_str("align") {
        Some(al) => Some(Align::parse(al).ok_or_else(|| format!("Unknown alignment \"{al}\": left, center, right or justify."))?),
        None => None,
    };
    let list = match a.opt_str("list") {
        Some(l) => Some(ListKind::parse(l).ok_or_else(|| format!("Unknown list \"{l}\": bullet, number, check or none."))?),
        None => None,
    };
    Ok(ParaPatch { style, align, list, level: a.opt_i64("level").map(|l| l.clamp(0, 5) as u8), checked: a.opt_bool("checked") })
}

/// Blocks as agents read them: index, id, kind, style and text (tables as rows).
pub fn blocks_json(doc: &Document, flow: &Flow) -> Vec<Value> {
    flow.iter()
        .enumerate()
        .map(|(i, b)| match b {
            Block::Paragraph(p) => {
                let mut v = json!({ "index": i, "id": p.id, "type": "paragraph", "text": p.text() });
                if p.style != ParaStyle::Normal {
                    v["style"] = json!(p.style.id());
                }
                if p.align != Align::Left {
                    v["align"] = json!(p.align.id());
                }
                if let Some(l) = p.list {
                    v["list"] = json!(l);
                    v["level"] = json!(p.level);
                    if l == ListKind::Check {
                        v["checked"] = json!(p.checked);
                    }
                }
                if p.runs.iter().any(|r| r.style != Default::default()) {
                    v["runs"] = json!(p.runs);
                }
                v
            }
            Block::Table(t) => {
                let rows: Vec<Vec<String>> = match &t.link {
                    Some(l) => folio_core::links::table_text(doc, l).unwrap_or_default(),
                    None => t.rows.iter().map(|r| r.iter().map(|c| c.plain()).collect()).collect(),
                };
                json!({ "index": i, "id": t.id, "type": "table", "header": t.header, "link": t.link, "rows": rows })
            }
            Block::Image(im) => {
                let m = doc.media.get(&im.media);
                json!({ "index": i, "id": im.id, "type": "image", "media": im.media, "name": m.map(|m| m.name.clone()), "width": im.width, "caption": im.caption })
            }
            Block::Chart(c) => json!({ "index": i, "id": c.id, "type": "chart", "chart": c.chart, "height": c.height }),
            Block::PageBreak { id } => json!({ "index": i, "id": id, "type": "pageBreak" }),
        })
        .collect()
}

/// Where new blocks go: after `after` (index or id; -1 first), else after the window's caret
/// block on this page, else at the end. Returns the insertion index.
pub fn insert_index(s: &Session, page_id: &str, flow: &Flow, a: &Args) -> CmdResult<usize> {
    if let Some(v) = a.get("after") {
        if v.as_i64() == Some(-1) {
            return Ok(0);
        }
        return Ok(block_of(flow, v)? + 1);
    }
    let ui = s.ui_state();
    if ui.page.as_deref() == Some(page_id)
        && let Some(t) = ui.text
        && t.focus.block < flow.len()
    {
        return Ok(t.focus.block + 1);
    }
    Ok(flow.len())
}

/// A hex colour `#rrggbb` (or "" to clear), checked.
pub fn color(a: &Args, key: &str) -> CmdResult<Option<String>> {
    match a.opt_str(key) {
        None => Ok(None),
        Some("") => Ok(Some(String::new())),
        Some(c) => {
            let c = c.trim();
            let hex = c.strip_prefix('#').unwrap_or(c);
            if hex.len() == 6 && hex.chars().all(|ch| ch.is_ascii_hexdigit()) || hex.len() == 3 && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
                let full = if hex.len() == 3 { hex.chars().flat_map(|ch| [ch, ch]).collect() } else { hex.to_string() };
                Ok(Some(format!("#{}", full.to_ascii_lowercase())))
            } else {
                named_color(c).map(|v| Some(v.to_string())).ok_or_else(|| format!("`{key}` is a colour like #1d4ed8, not \"{c}\"."))
            }
        }
    }
}

fn named_color(c: &str) -> Option<&'static str> {
    Some(match c.to_ascii_lowercase().as_str() {
        "black" => "#000000",
        "white" => "#ffffff",
        "red" => "#d92d20",
        "green" => "#16a34a",
        "blue" => "#1d4ed8",
        "yellow" => "#facc15",
        "orange" => "#f97316",
        "purple" => "#7c3aed",
        "grey" | "gray" => "#808080",
        _ => return None,
    })
}

/// Reads a picture file into the document's media; returns its id.
pub fn add_image_file(doc: &mut Document, path: &str) -> folio_core::Result<folio_core::Id> {
    let p = std::path::Path::new(path);
    let bytes = std::fs::read(p).map_err(|e| folio_core::Error(format!("Couldn't read {path}: {e}")))?;
    let mime = folio_core::Media::sniff(&bytes);
    if !mime.starts_with("image/") {
        return Err(folio_core::Error(format!("{path} isn't a picture folio can show (PNG, JPEG, GIF, WebP, BMP, SVG).")));
    }
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "image".into());
    Ok(doc.add_media(&name, bytes))
}

/// A short one-line summary of a page for lists.
pub fn summary(p: &folio_core::Page) -> String {
    match &p.body {
        folio_core::PageBody::Doc(d) => {
            let words = d.word_count();
            let title = d.blocks.iter().find_map(|b| b.para().filter(|p| !p.is_empty()).map(|p| p.text())).unwrap_or_default();
            let title: String = title.chars().take(60).collect();
            format!("{words} words{}", if title.is_empty() { String::new() } else { format!(" · \"{title}\"") })
        }
        folio_core::PageBody::Sheet(s) => match s.used_range() {
            Some(r) => {
                let formulas = s.cells.values().filter(|c| c.is_formula()).count();
                format!("{} · {} cells, {formulas} formulas{}", r.a1(), s.cells.len(), if s.charts.is_empty() { String::new() } else { format!(", {} charts", s.charts.len()) })
            }
            None => "empty".into(),
        },
        folio_core::PageBody::Deck(d) => {
            let first = d.slides.first().map(|s| s.title()).unwrap_or_default();
            format!("{} slides{}", d.slides.len(), if first.is_empty() { String::new() } else { format!(" · \"{first}\"") })
        }
    }
}

/// An absolute path (relative ones from the current folder).
pub fn absolute(path: &str) -> CmdResult<std::path::PathBuf> {
    let p = std::path::PathBuf::from(shellexpand(path));
    if p.is_absolute() {
        return Ok(p);
    }
    Ok(std::env::current_dir().map_err(|e| e.to_string())?.join(p))
}

fn shellexpand(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).display().to_string();
    }
    p.to_string()
}

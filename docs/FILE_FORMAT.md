# The .folio file format (format 1)

A `.folio` file is a zip archive:

| Entry | What it is |
| --- | --- |
| `mimetype` | First, stored (not compressed): `application/vnd.lsuite.folio`, so tools can recognise the file from its first bytes, as with ODF. |
| `document.json` | The whole document as JSON (below), media listed without their bytes. |
| `media/<id>.<ext>` | Each picture the document uses, as the bytes it came in (PNG, JPEG, GIF, WebP, BMP, SVG). |

Files are written atomically (a temporary file beside the target, then a rename). A reader must
refuse a `format` higher than it knows. Everything below is in the model's Rust types
(`crates/folio-core`), which serialise to exactly this JSON.

## document.json

```json
{
  "format": 1,
  "id": "k3m9x2ab",
  "title": "Quarterly review",
  "pages": [ { "id": "…", "name": "Report", "kind": "doc", … }, { "kind": "sheet", … }, { "kind": "deck", … } ],
  "media": { "p7q2…": { "id": "p7q2…", "name": "photo.jpg", "mime": "image/jpeg", "width": 1600, "height": 900 } },
  "meta": { "author": "", "created": "2026-10-07T01:00:00Z", "modified": "…", "generator": "folio 0.1.0" }
}
```

Ids are eight lowercase letters and digits. Page names are unique in a file and are what formulas
call a sheet (`'Raw data'!B2`), so they can't contain `! ' [ ] : * ? / \`.

### Document pages (`"kind": "doc"`)

```json
{ "id": "…", "name": "Report", "kind": "doc",
  "blocks": [
    { "type": "paragraph", "id": "…", "style": "heading1", "runs": [ { "text": "Results" } ] },
    { "type": "paragraph", "id": "…", "list": "bullet", "level": 0, "runs": [ { "text": "Up ", "bold": true }, { "text": "12 %" } ] },
    { "type": "table", "id": "…", "header": true, "link": "Numbers!A1:E5", "rows": [[{ "runs": [] }]] },
    { "type": "image", "id": "…", "media": "p7q2…", "width": 300, "caption": "", "align": "center" },
    { "type": "chart", "id": "…", "height": 240, "chart": { "kind": "column", "source": "Numbers!A1:C4", "title": "Revenue" } },
    { "type": "pageBreak", "id": "…" }
  ],
  "setup": { "width": 595, "height": 842, "marginTop": 72, "marginBottom": 72, "marginLeft": 72, "marginRight": 72, "header": "", "footer": "{page}" },
  "comments": [ { "id": "…", "author": "Ana", "text": "Source?", "at": "…", "resolved": false, "replies": [] } ],
  "trackChanges": false }
```

- Paragraph `style`: `normal` (left out), `title`, `subtitle`, `heading1`, `heading2`, `heading3`,
  `quote`, `code`, `caption`. `align`: `left` (left out), `center`, `right`, `justify`. `list`:
  `bullet`, `number`, `check` (with `checked`), `level` 0–5.
- Runs: `text` and any of `bold`, `italic`, `underline`, `strike`, `code`, `superscript`,
  `subscript`, `color` and `highlight` (`#rrggbb`), `link`, `size` (points), `font` (`sans`, `serif`,
  `mono`, `display` or a family name), `note` (a footnote's text, its number drawn after the run),
  `comment` (a comment id), `inserted` / `deleted` (a tracked change, by author).
- Sizes are points (72 to the inch). A4 is 595 × 842; Letter 612 × 792. Header and footer fill in
  `{page}`, `{pages}` and `{title}`.
- A table with a `link` shows that sheet range's computed values; its own `rows` are kept as a
  fallback for readers that don't compute.

### Sheet pages (`"kind": "sheet"`)

```json
{ "id": "…", "name": "Numbers", "kind": "sheet",
  "cells": {
    "A1": { "input": "Month", "format": { "bold": true } },
    "B2": { "input": "42000", "value": 42000, "format": { "number": "$#,##0" } },
    "D2": { "input": "=B2-C2", "value": 10500 }
  },
  "cols": { "0": 120 }, "rows": {}, "freezeRows": 1, "freezeCols": 0, "gridlines": true,
  "filter": { "range": "A1:E40", "rules": { "1": { "condition": ">40000" } } },
  "charts": [ { "id": "…", "x": 520, "y": 0, "w": 460, "h": 290, "chart": { "kind": "line", "source": "Numbers!A1:C4" } } ] }
```

- `input` is what was typed: a number, text, `TRUE`/`FALSE`, or a formula starting with `=`
  (Excel's syntax and functions). `value` is the computed result, stored so readers without a
  formula engine see results: a number, a string, a boolean, or `{"error": "#DIV/0!"}`. folio
  recomputes values when it opens a file.
- `format.number` is an Excel number format code (`0.00`, `#,##0`, `0%`, `$#,##0.00`, `yyyy-mm-dd`…).
  Other format fields: `bold`, `italic`, `underline`, `strike`, `color`, `fill`, `align`, `wrap`,
  `size`, `border` (any of `t r b l`). Dates are Excel serial numbers (the 1900 system).
- Column widths and row heights are pixels at 100 % (default 96 and 24), keyed by 0-based index.

### Deck pages (`"kind": "deck"`)

```json
{ "id": "…", "name": "Slides", "kind": "deck", "size": [960, 540],
  "theme": { "name": "paper", "background": "#fbfbfb", "text": "#0a0a0a", "accent": "#0a0a0a", "headingFont": "display", "bodyFont": "sans" },
  "slides": [ { "id": "…", "layout": "titleContent", "notes": "Say hello",
    "shapes": [
      { "id": "…", "name": "Title", "kind": { "type": "text" }, "x": 72, "y": 38, "w": 816, "h": 86, "placeholder": "title", "textSize": 16, "valign": "bottom",
        "text": [ { "type": "paragraph", "id": "…", "style": "title", "runs": [ { "text": "Results" } ] } ] },
      { "id": "…", "kind": { "type": "chart", "chart": { "kind": "line", "source": "Numbers!A1:C4" } }, "x": 72, "y": 150, "w": 816, "h": 340 }
    ] } ] }
```

- Coordinates are points on the slide (960 × 540 is 16:9, PowerPoint's default).
- Shape kinds: `text`, `rect`, `ellipse`, `triangle`, `line`, `arrow`, `image` (`media`), `chart`
  (`chart`), `table` (`table`, which may have a `link`). `fill`, `line` (colours), `lineWidth`,
  `rotation` (degrees), `color` (text), `textSize` (the size normal text has in the shape; styles
  scale from it), `valign` (`top`, `middle`, `bottom`).
- Layouts: `title`, `titleContent`, `section`, `twoContent`, `titleOnly`, `blank`.

## Live links

A link or chart source is a sheet range with its sheet named: `Numbers!A1:C4`, `'Raw data'!B:B`.
Renaming a sheet, or inserting and deleting rows and columns, rewrites every formula, link and
chart source in the file so they keep pointing at the same cells.

# Changelog

What changed in each folio release.

## 0.1.0 — 2026-10-07 (beta)

The first release of folio, lsuite's office app.

### New
- **One file, three kinds of page.** A `.folio` file holds any mix of documents, sheets and decks, listed in the Pages sidebar. Add, rename, reorder and duplicate pages; open the file anywhere, it is a zip of JSON and pictures (docs/FILE_FORMAT.md).
- **Documents.** Pages of paper laid out exactly as they print: paragraph styles (title, headings, quote, code…), bold, italic, underline, strikethrough, colours, sizes and fonts, lists and checklists, tables, pictures, page breaks, headers and footers with page numbers, page size and margins, footnotes, comments and tracked changes.
- **Sheets.** A grid with a real formula engine: references, ranges, other sheets, recalculation by dependencies, and the functions people use (SUM, AVERAGE, IF, VLOOKUP, XLOOKUP, INDEX, MATCH, COUNTIF, SUMIF, TEXT, DATE and many more). Number formats, sorting, filters, fill down and right, frozen rows, column widths, charts.
- **Decks.** Slides with layouts, text boxes, shapes, pictures, tables and charts; move and resize them on the slide, speaker notes, themes, and presenting full screen or with the presenter view.
- **Live links.** A table in a document, a chart on a slide or in a report can show a sheet's range as it is computed now, and follows every change.
- **Coming from another suite.** folio opens Word, Excel and PowerPoint files (also what Google Docs, Sheets and Slides download and what Pages, Numbers and Keynote export), OpenDocument files, CSV and Markdown, and exports to PDF, Word, Excel, PowerPoint, OpenDocument, CSV, Markdown and HTML.
- **Your AI can drive it.** Every action is a command shared by the window, the Agent panel (⌘J), `folio-cli` and `folio-mcp`, with one undo history. lsuite AI works without setup once you sign in; Claude Code, Codex, API keys and local models work too.
- **Plugins.** Spreadsheet functions written in Rust with folio's plugin SDK; ask your agent to build one and it appears without a restart.
- **lsuite.** folio lists itself in `~/.lsuite/apps/folio.json` for the other apps, and takes pictures from nori and frames from kimchi.

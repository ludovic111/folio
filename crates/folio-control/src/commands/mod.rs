//! Every command's spec and handler. Specs are listed here in one table so the docs, the CLI
//! help and the MCP tools are generated in a stable order.

pub mod account;
pub mod agent;
pub mod app;
pub mod deck;
pub mod doc;
pub mod file;
pub mod handoff;
pub mod history;
pub mod page;
pub mod plugin;
pub mod sheet;
pub mod text;
pub mod ui;
pub mod util;

use std::sync::Arc;

use crate::registry::Kind::*;
use crate::registry::{Args, COALESCE, Ctx, Param, Perm, Spec, edit, opt, query, req};
use crate::session::{CmdResult, Session};

const PAGE: Param = opt("page", String, "Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind.");
const SLIDE: Param = opt("slide", String, "Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first.");
const SHAPE: Param = req("shape", String, "Shape id or name (deck.read lists them).");
const OPT_SHAPE: Param = opt("shape", String, "A text box or shape on a slide (with page and slide): its id or name. Without it, the text is the document page's.");
const BLOCK: Param = req("block", Any, "Block index from doc.read (0 is the first) or its id.");
const AFTER: Param = opt("after", Any, "Put it after this block (index from doc.read, or id); -1 puts it first. Default: after the caret's block in the window, else at the end.");
const FROM: Param = req("from", Object, "Position {\"block\": 0, \"offset\": 0} (offset in characters; add \"cell\": [row, col] inside a table).");
const TO: Param = opt("to", Object, "End position, same shape as from (default: from, or the end of from's block when from has no offset).");
const RANGE: Param = req("range", String, "Cells in A1 notation: B2, B2:D9, B:B, 2:2.");
const CELL: Param = req("cell", String, "A cell, A1 notation.");
const FIND: Param = opt("find", String, "Text to look for instead of a position (the first match, or every match with all).");
const ALL: Param = opt("all", Boolean, "With find: every match (default false: the first).");
const X: Param = opt("x", Number, "Left edge in points from the slide's left (slides are 960×540 by default).");
const Y: Param = opt("y", Number, "Top edge in points.");
const W: Param = opt("w", Number, "Width in points.");
const H: Param = opt("h", Number, "Height in points.");

// Character formatting, shared by text.format, doc.format and deck.formatText.
const BOLD: Param = opt("bold", Boolean, "Bold on or off.");
const ITALIC: Param = opt("italic", Boolean, "Italic on or off.");
const UNDERLINE: Param = opt("underline", Boolean, "Underline on or off.");
const STRIKE: Param = opt("strike", Boolean, "Strikethrough on or off.");
const CODE: Param = opt("code", Boolean, "Inline code (mono) on or off.");
const SUPER: Param = opt("superscript", Boolean, "Superscript on or off.");
const SUB: Param = opt("subscript", Boolean, "Subscript on or off.");
const COLOR: Param = opt("color", String, "Text colour #rrggbb; \"\" clears.");
const HIGHLIGHT: Param = opt("highlight", String, "Highlight colour #rrggbb; \"\" clears.");
const LINK: Param = opt("link", String, "Link URL; \"\" removes the link.");
const SIZE: Param = opt("size", Number, "Size in points; 0 goes back to the paragraph style's.");
const FONT: Param = opt("font", String, "sans, serif, mono or display; \"\" goes back to the style's.");

// Paragraph settings, shared by text.paragraph, doc.setParagraph, doc.addParagraph.
const PSTYLE: Param = opt("style", String, "Paragraph style: normal, title, subtitle, heading1, heading2, heading3, quote, code or caption.");
const ALIGN: Param = opt("align", String, "left, center, right or justify.");
const LIST: Param = opt("list", String, "bullet, number, check, or none.");
const LEVEL: Param = opt("level", Integer, "List level, 0 to 5.");

pub static SPECS: &[Spec] = &[
    // ---- file -------------------------------------------------------------
    query("file.overview", "The whole open file in one bounded answer: title, where it is saved, every page with what it holds (documents: outline, words, first paragraphs; sheets: used range, a sample of values and formulas, errors; decks: each slide's title and text), live links between pages, undo history and what the window shows. Read it first.", &[]),
    query("file.info", "Where the open file is saved (or that it isn't yet), where it was imported from, its title, pages and whether everything is written to disk.", &[]),
    query("file.get", "The complete open file as JSON (the .folio format's document.json, media listed without their bytes).", &[]),
    query("file.recent", "Files opened recently, newest first, with whether each still exists.", &[]),
    query("file.formats", "What folio opens and writes: each format with its extensions, the apps that use it (Microsoft Word, Excel, PowerPoint, Google Docs, Sheets and Slides through their downloads, Apple Pages, Numbers and Keynote through their exports, LibreOffice), which kinds of page it carries, and what doesn't survive the trip.", &[]),
    query("file.templates", "Templates for file.new: a report, a letter, a budget, an invoice, a project plan, a pitch deck…", &[]),
    edit("file.new", "Create a new file and open it (it saves itself in folio's untitled folder until file.saveAs).", &[
        opt("title", String, "Title (default \"Untitled\")."),
        opt("kind", String, "The first page: doc (default), sheet, deck, or blank for none."),
        opt("template", String, "A template id from file.templates instead."),
    ]).perm(Perm::Files),
    edit("file.open", "Open a .folio file, or import a Word, Excel, PowerPoint, OpenDocument, CSV or Markdown file as a new untitled folio file (the original is left alone; file.export writes it back). Replaces the open file.", &[
        req("path", String, "The file to open."),
    ]).perm(Perm::Files),
    edit("file.import", "Add the pages of another file (Word, Excel, PowerPoint, OpenDocument, CSV, Markdown, or another .folio) to the open file, as one undo step.", &[
        req("path", String, "The file to bring in."),
        opt("at", Integer, "Insert the pages at this 0-based position (default: at the end)."),
    ]).perm(Perm::Files),
    edit("file.save", "Write the open file to disk now (it also saves itself a moment after every change). An untitled file needs file.saveAs.", &[]),
    edit("file.saveAs", "Save the open file as a .folio file at a path; it saves there from now on.", &[req("path", String, "Destination .folio file.")]).perm(Perm::Files),
    edit("file.export", "Write the open file (or some pages) in another format: pdf, docx, xlsx, pptx, odt, ods, odp, csv, md or html. Documents go to word processors, sheets to spreadsheets, decks to presentations; PDF and HTML take every kind. Returns what the format couldn't carry.", &[
        req("path", String, "Destination file; its extension picks the format unless format is given."),
        opt("format", String, "pdf, docx, xlsx, pptx, odt, ods, odp, csv, md or html."),
        opt("pages", Array, "Only these pages (ids, names or 1-based numbers).").of(String),
    ]).perm(Perm::Files),
    edit("file.rename", "Change the file's title (not its file name).", &[req("title", String, "New title."), COALESCE]),
    edit("file.close", "Close the open file (everything is saved) and go back to the home screen.", &[]),
    edit("file.batch", "Run several commands as one undo step. With atomic (the default) a failing command rolls back the ones before it.", &[
        req("commands", Array, "Array of {\"command\": \"sheet.set\", \"params\": {…}}.").of(Object),
        opt("atomic", Boolean, "Roll everything back if one command fails (default true)."),
        opt("label", String, "Name of the undo step (default \"batch\")."),
    ]),
    // ---- page -------------------------------------------------------------
    query("page.list", "The file's pages in order: id, name, kind (doc, sheet or deck) and a one-line summary.", &[]),
    query("page.get", "One page in full as JSON (the file format).", &[PAGE]),
    query("page.text", "One page as plain text (documents' text, sheets as tab-separated values, slides' text).", &[PAGE]),
    edit("page.add", "Add a page: a document (doc), a sheet or a deck.", &[
        req("kind", String, "doc, sheet or deck."),
        opt("name", String, "Its name (default \"Document 2\", \"Sheet 2\"…). Sheet names are what formulas use."),
        opt("at", Integer, "0-based position (default: after the page shown, else at the end)."),
    ]),
    edit("page.rename", "Rename a page. Formulas, links and charts that name a sheet follow.", &[PAGE, req("name", String, "New name."), COALESCE]),
    edit("page.remove", "Remove a page (history.undo brings it back).", &[PAGE]),
    edit("page.move", "Move a page to another position.", &[PAGE, req("to", Integer, "0-based position.")]),
    edit("page.duplicate", "Copy a page (as \"<name> copy\"), right after it.", &[PAGE]),
    // ---- text (documents and text on slides) ------------------------------
    query("text.read", "The paragraphs of a document page, or of a text box on a slide (with slide and shape): index, id, style, list, and runs with their formatting.", &[PAGE, SLIDE, OPT_SHAPE, opt("from", Integer, "First block index (default 0)."), opt("to", Integer, "Last block index.")]),
    edit("text.insert", "Insert text at a position (a newline starts a new paragraph). Returns the position after it.", &[
        PAGE, SLIDE, OPT_SHAPE,
        req("text", String, "The text."),
        opt("at", Object, "Position {\"block\": 0, \"offset\": 0} (\"cell\": [row, col] inside a table). Default: the window's caret, else the end."),
        opt("style", Object, "Formatting for the new text ({\"bold\": true}…); default: the formatting at the position."),
        COALESCE,
    ]),
    edit("text.delete", "Delete text from one position to another (across paragraphs too). Returns where the caret lands.", &[PAGE, SLIDE, OPT_SHAPE, FROM, req("to", Object, "End position."), COALESCE]),
    edit("text.format", "Change character formatting from one position to another.", &[PAGE, SLIDE, OPT_SHAPE, FROM, req("to", Object, "End position."), BOLD, ITALIC, UNDERLINE, STRIKE, CODE, SUPER, SUB, COLOR, HIGHLIGHT, LINK, SIZE, FONT, COALESCE]),
    edit("text.paragraph", "Change paragraph settings (style, alignment, list) for blocks from..to.", &[PAGE, SLIDE, OPT_SHAPE, req("from", Integer, "First block index."), opt("to", Integer, "Last block index (default: from)."), PSTYLE, ALIGN, LIST, LEVEL, opt("checked", Boolean, "A checklist item is done."), COALESCE]),
    edit("text.paste", "Insert copied content at a position: blocks as JSON (from text.copy), Markdown or plain text. Returns the position after it.", &[
        PAGE, SLIDE, OPT_SHAPE,
        req("at", Object, "Position {\"block\": 0, \"offset\": 0}."),
        opt("blocks", Array, "Blocks as JSON (the file format), e.g. from text.copy.").of(Object),
        opt("markdown", String, "Markdown to convert."),
        opt("text", String, "Plain text."),
    ]),
    query("text.copy", "The content from one position to another as blocks (JSON), Markdown and plain text.", &[PAGE, SLIDE, OPT_SHAPE, FROM, req("to", Object, "End position.")]),
    // ---- doc --------------------------------------------------------------
    query("doc.read", "A document page in reading order: each block with its index, id, kind, style and text; tables as rows; linked tables with their live values. With markdown, the page as Markdown.", &[PAGE, opt("markdown", Boolean, "Return the page as Markdown instead.")]),
    edit("doc.write", "Write Markdown into a document page (headings, paragraphs, **bold**, *italic*, `code`, links, lists, checklists, quotes, tables, --- for a page break). Appends at the end by default.", &[
        PAGE,
        req("markdown", String, "The content, in Markdown."),
        AFTER,
        opt("replace", Boolean, "Replace the whole page instead."),
    ]),
    edit("doc.addParagraph", "Add one paragraph.", &[PAGE, req("text", String, "Its text (plain)."), PSTYLE, ALIGN, LIST, LEVEL, AFTER]),
    edit("doc.setParagraph", "Change a paragraph's style, alignment or list (or a run of them).", &[PAGE, BLOCK, opt("toBlock", Any, "Last block of the run (default: block)."), PSTYLE, ALIGN, LIST, LEVEL, opt("checked", Boolean, "A checklist item is done.")]),
    edit("doc.setText", "Replace a paragraph's text (its style stays; formatting inside it goes).", &[PAGE, BLOCK, req("text", String, "New text.")]),
    edit("doc.format", "Format text found in the page (find, all) or from one position to another (from, to).", &[PAGE, FIND, ALL, opt("from", Object, "Start position."), TO, BOLD, ITALIC, UNDERLINE, STRIKE, CODE, SUPER, SUB, COLOR, HIGHLIGHT, LINK, SIZE, FONT]),
    edit("doc.replace", "Find and replace text in a document page (tables included).", &[PAGE, req("find", String, "Text to find."), req("replace", String, "Replacement."), opt("all", Boolean, "Every match (default true)."), opt("caseSensitive", Boolean, "Match case (default false).")]),
    query("doc.find", "Where text appears in a document page: positions of each match with the surrounding text.", &[PAGE, req("text", String, "Text to find."), opt("caseSensitive", Boolean, "Match case (default false).")]),
    edit("doc.deleteBlocks", "Delete whole blocks (paragraphs, tables, images, charts, page breaks).", &[PAGE, req("from", Any, "First block (index or id)."), opt("to", Any, "Last block (default: from).")]),
    edit("doc.moveBlock", "Move a block before another position.", &[PAGE, BLOCK, req("to", Integer, "New 0-based index.")]),
    edit("doc.insertTable", "Insert a table: empty, from data, or live-linked to a sheet range (its cells then always show the sheet's values).", &[
        PAGE,
        opt("rows", Integer, "Rows (default 3, or the data's)."),
        opt("cols", Integer, "Columns (default 3, or the data's)."),
        opt("data", Array, "Rows of cell text: [[\"Name\", \"Total\"], [\"A\", \"12\"]].").of(Array),
        opt("header", Boolean, "The first row is a header (default true)."),
        opt("link", String, "A sheet range to show live, e.g. 'Budget'!A1:D8."),
        AFTER,
    ]),
    edit("doc.editTable", "Change a table: its data, its link, a cell, rows and columns.", &[
        PAGE, BLOCK,
        opt("data", Array, "All rows of cell text (replaces the table's).").of(Array),
        opt("cell", Array, "[row, col] of one cell to set (0-based), with text.").of(Integer),
        opt("text", String, "With cell: its new text."),
        opt("link", String, "Link to a sheet range; \"\" unlinks (the values stay as text)."),
        opt("header", Boolean, "The first row is a header."),
        opt("banded", Boolean, "Shade every other row."),
        opt("insertRow", Integer, "Insert an empty row at this index."),
        opt("removeRow", Integer, "Remove this row."),
        opt("insertColumn", Integer, "Insert an empty column at this index."),
        opt("removeColumn", Integer, "Remove this column."),
    ]),
    edit("doc.insertImage", "Insert a picture (PNG, JPEG, GIF, WebP) from a file or from the file's media.", &[PAGE, opt("path", String, "Image file."), opt("media", String, "A media id from media.list instead."), opt("width", Number, "Width in points (default: the text width or the picture's size, whichever is smaller)."), opt("caption", String, "Caption under it."), opt("alt", String, "Description for screen readers."), AFTER]).perm(Perm::Files),
    edit("doc.insertChart", "Insert a chart that draws a sheet range live.", &[PAGE, req("source", String, "Sheet range, e.g. 'Sales'!A1:C13."), opt("kind", String, "column (default), bar, line, area, pie or scatter."), opt("title", String, "Title over it."), opt("height", Number, "Height in points (default 240)."), AFTER]),
    edit("doc.insertPageBreak", "Insert a page break.", &[PAGE, AFTER]),
    edit("doc.setup", "Page size, orientation, margins, header and footer of a document page. Header and footer fill in {page}, {pages} and {title}.", &[
        PAGE,
        opt("size", String, "a4, letter, legal, a5 or a3."),
        opt("orientation", String, "portrait or landscape."),
        opt("margins", Number, "All four margins in points (72 = 1 inch, 56.7 = 2 cm)."),
        opt("marginTop", Number, "Top margin in points."),
        opt("marginBottom", Number, "Bottom margin in points."),
        opt("marginLeft", Number, "Left margin in points."),
        opt("marginRight", Number, "Right margin in points."),
        opt("header", String, "Header text (\"\" for none)."),
        opt("footer", String, "Footer text (\"\" for none; default \"{page}\")."),
        opt("differentFirst", Boolean, "No header or footer on the first page."),
    ]),
    query("doc.outline", "The headings of a document page, in order, with their level and block index.", &[PAGE]),
    query("doc.stats", "Words, characters, paragraphs, tables, images and printed pages of a document page.", &[PAGE]),
    edit("doc.comment", "Comment on text (found, or from..to).", &[PAGE, FIND, opt("from", Object, "Start position."), TO, req("text", String, "The comment.")]),
    query("doc.comments", "The comments of a document page with what they are on, their replies and whether they are resolved.", &[PAGE]),
    edit("doc.replyComment", "Reply to a comment.", &[PAGE, req("comment", String, "Comment id."), req("text", String, "The reply.")]),
    edit("doc.resolveComment", "Mark a comment resolved (or open again), or delete it.", &[PAGE, req("comment", String, "Comment id."), opt("resolved", Boolean, "Default true."), opt("delete", Boolean, "Remove it and its mark.")]),
    edit("doc.footnote", "Add a footnote to text (found, or at a position): its number is drawn after the text and the note at the foot of the page.", &[PAGE, FIND, opt("at", Object, "Position of the end of the text it follows."), req("text", String, "The footnote's text.")]),
    edit("doc.trackChanges", "Turn tracked changes on or off for a document page: insertions show underlined and deletions struck through until accepted.", &[PAGE, req("on", Boolean, "On or off.")]),
    edit("doc.resolveChanges", "Accept or reject every tracked change in a document page.", &[PAGE, req("accept", Boolean, "true accepts, false rejects.")]),
    // ---- sheet ------------------------------------------------------------
    query("sheet.read", "Cells of a sheet: shown values, and inputs (formulas) where they differ. Default: the used range, at most 2,000 cells.", &[PAGE, opt("range", String, "A1 range (default: the used range)."), opt("inputs", Boolean, "Return what was typed (formulas) instead of values.")]),
    edit("sheet.set", "Set one cell: a number, text, TRUE/FALSE, a date, or a formula starting with =.", &[PAGE, CELL, req("value", Any, "What to type in it; \"\" empties it."), COALESCE]),
    edit("sheet.setRange", "Fill cells from a 2-D array starting at a cell (row by row). Formulas work; relative references are as typed.", &[PAGE, req("at", String, "Top-left cell, e.g. A1."), req("values", Array, "Rows: [[\"Item\", \"Price\"], [\"Tea\", 3.5], [\"Total\", \"=SUM(B2:B2)\"]].").of(Array)]),
    edit("sheet.clear", "Clear cells: contents, formats or both.", &[PAGE, RANGE, opt("what", String, "contents (default), formats or all.")]),
    edit("sheet.format", "Format cells: number format, text style, colours, alignment, wrap, borders.", &[
        PAGE, RANGE,
        opt("number", String, "Number format code: 0, 0.00, #,##0, 0%, $#,##0.00, #,##0.00 €, yyyy-mm-dd, d mmm yyyy, h:mm, @… or \"general\" (sheet.numberFormats lists presets)."),
        BOLD, ITALIC, UNDERLINE, STRIKE,
        opt("color", String, "Text colour #rrggbb; \"\" clears."),
        opt("fill", String, "Fill colour #rrggbb; \"\" clears."),
        opt("align", String, "left, center, right, or \"\" for automatic."),
        opt("wrap", Boolean, "Wrap long text."),
        SIZE,
        opt("border", String, "Borders: all, none, or letters t r b l (\"b\" underlines the cells)."),
    ]),
    edit("sheet.insertRows", "Insert empty rows; every formula, link and chart keeps pointing at the same cells.", &[PAGE, req("at", Integer, "Row number (1-based) the new rows take."), opt("count", Integer, "How many (default 1).")]),
    edit("sheet.deleteRows", "Delete rows; formulas that pointed into them show #REF!.", &[PAGE, req("at", Integer, "First row number (1-based)."), opt("count", Integer, "How many (default 1).")]),
    edit("sheet.insertColumns", "Insert empty columns; references follow.", &[PAGE, req("at", String, "Column letter the new columns take (e.g. C)."), opt("count", Integer, "How many (default 1).")]),
    edit("sheet.deleteColumns", "Delete columns; references into them show #REF!.", &[PAGE, req("at", String, "First column letter."), opt("count", Integer, "How many (default 1).")]),
    edit("sheet.sort", "Sort the rows of a range by one or more columns.", &[
        PAGE, RANGE,
        opt("by", Array, "Sort keys in order: [{\"column\": \"B\", \"descending\": true}] (default: the range's first column, ascending).").of(Object),
        opt("header", Boolean, "The first row is a header and stays put (default: guessed)."),
    ]),
    edit("sheet.filter", "Filter a range: rows whose cells don't pass are hidden (not deleted). Each call sets one column's rule; clear removes the filter.", &[
        PAGE,
        opt("range", String, "The table to filter, header row included (default: the filter's, else the used range)."),
        opt("column", String, "Column letter the rule is on."),
        opt("values", Array, "Keep rows showing one of these.").of(String),
        opt("condition", String, "Or a condition like >100, <>done, *draft*."),
        opt("clear", Boolean, "Remove the filter (or only column's rule)."),
    ]),
    edit("sheet.fill", "Fill a range from its first cells, like dragging the fill handle: series continue (1, 2 → 3, 4), formulas move their references, other values repeat.", &[PAGE, RANGE, opt("direction", String, "down (default) or right.")]),
    edit("sheet.copy", "Copy (or move) cells to another place, on this sheet or another; formulas move their relative references.", &[PAGE, req("from", String, "Source range."), req("to", String, "Top-left destination cell."), opt("toPage", String, "Destination sheet (default: the same)."), opt("move", Boolean, "Cut instead of copy."), opt("formats", Boolean, "Copy formats too (default true).")]),
    edit("sheet.resize", "Set column widths and row heights in pixels, or fit columns to their content.", &[PAGE, opt("columns", Object, "{\"A\": 160, \"B\": 90}."), opt("rows", Object, "{\"1\": 32}."), opt("fit", String, "Columns to fit to their content, e.g. A:D.")]),
    edit("sheet.freeze", "Keep the first rows and columns in view while scrolling (0 and 0 unfreezes).", &[PAGE, opt("rows", Integer, "Rows to freeze."), opt("columns", Integer, "Columns to freeze.")]),
    edit("sheet.setGridlines", "Show or hide the grid's lines.", &[PAGE, req("on", Boolean, "Show them.")]),
    query("sheet.evaluate", "Compute a formula on a sheet without writing it anywhere (as if typed in cell, default A1).", &[PAGE, req("formula", String, "The formula, with or without =."), opt("cell", String, "Where it is computed from.")]),
    query("sheet.find", "Cells whose shown value or formula contains text.", &[PAGE, req("text", String, "Text to find (not case-sensitive)."), opt("inFormulas", Boolean, "Search formulas too (default true).")]),
    query("sheet.functions", "The spreadsheet functions folio knows (built in and from plugins): name, syntax, what it does, category.", &[opt("search", String, "Only those whose name or description contains this."), opt("category", String, "Math, Statistical, Logical, Lookup, Text, Date, Financial, Information or Custom.")]),
    query("sheet.numberFormats", "Number format presets (codes for sheet.format number), each with an example.", &[]),
    edit("sheet.addChart", "Add a chart over the sheet, drawing a range live.", &[PAGE, RANGE, opt("kind", String, "column (default), bar, line, area, pie or scatter."), opt("title", String, "Title."), opt("seriesInRows", Boolean, "Each row is a series (default: each column)."), opt("x", Number, "Left in pixels from the grid's corner (default: right of the data)."), opt("y", Number, "Top in pixels."), opt("w", Number, "Width (default 480)."), opt("h", Number, "Height (default 300).")]),
    edit("sheet.updateChart", "Change a chart on a sheet.", &[PAGE, req("chart", String, "Chart id (sheet.read lists them)."), opt("range", String, "New source range."), opt("kind", String, "New kind."), opt("title", String, "New title."), opt("seriesInRows", Boolean, "Series in rows."), opt("legend", Boolean, "Show the legend."), opt("stacked", Boolean, "Stack bars and areas."), opt("x", Number, "Left."), opt("y", Number, "Top."), opt("w", Number, "Width."), opt("h", Number, "Height.")]),
    edit("sheet.removeChart", "Remove a chart from a sheet.", &[PAGE, req("chart", String, "Chart id.")]),
    // ---- deck -------------------------------------------------------------
    query("deck.read", "A deck: size, theme, and every slide with its layout, notes and shapes (id, name, kind, position, size, text).", &[PAGE]),
    edit("deck.addSlide", "Add a slide with a layout's placeholders, filled with title and body.", &[
        PAGE,
        opt("layout", String, "title, titleContent (default), section, twoContent, titleOnly or blank."),
        opt("title", String, "Title text."),
        opt("body", String, "Body text: one line per bullet; for twoContent, a blank line between the columns."),
        opt("notes", String, "Speaker notes."),
        opt("at", Integer, "0-based position (default: after the slide shown, else at the end)."),
    ]),
    edit("deck.removeSlide", "Remove a slide.", &[PAGE, SLIDE]),
    edit("deck.moveSlide", "Move a slide.", &[PAGE, SLIDE, req("to", Integer, "New 0-based position.")]),
    edit("deck.duplicateSlide", "Copy a slide, right after it.", &[PAGE, SLIDE]),
    edit("deck.setSlide", "Change a slide: its title or body placeholder's text, layout, background, speaker notes, hidden.", &[PAGE, SLIDE, opt("title", String, "Title text."), opt("body", String, "Body text (one line per bullet)."), opt("layout", String, "Apply a layout (adds missing placeholders)."), opt("background", String, "#rrggbb, or \"\" for the theme's."), opt("notes", String, "Speaker notes."), opt("hidden", Boolean, "Skip when presenting and exporting.")]),
    edit("deck.addShape", "Add a text box or a shape (rect, ellipse, triangle, line, arrow) to a slide.", &[
        PAGE, SLIDE,
        req("kind", String, "text, rect, ellipse, triangle, line or arrow."),
        X, Y, W, H,
        opt("text", String, "Text inside (one paragraph per line)."),
        opt("fill", String, "Fill #rrggbb."),
        opt("line", String, "Outline colour #rrggbb."),
        opt("lineWidth", Number, "Outline width in points."),
        opt("textSize", Number, "Text size in points (default 20)."),
        opt("color", String, "Text colour #rrggbb (default: the theme's)."),
        ALIGN,
        opt("name", String, "A name to find it by."),
    ]),
    edit("deck.updateShape", "Move, resize or restyle a shape, or replace its text.", &[
        PAGE, SLIDE, SHAPE, X, Y, W, H,
        opt("rotation", Number, "Degrees clockwise."),
        opt("text", String, "Replace its text (one paragraph per line)."),
        opt("fill", String, "Fill #rrggbb; \"\" for none."),
        opt("line", String, "Outline #rrggbb; \"\" for none."),
        opt("lineWidth", Number, "Outline width in points."),
        opt("textSize", Number, "Text size in points."),
        opt("color", String, "Text colour; \"\" for the theme's."),
        opt("valign", String, "top, middle or bottom."),
        opt("name", String, "New name."),
        COALESCE,
    ]),
    edit("deck.removeShape", "Remove a shape.", &[PAGE, SLIDE, SHAPE]),
    edit("deck.duplicateShape", "Copy a shape (offset a little).", &[PAGE, SLIDE, SHAPE]),
    edit("deck.arrange", "Bring a shape forward or send it back.", &[PAGE, SLIDE, SHAPE, req("order", String, "front, back, forward or backward.")]),
    edit("deck.formatText", "Format all the text in a shape: character formatting and paragraph settings.", &[PAGE, SLIDE, SHAPE, BOLD, ITALIC, UNDERLINE, STRIKE, COLOR, HIGHLIGHT, LINK, FONT, PSTYLE, ALIGN, LIST]),
    edit("deck.addImage", "Put a picture on a slide.", &[PAGE, SLIDE, opt("path", String, "Image file."), opt("media", String, "A media id instead."), X, Y, W, H]).perm(Perm::Files),
    edit("deck.addChart", "Put a chart on a slide that draws a sheet range live.", &[PAGE, SLIDE, req("source", String, "Sheet range, e.g. 'Sales'!A1:C13."), opt("kind", String, "column (default), bar, line, area, pie or scatter."), opt("title", String, "Title."), X, Y, W, H]),
    edit("deck.addTable", "Put a table on a slide: from data, or live-linked to a sheet range.", &[PAGE, SLIDE, opt("data", Array, "Rows of cell text.").of(Array), opt("rows", Integer, "Rows (default 3)."), opt("cols", Integer, "Columns (default 3)."), opt("link", String, "Sheet range shown live."), X, Y, W, H]),
    query("deck.themes", "The built-in deck themes with their colours and fonts.", &[]),
    edit("deck.setTheme", "Apply a theme to a deck, or change its colours and fonts.", &[PAGE, opt("theme", String, "paper, ink, grain, serif or mono."), opt("background", String, "#rrggbb."), opt("text", String, "#rrggbb."), opt("accent", String, "#rrggbb."), opt("headingFont", String, "display, sans, serif or mono."), opt("bodyFont", String, "display, sans, serif or mono.")]),
    edit("deck.present", "Present a deck full screen from a slide (Escape ends it).", &[PAGE, SLIDE, opt("presenter", Boolean, "Show the presenter view (notes, next slide, timer) instead.")]).window(),
    // ---- links, media -----------------------------------------------------
    query("link.list", "Every live link in the file: which table or chart on which page reads which sheet range, and whether it still resolves.", &[]),
    query("media.list", "Pictures in the file: id, name, type, size in pixels and bytes, and where each is used.", &[]),
    edit("media.add", "Add a picture file to the file's media (to place later by media id).", &[req("path", String, "Image file.")]).perm(Perm::Files),
    // ---- history ----------------------------------------------------------
    query("history.list", "The undo history: steps to undo (newest first) and to redo, each with what made it (window, agent, cli, mcp).", &[]),
    edit("history.undo", "Undo the last change, whoever made it.", &[]),
    edit("history.redo", "Redo the last undone change.", &[]),
    query("history.checkpoint", "The history position now, to come back to with history.revertTo.", &[]),
    edit("history.revertTo", "Undo every change made after a checkpoint (an agent's whole run).", &[req("checkpoint", Integer, "From history.checkpoint.")]),
    // ---- handoff (lsuite) -------------------------------------------------
    query("handoff.apps", "The other lsuite apps on this computer (from ~/.lsuite/apps): name, kind, version, whether running, and what folio can take from each (a picture from nori, a frame from kimchi).", &[]),
    edit("handoff.image", "Bring a picture from another lsuite app into the open file through its CLI: nori exports its open image (or a file), kimchi renders a frame of its open project (or a project file) at a time. It lands in a document page or on a slide.", &[
        req("app", String, "nori or kimchi."),
        opt("file", String, "A document of that app to take it from (default: what the app has open)."),
        opt("time", Number, "kimchi: the time in seconds (default: its playhead)."),
        PAGE, SLIDE, AFTER,
    ]).perm(Perm::AppControl),
    // ---- account (lsuite AI) ----------------------------------------------
    query("account.status", "lsuite AI: whether this computer is signed in (shared by every lsuite app), the account's email, plan, allowance used and when it resets, and the models in the plan.", &[opt("refresh", Boolean, "Ask the server again now.")]),
    edit("account.signIn", "Sign in to lsuite AI: opens the browser to connect folio (the account is shared by every lsuite app), or takes a key from the account page (lsk_…).", &[opt("key", String, "A key from lsuite.xyz/account, for headless sign-in."), opt("wait", Boolean, "Wait for the browser sign-in to finish (default false).")]).perm(Perm::PersonOnly),
    edit("account.signOut", "Sign out of lsuite AI on this computer (every lsuite app).", &[]).perm(Perm::PersonOnly),
    query("account.plans", "lsuite AI plans with prices, models and monthly allowances (from the server).", &[]),
    // ---- plugins ----------------------------------------------------------
    query("plugin.list", "Plugins: stock (shipped with folio), installed (lsuite plugins built in Rust), with id, name, kind (functions or filter), version, path and whether enabled.", &[]),
    query("plugin.info", "One plugin: what it adds (functions with their syntax, file formats), where it came from, its manifest.", &[req("id", String, "Plugin id.")]),
    edit("plugin.enable", "Switch a plugin on.", &[req("id", String, "Plugin id.")]).perm(Perm::Plugins),
    edit("plugin.disable", "Switch a plugin off (it stays installed).", &[req("id", String, "Plugin id.")]).perm(Perm::Plugins),
    edit("plugin.rescan", "Look in the plugin folder again and reload plugins that changed.", &[]).perm(Perm::Plugins),
    edit("plugin.install", "Install a built plugin bundle (a folder with plugin.toml and the library) and load it.", &[req("path", String, "The bundle folder.")]).perm(Perm::Plugins),
    edit("plugin.remove", "Remove an installed lsuite plugin (stock plugins can only be disabled).", &[req("id", String, "Plugin id.")]).perm(Perm::Plugins),
    query("plugin.guide", "How to write a folio plugin in Rust: the SDK, the kinds, the manifest, an example and the steps (Markdown, for agents).", &[]),
    query("plugin.toolchain", "Whether Rust (cargo, rustc) is installed, its version, and how to install it.", &[]),
    edit("plugin.new", "Start a plugin crate from the SDK template in ~/.lsuite/plugins-src/folio/<name>/.", &[req("name", String, "Crate name (lowercase, dashes)."), opt("kind", String, "functions (default) or filter.")]).perm(Perm::Plugins),
    edit("plugin.writeSource", "Write one file inside a plugin crate (paths outside it are refused).", &[req("name", String, "Crate name."), req("path", String, "Path inside the crate, e.g. src/lib.rs."), req("contents", String, "The file's contents.")]).perm(Perm::Plugins),
    edit("plugin.build", "Build a plugin crate (cargo build --release); returns ok and the compiler's errors as {file, line, message}.", &[req("name", String, "Crate name.")]).perm(Perm::Plugins),
    edit("plugin.publishLocal", "Bundle a built plugin crate and install it: its functions work at once.", &[req("name", String, "Crate name.")]).perm(Perm::Plugins),
    // ---- agent ------------------------------------------------------------
    query("agent.providers", "What can run the built-in agent: lsuite AI (no setup), coding CLIs on this computer (Claude Code, Codex), model APIs (Anthropic, OpenAI, OpenRouter, Google Gemini, Mistral) and local servers (Ollama, LM Studio); whether each is ready and what to do next.", &[opt("refresh", Boolean, "Check again now.")]).window(),
    query("agent.models", "The models a provider offers for the agent (the chosen one by default).", &[opt("provider", String, "A provider id from agent.providers."), opt("refresh", Boolean, "Fetch the list again now.")]).window(),
    edit("agent.setProvider", "Choose what runs the built-in agent (Settings › Agent).", &[req("provider", String, "A provider id from agent.providers."), opt("model", String, "Model id; empty for the provider's default."), opt("baseUrl", String, "A local or compatible server's address; empty for the default.")]).perm(Perm::PersonOnly),
    edit("agent.send", "Ask the built-in agent (the Agent panel) to do something, in words. It runs commands like any client (permissions apply) and shows them as cards. Returns the run at once, or once it ends with wait.", &[req("prompt", String, "The request."), opt("wait", Boolean, "Wait until the run ends (default false)."), opt("timeout", Number, "With wait: stop waiting after this many seconds (default 900).")]).window(),
    query("agent.status", "One agent run (the running or latest by default): request, whether it is working, the reply, every command it ran, changes, time and tokens.", &[opt("run", Integer, "Run id from agent.runs."), opt("wait", Boolean, "Wait until the run ends."), opt("timeout", Number, "With wait: seconds (default 900).")]).window(),
    query("agent.runs", "The agent's runs on the open file, oldest first.", &[]).window(),
    query("agent.conversation", "The Agent panel's conversation as it shows it.", &[opt("since", Integer, "Only entries from this index on.")]).window(),
    edit("agent.stop", "Stop the agent's run. Edits it finished stay (agent.revert removes them).", &[]).window(),
    edit("agent.revert", "Revert an agent run as one undo step.", &[opt("run", Integer, "Run id (default: the latest that changed something).")]).window(),
    edit("agent.newConversation", "Start a new conversation in the Agent panel.", &[]).window(),
    edit("agent.steer", "Redirect the running agent with a follow-up message.", &[req("prompt", String, "Additional instructions.")]).window(),
    // ---- app --------------------------------------------------------------
    query("app.info", "Version, folders, whether the window and the bridge run, and the open file.", &[]),
    query("app.commands", "Describe every command with its parameters, or one command.", &[opt("command", String, "One command name.")]),
    query("app.settings", "Every setting with its value.", &[]),
    edit("app.setSetting", "Change one setting by dotted key, e.g. appearance.mode or editing.author. Agent permissions stay with the person.", &[req("key", String, "Dotted key from app.settings."), req("value", Any, "New value (same type).")]).perm(Perm::Settings),
    edit("app.setAgentKey", "Save (or with no key, remove) an API key the built-in agent uses, in the OS keychain.", &[req("provider", String, "anthropic, openai, openrouter, gemini or mistral."), opt("key", String, "The key; empty removes it.")]).perm(Perm::PersonOnly),
    query("app.onboarding", "The first-run setup: whether it was done, the suites a person may come from (with the formats folio opens from each), the agent providers found on this computer and lsuite AI.", &[]),
    edit("app.finishOnboarding", "Finish (or skip) the first-run setup with the choices made.", &[opt("comingFrom", String, "office, google, apple, libreoffice or none."), opt("agent", Boolean, "Offer the Agent panel."), opt("provider", String, "The agent provider to use."), opt("author", String, "The name on comments and tracked changes.")]),
    query("app.checkUpdates", "Look on GitHub Releases for a newer folio and say where to get it.", &[]),
    edit("app.quit", "Quit folio (everything is saved).", &[]).perm(Perm::AppControl).window(),
    // ---- ui ---------------------------------------------------------------
    query("ui.state", "What the window shows: home or editor, the page, the caret or selected cells or slide, zoom, theme, open panels.", &[]),
    edit("ui.show", "Show a page in the window (and a slide, a cell or a block).", &[PAGE, SLIDE, opt("cell", String, "A cell to select."), opt("block", Integer, "A block to scroll to.")]).window(),
    edit("ui.select", "Select in the window: text from..to on a document page, a range of cells, or shapes on a slide.", &[opt("from", Object, "Text start {block, offset}."), opt("to", Object, "Text end."), opt("range", String, "Cells, A1."), opt("shapes", Array, "Shape ids.").of(String)]).window(),
    edit("ui.panel", "Open or close a panel or dialog: agent, inspector, pages, settings, export, open, plugins, account, palette, shortcuts, onboarding; or home.", &[req("name", String, "Panel or dialog name."), opt("open", Boolean, "Default true.")]).window(),
    edit("ui.zoom", "Zoom the work area.", &[req("level", Number, "1 = 100 % (0.25 to 4)."), ]).window(),
    edit("ui.theme", "Dark, light or follow the system.", &[req("mode", String, "dark, light or system.")]).perm(Perm::Settings),
    edit("ui.screenshot", "Save a PNG of the window and return its path (macOS).", &[opt("path", String, "Destination .png (default: a temporary file).")]).perm(Perm::Files).window(),
];

pub async fn dispatch(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    // Each family's handler is boxed: their futures are large (big matches), and on the stack of
    // a 2 MB thread (tests, tokio workers in debug builds) they would overflow it.
    match cx.spec.family() {
        "file" => Box::pin(file::run(s, cx, a)).await,
        "page" => Box::pin(page::run(s, cx, a)).await,
        "text" => Box::pin(text::run(s, cx, a)).await,
        "doc" => Box::pin(doc::run(s, cx, a)).await,
        "sheet" => Box::pin(sheet::run(s, cx, a)).await,
        "deck" => Box::pin(deck::run(s, cx, a)).await,
        "link" | "media" => Box::pin(page::run_misc(s, cx, a)).await,
        "history" => Box::pin(history::run(s, cx, a)).await,
        "handoff" => Box::pin(handoff::run(s, cx, a)).await,
        "account" => Box::pin(account::run(s, cx, a)).await,
        "plugin" => Box::pin(plugin::run(s, cx, a)).await,
        "agent" => Box::pin(agent::run(s, cx, a)).await,
        "app" => Box::pin(app::run(s, cx, a)).await,
        "ui" => Box::pin(ui::run(s, cx, a)).await,
        _ => Err(unhandled(cx)),
    }
}

pub fn unhandled(cx: &Ctx) -> std::string::String {
    format!("`{}` isn't handled (a bug in folio).", cx.spec.name)
}

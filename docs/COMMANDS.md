# folio commands

Generated from the command registry (`crates/folio-control`) by `folio-cli docs`. Do not edit by hand.

Every command works the same from the window, the built-in agent, `folio-cli` and `folio-mcp` (where `family.verb` becomes the tool `family_verb`). Pages are named by id, name or 1-based number; cells and ranges use A1 notation; text offsets count characters. Commands that edit the file also accept `coalesce` (consecutive edits from the same source and key fold into one undo step within about a second; a unique `gesture:` key keeps a gesture together across pauses). See [AI_CONTROL.md](AI_CONTROL.md).

## file

### `file.overview`

The whole open file in one bounded answer: title, where it is saved, every page with what it holds (documents: outline, words, first paragraphs; sheets: used range, a sample of values and formulas, errors; decks: each slide's title and text), live links between pages, undo history and what the window shows. Read it first. _(read only)_

### `file.info`

Where the open file is saved (or that it isn't yet), where it was imported from, its title, pages and whether everything is written to disk. _(read only)_

### `file.get`

The complete open file as JSON (the .folio format's document.json, media listed without their bytes). _(read only)_

### `file.recent`

Files opened recently, newest first, with whether each still exists. _(read only)_

### `file.formats`

What folio opens and writes: each format with its extensions, the apps that use it (Microsoft Word, Excel, PowerPoint, Google Docs, Sheets and Slides through their downloads, Apple Pages, Numbers and Keynote through their exports, LibreOffice), which kinds of page it carries, and what doesn't survive the trip. _(read only)_

### `file.templates`

Templates for file.new: a report, a letter, a budget, an invoice, a project plan, a pitch deck… _(read only)_

### `file.new`

Create a new file and open it (it saves itself in folio's untitled folder until file.saveAs). _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `title` | string |  | Title (default "Untitled"). |
| `kind` | string |  | The first page: doc (default), sheet, deck, or blank for none. |
| `template` | string |  | A template id from file.templates instead. |

### `file.open`

Open a .folio file, or import a Word, Excel, PowerPoint, OpenDocument, CSV or Markdown file as a new untitled folio file (the original is left alone; file.export writes it back). Replaces the open file. _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string | required | The file to open. |

### `file.import`

Add the pages of another file (Word, Excel, PowerPoint, OpenDocument, CSV, Markdown, or another .folio) to the open file, as one undo step. _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string | required | The file to bring in. |
| `at` | integer |  | Insert the pages at this 0-based position (default: at the end). |

### `file.save`

Write the open file to disk now (it also saves itself a moment after every change). An untitled file needs file.saveAs. _(changes things)_

### `file.saveAs`

Save the open file as a .folio file at a path; it saves there from now on. _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string | required | Destination .folio file. |

### `file.export`

Write the open file (or some pages) in another format: pdf, docx, xlsx, pptx, odt, ods, odp, csv, md or html. Documents go to word processors, sheets to spreadsheets, decks to presentations; PDF and HTML take every kind. Returns what the format couldn't carry. _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string | required | Destination file; its extension picks the format unless format is given. |
| `format` | string |  | pdf, docx, xlsx, pptx, odt, ods, odp, csv, md or html. |
| `pages` | array of strings |  | Only these pages (ids, names or 1-based numbers). |

### `file.rename`

Change the file's title (not its file name). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `title` | string | required | New title. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `file.close`

Close the open file (everything is saved) and go back to the home screen. _(changes things)_

### `file.batch`

Run several commands as one undo step. With atomic (the default) a failing command rolls back the ones before it. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `commands` | array of objects | required | Array of {"command": "sheet.set", "params": {…}}. |
| `atomic` | boolean |  | Roll everything back if one command fails (default true). |
| `label` | string |  | Name of the undo step (default "batch"). |

## page

### `page.list`

The file's pages in order: id, name, kind (doc, sheet or deck) and a one-line summary. _(read only)_

### `page.get`

One page in full as JSON (the file format). _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |

### `page.text`

One page as plain text (documents' text, sheets as tab-separated values, slides' text). _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |

### `page.add`

Add a page: a document (doc), a sheet or a deck. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `kind` | string | required | doc, sheet or deck. |
| `name` | string |  | Its name (default "Document 2", "Sheet 2"…). Sheet names are what formulas use. |
| `at` | integer |  | 0-based position (default: after the page shown, else at the end). |

### `page.rename`

Rename a page. Formulas, links and charts that name a sheet follow. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `name` | string | required | New name. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `page.remove`

Remove a page (history.undo brings it back). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |

### `page.move`

Move a page to another position. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `to` | integer | required | 0-based position. |

### `page.duplicate`

Copy a page (as "<name> copy"), right after it. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |

## text

### `text.read`

The paragraphs of a document page, or of a text box on a slide (with slide and shape): index, id, style, list, and runs with their formatting. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `shape` | string |  | A text box or shape on a slide (with page and slide): its id or name. Without it, the text is the document page's. |
| `from` | integer |  | First block index (default 0). |
| `to` | integer |  | Last block index. |

### `text.insert`

Insert text at a position (a newline starts a new paragraph). Returns the position after it. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `shape` | string |  | A text box or shape on a slide (with page and slide): its id or name. Without it, the text is the document page's. |
| `text` | string | required | The text. |
| `at` | object |  | Position {"block": 0, "offset": 0} ("cell": [row, col] inside a table). Default: the window's caret, else the end. |
| `style` | object |  | Formatting for the new text ({"bold": true}…); default: the formatting at the position. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `text.delete`

Delete text from one position to another (across paragraphs too). Returns where the caret lands. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `shape` | string |  | A text box or shape on a slide (with page and slide): its id or name. Without it, the text is the document page's. |
| `from` | object | required | Position {"block": 0, "offset": 0} (offset in characters; add "cell": [row, col] inside a table). |
| `to` | object | required | End position. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `text.format`

Change character formatting from one position to another. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `shape` | string |  | A text box or shape on a slide (with page and slide): its id or name. Without it, the text is the document page's. |
| `from` | object | required | Position {"block": 0, "offset": 0} (offset in characters; add "cell": [row, col] inside a table). |
| `to` | object | required | End position. |
| `bold` | boolean |  | Bold on or off. |
| `italic` | boolean |  | Italic on or off. |
| `underline` | boolean |  | Underline on or off. |
| `strike` | boolean |  | Strikethrough on or off. |
| `code` | boolean |  | Inline code (mono) on or off. |
| `superscript` | boolean |  | Superscript on or off. |
| `subscript` | boolean |  | Subscript on or off. |
| `color` | string |  | Text colour #rrggbb; "" clears. |
| `highlight` | string |  | Highlight colour #rrggbb; "" clears. |
| `link` | string |  | Link URL; "" removes the link. |
| `size` | number |  | Size in points; 0 goes back to the paragraph style's. |
| `font` | string |  | sans, serif, mono or display; "" goes back to the style's. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `text.paragraph`

Change paragraph settings (style, alignment, list) for blocks from..to. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `shape` | string |  | A text box or shape on a slide (with page and slide): its id or name. Without it, the text is the document page's. |
| `from` | integer | required | First block index. |
| `to` | integer |  | Last block index (default: from). |
| `style` | string |  | Paragraph style: normal, title, subtitle, heading1, heading2, heading3, quote, code or caption. |
| `align` | string |  | left, center, right or justify. |
| `list` | string |  | bullet, number, check, or none. |
| `level` | integer |  | List level, 0 to 5. |
| `checked` | boolean |  | A checklist item is done. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `text.paste`

Insert copied content at a position: blocks as JSON (from text.copy), Markdown or plain text. Returns the position after it. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `shape` | string |  | A text box or shape on a slide (with page and slide): its id or name. Without it, the text is the document page's. |
| `at` | object | required | Position {"block": 0, "offset": 0}. |
| `blocks` | array of objects |  | Blocks as JSON (the file format), e.g. from text.copy. |
| `markdown` | string |  | Markdown to convert. |
| `text` | string |  | Plain text. |

### `text.copy`

The content from one position to another as blocks (JSON), Markdown and plain text. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `shape` | string |  | A text box or shape on a slide (with page and slide): its id or name. Without it, the text is the document page's. |
| `from` | object | required | Position {"block": 0, "offset": 0} (offset in characters; add "cell": [row, col] inside a table). |
| `to` | object | required | End position. |

## doc

### `doc.read`

A document page in reading order: each block with its index, id, kind, style and text; tables as rows; linked tables with their live values. With markdown, the page as Markdown. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `markdown` | boolean |  | Return the page as Markdown instead. |

### `doc.write`

Write Markdown into a document page (headings, paragraphs, **bold**, *italic*, `code`, links, lists, checklists, quotes, tables, --- for a page break). Appends at the end by default. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `markdown` | string | required | The content, in Markdown. |
| `after` | any |  | Put it after this block (index from doc.read, or id); -1 puts it first. Default: after the caret's block in the window, else at the end. |
| `replace` | boolean |  | Replace the whole page instead. |

### `doc.addParagraph`

Add one paragraph. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `text` | string | required | Its text (plain). |
| `style` | string |  | Paragraph style: normal, title, subtitle, heading1, heading2, heading3, quote, code or caption. |
| `align` | string |  | left, center, right or justify. |
| `list` | string |  | bullet, number, check, or none. |
| `level` | integer |  | List level, 0 to 5. |
| `after` | any |  | Put it after this block (index from doc.read, or id); -1 puts it first. Default: after the caret's block in the window, else at the end. |

### `doc.setParagraph`

Change a paragraph's style, alignment or list (or a run of them). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `block` | any | required | Block index from doc.read (0 is the first) or its id. |
| `toBlock` | any |  | Last block of the run (default: block). |
| `style` | string |  | Paragraph style: normal, title, subtitle, heading1, heading2, heading3, quote, code or caption. |
| `align` | string |  | left, center, right or justify. |
| `list` | string |  | bullet, number, check, or none. |
| `level` | integer |  | List level, 0 to 5. |
| `checked` | boolean |  | A checklist item is done. |

### `doc.setText`

Replace a paragraph's text (its style stays; formatting inside it goes). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `block` | any | required | Block index from doc.read (0 is the first) or its id. |
| `text` | string | required | New text. |

### `doc.format`

Format text found in the page (find, all) or from one position to another (from, to). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `find` | string |  | Text to look for instead of a position (the first match, or every match with all). |
| `all` | boolean |  | With find: every match (default false: the first). |
| `from` | object |  | Start position. |
| `to` | object |  | End position, same shape as from (default: from, or the end of from's block when from has no offset). |
| `bold` | boolean |  | Bold on or off. |
| `italic` | boolean |  | Italic on or off. |
| `underline` | boolean |  | Underline on or off. |
| `strike` | boolean |  | Strikethrough on or off. |
| `code` | boolean |  | Inline code (mono) on or off. |
| `superscript` | boolean |  | Superscript on or off. |
| `subscript` | boolean |  | Subscript on or off. |
| `color` | string |  | Text colour #rrggbb; "" clears. |
| `highlight` | string |  | Highlight colour #rrggbb; "" clears. |
| `link` | string |  | Link URL; "" removes the link. |
| `size` | number |  | Size in points; 0 goes back to the paragraph style's. |
| `font` | string |  | sans, serif, mono or display; "" goes back to the style's. |

### `doc.replace`

Find and replace text in a document page (tables included). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `find` | string | required | Text to find. |
| `replace` | string | required | Replacement. |
| `all` | boolean |  | Every match (default true). |
| `caseSensitive` | boolean |  | Match case (default false). |

### `doc.find`

Where text appears in a document page: positions of each match with the surrounding text. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `text` | string | required | Text to find. |
| `caseSensitive` | boolean |  | Match case (default false). |

### `doc.deleteBlocks`

Delete whole blocks (paragraphs, tables, images, charts, page breaks). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `from` | any | required | First block (index or id). |
| `to` | any |  | Last block (default: from). |

### `doc.moveBlock`

Move a block before another position. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `block` | any | required | Block index from doc.read (0 is the first) or its id. |
| `to` | integer | required | New 0-based index. |

### `doc.insertTable`

Insert a table: empty, from data, or live-linked to a sheet range (its cells then always show the sheet's values). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `rows` | integer |  | Rows (default 3, or the data's). |
| `cols` | integer |  | Columns (default 3, or the data's). |
| `data` | array of arrays |  | Rows of cell text: [["Name", "Total"], ["A", "12"]]. |
| `header` | boolean |  | The first row is a header (default true). |
| `link` | string |  | A sheet range to show live, e.g. 'Budget'!A1:D8. |
| `after` | any |  | Put it after this block (index from doc.read, or id); -1 puts it first. Default: after the caret's block in the window, else at the end. |

### `doc.editTable`

Change a table: its data, its link, a cell, rows and columns. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `block` | any | required | Block index from doc.read (0 is the first) or its id. |
| `data` | array of arrays |  | All rows of cell text (replaces the table's). |
| `cell` | array of integers |  | [row, col] of one cell to set (0-based), with text. |
| `text` | string |  | With cell: its new text. |
| `link` | string |  | Link to a sheet range; "" unlinks (the values stay as text). |
| `header` | boolean |  | The first row is a header. |
| `banded` | boolean |  | Shade every other row. |
| `insertRow` | integer |  | Insert an empty row at this index. |
| `removeRow` | integer |  | Remove this row. |
| `insertColumn` | integer |  | Insert an empty column at this index. |
| `removeColumn` | integer |  | Remove this column. |

### `doc.insertImage`

Insert a picture (PNG, JPEG, GIF, WebP) from a file or from the file's media. _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `path` | string |  | Image file. |
| `media` | string |  | A media id from media.list instead. |
| `width` | number |  | Width in points (default: the text width or the picture's size, whichever is smaller). |
| `caption` | string |  | Caption under it. |
| `alt` | string |  | Description for screen readers. |
| `after` | any |  | Put it after this block (index from doc.read, or id); -1 puts it first. Default: after the caret's block in the window, else at the end. |

### `doc.insertChart`

Insert a chart that draws a sheet range live. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `source` | string | required | Sheet range, e.g. 'Sales'!A1:C13. |
| `kind` | string |  | column (default), bar, line, area, pie or scatter. |
| `title` | string |  | Title over it. |
| `height` | number |  | Height in points (default 240). |
| `after` | any |  | Put it after this block (index from doc.read, or id); -1 puts it first. Default: after the caret's block in the window, else at the end. |

### `doc.insertPageBreak`

Insert a page break. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `after` | any |  | Put it after this block (index from doc.read, or id); -1 puts it first. Default: after the caret's block in the window, else at the end. |

### `doc.setup`

Page size, orientation, margins, header and footer of a document page. Header and footer fill in {page}, {pages} and {title}. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `size` | string |  | a4, letter, legal, a5 or a3. |
| `orientation` | string |  | portrait or landscape. |
| `margins` | number |  | All four margins in points (72 = 1 inch, 56.7 = 2 cm). |
| `marginTop` | number |  | Top margin in points. |
| `marginBottom` | number |  | Bottom margin in points. |
| `marginLeft` | number |  | Left margin in points. |
| `marginRight` | number |  | Right margin in points. |
| `header` | string |  | Header text ("" for none). |
| `footer` | string |  | Footer text ("" for none; default "{page}"). |
| `differentFirst` | boolean |  | No header or footer on the first page. |

### `doc.outline`

The headings of a document page, in order, with their level and block index. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |

### `doc.stats`

Words, characters, paragraphs, tables, images and printed pages of a document page. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |

### `doc.comment`

Comment on text (found, or from..to). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `find` | string |  | Text to look for instead of a position (the first match, or every match with all). |
| `from` | object |  | Start position. |
| `to` | object |  | End position, same shape as from (default: from, or the end of from's block when from has no offset). |
| `text` | string | required | The comment. |

### `doc.comments`

The comments of a document page with what they are on, their replies and whether they are resolved. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |

### `doc.replyComment`

Reply to a comment. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `comment` | string | required | Comment id. |
| `text` | string | required | The reply. |

### `doc.resolveComment`

Mark a comment resolved (or open again), or delete it. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `comment` | string | required | Comment id. |
| `resolved` | boolean |  | Default true. |
| `delete` | boolean |  | Remove it and its mark. |

### `doc.footnote`

Add a footnote to text (found, or at a position): its number is drawn after the text and the note at the foot of the page. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `find` | string |  | Text to look for instead of a position (the first match, or every match with all). |
| `at` | object |  | Position of the end of the text it follows. |
| `text` | string | required | The footnote's text. |

### `doc.trackChanges`

Turn tracked changes on or off for a document page: insertions show underlined and deletions struck through until accepted. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `on` | boolean | required | On or off. |

### `doc.resolveChanges`

Accept or reject every tracked change in a document page. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `accept` | boolean | required | true accepts, false rejects. |

## sheet

### `sheet.read`

Cells of a sheet: shown values, and inputs (formulas) where they differ. Default: the used range, at most 2,000 cells. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `range` | string |  | A1 range (default: the used range). |
| `inputs` | boolean |  | Return what was typed (formulas) instead of values. |

### `sheet.set`

Set one cell: a number, text, TRUE/FALSE, a date, or a formula starting with =. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `cell` | string | required | A cell, A1 notation. |
| `value` | any | required | What to type in it; "" empties it. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `sheet.setRange`

Fill cells from a 2-D array starting at a cell (row by row). Formulas work; relative references are as typed. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `at` | string | required | Top-left cell, e.g. A1. |
| `values` | array of arrays | required | Rows: [["Item", "Price"], ["Tea", 3.5], ["Total", "=SUM(B2:B2)"]]. |

### `sheet.clear`

Clear cells: contents, formats or both. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `range` | string | required | Cells in A1 notation: B2, B2:D9, B:B, 2:2. |
| `what` | string |  | contents (default), formats or all. |

### `sheet.format`

Format cells: number format, text style, colours, alignment, wrap, borders. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `range` | string | required | Cells in A1 notation: B2, B2:D9, B:B, 2:2. |
| `number` | string |  | Number format code: 0, 0.00, #,##0, 0%, $#,##0.00, #,##0.00 €, yyyy-mm-dd, d mmm yyyy, h:mm, @… or "general" (sheet.numberFormats lists presets). |
| `bold` | boolean |  | Bold on or off. |
| `italic` | boolean |  | Italic on or off. |
| `underline` | boolean |  | Underline on or off. |
| `strike` | boolean |  | Strikethrough on or off. |
| `color` | string |  | Text colour #rrggbb; "" clears. |
| `fill` | string |  | Fill colour #rrggbb; "" clears. |
| `align` | string |  | left, center, right, or "" for automatic. |
| `wrap` | boolean |  | Wrap long text. |
| `size` | number |  | Size in points; 0 goes back to the paragraph style's. |
| `border` | string |  | Borders: all, none, or letters t r b l ("b" underlines the cells). |

### `sheet.insertRows`

Insert empty rows; every formula, link and chart keeps pointing at the same cells. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `at` | integer | required | Row number (1-based) the new rows take. |
| `count` | integer |  | How many (default 1). |

### `sheet.deleteRows`

Delete rows; formulas that pointed into them show #REF!. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `at` | integer | required | First row number (1-based). |
| `count` | integer |  | How many (default 1). |

### `sheet.insertColumns`

Insert empty columns; references follow. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `at` | string | required | Column letter the new columns take (e.g. C). |
| `count` | integer |  | How many (default 1). |

### `sheet.deleteColumns`

Delete columns; references into them show #REF!. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `at` | string | required | First column letter. |
| `count` | integer |  | How many (default 1). |

### `sheet.sort`

Sort the rows of a range by one or more columns. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `range` | string | required | Cells in A1 notation: B2, B2:D9, B:B, 2:2. |
| `by` | array of objects |  | Sort keys in order: [{"column": "B", "descending": true}] (default: the range's first column, ascending). |
| `header` | boolean |  | The first row is a header and stays put (default: guessed). |

### `sheet.filter`

Filter a range: rows whose cells don't pass are hidden (not deleted). Each call sets one column's rule; clear removes the filter. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `range` | string |  | The table to filter, header row included (default: the filter's, else the used range). |
| `column` | string |  | Column letter the rule is on. |
| `values` | array of strings |  | Keep rows showing one of these. |
| `condition` | string |  | Or a condition like >100, <>done, *draft*. |
| `clear` | boolean |  | Remove the filter (or only column's rule). |

### `sheet.fill`

Fill a range from its first cells, like dragging the fill handle: series continue (1, 2 → 3, 4), formulas move their references, other values repeat. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `range` | string | required | Cells in A1 notation: B2, B2:D9, B:B, 2:2. |
| `direction` | string |  | down (default) or right. |

### `sheet.copy`

Copy (or move) cells to another place, on this sheet or another; formulas move their relative references. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `from` | string | required | Source range. |
| `to` | string | required | Top-left destination cell. |
| `toPage` | string |  | Destination sheet (default: the same). |
| `move` | boolean |  | Cut instead of copy. |
| `formats` | boolean |  | Copy formats too (default true). |

### `sheet.resize`

Set column widths and row heights in pixels, or fit columns to their content. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `columns` | object |  | {"A": 160, "B": 90}. |
| `rows` | object |  | {"1": 32}. |
| `fit` | string |  | Columns to fit to their content, e.g. A:D. |

### `sheet.freeze`

Keep the first rows and columns in view while scrolling (0 and 0 unfreezes). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `rows` | integer |  | Rows to freeze. |
| `columns` | integer |  | Columns to freeze. |

### `sheet.setGridlines`

Show or hide the grid's lines. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `on` | boolean | required | Show them. |

### `sheet.evaluate`

Compute a formula on a sheet without writing it anywhere (as if typed in cell, default A1). _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `formula` | string | required | The formula, with or without =. |
| `cell` | string |  | Where it is computed from. |

### `sheet.find`

Cells whose shown value or formula contains text. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `text` | string | required | Text to find (not case-sensitive). |
| `inFormulas` | boolean |  | Search formulas too (default true). |

### `sheet.functions`

The spreadsheet functions folio knows (built in and from plugins): name, syntax, what it does, category. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `search` | string |  | Only those whose name or description contains this. |
| `category` | string |  | Math, Statistical, Logical, Lookup, Text, Date, Financial, Information or Custom. |

### `sheet.numberFormats`

Number format presets (codes for sheet.format number), each with an example. _(read only)_

### `sheet.addChart`

Add a chart over the sheet, drawing a range live. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `range` | string | required | Cells in A1 notation: B2, B2:D9, B:B, 2:2. |
| `kind` | string |  | column (default), bar, line, area, pie or scatter. |
| `title` | string |  | Title. |
| `seriesInRows` | boolean |  | Each row is a series (default: each column). |
| `x` | number |  | Left in pixels from the grid's corner (default: right of the data). |
| `y` | number |  | Top in pixels. |
| `w` | number |  | Width (default 480). |
| `h` | number |  | Height (default 300). |

### `sheet.updateChart`

Change a chart on a sheet. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `chart` | string | required | Chart id (sheet.read lists them). |
| `range` | string |  | New source range. |
| `kind` | string |  | New kind. |
| `title` | string |  | New title. |
| `seriesInRows` | boolean |  | Series in rows. |
| `legend` | boolean |  | Show the legend. |
| `stacked` | boolean |  | Stack bars and areas. |
| `x` | number |  | Left. |
| `y` | number |  | Top. |
| `w` | number |  | Width. |
| `h` | number |  | Height. |

### `sheet.removeChart`

Remove a chart from a sheet. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `chart` | string | required | Chart id. |

## deck

### `deck.read`

A deck: size, theme, and every slide with its layout, notes and shapes (id, name, kind, position, size, text). _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |

### `deck.addSlide`

Add a slide with a layout's placeholders, filled with title and body. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `layout` | string |  | title, titleContent (default), section, twoContent, titleOnly or blank. |
| `title` | string |  | Title text. |
| `body` | string |  | Body text: one line per bullet; for twoContent, a blank line between the columns. |
| `notes` | string |  | Speaker notes. |
| `at` | integer |  | 0-based position (default: after the slide shown, else at the end). |

### `deck.removeSlide`

Remove a slide. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |

### `deck.moveSlide`

Move a slide. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `to` | integer | required | New 0-based position. |

### `deck.duplicateSlide`

Copy a slide, right after it. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |

### `deck.setSlide`

Change a slide: its title or body placeholder's text, layout, background, speaker notes, hidden. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `title` | string |  | Title text. |
| `body` | string |  | Body text (one line per bullet). |
| `layout` | string |  | Apply a layout (adds missing placeholders). |
| `background` | string |  | #rrggbb, or "" for the theme's. |
| `notes` | string |  | Speaker notes. |
| `hidden` | boolean |  | Skip when presenting and exporting. |

### `deck.addShape`

Add a text box or a shape (rect, ellipse, triangle, line, arrow) to a slide. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `kind` | string | required | text, rect, ellipse, triangle, line or arrow. |
| `x` | number |  | Left edge in points from the slide's left (slides are 960×540 by default). |
| `y` | number |  | Top edge in points. |
| `w` | number |  | Width in points. |
| `h` | number |  | Height in points. |
| `text` | string |  | Text inside (one paragraph per line). |
| `fill` | string |  | Fill #rrggbb. |
| `line` | string |  | Outline colour #rrggbb. |
| `lineWidth` | number |  | Outline width in points. |
| `textSize` | number |  | Text size in points (default 20). |
| `color` | string |  | Text colour #rrggbb (default: the theme's). |
| `align` | string |  | left, center, right or justify. |
| `name` | string |  | A name to find it by. |

### `deck.updateShape`

Move, resize or restyle a shape, or replace its text. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `shape` | string | required | Shape id or name (deck.read lists them). |
| `x` | number |  | Left edge in points from the slide's left (slides are 960×540 by default). |
| `y` | number |  | Top edge in points. |
| `w` | number |  | Width in points. |
| `h` | number |  | Height in points. |
| `rotation` | number |  | Degrees clockwise. |
| `text` | string |  | Replace its text (one paragraph per line). |
| `fill` | string |  | Fill #rrggbb; "" for none. |
| `line` | string |  | Outline #rrggbb; "" for none. |
| `lineWidth` | number |  | Outline width in points. |
| `textSize` | number |  | Text size in points. |
| `color` | string |  | Text colour; "" for the theme's. |
| `valign` | string |  | top, middle or bottom. |
| `name` | string |  | New name. |
| `coalesce` | string |  | Consecutive edits from the same source with the same key within ~1 s fold into one undo step. Prefix a unique per-gesture key with gesture: to keep that gesture together across pauses. A different edit, undo or redo ends the group. |

### `deck.removeShape`

Remove a shape. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `shape` | string | required | Shape id or name (deck.read lists them). |

### `deck.duplicateShape`

Copy a shape (offset a little). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `shape` | string | required | Shape id or name (deck.read lists them). |

### `deck.arrange`

Bring a shape forward or send it back. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `shape` | string | required | Shape id or name (deck.read lists them). |
| `order` | string | required | front, back, forward or backward. |

### `deck.formatText`

Format all the text in a shape: character formatting and paragraph settings. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `shape` | string | required | Shape id or name (deck.read lists them). |
| `bold` | boolean |  | Bold on or off. |
| `italic` | boolean |  | Italic on or off. |
| `underline` | boolean |  | Underline on or off. |
| `strike` | boolean |  | Strikethrough on or off. |
| `color` | string |  | Text colour #rrggbb; "" clears. |
| `highlight` | string |  | Highlight colour #rrggbb; "" clears. |
| `link` | string |  | Link URL; "" removes the link. |
| `font` | string |  | sans, serif, mono or display; "" goes back to the style's. |
| `style` | string |  | Paragraph style: normal, title, subtitle, heading1, heading2, heading3, quote, code or caption. |
| `align` | string |  | left, center, right or justify. |
| `list` | string |  | bullet, number, check, or none. |

### `deck.addImage`

Put a picture on a slide. _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `path` | string |  | Image file. |
| `media` | string |  | A media id instead. |
| `x` | number |  | Left edge in points from the slide's left (slides are 960×540 by default). |
| `y` | number |  | Top edge in points. |
| `w` | number |  | Width in points. |
| `h` | number |  | Height in points. |

### `deck.addChart`

Put a chart on a slide that draws a sheet range live. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `source` | string | required | Sheet range, e.g. 'Sales'!A1:C13. |
| `kind` | string |  | column (default), bar, line, area, pie or scatter. |
| `title` | string |  | Title. |
| `x` | number |  | Left edge in points from the slide's left (slides are 960×540 by default). |
| `y` | number |  | Top edge in points. |
| `w` | number |  | Width in points. |
| `h` | number |  | Height in points. |

### `deck.addTable`

Put a table on a slide: from data, or live-linked to a sheet range. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `data` | array of arrays |  | Rows of cell text. |
| `rows` | integer |  | Rows (default 3). |
| `cols` | integer |  | Columns (default 3). |
| `link` | string |  | Sheet range shown live. |
| `x` | number |  | Left edge in points from the slide's left (slides are 960×540 by default). |
| `y` | number |  | Top edge in points. |
| `w` | number |  | Width in points. |
| `h` | number |  | Height in points. |

### `deck.themes`

The built-in deck themes with their colours and fonts. _(read only)_

### `deck.setTheme`

Apply a theme to a deck, or change its colours and fonts. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `theme` | string |  | paper, ink, grain, serif or mono. |
| `background` | string |  | #rrggbb. |
| `text` | string |  | #rrggbb. |
| `accent` | string |  | #rrggbb. |
| `headingFont` | string |  | display, sans, serif or mono. |
| `bodyFont` | string |  | display, sans, serif or mono. |

### `deck.present`

Present a deck full screen from a slide (Escape ends it). _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `presenter` | boolean |  | Show the presenter view (notes, next slide, timer) instead. |

## link

### `link.list`

Every live link in the file: which table or chart on which page reads which sheet range, and whether it still resolves. _(read only)_

## media

### `media.list`

Pictures in the file: id, name, type, size in pixels and bytes, and where each is used. _(read only)_

### `media.add`

Add a picture file to the file's media (to place later by media id). _(changes things · permission: files)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string | required | Image file. |

## history

### `history.list`

The undo history: steps to undo (newest first) and to redo, each with what made it (window, agent, cli, mcp). _(read only)_

### `history.undo`

Undo the last change, whoever made it. _(changes things)_

### `history.redo`

Redo the last undone change. _(changes things)_

### `history.checkpoint`

The history position now, to come back to with history.revertTo. _(read only)_

### `history.revertTo`

Undo every change made after a checkpoint (an agent's whole run). _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `checkpoint` | integer | required | From history.checkpoint. |

## handoff

### `handoff.apps`

The other lsuite apps on this computer (from ~/.lsuite/apps): name, kind, version, whether running, and what folio can take from each (a picture from nori, a frame from kimchi). _(read only)_

### `handoff.image`

Bring a picture from another lsuite app into the open file through its CLI: nori exports its open image (or a file), kimchi renders a frame of its open project (or a project file) at a time. It lands in a document page or on a slide. _(changes things · permission: app control)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `app` | string | required | nori or kimchi. |
| `file` | string |  | A document of that app to take it from (default: what the app has open). |
| `time` | number |  | kimchi: the time in seconds (default: its playhead). |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `after` | any |  | Put it after this block (index from doc.read, or id); -1 puts it first. Default: after the caret's block in the window, else at the end. |

## account

### `account.status`

lsuite AI: whether this computer is signed in (shared by every lsuite app), the account's email, plan, allowance used and when it resets, and the models in the plan. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `refresh` | boolean |  | Ask the server again now. |

### `account.signIn`

Sign in to lsuite AI: opens the browser to connect folio (the account is shared by every lsuite app), or takes a key from the account page (lsk_…). _(changes things · person only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `key` | string |  | A key from lsuite.xyz/account, for headless sign-in. |
| `wait` | boolean |  | Wait for the browser sign-in to finish (default false). |

### `account.signOut`

Sign out of lsuite AI on this computer (every lsuite app). _(changes things · person only)_

### `account.plans`

lsuite AI plans with prices, models and monthly allowances (from the server). _(read only)_

## plugin

### `plugin.list`

Plugins: stock (shipped with folio), installed (lsuite plugins built in Rust), with id, name, kind (functions or filter), version, path and whether enabled. _(read only)_

### `plugin.info`

One plugin: what it adds (functions with their syntax, file formats), where it came from, its manifest. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `id` | string | required | Plugin id. |

### `plugin.enable`

Switch a plugin on. _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `id` | string | required | Plugin id. |

### `plugin.disable`

Switch a plugin off (it stays installed). _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `id` | string | required | Plugin id. |

### `plugin.rescan`

Look in the plugin folder again and reload plugins that changed. _(changes things · permission: plugins)_

### `plugin.install`

Install a built plugin bundle (a folder with plugin.toml and the library) and load it. _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string | required | The bundle folder. |

### `plugin.remove`

Remove an installed lsuite plugin (stock plugins can only be disabled). _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `id` | string | required | Plugin id. |

### `plugin.guide`

How to write a folio plugin in Rust: the SDK, the kinds, the manifest, an example and the steps (Markdown, for agents). _(read only)_

### `plugin.toolchain`

Whether Rust (cargo, rustc) is installed, its version, and how to install it. _(read only)_

### `plugin.new`

Start a plugin crate from the SDK template in ~/.lsuite/plugins-src/folio/<name>/. _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `name` | string | required | Crate name (lowercase, dashes). |
| `kind` | string |  | functions (default) or filter. |

### `plugin.writeSource`

Write one file inside a plugin crate (paths outside it are refused). _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `name` | string | required | Crate name. |
| `path` | string | required | Path inside the crate, e.g. src/lib.rs. |
| `contents` | string | required | The file's contents. |

### `plugin.build`

Build a plugin crate (cargo build --release); returns ok and the compiler's errors as {file, line, message}. _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `name` | string | required | Crate name. |

### `plugin.publishLocal`

Bundle a built plugin crate and install it: its functions work at once. _(changes things · permission: plugins)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `name` | string | required | Crate name. |

## agent

### `agent.providers`

What can run the built-in agent: lsuite AI (no setup), coding CLIs on this computer (Claude Code, Codex), model APIs (Anthropic, OpenAI, OpenRouter, Google Gemini, Mistral) and local servers (Ollama, LM Studio); whether each is ready and what to do next. _(read only · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `refresh` | boolean |  | Check again now. |

### `agent.models`

The models a provider offers for the agent (the chosen one by default). _(read only · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `provider` | string |  | A provider id from agent.providers. |
| `refresh` | boolean |  | Fetch the list again now. |

### `agent.setProvider`

Choose what runs the built-in agent (Settings › Agent). _(changes things · person only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `provider` | string | required | A provider id from agent.providers. |
| `model` | string |  | Model id; empty for the provider's default. |
| `baseUrl` | string |  | A local or compatible server's address; empty for the default. |

### `agent.send`

Ask the built-in agent (the Agent panel) to do something, in words. It runs commands like any client (permissions apply) and shows them as cards. Returns the run at once, or once it ends with wait. _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `prompt` | string | required | The request. |
| `wait` | boolean |  | Wait until the run ends (default false). |
| `timeout` | number |  | With wait: stop waiting after this many seconds (default 900). |

### `agent.status`

One agent run (the running or latest by default): request, whether it is working, the reply, every command it ran, changes, time and tokens. _(read only · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `run` | integer |  | Run id from agent.runs. |
| `wait` | boolean |  | Wait until the run ends. |
| `timeout` | number |  | With wait: seconds (default 900). |

### `agent.runs`

The agent's runs on the open file, oldest first. _(read only · needs the window)_

### `agent.conversation`

The Agent panel's conversation as it shows it. _(read only · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `since` | integer |  | Only entries from this index on. |

### `agent.stop`

Stop the agent's run. Edits it finished stay (agent.revert removes them). _(changes things · needs the window)_

### `agent.revert`

Revert an agent run as one undo step. _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `run` | integer |  | Run id (default: the latest that changed something). |

### `agent.newConversation`

Start a new conversation in the Agent panel. _(changes things · needs the window)_

### `agent.steer`

Redirect the running agent with a follow-up message. _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `prompt` | string | required | Additional instructions. |

## app

### `app.info`

Version, folders, whether the window and the bridge run, and the open file. _(read only)_

### `app.commands`

Describe every command with its parameters, or one command. _(read only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `command` | string |  | One command name. |

### `app.settings`

Every setting with its value. _(read only)_

### `app.setSetting`

Change one setting by dotted key, e.g. appearance.mode or editing.author. Agent permissions stay with the person. _(changes things · permission: settings)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `key` | string | required | Dotted key from app.settings. |
| `value` | any | required | New value (same type). |

### `app.setAgentKey`

Save (or with no key, remove) an API key the built-in agent uses, in the OS keychain. _(changes things · person only)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `provider` | string | required | anthropic, openai, openrouter, gemini or mistral. |
| `key` | string |  | The key; empty removes it. |

### `app.onboarding`

The first-run setup: whether it was done, the suites a person may come from (with the formats folio opens from each), the agent providers found on this computer and lsuite AI. _(read only)_

### `app.finishOnboarding`

Finish (or skip) the first-run setup with the choices made. _(changes things)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `comingFrom` | string |  | office, google, apple, libreoffice or none. |
| `agent` | boolean |  | Offer the Agent panel. |
| `provider` | string |  | The agent provider to use. |
| `author` | string |  | The name on comments and tracked changes. |

### `app.checkUpdates`

Look on GitHub Releases for a newer folio and say where to get it. _(read only)_

### `app.quit`

Quit folio (everything is saved). _(changes things · permission: app control · needs the window)_

## ui

### `ui.state`

What the window shows: home or editor, the page, the caret or selected cells or slide, zoom, theme, open panels. _(read only)_

### `ui.show`

Show a page in the window (and a slide, a cell or a block). _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `page` | string |  | Page id, name or 1-based number (page.list). Defaults to the page the window shows, else the first page of the right kind. |
| `slide` | string |  | Slide: its id, or its 1-based number. Defaults to the slide the window shows, else the first. |
| `cell` | string |  | A cell to select. |
| `block` | integer |  | A block to scroll to. |

### `ui.select`

Select in the window: text from..to on a document page, a range of cells, or shapes on a slide. _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `from` | object |  | Text start {block, offset}. |
| `to` | object |  | Text end. |
| `range` | string |  | Cells, A1. |
| `shapes` | array of strings |  | Shape ids. |

### `ui.panel`

Open or close a panel or dialog: agent, inspector, pages, settings, export, open, plugins, account, palette, shortcuts, onboarding; or home. _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `name` | string | required | Panel or dialog name. |
| `open` | boolean |  | Default true. |

### `ui.zoom`

Zoom the work area. _(changes things · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `level` | number | required | 1 = 100 % (0.25 to 4). |

### `ui.theme`

Dark, light or follow the system. _(changes things · permission: settings)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `mode` | string | required | dark, light or system. |

### `ui.screenshot`

Save a PNG of the window and return its path (macOS). _(changes things · permission: files · needs the window)_

| Parameter | Type | | Description |
| --- | --- | --- | --- |
| `path` | string |  | Destination .png (default: a temporary file). |

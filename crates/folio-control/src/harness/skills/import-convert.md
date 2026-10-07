# Import or convert Office files
When: bring in a Word, Excel, PowerPoint, OpenDocument, CSV or Markdown file, or write the file out to another format. Needs the files permission.

## Steps
1. `file.formats` lists what folio reads and writes and what doesn't survive each trip. Tell the person about losses that matter for their file (macros, pivot tables, animations, tracked-change details…).
2. To work on a file: `file.open path=…` opens it as a new untitled folio file (the original is never touched). To add its pages to the open file instead: `file.import path=… at=N`.
3. Read what came in: `file.overview`, then `harness.check` (formula errors after an Excel import, broken links, overflowing slide text after a PowerPoint import) and `harness.look` on a page, a slide and a sheet range.
4. Repair what the import broke (fonts don't matter; structure and numbers do): heading styles lost as bold Normal text (`doc.setParagraph style=heading1`), numbers stored as text, formulas showing errors.
5. To write it out: `file.export path=out.docx` (or .xlsx, .pptx, .pdf, .odt, .ods, .odp, .csv, .md, .html; `pages=[…]` for some pages only). Documents go to word processors, sheets to spreadsheets, decks to presentations; PDF and HTML take every kind. Report what the answer says the format couldn't carry.

## Checks
- After an import: the page count and kinds match the source; `harness.check` shows no new errors.
- After an export: the answer names the file written and lists losses; say them in your reply.

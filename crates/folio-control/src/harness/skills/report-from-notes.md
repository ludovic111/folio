# Report from notes
When: the person gives notes, bullet points, a transcript or a rough draft and wants a finished, structured report.

## Steps
1. Read the notes (the request, a document page with `doc.read markdown=true`, or an imported file). List the facts, figures and decisions they hold; note what is missing.
2. Plan the outline: Title; a short summary (three to five sentences: purpose, main finding, what is asked of the reader); Heading 1 sections in a logical order (context, findings, options or analysis, recommendation, next steps); Heading 2 only inside long sections.
3. Use the document page shown when it is empty, else `page.add kind=doc name="Report"`. Write it in one call: `doc.write replace=true markdown="# Title\n\n## Summary\n\n…"`. Lists for parallel points, a Markdown table for anything compared on two dimensions.
4. Numbers that come from a sheet in the file: a live table (`doc.insertTable link='Sheet'!A1:D8 after=<block>`) or chart (`doc.insertChart source=… kind=column title="The takeaway"`), not typed copies.
5. Sources named in the notes become footnotes: `doc.footnote find="the sentence's end" text="Author, Title, year"`. Invent none.
6. `doc.setup footer="{page} / {pages}"` when it runs past one page.

## Checks
- `doc.outline`: one Title, Heading 1 sections, no skipped level, no heading longer than a line.
- Every fact in the notes is in the report; nothing in the report contradicts them; assumptions are said.
- `harness.check page=…` is clean; `harness.look page=… pageNumber=1` shows a clear first page.

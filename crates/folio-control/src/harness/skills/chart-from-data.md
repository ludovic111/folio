# Chart from data
When: the person wants a chart of numbers in a sheet (on the sheet, in a document or on a slide).

## Steps
1. Find the data: `sheet.read range=…`. The source is a header row plus the rows, with the category (or date) column first; no totals row, no blank row.
2. Decide the one message the chart shows, and the kind: column for a few categories, bar for long labels or many categories, line for a series over time, area for a cumulative total, pie for five or fewer parts of a whole, scatter for two measures. Series in columns by default; `seriesInRows=true` when each row is a series.
3. Place it where it is read:
   - on the sheet: `sheet.addChart range=A1:C13 kind=line title="Sales grew every month"`;
   - in a document: `doc.insertChart source='Sales'!A1:C13 kind=line title=… after=<block>`;
   - on a slide: `deck.addSlide layout=titleOnly title="Sales grew every month"` then `deck.addChart source='Sales'!A1:C13 kind=line x=72 y=140 w=816 h=360`.
4. The title states the takeaway. Format the source numbers first (`sheet.format number=…`): the axis follows them.

## Checks
- `harness.look` the chart where it sits (the slide, the document page, or the sheet range that includes it): every series and label readable, no totals bar dwarfing the rest.
- `link.list`: the chart's link resolves (`ok: true`).

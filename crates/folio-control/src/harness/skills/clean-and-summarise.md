# Clean data and summarise it
When: messy pasted or imported data (CSV, an export) to tidy, then a summary by category (what a pivot table would give).

## Steps
1. `sheet.read` the data (or a range of it). Note: header row, columns, types, problems (blank rows, totals rows inside, text numbers like "1 200", mixed date formats, stray spaces, duplicates, inconsistent category spelling).
2. Keep the raw data: `page.duplicate` it and rename the copy (`page.rename name="Clean"`), or clean in place only if the person asked.
3. Fix with commands, not by hand where a formula will do: `sheet.setRange` rewritten columns (trimmed text, one spelling per category, numbers as numbers, dates as yyyy-mm-dd), `sheet.deleteRows` blank or subtotal rows (bottom-up so numbers don't shift), `sheet.sort range=A1:F200 by=[{"column":"A"}] header=true`.
4. Summary (folio has no pivot tables or UNIQUE): on a page "Summary", write the distinct categories once in column A (header "Category"), then beside them `=SUMIFS(Clean!$D:$D,Clean!$B:$B,A2)`, `=COUNTIFS(Clean!$B:$B,A2)`, `=AVERAGEIFS(…)`; a total row with `=SUM(B2:B9)`. Quote sheet names with spaces: `'Raw data'!B:B`.
5. Format (bold header, `#,##0.00`, `0.0%` shares), fit columns, and add one chart of the summary if it helps.

## Checks
- The summary's grand total equals `sheet.evaluate formula="=SUM(Clean!D2:D500)"`; the counts add up to the number of data rows.
- `harness.check` shows no errors, and no blank cells inside the clean table.
- Every category in the data appears once in the summary (`sheet.find` a few odd spellings).

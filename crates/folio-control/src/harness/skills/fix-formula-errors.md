# Fix formula errors
When: cells show #REF!, #NAME?, #DIV/0!, #VALUE!, #N/A, #NUM! or a circular reference, or totals look wrong.

## Steps
1. `harness.check` (or `file.overview`) lists every error cell with its formula. `sheet.read range=… inputs=true` shows the formulas around it.
2. Find the first cause: errors spread to every cell that depends on them, so fix the cell whose own inputs are fine first.
3. By kind:
   - `#NAME?`: a misspelt function (`sheet.functions search=…`), text without quotes, or a sheet name with spaces not quoted (`'Raw data'!A1`).
   - `#REF!`: the formula pointed at deleted rows, columns or a removed sheet; rewrite the reference.
   - `#DIV/0!`: an empty or zero divisor; guard it, `=IF(C2=0,"",B2/C2)`, or fix the missing input.
   - `#VALUE!`: text where a number is expected ("1 200", "12 €"); turn the source into numbers (`sheet.setRange`), or use VALUE().
   - `#N/A`: a lookup found nothing; check the key's spelling and type (text "12" vs number 12); `IFNA(…,"not found")` only when a miss is legitimate.
   - Circular: a total including itself (`=SUM(B2:B10)` in B10); end the range above it.
4. Try the corrected formula with `sheet.evaluate formula=… cell=…` before writing it with `sheet.set` (or `sheet.setRange` / `sheet.fill` for a column).
5. Also look for silent mistakes: a SUM that stops before the last row, a range that includes the total, a hard-coded number inside a formula.

## Checks
- `harness.check page=…` reports no errors; the answer of your last sheet command lists no error cells.
- A total recomputed with `sheet.evaluate` matches the cell.

# Budget or model sheet
When: a budget, forecast, price calculation, loan or any sheet where outputs are computed from assumptions.

## Steps
1. List inputs (rates, prices, quantities, start date), calculations and outputs. Inputs are things a person would change; everything else is a formula.
2. `page.add kind=sheet name="Budget"` (a short name formulas can use). Inputs block first:
   `sheet.setRange at=A1 values=[["Assumptions","Value","Unit"],["VAT rate",0.2,"%"],["Months",12,""]]`.
3. The table below or beside it, header row first, one line per item, formulas referring to inputs absolutely:
   `[["Item","Monthly","Annual"],["Rent",1200,"=B6*$B$3"],…,["Total","=SUM(B6:B12)","=SUM(C6:C12)"]]`. Totals sit under the data and never include themselves.
4. Formats in one `file.batch`: `sheet.format range=A1:C1 bold=true border=b`, amounts `number="#,##0.00"`, rates `number="0.0%"`, the totals row bold with `border=t`, `sheet.freeze rows=1` when the table is long, `sheet.resize fit=A:D`.
5. A summary (outputs) block: the few numbers that answer the question, each a formula on the table.
6. A chart only if it says one thing: `sheet.addChart range=A5:B11 kind=column title="Rent is half the budget"` (no totals row in the range).

## Checks
- The answer of each `sheet.setRange` lists no error cells; `harness.check page=…` shows no errors and no blank cells inside the table.
- Recompute one total another way: `sheet.evaluate formula="=SUMPRODUCT(B6:B12,1)"` equals the total cell.
- Change one input in your head: every dependent cell is a formula (`sheet.read range=… inputs=true` shows no typed numbers outside the inputs).
- `harness.look page=… range=A1:D20` reads clearly.

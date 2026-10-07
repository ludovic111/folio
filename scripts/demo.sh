#!/usr/bin/env bash
# Builds a demo file with every kind of page (for screenshots and trying folio):
#   scripts/demo.sh [out.folio]
# Uses target/debug/folio-cli (cargo build -p folio-cli first).
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
cli="${FOLIO_CLI:-$here/target/debug/folio-cli}"
out="${1:-$HOME/.cache/folio-demo/Quarterly review.folio}"
mkdir -p "$(dirname "$out")"
rm -f "$out"
f() { "$cli" --file "$out" --compact "$@" > /dev/null; }
f file.new template=review title="Quarterly review"
f doc.write page=Report --markdown '## Notes from the team

The new plan brought in **38 customers** in September, against 21 in August. *Support load stayed flat*, thanks to the help pages written in July.

- [x] Ship the new pricing page
- [x] Hire a second designer
- [ ] Translate the help pages

> Revenue grew every month this quarter, and the margin is the best of the year.'
f doc.setup page=Report header="Quarterly review · Q3 2026" footer="Page {page} of {pages}"
f doc.comment page=Report find="38 customers" text="Can we break this down by plan?"
f sheet.setRange page=Numbers at=G1 --values '[["Plan","Customers","Share"],["Starter",21,"=H2/SUM($H$2:$H$4)"],["Team",12,"=H3/SUM($H$2:$H$4)"],["Studio",5,"=H4/SUM($H$2:$H$4)"]]'
f sheet.format page=Numbers range=G1:I1 --bold --fill "#e9e9e9" --border b
f sheet.format page=Numbers range=I2:I4 --number "0%"
f sheet.addChart page=Numbers range=A1:C4 kind=column title="Revenue and costs" x=0 y=150 w=520 h=280
f sheet.addChart page=Numbers range=G1:H4 kind=pie title="Customers by plan" x=540 y=150 w=360 h=280
f deck.setTheme page=Slides theme=ink
echo "$out"

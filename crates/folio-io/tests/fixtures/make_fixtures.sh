#!/bin/sh
# Converts the openpyxl / python-pptx fixtures with LibreOffice, so the tests also read files
# LibreOffice wrote: XLSX with saved results and shared formulas, ODS, ODP.
# SOFFICE=/path/to/soffice sh make_fixtures.sh
set -e
cd "$(dirname "$0")"
SOFFICE=${SOFFICE:-soffice}
tmp=$(mktemp -d)
"$SOFFICE" --headless --convert-to ods --outdir "$tmp" budget.xlsx >/dev/null
cp "$tmp/budget.ods" budget.ods
"$SOFFICE" --headless --convert-to xlsx --outdir "$tmp" budget.ods >/dev/null
cp "$tmp/budget.xlsx" budget-lo.xlsx
"$SOFFICE" --headless --convert-to odp --outdir "$tmp" deck.pptx >/dev/null
cp "$tmp/deck.odp" deck.odp
"$SOFFICE" --headless --convert-to pptx --outdir "$tmp" deck.odp >/dev/null
cp "$tmp/deck.pptx" deck-lo.pptx
rm -rf "$tmp"

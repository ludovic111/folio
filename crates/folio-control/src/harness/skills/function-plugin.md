# Write a spreadsheet function plugin
When: the person needs a spreadsheet function folio doesn't have (=MYFUNC(…)), written in Rust as a folio plugin. Needs the plugins permission.

## Steps
1. Check it doesn't exist: `sheet.functions search=…`. If a formula of existing functions does the job, offer that first.
2. `plugin.guide` (read it all: the SDK, the manifest, the example, the rules), then `plugin.toolchain`. If Rust is missing, tell the person how to install it (rustup) and stop.
3. `plugin.new name=my-functions kind=functions`; it answers with the crate's path and files.
4. Write `src/lib.rs` with `plugin.writeSource name=my-functions path=src/lib.rs contents=…`, following the guide's example: each function's name (UPPERCASE), its syntax and description, argument checks that return errors (#VALUE!) instead of panicking, empty cells and text handled.
5. `plugin.build name=my-functions`; fix from the structured errors `{file, line, message}` until it is green.
6. `plugin.publishLocal name=my-functions`: the functions work at once, no restart.
7. Try it: `sheet.evaluate formula="=MYFUNC(2,3)"` with normal inputs, edge cases (0, negative, empty, text) and compare with what you expect; then use it in the sheet if the person asked.

## Checks
- `plugin.list` shows it enabled; `sheet.functions search=MYFUNC` lists it with its syntax.
- Every `sheet.evaluate` result matches the expected value; bad inputs give a spreadsheet error, not a crash.

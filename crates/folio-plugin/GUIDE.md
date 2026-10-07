# Writing a folio plugin

A folio plugin is a small Rust library (`cdylib`) built on the `folio-plugin` SDK. It adds
**spreadsheet functions** (kind `functions`): `=GEOMEAN(A1:A9)` in any sheet of any file, listed
in the function picker under *Custom*. It can also add **import filters** (kind `filter`): a file
format folio reads into a document. folio loads it at runtime, with no restart, through a frozen
C ABI (ABI 1); a plugin that panics is switched off, never the app.

## The recipe (for an agent)

1. `plugin.guide` (this text) and `plugin.toolchain`. If Rust is missing, tell the person and
   offer to install it with rustup (https://rustup.rs). Never install it without their yes.
2. `plugin.new {name, kind}`: a crate in `~/.lsuite/plugins-src/folio/<name>/` with
   `Cargo.toml`, `plugin.toml` and `src/lib.rs` (a working example to edit).
3. Write the code: `plugin.writeSource {name, path, contents}` (paths inside the crate only), or
   your own file tools inside that folder.
4. `plugin.build {name}` until `ok` is true. Errors come back as `{file, line, column, message,
   rendered}`: fix them from there.
5. `plugin.publishLocal {name}`: builds, bundles the library with `plugin.toml` into
   `~/.lsuite/plugins/folio/<id>/` and loads it. Then try it: `sheet.evaluate {formula:
   "=MYFUNC(2, 3)"}`, or `sheet.set` a formula and `sheet.read` it. Show the person.

`LSUITE_HOME` replaces `~/.lsuite`. Building and installing plugins needs the agent's
**plugins** permission (Settings › Agent › Permissions), off until the person allows it.

## A function plugin

```rust
use folio_plugin::prelude::*;

/// Geometric mean of positive numbers.
fn geomean(args: &[Arg]) -> Result<f64, ErrorKind> {
    let xs = numbers(args)?; // numbers in ranges, typed values coerced, first error returned
    if xs.is_empty() || xs.iter().any(|x| *x <= 0.0) {
        return Err(ErrorKind::Num);
    }
    Ok((xs.iter().map(|x| x.ln()).sum::<f64>() / xs.len() as f64).exp())
}

/// Repeats text with a separator: JOINREPEAT("ab", 3, "-") is "ab-ab-ab".
fn join_repeat(args: &[Arg]) -> Result<String, ErrorKind> {
    let text = args[0].text()?;
    let times = args[1].number()?;
    let sep = match args.get(2) { Some(a) => a.text()?, None => String::new() };
    if !(0.0..=10_000.0).contains(&times) {
        return Err(ErrorKind::Value);
    }
    Ok(vec![text; times as usize].join(&sep))
}

export! {
    id: "com.example.stats",          // the same id as plugin.toml
    name: "Stats",
    version: env!("CARGO_PKG_VERSION"),
    functions: [
        // fn_def!(NAME, syntax, summary, min args, max args, function)
        fn_def!("GEOMEAN", "GEOMEAN(number1, [number2], …)", "Geometric mean of positive numbers.", 1, 255, geomean),
        fn_def!("JOINREPEAT", "JOINREPEAT(text, times, [separator])", "Repeats text with a separator.", 2, 3, join_repeat),
    ],
}
```

### The types

- `Arg` is one argument: `Arg::Value(Value)` (a single cell is passed as its value) or
  `Arg::Range { rows, cols, values }` (a range or an array, row by row).
  - `arg.value()`: the single value (a one-cell range's cell; a larger range is `#VALUE!`).
  - `arg.number()`, `arg.text()`, `arg.boolean()`: that value coerced, as spreadsheets do.
  - `arg.values()`: every value in it. `arg.numbers()`: its numbers, the way SUM reads them.
- `Value` is `Empty`, `Number(f64)`, `Text(String)`, `Bool(bool)` or `Error(ErrorKind)`.
  `Value::number()` (empty is 0, TRUE is 1, `"3"` is 3, other text `#VALUE!`), `Value::text()`
  (numbers to 15 significant digits, TRUE/FALSE), `Value::boolean()`.
- `ErrorKind` is `Div0` (`#DIV/0!`), `NA` (`#N/A`), `Name`, `Null`, `Num` (`#NUM!`), `Ref`,
  `Value` (`#VALUE!`) or `Circular`. `code()` gives the text; `code_number()` and
  `from_code_number()` its number in the ABI.
- `numbers(args)`: every number in the arguments, the way SUM reads them: in ranges only numbers
  count (text, TRUE/FALSE and blanks are skipped); a value typed as an argument is coerced; the
  first error is returned as `Err`, so `?` passes it on.
- A function is `fn(&[Arg]) -> T` where `T: IntoValue`: `Value`, `f64`, `i64`, `usize`, `bool`,
  `String`, `&str`, `ErrorKind`, `Option<T>` (`None` is an empty cell) or `Result<T, ErrorKind>`.
  A number that isn't finite becomes `#NUM!`.
- `Function` (made by `fn_def!`) and `Filter` (made by `filter_def!`) are what `export!` lists.

### The rules

- **Names** are uppercase letters, digits, `.` and `_` (`GEOMEAN`, `TEXT.SLUG`). A plugin can't
  replace a built-in function (`sheet.functions` lists them): that name is ignored. Between two
  plugins, the first one loaded keeps a name.
- **Argument counts**: folio checks `min`/`max` (at most 255) before calling; outside them the
  cell shows `#VALUE!`, so `args[0]` is safe when `min` is 1.
- **Pure and fast**: same arguments, same answer; no files, network or clock; no global
  mutable state (functions may run on any thread, many times a second). Return an
  `ErrorKind` rather than panicking. Results are single values: no arrays.
- **Panics** are caught (`#VALUE!` in the cell) and folio switches the plugin off (the person
  switches it back on in Plugins); keep `panic = "unwind"` in `Cargo.toml` so they can be
  caught.
- **Dependencies**: any crate that builds for the platform; prefer few and small.

## An import filter (optional)

A filter reads a file's bytes and returns a folio document as JSON (`document.json`'s shape).
folio fills in `format`, the document's `id`, and any page or block `id` you leave out.

```rust
fn import_txt(bytes: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "This file isn't UTF-8 text.".to_string())?;
    let blocks: Vec<String> = text.split("\n\n").map(|p| {
        format!(r#"{{"type":"paragraph","runs":[{{"text":{:?}}}]}}"#, p.trim())
    }).collect();
    Ok(format!(r#"{{"title":"Notes","pages":[{{"name":"Notes","kind":"doc","blocks":[{}]}}]}}"#, blocks.join(",")))
}

export! {
    id: "com.example.txt",
    name: "Plain text",
    version: env!("CARGO_PKG_VERSION"),
    functions: [],
    filters: [filter_def!("Plain text", "txt,text", "Paragraphs from a plain text file.", import_txt)],
}
```

(`{:?}` escapes like JSON for ordinary text; use a JSON crate for anything unusual.)

A document page is `{"name", "kind": "doc", "blocks": [...]}`; a paragraph block is
`{"type": "paragraph", "style": "normal" | "title" | "subtitle" | "heading1" | "heading2" |
"heading3" | "quote" | "code", "runs": [{"text": "…", "bold": true}]}`. A sheet page is
`{"name", "kind": "sheet", "cells": {"A1": {"input": "Item"}, "B2": {"input": "=SUM(B1:B1)"}}}`
(inputs as typed: numbers, text, TRUE/FALSE or formulas). Return `Err(message)` for a file you
can't read: the person sees the message. A filter adds a format to folio's import; it never
replaces a stock one (DOCX, XLSX, PPTX, ODF, CSV, Markdown, HTML, PDF).

## The crate

`plugin.new` writes these; keep them in step when you rename things.

`Cargo.toml`:

```toml
[package]
name = "my-stats"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
# The copy of the SDK folio carries (plugin.new points here; works offline, matches this folio):
folio-plugin = { path = "/Users/you/.lsuite/plugins-src/folio/.sdk/folio-plugin-0.1.0" }
# Without folio on the machine:
# folio-plugin = { git = "https://github.com/ludovic111/folio", tag = "v0.1.0" }

[profile.release]
panic = "unwind"   # so folio can catch a panic and switch the plugin off

[workspace]
```

`plugin.toml` (copied into the bundle next to the library):

```toml
id = "com.example.stats"        # reverse-DNS, the same as export!'s id
name = "Stats"
version = "0.1.0"
app = "folio"
kind = "functions"              # or "filter"
abi = 1
description = "Geometric mean and friends."
authors = ["Your Name"]

[library]
macos = "libmy_stats.dylib"
linux = "libmy_stats.so"
windows = "my_stats.dll"
```

A **bundle** is a folder with `plugin.toml` and the library. `plugin.install {path}` installs
one; `plugin.publishLocal` makes and installs one from the crate. Installed plugins live in
`~/.lsuite/plugins/folio/<id>/`; `plugin.rescan` reloads those whose library changed (hot
reload), `plugin.disable`/`plugin.enable` switch one off and on, `plugin.remove` deletes it.

## The ABI (for the curious; `export!` writes all of it)

The library exports `folio_plugin_entry(host_abi: u32) -> *const PluginVTable` (`ENTRY_SYMBOL`).
folio passes its ABI (`ABI_VERSION`, 1) and checks the table's `abi_version` (always its first
field) before reading anything else; `size` lets later ABIs append fields. The table holds the
id, name and version, the functions (`FfiFunction`: name, syntax, summary, min and max args) and
filters (`FfiFilter`: name, comma-separated extensions, summary), and four calls:

- `call(index, args, count, out) -> status`: arguments are `FfiValue`s, a tag (`TAG_EMPTY`,
  `TAG_NUMBER`, `TAG_TEXT`, `TAG_BOOL`, `TAG_ERROR`, `TAG_RANGE`) and a `repr(C)` union payload.
  The host owns the arguments (valid during the call only); the plugin writes the result into
  `*out` and owns its text until the host hands it back with `free_value(out)`, after every call.
- `import(index, bytes, len, out) -> status`: the document's JSON (or a message) in plugin-owned
  `FfiBytes`, released with `free_bytes`.
- Status codes: `OK`, `BAD_ARGUMENT`, `PANICKED` (the host switches the plugin off),
  `NO_SUCH_FUNCTION`, `FAILED` (an import that didn't work). Every call is wrapped in
  `catch_unwind`.

The plugin side of this lives in `folio_plugin::ffi` (`function_table`, `filter_table`, `call`,
`free_value`, `import`, `free_bytes`; `FfiStr::new`, `FfiStr::borrowed`, `to_string_lossy`,
`FfiValue::number`/`boolean`/`error`/`text`/`range`, `read_scalar`); `GUIDE` is this text.

//! folio plugin SDK.
//!
//! A folio plugin is a Rust `cdylib` that adds **spreadsheet functions** (and, optionally,
//! **import filters**) to folio. It is plain functions declared with [`export!`]; folio loads
//! the library at runtime through the frozen C ABI in [`ffi`], without a restart:
//!
//! ```ignore
//! use folio_plugin::prelude::*;
//!
//! fn geomean(args: &[Arg]) -> Result<f64, ErrorKind> {
//!     let xs = numbers(args)?;
//!     if xs.is_empty() || xs.iter().any(|x| *x <= 0.0) {
//!         return Err(ErrorKind::Num);
//!     }
//!     Ok((xs.iter().map(|x| x.ln()).sum::<f64>() / xs.len() as f64).exp())
//! }
//!
//! export! {
//!     id: "com.example.stats",
//!     name: "Stats",
//!     version: env!("CARGO_PKG_VERSION"),
//!     functions: [
//!         fn_def!("GEOMEAN", "GEOMEAN(number1, [number2], …)", "Geometric mean of positive numbers.", 1, 255, geomean),
//!     ],
//! }
//! ```
//!
//! A function receives its evaluated arguments ([`Arg`]: a [`Value`], or a range of them row
//! by row) and returns anything [`IntoValue`] (a [`Value`], a number, text, a bool, an
//! [`ErrorKind`], or a `Result` of those, so `?` works). folio checks the argument count
//! against the declared minimum and maximum before calling (`#VALUE!` otherwise), catches a
//! panic (`#VALUE!`, and the plugin is switched off), and never lets a plugin replace a
//! built-in function. Functions must be pure and thread-safe: folio may call them from any
//! thread, as often as it recalculates.
//!
//! `GUIDE.md` (returned by `plugin.guide`, kept in step with this crate by a test) is the
//! whole story for an agent: the manifest, the template, the build and the install.

#![deny(unsafe_op_in_unsafe_fn)]

pub mod ffi;

/// The ABI this SDK speaks (checked by the host before anything is called).
pub const ABI_VERSION: u32 = 1;
/// The symbol folio looks up in a plugin library.
pub const ENTRY_SYMBOL: &str = "folio_plugin_entry";
/// How to write a plugin, for agents and people (`plugin.guide`).
pub const GUIDE: &str = include_str!("../GUIDE.md");

/// Everything a plugin needs: `use folio_plugin::prelude::*;`.
pub mod prelude {
    pub use crate::{Arg, ErrorKind, Filter, Function, IntoValue, Value, export, filter_def, fn_def, numbers};
}

/// The errors a formula can produce, as spreadsheets show them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// `#DIV/0!`
    Div0,
    /// `#N/A`: nothing found.
    NA,
    /// `#NAME?`
    Name,
    /// `#NULL!`
    Null,
    /// `#NUM!`: a number out of range, a calculation that doesn't converge.
    Num,
    /// `#REF!`
    Ref,
    /// `#VALUE!`: a value of the wrong type.
    Value,
    /// `#CIRC!`
    Circular,
}

impl ErrorKind {
    /// The code shown in the cell, such as `#DIV/0!`.
    pub const fn code(self) -> &'static str {
        match self {
            ErrorKind::Div0 => "#DIV/0!",
            ErrorKind::NA => "#N/A",
            ErrorKind::Name => "#NAME?",
            ErrorKind::Null => "#NULL!",
            ErrorKind::Num => "#NUM!",
            ErrorKind::Ref => "#REF!",
            ErrorKind::Value => "#VALUE!",
            ErrorKind::Circular => "#CIRC!",
        }
    }

    /// Its number in the ABI (1 `#DIV/0!` … 8 `#CIRC!`).
    pub const fn code_number(self) -> u32 {
        match self {
            ErrorKind::Div0 => 1,
            ErrorKind::NA => 2,
            ErrorKind::Name => 3,
            ErrorKind::Null => 4,
            ErrorKind::Num => 5,
            ErrorKind::Ref => 6,
            ErrorKind::Value => 7,
            ErrorKind::Circular => 8,
        }
    }

    /// The error with that ABI number (`#VALUE!` for an unknown one).
    pub const fn from_code_number(n: u32) -> Self {
        match n {
            1 => ErrorKind::Div0,
            2 => ErrorKind::NA,
            3 => ErrorKind::Name,
            4 => ErrorKind::Null,
            5 => ErrorKind::Num,
            6 => ErrorKind::Ref,
            8 => ErrorKind::Circular,
            _ => ErrorKind::Value,
        }
    }
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

/// A cell's value: what arguments hold and what a function returns.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Value {
    /// An empty cell.
    #[default]
    Empty,
    Number(f64),
    Text(String),
    Bool(bool),
    Error(ErrorKind),
}

impl Value {
    /// As a number, the way spreadsheets coerce a single value: empty is 0, TRUE is 1, text
    /// that reads as a number is that number, other text is `#VALUE!`, an error is itself.
    pub fn number(&self) -> Result<f64, ErrorKind> {
        match self {
            Value::Empty => Ok(0.0),
            Value::Number(n) => Ok(*n),
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Value::Text(t) => t.trim().parse::<f64>().ok().filter(|n| n.is_finite()).ok_or(ErrorKind::Value),
            Value::Error(e) => Err(*e),
        }
    }

    /// As text: numbers as the General format shows them (up to 15 significant digits),
    /// `TRUE`/`FALSE`, empty is `""`, an error is itself.
    pub fn text(&self) -> Result<String, ErrorKind> {
        match self {
            Value::Empty => Ok(String::new()),
            Value::Number(n) => Ok(general(*n)),
            Value::Bool(b) => Ok(if *b { "TRUE" } else { "FALSE" }.into()),
            Value::Text(t) => Ok(t.clone()),
            Value::Error(e) => Err(*e),
        }
    }

    /// As TRUE or FALSE: numbers are TRUE unless 0, text `TRUE`/`FALSE` (any case), empty is
    /// FALSE, other text is `#VALUE!`.
    pub fn boolean(&self) -> Result<bool, ErrorKind> {
        match self {
            Value::Empty => Ok(false),
            Value::Number(n) => Ok(*n != 0.0),
            Value::Bool(b) => Ok(*b),
            Value::Text(t) if t.trim().eq_ignore_ascii_case("true") => Ok(true),
            Value::Text(t) if t.trim().eq_ignore_ascii_case("false") => Ok(false),
            Value::Text(_) => Err(ErrorKind::Value),
            Value::Error(e) => Err(*e),
        }
    }
}

fn general(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    // 15 significant digits, as spreadsheets show them, then the shortest form of that.
    let rounded: f64 = format!("{n:.14e}").parse().unwrap_or(n);
    format!("{rounded}")
}

/// An argument: one value (a single cell is passed as its value), or a range or array of
/// `rows × cols` values, row by row.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Value(Value),
    Range { rows: u32, cols: u32, values: Vec<Value> },
}

impl Arg {
    /// Every value in it: one for a single value, all of a range's row by row.
    pub fn values(&self) -> &[Value] {
        match self {
            Arg::Value(v) => std::slice::from_ref(v),
            Arg::Range { values, .. } => values,
        }
    }

    /// The single value: itself, or a one-cell range's cell; a larger range is `#VALUE!`.
    pub fn value(&self) -> Result<&Value, ErrorKind> {
        match self {
            Arg::Value(v) => Ok(v),
            Arg::Range { values, .. } if values.len() == 1 => Ok(&values[0]),
            Arg::Range { .. } => Err(ErrorKind::Value),
        }
    }

    /// The single value as a number ([`Value::number`]).
    pub fn number(&self) -> Result<f64, ErrorKind> {
        self.value()?.number()
    }

    /// The single value as text ([`Value::text`]).
    pub fn text(&self) -> Result<String, ErrorKind> {
        self.value()?.text()
    }

    /// The single value as TRUE or FALSE ([`Value::boolean`]).
    pub fn boolean(&self) -> Result<bool, ErrorKind> {
        self.value()?.boolean()
    }

    /// The numbers in it, the way SUM reads them ([`numbers`]).
    pub fn numbers(&self) -> Result<Vec<f64>, ErrorKind> {
        numbers(std::slice::from_ref(self))
    }
}

/// The numbers of the arguments, the way SUM reads them: in ranges only numbers count (text,
/// TRUE/FALSE and empty cells are skipped); a value typed as an argument is coerced
/// (`"3"` is 3, TRUE is 1, other text is `#VALUE!`); the first error found is returned.
pub fn numbers(args: &[Arg]) -> Result<Vec<f64>, ErrorKind> {
    let mut out = Vec::new();
    for a in args {
        match a {
            Arg::Value(Value::Empty) => {}
            Arg::Value(v) => out.push(v.number()?),
            Arg::Range { values, .. } => {
                for v in values {
                    match v {
                        Value::Number(n) => out.push(*n),
                        Value::Error(e) => return Err(*e),
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(out)
}

/// What a function may return. A number that isn't finite becomes `#NUM!`; `None` is an
/// empty cell; `Err(e)` shows the error.
pub trait IntoValue {
    fn into_value(self) -> Value;
}

impl IntoValue for Value {
    fn into_value(self) -> Value {
        match self {
            Value::Number(n) if !n.is_finite() => Value::Error(ErrorKind::Num),
            v => v,
        }
    }
}

impl IntoValue for f64 {
    fn into_value(self) -> Value {
        Value::Number(self).into_value()
    }
}

impl IntoValue for i64 {
    fn into_value(self) -> Value {
        Value::Number(self as f64)
    }
}

impl IntoValue for usize {
    fn into_value(self) -> Value {
        Value::Number(self as f64)
    }
}

impl IntoValue for bool {
    fn into_value(self) -> Value {
        Value::Bool(self)
    }
}

impl IntoValue for String {
    fn into_value(self) -> Value {
        Value::Text(self)
    }
}

impl IntoValue for &str {
    fn into_value(self) -> Value {
        Value::Text(self.to_string())
    }
}

impl IntoValue for ErrorKind {
    fn into_value(self) -> Value {
        Value::Error(self)
    }
}

impl<T: IntoValue> IntoValue for Option<T> {
    fn into_value(self) -> Value {
        self.map_or(Value::Empty, IntoValue::into_value)
    }
}

impl<T: IntoValue> IntoValue for Result<T, ErrorKind> {
    fn into_value(self) -> Value {
        match self {
            Ok(v) => v.into_value(),
            Err(e) => Value::Error(e),
        }
    }
}

/// A spreadsheet function: made by [`fn_def!`], listed in [`export!`].
#[derive(Clone, Copy, Debug)]
pub struct Function {
    /// Uppercase name, letters, digits, `.` and `_`: `GEOMEAN`, `TEXT.SLUG`.
    pub name: &'static str,
    /// How to call it, shown in the function picker: `GEOMEAN(number1, [number2], …)`.
    pub syntax: &'static str,
    /// One sentence on what it does.
    pub summary: &'static str,
    pub min_args: u32,
    /// At most 255.
    pub max_args: u32,
    pub run: fn(&[Arg]) -> Value,
}

/// An import filter: made by [`filter_def!`], listed in [`export!`]. `import` turns a file's
/// bytes into a folio document (`document.json`, see GUIDE.md) or says why it can't.
#[derive(Clone, Copy, Debug)]
pub struct Filter {
    /// What the file is called: `Org outline`.
    pub name: &'static str,
    /// Comma-separated extensions without dots: `org` or `txt,text`.
    pub extensions: &'static str,
    pub summary: &'static str,
    pub import: fn(&[u8]) -> Result<String, String>,
}

/// One spreadsheet function for [`export!`]:
/// `fn_def!(NAME, syntax, summary, min_args, max_args, function)`, where `function` is a
/// `fn(&[Arg]) -> T` and `T` is anything [`IntoValue`].
#[macro_export]
macro_rules! fn_def {
    ($name:expr, $syntax:expr, $summary:expr, $min:expr, $max:expr, $f:path $(,)?) => {
        $crate::Function {
            name: $name,
            syntax: $syntax,
            summary: $summary,
            min_args: $min as u32,
            max_args: $max as u32,
            run: {
                fn __folio_run(args: &[$crate::Arg]) -> $crate::Value {
                    $crate::IntoValue::into_value($f(args))
                }
                __folio_run
            },
        }
    };
}

/// One import filter for [`export!`]: `filter_def!(name, "ext1,ext2", summary, function)`,
/// where `function` is a `fn(&[u8]) -> Result<String, String>` giving `document.json`.
#[macro_export]
macro_rules! filter_def {
    ($name:expr, $extensions:expr, $summary:expr, $f:path $(,)?) => {
        $crate::Filter { name: $name, extensions: $extensions, summary: $summary, import: $f }
    };
}

/// Exports the plugin: its id (the same as `plugin.toml`'s), name, version, functions and,
/// optionally, import filters. Use it once, at the crate's root.
#[macro_export]
macro_rules! export {
    (
        id: $id:expr,
        name: $name:expr,
        version: $version:expr,
        functions: [$($f:expr),* $(,)?]
        $(, filters: [$($flt:expr),* $(,)?])?
        $(,)?
    ) => {
        const __FOLIO_FUNCTIONS: &[$crate::Function] = &[$($f),*];
        const __FOLIO_FILTERS: &[$crate::Filter] = &[$($($flt),*)?];
        static __FOLIO_FFI_FUNCTIONS: [$crate::ffi::FfiFunction; __FOLIO_FUNCTIONS.len()] = $crate::ffi::function_table(__FOLIO_FUNCTIONS);
        static __FOLIO_FFI_FILTERS: [$crate::ffi::FfiFilter; __FOLIO_FILTERS.len()] = $crate::ffi::filter_table(__FOLIO_FILTERS);

        unsafe extern "C" fn __folio_call(index: u32, args: *const $crate::ffi::FfiValue, count: u32, out: *mut $crate::ffi::FfiValue) -> i32 {
            // SAFETY: forwarded from the host, which follows the ABI's memory rules.
            unsafe { $crate::ffi::call(__FOLIO_FUNCTIONS, index, args, count, out) }
        }

        unsafe extern "C" fn __folio_import(index: u32, bytes: *const u8, len: usize, out: *mut $crate::ffi::FfiBytes) -> i32 {
            // SAFETY: forwarded from the host, which follows the ABI's memory rules.
            unsafe { $crate::ffi::import(__FOLIO_FILTERS, index, bytes, len, out) }
        }

        static __FOLIO_VTABLE: $crate::ffi::PluginVTable = $crate::ffi::PluginVTable {
            abi_version: $crate::ABI_VERSION,
            size: ::core::mem::size_of::<$crate::ffi::PluginVTable>() as u32,
            kinds: (if __FOLIO_FUNCTIONS.is_empty() { 0 } else { $crate::ffi::KIND_FUNCTIONS })
                | (if __FOLIO_FILTERS.is_empty() { 0 } else { $crate::ffi::KIND_FILTERS }),
            function_count: __FOLIO_FUNCTIONS.len() as u32,
            id: $crate::ffi::FfiStr::new($id),
            name: $crate::ffi::FfiStr::new($name),
            version: $crate::ffi::FfiStr::new($version),
            functions: __FOLIO_FFI_FUNCTIONS.as_ptr(),
            call: __folio_call,
            free_value: $crate::ffi::free_value,
            filter_count: __FOLIO_FILTERS.len() as u32,
            reserved: 0,
            filters: __FOLIO_FFI_FILTERS.as_ptr(),
            import: __folio_import,
            free_bytes: $crate::ffi::free_bytes,
        };

        /// The plugin's entry point: folio passes the ABI it speaks.
        #[unsafe(no_mangle)]
        pub extern "C" fn folio_plugin_entry(host_abi: u32) -> *const $crate::ffi::PluginVTable {
            if host_abi < $crate::ABI_VERSION { ::core::ptr::null() } else { &__FOLIO_VTABLE }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::ffi::*;
    use super::*;

    fn geomean(args: &[Arg]) -> Result<f64, ErrorKind> {
        let xs = numbers(args)?;
        if xs.is_empty() || xs.iter().any(|x| *x <= 0.0) {
            return Err(ErrorKind::Num);
        }
        Ok((xs.iter().map(|x| x.ln()).sum::<f64>() / xs.len() as f64).exp())
    }

    fn shout(args: &[Arg]) -> Result<String, ErrorKind> {
        Ok(args[0].text()?.to_uppercase() + "!")
    }

    fn boom(_: &[Arg]) -> Value {
        panic!("boom")
    }

    fn lines(bytes: &[u8]) -> Result<String, String> {
        std::str::from_utf8(bytes).map(|s| format!("{{\"lines\":{}}}", s.lines().count())).map_err(|_| "not UTF-8".into())
    }

    export! {
        id: "dev.folio.test",
        name: "Test",
        version: "1.2.3",
        functions: [
            fn_def!("GEOMEAN", "GEOMEAN(number1, …)", "Geometric mean.", 1, 255, geomean),
            fn_def!("SHOUT", "SHOUT(text)", "Upper case and a bang.", 1, 1, shout),
            fn_def!("BOOM", "BOOM()", "Panics.", 0, 0, boom),
        ],
        filters: [filter_def!("Lines", "lines,ln", "Counts lines.", lines)],
    }

    fn table() -> &'static PluginVTable {
        // SAFETY: the table is a static of this test binary.
        unsafe { &*folio_plugin_entry(ABI_VERSION) }
    }

    fn call_fn(i: u32, args: &[FfiValue]) -> (i32, Value) {
        let t = table();
        let mut out = FfiValue::EMPTY;
        let status = unsafe { (t.call)(i, args.as_ptr(), args.len() as u32, &mut out) };
        let v = unsafe { out.read_scalar() };
        unsafe { (t.free_value)(&mut out) };
        (status, v)
    }

    #[test]
    fn the_table_describes_the_plugin() {
        let t = table();
        assert_eq!(t.abi_version, ABI_VERSION);
        assert_eq!(t.size as usize, std::mem::size_of::<PluginVTable>());
        assert_eq!(t.kinds, KIND_FUNCTIONS | KIND_FILTERS);
        assert_eq!(unsafe { t.id.to_string_lossy() }, "dev.folio.test");
        assert_eq!(unsafe { t.version.to_string_lossy() }, "1.2.3");
        assert_eq!(t.function_count, 3);
        let f = unsafe { &*t.functions.add(1) };
        assert_eq!(unsafe { f.name.to_string_lossy() }, "SHOUT");
        assert_eq!((f.min_args, f.max_args), (1, 1));
        assert!(folio_plugin_entry(0).is_null(), "an older host is refused");
    }

    #[test]
    fn calls_cross_the_boundary() {
        let cells = [FfiValue::number(2.0), FfiValue::text("x"), FfiValue::number(8.0)];
        let (s, v) = call_fn(0, &[FfiValue::range(1, 3, &cells)]);
        assert_eq!(s, OK);
        assert_eq!(v, Value::Number(4.0));
        let (s, v) = call_fn(1, &[FfiValue::text("hé")]);
        assert_eq!((s, v), (OK, Value::Text("HÉ!".into())));
        let (s, v) = call_fn(0, &[FfiValue::number(-1.0)]);
        assert_eq!((s, v), (OK, Value::Error(ErrorKind::Num)));
        let (s, v) = call_fn(1, &[FfiValue::error(ErrorKind::NA)]);
        assert_eq!((s, v), (OK, Value::Error(ErrorKind::NA)));
        let (s, _) = call_fn(9, &[]);
        assert_eq!(s, NO_SUCH_FUNCTION);
    }

    #[test]
    fn a_panic_is_contained() {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let (s, v) = call_fn(2, &[]);
        std::panic::set_hook(prev);
        assert_eq!((s, v), (PANICKED, Value::Error(ErrorKind::Value)));
    }

    #[test]
    fn filters_import_bytes() {
        let t = table();
        assert_eq!(t.filter_count, 1);
        let f = unsafe { &*t.filters };
        assert_eq!(unsafe { f.extensions.to_string_lossy() }, "lines,ln");
        let data = b"a\nb\nc";
        let mut out = FfiBytes::EMPTY;
        let s = unsafe { (t.import)(0, data.as_ptr(), data.len(), &mut out) };
        assert_eq!(s, OK);
        let json = unsafe { std::slice::from_raw_parts(out.ptr, out.len) }.to_vec();
        unsafe { (t.free_bytes)(out) };
        assert_eq!(json, b"{\"lines\":3}");
    }

    #[test]
    fn coercions_follow_spreadsheets() {
        assert_eq!(Value::Text(" 3 ".into()).number(), Ok(3.0));
        assert_eq!(Value::Text("x".into()).number(), Err(ErrorKind::Value));
        assert_eq!(Value::Number(0.1 + 0.2).text(), Ok("0.3".into()));
        assert_eq!(Value::Number(42.0).text(), Ok("42".into()));
        assert_eq!(numbers(&[Arg::Value(Value::Bool(true)), Arg::Range { rows: 1, cols: 2, values: vec![Value::Bool(true), Value::Number(2.0)] }]), Ok(vec![1.0, 2.0]));
        assert_eq!(f64::NAN.into_value(), Value::Error(ErrorKind::Num));
        for e in [ErrorKind::Div0, ErrorKind::NA, ErrorKind::Name, ErrorKind::Null, ErrorKind::Num, ErrorKind::Ref, ErrorKind::Value, ErrorKind::Circular] {
            assert_eq!(ErrorKind::from_code_number(e.code_number()), e);
        }
    }

    /// Every public item of this crate (types, functions, macros, methods) is named in GUIDE.md.
    #[test]
    fn the_guide_names_every_export() {
        let mut missing = Vec::new();
        for src in [include_str!("lib.rs"), include_str!("ffi.rs")] {
            let is_ffi = src.contains("pub struct PluginVTable");
            for line in src.lines() {
                if line.starts_with("#[cfg(test)]") {
                    break;
                }
                let t = line.trim_start();
                let name = if let Some(rest) = t.strip_prefix("macro_rules! ") {
                    Some(format!("{}!", rest.trim_end_matches(" {")))
                } else {
                    ["pub const fn ", "pub fn ", "pub unsafe fn ", "pub unsafe extern \"C\" fn ", "pub struct ", "pub enum ", "pub trait ", "pub const ", "pub type ", "pub union ", "pub mod "]
                        .iter()
                        .find_map(|p| t.strip_prefix(p))
                        .map(|rest| rest.split(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or("").to_string())
                };
                // The ABI's own constants and tables are documented by group in the guide.
                if let Some(name) = name.filter(|n| !n.is_empty()) {
                    let ffi_detail = is_ffi && (name.starts_with("TAG_") || name.starts_with("KIND_") || name.starts_with("Ffi") || name.ends_with("Fn") || name == "EMPTY");
                    if !ffi_detail && !GUIDE.contains(&name) {
                        missing.push(name);
                    }
                }
            }
        }
        for word in ["plugin.toml", "cdylib", "panic = \"unwind\"", "plugin.new", "plugin.writeSource", "plugin.build", "plugin.publishLocal", "plugin.toolchain", "plugin.guide", "TAG_", "FfiValue", "free_value"] {
            if !GUIDE.contains(word) {
                missing.push(word.to_string());
            }
        }
        assert!(missing.is_empty(), "GUIDE.md doesn't mention: {missing:?}");
    }
}

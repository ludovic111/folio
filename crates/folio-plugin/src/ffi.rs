//! The C ABI between folio and a plugin library. **Frozen**: ABI 1's layouts below never
//! change (a compile-time check fails if they move). A later ABI appends fields to
//! [`PluginVTable`] (its `size` says how much a plugin filled in) or adds a new symbol.
//!
//! A library exports one symbol, `folio_plugin_entry(host_abi: u32) -> *const PluginVTable`.
//! The host passes the ABI it speaks; the plugin answers null if it needs a newer host. The
//! host reads `abi_version` (always the first field of every table, in every ABI) and refuses
//! the library unless it is [`ABI_VERSION`](crate::ABI_VERSION), before reading or calling
//! anything else. Plugin authors never write this by hand: [`export!`](crate::export) builds
//! the table.
//!
//! # Memory rules
//!
//! * **Strings in the table** (`id`, `name`, function names…) are the plugin's statics: valid
//!   while the library is loaded. The host copies them.
//! * **Arguments** to `call` are the host's: an array of `count` [`FfiValue`]s, and for a
//!   range an array of `rows × cols` scalar values (row by row; never nested ranges). Text
//!   points at UTF-8 bytes. All of it is valid only during the call; the plugin copies what it
//!   keeps (the SDK does).
//! * **The result** is written by the plugin into `*out`, which the host set to Empty before
//!   the call. Text in a result is allocated by the plugin; after every call (whatever the
//!   status) the host reads `*out`, copies the text, and hands it back with `free_value(out)`,
//!   which releases it with the plugin's own allocator. Results are scalars: a range in a
//!   result is read as `#VALUE!`.
//! * **Import filters** write the document into a buffer the plugin allocates ([`FfiBytes`]);
//!   the host copies it and releases it with `free_bytes`.
//!
//! # Status codes
//!
//! `call` and `import` return [`OK`], [`BAD_ARGUMENT`] (null pointers, a malformed value),
//! [`PANICKED`] (the plugin panicked: the host switches it off), [`NO_SUCH_FUNCTION`] or
//! [`FAILED`] (an import that didn't work; the bytes hold the message).

use std::panic::AssertUnwindSafe;

use crate::{Arg, ErrorKind, Filter, Function, Value};

/// The call finished; `*out` holds the result.
pub const OK: i32 = 0;
/// A null pointer or a malformed value from the host.
pub const BAD_ARGUMENT: i32 = 1;
/// The plugin panicked. `*out` holds `#VALUE!`; the host switches the plugin off.
pub const PANICKED: i32 = 2;
/// No function (or filter) at that index.
pub const NO_SUCH_FUNCTION: i32 = 3;
/// An import filter couldn't read the file; the bytes hold a message for the person.
pub const FAILED: i32 = 4;

/// [`FfiValue::tag`]: nothing (an empty cell).
pub const TAG_EMPTY: u32 = 0;
/// A number (`payload.number`).
pub const TAG_NUMBER: u32 = 1;
/// UTF-8 text (`payload.text`).
pub const TAG_TEXT: u32 = 2;
/// TRUE or FALSE (`payload.boolean`, 0 or 1).
pub const TAG_BOOL: u32 = 3;
/// An error (`payload.error`, an [`ErrorKind`] code: 1 `#DIV/0!` … 8 `#CIRC!`).
pub const TAG_ERROR: u32 = 4;
/// A range or array (`payload.range`), arguments only.
pub const TAG_RANGE: u32 = 5;

/// [`PluginVTable::kinds`] bit: the plugin adds spreadsheet functions.
pub const KIND_FUNCTIONS: u32 = 1;
/// [`PluginVTable::kinds`] bit: the plugin adds import filters.
pub const KIND_FILTERS: u32 = 2;

/// A borrowed UTF-8 string: `len` bytes at `ptr` (null when `len` is 0 is fine).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FfiStr {
    pub ptr: *const u8,
    pub len: usize,
}

impl FfiStr {
    pub const EMPTY: FfiStr = FfiStr { ptr: std::ptr::null(), len: 0 };

    /// A static string (the table's names).
    pub const fn new(s: &'static str) -> Self {
        FfiStr { ptr: s.as_ptr(), len: s.len() }
    }

    /// A string borrowed for the length of a call (the host's arguments).
    pub fn borrowed(s: &str) -> Self {
        FfiStr { ptr: s.as_ptr(), len: s.len() }
    }

    /// Copies the string (invalid UTF-8 is replaced, never trusted).
    ///
    /// # Safety
    /// `ptr` must point at `len` readable bytes (or `len` be 0).
    pub unsafe fn to_string_lossy(&self) -> String {
        if self.ptr.is_null() || self.len == 0 {
            return String::new();
        }
        // SAFETY: the caller vouches for `len` readable bytes at `ptr`.
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(self.ptr, self.len) }).into_owned()
    }
}

/// A range or array argument: `rows × cols` scalar values, row by row.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FfiRange {
    pub rows: u32,
    pub cols: u32,
    pub values: *const FfiValue,
}

/// What a value holds, chosen by [`FfiValue::tag`].
#[repr(C)]
#[derive(Clone, Copy)]
pub union FfiPayload {
    pub number: f64,
    pub boolean: u32,
    pub error: u32,
    pub text: FfiStr,
    pub range: FfiRange,
}

/// One value crossing the boundary: a tag ([`TAG_EMPTY`] …) and its payload.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FfiValue {
    pub tag: u32,
    pub payload: FfiPayload,
}

impl std::fmt::Debug for FfiValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FfiValue(tag {})", self.tag)
    }
}

impl FfiValue {
    pub const EMPTY: FfiValue = FfiValue { tag: TAG_EMPTY, payload: FfiPayload { number: 0.0 } };

    pub const fn number(n: f64) -> Self {
        FfiValue { tag: TAG_NUMBER, payload: FfiPayload { number: n } }
    }

    pub const fn boolean(b: bool) -> Self {
        FfiValue { tag: TAG_BOOL, payload: FfiPayload { boolean: b as u32 } }
    }

    pub const fn error(e: ErrorKind) -> Self {
        FfiValue { tag: TAG_ERROR, payload: FfiPayload { error: e.code_number() } }
    }

    /// Text borrowed from the caller (valid while `s` is).
    pub fn text(s: &str) -> Self {
        FfiValue { tag: TAG_TEXT, payload: FfiPayload { text: FfiStr::borrowed(s) } }
    }

    /// A range over `values` (valid while `values` is). `rows × cols` must equal its length.
    pub fn range(rows: u32, cols: u32, values: &[FfiValue]) -> Self {
        FfiValue { tag: TAG_RANGE, payload: FfiPayload { range: FfiRange { rows, cols, values: values.as_ptr() } } }
    }

    /// Reads a scalar value, copying its text. A range, an unknown tag or a bad pointer give
    /// `#VALUE!`; a number that isn't finite gives `#NUM!`.
    ///
    /// # Safety
    /// Text must point at `len` readable bytes.
    pub unsafe fn read_scalar(&self) -> Value {
        // SAFETY: the tag says which field is set.
        unsafe {
            match self.tag {
                TAG_EMPTY => Value::Empty,
                TAG_NUMBER if self.payload.number.is_finite() => Value::Number(self.payload.number),
                TAG_NUMBER => Value::Error(ErrorKind::Num),
                TAG_TEXT if self.payload.text.ptr.is_null() && self.payload.text.len > 0 => Value::Error(ErrorKind::Value),
                TAG_TEXT => Value::Text(self.payload.text.to_string_lossy()),
                TAG_BOOL => Value::Bool(self.payload.boolean != 0),
                TAG_ERROR => Value::Error(ErrorKind::from_code_number(self.payload.error)),
                _ => Value::Error(ErrorKind::Value),
            }
        }
    }
}

/// A spreadsheet function as the table describes it.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FfiFunction {
    pub name: FfiStr,
    pub syntax: FfiStr,
    pub summary: FfiStr,
    pub min_args: u32,
    pub max_args: u32,
}

/// An import filter as the table describes it. `extensions` is a comma-separated list
/// without dots (`org,outline`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FfiFilter {
    pub name: FfiStr,
    pub extensions: FfiStr,
    pub summary: FfiStr,
}

/// Bytes the plugin allocated (an import's document or message), freed with `free_bytes`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FfiBytes {
    pub ptr: *mut u8,
    pub len: usize,
}

impl FfiBytes {
    pub const EMPTY: FfiBytes = FfiBytes { ptr: std::ptr::null_mut(), len: 0 };
}

/// `call(index, args, count, out) -> status`.
pub type CallFn = unsafe extern "C" fn(index: u32, args: *const FfiValue, count: u32, out: *mut FfiValue) -> i32;
/// `free_value(value)`: releases the text of a result.
pub type FreeValueFn = unsafe extern "C" fn(value: *mut FfiValue);
/// `import(index, bytes, len, out) -> status`.
pub type ImportFn = unsafe extern "C" fn(index: u32, bytes: *const u8, len: usize, out: *mut FfiBytes) -> i32;
/// `free_bytes(bytes)`: releases an import's output.
pub type FreeBytesFn = unsafe extern "C" fn(bytes: FfiBytes);

/// The one table a plugin library hands out.
#[repr(C)]
pub struct PluginVTable {
    /// The ABI this table follows. Always the first field, in every ABI.
    pub abi_version: u32,
    /// `size_of::<PluginVTable>()` as the plugin was built; a host reads only what fits.
    pub size: u32,
    /// [`KIND_FUNCTIONS`] and/or [`KIND_FILTERS`].
    pub kinds: u32,
    pub function_count: u32,
    /// Reverse-DNS id, the same as `plugin.toml`'s.
    pub id: FfiStr,
    pub name: FfiStr,
    pub version: FfiStr,
    pub functions: *const FfiFunction,
    pub call: CallFn,
    pub free_value: FreeValueFn,
    pub filter_count: u32,
    pub reserved: u32,
    pub filters: *const FfiFilter,
    pub import: ImportFn,
    pub free_bytes: FreeBytesFn,
}

// Tables are plain data and function pointers to statics.
unsafe impl Sync for PluginVTable {}
unsafe impl Send for PluginVTable {}
unsafe impl Sync for FfiFunction {}
unsafe impl Sync for FfiFilter {}

/// The exported entry function's type, for hosts loading a library.
pub type EntryFn = unsafe extern "C" fn(host_abi: u32) -> *const PluginVTable;

// ABI 1 is frozen: a change that moves any of these fails to compile.
#[cfg(target_pointer_width = "64")]
const _: () = {
    use std::mem::{offset_of, size_of};
    assert!(size_of::<FfiStr>() == 16);
    assert!(size_of::<FfiRange>() == 16);
    assert!(size_of::<FfiValue>() == 24);
    assert!(offset_of!(FfiValue, payload) == 8);
    assert!(size_of::<FfiFunction>() == 56);
    assert!(offset_of!(FfiFunction, min_args) == 48);
    assert!(size_of::<FfiFilter>() == 48);
    assert!(size_of::<FfiBytes>() == 16);
    assert!(offset_of!(PluginVTable, size) == 4);
    assert!(offset_of!(PluginVTable, kinds) == 8);
    assert!(offset_of!(PluginVTable, function_count) == 12);
    assert!(offset_of!(PluginVTable, id) == 16);
    assert!(offset_of!(PluginVTable, functions) == 64);
    assert!(offset_of!(PluginVTable, call) == 72);
    assert!(offset_of!(PluginVTable, free_value) == 80);
    assert!(offset_of!(PluginVTable, filter_count) == 88);
    assert!(offset_of!(PluginVTable, filters) == 96);
    assert!(offset_of!(PluginVTable, import) == 104);
    assert!(offset_of!(PluginVTable, free_bytes) == 112);
    assert!(size_of::<PluginVTable>() == 120);
};

// ---- the plugin's side (used by `export!`) ---------------------------------------------------

/// The table of functions, built at compile time by [`export!`](crate::export).
pub const fn function_table<const N: usize>(defs: &[Function]) -> [FfiFunction; N] {
    let mut out = [FfiFunction { name: FfiStr::EMPTY, syntax: FfiStr::EMPTY, summary: FfiStr::EMPTY, min_args: 0, max_args: 0 }; N];
    let mut i = 0;
    while i < N {
        let f = &defs[i];
        out[i] = FfiFunction { name: FfiStr::new(f.name), syntax: FfiStr::new(f.syntax), summary: FfiStr::new(f.summary), min_args: f.min_args, max_args: f.max_args };
        i += 1;
    }
    out
}

/// The table of import filters, built at compile time by [`export!`](crate::export).
pub const fn filter_table<const N: usize>(defs: &[Filter]) -> [FfiFilter; N] {
    let mut out = [FfiFilter { name: FfiStr::EMPTY, extensions: FfiStr::EMPTY, summary: FfiStr::EMPTY }; N];
    let mut i = 0;
    while i < N {
        let f = &defs[i];
        out[i] = FfiFilter { name: FfiStr::new(f.name), extensions: FfiStr::new(f.extensions), summary: FfiStr::new(f.summary) };
        i += 1;
    }
    out
}

/// Reads the host's arguments into owned [`Arg`]s.
///
/// # Safety
/// `args` must hold `count` values laid out as the memory rules say.
unsafe fn lift_args(args: *const FfiValue, count: u32) -> Option<Vec<Arg>> {
    if count == 0 {
        return Some(Vec::new());
    }
    if args.is_null() {
        return None;
    }
    // SAFETY: the host passes `count` values.
    let raw = unsafe { std::slice::from_raw_parts(args, count as usize) };
    raw.iter()
        .map(|v| {
            if v.tag != TAG_RANGE {
                // SAFETY: scalar values follow the memory rules.
                return Some(Arg::Value(unsafe { v.read_scalar() }));
            }
            // SAFETY: the tag says the range field is set.
            let r = unsafe { v.payload.range };
            let n = (r.rows as usize).checked_mul(r.cols as usize)?;
            if n > 0 && r.values.is_null() {
                return None;
            }
            let cells = if n == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(r.values, n) } };
            // SAFETY: the host never nests ranges; a nested one reads as #VALUE!.
            let values = cells.iter().map(|c| unsafe { c.read_scalar() }).collect();
            Some(Arg::Range { rows: r.rows, cols: r.cols, values })
        })
        .collect()
}

/// Turns a result into its C form, moving text onto the heap (released by [`free_value`]).
fn lower_result(v: Value) -> FfiValue {
    match v {
        Value::Empty => FfiValue::EMPTY,
        Value::Number(n) if n.is_finite() => FfiValue::number(n),
        Value::Number(_) => FfiValue::error(ErrorKind::Num),
        Value::Bool(b) => FfiValue::boolean(b),
        Value::Error(e) => FfiValue::error(e),
        Value::Text(s) => {
            let bytes = s.into_bytes().into_boxed_slice();
            let len = bytes.len();
            let ptr = Box::into_raw(bytes) as *mut u8 as *const u8;
            FfiValue { tag: TAG_TEXT, payload: FfiPayload { text: FfiStr { ptr, len } } }
        }
    }
}

/// `call`, panic-guarded: runs function `index` of `defs` on the host's arguments.
///
/// # Safety
/// Called by the host with arguments that follow the memory rules.
pub unsafe fn call(defs: &[Function], index: u32, args: *const FfiValue, count: u32, out: *mut FfiValue) -> i32 {
    if out.is_null() {
        return BAD_ARGUMENT;
    }
    let Some(f) = defs.get(index as usize) else {
        // SAFETY: checked for null; the host owns it for this call.
        unsafe { *out = FfiValue::error(ErrorKind::Name) };
        return NO_SUCH_FUNCTION;
    };
    let run = AssertUnwindSafe(|| {
        // SAFETY: forwarded from the host's call.
        let args = unsafe { lift_args(args, count) }?;
        Some((f.run)(&args))
    });
    let (value, status) = match std::panic::catch_unwind(run) {
        Ok(Some(v)) => (v, OK),
        Ok(None) => (Value::Error(ErrorKind::Value), BAD_ARGUMENT),
        Err(_) => (Value::Error(ErrorKind::Value), PANICKED),
    };
    // SAFETY: checked for null above.
    unsafe { *out = lower_result(value) };
    status
}

/// `free_value`: releases the text of a value written by [`call`] and leaves Empty behind.
///
/// # Safety
/// `value` is null or a value this library wrote.
pub unsafe extern "C" fn free_value(value: *mut FfiValue) {
    if value.is_null() {
        return;
    }
    // SAFETY: a value this library wrote: text came from `Box<[u8]>` in `lower_result`.
    unsafe {
        let v = &mut *value;
        if v.tag == TAG_TEXT && !v.payload.text.ptr.is_null() {
            let t = v.payload.text;
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(t.ptr as *mut u8, t.len)));
        }
        *v = FfiValue::EMPTY;
    }
}

fn give(bytes: Vec<u8>) -> FfiBytes {
    let boxed = bytes.into_boxed_slice();
    let len = boxed.len();
    FfiBytes { ptr: Box::into_raw(boxed) as *mut u8, len }
}

/// `import`, panic-guarded: runs filter `index` of `defs` on a file's bytes.
///
/// # Safety
/// `bytes` holds `len` readable bytes; `out` is writable.
pub unsafe fn import(defs: &[Filter], index: u32, bytes: *const u8, len: usize, out: *mut FfiBytes) -> i32 {
    if out.is_null() || (bytes.is_null() && len > 0) {
        return BAD_ARGUMENT;
    }
    let Some(f) = defs.get(index as usize) else { return NO_SUCH_FUNCTION };
    // SAFETY: the host passes `len` bytes, valid for this call.
    let input = if len == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(bytes, len) } };
    let (data, status) = match std::panic::catch_unwind(AssertUnwindSafe(|| (f.import)(input))) {
        Ok(Ok(json)) => (json.into_bytes(), OK),
        Ok(Err(message)) => (message.into_bytes(), FAILED),
        Err(_) => (b"The plugin crashed while reading the file.".to_vec(), PANICKED),
    };
    // SAFETY: checked for null above.
    unsafe { *out = give(data) };
    status
}

/// `free_bytes`: releases bytes written by [`import`].
///
/// # Safety
/// `bytes` came from this library's [`import`] (or is empty).
pub unsafe extern "C" fn free_bytes(bytes: FfiBytes) {
    if !bytes.ptr.is_null() {
        // SAFETY: made by `give` from a `Box<[u8]>` of this length.
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(bytes.ptr, bytes.len)) });
    }
}

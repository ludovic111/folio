//! folio-calc: the spreadsheet formula engine of folio.
//!
//! It reads what people type in cells ([`parse_input`]), parses formulas into trees once per
//! cell, evaluates them with Excel's rules, keeps a dependency graph so a change recalculates
//! only what depends on it ([`Engine`]), formats numbers with Excel format codes
//! ([`format_value`]) and rewrites formulas when they are copied or when rows, columns and
//! sheets change ([`translate`], [`adjust_for_insert`], [`rename_sheet`]).
//!
//! ```
//! use folio_calc::{Addr, Engine, Value};
//!
//! let mut engine = Engine::new();
//! engine.set_sheets(&["Sheet1".to_string()]);
//! engine.set_cell(0, Addr::parse("A1").unwrap(), "2");
//! engine.set_cell(0, Addr::parse("A2").unwrap(), "=A1*21");
//! engine.recalc();
//! assert_eq!(engine.value(0, Addr::parse("A2").unwrap()), Value::Number(42.0));
//! ```
//!
//! # Decisions
//!
//! These choices were made without anyone to ask; they are easy to revisit.
//!
//! - **Excel compatibility first.** Dates are Excel 1900 serials, including the fictitious
//!   1900-02-29 (serial 60), so values match XLSX files. Number formats are Excel format codes.
//!   Function names accept the `_xlfn.` prefix XLSX files use. Numbers compare equal when they
//!   agree to about 15 significant digits (`=0.1+0.2=0.3` is TRUE, as in Excel).
//! - **Formula syntax** is the English (en-US) one: `,` between arguments, `.` for decimals.
//!   There is no space (intersection) operator, no union `(A1,B2)`, no 3-D references
//!   (`Sheet1:Sheet3!A1`), no R1C1 references (INDIRECT with `a1 = FALSE` gives `#REF!`) and no
//!   defined names (an unknown name gives `#NAME?`). A formula that does not parse shows
//!   `#NAME?` in its cell; [`validate_formula`] says what is wrong for the formula bar.
//! - **Arrays** are evaluated (`SUMPRODUCT(A1:A3*B1:B3)`, `SUM(IF(A1:A9>0, A1:A9))`,
//!   `{1,2;3,4}`), but dynamic arrays do not spill yet: a cell keeps the top-left element of an
//!   array, or for a range the cell in its own row or column (implicit intersection), else
//!   `#VALUE!`. Whole columns and rows (`A:A`, `1:1`) are cut to the area where any sheet has
//!   data, so they cost no more than the data they cover.
//! - **Custom functions** (plugins) cannot replace built-ins: [`Engine::register_function`]
//!   returns `false` for a built-in name and does nothing. They are never volatile.
//! - **Wrong argument counts** give `#VALUE!` (Excel refuses such formulas when typed).
//! - **Cycles** give `#CIRC!` in every cell of the cycle (no iterative calculation); cells that
//!   read a cycle get `#CIRC!` through ordinary error propagation.
//! - **OFFSET and INDIRECT** are volatile (as in Excel). The graph cannot know which cells they
//!   reach, so formulas downstream of volatile formulas are evaluated after all the others.
//! - **Criteria** (`COUNTIF` and friends, [`matches_criteria`]): a number matches only numbers
//!   (text that looks like a number does not), `"<>x"` also matches empty cells, `""` matches
//!   empty cells and empty text, `"="` only empty cells. Whole-column criteria only look at the
//!   used area, so `COUNTIF(A:A, "")` counts blanks up to the last used row.
//! - **Sheets** are identified by position; [`Engine::set_sheets`] keeps a sheet's cells when
//!   its name is still there (at any position) or when the sheet at its position was renamed.
//!   Setting a cell on a sheet that was never named creates `Sheet1`, `Sheet2`… up to it.
//! - **Not done yet**: dynamic array spilling, fractions in number formats (`# ?/?`),
//!   conditional sections (`[>100]`) and colours in number formats (read and ignored), locale
//!   formats, iterative calculation.

pub mod addr;
pub mod engine;
mod eval;
pub mod format;
pub mod functions;
pub mod input;
mod lexer;
pub mod parser;
pub mod rewrite;
mod util;
pub mod value;

pub use addr::{Addr, MAX_COLS, MAX_ROWS, Range, SheetRange, col_index, col_name, quote_sheet_name};
pub use engine::{Arg, CustomFn, Engine, FunctionInfo};
pub use format::{PRESETS, date_to_serial, format_value, general, is_date_format, serial_to_date, time_fraction};
pub use functions::criteria::matches_criteria;
pub use functions::{Builtin, builtin_functions};
pub use input::{Input, parse_input};
pub use parser::{ParseError, validate_formula};
pub use rewrite::{Axis, adjust_for_insert, references, rename_sheet, translate};
pub use value::{ErrorKind, Value};

//! Information functions.

use super::{Builtin, Imp, def};
use crate::eval::{Ctx, R};
use crate::value::{ErrorKind, Value};

const I: &str = "Information";

pub(crate) const FUNCTIONS: &[Builtin] = &[
    def("ISBLANK", I, "ISBLANK(value)", "TRUE for an empty cell.", 1, 1, Imp::Scalar(isblank)),
    def("ISNUMBER", I, "ISNUMBER(value)", "TRUE for a number.", 1, 1, Imp::Scalar(isnumber)),
    def("ISTEXT", I, "ISTEXT(value)", "TRUE for text.", 1, 1, Imp::Scalar(istext)),
    def("ISERROR", I, "ISERROR(value)", "TRUE for any error.", 1, 1, Imp::Scalar(iserror)),
    def("ISERR", I, "ISERR(value)", "TRUE for any error except #N/A.", 1, 1, Imp::Scalar(iserr)),
    def("ISNA", I, "ISNA(value)", "TRUE for #N/A.", 1, 1, Imp::Scalar(isna)),
    def("ISLOGICAL", I, "ISLOGICAL(value)", "TRUE for TRUE or FALSE.", 1, 1, Imp::Scalar(islogical)),
    def("NA", I, "NA()", "The #N/A error, for values not available.", 0, 0, Imp::Scalar(na)),
];

fn isblank(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Bool(args[0].is_empty()))
}

fn isnumber(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Bool(matches!(args[0], Value::Number(_))))
}

fn istext(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Bool(matches!(args[0], Value::Text(_))))
}

fn iserror(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Bool(matches!(args[0], Value::Error(_))))
}

fn iserr(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Bool(matches!(args[0], Value::Error(e) if e != ErrorKind::NA)))
}

fn isna(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Bool(args[0] == Value::Error(ErrorKind::NA)))
}

fn islogical(_: &Ctx, args: &[Value]) -> R<Value> {
    Ok(Value::Bool(matches!(args[0], Value::Bool(_))))
}

fn na(_: &Ctx, _: &[Value]) -> R<Value> {
    Err(ErrorKind::NA)
}

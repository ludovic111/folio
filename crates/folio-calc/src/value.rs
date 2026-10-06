//! Cell values and error kinds.
//!
//! A [`Value`] is what a cell holds after calculation. In a saved file it is stored as plain
//! JSON: a number, a string, a boolean, `null` for an empty cell and `{"error": "#DIV/0!"}`
//! for an error.

use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The value of a cell.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Value {
    /// Nothing in the cell.
    #[default]
    Empty,
    Number(f64),
    Text(String),
    Bool(bool),
    Error(ErrorKind),
}

/// The errors a formula can produce, as spreadsheets show them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// `#DIV/0!`: division by zero.
    Div0,
    /// `#N/A`: a value is not available (lookups that find nothing).
    NA,
    /// `#NAME?`: an unknown function or name, or a formula that does not parse.
    Name,
    /// `#NULL!`: an empty intersection.
    Null,
    /// `#NUM!`: a number out of range.
    Num,
    /// `#REF!`: a reference that does not exist.
    Ref,
    /// `#VALUE!`: a value of the wrong type.
    Value,
    /// `#CIRC!`: the cell is part of a circular reference.
    Circular,
}

impl ErrorKind {
    /// Every error kind, in a stable order.
    pub const ALL: [ErrorKind; 8] = [
        ErrorKind::Div0,
        ErrorKind::NA,
        ErrorKind::Name,
        ErrorKind::Null,
        ErrorKind::Num,
        ErrorKind::Ref,
        ErrorKind::Value,
        ErrorKind::Circular,
    ];

    /// The code shown in a cell, such as `#DIV/0!`.
    pub fn code(self) -> &'static str {
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

    /// Reads an error code such as `#N/A` (any case).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        Self::ALL.into_iter().find(|kind| kind.code().eq_ignore_ascii_case(s))
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl Value {
    /// The number in the cell, if it holds a number (booleans and text give `None`).
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// True for an empty cell (an empty string is text, not empty).
    pub fn is_empty(&self) -> bool {
        matches!(self, Value::Empty)
    }

    /// True for an error value.
    pub fn is_error(&self) -> bool {
        matches!(self, Value::Error(_))
    }

    /// The value as the General format shows it: numbers with up to 11 characters,
    /// `TRUE`/`FALSE`, error codes, text as is, and nothing for an empty cell.
    pub fn display(&self) -> String {
        match self {
            Value::Empty => String::new(),
            Value::Number(n) => crate::format::general(*n),
            Value::Text(s) => s.clone(),
            Value::Bool(true) => "TRUE".into(),
            Value::Bool(false) => "FALSE".into(),
            Value::Error(e) => e.code().into(),
        }
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Text(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Text(s)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<ErrorKind> for Value {
    fn from(e: ErrorKind) -> Self {
        Value::Error(e)
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Empty => serializer.serialize_none(),
            Value::Number(n) => serializer.serialize_f64(*n),
            Value::Text(s) => serializer.serialize_str(s),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Error(e) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("error", e.code())?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a number, a string, a boolean, null or {\"error\": code}")
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Empty)
    }
    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Empty)
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        Value::deserialize(d)
    }
    fn visit_bool<E: de::Error>(self, b: bool) -> Result<Value, E> {
        Ok(Value::Bool(b))
    }
    fn visit_i64<E: de::Error>(self, n: i64) -> Result<Value, E> {
        Ok(Value::Number(n as f64))
    }
    fn visit_u64<E: de::Error>(self, n: u64) -> Result<Value, E> {
        Ok(Value::Number(n as f64))
    }
    fn visit_f64<E: de::Error>(self, n: f64) -> Result<Value, E> {
        Ok(Value::Number(n))
    }
    fn visit_str<E: de::Error>(self, s: &str) -> Result<Value, E> {
        Ok(Value::Text(s.to_string()))
    }
    fn visit_string<E: de::Error>(self, s: String) -> Result<Value, E> {
        Ok(Value::Text(s))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut kind = None;
        while let Some(key) = map.next_key::<String>()? {
            if key == "error" {
                let code: String = map.next_value()?;
                kind = Some(
                    ErrorKind::parse(&code).ok_or_else(|| de::Error::custom(format!("unknown error code {code}")))?,
                );
            } else {
                map.next_value::<de::IgnoredAny>()?;
            }
        }
        kind.map(Value::Error).ok_or_else(|| de::Error::missing_field("error"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_round_trip() {
        for kind in ErrorKind::ALL {
            assert_eq!(ErrorKind::parse(kind.code()), Some(kind));
        }
        assert_eq!(ErrorKind::parse("#n/a"), Some(ErrorKind::NA));
        assert_eq!(ErrorKind::parse("#CIRC!"), Some(ErrorKind::Circular));
        assert_eq!(ErrorKind::parse("#WHAT"), None);
    }

    #[test]
    fn serde_shapes() {
        let cases = [
            (Value::Number(1.5), "1.5"),
            (Value::Text("hi".into()), "\"hi\""),
            (Value::Bool(true), "true"),
            (Value::Empty, "null"),
            (Value::Error(ErrorKind::Div0), "{\"error\":\"#DIV/0!\"}"),
        ];
        for (value, json) in cases {
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
            assert_eq!(serde_json::from_str::<Value>(json).unwrap(), value);
        }
        assert_eq!(serde_json::from_str::<Value>("3").unwrap(), Value::Number(3.0));
        assert!(serde_json::from_str::<Value>("{\"error\":\"#NOPE\"}").is_err());
        let list: Vec<Value> = serde_json::from_str("[1, null, \"a\"]").unwrap();
        assert_eq!(list, vec![Value::Number(1.0), Value::Empty, Value::Text("a".into())]);
    }

    #[test]
    fn helpers() {
        assert_eq!(Value::Number(2.0).as_number(), Some(2.0));
        assert_eq!(Value::Text("2".into()).as_number(), None);
        assert!(Value::Empty.is_empty());
        assert!(!Value::Text(String::new()).is_empty());
        assert_eq!(Value::Bool(false).display(), "FALSE");
        assert_eq!(Value::Error(ErrorKind::NA).display(), "#N/A");
        assert_eq!(Value::Number(0.1 + 0.2).display(), "0.3");
    }
}

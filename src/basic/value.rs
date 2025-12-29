//! BASIC value types

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// A BASIC value (integer, string, or array)
#[derive(Clone, Debug)]
pub enum Value {
    /// Integer value
    Integer(i64),
    /// String value
    String(String),
    /// Integer array
    IntArray(Vec<i64>),
    /// String array
    StringArray(Vec<String>),
}

impl Value {
    /// Get integer value, or None if not an integer
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Value::Integer(n) => Some(*n),
            Value::String(_) => None,
            Value::IntArray(_) => None,
            Value::StringArray(_) => None,
        }
    }

    /// Get string value, or None if not a string
    pub fn as_string(&self) -> Option<String> {
        match self {
            Value::Integer(_) => None,
            Value::String(s) => Some(s.clone()),
            Value::IntArray(_) => None,
            Value::StringArray(_) => None,
        }
    }

    /// Check if value is truthy (non-zero or non-empty)
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Integer(n) => *n != 0,
            Value::String(s) => !s.is_empty(),
            Value::IntArray(arr) => !arr.is_empty(),
            Value::StringArray(arr) => !arr.is_empty(),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Integer(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "{}", s),
            Value::IntArray(_) => write!(f, "[Array]"),
            Value::StringArray(_) => write!(f, "[Array]"),
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Value::Integer(0)
    }
}

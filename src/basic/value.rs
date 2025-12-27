//! BASIC value types

use alloc::string::String;
use core::fmt;

/// A BASIC value (integer or string)
#[derive(Clone, Debug)]
pub enum Value {
    /// Integer value
    Integer(i64),
    /// String value
    String(String),
}

impl Value {
    /// Get integer value, or None if string
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Value::Integer(n) => Some(*n),
            Value::String(_) => None,
        }
    }

    /// Check if value is truthy (non-zero or non-empty)
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Integer(n) => *n != 0,
            Value::String(s) => !s.is_empty(),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Integer(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "{}", s),
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Value::Integer(0)
    }
}

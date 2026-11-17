use std::cmp::Ordering;
use std::fmt;

use crate::ast::Type;

/// A runtime value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl Value {
    /// Apply the only implicit conversion (int -> float) when storing into `ty`.
    pub fn convert_to(self, ty: Type) -> Value {
        match (self, ty) {
            (Value::Int(n), Type::Float) => Value::Float(n as f64),
            (v, _) => v,
        }
    }

    pub fn as_bool(&self) -> bool {
        matches!(self, Value::Bool(true))
    }

    /// Order two values of compatible types (the parser guarantees compatibility).
    pub fn compare(&self, other: &Value) -> Ordering {
        match (self, other) {
            (Value::Str(a), Value::Str(b)) => a.cmp(b), // byte-wise, like C `strcmp`
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (a, b) => a.as_f64().partial_cmp(&b.as_f64()).unwrap_or(Ordering::Equal),
        }
    }

    fn as_f64(&self) -> f64 {
        match self {
            Value::Int(n) => *n as f64,
            Value::Float(f) => *f,
            _ => 0.0,
        }
    }
}

/// Printed exactly like the compiled C program (`%s`, `%lld`, `%g`, `true`/`false`).
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Str(s) => f.write_str(s),
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(x) => f.write_str(&format_g(*x)),
            Value::Bool(b) => write!(f, "{}", b),
        }
    }
}

/// Format a float like C's `printf("%g")`: 6 significant digits, trailing zeros
/// removed, scientific notation for very small or very large numbers.
pub fn format_g(x: f64) -> String {
    const PRECISION: i32 = 6;
    if x == 0.0 {
        return "0".to_string();
    }

    // Round to 6 significant digits first; the exponent decides the style.
    let sci = format!("{:.*e}", (PRECISION - 1) as usize, x);
    let (mantissa, exp) = sci.split_once('e').expect("`{:e}` always has an exponent");
    let exp: i32 = exp.parse().expect("`{:e}` exponent is an integer");

    if exp < -4 || exp >= PRECISION {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{}{:02}", trim_zeros(mantissa), sign, exp.abs())
    } else {
        let decimals = (PRECISION - 1 - exp) as usize;
        trim_zeros(&format!("{:.*}", decimals, x)).to_string()
    }
}

fn trim_zeros(s: &str) -> &str {
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { s }
}

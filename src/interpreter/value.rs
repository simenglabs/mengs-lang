use std::cmp::Ordering;
use std::fmt;

use crate::ast::{BinOp, Type};

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

    pub fn ty(&self) -> Type {
        match self {
            Value::Str(_) => Type::Str,
            Value::Int(_) => Type::Int,
            Value::Float(_) => Type::Float,
            Value::Bool(_) => Type::Bool,
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

    pub fn negate(self) -> Value {
        match self {
            Value::Int(n) => Value::Int(n.wrapping_neg()),
            Value::Float(x) => Value::Float(-x),
            v => v, // the parser only allows `-` on numbers
        }
    }

    /// `str(x)`, `int(x)`, `float(x)`, with the same rules and errors as the C runtime.
    pub fn convert(self, to: Type) -> Result<Value, String> {
        let bad = |s: &str| format!("Runtime error: cannot convert \"{}\" to {}", s, to.name());
        Ok(match (self, to) {
            (v, Type::Str) => Value::Str(v.to_string()),
            (Value::Float(x), Type::Int) => Value::Int(x as i64),
            (Value::Int(n), Type::Float) => Value::Float(n as f64),
            (Value::Str(s), Type::Int) => {
                let t = s.trim();
                match is_number(t, false).then(|| t.parse().ok()).flatten() {
                    Some(n) => Value::Int(n),
                    None => return Err(bad(&s)),
                }
            }
            (Value::Str(s), Type::Float) => {
                let t = s.trim();
                match is_number(t, true).then(|| t.parse().ok()).flatten() {
                    Some(x) => Value::Float(x),
                    None => return Err(bad(&s)),
                }
            }
            (v, _) => v, // same type
        })
    }

    /// `+ - * / mod`. int op int stays int (wrapping on overflow, like C in practice);
    /// any float makes the result float. Division by zero is a runtime error.
    pub fn arithmetic(&self, op: BinOp, other: &Value) -> Result<Value, String> {
        const DIV_ZERO: &str = "Runtime error: division by zero";
        if let (Value::Str(a), Value::Str(b)) = (self, other) {
            return Ok(Value::Str(format!("{}{}", a, b)));
        }
        if let (Value::Int(a), Value::Int(b)) = (self, other) {
            let (a, b) = (*a, *b);
            return Ok(Value::Int(match op {
                BinOp::Add => a.wrapping_add(b),
                BinOp::Sub => a.wrapping_sub(b),
                BinOp::Mul => a.wrapping_mul(b),
                BinOp::Div if b == 0 => return Err(DIV_ZERO.into()),
                BinOp::Div => a.wrapping_div(b),
                BinOp::Mod if b == 0 => return Err(DIV_ZERO.into()),
                _ => a.wrapping_rem(b),
            }));
        }
        let (a, b) = (self.as_f64(), other.as_f64());
        Ok(Value::Float(match op {
            BinOp::Add => a + b,
            BinOp::Sub => a - b,
            BinOp::Mul => a * b,
            _ if b == 0.0 => return Err(DIV_ZERO.into()),
            _ => a / b, // `mod` is int-only, so this is `/`
        }))
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

/// Accepted number text (after trimming spaces): `[+-]digits`, plus `.digits` for floats.
/// The C runtime uses exactly the same rule.
fn is_number(s: &str, allow_fraction: bool) -> bool {
    let s = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) if allow_fraction => (i, Some(f)),
        _ => (s, None),
    };
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    digits(int) && frac.is_none_or(digits)
}

/// Format a float like C's `printf("%g")`: 6 significant digits, trailing zeros
/// removed, scientific notation for very small or very large numbers.
pub fn format_g(x: f64) -> String {
    const PRECISION: i32 = 6;
    if x.is_nan() {
        return "nan".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.to_string();
    }
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

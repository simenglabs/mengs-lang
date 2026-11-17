use crate::ast::{CmpOp, Cond, Expr, Type};

use super::VAR_PREFIX;

/// C declaration for a variable or parameter, e.g. `long long v_age`.
/// `keep` values become `const` in C.
pub fn declare(ty: Type, keep: bool, name: &str) -> String {
    let qual = if keep { "const " } else { "" };
    match ty {
        // `const char *const` = a constant pointer to constant characters.
        Type::Str => format!("const char *{}{}{}", qual, VAR_PREFIX, name),
        Type::Int => format!("{}long long {}{}", qual, VAR_PREFIX, name),
        Type::Float => format!("{}double {}{}", qual, VAR_PREFIX, name),
        Type::Bool => format!("{}bool {}{}", qual, VAR_PREFIX, name),
    }
}

pub fn emit_condition(cond: &Cond) -> String {
    let (lhs, op, rhs) = match cond {
        Cond::Bool(e) => return emit_expr(e),
        Cond::Compare { lhs, op, rhs } => (lhs, op, rhs),
    };
    let op = match op {
        CmpOp::Eq => "==",
        CmpOp::Ne => "!=",
        CmpOp::Lt => "<",
        CmpOp::Gt => ">",
        CmpOp::Le => "<=",
        CmpOp::Ge => ">=",
    };
    let (l, r) = (emit_expr(lhs), emit_expr(rhs));
    match lhs.ty() {
        // Compare string contents (alphabetical order), not pointer addresses.
        Type::Str => format!("strcmp({}, {}) {} 0", l, r, op),
        Type::Int | Type::Float | Type::Bool => format!("{} {} {}", l, op, r),
    }
}

pub fn emit_expr(expr: &Expr) -> String {
    match expr {
        Expr::Str(s) => format!("\"{}\"", escape_c_string(s)),
        Expr::Int(n) => format!("{}LL", n),
        // `{:?}` always keeps a decimal point (2.0, not 2).
        Expr::Float(f) => format!("{:?}", f),
        Expr::Bool(b) => b.to_string(),
        Expr::Var { name, .. } => format!("{}{}", VAR_PREFIX, name),
    }
}

/// Escape text so it is safe inside a C string literal.
fn escape_c_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '?' => out.push_str("\\?"), // avoid C trigraphs (e.g. "??=")
            _ => out.push(c),
        }
    }
    out
}

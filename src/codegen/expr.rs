use crate::ast::{BinOp, Expr, Type, UnaryOp};

use super::{FN_PREFIX, VAR_PREFIX};

/// The C type for a Meng type (also used for return types).
pub fn c_type(ty: Type) -> &'static str {
    match ty {
        Type::Str => "const char *",
        Type::Int => "long long",
        Type::Float => "double",
        Type::Bool => "bool",
    }
}

/// C declaration for a variable or parameter, e.g. `long long v_age`.
/// `keep` values become `const` in C.
pub fn declare(ty: Type, keep: bool, name: &str) -> String {
    let qual = if keep { "const " } else { "" };
    match ty {
        // `const char *const` = a constant pointer to constant characters.
        Type::Str => format!("const char *{}{}{}", qual, VAR_PREFIX, name),
        _ => format!("{}{} {}{}", qual, c_type(ty), VAR_PREFIX, name),
    }
}

/// `f_Name(arg, ...)`. Default arguments were already filled in by the parser.
pub fn emit_call(name: &str, args: &[Expr]) -> String {
    let args: Vec<String> = args.iter().map(emit_top).collect();
    format!("{}{}({})", FN_PREFIX, name, args.join(", "))
}

/// Expression without its outermost parentheses, for `if (...)`, assignments and arguments.
pub fn emit_top(expr: &Expr) -> String {
    let s = emit_expr(expr);
    match expr {
        Expr::Binary { op, .. } if !matches!(op, BinOp::Div | BinOp::Mod) => s[1..s.len() - 1].to_string(),
        _ => s,
    }
}

/// Every operation is parenthesized, so C precedence never matters.
pub fn emit_expr(expr: &Expr) -> String {
    match expr {
        Expr::Str(s) => format!("\"{}\"", escape_c_string(s)),
        Expr::Int(n) => format!("{}LL", n),
        // `{:?}` always keeps a decimal point (2.0, not 2).
        Expr::Float(f) => format!("{:?}", f),
        Expr::Bool(b) => b.to_string(),
        Expr::Var { name, .. } => format!("{}{}", VAR_PREFIX, name),
        // The space avoids `--` when negating a negative literal.
        Expr::Unary { op: UnaryOp::Neg, expr } => format!("(- {})", emit_expr(expr)),
        Expr::Unary { op: UnaryOp::Not, expr } => format!("(!{})", emit_expr(expr)),
        Expr::Binary { op, lhs, rhs } => emit_binary(*op, lhs, rhs),
        Expr::Call { name, args, .. } => emit_call(name, args),
    }
}

fn emit_binary(op: BinOp, lhs: &Expr, rhs: &Expr) -> String {
    let (l, r) = (emit_expr(lhs), emit_expr(rhs));
    let float = lhs.ty() == Type::Float || rhs.ty() == Type::Float;
    let c_op = match op {
        // Division and remainder go through helpers that stop on division by zero.
        BinOp::Div if float => return format!("meng_div_float({}, {})", l, r),
        BinOp::Div => return format!("meng_div_int({}, {})", l, r),
        BinOp::Mod => return format!("meng_mod_int({}, {})", l, r),
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Le => "<=",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    };
    if lhs.ty() == Type::Str {
        // Compare string contents (alphabetical order), not pointer addresses.
        format!("(strcmp({}, {}) {} 0)", l, r, c_op)
    } else {
        format!("({} {} {})", l, c_op, r)
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

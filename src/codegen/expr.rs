use crate::ast::{BinOp, Expr, Type, UnaryOp};

use super::{FN_PREFIX, VAR_PREFIX};

/// C return type. A returned `str` is a heap string the caller owns.
pub fn c_return_type(ty: Option<Type>) -> &'static str {
    match ty {
        None => "void",
        Some(Type::Str) => "char *",
        Some(Type::Int) => "long long",
        Some(Type::Float) => "double",
        Some(Type::Bool) => "bool",
    }
}

/// `type name`, without a space after a pointer star (`char *name`).
pub fn c_decl(ty: &str, name: &str) -> String {
    if ty.ends_with('*') { format!("{}{}", ty, name) } else { format!("{} {}", ty, name) }
}

/// C declaration for a variable or parameter, e.g. `long long v_age`.
/// An owned `str` is a heap string (`char *`, freed by its owner); a borrowed
/// `str` only points at someone else's (`const char *`). `keep` adds `const`.
pub fn declare(ty: Type, keep: bool, borrow: bool, name: &str) -> String {
    let qual = if keep { "const " } else { "" };
    match ty {
        Type::Str if borrow => format!("const char *{}{}", VAR_PREFIX, name),
        Type::Str => format!("char *{}{}{}", qual, VAR_PREFIX, name),
        Type::Int => format!("{}long long {}{}", qual, VAR_PREFIX, name),
        Type::Float => format!("{}double {}{}", qual, VAR_PREFIX, name),
        Type::Bool => format!("{}bool {}{}", qual, VAR_PREFIX, name),
    }
}

/// A value to be *owned* by a variable, parameter, or return value.
/// For `str`: a moved variable hands over its pointer (and becomes NULL);
/// anything else is duplicated.
pub fn emit_owned(expr: &Expr) -> String {
    match expr {
        Expr::Var { name, ty: Type::Str, moved: true } => format!("meng_take(&{}{})", VAR_PREFIX, name),
        _ if expr.ty() == Type::Str => format!("meng_str_dup({})", emit_top(expr)),
        _ => emit_top(expr),
    }
}

/// `f_Name(arg, ...)`. Default arguments were already filled in by the parser.
/// Parameters own their values, so `str` arguments are moved or duplicated.
pub fn emit_call(name: &str, args: &[Expr]) -> String {
    let args: Vec<String> = args.iter().map(emit_owned).collect();
    format!("{}{}({})", FN_PREFIX, name, args.join(", "))
}

/// Whether evaluating the expression creates temporary heap strings, which
/// must be released after the statement.
pub fn has_temps(expr: &Expr) -> bool {
    match expr {
        Expr::Read { .. } => true,
        Expr::Call { ty: Type::Str, .. } => true,
        Expr::Convert { to: Type::Str, expr } if matches!(expr.ty(), Type::Int | Type::Float) => true,
        Expr::Binary { op: BinOp::Add, lhs, .. } if lhs.ty() == Type::Str => true,
        Expr::Str(_) | Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Var { .. } => false,
        Expr::Unary { expr, .. } | Expr::Convert { expr, .. } => has_temps(expr),
        Expr::Binary { lhs, rhs, .. } => has_temps(lhs) || has_temps(rhs),
        Expr::Call { args, .. } => args.iter().any(has_temps),
    }
}

/// Expression without its outermost parentheses, for `if (...)`, assignments and arguments.
pub fn emit_top(expr: &Expr) -> String {
    let s = emit_expr(expr);
    match expr {
        Expr::Binary { op, lhs, .. }
            if !matches!(op, BinOp::Div | BinOp::Mod) && !(*op == BinOp::Add && lhs.ty() == Type::Str) =>
        {
            s[1..s.len() - 1].to_string()
        }
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
        // A returned string is owned by the caller: it becomes a temporary.
        Expr::Call { name, args, ty: Type::Str } => format!("meng_temp({})", emit_call(name, args)),
        Expr::Call { name, args, .. } => emit_call(name, args),
        Expr::Convert { to, expr } => emit_convert(*to, expr),
        Expr::Read { prompt } => {
            let prompt = prompt.as_ref().map_or("\"\"".to_string(), |p| emit_top(p));
            format!("meng_read({})", prompt)
        }
    }
}

fn emit_convert(to: Type, expr: &Expr) -> String {
    let e = emit_top(expr);
    match (expr.ty(), to) {
        (from, to) if from == to => emit_expr(expr),
        (Type::Int, Type::Str) => format!("meng_int_to_str({})", e),
        (Type::Float, Type::Str) => format!("meng_float_to_str({})", e),
        (Type::Bool, Type::Str) => format!("({} ? \"true\" : \"false\")", emit_expr(expr)),
        (Type::Str, Type::Int) => format!("meng_str_to_int({})", e),
        (Type::Str, Type::Float) => format!("meng_str_to_float({})", e),
        (Type::Float, Type::Int) => format!("((long long)({}))", e),
        (_, _) => format!("((double)({}))", e), // int -> float
    }
}

fn emit_binary(op: BinOp, lhs: &Expr, rhs: &Expr) -> String {
    let (l, r) = (emit_expr(lhs), emit_expr(rhs));
    let float = lhs.ty() == Type::Float || rhs.ty() == Type::Float;
    let c_op = match op {
        BinOp::Add if lhs.ty() == Type::Str => return format!("meng_concat({}, {})", emit_top(lhs), emit_top(rhs)),
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

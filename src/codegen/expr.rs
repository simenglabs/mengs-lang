use crate::ast::{Function, Param, BinOp, Expr, Type, UnaryOp};

use super::{FN_PREFIX, VAR_PREFIX};

/// C return type. A returned `str` is a heap string the caller owns.
pub fn c_return_type(ty: Option<Type>) -> &'static str {
    match ty {
        None => "void",
        Some(Type::Str) => "char *",
        Some(Type::Int) => "long long",
        Some(Type::Float) => "double",
        Some(Type::Bool) => "bool",
        Some(Type::Array(_)) => "meng_array *",
    }
}

/// Runtime name and kind letter of an element type (`meng_push_int`, `meng_array_new('i')`).
pub fn elem_name(elem: Type) -> (&'static str, char) {
    match elem {
        Type::Int => ("int", 'i'),
        Type::Float => ("float", 'f'),
        Type::Bool => ("bool", 'b'),
        _ => ("str", 's'),
    }
}

/// Element value as the array stores it: strings are owned by the array.
pub fn emit_item(fns: &[Function], elem: Type, value: &Expr) -> String {
    if elem == Type::Str { emit_owned(fns, value) } else { emit_top(fns, value) }
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
        Type::Array(_) if borrow => format!("const meng_array *{}{}", VAR_PREFIX, name),
        Type::Array(_) => format!("meng_array *{}{}{}", qual, VAR_PREFIX, name),
    }
}

/// A value to be *owned* by a variable, parameter, array item, or return value.
/// For `str` and arrays: a moved variable hands over its pointer (and becomes NULL),
/// a fresh value (array literal, returned value) is used as is, anything else is copied.
pub fn emit_owned(fns: &[Function], expr: &Expr) -> String {
    match expr {
        Expr::Var { name, ty: Type::Str, moved: true } => format!("meng_take(&{}{})", VAR_PREFIX, name),
        Expr::Var { name, ty: Type::Array(_), moved: true } => format!("meng_take_array(&{}{})", VAR_PREFIX, name),
        Expr::Call { name, args, ty } if ty.is_heap() => emit_call(fns, name, args),
        Expr::Array { items, ty } => emit_array(fns, items, *ty),
        _ if expr.ty() == Type::Str => format!("meng_str_dup({})", emit_top(fns, expr)),
        _ if expr.ty().elem().is_some() => format!("meng_array_copy({})", emit_top(fns, expr)),
        _ => emit_top(fns, expr),
    }
}

/// A new array: `meng_push_int(meng_push_int(meng_array_new('i'), 1LL), 2LL)`.
fn emit_array(fns: &[Function], items: &[Expr], ty: Type) -> String {
    let elem = ty.elem().expect("an array type");
    let (name, kind) = elem_name(elem);
    items.iter().fold(format!("meng_array_new('{}')", kind), |acc, item| {
        format!("meng_push_{}({}, {})", name, acc, emit_item(fns, elem, item))
    })
}

/// Whether a parameter only borrows its argument (`keep` on a `str` or an array).
pub fn is_borrowed(p: &Param) -> bool {
    p.keep && p.ty.is_heap()
}

/// `f_Name(arg, ...)`. Default arguments were already filled in by the parser.
/// Parameters own their values, so `str` arguments are moved or duplicated;
/// a `keep` parameter only borrows, so its argument is passed as is.
pub fn emit_call(fns: &[Function], name: &str, args: &[Expr]) -> String {
    let params = fns.iter().find(|f| f.name == name).map(|f| f.params.as_slice()).unwrap_or(&[]);
    let args: Vec<String> = args
        .iter()
        .enumerate()
        .map(|(i, arg)| if params.get(i).is_some_and(is_borrowed) { emit_top(fns, arg) } else { emit_owned(fns, arg) })
        .collect();
    format!("{}{}({})", FN_PREFIX, name, args.join(", "))
}

/// Whether evaluating the expression creates temporary heap strings, which
/// must be released after the statement.
pub fn has_temps(expr: &Expr) -> bool {
    match expr {
        Expr::Read { .. } | Expr::Array { .. } => true,
        Expr::Call { ty, .. } if ty.is_heap() => true,
        Expr::Convert { to: Type::Str, expr } if matches!(expr.ty(), Type::Int | Type::Float) => true,
        Expr::Binary { op: BinOp::Add, lhs, .. } if lhs.ty() == Type::Str => true,
        Expr::Str(_) | Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Var { .. } => false,
        Expr::Unary { expr, .. } | Expr::Convert { expr, .. } | Expr::Len(expr) => has_temps(expr),
        Expr::Index { array, index, .. } => has_temps(array) || has_temps(index),
        Expr::Binary { lhs, rhs, .. } => has_temps(lhs) || has_temps(rhs),
        Expr::Call { args, .. } => args.iter().any(has_temps),
    }
}

/// Expression without its outermost parentheses, for `if (...)`, assignments and arguments.
pub fn emit_top(fns: &[Function], expr: &Expr) -> String {
    let s = emit_expr(fns, expr);
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
pub fn emit_expr(fns: &[Function], expr: &Expr) -> String {
    match expr {
        Expr::Str(s) => format!("\"{}\"", escape_c_string(s)),
        Expr::Int(n) => format!("{}LL", n),
        // `{:?}` always keeps a decimal point (2.0, not 2).
        Expr::Float(f) => format!("{:?}", f),
        Expr::Bool(b) => b.to_string(),
        Expr::Var { name, .. } => format!("{}{}", VAR_PREFIX, name),
        // The space avoids `--` when negating a negative literal.
        Expr::Unary { op: UnaryOp::Neg, expr } => format!("(- {})", emit_expr(fns, expr)),
        Expr::Unary { op: UnaryOp::Not, expr } => format!("(!{})", emit_expr(fns, expr)),
        Expr::Binary { op, lhs, rhs } => emit_binary(fns, *op, lhs, rhs),
        // A returned string is owned by the caller: it becomes a temporary.
        Expr::Call { name, args, ty: Type::Str } => format!("meng_temp({})", emit_call(fns, name, args)),
        Expr::Call { name, args, ty: Type::Array(_) } => format!("meng_temp_array({})", emit_call(fns, name, args)),
        Expr::Array { items, ty } => format!("meng_temp_array({})", emit_array(fns, items, *ty)),
        Expr::Index { array, index, ty } => {
            format!("meng_get_{}({}, {})", elem_name(*ty).0, emit_top(fns, array), emit_top(fns, index))
        }
        Expr::Len(expr) if expr.ty() == Type::Str => format!("((long long)strlen({}))", emit_top(fns, expr)),
        Expr::Len(expr) => format!("meng_array_len({})", emit_top(fns, expr)),
        Expr::Call { name, args, .. } => emit_call(fns, name, args),
        Expr::Convert { to, expr } => emit_convert(fns, *to, expr),
        Expr::Read { prompt } => {
            let prompt = prompt.as_ref().map_or("\"\"".to_string(), |p| emit_top(fns, p));
            format!("meng_read({})", prompt)
        }
    }
}

fn emit_convert(fns: &[Function], to: Type, expr: &Expr) -> String {
    let e = emit_top(fns, expr);
    match (expr.ty(), to) {
        (from, to) if from == to => emit_expr(fns, expr),
        (Type::Int, Type::Str) => format!("meng_int_to_str({})", e),
        (Type::Float, Type::Str) => format!("meng_float_to_str({})", e),
        (Type::Bool, Type::Str) => format!("({} ? \"true\" : \"false\")", emit_expr(fns, expr)),
        (Type::Str, Type::Int) => format!("meng_str_to_int({})", e),
        (Type::Str, Type::Float) => format!("meng_str_to_float({})", e),
        (Type::Float, Type::Int) => format!("((long long)({}))", e),
        (_, _) => format!("((double)({}))", e), // int -> float
    }
}

fn emit_binary(fns: &[Function], op: BinOp, lhs: &Expr, rhs: &Expr) -> String {
    let (l, r) = (emit_expr(fns, lhs), emit_expr(fns, rhs));
    let float = lhs.ty() == Type::Float || rhs.ty() == Type::Float;
    let c_op = match op {
        BinOp::Add if lhs.ty() == Type::Str => return format!("meng_concat({}, {})", emit_top(fns, lhs), emit_top(fns, rhs)),
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

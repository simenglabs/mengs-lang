use crate::ast::{CmpOp, Cond, Expr, Type};

use super::scope::{Access, Scope};

const KEYWORDS: &[&str] = &["set", "keep", "reuse", "show", "fnc", "if", "else", "true", "false"];

/// Two-character operators must come before their one-character prefixes.
const CMP_OPS: &[(&str, CmpOp)] = &[
    ("<=", CmpOp::Le),
    (">=", CmpOp::Ge),
    ("!=", CmpOp::Ne),
    ("=", CmpOp::Eq),
    ("<", CmpOp::Lt),
    (">", CmpOp::Gt),
];

/// Value: `"text"`, `42`, `3.14`, `true`/`false`, or a variable name.
/// `access` says whether a variable is only read or moved out.
pub fn parse_expr(arg: &str, scope: &mut Scope, line: usize, access: Access) -> Result<Expr, String> {
    if arg.starts_with('"') {
        return parse_string_literal(arg).map(Expr::Str);
    }
    match arg {
        "true" => return Ok(Expr::Bool(true)),
        "false" => return Ok(Expr::Bool(false)),
        _ => {}
    }
    if arg.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
        let parsed = if arg.contains('.') {
            arg.parse().ok().map(Expr::Float)
        } else {
            arg.parse().ok().map(Expr::Int)
        };
        return parsed.ok_or_else(|| format!("invalid number: `{}`", arg));
    }
    let name = parse_identifier(arg).map_err(|_| {
        format!("invalid value `{}` (expected \"text\", a number, true/false, or a variable name)", arg)
    })?;
    let ty = scope.access(&name, line, access)?;
    Ok(Expr::Var { name, ty })
}

/// `if` condition: `a OP b ->` or `bool_value ->` (after `if ` is stripped).
pub fn parse_condition(cond: &str, scope: &mut Scope, line: usize) -> Result<Cond, String> {
    let usage = "invalid if syntax, example: if a < b -> or if active ->";
    let cond = cond.strip_suffix("->").ok_or(usage)?.trim();

    let Some((pos, op_str, op)) = find_operator(cond) else {
        let value = parse_expr(cond, scope, line, Access::Read)?;
        if value.ty() != Type::Bool {
            return Err(format!("a condition without an operator must be bool, got {}", value.ty().name()));
        }
        return Ok(Cond::Bool(value));
    };

    let lhs = parse_expr(cond[..pos].trim(), scope, line, Access::Read)?;
    let rhs = parse_expr(cond[pos + op_str.len()..].trim(), scope, line, Access::Read)?;
    let (lt, rt) = (lhs.ty(), rhs.ty());
    if lt != rt && !(lt.is_numeric() && rt.is_numeric()) {
        return Err(format!("cannot compare {} with {}", lt.name(), rt.name()));
    }
    if lt == Type::Bool && !matches!(op, CmpOp::Eq | CmpOp::Ne) {
        return Err("bool values can only be compared with `=` or `!=`".into());
    }
    Ok(Cond::Compare { lhs, op, rhs })
}

/// Find the first comparison operator outside of string literals.
fn find_operator(s: &str) -> Option<(usize, &'static str, CmpOp)> {
    let (mut in_str, mut escaped) = (false, false);
    for (i, c) in s.char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
        } else if c == '"' {
            in_str = true;
        } else if let Some(&(op_str, op)) = CMP_OPS.iter().find(|(o, _)| s[i..].starts_with(o)) {
            return Some((i, op_str, op));
        }
    }
    None
}

/// Split an argument list on commas, ignoring commas inside strings.
pub fn split_args(s: &str) -> Result<Vec<&str>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Vec::new());
    }
    let mut parts = Vec::new();
    let (mut start, mut in_str, mut escaped) = (0, false, false);
    for (i, c) in s.char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
        } else if c == '"' {
            in_str = true;
        } else if c == ',' {
            parts.push(s[start..i].trim());
            start = i + 1;
        }
    }
    parts.push(s[start..].trim());
    if parts.iter().any(|p| p.is_empty()) {
        return Err(format!("empty argument in `({})`", s));
    }
    Ok(parts)
}

pub fn parse_identifier(s: &str) -> Result<String, String> {
    let mut chars = s.chars();
    let valid_start = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    let valid_rest = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid_start || !valid_rest {
        return Err(format!("invalid name: `{}`", s));
    }
    if KEYWORDS.contains(&s) {
        return Err(format!("`{}` is a keyword and cannot be used as a name", s));
    }
    Ok(s.to_string())
}

/// Parse a quoted string literal and process escape sequences
/// (`\"`, `\\`, `\n`, `\t`, `\r`).
fn parse_string_literal(arg: &str) -> Result<String, String> {
    let body = arg
        .strip_prefix('"')
        .ok_or_else(|| format!("a string must start with a quote (got `{}`)", arg))?;

    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                let rest = chars.as_str();
                if !rest.is_empty() {
                    return Err(format!("unexpected text after string: `{}`", rest));
                }
                return Ok(out);
            }
            '\\' => out.push(match chars.next() {
                Some('"') => '"',
                Some('\\') => '\\',
                Some('n') => '\n',
                Some('t') => '\t',
                Some('r') => '\r',
                Some(other) => return Err(format!("unknown escape sequence: `\\{}`", other)),
                None => break,
            }),
            _ => out.push(c),
        }
    }
    Err("string is not closed with a quote `\"`".to_string())
}

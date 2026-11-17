use crate::ast::{Expr, Param, Stmt, Type};

use super::expr::{parse_expr, parse_identifier, split_args};
use super::scope::{Access, Scope};

/// Optional prefix before a declared name.
#[derive(Clone, Copy, PartialEq)]
enum Modifier {
    None,
    /// Constant: never moved, `const` in C.
    Keep,
    /// Reusable: never moved.
    Reuse,
}

/// Parse a single-line statement and declare any new variable in `scope`.
/// Returns `Ok(None)` for empty lines and an error for anything unrecognized.
pub fn parse_stmt(line: &str, scope: &mut Scope, no: usize) -> Result<Option<Stmt>, String> {
    // Print: show(expr);
    if let Some(arg) = line.strip_prefix("show(").and_then(|s| s.strip_suffix(");")) {
        return parse_expr(arg.trim(), scope, no, Access::Read).map(|e| Some(Stmt::Print(e)));
    }
    // Variable: set [keep|reuse] name: type = expr;   or borrow: set name: type <== var;
    if let Some(rest) = line.strip_prefix("set ").and_then(|s| s.strip_suffix(';')) {
        return parse_set(rest, scope, no).map(Some);
    }
    // Copy: source ()=> new_name;
    if let Some((src, dst)) = line.strip_suffix(';').and_then(|s| s.split_once("()=>")) {
        return parse_copy(src.trim(), dst.trim(), scope, no).map(Some);
    }
    // Function call: Name(expr, expr);  — arguments are moved into the function.
    if let Some((name, args)) = line.strip_suffix(");").and_then(|s| s.split_once('(')) {
        if let Ok(name) = parse_identifier(name.trim()) {
            let args = split_args(args)?
                .into_iter()
                .map(|a| parse_expr(a, scope, no, Access::Move))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(Some(Stmt::Call { name, args }));
        }
    }
    if line.is_empty() {
        return Ok(None);
    }
    let hint = if line.ends_with(';') { "" } else { " (missing `;`?)" };
    Err(format!("unrecognized statement: `{}`{}", line, hint))
}

/// `[keep|reuse] name: type = expr` or `name: type <== var` (after `set `, without `;`).
fn parse_set(rest: &str, scope: &mut Scope, no: usize) -> Result<Stmt, String> {
    let example = "set age: int = 20;";
    let pos = rest
        .find('=')
        .ok_or_else(|| format!("invalid set syntax, example: {}", example))?;

    // `<==` is a borrow; the first `=` then sits right after a `<`.
    if rest[..pos].ends_with('<') && rest[pos..].starts_with("==") {
        return parse_borrow(&rest[..pos - 1], rest[pos + 2..].trim(), scope, no);
    }

    let (name, ty, modifier) = parse_typed_name(&rest[..pos], example)?;
    let value = parse_expr(rest[pos + 1..].trim(), scope, no, Access::Move)?;
    check_assign(ty, &value, &format!("variable `{}`", name))?;

    // Assigning from a borrow copies the reference: the new variable borrows the same owner.
    match &value {
        Expr::Var { name: src, .. } if modifier == Modifier::None && scope.borrow_root(src).is_some() => {
            scope.declare_borrow(&name, ty, src)?
        }
        _ => scope.declare(&name, ty, modifier != Modifier::None)?,
    }
    Ok(Stmt::Set { name, ty, keep: modifier == Modifier::Keep, value })
}

/// `set name: type <== owner;` — `name` refers to `owner` without taking it.
fn parse_borrow(decl: &str, owner: &str, scope: &mut Scope, no: usize) -> Result<Stmt, String> {
    let (name, ty, modifier) = parse_typed_name(decl, "set alias: str <== name;")?;
    if modifier != Modifier::None {
        return Err("a borrowed variable cannot be declared with `keep` or `reuse`".into());
    }
    let owner = parse_identifier(owner).map_err(|_| {
        format!("`<==` can only borrow from a variable (got `{}`)", owner)
    })?;
    let owner_ty = scope.access(&owner, no, Access::Read)?;
    if owner_ty != ty {
        return Err(format!(
            "`{}` is declared as {} but borrows `{}` of type {}",
            name,
            ty.name(),
            owner,
            owner_ty.name()
        ));
    }
    scope.declare_borrow(&name, ty, &owner)?;
    let value = Expr::Var { name: owner, ty };
    Ok(Stmt::Set { name, ty, keep: false, value })
}

/// `source ()=> new_name;` — `new_name` is an independent copy; `source` stays usable.
fn parse_copy(src: &str, dst: &str, scope: &mut Scope, no: usize) -> Result<Stmt, String> {
    let src = parse_identifier(src)
        .map_err(|_| format!("`()=>` can only copy a variable (got `{}`)", src))?;
    let dst = parse_identifier(dst)?;
    let ty = scope.access(&src, no, Access::Read)?;
    scope.declare(&dst, ty, false)?;
    Ok(Stmt::Set { name: dst, ty, keep: false, value: Expr::Var { name: src, ty } })
}

/// Function header: `Name(a: str, b: int = 0)->` (after `fnc ` is stripped).
pub fn parse_fn_header(header: &str) -> Result<(String, Vec<Param>), String> {
    let usage = "invalid function syntax, example: fnc Name(a: str, b: int = 0)->";
    let sig = header.strip_suffix("->").ok_or(usage)?.trim();
    let (name, params) = sig
        .strip_suffix(')')
        .and_then(|s| s.split_once('('))
        .ok_or(usage)?;

    let name = parse_identifier(name.trim())?;
    let mut parsed: Vec<Param> = Vec::new();
    for p in split_args(params)? {
        let param = parse_param(p)?;
        if parsed.iter().any(|q| q.name == param.name) {
            return Err(format!("parameter `{}` is declared twice", param.name));
        }
        if param.default.is_none() && parsed.iter().any(|q| q.default.is_some()) {
            return Err(format!(
                "parameter `{}` has no default, so it must come before parameters with defaults",
                param.name
            ));
        }
        parsed.push(param);
    }
    Ok((name, parsed))
}

/// Parameter: `[keep|reuse] name: type` or `[keep|reuse] name: type = literal`.
fn parse_param(p: &str) -> Result<Param, String> {
    let (decl, default) = match p.split_once('=') {
        Some((decl, default)) => (decl, Some(default.trim())),
        None => (p, None),
    };
    let (name, ty, modifier) = parse_typed_name(decl, "fnc Name(a: str, b: int = 0)->")?;

    let default = match default {
        None => None,
        Some(d) => {
            let value = parse_expr(d, &mut Scope::default(), 0, Access::Read)
                .ok()
                .filter(Expr::is_literal);
            let value = value.ok_or_else(|| {
                format!("default for parameter `{}` must be a literal value (got `{}`)", name, d)
            })?;
            check_assign(ty, &value, &format!("default for parameter `{}`", name))?;
            Some(value)
        }
    };
    Ok(Param {
        name,
        ty,
        keep: modifier == Modifier::Keep,
        reuse: modifier == Modifier::Reuse,
        default,
    })
}

/// Typed declaration: `[keep|reuse] name: type` (used by `set` and function parameters).
fn parse_typed_name(s: &str, example: &str) -> Result<(String, Type, Modifier), String> {
    let s = s.trim();
    let (s, modifier) = if let Some(rest) = s.strip_prefix("keep ") {
        (rest.trim(), Modifier::Keep)
    } else if let Some(rest) = s.strip_prefix("reuse ") {
        (rest.trim(), Modifier::Reuse)
    } else {
        (s, Modifier::None)
    };
    let (name, ty) = s
        .split_once(':')
        .ok_or_else(|| format!("a type is required for `{}`, example: {}", s, example))?;
    let name = parse_identifier(name.trim())?;
    let ty = Type::parse(ty.trim()).ok_or_else(|| {
        format!("unknown type `{}` (expected one of: str, int, float, bool)", ty.trim())
    })?;
    Ok((name, ty, modifier))
}

/// Make sure a value can be stored in the target type.
fn check_assign(target: Type, value: &Expr, what: &str) -> Result<(), String> {
    if target.accepts(value.ty()) {
        Ok(())
    } else {
        Err(format!("{} has type {}, but got a {} value", what, target.name(), value.ty().name()))
    }
}

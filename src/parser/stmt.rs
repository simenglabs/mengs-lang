use crate::ast::{Expr, Param, Signature, Stmt, Type};

use super::expr::{parse_call, parse_expr, parse_identifier, parse_push, parse_typed, split_args};
use super::scope::{Access, Kind, Scope};

/// Parse a single-line statement and declare any new variable in `scope`.
/// (`if`, `while`, and `return` are handled by the block parser.)
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
    // Reassign: name = expr;   or an array item: name[index] = expr;
    if let Some((target, value)) = line.strip_suffix(';').and_then(|s| s.split_once('=')) {
        let target = target.trim();
        if is_word(target) && !value.starts_with('=') {
            return parse_assign(target, value.trim(), scope, no).map(Some);
        }
        if let Some((name, index)) = target.strip_suffix(']').and_then(|t| t.split_once('[')) {
            if is_word(name.trim()) && !value.starts_with('=') {
                return parse_set_index(name.trim(), index, value.trim(), scope, no).map(Some);
            }
        }
    }
    // Append to an array: push(xs, value);
    if line.starts_with("push(") && line.ends_with(");") {
        let (name, value, elem) = parse_push(&line[..line.len() - 1], scope, no)?;
        return Ok(Some(Stmt::Push { name, value, elem }));
    }
    // Function call: Name(expr, expr);  — bare variables are moved into the function.
    if let Some((name, _)) = line.strip_suffix(");").and_then(|s| s.split_once('(')) {
        if is_word(name.trim()) {
            let (name, args) = parse_call(&line[..line.len() - 1], scope, no)?;
            return Ok(Some(Stmt::Call { name, args }));
        }
    }
    if line.is_empty() {
        return Ok(None);
    }
    let hint = if line.ends_with(';') { "" } else { " (missing `;`?)" };
    Err(format!("unrecognized statement: `{}`{}", line, hint))
}

/// Looks like a name (keywords included, so they get a clear error later).
fn is_word(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
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

    let (name, ty, kind) = parse_typed_name(&rest[..pos], example)?;
    let value = parse_typed(rest[pos + 1..].trim(), scope, no, Access::Move, ty)?;
    check_assign(ty, &value, &format!("variable `{}`", name))?;

    // Assigning from a borrow copies the reference: the new variable borrows the same owner.
    match &value {
        Expr::Var { name: src, .. } if kind == Kind::Owned && scope.borrow_root(src).is_some() => {
            scope.declare_borrow(&name, ty, src)?
        }
        _ => scope.declare(&name, ty, kind)?,
    }
    let borrow = scope.borrow_root(&name).is_some();
    Ok(Stmt::Set { name, ty, keep: kind == Kind::Keep, borrow, value })
}

/// `name = expr` (without `;`): give an existing variable a new value.
fn parse_assign(name: &str, value: &str, scope: &mut Scope, no: usize) -> Result<Stmt, String> {
    let name = parse_identifier(name)?;
    let expected = scope.type_of(&name).ok_or_else(|| format!("variable `{}` is not declared", name))?;
    // The value is evaluated first, so `x = x + 1` reads the old `x`.
    let value = parse_typed(value, scope, no, Access::Move, expected)?;
    let ty = scope.reassign(&name, no)?;
    check_assign(ty, &value, &format!("variable `{}`", name))?;
    Ok(Stmt::Assign { name, value })
}

/// `name[index] = value` (without `;`): change one item of an array.
fn parse_set_index(name: &str, index: &str, value: &str, scope: &mut Scope, no: usize) -> Result<Stmt, String> {
    let name = parse_identifier(name)?;
    let index = parse_expr(index, scope, no, Access::Read)?;
    if index.ty() != Type::Int {
        return Err(format!("an array index must be int, got {}", index.ty().name()));
    }
    let value = parse_expr(value, scope, no, Access::Move)?;
    let ty = scope.mutate(&name, no)?;
    let elem = ty.elem().ok_or_else(|| format!("only arrays can be indexed, but `{}` is {}", name, ty.with_article()))?;
    check_assign(elem, &value, &format!("an item of `{}`", name))?;
    Ok(Stmt::SetIndex { name, index, value, elem })
}

/// `set name: type <== owner;` — `name` refers to `owner` without taking it.
fn parse_borrow(decl: &str, owner: &str, scope: &mut Scope, no: usize) -> Result<Stmt, String> {
    let (name, ty, kind) = parse_typed_name(decl, "set alias: str <== name;")?;
    if kind != Kind::Owned {
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
    let value = Expr::Var { name: owner, ty, moved: false };
    Ok(Stmt::Set { name, ty, keep: false, borrow: true, value })
}

/// `source ()=> new_name;` — `new_name` is an independent copy; `source` stays usable.
fn parse_copy(src: &str, dst: &str, scope: &mut Scope, no: usize) -> Result<Stmt, String> {
    let src = parse_identifier(src)
        .map_err(|_| format!("`()=>` can only copy a variable (got `{}`)", src))?;
    let dst = parse_identifier(dst)?;
    let ty = scope.access(&src, no, Access::Read)?;
    scope.declare(&dst, ty, Kind::Owned)?;
    let value = Expr::Var { name: src, ty, moved: false };
    Ok(Stmt::Set { name: dst, ty, keep: false, borrow: false, value })
}

/// Function header (after `fnc `): `Name(a: str, b: int = 0)->` or `Name(a: int): int ->`.
pub fn parse_fn_header(header: &str) -> Result<Signature, String> {
    let usage = "invalid function syntax, example: fnc Name(a: str, b: int = 0)-> or fnc Add(a: int, b: int): int ->";
    let sig = header.strip_suffix("->").ok_or(usage)?.trim();
    let close = sig.rfind(')').ok_or(usage)?;
    let (name, params) = sig[..close].split_once('(').ok_or(usage)?;

    let ret = match sig[close + 1..].trim() {
        "" => None,
        after => {
            let ty = after.strip_prefix(':').ok_or(usage)?.trim();
            Some(parse_type(ty)?)
        }
    };

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
    Ok(Signature { name, params: parsed, ret })
}

/// Parameter: `[keep|reuse] name: type` or `[keep|reuse] name: type = literal`.
fn parse_param(p: &str) -> Result<Param, String> {
    let (decl, default) = match p.split_once('=') {
        Some((decl, default)) => (decl, Some(default.trim())),
        None => (p, None),
    };
    let (name, ty, kind) = parse_typed_name(decl, "fnc Name(a: str, b: int = 0)->")?;

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
    Ok(Param { name, ty, keep: kind == Kind::Keep, reuse: kind == Kind::Reuse, default })
}

/// Typed declaration: `[keep|reuse] name: type` (used by `set` and function parameters).
fn parse_typed_name(s: &str, example: &str) -> Result<(String, Type, Kind), String> {
    let s = s.trim();
    let (s, kind) = if let Some(rest) = s.strip_prefix("keep ") {
        (rest.trim(), Kind::Keep)
    } else if let Some(rest) = s.strip_prefix("reuse ") {
        (rest.trim(), Kind::Reuse)
    } else {
        (s, Kind::Owned)
    };
    let (name, ty) = s
        .split_once(':')
        .ok_or_else(|| format!("a type is required for `{}`, example: {}", s, example))?;
    let name = parse_identifier(name.trim())?;
    Ok((name, parse_type(ty.trim())?, kind))
}

fn parse_type(ty: &str) -> Result<Type, String> {
    Type::parse(ty).ok_or_else(|| format!("unknown type `{}` (expected str, int, float, bool, or an array of one of them like [int])", ty))
}

/// Make sure a value can be stored in the target type.
pub fn check_assign(target: Type, value: &Expr, what: &str) -> Result<(), String> {
    if target.accepts(value.ty()) {
        Ok(())
    } else {
        Err(format!("{} has type {}, but got {} value", what, target.name(), value.ty().with_article()))
    }
}

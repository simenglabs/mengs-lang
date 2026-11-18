//! Simple statements (`set`, reassign, item assignment, borrow, copy, `show`,
//! `push`, calls) and function headers, parsed from the tokens of one statement.

use crate::ast::{Expr, Param, Signature, Stmt, Type};

use super::diagnostic::{Diagnostic, Source, Statement};
use super::expr::{ExprParser, parse_identifier};
use super::lexer::Token;
use super::scope::{Access, Kind, Scope};

type Result<T> = std::result::Result<T, Diagnostic>;

/// Parse a simple statement and declare any new variable in `scope`.
/// (`if`, `while`, and `return` are handled by the block parser.)
pub fn parse_stmt(src: &Source, stmt: Statement, scope: &mut Scope) -> Result<Stmt> {
    let mut p = ExprParser::new(src, stmt, scope);
    let named = matches!(p.peek(), Some(Token::Ident(_)));
    // Print: show(expr);
    if p.is("show") && p.is_at(1, "(") {
        return parse_show(&mut p);
    }
    // Variable: set [keep|reuse] name: type = expr;   or borrow: set name: type <== var;
    if p.is("set") {
        return parse_set(&mut p);
    }
    // Copy: source ()=> new_name;
    if let Some(arrow) = (stmt.start..stmt.end).find(|&i| src.is(i, "()=>")) {
        return parse_copy(&mut p, arrow);
    }
    // Reassign: name = expr;
    if named && p.is_at(1, "=") {
        return parse_assign(&mut p);
    }
    // An array item: name[index] = expr;
    if named && p.is_at(1, "[") && is_item_assignment(src, stmt) {
        return parse_set_index(&mut p);
    }
    // Append to an array: push(xs, value);
    if p.is("push") && p.is_at(1, "(") {
        let (name, value, elem) = p.push()?;
        return Ok(Stmt::Push { name, value, elem });
    }
    // Function call: Name(expr, expr);  — bare variables are moved into the function.
    if named && p.is_at(1, "(") {
        let (name, args) = p.call_statement()?;
        return Ok(Stmt::Call { name, args });
    }
    let hint = if src.is(stmt.end - 1, ";") { "" } else { " (missing `;`?)" };
    Err(src.error(stmt.start, format!("unrecognized statement: `{}`{}", src.text(stmt.start, stmt.end), hint)))
}

/// A statement that ends without `;` (at the end of the file, or at a line
/// break where the next line starts a new statement). Reported just after its
/// last token, where the `;` is missing.
pub fn incomplete(src: &Source, stmt: Statement) -> Option<Diagnostic> {
    let last = stmt.end - 1;
    if src.is(last, ";") || src.is(last, "->") {
        return None;
    }
    let message = format!("unrecognized statement: `{}` (missing `;`?)", src.text(stmt.start, stmt.end));
    Some(src.error_after(last, message))
}

/// Token of the variable a `set` or a copy (`a ()=> b;`) declares.
pub fn declared_name(src: &Source, stmt: Statement) -> usize {
    if !src.is(stmt.start, "set") {
        return stmt.end - 2; // `source ()=> name;`
    }
    let name = stmt.start + 1;
    if src.is(name, "keep") || src.is(name, "reuse") { name + 1 } else { name }
}

/// After a `set` failed (e.g. a type error in its value), still declare the
/// variable if its name and type are readable, to avoid follow-up errors.
pub fn declare_after_error(src: &Source, stmt: Statement, scope: &mut Scope) {
    if !src.is(stmt.start, "set") {
        return;
    }
    let mut p = ExprParser::new(src, stmt, scope);
    p.pos = declared_name(src, stmt);
    let Ok((_, name)) = p.name() else { return };
    if !p.eat(":") {
        return;
    }
    if let Ok(ty) = p.type_until(&["=", "<=="]) {
        let _ = p.scope.declare(&name, ty, Kind::Reuse);
    }
}

/// `name[...] = ...`: the `]` matching the `[` after the name is followed by `=`.
fn is_item_assignment(src: &Source, stmt: Statement) -> bool {
    let mut depth = 0;
    for i in stmt.start + 1..stmt.end {
        match src.token(i) {
            Some(Token::Sym("[")) => depth += 1,
            Some(Token::Sym("]")) => {
                depth -= 1;
                if depth == 0 {
                    return src.is(i + 1, "=") && i + 1 < stmt.end;
                }
            }
            _ => {}
        }
    }
    false
}

/// `show(expr);`
fn parse_show(p: &mut ExprParser) -> Result<Stmt> {
    p.pos += 2; // `show` `(`
    let expr = p.or()?;
    if !p.eat(")") {
        return Err(if p.is(";") { p.error("missing `)`") } else { p.unexpected() });
    }
    p.finish()?;
    Ok(Stmt::Print(expr))
}

/// `set [keep|reuse] name: type = expr;` or `set name: type <== var;`
fn parse_set(p: &mut ExprParser) -> Result<Stmt> {
    let example = "set age: int = 20;";
    p.pos += 1; // `set`
    let kind = declaration_kind(p);
    let decl = p.pos;
    let (name_at, name) = p.name()?;
    if !p.eat(":") {
        let to = (decl..p.end).find(|&i| ["=", "<==", ";"].iter().any(|s| p.src.is(i, s))).unwrap_or(p.end);
        let message = format!("a type is required for `{}`, example: {}", p.text(decl, to), example);
        return Err(p.error_at(name_at, message));
    }
    let ty = p.type_until(&["=", "<=="])?;
    if p.eat("<==") {
        return parse_borrow(p, kind, name_at, name, ty);
    }
    if !p.eat("=") {
        return Err(p.error(format!("invalid set syntax, example: {}", example)));
    }
    let value = p.value(Access::Move, Some(ty))?;
    p.finish()?;
    check_assign(ty, &value, &format!("variable `{}`", name)).map_err(|e| p.error_at(name_at, e))?;

    // Assigning from a borrow copies the reference: the new variable borrows the same owner.
    let declared = match &value {
        Expr::Var { name: src, .. } if kind == Kind::Owned && p.scope.borrow_root(src).is_some() => {
            p.scope.declare_borrow(&name, ty, src)
        }
        _ => p.scope.declare(&name, ty, kind),
    };
    declared.map_err(|e| p.error_at(name_at, e))?;
    let borrow = p.scope.borrow_root(&name).is_some();
    Ok(Stmt::Set { name, ty, keep: kind == Kind::Keep, borrow, value })
}

/// An optional `keep` or `reuse` before a declared name.
fn declaration_kind(p: &mut ExprParser) -> Kind {
    if p.eat("keep") {
        Kind::Keep
    } else if p.eat("reuse") {
        Kind::Reuse
    } else {
        Kind::Owned
    }
}

/// `name = expr;`: give an existing variable a new value.
fn parse_assign(p: &mut ExprParser) -> Result<Stmt> {
    let (name_at, name) = p.name()?;
    p.pos += 1; // `=`
    let Some(expected) = p.scope.type_of(&name) else {
        return Err(p.error_at(name_at, format!("variable `{}` is not declared", name)));
    };
    // The value is evaluated first, so `x = x + 1` reads the old `x`.
    let value = p.value(Access::Move, Some(expected))?;
    p.finish()?;
    let ty = p.scope.reassign(&name, p.line(name_at)).map_err(|e| p.error_at(name_at, e))?;
    check_assign(ty, &value, &format!("variable `{}`", name)).map_err(|e| p.error_at(name_at, e))?;
    Ok(Stmt::Assign { name, value })
}

/// `name[index] = value;`: change one item of an array.
fn parse_set_index(p: &mut ExprParser) -> Result<Stmt> {
    let (name_at, name) = p.name()?;
    p.pos += 1; // `[`
    let index_at = p.pos;
    let index = p.or()?;
    if !p.eat("]") || !p.eat("=") {
        return Err(p.unexpected());
    }
    if index.ty() != Type::Int {
        return Err(p.error_at(index_at, format!("an array index must be int, got {}", index.ty().name())));
    }
    let value_at = p.pos;
    let value = p.value(Access::Move, None)?;
    p.finish()?;
    let ty = p.scope.mutate(&name, p.line(name_at)).map_err(|e| p.error_at(name_at, e))?;
    let Some(elem) = ty.elem() else {
        let message = format!("only arrays can be indexed, but `{}` is {}", name, ty.with_article());
        return Err(p.error_at(name_at, message));
    };
    check_assign(elem, &value, &format!("an item of `{}`", name)).map_err(|e| p.error_at(value_at, e))?;
    Ok(Stmt::SetIndex { name, index, value, elem })
}

/// `set name: type <== owner;` (after the `<==`) — `name` refers to `owner` without taking it.
fn parse_borrow(p: &mut ExprParser, kind: Kind, name_at: usize, name: String, ty: Type) -> Result<Stmt> {
    if kind != Kind::Owned {
        return Err(p.error_at(name_at, "a borrowed variable cannot be declared with `keep` or `reuse`"));
    }
    let owner_at = p.pos;
    let owner = match p.peek() {
        Some(Token::Ident(word)) if p.is_at(1, ";") => parse_identifier(word).ok(),
        _ => None,
    };
    let Some(owner) = owner else {
        let message = format!("`<==` can only borrow from a variable (got `{}`)", p.text(owner_at, p.end - 1));
        return Err(p.error_at(owner_at, message));
    };
    p.pos += 1;
    p.finish()?;
    let owner_ty = p.scope.access(&owner, p.line(owner_at), Access::Read).map_err(|e| p.error_at(owner_at, e))?;
    if owner_ty != ty {
        let message =
            format!("`{}` is declared as {} but borrows `{}` of type {}", name, ty.name(), owner, owner_ty.name());
        return Err(p.error_at(name_at, message));
    }
    p.scope.declare_borrow(&name, ty, &owner).map_err(|e| p.error_at(name_at, e))?;
    let value = Expr::Var { name: owner, ty, moved: false };
    Ok(Stmt::Set { name, ty, keep: false, borrow: true, value })
}

/// `source ()=> new_name;` (`arrow` is the `()=>` token) — `new_name` is an
/// independent copy; `source` stays usable.
fn parse_copy(p: &mut ExprParser, arrow: usize) -> Result<Stmt> {
    let src_at = p.pos;
    let source = match p.peek() {
        Some(Token::Ident(word)) if arrow == src_at + 1 => parse_identifier(word).ok(),
        _ => None,
    };
    let Some(source) = source else {
        return Err(p.error_at(src_at, format!("`()=>` can only copy a variable (got `{}`)", p.text(src_at, arrow))));
    };
    p.pos = arrow + 1;
    let (dst_at, dst) = p.name()?;
    p.finish()?;
    let ty = p.scope.access(&source, p.line(src_at), Access::Read).map_err(|e| p.error_at(src_at, e))?;
    p.scope.declare(&dst, ty, Kind::Owned).map_err(|e| p.error_at(dst_at, e))?;
    let value = Expr::Var { name: source, ty, moved: false };
    Ok(Stmt::Set { name: dst, ty, keep: false, borrow: false, value })
}

/// A function header with the tokens of its name and parameter names.
#[derive(Clone)]
pub struct Header {
    pub sig: Signature,
    pub name_at: usize,
    pub params_at: Vec<usize>,
}

/// Function header: `fnc Name(a: str, b: int = 0)->` or `fnc Name(a: int): int ->`.
pub fn parse_fn_header(src: &Source, stmt: Statement) -> Result<Header> {
    let usage = "invalid function syntax, example: fnc Name(a: str, b: int = 0)-> or fnc Add(a: int, b: int): int ->";
    let mut scope = Scope::default();
    let mut p = ExprParser::new(src, stmt, &mut scope);
    if !src.is(stmt.end - 1, "->") {
        return Err(p.error_at(stmt.end - 1, usage));
    }
    p.pos += 1; // `fnc`
    if !matches!(p.peek(), Some(Token::Ident(_))) || !p.is_at(1, "(") {
        return Err(p.error(usage));
    }
    let (name_at, name) = p.name()?;
    p.pos += 1; // `(`
    let (mut params, mut params_at): (Vec<Param>, Vec<usize>) = (Vec::new(), Vec::new());
    if !p.eat(")") {
        loop {
            let (at, param) = parse_param(&mut p)?;
            if params.iter().any(|q| q.name == param.name) {
                return Err(p.error_at(at, format!("parameter `{}` is declared twice", param.name)));
            }
            if param.default.is_none() && params.iter().any(|q| q.default.is_some()) {
                let message = format!(
                    "parameter `{}` has no default, so it must come before parameters with defaults",
                    param.name
                );
                return Err(p.error_at(at, message));
            }
            params.push(param);
            params_at.push(at);
            if p.eat(")") {
                break;
            }
            if !p.eat(",") {
                return Err(p.error(usage));
            }
        }
    }
    let ret = if p.eat(":") { Some(p.type_until(&[])?) } else { None };
    if !(p.is("->") && p.pos + 1 == p.end) {
        return Err(p.error(usage));
    }
    Ok(Header { sig: Signature { name, params, ret }, name_at, params_at })
}

/// Parameter: `[keep|reuse] name: type` or `[keep|reuse] name: type = literal`.
/// Returns the token of its name and the parameter.
fn parse_param(p: &mut ExprParser) -> Result<(usize, Param)> {
    let kind = declaration_kind(p);
    let (at, name) = p.name()?;
    if !p.eat(":") {
        let message = format!("a type is required for `{}`, example: fnc Name(a: str, b: int = 0)->", name);
        return Err(p.error_at(at, message));
    }
    let ty = p.type_until(&["=", ",", ")"])?;
    let default = if p.eat("=") { Some(parse_default(p, &name, ty)?) } else { None };
    Ok((at, Param { name, ty, keep: kind == Kind::Keep, reuse: kind == Kind::Reuse, default }))
}

/// The literal default of a parameter (up to the next `,` or `)`).
fn parse_default(p: &mut ExprParser, name: &str, ty: Type) -> Result<Expr> {
    let from = p.pos;
    let mut depth = 0usize;
    let mut to = from;
    while to < p.end && !(depth == 0 && (p.src.is(to, ",") || p.src.is(to, ")") || p.src.is(to, "->"))) {
        match p.src.token(to) {
            Some(Token::Sym("(" | "[")) => depth += 1,
            Some(Token::Sym(")" | "]")) => depth = depth.saturating_sub(1),
            _ => {}
        }
        to += 1;
    }
    let mut scope = Scope::default();
    let mut d = ExprParser::new(p.src, Statement { start: from, end: to }, &mut scope);
    let value = d.or().ok().filter(|v| d.pos == to && v.is_literal());
    let Some(value) = value else {
        let message = format!("default for parameter `{}` must be a literal value (got `{}`)", name, p.text(from, to));
        return Err(p.error_at(from, message));
    };
    check_assign(ty, &value, &format!("default for parameter `{}`", name)).map_err(|e| p.error_at(from, e))?;
    p.pos = to;
    Ok(value)
}

/// Make sure a value can be stored in the target type.
pub fn check_assign(target: Type, value: &Expr, what: &str) -> std::result::Result<(), String> {
    if target.accepts(value.ty()) {
        Ok(())
    } else {
        Err(format!("{} has type {}, but got {} value", what, target.name(), value.ty().with_article()))
    }
}

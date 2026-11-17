//! Parser: turns Meng source code into a type-checked `Program`.
//!
//! - `mod.rs`      — line reader, blocks (`fnc`, `if`/`else`, `while`), `return`, program structure
//! - `comments.rs` — strips `% ... %` comments before parsing
//! - `stmt.rs`     — single-line statements (`set`, reassign, borrow, copy, `show`, calls) and function headers
//! - `lexer.rs`    — tokens for expressions (numbers, strings, names, operators)
//! - `expr.rs`     — expressions by precedence, type checks, calls, identifiers
//! - `scope.rs`    — variable tracking (types, ownership: move / reuse / borrow / reassign)
//! - `check.rs`    — function call checks (argument count/types, defaults)

mod check;
mod comments;
mod expr;
mod lexer;
mod scope;
mod stmt;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{Function, Program, Signature, Stmt};
use comments::strip_comments;
use expr::{parse_condition, parse_expr};
use scope::{Access, Kind, Scope};
use stmt::{check_assign, parse_fn_header, parse_stmt};

/// Parse source code into a `Program`.
/// Unrecognized lines, syntax errors, type mismatches, undeclared or
/// already-moved variables, and unknown functions are reported with a line number.
pub fn parse(code: &str) -> Result<Program, String> {
    let code = strip_comments(code)?;
    let lines: Vec<(usize, &str)> = code.lines().enumerate().map(|(i, l)| (i + 1, l.trim())).collect();
    let functions = Rc::new(collect_signatures(&lines)?);
    Parser { lines, pos: 0, functions, current: None }.parse_program()
}

/// First pass: read every function header, so calls (and their return types)
/// can be checked even when the function is defined further down.
fn collect_signatures(lines: &[(usize, &str)]) -> Result<HashMap<String, Signature>, String> {
    let mut functions = HashMap::new();
    for &(no, line) in lines {
        if let Some(header) = line.strip_prefix("fnc ") {
            let sig = parse_fn_header(header).map_err(|e| at(no, e))?;
            if functions.contains_key(&sig.name) {
                return Err(at(no, format!("function `{}` is already defined", sig.name)));
            }
            functions.insert(sig.name.clone(), sig);
        }
    }
    Ok(functions)
}

/// Whether a block always ends in `return` (directly, or in both branches of an `if`).
fn always_returns(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Return(_) => true,
        Stmt::If { body, else_body, .. } => always_returns(body) && always_returns(else_body),
        _ => false,
    })
}

/// Prefix an error message with its line number.
fn at(line: usize, msg: impl Into<String>) -> String {
    format!("Line {}: {}", line, msg.into())
}

/// How a block was closed.
enum BlockEnd<'a> {
    /// `<-;`
    End,
    /// `<- else ->` (line number)
    Else(usize),
    /// `<- else if ... ->` (line number, condition)
    ElseIf(usize, &'a str),
}

/// Recognize a block terminator: `<-;`, `<- else ->`, or `<- else if ... ->`.
fn parse_block_end(no: usize, line: &str) -> Result<Option<BlockEnd<'_>>, String> {
    let Some(rest) = line.strip_prefix("<-").map(str::trim) else {
        return Ok(None);
    };
    if rest == ";" {
        return Ok(Some(BlockEnd::End));
    }
    if let Some(rest) = rest.strip_prefix("else").map(str::trim) {
        if rest == "->" {
            return Ok(Some(BlockEnd::Else(no)));
        }
        if let Some(cond) = rest.strip_prefix("if ") {
            return Ok(Some(BlockEnd::ElseIf(no, cond)));
        }
    }
    Err(at(no, "invalid block terminator, use `<-;`, `<- else ->`, or `<- else if a < b ->`"))
}

struct Parser<'a> {
    lines: Vec<(usize, &'a str)>,
    pos: usize,
    functions: Rc<HashMap<String, Signature>>,
    /// The function being parsed (for `return`), `None` at the top level.
    current: Option<Signature>,
}

impl<'a> Parser<'a> {
    fn next_line(&mut self) -> Option<(usize, &'a str)> {
        let line = self.lines.get(self.pos).copied();
        self.pos += 1;
        line
    }

    fn parse_program(mut self) -> Result<Program, String> {
        let mut program = Program::default();
        let mut main_scope = Scope::new(self.functions.clone());

        while let Some((no, line)) = self.next_line() {
            if let Some(header) = line.strip_prefix("fnc ") {
                let name = header.split('(').next().unwrap_or_default().trim();
                let sig = self.functions[name].clone(); // every header was read by the first pass
                program.functions.push(self.parse_function(no, sig)?);
            } else if parse_block_end(no, line)?.is_some() {
                return Err(at(no, "block terminator without a matching `fnc`/`if`/`while`"));
            } else {
                program.main.extend(self.parse_stmt_line(no, line, &mut main_scope)?);
            }
        }
        Ok(program)
    }

    /// `fnc Name(params)[: type] ->` followed by its body up to `<-;`.
    fn parse_function(&mut self, no: usize, sig: Signature) -> Result<Function, String> {
        let mut scope = Scope::new(self.functions.clone());
        for p in &sig.params {
            let kind = if p.keep { Kind::Keep } else if p.reuse { Kind::Reuse } else { Kind::Owned };
            scope.declare(&p.name, p.ty, kind).map_err(|e| at(no, e))?;
        }
        self.current = Some(sig.clone());
        let block = self.parse_block(no, &format!("function `{}`", sig.name), &mut scope);
        self.current = None;
        let (body, end) = block?;
        if let BlockEnd::Else(n) | BlockEnd::ElseIf(n, _) = end {
            return Err(at(n, "`else` without `if`"));
        }
        if let Some(ty) = sig.ret {
            if !always_returns(&body) {
                return Err(at(
                    no,
                    format!("function `{}` must return {} value on every path (missing `return`)", sig.name, ty.with_article()),
                ));
            }
        }
        let Signature { name, params, ret } = sig;
        Ok(Function { name, params, ret, body })
    }

    /// Read statements until a block terminator (function body, `if`, or `else`).
    fn parse_block(
        &mut self,
        opener: usize,
        what: &str,
        scope: &mut Scope,
    ) -> Result<(Vec<Stmt>, BlockEnd<'a>), String> {
        let mut body = Vec::new();
        while let Some((no, line)) = self.next_line() {
            if let Some(end) = parse_block_end(no, line)? {
                return Ok((body, end));
            }
            if line.starts_with("fnc ") {
                return Err(at(no, "functions can only be defined at the top level"));
            }
            body.extend(self.parse_stmt_line(no, line, scope)?);
        }
        Err(at(opener, format!("{} is not closed with `<-;`", what)))
    }

    /// `if <condition> ->` together with its `else if` / `else` chain.
    fn parse_if(&mut self, no: usize, cond: &str, scope: &mut Scope) -> Result<Stmt, String> {
        let cond = parse_condition(cond, scope, no).map_err(|e| at(no, e))?;

        // Each branch gets its own copy of the scope: variables declared inside
        // a block are not visible outside it.
        let mut then_scope = scope.clone();
        let (body, end) = self.parse_block(no, "`if` block", &mut then_scope)?;

        let mut else_scope = scope.clone();
        let else_body = match end {
            BlockEnd::End => Vec::new(),
            BlockEnd::ElseIf(n, cond) => vec![self.parse_if(n, cond, &mut else_scope)?],
            BlockEnd::Else(n) => {
                let (body, end) = self.parse_block(n, "`else` block", &mut else_scope)?;
                if let BlockEnd::Else(m) | BlockEnd::ElseIf(m, _) = end {
                    return Err(at(m, "`else` was already used, close the block with `<-;`"));
                }
                body
            }
        };

        // A variable moved in either branch counts as moved after the `if`,
        // unless that branch always returns.
        let then_state = (!always_returns(&body)).then_some(&then_scope);
        let else_state = (!always_returns(&else_body)).then_some(&else_scope);
        scope.merge_branches(then_state, else_state);
        Ok(Stmt::If { cond, body, else_body })
    }

    /// `while <condition> ->` and its body.
    fn parse_while(&mut self, no: usize, cond: &str, scope: &mut Scope) -> Result<Stmt, String> {
        let cond = parse_condition(cond, scope, no).map_err(|e| at(no, e))?;
        let mut body_scope = scope.clone();
        let (body, end) = self.parse_block(no, "`while` loop", &mut body_scope)?;
        if let BlockEnd::Else(n) | BlockEnd::ElseIf(n, _) = end {
            return Err(at(n, "`else` without `if`"));
        }
        // The body may run again: it must not leave outside variables moved.
        if !always_returns(&body) {
            scope.check_loop(&body_scope).map_err(|(line, e)| at(line, e))?;
        }
        Ok(Stmt::While { cond, body })
    }

    /// `return;` or `return value;` inside a function.
    fn parse_return(&mut self, no: usize, value: &str, scope: &mut Scope) -> Result<Stmt, String> {
        let Some(sig) = &self.current else {
            return Err("`return` can only be used inside a function".into());
        };
        match (value.trim(), sig.ret) {
            ("", None) => Ok(Stmt::Return(None)),
            ("", Some(ty)) => Err(format!("function `{}` must return {} value", sig.name, ty.with_article())),
            (_, None) => Err(format!("function `{}` has no return type, so use `return;` without a value", sig.name)),
            (value, Some(ty)) => {
                let expr = parse_expr(value, scope, no, Access::Move)?;
                check_assign(ty, &expr, &format!("return value of `{}`", sig.name))?;
                Ok(Stmt::Return(Some(expr)))
            }
        }
    }

    fn parse_stmt_line(&mut self, no: usize, line: &str, scope: &mut Scope) -> Result<Option<Stmt>, String> {
        if let Some(cond) = line.strip_prefix("if ") {
            return self.parse_if(no, cond, scope).map(Some);
        }
        if let Some(cond) = line.strip_prefix("while ") {
            return self.parse_while(no, cond, scope).map(Some);
        }
        if let Some(value) = line.strip_prefix("return").and_then(|s| s.strip_suffix(';')) {
            if value.is_empty() || value.starts_with(' ') {
                return self.parse_return(no, value, scope).map(Some).map_err(|e| at(no, e));
            }
        }
        parse_stmt(line, scope, no).map_err(|e| at(no, e))
    }
}

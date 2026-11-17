//! Parser: turns Meng source code into a type-checked `Program`.
//!
//! - `mod.rs`      — line reader, blocks (`fnc`, `if`/`else`), program structure
//! - `comments.rs` — strips `% ... %` comments before parsing
//! - `stmt.rs`     — single-line statements (`set`, borrow, copy, `show`, calls) and function headers
//! - `expr.rs`     — values, conditions, identifiers, string literals
//! - `scope.rs`    — variable tracking (types, ownership: move / reuse / borrow)
//! - `check.rs`    — function call checks (existence, argument count/types)

mod check;
mod comments;
mod expr;
mod scope;
mod stmt;

use crate::ast::{Expr, Function, Program, Stmt};
use check::{PendingCall, check_calls};
use comments::strip_comments;
use expr::parse_condition;
use scope::Scope;
use stmt::{parse_fn_header, parse_stmt};

/// Parse source code into a `Program`.
/// Unrecognized lines, syntax errors, type mismatches, undeclared or
/// already-moved variables, and unknown functions are reported with a line number.
pub fn parse(code: &str) -> Result<Program, String> {
    let code = strip_comments(code)?;
    let lines = code.lines().enumerate().map(|(i, l)| (i + 1, l.trim())).collect();
    Parser { lines, pos: 0, calls: Vec::new() }.parse_program()
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
    calls: Vec<PendingCall>,
}

impl<'a> Parser<'a> {
    fn next_line(&mut self) -> Option<(usize, &'a str)> {
        let line = self.lines.get(self.pos).copied();
        self.pos += 1;
        line
    }

    fn parse_program(mut self) -> Result<Program, String> {
        let mut program = Program::default();
        let mut main_scope = Scope::default();

        while let Some((no, line)) = self.next_line() {
            if let Some(header) = line.strip_prefix("fnc ") {
                let function = self.parse_function(no, header)?;
                if program.functions.iter().any(|f| f.name == function.name) {
                    return Err(at(no, format!("function `{}` is already defined", function.name)));
                }
                program.functions.push(function);
            } else if parse_block_end(no, line)?.is_some() {
                return Err(at(no, "block terminator without a matching `fnc`/`if`"));
            } else {
                program.main.extend(self.parse_stmt_line(no, line, &mut main_scope)?);
            }
        }

        check_calls(&program.functions, &self.calls)?;
        Ok(program)
    }

    /// `fnc Name(params)->` followed by its body up to `<-;`.
    fn parse_function(&mut self, no: usize, header: &str) -> Result<Function, String> {
        let (name, params) = parse_fn_header(header).map_err(|e| at(no, e))?;
        let mut scope = Scope::default();
        for p in &params {
            scope.declare(&p.name, p.ty, p.keep || p.reuse).map_err(|e| at(no, e))?;
        }
        let (body, end) = self.parse_block(no, &format!("function `{}`", name), &mut scope)?;
        if let BlockEnd::Else(n) | BlockEnd::ElseIf(n, _) = end {
            return Err(at(n, "`else` without `if`"));
        }
        Ok(Function { name, params, body })
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

        // A variable moved in either branch counts as moved after the `if`.
        scope.merge_moved(&then_scope);
        scope.merge_moved(&else_scope);
        Ok(Stmt::If { cond, body, else_body })
    }

    fn parse_stmt_line(&mut self, no: usize, line: &str, scope: &mut Scope) -> Result<Option<Stmt>, String> {
        if let Some(cond) = line.strip_prefix("if ") {
            return self.parse_if(no, cond, scope).map(Some);
        }

        let stmt = parse_stmt(line, scope, no).map_err(|e| at(no, e))?;
        if let Some(Stmt::Call { name, args }) = &stmt {
            let arg_types = args.iter().map(Expr::ty).collect();
            self.calls.push(PendingCall { line: no, name: name.clone(), arg_types });
        }
        Ok(stmt)
    }
}

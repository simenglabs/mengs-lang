//! Parser: turns Meng source code into a type-checked `Program`.
//!
//! - `mod.rs`      — statement reader (collects every error), blocks (`fnc`, `if`/`else`, `while`), `return`, program structure
//! - `comments.rs` — strips `% ... %` comments before parsing
//! - `diagnostic.rs` — errors with line/column, splitting lines into statements
//! - `stmt.rs`     — single-line statements (`set`, reassign, borrow, copy, `show`, calls) and function headers
//! - `lexer.rs`    — tokens for expressions (numbers, strings, names, operators)
//! - `expr.rs`     — expressions by precedence, type checks, calls, identifiers
//! - `scope.rs`    — variable tracking (types, ownership: move / reuse / borrow / reassign)
//! - `check.rs`    — function call checks (argument count/types, defaults)
//! - `symbols.rs`  — functions, variables, and ownership events for editors (`meng symbols`)
//! - `modules.rs`  — `use "file" meow Function;`: loading imported files

mod check;
mod comments;
mod diagnostic;
mod expr;
mod lexer;
mod modules;
mod scope;
mod stmt;
mod symbols;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{Expr, Function, Program, Signature, Stmt, always_returns};
pub use diagnostic::Diagnostic;
use diagnostic::{Piece, diagnostic, split_pieces};
use expr::{BUILTINS, parse_condition, parse_typed};
use scope::{Access, Kind, Scope};
use stmt::{check_assign, declare_after_error, parse_fn_header, parse_stmt};
pub use comments::strip_comments;
pub use modules::{SourceError, parse_file, symbols_file};
pub use symbols::Symbols;
use symbols::{FunctionSymbol, VariableSymbol};

/// Functions visible to a file through its `use` imports: name -> (signature, file).
pub type Imported = HashMap<String, (Signature, String)>;

/// Parse a single source (no imports) into a `Program`, or return every error
/// found (unrecognized statements, syntax and type errors, ownership errors, ...).
/// Files with `use` go through `parse_file`.
#[cfg(test)]
pub fn parse(code: &str) -> Result<Program, Vec<Diagnostic>> {
    analyze(code, &Imported::new(), false).0
}

/// Symbols of a single source, for tests.
#[cfg(test)]
pub fn symbols(code: &str) -> Symbols {
    analyze(code, &Imported::new(), false).1
}

/// Parse one file, given the functions it imports. A `library` (an imported
/// file) may only contain functions and `use`.
fn analyze(code: &str, imported: &Imported, library: bool) -> (Result<Program, Vec<Diagnostic>>, Symbols) {
    let code = match strip_comments(code) {
        Ok(code) => code,
        Err(e) => return (Err(vec![e]), Symbols::default()),
    };
    let pieces = split_pieces(&code);
    let last_line = code.lines().count().max(1);
    let mut errors = Vec::new();
    let (mut functions, headers) = collect_signatures(&pieces, imported, &mut errors);
    for (name, (sig, _)) in imported {
        functions.entry(name.clone()).or_insert_with(|| sig.clone());
    }
    let mut parser = Parser {
        library,
        pieces,
        pos: 0,
        functions: Rc::new(functions),
        headers,
        current: None,
        errors,
        events: Rc::default(),
        symbols: Symbols::default(),
        blocks: Vec::new(),
    };
    let program = parser.parse_program(last_line);
    let mut symbols = parser.symbols;
    symbols.events = parser.events.take();

    let mut errors = parser.errors;
    if errors.is_empty() {
        return (Ok(program), symbols);
    }
    errors.sort_by_key(|e| (e.line, e.column));
    errors.dedup();
    (Err(errors), symbols)
}

/// First pass: read every function header, so calls (and their return types)
/// can be checked even when the function is defined further down.
/// Returns the signatures by name, and by the index of their header piece.
fn collect_signatures(
    pieces: &[Piece],
    imported: &Imported,
    errors: &mut Vec<Diagnostic>,
) -> (HashMap<String, Signature>, HashMap<usize, Signature>) {
    let (mut functions, mut headers) = (HashMap::new(), HashMap::new());
    for (i, piece) in pieces.iter().enumerate() {
        let Some(header) = piece.text.strip_prefix("fnc ") else { continue };
        let sig = match parse_fn_header(header) {
            Ok(sig) if BUILTINS.contains(&sig.name.as_str()) => {
                Err(format!("`{}` is a built-in function and cannot be redefined", sig.name))
            }
            Ok(sig) if functions.contains_key(&sig.name) => Err(format!("function `{}` is already defined", sig.name)),
            Ok(sig) if imported.contains_key(&sig.name) => {
                Err(format!("function `{}` is already defined in `{}`", sig.name, imported[&sig.name].1))
            }
            other => other,
        };
        match sig {
            Ok(sig) => {
                functions.insert(sig.name.clone(), sig.clone());
                headers.insert(i, sig);
            }
            Err(e) => errors.push(diagnostic(*piece, e)),
        }
    }
    (functions, headers)
}

/// How a block was closed.
enum BlockEnd<'a> {
    /// `<-;` (or the end of the file, already reported)
    End,
    /// `<- else ->`
    Else(Piece<'a>),
    /// `<- else if ... ->` (with the condition)
    ElseIf(Piece<'a>, &'a str),
}

/// Recognize a block terminator: `<-;`, `<- else ->`, or `<- else if ... ->`.
fn parse_block_end(piece: Piece<'_>) -> Option<Result<BlockEnd<'_>, String>> {
    let rest = piece.text.strip_prefix("<-")?.trim();
    if rest == ";" {
        return Some(Ok(BlockEnd::End));
    }
    if let Some(rest) = rest.strip_prefix("else").map(str::trim) {
        if rest == "->" {
            return Some(Ok(BlockEnd::Else(piece)));
        }
        if let Some(cond) = rest.strip_prefix("if ") {
            return Some(Ok(BlockEnd::ElseIf(piece, cond)));
        }
    }
    Some(Err("invalid block terminator, use `<-;`, `<- else ->`, or `<- else if a < b ->`".into()))
}

struct Parser<'a> {
    /// An imported file: only functions and `use` at the top level.
    library: bool,
    pieces: Vec<Piece<'a>>,
    pos: usize,
    functions: Rc<HashMap<String, Signature>>,
    /// Valid function headers, by piece index.
    headers: HashMap<usize, Signature>,
    /// The function being parsed (for `return`), `None` at the top level.
    current: Option<Signature>,
    errors: Vec<Diagnostic>,
    events: Rc<RefCell<Vec<scope::OwnershipEvent>>>,
    symbols: Symbols,
    /// Indices into `symbols.variables` declared in each open block.
    blocks: Vec<Vec<usize>>,
}

impl<'a> Parser<'a> {
    fn next_piece(&mut self) -> Option<Piece<'a>> {
        let piece = self.pieces.get(self.pos).copied();
        self.pos += 1;
        piece
    }

    fn error(&mut self, piece: Piece, message: impl Into<String>) {
        self.errors.push(diagnostic(piece, message.into()));
    }

    /// An error on a given line (e.g. where a variable was moved inside a loop).
    fn error_on_line(&mut self, line: usize, fallback: Piece, message: String) {
        let piece = self.pieces.iter().find(|p| p.line == line).copied().unwrap_or(fallback);
        self.error(piece, message);
    }

    /// Record a declared variable (visible until its block closes).
    fn declare_symbol(&mut self, piece: Piece, name: &str, ty: crate::ast::Type, kind: &'static str) {
        let column = diagnostic(piece, format!("`{}`", name)).column;
        let function = self.current.as_ref().map(|f| f.name.clone());
        let symbol = VariableSymbol { name: name.to_string(), ty: ty.name(), kind, line: piece.line, column, end: piece.line, function };
        self.symbols.variables.push(symbol);
        if let Some(block) = self.blocks.last_mut() {
            block.push(self.symbols.variables.len() - 1);
        }
    }

    /// Close the innermost symbol block at `line`.
    fn close_symbols(&mut self, line: usize) {
        for i in self.blocks.pop().unwrap_or_default() {
            self.symbols.variables[i].end = line;
        }
    }

    /// Line of the piece read last (the block terminator after `parse_block`).
    fn last_line(&self) -> usize {
        self.pieces.get(self.pos.saturating_sub(1).min(self.pieces.len().saturating_sub(1))).map_or(1, |p| p.line)
    }

    fn parse_program(&mut self, last_line: usize) -> Program {
        let mut program = Program::default();
        let mut main_scope = Scope::new(self.functions.clone(), self.events.clone());
        self.blocks.push(Vec::new());

        while let Some(piece) = self.next_piece() {
            if piece.text.starts_with("fnc ") {
                match self.headers.get(&(self.pos - 1)).cloned() {
                    Some(sig) => program.functions.push(self.parse_function(piece, sig)),
                    None => self.skip_block(), // header error already reported
                }
            } else if parse_block_end(piece).is_some() {
                self.error(piece, "block terminator without a matching `fnc`/`if`/`while`");
            } else if piece.text.starts_with("use ") {
                // Loaded by `modules.rs`; only the syntax is checked here.
                if let Err(e) = modules::parse_use(piece.text) {
                    self.error(piece, e);
                }
            } else if self.library {
                self.error(piece, "an imported file can only contain functions and `use`");
                if piece.text.ends_with("->") {
                    self.skip_block();
                }
            } else if let Some(stmt) = self.parse_stmt_line(piece, &mut main_scope) {
                program.main.push(stmt);
            }
        }
        self.close_symbols(last_line);
        program
    }

    /// Skip the body of a block whose opener could not be parsed.
    fn skip_block(&mut self) {
        let mut depth = 1;
        while let Some(piece) = self.next_piece() {
            let text = piece.text;
            if text.starts_with("<-") && !text.ends_with("->") {
                depth -= 1;
            } else if !text.starts_with("<-") && text.ends_with("->") {
                depth += 1;
            }
            if depth == 0 {
                return;
            }
        }
    }

    /// `fnc Name(params)[: type] ->` followed by its body up to `<-;`.
    fn parse_function(&mut self, opener: Piece<'a>, sig: Signature) -> Function {
        let mut scope = Scope::new(self.functions.clone(), self.events.clone());
        self.current = Some(sig.clone());
        self.blocks.push(Vec::new());
        for p in &sig.params {
            let kind = if p.keep { Kind::Keep } else if p.reuse { Kind::Reuse } else { Kind::Owned };
            if let Err(e) = scope.declare(&p.name, p.ty, kind) {
                self.error(opener, e);
            }
            let label = if p.keep { "keep" } else if p.reuse { "reuse" } else { "param" };
            self.declare_symbol(opener, &p.name, p.ty, label);
        }
        let (body, end) = self.parse_block(opener, &format!("function `{}`", sig.name), &mut scope);
        let end_line = self.last_line();
        self.close_symbols(end_line);
        self.current = None;
        let column = diagnostic(opener, format!("`{}`", sig.name)).column;
        self.symbols.functions.push(FunctionSymbol { sig: sig.clone(), file: None, line: opener.line, column, end: end_line });
        if let BlockEnd::Else(p) | BlockEnd::ElseIf(p, _) = end {
            self.error(p, "`else` without `if`");
        }
        if let Some(ty) = sig.ret {
            if !always_returns(&body) {
                let msg = format!("function `{}` must return {} value on every path (missing `return`)", sig.name, ty.with_article());
                self.error(opener, msg);
            }
        }
        let Signature { name, params, ret } = sig;
        Function { name, params, ret, body }
    }

    /// Read statements until a block terminator (function body, `if`, `else`, or `while`).
    fn parse_block(&mut self, opener: Piece<'a>, what: &str, scope: &mut Scope) -> (Vec<Stmt>, BlockEnd<'a>) {
        self.blocks.push(Vec::new());
        let result = self.parse_block_body(opener, what, scope);
        let line = self.last_line();
        self.close_symbols(line);
        result
    }

    fn parse_block_body(&mut self, opener: Piece<'a>, what: &str, scope: &mut Scope) -> (Vec<Stmt>, BlockEnd<'a>) {
        let mut body = Vec::new();
        while let Some(piece) = self.next_piece() {
            match parse_block_end(piece) {
                Some(Ok(end)) => return (body, end),
                Some(Err(e)) => {
                    self.error(piece, e);
                    return (body, BlockEnd::End);
                }
                None => {}
            }
            if piece.text.starts_with("use ") {
                self.error(piece, "`use` is only allowed at the top level of a file");
                continue;
            }
            if piece.text.starts_with("fnc ") {
                self.error(piece, "functions can only be defined at the top level (is a `<-;` missing above?)");
                self.skip_block();
                continue;
            }
            body.extend(self.parse_stmt_line(piece, scope));
        }
        self.error(opener, format!("{} is not closed with `<-;`", what));
        (body, BlockEnd::End)
    }

    /// A condition; on error, it is reported and replaced by `true` so the block still parses.
    fn condition(&mut self, piece: Piece<'a>, cond: &str, scope: &mut Scope) -> Expr {
        parse_condition(cond, scope, piece.line).unwrap_or_else(|e| {
            self.error(piece, e);
            Expr::Bool(true)
        })
    }

    /// `if <condition> ->` together with its `else if` / `else` chain.
    fn parse_if(&mut self, opener: Piece<'a>, cond: &str, scope: &mut Scope) -> Stmt {
        let cond = self.condition(opener, cond, scope);

        // Each branch gets its own copy of the scope: variables declared inside
        // a block are not visible outside it.
        let mut then_scope = scope.clone();
        let (body, end) = self.parse_block(opener, "`if` block", &mut then_scope);

        let mut else_scope = scope.clone();
        let else_body = match end {
            BlockEnd::End => Vec::new(),
            BlockEnd::ElseIf(p, cond) => vec![self.parse_if(p, cond, &mut else_scope)],
            BlockEnd::Else(p) => {
                let (body, end) = self.parse_block(p, "`else` block", &mut else_scope);
                if let BlockEnd::Else(q) | BlockEnd::ElseIf(q, _) = end {
                    self.error(q, "`else` was already used, close the block with `<-;`");
                }
                body
            }
        };

        // A variable moved in either branch counts as moved after the `if`,
        // unless that branch always returns.
        let then_state = (!always_returns(&body)).then_some(&then_scope);
        let else_state = (!always_returns(&else_body)).then_some(&else_scope);
        scope.merge_branches(then_state, else_state);
        Stmt::If { cond, body, else_body }
    }

    /// `while <condition> ->` and its body.
    fn parse_while(&mut self, opener: Piece<'a>, cond: &str, scope: &mut Scope) -> Stmt {
        let cond = self.condition(opener, cond, scope);
        let mut body_scope = scope.clone();
        let (body, end) = self.parse_block(opener, "`while` loop", &mut body_scope);
        if let BlockEnd::Else(p) | BlockEnd::ElseIf(p, _) = end {
            self.error(p, "`else` without `if`");
        }
        // The body may run again: it must not leave outside variables moved.
        if !always_returns(&body) {
            if let Err((line, e)) = scope.check_loop(&body_scope) {
                self.error_on_line(line, opener, e);
            }
        }
        Stmt::While { cond, body }
    }

    /// `return;` or `return value;` inside a function.
    fn parse_return(&self, line: usize, value: &str, scope: &mut Scope) -> Result<Stmt, String> {
        let Some(sig) = &self.current else {
            return Err("`return` can only be used inside a function".into());
        };
        match (value.trim(), sig.ret) {
            ("", None) => Ok(Stmt::Return(None)),
            ("", Some(ty)) => Err(format!("function `{}` must return {} value", sig.name, ty.with_article())),
            (_, None) => Err(format!("function `{}` has no return type, so use `return;` without a value", sig.name)),
            (value, Some(ty)) => {
                let expr = parse_typed(value, scope, line, Access::Move, ty)?;
                check_assign(ty, &expr, &format!("return value of `{}`", sig.name))?;
                Ok(Stmt::Return(Some(expr)))
            }
        }
    }

    /// One statement. Errors are recorded and the statement is skipped.
    fn parse_stmt_line(&mut self, piece: Piece<'a>, scope: &mut Scope) -> Option<Stmt> {
        let text = piece.text;
        if let Some(cond) = text.strip_prefix("if ") {
            return Some(self.parse_if(piece, cond, scope));
        }
        if let Some(cond) = text.strip_prefix("while ") {
            return Some(self.parse_while(piece, cond, scope));
        }
        let result = match text.strip_prefix("return").and_then(|s| s.strip_suffix(';')) {
            Some(value) if value.is_empty() || value.starts_with(' ') => {
                self.parse_return(piece.line, value, scope).map(Some)
            }
            _ => parse_stmt(text, scope, piece.line),
        };
        if let Ok(Some(Stmt::Set { name, ty, keep, borrow, .. })) = &result {
            let kind = match (keep, borrow, scope.kind_of(name)) {
                (true, _, _) => "keep",
                (_, true, _) => "borrow",
                (_, _, Some(Kind::Reuse)) => "reuse",
                _ => "owned",
            };
            self.declare_symbol(piece, name, *ty, kind);
        }
        let is_return = text.starts_with("return") && text.ends_with(';');
        result.unwrap_or_else(|e| {
            self.error(piece, e);
            // A broken `return` still ends its path (no extra "missing `return`" error).
            if is_return {
                return Some(Stmt::Return(None));
            }
            // Keep a failed `set`'s variable known, so later lines don't report it as undeclared.
            declare_after_error(text, scope);
            None
        })
    }
}

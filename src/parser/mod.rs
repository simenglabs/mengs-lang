//! Parser: turns Meng source code into a type-checked `Program`.
//!
//! - `mod.rs`      — statement reader (collects every error), blocks (`fnc`, `if`/`else`, `while`), `return`, program structure
//! - `comments.rs` — strips `% ... %` comments before parsing
//! - `lexer.rs`    — tokens with their line and column
//! - `diagnostic.rs` — errors with line/column, the tokens of a file, splitting them into statements
//! - `stmt.rs`     — simple statements (`set`, reassign, borrow, copy, `show`, calls) and function headers
//! - `expr.rs`     — the token cursor; expressions by precedence, type checks, calls, identifiers
//! - `scope.rs`    — variable tracking (types, ownership: move / reuse / borrow / reassign)
//! - `check.rs`    — function call checks (argument count/types, defaults)
//! - `symbols.rs`  — functions, variables, and ownership events for editors (`meng symbols`)
//! - `modules.rs`  — `use "file.meng";`: loading imported files

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
pub use diagnostic::{Diagnostic, Severity};
use diagnostic::{Source, Statement};
use expr::{BUILTINS, ExprParser};
use scope::{Access, Kind, Scope};
use stmt::{Header, check_assign, declare_after_error, declared_name, incomplete, parse_fn_header, parse_stmt};
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

/// Display strings of the warnings of a single source, for tests.
#[cfg(test)]
pub fn warnings(code: &str) -> Vec<String> {
    analyze(code, &Imported::new(), false).1.iter().map(|w| w.to_string()).collect()
}

/// Symbols of a single source, for tests.
#[cfg(test)]
pub fn symbols(code: &str) -> Symbols {
    analyze(code, &Imported::new(), false).2
}

/// Parse one file, given the functions it imports. A `library` (an imported
/// file) may only contain functions and `use`. Returns the program (or the
/// errors), the warnings, and the symbols.
fn analyze(code: &str, imported: &Imported, library: bool) -> (Result<Program, Vec<Diagnostic>>, Vec<Diagnostic>, Symbols) {
    let code = match strip_comments(code) {
        Ok(code) => code,
        Err(e) => return (Err(vec![e]), Vec::new(), Symbols::default()),
    };
    let (src, mut errors) = Source::new(&code);
    let statements = src.statements();
    let last_line = code.lines().count().max(1);
    let (mut functions, headers) = collect_signatures(&src, &statements, imported, &mut errors);
    for (name, (sig, _)) in imported {
        functions.entry(name.clone()).or_insert_with(|| sig.clone());
    }
    let mut parser = Parser {
        library,
        src: &src,
        statements,
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

    let mut all = parser.errors;
    all.sort_by_key(|e| (e.line, e.column));
    all.dedup();
    let (errors, warnings): (Vec<_>, Vec<_>) = all.into_iter().partition(|d| d.severity == Severity::Error);
    if errors.is_empty() {
        return (Ok(program), warnings, symbols);
    }
    (Err(errors), warnings, symbols)
}

/// First pass: read every function header, so calls (and their return types)
/// can be checked even when the function is defined further down.
/// Returns the signatures by name, and the headers by the index of their statement.
fn collect_signatures(
    src: &Source,
    statements: &[Statement],
    imported: &Imported,
    errors: &mut Vec<Diagnostic>,
) -> (HashMap<String, Signature>, HashMap<usize, Header>) {
    let (mut functions, mut headers) = (HashMap::new(), HashMap::new());
    for (i, &stmt) in statements.iter().enumerate() {
        if !src.is(stmt.start, "fnc") {
            continue;
        }
        let header = parse_fn_header(src, stmt).and_then(|h| {
            let name = &h.sig.name;
            let clash = if BUILTINS.contains(&name.as_str()) {
                Some(format!("`{}` is a built-in function and cannot be redefined", name))
            } else if functions.contains_key(name) {
                Some(format!("function `{}` is already defined", name))
            } else {
                imported.get(name).map(|(_, file)| format!("function `{}` is already defined in `{}`", name, file))
            };
            match clash {
                Some(message) => Err(src.error(h.name_at, message)),
                None => Ok(h),
            }
        });
        match header {
            Ok(h) => {
                functions.insert(h.sig.name.clone(), h.sig.clone());
                headers.insert(i, h);
            }
            Err(e) if !src.is_broken(stmt) => errors.push(e),
            Err(_) => {}
        }
    }
    (functions, headers)
}

/// How a block was closed.
enum BlockEnd {
    /// `<-;` (or the end of the file, already reported)
    End,
    /// `<- else ->`
    Else(Statement),
    /// `<- else if ... ->`, with its condition (the tokens after `if`)
    ElseIf(Statement, Statement),
}

struct Parser<'a> {
    /// An imported file: only functions and `use` at the top level.
    library: bool,
    src: &'a Source,
    statements: Vec<Statement>,
    pos: usize,
    functions: Rc<HashMap<String, Signature>>,
    /// Valid function headers, by statement index.
    headers: HashMap<usize, Header>,
    /// The function being parsed (for `return`), `None` at the top level.
    current: Option<Signature>,
    errors: Vec<Diagnostic>,
    events: Rc<RefCell<Vec<scope::OwnershipEvent>>>,
    symbols: Symbols,
    /// Indices into `symbols.variables` declared in each open block.
    blocks: Vec<Vec<usize>>,
}

impl<'a> Parser<'a> {
    fn next_stmt(&mut self) -> Option<Statement> {
        let stmt = self.statements.get(self.pos).copied();
        self.pos += 1;
        stmt
    }

    /// Whether a statement starts with the word or symbol `s`.
    fn starts(&self, stmt: Statement, s: &str) -> bool {
        self.src.is(stmt.start, s)
    }

    /// Whether a statement opens a block (ends with `->`).
    fn opens_block(&self, stmt: Statement) -> bool {
        self.src.is(stmt.end - 1, "->")
    }

    /// Record an error about a statement, unless the statement has a lexing
    /// error (already reported, and the rest of its line is gone).
    fn fail(&mut self, stmt: Statement, error: Diagnostic) {
        if !self.src.is_broken(stmt) {
            self.errors.push(error);
        }
    }

    /// Record an error at token `at` of a statement.
    fn error(&mut self, stmt: Statement, at: usize, message: impl Into<String>) {
        let error = self.src.error(at, message);
        self.fail(stmt, error);
    }

    fn warn(&mut self, at: usize, message: impl Into<String>) {
        let mut d = self.src.error(at, message);
        d.severity = Severity::Warning;
        self.errors.push(d);
    }

    /// `reuse` on a value that is always copied has no effect (`at`: the declared name).
    fn warn_useless_reuse(&mut self, at: usize, ty: crate::ast::Type) {
        if !ty.is_heap() {
            self.warn(at, format!("`reuse` has no effect on {} values (they are always copied)", ty.name()));
        }
    }

    /// An error at the first `name` on a given line (e.g. where a variable was
    /// moved inside a loop), or at the first token of that line.
    fn error_on_line(&mut self, line: usize, name: &str, fallback: Statement, message: String) {
        let tokens = &self.src.tokens;
        let is_name = |t: &lexer::Spanned| t.line == line && matches!(&t.token, lexer::Token::Ident(w) if w == name);
        let at = tokens
            .iter()
            .position(is_name)
            .or_else(|| tokens.iter().position(|t| t.line == line))
            .unwrap_or(fallback.start);
        self.errors.push(self.src.error(at, message));
    }

    /// Record a declared variable (visible until its block closes); `at` is its name's token.
    fn declare_symbol(&mut self, at: usize, name: &str, ty: crate::ast::Type, kind: &'static str) {
        let token = &self.src.tokens[at];
        let (line, column) = (token.line, token.column);
        let function = self.current.as_ref().map(|f| f.name.clone());
        let symbol = VariableSymbol { name: name.to_string(), ty: ty.name(), kind, line, column, end: line, function };
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

    /// Line of the statement read last (the block terminator after `parse_block`).
    fn last_line(&self) -> usize {
        let last = self.pos.saturating_sub(1).min(self.statements.len().saturating_sub(1));
        self.statements.get(last).map_or(1, |s| self.src.line(s.start))
    }

    fn parse_program(&mut self, last_line: usize) -> Program {
        let mut program = Program::default();
        let mut main_scope = Scope::new(self.functions.clone(), self.events.clone());
        self.blocks.push(Vec::new());

        while let Some(stmt) = self.next_stmt() {
            if self.starts(stmt, "fnc") {
                match self.headers.get(&(self.pos - 1)).cloned() {
                    Some(header) => program.functions.push(self.parse_function(stmt, header)),
                    None => self.skip_block(), // header error already reported
                }
            } else if self.starts(stmt, "<-") {
                self.error(stmt, stmt.start, "block terminator without a matching `fnc`/`if`/`while`");
            } else if self.starts(stmt, "use") {
                // Loaded by `modules.rs`; only the syntax is checked here.
                let tokens: Vec<_> = self.src.tokens[stmt.start..stmt.end].iter().map(|t| t.token.clone()).collect();
                if let Err(e) = modules::parse_use(&tokens) {
                    self.error(stmt, stmt.start, e);
                }
            } else if self.library {
                self.error(stmt, stmt.start, "an imported file can only contain functions and `use`");
                if self.opens_block(stmt) {
                    self.skip_block();
                }
            } else if let Some(s) = self.parse_stmt_line(stmt, &mut main_scope) {
                program.main.push(s);
            }
        }
        self.close_symbols(last_line);
        program
    }

    /// Skip the body of a block whose opener could not be parsed.
    fn skip_block(&mut self) {
        let mut depth = 1;
        while let Some(stmt) = self.next_stmt() {
            let (closes, opens) = (self.starts(stmt, "<-"), self.opens_block(stmt));
            if closes && !opens {
                depth -= 1;
            } else if !closes && opens {
                depth += 1;
            }
            if depth == 0 {
                return;
            }
        }
    }

    /// `fnc Name(params)[: type] ->` followed by its body up to `<-;`.
    fn parse_function(&mut self, opener: Statement, header: Header) -> Function {
        let Header { sig, name_at, params_at } = header;
        let mut scope = Scope::new(self.functions.clone(), self.events.clone());
        self.current = Some(sig.clone());
        self.blocks.push(Vec::new());
        for (p, &at) in sig.params.iter().zip(&params_at) {
            let kind = if p.keep && p.ty.is_heap() {
                Kind::KeepParam
            } else if p.keep {
                Kind::Keep
            } else if p.reuse {
                Kind::Reuse
            } else {
                Kind::Owned
            };
            if let Err(e) = scope.declare(&p.name, p.ty, kind) {
                self.error(opener, at, e);
            }
            if p.reuse {
                self.warn_useless_reuse(at, p.ty);
            }
            let label = if p.keep { "keep" } else if p.reuse { "reuse" } else { "param" };
            self.declare_symbol(at, &p.name, p.ty, label);
        }
        let (body, end) = self.parse_block(opener, &format!("function `{}`", sig.name), &mut scope);
        let end_line = self.last_line();
        self.close_symbols(end_line);
        self.current = None;
        let name_token = &self.src.tokens[name_at];
        let (line, column) = (name_token.line, name_token.column);
        self.symbols.functions.push(FunctionSymbol { sig: sig.clone(), file: None, line, column, end: end_line });
        if let BlockEnd::Else(s) | BlockEnd::ElseIf(s, _) = end {
            self.error(s, s.start + 1, "`else` without `if`");
        }
        if let Some(ty) = sig.ret {
            if !always_returns(&body) {
                let msg = format!("function `{}` must return {} value on every path (missing `return`)", sig.name, ty.with_article());
                self.error(opener, name_at, msg);
            }
        }
        let Signature { name, params, ret } = sig;
        Function { name, params, ret, body }
    }

    /// Recognize a block terminator: `<-;`, `<- else ->`, or `<- else if ... ->`.
    fn parse_block_end(&self, stmt: Statement) -> Option<Result<BlockEnd, Diagnostic>> {
        if !self.starts(stmt, "<-") {
            return None;
        }
        let (s, len) = (stmt.start, stmt.end - stmt.start);
        if len == 2 && self.src.is(s + 1, ";") {
            return Some(Ok(BlockEnd::End));
        }
        if self.src.is(s + 1, "else") {
            if len == 3 && self.src.is(s + 2, "->") {
                return Some(Ok(BlockEnd::Else(stmt)));
            }
            if len > 2 && self.src.is(s + 2, "if") {
                return Some(Ok(BlockEnd::ElseIf(stmt, Statement { start: s + 3, end: stmt.end })));
            }
        }
        let message = "invalid block terminator, use `<-;`, `<- else ->`, or `<- else if a < b ->`";
        Some(Err(self.src.error(s, message)))
    }

    /// Read statements until a block terminator (function body, `if`, `else`, or `while`).
    fn parse_block(&mut self, opener: Statement, what: &str, scope: &mut Scope) -> (Vec<Stmt>, BlockEnd) {
        self.blocks.push(Vec::new());
        let result = self.parse_block_body(opener, what, scope);
        let line = self.last_line();
        self.close_symbols(line);
        result
    }

    fn parse_block_body(&mut self, opener: Statement, what: &str, scope: &mut Scope) -> (Vec<Stmt>, BlockEnd) {
        let mut body = Vec::new();
        while let Some(stmt) = self.next_stmt() {
            match self.parse_block_end(stmt) {
                Some(Ok(end)) => return (body, end),
                Some(Err(e)) => {
                    self.fail(stmt, e);
                    return (body, BlockEnd::End);
                }
                None => {}
            }
            if self.starts(stmt, "use") {
                self.error(stmt, stmt.start, "`use` is only allowed at the top level of a file");
                continue;
            }
            if self.starts(stmt, "fnc") {
                let message = "functions can only be defined at the top level (is a `<-;` missing above?)";
                self.error(stmt, stmt.start, message);
                self.skip_block();
                continue;
            }
            body.extend(self.parse_stmt_line(stmt, scope));
        }
        self.errors.push(self.src.error(opener.start, format!("{} is not closed with `<-;`", what)));
        (body, BlockEnd::End)
    }

    /// A condition (the tokens after `if` / `while` / `else if`, up to the `->`);
    /// on error, it is reported and replaced by `true` so the block still parses.
    fn condition(&mut self, cond: Statement, scope: &mut Scope) -> Expr {
        let result = ExprParser::new(self.src, cond, scope).condition();
        result.unwrap_or_else(|e| {
            self.fail(cond, e);
            Expr::Bool(true)
        })
    }

    /// `if <condition> ->` together with its `else if` / `else` chain.
    fn parse_if(&mut self, opener: Statement, cond: Statement, scope: &mut Scope) -> Stmt {
        let cond = self.condition(cond, scope);

        // Each branch gets its own copy of the scope: variables declared inside
        // a block are not visible outside it.
        let mut then_scope = scope.clone();
        let (body, end) = self.parse_block(opener, "`if` block", &mut then_scope);

        let mut else_scope = scope.clone();
        let else_body = match end {
            BlockEnd::End => Vec::new(),
            BlockEnd::ElseIf(s, cond) => vec![self.parse_if(s, cond, &mut else_scope)],
            BlockEnd::Else(s) => {
                let (body, end) = self.parse_block(s, "`else` block", &mut else_scope);
                if let BlockEnd::Else(q) | BlockEnd::ElseIf(q, _) = end {
                    self.error(q, q.start + 1, "`else` was already used, close the block with `<-;`");
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
    fn parse_while(&mut self, opener: Statement, cond: Statement, scope: &mut Scope) -> Stmt {
        let cond = self.condition(cond, scope);
        let mut body_scope = scope.clone();
        let (body, end) = self.parse_block(opener, "`while` loop", &mut body_scope);
        if let BlockEnd::Else(s) | BlockEnd::ElseIf(s, _) = end {
            self.error(s, s.start + 1, "`else` without `if`");
        }
        // The body may run again: it must not leave outside variables moved.
        if !always_returns(&body) {
            if let Err((line, name, e)) = scope.check_loop(&body_scope) {
                self.error_on_line(line, &name, opener, e);
            }
        }
        Stmt::While { cond, body }
    }

    /// `return;` or `return value;` inside a function.
    fn parse_return(&self, stmt: Statement, scope: &mut Scope) -> Result<Stmt, Diagnostic> {
        let src = self.src;
        let Some(sig) = &self.current else {
            return Err(src.error(stmt.start, "`return` can only be used inside a function"));
        };
        let mut p = ExprParser::new(src, stmt, scope);
        p.pos += 1; // `return`
        match (p.is(";"), sig.ret) {
            (true, None) => {
                p.finish()?;
                Ok(Stmt::Return(None))
            }
            (true, Some(ty)) => {
                Err(src.error(stmt.start, format!("function `{}` must return {} value", sig.name, ty.with_article())))
            }
            (false, None) => Err(src.error(
                stmt.start,
                format!("function `{}` has no return type, so use `return;` without a value", sig.name),
            )),
            (false, Some(ty)) => {
                let at = p.pos;
                let expr = p.value(Access::Move, Some(ty))?;
                p.finish()?;
                check_assign(ty, &expr, &format!("return value of `{}`", sig.name)).map_err(|e| src.error(at, e))?;
                Ok(Stmt::Return(Some(expr)))
            }
        }
    }

    /// One statement. Errors are recorded and the statement is skipped.
    fn parse_stmt_line(&mut self, stmt: Statement, scope: &mut Scope) -> Option<Stmt> {
        let after_keyword = Statement { start: stmt.start + 1, end: stmt.end };
        if self.starts(stmt, "if") {
            return Some(self.parse_if(stmt, after_keyword, scope));
        }
        if self.starts(stmt, "while") {
            return Some(self.parse_while(stmt, after_keyword, scope));
        }
        let is_return = self.starts(stmt, "return");
        let result = if let Some(e) = incomplete(self.src, stmt) {
            Err(e)
        } else if is_return {
            self.parse_return(stmt, scope)
        } else {
            parse_stmt(self.src, stmt, scope)
        };
        match result {
            Ok(s) => {
                if let Stmt::Set { name, ty, keep, borrow, .. } = &s {
                    let kind = match (keep, borrow, scope.kind_of(name)) {
                        (true, _, _) => "keep",
                        (_, true, _) => "borrow",
                        (_, _, Some(Kind::Reuse)) => "reuse",
                        _ => "owned",
                    };
                    let at = declared_name(self.src, stmt);
                    if kind == "reuse" {
                        self.warn_useless_reuse(at, *ty);
                    }
                    self.declare_symbol(at, name, *ty, kind);
                }
                Some(s)
            }
            Err(e) => {
                self.fail(stmt, e);
                // A broken `return` still ends its path (no extra "missing `return`" error).
                if is_return {
                    return Some(Stmt::Return(None));
                }
                // Keep a failed `set`'s variable known, so later lines don't report it as undeclared.
                declare_after_error(self.src, stmt, scope);
                None
            }
        }
    }
}

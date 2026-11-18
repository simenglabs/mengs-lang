//! Expressions, parsed by precedence (lowest first):
//!
//! | level      | operators                    |
//! |------------|------------------------------|
//! | or         | `or`                         |
//! | and        | `and`                        |
//! | not        | `not` (prefix)               |
//! | comparison | `= != < > <= >=` (no chains) |
//! | additive   | `+ -`                        |
//! | multiply   | `* / mod`                    |
//! | unary      | `-` (prefix)                 |
//! | primary    | literal, variable, call `F(...)`, built-in, `( ... )` |
//!
//! | postfix    | `xs[i]`                      |
//!
//! Built-ins: `str(x)`, `int(x)`, `float(x)`, `read()`, `read(prompt)`, `len(x)`,
//! and the statement `push(xs, value);`.
//!
//! `ExprParser` is also the token cursor the statement parsers (`stmt.rs`, `mod.rs`) use.

use crate::ast::{BinOp, Expr, Type, UnaryOp};

use super::check::check_call;
use super::diagnostic::{Diagnostic, KEYWORDS, Source, Statement};
use super::lexer::Token;
use super::scope::{Access, Scope};

/// Built-in functions; user functions cannot use these names.
pub const BUILTINS: &[&str] = &["str", "int", "float", "read", "len", "push"];

const EMPTY_ARRAY_HELP: &str = "an empty array `[]` needs a known type; use it directly, e.g. set xs: [int] = [];";

const COMPARISONS: &[(&str, BinOp)] = &[
    ("=", BinOp::Eq),
    ("!=", BinOp::Ne),
    ("<", BinOp::Lt),
    (">", BinOp::Gt),
    ("<=", BinOp::Le),
    (">=", BinOp::Ge),
];

type Result<T> = std::result::Result<T, Diagnostic>;

/// A cursor over the tokens of one statement, with the scope to check names against.
/// Errors point at the token they are about.
pub struct ExprParser<'p, 's> {
    pub src: &'p Source,
    /// First token of the statement (for messages).
    start: usize,
    pub pos: usize,
    /// End of the statement (exclusive): the cursor never reads past it.
    pub end: usize,
    pub scope: &'s mut Scope,
}

impl<'p, 's> ExprParser<'p, 's> {
    pub fn new(src: &'p Source, stmt: Statement, scope: &'s mut Scope) -> ExprParser<'p, 's> {
        ExprParser { src, start: stmt.start, pos: stmt.start, end: stmt.end, scope }
    }

    pub fn peek(&self) -> Option<&'p Token> {
        self.peek_at(0)
    }

    /// The token `k` places ahead, if it is still in the statement.
    pub fn peek_at(&self, k: usize) -> Option<&'p Token> {
        let i = self.pos + k;
        if i < self.end { self.src.token(i) } else { None }
    }

    /// Whether the token `k` places ahead is the symbol or word `s`.
    pub fn is_at(&self, k: usize, s: &str) -> bool {
        self.pos + k < self.end && self.src.is(self.pos + k, s)
    }

    pub fn is(&self, s: &str) -> bool {
        self.is_at(0, s)
    }

    /// Consume the next token if it is the symbol or word `s`.
    pub fn eat(&mut self, s: &str) -> bool {
        let hit = self.is(s);
        if hit {
            self.pos += 1;
        }
        hit
    }

    /// An error at token `i` (clamped to the statement).
    pub fn error_at(&self, i: usize, message: impl Into<String>) -> Diagnostic {
        self.src.error(i.min(self.end.saturating_sub(1)), message)
    }

    /// An error at the current token.
    pub fn error(&self, message: impl Into<String>) -> Diagnostic {
        self.error_at(self.pos, message)
    }

    /// Line of token `i` (for ownership events and "moved on line N").
    pub fn line(&self, i: usize) -> usize {
        self.src.line(i.min(self.end.saturating_sub(1)))
    }

    /// Source text of the tokens `from..to`.
    pub fn text(&self, from: usize, to: usize) -> &'p str {
        self.src.text(from, to)
    }

    /// The current token is not what the statement needs here.
    pub fn unexpected(&self) -> Diagnostic {
        let stmt = self.text(self.start, self.end);
        match self.peek() {
            Some(t) => self.error(format!("unexpected `{}` in `{}`", t, stmt)),
            None => self.error(format!("unexpected end of `{}`", stmt)),
        }
    }

    /// The statement ends here with its `;`.
    pub fn finish(&mut self) -> Result<()> {
        if self.is(";") && self.pos + 1 == self.end {
            self.pos += 1;
            return Ok(());
        }
        Err(self.unexpected())
    }

    /// A name (not a keyword). Returns its token index and the name.
    pub fn name(&mut self) -> Result<(usize, String)> {
        let at = self.pos;
        match self.peek() {
            Some(Token::Ident(word)) => {
                let name = parse_identifier(word).map_err(|e| self.error(e))?;
                self.pos += 1;
                Ok((at, name))
            }
            Some(t) => Err(self.error(format!("invalid name: `{}`", t))),
            None => Err(self.unexpected()),
        }
    }

    /// A type up to one of the `stops` (outside brackets), or up to a `;` or `->`:
    /// `int`, `[str]`, ...
    pub fn type_until(&mut self, stops: &[&str]) -> Result<Type> {
        let from = self.pos;
        let mut depth = 0usize;
        let stop = |p: &Self| p.is(";") || p.is("->") || stops.iter().any(|s| p.is(s));
        while self.pos < self.end && !(depth == 0 && stop(self)) {
            match self.peek() {
                Some(Token::Sym("[")) => depth += 1,
                Some(Token::Sym("]")) => depth = depth.saturating_sub(1),
                _ => {}
            }
            self.pos += 1;
        }
        let text = self.text(from, self.pos);
        Type::parse(text).ok_or_else(|| {
            let message = format!(
                "unknown type `{}` (expected str, int, float, bool, or an array of one of them like [int])",
                text
            );
            self.error_at(from, message)
        })
    }

    /// A value stored into a slot (a variable, return value, or array item). A bare
    /// `[]` takes its array type from the slot (`expected`). With `Access::Move`, a
    /// bare variable is moved (`set y: T = x;`).
    pub fn value(&mut self, access: Access, expected: Option<Type>) -> Result<Expr> {
        if let Some(ty @ Type::Array(_)) = expected {
            if self.is("[") && self.is_at(1, "]") && self.is_at(2, ";") {
                self.pos += 2;
                return Ok(Expr::Array { items: Vec::new(), ty });
            }
        }
        let at = self.pos;
        match self.or()? {
            Expr::Var { name, ty, .. } if access == Access::Move => self.take(at, name, ty),
            expr => Ok(expr),
        }
    }

    /// An `if` / `while` condition: a bool expression followed by the final `->`.
    pub fn condition(&mut self) -> Result<Expr> {
        if !self.src.is(self.end - 1, "->") {
            return Err(self.error_at(self.end - 1, "a condition must end with `->`, example: if a < b ->"));
        }
        let at = self.pos;
        let expr = self.or()?;
        if !(self.is("->") && self.pos + 1 == self.end) {
            return Err(self.unexpected());
        }
        self.pos += 1;
        if expr.ty() != Type::Bool {
            return Err(self.error_at(at, format!("a condition must be bool, got {}", expr.ty().name())));
        }
        Ok(expr)
    }

    /// `push(xs, value);` (the cursor is at `push`). Returns the array name, the value, and the element type.
    pub fn push(&mut self) -> Result<(String, Expr, Type)> {
        let usage = "invalid push, example: push(xs, 4);";
        self.pos += 2; // `push` `(`
        let name_at = self.pos;
        let name = match self.peek() {
            Some(Token::Ident(name)) => parse_identifier(name).map_err(|e| self.error(e))?,
            _ => return Err(self.error(usage)),
        };
        self.pos += 1;
        if !self.eat(",") {
            return Err(self.error(usage));
        }
        // A bare variable pushed into an array is moved into it.
        let value_at = self.pos;
        let value = match self.or()? {
            Expr::Var { name, ty, .. } => self.take(value_at, name, ty)?,
            value => value,
        };
        if !self.eat(")") || !self.is(";") {
            return Err(self.error(usage));
        }
        self.finish()?;
        let ty = self.scope.mutate(&name, self.line(name_at)).map_err(|e| self.error_at(name_at, e))?;
        let elem = ty.elem().ok_or_else(|| {
            self.error_at(name_at, format!("`push` needs an array, but `{}` is {}", name, ty.with_article()))
        })?;
        if !elem.accepts(value.ty()) {
            let message = format!("cannot push {} value into `{}` ({})", value.ty().with_article(), name, ty.name());
            return Err(self.error_at(value_at, message));
        }
        Ok((name, value, elem))
    }

    /// A call used as a statement: `Name(args);` (the function may have no return
    /// value). Returns the name and the arguments (defaults included).
    pub fn call_statement(&mut self) -> Result<(String, Vec<Expr>)> {
        let (name_at, name) = self.name()?;
        if !self.eat("(") {
            return Err(self.unexpected());
        }
        let (args, _) = self.call(name_at, &name)?;
        self.finish()?;
        Ok((name, args))
    }

    pub fn or(&mut self) -> Result<Expr> {
        let mut lhs = self.and()?;
        while self.is("or") {
            let op = self.pos;
            self.pos += 1;
            let rhs = self.and()?;
            lhs = logical(BinOp::Or, lhs, rhs).map_err(|e| self.error_at(op, e))?;
        }
        Ok(lhs)
    }

    fn and(&mut self) -> Result<Expr> {
        let mut lhs = self.not()?;
        while self.is("and") {
            let op = self.pos;
            self.pos += 1;
            let rhs = self.not()?;
            lhs = logical(BinOp::And, lhs, rhs).map_err(|e| self.error_at(op, e))?;
        }
        Ok(lhs)
    }

    fn not(&mut self) -> Result<Expr> {
        let op = self.pos;
        if !self.eat("not") {
            return self.comparison();
        }
        let expr = self.not()?;
        if expr.ty() != Type::Bool {
            return Err(self.error_at(op, format!("`not` needs a bool value, got {}", expr.ty().name())));
        }
        Ok(Expr::Unary { op: UnaryOp::Not, expr: Box::new(expr) })
    }

    fn comparison(&mut self) -> Result<Expr> {
        let lhs = self.additive()?;
        let at = self.pos;
        let Some(op) = self.comparison_op() else {
            return Ok(lhs);
        };
        let rhs = self.additive()?;
        if self.comparison_op().is_some() {
            return Err(self.error_at(self.pos - 1, "comparisons cannot be chained, use `and`: a < b and b < c"));
        }
        let (lt, rt) = (lhs.ty(), rhs.ty());
        if lt.elem().is_some() || rt.elem().is_some() {
            return Err(self.error_at(at, "arrays cannot be compared"));
        }
        if lt != rt && !(lt.is_numeric() && rt.is_numeric()) {
            return Err(self.error_at(at, format!("cannot compare {} with {}", lt.name(), rt.name())));
        }
        if lt == Type::Bool && !matches!(op, BinOp::Eq | BinOp::Ne) {
            return Err(self.error_at(at, "bool values can only be compared with `=` or `!=`"));
        }
        Ok(binary(op, lhs, rhs))
    }

    fn comparison_op(&mut self) -> Option<BinOp> {
        let op = COMPARISONS.iter().find(|(s, _)| self.peek() == Some(&Token::Sym(s)))?.1;
        self.pos += 1;
        Some(op)
    }

    fn additive(&mut self) -> Result<Expr> {
        let mut lhs = self.multiply()?;
        loop {
            let at = self.pos;
            let op = if self.eat("+") {
                BinOp::Add
            } else if self.eat("-") {
                BinOp::Sub
            } else {
                return Ok(lhs);
            };
            let rhs = self.multiply()?;
            lhs = arithmetic(op, lhs, rhs).map_err(|e| self.error_at(at, e))?;
        }
    }

    fn multiply(&mut self) -> Result<Expr> {
        let mut lhs = self.unary()?;
        loop {
            let at = self.pos;
            let op = if self.eat("*") {
                BinOp::Mul
            } else if self.eat("/") {
                BinOp::Div
            } else if self.eat("mod") {
                BinOp::Mod
            } else {
                return Ok(lhs);
            };
            let rhs = self.unary()?;
            lhs = arithmetic(op, lhs, rhs).map_err(|e| self.error_at(at, e))?;
        }
    }

    fn unary(&mut self) -> Result<Expr> {
        let at = self.pos;
        if !self.eat("-") {
            return self.postfix();
        }
        // `-5` is a negative literal (so it can be a parameter default).
        match self.peek() {
            Some(&Token::Int(n)) => {
                self.pos += 1;
                Ok(Expr::Int(-n))
            }
            Some(&Token::Float(x)) => {
                self.pos += 1;
                Ok(Expr::Float(-x))
            }
            _ => {
                let expr = self.unary()?;
                if !expr.ty().is_numeric() {
                    return Err(self.error_at(at, format!("`-` needs a number, got {}", expr.ty().name())));
                }
                Ok(Expr::Unary { op: UnaryOp::Neg, expr: Box::new(expr) })
            }
        }
    }

    /// `value[index]`, possibly repeated.
    fn postfix(&mut self) -> Result<Expr> {
        let at = self.pos;
        let mut expr = self.primary()?;
        while self.eat("[") {
            let index_at = self.pos;
            let index = self.or()?;
            if !self.eat("]") {
                return Err(self.error("missing `]`"));
            }
            let Some(ty) = expr.ty().elem() else {
                return Err(self.error_at(at, format!("only arrays can be indexed, got {}", expr.ty().name())));
            };
            if index.ty() != Type::Int {
                return Err(self.error_at(index_at, format!("an array index must be int, got {}", index.ty().name())));
            }
            expr = Expr::Index { array: Box::new(expr), index: Box::new(index), ty };
        }
        Ok(expr)
    }

    /// `[a, b, c]` (after the `[` at `open`). Ints and floats mix into a `[float]`.
    fn array_literal(&mut self, open: usize) -> Result<Expr> {
        if self.eat("]") {
            return Err(self.error_at(open, EMPTY_ARRAY_HELP));
        }
        let (mut items, mut starts) = (Vec::new(), Vec::new());
        loop {
            starts.push(self.pos);
            items.push(self.or()?);
            if self.eat("]") {
                break;
            }
            if !self.eat(",") {
                return Err(self.error("expected `,` or `]` in the array"));
            }
        }
        let mut elem = items[0].ty();
        for (item, at) in items[1..].iter().zip(&starts[1..]) {
            let ty = item.ty();
            if elem.is_numeric() && ty.is_numeric() {
                if ty == Type::Float {
                    elem = Type::Float;
                }
            } else if ty != elem {
                let message = format!("all items of an array must have the same type, got {} and {}", elem.name(), ty.name());
                return Err(self.error_at(*at, message));
            }
        }
        let ty = Type::array_of(elem).ok_or_else(|| self.error_at(open, "arrays cannot contain arrays"))?;
        Ok(Expr::Array { items, ty })
    }

    fn primary(&mut self) -> Result<Expr> {
        let at = self.pos;
        let Some(token) = self.peek() else {
            return Err(self.error("expected a value"));
        };
        self.pos += 1;
        match token {
            Token::Str(s) => Ok(Expr::Str(s.clone())),
            Token::Int(n) => Ok(Expr::Int(*n)),
            Token::Float(x) => Ok(Expr::Float(*x)),
            Token::Ident(word) => match word.as_str() {
                "true" => Ok(Expr::Bool(true)),
                "false" => Ok(Expr::Bool(false)),
                "str" | "int" | "float" if self.eat("(") => {
                    let to = Type::parse(word).expect("a type name");
                    let expr = self.or()?;
                    self.close_call(word)?;
                    let ok = match to {
                        Type::Str => expr.ty().elem().is_none(),
                        _ => matches!(expr.ty(), Type::Str | Type::Int | Type::Float),
                    };
                    if !ok {
                        return Err(self.error_at(at, format!("cannot convert {} to {}", expr.ty().name(), to.name())));
                    }
                    Ok(Expr::Convert { to, expr: Box::new(expr) })
                }
                "len" if self.eat("(") => {
                    let expr = self.or()?;
                    self.close_call("len")?;
                    if expr.ty() != Type::Str && expr.ty().elem().is_none() {
                        return Err(self.error_at(at, format!("`len` needs a str or an array, got {}", expr.ty().name())));
                    }
                    Ok(Expr::Len(Box::new(expr)))
                }
                "push" if self.eat("(") => Err(self.error_at(at, "`push(xs, value);` is a statement, not a value")),
                "read" if self.eat("(") => {
                    if self.eat(")") {
                        return Ok(Expr::Read { prompt: None });
                    }
                    let prompt_at = self.pos;
                    let prompt = self.or()?;
                    self.close_call("read")?;
                    if prompt.ty() != Type::Str {
                        let message = format!("the prompt of `read` must be a str, got {}", prompt.ty().name());
                        return Err(self.error_at(prompt_at, message));
                    }
                    Ok(Expr::Read { prompt: Some(Box::new(prompt)) })
                }
                _ if self.eat("(") => {
                    parse_identifier(word).map_err(|e| self.error_at(at, e))?;
                    let (args, ret) = self.call(at, word)?;
                    let Some(ty) = ret else {
                        let message =
                            format!("function `{}` does not return a value, so it cannot be used in an expression", word);
                        return Err(self.error_at(at, message));
                    };
                    Ok(Expr::Call { name: word.clone(), args, ty })
                }
                _ => {
                    let name = parse_identifier(word).map_err(|e| self.error_at(at, e))?;
                    let ty = self.scope.access(&name, self.line(at), Access::Read).map_err(|e| self.error_at(at, e))?;
                    Ok(Expr::Var { name, ty, moved: false })
                }
            },
            Token::Sym("[") => self.array_literal(at),
            Token::Sym("(") => {
                let expr = self.or()?;
                if !self.eat(")") {
                    return Err(self.error("missing `)`"));
                }
                Ok(expr)
            }
            t => Err(self.error_at(at, format!("expected a value, got `{}`", t))),
        }
    }

    /// Arguments of `name(...)` (the `(` is already consumed; `name_at` is the name's
    /// token). Bare variables are moved into the function, except for `keep`
    /// parameters, which borrow them. Checks the signature and fills in defaults.
    fn call(&mut self, name_at: usize, name: &str) -> Result<(Vec<Expr>, Option<Type>)> {
        let Some(sig) = self.scope.function(name) else {
            return Err(self.error_at(name_at, format!("function `{}` is not defined", name)));
        };
        // Each parameter's type, and whether it only borrows its argument (`keep` on a str or array).
        let params: Vec<(Type, bool)> = sig.params.iter().map(|p| (p.ty, p.keep && p.ty.is_heap())).collect();
        // The arguments, and the token where each one starts.
        let (mut args, mut starts) = (Vec::new(), Vec::new());
        if !self.eat(")") {
            loop {
                let at = self.pos;
                starts.push(at);
                // A bare `[]` takes the array type of its parameter.
                let empty = self.is("[") && self.is_at(1, "]") && (self.is_at(2, ",") || self.is_at(2, ")"));
                if let (true, Some(ty @ Type::Array(_))) = (empty, params.get(args.len()).map(|p| p.0)) {
                    self.pos += 2;
                    args.push(Expr::Array { items: Vec::new(), ty });
                } else {
                    let borrows = params.get(args.len()).is_some_and(|p| p.1);
                    let arg = match self.or()? {
                        // A `keep` parameter only borrows: the variable stays where it is.
                        Expr::Var { name, ty, .. } if !borrows => self.take(at, name, ty)?,
                        arg => arg,
                    };
                    args.push(arg);
                }
                if self.eat(")") {
                    break;
                }
                if !self.eat(",") {
                    return Err(self.error(format!("expected `,` or `)` in the call to `{}`", name)));
                }
            }
        }
        self.check_borrow_conflicts(name, &args, &params).map_err(|(i, e)| self.error_at(starts[i], e))?;
        let sig = self.scope.function(name).expect("checked above");
        let ret = sig.ret;
        let args = check_call(sig, args).map_err(|(i, e)| self.error_at(i.map_or(name_at, |i| starts[i]), e))?;
        Ok((args, ret))
    }

    /// A value borrowed by a `keep` parameter must stay alive during the whole call, so
    /// no other argument of the same call may move a variable the borrowed one reads
    /// (directly, through a `<==` borrow, or as an element of an array).
    /// The error carries the index of the moved argument.
    fn check_borrow_conflicts(
        &self,
        name: &str,
        args: &[Expr],
        params: &[(Type, bool)],
    ) -> std::result::Result<(), (usize, String)> {
        let mut roots: Vec<String> = Vec::new();
        for (arg, (_, borrows)) in args.iter().zip(params) {
            if *borrows {
                let mut names = Vec::new();
                var_names(arg, &mut names);
                for n in names {
                    roots.push(self.scope.borrow_root(&n).unwrap_or(n));
                }
            }
        }
        for (i, arg) in args.iter().enumerate() {
            if let Expr::Var { name: moved, moved: true, .. } = arg {
                if roots.contains(moved) {
                    return Err((
                        i,
                        format!(
                            "`{}` is moved and borrowed by the same call to `{}`; copy it with `{} ()=> new_name;` first",
                            moved, name, moved
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    /// A bare variable given away (`set y: T = x;`, `F(x)`, `return x;`); `at` is
    /// where the expression starts. Owned variables are moved; `keep`/`reuse`/borrows
    /// are copied.
    pub fn take(&mut self, at: usize, name: String, ty: Type) -> Result<Expr> {
        // `(x)`: point at the name, not at the parenthesis.
        let mut at = at;
        while self.src.is(at, "(") {
            at += 1;
        }
        self.scope.access(&name, self.line(at), Access::Move).map_err(|e| self.error_at(at, e))?;
        let moved = self.scope.is_owned(&name);
        Ok(Expr::Var { name, ty, moved })
    }

    /// The `)` that closes a built-in call.
    fn close_call(&mut self, name: &str) -> Result<()> {
        if self.eat(")") { Ok(()) } else { Err(self.error(format!("`{}` takes one value, expected `)`", name))) }
    }
}

fn binary(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }
}

/// `+ - * /` need numbers; `mod` needs two ints.
fn arithmetic(op: BinOp, lhs: Expr, rhs: Expr) -> std::result::Result<Expr, String> {
    let (lt, rt) = (lhs.ty(), rhs.ty());
    if op == BinOp::Add && (lt == Type::Str || rt == Type::Str) {
        if lt == rt {
            return Ok(binary(op, lhs, rhs));
        }
        return Err(format!(
            "`+` joins two strings or adds two numbers, got {} and {}; convert with str(...)",
            lt.name(),
            rt.name()
        ));
    }
    let ok = match op {
        BinOp::Mod => lt == Type::Int && rt == Type::Int,
        _ => lt.is_numeric() && rt.is_numeric(),
    };
    if !ok {
        let needs = if op == BinOp::Mod { "int values" } else { "numbers" };
        return Err(format!("`{}` needs {}, got {} and {}", op.symbol(), needs, lt.name(), rt.name()));
    }
    Ok(binary(op, lhs, rhs))
}

/// `and` / `or` need two bools.
fn logical(op: BinOp, lhs: Expr, rhs: Expr) -> std::result::Result<Expr, String> {
    let (lt, rt) = (lhs.ty(), rhs.ty());
    if lt != Type::Bool || rt != Type::Bool {
        return Err(format!("`{}` needs bool values, got {} and {}", op.symbol(), lt.name(), rt.name()));
    }
    Ok(binary(op, lhs, rhs))
}

pub fn parse_identifier(s: &str) -> std::result::Result<String, String> {
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

/// Every variable an expression reads.
fn var_names(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Var { name, .. } => out.push(name.clone()),
        Expr::Str(_) | Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) => {}
        Expr::Unary { expr, .. } | Expr::Convert { expr, .. } | Expr::Len(expr) => var_names(expr, out),
        Expr::Binary { lhs, rhs, .. } => {
            var_names(lhs, out);
            var_names(rhs, out);
        }
        Expr::Call { args, .. } => args.iter().for_each(|a| var_names(a, out)),
        Expr::Array { items, .. } => items.iter().for_each(|a| var_names(a, out)),
        Expr::Index { array, index, .. } => {
            var_names(array, out);
            var_names(index, out);
        }
        Expr::Read { prompt } => {
            if let Some(p) = prompt {
                var_names(p, out);
            }
        }
    }
}

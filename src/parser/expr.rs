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
//! Built-ins: `str(x)`, `int(x)`, `float(x)`, `read()`, `read(prompt)`.

use crate::ast::{BinOp, Expr, Type, UnaryOp};

use super::check::check_call;
use super::lexer::{Token, tokenize};
use super::scope::{Access, Scope};

const KEYWORDS: &[&str] = &[
    "set", "keep", "reuse", "show", "fnc", "if", "else", "while", "return", "true", "false", "and", "or",
    "not", "mod",
];

/// Built-in functions; user functions cannot use these names.
pub const BUILTINS: &[&str] = &["str", "int", "float", "read"];

const COMPARISONS: &[(&str, BinOp)] = &[
    ("=", BinOp::Eq),
    ("!=", BinOp::Ne),
    ("<", BinOp::Lt),
    (">", BinOp::Gt),
    ("<=", BinOp::Le),
    (">=", BinOp::Ge),
];

/// Parse and type-check an expression.
///
/// Ownership: variables inside an expression are only read. Only a *bare*
/// variable (`set y: int = x;`, `F(x)`) is moved, and only when `access` is `Move`.
pub fn parse_expr(src: &str, scope: &mut Scope, line: usize, access: Access) -> Result<Expr, String> {
    let mut p = ExprParser { tokens: tokenize(src)?, pos: 0, scope, line };
    let expr = p.or()?;
    if let Some(t) = p.tokens.get(p.pos) {
        return Err(format!("unexpected `{}` in `{}`", t, src));
    }
    match expr {
        Expr::Var { name, ty, .. } if access == Access::Move => Ok(p.take(name, ty)?),
        expr => Ok(expr),
    }
}

/// A call used as a statement: `Name(args)` (without `;`). The function may
/// have no return value. Returns the name and the arguments (defaults included).
pub fn parse_call(src: &str, scope: &mut Scope, line: usize) -> Result<(String, Vec<Expr>), String> {
    let mut p = ExprParser { tokens: tokenize(src)?, pos: 0, scope, line };
    let name = match p.tokens.first() {
        Some(Token::Ident(name)) => parse_identifier(name)?,
        _ => return Err(format!("invalid call: `{}`", src)),
    };
    p.pos = 1;
    if !p.eat("(") {
        return Err(format!("invalid call: `{}`", src));
    }
    let (args, _) = p.call(&name)?;
    if let Some(t) = p.tokens.get(p.pos) {
        return Err(format!("unexpected `{}` in `{}`", t, src));
    }
    Ok((name, args))
}

/// `if` / `while` condition (after the keyword, including the trailing `->`).
pub fn parse_condition(cond: &str, scope: &mut Scope, line: usize) -> Result<Expr, String> {
    let usage = "a condition must end with `->`, example: if a < b ->";
    let cond = cond.strip_suffix("->").ok_or(usage)?.trim();
    let expr = parse_expr(cond, scope, line, Access::Read)?;
    if expr.ty() != Type::Bool {
        return Err(format!("a condition must be bool, got {}", expr.ty().name()));
    }
    Ok(expr)
}

struct ExprParser<'s> {
    tokens: Vec<Token>,
    pos: usize,
    scope: &'s mut Scope,
    line: usize,
}

impl ExprParser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    /// Consume the next token if it is the symbol or word `s`.
    fn eat(&mut self, s: &str) -> bool {
        let hit = match self.peek() {
            Some(Token::Sym(sym)) => *sym == s,
            Some(Token::Ident(word)) => word == s,
            _ => false,
        };
        if hit {
            self.pos += 1;
        }
        hit
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut lhs = self.and()?;
        while self.eat("or") {
            lhs = logical(BinOp::Or, lhs, self.and()?)?;
        }
        Ok(lhs)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut lhs = self.not()?;
        while self.eat("and") {
            lhs = logical(BinOp::And, lhs, self.not()?)?;
        }
        Ok(lhs)
    }

    fn not(&mut self) -> Result<Expr, String> {
        if !self.eat("not") {
            return self.comparison();
        }
        let expr = self.not()?;
        if expr.ty() != Type::Bool {
            return Err(format!("`not` needs a bool value, got {}", expr.ty().name()));
        }
        Ok(Expr::Unary { op: UnaryOp::Not, expr: Box::new(expr) })
    }

    fn comparison(&mut self) -> Result<Expr, String> {
        let lhs = self.additive()?;
        let Some(op) = self.comparison_op() else {
            return Ok(lhs);
        };
        let rhs = self.additive()?;
        if self.comparison_op().is_some() {
            return Err("comparisons cannot be chained, use `and`: a < b and b < c".into());
        }
        let (lt, rt) = (lhs.ty(), rhs.ty());
        if lt != rt && !(lt.is_numeric() && rt.is_numeric()) {
            return Err(format!("cannot compare {} with {}", lt.name(), rt.name()));
        }
        if lt == Type::Bool && !matches!(op, BinOp::Eq | BinOp::Ne) {
            return Err("bool values can only be compared with `=` or `!=`".into());
        }
        Ok(binary(op, lhs, rhs))
    }

    fn comparison_op(&mut self) -> Option<BinOp> {
        let op = COMPARISONS.iter().find(|(s, _)| self.peek() == Some(&Token::Sym(s)))?.1;
        self.pos += 1;
        Some(op)
    }

    fn additive(&mut self) -> Result<Expr, String> {
        let mut lhs = self.multiply()?;
        loop {
            let op = if self.eat("+") {
                BinOp::Add
            } else if self.eat("-") {
                BinOp::Sub
            } else {
                return Ok(lhs);
            };
            lhs = arithmetic(op, lhs, self.multiply()?)?;
        }
    }

    fn multiply(&mut self) -> Result<Expr, String> {
        let mut lhs = self.unary()?;
        loop {
            let op = if self.eat("*") {
                BinOp::Mul
            } else if self.eat("/") {
                BinOp::Div
            } else if self.eat("mod") {
                BinOp::Mod
            } else {
                return Ok(lhs);
            };
            lhs = arithmetic(op, lhs, self.unary()?)?;
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if !self.eat("-") {
            return self.primary();
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
                    return Err(format!("`-` needs a number, got {}", expr.ty().name()));
                }
                Ok(Expr::Unary { op: UnaryOp::Neg, expr: Box::new(expr) })
            }
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let token = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        match token {
            Some(Token::Str(s)) => Ok(Expr::Str(s)),
            Some(Token::Int(n)) => Ok(Expr::Int(n)),
            Some(Token::Float(x)) => Ok(Expr::Float(x)),
            Some(Token::Ident(word)) => match word.as_str() {
                "true" => Ok(Expr::Bool(true)),
                "false" => Ok(Expr::Bool(false)),
                "str" | "int" | "float" if self.eat("(") => {
                    let to = Type::parse(&word).expect("a type name");
                    let expr = self.or()?;
                    self.close_call(&word)?;
                    let ok = match to {
                        Type::Str => true,
                        _ => matches!(expr.ty(), Type::Str | Type::Int | Type::Float),
                    };
                    if !ok {
                        return Err(format!("cannot convert {} to {}", expr.ty().name(), to.name()));
                    }
                    Ok(Expr::Convert { to, expr: Box::new(expr) })
                }
                "read" if self.eat("(") => {
                    if self.eat(")") {
                        return Ok(Expr::Read { prompt: None });
                    }
                    let prompt = self.or()?;
                    self.close_call("read")?;
                    if prompt.ty() != Type::Str {
                        return Err(format!("the prompt of `read` must be a str, got {}", prompt.ty().name()));
                    }
                    Ok(Expr::Read { prompt: Some(Box::new(prompt)) })
                }
                _ if self.eat("(") => {
                    parse_identifier(&word)?;
                    let (args, ret) = self.call(&word)?;
                    let ty = ret.ok_or_else(|| {
                        format!("function `{}` does not return a value, so it cannot be used in an expression", word)
                    })?;
                    Ok(Expr::Call { name: word, args, ty })
                }
                _ => {
                    let name = parse_identifier(&word)?;
                    let ty = self.scope.access(&name, self.line, Access::Read)?;
                    Ok(Expr::Var { name, ty, moved: false })
                }
            },
            Some(Token::Sym("(")) => {
                let expr = self.or()?;
                if !self.eat(")") {
                    return Err("missing `)`".into());
                }
                Ok(expr)
            }
            Some(t) => Err(format!("expected a value, got `{}`", t)),
            None => Err("expected a value".into()),
        }
    }
}

impl ExprParser<'_> {
    /// Arguments of `name(...)` (the `(` is already consumed). Bare variables are
    /// moved into the function. Checks the signature and fills in defaults.
    fn call(&mut self, name: &str) -> Result<(Vec<Expr>, Option<Type>), String> {
        if self.scope.function(name).is_none() {
            return Err(format!("function `{}` is not defined", name));
        }
        let mut args = Vec::new();
        if !self.eat(")") {
            loop {
                let arg = match self.or()? {
                    Expr::Var { name, ty, .. } => self.take(name, ty)?,
                    arg => arg,
                };
                args.push(arg);
                if self.eat(")") {
                    break;
                }
                if !self.eat(",") {
                    return Err(format!("expected `,` or `)` in the call to `{}`", name));
                }
            }
        }
        let sig = self.scope.function(name).expect("checked above");
        let ret = sig.ret;
        Ok((check_call(sig, args)?, ret))
    }

    /// A bare variable given away (`set y: T = x;`, `F(x)`, `return x;`).
    /// Owned variables are moved; `keep`/`reuse`/borrows are copied.
    fn take(&mut self, name: String, ty: Type) -> Result<Expr, String> {
        self.scope.access(&name, self.line, Access::Move)?;
        let moved = self.scope.is_owned(&name);
        Ok(Expr::Var { name, ty, moved })
    }

    /// The `)` that closes a built-in call.
    fn close_call(&mut self, name: &str) -> Result<(), String> {
        if self.eat(")") { Ok(()) } else { Err(format!("`{}` takes one value, expected `)`", name)) }
    }
}

fn binary(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }
}

/// `+ - * /` need numbers; `mod` needs two ints.
fn arithmetic(op: BinOp, lhs: Expr, rhs: Expr) -> Result<Expr, String> {
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
fn logical(op: BinOp, lhs: Expr, rhs: Expr) -> Result<Expr, String> {
    let (lt, rt) = (lhs.ty(), rhs.ty());
    if lt != Type::Bool || rt != Type::Bool {
        return Err(format!("`{}` needs bool values, got {} and {}", op.symbol(), lt.name(), rt.name()));
    }
    Ok(binary(op, lhs, rhs))
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

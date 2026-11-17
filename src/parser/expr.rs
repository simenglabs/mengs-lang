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
//! | primary    | literal, variable, `( ... )` |

use crate::ast::{BinOp, Expr, Type, UnaryOp};

use super::lexer::{Token, tokenize};
use super::scope::{Access, Scope};

const KEYWORDS: &[&str] = &[
    "set", "keep", "reuse", "show", "fnc", "if", "else", "true", "false", "and", "or", "not", "mod",
];

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
    if let (Expr::Var { name, .. }, Access::Move) = (&expr, access) {
        p.scope.access(name, line, Access::Move)?;
    }
    Ok(expr)
}

/// `if` condition (after `if ` is stripped, including the trailing `->`).
pub fn parse_condition(cond: &str, scope: &mut Scope, line: usize) -> Result<Expr, String> {
    let usage = "invalid if syntax, example: if a < b -> or if active ->";
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
                _ => {
                    let name = parse_identifier(&word)?;
                    let ty = self.scope.access(&name, self.line, Access::Read)?;
                    Ok(Expr::Var { name, ty })
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

fn binary(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }
}

/// `+ - * /` need numbers; `mod` needs two ints.
fn arithmetic(op: BinOp, lhs: Expr, rhs: Expr) -> Result<Expr, String> {
    let (lt, rt) = (lhs.ty(), rhs.ty());
    if op == BinOp::Add && lt == Type::Str && rt == Type::Str {
        return Err("joining strings with `+` is not supported yet".into());
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

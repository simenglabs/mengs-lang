//! Tokenizer for expressions: `a + 2 * (b - 1.5) >= 10 and not done`.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// String literal, escapes already processed.
    Str(String),
    Int(i64),
    Float(f64),
    /// Names and word keywords (`and`, `or`, `not`, `mod`, `true`, ...).
    Ident(String),
    /// Operators, parentheses, and commas.
    Sym(&'static str),
}

/// Two-character symbols must come before their one-character prefixes.
const SYMBOLS: &[&str] = &["<=", ">=", "!=", "+", "-", "*", "/", "=", "<", ">", "(", ")", ","];

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Str(s) => write!(f, "{:?}", s),
            Token::Int(n) => write!(f, "{}", n),
            Token::Float(x) => write!(f, "{:?}", x),
            Token::Ident(s) => f.write_str(s),
            Token::Sym(s) => f.write_str(s),
        }
    }
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut rest = src;
    loop {
        rest = rest.trim_start();
        let Some(c) = rest.chars().next() else {
            return Ok(tokens);
        };
        let (token, len) = if c == '"' {
            lex_string(rest)?
        } else if c.is_ascii_digit() {
            lex_number(rest)?
        } else if c.is_ascii_alphabetic() || c == '_' {
            let len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            (Token::Ident(rest[..len].to_string()), len)
        } else if let Some(sym) = SYMBOLS.iter().find(|s| rest.starts_with(**s)) {
            (Token::Sym(sym), sym.len())
        } else {
            return Err(format!("unexpected character `{}`", c));
        };
        tokens.push(token);
        rest = &rest[len..];
    }
}

/// `42` or `3.14` (a `.` must be followed by a digit to make a float).
fn lex_number(s: &str) -> Result<(Token, usize), String> {
    let digits = |s: &str| s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let mut len = digits(s);
    let is_float = s[len..].starts_with('.') && s[len + 1..].starts_with(|c: char| c.is_ascii_digit());
    if is_float {
        len += 1 + digits(&s[len + 1..]);
    }
    let text = &s[..len];
    let token = if is_float {
        text.parse().ok().map(Token::Float)
    } else {
        text.parse().ok().map(Token::Int)
    };
    token.map(|t| (t, len)).ok_or_else(|| format!("invalid number: `{}`", text))
}

/// A quoted string with the escapes `\"`, `\\`, `\n`, `\t`, `\r`.
fn lex_string(s: &str) -> Result<(Token, usize), String> {
    let mut out = String::new();
    let mut chars = s.char_indices().skip(1);
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Ok((Token::Str(out), i + 1)),
            '\\' => out.push(match chars.next().map(|(_, c)| c) {
                Some('"') => '"',
                Some('\\') => '\\',
                Some('n') => '\n',
                Some('t') => '\t',
                Some('r') => '\r',
                Some(other) => return Err(format!("unknown escape sequence: `\\{}`", other)),
                None => break,
            }),
            _ => out.push(c),
        }
    }
    Err("string is not closed with a quote `\"`".to_string())
}

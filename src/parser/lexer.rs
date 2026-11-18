//! Tokenizer for expressions: `a + 2 * (b - 1.5) >= 10 and not done`.

use std::fmt;

use super::diagnostic::{Diagnostic, Severity};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// String literal, escapes already processed.
    Str(String),
    Int(i64),
    Float(f64),
    /// Names and word keywords (`and`, `or`, `not`, `mod`, `true`, ...).
    Ident(String),
    /// Operators, brackets, and commas.
    Sym(&'static str),
}

/// Two-character symbols must come before their one-character prefixes.
const SYMBOLS: &[&str] = &["<=", ">=", "!=", "+", "-", "*", "/", "=", "<", ">", "(", ")", "[", "]", ","];

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
    lex_number_at(s).map_err(|(message, _)| message)
}

/// Like `lex_number`; the error carries the byte offset of the problem.
fn lex_number_at(s: &str) -> Result<(Token, usize), (String, usize)> {
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
    token.map(|t| (t, len)).ok_or_else(|| (format!("invalid number: `{}`", text), 0))
}

/// A quoted string with the escapes `\"`, `\\`, `\n`, `\t`, `\r`.
fn lex_string(s: &str) -> Result<(Token, usize), String> {
    lex_string_at(s).map_err(|(message, _)| message)
}

/// Like `lex_string`; the error carries the byte offset of the problem
/// (an unknown escape points at its backslash, an unclosed string at the opening quote).
fn lex_string_at(s: &str) -> Result<(Token, usize), (String, usize)> {
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
                Some(other) => return Err((format!("unknown escape sequence: `\\{}`", other), i)),
                None => break,
            }),
            _ => out.push(c),
        }
    }
    Err(("string is not closed with a quote `\"`".to_string(), 0))
}

/// Symbols that only `tokenize_source` knows; longest first, tried before `SYMBOLS`.
const STATEMENT_SYMBOLS: &[&str] = &["()=>", "<==", "->", ";", ":"];

/// A token with its 1-based line and column (columns count bytes).
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // used by the statement parser (Task 5)
pub struct Spanned {
    pub token: Token,
    pub line: usize,
    pub column: usize,
}

/// Tokenize a whole comment-free source. `<-` is one token only at the start of a
/// statement (no previous token, or after `;` or `->`); elsewhere `a <-1` is `a < -1`.
#[allow(dead_code)] // used by the statement parser (Task 5)
pub fn tokenize_source(code: &str) -> Result<Vec<Spanned>, Diagnostic> {
    let mut tokens: Vec<Spanned> = Vec::new();
    let (mut pos, mut line, mut line_start) = (0, 1, 0);
    while pos < code.len() {
        let rest = &code[pos..];
        let c = rest.chars().next().unwrap();
        if c == '\n' {
            line += 1;
            pos += 1;
            line_start = pos;
            continue;
        }
        if c.is_whitespace() {
            pos += c.len_utf8();
            continue;
        }
        let column = pos - line_start + 1;
        let fail = |message: String, offset: usize| Diagnostic {
            line,
            column: column + offset,
            message,
            severity: Severity::Error,
        };
        let starts_statement =
            matches!(tokens.last(), None | Some(Spanned { token: Token::Sym(";" | "->"), .. }));
        let lexed = if c == '"' {
            // A string literal cannot span lines.
            let line_end = rest.find('\n').unwrap_or(rest.len());
            lex_string_at(&rest[..line_end])
        } else if c.is_ascii_digit() {
            lex_number_at(rest)
        } else if c.is_ascii_alphabetic() || c == '_' {
            let len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            Ok((Token::Ident(rest[..len].to_string()), len))
        } else if starts_statement && rest.starts_with("<-") {
            Ok((Token::Sym("<-"), 2))
        } else if let Some(sym) = STATEMENT_SYMBOLS
            .iter()
            .chain(SYMBOLS)
            .find(|s| rest.starts_with(**s))
        {
            Ok((Token::Sym(sym), sym.len()))
        } else {
            Err((format!("unexpected character `{}`", c), 0))
        };
        let (token, len) = lexed.map_err(|(message, offset)| fail(message, offset))?;
        tokens.push(Spanned { token, line, column });
        pos += len;
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sp(code: &str) -> Vec<(Token, usize, usize)> {
        tokenize_source(code).unwrap().into_iter().map(|s| (s.token, s.line, s.column)).collect()
    }

    fn toks(code: &str) -> Vec<Token> {
        sp(code).into_iter().map(|(t, _, _)| t).collect()
    }

    fn id(s: &str) -> Token {
        Token::Ident(s.to_string())
    }

    #[test]
    fn spans_are_one_based_line_and_column() {
        let got = sp("set a: int = 1;\n  show(a);");
        let want = vec![
            (id("set"), 1, 1),
            (id("a"), 1, 5),
            (Token::Sym(":"), 1, 6),
            (id("int"), 1, 8),
            (Token::Sym("="), 1, 12),
            (Token::Int(1), 1, 14),
            (Token::Sym(";"), 1, 15),
            (id("show"), 2, 3),
            (Token::Sym("("), 2, 7),
            (id("a"), 2, 8),
            (Token::Sym(")"), 2, 9),
            (Token::Sym(";"), 2, 10),
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn statement_symbols() {
        let got = toks("a ()=> b;\nset c: str <== a;\nif x ->\n<- else ->\n<-;");
        let syms: Vec<&str> = got
            .iter()
            .filter_map(|t| match t {
                Token::Sym(s) if ["()=>", "<==", "->", "<-"].contains(s) => Some(*s),
                _ => None,
            })
            .collect();
        assert_eq!(syms, vec!["()=>", "<==", "->", "<-", "->", "<-"]);
        assert!(got.contains(&id("else")));
    }

    #[test]
    fn less_than_negative_is_not_a_block_end() {
        assert_eq!(
            toks("if a <-1 ->"),
            vec![id("if"), id("a"), Token::Sym("<"), Token::Sym("-"), Token::Int(1), Token::Sym("->")]
        );
    }

    #[test]
    fn lexing_errors_have_positions() {
        let e = tokenize_source("show(\"abc);").unwrap_err();
        assert_eq!((e.line, e.column), (1, 6));
        assert_eq!(e.message, "string is not closed with a quote `\"`");
        let e = tokenize_source("x @ y").unwrap_err();
        assert_eq!((e.line, e.column), (1, 3));
        assert_eq!(e.message, "unexpected character `@`");
    }
}

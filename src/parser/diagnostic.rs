//! Errors with a line and column, the tokens of a file, and splitting them into statements.

use std::fmt;

use super::lexer::{Spanned, Token, tokenize_source};

/// Whether a diagnostic stops compilation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

/// A parse error or warning. `Display` gives `Line N: message` (the column is shown separately).
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub line: usize,
    /// 1-based column.
    pub column: usize,
    pub message: String,
    pub severity: Severity,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Line {}: {}", self.line, self.message)
    }
}

/// Words that cannot be names (and so never end a value at the end of a line).
pub const KEYWORDS: &[&str] = &[
    "set", "keep", "reuse", "show", "fnc", "use", "if", "else", "while", "return", "true", "false", "and", "or",
    "not", "mod",
];

/// A comment-free file as tokens, with its text (for messages like "unrecognized statement: `...`").
pub struct Source {
    code: String,
    pub tokens: Vec<Spanned>,
    /// Byte offset where each line starts.
    line_starts: Vec<usize>,
    /// Lines with a lexing error: other errors on statements touching them are not reported.
    bad_lines: Vec<usize>,
}

impl Source {
    /// Tokenize a comment-free file. A lexing error is reported, the rest of its line is
    /// dropped, and the file is tokenized again, so later errors are still found.
    pub fn new(code: &str) -> (Source, Vec<Diagnostic>) {
        let mut code = code.to_string();
        let line_starts: Vec<usize> = std::iter::once(0).chain(code.match_indices('\n').map(|(i, _)| i + 1)).collect();
        let (mut errors, mut bad_lines) = (Vec::new(), Vec::new());
        let tokens = loop {
            match tokenize_source(&code) {
                Ok(tokens) => break tokens,
                Err(e) => {
                    let from = line_starts[e.line - 1] + e.column - 1;
                    let to = code[from..].find('\n').map_or(code.len(), |n| from + n);
                    code.replace_range(from..to, &" ".repeat(to - from));
                    if !bad_lines.contains(&e.line) {
                        bad_lines.push(e.line);
                        errors.push(e);
                    }
                }
            }
        };
        let tokens = join_block_ends(tokens);
        (Source { code, tokens, line_starts, bad_lines }, errors)
    }

    /// Byte offset of token `i` (the end of the file past the last token).
    fn offset(&self, i: usize) -> usize {
        self.tokens.get(i).map_or(self.code.len(), |t| self.line_starts[t.line - 1] + t.column - 1)
    }

    /// Source text of the tokens `from..to`, trimmed.
    pub fn text(&self, from: usize, to: usize) -> &str {
        if from >= to {
            return "";
        }
        self.code[self.offset(from)..self.offset(to)].trim()
    }

    /// The token at `i`, if any.
    pub fn token(&self, i: usize) -> Option<&Token> {
        self.tokens.get(i).map(|t| &t.token)
    }

    /// Whether token `i` is the symbol or word `s`.
    pub fn is(&self, i: usize, s: &str) -> bool {
        match self.token(i) {
            Some(Token::Sym(sym)) => *sym == s,
            Some(Token::Ident(word)) => word == s,
            _ => false,
        }
    }

    /// Line of token `i` (past the end: the last line with a token).
    pub fn line(&self, i: usize) -> usize {
        self.tokens.get(i).or(self.tokens.last()).map_or(1, |t| t.line)
    }

    /// An error at token `i` (past the end: at the last token).
    pub fn error(&self, i: usize, message: impl Into<String>) -> Diagnostic {
        let (line, column) = self.tokens.get(i).or(self.tokens.last()).map_or((1, 1), |t| (t.line, t.column));
        Diagnostic { line, column, message: message.into(), severity: Severity::Error }
    }

    /// An error just after token `i` (where a missing `;` belongs).
    pub fn error_after(&self, i: usize, message: impl Into<String>) -> Diagnostic {
        let mut d = self.error(i, message);
        d.column += self.text(i, i + 1).len();
        d
    }

    /// Whether a statement touches a line with a lexing error.
    pub fn is_broken(&self, stmt: Statement) -> bool {
        self.tokens[stmt.start..stmt.end].iter().any(|t| self.bad_lines.contains(&t.line))
    }
}

/// `<` `-` at the start of a line is a block end `<-` (e.g. after a missing `;`).
fn join_block_ends(tokens: Vec<Spanned>) -> Vec<Spanned> {
    let mut out: Vec<Spanned> = Vec::with_capacity(tokens.len());
    let mut iter = tokens.into_iter().peekable();
    while let Some(t) = iter.next() {
        let line_start = out.last().is_none_or(|prev| prev.line < t.line);
        let minus_next = iter
            .peek()
            .is_some_and(|n| n.token == Token::Sym("-") && n.line == t.line && n.column == t.column + 1);
        if line_start && t.token == Token::Sym("<") && minus_next {
            iter.next();
            out.push(Spanned { token: Token::Sym("<-"), ..t });
        } else {
            out.push(t);
        }
    }
    out
}

/// One statement or block marker: the tokens `start..end`. A complete statement
/// ends with `;` or with a block-opening `->` (`if a ->`, `<- else ->`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Statement {
    pub start: usize,
    pub end: usize,
}

impl Source {
    /// Split the tokens into statements. A statement ends after a `;` outside brackets,
    /// or after a `->`; it may span lines. When a line ends with a value and the next
    /// line starts a new one (`show(1)` / `show(2);`), the `;` is missing: the
    /// statement ends at the line break (and is reported as incomplete). A statement
    /// also ends with a line that has a lexing error (the rest of that line is gone).
    pub fn statements(&self) -> Vec<Statement> {
        let mut statements = Vec::new();
        let mut start = 0;
        while start < self.tokens.len() {
            let end = self.statement_end(start);
            statements.push(Statement { start, end });
            start = end;
        }
        statements
    }

    fn statement_end(&self, start: usize) -> usize {
        let tokens = &self.tokens;
        let mut depth = 0usize;
        for i in start..tokens.len() {
            match tokens[i].token {
                Token::Sym("(" | "[") => depth += 1,
                Token::Sym(")" | "]") => depth = depth.saturating_sub(1),
                Token::Sym(";") if depth == 0 => return i + 1,
                Token::Sym("->") => return i + 1,
                _ => {}
            }
            if let Some(next) = tokens.get(i + 1) {
                let (line, token) = (tokens[i].line, &tokens[i].token);
                if next.line > line
                    && (self.bad_lines.contains(&line) || (ends_value(token) && starts_value(&next.token)))
                {
                    return i + 1;
                }
            }
        }
        tokens.len()
    }
}

/// A token that can end a statement or a value: `)`, `]`, `;`, a literal, or a name.
fn ends_value(token: &Token) -> bool {
    match token {
        Token::Sym(s) => matches!(*s, ")" | "]" | ";"),
        Token::Ident(w) => !KEYWORDS.contains(&w.as_str()) || w == "true" || w == "false",
        Token::Str(_) | Token::Int(_) | Token::Float(_) => true,
    }
}

/// A token that cannot continue a value: a name or keyword (other than the
/// operators `and`, `or`, `mod`), a literal, or a block end.
fn starts_value(token: &Token) -> bool {
    match token {
        Token::Sym(s) => *s == "<-",
        Token::Ident(w) => !matches!(w.as_str(), "and" | "or" | "mod"),
        Token::Str(_) | Token::Int(_) | Token::Float(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(code: &str) -> Vec<(usize, usize, String)> {
        let (src, errors) = Source::new(code);
        assert!(errors.is_empty(), "{errors:?}");
        src.statements()
            .into_iter()
            .map(|s| (src.line(s.start), src.tokens[s.start].column, src.text(s.start, s.end).to_string()))
            .collect()
    }

    fn t(line: usize, column: usize, text: &str) -> (usize, usize, String) {
        (line, column, text.to_string())
    }

    #[test]
    fn splits_statements_and_blocks() {
        assert_eq!(
            texts("set a: int = 1; show(a);\nif a > 0 -> show(\"x;->\"); <-;"),
            vec![
                t(1, 1, "set a: int = 1;"),
                t(1, 17, "show(a);"),
                t(2, 1, "if a > 0 ->"),
                t(2, 13, "show(\"x;->\");"),
                t(2, 27, "<-;"),
            ]
        );
        assert_eq!(texts("  <- else ->\n"), vec![t(1, 3, "<- else ->")]);
    }

    #[test]
    fn statements_span_lines() {
        assert_eq!(
            texts("set a: int = 1\n  + 2;\nF(\n  \"a;b\",\n  3\n);"),
            vec![t(1, 1, "set a: int = 1\n  + 2;"), t(3, 1, "F(\n  \"a;b\",\n  3\n);")]
        );
    }

    #[test]
    fn a_missing_semicolon_ends_the_statement_at_the_line_break() {
        assert_eq!(texts("show(1)\nshow(2);"), vec![t(1, 1, "show(1)"), t(2, 1, "show(2);")]);
        assert_eq!(texts("if a ->\n  show(1)\n<-;"), vec![t(1, 1, "if a ->"), t(2, 3, "show(1)"), t(3, 1, "<-;")]);
        assert_eq!(texts("show((1);\nshow(2);"), vec![t(1, 1, "show((1);"), t(2, 1, "show(2);")]);
    }

    #[test]
    fn lexing_errors_drop_the_rest_of_the_line() {
        let (src, errors) = Source::new("show(\"a\\qb\");\nshow(1 @ 2);\nshow(3);");
        let found: Vec<(usize, usize)> = errors.iter().map(|e| (e.line, e.column)).collect();
        assert_eq!(found, vec![(1, 8), (2, 8)]);
        let statements = src.statements();
        let last = statements.last().unwrap();
        assert_eq!(src.text(last.start, last.end), "show(3);");
        assert!(src.is_broken(statements[0]) && !src.is_broken(*last));
    }

    #[test]
    fn errors_point_at_tokens() {
        let (src, _) = Source::new("show(1)\n");
        assert_eq!((src.error(2, "x").line, src.error(2, "x").column), (1, 6));
        assert_eq!(src.error_after(3, "x").column, 8);
    }
}

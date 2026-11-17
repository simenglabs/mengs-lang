//! Errors with a line and column, and splitting source lines into statements.

use std::fmt;

/// A parse error. `Display` gives `Line N: message` (the column is shown separately).
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub line: usize,
    /// 1-based column.
    pub column: usize,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Line {}: {}", self.line, self.message)
    }
}

/// One statement or block marker of the source: `show(1);`, `if a ->`, `<-;`.
/// A line can hold several (`set a: int = 1; show(a);`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece<'a> {
    pub line: usize,
    /// 1-based column where `text` starts.
    pub column: usize,
    /// Trimmed text.
    pub text: &'a str,
}

/// Split the (comment-free) source into pieces: after each `;` and after each
/// block opener `->`, outside string literals.
pub fn split_pieces(code: &str) -> Vec<Piece<'_>> {
    let mut pieces = Vec::new();
    for (i, line) in code.lines().enumerate() {
        let bytes = line.as_bytes();
        let (mut start, mut in_str, mut escaped) = (0, false, false);
        let mut push = |from: usize, to: usize| {
            let raw = &line[from..to];
            let text = raw.trim();
            if !text.is_empty() {
                let column = from + (raw.len() - raw.trim_start().len()) + 1;
                pieces.push(Piece { line: i + 1, column, text });
            }
        };
        let mut j = 0;
        while j < bytes.len() {
            let c = bytes[j];
            if in_str {
                match c {
                    _ if escaped => escaped = false,
                    b'\\' => escaped = true,
                    b'"' => in_str = false,
                    _ => {}
                }
            } else if c == b'"' {
                in_str = true;
            } else if c == b';' || (c == b'>' && j > 0 && bytes[j - 1] == b'-') {
                push(start, j + 1);
                start = j + 1;
            }
            j += 1;
        }
        push(start, line.len());
    }
    pieces
}

/// Build a diagnostic for `piece`, pointing at the first `name` quoted in the
/// message when it appears in the statement (e.g. the variable in
/// "variable `a` was moved"), otherwise at the start of the statement.
pub fn diagnostic(piece: Piece, message: String) -> Diagnostic {
    let offset = quoted_fragments(&message).find_map(|frag| find_word(piece.text, frag)).unwrap_or(0);
    Diagnostic { line: piece.line, column: piece.column + offset, message }
}

fn quoted_fragments(message: &str) -> impl Iterator<Item = &str> {
    message.split('`').skip(1).step_by(2).filter(|f| !f.is_empty())
}

/// Byte offset of `word` in `text`; names must match as whole words.
fn find_word(text: &str, word: &str) -> Option<usize> {
    let is_name = word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let ident = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    text.match_indices(word).map(|(i, _)| i).find(|&i| {
        !is_name || (!ident(text[..i].chars().next_back()) && !ident(text[i + word.len()..].chars().next()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(code: &str) -> Vec<(usize, usize, &str)> {
        split_pieces(code).into_iter().map(|p| (p.line, p.column, p.text)).collect()
    }

    #[test]
    fn splits_statements_and_blocks() {
        assert_eq!(
            texts("set a: int = 1; show(a);\nif a > 0 -> show(\"x;->\"); <-;"),
            vec![
                (1, 1, "set a: int = 1;"),
                (1, 17, "show(a);"),
                (2, 1, "if a > 0 ->"),
                (2, 13, "show(\"x;->\");"),
                (2, 27, "<-;"),
            ]
        );
        assert_eq!(texts("  <- else ->\n"), vec![(1, 3, "<- else ->")]);
    }

    #[test]
    fn points_at_the_quoted_name() {
        let piece = Piece { line: 3, column: 5, text: "show(a + ab);" };
        assert_eq!(diagnostic(piece, "variable `ab` is not declared".into()).column, 14);
        assert_eq!(diagnostic(piece, "variable `a` was moved".into()).column, 10);
        assert_eq!(diagnostic(piece, "cannot compare int with str".into()).column, 5);
    }
}

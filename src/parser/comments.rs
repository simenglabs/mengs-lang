use super::diagnostic::Diagnostic;

/// Remove `% ... %` comments. Comments may span multiple lines and may appear
/// after code on the same line. A `%` inside a string literal is not a comment.
///
/// Comments are replaced by spaces (newlines are kept), so line and column
/// numbers of the remaining code stay exactly the same.
pub fn strip_comments(code: &str) -> Result<String, Diagnostic> {
    let mut out = String::with_capacity(code.len());
    let (mut in_comment, mut in_str, mut escaped) = (false, false, false);
    let (mut line, mut column) = (1, 0);
    let mut comment_start = (0, 0);

    for c in code.chars() {
        column += 1;
        if c == '\n' {
            line += 1;
            column = 0;
            // Strings cannot span lines, so a stray quote never leaks past its line.
            in_str = false;
            escaped = false;
            out.push(c);
            continue;
        }

        if in_comment {
            if c == '%' {
                in_comment = false;
            }
            out.push(' ');
            continue;
        }

        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
        } else if c == '"' {
            in_str = true;
        } else if c == '%' {
            in_comment = true;
            comment_start = (line, column);
            out.push(' ');
            continue;
        }
        out.push(c);
    }

    if in_comment {
        let (line, column) = comment_start;
        return Err(Diagnostic { line, column, message: "comment is not closed with `%`".into() });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::strip_comments;

    #[test]
    fn removes_inline_and_full_line_comments() {
        let out = strip_comments("% header %\nshow(\"a\"); % note %\n").unwrap();
        assert_eq!(out, "          \nshow(\"a\");         \n");
    }

    #[test]
    fn multi_line_comment_keeps_line_numbers() {
        let out = strip_comments("% one\ntwo\nthree %show(x);").unwrap();
        assert_eq!(out, "     \n   \n       show(x);");
    }

    #[test]
    fn percent_inside_string_is_not_a_comment() {
        let src = "show(\"100% \\\"sure\\\" %\"); % real comment %";
        assert_eq!(strip_comments(src).unwrap(), "show(\"100% \\\"sure\\\" %\");                 ");
    }

    #[test]
    fn unclosed_comment_reports_start_line() {
        let err = strip_comments("show(\"a\");\n  % never closed\nshow(\"b\");").unwrap_err();
        assert_eq!((err.line, err.column), (2, 3));
        assert_eq!(err.to_string(), "Line 2: comment is not closed with `%`");
    }
}

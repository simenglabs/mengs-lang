/// Remove `% ... %` comments. Comments may span multiple lines and may appear
/// after code on the same line. A `%` inside a string literal is not a comment.
///
/// Newlines inside comments are kept so error line numbers stay correct, and
/// each comment is replaced by a single space so tokens around it never merge.
pub fn strip_comments(code: &str) -> Result<String, String> {
    let mut out = String::with_capacity(code.len());
    let (mut in_comment, mut in_str, mut escaped) = (false, false, false);
    let (mut line, mut comment_start) = (1, 0);

    for c in code.chars() {
        if c == '\n' {
            line += 1;
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
            comment_start = line;
            out.push(' ');
            continue;
        }
        out.push(c);
    }

    if in_comment {
        return Err(format!("Line {}: comment is not closed with `%`", comment_start));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::strip_comments;

    #[test]
    fn removes_inline_and_full_line_comments() {
        let out = strip_comments("% header %\nshow(\"a\"); % note %\n").unwrap();
        assert_eq!(out, " \nshow(\"a\");  \n");
    }

    #[test]
    fn multi_line_comment_keeps_line_numbers() {
        let out = strip_comments("% one\ntwo\nthree %show(x);").unwrap();
        assert_eq!(out, " \n\nshow(x);");
    }

    #[test]
    fn percent_inside_string_is_not_a_comment() {
        let src = "show(\"100% \\\"sure\\\" %\"); % real comment %";
        assert_eq!(strip_comments(src).unwrap(), "show(\"100% \\\"sure\\\" %\");  ");
    }

    #[test]
    fn unclosed_comment_reports_start_line() {
        let err = strip_comments("show(\"a\");\n% never closed\nshow(\"b\");").unwrap_err();
        assert_eq!(err, "Line 2: comment is not closed with `%`");
    }
}

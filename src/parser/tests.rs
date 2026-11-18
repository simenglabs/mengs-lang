//! Parser tests: ownership, types, calls, and syntax errors.
//! Each error test checks the line number and a fragment of the message.

use super::{parse, warnings};
use crate::ast::{Expr, Stmt};

/// The source must parse without errors.
fn ok(src: &str) {
    if let Err(errors) = parse(src) {
        panic!("expected success, got errors: {errors:?}\n--- source ---\n{src}");
    }
}

/// The source must report an error on `line` with a message containing `fragment`.
fn err(src: &str, line: usize, fragment: &str) {
    match parse(src) {
        Ok(_) => panic!("expected an error containing `{fragment}`, but it parsed\n--- source ---\n{src}"),
        Err(errors) => {
            let found = errors.iter().any(|e| e.line == line && e.message.contains(fragment));
            let all: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
            assert!(found, "expected `Line {line}: ...{fragment}...`, got {all:#?}\n--- source ---\n{src}");
        }
    }
}

// ------------------------------------------------------------- ownership: move

#[test]
fn assigning_a_variable_moves_it() {
    err("set a: str = \"x\";\nset b: str = a;\nshow(a);", 3, "variable `a` was moved on line 2");
}

#[test]
fn passing_a_variable_to_a_function_moves_it() {
    let src = "fnc F(s: str)->\n    show(s);\n<-;\nset a: str = \"x\";\nF(a);\nF(a);";
    err(src, 6, "variable `a` was moved on line 5");
}

#[test]
fn show_and_if_only_read() {
    ok("set a: int = 1;\nshow(a);\nshow(a);\nif a = 1 ->\n    show(a);\n<-;\nset b: int = a;");
}

#[test]
fn moved_variable_cannot_be_read_either() {
    err("set a: str = \"x\";\nset b: str = a;\nif a = \"x\" ->\n<-;", 3, "was moved");
}

#[test]
fn strings_move() {
    err("set a: str = \"x\";\nset b: str = a;\nset c: str = a;", 3, "was moved");
}

#[test]
fn literals_never_move_anything() {
    ok("fnc F(s: str)->\n<-;\nF(\"x\");\nF(\"x\");");
}

// ------------------------------------------------------------- keep / reuse

#[test]
fn keep_is_never_moved() {
    ok("set keep a: str = \"x\";\nset b: str = a;\nset c: str = a;\nshow(a);");
}

#[test]
fn reuse_is_never_moved() {
    let src = "fnc F(s: str)->\n<-;\nset reuse a: str = \"x\";\nF(a);\nF(a);\nset b: str = a;";
    ok(src);
}

#[test]
fn reuse_parameters_can_be_used_many_times() {
    ok("fnc F(reuse b: str)->\n    set z: str = b;\n    set w: str = b;\n<-;");
}

#[test]
fn keep_parameter_borrows_the_argument() {
    ok("fnc Len(keep s: str): int ->\n    return len(s);\n<-;\nset name: str = \"meng\";\nshow(Len(name));\nshow(Len(name));\nshow(name);");
}

#[test]
fn keep_parameter_cannot_be_moved() {
    let take = "fnc Take(s: str)->\n<-;\n";
    let msg = "is a keep parameter: it is only borrowed";
    err(&format!("{take}fnc F(keep s: str)->\n    Take(s);\n<-;"), 4, msg);
    err(&format!("{take}fnc F(keep s: str)->\n    set t: str = s;\n<-;"), 4, msg);
    err(&format!("{take}fnc F(keep s: str): str ->\n    return s;\n<-;"), 4, msg);
    err(&format!("{take}fnc F(keep xs: [int])->\n    set ys: [int] = xs;\n<-;"), 4, msg);
}

#[test]
fn borrowed_and_moved_in_the_same_call_is_an_error() {
    let msg = "is moved and borrowed by the same call";
    let f = "fnc F(keep a: str, b: str)->\n    show(a);\n<-;\nset name: str = \"x\";\n";
    err(&format!("{f}F(name, name);"), 5, msg);
    err(&format!("{f}set r: str <== name;\nF(r, name);"), 6, msg);
    err(&format!("{f}F(name + \"y\", name);"), 5, msg);
    let g = "fnc F(keep a: str, xs: [str])->\n    show(a);\n<-;\nset names: [str] = [\"x\"];\n";
    err(&format!("{g}F(names[0], names);"), 5, msg);
    let h = "fnc F(keep xs: [int], ys: [int])->\n    show(len(xs));\n<-;\nset a: [int] = [1];\n";
    err(&format!("{h}F(a, a);"), 5, msg);
    // Different variables, or a copy, are fine.
    ok(&format!("{f}set other: str = \"z\";\nF(name, other);"));
    ok(&format!("{f}name ()=> c;\nF(name, c);"));
}

#[test]
fn keep_parameter_can_be_copied_and_reborrowed() {
    ok("fnc G(keep s: str)->\n<-;\nfnc F(keep s: str): str ->\n    G(s);\n    s ()=> t;\n    return t;\n<-;");
}

#[test]
fn plain_parameter_is_moved() {
    err("fnc F(a: str)->\n    set x: str = a;\n    show(a);\n<-;", 3, "variable `a` was moved on line 2");
}

// ------------------------------------------------------------- borrow

#[test]
fn borrow_keeps_the_owner_usable() {
    ok("set a: str = \"x\";\nset b: str <== a;\nshow(a);\nshow(b);");
}

#[test]
fn borrow_can_be_used_many_times() {
    ok("fnc F(s: str)->\n<-;\nset a: str = \"x\";\nset b: str <== a;\nF(b);\nF(b);");
}

#[test]
fn borrow_is_invalid_after_owner_moves() {
    err("set a: str = \"x\";\nset b: str <== a;\nset c: str = a;\nshow(b);", 4, "`b` borrows `a`, but `a` was moved on line 3");
}

#[test]
fn borrow_of_a_borrow_points_to_the_root_owner() {
    let src = "set a: str = \"x\";\nset b: str <== a;\nset c: str <== b;\nset d: str = a;\nshow(c);";
    err(src, 5, "`c` borrows `a`");
}

#[test]
fn assigning_from_a_borrow_is_also_a_borrow() {
    let src = "set a: str = \"x\";\nset b: str <== a;\nset c: str = b;\nset d: str = a;\nshow(c);";
    err(src, 5, "`c` borrows `a`");
}

#[test]
fn borrow_requires_the_same_type() {
    err("set a: int = 1;\nset b: float <== a;", 2, "`b` is declared as float but borrows `a` of type int");
}

#[test]
fn borrow_requires_a_variable() {
    err("set b: str <== \"x\";", 1, "`<==` can only borrow from a variable");
}

#[test]
fn borrow_cannot_be_keep_or_reuse() {
    err("set a: str = \"x\";\nset keep b: str <== a;", 2, "cannot be declared with `keep` or `reuse`");
}

#[test]
fn borrow_of_a_moved_variable_fails() {
    err("set a: str = \"x\";\nset b: str = a;\nset c: str <== a;", 3, "was moved");
}

// ------------------------------------------------------------- copy

#[test]
fn copy_is_independent_of_the_source() {
    let src = "fnc F(s: str)->\n<-;\nset a: str = \"x\";\na ()=> b;\nF(a);\nF(b);";
    ok(src);
}

#[test]
fn copy_has_the_source_type() {
    err("set a: int = 1;\na ()=> b;\nset c: str = b;", 3, "variable `c` has type str, but got an int value");
}

#[test]
fn copy_of_a_moved_variable_fails() {
    err("set a: str = \"x\";\nset b: str = a;\na ()=> c;", 3, "was moved");
}

#[test]
fn copy_requires_a_variable() {
    err("\"x\" ()=> b;", 1, "`()=>` can only copy a variable");
}

#[test]
fn copy_target_must_be_new() {
    err("set a: str = \"x\";\nset b: str = \"y\";\na ()=> b;", 3, "variable `b` is already declared");
}

// ------------------------------------------------------------- ownership in if / else

#[test]
fn move_inside_if_counts_after_the_block() {
    err("set a: str = \"x\";\nif true ->\n    set b: str = a;\n<-;\nshow(a);", 5, "variable `a` was moved on line 3");
}

#[test]
fn move_in_else_counts_after_the_block() {
    let src = "set a: str = \"x\";\nif true ->\n<- else ->\n    set b: str = a;\n<-;\nshow(a);";
    err(src, 6, "variable `a` was moved on line 4");
}

#[test]
fn each_branch_can_move_the_same_variable() {
    ok("set a: str = \"x\";\nif true ->\n    set b: str = a;\n<- else if false ->\n    set c: str = a;\n<- else ->\n    set d: str = a;\n<-;");
}

#[test]
fn variables_declared_in_a_branch_are_not_visible_outside() {
    err("if true ->\n    set a: int = 1;\n<-;\nshow(a);", 4, "variable `a` is not declared");
}

// ------------------------------------------------------------- scope

#[test]
fn undeclared_variable() {
    err("show(nope);", 1, "variable `nope` is not declared");
}

#[test]
fn variable_declared_twice() {
    err("set a: int = 1;\nset a: int = 2;", 2, "variable `a` is already declared");
}

#[test]
fn keyword_cannot_be_a_name() {
    err("set show: int = 1;", 1, "`show` is a keyword");
}

#[test]
fn parameters_are_not_visible_at_top_level() {
    err("fnc F(a: int)->\n<-;\nshow(a);", 3, "variable `a` is not declared");
}

// ------------------------------------------------------------- types

#[test]
fn every_type_accepts_its_literal() {
    ok("set a: str = \"x\";\nset b: int = -3;\nset c: float = 1.5;\nset d: bool = true;");
}

#[test]
fn int_is_accepted_as_float() {
    ok("set a: float = 2;\nset b: int = 3;\nset c: float = b;");
}

#[test]
fn float_is_not_accepted_as_int() {
    err("set a: int = 1.5;", 1, "has type int, but got a float value");
}

#[test]
fn string_is_not_accepted_as_int() {
    err("set a: int = \"1\";", 1, "has type int, but got a str value");
}

#[test]
fn type_is_required() {
    err("set a = 1;", 1, "a type is required for `a`");
}

#[test]
fn unknown_type() {
    err("set a: number = 1;", 1, "unknown type `number`");
}

// ------------------------------------------------------------- conditions

#[test]
fn comparisons_between_compatible_types() {
    ok("set a: int = 1;\nset b: float = 2.5;\nif a < b ->\n<-;\nif \"x\" = \"y\" ->\n<-;\nif true != false ->\n<-;");
}

#[test]
fn cannot_compare_str_with_int() {
    err("if \"x\" = 1 ->\n<-;", 1, "cannot compare str with int");
}

#[test]
fn bool_only_supports_equality() {
    err("if true < false ->\n<-;", 1, "bool values can only be compared with `=` or `!=`");
}

#[test]
fn condition_without_operator_must_be_bool() {
    err("set a: int = 1;\nif a ->\n<-;", 2, "a condition must be bool, got int");
}

// ------------------------------------------------------------- functions and calls

#[test]
fn call_before_definition_is_allowed() {
    ok("F(1);\nfnc F(a: int)->\n<-;");
}

#[test]
fn undefined_function() {
    err("G(1);", 1, "function `G` is not defined");
}

#[test]
fn too_many_arguments() {
    err("fnc F(a: int)->\n<-;\nF(1, 2);", 3, "takes 1 argument(s), but 2 were given");
}

#[test]
fn too_few_arguments_with_defaults() {
    err("fnc F(a: int, b: int = 2, c: int = 3)->\n<-;\nF();", 3, "takes 1-3 argument(s), but 0 were given");
}

#[test]
fn defaults_can_be_omitted() {
    ok("fnc F(a: int, b: int = 2, c: int = 3)->\n<-;\nF(1);\nF(1, 5);\nF(1, 5, 6);");
}

#[test]
fn wrong_argument_type() {
    err("fnc F(a: int)->\n<-;\nF(\"x\");", 3, "argument 1 (`a`) of function `F` must be int, got str");
}

#[test]
fn default_must_match_the_type() {
    err("fnc F(a: int = \"x\")->\n<-;", 1, "default for parameter `a` has type int, but got a str value");
}

#[test]
fn default_must_be_a_literal() {
    err("set b: int = 1;\nfnc F(a: int = b)->\n<-;", 2, "must be a literal value");
}

#[test]
fn function_defined_twice() {
    err("fnc F()->\n<-;\nfnc F()->\n<-;", 3, "function `F` is already defined");
}

#[test]
fn parameter_declared_twice() {
    err("fnc F(a: int, a: int)->\n<-;", 1, "parameter `a` is declared twice");
}

#[test]
fn nested_functions_are_not_allowed() {
    err("fnc F()->\n    fnc G()->\n    <-;\n<-;", 2, "functions can only be defined at the top level");
}

// ------------------------------------------------------------- blocks and syntax

#[test]
fn unclosed_function() {
    err("fnc F()->\n    show(1);", 1, "is not closed with `<-;`");
}

#[test]
fn unclosed_if() {
    err("if true ->\n    show(1);", 1, "is not closed with `<-;`");
}

#[test]
fn else_without_if() {
    err("<- else ->\n<-;", 1, "block terminator without a matching `fnc`/`if`");
}

#[test]
fn stray_block_terminator() {
    err("show(1);\n<-;", 2, "block terminator without a matching `fnc`/`if`");
}

#[test]
fn else_after_else() {
    err("if true ->\n<- else ->\n<- else ->\n<-;", 3, "`else` was already used");
}

#[test]
fn missing_semicolon() {
    err("show(1)", 1, "unrecognized statement");
}

#[test]
fn unrecognized_statement() {
    err("hello world;", 1, "unrecognized statement");
}

#[test]
fn unclosed_string() {
    err("show(\"abc);", 1, "string is not closed");
}

#[test]
fn unknown_escape() {
    err("show(\"a\\qb\");", 1, "unknown escape sequence");
}

#[test]
fn unclosed_comment() {
    err("show(1);\n% never closed", 2, "comment is not closed");
}

#[test]
fn comments_are_ignored_and_keep_line_numbers() {
    err("% one\ntwo\nthree %\nshow(nope);", 4, "variable `nope` is not declared");
}

// ------------------------------------------------------------- AST shape

#[test]
fn else_if_is_nested_in_else_body() {
    let program = parse("if 1 < 2 ->\n<- else if 2 < 3 ->\n<- else ->\n    show(1);\n<-;").unwrap();
    let Stmt::If { else_body, .. } = &program.main[0] else { panic!("expected if") };
    assert_eq!(else_body.len(), 1);
    let Stmt::If { else_body: inner_else, .. } = &else_body[0] else { panic!("expected else-if") };
    assert!(matches!(inner_else[..], [Stmt::Print(_)]));
}

#[test]
fn functions_are_separated_from_main() {
    let program = parse("fnc F()->\n    show(1);\n<-;\nshow(2);\nF();").unwrap();
    assert_eq!(program.functions.len(), 1);
    assert_eq!(program.functions[0].body.len(), 1);
    assert_eq!(program.main.len(), 2);
}

// ------------------------------------------------------------- expressions

/// Parse `show(<expr>);` and return the expression.
fn expr(src: &str) -> crate::ast::Expr {
    let program = parse(&format!("show({src});")).unwrap_or_else(|e| panic!("`{src}`: {e:?}"));
    let Some(Stmt::Print(e)) = program.main.into_iter().next() else { panic!("expected show") };
    e
}

/// Fully parenthesized form, to check precedence.
fn shape(e: &crate::ast::Expr) -> String {
    use crate::ast::{Expr, UnaryOp};
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Float(x) => format!("{x:?}"),
        Expr::Bool(b) => b.to_string(),
        Expr::Str(s) => format!("{s:?}"),
        Expr::Var { name, .. } => name.clone(),
        Expr::Unary { op: UnaryOp::Neg, expr } => format!("(-{})", shape(expr)),
        Expr::Unary { op: UnaryOp::Not, expr } => format!("(not {})", shape(expr)),
        Expr::Binary { op, lhs, rhs } => format!("({} {} {})", shape(lhs), op.symbol(), shape(rhs)),
        Expr::Call { name, args, .. } => {
            format!("{}({})", name, args.iter().map(shape).collect::<Vec<_>>().join(", "))
        }
        Expr::Convert { to, expr } => format!("{}({})", to.name(), shape(expr)),
        Expr::Read { .. } => "read()".into(),
        Expr::Array { items, .. } => format!("[{}]", items.iter().map(shape).collect::<Vec<_>>().join(", ")),
        Expr::Index { array, index, .. } => format!("{}[{}]", shape(array), shape(index)),
        Expr::Len(e) => format!("len({})", shape(e)),
    }
}

#[test]
fn operator_precedence() {
    let cases = [
        ("1 + 2 * 3", "(1 + (2 * 3))"),
        ("(1 + 2) * 3", "((1 + 2) * 3)"),
        ("10 - 3 - 2", "((10 - 3) - 2)"),
        ("8 / 4 / 2", "((8 / 4) / 2)"),
        ("7 mod 3 + 1", "((7 mod 3) + 1)"),
        ("-2 * 3", "(-2 * 3)"),
        ("- (2 + 3)", "(-(2 + 3))"),
        ("2 - -3", "(2 - -3)"),
        ("1 + 2 < 4", "((1 + 2) < 4)"),
        ("not 1 < 2", "(not (1 < 2))"),
        ("true or false and false", "(true or (false and false))"),
        ("not true and false", "((not true) and false)"),
        ("1 < 2 and 3 >= 4 or 5 != 6", "(((1 < 2) and (3 >= 4)) or (5 != 6))"),
    ];
    for (src, expected) in cases {
        assert_eq!(shape(&expr(src)), expected, "`{src}`");
    }
}

#[test]
fn arithmetic_result_types() {
    use crate::ast::Type;
    assert_eq!(expr("7 / 2").ty(), Type::Int);
    assert_eq!(expr("7 / 2.0").ty(), Type::Float);
    assert_eq!(expr("1 + 2 * 1.5").ty(), Type::Float);
    assert_eq!(expr("7 mod 2").ty(), Type::Int);
    assert_eq!(expr("-(1.5)").ty(), Type::Float);
    assert_eq!(expr("1 < 2").ty(), Type::Bool);
    assert_eq!(expr("not true").ty(), Type::Bool);
}

#[test]
fn expressions_can_be_stored_and_converted() {
    ok("set a: int = 2 * 3;\nset b: float = a / 4;\nset c: bool = a > 1 and b < 2;");
    err("set a: int = 1 + 0.5;", 1, "variable `a` has type int, but got a float value");
}

#[test]
fn expression_type_errors() {
    err("show(1 + \"b\");", 1, "`+` joins two strings or adds two numbers, got int and str");
    err("show(5 mod 2.0);", 1, "`mod` needs int values, got int and float");
    err("show(not 1);", 1, "`not` needs a bool value, got int");
    err("show(-\"x\");", 1, "`-` needs a number, got str");
    err("show(1 and true);", 1, "`and` needs bool values, got int and bool");
    err("show(1 < \"a\");", 1, "cannot compare int with str");
}

#[test]
fn expression_syntax_errors() {
    err("show(1 < 2 < 3);", 1, "comparisons cannot be chained");
    err("show((1 + 2);", 1, "missing `)`");
    err("show(1 +);", 1, "expected a value");
    err("show(1 2);", 1, "unexpected `2`");
    err("show(1 @ 2);", 1, "unexpected character `@`");
    err("show(1.2.3);", 1, "unexpected character `.`");
    err("set mod: int = 1;", 1, "`mod` is a keyword");
}

#[test]
fn condition_can_be_any_bool_expression() {
    ok("set a: int = 1;\nif (a + 1) * 2 > 3 and not (a = 0) ->\n<-;");
    err("if 1 + 1 ->\n<-;", 1, "a condition must be bool, got int");
}

#[test]
fn variables_inside_expressions_are_only_read() {
    ok("set a: int = 1;\nset b: int = a + a;\nset c: int = a * 2;\nset d: bool = a = 1;\nshow(a);");
    ok("fnc F(n: int)->\n<-;\nset a: int = 1;\nF(a + 1);\nF(a * 2);\nshow(a);");
}

#[test]
fn a_bare_variable_still_moves() {
    err("set a: str = \"x\";\nset b: str = a + \"y\";\nset c: str = a;\nshow(a);", 4, "variable `a` was moved on line 3");
    err("set a: str = \"x\";\nset b: str = (a);\nshow(a);", 3, "variable `a` was moved on line 2");
}

#[test]
fn moved_variable_cannot_be_used_in_an_expression() {
    err("set a: str = \"x\";\nset b: str = a;\nshow(a + \"y\");", 3, "variable `a` was moved on line 2");
}

#[test]
fn negative_literals_are_valid_defaults() {
    ok("fnc F(n: int = -5, f: float = -1.5)->\n<-;");
    err("fnc F(n: int = 1 + 1)->\n<-;", 1, "must be a literal value");
}

// ------------------------------------------------------------- reassign

#[test]
fn reassign_changes_the_value() {
    ok("set a: int = 1;\na = a + 1;\nset f: float = 1;\nf = 2;");
    err("set a: int = 1;\na = \"x\";", 2, "variable `a` has type int, but got a str value");
    err("nope = 1;", 1, "variable `nope` is not declared");
}

#[test]
fn keep_cannot_be_reassigned() {
    err("set keep a: int = 1;\na = 2;", 2, "`a` is a constant (keep) and cannot be reassigned");
    err("fnc F(keep a: int)->\n    a = 2;\n<-;", 2, "`a` is a constant (keep)");
}

#[test]
fn borrow_cannot_be_reassigned() {
    err("set a: str = \"x\";\nset b: str <== a;\nb = \"y\";", 3, "`b` borrows `a` and cannot be reassigned");
}

#[test]
fn reassign_makes_a_moved_variable_usable() {
    ok("set a: str = \"x\";\nset b: str = a;\na = \"y\";\nshow(a);");
}

#[test]
fn reassign_makes_borrows_stale() {
    err("set a: str = \"x\";\nset b: str <== a;\na = \"y\";\nshow(b);", 4, "`b` borrows `a`, but `a` got a new value on line 3");
    ok("set a: str = \"x\";\nset b: str <== a;\na = \"y\";\nset c: str <== a;\nshow(c);");
}

#[test]
fn reassign_in_one_branch_does_not_revive() {
    let src = "set a: str = \"x\";\nset b: str = a;\nif true ->\n    a = \"y\";\n<-;\nshow(a);";
    err(src, 6, "variable `a` was moved on line 2");
    ok("set a: str = \"x\";\nset b: str = a;\nif true ->\n    a = \"y\";\n<- else ->\n    a = \"z\";\n<-;\nshow(a);");
}

// ------------------------------------------------------------- while

#[test]
fn while_loop() {
    ok("set i: int = 0;\nwhile i < 3 ->\n    show(i);\n    i = i + 1;\n<-;");
    err("while 1 ->\n<-;", 1, "a condition must be bool, got int");
    err("while true ->\n<- else ->\n<-;", 2, "`else` without `if`");
    err("while true ->\n    show(1);", 1, "`while` loop is not closed");
}

#[test]
fn moving_an_outer_variable_in_a_loop_is_an_error() {
    let src = "fnc G(s: str)->\n<-;\nset a: str = \"x\";\nwhile true ->\n    G(a);\n<-;";
    err(src, 5, "variable `a` is moved inside a loop");
}

#[test]
fn moving_in_a_loop_is_fine_with_a_new_value_or_a_local() {
    ok("fnc G(s: str)->\n<-;\nset a: str = \"x\";\nwhile true ->\n    G(a);\n    a = \"again\";\n<-;");
    ok("fnc G(s: str)->\n<-;\nwhile true ->\n    set t: str = \"local\";\n    G(t);\n<-;");
    ok("fnc G(s: str)->\n<-;\nset reuse a: str = \"x\";\nwhile true ->\n    G(a);\n<-;");
}

#[test]
fn borrow_made_stale_in_a_loop_is_an_error() {
    let src = "set a: str = \"x\";\nset r: str <== a;\nwhile true ->\n    show(r);\n    a = \"y\";\n<-;";
    err(src, 5, "`r` borrows a variable that gets a new value inside a loop");
}

#[test]
fn variables_declared_in_a_loop_are_not_visible_outside() {
    err("while false ->\n    set a: int = 1;\n<-;\nshow(a);", 4, "variable `a` is not declared");
}

// ------------------------------------------------------------- return values

#[test]
fn function_with_return_value() {
    ok("fnc Add(a: int, b: int): int ->\n    return a + b;\n<-;\nset s: int = Add(1, 2) * Add(3, 4);");
    ok("fnc Half(n: int): float ->\n    return n / 2;\n<-;");
}

#[test]
fn return_type_must_match() {
    err("fnc F(): int ->\n    return \"x\";\n<-;", 2, "return value of `F` has type int, but got a str value");
    err("fnc F(): int ->\n    return;\n<-;", 2, "function `F` must return an int value");
    err("fnc F()->\n    return 1;\n<-;", 2, "has no return type, so use `return;`");
    err("fnc F(): number ->\n<-;", 1, "unknown type `number`");
}

#[test]
fn every_path_must_return() {
    err("fnc F(): int ->\n    show(1);\n<-;", 1, "must return an int value on every path");
    err("fnc F(n: int): int ->\n    if n > 0 ->\n        return 1;\n    <-;\n<-;", 1, "on every path");
    err("fnc F(n: int): int ->\n    while n > 0 ->\n        return 1;\n    <-;\n<-;", 1, "on every path");
    ok("fnc F(n: int): int ->\n    if n > 0 ->\n        return 1;\n    <- else ->\n        return 2;\n    <-;\n<-;");
}

#[test]
fn return_only_inside_functions() {
    err("return 1;", 1, "`return` can only be used inside a function");
    err("if true ->\n    return;\n<-;", 2, "`return` can only be used inside a function");
    err("set return: int = 1;", 1, "`return` is a keyword");
}

#[test]
fn void_function_can_return_early() {
    ok("fnc F(n: int)->\n    if n > 0 ->\n        return;\n    <-;\n    show(n);\n<-;");
}

#[test]
fn moves_in_a_returning_branch_do_not_count_after_the_if() {
    let src = "fnc G(s: str)->\n<-;\nfnc H(c: bool)->\n    set a: str = \"x\";\n    if c ->\n        G(a);\n        return;\n    <-;\n    show(a);\n<-;";
    ok(src);
}

// ------------------------------------------------------------- calls in expressions

#[test]
fn calls_can_be_used_before_the_definition() {
    ok("show(Twice(2));\nfnc Twice(n: int): int ->\n    return n * 2;\n<-;");
}

#[test]
fn void_function_cannot_be_used_as_a_value() {
    err("fnc F()->\n<-;\nshow(F() + 1);", 3, "function `F` does not return a value");
}

#[test]
fn call_checks_in_expressions() {
    let f = "fnc F(a: int, b: int = 2): int ->\n    return a + b;\n<-;\n";
    ok(&format!("{f}show(F(1));\nshow(F(1, 5));"));
    err(&format!("{f}show(F(1, 2, 3));"), 4, "takes 1-2 argument(s), but 3 were given");
    err(&format!("{f}show(F(\"x\"));"), 4, "argument 1 (`a`) of function `F` must be int, got str");
    err(&format!("{f}show(F(1,));"), 4, "expected a value");
    err("show(Nope(1));", 1, "function `Nope` is not defined");
}

#[test]
fn defaults_are_filled_in_by_the_parser() {
    let program = parse("fnc F(a: int, b: int = 7)->\n<-;\nF(1);").unwrap();
    let Stmt::Call { args, .. } = &program.main[0] else { panic!("expected call") };
    assert_eq!(args, &vec![crate::ast::Expr::Int(1), crate::ast::Expr::Int(7)]);
}

#[test]
fn bare_variable_argument_moves_inside_expressions_too() {
    let f = "fnc F(s: str): int ->\n    return 1;\n<-;\nset s: str = \"x\";\n";
    err(&format!("{f}set n: int = F(s) + F(s);"), 5, "variable `s` was moved on line 5");
}

#[test]
fn function_header_syntax() {
    err("fnc F(a: int)-> int ->\n<-;", 1, "unrecognized statement: `int ->`");
    err("fnc F(a: int) int ->\n<-;", 1, "invalid function syntax");
    err("show(1);\nfnc F()->\n<-;\nfnc F()->\n<-;", 4, "function `F` is already defined");
}

// ------------------------------------------------------------- strings

#[test]
fn strings_can_be_joined_and_converted() {
    use crate::ast::Type;
    assert_eq!(expr("\"a\" + \"b\"").ty(), Type::Str);
    assert_eq!(expr("str(1) + str(2.5) + str(true)").ty(), Type::Str);
    assert_eq!(expr("int(\"4\") + int(2.9)").ty(), Type::Int);
    assert_eq!(expr("float(\"2.5\") * float(2)").ty(), Type::Float);
    assert_eq!(expr("read()").ty(), Type::Str);
    assert_eq!(expr("read(\"name: \")").ty(), Type::Str);
}

#[test]
fn string_operator_errors() {
    err("show(\"a\" + 1);", 1, "`+` joins two strings or adds two numbers, got str and int; convert with str(...)");
    err("show(\"a\" - \"b\");", 1, "`-` needs numbers, got str and str");
}

#[test]
fn builtin_errors() {
    err("show(int(true));", 1, "cannot convert bool to int");
    err("show(str(1, 2));", 1, "`str` takes one value");
    err("show(read(1));", 1, "the prompt of `read` must be a str, got int");
    err("fnc str(x: int): str ->\n    return \"x\";\n<-;", 1, "`str` is a built-in function and cannot be redefined");
}

#[test]
fn type_names_can_still_be_variables() {
    ok("set str: int = 1;\nshow(str + 1);\nshow(str(str));");
}

#[test]
fn bare_variable_moves_are_marked_for_codegen() {
    use crate::ast::Expr;
    let program = parse("set a: str = \"x\";\nset reuse r: str = \"y\";\nset b: str = a;\nset c: str = r;").unwrap();
    let moved = |i: usize| match &program.main[i] {
        Stmt::Set { value: Expr::Var { moved, .. }, .. } => *moved,
        _ => panic!("expected set from a variable"),
    };
    assert!(moved(2), "an owned variable is moved");
    assert!(!moved(3), "a reuse variable is copied");
}

#[test]
fn borrows_are_marked_for_codegen() {
    let program = parse("set a: str = \"x\";\nset b: str <== a;\nset c: str = b;\na ()=> d;").unwrap();
    let borrow = |i: usize| match &program.main[i] {
        Stmt::Set { borrow, .. } => *borrow,
        _ => panic!("expected set"),
    };
    assert!(!borrow(0));
    assert!(borrow(1), "<== is a borrow");
    assert!(borrow(2), "assigning from a borrow is a borrow");
    assert!(!borrow(3), "a copy owns its value");
}

// ------------------------------------------------------------- arrays

#[test]
fn array_types_and_literals() {
    use crate::ast::Type;
    assert_eq!(Type::parse("[int]"), Some(Type::Array(&Type::Int)));
    assert_eq!(Type::parse("[[int]]"), None);
    assert_eq!(expr("[1, 2]").ty(), Type::Array(&Type::Int));
    assert_eq!(expr("[1, 2.5]").ty(), Type::Array(&Type::Float));
    assert_eq!(expr("[\"a\"][0]").ty(), Type::Str);
    assert_eq!(expr("len([true])").ty(), Type::Int);
    assert_eq!(shape(&expr("[1, 2][0] + 1")), "([1, 2][0] + 1)");
    err("set xs: [int] = [1, \"a\"];", 1, "all items of an array must have the same type, got int and str");
    err("set xs: [int] = [1.5];", 1, "variable `xs` has type [int], but got a [float] value");
    err("set xs: [[int]] = [];", 1, "unknown type `[[int]]`");
}

#[test]
fn empty_array_takes_its_type_from_the_slot() {
    ok("set xs: [int] = [];\nxs = [];\nfnc F(a: [str]): [str] ->\n    return [];\n<-;\nshow(F([]));");
    err("show([]);", 1, "an empty array `[]` needs a known type");
    err("set n: int = len([]);", 1, "an empty array `[]` needs a known type");
}

#[test]
fn indexing_rules() {
    err("set xs: [int] = [1];\nshow(xs[0.5]);", 2, "an array index must be int, got float");
    err("set n: int = 1;\nshow(n[0]);", 2, "only arrays can be indexed, got int");
    err("set xs: [str] = [\"a\"];\nxs[0] = 1;", 2, "an item of `xs` has type str, but got an int value");
    err("set xs: [int] = [1];\nshow(xs = xs);", 2, "arrays cannot be compared");
    err("show(len(5));", 1, "`len` needs a str or an array, got int");
    err("show(str([1]));", 1, "cannot convert [int] to str");
}

#[test]
fn push_rules() {
    ok("set xs: [float] = [];\npush(xs, 1);\npush(xs, 2.5);");
    err("set xs: [int] = [1];\npush(xs, \"a\");", 2, "cannot push a str value into `xs` ([int])");
    err("set n: int = 1;\npush(n, 2);", 2, "`push` needs an array, but `n` is an int");
    err("set xs: [int] = [1];\nshow(push(xs, 1));", 2, "`push(xs, value);` is a statement, not a value");
    err("fnc push(x: int)->\n<-;", 1, "`push` is a built-in function and cannot be redefined");
}

#[test]
fn arrays_follow_ownership_rules() {
    err("set keep xs: [int] = [1];\npush(xs, 2);", 2, "`xs` is a constant (keep) and cannot be changed");
    err("set keep xs: [int] = [1];\nxs[0] = 2;", 2, "`xs` is a constant (keep) and cannot be changed");
    err("set xs: [int] = [1];\nset b: [int] <== xs;\npush(b, 2);", 3, "`b` borrows `xs` and cannot change it");
    err("set xs: [int] = [1];\nset b: [int] <== xs;\npush(xs, 2);\nshow(b);", 4, "`b` borrows `xs`, but `xs` got a new value on line 3");
    err("set xs: [int] = [1];\nset ys: [int] = xs;\npush(xs, 1);", 3, "variable `xs` was moved on line 2");
    err("set s: str = \"a\";\nset xs: [str] = [];\npush(xs, s);\nshow(s);", 4, "variable `s` was moved on line 3");
    ok("set s: str = \"a\";\nset xs: [str] = [s];\nshow(s);");
}

// ------------------------------------------------------------- error reporting

#[test]
fn every_error_is_reported_with_its_column() {
    let src = "set age: int = \"x\";\nshow(age + 1);\nshow(nope);\n  set a: int = 1; show(b);\nshow(1)";
    let errors = parse(src).unwrap_err();
    let found: Vec<(usize, usize)> = errors.iter().map(|e| (e.line, e.column)).collect();
    // Line 2 has no error: `age` stays declared after its failed `set`.
    // Line 5: the missing `;` is reported where it belongs, right after `show(1)`.
    assert_eq!(found, vec![(1, 5), (3, 6), (4, 24), (5, 8)], "{errors:?}");
}

#[test]
fn errors_inside_blocks_do_not_stop_parsing() {
    let src = "fnc F(n: int): int ->\n    show(x);\n<-;\nif 1 ->\n    show(y);\n<-;\nshow(z);";
    let lines: Vec<usize> = parse(src).unwrap_err().iter().map(|e| e.line).collect();
    assert_eq!(lines, vec![1, 2, 4, 5, 7]);
}

#[test]
fn several_statements_on_one_line() {
    ok("set a: int = 1; set reuse b: int = 2; show(a + b);");
    ok("if 1 < 2 -> show(\"yes\"); <- else -> show(\"no\"); <-;");
    ok("set i: int = 0;\nwhile i < 3 -> show(i); i = i + 1; <-;");
    ok("fnc F(): int -> return 1; <-;\nshow(F());");
    err("set a: int = 1; show(a); show(b);", 1, "variable `b` is not declared");
}

#[test]
fn comment_errors_have_a_column() {
    let errors = parse("show(1);\n  % never closed").unwrap_err();
    assert_eq!((errors[0].line, errors[0].column), (2, 3));
}

// ------------------------------------------------------------- symbols for editors

#[test]
fn symbols_know_block_scopes_and_moves() {
    let src = "fnc F(keep who: str): int ->\n    if true ->\n        set x: int = 1;\n    <-;\n    return 1;\n<-;\n\
               set name: str = \"a\";\nset other: str = name;\nname = \"b\";\nset r: str <== name;\nshow(nope);";
    let s = super::symbols(src);
    let var = |n: &str| s.variables.iter().find(|v| v.name == n).unwrap_or_else(|| panic!("no symbol {n}"));
    assert_eq!((var("who").kind, var("who").end, var("who").function.as_deref()), ("keep", 6, Some("F")));
    assert_eq!((var("x").line, var("x").end), (3, 4), "x lives in the if block");
    assert_eq!((var("name").kind, var("name").column), ("owned", 5));
    assert_eq!(var("r").kind, "borrow");
    assert_eq!(var("name").end, 11, "top-level variables last until the end of the file");
    let events: Vec<(&str, usize, bool)> = s.events.iter().map(|e| (e.name.as_str(), e.line, e.moved)).collect();
    assert_eq!(events, vec![("name", 8, true), ("name", 9, false)]);
    assert_eq!(s.functions[0].end, 6);
}

#[test]
fn symbols_json_is_well_formed() {
    let json = super::symbols("fnc F(s: str = \"a\\\"b\")->\n<-;\nset t: [int] = [];").to_json();
    assert!(json.starts_with("{\"functions\":[{\"name\":\"F\""), "{json}");
    assert!(json.contains(r#""signature":"fnc F(s: str = \"a\\\"b\")""#), "{json}");
    assert!(json.contains(r#"{"name":"t","type":"[int]","kind":"owned","line":3,"column":5,"end":3,"function":null}"#), "{json}");
}

// ------------------------------------------------------------- scalars are copied

#[test]
fn ints_are_copied() {
    ok("set a: int = 1;\nset b: int = a;\nshow(a);\nfnc F(n: int)->\n<-;\nF(a);\nF(a);\nset f: float = 1.5;\nset g: float = f;\nshow(f);\nset t: bool = true;\nset u: bool = t;\nshow(t);");
}

#[test]
fn scalar_moves_are_not_marked() {
    let program = parse("set a: int = 1;\nset b: int = a;").unwrap();
    assert!(matches!(&program.main[1], Stmt::Set { value: Expr::Var { moved: false, .. }, .. }));
}

#[test]
fn reuse_on_a_scalar_warns() {
    let src = "set reuse i: int = 0;\nshow(i);";
    assert_eq!(warnings(src), vec!["Line 1: `reuse` has no effect on int values (they are always copied)"]);
    assert!(parse(src).is_ok());
    assert_eq!(warnings("fnc F(reuse n: int)->\n<-;"), vec!["Line 1: `reuse` has no effect on int values (they are always copied)"]);
    assert!(warnings("set reuse s: str = \"x\";\nfnc F(reuse a: [int])->\n<-;").is_empty());
}

// ------------------------------------------------------------- statements from tokens

#[test]
fn statements_can_span_lines() {
    ok("set price: int = 2;\nset total: int = price\n    * 3\n    + 1;\nfnc Greet(\n    name: str,\n    greeting: str = \"Hi\"\n)->\n    show(greeting + name);\n<-;\nGreet(\n    \"Meng\"\n);\nif total > 1\n   and price > 0 ->\n    show(total);\n<-;");
}

#[test]
fn multi_line_statement_with_tricky_string() {
    let program = parse("show(\n    \"a; b -> c % d\"\n);").unwrap();
    assert!(matches!(&program.main[..], [Stmt::Print(Expr::Str(s))] if s == "a; b -> c % d"), "{:?}", program.main);
}

#[test]
fn missing_semicolon_points_at_its_line() {
    err("show(1)\nshow(2);", 1, "missing `;`");
    err("show(1)", 1, "missing `;`");
}

#[test]
fn columns_are_exact() {
    let errors = parse("set a: int = 1;\nshow(a + b);\nshow(1 + \"x\");\nset s: str = 5;").unwrap_err();
    let found: Vec<(usize, usize)> = errors.iter().map(|e| (e.line, e.column)).collect();
    assert_eq!(found, vec![(2, 10), (3, 8), (4, 5)], "{errors:?}");
}

#[test]
fn a_lexing_error_only_breaks_its_own_line() {
    let errors = parse("show(\"abc);\nset x: int = 1;\nshow(x);\nshow(nope);").unwrap_err();
    let found: Vec<(usize, usize)> = errors.iter().map(|e| (e.line, e.column)).collect();
    assert_eq!(found, vec![(1, 6), (4, 6)], "{errors:?}");
}

#[test]
fn unclosed_multi_line_header() {
    err("fnc F(\n  a: int\n->\n<-;", 3, "invalid function syntax");
}

#[test]
fn loop_errors_point_at_the_variable() {
    let src = "fnc G(s: str)->\n<-;\nset a: str = \"x\";\nwhile true ->\n    G(a);\n<-;";
    let errors = parse(src).unwrap_err();
    assert_eq!((errors[0].line, errors[0].column), (5, 7), "{errors:?}");
}

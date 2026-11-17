//! Parser tests: ownership, types, calls, and syntax errors.
//! Each error test checks the line number and a fragment of the message.

use super::parse;
use crate::ast::Stmt;

/// The source must parse without errors.
fn ok(src: &str) {
    if let Err(e) = parse(src) {
        panic!("expected success, got error: {e}\n--- source ---\n{src}");
    }
}

/// The source must fail on `line` with a message containing `fragment`.
fn err(src: &str, line: usize, fragment: &str) {
    match parse(src) {
        Ok(_) => panic!("expected an error containing `{fragment}`, but it parsed\n--- source ---\n{src}"),
        Err(e) => {
            let prefix = format!("Line {line}: ");
            assert!(
                e.starts_with(&prefix) && e.contains(fragment),
                "expected `{prefix}...{fragment}...`, got `{e}`\n--- source ---\n{src}"
            );
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
    err("set a: int = 1;\nset b: int = a;\nif a = 1 ->\n<-;", 3, "was moved");
}

#[test]
fn ints_move_like_strings() {
    err("set a: int = 1;\nset b: int = a;\nset c: int = a;", 3, "was moved");
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
fn keep_and_reuse_parameters_can_be_used_many_times() {
    ok("fnc F(keep a: str, reuse b: str)->\n    set x: str = a;\n    set y: str = a;\n    set z: str = b;\n    set w: str = b;\n<-;");
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
    err("set a: int = 1;\na ()=> b;\nset c: str = b;", 3, "variable `c` has type str, but got a int value");
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
    err("set a: int = 1;\nif a ->\n<-;", 2, "a condition without an operator must be bool, got int");
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

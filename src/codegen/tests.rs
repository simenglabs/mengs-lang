use super::generate_c;
use crate::ast::{BinOp, Expr, Function, Param, Program, Stmt, Type};

fn main_only(stmts: Vec<Stmt>) -> Program {
    Program { functions: vec![], main: stmts }
}

fn var(name: &str, ty: Type) -> Expr {
    Expr::Var { name: name.into(), ty, moved: false }
}

fn print(s: &str) -> Stmt {
    Stmt::Print(Expr::Str(s.into()))
}

fn set(name: &str, ty: Type, keep: bool, value: Expr) -> Stmt {
    Stmt::Set { name: name.into(), ty, keep, borrow: false, value }
}

fn bin(lhs: Expr, op: BinOp, rhs: Expr) -> Expr {
    Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }
}

#[test]
fn generates_printf() {
    let c = generate_c(&main_only(vec![print("Hello")]));
    assert!(c.contains("printf(\"%s\\n\", \"Hello\");"));
    assert!(c.ends_with("return 0;\n}\n"));
}

#[test]
fn escapes_quotes_and_backslashes() {
    let c = generate_c(&main_only(vec![print(r#"a "b" \c"#)]));
    assert!(c.contains(r#""a \"b\" \\c""#));
}

#[test]
fn generates_all_types() {
    let c = generate_c(&main_only(vec![
        set("s", Type::Str, false, Expr::Str("Meng".into())),
        set("i", Type::Int, false, Expr::Int(7)),
        set("f", Type::Float, false, Expr::Float(2.0)),
        set("b", Type::Bool, false, Expr::Bool(true)),
        Stmt::Print(var("s", Type::Str)),
        Stmt::Print(var("i", Type::Int)),
        Stmt::Print(var("f", Type::Float)),
        Stmt::Print(var("b", Type::Bool)),
    ]));
    assert!(c.contains("char *v_s = meng_str_dup(\"Meng\");"));
    assert!(c.contains("long long v_i = 7LL;"));
    assert!(c.contains("double v_f = 2.0;"));
    assert!(c.contains("bool v_b = true;"));
    assert!(c.contains("printf(\"%s\\n\", v_s);"));
    assert!(c.contains("printf(\"%lld\\n\", v_i);"));
    assert!(c.contains("printf(\"%g\\n\", v_f);"));
    assert!(c.contains("printf(\"%s\\n\", v_b ? \"true\" : \"false\");"));
}

#[test]
fn keep_becomes_const() {
    let c = generate_c(&main_only(vec![
        set("name", Type::Str, true, Expr::Str("nasri".into())),
        set("age", Type::Int, true, Expr::Int(20)),
    ]));
    assert!(c.contains("char *const v_name = meng_str_dup(\"nasri\");"));
    assert!(c.contains("const long long v_age = 20LL;"));
}

#[test]
fn generates_functions() {
    let program = Program {
        functions: vec![Function {
            name: "Greet".into(),
            params: vec![
                Param { name: "name".into(), ty: Type::Str, keep: false, reuse: false, default: None },
                Param { name: "n".into(), ty: Type::Int, keep: true, reuse: false, default: Some(Expr::Int(3)) },
            ],
            ret: None,
            body: vec![Stmt::Print(var("name", Type::Str))],
        }],
        main: vec![
            // The parser fills in defaults, so codegen receives every argument.
            Stmt::Call { name: "Greet".into(), args: vec![Expr::Str("Meng".into()), Expr::Int(3)] },
            Stmt::Call { name: "Greet".into(), args: vec![Expr::Str("A".into()), Expr::Int(9)] },
        ],
    };
    let c = generate_c(&program);
    assert!(c.contains("static void f_Greet(char *v_name, const long long v_n);"));
    assert!(c.contains("f_Greet(meng_str_dup(\"Meng\"), 3LL);"));
    assert!(c.contains("f_Greet(meng_str_dup(\"A\"), 9LL);"));
    // The function owns its `str` parameter and frees it.
    assert!(c.contains("    free(v_name);\n}"));
}

#[test]
fn generates_if_else_chain() {
    let c = generate_c(&main_only(vec![Stmt::If {
        cond: bin(var("a", Type::Int), BinOp::Lt, Expr::Int(0)),
        body: vec![print("negative")],
        else_body: vec![Stmt::If {
            cond: bin(var("a", Type::Int), BinOp::Eq, Expr::Int(0)),
            body: vec![print("zero")],
            else_body: vec![print("positive")],
        }],
    }]));
    assert!(c.contains(
        "    if (v_a < 0LL) {\n        printf(\"%s\\n\", \"negative\");\n    } else if (v_a == 0LL) {\n        printf(\"%s\\n\", \"zero\");\n    } else {\n        printf(\"%s\\n\", \"positive\");\n    }\n"
    ));
}

#[test]
fn string_and_bool_conditions() {
    let c = generate_c(&main_only(vec![
        Stmt::If {
            cond: bin(var("s", Type::Str), BinOp::Eq, Expr::Str("x".into())),
            body: vec![],
            else_body: vec![],
        },
        Stmt::If { cond: var("active", Type::Bool), body: vec![], else_body: vec![] },
    ]));
    assert!(c.contains("if (strcmp(v_s, \"x\") == 0) {\n    }\n"));
    assert!(c.contains("if (v_active) {\n    }\n"));
}

#[test]
fn generates_operators_with_parentheses() {
    let sum = bin(Expr::Int(1), BinOp::Add, bin(Expr::Int(2), BinOp::Mul, var("a", Type::Int)));
    let cond = bin(
        bin(var("a", Type::Int), BinOp::Gt, Expr::Int(0)),
        BinOp::And,
        Expr::Unary { op: crate::ast::UnaryOp::Not, expr: Box::new(var("b", Type::Bool)) },
    );
    let neg = Expr::Unary { op: crate::ast::UnaryOp::Neg, expr: Box::new(Expr::Int(-3)) };
    let c = generate_c(&main_only(vec![
        set("x", Type::Int, false, sum),
        Stmt::If { cond, body: vec![], else_body: vec![] },
        set("y", Type::Int, false, neg),
    ]));
    assert!(c.contains("long long v_x = 1LL + (2LL * v_a);"), "{c}");
    assert!(c.contains("if ((v_a > 0LL) && (!v_b)) {"), "{c}");
    assert!(c.contains("long long v_y = (- -3LL);"), "{c}");
    assert!(!c.contains("meng_division_by_zero"), "helpers only when dividing");
}

#[test]
fn division_uses_checked_helpers() {
    let c = generate_c(&main_only(vec![
        set("i", Type::Int, false, bin(var("a", Type::Int), BinOp::Div, Expr::Int(2))),
        set("m", Type::Int, false, bin(var("a", Type::Int), BinOp::Mod, Expr::Int(2))),
        set("f", Type::Float, false, bin(var("a", Type::Int), BinOp::Div, Expr::Float(2.0))),
    ]));
    assert!(c.contains("long long v_i = meng_div_int(v_a, 2LL);"), "{c}");
    assert!(c.contains("long long v_m = meng_mod_int(v_a, 2LL);"), "{c}");
    assert!(c.contains("double v_f = meng_div_float(v_a, 2.0);"), "{c}");
    assert!(c.contains("void meng_division_by_zero(void)"), "{c}");
}

#[test]
fn generates_return_types_while_and_assign() {
    let program = Program {
        functions: vec![Function {
            name: "Twice".into(),
            params: vec![Param { name: "n".into(), ty: Type::Int, keep: false, reuse: false, default: None }],
            ret: Some(Type::Int),
            body: vec![Stmt::Return(Some(bin(var("n", Type::Int), BinOp::Mul, Expr::Int(2))))],
        }],
        main: vec![
            set("i", Type::Int, false, Expr::Int(0)),
            Stmt::While {
                cond: bin(var("i", Type::Int), BinOp::Lt, Expr::Int(3)),
                body: vec![Stmt::Assign {
                    name: "i".into(),
                    value: Expr::Call { name: "Twice".into(), args: vec![var("i", Type::Int)], ty: Type::Int },
                }],
            },
        ],
    };
    let c = generate_c(&program);
    assert!(c.contains("static long long f_Twice(long long v_n);"), "{c}");
    assert!(c.contains("    return v_n * 2LL;\n"), "{c}");
    assert!(c.contains("    while (v_i < 3LL) {\n        v_i = f_Twice(v_i);\n    }\n"), "{c}");
}

#[test]
fn strings_are_moved_borrowed_and_freed() {
    use crate::parser::parse;
    let src = "fnc Use(s: str)->\n    show(s);\n<-;\n\
               set a: str = \"x\";\nset b: str <== a;\nset c: str = a;\nUse(c);\n\
               set reuse r: str = \"y\";\nUse(r);\nif true ->\n    set inner: str = r + \"!\";\n<-;";
    let c = generate_c(&parse(src).unwrap());
    assert!(c.contains("char *v_a = meng_str_dup(\"x\");"), "{c}");
    assert!(c.contains("const char *v_b = v_a;"), "a borrow only points: {c}");
    assert!(c.contains("char *v_c = meng_take(&v_a);"), "a move hands the pointer over: {c}");
    assert!(c.contains("f_Use(meng_take(&v_c));"), "{c}");
    assert!(c.contains("f_Use(meng_str_dup(v_r));"), "reuse passes a copy: {c}");
    assert!(c.contains("char *v_inner = meng_str_dup(meng_concat(v_r, \"!\"));"), "{c}");
    assert!(c.contains("        free(v_inner);\n    }"), "freed at the end of its block: {c}");
    assert!(c.contains("    free(v_r);\n    free(v_c);\n    free(v_a);\n    return 0;"), "{c}");
    assert!(!c.contains("free(v_b)"), "borrows are never freed: {c}");
}

#[test]
fn return_frees_locals_and_temporaries() {
    use crate::parser::parse;
    let src = "fnc F(n: int): str ->\n    set s: str = \"a\";\n    if n > 0 ->\n        return s + \"b\";\n    <-;\n    return s;\n<-;";
    let c = generate_c(&parse(src).unwrap());
    assert!(c.contains("static char *f_F(long long v_n)"), "{c}");
    assert!(c.contains(
        "        {\n            char *meng_ret = meng_str_dup(meng_concat(v_s, \"b\"));\n            \
         meng_temps_release(meng_mark);\n            free(v_s);\n            return meng_ret;\n        }"
    ), "{c}");
}

#[test]
fn runtime_is_only_added_when_used() {
    use crate::parser::parse;
    let c = generate_c(&parse("show(\"hi\");\nshow(1 + 2);").unwrap());
    assert!(!c.contains("meng_temp"), "{c}");
    assert!(!c.contains("meng_div"), "{c}");
}

#[test]
fn arrays_are_built_moved_and_freed() {
    use crate::parser::parse;
    let src = "fnc Sum(xs: [int]): int ->\n    return len(xs);\n<-;\n\
               set a: [int] = [1, 2];\npush(a, 3);\na[0] = 9;\nset b: [int] <== a;\nshow(b[1]);\n\
               set c: [int] = a;\nshow(Sum(c));\nset s: [str] = [\"x\"];";
    let c = generate_c(&parse(src).unwrap());
    assert!(c.contains("meng_array *v_a = meng_push_int(meng_push_int(meng_array_new('i'), 1LL), 2LL);"), "{c}");
    assert!(c.contains("meng_push_int(v_a, 3LL);"), "{c}");
    assert!(c.contains("meng_put_int(v_a, 0LL, 9LL);"), "{c}");
    assert!(c.contains("const meng_array *v_b = v_a;"), "{c}");
    assert!(c.contains("meng_get_int(v_b, 1LL)"), "{c}");
    assert!(c.contains("meng_array *v_c = meng_take_array(&v_a);"), "{c}");
    assert!(c.contains("f_Sum(meng_take_array(&v_c))"), "{c}");
    assert!(c.contains("meng_push_str(meng_array_new('s'), meng_str_dup(\"x\"))"), "{c}");
    assert!(c.contains("    meng_array_free(v_s);\n    meng_array_free(v_c);\n    meng_array_free(v_a);\n"), "{c}");
}

use super::generate_c;
use crate::ast::{CmpOp, Cond, Expr, Function, Param, Program, Stmt, Type};

fn main_only(stmts: Vec<Stmt>) -> Program {
    Program { functions: vec![], main: stmts }
}

fn var(name: &str, ty: Type) -> Expr {
    Expr::Var { name: name.into(), ty }
}

fn print(s: &str) -> Stmt {
    Stmt::Print(Expr::Str(s.into()))
}

fn set(name: &str, ty: Type, keep: bool, value: Expr) -> Stmt {
    Stmt::Set { name: name.into(), ty, keep, value }
}

fn cmp(lhs: Expr, op: CmpOp, rhs: Expr) -> Cond {
    Cond::Compare { lhs, op, rhs }
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
    assert!(c.contains("const char *v_s = \"Meng\";"));
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
    assert!(c.contains("const char *const v_name = \"nasri\";"));
    assert!(c.contains("const long long v_age = 20LL;"));
}

#[test]
fn generates_functions_with_defaults() {
    let program = Program {
        functions: vec![Function {
            name: "Greet".into(),
            params: vec![
                Param { name: "name".into(), ty: Type::Str, keep: false, reuse: false, default: None },
                Param { name: "n".into(), ty: Type::Int, keep: true, reuse: false, default: Some(Expr::Int(3)) },
            ],
            body: vec![Stmt::Print(var("name", Type::Str))],
        }],
        main: vec![
            Stmt::Call { name: "Greet".into(), args: vec![Expr::Str("Meng".into())] },
            Stmt::Call { name: "Greet".into(), args: vec![Expr::Str("A".into()), Expr::Int(9)] },
        ],
    };
    let c = generate_c(&program);
    assert!(c.contains("static void f_Greet(const char *v_name, const long long v_n);"));
    assert!(c.contains("f_Greet(\"Meng\", 3LL);"));
    assert!(c.contains("f_Greet(\"A\", 9LL);"));
}

#[test]
fn generates_if_else_chain() {
    let c = generate_c(&main_only(vec![Stmt::If {
        cond: cmp(var("a", Type::Int), CmpOp::Lt, Expr::Int(0)),
        body: vec![print("negative")],
        else_body: vec![Stmt::If {
            cond: cmp(var("a", Type::Int), CmpOp::Eq, Expr::Int(0)),
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
            cond: cmp(var("s", Type::Str), CmpOp::Eq, Expr::Str("x".into())),
            body: vec![],
            else_body: vec![],
        },
        Stmt::If { cond: Cond::Bool(var("active", Type::Bool)), body: vec![], else_body: vec![] },
    ]));
    assert!(c.contains("if (strcmp(v_s, \"x\") == 0) {\n    }\n"));
    assert!(c.contains("if (v_active) {\n    }\n"));
}

//! Code generator: turns a `Program` into C source code.
//!
//! - `mod.rs`  — program layout, functions, statements (incl. `while`, `return`)
//! - `expr.rs` — expressions, C types, string escaping

mod expr;
#[cfg(test)]
mod tests;

use crate::ast::{Expr, Function, Program, Stmt, Type};
use expr::{c_type, declare, emit_call, emit_top};

/// Prefixes for C names so they never clash with C keywords or functions
/// (e.g. a variable named `int` or a function named `printf`).
const VAR_PREFIX: &str = "v_";
const FN_PREFIX: &str = "f_";
const INDENT: &str = "    ";

/// Runtime helpers, added only when the program divides.
const DIVISION_HELPERS: &str = "\
static void meng_division_by_zero(void) {
    fflush(stdout);
    fprintf(stderr, \"[ERROR] Runtime error: division by zero\\n\");
    exit(1);
}
static long long meng_div_int(long long a, long long b) { if (b == 0) meng_division_by_zero(); return a / b; }
static long long meng_mod_int(long long a, long long b) { if (b == 0) meng_division_by_zero(); return a % b; }
static double meng_div_float(double a, double b) { if (b == 0) meng_division_by_zero(); return a / b; }

";

/// Generate C source code for a program.
pub fn generate_c(program: &Program) -> String {
    let mut c = String::new();

    // Prototypes first, so functions can call each other regardless of order.
    for f in &program.functions {
        c.push_str(&format!("{};\n", fn_signature(f)));
    }
    if !program.functions.is_empty() {
        c.push('\n');
    }
    for f in &program.functions {
        c.push_str(&format!("{} {{\n", fn_signature(f)));
        emit_block(&mut c, &f.body, 1);
        c.push_str("}\n\n");
    }

    c.push_str("int main() {\n");
    emit_block(&mut c, &program.main, 1);
    c.push_str("    return 0;\n}\n");

    let mut header = String::from("#include <stdbool.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n\n");
    if c.contains("meng_div_") || c.contains("meng_mod_") {
        header.push_str(DIVISION_HELPERS);
    }
    header + &c
}

fn fn_signature(f: &Function) -> String {
    let params = if f.params.is_empty() {
        "void".to_string()
    } else {
        f.params
            .iter()
            .map(|p| declare(p.ty, p.keep, &p.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let ret = f.ret.map_or("void", c_type);
    format!("static {} {}{}({})", ret, FN_PREFIX, f.name, params)
}

fn emit_block(c: &mut String, stmts: &[Stmt], depth: usize) {
    let indent = INDENT.repeat(depth);
    for stmt in stmts {
        c.push_str(&indent);
        match stmt {
            Stmt::Set { name, ty, keep, value } => {
                c.push_str(&format!("{} = {};\n", declare(*ty, *keep, name), emit_top(value)));
            }
            Stmt::Assign { name, value } => {
                c.push_str(&format!("{}{} = {};\n", VAR_PREFIX, name, emit_top(value)));
            }
            Stmt::Print(expr) => c.push_str(&emit_print(expr)),
            Stmt::Call { name, args } => c.push_str(&format!("{};\n", emit_call(name, args))),
            Stmt::If { .. } => emit_if(c, stmt, depth),
            Stmt::While { cond, body } => {
                c.push_str(&format!("while ({}) {{\n", emit_top(cond)));
                emit_block(c, body, depth + 1);
                c.push_str(&format!("{}}}\n", indent));
            }
            Stmt::Return(None) => c.push_str("return;\n"),
            Stmt::Return(Some(value)) => c.push_str(&format!("return {};\n", emit_top(value))),
        }
    }
}

/// `if (...) { ... } else if (...) { ... } else { ... }`
/// (the caller already wrote the indentation of the first line).
fn emit_if(c: &mut String, stmt: &Stmt, depth: usize) {
    let Stmt::If { cond, body, else_body } = stmt else {
        return;
    };
    let indent = INDENT.repeat(depth);
    c.push_str(&format!("if ({}) {{\n", emit_top(cond)));
    emit_block(c, body, depth + 1);
    c.push_str(&indent);
    c.push('}');

    match else_body.as_slice() {
        [] => {}
        [nested @ Stmt::If { .. }] => {
            c.push_str(" else ");
            emit_if(c, nested, depth);
            return;
        }
        _ => {
            c.push_str(" else {\n");
            emit_block(c, else_body, depth + 1);
            c.push_str(&indent);
            c.push('}');
        }
    }
    c.push('\n');
}

fn emit_print(expr: &Expr) -> String {
    let e = emit_top(expr);
    match expr.ty() {
        Type::Str => format!("printf(\"%s\\n\", {});\n", e),
        Type::Int => format!("printf(\"%lld\\n\", {});\n", e),
        Type::Float => format!("printf(\"%g\\n\", {});\n", e),
        Type::Bool => format!("printf(\"%s\\n\", {} ? \"true\" : \"false\");\n", e),
    }
}

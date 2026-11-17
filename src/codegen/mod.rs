//! Code generator: turns a `Program` into C source code.
//!
//! - `mod.rs`     — program layout, functions, statements, freeing owned strings
//! - `expr.rs`    — expressions, C types, owned values, string escaping
//! - `runtime.rs` — C helpers (division checks, heap strings), added when used
//!
//! Memory: an owned `str` variable is freed when its block ends (or on `return`).
//! A move sets the source to NULL, so freeing it afterwards does nothing.

mod expr;
mod runtime;
#[cfg(test)]
mod tests;

use crate::ast::{Expr, Function, Program, Stmt, Type, always_returns};
use expr::{c_decl, c_return_type, declare, emit_call, emit_owned, emit_top, has_temps};

/// Prefixes for C names so they never clash with C keywords or functions
/// (e.g. a variable named `int` or a function named `printf`).
const VAR_PREFIX: &str = "v_";
const FN_PREFIX: &str = "f_";
const INDENT: &str = "    ";

/// Generate C source code for a program.
pub fn generate_c(program: &Program) -> String {
    let mut e = Emitter { c: String::new(), functions: &program.functions, scopes: Vec::new(), mark: "0", temps: false };

    // Prototypes first, so functions can call each other regardless of order.
    for f in &program.functions {
        e.c.push_str(&format!("{};\n", fn_signature(f)));
    }
    if !program.functions.is_empty() {
        e.c.push('\n');
    }
    for f in &program.functions {
        e.function(f);
    }

    e.c.push_str("int main(void) {\n");
    e.mark = "0";
    e.temps = false;
    e.block(&program.main, 1, Vec::new());
    e.c.push_str("    return 0;\n}\n");

    let c = e.c;
    let mut out = String::from(
        "#include <ctype.h>\n#include <errno.h>\n#include <stdbool.h>\n#include <stdio.h>\n\
         #include <stdlib.h>\n#include <string.h>\n\n",
    );
    if c.contains("meng_div_") || c.contains("meng_mod_") {
        out.push_str(runtime::DIVISION);
    }
    const STRING_HELPERS: &[&str] =
        &["meng_temp", "meng_str_", "meng_take", "meng_concat", "meng_int_to_str", "meng_float_to_str", "meng_read"];
    if STRING_HELPERS.iter().any(|h| c.contains(h)) {
        out.push_str(runtime::STRINGS);
    }
    out + &c
}

fn fn_signature(f: &Function) -> String {
    let params = if f.params.is_empty() {
        "void".to_string()
    } else {
        f.params
            .iter()
            .map(|p| declare(p.ty, p.keep, false, &p.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let name = format!("{}{}({})", FN_PREFIX, f.name, params);
    format!("static {}", c_decl(c_return_type(f.ret), &name))
}

struct Emitter<'a> {
    c: String,
    functions: &'a [Function],
    /// Owned `str` variables of each open block, innermost last.
    scopes: Vec<Vec<String>>,
    /// Where the current function's temporaries start (`meng_mark`, or `0` in `main`).
    mark: &'static str,
    /// Whether the current function creates temporaries at all.
    temps: bool,
}

impl Emitter<'_> {
    fn line(&mut self, depth: usize, text: &str) {
        self.c.push_str(&INDENT.repeat(depth));
        self.c.push_str(text);
        self.c.push('\n');
    }

    fn function(&mut self, f: &Function) {
        self.c.push_str(&format!("{} {{\n", fn_signature(f)));
        self.mark = "meng_mark";
        self.temps = f.body.iter().any(|s| stmt_has_temps_deep(s, self.functions));
        if self.temps {
            self.line(1, "size_t meng_mark = meng_temps_mark();");
        }
        let owned_params = f.params.iter().filter(|p| p.ty == Type::Str).map(|p| p.name.clone()).collect();
        self.block(&f.body, 1, owned_params);
        self.c.push_str("}\n\n");
    }

    /// A block of statements; its owned strings are freed at the end.
    fn block(&mut self, stmts: &[Stmt], depth: usize, owned: Vec<String>) {
        self.scopes.push(owned);
        for stmt in stmts {
            self.stmt(stmt, depth);
            if stmt_has_temps(stmt, self.functions) {
                self.release(depth);
            }
        }
        let scope = self.scopes.pop().unwrap_or_default();
        if !always_returns(stmts) {
            for name in scope.iter().rev() {
                self.line(depth, &format!("free({}{});", VAR_PREFIX, name));
            }
        }
    }

    fn release(&mut self, depth: usize) {
        self.line(depth, &format!("meng_temps_release({});", self.mark));
    }

    fn stmt(&mut self, stmt: &Stmt, depth: usize) {
        match stmt {
            Stmt::Set { name, ty, keep, borrow, value } => {
                let value = if *borrow { emit_top(value) } else { emit_owned(value) };
                self.line(depth, &format!("{} = {};", declare(*ty, *keep, *borrow, name), value));
                if *ty == Type::Str && !*borrow {
                    self.scopes.last_mut().expect("inside a block").push(name.clone());
                }
            }
            Stmt::Assign { name, value } if value.ty() == Type::Str => {
                self.line(depth, &format!("meng_str_set(&{}{}, {});", VAR_PREFIX, name, emit_owned(value)));
            }
            Stmt::Assign { name, value } => {
                self.line(depth, &format!("{}{} = {};", VAR_PREFIX, name, emit_top(value)));
            }
            Stmt::Print(expr) => self.line(depth, &emit_print(expr)),
            Stmt::Call { name, args } => {
                let call = emit_call(name, args);
                // A returned string that nobody uses is still owned: make it a temporary.
                let call = if self.returns_str(name) { format!("meng_temp({})", call) } else { call };
                self.line(depth, &format!("{};", call));
            }
            Stmt::If { .. } => {
                self.c.push_str(&INDENT.repeat(depth));
                self.emit_if(stmt, depth);
            }
            Stmt::While { cond, body } => {
                self.line(depth, &format!("while ({}) {{", emit_top(cond)));
                if has_temps(cond) {
                    self.release(depth + 1);
                }
                self.block(body, depth + 1, Vec::new());
                self.line(depth, "}");
            }
            Stmt::Return(value) => self.emit_return(value.as_ref(), depth),
        }
    }

    /// `return`: compute the value first, then free every open block's strings.
    fn emit_return(&mut self, value: Option<&Expr>, depth: usize) {
        let mut cleanup: Vec<String> = Vec::new();
        // Also releases temporaries of enclosing conditions (`if F() = "x" -> return 1;`).
        if self.temps {
            cleanup.push(format!("meng_temps_release({});", self.mark));
        }
        for scope in self.scopes.iter().rev() {
            for name in scope.iter().rev() {
                cleanup.push(format!("free({}{});", VAR_PREFIX, name));
            }
        }
        match value {
            None => {
                for line in cleanup {
                    self.line(depth, &line);
                }
                self.line(depth, "return;");
            }
            Some(v) if cleanup.is_empty() => self.line(depth, &format!("return {};", emit_owned(v))),
            Some(v) => {
                let ty = c_return_type(Some(v.ty()));
                self.line(depth, "{");
                self.line(depth + 1, &format!("{} = {};", c_decl(ty, "meng_ret"), emit_owned(v)));
                for line in cleanup {
                    self.line(depth + 1, &line);
                }
                self.line(depth + 1, "return meng_ret;");
                self.line(depth, "}");
            }
        }
    }

    /// `if (...) { ... } else if (...) { ... } else { ... }`
    /// (the caller already wrote the indentation of the first line).
    fn emit_if(&mut self, stmt: &Stmt, depth: usize) {
        let Stmt::If { cond, body, else_body } = stmt else {
            return;
        };
        let indent = INDENT.repeat(depth);
        self.c.push_str(&format!("if ({}) {{\n", emit_top(cond)));
        self.block(body, depth + 1, Vec::new());
        self.c.push_str(&indent);
        self.c.push('}');

        match else_body.as_slice() {
            [] => {}
            [nested @ Stmt::If { .. }] => {
                self.c.push_str(" else ");
                self.emit_if(nested, depth);
                return;
            }
            _ => {
                self.c.push_str(" else {\n");
                self.block(else_body, depth + 1, Vec::new());
                self.c.push_str(&indent);
                self.c.push('}');
            }
        }
        self.c.push('\n');
    }

    fn returns_str(&self, name: &str) -> bool {
        self.functions.iter().any(|f| f.name == name && f.ret == Some(Type::Str))
    }
}

/// Temporaries created by the statement's own expressions (not nested blocks).
/// For an `if`, that includes every condition of its `else if` chain.
fn stmt_has_temps(stmt: &Stmt, functions: &[Function]) -> bool {
    match stmt {
        Stmt::Set { value, .. } | Stmt::Assign { value, .. } | Stmt::Print(value) => has_temps(value),
        Stmt::Return(value) => value.as_ref().is_some_and(has_temps),
        Stmt::Call { name, args } => {
            args.iter().any(has_temps) || functions.iter().any(|f| &f.name == name && f.ret == Some(Type::Str))
        }
        Stmt::If { cond, else_body, .. } => {
            has_temps(cond) || matches!(else_body.as_slice(), [nested @ Stmt::If { .. }] if stmt_has_temps(nested, functions))
        }
        Stmt::While { cond, .. } => has_temps(cond),
    }
}

/// Whether anything inside the statement (including nested blocks) creates temporaries.
fn stmt_has_temps_deep(stmt: &Stmt, functions: &[Function]) -> bool {
    let deep = |stmts: &[Stmt]| stmts.iter().any(|s| stmt_has_temps_deep(s, functions));
    match stmt {
        Stmt::If { cond, body, else_body } => has_temps(cond) || deep(body) || deep(else_body),
        Stmt::While { cond, body } => has_temps(cond) || deep(body),
        _ => stmt_has_temps(stmt, functions),
    }
}

fn emit_print(expr: &Expr) -> String {
    let e = emit_top(expr);
    match expr.ty() {
        Type::Str => format!("printf(\"%s\\n\", {});", e),
        Type::Int => format!("printf(\"%lld\\n\", {});", e),
        Type::Float => format!("printf(\"%g\\n\", {});", e),
        Type::Bool => format!("printf(\"%s\\n\", {} ? \"true\" : \"false\");", e),
    }
}

//! Tree-walking interpreter: runs a `Program` directly, without GCC.
//!
//! The parser has already checked types, ownership, and function calls, so the
//! interpreter only evaluates. Output matches the compiled C program exactly.
//!
//! - `mod.rs`   — statements, function calls, expressions
//! - `value.rs` — runtime values and printing

mod value;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::Write;

use crate::ast::{BinOp, Expr, Function, Program, Stmt, UnaryOp};
use value::Value;

/// Variables of one function call (or of the top level).
/// Block scoping is enforced by the parser, so one map per call is enough.
type Frame = HashMap<String, Value>;

/// Run a program, writing its output to `out`.
/// Errors are runtime errors (e.g. division by zero) or failures to write output.
pub fn run(program: &Program, out: &mut dyn Write) -> Result<(), String> {
    let functions = program.functions.iter().map(|f| (f.name.as_str(), f)).collect();
    let mut interp = Interpreter { functions, out };
    interp.exec_block(&program.main, &mut Frame::new())
}

struct Interpreter<'p, 'o> {
    functions: HashMap<&'p str, &'p Function>,
    out: &'o mut dyn Write,
}

impl Interpreter<'_, '_> {
    fn exec_block(&mut self, stmts: &[Stmt], frame: &mut Frame) -> Result<(), String> {
        for stmt in stmts {
            match stmt {
                Stmt::Set { name, ty, value, .. } => {
                    let v = eval(value, frame)?.convert_to(*ty);
                    frame.insert(name.clone(), v);
                }
                Stmt::Print(expr) => {
                    let v = eval(expr, frame)?;
                    writeln!(self.out, "{}", v).map_err(|e| format!("failed to write output: {}", e))?;
                }
                Stmt::Call { name, args } => self.call(name, args, frame)?,
                Stmt::If { cond, body, else_body } => {
                    let branch = if eval(cond, frame)?.as_bool() { body } else { else_body };
                    self.exec_block(branch, frame)?;
                }
            }
        }
        Ok(())
    }

    /// Call a function in a fresh frame; missing arguments use parameter defaults.
    fn call(&mut self, name: &str, args: &[Expr], frame: &Frame) -> Result<(), String> {
        let f = self.functions[name]; // existence is checked by the parser
        let mut callee = Frame::new();
        for (i, param) in f.params.iter().enumerate() {
            let value = match args.get(i) {
                Some(arg) => eval(arg, frame)?,
                None => {
                    let default = param.default.as_ref().expect("argument count is checked by the parser");
                    eval(default, &Frame::new())?
                }
            };
            callee.insert(param.name.clone(), value.convert_to(param.ty));
        }
        self.exec_block(&f.body, &mut callee)
    }
}

fn eval(expr: &Expr, frame: &Frame) -> Result<Value, String> {
    Ok(match expr {
        Expr::Str(s) => Value::Str(s.clone()),
        Expr::Int(n) => Value::Int(*n),
        Expr::Float(f) => Value::Float(*f),
        Expr::Bool(b) => Value::Bool(*b),
        Expr::Var { name, .. } => frame.get(name).cloned().expect("variables are checked by the parser"),
        Expr::Unary { op: UnaryOp::Not, expr } => Value::Bool(!eval(expr, frame)?.as_bool()),
        Expr::Unary { op: UnaryOp::Neg, expr } => eval(expr, frame)?.negate(),
        // `and` / `or` short-circuit, like C's `&&` / `||`.
        Expr::Binary { op: BinOp::And, lhs, rhs } => {
            Value::Bool(eval(lhs, frame)?.as_bool() && eval(rhs, frame)?.as_bool())
        }
        Expr::Binary { op: BinOp::Or, lhs, rhs } => {
            Value::Bool(eval(lhs, frame)?.as_bool() || eval(rhs, frame)?.as_bool())
        }
        Expr::Binary { op, lhs, rhs } => {
            let (l, r) = (eval(lhs, frame)?, eval(rhs, frame)?);
            if op.is_arithmetic() {
                l.arithmetic(*op, &r)?
            } else {
                let ord = l.compare(&r);
                Value::Bool(match op {
                    BinOp::Eq => ord.is_eq(),
                    BinOp::Ne => ord.is_ne(),
                    BinOp::Lt => ord.is_lt(),
                    BinOp::Gt => ord.is_gt(),
                    BinOp::Le => ord.is_le(),
                    _ => ord.is_ge(),
                })
            }
        }
    })
}

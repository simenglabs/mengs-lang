//! Tree-walking interpreter: runs a `Program` directly, without GCC.
//!
//! The parser has already checked types, ownership, and function calls, so the
//! interpreter only evaluates. Output matches the compiled C program exactly.
//!
//! - `mod.rs`   — statements, function calls, conditions
//! - `value.rs` — runtime values and printing

mod value;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::{self, Write};

use crate::ast::{CmpOp, Cond, Expr, Function, Program, Stmt};
use value::Value;

/// Variables of one function call (or of the top level).
/// Block scoping is enforced by the parser, so one map per call is enough.
type Frame = HashMap<String, Value>;

/// Run a program, writing its output to `out`.
pub fn run(program: &Program, out: &mut dyn Write) -> io::Result<()> {
    let functions = program.functions.iter().map(|f| (f.name.as_str(), f)).collect();
    let mut interp = Interpreter { functions, out };
    interp.exec_block(&program.main, &mut Frame::new())
}

struct Interpreter<'p, 'o> {
    functions: HashMap<&'p str, &'p Function>,
    out: &'o mut dyn Write,
}

impl Interpreter<'_, '_> {
    fn exec_block(&mut self, stmts: &[Stmt], frame: &mut Frame) -> io::Result<()> {
        for stmt in stmts {
            match stmt {
                Stmt::Set { name, ty, value, .. } => {
                    let v = eval(value, frame).convert_to(*ty);
                    frame.insert(name.clone(), v);
                }
                Stmt::Print(expr) => writeln!(self.out, "{}", eval(expr, frame))?,
                Stmt::Call { name, args } => self.call(name, args, frame)?,
                Stmt::If { cond, body, else_body } => {
                    let branch = if eval_cond(cond, frame) { body } else { else_body };
                    self.exec_block(branch, frame)?;
                }
            }
        }
        Ok(())
    }

    /// Call a function in a fresh frame; missing arguments use parameter defaults.
    fn call(&mut self, name: &str, args: &[Expr], frame: &Frame) -> io::Result<()> {
        let f = self.functions[name]; // existence is checked by the parser
        let mut callee = Frame::new();
        for (i, param) in f.params.iter().enumerate() {
            let value = match args.get(i) {
                Some(arg) => eval(arg, frame),
                None => {
                    let default = param.default.as_ref().expect("argument count is checked by the parser");
                    eval(default, &Frame::new())
                }
            };
            callee.insert(param.name.clone(), value.convert_to(param.ty));
        }
        self.exec_block(&f.body, &mut callee)
    }
}

fn eval(expr: &Expr, frame: &Frame) -> Value {
    match expr {
        Expr::Str(s) => Value::Str(s.clone()),
        Expr::Int(n) => Value::Int(*n),
        Expr::Float(f) => Value::Float(*f),
        Expr::Bool(b) => Value::Bool(*b),
        Expr::Var { name, .. } => frame
            .get(name)
            .cloned()
            .expect("variables are checked by the parser"),
    }
}

fn eval_cond(cond: &Cond, frame: &Frame) -> bool {
    match cond {
        Cond::Bool(e) => eval(e, frame).as_bool(),
        Cond::Compare { lhs, op, rhs } => {
            let ord = eval(lhs, frame).compare(&eval(rhs, frame));
            match op {
                CmpOp::Eq => ord.is_eq(),
                CmpOp::Ne => ord.is_ne(),
                CmpOp::Lt => ord.is_lt(),
                CmpOp::Gt => ord.is_gt(),
                CmpOp::Le => ord.is_le(),
                CmpOp::Ge => ord.is_ge(),
            }
        }
    }
}

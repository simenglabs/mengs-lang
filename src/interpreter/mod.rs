//! Tree-walking interpreter: runs a `Program` directly, without GCC.
//!
//! The parser has already checked types, ownership, and function calls, so the
//! interpreter only evaluates. Output matches the compiled C program exactly.
//!
//! - `mod.rs`   — statements (incl. `while`, `return`), function calls, expressions
//! - `value.rs` — runtime values and printing

mod value;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::{BufRead, Write};

use crate::ast::{BinOp, Expr, Function, Program, Stmt, UnaryOp};
use value::Value;

/// Variables of one function call (or of the top level).
/// Block scoping is enforced by the parser, so one map per call is enough.
type Frame = HashMap<String, Value>;

/// What happens after running a block: carry on, or leave the function.
enum Flow {
    Next,
    Return(Option<Value>),
}

/// Run a program, reading `read()` lines from `input` and writing output to `out`.
/// Errors are runtime errors (e.g. division by zero) or I/O failures.
pub fn run(program: &Program, input: &mut dyn BufRead, out: &mut dyn Write) -> Result<(), String> {
    let functions = program.functions.iter().map(|f| (f.name.as_str(), f)).collect();
    let mut interp = Interpreter { functions, input, out };
    interp.exec_block(&program.main, &mut Frame::new()).map(|_| ())
}

struct Interpreter<'p, 'o> {
    functions: HashMap<&'p str, &'p Function>,
    input: &'o mut dyn BufRead,
    out: &'o mut dyn Write,
}

impl Interpreter<'_, '_> {
    fn exec_block(&mut self, stmts: &[Stmt], frame: &mut Frame) -> Result<Flow, String> {
        for stmt in stmts {
            match stmt {
                Stmt::Set { name, ty, value, .. } => {
                    let v = self.eval(value, frame)?.convert_to(*ty);
                    frame.insert(name.clone(), v);
                }
                Stmt::Assign { name, value } => {
                    let v = self.eval(value, frame)?;
                    let slot = frame.get_mut(name).expect("variables are checked by the parser");
                    *slot = v.convert_to(slot.ty());
                }
                Stmt::Print(expr) => {
                    let v = self.eval(expr, frame)?;
                    writeln!(self.out, "{}", v).map_err(io_error)?;
                }
                Stmt::Call { name, args } => {
                    self.call(name, args, frame)?;
                }
                Stmt::If { cond, body, else_body } => {
                    let branch = if self.eval(cond, frame)?.as_bool() { body } else { else_body };
                    if let Flow::Return(v) = self.exec_block(branch, frame)? {
                        return Ok(Flow::Return(v));
                    }
                }
                Stmt::While { cond, body } => {
                    while self.eval(cond, frame)?.as_bool() {
                        if let Flow::Return(v) = self.exec_block(body, frame)? {
                            return Ok(Flow::Return(v));
                        }
                    }
                }
                Stmt::Return(value) => {
                    let v = value.as_ref().map(|e| self.eval(e, frame)).transpose()?;
                    return Ok(Flow::Return(v));
                }
            }
        }
        Ok(Flow::Next)
    }

    /// Call a function in a fresh frame (default arguments were filled in by the parser).
    /// Returns its return value, if it has one.
    fn call(&mut self, name: &str, args: &[Expr], frame: &mut Frame) -> Result<Option<Value>, String> {
        let f = self.functions[name]; // existence is checked by the parser
        let mut callee = Frame::new();
        for (param, arg) in f.params.iter().zip(args) {
            let value = self.eval(arg, frame)?.convert_to(param.ty);
            callee.insert(param.name.clone(), value);
        }
        Ok(match self.exec_block(&f.body, &mut callee)? {
            Flow::Return(Some(v)) => f.ret.map(|ty| v.convert_to(ty)),
            _ => None,
        })
    }

    fn eval(&mut self, expr: &Expr, frame: &mut Frame) -> Result<Value, String> {
        Ok(match expr {
            Expr::Str(s) => Value::Str(s.clone()),
            Expr::Int(n) => Value::Int(*n),
            Expr::Float(f) => Value::Float(*f),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Var { name, .. } => frame.get(name).cloned().expect("variables are checked by the parser"),
            Expr::Call { name, args, .. } => {
                self.call(name, args, frame)?.expect("the parser only allows calls that return a value")
            }
            Expr::Convert { to, expr } => self.eval(expr, frame)?.convert(*to)?,
            Expr::Read { prompt } => {
                if let Some(prompt) = prompt {
                    let p = self.eval(prompt, frame)?;
                    write!(self.out, "{}", p).and_then(|_| self.out.flush()).map_err(io_error)?;
                }
                let mut line = String::new();
                self.input.read_line(&mut line).map_err(io_error)?;
                let line = line.strip_suffix('\n').unwrap_or(&line);
                Value::Str(line.strip_suffix('\r').unwrap_or(line).to_string())
            }
            Expr::Unary { op: UnaryOp::Not, expr } => Value::Bool(!self.eval(expr, frame)?.as_bool()),
            Expr::Unary { op: UnaryOp::Neg, expr } => self.eval(expr, frame)?.negate(),
            // `and` / `or` short-circuit, like C's `&&` / `||`.
            Expr::Binary { op: BinOp::And, lhs, rhs } => {
                Value::Bool(self.eval(lhs, frame)?.as_bool() && self.eval(rhs, frame)?.as_bool())
            }
            Expr::Binary { op: BinOp::Or, lhs, rhs } => {
                Value::Bool(self.eval(lhs, frame)?.as_bool() || self.eval(rhs, frame)?.as_bool())
            }
            Expr::Binary { op, lhs, rhs } => {
                let (l, r) = (self.eval(lhs, frame)?, self.eval(rhs, frame)?);
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
}

fn io_error(e: std::io::Error) -> String {
    format!("input/output error: {}", e)
}

use super::{Cond, Expr, Type};

/// A single statement in the program, a function body, or an `if` block.
#[derive(Debug, PartialEq)]
pub enum Stmt {
    /// Also produced by a borrow (`set b: T <== a;`) and a copy (`a ()=> b;`).
    /// `keep` = constant (`const` in C). Ownership rules are checked by the parser.
    Set { name: String, ty: Type, keep: bool, value: Expr },
    Print(Expr),
    Call { name: String, args: Vec<Expr> },
    /// `else if` is stored as an `else_body` containing a single `If`.
    If { cond: Cond, body: Vec<Stmt>, else_body: Vec<Stmt> },
}

/// Function parameter: `[keep|reuse] name: type [= literal]`.
#[derive(Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub keep: bool,
    pub reuse: bool,
    pub default: Option<Expr>,
}

/// Function definition: `fnc Name(a: str, b: int = 0)-> ... <-;`
#[derive(Debug, PartialEq)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
}

/// A whole program: function definitions plus top-level statements (`main`).
#[derive(Debug, PartialEq, Default)]
pub struct Program {
    pub functions: Vec<Function>,
    pub main: Vec<Stmt>,
}

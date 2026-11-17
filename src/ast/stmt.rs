use super::{Expr, Type};

/// A single statement in the program, a function body, or an `if` block.
#[derive(Debug, PartialEq)]
pub enum Stmt {
    /// Also produced by a borrow (`set b: T <== a;`) and a copy (`a ()=> b;`).
    /// `keep` = constant (`const` in C). `borrow` = refers to another variable's
    /// value without owning it. Ownership rules are checked by the parser.
    Set { name: String, ty: Type, keep: bool, borrow: bool, value: Expr },
    /// `name = value;` (not allowed on `keep` or borrows).
    Assign { name: String, value: Expr },
    Print(Expr),
    /// A call used as a statement (any return value is ignored). `args` include defaults.
    Call { name: String, args: Vec<Expr> },
    /// `else if` is stored as an `else_body` containing a single `If`.
    If { cond: Expr, body: Vec<Stmt>, else_body: Vec<Stmt> },
    While { cond: Expr, body: Vec<Stmt> },
    /// `return value;` or `return;` (only inside functions).
    Return(Option<Expr>),
}

/// Whether a block always ends in `return` (directly, or in both branches of an `if`).
pub fn always_returns(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Return(_) => true,
        Stmt::If { body, else_body, .. } => always_returns(body) && always_returns(else_body),
        _ => false,
    })
}

/// Function parameter: `[keep|reuse] name: type [= literal]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub keep: bool,
    pub reuse: bool,
    pub default: Option<Expr>,
}

/// Function definition: `fnc Name(a: str, b: int = 0): int -> ... <-;`
#[derive(Debug, PartialEq)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    /// `None` = no return value.
    pub ret: Option<Type>,
    pub body: Vec<Stmt>,
}

/// Everything a caller needs to know about a function (its header).
#[derive(Debug, Clone, PartialEq)]
pub struct Signature {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
}

/// A whole program: function definitions plus top-level statements (`main`).
#[derive(Debug, PartialEq, Default)]
pub struct Program {
    pub functions: Vec<Function>,
    pub main: Vec<Stmt>,
}

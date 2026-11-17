use super::Type;

/// A value: a literal or a variable. Variable types are resolved by the parser.
#[derive(Debug, PartialEq)]
pub enum Expr {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Var { name: String, ty: Type },
}

impl Expr {
    pub fn ty(&self) -> Type {
        match self {
            Expr::Str(_) => Type::Str,
            Expr::Int(_) => Type::Int,
            Expr::Float(_) => Type::Float,
            Expr::Bool(_) => Type::Bool,
            Expr::Var { ty, .. } => *ty,
        }
    }
}

/// Comparison operator used in `if`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

/// An `if` condition: a comparison (`a < b`) or a single bool value (`active`).
#[derive(Debug, PartialEq)]
pub enum Cond {
    Compare { lhs: Expr, op: CmpOp, rhs: Expr },
    Bool(Expr),
}

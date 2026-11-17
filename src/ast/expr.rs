use super::Type;

/// An expression. Types are checked by the parser, so `ty()` is always valid.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Var { name: String, ty: Type },
    Unary { op: UnaryOp, expr: Box<Expr> },
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    /// Call of a function that returns a value. `args` already include defaults.
    Call { name: String, args: Vec<Expr>, ty: Type },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    /// `-x`
    Neg,
    /// `not x`
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    /// `mod` (remainder). `%` is taken by comments.
    Mod,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

impl BinOp {
    /// How the operator is written in Meng source.
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "mod",
            BinOp::Eq => "=",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Le => "<=",
            BinOp::Ge => ">=",
            BinOp::And => "and",
            BinOp::Or => "or",
        }
    }

    pub fn is_arithmetic(self) -> bool {
        matches!(self, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod)
    }
}

impl Expr {
    pub fn ty(&self) -> Type {
        match self {
            Expr::Str(_) => Type::Str,
            Expr::Int(_) => Type::Int,
            Expr::Float(_) => Type::Float,
            Expr::Bool(_) => Type::Bool,
            Expr::Var { ty, .. } | Expr::Call { ty, .. } => *ty,
            Expr::Unary { op: UnaryOp::Neg, expr } => expr.ty(),
            Expr::Unary { op: UnaryOp::Not, .. } => Type::Bool,
            // int op int -> int; any float makes the result float.
            Expr::Binary { op, lhs, rhs } if op.is_arithmetic() => {
                if lhs.ty() == Type::Float || rhs.ty() == Type::Float { Type::Float } else { Type::Int }
            }
            Expr::Binary { .. } => Type::Bool,
        }
    }

    /// A plain literal value (allowed as a parameter default).
    pub fn is_literal(&self) -> bool {
        matches!(self, Expr::Str(_) | Expr::Int(_) | Expr::Float(_) | Expr::Bool(_))
    }
}

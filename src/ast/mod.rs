//! Abstract syntax tree: the data produced by the parser and consumed by codegen.

mod expr;
mod stmt;
mod types;

pub use expr::{BinOp, Expr, UnaryOp};
pub use stmt::{Function, Param, Program, Signature, Stmt, always_returns};
pub use types::Type;

//! AST node definitions for Kairo v0.1 milestone scope.
//!
//! This deliberately only covers what's needed to represent:
//!
//! ```text
//! fn main() {
//!     name := "World"
//!     print("Hello, " + name)
//! }
//! ```

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub functions: Vec<FunctionDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDecl {
    pub name: String,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// `name := <expr>`
    VariableDecl { name: String, value: Expr },
    /// An expression evaluated for its side effect, e.g. `print(...)`
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    StringLiteral(String),
    Identifier(String),
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Call {
        callee: String,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
}
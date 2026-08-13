//! AST node definitions for Kairo v0.4 milestone scope.
//!
//! Covers function declarations, immutable/mutable variable
//! declarations, reassignment, string and int literals, booleans,
//! arithmetic/comparison binary operators, calls, if/else
//! statements, while loops, and grouping expressions (grouping
//! doesn't need its own node — it just controls parse order).

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
    /// `name := <expr>` (is_mutable: false) or `mut name := <expr>` (true)
    VariableDecl {
        name: String,
        value: Expr,
        is_mutable: bool,
    },
    /// `name = <expr>` — reassigns an existing mutable variable
    Assign { name: String, value: Expr },
    /// An expression evaluated for its side effect, e.g. `print(...)`
    Expr(Expr),
    /// `if <cond> { ... }` with an optional `else { ... }`
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
    /// `while <cond> { ... }`
    While { condition: Expr, body: Vec<Stmt> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    StringLiteral(String),
    IntLiteral(i64),
    BoolLiteral(bool),
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
    Sub,
    Mul,
    Div,
    Eq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
}
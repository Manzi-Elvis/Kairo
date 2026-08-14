//! AST node definitions for Kairo v0.6 milestone scope.
//!
//! Covers struct declarations, struct literals, field access,
//! user-defined functions with typed parameters and an optional
//! return type, return statements, mutable/immutable variable
//! declarations, reassignment, string/int/bool literals,
//! arithmetic/comparison operators, calls with comma-separated
//! arguments, if/else, while loops, and grouping expressions
//! (grouping doesn't need its own node — it just controls parse
//! order).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub structs: Vec<StructDecl>,
    pub enums: Vec<EnumDecl>,
    pub functions: Vec<FunctionDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<Param>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<EnumVariant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumVariant {
    pub name: String,
    /// Empty for a unit variant (`Pending`); populated for a data
    /// variant (`Failed(reason: String)`).
    pub fields: Vec<Param>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDecl {
    pub name: String,
    pub params: Vec<Param>,
    /// Parsed but not statically checked yet — no type-checker pass
    /// exists until a dedicated type-checker slice. `None` means the
    /// function returns Unit.
    pub return_type: Option<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub type_name: String,
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
    /// `return <expr>` or bare `return`
    Return(Option<Expr>),
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
    /// `Name { field: <expr>, ... }`
    StructLiteral {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    /// `EnumName::Variant` or `EnumName::Variant(field: <expr>, ...)`
    EnumLiteral {
        enum_name: String,
        variant: String,
        fields: Vec<(String, Expr)>,
    },
    /// `<expr>.field`
    FieldAccess { object: Box<Expr>, field: String },
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
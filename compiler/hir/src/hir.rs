//! High-level IR: a desugared form of the AST.
//!
//! `match` is fully lowered here into explicit tag tests
//! (`IsVariant`) and field extraction (`VariantField`) — the
//! `Pattern` concept from the AST does not exist at this level.
//!
//! `?` (`Expr::Try`) is NOT desugared here: determining which enum
//! an arbitrary expression evaluates to requires type inference,
//! which this purely syntactic lowering pass does not perform. It
//! passes through unchanged as `HExpr::Try`.
//!
//! MIR (ownership, borrows, moves, drops — spec section 58) is out
//! of scope: it only earns its keep in service of a native-codegen
//! backend, which does not exist yet.

use kairo_ast::{BinaryOp, EnumDecl, Param, StructDecl};

#[derive(Debug, Clone, PartialEq)]
pub struct HProgram {
    pub structs: Vec<StructDecl>,
    pub enums: Vec<EnumDecl>,
    pub functions: Vec<HFunctionDecl>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HFunctionDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<String>,
    pub body: Vec<HStmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HStmt {
    VariableDecl { name: String, value: HExpr, is_mutable: bool },
    Assign { name: String, value: HExpr },
    IndexAssign { name: String, index: HExpr, value: HExpr },
    Expr(HExpr),
    If { condition: HExpr, then_branch: Vec<HStmt>, else_branch: Option<Vec<HStmt>> },
    While { condition: HExpr, body: Vec<HStmt> },
    Return(Option<HExpr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum HExpr {
    StringLiteral(String),
    IntLiteral(i64),
    BoolLiteral(bool),
    Identifier(String),
    Binary { left: Box<HExpr>, op: BinaryOp, right: Box<HExpr> },
    Call { callee: String, args: Vec<HExpr> },
    StructLiteral { name: String, fields: Vec<(String, HExpr)> },
    FieldAccess { object: Box<HExpr>, field: String },
    EnumLiteral { enum_name: String, variant: String, fields: Vec<(String, HExpr)> },
    ArrayLiteral(Vec<HExpr>),
    Index { array: Box<HExpr>, index: Box<HExpr> },
    /// Not desugared — see module docs.
    Try(Box<HExpr>),
    /// Replaces `Pattern::EnumVariant`/literal testing: "does
    /// `scrutinee` currently hold `enum_name::variant`?"
    IsVariant { scrutinee: Box<HExpr>, enum_name: String, variant: String },
    /// Replaces positional pattern binding: extracts one field by
    /// name from an enum value already known to be this variant.
    VariantField { scrutinee: Box<HExpr>, enum_name: String, variant: String, field: String },
}
use crate::span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // Keywords
    Fn,
    If,
    Else,
    While,
    Mut,
    True,
    False,

    // Literals
    Identifier(String),
    StringLiteral(String),
    IntLiteral(i64),

    // Punctuation
    LParen,   // (
    RParen,   // )
    LBrace,   // {
    RBrace,   // }
    ColonEq,  // :=
    Eq,       // =
    Plus,     // +
    Minus,    // -
    Star,     // *
    Slash,    // /
    EqEq,     // ==
    NotEq,    // !=
    Lt,       // 
    Gt,       // >
    LtEq,     // <=
    GtEq,     // >=

    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}
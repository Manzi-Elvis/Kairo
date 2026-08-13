use kairo_ast::{BinaryOp, Expr, FunctionDecl, Program, Stmt};
use kairo_lexer::{Span, Token, TokenKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    UnexpectedToken {
        expected: String,
        found: TokenKind,
        span: Span,
    },
    UnexpectedEof {
        expected: String,
    },
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse_program(mut self) -> Result<Program, ParseError> {
        let mut functions = Vec::new();
        while !self.check(&TokenKind::Eof) {
            functions.push(self.parse_function_decl()?);
        }
        Ok(Program { functions })
    }

    fn parse_function_decl(&mut self) -> Result<FunctionDecl, ParseError> {
        self.expect(&TokenKind::Fn, "fn")?;
        let name = self.expect_identifier()?;
        self.expect(&TokenKind::LParen, "(")?;
        self.expect(&TokenKind::RParen, ")")?;
        self.expect(&TokenKind::LBrace, "{")?;
        let body = self.parse_block_stmts()?;
        self.expect(&TokenKind::RBrace, "}")?;

        Ok(FunctionDecl { name, body })
    }

    /// Parses statements until (but not consuming) the next `}`.
    fn parse_block_stmts(&mut self) -> Result<Vec<Stmt>, ParseError> {
        let mut stmts = Vec::new();
        while !self.check(&TokenKind::RBrace) {
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        if self.check(&TokenKind::If) {
            return self.parse_if_stmt();
        }
        if self.check(&TokenKind::While) {
            return self.parse_while_stmt();
        }

        // Lookahead: `identifier :=` is a variable declaration/assignment.
        if let TokenKind::Identifier(_) = self.peek_kind() {
            if self.peek_at_kind(1) == Some(&TokenKind::ColonEq) {
                let name = self.expect_identifier()?;
                self.expect(&TokenKind::ColonEq, ":=")?;
                let value = self.parse_expr()?;
                return Ok(Stmt::VariableDecl { name, value });
            }
        }

        let expr = self.parse_expr()?;
        Ok(Stmt::Expr(expr))
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.expect(&TokenKind::If, "if")?;
        let condition = self.parse_expr()?;
        self.expect(&TokenKind::LBrace, "{")?;
        let then_branch = self.parse_block_stmts()?;
        self.expect(&TokenKind::RBrace, "}")?;

        let else_branch = if self.check(&TokenKind::Else) {
            self.advance();
            self.expect(&TokenKind::LBrace, "{")?;
            let stmts = self.parse_block_stmts()?;
            self.expect(&TokenKind::RBrace, "}")?;
            Some(stmts)
        } else {
            None
        };

        Ok(Stmt::If {
            condition,
            then_branch,
            else_branch,
        })
    }

    fn parse_while_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.expect(&TokenKind::While, "while")?;
        let condition = self.parse_expr()?;
        self.expect(&TokenKind::LBrace, "{")?;
        let body = self.parse_block_stmts()?;
        self.expect(&TokenKind::RBrace, "}")?;

        Ok(Stmt::While { condition, body })
    }

    // --- expression parsing, lowest to highest precedence ---

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_equality()
    }

    fn parse_equality(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_comparison()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::EqEq => BinaryOp::Eq,
                TokenKind::NotEq => BinaryOp::NotEq,
                _ => break,
            };
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::Binary { left: Box::new(left), op, right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_term()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Lt => BinaryOp::Lt,
                TokenKind::Gt => BinaryOp::Gt,
                TokenKind::LtEq => BinaryOp::Le,
                TokenKind::GtEq => BinaryOp::Ge,
                _ => break,
            };
            self.advance();
            let right = self.parse_term()?;
            left = Expr::Binary { left: Box::new(left), op, right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_factor()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_factor()?;
            left = Expr::Binary { left: Box::new(left), op, right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_primary()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_primary()?;
            left = Expr::Binary { left: Box::new(left), op, right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen, ")")?;
                Ok(inner)
            }
            TokenKind::StringLiteral(value) => {
                self.advance();
                Ok(Expr::StringLiteral(value))
            }
            TokenKind::IntLiteral(value) => {
                self.advance();
                Ok(Expr::IntLiteral(value))
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr::BoolLiteral(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::BoolLiteral(false))
            }
            TokenKind::Identifier(name) => {
                self.advance();
                if self.check(&TokenKind::LParen) {
                    self.advance();
                    let args = self.parse_call_args()?;
                    self.expect(&TokenKind::RParen, ")")?;
                    Ok(Expr::Call { callee: name, args })
                } else {
                    Ok(Expr::Identifier(name))
                }
            }
            other => Err(ParseError::UnexpectedToken {
                expected: "expression".to_string(),
                found: other,
                span: self.peek().span,
            }),
        }
    }

    fn parse_call_args(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if self.check(&TokenKind::RParen) {
            return Ok(args);
        }
        args.push(self.parse_expr()?);
        // No comma token defined yet, so single-arg calls only.
        Ok(args)
    }

    // --- helpers ---

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_at_kind(&self, offset: usize) -> Option<&TokenKind> {
        self.tokens.get(self.pos + offset).map(|t| &t.kind)
    }

    fn check(&self, kind: &TokenKind) -> bool {
        self.peek_kind() == kind
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, kind: &TokenKind, expected_desc: &str) -> Result<Token, ParseError> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            Err(ParseError::UnexpectedToken {
                expected: expected_desc.to_string(),
                found: self.peek_kind().clone(),
                span: self.peek().span,
            })
        }
    }

    fn expect_identifier(&mut self) -> Result<String, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::Identifier(name) => {
                self.advance();
                Ok(name)
            }
            other => Err(ParseError::UnexpectedToken {
                expected: "identifier".to_string(),
                found: other,
                span: self.peek().span,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_lexer::Lexer;

    fn parse(source: &str) -> Result<Program, ParseError> {
        let tokens = Lexer::new(source).tokenize().expect("lex failed");
        Parser::new(tokens).parse_program()
    }

    #[test]
    fn parses_hello_world() {
        let source = r#"
            fn main() {
                name := "World"
                print("Hello, " + name)
            }
        "#;

        let program = parse(source).expect("parse failed");
        let func = &program.functions[0];
        assert_eq!(func.name, "main");

        assert_eq!(
            func.body[0],
            Stmt::VariableDecl {
                name: "name".to_string(),
                value: Expr::StringLiteral("World".to_string()),
            }
        );
    }

    #[test]
    fn parses_empty_function() {
        let program = parse("fn main() {}").unwrap();
        assert_eq!(program.functions[0].body.len(), 0);
    }

    #[test]
    fn reports_missing_closing_brace() {
        let err = parse("fn main() {").unwrap_err();
        match err {
            ParseError::UnexpectedToken { expected, found, .. } => {
                assert_eq!(expected, "expression");
                assert_eq!(found, TokenKind::Eof);
            }
            other => panic!("expected UnexpectedToken, got {other:?}"),
        }
    }

    #[test]
    fn reports_missing_fn_keyword() {
        let err = parse("main() {}").unwrap_err();
        match err {
            ParseError::UnexpectedToken { expected, .. } => assert_eq!(expected, "fn"),
            other => panic!("expected UnexpectedToken, got {other:?}"),
        }
    }

    #[test]
    fn parses_arithmetic_with_correct_precedence() {
        // 2 + 3 * 4 should parse as 2 + (3 * 4), not (2 + 3) * 4
        let program = parse("fn main() { x := 2 + 3 * 4 }").unwrap();
        let Stmt::VariableDecl { value, .. } = &program.functions[0].body[0] else {
            panic!("expected VariableDecl");
        };

        assert_eq!(
            *value,
            Expr::Binary {
                left: Box::new(Expr::IntLiteral(2)),
                op: BinaryOp::Add,
                right: Box::new(Expr::Binary {
                    left: Box::new(Expr::IntLiteral(3)),
                    op: BinaryOp::Mul,
                    right: Box::new(Expr::IntLiteral(4)),
                }),
            }
        );
    }

    #[test]
    fn parses_if_else() {
        let source = r#"
            fn main() {
                if 1 < 2 {
                    print("yes")
                } else {
                    print("no")
                }
            }
        "#;
        let program = parse(source).unwrap();
        let Stmt::If { condition, then_branch, else_branch } = &program.functions[0].body[0] else {
            panic!("expected If statement");
        };

        assert_eq!(
            *condition,
            Expr::Binary {
                left: Box::new(Expr::IntLiteral(1)),
                op: BinaryOp::Lt,
                right: Box::new(Expr::IntLiteral(2)),
            }
        );
        assert_eq!(then_branch.len(), 1);
        assert!(else_branch.is_some());
        assert_eq!(else_branch.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn parses_if_without_else() {
        let program = parse(r#"fn main() { if true { print("hi") } }"#).unwrap();
        let Stmt::If { else_branch, .. } = &program.functions[0].body[0] else {
            panic!("expected If statement");
        };
        assert!(else_branch.is_none());
    }

    #[test]
    fn parses_while_loop() {
        let source = r#"
            fn main() {
                while 1 < 2 {
                    print("looping")
                }
            }
        "#;
        let program = parse(source).unwrap();
        let Stmt::While { condition, body } = &program.functions[0].body[0] else {
            panic!("expected While statement");
        };
        assert_eq!(
            *condition,
            Expr::Binary {
                left: Box::new(Expr::IntLiteral(1)),
                op: BinaryOp::Lt,
                right: Box::new(Expr::IntLiteral(2)),
            }
        );
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn parses_grouping_overrides_precedence() {
        // (2 + 3) * 4 should parse as (2 + 3) * 4, not 2 + (3 * 4)
        let program = parse("fn main() { x := (2 + 3) * 4 }").unwrap();
        let Stmt::VariableDecl { value, .. } = &program.functions[0].body[0] else {
            panic!("expected VariableDecl");
        };

        assert_eq!(
            *value,
            Expr::Binary {
                left: Box::new(Expr::Binary {
                    left: Box::new(Expr::IntLiteral(2)),
                    op: BinaryOp::Add,
                    right: Box::new(Expr::IntLiteral(3)),
                }),
                op: BinaryOp::Mul,
                right: Box::new(Expr::IntLiteral(4)),
            }
        );
    }

    #[test]
    fn reports_unclosed_grouping() {
        let err = parse("fn main() { x := (2 + 3 }").unwrap_err();
        match err {
            ParseError::UnexpectedToken { expected, .. } => assert_eq!(expected, ")"),
            other => panic!("expected UnexpectedToken, got {other:?}"),
        }
    }
}
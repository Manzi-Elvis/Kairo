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

        let mut body = Vec::new();
        while !self.check(&TokenKind::RBrace) {
            body.push(self.parse_stmt()?);
        }
        self.expect(&TokenKind::RBrace, "}")?;

        Ok(FunctionDecl { name, body })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        // Lookahead: `identifier :=` is a variable declaration.
        // Anything else starting with an identifier is an expression statement.
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

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_primary()?;

        while self.check(&TokenKind::Plus) {
            self.advance();
            let right = self.parse_primary()?;
            left = Expr::Binary {
                left: Box::new(left),
                op: BinaryOp::Add,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::StringLiteral(value) => {
                self.advance();
                Ok(Expr::StringLiteral(value))
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
        // No comma token defined yet in v0.1 grammar, so single-arg calls only.
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

        assert_eq!(program.functions.len(), 1);
        let func = &program.functions[0];
        assert_eq!(func.name, "main");
        assert_eq!(func.body.len(), 2);

        assert_eq!(
            func.body[0],
            Stmt::VariableDecl {
                name: "name".to_string(),
                value: Expr::StringLiteral("World".to_string()),
            }
        );

        assert_eq!(
            func.body[1],
            Stmt::Expr(Expr::Call {
                callee: "print".to_string(),
                args: vec![Expr::Binary {
                    left: Box::new(Expr::StringLiteral("Hello, ".to_string())),
                    op: BinaryOp::Add,
                    right: Box::new(Expr::Identifier("name".to_string())),
                }],
            })
        );
    }

    #[test]
    fn parses_empty_function() {
        let program = parse("fn main() {}").unwrap();
        assert_eq!(program.functions.len(), 1);
        assert_eq!(program.functions[0].body.len(), 0);
    }

    #[test]
    fn reports_missing_closing_brace() {
        // With no closing brace, the parser tries to parse another
        // statement and fails on EOF while expecting an expression —
        // that's the correct, honest error for this input.
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
            ParseError::UnexpectedToken { expected, .. } => {
                assert_eq!(expected, "fn");
            }
            other => panic!("expected UnexpectedToken, got {other:?}"),
        }
    }
}
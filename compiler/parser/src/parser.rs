use kairo_ast::{BinaryOp, Expr, FunctionDecl, Param, Program, Stmt, StructDecl};
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
    /// Struct literals (`Name { ... }`) are ambiguous with an if/while
    /// block opening right after a bare identifier condition — Rust
    /// has the same problem and resolves it the same way: disallow
    /// struct literals directly inside if/while conditions, and turn
    /// the allowance back on inside any bracketed context (parens,
    /// call args, struct-literal field values) where it's unambiguous.
    allow_struct_literal: bool,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            allow_struct_literal: true,
        }
    }

    pub fn parse_program(mut self) -> Result<Program, ParseError> {
        let mut structs = Vec::new();
        let mut functions = Vec::new();
        while !self.check(&TokenKind::Eof) {
            if self.check(&TokenKind::Struct) {
                structs.push(self.parse_struct_decl()?);
            } else {
                functions.push(self.parse_function_decl()?);
            }
        }
        Ok(Program { structs, functions })
    }

    fn parse_struct_decl(&mut self) -> Result<StructDecl, ParseError> {
        self.expect(&TokenKind::Struct, "struct")?;
        let name = self.expect_identifier()?;
        self.expect(&TokenKind::LBrace, "{")?;
        let fields = self.parse_struct_fields()?;
        self.expect(&TokenKind::RBrace, "}")?;
        Ok(StructDecl { name, fields })
    }

    fn parse_struct_fields(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut fields = Vec::new();
        if self.check(&TokenKind::RBrace) {
            return Ok(fields);
        }
        fields.push(self.parse_param()?);
        while self.check(&TokenKind::Comma) {
            self.advance();
            fields.push(self.parse_param()?);
        }
        Ok(fields)
    }

    fn parse_function_decl(&mut self) -> Result<FunctionDecl, ParseError> {
        self.expect(&TokenKind::Fn, "fn")?;
        let name = self.expect_identifier()?;
        self.expect(&TokenKind::LParen, "(")?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen, ")")?;

        let return_type = if self.check(&TokenKind::Arrow) {
            self.advance();
            Some(self.expect_identifier()?)
        } else {
            None
        };

        self.expect(&TokenKind::LBrace, "{")?;
        let body = self.parse_block_stmts()?;
        self.expect(&TokenKind::RBrace, "}")?;

        Ok(FunctionDecl {
            name,
            params,
            return_type,
            body,
        })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        if self.check(&TokenKind::RParen) {
            return Ok(params);
        }
        params.push(self.parse_param()?);
        while self.check(&TokenKind::Comma) {
            self.advance();
            params.push(self.parse_param()?);
        }
        Ok(params)
    }

    fn parse_param(&mut self) -> Result<Param, ParseError> {
        let name = self.expect_identifier()?;
        self.expect(&TokenKind::Colon, ":")?;
        let type_name = self.expect_identifier()?;
        Ok(Param { name, type_name })
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
        if self.check(&TokenKind::Mut) {
            return self.parse_mut_decl();
        }
        if self.check(&TokenKind::Return) {
            return self.parse_return_stmt();
        }

        if let TokenKind::Identifier(_) = self.peek_kind() {
            match self.peek_at_kind(1) {
                Some(&TokenKind::ColonEq) => {
                    let name = self.expect_identifier()?;
                    self.expect(&TokenKind::ColonEq, ":=")?;
                    let value = self.parse_expr()?;
                    return Ok(Stmt::VariableDecl {
                        name,
                        value,
                        is_mutable: false,
                    });
                }
                Some(&TokenKind::Eq) => {
                    let name = self.expect_identifier()?;
                    self.expect(&TokenKind::Eq, "=")?;
                    let value = self.parse_expr()?;
                    return Ok(Stmt::Assign { name, value });
                }
                _ => {}
            }
        }

        let expr = self.parse_expr()?;
        Ok(Stmt::Expr(expr))
    }

    fn parse_mut_decl(&mut self) -> Result<Stmt, ParseError> {
        self.expect(&TokenKind::Mut, "mut")?;
        let name = self.expect_identifier()?;
        self.expect(&TokenKind::ColonEq, ":=")?;
        let value = self.parse_expr()?;
        Ok(Stmt::VariableDecl {
            name,
            value,
            is_mutable: true,
        })
    }

    fn parse_return_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.expect(&TokenKind::Return, "return")?;
        // A bare `return` (no expression) is only valid right before
        // the closing brace of its block.
        if self.check(&TokenKind::RBrace) {
            Ok(Stmt::Return(None))
        } else {
            let value = self.parse_expr()?;
            Ok(Stmt::Return(Some(value)))
        }
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, ParseError> {
        self.expect(&TokenKind::If, "if")?;
        let condition = self.parse_condition()?;
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
        let condition = self.parse_condition()?;
        self.expect(&TokenKind::LBrace, "{")?;
        let body = self.parse_block_stmts()?;
        self.expect(&TokenKind::RBrace, "}")?;

        Ok(Stmt::While { condition, body })
    }

    /// Parses an expression with struct literals disallowed at the
    /// top level, for use as an if/while condition. See the
    /// `allow_struct_literal` doc comment on `Parser`.
    fn parse_condition(&mut self) -> Result<Expr, ParseError> {
        let prev = self.allow_struct_literal;
        self.allow_struct_literal = false;
        let condition = self.parse_expr();
        self.allow_struct_literal = prev;
        condition
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
        let mut left = self.parse_postfix()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_postfix()?;
            left = Expr::Binary { left: Box::new(left), op, right: Box::new(right) };
        }
        Ok(left)
    }

    /// Applies postfix field access (`.field`, chainable) on top of
    /// a primary expression: `a.b.c` parses as
    /// `FieldAccess(FieldAccess(a, b), c)`.
    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;
        while self.check(&TokenKind::Dot) {
            self.advance();
            let field = self.expect_identifier()?;
            expr = Expr::FieldAccess { object: Box::new(expr), field };
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::LParen => {
                self.advance();
                let prev = self.allow_struct_literal;
                self.allow_struct_literal = true;
                let inner = self.parse_expr()?;
                self.allow_struct_literal = prev;
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
                } else if self.check(&TokenKind::LBrace) && self.allow_struct_literal {
                    self.parse_struct_literal(name)
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

    fn parse_struct_literal(&mut self, name: String) -> Result<Expr, ParseError> {
        self.expect(&TokenKind::LBrace, "{")?;
        let prev = self.allow_struct_literal;
        self.allow_struct_literal = true;

        let mut fields = Vec::new();
        if !self.check(&TokenKind::RBrace) {
            fields.push(self.parse_struct_literal_field()?);
            while self.check(&TokenKind::Comma) {
                self.advance();
                fields.push(self.parse_struct_literal_field()?);
            }
        }

        self.allow_struct_literal = prev;
        self.expect(&TokenKind::RBrace, "}")?;
        Ok(Expr::StructLiteral { name, fields })
    }

    fn parse_struct_literal_field(&mut self) -> Result<(String, Expr), ParseError> {
        let field_name = self.expect_identifier()?;
        self.expect(&TokenKind::Colon, ":")?;
        let value = self.parse_expr()?;
        Ok((field_name, value))
    }

    fn parse_call_args(&mut self) -> Result<Vec<Expr>, ParseError> {
        let prev = self.allow_struct_literal;
        self.allow_struct_literal = true;

        let mut args = Vec::new();
        if !self.check(&TokenKind::RParen) {
            args.push(self.parse_expr()?);
            while self.check(&TokenKind::Comma) {
                self.advance();
                args.push(self.parse_expr()?);
            }
        }

        self.allow_struct_literal = prev;
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
        assert_eq!(func.params.len(), 0);
        assert_eq!(func.return_type, None);

        assert_eq!(
            func.body[0],
            Stmt::VariableDecl {
                name: "name".to_string(),
                value: Expr::StringLiteral("World".to_string()),
                is_mutable: false,
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

    #[test]
    fn parses_mut_declaration() {
        let program = parse("fn main() { mut x := 5 }").unwrap();
        assert_eq!(
            program.functions[0].body[0],
            Stmt::VariableDecl {
                name: "x".to_string(),
                value: Expr::IntLiteral(5),
                is_mutable: true,
            }
        );
    }

    #[test]
    fn parses_reassignment() {
        let program = parse("fn main() { mut x := 5\nx = 6 }").unwrap();
        assert_eq!(
            program.functions[0].body[1],
            Stmt::Assign {
                name: "x".to_string(),
                value: Expr::IntLiteral(6),
            }
        );
    }

    #[test]
    fn parses_function_with_params_and_return_type() {
        let program = parse("fn add(a: Int, b: Int) -> Int { return a + b }").unwrap();
        let func = &program.functions[0];

        assert_eq!(func.name, "add");
        assert_eq!(
            func.params,
            vec![
                Param { name: "a".to_string(), type_name: "Int".to_string() },
                Param { name: "b".to_string(), type_name: "Int".to_string() },
            ]
        );
        assert_eq!(func.return_type, Some("Int".to_string()));
        assert_eq!(
            func.body[0],
            Stmt::Return(Some(Expr::Binary {
                left: Box::new(Expr::Identifier("a".to_string())),
                op: BinaryOp::Add,
                right: Box::new(Expr::Identifier("b".to_string())),
            }))
        );
    }

    #[test]
    fn parses_function_with_no_params() {
        let program = parse("fn greet() -> Int { return 1 }").unwrap();
        assert_eq!(program.functions[0].params.len(), 0);
    }

    #[test]
    fn parses_bare_return() {
        let program = parse("fn f() { return }").unwrap();
        assert_eq!(program.functions[0].body[0], Stmt::Return(None));
    }

    #[test]
    fn parses_call_with_multiple_args() {
        let program = parse("fn main() { x := add(1, 2) }").unwrap();
        let Stmt::VariableDecl { value, .. } = &program.functions[0].body[0] else {
            panic!("expected VariableDecl");
        };
        assert_eq!(
            *value,
            Expr::Call {
                callee: "add".to_string(),
                args: vec![Expr::IntLiteral(1), Expr::IntLiteral(2)],
            }
        );
    }

    #[test]
    fn parses_struct_decl() {
        let program = parse("struct Point { x: Int, y: Int }\nfn main() {}").unwrap();
        assert_eq!(program.structs.len(), 1);
        assert_eq!(program.structs[0].name, "Point");
        assert_eq!(
            program.structs[0].fields,
            vec![
                Param { name: "x".to_string(), type_name: "Int".to_string() },
                Param { name: "y".to_string(), type_name: "Int".to_string() },
            ]
        );
    }

    #[test]
    fn parses_struct_literal() {
        let program = parse("fn main() { p := Point { x: 1, y: 2 } }").unwrap();
        let Stmt::VariableDecl { value, .. } = &program.functions[0].body[0] else {
            panic!("expected VariableDecl");
        };
        assert_eq!(
            *value,
            Expr::StructLiteral {
                name: "Point".to_string(),
                fields: vec![
                    ("x".to_string(), Expr::IntLiteral(1)),
                    ("y".to_string(), Expr::IntLiteral(2)),
                ],
            }
        );
    }

    #[test]
    fn parses_field_access() {
        let program = parse("fn main() { print(p.x) }").unwrap();
        let Stmt::Expr(Expr::Call { args, .. }) = &program.functions[0].body[0] else {
            panic!("expected call expression statement");
        };
        assert_eq!(
            args[0],
            Expr::FieldAccess {
                object: Box::new(Expr::Identifier("p".to_string())),
                field: "x".to_string(),
            }
        );
    }

    #[test]
    fn parses_chained_field_access() {
        let program = parse("fn main() { print(a.b.c) }").unwrap();
        let Stmt::Expr(Expr::Call { args, .. }) = &program.functions[0].body[0] else {
            panic!("expected call expression statement");
        };
        assert_eq!(
            args[0],
            Expr::FieldAccess {
                object: Box::new(Expr::FieldAccess {
                    object: Box::new(Expr::Identifier("a".to_string())),
                    field: "b".to_string(),
                }),
                field: "c".to_string(),
            }
        );
    }

    #[test]
    fn if_condition_does_not_misparse_as_struct_literal() {
        // `if x { ... }`: the `{` must open the if-block, not a
        // struct literal `x { ... }`.
        let program = parse(r#"fn main() { if x { print("hi") } }"#).unwrap();
        let Stmt::If { condition, then_branch, .. } = &program.functions[0].body[0] else {
            panic!("expected If statement");
        };
        assert_eq!(*condition, Expr::Identifier("x".to_string()));
        assert_eq!(then_branch.len(), 1);
    }

    #[test]
    fn struct_literal_allowed_inside_call_args_within_if_condition() {
        // Inside f(...), we're in an unambiguous bracketed context
        // even though we're syntactically inside an if condition.
        let program =
            parse(r#"fn main() { if f(Point { x: 1, y: 2 }) { print("hi") } }"#).unwrap();
        let Stmt::If { condition, .. } = &program.functions[0].body[0] else {
            panic!("expected If statement");
        };
        assert_eq!(
            *condition,
            Expr::Call {
                callee: "f".to_string(),
                args: vec![Expr::StructLiteral {
                    name: "Point".to_string(),
                    fields: vec![
                        ("x".to_string(), Expr::IntLiteral(1)),
                        ("y".to_string(), Expr::IntLiteral(2)),
                    ],
                }],
            }
        );
    }
}
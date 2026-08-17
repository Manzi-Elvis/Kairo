use crate::span::Span;
use crate::token::{Token, TokenKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    UnterminatedString { line: usize, column: usize },
    UnrecognizedChar { ch: char, line: usize, column: usize },
}

pub struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    column: usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace_and_comments();

            let start_pos = self.pos;
            let start_line = self.line;
            let start_col = self.column;

            let Some(ch) = self.peek() else {
                tokens.push(Token::new(
                    TokenKind::Eof,
                    Span::new(start_pos, start_pos, start_line, start_col),
                ));
                break;
            };

            let kind = if ch.is_alphabetic() || ch == '_' {
                self.lex_identifier_or_keyword()
            } else if ch.is_ascii_digit() {
                self.lex_number()
            } else if ch == '"' {
                self.lex_string()?
            } else {
                self.lex_punctuation()?
            };

            let end_pos = self.pos;
            tokens.push(Token::new(
                kind,
                Span::new(start_pos, end_pos, start_line, start_col),
            ));
        }
        Ok(tokens)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.advance();
                }
                Some('/') if self.peek_at(1) == Some('/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    fn lex_identifier_or_keyword(&mut self) -> TokenKind {
        let mut ident = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                ident.push(c);
                self.advance();
            } else {
                break;
            }
        }
        match ident.as_str() {
            "fn" => TokenKind::Fn,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "mut" => TokenKind::Mut,
            "return" => TokenKind::Return,
            "struct" => TokenKind::Struct,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "enum" => TokenKind::Enum,
            "match" => TokenKind::Match,
            _ => TokenKind::Identifier(ident),
            
        }
    }

    fn lex_number(&mut self) -> TokenKind {
        let mut digits = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                digits.push(c);
                self.advance();
            } else {
                break;
            }
        }
        // Safe: we only ever pushed ASCII digits, so this always parses.
        TokenKind::IntLiteral(digits.parse().unwrap())
    }

    fn lex_string(&mut self) -> Result<TokenKind, LexError> {
        let start_line = self.line;
        let start_col = self.column;
        self.advance(); // consume opening quote

        let mut value = String::new();
        loop {
            match self.peek() {
                None => {
                    return Err(LexError::UnterminatedString {
                        line: start_line,
                        column: start_col,
                    })
                }
                Some('"') => {
                    self.advance();
                    break;
                }
                Some(c) => {
                    value.push(c);
                    self.advance();
                }
            }
        }
        Ok(TokenKind::StringLiteral(value))
    }

    fn lex_punctuation(&mut self) -> Result<TokenKind, LexError> {
        let line = self.line;
        let column = self.column;
        let ch = self.advance().unwrap();

        match ch {
            '(' => Ok(TokenKind::LParen),
            ')' => Ok(TokenKind::RParen),
            '{' => Ok(TokenKind::LBrace),
            '}' => Ok(TokenKind::RBrace),
            ',' => Ok(TokenKind::Comma),
            '.' => Ok(TokenKind::Dot),
            '+' => Ok(TokenKind::Plus),
            '-' if self.peek() == Some('>') => {
                self.advance();
                Ok(TokenKind::Arrow)
            }
            '-' => Ok(TokenKind::Minus),
            '*' => Ok(TokenKind::Star),
            '/' => Ok(TokenKind::Slash),
            '<' if self.peek() == Some('=') => {
                self.advance();
                Ok(TokenKind::LtEq)
            }
            '<' => Ok(TokenKind::Lt),
            '>' if self.peek() == Some('=') => {
                self.advance();
                Ok(TokenKind::GtEq)
            }
            '>' => Ok(TokenKind::Gt),
            '=' if self.peek() == Some('>') => {
                self.advance();
                Ok(TokenKind::FatArrow)
            }
            '=' if self.peek() == Some('=') => {
                self.advance();
                Ok(TokenKind::EqEq)
            }
            '=' => Ok(TokenKind::Eq),
            '!' if self.peek() == Some('=') => {
                self.advance();
                Ok(TokenKind::NotEq)
            }
            ':' if self.peek() == Some('=') => {
                self.advance();
                Ok(TokenKind::ColonEq)
            }
            ':' if self.peek() == Some(':') => {
                self.advance();
                Ok(TokenKind::ColonColon)
            }
            ':' => Ok(TokenKind::Colon),
            '[' => Ok(TokenKind::LBracket),
            ']' => Ok(TokenKind::RBracket),
            other => Err(LexError::UnrecognizedChar {
                ch: other,
                line,
                column,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        Lexer::new(source)
            .tokenize()
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn tokenizes_hello_world() {
        let source = r#"
            fn main() {
                name := "World"
                print("Hello, " + name)
            }
        "#;

        let kinds = kinds(source);

        assert_eq!(
            kinds,
            vec![
                TokenKind::Fn,
                TokenKind::Identifier("main".into()),
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::LBrace,
                TokenKind::Identifier("name".into()),
                TokenKind::ColonEq,
                TokenKind::StringLiteral("World".into()),
                TokenKind::Identifier("print".into()),
                TokenKind::LParen,
                TokenKind::StringLiteral("Hello, ".into()),
                TokenKind::Plus,
                TokenKind::Identifier("name".into()),
                TokenKind::RParen,
                TokenKind::RBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn skips_line_comments() {
        let kinds = kinds("fn // a comment\nmain");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Fn,
                TokenKind::Identifier("main".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn reports_unterminated_string() {
        let err = Lexer::new(r#""unterminated"#).tokenize().unwrap_err();
        assert_eq!(err, LexError::UnterminatedString { line: 1, column: 1 });
    }

    #[test]
    fn reports_unrecognized_char() {
        let err = Lexer::new("fn main() { @ }").tokenize().unwrap_err();
        assert_eq!(
            err,
            LexError::UnrecognizedChar {
                ch: '@',
                line: 1,
                column: 13
            }
        );
    }

    #[test]
    fn tracks_spans_correctly() {
        let tokens = Lexer::new("fn").tokenize().unwrap();
        assert_eq!(tokens[0].span.start, 0);
        assert_eq!(tokens[0].span.end, 2);
        assert_eq!(tokens[0].span.line, 1);
        assert_eq!(tokens[0].span.column, 1);
    }

    #[test]
    fn tokenizes_numbers_and_new_operators() {
        let kinds = kinds("5 - 2 * 3 / 1 == 4 != 5 < 6 > 1 <= 2 >= 3 if else true false");
        assert_eq!(
            kinds,
            vec![
                TokenKind::IntLiteral(5),
                TokenKind::Minus,
                TokenKind::IntLiteral(2),
                TokenKind::Star,
                TokenKind::IntLiteral(3),
                TokenKind::Slash,
                TokenKind::IntLiteral(1),
                TokenKind::EqEq,
                TokenKind::IntLiteral(4),
                TokenKind::NotEq,
                TokenKind::IntLiteral(5),
                TokenKind::Lt,
                TokenKind::IntLiteral(6),
                TokenKind::Gt,
                TokenKind::IntLiteral(1),
                TokenKind::LtEq,
                TokenKind::IntLiteral(2),
                TokenKind::GtEq,
                TokenKind::IntLiteral(3),
                TokenKind::If,
                TokenKind::Else,
                TokenKind::True,
                TokenKind::False,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn tokenizes_while_keyword() {
        let kinds = kinds("while true {}");
        assert_eq!(
            kinds,
            vec![
                TokenKind::While,
                TokenKind::True,
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn tokenizes_mut_and_assignment() {
        let kinds = kinds("mut x := 1\nx = 2");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Mut,
                TokenKind::Identifier("x".into()),
                TokenKind::ColonEq,
                TokenKind::IntLiteral(1),
                TokenKind::Identifier("x".into()),
                TokenKind::Eq,
                TokenKind::IntLiteral(2),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn tokenizes_function_signature() {
        let kinds = kinds("fn add(a: Int, b: Int) -> Int { return a }");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Fn,
                TokenKind::Identifier("add".into()),
                TokenKind::LParen,
                TokenKind::Identifier("a".into()),
                TokenKind::Colon,
                TokenKind::Identifier("Int".into()),
                TokenKind::Comma,
                TokenKind::Identifier("b".into()),
                TokenKind::Colon,
                TokenKind::Identifier("Int".into()),
                TokenKind::RParen,
                TokenKind::Arrow,
                TokenKind::Identifier("Int".into()),
                TokenKind::LBrace,
                TokenKind::Return,
                TokenKind::Identifier("a".into()),
                TokenKind::RBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn tokenizes_struct_and_field_access() {
        let kinds = kinds("struct Point { x: Int } p.x");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Struct,
                TokenKind::Identifier("Point".into()),
                TokenKind::LBrace,
                TokenKind::Identifier("x".into()),
                TokenKind::Colon,
                TokenKind::Identifier("Int".into()),
                TokenKind::RBrace,
                TokenKind::Identifier("p".into()),
                TokenKind::Dot,
                TokenKind::Identifier("x".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn tokenizes_enum_and_double_colon() {
        let kinds = kinds("enum Status { Pending } Status::Pending");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Enum,
                TokenKind::Identifier("Status".into()),
                TokenKind::LBrace,
                TokenKind::Identifier("Pending".into()),
                TokenKind::RBrace,
                TokenKind::Identifier("Status".into()),
                TokenKind::ColonColon,
                TokenKind::Identifier("Pending".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn tokenizes_match_and_fat_arrow() {
        let kinds = kinds("match x { _ => {} }");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Match,
                TokenKind::Identifier("x".into()),
                TokenKind::LBrace,
                TokenKind::Identifier("_".into()),
                TokenKind::FatArrow,
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::RBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn tokenizes_brackets() {
        let kinds = kinds("[1, 2]");
        assert_eq!(
            kinds,
            vec![
                TokenKind::LBracket,
                TokenKind::IntLiteral(1),
                TokenKind::Comma,
                TokenKind::IntLiteral(2),
                TokenKind::RBracket,
                TokenKind::Eof,
            ]
        );
    }
}
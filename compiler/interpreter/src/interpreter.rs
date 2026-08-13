use kairo_ast::{BinaryOp, Expr, FunctionDecl, Program, Stmt};
use std::collections::HashMap;

use crate::value::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    UndefinedVariable(String),
    UndefinedFunction(String),
    WrongArgCount { callee: String, expected: usize, found: usize },
    NoMainFunction,
}

pub struct Interpreter<'a> {
    env: HashMap<String, Value>,
    print_sink: &'a mut dyn FnMut(&str),
}

impl<'a> Interpreter<'a> {
    pub fn new(print_sink: &'a mut dyn FnMut(&str)) -> Self {
        Self {
            env: HashMap::new(),
            print_sink,
        }
    }

    /// Runs the program's `main` function. Fails if there isn't one,
    /// matching a real language's entry-point requirement.
    pub fn run(&mut self, program: &Program) -> Result<(), RuntimeError> {
        let main_fn = program
            .functions
            .iter()
            .find(|f| f.name == "main")
            .ok_or(RuntimeError::NoMainFunction)?;

        self.run_function(main_fn)
    }

    fn run_function(&mut self, func: &FunctionDecl) -> Result<(), RuntimeError> {
        for stmt in &func.body {
            self.exec_stmt(stmt)?;
        }
        Ok(())
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<(), RuntimeError> {
        match stmt {
            Stmt::VariableDecl { name, value } => {
                let v = self.eval_expr(value)?;
                self.env.insert(name.clone(), v);
                Ok(())
            }
            Stmt::Expr(expr) => {
                self.eval_expr(expr)?;
                Ok(())
            }
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, RuntimeError> {
        match expr {
            Expr::StringLiteral(s) => Ok(Value::String(s.clone())),

            Expr::Identifier(name) => self
                .env
                .get(name)
                .cloned()
                .ok_or_else(|| RuntimeError::UndefinedVariable(name.clone())),

            Expr::Binary { left, op, right } => {
                let l = self.eval_expr(left)?;
                let r = self.eval_expr(right)?;
                match op {
                    BinaryOp::Add => {
                        let Value::String(ls) = l;
                        let Value::String(rs) = r;
                        Ok(Value::String(ls + &rs))
                    }
                }
            }

            Expr::Call { callee, args } => self.call_function(callee, args),
        }
    }

    fn call_function(&mut self, callee: &str, args: &[Expr]) -> Result<Value, RuntimeError> {
        match callee {
            "print" => {
                if args.len() != 1 {
                    return Err(RuntimeError::WrongArgCount {
                        callee: "print".to_string(),
                        expected: 1,
                        found: args.len(),
                    });
                }
                let value = self.eval_expr(&args[0])?;
                (self.print_sink)(&value.display());
                Ok(Value::String(String::new()))
            }
            other => Err(RuntimeError::UndefinedFunction(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_lexer::Lexer;
    use kairo_parser::Parser;

    fn run_and_capture(source: &str) -> Result<Vec<String>, RuntimeError> {
        let tokens = Lexer::new(source).tokenize().expect("lex failed");
        let program = Parser::new(tokens).parse_program().expect("parse failed");

        let mut output = Vec::new();
        let mut sink = |s: &str| output.push(s.to_string());
        let mut interp = Interpreter::new(&mut sink);
        interp.run(&program)?;

        Ok(output)
    }

    #[test]
    fn runs_hello_world() {
        let source = r#"
            fn main() {
                name := "World"
                print("Hello, " + name)
            }
        "#;

        let output = run_and_capture(source).expect("run failed");
        assert_eq!(output, vec!["Hello, World".to_string()]);
    }

    #[test]
    fn reports_undefined_variable() {
        let source = r#"
            fn main() {
                print(missing)
            }
        "#;

        let err = run_and_capture(source).unwrap_err();
        assert_eq!(err, RuntimeError::UndefinedVariable("missing".to_string()));
    }

    #[test]
    fn reports_undefined_function() {
        let source = r#"
            fn main() {
                doesNotExist("hi")
            }
        "#;

        let err = run_and_capture(source).unwrap_err();
        assert_eq!(err, RuntimeError::UndefinedFunction("doesNotExist".to_string()));
    }

    #[test]
    fn reports_missing_main() {
        let source = r#"
            fn notMain() {}
        "#;

        let err = run_and_capture(source).unwrap_err();
        assert_eq!(err, RuntimeError::NoMainFunction);
    }
}
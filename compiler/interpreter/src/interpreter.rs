use kairo_ast::{BinaryOp, Expr, FunctionDecl, Program, Stmt};
use std::collections::HashMap;

use crate::value::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeError {
    UndefinedVariable(String),
    UndefinedFunction(String),
    WrongArgCount { callee: String, expected: usize, found: usize },
    NoMainFunction,
    TypeError(String),
    DivisionByZero,
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

    pub fn run(&mut self, program: &Program) -> Result<(), RuntimeError> {
        let main_fn = program
            .functions
            .iter()
            .find(|f| f.name == "main")
            .ok_or(RuntimeError::NoMainFunction)?;

        self.run_function(main_fn)
    }

    fn run_function(&mut self, func: &FunctionDecl) -> Result<(), RuntimeError> {
        self.exec_block(&func.body)
    }

    fn exec_block(&mut self, stmts: &[Stmt]) -> Result<(), RuntimeError> {
        for stmt in stmts {
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
            Stmt::If { condition, then_branch, else_branch } => {
                if self.eval_condition(condition)? {
                    self.exec_block(then_branch)
                } else if let Some(else_stmts) = else_branch {
                    self.exec_block(else_stmts)
                } else {
                    Ok(())
                }
            }
            Stmt::While { condition, body } => {
                while self.eval_condition(condition)? {
                    self.exec_block(body)?;
                }
                Ok(())
            }
        }
    }

    /// Evaluates an expression expected to produce a Bool, erroring
    /// with a clear message otherwise (used by if and while).
    fn eval_condition(&mut self, condition: &Expr) -> Result<bool, RuntimeError> {
        match self.eval_expr(condition)? {
            Value::Bool(b) => Ok(b),
            other => Err(RuntimeError::TypeError(format!(
                "condition must be Bool, found {}",
                other.type_name()
            ))),
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, RuntimeError> {
        match expr {
            Expr::StringLiteral(s) => Ok(Value::String(s.clone())),
            Expr::IntLiteral(i) => Ok(Value::Int(*i)),
            Expr::BoolLiteral(b) => Ok(Value::Bool(*b)),

            Expr::Identifier(name) => self
                .env
                .get(name)
                .cloned()
                .ok_or_else(|| RuntimeError::UndefinedVariable(name.clone())),

            Expr::Binary { left, op, right } => {
                let l = self.eval_expr(left)?;
                let r = self.eval_expr(right)?;
                self.eval_binary(*op, l, r)
            }

            Expr::Call { callee, args } => self.call_function(callee, args),
        }
    }

    fn eval_binary(&self, op: BinaryOp, l: Value, r: Value) -> Result<Value, RuntimeError> {
        use BinaryOp::*;

        match op {
            Add => match (l, r) {
                (Value::String(a), Value::String(b)) => Ok(Value::String(a + &b)),
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
                (a, b) => Err(type_error("+", &a, &b)),
            },
            Sub => int_op(l, r, "-", |a, b| Ok(Value::Int(a - b))),
            Mul => int_op(l, r, "*", |a, b| Ok(Value::Int(a * b))),
            Div => int_op(l, r, "/", |a, b| {
                if b == 0 {
                    Err(RuntimeError::DivisionByZero)
                } else {
                    Ok(Value::Int(a / b))
                }
            }),
            Eq => Ok(Value::Bool(l == r)),
            NotEq => Ok(Value::Bool(l != r)),
            Lt => int_op(l, r, "<", |a, b| Ok(Value::Bool(a < b))),
            Gt => int_op(l, r, ">", |a, b| Ok(Value::Bool(a > b))),
            Le => int_op(l, r, "<=", |a, b| Ok(Value::Bool(a <= b))),
            Ge => int_op(l, r, ">=", |a, b| Ok(Value::Bool(a >= b))),
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

fn type_error(op: &str, a: &Value, b: &Value) -> RuntimeError {
    RuntimeError::TypeError(format!(
        "cannot apply `{}` to {} and {}",
        op,
        a.type_name(),
        b.type_name()
    ))
}

/// Helper for binary ops that require both operands to be Int.
fn int_op(
    l: Value,
    r: Value,
    op: &str,
    f: impl FnOnce(i64, i64) -> Result<Value, RuntimeError>,
) -> Result<Value, RuntimeError> {
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => f(a, b),
        (a, b) => Err(type_error(op, &a, &b)),
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
        assert_eq!(run_and_capture(source).unwrap(), vec!["Hello, World".to_string()]);
    }

    #[test]
    fn runs_arithmetic() {
        let source = r#"
            fn main() {
                x := 2 + 3 * 4
                print(x)
            }
        "#;
        assert_eq!(run_and_capture(source).unwrap(), vec!["14".to_string()]);
    }

    #[test]
    fn runs_grouping_overrides_precedence() {
        let source = r#"
            fn main() {
                x := (2 + 3) * 4
                print(x)
            }
        "#;
        assert_eq!(run_and_capture(source).unwrap(), vec!["20".to_string()]);
    }

    #[test]
    fn runs_if_true_branch() {
        let source = r#"
            fn main() {
                if 1 < 2 {
                    print("yes")
                } else {
                    print("no")
                }
            }
        "#;
        assert_eq!(run_and_capture(source).unwrap(), vec!["yes".to_string()]);
    }

    #[test]
    fn runs_if_false_branch() {
        let source = r#"
            fn main() {
                if 5 == 6 {
                    print("yes")
                } else {
                    print("no")
                }
            }
        "#;
        assert_eq!(run_and_capture(source).unwrap(), vec!["no".to_string()]);
    }

    #[test]
    fn runs_if_with_no_else_and_false_condition() {
        let source = r#"
            fn main() {
                if false {
                    print("unreachable")
                }
                print("after")
            }
        "#;
        assert_eq!(run_and_capture(source).unwrap(), vec!["after".to_string()]);
    }

    #[test]
    fn runs_while_loop() {
        let source = r#"
            fn main() {
                counter := 0
                while counter < 3 {
                    print(counter)
                    counter := counter + 1
                }
            }
        "#;
        assert_eq!(
            run_and_capture(source).unwrap(),
            vec!["0".to_string(), "1".to_string(), "2".to_string()]
        );
    }

    #[test]
    fn while_loop_never_runs_if_condition_starts_false() {
        let source = r#"
            fn main() {
                while false {
                    print("nope")
                }
                print("done")
            }
        "#;
        assert_eq!(run_and_capture(source).unwrap(), vec!["done".to_string()]);
    }

    #[test]
    fn reports_division_by_zero() {
        let source = r#"
            fn main() {
                x := 5 / 0
            }
        "#;
        assert_eq!(run_and_capture(source).unwrap_err(), RuntimeError::DivisionByZero);
    }

    #[test]
    fn reports_type_error_on_bad_addition() {
        let source = r#"
            fn main() {
                x := 5 + true
            }
        "#;
        match run_and_capture(source).unwrap_err() {
            RuntimeError::TypeError(_) => {}
            other => panic!("expected TypeError, got {other:?}"),
        }
    }

    #[test]
    fn reports_type_error_on_non_bool_condition() {
        let source = r#"
            fn main() {
                if 5 {
                    print("no")
                }
            }
        "#;
        match run_and_capture(source).unwrap_err() {
            RuntimeError::TypeError(_) => {}
            other => panic!("expected TypeError, got {other:?}"),
        }
    }

    #[test]
    fn reports_undefined_variable() {
        let err = run_and_capture(r#"fn main() { print(missing) }"#).unwrap_err();
        assert_eq!(err, RuntimeError::UndefinedVariable("missing".to_string()));
    }

    #[test]
    fn reports_missing_main() {
        let err = run_and_capture(r#"fn notMain() {}"#).unwrap_err();
        assert_eq!(err, RuntimeError::NoMainFunction);
    }
}
use kairo_ast::{BinaryOp, EnumDecl, StructDecl};
use kairo_hir::{HExpr, HFunctionDecl, HProgram, HStmt};
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
    AlreadyDeclared(String),
    ImmutableAssignment(String),
    StructError(String),
    EnumError(String),
    IndexOutOfBounds(i64),
    /// Not a real user-facing error — used internally to unwind to
    /// the nearest function-call boundary when `?` hits an Err.
    TryPropagate(Value),
    NotResultShaped(String),
}

/// A variable binding: its current value plus whether `=` may update it.
struct Binding {
    value: Value,
    is_mutable: bool,
}

/// Signals whether a block finished normally or hit a `return`.
enum ControlFlow {
    Normal,
    Return(Value),
}

pub struct Interpreter<'a> {
    env: HashMap<String, Binding>,
    functions: HashMap<String, HFunctionDecl>,
    structs: HashMap<String, StructDecl>,
    enums: HashMap<String, EnumDecl>,
    print_sink: &'a mut dyn FnMut(&str),
}

impl<'a> Interpreter<'a> {
    pub fn new(print_sink: &'a mut dyn FnMut(&str)) -> Self {
        Self {
            env: HashMap::new(),
            functions: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            print_sink,
        }
    }

    pub fn run(&mut self, program: &HProgram) -> Result<(), RuntimeError> {
        for s in &program.structs {
            self.structs.insert(s.name.clone(), s.clone());
        }
        for e in &program.enums {
            self.enums.insert(e.name.clone(), e.clone());
        }
        for func in &program.functions {
            self.functions.insert(func.name.clone(), func.clone());
        }

        let main_fn = self
            .functions
            .get("main")
            .cloned()
            .ok_or(RuntimeError::NoMainFunction)?;

        self.exec_block(&main_fn.body)?;
        Ok(())
    }

    fn exec_block(&mut self, stmts: &[HStmt]) -> Result<ControlFlow, RuntimeError> {
        for stmt in stmts {
            match self.exec_stmt(stmt) {
                Err(RuntimeError::TryPropagate(err_value)) => {
                    return Ok(ControlFlow::Return(err_value));
                }
                other => {
                    let flow = other?;
                    if let ControlFlow::Return(_) = flow {
                        return Ok(flow);
                    }
                }
            }
        }
        Ok(ControlFlow::Normal)
    }

    fn exec_stmt(&mut self, stmt: &HStmt) -> Result<ControlFlow, RuntimeError> {
        match stmt {
            HStmt::VariableDecl { name, value, is_mutable } => {
                if self.env.contains_key(name) {
                    return Err(RuntimeError::AlreadyDeclared(name.clone()));
                }
                let v = self.eval_expr(value)?;
                self.env.insert(
                    name.clone(),
                    Binding { value: v, is_mutable: *is_mutable },
                );
                Ok(ControlFlow::Normal)
            }
            HStmt::Assign { name, value } => {
                let v = self.eval_expr(value)?;
                match self.env.get_mut(name) {
                    None => Err(RuntimeError::UndefinedVariable(name.clone())),
                    Some(binding) if !binding.is_mutable => {
                        Err(RuntimeError::ImmutableAssignment(name.clone()))
                    }
                    Some(binding) => {
                        binding.value = v;
                        Ok(ControlFlow::Normal)
                    }
                }
            }
            HStmt::IndexAssign { name, index, value } => {
                let idx_val = self.eval_expr(index)?;
                let Value::Int(i) = idx_val else {
                    return Err(RuntimeError::TypeError(format!(
                        "array index must be Int, found {}", idx_val.type_name()
                    )));
                };
                let new_val = self.eval_expr(value)?;
                match self.env.get_mut(name) {
                    None => Err(RuntimeError::UndefinedVariable(name.clone())),
                    Some(binding) if !binding.is_mutable => {
                        Err(RuntimeError::ImmutableAssignment(name.clone()))
                    }
                    Some(binding) => {
                        let Value::Array(items) = &mut binding.value else {
                            return Err(RuntimeError::TypeError(
                                "cannot index-assign into a non-array".to_string(),
                            ));
                        };
                        if i < 0 || i as usize >= items.len() {
                            return Err(RuntimeError::IndexOutOfBounds(i));
                        }
                        items[i as usize] = new_val;
                        Ok(ControlFlow::Normal)
                    }
                }
            }
            HStmt::Expr(expr) => {
                self.eval_expr(expr)?;
                Ok(ControlFlow::Normal)
            }
            HStmt::If { condition, then_branch, else_branch } => {
                if self.eval_condition(condition)? {
                    self.exec_block(then_branch)
                } else if let Some(else_stmts) = else_branch {
                    self.exec_block(else_stmts)
                } else {
                    Ok(ControlFlow::Normal)
                }
            }
            HStmt::While { condition, body } => {
                while self.eval_condition(condition)? {
                    let flow = self.exec_block(body)?;
                    if let ControlFlow::Return(_) = flow {
                        return Ok(flow);
                    }
                }
                Ok(ControlFlow::Normal)
            }
            HStmt::Return(expr) => {
                let value = match expr {
                    Some(e) => self.eval_expr(e)?,
                    None => Value::Unit,
                };
                Ok(ControlFlow::Return(value))
            }
        }
    }

    fn eval_condition(&mut self, condition: &HExpr) -> Result<bool, RuntimeError> {
        match self.eval_expr(condition)? {
            Value::Bool(b) => Ok(b),
            other => Err(RuntimeError::TypeError(format!(
                "condition must be Bool, found {}",
                other.type_name()
            ))),
        }
    }

    fn eval_expr(&mut self, expr: &HExpr) -> Result<Value, RuntimeError> {
        match expr {
            HExpr::StringLiteral(s) => Ok(Value::String(s.clone())),
            HExpr::IntLiteral(i) => Ok(Value::Int(*i)),
            HExpr::BoolLiteral(b) => Ok(Value::Bool(*b)),

            HExpr::Identifier(name) => self
                .env
                .get(name)
                .map(|binding| binding.value.clone())
                .ok_or_else(|| RuntimeError::UndefinedVariable(name.clone())),

            HExpr::Binary { left, op, right } => {
                let l = self.eval_expr(left)?;
                let r = self.eval_expr(right)?;
                self.eval_binary(*op, l, r)
            }

            HExpr::Call { callee, args } => self.call_function(callee, args),

            HExpr::StructLiteral { name, fields } => self.eval_struct_literal(name, fields),

            HExpr::FieldAccess { object, field } => match self.eval_expr(object)? {
                Value::Struct { name, fields } => fields.get(field).cloned().ok_or_else(|| {
                    RuntimeError::StructError(format!(
                        "struct `{}` has no field `{}`", name, field
                    ))
                }),
                other => Err(RuntimeError::TypeError(format!(
                    "cannot access field `{}` on {}", field, other.type_name()
                ))),
            },

            HExpr::EnumLiteral { enum_name, variant, fields } => {
                self.eval_enum_literal(enum_name, variant, fields)
            }

            HExpr::ArrayLiteral(elements) => {
                let mut values = Vec::with_capacity(elements.len());
                for e in elements {
                    values.push(self.eval_expr(e)?);
                }
                Ok(Value::Array(values))
            }

            HExpr::Index { array, index } => {
                let idx_val = self.eval_expr(index)?;
                let Value::Int(i) = idx_val else {
                    return Err(RuntimeError::TypeError(format!(
                        "array index must be Int, found {}", idx_val.type_name()
                    )));
                };
                let arr_val = self.eval_expr(array)?;
                let Value::Array(items) = arr_val else {
                    return Err(RuntimeError::TypeError(format!(
                        "cannot index into {}", arr_val.type_name()
                    )));
                };
                if i < 0 || i as usize >= items.len() {
                    return Err(RuntimeError::IndexOutOfBounds(i));
                }
                Ok(items[i as usize].clone())
            }

            HExpr::Try(inner) => {
                let value = self.eval_expr(inner)?;
                let Value::Enum { enum_name, variant, mut fields } = value else {
                    return Err(RuntimeError::NotResultShaped(
                        "? used on a non-enum value".to_string(),
                    ));
                };
                match variant.as_str() {
                    "Ok" => fields.remove("value").ok_or_else(|| {
                        RuntimeError::NotResultShaped(format!(
                            "enum `{}` variant `Ok` has no `value` field", enum_name
                        ))
                    }),
                    "Err" => Err(RuntimeError::TryPropagate(Value::Enum {
                        enum_name, variant, fields,
                    })),
                    other => Err(RuntimeError::NotResultShaped(format!(
                        "`?` requires an Ok/Err-shaped enum, found variant `{}`", other
                    ))),
                }
            }

            HExpr::IsVariant { scrutinee, enum_name, variant } => {
                match self.eval_expr(scrutinee)? {
                    Value::Enum { enum_name: vn, variant: vv, .. } => {
                        Ok(Value::Bool(&vn == enum_name && &vv == variant))
                    }
                    _ => Ok(Value::Bool(false)),
                }
            }

            HExpr::VariantField { scrutinee, enum_name, variant, field } => {
                match self.eval_expr(scrutinee)? {
                    Value::Enum { fields, .. } => fields.get(field).cloned().ok_or_else(|| {
                        RuntimeError::EnumError(format!(
                            "variant `{}::{}` has no field `{}`", enum_name, variant, field
                        ))
                    }),
                    other => Err(RuntimeError::TypeError(format!(
                        "expected enum `{}`, found {}", enum_name, other.type_name()
                    ))),
                }
            }
        }
    }

    fn eval_struct_literal(
        &mut self,
        name: &str,
        fields: &[(String, HExpr)],
    ) -> Result<Value, RuntimeError> {
        let Some(decl) = self.structs.get(name).cloned() else {
            return Err(RuntimeError::StructError(format!("undefined struct `{}`", name)));
        };
        if fields.len() != decl.fields.len() {
            return Err(RuntimeError::StructError(format!(
                "struct `{}` expects {} field(s), found {}",
                name, decl.fields.len(), fields.len()
            )));
        }
        let mut values: HashMap<String, Value> = HashMap::new();
        for (field_name, field_expr) in fields {
            if !decl.fields.iter().any(|f| &f.name == field_name) {
                return Err(RuntimeError::StructError(format!(
                    "struct `{}` has no field `{}`", name, field_name
                )));
            }
            if values.contains_key(field_name) {
                return Err(RuntimeError::StructError(format!(
                    "field `{}` specified more than once", field_name
                )));
            }
            let value = self.eval_expr(field_expr)?;
            values.insert(field_name.clone(), value);
        }
        for decl_field in &decl.fields {
            if !values.contains_key(&decl_field.name) {
                return Err(RuntimeError::StructError(format!(
                    "missing field `{}` in struct `{}`", decl_field.name, name
                )));
            }
        }
        Ok(Value::Struct { name: name.to_string(), fields: values })
    }

    fn eval_enum_literal(
        &mut self,
        enum_name: &str,
        variant: &str,
        fields: &[(String, HExpr)],
    ) -> Result<Value, RuntimeError> {
        let Some(decl) = self.enums.get(enum_name).cloned() else {
            return Err(RuntimeError::EnumError(format!("undefined enum `{}`", enum_name)));
        };
        let Some(variant_decl) = decl.variants.iter().find(|v| v.name == variant) else {
            return Err(RuntimeError::EnumError(format!(
                "enum `{}` has no variant `{}`", enum_name, variant
            )));
        };
        if fields.len() != variant_decl.fields.len() {
            return Err(RuntimeError::EnumError(format!(
                "variant `{}::{}` expects {} field(s), found {}",
                enum_name, variant, variant_decl.fields.len(), fields.len()
            )));
        }
        let mut values: HashMap<String, Value> = HashMap::new();
        for (field_name, field_expr) in fields {
            if !variant_decl.fields.iter().any(|f| &f.name == field_name) {
                return Err(RuntimeError::EnumError(format!(
                    "variant `{}::{}` has no field `{}`", enum_name, variant, field_name
                )));
            }
            if values.contains_key(field_name) {
                return Err(RuntimeError::EnumError(format!(
                    "field `{}` specified more than once", field_name
                )));
            }
            let value = self.eval_expr(field_expr)?;
            values.insert(field_name.clone(), value);
        }
        for f in &variant_decl.fields {
            if !values.contains_key(&f.name) {
                return Err(RuntimeError::EnumError(format!(
                    "missing field `{}` in variant `{}::{}`", f.name, enum_name, variant
                )));
            }
        }
        Ok(Value::Enum { enum_name: enum_name.to_string(), variant: variant.to_string(), fields: values })
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
                if b == 0 { Err(RuntimeError::DivisionByZero) } else { Ok(Value::Int(a / b)) }
            }),
            Eq => Ok(Value::Bool(l == r)),
            NotEq => Ok(Value::Bool(l != r)),
            Lt => int_op(l, r, "<", |a, b| Ok(Value::Bool(a < b))),
            Gt => int_op(l, r, ">", |a, b| Ok(Value::Bool(a > b))),
            Le => int_op(l, r, "<=", |a, b| Ok(Value::Bool(a <= b))),
            Ge => int_op(l, r, ">=", |a, b| Ok(Value::Bool(a >= b))),
        }
    }

    fn call_function(&mut self, callee: &str, args: &[HExpr]) -> Result<Value, RuntimeError> {
        if callee == "print" {
            if args.len() != 1 {
                return Err(RuntimeError::WrongArgCount { callee: "print".to_string(), expected: 1, found: args.len() });
            }
            let value = self.eval_expr(&args[0])?;
            (self.print_sink)(&value.display());
            return Ok(Value::Unit);
        }
        if callee == "len" {
            if args.len() != 1 {
                return Err(RuntimeError::WrongArgCount { callee: "len".to_string(), expected: 1, found: args.len() });
            }
            let Value::Array(items) = self.eval_expr(&args[0])? else {
                return Err(RuntimeError::TypeError("len expects an Array".to_string()));
            };
            return Ok(Value::Int(items.len() as i64));
        }
        if callee == "push" {
            if args.len() != 2 {
                return Err(RuntimeError::WrongArgCount { callee: "push".to_string(), expected: 2, found: args.len() });
            }
            let Value::Array(mut items) = self.eval_expr(&args[0])? else {
                return Err(RuntimeError::TypeError("push expects an Array as its first argument".to_string()));
            };
            let new_item = self.eval_expr(&args[1])?;
            items.push(new_item);
            return Ok(Value::Array(items));
        }

        let Some(func) = self.functions.get(callee).cloned() else {
            return Err(RuntimeError::UndefinedFunction(callee.to_string()));
        };
        self.call_user_function(&func, args)
    }

    fn call_user_function(
        &mut self,
        func: &HFunctionDecl,
        args: &[HExpr],
    ) -> Result<Value, RuntimeError> {
        if args.len() != func.params.len() {
            return Err(RuntimeError::WrongArgCount {
                callee: func.name.clone(), expected: func.params.len(), found: args.len(),
            });
        }
        let mut arg_values = Vec::with_capacity(args.len());
        for arg in args {
            arg_values.push(self.eval_expr(arg)?);
        }
        let mut call_env = HashMap::new();
        for (param, value) in func.params.iter().zip(arg_values) {
            call_env.insert(param.name.clone(), Binding { value, is_mutable: false });
        }
        let caller_env = std::mem::replace(&mut self.env, call_env);
        let result = self.exec_block(&func.body);
        self.env = caller_env;

        match result? {
            ControlFlow::Return(value) => Ok(value),
            ControlFlow::Normal => Ok(Value::Unit),
        }
    }
}

fn type_error(op: &str, a: &Value, b: &Value) -> RuntimeError {
    RuntimeError::TypeError(format!("cannot apply `{}` to {} and {}", op, a.type_name(), b.type_name()))
}

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
    use kairo_hir::lower_program;
    use kairo_lexer::Lexer;
    use kairo_parser::Parser;

    fn run_and_capture(source: &str) -> Result<Vec<String>, RuntimeError> {
        let tokens = Lexer::new(source).tokenize().expect("lex failed");
        let ast_program = Parser::new(tokens).parse_program().expect("parse failed");
        let program = lower_program(&ast_program);

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
        assert_eq!(run_and_capture("fn main() { x := 2 + 3 * 4\nprint(x) }").unwrap(), vec!["14".to_string()]);
    }

    #[test]
    fn runs_grouping_overrides_precedence() {
        assert_eq!(run_and_capture("fn main() { x := (2 + 3) * 4\nprint(x) }").unwrap(), vec!["20".to_string()]);
    }

    #[test]
    fn runs_if_true_branch() {
        let source = r#"fn main() { if 1 < 2 { print("yes") } else { print("no") } }"#;
        assert_eq!(run_and_capture(source).unwrap(), vec!["yes".to_string()]);
    }

    #[test]
    fn runs_if_false_branch() {
        let source = r#"fn main() { if 5 == 6 { print("yes") } else { print("no") } }"#;
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
    fn runs_while_loop_with_mut_and_assignment() {
        let source = r#"
            fn main() {
                mut counter := 0
                while counter < 3 {
                    print(counter)
                    counter = counter + 1
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
        assert_eq!(run_and_capture("fn main() { x := 5 / 0 }").unwrap_err(), RuntimeError::DivisionByZero);
    }

    #[test]
    fn reports_type_error_on_bad_addition() {
        match run_and_capture("fn main() { x := 5 + true }").unwrap_err() {
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

    #[test]
    fn calls_user_function_with_params_and_return_value() {
        let source = r#"
            fn add(a: Int, b: Int) -> Int {
                return a + b
            }
            fn main() {
                result := add(2, 3)
                print(result)
            }
        "#;
        assert_eq!(run_and_capture(source).unwrap(), vec!["5".to_string()]);
    }

    #[test]
    fn supports_recursion() {
        let source = r#"
            fn fib(n: Int) -> Int {
                if n < 2 { return n }
                return fib(n - 1) + fib(n - 2)
            }
            fn main() { print(fib(10)) }
        "#;
        assert_eq!(run_and_capture(source).unwrap(), vec!["55".to_string()]);
    }

    #[test]
    fn constructs_struct_and_accesses_fields() {
        let source = r#"
            struct Point { x: Int, y: Int }
            fn main() {
                p := Point { x: 3, y: 4 }
                print(p.x)
                print(p.y)
            }
        "#;
        assert_eq!(run_and_capture(source).unwrap(), vec!["3".to_string(), "4".to_string()]);
    }

    #[test]
    fn matches_enum_variant_and_binds_data() {
        let source = r#"
            enum Status { Pending, Failed(reason: String) }
            fn main() {
                s := Status::Failed(reason: "timeout")
                match s {
                    Status::Pending => { print("pending") }
                    Status::Failed(reason) => { print(reason) }
                }
            }
        "#;
        assert_eq!(run_and_capture(source).unwrap(), vec!["timeout".to_string()]);
    }

    #[test]
    fn matches_wildcard_fallback() {
        let source = r#"
            enum Status { Pending, Done }
            fn main() {
                s := Status::Pending
                match s { Status::Done => { print("done") } _ => { print("other") } }
            }
        "#;
        assert_eq!(run_and_capture(source).unwrap(), vec!["other".to_string()]);
    }

    #[test]
    fn indexes_array() {
        assert_eq!(run_and_capture("fn main() { a := [10, 20, 30]\nprint(a[1]) }").unwrap(), vec!["20".to_string()]);
    }

    #[test]
    fn len_and_push_builtins() {
        let source = r#"
            fn main() {
                a := [1, 2, 3]
                print(len(a))
                b := push(a, 4)
                print(len(b))
            }
        "#;
        assert_eq!(run_and_capture(source).unwrap(), vec!["3".to_string(), "4".to_string()]);
    }

    #[test]
    fn try_propagates_err_early() {
        let source = r#"
            enum IntResult { Ok(value: Int), Err(error: String) }
            fn fail() -> IntResult { return IntResult::Err(error: "boom") }
            fn compute() -> IntResult {
                x := fail()?
                return IntResult::Ok(value: x + 1)
            }
            fn main() {
                r := compute()
                match r {
                    IntResult::Ok(v) => { print(v) }
                    IntResult::Err(e) => { print(e) }
                }
            }
        "#;
        assert_eq!(run_and_capture(source).unwrap(), vec!["boom".to_string()]);
    }
}
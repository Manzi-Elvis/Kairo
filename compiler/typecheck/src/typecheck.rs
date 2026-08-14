use kairo_ast::{BinaryOp, Expr, FunctionDecl, Program, Stmt, StructDecl};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    Bool,
    String,
    Unit,
    Struct(String),
}

impl Type {
    pub fn name(&self) -> String {
        match self {
            Type::Int => "Int".to_string(),
            Type::Bool => "Bool".to_string(),
            Type::String => "String".to_string(),
            Type::Unit => "Unit".to_string(),
            Type::Struct(n) => n.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeError {
    UndefinedType(String),
    UndefinedVariable(String),
    UndefinedFunction(String),
    UndefinedStruct(String),
    AlreadyDeclared(String),
    ImmutableAssignment(String),
    Mismatch { expected: String, found: String, context: String },
    WrongArgCount { callee: String, expected: usize, found: usize },
    UnknownField { struct_name: String, field: String },
    MissingField { struct_name: String, field: String },
    DuplicateField(String),
    NotAStruct(String),
    NoMainFunction,
}

struct FunctionSig {
    params: Vec<Type>,
    return_type: Type,
}

pub struct TypeChecker {
    struct_names: HashSet<String>,
    struct_fields: HashMap<String, HashMap<String, Type>>,
    functions: HashMap<String, FunctionSig>,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            struct_names: HashSet::new(),
            struct_fields: HashMap::new(),
            functions: HashMap::new(),
        }
    }

    pub fn check_program(&mut self, program: &Program) -> Result<(), Vec<TypeError>> {
        let mut errors = Vec::new();

        for s in &program.structs {
            self.struct_names.insert(s.name.clone());
        }
        for s in &program.structs {
            self.collect_struct_fields(s, &mut errors);
        }
        for f in &program.functions {
            self.collect_function_sig(f, &mut errors);
        }
        if !self.functions.contains_key("main") {
            errors.push(TypeError::NoMainFunction);
        }
        for f in &program.functions {
            self.check_function_body(f, &mut errors);
        }

        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    fn resolve_type(&self, name: &str) -> Option<Type> {
        match name {
            "Int" => Some(Type::Int),
            "Bool" => Some(Type::Bool),
            "String" => Some(Type::String),
            "Unit" => Some(Type::Unit),
            other if self.struct_names.contains(other) => Some(Type::Struct(other.to_string())),
            _ => None,
        }
    }

    fn collect_struct_fields(&mut self, s: &StructDecl, errors: &mut Vec<TypeError>) {
        let mut fields = HashMap::new();
        for field in &s.fields {
            match self.resolve_type(&field.type_name) {
                Some(t) => { fields.insert(field.name.clone(), t); }
                None => errors.push(TypeError::UndefinedType(field.type_name.clone())),
            }
        }
        self.struct_fields.insert(s.name.clone(), fields);
    }

    fn collect_function_sig(&mut self, f: &FunctionDecl, errors: &mut Vec<TypeError>) {
        let mut params = Vec::new();
        for p in &f.params {
            match self.resolve_type(&p.type_name) {
                Some(t) => params.push(t),
                None => errors.push(TypeError::UndefinedType(p.type_name.clone())),
            }
        }
        let return_type = match &f.return_type {
            None => Type::Unit,
            Some(name) => self.resolve_type(name).unwrap_or_else(|| {
                errors.push(TypeError::UndefinedType(name.clone()));
                Type::Unit
            }),
        };
        self.functions.insert(f.name.clone(), FunctionSig { params, return_type });
    }

    fn check_function_body(&self, f: &FunctionDecl, errors: &mut Vec<TypeError>) {
        let mut scope: HashMap<String, (Type, bool)> = HashMap::new();
        for p in &f.params {
            if let Some(t) = self.resolve_type(&p.type_name) {
                scope.insert(p.name.clone(), (t, false));
            }
        }
        let return_type = self
            .functions
            .get(&f.name)
            .map(|sig| sig.return_type.clone())
            .unwrap_or(Type::Unit);

        for stmt in &f.body {
            self.check_stmt(stmt, &mut scope, &return_type, errors);
        }
    }

    fn check_stmt(
        &self,
        stmt: &Stmt,
        scope: &mut HashMap<String, (Type, bool)>,
        return_type: &Type,
        errors: &mut Vec<TypeError>,
    ) {
        match stmt {
            Stmt::VariableDecl { name, value, is_mutable } => {
                if scope.contains_key(name) {
                    errors.push(TypeError::AlreadyDeclared(name.clone()));
                }
                if let Some(t) = self.eval_expr_type(value, scope, errors) {
                    scope.insert(name.clone(), (t, *is_mutable));
                }
            }
            Stmt::Assign { name, value } => {
                let value_type = self.eval_expr_type(value, scope, errors);
                match scope.get(name) {
                    None => errors.push(TypeError::UndefinedVariable(name.clone())),
                    Some((_, false)) => errors.push(TypeError::ImmutableAssignment(name.clone())),
                    Some((declared, true)) => {
                        if let Some(found) = value_type {
                            if found != *declared {
                                errors.push(TypeError::Mismatch {
                                    expected: declared.name(),
                                    found: found.name(),
                                    context: format!("assignment to `{}`", name),
                                });
                            }
                        }
                    }
                }
            }
            Stmt::Expr(expr) => {
                self.eval_expr_type(expr, scope, errors);
            }
            Stmt::If { condition, then_branch, else_branch } => {
                self.check_condition(condition, scope, errors);
                for s in then_branch {
                    self.check_stmt(s, scope, return_type, errors);
                }
                if let Some(else_stmts) = else_branch {
                    for s in else_stmts {
                        self.check_stmt(s, scope, return_type, errors);
                    }
                }
            }
            Stmt::While { condition, body } => {
                self.check_condition(condition, scope, errors);
                for s in body {
                    self.check_stmt(s, scope, return_type, errors);
                }
            }
            Stmt::Return(expr) => {
                let found = match expr {
                    Some(e) => self.eval_expr_type(e, scope, errors),
                    None => Some(Type::Unit),
                };
                if let Some(found) = found {
                    if found != *return_type {
                        errors.push(TypeError::Mismatch {
                            expected: return_type.name(),
                            found: found.name(),
                            context: "return statement".to_string(),
                        });
                    }
                }
            }
        }
    }

    fn check_condition(
        &self,
        condition: &Expr,
        scope: &HashMap<String, (Type, bool)>,
        errors: &mut Vec<TypeError>,
    ) {
        if let Some(t) = self.eval_expr_type(condition, scope, errors) {
            if t != Type::Bool {
                errors.push(TypeError::Mismatch {
                    expected: "Bool".to_string(),
                    found: t.name(),
                    context: "condition".to_string(),
                });
            }
        }
    }

    fn eval_expr_type(
        &self,
        expr: &Expr,
        scope: &HashMap<String, (Type, bool)>,
        errors: &mut Vec<TypeError>,
    ) -> Option<Type> {
        match expr {
            Expr::StringLiteral(_) => Some(Type::String),
            Expr::IntLiteral(_) => Some(Type::Int),
            Expr::BoolLiteral(_) => Some(Type::Bool),
            Expr::Identifier(name) => match scope.get(name) {
                Some((t, _)) => Some(t.clone()),
                None => {
                    errors.push(TypeError::UndefinedVariable(name.clone()));
                    None
                }
            },
            Expr::Binary { left, op, right } => {
                let l = self.eval_expr_type(left, scope, errors);
                let r = self.eval_expr_type(right, scope, errors);
                self.check_binary(*op, l, r, errors)
            }
            Expr::Call { callee, args } => self.check_call(callee, args, scope, errors),
            Expr::StructLiteral { name, fields } => {
                self.check_struct_literal(name, fields, scope, errors)
            }
            Expr::FieldAccess { object, field } => {
                let obj_type = self.eval_expr_type(object, scope, errors)?;
                match &obj_type {
                    Type::Struct(struct_name) => {
                        match self.struct_fields.get(struct_name).and_then(|f| f.get(field)) {
                            Some(t) => Some(t.clone()),
                            None => {
                                errors.push(TypeError::UnknownField {
                                    struct_name: struct_name.clone(),
                                    field: field.clone(),
                                });
                                None
                            }
                        }
                    }
                    other => {
                        errors.push(TypeError::NotAStruct(other.name()));
                        None
                    }
                }
            }
        }
    }

    fn check_binary(
        &self,
        op: BinaryOp,
        l: Option<Type>,
        r: Option<Type>,
        errors: &mut Vec<TypeError>,
    ) -> Option<Type> {
        use BinaryOp::*;
        let (l, r) = (l?, r?);

        match op {
            Add => match (&l, &r) {
                (Type::String, Type::String) => Some(Type::String),
                (Type::Int, Type::Int) => Some(Type::Int),
                _ => { errors.push(binary_mismatch("+", &l, &r)); None }
            },
            Sub | Mul | Div => {
                if l == Type::Int && r == Type::Int {
                    Some(Type::Int)
                } else {
                    errors.push(binary_mismatch(op_symbol(op), &l, &r));
                    None
                }
            }
            Eq | NotEq => {
                if l == r {
                    Some(Type::Bool)
                } else {
                    errors.push(binary_mismatch(op_symbol(op), &l, &r));
                    None
                }
            }
            Lt | Gt | Le | Ge => {
                if l == Type::Int && r == Type::Int {
                    Some(Type::Bool)
                } else {
                    errors.push(binary_mismatch(op_symbol(op), &l, &r));
                    None
                }
            }
        }
    }

    fn check_call(
        &self,
        callee: &str,
        args: &[Expr],
        scope: &HashMap<String, (Type, bool)>,
        errors: &mut Vec<TypeError>,
    ) -> Option<Type> {
        if callee == "print" {
            if args.len() != 1 {
                errors.push(TypeError::WrongArgCount {
                    callee: "print".to_string(),
                    expected: 1,
                    found: args.len(),
                });
            } else {
                self.eval_expr_type(&args[0], scope, errors);
            }
            return Some(Type::Unit);
        }

        let Some(sig_len) = self.functions.get(callee).map(|s| s.params.len()) else {
            errors.push(TypeError::UndefinedFunction(callee.to_string()));
            for a in args {
                self.eval_expr_type(a, scope, errors);
            }
            return None;
        };

        if args.len() != sig_len {
            errors.push(TypeError::WrongArgCount {
                callee: callee.to_string(),
                expected: sig_len,
                found: args.len(),
            });
        }

        let params = self.functions.get(callee).unwrap().params.clone();
        for (i, arg) in args.iter().enumerate() {
            let arg_type = self.eval_expr_type(arg, scope, errors);
            if let (Some(found), Some(expected)) = (arg_type, params.get(i)) {
                if found != *expected {
                    errors.push(TypeError::Mismatch {
                        expected: expected.name(),
                        found: found.name(),
                        context: format!("argument {} to `{}`", i + 1, callee),
                    });
                }
            }
        }

        Some(self.functions.get(callee).unwrap().return_type.clone())
    }

    fn check_struct_literal(
        &self,
        name: &str,
        fields: &[(String, Expr)],
        scope: &HashMap<String, (Type, bool)>,
        errors: &mut Vec<TypeError>,
    ) -> Option<Type> {
        let Some(decl_fields) = self.struct_fields.get(name) else {
            errors.push(TypeError::UndefinedStruct(name.to_string()));
            for (_, e) in fields {
                self.eval_expr_type(e, scope, errors);
            }
            return None;
        };

        let mut seen: HashSet<&String> = HashSet::new();
        for (field_name, field_expr) in fields {
            let found = self.eval_expr_type(field_expr, scope, errors);
            match decl_fields.get(field_name) {
                None => {
                    errors.push(TypeError::UnknownField {
                        struct_name: name.to_string(),
                        field: field_name.clone(),
                    });
                }
                Some(expected) => {
                    if !seen.insert(field_name) {
                        errors.push(TypeError::DuplicateField(field_name.clone()));
                    }
                    if let Some(found) = found {
                        if found != *expected {
                            errors.push(TypeError::Mismatch {
                                expected: expected.name(),
                                found: found.name(),
                                context: format!("field `{}` of `{}`", field_name, name),
                            });
                        }
                    }
                }
            }
        }

        for decl_field_name in decl_fields.keys() {
            if !seen.contains(decl_field_name) {
                errors.push(TypeError::MissingField {
                    struct_name: name.to_string(),
                    field: decl_field_name.clone(),
                });
            }
        }

        Some(Type::Struct(name.to_string()))
    }
}

fn binary_mismatch(op: &str, l: &Type, r: &Type) -> TypeError {
    TypeError::Mismatch {
        expected: format!("matching operand types for `{}`", op),
        found: format!("{} and {}", l.name(), r.name()),
        context: format!("binary `{}`", op),
    }
}

fn op_symbol(op: BinaryOp) -> &'static str {
    use BinaryOp::*;
    match op {
        Add => "+", Sub => "-", Mul => "*", Div => "/",
        Eq => "==", NotEq => "!=", Lt => "<", Gt => ">", Le => "<=", Ge => ">=",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_lexer::Lexer;
    use kairo_parser::Parser;

    fn check(source: &str) -> Result<(), Vec<TypeError>> {
        let tokens = Lexer::new(source).tokenize().expect("lex failed");
        let program = Parser::new(tokens).parse_program().expect("parse failed");
        TypeChecker::new().check_program(&program)
    }

    #[test]
    fn passes_valid_program() {
        let source = r#"
            struct Point { x: Int, y: Int }
            fn add(a: Int, b: Int) -> Int { return a + b }
            fn main() {
                p := Point { x: 1, y: 2 }
                print(p.x)
                print(add(1, 2))
            }
        "#;
        assert_eq!(check(source), Ok(()));
    }

    #[test]
    fn catches_binary_type_mismatch() {
        let err = check("fn main() { x := 5 + true }").unwrap_err();
        assert!(matches!(err[0], TypeError::Mismatch { .. }));
    }

    #[test]
    fn catches_undefined_variable() {
        let err = check("fn main() { print(missing) }").unwrap_err();
        assert_eq!(err[0], TypeError::UndefinedVariable("missing".to_string()));
    }

    #[test]
    fn catches_wrong_arg_count() {
        let source = "fn add(a: Int, b: Int) -> Int { return a + b }\nfn main() { add(1) }";
        let err = check(source).unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::WrongArgCount { .. })));
    }

    #[test]
    fn catches_return_type_mismatch() {
        let err = check("fn f() -> Int { return true }\nfn main() {}").unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::Mismatch { .. })));
    }

    #[test]
    fn catches_struct_field_type_mismatch() {
        let source = "struct P { x: Int }\nfn main() { p := P { x: true } }";
        let err = check(source).unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::Mismatch { .. })));
    }

    #[test]
    fn catches_missing_struct_field() {
        let source = "struct P { x: Int, y: Int }\nfn main() { p := P { x: 1 } }";
        let err = check(source).unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::MissingField { .. })));
    }

    #[test]
    fn catches_non_bool_condition() {
        let err = check("fn main() { if 5 { print(\"no\") } }").unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::Mismatch { .. })));
    }

    #[test]
    fn catches_missing_main() {
        let err = check("fn notMain() {}").unwrap_err();
        assert!(err.contains(&TypeError::NoMainFunction));
    }

    #[test]
    fn catches_undefined_type_in_param() {
        let err = check("fn f(x: Ghost) {}\nfn main() {}").unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::UndefinedType(_))));
    }
}
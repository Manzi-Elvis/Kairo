use kairo_ast::{BinaryOp, EnumDecl, Expr, FunctionDecl, MatchArm, Pattern, Program, Stmt, StructDecl};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    Bool,
    String,
    Unit,
    Struct(String),
    Array(Box<Type>),
    Enum(String),
}

impl Type {
    pub fn name(&self) -> String {
        match self {
            Type::Int => "Int".to_string(),
            Type::Bool => "Bool".to_string(),
            Type::String => "String".to_string(),
            Type::Unit => "Unit".to_string(),
            Type::Struct(n) => n.clone(),
            Type::Array(inner) => format!("Array<{}>", inner.name()),
            Type::Enum(n) => n.clone(),
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
    CannotInferType(String),
    WrongArgCount { callee: String, expected: usize, found: usize },
    UnknownField { struct_name: String, field: String },
    MissingField { struct_name: String, field: String },
    DuplicateField(String),
    NotAStruct(String),
    NoMainFunction,
    UndefinedEnum(String),
    UndefinedVariant { enum_name: String, variant: String },
    NonExhaustiveMatch(String),
    NotResultShaped(String),
}

struct FunctionSig {
    params: Vec<Type>,
    return_type: Type,
}

pub struct TypeChecker {
    struct_names: HashSet<String>,
    struct_fields: HashMap<String, HashMap<String, Type>>,
    enum_names: HashSet<String>,
    enum_variants: HashMap<String, HashMap<String, Vec<(String, Type)>>>,
    functions: HashMap<String, FunctionSig>,
    current_return_type: Type,
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
            enum_names: HashSet::new(),
            enum_variants: HashMap::new(),
            functions: HashMap::new(),
            current_return_type: Type::Unit,
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
        for e in &program.enums {
            self.enum_names.insert(e.name.clone());
        }
        for e in &program.enums {
            self.collect_enum_variants(e, &mut errors);
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
                  other if self.enum_names.contains(other) => Some(Type::Enum(other.to_string())),
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

    fn collect_enum_variants(&mut self, e: &EnumDecl, errors: &mut Vec<TypeError>) {
        let mut variants = HashMap::new();
        for v in &e.variants {
            let mut fields = Vec::new();
            for field in &v.fields {
                match self.resolve_type(&field.type_name) {
                    Some(t) => fields.push((field.name.clone(), t)),
                    None => errors.push(TypeError::UndefinedType(field.type_name.clone())),
                }
            }
            variants.insert(v.name.clone(), fields);
        }
        self.enum_variants.insert(e.name.clone(), variants);
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

    fn check_function_body(&mut self, f: &FunctionDecl, errors: &mut Vec<TypeError>) {
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

        // Stored so Expr::Try (nested arbitrarily deep in eval_expr_type,
        // which isn't itself threaded with return_type) can check against it.
        // Safe because functions are checked one at a time, not re-entrantly.
        self.current_return_type = return_type.clone();
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

                    Some((_, false)) => {
                        errors.push(TypeError::ImmutableAssignment(name.clone()));
                    }

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
            Stmt::IndexAssign { name, index, value } => {
                let idx_type = self.eval_expr_type(index, scope, errors);
                if let Some(idx_type) = idx_type {
                    if idx_type != Type::Int {
                        errors.push(TypeError::Mismatch {
                            expected: "Int".to_string(),
                            found: idx_type.name(),
                            context: "array index".to_string(),
                        });
                    }
                }
                let value_type = self.eval_expr_type(value, scope, errors);
                match scope.get(name) {
                    None => errors.push(TypeError::UndefinedVariable(name.clone())),
                    Some((_, false)) => errors.push(TypeError::ImmutableAssignment(name.clone())),
                    Some((Type::Array(inner), true)) => {
                        if let Some(found) = value_type {
                            if found != **inner {
                                errors.push(TypeError::Mismatch {
                                    expected: inner.name(),
                                    found: found.name(),
                                    context: format!("index assignment to `{}`", name),
                                });
                            }
                        }
                    }
                    Some((other, true)) => errors.push(TypeError::Mismatch {
                        expected: "Array".to_string(),
                        found: other.name(),
                        context: format!("index assignment to `{}`", name),
                    }),
                }
            }

            Stmt::Expr(expr) => {
                self.eval_expr_type(expr, scope, errors);
            }

            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
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

            Stmt::Match { scrutinee, arms } => {
                if let Some(scrutinee_type) =
                    self.eval_expr_type(scrutinee, scope, errors)
                {
                    for arm in arms {
                        let mut arm_scope = scope.clone();

                        self.check_pattern(
                            &arm.pattern,
                            &scrutinee_type,
                            &mut arm_scope,
                            errors,
                        );

                        for s in &arm.body {
                            self.check_stmt(
                                s,
                                &mut arm_scope,
                                return_type,
                                errors,
                            );
                        }
                    }

                    self.check_exhaustiveness(
                        &scrutinee_type,
                        arms,
                        errors,
                    );
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

        fn check_pattern(
        &self,
        pattern: &Pattern,
        scrutinee_type: &Type,
        scope: &mut HashMap<String, (Type, bool)>,
        errors: &mut Vec<TypeError>,
    ) {
        match pattern {
            Pattern::Wildcard => {}

            Pattern::IntLiteral(_) => {
                if *scrutinee_type != Type::Int {
                    errors.push(TypeError::Mismatch {
                        expected: scrutinee_type.name(),
                        found: "Int".to_string(),
                        context: "match pattern".to_string(),
                    });
                }
            }

            Pattern::BoolLiteral(_) => {
                if *scrutinee_type != Type::Bool {
                    errors.push(TypeError::Mismatch {
                        expected: scrutinee_type.name(),
                        found: "Bool".to_string(),
                        context: "match pattern".to_string(),
                    });
                }
            }

            Pattern::StringLiteral(_) => {
                if *scrutinee_type != Type::String {
                    errors.push(TypeError::Mismatch {
                        expected: scrutinee_type.name(),
                        found: "String".to_string(),
                        context: "match pattern".to_string(),
                    });
                }
            }

            Pattern::EnumVariant {
                enum_name,
                variant,
                bindings,
            } => match scrutinee_type {
                Type::Enum(n) if n == enum_name => {
                    let Some(fields) = self
                        .enum_variants
                        .get(enum_name)
                        .and_then(|v| v.get(variant))
                    else {
                        errors.push(TypeError::UndefinedVariant {
                            enum_name: enum_name.clone(),
                            variant: variant.clone(),
                        });
                        return;
                    };

                    if bindings.len() != fields.len() {
                        errors.push(TypeError::WrongArgCount {
                            callee: format!("{}::{}", enum_name, variant),
                            expected: fields.len(),
                            found: bindings.len(),
                        });
                        return;
                    }

                    for (binding_name, (_, field_type)) in
                        bindings.iter().zip(fields.iter())
                    {
                        scope.insert(
                            binding_name.clone(),
                            (field_type.clone(), false),
                        );
                    }
                }

                other => {
                    errors.push(TypeError::Mismatch {
                        expected: other.name(),
                        found: format!("{}::{}", enum_name, variant),
                        context: "match pattern".to_string(),
                    });
                }
            },
        }
    }

    fn check_exhaustiveness(
        &self,
        scrutinee_type: &Type,
        arms: &[MatchArm],
        errors: &mut Vec<TypeError>,
    ) {
        if arms
            .iter()
            .any(|a| matches!(a.pattern, Pattern::Wildcard))
        {
            return;
        }

        match scrutinee_type {
            Type::Enum(name) => {
                if let Some(variants) = self.enum_variants.get(name) {
                    let covered: HashSet<&String> = arms
                        .iter()
                        .filter_map(|a| match &a.pattern {
                            Pattern::EnumVariant { variant, .. } => Some(variant),
                            _ => None,
                        })
                        .collect();

                    for variant_name in variants.keys() {
                        if !covered.contains(variant_name) {
                            errors.push(TypeError::NonExhaustiveMatch(
                                format!("{}::{}", name, variant_name),
                            ));
                        }
                    }
                }
            }

            Type::Bool => {
                let covered: HashSet<bool> = arms
                    .iter()
                    .filter_map(|a| match &a.pattern {
                        Pattern::BoolLiteral(b) => Some(*b),
                        _ => None,
                    })
                    .collect();

                if !covered.contains(&true) || !covered.contains(&false) {
                    errors.push(TypeError::NonExhaustiveMatch(
                        "Bool (missing true, false, or a wildcard `_`)"
                            .to_string(),
                    ));
                }
            }

            other => {
                errors.push(TypeError::NonExhaustiveMatch(
                    format!("{} requires a wildcard `_` arm", other.name()),
                ));
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
            Expr::EnumLiteral { enum_name, variant, fields } => {
                self.check_enum_literal(enum_name, variant, fields, scope, errors)
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
            Expr::ArrayLiteral(elements) => {
                if elements.is_empty() {
                    errors.push(TypeError::CannotInferType(
                        "empty array literal (no elements to infer type from)".to_string(),
                    ));
                    return None;
                }
                let first = self.eval_expr_type(&elements[0], scope, errors)?;
                for e in &elements[1..] {
                    if let Some(t) = self.eval_expr_type(e, scope, errors) {
                        if t != first {
                            errors.push(TypeError::Mismatch {
                                expected: first.name(),
                                found: t.name(),
                                context: "array literal element".to_string(),
                            });
                        }
                    }
                }
                Some(Type::Array(Box::new(first)))
            }

            Expr::Index { array, index } => {
                let arr_type = self.eval_expr_type(array, scope, errors)?;
                let idx_type = self.eval_expr_type(index, scope, errors);
                if let Some(idx_type) = idx_type {
                    if idx_type != Type::Int {
                        errors.push(TypeError::Mismatch {
                            expected: "Int".to_string(),
                            found: idx_type.name(),
                            context: "array index".to_string(),
                        });
                    }
                }
                match arr_type {
                    Type::Array(inner) => Some(*inner),
                    other => {
                        errors.push(TypeError::Mismatch {
                            expected: "Array".to_string(),
                            found: other.name(),
                            context: "indexing".to_string(),
                        });
                        None
                    }
                }
            }
            Expr::Try(inner) => self.check_try(inner, scope, errors),        }
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

        if callee == "len" {
            if args.len() != 1 {
                errors.push(TypeError::WrongArgCount { callee: "len".to_string(), expected: 1, found: args.len() });
            } else if let Some(t) = self.eval_expr_type(&args[0], scope, errors) {
                if !matches!(t, Type::Array(_)) {
                    errors.push(TypeError::Mismatch {
                        expected: "Array".to_string(), found: t.name(), context: "argument to `len`".to_string(),
                    });
                }
            }
            return Some(Type::Int);
        }
        if callee == "push" {
            if args.len() != 2 {
                errors.push(TypeError::WrongArgCount { callee: "push".to_string(), expected: 2, found: args.len() });
                for a in args { self.eval_expr_type(a, scope, errors); }
                return None;
            }
            let arr_t = self.eval_expr_type(&args[0], scope, errors);
            let val_t = self.eval_expr_type(&args[1], scope, errors);
            return match arr_t {
                Some(Type::Array(inner)) => {
                    if let Some(val_t) = val_t {
                        if val_t != *inner {
                            errors.push(TypeError::Mismatch {
                                expected: inner.name(), found: val_t.name(),
                                context: "second argument to `push`".to_string(),
                            });
                        }
                    }
                    Some(Type::Array(inner))
                }
                Some(other) => {
                    errors.push(TypeError::Mismatch {
                        expected: "Array".to_string(), found: other.name(),
                        context: "first argument to `push`".to_string(),
                    });
                    None
                }
                None => None,
            };
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

    fn check_enum_literal(
        &self,
        enum_name: &str,
        variant: &str,
        fields: &[(String, Expr)],
        scope: &HashMap<String, (Type, bool)>,
        errors: &mut Vec<TypeError>,
    ) -> Option<Type> {
        let Some(variants) = self.enum_variants.get(enum_name) else {
            errors.push(TypeError::UndefinedEnum(enum_name.to_string()));
            for (_, e) in fields {
                self.eval_expr_type(e, scope, errors);
            }
            return None;
        };
        let Some(decl_fields) = variants.get(variant) else {
            errors.push(TypeError::UndefinedVariant {
                enum_name: enum_name.to_string(),
                variant: variant.to_string(),
            });
            for (_, e) in fields {
                self.eval_expr_type(e, scope, errors);
            }
            return None;
        };

        let mut seen: HashSet<&String> = HashSet::new();
        for (field_name, field_expr) in fields {
            let found = self.eval_expr_type(field_expr, scope, errors);
            match decl_fields.iter().find(|(n, _)| n == field_name) {
                None => errors.push(TypeError::UnknownField {
                    struct_name: format!("{}::{}", enum_name, variant),
                    field: field_name.clone(),
                }),
                Some((_, expected)) => {
                    if !seen.insert(field_name) {
                        errors.push(TypeError::DuplicateField(field_name.clone()));
                    }
                    if let Some(found) = found {
                        if found != *expected {
                            errors.push(TypeError::Mismatch {
                                expected: expected.name(),
                                found: found.name(),
                                context: format!("field `{}` of `{}::{}`", field_name, enum_name, variant),
                            });
                        }
                    }
                }
            }
        }
        for (decl_field_name, _) in decl_fields {
            if !seen.contains(decl_field_name) {
                errors.push(TypeError::MissingField {
                    struct_name: format!("{}::{}", enum_name, variant),
                    field: decl_field_name.clone(),
                });
            }
        }

        Some(Type::Enum(enum_name.to_string()))
    }

    fn check_try(
        &self,
        inner: &Expr,
        scope: &HashMap<String, (Type, bool)>,
        errors: &mut Vec<TypeError>,
    ) -> Option<Type> {
        let inner_type = self.eval_expr_type(inner, scope, errors)?;
        let Type::Enum(enum_name) = &inner_type else {
            errors.push(TypeError::NotResultShaped(format!(
                "`?` requires an enum type, found {}", inner_type.name()
            )));
            return None;
        };
        let variants = self.enum_variants.get(enum_name)?;
        let ok_fields = variants.get("Ok")?;
        let err_fields = variants.get("Err")?;
        if ok_fields.len() != 1 || ok_fields[0].0 != "value"
            || err_fields.len() != 1 || err_fields[0].0 != "error"
        {
            errors.push(TypeError::NotResultShaped(format!(
                "enum `{}` is not Ok/Err-shaped (need Ok(value: T), Err(error: E))", enum_name
            )));
            return None;
        }
        if self.current_return_type != inner_type {
            errors.push(TypeError::Mismatch {
                expected: self.current_return_type.name(),
                found: inner_type.name(),
                context: "`?` requires the current function to return the same Result-shaped enum".to_string(),
            });
        }
        Some(ok_fields[0].1.clone())
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

    #[test]
    fn passes_valid_enum_construction() {
        let source = r#"
            enum Status { Pending, Failed(reason: String) }
            fn main() {
                a := Status::Pending
                b := Status::Failed(reason: "x")
            }
        "#;
        assert_eq!(check(source), Ok(()));
    }

    #[test]
    fn catches_undefined_variant() {
        let source = "enum Status { Pending }\nfn main() { s := Status::Ghost }";
        let err = check(source).unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::UndefinedVariant { .. })));
    }

    #[test]
    fn catches_variant_field_type_mismatch() {
        let source =
            "enum Status { Failed(reason: String) }\nfn main() { s := Status::Failed(reason: 5) }";
        let err = check(source).unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::Mismatch { .. })));
    }

    #[test]
    fn passes_exhaustive_enum_match() {
        let source = r#"
            enum Status { Pending, Done }
            fn main() {
                s := Status::Pending
                match s {
                    Status::Pending => { print("p") }
                    Status::Done => { print("d") }
                }
            }
        "#;
        assert_eq!(check(source), Ok(()));
    }

    #[test]
    fn catches_non_exhaustive_enum_match() {
        let source = r#"
            enum Status { Pending, Done }
            fn main() {
                s := Status::Pending
                match s {
                    Status::Pending => { print("p") }
                }
            }
        "#;
        let err = check(source).unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::NonExhaustiveMatch(_))));
    }

    #[test]
    fn wildcard_satisfies_exhaustiveness() {
        let source = r#"
            enum Status { Pending, Done }
            fn main() {
                s := Status::Pending
                match s {
                    Status::Pending => { print("p") }
                    _ => { print("other") }
                }
            }
        "#;
        assert_eq!(check(source), Ok(()));
    }

    #[test]
    fn catches_int_match_without_wildcard() {
        let source = "fn main() { x := 5\nmatch x { 1 => { print(\"one\") } } }";
        let err = check(source).unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::NonExhaustiveMatch(_))));
    }

    #[test]
    fn binds_enum_field_type_correctly() {
        let source = r#"
            enum Status { Failed(reason: String) }
            fn main() {
                s := Status::Failed(reason: "x")
                match s {
                    Status::Failed(reason) => { print(reason) }
                }
            }
        "#;
        assert_eq!(check(source), Ok(()));
    }

    #[test]
    fn catches_pattern_type_mismatch() {
        let source = "fn main() { x := 5\nmatch x { true => { print(\"t\") } _ => { print(\"o\") } } }";
        let err = check(source).unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::Mismatch { .. })));
    }

    #[test]
    fn passes_valid_array_usage() {
        let source = r#"
            fn main() {
                mut a := [1, 2, 3]
                a[0] = 9
                x := a[0]
                print(len(a))
                b := push(a, 4)
            }
        "#;
        assert_eq!(check(source), Ok(()));
    }

    #[test]
    fn catches_array_element_type_mismatch() {
        let err = check("fn main() { a := [1, true] }").unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::Mismatch { .. })));
    }

    #[test]
    fn catches_non_int_index() {
        let err = check("fn main() { a := [1, 2]\nx := a[\"k\"] }").unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::Mismatch { .. })));
    }

    #[test]
    fn catches_index_assign_type_mismatch() {
        let err = check("fn main() { mut a := [1, 2]\na[0] = true }").unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::Mismatch { .. })));
    }

    #[test]
    fn catches_push_type_mismatch() {
        let err = check("fn main() { a := [1, 2]\nb := push(a, true) }").unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::Mismatch { .. })));
    }

    #[test]
    fn passes_valid_try_usage() {
        let source = r#"
            enum IntResult { Ok(value: Int), Err(error: String) }
            fn make() -> IntResult { return IntResult::Ok(value: 1) }
            fn compute() -> IntResult {
                x := make()?
                return IntResult::Ok(value: x)
            }
            fn main() {}
        "#;
        assert_eq!(check(source), Ok(()));
    }

    #[test]
    fn catches_try_on_non_enum() {
        let source = "fn f() -> Int { x := 5?\nreturn x }\nfn main() {}";
        let err = check(source).unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::NotResultShaped(_))));
    }

    #[test]
    fn catches_try_return_type_mismatch() {
        let source = r#"
            enum IntResult { Ok(value: Int), Err(error: String) }
            fn make() -> IntResult { return IntResult::Ok(value: 1) }
            fn compute() -> Int {
                x := make()?
                return x
            }
            fn main() {}
        "#;
        let err = check(source).unwrap_err();
        assert!(err.iter().any(|e| matches!(e, TypeError::Mismatch { .. })));
    }
}
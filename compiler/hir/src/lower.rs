use kairo_ast as ast;
use crate::hir::{HExpr, HFunctionDecl, HProgram, HStmt};
use std::collections::HashMap;

pub fn lower_program(program: &ast::Program) -> HProgram {
    Lowerer::new(&program.enums).lower_program(program)
}

struct Lowerer {
    // enum_name -> variant_name -> ordered field names
    enum_fields: HashMap<String, HashMap<String, Vec<String>>>,
    temp_counter: usize,
}

impl Lowerer {
    fn new(enums: &[ast::EnumDecl]) -> Self {
        let mut enum_fields = HashMap::new();
        for e in enums {
            let variants = e
                .variants
                .iter()
                .map(|v| (v.name.clone(), v.fields.iter().map(|f| f.name.clone()).collect()))
                .collect();
            enum_fields.insert(e.name.clone(), variants);
        }
        Self { enum_fields, temp_counter: 0 }
    }

    fn lower_program(mut self, program: &ast::Program) -> HProgram {
        let functions = program.functions.iter().map(|f| self.lower_function(f)).collect();
        HProgram { structs: program.structs.clone(), enums: program.enums.clone(), functions }
    }

    fn lower_function(&mut self, f: &ast::FunctionDecl) -> HFunctionDecl {
        HFunctionDecl {
            name: f.name.clone(),
            params: f.params.clone(),
            return_type: f.return_type.clone(),
            body: self.lower_stmts(&f.body),
        }
    }

    fn fresh_temp(&mut self) -> String {
        self.temp_counter += 1;
        format!("__match_scrutinee_{}", self.temp_counter)
    }

    fn lower_stmts(&mut self, stmts: &[ast::Stmt]) -> Vec<HStmt> {
        stmts.iter().flat_map(|s| self.lower_stmt(s)).collect()
    }

    /// Returns a Vec because `match` lowers into two statements
    /// (a temp declaration, then an if/else chain), while every
    /// other statement lowers 1:1.
    fn lower_stmt(&mut self, stmt: &ast::Stmt) -> Vec<HStmt> {
        match stmt {
            ast::Stmt::VariableDecl { name, value, is_mutable } => vec![HStmt::VariableDecl {
                name: name.clone(), value: self.lower_expr(value), is_mutable: *is_mutable,
            }],
            ast::Stmt::Assign { name, value } => vec![HStmt::Assign {
                name: name.clone(), value: self.lower_expr(value),
            }],
            ast::Stmt::IndexAssign { name, index, value } => vec![HStmt::IndexAssign {
                name: name.clone(), index: self.lower_expr(index), value: self.lower_expr(value),
            }],
            ast::Stmt::Expr(e) => vec![HStmt::Expr(self.lower_expr(e))],
            ast::Stmt::If { condition, then_branch, else_branch } => vec![HStmt::If {
                condition: self.lower_expr(condition),
                then_branch: self.lower_stmts(then_branch),
                else_branch: else_branch.as_ref().map(|b| self.lower_stmts(b)),
            }],
            ast::Stmt::While { condition, body } => vec![HStmt::While {
                condition: self.lower_expr(condition), body: self.lower_stmts(body),
            }],
            ast::Stmt::Return(expr) => vec![HStmt::Return(expr.as_ref().map(|e| self.lower_expr(e)))],
            ast::Stmt::Match { scrutinee, arms } => self.lower_match(scrutinee, arms),
        }
    }

    fn lower_match(&mut self, scrutinee: &ast::Expr, arms: &[ast::MatchArm]) -> Vec<HStmt> {
        let temp = self.fresh_temp();
        let temp_decl = HStmt::VariableDecl {
            name: temp.clone(), value: self.lower_expr(scrutinee), is_mutable: false,
        };
        let mut out = vec![temp_decl];
        out.extend(self.lower_match_arms(&temp, arms));
        out
    }

    /// Builds a nested if/else chain, one level per arm, in source
    /// order (first match wins, matching the interpreter's existing
    /// semantics). A wildcard arm becomes the final unconditional
    /// `else` and any arms after it are unreachable, so lowering
    /// stops there.
    fn lower_match_arms(&mut self, temp: &str, arms: &[ast::MatchArm]) -> Vec<HStmt> {
        let Some((arm, rest)) = arms.split_first() else { return vec![] };
        let arm_body = self.lower_pattern_body(temp, &arm.pattern, &arm.body);

        if matches!(arm.pattern, ast::Pattern::Wildcard) {
            return arm_body;
        }

        let test = self.pattern_test(temp, &arm.pattern);
        let else_branch = self.lower_match_arms(temp, rest);
        vec![HStmt::If {
            condition: test,
            then_branch: arm_body,
            else_branch: if else_branch.is_empty() { None } else { Some(else_branch) },
        }]
    }

    fn pattern_test(&self, temp: &str, pattern: &ast::Pattern) -> HExpr {
        let temp_ref = || HExpr::Identifier(temp.to_string());
        match pattern {
            ast::Pattern::Wildcard => HExpr::BoolLiteral(true),
            ast::Pattern::IntLiteral(v) => HExpr::Binary {
                left: Box::new(temp_ref()), op: ast::BinaryOp::Eq, right: Box::new(HExpr::IntLiteral(*v)),
            },
            ast::Pattern::BoolLiteral(v) => HExpr::Binary {
                left: Box::new(temp_ref()), op: ast::BinaryOp::Eq, right: Box::new(HExpr::BoolLiteral(*v)),
            },
            ast::Pattern::StringLiteral(v) => HExpr::Binary {
                left: Box::new(temp_ref()), op: ast::BinaryOp::Eq, right: Box::new(HExpr::StringLiteral(v.clone())),
            },
            ast::Pattern::EnumVariant { enum_name, variant, .. } => HExpr::IsVariant {
                scrutinee: Box::new(temp_ref()), enum_name: enum_name.clone(), variant: variant.clone(),
            },
        }
    }

    /// Prepends field-binding statements (for enum variant patterns
    /// with bindings) before the arm's own lowered body.
    fn lower_pattern_body(&mut self, temp: &str, pattern: &ast::Pattern, body: &[ast::Stmt]) -> Vec<HStmt> {
        let mut out = Vec::new();
        if let ast::Pattern::EnumVariant { enum_name, variant, bindings } = pattern {
            if let Some(field_names) = self.enum_fields.get(enum_name).and_then(|v| v.get(variant)).cloned() {
                for (binding_name, field_name) in bindings.iter().zip(field_names.iter()) {
                    out.push(HStmt::VariableDecl {
                        name: binding_name.clone(),
                        value: HExpr::VariantField {
                            scrutinee: Box::new(HExpr::Identifier(temp.to_string())),
                            enum_name: enum_name.clone(),
                            variant: variant.clone(),
                            field: field_name.clone(),
                        },
                        is_mutable: false,
                    });
                }
            }
        }
        out.extend(self.lower_stmts(body));
        out
    }

    fn lower_expr(&mut self, expr: &ast::Expr) -> HExpr {
        match expr {
            ast::Expr::StringLiteral(s) => HExpr::StringLiteral(s.clone()),
            ast::Expr::IntLiteral(v) => HExpr::IntLiteral(*v),
            ast::Expr::BoolLiteral(v) => HExpr::BoolLiteral(*v),
            ast::Expr::Identifier(n) => HExpr::Identifier(n.clone()),
            ast::Expr::Binary { left, op, right } => HExpr::Binary {
                left: Box::new(self.lower_expr(left)), op: *op, right: Box::new(self.lower_expr(right)),
            },
            ast::Expr::Call { callee, args } => HExpr::Call {
                callee: callee.clone(), args: args.iter().map(|a| self.lower_expr(a)).collect(),
            },
            ast::Expr::StructLiteral { name, fields } => HExpr::StructLiteral {
                name: name.clone(),
                fields: fields.iter().map(|(n, e)| (n.clone(), self.lower_expr(e))).collect(),
            },
            ast::Expr::FieldAccess { object, field } => HExpr::FieldAccess {
                object: Box::new(self.lower_expr(object)), field: field.clone(),
            },
            ast::Expr::EnumLiteral { enum_name, variant, fields } => HExpr::EnumLiteral {
                enum_name: enum_name.clone(), variant: variant.clone(),
                fields: fields.iter().map(|(n, e)| (n.clone(), self.lower_expr(e))).collect(),
            },
            ast::Expr::ArrayLiteral(elements) => {
                HExpr::ArrayLiteral(elements.iter().map(|e| self.lower_expr(e)).collect())
            }
            ast::Expr::Index { array, index } => HExpr::Index {
                array: Box::new(self.lower_expr(array)), index: Box::new(self.lower_expr(index)),
            },
            ast::Expr::Try(inner) => HExpr::Try(Box::new(self.lower_expr(inner))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_lexer::Lexer;
    use kairo_parser::Parser;

    fn lower_source(source: &str) -> HProgram {
        let tokens = Lexer::new(source).tokenize().expect("lex failed");
        let program = Parser::new(tokens).parse_program().expect("parse failed");
        lower_program(&program)
    }

    #[test]
    fn lowers_wildcard_only_match_to_its_body() {
        let hir = lower_source("fn main() { x := 5\nmatch x { _ => { print(1) } } }");
        // temp decl + the wildcard's body inlined directly (no If wrapper)
        assert_eq!(hir.functions[0].body.len(), 3);
        assert!(matches!(hir.functions[0].body[0], HStmt::VariableDecl { .. }));
        assert!(matches!(hir.functions[0].body[1], HStmt::VariableDecl { .. }));
        assert_eq!(hir.functions[0].body[2], HStmt::Expr(HExpr::Call {
            callee: "print".to_string(), args: vec![HExpr::IntLiteral(1)],
        }));
    }

    #[test]
    fn lowers_int_literal_pattern_to_equality_test() {
        let hir = lower_source("fn main() { x := 5\nmatch x { 1 => { print(1) } _ => { print(2) } } }");
        let HStmt::If { condition, .. } = &hir.functions[0].body[2] else {
            panic!("expected If as second statement");
        };
        assert_eq!(
            *condition,
            HExpr::Binary {
                left: Box::new(HExpr::Identifier("__match_scrutinee_1".to_string())),
                op: ast::BinaryOp::Eq,
                right: Box::new(HExpr::IntLiteral(1)),
            }
        );
    }

    #[test]
    fn lowers_enum_pattern_to_is_variant_and_binds_field() {
        let source = r#"
            enum Status { Pending, Failed(reason: String) }
            fn main() {
                s := Status::Pending
                match s {
                    Status::Failed(reason) => { print(reason) }
                    _ => { print("ok") }
                }
            }
        "#;
        let hir = lower_source(source);
        let HStmt::If { condition, then_branch, .. } = &hir.functions[0].body[2] else {
            panic!("expected If as second statement");
        };
        assert_eq!(
            *condition,
            HExpr::IsVariant {
                scrutinee: Box::new(HExpr::Identifier("__match_scrutinee_1".to_string())),
                enum_name: "Status".to_string(),
                variant: "Failed".to_string(),
            }
        );
        assert_eq!(
            then_branch[0],
            HStmt::VariableDecl {
                name: "reason".to_string(),
                value: HExpr::VariantField {
                    scrutinee: Box::new(HExpr::Identifier("__match_scrutinee_1".to_string())),
                    enum_name: "Status".to_string(),
                    variant: "Failed".to_string(),
                    field: "reason".to_string(),
                },
                is_mutable: false,
            }
        );
    }

    #[test]
    fn leaves_try_as_passthrough_node() {
        let hir = lower_source("fn f() -> Int { x := doThing()?\nreturn x }\nfn main() {}");
        let HStmt::VariableDecl { value, .. } = &hir.functions[0].body[0] else {
            panic!("expected VariableDecl");
        };
        assert!(matches!(value, HExpr::Try(_)));
    }
}
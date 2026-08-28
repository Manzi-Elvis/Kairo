use kairo_ast::{EnumDecl, FunctionDecl, Program, StructDecl};
use kairo_lexer::Lexer;
use kairo_parser::Parser;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    Io(String),
    Lex(String),
    Parse(String),
    ModuleNameMismatch { file: String, declared: String },
    CyclicImport(String),
    DuplicateDeclaration { name: String, module_a: String, module_b: String },
    SymbolNotAccessible { name: String, from_module: String, origin_module: String },
}

pub trait ModuleSource {
    fn read_module(&self, name: &str) -> Result<String, LoadError>;
}

impl ModuleSource for HashMap<String, String> {
    fn read_module(&self, name: &str) -> Result<String, LoadError> {
        self.get(name)
            .cloned()
            .ok_or_else(|| LoadError::Io(format!("module `{}` not found", name)))
    }
}

struct LoadedModule {
    name: String,
    imports: Vec<String>,
    structs: Vec<StructDecl>,
    enums: Vec<EnumDecl>,
    functions: Vec<FunctionDecl>,
}

pub fn load_program(entry: &str, source: &dyn ModuleSource) -> Result<Program, LoadError> {
    let mut visited: HashMap<String, LoadedModule> = HashMap::new();
    let mut in_progress: HashSet<String> = HashSet::new();
    load_module(entry, source, &mut visited, &mut in_progress)?;

    check_duplicates(&visited)?;
    check_visibility(&visited)?;

    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut functions = Vec::new();
    for m in visited.into_values() {
        structs.extend(m.structs);
        enums.extend(m.enums);
        functions.extend(m.functions);
    }

    Ok(Program { module_name: None, imports: Vec::new(), structs, enums, functions })
}

fn load_module(
    name: &str,
    source: &dyn ModuleSource,
    visited: &mut HashMap<String, LoadedModule>,
    in_progress: &mut HashSet<String>,
) -> Result<(), LoadError> {
    if visited.contains_key(name) {
        return Ok(());
    }
    if !in_progress.insert(name.to_string()) {
        return Err(LoadError::CyclicImport(name.to_string()));
    }

    let source_text = source.read_module(name)?;
    let tokens = Lexer::new(&source_text)
        .tokenize()
        .map_err(|e| LoadError::Lex(format!("{:?}", e)))?;
    let program = Parser::new(tokens)
        .parse_program()
        .map_err(|e| LoadError::Parse(format!("{:?}", e)))?;

    if let Some(declared) = &program.module_name {
        if declared != name {
            return Err(LoadError::ModuleNameMismatch {
                file: name.to_string(),
                declared: declared.clone(),
            });
        }
    }

    for imp in &program.imports {
        load_module(imp, source, visited, in_progress)?;
    }

    in_progress.remove(name);
    visited.insert(
        name.to_string(),
        LoadedModule {
            name: name.to_string(),
            imports: program.imports,
            structs: program.structs,
            enums: program.enums,
            functions: program.functions,
        },
    );
    Ok(())
}

fn check_duplicates(visited: &HashMap<String, LoadedModule>) -> Result<(), LoadError> {
    let mut seen: HashMap<String, String> = HashMap::new();
    for m in visited.values() {
        let names: Vec<&str> = m.structs.iter().map(|s| s.name.as_str())
            .chain(m.enums.iter().map(|e| e.name.as_str()))
            .chain(m.functions.iter().map(|f| f.name.as_str()))
            .collect();
        for n in names {
            if let Some(existing_module) = seen.get(n) {
                return Err(LoadError::DuplicateDeclaration {
                    name: n.to_string(),
                    module_a: existing_module.clone(),
                    module_b: m.name.clone(),
                });
            }
            seen.insert(n.to_string(), m.name.clone());
        }
    }
    Ok(())
}

struct Checker<'a> {
    fn_origin: &'a HashMap<String, (String, bool)>,
    struct_origin: &'a HashMap<String, (String, bool)>,
    enum_origin: &'a HashMap<String, (String, bool)>,
}

impl<'a> Checker<'a> {
    fn check_accessible(
        &self,
        name: &str,
        current: &LoadedModule,
        origin: &HashMap<String, (String, bool)>,
    ) -> Result<(), LoadError> {
        let Some((origin_module, exported)) = origin.get(name) else { return Ok(()) };
        if origin_module == &current.name {
            return Ok(());
        }
        if *exported && current.imports.contains(origin_module) {
            return Ok(());
        }
        Err(LoadError::SymbolNotAccessible {
            name: name.to_string(),
            from_module: current.name.clone(),
            origin_module: origin_module.clone(),
        })
    }

    fn check_type_name(&self, name: &str, current: &LoadedModule) -> Result<(), LoadError> {
        self.check_accessible(name, current, self.struct_origin)?;
        self.check_accessible(name, current, self.enum_origin)?;
        Ok(())
    }

    fn check_expr(&self, expr: &kairo_ast::Expr, current: &LoadedModule) -> Result<(), LoadError> {
        use kairo_ast::Expr::*;
        match expr {
            StringLiteral(_) | IntLiteral(_) | BoolLiteral(_) | Identifier(_) => Ok(()),
            Binary { left, right, .. } => {
                self.check_expr(left, current)?;
                self.check_expr(right, current)
            }
            Call { callee, args } => {
                self.check_accessible(callee, current, self.fn_origin)?;
                for a in args { self.check_expr(a, current)?; }
                Ok(())
            }
            StructLiteral { name, fields } => {
                self.check_accessible(name, current, self.struct_origin)?;
                for (_, e) in fields { self.check_expr(e, current)?; }
                Ok(())
            }
            FieldAccess { object, .. } => self.check_expr(object, current),
            EnumLiteral { enum_name, fields, .. } => {
                self.check_accessible(enum_name, current, self.enum_origin)?;
                for (_, e) in fields { self.check_expr(e, current)?; }
                Ok(())
            }
            ArrayLiteral(elements) => {
                for e in elements { self.check_expr(e, current)?; }
                Ok(())
            }
            Index { array, index } => {
                self.check_expr(array, current)?;
                self.check_expr(index, current)
            }
            Try(inner) => self.check_expr(inner, current),
        }
    }

    fn check_stmt(&self, stmt: &kairo_ast::Stmt, current: &LoadedModule) -> Result<(), LoadError> {
        use kairo_ast::Stmt::*;
        match stmt {
            VariableDecl { value, .. } => self.check_expr(value, current),
            Assign { value, .. } => self.check_expr(value, current),
            IndexAssign { index, value, .. } => {
                self.check_expr(index, current)?;
                self.check_expr(value, current)
            }
            Expr(e) => self.check_expr(e, current),
            If { condition, then_branch, else_branch } => {
                self.check_expr(condition, current)?;
                for s in then_branch { self.check_stmt(s, current)?; }
                if let Some(else_stmts) = else_branch {
                    for s in else_stmts { self.check_stmt(s, current)?; }
                }
                Ok(())
            }
            While { condition, body } => {
                self.check_expr(condition, current)?;
                for s in body { self.check_stmt(s, current)?; }
                Ok(())
            }
            Return(expr) => {
                if let Some(e) = expr { self.check_expr(e, current)?; }
                Ok(())
            }
            Match { scrutinee, arms } => {
                self.check_expr(scrutinee, current)?;
                for arm in arms {
                    if let kairo_ast::Pattern::EnumVariant { enum_name, .. } = &arm.pattern {
                        self.check_accessible(enum_name, current, self.enum_origin)?;
                    }
                    for s in &arm.body { self.check_stmt(s, current)?; }
                }
                Ok(())
            }
        }
    }
}

fn check_visibility(visited: &HashMap<String, LoadedModule>) -> Result<(), LoadError> {
    let mut struct_origin: HashMap<String, (String, bool)> = HashMap::new();
    let mut enum_origin: HashMap<String, (String, bool)> = HashMap::new();
    let mut fn_origin: HashMap<String, (String, bool)> = HashMap::new();
    for m in visited.values() {
        for s in &m.structs { struct_origin.insert(s.name.clone(), (m.name.clone(), s.is_exported)); }
        for e in &m.enums { enum_origin.insert(e.name.clone(), (m.name.clone(), e.is_exported)); }
        for f in &m.functions { fn_origin.insert(f.name.clone(), (m.name.clone(), f.is_exported)); }
    }

    let checker = Checker { fn_origin: &fn_origin, struct_origin: &struct_origin, enum_origin: &enum_origin };
    for m in visited.values() {
        for f in &m.functions {
            if let Some(rt) = &f.return_type { checker.check_type_name(rt, m)?; }
            for p in &f.params { checker.check_type_name(&p.type_name, m)?; }
            for stmt in &f.body { checker.check_stmt(stmt, m)?; }
        }
        for s in &m.structs {
            for field in &s.fields { checker.check_type_name(&field.type_name, m)?; }
        }
        for e in &m.enums {
            for v in &e.variants {
                for field in &v.fields { checker.check_type_name(&field.type_name, m)?; }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn loads_and_merges_simple_import() {
        let src = sources(&[
            ("geometry", "module geometry\nexport struct Point { x: Int }\nexport fn getX(p: Point) -> Int { return p.x }"),
            ("main", "module main\nimport geometry\nfn main() { p := Point { x: 5 }\nprint(getX(p)) }"),
        ]);
        let program = load_program("main", &src).unwrap();
        assert_eq!(program.structs.len(), 1);
        assert_eq!(program.functions.len(), 2);
    }

    #[test]
    fn detects_module_name_mismatch() {
        let src = sources(&[("foo", "module bar\nfn f() {}")]);
        let err = load_program("foo", &src).unwrap_err();
        assert!(matches!(err, LoadError::ModuleNameMismatch { .. }));
    }

    #[test]
    fn detects_cyclic_import() {
        let src = sources(&[
            ("a", "module a\nimport b\nfn fa() {}"),
            ("b", "module b\nimport a\nfn fb() {}"),
        ]);
        let err = load_program("a", &src).unwrap_err();
        assert!(matches!(err, LoadError::CyclicImport(_)));
    }

    #[test]
    fn detects_duplicate_declaration_across_modules() {
        let src = sources(&[
            ("a", "module a\nimport b\nfn shared() {}"),
            ("b", "module b\nfn shared() {}"),
        ]);
        let err = load_program("a", &src).unwrap_err();
        assert!(matches!(err, LoadError::DuplicateDeclaration { .. }));
    }

    #[test]
    fn blocks_use_of_non_exported_symbol() {
        let src = sources(&[
            ("geometry", "module geometry\nfn helper() -> Int { return 1 }"),
            ("main", "module main\nimport geometry\nfn main() { print(helper()) }"),
        ]);
        let err = load_program("main", &src).unwrap_err();
        assert!(matches!(err, LoadError::SymbolNotAccessible { .. }));
    }

    #[test]
    fn blocks_use_of_symbol_without_direct_import() {
        let src = sources(&[
            ("a", "module a\nexport fn foo() -> Int { return 1 }"),
            ("b", "module b\nimport a\nfn useFoo() -> Int { return foo() }"),
            ("main", "module main\nimport b\nfn main() { print(foo()) }"),
        ]);
        let err = load_program("main", &src).unwrap_err();
        assert!(matches!(err, LoadError::SymbolNotAccessible { .. }));
    }
}
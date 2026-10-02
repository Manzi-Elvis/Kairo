//! Emits textual LLVM IR for a restricted subset of Kairo: Int/Bool/
//! String, struct and enum values (incl. match, which HIR already
//! desugars to IsVariant/VariantField/if-else — no separate match
//! codegen needed), arithmetic, comparisons, user functions (with
//! recursion), if/while, and print. Arrays and ? are rejected with
//! CodegenError::UnsupportedFeature — deferred to later slices.
//!
//! Known limitations of this pass:
//! - Native Int is 32-bit (i32), unlike the interpreter's 64-bit Int.
//! - Division by zero is undefined behavior in native code.
//! - A function whose body doesn't explicitly return on every path
//!   gets a trailing default return rather than the interpreter's
//!   Unit-mismatch behavior.
//! - All heap allocations (Strings, structs, enums) are leaked via
//!   malloc — no free, refcounting, or GC yet.
//! - Struct and enum equality (`==`/`!=`) are not yet implemented:
//!   comparing two heap pointers natively would compare addresses,
//!   not values, which would silently diverge from the
//!   interpreter's structural equality — so both are rejected
//!   outright rather than emitting incorrect IR.
//! - Every enum shares one physical LLVM type (`%__kairo_enum = type
//!   { i32, i8* }`: a tag plus an opaque payload pointer), with a
//!   separate payload struct type per variant that has fields. This
//!   is simpler than per-enum union types and works because the
//!   type checker already guarantees every access is on the correct
//!   enum/variant.

use kairo_ast::{BinaryOp, Param};
use kairo_hir::{HExpr, HFunctionDecl, HProgram, HStmt};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    UnsupportedType(String),
    UnsupportedFeature(String),
    UndefinedVariable(String),
    UndefinedFunction(String),
    NoMainFunction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LType {
    I32,
    I1,
    Str,
    Struct(String),
    Enum(String),
    Void,
}

impl LType {
    fn llvm(&self) -> String {
        match self {
            LType::I32 => "i32".to_string(),
            LType::I1 => "i1".to_string(),
            LType::Str => "i8*".to_string(),
            LType::Struct(name) => format!("%{name}*"),
            LType::Enum(_) => "%__kairo_enum*".to_string(),
            LType::Void => "void".to_string(),
        }
    }

    fn from_name(
        name: &str,
        struct_names: &HashSet<String>,
        enum_names: &HashSet<String>,
    ) -> Result<Self, CodegenError> {
        match name {
            "Int" => Ok(LType::I32),
            "Bool" => Ok(LType::I1),
            "String" => Ok(LType::Str),
            other if struct_names.contains(other) => Ok(LType::Struct(other.to_string())),
            other if enum_names.contains(other) => Ok(LType::Enum(other.to_string())),
            other => Err(CodegenError::UnsupportedType(other.to_string())),
        }
    }
}

struct FnSig {
    params: Vec<LType>,
    ret: LType,
}

type StructTable = HashMap<String, Vec<(String, LType)>>;
/// enum name -> variant list in declaration order (index == tag),
/// each with its field list in declaration order.
type EnumTable = HashMap<String, Vec<(String, Vec<(String, LType)>)>>;

pub fn generate(program: &HProgram) -> Result<String, CodegenError> {
    if !program.functions.iter().any(|f| f.name == "main") {
        return Err(CodegenError::NoMainFunction);
    }

    let struct_names: HashSet<String> = program.structs.iter().map(|s| s.name.clone()).collect();
    let enum_names: HashSet<String> = program.enums.iter().map(|e| e.name.clone()).collect();

    let mut struct_types: StructTable = HashMap::new();
    for s in &program.structs {
        let mut fields = Vec::new();
        for f in &s.fields {
            fields.push((f.name.clone(), LType::from_name(&f.type_name, &struct_names, &enum_names)?));
        }
        struct_types.insert(s.name.clone(), fields);
    }

    let mut enum_types: EnumTable = HashMap::new();
    for e in &program.enums {
        let mut variants = Vec::new();
        for v in &e.variants {
            let mut fields = Vec::new();
            for f in &v.fields {
                fields.push((f.name.clone(), LType::from_name(&f.type_name, &struct_names, &enum_names)?));
            }
            variants.push((v.name.clone(), fields));
        }
        enum_types.insert(e.name.clone(), variants);
    }

    let mut fn_sigs: HashMap<String, FnSig> = HashMap::new();
    for f in &program.functions {
        let params = f
            .params
            .iter()
            .map(|p: &Param| LType::from_name(&p.type_name, &struct_names, &enum_names))
            .collect::<Result<Vec<_>, _>>()?;
        let ret = match &f.return_type {
            Some(t) => LType::from_name(t, &struct_names, &enum_names)?,
            None => LType::Void,
        };
        fn_sigs.insert(f.name.clone(), FnSig { params, ret });
    }

    let mut out = Vec::new();
    out.push("declare i32 @printf(i8*, ...)".to_string());
    out.push("declare i64 @strlen(i8*)".to_string());
    out.push("declare i8* @malloc(i64)".to_string());
    out.push("declare i8* @strcpy(i8*, i8*)".to_string());
    out.push("declare i8* @strcat(i8*, i8*)".to_string());
    out.push("declare i32 @strcmp(i8*, i8*)".to_string());

    for s in &program.structs {
        let field_types: Vec<String> =
            struct_types[&s.name].iter().map(|(_, t)| t.llvm()).collect();
        out.push(format!("%{} = type {{ {} }}", s.name, field_types.join(", ")));
    }

    if !program.enums.is_empty() {
        out.push("%__kairo_enum = type { i32, i8* }".to_string());
        for e in &program.enums {
            for (variant_name, fields) in &enum_types[&e.name] {
                if fields.is_empty() {
                    continue;
                }
                let field_types: Vec<String> = fields.iter().map(|(_, t)| t.llvm()).collect();
                out.push(format!(
                    "%{}_{} = type {{ {} }}",
                    e.name,
                    variant_name,
                    field_types.join(", ")
                ));
            }
        }
    }

    out.push(r#"@.int_fmt = private unnamed_addr constant [4 x i8] c"%d\0A\00""#.to_string());
    out.push(r#"@.str_fmt = private unnamed_addr constant [4 x i8] c"%s\0A\00""#.to_string());
    out.push(r#"@.true_str = private unnamed_addr constant [6 x i8] c"true\0A\00""#.to_string());
    out.push(r#"@.false_str = private unnamed_addr constant [7 x i8] c"false\0A\00""#.to_string());

    let mut string_globals: HashMap<String, (String, usize)> = HashMap::new();
    for (i, s) in collect_string_literals(program).into_iter().enumerate() {
        let label = format!("@.str{}", i + 1);
        let (escaped, len) = llvm_escape_string(&s);
        out.push(format!("{label} = private unnamed_addr constant [{len} x i8] c\"{escaped}\""));
        string_globals.insert(s, (label, len));
    }
    out.push(String::new());

    for f in &program.functions {
        if f.name == "main" {
            continue;
        }
        let sig = fn_sigs.get(&f.name).expect("signature precomputed");
        let mut gen = FnCodegen::new(&fn_sigs, &string_globals, &struct_types, &enum_types);
        out.push(gen.generate_function(f, sig)?);
        out.push(String::new());
    }

    let kairo_main = program.functions.iter().find(|f| f.name == "main").unwrap();
    let main_sig = fn_sigs.get("main").expect("signature precomputed");
    let mut renamed = kairo_main.clone();
    renamed.name = "kairo_main".to_string();
    let mut gen = FnCodegen::new(&fn_sigs, &string_globals, &struct_types, &enum_types);
    out.push(gen.generate_function(&renamed, main_sig)?);
    out.push(String::new());

    out.push("define i32 @main() {".to_string());
    out.push("entry:".to_string());
    out.push("  call void @kairo_main()".to_string());
    out.push("  ret i32 0".to_string());
    out.push("}".to_string());

    Ok(out.join("\n"))
}

fn llvm_escape_string(s: &str) -> (String, usize) {
    let mut out = String::new();
    let mut len = 0;
    for byte in s.bytes() {
        len += 1;
        match byte {
            b'\\' => out.push_str("\\5C"),
            b'"' => out.push_str("\\22"),
            0x20..=0x7E => out.push(byte as char),
            other => out.push_str(&format!("\\{other:02X}")),
        }
    }
    len += 1;
    out.push_str("\\00");
    (out, len)
}

fn collect_string_literals(program: &HProgram) -> Vec<String> {
    let mut found = Vec::new();
    for f in &program.functions {
        collect_in_stmts(&f.body, &mut found);
    }
    found
}

fn collect_in_stmts(stmts: &[HStmt], found: &mut Vec<String>) {
    for s in stmts {
        collect_in_stmt(s, found);
    }
}

fn collect_in_stmt(stmt: &HStmt, found: &mut Vec<String>) {
    match stmt {
        HStmt::VariableDecl { value, .. } => collect_in_expr(value, found),
        HStmt::Assign { value, .. } => collect_in_expr(value, found),
        HStmt::IndexAssign { index, value, .. } => {
            collect_in_expr(index, found);
            collect_in_expr(value, found);
        }
        HStmt::Expr(e) => collect_in_expr(e, found),
        HStmt::If { condition, then_branch, else_branch } => {
            collect_in_expr(condition, found);
            collect_in_stmts(then_branch, found);
            if let Some(b) = else_branch {
                collect_in_stmts(b, found);
            }
        }
        HStmt::While { condition, body } => {
            collect_in_expr(condition, found);
            collect_in_stmts(body, found);
        }
        HStmt::Return(expr) => {
            if let Some(e) = expr {
                collect_in_expr(e, found);
            }
        }
    }
}

fn collect_in_expr(expr: &HExpr, found: &mut Vec<String>) {
    match expr {
        HExpr::StringLiteral(s) => {
            if !found.contains(s) {
                found.push(s.clone());
            }
        }
        HExpr::IntLiteral(_) | HExpr::BoolLiteral(_) | HExpr::Identifier(_) => {}
        HExpr::Binary { left, right, .. } => {
            collect_in_expr(left, found);
            collect_in_expr(right, found);
        }
        HExpr::Call { args, .. } => {
            for a in args {
                collect_in_expr(a, found);
            }
        }
        HExpr::StructLiteral { fields, .. } | HExpr::EnumLiteral { fields, .. } => {
            for (_, e) in fields {
                collect_in_expr(e, found);
            }
        }
        HExpr::FieldAccess { object, .. } => collect_in_expr(object, found),
        HExpr::ArrayLiteral(elements) => {
            for e in elements {
                collect_in_expr(e, found);
            }
        }
        HExpr::Index { array, index } => {
            collect_in_expr(array, found);
            collect_in_expr(index, found);
        }
        HExpr::Try(inner) => collect_in_expr(inner, found),
        HExpr::IsVariant { scrutinee, .. } => collect_in_expr(scrutinee, found),
        HExpr::VariantField { scrutinee, .. } => collect_in_expr(scrutinee, found),
    }
}

struct FnCodegen<'a> {
    fn_sigs: &'a HashMap<String, FnSig>,
    strings: &'a HashMap<String, (String, usize)>,
    struct_types: &'a StructTable,
    enum_types: &'a EnumTable,
    locals: HashMap<String, (String, LType)>,
    lines: Vec<String>,
    temp_counter: usize,
    block_counter: usize,
    terminated: bool,
}

impl<'a> FnCodegen<'a> {
    fn new(
        fn_sigs: &'a HashMap<String, FnSig>,
        strings: &'a HashMap<String, (String, usize)>,
        struct_types: &'a StructTable,
        enum_types: &'a EnumTable,
    ) -> Self {
        Self {
            fn_sigs,
            strings,
            struct_types,
            enum_types,
            locals: HashMap::new(),
            lines: Vec::new(),
            temp_counter: 0,
            block_counter: 0,
            terminated: false,
        }
    }

    fn fresh_temp(&mut self) -> String {
        self.temp_counter += 1;
        format!("%t{}", self.temp_counter)
    }

    fn fresh_idx(&mut self) -> usize {
        self.block_counter += 1;
        self.block_counter
    }

    fn emit(&mut self, line: impl Into<String>) {
        if !self.terminated {
            self.lines.push(line.into());
        }
    }

    fn start_block(&mut self, label: &str) {
        self.lines.push(format!("{label}:"));
        self.terminated = false;
    }

    fn generate_function(&mut self, f: &HFunctionDecl, sig: &FnSig) -> Result<String, CodegenError> {
        if f.name == "kairo_main" && f.return_type.is_some() {
            return Err(CodegenError::UnsupportedFeature(
                "native codegen requires fn main() to have no return type".to_string(),
            ));
        }

        let ret_ty = sig.ret.clone();

        let mut param_decls = Vec::new();
        for (p, ty) in f.params.iter().zip(&sig.params) {
            param_decls.push(format!("{} %arg_{}", ty.llvm(), p.name));
        }

        self.lines.push(format!(
            "define {} @{}({}) {{",
            ret_ty.llvm(),
            f.name,
            param_decls.join(", ")
        ));
        self.lines.push("entry:".to_string());
        self.terminated = false;

        for (p, ty) in f.params.iter().zip(&sig.params) {
            let slot = format!("%{}", p.name);
            self.lines.push(format!("  {slot} = alloca {}", ty.llvm()));
            self.lines
                .push(format!("  store {} %arg_{}, {}* {slot}", ty.llvm(), p.name, ty.llvm()));
            self.locals.insert(p.name.clone(), (slot, ty.clone()));
        }

        self.gen_stmts(&f.body)?;

        if !self.terminated {
            match &ret_ty {
                LType::Void => self.lines.push("  ret void".to_string()),
                LType::I32 => self.lines.push("  ret i32 0".to_string()),
                LType::I1 => self.lines.push("  ret i1 0".to_string()),
                LType::Str => self.lines.push("  ret i8* null".to_string()),
                LType::Struct(name) => self.lines.push(format!("  ret %{name}* null")),
                LType::Enum(_) => self.lines.push("  ret %__kairo_enum* null".to_string()),
            }
        }
        self.lines.push("}".to_string());

        Ok(self.lines.join("\n"))
    }

    fn gen_stmts(&mut self, stmts: &[HStmt]) -> Result<(), CodegenError> {
        for s in stmts {
            if self.terminated {
                break;
            }
            self.gen_stmt(s)?;
        }
        Ok(())
    }

    fn gen_stmt(&mut self, stmt: &HStmt) -> Result<(), CodegenError> {
        match stmt {
            HStmt::VariableDecl { name, value, .. } => {
                let (val, ty) = self.gen_expr(value)?;
                let slot = format!("%{name}");
                self.emit(format!("  {slot} = alloca {}", ty.llvm()));
                self.emit(format!("  store {} {val}, {}* {slot}", ty.llvm(), ty.llvm()));
                self.locals.insert(name.clone(), (slot, ty));
                Ok(())
            }
            HStmt::Assign { name, value } => {
                let (val, ty) = self.gen_expr(value)?;
                let (slot, _) = self
                    .locals
                    .get(name)
                    .cloned()
                    .ok_or_else(|| CodegenError::UndefinedVariable(name.clone()))?;
                self.emit(format!("  store {} {val}, {}* {slot}", ty.llvm(), ty.llvm()));
                Ok(())
            }
            HStmt::IndexAssign { .. } => Err(CodegenError::UnsupportedFeature(
                "arrays are not yet supported in native codegen".to_string(),
            )),
            HStmt::Expr(e) => {
                self.gen_expr(e)?;
                Ok(())
            }
            HStmt::If { condition, then_branch, else_branch } => {
                let (cond_val, _) = self.gen_expr(condition)?;
                let idx = self.fresh_idx();
                let then_label = format!("then{idx}");
                let else_label = format!("else{idx}");
                let merge_label = format!("merge{idx}");

                self.emit(format!("  br i1 {cond_val}, label %{then_label}, label %{else_label}"));

                self.start_block(&then_label);
                self.gen_stmts(then_branch)?;
                let then_terminated = self.terminated;
                if !then_terminated {
                    self.emit(format!("  br label %{merge_label}"));
                }

                self.start_block(&else_label);
                if let Some(else_stmts) = else_branch {
                    self.gen_stmts(else_stmts)?;
                }
                let else_terminated = self.terminated;
                if !else_terminated {
                    self.emit(format!("  br label %{merge_label}"));
                }

                if then_terminated && else_terminated {
                    self.start_block(&merge_label);
                    self.lines.push("  unreachable".to_string());
                    self.terminated = true;
                } else {
                    self.start_block(&merge_label);
                }
                Ok(())
            }
            HStmt::While { condition, body } => {
                let idx = self.fresh_idx();
                let cond_label = format!("whilecond{idx}");
                let body_label = format!("whilebody{idx}");
                let end_label = format!("whileend{idx}");

                self.emit(format!("  br label %{cond_label}"));
                self.start_block(&cond_label);
                let (cond_val, _) = self.gen_expr(condition)?;
                self.emit(format!(
                    "  br i1 {cond_val}, label %{body_label}, label %{end_label}"
                ));

                self.start_block(&body_label);
                self.gen_stmts(body)?;
                if !self.terminated {
                    self.emit(format!("  br label %{cond_label}"));
                }

                self.start_block(&end_label);
                Ok(())
            }
            HStmt::Return(expr) => {
                match expr {
                    Some(e) => {
                        let (val, ty) = self.gen_expr(e)?;
                        self.lines.push(format!("  ret {} {val}", ty.llvm()));
                    }
                    None => self.lines.push("  ret void".to_string()),
                }
                self.terminated = true;
                Ok(())
            }
        }
    }

    fn gen_expr(&mut self, expr: &HExpr) -> Result<(String, LType), CodegenError> {
        match expr {
            HExpr::IntLiteral(v) => Ok((v.to_string(), LType::I32)),
            HExpr::BoolLiteral(b) => Ok((if *b { "1" } else { "0" }.to_string(), LType::I1)),
            HExpr::StringLiteral(s) => {
                let (label, len) = self
                    .strings
                    .get(s)
                    .cloned()
                    .expect("string literal precollected by collect_string_literals");
                let t = self.fresh_temp();
                self.emit(format!(
                    "  {t} = getelementptr [{len} x i8], [{len} x i8]* {label}, i32 0, i32 0"
                ));
                Ok((t, LType::Str))
            }
            HExpr::Identifier(name) => {
                let (slot, ty) = self
                    .locals
                    .get(name)
                    .cloned()
                    .ok_or_else(|| CodegenError::UndefinedVariable(name.clone()))?;
                let t = self.fresh_temp();
                self.emit(format!("  {t} = load {}, {}* {slot}", ty.llvm(), ty.llvm()));
                Ok((t, ty))
            }
            HExpr::Binary { left, op, right } => self.gen_binary(left, *op, right),
            HExpr::Call { callee, args } => self.gen_call(callee, args),
            HExpr::StructLiteral { name, fields } => self.gen_struct_literal(name, fields),
            HExpr::FieldAccess { object, field } => self.gen_field_access(object, field),
            HExpr::EnumLiteral { enum_name, variant, fields } => {
                self.gen_enum_literal(enum_name, variant, fields)
            }
            HExpr::IsVariant { scrutinee, enum_name, variant } => {
                self.gen_is_variant(scrutinee, enum_name, variant)
            }
            HExpr::VariantField { scrutinee, enum_name, variant, field } => {
                self.gen_variant_field(scrutinee, enum_name, variant, field)
            }
            HExpr::ArrayLiteral(_) | HExpr::Index { .. } | HExpr::Try(_) => {
                Err(CodegenError::UnsupportedFeature(
                    "arrays and ? are not yet supported in native codegen".to_string(),
                ))
            }
        }
    }

    fn gen_struct_literal(
        &mut self,
        name: &str,
        fields: &[(String, HExpr)],
    ) -> Result<(String, LType), CodegenError> {
        let field_table = self.struct_types.get(name).cloned().ok_or_else(|| {
            CodegenError::UnsupportedFeature(format!("undefined struct `{}` in native codegen", name))
        })?;

        let size_ptr = self.fresh_temp();
        self.emit(format!("  {size_ptr} = getelementptr %{name}, %{name}* null, i32 1"));
        let size = self.fresh_temp();
        self.emit(format!("  {size} = ptrtoint %{name}* {size_ptr} to i64"));
        let raw = self.fresh_temp();
        self.emit(format!("  {raw} = call i8* @malloc(i64 {size})"));
        let obj = self.fresh_temp();
        self.emit(format!("  {obj} = bitcast i8* {raw} to %{name}*"));

        for (field_name, field_expr) in fields {
            let (val, _) = self.gen_expr(field_expr)?;
            let idx = field_table
                .iter()
                .position(|(n, _)| n == field_name)
                .ok_or_else(|| {
                    CodegenError::UnsupportedFeature(format!(
                        "struct `{}` has no field `{}`",
                        name, field_name
                    ))
                })?;
            let field_ty = &field_table[idx].1;
            let field_ptr = self.fresh_temp();
            self.emit(format!(
                "  {field_ptr} = getelementptr %{name}, %{name}* {obj}, i32 0, i32 {idx}"
            ));
            self.emit(format!(
                "  store {} {val}, {}* {field_ptr}",
                field_ty.llvm(),
                field_ty.llvm()
            ));
        }

        Ok((obj, LType::Struct(name.to_string())))
    }

    fn gen_field_access(
        &mut self,
        object: &HExpr,
        field: &str,
    ) -> Result<(String, LType), CodegenError> {
        let (obj_val, obj_ty) = self.gen_expr(object)?;
        let LType::Struct(struct_name) = obj_ty else {
            return Err(CodegenError::UnsupportedFeature(
                "field access on a non-struct value is not supported in native codegen".to_string(),
            ));
        };
        let field_table = self.struct_types.get(&struct_name).cloned().ok_or_else(|| {
            CodegenError::UnsupportedFeature(format!(
                "undefined struct `{}` in native codegen",
                struct_name
            ))
        })?;
        let idx = field_table
            .iter()
            .position(|(n, _)| n == field)
            .ok_or_else(|| {
                CodegenError::UnsupportedFeature(format!(
                    "struct `{}` has no field `{}`",
                    struct_name, field
                ))
            })?;
        let field_ty = field_table[idx].1.clone();

        let field_ptr = self.fresh_temp();
        self.emit(format!(
            "  {field_ptr} = getelementptr %{struct_name}, %{struct_name}* {obj_val}, i32 0, i32 {idx}"
        ));
        let t = self.fresh_temp();
        self.emit(format!("  {t} = load {}, {}* {field_ptr}", field_ty.llvm(), field_ty.llvm()));
        Ok((t, field_ty))
    }

    /// Looks up a variant's tag index and field list within its enum.
    fn find_variant(
        &self,
        enum_name: &str,
        variant: &str,
    ) -> Result<(usize, Vec<(String, LType)>), CodegenError> {
        let variants = self.enum_types.get(enum_name).ok_or_else(|| {
            CodegenError::UnsupportedFeature(format!("undefined enum `{}` in native codegen", enum_name))
        })?;
        variants
            .iter()
            .position(|(n, _)| n == variant)
            .map(|idx| (idx, variants[idx].1.clone()))
            .ok_or_else(|| {
                CodegenError::UnsupportedFeature(format!(
                    "enum `{}` has no variant `{}`",
                    enum_name, variant
                ))
            })
    }

    fn gen_enum_literal(
        &mut self,
        enum_name: &str,
        variant: &str,
        fields: &[(String, HExpr)],
    ) -> Result<(String, LType), CodegenError> {
        let (tag, field_table) = self.find_variant(enum_name, variant)?;

        // Allocate the shared { i32, i8* } union struct.
        let union_size_ptr = self.fresh_temp();
        self.emit(format!(
            "  {union_size_ptr} = getelementptr %__kairo_enum, %__kairo_enum* null, i32 1"
        ));
        let union_size = self.fresh_temp();
        self.emit(format!("  {union_size} = ptrtoint %__kairo_enum* {union_size_ptr} to i64"));
        let union_raw = self.fresh_temp();
        self.emit(format!("  {union_raw} = call i8* @malloc(i64 {union_size})"));
        let union_obj = self.fresh_temp();
        self.emit(format!("  {union_obj} = bitcast i8* {union_raw} to %__kairo_enum*"));

        let tag_ptr = self.fresh_temp();
        self.emit(format!(
            "  {tag_ptr} = getelementptr %__kairo_enum, %__kairo_enum* {union_obj}, i32 0, i32 0"
        ));
        self.emit(format!("  store i32 {tag}, i32* {tag_ptr}"));

        let payload_ptr_field = self.fresh_temp();
        self.emit(format!(
            "  {payload_ptr_field} = getelementptr %__kairo_enum, %__kairo_enum* {union_obj}, i32 0, i32 1"
        ));

        if field_table.is_empty() {
            self.emit(format!("  store i8* null, i8** {payload_ptr_field}"));
        } else {
            let payload_type = format!("%{enum_name}_{variant}");
            let psize_ptr = self.fresh_temp();
            self.emit(format!(
                "  {psize_ptr} = getelementptr {payload_type}, {payload_type}* null, i32 1"
            ));
            let psize = self.fresh_temp();
            self.emit(format!("  {psize} = ptrtoint {payload_type}* {psize_ptr} to i64"));
            let praw = self.fresh_temp();
            self.emit(format!("  {praw} = call i8* @malloc(i64 {psize})"));
            let payload_obj = self.fresh_temp();
            self.emit(format!("  {payload_obj} = bitcast i8* {praw} to {payload_type}*"));

            for (field_name, field_expr) in fields {
                let (val, _) = self.gen_expr(field_expr)?;
                let idx = field_table
                    .iter()
                    .position(|(n, _)| n == field_name)
                    .ok_or_else(|| {
                        CodegenError::UnsupportedFeature(format!(
                            "variant `{}::{}` has no field `{}`",
                            enum_name, variant, field_name
                        ))
                    })?;
                let field_ty = &field_table[idx].1;
                let fptr = self.fresh_temp();
                self.emit(format!(
                    "  {fptr} = getelementptr {payload_type}, {payload_type}* {payload_obj}, i32 0, i32 {idx}"
                ));
                self.emit(format!("  store {} {val}, {}* {fptr}", field_ty.llvm(), field_ty.llvm()));
            }

            self.emit(format!("  store i8* {praw}, i8** {payload_ptr_field}"));
        }

        Ok((union_obj, LType::Enum(enum_name.to_string())))
    }

    fn gen_is_variant(
        &mut self,
        scrutinee: &HExpr,
        enum_name: &str,
        variant: &str,
    ) -> Result<(String, LType), CodegenError> {
        let (tag, _) = self.find_variant(enum_name, variant)?;
        let (obj, _) = self.gen_expr(scrutinee)?;

        let tag_ptr = self.fresh_temp();
        self.emit(format!(
            "  {tag_ptr} = getelementptr %__kairo_enum, %__kairo_enum* {obj}, i32 0, i32 0"
        ));
        let loaded_tag = self.fresh_temp();
        self.emit(format!("  {loaded_tag} = load i32, i32* {tag_ptr}"));
        let t = self.fresh_temp();
        self.emit(format!("  {t} = icmp eq i32 {loaded_tag}, {tag}"));
        Ok((t, LType::I1))
    }

    fn gen_variant_field(
        &mut self,
        scrutinee: &HExpr,
        enum_name: &str,
        variant: &str,
        field: &str,
    ) -> Result<(String, LType), CodegenError> {
        let (_, field_table) = self.find_variant(enum_name, variant)?;
        let idx = field_table
            .iter()
            .position(|(n, _)| n == field)
            .ok_or_else(|| {
                CodegenError::UnsupportedFeature(format!(
                    "variant `{}::{}` has no field `{}`",
                    enum_name, variant, field
                ))
            })?;
        let field_ty = field_table[idx].1.clone();
        let payload_type = format!("%{enum_name}_{variant}");

        let (obj, _) = self.gen_expr(scrutinee)?;
        let payload_ptr_field = self.fresh_temp();
        self.emit(format!(
            "  {payload_ptr_field} = getelementptr %__kairo_enum, %__kairo_enum* {obj}, i32 0, i32 1"
        ));
        let payload_raw = self.fresh_temp();
        self.emit(format!("  {payload_raw} = load i8*, i8** {payload_ptr_field}"));
        let payload_obj = self.fresh_temp();
        self.emit(format!("  {payload_obj} = bitcast i8* {payload_raw} to {payload_type}*"));

        let fptr = self.fresh_temp();
        self.emit(format!(
            "  {fptr} = getelementptr {payload_type}, {payload_type}* {payload_obj}, i32 0, i32 {idx}"
        ));
        let t = self.fresh_temp();
        self.emit(format!("  {t} = load {}, {}* {fptr}", field_ty.llvm(), field_ty.llvm()));
        Ok((t, field_ty))
    }

    fn gen_binary(
        &mut self,
        left: &HExpr,
        op: BinaryOp,
        right: &HExpr,
    ) -> Result<(String, LType), CodegenError> {
        let (l, lty) = self.gen_expr(left)?;
        let (r, _) = self.gen_expr(right)?;

        use BinaryOp::*;
        match op {
            Eq | NotEq if matches!(lty, LType::Struct(_) | LType::Enum(_)) => {
                Err(CodegenError::UnsupportedFeature(
                    "struct/enum equality is not yet supported in native codegen".to_string(),
                ))
            }
            Add if lty == LType::Str => {
                let len_a = self.fresh_temp();
                self.emit(format!("  {len_a} = call i64 @strlen(i8* {l})"));
                let len_b = self.fresh_temp();
                self.emit(format!("  {len_b} = call i64 @strlen(i8* {r})"));
                let total = self.fresh_temp();
                self.emit(format!("  {total} = add i64 {len_a}, {len_b}"));
                let total1 = self.fresh_temp();
                self.emit(format!("  {total1} = add i64 {total}, 1"));
                let buf = self.fresh_temp();
                self.emit(format!("  {buf} = call i8* @malloc(i64 {total1})"));
                let copy_res = self.fresh_temp();
                self.emit(format!("  {copy_res} = call i8* @strcpy(i8* {buf}, i8* {l})"));
                let cat_res = self.fresh_temp();
                self.emit(format!("  {cat_res} = call i8* @strcat(i8* {buf}, i8* {r})"));
                Ok((buf, LType::Str))
            }
            Eq if lty == LType::Str => {
                let cmp = self.fresh_temp();
                self.emit(format!("  {cmp} = call i32 @strcmp(i8* {l}, i8* {r})"));
                let t = self.fresh_temp();
                self.emit(format!("  {t} = icmp eq i32 {cmp}, 0"));
                Ok((t, LType::I1))
            }
            NotEq if lty == LType::Str => {
                let cmp = self.fresh_temp();
                self.emit(format!("  {cmp} = call i32 @strcmp(i8* {l}, i8* {r})"));
                let t = self.fresh_temp();
                self.emit(format!("  {t} = icmp ne i32 {cmp}, 0"));
                Ok((t, LType::I1))
            }
            _ => {
                let t = self.fresh_temp();
                let (instr, result_ty) = match op {
                    Add => (format!("add i32 {l}, {r}"), LType::I32),
                    Sub => (format!("sub i32 {l}, {r}"), LType::I32),
                    Mul => (format!("mul i32 {l}, {r}"), LType::I32),
                    Div => (format!("sdiv i32 {l}, {r}"), LType::I32),
                    Lt => (format!("icmp slt i32 {l}, {r}"), LType::I1),
                    Gt => (format!("icmp sgt i32 {l}, {r}"), LType::I1),
                    Le => (format!("icmp sle i32 {l}, {r}"), LType::I1),
                    Ge => (format!("icmp sge i32 {l}, {r}"), LType::I1),
                    Eq => {
                        let ty = if lty == LType::I1 { "i1" } else { "i32" };
                        (format!("icmp eq {ty} {l}, {r}"), LType::I1)
                    }
                    NotEq => {
                        let ty = if lty == LType::I1 { "i1" } else { "i32" };
                        (format!("icmp ne {ty} {l}, {r}"), LType::I1)
                    }
                };
                self.emit(format!("  {t} = {instr}"));
                Ok((t, result_ty))
            }
        }
    }

    fn gen_call(&mut self, callee: &str, args: &[HExpr]) -> Result<(String, LType), CodegenError> {
        if callee == "print" {
            if args.len() != 1 {
                return Err(CodegenError::UnsupportedFeature(
                    "print expects exactly 1 argument".to_string(),
                ));
            }
            let (val, ty) = self.gen_expr(&args[0])?;
            match ty {
                LType::I32 => {
                    let fmt_ptr = self.fresh_temp();
                    self.emit(format!(
                        "  {fmt_ptr} = getelementptr [4 x i8], [4 x i8]* @.int_fmt, i32 0, i32 0"
                    ));
                    let t = self.fresh_temp();
                    self.emit(format!(
                        "  {t} = call i32 (i8*, ...) @printf(i8* {fmt_ptr}, i32 {val})"
                    ));
                }
                LType::I1 => {
                    let idx = self.fresh_idx();
                    let true_label = format!("printtrue{idx}");
                    let false_label = format!("printfalse{idx}");
                    let end_label = format!("printend{idx}");
                    self.emit(format!(
                        "  br i1 {val}, label %{true_label}, label %{false_label}"
                    ));

                    self.start_block(&true_label);
                    let tp = self.fresh_temp();
                    self.emit(format!(
                        "  {tp} = getelementptr [6 x i8], [6 x i8]* @.true_str, i32 0, i32 0"
                    ));
                    self.emit(format!("  call i32 (i8*, ...) @printf(i8* {tp})"));
                    self.emit(format!("  br label %{end_label}"));

                    self.start_block(&false_label);
                    let fp = self.fresh_temp();
                    self.emit(format!(
                        "  {fp} = getelementptr [7 x i8], [7 x i8]* @.false_str, i32 0, i32 0"
                    ));
                    self.emit(format!("  call i32 (i8*, ...) @printf(i8* {fp})"));
                    self.emit(format!("  br label %{end_label}"));

                    self.start_block(&end_label);
                }
                LType::Str => {
                    let fmt_ptr = self.fresh_temp();
                    self.emit(format!(
                        "  {fmt_ptr} = getelementptr [4 x i8], [4 x i8]* @.str_fmt, i32 0, i32 0"
                    ));
                    let t = self.fresh_temp();
                    self.emit(format!(
                        "  {t} = call i32 (i8*, ...) @printf(i8* {fmt_ptr}, i8* {val})"
                    ));
                }
                LType::Struct(_) | LType::Enum(_) => {
                    return Err(CodegenError::UnsupportedFeature(
                        "print of struct/enum values is not yet supported in native codegen"
                            .to_string(),
                    ));
                }
                LType::Void => unreachable!("print argument cannot be void"),
            }
            return Ok(("0".to_string(), LType::Void));
        }

        let sig = self
            .fn_sigs
            .get(callee)
            .ok_or_else(|| CodegenError::UndefinedFunction(callee.to_string()))?;
        let ret_ty = sig.ret.clone();

        let mut arg_strs = Vec::new();
        for a in args {
            let (val, ty) = self.gen_expr(a)?;
            arg_strs.push(format!("{} {val}", ty.llvm()));
        }

        let call_expr = format!("call {} @{}({})", ret_ty.llvm(), callee, arg_strs.join(", "));
        if ret_ty == LType::Void {
            self.emit(format!("  {call_expr}"));
            Ok(("0".to_string(), LType::Void))
        } else {
            let t = self.fresh_temp();
            self.emit(format!("  {t} = {call_expr}"));
            Ok((t, ret_ty))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_hir::lower_program;
    use kairo_lexer::Lexer;
    use kairo_parser::Parser;

    fn gen_source(source: &str) -> Result<String, CodegenError> {
        let tokens = Lexer::new(source).tokenize().expect("lex failed");
        let ast = Parser::new(tokens).parse_program().expect("parse failed");
        let hir = lower_program(&ast);
        generate(&hir)
    }

    #[test]
    fn generates_int_arithmetic_and_print() {
        let ir = gen_source("fn main() { x := 2 + 3 * 4\nprint(x) }").unwrap();
        assert!(ir.contains("define void @kairo_main()"));
        assert!(ir.contains("mul i32"));
        assert!(ir.contains("add i32"));
        assert!(ir.contains("call i32 (i8*, ...) @printf"));
        assert!(ir.contains("define i32 @main()"));
    }

    #[test]
    fn generates_bool_print_branch() {
        let ir = gen_source("fn main() { print(1 < 2) }").unwrap();
        assert!(ir.contains("icmp slt i32"));
        assert!(ir.contains("@.true_str"));
        assert!(ir.contains("@.false_str"));
    }

    #[test]
    fn generates_if_else_branches() {
        let ir = gen_source("fn main() { if 1 < 2 { print(1) } else { print(2) } }").unwrap();
        assert!(ir.contains("br i1"));
        assert!(ir.contains("then1:"));
        assert!(ir.contains("else1:"));
        assert!(ir.contains("merge1:"));
    }

    #[test]
    fn generates_while_loop() {
        let source = r#"
            fn main() {
                mut i := 0
                while i < 3 {
                    print(i)
                    i = i + 1
                }
            }
        "#;
        let ir = gen_source(source).unwrap();
        assert!(ir.contains("whilecond1"));
        assert!(ir.contains("whilebody1"));
        assert!(ir.contains("whileend1"));
    }

    #[test]
    fn generates_recursive_function_call() {
        let source = r#"
            fn fib(n: Int) -> Int {
                if n < 2 { return n }
                return fib(n - 1) + fib(n - 2)
            }
            fn main() { print(fib(10)) }
        "#;
        let ir = gen_source(source).unwrap();
        assert!(ir.contains("define i32 @fib(i32 %arg_n)"));
        assert!(ir.contains("call i32 @fib("));
    }

    #[test]
    fn rejects_missing_main() {
        let err = gen_source("fn notMain() {}").unwrap_err();
        assert_eq!(err, CodegenError::NoMainFunction);
    }

    #[test]
    fn generates_string_literal_and_print() {
        let ir = gen_source(r#"fn main() { print("hi") }"#).unwrap();
        assert!(ir.contains("@.str1 = private unnamed_addr constant"));
        assert!(ir.contains("@.str_fmt"));
        assert!(ir.contains("call i32 (i8*, ...) @printf(i8*"));
    }

    #[test]
    fn dedupes_repeated_string_literals() {
        let source = "fn main() { print(\"hi\")\nprint(\"hi\") }";
        let ir = gen_source(source).unwrap();
        assert!(ir.contains("@.str1"));
        assert!(!ir.contains("@.str2"));
    }

    #[test]
    fn generates_string_concatenation() {
        let source = "fn main() { x := \"a\" + \"b\"\nprint(x) }";
        let ir = gen_source(source).unwrap();
        assert!(ir.contains("call i64 @strlen"));
        assert!(ir.contains("call i8* @malloc"));
        assert!(ir.contains("call i8* @strcpy"));
        assert!(ir.contains("call i8* @strcat"));
    }

    #[test]
    fn generates_string_equality_via_strcmp() {
        let ir = gen_source(r#"fn main() { print("a" == "b") }"#).unwrap();
        assert!(ir.contains("call i32 @strcmp"));
        assert!(ir.contains("icmp eq i32"));
    }

    #[test]
    fn generates_struct_construction_and_field_access() {
        let source = r#"
            struct Point { x: Int, y: Int }
            fn main() {
                p := Point { x: 3, y: 4 }
                print(p.x)
            }
        "#;
        let ir = gen_source(source).unwrap();
        assert!(ir.contains("%Point = type { i32, i32 }"));
        assert!(ir.contains("call i8* @malloc"));
        assert!(ir.contains("getelementptr %Point, %Point* null, i32 1"));
        assert!(ir.contains("getelementptr %Point, %Point*"));
    }

    #[test]
    fn rejects_struct_equality() {
        let source = r#"
            struct Point { x: Int }
            fn main() {
                a := Point { x: 1 }
                b := Point { x: 1 }
                print(a == b)
            }
        "#;
        let err = gen_source(source).unwrap_err();
        assert!(matches!(err, CodegenError::UnsupportedFeature(_)));
    }

    #[test]
    fn generates_enum_construction_and_match() {
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
        let ir = gen_source(source).unwrap();
        assert!(ir.contains("%__kairo_enum = type { i32, i8* }"));
        assert!(ir.contains("%Status_Failed = type { i8* }"));
        assert!(ir.contains("getelementptr %__kairo_enum, %__kairo_enum*"));
        assert!(ir.contains("icmp eq i32"));
        assert!(ir.contains("bitcast i8* {{payload}}".replace("{{payload}}", "").as_str()) || ir.contains("bitcast i8*"));
    }

    #[test]
    fn generates_unit_variant_with_null_payload() {
        let source = r#"
            enum Status { Pending, Done }
            fn main() {
                s := Status::Pending
                print(1)
            }
        "#;
        let ir = gen_source(source).unwrap();
        assert!(ir.contains("store i8* null, i8**"));
    }

    #[test]
    fn rejects_enum_equality() {
        let source = r#"
            enum Status { Pending }
            fn main() {
                a := Status::Pending
                b := Status::Pending
                print(a == b)
            }
        "#;
        let err = gen_source(source).unwrap_err();
        assert!(matches!(err, CodegenError::UnsupportedFeature(_)));
    }
}
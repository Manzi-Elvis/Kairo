//! Emits textual LLVM IR for a restricted subset of Kairo: Int/Bool/
//! String, structs, enums (incl. match, via HIR's IsVariant/
//! VariantField desugaring), arrays, ?, arithmetic, comparisons,
//! user functions (with recursion), if/while, and print. Modules
//! are the one remaining gap (the CLI wiring doesn't yet route a
//! loader-merged multi-file Program into codegen).
//!
//! Known limitations of this pass:
//! - Native Int is 32-bit (i32), unlike the interpreter's 64-bit Int.
//! - Division by zero is undefined behavior in native code.
//! - A function whose body doesn't explicitly return on every path
//!   gets a trailing default return rather than the interpreter's
//!   Unit-mismatch behavior.
//! - All heap allocations (Strings, structs, enums, arrays) are
//!   leaked via malloc — no free, refcounting, or GC yet.
//! - Struct, enum, and array equality (`==`/`!=`) are not yet
//!   implemented: comparing heap pointers natively would compare
//!   addresses, not values, diverging from the interpreter's
//!   structural equality — so all three are rejected outright.
//! - Every enum shares one physical LLVM type (tag + opaque payload
//!   pointer); every array shares one physical LLVM type (length +
//!   opaque element-buffer pointer). Out-of-bounds array access
//!   prints a message and calls `exit(1)` rather than invoking
//!   undefined behavior.
//! - `?` early-returns the whole Err enum value directly, relying on
//!   every enum sharing the same physical LLVM type — no cast is
//!   needed even though the function's declared return type differs
//!   by Kairo-level enum name, since the type checker already
//!   guarantees it's the same enum as the `?`'d expression's type.

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
    Array(Box<LType>),
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
            LType::Array(_) => "%__kairo_array*".to_string(),
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
    out.push("declare i8* @memcpy(i8*, i8*, i64)".to_string());
    out.push("declare void @exit(i32)".to_string());

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

    out.push("%__kairo_array = type { i64, i8* }".to_string());

    out.push(r#"@.int_fmt = private unnamed_addr constant [4 x i8] c"%d\0A\00""#.to_string());
    out.push(r#"@.str_fmt = private unnamed_addr constant [4 x i8] c"%s\0A\00""#.to_string());
    out.push(r#"@.true_str = private unnamed_addr constant [6 x i8] c"true\0A\00""#.to_string());
    out.push(r#"@.false_str = private unnamed_addr constant [7 x i8] c"false\0A\00""#.to_string());
    let (oob_escaped, oob_len) = llvm_escape_string("index out of bounds\n");
    out.push(format!(
        "@.oob_msg = private unnamed_addr constant [{oob_len} x i8] c\"{oob_escaped}\""
    ));

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
        let mut gen = FnCodegen::new(&fn_sigs, &string_globals, &struct_types, &enum_types, oob_len);
        out.push(gen.generate_function(f, sig)?);
        out.push(String::new());
    }

    let kairo_main = program.functions.iter().find(|f| f.name == "main").unwrap();
    let main_sig = fn_sigs.get("main").expect("signature precomputed");
    let mut renamed = kairo_main.clone();
    renamed.name = "kairo_main".to_string();
    let mut gen = FnCodegen::new(&fn_sigs, &string_globals, &struct_types, &enum_types, oob_len);
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
    oob_len: usize,
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
        oob_len: usize,
    ) -> Self {
        Self {
            fn_sigs,
            strings,
            struct_types,
            enum_types,
            oob_len,
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

    fn emit_sizeof(&mut self, llvm_ty: &str) -> String {
        let size_ptr = self.fresh_temp();
        self.emit(format!("  {size_ptr} = getelementptr {llvm_ty}, {llvm_ty}* null, i32 1"));
        let size = self.fresh_temp();
        self.emit(format!("  {size} = ptrtoint {llvm_ty}* {size_ptr} to i64"));
        size
    }

    fn emit_bounds_check(&mut self, idx64: &str, len64: &str) {
        let inb = self.fresh_temp();
        self.emit(format!("  {inb} = icmp ult i64 {idx64}, {len64}"));
        let idx = self.fresh_idx();
        let ok_label = format!("idxok{idx}");
        let oob_label = format!("idxoob{idx}");
        self.emit(format!("  br i1 {inb}, label %{ok_label}, label %{oob_label}"));

        self.start_block(&oob_label);
        let msgp = self.fresh_temp();
        let len = self.oob_len;
        self.emit(format!(
            "  {msgp} = getelementptr [{len} x i8], [{len} x i8]* @.oob_msg, i32 0, i32 0"
        ));
        self.emit(format!("  call i32 (i8*, ...) @printf(i8* {msgp})"));
        self.emit("  call void @exit(i32 1)".to_string());
        self.lines.push("  unreachable".to_string());
        self.terminated = true;

        self.start_block(&ok_label);
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
                LType::Array(_) => self.lines.push("  ret %__kairo_array* null".to_string()),
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
            HStmt::IndexAssign { name, index, value } => {
                let (slot, arr_ty) = self
                    .locals
                    .get(name)
                    .cloned()
                    .ok_or_else(|| CodegenError::UndefinedVariable(name.clone()))?;
                let LType::Array(elem_ty) = arr_ty else {
                    return Err(CodegenError::UnsupportedFeature(
                        "index assignment on a non-array value".to_string(),
                    ));
                };

                let arr_val = self.fresh_temp();
                self.emit(format!(
                    "  {arr_val} = load %__kairo_array*, %__kairo_array** {slot}"
                ));

                let (idx_val, _) = self.gen_expr(index)?;
                let idx64 = self.fresh_temp();
                self.emit(format!("  {idx64} = sext i32 {idx_val} to i64"));

                let len_ptr = self.fresh_temp();
                self.emit(format!(
                    "  {len_ptr} = getelementptr %__kairo_array, %__kairo_array* {arr_val}, i32 0, i32 0"
                ));
                let len64 = self.fresh_temp();
                self.emit(format!("  {len64} = load i64, i64* {len_ptr}"));

                self.emit_bounds_check(&idx64, &len64);

                let data_field = self.fresh_temp();
                self.emit(format!(
                    "  {data_field} = getelementptr %__kairo_array, %__kairo_array* {arr_val}, i32 0, i32 1"
                ));
                let raw = self.fresh_temp();
                self.emit(format!("  {raw} = load i8*, i8** {data_field}"));
                let elem_llvm = elem_ty.llvm();
                let typed = self.fresh_temp();
                self.emit(format!("  {typed} = bitcast i8* {raw} to {elem_llvm}*"));
                let elem_ptr = self.fresh_temp();
                self.emit(format!(
                    "  {elem_ptr} = getelementptr {elem_llvm}, {elem_llvm}* {typed}, i64 {idx64}"
                ));

                let (val, _) = self.gen_expr(value)?;
                self.emit(format!("  store {elem_llvm} {val}, {elem_llvm}* {elem_ptr}"));
                Ok(())
            }
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
            HExpr::ArrayLiteral(elements) => self.gen_array_literal(elements),
            HExpr::Index { array, index } => self.gen_index(array, index),
            HExpr::Try(inner) => self.gen_try(inner),
        }
    }

    /// `?`: evaluate inner (must be an enum value), branch on whether
    /// its tag is `Ok`. If not Ok, return the whole enum value
    /// unchanged from the current function immediately (no cast
    /// needed — every enum shares one physical LLVM type). If Ok,
    /// extract and continue with its `value` field.
    fn gen_try(&mut self, inner: &HExpr) -> Result<(String, LType), CodegenError> {
        let (obj, ty) = self.gen_expr(inner)?;
        let LType::Enum(enum_name) = ty else {
            return Err(CodegenError::UnsupportedFeature(
                "? requires an enum value".to_string(),
            ));
        };
        let (ok_tag, ok_fields) = self.find_variant(&enum_name, "Ok")?;
        self.find_variant(&enum_name, "Err")?; // just confirm it exists

        let value_idx = ok_fields.iter().position(|(n, _)| n == "value").ok_or_else(|| {
            CodegenError::UnsupportedFeature(format!(
                "enum `{}` variant `Ok` has no `value` field",
                enum_name
            ))
        })?;
        let value_ty = ok_fields[value_idx].1.clone();

        let tag_ptr = self.fresh_temp();
        self.emit(format!(
            "  {tag_ptr} = getelementptr %__kairo_enum, %__kairo_enum* {obj}, i32 0, i32 0"
        ));
        let loaded_tag = self.fresh_temp();
        self.emit(format!("  {loaded_tag} = load i32, i32* {tag_ptr}"));
        let is_ok = self.fresh_temp();
        self.emit(format!("  {is_ok} = icmp eq i32 {loaded_tag}, {ok_tag}"));

        let idx = self.fresh_idx();
        let ok_label = format!("tryok{idx}");
        let err_label = format!("tryerr{idx}");
        self.emit(format!("  br i1 {is_ok}, label %{ok_label}, label %{err_label}"));

        self.start_block(&err_label);
        self.lines.push(format!("  ret %__kairo_enum* {obj}"));
        self.terminated = true;

        self.start_block(&ok_label);
        let payload_type = format!("%{enum_name}_Ok");
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
            "  {fptr} = getelementptr {payload_type}, {payload_type}* {payload_obj}, i32 0, i32 {value_idx}"
        ));
        let t = self.fresh_temp();
        self.emit(format!("  {t} = load {}, {}* {fptr}", value_ty.llvm(), value_ty.llvm()));
        Ok((t, value_ty))
    }

    fn gen_array_literal(&mut self, elements: &[HExpr]) -> Result<(String, LType), CodegenError> {
        if elements.is_empty() {
            return Err(CodegenError::UnsupportedFeature(
                "empty array literals are not yet supported in native codegen".to_string(),
            ));
        }

        let mut values = Vec::with_capacity(elements.len());
        let mut elem_ty: Option<LType> = None;
        for e in elements {
            let (val, ty) = self.gen_expr(e)?;
            elem_ty.get_or_insert_with(|| ty.clone());
            values.push(val);
        }
        let elem_ty = elem_ty.unwrap();
        let elem_llvm = elem_ty.llvm();
        let count = values.len() as i64;

        let elemsize = self.emit_sizeof(&elem_llvm);
        let total_bytes = self.fresh_temp();
        self.emit(format!("  {total_bytes} = mul i64 {count}, {elemsize}"));
        let buf_raw = self.fresh_temp();
        self.emit(format!("  {buf_raw} = call i8* @malloc(i64 {total_bytes})"));
        let buf_typed = self.fresh_temp();
        self.emit(format!("  {buf_typed} = bitcast i8* {buf_raw} to {elem_llvm}*"));

        for (i, val) in values.iter().enumerate() {
            let elem_ptr = self.fresh_temp();
            self.emit(format!(
                "  {elem_ptr} = getelementptr {elem_llvm}, {elem_llvm}* {buf_typed}, i64 {i}"
            ));
            self.emit(format!("  store {elem_llvm} {val}, {elem_llvm}* {elem_ptr}"));
        }

        let arr_size = self.emit_sizeof("%__kairo_array");
        let arr_raw = self.fresh_temp();
        self.emit(format!("  {arr_raw} = call i8* @malloc(i64 {arr_size})"));
        let arr_obj = self.fresh_temp();
        self.emit(format!("  {arr_obj} = bitcast i8* {arr_raw} to %__kairo_array*"));

        let len_ptr = self.fresh_temp();
        self.emit(format!(
            "  {len_ptr} = getelementptr %__kairo_array, %__kairo_array* {arr_obj}, i32 0, i32 0"
        ));
        self.emit(format!("  store i64 {count}, i64* {len_ptr}"));
        let data_ptr = self.fresh_temp();
        self.emit(format!(
            "  {data_ptr} = getelementptr %__kairo_array, %__kairo_array* {arr_obj}, i32 0, i32 1"
        ));
        self.emit(format!("  store i8* {buf_raw}, i8** {data_ptr}"));

        Ok((arr_obj, LType::Array(Box::new(elem_ty))))
    }

    fn gen_index(&mut self, array: &HExpr, index: &HExpr) -> Result<(String, LType), CodegenError> {
        let (arr_val, arr_ty) = self.gen_expr(array)?;
        let LType::Array(elem_ty) = arr_ty else {
            return Err(CodegenError::UnsupportedFeature(
                "indexing a non-array value".to_string(),
            ));
        };

        let (idx_val, _) = self.gen_expr(index)?;
        let idx64 = self.fresh_temp();
        self.emit(format!("  {idx64} = sext i32 {idx_val} to i64"));

        let len_ptr = self.fresh_temp();
        self.emit(format!(
            "  {len_ptr} = getelementptr %__kairo_array, %__kairo_array* {arr_val}, i32 0, i32 0"
        ));
        let len64 = self.fresh_temp();
        self.emit(format!("  {len64} = load i64, i64* {len_ptr}"));

        self.emit_bounds_check(&idx64, &len64);

        let data_field = self.fresh_temp();
        self.emit(format!(
            "  {data_field} = getelementptr %__kairo_array, %__kairo_array* {arr_val}, i32 0, i32 1"
        ));
        let raw = self.fresh_temp();
        self.emit(format!("  {raw} = load i8*, i8** {data_field}"));
        let elem_llvm = elem_ty.llvm();
        let typed = self.fresh_temp();
        self.emit(format!("  {typed} = bitcast i8* {raw} to {elem_llvm}*"));
        let elem_ptr = self.fresh_temp();
        self.emit(format!(
            "  {elem_ptr} = getelementptr {elem_llvm}, {elem_llvm}* {typed}, i64 {idx64}"
        ));
        let t = self.fresh_temp();
        self.emit(format!("  {t} = load {elem_llvm}, {elem_llvm}* {elem_ptr}"));
        Ok((t, *elem_ty))
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
            Eq | NotEq
                if matches!(lty, LType::Struct(_) | LType::Enum(_) | LType::Array(_)) =>
            {
                Err(CodegenError::UnsupportedFeature(
                    "struct/enum/array equality is not yet supported in native codegen".to_string(),
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
                LType::Struct(_) | LType::Enum(_) | LType::Array(_) => {
                    return Err(CodegenError::UnsupportedFeature(
                        "print of struct/enum/array values is not yet supported in native codegen"
                            .to_string(),
                    ));
                }
                LType::Void => unreachable!("print argument cannot be void"),
            }
            return Ok(("0".to_string(), LType::Void));
        }

        if callee == "len" {
            if args.len() != 1 {
                return Err(CodegenError::UnsupportedFeature(
                    "len expects exactly 1 argument".to_string(),
                ));
            }
            let (arr_val, arr_ty) = self.gen_expr(&args[0])?;
            if !matches!(arr_ty, LType::Array(_)) {
                return Err(CodegenError::UnsupportedFeature(
                    "len expects an array".to_string(),
                ));
            }
            let len_ptr = self.fresh_temp();
            self.emit(format!(
                "  {len_ptr} = getelementptr %__kairo_array, %__kairo_array* {arr_val}, i32 0, i32 0"
            ));
            let len64 = self.fresh_temp();
            self.emit(format!("  {len64} = load i64, i64* {len_ptr}"));
            let len32 = self.fresh_temp();
            self.emit(format!("  {len32} = trunc i64 {len64} to i32"));
            return Ok((len32, LType::I32));
        }

        if callee == "push" {
            if args.len() != 2 {
                return Err(CodegenError::UnsupportedFeature(
                    "push expects exactly 2 arguments".to_string(),
                ));
            }
            let (arr_val, arr_ty) = self.gen_expr(&args[0])?;
            let LType::Array(elem_ty) = arr_ty else {
                return Err(CodegenError::UnsupportedFeature(
                    "push expects an array as its first argument".to_string(),
                ));
            };
            let (item_val, _) = self.gen_expr(&args[1])?;
            let elem_llvm = elem_ty.llvm();

            let len_ptr = self.fresh_temp();
            self.emit(format!(
                "  {len_ptr} = getelementptr %__kairo_array, %__kairo_array* {arr_val}, i32 0, i32 0"
            ));
            let old_len = self.fresh_temp();
            self.emit(format!("  {old_len} = load i64, i64* {len_ptr}"));
            let data_ptr_field = self.fresh_temp();
            self.emit(format!(
                "  {data_ptr_field} = getelementptr %__kairo_array, %__kairo_array* {arr_val}, i32 0, i32 1"
            ));
            let old_raw = self.fresh_temp();
            self.emit(format!("  {old_raw} = load i8*, i8** {data_ptr_field}"));

            let elemsize = self.emit_sizeof(&elem_llvm);
            let old_bytes = self.fresh_temp();
            self.emit(format!("  {old_bytes} = mul i64 {old_len}, {elemsize}"));
            let new_len = self.fresh_temp();
            self.emit(format!("  {new_len} = add i64 {old_len}, 1"));
            let new_bytes = self.fresh_temp();
            self.emit(format!("  {new_bytes} = mul i64 {new_len}, {elemsize}"));

            let new_raw = self.fresh_temp();
            self.emit(format!("  {new_raw} = call i8* @malloc(i64 {new_bytes})"));
            let memcpy_res = self.fresh_temp();
            self.emit(format!(
                "  {memcpy_res} = call i8* @memcpy(i8* {new_raw}, i8* {old_raw}, i64 {old_bytes})"
            ));

            let new_typed = self.fresh_temp();
            self.emit(format!("  {new_typed} = bitcast i8* {new_raw} to {elem_llvm}*"));
            let new_elem_ptr = self.fresh_temp();
            self.emit(format!(
                "  {new_elem_ptr} = getelementptr {elem_llvm}, {elem_llvm}* {new_typed}, i64 {old_len}"
            ));
            self.emit(format!("  store {elem_llvm} {item_val}, {elem_llvm}* {new_elem_ptr}"));

            let new_arr_size = self.emit_sizeof("%__kairo_array");
            let new_arr_raw = self.fresh_temp();
            self.emit(format!("  {new_arr_raw} = call i8* @malloc(i64 {new_arr_size})"));
            let new_arr_obj = self.fresh_temp();
            self.emit(format!("  {new_arr_obj} = bitcast i8* {new_arr_raw} to %__kairo_array*"));
            let new_len_ptr = self.fresh_temp();
            self.emit(format!(
                "  {new_len_ptr} = getelementptr %__kairo_array, %__kairo_array* {new_arr_obj}, i32 0, i32 0"
            ));
            self.emit(format!("  store i64 {new_len}, i64* {new_len_ptr}"));
            let new_data_ptr = self.fresh_temp();
            self.emit(format!(
                "  {new_data_ptr} = getelementptr %__kairo_array, %__kairo_array* {new_arr_obj}, i32 0, i32 1"
            ));
            self.emit(format!("  store i8* {new_raw}, i8** {new_data_ptr}"));

            return Ok((new_arr_obj, LType::Array(elem_ty)));
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
        assert!(ir.contains("call i32 (i8*, ...) @printf"));
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
        assert!(ir.contains("call i32 @fib("));
    }

    #[test]
    fn rejects_missing_main() {
        let err = gen_source("fn notMain() {}").unwrap_err();
        assert_eq!(err, CodegenError::NoMainFunction);
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
    }

    #[test]
    fn generates_array_literal_and_index() {
        let source = "fn main() { a := [1, 2, 3]\nprint(a[1]) }";
        let ir = gen_source(source).unwrap();
        assert!(ir.contains("%__kairo_array = type { i64, i8* }"));
        assert!(ir.contains("@.oob_msg"));
    }

    #[test]
    fn generates_push_builtin() {
        let source = "fn main() { a := [1, 2]\nb := push(a, 3)\nprint(len(b)) }";
        let ir = gen_source(source).unwrap();
        assert!(ir.contains("call i8* @memcpy"));
    }

    #[test]
    fn generates_try_operator_branches() {
        let source = r#"
            enum IntResult { Ok(value: Int), Err(error: String) }
            fn make() -> IntResult { return IntResult::Ok(value: 5) }
            fn compute() -> IntResult {
                x := make()?
                return IntResult::Ok(value: x + 1)
            }
            fn main() {}
        "#;
        let ir = gen_source(source).unwrap();
        assert!(ir.contains("tryok1:"));
        assert!(ir.contains("tryerr1:"));
        assert!(ir.contains("ret %__kairo_enum*"));
        assert!(ir.contains("%IntResult_Ok = type { i32 }"));
    }

    #[test]
    fn rejects_try_on_non_enum() {
        let source = "fn f() -> Int { x := 5?\nreturn x }\nfn main() {}";
        let err = gen_source(source).unwrap_err();
        assert!(matches!(err, CodegenError::UnsupportedFeature(_)));
    }
}
//! Signatures of Kairo's standard-library functions that have a fixed
//! (monomorphic) type. Polymorphic ones (`print`, `len`, `push`,
//! `toString`) are special-cased in the type checker, interpreter and
//! codegen instead.
//!
//! A name may appear in several rows (overloads): the type checker and
//! codegen pick the row whose parameter types match the arguments.
//!
//! Adding a function: add a row here, implement it in the interpreter
//! (compiler/interpreter/src/stdlib.rs) and in the native runtime
//! (compiler/codegen/runtime/kairo_runtime.c) with identical behavior.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BType {
    Int,
    Float,
    Bool,
    Str,
    StrArray,
    /// No value. Only valid as a return type.
    Unit,
}

#[derive(Debug)]
pub struct Builtin {
    pub name: &'static str,
    pub params: &'static [BType],
    pub ret: BType,
    /// Name of the C function implementing it in the native runtime.
    pub c_symbol: &'static str,
}

pub const BUILTINS: &[Builtin] = &[
    // input and conversion
    Builtin { name: "input", params: &[], ret: BType::Str, c_symbol: "kairo_input" },
    Builtin { name: "toInt", params: &[BType::Str], ret: BType::Int, c_symbol: "kairo_to_int" },
    Builtin { name: "toInt", params: &[BType::Float], ret: BType::Int, c_symbol: "kairo_float_to_int" },
    Builtin { name: "isInt", params: &[BType::Str], ret: BType::Bool, c_symbol: "kairo_is_int" },
    Builtin { name: "toFloat", params: &[BType::Int], ret: BType::Float, c_symbol: "kairo_int_to_float" },
    Builtin { name: "toFloat", params: &[BType::Str], ret: BType::Float, c_symbol: "kairo_to_float" },
    Builtin { name: "isFloat", params: &[BType::Str], ret: BType::Bool, c_symbol: "kairo_is_float" },
    // strings
    Builtin { name: "substring", params: &[BType::Str, BType::Int, BType::Int], ret: BType::Str, c_symbol: "kairo_substring" },
    Builtin { name: "contains", params: &[BType::Str, BType::Str], ret: BType::Bool, c_symbol: "kairo_contains" },
    Builtin { name: "indexOf", params: &[BType::Str, BType::Str], ret: BType::Int, c_symbol: "kairo_index_of" },
    Builtin { name: "startsWith", params: &[BType::Str, BType::Str], ret: BType::Bool, c_symbol: "kairo_starts_with" },
    Builtin { name: "endsWith", params: &[BType::Str, BType::Str], ret: BType::Bool, c_symbol: "kairo_ends_with" },
    Builtin { name: "trim", params: &[BType::Str], ret: BType::Str, c_symbol: "kairo_trim" },
    Builtin { name: "toUpper", params: &[BType::Str], ret: BType::Str, c_symbol: "kairo_to_upper" },
    Builtin { name: "toLower", params: &[BType::Str], ret: BType::Str, c_symbol: "kairo_to_lower" },
    Builtin { name: "replace", params: &[BType::Str, BType::Str, BType::Str], ret: BType::Str, c_symbol: "kairo_replace" },
    Builtin { name: "split", params: &[BType::Str, BType::Str], ret: BType::StrArray, c_symbol: "kairo_split" },
    Builtin { name: "join", params: &[BType::StrArray, BType::Str], ret: BType::Str, c_symbol: "kairo_join" },
    // files, arguments, exit
    Builtin { name: "readFile", params: &[BType::Str], ret: BType::Str, c_symbol: "kairo_read_file" },
    Builtin { name: "writeFile", params: &[BType::Str, BType::Str], ret: BType::Unit, c_symbol: "kairo_write_file" },
    Builtin { name: "appendFile", params: &[BType::Str, BType::Str], ret: BType::Unit, c_symbol: "kairo_append_file" },
    Builtin { name: "fileExists", params: &[BType::Str], ret: BType::Bool, c_symbol: "kairo_file_exists" },
    Builtin { name: "deleteFile", params: &[BType::Str], ret: BType::Unit, c_symbol: "kairo_delete_file" },
    Builtin { name: "args", params: &[], ret: BType::StrArray, c_symbol: "kairo_args" },
    Builtin { name: "exit", params: &[BType::Int], ret: BType::Unit, c_symbol: "kairo_exit" },
    // math
    Builtin { name: "sqrt", params: &[BType::Float], ret: BType::Float, c_symbol: "kairo_sqrt" },
    Builtin { name: "pow", params: &[BType::Float, BType::Float], ret: BType::Float, c_symbol: "kairo_pow" },
    Builtin { name: "floor", params: &[BType::Float], ret: BType::Float, c_symbol: "kairo_floor" },
    Builtin { name: "ceil", params: &[BType::Float], ret: BType::Float, c_symbol: "kairo_ceil" },
    Builtin { name: "round", params: &[BType::Float], ret: BType::Float, c_symbol: "kairo_round" },
    Builtin { name: "abs", params: &[BType::Int], ret: BType::Int, c_symbol: "kairo_abs_int" },
    Builtin { name: "abs", params: &[BType::Float], ret: BType::Float, c_symbol: "kairo_abs_float" },
    Builtin { name: "min", params: &[BType::Int, BType::Int], ret: BType::Int, c_symbol: "kairo_min_int" },
    Builtin { name: "min", params: &[BType::Float, BType::Float], ret: BType::Float, c_symbol: "kairo_min_float" },
    Builtin { name: "max", params: &[BType::Int, BType::Int], ret: BType::Int, c_symbol: "kairo_max_int" },
    Builtin { name: "max", params: &[BType::Float, BType::Float], ret: BType::Float, c_symbol: "kairo_max_float" },
    Builtin { name: "sin", params: &[BType::Float], ret: BType::Float, c_symbol: "kairo_sin" },
    Builtin { name: "cos", params: &[BType::Float], ret: BType::Float, c_symbol: "kairo_cos" },
    Builtin { name: "tan", params: &[BType::Float], ret: BType::Float, c_symbol: "kairo_tan" },
    Builtin { name: "exp", params: &[BType::Float], ret: BType::Float, c_symbol: "kairo_exp" },
    Builtin { name: "log", params: &[BType::Float], ret: BType::Float, c_symbol: "kairo_log" },
    Builtin { name: "atan2", params: &[BType::Float, BType::Float], ret: BType::Float, c_symbol: "kairo_atan2" },
    Builtin { name: "pi", params: &[], ret: BType::Float, c_symbol: "kairo_pi" },
];

/// The first row with this name.
pub fn lookup(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|b| b.name == name)
}

pub fn is_builtin(name: &str) -> bool {
    BUILTINS.iter().any(|b| b.name == name)
}

/// All rows with this name (its overloads).
pub fn candidates(name: &str) -> impl Iterator<Item = &'static Builtin> + '_ {
    BUILTINS.iter().filter(move |b| b.name == name)
}

/// The overload whose parameter types are exactly `args`.
pub fn resolve(name: &str, args: &[BType]) -> Option<&'static Builtin> {
    candidates(name).find(|b| b.params == args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_are_unique() {
        let mut seen = Vec::new();
        for b in BUILTINS {
            let key = (b.name, b.params);
            assert!(!seen.contains(&key), "duplicate signature for {}", b.name);
            seen.push(key);
        }
    }

    #[test]
    fn finds_known_builtins_only() {
        assert_eq!(lookup("toInt").unwrap().ret, BType::Int);
        assert!(lookup("nope").is_none());
    }

    #[test]
    fn c_symbols_are_unique_and_namespaced() {
        let mut seen = std::collections::HashSet::new();
        for b in BUILTINS {
            assert!(b.c_symbol.starts_with("kairo_"));
            assert!(seen.insert(b.c_symbol), "duplicate symbol {}", b.c_symbol);
        }
    }

    #[test]
    fn resolves_overloads_by_argument_types() {
        assert_eq!(resolve("toInt", &[BType::Str]).unwrap().c_symbol, "kairo_to_int");
        assert_eq!(resolve("toInt", &[BType::Float]).unwrap().c_symbol, "kairo_float_to_int");
        assert!(resolve("toInt", &[BType::Bool]).is_none());
    }

    #[test]
    fn resolves_math_overloads() {
        assert_eq!(resolve("abs", &[BType::Int]).unwrap().c_symbol, "kairo_abs_int");
        assert_eq!(resolve("abs", &[BType::Float]).unwrap().c_symbol, "kairo_abs_float");
        assert_eq!(
            resolve("max", &[BType::Float, BType::Float]).unwrap().c_symbol,
            "kairo_max_float"
        );
        assert!(resolve("max", &[BType::Int, BType::Float]).is_none());
    }
}
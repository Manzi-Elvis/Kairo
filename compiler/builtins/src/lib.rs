//! Signatures of Kairo's standard-library functions that have a fixed
//! (monomorphic) type. Polymorphic ones (`print`, `len`, `push`,
//! `toString`) are special-cased in the type checker, interpreter and
//! codegen instead.
//!
//! Adding a function: add a row here, implement it in the interpreter
//! (compiler/interpreter/src/stdlib.rs) and in the native runtime
//! (compiler/codegen/runtime/kairo_runtime.c) with identical behavior.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BType {
    Int,
    Bool,
    Str,
    StrArray,
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
    Builtin { name: "input", params: &[], ret: BType::Str, c_symbol: "kairo_input" },
    Builtin { name: "toInt", params: &[BType::Str], ret: BType::Int, c_symbol: "kairo_to_int" },
    Builtin { name: "isInt", params: &[BType::Str], ret: BType::Bool, c_symbol: "kairo_is_int" },
];

pub fn lookup(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|b| b.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for b in BUILTINS {
            assert!(seen.insert(b.name), "duplicate builtin {}", b.name);
        }
    }

    #[test]
    fn finds_known_builtins_only() {
        assert_eq!(lookup("toInt").unwrap().ret, BType::Int);
        assert!(lookup("nope").is_none());
    }
}
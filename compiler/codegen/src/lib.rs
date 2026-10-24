pub mod codegen;
pub use codegen::{generate, CodegenError};
/// C source of the native runtime, compiled together with every program.
pub const RUNTIME_C: &str = include_str!("../runtime/kairo_runtime.c");
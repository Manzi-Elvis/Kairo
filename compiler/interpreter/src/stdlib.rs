//! Interpreter implementations of the functions listed in
//! `kairo_builtins`. Behavior must match the native runtime
//! (compiler/codegen/runtime/kairo_runtime.c) exactly.

use crate::interpreter::RuntimeError;
use crate::value::Value;
use std::io::BufRead;

pub fn call(name: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
    match (name, args.as_slice()) {
        ("input", []) => Ok(Value::String(read_line())),
        ("toInt", [Value::String(s)]) => match s.parse::<i64>() {
            Ok(n) => Ok(Value::Int(n)),
            Err(_) => Err(RuntimeError::StdlibError(format!("invalid integer: \"{s}\""))),
        },
        ("isInt", [Value::String(s)]) => Ok(Value::Bool(s.parse::<i64>().is_ok())),
        _ => Err(RuntimeError::TypeError(format!("bad arguments to `{name}`"))),
    }
}

/// One line from stdin without its line ending; "" at end of input.
fn read_line() -> String {
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    if line.ends_with('\n') {
        line.pop();
    }
    if line.ends_with('\r') {
        line.pop();
    }
    line
}
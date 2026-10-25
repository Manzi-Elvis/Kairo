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
        ("substring", [Value::String(s), Value::Int(start), Value::Int(end)]) => {
            let len = s.len() as i64;
            if *start < 0 || *start > len {
                return Err(RuntimeError::IndexOutOfBounds(*start));
            }
            if *end < *start || *end > len {
                return Err(RuntimeError::IndexOutOfBounds(*end));
            }
            let bytes = &s.as_bytes()[*start as usize..*end as usize];
            String::from_utf8(bytes.to_vec()).map(Value::String).map_err(|_| {
                RuntimeError::StdlibError("substring splits a multi-byte character".to_string())
            })
        }
        ("contains", [Value::String(s), Value::String(part)]) => {
            Ok(Value::Bool(s.contains(part.as_str())))
        }
        ("indexOf", [Value::String(s), Value::String(part)]) => {
            Ok(Value::Int(s.find(part.as_str()).map_or(-1, |i| i as i64)))
        }
        ("startsWith", [Value::String(s), Value::String(part)]) => {
            Ok(Value::Bool(s.starts_with(part.as_str())))
        }
        ("endsWith", [Value::String(s), Value::String(part)]) => {
            Ok(Value::Bool(s.ends_with(part.as_str())))
        }
        ("trim", [Value::String(s)]) => Ok(Value::String(s.trim_matches(is_space).to_string())),
        ("toUpper", [Value::String(s)]) => Ok(Value::String(s.to_ascii_uppercase())),
        ("toLower", [Value::String(s)]) => Ok(Value::String(s.to_ascii_lowercase())),
        ("replace", [Value::String(s), Value::String(from), Value::String(to)]) => {
            if from.is_empty() {
                return Err(RuntimeError::StdlibError("replace: empty search string".to_string()));
            }
            Ok(Value::String(s.replace(from.as_str(), to.as_str())))
        }
        ("split", [Value::String(s), Value::String(sep)]) => {
            if sep.is_empty() {
                return Err(RuntimeError::StdlibError("split: empty separator".to_string()));
            }
            Ok(Value::Array(
                s.split(sep.as_str()).map(|p| Value::String(p.to_string())).collect(),
            ))
        }
        ("join", [Value::Array(items), Value::String(sep)]) => {
            let mut parts = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(p) => parts.push(p.as_str()),
                    other => {
                        return Err(RuntimeError::TypeError(format!(
                            "join expects an array of strings, found {}",
                            other.type_name()
                        )))
                    }
                }
            }
            Ok(Value::String(parts.join(sep.as_str())))
        }
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

/// The six ASCII whitespace characters `trim` strips (same set as the
/// native runtime).
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}
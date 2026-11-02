//! Interpreter implementations of the functions listed in
//! `kairo_builtins`. Behavior must match the native runtime
//! (compiler/codegen/runtime/kairo_runtime.c) exactly.

use crate::interpreter::RuntimeError;
use crate::value::Value;
use std::io::{BufRead, Write};

pub fn call(name: &str, args: Vec<Value>, program_args: &[String]) -> Result<Value, RuntimeError> {
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
        ("readFile", [Value::String(path)]) => {
            let bytes = std::fs::read(path)
                .map_err(|_| RuntimeError::StdlibError(format!("cannot read file: {path}")))?;
            if bytes.contains(&0) {
                return Err(RuntimeError::StdlibError(format!(
                    "file contains a NUL byte: {path}"
                )));
            }
            String::from_utf8(bytes).map(Value::String).map_err(|_| {
                RuntimeError::StdlibError(format!("file is not valid UTF-8: {path}"))
            })
        }
        ("writeFile", [Value::String(path), Value::String(text)]) => std::fs::write(path, text)
            .map(|_| Value::Unit)
            .map_err(|_| RuntimeError::StdlibError(format!("cannot write file: {path}"))),
        ("appendFile", [Value::String(path), Value::String(text)]) => {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut f| f.write_all(text.as_bytes()))
                .map(|_| Value::Unit)
                .map_err(|_| RuntimeError::StdlibError(format!("cannot write file: {path}")))
        }
        ("fileExists", [Value::String(path)]) => {
            Ok(Value::Bool(std::fs::File::open(path).is_ok()))
        }
        ("deleteFile", [Value::String(path)]) => std::fs::remove_file(path)
            .map(|_| Value::Unit)
            .map_err(|_| RuntimeError::StdlibError(format!("cannot delete file: {path}"))),
        ("args", []) => Ok(Value::Array(
            program_args.iter().map(|a| Value::String(a.clone())).collect(),
        )),
        ("exit", [Value::Int(code)]) => Err(RuntimeError::Exit(*code as i32)),
        ("toInt", [Value::Float(x)]) => {
            if x.is_nan() || *x >= 9223372036854775808.0 || *x < -9223372036854775808.0 {
                Err(RuntimeError::StdlibError("cannot convert Float to Int".to_string()))
            } else {
                Ok(Value::Int(*x as i64))
            }
        }
        ("toFloat", [Value::Int(n)]) => Ok(Value::Float(*n as f64)),
        ("toFloat", [Value::String(s)]) => {
            if valid_float(s) {
                Ok(Value::Float(s.parse::<f64>().unwrap_or(f64::NAN)))
            } else {
                Err(RuntimeError::StdlibError(format!("invalid float: \"{s}\"")))
            }
        }
        ("isFloat", [Value::String(s)]) => Ok(Value::Bool(valid_float(s))),
        ("sqrt", [Value::Float(x)]) => Ok(Value::Float(x.sqrt())),
        ("pow", [Value::Float(x), Value::Float(y)]) => Ok(Value::Float(x.powf(*y))),
        ("floor", [Value::Float(x)]) => Ok(Value::Float(x.floor())),
        ("ceil", [Value::Float(x)]) => Ok(Value::Float(x.ceil())),
        ("round", [Value::Float(x)]) => Ok(Value::Float(x.round())),
        ("abs", [Value::Int(n)]) => Ok(Value::Int(n.wrapping_abs())),
        ("abs", [Value::Float(x)]) => Ok(Value::Float(x.abs())),
        ("min", [Value::Int(a), Value::Int(b)]) => Ok(Value::Int(if a < b { *a } else { *b })),
        ("min", [Value::Float(a), Value::Float(b)]) => {
            Ok(Value::Float(if a < b { *a } else { *b }))
        }
        ("max", [Value::Int(a), Value::Int(b)]) => Ok(Value::Int(if a > b { *a } else { *b })),
        ("max", [Value::Float(a), Value::Float(b)]) => {
            Ok(Value::Float(if a > b { *a } else { *b }))
        }
        ("sin", [Value::Float(x)]) => Ok(Value::Float(x.sin())),
        ("cos", [Value::Float(x)]) => Ok(Value::Float(x.cos())),
        ("tan", [Value::Float(x)]) => Ok(Value::Float(x.tan())),
        ("exp", [Value::Float(x)]) => Ok(Value::Float(x.exp())),
        ("log", [Value::Float(x)]) => Ok(Value::Float(x.ln())),
        ("atan2", [Value::Float(y), Value::Float(x)]) => Ok(Value::Float(y.atan2(*x))),
        ("pi", []) => Ok(Value::Float(std::f64::consts::PI)),
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

/// `[+-] digits [. digits] [(e|E) [+-] digits]` (same grammar as the
/// native runtime's valid_float).
fn valid_float(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - start
    };
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    if digits(&mut i) == 0 {
        return false;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        if digits(&mut i) == 0 {
            return false;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return false;
        }
    }
    i == b.len()
}
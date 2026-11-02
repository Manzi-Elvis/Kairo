use std::collections::HashMap;

/// Runtime values for the v0.6 milestone interpreter.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Int(i64),
    Bool(bool),
    Float(f64),
    /// The result of a function with no return statement, or a bare
    /// `return`. Not user-constructible from source syntax yet.
    Unit,
    Struct {
        name: String,
        fields: HashMap<String, Value>,
    },
    Array(Vec<Value>),
    Enum {
        enum_name: String,
        variant: String,
        fields: HashMap<String, Value>,
    },
}

impl Value {
    /// How this value is rendered when passed to `print`.
    pub fn display(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Int(i) => i.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Unit => String::new(),
            Value::Float(x) => format_float(*x),
            Value::Struct { name, fields } => {
                // HashMap iteration order isn't stable, so sort field
                // names for deterministic, testable output.
                let mut names: Vec<&String> = fields.keys().collect();
                names.sort();
                let parts: Vec<String> = names
                    .iter()
                    .map(|n| format!("{}: {}", n, fields[*n].display()))
                    .collect();
                format!("{} {{ {} }}", name, parts.join(", "))
            }
            Value::Array(items) => {
                let parts: Vec<String> = items.iter().map(|v| v.display()).collect();
                format!("[{}]", parts.join(", "))
            }
            Value::Enum {
                enum_name,
                variant,
                fields,
            } => {
                if fields.is_empty() {
                    format!("{}::{}", enum_name, variant)
                } else {
                    let mut names: Vec<&String> = fields.keys().collect();
                    names.sort();
                    let parts: Vec<String> = names
                        .iter()
                        .map(|n| format!("{}: {}", n, fields[*n].display()))
                        .collect();
                    format!("{}::{}({})", enum_name, variant, parts.join(", "))
                }
            }
        }
    }

    /// A short name used in type-error messages.
    pub fn type_name(&self) -> String {
        match self {
            Value::String(_) => "String".to_string(),
            Value::Int(_) => "Int".to_string(),
            Value::Bool(_) => "Bool".to_string(),
            Value::Unit => "Unit".to_string(),
            Value::Struct { name, .. } => name.clone(),
            Value::Enum { enum_name, .. } => enum_name.clone(),
            Value::Array(_) => "Array".to_string(),
            Value::Float(_) => "Float".to_string(),
        }
    }
}

/// Floats print with up to 15 significant digits, exactly like C's
/// printf("%.15g"). The native runtime (kairo_float_to_string) does the
/// same, so both backends print identical text.
pub fn format_float(x: f64) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let sign = if x.is_sign_negative() { "-" } else { "" };
    let sci = format!("{:.14e}", x.abs()); // e.g. "1.23456789012346e2"
    let (mantissa, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let all_digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let trimmed = all_digits.trim_end_matches('0');
    let digits = if trimmed.is_empty() { "0" } else { trimmed };

    let mut out = String::from(sign);
    if exp < -4 || exp >= 15 {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if exp < 0 { '-' } else { '+' });
        out.push_str(&format!("{:02}", exp.abs()));
    } else if exp >= 0 {
        let int_len = exp as usize + 1;
        if digits.len() <= int_len {
            out.push_str(digits);
            out.push_str(&"0".repeat(int_len - digits.len()));
        } else {
            out.push_str(&digits[..int_len]);
            out.push('.');
            out.push_str(&digits[int_len..]);
        }
    } else {
        out.push_str("0.");
        out.push_str(&"0".repeat((-exp - 1) as usize));
        out.push_str(digits);
    }
    out
}
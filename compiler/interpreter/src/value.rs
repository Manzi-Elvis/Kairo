use std::collections::HashMap;

/// Runtime values for the v0.6 milestone interpreter.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Int(i64),
    Bool(bool),
    /// The result of a function with no return statement, or a bare
    /// `return`. Not user-constructible from source syntax yet.
    Unit,
    Struct {
        name: String,
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
        }
    }
}
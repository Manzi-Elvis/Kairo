/// Runtime values for the v0.2 milestone interpreter.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Int(i64),
    Bool(bool),
}

impl Value {
    /// How this value is rendered when passed to `print`.
    pub fn display(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Int(i) => i.to_string(),
            Value::Bool(b) => b.to_string(),
        }
    }

    /// A short name used in type-error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::String(_) => "String",
            Value::Int(_) => "Int",
            Value::Bool(_) => "Bool",
        }
    }
}
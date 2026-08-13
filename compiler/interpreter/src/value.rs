/// Runtime values for the v0.5 milestone interpreter.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Int(i64),
    Bool(bool),
    /// The result of a function with no return statement, or a bare
    /// `return`. Not user-constructible from source syntax yet.
    Unit,
}

impl Value {
    /// How this value is rendered when passed to `print`.
    pub fn display(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Int(i) => i.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Unit => String::new(),
        }
    }

    /// A short name used in type-error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::String(_) => "String",
            Value::Int(_) => "Int",
            Value::Bool(_) => "Bool",
            Value::Unit => "Unit",
        }
    }
}
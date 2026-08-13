/// Runtime values for the v0.1 milestone interpreter.
///
/// Only strings exist for now — enough to run the hello-world
/// program. More variants (Int, Bool, ...) arrive with the type
/// system in Phase 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    String(String),
}

impl Value {
    /// How this value is rendered when passed to `print`.
    pub fn display(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
        }
    }
}
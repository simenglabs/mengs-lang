/// Data types in the Meng language.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Type {
    Str,
    Int,
    Float,
    Bool,
}

impl Type {
    /// Parse a type name as written in Meng source (`int`, `str`, ...).
    pub fn parse(name: &str) -> Option<Type> {
        match name {
            "str" => Some(Type::Str),
            "int" => Some(Type::Int),
            "float" => Some(Type::Float),
            "bool" => Some(Type::Bool),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Type::Str => "str",
            Type::Int => "int",
            Type::Float => "float",
            Type::Bool => "bool",
        }
    }

    pub fn is_numeric(self) -> bool {
        matches!(self, Type::Int | Type::Float)
    }

    /// Whether a value of type `value` can be stored in a slot of type `self`.
    /// The only implicit conversion is int -> float.
    pub fn accepts(self, value: Type) -> bool {
        self == value || (self == Type::Float && value == Type::Int)
    }
}

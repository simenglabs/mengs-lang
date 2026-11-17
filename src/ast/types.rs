/// Data types in the Meng language.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Type {
    Str,
    Int,
    Float,
    Bool,
    /// `[int]`, `[str]`, ...: a growable array of a non-array type.
    Array(&'static Type),
}

impl Type {
    /// Parse a type name as written in Meng source (`int`, `str`, `[int]`, ...).
    pub fn parse(name: &str) -> Option<Type> {
        if let Some(inner) = name.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            return Type::parse(inner.trim()).and_then(Type::array_of);
        }
        match name {
            "str" => Some(Type::Str),
            "int" => Some(Type::Int),
            "float" => Some(Type::Float),
            "bool" => Some(Type::Bool),
            _ => None,
        }
    }

    /// The array type with this element type (arrays cannot be nested).
    pub fn array_of(elem: Type) -> Option<Type> {
        match elem {
            Type::Str => Some(Type::Array(&Type::Str)),
            Type::Int => Some(Type::Array(&Type::Int)),
            Type::Float => Some(Type::Array(&Type::Float)),
            Type::Bool => Some(Type::Array(&Type::Bool)),
            Type::Array(_) => None,
        }
    }

    /// Element type of an array.
    pub fn elem(self) -> Option<Type> {
        match self {
            Type::Array(e) => Some(*e),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Type::Str => "str",
            Type::Int => "int",
            Type::Float => "float",
            Type::Bool => "bool",
            Type::Array(Type::Str) => "[str]",
            Type::Array(Type::Int) => "[int]",
            Type::Array(Type::Float) => "[float]",
            Type::Array(_) => "[bool]",
        }
    }

    /// The name with its article, for messages: "an int", "a str".
    pub fn with_article(self) -> &'static str {
        match self {
            Type::Str => "a str",
            Type::Int => "an int",
            Type::Float => "a float",
            Type::Bool => "a bool",
            Type::Array(Type::Str) => "a [str]",
            Type::Array(Type::Int) => "an [int]",
            Type::Array(Type::Float) => "a [float]",
            Type::Array(_) => "a [bool]",
        }
    }

    pub fn is_numeric(self) -> bool {
        matches!(self, Type::Int | Type::Float)
    }

    /// Values that live on the heap and are owned (moved, freed): `str` and arrays.
    pub fn is_heap(self) -> bool {
        matches!(self, Type::Str | Type::Array(_))
    }

    /// Whether a value of type `value` can be stored in a slot of type `self`.
    /// The only implicit conversion is int -> float.
    pub fn accepts(self, value: Type) -> bool {
        self == value || (self == Type::Float && value == Type::Int)
    }
}

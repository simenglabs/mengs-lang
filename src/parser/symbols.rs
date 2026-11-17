//! Symbols for editor tooling (`meng symbols`): functions, variables with the
//! lines where they are visible, and ownership events (moves, reassigns).

use crate::ast::{Expr, Signature};

use super::scope::OwnershipEvent;

#[derive(Debug, Default)]
pub struct Symbols {
    pub functions: Vec<FunctionSymbol>,
    pub variables: Vec<VariableSymbol>,
    pub events: Vec<OwnershipEvent>,
}

#[derive(Debug)]
pub struct FunctionSymbol {
    pub sig: Signature,
    pub line: usize,
    pub column: usize,
    /// Line of the closing `<-;`.
    pub end: usize,
}

#[derive(Debug)]
pub struct VariableSymbol {
    pub name: String,
    pub ty: &'static str,
    /// `owned`, `keep`, `reuse`, `borrow`, or `param`.
    pub kind: &'static str,
    pub line: usize,
    pub column: usize,
    /// Last line where the variable is visible (end of its block).
    pub end: usize,
    /// Enclosing function, `None` at the top level.
    pub function: Option<String>,
}

impl Symbols {
    pub fn to_json(&self) -> String {
        let functions: Vec<String> = self.functions.iter().map(function_json).collect();
        let variables: Vec<String> = self
            .variables
            .iter()
            .map(|v| {
                format!(
                    r#"{{"name":{},"type":{},"kind":{},"line":{},"column":{},"end":{},"function":{}}}"#,
                    quote(&v.name),
                    quote(v.ty),
                    quote(v.kind),
                    v.line,
                    v.column,
                    v.end,
                    v.function.as_deref().map_or("null".to_string(), quote)
                )
            })
            .collect();
        let events: Vec<String> = self
            .events
            .iter()
            .map(|e| format!(r#"{{"name":{},"line":{},"moved":{}}}"#, quote(&e.name), e.line, e.moved))
            .collect();
        format!(
            "{{\"functions\":[{}],\"variables\":[{}],\"events\":[{}]}}\n",
            functions.join(","),
            variables.join(","),
            events.join(",")
        )
    }
}

fn function_json(f: &FunctionSymbol) -> String {
    let params: Vec<String> = f
        .sig
        .params
        .iter()
        .map(|p| {
            let kind = if p.keep { "keep" } else if p.reuse { "reuse" } else { "param" };
            let default = p.default.as_ref().map_or("null".to_string(), |d| quote(&literal_text(d)));
            format!(r#"{{"name":{},"type":{},"kind":{},"default":{}}}"#, quote(&p.name), quote(p.ty.name()), quote(kind), default)
        })
        .collect();
    format!(
        r#"{{"name":{},"line":{},"column":{},"end":{},"signature":{},"returns":{},"params":[{}]}}"#,
        quote(&f.sig.name),
        f.line,
        f.column,
        f.end,
        quote(&signature_text(&f.sig)),
        f.sig.ret.map_or("null".to_string(), |t| quote(t.name())),
        params.join(",")
    )
}

/// `fnc Name(keep a: int = 0, b: str): int`
pub fn signature_text(sig: &Signature) -> String {
    let params: Vec<String> = sig
        .params
        .iter()
        .map(|p| {
            let prefix = if p.keep { "keep " } else if p.reuse { "reuse " } else { "" };
            let default = p.default.as_ref().map_or(String::new(), |d| format!(" = {}", literal_text(d)));
            format!("{}{}: {}{}", prefix, p.name, p.ty.name(), default)
        })
        .collect();
    let ret = sig.ret.map_or(String::new(), |t| format!(": {}", t.name()));
    format!("fnc {}({}){}", sig.name, params.join(", "), ret)
}

/// A literal as written in Meng source.
fn literal_text(e: &Expr) -> String {
    match e {
        Expr::Str(s) => format!("{:?}", s),
        Expr::Int(n) => n.to_string(),
        Expr::Float(x) => format!("{:?}", x),
        Expr::Bool(b) => b.to_string(),
        _ => "...".to_string(),
    }
}

/// A JSON string literal.
fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

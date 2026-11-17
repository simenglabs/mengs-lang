use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{Signature, Type};

/// How an expression accesses a variable.
#[derive(Clone, Copy, PartialEq)]
pub enum Access {
    /// `show(x)`, `if x ...`, `x + 1`: look at the value, ownership stays.
    Read,
    /// `set y: T = x;`, `Func(x)`, `return x;`: ownership moves out of `x`.
    Move,
}

/// How a variable was declared.
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// Moved on first move.
    Owned,
    /// `reuse`: never moved.
    Reuse,
    /// `keep`: constant, never moved or reassigned.
    Keep,
}

#[derive(Clone)]
struct VarInfo {
    ty: Type,
    kind: Kind,
    /// For `set y: T <== x;`: the owner this variable borrows from.
    borrows: Option<String>,
    /// Line where the value was moved out.
    moved_at: Option<usize>,
    /// For borrows: line where the owner got a new value, so the borrow is stale.
    stale_at: Option<usize>,
}

/// Variables visible in a block, with their type and ownership state,
/// plus the signatures of all functions in the program.
#[derive(Clone, Default)]
pub struct Scope {
    vars: HashMap<String, VarInfo>,
    functions: Rc<HashMap<String, Signature>>,
}

impl Scope {
    pub fn new(functions: Rc<HashMap<String, Signature>>) -> Scope {
        Scope { vars: HashMap::new(), functions }
    }

    pub fn function(&self, name: &str) -> Option<&Signature> {
        self.functions.get(name)
    }

    pub fn declare(&mut self, name: &str, ty: Type, kind: Kind) -> Result<(), String> {
        self.insert(name, VarInfo { ty, kind, borrows: None, moved_at: None, stale_at: None })
    }

    /// Declare `name` as a borrow of `owner` (or of whatever `owner` itself borrows).
    pub fn declare_borrow(&mut self, name: &str, ty: Type, owner: &str) -> Result<(), String> {
        let root = self.borrow_root(owner).unwrap_or_else(|| owner.to_string());
        let info = VarInfo { ty, kind: Kind::Reuse, borrows: Some(root), moved_at: None, stale_at: None };
        self.insert(name, info)
    }

    fn insert(&mut self, name: &str, info: VarInfo) -> Result<(), String> {
        if self.vars.contains_key(name) {
            return Err(format!("variable `{}` is already declared", name));
        }
        self.vars.insert(name.to_string(), info);
        Ok(())
    }

    /// The owner a borrowed variable points to, if it is a borrow.
    pub fn borrow_root(&self, name: &str) -> Option<String> {
        self.vars.get(name).and_then(|v| v.borrows.clone())
    }

    /// Access a variable, checking it (and the owner it borrows from) still has
    /// its value. A `Move` of a regular owned variable marks it as moved.
    pub fn access(&mut self, name: &str, line: usize, access: Access) -> Result<Type, String> {
        let var = self.get(name)?;
        if let Some(prev) = var.moved_at {
            return Err(format!(
                "variable `{}` was moved on line {} and can no longer be used; \
                 declare it with `reuse`, borrow it with `<==`, or copy it with `{} ()=> new_name;`",
                name, prev, name
            ));
        }
        if let Some(owner) = &var.borrows {
            if let Some(prev) = self.vars.get(owner).and_then(|o| o.moved_at) {
                return Err(format!("`{}` borrows `{}`, but `{}` was moved on line {}", name, owner, owner, prev));
            }
            if let Some(prev) = var.stale_at {
                return Err(format!(
                    "`{}` borrows `{}`, but `{}` got a new value on line {}; borrow it again",
                    name, owner, owner, prev
                ));
            }
        }

        let ty = var.ty;
        if access == Access::Move && var.kind == Kind::Owned && var.borrows.is_none() {
            self.vars.get_mut(name).unwrap().moved_at = Some(line);
        }
        Ok(ty)
    }

    /// `name = ...;` — gives the variable a value again (a moved variable becomes
    /// usable) and makes existing borrows of it stale. Returns its type.
    pub fn reassign(&mut self, name: &str, line: usize) -> Result<Type, String> {
        let var = self.get(name)?;
        if var.kind == Kind::Keep {
            return Err(format!("`{}` is a constant (keep) and cannot be reassigned", name));
        }
        if let Some(owner) = &var.borrows {
            return Err(format!("`{}` borrows `{}` and cannot be reassigned; reassign `{}` instead", name, owner, owner));
        }
        let ty = var.ty;
        self.vars.get_mut(name).unwrap().moved_at = None;
        self.make_borrows_stale(name, line);
        Ok(ty)
    }

    /// `push(xs, v);`, `xs[i] = v;` — changes the value in place. Like a reassign,
    /// this makes existing borrows stale (the array may move in memory).
    pub fn mutate(&mut self, name: &str, line: usize) -> Result<Type, String> {
        let ty = self.access(name, line, Access::Read)?;
        let var = self.get(name)?;
        if var.kind == Kind::Keep {
            return Err(format!("`{}` is a constant (keep) and cannot be changed", name));
        }
        if let Some(owner) = &var.borrows {
            return Err(format!("`{}` borrows `{}` and cannot change it; change `{}` instead", name, owner, owner));
        }
        self.make_borrows_stale(name, line);
        Ok(ty)
    }

    fn make_borrows_stale(&mut self, owner: &str, line: usize) {
        for v in self.vars.values_mut() {
            if v.borrows.as_deref() == Some(owner) {
                v.stale_at = Some(line);
            }
        }
    }

    /// Type of a declared variable (without checking its ownership state).
    pub fn type_of(&self, name: &str) -> Option<Type> {
        self.vars.get(name).map(|v| v.ty)
    }

    /// Whether moving this variable really takes its value (an owned variable,
    /// not `keep`/`reuse`/a borrow, which are copied instead).
    pub fn is_owned(&self, name: &str) -> bool {
        self.vars.get(name).is_some_and(|v| v.kind == Kind::Owned && v.borrows.is_none())
    }

    fn get(&self, name: &str) -> Result<&VarInfo, String> {
        self.vars.get(name).ok_or_else(|| format!("variable `{}` is not declared", name))
    }

    /// After an `if`: take the ownership state from the branches. `None` is a branch
    /// that always returns, so its moves do not matter after the `if`.
    /// A variable moved in either remaining branch counts as moved.
    pub fn merge_branches(&mut self, a: Option<&Scope>, b: Option<&Scope>) {
        for (name, var) in self.vars.iter_mut() {
            let x = a.and_then(|s| s.vars.get(name));
            let y = b.and_then(|s| s.vars.get(name));
            var.moved_at = x.and_then(|v| v.moved_at).or(y.and_then(|v| v.moved_at));
            var.stale_at = x.and_then(|v| v.stale_at).or(y.and_then(|v| v.stale_at));
        }
    }

    /// After a loop body: a variable from outside that the body leaves moved
    /// (or a borrow it leaves stale) would be broken on the next iteration.
    /// Returns the offending line and message.
    pub fn check_loop(&self, body: &Scope) -> Result<(), (usize, String)> {
        for (name, var) in &self.vars {
            let Some(after) = body.vars.get(name) else { continue };
            if let (None, Some(line)) = (var.moved_at, after.moved_at) {
                return Err((line, format!(
                    "variable `{}` is moved inside a loop, so the next iteration could not use it; \
                     declare it with `reuse`, borrow it, copy it, or give it a new value before the loop repeats",
                    name
                )));
            }
            if let (None, Some(line)) = (var.stale_at, after.stale_at) {
                return Err((line, format!(
                    "`{}` borrows a variable that gets a new value inside a loop, so the next iteration could not use it",
                    name
                )));
            }
        }
        Ok(())
    }
}

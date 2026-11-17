use std::collections::HashMap;

use crate::ast::Type;

/// How an expression accesses a variable.
#[derive(Clone, Copy, PartialEq)]
pub enum Access {
    /// `show(x)`, `if x ...`: look at the value, ownership stays.
    Read,
    /// `set y: T = x;`, `Func(x)`: ownership moves out of `x`.
    Move,
}

#[derive(Clone)]
struct VarInfo {
    ty: Type,
    /// `keep` / `reuse` variables (and borrows) are never moved out.
    reusable: bool,
    /// For `set y: T <== x;`: the owner this variable borrows from.
    borrows: Option<String>,
    /// Line where the value was moved out.
    moved_at: Option<usize>,
}

/// Variables visible in a block, with their type and ownership state.
#[derive(Clone, Default)]
pub struct Scope {
    vars: HashMap<String, VarInfo>,
}

impl Scope {
    /// Declare an owned variable (`reusable` = declared with `keep` or `reuse`).
    pub fn declare(&mut self, name: &str, ty: Type, reusable: bool) -> Result<(), String> {
        self.insert(name, VarInfo { ty, reusable, borrows: None, moved_at: None })
    }

    /// Declare `name` as a borrow of `owner` (or of whatever `owner` itself borrows).
    pub fn declare_borrow(&mut self, name: &str, ty: Type, owner: &str) -> Result<(), String> {
        let root = self.borrow_root(owner).unwrap_or_else(|| owner.to_string());
        self.insert(name, VarInfo { ty, reusable: true, borrows: Some(root), moved_at: None })
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
        let var = self
            .vars
            .get(name)
            .ok_or_else(|| format!("variable `{}` is not declared", name))?;

        if let Some(prev) = var.moved_at {
            return Err(format!(
                "variable `{}` was moved on line {} and can no longer be used; \
                 declare it with `reuse`, borrow it with `<==`, or copy it with `{} ()=> new_name;`",
                name, prev, name
            ));
        }
        if let Some(owner) = &var.borrows {
            if let Some(prev) = self.vars.get(owner).and_then(|o| o.moved_at) {
                return Err(format!(
                    "`{}` borrows `{}`, but `{}` was moved on line {}",
                    name, owner, owner, prev
                ));
            }
        }

        let ty = var.ty;
        if access == Access::Move && !var.reusable {
            self.vars.get_mut(name).unwrap().moved_at = Some(line);
        }
        Ok(ty)
    }

    /// Copy "moved" marks from a branch scope (`if`/`else`) back into this one:
    /// a variable moved in either branch counts as moved after the `if`.
    pub fn merge_moved(&mut self, branch: &Scope) {
        for (name, var) in self.vars.iter_mut() {
            if var.moved_at.is_none() {
                var.moved_at = branch.vars.get(name).and_then(|b| b.moved_at);
            }
        }
    }
}

use crate::ast::{Function, Type};

use super::at;

/// A function call checked after every function is known,
/// so functions can be called before they are defined.
pub struct PendingCall {
    pub line: usize,
    pub name: String,
    pub arg_types: Vec<Type>,
}

/// Check that each called function exists, the argument count fits (taking
/// defaults into account), and argument types match.
pub fn check_calls(functions: &[Function], calls: &[PendingCall]) -> Result<(), String> {
    for call in calls {
        let f = functions
            .iter()
            .find(|f| f.name == call.name)
            .ok_or_else(|| at(call.line, format!("function `{}` is not defined", call.name)))?;

        let max = f.params.len();
        let min = f.params.iter().filter(|p| p.default.is_none()).count();
        let given = call.arg_types.len();
        if given < min || given > max {
            let expected = if min == max { max.to_string() } else { format!("{}-{}", min, max) };
            return Err(at(
                call.line,
                format!("function `{}` takes {} argument(s), but {} were given", call.name, expected, given),
            ));
        }

        for (i, (param, &ty)) in f.params.iter().zip(&call.arg_types).enumerate() {
            if !param.ty.accepts(ty) {
                return Err(at(
                    call.line,
                    format!(
                        "argument {} (`{}`) of function `{}` must be {}, got {}",
                        i + 1,
                        param.name,
                        call.name,
                        param.ty.name(),
                        ty.name()
                    ),
                ));
            }
        }
    }
    Ok(())
}

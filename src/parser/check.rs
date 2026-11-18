use crate::ast::{Expr, Signature};

/// Check a call against the function's signature (argument count, taking
/// defaults into account, and argument types), then fill in missing
/// arguments with the parameters' default values. An error carries the index of
/// the argument it is about (`None` for a wrong argument count).
pub fn check_call(sig: &Signature, mut args: Vec<Expr>) -> Result<Vec<Expr>, (Option<usize>, String)> {
    let max = sig.params.len();
    let min = sig.params.iter().filter(|p| p.default.is_none()).count();
    let given = args.len();
    if given < min || given > max {
        let expected = if min == max { max.to_string() } else { format!("{}-{}", min, max) };
        return Err((None, format!("function `{}` takes {} argument(s), but {} were given", sig.name, expected, given)));
    }

    for (i, (param, arg)) in sig.params.iter().zip(&args).enumerate() {
        if !param.ty.accepts(arg.ty()) {
            let message = format!(
                "argument {} (`{}`) of function `{}` must be {}, got {}",
                i + 1,
                param.name,
                sig.name,
                param.ty.name(),
                arg.ty().name()
            );
            return Err((Some(i), message));
        }
    }

    let defaults = sig.params[given..].iter().filter_map(|p| p.default.clone());
    args.extend(defaults);
    Ok(args)
}

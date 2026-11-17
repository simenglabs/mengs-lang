# Plan

How to work through [TODO.md](TODO.md). Phases are ordered by dependency:
each one builds on the previous. Within a phase, items can be done in any order.

Every feature must:
- work in **both** backends (C codegen and interpreter) with identical output,
- be covered by tests (`cargo test`) and an example in `test/main.meng`,
- be added to the README and, if it adds syntax, to the VS Code grammar/snippets.

---

## Phase 0 — Safety net ✅
| Item | Notes |
|---|---|
| First git commit | Before any refactor. |
| Parser unit tests | Ownership (move, reuse, keep, borrow, copy, moves inside `if`), type errors, unrecognized lines. These protect the Phase 1 rewrite. |

## Phase 1 — Real expressions ✅
Today an expression is a single literal or variable. Everything else in the
roadmap (arithmetic, logic, return values, loops) needs real expressions.

| Item | Notes |
|---|---|
| Tokenizer + expression parser | Replace string splitting in `parser/expr.rs` with tokens and precedence climbing. Statements stay line-based. |
| Arithmetic `+ - * / mod` | `int op int → int`, any `float → float`. Division by zero: runtime error in both backends. Remainder is `mod` because `%` starts a comment. |
| Logic `and`, `or`, `not` | Merge `Cond` into `Expr`: a condition is just an expression of type `bool`. |

**Decided:** variables inside a larger expression are only **read**, never moved.
Only a *bare* variable moves (`set y: int = x;`, `F(x)`).
So `set c: int = a + b;` leaves `a` and `b` usable, and `a + a` is valid.

## Phase 2 — Statements ✅
| Item | Notes |
|---|---|
| Reassign `x = 5;` | Not allowed on `keep` or borrows. Reassigning a moved variable makes it usable again. |
| Return values | `fnc Add(a: int, b: int): int -> return a + b; <-;`. Calls become expressions. Checker: every path returns. |
| Loops | `while cond -> ... <-;`. Moving an outer variable inside a loop is an error (it would be moved twice). |

## Phase 3 — Data and memory ✅
| Item | Notes |
|---|---|
| Heap strings + `free` | Strings created at runtime are owned; the compiler emits `free` when the owner goes out of scope. This is where ownership finally saves memory. |
| String concatenation | `"a" + name`. Needs heap strings. |
| Input `read()` | Returns `str`. Add `int(s)` / `float(s)` conversions. |
| Arrays | `set xs: [int] = [1, 2, 3];`, `xs[0]`, `len(xs)`. Same ownership rules as strings. |

## Phase 4 — Tooling and quality ✅
| Item | Notes |
|---|---|
| All errors at once | Parser collects errors and skips to the next line instead of stopping. The extension already shows multiple. |
| Column in errors | Comes almost for free from the Phase 1 tokenizer. |
| `output.c` in a temp/build folder, `-O2` | Small changes in `backend.rs`. |
| Several statements per line | Becomes possible once the tokenizer handles `;`. |
| Smarter autocomplete | `if`/loop block scope, hide moved variables. Best done by having `meng` output symbols (e.g. `meng symbols file.meng`) instead of regexes in JS. |

## Phase 5 — Distribution
| Item | Notes |
|---|---|
| Prebuilt binaries | GitHub Actions builds per OS into Releases; `install.sh` downloads them, so Rust is no longer required. |
| Extension on the Marketplace | README, icon, `vsce publish`. |
| Modules / `import` | Last: needs a stable language first. |

# v1 Foundations Implementation Plan (D8, D3, D7)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put the language guides in `docs/` with a single milestone roadmap (D8), switch to the accepted ownership rules — scalars copy, `keep` parameters borrow (D3) — and replace the line-based statement parser with a token-based one so statements can span lines and errors have exact columns (D7).

**Architecture:** Meng is a Rust compiler (`src/`) with a parser that type- and ownership-checks into an AST, a C code generator (`src/codegen/`) built with gcc, and a tree-walking interpreter (`src/interpreter/`); both backends must print identical output. Today statements are split into "pieces" at `;` / `->` (`src/parser/diagnostic.rs::split_pieces`) and parsed from strings (`src/parser/stmt.rs`); expressions are already token-based (`src/parser/lexer.rs`, `src/parser/expr.rs`). This plan keeps the module layout and replaces the string-based statement layer with tokens that carry line/column.

**Tech Stack:** Rust 2024 edition, no crates; C99 output compiled with gcc/clang; VS Code extension in plain JavaScript (`editors/vscode/`).

**Spec:** `docs/design/v1-decisions.md` (decisions D3, D7, D8; all Accepted).

**Follow-up plans (written after this one lands, because they build on the new parser):** Plan 2 — M2 data: structs, maps, optionals, `T or error` + `try`, `for` / `break` / `continue`. Plan 3 — D2 tasks: `spawn` / `wait`, channels, scheduler.

## Global Constraints

- All code, comments, error messages, and docs are in English.
- No new Rust crates in this plan (`Cargo.toml` `[dependencies]` stays empty).
- Do not modify `TODO.md` or `test/deps.meow` (the maintainer's uncommitted work); never `git add -A` without excluding them: use `git add -A -- . ':!TODO.md' ':!test/deps.meow'`.
- Every commit message ends with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- `cargo test` passes after every task.
- The interpreter and the compiled program print identical output for `test/main.meng`: `cargo build --release && ./target/release/meng interpret test/main.meng > /tmp/i.txt && ./target/release/meng compile test/main.meng && ./main > /tmp/c.txt && diff /tmp/c.txt /tmp/i.txt && rm main` prints nothing.
- Generated C stays warning-free and memory-safe: `./target/release/meng emit test/main.meng > /tmp/m.c && cc -Wall -Wextra -Werror -o /dev/null /tmp/m.c && cc -fsanitize=address -g -o /tmp/m_asan /tmp/m.c && /tmp/m_asan > /dev/null` succeeds with no output on stderr.
- Output formats stay exactly as today (the VS Code extension parses them): diagnostics `Display` is `Line N: message`; the CLI prints `[ERROR] file:line:column: message` followed by the source line and a caret; `meng symbols` JSON keeps its fields.
- Existing syntax keeps working unchanged; only the behavior changes listed in Tasks 2–3 are allowed to break existing programs.

## Review Focus

1. A statement spanning several lines whose string literal contains `;`, `->`, or `%` — must parse as one statement with the string intact (Task 5 test `multi_line_statement_with_tricky_string`).
2. `if a <-1 ->` (comparison with a negative number right after `<`) — must still mean `a < -1`, not a block terminator (Task 4 test `less_than_negative_is_not_a_block_end`).
3. A `keep` heap parameter passed on to a function whose parameter is *not* `keep` — must be a compile error with the copy hint, not a silent copy and not a crash (Task 3 test `keep_parameter_cannot_be_moved`).
4. Code examples in the guides drifting from the real language — every ```` ```meng ```` block in `docs/guide/` must pass `meng check` (Task 1 test `guide_examples_compile`, kept green by Tasks 2, 3, 5).
5. A missing `;` at the end of a line, which now merges with the next line into one statement — must still report `missing `;`` on the line where it is missing (Task 5 test `missing_semicolon_points_at_its_line`).

---

### Task 1: Documentation in `docs/` and a single roadmap (D8)

**Files:**
- Create: `docs/README.md`, `docs/guide/language.md`, `docs/guide/ownership.md`, `docs/guide/tooling.md`
- Create: `tests/docs.rs`
- Modify: `PLAN.md` (rewrite), `README.md` (links only)

**Interfaces:**
- Produces: `tests/docs.rs` — an integration test that runs the built `meng` binary (`env!("CARGO_BIN_EXE_meng")`) as `meng check -` on every ```` ```meng ```` block of every `docs/guide/*.md` file. Later tasks keep these examples valid. Blocks that are not meant to compile use ```` ```text ````.

- [ ] **Step 1: Write the failing test** `tests/docs.rs` with `#[test] fn guide_examples_compile()`: for each `docs/guide/*.md` (sorted), extract each block between a line equal to "```meng" and the next "```"; pipe it to `meng check -` (working directory: `docs/guide/`); collect failures as `"<file>, block starting line <n>: <stderr>"`; assert the failure list is empty and that at least one block was found in total.
- [ ] **Step 2: Run it** — `cargo test --test docs` — expected FAIL (no guide files, zero blocks found).
- [ ] **Step 3: Write `docs/guide/language.md`** — the syntax guide for everything implemented today: comments `% %`; types `str int float bool` and arrays `[int]` etc.; `set` / `keep` / `reuse`; reassign; expressions and operator precedence (`or`, `and`, `not`, comparisons, `+ -`, `* / mod`, unary `-`; `mod` because `%` starts a comment; int/int division truncates); conditions `if` / `<- else if ... ->` / `<- else ->` / `<-;`; `while`; functions with defaults and return types (every path must `return`); strings (`+`, escapes, `str()` / `int()` / `float()`, `read()`, `len()`); arrays (`push`, indexing, `len`, `[]` typed by its slot); several statements per line; modules (`use "file.meng";`) and packages (`use "name";`); runtime errors (division by zero, index out of bounds, invalid conversion). Each section has a short ```` ```meng ```` example. Take the facts from `test/main.meng`, `README.md`, and the parser tests; do not document anything not implemented.
- [ ] **Step 4: Write `docs/guide/ownership.md`** — today's rules (they change in Tasks 2–3): move on `set y: T = x;`, function arguments, `return x;`, `push`; reading (`show`, conditions, inside expressions) never moves; `keep`, `reuse`, borrow `<==` (invalid after the owner is moved, reassigned, or changed), copy `()=>`; moves inside `if` branches and loops; heap strings and arrays are freed when their block ends. Examples that are *errors* go in ```` ```text ```` blocks with the error message shown.
- [ ] **Step 5: Write `docs/guide/tooling.md`** — CLI commands (`interpret`, `run`, `compile`, `check`, `emit`, `symbols`, `add`, `deps`), `deps.meow` format and registry (`MENG_REGISTRY`), installer options (`--vscode`, `--from-source`, `MENG_VERSION`, `MENG_RELEASE_URL`), the VS Code extension features. No ```` ```meng ```` blocks needed here.
- [ ] **Step 6: Write `docs/README.md`** — index linking the three guides and `design/v1-decisions.md`.
- [ ] **Step 7: Rewrite `PLAN.md`** as the single roadmap: the seven milestones of the v1 backend plan (titles from `TODO.md` §"v1 backend release plan"), each item marked ✅ done, ◐ partial (say what is missing), or ☐ todo, using the "Already done" section of `docs/design/v1-decisions.md` and this plan's tasks; link each decision D1–D8 where it applies; keep "Follow-up plans" from this plan's header. Remove the old Phase 0–5 content (it is all done; summarize it in one "History" line).
- [ ] **Step 8: Update `README.md`** — keep it short; replace the dependencies snippet with a link to `docs/guide/tooling.md`, add a "Documentation" link to `docs/README.md`. Do not add syntax to the README.
- [ ] **Step 9: Run** `cargo test` — expected: all pass, including `guide_examples_compile`.
- [ ] **Step 10: Commit** — `git add docs/README.md docs/guide tests/docs.rs PLAN.md README.md && git commit` with message `Move language guides to docs/ and make PLAN.md follow the v1 milestones (D8)`.

---

### Task 2: Warnings, and scalars are copied (D3.1)

**Files:**
- Modify: `src/parser/diagnostic.rs`, `src/parser/mod.rs`, `src/parser/modules.rs`, `src/parser/scope.rs`, `src/parser/stmt.rs`, `src/parser/expr.rs`, `src/main.rs`
- Modify: `editors/vscode/extension.js` (warning severity)
- Modify: `src/parser/tests.rs`, `test/main.meng`, `docs/guide/ownership.md`, `docs/guide/language.md`

**Interfaces:**
- Produces: `pub enum Severity { Error, Warning }` and a `severity: Severity` field on `Diagnostic` (Display unchanged: `Line N: message`). `analyze` returns `(Result<Program, Vec<Diagnostic>>, Vec<Diagnostic>, Symbols)` — the middle vector holds the warnings, the `Err` vector only errors. `parse_file(name, code, dir) -> Result<(Program, Vec<SourceError>), Vec<SourceError>>`: `Ok` carries the warnings; `Err` carries errors and warnings (sorted by file order, line, column). The test-only `parse(code)` keeps returning `Result<Program, Vec<Diagnostic>>` and its `Err` contains only errors.
- Produces: CLI prints warnings as `[WARN] file:line:column: message` + source line + caret (same layout as errors) and still exits 0 when there are only warnings; the `[FAILED] N errors` summary counts errors only.
- Produces: the extension maps `[WARN] -:line:column: message` lines to `vscode.DiagnosticSeverity.Warning`.

- [ ] **Step 1: Write the failing tests** in `src/parser/tests.rs`:
  - `ints_are_copied`: `ok("set a: int = 1;\nset b: int = a;\nshow(a);\nfnc F(n: int)->\n<-;\nF(a);\nF(a);\nset f: float = 1.5;\nset g: float = f;\nshow(f);\nset t: bool = true;\nset u: bool = t;\nshow(t);")`.
  - `scalar_moves_are_not_marked`: parse `set a: int = 1;\nset b: int = a;` and assert the `Stmt::Set` value is `Expr::Var { moved: false, .. }`.
  - `reuse_on_a_scalar_warns`: with a new helper `warnings(src) -> Vec<String>` (Display strings of `Severity::Warning` diagnostics from `analyze`), assert `warnings("set reuse i: int = 0;\nshow(i);") == vec!["Line 1: `reuse` has no effect on int values (they are always copied)"]`, and that `parse` of the same source is `Ok`.
  - Convert the tests that relied on int moves to `str`: `moved_variable_cannot_be_read_either`, `ints_move_like_strings` (rename to `strings_move`), `a_bare_variable_still_moves`, `moved_variable_cannot_be_used_in_an_expression` — same lines and messages, values `"x"` instead of `1`.
- [ ] **Step 2: Run** `cargo test` — expected: the three new tests FAIL.
- [ ] **Step 3: Implement.** In `scope.rs`, a `Move` access marks a variable moved only when its type `is_heap()`; scalar variables are never moved. In `expr.rs::take`, `moved` is true only for owned heap variables. Add the warning when `set reuse` / a `reuse` parameter has a non-heap type (message exactly as in the test, with the type name). Plumb warnings through `analyze`, `modules.rs`, and `main.rs::report` (prefix `[WARN]`). In `extension.js`, accept both `ERROR` and `WARN` in the stderr regex and set the severity accordingly.
- [ ] **Step 4: Update** `test/main.meng` (line 201: `set reuse i: int = 0;` → `set i: int = 0;`; keep `reuse` on `str` / arrays), and the ownership guide: scalars are copied, only `str` and arrays move, `reuse` on scalars only warns.
- [ ] **Step 5: Run** `cargo test`, the identical-output check, and the generated-C check from Global Constraints — expected: all pass; `meng check test/main.meng` prints no `[WARN]`.
- [ ] **Step 6: Commit** — message `Copy scalars instead of moving them; add warnings (D3.1)`.

---

### Task 3: `keep` parameters borrow (D3.3, D3.4)

**Files:**
- Modify: `src/parser/scope.rs`, `src/parser/expr.rs`, `src/parser/mod.rs`, `src/codegen/mod.rs`, `src/codegen/expr.rs`
- Modify: `src/parser/tests.rs`, `src/codegen/tests.rs`, `src/interpreter/tests.rs`, `docs/guide/ownership.md`

**Interfaces:**
- Consumes: Task 2's scalar-copy rule (this task only concerns heap types: `str` and arrays).
- Produces: a new `Kind::KeepParam` in `scope.rs` for `keep` parameters of heap type. A `Move` access of a `KeepParam` is an error with the exact message: ``"`{name}` is a keep parameter: it is only borrowed, so it cannot be moved; copy it with `{name} ()=> new_name;`"``. Reading, borrowing (`<==`), copying (`()=>`), and passing it to another `keep` parameter are allowed.
- Produces: at a call, an argument for a `keep` heap parameter is parsed with `Access::Read` (the caller's variable is **not** moved and nothing is copied). C: such parameters are declared `const char *v_x` / `const meng_array *v_x`, are not freed by the callee, and receive the argument as `emit_top(arg)` (no `meng_str_dup` / `meng_take` / `meng_array_copy`).
- Unchanged: `keep` *variables* (`set keep x`) and `reuse` variables are still copied implicitly when moved.

- [ ] **Step 1: Write the failing tests:**
  - parser `keep_parameter_borrows_the_argument`: `ok("fnc Len(keep s: str): int ->\n    return len(s);\n<-;\nset name: str = \"meng\";\nshow(Len(name));\nshow(Len(name));\nshow(name);")`.
  - parser `keep_parameter_cannot_be_moved`: with `fnc Take(s: str)->\n<-;\n` prepended, each of `fnc F(keep s: str)->\n    Take(s);\n<-;`, `fnc F(keep s: str)->\n    set t: str = s;\n<-;`, `fnc F(keep s: str): str ->\n    return s;\n<-;`, `fnc F(keep xs: [int])->\n    set ys: [int] = xs;\n<-;` reports on the line of the move a message containing "is a keep parameter: it is only borrowed".
  - parser `keep_parameter_can_be_copied_and_reborrowed`: `ok("fnc G(keep s: str)->\n<-;\nfnc F(keep s: str): str ->\n    G(s);\n    s ()=> t;\n    return t;\n<-;")`.
  - Replace `keep_and_reuse_parameters_can_be_used_many_times` with a `reuse`-only version (`fnc F(reuse b: str)->` moving `b` twice is `ok`).
  - codegen `keep_parameters_are_borrowed`: for `fnc Len(keep s: str): int -> return len(s); <-;\nset name: str = "meng";\nshow(Len(name));` the C contains `static long long f_Len(const char *v_s)`, contains `f_Len(v_name)`, and the body of `f_Len` contains no `free(v_s)`.
  - interpreter `keep_parameters_leave_the_argument_usable`: the program from the first parser test prints `4\n4\nmeng\n`.
- [ ] **Step 2: Run** `cargo test` — expected: the new tests FAIL.
- [ ] **Step 3: Implement** the parser rule (declare heap `keep` params as `KeepParam`; parse matching call arguments with `Access::Read`; the `symbols` kind label stays `"keep"`), then codegen (signature, call arguments, no free). The interpreter needs no change beyond what the tests show (it clones values).
- [ ] **Step 4: Update** `docs/guide/ownership.md`: `keep` parameters borrow (no move, no copy), cannot escape; borrows are local and read-only; `reuse` copies the whole value each time (O(n) for strings and arrays) and is meant for small values; `keep` variables are immutable.
- [ ] **Step 5: Run** `cargo test`, the identical-output check, and the generated-C check (including ASan) — expected: all pass.
- [ ] **Step 6: Commit** — message `Make keep parameters borrow their argument (D3)`.

---

### Task 4: Source lexer with positions (D7, part 1)

**Files:**
- Modify: `src/parser/lexer.rs`
- Test: `src/parser/lexer.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub struct Spanned { pub token: Token, pub line: usize, pub column: usize }` (1-based; `column` counts bytes, like `Diagnostic::column` today).
- Produces: `pub fn tokenize_source(code: &str) -> Result<Vec<Spanned>, Diagnostic>` over comment-free source (comments are already replaced by spaces, so positions match the original file). It adds these symbols to the existing ones: `;` `:` `->` `<-` `<==` `()=>`; longest match first.
- Rule for `<-`: it is a single token only when it starts a statement — there is no previous token, or the previous token is `;` or `->`. Otherwise `<` and `-` are separate tokens (so `a <-1` is `a < -1`).
- Unchanged: the existing `tokenize(src: &str) -> Result<Vec<Token>, String>` keeps working until Task 5 removes its last caller.
- Lexing errors (`unexpected character`, `invalid number`, unclosed string, unknown escape) become `Diagnostic`s at the offending character; a string literal still cannot span lines.

- [ ] **Step 1: Write the failing tests** in `lexer.rs`:
  - `spans_are_one_based_line_and_column`: `tokenize_source("set a: int = 1;\n  show(a);")` gives tokens with `(line, column)` `set`(1,1) `a`(1,5) `:`(1,6) `int`(1,8) `=`(1,12) `1`(1,14) `;`(1,15) `show`(2,3) `(`(2,7) `a`(2,8) `)`(2,9) `;`(2,10).
  - `statement_symbols`: `"a ()=> b;\nset c: str <== a;\nif x ->\n<- else ->\n<-;"` yields `Sym("()=>")`, `Sym("<==")`, `Sym("->")` (×2), `Sym("<-")` (×2) in order, and `Ident("else")`.
  - `less_than_negative_is_not_a_block_end`: `"if a <-1 ->"` yields `Ident(if) Ident(a) Sym("<") Sym("-") Int(1) Sym("->")`.
  - `lexing_errors_have_positions`: `"show(\"abc);"` → `Err` at (1,6) with message `string is not closed with a quote `"``; `"x @ y"` → (1,3) `unexpected character `@``.
- [ ] **Step 2: Run** `cargo test lexer` — expected FAIL.
- [ ] **Step 3: Implement** `tokenize_source` by extending the existing lexing helpers (`lex_number`, `lex_string`) with line/column tracking.
- [ ] **Step 4: Run** `cargo test` — expected PASS.
- [ ] **Step 5: Commit** — message `Lex whole sources with line and column (D7, part 1)`.

---

### Task 5: Token-based statement parser (D7, part 2)

**Files:**
- Modify: `src/parser/diagnostic.rs` (statements from tokens instead of `split_pieces`), `src/parser/mod.rs`, `src/parser/stmt.rs`, `src/parser/expr.rs`, `src/parser/modules.rs`, `src/parser/symbols.rs` (columns), `src/parser/lexer.rs` (remove the old `tokenize` once unused)
- Modify: `src/parser/tests.rs`, `docs/guide/language.md`

**Interfaces:**
- Consumes: `tokenize_source`, `Spanned` (Task 4).
- Produces: a statement is the token range up to a `;` at bracket depth 0, or up to and including a block-opening `->`; a statement may span any number of lines. `<-;`, `<- else ->`, `<- else if <cond> ->` are block ends as today.
- Produces: every error carries the position of the token it is about: unknown / moved / undeclared names → that name's token; operator type errors → the operator token; a value of the wrong type in `set` → the declared name (as today); wrong argument type → the argument's first token; unexpected token → that token. The `diagnostic()` "search for the quoted name" heuristic is removed.
- Produces: `meng symbols` columns come from tokens (same JSON).
- Unchanged: all existing syntax, messages (apart from columns becoming exact), the `Imported` / `parse_file` / `symbols_file` API, and the `Diagnostic` / `SourceError` types.

- [ ] **Step 1: Write the failing tests** in `src/parser/tests.rs`:
  - `statements_can_span_lines`: `ok("set price: int = 2;\nset total: int = price\n    * 3\n    + 1;\nfnc Greet(\n    name: str,\n    greeting: str = \"Hi\"\n)->\n    show(greeting + name);\n<-;\nGreet(\n    \"Meng\"\n);\nif total > 1\n   and price > 0 ->\n    show(total);\n<-;")`.
  - `multi_line_statement_with_tricky_string`: parse `"show(\n    \"a; b -> c % d\"\n);"` and assert the `Stmt::Print` holds `Expr::Str("a; b -> c % d")`.
  - `missing_semicolon_points_at_its_line`: `err("show(1)\nshow(2);", 1, "missing `;`")`; and `err("show(1)", 1, "missing `;`")`.
  - `columns_are_exact`: errors of `"set a: int = 1;\nshow(a + b);\nshow(1 + \"x\");\nset s: str = 5;"` have `(line, column)` = `(2, 10)` for `b`, `(3, 8)` for the `+`, `(4, 5)` for `s`.
  - Keep every existing test passing; where a test pinned a heuristic column that is now more precise, update only the column and say so in the report.
- [ ] **Step 2: Run** `cargo test` — expected: the new tests FAIL.
- [ ] **Step 3: Implement.** Replace `split_pieces` with statement splitting over `Spanned` tokens; port `stmt.rs` statement parsers (`show`, `set`, borrow, copy, reassign, item assignment, `push`, calls, `return`, `use`, function headers, parameters) and `expr.rs` (`ExprParser` over `&[Spanned]`) to tokens; carry positions into `Diagnostic`; keep the error recovery from today (record, skip the statement, declare a failed `set`'s variable, skip blocks with a broken header). Remove `tokenize` (old) and the quoted-name heuristic when nothing uses them.
- [ ] **Step 4: Add** to `docs/guide/language.md` a short "Long statements" section with a ```` ```meng ```` example of a statement and a function call spanning lines.
- [ ] **Step 5: Run** `cargo test`, the identical-output check, the generated-C check, then `printf 'set a: int = 1;\nshow(b);\n' | ./target/release/meng check -` and expect `[ERROR] -:2:6: variable `b` is not declared`, and `./target/release/meng symbols test/main.meng | python3 -m json.tool > /dev/null` succeeds.
- [ ] **Step 6: Commit** — message `Parse statements from tokens: multi-line statements, exact columns (D7, part 2)`.

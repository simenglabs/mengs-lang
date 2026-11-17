# Meng

A small, statically typed toy language with compile-time ownership rules.
Meng programs (`.ml`) are either run by a built-in interpreter or compiled
to C and built into a native binary with GCC.

```meng
fnc Greet(name: str, greeting: str = "Hello")->
    show(greeting);
    show(name);
<-;

set keep owner: str = "nasri";
Greet(owner);
Greet(owner, "Good morning");
```

## Getting started

Requirements: Rust (2024 edition) and, for `compile`/`run`, GCC.

```bash
curl -sSf https://raw.githubusercontent.com/menglabs/mengs-lang/main/install.sh | sh
# with the VS Code extension:
curl -sSf https://raw.githubusercontent.com/menglabs/mengs-lang/main/install.sh | sh -s -- --vscode
```

From a local checkout: `./install.sh` (or `./install.sh --vscode`).

Open a new terminal afterwards if the installer says it updated your PATH.
To uninstall: `cargo uninstall mengs-lang`.

```bash
meng interpret test/main.ml     # run with the interpreter (no GCC)
meng run test/main.ml           # compile to a binary via GCC and run it
meng compile test/main.ml       # compile only (binary named after the file)
meng check test/main.ml         # type/ownership check only (`-` reads stdin)
```

The interpreter and the compiled binary produce identical output.

## Language

### Comments
```meng
% single line %
%
  multiple
  lines
%
```
A `%` inside a string is plain text. A comment cannot contain `%`.

### Variables and types
Every variable needs a type: `str`, `int`, `float`, or `bool`.
```meng
set name: str = "Meng";
set count: int = 3;
set ratio: float = 1.5;     % an int literal is also accepted for float %
set ready: bool = true;
show(name);                 % prints a value %
```
Strings support the escapes `\" \\ \n \t \r`.

### Constants
```meng
set keep owner: str = "nasri";
```

### Ownership
Assigning a variable to another variable, or passing it to a function,
**moves** it: the original can no longer be used. Reading it (`show`, `if`)
does not move it.

```meng
set a: str = "hi";
set b: str = a;     % a is moved %
show(a);            % error: variable `a` was moved on line 2 %
```

There are four ways to keep using a value:

| Syntax | Meaning |
|---|---|
| `set keep x: str = "v";` | constant, never moved |
| `set reuse x: str = "v";` | can be moved any number of times |
| `set y: str <== x;` | borrow: `x` stays usable; `y` is invalid once `x` moves |
| `x ()=> y;` | copy: `y` is a new, independent value |

### Conditions
Operators: `=  !=  <  >  <=  >=`. A bare `bool` value also works.
```meng
if count > 1 ->
    show("many");
<- else if count = 1 ->
    show("one");
<- else ->
    show("none");
<-;
```
`int` and `float` can be compared with each other; `bool` only with `=`/`!=`.

### Functions
Defined at the top level, with optional default values and `keep`/`reuse` parameters.
```meng
fnc Welcome(name: str, greeting: str = "Hello", repeat: int = 1)->
    show(greeting);
    show(name);
<-;

Welcome("Meng");
Welcome("Meng", "Hey", 2);
```

See [`test/main.ml`](test/main.ml) for a full tour.

## Project layout

```
src/
  main.rs          CLI (compile, run, interpret, check)
  ast/             types, expressions, statements
  parser/          comments, statements, expressions, scopes/ownership, call checks
  codegen/         AST -> C
  interpreter/     tree-walking interpreter
  backend.rs       writes output.c and calls GCC
editors/vscode/    VS Code extension
test/main.ml       example program
```

Run the tests with `cargo test`.

## VS Code extension

`editors/vscode` adds syntax highlighting, completion, hover, signature help,
go-to-definition, snippets, and live errors (from `meng check`).
See [its README](editors/vscode/README.md).

## Roadmap

See [TODO.md](TODO.md).

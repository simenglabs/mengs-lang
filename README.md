<p align="center">
  <img src="assets/branding/mengslang-logo.png" alt="Mengslang — a meowing cat with code brackets" width="320">
</p>

# Meng Language

A small, statically typed programming language with compile-time ownership checks.
Run programs with the built-in interpreter or compile them to native binaries through C.

The name comes from **meng** — an Indonesian nickname for a cat, inspired by its meow.

## Install

Downloads a prebuilt binary for Linux or macOS (no Rust needed); without a release
for your machine it builds from source, which needs Rust (2024 edition).
GCC is only needed to compile programs to native binaries.

```bash
curl -sSf https://raw.githubusercontent.com/simenglabs/mengs-lang/main/install.sh | sh
```

To also install the VS Code extension, append `-s -- --vscode` after `sh`
(`--from-source` builds with Rust instead of downloading).
From a local checkout, use `./install.sh`.
Open a new terminal if the installer updates your PATH.

## Get started

```bash
meng interpret test/main.meng
```

The CLI also supports `check`, `compile`, `run`, and `emit` (prints the generated C).

Dependencies are listed in `deps.meow` (`name:version` per line) and imported with `use "name" meow Function;`:

```bash
meng add postgres:0.1.0   # add or change a dependency, then install it
meng deps                 # install / update / remove packages to match deps.meow
```
Import selected functions from another file:

```meng
use "greetings" meow Hello, Goodbye;
show(Hello("Meng"));
```

For files inside folders, separate the module names with dots:

```meng
use "utils.parser" meow func_1, func_2;
```

This loads `utils/parser.meng` relative to the current source file. More levels
work the same way: `utils.text.parser` loads `utils/text/parser.meng`.

The earlier `greetings` example loads `greetings.meng`. An explicit
`.meng` extension also works. If an extensionless local file does not exist,
Meng looks for an installed package. Only functions listed after `meow` are
available to the importing file. The list is required; replace old
`use "greetings";` imports with `use "greetings" meow Hello;`.

Language syntax and usage guides will live in the documentation.

## Development

Run `cargo test` to check the compiler and interpreter.

- [VS Code extension](editors/vscode/README.md)
- [Roadmap](TODO.md)

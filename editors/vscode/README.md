# Meng for VS Code

Language support for [Meng](../../README.md) (`.ml`, `.meng`).

## Features

- Syntax highlighting
- Live errors from the real compiler (`meng check`), as you type
- Completion: keywords, types after `:`, functions (with argument snippets), variables in scope
- Hover (variable types, function signatures) and signature help
- Go to definition
- Snippets: `fnc`, `if`, `ife`, `elif`, `set`, `setk`, `setr`, `borrow`, `copy`, `show`, `%`
- Commands: **Meng: Run File (Interpreter)**, **Meng: Compile & Run File (GCC)**

## Requirements

The `meng` binary. Install it from the repository root with `cargo install --path .`.
The extension looks for `meng` on `PATH`, then in `~/.cargo/bin`.
Use the `meng.compilerPath` setting to point at another binary.

## Install locally

```bash
npx @vscode/vsce package --allow-missing-repository --skip-license
code --install-extension meng-lang-0.1.0.vsix
```

Reinstall after changing the extension source.

## Known limitations

- Only the first error is shown (the compiler stops at the first error).
- Completion does not know about `if`-block scope or moved variables.
- `.ml` is also used by OCaml; use `.meng` if both extensions are installed.

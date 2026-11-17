# Meng for VS Code

Language support for [Meng](../../README.md) (`.meng`).

## Features

- Syntax highlighting
- Live errors from the real compiler (`meng check`), as you type, all at once and underlined at their column
- Completion: keywords, types after `:`, built-ins, functions (with argument snippets), and the variables
  usable at the cursor (block scopes of `if`/`while`, moved variables hidden), using `meng symbols`
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

- Without a recent `meng` (with `meng symbols`), completion falls back to a simpler text index.

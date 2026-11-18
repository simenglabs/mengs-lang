# Tooling guide

## Installing

```bash
curl -sSf https://raw.githubusercontent.com/simenglabs/mengs-lang/main/install.sh | sh
```

The installer downloads a prebuilt `meng` binary (checked against its `.sha256`) into
`~/.meng/bin`. If no release exists for your machine, it builds from source, which needs
Rust. GCC is only needed to compile programs to native binaries. Open a new terminal if
the installer updated your `PATH`.

Options (append `-s -- <option>` after `sh` when piping):

| Option | Effect |
|---|---|
| `--vscode` | also install the VS Code extension |
| `--from-source` | build with Rust instead of downloading |

Environment variables:

| Variable | Effect |
|---|---|
| `MENG_VERSION` | pin a release, for example `v0.1.0` (default: latest) |
| `MENG_RELEASE_URL` | download from a mirror (or a local folder) instead of GitHub Releases |
| `MENG_REPO` | build from another source repository |

Running `./install.sh` from a local checkout always builds from that checkout.

## Commands

| Command | What it does |
|---|---|
| `meng interpret <file.meng>` | run directly with the interpreter (no GCC needed) |
| `meng run <file.meng>` | compile through C and run immediately |
| `meng compile <file.meng>` | compile to a native binary (via GCC), named after the file |
| `meng check <file.meng>` | only check for errors; `-` reads the program from standard input |
| `meng emit <file.meng>` | print the generated C code |
| `meng symbols <file.meng>` | print functions and variables as JSON (used by editors) |
| `meng add <name[:version]>` | add or change a dependency in `deps.meow`, then install it |
| `meng deps` | install, update, and remove packages to match `deps.meow` |

Errors are printed as `[ERROR] file:line:column: message`, followed by the source line
and a caret. Warnings use the same form with `[WARN] file:line:column: message`; they do
not stop compilation, and `meng check` still succeeds when there are only warnings.
`meng check` reports all errors and warnings at once. The interpreter and a compiled
program print the same output.

## Dependencies

A project lists its dependencies in `deps.meow`, in the folder where you run `meng`.
One `name:version` per line; `% comments %` and blank lines are allowed.

```text
% my project %
postgres:0.1.0
```

Import an installed package with `use "postgres";` (a name, not a `.meng` file).
`meng deps` installs the listed packages into `meng_deps/<name>/` of the project (flat:
the packages a package needs are installed there too), updates packages whose version
changed, and removes packages that are no longer listed. `meng add postgres:0.1.0`
edits `deps.meow` and then does the same; if the install fails, `deps.meow` is left as
it was.

A package is a git repository tagged `v<version>` whose entry file is `lib.meng`; it may
have its own `deps.meow`.

### Registry

Package names are resolved through a registry: a git repository with an `index.meow`
file holding one `name = git-url` per line. The default registry is
`https://github.com/simenglabs/meng-registry.git`. Set `MENG_REGISTRY` to a git URL or
a local folder to use another one.

## VS Code extension

Install it with `./install.sh --vscode`, from a GitHub release (`meng-lang.vsix`), or
by packaging it yourself (see [editors/vscode/README.md](../../editors/vscode/README.md)).
It provides:

- syntax highlighting
- live errors and warnings from `meng check`, underlined at their column
- completion for keywords, types, built-ins, functions, and the variables usable at the
  cursor (based on `meng symbols`)
- hover and signature help
- go to definition
- snippets (`fnc`, `if`, `ife`, `elif`, `set`, `setk`, `setr`, `borrow`, `copy`, `show`, `%`)
- the commands **Meng: Run File (Interpreter)** and **Meng: Compile & Run File (GCC)**

It looks for `meng` on `PATH`, then in `~/.meng/bin` and `~/.cargo/bin`; the
`meng.compilerPath` setting points it at another binary.

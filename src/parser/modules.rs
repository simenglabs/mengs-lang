//! `use "file" meow Function;` — load selected functions from a local file or
//! package (`meng_deps/name/lib.meng`, see `deps.rs`).
//!
//! An imported file may only contain functions (and its own `use`). A file sees
//! its own functions plus the functions explicitly selected by its imports.
//! All functions end up in one program, so names must be unique across files.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::Program;
use crate::deps;

use super::comments::strip_comments;
use super::diagnostic::{Diagnostic, diagnostic, split_pieces};
use super::expr::parse_identifier;
use super::symbols::{FunctionSymbol, Symbols};
use super::{Imported, analyze};

/// An error in one of the files of the program.
#[derive(Debug)]
pub struct SourceError {
    pub file: String,
    /// Source of that file, for showing the line.
    pub code: String,
    pub diagnostic: Diagnostic,
}

/// A module path and the functions selected by its `meow` list.
#[derive(Debug, PartialEq)]
pub struct Use {
    pub path: String,
    pub functions: Vec<String>,
}

/// Dots separate module components: `utils.parser` -> `utils/parser.meng`.
fn is_dotted_module(path: &str) -> bool {
    path.contains('.') && path.split('.').all(|part| {
        !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    })
}

pub fn parse_use(text: &str) -> Result<Use, String> {
    let usage = "invalid use, example: use \"greetings\" meow function_1, function_2;";
    let rest = text.strip_prefix("use ").and_then(|s| s.strip_suffix(';')).ok_or(usage)?.trim();
    let rest = rest.strip_prefix('"').ok_or(usage)?;
    let (path, rest) = rest.split_once('"').ok_or(usage)?;
    if path.is_empty() || !(path.ends_with(".meng") || Path::new(path).extension().is_none() || is_dotted_module(path)) {
        return Err(format!("invalid module path `{}`; expected a .meng file, a module name such as utils.parser, or a package name", path));
    }
    if !rest.starts_with(char::is_whitespace) {
        return Err(usage.into());
    }
    let names = rest.trim_start().strip_prefix("meow").ok_or(usage)?;
    if !names.starts_with(char::is_whitespace) || names.trim().is_empty() {
        return Err(usage.into());
    }
    let mut functions = Vec::new();
    for name in names.split(',') {
        let name = parse_identifier(name.trim())?;
        if functions.contains(&name) {
            return Err(format!("function `{}` is listed more than once in this import", name));
        }
        functions.push(name);
    }
    Ok(Use { path: path.to_string(), functions })
}

/// Where an import points: a file relative to `dir`, or an installed package.
fn resolve(dir: &Path, path: &str) -> Result<(PathBuf, String), String> {
    if path.ends_with(".meng") {
        return Ok((dir.join(path), display_path(dir, path)));
    }
    let local = if is_dotted_module(path) {
        format!("{}.meng", path.replace('.', "/"))
    } else {
        format!("{}.meng", path)
    };
    if dir.join(&local).is_file() {
        return Ok((dir.join(&local), display_path(dir, &local)));
    }
    if !deps::is_package_name(path) {
        return Err(format!("cannot find `{}` (looked for {})", path, dir.join(&local).display()));
    }
    let found = deps::find_package(dir, path).ok_or_else(|| {
        format!(
            "package `{}` is not installed; add it with `meng add {}:<version>` (or list it in {} and run `meng deps`)",
            path,
            path,
            deps::DEPS_FILE
        )
    })?;
    // Show it relative to the current folder when possible.
    let cwd = std::env::current_dir().ok().and_then(|d| d.canonicalize().ok());
    let shown = cwd.and_then(|c| found.strip_prefix(c).ok().map(Path::to_path_buf)).unwrap_or_else(|| found.clone());
    Ok((found, shown.display().to_string()))
}

/// Parse a program file and everything it imports. `name` is how the file is
/// shown in errors (`-` for standard input); imports resolve from `dir`.
pub fn parse_file(name: &str, code: &str, dir: &Path) -> Result<Program, Vec<SourceError>> {
    let mut loader = Loader::default();
    let root = loader.load(name, code, dir, false);
    let mut program = Program::default();
    for module in &mut loader.done {
        program.functions.append(&mut module.functions);
    }
    match root {
        Some(mut root) if loader.errors.is_empty() => {
            program.functions.append(&mut root.functions);
            program.main = root.main;
            Ok(program)
        }
        _ => Err(loader.errors),
    }
}

/// Symbols of a file for editors, including the functions it imports.
pub fn symbols_file(name: &str, code: &str, dir: &Path) -> Symbols {
    let mut loader = Loader::default();
    loader.load(name, code, dir, false);
    loader.root_symbols.unwrap_or_default()
}

#[derive(Default)]
struct Loader {
    /// Files fully loaded, by canonical path: what they provide to importers.
    exports: HashMap<PathBuf, Imported>,
    /// Exported function symbols per file (for editors).
    export_symbols: HashMap<PathBuf, Vec<FunctionSymbol>>,
    /// Files currently being loaded (to detect import cycles).
    loading: Vec<PathBuf>,
    /// Imported files in load order (dependencies first).
    done: Vec<Program>,
    errors: Vec<SourceError>,
    root_symbols: Option<Symbols>,
}

impl Loader {
    /// Load one file. Returns its parsed program when it has no errors.
    fn load(&mut self, name: &str, code: &str, dir: &Path, library: bool) -> Option<Program> {
        let (imported, imported_symbols, ok) = self.load_imports(name, code, dir);
        let (result, mut symbols) = analyze(code, &imported, library);

        if !library {
            symbols.functions.extend(imported_symbols);
            self.root_symbols = Some(symbols);
        }
        match result {
            Ok(program) if ok => Some(program),
            Ok(_) => None,
            Err(errors) => {
                for diagnostic in errors {
                    self.errors.push(SourceError { file: name.into(), code: code.into(), diagnostic });
                }
                None
            }
        }
    }

    /// Load every `use` of a file. Returns the functions it can call, their
    /// symbols, and whether all imports loaded without errors.
    fn load_imports(&mut self, name: &str, code: &str, dir: &Path) -> (Imported, Vec<FunctionSymbol>, bool) {
        let (mut imported, mut symbols, mut ok) = (Imported::new(), Vec::new(), true);
        let Ok(stripped) = strip_comments(code) else {
            return (imported, symbols, true); // reported by `analyze`
        };
        for piece in split_pieces(&stripped) {
            let Ok(import) = piece.text.strip_prefix("use ").map(|_| parse_use(piece.text)).transpose() else { continue };
            let Some(import) = import else { continue };
            let path = import.path;
            let fail = |loader: &mut Loader, message: String| {
                loader.errors.push(SourceError { file: name.into(), code: code.into(), diagnostic: diagnostic(piece, message) });
            };

            let (full, display) = match resolve(dir, &path) {
                Ok(found) => found,
                Err(e) => {
                    fail(self, e);
                    ok = false;
                    continue;
                }
            };
            let Ok(canonical) = full.canonicalize() else {
                fail(self, format!("cannot find `{}` (looked for {})", path, full.display()));
                ok = false;
                continue;
            };
            if let Some(start) = self.loading.iter().position(|p| *p == canonical) {
                let cycle: Vec<String> = self.loading[start..].iter().chain([&canonical]).map(|p| file_name(p)).collect();
                fail(self, format!("import cycle: `{}` ({})", path, cycle.join(" -> ")));
                ok = false;
                continue;
            }

            if !self.exports.contains_key(&canonical) {
                let Ok(source) = fs::read_to_string(&canonical) else {
                    fail(self, format!("cannot read `{}`", path));
                    ok = false;
                    continue;
                };
                let errors_before = self.errors.len();
                self.loading.push(canonical.clone());
                // Resolve its own imports from its folder, keeping paths relative for display.
                let sub_dir = Path::new(&display).parent().map_or_else(|| dir.to_path_buf(), Path::to_path_buf);
                let program = self.load_library(&display, &source, &sub_dir, &canonical);
                self.loading.pop();
                match program {
                    Some(program) => self.done.push(program),
                    None if self.errors.len() > errors_before => {
                        fail(self, format!("`{}` has errors (see above)", path));
                        ok = false;
                    }
                    None => ok = false,
                }
            }

            // Only selected functions become visible; the full module still loads
            // so imported functions can call their own helpers.
            let exports = self.exports.get(&canonical).cloned().unwrap_or_default();
            let selected: HashSet<_> = import.functions.iter().cloned().collect();
            for fn_name in import.functions {
                let Some((sig, file)) = exports.get(&fn_name).cloned() else {
                    fail(self, format!("module `{}` has no function `{}`", path, fn_name));
                    ok = false;
                    continue;
                };
                match imported.get(&fn_name) {
                    Some((_, other)) if *other != file => {
                        fail(self, format!("function `{}` is defined in both `{}` and `{}`", fn_name, other, file));
                        ok = false;
                    }
                    _ => {
                        imported.insert(fn_name, (sig, file));
                    }
                }
            }
            for f in self.export_symbols.get(&canonical).into_iter().flatten() {
                if selected.contains(&f.sig.name) && !symbols.iter().any(|s: &FunctionSymbol| s.sig.name == f.sig.name) {
                    symbols.push(FunctionSymbol { sig: f.sig.clone(), file: f.file.clone(), ..*f });
                }
            }
        }
        (imported, symbols, ok)
    }

    /// Load an imported file and record what it exports.
    fn load_library(&mut self, display: &str, code: &str, dir: &Path, canonical: &Path) -> Option<Program> {
        let (mut exports, mut export_symbols, imports_ok) = self.load_imports(display, code, dir);
        let (result, symbols) = analyze(code, &exports, true);
        for f in symbols.functions {
            exports.insert(f.sig.name.clone(), (f.sig.clone(), display.to_string()));
            export_symbols.push(FunctionSymbol { file: Some(display.to_string()), ..f });
        }
        self.exports.insert(canonical.to_path_buf(), exports);
        self.export_symbols.insert(canonical.to_path_buf(), export_symbols);
        match result {
            Ok(program) if imports_ok => Some(program),
            Ok(_) => None,
            Err(errors) => {
                for diagnostic in errors {
                    self.errors.push(SourceError { file: display.into(), code: code.into(), diagnostic });
                }
                None
            }
        }
    }
}

/// How an imported file is shown: relative to the importing file's folder.
fn display_path(dir: &Path, path: &str) -> String {
    let joined = dir.join(path);
    joined.strip_prefix(".").unwrap_or(&joined).display().to_string()
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write files into a fresh temporary folder and return it.
    fn folder(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meng-modules-{}-{}", std::process::id(), files[0].0.replace('/', "_")));
        let _ = fs::remove_dir_all(&dir);
        for (name, code) in files {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, code).unwrap();
        }
        dir
    }

    fn errors(dir: &Path, main: &str) -> Vec<String> {
        let code = fs::read_to_string(dir.join(main)).unwrap();
        match parse_file(main, &code, dir) {
            Ok(_) => Vec::new(),
            Err(errors) => errors
                .iter()
                .map(|e| format!("{}:{}: {}", e.file, e.diagnostic.line, e.diagnostic.message))
                .map(|e| e.replace(&format!("{}/", dir.display()), "")) // show paths relative to the folder
                .collect(),
        }
    }

    #[test]
    fn imports_functions_transitively() {
        let dir = folder(&[
            ("main.meng", "use \"lib/math.meng\" meow Square, Label;\nshow(Square(3));\nshow(Label(\"x\", 1));"),
            ("lib/math.meng", "use \"text.meng\" meow Label;\nfnc Square(n: int): int -> return n * n; <-;"),
            ("lib/text.meng", "fnc Label(s: str, n: int): str -> return s + str(n); <-;"),
        ]);
        let code = fs::read_to_string(dir.join("main.meng")).unwrap();
        let code = format!("{code}\nfnc Own()->\n<-;");
        let program = parse_file("main.meng", &code, &dir).unwrap();
        let names: Vec<&str> = program.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["Label", "Square", "Own"], "dependencies first, then the file's own functions");
        assert_eq!(program.main.len(), 2);

        let symbols = symbols_file("main.meng", &code, &dir);
        let files: Vec<(&str, bool)> = symbols
            .functions
            .iter()
            .map(|f| (f.sig.name.as_str(), f.file.as_deref().is_some_and(|p| p.ends_with("lib/text.meng") || p.ends_with("lib/math.meng"))))
            .collect();
        assert_eq!(files, vec![("Own", false), ("Label", true), ("Square", true)]);
    }

    #[test]
    fn import_errors() {
        let dir = folder(&[
            ("a.meng", "use \"missing.meng\" meow F;\nuse \"lib.meng\" meow F;\nuse \"other.meng\" meow F;\nfnc F(): int -> return 9; <-;"),
            ("lib.meng", "fnc F(): int -> return 1; <-;\nshow(1);"),
            ("other.meng", "fnc F(): int -> return 2; <-;"),
        ]);
        let errs = errors(&dir, "a.meng");
        let expected = [
            "a.meng:1: cannot find `missing.meng`",
            "lib.meng:2: an imported file can only contain functions and `use`",
            "a.meng:2: `lib.meng` has errors (see above)",
            "a.meng:3: function `F` is defined in both `lib.meng` and `other.meng`",
            "a.meng:4: function `F` is already defined in `lib.meng`",
        ];
        for e in expected {
            assert!(errs.iter().any(|x| x.starts_with(e)), "missing `{e}` in {errs:#?}");
        }
    }

    #[test]
    fn import_cycles_are_reported() {
        let dir = folder(&[
            ("cyc.meng", "use \"one.meng\" meow One;"),
            ("one.meng", "use \"two.meng\" meow Two;\nfnc One(): int -> return 1; <-;"),
            ("two.meng", "use \"one.meng\" meow One;\nfnc Two(): int -> return 2; <-;"),
        ]);
        let errs = errors(&dir, "cyc.meng");
        assert!(errs.iter().any(|e| e.contains("import cycle: `one.meng` (one.meng -> two.meng -> one.meng)")), "{errs:#?}");
    }

    #[test]
    fn selected_imports_keep_helpers_and_filter_editor_symbols() {
        let dir = folder(&[
            ("select.meng", "use \"greetings\" meow function_1, function_2;\nshow(function_1());\nshow(function_2());"),
            ("greetings.meng", "fnc helper(): int -> return 7; <-;\nfnc function_1(): int -> return helper(); <-;\nfnc function_2(): int -> return 9; <-;"),
        ]);
        let code = fs::read_to_string(dir.join("select.meng")).unwrap();
        let program = parse_file("select.meng", &code, &dir).unwrap();
        assert_eq!(program.functions.len(), 3, "helpers must remain available at runtime");
        let mut output = Vec::new();
        crate::interpreter::run(&program, &mut std::io::Cursor::new(""), &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "7\n9\n");
        let symbols = symbols_file("select.meng", &code, &dir);
        let names: Vec<_> = symbols.functions.iter().map(|f| f.sig.name.as_str()).collect();
        assert_eq!(names, vec!["function_1", "function_2"]);
        let bad = format!("{code}\nshow(helper());");
        let errors = parse_file("select.meng", &bad, &dir).unwrap_err();
        assert!(errors.iter().any(|e| e.diagnostic.message.contains("helper")));
    }

    #[test]
    fn repeated_imports_select_additional_functions() {
        let dir = folder(&[
            ("repeat.meng", "use \"lib\" meow A;\nuse \"lib.meng\" meow B;\nuse \"lib\" meow A;\nshow(A() + B());"),
            ("lib.meng", "fnc A(): int -> return 1; <-;\nfnc B(): int -> return 2; <-;"),
        ]);
        assert!(errors(&dir, "repeat.meng").is_empty());
        let code = fs::read_to_string(dir.join("repeat.meng")).unwrap();
        assert_eq!(parse_file("repeat.meng", &code, &dir).unwrap().functions.len(), 2);
    }

    #[test]
    fn missing_selected_function_is_reported_at_import() {
        let dir = folder(&[
            ("missing-name.meng", "use \"lib\" meow Missing;"),
            ("lib.meng", "fnc Available()-> <-;"),
        ]);
        assert!(errors(&dir, "missing-name.meng").iter().any(|e| e == "missing-name.meng:1: module `lib` has no function `Missing`"));
    }

    #[test]
    fn package_imports_and_local_file_precedence() {
        let dir = folder(&[
            ("packages.meng", "use \"greetings\" meow Local;\nuse \"installed\" meow Packaged;\nshow(Local() + Packaged());"),
            ("greetings.meng", "fnc Local(): int -> return 1; <-;"),
            ("meng_deps/greetings/lib.meng", "fnc Other()-> <-;"),
            ("meng_deps/installed/lib.meng", "fnc Packaged(): int -> return 2; <-;"),
        ]);
        assert!(errors(&dir, "packages.meng").is_empty());
    }

    #[test]
    fn dotted_modules_resolve_nested_files_and_relative_imports() {
        let dir = folder(&[
            ("dotted.meng", "use \"utils.parser\" meow func_1, func_2;\nshow(func_1());\nshow(func_2());"),
            ("utils/parser.meng", "use \"nested.helper\" meow helper;\nfnc func_1(): int -> return helper(); <-;\nfnc func_2(): int -> return 2; <-;"),
            ("utils/nested/helper.meng", "fnc helper(): int -> return 1; <-;"),
        ]);
        let code = fs::read_to_string(dir.join("dotted.meng")).unwrap();
        let program = parse_file("dotted.meng", &code, &dir).unwrap();
        let mut output = Vec::new();
        crate::interpreter::run(&program, &mut std::io::Cursor::new(""), &mut output).unwrap();
        assert_eq!(output, b"1\n2\n");
        let symbols = symbols_file("dotted.meng", &code, &dir);
        assert_eq!(symbols.functions.len(), 2);
        assert!(symbols.functions.iter().all(|f| f.file.as_ref().unwrap().ends_with("utils/parser.meng")));
        let errors = parse_file("dotted.meng", "use \"utils.missing\" meow F;", &dir).unwrap_err();
        assert!(errors.iter().any(|e| e.diagnostic.message.contains("utils/missing.meng")));
    }

    #[test]
    fn use_syntax() {
        assert_eq!(
            parse_use("use \"greetings\" meow function_1, function_2;"),
            Ok(Use { path: "greetings".into(), functions: vec!["function_1".into(), "function_2".into()] })
        );
        assert!(parse_use("use \"a/b.meng\" meow F;").is_ok());
        for source in [
            "use a.meng meow F;", "use \"utils..parser\" meow F;", "use \"a\";",
            "use \"a\" meow;", "use \"a\" meow F,;", "use \"a\" meow F F;",
            "use \"a\" meow F, F;", "use \"a\" meow 123;", "use \"a\" meow return;",
            "use \"a\"meow F;", "use \"a\" meowF;", "use \"\" meow F;",
        ] {
            assert!(parse_use(source).is_err(), "{source}");
        }
    }
}

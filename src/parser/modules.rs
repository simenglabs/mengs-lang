//! `use "file.meng";` — load imported files; `use "name";` — load an installed
//! package (`meng_deps/name/lib.meng`, see `deps.rs`).
//!
//! An imported file may only contain functions (and its own `use`). A file sees
//! its own functions plus every function its imports provide (transitively).
//! All functions end up in one program, so names must be unique across files.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::Program;
use crate::deps;

use super::comments::strip_comments;
use super::diagnostic::{Diagnostic, Severity, Source};
use super::lexer::Token;
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

/// The tokens of `use "path.meng";` -> `path.meng` (a file), of `use "name";` -> `name` (a package).
pub fn parse_use(tokens: &[Token]) -> Result<String, String> {
    let usage = "invalid use, example: use \"math.meng\"; or use \"package\";";
    let [Token::Ident(word), Token::Str(path), Token::Sym(";")] = tokens else {
        return Err(usage.into());
    };
    if word != "use" {
        return Err(usage.into());
    }
    if path.contains('"') || !(path.ends_with(".meng") || deps::is_package_name(path)) {
        return Err(format!(
            "`{}` is neither a .meng file nor a package name, example: use \"math.meng\"; or use \"package\";",
            path
        ));
    }
    Ok(path.clone())
}

/// Where an import points: a file relative to `dir`, or an installed package.
fn resolve(dir: &Path, path: &str) -> Result<(PathBuf, String), String> {
    if path.ends_with(".meng") {
        return Ok((dir.join(path), display_path(dir, path)));
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
/// `Ok` carries the warnings; `Err` carries the errors and warnings (in file
/// order, then by line and column).
pub fn parse_file(name: &str, code: &str, dir: &Path) -> Result<(Program, Vec<SourceError>), Vec<SourceError>> {
    let mut loader = Loader::default();
    let root = loader.load(name, code, dir, false);
    let mut program = Program::default();
    for module in &mut loader.done {
        program.functions.append(&mut module.functions);
    }
    match root {
        Some(mut root) if !loader.has_errors() => {
            program.functions.append(&mut root.functions);
            program.main = root.main;
            Ok((program, loader.errors))
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
    /// Errors and warnings of all files, in load order.
    errors: Vec<SourceError>,
    root_symbols: Option<Symbols>,
}

impl Loader {
    fn has_errors(&self) -> bool {
        self.error_count() > 0
    }

    fn error_count(&self) -> usize {
        self.errors.iter().filter(|e| e.diagnostic.severity == Severity::Error).count()
    }

    /// Record the diagnostics of one file: errors and warnings, sorted by position.
    fn record(&mut self, file: &str, code: &str, errors: Vec<Diagnostic>, warnings: Vec<Diagnostic>) {
        let mut all: Vec<Diagnostic> = errors.into_iter().chain(warnings).collect();
        all.sort_by_key(|d| (d.line, d.column));
        for diagnostic in all {
            self.errors.push(SourceError { file: file.into(), code: code.into(), diagnostic });
        }
    }

    /// Load one file. Returns its parsed program when it has no errors.
    fn load(&mut self, name: &str, code: &str, dir: &Path, library: bool) -> Option<Program> {
        let (imported, imported_symbols, ok) = self.load_imports(name, code, dir);
        let (result, warnings, mut symbols) = analyze(code, &imported, library);

        if !library {
            symbols.functions.extend(imported_symbols);
            self.root_symbols = Some(symbols);
        }
        match result {
            Ok(program) => {
                self.record(name, code, Vec::new(), warnings);
                ok.then_some(program)
            }
            Err(errors) => {
                self.record(name, code, errors, warnings);
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
        let mut seen = HashSet::new();
        // Lexing errors and invalid `use` statements are reported by `analyze`.
        let (src, _) = Source::new(&stripped);
        for stmt in src.statements() {
            if !src.is(stmt.start, "use") {
                continue;
            }
            let tokens: Vec<Token> = src.tokens[stmt.start..stmt.end].iter().map(|t| t.token.clone()).collect();
            let Ok(path) = parse_use(&tokens) else { continue };
            // Errors point at the path.
            let at = stmt.start + 1;
            let fail = |loader: &mut Loader, message: String| {
                loader.errors.push(SourceError { file: name.into(), code: code.into(), diagnostic: src.error(at, message) });
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
            if !seen.insert(canonical.clone()) {
                continue; // the same file used twice
            }
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
                let errors_before = self.error_count();
                self.loading.push(canonical.clone());
                // Resolve its own imports from its folder, keeping paths relative for display.
                let sub_dir = Path::new(&display).parent().map_or_else(|| dir.to_path_buf(), Path::to_path_buf);
                let program = self.load_library(&display, &source, &sub_dir, &canonical);
                self.loading.pop();
                match program {
                    Some(program) => self.done.push(program),
                    None if self.error_count() > errors_before => {
                        fail(self, format!("`{}` has errors (see above)", path));
                        ok = false;
                    }
                    None => ok = false,
                }
            }

            // Bring the file's functions (and what it imports) into scope.
            for (fn_name, (sig, file)) in self.exports.get(&canonical).cloned().unwrap_or_default() {
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
                if !symbols.iter().any(|s: &FunctionSymbol| s.sig.name == f.sig.name) {
                    symbols.push(FunctionSymbol { sig: f.sig.clone(), file: f.file.clone(), ..*f });
                }
            }
        }
        (imported, symbols, ok)
    }

    /// Load an imported file and record what it exports.
    fn load_library(&mut self, display: &str, code: &str, dir: &Path, canonical: &Path) -> Option<Program> {
        let (mut exports, mut export_symbols, imports_ok) = self.load_imports(display, code, dir);
        let (result, warnings, symbols) = analyze(code, &exports, true);
        for f in symbols.functions {
            exports.insert(f.sig.name.clone(), (f.sig.clone(), display.to_string()));
            export_symbols.push(FunctionSymbol { file: Some(display.to_string()), ..f });
        }
        self.exports.insert(canonical.to_path_buf(), exports);
        self.export_symbols.insert(canonical.to_path_buf(), export_symbols);
        match result {
            Ok(program) => {
                self.record(display, code, Vec::new(), warnings);
                imports_ok.then_some(program)
            }
            Err(errors) => {
                self.record(display, code, errors, warnings);
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
            ("main.meng", "use \"lib/math.meng\";\nshow(Square(3));\nshow(Label(\"x\", 1));"),
            ("lib/math.meng", "use \"text.meng\";\nfnc Square(n: int): int -> return n * n; <-;"),
            ("lib/text.meng", "fnc Label(s: str, n: int): str -> return s + str(n); <-;"),
        ]);
        let code = fs::read_to_string(dir.join("main.meng")).unwrap();
        let code = format!("{code}\nfnc Own()->\n<-;");
        let (program, _) = parse_file("main.meng", &code, &dir).unwrap();
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
            ("a.meng", "use \"missing.meng\";\nuse \"lib.meng\";\nuse \"other.meng\";\nfnc F(): int -> return 9; <-;"),
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
            ("cyc.meng", "use \"one.meng\";"),
            ("one.meng", "use \"two.meng\";\nfnc One(): int -> return 1; <-;"),
            ("two.meng", "use \"one.meng\";\nfnc Two(): int -> return 2; <-;"),
        ]);
        let errs = errors(&dir, "cyc.meng");
        assert!(errs.iter().any(|e| e.contains("import cycle: `one.meng` (one.meng -> two.meng -> one.meng)")), "{errs:#?}");
    }

    #[test]
    fn use_syntax() {
        let parse = |code: &str| {
            let tokens = crate::parser::lexer::tokenize_source(code).unwrap();
            parse_use(&tokens.into_iter().map(|t| t.token).collect::<Vec<_>>())
        };
        assert_eq!(parse("use \"a/b.meng\";"), Ok("a/b.meng".to_string()));
        assert_eq!(parse("use\n  \"pkg\"\n;"), Ok("pkg".to_string()));
        assert!(parse("use math;").is_err(), "the path must be quoted");
        assert!(parse("use \"a.meng\"").is_err());
        assert!(parse("use \"a.txt\";").is_err());
    }
}

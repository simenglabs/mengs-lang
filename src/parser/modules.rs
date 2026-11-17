//! `use "file.meng";` — load imported files.
//!
//! An imported file may only contain functions (and its own `use`). A file sees
//! its own functions plus every function its imports provide (transitively).
//! All functions end up in one program, so names must be unique across files.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::Program;

use super::comments::strip_comments;
use super::diagnostic::{Diagnostic, diagnostic, split_pieces};
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

/// `use "path.meng";` -> `path.meng`
pub fn parse_use(text: &str) -> Result<String, String> {
    let usage = "invalid use, example: use \"math.meng\";";
    let path = text.strip_prefix("use ").and_then(|s| s.strip_suffix(';')).ok_or(usage)?.trim();
    let path = path.strip_prefix('"').and_then(|s| s.strip_suffix('"')).ok_or(usage)?;
    if !path.ends_with(".meng") || path.contains('"') {
        return Err(format!("`{}` is not a .meng file, example: use \"math.meng\";", path));
    }
    Ok(path.to_string())
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
        let mut seen = HashSet::new();
        for piece in split_pieces(&stripped) {
            let Ok(path) = piece.text.strip_prefix("use ").map(|_| parse_use(piece.text)).transpose() else { continue };
            let Some(path) = path else { continue };
            let fail = |loader: &mut Loader, message: String| {
                loader.errors.push(SourceError { file: name.into(), code: code.into(), diagnostic: diagnostic(piece, message) });
            };

            let display = display_path(dir, &path);
            let full = dir.join(&path);
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
            ("main.meng", "use \"lib/math.meng\";\nshow(Square(3));\nshow(Label(\"x\", 1));"),
            ("lib/math.meng", "use \"text.meng\";\nfnc Square(n: int): int -> return n * n; <-;"),
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
        assert_eq!(parse_use("use \"a/b.meng\";"), Ok("a/b.meng".to_string()));
        assert!(parse_use("use a.meng;").is_err());
        assert!(parse_use("use \"a.txt\";").is_err());
    }
}

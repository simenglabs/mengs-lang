mod ast;
mod backend;
mod codegen;
mod deps;
mod interpreter;
mod parser;

use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process;

enum Mode {
    Compile,
    Run,
    Interpret,
    Check,
    Emit,
    Symbols,
}

fn print_usage() {
    println!("Usage:");
    println!("  meng compile <file.meng>    - Compile to a binary (via GCC)");
    println!("  meng run <file.meng>        - Compile and run immediately");
    println!("  meng interpret <file.meng>  - Run directly with the interpreter (no GCC)");
    println!("  meng check <file.meng>      - Only check for errors (use `-` to read stdin)");
    println!("  meng emit <file.meng>       - Print the generated C code");
    println!("  meng symbols <file.meng>    - Print functions and variables as JSON (for editors)");
    println!();
    println!("Dependencies (deps.meow in the current folder):");
    println!("  meng add <name[:version]>   - Add or change a dependency, then install");
    println!("  meng deps                   - Install / update / remove dependencies to match deps.meow");
}

/// `meng add` / `meng deps` work on the project in the current folder.
fn run_deps(args: &[String]) -> ! {
    let project = Path::new(".");
    let result = match (args[1].as_str(), args.get(2)) {
        ("deps", None) => deps::install(project),
        ("add", Some(spec)) if args.len() == 3 => deps::add(spec, project),
        ("add", _) => Err("usage: meng add <name[:version]>, e.g. meng add postgres:0.1.0".into()),
        _ => Err("usage: meng deps".into()),
    };
    match result {
        Ok(()) => process::exit(0),
        Err(e) => {
            eprintln!("[ERROR] {}", e);
            process::exit(1);
        }
    }
}

/// Print an error as `[ERROR] file:line:column: message`, followed by the
/// source line and a caret under the column.
fn report(error: &parser::SourceError) {
    let (filename, e) = (&error.file, &error.diagnostic);
    let line = error.code.lines().nth(e.line - 1).unwrap_or("");
    // Diagnostics count bytes; show characters (and keep tabs so the caret lines up).
    let prefix: String = line.get(..e.column.saturating_sub(1)).unwrap_or(line).chars().collect();
    let column = prefix.chars().count() + 1;
    eprintln!("[ERROR] {}:{}:{}: {}", filename, e.line, column, e.message);
    let pad: String = prefix.chars().map(|c| if c == '\t' { '\t' } else { ' ' }).collect();
    let number = e.line.to_string();
    eprintln!("  {} | {}", number, line);
    eprintln!("  {} | {}^", " ".repeat(number.len()), pad);
}

fn read_source(filename: &str) -> io::Result<String> {
    if filename == "-" {
        let mut code = String::new();
        io::stdin().read_to_string(&mut code)?;
        Ok(code)
    } else {
        fs::read_to_string(filename)
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if matches!(args.get(1).map(String::as_str), Some("deps" | "add")) {
        run_deps(&args);
    }
    if args.len() < 3 {
        print_usage();
        return;
    }

    let mode = match args[1].as_str() {
        "compile" => Mode::Compile,
        "run" => Mode::Run,
        "interpret" => Mode::Interpret,
        "check" => Mode::Check,
        "emit" => Mode::Emit,
        "symbols" => Mode::Symbols,
        other => {
            eprintln!("[ERROR] Unknown command: {}", other);
            print_usage();
            process::exit(1);
        }
    };
    let filename = &args[2];
    if filename != "-" && Path::new(filename).extension().and_then(|e| e.to_str()) != Some("meng") {
        eprintln!("[ERROR] Meng source files must use the .meng extension: {}", filename);
        process::exit(1);
    }

    let code = read_source(filename).unwrap_or_else(|_| {
        eprintln!("[ERROR] Failed to read file: {}", filename);
        process::exit(1);
    });

    // `use "x.meng";` resolves from the file's folder (the current folder for stdin).
    let dir = match Path::new(filename).parent() {
        Some(p) if filename != "-" && !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => Path::new(".").to_path_buf(),
    };

    // Editors ask for symbols while the code may still have errors.
    if let Mode::Symbols = mode {
        print!("{}", parser::symbols_file(filename, &code, &dir).to_json());
        return;
    }

    // Pipeline: source (+ imports) -> AST -> (interpreter | C -> binary)
    let ast = parser::parse_file(filename, &code, &dir).unwrap_or_else(|errors| {
        for e in &errors {
            report(e);
        }
        if errors.len() > 1 {
            eprintln!("[FAILED] {} errors in '{}'", errors.len(), filename);
        }
        process::exit(1);
    });

    match mode {
        Mode::Check => println!("[OK] No errors in '{}'", filename),
        Mode::Emit => print!("{}", codegen::generate_c(&ast)),
        Mode::Symbols => unreachable!("handled before parsing"),
        Mode::Interpret => {
            let (stdin, stdout) = (io::stdin(), io::stdout());
            if let Err(e) = interpreter::run(&ast, &mut stdin.lock(), &mut stdout.lock()) {
                eprintln!("[ERROR] {}", e);
                process::exit(1);
            }
        }
        Mode::Compile | Mode::Run => {
            let output_binary = Path::new(filename)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("program");

            let c_code = codegen::generate_c(&ast);
            if let Err(e) = backend::build(&c_code, output_binary) {
                eprintln!("[FAILED] {}", e);
                process::exit(1);
            }
            println!("[OK] Compiled to binary: '{}'", output_binary);

            if let Mode::Run = mode {
                println!("[..] Running program...\n---");
                backend::run(output_binary);
            }
        }
    }
}

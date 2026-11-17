mod ast;
mod backend;
mod codegen;
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
}

fn print_usage() {
    println!("Usage:");
    println!("  meng compile <file.meng>    - Compile to a binary (via GCC)");
    println!("  meng run <file.meng>        - Compile and run immediately");
    println!("  meng interpret <file.meng>  - Run directly with the interpreter (no GCC)");
    println!("  meng check <file.meng>      - Only check for errors (use `-` to read stdin)");
}

/// Print an error as `[ERROR] file:line:column: message`, followed by the
/// source line and a caret under the column.
fn report(filename: &str, code: &str, e: &parser::Diagnostic) {
    let line = code.lines().nth(e.line - 1).unwrap_or("");
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
    if args.len() < 3 {
        print_usage();
        return;
    }

    let mode = match args[1].as_str() {
        "compile" => Mode::Compile,
        "run" => Mode::Run,
        "interpret" => Mode::Interpret,
        "check" => Mode::Check,
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

    // Pipeline: source -> AST -> (interpreter | C -> binary)
    let ast = parser::parse(&code).unwrap_or_else(|errors| {
        for e in &errors {
            report(filename, &code, e);
        }
        if errors.len() > 1 {
            eprintln!("[FAILED] {} errors in '{}'", errors.len(), filename);
        }
        process::exit(1);
    });

    match mode {
        Mode::Check => println!("[OK] No errors in '{}'", filename),
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

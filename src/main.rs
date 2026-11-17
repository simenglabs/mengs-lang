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
    println!("  meng compile <file.ml>    - Compile to a binary (via GCC)");
    println!("  meng run <file.ml>        - Compile and run immediately");
    println!("  meng interpret <file.ml>  - Run directly with the interpreter (no GCC)");
    println!("  meng check <file.ml>      - Only check for errors (use `-` to read stdin)");
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

    let code = read_source(filename).unwrap_or_else(|_| {
        eprintln!("[ERROR] Failed to read file: {}", filename);
        process::exit(1);
    });

    // Pipeline: source -> AST -> (interpreter | C -> binary)
    let ast = parser::parse(&code).unwrap_or_else(|e| {
        eprintln!("[ERROR] {}: {}", filename, e);
        process::exit(1);
    });

    match mode {
        Mode::Check => println!("[OK] No errors in '{}'", filename),
        Mode::Interpret => {
            let stdout = io::stdout();
            if let Err(e) = interpreter::run(&ast, &mut stdout.lock()) {
                eprintln!("[ERROR] Failed to write output: {}", e);
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

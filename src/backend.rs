use std::env;
use std::fs;
use std::process::{self, Command};

/// Compile C code into a binary with GCC. The C file goes to a temporary
/// folder and is removed afterwards (kept if GCC fails, for inspection).
pub fn build(c_code: &str, output_binary: &str) -> Result<(), String> {
    let c_file = env::temp_dir().join(format!("meng-{}.c", process::id()));
    fs::write(&c_file, c_code).map_err(|e| format!("Failed to write {}: {}", c_file.display(), e))?;

    let status = Command::new("gcc")
        .arg(&c_file)
        .arg("-O2")
        // `if x = x` in Meng is valid; don't surface a C warning about generated code.
        .arg("-Wno-tautological-compare")
        .arg("-o")
        .arg(output_binary)
        .status();

    match status {
        Ok(s) if s.success() => {
            let _ = fs::remove_file(&c_file);
            Ok(())
        }
        Ok(_) => Err(format!("GCC compilation failed (generated C kept at {}).", c_file.display())),
        Err(e) => Err(format!("Could not run gcc: {}", e)),
    }
}

/// Run the compiled binary.
pub fn run(binary: &str) {
    let _ = Command::new(format!("./{}", binary)).status();
}

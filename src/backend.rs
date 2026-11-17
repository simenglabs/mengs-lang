use std::fs;
use std::process::Command;

const C_OUTPUT: &str = "output.c";

/// Write the C code to disk and compile it into a binary with GCC.
pub fn build(c_code: &str, output_binary: &str) -> Result<(), String> {
    fs::write(C_OUTPUT, c_code).map_err(|e| format!("Failed to write {}: {}", C_OUTPUT, e))?;

    let status = Command::new("gcc")
        .arg(C_OUTPUT)
        // `if x = x` in Meng is valid; don't surface a C warning about generated code.
        .arg("-Wno-tautological-compare")
        .arg("-o")
        .arg(output_binary)
        .status();

    match status {
        Ok(s) if s.success() => Ok(()),
        _ => Err("GCC compilation failed.".to_string()),
    }
}

/// Run the compiled binary.
pub fn run(binary: &str) {
    let _ = Command::new(format!("./{}", binary)).status();
}

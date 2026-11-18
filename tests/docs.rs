//! Every ```meng block in docs/guide/*.md must pass `meng check -`.
//! Blocks that are not meant to compile use ```text instead.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn guide_examples_compile() {
    let guide = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/guide");
    let mut files: Vec<_> = fs::read_dir(&guide)
        .map(|rd| rd.map(|e| e.unwrap().path()).collect())
        .unwrap_or_default();
    files.retain(|p: &std::path::PathBuf| p.extension().is_some_and(|e| e == "md"));
    files.sort();

    let mut failures = Vec::new();
    let mut blocks = 0;
    for file in &files {
        let text = fs::read_to_string(file).unwrap();
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        let mut current: Option<(usize, String)> = None;
        for (i, line) in text.lines().enumerate() {
            match current.take() {
                None => {
                    if line == "```meng" {
                        current = Some((i + 1, String::new()));
                    }
                }
                Some((start, mut code)) => {
                    if line == "```" {
                        blocks += 1;
                        if let Err(e) = check(&guide, &code) {
                            failures.push(format!("{}, block starting line {}: {}", name, start, e));
                        }
                    } else {
                        code.push_str(line);
                        code.push('\n');
                        current = Some((start, code));
                    }
                }
            }
        }
    }
    assert!(blocks > 0, "no ```meng blocks found in docs/guide");
    assert!(failures.is_empty(), "guide examples that do not compile:\n{}", failures.join("\n"));
}

fn check(dir: &Path, code: &str) -> Result<(), String> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_meng"))
        .args(["check", "-"])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    child.stdin.take().unwrap().write_all(code.as_bytes()).unwrap();
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
    }
}

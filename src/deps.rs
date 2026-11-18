//! Dependencies: `deps.meow`, `meng deps`, `meng add`.
//!
//! - `deps.meow` lists one `name:version` per line (`% comments %` allowed).
//! - A registry is a git repository with `index.meow`: one `name = git-url` per line.
//!   Default: simenglabs/meng-registry; override with `MENG_REGISTRY` (git URL or folder).
//! - A package is a git repository tagged `v<version>`, with `lib.meng` as its entry
//!   file and optionally its own `deps.meow`.
//! - Packages are installed into `meng_deps/<name>/` of the project (flat: a
//!   package's own dependencies go there too) and imported with `use "name" meow Function;`.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::parser::strip_comments;

pub const DEPS_FILE: &str = "deps.meow";
pub const DEPS_DIR: &str = "meng_deps";
/// Entry file of a package.
pub const PACKAGE_ENTRY: &str = "lib.meng";
const DEFAULT_REGISTRY: &str = "https://github.com/simenglabs/meng-registry.git";
/// Written into an installed package, so `meng deps` knows its version.
const VERSION_MARKER: &str = ".meng-version";

#[derive(Debug, Clone, PartialEq)]
pub struct Dep {
    pub name: String,
    pub version: String,
}

/// `meng add name[:version]` — add or change a dependency, then install.
pub fn add(spec: &str, project: &Path) -> Result<(), String> {
    let (name, version) = parse_spec(spec)?;
    let version = match version {
        Some(v) => v,
        None => {
            let url = Registry::load()?.url(&name)?;
            let latest = latest_version(&url)?;
            println!("[..] Latest version of {} is {}", name, latest);
            latest
        }
    };

    let file = project.join(DEPS_FILE);
    let text = fs::read_to_string(&file).unwrap_or_default();
    let mut found = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| match parse_spec(line.trim()) {
            Ok((n, _)) if n == name => {
                found = true;
                format!("{}:{}", name, version)
            }
            _ => line.to_string(),
        })
        .collect();
    if !found {
        lines.push(format!("{}:{}", name, version));
    }
    fs::write(&file, lines.join("\n") + "\n").map_err(|e| format!("cannot write {}: {}", file.display(), e))?;
    if let Err(e) = sync(project, true) {
        // Keep deps.meow as it was (so it never lists something that cannot be
        // installed), and put meng_deps back in line with it.
        let _ = if text.is_empty() { fs::remove_file(&file) } else { fs::write(&file, &text) };
        if !text.is_empty() {
            let _ = sync(project, false);
        }
        return Err(format!("{} (deps.meow was not changed)", e));
    }
    println!("[OK] {} {}:{}", if found { "Updated" } else { "Added" }, name, version);
    Ok(())
}

/// `meng deps` — install what `deps.meow` asks for (and what those packages need),
/// update changed versions, and remove packages that are no longer listed.
pub fn install(project: &Path) -> Result<(), String> {
    sync(project, true)
}

fn sync(project: &Path, verbose: bool) -> Result<(), String> {
    let say = |line: String| {
        if verbose {
            println!("{}", line);
        }
    };
    let file = project.join(DEPS_FILE);
    let text = fs::read_to_string(&file)
        .map_err(|_| format!("no {} in {}; add a dependency with `meng add name:version`", DEPS_FILE, project.display()))?;
    let direct = parse_deps(&text, DEPS_FILE)?;
    let registry = Registry::load()?;
    let deps_dir = project.join(DEPS_DIR);

    // name -> (version, who asked for it)
    let mut resolved: HashMap<String, (String, String)> = HashMap::new();
    let mut queue: Vec<(Dep, String)> = direct.into_iter().map(|d| (d, DEPS_FILE.to_string())).collect();
    let mut order = Vec::new();
    while !queue.is_empty() {
        let (dep, by) = queue.remove(0);
        if let Some((version, other)) = resolved.get(&dep.name) {
            if *version != dep.version {
                return Err(format!(
                    "version conflict: {} {} is required by {}, but {} needs {}",
                    dep.name, version, other, by, dep.version
                ));
            }
            continue;
        }
        let url = registry.url(&dep.name)?;
        let status = install_one(&dep, &url, &deps_dir)?;
        say(format!("[OK] {} {} ({})", dep.name, dep.version, status));
        resolved.insert(dep.name.clone(), (dep.version.clone(), by));
        order.push(dep.name.clone());

        // The package's own dependencies.
        let own = deps_dir.join(&dep.name).join(DEPS_FILE);
        if let Ok(text) = fs::read_to_string(&own) {
            let label = format!("{}/{}", dep.name, DEPS_FILE);
            for d in parse_deps(&text, &label)? {
                queue.push((d, format!("{} {}", dep.name, dep.version)));
            }
        }
    }

    // Remove packages nobody needs any more.
    if let Ok(entries) = fs::read_dir(&deps_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() && !resolved.contains_key(&name) {
                fs::remove_dir_all(entry.path()).map_err(|e| format!("cannot remove {}: {}", entry.path().display(), e))?;
                say(format!("[OK] Removed {} (no longer in {})", name, DEPS_FILE));
            }
        }
    }
    if order.is_empty() {
        say(format!("[OK] No dependencies in {}", DEPS_FILE));
    }
    Ok(())
}

/// Download one package version into `deps_dir/name` unless it is already there.
fn install_one(dep: &Dep, url: &str, deps_dir: &Path) -> Result<&'static str, String> {
    let target = deps_dir.join(&dep.name);
    let marker = target.join(VERSION_MARKER);
    let current = fs::read_to_string(&marker).ok();
    if current.as_deref().map(str::trim) == Some(dep.version.as_str()) {
        return Ok("up to date");
    }

    fs::create_dir_all(deps_dir).map_err(|e| format!("cannot create {}: {}", deps_dir.display(), e))?;
    // Download next to the target first, so a failed download keeps the old version.
    let staging = deps_dir.join(format!(".{}-download", dep.name));
    let _ = fs::remove_dir_all(&staging);
    let tag = format!("v{}", dep.version);
    git(&["clone", "--quiet", "--depth", "1", "--branch", &tag, url, &staging.to_string_lossy()]).map_err(|e| {
        let _ = fs::remove_dir_all(&staging);
        format!("cannot download {} {} from {} (is there a tag {}?): {}", dep.name, dep.version, url, tag, e)
    })?;
    let _ = fs::remove_dir_all(staging.join(".git"));
    if !staging.join(PACKAGE_ENTRY).is_file() {
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("{} {} has no {} (the entry file of a package)", dep.name, dep.version, PACKAGE_ENTRY));
    }
    fs::write(staging.join(VERSION_MARKER), &dep.version).map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(&target);
    fs::rename(&staging, &target).map_err(|e| format!("cannot install {}: {}", dep.name, e))?;
    Ok(if current.is_some() { "updated" } else { "installed" })
}

/// Parse `deps.meow`: `name:version` per line, `% comments %` and blank lines ignored.
pub fn parse_deps(text: &str, label: &str) -> Result<Vec<Dep>, String> {
    let text = strip_comments(text).map_err(|e| format!("{} line {}: {}", label, e.line, e.message))?;
    let mut deps: Vec<Dep> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (name, version) = parse_spec(line).map_err(|e| format!("{} line {}: {}", label, i + 1, e))?;
        let version = version.ok_or_else(|| format!("{} line {}: `{}` needs a version, e.g. {}:0.1.0", label, i + 1, name, name))?;
        if deps.iter().any(|d| d.name == name) {
            return Err(format!("{} line {}: `{}` is listed twice", label, i + 1, name));
        }
        deps.push(Dep { name, version });
    }
    Ok(deps)
}

/// `name` or `name:1.2.3`.
fn parse_spec(spec: &str) -> Result<(String, Option<String>), String> {
    let (name, version) = match spec.split_once(':') {
        Some((n, v)) => (n.trim(), Some(v.trim())),
        None => (spec.trim(), None),
    };
    if !is_package_name(name) {
        return Err(format!("invalid package name `{}` (use lowercase letters, digits, `_` and `-`)", name));
    }
    if let Some(v) = version {
        if !is_version(v) {
            return Err(format!("invalid version `{}` for {} (expected major.minor.patch, e.g. 0.1.0)", v, name));
        }
    }
    Ok((name.to_string(), version.map(str::to_string)))
}

pub fn is_package_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn is_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Highest `vX.Y.Z` tag of a git repository.
fn latest_version(url: &str) -> Result<String, String> {
    let out = git(&["ls-remote", "--tags", url])?;
    out.lines()
        .filter_map(|l| l.split("refs/tags/v").nth(1))
        .filter(|v| is_version(v))
        .max_by_key(|v| v.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>())
        .map(str::to_string)
        .ok_or_else(|| format!("{} has no version tags like v0.1.0", url))
}

/// The package index: name -> git URL.
struct Registry {
    source: String,
    packages: HashMap<String, String>,
}

impl Registry {
    fn load() -> Result<Registry, String> {
        let source = env::var("MENG_REGISTRY").unwrap_or_else(|_| DEFAULT_REGISTRY.to_string());
        let index = if Path::new(&source).join("index.meow").is_file() {
            fs::read_to_string(Path::new(&source).join("index.meow")).map_err(|e| e.to_string())?
        } else {
            let tmp = temp_dir("registry");
            let result = git(&["clone", "--quiet", "--depth", "1", &source, &tmp.to_string_lossy()])
                .and_then(|_| fs::read_to_string(tmp.join("index.meow")).map_err(|_| "it has no index.meow".to_string()));
            let _ = fs::remove_dir_all(&tmp);
            result.map_err(|e| format!("cannot read the package registry {}: {}", source, e))?
        };
        let mut packages = HashMap::new();
        let index = strip_comments(&index).map_err(|e| format!("registry index line {}: {}", e.line, e.message))?;
        for line in index.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let (name, url) = line.split_once('=').ok_or_else(|| format!("invalid registry line `{}` (expected name = url)", line))?;
            packages.insert(name.trim().to_string(), url.trim().to_string());
        }
        Ok(Registry { source, packages })
    }

    fn url(&self, name: &str) -> Result<String, String> {
        self.packages.get(name).cloned().ok_or_else(|| format!("package `{}` is not in the registry ({})", name, self.source))
    }
}

fn git(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0") // never wait for a password
        .output()
        .map_err(|e| format!("git is required for dependencies: {}", e))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().lines().last().unwrap_or("git failed").to_string())
    }
}

fn temp_dir(what: &str) -> PathBuf {
    env::temp_dir().join(format!("meng-{}-{}", what, std::process::id()))
}

/// `meng_deps/<name>/lib.meng` in `dir` or the closest parent folder that has it.
pub fn find_package(dir: &Path, name: &str) -> Option<PathBuf> {
    let start = dir.canonicalize().ok()?;
    start.ancestors().map(|d| d.join(DEPS_DIR).join(name).join(PACKAGE_ENTRY)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_deps_files() {
        let deps = parse_deps("% deps %\nexample_1:0.0.1\n\n  my-lib:1.20.3  % pinned %\n", "deps.meow").unwrap();
        assert_eq!(deps, vec![
            Dep { name: "example_1".into(), version: "0.0.1".into() },
            Dep { name: "my-lib".into(), version: "1.20.3".into() },
        ]);
        assert_eq!(parse_deps("a:1.0", "deps.meow").unwrap_err(), "deps.meow line 1: invalid version `1.0` for a (expected major.minor.patch, e.g. 0.1.0)");
        assert_eq!(parse_deps("Bad:1.0.0", "deps.meow").unwrap_err(), "deps.meow line 1: invalid package name `Bad` (use lowercase letters, digits, `_` and `-`)");
        assert_eq!(parse_deps("a", "deps.meow").unwrap_err(), "deps.meow line 1: `a` needs a version, e.g. a:0.1.0");
        assert_eq!(parse_deps("a:1.0.0\na:2.0.0", "deps.meow").unwrap_err(), "deps.meow line 2: `a` is listed twice");
    }
}

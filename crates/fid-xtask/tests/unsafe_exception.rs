//! Gate for the one named `unsafe` exception (decisions/D-048): the Windows token-file ACL
//! shim `fid-ctrl-acl`, module `ffi`.
//!
//! - every `crates/*/Cargo.toml` except `fid-ctrl-acl` has `[lints] workspace = true`;
//! - `fid-ctrl-acl` denies `unsafe_code` and its `[lints.clippy]` holds every workspace clippy
//!   lint at the same level;
//! - lifting the `unsafe_code` lint (`allow`, `expect` or `warn`) appears only on `mod ffi` in
//!   `crates/fid-ctrl-acl/src/lib.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

const EXCEPTION_CRATE: &str = "fid-ctrl-acl";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn table(path: &Path) -> toml::Table {
    let text = std::fs::read_to_string(path).unwrap();
    text.parse::<toml::Table>().unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn get<'a>(t: &'a toml::Table, keys: &[&str]) -> Option<&'a toml::Value> {
    let (first, rest) = keys.split_first()?;
    let mut v = t.get(*first)?;
    for k in rest {
        v = v.as_table()?.get(*k)?;
    }
    Some(v)
}

fn crate_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root().join("crates"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("Cargo.toml").is_file())
        .collect();
    dirs.sort();
    dirs
}

#[test]
fn every_crate_but_the_exception_uses_the_workspace_lints() {
    let dirs = crate_dirs();
    assert!(dirs.len() > 50, "crate scan found too few crates");
    let mut bad = Vec::new();
    for d in dirs {
        let name = d.file_name().unwrap().to_string_lossy().to_string();
        let t = table(&d.join("Cargo.toml"));
        let workspace = get(&t, &["lints", "workspace"]).and_then(toml::Value::as_bool) == Some(true);
        if name == EXCEPTION_CRATE {
            assert!(!workspace, "{name} must declare its own lints (D-048)");
        } else if !workspace {
            bad.push(name);
        }
    }
    assert!(bad.is_empty(), "crates without `[lints] workspace = true` (D-048 allows only {EXCEPTION_CRATE}): {bad:?}");
}

#[test]
fn the_exception_crate_lints_are_a_superset_of_the_workspace() {
    let ws = table(&root().join("Cargo.toml"));
    let ex = table(&root().join("crates").join(EXCEPTION_CRATE).join("Cargo.toml"));
    let ws_clippy = get(&ws, &["workspace", "lints", "clippy"]).and_then(toml::Value::as_table).unwrap();
    let ex_clippy = get(&ex, &["lints", "clippy"]).and_then(toml::Value::as_table).unwrap();
    assert!(!ws_clippy.is_empty());
    for (lint, level) in ws_clippy {
        assert_eq!(ex_clippy.get(lint), Some(level), "{EXCEPTION_CRATE}: clippy::{lint} must match the workspace");
    }
    assert_eq!(
        get(&ex, &["lints", "clippy", "undocumented_unsafe_blocks"]).and_then(toml::Value::as_str),
        Some("deny")
    );
    assert_eq!(get(&ex, &["lints", "rust", "unsafe_code"]).and_then(toml::Value::as_str), Some("deny"));
    // The workspace itself still forbids it.
    assert_eq!(get(&ws, &["workspace", "lints", "rust", "unsafe_code"]).and_then(toml::Value::as_str), Some("forbid"));
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "target" || n == ".git") {
                continue;
            }
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Lines that lift the `unsafe_code` lint, as (file, 1-based line, text).
fn lifts(paths: &[PathBuf]) -> Vec<(PathBuf, usize, String)> {
    // Built from pieces so this file does not match itself.
    let lint = concat!("unsafe", "_code");
    let mut out = Vec::new();
    for p in paths {
        let Ok(text) = std::fs::read_to_string(p) else { continue };
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
            let lifted = ["allow(", "expect(", "warn("].iter().any(|a| compact.contains(a));
            if compact.contains(lint) && lifted {
                out.push((p.clone(), i + 1, line.to_string()));
            }
        }
    }
    out
}

#[test]
fn unsafe_code_is_allowed_only_on_mod_ffi_of_the_exception_crate() {
    let mut files = Vec::new();
    rust_files(&root().join("crates"), &mut files);
    if root().join("vendor").is_dir() {
        rust_files(&root().join("vendor"), &mut files);
    }
    assert!(files.len() > 100, "file scan found too few Rust files");
    let shim = root().join("crates").join(EXCEPTION_CRATE).join("src").join("lib.rs");
    let shim_lines: Vec<String> = std::fs::read_to_string(&shim).unwrap().lines().map(String::from).collect();
    let mut found_ffi = 0;
    let mut bad = Vec::new();
    for (path, line, text) in lifts(&files) {
        let next = shim_lines.get(line).map(|l| l.trim().to_string()).unwrap_or_default();
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let allowed_site = path == shim && compact == concat!("#[allow(unsafe", "_code)]") && next == "mod ffi {";
        if allowed_site {
            found_ffi += 1;
        } else {
            bad.push(format!("{}:{line}: {}", path.display(), text.trim()));
        }
    }
    assert!(bad.is_empty(), "unsafe_code lifted outside D-048's scope:\n{}", bad.join("\n"));
    assert_eq!(found_ffi, 1, "expected exactly one allow on `mod ffi` in {}", shim.display());
}

#[test]
fn the_scan_catches_lifts_in_any_form() {
    let dir = std::env::temp_dir().join(format!("fid-xtask-unsafe-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let lint = concat!("unsafe", "_code");
    let cases = [
        format!("#![allow({lint})]"),
        format!("#[allow(dead_code, {lint})]"),
        format!("#[ expect( {lint} ) ]"),
        format!("#![warn({lint})]"),
    ];
    let mut paths = Vec::new();
    for (i, c) in cases.iter().enumerate() {
        let p = dir.join(format!("case{i}.rs"));
        std::fs::write(&p, format!("{c}\nfn f() {{}}\n")).unwrap();
        paths.push(p);
    }
    let clean = dir.join("clean.rs");
    std::fs::write(&clean, format!("#![forbid({lint})]\n// {lint} is forbidden\n")).unwrap();
    paths.push(clean);
    let hits = lifts(&paths);
    assert_eq!(hits.len(), cases.len(), "{hits:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

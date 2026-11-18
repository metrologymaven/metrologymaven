//! Test inventory for the conformance gate (spec node `IO.DMIS.CONF`).
//!
//! The inventory lists every runnable test as `<package>::<test target>::<test name>: test`.
//! It is built from `cargo test --workspace --no-run --message-format=json` (which names each
//! test executable with its package manifest and target) by running each executable with
//! `--list` and with `--list --ignored`: ignored tests are left out, because a test that does
//! not run cannot back a support claim.
//!
//! The gate checks that a named test exists and runs; it does not check that the test proves
//! the claim. Reviewers must check that.

use crate::conf::{parse_inventory, ConfError};
use fid_canon::{parse, CValue};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// One test executable from the cargo build messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestBinary {
    /// The package's `Cargo.toml`.
    pub manifest_path: PathBuf,
    /// The target name, as cargo reports it.
    pub target: String,
    pub executable: PathBuf,
}

fn get<'a>(v: &'a CValue, key: &str) -> Option<&'a CValue> {
    match v {
        CValue::Obj(m) => m.get(key),
        _ => None,
    }
}

fn get_str<'a>(v: &'a CValue, key: &str) -> Option<&'a str> {
    match get(v, key) {
        Some(CValue::Str(s)) => Some(s),
        _ => None,
    }
}

/// The test executables in a `cargo ... --message-format=json` stream, in stream order.
pub fn test_binaries(cargo_json: &str) -> Result<Vec<TestBinary>, ConfError> {
    let mut out = Vec::new();
    for (i, line) in cargo_json.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v = parse(line.as_bytes()).map_err(|e| ConfError::Malformed(format!("cargo JSON line {}: {e}", i + 1)))?;
        if get_str(&v, "reason") != Some("compiler-artifact") {
            continue;
        }
        if !matches!(get(&v, "profile").and_then(|p| get(p, "test")), Some(CValue::Bool(true))) {
            continue;
        }
        let (Some(exe), Some(manifest), Some(target)) = (
            get_str(&v, "executable"),
            get_str(&v, "manifest_path"),
            get(&v, "target").and_then(|t| get_str(t, "name")),
        ) else {
            continue;
        };
        out.push(TestBinary {
            manifest_path: PathBuf::from(manifest),
            target: target.to_string(),
            executable: PathBuf::from(exe),
        });
    }
    Ok(out)
}

/// Qualified IDs of the tests in `list` (output of `<exe> --list`) that are not in `ignored`
/// (output of `<exe> --list --ignored`).
pub fn qualified_ids(package: &str, target: &str, list: &str, ignored: &str) -> BTreeSet<String> {
    let ignored = parse_inventory(ignored);
    parse_inventory(list).difference(&ignored).map(|t| format!("{package}::{target}::{t}")).collect()
}

/// The inventory file text: one `<id>: test` line per ID, sorted.
pub fn inventory_text(ids: &BTreeSet<String>) -> String {
    ids.iter().map(|id| format!("{id}: test\n")).collect()
}

/// The `[package] name` of a manifest.
pub fn package_name(manifest: &Path) -> Result<String, ConfError> {
    let text = std::fs::read_to_string(manifest).map_err(|e| ConfError::Io(format!("{}: {e}", manifest.display())))?;
    let t: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| ConfError::Malformed(format!("{}: {}", manifest.display(), e.message())))?;
    match t.get("package").and_then(|p| p.get("name")) {
        Some(toml::Value::String(n)) => Ok(n.clone()),
        _ => Err(ConfError::Malformed(format!("{}: no [package] name", manifest.display()))),
    }
}

fn run_list(exe: &Path, ignored: bool) -> Result<String, ConfError> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--list");
    if ignored {
        cmd.arg("--ignored");
    }
    let out = cmd.output().map_err(|e| ConfError::Io(format!("{}: {e}", exe.display())))?;
    if !out.status.success() {
        return Err(ConfError::Io(format!("{} --list failed", exe.display())));
    }
    String::from_utf8(out.stdout).map_err(|_| ConfError::Io(format!("{}: --list output is not UTF-8", exe.display())))
}

/// Runs every test executable in the cargo JSON stream and returns the inventory text.
pub fn build_inventory(cargo_json: &str) -> Result<String, ConfError> {
    let mut ids = BTreeSet::new();
    for b in test_binaries(cargo_json)? {
        let package = package_name(&b.manifest_path)?;
        let list = run_list(&b.executable, false)?;
        let ignored = run_list(&b.executable, true)?;
        ids.extend(qualified_ids(&package, &b.target, &list, &ignored));
    }
    Ok(inventory_text(&ids))
}

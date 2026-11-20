//! Shared helper for the conformance tests: the crate's own test inventory, read from the
//! test sources.

use std::path::Path;

/// `fid-io-dmis::<target>::<test>: test` for every non-ignored top-level `#[test]` function
/// in `tests/*.rs`, in the format of `cargo test -- --list`. CI checks the same claims against
/// the real `--list` inventory of the whole workspace; this lets a single-crate test run check
/// that each declared test exists without compiling the workspace.
pub fn crate_test_inventory() -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();
    let mut out = String::new();
    for path in files {
        let target = path.file_stem().unwrap().to_str().unwrap().to_string();
        let text = std::fs::read_to_string(&path).unwrap();
        let (mut is_test, mut ignored) = (false, false);
        for line in text.lines() {
            let t = line.trim_start();
            if line.starts_with("#[test]") {
                is_test = true;
            } else if line.starts_with("#[ignore") {
                ignored = true;
            } else if line.starts_with("#[") || t.starts_with("///") {
                continue;
            } else {
                if is_test && !ignored {
                    if let Some(name) = line.strip_prefix("fn ").and_then(|r| r.split('(').next()) {
                        out.push_str(&format!("fid-io-dmis::{target}::{name}: test\n"));
                    }
                }
                is_test = false;
                ignored = false;
            }
        }
    }
    out
}

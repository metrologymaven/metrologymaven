//! KRN.MAN.WASM AC-4: the shim source has no `unsafe` and calls into the kernel only through
//! `evaluate`. The scan rules are pinned in the leaf's Design:
//!
//! - no source file of the crate contains the word `unsafe` (comments included);
//! - the read-call-write files (`main.rs`, `shim.rs`, `call.rs`) import from `fid_kci` only
//!   the items in [`ALLOWED_ITEMS`], and the only function among them is `evaluate`;
//! - in those files, `Type::function(` calls on kernel types are only the constructors in
//!   [`ALLOWED_CONSTRUCTORS`] (building the call and the empty reference table).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

const SHIM_FILES: [&str; 3] = ["main.rs", "shim.rs", "call.rs"];
const ALLOWED_ITEMS: [&str; 6] = ["AlgorithmId", "AlgorithmTable", "CardRegistry", "Digest", "KernelCall", "evaluate"];
const ALLOWED_CONSTRUCTORS: [&str; 4] =
    ["AlgorithmId::parse", "AlgorithmTable::new", "CardRegistry::default", "Digest::parse"];

fn src(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(name)).unwrap()
}

fn code_only(text: &str) -> String {
    text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
}

/// Items named in `use fid_kci::...;` statements and in inline `fid_kci::<item>` paths.
fn kernel_items(code: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut rest = code;
    while let Some(i) = rest.find("fid_kci::") {
        rest = &rest[i + "fid_kci::".len()..];
        let group = if let Some(inner) = rest.strip_prefix('{') {
            &inner[..inner.find('}').unwrap()]
        } else {
            &rest[..rest.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(rest.len())]
        };
        items.extend(group.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    }
    items
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[test]
fn ac4_no_unsafe_anywhere_in_the_crate_sources() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut n = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("unsafe"), "{} mentions unsafe", path.display());
        n += 1;
    }
    assert_eq!(n, 5, "lib.rs, main.rs, shim.rs, call.rs, stand_in.rs");
}

#[test]
fn ac4_the_only_kernel_function_the_shim_calls_is_evaluate() {
    let mut functions = Vec::new();
    for f in SHIM_FILES {
        let code = code_only(&src(f));
        for item in kernel_items(&code) {
            assert!(ALLOWED_ITEMS.contains(&item.as_str()), "{f}: imports fid_kci::{item}");
            if item.starts_with(|c: char| c.is_ascii_lowercase()) {
                functions.push(item);
            }
        }
        for ty in ["AlgorithmId", "AlgorithmTable", "CardRegistry", "Digest", "KernelCall", "KernelResult"] {
            let pat = format!("{ty}::");
            let mut rest = code.as_str();
            while let Some(i) = rest.find(&pat) {
                let before_ok = rest[..i].chars().next_back().is_none_or(|c| !is_ident(c));
                rest = &rest[i + pat.len()..];
                let method: String = rest.chars().take_while(|&c| is_ident(c)).collect();
                if before_ok && rest[method.len()..].starts_with('(') {
                    let call = format!("{ty}::{method}");
                    assert!(ALLOWED_CONSTRUCTORS.contains(&call.as_str()), "{f}: calls {call}");
                }
            }
        }
    }
    assert_eq!(functions, ["evaluate"]);
    // And `evaluate` is actually called, once, in shim.rs.
    assert_eq!(code_only(&src("shim.rs")).matches("evaluate(").count(), 1);
}

#[test]
fn the_scanner_sees_what_it_should() {
    assert_eq!(
        kernel_items("use fid_kci::{evaluate, AlgorithmTable};\nlet x = fid_kci::sha256(b);"),
        ["evaluate", "AlgorithmTable", "sha256"]
    );
    assert_eq!(code_only("// unsafe\nlet a = 1;"), "let a = 1;");
}

//! Offline dependency isolation check for `fid-verify` (`REQ-TRUST.VERIFY.CLI-001` AC-6).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::Command;

const FORBIDDEN_CRATES: &[&str] = &[
    "libsqlite3-sys",
    "rusqlite",
    "fid-bus",
    "fid-bus-api",
    "fid-store",
    "fid-esig",
    "fid-ctrl-acl",
    "fid-net",
    "fid-render",
    "fid-gpu-select",
    "fid-cli",
    "fid-mcp",
    "reqwest",
    "hyper",
    "rustls",
    "native-tls",
    "tokio",
    "ureq",
    "surf",
    "h2",
    "quinn",
    "winapi",
    "windows-sys",
    "wgpu",
    "egui",
];

#[test]
fn fid_verify_has_no_network_sqlite_bus_or_gui_dependencies() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let out = Command::new(cargo)
        .current_dir(&root)
        .args(["tree", "--locked", "--offline", "-p", "fid-verify", "-e", "normal", "--prefix", "none"])
        .output()
        .unwrap();
    assert!(out.status.success(), "cargo tree failed: {}", String::from_utf8_lossy(&out.stderr));
    let tree = String::from_utf8(out.stdout).unwrap();
    let names: Vec<&str> = tree.lines().filter_map(|l| l.split_whitespace().next()).collect();
    assert!(!names.is_empty(), "empty dependency tree");
    for forbidden in FORBIDDEN_CRATES {
        assert!(!names.contains(forbidden), "fid-verify dependency tree contains forbidden crate {forbidden}:\n{tree}");
    }
}

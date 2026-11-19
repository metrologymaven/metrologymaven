//! The real kernel closure passes the implementation-digest scan (KRN.MAN.MANI.IMPL,
//! REQ-KRN.MAN.MANI.IMPL-001).
//!
//! Every crate in the `num` and `kernel` layers of `layers.toml` is registered for one synthetic
//! algorithm and the digests are computed over this checkout, from a fresh `cargo metadata` and
//! `git ls-files`, by the crate's public API. So every hashed source file of every crate those
//! crates reach is scanned by the same fail-closed scanner the manifest generator uses: a
//! restricted identifier (`include`, `include_str`, `include_bytes`, `env`, `option_env`,
//! `path`) used outside its exact allowed shapes, a build script or a proc macro in the closure
//! fails this test with `MetadataInvalid`.
//!
//! It runs `cargo metadata` and `git ls-files`, so it is not built under WASI.
#![cfg(not(target_os = "wasi"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fid_kci::AlgorithmId;
use fid_manifest_gen::implclosure::{git_tracked_files, impl_digests_from_files, ImplError, Registration};
use toml::{Table, Value};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_toml(name: &str) -> Table {
    fs::read_to_string(workspace().join(name)).unwrap().parse().unwrap()
}

/// The workspace crates in the `num` and `kernel` layers, sorted.
fn kernel_crates() -> Vec<String> {
    let layers = read_toml("layers.toml");
    let Some(Value::Table(crates)) = layers.get("crates") else { panic!("layers.toml: no [crates]") };
    let mut out: Vec<String> = crates
        .iter()
        .filter(|(_, layer)| matches!(layer.as_str(), Some("num" | "kernel")))
        .map(|(name, _)| name.clone())
        .collect();
    out.sort();
    out
}

/// The third-party crates of `kernel-allowlist.toml` (none today).
fn third_party_allowlist() -> Vec<String> {
    let allow = read_toml("kernel-allowlist.toml");
    match allow.get("allow") {
        None => Vec::new(),
        Some(Value::Array(entries)) => entries
            .iter()
            .map(|e| e.get("name").and_then(Value::as_str).expect("allow entry without a name").to_string())
            .collect(),
        Some(_) => panic!("kernel-allowlist.toml: `allow` is not an array"),
    }
}

/// `cargo metadata --format-version 1` of this workspace, written to a file.
fn metadata_file() -> PathBuf {
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(workspace().join("Cargo.toml"))
        .output()
        .unwrap();
    assert!(out.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr));
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fid-manifest-gen-kernel-closure");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("metadata.json");
    fs::write(&file, &out.stdout).unwrap();
    file
}

/// A synthetic algorithm ID for the `i`-th kernel crate.
fn algorithm(i: usize) -> AlgorithmId {
    AlgorithmId::parse(&format!("closure.scan.crate{i}@1.0.0")).unwrap()
}

#[test]
fn every_kernel_closure_crate_passes_the_scan() {
    let crates = kernel_crates();
    assert!(crates.iter().any(|c| c == "fid-kci"), "fid-kci must be in the kernel layer");
    let algorithms: Vec<AlgorithmId> = (0..crates.len()).map(algorithm).collect();
    let registrations: Vec<Registration> = crates
        .iter()
        .enumerate()
        .map(|(i, c)| Registration { algorithm: algorithm(i), crate_name: c.clone() })
        .collect();
    let mut allowlist = crates.clone();
    allowlist.extend(third_party_allowlist());

    let ws = workspace();
    let tracked = git_tracked_files(&ws).unwrap();
    let result = impl_digests_from_files(&metadata_file(), &ws, tracked, &algorithms, &registrations, &allowlist);
    let table = match result {
        Ok(table) => table,
        Err(e @ ImplError::MetadataInvalid { .. }) => {
            panic!("a kernel-closure crate is refused by the implclosure scan: {e}")
        }
        Err(e) => panic!("implementation digests over the kernel closure failed: {e}"),
    };
    assert_eq!(table.entries().len(), crates.len());
    // Each registering crate is in its own closure, so every kernel crate was scanned.
    for (i, c) in crates.iter().enumerate() {
        let entry = table.entries().iter().find(|e| e.algorithm == algorithm(i)).unwrap();
        assert!(entry.closure.iter().any(|r| r.name == *c), "{c} missing from its own closure");
    }
}

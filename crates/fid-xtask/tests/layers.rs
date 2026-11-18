//! Acceptance criteria of spec node ECO.REPO.LAYERS (AC-1 .. AC-6).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::{parse_layer_map, run};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/layers")
}

fn check_fixture(name: &str) {
    let ws = fixtures().join(name);
    let want: Vec<String> =
        std::fs::read_to_string(ws.join("expected.txt")).unwrap().lines().map(String::from).collect();
    let got = run(&ws, &ws.join("layers.toml")).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(got, want, "{name}");
    // AC-6: the output is identical across runs.
    assert_eq!(run(&ws, &ws.join("layers.toml")).unwrap(), got, "{name}: not deterministic");
}

/// AC-1
#[test]
fn ac1_ok() {
    check_fixture("ok");
}

/// AC-2, and REQ-005: dev-dependencies follow the same upward rule.
#[test]
fn ac2_upward() {
    check_fixture("upward");
    check_fixture("upward-dev");
}

/// AC-3: the path goes through the third-party crate.
#[test]
fn ac3_transitive() {
    check_fixture("transitive");
}

/// AC-4
#[test]
fn ac4_unassigned_and_unknown() {
    check_fixture("unassigned");
    check_fixture("unknown");
}

/// AC-5, and REQ-003 for tool crates.
#[test]
fn ac5_test_support_and_tool() {
    check_fixture("testsupport");
    check_fixture("testsupport-normal");
    check_fixture("tool");
}

#[test]
fn bad_maps_are_refused() {
    for (text, why) in [
        ("[crates]\n\"a\" = \"num\"\n", "missing order"),
        ("order = [\"num\"]\n[crates]\n\"a\" = \"gpu\"\n", "unknown layer"),
        ("order = [\"num\"]\n[crates]\n\"a\" = \"num\"\n\"a\" = \"num\"\n", "assigned twice"),
        ("order = [\"num\"]\n[extra]\n", "unknown section"),
    ] {
        let err = parse_layer_map(text).unwrap_err();
        assert!(err.starts_with("LAYER-MAP") && err.contains(why), "{err}");
    }
}

/// The repository's own workspace obeys its layer map.
#[test]
fn repository_layers_are_clean() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let findings = run(&root, &root.join("layers.toml")).unwrap();
    assert!(findings.is_empty(), "{findings:?}");
}

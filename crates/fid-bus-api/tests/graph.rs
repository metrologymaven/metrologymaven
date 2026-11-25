//! Acceptance criteria AC-1 and AC-6 of spec node CORE.BUS.API: the crate's place in the
//! crate graph, and its wasm32 build.
//!
//! AC-1's `cargo xtask layers` and `cargo xtask lint-crate-graph` run as their own CI steps
//! ("Crate layering", "No side doors"); this file checks the inputs they read for this crate.
//! It runs `cargo`, so it is not built under WASI.
#![cfg(not(target_os = "wasi"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use std::path::{Path, PathBuf};
use std::process::Command;
use toml::{Table, Value};

const CRATE: &str = "fid-bus-api";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_toml(name: &str) -> Table {
    std::fs::read_to_string(workspace().join(name)).unwrap().parse().unwrap()
}

fn get<'a>(v: &'a CValue, key: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(key).unwrap_or_else(|| panic!("no key {key}")),
        other => panic!("not an object: {other:?}"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("not an array: {other:?}"),
    }
}

fn str_of(v: &CValue) -> Option<&str> {
    match v {
        CValue::Str(s) => Some(s),
        _ => None,
    }
}

/// AC-1, REQ-001: the normal dependencies are exactly `fid-canon` and `fid-types`, both the
/// workspace's own path crates.
#[test]
fn ac1_normal_dependencies_are_exactly_canon_and_types() {
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps", "--manifest-path"])
        .arg(workspace().join("Cargo.toml"))
        .output()
        .unwrap();
    assert!(out.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr));
    let meta = fid_canon::parse(&out.stdout).unwrap();
    let pkg = arr(get(&meta, "packages"))
        .iter()
        .find(|p| str_of(get(p, "name")) == Some(CRATE))
        .unwrap_or_else(|| panic!("{CRATE} is not a workspace package"));
    let mut normal: Vec<(String, bool)> = arr(get(pkg, "dependencies"))
        .iter()
        .filter(|d| matches!(get(d, "kind"), CValue::Null))
        .map(|d| {
            let name = str_of(get(d, "name")).unwrap().to_string();
            let is_path = matches!(get(d, "source"), CValue::Null);
            (name, is_path)
        })
        .collect();
    normal.sort();
    assert_eq!(normal, [("fid-canon".to_string(), true), ("fid-types".to_string(), true)]);
}

/// AC-1, REQ-002 and REQ-003: layer `model`; the `bus_api` door of CORE.BUS.LINT and one of
/// its `[core]` crates.
#[test]
fn ac1_layer_and_lint_allowlist() {
    let layers = read_toml("layers.toml");
    let Some(Value::Table(crates)) = layers.get("crates") else { panic!("layers.toml: no [crates]") };
    assert_eq!(crates.get(CRATE).and_then(Value::as_str), Some("model"));

    let allow = read_toml("lint-allowlist.toml");
    let bus_api = allow.get("surfaces").and_then(|s| s.get("bus_api")).and_then(Value::as_str);
    assert_eq!(bus_api, Some(CRATE));
    let core: Vec<&str> = allow
        .get("core")
        .and_then(|c| c.get("crates"))
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(core.contains(&CRATE), "{core:?}");
}

/// AC-6, REQ-011: `cargo build -p fid-bus-api --target wasm32-unknown-unknown` succeeds.
///
/// Every CI test job installs that target. Where it is missing outside CI (a developer
/// machine), the build cannot run and the test says so instead of failing; under CI
/// (`CI` set) a missing target fails the test.
#[test]
fn ac6_builds_for_wasm32_unknown_unknown() {
    let target_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fid-bus-api-wasm32");
    let out = Command::new(env!("CARGO"))
        .args(["build", "--locked", "-p", CRATE, "--target", "wasm32-unknown-unknown", "--manifest-path"])
        .arg(workspace().join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() && stderr.contains("target may not be installed") && std::env::var_os("CI").is_none() {
        eprintln!("skipped: the wasm32-unknown-unknown target is not installed here (CI runs this build)");
        return;
    }
    assert!(out.status.success(), "wasm32-unknown-unknown build failed:\n{stderr}");
    let rlib = target_dir.join("wasm32-unknown-unknown/debug/libfid_bus_api.rlib");
    assert!(rlib.is_file(), "no {}", rlib.display());
}

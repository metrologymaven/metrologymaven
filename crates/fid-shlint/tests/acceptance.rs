//! Acceptance criteria of spec node VIZ.RENDER.FRAME.SHLINT (AC-1 .. AC-4) and REQ-004.
//! Fixtures under `fixtures/viz/shlint/` are hand-written tiny crates; the expected output
//! below is hand-labelled from the spec's rules, never produced by running this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_shlint::{run, Output};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/viz/shlint").join(name)
}

fn lint(name: &str) -> Output {
    let dir = fixture(name);
    let args: Vec<String> = vec![
        "--crate-dir".into(),
        dir.to_string_lossy().into_owned(),
        "--metadata".into(),
        dir.join("metadata.json").to_string_lossy().into_owned(),
    ];
    run(&args)
}

#[test]
fn ac1_ok_fixture_exits_0() {
    let out = lint("ok");
    assert_eq!(out.stderr, "");
    assert_eq!(out.stdout, "fid-shlint: ok\n");
    assert_eq!(out.code, 0);
}

#[test]
fn ac2_unlisted_module_exits_1_naming_the_module() {
    let out = lint("unlisted");
    assert_eq!(out.stderr, "");
    assert_eq!(
        out.stdout,
        "shaders/overlay.wgsl: [SHLINT-001/unlisted-module] WGSL module is not listed in shaders/manifest.toml\n\
         fid-shlint: 1 finding(s)\n"
    );
    assert_eq!(out.code, 1);
}

#[test]
fn ac3_readback_of_storage_buffer_exits_1_naming_file_and_line() {
    let out = lint("readback");
    assert_eq!(out.stderr, "");
    assert_eq!(
        out.stdout,
        "src/readback.rs:6: [SHLINT-002/readback] maps `points` for reading, which shaders/manifest.toml \
         classifies as input, not as a colour, depth or ID target\n\
         fid-shlint: 1 finding(s)\n"
    );
    assert_eq!(out.code, 1);
}

#[test]
fn ac4_transitive_winit_exits_1_naming_the_crate_path() {
    let out = lint("toolkit");
    assert_eq!(out.stderr, "");
    assert_eq!(
        out.stdout,
        "fid-render -> fid-window-host -> winit: [SHLINT-003/toolkit] fid-render depends on UI-toolkit crate winit\n\
         fid-shlint: 1 finding(s)\n"
    );
    assert_eq!(out.code, 1);
}

#[test]
fn allowed_toolkit_is_reported_but_passes() {
    let out = lint("allowed");
    assert_eq!(out.stderr, "");
    assert_eq!(
        out.stdout,
        "fid-render -> fid-window-host -> winit: [SHLINT-003/toolkit-allowed] fid-render depends on UI-toolkit \
         crate winit, allowed in shaders/manifest.toml: fixture: window hosting is under review\n\
         fid-shlint: ok (1 informational)\n"
    );
    assert_eq!(out.code, 0);
}

#[test]
fn metadata_for_another_crate_dir_exits_2() {
    let out = run(&[
        "--crate-dir".to_string(),
        fixture("ok").to_string_lossy().into_owned(),
        "--metadata".to_string(),
        fixture("toolkit").join("metadata.json").to_string_lossy().into_owned(),
    ]);
    assert_eq!(out.code, 2);
    assert_eq!(out.stdout, "");
    assert!(out.stderr.starts_with("fid-shlint: error: MetadataMismatch: manifest_path Cargo.toml is not in --crate-dir "), "{}", out.stderr);
}

#[test]
fn output_is_deterministic() {
    for name in ["ok", "unlisted", "readback", "toolkit", "allowed"] {
        assert_eq!(lint(name), lint(name), "{name}");
    }
}

#[test]
fn usage_errors_exit_2() {
    let ok = fixture("ok");
    let meta = ok.join("metadata.json").to_string_lossy().into_owned();
    let dir = ok.to_string_lossy().into_owned();
    for args in [
        vec![],
        vec!["--crate-dir".to_string(), dir.clone()],
        vec!["--metadata".to_string(), meta.clone()],
        vec!["--crate-dir".to_string(), dir.clone(), "--metadata".to_string(), meta.clone(), "--bogus".to_string()],
        vec!["--crate-dir".to_string(), dir.clone(), "--crate-dir".to_string(), dir.clone()],
        vec!["--crate-dir".to_string()],
    ] {
        let out = run(&args);
        assert_eq!(out.code, 2, "{args:?}");
        assert!(out.stderr.starts_with("fid-shlint: error: Usage: "), "{}", out.stderr);
        assert_eq!(out.stdout, "");
    }
}

#[test]
fn missing_manifest_or_metadata_fails_closed() {
    let ok = fixture("ok");
    // The fixture root has no shaders/manifest.toml.
    let out = run(&[
        "--crate-dir".to_string(),
        fixture("").to_string_lossy().into_owned(),
        "--metadata".to_string(),
        ok.join("metadata.json").to_string_lossy().into_owned(),
    ]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.starts_with("fid-shlint: error: ManifestMalformed: "), "{}", out.stderr);

    let out = run(&[
        "--crate-dir".to_string(),
        ok.to_string_lossy().into_owned(),
        "--metadata".to_string(),
        ok.join("no-such-metadata.json").to_string_lossy().into_owned(),
    ]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.starts_with("fid-shlint: error: Unreadable: "), "{}", out.stderr);

    // A metadata document for another package.
    let out = run(&[
        "--crate-dir".to_string(),
        ok.to_string_lossy().into_owned(),
        "--metadata".to_string(),
        ok.join("metadata.json").to_string_lossy().into_owned(),
        "--package".to_string(),
        "fid-viewer".to_string(),
    ]);
    assert_eq!(out.code, 2);
    assert_eq!(out.stderr, "fid-shlint: error: MetadataMalformed: package fid-viewer is not a workspace member\n");
}

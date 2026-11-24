//! Unit and CLI integration tests for `cargo xtask fmt` (`crates/fid-xtask/src/fmt.rs`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::fmt::{
    collect_rs_files, estimate_cargo_pkg_bytes, estimate_file_arg_bytes, find_rustfmt_config_for_dir,
    parse_workspace_metadata, plan_batches, FmtBatch, PackageFmtInfo,
};
use std::path::{Path, PathBuf};
use std::process::Command;

fn pkg(name: &str, files: &[&str]) -> PackageFmtInfo {
    let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
    let estimated_bytes = paths.iter().map(|p| estimate_file_arg_bytes(p)).sum();
    let pkg_dir = PathBuf::from(format!("crates/{name}"));
    PackageFmtInfo { name: name.to_string(), edition: "2021".to_string(), pkg_dir, files: paths, estimated_bytes }
}

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("fid-xtask-fmt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, rel: &str, contents: &str) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, contents).unwrap();
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fid-xtask")).arg("fmt").args(args).output().unwrap()
}

#[test]
fn plan_batches_packs_small_crates_together() {
    let packages = vec![pkg("crate-a", &["crates/a/src/lib.rs"]), pkg("crate-b", &["crates/b/src/lib.rs"])];
    let total = estimate_cargo_pkg_bytes(&packages[0]) + estimate_cargo_pkg_bytes(&packages[1]);
    let batches = plan_batches(&packages, total + 10);
    assert_eq!(
        batches,
        vec![FmtBatch::CargoFmt {
            packages: vec!["crate-a".to_string(), "crate-b".to_string()],
            estimated_bytes: total,
        }]
    );
}

#[test]
fn plan_batches_splits_multi_crate_workspace_under_budget() {
    let p1 = pkg("crate-a", &["crates/crate-a/src/lib.rs", "crates/crate-a/src/one.rs"]);
    let p2 = pkg("crate-b", &["crates/crate-b/src/lib.rs", "crates/crate-b/src/two.rs"]);
    let p3 = pkg("crate-c", &["crates/crate-c/src/lib.rs", "crates/crate-c/src/three.rs"]);
    // Budget fits one package (including its `-p <pkg>` flag) per batch, forcing 3 CargoFmt batches.
    let budget = estimate_cargo_pkg_bytes(&p1).max(estimate_cargo_pkg_bytes(&p2)).max(estimate_cargo_pkg_bytes(&p3));
    let batches = plan_batches(&[p1.clone(), p2.clone(), p3.clone()], budget);
    assert_eq!(batches.len(), 3, "expected 3 batches, got {batches:?}");
    for b in &batches {
        match b {
            FmtBatch::CargoFmt { packages, estimated_bytes } => {
                assert_eq!(packages.len(), 1);
                assert!(*estimated_bytes <= budget);
            }
            other => panic!("expected CargoFmt batch, got {other:?}"),
        }
    }
}

#[test]
fn plan_batches_accounts_for_cargo_pkg_flags() {
    let packages = vec![pkg("crate-one", &[]), pkg("crate-two", &[]), pkg("crate-three", &[])];
    // Each zero-file crate contributes `name.len() + 4` bytes (13..15 bytes), so budget 28 fits 2 then 1.
    let batches = plan_batches(&packages, 28);
    assert_eq!(
        batches,
        vec![
            FmtBatch::CargoFmt {
                packages: vec!["crate-one".to_string(), "crate-two".to_string()],
                estimated_bytes: 26,
            },
            FmtBatch::CargoFmt { packages: vec!["crate-three".to_string()], estimated_bytes: 15 },
        ]
    );
}

#[test]
fn plan_batches_chunks_oversized_single_crate_into_direct_rustfmt() {
    let small_before = pkg("crate-a", &["crates/a/src/lib.rs"]);
    let oversized = pkg(
        "crate-big",
        &[
            "crates/crate-big/src/a.rs",
            "crates/crate-big/src/b.rs",
            "crates/crate-big/src/c.rs",
            "crates/crate-big/src/d.rs",
        ],
    );
    let small_after = pkg("crate-z", &["crates/z/src/lib.rs"]);

    // Budget allows small crates (~25 bytes) and ~1-2 files of crate-big, but not all 4 files (~116 bytes).
    let budget = 60;
    let batches = plan_batches(&[small_before, oversized, small_after], budget);
    assert_eq!(batches.len(), 4, "batches: {batches:?}");
    assert!(matches!(&batches[0], FmtBatch::CargoFmt { packages, .. } if packages == &["crate-a"]));
    assert!(matches!(&batches[1], FmtBatch::DirectRustfmt { package, files, estimated_bytes, .. }
        if package == "crate-big" && files.len() == 2 && *estimated_bytes <= budget));
    assert!(matches!(&batches[2], FmtBatch::DirectRustfmt { package, files, estimated_bytes, .. }
        if package == "crate-big" && files.len() == 2 && *estimated_bytes <= budget));
    assert!(matches!(&batches[3], FmtBatch::CargoFmt { packages, .. } if packages == &["crate-z"]));
}

#[test]
fn plan_batches_makes_forward_progress_when_single_path_exceeds_budget() {
    let huge_path_pkg = pkg("crate-long", &["crates/very_long_package_directory_name/src/extremely_long_module.rs"]);
    let batches = plan_batches(&[huge_path_pkg], 10);
    assert_eq!(batches.len(), 1);
    match &batches[0] {
        FmtBatch::DirectRustfmt { files, .. } => assert_eq!(files.len(), 1),
        other => panic!("expected DirectRustfmt, got {other:?}"),
    }
}

#[test]
fn collect_rs_files_skips_target_hidden_and_nested_subpackages() {
    let tmp = TempDirGuard::new("collect");
    tmp.write("src/lib.rs", "pub fn ok() {}\n");
    tmp.write("tests/it.rs", "#[test] fn it() {}\n");
    tmp.write("target/debug/gen.rs", "ignored\n");
    tmp.write(".git/hooks/x.rs", "ignored\n");
    tmp.write("subpkg/Cargo.toml", "[package]\nname = \"subpkg\"\nversion = \"0.1.0\"\n");
    tmp.write("subpkg/src/lib.rs", "ignored\n");

    let got = collect_rs_files(tmp.path()).unwrap();
    let rels: Vec<String> =
        got.iter().map(|p| p.strip_prefix(tmp.path()).unwrap().to_string_lossy().replace('\\', "/")).collect();
    assert_eq!(rels, vec!["src/lib.rs", "tests/it.rs"]);
}

#[test]
fn parse_workspace_metadata_rejects_invalid_json() {
    assert!(parse_workspace_metadata(b"not json").is_err());
    assert!(parse_workspace_metadata(b"{}").is_err());
}

#[test]
fn cli_multi_crate_check_format_and_recheck() {
    let tmp = TempDirGuard::new("cli-multi");
    tmp.write("rustfmt.toml", "max_width = 120\nuse_small_heuristics = \"Max\"\n");
    tmp.write("Cargo.toml", "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\n");
    tmp.write("crates/a/Cargo.toml", "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
    tmp.write("crates/a/src/lib.rs", "pub fn alpha(x: i32) -> i32 {\n    x + 1\n}\n");
    tmp.write("crates/b/Cargo.toml", "[package]\nname = \"b\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
    tmp.write("crates/b/src/lib.rs", "pub fn beta(x: i32) -> i32 {\n    x + 2\n}\n");

    let ws = tmp.path().to_str().unwrap();

    // 1. Clean workspace -> --check exits 0.
    let out = run_cli(&["--check", "--workspace", ws, "--max-batch-bytes", "100"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    // 2. Introduce formatting drift in crate b -> --check exits 1.
    tmp.write("crates/b/src/lib.rs", "pub fn beta(   x:i32)->i32{x+2}\n");
    let out = run_cli(&["--check", "--workspace", ws, "--max-batch-bytes", "100"]);
    assert_eq!(out.status.code(), Some(1));

    // 3. Format workspace -> exits 0.
    let out = run_cli(&["--workspace", ws, "--max-batch-bytes", "100"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    // 4. Re-check -> exits 0.
    let out = run_cli(&["--check", "--workspace", ws, "--max-batch-bytes", "100"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn cli_oversized_crate_direct_rustfmt_chunking_check_and_format() {
    let tmp = TempDirGuard::new("cli-chunk");
    tmp.write("rustfmt.toml", "max_width = 120\n");
    tmp.write("Cargo.toml", "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\n");
    tmp.write("crates/big/Cargo.toml", "[package]\nname = \"big\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
    tmp.write("crates/big/src/lib.rs", "pub mod m1;\npub mod m2;\n");
    tmp.write("crates/big/src/m1.rs", "pub fn m1() -> i32 {\n    1\n}\n");
    tmp.write("crates/big/src/m2.rs", "pub fn   m2(  )->i32{2}\n");

    let ws = tmp.path().to_str().unwrap();
    // Force max-batch-bytes smaller than the 3 files' combined path lengths so DirectRustfmt runs.
    let out = run_cli(&["--check", "--workspace", ws, "--max-batch-bytes", "90"]);
    assert_eq!(out.status.code(), Some(1));

    let out = run_cli(&["--workspace", ws, "--max-batch-bytes", "90"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let out = run_cli(&["--check", "--workspace", ws, "--max-batch-bytes", "90"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn find_rustfmt_config_for_dir_respects_nearest_crate_config_and_workspace_boundary() {
    let tmp = TempDirGuard::new("cfg-lookup");
    tmp.write("rustfmt.toml", "max_width = 90\n");
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    tmp.write("ws/crates/plain/src/lib.rs", "");
    tmp.write("ws/crates/override/.rustfmt.toml", "max_width = 100\n");
    tmp.write("ws/crates/override/src/lib.rs", "");
    tmp.write("ws/vendor/group/rustfmt.toml", "max_width = 80\n");
    tmp.write("ws/vendor/group/sub/src/lib.rs", "");

    // Must stop at `ws` and NOT pick up the parent directory's `rustfmt.toml` outside the workspace.
    assert_eq!(find_rustfmt_config_for_dir(&ws.join("crates/plain"), &ws), None);

    // Once workspace root has `rustfmt.toml`, plain crate resolves to it while overrides win locally.
    tmp.write("ws/rustfmt.toml", "max_width = 120\n");
    assert_eq!(find_rustfmt_config_for_dir(&ws.join("crates/plain"), &ws), Some(ws.join("rustfmt.toml")));
    assert_eq!(
        find_rustfmt_config_for_dir(&ws.join("crates/override"), &ws),
        Some(ws.join("crates/override/.rustfmt.toml"))
    );
    assert_eq!(
        find_rustfmt_config_for_dir(&ws.join("vendor/group/sub"), &ws),
        Some(ws.join("vendor/group/rustfmt.toml"))
    );
}

#[test]
fn cli_direct_rustfmt_respects_crate_local_rustfmt_config() {
    let tmp = TempDirGuard::new("cli-local-cfg");
    tmp.write("rustfmt.toml", "max_width = 120\nuse_small_heuristics = \"Max\"\n");
    tmp.write("Cargo.toml", "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\n");

    // Crate inheriting workspace `rustfmt.toml` (`max_width = 120`, `use_small_heuristics = "Max"`).
    tmp.write(
        "crates/root_cfg/Cargo.toml",
        "[package]\nname = \"root_cfg\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    tmp.write(
        "crates/root_cfg/src/lib.rs",
        "pub struct Point {\n    pub x_coord: i32,\n    pub y_coord: i32,\n}\n\npub fn make_point() -> Point {\n    Point { x_coord: 1, y_coord: 2 }\n}\n",
    );
    tmp.write("crates/root_cfg/src/extra.rs", "pub fn extra_root_helper() -> i32 {\n    42\n}\n");

    // Crate with its own `rustfmt.toml` (`max_width = 100`, `use_small_heuristics = "Default"`).
    // Under `"Default"`, `Point { x_coord: 1, y_coord: 2 }` wraps across lines (exceeding default struct_lit_width=18),
    // whereas workspace `"Max"` would collapse it onto one line.
    tmp.write(
        "crates/local_cfg/Cargo.toml",
        "[package]\nname = \"local_cfg\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    tmp.write("crates/local_cfg/rustfmt.toml", "max_width = 100\nuse_small_heuristics = \"Default\"\n");
    tmp.write(
        "crates/local_cfg/src/lib.rs",
        "pub struct Point {\n    pub x_coord: i32,\n    pub y_coord: i32,\n}\n\npub fn make_point() -> Point {\n    Point {\n        x_coord: 1,\n        y_coord: 2,\n    }\n}\n",
    );
    tmp.write("crates/local_cfg/src/extra.rs", "pub fn extra_local_helper() -> i32 {\n    7\n}\n");

    let ws = tmp.path().to_str().unwrap();
    // Force `DirectRustfmt` by setting budget smaller than two file paths.
    let out = run_cli(&["--check", "--workspace", ws, "--max-batch-bytes", "90"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    // Reformat `local_cfg` to match workspace `"Max"` style instead of its local `"Default"` rule -> must fail `--check`.
    tmp.write(
        "crates/local_cfg/src/lib.rs",
        "pub struct Point {\n    pub x_coord: i32,\n    pub y_coord: i32,\n}\n\npub fn make_point() -> Point {\n    Point { x_coord: 1, y_coord: 2 }\n}\n",
    );
    let out = run_cli(&["--check", "--workspace", ws, "--max-batch-bytes", "90"]);
    assert_eq!(out.status.code(), Some(1));

    // Write-mode `fmt` under `DirectRustfmt` restores `local_cfg` using its local `rustfmt.toml`.
    let out = run_cli(&["--workspace", ws, "--max-batch-bytes", "90"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let local_src = std::fs::read_to_string(tmp.path().join("crates/local_cfg/src/lib.rs")).unwrap();
    assert!(local_src.contains("x_coord: 1,\n"), "expected crate-local multiline formatting, got:\n{local_src}");

    let out = run_cli(&["--check", "--workspace", ws, "--max-batch-bytes", "90"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn cli_rejects_bad_usage() {
    let out = run_cli(&["--max-batch-bytes", "0"]);
    assert_eq!(out.status.code(), Some(2));

    let out = run_cli(&["--unknown-flag"]);
    assert_eq!(out.status.code(), Some(2));
}

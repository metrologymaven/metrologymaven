//! Runs the shader lint on the real renderer crate, `crates/fid-render` (VIZ.RENDER.FRAME.SHLINT
//! REQ-001 .. REQ-003 applied to the workspace, added with VIZ.RENDER.SCENE.IDTAB): its shader
//! manifest parses, it has no unmanifested shader or readback, and its resolved dependency graph
//! (from `cargo metadata`, offline and locked) reaches no UI-toolkit crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

#[test]
fn fid_render_passes_the_shader_lint() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let crate_dir = root.join("crates/fid-render");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .current_dir(&root)
        .args(["metadata", "--format-version", "1", "--offline", "--locked"])
        .output()
        .unwrap();
    assert!(out.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr));
    let findings = fid_shlint::check(&crate_dir, &out.stdout, &root, fid_shlint::DEFAULT_PACKAGE).unwrap();
    let text: Vec<String> = findings.iter().map(|f| f.to_string()).collect();
    assert!(text.is_empty(), "fid-render shader lint findings:\n{}", text.join("\n"));
}

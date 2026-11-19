//! AUTO.SURF.GEN AC-1, AC-2 and AC-5 through the `fid-surface-gen` binary that `cargo xtask gen-surfaces` runs. The
//! expected tree is written by `fixtures/auto/surf/gen/make_fixtures.py` (independent
//! Python reference); `gen.rs` covers the rest.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-surface-gen")).current_dir(repo_root()).args(args).output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                let rel = p.strip_prefix(root).unwrap();
                let key: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                out.insert(key.join("/"), fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn expected() -> BTreeMap<String, Vec<u8>> {
    let raw = fs::read(repo_root().join("fixtures/auto/surf/gen/expected-min-classes.json")).unwrap();
    let fid_canon::CValue::Obj(o) = fid_canon::parse(&raw).unwrap() else { panic!("expected an object") };
    o.into_iter()
        .map(|(k, v)| match v {
            fid_canon::CValue::Str(s) => (k, s.into_bytes()),
            other => panic!("{k}: {other:?}"),
        })
        .collect()
}

#[test]
fn gen_surfaces_twice_gives_the_reference_tree() {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("gen-surfaces");
    let _ = fs::remove_dir_all(&base);
    let mut trees = Vec::new();
    for name in ["a", "b"] {
        let out = base.join(name);
        let out_s = out.to_string_lossy().into_owned();
        let (code, stdout) =
            run(&["--manifest", "fixtures/manifest/min-classes.json", "--out", &out_s, "--test-emitter", "echo"]);
        assert_eq!(code, 0, "{stdout}");
        assert!(stdout.contains("echo: 14 files, 13 commands"), "{stdout}");
        trees.push(tree(&out));
    }
    assert_eq!(trees[0], trees[1]);
    assert_eq!(trees[0], expected());
    for (path, bytes) in &trees[0] {
        assert!(!bytes.contains(&b'\r'), "{path} has a CR byte");
    }
}

#[test]
fn gen_surfaces_usage_errors() {
    assert_eq!(run(&[]).0, 2);
    assert_eq!(run(&["--manifest", "x", "--out", "y", "--test-emitter", "nope"]).0, 2);
    let missing = Path::new(env!("CARGO_TARGET_TMPDIR")).join("gen-surfaces-missing");
    let (code, _) = run(&["--manifest", "no/such/manifest.json", "--out", &missing.to_string_lossy()]);
    assert_eq!(code, 1);
    assert!(!missing.exists());
}

//! AC-9 of spec node AUTO.SURF.COMPOSE: the bin target `fid` belongs to `fid-compose` and to no
//! other package (no `[surfaces]` crate), `layers.toml` maps `fid-compose` to `app`, and the
//! moved binary keeps `fid canon` byte for byte. `cargo xtask layers` and
//! `cargo xtask lint-crate-graph` run as their own CI steps; the moved `CARGO_BIN_EXE_fid`
//! tests are `bin.rs`, `ctrl.rs` and `tree.rs` here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn field<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    let CValue::Obj(o) = v else { panic!("not an object") };
    o.get(k).unwrap_or_else(|| panic!("no {k}"))
}

fn text(v: &CValue) -> &str {
    let CValue::Str(s) = v else { panic!("not a string") };
    s
}

#[test]
fn ac9_only_fid_compose_builds_the_fid_binary() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--offline"])
        .current_dir(root())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let meta = parse(&out.stdout).unwrap();
    let CValue::Arr(packages) = field(&meta, "packages") else { panic!("packages") };
    let mut owners = Vec::new();
    for p in packages {
        let CValue::Arr(targets) = field(p, "targets") else { panic!("targets") };
        for t in targets {
            let CValue::Arr(kinds) = field(t, "kind") else { panic!("kind") };
            if text(field(t, "name")) == "fid" && kinds.iter().any(|k| text(k) == "bin") {
                owners.push(text(field(p, "name")).to_string());
            }
        }
    }
    assert_eq!(owners, ["fid-compose"]);

    let allow = std::fs::read_to_string(root().join("lint-allowlist.toml")).unwrap();
    let surfaces = allow.lines().find(|l| l.starts_with("crates = [\"fid-cli\"")).expect("[surfaces] crates line");
    assert!(!surfaces.contains("fid-compose"), "{surfaces}");

    let layers = std::fs::read_to_string(root().join("layers.toml")).unwrap();
    assert!(layers.lines().any(|l| l.trim() == "\"fid-compose\" = \"app\""));
}

/// `fid canon` through the moved binary, byte for byte as the CI smoke test expects.
#[test]
fn ac9_fid_canon_is_unchanged() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fid"))
        .arg("canon")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(br#"{"b":1,"a":[1e21,0.1]}"#).unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"{\"a\":[1e+21,0.1],\"b\":1}\n");
    let out = Command::new(env!("CARGO_BIN_EXE_fid")).arg("--version").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8(out.stdout).unwrap().contains(env!("CARGO_PKG_VERSION")));
}

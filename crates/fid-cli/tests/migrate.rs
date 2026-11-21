//! MIG.SRC.REG AC-5 and AC-6 through the `fid` binary: `fid migrate list-sources` against the
//! synthetic register `fixtures/mig/src/reg/register-ok.json`, compared with the outputs of
//! the independent reference `fixtures/mig/src/reg/make_fixtures.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_surface::{Envelope, Outcome};
use std::path::{Path, PathBuf};
use std::process::Command;

const REGISTER: &str = "fixtures/mig/src/reg/register-ok.json";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fid(args: &[&str]) -> (i32, Vec<u8>) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid")).current_dir(root()).args(args).output().unwrap();
    (out.status.code().unwrap(), out.stdout)
}

/// AC-5: `--status NOT_ADMITTED --json` returns exactly the not-admitted entries with their
/// export routes.
#[test]
fn ac5_not_admitted_json() {
    let (code, out) = fid(&["migrate", "list-sources", "--status", "NOT_ADMITTED", "--json", "--register", REGISTER]);
    assert_eq!(code, 0);
    let text = String::from_utf8(out).unwrap();
    let env = Envelope::decode(text.trim_end().as_bytes()).unwrap();
    env.validate().unwrap();
    assert_eq!(env.outcome, Outcome::Computed);
    assert_eq!(env.command_id, "migrate.list_sources@1");
    let result = env.result.unwrap();
    let want = std::fs::read(root().join("fixtures/mig/src/reg/expected/list-not-admitted.json")).unwrap();
    assert_eq!(fid_canon::to_canonical_bytes(&result).unwrap(), want.trim_ascii_end());
    let CValue::Obj(o) = &result else { panic!() };
    let Some(CValue::Arr(entries)) = o.get("entries") else { panic!() };
    assert_eq!(entries.len(), 2);
    for e in entries {
        let CValue::Obj(e) = e else { panic!() };
        assert_eq!(e.get("status"), Some(&CValue::Str("NOT_ADMITTED".into())));
        assert!(e.contains_key("export_route"));
    }
}

/// AC-6: `--format md` twice gives `expected/sources.md` byte for byte.
#[test]
fn ac6_page_twice() {
    let want = std::fs::read(root().join("fixtures/mig/src/reg/expected/sources.md")).unwrap();
    let a = fid(&["migrate", "list-sources", "--format", "md", "--register", REGISTER]);
    let b = fid(&["migrate", "list-sources", "--format", "md", "--register", REGISTER]);
    assert_eq!(a, (0, want.clone()));
    assert_eq!(b, (0, want));
}

/// The embedded register lists, and bad arguments are usage or validation errors.
#[test]
fn embedded_and_errors() {
    let (code, out) = fid(&["migrate", "list-sources"]);
    assert_eq!(code, 0);
    assert!(String::from_utf8(out).unwrap().contains("\"policy\":\"D-033-A\""));
    assert_eq!(fid(&["migrate", "list-sources", "--bogus"]).0, 2);
    assert_eq!(fid(&["migrate", "list-sources", "--register", REGISTER, "--register", REGISTER]).0, 2);
    assert_eq!(fid(&["migrate", "list-sources", "--status", "ADMITTED", "--status", "ADMITTED"]).0, 2);
    assert_ne!(fid(&["migrate", "list-sources", "--status", "admitted"]).0, 0);
    assert_ne!(fid(&["migrate", "list-sources", "--register", "no-such-file.json"]).0, 0);
}

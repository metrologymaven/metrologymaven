//! `verify.check_signature@1` (TRUST.SIGN.TROOT): its parameter and result schemas compile
//! for the bus (`fid_bus::validate::Compiled`) and accept real values: every expected report
//! of `fixtures/troot/expected/` (written by the independent `fixtures/troot/make_fixtures.py`)
//! and the parameter shapes the CLI and MCP send; values outside the schema are refused.
//! The test lives here because fid-sign-core is tested on wasm32, where fid-bus (SQLite)
//! does not build.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::CValue;
use std::path::PathBuf;

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn parse(b: &[u8]) -> CValue {
    fid_canon::parse(b).unwrap()
}

#[test]
fn schemas_compile_and_accept_real_values() {
    let decls = fid_sign_core::troot::declarations();
    assert_eq!(decls.len(), 1);
    let d = &decls[0].def;
    let params = Compiled::compile(&d.params_schema).unwrap();
    let result = Compiled::compile(&d.result_schema).unwrap();
    let ok = |c: &Compiled, v: &CValue| c.evaluate(v, &NoHooks).is_ok();

    for p in [
        br#"{"package":"pkg-1","trust_root":"root-1"}"#.as_slice(),
        br#"{"package":"pkg-1","trust_root":"root-1","revoked":"rev-1"}"#,
        br#"{"package":"pkg-1","trust_root":"root-1","revoked":"rev-1","keys":"keys-1"}"#,
    ] {
        assert!(ok(&params, &parse(p)), "{}", String::from_utf8_lossy(p));
    }
    for p in [
        br#"{"package":"pkg-1"}"#.as_slice(),
        br#"{"package":"","trust_root":"root-1"}"#,
        br#"{"package":"pkg-1","trust_root":"root-1","extra":"x"}"#,
        br#"{"package":"pkg-1","trust_root":"root-1","keys":3}"#,
    ] {
        assert!(!ok(&params, &parse(p)), "{}", String::from_utf8_lossy(p));
    }

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/troot/expected");
    let mut n = 0;
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    names.sort();
    for path in names {
        let v = parse(&std::fs::read(&path).unwrap());
        if let Err(f) = result.evaluate(&v, &NoHooks) {
            panic!("{}: {} at {} ({})", path.display(), f.keyword, f.pointer, f.message);
        }
        n += 1;
    }
    assert!(n >= 15, "{n}");
    for bad in [
        br#"{"package_status":"SIGNED"}"#.as_slice(),
        br#"{"package_status":"MAYBE","signatures":[]}"#,
        br#"{"package_status":"SIGNED","signatures":[{"index":0,"envelope":"e","claimed_keyid":"","status":"TRUSTED"}]}"#,
        br#"{"package_status":"SIGNED","signatures":[{"index":0,"envelope":"e","claimed_keyid":"","status":"INVALID","signed_at":"x"}]}"#,
        br#"{"package_status":"SIGNED","signatures":[],"statement_sha256":"ABC"}"#,
    ] {
        assert!(!ok(&result, &parse(bad)), "{}", String::from_utf8_lossy(bad));
    }
}

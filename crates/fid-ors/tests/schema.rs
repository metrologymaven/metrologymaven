//! UNC.ORS.TRACE command definitions: real parameters and results validated with the bus's
//! own compiled schemas (`fid_bus::validate::Compiled`). Hand-labelled; fixtures from
//! `fixtures/ors/trace/make_fixtures.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse_with_limits, CValue};
use fid_gps::dmt::DmtTable;
use fid_ors::{check_suite, command_defs, invoke_mcp, Suite, SuiteStore, CHECK_SUITE, LIST_SUITE};
use std::path::PathBuf;

fn read(rel: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ors/trace").join(rel);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn dmt() -> DmtTable {
    DmtTable::load(&read("dmt_stub.json")).unwrap()
}

fn bundle(rel: &str) -> Suite {
    Suite::from_bundle(&read(rel)).unwrap()
}

fn store() -> SuiteStore {
    let mut s = SuiteStore::new();
    s.add_dmt(dmt()).unwrap();
    // The good suite is the bad licence bundle with its licence restored.
    let good =
        String::from_utf8(read("bad/licence_proprietary.json")).unwrap().replace("\"proprietary\"", "\"CC-BY-4.0\"");
    s.install(Suite::from_bundle(good.as_bytes()).unwrap()).unwrap();
    s.stage(bundle("versioning/bumped_without_changelog.json")).unwrap();
    s
}

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

#[test]
fn registry_definitions_and_schemas_validate_real_values() {
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 2);
    let ids: Vec<String> = defs.iter().map(|d| format!("{}@{}", d.id, d.major)).collect();
    assert_eq!(ids, [CHECK_SUITE, LIST_SUITE]);
    let check_p = Compiled::compile(&defs[0].params_schema).unwrap();
    let check_r = Compiled::compile(&defs[0].result_schema).unwrap();
    let list_r = Compiled::compile(&defs[1].result_schema).unwrap();
    let ok = |c: &Compiled, v: &CValue, what: &str| {
        if let Err(f) = c.evaluate(v, &NoHooks) {
            panic!("{what}: {} at {} ({})", f.keyword, f.pointer, f.message);
        }
    };
    let s = store();
    let mut log = Vec::new();
    for p in [
        r#"{"suite":"ors-trace-good@1.0.0"}"#,
        r#"{"suite":"ors-trace-good@1.0.0","staged":false}"#,
        r#"{"suite":"ors-trace-good@1.1.0","previous":"ors-trace-good@1.0.0","staged":true}"#,
    ] {
        let v = parse_with_limits(p.as_bytes(), 4096, 4).unwrap();
        ok(&check_p, &v, p);
        let env = invoke_mcp(&s, &mut log, "rdl_check_suite", p.as_bytes());
        let result = env.result.as_ref().unwrap_or_else(|| panic!("{p}: {:?}", env.error));
        ok(&check_r, result, p);
    }
    // A result whose counts were strings, or with no digest, is refused by the schema.
    let env = invoke_mcp(&s, &mut log, "rdl_check_suite", br#"{"suite":"ors-trace-good@1.0.0"}"#);
    let CValue::Obj(mut bad) = env.result.unwrap() else { panic!("not an object") };
    bad.insert("cases".to_string(), CValue::Str("6".to_string()));
    assert!(check_r.evaluate(&CValue::Obj(bad.clone()), &NoHooks).is_err(), "string count");
    bad.insert("cases".to_string(), CValue::Num(6.0));
    bad.remove("suite_digest");
    assert!(check_r.evaluate(&CValue::Obj(bad), &NoHooks).is_err(), "no suite_digest");
    // A failing result validates too (findings with null and non-null case).
    let table = dmt();
    for name in ["licence_proprietary", "long_note"] {
        let r = check_suite(&bundle(&format!("bad/{name}.json")), Some(&table), None).unwrap();
        ok(&check_r, &r.to_value(), name);
    }
    let env = invoke_mcp(&s, &mut log, "rdl_list_suite", b"{}");
    ok(&list_r, env.result.as_ref().unwrap(), "list");
}

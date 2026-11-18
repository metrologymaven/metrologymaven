//! Acceptance criteria of spec node CORE.REG.VER.LIFE (AC-1 .. AC-3).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_registry::{
    check_migrators, check_transition, retained, CommandDef, Date, Lifecycle, LifecycleDecl, MigratorRecord, State,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> BTreeMap<String, CValue> {
    let v = parse(&std::fs::read(root().join("fixtures/core/reg/ver/life").join(name)).unwrap()).unwrap();
    let CValue::Obj(o) = v else { panic!("{name}: object expected") };
    o
}

fn arr(v: &CValue) -> &[CValue] {
    let CValue::Arr(a) = v else { panic!("array expected") };
    a
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    let CValue::Obj(o) = v else { panic!("object expected") };
    o
}

fn text<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s.as_str(),
        other => panic!("{k}: string expected, got {other:?}"),
    }
}

fn registry() -> Vec<LifecycleDecl> {
    let f = fixture("registry.json");
    arr(&f["definitions"])
        .iter()
        .map(|v| LifecycleDecl::from_def(&CommandDef::from_value(v.clone()).unwrap()).unwrap())
        .collect()
}

/// AC-1: all 16 state pairs plus the 12-month boundary, each with its expected outcome.
#[test]
fn ac1_transitions() {
    let f = fixture("transitions.json");
    let cases = arr(&f["cases"]);
    assert_eq!(cases.len(), 30);
    let mut legal = Vec::new();
    for (i, c) in cases.iter().enumerate() {
        let c = obj(c);
        let name = text(c, "name");
        let old = Lifecycle::from_map(obj(&c["from"])).unwrap();
        let new = State::parse(text(c, "to")).unwrap();
        let release = Date::parse(text(c, "release")).unwrap();
        let got = check_transition(&old, new, release).map_err(|e| e.code());
        let want = text(c, "expect");
        assert_eq!(got.err().unwrap_or("ok"), want, "case {name}");
        if i < 16 && want == "ok" {
            legal.push((old.state, new));
        }
    }
    use State::*;
    assert_eq!(legal, [(Active, Deprecated), (Deprecated, Active), (Deprecated, Hidden), (Hidden, Removed)]);
}

/// AC-1 boundary, stated directly: 11 months 30 days is too short, 12 months is enough.
#[test]
fn ac1_twelve_month_boundary() {
    let old = Lifecycle {
        state: State::Deprecated,
        since: Date::new(2025, 1, 15).unwrap(),
        replaced_by: None,
        remove_after: None,
    };
    let at = |y, m, d| check_transition(&old, State::Hidden, Date::new(y, m, d).unwrap()).map_err(|e| e.code());
    assert_eq!(at(2026, 1, 14), Err("deprecation_too_short"));
    assert_eq!(at(2026, 1, 15), Ok(()));
}

/// AC-2: the manifest input lists hidden and active majors and never a removed one.
#[test]
fn ac2_retained() {
    let decls = registry();
    let got: Vec<String> = retained(&decls).unwrap().iter().map(|d| d.command.to_string()).collect();
    let f = fixture("registry.json");
    let want: Vec<String> = arr(&f["expect_retained"])
        .iter()
        .map(|v| match v {
            CValue::Str(s) => s.clone(),
            _ => panic!(),
        })
        .collect();
    assert_eq!(got, want);
    assert!(!got.iter().any(|s| s == "demo.old@1"));
}

/// A pair declared twice has two states: refused.
#[test]
fn retained_refuses_duplicates() {
    let mut decls = registry();
    decls.push(decls[0].clone());
    assert_eq!(retained(&decls).map(|_| ()).map_err(|e| e.code()), Err("bad_lifecycle"));
}

/// Duplicate (ID, major) declarations are refused by the migrator check too.
#[test]
fn check_migrators_refuses_duplicate_decls() {
    let mut decls = registry();
    let mut dup = decls[0].clone();
    dup.lifecycle = Lifecycle { state: State::Removed, ..dup.lifecycle };
    decls.push(dup);
    let m = MigratorRecord::from_value(
        &parse(br#"{"algorithm":"demo.cmd.migrate@1.0.0","command":"demo.cmd","from_major":1,"to_major":2}"#).unwrap(),
    )
    .unwrap();
    assert_eq!(check_migrators(&decls, &[m]).map_err(|e| e.code()), Err("bad_lifecycle"));
}

/// `replaced_by` under the same name must point to a higher major; a rename is fine.
#[test]
fn replaced_by_points_up() {
    let decl = |replaced_by: &str| {
        let mut v = arr(&fixture("registry.json")["definitions"])[0].clone();
        let CValue::Obj(o) = &mut v else { panic!() };
        let CValue::Obj(life) = o.get_mut("lifecycle").unwrap() else { panic!() };
        life.insert("replaced_by".into(), CValue::Str(replaced_by.into()));
        LifecycleDecl::from_def(&CommandDef::from_value(v).unwrap()).map(|_| ()).map_err(|e| e.code())
    };
    assert_eq!(decl("demo.cmd@2"), Ok(()));
    assert_eq!(decl("demo.renamed@1"), Ok(()));
    assert_eq!(decl("demo.cmd@1"), Err("bad_lifecycle"));
    assert_eq!(decl("demo.cmd@0"), Err("bad_lifecycle"));
}

/// AC-3: migrator sets, each failing or passing as labelled.
#[test]
fn ac3_migrators() {
    let decls = registry();
    let f = fixture("migrators.json");
    for c in arr(&f["cases"]) {
        let c = obj(c);
        let records: Vec<MigratorRecord> =
            arr(&c["migrators"]).iter().map(|v| MigratorRecord::from_value(v).unwrap()).collect();
        for r in &records {
            assert_eq!(&MigratorRecord::from_value(&r.to_value()).unwrap(), r, "round trip");
        }
        let got = check_migrators(&decls, &records).map_err(|e| e.code());
        assert_eq!(got.err().unwrap_or("ok"), text(c, "expect"), "case {}", text(c, "name"));
    }
}

#[test]
fn malformed_migrator_is_invalid() {
    let v =
        parse(br#"{"algorithm":"demo.cmd.migrate@1.0.0","command":"demo.cmd","from_major":1.5,"to_major":2}"#).unwrap();
    assert_eq!(MigratorRecord::from_value(&v).map_err(|e| e.code()), Err("migrator_invalid"));
}

#[test]
fn lifecycle_metadata_rules() {
    let life = |json: &str| {
        let CValue::Obj(o) = parse(json.as_bytes()).unwrap() else { panic!() };
        Lifecycle::from_map(&o).map_err(|e| e.code())
    };
    let ok =
        life(r#"{"state":"deprecated","since":"2026-01-01","replaced_by":"demo.cmd@2","remove_after":"2027-01-01"}"#);
    let ok = ok.unwrap();
    assert_eq!(Lifecycle::from_map(obj(&ok.to_value())).unwrap(), ok);
    for bad in [
        r#"{"state":"experimental","since":"2026-01-01"}"#,
        r#"{"state":"active"}"#,
        r#"{"state":"active","since":"2026-01-01","replaced_by":"demo.cmd@2"}"#,
        r#"{"state":"hidden","since":"2026-01-01","remove_after":"2025-12-31"}"#,
        r#"{"state":"hidden","since":"2025-02-29"}"#,
        r#"{"state":"hidden","since":"2025-1-01"}"#,
        r#"{"state":"hidden","since":"2026-01-01","note":"x"}"#,
    ] {
        assert_eq!(life(bad), Err("bad_lifecycle"), "{bad}");
    }
    assert_eq!(Date::parse("2024-02-29").map(|d| d.to_string()), Ok("2024-02-29".to_string()));
}

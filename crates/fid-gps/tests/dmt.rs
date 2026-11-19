//! Acceptance criteria of spec node GDT.RES.SCHEMA (AC-1 .. AC-6) plus the error paths and
//! limits pinned in the leaf's Design. Expected digests come from the independent stdlib-Python
//! reference `fixtures/gdt/dmt/make_fixtures.py` (`synthetic_table_v1.digest`,
//! `synthetic_table_v1.rows.json`); error cases are hand-labelled from the leaf's rules. All
//! rows are synthetic placeholders with no standards content.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gps::dmt::row::row_from_value;
use fid_gps::dmt::{
    profile_vocabulary, AlgId, Dialect, DmtError, DmtKey, DmtTable, Operator, OperatorChain, OperatorKind, Param,
    ProfileKeyRule, ReviewState, RowStatus, SemVer, DMT_TABLE_JSON_SCHEMA, NAMED_PROFILE_KEYS,
};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/dmt")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap()
}

fn table() -> DmtTable {
    DmtTable::load(&read("synthetic_table_v1.json")).unwrap()
}

fn doc() -> CValue {
    fid_canon::parse(&read("synthetic_table_v1.json")).unwrap()
}

fn obj(v: &mut CValue) -> &mut BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("object"),
    }
}

fn arr(v: &mut CValue) -> &mut Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("array"),
    }
}

fn rows(d: &mut CValue) -> &mut Vec<CValue> {
    arr(obj(d).get_mut("rows").unwrap())
}

/// Row `i` (in key order) of the fixture document.
fn row(d: &mut CValue, i: usize) -> &mut BTreeMap<String, CValue> {
    obj(&mut rows(d)[i])
}

/// First operator of the first variant of row 0.
fn op0(d: &mut CValue) -> &mut BTreeMap<String, CValue> {
    let variants = arr(row(d, 0).get_mut("variants").unwrap());
    let steps = arr(obj(&mut variants[0]).get_mut("steps").unwrap());
    obj(&mut steps[0])
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn strs(v: &[&str]) -> CValue {
    CValue::Arr(v.iter().map(|x| s(x)).collect())
}

fn load(d: &CValue) -> Result<DmtTable, DmtError> {
    DmtTable::load(&fid_canon::to_canonical_bytes(d).unwrap())
}

/// Loads the fixture after `f` edits it and returns the error.
fn fail(f: impl FnOnce(&mut CValue)) -> DmtError {
    let mut d = doc();
    f(&mut d);
    load(&d).unwrap_err()
}

fn malformed(e: &DmtError) -> &'static str {
    assert_eq!(e.code(), "malformed_table", "{e}");
    e.reason().unwrap()
}

// ------------------------------------------------------------------ AC-1 .. AC-2

#[test]
fn ac1_synthetic_table_loads_with_the_recorded_digest() {
    let t = table();
    assert_eq!(t.rows().len(), 20);
    assert!(t.rows().values().all(|r| r.synthetic()));
    let expected = String::from_utf8(read("synthetic_table_v1.digest")).unwrap();
    assert_eq!(t.digest().as_str(), expected.trim_end());
    assert_eq!(fid_canon::digest_bytes(t.canonical_bytes()).to_string(), expected.trim_end());
    assert_eq!(t.version().to_string(), "1.0.0");
}

#[test]
fn ac1_row_and_chain_digests_match_the_reference() {
    let t = table();
    let CValue::Obj(per_row) = fid_canon::parse(&read("synthetic_table_v1.rows.json")).unwrap() else { panic!() };
    assert_eq!(per_row.len(), t.rows().len());
    for (key, exp) in &per_row {
        let r = t.get(key).unwrap();
        let CValue::Obj(exp) = exp else { panic!() };
        assert_eq!(exp.get("row"), Some(&s(r.digest().as_str())), "{key}");
        let Some(CValue::Obj(chains)) = exp.get("chains") else { panic!() };
        assert_eq!(chains.len(), r.variants().len());
        for c in r.variants() {
            assert_eq!(chains.get(c.variant()), Some(&s(c.digest().as_str())), "{key} {}", c.variant());
        }
    }
}

#[test]
fn ac2_shuffled_file_order_gives_the_same_digest() {
    let a = table();
    let b = DmtTable::load(&read("synthetic_table_v1_shuffled.json")).unwrap();
    assert_ne!(read("synthetic_table_v1.json"), read("synthetic_table_v1_shuffled.json"));
    assert_eq!(a.digest(), b.digest());
    assert_eq!(a.canonical_bytes(), b.canonical_bytes());
    assert_eq!(a, b);
}

#[test]
fn canonical_form_is_stable_and_reloads_to_itself() {
    let t = table();
    let again = DmtTable::load(t.canonical_bytes()).unwrap();
    assert_eq!(again.canonical_bytes(), t.canonical_bytes());
    assert_eq!(again.digest(), t.digest());
    // The canonical form is the RFC 8785 form of the file (rows already in key order).
    assert_eq!(t.canonical_bytes(), fid_canon::canonicalize(&read("synthetic_table_v1.json")).unwrap());
}

// ------------------------------------------------------------------ AC-3: key grammar

#[test]
fn ac3_modifiers_out_of_order_are_bad_dmt_key() {
    let e = DmtKey::parse("dmt:asme/position/mmc+cz@1").unwrap_err();
    assert_eq!(e.code(), "bad_dmt_key");
    assert_eq!(e, DmtError::BadDmtKey { key: "dmt:asme/position/mmc+cz@1".to_owned() });
    // Also inside a table, with the row's own modifiers in the same (wrong) order.
    let e = fail(|d| {
        let r = row(d, 0);
        r.insert("key".to_owned(), s("dmt:asme/angularity/mmc+cz@1"));
        r.insert("modifiers".to_owned(), strs(&["mmc", "cz"]));
    });
    assert_eq!(e.code(), "bad_dmt_key");
}

#[test]
fn key_grammar_is_exact() {
    let k = DmtKey::parse("dmt:asme/position/cz+mmc@1").unwrap();
    assert_eq!((k.dialect(), k.characteristic(), k.version()), (Dialect::Asme, "position", 1));
    assert_eq!(k.modifiers(), ["cz".to_owned(), "mmc".to_owned()]);
    assert!(DmtKey::parse("dmt:iso/flatness/none@4294967295").unwrap().modifiers().is_empty());
    let bad = [
        "",
        "dmt:asme/position/mmc+mmc@1",
        "dmt:asme/position/@1",
        "dmt:asme/position/none",
        "dmt:asme/position/none@0",
        "dmt:asme/position/none@01",
        "dmt:asme/position/none@4294967296",
        "dmt:asme/position/none@1@1",
        "dmt:asme/position/none+mmc@1",
        "dmt:asme/position/mmc+none@1",
        "dmt:ansi/position/none@1",
        "dmt:ASME/position/none@1",
        "dmt:asme/Position/none@1",
        "dmt:asme/position/MMC@1",
        "DMT:asme/position/none@1",
        "dmt:asme/position/none/x@1",
        "dmt:asme/position@1",
        "dmt:asme/1position/none@1",
        "dmt:asme/position/mmc+@1",
        " dmt:asme/position/none@1",
        "dmt:asme/position/a+b+c+d+e+f+g+h+i@1",
    ];
    for b in bad {
        assert_eq!(DmtKey::parse(b).unwrap_err().code(), "bad_dmt_key", "{b:?}");
    }
    // Eight modifiers is the limit; 128 bytes is the limit.
    assert!(DmtKey::parse("dmt:asme/position/a+b+c+d+e+f+g+h@1").is_ok());
    let long_char = "c".repeat(32);
    let at_limit = format!("dmt:asme/{long_char}/{}@1", ["m"; 1].join("+"));
    assert!(DmtKey::parse(&at_limit).is_ok());
    let too_long = format!(
        "dmt:asme/position/{}@1",
        (0..8).map(|i| format!("m{i}{}", "x".repeat(13))).collect::<Vec<_>>().join("+")
    );
    assert!(too_long.len() > 128);
    assert_eq!(DmtKey::parse(&too_long).unwrap_err().code(), "bad_dmt_key");
    // Built from parts in any order, the key is canonical.
    let k = DmtKey::from_parts(Dialect::Asme, "position", &["mmc", "cz"], 1).unwrap();
    assert_eq!(k.as_str(), "dmt:asme/position/cz+mmc@1");
    assert_eq!(DmtKey::from_parts(Dialect::Iso, "flatness", &[], 2).unwrap().as_str(), "dmt:iso/flatness/none@2");
    assert_eq!(DmtKey::from_parts(Dialect::Asme, "position", &["mmc", "mmc"], 1).unwrap_err().code(), "bad_dmt_key");
}

#[test]
fn keys_order_by_bytes() {
    let mut keys: Vec<DmtKey> = ["dmt:iso/a/none@1", "dmt:asme/b/none@1", "dmt:asme/a/x@1", "dmt:asme/a/none@1"]
        .iter()
        .map(|k| DmtKey::parse(k).unwrap())
        .collect();
    keys.sort();
    let texts: Vec<&str> = keys.iter().map(DmtKey::as_str).collect();
    assert_eq!(texts, ["dmt:asme/a/none@1", "dmt:asme/a/x@1", "dmt:asme/b/none@1", "dmt:iso/a/none@1"]);
}

// ------------------------------------------------------------------ AC-4: duplicate keys

#[test]
fn ac4_two_rows_with_the_same_key_fail_duplicate_dmt_key() {
    let e = fail(|d| {
        let first = rows(d)[0].clone();
        rows(d).push(first);
    });
    assert_eq!(e, DmtError::DuplicateDmtKey { key: "dmt:asme/angularity/none@1".to_owned() });
    // Same key, different content: still a duplicate.
    let e = fail(|d| {
        let mut first = rows(d)[0].clone();
        obj(&mut first).insert("notes".to_owned(), s("different"));
        rows(d).push(first);
    });
    assert_eq!(e.code(), "duplicate_dmt_key");
}

#[test]
fn case_only_duplicates_are_refused() {
    // A key differing only in case is not a valid key at all.
    let e = fail(|d| {
        let mut first = rows(d)[0].clone();
        obj(&mut first).insert("key".to_owned(), s("dmt:asme/Angularity/none@1"));
        rows(d).push(first);
    });
    assert_eq!(e.code(), "bad_dmt_key");
    // A member name differing only in case is an unknown field.
    assert_eq!(malformed(&fail(|d| drop(row(d, 0).insert("Key".to_owned(), s("x"))))), "unknown_field");
    // Reviewer IDs differing only in case: the upper-case one is refused.
    let e = fail(|d| {
        let review = CValue::Obj(
            [("state".to_owned(), s("signed_off")), ("reviewers".to_owned(), strs(&["REV-A", "rev-a"]))]
                .into_iter()
                .collect(),
        );
        row(d, 0).insert("review".to_owned(), review);
    });
    assert_eq!(malformed(&e), "bad_reviewer");
    // Variant names likewise.
    let e = fail(|d| {
        let variants = arr(row(d, 0).get_mut("variants").unwrap());
        let mut v = variants[0].clone();
        obj(&mut v).insert("variant".to_owned(), s("Surface"));
        variants.insert(0, v);
    });
    assert_eq!(malformed(&e), "bad_variant");
}

#[test]
fn duplicate_json_member_names_are_refused() {
    let text = String::from_utf8(read("synthetic_table_v1.json")).unwrap();
    let dup = text.replacen(
        "\"schema\": \"fid.dmt-table/1\",",
        "\"schema\": \"fid.dmt-table/1\", \"schema\": \"fid.dmt-table/1\",",
        1,
    );
    assert_ne!(dup, text);
    assert_eq!(malformed(&DmtTable::load(dup.as_bytes()).unwrap_err()), "duplicate_key");
}

// ------------------------------------------------------------------ AC-5: operator kinds

#[test]
fn ac5_unknown_operator_kind() {
    let e = fail(|d| drop(op0(d).insert("kind".to_owned(), s("smoothing"))));
    assert_eq!(e, DmtError::UnknownOperatorKind { kind: "smoothing".to_owned() });
    assert_eq!(fail(|d| drop(op0(d).insert("kind".to_owned(), s("Partition")))).code(), "unknown_operator_kind");
    assert_eq!(malformed(&fail(|d| drop(op0(d).insert("kind".to_owned(), CValue::Bool(true))))), "wrong_type");
}

#[test]
fn operator_vocabulary_is_closed_and_round_trips() {
    let names: Vec<&str> = OperatorKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(
        names,
        [
            "partition",
            "extraction",
            "filtration",
            "reconstruction",
            "association",
            "collection",
            "construction",
            "reduction",
            "evaluation"
        ]
    );
    for k in OperatorKind::ALL {
        assert_eq!(OperatorKind::parse(k.as_str()).unwrap(), k);
    }
}

// ------------------------------------------------------------------ AC-6: schema export

#[test]
fn ac6_exported_json_schema_is_2020_12_and_closed() {
    let CValue::Obj(s) = fid_canon::parse(DMT_TABLE_JSON_SCHEMA.as_bytes()).unwrap() else { panic!() };
    assert_eq!(s.get("$schema"), Some(&CValue::Str("https://json-schema.org/draft/2020-12/schema".to_owned())));
    assert_eq!(s.get("additionalProperties"), Some(&CValue::Bool(false)));
    let file =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/dmt-table-v1.schema.json")).unwrap();
    assert_eq!(DMT_TABLE_JSON_SCHEMA, file);
    // Validation of the fixtures with an independent validator: fixtures/gdt/dmt/check_schema.py (CI).
}

#[test]
fn ac6_digest_is_platform_independent() {
    // Pinned literal (from the Python reference); this test runs on Linux, Windows and macOS CI.
    assert_eq!(table().digest().as_str(), "sha256:aeedc7fcc3731625722395c31fccaf056f2b6a179149a912119c2efed09b9d17");
}

// ------------------------------------------------------------------ other errors

#[test]
fn bad_algorithm_ids() {
    for alg in [
        "syn.partition.whole_feature",
        "syn.partition@0.1.0",
        "syn.partition.whole.feature@0.1.0",
        "syn.partition.whole_feature@0.1",
        "syn.partition.whole_feature@01.1.0",
        "Syn.partition.whole_feature@0.1.0",
        "syn.partition.whole_feature@0.1.0-rc1",
        "",
    ] {
        let e = fail(|d| drop(op0(d).insert("alg".to_owned(), s(alg))));
        assert_eq!(e.code(), "bad_algorithm_id", "{alg:?}");
    }
}

#[test]
fn free_text_is_capped_at_280_characters() {
    let e = fail(|d| drop(row(d, 0).insert("notes".to_owned(), s(&"x".repeat(281)))));
    assert_eq!(e, DmtError::FieldTooLong { field: "notes" });
    // 280 two-byte characters are fine: the cap counts characters, not bytes.
    let mut d = doc();
    row(&mut d, 0).insert("notes".to_owned(), s(&"\u{e9}".repeat(280)));
    assert!(load(&d).is_ok());
    assert_eq!(malformed(&fail(|d| drop(row(d, 0).insert("notes".to_owned(), s("a\u{7}b"))))), "bad_notes");
    assert_eq!(malformed(&fail(|d| drop(row(d, 0).insert("notes".to_owned(), s("a\u{85}b"))))), "bad_notes");
}

#[test]
fn grammar_fields_are_length_checked_first() {
    let e = fail(|d| {
        let clauses = arr(row(d, 0).get_mut("clauses").unwrap());
        obj(&mut clauses[0]).insert("clause".to_owned(), s(&"1.".repeat(13)));
    });
    assert_eq!(e, DmtError::FieldTooLong { field: "clause" });
    let e = fail(|d| drop(row(d, 0).insert("actual_value".to_owned(), s(&format!("{}@1", "a".repeat(95))))));
    assert_eq!(e, DmtError::FieldTooLong { field: "actual_value" });
    let e = fail(|d| {
        drop(
            op0(d)
                .insert("params".to_owned(), CValue::Obj([("t".to_owned(), s(&"a".repeat(65)))].into_iter().collect())),
        )
    });
    assert_eq!(e, DmtError::FieldTooLong { field: "param" });
}

/// Loads the fixture after `f` edits it and checks the `malformed_table` reason.
fn expect(reason: &str, f: impl FnOnce(&mut CValue)) {
    assert_eq!(malformed(&fail(f)), reason);
}

#[test]
fn malformed_tables() {
    let set = |field: &'static str, v: CValue| move |d: &mut CValue| drop(row(d, 0).insert(field.to_owned(), v));
    expect("unknown_field", |d: &mut CValue| drop(obj(d).insert("extra".to_owned(), CValue::Bool(true))));
    expect("missing_field", |d: &mut CValue| drop(row(d, 0).remove("review")));
    expect("bad_schema", |d: &mut CValue| drop(obj(d).insert("schema".to_owned(), s("fid.dmt-table/2"))));
    expect("bad_version", |d: &mut CValue| drop(obj(d).insert("version".to_owned(), s("1.0"))));
    expect("wrong_type", |d: &mut CValue| drop(obj(d).insert("version".to_owned(), CValue::Num(1.0))));
    expect("key_mismatch", set("dialect", s("iso")));
    expect("key_mismatch", set("characteristic", s("flatness")));
    expect("key_mismatch", set("modifiers", strs(&["mmc"])));
    expect("bad_dialect", set("dialect", s("ASME")));
    expect("not_sorted", set("editions", strs(&["y14.5-2018", "y14.5-2009"])));
    expect("not_sorted", set("editions", strs(&["y14.5-2018", "y14.5-2018"])));
    expect("bad_count", set("editions", strs(&[])));
    expect("bad_edition", set("editions", strs(&["Y14.5-2018"])));
    expect("bad_count", set("clauses", CValue::Arr(vec![])));
    expect("bad_count", set("variants", CValue::Arr(vec![])));
    expect("unknown_default_variant", set("default_variant", s("axis")));
    expect("bad_actual_value", set("actual_value", s("av.synthetic")));
    expect("bad_decision_rule_hint", set("decision_rule_hint", s("DR@1")));
    expect("bad_ors_case", set("ors_cases", strs(&["ORS:1"])));
    expect("not_sorted", set("ors_cases", strs(&["ors:b", "ors:a"])));
    expect("bad_status", set("status", s("Supported")));
    expect("bad_limitation", set("limitations", strs(&["free text here"])));
    expect("not_sorted", set("limitations", strs(&["b", "a"])));
    expect("bad_review_state", set("review", CValue::Obj([("state".to_owned(), s("approved"))].into_iter().collect())));
    expect(
        "unknown_field",
        set(
            "review",
            CValue::Obj(
                [("state".to_owned(), s("draft")), ("reviewers".to_owned(), strs(&["a"]))].into_iter().collect(),
            ),
        ),
    );
    expect("missing_field", set("review", CValue::Obj([("state".to_owned(), s("signed_off"))].into_iter().collect())));
    expect(
        "bad_count",
        set(
            "review",
            CValue::Obj(
                [("state".to_owned(), s("signed_off")), ("reviewers".to_owned(), strs(&[]))].into_iter().collect(),
            ),
        ),
    );
    expect(
        "not_sorted",
        set(
            "review",
            CValue::Obj(
                [("state".to_owned(), s("signed_off")), ("reviewers".to_owned(), strs(&["rev-a", "rev-a"]))]
                    .into_iter()
                    .collect(),
            ),
        ),
    );
    expect("wrong_type", set("synthetic", s("true")));
    expect("wrong_type", set("notes", CValue::Null));
    expect("bad_param", |d: &mut CValue| {
        drop(
            op0(d).insert("params".to_owned(), CValue::Obj([("x".to_owned(), CValue::Num(1.5))].into_iter().collect())),
        )
    });
    expect("bad_param", |d: &mut CValue| {
        drop(op0(d).insert(
            "params".to_owned(),
            CValue::Obj([("x".to_owned(), s("0x7ff8000000000000"))].into_iter().collect()),
        ))
    });
    expect("bad_param", |d: &mut CValue| {
        drop(op0(d).insert("params".to_owned(), CValue::Obj([("x".to_owned(), CValue::Null)].into_iter().collect())))
    });
    expect("bad_param_name", |d: &mut CValue| {
        drop(
            op0(d)
                .insert("params".to_owned(), CValue::Obj([("X".to_owned(), CValue::Bool(true))].into_iter().collect())),
        )
    });
    expect("unknown_field", |d: &mut CValue| drop(op0(d).insert("note".to_owned(), s("x"))));
    expect("unknown_field", |d: &mut CValue| {
        let clauses = arr(row(d, 0).get_mut("clauses").unwrap());
        obj(&mut clauses[0]).insert("text".to_owned(), s("x"));
    });
    expect("bad_clause", |d: &mut CValue| {
        let clauses = arr(row(d, 0).get_mut("clauses").unwrap());
        obj(&mut clauses[0]).insert("clause".to_owned(), s("see fig"));
    });
    expect("not_sorted", |d: &mut CValue| {
        // perpendicularity (row 7) has variants axis, surface: swap them.
        arr(row(d, 7).get_mut("variants").unwrap()).reverse();
    });
    expect("wrong_type", |d: &mut CValue| drop(obj(d).insert("rows".to_owned(), CValue::Obj(BTreeMap::new()))));
}

#[test]
fn input_limits() {
    // Too deep: a parameter value that is itself an array is one level past the limit.
    let e =
        fail(|d| {
            drop(op0(d).insert(
                "params".to_owned(),
                CValue::Obj([("x".to_owned(), CValue::Arr(vec![]))].into_iter().collect()),
            ))
        });
    assert_eq!(malformed(&e), "input_limit");
    assert_eq!(malformed(&DmtTable::load(&vec![b' '; 32 * 1024 * 1024 + 1]).unwrap_err()), "too_large");
    assert_eq!(malformed(&DmtTable::load(b"{").unwrap_err()), "syntax");
    assert_eq!(malformed(&DmtTable::load(&[0xff, 0xfe]).unwrap_err()), "invalid_utf8");
    assert_eq!(malformed(&DmtTable::load(b"[]").unwrap_err()), "wrong_type");
}

#[test]
fn more_than_ten_thousand_rows_are_refused() {
    let empty = r#"{"schema":"fid.dmt-table/1","version":"1.0.0","rows":["#;
    let mut text = String::from(empty);
    for i in 0..10_001 {
        if i > 0 {
            text.push(',');
        }
        text.push_str("{}");
    }
    text.push_str("]}");
    assert_eq!(malformed(&DmtTable::load(text.as_bytes()).unwrap_err()), "too_many_rows");
}

#[test]
fn a_five_thousand_row_table_loads() {
    let mut text = String::from(r#"{"schema":"fid.dmt-table/1","version":"2.0.0","rows":["#);
    for i in 0..5_000 {
        if i > 0 {
            text.push(',');
        }
        text.push_str(&format!(
            r#"{{"key":"dmt:asme/c{i:05}/none@1","dialect":"asme","editions":["y14.5-2018"],"characteristic":"c{i:05}","modifiers":[],"clauses":[{{"designator":"asme-y14.5","edition":"2018","clause":"90.1"}}],"variants":[{{"variant":"placeholder","steps":[]}}],"default_variant":"placeholder","actual_value":"av.synthetic.x@1","decision_rule_hint":"dr.synthetic.default@1","ors_cases":[],"status":"unsupported","limitations":[],"review":{{"state":"draft"}},"notes":"","synthetic":true}}"#
        ));
    }
    text.push_str("]}");
    let t = DmtTable::load(text.as_bytes()).unwrap();
    assert_eq!(t.rows().len(), 5_000);
}

#[test]
fn hostile_inputs_never_panic() {
    let bytes = read("synthetic_table_v1.json");
    // Every truncation on a 97-byte stride, and a byte flip at every 211th position.
    for cut in (0..bytes.len()).step_by(97) {
        let _ = DmtTable::load(&bytes[..cut]);
    }
    for at in (0..bytes.len()).step_by(211) {
        for flip in [0x01u8, 0x20, 0x80] {
            let mut b = bytes.clone();
            b[at] ^= flip;
            let _ = DmtTable::load(&b);
        }
    }
    for k in ["dmt:", "dmt:asme//none@1", "dmt:asme/a/none@", "@", "dmt:asme/a/\u{e9}@1", "dmt:asme/a/none@\u{661}"] {
        assert!(DmtKey::parse(k).is_err());
    }
}

// ------------------------------------------------------------------ types

#[test]
fn row_fields_are_typed() {
    let t = table();
    let r = t.get("dmt:asme/position/cz+mmc@1").unwrap();
    assert_eq!(r.status(), RowStatus::Partial);
    assert_eq!(r.limitations(), ["pattern_size_limited", "zone_def_unsupported:nonuniform"]);
    assert_eq!(*r.review(), ReviewState::UnderReview);
    assert_eq!(r.default_chain().unwrap().variant(), "axis");
    let op = &r.variant("surface").unwrap().steps()[3];
    assert_eq!(op.kind(), OperatorKind::Construction);
    assert_eq!(op.params().get("fraction").and_then(Param::as_f64), Some(0.5));
    assert_eq!(r.variant("surface").unwrap().steps()[4].params().get("sign"), Some(&Param::Int(-1)));
    let f = t.get("dmt:asme/flatness/none@1").unwrap();
    assert_eq!(*f.review(), ReviewState::SignedOff { reviewers: vec!["rev-a".to_owned(), "rev-b".to_owned()] });
    assert_eq!(f.clauses()[0].designator, "asme-y14.5");
    let stub = t.get("dmt:iso/flatness/none@1").unwrap();
    assert!(stub.variants()[0].steps().is_empty() && stub.ors_cases().is_empty() && stub.notes().is_empty());
    assert!(t.get("dmt:asme/flatness/mmc@1").is_none());
    assert!(t.get("not a key").is_none());
}

// ------------------------------------------------------------------ defaults-profile keys

#[test]
fn profile_vocabulary_is_named_keys_plus_multi_variant_rows() {
    let t = table();
    let asme = profile_vocabulary(&t, Dialect::Asme);
    let keys: Vec<&str> = asme.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "dmt:asme/perpendicularity/none@1",
            "dmt:asme/position/cz+mmc@1",
            "dmt:asme/position/mmc@1",
            "dmt:asme/position/none@1",
            "dmt:asme/size/none@1",
            "gdt.datum.label_pattern",
            "scan.edge.extraction_method",
            "unc.decision_rule_hint",
        ]
    );
    assert_eq!(
        asme.get("dmt:asme/size/none@1"),
        Some(&ProfileKeyRule::Variant(vec!["global".to_owned(), "local".to_owned()]))
    );
    assert_eq!(asme.get("unc.decision_rule_hint"), Some(&ProfileKeyRule::Text));
    let iso = profile_vocabulary(&t, Dialect::Iso);
    assert_eq!(iso.len(), NAMED_PROFILE_KEYS.len());
    // Named keys are ascending and every vocabulary key fits the defaults-profile key grammar.
    assert!(NAMED_PROFILE_KEYS.windows(2).all(|w| w[0].0 < w[1].0));
    for k in asme.keys() {
        let b = k.as_bytes();
        assert!(b.len() <= 128 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()));
        assert!(b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_.:/+@-".contains(&c)), "{k}");
    }
}

// ------------------------------------------------------------------ construction through the API

fn op(params: BTreeMap<String, Param>) -> Result<Operator, DmtError> {
    Operator::new(OperatorKind::Partition, AlgId::parse("syn.partition.whole_feature@0.1.0").unwrap(), params)
}

fn one(name: &str, p: Param) -> BTreeMap<String, Param> {
    [(name.to_owned(), p)].into_iter().collect()
}

#[test]
fn over_limit_operators_and_chains_cannot_be_built() {
    // Operators: every parameter rule of the loader also holds for `Operator::new`.
    let many: BTreeMap<String, Param> = (0..33).map(|i| (format!("p{i}"), Param::Bool(true))).collect();
    assert_eq!(malformed(&op(many).unwrap_err()), "too_many_params");
    assert_eq!(op(one("t", Param::Token("a".repeat(65)))).unwrap_err(), DmtError::FieldTooLong { field: "param" });
    assert_eq!(malformed(&op(one("t", Param::Token("Bad".to_owned()))).unwrap_err()), "bad_param");
    assert_eq!(malformed(&op(one("t", Param::Token(String::new()))).unwrap_err()), "bad_param");
    assert_eq!(malformed(&op(one("f", Param::F64Bits(f64::NAN.to_bits()))).unwrap_err()), "bad_param");
    assert_eq!(malformed(&op(one("f", Param::F64Bits(f64::INFINITY.to_bits()))).unwrap_err()), "bad_param");
    assert_eq!(malformed(&op(one("X", Param::Bool(true))).unwrap_err()), "bad_param_name");
    assert_eq!(op(one(&"n".repeat(33), Param::Bool(true))).unwrap_err(), DmtError::FieldTooLong { field: "param_name" });
    let ok = op(one("f", Param::F64Bits(0.5f64.to_bits()))).unwrap();
    assert_eq!(ok.params().len(), 1);
    // Chains: at most 64 steps and a grammar-checked name.
    let steps = vec![ok.clone(); 65];
    assert_eq!(malformed(&OperatorChain::new("default", steps).unwrap_err()), "too_many_steps");
    assert_eq!(malformed(&OperatorChain::new("Default", vec![]).unwrap_err()), "bad_variant");
    assert_eq!(OperatorChain::new(&"v".repeat(33), vec![]).unwrap_err(), DmtError::FieldTooLong { field: "variant" });
    let c = OperatorChain::new("default", vec![ok; 64]).unwrap();
    assert_eq!(c.steps().len(), 64);
    assert_eq!(c.digest(), Digest::from_bytes(fid_canon::digest_bytes(c.canonical_bytes()).0));
}

#[test]
fn over_limit_rows_cannot_be_built_or_inserted() {
    // A row exists only through `row_from_value`, which refuses an over-limit row.
    let mut d = doc();
    row(&mut d, 0).insert("notes".to_owned(), s(&"x".repeat(281)));
    let CValue::Arr(rs) = obj(&mut d).remove("rows").unwrap() else { panic!() };
    assert_eq!(row_from_value(&rs[0]).unwrap_err(), DmtError::FieldTooLong { field: "notes" });
    let mut d = doc();
    let variants = arr(row(&mut d, 0).get_mut("variants").unwrap());
    let steps = arr(obj(&mut variants[0]).get_mut("steps").unwrap());
    let first = steps[0].clone();
    steps.resize(65, first);
    assert_eq!(malformed(&load(&d).unwrap_err()), "too_many_steps");

    // `DmtTable::new` re-validates every row and its placement; valid rows give the same table.
    let t = table();
    let again = DmtTable::new(t.version(), t.rows().clone()).unwrap();
    assert_eq!(again.digest(), t.digest());
    let mut moved = t.rows().clone();
    let r = moved.remove(&DmtKey::parse("dmt:asme/flatness/none@1").unwrap()).unwrap();
    moved.insert(DmtKey::parse("dmt:asme/flatness/mmc@1").unwrap(), r);
    assert_eq!(malformed(&DmtTable::new(t.version(), moved).unwrap_err()), "key_mismatch");
    let v2 = SemVer::parse("2.0.0").unwrap();
    assert_ne!(DmtTable::new(v2, t.rows().clone()).unwrap().digest(), t.digest());
}

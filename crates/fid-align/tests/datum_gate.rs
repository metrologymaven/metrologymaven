//! Integration tests for spec node `ALN.DATUM.SYS.GATE` (AC-1..AC-4, REQ-001..005).
//!
//! Fixtures (`fixtures/aln/datum/sys/gate_asme.json` and test-only DMT tables) are generated
//! by the independent stdlib-Python oracle `fixtures/aln/datum/sys/make_gate_fixtures.py`
//! (CC0, `synthetic: true`, no standards text).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align::datum_gate::{
    compute_combined_digest, resolve_datum_chain, DatumFeatureType, DatumModifier, DatumPrecedence, DatumRef,
    DialectContext, GateError,
};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_gps::dmt::{Dialect, DmtTable};
use fid_gps::resolve::VariantSource;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aln/datum/sys")
}

fn load_fixture_json(name: &str) -> BTreeMap<String, CValue> {
    let bytes = std::fs::read(fixture_dir().join(name)).expect("read fixture");
    match parse(&bytes).expect("parse fixture JSON") {
        CValue::Obj(o) => o,
        other => panic!("expected object in {name}, got {other:?}"),
    }
}

fn load_table(name: &str) -> DmtTable {
    let bytes = std::fs::read(fixture_dir().join(name)).expect("read DMT table fixture");
    DmtTable::load(&bytes).expect("load test DMT table")
}

fn str_field<'a>(obj: &'a BTreeMap<String, CValue>, key: &str) -> &'a str {
    match obj.get(key) {
        Some(CValue::Str(s)) => s.as_str(),
        other => panic!("expected string at {key}, got {other:?}"),
    }
}

fn str_vec(obj: &BTreeMap<String, CValue>, key: &str) -> Vec<String> {
    match obj.get(key) {
        Some(CValue::Arr(arr)) => arr
            .iter()
            .map(|v| match v {
                CValue::Str(s) => s.clone(),
                other => panic!("expected string in {key}, got {other:?}"),
            })
            .collect(),
        other => panic!("expected array at {key}, got {other:?}"),
    }
}

fn load_gate_asme() -> (Vec<DatumRef>, DialectContext, BTreeMap<String, CValue>) {
    let root = load_fixture_json("gate_asme.json");
    let CValue::Obj(ctx_obj) = root.get("context").expect("context") else {
        panic!("context not object");
    };
    let dialect = match str_field(ctx_obj, "dialect") {
        "asme" => Dialect::Asme,
        "iso" => Dialect::Iso,
        other => panic!("unknown dialect {other}"),
    };
    let ctx = DialectContext {
        dialect: Some(dialect),
        edition: Some(str_field(ctx_obj, "edition").to_string()),
        defaults_profile: Some(str_field(ctx_obj, "defaults_profile").to_string()),
        profile_choices: BTreeMap::new(),
    };
    let CValue::Arr(datum_arr) = root.get("datums").expect("datums") else {
        panic!("datums not array");
    };
    let mut datums = Vec::new();
    for item in datum_arr {
        let CValue::Obj(d) = item else { panic!("datum not object") };
        let label = str_field(d, "label").to_string();
        let feature_type = DatumFeatureType::parse(str_field(d, "feature_type")).unwrap();
        let modifiers = str_vec(d, "modifiers").iter().map(|m| DatumModifier::parse(m).unwrap()).collect();
        datums.push(DatumRef { label, feature_type, modifiers, variant_override: None });
    }
    let CValue::Obj(expected) = root.get("expected").expect("expected") else {
        panic!("expected not object");
    };
    (datums, ctx, expected.clone())
}

#[test]
fn ac1_gate_asme_resolves_three_datums_with_expected_row_ids_and_combined_digest() {
    let (datums, ctx, expected) = load_gate_asme();
    let dmt = load_table("test_dmt_supported.json");

    let resolved = resolve_datum_chain(&datums, &ctx, &dmt).expect("AC-1 resolution must succeed");

    assert_eq!(resolved.per_datum.len(), 3);
    assert_eq!(resolved.dmt_digest.as_str(), str_field(&expected, "dmt_digest"));
    assert_eq!(resolved.combined_digest.as_str(), str_field(&expected, "combined_digest"));

    let expected_row_ids = str_vec(&expected, "row_ids");
    let expected_variants = str_vec(&expected, "variants");
    let expected_sources = str_vec(&expected, "variant_sources");
    let expected_chain_digests = str_vec(&expected, "chain_digests");
    let labels = ["A", "B", "C"];
    let precs = [DatumPrecedence::Primary, DatumPrecedence::Secondary, DatumPrecedence::Tertiary];

    for i in 0..3 {
        let d = &resolved.per_datum[i];
        assert_eq!(d.label, labels[i]);
        assert_eq!(d.precedence, precs[i]);
        assert_eq!(d.row_id.as_str(), expected_row_ids[i]);
        assert_eq!(d.variant, expected_variants[i]);
        assert_eq!(d.variant_source.as_str(), expected_sources[i]);
        assert_eq!(d.chain_digest.as_str(), expected_chain_digests[i]);
        assert!(d.limitations.is_empty());
    }

    // Recompute combined digest directly from per-datum digests.
    let digests: Vec<Digest> = resolved.per_datum.iter().map(|d| d.chain_digest).collect();
    assert_eq!(compute_combined_digest(&digests).unwrap(), resolved.combined_digest);

    // D0 determinism (REQ-005): identical inputs give byte-identical output.
    let resolved_again = resolve_datum_chain(&datums, &ctx, &dmt).unwrap();
    assert_eq!(resolved, resolved_again);
    assert_eq!(
        to_canonical_bytes(&resolved.to_cvalue()).unwrap(),
        to_canonical_bytes(&resolved_again.to_cvalue()).unwrap()
    );
}

#[test]
fn ac2_missing_defaults_profile_or_context_fields_fail_dialect_unspecified() {
    let (datums, mut ctx, _) = load_gate_asme();
    let dmt = load_table("test_dmt_supported.json");

    // AC-2: defaults_profile missing -> dialect_unspecified{fields: [defaults_profile]}
    ctx.defaults_profile = None;
    let err = resolve_datum_chain(&datums, &ctx, &dmt).unwrap_err();
    assert_eq!(err.code(), "dialect_unspecified");
    assert_eq!(err, GateError::DialectUnspecified { fields: vec!["defaults_profile"] });

    // Empty/whitespace defaults_profile is also refused (never defaulted).
    ctx.defaults_profile = Some("   ".to_string());
    let err_empty = resolve_datum_chain(&datums, &ctx, &dmt).unwrap_err();
    assert_eq!(err_empty, GateError::DialectUnspecified { fields: vec!["defaults_profile"] });

    // All three missing lists every missing field in deterministic order.
    let empty_ctx = DialectContext::default();
    let err_all = resolve_datum_chain(&datums, &empty_ctx, &dmt).unwrap_err();
    assert_eq!(err_all, GateError::DialectUnspecified { fields: vec!["dialect", "edition", "defaults_profile"] });
}

#[test]
fn ac3_secondary_datum_unsupported_passes_through_label_row_and_key() {
    let (datums, ctx, expected) = load_gate_asme();
    let dmt = load_table("test_dmt_secondary_unsupported.json");

    let err = resolve_datum_chain(&datums, &ctx, &dmt).unwrap_err();
    assert_eq!(err.code(), "UNSUPPORTED");
    match err {
        GateError::Unsupported { label, row_id, key, dmt_digest } => {
            assert_eq!(label, "B");
            assert_eq!(row_id.as_str(), "dmt:asme/datum_plane_secondary/rmb@1");
            assert_eq!(key, "dmt:asme/datum_plane_secondary/rmb");
            assert_eq!(dmt_digest.as_str(), str_field(&expected, "secondary_unsupported_dmt_digest"));
        }
        other => panic!("expected GateError::Unsupported, got {other:?}"),
    }
}

#[test]
fn ac4_mmb_and_lmb_modifiers_fail_unsupported_modifier_p1a() {
    let (mut datums, ctx, _) = load_gate_asme();
    let dmt = load_table("test_dmt_supported.json");

    // Datum B at MMB -> unsupported_modifier_p1a.
    datums[1].modifiers = vec![DatumModifier::Mmb];
    let err = resolve_datum_chain(&datums, &ctx, &dmt).unwrap_err();
    assert_eq!(err.code(), "unsupported_modifier_p1a");
    assert_eq!(err, GateError::UnsupportedModifierP1a { label: "B".to_string(), modifier: "mmb".to_string() });

    // Datum C at LMB -> unsupported_modifier_p1a.
    datums[1].modifiers = vec![DatumModifier::Rmb];
    datums[2].modifiers = vec![DatumModifier::Lmb];
    let err_lmb = resolve_datum_chain(&datums, &ctx, &dmt).unwrap_err();
    assert_eq!(err_lmb.code(), "unsupported_modifier_p1a");
    assert_eq!(err_lmb, GateError::UnsupportedModifierP1a { label: "C".to_string(), modifier: "lmb".to_string() });
}

#[test]
fn unresolved_profile_choice_override_and_input_validation() {
    let (mut datums, mut ctx, _) = load_gate_asme();
    let dmt = load_table("test_dmt_supported.json");

    // Unresolved datum feature type passes through UNRESOLVED with label and derived key.
    let cyl = vec![DatumRef::rmb("A", DatumFeatureType::Cylinder)];
    let err = resolve_datum_chain(&cyl, &ctx, &dmt).unwrap_err();
    assert_eq!(err.code(), "UNRESOLVED");
    match err {
        GateError::Unresolved { label, key, dmt_digest } => {
            assert_eq!(label, "A");
            assert_eq!(key, "dmt:asme/datum_cylinder_primary/rmb");
            assert_eq!(*dmt_digest, dmt.digest());
        }
        other => panic!("expected UNRESOLVED, got {other:?}"),
    }

    // Profile choice and explicit override are forwarded to fid-gps and reflected in output.
    ctx.profile_choices.insert("dmt:asme/datum_plane_primary/rmb@1".to_string(), "lsq_plane".to_string());
    datums[1].variant_override = Some("lsq_plane".to_string());
    let resolved = resolve_datum_chain(&datums, &ctx, &dmt).unwrap();
    assert_eq!(resolved.per_datum[0].variant, "lsq_plane");
    assert_eq!(resolved.per_datum[0].variant_source, VariantSource::Profile);
    assert_eq!(resolved.per_datum[1].variant, "lsq_plane");
    assert_eq!(resolved.per_datum[1].variant_source, VariantSource::Override);
    assert_eq!(resolved.per_datum[2].variant, "tangent_plane");
    assert_eq!(resolved.per_datum[2].variant_source, VariantSource::RowDefault);

    // Input validation: empty, >3, duplicate label.
    assert_eq!(resolve_datum_chain(&[], &ctx, &dmt).unwrap_err().code(), "bad_input");
    let dup = vec![DatumRef::rmb("A", DatumFeatureType::Plane), DatumRef::rmb("A", DatumFeatureType::Plane)];
    assert_eq!(resolve_datum_chain(&dup, &ctx, &dmt).unwrap_err().code(), "bad_input");
    let four = vec![
        DatumRef::rmb("A", DatumFeatureType::Plane),
        DatumRef::rmb("B", DatumFeatureType::Plane),
        DatumRef::rmb("C", DatumFeatureType::Plane),
        DatumRef::rmb("D", DatumFeatureType::Plane),
    ];
    assert_eq!(resolve_datum_chain(&four, &ctx, &dmt).unwrap_err().code(), "bad_input");
}

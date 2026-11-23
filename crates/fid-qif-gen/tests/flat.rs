//! Integration tests for `IO.QIF.GEN.FLAT` (AC-1..AC-6, cycle detection, identity tables,
//! and `fid-qif-ir` canonical JSON round-trip).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use fid_canon::{to_canonical_bytes, CValue};
use fid_qif_gen::flat::{encode_qif_ir, flatten_pinned, FlatError};
use fid_qif_gen::pin::PinnedSet;
use fid_qif_ir::{CountTarget, IrIdentityKind, IrQName, SchemaIr, MAX_IR_BYTES};

const NS: &str = "http://example.org/qif-flat";

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/qif/gen/flat")
}

fn parse_fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/qif/gen/parse")
}

#[test]
fn ac1_extension_chain_flattens_base_before_derived_and_matches_golden_ir() {
    let pinned = PinnedSet::verify(&fixtures().join("ext_chain")).expect("verify ext_chain");
    let ir = flatten_pinned(&pinned).expect("flatten ext_chain");

    let (actual_bytes, digest) = encode_qif_ir(&ir).expect("encode qif-ir.json");
    let expected_bytes = fs::read(fixtures().join("ext_chain.ir.json")).expect("read ext_chain.ir.json");
    assert_eq!(
        String::from_utf8_lossy(&actual_bytes),
        String::from_utf8_lossy(&expected_bytes),
        "flattened ext_chain IR must match ext_chain.ir.json byte-for-byte"
    );

    // Round-trip through fid-qif-ir runtime decoder yields identical SchemaIr and SHA-256.
    let decoded = SchemaIr::from_json(&actual_bytes).expect("decode qif-ir.json");
    assert_eq!(decoded, ir);
    assert_eq!(decoded.digest().expect("digest"), digest);
}

#[test]
fn ac2_nested_anonymous_types_follow_fixed_naming_rule() {
    let pinned = PinnedSet::verify(&fixtures().join("anon")).expect("verify anon");
    let ir = flatten_pinned(&pinned).expect("flatten anon");

    let names_val =
        CValue::Obj(ir.names.iter().map(|(k, v)| (k.clone(), CValue::Str(v.clone()))).collect::<BTreeMap<_, _>>());
    let actual = to_canonical_bytes(&names_val).expect("canonical names");
    let expected = fs::read(fixtures().join("anon.names.json")).expect("read anon.names.json");
    assert_eq!(
        String::from_utf8_lossy(&actual),
        String::from_utf8_lossy(&expected),
        "anonymous type names must match anon.names.json"
    );
    assert_eq!(ir.types.len(), 2);
}

#[test]
fn ac3_colliding_anonymous_paths_fail_with_ir_name_collision_and_both_positions() {
    let pinned = PinnedSet::verify(&fixtures().join("collide")).expect("verify collide");
    let err = flatten_pinned(&pinned).expect_err("colliding anonymous paths must fail");
    assert_eq!(err.code(), "ir_name_collision");
    match err {
        FlatError::NameCollision { name, first, second } => {
            assert_eq!(name, format!("{{{NS}}}A_B_C_Anon"));
            assert_eq!(first.file, "collide.xsd");
            assert_eq!(second.file, "collide.xsd");
            assert_ne!(first.line, second.line, "must report both distinct source positions");
        }
        other => panic!("expected NameCollision, got {other:?}"),
    }
}

#[test]
fn ac4_substitution_group_lists_members_in_source_order_and_marks_abstract_head_non_instantiable() {
    let pinned = PinnedSet::verify(&fixtures().join("subst")).expect("verify subst");
    let ir = flatten_pinned(&pinned).expect("flatten subst");

    let head_key = format!("{{{NS}}}Shape");
    let head = ir.elements.get(&head_key).expect("Shape head must exist");
    assert!(head.is_abstract);
    assert!(!head.instantiable, "abstract head must never be offered as instantiable");

    let members = ir.substitution.get(&head_key).expect("substitution group entry for Shape");
    let expected = vec![
        IrQName::new(Some(NS.to_string()), "Circle"),
        IrQName::new(Some(NS.to_string()), "Plane"),
        IrQName::new(Some(NS.to_string()), "Cylinder"),
    ];
    assert_eq!(members, &expected, "members must be in exact source order");
}

#[test]
fn ac5_count_rules_link_repeated_child_and_reject_unlinked_count() {
    // Valid `n` attribute on list type links to repeated `Point` child.
    let pinned = PinnedSet::verify(&fixtures().join("counts")).expect("verify counts");
    let ir = flatten_pinned(&pinned).expect("flatten counts");
    assert_eq!(ir.count_rules.len(), 1);
    let rule = &ir.count_rules[0];
    assert_eq!(rule.owner_type, IrQName::new(Some(NS.to_string()), "PointListType"));
    assert_eq!(rule.attribute, IrQName::new(None, "n"));
    assert_eq!(rule.target, CountTarget::Element(IrQName::new(Some(NS.to_string()), "Point")));

    // Unmatched `count` attribute fails with `count_rules_unlinked` naming it.
    let bad_pinned = PinnedSet::verify(&fixtures().join("counts_bad")).expect("verify counts_bad");
    let err = flatten_pinned(&bad_pinned).expect_err("unlinked count must fail");
    assert_eq!(err.code(), "count_rules_unlinked");
    match err {
        FlatError::CountRulesUnlinked { unlinked } => {
            assert_eq!(unlinked.len(), 1);
            assert_eq!(unlinked[0].owner_type, IrQName::new(Some(NS.to_string()), "BrokenCountType"));
            assert_eq!(unlinked[0].attribute, IrQName::new(None, "count"));
            assert_eq!(unlinked[0].source.file, "counts_bad.xsd");
        }
        other => panic!("expected CountRulesUnlinked, got {other:?}"),
    }
}

#[test]
fn ac6_two_runs_in_different_working_dirs_yield_identical_qif_ir_sha256() {
    // Copy the multi-file QIF 3.0-shaped bundle into two separate temp directories and verify
    // byte-identical `qif-ir.json` and identical SHA-256 (REQ-IO.QIF.GEN.FLAT-005).
    let src = parse_fixtures().join("qif3_bundle");
    let base = std::env::temp_dir().join(format!("fid_qif_flat_ac6_{}", std::process::id()));
    let dir_a = base.join("workdir_alpha");
    let dir_b = base.join("nested/workdir_beta");
    let _ = fs::remove_dir_all(&base);
    copy_dir_all(&src, &dir_a);
    copy_dir_all(&src, &dir_b);

    let pinned_a = PinnedSet::verify(&dir_a).expect("verify bundle in dir_a");
    let pinned_b = PinnedSet::verify(&dir_b).expect("verify bundle in dir_b");
    let ir_a = flatten_pinned(&pinned_a).expect("flatten dir_a");
    let ir_b = flatten_pinned(&pinned_b).expect("flatten dir_b");

    let (bytes_a, digest_a) = encode_qif_ir(&ir_a).expect("encode dir_a");
    let (bytes_b, digest_b) = encode_qif_ir(&ir_b).expect("encode dir_b");
    assert_eq!(bytes_a, bytes_b, "qif-ir.json must be byte-identical across working directories");
    assert_eq!(digest_a, digest_b, "SHA-256 digest must match across working directories");

    // Verify bijective IR-name -> emit-name map (REQ-006).
    let unique_emits: std::collections::BTreeSet<&String> = ir_a.names.values().collect();
    assert_eq!(unique_emits.len(), ir_a.names.len(), "emit names must be strictly 1-to-1");
    assert_eq!(ir_a.names.len(), ir_a.types.len());

    // Round-trip decoded SchemaIr.
    let decoded = SchemaIr::from_json(&bytes_a).expect("decode qif3_bundle qif-ir.json");
    assert_eq!(decoded, ir_a);

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn base_type_cycle_and_identity_tables_and_bounded_decoder() {
    // Base-type cycle fails with `ir_base_cycle`.
    let cyc_pinned = PinnedSet::verify(&fixtures().join("cycle")).expect("verify cycle");
    let err = flatten_pinned(&cyc_pinned).expect_err("cycle must fail");
    assert_eq!(err.code(), "ir_base_cycle");
    match err {
        FlatError::BaseCycle { cycle, .. } => {
            assert!(cycle.len() >= 3, "cycle should list closed loop, got {cycle:?}");
        }
        other => panic!("expected BaseCycle, got {other:?}"),
    }

    // Identity constraints (`key`, `keyref`, `unique`) compile into `identity` tables.
    let id_pinned = PinnedSet::verify(&fixtures().join("identity")).expect("verify identity");
    let id_ir = flatten_pinned(&id_pinned).expect("flatten identity");
    assert_eq!(id_ir.identity.len(), 3);
    let kinds: Vec<IrIdentityKind> = id_ir.identity.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&IrIdentityKind::Key));
    assert!(kinds.contains(&IrIdentityKind::Keyref));
    assert!(kinds.contains(&IrIdentityKind::Unique));

    // Bounded decoder rejects oversized input cleanly.
    let huge = vec![b' '; MAX_IR_BYTES + 1];
    assert!(SchemaIr::from_json(&huge).is_err());
}

#[test]
#[ignore = "AC-6 upstream DMSC QIF 3.0 XSDs not pinned yet (licence unverified); see xsd/qif3/README.md"]
fn ac6_real_qif3_set_flattens_deterministically() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../xsd/qif3");
    let pinned = PinnedSet::verify(&dir).expect("real QIF 3.0 pinned set must verify");
    let ir1 = flatten_pinned(&pinned).expect("first flatten");
    let ir2 = flatten_pinned(&pinned).expect("second flatten");
    let (_, d1) = encode_qif_ir(&ir1).expect("encode 1");
    let (_, d2) = encode_qif_ir(&ir2).expect("encode 2");
    assert_eq!(d1, d2);
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) {
    fs::create_dir_all(dst).expect("mkdir");
    for entry in fs::read_dir(src).expect("read_dir") {
        let entry = entry.expect("entry");
        let ty = entry.file_type().expect("file_type");
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy");
        }
    }
}

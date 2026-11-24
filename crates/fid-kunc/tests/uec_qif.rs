//! Integration tests for `UNC.UEC.QIF` (QIF 3.0 mapping of `UncertaintyStatement`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kunc::statement::{EvidenceClass, Method, SourceKind, Uec0Reason, UncertaintyStatement};
use fid_kunc::uec::qif_map::{
    from_qif, to_qif, within_one_ulp, NativeInterval, NativeUncFields, QifMapError, UncQifFragment, ALGORITHM_ID,
    EXTENSION_VERSION, MAP_ID, MAX_EXT_BYTES, QIF_IMPORT_SOURCE_ID, QIF_UNC_MAP,
};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Extracts `fx:UncertaintyStatement` XML and native uncertainty elements from a fixture `.qif`
/// document (test-only lightweight XML extractor; L2 `IO.QIF` owns full QIF XML parsing).
fn extract_fragment_from_qif(xml: &str) -> UncQifFragment {
    let ext_xml = if let Some(start) = xml.find("<fx:UncertaintyStatement") {
        let end_tag = "</fx:UncertaintyStatement>";
        let end = xml.find(end_tag).expect("closing fx:UncertaintyStatement") + end_tag.len();
        xml.as_bytes()[start..end].to_vec()
    } else {
        Vec::new()
    };

    let tag_val = |tag: &str| -> Option<String> {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let s = xml.find(&open)? + open.len();
        let e = xml[s..].find(&close)? + s;
        Some(xml[s..e].trim().to_owned())
    };
    let tag_f64 = |tag: &str| -> Option<f64> { tag_val(tag).map(|s| s.parse::<f64>().expect("valid f64")) };

    let interval = match (tag_f64("LowerBound"), tag_f64("UpperBound")) {
        (Some(low), Some(high)) => Some(NativeInterval { low, high }),
        _ => None,
    };

    UncQifFragment {
        native: NativeUncFields {
            u_std: tag_f64("CombinedUncertainty"),
            expanded_u: tag_f64("ExpandedUncertainty"),
            k: tag_f64("CoverageFactor"),
            interval,
            p: tag_f64("CoverageProbability"),
            unit: tag_val("Unit"),
        },
        ext_xml,
    }
}

/// AC-1 & REQ-UNC.UEC.QIF-001: every valid statement fixture across UEC-0..3 and all methods
/// round-trips through `to_qif` and `from_qif` with an identical canonical digest.
#[test]
fn ac1_all_valid_statement_fixtures_roundtrip_with_identical_digest() {
    let dir = repo_root().join("fixtures/unc/stmt/valid");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read fixtures/unc/stmt/valid")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 12, "expected at least 12 valid statement fixtures");

    for path in paths {
        let bytes = std::fs::read(&path).expect("read statement fixture");
        let orig =
            UncertaintyStatement::parse_canonical(&bytes).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        let frag = to_qif(&orig);
        let frag2 = to_qif(&orig);
        // D0 determinism: mapping the same statement twice gives byte-identical XML.
        assert_eq!(frag.ext_xml, frag2.ext_xml);
        assert_eq!(frag.native, frag2.native);

        let rt = from_qif(&frag).unwrap_or_else(|e| panic!("from_qif {}: {e}", path.display()));
        assert_eq!(rt.digest(), orig.digest(), "digest mismatch on {}", path.display());
        assert_eq!(rt.canonical_json(), orig.canonical_json());
    }
}

/// AC-2 & REQ-UNC.UEC.QIF-002: UEC-0 statements produce no native numeric uncertainty fields
/// and carry UEC-0 plus their reason in the extension element.
#[test]
fn ac2_uec0_has_no_native_numeric_fields_and_carries_reason() {
    let bytes =
        std::fs::read(repo_root().join("fixtures/unc/stmt/valid/uec0_model_missing.json")).expect("read uec0 fixture");
    let stmt = UncertaintyStatement::parse_canonical(&bytes).expect("parse uec0");
    let frag = to_qif(&stmt);

    assert!(!frag.native.has_numeric());
    assert_eq!(frag.native, NativeUncFields::default());
    let xml = std::str::from_utf8(&frag.ext_xml).expect("utf8 xml");
    assert!(xml.contains("<fx:Class>UEC-0</fx:Class>"), "xml: {xml}");
    assert!(xml.contains("<fx:Uec0Reason>model_missing</fx:Uec0Reason>"), "xml: {xml}");

    let rt = from_qif(&frag).expect("roundtrip uec0");
    assert_eq!(rt.class(), EvidenceClass::Uec0);
    assert_eq!(rt.uec0_reason(), Some(Uec0Reason::ModelMissing));
    assert_eq!(rt.digest(), stmt.digest());

    // A UEC-0 extension paired with a stray native numeric field must fail closed with conflict.
    let mut bad = frag;
    bad.native.expanded_u = Some(0.001);
    let err = from_qif(&bad).expect_err("UEC-0 with native U must fail");
    assert_eq!(err, QifMapError::Conflict { field: "U" });
}

/// AC-3: all four QIF Results documents in `fixtures/unc/qif/docs/*.qif` (one per class UEC-0..3)
/// parse and round-trip with canonical digests matching the source statements, and their extension
/// XML matches `to_qif` byte for byte.
#[test]
fn ac3_qif_docs_per_class_roundtrip_and_match_to_qif() {
    let pairs = [
        ("fixtures/unc/qif/docs/uec0.qif", "fixtures/unc/stmt/valid/uec0_model_missing.json"),
        ("fixtures/unc/qif/docs/uec1.qif", "fixtures/unc/stmt/valid/uec1_declared.json"),
        ("fixtures/unc/qif/docs/uec2.qif", "fixtures/unc/stmt/valid/uec2_lpu.json"),
        ("fixtures/unc/qif/docs/uec3.qif", "fixtures/unc/stmt/valid/uec3_iso15530_3.json"),
    ];
    for (qif_rel, json_rel) in pairs {
        let qif_xml = std::fs::read_to_string(repo_root().join(qif_rel)).expect("read qif doc");
        let stmt_bytes = std::fs::read(repo_root().join(json_rel)).expect("read stmt json");
        let expected = UncertaintyStatement::parse_canonical(&stmt_bytes).expect("parse stmt");

        let frag = extract_fragment_from_qif(&qif_xml);
        assert_eq!(frag.ext_xml, to_qif(&expected).ext_xml, "extension XML in {qif_rel} must match to_qif");
        let imported = from_qif(&frag).unwrap_or_else(|e| panic!("from_qif {qif_rel}: {e}"));
        assert_eq!(imported.digest(), expected.digest());
    }
}

/// AC-4 & REQ-UNC.UEC.QIF-004: native-only QIF fragment (no extension) imports as UEC-1,
/// method `DECLARED`, source kind `Declared` with id `qif_import`, never UEC-2 or UEC-3.
#[test]
fn ac4_native_only_imports_as_uec1_declared_qif_import() {
    let qif_xml =
        std::fs::read_to_string(repo_root().join("fixtures/unc/qif/native_only.qif")).expect("read native_only.qif");
    let frag = extract_fragment_from_qif(&qif_xml);
    assert!(!frag.has_extension());

    let stmt = from_qif(&frag).expect("native-only import must succeed");
    assert_eq!(stmt.class(), EvidenceClass::Uec1);
    assert_eq!(stmt.method(), Some(Method::Declared));
    assert_eq!(stmt.unit(), Some("si:m"));
    assert_eq!(stmt.u_std(), Some(0.00000125));
    assert_eq!(stmt.expanded().map(|e| (e.value, e.k)), Some((0.0000025, 2.0)));
    assert_eq!(stmt.sources().len(), 1);
    assert_eq!(stmt.sources()[0].kind, SourceKind::Declared);
    assert_eq!(stmt.sources()[0].id, QIF_IMPORT_SOURCE_ID);

    // Determinism: importing the same native-only fragment twice yields identical digest.
    let stmt2 = from_qif(&frag).expect("native-only import 2");
    assert_eq!(stmt.digest(), stmt2.digest());
}

/// AC-5 & REQ-UNC.UEC.QIF-005: native U differing from extension U by > 1 ulp fails with
/// `qif_unc_conflict{field: "U"}`, while a 1-ulp difference is accepted.
#[test]
fn ac5_native_conflict_and_one_ulp_boundary() {
    let qif_xml =
        std::fs::read_to_string(repo_root().join("fixtures/unc/qif/conflict.qif")).expect("read conflict.qif");
    let frag = extract_fragment_from_qif(&qif_xml);
    let err = from_qif(&frag).expect_err("conflict.qif must fail");
    assert_eq!(err, QifMapError::Conflict { field: "U" });
    assert_eq!(err.code(), "qif_unc_conflict");
    assert_eq!(err.to_string(), "qif_unc_conflict{field: \"U\"}");

    // Verify exact 1-ulp tolerance boundary (accepted at 1 ulp, rejected at 2 ulps).
    let stmt_bytes = std::fs::read(repo_root().join("fixtures/unc/stmt/valid/uec1_declared.json")).expect("read uec1");
    let orig = UncertaintyStatement::parse_canonical(&stmt_bytes).expect("parse uec1");
    let orig_u = orig.expanded().unwrap().value;

    let mut one_ulp_frag = to_qif(&orig);
    one_ulp_frag.native.expanded_u = Some(f64::from_bits(orig_u.to_bits() + 1));
    assert!(within_one_ulp(orig_u, one_ulp_frag.native.expanded_u.unwrap()));
    let ok = from_qif(&one_ulp_frag).expect("1 ulp difference must be accepted");
    assert_eq!(ok.digest(), orig.digest());

    let mut two_ulp_frag = to_qif(&orig);
    two_ulp_frag.native.expanded_u = Some(f64::from_bits(orig_u.to_bits() + 2));
    assert!(!within_one_ulp(orig_u, two_ulp_frag.native.expanded_u.unwrap()));
    let err2 = from_qif(&two_ulp_frag).expect_err("2 ulp difference must fail");
    assert_eq!(err2, QifMapError::Conflict { field: "U" });
}

/// AC-6 & REQ-UNC.UEC.QIF-006: unknown extension version `unc-qif-stmt@9` fails with
/// `unsupported_extension`, and inputs over 1 MiB or nesting depth > 16 fail with `limit_exceeded`.
#[test]
fn ac6_unsupported_version_and_bounds() {
    let qif_xml = std::fs::read_to_string(repo_root().join("fixtures/unc/qif/unsupported_version.qif"))
        .expect("read unsupported_version.qif");
    let frag = extract_fragment_from_qif(&qif_xml);
    let err = from_qif(&frag).expect_err("unc-qif-stmt@9 must fail");
    assert_eq!(err, QifMapError::UnsupportedExtension { version: "unc-qif-stmt@9".to_owned() });
    assert_eq!(err.code(), "unsupported_extension");

    // > 1 MiB fails closed with limit_exceeded.
    let big = UncQifFragment { native: NativeUncFields::default(), ext_xml: vec![b' '; MAX_EXT_BYTES + 1] };
    assert_eq!(from_qif(&big).expect_err("oversized"), QifMapError::LimitExceeded);

    // Depth > 16 fails closed with limit_exceeded.
    let mut deep = String::new();
    for i in 0..18 {
        deep.push_str(&format!("<a{i}>"));
    }
    for i in (0..18).rev() {
        deep.push_str(&format!("</a{i}>"));
    }
    let deep_frag = UncQifFragment { native: NativeUncFields::default(), ext_xml: deep.into_bytes() };
    assert_eq!(from_qif(&deep_frag).expect_err("over-deep"), QifMapError::LimitExceeded);

    // Metadata and mapping table sanity checks.
    assert_eq!(ALGORITHM_ID, "unc.uec.qif_map@1.0.0");
    assert_eq!(EXTENSION_VERSION, "unc-qif-stmt@1");
    assert_eq!(MAP_ID, "qif-unc-map@1");
    assert_eq!(QIF_UNC_MAP.len(), 20);
}

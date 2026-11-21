//! UNC.RDL.REP: table `rep-1@1.0.0` (REQ-001, REQ-009), its canonical JSON and digest.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::kdec::Decimal;
use fid_rep::{Bound, BoundMode, FieldKind, RepError, Table, TABLE_DIGEST, TABLE_JSON, TABLE_VERSION};

const DIGEST_FILE: &str = include_str!("../../../fixtures/rdl/rep/rep-1@1.0.0.sha256");

#[test]
fn rep1_bounds_match_req001() {
    let t = Table::rep1();
    assert_eq!(t.version(), "rep-1@1.0.0");
    assert_eq!(TABLE_VERSION, "rep-1@1.0.0");
    let want = [
        (FieldKind::Length, BoundMode::Abs, "1e-9", "m"),
        (FieldKind::Angle, BoundMode::Abs, "1e-9", "rad"),
        (FieldKind::Dimensionless, BoundMode::Rel, "1e-9", "1"),
        (FieldKind::ExpandedUncertainty, BoundMode::Rel, "1e-6", "1"),
        (FieldKind::PConformance, BoundMode::Abs, "1e-6", "1"),
    ];
    assert_eq!(t.bounds().len(), want.len());
    for (b, (kind, mode, value, unit)) in t.bounds().iter().zip(want) {
        assert_eq!((b.kind, b.mode, b.value, b.unit.as_str()), (kind, mode, Decimal::parse(value).unwrap(), unit));
    }
    assert!(t.bound(FieldKind::Exact).is_none());
}

#[test]
fn canonical_json_matches_embedded_file_and_digest() {
    assert_eq!(Table::rep1().canonical_json(), TABLE_JSON);
    assert_eq!(DIGEST_FILE.trim_end(), TABLE_DIGEST);
    assert_eq!(fid_testkit::sha256_hex(TABLE_JSON.as_bytes()), TABLE_DIGEST);
}

#[test]
fn table_rules() {
    let b =
        |kind, mode, v: &str, unit: &str| Bound { kind, mode, value: Decimal::parse(v).unwrap(), unit: unit.into() };
    let err = |r: Result<Table, RepError>| r.unwrap_err();
    assert_eq!(err(Table::new("", vec![])), RepError::TableInvalid("empty_version"));
    assert_eq!(
        err(Table::new("x@1", vec![b(FieldKind::Exact, BoundMode::Abs, "0", "1")])),
        RepError::TableInvalid("bound_for_exact")
    );
    assert_eq!(
        err(Table::new(
            "x@1",
            vec![b(FieldKind::Length, BoundMode::Abs, "1e-9", "m"), b(FieldKind::Length, BoundMode::Abs, "1e-8", "m")]
        )),
        RepError::TableInvalid("duplicate_kind")
    );
    assert_eq!(
        err(Table::new("x@1", vec![b(FieldKind::Length, BoundMode::Abs, "-1e-9", "m")])),
        RepError::TableInvalid("negative_bound")
    );
    assert_eq!(
        err(Table::new("x@1", vec![b(FieldKind::Dimensionless, BoundMode::Rel, "1e-9", "m")])),
        RepError::TableInvalid("bad_unit")
    );
    // Rows are put in kind order, so canonical JSON does not depend on input order.
    let t = Table::new(
        "x@1",
        vec![b(FieldKind::Angle, BoundMode::Abs, "2e-9", "rad"), b(FieldKind::Length, BoundMode::Abs, "3e-9", "m")],
    )
    .unwrap();
    assert_eq!(
        t.canonical_json(),
        "{\"algorithm\":\"rdl.rep.classify@1.0.0\",\"bounds\":[{\"bound\":\"3e-9\",\"kind\":\"length\",\"mode\":\"abs\",\
         \"unit\":\"m\"},{\"bound\":\"2e-9\",\"kind\":\"angle\",\"mode\":\"abs\",\"unit\":\"rad\"},{\"kind\":\"exact\",\
         \"mode\":\"exact\"}],\"schema\":\"rep-table@1\",\"version\":\"x@1\"}"
    );
    assert_eq!(FieldKind::parse("roughness").unwrap_err().code(), "kind_unknown");
    for k in FieldKind::ALL {
        assert_eq!(FieldKind::parse(k.as_str()), Ok(k));
    }
}

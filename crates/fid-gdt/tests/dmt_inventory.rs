//! Integration tests covering AC-1..AC-4 and REQ-001..REQ-005 of spec node
//! `GDT.RES.ROWASME.INV`, including the CI check over the committed P1a ASME
//! inventory `dmt/inventory/asme-y14.5-2018-p1a.json`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_gdt::dmt_inventory::{
    coverage, CoverageFinding, CoverageFindingKind, Inventory, InventoryEntry, InventoryError, InventoryFamily,
    RequiredStatus,
};
use fid_gps::dmt::{Dialect, DmtKey, DmtTable, RowStatus};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/gdt/dmt/");
const COMMITTED_P1A_INV: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../dmt/inventory/asme-y14.5-2018-p1a.json");

fn load_table(name: &str) -> DmtTable {
    let bytes = fs::read(format!("{FIXTURE_DIR}{name}")).unwrap();
    DmtTable::load(&bytes).unwrap()
}

fn load_expected() -> BTreeMap<String, CValue> {
    let bytes = fs::read(format!("{FIXTURE_DIR}inventory_expected.json")).unwrap();
    match parse(&bytes).unwrap() {
        CValue::Obj(m) => m,
        other => panic!("expected object, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// AC-1: `inventory_synthetic.json` (6 keys) vs `synthetic_table_v1.json` (4 of
//       them, one `unsupported`) -> two `missing_row` + one `status_below_required`
//       in deterministic key order

#[test]
fn ac1_synthetic_coverage_reports_two_missing_and_one_below_required_in_key_order() {
    let table = load_table("synthetic_table_v1.json");
    let inv_bytes = fs::read(format!("{FIXTURE_DIR}inventory_synthetic.json")).unwrap();
    let inv = Inventory::load(&inv_bytes).unwrap();
    assert_eq!(inv.len(), 6);
    assert_eq!(inv.dialect(), Dialect::Asme);
    assert_eq!(inv.phase(), "P1a");

    let expected = load_expected();
    assert_eq!(
        Some(&CValue::Str(inv.digest().as_str().to_owned())),
        expected.get("synthetic_digest"),
        "Rust canonical digest must match independent Python fixture digest"
    );

    let findings = coverage(&table, &inv);
    assert_eq!(findings.len(), 3);
    assert_eq!(findings[0].kind(), CoverageFindingKind::MissingRow);
    assert_eq!(findings[0].key().as_str(), "dmt:asme/caliper/none@1");
    assert_eq!(findings[1].kind(), CoverageFindingKind::StatusBelowRequired);
    assert_eq!(findings[1].key().as_str(), "dmt:asme/concentricity/none@1");
    assert!(matches!(
        findings[1],
        CoverageFinding::StatusBelowRequired {
            status: RowStatus::Unsupported,
            required: RequiredStatus::SupportedOrPartial,
            ..
        }
    ));
    assert_eq!(findings[2].kind(), CoverageFindingKind::MissingRow);
    assert_eq!(findings[2].key().as_str(), "dmt:asme/drf_planar/none@1");

    // Findings are sorted strictly by DmtKey and match the independent Python reference.
    for w in findings.windows(2) {
        assert!(w[0].key() < w[1].key(), "findings not sorted by key");
    }
    let got_val = CValue::Arr(findings.iter().map(CoverageFinding::to_cvalue).collect());
    assert_eq!(
        to_canonical_bytes(&got_val).unwrap(),
        to_canonical_bytes(expected.get("ac1_findings").unwrap()).unwrap()
    );

    // Determinism (REQ-003): shuffled table produces byte-identical findings.
    let shuffled = load_table("synthetic_table_v1_shuffled.json");
    assert_eq!(coverage(&shuffled, &inv), findings);

    // `unsupported_ok` allows an `unsupported` row without reporting `status_below_required`.
    let mut entries = BTreeMap::new();
    entries.insert(
        DmtKey::parse("dmt:asme/concentricity/none@1").unwrap(),
        InventoryEntry { family: InventoryFamily::Pos, required_status: RequiredStatus::UnsupportedOk },
    );
    let inv_ok = Inventory::new(Dialect::Asme, vec!["y14.5-2018".into()], "P1a".into(), entries).unwrap();
    assert!(coverage(&table, &inv_ok).is_empty());
}

// ---------------------------------------------------------------------------
// AC-2: Table covering every inventory key as `supported` -> zero findings

#[test]
fn ac2_all_supported_has_zero_findings() {
    let table = load_table("synthetic_table_v1.json");
    let inv_bytes = fs::read(format!("{FIXTURE_DIR}inventory_all_supported.json")).unwrap();
    let inv = Inventory::load(&inv_bytes).unwrap();
    let expected = load_expected();
    assert_eq!(Some(&CValue::Str(inv.digest().as_str().to_owned())), expected.get("all_supported_digest"));
    assert!(coverage(&table, &inv).is_empty());
}

// ---------------------------------------------------------------------------
// AC-3: Error paths (duplicate key, unknown family `PROFILE`, `bad_dmt_key`,
//       unknown field, dialect mismatch)

#[test]
fn ac3_error_paths() {
    // 1. Duplicate key -> `inventory_invalid`.
    let dup_json = br#"{
      "schema": "dmt-inventory/v1",
      "dialect": "asme",
      "editions": ["y14.5-2018"],
      "phase": "P1a",
      "entries": [
        { "key": "dmt:asme/flatness/none@1", "family": "FORM", "required_status": "supported_or_partial" },
        { "key": "dmt:asme/flatness/none@1", "family": "FORM", "required_status": "supported_or_partial" }
      ]
    }"#;
    let err = Inventory::load(dup_json).unwrap_err();
    assert_eq!(err.code(), "inventory_invalid");
    assert_eq!(err.field(), Some("entries.key"));

    // 2. Unknown family `PROFILE` -> `inventory_invalid`.
    let bad_fam = br#"{
      "schema": "dmt-inventory/v1",
      "dialect": "asme",
      "editions": ["y14.5-2018"],
      "phase": "P1a",
      "entries": [
        { "key": "dmt:asme/flatness/none@1", "family": "PROFILE", "required_status": "supported_or_partial" }
      ]
    }"#;
    let err = Inventory::load(bad_fam).unwrap_err();
    assert_eq!(err.code(), "inventory_invalid");
    assert_eq!(err.field(), Some("entries.family"));

    // 3. Invalid DMT key -> `bad_dmt_key`.
    let bad_key = br#"{
      "schema": "dmt-inventory/v1",
      "dialect": "asme",
      "editions": ["y14.5-2018"],
      "phase": "P1a",
      "entries": [
        { "key": "dmt:asme/FLATNESS/none@1", "family": "FORM", "required_status": "supported_or_partial" }
      ]
    }"#;
    let err = Inventory::load(bad_key).unwrap_err();
    assert_eq!(err.code(), "bad_dmt_key");
    assert!(matches!(err, InventoryError::BadDmtKey { .. }));

    // 4. Unknown field -> `inventory_invalid`.
    let unknown_field = br#"{
      "schema": "dmt-inventory/v1",
      "dialect": "asme",
      "editions": ["y14.5-2018"],
      "phase": "P1a",
      "extra": "nope",
      "entries": []
    }"#;
    let err = Inventory::load(unknown_field).unwrap_err();
    assert_eq!(err.code(), "inventory_invalid");

    // 5. Key dialect mismatch -> `inventory_invalid`.
    let wrong_dialect = br#"{
      "schema": "dmt-inventory/v1",
      "dialect": "asme",
      "editions": ["y14.5-2018"],
      "phase": "P1a",
      "entries": [
        { "key": "dmt:iso/flatness/none@1", "family": "FORM", "required_status": "supported_or_partial" }
      ]
    }"#;
    let err = Inventory::load(wrong_dialect).unwrap_err();
    assert_eq!(err.code(), "inventory_invalid");
    assert_eq!(err.field(), Some("entries.key"));
}

// ---------------------------------------------------------------------------
// AC-4 + CI check: Committed P1a ASME inventory loads, every ASME family child
//                  (SIZE, FORM, ORIENT, POS, DRF) owns >= 1 key, canonical roundtrip,
//                  and deterministic coverage run over synthetic DMT

#[test]
fn ac4_committed_p1a_inventory_loads_and_covers_all_asme_families() {
    let bytes = fs::read(COMMITTED_P1A_INV).unwrap();
    let inv = Inventory::load(&bytes).unwrap();
    assert_eq!(inv.dialect(), Dialect::Asme);
    assert_eq!(inv.phase(), "P1a");
    assert!(inv.editions().contains(&"y14.5-2018".to_owned()));

    // Canonical roundtrip preserves digest.
    let reparsed = Inventory::load(inv.canonical_json()).unwrap();
    assert_eq!(reparsed.digest(), inv.digest());
    assert_eq!(reparsed, inv);

    // Every family child of GDT.RES.ROWASME (SIZE, FORM, ORIENT, POS, DRF) owns >= 1 key,
    // and every key belongs exclusively to one of those five families (REQ-004).
    let mut owned: BTreeSet<InventoryFamily> = BTreeSet::new();
    for entry in inv.entries().values() {
        assert!(
            InventoryFamily::ASME_P1A.contains(&entry.family),
            "unexpected non-ASME-P1a family in P1a inventory: {}",
            entry.family
        );
        owned.insert(entry.family);
    }
    for fam in InventoryFamily::ASME_P1A {
        assert!(owned.contains(&fam), "ASME P1a family {fam} owns zero keys");
    }

    // CI coverage pass against the synthetic DMT runs deterministically without panicking.
    let table = load_table("synthetic_table_v1.json");
    let findings = coverage(&table, &inv);
    assert_eq!(findings, coverage(&table, &inv));
}

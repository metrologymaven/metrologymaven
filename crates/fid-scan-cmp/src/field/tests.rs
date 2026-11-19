//! Acceptance criteria AC-1, AC-2 and AC-4 of spec node SCAN.CMP.DEV.FIELD plus requirement
//! checks (AC-3 is the compile-fail doctests in the crate root). `FieldBuilder` is
//! crate-private, so these are unit tests. Expected canonical JSON, digests, layout lengths,
//! statistics and number strings come from `fixtures/scan/cmp/field_golden.json` and
//! `field_numbers.txt`, written by the independent Python reference
//! `fixtures/scan/cmp/make_field_golden.py` (stdlib `hashlib`, `json`, `decimal`,
//! `fractions`); none come from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::dev::NoMatchReason;
use crate::number::es_number;
use std::collections::BTreeMap;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/scan/cmp/");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap()
}

fn f64_hex(s: &str) -> f64 {
    assert_eq!(s.len(), 16, "f64 hex {s:?}");
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

type Case = BTreeMap<String, String>;

/// The cases of the golden file, read line by line (the generator writes sorted keys, one
/// per line, `indent=1`).
fn cases() -> Vec<Case> {
    let text = read("field_golden.json");
    let mut out = Vec::new();
    let mut cur = Case::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('}') {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        let Some((k, v)) = t.split_once("\": ") else { continue };
        let Some(k) = k.strip_prefix('"') else { continue };
        if k == "about" || k == "cases" {
            continue;
        }
        let v = v.strip_suffix(',').unwrap_or(v);
        let v = match v.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            Some(inner) => inner.replace("\\\"", "\""),
            None => v.to_string(),
        };
        cur.insert(k.to_string(), v);
    }
    assert_eq!(out.len(), 5, "case count");
    out
}

fn case(name: &str) -> Case {
    cases().into_iter().find(|c| c["name"] == name).unwrap()
}

fn opt_digest(s: &str) -> Option<Digest> {
    if s.is_empty() {
        None
    } else {
        Some(Digest::parse(s).unwrap())
    }
}

fn provenance(c: &Case) -> FieldProvenance {
    FieldProvenance {
        method: Method::ALL.into_iter().find(|m| m.as_str() == c["method"]).unwrap(),
        max_distance_m: f64_hex(&c["max_distance_m_hex"]),
        max_angle_rad: f64_hex(&c["max_angle_rad_hex"]),
        alignment_id: c["alignment_id"].clone(),
        alignment_digest: Digest::parse(&c["alignment_digest"]).unwrap(),
        actual_digest: Digest::parse(&c["actual_digest"]).unwrap(),
        nominal_digest: Digest::parse(&c["nominal_digest"]).unwrap(),
        nominal_kind: ElementKind::ALL.into_iter().find(|k| k.as_str() == c["nominal_kind"]).unwrap(),
        actual_selection: opt_digest(&c["actual_selection"]),
        nominal_selection: opt_digest(&c["nominal_selection"]),
        algorithm_id: AlgorithmId::parse(&c["algorithm_id"]).unwrap(),
        kernel_version: c["kernel_version"].clone(),
    }
}

fn results(c: &Case) -> Vec<DevResult> {
    c["points"]
        .split_whitespace()
        .map(|t| match t {
            "N" => DevResult::NoMatch(NoMatchReason::BeyondDistance),
            "X" => DevResult::Excluded,
            v => DevResult::Valid(f64_hex(v.strip_prefix("V:").unwrap())),
        })
        .collect()
}

fn build(c: &Case, order: &[usize]) -> DeviationField {
    let rs = results(c);
    let mut b = FieldBuilder::new(rs.len(), provenance(c)).unwrap();
    for &i in order {
        b.set(i, rs[i]).unwrap();
    }
    b.finish().unwrap()
}

fn layout_bytes(f: &DeviationField) -> Vec<u8> {
    let mut out = Vec::new();
    f.write_layout(|b| out.extend_from_slice(b));
    out
}

fn sample_provenance() -> FieldProvenance {
    provenance(&case("ac4_golden"))
}

// ------------------------------------------------------------------ AC-1

/// AC-1: fill orders 4, 2, 0, 1, 3 and 0..4 give the same digest, and it is the golden one.
#[test]
fn ac1_digest_independent_of_fill_order() {
    let c = case("ac1_five_points");
    let a = build(&c, &[4, 2, 0, 1, 3]);
    let b = build(&c, &[0, 1, 2, 3, 4]);
    assert_eq!(a.digest(), b.digest());
    assert_eq!(a, b);
    assert_eq!(a.digest().hex(), c["sha256"]);
}

/// REQ-SCAN.CMP.DEV.FIELD-005 beyond one hash block: 3 000 points (three streaming blocks)
/// filled forwards and backwards hash to the one-shot SHA-256 of the whole layout.
#[test]
fn digest_streams_across_blocks_and_ignores_order() {
    let n = 3000;
    let rs: Vec<DevResult> = (0..n)
        .map(|i| match i % 7 {
            0 => DevResult::NoMatch(NoMatchReason::Angle),
            3 => DevResult::Excluded,
            _ => DevResult::Valid((i as f64 - 1500.0) * 1e-6),
        })
        .collect();
    let mut fwd = FieldBuilder::new(n, sample_provenance()).unwrap();
    let mut bwd = FieldBuilder::new(n, sample_provenance()).unwrap();
    for (i, r) in rs.iter().enumerate() {
        fwd.set(i, *r).unwrap();
    }
    for (i, r) in rs.iter().enumerate().rev() {
        bwd.set(i, *r).unwrap();
    }
    let (fwd, bwd) = (fwd.finish().unwrap(), bwd.finish().unwrap());
    assert_eq!(fwd.digest(), bwd.digest());
    let bytes = layout_bytes(&fwd);
    assert_eq!(bytes.len(), 6 + 1 + 8 + fwd.provenance_json().len() + 8 + 9 * n);
    assert_eq!(fwd.digest(), fid_kci::kci::sha256_digest(&bytes));
}

// ------------------------------------------------------------------ AC-2, REQ-003

/// AC-2: index 3 unset fails `scan.cmp.field_incomplete { index: 3 }`.
#[test]
fn ac2_unset_index_fails() {
    let mut b = FieldBuilder::new(5, sample_provenance()).unwrap();
    for i in [0, 1, 2, 4] {
        b.set(i, DevResult::Valid(0.001)).unwrap();
    }
    let e = b.finish().unwrap_err();
    assert_eq!(e, FieldError::Incomplete { index: 3, kind: IncompleteKind::Unset });
    assert_eq!(e.code(), "scan.cmp.field_incomplete");
    assert_eq!(e.to_string(), "scan.cmp.field_incomplete: index 3 unset");
}

/// An index set twice fails too, and the lowest offending index is named whichever the
/// kind.
#[test]
fn set_twice_and_lowest_offending_index() {
    let mut b = FieldBuilder::new(4, sample_provenance()).unwrap();
    for i in 0..4 {
        b.set(i, DevResult::Excluded).unwrap();
    }
    b.set(2, DevResult::Valid(1e-3)).unwrap();
    assert_eq!(b.finish().unwrap_err(), FieldError::Incomplete { index: 2, kind: IncompleteKind::SetTwice });

    let mut b = FieldBuilder::new(4, sample_provenance()).unwrap();
    b.set(0, DevResult::Excluded).unwrap();
    b.set(2, DevResult::Excluded).unwrap();
    b.set(2, DevResult::Excluded).unwrap();
    b.set(2, DevResult::Excluded).unwrap();
    b.set(3, DevResult::Excluded).unwrap();
    assert_eq!(b.finish().unwrap_err(), FieldError::Incomplete { index: 1, kind: IncompleteKind::Unset });
}

/// An index out of range is refused and records nothing.
#[test]
fn out_of_range_index_is_refused() {
    let mut b = FieldBuilder::new(2, sample_provenance()).unwrap();
    let e = b.set(2, DevResult::Valid(0.0)).unwrap_err();
    assert_eq!(e, FieldError::IndexOutOfRange { index: 2, len: 2 });
    assert_eq!(e.code(), "scan.cmp.field_index_out_of_range");
    b.set(0, DevResult::Excluded).unwrap();
    b.set(1, DevResult::Excluded).unwrap();
    assert_eq!(b.finish().unwrap().len(), 2);
}

/// NaN and infinities in a `VALID` value fail `scan.cmp.non_finite` with the index and
/// leave the index unset.
#[test]
fn non_finite_valid_value_is_refused() {
    for bad in [f64::NAN, -f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::from_bits(0x7ff0_0000_0000_0001)] {
        let mut b = FieldBuilder::new(3, sample_provenance()).unwrap();
        let e = b.set(1, DevResult::Valid(bad)).unwrap_err();
        assert_eq!(e, FieldError::NonFinite { index: Some(1) });
        assert_eq!(e.code(), "scan.cmp.non_finite");
        b.set(0, DevResult::Excluded).unwrap();
        b.set(2, DevResult::Excluded).unwrap();
        assert_eq!(b.finish().unwrap_err(), FieldError::Incomplete { index: 1, kind: IncompleteKind::Unset });
    }
}

// ------------------------------------------------------------------ AC-4, REQ-001, REQ-005

/// AC-4 and every other golden case: canonical provenance JSON byte for byte, its SHA-256,
/// the layout length and the field digest.
#[test]
fn ac4_golden_cases() {
    for c in cases() {
        let name = &c["name"];
        let n = results(&c).len();
        let f = build(&c, &(0..n).collect::<Vec<_>>());
        let prov = String::from_utf8(f.provenance_json().to_vec()).unwrap();
        assert_eq!(prov, c["provenance_canonical"], "{name}: provenance JSON");
        assert_eq!(fid_kci::kci::sha256_digest(f.provenance_json()).hex(), c["provenance_sha256"], "{name}");
        assert_eq!(f.provenance().canonical_json().unwrap(), f.provenance_json(), "{name}");
        assert_eq!(layout_bytes(&f).len().to_string(), c["layout_len"], "{name}: layout length");
        assert_eq!(f.digest().hex(), c["sha256"], "{name}: digest");
        assert_eq!(f.digest().as_str(), format!("sha256:{}", c["sha256"]));
        assert_eq!(f.len(), n);
        assert_eq!(f.is_empty(), n == 0);
    }
}

/// REQ-001: the provenance holds every recorded parameter and there is one status per point.
#[test]
fn provenance_and_one_status_per_point() {
    let c = case("selections_mesh");
    let f = build(&c, &[3, 1, 0, 2]);
    let p = f.provenance();
    assert_eq!(p.method, Method::Shortest);
    assert_eq!(p.max_distance_m.to_bits(), 1e21_f64.to_bits());
    assert_eq!(p.max_angle_rad.to_bits(), 0);
    assert_eq!(p.alignment_id, "aln-rps-0001");
    assert_eq!(p.nominal_kind, ElementKind::Mesh);
    assert!(p.actual_selection.is_some() && p.nominal_selection.is_some());
    assert_eq!(f.statuses(), &[DevStatus::Excluded, DevStatus::Valid, DevStatus::Valid, DevStatus::NoMatch]);
    assert_eq!(f.values().len(), f.len());
    assert_eq!(f.status(4), None);
    assert_eq!(f.value(4), None);
}

// ------------------------------------------------------------------ REQ-004 and statistics

fn opt_hex(s: &str) -> Option<f64> {
    if s.is_empty() {
        None
    } else {
        Some(f64_hex(s))
    }
}

fn bits(x: Option<f64>) -> Option<u64> {
    x.map(f64::to_bits)
}

/// REQ-004: non-`VALID` points store +0.0, have no value and are left out of the
/// statistics; the statistics match the oracle bit for bit.
#[test]
fn non_valid_points_store_zero_and_are_excluded_from_stats() {
    for c in cases() {
        let name = &c["name"];
        let n = results(&c).len();
        let f = build(&c, &(0..n).rev().collect::<Vec<_>>());
        for i in 0..n {
            if f.status(i) != Some(DevStatus::Valid) {
                assert_eq!(f.values()[i].to_bits(), 0, "{name}: stored value of point {i}");
                assert_eq!(f.value(i), None);
            } else {
                assert_eq!(f.value(i), Some(f.values()[i]));
            }
        }
        let valid: Vec<usize> = f.valid_indexes().collect();
        assert_eq!(valid.len().to_string(), c["stat_valid"], "{name}");
        let st = f.stats().unwrap();
        assert_eq!(st.valid.to_string(), c["stat_valid"], "{name}: valid");
        assert_eq!(st.no_match.to_string(), c["stat_no_match"], "{name}: no_match");
        assert_eq!(st.excluded.to_string(), c["stat_excluded"], "{name}: excluded");
        assert_eq!(bits(st.min), bits(opt_hex(&c["stat_min_hex"])), "{name}: min");
        assert_eq!(bits(st.max), bits(opt_hex(&c["stat_max_hex"])), "{name}: max");
        assert_eq!(bits(st.mean), bits(opt_hex(&c["stat_mean_hex"])), "{name}: mean");
    }
}

/// A `VALID` -0.0 is stored as +0.0 (so the digest does not depend on the zero's sign).
#[test]
fn negative_zero_is_stored_as_positive_zero() {
    let mut a = FieldBuilder::new(1, sample_provenance()).unwrap();
    a.set(0, DevResult::Valid(-0.0)).unwrap();
    let mut b = FieldBuilder::new(1, sample_provenance()).unwrap();
    b.set(0, DevResult::Valid(0.0)).unwrap();
    let (a, b) = (a.finish().unwrap(), b.finish().unwrap());
    assert_eq!(a.value(0).map(f64::to_bits), Some(0));
    assert_eq!(a.digest(), b.digest());
}

/// A statistic whose sum overflows fails `scan.cmp.non_finite` without an index.
#[test]
fn overflowing_mean_is_non_finite() {
    let mut b = FieldBuilder::new(2, sample_provenance()).unwrap();
    b.set(0, DevResult::Valid(f64::MAX)).unwrap();
    b.set(1, DevResult::Valid(f64::MAX)).unwrap();
    let f = b.finish().unwrap();
    assert_eq!(f.stats().unwrap_err(), FieldError::NonFinite { index: None });
}

// ------------------------------------------------------------------ provenance domain

/// A parameter name and the edit that puts it out of its domain.
type Edit = (&'static str, fn(&mut FieldProvenance));

#[test]
fn invalid_provenance_is_refused() {
    let base = sample_provenance();
    let cases: [Edit; 11] = [
        ("max_distance_m", |p| p.max_distance_m = 0.0),
        ("max_distance_m", |p| p.max_distance_m = -0.0),
        ("max_distance_m", |p| p.max_distance_m = -1e-3),
        ("max_distance_m", |p| p.max_distance_m = f64::NAN),
        ("max_distance_m", |p| p.max_distance_m = f64::INFINITY),
        ("max_angle_rad", |p| p.max_angle_rad = -1e-300),
        ("max_angle_rad", |p| p.max_angle_rad = f64::NAN),
        ("max_angle_rad", |p| p.max_angle_rad = f64::from_bits(core::f64::consts::PI.to_bits() + 1)),
        ("max_angle_rad", |p| p.max_angle_rad = f64::INFINITY),
        ("alignment_id", |p| p.alignment_id.clear()),
        ("kernel_version", |p| p.kernel_version.clear()),
    ];
    for (name, edit) in cases {
        let mut p = base.clone();
        edit(&mut p);
        let e = FieldBuilder::new(0, p.clone()).unwrap_err();
        assert_eq!(e, FieldError::InvalidParameter { name }, "{name}");
        assert_eq!(e.code(), "scan.cmp.invalid_parameter");
        assert_eq!(p.canonical_json().unwrap_err(), e);
    }
    // The domain edges are accepted: angle -0.0 (written "0"), angle PI, tiny distance.
    let mut p = base;
    p.max_angle_rad = -0.0;
    p.max_distance_m = f64::from_bits(1);
    assert!(String::from_utf8(p.canonical_json().unwrap()).unwrap().contains("\"max_angle_rad\":\"0\""));
    p.max_angle_rad = core::f64::consts::PI;
    assert!(FieldBuilder::new(0, p).is_ok());
}

// ------------------------------------------------------------------ empty and allocation

/// An empty actual gives an empty field with the golden digest and empty statistics.
#[test]
fn empty_field() {
    let c = case("empty");
    let f = FieldBuilder::new(0, provenance(&c)).unwrap().finish().unwrap();
    assert!(f.is_empty());
    assert_eq!(f.valid_indexes().count(), 0);
    assert_eq!(f.digest().hex(), c["sha256"]);
    let st = f.stats().unwrap();
    assert_eq!((st.valid, st.no_match, st.excluded, st.min, st.max, st.mean), (0, 0, 0, None, None, None));
}

/// A point count whose columns cannot be allocated fails instead of aborting.
#[test]
fn huge_point_count_fails_allocation() {
    let e = FieldBuilder::new(usize::MAX, sample_provenance()).unwrap_err();
    let needed = (usize::MAX as u64).saturating_mul(10);
    assert_eq!(e, FieldError::AllocationFailed { needed });
    assert_eq!(e.code(), "scan.cmp.allocation_failed");
}

// ------------------------------------------------------------------ wire names, numbers

#[test]
fn status_bytes_and_names() {
    let got: Vec<(u8, &str)> = DevStatus::ALL.iter().map(|s| (s.byte(), s.as_str())).collect();
    assert_eq!(got, [(1, "VALID"), (2, "NO_MATCH"), (3, "EXCLUDED")]);
    assert_eq!(core::mem::size_of::<DevStatus>(), 1);
    assert_eq!(Method::ALL.map(Method::as_str), ["normal", "shortest"]);
    assert_eq!(ELEMENT_KIND, "deviation_field");
    assert_eq!(LAYOUT_TAG, *b"FIDDEV");
    assert_eq!(LAYOUT_VERSION, 1);
}

/// The number strings match the oracle's ECMAScript `Number::toString`.
#[test]
fn number_strings_match_oracle() {
    let text = read("field_numbers.txt");
    let mut n = 0;
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let (hex, want) = line.split_once(' ').unwrap();
        let v = f64_hex(hex);
        assert_eq!(es_number(v).as_deref(), Some(want), "{hex}");
        assert_eq!(es_number(-v).as_deref().map(|s| s.trim_start_matches('-')), Some(want), "-{hex}");
        n += 1;
    }
    assert_eq!(n, 26);
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(es_number(bad), None);
    }
}

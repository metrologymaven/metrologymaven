//! RPT.STATS.SPC.CONST tests. The table data and its pinned digest come from the independent
//! Python reference `fixtures/rpt/stats/make_spc_constants.py` (normal-distribution integrals,
//! stdlib only), never from running this crate. These tests check the embedded table against
//! the formulas of REQ-RPT.STATS.SPC.CONST-005/-006 with their own arithmetic (AC-1), the
//! hand-labelled n = 5 row of the leaf (AC-2), the error path (AC-3) and the pinned digest (AC-4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{parse_json, Json};
use fid_num::f::sqrt;
use fid_stats::{
    constants, spc_table, ReviewStatus, SpcRow, StatsError, StoredValue, SPC_MAX_N, SPC_MIN_N, SPC_TABLE_ID,
    SPC_TABLE_JSON, SPC_TABLE_VERSION,
};

/// Slack for the f64 evaluation of the formulas. The reference keeps every computed value at
/// least 1e-9 away from a rounding tie of its stored digits, so this never decides a case.
const EVAL_SLACK: f64 = 1e-12;

fn half_unit(v: &StoredValue) -> f64 {
    match v.decimals() {
        3 => 0.0005,
        4 => 0.00005,
        d => panic!("{} has {d} decimals", v.text()),
    }
}

fn agrees(name: &str, n: u8, stored: &StoredValue, formula: f64) {
    let diff = (stored.value() - formula).abs();
    assert!(
        diff <= half_unit(stored) + EVAL_SLACK,
        "{name} at n = {n}: stored {} but the formula gives {formula} (difference {diff})",
        stored.text()
    );
}

fn at_least_zero(x: f64) -> f64 {
    if x < 0.0 {
        0.0
    } else {
        x
    }
}

/// Gamma(n/2) / Gamma((n-1)/2) by the exact half-integer recursion:
/// r(2) = 1/sqrt(pi), r(n+1) = ((n-1)/2) / r(n).
fn gamma_ratio(n: u8) -> f64 {
    let mut r = 1.0 / sqrt(core::f64::consts::PI).unwrap();
    let mut k = 2u8;
    while k < n {
        r = ((f64::from(k) - 1.0) / 2.0) / r;
        k += 1;
    }
    r
}

fn stored_values(r: &SpcRow) -> Vec<(&'static str, StoredValue)> {
    let mut v = vec![
        ("d2", r.d2),
        ("d3", r.d3),
        ("c4", r.c4),
        ("a2", r.a2),
        ("a3", r.a3),
        ("b3", r.b3),
        ("b4", r.b4),
        ("d3_factor", r.d3_factor),
        ("d4", r.d4),
    ];
    if let Some(e2) = r.e2 {
        v.push(("e2", e2));
    }
    v
}

#[test]
fn table_covers_2_to_25_with_every_column() {
    let rows = spc_table();
    assert_eq!(rows.len(), 24);
    assert_eq!((SPC_MIN_N, SPC_MAX_N), (2, 25));
    for (i, r) in rows.iter().enumerate() {
        assert_eq!(usize::from(r.n), i + 2, "rows are in n order");
        // E2 is the individuals-chart factor from moving ranges of two: n = 2 only.
        assert_eq!(r.e2.is_some(), r.n == 2, "e2 at n = {}", r.n);
        for (name, v) in stored_values(r) {
            let parsed: f64 = v.text().parse().unwrap();
            assert_eq!(parsed.to_bits(), v.value().to_bits(), "{name} at n = {}: text and value differ", r.n);
            assert!(v.value().is_finite() && v.value() >= 0.0, "{name} at n = {}", r.n);
            let want = if name == "c4" { 4 } else { 3 };
            assert_eq!(v.decimals(), want, "{name} at n = {} stored digits", r.n);
        }
    }
}

/// AC-1: every stored chart factor and c4 agrees with REQ-005 and REQ-006 within half a unit in
/// its last stored digit.
#[test]
fn ac1_factors_and_c4_agree_with_the_formulas() {
    for r in spc_table() {
        let n = r.n;
        let (d2, d3, c4) = (r.d2.value(), r.d3.value(), r.c4.value());
        let sn = sqrt(f64::from(n)).unwrap();
        let k = sqrt(1.0 - c4 * c4).unwrap() / c4;
        agrees("A2", n, &r.a2, 3.0 / (d2 * sn));
        agrees("A3", n, &r.a3, 3.0 / (c4 * sn));
        agrees("B3", n, &r.b3, at_least_zero(1.0 - 3.0 * k));
        agrees("B4", n, &r.b4, 1.0 + 3.0 * k);
        agrees("D3", n, &r.d3_factor, at_least_zero(1.0 - 3.0 * d3 / d2));
        agrees("D4", n, &r.d4, 1.0 + 3.0 * d3 / d2);
        if let Some(e2) = &r.e2 {
            agrees("E2", n, e2, 3.0 / d2);
        }
        let c4_formula = sqrt(2.0 / (f64::from(n) - 1.0)).unwrap() * gamma_ratio(n);
        agrees("c4", n, &r.c4, c4_formula);
    }
}

/// The gamma recursion of the test is itself right: the n = 2 and n = 3 closed forms.
#[test]
fn gamma_ratio_closed_forms() {
    let pi = core::f64::consts::PI;
    assert!((gamma_ratio(2) - 1.0 / sqrt(pi).unwrap()).abs() < 1e-15);
    // Gamma(3/2) / Gamma(1) = sqrt(pi) / 2.
    assert!((gamma_ratio(3) - sqrt(pi).unwrap() / 2.0).abs() < 1e-15);
    // c4(2) = sqrt(2/pi).
    let c4_2 = sqrt(2.0).unwrap() * gamma_ratio(2);
    assert!((c4_2 - sqrt(2.0 / pi).unwrap()).abs() < 1e-15);
}

/// AC-2: the n = 5 row, hand-labelled from the leaf's seed table.
#[test]
fn ac2_n5_row() {
    let c = constants(5).unwrap();
    assert_eq!(c.row.n, 5);
    assert_eq!(c.row.d2.text(), "2.326");
    assert_eq!(c.row.d3.text(), "0.864");
    assert_eq!(c.row.c4.text(), "0.9400");
    assert_eq!(c.row.d2.value(), 2.326);
    assert_eq!(c.row.d3.value(), 0.864);
    assert_eq!(c.row.c4.value(), 0.94);
    assert_eq!(c.table_id, "spc.constants.normal");
    assert_eq!(c.table_version, "1.0.0");
    assert_eq!(c.review, ReviewStatus::Unverified);
    assert_eq!(c.review.as_str(), "UNVERIFIED");
    assert_eq!(c.row.review, ReviewStatus::Unverified);
}

/// The leaf's whole seed table (n = 2..10), hand-transcribed.
#[test]
fn seed_rows_match_the_leaf() {
    let seeds = [
        (2, "1.128", "0.853", "0.7979"),
        (3, "1.693", "0.888", "0.8862"),
        (4, "2.059", "0.880", "0.9213"),
        (5, "2.326", "0.864", "0.9400"),
        (6, "2.534", "0.848", "0.9515"),
        (7, "2.704", "0.833", "0.9594"),
        (8, "2.847", "0.820", "0.9650"),
        (9, "2.970", "0.808", "0.9693"),
        (10, "3.078", "0.797", "0.9727"),
    ];
    for (n, d2, d3, c4) in seeds {
        let r = constants(n).unwrap().row;
        assert_eq!((r.d2.text(), r.d3.text(), r.c4.text()), (d2, d3, c4), "n = {n}");
    }
}

/// AC-3: n outside 2..=25 fails with `stats.subgroup_size_unsupported`.
#[test]
fn ac3_unsupported_sizes() {
    for n in [0u8, 1, 26, 255] {
        let e = constants(n).unwrap_err();
        assert_eq!(e, StatsError::SubgroupSizeUnsupported { n });
        assert_eq!(e.code(), "stats.subgroup_size_unsupported");
        assert_eq!(e.to_string(), format!("stats.subgroup_size_unsupported: n = {n}"));
    }
    for n in 2u8..=25 {
        assert_eq!(constants(n).unwrap().row.n, n);
    }
}

/// Every row is UNVERIFIED and its citation says no source has been chosen (REQ-002, D-045's
/// "nothing unreviewed looks settled").
#[test]
fn every_row_unverified_with_honest_citation() {
    for r in spc_table() {
        assert_eq!(r.review, ReviewStatus::Unverified, "n = {}", r.n);
        assert!(r.citation.source.starts_with("none chosen"), "n = {}", r.n);
        assert_eq!(constants(r.n).unwrap().review, ReviewStatus::Unverified);
    }
}

/// Every released version and its digest, as literals (REQ-007). A released entry never changes;
/// a new table is a new version with a new entry and a new pin file.
const RELEASED: &[(&str, &str)] =
    &[("1.0.0", "sha256:4e1161ea7b62c9e597a96850628c0118fcf9c34e58004f1be71b90e78fd1e4f8")];

/// REQ-007: the embedded table is the one released under its version.
#[test]
fn released_version_digest_is_unchanged() {
    let entry = RELEASED.iter().find(|(v, _)| *v == SPC_TABLE_VERSION);
    let (_, digest) = entry.unwrap_or_else(|| panic!("version {SPC_TABLE_VERSION} is not in RELEASED"));
    assert_eq!(fid_testkit::sha256_hex(SPC_TABLE_JSON), *digest);
}

/// AC-4: the embedded table hashes to the released digest.
#[test]
fn ac4_embedded_table_matches_released_digest() {
    let path = format!("{}/fixtures/rpt/stats/spc_constants_v1.sha256", fid_testkit::WORKSPACE_ROOT);
    let pinned = std::fs::read_to_string(path).unwrap();
    assert_eq!(fid_testkit::sha256_hex(SPC_TABLE_JSON), pinned.trim());
    assert_eq!(SPC_TABLE_VERSION, "1.0.0", "a new digest needs a new version and a new pin file");
}

fn str_field<'a>(j: &'a Json, key: &str) -> &'a str {
    j.get(key).str()
}

/// The Rust rows are exactly the embedded JSON's rows (the JSON is what was released and pinned).
#[test]
fn rows_equal_the_embedded_json() {
    let doc = parse_json(core::str::from_utf8(SPC_TABLE_JSON).unwrap());
    assert_eq!(str_field(&doc, "table_id"), SPC_TABLE_ID);
    assert_eq!(str_field(&doc, "version"), SPC_TABLE_VERSION);
    let rounding = doc.get("rounding");
    for key in ["c4", "d2", "d3", "factors"] {
        assert!(!rounding.get(key).is_null(), "rounding.{key}");
    }
    let rows = doc.get("rows").arr();
    assert_eq!(rows.len(), spc_table().len());
    for (j, r) in rows.iter().zip(spc_table()) {
        assert_eq!(j.get("n").num(), f64::from(r.n));
        for (name, v) in stored_values(r) {
            assert_eq!(str_field(j, name), v.text(), "{name} at n = {}", r.n);
        }
        if r.e2.is_none() {
            assert!(j.get("e2").is_null(), "e2 at n = {}", r.n);
        }
        assert_eq!(str_field(j, "review"), r.review.as_str());
        let cit = j.get("citation");
        assert_eq!(str_field(cit, "source"), r.citation.source);
        assert_eq!(str_field(cit, "edition"), r.citation.edition);
        assert_eq!(str_field(cit, "table"), r.citation.table);
    }
}

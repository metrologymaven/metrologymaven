//! UNC.RDL.REP AC-1 .. AC-5 and AC-7 over `fixtures/rdl/rep/cases.csv`, whose labels come
//! from the independent reference `fixtures/rdl/rep/make_cases.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::kdec::Decimal;
use fid_rep::{classify, combine, Annotation, Ctx, Determinism, FieldKind, NotReplayableCause, Outcome};

const CASES: &str = include_str!("../../../fixtures/rdl/rep/cases.csv");

fn value(s: &str) -> Option<f64> {
    (s != "absent").then(|| f64::from_bits(u64::from_str_radix(s, 16).unwrap()))
}

/// `(expected, got)` for one CSV row, both as `OUTCOME[|ANNOTATION...]` or `ERR:<code>`.
fn run_row(f: &[&str]) -> (String, String) {
    let want = if f[9].is_empty() { f[8].to_string() } else { format!("{}|{}", f[8], f[9]) };
    let ctx = Ctx {
        same_manifest: f[4] == "1",
        determinism: Determinism::parse(f[5]).unwrap(),
        same_platform_class: f[6] == "1",
        limit: (!f[7].is_empty()).then(|| Decimal::parse(f[7]).unwrap()),
    };
    let got = match FieldKind::parse(f[1]).and_then(|k| classify(k, value(f[2]), value(f[3]), &ctx)) {
        Err(e) => format!("ERR:{}", e.code()),
        Ok(o) => {
            let mut s = o.outcome.as_str().to_string();
            for a in &o.annotations {
                s.push('|');
                s.push_str(a.as_str());
            }
            s
        }
    };
    (want, got)
}

fn rows() -> Vec<Vec<&'static str>> {
    let mut lines = CASES.lines();
    assert_eq!(
        lines.next().unwrap(),
        "name,kind,recorded,replayed,same_manifest,determinism,same_platform_class,limit,expected,annotation"
    );
    lines.map(|l| l.split(',').collect::<Vec<_>>()).inspect(|f| assert_eq!(f.len(), 10, "{f:?}")).collect()
}

fn row(name: &str) -> (String, String) {
    let r = rows().into_iter().find(|f| f[0] == name).unwrap_or_else(|| panic!("no row {name}"));
    run_row(&r)
}

/// AC-1 (and AC-7: the labels are fixed, so passing on every CI target means identical results).
#[test]
fn ac1_every_row_matches() {
    let rows = rows();
    assert!(rows.len() >= 100, "{} rows", rows.len());
    let bad: Vec<String> = rows
        .iter()
        .filter_map(|f| {
            let (want, got) = run_row(f);
            (want != got).then(|| format!("{}: want {want}, got {got}", f[0]))
        })
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

/// AC-2
#[test]
fn ac2_length_boundary() {
    assert_eq!(row("len_just_over").1, "DIVERGENT");
    assert_eq!(row("len_just_under").1, "EQUIVALENT");
    // 1e-9 is not a double; the exact comparison rejects fl(1e-9).
    assert_eq!(row("len_decimal_not_f64_over").1, "DIVERGENT");
}

/// AC-3
#[test]
fn ac3_uec0() {
    let ctx = Ctx { same_manifest: false, determinism: Determinism::D3, same_platform_class: true, limit: None };
    let u = FieldKind::ExpandedUncertainty;
    assert_eq!(classify(u, None, None, &ctx).unwrap().outcome, Outcome::Identical);
    assert_eq!(classify(u, Some(0.004), None, &ctx).unwrap().outcome, Outcome::Divergent);
    assert_eq!(classify(u, None, Some(0.004), &ctx).unwrap().outcome, Outcome::Divergent);
}

/// AC-4: CONFORMS versus INDETERMINATE with p differing by 1e-7.
#[test]
fn ac4_outcome_flip() {
    let ctx = Ctx { same_manifest: false, determinism: Determinism::D3, same_platform_class: true, limit: None };
    let p = classify(FieldKind::PConformance, Some(0.95), Some(0.9500001), &ctx).unwrap();
    assert_eq!(p.outcome, Outcome::Equivalent);
    let decision = fid_rep::classify_exact_bytes(b"CONFORMS", b"INDETERMINATE");
    assert_eq!(decision.outcome, Outcome::Divergent);
    assert_eq!(combine(&[p.outcome, decision.outcome]), Outcome::Divergent);
}

/// AC-5
#[test]
fn ac5_same_manifest_ulp() {
    assert_eq!(row("ulp_same_manifest_D0").1, "DIVERGENT");
    let ctx = Ctx { same_manifest: true, determinism: Determinism::D0, same_platform_class: true, limit: None };
    let x = 7.25f64;
    let o = classify(FieldKind::Length, Some(x), Some(f64::from_bits(x.to_bits() + 1)), &ctx).unwrap();
    assert_eq!(o.outcome, Outcome::Divergent);
}

/// AC-6 and REQ-UNC.RDL.REP-008.
#[test]
fn ac6_aggregate() {
    use Outcome::*;
    let nr = NotReplayable(NotReplayableCause::Other);
    let trap = NotReplayable(NotReplayableCause::Resource);
    assert_eq!(combine(&[Equivalent, nr, Identical]), nr);
    assert_eq!(combine(&[Identical, Equivalent]), Equivalent);
    assert_eq!(combine(&[nr, Divergent, Identical]), Divergent);
    assert_eq!(combine(&[Identical]), Identical);
    assert_eq!(combine(&[]), nr);
    assert_eq!(combine(&[Equivalent, trap, nr]), trap);
    assert_eq!(trap.as_str(), "NOT_REPLAYABLE(resource)");
    assert_eq!(nr.as_str(), "NOT_REPLAYABLE");
}

/// A table without a row for the kind: `DIVERGENT` with `NO_TOLERANCE` unless bit-equal.
#[test]
fn missing_bound_is_no_tolerance() {
    let rows = fid_rep::Table::rep1().bounds().iter().filter(|b| b.kind != FieldKind::Angle).cloned().collect();
    let t = fid_rep::Table::new("test-only@0", rows).unwrap();
    let ctx = Ctx { same_manifest: false, determinism: Determinism::D3, same_platform_class: true, limit: None };
    let o = fid_rep::classify_with(&t, FieldKind::Angle, Some(1.0), Some(1.0 + 1e-12), &ctx).unwrap();
    assert_eq!(o.outcome, Outcome::Divergent);
    assert_eq!(o.annotations, vec![Annotation::NoTolerance]);
    let o = fid_rep::classify_with(&t, FieldKind::Angle, Some(1.0), Some(1.0), &ctx).unwrap();
    assert_eq!(o.outcome, Outcome::Identical);
    assert!(o.annotations.is_empty());
}

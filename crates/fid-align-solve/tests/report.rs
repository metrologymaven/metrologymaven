//! Acceptance criteria AC-2 and AC-3 of spec node ALN.QA.REPORT plus the pinned edge cases.
//! (AC-1, AC-4 and AC-5 are in `fid-align`'s `tests/residuals.rs`, which reads the JSON
//! fixture with `fid-canon`.) Every expected statistic, stats JSON and digest comes from
//! `fixtures/aln/qa/make_report_fixtures.py`, an independent Python reference of the rules
//! pinned in the leaf's Design; nothing expected is produced by this crate. CI runs this file
//! on Linux, Windows, macOS and wasm32-wasip1, so equality with the reference on each is
//! bit-identity across platforms.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align_solve::{
    Disposition, Entry, EntryKind, EntryRef, RejectReason, ReportBuilder, ReportError, ResidualReport, Unit, MAX_ITEM,
    MAX_REASONS,
};
use std::collections::BTreeMap;

// The workspace root comes from fid-testkit: kernel-closure sources do not use the
// manifest-directory macro (KRN.MAN.MANI.IMPL).
fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{}/fixtures/aln/qa/{name}", fid_testkit::WORKSPACE_ROOT)).unwrap()
}

/// `2^k` for `k` in `[-1022, 1023]`, exactly (no std float power in kernel-closure code).
fn pow2(k: i32) -> f64 {
    f64::from_bits(((k + 1023) as u64) << 52)
}

fn bits(x: f64) -> String {
    format!("0x{:016x}", x.to_bits())
}

fn golden() -> BTreeMap<String, String> {
    read("report-1m.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (k, v) = l.split_once(' ').unwrap();
            (k.to_string(), v.to_string())
        })
        .collect()
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The seeded generator pinned in the node (seed 13), entry by entry.
fn big_report(n: usize) -> ResidualReport {
    let outlier = RejectReason::new("outlier").unwrap();
    let mut b = ReportBuilder::new();
    let mut state = 13u64;
    for i in 0..n {
        let z = splitmix64(&mut state);
        let kind = EntryKind::ALL[(z & 3) as usize];
        let used = (z >> 2) & 15 != 0;
        let weight = 1.0 + ((z >> 6) & 7) as f64 * 0.25;
        let r = ((z >> 11) as f64 * pow2(-53) - 0.5) * 0.002;
        let vector = (kind == EntryKind::Point).then_some([r * 0.6, r * 0.8, 0.0]);
        let entry = Entry {
            reference: EntryRef { input: (i % 3) as u32, item: i as u64 },
            kind,
            residual_m: r,
            vector_m: vector,
            weight: Some(weight),
        };
        let d = if used { Disposition::Used } else { Disposition::Rejected(outlier.clone()) };
        assert_eq!(b.push(entry, d).unwrap(), i as u64);
    }
    b.finish().unwrap()
}

fn plain(r: f64) -> Entry {
    Entry {
        reference: EntryRef { input: 0, item: 0 },
        kind: EntryKind::Feature,
        residual_m: r,
        vector_m: None,
        weight: None,
    }
}

fn weighted(r: f64, w: f64) -> Entry {
    Entry { weight: Some(w), ..plain(r) }
}

fn build(rs: &[f64]) -> Result<ResidualReport, ReportError> {
    let mut b = ReportBuilder::new();
    for &r in rs {
        b.push(plain(r), Disposition::Used)?;
    }
    b.finish()
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_the_seeded_million_entry_report_equals_the_reference_bit_for_bit() {
    let g = golden();
    let rep = big_report(1_000_000);
    let st = rep.stats();
    assert_eq!(rep.len().to_string(), g["n_entries"]);
    assert_eq!(st.n_used().to_string(), g["n_used"]);
    assert_eq!(st.n_rejected().to_string(), g["n_rejected"]);
    assert_eq!(bits(st.rms().unwrap().value()), g["rms"]);
    assert_eq!(bits(st.weighted_rms().unwrap().value()), g["weighted_rms"]);
    assert_eq!(bits(st.max_abs().unwrap().value()), g["max_abs"]);
    assert_eq!(st.max_entry().unwrap().to_string(), g["max_entry"]);
    assert_eq!(std::str::from_utf8(rep.stats_json()).unwrap(), g["stats_json"]);
    assert_eq!(rep.digest().as_str(), g["digest"]);
    // AC-5's spot checks of the generator.
    assert_eq!(bits(rep.entry(900_000).unwrap().residual.value()), g["entry_900000"]);
    assert_eq!(bits(rep.entry(999_999).unwrap().residual.value()), g["entry_999999"]);
    // Units are carried with every statistic and entry.
    assert_eq!(st.rms().unwrap().unit(), Unit::Metre);
    assert_eq!(st.weighted_rms().unwrap().unit().as_str(), "m");
    assert_eq!(rep.entry(0).unwrap().residual.unit(), Unit::Metre);
}

// ------------------------------------------------------------------ AC-3 and refusals

#[test]
fn ac3_a_nan_or_infinite_residual_fails_with_nan_residual_naming_the_entry() {
    let mut b = ReportBuilder::new();
    b.push(plain(1e-3), Disposition::Used).unwrap();
    let err = b.push(plain(f64::NAN), Disposition::Used).unwrap_err();
    assert_eq!(err, ReportError::NanResidual { index: 1 });
    assert_eq!(err.code(), "nan_residual");
    assert_eq!(err.to_string(), "nan_residual: entry 1");
    // Nothing was recorded: the next entry is index 1 again.
    assert_eq!(b.len(), 1);
    for bad in [f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(b.push(plain(bad), Disposition::Used).unwrap_err().code(), "nan_residual");
    }
    let mut v = plain(0.0);
    v.vector_m = Some([0.0, f64::NAN, 0.0]);
    assert_eq!(b.push(v, Disposition::Used).unwrap_err(), ReportError::NanResidual { index: 1 });
    // A rejected entry is checked too.
    let r = Disposition::Rejected(RejectReason::new("outlier").unwrap());
    assert_eq!(b.push(plain(f64::NAN), r).unwrap_err().code(), "nan_residual");
    assert_eq!(b.len(), 1);
}

#[test]
fn an_empty_report_is_refused_and_an_all_rejected_report_has_counts_only() {
    assert_eq!(ReportBuilder::new().finish().unwrap_err(), ReportError::EmptyReport);
    let mut b = ReportBuilder::new();
    let r = RejectReason::new("outlier").unwrap();
    b.push(plain(1e-3), Disposition::Rejected(r.clone())).unwrap();
    b.push(plain(-2e-3), Disposition::Rejected(r)).unwrap();
    let rep = b.finish().unwrap();
    let st = rep.stats();
    assert_eq!((st.n_used(), st.n_rejected()), (0, 2));
    assert!(st.rms().is_none() && st.weighted_rms().is_none() && st.max_abs().is_none() && st.max_entry().is_none());
    assert_eq!(std::str::from_utf8(rep.stats_json()).unwrap(), r#"{"n_rejected":"2","n_used":"0"}"#);
    assert_eq!(rep.entry(1).unwrap().reason, Some("outlier"));
}

#[test]
fn sums_of_squares_beyond_f64_max_give_stat_overflow() {
    let big = pow2(511);
    // 2 * 2^1022 = 2^1023 is finite: RMS is 2^511 exactly.
    let rep = build(&[big, -big]).unwrap();
    assert_eq!(rep.stats().rms().unwrap().value(), big);
    // 4 * 2^1022 = 2^1024 overflows in the sum; 2^512 squared overflows in the product.
    assert_eq!(build(&[big; 4]).unwrap_err(), ReportError::StatOverflow);
    assert_eq!(build(&[pow2(512)]).unwrap_err().code(), "stat_overflow");
    assert_eq!(build(&[f64::MAX]).unwrap_err().code(), "stat_overflow");
    // A weighted square that overflows (2^100 * 2^1000) while the plain square does not.
    let mut b = ReportBuilder::new();
    b.push(weighted(pow2(500), pow2(100)), Disposition::Used).unwrap();
    assert_eq!(b.finish().unwrap_err(), ReportError::StatOverflow);
}

#[test]
fn weights_are_all_or_none_and_positive() {
    let mut b = ReportBuilder::new();
    b.push(weighted(1e-3, 1.0), Disposition::Used).unwrap();
    assert_eq!(b.push(plain(1e-3), Disposition::Used).unwrap_err(), ReportError::MixedWeights { index: 1 });
    for w in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(b.push(weighted(1e-3, w), Disposition::Used).unwrap_err(), ReportError::BadWeight { index: 1 });
    }
    let mut b = ReportBuilder::new();
    b.push(plain(1e-3), Disposition::Used).unwrap();
    assert_eq!(b.push(weighted(1e-3, 1.0), Disposition::Used).unwrap_err().code(), "mixed_weights");
}

#[test]
fn entry_fields_fail_closed() {
    for bad in ["", "Outlier", "1x", "a-b", "a b", &"a".repeat(65)] {
        assert_eq!(RejectReason::new(bad).unwrap_err().code(), "bad_entry", "{bad:?}");
    }
    assert!(RejectReason::new(&"a".repeat(64)).is_ok());
    let mut b = ReportBuilder::new();
    let mut e = plain(0.0);
    e.reference.item = MAX_ITEM + 1;
    assert_eq!(b.push(e, Disposition::Used).unwrap_err().code(), "bad_entry");
    e.reference.item = MAX_ITEM;
    assert_eq!(b.push(e, Disposition::Used).unwrap(), 0);
    // At most MAX_REASONS distinct reasons; a repeated one is free.
    let mut b = ReportBuilder::new();
    for k in 0..MAX_REASONS {
        b.push(plain(0.0), Disposition::Rejected(RejectReason::new(&format!("r{k}")).unwrap())).unwrap();
    }
    b.push(plain(0.0), Disposition::Rejected(RejectReason::new("r0").unwrap())).unwrap();
    let extra = Disposition::Rejected(RejectReason::new("one_more").unwrap());
    assert_eq!(b.push(plain(0.0), extra).unwrap_err(), ReportError::InputLimit("reasons"));
    assert_eq!(b.len(), MAX_REASONS + 1);
}

#[test]
fn the_maximum_takes_the_lowest_index_on_ties_and_negative_zero_is_stored_as_plus_zero() {
    let u = pow2(-8);
    let rep = build(&[-3.0 * u, 3.0 * u, -3.0 * u, 1.0 * u]).unwrap();
    assert_eq!(rep.stats().max_entry(), Some(0));
    assert_eq!(rep.stats().max_abs().unwrap().value(), 3.0 * u);
    let mut b = ReportBuilder::new();
    let mut e = plain(-0.0);
    e.vector_m = Some([-0.0, 0.0, -0.0]);
    b.push(e, Disposition::Used).unwrap();
    let rep = b.finish().unwrap();
    let v = rep.entry(0).unwrap();
    assert_eq!(v.residual.value().to_bits(), 0);
    assert!(v.vector.unwrap().value().iter().all(|c| c.to_bits() == 0));
    assert_eq!(rep.stats().rms().unwrap().value().to_bits(), 0);
}

#[test]
fn reports_are_deterministic_and_pages_are_bounded_slices() {
    let a = big_report(3000);
    let b = big_report(3000);
    assert_eq!(a, b);
    assert_eq!(a.digest(), b.digest());
    let page: Vec<u64> = a.entries(2990, 100).map(|e| e.index).collect();
    assert_eq!(page, (2990..3000).collect::<Vec<u64>>());
    assert_eq!(a.entries(3000, 10).count(), 0);
    assert_eq!(a.entries(usize::MAX, usize::MAX).count(), 0);
    // A different entry changes the digest.
    let c = big_report(2999);
    assert_ne!(a.digest(), c.digest());
}

//! RPT.STATS.TREND.HIST tests. The cases in `fixtures/rpt/stats/hist_cases.json` are
//! hand-chosen and hand-labelled; their expected edges (IEEE 754 bits), counts and both digests
//! come from the independent Python reference `fixtures/rpt/stats/make_hist_cases.py`, never
//! from running this crate. CI runs this file on x86-64, ARM64 and wasm32 (wasmtime), so the
//! digest tests are the cross-target identity of AC-4.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{parse_json, Json};
use fid_num::rng::{Address, Cursor};
use fid_stats::{histogram, BinRule, BinningReason, Bins, RuleId, StatsError, HISTOGRAM_MAX_BINS};

fn fixture() -> Json {
    let file = format!("{}/fixtures/rpt/stats/hist_cases.json", fid_testkit::WORKSPACE_ROOT);
    parse_json(&std::fs::read_to_string(file).unwrap())
}

fn hex_f64(s: &str) -> f64 {
    assert_eq!(s.len(), 16, "{s}");
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn rule(j: &Json) -> BinRule {
    match j.get("name").str() {
        "sturges" => BinRule::Sturges,
        "fixed_count" => BinRule::FixedCount { k: j.get("k").num() as u32 },
        "fixed_width" => {
            BinRule::FixedWidth { width: hex_f64(j.get("width").str()), anchor: hex_f64(j.get("anchor").str()) }
        }
        other => panic!("rule {other}"),
    }
}

fn error_token(e: StatsError) -> String {
    match e {
        StatsError::BadBinning { reason } => format!("{}:{}", e.code(), reason.as_str()),
        StatsError::NonFinite { index } => format!("{}:{index}", e.code()),
        StatsError::InsufficientData => e.code().to_owned(),
        other => panic!("unexpected error kind {other}"),
    }
}

/// The digest block of one result, as `make_hist_cases.py` writes it.
fn block(id: &str, r: &Result<Bins, StatsError>) -> String {
    match r {
        Err(e) => format!("{id} error:{}\n", error_token(*e)),
        Ok(b) => {
            let mut s = format!("{id} {} {} {}\n", b.rule.as_str(), b.edges.len(), u8::from(b.degenerate));
            for e in &b.edges {
                s.push_str(&format!("e {:016x}\n", e.to_bits()));
            }
            for c in &b.counts {
                s.push_str(&format!("c {c}\n"));
            }
            s
        }
    }
}

fn digest(text: &str) -> String {
    let got = fid_testkit::sha256_hex(text.as_bytes());
    got.strip_prefix("sha256:").unwrap_or(&got).to_owned()
}

/// The shape every successful result must have (REQ-004, REQ-005, REQ-007).
fn check_invariants(id: &str, n: usize, b: &Bins) {
    assert_eq!(b.edges.len(), b.counts.len() + 1, "{id}");
    assert_eq!(b.total(), n as u64, "{id}: counts sum to n");
    assert!((1..=HISTOGRAM_MAX_BINS).contains(&b.bin_count()), "{id}");
    assert!(b.edges.iter().all(|e| e.is_finite()), "{id}");
    if b.degenerate {
        assert_eq!(b.bin_count(), 1, "{id}");
        assert_eq!(b.edges[0].to_bits(), b.edges[1].to_bits(), "{id}");
    } else {
        assert!(b.edges.windows(2).all(|p| p[0] < p[1]), "{id}: strictly increasing edges");
    }
}

/// AC-1, AC-2, AC-3 and every other fixture case: edges bit for bit, counts, the degenerate
/// flag and the rule ID, or the refusal with its reason; then the digest of all results.
#[test]
fn fixture_cases_and_cross_target_digest() {
    let fx = fixture();
    assert_eq!(fx.get("max_bins").num() as usize, HISTOGRAM_MAX_BINS);
    let cases = fx.get("cases").arr();
    assert!(cases.len() >= 30, "{}", cases.len());
    let mut text = String::new();
    for c in cases {
        let id = c.get("id").str();
        let values: Vec<f64> = c.get("values").arr().iter().map(|v| hex_f64(v.str())).collect();
        let r = rule(c.get("rule"));
        let want = c.get("expect");
        let got = histogram(&values, &r);
        match &got {
            Err(e) => {
                assert!(!want.get("error").is_null(), "{id}: unexpected error {e}");
                assert_eq!(error_token(*e), want.get("error").str(), "{id}");
            }
            Ok(b) => {
                assert!(want.get("error").is_null(), "{id}: expected {:?}, got {b:?}", want.get("error"));
                assert_eq!(b.rule.as_str(), want.get("rule_id").str(), "{id}");
                assert_eq!(b.rule, r.id(), "{id}");
                let edges: Vec<u64> = want.get("edges").arr().iter().map(|e| hex_f64(e.str()).to_bits()).collect();
                assert_eq!(b.edges.iter().map(|e| e.to_bits()).collect::<Vec<_>>(), edges, "{id}: edges");
                let counts: Vec<u64> = want.get("counts").arr().iter().map(|x| x.num() as u64).collect();
                assert_eq!(b.counts, counts, "{id}: counts");
                assert_eq!(b.degenerate, want.get("degenerate").truthy(), "{id}: degenerate");
                check_invariants(id, values.len(), b);
            }
        }
        text.push_str(&block(id, &got));
    }
    assert_eq!(digest(&text), fx.get("digest").str(), "cross-target result digest");
}

/// AC-4: 10 000 values from the fid-num Philox stream (seed 42), binned with each rule; the
/// bins and the digest equal the reference's on every target.
#[test]
fn random_10k_cross_target_digest() {
    let fx = fixture();
    let rnd = fx.get("random_10k");
    let addr = Address { seed: rnd.get("seed").num() as u64, stream: rnd.get("stream").num() as u64 };
    let n = rnd.get("n").num() as usize;
    assert_eq!(n, 10_000);
    let scale = hex_f64(rnd.get("scale").str());
    let offset = hex_f64(rnd.get("offset").str());
    let mut cur = Cursor::new(addr, 0);
    let values: Vec<f64> = (0..n).map(|_| scale * cur.uniform_f64() + offset).collect();
    let mut text = String::new();
    let rules = rnd.get("rules").arr();
    assert_eq!(rules.len(), 3);
    for (want, expected_id) in rules.iter().zip([RuleId::Sturges, RuleId::FixedCount, RuleId::FixedWidth]) {
        let r = rule(want.get("rule"));
        assert_eq!(r.id(), expected_id);
        let got = histogram(&values, &r);
        let b = got.as_ref().unwrap();
        assert_eq!(b.rule.as_str(), want.get("rule_id").str());
        assert_eq!(b.bin_count(), want.get("bins").num() as usize);
        let counts: Vec<u64> = want.get("counts").arr().iter().map(|x| x.num() as u64).collect();
        assert_eq!(b.counts, counts, "{}", b.rule);
        assert!(!b.degenerate);
        check_invariants(b.rule.as_str(), n, b);
        text.push_str(&block("random_10k", &got));
    }
    // Sturges for n = 10 000: bit_length(9 999) + 1 = 14 + 1 = 15 bins (hand-computed).
    assert_eq!(histogram(&values, &BinRule::Sturges).unwrap().bin_count(), 15);
    assert_eq!(digest(&text), rnd.get("digest").str(), "cross-target digest of the seeded histograms");
}

/// AC-2 written out: {0, 1, 2, 3, 4} with fixed_count k = 4.
#[test]
fn ac2_edge_assignment() {
    let b = histogram(&[0.0, 1.0, 2.0, 3.0, 4.0], &BinRule::FixedCount { k: 4 }).unwrap();
    assert_eq!(b.edges, vec![0.0, 1.0, 2.0, 3.0, 4.0]);
    assert_eq!(b.counts, vec![1, 1, 1, 2]);
    assert_eq!(b.rule, RuleId::FixedCount);
    assert!(!b.degenerate);
}

/// AC-3 written out: width 0 fails with `stats.bad_binning` and returns no bins.
#[test]
fn ac3_zero_width_refused() {
    let e = histogram(&[1.0, 2.0], &BinRule::FixedWidth { width: 0.0, anchor: 0.0 }).unwrap_err();
    assert_eq!(e, StatsError::BadBinning { reason: BinningReason::WidthNotPositive });
    assert_eq!(e.code(), "stats.bad_binning");
    assert_eq!(e.to_string(), "stats.bad_binning: width_not_positive");
}

/// REQ-001: k = ⌈log2 n⌉ + 1, hand-computed for n around powers of two.
#[test]
fn sturges_bin_counts() {
    for (n, k) in [(2usize, 2usize), (3, 3), (4, 3), (5, 4), (8, 4), (9, 5), (16, 5), (17, 6), (1024, 11), (1025, 12)] {
        let values: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let b = histogram(&values, &BinRule::Sturges).unwrap();
        assert_eq!(b.bin_count(), k, "n = {n}");
        assert_eq!(b.total(), n as u64);
        assert_eq!(b.edges[k], (n - 1) as f64, "n = {n}: e_k is max exactly");
    }
}

/// The histogram depends on the data set, not on the order of its values.
#[test]
fn order_of_values_does_not_matter() {
    let mut cur = Cursor::new(Address { seed: 7, stream: 3 }, 0);
    let values: Vec<f64> = (0..500).map(|_| 4.0 * cur.uniform_f64() - 1.0).collect();
    let mut shuffled = values.clone();
    fid_num::rng::shuffle(&mut shuffled, &mut cur).unwrap();
    assert_ne!(values, shuffled);
    for r in [BinRule::Sturges, BinRule::FixedCount { k: 13 }, BinRule::FixedWidth { width: 0.3, anchor: 0.05 }] {
        let a = histogram(&values, &r).unwrap();
        let b = histogram(&shuffled, &r).unwrap();
        assert_eq!(
            a.edges.iter().map(|e| e.to_bits()).collect::<Vec<_>>(),
            b.edges.iter().map(|e| e.to_bits()).collect::<Vec<_>>()
        );
        assert_eq!(a.counts, b.counts);
    }
}

/// Refusals carry their reason and return no bins; nothing is silently dropped.
#[test]
fn refusals_and_codes() {
    assert_eq!(histogram(&[], &BinRule::Sturges), Err(StatsError::InsufficientData));
    assert_eq!(StatsError::InsufficientData.code(), "stats.insufficient_data");
    let e = histogram(&[0.0, f64::NEG_INFINITY], &BinRule::Sturges).unwrap_err();
    assert_eq!(e, StatsError::NonFinite { index: 1 });
    assert_eq!(e.to_string(), "stats.non_finite: index 1");
    let e = histogram(&[0.0, 1.0], &BinRule::FixedCount { k: 0 }).unwrap_err();
    assert_eq!(e, StatsError::BadBinning { reason: BinningReason::ZeroBins });
    let ok = histogram(&[0.0, 1.0], &BinRule::FixedCount { k: HISTOGRAM_MAX_BINS as u32 }).unwrap();
    assert_eq!(ok.bin_count(), HISTOGRAM_MAX_BINS);
    let e = histogram(&[0.0, 1.0], &BinRule::FixedCount { k: HISTOGRAM_MAX_BINS as u32 + 1 }).unwrap_err();
    assert_eq!(e, StatsError::BadBinning { reason: BinningReason::TooManyBins });
    let e = histogram(&[0.0, 1.0], &BinRule::FixedWidth { width: 1.0, anchor: f64::INFINITY }).unwrap_err();
    assert_eq!(e, StatsError::BadBinning { reason: BinningReason::AnchorNotFinite });
}

#[test]
fn rule_ids() {
    assert_eq!(BinRule::default(), BinRule::Sturges);
    for (id, s, name) in [
        (RuleId::Sturges, "sturges@1", "sturges"),
        (RuleId::FixedCount, "fixed_count@1", "fixed_count"),
        (RuleId::FixedWidth, "fixed_width@1", "fixed_width"),
    ] {
        assert_eq!(id.as_str(), s);
        assert_eq!(id.to_string(), s);
        assert_eq!(id.name(), name);
        assert_eq!(id.version(), 1);
    }
}

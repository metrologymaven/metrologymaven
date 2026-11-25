//! Acceptance criteria of spec node GDT.DIM.SIZEISO.RANK (AC-1 .. AC-4) on
//! `dim::size::iso::rank`. Inputs and expected values come from the independent stdlib-Python
//! oracle `fixtures/gdt/size/make_rank_fixtures.py`: `rank_sizes.json` (AC-1, AC-2, AC-3 and
//! edge cases) and `rank_seeded.json` (AC-4). Every `exact` value is the correctly rounded
//! true statistic (rational arithmetic); every `pinned` value is the leaf's written procedure
//! re-implemented in the oracle. Comparing bit patterns on every CI platform is AC-4's
//! platform bit-identity. Invented values only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{digest_bytes, CValue};
use fid_gps::dim::size::iso::rank::{rank_statistic, MedianEven, RankError, RankStat, StdDenominator};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/size")
}

fn load(name: &str) -> CValue {
    fid_canon::parse(&std::fs::read(dir().join(name)).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("object"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("array"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("string"),
    }
}

fn int(v: &CValue) -> u64 {
    match v {
        CValue::Num(n) => *n as u64,
        _ => panic!("number"),
    }
}

fn bits(v: &CValue) -> u64 {
    u64::from_str_radix(st(v).strip_prefix("0x").unwrap(), 16).unwrap()
}

fn hx(x: f64) -> String {
    format!("0x{:016x}", x.to_bits())
}

/// IEEE 754 totalOrder as an unsigned key (NaN-free values).
fn order_key(x: f64) -> u64 {
    let b = x.to_bits();
    if b >> 63 == 1 {
        !b
    } else {
        b | (1 << 63)
    }
}

fn ulps(a: f64, b: f64) -> u64 {
    order_key(a).abs_diff(order_key(b))
}

/// Checks one result against its oracle entry: an error code (and its fields), or the bits
/// of `pinned` if present (else of `exact`) and the distance to `exact` within `max_ulps`.
fn check(what: &str, got: Result<f64, RankError>, want: &BTreeMap<String, CValue>) {
    if let Some(code) = want.get("error") {
        let e = got.expect_err(what);
        assert_eq!(e.code(), st(code), "{what}");
        match e {
            RankError::NanInput { index } | RankError::InfiniteInput { index } => {
                assert_eq!(index as u64, int(&want["index"]), "{what}")
            }
            RankError::InsufficientCount { needed, got } => {
                assert_eq!(needed as u64, int(&want["needed"]), "{what}");
                assert_eq!(got as u64, int(&want["got"]), "{what}");
            }
            _ => {}
        }
        return;
    }
    let x = got.unwrap_or_else(|e| panic!("{what}: {e}"));
    let exact = f64::from_bits(bits(&want["exact"]));
    match want.get("pinned") {
        Some(p) => {
            assert_eq!(hx(x), st(p), "{what}: pinned procedure");
            let max = int(&want["max_ulps"]);
            assert!(ulps(x, exact) <= max, "{what}: {} ulps from exact {}", ulps(x, exact), hx(exact));
        }
        None => assert_eq!(hx(x), hx(exact), "{what}: exact"),
    }
}

/// AC-1, AC-2, AC-3 and the edge cases: every statistic of every case, bit for bit.
#[test]
fn cases_match_the_oracle() {
    let doc = load("rank_sizes.json");
    let keys: Vec<&str> = arr(&obj(&doc)["keys"]).iter().map(st).collect();
    let all: Vec<&str> = RankStat::ALL.iter().map(|s| s.key()).collect();
    assert_eq!(keys, all, "the oracle covers every statistic");
    let cases = arr(&obj(&doc)["cases"]);
    assert!(cases.len() >= 15);
    for c in cases {
        let c = obj(c);
        let name = st(&c["name"]);
        let sizes: Vec<f64> = arr(&c["sizes"]).iter().map(|v| f64::from_bits(bits(v))).collect();
        let results = obj(&c["results"]);
        assert_eq!(results.len(), RankStat::ALL.len(), "{name}");
        for (key, want) in results {
            let stat = RankStat::from_key(key).unwrap_or_else(|| panic!("{name}: key {key}"));
            check(&format!("{name}/{key}"), rank_statistic(&sizes, stat), obj(want));
        }
    }
}

/// AC-1: the dyadic ten-value case, spelt out by hand (offsets from 10240 in units of
/// 2^-20 m: sorted -8 -3 -1 0 2 5 5 7 11 12, sum 30): max, min, range, mid-range, the
/// medians and the (exact) mean bit for bit. The standard deviations are checked against the
/// oracle's correctly rounded values in `cases_match_the_oracle` (within 1 ulp).
#[test]
fn ac1_dyadic_ten() {
    let u = 2f64.powi(-20);
    let sizes: Vec<f64> = [7, -3, 12, 0, -8, 5, 5, 2, -1, 11].iter().map(|k| f64::from(10240 + k) * u).collect();
    let v = |s| rank_statistic(&sizes, s).unwrap();
    assert_eq!(v(RankStat::Max), 10252.0 * u);
    assert_eq!(v(RankStat::Min), 10232.0 * u);
    assert_eq!(v(RankStat::Range), 20.0 * u);
    assert_eq!(v(RankStat::MidRange), 10242.0 * u);
    assert_eq!(v(RankStat::Median { even: MedianEven::Lower }), 10242.0 * u);
    assert_eq!(v(RankStat::Median { even: MedianEven::Upper }), 10245.0 * u);
    assert_eq!(v(RankStat::Median { even: MedianEven::MeanOfMiddle }), 10243.5 * u);
    assert_eq!(v(RankStat::Mean), 10243.0 * u);
}

/// AC-2: an even count gives the mean of the middle pair, the lower and the upper middle
/// value; the midpoint of 0.01 and its successor is a tie and rounds to even (0.01).
#[test]
fn ac2_even_medians() {
    let a = 0.01f64;
    let b = f64::from_bits(a.to_bits() + 1);
    let sizes = [0.0102, a, b, 0.0099, 0.0101, 0.0098];
    let m = |even| rank_statistic(&sizes, RankStat::Median { even }).unwrap();
    assert_eq!(m(MedianEven::Lower).to_bits(), a.to_bits());
    assert_eq!(m(MedianEven::Upper).to_bits(), b.to_bits());
    let even = if a.to_bits() % 2 == 0 { a } else { b };
    assert_eq!(m(MedianEven::MeanOfMiddle).to_bits(), even.to_bits());
}

/// AC-3: the typed errors, and their order.
#[test]
fn ac3_errors() {
    for s in RankStat::ALL {
        assert_eq!(rank_statistic(&[], s), Err(RankError::EmptySizes));
        assert_eq!(rank_statistic(&[1.0, f64::NAN, f64::INFINITY], s), Err(RankError::NanInput { index: 1 }));
        assert_eq!(rank_statistic(&[f64::NEG_INFINITY, f64::NAN], s), Err(RankError::InfiniteInput { index: 0 }));
    }
    let n1 = RankStat::StdDev { denominator: StdDenominator::NMinus1 };
    assert_eq!(rank_statistic(&[0.5], n1), Err(RankError::InsufficientCount { needed: 2, got: 1 }));
    assert_eq!(rank_statistic(&[f64::NAN], n1), Err(RankError::NanInput { index: 0 }));
    assert_eq!(rank_statistic(&[0.5], RankStat::StdDev { denominator: StdDenominator::N }), Ok(0.0));
    assert_eq!(rank_statistic(&[f64::MAX, -f64::MAX], RankStat::Range), Err(RankError::Overflow));
    assert_eq!(rank_statistic(&[f64::MAX, f64::MAX], RankStat::Mean), Err(RankError::Overflow));
    let codes: Vec<&str> = [
        RankError::EmptySizes,
        RankError::NanInput { index: 0 },
        RankError::InfiniteInput { index: 0 },
        RankError::InsufficientCount { needed: 2, got: 1 },
        RankError::Overflow,
    ]
    .iter()
    .map(RankError::code)
    .collect();
    assert_eq!(codes, ["empty_sizes", "nan_input", "infinite_input", "insufficient_count", "overflow"]);
}

/// The deviation-overflow path of the standard deviation: the sum (`-MAX`) and the mean
/// (`-MAX / 3`) are finite, but the deviation `MAX - mean` overflows, so both denominators
/// are `overflow` while the mean itself is returned.
#[test]
fn std_dev_deviation_overflow() {
    let sizes = [f64::MAX, -f64::MAX, -f64::MAX];
    assert_eq!(rank_statistic(&sizes, RankStat::Mean), Ok(-f64::MAX / 3.0));
    for denominator in [StdDenominator::N, StdDenominator::NMinus1] {
        assert_eq!(rank_statistic(&sizes, RankStat::StdDev { denominator }), Err(RankError::Overflow));
    }
}

/// Stable keys round-trip.
#[test]
fn keys_round_trip() {
    for s in RankStat::ALL {
        assert_eq!(RankStat::from_key(s.key()), Some(s));
    }
    assert_eq!(RankStat::from_key("median"), None);
}

/// The AC-4 stream; see the oracle's docstring (both must implement it identically).
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

/// AC-4: one million seeded sizes. Every statistic matches the oracle bit for bit (so it is
/// identical on every CI platform), and max, min, the three medians, range and mid-range
/// are unchanged when the input is reversed or shuffled.
#[test]
fn ac4_seeded_million() {
    let doc = load("rank_seeded.json");
    let doc = obj(&doc);
    let count = int(&doc["count"]) as usize;
    assert_eq!(count, 1_000_000);
    assert_eq!(st(&doc["seed"]), "0x512e");
    let scale = 2f64.powi(-33);
    let mut rng = SplitMix(0x512e);
    let sizes: Vec<f64> = (0..count).map(|_| ((1u64 << 23) + (rng.next() >> 41)) as f64 * scale).collect();
    let mut bytes = Vec::with_capacity(count * 8);
    for x in &sizes {
        bytes.extend_from_slice(&x.to_bits().to_be_bytes());
    }
    assert_eq!(
        digest_bytes(&bytes).to_string(),
        format!("sha256:{}", st(&doc["sha256"])),
        "the size stream differs from the oracle"
    );
    let results = obj(&doc["results"]);
    assert_eq!(results.len(), RankStat::ALL.len());
    let mut order_free = Vec::new();
    for s in RankStat::ALL {
        let got = rank_statistic(&sizes, s);
        check(&format!("seeded/{}", s.key()), got, obj(&results[s.key()]));
        if !matches!(s, RankStat::Mean | RankStat::StdDev { .. }) {
            order_free.push((s, got.unwrap()));
        }
    }
    assert_eq!(order_free.len(), 7);
    let mut reversed = sizes.clone();
    reversed.reverse();
    // A fixed permutation: Fisher-Yates driven by the same generator, seeded differently.
    let mut shuffled = sizes;
    let mut rng = SplitMix(0x5eed);
    for i in (1..shuffled.len()).rev() {
        let j = (rng.next() % (i as u64 + 1)) as usize;
        shuffled.swap(i, j);
    }
    for (s, want) in order_free {
        for (label, xs) in [("reversed", &reversed), ("shuffled", &shuffled)] {
            let got = rank_statistic(xs, s).unwrap();
            assert_eq!(hx(got), hx(want), "{} {label}", s.key());
        }
    }
}

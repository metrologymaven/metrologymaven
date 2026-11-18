//! Acceptance criteria of spec node KRN.NUM.KDEC (AC-1 to AC-5).
//! Expected values come from `fixtures/num/make_kdec_fixtures.py` (exact rationals).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use core::cmp::Ordering;

use fid_num::kdec::{self, Decimal, KdecError};
use fid_num::NumError;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/num/kdec-boundary-v1.txt");

fn records(kind: &str) -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(FIXTURE).expect("read fixture");
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| l.split('|').map(str::to_string).collect::<Vec<_>>())
        .filter(|f| f[0] == kind)
        .collect()
}

fn dec(s: &str) -> Decimal {
    Decimal::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"))
}

fn ord(s: &str) -> Ordering {
    match s {
        "LT" => Ordering::Less,
        "EQ" => Ordering::Equal,
        "GT" => Ordering::Greater,
        other => panic!("bad ordering {other}"),
    }
}

fn bits(s: &str) -> u64 {
    u64::from_str_radix(s, 16).expect("hex bits")
}

fn kind(s: &str) -> KdecError {
    match s {
        "syntax" => KdecError::Syntax,
        "too_many_digits" => KdecError::TooManyDigits,
        "exponent_range" => KdecError::ExponentRange,
        "coefficient_overflow" => KdecError::CoefficientOverflow,
        other => panic!("bad kind {other}"),
    }
}

/// Checks `got` against `ok|neg|coeff|exp10` or `err|kind` fields.
fn check_result(what: &str, got: Result<Decimal, NumError>, f: &[String]) {
    match f[0].as_str() {
        "ok" => {
            let d = got.unwrap_or_else(|e| panic!("{what}: unexpected error {e}"));
            assert_eq!(d.is_negative(), f[1] == "1", "{what}: sign");
            assert_eq!(d.coeff(), f[2].parse::<u128>().unwrap(), "{what}: coeff");
            assert_eq!(d.exp10(), f[3].parse::<i32>().unwrap(), "{what}: exp10");
        }
        "err" => {
            let e = got.expect_err(what);
            assert_eq!(KdecError::of(&e), Some(kind(&f[1])), "{what}: {e}");
        }
        other => panic!("{what}: bad expectation {other}"),
    }
}

/// FNV-1a over every output, for the cross-host identity check (AC-5).
fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// AC-1: every ordering of the boundary fixture matches the exact rational result.
#[test]
fn ac1_boundary_orderings() {
    let rows = records("cmp");
    assert!(rows.len() >= 20_000, "fixture too small: {}", rows.len());
    let mut by_kind = [0usize; 3];
    for f in &rows {
        let d = dec(&f[1]).shift10(f[2].parse().unwrap()).unwrap();
        let x = f64::from_bits(bits(&f[3]));
        let want = ord(&f[4]);
        assert_eq!(kdec::cmp(x, &d), Ok(want), "x={x:e} ({}) vs {}e{}", f[3], f[1], f[2]);
        by_kind[(want as i8 + 1) as usize] += 1;
    }
    assert!(by_kind.iter().all(|&n| n > 100), "all three orderings exercised: {by_kind:?}");
}

/// AC-2: the 10.050 mm limit in metres.
#[test]
fn ac2_ten_point_zero_five_zero_mm() {
    let limit = dec("10.050").shift10(-3).unwrap();
    assert_eq!(limit, dec("0.010050"));
    assert_eq!(kdec::cmp(0.010_050_000_4, &limit), Ok(Ordering::Greater));
    // The nearest f64 to 0.01005 is 0.01004999999999999962... (exact rationals, fixture).
    let r = kdec::cmp(0.01005, &limit).unwrap();
    assert_ne!(r, Ordering::Equal);
    assert_eq!(r, Ordering::Less);
    // A naive conversion of the limit to f64 would call the two equal.
    assert_eq!(limit.to_f64().unwrap().to_bits(), 0.01005f64.to_bits());
}

/// REQ-003: equal only for the same real number.
#[test]
fn equal_only_when_exact() {
    assert_eq!(kdec::cmp(0.5, &dec("0.50")), Ok(Ordering::Equal));
    assert_eq!(kdec::cmp(0.5, &dec("5e-1")), Ok(Ordering::Equal));
    assert_eq!(kdec::cmp(0.1, &dec("0.1")), Ok(Ordering::Greater));
    assert_eq!(kdec::cmp(-0.1, &dec("-0.1")), Ok(Ordering::Less));
    assert_eq!(kdec::cmp(-0.0, &dec("0")), Ok(Ordering::Equal));
    assert_eq!(kdec::cmp(0.0, &dec("-0.000")), Ok(Ordering::Equal));
    assert_eq!(kdec::cmp(-0.0, &dec("1e-400")), Ok(Ordering::Less));
    assert_eq!(kdec::cmp(f64::from_bits(1), &dec("1e-400")), Ok(Ordering::Greater));
    assert_eq!(kdec::cmp(f64::MAX, &dec("1e400")), Ok(Ordering::Less));
    assert_eq!(kdec::cmp(-f64::MAX, &dec("-1e400")), Ok(Ordering::Greater));
}

/// REQ-002: non-finite x is a typed error.
#[test]
fn non_finite_is_an_error() {
    let d = dec("1");
    assert_eq!(kdec::cmp(f64::NAN, &d), Err(NumError::NotANumber { op: "kdec.cmp", index: None }));
    assert_eq!(kdec::cmp(f64::INFINITY, &d), Err(NumError::Infinite { op: "kdec.cmp", index: None }));
    assert_eq!(kdec::cmp(f64::NEG_INFINITY, &d), Err(NumError::Infinite { op: "kdec.cmp", index: None }));
}

/// AC-3: correctly rounded conversion, bit-equal to the offline value and to `str::parse::<f64>`.
#[test]
fn ac3_to_f64_correctly_rounded() {
    let rows = records("f64");
    assert!(rows.len() >= 10_000, "fixture too small: {}", rows.len());
    for f in &rows {
        let want = bits(&f[2]);
        let got = dec(&f[1]).to_f64().unwrap().to_bits();
        assert_eq!(got, want, "to_f64({})", f[1]);
        assert_eq!(f[1].parse::<f64>().unwrap().to_bits(), want, "core parse of {}", f[1]);
    }
}

#[test]
fn to_f64_overflow_and_underflow() {
    assert_eq!(dec("1e400").to_f64(), Err(NumError::Infinite { op: "kdec.to_f64", index: None }));
    assert_eq!(dec("-1.8e308").to_f64(), Err(NumError::Infinite { op: "kdec.to_f64", index: None }));
    assert_eq!(dec("1.7976931348623157e308").to_f64(), Ok(f64::MAX));
    assert_eq!(dec("1e-400").to_f64().map(f64::to_bits), Ok(0));
    assert_eq!(dec("-1e-400").to_f64().map(f64::to_bits), Ok(0x8000_0000_0000_0000));
    assert_eq!(dec("0").to_f64().map(f64::to_bits), Ok(0));
}

/// AC-4: the listed invalid inputs give typed parse errors.
#[test]
fn ac4_parse_errors() {
    let cases = [
        ("1e999", KdecError::ExponentRange),
        ("--1", KdecError::Syntax),
        ("0x10", KdecError::Syntax),
        ("", KdecError::Syntax),
        ("NaN", KdecError::Syntax),
        ("12345678901234567890123456789012345678901", KdecError::TooManyDigits),
    ];
    for (s, k) in cases {
        let e = Decimal::parse(s).expect_err(s);
        assert_eq!(e, NumError::Domain { op: "kdec.parse", detail: k.detail() }, "{s:?}");
        assert_eq!(KdecError::of(&e), Some(k));
    }
    assert_eq!(KdecError::of(&NumError::EmptyInput { op: "x" }), None);
    // A Domain error from another module with the same detail is not a kdec error.
    assert_eq!(KdecError::of(&NumError::Domain { op: "card.parse", detail: "syntax" }), None);
    assert_eq!(
        KdecError::of(&NumError::Domain { op: "kdec.cmp", detail: "big_integer_capacity" }),
        Some(KdecError::Capacity)
    );
}

/// REQ-001: the parser rules, case by case from the fixture.
#[test]
fn parse_rules() {
    let rows = records("parse");
    assert!(rows.len() >= 300);
    for f in &rows {
        check_result(&format!("parse({:?})", f[1]), Decimal::parse(&f[1]), &f[2..]);
    }
}

/// REQ-004: exact unit shifts and products.
#[test]
fn shift_and_mul_exact() {
    for f in &records("shift") {
        let k: i32 = f[2].parse().unwrap();
        check_result(&format!("shift10({}, {k})", f[1]), dec(&f[1]).shift10(k), &f[3..]);
    }
    for f in &records("mul") {
        check_result(&format!("mul({}, {})", f[1], f[2]), dec(&f[1]).mul(&dec(&f[2])), &f[3..]);
    }
    // 1 inch in metres, exactly.
    let inch = dec("1").mul(&dec("25.4")).unwrap().shift10(-3).unwrap();
    assert_eq!(inch, dec("0.0254"));
    assert_eq!(dec("0.396").mul(&dec("0.0254")).unwrap(), dec("0.0100584"));
}

/// Decimal vs Decimal orderings and `Ord` consistency.
#[test]
fn decimal_orderings() {
    let rows = records("dec");
    assert!(rows.len() >= 2_000);
    for f in &rows {
        let (a, b) = (dec(&f[1]), dec(&f[2]));
        let want = ord(&f[3]);
        assert_eq!(kdec::cmp_dec(&a, &b), want, "{} vs {}", f[1], f[2]);
        assert_eq!(a.cmp(&b), want);
        assert_eq!(kdec::cmp_dec(&b, &a), want.reverse());
        assert_eq!(a == b, want == Ordering::Equal, "{} == {}", f[1], f[2]);
    }
}

#[test]
fn display_is_canonical_and_round_trips() {
    for s in ["10.050", "-0.0254", "0", "1e400", "-1e-400", "340282366920938463463374607431768211455"] {
        let d = dec(s);
        assert_eq!(dec(&d.to_string()), d, "{s}");
    }
    assert_eq!(dec("10.050").to_string(), "1005e-2");
    assert_eq!(dec("-0").to_string(), "0e0");
    assert_eq!("2.5".parse::<Decimal>(), Decimal::new(false, 25, -1));
    assert_eq!(Decimal::new(true, 2500, -3).map(|d| (d.coeff(), d.exp10())), Ok((25, -1)));
    // Zero has no exponent for value operations; parse checks the written exponent.
    assert_eq!(Decimal::new(true, 0, 9999), Ok(Decimal::ZERO));
    assert_eq!(Decimal::ZERO.shift10(9999), Ok(Decimal::ZERO));
    assert!(Decimal::parse("0e401").is_err());
}

/// AC-5: one digest over all fixture outputs. Every output already equals the offline
/// value, so the digest is the same on every host CI runs (x86-64 Linux and Windows,
/// ARM64 macOS); it is printed so hosts can be compared by eye in the logs.
#[test]
fn ac5_output_digest() {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for f in &records("cmp") {
        let d = dec(&f[1]).shift10(f[2].parse().unwrap()).unwrap();
        let r = kdec::cmp(f64::from_bits(bits(&f[3])), &d).unwrap();
        fnv(&mut h, &[r as i8 as u8]);
    }
    for f in &records("f64") {
        fnv(&mut h, &dec(&f[1]).to_f64().unwrap().to_bits().to_le_bytes());
    }
    // The same digest computed from the fixture's expected values.
    let mut want = 0xcbf2_9ce4_8422_2325u64;
    for f in &records("cmp") {
        fnv(&mut want, &[ord(&f[4]) as i8 as u8]);
    }
    for f in &records("f64") {
        fnv(&mut want, &bits(&f[2]).to_le_bytes());
    }
    println!("kdec output digest: {h:016x}");
    assert_eq!(h, want);
}

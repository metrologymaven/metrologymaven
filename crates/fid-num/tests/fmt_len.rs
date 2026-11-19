//! Acceptance criteria of spec node KRN.NUM.FMT.LEN (AC-1 .. AC-8).
//!
//! Expected strings come from `fixtures/num/fmt/make_len_fixtures.py` (exact `fractions`
//! arithmetic, cross-checked against the `decimal` module), never from this crate. CI runs
//! these tests on Linux x86-64, Windows x86-64, macOS ARM64 and wasm32-wasip1 under
//! wasmtime; the output digest is compared with the one the oracle wrote, so a pass on every
//! target shows identical outputs everywhere (AC-7).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use core::cmp::Ordering;

use fid_num::fmt::len::format;
use fid_num::fmt::{FmtError, FormatSpec, FormatterId, Limits, Precision, Reported, Unit};
use fid_num::kdec::{self, Decimal};
use fid_num::NumError;
use fid_testkit::Sha256Hasher;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/num/fmt/");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap()
}

fn hexf(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn unit(s: &str) -> Unit {
    match s {
        "m" => Unit::M,
        "mm" => Unit::Mm,
        "um" => Unit::Um,
        "in" => Unit::In,
        "1" => Unit::One,
        _ => panic!("unit {s}"),
    }
}

fn formatter(s: &str) -> FormatterId {
    match s {
        "length" => FormatterId::LENGTH_V1,
        "scalar" => FormatterId::SCALAR_V1,
        "other" => FormatterId { kind: "other", major: 1 },
        _ => panic!("formatter {s}"),
    }
}

fn precision(s: &str) -> Precision {
    let n: u8 = s[1..].parse().unwrap();
    match &s[..1] {
        "d" => Precision::Decimals(n),
        "s" => Precision::SigDigits(n),
        _ => panic!("precision {s}"),
    }
}

fn limit(s: &str) -> Option<Decimal> {
    (s != "-").then(|| Decimal::parse(s).unwrap())
}

struct Case {
    x: f64,
    spec: FormatSpec,
    expect: String,
}

fn parse_case(fields: &[&str]) -> Case {
    assert_eq!(fields.len(), 7, "{fields:?}");
    let (lower, upper) = (limit(fields[4]), limit(fields[5]));
    let limits = (lower.is_some() || upper.is_some()).then_some(Limits { lower, upper });
    Case {
        x: hexf(fields[0]),
        spec: FormatSpec {
            formatter: formatter(fields[1]),
            unit: unit(fields[2]),
            precision: precision(fields[3]),
            limits,
        },
        expect: fields[6].to_string(),
    }
}

fn show(r: &Result<Reported, NumError>, spec: &FormatSpec) -> String {
    match r {
        Ok(rep) => format!("ok:{}:{}:{}", rep.value, rep.digits_added, u8::from(rep.tie_at_max_precision)),
        Err(e) => {
            let kind = FmtError::of(e).unwrap_or_else(|| panic!("not a formatter error: {e:?}"));
            let NumError::Domain { op, .. } = e else { panic!("{e:?}") };
            let want_op = match spec.formatter {
                f if f == FormatterId::LENGTH_V1 => "fmt.length@1",
                f if f == FormatterId::SCALAR_V1 => "fmt.scalar@1",
                _ => "fmt",
            };
            assert_eq!(*op, want_op);
            format!("err:{}", kind.detail())
        }
    }
}

fn golden() -> (Vec<Case>, String) {
    let text = read("len-golden-v1.txt");
    let mut cases = Vec::new();
    let mut digest = None;
    for line in text.lines() {
        if let Some(d) = line.strip_prefix("# outputs-sha256: ") {
            digest = Some(d.to_string());
        } else if !line.starts_with('#') && !line.is_empty() {
            cases.push(parse_case(&line.split(' ').collect::<Vec<_>>()));
        }
    }
    (cases, digest.expect("digest line"))
}

/// AC-1 and AC-7: every golden string is identical, and the digest of all outputs equals
/// the oracle's.
#[test]
fn ac1_ac7_golden_corpus() {
    let (cases, digest) = golden();
    assert_eq!(cases.len(), 50_000);
    let mut h = Sha256Hasher::new();
    let mut bad = Vec::new();
    for (i, c) in cases.iter().enumerate() {
        let got = show(&format(c.x, &c.spec), &c.spec);
        if got != c.expect && bad.len() < 20 {
            bad.push(format!("case {i}: x={:016x} {:?}\n  got  {got}\n  want {}", c.x.to_bits(), c.spec, c.expect));
        }
        h.update(got.as_bytes());
        h.update(b"\n");
    }
    assert!(bad.is_empty(), "{} mismatches, first:\n{}", bad.len(), bad.join("\n"));
    assert_eq!(h.finish(), digest);
}

/// REQ-008 on every golden output: ASCII digits, at most one `.`, a digit before it,
/// optional leading `-`, and no minus on an all-zero string.
#[test]
fn canonical_form() {
    let (cases, _) = golden();
    for c in &cases {
        let Ok(rep) = format(c.x, &c.spec) else { continue };
        let v = rep.value.as_str();
        let body = v.strip_prefix('-').unwrap_or(v);
        assert!(!body.is_empty() && body.bytes().all(|b| b.is_ascii_digit() || b == b'.'), "{v}");
        assert!(body.bytes().filter(|&b| b == b'.').count() <= 1, "{v}");
        assert!(body.as_bytes()[0].is_ascii_digit() && !body.ends_with('.'), "{v}");
        if v.starts_with('-') {
            assert!(body.bytes().any(|b| (b'1'..=b'9').contains(&b)), "minus on zero: {v}");
        }
        let int = body.split('.').next().unwrap();
        assert!(int == "0" || !int.starts_with('0'), "leading zero: {v}");
    }
}

/// Step 5 of the Design under `kdec::cmp_dec`: wherever the output in metres fits a
/// `Decimal`, a violated limit that is not a tie is shown strictly on its side, and a tie
/// is not.
#[test]
fn side_rule_agrees_with_cmp_dec() {
    let (cases, _) = golden();
    let mut checked = 0;
    for c in &cases {
        let Some(lim) = c.spec.limits else { continue };
        if !matches!(c.spec.unit, Unit::M | Unit::One) {
            continue;
        }
        let Ok(rep) = format(c.x, &c.spec) else { continue };
        let Ok(out) = Decimal::parse(&rep.value) else { continue };
        let side = match (lim.upper, lim.lower) {
            (Some(u), _) if kdec::cmp(c.x, &u).unwrap() == Ordering::Greater => Some((u, Ordering::Greater)),
            (_, Some(l)) if kdec::cmp(c.x, &l).unwrap() == Ordering::Less => Some((l, Ordering::Less)),
            _ => None,
        };
        match side {
            Some((l, want)) => {
                assert_eq!(kdec::cmp_dec(&out, &l) == want, !rep.tie_at_max_precision, "{:?} {}", c.spec, rep.value);
                checked += 1;
            }
            None => assert_eq!((rep.digits_added, rep.tie_at_max_precision), (0, false)),
        }
    }
    assert!(checked > 500, "only {checked} violated-limit cases checked");
}

/// 17 significant digits in m (and for scalars) read back to the same double, bit for
/// bit; `-0.0` reads back as `+0.0` because zero prints unsigned (REQ-008).
#[test]
fn round_trip_17_digits() {
    let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut xs: Vec<f64> = golden().0.iter().map(|c| c.x).collect();
    for _ in 0..5_000 {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        xs.push(f64::from_bits(s));
    }
    xs.extend([f64::MAX, f64::MIN, f64::MIN_POSITIVE, f64::from_bits(1), -f64::from_bits(1), 0.1, -0.0]);
    let mut n = 0;
    for x in xs.into_iter().filter(|x| x.is_finite()) {
        for f in [FormatterId::LENGTH_V1, FormatterId::SCALAR_V1] {
            let u = if f == FormatterId::LENGTH_V1 { Unit::M } else { Unit::One };
            let spec = FormatSpec { formatter: f, unit: u, precision: Precision::SigDigits(17), limits: None };
            let v = format(x, &spec).unwrap().value;
            let back: f64 = v.parse().unwrap();
            let want = if x == 0.0 { 0.0f64.to_bits() } else { x.to_bits() };
            assert_eq!(back.to_bits(), want, "{x:e} -> {v}");
            n += 1;
        }
    }
    assert!(n > 90_000);
}

fn ac_cases() -> Vec<(String, Case)> {
    read("len-ac-v1.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            (f[0].to_string(), parse_case(&f[1..]))
        })
        .collect()
}

fn ac(name: &str) -> (Case, Result<Reported, NumError>) {
    let (_, c) = ac_cases().into_iter().find(|(n, _)| n == name).unwrap();
    let r = format(c.x, &c.spec);
    assert_eq!(show(&r, &c.spec), c.expect, "{name}");
    (c, r)
}

fn dec(s: &str) -> Decimal {
    Decimal::parse(s).unwrap()
}

/// Every named example matches the oracle.
#[test]
fn acceptance_examples_match_oracle() {
    for (name, c) in ac_cases() {
        assert_eq!(show(&format(c.x, &c.spec), &c.spec), c.expect, "{name}");
    }
}

#[test]
fn ac2_exact_conversion() {
    let (_, r) = ac("ac2_inch");
    let v = r.unwrap();
    assert!(v.value == "1.000" || v.value == "1.001", "{}", v.value);
    assert_eq!(v.exact_conversion.factor, "1/0.0254");
    let (_, r) = ac("ac2_mm");
    assert_eq!(r.unwrap().value, "10.000");
}

#[test]
fn ac3_side_rule_adds_digits() {
    let (_, r) = ac("ac3_side");
    let v = r.unwrap();
    assert_ne!(v.value, "0.050");
    assert_eq!(kdec::cmp_dec(&dec(&v.value), &dec("0.050")), Ordering::Greater);
    assert!(v.digits_added >= 1);
    assert!(!v.tie_at_max_precision);
}

#[test]
fn ac4_wrong_side_guard() {
    let (_, r) = ac("ac4_wrong_side");
    let v = r.unwrap();
    assert_ne!(v.value, "0.05");
    assert_eq!(kdec::cmp_dec(&dec(&v.value), &dec("0.0505")), Ordering::Greater);
    assert!(v.digits_added >= 1);
}

#[test]
fn ac5_conforming_value_may_print_as_limit() {
    let (_, r) = ac("ac5_conforming");
    let v = r.unwrap();
    assert_eq!((v.value.as_str(), v.digits_added, v.tie_at_max_precision), ("0.050", 0, false));
}

#[test]
fn ac6_errors() {
    for (name, want) in [
        ("ac6_nan", FmtError::NonFinite),
        ("ac6_inf", FmtError::NonFinite),
        ("ac6_dec25", FmtError::PrecisionOutOfRange),
        ("ac6_sig18", FmtError::PrecisionOutOfRange),
        ("ac6_inverted", FmtError::LimitsInverted),
    ] {
        let (_, r) = ac(name);
        assert_eq!(FmtError::of(&r.unwrap_err()), Some(want), "{name}");
    }
    let spec = |unit, formatter| FormatSpec { formatter, unit, precision: Precision::Decimals(3), limits: None };
    for (u, f) in [
        (Unit::One, FormatterId::LENGTH_V1),
        (Unit::Deg, FormatterId::LENGTH_V1),
        (Unit::Mm, FormatterId::SCALAR_V1),
        (Unit::M, FormatterId { kind: "length", major: 2 }),
    ] {
        let e = format(1.0, &spec(u, f)).unwrap_err();
        assert_eq!(FmtError::of(&e), Some(FmtError::UnitNotSupported), "{u} {f}");
    }
    assert_eq!(FmtError::of(&NumError::Domain { op: "kdec.parse", detail: "syntax" }), None);
    assert_eq!(FmtError::of(&NumError::EmptyInput { op: "fmt.length@1" }), None);
}

#[test]
fn ac8_minus_sign_rule() {
    let (_, r) = ac("ac8_no_minus");
    assert_eq!(r.unwrap().value, "0.000");
    let (_, r) = ac("ac8_lower_zero");
    let v = r.unwrap();
    assert_eq!(v.value, "-0.000000001");
    assert_eq!(v.digits_added, 6);
}

#[test]
fn reported_record_and_ids() {
    assert_eq!(FormatterId::LENGTH_V1.to_string(), "fmt.length@1");
    assert_eq!(FormatterId::SCALAR_V1.to_string(), "fmt.scalar@1");
    for (u, factor) in [(Unit::M, "1"), (Unit::Mm, "1000"), (Unit::Um, "1000000"), (Unit::In, "1/0.0254")] {
        let spec =
            FormatSpec { formatter: FormatterId::LENGTH_V1, unit: u, precision: Precision::SigDigits(3), limits: None };
        let r = format(0.0254, &spec).unwrap();
        assert_eq!((r.unit, r.formatter), (u, FormatterId::LENGTH_V1));
        assert_eq!((r.exact_conversion.from, r.exact_conversion.to, r.exact_conversion.factor), (Unit::M, u, factor));
        assert_eq!((r.digits_added, r.tie_at_max_precision), (0, false));
    }
    let spec = FormatSpec {
        formatter: FormatterId::SCALAR_V1,
        unit: Unit::One,
        precision: Precision::Decimals(0),
        limits: None,
    };
    let r = format(-0.0, &spec).unwrap();
    assert_eq!(r.value, "0");
    assert_eq!((r.exact_conversion.from, r.exact_conversion.to), (Unit::One, Unit::One));
    let r = format(2.5, &spec).unwrap();
    assert_eq!(r.value, "2", "half-even");
}

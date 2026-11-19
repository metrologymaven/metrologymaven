//! Acceptance criteria of spec node KRN.NUM.FMT.ANG (AC-1 .. AC-7).
//!
//! Expected strings come from `fixtures/num/fmt/make_ang_fixtures.py` (pi to 2100 decimals
//! from the Chudnovsky series, exact integer arithmetic, cross-checked against the
//! `decimal` module), never from this crate. CI runs these tests on Linux x86-64, Windows
//! x86-64, macOS ARM64 and wasm32-wasip1 under wasmtime; the output digest is compared with
//! the one the oracle wrote, so a pass on every target shows identical outputs (AC-7).
//! The coarse-ladder cases (`undecidable_rounding`, `undecidable_side`) run as a unit test
//! in `src/fmt/ang.rs`, because the ladder is not a public input.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use core::cmp::Ordering;

use fid_num::fmt::ang::{format, PiEnclosure, PI_LADDER};
use fid_num::fmt::{FmtError, FormatSpec, FormatterId, Limits, Precision, Reported, SideOutcome, Unit};
use fid_num::kdec::{self, Decimal};
use fid_num::NumError;
use fid_testkit::Sha256Hasher;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/num/fmt/");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap()
}

fn unit(s: &str) -> Unit {
    match s {
        "rad" => Unit::Rad,
        "deg" => Unit::Deg,
        "dms" => Unit::Dms,
        "m" => Unit::M,
        "mm" => Unit::Mm,
        "in" => Unit::In,
        "1" => Unit::One,
        _ => panic!("unit {s}"),
    }
}

fn formatter(s: &str) -> FormatterId {
    match s {
        "angle" => FormatterId::ANGLE_V1,
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
        x: f64::from_bits(u64::from_str_radix(fields[0], 16).unwrap()),
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
        Ok(v) => {
            let enc = v.exact_conversion.pi_enclosure.map_or("-".to_string(), |i| i.to_string());
            let tie = u8::from(v.tie_at_max_precision);
            format!("ok:{}:{}:{}:{}:{}", v.value, v.digits_added, tie, v.side.name(), enc)
        }
        Err(e) => {
            let kind = FmtError::of(e).unwrap_or_else(|| panic!("not a formatter error: {e:?}"));
            let NumError::Domain { op, .. } = e else { panic!("{e:?}") };
            let want_op = if spec.formatter == FormatterId::ANGLE_V1 { "fmt.angle@1" } else { "fmt" };
            assert_eq!(*op, want_op);
            format!("err:{}", kind.detail())
        }
    }
}

fn golden() -> (Vec<Case>, String) {
    let text = read("ang-golden-v1.txt");
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

/// AC-1 and AC-7: every golden string is identical (value, digits added, tie flag, side
/// outcome and pi enclosure index), and the digest of all outputs equals the oracle's.
#[test]
fn ac1_ac7_golden_corpus() {
    let (cases, digest) = golden();
    assert_eq!(cases.len(), 20_000);
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

/// The printed string as an exact decimal: the value itself for rad and deg, and for dms
/// the total in seconds, `(D * 3600 + MM * 60) + SS.s...`, parsed field by field; the
/// second element scales the limit to the same unit (1, or 3600 seconds per degree).
fn value_and_limit_scale(v: &str, u: Unit) -> (Decimal, Decimal) {
    if u != Unit::Dms {
        return (Decimal::parse(v).unwrap(), Decimal::parse("1").unwrap());
    }
    let neg = v.starts_with('-');
    let body = v.trim_start_matches('-');
    let mut f = body.split(':');
    let (d, m, s) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
    let (s_int, s_frac) = s.split_once('.').unwrap_or((s, ""));
    assert!(f.next().is_none() && m.len() == 2 && s_int.len() == 2, "{v}");
    let (d, m, s_int): (u128, u128, u128) = (d.parse().unwrap(), m.parse().unwrap(), s_int.parse().unwrap());
    assert!(m < 60 && s_int < 60, "{v}");
    let p = u32::try_from(s_frac.len()).unwrap();
    let frac: u128 = if s_frac.is_empty() { 0 } else { s_frac.parse().unwrap() };
    let total = ((d * 3600 + m * 60 + s_int) * 10u128.pow(p)) + frac;
    (Decimal::new(neg, total, -i32::try_from(p).unwrap()).unwrap(), Decimal::parse("3600").unwrap())
}

/// REQ-005 under `kdec::cmp_dec`, independently of the oracle: wherever the output fits a
/// `Decimal`, a violated limit (as reported) is shown strictly on its side, also at the
/// cap; and nothing is added when no limit is violated. For dms, seconds are compared
/// with `3600 * L`.
#[test]
fn side_rule_agrees_with_cmp_dec() {
    let (cases, _) = golden();
    let mut checked = 0;
    for c in &cases {
        let Ok(rep) = format(c.x, &c.spec) else { continue };
        let Some(lim) = c.spec.limits else {
            assert_eq!(rep.side, SideOutcome::NoLimits);
            continue;
        };
        let (want, l) = match rep.side {
            SideOutcome::AboveUpper => (Ordering::Greater, lim.upper.unwrap()),
            SideOutcome::BelowLower => (Ordering::Less, lim.lower.unwrap()),
            SideOutcome::Within => {
                assert_eq!((rep.digits_added, rep.tie_at_max_precision), (0, false));
                continue;
            }
            s => panic!("{s:?} with the real ladder"),
        };
        if rep.value.len() > 36 {
            continue;
        }
        let (out, scale) = value_and_limit_scale(&rep.value, c.spec.unit);
        let Ok(l) = l.mul(&scale) else { continue };
        assert_eq!(kdec::cmp_dec(&out, &l), want, "{:?} {}", c.spec, rep.value);
        checked += 1;
    }
    assert!(checked > 2000, "only {checked} violated-limit cases checked");
}

/// REQ-008 / Design: canonical ASCII forms. deg and rad as fmt.length@1; dms as
/// `[-]D:MM:SS[.s...]` with no leading zeros on D, and no minus on an all-zero output.
#[test]
fn canonical_form() {
    let (cases, _) = golden();
    for c in &cases {
        let Ok(rep) = format(c.x, &c.spec) else { continue };
        let v = rep.value.as_str();
        let body = v.strip_prefix('-').unwrap_or(v);
        if v.starts_with('-') {
            assert!(body.bytes().any(|b| (b'1'..=b'9').contains(&b)), "minus on zero: {v}");
        }
        if c.spec.unit == Unit::Dms {
            let parts: Vec<&str> = body.split(':').collect();
            assert_eq!(parts.len(), 3, "{v}");
            assert!(parts[0] == "0" || !parts[0].starts_with('0'), "{v}");
            let Precision::Decimals(p) = c.spec.precision else { panic!() };
            let decimals = usize::from(p) + usize::from(rep.digits_added);
            let want_len = 2 + decimals + usize::from(decimals > 0);
            assert_eq!(parts[2].len(), want_len, "{v}");
        } else {
            assert!(body.bytes().all(|b| b.is_ascii_digit() || b == b'.'), "{v}");
            assert!(body.bytes().filter(|&b| b == b'.').count() <= 1, "{v}");
            let int = body.split('.').next().unwrap();
            assert!(int == "0" || !int.starts_with('0'), "leading zero: {v}");
        }
    }
}

fn ac_cases() -> Vec<(String, Case)> {
    read("ang-ac-v1.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            (f[0].to_string(), parse_case(&f[1..]))
        })
        .collect()
}

fn ac(name: &str) -> Result<Reported, NumError> {
    let (_, c) = ac_cases().into_iter().find(|(n, _)| n == name).unwrap();
    let r = format(c.x, &c.spec);
    assert_eq!(show(&r, &c.spec), c.expect, "{name}");
    r
}

#[test]
fn acceptance_examples_match_oracle() {
    let cases = ac_cases();
    assert_eq!(cases.len(), 9);
    for (name, c) in cases {
        assert_eq!(show(&format(c.x, &c.spec), &c.spec), c.expect, "{name}");
    }
}

/// AC-2: pi/4 in deg at 6 decimals and pi/2 in dms at 1 decimal on seconds.
#[test]
fn ac2_exact_angles() {
    assert_eq!(core::f64::consts::FRAC_PI_4.to_bits(), 0x3fe9_21fb_5444_2d18);
    let v = ac("ac2_quarter_pi_deg").unwrap();
    assert_eq!(v.value, "45.000000");
    let c = v.exact_conversion;
    assert_eq!((c.from, c.to, c.factor, c.pi_enclosure), (Unit::Rad, Unit::Deg, "180/pi", Some(0)));
    assert_eq!(v.formatter.to_string(), "fmt.angle@1");
    assert_eq!(ac("ac2_half_pi_dms").unwrap().value, "90:00:00.0");
}

/// AC-3: 10 deg 59 min 59.96 s rounds to 60.0 s, which carries into minutes and degrees.
#[test]
fn ac3_dms_carry() {
    assert_eq!(ac("ac3_carry").unwrap().value, "11:00:00.0");
    assert_eq!(ac("ac3_carry_neg").unwrap().value, "-11:00:00.0");
}

/// AC-4: the next double above pi/6 against an upper limit of 30.000 degrees is printed
/// strictly above it (it is never `30.000` with the side unknown).
#[test]
fn ac4_side_against_degree_limit() {
    let (_, c) = ac_cases().into_iter().find(|(n, _)| n == "ac4_side").unwrap();
    assert_eq!(c.x.to_bits(), core::f64::consts::FRAC_PI_6.to_bits() + 1, "next double above pi/6");
    let v = ac("ac4_side").unwrap();
    match v.side {
        SideOutcome::AboveUpper => {
            assert_ne!(v.value, "30.000");
            let out = Decimal::parse(&v.value).unwrap();
            assert_eq!(kdec::cmp_dec(&out, &Decimal::parse("30").unwrap()), Ordering::Greater);
            assert!(v.digits_added >= 1);
        }
        SideOutcome::Undecidable => {}
        s => panic!("{s:?}"),
    }
    let v = ac("ac4_side_dms").unwrap();
    assert_eq!(v.side, SideOutcome::AboveUpper);
    assert!(v.value.starts_with("30:00:00.") && v.value.bytes().skip(9).any(|b| b != b'0'), "{}", v.value);
}

/// AC-5: the ladder is nested, each gap is exactly 10^-digits, the gaps are at most
/// 10^-50, 10^-200 and 10^-1000, and every rung is the truncation of the oracle's
/// independently computed pi (2100 decimals). Machin's formula is checked in the unit
/// tests of `fmt::ang`.
#[test]
fn ac5_ladder() {
    let text = read("ang-pi-v1.txt");
    let pi = text.lines().find(|l| !l.starts_with('#')).unwrap();
    assert!(pi.starts_with("3.") && pi.len() == 2102);
    assert_eq!(&pi[..52], "3.14159265358979323846264338327950288419716939937510");
    let mut prev: Option<&PiEnclosure> = None;
    for (r, min_digits) in PI_LADDER.iter().zip([50u16, 200, 1000]) {
        let d = usize::from(r.digits);
        assert!(r.digits >= min_digits);
        assert_eq!(r.lo, &pi[..2 + d], "lo of {d} decimals is the truncation of pi");
        assert_eq!(r.hi.len(), r.lo.len());
        // hi = lo + 10^-d: increment the decimal string by one unit in the last place.
        let mut inc = r.lo.as_bytes().to_vec();
        for b in inc.iter_mut().rev() {
            match *b {
                b'9' => *b = b'0',
                b'.' => {}
                _ => {
                    *b += 1;
                    break;
                }
            }
        }
        assert_eq!(r.hi.as_bytes(), inc.as_slice());
        assert!(r.lo < r.hi);
        if let Some(p) = prev {
            // Nested: p.lo <= r.lo and r.hi <= p.hi (same-length prefixes compare as numbers).
            let pd = usize::from(p.digits);
            assert!(&r.lo[..2 + pd] >= p.lo);
            assert!(&r.hi[..2 + pd] <= p.hi);
        }
        prev = Some(r);
    }
}

/// AC-6 and the other typed errors.
#[test]
fn ac6_errors() {
    for name in ["ac6_nan", "ac6_inf", "ac6_neg_inf"] {
        assert_eq!(FmtError::of(&ac(name).unwrap_err()), Some(FmtError::NonFinite), "{name}");
    }
    let spec = |unit, precision| FormatSpec { formatter: FormatterId::ANGLE_V1, unit, precision, limits: None };
    for (u, p) in [
        (Unit::Dms, Precision::SigDigits(5)),
        (Unit::Deg, Precision::Decimals(21)),
        (Unit::Rad, Precision::SigDigits(18)),
    ] {
        let e = format(1.0, &spec(u, p)).unwrap_err();
        assert_eq!(FmtError::of(&e), Some(FmtError::PrecisionOutOfRange), "{u} {p:?}");
    }
    for u in [Unit::M, Unit::One] {
        let e = format(1.0, &spec(u, Precision::Decimals(1))).unwrap_err();
        assert_eq!(FmtError::of(&e), Some(FmtError::UnitNotSupported));
    }
    let mut s = spec(Unit::Deg, Precision::Decimals(1));
    s.formatter = FormatterId::LENGTH_V1;
    let e = format(1.0, &s).unwrap_err();
    assert_eq!(e, NumError::Domain { op: "fmt", detail: "unit_not_supported" });
    assert_eq!(FmtError::UndecidableRounding.detail(), "undecidable_rounding");
    assert_eq!(
        FmtError::of(&NumError::Domain { op: "fmt.angle@1", detail: "undecidable_rounding" }),
        Some(FmtError::UndecidableRounding)
    );
}

/// The rad unit is fmt.scalar@1 with factor 1 and no enclosure; deg and dms name the rung.
#[test]
fn reported_record() {
    let spec = |unit| FormatSpec { formatter: FormatterId::ANGLE_V1, unit, precision: Precision::SigDigits(17), limits: None };
    let r = format(1.0, &spec(Unit::Rad)).unwrap();
    assert_eq!(r.value, "1.0000000000000000");
    assert_eq!(
        (r.exact_conversion.from, r.exact_conversion.to, r.exact_conversion.factor, r.exact_conversion.pi_enclosure),
        (Unit::Rad, Unit::Rad, "1", None)
    );
    assert_eq!(r.side, SideOutcome::NoLimits);
    let r = format(1.0, &spec(Unit::Deg)).unwrap();
    assert_eq!(r.value, "57.295779513082321");
    assert_eq!(r.exact_conversion.pi_enclosure, Some(0));
    // The largest double needs the 1000-decimal rung to print its 310 integer digits.
    let mut s = spec(Unit::Deg);
    s.precision = Precision::Decimals(0);
    let r = format(f64::MAX, &s).unwrap();
    assert_eq!(r.value.len(), 311);
    assert_eq!(r.exact_conversion.pi_enclosure, Some(2));
    // Zero against a zero limit is on it.
    let mut s = spec(Unit::Dms);
    s.precision = Precision::Decimals(0);
    s.limits = Some(Limits { lower: Some(Decimal::ZERO), upper: Some(Decimal::ZERO) });
    let r = format(-0.0, &s).unwrap();
    assert_eq!((r.value.as_str(), r.side), ("0:00:00", SideOutcome::Within));
}

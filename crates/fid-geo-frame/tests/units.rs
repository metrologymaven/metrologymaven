//! Acceptance criteria of spec node GEO.FRAME.UNITS (AC-1 to AC-4) plus requirement checks.
//! Expected bits and strings come from `fixtures/geo/frame/units_cases.json`, written by the
//! independent Python reference `tools/fixtures/geo_units_cases.py` (stdlib `fractions`);
//! none come from this crate. The AC-2 to AC-4 values below are hand-labelled from the spec.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_frame::units::{format, to_si, MAX_SIG_DIGITS, PI_256};
use fid_geo_frame::{AngleUnit, AuthoredQuantity, LengthUnit, Unit, UnitsError};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/frame/units_cases.json");

const M: Unit = Unit::Length(LengthUnit::M);
const MM: Unit = Unit::Length(LengthUnit::Mm);
const IN: Unit = Unit::Length(LengthUnit::In);

/// The string value of `"key":"..."` in a one-line case (texts never hold `"` or `\`).
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\":");
    let start = line.find(&pat)? + pat.len();
    let start = start + line.get(start..)?.find('"')? + 1;
    let len = line.get(start..)?.find('"')?;
    line.get(start..start + len)
}

/// The integer value of `"key":123`.
fn int_field(line: &str, key: &str) -> Option<u8> {
    let pat = format!("\"{key}\":");
    let start = line.find(&pat)? + pat.len();
    let rest = line.get(start..)?;
    let end = rest.find(|c: char| !c.is_ascii_digit())?;
    rest.get(..end)?.parse().ok()
}

struct Sections {
    pi_hex: String,
    parse: Vec<String>,
    display: Vec<String>,
}

fn load() -> Sections {
    let text = std::fs::read_to_string(FIXTURE).unwrap();
    let mut s = Sections { pi_hex: String::new(), parse: Vec::new(), display: Vec::new() };
    let mut section = "";
    for line in text.lines() {
        let t = line.trim();
        if let Some(v) = field(t, "pi_256_hex") {
            s.pi_hex = v.to_owned();
        } else if t == "\"parse\": [" {
            section = "parse";
        } else if t == "\"display\": [" {
            section = "display";
        } else if t.starts_with('{') && t.len() > 1 {
            match section {
                "parse" => s.parse.push(t.to_owned()),
                "display" => s.display.push(t.to_owned()),
                _ => panic!("case outside a section: {t}"),
            }
        }
    }
    s
}

fn code_of(e: UnitsError) -> &'static str {
    e.code()
}

#[test]
fn fixture_pi_constant_matches() {
    let s = load();
    let ours: String = PI_256.iter().rev().map(|l| format!("{l:08x}")).collect();
    assert_eq!(ours, s.pi_hex);
}

/// AC-1: every authored length and angle converts bit for bit (or fails with the labelled error).
#[test]
fn ac1_fixture_parse_cases_bit_exact() {
    let s = load();
    assert_eq!(s.parse.len(), 2000);
    let mut ok = 0;
    for line in &s.parse {
        let name = field(line, "name").unwrap();
        let text = field(line, "text").unwrap();
        let unit = Unit::from_symbol(field(line, "unit").unwrap()).unwrap();
        let got = AuthoredQuantity::parse(text, unit);
        match (field(line, "bits"), field(line, "error")) {
            (Some(bits), None) => {
                let q = got.unwrap_or_else(|e| panic!("{name}: {text} {unit}: {e}"));
                assert_eq!(q.si_hex(), bits, "{name}: {text} {unit}");
                assert_eq!(q.text(), text, "{name}: REQ-002 text kept");
                assert_eq!(q.unit(), unit);
                ok += 1;
            }
            (None, Some(err)) => {
                assert_eq!(got.map(|q| q.si_hex()).map_err(code_of), Err(err), "{name}: {text} {unit}");
            }
            _ => panic!("{name}: malformed case"),
        }
    }
    assert!(ok > 1900, "only {ok} successful cases");
}

/// AC-2: 1 in is the f64 nearest 0.0254; 0.1 mm is the f64 nearest 1e-4 (one rounding).
#[test]
fn ac2_inch_and_tenth_millimetre() {
    let q = AuthoredQuantity::parse("1", IN).unwrap();
    assert_eq!(q.si(), 0.0254);
    assert_eq!(q.text(), "1");
    let t = AuthoredQuantity::parse("0.1", MM).unwrap();
    assert_eq!(t.si(), 1e-4);
    // Rounding twice (0.9 to f64, then times 0.001 rounded) gives a different double for
    // 0.9 mm; one rounding gives the f64 nearest 9e-4 (hand-checked with exact fractions).
    let n = AuthoredQuantity::parse("0.9", MM).unwrap();
    assert_eq!(n.si(), 9e-4);
    let mut twice = 0.9f64;
    twice *= 0.001;
    assert_ne!(twice.to_bits(), n.si().to_bits());
}

/// Centimetre (maintainer decision, needed by IO.CAD.STEP.HDR): exact factor 1/100, symbol
/// `cm`, one rounding. Values hand-labelled: 1 cm and 10 mm are both the f64 nearest 0.01;
/// 2.54 cm is the f64 nearest 0.0254 (1 in); 0.7 cm is the f64 nearest 0.007, which
/// rounding twice (0.7 to f64, then times 0.01) misses by one ulp (checked with exact
/// decimals).
#[test]
fn centimetre_is_exact_and_round_trips() {
    let cm = Unit::Length(LengthUnit::Cm);
    assert_eq!(LengthUnit::Cm.symbol(), "cm");
    assert_eq!(LengthUnit::Cm.ratio(), (1, 100));
    assert_eq!(cm.to_string(), "cm");
    assert_eq!(Unit::from_symbol("cm"), Some(cm));
    assert_eq!(Unit::from_symbol("CM"), None);
    assert_eq!(LengthUnit::ALL.iter().filter(|u| u.symbol() == "cm").count(), 1);

    let one = AuthoredQuantity::parse("1", cm).unwrap();
    assert_eq!(one.si(), 0.01);
    assert_eq!(one.si().to_bits(), to_si("10", MM).unwrap().to_bits());
    assert_eq!(one.to_record_json(), r#"{"si_hex":"3f847ae147ae147b","text":"1","unit":"cm"}"#);
    assert_eq!(to_si("2.54", cm).unwrap(), to_si("1", IN).unwrap());
    let n = to_si("0.7", cm).unwrap();
    assert_eq!(n, 0.007);
    let mut twice = 0.7f64;
    twice *= 0.01;
    assert_ne!(twice.to_bits(), n.to_bits());
    assert_eq!(to_si("-0", cm).unwrap().to_bits(), 1 << 63);

    assert_eq!(format(0.0254, cm, 2).unwrap(), "2.54");
    assert_eq!(format(-0.0254, cm, 2).unwrap(), "-2.54");
    assert_eq!(format(0.01, cm, 0).unwrap(), "1");
    assert_eq!(format(1.0, cm, 3).unwrap(), "100.000");
    assert_eq!(format(f64::NAN, cm, 2), Err(UnitsError::NonFinite));
}

/// AC-3: typed errors for overflow, NaN, comma decimal and a 41-digit decimal.
#[test]
fn ac3_typed_errors() {
    assert_eq!(to_si("1e400", M), Err(UnitsError::Overflow));
    assert_eq!(to_si("NaN", M), Err(UnitsError::NonFinite));
    assert_eq!(to_si("1,5", M), Err(UnitsError::NotDecimal));
    let d41 = "12345678901234567890123456789012345678901";
    assert_eq!(d41.len() as u32, MAX_SIG_DIGITS + 1);
    assert_eq!(to_si(d41, M), Err(UnitsError::TooManyDigits));
    assert!(to_si(&d41[..40], M).is_ok());
    assert!(AuthoredQuantity::parse("1e400", M).is_err());
}

/// AC-4: the f64 nearest 0.0254 in inches at 6 digits is "1.000000"; the 500 display cases match.
#[test]
fn ac4_display_cases_exact() {
    assert_eq!(format(0.0254, IN, 6).unwrap(), "1.000000");
    let s = load();
    assert_eq!(s.display.len(), 500);
    for line in &s.display {
        let name = field(line, "name").unwrap();
        let bits = u64::from_str_radix(field(line, "bits").unwrap(), 16).unwrap();
        let unit = Unit::from_symbol(field(line, "unit").unwrap()).unwrap();
        let digits = int_field(line, "digits").unwrap();
        let out = field(line, "out").unwrap();
        assert_eq!(format(f64::from_bits(bits), unit, digits).unwrap(), out, "{name}");
    }
}

/// REQ-006: the conversion record is canonical JSON `{si_hex, text, unit}`.
#[test]
fn req6_conversion_record() {
    let q = AuthoredQuantity::parse("10", MM).unwrap();
    assert_eq!(q.to_record_json(), r#"{"si_hex":"3f847ae147ae147b","text":"10","unit":"mm"}"#);
    let z = AuthoredQuantity::parse("-0", MM).unwrap();
    assert_eq!(z.to_record_json(), r#"{"si_hex":"8000000000000000","text":"-0","unit":"mm"}"#);
    let a = AuthoredQuantity::parse("+90.0", Unit::Angle(AngleUnit::Deg)).unwrap();
    assert_eq!(a.to_record_json(), r#"{"si_hex":"3ff921fb54442d18","text":"+90.0","unit":"deg"}"#);
}

/// REQ-003: angles through the 256-bit constant land on the familiar doubles.
#[test]
fn req3_angle_landmarks() {
    let deg = Unit::Angle(AngleUnit::Deg);
    assert_eq!(to_si("180", deg).unwrap(), core::f64::consts::PI);
    assert_eq!(to_si("200", Unit::Angle(AngleUnit::Gon)).unwrap(), core::f64::consts::PI);
    assert_eq!(to_si("90", deg).unwrap(), core::f64::consts::FRAC_PI_2);
    assert_eq!(to_si("-45", deg).unwrap(), -core::f64::consts::FRAC_PI_4);
    assert_eq!(to_si("2.5", Unit::Angle(AngleUnit::Rad)).unwrap(), 2.5);
}

/// Display and parse are bounded and allocation-light for the extremes.
#[test]
fn extremes_stay_in_capacity() {
    for u in LengthUnit::ALL.map(Unit::Length).into_iter().chain(AngleUnit::ALL.map(Unit::Angle)) {
        for x in [f64::MAX, -f64::MAX, f64::MIN_POSITIVE, f64::from_bits(1), 0.0] {
            assert!(format(x, u, 20).is_ok(), "{x:e} {u}");
        }
        for t in [
            "9999999999999999999999999999999999999999e290",
            "1e-385",
            "0.0000000000000000000000000000000000000001e-345",
            "1e330",
        ] {
            let r = to_si(t, u);
            assert!(matches!(r, Ok(_) | Err(UnitsError::Overflow)), "{t} {u}: {r:?}");
        }
    }
}

//! Acceptance criteria of spec node KRN.NUM.FMT.REG (AC-1 .. AC-5) and the version guard.
//!
//! Expected values come from `fixtures/num/fmt/make_reg_fixtures.py` and, through it, from
//! the LEN and ANG oracles' golden corpora (exact `fractions` arithmetic in the standard
//! library), never from this crate. CI runs these tests on Linux x86-64, Windows x86-64,
//! macOS ARM64 and wasm32-wasip1 under wasmtime; every output is compared with a digest the
//! oracle wrote, so a pass on every target shows identical results everywhere (AC-5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::fmt::{
    self, descriptor, hexbits, lookup, registry, FmtError, FormatSpec, FormatterDescriptor, FormatterId, Limits,
    LookupError, Precision, Reported, SideOutcome, Unit, MAX_ID_LEN,
};
use fid_num::kdec::Decimal;
use fid_num::NumError;
use fid_testkit::Sha256Hasher;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../");

fn read(path: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}{path}")).unwrap()
}

/// Every [`Unit`]; the match fails to compile when a variant is added, so this list and the
/// registry's unit sets are revisited together.
const ALL_UNITS: [Unit; 8] = [Unit::M, Unit::Mm, Unit::Um, Unit::In, Unit::One, Unit::Rad, Unit::Deg, Unit::Dms];

fn _all_units_is_exhaustive(u: Unit) {
    match u {
        Unit::M | Unit::Mm | Unit::Um | Unit::In | Unit::One | Unit::Rad | Unit::Deg | Unit::Dms => {}
    }
}

fn unit(s: &str) -> Unit {
    *ALL_UNITS.iter().find(|u| u.symbol() == s).unwrap_or_else(|| panic!("unit {s}"))
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

fn spec(id: FormatterId, unit: Unit, precision: Precision) -> FormatSpec {
    FormatSpec { formatter: id, unit, precision, limits: None }
}

struct Case {
    x: f64,
    spec: FormatSpec,
}

/// The cases of `d` in its corpus: lines whose formatter column equals its kind, in order.
fn corpus(d: &FormatterDescriptor) -> Vec<Case> {
    let mut cases = Vec::new();
    for line in read(d.corpus).lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(' ').collect();
        assert_eq!(f.len(), 7, "{line}");
        if f[1] != d.kind() {
            continue;
        }
        let (lower, upper) = (limit(f[4]), limit(f[5]));
        let limits = (lower.is_some() || upper.is_some()).then_some(Limits { lower, upper });
        cases.push(Case {
            x: f64::from_bits(u64::from_str_radix(f[0], 16).unwrap()),
            spec: FormatSpec { formatter: d.id, unit: unit(f[2]), precision: precision(f[3]), limits },
        });
    }
    cases
}

/// The guard line of one result, as pinned in `make_reg_fixtures.py`; `Err` when the
/// record names another formatter.
fn guard_line(id: FormatterId, r: &Result<Reported, NumError>) -> Result<String, String> {
    match r {
        Ok(v) => {
            if v.formatter != id {
                return Err(format!("{id}: result names formatter {}", v.formatter));
            }
            let c = &v.exact_conversion;
            let pi = c.pi_enclosure.map_or("-".to_string(), |i| i.to_string());
            let tie = u8::from(v.tie_at_max_precision);
            Ok(format!(
                "ok {} {} {} {} {pi} {} {tie} {}",
                v.value,
                c.from,
                c.to,
                c.factor,
                v.digits_added,
                v.side.name()
            ))
        }
        Err(NumError::Domain { op, detail }) => Ok(format!("err {op} {detail}")),
        Err(e) => Ok(format!("err {e:?}")),
    }
}

/// The version guard (REQ-KRN.NUM.FMT.REG-003): recomputes the digest of `fmt_fn` over the
/// descriptor's frozen corpus and fails, naming the formatter, if it is not the
/// descriptor's `corpus_digest` (the major is the descriptor's, hence unchanged).
fn guard(
    d: &FormatterDescriptor,
    fmt_fn: impl Fn(f64, &FormatSpec) -> Result<Reported, NumError>,
) -> Result<usize, String> {
    let cases = corpus(d);
    let mut h = Sha256Hasher::new();
    for c in &cases {
        h.update(guard_line(d.id, &fmt_fn(c.x, &c.spec))?.as_bytes());
        h.update(b"\n");
    }
    let got = h.finish();
    if got != d.corpus_digest {
        return Err(format!(
            "{}: outputs over {} changed (digest {got}, registry {}) while the major is unchanged; \
             a changed output needs a new major",
            d.id, d.corpus, d.corpus_digest
        ));
    }
    Ok(cases.len())
}

/// The oracle's digest file: `(id, corpus, cases, digest)` per formatter.
fn oracle_digests() -> Vec<(String, String, usize, String)> {
    read("fixtures/num/fmt/reg-digests-v1.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            assert_eq!(f.len(), 4, "{l}");
            (f[0].to_string(), f[1].to_string(), f[2].parse().unwrap(), f[3].to_string())
        })
        .collect()
}

/// AC-1 and REQ-001: the four formatters, sorted by (kind, major), unique, with well-formed
/// non-empty digests; the registry equals the oracle's digest file entry for entry (AC-5:
/// identical registry contents on every target).
#[test]
fn ac1_registry_listing() {
    let reg = registry();
    let ids: Vec<String> = reg.iter().map(|d| d.id.to_string()).collect();
    assert_eq!(ids, ["fmt.angle@1", "fmt.hexbits@1", "fmt.length@1", "fmt.scalar@1"]);
    for w in reg.windows(2) {
        assert!((w[0].kind(), w[0].major()) < (w[1].kind(), w[1].major()), "not sorted or duplicate: {ids:?}");
    }
    for d in reg {
        let hex = d.corpus_digest.strip_prefix("sha256:").unwrap();
        assert_eq!(hex.len(), 64, "{}", d.id);
        assert!(hex.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)), "{}", d.id);
        assert_eq!(d.kind(), d.id.kind);
        assert_eq!(d.major(), d.id.major);
        assert!(!d.units.is_empty());
        assert_eq!(descriptor(d.id).unwrap(), d);
        assert_eq!(lookup(&d.id.to_string()).unwrap(), d);
    }
    let oracle = oracle_digests();
    assert_eq!(oracle.len(), reg.len());
    for (d, (id, corpus, _, digest)) in reg.iter().zip(&oracle) {
        assert_eq!(
            (d.id.to_string().as_str(), d.corpus, d.corpus_digest),
            (id.as_str(), corpus.as_str(), digest.as_str())
        );
    }
}

/// REQ-003: the version guard passes for every registered formatter on this target, over
/// exactly the oracle's number of cases.
#[test]
fn version_guard_every_formatter() {
    let oracle = oracle_digests();
    for (d, (_, _, n, _)) in registry().iter().zip(&oracle) {
        assert_eq!(guard(d, fmt::format), Ok(*n));
    }
}

/// AC-2: a test build of `fmt.length@1` with deliberately altered rounding (truncation
/// toward zero instead of half-even) and the major unchanged fails the guard, naming
/// `fmt.length@1`. The scalar formatter, which shares the corpus file, is untouched.
#[test]
fn ac2_guard_detects_altered_rounding() {
    let truncating = |x: f64, s: &FormatSpec| {
        let mut wider = *s;
        wider.precision = match s.precision {
            Precision::Decimals(p) if p < 20 => Precision::Decimals(p + 1),
            Precision::SigDigits(n) if n < 17 => Precision::SigDigits(n + 1),
            p => p,
        };
        let changed = wider.precision != s.precision;
        fmt::format(x, &wider).map(|mut r| {
            if changed {
                r.value.pop();
                if r.value.ends_with('.') {
                    r.value.pop();
                }
            }
            r
        })
    };
    let len = descriptor(FormatterId::LENGTH_V1).unwrap();
    let err = guard(len, truncating).unwrap_err();
    assert!(err.starts_with("fmt.length@1: "), "{err}");
    assert!(err.contains("len-golden-v1"), "{err}");
    assert!(guard(len, fmt::format).is_ok());
    assert!(guard(descriptor(FormatterId::SCALAR_V1).unwrap(), fmt::format).is_ok());
}

/// AC-3 and REQ-002: an unknown major of a known kind is refused, naming `fmt.length@9`;
/// no other version is substituted.
#[test]
fn ac3_unknown_formatter() {
    let s = |id| FormatSpec { formatter: id, unit: Unit::Mm, precision: Precision::Decimals(3), limits: None };
    let id = FormatterId { kind: "length", major: 9 };
    let e = fmt::format(0.001, &s(id)).unwrap_err();
    assert_eq!(e, NumError::UnknownFormatter { id });
    assert_eq!(FmtError::of(&e), Some(FmtError::UnknownFormatter));
    assert_eq!(FmtError::UnknownFormatter.detail(), "unknown_formatter");
    assert_eq!(e.to_string(), "fmt: unknown_formatter: fmt.length@9");
    assert_eq!(descriptor(id), Err(NumError::UnknownFormatter { id }));
    for id in [
        FormatterId { kind: "length", major: 0 },
        FormatterId { kind: "length", major: 2 },
        FormatterId { kind: "Length", major: 1 },
        FormatterId { kind: "length ", major: 1 },
        FormatterId { kind: "", major: 1 },
        FormatterId { kind: "angle", major: 2 },
        FormatterId { kind: "hexbits", major: u16::MAX },
        FormatterId { kind: "other", major: 1 },
    ] {
        assert_eq!(fmt::format(0.001, &s(id)), Err(NumError::UnknownFormatter { id }), "{id}");
        // Also for a NaN: the ID is checked before the value.
        assert_eq!(fmt::format(f64::NAN, &s(id)), Err(NumError::UnknownFormatter { id }), "{id}");
    }
    // The corpus line with formatter `other` (fmt::len answers unit_not_supported there):
    // through the registry it is an unknown formatter.
    let other = FormatterId { kind: "other", major: 1 };
    let r = fmt::format(1.0, &FormatSpec { formatter: other, unit: Unit::M, ..s(other) });
    assert_eq!(r, Err(NumError::UnknownFormatter { id: other }));
}

/// REQ-002: `format` looks up the registry descriptor before dispatching, so every
/// success comes from a registered ID (and a unit that entry lists); a dispatch arm with no
/// registry entry cannot succeed. Every registered formatter succeeds at least once.
#[test]
fn every_format_success_has_a_registry_entry() {
    let kinds = ["length", "scalar", "angle", "hexbits", "other", "Length", ""];
    let majors = [0u16, 1, 2, 9, u16::MAX];
    let precisions = [Precision::Decimals(0), Precision::Decimals(3), Precision::SigDigits(1), Precision::SigDigits(6)];
    let mut ids: Vec<FormatterId> = registry().iter().map(|d| d.id).collect();
    for kind in kinds {
        for major in majors {
            ids.push(FormatterId { kind, major });
        }
    }
    let mut ok_per_entry = vec![0usize; registry().len()];
    for id in &ids {
        for u in ALL_UNITS {
            for p in precisions {
                for x in [0.0, -0.0, 0.001, -1.5, 1e300] {
                    let r = fmt::format(x, &spec(*id, u, p));
                    if let Ok(rep) = r {
                        let d = descriptor(*id).unwrap_or_else(|e| panic!("{id}: success without entry: {e}"));
                        assert_eq!(rep.formatter, *id, "{id}");
                        assert!(d.units.contains(&u), "{id} {u:?}: unit not listed");
                        let k = registry().iter().position(|e| e.id == *id).unwrap();
                        ok_per_entry[k] += 1;
                    } else if descriptor(*id).is_err() {
                        assert_eq!(r, Err(NumError::UnknownFormatter { id: *id }), "{id}");
                    }
                }
            }
        }
    }
    for (d, n) in registry().iter().zip(&ok_per_entry) {
        assert!(*n > 0, "{}: no success", d.id);
    }
}

/// Textual lookups: exact `fmt.<kind>@<major>` only; versionless IDs are ambiguous and
/// refused; everything else is malformed or unknown, never resolved to another entry.
#[test]
fn lookup_refuses_unknown_ambiguous_and_malformed() {
    assert_eq!(lookup("fmt.length@1").unwrap().id, FormatterId::LENGTH_V1);
    assert_eq!(lookup("fmt.hexbits@1").unwrap().id, FormatterId::HEXBITS_V1);
    for k in ["length", "angle", "scalar", "hexbits", "nosuch"] {
        let e = lookup(&format!("fmt.{k}")).unwrap_err();
        let LookupError::Ambiguous { kind } = e else { panic!("{k}: {e:?}") };
        assert_eq!(kind.as_str(), k);
        assert_eq!(e.name(), "ambiguous_formatter");
        assert_eq!(e.to_string(), format!("fmt: ambiguous_formatter: fmt.{k} names no major"));
    }
    for t in ["fmt.length@9", "fmt.length@2", "fmt.length@65535", "fmt.nosuch@1", "fmt.a_1@7"] {
        let e = lookup(t).unwrap_err();
        let LookupError::Unknown { id } = e else { panic!("{t}: {e:?}") };
        assert_eq!(id.as_str(), t);
        assert_eq!(format!("{id:?}"), format!("{t:?}"));
        assert_eq!(e.to_string(), format!("fmt: unknown_formatter: {t}"));
    }
    let long = "x".repeat(1 << 20);
    for t in [
        "",
        "fmt",
        "fmt.",
        "fmt.@1",
        "fmt.length@",
        "fmt.length@0",
        "fmt.length@01",
        "fmt.length@+1",
        "fmt.length@-1",
        "fmt.length@1.0",
        "fmt.length@1 ",
        " fmt.length@1",
        "fmt.length@1@1",
        "fmt.length@65536",
        "fmt.length@100000",
        "fmt.Length@1",
        "FMT.length@1",
        "fmt.LENGTH@1",
        "fmt.1length@1",
        "fmt._length@1",
        "fmt.len-gth@1",
        "fmt.len.gth@1",
        "fmt.längth@1",
        "length@1",
        "fmt:length@1",
        "fmt.abcdefghijklmnopq@1",
        "fmt.length@1\0",
        long.as_str(),
    ] {
        assert_eq!(lookup(t), Err(LookupError::Malformed), "{t:?}");
    }
    assert_eq!(LookupError::Malformed.to_string(), "fmt: malformed_formatter_id");
    // The longest well-formed ID fits MAX_ID_LEN and is echoed whole.
    let t = "fmt.abcdefghijklmnop@65535";
    assert_eq!(t.len(), MAX_ID_LEN);
    let LookupError::Unknown { id } = lookup(t).unwrap_err() else { panic!("{t}") };
    assert_eq!(id.as_str(), t);
}

/// The descriptors' unit sets and precision bounds are the ones the implementations accept.
#[test]
fn descriptors_match_implementations() {
    for d in registry() {
        let (max_dec, min_sig, max_sig) = d.precision_bounds;
        let mut sig_seen = false;
        for u in ALL_UNITS {
            let r = fmt::format(1.0, &spec(d.id, u, Precision::Decimals(0)));
            if !d.units.contains(&u) {
                assert_eq!(FmtError::of(&r.unwrap_err()), Some(FmtError::UnitNotSupported), "{} {u}", d.id);
                continue;
            }
            let ok = |p| fmt::format(1.0, &spec(d.id, u, p));
            let range_err = |p| FmtError::of(&ok(p).unwrap_err()) == Some(FmtError::PrecisionOutOfRange);
            let r = r.unwrap();
            assert_eq!((r.formatter, r.unit), (d.id, u), "{} {u}", d.id);
            assert_eq!(r.side, SideOutcome::NoLimits);
            assert!(ok(Precision::Decimals(max_dec)).is_ok(), "{} {u}", d.id);
            assert!(range_err(Precision::Decimals(max_dec + 1)), "{} {u}", d.id);
            assert!(range_err(Precision::SigDigits(max_sig + 1)), "{} {u}", d.id);
            assert!(min_sig == 0 || range_err(Precision::SigDigits(min_sig - 1)), "{} {u}", d.id);
            if min_sig > 0 && ok(Precision::SigDigits(min_sig)).is_ok() {
                assert!(ok(Precision::SigDigits(max_sig)).is_ok(), "{} {u}", d.id);
                sig_seen = true;
            }
        }
        assert_eq!(sig_seen, max_sig > 0, "{}", d.id);
    }
}

/// Dispatch returns the implementation's record unchanged, including `side`,
/// `pi_enclosure`, `digits_added` and `tie_at_max_precision` (#200, #213).
#[test]
fn dispatch_preserves_reported_fields() {
    let lim = |lo: Option<&str>, hi: Option<&str>| {
        Some(Limits { lower: lo.map(|s| Decimal::parse(s).unwrap()), upper: hi.map(|s| Decimal::parse(s).unwrap()) })
    };
    // AC-3 of LEN: the next double above 5e-5 m against an upper limit of 0.050 mm.
    let x = f64::from_bits(5e-5_f64.to_bits() + 1);
    let s = FormatSpec {
        formatter: FormatterId::LENGTH_V1,
        unit: Unit::Mm,
        precision: Precision::Decimals(3),
        limits: lim(None, Some("0.00005")),
    };
    let r = fmt::format(x, &s).unwrap();
    assert_eq!(r, fmt::len::format(x, &s).unwrap());
    assert_eq!(r.side, SideOutcome::AboveUpper);
    assert!(r.digits_added >= 1 && r.value != "0.050", "{r:?}");
    // An angle in degrees: the enclosure index and side come through.
    let s = FormatSpec {
        formatter: FormatterId::ANGLE_V1,
        unit: Unit::Deg,
        precision: Precision::Decimals(3),
        limits: lim(Some("-1"), Some("45")),
    };
    let r = fmt::format(1.0, &s).unwrap();
    assert_eq!(r, fmt::ang::format(1.0, &s).unwrap());
    assert_eq!(r.side, SideOutcome::AboveUpper);
    assert!(r.exact_conversion.pi_enclosure.is_some());
    assert_eq!(r.exact_conversion.factor, "180/pi");
    // Errors are the implementation's too.
    let s = FormatSpec { precision: Precision::Decimals(21), ..s };
    assert_eq!(fmt::format(1.0, &s), fmt::ang::format(1.0, &s));
}

/// AC-4 and REQ-004, -005: hex rendering against CORE.JRN.CANON and the oracle's corpus.
#[test]
fn ac4_hexbits() {
    assert_eq!(hexbits(0.0).unwrap(), "0x0000000000000000");
    assert_eq!(hexbits(-0.0).unwrap(), "0x8000000000000000");
    assert_eq!(hexbits(1.0).unwrap(), "0x3ff0000000000000");
    assert_eq!(hexbits(f64::from_bits(1)).unwrap(), "0x0000000000000001");
    assert_eq!(hexbits(-f64::MAX).unwrap(), "0xffefffffffffffff");
    for x in [f64::NAN, -f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::from_bits(0x7ff0_0000_0000_0001)] {
        let e = hexbits(x).unwrap_err();
        assert_eq!(e, NumError::Domain { op: "fmt.hexbits@1", detail: "non_finite" });
        assert_eq!(FmtError::of(&e), Some(FmtError::NonFinite));
    }
    // Every bit-significant f64 string in the CANON vectors (a JSON string "0x" plus 16
    // lowercase hex digits in a canonical output) round-trips byte for byte. vectors.json
    // publishes no f64 entry today; the scan picks them up when it does.
    let text = read("fixtures/core/jrn/canon/vectors.json");
    let b = text.as_bytes();
    // Every 20-byte window, the last one included (`i` in `0..=len-20`).
    for w in b.windows(20) {
        if w[0] == b'"'
            && &w[1..3] == b"0x"
            && w[19] == b'"'
            && w[3..19].iter().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            let s = core::str::from_utf8(&w[1..19]).unwrap();
            let x = f64::from_bits(u64::from_str_radix(&s[2..], 16).unwrap());
            if x.is_finite() {
                assert_eq!(hexbits(x).unwrap(), s);
            }
        }
    }
    // The oracle's corpus through `hexbits` directly (the guard covers `fmt::format`).
    let d = descriptor(FormatterId::HEXBITS_V1).unwrap();
    let mut n = 0;
    for line in read(d.corpus).lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split(' ').collect();
        let x = f64::from_bits(u64::from_str_radix(f[0], 16).unwrap());
        match f[6].strip_prefix("ok:") {
            Some(ok) => {
                assert_eq!(hexbits(x).unwrap(), ok.split(':').next().unwrap(), "{line}");
                n += 1;
            }
            None if f[6] == "err:non_finite" => assert!(hexbits(x).is_err(), "{line}"),
            None => {}
        }
    }
    assert!(n > 2_500, "{n}");
}

/// `fmt.hexbits@1` through the dispatcher: identity conversion, exact side, pinned errors.
#[test]
fn hexbits_through_format() {
    let s = |unit, precision, lo: Option<&str>, hi: Option<&str>| FormatSpec {
        formatter: FormatterId::HEXBITS_V1,
        unit,
        precision,
        limits: Some(Limits {
            lower: lo.map(|t| Decimal::parse(t).unwrap()),
            upper: hi.map(|t| Decimal::parse(t).unwrap()),
        }),
    };
    let r = fmt::format(-0.0, &s(Unit::Rad, Precision::Decimals(0), Some("0"), Some("0"))).unwrap();
    assert_eq!(r.value, "0x8000000000000000");
    assert_eq!(r.side, SideOutcome::Within);
    assert_eq!(
        (r.exact_conversion.from, r.exact_conversion.to, r.exact_conversion.factor),
        (Unit::Rad, Unit::Rad, "1")
    );
    assert_eq!(r.exact_conversion.pi_enclosure, None);
    assert_eq!((r.digits_added, r.tie_at_max_precision), (0, false));
    let above =
        fmt::format(f64::from_bits(5e-5_f64.to_bits() + 1), &s(Unit::M, Precision::Decimals(0), None, Some("0.00005")));
    assert_eq!(above.unwrap().side, SideOutcome::AboveUpper);
    let below = fmt::format(-1.0, &s(Unit::One, Precision::Decimals(0), Some("-0.5"), Some("-2")));
    assert_eq!(FmtError::of(&below.unwrap_err()), Some(FmtError::LimitsInverted));
    let below = fmt::format(-1.0, &s(Unit::One, Precision::Decimals(0), Some("-0.5"), None)).unwrap();
    assert_eq!(below.side, SideOutcome::BelowLower);
    let none = fmt::format(2.0, &spec(FormatterId::HEXBITS_V1, Unit::M, Precision::Decimals(0))).unwrap();
    assert_eq!(none.side, SideOutcome::NoLimits);
    // Error order: unit, precision, limits, value.
    let e = fmt::format(f64::NAN, &s(Unit::Mm, Precision::SigDigits(3), Some("1"), Some("0"))).unwrap_err();
    assert_eq!(e, NumError::Domain { op: "fmt.hexbits@1", detail: "unit_not_supported" });
    let e = fmt::format(f64::NAN, &s(Unit::M, Precision::SigDigits(3), Some("1"), Some("0"))).unwrap_err();
    assert_eq!(e, NumError::Domain { op: "fmt.hexbits@1", detail: "precision_out_of_range" });
    let e = fmt::format(f64::NAN, &s(Unit::M, Precision::Decimals(0), Some("1"), Some("0"))).unwrap_err();
    assert_eq!(e, NumError::Domain { op: "fmt.hexbits@1", detail: "limits_inverted" });
    let e = fmt::format(f64::NAN, &s(Unit::M, Precision::Decimals(0), None, None)).unwrap_err();
    assert_eq!(e, NumError::Domain { op: "fmt.hexbits@1", detail: "non_finite" });
}

/// Every `FmtError` kind round-trips through its `NumError` carrier.
#[test]
fn fmt_error_kinds_round_trip() {
    for k in [
        FmtError::NonFinite,
        FmtError::PrecisionOutOfRange,
        FmtError::LimitsInverted,
        FmtError::UnitNotSupported,
        FmtError::UndecidableRounding,
        FmtError::InvalidLadder,
        FmtError::Alloc,
        FmtError::Capacity,
    ] {
        let e = NumError::Domain { op: "fmt.length@1", detail: k.detail() };
        assert_eq!(FmtError::of(&e), Some(k));
    }
    let e = NumError::UnknownFormatter { id: FormatterId::LENGTH_V1 };
    assert_eq!(FmtError::of(&e), Some(FmtError::UnknownFormatter));
    assert_eq!(FmtError::of(&NumError::EmptyInput { op: "fmt" }), None);
}

//! Acceptance criteria of spec node UNC.INST.PORT.SPH (AC-1 .. AC-4) plus the rule checks of
//! `fid_kunc::inst::sph`.
//!
//! Expected values come from the independent Python reference
//! `fixtures/unc/inst/port/make_sph_cases.py` (a 50-digit decimal reference that knows
//! nothing about the evaluation order, and a bit model of the documented order built on the
//! Python model of the fid-num libm), never from this crate, or are hand-labelled from the
//! rules in the module documentation (the refusal cases). AC-4 (bit identity on x86-64,
//! ARM64 and wasm32) is the bit comparison with the Python model, run by CI on every target.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kunc::inst::sph::{
    sph_cov, CorrelationCheck, SphCov, SphParams, ALGORITHM_ID, AZ_MAX, DET_TOL, EL_MAX, R_MAX, R_MIN, SCALE_MIN,
};
use fid_kunc::inst::{Cov3, InstError, ValueReason};
use fid_num::NumError;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/inst/port/");

// ------------------------------------------------------------------ a small JSON reader

/// The fixture holds only objects, arrays and strings without escapes.
#[derive(Debug, Clone)]
enum J {
    S(String),
    A(Vec<J>),
    O(Vec<(String, J)>),
}

impl J {
    fn get(&self, key: &str) -> &J {
        match self {
            J::O(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v).unwrap_or_else(|| panic!("missing {key}")),
            _ => panic!("not an object at {key}"),
        }
    }
    fn s(&self) -> &str {
        match self {
            J::S(s) => s,
            _ => panic!("not a string"),
        }
    }
    fn a(&self) -> &[J] {
        match self {
            J::A(v) => v,
            _ => panic!("not an array"),
        }
    }
    fn bits(&self) -> u64 {
        u64::from_str_radix(self.s(), 16).unwrap()
    }
    fn f(&self) -> f64 {
        f64::from_bits(self.bits())
    }
}

fn parse_json(b: &[u8]) -> J {
    fn ws(b: &[u8], i: &mut usize) {
        while *i < b.len() && b[*i].is_ascii_whitespace() {
            *i += 1;
        }
    }
    fn string(b: &[u8], i: &mut usize) -> String {
        assert_eq!(b[*i], b'"');
        let start = *i + 1;
        let end = start + b[start..].iter().position(|&c| c == b'"').unwrap();
        assert!(!b[start..end].contains(&b'\\'));
        *i = end + 1;
        String::from_utf8(b[start..end].to_vec()).unwrap()
    }
    fn value(b: &[u8], i: &mut usize) -> J {
        ws(b, i);
        match b[*i] {
            b'"' => J::S(string(b, i)),
            b'[' => {
                *i += 1;
                let mut v = Vec::new();
                loop {
                    ws(b, i);
                    if b[*i] == b']' {
                        *i += 1;
                        return J::A(v);
                    }
                    v.push(value(b, i));
                    ws(b, i);
                    if b[*i] == b',' {
                        *i += 1;
                    }
                }
            }
            b'{' => {
                *i += 1;
                let mut m = Vec::new();
                loop {
                    ws(b, i);
                    if b[*i] == b'}' {
                        *i += 1;
                        return J::O(m);
                    }
                    let k = string(b, i);
                    ws(b, i);
                    assert_eq!(b[*i], b':');
                    *i += 1;
                    m.push((k, value(b, i)));
                    ws(b, i);
                    if b[*i] == b',' {
                        *i += 1;
                    }
                }
            }
            c => panic!("unexpected byte {c}"),
        }
    }
    let mut i = 0;
    value(b, &mut i)
}

fn fixture() -> J {
    let bytes = std::fs::read(format!("{FIX}sph_cases.json")).unwrap();
    assert!(!bytes.contains(&b'\r'), "sph_cases.json must have LF line endings");
    let doc = parse_json(&bytes);
    assert_eq!(doc.get("algorithm").s(), ALGORITHM_ID);
    assert_eq!(doc.get("seed").s(), "0x535048");
    doc
}

fn params_of(case: &J) -> SphParams {
    let p = case.get("params");
    SphParams {
        sigma_r0: p.get("sigma_r0").f(),
        sigma_r1: p.get("sigma_r1").f(),
        sigma_az: p.get("sigma_az").f(),
        sigma_el: p.get("sigma_el").f(),
        rho_r_az: p.get("rho_r_az").f(),
        rho_r_el: p.get("rho_r_el").f(),
        rho_az_el: p.get("rho_az_el").f(),
    }
}

fn run(case: &J) -> SphCov {
    let o = case.get("obs");
    let name = case.get("name").s();
    sph_cov(&params_of(case), o.get("r").f(), o.get("az").f(), o.get("el").f())
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Relative Frobenius distance of `cov` from the 50-digit reference (each entry rounded once
/// to the nearest double by `str::parse`, which adds at most half an ulp per entry).
fn rel_frobenius(cov: &Cov3, reference: &[J]) -> f64 {
    let rows = cov.rows();
    let mut num = 0.0_f64;
    let mut den = 0.0_f64;
    for (k, r) in reference.iter().enumerate() {
        let want: f64 = r.s().parse().unwrap();
        let got = rows[k / 3][k % 3];
        num += (got - want) * (got - want);
        den += want * want;
    }
    assert!(den > 0.0);
    (num / den).sqrt()
}

/// Every check of one fixture case: predicted bits (AC-4), labels, and accuracy when the
/// case is inside the validated domain (AC-1).
fn check_case(case: &J) -> bool {
    let name = case.get("name").s();
    let out = run(case);
    let want_bits: Vec<u64> = case.get("cov_bits").a().iter().map(J::bits).collect();
    assert_eq!(out.cov().to_bits().to_vec(), want_bits, "{name}: bits");
    assert_eq!(out.sigma_r().to_bits(), case.get("sigma_r").bits(), "{name}: sigma_r");
    assert_eq!(out.correlation().code(), case.get("correlation").s(), "{name}: correlation");
    let notes: Vec<&str> = case.get("notes").a().iter().map(J::s).collect();
    assert_eq!(out.notes().codes(), notes, "{name}: notes");
    assert_eq!(out.notes().is_validated(), notes.is_empty(), "{name}");
    // Exact symmetry and +0 (never -0) everywhere.
    let rows = out.cov().rows();
    for (i, row) in rows.iter().enumerate() {
        for (l, v) in row.iter().enumerate() {
            assert_eq!(v.to_bits(), rows[l][i].to_bits(), "{name}: symmetry");
            assert!(v.to_bits() != (-0.0_f64).to_bits(), "{name}: -0");
        }
    }
    if notes.is_empty() {
        let e = rel_frobenius(out.cov(), case.get("cov_ref").a());
        assert!(e <= 1e-14, "{name}: relative Frobenius error {e:e}");
        true
    } else {
        false
    }
}

// ------------------------------------------------------------------ AC-1 and AC-4

/// AC-1: 200 seeded cases (seed 0x535048), each within 1e-14 relative Frobenius norm of the
/// 50-digit reference. AC-4: every entry equals the bits of the Python model of the
/// documented order (this test runs on x86-64, ARM64 and wasm32 in CI).
#[test]
fn ac1_ac4_seeded_cases() {
    let doc = fixture();
    let cases = doc.get("cases").a();
    assert_eq!(cases.len(), 200);
    let mut checked = 0;
    for case in cases {
        assert!(check_case(case), "{}: a seeded case left the validated domain", case.get("name").s());
        checked += 1;
    }
    assert_eq!(checked, 200);
}

/// Boundary and outside-domain cases: bits and labels everywhere, accuracy where inside.
#[test]
fn edges_and_outside_domain() {
    let doc = fixture();
    let edges = doc.get("edges").a();
    assert_eq!(edges.len(), 16);
    let mut inside = 0;
    for case in edges {
        if check_case(case) {
            inside += 1;
        }
    }
    // r-min, r-max, el +-1.5, az +pi and -2 pi, three correlation edges, zero angle sigmas.
    assert_eq!(inside, 10);
}

// ------------------------------------------------------------------ AC-2

/// AC-2: az = el = 0, r = 10 m, zero correlations: diag(sigma_r², (r sigma_az)²,
/// (r sigma_el)²) within 2 ulp per entry, every off-diagonal entry exactly +0.
#[test]
fn ac2_axis_aligned() {
    let doc = fixture();
    let case = doc.get("axis_aligned");
    assert!(check_case(case));
    let rows = run(case).cov().rows();
    let nearest: Vec<f64> = case.get("diag_nearest").a().iter().map(J::f).collect();
    for (i, (row, want)) in rows.iter().zip(&nearest).enumerate() {
        for (l, got) in row.iter().enumerate() {
            if l == i {
                let ulps = got.to_bits().abs_diff(want.to_bits());
                assert!(ulps <= 2, "diagonal {i}: {ulps} ulp");
            } else {
                assert_eq!(got.to_bits(), 0, "off-diagonal ({i}, {l})");
            }
        }
    }
}

// ------------------------------------------------------------------ AC-3 and refusals

fn base() -> SphParams {
    SphParams {
        sigma_r0: 1.5e-5,
        sigma_r1: 2.5e-6,
        sigma_az: 2.5e-5,
        sigma_el: 2.5e-5,
        rho_r_az: 0.3,
        rho_r_el: -0.2,
        rho_az_el: 0.1,
    }
}

fn refusal(p: SphParams, r: f64, az: f64, el: f64) -> InstError {
    sph_cov(&p, r, az, el).unwrap_err()
}

fn bad_param(name: &'static str, reason: ValueReason) -> InstError {
    InstError::InvalidParameter { name, reason }
}

fn bad_obs(name: &'static str, reason: ValueReason) -> InstError {
    InstError::InvalidObservation { name, reason }
}

/// AC-3: `rho_az_el = 1.2` is `invalid_parameter`.
#[test]
fn ac3_correlation_out_of_range() {
    let p = SphParams { rho_az_el: 1.2, ..base() };
    let e = refusal(p, 5.0, 0.3, 0.1);
    assert_eq!(e, bad_param("rho_az_el", ValueReason::OutOfRange));
    assert_eq!(e.code(), "invalid_parameter");
}

/// Module rule 1 (hand-labelled): non-finite and negative sigmas, non-finite and
/// out-of-range correlations, first failure in the documented order.
#[test]
fn parameter_refusals() {
    use ValueReason::{Negative, NotFinite, OutOfRange};
    let cases: [(SphParams, InstError); 12] = [
        (SphParams { sigma_r0: f64::NAN, ..base() }, bad_param("sigma_r0", NotFinite)),
        (SphParams { sigma_r0: -1e-9, ..base() }, bad_param("sigma_r0", Negative)),
        (SphParams { sigma_r1: f64::INFINITY, ..base() }, bad_param("sigma_r1", NotFinite)),
        (SphParams { sigma_r1: -f64::MIN_POSITIVE, ..base() }, bad_param("sigma_r1", Negative)),
        (SphParams { sigma_az: f64::NEG_INFINITY, ..base() }, bad_param("sigma_az", NotFinite)),
        (SphParams { sigma_el: -5e-324, ..base() }, bad_param("sigma_el", Negative)),
        (SphParams { rho_r_az: f64::NAN, ..base() }, bad_param("rho_r_az", NotFinite)),
        (SphParams { rho_r_el: -1.000_000_000_000_000_2, ..base() }, bad_param("rho_r_el", OutOfRange)),
        (SphParams { rho_az_el: f64::INFINITY, ..base() }, bad_param("rho_az_el", NotFinite)),
        // First failure wins: sigma_r0 before a bad correlation.
        (SphParams { sigma_r0: -1.0, rho_az_el: 2.0, ..base() }, bad_param("sigma_r0", Negative)),
        // Parameters before the observation.
        (SphParams { sigma_el: f64::NAN, ..base() }, bad_param("sigma_el", NotFinite)),
        (SphParams { rho_r_az: 1.5, rho_r_el: 1.5, ..base() }, bad_param("rho_r_az", OutOfRange)),
    ];
    for (p, want) in cases {
        assert_eq!(refusal(p, f64::NAN, 0.3, 0.1), want, "{p:?}");
    }
    // -0.0 sigmas and |rho| = 1 exactly are accepted.
    let p = SphParams { sigma_r0: -0.0, sigma_r1: -0.0, rho_r_az: -1.0, rho_r_el: 0.0, rho_az_el: 0.0, ..base() };
    let out = sph_cov(&p, 5.0, 0.3, 0.1).unwrap();
    assert_eq!(out.sigma_r().to_bits(), 0);
    assert_eq!(out.correlation(), CorrelationCheck::SingularWithinRounding);
}

/// Module rule 2 (hand-labelled): correlations that are pairwise valid but cannot hold at
/// once are refused; the determinant's rounding resolution decides singular versus
/// definite, and both are reported.
#[test]
fn joint_correlation_check() {
    // det = 1 - 3 * 0.81 + 2 * (0.9 * 0.9 * -0.9) = -2.888 + 1 = -1.888.
    let p = SphParams { rho_r_az: 0.9, rho_r_el: 0.9, rho_az_el: -0.9, ..base() };
    assert_eq!(refusal(p, 5.0, 0.3, 0.1), bad_param("correlation", ValueReason::NotPositiveSemidefinite));
    // Equicorrelation -0.5: det = 1 - 0.75 - 0.25 = 0 exactly, singular but valid.
    let p = SphParams { rho_r_az: -0.5, rho_r_el: -0.5, rho_az_el: -0.5, ..base() };
    let out = sph_cov(&p, 5.0, 0.3, 0.1).unwrap();
    assert_eq!(out.correlation(), CorrelationCheck::SingularWithinRounding);
    assert_eq!(out.correlation_det().to_bits(), 0);
    // Slightly beyond: rho = -0.5 - 2^-20 gives det about -4.3e-6 < -DET_TOL.
    let q = -0.5 - 2.0_f64.powi(-20);
    let p = SphParams { rho_r_az: q, rho_r_el: q, rho_az_el: q, ..base() };
    assert_eq!(refusal(p, 5.0, 0.3, 0.1), bad_param("correlation", ValueReason::NotPositiveSemidefinite));
    // Uncorrelated: det = 1, positive definite.
    let p = SphParams::uncorrelated(1e-5, 1e-6, 2e-5, 2e-5);
    let out = sph_cov(&p, 5.0, 0.3, 0.1).unwrap();
    assert_eq!(out.correlation(), CorrelationCheck::PositiveDefinite);
    assert_eq!(out.correlation_det(), 1.0);
    assert_eq!(out.correlation().code(), "positive_definite");
    assert_eq!(CorrelationCheck::SingularWithinRounding.code(), "singular_within_rounding");
}

/// Module rule 3 (hand-labelled): the observation is checked after the parameters.
#[test]
fn observation_refusals() {
    use ValueReason::{Negative, NotFinite};
    let p = base();
    assert_eq!(refusal(p, f64::NAN, 0.3, 0.1), bad_obs("r", NotFinite));
    assert_eq!(refusal(p, f64::INFINITY, f64::NAN, 0.1), bad_obs("r", NotFinite));
    assert_eq!(refusal(p, 5.0, f64::NAN, 0.1), bad_obs("az", NotFinite));
    assert_eq!(refusal(p, 5.0, 0.3, f64::NEG_INFINITY), bad_obs("el", NotFinite));
    assert_eq!(refusal(p, -1e-12, 0.3, 0.1), bad_obs("r", Negative));
    assert_eq!(refusal(p, -1e-12, f64::NAN, 0.1), bad_obs("az", NotFinite));
    assert_eq!(bad_obs("r", Negative).code(), "invalid_observation");
    // r = -0.0 is zero, accepted (outside the validated domain).
    let out = sph_cov(&p, -0.0, 0.3, 0.1).unwrap();
    assert!(out.notes().range_outside());
}

/// Module rule 7: an overflow is `numeric`, never an infinite covariance.
#[test]
fn overflow_is_numeric() {
    let p = SphParams::uncorrelated(1e200, 0.0, 1e-5, 1e-5);
    let e = refusal(p, 5.0, 0.3, 0.1);
    assert_eq!(e.code(), "numeric");
    assert!(matches!(e, InstError::Numeric(NumError::Infinite { .. })), "{e:?}");
    // sigma_r0 + sigma_r1 * r overflowing on its own.
    let p = SphParams::uncorrelated(f64::MAX, f64::MAX, 0.0, 0.0);
    assert_eq!(refusal(p, 2.0, 0.0, 0.0).code(), "numeric");
}

/// The validated-domain notes and their bounds (module section on accuracy).
#[test]
fn domain_notes() {
    let p = base();
    let at = |r: f64, az: f64, el: f64| sph_cov(&p, r, az, el).unwrap();
    assert!(at(R_MIN, 0.0, 0.0).notes().is_validated());
    assert!(at(R_MAX, AZ_MAX, EL_MAX).notes().is_validated());
    assert!(at(R_MAX, -AZ_MAX, -EL_MAX).notes().is_validated());
    let n = at(R_MAX * 1.000_000_1, 7.0, -1.6);
    assert!(n.notes().range_outside() && n.notes().azimuth_outside() && n.notes().elevation_outside());
    assert_eq!(n.notes().codes(), ["range_outside", "elevation_outside", "azimuth_outside"]);
    assert!(at(0.099_999_999, 0.0, 0.0).notes().range_outside());
    let z = sph_cov(&SphParams::uncorrelated(0.0, 0.0, 0.0, 0.0), 5.0, 0.3, 0.1).unwrap();
    assert!(z.notes().zero_covariance() && !z.notes().scale_below_validated());
    assert_eq!(z.cov().to_bits(), [0; 9]);
    let tiny = sph_cov(&SphParams::uncorrelated(1e-160, 0.0, 0.0, 0.0), 5.0, 0.3, 0.1).unwrap();
    assert!(tiny.notes().scale_below_validated() && !tiny.notes().is_validated());
}

#[test]
fn accessors_and_codes() {
    assert_eq!(ALGORITHM_ID, "unc.inst.sph_cov@1.0.0");
    assert_eq!(DET_TOL, 2.0_f64.powi(-47));
    assert_eq!(SCALE_MIN, 2.0_f64.powi(-962));
    assert_eq!((R_MIN, R_MAX, EL_MAX), (0.1, 100.0, 1.5));
    assert_eq!(AZ_MAX, 2.0 * core::f64::consts::PI);
    let out = sph_cov(&base(), 5.0, 0.3, 0.1).unwrap();
    let c = out.cov();
    assert_eq!(c.get(0, 2), Some(c.rows()[0][2]));
    assert_eq!(c.get(3, 0), None);
    assert_eq!(c.get(0, 3), None);
    for (e, code) in [
        (ValueReason::NotFinite, "not_finite"),
        (ValueReason::Negative, "negative"),
        (ValueReason::OutOfRange, "out_of_range"),
        (ValueReason::NotPositiveSemidefinite, "not_positive_semidefinite"),
    ] {
        assert_eq!(e.code(), code);
    }
    assert_eq!(
        bad_param("rho_az_el", ValueReason::OutOfRange).to_string(),
        "invalid_parameter{rho_az_el: out_of_range}"
    );
}

/// The spec card parses (KRN.MAN.CARD schema and standards-text check) and matches the ID.
#[test]
fn spec_card() {
    let card = fid_kci::parse_card(include_str!("../cards/unc.inst.sph_cov@1.0.0.toml")).unwrap();
    let id = fid_kci::AlgorithmId::parse(ALGORITHM_ID).unwrap();
    let report = fid_kci::check(&[id], std::slice::from_ref(&card.id));
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(card.determinism, "D0");
    // `sph_cov` takes four data inputs (parameters, r, az, el); the binding stops compiling
    // if the signature changes.
    let _: fn(&SphParams, f64, f64, f64) -> Result<SphCov, InstError> = sph_cov;
    assert_eq!(card.input_arity, fid_kci::InputArity::exactly(4));
}

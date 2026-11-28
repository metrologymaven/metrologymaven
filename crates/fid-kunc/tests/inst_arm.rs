//! Acceptance criteria of spec node UNC.INST.PORT.ARM (AC-1 .. AC-4) plus the rule checks of
//! `fid_kunc::inst::arm`.
//!
//! Expected values come from the independent Python reference
//! `fixtures/unc/inst/port/make_arm_cases.py` (a 50-digit decimal reference of the spec's
//! formula that knows nothing about the evaluation order, a bit model of the documented
//! order, and exact rational labels of the norm thresholds), never from this crate, or are
//! hand-labelled from the rules in the module documentation (the refusal cases). The bit
//! comparison runs on x86-64, ARM64 and wasm32 in CI.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kunc::inst::arm::{
    arm_cov, ArmCov, ArmParams, ALGORITHM_ID, MIN_DIRECTION_NORM, NORM_ACC_BITS, REL_FROBENIUS_BOUND, SIGMA_MIN,
    UNIT_TOLERANCE,
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
    fn opt(&self, key: &str) -> Option<&J> {
        match self {
            J::O(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => panic!("not an object at {key}"),
        }
    }
    fn get(&self, key: &str) -> &J {
        self.opt(key).unwrap_or_else(|| panic!("missing {key}"))
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
    fn vec3(&self) -> [f64; 3] {
        let v: Vec<f64> = self.a().iter().map(J::f).collect();
        [v[0], v[1], v[2]]
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
    let bytes = std::fs::read(format!("{FIX}arm_cases.json")).unwrap();
    assert!(!bytes.contains(&b'\r'), "arm_cases.json must have LF line endings");
    let doc = parse_json(&bytes);
    assert_eq!(doc.get("algorithm").s(), ALGORITHM_ID);
    assert_eq!(doc.get("seed").s(), "0x41524d");
    doc
}

fn params_of(case: &J) -> ArmParams {
    let p = case.get("params");
    ArmParams {
        sigma_p: p.get("sigma_p").f(),
        sigma_d1: p.get("sigma_d1").f(),
        axial_factor: p.opt("axial_factor").map(J::f),
    }
}

fn run(case: &J) -> ArmCov {
    let name = case.get("name").s();
    arm_cov(&params_of(case), case.get("p").vec3(), case.opt("u").map(J::vec3))
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

/// Every check of one fixture case: predicted bits, labels, and accuracy (AC-1) when no flag
/// limits it. Returns whether the accuracy was checked.
fn check_case(case: &J) -> bool {
    let name = case.get("name").s();
    let out = run(case);
    let want_bits: Vec<u64> = case.get("cov_bits").a().iter().map(J::bits).collect();
    assert_eq!(out.cov().to_bits().to_vec(), want_bits, "{name}: bits");
    assert_eq!(out.sigma_lat().to_bits(), case.get("sigma_lat").bits(), "{name}: sigma_lat");
    assert_eq!(out.sigma_ax().to_bits(), case.get("sigma_ax").bits(), "{name}: sigma_ax");
    assert_eq!(out.axial_factor().to_bits(), case.get("axial_factor_used").bits(), "{name}: axial_factor");
    match (out.direction(), case.opt("direction")) {
        (None, None) => {}
        (Some(got), Some(want)) => {
            let want: Vec<u64> = want.a().iter().map(J::bits).collect();
            assert_eq!(got.map(f64::to_bits).to_vec(), want, "{name}: direction");
        }
        (got, want) => panic!("{name}: direction {got:?} vs {want:?}"),
    }
    let flags: Vec<&str> = case.get("flags").a().iter().map(J::s).collect();
    assert_eq!(out.flags().codes(), flags, "{name}: flags");
    let limited = flags.iter().any(|f| *f == "zero_covariance" || *f == "scale_below_validated");
    assert_eq!(out.flags().is_validated(), !limited, "{name}");
    // Exact symmetry and +0 (never -0) everywhere.
    let rows = out.cov().rows();
    for (i, row) in rows.iter().enumerate() {
        for (l, v) in row.iter().enumerate() {
            assert_eq!(v.to_bits(), rows[l][i].to_bits(), "{name}: symmetry");
            assert!(v.to_bits() != (-0.0_f64).to_bits(), "{name}: -0");
        }
    }
    if limited {
        return false;
    }
    let e = rel_frobenius(out.cov(), case.get("cov_ref").a());
    assert!(e <= 1e-14, "{name}: relative Frobenius error {e:e}");
    true
}

// ------------------------------------------------------------------ AC-1

/// AC-1: 100 seeded cases (seed 0x41524D), each within 1e-14 relative Frobenius norm of the
/// 50-digit reference, and every entry equal to the bits of the Python model of the
/// documented order.
#[test]
fn ac1_seeded_cases() {
    let doc = fixture();
    let cases = doc.get("cases").a();
    assert_eq!(cases.len(), 100);
    let mut checked = 0;
    for case in cases {
        assert!(check_case(case), "{}: a seeded case carries an accuracy flag", case.get("name").s());
        checked += 1;
    }
    assert_eq!(checked, 100);
    // The draw covers isotropic, absent-factor and non-unit directions.
    assert!(cases.iter().any(|c| c.opt("u").is_none()));
    assert!(cases.iter().any(|c| c.get("params").opt("axial_factor").is_none() && c.opt("u").is_some()));
    assert!(cases.iter().any(|c| c.get("flags").a().iter().any(|f| f.s() == "direction_off_unit")));
}

/// Boundary and flagged cases: bits and flags everywhere, accuracy where no flag limits it.
#[test]
fn edges_and_flags() {
    let doc = fixture();
    let edges = doc.get("edges").a();
    assert_eq!(edges.len(), 27);
    let mut checked = 0;
    for case in edges {
        if check_case(case) {
            checked += 1;
        }
    }
    // all-zero-sigma, all-zero-sigma-iso, p-origin-no-d1, tiny-sigma and p-subnormal-norm
    // carry an accuracy flag.
    assert_eq!(checked, 22);
}

// ------------------------------------------------------------------ AC-2

/// AC-2: without a probe direction the result is sigma²·I: every off-diagonal entry exactly
/// +0, the three diagonal entries bit-identical and equal to the correctly rounded square of
/// the returned sigma_lat (0.5 ulp), and for the dyadic cases (exact sigma_lat) within 1 ulp
/// of the exact sigma_lat².
#[test]
fn ac2_isotropic() {
    let doc = fixture();
    let cases = doc.get("isotropic").a();
    assert_eq!(cases.len(), 7);
    let mut dyadic = 0;
    for case in cases {
        let name = case.get("name").s();
        assert!(check_case(case), "{name}");
        let out = run(case);
        assert_eq!(out.direction(), None);
        let rows = out.cov().rows();
        let sl = out.sigma_lat();
        for (i, row) in rows.iter().enumerate() {
            for (l, got) in row.iter().enumerate() {
                if l == i {
                    assert_eq!(got.to_bits(), (sl * sl).to_bits(), "{name}: diagonal {i}");
                } else {
                    assert_eq!(got.to_bits(), 0, "{name}: off-diagonal ({i}, {l})");
                }
            }
        }
        if case.get("exact_sigma").s() == "yes" {
            let exact: f64 = case.get("sigma_lat_sq_exact").s().parse().unwrap();
            let ulps = rows[0][0].to_bits().abs_diff(exact.to_bits());
            assert!(ulps <= 1, "{name}: {ulps} ulp from the exact sigma_lat²");
            dyadic += 1;
        }
    }
    assert_eq!(dyadic, 4);
}

// ------------------------------------------------------------------ AC-3

/// AC-3: u along z (unit, -z, 7z, and with a -0 component), axial_factor 2: the covariance is
/// diagonal (exact +0 off the diagonal), so its eigenvalues are the diagonal entries, each
/// within 1e-15 relative of sigma_lat² (1, 1, 4) from the exact inputs.
#[test]
fn ac3_axial_eigenvalues() {
    let doc = fixture();
    let cases = doc.get("axial").a();
    assert_eq!(cases.len(), 4);
    for case in cases {
        let name = case.get("name").s();
        assert!(check_case(case), "{name}");
        let out = run(case);
        let d = out.direction().unwrap();
        assert_eq!((d[0].to_bits(), d[1].to_bits(), d[2].abs()), (0, 0, 1.0), "{name}");
        let rows = out.cov().rows();
        let eig: Vec<f64> = case.get("eig_ref").a().iter().map(|v| v.s().parse().unwrap()).collect();
        for (i, row) in rows.iter().enumerate() {
            for (l, got) in row.iter().enumerate() {
                if l == i {
                    let rel = ((got - eig[i]) / eig[i]).abs();
                    assert!(rel <= 1e-15, "{name}: eigenvalue {i} off by {rel:e}");
                } else {
                    assert_eq!(got.to_bits(), 0, "{name}: off-diagonal ({i}, {l})");
                }
            }
        }
    }
}

// ------------------------------------------------------------------ AC-4 and refusals

fn base() -> ArmParams {
    ArmParams { sigma_p: 1.5e-5, sigma_d1: 2.5e-6, axial_factor: Some(2.0) }
}

/// Parameters, point, direction and the expected refusal.
type RefusalCase = (ArmParams, [f64; 3], Option<[f64; 3]>, InstError);

/// The signature of `arm_cov`.
type ArmFn = fn(&ArmParams, [f64; 3], Option<[f64; 3]>) -> Result<ArmCov, InstError>;

const P: [f64; 3] = [0.4, 0.1, 0.7];
const U: [f64; 3] = [0.2, -0.3, 0.9];

fn bad_param(name: &'static str, reason: ValueReason) -> InstError {
    InstError::InvalidParameter { name, reason }
}

fn bad_dir(name: &'static str, reason: ValueReason) -> InstError {
    InstError::InvalidDirection { name, reason }
}

/// AC-4: a zero direction is `invalid_direction`; `sigma_p = -1e-6` is `invalid_parameter`.
#[test]
fn ac4_error_paths() {
    let e = arm_cov(&base(), P, Some([0.0, 0.0, 0.0])).unwrap_err();
    assert_eq!(e, bad_dir("u", ValueReason::BelowMinimumNorm));
    assert_eq!(e.code(), "invalid_direction");
    assert_eq!(e.to_string(), "invalid_direction{u: below_minimum_norm}");
    let e = arm_cov(&ArmParams { sigma_p: -1e-6, ..base() }, P, Some(U)).unwrap_err();
    assert_eq!(e, bad_param("sigma_p", ValueReason::Negative));
    assert_eq!(e.code(), "invalid_parameter");
}

/// Module rules 1 to 3 (hand-labelled): first failure in the documented order.
#[test]
fn refusals_in_order() {
    use ValueReason::{BelowMinimumNorm, Missing, Negative, NotFinite};
    let nan = f64::NAN;
    let inf = f64::INFINITY;
    let cases: [RefusalCase; 16] = [
        (ArmParams { sigma_p: nan, ..base() }, P, Some(U), bad_param("sigma_p", NotFinite)),
        (ArmParams { sigma_p: -5e-324, ..base() }, P, Some(U), bad_param("sigma_p", Negative)),
        (ArmParams { sigma_d1: inf, ..base() }, P, Some(U), bad_param("sigma_d1", NotFinite)),
        (ArmParams { sigma_d1: -1e-9, ..base() }, P, Some(U), bad_param("sigma_d1", Negative)),
        (ArmParams { axial_factor: Some(-inf), ..base() }, P, Some(U), bad_param("axial_factor", NotFinite)),
        (ArmParams { axial_factor: Some(-0.5), ..base() }, P, Some(U), bad_param("axial_factor", Negative)),
        // Parameters before the point, the point before the direction.
        (ArmParams { sigma_p: -1.0, axial_factor: Some(nan), ..base() }, P, Some(U), bad_param("sigma_p", Negative)),
        (ArmParams { axial_factor: Some(-1.0), ..base() }, [nan; 3], None, bad_param("axial_factor", Negative)),
        (base(), [0.0, inf, nan], Some([nan; 3]), InstError::InvalidObservation { name: "p_y", reason: NotFinite }),
        (base(), [0.0, 0.0, -inf], None, InstError::InvalidObservation { name: "p_z", reason: NotFinite }),
        (base(), P, Some([0.0, 0.0, nan]), bad_dir("u_z", NotFinite)),
        (base(), P, Some([inf, 0.0, 0.0]), bad_dir("u_x", NotFinite)),
        (base(), P, Some([0.0, 0.0, f64::from_bits(MIN_DIRECTION_NORM.to_bits() - 1)]), bad_dir("u", BelowMinimumNorm)),
        (base(), P, Some([5e-324, -5e-324, 0.0]), bad_dir("u", BelowMinimumNorm)),
        // A declared axial factor other than 1 cannot be applied without a direction.
        (base(), P, None, bad_dir("u", Missing)),
        (ArmParams { axial_factor: Some(0.0), ..base() }, P, None, bad_dir("u", Missing)),
    ];
    for (params, p, u, want) in cases {
        assert_eq!(arm_cov(&params, p, u).unwrap_err(), want, "{params:?} {p:?} {u:?}");
    }
    // -0.0 parameters are zero; an axial factor of exactly 1 (or none) needs no direction.
    let out = arm_cov(&ArmParams { sigma_p: -0.0, sigma_d1: -0.0, axial_factor: Some(1.0) }, P, None).unwrap();
    assert_eq!(out.sigma_lat().to_bits(), 0);
    assert!(out.flags().zero_covariance());
    let out = arm_cov(&ArmParams::isotropic(1e-5, 0.0), P, None).unwrap();
    assert_eq!(out.axial_factor(), 1.0);
    assert_eq!(out.sigma_ax().to_bits(), out.sigma_lat().to_bits());
    // ||u|| = 1e-12 exactly is accepted (the test is strict).
    assert!(arm_cov(&base(), P, Some([0.0, -MIN_DIRECTION_NORM, 0.0])).is_ok());
}

/// Module rule 3 against the oracle: 160 directions on and around ||u|| = 1e-12, 1 - 1e-9
/// and 1 + 1e-9, each labelled by the exact rational comparison in `make_arm_cases.py`.
#[test]
fn direction_thresholds_exact() {
    let doc = fixture();
    let cases = doc.get("direction_cases").a();
    assert_eq!(cases.len(), 160);
    let mut seen = [0_usize; 3];
    for case in cases {
        let u = case.get("u").vec3();
        let got = match arm_cov(&base(), P, Some(u)) {
            Ok(out) => {
                if out.flags().direction_off_unit() {
                    "off_unit"
                } else {
                    "unit"
                }
            }
            Err(e) => {
                assert_eq!(e, bad_dir("u", ValueReason::BelowMinimumNorm), "{u:?}");
                "refused"
            }
        };
        let want = case.get("expect").s();
        assert_eq!(got, want, "{u:?}");
        seen[["refused", "off_unit", "unit"].iter().position(|w| *w == want).unwrap()] += 1;
    }
    assert!(seen.iter().all(|n| *n > 0), "{seen:?}");
}

/// Module rule 7: an overflow is `numeric`, never an infinite covariance.
#[test]
fn overflow_is_numeric() {
    let numeric = |params: ArmParams, p: [f64; 3], u: Option<[f64; 3]>| {
        let e = arm_cov(&params, p, u).unwrap_err();
        assert_eq!(e.code(), "numeric", "{e:?}");
        e
    };
    // The point norm itself overflows (sqrt(3) * MAX).
    let e = numeric(ArmParams::isotropic(1e-5, 0.0), [f64::MAX; 3], None);
    assert!(matches!(e, InstError::Numeric(NumError::Infinite { .. })), "{e:?}");
    // sigma_lat overflows.
    numeric(ArmParams::isotropic(1e-5, f64::MAX), [3.0, 0.0, 0.0], None);
    // sigma_ax overflows.
    numeric(ArmParams { sigma_p: 1e300, sigma_d1: 0.0, axial_factor: Some(1e10) }, P, Some(U));
    // An entry overflows (sigma_lat² > MAX).
    numeric(ArmParams::isotropic(1e200, 0.0), P, None);
    // A huge but finite point norm is fine when sigma_d1 is zero.
    let out = arm_cov(&ArmParams::isotropic(1e-5, 0.0), [1e300, 1e300, 1e300], None).unwrap();
    assert_eq!(out.sigma_lat(), 1e-5);
}

/// The flags and their bounds (module section on accuracy and flags), hand-labelled.
#[test]
fn flags_and_bounds() {
    let at =
        |sigma_p: f64, p: [f64; 3]| arm_cov(&ArmParams { sigma_p, sigma_d1: 0.0, axial_factor: Some(2.0) }, p, Some(U));
    assert!(at(SIGMA_MIN, P).unwrap().flags().is_validated());
    let below = at(SIGMA_MIN / 2.0, P).unwrap();
    assert!(below.flags().scale_below_validated() && !below.flags().is_validated());
    // A subnormal point norm is flagged even when sigma_d1 is zero.
    let sub = at(1e-5, [0.0, f64::MIN_POSITIVE / 2.0, 0.0]).unwrap();
    assert_eq!(sub.flags().codes(), ["direction_off_unit", "scale_below_validated"]);
    assert!(at(1e-5, [0.0, f64::MIN_POSITIVE, 0.0]).unwrap().flags().is_validated());
    // Unit and inside the window 1 +- 1e-9 (the exact edges are oracle cases).
    for u in [[0.0, 1.0, 0.0], [0.6, 0.8, 0.0], [1.0 + 5e-10, 0.0, 0.0], [0.0, 0.0, -(1.0 - 5e-10)]] {
        let out = arm_cov(&base(), P, Some(u)).unwrap();
        assert!(!out.flags().direction_off_unit(), "{u:?}");
        assert!(out.flags().codes().is_empty(), "{u:?}");
    }
    // The axial factor 0: singular along the probe, flagged.
    let z = arm_cov(&ArmParams { axial_factor: Some(0.0), ..base() }, P, Some([0.0, 0.0, 1.0])).unwrap();
    assert_eq!(z.flags().codes(), ["axial_zero"]);
    assert_eq!(z.cov().get(2, 2).map(f64::to_bits), Some(0));
    assert_eq!(z.sigma_ax().to_bits(), 0);
    // Scale-invariant direction: u and 2^600 u give the same bits.
    let a = arm_cov(&base(), P, Some(U)).unwrap();
    let b = arm_cov(&base(), P, Some(U.map(|c| c * 2.0_f64.powi(600)))).unwrap();
    assert_eq!(a.cov().to_bits(), b.cov().to_bits());
    assert_eq!(a.direction(), b.direction());
    assert!(b.flags().direction_off_unit());
}

#[test]
fn accessors_and_codes() {
    assert_eq!(ALGORITHM_ID, "unc.inst.arm_cov@1.0.0");
    assert_eq!(NORM_ACC_BITS, 4352);
    assert_eq!(SIGMA_MIN, 2.0_f64.powi(-480));
    assert_eq!(REL_FROBENIUS_BOUND, 37.0 * 2.0_f64.powi(-53));
    const _: () = assert!(REL_FROBENIUS_BOUND < 1e-14);
    assert_eq!((MIN_DIRECTION_NORM, UNIT_TOLERANCE), (1e-12, 1e-9));
    for (e, code) in [(ValueReason::BelowMinimumNorm, "below_minimum_norm"), (ValueReason::Missing, "missing")] {
        assert_eq!(e.code(), code);
    }
    assert_eq!(bad_dir("u_x", ValueReason::NotFinite).to_string(), "invalid_direction{u_x: not_finite}");
    let out = arm_cov(&base(), P, Some(U)).unwrap();
    let c = out.cov();
    assert_eq!(c.get(1, 2), Some(c.rows()[1][2]));
    assert_eq!(c.get(3, 0), None);
}

/// The spec card parses (KRN.MAN.CARD schema and standards-text check) and matches the ID.
#[test]
fn spec_card() {
    let card = fid_kci::parse_card(include_str!("../cards/unc.inst.arm_cov@1.0.0.toml")).unwrap();
    let id = fid_kci::AlgorithmId::parse(ALGORITHM_ID).unwrap();
    let report = fid_kci::check(&[id], std::slice::from_ref(&card.id));
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(card.determinism, "D0");
    // `arm_cov` takes three data inputs (parameters, point, direction); the binding stops
    // compiling if the signature changes.
    let _: ArmFn = arm_cov;
    assert_eq!(card.input_arity, fid_kci::InputArity::exactly(3));
}

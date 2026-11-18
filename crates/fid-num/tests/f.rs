//! Acceptance criteria of spec node KRN.NUM.FN (AC-1 .. AC-4) and further error paths.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use core::cmp::Ordering;
use fid_num::f::{self, selftest_inputs, selftest_outputs, FnId, SELFTEST_INPUTS_PER_FN};
use fid_num::NumError;
use fid_testkit::Sha256Hasher;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/num/");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap()
}

/// 2^k for any k in [-2044, 2046], applied in two exact steps.
fn scale(x: f64, k: i32) -> f64 {
    let p = |k: i32| f64::from_bits(((k + 1023) as u64) << 52);
    let k1 = k / 2;
    x * p(k1) * p(k - k1)
}

/// AC-1: per function, the inputs and the output bit patterns hash to the golden digests
/// (computed by the independent Python model). CI runs this on Linux x86-64, Windows
/// x86-64, macOS ARM64 and wasm32-wasip1 under wasmtime.
#[test]
fn ac1_selftest_golden_digests() {
    let text = read("fn-selftest-v1.txt");
    let rows: Vec<Vec<&str>> =
        text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).map(|l| l.split(' ').collect()).collect();
    assert_eq!(rows.len(), FnId::ALL.len());
    for (f, row) in FnId::ALL.iter().zip(&rows) {
        assert_eq!(row[0], f.name());
        assert_eq!(row[1].parse::<usize>().unwrap(), f.index());
        let mut hin = Sha256Hasher::new();
        let mut n = 0;
        for (a, b) in selftest_inputs(*f) {
            hin.update(&a.to_bits().to_le_bytes());
            hin.update(&b.to_bits().to_le_bytes());
            n += 1;
        }
        assert_eq!(n, SELFTEST_INPUTS_PER_FN);
        assert_eq!(hin.finish(), row[4], "{}: input generator differs from the fixture definition", f.name());
        let mut hout = Sha256Hasher::new();
        for y in selftest_outputs(*f) {
            hout.update(&y.to_le_bytes());
        }
        assert_eq!(hout.finish(), row[5], "{}: output digest", f.name());
    }
}

/// AC-2: every self-test output is within 1 ulp of the 90-digit oracle; prints the maximum
/// error per function.
#[test]
fn ac2_oracle_within_one_ulp() {
    let text = read("fn-oracle-v1.txt");
    let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        if let Some(name) = line.strip_prefix("@ ") {
            sections.push((name.to_string(), Vec::new()));
        } else {
            sections.last_mut().unwrap().1.push(line);
        }
    }
    assert_eq!(sections.len(), FnId::ALL.len());
    for (f, (name, lines)) in FnId::ALL.iter().zip(&sections) {
        assert_eq!(name, f.name());
        assert_eq!(lines.len(), SELFTEST_INPUTS_PER_FN);
        let mut worst = 0.0f64;
        let mut worst_at = (0.0, 0.0);
        for ((a, b), line) in selftest_inputs(*f).zip(lines) {
            let y = f.eval(a, b).unwrap();
            if *line == "z" {
                assert_eq!(y, 0.0, "{}({a:e}, {b:e}) should be exactly 0", f.name());
                continue;
            }
            let p: Vec<&str> = line.split(' ').collect();
            let r = f64::from_bits(u64::from_str_radix(p[0], 16).unwrap());
            let d: f64 = p[1].parse().unwrap();
            let e: i32 = p[2].parse().unwrap();
            let err = (scale(y - r, -e) - d).abs();
            if err > worst {
                worst = err;
                worst_at = (a, b);
            }
        }
        println!("{:>5}: max error {worst:.4} ulp at ({:e}, {:e})", f.name(), worst_at.0, worst_at.1);
        assert!(worst <= 1.0, "{}: {worst} ulp at {worst_at:?}", f.name());
    }
}

fn is_domain(r: Result<f64, NumError>) -> bool {
    matches!(r, Err(NumError::Domain { .. }))
}

fn is_nan_err(r: Result<f64, NumError>) -> bool {
    matches!(r, Err(NumError::NotANumber { .. }))
}

fn is_inf_err(r: Result<f64, NumError>) -> bool {
    matches!(r, Err(NumError::Infinite { .. }))
}

/// AC-3: the listed out-of-domain and NaN calls return the typed error and no value.
#[test]
fn ac3_error_paths() {
    assert!(matches!(f::asin(1.0000000000000002), Err(NumError::Domain { op: "f.asin", .. })));
    assert!(matches!(f::ln(-0.5), Err(NumError::Domain { op: "f.ln", .. })));
    assert!(matches!(f::sqrt(-1e-300), Err(NumError::Domain { op: "f.sqrt", .. })));
    assert!(matches!(f::atan2(f64::NAN, 1.0), Err(NumError::NotANumber { op: "f.atan2", .. })));
}

/// AC-4: signed zeros by bit pattern in both orders, and NaN operands.
#[test]
fn ac4_total_min_max() {
    let (p, n) = (0.0f64, -0.0f64);
    for (a, b) in [(p, n), (n, p)] {
        assert_eq!(f::total_min(a, b).unwrap().to_bits(), n.to_bits());
        assert_eq!(f::total_max(a, b).unwrap().to_bits(), p.to_bits());
    }
    for (a, b) in [(f64::NAN, 1.0), (1.0, f64::NAN), (f64::NAN, f64::NAN), (-f64::NAN, 0.0)] {
        assert!(is_nan_err(f::total_min(a, b)));
        assert!(is_nan_err(f::total_max(a, b)));
        assert!(matches!(f::total_cmp(a, b), Err(NumError::NotANumber { .. })));
    }
}

#[test]
fn total_order_and_clamp() {
    assert_eq!(f::total_cmp(-0.0, 0.0).unwrap(), Ordering::Less);
    assert_eq!(f::total_cmp(1.0, 1.0).unwrap(), Ordering::Equal);
    assert_eq!(f::total_cmp(f64::INFINITY, f64::MAX).unwrap(), Ordering::Greater);
    assert_eq!(f::total_min(3.0, -2.0).unwrap(), -2.0);
    assert_eq!(f::total_max(3.0, -2.0).unwrap(), 3.0);
    assert_eq!(f::total_max(f64::NEG_INFINITY, -1e308).unwrap(), -1e308);
    assert_eq!(f::total_clamp(5.0, 0.0, 1.0).unwrap(), 1.0);
    assert_eq!(f::total_clamp(-5.0, 0.0, 1.0).unwrap(), 0.0);
    assert_eq!(f::total_clamp(0.5, 0.0, 1.0).unwrap(), 0.5);
    assert_eq!(f::total_clamp(-0.0, 0.0, 1.0).unwrap().to_bits(), 0.0f64.to_bits());
    assert_eq!(f::total_clamp(0.0, -1.0, -0.0).unwrap().to_bits(), (-0.0f64).to_bits());
    assert_eq!(f::total_clamp(2.0, 1.0, 1.0).unwrap(), 1.0);
    assert!(is_domain(f::total_clamp(0.5, 1.0, 0.0)));
    assert!(is_domain(f::total_clamp(0.0, 0.0, -0.0)));
    assert!(is_nan_err(f::total_clamp(f64::NAN, 0.0, 1.0)));
    assert!(is_nan_err(f::total_clamp(0.5, f64::NAN, 1.0)));
    assert!(is_nan_err(f::total_clamp(0.5, 0.0, f64::NAN)));
}

#[test]
fn domain_errors() {
    for x in [1.0000000000000002, -1.0000000000000002, 2.0, -1e300] {
        assert!(is_domain(f::asin(x)), "asin({x})");
        assert!(is_domain(f::acos(x)), "acos({x})");
    }
    for x in [-1e-300, -5e-324, -1.0, -f64::MAX] {
        assert!(is_domain(f::sqrt(x)), "sqrt({x})");
        assert!(is_domain(f::ln(x)), "ln({x})");
    }
    assert!(is_domain(f::ln(0.0)));
    assert!(is_domain(f::ln(-0.0)));
    assert_eq!(f::sqrt(-0.0).unwrap().to_bits(), (-0.0f64).to_bits());
    assert_eq!(f::asin(1.0).unwrap(), core::f64::consts::FRAC_PI_2);
    assert_eq!(f::acos(-1.0).unwrap(), core::f64::consts::PI);
}

#[test]
fn non_finite_inputs_and_overflow() {
    let unary: [fn(f64) -> Result<f64, NumError>; 10] =
        [f::sqrt, f::sin, f::cos, f::tan, f::asin, f::acos, f::atan, f::exp, f::ln, f::cbrt];
    for g in unary {
        assert!(is_nan_err(g(f64::NAN)));
        assert!(is_inf_err(g(f64::INFINITY)));
        assert!(is_inf_err(g(f64::NEG_INFINITY)));
    }
    for g in [f::atan2, f::hypot] {
        assert!(is_nan_err(g(f64::NAN, 1.0)));
        assert!(is_nan_err(g(1.0, f64::NAN)));
        assert!(is_nan_err(g(f64::INFINITY, f64::NAN)), "NaN is reported before infinity");
        assert!(is_inf_err(g(f64::INFINITY, 1.0)));
        assert!(is_inf_err(g(1.0, f64::NEG_INFINITY)));
    }
    assert!(is_inf_err(f::exp(709.79)));
    assert!(f::exp(709.782712893384).unwrap().is_finite());
    assert!(is_inf_err(f::hypot(f64::MAX, f64::MAX)));
    assert_eq!(f::exp(-746.0).unwrap(), 0.0);
}

#[test]
fn exact_values() {
    use core::f64::consts::{FRAC_PI_2, PI};
    assert_eq!(f::sqrt(4.0).unwrap(), 2.0);
    assert_eq!(f::cbrt(-27.0).unwrap(), -3.0);
    assert_eq!(f::exp(0.0).unwrap(), 1.0);
    assert_eq!(f::ln(1.0).unwrap().to_bits(), 0);
    assert_eq!(f::hypot(3.0, -4.0).unwrap(), 5.0);
    assert_eq!(f::sin(-0.0).unwrap().to_bits(), (-0.0f64).to_bits());
    assert_eq!(f::cos(0.0).unwrap(), 1.0);
    assert_eq!(f::atan2(0.0, -1.0).unwrap(), PI);
    assert_eq!(f::atan2(-0.0, -1.0).unwrap(), -PI);
    assert_eq!(f::atan2(-0.0, 1.0).unwrap().to_bits(), (-0.0f64).to_bits());
    assert_eq!(f::atan2(1.0, 0.0).unwrap(), FRAC_PI_2);
    assert_eq!(f::atan(f64::MAX).unwrap(), FRAC_PI_2);
}

#[test]
fn fn_ids() {
    for (i, f) in FnId::ALL.iter().enumerate() {
        assert_eq!(f.index(), i);
    }
    assert!(!f::VENDOR_VERSION.is_empty());
}

//! Acceptance criteria of spec node `UNC.PROP.VALID` (AC-1..AC-5, REQ-UNC.PROP.VALID-001..004).
//!
//! Expected validation outcomes, differences and exact decimal tolerances come from the
//! independent stdlib Python reference `fixtures/unc/valid/make_fixtures.py` (using exact
//! `fractions.Fraction` arithmetic), never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kunc::prop::valid::{delta_for_u, validate_lpu, ValidateError, ValidateInput, ALGORITHM_ID, DEFAULT_N_DIG};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/valid/");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn hex_f64(s: &str) -> f64 {
    let s = s.trim().trim_matches('"');
    f64::from_bits(u64::from_str_radix(s, 16).unwrap_or_else(|e| panic!("hex {s}: {e}")))
}

fn raw<'a>(doc: &'a str, from: usize, key: &str) -> &'a str {
    let pat = format!("\"{key}\": ");
    let at = doc[from..].find(&pat).unwrap_or_else(|| panic!("missing key {key}")) + from + pat.len();
    let rest = &doc[at..];
    let mut depth = 0i32;
    for (i, c) in rest.char_indices() {
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' if depth == 0 => return rest[..i].trim(),
            ']' | '}' => depth -= 1,
            ',' if depth == 0 => return rest[..i].trim(),
            _ => {}
        }
    }
    rest.trim()
}

struct ModelFixture {
    input: ValidateInput,
    validated: bool,
    d_low_bits: u64,
    d_high_bits: u64,
    delta_bits: u64,
    exp10: Option<i32>,
}

fn load_model_fixture(name: &str) -> ModelFixture {
    let doc = read(name);
    let inp_at = doc.find("\"input\":").expect("input section");
    let exp_at = doc.find("\"expected\":").expect("expected section");
    let n_dig: u8 = raw(&doc, inp_at, "n_dig").parse().expect("n_dig");
    let input = ValidateInput {
        y: hex_f64(raw(&doc, inp_at, "y_hex")),
        u_p: hex_f64(raw(&doc, inp_at, "u_p_hex")),
        p_lpu: hex_f64(raw(&doc, inp_at, "p_lpu_hex")),
        y_low: hex_f64(raw(&doc, inp_at, "y_low_hex")),
        y_high: hex_f64(raw(&doc, inp_at, "y_high_hex")),
        p_mcm: hex_f64(raw(&doc, inp_at, "p_mcm_hex")),
        u_mcm: hex_f64(raw(&doc, inp_at, "u_mcm_hex")),
        n_dig,
    };
    let validated = match raw(&doc, exp_at, "validated") {
        "true" => true,
        "false" => false,
        other => panic!("unexpected validated value: {other}"),
    };
    let exp10_raw = raw(&doc, exp_at, "exp10");
    let exp10 = if exp10_raw == "null" { None } else { Some(exp10_raw.parse().expect("exp10")) };
    ModelFixture {
        input,
        validated,
        d_low_bits: hex_f64(raw(&doc, exp_at, "d_low_hex")).to_bits(),
        d_high_bits: hex_f64(raw(&doc, exp_at, "d_high_hex")).to_bits(),
        delta_bits: hex_f64(raw(&doc, exp_at, "delta_hex")).to_bits(),
        exp10,
    }
}

#[test]
fn ac1_linear_sum_validates() {
    assert_eq!(ALGORITHM_ID, "unc.prop.validate_lpu@1.0.0");
    assert_eq!(DEFAULT_N_DIG, 2);

    let f = load_model_fixture("linear_sum.json");
    let out = validate_lpu(&f.input).expect("linear_sum validates without error");
    assert!(f.validated);
    assert_eq!(out.validated, f.validated);
    assert_eq!(out.d_low.to_bits(), f.d_low_bits);
    assert_eq!(out.d_high.to_bits(), f.d_high_bits);
    assert_eq!(out.delta.to_bits(), f.delta_bits);
    assert_eq!(out.exp10, f.exp10);
    assert!(out.d_low <= out.delta && out.d_high <= out.delta);
}

#[test]
fn ac2_square_near_zero_does_not_validate() {
    let f = load_model_fixture("square_near_zero.json");
    let out = validate_lpu(&f.input).expect("square_near_zero runs without error");
    assert!(!f.validated);
    assert_eq!(out.validated, f.validated);
    assert_eq!(out.d_low.to_bits(), f.d_low_bits);
    assert_eq!(out.d_high.to_bits(), f.d_high_bits);
    assert_eq!(out.delta.to_bits(), f.delta_bits);
    assert_eq!(out.exp10, f.exp10);
    assert!(out.d_low > out.delta || out.d_high > out.delta);
}

#[test]
fn ac3_delta_cases_exact() {
    let csv = read("delta_cases.csv");
    let mut rows = 0usize;
    for (idx, line) in csv.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        assert_eq!(cols.len(), 6, "bad row {idx}: {line}");
        let u_mcm = hex_f64(cols[1]);
        let n_dig: u8 = cols[2].parse().expect("n_dig");
        let want_exp10: Option<i32> = if cols[3].is_empty() { None } else { Some(cols[3].parse().expect("exp10")) };
        let want_delta_bits = hex_f64(cols[5]).to_bits();

        let tol = delta_for_u(u_mcm, n_dig).unwrap_or_else(|e| panic!("row {idx} ({line}): {e}"));
        assert_eq!(tol.exp10(), want_exp10, "exp10 mismatch on row {idx}: {line}");
        assert_eq!(tol.delta().to_bits(), want_delta_bits, "delta bits mismatch on row {idx}: {line}");
        rows += 1;
    }
    assert!(rows >= 50, "expected full delta_cases table, got {rows}");
}

#[test]
fn ac4_error_paths_incomparable() {
    let base = load_model_fixture("linear_sum.json").input;

    // AC-4: p_lpu = 0.95 vs p_mcm = 0.99
    let mut bad_p = base;
    bad_p.p_lpu = 0.95;
    bad_p.p_mcm = 0.99;
    let err = validate_lpu(&bad_p).unwrap_err();
    assert_eq!(err.code(), "incomparable");
    assert_eq!(err, ValidateError::Incomparable { reason: "coverage_probability_mismatch" });
    assert_eq!(err.to_string(), "incomparable{coverage_probability_mismatch}");

    // Out-of-range coverage probabilities
    for p in [0.0, 1.0, -0.5, 1.5] {
        let mut x = base;
        x.p_lpu = p;
        x.p_mcm = p;
        assert_eq!(validate_lpu(&x).unwrap_err().code(), "incomparable");
    }

    // Empty or reversed LPU interval (U_p <= 0, including exact X ~ N(0, 1) where U_p = 0)
    for u_p in [0.0, -0.1] {
        let mut x = base;
        x.u_p = u_p;
        assert_eq!(
            validate_lpu(&x).unwrap_err(),
            ValidateError::Incomparable { reason: "empty_or_reversed_lpu_interval" }
        );
    }

    // Empty or reversed MCM interval (y_low >= y_high)
    let mut empty_mcm = base;
    empty_mcm.y_high = empty_mcm.y_low;
    assert_eq!(
        validate_lpu(&empty_mcm).unwrap_err(),
        ValidateError::Incomparable { reason: "empty_or_reversed_mcm_interval" }
    );
    let mut rev_mcm = base;
    rev_mcm.y_high = rev_mcm.y_low - 1.0;
    assert_eq!(
        validate_lpu(&rev_mcm).unwrap_err(),
        ValidateError::Incomparable { reason: "empty_or_reversed_mcm_interval" }
    );

    // Non-finite inputs (NaN / Inf) in every field
    let non_finite_values = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
    for nf in non_finite_values {
        for field in 0..7 {
            let mut x = base;
            match field {
                0 => x.y = nf,
                1 => x.u_p = nf,
                2 => x.p_lpu = nf,
                3 => x.y_low = nf,
                4 => x.y_high = nf,
                5 => x.p_mcm = nf,
                _ => x.u_mcm = nf,
            }
            assert_eq!(validate_lpu(&x).unwrap_err(), ValidateError::Incomparable { reason: "non_finite" });
        }
    }

    // Negative u_mcm or out-of-range n_dig
    let mut neg_u = base;
    neg_u.u_mcm = -1e-3;
    assert_eq!(validate_lpu(&neg_u).unwrap_err(), ValidateError::Incomparable { reason: "negative_u_mcm" });
    for n_dig in [0u8, 18u8, 255u8] {
        let mut bad_n = base;
        bad_n.n_dig = n_dig;
        assert_eq!(validate_lpu(&bad_n).unwrap_err(), ValidateError::Incomparable { reason: "invalid_n_dig" });
    }
}

#[test]
fn req002_exact_boundary_comparison() {
    // Choose exact binary powers so y +/- U_p and differences have zero rounding error.
    // u_mcm = 1.0, n_dig = 1 => l = 0, exact delta = 0.5 (exact in binary64!).
    let at_boundary = ValidateInput::new(10.0, 2.0, 0.95, 7.5, 12.5, 0.95, 1.0).with_n_dig(1);
    let out = validate_lpu(&at_boundary).expect("exact boundary");
    assert_eq!(out.delta, 0.5);
    assert_eq!(out.d_low, 0.5);
    assert_eq!(out.d_high, 0.5);
    assert!(out.validated, "d == delta must validate");

    // 1 ULP above 0.5 on y_low -> d_low > delta -> validated = false
    let just_above_low = ValidateInput::new(10.0, 2.0, 0.95, 7.5 - f64::EPSILON * 4.0, 12.5, 0.95, 1.0).with_n_dig(1);
    let out_low = validate_lpu(&just_above_low).expect("just above on low");
    assert!(!out_low.validated, "d_low > delta must fail validation");

    // 1 ULP above 0.5 on y_high -> d_high > delta -> validated = false
    let just_above_high = ValidateInput::new(10.0, 2.0, 0.95, 7.5, 12.5 + f64::EPSILON * 8.0, 0.95, 1.0).with_n_dig(1);
    let out_high = validate_lpu(&just_above_high).expect("just above on high");
    assert!(!out_high.validated, "d_high > delta must fail validation");

    // Non-dyadic decimal tolerance: u_mcm = 0.1, n_dig = 1 => l = -1, exact delta = 0.05.
    // The binary64 nearest to 0.05 is 0.05000000000000000277555756156289135105907917022705078125,
    // which is strictly GREATER than the exact decimal 1/20. Exact comparison must reject d = f64(0.05)!
    let tol_005 = delta_for_u(0.1, 1).expect("delta for 0.1");
    assert_eq!(tol_005.exp10(), Some(-1));
    assert_eq!(tol_005.delta(), 0.05);
    assert!(!tol_005.within_delta(0.05), "f64(0.05) > exact 1/20 must be rejected");
    assert!(tol_005.within_delta(f64::from_bits(0.05_f64.to_bits() - 1)), "next_down(0.05) < exact 1/20");

    // u_mcm = 0 => delta = 0; only exact zero differences validate
    let zero_match = ValidateInput::new(10.0, 2.0, 0.95, 8.0, 12.0, 0.95, 0.0);
    let out_zm = validate_lpu(&zero_match).expect("zero u match");
    assert!(out_zm.validated);
    assert_eq!(out_zm.delta, 0.0);
    assert_eq!(out_zm.exp10, None);

    let zero_miss = ValidateInput::new(10.0, 2.0, 0.95, 8.0, 12.0 + 1e-12, 0.95, 0.0);
    let out_zmiss = validate_lpu(&zero_miss).expect("zero u miss");
    assert!(!out_zmiss.validated);
}

#[test]
fn ac5_canonical_digest_bit_identical() {
    let f1 = load_model_fixture("linear_sum.json");
    let out1 = validate_lpu(&f1.input).expect("ac1");
    let d1a = out1.digest().expect("digest 1a");
    let d1b = out1.digest().expect("digest 1b");
    assert_eq!(d1a, d1b);

    let f2 = load_model_fixture("square_near_zero.json");
    let out2 = validate_lpu(&f2.input).expect("ac2");
    let d2 = out2.digest().expect("digest 2");
    assert_ne!(d1a, d2);

    // Verify canonical JSON contains exact hex bits matching the independent Python oracle.
    let c1 = String::from_utf8(out1.to_canonical_bytes().expect("bytes 1")).expect("utf8");
    assert!(c1.contains("\"algorithm\":\"unc.prop.validate_lpu@1.0.0\""));
    assert!(c1.contains("\"validated\":\"true\""));
    assert!(c1.contains(&format!("0x{:016x}", f1.delta_bits)));
}

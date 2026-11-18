//! Acceptance criteria of spec node KRN.NUM.ERR (AC-1, AC-2).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::{check_finite, check_finite_slice, probe_fp_env, NumError};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/num/err-special-values-v1.txt");

fn fixture() -> Vec<(String, u64, String)> {
    let text = std::fs::read_to_string(FIXTURE).expect("read fixture");
    let rows: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let bits = u64::from_str_radix(f[1].trim_start_matches("0x"), 16).expect("hex bits");
            (f[0].to_string(), bits, f[2].to_string())
        })
        .collect();
    assert!(rows.len() >= 16, "fixture too small");
    rows
}

/// AC-1: every special value alone and at index 517 of 1 000 normal values.
#[test]
fn ac1_special_values() {
    for (name, bits, expected) in fixture() {
        let x = f64::from_bits(bits);
        let alone = check_finite("test", x);
        let mut xs: Vec<f64> = (0..1000).map(|i| f64::from(i) * 0.25 + 1.0).collect();
        xs[517] = x;
        let slice = check_finite_slice("test", &xs);
        match expected.as_str() {
            "ok" => {
                assert_eq!(alone.map(f64::to_bits), Ok(bits), "{name}: value must pass bit-equal");
                assert_eq!(slice, Ok(()), "{name}");
            }
            "nan" => {
                assert_eq!(alone, Err(NumError::NotANumber { op: "test", index: None }), "{name}");
                assert_eq!(slice, Err(NumError::NotANumber { op: "test", index: Some(517) }), "{name}");
            }
            "inf" => {
                assert_eq!(alone, Err(NumError::Infinite { op: "test", index: None }), "{name}");
                assert_eq!(slice, Err(NumError::Infinite { op: "test", index: Some(517) }), "{name}");
            }
            other => panic!("{name}: unknown expectation {other}"),
        }
    }
}

/// The first offending index wins.
#[test]
fn first_offender_reported() {
    let xs = [1.0, 2.0, f64::INFINITY, f64::NAN];
    assert_eq!(check_finite_slice("op", &xs), Err(NumError::Infinite { op: "op", index: Some(2) }));
    assert_eq!(check_finite_slice("op", &[]), Ok(()));
}

/// AC-2: a normal IEEE 754 environment passes (CI runs x86-64 Linux and Windows, ARM64 macOS).
#[test]
fn ac2_probe_normal_environment() {
    assert_eq!(probe_fp_env(), Ok(()));
}

#[test]
fn display_names_op_and_index() {
    let e = NumError::NotANumber { op: "fit.plane", index: Some(3) };
    assert_eq!(e.to_string(), "fit.plane: NaN at index 3");
    let e = NumError::LengthMismatch { op: "dot", a: 2, b: 3 };
    assert_eq!(e.to_string(), "dot: length mismatch (2 vs 3)");
}

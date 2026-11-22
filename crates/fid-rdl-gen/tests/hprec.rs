//! Acceptance and accuracy tests for `UNC.RDL.GEN.HPREC` (`fid_rdl_gen::hp`).
//! Built and executed on native targets AND `wasm32-wasip1` (via `ci/wasm32-crates.txt`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use fid_canon::{digest as canon_digest, CValue};
use fid_rdl_gen::{
    geometry_rng, hp_refine, hp_uniform, round16, GeneratorRecord, Hp, HpError, ReferenceValue, MAX_REFINE_ITERATIONS,
    PRECISION_BITS,
};
use fid_types::Digest;

const ROUND16_CSV: &str = include_str!("../../../fixtures/rdl/gen/hprec/round16.csv");
const CIRCLE_MP_TXT: &str = include_str!("../../../fixtures/rdl/gen/hprec/circle_mp.txt");
const CARGO_TOML: &str = include_str!("../Cargo.toml");
const CARGO_LOCK: &str = include_str!("../../../Cargo.lock");

/// AC-1: Every one of the 500 values in `fixtures/rdl/gen/hprec/round16.csv` (including exact
/// halfway ties to even/odd and carry-over ties) matches `round16` in both decimal and f64 bits.
#[test]
fn ac1_round16_matches_all_500_fixture_rows() {
    let lines: Vec<&str> = ROUND16_CSV.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 501, "expected header + 500 rows");
    assert_eq!(lines[0], "input,expected_decimal,expected_hex");

    for (idx, line) in lines.iter().skip(1).enumerate() {
        let cols: Vec<&str> = line.split(',').collect();
        assert_eq!(cols.len(), 3, "row {idx}: {line}");
        let hp = Hp::parse_decimal(cols[0]).unwrap_or_else(|e| panic!("row {idx} parse {}: {e}", cols[0]));
        let got = round16(&hp).unwrap_or_else(|e| panic!("row {idx} round16 {}: {e}", cols[0]));
        assert_eq!(got.decimal, cols[1], "row {idx} input={} decimal mismatch", cols[0]);
        assert_eq!(got.hex, cols[2], "row {idx} input={} f64 hex mismatch", cols[0]);
    }
}

/// AC-2: 20 rounded points near a circle refined with `hp_refine` from the pre-rounding
/// solution achieve relative gradient norm < 1e-50 and agree with `circle_mp.txt` to > 50 digits.
#[test]
fn ac2_hp_refine_circle_matches_independent_reference_to_50_digits() {
    let mut kv: BTreeMap<&str, &str> = BTreeMap::new();
    for line in CIRCLE_MP_TXT.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            kv.insert(k, v);
        }
    }

    let p0 = vec![
        Hp::parse_decimal(kv["p0_cx"]).unwrap(),
        Hp::parse_decimal(kv["p0_cy"]).unwrap(),
        Hp::parse_decimal(kv["p0_r"]).unwrap(),
    ];
    let mut pts: Vec<(Hp, Hp)> = Vec::new();
    for i in 0..20 {
        let key = format!("pt_{i}");
        let (xs, ys) = kv[key.as_str()].split_once(',').unwrap();
        let x = Hp::parse_decimal(xs).unwrap();
        let y = Hp::parse_decimal(ys).unwrap();
        // Verify each point is already a round16 fixed point.
        assert_eq!(round16(&x).unwrap().decimal, xs);
        assert_eq!(round16(&y).unwrap().decimal, ys);
        pts.push((x, y));
    }

    let (sol, cert) = hp_refine(
        |p| {
            let (cx, cy, rad) = (p[0], p[1], p[2]);
            let mut r = Vec::with_capacity(pts.len());
            let mut j = Vec::with_capacity(pts.len());
            for &(px, py) in &pts {
                let dx = px - cx;
                let dy = py - cy;
                let dist = (dx * dx + dy * dy).sqrt()?;
                r.push(dist - rad);
                j.push(vec![(-dx / dist)?, (-dy / dist)?, -Hp::ONE]);
            }
            Ok((r, j))
        },
        &p0,
    )
    .expect("circle hp_refine must converge");

    let tol_50 = Hp::parse_decimal("1e-50").unwrap();
    assert!(cert.grad_norm < tol_50, "grad_norm={} must be < 1e-50", cert.grad_norm.to_decimal_sig(12).unwrap());
    assert!(cert.iterations > 0 && cert.iterations <= MAX_REFINE_ITERATIONS);

    let ref_vals = [
        Hp::parse_decimal(kv["ref_cx"]).unwrap(),
        Hp::parse_decimal(kv["ref_cy"]).unwrap(),
        Hp::parse_decimal(kv["ref_r"]).unwrap(),
    ];
    let tol_rel = Hp::parse_decimal("1e-50").unwrap();
    for (i, (&got, &want)) in sol.iter().zip(ref_vals.iter()).enumerate() {
        let rel = ((got - want).abs() / want.abs()).unwrap();
        assert!(
            rel < tol_rel,
            "param {i} mismatch: got={} want={} rel={}",
            got.to_decimal_sig(60).unwrap(),
            want.to_decimal_sig(60).unwrap(),
            rel.to_decimal_sig(12).unwrap()
        );
        // First 50 significant digits match the independent reference verbatim.
        assert_eq!(got.to_decimal_sig(50).unwrap(), want.to_decimal_sig(50).unwrap());
        // Also verify 55-digit ReferenceValue formatting.
        let rv = ReferenceValue::from_hp(&got, 55).unwrap();
        assert!(rv.decimal.len() >= 55);
        assert!(rv.hex.starts_with("0x") && rv.hex.len() == 18);
    }
}

/// AC-3: Same seed produces byte-identical GeneratorRecord, RNG draws, and reference outputs
/// matching pinned golden digests across x86-64, ARM64, and wasm32-wasip1.
#[test]
fn ac3_deterministic_generator_record_and_rng_across_targets() {
    let rec = GeneratorRecord {
        family: "lsnull.circle2d".to_string(),
        method: "nullspace-hp-gn".to_string(),
        tool_version: "0.0.1".to_string(),
        seed: 2026_1122_0001,
        precision_bits: PRECISION_BITS,
        digits: 55,
    };
    let bytes1 = rec.to_canonical_bytes().unwrap();
    let bytes2 = rec.to_canonical_bytes().unwrap();
    assert_eq!(bytes1, bytes2);
    assert_eq!(
        String::from_utf8(bytes1).unwrap(),
        r#"{"digits":55,"family":"lsnull.circle2d","method":"nullspace-hp-gn","precision_bits":256,"seed":"202611220001","tool_version":"0.0.1"}"#
    );
    let d1 = rec.digest().unwrap();
    let d2 = rec.digest().unwrap();
    assert_eq!(d1, d2);
    assert_eq!(
        d1.as_str(),
        "sha256:ef18878445ce376a4c29ccf1e03cb7a7eadf9e74c96103b7bc5db247b04aacfe",
        "rec digest must match golden"
    );
    let rdl_rec = rec.to_rdl_record().unwrap();
    assert_eq!(rdl_rec.id, "lsnull.circle2d:nullspace-hp-gn@p256d55");
    assert_eq!(rdl_rec.seed, "202611220001");

    // Seeded Philox4x32-10 RNG produces identical 55-digit reference values on every target.
    let mut c1 = geometry_rng(rec.seed, 1);
    let mut c2 = geometry_rng(rec.seed, 1);
    let mut out1 = Vec::new();
    let mut out2 = Vec::new();
    for _ in 0..8 {
        let a = hp_uniform(&mut c1);
        let b = hp_uniform(&mut c2);
        let ra = ReferenceValue::from_hp(&(a * Hp::pi()).cos(), 55).unwrap();
        let rb = ReferenceValue::from_hp(&(b * Hp::pi()).cos(), 55).unwrap();
        out1.push(ra);
        out2.push(rb);
    }
    assert_eq!(out1, out2);

    let canonical_outputs = CValue::Arr(
        out1.iter()
            .map(|rv| {
                let mut m = BTreeMap::new();
                m.insert("decimal".to_string(), CValue::Str(rv.decimal.clone()));
                m.insert("hex".to_string(), CValue::Str(rv.hex.clone()));
                CValue::Obj(m)
            })
            .collect(),
    );
    let out_digest = Digest::from_bytes(canon_digest(&canonical_outputs).unwrap().0);
    assert_eq!(
        out_digest.as_str(),
        "sha256:714afda9ff72d5f2721f106aee9365a47b294d0f30507178525301679271ab44",
        "RNG + 55-digit reference output digest must match golden"
    );
}

/// Parses `Cargo.lock` into `package_name -> direct dependency names`.
fn parse_cargo_lock_deps(lock: &str) -> BTreeMap<String, Vec<String>> {
    let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut cur_name: Option<String> = None;
    let mut in_deps = false;

    for raw in lock.lines() {
        let line = raw.trim();
        if line == "[[package]]" {
            cur_name = None;
            in_deps = false;
            continue;
        }
        if let Some(rest) = line.strip_prefix("name = \"") {
            if let Some(name) = rest.strip_suffix('"') {
                cur_name = Some(name.to_string());
                graph.entry(name.to_string()).or_default();
            }
            continue;
        }
        if line == "dependencies = [" {
            in_deps = true;
            continue;
        }
        if in_deps {
            if line == "]" {
                in_deps = false;
                continue;
            }
            let trimmed = line.trim_end_matches(',').trim();
            if let Some(quoted) = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                // Entries are `"pkg"` or `"pkg 0.1.0"` or `"pkg 0.1.0 (registry+...)"`.
                if let Some(dep_name) = quoted.split_whitespace().next() {
                    if let Some(ref pkg) = cur_name {
                        graph.entry(pkg.clone()).or_default().push(dep_name.to_string());
                    }
                }
            }
        }
    }
    graph
}

/// AC-4: Crate direct and transitive dependency graph (from `Cargo.toml` and `Cargo.lock`)
/// contains no kernel fit or solver crate.
#[test]
fn ac4_no_kernel_fit_or_solver_dependencies() {
    let forbidden = ["fid-fit", "fid-krn-fit", "fid-solv", "fid-clarabel", "fid-align-solve"];
    for name in forbidden {
        assert!(!CARGO_TOML.contains(name), "fid-rdl-gen Cargo.toml must not depend on {name}");
    }

    let graph = parse_cargo_lock_deps(CARGO_LOCK);
    assert!(graph.contains_key("fid-rdl-gen"), "fid-rdl-gen must appear in Cargo.lock");

    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    queue.push_back("fid-rdl-gen".to_string());
    while let Some(pkg) = queue.pop_front() {
        if let Some(deps) = graph.get(&pkg) {
            for dep in deps {
                if visited.insert(dep.clone()) {
                    queue.push_back(dep.clone());
                }
            }
        }
    }
    // Sanity check that we actually resolved fid-rdl-gen's workspace deps.
    for expected in ["fid-canon", "fid-num", "fid-rdl", "fid-types"] {
        assert!(visited.contains(expected), "expected {expected} in fid-rdl-gen transitive closure");
    }
    for name in forbidden {
        assert!(!visited.contains(name), "fid-rdl-gen transitive dependency closure must not contain {name}");
    }
}

/// Transcendental accuracy (pi, sqrt, sin, cos, atan, atan2) to > 65 decimal digits,
/// plus error paths (`hp_not_converged`, `hp_singular`, domain errors).
#[test]
fn transcendentals_and_error_paths() {
    let eps = Hp::parse_decimal("1e-65").unwrap();

    // Known 70-digit pi
    let pi_ref = Hp::parse_decimal("3.1415926535897932384626433832795028841971693993751058209749445923078164").unwrap();
    assert!((Hp::pi() - pi_ref).abs() < eps);

    // 4 * atan(1) == pi
    assert!((Hp::ONE.atan().scale_pow2(2) - Hp::pi()).abs() < eps);

    // sqrt(2)^2 == 2
    let s2 = Hp::TWO.sqrt().unwrap();
    assert!((s2 * s2 - Hp::TWO).abs() < eps);

    // sin^2 + cos^2 == 1 and atan2(sin(theta), cos(theta)) == theta across all quadrants
    for num in [-11i64, -5, -2, 1, 3, 7, 13] {
        let theta = (Hp::from_i64(num) * Hp::pi() / Hp::from_i64(17)).unwrap();
        let (s, c) = theta.try_sin_cos().unwrap();
        assert!((s * s + c * c - Hp::ONE).abs() < eps, "sin^2+cos^2 failed for {num}/17 pi");
        let recovered = Hp::atan2(s, c).unwrap();
        assert!((recovered - theta).abs() < eps, "atan2 roundtrip failed for {num}/17 pi");
    }

    // hp_singular on zero Jacobian and on rank-deficient 2x2 Jacobian
    let err = hp_refine(|_| Ok((vec![Hp::ONE], vec![vec![Hp::ZERO]])), &[Hp::ONE]).unwrap_err();
    assert_eq!(err, HpError::Singular);
    assert_eq!(err.code(), "hp_singular");

    let err2 = hp_refine(
        |_| Ok((vec![Hp::ONE, Hp::ZERO], vec![vec![Hp::ONE, Hp::ONE], vec![Hp::ONE, Hp::ONE]])),
        &[Hp::ONE, Hp::ONE],
    )
    .unwrap_err();
    assert_eq!(err2, HpError::Singular);

    // hp_not_converged after 50 iterations
    let p_huge = Hp::parse_decimal("1e100").unwrap();
    let err = hp_refine(|p| Ok((vec![p[0] * p[0] + Hp::ONE], vec![vec![p[0].scale_pow2(1)]])), &[p_huge]).unwrap_err();
    assert_eq!(err.code(), "hp_not_converged");
    assert!(matches!(err, HpError::NotConverged { iterations: 50, .. }));
}

/// Hostile / extreme inputs fail closed immediately (no hang, no silent overflow, no wrap).
#[test]
fn hostile_inputs_fail_closed_fast() {
    // 1. Extreme scale_pow2 produces non-finite sentinel; checked version fails with Domain.
    let huge = Hp::ONE.scale_pow2(i32::MAX);
    assert!(!huge.is_finite());
    assert!(Hp::ONE.try_scale_pow2(i32::MAX).is_err());
    assert!(Hp::ONE.try_scale_pow2(i32::MIN).is_err());
    assert!(huge.to_decimal_sig(16).is_err());
    assert!(round16(&huge).is_err());
    assert!(ReferenceValue::from_hp(&huge, 55).is_err());
    assert!((huge / Hp::TWO).is_err());
    assert!(huge.sqrt().is_err());
    assert!(huge.try_sin_cos().is_err());
    assert!(Hp::atan2(huge, Hp::ONE).is_err());
    assert!(hp_refine(|p| Ok((vec![p[0]], vec![vec![Hp::ONE]])), &[huge]).is_err());

    // 2. Repeated squaring overflows into non-finite sentinel and try_mul rejects it.
    let mut cur = Hp::TWO;
    for _ in 0..25 {
        cur = cur * cur;
    }
    assert!(!cur.is_finite());
    assert!(cur.try_mul(Hp::TWO).is_err());

    // 3. Large finite exponent still within Hp bounds but above decimal conversion bound fails fast.
    let big_finite = Hp::ONE.scale_pow2(100_000);
    assert!(big_finite.is_finite());
    assert!(big_finite.to_decimal_sig(16).is_err());
    assert!(round16(&big_finite).is_err());

    // 4. Large trig argument fails closed instead of silently losing range-reduction precision.
    let big_angle = Hp::ONE.scale_pow2(100);
    assert!(big_angle.is_finite());
    assert!(matches!(big_angle.try_sin_cos(), Err(HpError::Domain(_))));

    // 5. Malformed and extreme decimal strings fail closed immediately.
    let overlong = "1".repeat(3000);
    let bad_inputs = [
        overlong.as_str(),
        "",
        "   ",
        ".",
        "-",
        "+",
        "1..2",
        "1e",
        "1e+",
        "1e-",
        "1e99999999",
        "1e-99999999",
        "1e4001",
        "1e-4001",
        "abc",
        "--1",
        "NaN",
        "inf",
    ];
    for s in bad_inputs {
        assert!(matches!(Hp::parse_decimal(s), Err(HpError::Parse(_))), "expected Parse error for {s:?}");
    }

    // 6. GeneratorRecord enforces digits >= 50, precision_bits >= 200, and non-empty fields.
    let base = GeneratorRecord {
        family: "lsnull.circle2d".to_string(),
        method: "nullspace-hp-gn".to_string(),
        tool_version: "0.0.1".to_string(),
        seed: 42,
        precision_bits: PRECISION_BITS,
        digits: 55,
    };
    assert!(base.validate().is_ok());

    let mut bad_digits = base.clone();
    bad_digits.digits = 49;
    assert!(bad_digits.validate().is_err());
    assert!(bad_digits.to_canonical_bytes().is_err());
    assert!(bad_digits.digest().is_err());
    assert!(bad_digits.to_rdl_record().is_err());

    let mut bad_bits = base.clone();
    bad_bits.precision_bits = 199;
    assert!(bad_bits.validate().is_err());

    let mut bad_family = base;
    bad_family.family = "   ".to_string();
    assert!(bad_family.validate().is_err());
}

//! Acceptance criteria of spec node KRN.SOLV.CONIC.WRAP (AC-1, AC-2, AC-3, AC-5; AC-4 is a
//! unit test in `src/conic/tests.rs`, because only the crate itself can run other limits).
//!
//! AC-1 optima come from `fixtures/solv/wrap/small-v1/make_fixtures.py` (exact rational
//! arithmetic, independent of any solver). The error cases are hand-labelled from the spec's
//! rules. The output-bit pins in `OUTPUT_PINS` are regression pins of this crate's own output
//! (not an oracle); CI checks them on x86-64 Linux and Windows, ARM64 macOS and wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_solv::conic::{
    limits_descriptor, solve_lp, solve_qp, solve_socp, Cone, ConicCause, ConicError, ConicLimits, ConicProblem,
    ConicProblemBuilder, ConicSolution, Csc, LimitValue, ProblemClass, LIMITS_NAME,
};
use fid_testkit::Sha256Hasher;

const FIXTURE: &str = include_str!("../../../fixtures/solv/wrap/small-v1/problems.txt");

fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|t| f64::from_bits(u64::from_str_radix(t, 16).unwrap())).collect()
}

fn usizes(s: &str) -> Vec<usize> {
    s.split_whitespace().map(|t| t.parse().unwrap()).collect()
}

struct Case {
    name: String,
    class: String,
    problem: ConicProblem,
    objective: f64,
}

fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut head = String::new();
    for l in FIXTURE.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        if let Some(rest) = l.strip_prefix("problem ") {
            head = rest.to_string();
            fields.clear();
        } else if l == "end" {
            let get = |k: &str| fields.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap();
            let h: Vec<&str> = head.split_whitespace().collect();
            let n: usize = get("n").parse().unwrap();
            let m: usize = get("m").parse().unwrap();
            let a = Csc::new(m, n, usizes(&get("a_colptr")), usizes(&get("a_rowval")), hexes(&get("a_nzval")));
            let toks: Vec<String> = get("cones").split_whitespace().map(String::from).collect();
            let cones = toks
                .chunks(2)
                .map(|c| {
                    let d = c[1].parse().unwrap();
                    match c[0].as_str() {
                        "zero" => Cone::Zero(d),
                        "nonneg" => Cone::Nonneg(d),
                        "soc" => Cone::Soc(d),
                        k => panic!("cone {k}"),
                    }
                })
                .collect();
            let mut builder = ConicProblemBuilder::new(hexes(&get("q")), a, hexes(&get("b")), cones);
            if fields.iter().all(|(k, _)| k != "p") {
                builder = builder.p(Csc::new(
                    n,
                    n,
                    usizes(&get("p_colptr")),
                    usizes(&get("p_rowval")),
                    hexes(&get("p_nzval")),
                ));
            }
            out.push(Case {
                name: h[0].to_string(),
                class: h[2].to_string(),
                problem: builder.build().unwrap(),
                objective: hexes(&get("objective_f64"))[0],
            });
        } else {
            let (k, v) = l.split_once(' ').unwrap_or((l, ""));
            fields.push((k.to_string(), v.to_string()));
        }
    }
    assert_eq!(out.len(), 3);
    out
}

fn solve(c: &Case) -> Result<ConicSolution, ConicError> {
    match c.class.as_str() {
        "lp" => solve_lp(&c.problem),
        "qp" => solve_qp(&c.problem),
        "socp" => solve_socp(&c.problem),
        k => panic!("class {k}"),
    }
}

/// SHA-256 of every output bit: iterations (LE u32), objective, x, y, s and the residuals
/// (LE f64 bits), in that order.
fn output_digest(s: &ConicSolution) -> String {
    let mut h = Sha256Hasher::new();
    h.update(&s.iterations.to_le_bytes());
    h.update(&s.objective.to_bits().to_le_bytes());
    for v in s.x.iter().chain(&s.y).chain(&s.s) {
        h.update(&v.to_bits().to_le_bytes());
    }
    let r = &s.residuals;
    for v in [r.primal, r.dual, r.gap_abs, r.gap_rel] {
        h.update(&v.to_bits().to_le_bytes());
    }
    h.finish()
}

/// Output-bit pins per problem: regression pins of this crate's own output, recorded from a
/// local x86-64 run and checked by CI on every target (x86-64, aarch64, wasm32). One table
/// serves all targets because they agree bit for bit; a target that diverged would get its
/// own `#[cfg(target_arch)]` table here.
const OUTPUT_PINS: [(&str, &str); 3] = [
    ("straightness-lp", "sha256:1ace3009401adb1fe3c067e04a3a4621d071ad75bac73e7b34a8cea44114b784"),
    ("box-qp", "sha256:8d69fe4aac6f747496be12daba41769616cd062100ee8a739caa158f46115ec6"),
    ("circle-socp", "sha256:d81c5283a593434b13b6df9dc930c1fc675655bfd94fa4ea35ff58340d9c5abb"),
];

/// AC-1: each objective within 1e-9 relative of the exact optimum, status solved, all
/// values finite (REQ-006).
#[test]
fn ac1_small_v1_objectives() {
    for c in cases() {
        let s = solve(&c).unwrap_or_else(|e| panic!("{}: {e}", c.name));
        let rel = (s.objective - c.objective).abs() / c.objective.abs();
        assert!(rel <= 1e-9, "{}: objective {:e} vs {:e} (rel {rel:e})", c.name, s.objective, c.objective);
        assert_eq!(s.x.len(), c.problem.n());
        assert_eq!(s.y.len(), c.problem.m());
        assert_eq!(s.s.len(), c.problem.m());
        assert!(s.x.iter().chain(&s.y).chain(&s.s).all(|v| v.is_finite()));
        assert!(s.iterations > 0 && s.iterations <= ConicLimits::FIXED.max_iter);
    }
}

/// Output bits are pinned and identical across repeated solves (D0).
#[test]
fn output_bits_pinned() {
    let mut got = Vec::new();
    for c in cases() {
        let a = solve(&c).unwrap();
        let b = solve(&c).unwrap();
        assert_eq!(output_digest(&a), output_digest(&b), "{}: repeated solve differs", c.name);
        got.push((c.name.clone(), output_digest(&a)));
    }
    let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, OUTPUT_PINS.map(|(n, _)| n));
    // all digests in one message, so a failing target prints every value it produced
    let pins: Vec<(String, String)> = OUTPUT_PINS.iter().map(|(n, d)| (n.to_string(), d.to_string())).collect();
    assert_eq!(got, pins, "output bits changed on {}", std::env::consts::ARCH);
}

fn csc_dense(rows: &[&[f64]]) -> Csc {
    let m = rows.len();
    let n = rows.first().map_or(0, |r| r.len());
    let (mut colptr, mut rowval, mut nzval) = (vec![0], Vec::new(), Vec::new());
    for j in 0..n {
        for (i, r) in rows.iter().enumerate() {
            if r[j] != 0.0 {
                rowval.push(i);
                nzval.push(r[j]);
            }
        }
        colptr.push(rowval.len());
    }
    Csc::new(m, n, colptr, rowval, nzval)
}

/// x >= 1 and x <= 0 as rows -x + s = -1, x + s = 0, s >= 0.
fn infeasible_lp() -> ConicProblem {
    ConicProblemBuilder::new(vec![1.0], csc_dense(&[&[-1.0], &[1.0]]), vec![-1.0, 0.0], vec![Cone::Nonneg(2)])
        .build()
        .unwrap()
}

fn invalid_field(r: Result<ConicProblem, ConicError>) -> &'static str {
    match r {
        Err(ConicError::InvalidProblem { field }) => field,
        other => panic!("expected InvalidProblem, got {other:?}"),
    }
}

/// AC-2 and the rest of REQ-001: each rejection names its field.
#[test]
fn ac2_invalid_problem() {
    let a = || csc_dense(&[&[-1.0], &[1.0]]);
    let nn = || vec![Cone::Nonneg(2)];
    // AC-2: NaN in b
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![1.0], a(), vec![f64::NAN, 0.0], nn()).build()), "b");
    // AC-2: SOC of dimension 1
    let soc1 = vec![Cone::Soc(1), Cone::Nonneg(1)];
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![1.0], a(), vec![0.0, 0.0], soc1).build()), "cones");
    // non-finite entries
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![f64::INFINITY], a(), vec![0.0, 0.0], nn()).build()), "q");
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![1.0], a(), vec![0.0, f64::INFINITY], nn()).build()), "b");
    let a_nan = csc_dense(&[&[f64::NAN], &[1.0]]);
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![1.0], a_nan, vec![0.0, 0.0], nn()).build()), "A");
    let p_nan = csc_dense(&[&[f64::NAN]]);
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![1.0], a(), vec![0.0, 0.0], nn()).p(p_nan).build()), "P");
    // dimension mismatches
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![1.0, 1.0], a(), vec![0.0, 0.0], nn()).build()), "A");
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![1.0], a(), vec![0.0], nn()).build()), "b");
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![], a(), vec![0.0, 0.0], nn()).build()), "q");
    let p_big = csc_dense(&[&[1.0, 0.0], &[0.0, 1.0]]);
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![1.0], a(), vec![0.0, 0.0], nn()).p(p_big).build()), "P");
    // cone dimensions not summing to the rows of A; zero-dimension cone
    let three = vec![Cone::Nonneg(3)];
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![1.0], a(), vec![0.0, 0.0], three).build()), "cones");
    let zero = vec![Cone::Zero(0), Cone::Nonneg(2)];
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![1.0], a(), vec![0.0, 0.0], zero).build()), "cones");
    // P entry below the diagonal
    let p_lower = Csc::new(2, 2, vec![0, 2, 3], vec![0, 1, 1], vec![1.0, 0.5, 1.0]);
    let a2 = csc_dense(&[&[1.0, 0.0], &[0.0, 1.0]]);
    let r = ConicProblemBuilder::new(vec![1.0, 1.0], a2.clone(), vec![0.0, 0.0], nn()).p(p_lower).build();
    assert_eq!(invalid_field(r), "P");
    // malformed CSC: unsorted rows, row out of range, bad colptr
    let unsorted = Csc::new(2, 1, vec![0, 2], vec![1, 0], vec![1.0, 1.0]);
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![1.0], unsorted, vec![0.0, 0.0], nn()).build()), "A");
    let out_of_range = Csc::new(2, 1, vec![0, 1], vec![2], vec![1.0]);
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![1.0], out_of_range, vec![0.0, 0.0], nn()).build()), "A");
    let bad_ptr = Csc::new(2, 1, vec![0, 3], vec![0, 1], vec![1.0, 1.0]);
    assert_eq!(invalid_field(ConicProblemBuilder::new(vec![1.0], bad_ptr, vec![0.0, 0.0], nn()).build()), "A");
    // a negative diagonal entry of P (necessary PSD check)
    let p_neg = Csc::new(2, 2, vec![0, 1, 2], vec![0, 1], vec![1.0, -1e-300]);
    let r = ConicProblemBuilder::new(vec![1.0, 1.0], a2.clone(), vec![0.0, 0.0], nn()).p(p_neg).build();
    assert_eq!(invalid_field(r), "P");
    // a zero or missing diagonal entry is accepted
    let p_zero = Csc::new(2, 2, vec![0, 1, 1], vec![0], vec![0.0]);
    assert!(ConicProblemBuilder::new(vec![1.0, 1.0], a2.clone(), vec![0.0, 0.0], nn()).p(p_zero).build().is_ok());
    // a valid upper-triangular P is accepted
    let p_upper = Csc::new(2, 2, vec![0, 1, 3], vec![0, 0, 1], vec![1.0, 0.5, 1.0]);
    assert!(ConicProblemBuilder::new(vec![1.0, 1.0], a2, vec![0.0, 0.0], nn()).p(p_upper).build().is_ok());
}

/// AC-2 and REQ-002: entry points check the problem class.
#[test]
fn ac2_wrong_problem_class() {
    let socp = cases().into_iter().find(|c| c.class == "socp").unwrap().problem;
    assert!(matches!(
        solve_lp(&socp),
        Err(ConicError::WrongProblemClass { entry: ProblemClass::Lp, has_p: false, has_soc: true })
    ));
    assert!(matches!(solve_qp(&socp), Err(ConicError::WrongProblemClass { entry: ProblemClass::Qp, .. })));
    let qp = cases().into_iter().find(|c| c.class == "qp").unwrap().problem;
    assert!(matches!(solve_lp(&qp), Err(ConicError::WrongProblemClass { entry: ProblemClass::Lp, has_p: true, .. })));
    assert!(matches!(solve_socp(&qp), Err(ConicError::WrongProblemClass { entry: ProblemClass::Socp, .. })));
    // an LP is also a QP and an SOCP
    let lp = cases().into_iter().find(|c| c.class == "lp").unwrap();
    let expect = solve_lp(&lp.problem).unwrap().objective;
    assert!(solve_qp(&lp.problem).is_ok());
    assert!(solve_socp(&lp.problem).is_ok());
    assert!((expect - lp.objective).abs() <= 1e-9 * lp.objective.abs());
}

/// AC-3: x >= 1 and x <= 0 is primal infeasible: typed error, finite residuals, no solution.
#[test]
fn ac3_primal_infeasible() {
    match solve_lp(&infeasible_lp()) {
        Err(ConicError::Solver(e)) => {
            assert_eq!(e.cause, ConicCause::PrimalInfeasible);
            assert!(e.iterations > 0);
            for v in [e.primal_residual, e.dual_residual, e.gap] {
                assert!(v.is_some_and(f64::is_finite), "{e:?}");
            }
        }
        other => panic!("expected PrimalInfeasible, got {other:?}"),
    }
}

/// REQ-005: minimise -x subject to x >= 0 is unbounded (dual infeasible).
#[test]
fn dual_infeasible() {
    let p =
        ConicProblemBuilder::new(vec![-1.0], csc_dense(&[&[-1.0]]), vec![0.0], vec![Cone::Nonneg(1)]).build().unwrap();
    match solve_lp(&p) {
        Err(ConicError::Solver(e)) => assert_eq!(e.cause, ConicCause::DualInfeasible),
        other => panic!("expected DualInfeasible, got {other:?}"),
    }
}

/// The ordering size budget is a typed error before the solver is set up.
#[test]
fn too_large_is_typed() {
    let m = 20_000;
    let a = Csc::zeros(m, 1);
    let p = ConicProblemBuilder::new(vec![1.0], a, vec![0.0; m], vec![Cone::Nonneg(m)]).build().unwrap();
    assert!(matches!(solve_lp(&p), Err(ConicError::TooLarge { kkt_dim: 20_001, kkt_nnz: 20_001 })));
}

/// AC-5: the descriptor lists every constant as hex (floats) and is a fixed byte string.
#[test]
fn ac5_limits_descriptor() {
    let d = limits_descriptor();
    assert_eq!(d.name, LIMITS_NAME);
    assert_eq!(d.version, ConicLimits::FIXED.version);
    let l = ConicLimits::FIXED;
    let get = |k: &str| d.constants.iter().find(|(name, _)| *name == k).map(|(_, v)| v.clone()).unwrap();
    assert_eq!(get("max_iter"), LimitValue::Uint(u64::from(l.max_iter)));
    assert_eq!(get("tol_feas"), LimitValue::F64(l.tol_feas));
    assert_eq!(get("tol_gap_abs"), LimitValue::F64(l.tol_gap_abs));
    assert_eq!(get("tol_gap_rel"), LimitValue::F64(l.tol_gap_rel));
    assert_eq!(get("tol_infeas_abs"), LimitValue::F64(l.tol_infeas));
    assert_eq!(get("tol_infeas_rel"), LimitValue::F64(l.tol_infeas));
    assert_eq!(get("static_regularization_constant"), LimitValue::F64(l.static_reg));
    assert_eq!(get("iterative_refinement_max_iter"), LimitValue::Uint(u64::from(l.refine_steps)));
    let json = d.canonical_json();
    assert_eq!(json, limits_descriptor().canonical_json());
    assert_eq!(json, EXPECTED_DESCRIPTOR);
}

/// Hand-written expected descriptor; float bits from Python's `struct.pack('<d', v)`.
const EXPECTED_DESCRIPTOR: &str = concat!(
    "{\"constants\":{",
    "\"direct_kkt_solver\":true,",
    "\"direct_solve_method\":\"qdldl\",",
    "\"dynamic_regularization_delta\":\"3e8ad7f29abcaf48\",",
    "\"dynamic_regularization_enable\":true,",
    "\"dynamic_regularization_eps\":\"3d3c25c268497682\",",
    "\"equilibrate_enable\":true,",
    "\"equilibrate_max_iter\":10,",
    "\"equilibrate_max_scaling\":\"40c3880000000000\",",
    "\"equilibrate_min_scaling\":\"3f1a36e2eb1c432d\",",
    "\"input_sparse_dropzeros\":false,",
    "\"iterative_refinement_abstol\":\"3d719799812dea11\",",
    "\"iterative_refinement_enable\":true,",
    "\"iterative_refinement_max_iter\":10,",
    "\"iterative_refinement_reltol\":\"3d3c25c268497682\",",
    "\"iterative_refinement_stop_ratio\":\"4014000000000000\",",
    "\"linesearch_backtrack_step\":\"3fe999999999999a\",",
    "\"max_iter\":200,",
    "\"max_step_fraction\":\"3fefae147ae147ae\",",
    "\"min_switch_step_length\":\"3fb999999999999a\",",
    "\"min_terminate_step_length\":\"3f1a36e2eb1c432d\",",
    "\"ordering_max_dim\":20000,",
    "\"ordering_max_fill\":4000000,",
    "\"ordering_max_nnz\":1000000,",
    "\"presolve_enable\":false,",
    "\"reduced_tol_feas\":\"3f1a36e2eb1c432d\",",
    "\"reduced_tol_gap_abs\":\"3f0a36e2eb1c432d\",",
    "\"reduced_tol_gap_rel\":\"3f0a36e2eb1c432d\",",
    "\"reduced_tol_infeas_abs\":\"3d95fd7fe1796495\",",
    "\"reduced_tol_infeas_rel\":\"3f0a36e2eb1c432d\",",
    "\"reduced_tol_ktratio\":\"3f1a36e2eb1c432d\",",
    "\"static_regularization_constant\":\"3e45798ee2308c3a\",",
    "\"static_regularization_enable\":true,",
    "\"static_regularization_proportional\":\"3970000000000000\",",
    "\"tol_feas\":\"3ddb7cdfd9d7bdbb\",",
    "\"tol_gap_abs\":\"3ddb7cdfd9d7bdbb\",",
    "\"tol_gap_rel\":\"3ddb7cdfd9d7bdbb\",",
    "\"tol_infeas_abs\":\"3ddb7cdfd9d7bdbb\",",
    "\"tol_infeas_rel\":\"3ddb7cdfd9d7bdbb\",",
    "\"tol_ktratio\":\"3eb0c6f7a0b5ed8d\"",
    "},\"name\":\"fid-solv.conic.limits\",\"version\":1}",
);

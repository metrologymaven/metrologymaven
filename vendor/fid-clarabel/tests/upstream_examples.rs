//! KRN.SOLV.CONIC.PATCH AC-3 and AC-4: the patched solver on upstream Clarabel 0.11.1's own
//! in-scope example problems (`examples/rust/example_{lp,qp,socp,box}.rs` and
//! `examples/data/hs35.json`).
//!
//! AC-3: every objective agrees within 1e-8 relative with the value recorded from the
//! unpatched upstream build (`fixtures/solv/patch/upstream-examples-v1/objectives.txt`; the
//! recorded values are checked against closed-form optima by `check_objectives.py`).
//! AC-4: solving the same problem twice gives bit-identical output.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(non_snake_case)]

use fid_clarabel::algebra::*;
use fid_clarabel::solver::*;
use std::path::PathBuf;

struct Problem {
    name: &'static str,
    P: CscMatrix,
    q: Vec<f64>,
    A: CscMatrix,
    b: Vec<f64>,
    cones: Vec<SupportedConeT>,
    settings: DefaultSettings,
}

/// Settings of example_lp.rs and example_box.rs.
fn lp_settings() -> DefaultSettings {
    DefaultSettings { equilibrate_enable: true, max_iter: 50, ..DefaultSettings::default() }
}

fn problems() -> Vec<Problem> {
    let mut out = Vec::new();

    // example_lp.rs
    out.push(Problem {
        name: "lp",
        P: CscMatrix::zeros((2, 2)),
        q: vec![1., -1.],
        A: CscMatrix::from(&[[1., 0.], [0., 1.], [-1., 0.], [0., -1.]]),
        b: vec![1.; 4],
        cones: vec![NonnegativeConeT(4)],
        settings: lp_settings(),
    });

    // example_qp.rs
    out.push(Problem {
        name: "qp",
        P: CscMatrix::from(&[[6., 0.], [0., 4.]]),
        q: vec![-1., -4.],
        A: CscMatrix::from(&[[1., -2.], [1., 0.], [0., 1.], [-1., 0.], [0., -1.]]),
        b: vec![0., 1., 1., 1., 1.],
        cones: vec![ZeroConeT(1), NonnegativeConeT(4)],
        settings: DefaultSettings::default(),
    });

    // example_socp.rs
    out.push(Problem {
        name: "socp",
        P: CscMatrix::from(&[[0., 0.], [0., 2.]]),
        q: vec![0., 0.],
        A: CscMatrix::from(&[[0., 0.], [-2., 0.], [0., -1.]]),
        b: vec![1., -2., -2.],
        cones: vec![SecondOrderConeT(3)],
        settings: DefaultSettings::default(),
    });

    // example_box.rs
    let n = 200;
    let I1 = CscMatrix::identity(n);
    let mut I2 = CscMatrix::identity(n);
    I2.negate();
    out.push(Problem {
        name: "box",
        P: CscMatrix::identity(n),
        q: vec![1.; n],
        A: CscMatrix::vcat(&I1, &I2).unwrap(),
        b: vec![1.; 2 * n],
        cones: vec![NonnegativeConeT(2 * n)],
        settings: lp_settings(),
    });

    // examples/data/hs35.json (problem data; its settings are the defaults)
    out.push(Problem {
        name: "hs35",
        P: CscMatrix::new(
            3,
            3,
            vec![0, 1, 3, 5],
            vec![0, 0, 1, 0, 2],
            vec![4.000000000000001, 2.0000000000000004, 4.000000000000001, 2.0, 2.0],
        ),
        q: vec![-8.0, -6.0, -4.0],
        A: CscMatrix::new(4, 3, vec![0, 2, 4, 6], vec![0, 1, 0, 2, 0, 3], vec![1.0, -1.0, 1.0, -1.0, 2.0, -1.0]),
        b: vec![3.0, 0.0, 0.0, 0.0],
        cones: vec![NonnegativeConeT(4)],
        settings: DefaultSettings::default(),
    });
    out
}

fn solve(p: &Problem) -> DefaultSolution {
    let mut solver = DefaultSolver::new(&p.P, &p.q, &p.A, &p.b, &p.cones, p.settings.clone()).unwrap();
    solver.solve();
    solver.solution
}

/// `(name, status, objective)` from the fixture.
fn recorded() -> Vec<(String, String, f64)> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../fixtures/solv/patch/upstream-examples-v1/objectives.txt");
    let text = std::fs::read_to_string(&path).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            assert_eq!(f.len(), 4, "bad fixture line {l:?}");
            let bits = u64::from_str_radix(f[2], 16).unwrap();
            let value = f64::from_bits(bits);
            assert_eq!(value, f[3].parse::<f64>().unwrap(), "hex and decimal disagree in {l:?}");
            (f[0].to_string(), f[1].to_string(), value)
        })
        .collect()
}

#[test]
fn ac3_objectives_match_unpatched_upstream() {
    let rec = recorded();
    let probs = problems();
    assert_eq!(rec.len(), probs.len());
    for (p, (name, status, obj)) in probs.iter().zip(rec.iter()) {
        assert_eq!(p.name, name);
        let sol = solve(p);
        assert_eq!(format!("{:?}", sol.status), *status, "{name}");
        let rel = (sol.obj_val - obj).abs() / obj.abs();
        eprintln!("{name}: patched {:e}, upstream {obj:e}, relative difference {rel:e}", sol.obj_val);
        assert!(rel <= 1e-8, "{name}: patched {:e} vs upstream {:e} (relative {rel:e})", sol.obj_val, obj);
    }
}

fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|x| x.to_bits()).collect()
}

#[test]
fn ac4_repeated_solves_are_bit_identical() {
    for p in problems() {
        let a = solve(&p);
        let b = solve(&p);
        assert_eq!(bits(&a.x), bits(&b.x), "{}", p.name);
        assert_eq!(bits(&a.z), bits(&b.z), "{}", p.name);
        assert_eq!(bits(&a.s), bits(&b.s), "{}", p.name);
        assert_eq!(a.obj_val.to_bits(), b.obj_val.to_bits(), "{}", p.name);
        assert_eq!(a.obj_val_dual.to_bits(), b.obj_val_dual.to_bits(), "{}", p.name);
        assert_eq!(a.iterations, b.iterations, "{}", p.name);
        assert_eq!(a.status, b.status, "{}", p.name);
    }
}

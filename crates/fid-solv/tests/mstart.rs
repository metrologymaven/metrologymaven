//! Acceptance criteria of spec node KRN.SOLV.MSTART (AC-1 .. AC-5) and the KNP-1 edge cases.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;

use fid_num::NumError;
use fid_solv::mstart::{
    generate_starts, run, run_listed, MultiStartError, SpecError, StartError, StartOrigin, StartSpec, StartSummary,
    DEFAULT_TOLERANCE, MAX_STARTS,
};
use fid_testkit::Sha256Hasher;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/solv/mstart-double-well-v1.txt");

fn f64_hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

/// Per start: (start, well, objective, params).
type Row = (Vec<f64>, String, f64, Vec<f64>);

struct Fixture {
    text: String,
}

impl Fixture {
    fn load() -> Self {
        Fixture { text: std::fs::read_to_string(FIX).unwrap() }
    }
    fn field(&self, k: &str) -> String {
        self.text.lines().find_map(|l| l.strip_prefix(&format!("{k} "))).map(str::to_string).unwrap()
    }
    fn floats(&self, k: &str) -> Vec<f64> {
        self.field(k).split_whitespace().map(f64_hex).collect()
    }
    fn spec(&self) -> StartSpec {
        StartSpec {
            primary: self.floats("primary"),
            half_widths: self.floats("half_widths"),
            count: self.field("count").parse().unwrap(),
            seed: self.field("seed").parse().unwrap(),
            stream: self.field("stream").parse().unwrap(),
            tolerances: self.floats("tolerances"),
        }
    }
    fn rows(&self) -> Vec<Row> {
        self.text
            .lines()
            .filter_map(|l| l.strip_prefix("start "))
            .map(|l| {
                let t: Vec<&str> = l.split_whitespace().collect();
                // i x y well W objective O params px py
                assert_eq!((t[3], t[5], t[7]), ("well", "objective", "params"), "{l}");
                (
                    vec![f64_hex(t[1]), f64_hex(t[2])],
                    t[4].to_string(),
                    f64_hex(t[6]),
                    vec![f64_hex(t[8]), f64_hex(t[9])],
                )
            })
            .collect()
    }
}

/// The fixture's toy local solver (see fixtures/solv/make_mstart_fixtures.py): 60 halvings
/// of the distance to the minimiser of the well the start lies in, then that well's
/// objective. Returns the well name as the solver value.
fn double_well_solve(s: &[f64]) -> Result<(&'static str, f64, Vec<f64>), String> {
    let (mut x, mut y) = (s[0], s[1]);
    let left = x < 0.125;
    let cx = if left { -1.0 } else { 1.0 };
    for _ in 0..60 {
        let d = (x - cx) * 0.5;
        x = cx + d;
        let d = (y - 0.0) * 0.5;
        y = 0.0 + d;
    }
    let dx = x - cx;
    let mut obj = dx * dx + y * y;
    if !left {
        obj += 0.5;
    }
    Ok((if left { "left" } else { "right" }, obj, vec![x, y]))
}

fn digest(rows: impl IntoIterator<Item = Vec<f64>>) -> String {
    let mut h = Sha256Hasher::new();
    for row in rows {
        for v in row {
            h.update(&v.to_le_bytes());
        }
    }
    h.finish()
}

/// AC-1 (and AC-3: CI runs this on x86-64 Linux and Windows, ARM64 macOS and wasm32 under
/// wasmtime against the same golden digests, so the outputs are bit-identical across them).
#[test]
fn ac1_double_well_selects_deeper_well_and_flags() {
    let fx = Fixture::load();
    let spec = fx.spec();
    assert_eq!((spec.count, spec.seed), (8, 42));
    let out = run(&spec, double_well_solve).unwrap();
    let rows = fx.rows();
    assert_eq!(out.starts.len(), 8);
    for (i, (start, _, objective, params)) in rows.iter().enumerate() {
        let bits = |v: &[f64]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&out.starts[i]), bits(start), "start {i}");
        match &out.per_start[i] {
            StartSummary::Converged { objective: o, params: p } => {
                assert_eq!(o.to_bits(), objective.to_bits(), "objective {i}");
                assert_eq!(bits(p), bits(params), "params {i}");
            }
            StartSummary::Failed(e) => panic!("start {i} failed: {e:?}"),
        }
    }
    assert_eq!(digest(out.starts.clone()), fx.field("starts_digest"));
    let outcome = out.per_start.iter().map(|s| match s {
        StartSummary::Converged { objective, params } => [vec![*objective], params.clone()].concat(),
        StartSummary::Failed(_) => unreachable!(),
    });
    assert_eq!(digest(outcome), fx.field("outcome_digest"));
    assert_eq!(out.best_index.to_string(), fx.field("best_index"));
    assert_eq!(out.best, "left", "the deeper well is selected");
    assert_eq!(rows[out.best_index as usize].1, "left");
    assert_eq!(rows[0].1, "right", "the primary start is in the shallow well");
    assert_eq!(out.agreement.to_string(), fx.field("agreement"));
    assert_eq!(out.local_optimum_only.to_string(), fx.field("local_optimum_only"));
    assert!(out.local_optimum_only && !out.agreement);
    assert_eq!(out.origin, StartOrigin::Seeded { seed: 42, stream: 0 });
}

/// AC-3 (in-process half): repeated runs give identical outputs.
#[test]
fn ac3_repeated_runs_identical() {
    let spec = Fixture::load().spec();
    let a = run(&spec, double_well_solve).unwrap();
    for _ in 0..5 {
        assert_eq!(run(&spec, double_well_solve).unwrap(), a);
    }
}

/// AC-2: a convex quadratic, min at (3, -1); every start converges there, so they agree.
#[test]
fn ac2_convex_quadratic_agrees() {
    let solve = |s: &[f64]| -> Result<((), f64, Vec<f64>), String> {
        let c = [3.0, -1.0];
        let mut p = s.to_vec();
        for _ in 0..200 {
            for (x, cj) in p.iter_mut().zip(c) {
                *x = cj + (*x - cj) * 0.5;
            }
        }
        let (dx, dy) = (p[0] - c[0], p[1] - c[1]);
        Ok(((), dx * dx + 2.0 * dy * dy, p))
    };
    let spec = StartSpec {
        primary: vec![0.0, 0.0],
        half_widths: vec![5.0, 5.0],
        count: 8,
        seed: 7,
        stream: 3,
        tolerances: vec![],
    };
    let out = run(&spec, solve).unwrap();
    assert!(out.agreement);
    assert!(!out.local_optimum_only);
    assert_eq!(out.per_start.len(), 8);
}

/// AC-4: every start fails, so the error lists 8 entries and there is no value.
#[test]
fn ac4_all_failed() {
    let spec = Fixture::load().spec();
    let calls = RefCell::new(0);
    let r = run(&spec, |_s: &[f64]| -> Result<((), f64, Vec<f64>), String> {
        *calls.borrow_mut() += 1;
        Err(format!("no convergence {}", calls.borrow()))
    });
    match r {
        Err(MultiStartError::AllFailed(errs)) => {
            assert_eq!(errs.len(), 8);
            for (i, e) in errs.iter().enumerate() {
                assert_eq!(e, &StartError::Solver(format!("no convergence {}", i + 1)));
            }
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(*calls.borrow(), 8);
}

/// AC-5: equal objectives go to the lower parameter vector in total order, then the lower
/// start index, on every run.
#[test]
fn ac5_tie_break_is_deterministic() {
    let starts: Vec<Vec<f64>> = (0..5).map(|i| vec![f64::from(i), 0.0]).collect();
    let results = [vec![1.0, 2.0], vec![0.5, 9.0], vec![0.5, 9.0], vec![0.5, 10.0], vec![2.0, -5.0]];
    for _ in 0..10 {
        let out = run_listed(&starts, &[1.0, 1.0], |s: &[f64]| -> Result<(usize, f64, Vec<f64>), ()> {
            let i = s[0] as usize;
            Ok((i, 4.25, results[i].clone()))
        })
        .unwrap();
        assert_eq!((out.best, out.best_index), (1, 1));
        assert!(out.local_optimum_only);
        assert_eq!(out.origin, StartOrigin::Listed);
    }
    // Total order puts -0.0 below +0.0, in objectives and in parameters.
    let out = run_listed(&[vec![0.0], vec![1.0]], &[], |s: &[f64]| -> Result<((), f64, Vec<f64>), ()> {
        Ok(((), 0.0, vec![if s[0] == 0.0 { 0.0 } else { -0.0 }]))
    })
    .unwrap();
    assert_eq!(out.best_index, 1);
    assert!(out.agreement, "-0.0 and +0.0 are within any tolerance");
    let out = run_listed(&[vec![0.0], vec![1.0]], &[], |s: &[f64]| -> Result<((), f64, Vec<f64>), ()> {
        Ok(((), if s[0] == 0.0 { 0.0 } else { -0.0 }, vec![5.0]))
    })
    .unwrap();
    assert_eq!(out.best_index, 1);
    // A lower objective wins over a lower parameter vector.
    let out = run_listed(&[vec![0.0], vec![1.0]], &[], |s: &[f64]| -> Result<((), f64, Vec<f64>), ()> {
        Ok(((), if s[0] == 0.0 { 2.0 } else { 1.0 }, vec![10.0 - s[0]]))
    })
    .unwrap();
    assert_eq!(out.best_index, 1);
}

/// REQ-001: start 0 is the primary, bit for bit; the others lie in the box; seed and stream
/// select the sequence.
#[test]
fn starts_are_seeded_and_in_box() {
    let spec = StartSpec {
        primary: vec![-0.0, 1e-3, 250.0],
        half_widths: vec![0.5, 0.0, 3.0],
        count: 64,
        seed: 0xDEAD_BEEF,
        stream: 9,
        tolerances: vec![],
    };
    let a = generate_starts(&spec).unwrap();
    assert_eq!(a.len(), 64);
    assert_eq!(
        a[0].iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
        spec.primary.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
    );
    for s in &a[1..] {
        for ((x, p), h) in s.iter().zip(&spec.primary).zip(&spec.half_widths) {
            assert!((x - p).abs() <= *h, "{s:?}");
        }
        assert_eq!(s[1], 1e-3, "zero half width keeps the parameter");
    }
    assert_eq!(generate_starts(&spec).unwrap(), a);
    let other_seed = generate_starts(&StartSpec { seed: 1, ..spec.clone() }).unwrap();
    let other_stream = generate_starts(&StartSpec { stream: 10, ..spec.clone() }).unwrap();
    assert_ne!(other_seed[1..], a[1..]);
    assert_ne!(other_stream[1..], a[1..]);
    // A shorter run is a prefix of a longer one with the same seed.
    let short = generate_starts(&StartSpec { count: 5, ..spec.clone() }).unwrap();
    assert_eq!(short[..], a[..5]);
}

/// REQ-002: the solve runs once per start, in index order.
#[test]
fn solve_runs_in_index_order() {
    let spec = Fixture::load().spec();
    let seen = RefCell::new(Vec::new());
    let out = run(&spec, |s: &[f64]| {
        seen.borrow_mut().push(s.to_vec());
        double_well_solve(s)
    })
    .unwrap();
    assert_eq!(*seen.borrow(), out.starts);
}

/// KNP-1: NaN or infinite objectives and parameters are typed per-start errors and never
/// selected; if every start has one, the run fails.
#[test]
fn non_finite_outcomes_are_start_errors() {
    let starts: Vec<Vec<f64>> = (0..4).map(|i| vec![f64::from(i)]).collect();
    let out = run_listed(&starts, &[], |s: &[f64]| -> Result<(u32, f64, Vec<f64>), ()> {
        match s[0] as u32 {
            0 => Ok((0, f64::NAN, vec![0.0])),
            1 => Ok((1, 3.0, vec![1.0])),
            2 => Ok((2, f64::NEG_INFINITY, vec![2.0])),
            _ => Ok((3, -1.0, vec![f64::INFINITY])),
        }
    })
    .unwrap();
    assert_eq!(out.best, 1);
    assert!(out.agreement, "only one start succeeded");
    let nan = NumError::NotANumber { op: "mstart.objective", index: None };
    assert_eq!(out.per_start[0], StartSummary::Failed(StartError::Numeric(nan)));
    assert_eq!(
        out.per_start[2],
        StartSummary::Failed(StartError::Numeric(NumError::Infinite { op: "mstart.objective", index: None }))
    );
    assert_eq!(
        out.per_start[3],
        StartSummary::Failed(StartError::Numeric(NumError::Infinite { op: "mstart.params", index: Some(0) }))
    );
    let r = run_listed(&starts, &[], |_s: &[f64]| -> Result<((), f64, Vec<f64>), ()> { Ok(((), f64::NAN, vec![0.0])) });
    match r {
        Err(MultiStartError::AllFailed(errs)) => {
            assert_eq!(errs, vec![StartError::Numeric(nan); 4]);
        }
        other => panic!("{other:?}"),
    }
}

/// A parameter vector of the wrong length is that start's error.
#[test]
fn wrong_param_length_is_start_error() {
    let out = run_listed(&[vec![0.0, 0.0], vec![1.0, 1.0]], &[], |s: &[f64]| -> Result<((), f64, Vec<f64>), ()> {
        Ok(((), 0.0, if s[0] == 0.0 { vec![0.0] } else { s.to_vec() }))
    })
    .unwrap();
    assert_eq!(out.best_index, 1);
    assert_eq!(out.per_start[0], StartSummary::Failed(StartError::ParamLength { expected: 2, got: 1 }));
}

/// REQ-004: tolerances are per parameter; empty means DEFAULT_TOLERANCE.
#[test]
fn agreement_uses_tolerances() {
    let agree = |offset: f64, tol: &[f64]| {
        run_listed(&[vec![0.0, 0.0], vec![1.0, 0.0]], tol, |s: &[f64]| -> Result<((), f64, Vec<f64>), ()> {
            Ok(((), s[0], vec![1.0 + s[0] * offset, 2.0]))
        })
        .unwrap()
        .agreement
    };
    assert!(agree(DEFAULT_TOLERANCE * 0.5, &[]));
    assert!(!agree(2e-9, &[]));
    assert!(agree(2e-9, &[3e-9, 0.0]));
    assert!(!agree(2e-9, &[1e-9, 1.0]));
    assert!(agree(0.0, &[0.0, 0.0]));
}

/// Bounded start counts and spec validation; the solve never runs on a rejected spec.
#[test]
fn invalid_specs_are_rejected() {
    let base = StartSpec {
        primary: vec![1.0, 2.0],
        half_widths: vec![0.5, 0.5],
        count: 8,
        seed: 1,
        stream: 0,
        tolerances: vec![],
    };
    let check = |spec: StartSpec, want: SpecError| {
        let r = run(&spec, |_s: &[f64]| -> Result<((), f64, Vec<f64>), ()> { panic!("solve ran") });
        assert_eq!(r, Err(MultiStartError::InvalidSpec(want)));
    };
    check(StartSpec { count: 0, ..base.clone() }, SpecError::NoStarts);
    check(
        StartSpec { count: MAX_STARTS + 1, ..base.clone() },
        SpecError::TooManyStarts { count: MAX_STARTS as usize + 1, max: MAX_STARTS },
    );
    check(StartSpec { primary: vec![], half_widths: vec![], ..base.clone() }, SpecError::NoParameters);
    check(
        StartSpec { primary: vec![1.0, f64::NAN], ..base.clone() },
        SpecError::NotFinite { field: "primary", start: None, index: 1 },
    );
    check(
        StartSpec { half_widths: vec![f64::INFINITY, 0.5], ..base.clone() },
        SpecError::NotFinite { field: "half_widths", start: None, index: 0 },
    );
    check(
        StartSpec { half_widths: vec![0.5, -0.1], ..base.clone() },
        SpecError::Negative { field: "half_widths", index: 1 },
    );
    check(
        StartSpec { half_widths: vec![0.5], ..base.clone() },
        SpecError::LengthMismatch { field: "half_widths", start: None, expected: 2, got: 1 },
    );
    check(
        StartSpec { tolerances: vec![1e-9], ..base.clone() },
        SpecError::LengthMismatch { field: "tolerances", start: None, expected: 2, got: 1 },
    );
    check(
        StartSpec { tolerances: vec![1e-9, f64::NAN], ..base.clone() },
        SpecError::NotFinite { field: "tolerances", start: None, index: 1 },
    );
    check(
        StartSpec { tolerances: vec![-1e-9, 0.0], ..base.clone() },
        SpecError::Negative { field: "tolerances", index: 0 },
    );
    // A start that overflows is rejected, not passed to the solve.
    let huge = StartSpec { primary: vec![f64::MAX], half_widths: vec![f64::MAX], count: MAX_STARTS, ..base.clone() };
    assert!(matches!(generate_starts(&huge), Err(SpecError::StartNotFinite { index: 0, .. })));
    // MAX_STARTS itself is accepted.
    assert_eq!(generate_starts(&StartSpec { count: MAX_STARTS, ..base.clone() }).unwrap().len(), MAX_STARTS as usize);
    // Listed starts: ragged, NaN, empty.
    let ok = |_s: &[f64]| -> Result<((), f64, Vec<f64>), ()> { panic!("solve ran") };
    assert_eq!(
        run_listed(&[vec![1.0], vec![1.0, 2.0]], &[], ok),
        Err(MultiStartError::InvalidSpec(SpecError::LengthMismatch {
            field: "start",
            start: Some(1),
            expected: 1,
            got: 2
        }))
    );
    assert_eq!(
        run_listed(&[vec![1.0], vec![f64::NAN]], &[], ok),
        Err(MultiStartError::InvalidSpec(SpecError::NotFinite { field: "start", start: Some(1), index: 0 }))
    );
    assert_eq!(run_listed(&[], &[], ok), Err(MultiStartError::InvalidSpec(SpecError::NoStarts)));
    assert_eq!(run_listed(&[vec![]], &[], ok), Err(MultiStartError::InvalidSpec(SpecError::NoParameters)));
}

/// A single start is the primary alone and trivially agrees.
#[test]
fn single_start() {
    let spec = StartSpec {
        primary: vec![0.6, 0.3],
        half_widths: vec![1.0, 1.0],
        count: 1,
        seed: 0,
        stream: 0,
        tolerances: vec![],
    };
    let out = run(&spec, double_well_solve).unwrap();
    assert_eq!((out.best, out.best_index, out.agreement), ("right", 0, true));
    assert_eq!(out.starts, vec![vec![0.6, 0.3]]);
}

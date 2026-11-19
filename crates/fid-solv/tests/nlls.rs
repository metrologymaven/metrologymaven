//! Acceptance criteria of spec node KRN.SOLV.NLLS (AC-1 .. AC-5) and the error paths.
//!
//! Golden values come from `fixtures/solv/make_nlls_fixtures.py`: a 50-digit reference solution
//! (independent of the driver) and a bit-exact Python model of the pinned algorithm. CI runs
//! these tests on x86-64 Linux and Windows, ARM64 macOS and wasm32 under wasmtime against the
//! same golden bits and digests (AC-3).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::{f, NumError};
use fid_solv::nlls::{
    solve, Method, NllsConfig, NllsProblem, NllsResult, SolverError, Stage, StopReason, MAX_ITER,
};
use fid_testkit::Sha256Hasher;

const STRD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/solv/strd-models-synth-v1.txt");
const CIRCLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/solv/circle-synth-v1.txt");

fn hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(hex).collect()
}

fn decimals(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|t| t.parse().unwrap()).collect()
}

/// One problem block of a fixture: `key -> rest of line`, in file order.
struct Block {
    head: String,
    lines: Vec<(String, String)>,
}

impl Block {
    fn get(&self, k: &str) -> Option<&str> {
        self.lines.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str())
    }
    fn field(&self, k: &str) -> &str {
        self.get(k).unwrap_or_else(|| panic!("{}: no {k}", self.head))
    }
    /// A `key value` pair from the head line.
    fn head_value(&self, k: &str) -> String {
        let t: Vec<&str> = self.head.split_whitespace().collect();
        t.iter().position(|x| *x == k).map(|i| t[i + 1].to_string()).unwrap()
    }
}

fn split_line(l: &str) -> (String, String) {
    let (k, v) = l.split_once(' ').unwrap_or((l, ""));
    (k.to_string(), v.to_string())
}

/// Splits a fixture into its preamble (before the first `problem`) and problem blocks.
fn blocks(text: &str) -> (Block, Vec<Block>) {
    let mut pre = Block { head: String::new(), lines: Vec::new() };
    let mut out: Vec<Block> = Vec::new();
    for l in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        if let Some(rest) = l.strip_prefix("problem ") {
            out.push(Block { head: rest.to_string(), lines: Vec::new() });
        } else if let Some(b) = out.last_mut() {
            b.lines.push(split_line(l));
        } else {
            pre.lines.push(split_line(l));
        }
    }
    (pre, out)
}

/// A curve fit `r_i = model(x_i, p) - y_i` on one of the NIST StRD model forms, evaluated with
/// exactly the operations of the Python model (same order, `fid_num::f` functions).
struct Curve {
    model: String,
    n: usize,
    xs: Vec<f64>,
    ys: Vec<f64>,
}

fn model(name: &str, x: f64, b: &[f64]) -> Result<(f64, Vec<f64>), NumError> {
    let inv_pi = 1.0 / core::f64::consts::PI;
    Ok(match name {
        "misra1a" => {
            let e = f::exp(-(b[1] * x))?;
            (b[0] * (1.0 - e), vec![1.0 - e, (b[0] * x) * e])
        }
        "chwirut" => {
            let e = f::exp(-(b[0] * x))?;
            let u = b[1] + b[2] * x;
            let fv = e / u;
            (fv, vec![-(x * fv), -(fv / u), -((x * fv) / u)])
        }
        "danwood" => {
            let lx = f::ln(x)?;
            let w = f::exp(b[1] * lx)?;
            (b[0] * w, vec![w, (b[0] * w) * lx])
        }
        "lanczos3" => {
            let mut fv = 0.0;
            let mut g = Vec::new();
            for i in 0..3 {
                let e = f::exp(-(b[2 * i + 1] * x))?;
                fv += b[2 * i] * e;
                g.push(e);
                g.push(-((b[2 * i] * x) * e));
            }
            (fv, g)
        }
        "kirby2" => {
            let xx = x * x;
            let num = (b[0] + b[1] * x) + b[2] * xx;
            let den = (1.0 + b[3] * x) + b[4] * xx;
            let fv = num / den;
            (fv, vec![1.0 / den, x / den, xx / den, -((fv * x) / den), -((fv * xx) / den)])
        }
        "misra1d" => {
            let u = 1.0 + b[1] * x;
            let q = x / u;
            ((b[0] * b[1]) * q, vec![b[1] * q, (b[0] * q) / u])
        }
        "roszman1" => {
            let u = x - b[3];
            let t = b[2] / u;
            let w = 1.0 + t * t;
            let fv = (b[0] - b[1] * x) - f::atan(t)? * inv_pi;
            (fv, vec![1.0, -x, -((1.0 / u) / w) * inv_pi, -((t / u) / w) * inv_pi])
        }
        "mgh17" => {
            let e4 = f::exp(-(x * b[3]))?;
            let e5 = f::exp(-(x * b[4]))?;
            let fv = (b[0] + b[1] * e4) + b[2] * e5;
            (fv, vec![1.0, e4, e5, -((x * b[1]) * e4), -((x * b[2]) * e5)])
        }
        other => panic!("no model {other}"),
    })
}

impl NllsProblem for Curve {
    fn n_params(&self) -> usize {
        self.n
    }
    fn n_resid(&self) -> usize {
        self.xs.len()
    }
    fn eval(&self, p: &[f64], r: &mut [f64], j: &mut [f64]) -> Result<(), NumError> {
        for (i, (&x, &y)) in self.xs.iter().zip(&self.ys).enumerate() {
            let (fv, g) = model(&self.model, x, p)?;
            r[i] = fv - y;
            j[i * self.n..(i + 1) * self.n].copy_from_slice(&g);
        }
        Ok(())
    }
}

fn curve(b: &Block) -> Curve {
    Curve {
        model: b.head.split_whitespace().next().unwrap().to_string(),
        n: b.head_value("n").parse().unwrap(),
        xs: hexes(b.field("x")),
        ys: hexes(b.field("y")),
    }
}

fn reason_name(r: StopReason) -> &'static str {
    match r {
        StopReason::ZeroResidual => "zero_residual",
        StopReason::Gradient => "gradient",
        StopReason::Step => "step",
        StopReason::Cost => "cost",
    }
}

fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|x| x.to_bits()).collect()
}

/// Compares a result with a golden `p .. cost .. iterations .. evaluations .. reason .. rank ..`
/// line and returns the digest row (p, cost, iterations, evaluations).
fn check_run(tag: &str, res: &NllsResult, line: &str) -> Vec<f64> {
    let t: Vec<&str> = line.split_whitespace().collect();
    let at = |k: &str| t.iter().position(|x| *x == k).unwrap_or_else(|| panic!("{tag}: no {k}"));
    let p = hexes(&t[at("p") + 1..at("cost")].join(" "));
    assert_eq!(bits(&res.p), bits(&p), "{tag}: p");
    assert_eq!(res.cost.to_bits(), hex(t[at("cost") + 1]).to_bits(), "{tag}: cost");
    assert_eq!(res.iterations.to_string(), t[at("iterations") + 1], "{tag}: iterations");
    assert_eq!(res.evaluations.to_string(), t[at("evaluations") + 1], "{tag}: evaluations");
    assert_eq!(reason_name(res.stop_reason), t[at("reason") + 1], "{tag}: reason");
    assert_eq!(res.rank.to_string(), t[at("rank") + 1], "{tag}: rank");
    [res.p.clone(), vec![res.cost, res.iterations as f64, res.evaluations as f64]].concat()
}

fn digest(rows: &[Vec<f64>]) -> String {
    let mut h = Sha256Hasher::new();
    for row in rows {
        for v in row {
            h.update(&v.to_le_bytes());
        }
    }
    h.finish()
}

/// Smallest number of agreeing significant digits as a relative error bound check.
fn assert_rel(tag: &str, got: &[f64], want: &[f64], rel: f64) {
    for (k, (&g, &w)) in got.iter().zip(want).enumerate() {
        let e = (g - w).abs();
        assert!(e <= rel * w.abs(), "{tag}: parameter {k}: {g:e} vs reference {w:e} (rel {:e})", e / w.abs());
    }
}

fn load(path: &str) -> String {
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

/// AC-1 and AC-3: Levenberg-Marquardt with the default configuration on the StRD model forms
/// (lower and average difficulty, two starts each): the 50-digit reference parameters to at
/// least 6 significant digits, and every returned bit pattern equal to the golden model.
#[test]
fn ac1_ac3_strd_models_lm_default() {
    let text = load(STRD);
    let (pre, probs) = blocks(&text);
    let cfg = NllsConfig::default();
    assert_eq!(probs.len(), 8);
    let mut rows = Vec::new();
    for b in &probs {
        let prob = curve(b);
        let reference = decimals(b.field("ref"));
        for start in ["start1", "start2"] {
            let tag = format!("{} {start}", prob.model);
            let p0 = hexes(b.field(start));
            let res = solve(&prob, &p0, &cfg).unwrap_or_else(|e| panic!("{tag}: {e:?}"));
            rows.push(check_run(&tag, &res, b.field(&format!("lm_{start}"))));
            assert_rel(&tag, &res.p, &reference, 1e-6);
            assert_eq!(res.rank, prob.n);
            assert!(res.cond >= 1.0 && res.cond <= cfg.cond_limit);
            assert_eq!(res.config, cfg, "the configuration is echoed");
            assert_eq!(res.residuals.len(), prob.xs.len());
        }
    }
    assert_eq!(digest(&rows), pre.field("digest"));
}

#[test]
fn default_config_matches_the_fixture_record() {
    let text = load(STRD);
    let (pre, _) = blocks(&text);
    let t: Vec<&str> = pre.field("config").split_whitespace().collect();
    let v = |k: &str| t[t.iter().position(|x| *x == k).unwrap() + 1];
    let d = NllsConfig::default();
    assert_eq!(v("method"), "lm");
    assert_eq!(d.method, Method::LevenbergMarquardt);
    assert_eq!(v("max_iter").parse::<usize>().unwrap(), d.max_iter);
    for (k, x) in [
        ("tol_grad", d.tol_grad),
        ("tol_step", d.tol_step),
        ("tol_cost", d.tol_cost),
        ("cond_limit", d.cond_limit),
        ("rank_tol", d.rank_tol),
        ("lm_tau", d.lm_tau),
    ] {
        assert_eq!(v(k).parse::<f64>().unwrap().to_bits(), x.to_bits(), "{k}");
    }
}

/// 2D circle fit: residual `sqrt(dx^2 + dy^2) - R`, parameters (a, b, R) in metres.
struct Circle {
    xs: Vec<f64>,
    ys: Vec<f64>,
}

impl NllsProblem for Circle {
    fn n_params(&self) -> usize {
        3
    }
    fn n_resid(&self) -> usize {
        self.xs.len()
    }
    fn eval(&self, p: &[f64], r: &mut [f64], j: &mut [f64]) -> Result<(), NumError> {
        for (i, (&x, &y)) in self.xs.iter().zip(&self.ys).enumerate() {
            let dx = x - p[0];
            let dy = y - p[1];
            let d = f::sqrt(dx * dx + dy * dy)?;
            r[i] = d - p[2];
            j[3 * i..3 * i + 3].copy_from_slice(&[-(dx / d), -(dy / d), -1.0]);
        }
        Ok(())
    }
}

/// AC-2 (and AC-3 for the circle): GN and LM converge to the same parameters within 1e-12 m,
/// both match the 50-digit reference within 1e-12 m, and both report rank 3.
#[test]
fn ac2_circle_gn_and_lm_agree() {
    let text = load(CIRCLE);
    let (pre, _) = blocks(&text);
    let prob = Circle { xs: hexes(pre.field("x")), ys: hexes(pre.field("y")) };
    assert_eq!(prob.xs.len(), 50);
    let p0 = hexes(pre.field("start"));
    let reference = decimals(pre.field("ref"));
    let mut rows = Vec::new();
    let mut ps = Vec::new();
    for (tag, method) in [("gn", Method::GaussNewton), ("lm", Method::LevenbergMarquardt)] {
        let cfg = NllsConfig { method, ..NllsConfig::default() };
        let res = solve(&prob, &p0, &cfg).unwrap_or_else(|e| panic!("{tag}: {e:?}"));
        rows.push(check_run(tag, &res, pre.field(tag)));
        assert_eq!(res.rank, 3, "{tag}");
        for (k, (&g, &w)) in res.p.iter().zip(&reference).enumerate() {
            assert!((g - w).abs() <= 1e-12, "{tag}: parameter {k} off the reference by {:e} m", (g - w).abs());
        }
        assert_eq!(res.lambda.is_some(), method == Method::LevenbergMarquardt);
        ps.push(res.p);
    }
    for k in 0..3 {
        assert!((ps[0][k] - ps[1][k]).abs() <= 1e-12, "GN and LM differ in parameter {k}");
    }
    assert_eq!(digest(&rows), pre.field("digest"));
}

/// Straight line `b0 + b1 x + b2 x`: the last two Jacobian columns are identical.
struct Duplicate;

impl NllsProblem for Duplicate {
    fn n_params(&self) -> usize {
        3
    }
    fn n_resid(&self) -> usize {
        5
    }
    fn eval(&self, p: &[f64], r: &mut [f64], j: &mut [f64]) -> Result<(), NumError> {
        let ys = [2.1, 3.9, 6.2, 7.8, 10.1];
        for (i, (ri, y)) in r.iter_mut().zip(ys).enumerate() {
            let x = (i + 1) as f64;
            *ri = ((p[0] + p[1] * x) + p[2] * x) - y;
            j[3 * i..3 * i + 3].copy_from_slice(&[1.0, x, x]);
        }
        Ok(())
    }
}

/// AC-4: identical Jacobian columns give `RankDeficient { rank: 2, .. }` and no solution, for
/// both methods.
#[test]
fn ac4_identical_columns_are_rank_deficient() {
    for method in [Method::GaussNewton, Method::LevenbergMarquardt] {
        let cfg = NllsConfig { method, ..NllsConfig::default() };
        match solve(&Duplicate, &[0.0, 0.5, 0.5], &cfg) {
            Err(SolverError::RankDeficient { rank, last, .. }) => {
                assert_eq!(rank, 2);
                assert_eq!(last.rank, Some(2));
                assert_eq!(last.evaluations, 1);
                assert_eq!(last.iterations, 0);
                assert!(last.cost.is_some());
            }
            other => panic!("{method:?}: {other:?}"),
        }
    }
}

/// AC-4: `max_iter = 1` on misra1a from start 1 is `IterationLimit` with the cost after the one
/// accepted step (golden value from the Python model).
#[test]
fn ac4_iteration_limit_reports_last_cost() {
    let text = load(STRD);
    let (_, probs) = blocks(&text);
    let b = probs.iter().find(|b| b.head.starts_with("misra1a ")).unwrap();
    let prob = curve(b);
    let cfg = NllsConfig { max_iter: 1, ..NllsConfig::default() };
    let t: Vec<&str> = b.field("iterlimit_start1").split_whitespace().collect();
    match solve(&prob, &hexes(b.field("start1")), &cfg) {
        Err(e @ SolverError::IterationLimit { .. }) => {
            let last = e.last();
            assert_eq!(last.iterations, 1);
            assert_eq!(last.cost.unwrap().to_bits(), hex(t[1]).to_bits());
            assert_eq!(last.evaluations.to_string(), t[3]);
            assert!(last.lambda.is_some() && last.rank == Some(2));
        }
        other => panic!("{other:?}"),
    }
}

/// `y = 2 + 3x` with exact data, started at the solution: zero residual, no step.
struct Line;

impl NllsProblem for Line {
    fn n_params(&self) -> usize {
        2
    }
    fn n_resid(&self) -> usize {
        4
    }
    fn eval(&self, p: &[f64], r: &mut [f64], j: &mut [f64]) -> Result<(), NumError> {
        for (i, ri) in r.iter_mut().enumerate() {
            let x = i as f64;
            *ri = (p[0] + p[1] * x) - (2.0 + 3.0 * x);
            j[2 * i..2 * i + 2].copy_from_slice(&[1.0, x]);
        }
        Ok(())
    }
}

#[test]
fn zero_residual_stops_without_a_step() {
    let res = solve(&Line, &[2.0, 3.0], &NllsConfig::default()).unwrap();
    assert_eq!(res.stop_reason, StopReason::ZeroResidual);
    assert_eq!((res.iterations, res.evaluations, res.cost.to_bits()), (0, 1, 0));
    assert_eq!(res.lambda, None);
}

#[test]
fn gauss_newton_solves_a_linear_problem_in_one_step() {
    let cfg = NllsConfig { method: Method::GaussNewton, ..NllsConfig::default() };
    let res = solve(&Line, &[0.0, 0.0], &cfg).unwrap();
    assert!((res.p[0] - 2.0).abs() <= 1e-13 && (res.p[1] - 3.0).abs() <= 1e-13, "{:?}", res.p);
    assert!(res.iterations <= 2, "{}", res.iterations);
}

/// A problem whose evaluation misbehaves at the start in a chosen way.
struct Bad(u8);

impl NllsProblem for Bad {
    fn n_params(&self) -> usize {
        1
    }
    fn n_resid(&self) -> usize {
        3
    }
    fn eval(&self, p: &[f64], r: &mut [f64], j: &mut [f64]) -> Result<(), NumError> {
        r.copy_from_slice(&[p[0] - 1.0, p[0], p[0] + 1.0]);
        j.copy_from_slice(&[1.0, 1.0, 1.0]);
        match self.0 {
            1 => r[1] = f64::NAN,
            2 => j[2] = f64::INFINITY,
            3 => return Err(NumError::Domain { op: "test.eval", detail: "outside the model" }),
            4 => r[0] = f64::MAX,
            _ => {}
        }
        Ok(())
    }
}

#[test]
fn callback_failures_are_typed_errors() {
    let cfg = NllsConfig::default();
    let e = solve(&Bad(1), &[3.0], &cfg).unwrap_err();
    assert!(matches!(e, SolverError::NonFinite { stage: Stage::Residual, index: Some(1), .. }), "{e:?}");
    let e = solve(&Bad(2), &[3.0], &cfg).unwrap_err();
    assert!(matches!(e, SolverError::NonFinite { stage: Stage::Jacobian, index: Some(2), .. }), "{e:?}");
    let e = solve(&Bad(3), &[3.0], &cfg).unwrap_err();
    assert!(matches!(e, SolverError::Numeric { error: NumError::Domain { .. }, .. }), "{e:?}");
    assert_eq!(e.last().evaluations, 1);
    let e = solve(&Bad(4), &[3.0], &cfg).unwrap_err();
    assert!(matches!(e, SolverError::NonFinite { stage: Stage::Cost, .. }), "{e:?}");
    // The well-behaved variant converges to p = 0.
    let res = solve(&Bad(0), &[3.0], &cfg).unwrap();
    assert!(res.p[0].abs() <= 1e-9, "{:?}", res.p);
}

#[test]
fn invalid_inputs_are_rejected_before_any_evaluation() {
    let d = NllsConfig::default();
    let cases: [(NllsConfig, &str); 10] = [
        (NllsConfig { max_iter: MAX_ITER + 1, ..d }, "max_iter"),
        (NllsConfig { tol_grad: f64::NAN, ..d }, "tol_grad"),
        (NllsConfig { tol_grad: -1e-10, ..d }, "tol_grad"),
        (NllsConfig { tol_step: f64::INFINITY, ..d }, "tol_step"),
        (NllsConfig { tol_cost: -0.5, ..d }, "tol_cost"),
        (NllsConfig { cond_limit: 0.5, ..d }, "cond_limit"),
        (NllsConfig { cond_limit: f64::NAN, ..d }, "cond_limit"),
        (NllsConfig { rank_tol: 1.0, ..d }, "rank_tol"),
        (NllsConfig { lm_tau: 0.0, ..d }, "lm_tau"),
        (NllsConfig { lm_tau: f64::NAN, ..d }, "lm_tau"),
    ];
    for (cfg, want) in cases {
        match solve(&Line, &[0.0, 0.0], &cfg) {
            Err(SolverError::InvalidConfig { field, last }) => {
                assert_eq!(field, want);
                assert_eq!(last.evaluations, 0);
            }
            other => panic!("{want}: {other:?}"),
        }
    }
    assert!(matches!(solve(&Line, &[0.0], &d), Err(SolverError::InvalidConfig { field: "p0", .. })));
    let e = solve(&Line, &[0.0, f64::NAN], &d).unwrap_err();
    assert!(matches!(e, SolverError::NonFinite { stage: Stage::Start, index: Some(1), .. }), "{e:?}");

    struct Shape(usize, usize);
    impl NllsProblem for Shape {
        fn n_params(&self) -> usize {
            self.0
        }
        fn n_resid(&self) -> usize {
            self.1
        }
        fn eval(&self, _: &[f64], _: &mut [f64], _: &mut [f64]) -> Result<(), NumError> {
            panic!("must not be evaluated")
        }
    }
    let field = |s: Shape, p0: &[f64]| match solve(&s, p0, &d) {
        Err(SolverError::InvalidConfig { field, .. }) => field,
        other => panic!("{other:?}"),
    };
    assert_eq!(field(Shape(0, 3), &[]), "n_params");
    assert_eq!(field(Shape(3, 2), &[0.0; 3]), "n_resid");
    assert_eq!(field(Shape(2, usize::MAX), &[0.0; 2]), "n_resid");
    assert_eq!(field(Shape(5000, 6000), &[]), "n_params");
}

#[test]
fn repeated_runs_are_bit_identical() {
    let text = load(STRD);
    let (_, probs) = blocks(&text);
    let b = probs.iter().find(|b| b.head.starts_with("lanczos3 ")).unwrap();
    let prob = curve(b);
    let p0 = hexes(b.field("start2"));
    let a = solve(&prob, &p0, &NllsConfig::default()).unwrap();
    let c = solve(&prob, &p0, &NllsConfig::default()).unwrap();
    assert_eq!(a, c);
}

/// AC-5: the driver source uses no FMA, no `powi`, no `f64::min`/`max`, no hash containers, no
/// threads and no `unsafe`, takes square roots only through `fid_num::f`, and never forms the
/// cross-product of the Jacobian (no transpose and no matrix product in the source).
#[test]
fn ac5_source_has_no_forbidden_constructs() {
    let sources = [include_str!("../src/nlls/mod.rs"), include_str!("../src/nlls/qr.rs")];
    let forbidden = [
        "mul_add", "powi", "powf", "f64::min", "f64::max", ".min(", ".max(", "HashMap", "HashSet", "unsafe",
        "std::thread", "spawn(", "rayon", ".sqrt()", ".exp()", ".ln()", "transpose", "mat_mul", "mat_t_vec", "jtj", "JtJ",
    ];
    for src in sources {
        for word in forbidden {
            assert!(!src.contains(word), "forbidden construct {word:?} in the nlls source");
        }
    }
}

/// AC-1 on the real NIST StRD files, which are not committed (decision D-038). Convert them
/// locally with `fixtures/solv/nist-strd-nls-v1/convert.py`, then run
/// `FID_NIST_STRD_NLS=<output dir> cargo test -p fid-solv --test nlls -- --ignored`.
/// Datasets whose model form has no implementation here are listed and skipped.
#[test]
#[ignore = "needs the NIST StRD files converted locally (D-038: not redistributed)"]
fn ac1_nist_strd_local() {
    let dir = std::env::var("FID_NIST_STRD_NLS").expect("set FID_NIST_STRD_NLS to the convert.py output");
    let mut paths: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    let known = ["misra1a", "chwirut", "danwood", "lanczos3", "kirby2", "misra1d", "roszman1", "mgh17"];
    let mut ran = 0;
    for path in paths.iter().filter(|p| p.extension().is_some_and(|e| e == "txt")) {
        let text = std::fs::read_to_string(path).unwrap().replace("
", "
");
        let (_, probs) = blocks(&text);
        for b in &probs {
            let prob = curve(b);
            if !known.contains(&prob.model.as_str()) {
                eprintln!("skipped {}: model {} not implemented in this test", path.display(), prob.model);
                continue;
            }
            let reference = decimals(b.field("ref"));
            for start in ["start1", "start2"] {
                let tag = format!("{} {start}", path.display());
                let res = solve(&prob, &decimals(b.field(start)), &NllsConfig::default())
                    .unwrap_or_else(|e| panic!("{tag}: {e:?}"));
                assert_rel(&tag, &res.p, &reference, 1e-6);
                ran += 1;
            }
        }
    }
    assert!(ran > 0, "no dataset ran");
}

//! Acceptance criteria of spec node KRN.LSQ.LIN that need no unbuilt node (AC-3, AC-4, AC-6)
//! and the error paths.
//!
//! Expected values come from `fixtures/fit/make_lin_fixtures.py`: an exact rational /
//! 80-digit reference of each fit (point, direction, RMS residual; compared here with a
//! tolerance) and a bit-exact Python model of the pinned rules of `fid_fit::ls_lin` (the
//! canonical result bytes and their SHA-256, compared exactly). CI runs this file on x86-64
//! Linux and Windows, ARM64 macOS and wasm32 under wasmtime against the same bytes, so the
//! output digests are identical across targets (AC-3).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// `FitError` is large by design (see the crate-level note in `fid-fit`).
#![allow(clippy::result_large_err)]

use fid_fit::contract::{
    encode, Constraint, Fit, FitError, FitParams, FitResult, FittedFeature, Method, PointsView, StopReason, ValueType,
    Weights,
};
use fid_fit::ls_lin::{command_rows, projection_frame, sym_eig, Line2Ls, Line3Ls, PlaneLs, PointLs, PRIMITIVES};
use fid_kci::kci::{sha256, to_hex};
use fid_kci::{check, parse_card, CardRegistry};
use fid_types::{Determinism, SideEffect};

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fit/lin-synth-v1/cases.txt");
const TRAIT_ROW: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fit/trait-rows-v1/fit.ls_plane.json");

const CARDS: [&str; 4] = [
    include_str!("../cards/fit.point.ls@1.0.0.toml"),
    include_str!("../cards/fit.line2.ls@1.0.0.toml"),
    include_str!("../cards/fit.line3.ls@1.0.0.toml"),
    include_str!("../cards/fit.plane.ls@1.0.0.toml"),
];

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

fn hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(hex).collect()
}

fn decimals(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|t| t.parse().unwrap()).collect()
}

fn v3(v: &[f64]) -> [f64; 3] {
    [v[0], v[1], v[2]]
}

struct Case {
    name: String,
    vals: Vec<(String, String)>,
}

impl Case {
    fn get(&self, k: &str) -> Option<&str> {
        self.vals.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str())
    }
    fn field(&self, k: &str) -> &str {
        self.get(k).unwrap_or_else(|| panic!("{}: no {k}", self.name))
    }
}

fn cases() -> Vec<Case> {
    let text = read(CASES);
    let mut out = Vec::new();
    let mut cur: Option<Case> = None;
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let (k, v) = line.split_once(' ').unwrap_or((line, ""));
        match k {
            "case" => cur = Some(Case { name: v.to_string(), vals: Vec::new() }),
            "end" => out.push(cur.take().unwrap()),
            _ => cur.as_mut().unwrap().vals.push((k.to_string(), v.to_string())),
        }
    }
    out
}

fn params_of(c: &Case) -> FitParams {
    params_and_keys(c).0
}

/// The parameters of a case and the command-row parameter names they set (the points, the
/// default thresholds of `FitParams::new` and each constraint).
fn params_and_keys(c: &Case) -> (FitParams, Vec<&'static str>) {
    let mut p = FitParams::new(Method::LeastSquares);
    let mut keys = vec!["points", "thresholds.rank_tol", "thresholds.min_coverage", "thresholds.eig_ratio_max"];
    if let Some(f) = c.get("fixed") {
        p.constraints.push(Constraint::FixedAxisDirection(v3(&hexes(f))));
        keys.push("constraints.fixed_axis_direction");
    }
    if let Some(pp) = c.get("plane_p") {
        p.constraints.push(Constraint::ProjectionPlane { p: v3(&hexes(pp)), n: v3(&hexes(c.field("plane_n"))) });
        keys.extend(["constraints.projection_plane.p", "constraints.projection_plane.n"]);
    }
    (p, keys)
}

fn run(prim: &str, pts: &PointsView<'_>, params: &FitParams, seed: Option<u64>) -> Result<FitResult, FitError> {
    match prim {
        "point" => PointLs.fit(pts, params, seed),
        "line2" => Line2Ls.fit(pts, params, seed),
        "line3" => Line3Ls.fit(pts, params, seed),
        "plane" => PlaneLs.fit(pts, params, seed),
        other => panic!("unknown primitive {other}"),
    }
}

fn point_and_direction(f: &FittedFeature) -> (Vec<f64>, Option<Vec<f64>>) {
    match f {
        FittedFeature::Point { c } => (c.to_vec(), None),
        FittedFeature::Line2 { p, d } => (p.to_vec(), Some(d.to_vec())),
        FittedFeature::Line3 { p, d } => (p.to_vec(), Some(d.to_vec())),
        FittedFeature::Plane { p, n } => (p.to_vec(), Some(n.to_vec())),
        other => panic!("unexpected feature {other:?}"),
    }
}

fn max_diff(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, |m, x| if x > m { x } else { m })
}

/// AC-3: every fixture case 1 km from the origin reproduces the bit-exact model's canonical
/// bytes and SHA-256 (identical on every CI target), and agrees with the exact reference.
#[test]
fn ac3_far_from_origin_bytes_digests_and_accuracy() {
    let all = cases();
    assert_eq!(all.len(), 14);
    let mut ok = 0;
    for c in &all {
        if c.field("expect") != "ok" {
            continue;
        }
        let prim = c.field("primitive");
        let xyz = hexes(c.field("xyz"));
        assert_eq!(xyz.len(), 3 * c.field("n").parse::<usize>().unwrap());
        let pts = PointsView::new(&xyz, None).unwrap();
        let r = run(prim, &pts, &params_of(c), None).unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
        let bytes = encode(&r).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert_eq!(text, c.field("encoded"), "{}: canonical bytes", c.name);
        assert_eq!(format!("sha256:{}", to_hex(&sha256(&bytes))), c.field("digest"), "{}", c.name);
        // Independent of the model: the exact reference within the script's tolerances.
        let (p, d) = point_and_direction(&r.feature);
        assert!(max_diff(&p, &decimals(c.field("ref_point"))) <= 1e-12, "{}: point", c.name);
        if let Some(d) = d {
            assert!(max_diff(&d, &decimals(c.field("ref_direction"))) <= 1e-12, "{}: direction", c.name);
        }
        let rms_ref = decimals(c.field("ref_rms"))[0];
        assert!((r.stats.rms - rms_ref).abs() <= 1e-6 * rms_ref + 1e-12, "{}: rms", c.name);
        assert_eq!(r.diagnostics.iterations, 0);
        assert_eq!(r.diagnostics.stop, StopReason::ClosedForm);
        assert!(r.certificate.is_none());
        ok += 1;
    }
    assert_eq!(ok, 9);
}

/// AC-4: 200 points spread evenly on a 5 m circle 1 km out (exact, 1 nm and 1 um noise) give
/// `IllConditioned` for a 3D line, carrying the model's eigenvalue ratio; likewise a noisy
/// circle for a 2D line and a noisy cube (isotropic scatter) for a plane. The plane through
/// the exact circle is fine.
#[test]
fn ac4_ill_conditioned_cases() {
    let all = cases();
    let params = FitParams::new(Method::LeastSquares);
    let ill: Vec<&Case> = all.iter().filter(|c| c.field("expect") == "ill_conditioned").collect();
    let names: Vec<&str> = ill.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["circle-line3", "circle-1nm-line3", "circle-1um-line3", "circle-1um-line2", "cube-1um-plane"]);
    for c in ill {
        let xyz = hexes(c.field("xyz"));
        let pts = PointsView::new(&xyz, None).unwrap();
        match run(c.field("primitive"), &pts, &params, None) {
            Err(FitError::IllConditioned { cond: Some(k) }) => {
                assert_eq!(k.to_bits(), hex(c.field("cond")).to_bits(), "{}", c.name);
                assert!(k > params.thresholds.eig_ratio_max && k <= 1.0, "{}", c.name);
            }
            other => panic!("{}: expected IllConditioned, got {other:?}", c.name),
        }
    }
    let line = all.iter().find(|c| c.name == "circle-line3").unwrap();
    let plane = all.iter().find(|c| c.name == "circle-plane").unwrap();
    assert_eq!(plane.field("xyz"), line.field("xyz"));
    let xyz = hexes(plane.field("xyz"));
    assert!(PlaneLs.fit(&PointsView::new(&xyz, None).unwrap(), &params, None).is_ok());
}

/// The clean long line and flat plane pass with a large margin (ratio below 1e-9 against the
/// limit 0.9), the ratio is reported in the diagnostics, and the threshold is what decides:
/// a limit of 0 turns the same fit into `IllConditioned`; a limit outside `[0, 1)` is refused.
#[test]
fn eig_ratio_threshold() {
    let all = cases();
    for name in ["line3-far", "plane-far", "line2-xy", "line2-tilted", "circle-plane"] {
        let c = all.iter().find(|c| c.name == name).unwrap();
        let xyz = hexes(c.field("xyz"));
        let pts = PointsView::new(&xyz, None).unwrap();
        let p = params_of(c);
        let r = run(c.field("primitive"), &pts, &p, None).unwrap();
        assert!(r.diagnostics.cond >= 0.0 && r.diagnostics.cond < 1e-9, "{name}: ratio {:e}", r.diagnostics.cond);
        let mut strict = p.clone();
        strict.thresholds.eig_ratio_max = 0.0;
        if r.diagnostics.cond > 0.0 {
            let want = Err(FitError::IllConditioned { cond: Some(r.diagnostics.cond) });
            assert_eq!(run(c.field("primitive"), &pts, &strict, None), want, "{name}");
        }
        for bad in [1.0, -0.1, f64::NAN] {
            let mut q = p.clone();
            q.thresholds.eig_ratio_max = bad;
            let want = Err(FitError::InvalidParams { field: "thresholds.eig_ratio_max" });
            assert_eq!(run(c.field("primitive"), &pts, &q, None), want, "{name}");
        }
    }
}

/// Fits are deterministic and ignore the seed (D0).
#[test]
fn seed_is_ignored_and_runs_repeat() {
    let all = cases();
    for c in all.iter().filter(|c| c.field("expect") == "ok") {
        let xyz = hexes(c.field("xyz"));
        let pts = PointsView::new(&xyz, None).unwrap();
        let p = params_of(c);
        let a = encode(&run(c.field("primitive"), &pts, &p, None).unwrap()).unwrap();
        let b = encode(&run(c.field("primitive"), &pts, &p, Some(42)).unwrap()).unwrap();
        assert_eq!(a, b, "{}", c.name);
    }
}

/// Hand-labelled: a perfect plane z = 0 on a grid has normal (0, 0, 1) bit for bit and zero
/// residuals (the scatter has an exact zero row, so no rotation touches z).
#[test]
fn exact_plane_and_line() {
    let mut xyz = Vec::new();
    for i in 0..5 {
        for j in 0..4 {
            xyz.extend_from_slice(&[f64::from(i) * 0.25, f64::from(j) * 0.5, 0.0]);
        }
    }
    let pts = PointsView::new(&xyz, None).unwrap();
    let r = PlaneLs.fit(&pts, &FitParams::new(Method::LeastSquares), None).unwrap();
    let FittedFeature::Plane { p, n } = r.feature else { panic!() };
    assert_eq!(n.map(f64::to_bits), [0.0, 0.0, 1.0].map(f64::to_bits));
    assert_eq!(p, [0.5, 0.75, 0.0]);
    assert!(r.residuals.iter().all(|x| *x == 0.0));
    assert_eq!(r.diagnostics.rank, 3);

    // Points on a line parallel to x, 1 km out: direction (1, 0, 0), cond 1, zero residuals.
    let xyz: Vec<f64> = (0..7).flat_map(|k| [1000.0 + f64::from(k) * 0.125, 5.0, -3.0]).collect();
    let pts = PointsView::new(&xyz, None).unwrap();
    let r = Line3Ls.fit(&pts, &FitParams::new(Method::LeastSquares), None).unwrap();
    let FittedFeature::Line3 { p, d } = r.feature else { panic!() };
    assert_eq!(d, [1.0, 0.0, 0.0]);
    assert_eq!(p, [1000.375, 5.0, -3.0]);
    assert_eq!(r.diagnostics.cond, 0.0);
    assert_eq!(r.diagnostics.rank, 4);
    assert!(r.residuals.iter().all(|x| *x == 0.0));

    // A 2D line in the default plane: the same points project to (x, y).
    let r = Line2Ls.fit(&pts, &FitParams::new(Method::LeastSquares), None).unwrap();
    let FittedFeature::Line2 { p, d } = r.feature else { panic!() };
    assert_eq!(d, [1.0, 0.0]);
    assert_eq!(p, [1000.375, 5.0]);
    assert!(r.residuals.iter().all(|x| *x == 0.0));
}

/// REQ-001: the point fit returns the centroid and distances from it.
#[test]
fn point_is_centroid_with_distances() {
    let xyz = [1.0, 2.0, 2.0, -1.0, -2.0, -2.0, 0.0, 0.0, 3.0, 0.0, 0.0, -3.0];
    let pts = PointsView::new(&xyz, None).unwrap();
    let r = PointLs.fit(&pts, &FitParams::new(Method::LeastSquares), None).unwrap();
    assert_eq!(r.feature, FittedFeature::Point { c: [0.0, 0.0, 0.0] });
    assert_eq!(r.residuals, vec![3.0, 3.0, 3.0, 3.0]);
    assert_eq!((r.diagnostics.rank, r.diagnostics.cond, r.diagnostics.grad_norm), (3, 0.0, 0.0));
    assert_eq!(r.algorithm.to_string(), "fit.point.ls@1.0.0");

    let one = [7.0, -8.0, 9.0];
    let r = PointLs.fit(&PointsView::new(&one, None).unwrap(), &FitParams::new(Method::LeastSquares), None).unwrap();
    assert_eq!(r.feature, FittedFeature::Point { c: one });
    assert_eq!(r.residuals, vec![0.0]);
}

/// Line2 projection frame (rule 7): the default plane gives the x / y frame; a tilted plane
/// gives an orthonormal right-handed frame.
#[test]
fn projection_frame_rule() {
    let (p0, e1, e2) = projection_frame(&FitParams::new(Method::LeastSquares)).unwrap();
    assert_eq!((p0, e1, e2), ([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
    let mut p = FitParams::new(Method::LeastSquares);
    p.constraints.push(Constraint::ProjectionPlane { p: [1.0, 2.0, 3.0], n: [0.0, 0.0, -2.0] });
    let (p0, e1, e2) = projection_frame(&p).unwrap();
    assert_eq!((p0, e1, e2), ([1.0, 2.0, 3.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
    let mut p = FitParams::new(Method::LeastSquares);
    p.constraints.push(Constraint::ProjectionPlane { p: [0.0; 3], n: [1.0, 2.0, 2.0] });
    let (_, e1, e2) = projection_frame(&p).unwrap();
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let n = [1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0];
    assert!(dot(e1, n).abs() < 1e-15 && dot(e2, n).abs() < 1e-15 && dot(e1, e2).abs() < 1e-15);
    let cross = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
    assert!(max_diff(&cross, &n) < 1e-15, "e1 x e2 = n");
}

/// Error paths: point counts and degeneracy from the contract checks, parameter refusals.
#[test]
fn error_paths() {
    let ls = || FitParams::new(Method::LeastSquares);
    let one = [1.0, 2.0, 3.0];
    let one = PointsView::new(&one, None).unwrap();
    assert_eq!(Line3Ls.fit(&one, &ls(), None), Err(FitError::TooFewPoints { n: 1, need: 2 }));
    assert_eq!(Line2Ls.fit(&one, &ls(), None), Err(FitError::TooFewPoints { n: 1, need: 2 }));
    assert_eq!(PlaneLs.fit(&one, &ls(), None), Err(FitError::TooFewPoints { n: 1, need: 3 }));
    let empty = PointsView::new(&[], None).unwrap();
    assert_eq!(PointLs.fit(&empty, &ls(), None), Err(FitError::TooFewPoints { n: 0, need: 1 }));

    // A fixed direction makes one point enough; none is still too few.
    let mut fixed = ls();
    fixed.constraints.push(Constraint::FixedAxisDirection([0.0, 0.0, -1.0]));
    let r = Line3Ls.fit(&one, &fixed, None).unwrap();
    assert_eq!(r.feature, FittedFeature::Line3 { p: [1.0, 2.0, 3.0], d: [0.0, 0.0, 1.0] });
    assert_eq!((r.diagnostics.rank, r.diagnostics.cond), (2, 0.0));
    let r = PlaneLs.fit(&one, &fixed, None).unwrap();
    assert_eq!(r.feature, FittedFeature::Plane { p: [1.0, 2.0, 3.0], n: [0.0, 0.0, 1.0] });
    assert_eq!(r.diagnostics.rank, 1);
    assert_eq!(Line3Ls.fit(&empty, &fixed, None), Err(FitError::TooFewPoints { n: 0, need: 1 }));

    // Collinear points have no plane; coincident points no line.
    let col: Vec<f64> = (0..10).flat_map(|k| [f64::from(k), 2.0 * f64::from(k), 1.0]).collect();
    let col = PointsView::new(&col, None).unwrap();
    assert!(matches!(PlaneLs.fit(&col, &ls(), None), Err(FitError::Collinear { n: 10, rank: 1, .. })));
    let same = [4.0, 5.0, 6.0, 4.0, 5.0, 6.0, 4.0, 5.0, 6.0];
    let same = PointsView::new(&same, None).unwrap();
    assert!(matches!(Line3Ls.fit(&same, &ls(), None), Err(FitError::Collinear { n: 3, rank: 0, .. })));

    // Parameter refusals.
    let tri = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    let tri = PointsView::new(&tri, None).unwrap();
    let bad = |field| Err(FitError::InvalidParams { field });
    let mut p = ls();
    p.method = Method::MinimumZone;
    assert_eq!(PlaneLs.fit(&tri, &p, None), bad("method"));
    let mut p = ls();
    p.weights = Weights::PerPoint(vec![1.0, 2.0, 3.0]);
    assert_eq!(PlaneLs.fit(&tri, &p, None), bad("weights"));
    let mut p = ls();
    p.constraints.push(Constraint::FixedRadius(1.0));
    assert_eq!(Line3Ls.fit(&tri, &p, None), bad("constraints"));
    let mut p = ls();
    p.constraints.push(Constraint::ProjectionPlane { p: [0.0; 3], n: [0.0, 0.0, 1.0] });
    assert_eq!(PlaneLs.fit(&tri, &p, None), bad("constraints"));
    assert!(Line2Ls.fit(&tri, &p, None).is_ok());
    let mut p = ls();
    p.constraints.push(Constraint::FixedAxisDirection([1.0, 0.0, 0.0]));
    assert_eq!(PointLs.fit(&tri, &p, None), bad("constraints"));
    // A fixed line2 direction along the plane normal has no in-plane direction.
    let mut p = ls();
    p.constraints.push(Constraint::FixedAxisDirection([0.0, 0.0, 1.0]));
    assert_eq!(Line2Ls.fit(&tri, &p, None), bad("constraints.fixed_axis_direction"));
    // The contract's validation runs first.
    let mut p = ls();
    p.thresholds.rank_tol = 2.0;
    assert_eq!(Line3Ls.fit(&tri, &p, None), bad("thresholds.rank_tol"));
}

/// The interim Jacobi (`KRN.NUM.LA.EIG` rules): identity (all eigenvalues tied) gives the unit
/// basis in order with no sweep; a 2 x 2 case; size and finiteness refusals.
#[test]
fn sym_eig_cases() {
    let e = sym_eig(&[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]).unwrap();
    assert_eq!(e.values, [1.0; 3]);
    assert_eq!(e.vectors, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    assert_eq!(e.sweeps, 0);
    // [[2, 1], [1, 2]]: theta = 0, t = 1, c = s = 1/sqrt(2); eigenvalues 1 and 3.
    let e = sym_eig(&[[2.0, 1.0], [1.0, 2.0]]).unwrap();
    assert_eq!(e.values, [1.0, 3.0]);
    let h = core::f64::consts::FRAC_1_SQRT_2;
    assert!(max_diff(&e.vectors[0], &[h, -h]) < 1e-15 && max_diff(&e.vectors[1], &[h, h]) < 1e-15);
    // Only the upper triangle is read.
    let e = sym_eig(&[[2.0, 1.0], [99.0, 2.0]]).unwrap();
    assert_eq!(e.values, [1.0, 3.0]);
    assert_eq!(sym_eig(&[[1.0; 4]; 4]), Err(FitError::InvalidParams { field: "eig.size" }));
    assert!(matches!(sym_eig(&[[1.0, f64::NAN], [0.0, 1.0]]), Err(FitError::Num(_))));
}

/// AC-6 and REQ-KRN.LSQ-009: each algorithm ID has a spec card that parses, and the card
/// check of `KRN.MAN.CARD` between the IDs and the cards is clean.
#[test]
fn ac6_spec_cards() {
    let cards: Vec<_> = CARDS.iter().map(|t| parse_card(t).unwrap()).collect();
    let card_ids: Vec<_> = cards.iter().map(|c| c.id.clone()).collect();
    let table_ids =
        vec![PointLs.algorithm_id(), Line2Ls.algorithm_id(), Line3Ls.algorithm_id(), PlaneLs.algorithm_id()];
    let names: Vec<String> = table_ids.iter().map(|i| i.to_string()).collect();
    assert_eq!(names, ["fit.point.ls@1.0.0", "fit.line2.ls@1.0.0", "fit.line3.ls@1.0.0", "fit.plane.ls@1.0.0"]);
    let report = check(&table_ids, &card_ids);
    assert!(report.is_clean(), "{report:?}");
    assert!(cards.iter().all(|c| c.determinism == "D0"));
    let reg = CardRegistry::new(cards).unwrap();
    assert_eq!(reg.ids().len(), 4);
}

/// The four command data rows: W2 with R1 dry run, D0, every numeric field with a quantity
/// and unit; the plane row is byte-identical to the contract's fixture row.
#[test]
fn command_rows_validate() {
    let rows = command_rows().unwrap();
    assert_eq!(rows.len(), PRIMITIVES.len());
    let names: Vec<&str> = rows.iter().map(|r| r.command.as_str()).collect();
    assert_eq!(names, ["fit.ls_point@1", "fit.ls_line2@1", "fit.ls_line3@1", "fit.ls_plane@1"]);
    for r in &rows {
        r.validate().unwrap();
        assert_eq!((r.side_effect, r.dry_run, r.determinism), (SideEffect::W2, SideEffect::R1, Determinism::D0));
    }
    let want = read(TRAIT_ROW);
    assert_eq!(String::from_utf8(rows[3].to_json().unwrap()).unwrap(), want.trim_end());
}

/// A minimal reader for the canonical result JSON (`contract::encode`): strings without
/// escapes, arrays, objects and `null`, which is all that encoding writes.
#[derive(Debug)]
enum J {
    S(String),
    A(Vec<J>),
    O(Vec<(String, J)>),
    Null,
}

fn parse_json(b: &[u8], i: &mut usize) -> J {
    match b[*i] {
        b'"' => {
            let start = *i + 1;
            let end = start + b[start..].iter().position(|&c| c == b'"').unwrap();
            *i = end + 1;
            J::S(String::from_utf8(b[start..end].to_vec()).unwrap())
        }
        b'[' => {
            *i += 1;
            let mut v = Vec::new();
            while b[*i] != b']' {
                v.push(parse_json(b, i));
                if b[*i] == b',' {
                    *i += 1;
                }
            }
            *i += 1;
            J::A(v)
        }
        b'{' => {
            *i += 1;
            let mut v = Vec::new();
            while b[*i] != b'}' {
                let J::S(k) = parse_json(b, i) else { panic!("key") };
                assert_eq!(b[*i], b':');
                *i += 1;
                v.push((k, parse_json(b, i)));
                if b[*i] == b',' {
                    *i += 1;
                }
            }
            *i += 1;
            J::O(v)
        }
        b'n' => {
            assert_eq!(&b[*i..*i + 4], b"null");
            *i += 4;
            J::Null
        }
        c => panic!("unexpected byte {c}"),
    }
}

/// Leaves of the result object with dotted names (`feature.p`, `stats.rms`, ...).
fn flatten<'a>(prefix: &str, j: &'a J, out: &mut Vec<(String, &'a J)>) {
    match j {
        J::O(members) => {
            for (k, v) in members {
                let name = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten(&name, v, out);
            }
        }
        _ => out.push((prefix.to_string(), j)),
    }
}

fn is_hex_f64(j: &J) -> bool {
    matches!(j, J::S(s) if s.len() == 16 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
}

fn is_dec_int(j: &J) -> bool {
    matches!(j, J::S(s) if !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) && (s == "0" || !s.starts_with('0')))
}

/// The wire form of each command-layer value type (the note for CORE.REG in
/// `contract::rows`): number = 16-hex-digit IEEE-754 bit string, integer = decimal string,
/// arrays of those, points = array of 3-element number arrays, string = JSON string.
fn wire_matches(ty: ValueType, j: &J) -> bool {
    match (ty, j) {
        (ValueType::Number, _) => is_hex_f64(j),
        (ValueType::Integer, _) => is_dec_int(j),
        (ValueType::NumberArray, J::A(v)) => v.iter().all(is_hex_f64),
        (ValueType::IntegerArray, J::A(v)) => v.iter().all(is_dec_int),
        (ValueType::Points, J::A(v)) => {
            v.iter().all(|p| matches!(p, J::A(c) if c.len() == 3 && c.iter().all(is_hex_f64)))
        }
        (ValueType::String, J::S(_)) => true,
        _ => false,
    }
}

/// For each primitive, a real `FitResult` (from the fixture) flattened from its canonical
/// bytes carries every required result field of its command row with the declared type, and
/// nothing undeclared except `feature.kind` and a `null` certificate; every parameter a case
/// sets is a declared parameter of the row.
#[test]
fn real_results_match_command_rows() {
    let all = cases();
    let rows = command_rows().unwrap();
    for (row, case) in rows.iter().zip(["point-far", "line2-xy", "line3-far", "plane-far"]) {
        let c = all.iter().find(|c| c.name == case).unwrap();
        let xyz = hexes(c.field("xyz"));
        let r = run(c.field("primitive"), &PointsView::new(&xyz, None).unwrap(), &params_of(c), None).unwrap();
        assert_eq!(row.algorithm, r.algorithm);
        let bytes = encode(&r).unwrap();
        let mut i = 0;
        let json = parse_json(&bytes, &mut i);
        assert_eq!(i, bytes.len());
        let mut leaves = Vec::new();
        flatten("", &json, &mut leaves);
        for f in &row.result {
            match leaves.iter().find(|(k, _)| k == f.name) {
                Some((_, j)) => assert!(wire_matches(f.ty, j), "{case}: {} is not a {:?}: {j:?}", f.name, f.ty),
                None => assert!(!f.required, "{case}: required field {} missing", f.name),
            }
        }
        for (k, j) in &leaves {
            let declared = row.result.iter().any(|f| f.name == k);
            let allowed = k == "feature.kind" || (k == "certificate" && matches!(j, J::Null));
            assert!(declared || allowed, "{case}: undeclared result field {k}");
        }
    }
    for c in all.iter() {
        for k in params_and_keys(c).1 {
            assert!(rows[0].params.iter().any(|f| f.name == k), "{}: undeclared param {k}", c.name);
        }
    }
}

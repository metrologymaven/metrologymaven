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

use fid_fit::contract::{
    encode, Constraint, Fit, FitError, FitParams, FitResult, FittedFeature, Method, PointsView, StopReason, Weights,
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
    let mut p = FitParams::new(Method::LeastSquares);
    if let Some(f) = c.get("fixed") {
        p.constraints.push(Constraint::FixedAxisDirection(v3(&hexes(f))));
    }
    if let Some(pp) = c.get("plane_p") {
        p.constraints.push(Constraint::ProjectionPlane { p: v3(&hexes(pp)), n: v3(&hexes(c.field("plane_n"))) });
    }
    p
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
    assert_eq!(all.len(), 10);
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

/// AC-4: 200 points spread evenly on a circle give `IllConditioned` for a 3D line, carrying
/// the eigenvalue-gap condition of the model; the plane through the same points is fine.
#[test]
fn ac4_circle_line3_is_ill_conditioned() {
    let all = cases();
    let c = all.iter().find(|c| c.name == "circle-line3").unwrap();
    assert_eq!(c.field("expect"), "ill_conditioned");
    let xyz = hexes(c.field("xyz"));
    let pts = PointsView::new(&xyz, None).unwrap();
    let params = FitParams::new(Method::LeastSquares);
    match Line3Ls.fit(&pts, &params, None) {
        Err(FitError::IllConditioned { cond: Some(k) }) => {
            assert_eq!(k.to_bits(), hex(c.field("cond")).to_bits());
            assert!(k > params.nlls.cond_limit);
        }
        other => panic!("expected IllConditioned, got {other:?}"),
    }
    let plane = all.iter().find(|c| c.name == "circle-plane").unwrap();
    assert_eq!(plane.field("xyz"), c.field("xyz"));
    assert!(PlaneLs.fit(&pts, &params, None).is_ok());
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
    assert_eq!(r.diagnostics.cond, 1.0);
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
    assert_eq!((r.diagnostics.rank, r.diagnostics.cond, r.diagnostics.grad_norm), (3, 1.0, 0.0));
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
    assert_eq!((r.diagnostics.rank, r.diagnostics.cond), (2, 1.0));
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

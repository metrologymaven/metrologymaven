//! GEO.CAD.TOPO.PCURVE acceptance tests. Expected values come from the independent 60-digit
//! oracle and the bit-exact model in `fixtures/geo/cad/topo/make_pcurve_fixtures.py`
//! (`pcurve_expected.json`); the NURBS cases are hand-labelled from their geometry.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{build, builder, hex, load, J};
use fid_geo_cad::{
    derive_coedge, derive_pcurves, pcurve_disagreement, Body, BodyBuilder, CoedgeIx, CurveDef, CurveParams,
    DisagreementError, EntityIx, FailReason, Frame, InversionFault, Label, PcurveError, Pcurves, Sense, SurfaceDef,
    TopologyId, TrimCurve, DEFAULT_TOL_M,
};
use fid_geo_nurbs::{invert_surface, Curve, NurbsSurface, NurbsSurfaceParts, Status, Surface};

const FIXTURES: [&str; 9] = [
    "plate_with_hole_nopcurve",
    "plate_with_hole_offset",
    "cylinder_band",
    "cylinder_band_pcurves",
    "cylinder_band_pcurve_off",
    "sphere_meridian",
    "sphere_pole_straddle",
    "cone_apex_sector",
    "torus_tubes",
];

fn expected() -> J {
    load("pcurve_expected.json")
}

fn label(body: &Body, ix: EntityIx) -> String {
    match body.id_of(ix).unwrap() {
        TopologyId::Imported(l) => l.as_str().to_string(),
        TopologyId::Derived(_) => panic!("fixture entities are labelled"),
    }
}

fn coedge(body: &Body, name: &str) -> CoedgeIx {
    match body.lookup(&TopologyId::Imported(Label::new(name).unwrap())) {
        Some(EntityIx::Coedge(c)) => c,
        other => panic!("{name}: {other:?}"),
    }
}

/// Every coedge index of the body (canonical order), through the loops.
fn coedges(body: &Body) -> Vec<CoedgeIx> {
    let mut v: Vec<CoedgeIx> = body.loops().iter().flat_map(|l| l.coedges().iter().copied()).collect();
    v.sort();
    v
}

fn derive(body: &Body) -> Pcurves {
    let p = derive_pcurves(body, DEFAULT_TOL_M).unwrap();
    assert_eq!(p.curves.len(), body.coedges().len());
    assert_eq!(p.charts.len(), body.faces().len());
    p
}

/// The chart offset of the loop holding coedge `c`.
fn offset(body: &Body, p: &Pcurves, c: CoedgeIx) -> [f64; 2] {
    let lx = body.coedge(c).unwrap().r#loop();
    let fx = body.r#loop(lx).unwrap().face();
    let j = body.face(fx).unwrap().loops().iter().position(|&l| l == lx).unwrap();
    p.charts[fx.index()].loop_offsets[j]
}

fn h(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

fn reason(r: FailReason) -> String {
    match r {
        FailReason::NoEdgeCurve => "NoEdgeCurve".into(),
        FailReason::CurveEval => "CurveEval".into(),
        FailReason::SurfaceEval => "SurfaceEval".into(),
        FailReason::Tolerance => "Tolerance".into(),
        FailReason::Inversion(InversionFault::NonUnique) => "Inversion:NonUnique".into(),
        FailReason::Inversion(InversionFault::Tie) => "Inversion:Tie".into(),
        FailReason::Inversion(InversionFault::Unresolved(r)) => format!("Inversion:Unresolved:{r:?}"),
        FailReason::Inversion(InversionFault::Error(c)) => format!("Inversion:Error:{c}"),
    }
}

/// The pinned digest text of one coedge (see the fixture script).
fn coedge_text(r: &Result<TrimCurve, PcurveError>) -> String {
    match r {
        Ok(TrimCurve::Imported(_)) => "I\n".into(),
        Ok(TrimCurve::Derived { uv, t, max_dev_m }) => {
            let mut s = format!("D {}\n", uv.len());
            for (p, t) in uv.iter().zip(t) {
                s += &format!("{} {} {}\n", h(*t), h(p[0]), h(p[1]));
            }
            s + &format!("m {}\n", h(*max_dev_m))
        }
        Err(PcurveError::PcurveFailed { best_deviation_m, reason: r, .. }) => {
            format!("F {} {}\n", reason(*r), h(*best_deviation_m))
        }
        Err(e) => panic!("unexpected {e}"),
    }
}

fn body_text(p: &Pcurves) -> String {
    let mut s: String = p.curves.iter().map(coedge_text).collect();
    for c in &p.charts {
        let per = |x: Option<f64>| x.map_or("-".to_string(), h);
        s += &format!("S {} {}\n", per(c.period_u), per(c.period_v));
        for (o, w) in c.loop_offsets.iter().zip(&c.loop_periods) {
            let w = w.map_or("-".to_string(), |w| format!("{} {}", w[0], w[1]));
            s += &format!("L {} {} {}\n", h(o[0]), h(o[1]), w);
        }
    }
    for w in &p.windings {
        match w {
            PcurveError::SeamWinding { r#loop, periods } => {
                s += &format!("W {} {} {}\n", r#loop.index(), periods[0], periods[1])
            }
            e => panic!("not a winding: {e}"),
        }
    }
    s
}

fn sha(s: &str) -> String {
    hex(&fid_kci::kci::sha256(s.as_bytes()))
}

fn surface_point(body: &Body, c: CoedgeIx, uv: [f64; 2]) -> [f64; 3] {
    let lx = body.coedge(c).unwrap().r#loop();
    let s = body.face(body.r#loop(lx).unwrap().face()).unwrap().surface();
    s.as_analytic().unwrap().eval(uv[0], uv[1], 0).unwrap().p
}

fn curve_point(body: &Body, c: CoedgeIx, t: f64) -> [f64; 3] {
    let e = body.edge(body.coedge(c).unwrap().edge()).unwrap();
    e.curve().unwrap().as_analytic().unwrap().eval(t, 0).unwrap().p
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    let e = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt()
}

/// REQ-002 checked from outside: every vertex and every chord midpoint maps back within the
/// tolerance, and `max_dev_m` bounds them.
fn check_within_tolerance(body: &Body, c: CoedgeIx, uv: &[[f64; 2]], t: &[f64], max_dev: f64) {
    assert!(max_dev < DEFAULT_TOL_M);
    for (p, &t) in uv.iter().zip(t) {
        let d = dist(surface_point(body, c, *p), curve_point(body, c, t));
        assert!(d <= max_dev * 1.000001 + 1e-17, "vertex at t {t}: {d:e}");
    }
    for k in 1..uv.len() {
        let mid = [(uv[k - 1][0] + uv[k][0]) * 0.5, (uv[k - 1][1] + uv[k][1]) * 0.5];
        let d = dist(surface_point(body, c, mid), curve_point(body, c, (t[k - 1] + t[k]) * 0.5));
        assert!(d <= DEFAULT_TOL_M, "chord {k}: {d:e}");
    }
}

/// Compares one fixture with the oracle and the model digests.
fn check_fixture(name: &str) -> (Body, Pcurves) {
    let body = build(name);
    let p = derive(&body);
    let e = expected();
    let fx = e.get("fixtures").get(name);
    for c in coedges(&body) {
        let lab = label(&body, EntityIx::Coedge(c));
        let x = fx.get("coedges").get(&lab);
        let r = &p.curves[c.index()];
        assert_eq!(sha(&coedge_text(r)), x.get("digest").str(), "{name} {lab}: model digest (AC-5)");
        match x.get("status").str() {
            "derived" => {
                let Ok(TrimCurve::Derived { uv, t, max_dev_m }) = r else { panic!("{name} {lab}: {r:?}") };
                assert_eq!(uv.len(), x.get("vertices").u(), "{name} {lab}");
                let segments = t.windows(2).filter(|w| w[0].to_bits() != w[1].to_bits()).count();
                assert_eq!(segments, x.get("segments").u(), "{name} {lab}");
                assert!(segments <= 1 << 16);
                assert!((max_dev_m - x.get("max_dev_m").f()).abs() <= 1e-15, "{name} {lab}: max_dev {max_dev_m:e}");
                // Charted uv at the 17 initial samples (duplicates at poles in order).
                let off = offset(&body, &p, c);
                let mut from = 0;
                for s in x.get("samples").arr() {
                    let tb = u64::from_str_radix(s.get("t").str(), 16).unwrap();
                    let i = from + t[from..].iter().position(|v| v.to_bits() == tb).unwrap();
                    let want = s.get("uv").v2();
                    for k in 0..2 {
                        let got = uv[i][k] + off[k];
                        assert!((got - want[k]).abs() <= 1e-12, "{name} {lab} t {}: {got} vs {}", t[i], want[k]);
                    }
                    from = i + 1;
                }
                check_within_tolerance(&body, c, uv, t, *max_dev_m);
                // REQ-004: consecutive vertices differ by less than half a period, except at a
                // pole duplicate (same t), where the singular coordinate jumps by design.
                let ch = &p.charts[body.r#loop(body.coedge(c).unwrap().r#loop()).unwrap().face().index()];
                for k in 1..uv.len() {
                    if t[k].to_bits() == t[k - 1].to_bits() {
                        continue;
                    }
                    for (i, per) in [ch.period_u, ch.period_v].into_iter().enumerate() {
                        if let Some(per) = per {
                            assert!((uv[k][i] - uv[k - 1][i]).abs() < per * 0.5, "{name} {lab}: jump at {k}");
                        }
                    }
                }
            }
            "failed" => {
                let Err(PcurveError::PcurveFailed { coedge: cx, best_deviation_m, reason: r }) = r else {
                    panic!("{name} {lab}: {r:?}")
                };
                assert_eq!(*cx, c);
                assert_eq!(reason(*r), x.get("reason").str(), "{name} {lab}");
                let want = x.get("best_deviation_m").f();
                assert!(*best_deviation_m > DEFAULT_TOL_M);
                if want < 1e-4 {
                    // Pole straddle: the oracle's own refinement fails at the same segment.
                    assert!((best_deviation_m - want).abs() <= 1e-6 * want, "{name} {lab}: {best_deviation_m:e}");
                } else {
                    assert!((best_deviation_m - want).abs() <= 1e-15, "{name} {lab}: {best_deviation_m:e}");
                }
            }
            "imported" => {
                let Ok(TrimCurve::Imported(pc)) = r else { panic!("{name} {lab}: {r:?}") };
                assert_eq!(Some(pc), body.coedge(c).unwrap().pcurve(), "AC-3: returned unchanged");
                let d = pcurve_disagreement(&body, c).unwrap();
                assert!((d - x.get("disagreement_m").f()).abs() <= 1e-15, "{name} {lab}: disagreement {d:e}");
            }
            s => panic!("status {s}"),
        }
    }
    for (fi, f) in body.faces().iter().enumerate() {
        let ch = &p.charts[fi];
        for (j, &lx) in f.loops().iter().enumerate() {
            let lab = label(&body, EntityIx::Loop(lx));
            let want = fx.get("loops").get(&lab).get("periods");
            let got = ch.loop_periods[j];
            if want.is_null() {
                assert_eq!(got, None, "{name} {lab}");
            } else {
                let w = want.arr();
                assert_eq!(got, Some([w[0].f() as i64, w[1].f() as i64]), "{name} {lab}");
                let wound =
                    p.windings.iter().any(|x| matches!(x, PcurveError::SeamWinding { r#loop, .. } if *r#loop == lx));
                assert_eq!(wound, got != Some([0, 0]), "{name} {lab}: SeamWinding report");
            }
        }
    }
    assert_eq!(sha(&body_text(&p)), fx.get("body_digest").str(), "{name}: body digest (AC-5)");
    (body, p)
}

#[test]
fn every_fixture_matches_the_oracle_and_the_model() {
    for name in FIXTURES {
        check_fixture(name);
    }
}

#[test]
fn ac1_plate_with_hole_derived_within_tolerance() {
    let (body, p) = check_fixture("plate_with_hole_nopcurve");
    for c in coedges(&body) {
        let Ok(TrimCurve::Derived { max_dev_m, .. }) = &p.curves[c.index()] else { panic!() };
        assert!(*max_dev_m < 1e-8);
    }
    let Ok(TrimCurve::Derived { uv, .. }) = &p.curves[coedge(&body, "c-hole").index()] else { panic!() };
    assert_eq!(uv.len(), 4097, "16 x 2^8 segments: the sagitta first drops below 1e-8 m there");
    assert!(p.errors().is_empty());
}

#[test]
fn ac2_cylinder_band_loops_are_continuous_and_wind_once() {
    let (body, p) = check_fixture("cylinder_band");
    let ch = &p.charts[0];
    assert_eq!(ch.period_u, Some(core::f64::consts::TAU));
    assert_eq!(ch.period_v, None);
    let mut periods: Vec<[i64; 2]> = ch.loop_periods.iter().map(|x| x.unwrap()).collect();
    periods.sort();
    assert_eq!(periods, vec![[-1, 0], [1, 0]], "each loop winds once (opposite senses)");
    assert_eq!(p.windings.len(), 2);
    for w in &p.windings {
        let PcurveError::SeamWinding { periods, .. } = w else { panic!() };
        assert_eq!(periods[0].abs(), 1);
    }
    // In the face chart both loops cover u in [0, 2 pi].
    for c in coedges(&body) {
        let Ok(TrimCurve::Derived { uv, .. }) = &p.curves[c.index()] else { panic!() };
        let off = offset(&body, &p, c);
        for q in uv {
            assert!((-1e-12..=core::f64::consts::TAU + 1e-12).contains(&(q[0] + off[0])));
        }
    }
}

#[test]
fn ac3_imported_pcurves_are_returned_unchanged_without_derivation() {
    let (body, p) = check_fixture("cylinder_band_pcurves");
    // The same face with no 3-D curves at all: derivation could not run (NoEdgeCurve), yet
    // the result is identical, so it was never run.
    let mut script = load("cylinder_band_pcurves.json");
    if let J::Obj(m) = &mut script {
        if let Some(J::Arr(edges)) = m.get_mut("edges") {
            for e in edges {
                if let J::Obj(e) = e {
                    e.insert("curve".into(), J::Null);
                }
            }
        }
    }
    let bare = builder(&script).build().unwrap();
    let q = derive(&bare);
    assert_eq!(q.curves, p.curves);
    assert_eq!(q.charts, p.charts);
    for c in coedges(&bare) {
        assert!(matches!(
            derive_coedge(&bare, c, DEFAULT_TOL_M),
            Err(PcurveError::PcurveFailed { reason: FailReason::NoEdgeCurve, .. })
        ));
        assert_eq!(pcurve_disagreement(&bare, c), Err(DisagreementError::NoEdgeCurve));
        assert!(pcurve_disagreement(&body, c).unwrap() <= 1e-15);
    }
    // REQ-005: a pcurve 1 mm off its edge is measured at 1 mm.
    let (off, _) = check_fixture("cylinder_band_pcurve_off");
    let d = pcurve_disagreement(&off, coedge(&off, "c-top")).unwrap();
    assert!((d - 1e-3).abs() < 1e-12);
    // A derived coedge has nothing to compare.
    let plate = build("plate_with_hole_nopcurve");
    assert_eq!(pcurve_disagreement(&plate, coedge(&plate, "c-hole")), Err(DisagreementError::NotImported));
}

#[test]
fn ac4_off_surface_edge_fails_and_the_others_succeed() {
    let (body, p) = check_fixture("plate_with_hole_offset");
    let hole = coedge(&body, "c-hole");
    for c in coedges(&body) {
        match &p.curves[c.index()] {
            Err(PcurveError::PcurveFailed { coedge, best_deviation_m, reason }) => {
                assert_eq!(*coedge, hole);
                assert_eq!(*reason, FailReason::Tolerance);
                assert!((best_deviation_m - 1e-3).abs() < 1e-12, "{best_deviation_m:e}");
            }
            Ok(TrimCurve::Derived { max_dev_m, .. }) => {
                assert_ne!(c, hole);
                assert!(*max_dev_m < DEFAULT_TOL_M);
            }
            r => panic!("{r:?}"),
        }
    }
    assert_eq!(p.errors().len(), 1);
    // The hole loop has no chart periods; the outer loop is complete.
    assert_eq!(p.charts[0].loop_periods, vec![Some([0, 0]), None]);
}

#[test]
fn ac5_bit_identical_digests() {
    // The per-coedge and per-body digests of every fixture equal those of the bit-exact model
    // (checked in check_fixture); here the AC-1 run is repeated to show it is a pure function.
    let body = build("plate_with_hole_nopcurve");
    let a = derive(&body);
    let b = derive(&body);
    assert_eq!(body_text(&a), body_text(&b));
    assert_eq!(
        sha(&body_text(&a)),
        expected().get("fixtures").get("plate_with_hole_nopcurve").get("body_digest").str()
    );
}

#[test]
fn sphere_poles_inherit_and_duplicate() {
    let (body, p) = check_fixture("sphere_meridian");
    let Ok(TrimCurve::Derived { uv, t, .. }) = &p.curves[coedge(&body, "c-meridian").index()] else { panic!() };
    let dups: Vec<usize> = (1..t.len()).filter(|&k| t[k].to_bits() == t[k - 1].to_bits()).collect();
    assert_eq!(dups.len(), 2, "one duplicate vertex per pole");
    for k in dups {
        assert_eq!(uv[k][1].to_bits(), uv[k - 1][1].to_bits(), "same latitude");
        assert!((uv[k][1].abs() - core::f64::consts::FRAC_PI_2).abs() < 1e-12, "at a pole");
        assert_eq!(uv[k - 1][0], uv[k - 2][0], "inherits u from the left");
        assert_eq!(uv[k][0], uv[k + 1][0], "duplicate takes u from the right");
    }
    assert_eq!(p.charts[0].loop_periods, vec![Some([0, 0])], "a hemisphere boundary does not wind");
    assert!(p.windings.is_empty());
}

#[test]
fn pole_between_samples_fails_at_the_depth_limit() {
    let (body, p) = check_fixture("sphere_pole_straddle");
    match &p.curves[coedge(&body, "c-meridian").index()] {
        Err(PcurveError::PcurveFailed { reason: FailReason::Tolerance, best_deviation_m, .. }) => {
            assert!(*best_deviation_m > DEFAULT_TOL_M)
        }
        r => panic!("{r:?}"),
    }
}

#[test]
fn cone_apex_takes_the_generator_azimuth() {
    let (body, p) = check_fixture("cone_apex_sector");
    for (name, apex_first) in [("c-gen0", true), ("c-gen1", false)] {
        let Ok(TrimCurve::Derived { uv, .. }) = &p.curves[coedge(&body, name).index()] else { panic!() };
        let (apex, next) = if apex_first { (uv[0], uv[1]) } else { (uv[uv.len() - 1], uv[uv.len() - 2]) };
        assert_eq!(apex[0], next[0], "{name}: apex u inherited");
        assert!(apex[1].abs() < 1e-15, "{name}: apex at v = 0");
    }
    assert_eq!(p.charts[0].loop_periods, vec![Some([0, 0])]);
}

#[test]
fn torus_tubes_wind_in_v() {
    let (_, p) = check_fixture("torus_tubes");
    let ch = &p.charts[0];
    assert_eq!((ch.period_u, ch.period_v), (Some(core::f64::consts::TAU), Some(core::f64::consts::TAU)));
    assert_eq!(ch.loop_periods, vec![Some([0, 1]), Some([0, -1])]);
}

#[test]
fn bad_tolerance_is_refused() {
    let body = build("cylinder_band");
    for tol in [0.0, -1e-8, f64::NAN, f64::INFINITY] {
        assert!(matches!(derive_pcurves(&body, tol), Err(PcurveError::BadTolerance { .. })));
    }
}

// ---------------------------------------------------------------- NURBS faces (hand-labelled)

fn frame(origin: [f64; 3], axis: [f64; 3], ref_dir: [f64; 3]) -> Frame {
    Frame { origin, axis, ref_dir }
}

/// One face on `surface` with a single loop of one line edge out and back (two coedges).
fn nurbs_face(surface: NurbsSurface, a: [f64; 3], b: [f64; 3]) -> (Body, CoedgeIx) {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let dir = [d[0] / len, d[1] / len, d[2] / len];
    // Any unit normal to dir.
    let n = if dir[2].abs() < 0.9 { [-dir[1], dir[0], 0.0] } else { [0.0, -dir[2], dir[1]] };
    let nl = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    let n = [n[0] / nl, n[1] / nl, n[2] / nl];
    let mut bb = BodyBuilder::new([7; 16]);
    let v0 = bb.add_vertex(a, None);
    let v1 = bb.add_vertex(b, None);
    let line = CurveDef::analytic(CurveParams::Line { frame: frame(a, n, dir) }).unwrap();
    let e = bb.add_edge(Some(line), v0, v1, [0.0, len], None);
    let c0 = bb.add_coedge(e, Sense::Same, None, Some("c-out"));
    let c1 = bb.add_coedge(e, Sense::Reversed, None, None);
    let l = bb.add_loop(vec![c0, c1], true, None);
    let f = bb.add_face(SurfaceDef::nurbs(surface).unwrap(), Sense::Same, vec![l], None);
    bb.add_shell(vec![f], false);
    let body = bb.build().unwrap();
    let c = coedge(&body, "c-out");
    (body, c)
}

/// The bilinear patch S(u, v) = (u, v, 0) on [0, 1]^2.
fn unit_patch() -> NurbsSurface {
    NurbsSurface::new(NurbsSurfaceParts {
        degree_u: 1,
        degree_v: 1,
        knots_u: vec![0.0, 0.0, 1.0, 1.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        n_u: 2,
        n_v: 2,
        ctrl: vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
        weights: None,
        periodic_u: false,
        periodic_v: false,
    })
    .unwrap()
}

#[test]
fn nurbs_surface_goes_through_invert_surface() {
    let (a, b) = ([0.25, 0.375, 0.0], [0.625, 0.75, 0.0]);
    let (body, c) = nurbs_face(unit_patch(), a, b);
    let p = derive(&body);
    let Ok(TrimCurve::Derived { uv, t, max_dev_m }) = &p.curves[c.index()] else { panic!("{:?}", p.curves) };
    assert_eq!(uv.len(), 17, "a line maps to a line on a plane patch");
    assert!(*max_dev_m <= 1e-15);
    let len = t[16];
    for (q, &t) in uv.iter().zip(t) {
        // Hand-labelled: S(u, v) = (u, v, 0), so uv is the point's (x, y).
        let s = t / len;
        assert!((q[0] - (a[0] + s * (b[0] - a[0]))).abs() < 1e-12);
        assert!((q[1] - (a[1] + s * (b[1] - a[1]))).abs() < 1e-12);
    }
    assert_eq!(p.charts[0].period_u, None);
}

#[test]
fn nurbs_off_surface_edge_is_measured() {
    let (body, c) = nurbs_face(unit_patch(), [0.25, 0.375, 1e-3], [0.625, 0.75, 1e-3]);
    match derive_coedge(&body, c, DEFAULT_TOL_M) {
        Err(PcurveError::PcurveFailed { reason: FailReason::Tolerance, best_deviation_m, .. }) => {
            assert!((best_deviation_m - 1e-3).abs() < 1e-15)
        }
        r => panic!("{r:?}"),
    }
}

/// A rational quarter cylinder of radius 1 around the z axis, z in [0, 1].
fn quarter_cylinder() -> NurbsSurface {
    let w = core::f64::consts::FRAC_1_SQRT_2;
    NurbsSurface::new(NurbsSurfaceParts {
        degree_u: 2,
        degree_v: 1,
        knots_u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        n_u: 3,
        n_v: 2,
        ctrl: vec![
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
        ],
        weights: Some(vec![1.0, 1.0, w, w, 1.0, 1.0]),
        periodic_u: false,
        periodic_v: false,
    })
    .unwrap()
}

// Each of the 17 inversions exhausts the GEO.NURBS.INVERT subdivision budget (slow in debug
// builds and under wasmtime), so this one is skipped on wasm32; the status mapping itself is
// unit-tested in `topo::pcurve`.
#[test]
#[cfg_attr(target_arch = "wasm32", ignore)]
fn an_uncertified_inversion_never_yields_a_pcurve() {
    // An edge on the cylinder axis: every point of the arc is equally near, so GEO.NURBS.INVERT
    // cannot certify a unique foot. Whatever status it reports, the coedge must fail, and with
    // the reason that status implies (never a pcurve presented as valid).
    let s = quarter_cylinder();
    let a = [0.0, 0.0, 0.25];
    let status = match invert_surface(&s, a) {
        Ok(i) => i.status,
        Err(fid_geo_nurbs::InvertError::NonConvergence { best }) => best.status,
        Err(e) => panic!("{e:?}"),
    };
    let (body, c) = nurbs_face(s, a, [0.0, 0.0, 0.75]);
    let r = derive_coedge(&body, c, DEFAULT_TOL_M);
    let Err(PcurveError::PcurveFailed { reason, best_deviation_m, .. }) = r else { panic!("{r:?}") };
    match status {
        Status::Unresolved { reason: why } => {
            assert_eq!(reason, FailReason::Inversion(InversionFault::Unresolved(why)))
        }
        Status::Certified { tie: true } => assert_eq!(reason, FailReason::Inversion(InversionFault::Tie)),
        Status::Certified { tie: false } => assert_eq!(reason, FailReason::Tolerance),
    }
    assert!(best_deviation_m > 0.99, "the axis is 1 m from the surface: {best_deviation_m}");
}

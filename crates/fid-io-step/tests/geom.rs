//! Acceptance criteria of spec node IO.CAD.STEP.GEOM (AC-1 .. AC-5) and REQ-001 .. REQ-005.
//! Fixtures and expected values come from `fixtures/io/cad/step/geom/make_cases.py`, an
//! independent stdlib Python oracle (exact rational B-spline evaluation, the unit rule applied
//! in IEEE binary64); none are produced by this crate. Closed-form checks (cylinder, torus and
//! plane distances) are written out here from the geometry, not taken from the mapper.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_nurbs::{AnalyticSurface, Curve, NurbsSurface, Surface};
use fid_io_reg::prov::LengthUnit;
use fid_io_step::geom::{
    ApproxStatus, Curve2, CurveRef, GeomMap, GeomOptions, ParamBox, SkipReason, SurfaceRef, APPROX_OUT_OF_TOLERANCE,
    FRAME_REORTHONORMALISED, MAX_REFINEMENTS, SAMPLES_PER_SPAN,
};
use fid_io_step::{map_geometry, parse, resolve_header, HeaderOptions};
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/cad/step/geom")
}

fn map_with(name: &str, unit: Option<LengthUnit>, opts: &GeomOptions) -> GeomMap {
    let bytes = std::fs::read(dir().join(name)).unwrap();
    let file = parse(&bytes).unwrap();
    let header = resolve_header(&file, &HeaderOptions { unit, ..HeaderOptions::default() }).unwrap();
    map_geometry(&file, &header, opts).unwrap()
}

fn map(name: &str) -> GeomMap {
    map_with(name, None, &GeomOptions::default())
}

fn f(h: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(h, 16).unwrap())
}

/// Rows `values` of expected.txt for `(file, entity, field)`.
fn expected(file: &str, entity: u64, field: &str) -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(dir().join("expected.txt")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            (w.len() >= 3 && w[0] == file && w[1] == entity.to_string() && w[2] == field)
                .then(|| w[3..].iter().map(|s| s.to_string()).collect())
        })
        .collect()
}

fn one(file: &str, entity: u64, field: &str) -> Vec<f64> {
    let rows = expected(file, entity, field);
    assert_eq!(rows.len(), 1, "{file} {entity} {field}");
    rows[0].iter().map(|h| f(h)).collect()
}

fn bits(a: &[f64]) -> Vec<u64> {
    a.iter().map(|x| x.to_bits()).collect()
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

fn surface(m: &GeomMap, n: u64) -> &SurfaceRef {
    m.surfaces.get(&n).unwrap_or_else(|| panic!("surface {n} not mapped; skipped: {:?}", m.skipped))
}

fn analytic(m: &GeomMap, n: u64) -> AnalyticSurface {
    match surface(m, n) {
        SurfaceRef::Analytic(a) => *a,
        other => panic!("{n}: {other:?}"),
    }
}

fn skip_code(m: &GeomMap, n: u64) -> (&str, &'static str) {
    let s = m.skipped.iter().find(|s| s.entity == n).unwrap_or_else(|| panic!("{n} not skipped"));
    (s.type_name.as_str(), s.reason.code())
}

/// Every expected `skipped` row of a fixture holds.
fn check_skips(file: &str, m: &GeomMap) {
    let text = std::fs::read_to_string(dir().join("expected.txt")).unwrap();
    let mut seen = 0;
    for l in text.lines() {
        let w: Vec<&str> = l.split_whitespace().collect();
        if w.len() == 5 && w[0] == file && w[2] == "skipped" {
            let n: u64 = w[1].parse().unwrap();
            assert_eq!(skip_code(m, n), (w[3], expected_code(w[4])), "{file} #{n}");
            seen += 1;
        }
    }
    assert_eq!(seen, m.skipped.len(), "{file}: unexpected skips {:?}", m.skipped);
}

fn expected_code(s: &str) -> &'static str {
    [
        "unsupported",
        "malformed",
        "nurbs_invalid",
        "geo_invalid",
        "dangling_reference",
        "no_context",
        "context_conflict",
        "offset_degenerate_normal",
    ]
    .into_iter()
    .find(|c| *c == s)
    .unwrap()
}

// ------------------------------------------------------------------ AC-1, REQ-001

#[test]
fn ac1_analytic_surfaces_are_file_values_times_factor_bit_exactly() {
    let file = "analytic_set.stp";
    let m = map(file);
    check_skips(file, &m);
    for (n, a) in [10u64, 11, 12, 13, 14].map(|n| (n, analytic(&m, n))) {
        let (got, kind): (Vec<f64>, &str) = match a {
            AnalyticSurface::Plane(p) => (p.placement().origin().to_vec(), "plane"),
            AnalyticSurface::Cylinder(c) => ([&c.placement().origin()[..], &[c.radius()]].concat(), "cylinder"),
            AnalyticSurface::Cone(c) => ([&c.placement().origin()[..], &[c.radius(), c.half_angle()]].concat(), "cone"),
            AnalyticSurface::Sphere(s) => ([&s.placement().origin()[..], &[s.radius()]].concat(), "sphere"),
            AnalyticSurface::Torus(t) => {
                let (r1, r2) = t.radii();
                ([&t.placement().origin()[..], &[r1, r2]].concat(), "torus")
            }
        };
        assert_eq!(bits(&got), bits(&one(file, n, kind)), "{file} #{n} {kind}");
    }
    for n in [10u64, 11, 14, 16] {
        let e = one(file, n, "frame");
        let pl = *analytic(&m, n).placement_ref();
        let got = [pl.axis(), pl.ref_dir()].concat();
        for (g, w) in got.iter().zip(&e) {
            assert!((g - w).abs() <= 1e-15, "{file} #{n} frame {got:?} vs {e:?}");
        }
    }
    // The skew reference direction of #15 is reported.
    let w: Vec<_> = m.warnings.iter().filter(|w| w.code == FRAME_REORTHONORMALISED).collect();
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].entity, 15);
    assert!((w[0].value - 0.5 / 1.25_f64.sqrt()).abs() < 1e-15);

    for n in [1u64, 5, 30, 31, 32, 33] {
        assert_eq!(bits(&m.points[&n]), bits(&one(file, n, "point")), "point #{n}");
    }
    match &m.curves[&21] {
        CurveRef::Line { line, magnitude_m } => {
            let got = [&line.placement().origin()[..], &[*magnitude_m]].concat();
            assert_eq!(bits(&got), bits(&one(file, 21, "line")));
            assert_eq!(line.placement().ref_dir(), [1.0, 0.0, 0.0]);
        }
        other => panic!("{other:?}"),
    }
    match &m.curves[&22] {
        CurveRef::Circle(c) => {
            let got = [&c.placement().origin()[..], &[c.radius()]].concat();
            assert_eq!(bits(&got), bits(&one(file, 22, "circle")));
        }
        other => panic!("{other:?}"),
    }
    match &m.curves[&23] {
        CurveRef::Ellipse(e) => {
            let (a, b) = e.semi_axes();
            let got = [&e.placement().origin()[..], &[a, b]].concat();
            assert_eq!(bits(&got), bits(&one(file, 23, "ellipse")));
        }
        other => panic!("{other:?}"),
    }
    for n in [24u64, 25] {
        match &m.curves[&n] {
            CurveRef::Trimmed(t) => {
                let got = [t.trim_1.param.unwrap(), t.trim_2.param.unwrap()];
                assert_eq!(bits(&got), bits(&one(file, n, "trim")), "trim #{n}");
                assert!(t.sense);
                if n == 25 {
                    assert_eq!(bits(&t.trim_1.point.unwrap()), bits(&one(file, 25, "trim1_point")));
                    assert_eq!(t.basis, 21);
                } else {
                    assert_eq!(t.trim_1.point, None);
                }
            }
            other => panic!("{other:?}"),
        }
    }
    for n in [34u64, 35] {
        let c = match &m.curves[&n] {
            CurveRef::Polyline(c) if n == 34 => c,
            CurveRef::Nurbs(c) if n == 35 => c,
            other => panic!("{other:?}"),
        };
        assert_eq!(bits(c.knots().as_slice()), bits(&one(file, n, "knots")));
        let flat: Vec<f64> = c.ctrl().iter().flatten().copied().collect();
        assert_eq!(bits(&flat), bits(&one(file, n, "ctrl")));
        assert!(!c.is_rational());
    }
}

/// Small helper so the frame check above can read any analytic surface's placement.
trait PlacementRef {
    fn placement_ref(&self) -> &fid_geo_nurbs::Placement;
}

impl PlacementRef for AnalyticSurface {
    fn placement_ref(&self) -> &fid_geo_nurbs::Placement {
        match self {
            AnalyticSurface::Plane(p) => p.placement(),
            AnalyticSurface::Cylinder(c) => c.placement(),
            AnalyticSurface::Cone(c) => c.placement(),
            AnalyticSurface::Sphere(s) => s.placement(),
            AnalyticSurface::Torus(t) => t.placement(),
        }
    }
}

#[test]
fn req001_plane_angles_use_the_context_angle_factor() {
    let file = "degree_units.stp";
    let m = map(file);
    check_skips(file, &m);
    match analytic(&m, 10) {
        AnalyticSurface::Cone(c) => {
            let got = [&c.placement().origin()[..], &[c.radius(), c.half_angle()]].concat();
            assert_eq!(bits(&got), bits(&one(file, 10, "cone")));
        }
        other => panic!("{other:?}"),
    }
    match &m.curves[&12] {
        CurveRef::Trimmed(t) => {
            assert_eq!(bits(&[t.trim_1.param.unwrap(), t.trim_2.param.unwrap()]), bits(&one(file, 12, "trim")));
        }
        other => panic!("{other:?}"),
    }
}

// ------------------------------------------------------------------ AC-2, REQ-002

#[test]
fn ac2_rational_bspline_is_preserved_and_evaluates_like_the_exact_oracle() {
    let file = "rational_bspline.stp";
    let m = map(file);
    check_skips(file, &m);
    let s = match surface(&m, 10) {
        SurfaceRef::Nurbs(s) => s,
        other => panic!("{other:?}"),
    };
    assert!(s.is_rational());
    assert_eq!((s.degree_u(), s.degree_v(), s.shape()), (2, 2, (3, 4)));
    assert_eq!(bits(s.knots_u().as_slice()), bits(&one(file, 10, "knots_u")));
    assert_eq!(bits(s.knots_v().as_slice()), bits(&one(file, 10, "knots_v")));
    assert_eq!(bits(s.weights().unwrap()), bits(&one(file, 10, "weights")));
    let flat: Vec<f64> = s.ctrl().iter().flatten().copied().collect();
    assert_eq!(bits(&flat), bits(&one(file, 10, "ctrl")));
    let plain = match surface(&m, 12) {
        SurfaceRef::Nurbs(s) => s,
        other => panic!("{other:?}"),
    };
    assert!(!plain.is_rational());
    for (n, sf) in [(10u64, s), (12, plain)] {
        let rows = expected(file, n, "eval");
        assert_eq!(rows.len(), 100);
        let mut worst = 0.0_f64;
        for r in rows {
            let v: Vec<f64> = r.iter().map(|h| f(h)).collect();
            let p = sf.eval(v[0], v[1], 0).unwrap().p;
            worst = worst.max(dist(p, [v[2], v[3], v[4]]));
        }
        assert!(worst <= 1e-12, "surface #{n}: {worst:e}");
    }
    let c = match &m.curves[&11] {
        CurveRef::Nurbs(c) => c,
        other => panic!("{other:?}"),
    };
    assert!(c.is_rational());
    assert_eq!(c.degree(), 3);
    assert_eq!(bits(c.knots().as_slice()), bits(&one(file, 11, "knots")));
    assert_eq!(bits(c.weights().unwrap()), bits(&one(file, 11, "weights")));
    let flat: Vec<f64> = c.ctrl().iter().flatten().copied().collect();
    assert_eq!(bits(&flat), bits(&one(file, 11, "ctrl")));
    let rows = expected(file, 11, "eval");
    assert_eq!(rows.len(), 100);
    for r in rows {
        let v: Vec<f64> = r.iter().map(|h| f(h)).collect();
        let p = c.eval(v[0], 0).unwrap().p;
        assert!(dist(p, [v[1], v[2], v[3]]) <= 1e-12, "curve at {}", v[0]);
    }
}

#[test]
fn req002_invalid_bsplines_and_other_bad_entities_are_reported_and_skipped() {
    let file = "invalid.stp";
    let m = map(file);
    check_skips(file, &m);
    let at = |n: u64| m.skipped.iter().find(|s| s.entity == n).unwrap().reason;
    match at(12) {
        SkipReason::NurbsInvalid { at: 12, err } => assert_eq!(err.code(), "BadKnots"),
        other => panic!("{other:?}"),
    }
    match at(13) {
        SkipReason::NurbsInvalid { at: 13, err } => assert_eq!(err.code(), "CountMismatch"),
        other => panic!("{other:?}"),
    }
    assert_eq!(at(11), SkipReason::Malformed { at: 11, what: "arity" });
    assert_eq!(at(15), SkipReason::Dangling { to: 99 });
    assert_eq!(at(16), SkipReason::Malformed { at: 8, what: "zero_direction" });
    assert_eq!(at(14), SkipReason::Unsupported { at: 14 });
    assert!(matches!(analytic(&m, 10), AnalyticSurface::Plane(_)));
    match analytic(&m, 18) {
        AnalyticSurface::Cylinder(c) => assert_eq!(c.radius().to_bits(), one(file, 18, "cylinder")[3].to_bits()),
        other => panic!("{other:?}"),
    }
}

// ------------------------------------------------------------------ AC-3, REQ-003, REQ-004

/// The sample grid of an approximant: 33 points per knot span of its own knot vector.
fn grid(knots: &[f64]) -> Vec<f64> {
    let mut b: Vec<f64> = Vec::new();
    for &k in knots {
        if b.last() != Some(&k) {
            b.push(k);
        }
    }
    let mut out = Vec::new();
    for w in b.windows(2) {
        for k in 0..32 {
            out.push(w[0] + (w[1] - w[0]) * (k as f64 / 32.0));
        }
    }
    out.push(*b.last().unwrap());
    out
}

fn approximant(m: &GeomMap, n: u64) -> &NurbsSurface {
    match surface(m, n) {
        SurfaceRef::Approximated(s) => s,
        other => panic!("{other:?}"),
    }
}

#[test]
fn ac3_offset_cylinder_error_is_within_tolerance_and_matches_the_closed_form() {
    let file = "offset_cyl.stp";
    let m = map(file);
    check_skips(file, &m);
    let axis = one(file, 11, "axis");
    for n in [11u64, 12] {
        let r = one(file, n, "radius")[0];
        let a = m.approximations[&n];
        assert_eq!(a.status, ApproxStatus::WithinTolerance);
        assert!(a.max_dev_m <= 1e-7, "#{n}: {:e}", a.max_dev_m);
        assert!(a.max_dev_m > 0.0);
        // REQ-003: at least 33 x 33 samples per knot span pair.
        assert_eq!(a.grid, [a.spans[0] * 32 + 1, a.spans[1] * 32 + 1]);
        assert_eq!(a.samples, (a.grid[0] * a.grid[1]) as u64);
        assert!(a.refined <= MAX_REFINEMENTS);
        assert!(a.domain_defaulted, "cylinder v is unbounded and no box was given");
        let s = approximant(&m, n);
        let (us, vs) = (grid(s.knots_u().as_slice()), grid(s.knots_v().as_slice()));
        assert_eq!([us.len(), vs.len()], a.grid);
        let mut worst = 0.0_f64;
        for &u in &us {
            for &v in &vs {
                let p = s.eval(u, v, 0).unwrap().p;
                let rho = (p[0] - axis[0]).hypot(p[1] - axis[1]);
                worst = worst.max((rho - r).abs());
            }
        }
        assert!((worst - a.max_dev_m).abs() <= 1e-9, "#{n}: closed form {worst:e} vs recorded {:e}", a.max_dev_m);
    }
    // The plane offset is exact (linear in both directions, no refinement).
    let a = m.approximations[&14];
    assert_eq!((a.status, a.refined, a.spans), (ApproxStatus::WithinTolerance, 0, [1, 1]));
    assert!(a.max_dev_m <= 1e-15);
    let z = one(file, 14, "plane_z")[0];
    for c in approximant(&m, 14).ctrl() {
        assert!((c[2] - z).abs() <= 1e-15);
    }
}

#[test]
fn req003_caller_box_sets_the_fitted_domain() {
    let mut opts = GeomOptions::default();
    let b = ParamBox { u: (0.0, core::f64::consts::PI), v: (-0.004, 0.006) };
    opts.domains.insert(11, b);
    let m = map_with("offset_cyl.stp", None, &opts);
    let a = m.approximations[&11];
    assert_eq!(a.domain, b);
    assert!(!a.domain_defaulted);
    assert!(a.max_dev_m <= 1e-7);
    assert_eq!(approximant(&m, 11).domain(), (b.u, b.v));
}

#[test]
fn req004_out_of_tolerance_after_four_refinements() {
    let opts = GeomOptions { approx_tol_m: 1e-15, ..GeomOptions::default() };
    let m = map_with("offset_cyl.stp", None, &opts);
    let a = m.approximations[&11];
    assert_eq!(a.status, ApproxStatus::OutOfTolerance);
    assert_eq!(a.status.code(), APPROX_OUT_OF_TOLERANCE);
    assert_eq!(a.refined, MAX_REFINEMENTS);
    assert!(a.max_dev_m > 1e-15 && a.max_dev_m < 1e-9, "{:e}", a.max_dev_m);
    // 8 initial spans around the turn, halved four times; v stays one linear span.
    assert_eq!(a.spans, [8 << MAX_REFINEMENTS, 1]);
    assert_eq!(a.grid[0], a.spans[0] * (SAMPLES_PER_SPAN - 1) + 1);
    // The approximant is still recorded.
    assert!(m.surfaces.contains_key(&11));
}

#[test]
fn bad_options_are_refused() {
    let bytes = std::fs::read(dir().join("offset_cyl.stp")).unwrap();
    let file = parse(&bytes).unwrap();
    let header = resolve_header(&file, &HeaderOptions::default()).unwrap();
    for tol in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let opts = GeomOptions { approx_tol_m: tol, ..GeomOptions::default() };
        assert_eq!(map_geometry(&file, &header, &opts).unwrap_err().code(), "bad_option");
    }
    let mut opts = GeomOptions::default();
    opts.domains.insert(11, ParamBox { u: (1.0, 1.0), v: (0.0, 1.0) });
    let m = map_geometry(&file, &header, &opts).unwrap();
    assert_eq!(skip_code(&m, 11), ("OFFSET_SURFACE", "bad_domain"));
}

// ------------------------------------------------------------------ AC-4, REQ-005

#[test]
fn ac4_unknown_surface_is_skipped_and_the_plane_is_mapped() {
    let file = "unknown_surface.stp";
    let m = map(file);
    check_skips(file, &m);
    match analytic(&m, 10) {
        AnalyticSurface::Plane(p) => assert_eq!(bits(&p.placement().origin()), bits(&one(file, 10, "plane"))),
        other => panic!("{other:?}"),
    }
    assert_eq!(m.surfaces.len(), 1);
    assert_eq!(m.skipped.len(), 1);
    assert_eq!(m.skipped[0].entity, 11);
    assert_eq!(m.skipped[0].type_name, "RECTANGULAR_TRIMMED_SURFACE");
    assert_eq!(m.skipped[0].reason, SkipReason::Unsupported { at: 11 });
}

// ------------------------------------------------------------------ swept surfaces, pcurves

fn mm() -> Option<LengthUnit> {
    // HDR gap (PR #625, Open questions): the pcurves' 2D parametric context declares no unit,
    // which IO.CAD.STEP.HDR refuses with `unit_required` without a caller unit. The 3D context
    // declares millimetre, so the caller unit agrees. Drop this helper once HDR exempts unitless
    // 2D contexts; `unitless_2d_context_is_still_parameter_space` covers that case already.
    Some(LengthUnit::Mm)
}

#[test]
fn extrusion_and_revolution_are_exact_nurbs() {
    let file = "swept.stp";
    let m = map_with(file, mm(), &GeomOptions::default());
    check_skips(file, &m);
    // Extrusion of a B-spline: exact against the rational oracle, STEP parameters kept.
    let (s, exact, defaulted) = match surface(&m, 11) {
        SurfaceRef::Extrusion { nurbs, param_exact, domain_defaulted } => (nurbs, *param_exact, *domain_defaulted),
        other => panic!("{other:?}"),
    };
    assert!(exact && defaulted);
    let rows = expected(file, 11, "eval");
    assert_eq!(rows.len(), 50);
    for r in rows {
        let v: Vec<f64> = r.iter().map(|h| f(h)).collect();
        let p = s.eval(v[0], v[1], 0).unwrap().p;
        assert!(dist(p, [v[2], v[3], v[4]]) <= 1e-12);
    }
    // Revolution of a segment parallel to the axis: a cylinder of radius 7 mm, u in [0, 15].
    let check = |n: u64, f: &dyn Fn([f64; 3]) -> f64| {
        let s = match surface(&m, n) {
            SurfaceRef::Revolution { nurbs, .. } | SurfaceRef::Extrusion { nurbs, .. } => nurbs,
            other => panic!("{other:?}"),
        };
        let ((u0, u1), (v0, v1)) = s.domain();
        let mut worst = 0.0_f64;
        for i in 0..=40 {
            for j in 0..=40 {
                let u = u0 + (u1 - u0) * (i as f64 / 40.0);
                let v = v0 + (v1 - v0) * (j as f64 / 40.0);
                worst = worst.max(f(s.eval(u, v, 0).unwrap().p).abs());
            }
        }
        assert!(worst <= 1e-15, "#{n}: {worst:e}");
        s.domain()
    };
    let r17 = one(file, 17, "cylinder_radius")[0];
    let ((u0, u1), (v0, v1)) = check(17, &|p| p[0].hypot(p[1]) - r17);
    assert_eq!([u0, u1], [one(file, 17, "u_range")[0], one(file, 17, "u_range")[1]]);
    assert_eq!([v0, v1], [0.0, core::f64::consts::TAU]);
    let t = one(file, 22, "torus");
    check(22, &|p| (p[0].hypot(p[1]) - t[0]).hypot(p[2]) - t[1]);
    let r24 = one(file, 24, "cylinder_radius")[0];
    check(24, &|p| p[0].hypot(p[1]) - r24);
    match surface(&m, 24) {
        SurfaceRef::Extrusion { param_exact, .. } => assert!(!param_exact),
        other => panic!("{other:?}"),
    }
}

#[test]
fn pcurves_and_surface_curves() {
    let file = "swept.stp";
    let m = map_with(file, mm(), &GeomOptions::default());
    let pc = &m.pcurves[&36];
    assert_eq!((pc.surface, pc.representation, pc.curve_entity), (30, 35, 34));
    assert!(pc.param_exact);
    assert_eq!(pc.scale, [0.001, 0.001]);
    match &pc.curve {
        Curve2::Line { origin, vector } => {
            assert_eq!(bits(&[origin[0], origin[1], vector[0], vector[1]]), bits(&one(file, 36, "line2")))
        }
        other => panic!("{other:?}"),
    }
    for n in [46u64, 47] {
        let pc = &m.pcurves[&n];
        assert!(pc.param_exact);
        match &pc.curve {
            Curve2::Nurbs(c) => {
                let flat: Vec<f64> = c.ctrl().iter().flat_map(|p| [p[0], p[1]]).collect();
                assert_eq!(bits(&flat), bits(&one(file, n, "ctrl2")), "pcurve #{n}");
                assert!(c.ctrl().iter().all(|p| p[2] == 0.0));
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(m.pcurves[&47].scale, [1.0, 1.0]);
    match &m.curves[&39] {
        CurveRef::Surface { curve_3d, associated, seam, .. } => {
            assert_eq!((*curve_3d, associated.as_slice(), *seam), (38, &[36u64][..], false));
        }
        other => panic!("{other:?}"),
    }
    // Parameter-space geometry (2D points and curves) is not mapped on its own.
    assert!(!m.curves.contains_key(&34) && !m.curves.contains_key(&44));
    assert!(!m.points.contains_key(&31));
}

// ------------------------------------------------------------------ contexts

#[test]
fn each_entity_uses_its_own_context_units() {
    let file = "contexts.stp";
    let m = map(file);
    check_skips(file, &m);
    assert_eq!(bits(&m.points[&5]), bits(&one(file, 5, "point")));
    assert_eq!(bits(&m.points[&6]), bits(&one(file, 6, "point")));
    for n in [10u64, 11] {
        match analytic(&m, n) {
            AnalyticSurface::Cylinder(c) => {
                let got = [&c.placement().origin()[..], &[c.radius()]].concat();
                assert_eq!(bits(&got), bits(&one(file, n, "cylinder")), "#{n}");
            }
            other => panic!("{other:?}"),
        }
    }
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_mapping_twice_gives_byte_identical_records() {
    let files = [
        ("analytic_set.stp", None),
        ("degree_units.stp", None),
        ("rational_bspline.stp", None),
        ("offset_cyl.stp", None),
        ("offset_extrusion.stp", None),
        ("unknown_surface.stp", None),
        ("invalid.stp", None),
        ("swept.stp", mm()),
        ("contexts.stp", None),
    ];
    for (name, unit) in files {
        let a = map_with(name, unit, &GeomOptions::default()).to_records();
        let b = map_with(name, unit, &GeomOptions::default()).to_records();
        assert_eq!(a, b, "{name}");
        assert!(a.ends_with(b"\n") && !a.is_empty(), "{name}");
        let text = String::from_utf8(a).unwrap();
        assert!(text.lines().all(|l| !l.is_empty()), "{name}");
    }
    let text = String::from_utf8(map("offset_cyl.stp").to_records()).unwrap();
    assert!(text.lines().any(|l| l.starts_with("approx 11 approx_within_tolerance ")));
    assert!(text.lines().any(|l| l.starts_with("skipped 16 OFFSET_SURFACE offset_degenerate_normal")));
}

// ------------------------------------------------------------------ robustness

#[test]
fn perturbed_parameters_never_panic_and_never_stop_the_mapping() {
    use fid_io_step::Param;
    // A loose tolerance keeps the offset fits to one level so the sweep stays fast.
    let opts = GeomOptions { approx_tol_m: 1.0, ..GeomOptions::default() };
    for (name, unit) in [
        ("analytic_set.stp", None),
        ("rational_bspline.stp", None),
        ("offset_cyl.stp", None),
        ("swept.stp", mm()),
        ("invalid.stp", None),
    ] {
        let bytes = std::fs::read(dir().join(name)).unwrap();
        let file = parse(&bytes).unwrap();
        let header = resolve_header(&file, &HeaderOptions { unit, ..HeaderOptions::default() }).unwrap();
        let mut runs = 0;
        for (&n, inst) in file.data.iter().filter(|(&n, _)| n < 9000) {
            for (pi, (_, params)) in inst.parts.iter().enumerate() {
                for i in 0..params.len() {
                    for rep in [
                        Param::Unset,
                        Param::Int(-1),
                        Param::Real(0.0, None),
                        Param::Real(1e300, None),
                        Param::Ref(n),
                        Param::Ref(999_999),
                        Param::List(vec![]),
                    ] {
                        let mut f = file.clone();
                        f.data.get_mut(&n).unwrap().parts[pi].1[i] = rep;
                        let m = map_geometry(&f, &header, &opts).unwrap();
                        // Every other entity is still mapped or reported.
                        assert!(m.surfaces.len() + m.curves.len() + m.points.len() + m.skipped.len() > 0);
                        runs += 1;
                    }
                }
            }
        }
        assert!(runs > 50, "{name}: {runs}");
    }
}

#[test]
fn unitless_2d_context_is_still_parameter_space() {
    // Simulates HDR exempting the unitless 2D context: it is absent from `header.contexts`.
    let bytes = std::fs::read(dir().join("swept.stp")).unwrap();
    let file = parse(&bytes).unwrap();
    let mut header = resolve_header(&file, &HeaderOptions { unit: mm(), ..HeaderOptions::default() }).unwrap();
    assert!(header.contexts.remove(&9205).is_some());
    let m = map_geometry(&file, &header, &GeomOptions::default()).unwrap();
    let full = map_with("swept.stp", mm(), &GeomOptions::default());
    assert_eq!(m.to_records(), full.to_records());
    assert!(!m.curves.contains_key(&34) && !m.curves.contains_key(&44) && !m.points.contains_key(&31));
    assert!(m.pcurves.contains_key(&36) && m.pcurves.contains_key(&46));
}

// ------------------------------------------------------------------ offset of an extrusion

fn offset_ext_check(m: &GeomMap, field: &str) -> f64 {
    let file = "offset_extrusion.stp";
    let s = approximant(m, 12);
    let rows = expected(file, 12, field);
    assert_eq!(rows.len(), 50);
    let mut worst = 0.0_f64;
    for r in rows {
        let v: Vec<f64> = r.iter().map(|h| f(h)).collect();
        let p = s.eval(v[0], v[1], 0).unwrap().p;
        worst = worst.max(dist(p, [v[2], v[3], v[4]]));
    }
    worst
}

#[test]
fn offset_of_extrusion_records_the_defaulted_basis_range() {
    let file = "offset_extrusion.stp";
    let m = map(file);
    check_skips(file, &m);
    let a = m.approximations[&12];
    assert_eq!(a.status, ApproxStatus::WithinTolerance);
    assert!(a.domain_defaulted, "the extrusion basis got v = [0, 1] without a box");
    assert_eq!(a.domain, ParamBox { u: (0.0, 1.0), v: (0.0, 1.0) });
    assert_eq!(a.spans[1], 1, "the offset of an extrusion is linear along the extrusion");
    let worst = offset_ext_check(&m, "eval_default");
    assert!(worst <= 2.0 * a.tolerance_m, "{worst:e} (recorded {:e})", a.max_dev_m);
}

#[test]
fn offset_of_extrusion_uses_the_offset_box_for_its_basis() {
    let file = "offset_extrusion.stp";
    let bx = one(file, 12, "box");
    let b = ParamBox { u: (bx[0], bx[1]), v: (bx[2], bx[3]) };
    let mut opts = GeomOptions::default();
    opts.domains.insert(12, b);
    let m = map_with(file, None, &opts);
    let a = m.approximations[&12];
    assert_eq!(a.status, ApproxStatus::WithinTolerance);
    assert!(!a.domain_defaulted);
    assert_eq!(a.domain, b);
    assert_eq!(approximant(&m, 12).domain(), (b.u, b.v));
    let worst = offset_ext_check(&m, "eval_box");
    assert!(worst <= 2.0 * a.tolerance_m, "{worst:e} (recorded {:e})", a.max_dev_m);

    // A box given only for the basis extrusion is used for it, and so for the offset's domain.
    let mut opts = GeomOptions::default();
    opts.domains.insert(11, b);
    let m = map_with(file, None, &opts);
    let a = m.approximations[&12];
    assert!(!a.domain_defaulted);
    assert_eq!(a.domain, b);
    assert!(offset_ext_check(&m, "eval_box") <= 2.0 * a.tolerance_m);
}

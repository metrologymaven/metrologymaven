//! GEO.CAD.TOPO.TRIM acceptance tests. Expected values come from the independent exact-rational
//! and 60-digit oracle `fixtures/geo/cad/topo/make_trim_fixtures.py` (`trim_expected.json`); the
//! seam, pole, apex, NURBS and undecidable cases are hand-labelled from the spec's rules and the
//! fixtures' geometry.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use core::cmp::Ordering;
use core::f64::consts::{FRAC_PI_2, PI, TAU};

use common::{build, load, J};
use fid_geo_cad::topo::trim::exact::{orient, Shifted};
use fid_geo_cad::{
    classify_uv, derive_pcurves, Body, BodyBuilder, Curve2, EntityIx, FaceIx, FailReason, Label, Sense, SurfaceDef,
    TopologyId, TrimClass, TrimError, TrimIndex, Undecidable, DEFAULT_TOL_M,
};
use fid_geo_nurbs::{AnalyticSurface, NurbsCurve, NurbsCurveParts, NurbsSurface, NurbsSurfaceParts};

fn expected() -> J {
    load("trim_expected.json")
}

fn face(body: &Body, name: &str) -> FaceIx {
    match body.lookup(&TopologyId::Imported(Label::new(name).unwrap())) {
        Some(EntityIx::Face(f)) => f,
        other => panic!("{name}: {other:?}"),
    }
}

fn index(body: &Body, f: FaceIx) -> TrimIndex {
    let p = derive_pcurves(body, DEFAULT_TOL_M).unwrap();
    TrimIndex::build(body, &p, f).unwrap()
}

fn fixture_index(name: &str, face_label: &str) -> TrimIndex {
    let body = build(name);
    let f = face(&body, face_label);
    index(&body, f)
}

fn hexf(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn class(c: &str) -> TrimClass {
    match c {
        "I" => TrimClass::Inside,
        "O" => TrimClass::Outside,
        "B" => TrimClass::OnBoundary,
        _ => panic!("class {c}"),
    }
}

/// A case row "<u hex> <v hex> <class>".
fn row(j: &J) -> (f64, f64, TrimClass) {
    let f: Vec<&str> = j.str().split(' ').collect();
    assert_eq!(f.len(), 3);
    (hexf(f[0]), hexf(f[1]), class(f[2]))
}

fn letters(v: &[TrimClass]) -> String {
    v.iter().map(|c| c.letter()).collect()
}

struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn between(&mut self, lo: f64, hi: f64) -> f64 {
        let x = (self.next() >> 11) as f64 * 2f64.powi(-53);
        lo + (hi - lo) * x
    }
}

/// AC-1 and AC-5: 10 000 seeded points on the plate with a hole match the analytic answer;
/// the result vector is compared byte for byte, so it is the same on every OS.
#[test]
fn ac1_ac5_plate_with_hole_seeded_points() {
    let e = expected();
    let ac1 = e.get("ac1");
    let idx = fixture_index("plate_with_hole", "f-plate");
    assert_eq!(idx.segments().len(), ac1.get("segments").u());
    let tol = ac1.get("tol_m").f();
    let bx = ac1.get("box").fs();
    let mut rng = SplitMix(ac1.get("seed").u() as u64);
    let mut out = Vec::with_capacity(ac1.get("count").u());
    for _ in 0..ac1.get("count").u() {
        let u = rng.between(bx[0], bx[1]);
        let v = rng.between(bx[2], bx[3]);
        out.push(classify_uv(&idx, u, v, tol).unwrap());
    }
    let got = letters(&out);
    let want = ac1.get("classes").str();
    let first = got.chars().zip(want.chars()).position(|(a, b)| a != b);
    assert_eq!(first, None, "first difference at point {first:?}");
    assert_eq!(got, want);
    for c in ["I", "O", "B"] {
        assert_eq!(got.matches(c).count(), ac1.get("counts").get(c).u(), "{c}");
    }
    // Seeded points within a few tolerances of the boundary (the band cases).
    let band = ac1.get("band_cases").arr();
    assert!(band.len() >= 2000);
    let mut seen = [0usize; 3];
    for c in band {
        let (u, v, want) = row(c);
        assert_eq!(classify_uv(&idx, u, v, tol), Ok(want), "({u:e}, {v:e})");
        seen[want as usize] += 1;
    }
    assert!(seen.iter().all(|&n| n > 300), "{seen:?}");
}

/// AC-2: the window across the seam; u, u + 2 pi and u - 2 pi classify identically and match
/// the 60-digit closed form.
#[test]
fn ac2_cylinder_window_across_the_seam() {
    let e = expected();
    let ac2 = e.get("ac2");
    let idx = fixture_index("cylinder_window", "f-window");
    assert_eq!(idx.undecidable(), None);
    let tol = ac2.get("tol_m").f();
    let cases = ac2.get("cases").arr();
    assert!(cases.len() > 600);
    let mut seen = [0usize; 3];
    for c in cases {
        let (u, v, want) = row(c);
        for w in [u, u + TAU, u - TAU] {
            assert_eq!(classify_uv(&idx, w, v, tol), Ok(want), "u {w:e} v {v:e}");
        }
        seen[want as usize] += 1;
    }
    assert!(seen.iter().all(|&n| n > 20), "{seen:?}");
}

/// AC-3 and the fuzz: vertices, points on horizontal segments, rays through vertices,
/// near-boundary points on both sides, both loop orientations, at two tolerances.
#[test]
fn ac3_polygon_fuzz_matches_exact_oracle() {
    let e = expected();
    let body = build("trim_polygons");
    let p = derive_pcurves(&body, DEFAULT_TOL_M).unwrap();
    let mut total = 0;
    for (label, per_tol) in e.get("polygons").obj() {
        let idx = TrimIndex::build(&body, &p, face(&body, label)).unwrap();
        for (tol, cases) in per_tol.obj() {
            let tol: f64 = tol.parse().unwrap();
            for c in cases.arr() {
                let (u, v, want) = row(c);
                assert_eq!(classify_uv(&idx, u, v, tol), Ok(want), "{label} tol {tol:e} ({u:e}, {v:e})");
                total += 1;
            }
        }
    }
    assert!(total > 2000, "{total}");
}

/// AC-3: every polyline vertex and segment end is on the boundary, exactly, even with a
/// tolerance far below the band's resolution.
#[test]
fn ac3_vertices_are_on_the_boundary() {
    for (name, label) in [("plate_with_hole", "f-plate"), ("trim_polygons", "f-comb"), ("cylinder_window", "f-window")]
    {
        let idx = fixture_index(name, label);
        assert!(!idx.segments().is_empty());
        for s in idx.segments() {
            for p in [s.a, s.b] {
                for tol in [1e-7, 1e-300] {
                    assert_eq!(classify_uv(&idx, p[0], p[1], tol), Ok(TrimClass::OnBoundary), "{name} {p:?}");
                }
            }
            // Points exactly on axis-aligned segments.
            if s.a[0] == s.b[0] || s.a[1] == s.b[1] {
                let m = [(s.a[0] + s.b[0]) * 0.5, (s.a[1] + s.b[1]) * 0.5];
                assert_eq!(classify_uv(&idx, m[0], m[1], 1e-300), Ok(TrimClass::OnBoundary), "{name} {m:?}");
            }
        }
    }
}

/// AC-4 and REQ-003: the tolerance is checked first, then the parameter.
#[test]
fn ac4_bad_tolerance_and_domain() {
    let idx = fixture_index("plate_with_hole", "f-plate");
    for tol in [0.0, -0.0, -1e-7, f64::NAN, f64::INFINITY] {
        assert!(matches!(classify_uv(&idx, 0.05, 0.05, tol), Err(TrimError::BadTolerance { .. })), "{tol}");
        assert!(matches!(classify_uv(&idx, f64::NAN, 0.05, tol), Err(TrimError::BadTolerance { .. })));
    }
    assert_eq!(classify_uv(&idx, 0.05, 0.05, 0.0).unwrap_err().code(), "BadTolerance");
    for (u, v) in [(f64::NAN, 0.0), (0.0, f64::INFINITY), (f64::NEG_INFINITY, 0.0)] {
        assert!(matches!(classify_uv(&idx, u, v, 1e-7), Err(TrimError::OutsideDomain { .. })));
    }
    // The sphere's v is limited to [-pi/2, pi/2]; u is periodic but must stay near the chart.
    let sph = fixture_index("sphere_meridian", "f-hemisphere");
    assert!(matches!(classify_uv(&sph, 1.0, 1.6, 1e-7), Err(TrimError::OutsideDomain { .. })));
    assert!(matches!(classify_uv(&sph, 1e300, 0.0, 1e-7), Err(TrimError::OutsideDomain { .. })));
    // At most 2^20 periods from the chart.
    let far = 2f64.powi(21) * TAU;
    assert!(matches!(classify_uv(&sph, far, 0.0, 1e-7), Err(TrimError::OutsideDomain { .. })));
    assert_eq!(classify_uv(&sph, 4.0 + 1000.0 * TAU, 0.0, 1e-7), Ok(TrimClass::Inside));
}

/// A failed pcurve or a winding loop gives a typed Undecidable for every point, never a guess.
#[test]
fn undecidable_faces_never_guess() {
    let idx = fixture_index("plate_with_hole_offset", "f-plate");
    let r = classify_uv(&idx, 0.09, 0.07, 1e-7);
    match r {
        Err(TrimError::Undecidable(Undecidable::PcurveFailed { reason: FailReason::Tolerance, .. })) => {}
        other => panic!("{other:?}"),
    }
    assert!(idx.segments().is_empty());
    // The tolerance and domain are still checked first.
    assert!(matches!(classify_uv(&idx, 0.09, 0.07, 0.0), Err(TrimError::BadTolerance { .. })));
    let straddle = fixture_index("sphere_pole_straddle", "f-hemisphere");
    assert!(matches!(
        classify_uv(&straddle, 1.0, 0.0, 1e-7),
        Err(TrimError::Undecidable(Undecidable::PcurveFailed { .. }))
    ));
    for (name, label, periods) in [("torus_tubes", "f-torus", [0, 1])] {
        let idx = fixture_index(name, label);
        match classify_uv(&idx, 1.0, 0.01, 1e-7) {
            Err(TrimError::Undecidable(Undecidable::Winding { periods: p, .. })) => assert_eq!(p, periods, "{name}"),
            other => panic!("{name}: {other:?}"),
        }
    }
}

/// Seams are crossed, not boundary: the capped cylinder's side face (a rectangle
/// [0, 2 pi] x [0, 1] closed by a seam edge used twice).
#[test]
fn seam_is_interior() {
    let body = build("cylinder_capped");
    let f = body
        .loops()
        .iter()
        .map(|l| l.face())
        .find(|&f| matches!(body.face(f).unwrap().surface().as_analytic(), Some(AnalyticSurface::Cylinder(_))))
        .unwrap();
    let idx = index(&body, f);
    assert_eq!(idx.segments().iter().filter(|s| s.seam).count(), 2);
    let tol = 1e-7;
    for (u, v, want) in [
        (0.0, 0.5, TrimClass::Inside),
        (TAU, 0.5, TrimClass::Inside),
        (-TAU, 0.5, TrimClass::Inside),
        (1e-300, 0.5, TrimClass::Inside),
        (-1e-12, 0.5, TrimClass::Inside),
        (PI, 0.5, TrimClass::Inside),
        (-3.0, 0.5, TrimClass::Inside),
        (20.0, 0.25, TrimClass::Inside),
        (1.0, 1.5, TrimClass::Outside),
        (0.0, -0.5, TrimClass::Outside),
        (1.0, 1.0, TrimClass::OnBoundary),
        (0.0, 1.0, TrimClass::OnBoundary),
        (TAU, 0.0, TrimClass::OnBoundary),
        (2.0, 1.0 + 0.5e-7, TrimClass::OnBoundary),
        (2.0, 1.0 + 2e-7, TrimClass::Outside),
        (2.0, 1.0 - 2e-7, TrimClass::Inside),
    ] {
        assert_eq!(classify_uv(&idx, u, v, tol), Ok(want), "({u}, {v})");
    }
}

/// Poles and apexes follow PCURVE's chart: the pole is on the boundary of the lune bounded by a
/// great circle through both poles, at every u; the cone apex likewise.
#[test]
fn poles_and_apex() {
    let sph = fixture_index("sphere_meridian", "f-hemisphere");
    let tol = 1e-7;
    // The meridian loop runs up u = 0, along the north pole to u = pi, down u = pi and back
    // along the south pole: clockwise in the chart, so with material on its left the face is
    // u in [pi, 2 pi] (y < 0).
    for (u, v, want) in [
        (FRAC_PI_2, 0.0, TrimClass::Outside),
        (0.5, 0.3, TrimClass::Outside),
        (3.0, -1.2, TrimClass::Outside),
        (FRAC_PI_2, 1.5, TrimClass::Outside),
        (3.0 * FRAC_PI_2, 0.0, TrimClass::Inside),
        (-FRAC_PI_2, 0.0, TrimClass::Inside),
        (PI + 0.5, 0.3, TrimClass::Inside),
        (-0.5, 1.5, TrimClass::Inside),
        (0.0, 0.3, TrimClass::OnBoundary),
        (PI, -0.3, TrimClass::OnBoundary),
        (1.0, FRAC_PI_2, TrimClass::OnBoundary),
        (4.0, FRAC_PI_2, TrimClass::OnBoundary),
        (5.0, -FRAC_PI_2, TrimClass::OnBoundary),
    ] {
        assert_eq!(classify_uv(&sph, u, v, tol), Ok(want), "sphere ({u}, {v})");
    }
    // The sector loop (up generator 0, along the arc +u, down generator 1, back along the apex)
    // is clockwise in the chart: the face is everything of the cone except the sector.
    let cone = fixture_index("cone_apex_sector", "f-cone");
    for (u, v, want) in [
        (PI / 4.0, 0.025, TrimClass::Outside),
        (PI, 0.025, TrimClass::Inside),
        (PI / 4.0, 0.06, TrimClass::Inside),
        (PI / 4.0, -0.01, TrimClass::Inside),
        (1.0, 0.0, TrimClass::OnBoundary),
        (3.0, 0.0, TrimClass::OnBoundary),
        (PI / 4.0, 0.05, TrimClass::OnBoundary),
        (0.0, 0.03, TrimClass::OnBoundary),
    ] {
        assert_eq!(classify_uv(&cone, u, v, tol), Ok(want), "cone ({u}, {v})");
    }
}

/// Seam spheres, a cap, a band and a capped cylinder: the column rule against the analytic
/// faces (exact rational inside/outside, closed-form band), including the poles.
#[test]
fn periodic_faces_match_analytic_answers() {
    let e = expected();
    let mut total = 0;
    for (name, spec) in e.get("periodic").obj() {
        let body = build(name);
        let f = match spec.get("face") {
            J::Null => body
                .loops()
                .iter()
                .map(|l| l.face())
                .find(|&f| matches!(body.face(f).unwrap().surface().as_analytic(), Some(AnalyticSurface::Cylinder(_))))
                .unwrap(),
            j => face(&body, j.str()),
        };
        let idx = index(&body, f);
        assert_eq!(idx.undecidable(), None, "{name}");
        let tol = spec.get("tol_m").f();
        for c in spec.get("cases").arr() {
            let (u, v, want) = row(c);
            assert_eq!(classify_uv(&idx, u, v, tol), Ok(want), "{name} ({u:e}, {v:e})");
            total += 1;
        }
    }
    assert!(total > 1500, "{total}");
}

/// A plane loop that uses a bridge edge twice: both uses are boundary (no periodic direction).
#[test]
fn bridge_edge_is_boundary_on_a_plane() {
    let idx = fixture_index("trim_polygons", "f-bridge");
    assert!(idx.segments().iter().all(|s| !s.seam));
    let s = 1.0 / 64.0;
    let tol = 1e-7;
    for (u, v, want) in [
        (0.5 * s, 2.0 * s, TrimClass::OnBoundary),
        (0.5 * s, 2.0 * s + tol / 2.0, TrimClass::OnBoundary),
        (0.5 * s, 2.0 * s - tol / 2.0, TrimClass::OnBoundary),
        (0.5 * s, 2.0 * s + 2.0 * tol, TrimClass::Inside),
        (2.0 * s, 2.0 * s, TrimClass::Outside),
        (3.5 * s, 2.0 * s, TrimClass::Inside),
    ] {
        assert_eq!(classify_uv(&idx, u, v, tol), Ok(want), "({u}, {v})");
    }
}

fn nurbs_line(a: [f64; 2], b: [f64; 2]) -> Curve2 {
    Curve2::nurbs(
        NurbsCurve::new(NurbsCurveParts {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            ctrl: vec![[a[0], a[1], 0.0], [b[0], b[1], 0.0]],
            weights: None,
            periodic: false,
        })
        .unwrap(),
    )
    .unwrap()
}

/// A bilinear NURBS patch equal to the plane z = 0 on [0, 1]^2 (so 3-D distance is uv distance),
/// trimmed to [0.25, 0.75]^2 by imported NURBS pcurves (flattened) and line pcurves.
#[test]
fn nurbs_surface_and_nurbs_pcurves() {
    let s = NurbsSurface::new(NurbsSurfaceParts {
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
    .unwrap();
    let mut bb = BodyBuilder::new([7; 16]);
    let c = [[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]];
    let vs: Vec<usize> = c.iter().map(|p| bb.add_vertex([p[0], p[1], 0.0], None)).collect();
    let mut cs = Vec::new();
    for k in 0..4 {
        let (a, b) = (c[k], c[(k + 1) % 4]);
        let e = bb.add_edge(None, vs[k], vs[(k + 1) % 4], [0.0, 1.0], None);
        let pc = if k % 2 == 0 { nurbs_line(a, b) } else { Curve2::line(a, [b[0] - a[0], b[1] - a[1]]).unwrap() };
        cs.push(bb.add_coedge(e, Sense::Same, Some(pc), None));
    }
    let l = bb.add_loop(cs, true, None);
    let f = bb.add_face(SurfaceDef::nurbs(s).unwrap(), Sense::Same, vec![l], None);
    bb.add_shell(vec![f], false);
    let body = bb.build().unwrap();
    let fx = body.loops()[0].face();
    let idx = index(&body, fx);
    assert_eq!(idx.undecidable(), None);
    // 16 chords per NURBS pcurve, one per line pcurve.
    assert_eq!(idx.segments().len(), 2 * 16 + 2);
    let tol = 1e-7;
    for (u, v, want) in [
        (0.5, 0.5, TrimClass::Inside),
        (0.25, 0.5, TrimClass::OnBoundary),
        (0.5, 0.25, TrimClass::OnBoundary),
        (0.25 - 0.5e-7, 0.5, TrimClass::OnBoundary),
        (0.5, 0.75 + 0.9e-7, TrimClass::OnBoundary),
        (0.25 - 2e-7, 0.5, TrimClass::Outside),
        (0.25 + 2e-7, 0.5, TrimClass::Inside),
        (0.1, 0.9, TrimClass::Outside),
        (1.0, 1.0, TrimClass::Outside),
    ] {
        assert_eq!(classify_uv(&idx, u, v, tol), Ok(want), "({u}, {v})");
    }
    // A smaller tolerance re-flattens the NURBS pcurves at tol / 4; straight ones still give
    // the same chords and answers.
    for (u, v, want) in
        [(0.5, 0.5, TrimClass::Inside), (0.25, 0.5, TrimClass::OnBoundary), (0.1, 0.5, TrimClass::Outside)]
    {
        assert_eq!(classify_uv(&idx, u, v, 1e-12), Ok(want), "({u}, {v})");
    }
    assert!(matches!(classify_uv(&idx, 1.5, 0.5, tol), Err(TrimError::OutsideDomain { .. })));
    assert!(matches!(classify_uv(&idx, 0.5, -0.01, tol), Err(TrimError::OutsideDomain { .. })));
}

/// A face with no loops is the whole surface.
#[test]
fn no_loops_is_whole_surface() {
    let s = SurfaceDef::analytic(fid_geo_cad::SurfaceParams::Plane {
        frame: fid_geo_cad::Frame { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] },
    })
    .unwrap();
    let mut bb = BodyBuilder::new([9; 16]);
    let f = bb.add_face(s, Sense::Same, vec![], None);
    bb.add_shell(vec![f], false);
    let body = bb.build().unwrap();
    let fx = first_face(&body);
    let idx = index(&body, fx);
    assert_eq!(classify_uv(&idx, 123.0, -4.0, 1e-7), Ok(TrimClass::Inside));
}

fn first_face(body: &Body) -> FaceIx {
    let p = derive_pcurves(body, DEFAULT_TOL_M).unwrap();
    // The only face: find it through the build API by trying the face of every loop, else
    // the TrimIndex error path for an unknown face.
    assert_eq!(body.faces().len(), 1);
    assert_eq!(p.charts.len(), 1);
    match body.lookup(body.faces()[0].id()) {
        Some(EntityIx::Face(f)) => f,
        other => panic!("{other:?}"),
    }
}

/// The pcurves must come from the same body.
#[test]
fn mismatched_pcurves_are_refused() {
    let plate = build("plate_with_hole");
    let other = build("cylinder_window");
    let p = derive_pcurves(&other, DEFAULT_TOL_M).unwrap();
    assert_eq!(TrimIndex::build(&plate, &p, face(&plate, "f-plate")), Err(TrimError::Mismatch));
}

/// The exact orientation predicate against the rational oracle, both orientations.
#[test]
fn orientation_signs_are_exact() {
    let e = expected();
    let cases = e.get("orient").arr();
    assert!(cases.len() > 400);
    let val = |s: &str| -> Shifted {
        let (x, k) = s.split_once(':').unwrap();
        Shifted { x: hexf(x), k: k.parse().unwrap(), p: TAU }
    };
    for c in cases {
        let f: Vec<&str> = c.str().split(' ').collect();
        assert_eq!(f.len(), 7);
        let pt = |i: usize| [val(f[i]), val(f[i + 1])];
        let want = match f[6] {
            "1" => Ordering::Greater,
            "-1" => Ordering::Less,
            "0" => Ordering::Equal,
            s => panic!("sign {s}"),
        };
        assert_eq!(orient(pt(0), pt(2), pt(4)), Some(want), "{c:?}");
    }
}

/// Imported pcurves that jump a whole period between coedges are made continuous: the window
/// with its bottom pcurve written one period to the right classifies as before.
#[test]
fn imported_period_jump_is_unwrapped() {
    let mut script = load("cylinder_window.json");
    if let J::Obj(top) = &mut script {
        if let Some(J::Arr(coedges)) = top.get_mut("coedges") {
            if let Some(J::Obj(c)) = coedges.get_mut(0) {
                if let Some(J::Obj(p)) = c.get_mut("pcurve") {
                    p.insert("origin".into(), J::Arr(vec![J::Num(TAU), J::Num(0.0)]));
                }
            }
        }
    }
    let body = common::builder(&script).build().unwrap();
    let idx = index(&body, face(&body, "f-window"));
    assert_eq!(idx.undecidable(), None);
    let lo = idx.segments().iter().map(|s| s.bbox[0][0]).fold(f64::INFINITY, |a, b| if b < a { b } else { a });
    assert!(lo > 5.0, "the loop was moved with its first coedge: {lo}");
    let e = expected();
    let ac2 = e.get("ac2");
    let tol = ac2.get("tol_m").f();
    for c in ac2.get("cases").arr() {
        let (u, v, want) = row(c);
        assert_eq!(classify_uv(&idx, u, v, tol), Ok(want), "u {u:e} v {v:e}");
    }
}

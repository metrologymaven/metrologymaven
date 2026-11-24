//! GEO.DIST.FACE.ONE: closest point on a single trimmed face.
//!
//! Expected values come from the independent oracle fixtures of GEO.DIST.FACE.REF
//! (`fixtures/geo/dist/face/ref_v1`, written by `tools/fixtures/face_proj_oracle.py` at 60
//! decimal digits with exact rational trim tests; nothing here computes an expected value
//! with this crate) or are hand-labelled from geometry (a point on a cylinder axis is the
//! radius away from the cylinder). `OUTPUT_PIN` is a regression pin of this crate's own
//! output (not an oracle), recorded from a local x86-64 run; CI checks it on Linux, Windows
//! and macOS (AC-5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_geo_cad::{derive_pcurves, Body, FaceIx, DEFAULT_TOL_M};
use fid_geo_dist::{
    project_face, FaceError, FaceFeature, FaceHit, FaceProjection, FaceProjector, FaceQueryOptions, Unsupported,
    FACE_TIE_WINDOW_M,
};
use fid_kci::kci::{to_hex, Sha256};

struct Prepared<'a> {
    faces: Vec<FaceProjector<'a>>,
}

/// One single-face body per face of a fixture body, in face-ID order.
fn face_bodies(fx: &Fixture) -> Vec<Body> {
    faces(&fx.body).iter().map(|(l, _)| build_face_body(&fx.body, l)).collect()
}

fn only_face(body: &Body) -> FaceIx {
    match body.lookup(body.faces()[0].id()).unwrap() {
        fid_geo_cad::EntityIx::Face(fx) => fx,
        other => panic!("{other:?}"),
    }
}

fn prepare(bodies: &[Body]) -> Prepared<'_> {
    let mut faces = Vec::new();
    for body in bodies {
        let pc = derive_pcurves(body, DEFAULT_TOL_M).unwrap();
        faces.push(FaceProjector::new(body, &pc, only_face(body)).unwrap());
    }
    Prepared { faces }
}

fn label(id: &fid_geo_cad::TopologyId) -> String {
    match id {
        fid_geo_cad::TopologyId::Imported(l) => l.as_str().to_string(),
        other => panic!("derived id {other}"),
    }
}

fn to_hit(h: &FaceHit) -> Hit {
    let feature = match &h.feature {
        FaceFeature::Interior => Feature::Interior,
        FaceFeature::Edge(e) => Feature::Edge(label(e)),
        FaceFeature::Vertex(v) => Feature::Vertex(label(v)),
    };
    Hit { face: label(&h.face), feature, foot: h.foot, distance: h.distance }
}

/// Every face of the body, minimum with the lowest face ID on a tie (the body rule of the
/// fixtures; GEO.DIST.FACE.BODY owns the real one).
fn project_all(pr: &Prepared<'_>, p: [f64; 3], opts: FaceQueryOptions) -> Result<Option<FaceHit>, String> {
    let mut best: Option<FaceHit> = None;
    for f in &pr.faces {
        match f.project(p, opts).map_err(|e| format!("{}: {e:?}", label(f.face_id())))? {
            FaceProjection::Hit(h) => {
                if best.as_ref().map_or(true, |b| h.distance + FACE_TIE_WINDOW_M < b.distance) {
                    best = Some(h);
                }
            }
            FaceProjection::NoHit => {}
        }
    }
    Ok(best)
}

/// The torus segment's face F1 (a band of the torus, closed in v by a seam edge) cannot be
/// classified by GEO.CAD.TOPO.TRIM, which refuses a seam on a doubly periodic surface; the
/// projection reports that instead of guessing (open item in NODE.md).
const TRIM_GAP: &str = "torus_segment.json";

fn hit_bytes(h: &mut Sha256, r: &Result<FaceProjection, FaceError>) {
    match r {
        Ok(FaceProjection::Hit(x)) => {
            h.update(b"H");
            for v in x.foot.iter().chain([x.distance].iter()).chain(x.uv.iter()) {
                h.update(&v.to_bits().to_le_bytes());
            }
            let (k, id) = match &x.feature {
                FaceFeature::Interior => ("I", String::new()),
                FaceFeature::Edge(e) => ("E", label(e)),
                FaceFeature::Vertex(v) => ("V", label(v)),
            };
            h.update(k.as_bytes());
            h.update(id.as_bytes());
            h.update(label(&x.face).as_bytes());
        }
        Ok(FaceProjection::NoHit) => h.update(b"N"),
        Err(e) => {
            h.update(b"X");
            h.update(e.code().as_bytes());
        }
    }
}

/// Regression pin over every face result of every query of `ref_v1` (AC-5); see the module
/// documentation.
const OUTPUT_PIN: &str = "54d7239f1fd3ef6fba5e1759d57c82035dee4571db3d18abb435770a44fdf487";

/// AC-1: every single-face query of the reference bodies agrees with the oracle: foot and
/// distance within 1e-9 m and the feature by the fixture rule (the body answer is the
/// minimum over the faces with the lowest face ID on a tie). AC-5: the bits of every face
/// result hash to `OUTPUT_PIN` on every CI platform.
#[test]
fn ac1_ac5_reference_fixtures_and_output_pin() {
    let mut total = 0;
    let mut bad = Vec::new();
    let mut pin = Sha256::new();
    for file in manifest_files() {
        let fx = load(&file);
        let bodies = face_bodies(&fx);
        let pr = prepare(&bodies);
        let opts = FaceQueryOptions::default();
        for q in &fx.queries {
            for f in &pr.faces {
                hit_bytes(&mut pin, &f.project(q.p, opts));
            }
            if fx.file == TRIM_GAP {
                let f1 = pr.faces.iter().find(|f| label(f.face_id()) == "F1").unwrap();
                match f1.project(q.p, opts) {
                    Err(FaceError::Trim(fid_geo_cad::TrimError::Undecidable(u))) => {
                        assert_eq!(u.code(), "DoublyPeriodicSeam", "{}", q.id)
                    }
                    other => panic!("{} {}: the trim gap has closed ({other:?}); compare it", fx.file, q.id),
                }
                continue;
            }
            total += 1;
            let h = match project_all(&pr, q.p, opts) {
                Ok(h) => h.unwrap(),
                Err(e) => {
                    bad.push(format!("{} {} [{}]: error {e}", fx.file, q.id, q.gen));
                    continue;
                }
            };
            let hit = to_hit(&h);
            if !matches(&fx, q, &hit) {
                bad.push(format!(
                    "{} {} [{}]: got {} {:?} d={:e} foot={:?}; want {} {:?} d={:e} foot={:?}",
                    fx.file,
                    q.id,
                    q.gen,
                    hit.face,
                    hit.feature,
                    hit.distance,
                    hit.foot,
                    q.answer.face,
                    q.answer.feature,
                    q.answer.distance,
                    q.answer.foot
                ));
            }
        }
    }
    for b in &bad {
        eprintln!("{b}");
    }
    assert!(bad.is_empty(), "{} of {total} queries disagree with the oracle", bad.len());
    assert!(total >= 1100, "only {total} queries compared");
    let got = to_hex(&pin.finish());
    assert_eq!(got, OUTPUT_PIN, "AC-5 output pin");
}

/// Projects every query of one fixture body.
fn run_file(file: &str, opts: impl Fn(&Query) -> FaceQueryOptions) -> Vec<(Query, Option<FaceHit>)> {
    let fx = load(file);
    let bodies = face_bodies(&fx);
    let pr = prepare(&bodies);
    fx.queries.iter().map(|q| (q.clone(), project_all(&pr, q.p, opts(q)).unwrap())).collect()
}

/// AC-2: on the planar-annulus trap (paraboloid annulus, query above the hole's centre) the
/// foot is on the inner circle edge E2, not at the trimmed-away surface minimum.
#[test]
fn ac2_annulus_trap() {
    let fx = load("annulus_trap.json");
    let mut n = 0;
    for (q, h) in run_file("annulus_trap.json", |_| FaceQueryOptions::default()) {
        if q.gen != "trap" {
            continue;
        }
        let h = h.unwrap();
        assert_eq!(h.feature, FaceFeature::Edge(fid_geo_cad::TopologyId::Imported(lbl("E2"))), "{}", q.id);
        assert!(h.distance > q.untrimmed_distance + 1e-9, "{}: trimmed-away minimum returned", q.id);
        assert!(matches(&fx, &q, &to_hit(&h)), "{}", q.id);
        n += 1;
    }
    assert!(n >= 20, "{n} trap queries");
}

fn lbl(s: &str) -> fid_geo_cad::Label {
    fid_geo_cad::Label::new(s).unwrap()
}

/// AC-3: on the saddle whose global surface minimum is trimmed away, the inside local
/// minimum is returned (every query whose untrimmed minimum is trimmed away and whose
/// reference answer is interior).
#[test]
fn ac3_hidden_interior_minimum() {
    let fx = load("saddle_two_minima.json");
    let mut n = 0;
    for (q, h) in run_file("saddle_two_minima.json", |_| FaceQueryOptions::default()) {
        if !(q.has("trimmed_away") && q.answer.feature == Feature::Interior) {
            continue;
        }
        let h = h.unwrap();
        assert_eq!(h.feature, FaceFeature::Interior, "{}", q.id);
        assert!(matches(&fx, &q, &to_hit(&h)), "{}", q.id);
        assert!(h.distance > q.untrimmed_distance + 1e-9, "{}", q.id);
        n += 1;
    }
    assert!(n >= 5, "{n} hidden-minimum queries");
}

/// AC-4 / REQ-004: a maximum distance below the oracle's distance gives NoHit on every
/// face; one above it by twice the comparison tolerance gives the hit.
#[test]
fn ac4_max_distance() {
    for file in ["cube.json", "saddle_two_minima.json", "sphere_cap.json"] {
        let below =
            |q: &Query| FaceQueryOptions { max_distance: Some(q.answer.distance * 0.999), ..Default::default() };
        for (q, h) in run_file(file, below) {
            assert!(h.is_none(), "{file} {}: hit below the true distance", q.id);
        }
        let above = |q: &Query| FaceQueryOptions {
            max_distance: Some(q.answer.distance + 2.0 * fid_geo_dist::FACE_REF_FOOT_TOL_M),
            ..Default::default()
        };
        for (q, h) in run_file(file, above) {
            let h = h.unwrap_or_else(|| panic!("{file} {}: no hit within the true distance", q.id));
            assert!((h.distance - q.answer.distance).abs() <= 1e-9, "{file} {}", q.id);
        }
    }
}

/// REQ-002 for analytic faces: every foot lies on its surface (implicit residual) within
/// 1e-9 m; and the call is repeatable bit for bit (REQ-006).
#[test]
fn req002_feet_on_surface_and_repeatable() {
    for file in ["cube.json", "tube.json", "cone_frustum.json", "sphere_cap.json"] {
        let fx = load(file);
        let bodies = face_bodies(&fx);
        let pr = prepare(&bodies);
        let surfs = faces(&fx.body);
        for q in fx.queries.iter().step_by(3) {
            for f in &pr.faces {
                let a = f.project(q.p, FaceQueryOptions::default()).unwrap();
                let b = f.project(q.p, FaceQueryOptions::default()).unwrap();
                assert_eq!(a, b, "{file} {}", q.id);
                let FaceProjection::Hit(h) = a else { panic!("{file} {}: no hit", q.id) };
                let s = surfs.iter().find(|(l, _)| *l == label(&h.face)).unwrap().1.clone().unwrap();
                assert!(s.residual(h.foot) <= 1e-9, "{file} {} {}: off surface", q.id, label(&h.face));
                assert!((norm(sub(h.foot, q.p)) - h.distance).abs() <= 1e-15, "{file} {}", q.id);
            }
        }
    }
}

/// Stationary points that form a circle or a sphere (hand-labelled: every point of the
/// circle or sphere is at the same distance).
#[test]
fn stationary_continua() {
    let fx = load("tube.json");
    let bodies = face_bodies(&fx);
    let pr = prepare(&bodies);
    // Outer cylinder F1: radius 0.0625 about the z axis through (0.015625, -0.03125), z in
    // [0.0078125, 0.1015625]; a point on the axis at z = 0.05 is 0.0625 from it.
    let f1 = pr.faces.iter().find(|f| label(f.face_id()) == "F1").unwrap();
    let FaceProjection::Hit(h) = f1.project([0.015625, -0.03125, 0.05], FaceQueryOptions::default()).unwrap() else {
        panic!()
    };
    assert_eq!(h.feature, FaceFeature::Interior);
    assert!((h.distance - 0.0625).abs() <= 1e-15, "{}", h.distance);
    // Sphere cap F1: centre (-0.0234375, 0.046875, 0.03125), radius 0.0625.
    let fx = load("sphere_cap.json");
    let bodies = face_bodies(&fx);
    let pr = prepare(&bodies);
    let f1 = pr.faces.iter().find(|f| label(f.face_id()) == "F1").unwrap();
    let FaceProjection::Hit(h) = f1.project([-0.0234375, 0.046875, 0.03125], FaceQueryOptions::default()).unwrap()
    else {
        panic!()
    };
    assert!((h.distance - 0.0625).abs() <= 1e-15, "{}", h.distance);
}

/// Errors: a non-finite query, bad options, and a NURBS face with an interior knot of
/// multiplicity equal to the degree (a possible crease) are refused.
#[test]
fn refusals() {
    let fx = load("cube.json");
    let body = build_face_body(&fx.body, "F1");
    let ix = only_face(&body);
    let o = FaceQueryOptions::default();
    assert_eq!(project_face(&body, ix, [f64::NAN, 0.0, 0.0], o), Err(FaceError::NonFiniteQuery));
    assert_eq!(project_face(&body, ix, [0.0, f64::INFINITY, 0.0], o), Err(FaceError::NonFiniteQuery));
    for bad in [
        FaceQueryOptions { topo_tol_m: 0.0, ..o },
        FaceQueryOptions { topo_tol_m: f64::NAN, ..o },
        FaceQueryOptions { max_distance: Some(-1.0), ..o },
        FaceQueryOptions { max_distance: Some(f64::NAN), ..o },
    ] {
        assert_eq!(project_face(&body, ix, [0.1, 0.0, 0.0], bad), Err(FaceError::BadOptions));
    }
    // A degree-1 NURBS face (two bilinear patches: a crease line at u = 0.5).
    let mut j = load("saddle_two_minima.json").body;
    let Json::Obj(top) = &mut j else { panic!() };
    let faces_json = &mut top.iter_mut().find(|(k, _)| k == "faces").unwrap().1;
    let Json::Arr(fs) = faces_json else { panic!() };
    let Json::Obj(f) = &mut fs[0] else { panic!() };
    f.iter_mut().find(|(k, _)| k == "surface").unwrap().1 = parse_json(
        r#"{"type": "nurbs", "degree_u": 1, "degree_v": 1, "n_u": 3, "n_v": 2,
        "knots_u": [0, 0, 0.5, 1, 1], "knots_v": [0, 0, 1, 1],
        "ctrl": [[0, 0, 0], [0, 1, 0], [0.5, 0, 0.1], [0.5, 1, 0.1], [1, 0, 0], [1, 1, 0]],
        "weights": null}"#,
    );
    let body = build_face_body(&j, "F1");
    let pc = derive_pcurves(&body, DEFAULT_TOL_M).unwrap();
    let e = FaceProjector::new(&body, &pc, only_face(&body)).unwrap_err();
    assert_eq!(e, FaceError::Unsupported(Unsupported::Crease));
    assert_eq!(e.code(), "Unsupported");
}

/// Features and tie rules on the cube (hand-labelled): over a face centre the foot is
/// interior; off the middle of an edge it is that edge; off a corner it is that vertex.
#[test]
fn cube_features() {
    let fx = load("cube.json");
    // Cube [0.0625, 0.1875] x [-0.03125, 0.09375] x [0.015625, 0.140625]; F6 is the top
    // (z = 0.140625), E2 runs along x at y = -0.03125, z = 0.140625 from V5 to V6.
    let body = build_face_body(&fx.body, "F6");
    let ix = only_face(&body);
    let o = FaceQueryOptions::default();
    let get = |p: [f64; 3]| match project_face(&body, ix, p, o).unwrap() {
        FaceProjection::Hit(h) => h,
        FaceProjection::NoHit => panic!("no hit"),
    };
    let h = get([0.125, 0.03125, 0.2]);
    assert_eq!(h.feature, FaceFeature::Interior);
    assert_eq!(h.foot, [0.125, 0.03125, 0.140625]);
    let h = get([0.125, -0.05, 0.2]);
    assert_eq!(h.feature, FaceFeature::Edge(fid_geo_cad::TopologyId::Imported(lbl("E2"))));
    assert_eq!(h.foot, [0.125, -0.03125, 0.140625]);
    let h = get([0.05, -0.05, 0.2]);
    let FaceFeature::Vertex(v) = &h.feature else { panic!("{:?}", h.feature) };
    assert_eq!(label(v), "V5");
    assert_eq!(h.foot, [0.0625, -0.03125, 0.140625]);
    // A query exactly above the edge: the interior candidate (on the boundary) ties with the
    // edge and wins (Interior before Edge).
    let h = get([0.125, -0.03125, 0.2]);
    assert_eq!(h.feature, FaceFeature::Interior);
}

const ONE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/dist/face/one/");

/// Edges given by an ellipse arc and by a rational NURBS curve (the 1-D branch and bound),
/// against the independent oracle `fixtures/geo/dist/face/one/make_curve_edges.py`.
#[test]
fn ellipse_and_nurbs_edges() {
    for file in ["ellipse_segment.json", "nurbs_arc.json"] {
        let fx = load_from(ONE, file);
        let bodies = face_bodies(&fx);
        let pr = prepare(&bodies);
        let mut edge_answers = 0;
        for q in &fx.queries {
            let h = project_all(&pr, q.p, FaceQueryOptions::default()).unwrap().unwrap();
            let hit = to_hit(&h);
            assert!(
                matches(&fx, q, &hit),
                "{file} {}: got {:?} {:?} d={:e}; want {:?} {:?} d={:e}",
                q.id,
                hit.feature,
                hit.foot,
                hit.distance,
                q.answer.feature,
                q.answer.foot,
                q.answer.distance
            );
            if matches!(q.answer.feature, Feature::Edge(ref e) if e == "E1") {
                edge_answers += 1;
            }
        }
        assert!(edge_answers >= 10, "{file}: {edge_answers} answers on the curved edge");
    }
}

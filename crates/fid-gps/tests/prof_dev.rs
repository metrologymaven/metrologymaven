//! Acceptance tests for `GDT.PROF.DEV` (`fid_gps::prof::dev`).
//!
//! Expected values come from the independent oracle `fixtures/gdt/prof/make_dev_fixtures.py`
//! (60-digit decimal closed forms on the exact binary64 inputs, and the pose digest by
//! Python `hashlib`) or are hand-labelled from the geometry; nothing here computes an
//! expected value with this crate. AC-5 pins the bits of the output for 100 000 seeded points
//! (the same pin on every CI platform).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_geo_cad::{
    Body, BodyBuilder, CurveDef, CurveParams, EntityIx, Frame, Label, Sense, SurfaceDef, SurfaceParams, TopologyId,
};
use fid_geo_dist::{SignStatus, SignUndefined};
use fid_geo_frame::Rigid;
use fid_geo_nurbs::{NurbsSurface, NurbsSurfaceParts};
use fid_geo_types::FrameRef;
use fid_gps::prof::dev::{
    deviations, pose_digest, DevError, DevOutcome, DeviationSet, FaceSet, TolerancedFace, BOUNDARY_TOL_M,
};

type V3 = [f64; 3];

const DOME: &str = include_str!("../../../fixtures/gdt/prof/dev_sphere_plane.json");
const APPROX: &str = include_str!("../../../fixtures/gdt/prof/dev_nurbs_approx.json");

// ------------------------------------------------------------------ JSON helpers

fn get<'a>(v: &'a CValue, key: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(key).unwrap_or(&CValue::Null),
        other => panic!("expected an object for {key}, got {other:?}"),
    }
}

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(x) => *x,
        CValue::I64(x) => *x as f64,
        CValue::U64(x) => *x as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("expected an array, got {other:?}"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

fn truthy(v: &CValue) -> bool {
    matches!(v, CValue::Bool(true))
}

fn is_null(v: &CValue) -> bool {
    matches!(v, CValue::Null)
}

fn v3(v: &CValue) -> V3 {
    let a = arr(v);
    [num(&a[0]), num(&a[1]), num(&a[2])]
}

fn hexf(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s.strip_prefix("0x").unwrap(), 16).unwrap())
}

fn hex3(v: &CValue) -> V3 {
    let a = arr(v);
    [hexf(st(&a[0])), hexf(st(&a[1])), hexf(st(&a[2]))]
}

fn dec(v: &CValue) -> f64 {
    st(v).parse::<f64>().unwrap()
}

fn dec3(v: &CValue) -> V3 {
    let a = arr(v);
    [dec(&a[0]), dec(&a[1]), dec(&a[2])]
}

fn tid(s: &str) -> TopologyId {
    TopologyId::Imported(Label::new(s).unwrap())
}

fn label_of(id: &TopologyId) -> String {
    match id {
        TopologyId::Imported(l) => l.as_str().to_string(),
        other => panic!("derived id {other}"),
    }
}

fn frame_ref() -> FrameRef {
    FrameRef::from_bytes([7; 16])
}

// ------------------------------------------------------------------ topology-JSON bodies

fn frame(j: &CValue) -> Frame {
    Frame { origin: v3(get(j, "origin")), axis: v3(get(j, "axis")), ref_dir: v3(get(j, "ref_dir")) }
}

fn curve(j: &CValue) -> Option<CurveDef> {
    if is_null(j) {
        return None;
    }
    Some(match st(get(j, "type")) {
        // A line runs along its frame's reference direction.
        "line" => CurveDef::analytic(CurveParams::Line {
            frame: Frame { origin: v3(get(j, "origin")), axis: v3(get(j, "normal")), ref_dir: v3(get(j, "direction")) },
        })
        .unwrap(),
        "circle" => CurveDef::analytic(CurveParams::Circle { frame: frame(j), radius: num(get(j, "radius")) }).unwrap(),
        t => panic!("curve type {t}"),
    })
}

fn surface(j: &CValue) -> SurfaceDef {
    match st(get(j, "type")) {
        "plane" => SurfaceDef::analytic(SurfaceParams::Plane { frame: frame(j) }).unwrap(),
        "sphere" => {
            SurfaceDef::analytic(SurfaceParams::Sphere { frame: frame(j), radius: num(get(j, "radius")) }).unwrap()
        }
        "nurbs" => SurfaceDef::nurbs(
            NurbsSurface::new(NurbsSurfaceParts {
                degree_u: num(get(j, "degree_u")) as usize,
                degree_v: num(get(j, "degree_v")) as usize,
                knots_u: arr(get(j, "knots_u")).iter().map(num).collect(),
                knots_v: arr(get(j, "knots_v")).iter().map(num).collect(),
                n_u: num(get(j, "n_u")) as usize,
                n_v: num(get(j, "n_v")) as usize,
                ctrl: arr(get(j, "ctrl")).iter().map(v3).collect(),
                weights: None,
                periodic_u: false,
                periodic_v: false,
            })
            .unwrap(),
        )
        .unwrap(),
        t => panic!("surface type {t}"),
    }
}

fn sense(j: &CValue) -> Sense {
    match st(j) {
        "same" => Sense::Same,
        "reversed" => Sense::Reversed,
        s => panic!("sense {s}"),
    }
}

fn label(j: &CValue) -> Option<&str> {
    let l = get(j, "label");
    if is_null(l) {
        None
    } else {
        Some(st(l))
    }
}

fn idx(v: &CValue) -> usize {
    num(v) as usize
}

fn load_body(j: &CValue) -> Body {
    let hex = st(get(j, "body_id"));
    let mut id = [0u8; 16];
    for (k, b) in id.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[2 * k..2 * k + 2], 16).unwrap();
    }
    let mut b = BodyBuilder::new(id);
    for v in arr(get(j, "vertices")) {
        b.add_vertex(v3(get(v, "point")), label(v));
    }
    for e in arr(get(j, "edges")) {
        let r = arr(get(e, "range"));
        b.add_edge(
            curve(get(e, "curve")),
            idx(get(e, "start")),
            idx(get(e, "end")),
            [num(&r[0]), num(&r[1])],
            label(e),
        );
    }
    for c in arr(get(j, "coedges")) {
        assert!(is_null(get(c, "pcurve")));
        b.add_coedge(idx(get(c, "edge")), sense(get(c, "sense")), None, label(c));
    }
    for l in arr(get(j, "loops")) {
        b.add_loop(arr(get(l, "coedges")).iter().map(idx).collect(), truthy(get(l, "outer")), label(l));
    }
    for f in arr(get(j, "faces")) {
        b.add_face(
            surface(get(f, "surface")),
            sense(get(f, "sense")),
            arr(get(f, "loops")).iter().map(idx).collect(),
            label(f),
        );
    }
    for s in arr(get(j, "shells")) {
        b.add_shell(arr(get(s, "faces")).iter().map(idx).collect(), truthy(get(s, "closed")));
    }
    b.build().unwrap()
}

struct Fixture {
    root: CValue,
    body: Body,
}

fn fixture(text: &str) -> Fixture {
    let root = parse(text.as_bytes()).unwrap();
    let body = load_body(get(&root, "body"));
    Fixture { root, body }
}

fn case<'a>(fx: &'a Fixture, id: &str) -> &'a CValue {
    arr(get(&fx.root, "cases")).iter().find(|c| st(get(c, "id")) == id).unwrap_or_else(|| panic!("case {id}"))
}

fn tol_faces(c: &CValue) -> Vec<TolerancedFace> {
    arr(get(c, "toleranced_faces"))
        .iter()
        .map(|f| TolerancedFace { face: tid(st(get(f, "face"))), approx_error_m: num(get(f, "approx_error_m")) })
        .collect()
}

fn pose(c: &CValue) -> Rigid {
    let p = get(c, "pose");
    let r = arr(get(p, "r"));
    Rigid::new([hex3(&r[0]), hex3(&r[1]), hex3(&r[2])], hex3(get(p, "t"))).unwrap()
}

fn close3(a: V3, b: V3, tol: f64) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() <= tol)
}

/// Runs one oracle case and checks every point against its expected answer.
fn run_case(fx: &Fixture, id: &str) -> DeviationSet {
    let c = case(fx, id);
    let tol = num(get(&fx.root, "tolerance_m"));
    let set = FaceSet::new(&fx.body, frame_ref(), &tol_faces(c)).unwrap();
    let points: Vec<V3> = arr(get(c, "points")).iter().map(hex3).collect();
    let pose = pose(c);
    let out = deviations(&points, &pose, &set, num(get(c, "search_radius_m"))).unwrap();
    assert_eq!(out.pose_digest.as_str(), st(get(c, "pose_digest")), "{id}: pose digest");
    let expected = arr(get(c, "expected"));
    assert_eq!(out.items.len(), expected.len());
    let mut bad = Vec::new();
    for (k, (d, e)) in out.items.iter().zip(expected).enumerate() {
        assert_eq!(d.index as usize, k);
        if truthy(get(e, "no_projection")) {
            if !d.no_projection() {
                bad.push(format!("{id}[{k}]: want no_projection, got {d:?}"));
            }
            continue;
        }
        let Some(p) = d.projected() else {
            bad.push(format!("{id}[{k}]: no projection"));
            continue;
        };
        let want_face = st(get(e, "face"));
        let want_foot = dec3(get(e, "foot"));
        let want_n = dec3(get(e, "normal"));
        let want_dev = dec(get(e, "signed_dev"));
        let want_b = truthy(get(e, "on_boundary"));
        let want_err = if is_null(get(e, "approx_error_m")) { 0.0 } else { dec(get(e, "approx_error_m")) };
        let ok = label_of(&p.face_id) == want_face
            && close3(p.foot_point, want_foot, tol)
            && p.normal.is_some_and(|n| close3(n, want_n, tol))
            && p.signed_dev.is_some_and(|s| (s - want_dev).abs() <= tol)
            && (p.distance - want_dev.abs()).abs() <= tol
            && p.on_boundary == want_b
            && p.approx_error == want_err;
        if !ok {
            bad.push(format!(
                "{id}[{k}]: got {} foot={:?} n={:?} dev={:?} b={} err={:e}; want {want_face} foot={want_foot:?} \
                 n={want_n:?} dev={want_dev:e} b={want_b} err={want_err:e}",
                label_of(&p.face_id),
                p.foot_point,
                p.normal,
                p.signed_dev,
                p.on_boundary,
                p.approx_error
            ));
        }
    }
    assert!(bad.is_empty(), "{} of {} points disagree:\n{}", bad.len(), expected.len(), bad.join("\n"));
    out
}

// ------------------------------------------------------------------ acceptance criteria

/// AC-1: exact sphere (r = 20 mm) and plane faces, points at +-0.05 mm: signed values
/// within 1e-12 m of the oracle, approximation errors 0.
#[test]
fn ac1_sphere_and_plane_offsets() {
    let fx = fixture(DOME);
    let out = run_case(&fx, "ac1_identity");
    assert_eq!(out.no_projection_count(), 0);
    assert!(out.items.iter().all(|d| d.projected().unwrap().approx_error == 0.0));
    assert_eq!(out.max_approx_error, Some(0.0));
    let (mut pos, mut neg) = (0, 0);
    for d in &out.items {
        let p = d.projected().unwrap();
        assert!(!p.on_boundary);
        match p.sign {
            SignStatus::Outside => pos += 1,
            SignStatus::Inside => neg += 1,
            s => panic!("sign {s:?}"),
        }
    }
    assert_eq!((pos, neg), (40, 40));
}

/// AC-2: every deviation on the face declared with 2e-7 m carries 2e-7 m; the exact face
/// carries 0; the result maximum is 2e-7 m.
#[test]
fn ac2_declared_approximation_error() {
    let fx = fixture(APPROX);
    let out = run_case(&fx, "ac2_approx");
    let want = dec(get(&fx.root, "max_approx_error_m"));
    assert_eq!(out.max_approx_error, Some(want));
    let on_n = out.items.iter().filter(|d| label_of(&d.projected().unwrap().face_id) == "N-face").count();
    assert_eq!(on_n, 10);
}

/// AC-3: feet on the shared equator edge are `on_boundary`, and the lower face ID
/// (`F1-base`, face index 1) is reported over `F2-dome` (face index 0).
#[test]
fn ac3_shared_edge_lower_face_id() {
    let fx = fixture(DOME);
    let out = run_case(&fx, "ac3_shared_edge");
    assert!(out.items.iter().all(|d| d.on_boundary()));
}

/// REQ-003 for interior feet: 8e-10 m from the equator is on the boundary, 5e-9 m is not.
#[test]
fn boundary_band_of_interior_feet() {
    let fx = fixture(DOME);
    let out = run_case(&fx, "boundary_band_interior");
    let flags: Vec<bool> = out.items.iter().map(|d| d.on_boundary()).collect();
    assert_eq!(flags, [true, true, false, false]);
    assert_eq!(BOUNDARY_TOL_M, 1e-9);
}

/// AC-4: a point 10 mm away with search radius 1 mm has `no_projection`.
#[test]
fn ac4_beyond_search_radius() {
    let fx = fixture(DOME);
    let out = run_case(&fx, "ac4_far");
    assert_eq!(out.no_projection_count(), 3);
    assert_eq!(out.max_approx_error, None, "nothing hit: no maximum is claimed");
}

/// The pose maps measured points into the nominal frame before projection.
#[test]
fn pose_is_applied_and_digested() {
    let fx = fixture(DOME);
    run_case(&fx, "pose_rot_z");
}

/// Only the toleranced faces are projected onto.
#[test]
fn restriction_to_toleranced_faces() {
    let fx = fixture(DOME);
    let out = run_case(&fx, "restrict_dome");
    assert_eq!(out.no_projection_count(), 4);
}

// ------------------------------------------------------------------ AC-5 determinism

/// SplitMix64 (test streams only).
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn in_range(&mut self, lo: f64, hi: f64) -> f64 {
        let u = (self.next() >> 40) as f64 / 16_777_216.0;
        lo + u * (hi - lo)
    }
}

fn push_f64(out: &mut Vec<u8>, x: f64) {
    out.extend_from_slice(&x.to_bits().to_le_bytes());
}

fn encode(out: &mut Vec<u8>, set: &DeviationSet) {
    for d in &set.items {
        out.extend_from_slice(&d.index.to_le_bytes());
        match &d.outcome {
            DevOutcome::NoProjection => out.push(b'N'),
            DevOutcome::Projected(p) => {
                out.push(b'P');
                out.extend_from_slice(label_of(&p.face_id).as_bytes());
                for x in p.foot_point {
                    push_f64(out, x);
                }
                match p.normal {
                    Some(n) => n.iter().for_each(|x| push_f64(out, *x)),
                    None => out.push(b'-'),
                }
                push_f64(out, p.distance);
                match p.signed_dev {
                    Some(s) => push_f64(out, s),
                    None => out.push(b'-'),
                }
                out.push(p.sign.code());
                push_f64(out, p.approx_error);
                out.push(u8::from(p.on_boundary));
            }
        }
    }
    match set.max_approx_error {
        Some(m) => push_f64(out, m),
        None => out.push(b'-'),
    }
    out.extend_from_slice(set.pose_digest.as_str().as_bytes());
}

/// Regression pin of AC-5 (SHA-256 of every output bit for the 100 000 points below).
const AC5_PIN: &str = "sha256:ceb3b823dd8fa1f189308f80438f894bf0f0688ea9f1fd3507ae52e9ec526348";

/// AC-5: 100 000 seeded points (seed 5) around the dome under a non-trivial pose give
/// bit-identical output on every CI platform (Linux, Windows, macOS).
#[test]
fn ac5_bit_identical_output() {
    let fx = fixture(DOME);
    let faces = tol_faces(case(&fx, "ac1_identity"));
    let set = FaceSet::new(&fx.body, frame_ref(), &faces).unwrap();
    let pose = pose(case(&fx, "pose_rot_z"));
    let mut rng = SplitMix(5);
    let points: Vec<V3> = (0..100_000)
        .map(|_| [rng.in_range(-0.024, 0.024), rng.in_range(-0.024, 0.024), rng.in_range(-0.003, 0.0235)])
        .collect();
    let out = deviations(&points, &pose, &set, 0.002).unwrap();
    let hits = out.items.len() - out.no_projection_count();
    assert!(hits > 10_000, "only {hits} points projected");
    let mut bytes = Vec::new();
    encode(&mut bytes, &out);
    let got = fid_types::Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
    assert_eq!(got.as_str(), AC5_PIN, "AC-5 output pin");
}

// ------------------------------------------------------------------ honesty (D-045) and errors

/// A point on the nominal plane within the foot error bound has no sign of record, so no
/// signed deviation is claimed; exactly on it, the deviation is 0 (hand-labelled).
#[test]
fn no_signed_value_without_a_sign_of_record() {
    let fx = fixture(DOME);
    let faces = tol_faces(case(&fx, "ac1_identity"));
    let set = FaceSet::new(&fx.body, frame_ref(), &faces).unwrap();
    let pts = [[0.001, 0.002, -1e-12], [0.001, 0.002, 0.0]];
    let out = deviations(&pts, &Rigid::IDENTITY, &set, 0.001).unwrap();
    let a = out.items[0].projected().unwrap();
    assert_eq!(a.sign, SignStatus::Undecidable);
    assert_eq!(a.signed_dev, None);
    assert_eq!(a.distance, 1e-12);
    let b = out.items[1].projected().unwrap();
    assert_eq!(b.sign, SignStatus::OnSurface);
    assert_eq!(b.signed_dev, Some(0.0));
}

/// Beyond the open edge of a sheet face the sign is undefined: no signed deviation, but the
/// distance, the foot and the boundary flag are given (hand-labelled: N-face's edge x = 0).
#[test]
fn open_edge_foot_has_no_sign() {
    let fx = fixture(APPROX);
    let faces = tol_faces(case(&fx, "ac2_approx"));
    let set = FaceSet::new(&fx.body, frame_ref(), &faces).unwrap();
    let out = deviations(&[[-0.001, 0.01, 0.001]], &Rigid::IDENTITY, &set, 0.01).unwrap();
    let p = out.items[0].projected().unwrap();
    assert_eq!(label_of(&p.face_id), "N-face");
    assert!(p.on_boundary);
    assert!(matches!(p.sign, SignStatus::Undefined(SignUndefined::Boundary)), "{:?}", p.sign);
    assert_eq!(p.signed_dev, None);
    assert!(close3(p.foot_point, [0.0, 0.01, 0.0], 1e-12));
    assert!((p.distance - 0.001 * core::f64::consts::SQRT_2).abs() <= 1e-12);
    assert_eq!(p.approx_error, 2e-7);
}

#[test]
fn input_errors() {
    let fx = fixture(DOME);
    let faces = tol_faces(case(&fx, "ac1_identity"));
    let set = FaceSet::new(&fx.body, frame_ref(), &faces).unwrap();
    let id = Rigid::IDENTITY;
    let e = deviations(&[[0.0, 0.0, 0.01], [f64::NAN, 0.0, 0.0]], &id, &set, 0.001).unwrap_err();
    assert_eq!(e, DevError::NanInput { index: 1 });
    assert_eq!(e.code(), "nan_input");
    let e = deviations(&[[f64::INFINITY, 0.0, 0.0]], &id, &set, 0.001).unwrap_err();
    assert_eq!(e.code(), "nan_input");
    for r in [f64::NAN, -1e-3, f64::INFINITY] {
        assert_eq!(deviations(&[[0.0; 3]], &id, &set, r).unwrap_err(), DevError::BadSearchRadius);
    }
    let out = deviations(&[], &id, &set, 0.001).unwrap();
    assert!(out.items.is_empty());
    assert_eq!(out.max_approx_error, None);

    let tf = |s: &str, e: f64| TolerancedFace { face: tid(s), approx_error_m: e };
    let new = |f: &[TolerancedFace]| FaceSet::new(&fx.body, frame_ref(), f).map(|_| ()).unwrap_err();
    assert_eq!(new(&[]), DevError::EmptyFaceSet);
    assert_eq!(new(&[tf("F9-none", 0.0)]), DevError::FaceUnknown { face: tid("F9-none") });
    assert_eq!(new(&[tf("E-eq", 0.0)]).code(), "face_unknown");
    assert_eq!(new(&[tf("F2-dome", 0.0), tf("F2-dome", 0.0)]).code(), "duplicate_face");
    for bad in [f64::NAN, -1e-9, f64::INFINITY] {
        assert_eq!(new(&[tf("F1-base", bad)]), DevError::BadApproxError { face: tid("F1-base") });
    }
}

/// The pose digest is the SHA-256 of the canonical pose: the identity's value is computed
/// independently in the oracle fixture.
#[test]
fn pose_digest_matches_oracle() {
    let fx = fixture(DOME);
    let c = case(&fx, "ac1_identity");
    assert_eq!(pose_digest(&Rigid::IDENTITY).unwrap().as_str(), st(get(c, "pose_digest")));
}

// ------------------------------------------------------------------ torus faces fail closed

/// A closed torus (one face, no loops; major 30 mm, minor 5 mm) beside nothing else.
fn torus_body() -> Body {
    let mut b = BodyBuilder::new([0x70; 16]);
    let s = SurfaceDef::analytic(SurfaceParams::Torus {
        frame: Frame { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] },
        major_radius: 0.03,
        minor_radius: 0.005,
    })
    .unwrap();
    let f = b.add_face(s, Sense::Same, Vec::new(), Some("T-face"));
    b.add_shell(vec![f], true);
    b.build().unwrap()
}

/// Any profile on a torus face fails closed: the face set is refused (the trim gap of
/// GEO.DIST.FACE.ONE and BODY), not projected.
#[test]
fn torus_face_is_refused() {
    let body = torus_body();
    assert!(matches!(body.lookup(&tid("T-face")), Some(EntityIx::Face(_))));
    let e = FaceSet::new(&body, frame_ref(), &[TolerancedFace { face: tid("T-face"), approx_error_m: 0.0 }])
        .map(|_| ())
        .unwrap_err();
    assert_eq!(e, DevError::TorusUnsupported { face: tid("T-face") });
    assert_eq!(e.code(), "torus_unsupported");
}

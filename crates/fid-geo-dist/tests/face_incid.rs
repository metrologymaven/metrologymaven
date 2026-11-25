//! GEO.DIST.FACE.INCID: incident normals and angles, and the CAD signed distance.
//!
//! Expected values come from the independent oracle `fixtures/geo/dist/face/incid/
//! make_incid_cases.py` (exact integer inside tests and 60-digit distances on a 2^-30 m grid;
//! nothing there uses this crate) or are hand-labelled from geometry (a point farther from
//! the centre than the radius is outside a sphere; a foot on a free edge has no sign). The
//! point streams below mirror the script bit for bit, checked by the SHA-256 of the points.
//! The feet handed to the module are built here from the oracle's feature with the closed
//! forms of GEO.NURBS.ANALYTIC (they are inputs, not expected values); a subset is also
//! projected through GEO.DIST.FACE.ONE end to end.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use core::f64::consts::{FRAC_PI_2, PI, TAU};

use common::{parse_json, Json};
use fid_geo_cad::{
    Body, BodyBuilder, Curve2, CurveDef, CurveParams, EntityIx, FaceIx, Frame, Label, Sense, SurfaceDef, SurfaceParams,
    TopologyId,
};
use fid_geo_dist::{
    FaceFeature, FaceHit, FaceProjection, FaceProjector, FaceQueryOptions, Incid, IncidError, SignStatus,
    SignUndefined, Undefined, FACE_TIE_WINDOW_M,
};
use fid_geo_nurbs::{AnalyticCurve, NurbsCurve, NurbsCurveParts, NurbsSurface, NurbsSurfaceParts};
use fid_kci::kci::{to_hex, Sha256};

type V3 = [f64; 3];

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/");
const GRID: f64 = 1_073_741_824.0; // 2^30 grid units per metre
const U: i64 = 1 << 30;

// ------------------------------------------------------------------ body loader

fn read(rel: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}{rel}")).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn v3(j: &Json) -> V3 {
    let a = j.arr();
    [a[0].num(), a[1].num(), a[2].num()]
}

fn v2(j: &Json) -> [f64; 2] {
    let a = j.arr();
    [a[0].num(), a[1].num()]
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn label(j: &Json) -> Option<&str> {
    let l = j.get("label");
    if l.is_null() {
        None
    } else {
        Some(l.str())
    }
}

fn frame(j: &Json, off: V3) -> Frame {
    Frame { origin: add(v3(j.get("origin")), off), axis: v3(j.get("axis")), ref_dir: v3(j.get("ref_dir")) }
}

/// A line runs along GEO.NURBS.ANALYTIC's reference direction. The fixtures of this leaf give
/// it as `direction` (with a perpendicular `normal` for the placement axis); the topology
/// fixture `cube_labelled.json` gives the direction in `axis` (its `ref_dir` is a
/// perpendicular), so its two vectors are swapped.
fn line_frame(j: &Json, off: V3) -> Frame {
    let origin = add(v3(j.get("origin")), off);
    if RAW_LINES.with(|r| r.get()) {
        return frame(j, off);
    }
    if j.get("direction").is_null() {
        Frame { origin, axis: v3(j.get("ref_dir")), ref_dir: v3(j.get("axis")) }
    } else {
        Frame { origin, axis: v3(j.get("normal")), ref_dir: v3(j.get("direction")) }
    }
}

thread_local! {
    /// Read `cube_labelled.json` lines as written (along `ref_dir`), for the fail-closed test.
    static RAW_LINES: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

fn nurbs_curve(j: &Json) -> NurbsCurve {
    let ctrl = j
        .get("ctrl")
        .arr()
        .iter()
        .map(|p| {
            let a = p.arr();
            [a[0].num(), a[1].num(), if a.len() == 3 { a[2].num() } else { 0.0 }]
        })
        .collect();
    let w = j.get("weights");
    NurbsCurve::new(NurbsCurveParts {
        degree: j.get("degree").num() as usize,
        knots: j.get("knots").arr().iter().map(Json::num).collect(),
        ctrl,
        weights: if w.is_null() { None } else { Some(w.arr().iter().map(Json::num).collect()) },
        periodic: j.get("periodic").truthy(),
    })
    .unwrap()
}

fn curve(j: &Json, off: V3) -> Option<CurveDef> {
    if j.is_null() {
        return None;
    }
    Some(match j.get("type").str() {
        "line" => CurveDef::analytic(CurveParams::Line { frame: line_frame(j, off) }).unwrap(),
        "circle" => {
            CurveDef::analytic(CurveParams::Circle { frame: frame(j, off), radius: j.get("radius").num() }).unwrap()
        }
        t => panic!("curve type {t}"),
    })
}

fn pcurve(j: &Json) -> Option<Curve2> {
    if j.is_null() {
        return None;
    }
    Some(match j.get("type").str() {
        "line2" => Curve2::line(v2(j.get("origin")), v2(j.get("dir"))).unwrap(),
        "nurbs" => Curve2::nurbs(nurbs_curve(j)).unwrap(),
        t => panic!("pcurve type {t}"),
    })
}

fn surface(j: &Json, off: V3) -> SurfaceDef {
    let fr = frame(j, off);
    let p = match j.get("type").str() {
        "plane" => SurfaceParams::Plane { frame: fr },
        "cylinder" => SurfaceParams::Cylinder { frame: fr, radius: j.get("radius").num() },
        "sphere" => SurfaceParams::Sphere { frame: fr, radius: j.get("radius").num() },
        t => panic!("surface type {t}"),
    };
    SurfaceDef::analytic(p).unwrap()
}

fn sense(j: &Json) -> Sense {
    match j.str() {
        "same" => Sense::Same,
        "reversed" => Sense::Reversed,
        s => panic!("sense {s}"),
    }
}

/// A topology-JSON body (GEO.CAD.TOPO fixture format) translated by `off`.
fn load_body(rel: &str, off: V3) -> Body {
    let j = parse_json(&read(rel));
    let hex = j.get("body_id").str();
    let mut id = [0u8; 16];
    for (k, b) in id.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[2 * k..2 * k + 2], 16).unwrap();
    }
    let mut b = BodyBuilder::new(id);
    for v in j.get("vertices").arr() {
        b.add_vertex(add(v3(v.get("point")), off), label(v));
    }
    for e in j.get("edges").arr() {
        let r = v2(e.get("range"));
        b.add_edge(curve(e.get("curve"), off), e.get("start").num() as usize, e.get("end").num() as usize, r, label(e));
    }
    for c in j.get("coedges").arr() {
        b.add_coedge(c.get("edge").num() as usize, sense(c.get("sense")), pcurve(c.get("pcurve")), label(c));
    }
    for l in j.get("loops").arr() {
        let cs = l.get("coedges").arr().iter().map(|x| x.num() as usize).collect();
        b.add_loop(cs, l.get("outer").truthy(), label(l));
    }
    for f in j.get("faces").arr() {
        let ls = f.get("loops").arr().iter().map(|x| x.num() as usize).collect();
        b.add_face(surface(f.get("surface"), off), sense(f.get("sense")), ls, label(f));
    }
    for s in j.get("shells").arr() {
        b.add_shell(s.get("faces").arr().iter().map(|x| x.num() as usize).collect(), s.get("closed").truthy());
    }
    b.build().unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn tid(s: &str) -> TopologyId {
    TopologyId::Imported(Label::new(s).unwrap())
}

fn face_ix(body: &Body, id: &TopologyId) -> FaceIx {
    match body.lookup(id) {
        Some(EntityIx::Face(f)) => f,
        other => panic!("{id}: {other:?}"),
    }
}

// ------------------------------------------------------------------ pinned point streams

struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: i64) -> i64 {
        (self.next() % n as u64) as i64
    }
}

fn isqrt(n: i64) -> i64 {
    let mut r = (n as f64).sqrt() as i64;
    while r * r > n {
        r -= 1;
    }
    while (r + 1) * (r + 1) <= n {
        r += 1;
    }
    r
}

const MIN_D: i64 = 16;

fn cube_points() -> Vec<[i64; 3]> {
    let mut g = SplitMix(29);
    let mut out = Vec::new();
    for _ in 0..10_000 {
        loop {
            let pick = g.below(20);
            let base = if pick < 12 {
                let (a, bits) = ((pick / 4) as usize, pick % 4);
                let others: Vec<usize> = (0..3).filter(|&i| i != a).collect();
                let mut b = [0i64; 3];
                b[others[0]] = U * (bits & 1);
                b[others[1]] = U * ((bits >> 1) & 1);
                b[a] = g.below(U + 1);
                b
            } else {
                let c = pick - 12;
                [U * (c & 1), U * ((c >> 1) & 1), U * ((c >> 2) & 1)]
            };
            let e = g.below(14);
            let de = 1i64 << (23 - e);
            let mut p = [0i64; 3];
            for (k, x) in p.iter_mut().enumerate() {
                *x = base[k] + g.below(2 * de + 1) - de;
            }
            if cube_distance2(p) >= MIN_D * MIN_D {
                out.push(p);
                break;
            }
        }
    }
    out
}

fn cube_distance2(p: [i64; 3]) -> i64 {
    if p.iter().all(|&k| 0 < k && k < U) {
        let m = p.iter().map(|&k| k.min(U - k)).min().unwrap();
        return m * m;
    }
    p.iter()
        .map(|&k| {
            if k < 0 {
                k * k
            } else if k > U {
                (k - U) * (k - U)
            } else {
                0
            }
        })
        .sum()
}

const R_IN: i64 = U >> 7;
const H: i64 = U >> 7;

fn tube_points() -> Vec<[i64; 3]> {
    let mut g = SplitMix(31);
    let mut out = Vec::new();
    for n in 0..2_000 {
        let zc = if n % 2 == 0 { H } else { 0 };
        loop {
            let x0 = g.below(2 * R_IN + 1) - R_IN;
            let mut y0 = isqrt(R_IN * R_IN - x0 * x0);
            if g.below(2) == 1 {
                y0 = -y0;
            }
            let e = g.below(12);
            let de = 1i64 << (21 - e);
            let p = [x0 + g.below(2 * de + 1) - de, y0 + g.below(2 * de + 1) - de, zc + g.below(2 * de + 1) - de];
            let near = (p[0] * p[0] + p[1] * p[1] - R_IN * R_IN).abs() < 2 * R_IN * MIN_D || (p[2] - zc).abs() < MIN_D;
            if !near {
                out.push(p);
                break;
            }
        }
    }
    out
}

fn metres(p: [i64; 3]) -> V3 {
    [p[0] as f64 / GRID, p[1] as f64 / GRID, p[2] as f64 / GRID]
}

fn digest(pts: &[V3]) -> String {
    let mut h = Sha256::new();
    for p in pts {
        for x in p {
            h.update(&x.to_bits().to_le_bytes());
        }
    }
    to_hex(&h.finish())
}

// ------------------------------------------------------------------ oracle cases

#[derive(Debug, Clone)]
struct Case {
    outside: bool,
    kind: String,
    entity: String,
    face: String,
    distance: f64,
}

/// The `cube` or `tube` section of `cases.txt`: header digest and cases.
fn cases(section: &str) -> (String, Vec<Case>) {
    let text = read("geo/dist/face/incid/cases.txt");
    let mut digest = None;
    let mut count = 0usize;
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let w: Vec<&str> = line.split_whitespace().collect();
        if w[0] == "cube" || w[0] == "tube" {
            inside = w[0] == section;
            if inside {
                count = w[4].parse().unwrap();
                digest = Some(w[6].to_string());
            }
            continue;
        }
        if inside {
            out.push(Case {
                outside: w[0] == "o",
                kind: w[1].to_string(),
                entity: w[2].to_string(),
                face: w[3].to_string(),
                distance: w[4].parse().unwrap(),
            });
        }
    }
    assert_eq!(out.len(), count, "{section}: case count");
    (digest.unwrap(), out)
}

// ------------------------------------------------------------------ feet (inputs)

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len(a: V3) -> f64 {
    ((a[0] * a[0] + a[1] * a[1]) + a[2] * a[2]).sqrt()
}

/// The closest point named by the oracle's feature, from the closed forms.
fn hit_for(body: &Body, p: V3, c: &Case) -> FaceHit {
    let face = tid(&c.face);
    let surf = body.face(face_ix(body, &face)).unwrap().surface().as_analytic().unwrap();
    let (foot, feature) = match c.kind.as_str() {
        "I" => (surf.invert(p).unwrap().foot, FaceFeature::Interior),
        "E" => {
            let e = match body.lookup(&tid(&c.entity)) {
                Some(EntityIx::Edge(e)) => e,
                other => panic!("{other:?}"),
            };
            let curve: &AnalyticCurve = body.edge(e).unwrap().curve().unwrap().as_analytic().unwrap();
            (curve.invert(p).unwrap().foot, FaceFeature::Edge(tid(&c.entity)))
        }
        "V" => {
            let v = match body.lookup(&tid(&c.entity)) {
                Some(EntityIx::Vertex(v)) => v,
                other => panic!("{other:?}"),
            };
            (body.vertex(v).unwrap().point(), FaceFeature::Vertex(tid(&c.entity)))
        }
        k => panic!("kind {k}"),
    };
    let f = surf.invert(foot).unwrap();
    FaceHit { foot, distance: len(sub(p, foot)), face, uv: [f.u, f.v], feature }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-15 * b + 1e-17
}

/// Signs every case of a stream and checks it against the oracle; returns the hits.
fn check_stream(body: &Body, pts: &[V3], cs: &[Case], off: V3) {
    let incid = Incid::new(body).unwrap();
    for (i, (p, c)) in pts.iter().zip(cs).enumerate() {
        let p = add(*p, off);
        let hit = hit_for(body, p, c);
        let s = incid.signed(p, &hit).unwrap_or_else(|e| panic!("case {i} {c:?}: {e}"));
        let want = if c.outside { SignStatus::Outside } else { SignStatus::Inside };
        assert_eq!(s.status, want, "case {i} {c:?} p {p:?}: {s:?}");
        assert_eq!(s.distance > 0.0, c.outside, "case {i}");
        assert!(close(s.unsigned, c.distance), "case {i} {c:?}: distance {} vs oracle {}", s.unsigned, c.distance);
    }
}

// ------------------------------------------------------------------ AC-1: cube edges and corners

#[test]
fn ac1_cube_edges_and_corners_seed_29() {
    let pts: Vec<V3> = cube_points().into_iter().map(metres).collect();
    let (want, cs) = cases("cube");
    assert_eq!(digest(&pts), want, "point stream differs from the oracle's");
    assert_eq!(cs.len(), 10_000);
    for kind in ["I", "E", "V"] {
        assert!(cs.iter().filter(|c| c.kind == kind).count() > 300, "too few {kind} feet");
    }
    assert!(cs.iter().filter(|c| !c.outside).count() > 1_500);
    let body = load_body("geo/cad/topo/cube_labelled.json", [0.0; 3]);
    check_stream(&body, &pts, &cs, [0.0; 3]);
}

/// Accuracy far from the origin: the same cube and points translated by 2^12, -2^13, 2^14 m.
#[test]
fn cube_far_from_origin() {
    let off = [4096.0, -8192.0, 16384.0];
    let pts: Vec<V3> = cube_points().into_iter().take(2_000).map(metres).collect();
    let (_, cs) = cases("cube");
    let body = load_body("geo/cad/topo/cube_labelled.json", off);
    check_stream(&body, &pts, &cs[..2_000], off);
}

#[test]
fn cube_vertex_and_edge_incidents() {
    let body = load_body("geo/cad/topo/cube_labelled.json", [0.0; 3]);
    let incid = Incid::new(&body).unwrap();
    // Corner #8 at (1, 1, 1): three faces, each with interior angle pi / 2, ascending face ID.
    let hit = FaceHit {
        foot: [1.0, 1.0, 1.0],
        distance: 0.0,
        face: tid("#52"),
        uv: [1.0, 1.0],
        feature: FaceFeature::Vertex(tid("#8")),
    };
    let inc = incid.incident_at(&hit).unwrap();
    assert_eq!(inc.len(), 3);
    let normals: Vec<V3> = inc.iter().map(|i| i.normal).collect();
    assert_eq!(normals, vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    for i in &inc {
        assert!((i.angle - FRAC_PI_2).abs() < 1e-15, "{i:?}");
        assert!(!i.fallback);
    }
    // Edge #9 (y = z = 0): faces #53 (normal -y) and #55 (normal -z), pi each.
    let hit = FaceHit {
        foot: [0.25, 0.0, 0.0],
        distance: 0.0,
        face: tid("#53"),
        uv: [0.0, 0.25],
        feature: FaceFeature::Edge(tid("#9")),
    };
    let inc = incid.incident_at(&hit).unwrap();
    assert_eq!(inc.iter().map(|i| i.normal).collect::<Vec<_>>(), vec![[0.0, -1.0, 0.0], [0.0, 0.0, -1.0]]);
    assert!(inc.iter().all(|i| i.angle == PI));
    // On the diagonal plane of the edge, outside and inside.
    let s = incid.signed([0.25, -0.5, -0.5], &hit).unwrap();
    assert_eq!(s.status, SignStatus::Outside);
    // The foot itself: on the surface.
    let s = incid.signed([0.25, 0.0, 0.0], &hit).unwrap();
    assert_eq!(s.status, SignStatus::OnSurface);
    assert_eq!(s.distance.to_bits(), 0.0f64.to_bits());
}

// ------------------------------------------------------------------ AC-2: tube rim

#[test]
fn ac2_tube_hole_rim_is_outside() {
    let pts: Vec<V3> = tube_points().into_iter().map(metres).collect();
    let (want, cs) = cases("tube");
    assert_eq!(digest(&pts), want, "point stream differs from the oracle's");
    assert_eq!(cs.len(), 2_000);
    // The hole side of both rims (edge feet on T-E4 top, T-E3 bottom) is outside the material.
    let rim: Vec<&Case> = cs.iter().filter(|c| c.kind == "E").collect();
    assert!(rim.len() > 400);
    assert!(rim.iter().all(|c| c.outside && (c.entity == "T-E4" || c.entity == "T-E3")));
    let body = load_body("geo/dist/face/incid/tube.json", [0.0; 3]);
    check_stream(&body, &pts, &cs, [0.0; 3]);
}

#[test]
fn tube_seam_vertex_and_seam_edge() {
    let body = load_body("geo/dist/face/incid/tube.json", [0.0; 3]);
    let incid = Incid::new(&body).unwrap();
    let (ri, h) = (1.0 / 128.0, 1.0 / 128.0);
    // T-V4 (inner top, on the seam): the inner cylinder has two corners there (pi / 2 each)
    // and the annulus one straight corner of its closed hole circle (pi).
    let hit = FaceHit {
        foot: [ri, 0.0, h],
        distance: 0.0,
        face: tid("T-F2"),
        uv: [0.0, h],
        feature: FaceFeature::Vertex(tid("T-V4")),
    };
    let inc = incid.incident_at(&hit).unwrap();
    assert_eq!(inc.len(), 2, "{inc:?}");
    assert!((inc[0].angle - PI).abs() < 1e-12 && (inc[1].angle - PI).abs() < 1e-12, "{inc:?}");
    assert!((inc[0].normal[0] + 1.0).abs() < 1e-15, "inner cylinder points to the axis: {inc:?}");
    assert!((inc[1].normal[2] - 1.0).abs() < 1e-15, "{inc:?}");
    // Above the rim, inside the hole: outside the material.
    let p = [ri - 1e-4, 0.0, h + 1e-4];
    assert_eq!(incid.signed(p, &hit).unwrap().status, SignStatus::Outside);
    // Seam edge T-E6: two coedges on one face (a smooth edge, not a boundary).
    let hit = FaceHit {
        foot: [ri, 0.0, h * 0.5],
        distance: 0.0,
        face: tid("T-F2"),
        uv: [0.0, h * 0.5],
        feature: FaceFeature::Edge(tid("T-E6")),
    };
    let inc = incid.incident_at(&hit).unwrap();
    assert_eq!(inc.len(), 2);
    for i in &inc {
        assert!((i.normal[0] + 1.0).abs() < 1e-15 && i.angle == PI, "{i:?}");
    }
    assert_eq!(incid.signed([ri - 1e-3, 0.0, h * 0.5], &hit).unwrap().status, SignStatus::Outside);
    assert_eq!(incid.signed([ri + 1e-3, 0.0, h * 0.5], &hit).unwrap().status, SignStatus::Inside);
}

// ------------------------------------------------------------------ end to end through FACE.ONE

/// Brute force over the faces with GEO.DIST.FACE.ONE (smallest distance; within the tie
/// window interior before edge before vertex, then the lowest face ID), then signed.
fn through_face_one(rel: &str, pts: &[V3], cs: &[Case], every: usize) {
    let body = load_body(rel, [0.0; 3]);
    let incid = Incid::new(&body).unwrap();
    let mut faces: Vec<(TopologyId, FaceProjector<'_>)> = Vec::new();
    for f in body.faces() {
        let fx = face_ix(&body, f.id());
        faces.push((f.id().clone(), FaceProjector::new(&body, incid.pcurves(), fx).unwrap()));
    }
    faces.sort_by(|a, b| a.0.cmp(&b.0));
    let rank = |h: &FaceHit| match h.feature {
        FaceFeature::Interior => 0,
        FaceFeature::Edge(_) => 1,
        FaceFeature::Vertex(_) => 2,
    };
    let mut n = 0;
    for (i, (p, c)) in pts.iter().zip(cs).enumerate().step_by(every) {
        let mut best: Option<FaceHit> = None;
        for (_, fp) in &faces {
            let FaceProjection::Hit(h) = fp.project(*p, FaceQueryOptions::default()).unwrap() else { continue };
            let better = match &best {
                None => true,
                Some(b) => {
                    h.distance < b.distance - FACE_TIE_WINDOW_M
                        || ((h.distance - b.distance).abs() <= FACE_TIE_WINDOW_M && rank(&h) < rank(b))
                }
            };
            if better {
                best = Some(h);
            }
        }
        let hit = best.unwrap();
        assert!((hit.distance - c.distance).abs() <= 1e-9, "case {i}: {hit:?} vs {c:?}");
        let s = incid.signed(*p, &hit).unwrap();
        assert_eq!(s.distance > 0.0, c.outside, "case {i} {c:?}: {hit:?} {s:?}");
        n += 1;
    }
    assert!(n > 100);
}

#[test]
fn cube_through_face_one() {
    let pts: Vec<V3> = cube_points().into_iter().map(metres).collect();
    let (_, cs) = cases("cube");
    through_face_one("geo/cad/topo/cube_labelled.json", &pts, &cs, 50);
}

#[test]
fn tube_through_face_one() {
    let pts: Vec<V3> = tube_points().into_iter().map(metres).collect();
    let (_, cs) = cases("tube");
    through_face_one("geo/dist/face/incid/tube.json", &pts, &cs, 10);
}

// ------------------------------------------------------------------ AC-3: free edges

#[test]
fn ac3_open_sheet_free_edge_is_undefined() {
    let body = load_body("geo/dist/face/open_sheet.json", [0.0; 3]);
    let incid = Incid::new(&body).unwrap();
    let edge = FaceHit {
        foot: [0.5, 0.0, 0.0],
        distance: 0.25,
        face: tid("S-F1"),
        uv: [0.5, 0.0],
        feature: FaceFeature::Edge(tid("S-E1")),
    };
    for p in [[0.5, -0.25, 0.0], [0.5, -0.1, 0.2], [0.5, -0.1, -0.2]] {
        let s = incid.signed(p, &edge).unwrap();
        assert!(!s.sign_defined(), "{s:?}");
        assert_eq!(s.status, SignStatus::Undefined(SignUndefined::Boundary));
        assert!(s.distance > 0.0 && s.distance == s.unsigned);
    }
    match incid.incident_at(&edge) {
        Err(Undefined::Sign { reason: SignUndefined::Boundary, entity }) => assert_eq!(entity, tid("S-E1")),
        other => panic!("{other:?}"),
    }
    // A corner of the sheet touches free edges: undefined too.
    let corner = FaceHit {
        foot: [1.0, 1.0, 0.0],
        distance: 0.1,
        face: tid("S-F1"),
        uv: [1.0, 1.0],
        feature: FaceFeature::Vertex(tid("S-V3")),
    };
    assert_eq!(incid.signed([1.1, 1.1, 0.1], &corner).unwrap().status, SignStatus::Undefined(SignUndefined::Boundary));
    // The interior of the sheet has a normal (+z): signs are defined there.
    let inner = FaceHit {
        foot: [0.5, 0.5, 0.0],
        distance: 0.2,
        face: tid("S-F1"),
        uv: [0.5, 0.5],
        feature: FaceFeature::Interior,
    };
    assert_eq!(incid.signed([0.5, 0.5, 0.2], &inner).unwrap().status, SignStatus::Outside);
    assert_eq!(incid.signed([0.5, 0.5, -0.2], &inner).unwrap().status, SignStatus::Inside);
}

// ------------------------------------------------------------------ AC-4: sphere apex

/// The exact unit-sphere octant as a rational biquadratic NURBS face (no loops): `u` from the
/// x axis to the y axis, `v` from the equator to the pole, whose control row `v = 1`
/// collapses to the pole `(0, 0, 1)`, so `Su = 0` exactly there.
fn octant(sense: Sense) -> Body {
    let w = core::f64::consts::FRAC_1_SQRT_2;
    let uc = [[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let wu = [1.0, w, 1.0];
    let wv = [1.0, w, 1.0];
    let mut ctrl = Vec::new();
    let mut weights = Vec::new();
    for (c, a) in uc.iter().zip(wu) {
        for (j, b) in wv.iter().enumerate() {
            let p = match j {
                0 => [c[0], c[1], 0.0],
                1 => [c[0], c[1], 1.0],
                _ => [0.0, 0.0, 1.0],
            };
            ctrl.push(p);
            weights.push(a * b);
        }
    }
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let s = NurbsSurface::new(NurbsSurfaceParts {
        degree_u: 2,
        degree_v: 2,
        knots_u: knots.clone(),
        knots_v: knots,
        n_u: 3,
        n_v: 3,
        ctrl,
        weights: Some(weights),
        periodic_u: false,
        periodic_v: false,
    })
    .unwrap();
    let mut b = BodyBuilder::new([7; 16]);
    let f = b.add_face(SurfaceDef::nurbs(s).unwrap(), sense, vec![], Some("P-F1"));
    b.add_shell(vec![f], false);
    b.build().unwrap()
}

#[test]
fn ac4_sphere_apex_uses_fallback_normal() {
    for (sense, flip) in [(Sense::Same, 1.0), (Sense::Reversed, -1.0)] {
        let body = octant(sense);
        let incid = Incid::new(&body).unwrap();
        let hit = FaceHit {
            foot: [0.0, 0.0, 1.0],
            distance: 0.0,
            face: tid("P-F1"),
            uv: [0.5, 1.0],
            feature: FaceFeature::Interior,
        };
        let inc = incid.incident_at(&hit).unwrap();
        assert_eq!(inc.len(), 1);
        assert!(inc[0].fallback, "the pole normal is degenerate: {inc:?}");
        assert!(inc[0].normal[2] * flip > 1.0 - 1e-9, "{inc:?}");
        assert_eq!(inc[0].angle, TAU);
        // Hand-labelled: farther from the centre than the radius is outside the sphere.
        for (p, out) in [([0.0, 0.0, 1.25], true), ([0.0, 0.0, 0.75], false), ([0.0, 0.0, 1.0 + 1e-6], true)] {
            let s = incid.signed(p, &hit).unwrap();
            let want = if out == (flip > 0.0) { SignStatus::Outside } else { SignStatus::Inside };
            assert_eq!(s.status, want, "{p:?} {sense:?}");
        }
        // Next to the pole the normal is regular: no fallback.
        let v = 1.0 - 1e-3;
        let hit2 = FaceHit { uv: [0.5, v], ..hit.clone() };
        let foot = {
            use fid_geo_nurbs::Surface;
            body.faces()[0].surface().as_nurbs().unwrap().eval(0.5, v, 0).unwrap().p
        };
        let hit2 = FaceHit { foot, ..hit2 };
        let inc = incid.incident_at(&hit2).unwrap();
        assert!(!inc[0].fallback);
    }
}

// ------------------------------------------------------------------ fail closed

#[test]
fn bad_hits_are_refused() {
    let body = load_body("geo/cad/topo/cube_labelled.json", [0.0; 3]);
    let incid = Incid::new(&body).unwrap();
    let good = FaceHit {
        foot: [1.0, 0.5, 0.5],
        distance: 0.0,
        face: tid("#52"),
        uv: [0.5, 0.5],
        feature: FaceFeature::Interior,
    };
    assert!(incid.incident_at(&good).is_ok());
    // NaN foot.
    let h = FaceHit { foot: [f64::NAN, 0.5, 0.5], ..good.clone() };
    assert_eq!(incid.incident_at(&h), Err(Undefined::Error(IncidError::NonFiniteHit)));
    // Unknown face, and an edge that does not bound the hit face (#9 is not on #52).
    let h = FaceHit { face: tid("#99"), ..good.clone() };
    assert!(matches!(incid.incident_at(&h), Err(Undefined::Error(IncidError::UnknownFace { .. }))));
    let h = FaceHit { feature: FaceFeature::Edge(tid("#9")), ..good.clone() };
    assert!(matches!(incid.incident_at(&h), Err(Undefined::Error(IncidError::NotOnFace { .. }))));
    // A foot far from the face's surface: no parameters reproduce it, no normal is guessed.
    let h = FaceHit { foot: [5.0, 0.5, 0.5], uv: [0.5, 0.5], ..good.clone() };
    assert!(matches!(incid.incident_at(&h), Err(Undefined::Error(IncidError::Mapping { .. }))));
    // A wrong `uv` with a correct foot is repaired by inversion (same normal).
    let h = FaceHit { uv: [9.0, 9.0], ..good.clone() };
    assert_eq!(incid.incident_at(&h).unwrap()[0].normal, [1.0, 0.0, 0.0]);
    // Results are deterministic.
    let a = incid.signed([1.5, 0.5, 0.5], &good).unwrap();
    let b = incid.signed([1.5, 0.5, 0.5], &good).unwrap();
    assert_eq!(a.distance.to_bits(), b.distance.to_bits());
    assert_eq!(a.bound.to_bits(), b.bound.to_bits());
}

/// `cube_labelled.json` read literally: its lines run along `ref_dir`, perpendicular to their
/// edges, so no edge curve meets its far vertex and the derived pcurves fail. A corner foot is
/// then refused (no tangent reproduces the vertex) instead of being given a guessed angle,
/// while an interior foot still has its face normal.
#[test]
fn inconsistent_edge_curves_fail_closed() {
    RAW_LINES.with(|r| r.set(true));
    let body = load_body("geo/cad/topo/cube_labelled.json", [0.0; 3]);
    RAW_LINES.with(|r| r.set(false));
    let incid = Incid::new(&body).unwrap();
    let corner = FaceHit {
        foot: [1.0, 1.0, 1.0],
        distance: 0.0,
        face: tid("#52"),
        uv: [1.0, 1.0],
        feature: FaceFeature::Vertex(tid("#8")),
    };
    assert!(matches!(incid.incident_at(&corner), Err(Undefined::Error(IncidError::Mapping { .. }))));
    let inner = FaceHit {
        foot: [1.0, 0.5, 0.5],
        distance: 0.0,
        face: tid("#52"),
        uv: [0.5, 0.5],
        feature: FaceFeature::Interior,
    };
    assert_eq!(incid.signed([1.5, 0.5, 0.5], &inner).unwrap().status, SignStatus::Outside);
}

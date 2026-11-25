//! GEO.DIST.FACE.BODY: face-set, body and instance projection targets.
//!
//! Expected values come from independent oracles or are hand-labelled from geometry; nothing
//! here computes an expected value with this crate:
//!
//! - AC-1: the GEO.DIST.FACE.REF fixtures `fixtures/geo/dist/face/ref_v1` (oracle
//!   `tools/fixtures/face_proj_oracle.py`, 60 digits with exact trim tests);
//! - AC-5 and the concave L-block: `fixtures/geo/dist/face/body/cases.json`, written by the
//!   oracle `make_body_cases.py` next to it (rational and 60-digit decimal arithmetic);
//! - AC-3: hand-labelled (a point inside a unit cube at `(1/4, 1/4, 1/2)` is 1/4 from the
//!   faces `x = 0` and `y = 0`);
//! - AC-2, AC-4 and AC-6 compare the target with itself in another configuration (brute
//!   force, the definition frame, one thread), which is what those criteria state.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::needless_range_loop)]

mod common;

use core::sync::atomic::AtomicBool;

use common::{load, manifest_files, matches, parse_hex_f64, parse_json, Feature, Hit, Json};
use fid_geo_cad::{
    Body, BodyBuilder, Curve2, CurveDef, CurveParams, EntityIx, FaceIx, Frame, Instance, InstancePath, InstanceView,
    Label, Sense, SurfaceDef, SurfaceParams, TopologyId,
};
use fid_geo_dist::{
    encode, project_batch, BatchConfig, BatchError, BodyError, BodyTarget, FaceError, FaceFeature, FaceHit,
    FaceSetTarget, FaceTarget, HitSign, InstanceTarget, ProjectError, Projection, ProjectionOptions, ProjectionTarget,
    SignStatus, SignUndefined, TriangleSubset, FACE_REF_FOOT_TOL_M,
};
use fid_geo_frame::{AxisAngle, FrameId, FrameKind, FrameTree, Rigid, Similarity, Transform};
use fid_geo_nurbs::{NurbsCurve, NurbsCurveParts, NurbsSurface, NurbsSurfaceParts};
use fid_geo_types::{FrameRef, PointSet};

type V3 = [f64; 3];

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/");

fn frame_ref() -> FrameRef {
    FrameRef::from_bytes([3; 16])
}

fn opts() -> ProjectionOptions<'static, 'static> {
    ProjectionOptions::default()
}

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

fn hex3(j: &Json) -> V3 {
    let a = j.arr();
    [parse_hex_f64(a[0].str()), parse_hex_f64(a[1].str()), parse_hex_f64(a[2].str())]
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len(a: V3) -> f64 {
    ((a[0] * a[0] + a[1] * a[1]) + a[2] * a[2]).sqrt()
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

fn face_ix(body: &Body, id: &str) -> FaceIx {
    match body.lookup(&tid(id)) {
        Some(EntityIx::Face(f)) => f,
        other => panic!("{id}: {other:?}"),
    }
}

fn face_label(body: &Body, entity: u64) -> String {
    label_of(body.faces()[entity as usize].id())
}

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

    /// Uniform in `[lo, hi)` on a 2^-24 grid of the unit interval.
    fn in_range(&mut self, lo: f64, hi: f64) -> f64 {
        let u = (self.next() >> 40) as f64 / 16_777_216.0;
        lo + u * (hi - lo)
    }
}

// ------------------------------------------------------------------ topology-JSON bodies

fn tframe(j: &Json, off: V3) -> Frame {
    Frame { origin: add(v3(j.get("origin")), off), axis: v3(j.get("axis")), ref_dir: v3(j.get("ref_dir")) }
}

/// A line runs along GEO.NURBS.ANALYTIC's reference direction: the INCID fixtures give it as
/// `direction` (with a perpendicular `normal`), `cube_labelled.json` in `axis` (its
/// `ref_dir` is a perpendicular), as GEO.DIST.FACE.INCID's tests read them.
fn line_frame(j: &Json, off: V3) -> Frame {
    let origin = add(v3(j.get("origin")), off);
    if j.get("direction").is_null() {
        Frame { origin, axis: v3(j.get("ref_dir")), ref_dir: v3(j.get("axis")) }
    } else {
        Frame { origin, axis: v3(j.get("normal")), ref_dir: v3(j.get("direction")) }
    }
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

fn tcurve(j: &Json, off: V3) -> Option<CurveDef> {
    if j.is_null() {
        return None;
    }
    Some(match j.get("type").str() {
        "line" => CurveDef::analytic(CurveParams::Line { frame: line_frame(j, off) }).unwrap(),
        "circle" => {
            CurveDef::analytic(CurveParams::Circle { frame: tframe(j, off), radius: j.get("radius").num() }).unwrap()
        }
        t => panic!("curve type {t}"),
    })
}

fn tpcurve(j: &Json) -> Option<Curve2> {
    if j.is_null() {
        return None;
    }
    Some(match j.get("type").str() {
        "line2" => Curve2::line(v2(j.get("origin")), v2(j.get("dir"))).unwrap(),
        "nurbs" => Curve2::nurbs(nurbs_curve(j)).unwrap(),
        t => panic!("pcurve type {t}"),
    })
}

fn tsurface(j: &Json, off: V3) -> SurfaceDef {
    let fr = tframe(j, off);
    let p = match j.get("type").str() {
        "plane" => SurfaceParams::Plane { frame: fr },
        "cylinder" => SurfaceParams::Cylinder { frame: fr, radius: j.get("radius").num() },
        "sphere" => SurfaceParams::Sphere { frame: fr, radius: j.get("radius").num() },
        "cone" => {
            SurfaceParams::Cone { frame: fr, radius: j.get("radius").num(), half_angle: j.get("half_angle").num() }
        }
        t => panic!("surface type {t}"),
    };
    SurfaceDef::analytic(p).unwrap()
}

fn tsense(j: &Json) -> Sense {
    match j.str() {
        "same" => Sense::Same,
        "reversed" => Sense::Reversed,
        s => panic!("sense {s}"),
    }
}

fn opt_label(j: &Json) -> Option<&str> {
    let l = j.get("label");
    if l.is_null() {
        None
    } else {
        Some(l.str())
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
        b.add_vertex(add(v3(v.get("point")), off), opt_label(v));
    }
    for e in j.get("edges").arr() {
        let r = v2(e.get("range"));
        b.add_edge(
            tcurve(e.get("curve"), off),
            e.get("start").num() as usize,
            e.get("end").num() as usize,
            r,
            opt_label(e),
        );
    }
    for c in j.get("coedges").arr() {
        b.add_coedge(c.get("edge").num() as usize, tsense(c.get("sense")), tpcurve(c.get("pcurve")), opt_label(c));
    }
    for l in j.get("loops").arr() {
        let cs = l.get("coedges").arr().iter().map(|x| x.num() as usize).collect();
        b.add_loop(cs, l.get("outer").truthy(), opt_label(l));
    }
    for f in j.get("faces").arr() {
        let ls = f.get("loops").arr().iter().map(|x| x.num() as usize).collect();
        b.add_face(tsurface(f.get("surface"), off), tsense(f.get("sense")), ls, opt_label(f));
    }
    for s in j.get("shells").arr() {
        b.add_shell(s.get("faces").arr().iter().map(|x| x.num() as usize).collect(), s.get("closed").truthy());
    }
    b.build().unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// Several topology-JSON bodies as separate shells of one body: part `k` is translated by
/// its offset and every label gets the suffix `@k`.
fn load_multi(parts: &[(&str, V3)]) -> Body {
    let mut b = BodyBuilder::new([0x5a; 16]);
    for (k, &(rel, off)) in parts.iter().enumerate() {
        let j = parse_json(&read(rel));
        let tag = |l: Option<&str>| l.map(|x| format!("{x}@{k}"));
        let (mut vs, mut es, mut cs, mut ls, mut fs) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for v in j.get("vertices").arr() {
            vs.push(b.add_vertex(add(v3(v.get("point")), off), tag(opt_label(v)).as_deref()));
        }
        for e in j.get("edges").arr() {
            let (s0, s1) = (vs[e.get("start").num() as usize], vs[e.get("end").num() as usize]);
            es.push(b.add_edge(tcurve(e.get("curve"), off), s0, s1, v2(e.get("range")), tag(opt_label(e)).as_deref()));
        }
        for c in j.get("coedges").arr() {
            let e = es[c.get("edge").num() as usize];
            cs.push(b.add_coedge(e, tsense(c.get("sense")), tpcurve(c.get("pcurve")), tag(opt_label(c)).as_deref()));
        }
        for l in j.get("loops").arr() {
            let list = l.get("coedges").arr().iter().map(|x| cs[x.num() as usize]).collect();
            ls.push(b.add_loop(list, l.get("outer").truthy(), tag(opt_label(l)).as_deref()));
        }
        for f in j.get("faces").arr() {
            let list = f.get("loops").arr().iter().map(|x| ls[x.num() as usize]).collect();
            fs.push(b.add_face(
                tsurface(f.get("surface"), off),
                tsense(f.get("sense")),
                list,
                tag(opt_label(f)).as_deref(),
            ));
        }
        for sh in j.get("shells").arr() {
            b.add_shell(
                sh.get("faces").arr().iter().map(|x| fs[x.num() as usize]).collect(),
                sh.get("closed").truthy(),
            );
        }
    }
    b.build().unwrap_or_else(|e| panic!("multi body: {e}"))
}

// ------------------------------------------------------------------ ref_v1 bodies, one shell per face

/// Builds a GEO.DIST.FACE.REF body description (`fid-brep-builder/0`) with every face in its
/// own open shell and its own copies of its edges and vertices, labelled `<face>/<label>`
/// (face labels unchanged). The fixtures give both coedges of some shared closed edges the
/// same sense, which GEO.CAD.TOPO.MODEL's pairing check refuses for a shared edge (recorded
/// by GEO.DIST.FACE.ONE); the geometry is unchanged. Pcurves as in `common::build_face_body`.
fn build_split_body(j: &Json) -> Body {
    let mut id = [0u8; 16];
    for (k, b) in j.get("body_id").str().bytes().take(16).enumerate() {
        id[k] = b;
    }
    let mut bb = BodyBuilder::new(id);
    let frame =
        |pl: &Json| Frame { origin: v3(pl.get("origin")), axis: v3(pl.get("axis")), ref_dir: v3(pl.get("ref_dir")) };
    let edge_json = |label: &str| -> &Json {
        j.get("edges").arr().iter().find(|x| x.get("label").str() == label).unwrap_or_else(|| panic!("{label}"))
    };
    for face in j.get("faces").arr() {
        let fl = face.get("label").str();
        let q = |s: &str| format!("{fl}/{s}");
        let mut edge_labels: Vec<String> = Vec::new();
        for l in face.get("loops").arr() {
            for c in l.get("coedges").arr() {
                let e = c.get("edge").str().to_string();
                if !edge_labels.contains(&e) {
                    edge_labels.push(e);
                }
            }
        }
        let mut vertex_labels: Vec<String> = Vec::new();
        for e in &edge_labels {
            for end in ["start", "end"] {
                let v = edge_json(e).get(end).str().to_string();
                if !vertex_labels.contains(&v) {
                    vertex_labels.push(v);
                }
            }
        }
        let mut vix = Vec::new();
        for vl in &vertex_labels {
            let v = j.get("vertices").arr().iter().find(|x| x.get("label").str() == vl).unwrap();
            vix.push(bb.add_vertex(v3(v.get("point")), Some(&q(vl))));
        }
        let vpos = |l: &str| vix[vertex_labels.iter().position(|x| x == l).unwrap()];
        let mut eix = Vec::new();
        let mut ranges = Vec::new();
        for el in &edge_labels {
            let e = edge_json(el);
            let c = e.get("curve");
            let curve = if c.is_null() {
                None
            } else if c.get("type").str() == "nurbs" {
                let nums = |x: &Json| x.arr().iter().map(Json::num).collect::<Vec<f64>>();
                let weights = if c.get("weights").is_null() { None } else { Some(nums(c.get("weights"))) };
                let n = NurbsCurve::new(NurbsCurveParts {
                    degree: c.get("degree").num() as usize,
                    knots: nums(c.get("knots")),
                    ctrl: c.get("ctrl").arr().iter().map(v3).collect(),
                    weights,
                    periodic: false,
                })
                .unwrap();
                Some(CurveDef::nurbs(n).unwrap())
            } else {
                let params = match c.get("type").str() {
                    "line" => {
                        let d = v3(c.get("direction"));
                        let helper = if d[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
                        let a = [
                            d[1] * helper[2] - d[2] * helper[1],
                            d[2] * helper[0] - d[0] * helper[2],
                            d[0] * helper[1] - d[1] * helper[0],
                        ];
                        let n = len(a);
                        CurveParams::Line {
                            frame: Frame {
                                origin: v3(c.get("origin")),
                                axis: [a[0] / n, a[1] / n, a[2] / n],
                                ref_dir: d,
                            },
                        }
                    }
                    "circle" => CurveParams::Circle { frame: frame(c.get("placement")), radius: c.get("radius").num() },
                    "ellipse" => CurveParams::Ellipse {
                        frame: frame(c.get("placement")),
                        semi_axis_1: c.get("semi_axis_1").num(),
                        semi_axis_2: c.get("semi_axis_2").num(),
                    },
                    t => panic!("edge curve type {t}"),
                };
                Some(CurveDef::analytic(params).unwrap())
            };
            let r = e.get("param_range");
            let range = if r.is_null() { [0.0, 1.0] } else { [r.arr()[0].num(), r.arr()[1].num()] };
            ranges.push(range);
            eix.push(bb.add_edge(curve, vpos(e.get("start").str()), vpos(e.get("end").str()), range, Some(&q(el))));
        }
        let sense = |s: &Json| if s.str() == "reversed" { Sense::Reversed } else { Sense::Same };
        let mut loops = Vec::new();
        for l in face.get("loops").arr() {
            let mut cos = Vec::new();
            for c in l.get("coedges").arr() {
                let k = edge_labels.iter().position(|x| x == c.get("edge").str()).unwrap();
                let pc = c.get("pcurve");
                let pcurve = if pc.is_null() {
                    None
                } else {
                    match pc.get("type").str() {
                        "line2" => {
                            let (a, b) = (pc.get("from").arr(), pc.get("to").arr());
                            let (a, b) = ([a[0].num(), a[1].num()], [b[0].num(), b[1].num()]);
                            let [t0, t1] = ranges[k];
                            let d = [(b[0] - a[0]) / (t1 - t0), (b[1] - a[1]) / (t1 - t0)];
                            let o = [a[0] - t0 * d[0], a[1] - t0 * d[1]];
                            Some(Curve2::line(o, d).unwrap())
                        }
                        "circle2" => None,
                        t => panic!("pcurve type {t}"),
                    }
                };
                cos.push(bb.add_coedge(eix[k], sense(c.get("sense")), pcurve, Some(&q(c.get("label").str()))));
            }
            loops.push(bb.add_loop(cos, l.get("outer").truthy(), Some(&q(l.get("label").str()))));
        }
        let s = face.get("surface");
        let surface = match s.get("type").str() {
            "nurbs" => {
                let nums = |x: &Json| x.arr().iter().map(Json::num).collect::<Vec<f64>>();
                let weights = if s.get("weights").is_null() { None } else { Some(nums(s.get("weights"))) };
                SurfaceDef::nurbs(
                    NurbsSurface::new(NurbsSurfaceParts {
                        degree_u: s.get("degree_u").num() as usize,
                        degree_v: s.get("degree_v").num() as usize,
                        knots_u: nums(s.get("knots_u")),
                        knots_v: nums(s.get("knots_v")),
                        n_u: s.get("n_u").num() as usize,
                        n_v: s.get("n_v").num() as usize,
                        ctrl: s.get("ctrl").arr().iter().map(v3).collect(),
                        weights,
                        periodic_u: false,
                        periodic_v: false,
                    })
                    .unwrap(),
                )
                .unwrap()
            }
            t => {
                let fr = frame(s.get("placement"));
                let params = match t {
                    "plane" => SurfaceParams::Plane { frame: fr },
                    "cylinder" => SurfaceParams::Cylinder { frame: fr, radius: s.get("radius").num() },
                    "cone" => SurfaceParams::Cone {
                        frame: fr,
                        radius: s.get("radius").num(),
                        half_angle: s.get("half_angle").num(),
                    },
                    "sphere" => SurfaceParams::Sphere { frame: fr, radius: s.get("radius").num() },
                    "torus" => SurfaceParams::Torus {
                        frame: fr,
                        major_radius: s.get("major_radius").num(),
                        minor_radius: s.get("minor_radius").num(),
                    },
                    t => panic!("surface type {t}"),
                };
                SurfaceDef::analytic(params).unwrap()
            }
        };
        let f = bb.add_face(surface, sense(face.get("sense")), loops, Some(fl));
        bb.add_shell(vec![f], false);
    }
    bb.build().unwrap_or_else(|e| panic!("body {}: {e}", j.get("body_id").str()))
}

/// `F2/E1` -> `E1`.
fn strip(id: &TopologyId) -> String {
    let l = label_of(id);
    l.split_once('/').map_or(l.clone(), |(_, r)| r.to_string())
}

fn to_hit(h: &FaceHit) -> Hit {
    let feature = match &h.feature {
        FaceFeature::Interior => Feature::Interior,
        FaceFeature::Edge(e) => Feature::Edge(strip(e)),
        FaceFeature::Vertex(v) => Feature::Vertex(strip(v)),
    };
    Hit { face: label_of(&h.face), feature, foot: h.foot, distance: h.distance }
}

/// The torus band face F1 is refused by GEO.CAD.TOPO.TRIM (`DoublyPeriodicSeam`); see NODE.md.
const TRIM_GAP: (&str, &str) = ("torus_segment.json", "F1");

// ------------------------------------------------------------------ AC-1

/// AC-1: the body answer (closest point over every face, lowest face ID within 1e-15 m) of
/// every `ref_v1` query agrees with the oracle: face, foot and distance within 1e-9 m, the
/// feature by the fixture rule. All 160 queries of the torus band fail closed on its face F1
/// (the open trim gap); none of them is answered.
#[test]
fn ac1_ref_v1_body_queries() {
    let mut compared = 0;
    let mut gap = 0;
    let mut bad = Vec::new();
    for file in manifest_files() {
        let fx = load(&file);
        let body = build_split_body(&fx.body);
        let target = BodyTarget::new(&body, frame_ref()).unwrap();
        for q in &fx.queries {
            match target.set().closest(q.p, &opts()) {
                Ok(Some((_, h))) => {
                    compared += 1;
                    let hit = to_hit(&h);
                    if !matches(&fx, q, &hit) {
                        bad.push(format!(
                            "{} {} [{}]: got {} {:?} d={:e}; want {} {:?} d={:e}",
                            fx.file,
                            q.id,
                            q.gen,
                            hit.face,
                            hit.feature,
                            hit.distance,
                            q.answer.face,
                            q.answer.feature,
                            q.answer.distance
                        ));
                    }
                }
                Ok(None) => bad.push(format!("{} {}: no hit", fx.file, q.id)),
                Err(BodyError::Face { face, error }) if (fx.file.as_str(), label_of(&face).as_str()) == TRIM_GAP => {
                    match *error {
                        FaceError::Trim(fid_geo_cad::TrimError::Undecidable(u)) => {
                            assert_eq!(u.code(), "DoublyPeriodicSeam")
                        }
                        other => panic!("{} {}: {other:?}", fx.file, q.id),
                    }
                    gap += 1;
                }
                Err(e) => bad.push(format!("{} {}: error {e}", fx.file, q.id)),
            }
        }
    }
    for b in &bad {
        eprintln!("{b}");
    }
    assert!(bad.is_empty(), "{} of {compared} queries disagree with the oracle", bad.len());
    assert!(compared >= 1300, "only {compared} queries compared");
    assert_eq!(gap, 160, "the torus trim gap has changed: update AC-1 in NODE.md");
    eprintln!("AC-1: {compared} queries agree, {gap} fail closed on the torus trim gap");
}

// ------------------------------------------------------------------ AC-2

fn bodies_for_ac2() -> Vec<(String, Body)> {
    let mut out = vec![
        ("cube_labelled".to_string(), load_body("geo/cad/topo/cube_labelled.json", [0.0; 3])),
        ("tube".to_string(), load_body("geo/dist/face/incid/tube.json", [0.0; 3])),
        ("lblock".to_string(), load_body("geo/dist/face/incid/lblock.json", [0.0; 3])),
        ("cone_c20".to_string(), load_body("geo/dist/face/incid/cone_c20.json", [0.0; 3])),
        ("cone_c30".to_string(), load_body("geo/dist/face/incid/cone_c30.json", [0.0; 3])),
    ];
    for f in ["cube.json", "sphere_cap.json", "cone_frustum.json", "tube.json", "nurbs_rational_pair.json"] {
        out.push((format!("ref_v1/{f}"), build_split_body(&load(f).body)));
    }
    out
}

/// The vertex box of a body, widened by half its largest extent.
fn query_box(body: &Body) -> (V3, V3) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for v in body.vertices() {
        let p = v.point();
        for k in 0..3 {
            if p[k] < lo[k] {
                lo[k] = p[k];
            }
            if p[k] > hi[k] {
                hi[k] = p[k];
            }
        }
    }
    let mut e = 0.0f64;
    for k in 0..3 {
        if hi[k] - lo[k] > e {
            e = hi[k] - lo[k];
        }
    }
    ([lo[0] - e / 2.0, lo[1] - e / 2.0, lo[2] - e / 2.0], [hi[0] + e / 2.0, hi[1] + e / 2.0, hi[2] + e / 2.0])
}

fn record(r: &Result<Projection, ProjectError>) -> Vec<u8> {
    match r {
        Ok(p) => encode(&[*p]).unwrap(),
        Err(e) => format!("error {e:?}").into_bytes(),
    }
}

/// AC-2: 10 000 queries from SplitMix64 seed 23 over the invented analytic bodies (the
/// GEO.CAD.TESS `analytic_set` bodies do not exist yet; see NODE.md) and four `ref_v1`
/// bodies, with and without a maximum distance: the pruned search returns exactly the bytes
/// of the brute-force loop over every face. Every foot lies in its face's box (REQ-004).
#[test]
fn ac2_bit_identical_to_brute_force_seed_23() {
    let bodies = bodies_for_ac2();
    let targets: Vec<BodyTarget<'_>> = bodies.iter().map(|(_, b)| BodyTarget::new(b, frame_ref()).unwrap()).collect();
    let boxes: Vec<(V3, V3)> = bodies.iter().map(|(_, b)| query_box(b)).collect();
    let mut g = SplitMix(23);
    let mut hits = 0;
    let mut failed = 0;
    for i in 0..10_000usize {
        let k = i % targets.len();
        let (lo, hi) = boxes[k];
        let p = [g.in_range(lo[0], hi[0]), g.in_range(lo[1], hi[1]), g.in_range(lo[2], hi[2])];
        let max = if i % 5 == 4 { Some((hi[0] - lo[0]) * 0.125) } else { None };
        let o = ProjectionOptions { max_distance: max, ..opts() };
        let t = &targets[k];
        let fast = t.project(p, &o);
        let slow = t.set().project_brute(p, &o);
        if fast.is_err() {
            // Fail closed (a NURBS face search that cannot resolve a region, GEO.DIST.FACE.ONE
            // `NonConvergence`): the brute-force loop must fail on the same face.
            failed += 1;
        }
        assert_eq!(record(&fast), record(&slow), "{} query {i} at {p:?}", bodies[k].0);
        if let Ok(Projection::Hit(h)) = fast {
            hits += 1;
            let pos = t.set().faces().iter().position(|f| f.index() as u64 == h.entity_id).unwrap();
            let b = t.set().bounds().boxes()[pos];
            for a in 0..3 {
                assert!(
                    b.min()[a] <= h.foot[a] && h.foot[a] <= b.max()[a],
                    "{} query {i}: foot outside box",
                    bodies[k].0
                );
            }
        }
    }
    assert!(hits > 8000, "only {hits} hits");
    assert!(failed <= 20, "{failed} queries failed closed");
    eprintln!("AC-2: {hits} hits, {failed} failed closed in both paths");
}

/// The hierarchy with several leaves (34 faces, leaves of at most 8): three unit cubes and
/// two L-blocks as separate closed shells of one body, partly overlapping in every axis
/// range, so leaves are visited in lower-bound order and the search breaks out early. 4 000
/// queries (SplitMix64 seed 17), with and without a maximum distance: the pruned search gives
/// exactly the bytes of the brute-force loop, and the unsigned hit too.
#[test]
fn multi_leaf_pruned_equals_brute_force() {
    let cube = "geo/cad/topo/cube_labelled.json";
    let lblock = "geo/dist/face/incid/lblock.json";
    let body = load_multi(&[
        (cube, [0.0, 0.0, 0.0]),
        (cube, [1.5, 0.25, 0.0]),
        (cube, [0.5, 2.75, 0.5]),
        (lblock, [3.0, 3.0, 0.0]),
        (lblock, [-3.0, 0.5, -0.5]),
    ]);
    assert_eq!(body.faces().len(), 34);
    let t = BodyTarget::new(&body, frame_ref()).unwrap();
    assert!(t.set().bounds().whole().iter().all(|w| !w));
    let mut g = SplitMix(17);
    let mut faces_hit = std::collections::BTreeSet::new();
    for i in 0..4000usize {
        let p = [g.in_range(-4.0, 6.0), g.in_range(-1.0, 6.0), g.in_range(-1.5, 2.5)];
        let max = if i % 4 == 3 { Some(0.3) } else { None };
        let o = ProjectionOptions { max_distance: max, ..opts() };
        let fast = t.project(p, &o);
        assert!(fast.is_ok(), "query {i}: {fast:?}");
        assert_eq!(record(&fast), record(&t.set().project_brute(p, &o)), "query {i} at {p:?}");
        assert_eq!(t.set().closest(p, &o).unwrap(), t.set().closest_brute(p, &o).unwrap(), "query {i}");
        if let Ok(Projection::Hit(h)) = fast {
            faces_hit.insert(h.entity_id);
        }
    }
    assert!(faces_hit.len() >= 30, "only {} faces ever won", faces_hit.len());
}

/// GEO.DIST.FACE.ONE cone-nappe rule, directly on a `FaceProjector`: the INCID cone C30 face
/// (apex at the origin, nappe below it, the apex line not a trim boundary) projects a point
/// above the apex to the apex, never to the other nappe (which is nearer: before the rule
/// the foot was there, at distance cos(30 deg) of the true one).
#[test]
fn face_projector_cone_other_nappe_dropped() {
    let body = load_body("geo/dist/face/incid/cone_c30.json", [0.0; 3]);
    let pc = fid_geo_cad::derive_pcurves(&body, fid_geo_cad::DEFAULT_TOL_M).unwrap();
    let fp = fid_geo_dist::FaceProjector::new(&body, &pc, face_ix(&body, "C30-F1")).unwrap();
    for k in 1..=16 {
        let z = k as f64 / 1024.0;
        let fid_geo_dist::FaceProjection::Hit(h) = fp.project([0.0, 0.0, z], Default::default()).unwrap() else {
            panic!("{z}: no hit")
        };
        assert!(len(h.foot) <= 1e-9, "{z}: foot {:?} is not the apex", h.foot);
        assert!(h.foot[2] <= 1e-9, "{z}: foot on the other nappe");
        assert!((h.distance - z).abs() <= 1e-12, "{z}: {}", h.distance);
    }
}

/// The analytic faces of the invented bodies get certified (not whole-space) boxes, and a
/// cube face's box is its square padded by 1e-6 m.
#[test]
fn face_bounds_are_certified_and_tight() {
    for (name, body) in bodies_for_ac2() {
        let t = BodyTarget::new(&body, frame_ref()).unwrap();
        for (k, w) in t.set().bounds().whole().iter().enumerate() {
            assert!(!w, "{name}: face {k} got the whole-space box");
        }
    }
    let body = load_body("geo/cad/topo/cube_labelled.json", [0.0; 3]);
    let t = BodyTarget::new(&body, frame_ref()).unwrap();
    let pos = t.set().faces().iter().position(|&f| f == face_ix(&body, "#52")).unwrap();
    let b = t.set().bounds().boxes()[pos];
    let want_lo = [1.0, 0.0, 0.0];
    let want_hi = [1.0, 1.0, 1.0];
    for a in 0..3 {
        let (lo, hi) = (b.min()[a], b.max()[a]);
        assert!(lo < want_lo[a] && want_lo[a] - lo <= 2e-6, "axis {a}: {lo}");
        assert!(hi > want_hi[a] && hi - want_hi[a] <= 2e-6, "axis {a}: {hi}");
    }
}

// ------------------------------------------------------------------ AC-3

/// AC-3 (hand-labelled): a point on the diagonal plane of a cube edge is equally far from
/// both faces; the lower face ID wins, inside (interior feet) and outside (both faces give
/// the edge).
#[test]
fn ac3_cube_tie_lowest_face_id() {
    let body = load_body("geo/cad/topo/cube_labelled.json", [0.0; 3]);
    let t = BodyTarget::new(&body, frame_ref()).unwrap();
    // Faces: #51 x = 0, #52 x = 1, #53 y = 0, #54 y = 1, #55 z = 0, #56 z = 1.
    let cases: [(V3, &str, f64, SignStatus, Option<&str>); 6] = [
        ([0.25, 0.25, 0.5], "#51", 0.25, SignStatus::Inside, None),
        ([0.75, 0.25, 0.5], "#52", 0.25, SignStatus::Inside, None),
        ([0.75, 0.75, 0.5], "#52", 0.25, SignStatus::Inside, None),
        ([0.5, 0.25, 0.25], "#53", 0.25, SignStatus::Inside, None),
        ([-0.25, -0.25, 0.5], "#51", 0.25 * core::f64::consts::SQRT_2, SignStatus::Outside, Some("#17")),
        ([1.25, 1.25, 0.5], "#52", 0.25 * core::f64::consts::SQRT_2, SignStatus::Outside, Some("#20")),
    ];
    for (p, face, d, status, edge) in cases {
        let Projection::Hit(h) = t.project(p, &opts()).unwrap() else { panic!("{p:?}: no hit") };
        assert_eq!(face_label(&body, h.entity_id), face, "{p:?}");
        assert!((h.distance - d).abs() <= 1e-15, "{p:?}: {}", h.distance);
        assert_eq!(h.sign, HitSign::Cad(status), "{p:?}");
        let (_, fh) = t.set().closest(p, &opts()).unwrap().unwrap();
        match edge {
            Some(e) => assert_eq!(fh.feature, FaceFeature::Edge(tid(e)), "{p:?}"),
            None => assert_eq!(fh.feature, FaceFeature::Interior, "{p:?}"),
        }
    }
}

// ------------------------------------------------------------------ AC-4

fn instance_tree(rot: [[f64; 3]; 3], t: V3, scale: f64) -> (FrameTree, FrameId) {
    let mut tree = FrameTree::new();
    let id = FrameId::from_bytes([9; 16]);
    let to_parent = Transform::Similarity(Similarity::new(Rigid::new(rot, t).unwrap(), scale).unwrap());
    tree.insert(fid_geo_frame::Frame {
        id,
        name: "instance".to_string(),
        kind: FrameKind::Part,
        parent: Some(tree.world()),
        to_parent,
    })
    .unwrap();
    (tree, id)
}

/// AC-4: an instance placed with a rotation and scale 2 gives exactly the definition-frame
/// result mapped back, with distances doubled (closest point and along a direction); and,
/// hand-labelled, the world image of the cube centre is 2 x 0.5 = 1 m from the instance.
#[test]
fn ac4_instance_rotation_scale_2() {
    let body = load_body("geo/cad/topo/cube_labelled.json", [0.0; 3]);
    let rot = AxisAngle { axis: [1.0, 2.0, 3.0], angle: 0.75 }.to_matrix().unwrap();
    let (tree, frame) = instance_tree(rot, [0.5, -1.25, 2.0], 2.0);
    let inst = Instance::new(*body.id(), frame, InstancePath::new(vec![tid("I1")]).unwrap());
    let view = InstanceView::new(&inst, &body, &tree).unwrap();
    let it = InstanceTarget::new(&view, &tree, FrameId::WORLD).unwrap();
    assert_eq!(it.frame(), FrameRef::from_bytes([0; 16]));
    let def = BodyTarget::new(&body, FrameRef::from_bytes(frame.to_bytes())).unwrap();
    let to_world = tree.resolve(frame, FrameId::WORLD).unwrap();
    assert_eq!(to_world.scale(), 2.0);
    let to_def = to_world.inverse().unwrap();
    let mut g = SplitMix(4);
    let centre = to_world.apply_point(&[0.5, 0.5, 0.5]).unwrap();
    for i in 0..600 {
        let p = if i == 0 { centre } else { [g.in_range(-3.0, 4.0), g.in_range(-4.0, 3.0), g.in_range(-1.0, 6.0)] };
        let q = to_def.apply_point(&p).unwrap();
        let Projection::Hit(want) = def.project(q, &opts()).unwrap() else { panic!() };
        let Projection::Hit(got) = it.project(p, &opts()).unwrap() else { panic!() };
        assert_eq!(got.entity_id, want.entity_id, "{i}");
        assert_eq!(got.distance.to_bits(), (2.0 * want.distance).to_bits(), "{i}");
        assert_eq!(got.signed_distance.to_bits(), (2.0 * want.signed_distance).to_bits(), "{i}");
        assert_eq!(got.foot, to_world.apply_point(&want.foot).unwrap(), "{i}");
        assert_eq!(got.sign, want.sign, "{i}");
        if i == 0 {
            assert!((got.distance - 1.0).abs() <= 1e-12, "centre: {}", got.distance);
            assert_eq!(got.sign, HitSign::Cad(SignStatus::Inside));
        }
        // Along a direction.
        let v = [g.in_range(-1.0, 1.0), g.in_range(-1.0, 1.0), g.in_range(-1.0, 1.0)];
        let w = to_def.apply_dir(&v).unwrap();
        match (def.project_along(q, w, &opts()).unwrap(), it.project_along(p, v, &opts()).unwrap()) {
            (Projection::NoHit, Projection::NoHit) => {}
            (Projection::Hit(a), Projection::Hit(b)) => {
                assert_eq!(b.distance.to_bits(), (2.0 * a.distance).to_bits(), "{i}");
                assert_eq!(b.signed_distance.to_bits(), (2.0 * a.signed_distance).to_bits(), "{i}");
                assert_eq!(b.foot, to_world.apply_point(&a.foot).unwrap(), "{i}");
            }
            other => panic!("{i}: {other:?}"),
        }
    }
    // A maximum distance is in query units.
    let o = ProjectionOptions { max_distance: Some(0.999), ..opts() };
    assert_eq!(it.project(centre, &o).unwrap(), Projection::NoHit);
    let o = ProjectionOptions { max_distance: Some(1.001), ..opts() };
    assert!(matches!(it.project(centre, &o).unwrap(), Projection::Hit(_)));
}

// ------------------------------------------------------------------ AC-5

fn cases() -> Json {
    parse_json(&read("geo/dist/face/body/cases.json"))
}

fn check_tube_along(off: V3, brute: bool) -> (usize, usize) {
    let body = load_body("geo/dist/face/incid/tube.json", off);
    let t = BodyTarget::new(&body, frame_ref()).unwrap();
    let doc = cases();
    let (mut hits, mut none) = (0, 0);
    for (i, c) in doc.get("tube_along").arr().iter().enumerate() {
        let p = add(hex3(c.get("p")), off);
        let v = hex3(c.get("v"));
        let kind = c.get("kind").str();
        let r = if brute { t.set().project_along_brute(p, v, &opts()) } else { t.project_along(p, v, &opts()) };
        let r = r.unwrap_or_else(|e| panic!("case {i} ({kind}): {e}"));
        let want = c.get("hit");
        if want.is_null() {
            assert_eq!(r, Projection::NoHit, "case {i} ({kind}) at {p:?} along {v:?}");
            none += 1;
            continue;
        }
        let Projection::Hit(h) = r else { panic!("case {i} ({kind}): no hit, want {}", want.get("face").str()) };
        assert_eq!(face_label(&body, h.entity_id), want.get("face").str(), "case {i} ({kind})");
        let foot = add(hex3(want.get("foot")), off);
        assert!(len(sub(h.foot, foot)) <= FACE_REF_FOOT_TOL_M, "case {i}: foot {:?} vs {foot:?}", h.foot);
        let d = parse_hex_f64(want.get("distance").str());
        assert!((h.distance - d).abs() <= FACE_REF_FOOT_TOL_M, "case {i}: {} vs {d}", h.distance);
        assert_eq!(h.sign, HitSign::Along);
        let s_sign = want.get("s_sign").num();
        assert!(h.signed_distance * s_sign >= 0.0 && h.signed_distance.abs() == h.distance, "case {i}");
        hits += 1;
    }
    (hits, none)
}

/// AC-5: projection along a direction on the cylinder with a hole matches the oracle: the
/// nearest intersection inside the trim (face, foot within 1e-9 m), and lines through the
/// hole give `NoHit`. Also by the brute-force loop and far from the origin.
#[test]
fn ac5_tube_along_oracle() {
    let doc = cases();
    let through = doc.get("tube_along").arr().iter().filter(|c| c.get("kind").str() == "through_hole").count();
    assert!(through >= 50);
    let (hits, none) = check_tube_along([0.0; 3], false);
    assert!(hits >= 200 && none >= through, "{hits} hits, {none} no-hits");
    assert_eq!(check_tube_along([0.0; 3], true), (hits, none));
    assert_eq!(check_tube_along([4096.0, -8192.0, 16384.0], false), (hits, none));
}

// ------------------------------------------------------------------ concave body

fn check_lblock(off: V3) {
    let body = load_body("geo/dist/face/incid/lblock.json", off);
    let t = BodyTarget::new(&body, frame_ref()).unwrap();
    let mut kinds = [0usize; 2];
    for (i, c) in cases().get("lblock").arr().iter().enumerate() {
        let p = add(hex3(c.get("p")), off);
        let d = parse_hex_f64(c.get("distance").str());
        let Projection::Hit(h) = t.project(p, &opts()).unwrap() else { panic!("{i}: no hit") };
        assert!((h.distance - d).abs() <= FACE_REF_FOOT_TOL_M, "{i} ({}): {} vs {d}", c.get("kind").str(), h.distance);
        let want = match c.get("side").str() {
            "inside" => SignStatus::Inside,
            _ => SignStatus::Outside,
        };
        assert_eq!(h.sign, HitSign::Cad(want), "{i} ({}) at {p:?}", c.get("kind").str());
        assert_eq!(h.signed_distance < 0.0, want == SignStatus::Inside, "{i}");
        kinds[usize::from(want == SignStatus::Inside)] += 1;
    }
    assert!(kinds[0] >= 100 && kinds[1] >= 100, "{kinds:?}");
}

/// The concave L-block: distances within 1e-9 m and inside/outside of the oracle, near the
/// concave edge and corners too, at the origin and 2^17 m away.
#[test]
fn concave_lblock_matches_oracle() {
    check_lblock([0.0; 3]);
    check_lblock([131_072.0, -262_144.0, 65_536.0]);
}

// ------------------------------------------------------------------ cone apex

/// A foot at a cone apex never gets a wrong sign: points beyond the tip on and near the axis
/// (outside the solid cones, closest to the apex) are Outside or `Degenerate`, never Inside.
/// C30: apex at the origin, solid below it. C20: `r0 = 3`, half-angle 20 degrees, apex at
/// `z = -3 / tan(20 deg)` (the vertex C20-V1), solid above it.
#[test]
fn cone_apex_never_wrong_sign() {
    for (rel, up) in [("geo/dist/face/incid/cone_c30.json", 1.0), ("geo/dist/face/incid/cone_c20.json", -1.0)] {
        let body = load_body(rel, [0.0; 3]);
        let apex = body.vertices().iter().map(|v| v.point()).find(|p| p[0] == 0.0 && p[1] == 0.0).unwrap();
        let t = BodyTarget::new(&body, frame_ref()).unwrap();
        let mut degenerate = 0;
        for k in 1..=40 {
            let z = k as f64 / 4096.0;
            for dx in [0.0, z / 1024.0, -z / 4096.0] {
                let p = [apex[0] + dx, apex[1], apex[2] + up * z];
                let Projection::Hit(h) = t.project(p, &opts()).unwrap() else { panic!("{rel} {p:?}") };
                assert!(len(sub(h.foot, apex)) <= 1e-9, "{rel} {p:?}: foot {:?} is not the apex", h.foot);
                match h.sign {
                    HitSign::Cad(SignStatus::Outside) => assert!(h.signed_distance > 0.0),
                    HitSign::Cad(SignStatus::Undefined(SignUndefined::Degenerate)) => degenerate += 1,
                    other => panic!("{rel} {p:?}: {other:?}"),
                }
            }
        }
        assert!(degenerate > 0, "{rel}: the apex rule was not exercised");
        // Inside the solid just past the apex: the cone surface, Inside.
        let p = [apex[0], apex[1], apex[2] - up * 0.01];
        let Projection::Hit(h) = t.project(p, &opts()).unwrap() else { panic!() };
        assert_eq!(h.sign, HitSign::Cad(SignStatus::Inside), "{rel}");
    }
}

// ------------------------------------------------------------------ AC-6

/// AC-6: batch projection of 10^5 points with 1 and 8 threads gives identical bytes (closest
/// point on the L-block, and along a direction on the tube).
#[test]
fn ac6_threads_1_and_8_identical() {
    let body = load_body("geo/dist/face/incid/lblock.json", [0.0; 3]);
    let t = BodyTarget::new(&body, frame_ref()).unwrap();
    let mut g = SplitMix(6);
    let pts: Vec<V3> =
        (0..100_000).map(|_| [g.in_range(-0.5, 2.5), g.in_range(-0.5, 2.5), g.in_range(-0.5, 1.5)]).collect();
    let set = PointSet::from_points(frame_ref(), &pts).unwrap();
    let cancel = AtomicBool::new(false);
    let one = project_batch(&set, &t, &opts(), &BatchConfig { threads: 1, block: 4096 }, &cancel).unwrap();
    let eight = project_batch(&set, &t, &opts(), &BatchConfig { threads: 8, block: 1024 }, &cancel).unwrap();
    assert_eq!(encode(&one).unwrap(), encode(&eight).unwrap());
    assert_eq!(one.len(), 100_000);

    let tube = load_body("geo/dist/face/incid/tube.json", [0.0; 3]);
    let tt = BodyTarget::new(&tube, frame_ref()).unwrap();
    let pts: Vec<V3> =
        (0..20_000).map(|_| [g.in_range(-0.03, 0.03), g.in_range(-0.03, 0.03), g.in_range(-0.01, 0.02)]).collect();
    let set = PointSet::from_points(frame_ref(), &pts).unwrap();
    let o = ProjectionOptions { direction: Some([0.25, -0.5, 1.0]), ..opts() };
    let one = project_batch(&set, &tt, &o, &BatchConfig { threads: 1, block: 4096 }, &cancel).unwrap();
    let eight = project_batch(&set, &tt, &o, &BatchConfig { threads: 8, block: 64 }, &cancel).unwrap();
    assert_eq!(encode(&one).unwrap(), encode(&eight).unwrap());
}

// ------------------------------------------------------------------ targets, options, refusals

fn is_sync<T: Sync>() {}

#[test]
fn targets_are_sync_and_frames_are_checked() {
    is_sync::<BodyTarget<'static>>();
    is_sync::<FaceSetTarget<'static>>();
    is_sync::<FaceTarget<'static>>();
    is_sync::<InstanceTarget<'static>>();
    let body = load_body("geo/cad/topo/cube_labelled.json", [0.0; 3]);
    let t = BodyTarget::new(&body, frame_ref()).unwrap();
    let set = PointSet::from_points(FrameRef::from_bytes([8; 16]), &[[0.5, 0.5, 2.0]]).unwrap();
    let e = project_batch(&set, &t, &opts(), &BatchConfig::default(), &AtomicBool::new(false)).unwrap_err();
    assert!(matches!(e, BatchError::FrameMismatch(_)), "{e:?}");
}

/// A face target and a face set see only their faces; the sign still uses the whole body;
/// the subset option restricts a body target the same way.
#[test]
fn face_and_face_set_targets() {
    let body = load_body("geo/cad/topo/cube_labelled.json", [0.0; 3]);
    let top = face_ix(&body, "#56");
    let ft = FaceTarget::new(&body, frame_ref(), top).unwrap();
    // Closest to x = 0 overall, but the face target only has the top face.
    let p = [0.1, 0.5, 0.5];
    let Projection::Hit(h) = ft.project(p, &opts()).unwrap() else { panic!() };
    assert_eq!(face_label(&body, h.entity_id), "#56");
    assert!((h.distance - 0.5).abs() <= 1e-15);
    assert_eq!(h.sign, HitSign::Cad(SignStatus::Inside));
    let fs = FaceSetTarget::from_ids(&body, frame_ref(), &[tid("#54"), tid("#53")]).unwrap();
    let Projection::Hit(h) = fs.project(p, &opts()).unwrap() else { panic!() };
    assert_eq!(face_label(&body, h.entity_id), "#53");
    // Subset on the body target.
    let t = BodyTarget::new(&body, frame_ref()).unwrap();
    let mut ids: Vec<u64> = [face_ix(&body, "#54").index() as u64, face_ix(&body, "#53").index() as u64].to_vec();
    ids.sort_unstable();
    let sub = TriangleSubset::new(&ids, body.faces().len() as u64).unwrap();
    let o = ProjectionOptions { subset: Some(&sub), ..opts() };
    assert_eq!(t.project(p, &o).unwrap(), fs.project(p, &opts()).unwrap());
    let bad_ids = [0u64];
    let bad = TriangleSubset::new(&bad_ids, 12).unwrap();
    let o = ProjectionOptions { subset: Some(&bad), ..opts() };
    assert_eq!(t.check_options(&o), Err(ProjectError::Body(BodyError::SubsetMismatch)));
}

#[test]
fn refusals_fail_closed() {
    let body = load_body("geo/cad/topo/cube_labelled.json", [0.0; 3]);
    assert_eq!(FaceSetTarget::new(&body, frame_ref(), &[]).unwrap_err(), BodyError::EmptyTarget);
    let f = face_ix(&body, "#51");
    assert!(matches!(FaceSetTarget::new(&body, frame_ref(), &[f, f]).unwrap_err(), BodyError::DuplicateFace { .. }));
    assert_eq!(FaceSetTarget::from_ids(&body, frame_ref(), &[tid("#1")]).unwrap_err(), BodyError::UnknownFace);
    assert_eq!(BodyTarget::with_tolerance(&body, frame_ref(), 0.0).unwrap_err(), BodyError::BadTolerance);
    let t = BodyTarget::new(&body, frame_ref()).unwrap();
    let e = |r: Result<Projection, ProjectError>| match r {
        Err(ProjectError::Body(b)) => b,
        other => panic!("{other:?}"),
    };
    assert_eq!(e(t.project([f64::NAN, 0.0, 0.0], &opts())), BodyError::NonFiniteQuery);
    assert_eq!(e(t.project([1e300, 0.0, 0.0], &opts())), BodyError::QueryOutOfRange);
    let o = ProjectionOptions { max_distance: Some(-1.0), ..opts() };
    assert_eq!(e(t.project([0.5, 0.5, 0.5], &o)), BodyError::InvalidMaxDistance);
    assert_eq!(e(t.project_along([0.5, 0.5, 0.5], [0.0; 3], &opts())), BodyError::InvalidDirection);
    // A line lying in a face's plane is a continuum: refused, not guessed.
    assert!(matches!(e(t.project_along([0.5, 0.5, 1.0], [1.0, 0.0, 0.0], &opts())), BodyError::AlongDegenerate { .. }));
    // max_distance: no hit beyond it.
    let o = ProjectionOptions { max_distance: Some(0.2), ..opts() };
    assert_eq!(t.project([0.5, 0.5, 0.5], &o).unwrap(), Projection::NoHit);
    assert_eq!(t.project_along([0.5, 0.5, 0.5], [0.0, 0.0, 1.0], &o).unwrap(), Projection::NoHit);
}

/// Torus and NURBS faces are not intersected along a direction here: a line meeting such a
/// face's box is refused (fail closed). (Closest-point queries on the torus band all fail
/// closed on its face F1; see AC-1.)
#[test]
fn torus_and_nurbs_fail_closed() {
    let fx = load("torus_segment.json");
    let body = build_split_body(&fx.body);
    let t = BodyTarget::new(&body, frame_ref()).unwrap();
    let q = &fx.queries[0];
    let e = t.project_along(q.p, [0.0, 0.0, 1.0], &opts());
    assert!(matches!(&e, Err(ProjectError::Body(BodyError::AlongUnsupported { .. }))), "{e:?}");
    let fx = load("nurbs_rational_pair.json");
    let body = build_split_body(&fx.body);
    let t = BodyTarget::new(&body, frame_ref()).unwrap();
    let q = &fx.queries[0];
    let e = t.project_along(q.p, [0.0, 0.0, 1.0], &opts());
    assert!(matches!(&e, Err(ProjectError::Body(BodyError::AlongUnsupported { .. }))), "{e:?}");
}

/// Results are the same whatever the query order (no state between queries) and match
/// a fresh target (REQ-006).
#[test]
fn repeatable_and_stateless() {
    let body = load_body("geo/dist/face/incid/tube.json", [0.0; 3]);
    let a = BodyTarget::new(&body, frame_ref()).unwrap();
    let b = BodyTarget::new(&body, frame_ref()).unwrap();
    let mut g = SplitMix(77);
    let pts: Vec<V3> =
        (0..300).map(|_| [g.in_range(-0.03, 0.03), g.in_range(-0.03, 0.03), g.in_range(-0.01, 0.02)]).collect();
    let fwd: Vec<Vec<u8>> = pts.iter().map(|&p| record(&a.project(p, &opts()))).collect();
    let rev: Vec<Vec<u8>> = pts.iter().rev().map(|&p| record(&b.project(p, &opts()))).collect();
    let rev: Vec<Vec<u8>> = rev.into_iter().rev().collect();
    assert_eq!(fwd, rev);
}

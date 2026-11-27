//! GEO.CAD.TESS.EDGE acceptance tests against `fixtures/geo/cad/tess/analytic_set/`, whose
//! expected values come from the independent 60-digit oracle `make_tess_fixtures.py`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::J;
use fid_geo_cad::{
    derive_pcurves, sample_curve, tessellate_edges, Body, CurveDef, CurveParams, EdgeIx, EdgeReport, EdgeTess,
    EdgeTessError, EdgeTessParams, EntityIx, Frame, Label, SampleError, Sense, TopologyId, UvError, DEFAULT_TOL_M,
};
use fid_geo_nurbs::{AnalyticCurve, Curve, Surface};

const SET: &str = "../tess/analytic_set/";
const BODIES: [&str; 4] = ["cube", "washer", "sphere", "torus"];

fn script(name: &str) -> J {
    common::load(&format!("{SET}{name}.json"))
}

fn expected() -> J {
    common::load(&format!("{SET}expected.json"))
}

fn body(name: &str) -> Body {
    common::builder(&script(name)).build().unwrap()
}

fn params() -> EdgeTessParams {
    let e = expected();
    EdgeTessParams::new(e.get("chordal_m").f(), e.get("angular_rad").f()).unwrap()
}

fn run(b: &Body, p: &EdgeTessParams) -> EdgeTess {
    let pc = derive_pcurves(b, DEFAULT_TOL_M).unwrap();
    tessellate_edges(b, &pc, p).unwrap()
}

fn id(label: &str) -> TopologyId {
    TopologyId::Imported(Label::new(label).unwrap())
}

fn edge_ix(b: &Body, label: &str) -> EdgeIx {
    match b.lookup(&id(label)) {
        Some(EntityIx::Edge(e)) => e,
        other => panic!("{label}: {other:?}"),
    }
}

fn label_of(t: &TopologyId) -> String {
    match t {
        TopologyId::Imported(l) => l.as_str().to_string(),
        TopologyId::Derived(_) => panic!("derived ID"),
    }
}

fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|x| x.to_bits()).collect()
}

fn analytic(b: &Body, e: EdgeIx) -> AnalyticCurve {
    *b.edge(e).unwrap().curve().unwrap().as_analytic().unwrap()
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// Distance from `p` to the segment `[a, b]`, written independently of the crate.
fn seg_dist(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let e = sub(b, a);
    let ee = dot(e, e);
    let s = if ee == 0.0 { 0.0 } else { (dot(sub(p, a), e) / ee).clamp(0.0, 1.0) };
    norm(sub(p, [a[0] + s * e[0], a[1] + s * e[1], a[2] + s * e[2]]))
}

fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    (dot(a, b) / (norm(a) * norm(b))).clamp(-1.0, 1.0).acos()
}

// ---------------------------------------------------------------- AC-1

#[test]
fn ac1_dense_check_on_the_analytic_set() {
    let p = params();
    let exp = expected();
    for name in BODIES {
        let b = body(name);
        let out = run(&b, &p);
        assert!(out.reports.is_empty(), "{name}: {:?}", out.reports);
        let eb = exp.get("bodies").get(name);
        for es in &out.edges {
            let label = label_of(b.edge(es.edge).unwrap().id());
            let oe = eb.get("edges").get(&label);
            assert_eq!(bits(&es.t), bits(&oe.get("t").fs()), "{name}/{label}: parameters");
            assert!(es.achieved_m <= p.chordal_m && es.achieved_rad <= p.angular_rad, "{name}/{label}");
            assert!((es.achieved_m - oe.get("achieved_m").f()).abs() <= 1e-15, "{name}/{label}: achieved_m");
            assert!((es.achieved_rad - oe.get("achieved_rad").f()).abs() <= 1e-12, "{name}/{label}: achieved_rad");
            let c = analytic(&b, es.edge);
            let (mut worst_m, mut worst_a) = (0.0f64, 0.0f64);
            for w in es.t.windows(2) {
                let (l, r) = (c.eval(w[0], 1).unwrap(), c.eval(w[1], 1).unwrap());
                let (tl, tr) = (l.d1.unwrap(), r.d1.unwrap());
                worst_a = worst_a.max(angle(tl, tr));
                for j in 1..1000 {
                    let s = w[0] + (w[1] - w[0]) * (j as f64) / 1000.0;
                    let q = c.eval(s, 1).unwrap();
                    worst_m = worst_m.max(seg_dist(q.p, l.p, r.p));
                    let tq = q.d1.unwrap();
                    worst_a = worst_a.max(angle(tl, tq)).max(angle(tq, tr));
                }
            }
            assert!(worst_m <= p.chordal_m, "{name}/{label}: dense chordal {worst_m:e}");
            assert!(worst_a <= p.angular_rad, "{name}/{label}: dense angle {worst_a}");
        }
    }
}

// ---------------------------------------------------------------- AC-2

#[test]
fn ac2_coedges_share_vertex_indices() {
    let p = params();
    let exp = expected();
    for name in BODIES.iter().chain(&["plate_offset"]) {
        let b = body(name);
        let out = run(&b, &p);
        let eb = exp.get("bodies").get(name);
        assert_eq!(out.points.len(), eb.get("n_points").u(), "{name}: mesh vertices");
        for es in &out.edges {
            let label = label_of(b.edge(es.edge).unwrap().id());
            let want: Vec<u64> = eb.get("edges").get(&label).get("vertex_ix").fs().iter().map(|&x| x as u64).collect();
            assert_eq!(es.vertex_ix, want, "{name}/{label}: vertex indices");
            let users: Vec<_> = out.coedges.iter().filter(|c| b.coedge(c.coedge).unwrap().edge() == es.edge).collect();
            if *name != "plate_offset" {
                assert_eq!(users.len(), 2, "{name}/{label}: closed shell");
                assert_ne!(
                    b.coedge(users[0].coedge).unwrap().sense(),
                    b.coedge(users[1].coedge).unwrap().sense(),
                    "{name}/{label}"
                );
            }
            for c in &users {
                let mut back = c.vertex_ix.clone();
                if b.coedge(c.coedge).unwrap().sense() == Sense::Reversed {
                    back.reverse();
                }
                assert_eq!(back, es.vertex_ix, "{name}/{label}: coedge list");
            }
            // Ends are the body vertices' shared indices.
            let e = b.edge(es.edge).unwrap();
            let (first, last) = (es.vertex_ix[0], *es.vertex_ix.last().unwrap());
            assert_eq!(out.points[first as usize], b.vertex(e.start()).unwrap().point());
            assert_eq!(out.points[last as usize], b.vertex(e.end()).unwrap().point());
        }
        for c in &out.coedges {
            let label = label_of(b.coedge(c.coedge).unwrap().id());
            let oc = eb.get("coedges").get(&label);
            let want: Vec<u64> = oc.get("vertex_ix").fs().iter().map(|&x| x as u64).collect();
            assert_eq!(c.vertex_ix, want, "{name}/{label}: coedge vertex indices");
        }
    }
}

#[test]
fn ac2_one_mesh_vertex_per_body_vertex() {
    let p = params();
    for name in BODIES {
        let b = body(name);
        let out = run(&b, &p);
        let mut ends: Vec<(usize, u64)> = Vec::new();
        for es in &out.edges {
            let e = b.edge(es.edge).unwrap();
            ends.push((e.start().index(), es.vertex_ix[0]));
            ends.push((e.end().index(), *es.vertex_ix.last().unwrap()));
        }
        ends.sort();
        ends.dedup();
        assert_eq!(ends.len(), b.vertices().len(), "{name}: one index per body vertex");
        let interior: usize = out.edges.iter().map(|e| e.t.len() - 2).sum();
        assert_eq!(out.points.len(), b.vertices().len() + interior, "{name}");
    }
}

#[test]
fn coedge_uv_lies_in_the_face_chart() {
    let p = params();
    let exp = expected();
    for name in BODIES.iter().chain(&["plate_offset"]) {
        let b = body(name);
        let out = run(&b, &p);
        let eb = exp.get("bodies").get(name);
        for c in &out.coedges {
            let co = b.coedge(c.coedge).unwrap();
            let label = label_of(co.id());
            if *name == "plate_offset" && label == "c-hole" {
                // The hole circle is 1e-6 m off the plane: GEO.CAD.TOPO.PCURVE refuses it, the
                // vertex indices stay.
                assert!(matches!(c.uv, Err(UvError::Pcurve(_))), "{:?}", c.uv);
                assert_eq!(c.vertex_ix.len(), 65);
                continue;
            }
            let uv = c.uv.as_ref().unwrap_or_else(|e| panic!("{name}/{label}: {e:?}"));
            assert_eq!(uv.len(), c.vertex_ix.len());
            let face = b.face(b.r#loop(co.r#loop()).unwrap().face()).unwrap();
            let surf = face.surface().as_analytic().unwrap();
            let line = matches!(analytic(&b, co.edge()), AnalyticCurve::Line(_));
            let bound = if line { 1e-12 } else { 1e-7 };
            for (k, (q, &ix)) in uv.iter().zip(&c.vertex_ix).enumerate() {
                let s = surf.eval(q[0], q[1], 0).unwrap().p;
                let d = norm(sub(s, out.points[ix as usize]));
                assert!(d <= bound, "{name}/{label} sample {k}: S(uv) is {d:e} m from the mesh point");
            }
            if let Some(want) = eb.get("coedges").get(&label).opt("uv") {
                for (k, (q, w)) in uv.iter().zip(want.arr()).enumerate() {
                    let w = w.v2();
                    let d = ((q[0] - w[0]).powi(2) + (q[1] - w[1]).powi(2)).sqrt();
                    assert!(d <= bound, "{name}/{label} sample {k}: uv {q:?} vs oracle {w:?}");
                }
            }
        }
    }
}

#[test]
fn seam_samples_appear_at_both_chart_sides() {
    let p = params();
    for (name, seam, a, bl, period_ix) in [
        ("washer", "e-outer-seam", "c-oc-seam-up", "c-oc-seam-down", 0usize),
        ("washer", "e-inner-seam", "c-ic-seam-up", "c-ic-seam-down", 0),
        ("torus", "e-v-seam", "c-v", "c-v-back", 0),
        ("torus", "e-u-seam", "c-u", "c-u-back", 1),
    ] {
        let b = body(name);
        let out = run(&b, &p);
        let e = edge_ix(&b, seam);
        let find = |l: &str| out.coedges.iter().find(|c| b.coedge(c.coedge).unwrap().id() == &id(l)).unwrap();
        let (ca, cb) = (find(a), find(bl));
        let (ua, ub) = (ca.uv.as_ref().unwrap(), cb.uv.as_ref().unwrap());
        let n = out.edges[e.index()].t.len();
        assert_eq!((ua.len(), ub.len()), (n, n));
        let back: Vec<u64> = cb.vertex_ix.iter().rev().copied().collect();
        assert_eq!(ca.vertex_ix, back);
        for (k, (qa, qb)) in ua.iter().zip(ub.iter().rev()).enumerate() {
            let jump = (qa[period_ix] - qb[period_ix]).abs();
            assert!((jump - core::f64::consts::TAU).abs() <= 1e-9, "{name}/{seam} sample {k}: {qa:?} {qb:?}");
            let other = 1 - period_ix;
            assert!((qa[other] - qb[other]).abs() <= 1e-9, "{name}/{seam} sample {k}");
        }
    }
}

// ---------------------------------------------------------------- AC-3

fn circle(radius: f64) -> CurveDef {
    let frame = Frame { origin: [0.0, 0.0, 0.0], axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] };
    CurveDef::analytic(CurveParams::Circle { frame, radius }).unwrap()
}

fn case_params(c: &J) -> EdgeTessParams {
    let mut p = EdgeTessParams::new(c.get("chordal_m").f(), c.get("angular_rad").f()).unwrap();
    p.max_depth = c.get("max_depth").u() as u32;
    p
}

#[test]
fn ac3_circle_sample_count_is_the_closed_form_minimum() {
    let exp = expected();
    let cases = exp.get("circle_cases").arr();
    assert_eq!(cases.len(), 5);
    for c in cases {
        let name = c.get("name").str();
        let s = sample_curve(&circle(c.get("radius").f()), c.get("range").v2(), &case_params(c)).unwrap();
        assert!(s.within_tolerance, "{name}");
        assert_eq!(s.t.len(), c.get("closed_form_samples").u(), "{name}: closed form");
        assert_eq!(bits(&s.t), bits(&c.get("t").fs()), "{name}: parameters");
        assert_eq!(s.points.len(), s.t.len());
        assert!((s.achieved_m - c.get("achieved_m").f()).abs() <= 1e-15, "{name}");
    }
}

// ---------------------------------------------------------------- REQ-005

#[test]
fn req005_depth_limit_keeps_the_samples_and_reports() {
    let exp = expected();
    let c = &exp.get("tolerance_cases").arr()[0];
    let s = sample_curve(&circle(c.get("radius").f()), c.get("range").v2(), &case_params(c)).unwrap();
    assert!(!s.within_tolerance);
    assert_eq!(bits(&s.t), bits(&c.get("t").fs()));
    let rel = |a: f64, b: f64| ((a - b) / b).abs();
    assert!(rel(s.achieved_m, c.get("achieved_m").f()) <= 1e-12, "{}", s.achieved_m);
    assert!(rel(s.achieved_rad, c.get("achieved_rad").f()) <= 1e-12, "{}", s.achieved_rad);

    // Through the body: every circle of the washer is out of tolerance at depth 3, the seams are
    // not, and both coedges of each circle still share its 9 samples.
    let b = body("washer");
    let mut p = params();
    p.max_depth = 3;
    let out = run(&b, &p);
    let reported: Vec<String> = out
        .reports
        .iter()
        .map(|r| {
            assert!(matches!(r, EdgeReport::EdgeTolerance { .. }), "{r:?}");
            label_of(b.edge(r.edge()).unwrap().id())
        })
        .collect();
    assert_eq!(reported, ["e-inner-bottom", "e-inner-top", "e-outer-bottom", "e-outer-top"]);
    for l in &reported {
        let es = &out.edges[edge_ix(&b, l).index()];
        assert_eq!(es.t.len(), 9);
        for c in out.coedges.iter().filter(|c| b.coedge(c.coedge).unwrap().edge() == es.edge) {
            assert_eq!(c.vertex_ix.len(), 9);
        }
    }
}

// ---------------------------------------------------------------- AC-4

#[test]
fn ac4_reverse_edge_order_is_bit_identical() {
    let p = params();
    for name in BODIES {
        let b = body(name);
        let out = run(&b, &p);
        for es in out.edges.iter().rev() {
            let e = b.edge(es.edge).unwrap();
            let s = sample_curve(e.curve().unwrap(), e.param_range(), &p).unwrap();
            assert_eq!(bits(&s.t), bits(&es.t), "{name}");
        }
    }
    for name in ["cube", "washer"] {
        let (a, r) = (body(name), body(&format!("{name}_reversed")));
        let (oa, or) = (run(&a, &p), run(&r, &p));
        assert_eq!(oa.points.len(), or.points.len());
        for (x, y) in oa.points.iter().zip(&or.points) {
            assert_eq!(bits(x), bits(y), "{name}: points");
        }
        for es in &oa.edges {
            let label = label_of(a.edge(es.edge).unwrap().id());
            let other = &or.edges[edge_ix(&r, &label).index()];
            assert_eq!(bits(&es.t), bits(&other.t), "{name}/{label}");
            assert_eq!(es.vertex_ix, other.vertex_ix, "{name}/{label}");
        }
        for c in &oa.coedges {
            let cid = a.coedge(c.coedge).unwrap().id().clone();
            let other = or.coedges.iter().find(|x| r.coedge(x.coedge).unwrap().id() == &cid).unwrap();
            assert_eq!(c.vertex_ix, other.vertex_ix);
        }
    }
    // The reversed builder really changes the canonical edge order.
    let (a, r) = (body("washer"), body("washer_reversed"));
    let la: Vec<String> = a.edges().iter().map(|e| label_of(e.id())).collect();
    let lr: Vec<String> = r.edges().iter().map(|e| label_of(e.id())).collect();
    assert_ne!(la, lr);
}

// ---------------------------------------------------------------- AC-5

#[test]
fn ac5_surface_gap_is_reported() {
    let p = params();
    let b = body("plate_offset");
    let out = run(&b, &p);
    let eb = expected();
    let eb = eb.get("bodies").get("plate_offset");
    for es in &out.edges {
        let label = label_of(b.edge(es.edge).unwrap().id());
        let want = eb.get("edges").get(&label).get("max_surface_gap_m").f();
        let got = es.max_surface_gap_m.unwrap();
        assert!((got - want).abs() <= 1e-15, "{label}: {got:e} vs {want:e}");
        assert!(!es.gap_upper_bound);
    }
    let hole = &out.edges[edge_ix(&b, "e-hole").index()];
    assert!((hole.max_surface_gap_m.unwrap() - 1e-6).abs() <= 1e-15);
    assert!(out.reports.is_empty(), "{:?}", out.reports);
    for name in BODIES {
        let b = body(name);
        for es in &run(&b, &p).edges {
            assert!(es.max_surface_gap_m.unwrap() <= 1e-12, "{name}");
        }
    }
}

// ---------------------------------------------------------------- errors and reports

#[test]
fn parameters_are_checked() {
    for (t, a) in [(0.0, 0.2), (-1e-5, 0.2), (f64::NAN, 0.2), (f64::INFINITY, 0.2), (1e-5, 0.0), (1e-5, 3.2)] {
        assert!(matches!(EdgeTessParams::new(t, a), Err(SampleError::BadParams { .. })), "{t} {a}");
    }
    let mut p = params();
    p.max_depth = 25;
    let c = circle(0.01);
    assert_eq!(sample_curve(&c, [0.0, 1.0], &p), Err(SampleError::BadParams { what: "max_depth" }));
    p = params();
    p.max_samples = 1;
    assert_eq!(sample_curve(&c, [0.0, 1.0], &p), Err(SampleError::BadParams { what: "max_samples" }));
    p = params();
    p.angular_rad = f64::NAN;
    let b = body("cube");
    let pc = derive_pcurves(&b, DEFAULT_TOL_M).unwrap();
    assert_eq!(
        tessellate_edges(&b, &pc, &p),
        Err(EdgeTessError::Sample(SampleError::BadParams { what: "angular_rad" }))
    );
    p = params();
    for r in [[1.0, 1.0], [1.0, 0.0], [f64::NAN, 1.0], [0.0, f64::INFINITY]] {
        assert_eq!(sample_curve(&c, r, &p), Err(SampleError::BadRange), "{r:?}");
    }
}

#[test]
fn sample_bound_fails_closed() {
    let mut p = params();
    p.max_samples = 128;
    let c = circle(0.01);
    assert_eq!(sample_curve(&c, [0.0, core::f64::consts::TAU], &p), Err(SampleError::TooManySamples { limit: 128 }));
    p.max_samples = 129;
    assert_eq!(sample_curve(&c, [0.0, core::f64::consts::TAU], &p).unwrap().t.len(), 129);
    let b = body("washer");
    p.max_samples = 383;
    let pc = derive_pcurves(&b, DEFAULT_TOL_M).unwrap();
    assert_eq!(tessellate_edges(&b, &pc, &p), Err(EdgeTessError::Sample(SampleError::TooManySamples { limit: 383 })));
    p.max_samples = 384;
    assert_eq!(tessellate_edges(&b, &pc, &p).unwrap().points.len(), 384);
}

#[test]
fn pcurves_of_another_body_are_refused() {
    let p = params();
    let (cube, washer) = (body("cube"), body("washer"));
    let pc = derive_pcurves(&washer, DEFAULT_TOL_M).unwrap();
    assert_eq!(tessellate_edges(&cube, &pc, &p), Err(EdgeTessError::PcurvesMismatch));
}

#[test]
fn edges_without_curves_keep_their_vertices() {
    // Planar faces with imported line pcurves and no 3-D curves (GEO.CAD.TOPO.TRIM fixture).
    let b = common::build("trim_polygons");
    let out = run(&b, &params());
    assert_eq!(out.reports.len(), b.edges().len());
    for r in &out.reports {
        assert!(matches!(r, EdgeReport::NoEdgeCurve { .. }), "{r:?}");
    }
    for es in &out.edges {
        assert_eq!(bits(&es.t), bits(&b.edge(es.edge).unwrap().param_range()));
        assert_eq!(es.achieved_m, f64::INFINITY);
    }
    assert_eq!(out.points.len(), b.vertices().len());
    for c in &out.coedges {
        assert_eq!(c.uv.as_ref().unwrap().len(), 2);
    }
}

fn set_point(j: &mut J, vertex: usize, p: [f64; 3]) {
    let J::Obj(m) = j else { panic!() };
    let Some(J::Arr(vs)) = m.get_mut("vertices") else { panic!() };
    let J::Obj(v) = &mut vs[vertex] else { panic!() };
    v.insert("point".to_string(), J::Arr(p.iter().map(|&x| J::Num(x)).collect()));
}

#[test]
fn req003_vertex_off_its_curve_is_reported() {
    let mut s = script("cube");
    set_point(&mut s, 7, [0.02, 0.02, 0.02 + 1e-9]);
    let b = common::builder(&s).build().unwrap();
    let out = run(&b, &params());
    let mut off: Vec<String> = out
        .reports
        .iter()
        .filter_map(|r| match r {
            EdgeReport::VertexOffCurve { edge, gap_m } => {
                assert!((gap_m - 1e-9).abs() <= 1e-18, "{gap_m:e}");
                Some(label_of(b.edge(*edge).unwrap().id()))
            }
            _ => None,
        })
        .collect();
    off.sort();
    assert_eq!(off, ["e37", "e57", "e67"]);
    // The mesh point of the vertex is the stored point.
    let v7 = b.lookup(&id("v7"));
    let Some(EntityIx::Vertex(v7)) = v7 else { panic!() };
    let es = &out.edges[edge_ix(&b, "e67").index()];
    assert_eq!(out.points[*es.vertex_ix.last().unwrap() as usize], b.vertex(v7).unwrap().point());
}

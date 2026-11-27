//! GEO.CAD.NOMINAL.DIRECT acceptance tests. Fixture and expected values:
//! `fixtures/geo/cad/nominal/` (`make_nominal_fixtures.py`, independent stdlib Python).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::J;
use core::f64::consts::{FRAC_PI_2, PI, TAU};
use fid_geo_cad::{
    map_edge, map_face, map_vertex, Body, BodyBuilder, CurveDef, CurveParams, DirectError, DirectResult, EdgeIx,
    EntityIx, EntityKind, Extent, FaceIx, Frame, Label, Material, Method, NominalElement, NominalKind, NominalParams,
    Sense, SurfaceDef, SurfaceParams, TopologyId, UnsupportedKind, VertexIx,
};
use fid_geo_nurbs::{NurbsCurve, NurbsCurveParts, NurbsSurface, NurbsSurfaceParts};

fn script() -> J {
    common::load("../nominal/analytic_block.json")
}

fn expected() -> J {
    common::load("../nominal/expected.json")
}

fn block() -> Body {
    common::builder(&script()).build().unwrap()
}

fn id(label: &str) -> TopologyId {
    TopologyId::Imported(Label::new(label).unwrap())
}

fn face_ix(body: &Body, label: &str) -> FaceIx {
    match body.lookup(&id(label)) {
        Some(EntityIx::Face(ix)) => ix,
        other => panic!("{label}: {other:?}"),
    }
}

fn edge_ix(body: &Body, label: &str) -> EdgeIx {
    match body.lookup(&id(label)) {
        Some(EntityIx::Edge(ix)) => ix,
        other => panic!("{label}: {other:?}"),
    }
}

fn vertex_ix(body: &Body, label: &str) -> VertexIx {
    match body.lookup(&id(label)) {
        Some(EntityIx::Vertex(ix)) => ix,
        other => panic!("{label}: {other:?}"),
    }
}

fn element(r: Result<DirectResult, DirectError>) -> NominalElement {
    match r.unwrap() {
        DirectResult::Element(e) => e,
        other => panic!("not an element: {other:?}"),
    }
}

fn bits3(a: [f64; 3]) -> [u64; 3] {
    a.map(f64::to_bits)
}

fn same_frame(a: &Frame, b: &Frame) {
    assert_eq!(bits3(a.origin), bits3(b.origin));
    assert_eq!(bits3(a.axis), bits3(b.axis));
    assert_eq!(bits3(a.ref_dir), bits3(b.ref_dir));
}

fn same(a: f64, b: f64) {
    assert_eq!(a.to_bits(), b.to_bits(), "{a} vs {b}");
}

/// The element's parameters equal the fixture's surface definition bit for bit.
fn check_surface_params(e: &NominalElement, g: &J) {
    let fr = common::frame(g);
    match (*e.params(), g.get("type").str()) {
        (NominalParams::Plane { frame }, "plane") => same_frame(&frame, &fr),
        (NominalParams::Cylinder { frame, radius }, "cylinder") => {
            same_frame(&frame, &fr);
            same(radius, g.get("radius").f());
        }
        (NominalParams::Cone { frame, radius, half_angle }, "cone") => {
            same_frame(&frame, &fr);
            same(radius, g.get("radius").f());
            same(half_angle, g.get("half_angle").f());
        }
        (NominalParams::Sphere { frame, radius }, "sphere") => {
            same_frame(&frame, &fr);
            same(radius, g.get("radius").f());
        }
        (NominalParams::Torus { frame, major_radius, minor_radius }, "torus") => {
            same_frame(&frame, &fr);
            same(major_radius, g.get("major_radius").f());
            same(minor_radius, g.get("minor_radius").f());
        }
        (p, t) => panic!("{p:?} for a {t} surface"),
    }
}

fn check_extent(got: &Extent, want: &J) {
    match (got, want) {
        (Extent::NotDefined, J::Str(s)) => assert_eq!(s, "not_defined"),
        (Extent::Planar { u, v }, J::Obj(m)) if m.contains_key("planar") => {
            let p = want.get("planar");
            let (wu, wv) = (p.get("u").v2(), p.get("v").v2());
            same(u[0], wu[0]);
            same(u[1], wu[1]);
            same(v[0], wv[0]);
            same(v[1], wv[1]);
        }
        (Extent::Axial { h_min, h_max }, J::Obj(m)) if m.contains_key("axial") => {
            let w = want.get("axial").v2();
            same(*h_min, w[0]);
            same(*h_max, w[1]);
        }
        (Extent::Angular { a0, a1 }, J::Obj(m)) if m.contains_key("angular") => {
            let w = want.get("angular").v2();
            same(*a0, w[0]);
            same(*a1, w[1]);
        }
        (g, w) => panic!("extent {g:?}, expected {w:?}"),
    }
}

fn material(s: &str) -> Material {
    match s {
        "Outside" => Material::Outside,
        "Inside" => Material::Inside,
        m => panic!("material {m}"),
    }
}

// ---------------------------------------------------------------- AC-1, AC-4

#[test]
fn ac1_every_face_of_the_analytic_block_is_copied_exactly() {
    let body = block();
    let sc = script();
    let exp = expected();
    let faces = sc.get("faces").arr();
    assert_eq!(faces.len(), 12);
    assert_eq!(exp.get("faces").obj().len(), faces.len());
    for f in faces {
        let label = f.get("label").str();
        let want = exp.get("faces").get(label);
        let e = element(map_face(&body, face_ix(&body, label)));
        assert_eq!(e.kind().as_str(), want.get("kind").str(), "{label}");
        check_surface_params(&e, f.get("surface"));
        assert_eq!(e.material(), Some(material(want.get("material").str())), "{label}");
        assert_eq!(e.method(), Method::Direct);
        assert_eq!(e.max_deviation_m().to_bits(), 0.0f64.to_bits());
        assert_eq!(e.sources(), &[id(label)]);
        check_extent(e.extent(), want.get("extent"));
    }
}

#[test]
fn ac1_hole_countersink_dimple_and_fillet_are_inside_boss_outside() {
    let body = block();
    for (label, kind, m) in [
        ("f.hole", NominalKind::Cylinder, Material::Inside),
        ("f.csink", NominalKind::Cone, Material::Inside),
        ("f.dimple", NominalKind::Sphere, Material::Inside),
        ("f.fillet", NominalKind::Torus, Material::Inside),
        ("f.boss", NominalKind::Cylinder, Material::Outside),
    ] {
        let e = element(map_face(&body, face_ix(&body, label)));
        assert_eq!((e.kind(), e.material()), (kind, Some(m)), "{label}");
        assert_eq!(e.plane_normal_out_of_material(), None);
    }
}

#[test]
fn ac1_plane_normal_out_of_material_follows_the_sense() {
    let body = block();
    // Bottom: axis -Z, sense same; x0: axis +X, sense reversed (normal -X out of material).
    let bottom = element(map_face(&body, face_ix(&body, "f.box.bottom")));
    assert_eq!(bottom.plane_normal_out_of_material().map(bits3), Some(bits3([0.0, 0.0, -1.0])));
    let x0 = element(map_face(&body, face_ix(&body, "f.box.x0")));
    assert_eq!(x0.material(), Some(Material::Inside));
    assert_eq!(x0.plane_normal_out_of_material().map(bits3), Some(bits3([-1.0, 0.0, 0.0])));
}

#[test]
fn ac4_extents_are_bit_identical_and_independent_of_builder_order() {
    // The expected extents are exact binary64 values (checked by the oracle), compared bit for
    // bit in ac1 on every CI platform; here the same body built with every list reversed must
    // give the same elements.
    let sc = script();
    let body = block();
    let rev = common::builder(&reversed(&sc)).build().unwrap();
    for f in sc.get("faces").arr() {
        let label = f.get("label").str();
        let a = element(map_face(&body, face_ix(&body, label)));
        let b = element(map_face(&rev, face_ix(&rev, label)));
        assert_eq!(a, b, "{label}");
    }
}

/// The script with every builder list reversed and references remapped (loops keep their
/// cyclic coedge order).
fn reversed(sc: &J) -> J {
    use std::collections::BTreeMap;
    let len = |k: &str| sc.get(k).arr().len();
    let (nv, ne, nc, nl, nf) = (len("vertices"), len("edges"), len("coedges"), len("loops"), len("faces"));
    let remap = |j: &J, key: &str, n: usize| -> J {
        let mut m: BTreeMap<String, J> = j.obj().clone();
        let new = match j.get(key) {
            J::Num(x) => J::Num((n - 1 - *x as usize) as f64),
            J::Arr(xs) => J::Arr(xs.iter().map(|x| J::Num((n - 1 - x.u()) as f64)).collect()),
            other => panic!("{other:?}"),
        };
        m.insert(key.to_string(), new);
        J::Obj(m)
    };
    let mut out = sc.obj().clone();
    let rev_list = |k: &str, f: &dyn Fn(&J) -> J| J::Arr(sc.get(k).arr().iter().rev().map(f).collect());
    out.insert("vertices".into(), rev_list("vertices", &|v| v.clone()));
    out.insert("edges".into(), rev_list("edges", &|e| remap(&remap(e, "start", nv), "end", nv)));
    out.insert("coedges".into(), rev_list("coedges", &|c| remap(c, "edge", ne)));
    out.insert("loops".into(), rev_list("loops", &|l| remap(l, "coedges", nc)));
    out.insert("faces".into(), rev_list("faces", &|f| remap(f, "loops", nl)));
    let shell_faces = J::Arr((0..nf).rev().map(|i| J::Num((nf - 1 - i) as f64)).collect());
    let mut shell = BTreeMap::new();
    shell.insert("faces".to_string(), shell_faces);
    shell.insert("closed".to_string(), J::Bool(true));
    out.insert("shells".into(), J::Arr(vec![J::Obj(shell)]));
    J::Obj(out)
}

// ---------------------------------------------------------------- AC-2

#[test]
fn ac2_hole_edge_is_a_full_circle_and_a_corner_is_a_point() {
    let body = block();
    let sc = script();
    let hole = sc.get("edges").arr().iter().find(|e| e.get("label").str() == "e.hole.bottom").unwrap();
    let e = element(map_edge(&body, edge_ix(&body, "e.hole.bottom")));
    match *e.params() {
        NominalParams::Circle { frame, radius } => {
            same_frame(&frame, &common::frame(hole.get("curve")));
            same(radius, 0.0078125);
        }
        p => panic!("{p:?}"),
    }
    assert_eq!(e.extent(), &Extent::Angular { a0: 0.0, a1: TAU });
    assert_eq!(e.material(), None);
    assert_eq!(e.sources(), &[id("e.hole.bottom")]);

    let p = element(map_vertex(&body, vertex_ix(&body, "v.corner.000")));
    assert_eq!(p.kind(), NominalKind::Point);
    assert_eq!(p.params(), &NominalParams::Point { point: [0.0, 0.0, 0.0] });
    assert_eq!(p.extent(), &Extent::NotDefined);
    assert_eq!(p.method(), Method::Direct);
}

#[test]
fn ac2_every_edge_and_vertex_matches_the_oracle() {
    let body = block();
    let sc = script();
    let exp = expected();
    for e in sc.get("edges").arr() {
        let label = e.get("label").str();
        let want = exp.get("edges").get(label);
        let got = element(map_edge(&body, edge_ix(&body, label)));
        assert_eq!(got.kind().as_str(), want.get("kind").str(), "{label}");
        let fr = common::frame(e.get("curve"));
        match *got.params() {
            NominalParams::Line { frame } => same_frame(&frame, &fr),
            NominalParams::Circle { frame, radius } => {
                same_frame(&frame, &fr);
                same(radius, e.get("curve").get("radius").f());
            }
            p => panic!("{p:?}"),
        }
        check_extent(got.extent(), want.get("extent"));
    }
    for v in sc.get("vertices").arr() {
        let label = v.get("label").str();
        let want = exp.get("vertices").get(label);
        let got = element(map_vertex(&body, vertex_ix(&body, label)));
        assert_eq!(got.kind().as_str(), want.get("kind").str());
        match *got.params() {
            NominalParams::Point { point } => assert_eq!(bits3(point), bits3(want.get("point").v3())),
            p => panic!("{p:?}"),
        }
    }
}

// ---------------------------------------------------------------- AC-3 and other refusals

const Z: Frame = Frame { origin: [0.0, 0.0, 0.0], axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] };

fn plane() -> SurfaceDef {
    SurfaceDef::analytic(SurfaceParams::Plane { frame: Z }).unwrap()
}

fn nurbs_segment(a: [f64; 3], b: [f64; 3]) -> CurveDef {
    CurveDef::nurbs(
        NurbsCurve::new(NurbsCurveParts {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            ctrl: vec![a, b],
            weights: None,
            periodic: false,
        })
        .unwrap(),
    )
    .unwrap()
}

fn line(origin: [f64; 3], dir: [f64; 3]) -> CurveDef {
    CurveDef::analytic(CurveParams::Line { frame: Frame { origin, axis: [0.0, 0.0, 1.0], ref_dir: dir } }).unwrap()
}

fn unit_circle() -> CurveDef {
    CurveDef::analytic(CurveParams::Circle { frame: Z, radius: 1.0 }).unwrap()
}

/// A plane face bounded by the unit-circle arc `[t0, t1]` and a closing edge `back`
/// (built from the arc's end point to its start point).
fn arc_face(t0: f64, t1: f64, back: impl Fn([f64; 3], [f64; 3]) -> Option<CurveDef>) -> Body {
    let at = |t: f64| [fid_num::f::cos(t).unwrap(), fid_num::f::sin(t).unwrap(), 0.0];
    let (a, b) = (at(t0), at(t1));
    let mut bb = BodyBuilder::new([7; 16]);
    let va = bb.add_vertex(a, Some("v.a"));
    let vb = bb.add_vertex(b, Some("v.b"));
    let arc = bb.add_edge(Some(unit_circle()), va, vb, [t0, t1], Some("e.arc"));
    let len = fid_num::f::hypot(a[0] - b[0], a[1] - b[1]).unwrap();
    let close = bb.add_edge(back(b, a), vb, va, [0.0, len], Some("e.back"));
    let c0 = bb.add_coedge(arc, Sense::Same, None, None);
    let c1 = bb.add_coedge(close, Sense::Same, None, None);
    let l = bb.add_loop(vec![c0, c1], true, None);
    let f = bb.add_face(plane(), Sense::Same, vec![l], Some("f"));
    bb.add_shell(vec![f], false);
    bb.build().unwrap()
}

fn straight(from: [f64; 3], to: [f64; 3]) -> Option<CurveDef> {
    let d = [to[0] - from[0], to[1] - from[1], 0.0];
    let n = fid_num::f::hypot(d[0], d[1]).unwrap();
    Some(line(from, [d[0] / n, d[1] / n, 0.0]))
}

#[test]
fn ac3_nurbs_face_is_not_analytic() {
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
    let mut bb = BodyBuilder::new([1; 16]);
    let f = bb.add_face(SurfaceDef::nurbs(s).unwrap(), Sense::Same, vec![], Some("f.nurbs"));
    bb.add_shell(vec![f], false);
    let body = bb.build().unwrap();
    assert_eq!(map_face(&body, face_ix(&body, "f.nurbs")), Ok(DirectResult::NotAnalytic));
}

#[test]
fn ac3_ellipse_edge_is_unsupported_but_bounds_a_plane_exactly() {
    let ellipse = CurveDef::analytic(CurveParams::Ellipse { frame: Z, semi_axis_1: 0.02, semi_axis_2: 0.01 }).unwrap();
    let mut bb = BodyBuilder::new([2; 16]);
    let v = bb.add_vertex([0.02, 0.0, 0.0], Some("v.e"));
    let e = bb.add_edge(Some(ellipse), v, v, [0.0, TAU], Some("e.ellipse"));
    let c = bb.add_coedge(e, Sense::Same, None, None);
    let l = bb.add_loop(vec![c], true, None);
    let f = bb.add_face(plane(), Sense::Reversed, vec![l], Some("f.ellipse"));
    bb.add_shell(vec![f], false);
    let body = bb.build().unwrap();
    assert_eq!(
        map_edge(&body, edge_ix(&body, "e.ellipse")),
        Ok(DirectResult::UnsupportedKind { kind: UnsupportedKind::Ellipse })
    );
    let face = element(map_face(&body, face_ix(&body, "f.ellipse")));
    assert_eq!(face.extent(), &Extent::Planar { u: [-0.02, 0.02], v: [-0.01, 0.01] });
    assert_eq!(face.material(), Some(Material::Inside));
    assert_eq!(face.plane_normal_out_of_material().map(bits3), Some(bits3([0.0, 0.0, -1.0])));
}

#[test]
fn arc_extremes_are_kept_only_inside_the_arc_range() {
    // Upper half circle [0, pi]: u in [-1, 1] (ends), v in [0, 1] (peak at pi/2, the low
    // extreme 3 pi / 2 lies outside the arc). Hand-labelled.
    let body = arc_face(0.0, PI, straight);
    let e = element(map_face(&body, face_ix(&body, "f")));
    match *e.extent() {
        Extent::Planar { u, v } => {
            same(u[1], 1.0);
            assert!((u[0] + 1.0).abs() <= 1e-15, "{u:?}");
            same(v[0], 0.0);
            same(v[1], 1.0);
        }
        x => panic!("{x:?}"),
    }
    // Right half [-pi/2, pi/2]: the u maximum at t = 0 is one turn above t0; u in [0, 1].
    let body = arc_face(-FRAC_PI_2, FRAC_PI_2, straight);
    match *element(map_face(&body, face_ix(&body, "f"))).extent() {
        Extent::Planar { u, v } => {
            same(u[1], 1.0);
            assert!(u[0].abs() <= 1e-16, "{u:?}");
            same(v[0], -1.0);
            same(v[1], 1.0);
        }
        x => panic!("{x:?}"),
    }
    // Arc [pi/2, pi]: neither the u maximum (0) nor the v minimum (3 pi / 2) is inside.
    let body = arc_face(FRAC_PI_2, PI, straight);
    match *element(map_face(&body, face_ix(&body, "f"))).extent() {
        Extent::Planar { u, v } => {
            assert!(u[1].abs() <= 1e-16 && (u[0] + 1.0).abs() <= 1e-15, "{u:?}");
            assert!(v[0].abs() <= 1e-15 && (v[1] - 1.0).abs() <= 1e-15, "{v:?}");
        }
        x => panic!("{x:?}"),
    }
}

#[test]
fn nurbs_boundary_edge_refuses_the_extent() {
    let body = arc_face(0.0, PI, |a, b| Some(nurbs_segment(a, b)));
    let back = edge_ix(&body, "e.back");
    assert_eq!(map_face(&body, face_ix(&body, "f")), Err(DirectError::NurbsBoundary { edge: back }));
    assert_eq!(map_edge(&body, back), Ok(DirectResult::NotAnalytic));
}

#[test]
fn edge_without_curve_is_refused() {
    let body = arc_face(0.0, PI, |_, _| None);
    let back = edge_ix(&body, "e.back");
    assert_eq!(map_face(&body, face_ix(&body, "f")), Err(DirectError::EdgeWithoutCurve { edge: back }));
    assert_eq!(map_edge(&body, back), Err(DirectError::EdgeWithoutCurve { edge: back }));
    assert_eq!(DirectError::EdgeWithoutCurve { edge: back }.code(), "EdgeWithoutCurve");
}

#[test]
fn faces_without_loops_are_unbounded_or_not_defined() {
    let mut bb = BodyBuilder::new([3; 16]);
    let p = bb.add_face(plane(), Sense::Same, vec![], Some("f.plane"));
    let cyl = SurfaceDef::analytic(SurfaceParams::Cylinder { frame: Z, radius: 0.5 }).unwrap();
    let c = bb.add_face(cyl, Sense::Reversed, vec![], Some("f.cyl"));
    let sph = SurfaceDef::analytic(SurfaceParams::Sphere { frame: Z, radius: 0.5 }).unwrap();
    let s = bb.add_face(sph, Sense::Same, vec![], Some("f.sphere"));
    bb.add_shell(vec![p, c, s], false);
    let body = bb.build().unwrap();
    let get = |l: &str| element(map_face(&body, face_ix(&body, l)));
    assert_eq!(get("f.plane").extent(), &Extent::Unbounded);
    assert_eq!(get("f.cyl").extent(), &Extent::Unbounded);
    assert_eq!(get("f.cyl").material(), Some(Material::Inside));
    assert_eq!(get("f.sphere").extent(), &Extent::NotDefined);
    assert_eq!(get("f.sphere").material(), Some(Material::Outside));
}

#[test]
fn an_index_from_another_body_is_refused() {
    let big = block();
    let small = arc_face(0.0, PI, straight);
    let labels = ["f.box.top", "f.box.bottom", "f.hole"];
    let f = labels.iter().map(|l| face_ix(&big, l)).find(|f| f.index() >= small.faces().len()).unwrap();
    let e = edge_ix(&big, "e.hole.bottom");
    let v = vertex_ix(&big, "v.corner.111");
    assert_eq!(map_face(&small, f), Err(DirectError::UnknownEntity { kind: EntityKind::Face, index: f.index() }));
    if e.index() >= small.edges().len() {
        assert_eq!(map_edge(&small, e).unwrap_err().code(), "UnknownEntity");
    }
    if v.index() >= small.vertices().len() {
        assert_eq!(map_vertex(&small, v).unwrap_err().code(), "UnknownEntity");
    }
}

// ---------------------------------------------------------------- sides of the boundary

#[test]
fn cone_tip_extent_reaches_the_apex() {
    // Oracle case: a drill point whose only loop is its rim circle; the apex is on no entity.
    let sc = common::load("../nominal/cone_tip.json");
    let exp = common::load("../nominal/cone_tip_expected.json");
    let body = common::builder(&sc).build().unwrap();
    let e = element(map_face(&body, face_ix(&body, "f.tip")));
    let want = exp.get("faces").get("f.tip");
    check_surface_params(&e, sc.get("faces").arr()[0].get("surface"));
    check_extent(e.extent(), want.get("extent"));
    assert_eq!(e.material(), Some(material(want.get("material").str())));
    assert_eq!(e.extent(), &Extent::Axial { h_min: 0.0, h_max: 0.015625 });
}

/// One cone or cylinder face bounded only by circles at the given heights (one loop each).
fn circles_face(surface: SurfaceParams, heights: &[(f64, f64)]) -> Body {
    let mut bb = BodyBuilder::new([4; 16]);
    let mut loops = Vec::new();
    for (i, &(z, r)) in heights.iter().enumerate() {
        let v = bb.add_vertex([r, 0.0, z], Some(&format!("v{i}")));
        let frame = Frame { origin: [0.0, 0.0, z], axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] };
        let c = CurveDef::analytic(CurveParams::Circle { frame, radius: r }).unwrap();
        let e = bb.add_edge(Some(c), v, v, [0.0, TAU], Some(&format!("e{i}")));
        let co = bb.add_coedge(e, Sense::Same, None, None);
        loops.push(bb.add_loop(vec![co], i == 0, None));
    }
    let f = bb.add_face(SurfaceDef::analytic(surface).unwrap(), Sense::Same, loops, Some("f"));
    bb.add_shell(vec![f], false);
    bb.build().unwrap()
}

#[test]
fn cone_tip_with_offset_origin_takes_the_computed_apex() {
    // r0 = 0.01 at z = 0, half-angle pi/4: apex at z = -0.01 / tan(pi/4) ~ -0.01; rim at 0.005.
    let cone = SurfaceParams::Cone { frame: Z, radius: 0.01, half_angle: core::f64::consts::FRAC_PI_4 };
    let body = circles_face(cone, &[(0.005, 0.015)]);
    match *element(map_face(&body, face_ix(&body, "f"))).extent() {
        Extent::Axial { h_min, h_max } => {
            assert!((h_min + 0.01).abs() <= 1e-17, "{h_min}");
            same(h_max, 0.005);
        }
        x => panic!("{x:?}"),
    }
    // Two circles: a band; the apex below it is not in the face.
    let body = circles_face(cone, &[(0.005, 0.015), (0.0025, 0.0125)]);
    assert_eq!(element(map_face(&body, face_ix(&body, "f"))).extent(), &Extent::Axial { h_min: 0.0025, h_max: 0.005 });
}

#[test]
fn cylinder_needs_a_band_of_two_circle_heights() {
    let cyl = SurfaceParams::Cylinder { frame: Z, radius: 0.5 };
    let one = circles_face(cyl, &[(0.0, 0.5)]);
    assert_eq!(map_face(&one, face_ix(&one, "f")), Err(DirectError::NotABand));
    let two = circles_face(cyl, &[(0.0, 0.5), (0.25, 0.5)]);
    assert_eq!(element(map_face(&two, face_ix(&two, "f"))).extent(), &Extent::Axial { h_min: 0.0, h_max: 0.25 });
    let three = circles_face(cyl, &[(0.0, 0.5), (0.25, 0.5), (0.5, 0.5)]);
    assert_eq!(map_face(&three, face_ix(&three, "f")), Err(DirectError::NotABand));
    // An off-axis circle does not wrap the cylinder.
    let mut bb = BodyBuilder::new([5; 16]);
    let v = bb.add_vertex([0.6, 0.0, 0.0], Some("v"));
    let frame = Frame { origin: [0.1, 0.0, 0.0], axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] };
    let e = bb.add_edge(
        Some(CurveDef::analytic(CurveParams::Circle { frame, radius: 0.5 }).unwrap()),
        v,
        v,
        [0.0, TAU],
        None,
    );
    let c = bb.add_coedge(e, Sense::Same, None, None);
    let l = bb.add_loop(vec![c], true, None);
    let f = bb.add_face(SurfaceDef::analytic(cyl).unwrap(), Sense::Same, vec![l], Some("f"));
    bb.add_shell(vec![f], false);
    let body = bb.build().unwrap();
    assert_eq!(map_face(&body, face_ix(&body, "f")), Err(DirectError::NotABand));
}

#[test]
fn cylinder_window_patch_is_not_a_band() {
    // Half cylinder r = 1, z in [0, 1]: two half-circle arcs and two lines.
    let cyl = SurfaceParams::Cylinder { frame: Z, radius: 1.0 };
    let mut bb = BodyBuilder::new([6; 16]);
    let p = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [-1.0, 0.0, 1.0], [1.0, 0.0, 1.0]];
    let v: Vec<usize> = p.iter().enumerate().map(|(i, q)| bb.add_vertex(*q, Some(&format!("v{i}")))).collect();
    let arc = |z: f64| {
        let frame = Frame { origin: [0.0, 0.0, z], axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] };
        Some(CurveDef::analytic(CurveParams::Circle { frame, radius: 1.0 }).unwrap())
    };
    let e0 = bb.add_edge(arc(0.0), v[0], v[1], [0.0, PI], None);
    let up = |x: f64| {
        let frame = Frame { origin: [x, 0.0, 0.0], axis: [1.0, 0.0, 0.0], ref_dir: [0.0, 0.0, 1.0] };
        Some(CurveDef::analytic(CurveParams::Line { frame }).unwrap())
    };
    let e1 = bb.add_edge(up(-1.0), v[1], v[2], [0.0, 1.0], None);
    let e2 = bb.add_edge(arc(1.0), v[3], v[2], [0.0, PI], None);
    let e3 = bb.add_edge(up(1.0), v[0], v[3], [0.0, 1.0], None);
    let cs = vec![
        bb.add_coedge(e0, Sense::Same, None, None),
        bb.add_coedge(e1, Sense::Same, None, None),
        bb.add_coedge(e2, Sense::Reversed, None, None),
        bb.add_coedge(e3, Sense::Reversed, None, None),
    ];
    let l = bb.add_loop(cs, true, None);
    let f = bb.add_face(SurfaceDef::analytic(cyl).unwrap(), Sense::Same, vec![l], Some("f"));
    bb.add_shell(vec![f], false);
    let body = bb.build().unwrap();
    assert_eq!(map_face(&body, face_ix(&body, "f")), Err(DirectError::NotABand));
    assert_eq!(DirectError::NotABand.code(), "NotABand");
}

#[test]
fn plane_with_holes_but_no_outer_loop_is_refused() {
    let mut bb = BodyBuilder::new([8; 16]);
    let v = bb.add_vertex([1.0, 0.0, 0.0], Some("v"));
    let e = bb.add_edge(Some(unit_circle()), v, v, [0.0, TAU], None);
    let c = bb.add_coedge(e, Sense::Reversed, None, None);
    let l = bb.add_loop(vec![c], false, None);
    let f = bb.add_face(plane(), Sense::Same, vec![l], Some("f"));
    bb.add_shell(vec![f], false);
    let body = bb.build().unwrap();
    assert_eq!(map_face(&body, face_ix(&body, "f")), Err(DirectError::NoOuterLoop));
}

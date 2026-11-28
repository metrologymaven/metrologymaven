//! IO.CAD.EXPORT.BOUND acceptance tests. Expected values: `fixtures/io/cad/export/bound/
//! plane_points.json` (AC-1) and `cases.json` (AC-2 to AC-4 and the refusals), written by the
//! independent stdlib script `make_cases.py` in the same folder, compared bit for bit.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use fid_canon::CValue;
use fid_geo_frame::{Frame, FrameId, FrameKind, FrameTree, Rigid, Similarity, Transform};
use fid_geo_nurbs::{
    Circle, Cone, Cylinder, Ellipse, Line, NurbsCurve, NurbsCurveParts, NurbsSurface, NurbsSurfaceParts, Placement,
    Plane, Sphere, Torus,
};
use fid_io_cad_export::{
    plan, Bounds, ElementId, ElementInput, Extent, ExtentFault, ExtentSource, Geometry, NameFault, PlanError,
    PointsUnused, UntrustedName, MAX_NAME_BYTES,
};

// ---------------------------------------------------------------- fixture reading

fn fixture(name: &str) -> CValue {
    let p = format!("{}/../../fixtures/io/cad/export/bound/{name}", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(!bytes.contains(&b'\r'), "{name}: LF line endings expected");
    fid_canon::parse(&bytes).unwrap()
}

fn get<'a>(v: &'a CValue, key: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(key).unwrap_or_else(|| panic!("missing key {key}")),
        _ => panic!("not an object at {key}"),
    }
}

fn opt<'a>(v: &'a CValue, key: &str) -> Option<&'a CValue> {
    match v {
        CValue::Obj(m) => match m.get(key) {
            None | Some(CValue::Null) => None,
            Some(x) => Some(x),
        },
        _ => panic!("not an object at {key}"),
    }
}

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(f) => *f,
        other => panic!("not a number: {other:?}"),
    }
}

fn text(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("not a string: {other:?}"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("not an array: {other:?}"),
    }
}

fn v3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    assert_eq!(a.len(), 3);
    [num(&a[0]), num(&a[1]), num(&a[2])]
}

fn m3(v: &CValue) -> [[f64; 3]; 3] {
    let a = arr(v);
    [v3(&a[0]), v3(&a[1]), v3(&a[2])]
}

fn frame_id(v: &CValue) -> FrameId {
    FrameId::parse(text(v)).unwrap()
}

fn element_id(v: &CValue) -> ElementId {
    ElementId::parse(text(v)).unwrap()
}

fn tree(doc: &CValue) -> FrameTree {
    let mut t = FrameTree::new();
    for f in arr(get(doc, "frames")) {
        let rigid = Rigid::new(m3(get(f, "rotation")), v3(get(f, "translation"))).unwrap();
        let to_parent = match opt(f, "scale") {
            None => Transform::Rigid(rigid),
            Some(s) => Transform::Similarity(Similarity::new(rigid, num(s)).unwrap()),
        };
        t.insert(Frame {
            id: frame_id(get(f, "id")),
            name: text(get(f, "name")).to_owned(),
            kind: FrameKind::User,
            parent: Some(frame_id(get(f, "parent"))),
            to_parent,
        })
        .unwrap();
    }
    t
}

fn placement(g: &CValue) -> Placement {
    Placement::new(v3(get(g, "origin")), v3(get(g, "axis")), v3(get(g, "ref_dir"))).unwrap()
}

fn geometry(g: &CValue) -> Geometry {
    let r = |k: &str| num(get(g, k));
    match text(get(g, "kind")) {
        "point" => Geometry::Point(v3(get(g, "point"))),
        "line" => Geometry::Line(Line::new(placement(g))),
        "plane" => Geometry::Plane(Plane::new(placement(g))),
        "cylinder" => Geometry::Cylinder(Cylinder::new(placement(g), r("radius")).unwrap()),
        "cone" => Geometry::Cone(Cone::new(placement(g), r("radius"), r("half_angle")).unwrap()),
        "circle" => Geometry::Circle(Circle::new(placement(g), r("radius")).unwrap()),
        "ellipse" => Geometry::Ellipse(Ellipse::new(placement(g), r("semi_axis_1"), r("semi_axis_2")).unwrap()),
        "sphere" => Geometry::Sphere(Sphere::new(placement(g), r("radius")).unwrap()),
        "torus" => Geometry::Torus(Torus::new(placement(g), r("major_radius"), r("minor_radius")).unwrap()),
        "nurbs_curve" => Geometry::NurbsCurve(
            NurbsCurve::new(NurbsCurveParts {
                degree: num(get(g, "degree")) as usize,
                knots: arr(get(g, "knots")).iter().map(num).collect(),
                ctrl: arr(get(g, "ctrl")).iter().map(v3).collect(),
                weights: None,
                periodic: false,
            })
            .unwrap(),
        ),
        "nurbs_surface" => Geometry::NurbsSurface(
            NurbsSurface::new(NurbsSurfaceParts {
                degree_u: num(get(g, "degree_u")) as usize,
                degree_v: num(get(g, "degree_v")) as usize,
                knots_u: arr(get(g, "knots_u")).iter().map(num).collect(),
                knots_v: arr(get(g, "knots_v")).iter().map(num).collect(),
                n_u: num(get(g, "n_u")) as usize,
                n_v: num(get(g, "n_v")) as usize,
                ctrl: arr(get(g, "ctrl")).iter().map(v3).collect(),
                weights: None,
                periodic_u: false,
                periodic_v: false,
            })
            .unwrap(),
        ),
        other => panic!("unknown kind {other}"),
    }
}

fn extent(x: &CValue) -> Extent {
    let v: Vec<f64> = arr(get(x, "values")).iter().map(num).collect();
    match text(get(x, "kind")) {
        "line" => Extent::Line(v[0], v[1]),
        "rect" => Extent::Rect(v[0], v[1], v[2], v[3]),
        "axial" => Extent::Axial(v[0], v[1]),
        other => panic!("unknown extent {other}"),
    }
}

struct Owned {
    id: ElementId,
    name: String,
    frame: FrameId,
    geom: Geometry,
    points: Option<Vec<[f64; 3]>>,
    extent: Option<Extent>,
}

fn owned(e: &CValue) -> Owned {
    Owned {
        id: element_id(get(e, "id")),
        name: text(get(e, "name")).to_owned(),
        frame: frame_id(get(e, "frame")),
        geom: geometry(get(e, "geom")),
        points: opt(e, "points").map(|p| arr(p).iter().map(v3).collect()),
        extent: opt(e, "extent").map(extent),
    }
}

fn inputs(els: &[Owned]) -> Vec<ElementInput<'_>> {
    els.iter()
        .map(|o| ElementInput {
            element: o.id,
            name: &o.name,
            frame: o.frame,
            geom: &o.geom,
            points: o.points.as_deref(),
            extent: o.extent,
        })
        .collect()
}

// ---------------------------------------------------------------- comparison

fn same_bits(case: &str, what: &str, got: f64, want: f64) {
    assert_eq!(got.to_bits(), want.to_bits(), "{case}: {what}: got {got:e}, want {want:e}");
}

fn same3(case: &str, what: &str, got: [f64; 3], want: &CValue) {
    let want = v3(want);
    for i in 0..3 {
        same_bits(case, &format!("{what}[{i}]"), got[i], want[i]);
    }
}

fn check_placement(case: &str, pl: &Placement, want: &CValue) {
    same3(case, "origin", pl.origin(), get(want, "origin"));
    same3(case, "axis", pl.axis(), get(want, "axis"));
    same3(case, "ref_dir", pl.ref_dir(), get(want, "ref_dir"));
}

fn check_geom(case: &str, got: &Geometry, want: &CValue) {
    assert_eq!(got.kind().as_str(), text(get(want, "kind")), "{case}: kind");
    let w = |k: &str| num(get(want, k));
    match got {
        Geometry::Point(p) => same3(case, "point", *p, get(want, "point")),
        Geometry::Line(l) => check_placement(case, l.placement(), want),
        Geometry::Plane(p) => check_placement(case, p.placement(), want),
        Geometry::Cylinder(c) => {
            check_placement(case, c.placement(), want);
            same_bits(case, "radius", c.radius(), w("radius"));
        }
        Geometry::Cone(c) => {
            check_placement(case, c.placement(), want);
            same_bits(case, "radius", c.radius(), w("radius"));
            same_bits(case, "half_angle", c.half_angle(), w("half_angle"));
        }
        Geometry::Circle(c) => {
            check_placement(case, c.placement(), want);
            same_bits(case, "radius", c.radius(), w("radius"));
        }
        Geometry::Ellipse(e) => {
            check_placement(case, e.placement(), want);
            let (a, b) = e.semi_axes();
            same_bits(case, "semi_axis_1", a, w("semi_axis_1"));
            same_bits(case, "semi_axis_2", b, w("semi_axis_2"));
        }
        Geometry::Sphere(c) => {
            check_placement(case, c.placement(), want);
            same_bits(case, "radius", c.radius(), w("radius"));
        }
        Geometry::Torus(c) => {
            check_placement(case, c.placement(), want);
            let (major, minor) = c.radii();
            same_bits(case, "major_radius", major, w("major_radius"));
            same_bits(case, "minor_radius", minor, w("minor_radius"));
        }
        Geometry::NurbsCurve(c) => check_ctrl(case, c.ctrl(), want),
        Geometry::NurbsSurface(c) => check_ctrl(case, c.ctrl(), want),
    }
}

fn check_ctrl(case: &str, ctrl: &[[f64; 3]], want: &CValue) {
    let want_ctrl = arr(get(want, "ctrl"));
    assert_eq!(ctrl.len(), want_ctrl.len(), "{case}: ctrl count");
    for (i, (p, q)) in ctrl.iter().zip(want_ctrl).enumerate() {
        same3(case, &format!("ctrl[{i}]"), *p, q);
    }
}

fn check_bounds(case: &str, got: Bounds, want: &CValue) {
    if let CValue::Str(s) = want {
        assert_eq!(s, "intrinsic", "{case}");
        assert_eq!(got, Bounds::Intrinsic, "{case}");
        return;
    }
    let Bounds::Extent { extent, source } = got else {
        panic!("{case}: expected an extent, got {got:?}");
    };
    assert_eq!(extent.kind().as_str(), text(get(want, "extent")), "{case}: extent kind");
    let values: Vec<f64> = extent.intervals().iter().flat_map(|(a, b)| [*a, *b]).collect();
    let want_values: Vec<f64> = arr(get(want, "values")).iter().map(num).collect();
    assert_eq!(values.len(), want_values.len(), "{case}");
    for (i, (g, w)) in values.iter().zip(&want_values).enumerate() {
        same_bits(case, &format!("extent[{i}]"), *g, *w);
    }
    assert_eq!(source.code(), text(get(want, "source")), "{case}: source");
    if let ExtentSource::FittedPoints { points, margin_m } = source {
        assert_eq!(points as f64, num(get(want, "points")), "{case}: points");
        same_bits(case, "margin", margin_m, num(get(want, "margin")));
    }
}

fn detail_str(e: &PlanError, key: &str) -> Option<String> {
    Some(match (e, key) {
        (PlanError::DegenerateExtent { axis, .. }, "axis") => (*axis).to_owned(),
        (PlanError::BadExtent { fault: ExtentFault::Empty { axis }, .. }, "axis") => (*axis).to_owned(),
        (PlanError::BadExtent { fault: ExtentFault::Empty { .. }, .. }, "fault") => "empty".to_owned(),
        (PlanError::FrameNotFound { frame, .. }, "frame") => frame.to_string(),
        (PlanError::BadName { fault, .. }, "fault") => fault.code().to_owned(),
        (PlanError::ExtentNotApplicable { kind, .. }, "kind") => kind.as_str().to_owned(),
        (PlanError::ExtentKindMismatch { expected, .. }, "expected") => expected.as_str().to_owned(),
        (PlanError::ExtentKindMismatch { found, .. }, "found") => found.as_str().to_owned(),
        (PlanError::PointsNotApplicable { reason, .. }, "reason") => match reason {
            PointsUnused::Bounded => "bounded".to_owned(),
            PointsUnused::CallerExtent => "caller_extent".to_owned(),
        },
        _ => return None,
    })
}

fn detail_num(e: &PlanError, key: &str) -> Option<f64> {
    Some(match (e, key) {
        (PlanError::DuplicateElement { first, .. }, "first") => *first as f64,
        (PlanError::DuplicateElement { index, .. }, "index") => *index as f64,
        (PlanError::BadName { fault: NameFault::ForbiddenChar { offset }, .. }, "offset") => *offset as f64,
        _ => return None,
    })
}

fn check_error(case: &str, got: &PlanError, want: &CValue) {
    assert_eq!(got.code(), text(get(want, "code")), "{case}: {got}");
    assert_eq!(got.element(), opt(want, "element").map(element_id), "{case}: element");
    let CValue::Obj(m) = want else { panic!() };
    for (key, v) in m {
        if key == "code" || key == "element" {
            continue;
        }
        match v {
            CValue::Str(s) => assert_eq!(detail_str(got, key).as_deref(), Some(s.as_str()), "{case}: {key}"),
            CValue::Num(n) => assert_eq!(detail_num(got, key), Some(*n), "{case}: {key}"),
            other => panic!("{case}: detail {key} {other:?}"),
        }
    }
}

/// Runs every case of a fixture document; returns how many cases and items were checked.
fn run_document(name: &str) -> (usize, usize) {
    let doc = fixture(name);
    assert_eq!(text(get(&doc, "format")), "fid.test.io.cad.export.bound/1");
    let t = tree(&doc);
    let mut items_checked = 0;
    let cases = arr(get(&doc, "cases"));
    for case in cases {
        let cname = text(get(case, "name"));
        let els: Vec<Owned> = arr(get(case, "elements")).iter().map(owned).collect();
        let ins = inputs(&els);
        let result = plan(&t, frame_id(get(case, "target")), &ins, num(get(case, "margin")));
        let expect = get(case, "expect");
        match (result, opt(expect, "items"), opt(expect, "error")) {
            (Ok(p), Some(items), None) => {
                let items = arr(items);
                assert_eq!(p.items().len(), items.len(), "{cname}: item count");
                assert_eq!(p.frame(), frame_id(get(case, "target")), "{cname}: plan frame");
                for ((got, want), o) in p.items().iter().zip(items).zip(&els) {
                    assert_eq!(got.element(), element_id(get(want, "id")), "{cname}: order");
                    assert_eq!(got.name().as_untrusted_str(), o.name, "{cname}: name");
                    assert_eq!(got.source_frame(), o.frame, "{cname}: source frame");
                    check_geom(cname, got.geom(), get(want, "geom"));
                    check_bounds(cname, got.bounds(), get(want, "bounds"));
                    items_checked += 1;
                }
            }
            (Err(e), None, Some(want)) => check_error(cname, &e, want),
            (got, _, _) => panic!("{cname}: unexpected {got:?}"),
        }
    }
    (cases.len(), items_checked)
}

// ---------------------------------------------------------------- acceptance criteria

/// AC-1: a fitted plane with 4 points spanning 10 x 20 mm, no extent: [0, 0.01] x [0, 0.02] m
/// from `fitted_points`.
#[test]
fn ac1_plane_points_fixture() {
    let (cases, items) = run_document("plane_points.json");
    assert_eq!((cases, items), (1, 1));
    let doc = fixture("plane_points.json");
    let case = &arr(get(&doc, "cases"))[0];
    let els: Vec<Owned> = arr(get(case, "elements")).iter().map(owned).collect();
    let p = plan(&FrameTree::new(), FrameId::WORLD, &inputs(&els), 0.0).unwrap();
    let item = &p.items()[0];
    assert_eq!(item.extent(), Some(Extent::Rect(0.0, 0.01, 0.0, 0.02)));
    assert_eq!(item.extent_source(), Some(ExtentSource::FittedPoints { points: 4, margin_m: 0.0 }));
    assert_eq!(item.extent_source().unwrap().code(), "fitted_points");
}

/// AC-2 to AC-4 and every refusal, bit for bit against the oracle.
#[test]
fn oracle_cases() {
    let (cases, items) = run_document("cases.json");
    // Counted so a truncated fixture cannot pass silently.
    assert_eq!(cases, 29);
    assert_eq!(items, 16);
}

fn frames_doc() -> FrameTree {
    tree(&fixture("cases.json"))
}

fn case_inputs(name: &str) -> (CValue, Vec<Owned>) {
    let doc = fixture("cases.json");
    let case = arr(get(&doc, "cases")).iter().find(|c| text(get(c, "name")) == name).unwrap().clone();
    let els = arr(get(&case, "elements")).iter().map(owned).collect();
    (case, els)
}

/// AC-2: a cylinder with a caller axial extent [-5, 15] mm keeps it (source `caller`), here
/// after a rigid two-level move into `world`.
#[test]
fn ac2_cylinder_caller_extent() {
    let (_, els) = case_inputs("ac2_cylinder_caller");
    let p = plan(&frames_doc(), FrameId::WORLD, &inputs(&els), 0.0).unwrap();
    assert_eq!(p.items()[0].extent(), Some(Extent::Axial(-0.005, 0.015)));
    assert_eq!(p.items()[0].extent_source(), Some(ExtentSource::Caller));
}

/// AC-3: a constructed line with no points and no extent is `extent_required{element}`.
#[test]
fn ac3_line_without_points_or_extent() {
    let (_, els) = case_inputs("ac3_line_no_points");
    let e = plan(&FrameTree::new(), FrameId::WORLD, &inputs(&els), 0.0).unwrap_err();
    assert_eq!(e, PlanError::ExtentRequired { element: els[0].id });
    assert_eq!(e.code(), "extent_required");
}

/// AC-4: a circle in a frame translated by (1, 0, 0) m, target `world`: the centre moves by
/// exactly 1 m.
#[test]
fn ac4_circle_shifted_exactly() {
    let (_, els) = case_inputs("ac4_circle_shift");
    let p = plan(&frames_doc(), FrameId::WORLD, &inputs(&els), 0.0).unwrap();
    let Geometry::Circle(c) = p.items()[0].geom() else { panic!() };
    let Geometry::Circle(src) = &els[0].geom else { panic!() };
    let before = src.placement().origin();
    let after = c.placement().origin();
    assert_eq!(after, [before[0] + 1.0, before[1], before[2]]);
    assert_eq!(after, [1.25, -0.5, 0.125]);
    assert_eq!(c.radius(), src.radius());
    assert_eq!(p.items()[0].bounds(), Bounds::Intrinsic);
}

// ---------------------------------------------------------------- REQ-004 and fail-closed edges

fn uuid(n: u128) -> ElementId {
    ElementId::from_u128(n)
}

fn xy_plane() -> Geometry {
    Geometry::Plane(Plane::new(Placement::new([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]).unwrap()))
}

fn input<'a>(geom: &'a Geometry, points: Option<&'a [[f64; 3]]>, extent: Option<Extent>) -> ElementInput<'a> {
    ElementInput { element: uuid(1), name: "E-1", frame: FrameId::WORLD, geom, points, extent }
}

/// REQ-004: the moved placement equals the rotation applied to the input within 1e-15 (the
/// renormalisation moves a unit vector by ulps, far below 1e-12).
#[test]
fn req4_parameters_kept_under_rotation() {
    let t = frames_doc();
    let (_, els) = case_inputs("bounded_kinds_rotated");
    let p = plan(&t, FrameId::WORLD, &inputs(&els), 0.0).unwrap();
    let mut checked = 0;
    for (item, o) in p.items().iter().zip(&els) {
        let xf = t.resolve(o.frame, FrameId::WORLD).unwrap();
        let (Some(a), Some(b)) = (placement_of(item.geom()), placement_of(&o.geom)) else {
            continue;
        };
        let near = |x: [f64; 3], y: [f64; 3], tol: f64| (0..3).all(|i| (x[i] - y[i]).abs() <= tol);
        assert!(near(a.axis(), xf.apply_dir(&b.axis()).unwrap(), 1e-15));
        assert!(near(a.ref_dir(), xf.apply_dir(&b.ref_dir()).unwrap(), 1e-15));
        assert_eq!(a.origin(), xf.apply_point(&b.origin()).unwrap());
        checked += 1;
    }
    assert_eq!(checked, 4);
}

fn placement_of(g: &Geometry) -> Option<Placement> {
    match g {
        Geometry::Line(x) => Some(*x.placement()),
        Geometry::Plane(x) => Some(*x.placement()),
        Geometry::Cylinder(x) => Some(*x.placement()),
        Geometry::Cone(x) => Some(*x.placement()),
        Geometry::Circle(x) => Some(*x.placement()),
        Geometry::Ellipse(x) => Some(*x.placement()),
        Geometry::Sphere(x) => Some(*x.placement()),
        Geometry::Torus(x) => Some(*x.placement()),
        _ => None,
    }
}

#[test]
fn non_finite_point_is_refused_with_its_index() {
    let g = xy_plane();
    let pts = [[0.0, 0.0, 0.0], [0.01, 0.0, 0.0], [f64::NAN, 0.0, 0.0], [f64::INFINITY, 0.0, 0.0]];
    let e = plan(&FrameTree::new(), FrameId::WORLD, &[input(&g, Some(&pts), None)], 0.0).unwrap_err();
    assert_eq!(e, PlanError::NonFinitePoint { element: uuid(1), index: 2 });
}

#[test]
fn non_finite_caller_extent_and_margin_are_refused() {
    let g = xy_plane();
    let x = Extent::Rect(0.0, f64::INFINITY, 0.0, 0.01);
    let e = plan(&FrameTree::new(), FrameId::WORLD, &[input(&g, None, Some(x))], 0.0).unwrap_err();
    assert_eq!(e, PlanError::BadExtent { element: uuid(1), fault: ExtentFault::NonFinite });
    let x = Extent::Rect(0.0, 0.01, f64::NAN, 0.01);
    let e = plan(&FrameTree::new(), FrameId::WORLD, &[input(&g, None, Some(x))], 0.0).unwrap_err();
    assert_eq!(e.code(), "bad_extent");
    for m in [f64::NAN, f64::INFINITY, -1e-300] {
        let e = plan(&FrameTree::new(), FrameId::WORLD, &[input(&g, None, Some(Extent::Rect(0.0, 1.0, 0.0, 1.0)))], m)
            .unwrap_err();
        assert_eq!(e.code(), "bad_margin");
    }
}

#[test]
fn fitted_extent_overflow_is_refused() {
    let g = xy_plane();
    let pts = [[f64::MAX, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let e = plan(&FrameTree::new(), FrameId::WORLD, &[input(&g, Some(&pts), None)], f64::MAX).unwrap_err();
    assert_eq!(e, PlanError::BadExtent { element: uuid(1), fault: ExtentFault::NonFinite });
}

/// Zeros are reported as +0.0, so an extent has one representation.
#[test]
fn negative_zero_is_reported_as_positive_zero() {
    let g = xy_plane();
    let x = Extent::Rect(-0.0, 0.01, -0.0, 0.02);
    let p = plan(&FrameTree::new(), FrameId::WORLD, &[input(&g, None, Some(x))], -0.0).unwrap();
    let Some(Extent::Rect(u0, _, v0, _)) = p.items()[0].extent() else { panic!() };
    assert_eq!((u0.to_bits(), v0.to_bits()), (0, 0));
    assert_eq!(p.margin_m().to_bits(), 0);
    let pts = [[-0.0, -0.0, 0.0], [0.01, 0.02, 0.0]];
    let p = plan(&FrameTree::new(), FrameId::WORLD, &[input(&g, Some(&pts), None)], 0.0).unwrap();
    let Some(Extent::Rect(u0, _, v0, _)) = p.items()[0].extent() else { panic!() };
    assert_eq!((u0.to_bits(), v0.to_bits()), (0, 0));
}

#[test]
fn similarity_scales_radius_and_extent() {
    let (_, els) = case_inputs("similarity_cylinder");
    let p = plan(&frames_doc(), FrameId::WORLD, &inputs(&els), 0.0).unwrap();
    let Geometry::Cylinder(c) = p.items()[0].geom() else { panic!() };
    assert_eq!(c.radius(), 1.5 * 0.004);
    let Some(Extent::Axial(h0, h1)) = p.items()[0].extent() else { panic!() };
    assert_eq!((h0, h1), (1.5 * -0.003, 1.5 * 0.012));
}

#[test]
fn names_are_validated() {
    assert_eq!(UntrustedName::new(""), Err(NameFault::Empty));
    let long = "a".repeat(MAX_NAME_BYTES + 1);
    assert_eq!(UntrustedName::new(&long), Err(NameFault::TooLong { bytes: MAX_NAME_BYTES + 1 }));
    assert!(UntrustedName::new(&"a".repeat(MAX_NAME_BYTES)).is_ok());
    assert_eq!(UntrustedName::new("ab\u{202e}c"), Err(NameFault::ForbiddenChar { offset: 2 }));
    assert_eq!(UntrustedName::new("x\u{feff}"), Err(NameFault::ForbiddenChar { offset: 1 }));
    assert_eq!(UntrustedName::new("é\n"), Err(NameFault::ForbiddenChar { offset: 2 }));
    let n = UntrustedName::new("Bohrung Ø10").unwrap();
    assert_eq!(n.as_untrusted_str(), "Bohrung Ø10");
    assert!(!n.is_empty());
}

#[test]
fn error_codes_and_display_are_stable() {
    let e = uuid(7);
    let cases: Vec<(PlanError, &str)> = vec![
        (PlanError::EmptySelection, "empty_selection"),
        (PlanError::TooManyElements { count: 2 }, "too_many_elements"),
        (PlanError::BadMargin { value: -1.0 }, "bad_margin"),
        (PlanError::FrameNotFound { element: None, frame: FrameId::WORLD }, "frame_not_found"),
        (PlanError::FrameNotFound { element: Some(e), frame: FrameId::WORLD }, "frame_not_found"),
        (PlanError::ExtentRequired { element: e }, "extent_required"),
        (PlanError::EmptyPoints { element: e }, "empty_points"),
        (PlanError::ExtentCrossesApex { element: e }, "extent_crosses_apex"),
        (PlanError::DegenerateExtent { element: e, axis: "u" }, "degenerate_extent"),
        (PlanError::PointsNotApplicable { element: e, reason: PointsUnused::Bounded }, "points_not_applicable"),
        (PlanError::AllocationFailed, "allocation_failed"),
    ];
    for (err, code) in cases {
        assert_eq!(err.code(), code);
        assert!(!err.to_string().is_empty());
    }
    assert_eq!(PlanError::ExtentRequired { element: e }.element(), Some(e));
    assert_eq!(PlanError::EmptySelection.element(), None);
    assert_eq!(PlanError::FrameNotFound { element: Some(e), frame: FrameId::WORLD }.element(), Some(e));
    assert_eq!(PlanError::FrameNotFound { element: None, frame: FrameId::WORLD }.element(), None);
}

/// Distinct elements sharing a frame resolve it once and plan in selection order; the input
/// is left untouched.
#[test]
fn selection_order_is_kept() {
    let t = frames_doc();
    let rot = FrameId::parse("10000000000000000000000000000002").unwrap();
    let geoms: Vec<Geometry> = (0..5).map(|i| Geometry::Point([f64::from(i) * 0.001, 0.0, 0.0])).collect();
    let names: Vec<String> = (0..5).map(|i| format!("P-{i}")).collect();
    let order = [4u128, 2, 9, 1, 3];
    let ins: Vec<ElementInput<'_>> = geoms
        .iter()
        .zip(&names)
        .zip(order)
        .map(|((g, n), id)| ElementInput {
            element: uuid(id),
            name: n,
            frame: rot,
            geom: g,
            points: None,
            extent: None,
        })
        .collect();
    let p = plan(&t, FrameId::WORLD, &ins, 0.0).unwrap();
    let got: Vec<ElementId> = p.items().iter().map(|i| i.element()).collect();
    assert_eq!(got, order.map(uuid).to_vec());
    let mut counts: BTreeMap<FrameId, usize> = BTreeMap::new();
    for i in p.items() {
        *counts.entry(i.source_frame()).or_default() += 1;
    }
    assert_eq!(counts.get(&rot), Some(&5));
    assert_eq!(geoms[3], Geometry::Point([0.003, 0.0, 0.0]));
}

/// The margin is added in the target frame: under the scale 1.5 the export is widened by
/// exactly the reported margin (review of #681).
#[test]
fn similarity_margin_is_in_target_units() {
    let (_, els) = case_inputs("similarity_cylinder_margin");
    let p = plan(&frames_doc(), FrameId::WORLD, &inputs(&els), 0.001).unwrap();
    let Some(Extent::Axial(h0, h1)) = p.items()[0].extent() else { panic!() };
    assert_eq!((h0, h1), (1.5 * -0.003 - 0.001, 1.5 * 0.012 + 0.001));
    assert_eq!(p.items()[0].extent_source(), Some(ExtentSource::FittedPoints { points: 3, margin_m: 0.001 }));
}

/// A missing element frame names the element; a missing target frame names none.
#[test]
fn frame_not_found_names_the_element() {
    let g = Geometry::Point([0.0; 3]);
    let missing = FrameId::parse("20000000000000000000000000000009").unwrap();
    let mut e = input(&g, None, None);
    e.frame = missing;
    let err = plan(&FrameTree::new(), FrameId::WORLD, &[e], 0.0).unwrap_err();
    assert_eq!(err, PlanError::FrameNotFound { element: Some(uuid(1)), frame: missing });
    let err = plan(&FrameTree::new(), missing, &[input(&g, None, None)], 0.0).unwrap_err();
    assert_eq!(err, PlanError::FrameNotFound { element: None, frame: missing });
}

/// A scale that rounds both ends of a valid caller interval together gives a zero-width
/// target extent, refused as `degenerate_extent` (review of #681).
#[test]
fn zero_width_after_scaling_is_degenerate() {
    let tiny = FrameId::parse("30000000000000000000000000000001").unwrap();
    let mut t = FrameTree::new();
    let s = Similarity::new(Rigid::IDENTITY, 1e-310).unwrap();
    t.insert(Frame {
        id: tiny,
        name: "tiny".to_owned(),
        kind: FrameKind::User,
        parent: Some(FrameId::WORLD),
        to_parent: Transform::Similarity(s),
    })
    .unwrap();
    let g = Geometry::Line(Line::new(Placement::new([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]).unwrap()));
    let hi = 1.0 + f64::EPSILON;
    assert_eq!(1e-310 * 1.0, 1e-310 * hi, "the scale must merge the ends");
    let mut e = input(&g, None, Some(Extent::Line(1.0, hi)));
    e.frame = tiny;
    let err = plan(&t, FrameId::WORLD, &[e], 0.0).unwrap_err();
    assert_eq!(err, PlanError::DegenerateExtent { element: uuid(1), axis: "t" });
}

/// An apex exactly at an end of the interval (r0 = 0, extent [0, h]) is accepted; the same
/// cone over [-h, h] holds it strictly inside and is refused.
#[test]
fn apex_at_interval_end_is_accepted() {
    let pl = Placement::new([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]).unwrap();
    let g = Geometry::Cone(Cone::new(pl, 0.0, 0.3).unwrap());
    let p = plan(&FrameTree::new(), FrameId::WORLD, &[input(&g, None, Some(Extent::Axial(0.0, 0.02)))], 0.0).unwrap();
    assert_eq!(p.items()[0].extent(), Some(Extent::Axial(0.0, 0.02)));
    let p = plan(&FrameTree::new(), FrameId::WORLD, &[input(&g, None, Some(Extent::Axial(-0.02, 0.0)))], 0.0).unwrap();
    assert_eq!(p.items()[0].extent(), Some(Extent::Axial(-0.02, 0.0)));
    let err =
        plan(&FrameTree::new(), FrameId::WORLD, &[input(&g, None, Some(Extent::Axial(-0.02, 0.02)))], 0.0).unwrap_err();
    assert_eq!(err, PlanError::ExtentCrossesApex { element: uuid(1) });
}

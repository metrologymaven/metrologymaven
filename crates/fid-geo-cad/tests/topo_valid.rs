//! GEO.CAD.TOPO.VALID acceptance tests. Expected reports come from the independent oracle
//! `fixtures/geo/cad/topo/make_valid_fixtures.py` (closed-form geometry, exact rational loop
//! tests); a few further expectations are hand-labelled from the spec's rules and say so.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{build, builder, builder_with, load, J};
use fid_geo_cad::{
    body_digest, encode, validate, Body, BodyBuilder, CurveDef, CurveParams, Defect, DefectKind, FailReason, Frame,
    PairingFault, Sense, SurfaceDef, SurfaceParams, TopoBuildError, TopologyId, ValidError, ValidationReport,
};

const TOL: f64 = 1e-7;

const DEFECT_FIXTURES: [&str; 8] = [
    "open_loop",
    "self_intersecting_loop",
    "dangling_edge",
    "inconsistent_orientation",
    "edge_off_surface",
    "pcurve_mismatch",
    "pcurve_failed",
    "seam_winding",
];

fn expected() -> J {
    load("valid_expected.json")
}

fn hex(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

fn id(t: &TopologyId) -> String {
    match t {
        TopologyId::Imported(l) => l.as_str().to_string(),
        TopologyId::Derived(_) => t.to_string(),
    }
}

fn q(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn reason(r: FailReason) -> &'static str {
    match r {
        FailReason::NoEdgeCurve => "NoEdgeCurve",
        FailReason::CurveEval => "CurveEval",
        FailReason::SurfaceEval => "SurfaceEval",
        FailReason::Inversion(_) => "Inversion",
        FailReason::Tolerance => "Tolerance",
    }
}

/// The pinned canonical JSON of a report (see the fixture script's docstring).
fn canonical(r: &ValidationReport) -> String {
    let defects: Vec<String> = r
        .defects
        .iter()
        .map(|d| match d {
            Defect::OpenLoop { r#loop, gap_m } => {
                format!("{{\"gap_m\":{},\"kind\":\"OpenLoop\",\"loop\":{}}}", q(&hex(*gap_m)), q(&id(r#loop)))
            }
            Defect::SelfIntersectingLoop { r#loop } => {
                format!("{{\"kind\":\"SelfIntersectingLoop\",\"loop\":{}}}", q(&id(r#loop)))
            }
            Defect::DanglingEdge { edge, uses } => {
                format!("{{\"edge\":{},\"kind\":\"DanglingEdge\",\"uses\":{uses}}}", q(&id(edge)))
            }
            Defect::NonManifoldEdge { edge, uses } => {
                format!("{{\"edge\":{},\"kind\":\"NonManifoldEdge\",\"uses\":{uses}}}", q(&id(edge)))
            }
            Defect::InconsistentOrientation { edge } => {
                format!("{{\"edge\":{},\"kind\":\"InconsistentOrientation\"}}", q(&id(edge)))
            }
            Defect::EdgeOffSurface { edge, face, deviation_m } => format!(
                "{{\"deviation_m\":{},\"edge\":{},\"face\":{},\"kind\":\"EdgeOffSurface\"}}",
                q(&hex(*deviation_m)),
                q(&id(edge)),
                q(&id(face))
            ),
            Defect::PcurveMismatch { coedge, deviation_m } => format!(
                "{{\"coedge\":{},\"deviation_m\":{},\"kind\":\"PcurveMismatch\"}}",
                q(&id(coedge)),
                q(&hex(*deviation_m))
            ),
            Defect::PcurveFailed { coedge, reason: r, best_deviation_m } => format!(
                "{{\"best_deviation_m\":{},\"coedge\":{},\"kind\":\"PcurveFailed\",\"reason\":{}}}",
                q(&hex(*best_deviation_m)),
                q(&id(coedge)),
                q(reason(*r))
            ),
            Defect::SeamWinding { r#loop, periods } => format!(
                "{{\"kind\":\"SeamWinding\",\"loop\":{},\"periods\":[{},{}]}}",
                q(&id(r#loop)),
                periods[0],
                periods[1]
            ),
        })
        .collect();
    format!(
        "{{\"defects\":[{}],\"tolerance_m\":{},\"truncated\":{}}}",
        defects.join(","),
        q(&hex(r.tolerance_m)),
        r.truncated
    )
}

/// The measured value of a defect, if it has one.
fn value(d: &Defect) -> Option<f64> {
    match d {
        Defect::OpenLoop { gap_m, .. } => Some(*gap_m),
        Defect::EdgeOffSurface { deviation_m, .. } | Defect::PcurveMismatch { deviation_m, .. } => Some(*deviation_m),
        Defect::PcurveFailed { best_deviation_m, .. } => Some(*best_deviation_m),
        _ => None,
    }
}

/// Compares a report with the oracle's: kinds, IDs, counts and values (within 1e-12 m, AC-2)
/// and the canonical JSON byte for byte (AC-5).
fn check_report(name: &str, r: &ValidationReport, exp: &J) {
    let rows = exp.get("defects").arr();
    assert_eq!(r.defects.len(), rows.len(), "{name}: defect count");
    for (d, row) in r.defects.iter().zip(rows) {
        assert_eq!(d.kind().code(), row.get("kind").str(), "{name}: kind");
        assert_eq!(id(d.entity()), row.get("entity").str(), "{name}: entity");
        if let Some(face) = row.opt("face") {
            assert_eq!(d.second().map(id).as_deref(), Some(face.str()), "{name}: face");
        }
        match (value(d), row.opt("value")) {
            (Some(v), Some(x)) => {
                let x: f64 = x.str().parse().unwrap();
                if x.is_infinite() {
                    assert_eq!(v, x, "{name}: value");
                } else {
                    assert!((v - x).abs() <= 1e-12, "{name}: value {v} against the oracle's {x}");
                }
            }
            (None, None) => {}
            other => panic!("{name}: value mismatch {other:?}"),
        }
    }
    assert_eq!(r.truncated, exp.get("truncated").u() as u64, "{name}: truncated");
    assert_eq!(canonical(r), exp.get("canonical").str(), "{name}: canonical JSON (AC-5)");
}

fn defect_body(name: &str) -> Body {
    build(&format!("defects/{name}"))
}

fn cube_labelled() -> Body {
    builder_with(&load("cube_labelled.json"), true).build().unwrap()
}

#[test]
fn ac1_labelled_cube_is_clean() {
    let r = validate(&cube_labelled(), TOL, None).unwrap();
    assert!(r.defects.is_empty(), "{:?}", r.defects);
    assert_eq!(r.truncated, 0);
    check_report("cube_labelled", &r, expected().get("reports").get("cube_labelled"));
}

#[test]
fn ac2_each_fixture_reports_its_seeded_defect() {
    let exp = expected();
    let mut kinds = std::collections::BTreeSet::new();
    for name in DEFECT_FIXTURES {
        let r = validate(&defect_body(name), TOL, None).unwrap();
        check_report(name, &r, exp.get("reports").get(&format!("defects/{name}")));
        kinds.extend(r.defects.iter().map(Defect::kind));
    }
    // Every kind a built body can carry is exercised (NonManifoldEdge cannot: see below).
    let all = [
        DefectKind::OpenLoop,
        DefectKind::SelfIntersectingLoop,
        DefectKind::DanglingEdge,
        DefectKind::InconsistentOrientation,
        DefectKind::EdgeOffSurface,
        DefectKind::PcurveMismatch,
        DefectKind::PcurveFailed,
        DefectKind::SeamWinding,
    ];
    assert_eq!(kinds.into_iter().collect::<Vec<_>>(), all.to_vec());
}

#[test]
fn ac2_a_non_manifold_edge_never_reaches_validation() {
    // More than two faces on an edge needs more than two coedges, which the builder refuses
    // (GEO.CAD.TOPO.MODEL check 4); expectation hand-labelled in the fixture script.
    let script = load("defects/non_manifold_edge.json");
    let exp = expected();
    let nm = exp.get("non_manifold_edge");
    assert_eq!((nm.get("code").str(), nm.get("fault").str()), ("BadPairing", "TooMany"));
    assert_eq!(
        builder(&script).build().unwrap_err(),
        TopoBuildError::BadPairing { edge: nm.get("edge").u(), fault: PairingFault::TooMany }
    );
}

#[test]
fn ac3_validation_leaves_the_body_unchanged() {
    for name in DEFECT_FIXTURES {
        let body = defect_body(name);
        let (digest, bytes) = (body_digest(&body), encode(&body));
        let copy = body.clone();
        validate(&body, TOL, None).unwrap();
        validate(&body, TOL, Some(1)).unwrap();
        assert_eq!(body_digest(&body), digest, "{name}");
        assert_eq!(encode(&body), bytes, "{name}");
        assert_eq!(body, copy, "{name}");
    }
}

#[test]
fn ac4_cap_lists_the_first_defects_and_counts_the_rest() {
    let exp = expected();
    let body = build("valid_cap50");
    let full = validate(&body, TOL, None).unwrap();
    assert_eq!((full.defects.len(), full.truncated), (50, 0));
    check_report("valid_cap50", &full, exp.get("reports").get("valid_cap50"));
    let capped = validate(&body, TOL, Some(20)).unwrap();
    assert_eq!((capped.defects.len(), capped.truncated), (20, 30));
    check_report("valid_cap50@20", &capped, exp.get("reports").get("valid_cap50@20"));
    assert_eq!(capped.defects[..], full.defects[..20]);
    // Hand-labelled edges of the cap: none kept, exactly all, more than all.
    let none = validate(&body, TOL, Some(0)).unwrap();
    assert_eq!((none.defects.len(), none.truncated), (0, 50));
    assert_eq!(validate(&body, TOL, Some(50)).unwrap(), full);
    assert_eq!(validate(&body, TOL, Some(u64::MAX)).unwrap(), full);
}

#[test]
fn ac5_reports_are_pure_and_canonical() {
    // The canonical JSON equals the oracle's bit patterns in ac2 and ac4 on every platform CI
    // runs; here repeated runs give the same bytes.
    for name in DEFECT_FIXTURES {
        let body = defect_body(name);
        let a = canonical(&validate(&body, TOL, None).unwrap());
        let b = canonical(&validate(&body, TOL, None).unwrap());
        assert_eq!(a, b, "{name}");
    }
}

#[test]
fn capped_cylinder_is_clean() {
    // Oracle-checked (on the same script with labels added; an empty report carries no IDs).
    let r = validate(&build("cylinder_capped"), TOL, None).unwrap();
    check_report("cylinder_capped", &r, expected().get("reports").get("cylinder_capped"));
}

#[test]
fn an_undetermined_face_counts_as_agreeing_with_its_sense() {
    // The band of a closed cylinder winds around its surface, so its orientation is not
    // measured; the caps are. Oracle-checked: clean, and with the top cap marked Reversed only
    // the top edge (a global sign error would flag the bottom edge instead).
    let exp = expected();
    for name in ["capped_band", "capped_band_top_reversed"] {
        let r = validate(&build(name), TOL, None).unwrap();
        check_report(name, &r, exp.get("reports").get(name));
    }
}

#[test]
fn other_valid_bodies_are_clean() {
    // Hand-labelled: well-formed bodies of the MODEL and PCURVE fixtures. The bands and
    // hemispheres wind once around a singly periodic surface, which is not a defect.
    // Only the MODEL cube gives its line directions in `axis` (see common::curve_with).
    for (name, swap) in [
        ("cube_unlabelled", true),
        ("sphere_hemispheres", false),
        ("plate_with_hole_nopcurve", false),
        ("cylinder_band", false),
    ] {
        let body = builder_with(&load(&format!("{name}.json")), swap).build().unwrap();
        let r = validate(&body, TOL, None).unwrap();
        assert!(r.defects.is_empty(), "{name}: {:?}", r.defects);
    }
}

#[test]
fn reversing_both_caps_flags_both_cap_edges() {
    // Hand-labelled: with both caps marked Reversed (their loops still counter-clockwise about
    // their outward axes) each cap disagrees with its sense and the side face agrees, so both
    // circle edges (cyl-e0 top, cyl-e2 bottom) are inconsistent; the seam lies in one face.
    let mut script = load("defects/inconsistent_orientation.json");
    let J::Obj(ref mut top) = script else { panic!() };
    let J::Arr(ref mut faces) = top.get_mut("faces").unwrap() else { panic!() };
    let J::Obj(ref mut bottom) = faces[2] else { panic!() };
    bottom.insert("sense".to_string(), J::Str("reversed".to_string()));
    let r = validate(&builder(&script).build().unwrap(), TOL, None).unwrap();
    let ids: Vec<(DefectKind, String)> = r.defects.iter().map(|d| (d.kind(), id(d.entity()))).collect();
    assert_eq!(
        ids,
        vec![
            (DefectKind::InconsistentOrientation, "cyl-e0".to_string()),
            (DefectKind::InconsistentOrientation, "cyl-e2".to_string())
        ]
    );
}

#[test]
fn a_pcurve_that_folds_back_along_its_neighbours_self_intersects() {
    // Hand-labelled: the pcurve_mismatch sheet with its imported pcurve 2^-20 m above the
    // bottom edge instead of below. The connectors to the left and right edges then run back
    // along those edges (collinear fold-backs), so the loop meets itself; the disagreement is
    // still 2^-20 m.
    let mut script = load("defects/pcurve_mismatch.json");
    let J::Obj(ref mut top) = script else { panic!() };
    let J::Arr(ref mut coedges) = top.get_mut("coedges").unwrap() else { panic!() };
    let J::Obj(ref mut bottom) = coedges[0] else { panic!() };
    let J::Obj(ref mut pc) = bottom.get_mut("pcurve").unwrap() else { panic!() };
    pc.insert("origin".to_string(), J::Arr(vec![J::Num(0.0), J::Num(2f64.powi(-20))]));
    let r = validate(&builder(&script).build().unwrap(), TOL, None).unwrap();
    assert_eq!(
        r.defects,
        vec![
            Defect::SelfIntersectingLoop { r#loop: TopologyId::Imported(fid_geo_cad::Label::new("l").unwrap()) },
            Defect::PcurveMismatch {
                coedge: TopologyId::Imported(fid_geo_cad::Label::new("c-bottom").unwrap()),
                deviation_m: 2f64.powi(-20)
            },
        ]
    );
}

fn plane_frame(z: f64) -> Frame {
    Frame { origin: [0.0, 0.0, z], axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] }
}

fn line_def(o: [f64; 3], d: [f64; 3]) -> CurveDef {
    CurveDef::analytic(CurveParams::Line { frame: Frame { origin: o, axis: [0.0, 0.0, 1.0], ref_dir: d } }).unwrap()
}

/// A unit square sheet with a circular hole loop of radius `r` about `(cx, 0.5)`.
fn plate_with_hole(cx: f64, r: f64) -> Body {
    let mut b = BodyBuilder::new([7; 16]);
    let pts = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
    let dirs = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]];
    let vs: Vec<usize> = pts.iter().map(|&p| b.add_vertex(p, None)).collect();
    let mut cs = Vec::new();
    for i in 0..4 {
        let e = b.add_edge(Some(line_def(pts[i], dirs[i])), vs[i], vs[(i + 1) % 4], [0.0, 1.0], None);
        cs.push(b.add_coedge(e, Sense::Same, None, None));
    }
    let outer = b.add_loop(cs, true, Some("l-outer"));
    let hv = b.add_vertex([cx + r, 0.5, 0.0], None);
    let circle = CurveDef::analytic(CurveParams::Circle {
        frame: Frame { origin: [cx, 0.5, 0.0], axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] },
        radius: r,
    })
    .unwrap();
    let he = b.add_edge(Some(circle), hv, hv, [0.0, core::f64::consts::TAU], None);
    let hc = b.add_coedge(he, Sense::Reversed, None, None);
    let hole = b.add_loop(vec![hc], false, Some("l-hole"));
    let f = b.add_face(
        SurfaceDef::analytic(SurfaceParams::Plane { frame: plane_frame(0.0) }).unwrap(),
        Sense::Same,
        vec![outer, hole],
        None,
    );
    b.add_shell(vec![f], false);
    b.build().unwrap()
}

#[test]
fn two_loops_that_cross_are_both_reported() {
    // Hand-labelled: a hole of radius 0.25 about (1, 0.5) crosses the square's right edge; a
    // hole about (0.5, 0.5) does not.
    let r = validate(&plate_with_hole(1.0, 0.25), TOL, None).unwrap();
    let ids: Vec<(DefectKind, String)> = r.defects.iter().map(|d| (d.kind(), id(d.entity()))).collect();
    assert_eq!(
        ids,
        vec![
            (DefectKind::SelfIntersectingLoop, "l-hole".to_string()),
            (DefectKind::SelfIntersectingLoop, "l-outer".to_string())
        ]
    );
    assert!(validate(&plate_with_hole(0.5, 0.25), TOL, None).unwrap().defects.is_empty());
}

#[test]
fn bad_tolerance_is_refused() {
    let body = cube_labelled();
    for t in [0.0, -1e-7, f64::NAN, f64::INFINITY] {
        match validate(&body, t, None) {
            Err(ValidError::BadTolerance { value }) => assert_eq!(value.to_bits(), t.to_bits()),
            other => panic!("{t}: {other:?}"),
        }
    }
}

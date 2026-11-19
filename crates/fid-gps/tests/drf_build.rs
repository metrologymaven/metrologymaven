//! Acceptance criteria of spec node GDT.DRF.BUILD on the pure builder (AC-1, AC-2, AC-3 error
//! path, AC-4), REQ-001 .. REQ-005, and every precedence sequence of plane and axis for one, two
//! and three datums. Expected frames and refusals come from the independent high-precision
//! Python reference `fixtures/gdt/drf/build/make_cases.py` (`cases.json`); the tolerance
//! boundaries and input errors are hand-labelled from the leaf's rules. Synthetic data only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gps::dmt::{AlgId, DmtKey};
use fid_gps::drf::build::{
    build_drf, DatumKind, Dof, DofSet, Drf, DrfError, Precedence, SimGeometry, SimulatedDatum, ANGLE_TOL, COAXIAL_TOL,
    MAX_DATUMS,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// AC-1: origin and axes within 1e-12 m and 1e-12 rad of the reference.
const TOL: f64 = 1e-12;

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("object"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("array"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("string"),
    }
}

fn v3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    assert_eq!(a.len(), 3);
    [0, 1, 2].map(|i| st(&a[i]).parse::<f64>().unwrap())
}

fn dofset(v: &CValue) -> DofSet {
    let d: Vec<Dof> = arr(v).iter().map(|n| Dof::parse(st(n)).unwrap()).collect();
    DofSet::of(&d)
}

struct Case {
    name: String,
    datums: Vec<SimulatedDatum>,
    expect: CValue,
}

fn cases() -> Vec<Case> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/drf/build/cases.json");
    let doc = fid_canon::parse(&std::fs::read(path).unwrap()).unwrap();
    arr(&obj(&doc)["cases"])
        .iter()
        .map(|c| {
            let c = obj(c);
            let datums = arr(&c["datums"])
                .iter()
                .map(|d| {
                    let d = obj(d);
                    let (point, dir) = (v3(&d["point"]), v3(&d["dir"]));
                    let geometry = match st(&d["kind"]) {
                        "plane" => SimGeometry::Plane { point, normal: dir },
                        "axis" => SimGeometry::Axis { point, direction: dir },
                        k => panic!("kind {k}"),
                    };
                    SimulatedDatum {
                        label: st(&d["label"]).to_owned(),
                        geometry,
                        alg: AlgId::parse(st(&d["alg"])).unwrap(),
                        dmt_row: DmtKey::parse(st(&d["dmt_row"])).unwrap(),
                    }
                })
                .collect();
            Case { name: st(&c["name"]).to_owned(), datums, expect: c["expect"].clone() }
        })
        .collect()
}

fn case(name: &str) -> Case {
    cases().into_iter().find(|c| c.name == name).unwrap()
}

fn close(a: [f64; 3], b: [f64; 3], what: &str, name: &str) {
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() <= TOL, "{name}: {what}[{i}] = {:e}, reference {:e}", a[i], b[i]);
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Structural invariants every built DRF holds.
fn invariants(drf: &Drf, name: &str) {
    let mut seen = DofSet::EMPTY;
    for d in drf.datums() {
        assert!(!d.constrained.is_empty(), "{name}: {} constrains nothing", d.label);
        assert!(d.constrained.intersection(seen).is_empty(), "{name}: {} re-constrains", d.label);
        seen = seen.union(d.constrained);
    }
    assert!(seen.intersection(drf.free()).is_empty());
    assert_eq!(seen.union(drf.free()), DofSet::ALL, "{name}");
    let [x, y, z] = drf.axes();
    for (a, b) in [(x, x), (y, y), (z, z)] {
        assert!((dot(a, b) - 1.0).abs() <= 1e-15, "{name}: not unit");
    }
    for (a, b) in [(x, y), (y, z), (z, x)] {
        assert!(dot(a, b).abs() <= 1e-15, "{name}: not orthogonal");
    }
    close(cross(x, y), z, "x cross y", name);
}

#[test]
fn reference_cases() {
    let all = cases();
    assert!(all.len() >= 80);
    for c in &all {
        let got = build_drf(&c.datums);
        let e = obj(&c.expect);
        if let Some(err) = e.get("error") {
            let err = obj(err);
            assert_eq!(st(&err["code"]), "degenerate_datum_pair");
            let pair = arr(&err["pair"]);
            let want =
                DrfError::DegenerateDatumPair { first: st(&pair[0]).to_owned(), second: st(&pair[1]).to_owned() };
            assert_eq!(got, Err(want), "{}", c.name);
            continue;
        }
        let drf = got.unwrap_or_else(|e| panic!("{}: {e}", c.name));
        let ok = obj(&e["ok"]);
        let dof = arr(&ok["dof"]);
        assert_eq!(drf.datums().len(), dof.len(), "{}", c.name);
        for (d, want) in drf.datums().iter().zip(dof) {
            assert_eq!(d.constrained, dofset(want), "{}: datum {}", c.name, d.label);
        }
        assert_eq!(drf.free(), dofset(&ok["free"]), "{}: free", c.name);
        close(drf.origin(), v3(&ok["origin"]), "origin", &c.name);
        let axes = arr(&ok["axes"]);
        for (i, axis) in drf.axes().into_iter().enumerate() {
            close(axis, v3(&axes[i]), ["x", "y", "z"][i], &c.name);
        }
        invariants(&drf, &c.name);
    }
}

#[test]
fn every_precedence_sequence_is_covered() {
    // REQ-003: every sequence of plane (P) and axis (X) of length 1, 2 and 3, each with at least
    // one built DRF and, from length 2 on, at least one refusal.
    let mut built = BTreeSet::new();
    let mut refused = BTreeSet::new();
    for c in cases() {
        let seq: String =
            c.datums.iter().map(|d| if d.geometry.kind() == DatumKind::Plane { 'P' } else { 'X' }).collect();
        if obj(&c.expect).contains_key("ok") {
            built.insert(seq);
        } else {
            refused.insert(seq);
        }
    }
    let mut all = BTreeSet::new();
    for n in 1..=3u32 {
        for bits in 0..(1u32 << n) {
            all.insert((0..n).map(|i| if (bits >> i) & 1 == 0 { 'P' } else { 'X' }).collect::<String>());
        }
    }
    assert_eq!(all.len(), 14);
    assert_eq!(built, all);
    let multi: BTreeSet<String> = all.iter().filter(|s| s.len() > 1).cloned().collect();
    // Every two- and three-datum sequence has a refusal case, except plane-axis and axis-plane:
    // a secondary of the other type always constrains something.
    let expect_refused: BTreeSet<String> = multi.into_iter().filter(|s| s != "PX" && s != "XP").collect();
    assert_eq!(refused, expect_refused);
}

#[test]
fn ac1_block_abc() {
    let drf = build_drf(&case("block_abc").datums).unwrap();
    let c: Vec<DofSet> = drf.datums().iter().map(|d| d.constrained).collect();
    assert_eq!(c[0], DofSet::of(&[Dof::Tz, Dof::Rx, Dof::Ry]));
    assert_eq!(c[1], DofSet::of(&[Dof::Ty, Dof::Rz]));
    assert_eq!(c[2], DofSet::of(&[Dof::Tx]));
    assert!(drf.free().is_empty());
    // Analytic intersection of z = 0.01, y = 0.02, x = 0.03; axes the identity.
    close(drf.origin(), [0.03, 0.02, 0.01], "origin", "block_abc");
    assert_eq!(drf.axes(), [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    let p: Vec<Precedence> = drf.datums().iter().map(|d| d.precedence).collect();
    assert_eq!(p, [Precedence::Primary, Precedence::Secondary, Precedence::Tertiary]);
}

#[test]
fn ac2_plate_bore() {
    let drf = build_drf(&case("plate_bore").datums).unwrap();
    assert_eq!(drf.datums()[0].constrained, DofSet::of(&[Dof::Tz, Dof::Rx, Dof::Ry]));
    assert_eq!(drf.datums()[1].constrained, DofSet::of(&[Dof::Tx, Dof::Ty]));
    assert_eq!(drf.free(), DofSet::of(&[Dof::Rz]));
    assert_eq!(drf.datums()[1].kind, DatumKind::Axis);
}

#[test]
fn ac3_parallel_planes_refused() {
    let e = build_drf(&case("pp_parallel").datums).unwrap_err();
    assert_eq!(e, DrfError::DegenerateDatumPair { first: "A".into(), second: "B".into() });
    assert_eq!(e.code(), "degenerate_datum_pair");
    assert_eq!(e.to_string(), "degenerate_datum_pair{A,B}");
}

#[test]
fn ac4_single_axis() {
    let drf = build_drf(&case("x_single").datums).unwrap();
    assert_eq!(drf.free(), DofSet::of(&[Dof::Tz, Dof::Rz]));
    assert_eq!(drf.free().to_string(), "{tz, rz}");
}

#[test]
fn req005_simulator_and_row_recorded() {
    let c = case("pxp_clock");
    let drf = build_drf(&c.datums).unwrap();
    for (d, s) in drf.datums().iter().zip(&c.datums) {
        assert_eq!(d.label, s.label);
        assert_eq!(d.alg, s.alg);
        assert_eq!(d.dmt_row, s.dmt_row);
    }
    assert_eq!(drf.datums()[0].alg.as_str(), "fit.tan_plane.minimax@1.0.0");
    assert_eq!(drf.datums()[1].alg.as_str(), "fit.tan_cylinder.max_inscribed@1.0.0");
}

// ---- hand-labelled from the leaf's rules -------------------------------------------------------

fn plane(label: &str, point: [f64; 3], normal: [f64; 3]) -> SimulatedDatum {
    SimulatedDatum {
        label: label.into(),
        geometry: SimGeometry::Plane { point, normal },
        alg: AlgId::parse("fit.tan_plane.minimax@1.0.0").unwrap(),
        dmt_row: DmtKey::parse("dmt:asme/position/none@1").unwrap(),
    }
}

fn axis(label: &str, point: [f64; 3], direction: [f64; 3]) -> SimulatedDatum {
    SimulatedDatum {
        label: label.into(),
        geometry: SimGeometry::Axis { point, direction },
        alg: AlgId::parse("fit.tan_cylinder.max_inscribed@1.0.0").unwrap(),
        dmt_row: DmtKey::parse("dmt:asme/position/none@1").unwrap(),
    }
}

/// A unit vector at angle `t` (radians) from `z` towards `y`.
fn tilted(t: f64) -> [f64; 3] {
    [0.0, t.sin(), t.cos()]
}

#[test]
fn angular_tolerance_is_pinned() {
    let a = plane("A", [0.0; 3], [0.0, 0.0, 1.0]);
    // Secondary plane tilted 0.5e-9 rad from the primary: parallel, refused.
    let e = build_drf(&[a.clone(), plane("B", [0.0; 3], tilted(0.5 * ANGLE_TOL))]).unwrap_err();
    assert_eq!(e, DrfError::DegenerateDatumPair { first: "A".into(), second: "B".into() });
    // Tilted 2e-9 rad: not parallel, constrains {ty, rz}.
    let drf = build_drf(&[a.clone(), plane("B", [0.0; 3], tilted(2.0 * ANGLE_TOL))]).unwrap();
    assert_eq!(drf.datums()[1].constrained, DofSet::of(&[Dof::Ty, Dof::Rz]));
    // An axis 0.5e-9 rad off perpendicular to the primary plane still counts as perpendicular:
    // {tx, ty}, rz free. At 2e-9 rad it is oblique and also fixes rz.
    let drf = build_drf(&[a.clone(), axis("B", [0.0; 3], tilted(0.5 * ANGLE_TOL))]).unwrap();
    assert_eq!(drf.datums()[1].constrained, DofSet::of(&[Dof::Tx, Dof::Ty]));
    let drf = build_drf(&[a.clone(), axis("B", [0.0; 3], tilted(2.0 * ANGLE_TOL))]).unwrap();
    assert_eq!(drf.datums()[1].constrained, DofSet::of(&[Dof::Tx, Dof::Ty, Dof::Rz]));
    // An axis 0.5e-9 rad off lying along the plane counts as lying along it: {ty, rz}.
    let along = |t: f64| [t.cos(), 0.0, t.sin()];
    let drf = build_drf(&[a.clone(), axis("B", [0.0; 3], along(0.5 * ANGLE_TOL))]).unwrap();
    assert_eq!(drf.datums()[1].constrained, DofSet::of(&[Dof::Ty, Dof::Rz]));
    let drf = build_drf(&[a, axis("B", [0.0; 3], along(2.0 * ANGLE_TOL))]).unwrap();
    assert_eq!(drf.datums()[1].constrained, DofSet::of(&[Dof::Tx, Dof::Ty, Dof::Rz]));
}

#[test]
fn coaxial_tolerance_is_pinned() {
    let a = axis("A", [0.0; 3], [0.0, 0.0, 1.0]);
    let e = build_drf(&[a.clone(), axis("B", [0.5 * COAXIAL_TOL, 0.0, 0.3], [0.0, 0.0, 1.0])]).unwrap_err();
    assert_eq!(e, DrfError::DegenerateDatumPair { first: "A".into(), second: "B".into() });
    let drf = build_drf(&[a, axis("B", [2.0 * COAXIAL_TOL, 0.0, 0.3], [0.0, 0.0, 1.0])]).unwrap();
    assert_eq!(drf.datums()[1].constrained, DofSet::of(&[Dof::Rz]));
    close(drf.axes()[0], [1.0, 0.0, 0.0], "x", "coaxial");
}

#[test]
fn nothing_left_to_constrain() {
    // A full DRF from two datums leaves the tertiary nothing: refused against the secondary.
    let e = build_drf(&[
        axis("A", [0.0; 3], [0.0, 0.0, 1.0]),
        axis("B", [0.1, 0.0, 0.2], [1.0, 0.0, 0.0]),
        axis("C", [0.0, 0.1, 0.4], [0.0, 1.0, 0.0]),
    ])
    .unwrap_err();
    assert_eq!(e, DrfError::DegenerateDatumPair { first: "B".into(), second: "C".into() });
}

#[test]
fn free_clocking_is_pinned() {
    // A single plane with normal z: x is world x.
    let drf = build_drf(&[plane("A", [0.0; 3], [0.0, 0.0, 1.0])]).unwrap();
    assert_eq!(drf.axes(), [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    // Normal x: the least aligned world axis is y (tie with z, lowest index), so x = world y.
    let drf = build_drf(&[plane("A", [0.0; 3], [1.0, 0.0, 0.0])]).unwrap();
    assert_eq!(drf.axes(), [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]]);
}

#[test]
fn input_errors() {
    let a = plane("A", [0.0; 3], [0.0, 0.0, 1.0]);
    assert_eq!(build_drf(&[]), Err(DrfError::NoDatums));
    let four: Vec<SimulatedDatum> = ["A", "B", "C", "D"].iter().map(|l| plane(l, [0.0; 3], [0.0, 0.0, 1.0])).collect();
    assert_eq!(build_drf(&four), Err(DrfError::TooManyDatums { count: MAX_DATUMS + 1 }));
    assert_eq!(build_drf(&four).unwrap_err().code(), "too_many_datums");
    assert_eq!(build_drf(&[plane("", [0.0; 3], [0.0, 0.0, 1.0])]), Err(DrfError::EmptyLabel));
    assert_eq!(
        build_drf(&[a.clone(), plane("A", [0.0; 3], [0.0, 1.0, 0.0])]),
        Err(DrfError::DuplicateLabel { label: "A".into() })
    );
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            build_drf(&[a.clone(), plane("B", [bad, 0.0, 0.0], [0.0, 1.0, 0.0])]),
            Err(DrfError::NotFinite { label: "B".into() })
        );
        assert_eq!(
            build_drf(&[a.clone(), axis("B", [0.0; 3], [0.0, bad, 0.0])]),
            Err(DrfError::NotFinite { label: "B".into() })
        );
    }
    assert_eq!(
        build_drf(&[a.clone(), plane("B", [0.0; 3], [0.0, 1.0 + 1e-6, 0.0])]),
        Err(DrfError::NotUnit { label: "B".into() })
    );
    assert_eq!(build_drf(&[axis("A", [0.0; 3], [0.0; 3])]), Err(DrfError::NotUnit { label: "A".into() }));
    // A huge direction overflows the norm: not finite.
    assert_eq!(build_drf(&[axis("A", [0.0; 3], [1e200, 0.0, 0.0])]), Err(DrfError::NotFinite { label: "A".into() }));
    // Within UNIT_TOL the direction is accepted and normalised.
    let drf = build_drf(&[plane("A", [0.0; 3], [0.0, 0.0, 1.0 + 1e-10])]).unwrap();
    assert_eq!(drf.axes()[2], [0.0, 0.0, 1.0]);
}

#[test]
fn dof_set_api() {
    let s = DofSet::of(&[Dof::Rz, Dof::Tx, Dof::Tx]);
    assert_eq!(s.len(), 2);
    assert_eq!(s.dofs(), [Dof::Tx, Dof::Rz]);
    assert_eq!(s.to_string(), "{tx, rz}");
    assert_eq!(DofSet::EMPTY.to_string(), "{}");
    assert_eq!(DofSet::ALL.len(), 6);
    assert_eq!(DofSet::from_bits(0b100_0000), None);
    assert_eq!(DofSet::from_bits(s.bits()), Some(s));
    assert_eq!(DofSet::ALL.minus(s), DofSet::of(&[Dof::Ty, Dof::Tz, Dof::Rx, Dof::Ry]));
    for d in Dof::ALL {
        assert_eq!(Dof::parse(d.name()), Some(d));
        assert!(DofSet::EMPTY.with(d).contains(d));
    }
    assert_eq!(Dof::parse("tw"), None);
}

#[test]
fn deterministic_bits() {
    // D0: the same input gives bit-identical output.
    for c in cases() {
        assert_eq!(build_drf(&c.datums), build_drf(&c.datums), "{}", c.name);
    }
}

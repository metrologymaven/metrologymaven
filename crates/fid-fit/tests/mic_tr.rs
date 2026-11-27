//! Acceptance criteria of spec node KRN.ENV.MIC.TR (AC-1 .. AC-4) and the error paths.
//!
//! Expected regions, rows and boundary distances come from
//! `fixtures/fit/make_mic_tr_fixtures.py`: constants in 80-digit decimal arithmetic rounded once
//! to binary64, the polyhedron's farthest vertex by enumeration, and the pinned binary64 rules
//! (Neumaier dot, norm2 scaling, the direction and basis rules) re-implemented from the spec,
//! not from the Rust code. CI runs these tests on x86-64 Linux and Windows, ARM64 macOS and
//! wasm32 under wasmtime against the same expected bits.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_fit::contract::{
    algorithm_id, Fit, FitDiagnostics, FitError, FitResult, FittedFeature, Method, PointsView, Primitive, StopReason,
};
use fid_fit::ls_rnd::{default_params, Circle2Ls};
use fid_fit::mic_tr::{
    check_defined, is_defined, TrustRegion, DIRS2, DIRS3, FACETS_2D, FACETS_3D, K2, K3, MIC_DEFINED,
};

const CASES: &str = include_str!("../../../fixtures/fit/mic-tr-v1/cases.txt");

fn hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn hexes(v: &[&str]) -> Vec<f64> {
    v.iter().map(|s| hex(s)).collect()
}

struct Check {
    label: String,
    x: [f64; 3],
    tol: f64,
    slack: f64,
    verdict: Verdict,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verdict {
    Ok,
    Unbounded,
    Refused,
}

struct Case {
    name: String,
    feature: FittedFeature,
    fraction: f64,
    centre: [f64; 3],
    axis: Option<[f64; 3]>,
    radius: f64,
    inradius: f64,
    floor: f64,
    rows: Vec<([f64; 3], f64)>,
    checks: Vec<Check>,
}

fn v3(x: &[f64]) -> [f64; 3] {
    [x[0], x[1], x[2]]
}

fn cases() -> Vec<Case> {
    let text = CASES.replace("\r\n", "\n");
    let mut out = Vec::new();
    let mut cur: Option<Case> = None;
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let t: Vec<&str> = line.split(' ').collect();
        let c = cur.as_mut();
        match t[0] {
            "case" => {
                cur = Some(Case {
                    name: t[1].to_string(),
                    feature: FittedFeature::Point { c: [0.0; 3] },
                    fraction: f64::NAN,
                    centre: [f64::NAN; 3],
                    axis: None,
                    radius: f64::NAN,
                    inradius: f64::NAN,
                    floor: f64::NAN,
                    rows: Vec::new(),
                    checks: Vec::new(),
                })
            }
            "feature" => {
                let x = hexes(&t[2..]);
                c.unwrap().feature = match t[1] {
                    "circle2" => FittedFeature::Circle2 { c: [x[0], x[1]], r: x[2] },
                    "sphere" => FittedFeature::Sphere { c: v3(&x), r: x[3] },
                    "cylinder" => FittedFeature::Cylinder { p: v3(&x), d: v3(&x[3..]), r: x[6] },
                    k => panic!("unknown feature {k}"),
                }
            }
            "fraction" => c.unwrap().fraction = hex(t[1]),
            "centre" => c.unwrap().centre = v3(&hexes(&t[1..])),
            "axis" => c.unwrap().axis = if t[1] == "none" { None } else { Some(v3(&hexes(&t[1..]))) },
            "radius" => c.unwrap().radius = hex(t[1]),
            "inradius" => c.unwrap().inradius = hex(t[1]),
            "floor" => c.unwrap().floor = hex(t[1]),
            "row" => {
                let x = hexes(&t[1..]);
                assert_eq!(x.len(), 4, "{line}");
                c.unwrap().rows.push((v3(&x), x[3]));
            }
            "check" => {
                assert_eq!(t.len(), 8, "{line}");
                let x = hexes(&t[2..7]);
                c.unwrap().checks.push(Check {
                    label: t[1].to_string(),
                    x: v3(&x),
                    tol: x[3],
                    slack: x[4],
                    verdict: match t[7] {
                        "unbounded" => Verdict::Unbounded,
                        "ok" => Verdict::Ok,
                        "refused" => Verdict::Refused,
                        v => panic!("unknown verdict {v}"),
                    },
                });
            }
            "end" => out.push(cur.take().unwrap()),
            k => panic!("unknown line {k}"),
        }
    }
    assert_eq!(out.len(), 10);
    out
}

fn bits3(v: [f64; 3]) -> [u64; 3] {
    [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()]
}

/// AC-1: centre, axis, radius and every constraint row equal the oracle bit for bit.
#[test]
fn ac1_regions_and_rows_bit_exact() {
    for c in cases() {
        let tr = TrustRegion::from_feature(&c.feature, c.fraction).unwrap();
        assert_eq!(tr.primitive(), c.feature.primitive(), "{}", c.name);
        assert_eq!(bits3(tr.centre()), bits3(c.centre), "{} centre", c.name);
        assert_eq!(tr.axis().map(bits3), c.axis.map(bits3), "{} axis", c.name);
        assert_eq!(tr.radius_m().to_bits(), c.radius.to_bits(), "{} radius", c.name);
        assert_eq!(tr.inradius_m().to_bits(), c.inradius.to_bits(), "{} inradius", c.name);
        assert_eq!(tr.fraction().to_bits(), c.fraction.to_bits(), "{} fraction", c.name);
        assert_eq!(tr.boundary_tol_floor().unwrap().to_bits(), c.floor.to_bits(), "{} floor", c.name);
        let rows = tr.rows().unwrap();
        let want = if c.feature.primitive() == Primitive::Sphere { FACETS_3D } else { FACETS_2D };
        assert_eq!(rows.a.len(), want, "{}", c.name);
        assert_eq!(rows.b.len(), want, "{}", c.name);
        assert_eq!(c.rows.len(), want, "{}", c.name);
        for (k, (a, b)) in c.rows.iter().enumerate() {
            assert_eq!(bits3(rows.a[k]), bits3(*a), "{} row {k} a", c.name);
            assert_eq!(rows.b[k].to_bits(), b.to_bits(), "{} row {k} b", c.name);
        }
    }
}

/// AC-1 recorded values: radius is fraction times the least-squares radius; the cylinder's
/// facet normals are perpendicular to the axis (a prism along it).
#[test]
fn ac1_recorded_fraction_and_prism() {
    for c in cases() {
        let tr = TrustRegion::from_feature(&c.feature, c.fraction).unwrap();
        let r = match c.feature {
            FittedFeature::Circle2 { r, .. } | FittedFeature::Sphere { r, .. } | FittedFeature::Cylinder { r, .. } => r,
            _ => unreachable!(),
        };
        assert_eq!(tr.radius_m(), c.fraction * r, "{}", c.name);
        if let Some(d) = tr.axis() {
            for a in tr.rows().unwrap().a {
                let dot = a[0] * d[0] + a[1] * d[1] + a[2] * d[2];
                assert!(dot.abs() < 1e-15, "{}: {dot:e}", c.name);
            }
        }
    }
}

/// AC-1 through a real least-squares result: the region sits on the fit's own centre.
#[test]
fn ac1_from_ls_result() {
    let mut xyz = Vec::new();
    for k in 0..12 {
        let t = (k as f64) * core::f64::consts::PI / 6.0;
        xyz.extend_from_slice(&[0.3 + 0.01 * t.cos(), -0.2 + 0.01 * t.sin(), 0.0]);
    }
    let pts = PointsView::new(&xyz, None).unwrap();
    let res = Circle2Ls.fit(&pts, &default_params(), None).unwrap();
    let tr = TrustRegion::from_ls(&res, 0.2).unwrap();
    let FittedFeature::Circle2 { c, r } = res.feature else { panic!("{:?}", res.feature) };
    assert_eq!(bits3(tr.centre()), bits3([c[0], c[1], 0.0]));
    assert_eq!(tr.radius_m().to_bits(), (0.2 * r).to_bits());
    assert_eq!(tr.inradius_m().to_bits(), (0.2 * r * K2).to_bits());
}

/// A result from another method is not a least-squares result.
#[test]
fn from_ls_refuses_other_methods() {
    let feature = FittedFeature::Circle2 { c: [0.0, 0.0], r: 0.01 };
    let diag = FitDiagnostics { grad_norm: 0.0, rank: 3, cond: 1.0, iterations: 0, stop: StopReason::Exchange };
    let alg = algorithm_id(Primitive::Circle2, Method::MinimumCircumscribed, (1, 0, 0)).unwrap();
    let res = FitResult::assemble(&feature, vec![0.0, 0.0, 0.0], diag, None, alg).unwrap();
    assert_eq!(TrustRegion::from_ls(&res, 0.2), Err(FitError::InvalidParams { field: "result" }));
}

/// AC-2: on the boundary within `boundary_tol` (or beyond it) is `unbounded_envelope` with the
/// boundary distance; 1 percent inside, the check passes.
#[test]
fn ac2_boundary_checks() {
    let mut seen = (0, 0, 0);
    for c in cases() {
        let tr = TrustRegion::from_feature(&c.feature, c.fraction).unwrap();
        for k in &c.checks {
            let d = tr.boundary_distance(k.x).unwrap();
            assert_eq!(d.to_bits(), k.slack.to_bits(), "{} {}", c.name, k.label);
            match tr.check_boundary(k.x, k.tol) {
                Err(FitError::UnboundedEnvelope { boundary_distance_m }) => {
                    assert_eq!(k.verdict, Verdict::Unbounded, "{} {}", c.name, k.label);
                    assert_eq!(boundary_distance_m.to_bits(), k.slack.to_bits(), "{} {}", c.name, k.label);
                    seen.0 += 1;
                }
                Ok(dist) => {
                    assert_eq!(k.verdict, Verdict::Ok, "{} {}", c.name, k.label);
                    assert_eq!(dist.to_bits(), k.slack.to_bits(), "{} {}", c.name, k.label);
                    seen.1 += 1;
                }
                Err(FitError::InvalidParams { field: "boundary_tol" }) => {
                    assert_eq!(k.verdict, Verdict::Refused, "{} {}", c.name, k.label);
                    seen.2 += 1;
                }
                Err(e) => panic!("{} {}: {e:?}", c.name, k.label),
            }
            // Spec-level sanity, independent of the oracle's bits: the distances are the
            // geometric ones to within rounding of the centre's coordinates.
            let h = tr.inradius_m();
            let m = c.centre.iter().fold(0.0_f64, |a, x| if x.abs() > a { x.abs() } else { a });
            let tol = 1e-15 * (1.0 + m);
            let base = k.label.trim_end_matches("_at_floor").trim_end_matches("_below_floor");
            let geometric = if base.starts_with("inside_1pct") {
                Some(0.01 * h)
            } else if base.starts_with("on_facet") || base == "vertex" {
                Some(0.0)
            } else if base == "centre" {
                Some(h)
            } else {
                None
            };
            match geometric {
                Some(g) => assert!((d - g).abs() <= tol, "{} {}: {d:e} vs {g:e}", c.name, k.label),
                None => assert!(d < -0.4 * h, "{} {}: {d:e}", c.name, k.label),
            }
        }
    }
    assert!(seen.0 >= 50 && seen.1 >= 30 && seen.2 == 10, "{seen:?}");
}

/// Exactly at the tolerance is still the boundary; a tolerance below the rounding floor (`0`
/// and `-0.0` included) is refused, while the floor itself is accepted.
#[test]
fn ac2_tolerance_edge() {
    let tr = TrustRegion::from_feature(&FittedFeature::Circle2 { c: [0.0, 0.0], r: 0.01 }, 0.2).unwrap();
    let centre_dist = tr.boundary_distance([0.0, 0.0, 0.0]).unwrap();
    assert_eq!(
        tr.check_boundary([0.0, 0.0, 0.0], centre_dist),
        Err(FitError::UnboundedEnvelope { boundary_distance_m: centre_dist })
    );
    for tol in [0.0, -0.0] {
        assert_eq!(
            tr.check_boundary([0.0, 0.0, 0.0], tol),
            Err(FitError::InvalidParams { field: "boundary_tol" }),
            "{tol:e}"
        );
    }
    let floor = tr.boundary_tol_floor().unwrap();
    assert_eq!(floor, 16.0 * f64::EPSILON * 0.002);
    assert_eq!(tr.check_boundary([0.0, 0.0, 0.0], floor), Ok(centre_dist));
    let e = FitError::UnboundedEnvelope { boundary_distance_m: 0.0 };
    assert_eq!(e.code(), "unbounded_envelope");
    assert!(e.to_string().contains("unbounded envelope"), "{e}");
}

/// The far-from-origin case that a zero tolerance let through: on facet 5 of a circle 1.5 km
/// from the origin the computed slack is +1.1e-13 m (the exact one is about -1e-15 m), so a
/// tolerance of 0 would pass it; at the floor it is `unbounded_envelope`, below it refused.
#[test]
fn ac2_far_centre_on_facet_at_floor() {
    let c = cases().into_iter().find(|c| c.name == "circle_far").unwrap();
    let tr = TrustRegion::from_feature(&c.feature, c.fraction).unwrap();
    let k = c.checks.iter().find(|k| k.label == "on_facet_5_at_floor").unwrap();
    assert!(k.slack > 0.0, "{:e}", k.slack);
    assert_eq!(k.tol.to_bits(), tr.boundary_tol_floor().unwrap().to_bits());
    assert!(matches!(tr.check_boundary(k.x, k.tol), Err(FitError::UnboundedEnvelope { .. })));
    let below = c.checks.iter().find(|k| k.label == "on_facet_5_below_floor").unwrap();
    assert_eq!(tr.check_boundary(below.x, below.tol), Err(FitError::InvalidParams { field: "boundary_tol" }));
    assert_eq!(tr.check_boundary(k.x, 0.0), Err(FitError::InvalidParams { field: "boundary_tol" }));
}

/// The tolerance must be finite and not negative; the position must be finite.
#[test]
fn ac2_invalid_tolerance_and_position() {
    let tr = TrustRegion::from_feature(&FittedFeature::Sphere { c: [0.1, 0.2, 0.3], r: 0.02 }, 0.2).unwrap();
    for tol in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1e-9, -f64::MIN_POSITIVE] {
        assert_eq!(
            tr.check_boundary([0.1, 0.2, 0.3], tol),
            Err(FitError::InvalidParams { field: "boundary_tol" }),
            "{tol:e}"
        );
    }
    for x in [[f64::NAN, 0.0, 0.0], [0.0, f64::INFINITY, 0.0], [0.0, 0.0, f64::NEG_INFINITY]] {
        let e = tr.check_boundary(x, 1e-9).unwrap_err();
        assert!(matches!(e, FitError::Num(_)), "{e:?}");
    }
}

/// AC-3: a fraction that is not finite and positive is `InvalidParams{field: "fraction"}`, as
/// is one whose radius overflows or underflows.
#[test]
fn ac3_invalid_fraction() {
    let circle = FittedFeature::Circle2 { c: [0.0, 0.0], r: 0.01 };
    let sphere = FittedFeature::Sphere { c: [0.0, 0.0, 0.0], r: 10.0 };
    let cyl = FittedFeature::Cylinder { p: [0.0; 3], d: [0.0, 0.0, 1.0], r: 0.01 };
    for feat in [&circle, &sphere, &cyl] {
        for fr in [0.0, -0.0, -0.2, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e-320] {
            assert_eq!(
                TrustRegion::from_feature(feat, fr),
                Err(FitError::InvalidParams { field: "fraction" }),
                "{feat:?} {fr:e}"
            );
        }
    }
    // fraction * r overflows (sphere r = 10 m).
    assert_eq!(TrustRegion::from_feature(&sphere, f64::MAX), Err(FitError::InvalidParams { field: "fraction" }));
    // A fraction above 1 is a valid, if unusual, region.
    assert!(TrustRegion::from_feature(&circle, 1.5).is_ok());
}

/// An invalid least-squares feature is refused by the contract's canonical form.
#[test]
fn invalid_feature_refused() {
    let bad_r = FittedFeature::Circle2 { c: [0.0, 0.0], r: -0.01 };
    assert_eq!(TrustRegion::from_feature(&bad_r, 0.2), Err(FitError::InvalidParams { field: "feature.r" }));
    let bad_c = FittedFeature::Sphere { c: [f64::NAN, 0.0, 0.0], r: 0.01 };
    assert!(matches!(TrustRegion::from_feature(&bad_c, 0.2), Err(FitError::Num(_))));
    let bad_d = FittedFeature::Cylinder { p: [0.0; 3], d: [0.0; 3], r: 0.01 };
    assert_eq!(TrustRegion::from_feature(&bad_d, 0.2), Err(FitError::InvalidDirection));
}

/// AC-4: slot is `method_undefined{primitive: slot, method: mi}`; exactly circle2, sphere and
/// cylinder are defined.
#[test]
fn ac4_defined_primitives() {
    let e = check_defined(Primitive::Slot).unwrap_err();
    assert_eq!(e, FitError::MethodUndefined { primitive: Primitive::Slot, method: Method::MaximumInscribed });
    assert_eq!(e.code(), "method_undefined");
    assert_eq!(MIC_DEFINED, [Primitive::Circle2, Primitive::Sphere, Primitive::Cylinder]);
    for &p in Primitive::ALL {
        let want = matches!(p, Primitive::Circle2 | Primitive::Sphere | Primitive::Cylinder);
        assert_eq!(is_defined(p), want, "{p:?}");
        assert_eq!(check_defined(p).is_ok(), want, "{p:?}");
        // The contract's table is the same one (REQ-004): circle3 is not defined for MI, but
        // stays defined for minimum circumscribed.
        assert_eq!(Method::MaximumInscribed.defined_for(p), want, "{p:?}");
        assert_eq!(algorithm_id(p, Method::MaximumInscribed, (1, 0, 0)).is_ok(), want, "{p:?}");
        if !want {
            assert_eq!(
                check_defined(p),
                Err(FitError::MethodUndefined { primitive: p, method: Method::MaximumInscribed })
            );
        }
    }
    let slot = FittedFeature::Slot { c: [0.0; 3], n: [0.0, 0.0, 1.0], d: [1.0, 0.0, 0.0], length: 0.02, width: 0.01 };
    assert_eq!(
        TrustRegion::from_feature(&slot, 0.2),
        Err(FitError::MethodUndefined { primitive: Primitive::Slot, method: Method::MaximumInscribed })
    );
    assert!(Method::MinimumCircumscribed.defined_for(Primitive::Circle3));
    let c3 = FittedFeature::Circle3 { c: [0.0; 3], n: [0.0, 0.0, 1.0], r: 0.01 };
    assert_eq!(
        TrustRegion::from_feature(&c3, 0.2),
        Err(FitError::MethodUndefined { primitive: Primitive::Circle3, method: Method::MaximumInscribed })
    );
}

/// The constant tables: unit length to within rounding, the 2D normals 11.25 degrees apart
/// and every 3D normal a distinct direction; the two constants in range.
#[test]
fn constant_tables() {
    for (k, u) in DIRS2.iter().enumerate() {
        let n = u[0] * u[0] + u[1] * u[1];
        assert!((n - 1.0).abs() <= 4.0 * f64::EPSILON, "dir2 {k}: {n:e}");
        let next = DIRS2[(k + 1) % FACETS_2D];
        let cos = u[0] * next[0] + u[1] * next[1];
        assert!((cos - 0.9807852804032304).abs() <= 4.0 * f64::EPSILON, "dir2 {k}: {cos:e}");
        let opp = DIRS2[(k + 16) % FACETS_2D];
        assert_eq!([opp[0] + u[0], opp[1] + u[1]], [0.0, 0.0], "dir2 {k}");
    }
    for (k, u) in DIRS3.iter().enumerate() {
        let n = u[0] * u[0] + u[1] * u[1] + u[2] * u[2];
        assert!((n - 1.0).abs() <= 4.0 * f64::EPSILON, "dir3 {k}: {n:e}");
        for (j, w) in DIRS3.iter().enumerate().skip(k + 1) {
            assert_ne!(u, w, "dir3 {k} {j}");
        }
    }
    const { assert!(K2 > 0.995 && K2 < 0.9952) };
    const { assert!(K3 > 0.8864 && K3 < 0.8865) };
}

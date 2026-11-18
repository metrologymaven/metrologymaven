//! Reference fixtures for trimmed-face projection: spec node GEO.DIST.FACE.REF.
//!
//! The fixtures come from the independent oracle `tools/fixtures/face_proj_oracle.py`
//! (60-digit Decimal plus exact rational trim tests); nothing here computes expected values.
//! GEO.DIST.FACE.ONE and GEO.DIST.FACE.BODY add their projectors to `compare` with the same
//! 1e-9 m tolerance (REQ-GEO.DIST.FACE.REF-005).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_geo_dist::FACE_REF_FOOT_TOL_M;
use fid_testkit::sha256_hex;

const ANALYTIC: [&str; 5] = ["cube.json", "tube.json", "sphere_cap.json", "torus_segment.json", "cone_frustum.json"];
const SPECIAL: [&str; 2] = ["annulus_trap.json", "saddle_two_minima.json"];

fn all() -> Vec<Fixture> {
    manifest_files().iter().map(|f| load(f)).collect()
}

/// AC-1 (Rust side): every file matches the SHA-256, seed and oracle version in the manifest,
/// and the set holds the five analytic bodies, NURBS bodies and the two named special files.
#[test]
fn ac1_manifest_matches_files() {
    let man = parse_json(&std::fs::read_to_string(format!("{FIX}manifest.json")).unwrap());
    let version = man.get("oracle_version").str().to_string();
    assert_eq!(man.get("seed").num(), 31.0);
    let files = man.get("files").arr();
    for f in files {
        let path = f.get("path").str();
        let bytes = std::fs::read(format!("{FIX}{path}")).unwrap();
        assert_eq!(sha256_hex(&bytes), f.get("sha256").str(), "{path}: SHA-256 differs from manifest");
        assert_eq!(f.get("oracle_version").str(), version);
        let doc = parse_json(std::str::from_utf8(&bytes).unwrap());
        assert_eq!(doc.get("oracle_version").str(), version, "{path}");
        assert_eq!(doc.get("body_seed").num(), f.get("seed").num(), "{path}");
        assert_eq!(doc.get("queries").arr().len() as f64, f.get("queries").num(), "{path}");
    }
    let names: Vec<&str> = files.iter().map(|f| f.get("path").str()).collect();
    for want in ANALYTIC.iter().chain(SPECIAL.iter()) {
        assert!(names.contains(want), "{want} missing from manifest");
    }
    let nurbs = all().iter().filter(|fx| !fx.analytic).count();
    assert!(nurbs >= 3, "need trimmed NURBS bodies, found {nurbs}");
    let rational = all()
        .iter()
        .flat_map(|fx| fx.body.get("faces").arr().to_vec())
        .filter(|f| f.get("surface").get("type").str() == "nurbs" && !f.get("surface").get("weights").is_null())
        .count();
    assert!(rational >= 1, "need a rational NURBS face");
}

/// REQ-002 and internal consistency: hex and decimal agree (checked while loading), the
/// distance is |p - foot|, IDs exist and belong to the face, analytic feet lie on their
/// surface, and the trim-ignoring distance is never longer than the true one.
#[test]
fn fixtures_are_self_consistent() {
    for fx in all() {
        let body = &fx.body;
        let surfs = faces(body);
        let mut face_edges: Vec<(String, Vec<String>)> = Vec::new();
        for f in body.get("faces").arr() {
            let mut es = Vec::new();
            for l in f.get("loops").arr() {
                for c in l.get("coedges").arr() {
                    es.push(c.get("edge").str().to_string());
                }
            }
            face_edges.push((f.get("label").str().to_string(), es));
        }
        let edge_vertices = |e: &str| -> Vec<String> {
            let ej = body.get("edges").arr().iter().find(|x| x.get("label").str() == e).unwrap();
            vec![ej.get("start").str().to_string(), ej.get("end").str().to_string()]
        };
        assert!(!fx.queries.is_empty());
        for q in &fx.queries {
            for a in std::iter::once(&q.answer).chain(q.alternatives.iter()) {
                let ctx = format!("{} {}", fx.file, q.id);
                let d = norm(sub(q.p, a.foot));
                assert!((d - a.distance).abs() <= 1e-12 + 1e-12 * d, "{ctx}: distance {} vs |p-foot| {d}", a.distance);
                let (_, es) = face_edges.iter().find(|(l, _)| *l == a.face).expect("face exists");
                match &a.feature {
                    Feature::Interior => {}
                    Feature::Edge(e) => assert!(es.contains(e), "{ctx}: edge {e} not on face {}", a.face),
                    Feature::Vertex(v) => {
                        assert!(es.iter().any(|e| edge_vertices(e).contains(v)), "{ctx}: vertex {v} not on {}", a.face)
                    }
                }
                let s = &surfs.iter().find(|(l, _)| *l == a.face).unwrap().1;
                if let Some(s) = s {
                    assert!(s.residual(a.foot) <= 1e-12, "{ctx}: foot off surface by {}", s.residual(a.foot));
                }
                assert!((a.distance - q.answer.distance).abs() <= 1e-12, "{ctx}: alternative not a tie");
            }
            assert!(q.untrimmed_distance <= q.answer.distance + 1e-15, "{} {}", fx.file, q.id);
            let ud = norm(sub(q.p, q.untrimmed_foot));
            assert!((ud - q.untrimmed_distance).abs() <= 1e-12 + 1e-12 * ud, "{} {}", fx.file, q.id);
        }
    }
}

/// The comparison accepts the reference answers (and their listed ties) and rejects a foot
/// moved by more than 1e-9 m, but not one moved by less (REQ-005 tolerance).
#[test]
fn comparison_tolerance_is_1e9() {
    for fx in all() {
        assert!(compare(&fx, &reference).is_empty(), "{}: reference answers rejected", fx.file);
        let alt = |_: &Fixture, q: &Query| -> Hit {
            let a = q.alternatives.last().unwrap_or(&q.answer);
            Hit { face: a.face.clone(), feature: a.feature.clone(), foot: a.foot, distance: a.distance }
        };
        assert!(compare(&fx, &alt).is_empty(), "{}: alternatives rejected", fx.file);
        let shift = |s: f64| {
            move |fx: &Fixture, q: &Query| -> Hit {
                let mut h = reference(fx, q);
                h.foot[0] += s;
                h
            }
        };
        assert!(compare(&fx, &shift(0.9 * FACE_REF_FOOT_TOL_M)).is_empty(), "{}", fx.file);
        assert_eq!(compare(&fx, &shift(1.5 * FACE_REF_FOOT_TOL_M)).len(), fx.queries.len(), "{}", fx.file);
    }
}

/// AC-3: a projector that ignores trimming fails the comparison on every body, on every query
/// whose untrimmed minimum lies outside the trim.
#[test]
fn ac3_trim_ignoring_double_fails() {
    for fx in all() {
        let bad = compare(&fx, &trim_ignoring);
        let trimmed: Vec<&Query> = fx.queries.iter().filter(|q| q.has("trimmed_away")).collect();
        assert!(trimmed.len() >= 50, "{}", fx.file);
        for q in &trimmed {
            assert!(bad.contains(&q.id), "{} {}: trim-ignoring double passed", fx.file, q.id);
        }
        if fx.analytic {
            // The closed-form double computed here agrees with the oracle's own trim-ignoring answer.
            for q in &fx.queries {
                let h = trim_ignoring(&fx, q);
                assert!((h.distance - q.untrimmed_distance).abs() <= 1e-12, "{} {}", fx.file, q.id);
            }
        }
    }
}

/// AC-4 / REQ-003: at least 50 trimmed-away and 50 near-edge queries per body, recounted from
/// the numbers.
#[test]
fn ac4_coverage_counts() {
    for fx in all() {
        let trimmed = fx.queries.iter().filter(|q| q.untrimmed_distance < q.answer.distance - 1e-9).count();
        let near = fx.queries.iter().filter(|q| q.edge_distance.is_some_and(|d| d <= 1e-6)).count();
        assert!(trimmed >= 50, "{}: {trimmed} trimmed-away queries", fx.file);
        assert!(near >= 50, "{}: {near} near-edge queries", fx.file);
        for q in &fx.queries {
            assert_eq!(q.has("trimmed_away"), q.untrimmed_distance < q.answer.distance - 1e-9, "{}", q.id);
        }
        let gens = ["random", "trimmed_away", "near_edge", "near_vertex"];
        for g in gens {
            assert!(fx.queries.iter().any(|q| q.gen == g), "{}: no {g} queries", fx.file);
        }
        if faces(&fx.body).len() >= 2 {
            assert!(fx.queries.iter().any(|q| q.has("equidistant")), "{}: no equidistant queries", fx.file);
        }
    }
}

#[test]
fn hex_float_parser() {
    for x in [0.0, -0.0, 1.0, 0.1, -2.5e-300, 5e-324, f64::MAX, 1e-9, 0.125] {
        let s = python_hex(x);
        assert_eq!(parse_hex_f64(&s).to_bits(), x.to_bits(), "{s}");
    }
    assert_eq!(parse_hex_f64("0x1.999999999999ap-4").to_bits(), 0.1f64.to_bits());
    assert_eq!(parse_hex_f64("-0x1.8000000000000p+1").to_bits(), (-3.0f64).to_bits());
}

/// Python-style `float.hex()` spelling, for the round trip above.
fn python_hex(x: f64) -> String {
    let b = x.to_bits();
    let sign = if b >> 63 == 1 { "-" } else { "" };
    let e = ((b >> 52) & 0x7ff) as i32;
    let f = b & ((1u64 << 52) - 1);
    if e == 0 && f == 0 {
        format!("{sign}0x0.0p+0")
    } else if e == 0 {
        format!("{sign}0x0.{f:013x}p-1022")
    } else {
        format!("{sign}0x1.{f:013x}p{:+}", e - 1023)
    }
}

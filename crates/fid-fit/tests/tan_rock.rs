//! Acceptance criteria of spec node KRN.ENV.TAN.ROCK (AC-1 .. AC-4) and the error paths.
//!
//! Expected reports come from `fixtures/fit/make_tan_rock_fixtures.py`: exact rational geometry
//! of the crowned and flat plates (stable contact sets by brute force), correctly rounded
//! objectives and normals, and a report oracle written from the spec's rules. CI runs these
//! tests on x86-64 Linux and Windows, ARM64 macOS and wasm32 under wasmtime against the same
//! expected bytes (REQ-005).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_fit::contract::{command_row, FitError, FittedFeature, Method, PointsView, Primitive};
use fid_fit::tan_rock::{
    check_defined, encode_report, is_defined, rock_report, CandidateSet, MaterialSide, TanVariant, TieBreak,
    DEFINED_PAIRS, MAX_CANDIDATES,
};
use fid_num::NumError;
use fid_types::Determinism;

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fit/tan-rock-v1/cases.txt");
const CROWNED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fit/tan-rock-v1/crowned-plate.txt");
const FLAT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fit/tan-rock-v1/flat-plate.txt");

fn hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

struct Case {
    name: String,
    rock_tol: f64,
    cands: Vec<CandidateSet>,
    order: Vec<Vec<u32>>,
    expect: String,
}

fn contacts(s: &str) -> Vec<u32> {
    s.split(',').map(|c| c.parse().unwrap()).collect()
}

fn cases() -> Vec<Case> {
    let text = read(CASES);
    let mut out = Vec::new();
    let mut cur: Option<Case> = None;
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let (k, v) = line.split_once(' ').unwrap_or((line, ""));
        match k {
            "case" => {
                cur = Some(Case {
                    name: v.to_string(),
                    rock_tol: f64::NAN,
                    cands: Vec::new(),
                    order: Vec::new(),
                    expect: String::new(),
                })
            }
            "rock_tol" => cur.as_mut().unwrap().rock_tol = hex(v),
            "cand" => {
                let f: Vec<&str> = v.split(' ').collect();
                assert_eq!(f.len(), 9, "{line}");
                assert_eq!(f[2], "plane");
                let x: Vec<f64> = f[3..].iter().map(|s| hex(s)).collect();
                cur.as_mut().unwrap().cands.push(CandidateSet {
                    contacts: contacts(f[0]),
                    objective: hex(f[1]),
                    feature: FittedFeature::Plane { p: [x[0], x[1], x[2]], n: [x[3], x[4], x[5]] },
                });
            }
            "order" => cur.as_mut().unwrap().order = v.split(';').map(contacts).collect(),
            "expect" => cur.as_mut().unwrap().expect = v.to_string(),
            "end" => out.push(cur.take().unwrap()),
            _ => panic!("unknown line {line}"),
        }
    }
    assert_eq!(out.len(), 5);
    out
}

fn case(name: &str) -> Case {
    cases().into_iter().find(|c| c.name == name).unwrap()
}

fn bytes(cands: &[CandidateSet], tol: f64) -> String {
    String::from_utf8(encode_report(&rock_report(cands, tol).unwrap()).unwrap()).unwrap()
}

/// A deterministic permutation (xorshift64 Fisher-Yates), so the test is reproducible.
fn shuffled(v: &[CandidateSet], seed: u64) -> Vec<CandidateSet> {
    let mut out = v.to_vec();
    let mut s = seed | 1;
    for i in (1..out.len()).rev() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        let j = (s % (i as u64 + 1)) as usize;
        out.swap(i, j);
    }
    out
}

fn points(path: &str) -> Vec<f64> {
    read(path)
        .lines()
        .filter(|l| l.starts_with("p "))
        .flat_map(|l| l[2..].split(' ').map(hex).collect::<Vec<_>>())
        .collect()
}

#[test]
fn every_fixture_case_matches_the_oracle() {
    for c in cases() {
        let r = rock_report(&c.cands, c.rock_tol).unwrap();
        let got: Vec<Vec<u32>> = r.candidates.iter().map(|s| s.contacts.clone()).collect();
        assert_eq!(got, c.order, "{}", c.name);
        assert_eq!(String::from_utf8(encode_report(&r).unwrap()).unwrap(), c.expect, "{}", c.name);
    }
}

#[test]
fn ac1_crowned_plate_lists_four_sets_in_the_documented_order() {
    let c = case("crowned_l1");
    // The candidates refer to the crowned plate's points: 25 points, apex first.
    let xyz = points(CROWNED);
    let pts = PointsView::new(&xyz, None).unwrap();
    assert_eq!(pts.len(), 25);
    assert!(c.cands.iter().all(|s| s.contacts.iter().all(|&i| (i as usize) < pts.len())));

    let r = rock_report(&c.cands, c.rock_tol).unwrap();
    let got: Vec<&[u32]> = r.candidates.iter().map(|s| s.contacts.as_slice()).collect();
    // Hand-labelled: four facets with bit-equal objectives, ordered by contact indices.
    assert_eq!(got, vec![&[0, 1, 2][..], &[0, 1, 4], &[0, 2, 3], &[0, 3, 4]]);
    assert!(r.candidates.windows(2).all(|w| w[0].objective.to_bits() == w[1].objective.to_bits()));
    assert_eq!(r.selected, 0);
    assert_eq!(r.selected().unwrap().contacts, vec![0, 1, 2]);
    assert!(r.rocking);
    assert_eq!(r.tie_break, TieBreak::ContactOrder);
    assert_eq!((r.merged, r.excluded), (4, 0));
    // The kept duplicate of {0,2,3} is the converged one, not the one-ulp-worse start.
    assert_eq!(r.candidates[2].objective, c.cands[3].objective);
}

#[test]
fn ac2_shuffled_candidates_give_identical_bytes() {
    for c in cases() {
        let want = bytes(&c.cands, c.rock_tol);
        assert_eq!(want, c.expect, "{}", c.name);
        let mut rev = c.cands.clone();
        rev.reverse();
        assert_eq!(bytes(&rev, c.rock_tol), want, "{} reversed", c.name);
        for seed in 1..=24u64 {
            assert_eq!(bytes(&shuffled(&c.cands, seed), c.rock_tol), want, "{} seed {seed}", c.name);
        }
        // Permuting the contact lists inside each candidate changes nothing either.
        let mut inner = c.cands.clone();
        for s in &mut inner {
            s.contacts.reverse();
        }
        assert_eq!(bytes(&inner, c.rock_tol), want, "{} contacts reversed", c.name);
    }
}

#[test]
fn ac3_flat_plate_one_stable_set() {
    let c = case("flat_minimax");
    assert_eq!(points(FLAT).len(), 25 * 3);
    let r = rock_report(&c.cands, c.rock_tol).unwrap();
    assert_eq!(r.candidates.len(), 1);
    assert_eq!(r.candidates[0].contacts, vec![0, 1, 2, 3]);
    assert!(!r.rocking);
    assert_eq!(r.tie_break, TieBreak::Single);
    assert_eq!(r.merged, 3);
}

#[test]
fn ac4_undefined_pair_names_both() {
    let e = check_defined(Primitive::Sphere, TanVariant::ConstrainedL1).unwrap_err();
    assert_eq!(e, FitError::TanMethodUndefined { primitive: Primitive::Sphere, variant: TanVariant::ConstrainedL1 });
    assert_eq!(e.code(), "method_undefined");
    assert_eq!(e.to_string(), "fit: tangential variant constrained_l1 undefined for sphere");
    match e {
        FitError::TanMethodUndefined { primitive, variant } => {
            assert_eq!((primitive.slug(), variant.slug()), ("sphere", "constrained_l1"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn defined_pairs_table() {
    // Hand-labelled from the spec: plane, 2D line and cylinder in all three variants.
    let mut n = 0;
    for &p in Primitive::ALL {
        for &v in TanVariant::ALL {
            let want = matches!(p, Primitive::Plane | Primitive::Line2 | Primitive::Cylinder);
            assert_eq!(is_defined(p, v), want, "{} {}", p.slug(), v.slug());
            assert_eq!(check_defined(p, v).is_ok(), want);
            n += usize::from(want);
        }
    }
    assert_eq!(n, 9);
    assert_eq!(DEFINED_PAIRS.len(), 9);
    assert_eq!(
        check_defined(Primitive::Line3, TanVariant::ExternalMinimax),
        Err(FitError::TanMethodUndefined { primitive: Primitive::Line3, variant: TanVariant::ExternalMinimax })
    );
}

#[test]
fn variant_names_and_required_variant() {
    let slugs: Vec<&str> = TanVariant::ALL.iter().map(|v| v.slug()).collect();
    assert_eq!(slugs, ["external_minimax", "constrained_l2", "constrained_l1"]);
    let segs: Vec<&str> = TanVariant::ALL.iter().map(|v| v.method_segment()).collect();
    assert_eq!(segs, ["tan_minimax", "tan_l2", "tan_l1"]);
    for &v in TanVariant::ALL {
        assert_eq!(TanVariant::parse(v.slug()), Some(v));
        assert_eq!(v.to_string(), v.slug());
    }
    assert_eq!(TanVariant::parse("minimax"), None);
    assert_eq!(TanVariant::require(None), Err(FitError::InvalidParams { field: "variant" }));
    assert_eq!(TanVariant::require(Some(TanVariant::ConstrainedL2)), Ok(TanVariant::ConstrainedL2));
}

#[test]
fn material_side() {
    let xyz = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    let normals = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
    let bare = PointsView::new(&xyz, None).unwrap();
    let with = PointsView::new(&xyz, Some(&normals)).unwrap();
    assert_eq!(MaterialSide::PointNormals.validate(&with), Ok(()));
    assert_eq!(MaterialSide::PointNormals.validate(&bare), Err(FitError::InvalidParams { field: "material_side" }));
    // The sign is kept: a downward side stays downward.
    assert_eq!(MaterialSide::outward([0.0, 0.0, -2.0]), Ok(MaterialSide::Outward([0.0, 0.0, -1.0])));
    assert_eq!(MaterialSide::outward([0.0, 0.0, 0.0]), Err(FitError::InvalidDirection));
    assert!(matches!(MaterialSide::outward([f64::NAN, 0.0, 1.0]), Err(FitError::Num(_))));
    assert_eq!(MaterialSide::outward([0.0, 0.0, -2.0]).unwrap().validate(&bare), Ok(()));
    assert_eq!(
        MaterialSide::Outward([0.0, 0.0, 2.0]).validate(&bare),
        Err(FitError::InvalidParams { field: "material_side" })
    );
    assert!(matches!(MaterialSide::Outward([0.0, f64::INFINITY, 0.0]).validate(&bare), Err(FitError::Num(_))));
}

fn cand(contacts: &[u32], objective: f64) -> CandidateSet {
    CandidateSet {
        contacts: contacts.to_vec(),
        objective,
        feature: FittedFeature::Plane { p: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] },
    }
}

#[test]
fn error_paths() {
    let ok = [cand(&[0, 1, 2], 1.0)];
    for tol in [f64::NAN, f64::INFINITY, -1e-9, 1.5, f64::NEG_INFINITY] {
        assert_eq!(rock_report(&ok, tol), Err(FitError::InvalidParams { field: "rock_tol" }), "{tol}");
    }
    assert_eq!(rock_report(&[], 0.0), Err(FitError::InvalidParams { field: "candidates" }));
    let many = vec![cand(&[0], 1.0); MAX_CANDIDATES + 1];
    assert_eq!(rock_report(&many, 0.0), Err(FitError::InvalidParams { field: "candidates" }));
    assert_eq!(rock_report(&[cand(&[], 1.0)], 0.0), Err(FitError::InvalidParams { field: "candidates.contacts" }));
    assert_eq!(
        rock_report(&[cand(&[0, 1, 2], 1.0), cand(&[3, 1, 3], 1.0)], 0.0),
        Err(FitError::InvalidParams { field: "candidates.contacts" })
    );
    assert_eq!(
        rock_report(&[cand(&[0], 1.0), cand(&[1], f64::NAN)], 0.0),
        Err(FitError::Num(NumError::NotANumber { op: "tan_rock.objective", index: Some(1) }))
    );
    assert_eq!(
        rock_report(&[cand(&[0], f64::INFINITY)], 0.0),
        Err(FitError::Num(NumError::Infinite { op: "tan_rock.objective", index: Some(0) }))
    );
    assert_eq!(
        rock_report(&[cand(&[0], -1e-300)], 0.0),
        Err(FitError::InvalidParams { field: "candidates.objective" })
    );
    let bad_feature = CandidateSet { feature: FittedFeature::Plane { p: [0.0; 3], n: [0.0; 3] }, ..cand(&[0], 1.0) };
    assert_eq!(rock_report(&[bad_feature], 0.0), Err(FitError::InvalidDirection));
    assert_eq!(
        rock_report(&[cand(&[0], f64::MAX)], 1.0),
        Err(FitError::Num(NumError::Infinite { op: "tan_rock.bound", index: None }))
    );
}

#[test]
fn rock_tol_edges_and_negative_zero() {
    // rock_tol = 0 keeps only bit-equal best objectives; -0.0 is stored as +0.0.
    let cands = [cand(&[1, 2, 3], 2.0), cand(&[0, 1, 2], f64::from_bits(2.0f64.to_bits() + 1)), cand(&[5], 2.0)];
    let r = rock_report(&cands, -0.0).unwrap();
    assert_eq!(r.rock_tol.to_bits(), 0);
    assert_eq!(r.candidates.len(), 2);
    assert_eq!(r.candidates[0].contacts, vec![1, 2, 3]);
    assert_eq!(r.candidates[1].contacts, vec![5]);
    assert_eq!((r.excluded, r.tie_break), (1, TieBreak::ContactOrder));
    // rock_tol = 1 keeps everything up to twice the best.
    let cands = [cand(&[0], 1.0), cand(&[1], 2.0), cand(&[2], f64::from_bits(2.0f64.to_bits() + 1))];
    let r = rock_report(&cands, 1.0).unwrap();
    assert_eq!(r.candidates.len(), 2);
    assert_eq!((r.excluded, r.tie_break), (1, TieBreak::Objective));
}

fn plane(contacts: &[u32], objective: f64, n: [f64; 3]) -> CandidateSet {
    CandidateSet { contacts: contacts.to_vec(), objective, feature: FittedFeature::Plane { p: [0.0; 3], n } }
}

#[test]
fn objective_tie_between_different_features_is_refused() {
    let want = Err(FitError::AmbiguousCandidate { first_contact: 0, contacts: 3 });
    // Opposite-sign normals (the two material sides) for one contact set, same objective bits.
    let up = plane(&[0, 1, 2], 1.0, [0.0, 0.0, 1.0]);
    let down = plane(&[2, 1, 0], 1.0, [0.0, 0.0, -1.0]);
    // Any other feature difference with a bit-equal objective is refused the same way.
    let tilt = plane(&[1, 0, 2], 1.0, [0.0, 0.6, 0.8]);
    let other = plane(&[5, 6], 0.5, [0.0, 0.0, 1.0]);
    for pair in [[&up, &down], [&down, &up], [&up, &tilt], [&tilt, &up]] {
        let input = vec![pair[0].clone(), other.clone(), pair[1].clone()];
        assert_eq!(rock_report(&input, 0.5), want);
        assert_eq!(rock_report(&input, 0.5).unwrap_err().code(), "ambiguous_candidate");
    }
    // Two ambiguous sets: the first in contact order is named, whatever the input order.
    let later = [plane(&[3, 4], 1.0, [0.0, 0.0, 1.0]), plane(&[4, 3], 1.0, [0.0, 0.0, -1.0])];
    let input = vec![later[0].clone(), up.clone(), later[1].clone(), down.clone()];
    assert_eq!(rock_report(&input, 0.5), want);
    let rev: Vec<_> = input.iter().rev().cloned().collect();
    assert_eq!(rock_report(&rev, 0.5), want);
    // Not ties: exact repeats merge, and a different feature with a worse objective loses.
    let worse = plane(&[0, 1, 2], f64::from_bits(1.0f64.to_bits() + 1), [0.0, 0.0, -1.0]);
    let r = rock_report(&[up.clone(), worse, up.clone()], 0.0).unwrap();
    assert_eq!((r.candidates.len(), r.merged), (1, 2));
    assert_eq!(r.candidates[0].feature, up.feature);
}

#[test]
fn contract_tangential_table_matches_defined_pairs() {
    for &p in Primitive::ALL {
        let any = TanVariant::ALL.iter().any(|&v| is_defined(p, v));
        assert_eq!(Method::Tangential.defined_for(p), any, "{}", p.slug());
    }
    assert!(Method::Tangential.defined_for(Primitive::Cylinder));
    assert!(!Method::Tangential.defined_for(Primitive::Line3));
    // fit.tan_line3@1 is no longer emitted; fit.tan_cylinder@1 is.
    assert_eq!(
        command_row(Primitive::Line3, Method::Tangential, (1, 0, 0), Determinism::D0).unwrap_err(),
        FitError::MethodUndefined { primitive: Primitive::Line3, method: Method::Tangential }
    );
    let row = command_row(Primitive::Cylinder, Method::Tangential, (1, 0, 0), Determinism::D0).unwrap();
    row.validate().unwrap();
    assert_eq!(row.command.as_str(), "fit.tan_cylinder@1");
    let names: Vec<String> = Primitive::ALL
        .iter()
        .filter_map(|&p| command_row(p, Method::Tangential, (1, 0, 0), Determinism::D0).ok())
        .map(|r| r.command.as_str().to_string())
        .collect();
    assert_eq!(names, ["fit.tan_line2@1", "fit.tan_plane@1", "fit.tan_cylinder@1"]);
}

/// Not a timing test (the budget is unmeasured): 1000 candidates in 250 contact sets, four
/// starts each, with objectives on a grid so some are bit-equal, give a well-formed report
/// that does not depend on the input order.
#[test]
fn smoke_1000_candidates() {
    let mut cands = Vec::new();
    for start in 0..4u32 {
        for set in 0..250u32 {
            // Contact sets {set, set + 1000, set + 2000}, listed in a start-dependent order.
            let mut contacts = vec![set, set + 1000, set + 2000];
            contacts.rotate_left(start as usize % 3);
            // Objective 1 + (set mod 50) / 1024 (exact), one ulp worse on starts 1 and 3.
            let base = 1.0 + f64::from(set % 50) / 1024.0;
            let objective = if start % 2 == 1 { f64::from_bits(base.to_bits() + 1) } else { base };
            cands.push(plane(&contacts, objective, [0.0, 0.0, 1.0]));
        }
    }
    assert_eq!(cands.len(), 1000);
    // rock_tol = 4/1024 keeps objective levels 0..=4 (bound 1 + 4/1024 exactly): 5 levels x 5 sets.
    let r = rock_report(&cands, 4.0 / 1024.0).unwrap();
    assert_eq!(r.merged, 750);
    assert_eq!(r.candidates.len(), 25);
    assert_eq!(r.excluded, 225);
    assert!(r.rocking);
    assert_eq!(r.tie_break, TieBreak::ContactOrder);
    let firsts: Vec<u32> = r.candidates.iter().map(|c| c.contacts[0]).collect();
    let mut want = Vec::new();
    for level in 0..5u32 {
        want.extend((0..5u32).map(|k| level + 50 * k));
    }
    assert_eq!(firsts, want);
    assert!(r
        .candidates
        .iter()
        .all(|c| c.objective.to_bits() == (1.0 + f64::from(c.contacts[0] % 50) / 1024.0).to_bits()));
    let want_bytes = encode_report(&r).unwrap();
    for seed in [7u64, 99, 12345] {
        assert_eq!(encode_report(&rock_report(&shuffled(&cands, seed), 4.0 / 1024.0).unwrap()).unwrap(), want_bytes);
    }
}

#[test]
fn polyline_feature_passes_through_unchanged() {
    let feature = FittedFeature::Polyline { vertices: vec![[0.0, 0.0, 0.0], [1.0, -0.0, 2.0]] };
    let c = CandidateSet { contacts: vec![1, 0], objective: 0.5, feature: feature.clone() };
    let r = rock_report(&[c], 0.25).unwrap();
    assert_eq!(r.candidates[0].feature, feature);
    let FittedFeature::Polyline { vertices } = &r.candidates[0].feature else { panic!() };
    assert_eq!(vertices[1][1].to_bits(), (-0.0f64).to_bits());
}

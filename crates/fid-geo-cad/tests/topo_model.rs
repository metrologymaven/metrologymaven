//! GEO.CAD.TOPO.MODEL acceptance tests.
//!
//! Expected encodings, digests, IDs and errors come from outside the Rust code:
//! `fixtures/geo/cad/topo/expected.json`, written by the independent stdlib Python reference
//! `fixtures/geo/cad/topo/make_topo_fixtures.py`; each invalid script also carries its
//! hand-labelled expected error.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_4;

use fid_geo_cad::topo::encode::MalformedReason;
use fid_geo_cad::{
    body_digest, decode, encode, Body, BodyBuilder, Curve2, CurveDef, CurveParams, DecodeError, EntityIx, EntityKind,
    Frame, Label, Sense, SurfaceDef, SurfaceParams, TopoBuildError, TopologyId,
};
use fid_geo_nurbs::{NurbsCurve, NurbsCurveParts, NurbsSurface, NurbsSurfaceParts};

mod common;

use common::*;

fn id_text(id: &TopologyId) -> String {
    match id {
        TopologyId::Imported(l) => l.as_str().to_string(),
        TopologyId::Derived(_) => id.to_string(),
    }
}

fn ids_by_kind(body: &Body) -> BTreeMap<&'static str, Vec<String>> {
    let mut m = BTreeMap::new();
    m.insert("shells", body.shells().iter().map(|x| id_text(x.id())).collect());
    m.insert("faces", body.faces().iter().map(|x| id_text(x.id())).collect());
    m.insert("loops", body.loops().iter().map(|x| id_text(x.id())).collect());
    m.insert("coedges", body.coedges().iter().map(|x| id_text(x.id())).collect());
    m.insert("edges", body.edges().iter().map(|x| id_text(x.id())).collect());
    m.insert("vertices", body.vertices().iter().map(|x| id_text(x.id())).collect());
    m
}

fn check_against_expected(name: &str, body: &Body) {
    let exp = expected();
    let e = exp.get("valid").get(name);
    let enc = encode(body);
    assert_eq!(enc.len(), e.get("encoding_len").u(), "{name} length");
    assert_eq!(hex(&enc), e.get("encoding_hex").str(), "{name} encoding");
    assert_eq!(body_digest(body).as_str(), e.get("digest").str(), "{name} digest");
    let ids = ids_by_kind(body);
    for (kind, list) in e.get("ids").obj() {
        let want: Vec<String> = list.arr().iter().map(|x| x.str().to_string()).collect();
        assert_eq!(ids[kind.as_str()], want, "{name} {kind} ids");
    }
}

const VALID: [&str; 4] = ["cube_labelled", "cube_unlabelled", "cylinder_capped", "sphere_hemispheres"];

// ---------------------------------------------------------------- acceptance

/// AC-1: every labelled entity's ID is its label, and the digest matches.
#[test]
fn ac1_labelled_cube_ids_and_digest() {
    let script = load("cube_labelled.json");
    let body = builder(&script).build().unwrap();
    assert_eq!((body.vertices().len(), body.edges().len(), body.faces().len()), (8, 12, 6));
    assert_eq!((body.coedges().len(), body.loops().len(), body.shells().len()), (24, 6, 1));
    // Every builder label is the ID of exactly one entity of the right kind.
    for (plural, kind) in [
        ("vertices", EntityKind::Vertex),
        ("edges", EntityKind::Edge),
        ("coedges", EntityKind::Coedge),
        ("loops", EntityKind::Loop),
        ("faces", EntityKind::Face),
    ] {
        for ent in script.get(plural).arr() {
            let id = TopologyId::Imported(Label::new(ent.label().unwrap()).unwrap());
            let ix = body.lookup(&id).unwrap_or_else(|| panic!("{id} missing"));
            let got = match ix {
                EntityIx::Shell(_) => EntityKind::Shell,
                EntityIx::Face(_) => EntityKind::Face,
                EntityIx::Loop(_) => EntityKind::Loop,
                EntityIx::Coedge(_) => EntityKind::Coedge,
                EntityIx::Edge(_) => EntityKind::Edge,
                EntityIx::Vertex(_) => EntityKind::Vertex,
            };
            assert_eq!(got, kind);
            assert_eq!(body.id_of(ix), Some(&id));
        }
    }
    for f in body.faces() {
        assert!(matches!(f.id(), TopologyId::Imported(_)));
    }
    check_against_expected("cube_labelled", &body);
}

/// AC-2: the unlabelled cube builds twice to the same derived IDs and the committed bytes.
#[test]
fn ac2_unlabelled_cube_is_reproducible() {
    let a = build("cube_unlabelled");
    let b = build("cube_unlabelled");
    assert_eq!(a, b);
    assert_eq!(encode(&a), encode(&b));
    for f in a.faces() {
        assert!(matches!(f.id(), TopologyId::Derived(_)));
    }
    check_against_expected("cube_unlabelled", &a);
}

/// The other valid fixtures (analytic and NURBS geometry, seams, pcurves, -0.0 input).
#[test]
fn valid_fixtures_match_python_reference() {
    for name in VALID {
        check_against_expected(name, &build(name));
    }
}

/// AC-3, AC-4 and every invalid variant: the hand-labelled error, code and message.
#[test]
fn invalid_fixtures_fail_with_expected_error() {
    let exp = expected();
    let invalid = exp.get("invalid").obj();
    assert!(invalid.len() >= 15);
    for (name, e) in invalid {
        let script = load(&format!("invalid/{name}.json"));
        assert_eq!(script.get("expect").get("code").str(), e.get("code").str());
        let err = builder(&script).build().unwrap_err();
        assert_eq!(err.code(), e.get("code").str(), "{name}");
        assert_eq!(err.to_string(), e.get("message").str(), "{name}");
    }
}

#[test]
fn ac3_duplicate_id_names_label_and_paths() {
    let err = builder(&load("invalid/cube_duplicate_label.json")).build().unwrap_err();
    match err {
        TopoBuildError::DuplicateId { id, first, second } => {
            assert_eq!(id, TopologyId::Imported(Label::new("#12").unwrap()));
            assert_eq!((first.kind, first.steps), (EntityKind::Face, vec![0, 1]));
            assert_eq!((second.kind, second.steps), (EntityKind::Face, vec![0, 4]));
        }
        e => panic!("{e:?}"),
    }
}

#[test]
fn ac4_bad_reference_names_the_coedge() {
    let err = builder(&load("invalid/cube_bad_edge_ref.json")).build().unwrap_err();
    assert_eq!(
        err,
        TopoBuildError::BadReference { kind: EntityKind::Coedge, index: 5, target: EntityKind::Edge, target_index: 99 }
    );
}

/// SplitMix64, the corpus generator of AC-5 (seed 7).
struct SplitMix(u64);

impl SplitMix {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// AC-5: 200 truncated or bit-flipped copies of the AC-1 encoding fail closed, no panic.
#[test]
fn ac5_corrupted_copies_fail_closed() {
    let good = encode(&build("cube_labelled"));
    let mut rng = SplitMix(7);
    let mut panics = 0;
    for i in 0..200 {
        let mut bytes = good.clone();
        if i % 2 == 0 {
            bytes.truncate(rng.below(good.len()));
        } else {
            let bit = rng.below(good.len() * 8);
            bytes[bit / 8] ^= 1 << (bit % 8);
        }
        match std::panic::catch_unwind(|| decode(&bytes)) {
            Ok(Err(DecodeError::Malformed { .. } | DecodeError::TooLarge { .. })) => {}
            Ok(other) => panic!("copy {i}: {other:?}"),
            Err(_) => panics += 1,
        }
    }
    assert_eq!(panics, 0);
}

/// Replaces the trailer so corruption reaches the parser and the build checks.
fn reseal(payload_and_trailer: &mut Vec<u8>) {
    let n = payload_and_trailer.len() - 32;
    payload_and_trailer.truncate(n);
    let t = fid_kci::kci::sha256(payload_and_trailer);
    payload_and_trailer.extend_from_slice(&t);
}

/// Bounded, full validation behind the checksum: bit flips with a valid trailer either fail
/// with a typed error or decode to a body whose encoding is exactly the input.
#[test]
fn resealed_corruption_never_yields_a_noncanonical_body() {
    for name in VALID {
        let good = encode(&build(name));
        let mut rng = SplitMix(7);
        for _ in 0..600 {
            let mut bytes = good.clone();
            let bit = rng.below((good.len() - 32) * 8);
            bytes[bit / 8] ^= 1 << (bit % 8);
            reseal(&mut bytes);
            match decode(&bytes) {
                Ok(body) => assert_eq!(encode(&body), bytes),
                Err(DecodeError::Malformed { .. } | DecodeError::TooLarge { .. }) => {}
                Err(DecodeError::UnsupportedVersion { version }) => assert_ne!(version, 1),
            }
        }
    }
}

/// AC-6: encode, decode, encode is byte-identical and the decoded body equals the built one.
#[test]
fn ac6_round_trip() {
    for name in VALID {
        let body = build(name);
        let enc = encode(&body);
        let back = decode(&enc).unwrap();
        assert_eq!(back, body, "{name}");
        assert_eq!(encode(&back), enc, "{name}");
        assert_eq!(body_digest(&back), body_digest(&body));
    }
}

#[test]
fn ids_iterate_imported_first_then_bytewise() {
    let body = build("cube_labelled");
    let ids: Vec<&TopologyId> = body.ids().keys().collect();
    // 56 labels, then the one derived shell ID.
    assert_eq!(ids.len(), 57);
    assert!(ids[..56].iter().all(|i| matches!(i, TopologyId::Imported(_))));
    assert!(matches!(ids[56], TopologyId::Derived(_)));
    // Bytewise: "#10" sorts before "#2".
    let pos = |s: &str| ids.iter().position(|i| **i == TopologyId::Imported(Label::new(s).unwrap())).unwrap();
    assert!(pos("#10") < pos("#2"));
}

// ---------------------------------------------------------------- decoder edge cases

fn header(version: u8, counts: [u64; 6]) -> Vec<u8> {
    let mut v = b"FIDBREP".to_vec();
    v.push(version);
    v.extend_from_slice(&[0; 16]);
    for c in counts {
        v.extend_from_slice(&c.to_le_bytes());
    }
    v.extend_from_slice(&[0; 32]);
    reseal(&mut v);
    v
}

#[test]
fn decoder_limits_and_version() {
    assert!(matches!(decode(&header(1, [0, 0, 0, 0, 0, 10_000_001])), Err(DecodeError::TooLarge { .. })));
    assert!(matches!(decode(&header(1, [u64::MAX, 1, 0, 0, 0, 0])), Err(DecodeError::TooLarge { .. })));
    assert!(matches!(
        decode(&header(1, [0, 0, 0, 0, 0, 1000])),
        Err(DecodeError::Malformed { reason: MalformedReason::Truncated, .. })
    ));
    assert_eq!(decode(&header(2, [0; 6])), Err(DecodeError::UnsupportedVersion { version: 2 }));
    // The empty body is valid and canonical.
    let empty = decode(&header(1, [0; 6])).unwrap();
    assert_eq!(empty.entity_count(), 0);
    assert_eq!(encode(&empty), header(1, [0; 6]));
    assert!(matches!(decode(&[]), Err(DecodeError::Malformed { .. })));
    let mut bad_magic = header(1, [0; 6]);
    bad_magic[0] = b'X';
    reseal(&mut bad_magic);
    assert!(matches!(decode(&bad_magic), Err(DecodeError::Malformed { reason: MalformedReason::Magic, .. })));
}

/// Offset of the first vertex point of the cylinder encoding (its last entity, so the last
/// 24 bytes before the trailer are its point).
fn last_point_offset(enc: &[u8]) -> usize {
    enc.len() - 32 - 24
}

#[test]
fn decoder_refuses_nan_infinity_and_negative_zero() {
    let good = encode(&build("cylinder_capped"));
    for bits in [f64::NAN.to_bits(), f64::INFINITY.to_bits(), (-0.0f64).to_bits(), 0x7ff0_0000_0000_0001] {
        let mut bytes = good.clone();
        let at = last_point_offset(&bytes);
        bytes[at..at + 8].copy_from_slice(&bits.to_le_bytes());
        reseal(&mut bytes);
        assert!(
            matches!(decode(&bytes), Err(DecodeError::Malformed { reason: MalformedReason::BadFloat, offset }) if offset == at),
            "{bits:x}"
        );
    }
}

#[test]
fn decoder_refuses_non_canonical_order() {
    // Swap the two vertices of the cylinder: still a valid body, but not in canonical order.
    let body = build("cylinder_capped");
    let good = encode(&body);
    assert_eq!(body.vertices().len(), 2);
    let mut b = BodyBuilder::new(*body.id());
    for v in body.vertices().iter().rev() {
        b.add_vertex(v.point(), None);
    }
    for e in body.edges() {
        b.add_edge(e.curve().cloned(), 1 - e.start().index(), 1 - e.end().index(), e.param_range(), None);
    }
    for c in body.coedges() {
        b.add_coedge(c.edge().index(), c.sense(), c.pcurve().cloned(), None);
    }
    for l in body.loops() {
        b.add_loop(l.coedges().iter().map(|c| c.index()).collect(), l.outer(), None);
    }
    for f in body.faces() {
        b.add_face(f.surface().clone(), f.sense(), f.loops().iter().map(|l| l.index()).collect(), None);
    }
    for s in body.shells() {
        b.add_shell(s.faces().iter().map(|f| f.index()).collect(), s.closed());
    }
    // The builder canonicalises it back.
    assert_eq!(encode(&b.build().unwrap()), good);
    // A derived ID that is not the derivation of its path is refused: flip a bit of the
    // shell ID (tag at byte 72, UUID from byte 73).
    let mut bytes = good.clone();
    assert_eq!(bytes[72], 0);
    bytes[73] ^= 1;
    reseal(&mut bytes);
    match decode(&bytes) {
        Err(DecodeError::Malformed { reason: MalformedReason::NonCanonical, .. }) => {}
        other => panic!("{other:?}"),
    }
    // A shell may not carry an imported label.
    let mut bytes = good;
    bytes[72] = 1;
    reseal(&mut bytes);
    assert!(matches!(decode(&bytes), Err(DecodeError::Malformed { reason: MalformedReason::BadLabel, .. })));
}

// ---------------------------------------------------------------- build-time checks

fn plane() -> SurfaceDef {
    SurfaceDef::analytic(SurfaceParams::Plane {
        frame: Frame { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] },
    })
    .unwrap()
}

fn circle_edge(b: &mut BodyBuilder, v: usize) -> usize {
    let c = CurveDef::analytic(CurveParams::Circle {
        frame: Frame { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] },
        radius: 1.0,
    })
    .unwrap();
    b.add_edge(Some(c), v, v, [0.0, std::f64::consts::TAU], None)
}

#[test]
fn nan_and_infinity_are_refused_at_build() {
    let mut b = BodyBuilder::new([1; 16]);
    b.add_vertex([0.0, f64::NAN, 0.0], None);
    assert_eq!(b.build().unwrap_err(), TopoBuildError::NonFinite { kind: EntityKind::Vertex, index: 0 });
    let mut b = BodyBuilder::new([1; 16]);
    let v = b.add_vertex([1.0, 0.0, 0.0], None);
    b.add_edge(None, v, v, [0.0, f64::INFINITY], None);
    assert_eq!(b.build().unwrap_err(), TopoBuildError::NonFinite { kind: EntityKind::Edge, index: 0 });
    // GEO.NURBS refuses NaN in geometry.
    assert!(CurveDef::analytic(CurveParams::Circle {
        frame: Frame { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] },
        radius: f64::NAN,
    })
    .is_err());
    assert!(Curve2::line([0.0, f64::NAN], [1.0, 0.0]).is_err());
    assert!(Curve2::line([0.0, 0.0], [0.0, 0.0]).is_err());
}

#[test]
fn negative_zero_is_stored_as_positive_zero() {
    let a = build("cube_unlabelled");
    let mut script = load("cube_unlabelled.json");
    if let J::Obj(m) = &mut script {
        if let Some(J::Arr(vs)) = m.get_mut("vertices") {
            if let J::Obj(v0) = &mut vs[0] {
                v0.insert("point".into(), J::Arr(vec![J::Num(-0.0), J::Num(-0.0), J::Num(-0.0)]));
            }
        }
    }
    let b = builder(&script).build().unwrap();
    assert_eq!(encode(&a), encode(&b));
    for v in b.vertices() {
        for x in v.point() {
            assert!(x.is_sign_positive());
        }
    }
}

#[test]
fn empty_shell_and_orphans() {
    let mut b = BodyBuilder::new([2; 16]);
    b.add_shell(vec![], true);
    assert_eq!(b.build().unwrap_err(), TopoBuildError::EmptyShell { shell: 0 });

    let mut b = BodyBuilder::new([2; 16]);
    b.add_face(plane(), Sense::Same, vec![], None);
    assert_eq!(b.build().unwrap_err(), TopoBuildError::Orphan { kind: EntityKind::Face, index: 0 });

    let mut b = BodyBuilder::new([2; 16]);
    let v = b.add_vertex([1.0, 0.0, 0.0], None);
    circle_edge(&mut b, v);
    let f = b.add_face(plane(), Sense::Same, vec![], None);
    b.add_shell(vec![f], false);
    assert_eq!(b.build().unwrap_err(), TopoBuildError::Orphan { kind: EntityKind::Edge, index: 0 });
}

#[test]
fn label_is_untrusted_data() {
    // Any UTF-8 is held verbatim and shown escaped.
    let mut b = BodyBuilder::new([3; 16]);
    let v = b.add_vertex([1.0, 0.0, 0.0], Some("a\u{7}\"\n"));
    let e = circle_edge(&mut b, v);
    let c = b.add_coedge(e, Sense::Same, None, Some("\u{1F600}"));
    let l = b.add_loop(vec![c], true, None);
    let f = b.add_face(plane(), Sense::Same, vec![l], Some("#12"));
    b.add_shell(vec![f], false);
    let body = b.build().unwrap();
    let id = body.vertices()[0].id().clone();
    assert_eq!(id.to_string(), "\"a\\u{7}\\\"\\n\"");
    assert_eq!(decode(&encode(&body)).unwrap(), body);
}

/// Every analytic type and a NURBS surface and curve survive the round trip.
#[test]
fn every_geometry_type_round_trips() {
    let fr = Frame { origin: [0.0, 1.0, -2.0], axis: [0.0, 0.0, 1.0], ref_dir: [1.0, 0.0, 0.0] };
    let surfaces = [
        SurfaceDef::analytic(SurfaceParams::Cylinder { frame: fr, radius: 2.0 }).unwrap(),
        SurfaceDef::analytic(SurfaceParams::Cone { frame: fr, radius: 0.0, half_angle: FRAC_PI_4 }).unwrap(),
        SurfaceDef::analytic(SurfaceParams::Sphere { frame: fr, radius: 3.0 }).unwrap(),
        SurfaceDef::analytic(SurfaceParams::Torus { frame: fr, major_radius: 5.0, minor_radius: 1.0 }).unwrap(),
        SurfaceDef::nurbs(
            NurbsSurface::new(NurbsSurfaceParts {
                degree_u: 1,
                degree_v: 1,
                knots_u: vec![0.0, 0.0, 1.0, 1.0],
                knots_v: vec![-0.0, -0.0, 2.0, 2.0],
                n_u: 2,
                n_v: 2,
                ctrl: vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, -0.0], [1.0, 1.0, 0.5]],
                weights: Some(vec![1.0, 2.0, 1.0, 1.0]),
                periodic_u: false,
                periodic_v: true,
            })
            .unwrap(),
        )
        .unwrap(),
    ];
    let ellipse = CurveDef::analytic(CurveParams::Ellipse { frame: fr, semi_axis_1: 2.0, semi_axis_2: 1.0 }).unwrap();
    let nurbs = CurveDef::nurbs(
        NurbsCurve::new(NurbsCurveParts {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            ctrl: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]],
            weights: None,
            periodic: false,
        })
        .unwrap(),
    )
    .unwrap();
    let mut b = BodyBuilder::new([4; 16]);
    let mut faces = Vec::new();
    for s in surfaces {
        faces.push(b.add_face(s, Sense::Reversed, vec![], None));
    }
    // A planar face bounded by an ellipse, and an open face bounded by a NURBS edge plus a line.
    let v0 = b.add_vertex([2.0, 1.0, -2.0], None);
    let e0 = b.add_edge(Some(ellipse), v0, v0, [0.0, 1.0], None);
    let c0 = b.add_coedge(e0, Sense::Same, None, None);
    let l0 = b.add_loop(vec![c0], true, None);
    faces.push(b.add_face(plane(), Sense::Same, vec![l0], None));
    let va = b.add_vertex([0.0, 0.0, 0.0], None);
    let vb = b.add_vertex([2.0, 0.0, 0.0], None);
    let e1 = b.add_edge(Some(nurbs), va, vb, [0.0, 1.0], None);
    let e2 = b.add_edge(None, vb, va, [0.0, 2.0], Some("chord"));
    let pc = Curve2::nurbs(
        NurbsCurve::new(NurbsCurveParts {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            ctrl: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            weights: None,
            periodic: false,
        })
        .unwrap(),
    )
    .unwrap();
    let c1 = b.add_coedge(e1, Sense::Same, Some(pc), None);
    let c2 = b.add_coedge(e2, Sense::Same, Some(Curve2::line([1.0, 0.0], [-1.0, 0.0]).unwrap()), None);
    let l1 = b.add_loop(vec![c1, c2], true, None);
    faces.push(b.add_face(plane(), Sense::Same, vec![l1], None));
    b.add_shell(faces, false);
    let body = b.build().unwrap();
    let enc = encode(&body);
    let back = decode(&enc).unwrap();
    assert_eq!(back, body);
    assert_eq!(encode(&back), enc);
    // -0.0 in the NURBS surface was stored as +0.0.
    let n = body.faces().iter().find_map(|f| f.surface().as_nurbs()).unwrap();
    assert!(n.knots_v().as_slice()[0].is_sign_positive());
    // A pcurve must be planar.
    let bent = NurbsCurve::new(NurbsCurveParts {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        ctrl: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 1.0]],
        weights: None,
        periodic: false,
    })
    .unwrap();
    assert!(Curve2::nurbs(bent).is_err());
}

/// Error types stay small (clippy `result_large_err` threshold is 128 bytes).
#[test]
fn error_types_are_small() {
    assert!(std::mem::size_of::<TopoBuildError>() < 128, "{}", std::mem::size_of::<TopoBuildError>());
    assert!(std::mem::size_of::<DecodeError>() < 128, "{}", std::mem::size_of::<DecodeError>());
    assert!(std::mem::size_of::<fid_geo_cad::GeomError>() < 128);
}

/// Two vertex records swapped in the raw bytes (IDs and points move together), trailer
/// resealed: the topology still builds, but the derived IDs no longer match their index paths.
#[test]
fn decoder_refuses_swapped_vertex_records() {
    let good = encode(&build("cylinder_capped"));
    // The last two records before the trailer: tag 0x00 + 16-byte UUID + 3 f64 = 41 bytes.
    let end = good.len() - 32;
    let (a, b) = (end - 82, end - 41);
    assert_eq!((good[a], good[b]), (0, 0));
    let mut bytes = good.clone();
    bytes[a..end].copy_from_slice(&[&good[b..end], &good[a..b]].concat());
    assert_ne!(bytes, good);
    reseal(&mut bytes);
    match decode(&bytes) {
        Err(DecodeError::Malformed { reason: MalformedReason::NonCanonical, .. }) => {}
        other => panic!("{other:?}"),
    }
}

/// A byte after the last vertex (trailer resealed) is `TrailingBytes`.
#[test]
fn decoder_refuses_trailing_bytes() {
    let mut bytes = header(1, [0; 6]);
    let at = bytes.len() - 32;
    bytes.insert(at, 0);
    reseal(&mut bytes);
    assert_eq!(decode(&bytes), Err(DecodeError::Malformed { offset: at, reason: MalformedReason::TrailingBytes }));
    let mut bytes = encode(&build("sphere_hemispheres"));
    let at = bytes.len() - 32;
    bytes.insert(at, 7);
    reseal(&mut bytes);
    assert_eq!(decode(&bytes), Err(DecodeError::Malformed { offset: at, reason: MalformedReason::TrailingBytes }));
}

/// A count above `u32::MAX` is `BadCount` at the same offset on 32- and 64-bit targets.
#[test]
fn decoder_count_above_u32_is_bad_count_everywhere() {
    let good = encode(&build("cylinder_capped"));
    // First shell record at 72: tag, 16-byte UUID, `closed`, then its face count at 90.
    assert_eq!(good[72], 0);
    for v in [1u64 << 32, u64::MAX] {
        let mut bytes = good.clone();
        bytes[90..98].copy_from_slice(&v.to_le_bytes());
        reseal(&mut bytes);
        assert_eq!(decode(&bytes), Err(DecodeError::Malformed { offset: 98, reason: MalformedReason::BadCount }));
    }
}

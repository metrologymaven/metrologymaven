//! Acceptance criteria of spec node GEO.SEL.MODEL (AC-1, AC-2, AC-3, AC-5; AC-4 is in
//! `tests/memory.rs`) plus requirement checks. Expected ranges, encodings and digests come
//! from `fixtures/geo/sel/sets.json`, written by the independent Python reference
//! `tools/fixtures/geo_sel_sets.py` (stdlib `hashlib`); none come from this crate. The
//! pointset_small element digest is cross-checked against
//! `fixtures/geo/types/digest_vectors.json` (independent `geo_digest_vectors.py`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_sel::model::{
    DIGEST_TAG, INDEX_SET_FIELDS, MAX_QUERY_BYTES, QUERY_FIELDS, SELECTION_FIELDS, SELECTION_TAG,
};
use fid_geo_sel::{
    apply, domain_count, Domain, EntityId, IndexSet, IndexSetBuilder, SelError, Selection, SelectionBody,
    SelectionQuery,
};
use fid_geo_types::{
    digest, Aabb, AttributeSet, CadBodyRef, ElementDigest, ElementKind, FrameRef, GeometryElement, Mesh, PointBuffer,
    PointCloud, PointSet,
};
use fid_types::Digest;
use std::collections::BTreeMap;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/");

fn read(path: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}{path}")).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex32(s: &str) -> [u8; 32] {
    assert_eq!(s.len(), 64);
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
    }
    out
}

/// The value of a `"key": value` line, without quotes and trailing comma.
fn value_of<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.trim().strip_prefix(&format!("\"{key}\": "))?;
    Some(rest.trim_end_matches(',').trim_matches('"'))
}

// ------------------------------------------------------------------ sets.json

const CASE_KEYS: [&str; 9] =
    ["bound", "count", "domain", "encoding_hex", "indexes", "name", "ranges", "sha256", "target_digest"];

/// The cases, read line by line (the generator writes sorted keys, one per line;
/// `target_digest` is the last key of a case).
fn cases() -> Vec<BTreeMap<&'static str, String>> {
    let text = read("geo/sel/sets.json");
    let mut out = Vec::new();
    let mut cur = BTreeMap::new();
    for line in text.lines() {
        for key in CASE_KEYS {
            if let Some(v) = value_of(line, key) {
                assert!(cur.insert(key, v.to_string()).is_none(), "duplicate key {key}");
                if key == "target_digest" {
                    assert_eq!(cur.len(), CASE_KEYS.len());
                    out.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    assert!(cur.is_empty());
    assert_eq!(out.len(), 19, "case count");
    out
}

fn case(name: &str) -> BTreeMap<&'static str, String> {
    cases().into_iter().find(|c| c["name"] == name).unwrap()
}

fn domain_of(s: &str) -> Domain {
    Domain::ALL.into_iter().find(|d| d.as_str() == s).unwrap()
}

fn indexes_of(s: &str) -> Vec<u64> {
    s.split_whitespace().map(|t| t.parse().unwrap()).collect()
}

fn ranges_of(s: &str) -> Vec<(u64, u64)> {
    s.split_whitespace()
        .map(|t| {
            let (a, b) = t.split_once(':').unwrap();
            (a.parse().unwrap(), b.parse().unwrap())
        })
        .collect()
}

// ------------------------------------------------------------------ pointset_small

fn small_points() -> (FrameRef, PointBuffer) {
    let mut frame = None;
    let mut buf = PointBuffer::default();
    let f64_hex = |s: &str| f64::from_bits(u64::from_str_radix(s, 16).unwrap());
    for line in read("geo/types/pointset_small.txt").lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        match f[0] {
            "frame" => {
                let mut b = [0u8; 16];
                for (i, o) in b.iter_mut().enumerate() {
                    *o = u8::from_str_radix(&f[1][2 * i..2 * i + 2], 16).unwrap();
                }
                frame = Some(FrameRef::from_bytes(b));
            }
            "p" => {
                assert_eq!(f[1].parse::<usize>().unwrap(), buf.x.len());
                buf.x.push(f64_hex(f[2]));
                buf.y.push(f64_hex(f[3]));
                buf.z.push(f64_hex(f[4]));
            }
            "count" | "aabb_min" | "aabb_max" => {}
            other => panic!("unknown line kind {other}"),
        }
    }
    assert_eq!(buf.x.len(), 1000);
    (frame.unwrap(), buf)
}

fn pointset_small() -> GeometryElement {
    let (frame, buf) = small_points();
    GeometryElement::PointSet(PointSet::new(frame, buf, AttributeSet::default()).unwrap())
}

fn pointset_small_vector_digest() -> ElementDigest {
    let text = read("geo/types/digest_vectors.json");
    let mut name = None;
    for line in text.lines() {
        if let Some(v) = value_of(line, "name") {
            name = Some(v.to_string());
        } else if let Some(v) = value_of(line, "sha256") {
            if name.as_deref() == Some("pointset_small") {
                return ElementDigest(unhex32(v));
            }
        }
    }
    panic!("pointset_small vector missing")
}

const ID: EntityId = EntityId::from_bytes([0x5e; 16]);

fn unit_cube_mesh() -> GeometryElement {
    let v = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let t = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
    GeometryElement::Mesh(Mesh::from_points(FrameRef::from_bytes([9; 16]), &v, &t).unwrap())
}

fn cad_body() -> GeometryElement {
    let aabb = Aabb::new([0.0; 3], [1.0; 3]).unwrap();
    GeometryElement::CadBody(CadBodyRef::new(FrameRef::from_bytes([4; 16]), [8; 16], Digest::from_bytes([3; 32]), aabb))
}

// ------------------------------------------------------------------ AC-1

/// AC-1 / REQ-001 / REQ-002: shuffled lists with duplicates give the reference ranges,
/// encodings and digests exactly. CI runs this on x86-64 (Linux, Windows), ARM64 (macOS)
/// and wasm32-wasip1.
#[test]
fn ac1_encodings_and_digests_match_reference() {
    for c in cases() {
        let name = &c["name"];
        let domain = domain_of(&c["domain"]);
        let bound: u64 = c["bound"].parse().unwrap();
        let target = ElementDigest(unhex32(&c["target_digest"]));
        let set = IndexSet::from_indices(domain, &indexes_of(&c["indexes"]), bound).unwrap();
        assert_eq!(set.ranges(), ranges_of(&c["ranges"]).as_slice(), "{name} ranges");
        assert_eq!(set.len().to_string(), c["count"], "{name} count");
        assert_eq!(hex(&set.encoding(target)), c["encoding_hex"], "{name} encoding");
        assert_eq!(set.digest(target).to_string(), c["sha256"], "{name} digest");
        assert_eq!(&set.encoding(target)[..6], &DIGEST_TAG);
    }
}

/// REQ-001: same members, any order or multiplicity or construction path, same set.
#[test]
fn same_members_same_encoding_whatever_the_input() {
    for c in cases() {
        let domain = domain_of(&c["domain"]);
        let bound: u64 = c["bound"].parse().unwrap();
        let target = ElementDigest(unhex32(&c["target_digest"]));
        let listed = indexes_of(&c["indexes"]);
        let a = IndexSet::from_indices(domain, &listed, bound).unwrap();
        let mut sorted = listed.clone();
        sorted.sort_unstable();
        sorted.dedup();
        let mut rev = sorted.clone();
        rev.reverse();
        let mut doubled = listed.clone();
        doubled.extend_from_slice(&rev);
        for other in [&sorted, &rev, &doubled] {
            let b = IndexSet::from_indices(domain, other, bound).unwrap();
            assert_eq!(a, b);
            assert_eq!(a.digest(target), b.digest(target));
        }
        // From the reference ranges, split into unit ranges and shuffled by reversal.
        let mut units: Vec<(u64, u64)> = sorted.iter().map(|&i| (i, i + 1)).collect();
        units.reverse();
        assert_eq!(IndexSet::from_ranges(domain, &units, bound).unwrap(), a);
        assert_eq!(IndexSet::from_ranges(domain, &ranges_of(&c["ranges"]), bound).unwrap(), a);
        // Membership by binary search agrees with the explicit list.
        if bound <= 5000 {
            for i in 0..bound {
                assert_eq!(a.contains(i), sorted.binary_search(&i).is_ok(), "{} member {i}", c["name"]);
            }
        }
        assert_eq!(a.iter().collect::<Vec<_>>(), sorted);
        assert_eq!(a.max_index(), sorted.last().copied());
    }
}

/// The digest binds the domain and the target element digest.
#[test]
fn digest_binds_domain_and_target() {
    let t1 = ElementDigest([1; 32]);
    let t2 = ElementDigest([2; 32]);
    let p = IndexSet::from_indices(Domain::Points, &[1, 2, 3], 10).unwrap();
    let t = IndexSet::from_indices(Domain::Triangles, &[1, 2, 3], 10).unwrap();
    assert_ne!(p.digest(t1), t.digest(t1));
    assert_ne!(p.digest(t1), p.digest(t2));
    assert_eq!(p.digest(t1).to_digest().to_string(), format!("sha256:{}", p.digest(t1)));
}

// ------------------------------------------------------------------ AC-2

/// AC-2 / REQ-003: a selection on pointset_small applies to the same element and is stale
/// on a copy with one coordinate changed.
#[test]
fn ac2_apply_and_stale_detection() {
    let element = pointset_small();
    let d = digest(&element);
    assert_eq!(d, pointset_small_vector_digest(), "element digest vs independent vector");
    let c = case("pointset_small");
    assert_eq!(c["target_digest"], d.to_string());
    let listed = indexes_of(&c["indexes"]);
    let sel = Selection::on_element(ID, &element, Domain::Points, &listed).unwrap();
    assert_eq!(sel.target(), ID);
    assert_eq!(sel.target_digest(), d);
    assert_eq!(sel.content_digest().unwrap().to_string(), c["sha256"]);

    let members = apply(&sel, &element).unwrap();
    assert_eq!(members.len(), 334);
    assert_eq!(members.iter().collect::<Vec<_>>(), (0..1000).step_by(3).collect::<Vec<u64>>());

    let (frame, mut buf) = small_points();
    buf.y[417] = f64::from_bits(buf.y[417].to_bits() + 1);
    let changed = GeometryElement::PointSet(PointSet::new(frame, buf, AttributeSet::default()).unwrap());
    let found = digest(&changed);
    assert_ne!(found, d);
    assert_eq!(apply(&sel, &changed), Err(SelError::StaleSelection { expected: d, found }));
    assert_eq!(apply(&sel, &changed).unwrap_err().code(), "stale_selection");
}

/// A point cloud with the same points is a different element (kind byte), so stale too.
#[test]
fn same_points_other_kind_is_stale() {
    let element = pointset_small();
    let sel = Selection::on_element(ID, &element, Domain::Points, &[0, 1]).unwrap();
    let (frame, buf) = small_points();
    let cloud = GeometryElement::PointCloud(PointCloud::new(frame, buf, AttributeSet::default()).unwrap());
    assert!(matches!(apply(&sel, &cloud), Err(SelError::StaleSelection { .. })));
}

// ------------------------------------------------------------------ AC-3

/// AC-3 / REQ-004: index 1000 on a 1 000-point element is out of range.
#[test]
fn ac3_index_out_of_range() {
    let element = pointset_small();
    let err = Selection::on_element(ID, &element, Domain::Points, &[3, 1000, 2]).unwrap_err();
    assert_eq!(err, SelError::IndexOutOfRange { index: 1000, count: 1000 });
    assert_eq!(err.code(), "index_out_of_range");
    assert!(Selection::on_element(ID, &element, Domain::Points, &[999]).is_ok());
    assert_eq!(
        IndexSet::from_indices(Domain::Points, &[1000], 1000),
        Err(SelError::IndexOutOfRange { index: 1000, count: 1000 })
    );
    assert_eq!(
        IndexSet::from_ranges(Domain::Points, &[(990, 1001)], 1000),
        Err(SelError::IndexOutOfRange { index: 1000, count: 1000 })
    );
    let mut b = IndexSetBuilder::new(Domain::Points, 0);
    assert_eq!(b.push(0), Err(SelError::IndexOutOfRange { index: 0, count: 0 }));
    assert_eq!(IndexSet::from_ranges(Domain::Points, &[(5, 5)], 10), Err(SelError::InvalidRange { start: 5, end: 5 }));
    assert_eq!(IndexSet::from_ranges(Domain::Points, &[(6, 5)], 10), Err(SelError::InvalidRange { start: 6, end: 5 }));
}

/// Triangles index a mesh by triangle count; vertices are its points domain.
#[test]
fn mesh_domains_and_bounds() {
    let mesh = unit_cube_mesh();
    assert_eq!(domain_count(&mesh, Domain::Triangles), Ok(Some(4)));
    assert_eq!(domain_count(&mesh, Domain::Points), Ok(Some(4)));
    let sel = Selection::on_element(ID, &mesh, Domain::Triangles, &[3, 0]).unwrap();
    assert_eq!(apply(&sel, &mesh).unwrap().ranges(), &[(0, 1), (3, 4)]);
    assert_eq!(
        Selection::on_element(ID, &mesh, Domain::Triangles, &[4]),
        Err(SelError::IndexOutOfRange { index: 4, count: 4 })
    );
    assert_eq!(
        Selection::on_element(ID, &mesh, Domain::Faces, &[0]),
        Err(SelError::DomainMismatch { domain: Domain::Faces, kind: ElementKind::Mesh })
    );
}

/// DomainMismatch: a point set has no triangles; a CAD body has faces and edges only,
/// whose count comes from GEO.CAD.
#[test]
fn domain_mismatch() {
    let ps = pointset_small();
    assert_eq!(
        Selection::on_element(ID, &ps, Domain::Triangles, &[0]),
        Err(SelError::DomainMismatch { domain: Domain::Triangles, kind: ElementKind::PointSet })
    );
    let cad = cad_body();
    assert_eq!(domain_count(&cad, Domain::Faces), Ok(None));
    assert_eq!(domain_count(&cad, Domain::Edges), Ok(None));
    assert_eq!(
        domain_count(&cad, Domain::Points),
        Err(SelError::DomainMismatch { domain: Domain::Points, kind: ElementKind::CadBodyRef })
    );
    assert_eq!(
        Selection::on_element(ID, &cad, Domain::Faces, &[0]),
        Err(SelError::DomainMismatch { domain: Domain::Faces, kind: ElementKind::CadBodyRef })
    );
    // With a face count from the topology: digest and domain are checked by apply.
    let faces = IndexSet::from_indices(Domain::Faces, &[2, 7], 12).unwrap();
    let sel = Selection::new(ID, digest(&cad), SelectionBody::IndexSet(faces.clone()));
    assert_eq!(apply(&sel, &cad), Ok(&faces));
    // A points set bound to the CAD body's digest still fails on the domain.
    let pts = IndexSet::from_indices(Domain::Points, &[0], 1).unwrap();
    let sel = Selection::new(ID, digest(&cad), SelectionBody::IndexSet(pts));
    assert_eq!(
        apply(&sel, &cad),
        Err(SelError::DomainMismatch { domain: Domain::Points, kind: ElementKind::CadBodyRef })
    );
}

/// apply re-checks the bound: a decoded selection that names the right digest but an
/// index past the element fails with IndexOutOfRange, not silently.
#[test]
fn apply_rechecks_bounds_of_unchecked_sets() {
    let element = pointset_small();
    let big = IndexSet::from_indices(Domain::Points, &[5, 1500], 2000).unwrap();
    let sel = Selection::new(ID, digest(&element), SelectionBody::IndexSet(big));
    let decoded = Selection::decode(&sel.encode()).unwrap();
    assert_eq!(apply(&decoded, &element), Err(SelError::IndexOutOfRange { index: 1500, count: 1000 }));
}

/// A query selection is not applied here; evaluation belongs to GEO.SEL.QUERY.
#[test]
fn query_selection_is_not_an_index_set() {
    let element = pointset_small();
    let q = SelectionQuery::from_bytes(b"group G".to_vec()).unwrap();
    let sel = Selection::new(ID, digest(&element), SelectionBody::Query(q));
    assert_eq!(apply(&sel, &element), Err(SelError::NotIndexSet));
    assert_eq!(sel.content_digest(), None);
    assert_eq!(
        SelectionQuery::from_bytes(vec![0; MAX_QUERY_BYTES + 1]),
        Err(SelError::QueryTooLarge { len: MAX_QUERY_BYTES as u64 + 1 })
    );
    assert!(SelectionQuery::from_bytes(vec![0; MAX_QUERY_BYTES]).is_ok());
}

/// Conversion back to an explicit list is bounded by the caller's limit.
#[test]
fn to_indices_is_bounded() {
    let s = IndexSet::from_ranges(Domain::Points, &[(10, 20), (0, 5)], 100).unwrap();
    assert_eq!(s.to_indices(15).unwrap(), vec![0, 1, 2, 3, 4, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19]);
    assert_eq!(s.to_indices(14), Err(SelError::TooManyIndices { count: 15, limit: 14 }));
    let huge = IndexSet::from_ranges(Domain::Points, &[(0, u64::MAX)], u64::MAX).unwrap();
    assert_eq!(huge.len(), u64::MAX);
    assert_eq!(huge.to_indices(1000), Err(SelError::TooManyIndices { count: u64::MAX, limit: 1000 }));
    assert!(IndexSet::empty(Domain::Edges).to_indices(0).unwrap().is_empty());
}

// ------------------------------------------------------------------ AC-5

/// AC-5 / REQ-005: the serialised selection holds exactly target, target digest and body,
/// and nothing named or sized like camera, pixel or GPU data.
#[test]
fn ac5_serialised_selection_has_no_view_fields() {
    const FORBIDDEN: [&str; 12] = [
        "camera",
        "pixel",
        "screen",
        "gpu",
        "viewport",
        "view",
        "ray",
        "lasso",
        "handle",
        "texture",
        "projection",
        "cursor",
    ];
    for name in SELECTION_FIELDS.iter().chain(INDEX_SET_FIELDS.iter()).chain(QUERY_FIELDS.iter()) {
        for bad in FORBIDDEN {
            assert!(!name.contains(bad), "field {name} looks like view state ({bad})");
        }
    }
    // Byte accounting: tag, version, target, target digest, body tag, then the body.
    let element = pointset_small();
    let sel = Selection::on_element(ID, &element, Domain::Points, &[1, 2, 3, 9, 40]).unwrap();
    let enc = sel.encode();
    let SelectionBody::IndexSet(set) = sel.body() else { panic!("index set") };
    assert_eq!(enc.len(), 6 + 1 + 16 + 32 + 1 + (1 + 8 + 16 * set.ranges().len()));
    assert_eq!(&enc[..6], &SELECTION_TAG);
    assert_eq!(&enc[7..23], &ID.to_bytes());
    assert_eq!(&enc[23..55], &sel.target_digest().to_bytes());
    assert_eq!(Selection::decode(&enc).unwrap(), sel);

    let q =
        Selection::new(ID, sel.target_digest(), SelectionBody::Query(SelectionQuery::from_bytes(vec![7; 5]).unwrap()));
    let enc = q.encode();
    assert_eq!(enc.len(), 6 + 1 + 16 + 32 + 1 + (4 + 5));
    assert_eq!(Selection::decode(&enc).unwrap(), q);
}

/// Decoding accepts only the canonical layout.
#[test]
fn decode_rejects_non_canonical_input() {
    let element = pointset_small();
    let sel = Selection::on_element(ID, &element, Domain::Points, &[1, 2, 5]).unwrap();
    let good = sel.encode();
    let malformed = |b: &[u8]| matches!(Selection::decode(b), Err(SelError::Malformed { .. }));

    let mut trailing = good.clone();
    trailing.push(0);
    assert!(malformed(&trailing));
    assert!(malformed(&good[..good.len() - 1]));
    assert!(malformed(&[]));
    let mut tag = good.clone();
    tag[0] = b'X';
    assert!(malformed(&tag));
    let mut ver = good.clone();
    ver[6] = 2;
    assert!(malformed(&ver));
    let mut body = good.clone();
    body[55] = 3;
    assert!(malformed(&body));
    let mut dom = good.clone();
    dom[56] = 0;
    assert!(malformed(&dom));

    // Ranges start after: header 56, domain 1, n_ranges 8 = byte 65.
    let ranges_at = 65;
    let put = |b: &mut Vec<u8>, at: usize, v: u64| b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    // [1,3) [5,6): make the second start at 3 (touching) -> not coalesced.
    let mut touch = good.clone();
    put(&mut touch, ranges_at + 16, 3);
    assert!(malformed(&touch));
    // Empty range [1,1).
    let mut empty = good.clone();
    put(&mut empty, ranges_at + 8, 1);
    assert!(malformed(&empty));
    // Descending ranges.
    let mut desc = good.clone();
    put(&mut desc, ranges_at, 7);
    put(&mut desc, ranges_at + 8, 9);
    assert!(malformed(&desc));
    // A range count far beyond the input is rejected before allocating.
    let mut many = good.clone();
    put(&mut many, 57, u64::MAX);
    assert!(malformed(&many));
    // An over-long query length.
    let q = Selection::new(ID, sel.target_digest(), SelectionBody::Query(SelectionQuery::from_bytes(vec![]).unwrap()));
    let mut long = q.encode();
    long[56..60].copy_from_slice(&(MAX_QUERY_BYTES as u32 + 1).to_le_bytes());
    long.resize(long.len() + MAX_QUERY_BYTES + 1, 0);
    assert_eq!(Selection::decode(&long), Err(SelError::QueryTooLarge { len: MAX_QUERY_BYTES as u64 + 1 }));
}

/// Errors have stable codes and readable messages.
#[test]
fn error_codes() {
    let e = SelError::StaleSelection { expected: ElementDigest([0; 32]), found: ElementDigest([1; 32]) };
    assert_eq!(e.code(), "stale_selection");
    assert!(e.to_string().contains("0101"));
    assert_eq!(SelError::DomainMismatch { domain: Domain::Faces, kind: ElementKind::Mesh }.code(), "domain_mismatch");
    assert_eq!(SelError::NotIndexSet.code(), "not_index_set");
}

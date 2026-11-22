//! Acceptance criteria of spec node GEO.SEL.ALGEBRA (AC-1 .. AC-5) plus edge cases.
//!
//! Expected ranges and digests come from `fixtures/geo/sel/algebra_cases.json`, written by
//! the independent Python set/BFS/brute-force reference `tools/fixtures/geo_sel_algebra.py`
//! (seed 73); none come from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_index::{PointIndex, COORD_LIMIT};
use fid_geo_sel::{
    difference, difference_selections, grow, grow_points, grow_selection, grow_triangles, intersection,
    intersection_selections, invert, invert_selection, shrink, shrink_points, shrink_selection, shrink_triangles,
    sym_difference, sym_difference_selections, union, union_selections, Adjacency, Domain, EntityId, GrowSpec,
    IndexSet, SelError, Selection, SelectionBody, SelectionQuery,
};
use fid_geo_types::{digest, AttributeSet, ElementDigest, FrameRef, GeometryElement, Mesh, PointBuffer, PointSet};
use std::collections::BTreeMap;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/");

fn read(path: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}{path}")).unwrap()
}

fn unhex32(s: &str) -> [u8; 32] {
    assert_eq!(s.len(), 64);
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
    }
    out
}

fn unhex16(s: &str) -> [u8; 16] {
    assert_eq!(s.len(), 32);
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
    }
    out
}

fn hexf(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn domain_of(s: &str) -> Domain {
    Domain::ALL.into_iter().find(|d| d.as_str() == s).unwrap()
}

fn ranges_of(s: &str) -> Vec<(u64, u64)> {
    if s.trim().is_empty() {
        return Vec::new();
    }
    s.split_whitespace()
        .map(|t| {
            let (a, b) = t.split_once(':').unwrap();
            (a.parse().unwrap(), b.parse().unwrap())
        })
        .collect()
}

fn parse_section(text: &str, section: &str) -> Vec<BTreeMap<String, String>> {
    let marker = format!("\"{section}\": [");
    let start = text.find(&marker).unwrap() + marker.len();
    let rest = &text[start..];
    let mut out = Vec::new();
    let mut cur: Option<BTreeMap<String, String>> = None;
    for line in rest.lines() {
        let t = line.trim();
        if t == "]" || t == "]," {
            break;
        }
        if t == "{" {
            cur = Some(BTreeMap::new());
            continue;
        }
        if t == "}" || t == "}," {
            if let Some(m) = cur.take() {
                out.push(m);
            }
            continue;
        }
        if let Some(map) = cur.as_mut() {
            if let Some((k, v)) = t.split_once(": ") {
                let key = k.trim_matches('"').to_string();
                let val = v.trim_end_matches(',').trim_matches('"').to_string();
                map.insert(key, val);
            }
        }
    }
    out
}

// ------------------------------------------------------------------ fixture elements

fn load_pointset_small() -> (PointSet, GeometryElement) {
    let text = read("geo/types/pointset_small.txt");
    let mut frame = None;
    let mut buf = PointBuffer::default();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("frame ") {
            frame = Some(FrameRef::from_bytes(unhex16(rest)));
        } else if let Some(rest) = t.strip_prefix("p ") {
            let p: Vec<&str> = rest.split_whitespace().collect();
            buf.x.push(hexf(p[1]));
            buf.y.push(hexf(p[2]));
            buf.z.push(hexf(p[3]));
        }
    }
    let ps = PointSet::new(frame.unwrap(), buf, AttributeSet::default()).unwrap();
    let el = GeometryElement::PointSet(ps.clone());
    (ps, el)
}

fn load_mesh(path: &str) -> (Mesh, GeometryElement) {
    let text = read(path);
    let mut frame = None;
    let mut buf = PointBuffer::default();
    let mut tris = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("frame ") {
            frame = Some(FrameRef::from_bytes(unhex16(rest)));
        } else if let Some(rest) = t.strip_prefix("v ") {
            let p: Vec<&str> = rest.split_whitespace().collect();
            buf.x.push(p[0].parse().unwrap());
            buf.y.push(p[1].parse().unwrap());
            buf.z.push(p[2].parse().unwrap());
        } else if let Some(rest) = t.strip_prefix("t ") {
            let p: Vec<&str> = rest.split_whitespace().collect();
            tris.push([p[0].parse().unwrap(), p[1].parse().unwrap(), p[2].parse().unwrap()]);
        }
    }
    let mesh = Mesh::new(frame.unwrap(), buf, AttributeSet::default(), tris).unwrap();
    let el = GeometryElement::Mesh(mesh.clone());
    (mesh, el)
}

/// Exact `mesh_sphere_1e5` from `fixtures/geo/index/make_bvh_fixtures.py` (N_SUB = 71, 100 820 triangles).
fn build_mesh_sphere_1e5() -> Mesh {
    let phi = (1.0_f64 + 5.0_f64.sqrt()) / 2.0;
    let ico_v: [[f64; 3]; 12] = [
        [-1.0, phi, 0.0],
        [1.0, phi, 0.0],
        [-1.0, -phi, 0.0],
        [1.0, -phi, 0.0],
        [0.0, -1.0, phi],
        [0.0, 1.0, phi],
        [0.0, -1.0, -phi],
        [0.0, 1.0, -phi],
        [phi, 0.0, -1.0],
        [phi, 0.0, 1.0],
        [-phi, 0.0, -1.0],
        [-phi, 0.0, 1.0],
    ];
    let ico_f: [[usize; 3]; 20] = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    let n = 71usize;
    let fn_ = n as f64;
    let mut buf = PointBuffer::default();
    let mut tris = Vec::with_capacity(20 * n * n);
    for [ia, ib, ic] in ico_f {
        let (a, b, c) = (ico_v[ia], ico_v[ib], ico_v[ic]);
        let base = buf.len() as usize;
        for i in 0..=n {
            for j in 0..=(n - i) {
                let (wa, wb, wc) = ((n - i - j) as f64, i as f64, j as f64);
                let px = ((a[0] * wa + b[0] * wb) + c[0] * wc) / fn_;
                let py = ((a[1] * wa + b[1] * wb) + c[1] * wc) / fn_;
                let pz = ((a[2] * wa + b[2] * wb) + c[2] * wc) / fn_;
                let ln = ((px * px + py * py) + pz * pz).sqrt();
                buf.x.push(px / ln);
                buf.y.push(py / ln);
                buf.z.push(pz / ln);
            }
        }
        let idx = |i: usize, j: usize| -> u64 { (base + i * (n + 1) - (i * i.saturating_sub(1)) / 2 + j) as u64 };
        for i in 0..n {
            for j in 0..(n - i) {
                tris.push([idx(i, j), idx(i + 1, j), idx(i, j + 1)]);
                if j + 1 < n - i {
                    tris.push([idx(i + 1, j), idx(i + 1, j + 1), idx(i, j + 1)]);
                }
            }
        }
    }
    assert_eq!(tris.len(), 100_820);
    Mesh::new(FrameRef::from_bytes([0x11; 16]), buf, AttributeSet::default(), tris).unwrap()
}

// ------------------------------------------------------------------ AC-1 & AC-5

#[test]
fn ac1_ac5_set_algebra_fixture() {
    let text = read("geo/sel/algebra_cases.json");
    let cases = parse_section(&text, "set_ops");
    assert!(cases.len() >= 10);
    let eid = EntityId::from_bytes([0x42; 16]);

    for c in &cases {
        let name = &c["name"];
        let domain = domain_of(&c["domain"]);
        let bound: u64 = c["bound"].parse().unwrap();
        let target = ElementDigest(unhex32(&c["target_digest"]));
        let a = IndexSet::from_ranges(domain, &ranges_of(&c["a"]), bound).unwrap();
        let b = IndexSet::from_ranges(domain, &ranges_of(&c["b"]), bound).unwrap();
        let da_before = a.digest(target);
        let db_before = b.digest(target);

        let u = union(&a, &b).unwrap();
        assert_eq!(u.ranges(), ranges_of(&c["union"]), "{name} union ranges");
        assert_eq!(u.digest(target).to_string(), c["union_sha256"], "{name} union digest");

        let inter = intersection(&a, &b).unwrap();
        assert_eq!(inter.ranges(), ranges_of(&c["intersection"]), "{name} intersection ranges");
        assert_eq!(inter.digest(target).to_string(), c["intersection_sha256"], "{name} intersection digest");

        let diff = difference(&a, &b).unwrap();
        assert_eq!(diff.ranges(), ranges_of(&c["difference"]), "{name} difference ranges");
        assert_eq!(diff.digest(target).to_string(), c["difference_sha256"], "{name} difference digest");

        let sdiff = sym_difference(&a, &b).unwrap();
        assert_eq!(sdiff.ranges(), ranges_of(&c["sym_difference"]), "{name} sym_difference ranges");
        assert_eq!(sdiff.digest(target).to_string(), c["sym_difference_sha256"], "{name} sym_difference digest");

        let inv = invert(&a, bound).unwrap();
        assert_eq!(inv.ranges(), ranges_of(&c["invert_a"]), "{name} invert ranges");
        assert_eq!(inv.digest(target).to_string(), c["invert_a_sha256"], "{name} invert digest");
        assert_eq!(invert(&inv, bound).unwrap(), a, "{name} double invert");

        // Selection wrappers agree and leave input digests unchanged (AC-5).
        let sa = Selection::new(eid, target, SelectionBody::IndexSet(a.clone()));
        let sb = Selection::new(eid, target, SelectionBody::IndexSet(b.clone()));
        assert_eq!(union_selections(&sa, &sb).unwrap().content_digest().unwrap().to_string(), c["union_sha256"]);
        assert_eq!(
            intersection_selections(&sa, &sb).unwrap().content_digest().unwrap().to_string(),
            c["intersection_sha256"]
        );
        assert_eq!(
            difference_selections(&sa, &sb).unwrap().content_digest().unwrap().to_string(),
            c["difference_sha256"]
        );
        assert_eq!(
            sym_difference_selections(&sa, &sb).unwrap().content_digest().unwrap().to_string(),
            c["sym_difference_sha256"]
        );
        assert_eq!(invert_selection(&sa, bound).unwrap().content_digest().unwrap().to_string(), c["invert_a_sha256"]);

        assert_eq!(a.digest(target), da_before, "{name} input a digest mutated");
        assert_eq!(b.digest(target), db_before, "{name} input b digest mutated");
        assert_eq!(sa.content_digest().unwrap(), da_before);
        assert_eq!(sb.content_digest().unwrap(), db_before);
    }
}

#[test]
fn ac1_ac2_ac5_mesh_grow_shrink_fixture() {
    let text = read("geo/sel/algebra_cases.json");
    let cases = parse_section(&text, "mesh_ops");
    assert!(cases.len() >= 14);
    let (cube, cube_el) = load_mesh("geo/types/mesh_cube.txt");
    let (open, open_el) = load_mesh("geo/types/mesh_open_flipped.txt");
    let sphere = build_mesh_sphere_1e5();
    let sphere_el = GeometryElement::Mesh(sphere.clone());

    for c in &cases {
        let name = &c["name"];
        let (mesh, el) = match c["element"].as_str() {
            "mesh_cube" => (&cube, &cube_el),
            "mesh_open_flipped" => (&open, &open_el),
            "mesh_sphere_1e5" => (&sphere, &sphere_el),
            other => panic!("unexpected element {other}"),
        };
        let adj = match c["adjacency"].as_str() {
            "edge" => Adjacency::Edge,
            "vertex" => Adjacency::Vertex,
            other => panic!("unexpected adjacency {other}"),
        };
        let rings: u32 = c["rings"].parse().unwrap();
        let target = ElementDigest(unhex32(&c["target_digest"]));
        let input = IndexSet::from_ranges(Domain::Triangles, &ranges_of(&c["input"]), mesh.triangle_count()).unwrap();
        let before = input.digest(target);

        let res = match c["op"].as_str() {
            "grow" => {
                let r1 = grow_triangles(mesh, &input, rings, adj).unwrap();
                let r2 = grow(el, &input, GrowSpec::Rings(rings, adj)).unwrap();
                assert_eq!(r1, r2, "{name}");
                r1
            }
            "shrink" => {
                let r1 = shrink_triangles(mesh, &input, rings, adj).unwrap();
                let r2 = shrink(el, &input, GrowSpec::Rings(rings, adj)).unwrap();
                assert_eq!(r1, r2, "{name}");
                r1
            }
            other => panic!("unexpected op {other}"),
        };
        assert_eq!(res.ranges(), ranges_of(&c["result"]), "{name} ranges");
        assert_eq!(res.len().to_string(), c["result_count"], "{name} count");
        assert_eq!(res.digest(target).to_string(), c["result_sha256"], "{name} digest");
        assert_eq!(input.digest(target), before, "{name} input mutated");

        // AC-2 explicit check on mesh_cube: 1 edge ring around triangle 0 adds exactly its 3 edge neighbours.
        if name == "cube_ac2_edge_grow_1" {
            assert_eq!(res.len(), 4);
            assert!(res.contains(0));
        }
    }
}

#[test]
fn ac1_ac5_point_grow_shrink_fixture() {
    let text = read("geo/sel/algebra_cases.json");
    let cases = parse_section(&text, "point_ops");
    assert!(cases.len() >= 7);
    let (ps, el) = load_pointset_small();
    let idx = PointIndex::build(ps.buffer(), 1).unwrap();
    let real_digest = digest(&el);

    for c in &cases {
        let name = &c["name"];
        let radius = hexf(&c["radius_hex"]);
        let target = ElementDigest(unhex32(&c["target_digest"]));
        assert_eq!(target, real_digest);
        let input = IndexSet::from_ranges(Domain::Points, &ranges_of(&c["input"]), ps.len()).unwrap();
        let before = input.digest(target);

        let res = match c["op"].as_str() {
            "grow" => {
                let r1 = grow_points(ps.buffer(), &idx, &input, radius).unwrap();
                let r2 = grow(&el, &input, GrowSpec::Radius(radius)).unwrap();
                assert_eq!(r1, r2, "{name}");
                r1
            }
            "shrink" => {
                let r1 = shrink_points(ps.buffer(), &idx, &input, radius).unwrap();
                let r2 = shrink(&el, &input, GrowSpec::Radius(radius)).unwrap();
                assert_eq!(r1, r2, "{name}");
                r1
            }
            other => panic!("unexpected op {other}"),
        };
        assert_eq!(res.ranges(), ranges_of(&c["result"]), "{name} ranges");
        assert_eq!(res.len().to_string(), c["result_count"], "{name} count");
        assert_eq!(res.digest(target).to_string(), c["result_sha256"], "{name} digest");
        assert_eq!(input.digest(target), before, "{name} input mutated");

        // Selection wrapper round-trip.
        let sel = Selection::new(EntityId::from_bytes([7; 16]), target, SelectionBody::IndexSet(input));
        let sel_res = match c["op"].as_str() {
            "grow" => grow_selection(&el, &sel, GrowSpec::Radius(radius)).unwrap(),
            _ => shrink_selection(&el, &sel, GrowSpec::Radius(radius)).unwrap(),
        };
        assert_eq!(sel_res.content_digest().unwrap().to_string(), c["result_sha256"], "{name} selection");
    }
}

// ------------------------------------------------------------------ AC-3: error paths

#[test]
fn ac3_mismatches_and_error_paths() {
    let (_, ps_el) = load_pointset_small();
    let (cube, cube_el) = load_mesh("geo/types/mesh_cube.txt");
    let id_a = EntityId::from_bytes([1; 16]);
    let id_b = EntityId::from_bytes([2; 16]);

    let sel_pts = Selection::on_element(id_a, &ps_el, Domain::Points, &[0, 1, 2]).unwrap();
    let sel_cube_pts = Selection::on_element(id_b, &cube_el, Domain::Points, &[0, 1]).unwrap();
    let sel_cube_tris = Selection::on_element(id_b, &cube_el, Domain::Triangles, &[0, 1]).unwrap();

    // AC-3: different elements -> ElementMismatch.
    let e = union_selections(&sel_pts, &sel_cube_pts).unwrap_err();
    assert_eq!(e, SelError::ElementMismatch { left: sel_pts.target_digest(), right: sel_cube_pts.target_digest() });
    assert_eq!(e.code(), "element_mismatch");
    assert!(matches!(intersection_selections(&sel_pts, &sel_cube_pts), Err(SelError::ElementMismatch { .. })));
    assert!(matches!(difference_selections(&sel_pts, &sel_cube_pts), Err(SelError::ElementMismatch { .. })));
    assert!(matches!(sym_difference_selections(&sel_pts, &sel_cube_pts), Err(SelError::ElementMismatch { .. })));

    // Same element, different domains -> SetDomainMismatch (code "domain_mismatch").
    let e = union_selections(&sel_cube_pts, &sel_cube_tris).unwrap_err();
    assert_eq!(e, SelError::SetDomainMismatch { left: Domain::Points, right: Domain::Triangles });
    assert_eq!(e.code(), "domain_mismatch");

    let s_pts = IndexSet::from_indices(Domain::Points, &[0, 1], 10).unwrap();
    let s_tris = IndexSet::from_indices(Domain::Triangles, &[0, 1], 10).unwrap();
    assert_eq!(union(&s_pts, &s_tris).unwrap_err().code(), "domain_mismatch");
    assert_eq!(intersection(&s_pts, &s_tris).unwrap_err().code(), "domain_mismatch");
    assert_eq!(difference(&s_pts, &s_tris).unwrap_err().code(), "domain_mismatch");
    assert_eq!(sym_difference(&s_pts, &s_tris).unwrap_err().code(), "domain_mismatch");

    // Query selection refused as NotIndexSet.
    let q = Selection::new(
        id_a,
        sel_pts.target_digest(),
        SelectionBody::Query(SelectionQuery::from_bytes(b"{}".to_vec()).unwrap()),
    );
    assert_eq!(union_selections(&sel_pts, &q).unwrap_err(), SelError::NotIndexSet);
    assert_eq!(invert_selection(&q, 10).unwrap_err(), SelError::NotIndexSet);

    // BadRadius errors (even on empty sets).
    let empty_pts = IndexSet::empty(Domain::Points);
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.001, COORD_LIMIT * 2.0] {
        assert_eq!(grow(&ps_el, &empty_pts, GrowSpec::Radius(bad)).unwrap_err(), SelError::BadRadius);
        assert_eq!(shrink(&ps_el, &empty_pts, GrowSpec::Radius(bad)).unwrap_err(), SelError::BadRadius);
    }
    assert_eq!(SelError::BadRadius.code(), "bad_radius");

    // Wrong domain or element kind for Rings / Radius.
    assert_eq!(grow_triangles(&cube, &s_pts, 1, Adjacency::Edge).unwrap_err().code(), "domain_mismatch");
    assert_eq!(shrink(&ps_el, &s_tris, GrowSpec::Rings(1, Adjacency::Edge)).unwrap_err().code(), "domain_mismatch");
    assert_eq!(grow(&cube_el, &s_tris, GrowSpec::Radius(0.1)).unwrap_err().code(), "domain_mismatch");

    // Invert out of range.
    let big = IndexSet::from_indices(Domain::Points, &[9], 10).unwrap();
    assert_eq!(invert(&big, 5).unwrap_err(), SelError::IndexOutOfRange { index: 9, count: 5 });
}

// ------------------------------------------------------------------ AC-4: 10^6 ranges linear time & O(ranges)

#[test]
fn ac4_million_ranges_linear_and_bounded_memory() {
    let n = 1_000_000u64;
    // Operand A: 1 000 000 disjoint ranges [4k, 4k + 2)
    let ra: Vec<(u64, u64)> = (0..n).map(|k| (4 * k, 4 * k + 2)).collect();
    // Operand B: 1 000 000 disjoint ranges [4k + 1, 4k + 3)
    let rb: Vec<(u64, u64)> = (0..n).map(|k| (4 * k + 1, 4 * k + 3)).collect();
    let a = IndexSet::from_ranges(Domain::Points, &ra, 4 * n).unwrap();
    let b = IndexSet::from_ranges(Domain::Points, &rb, 4 * n).unwrap();
    assert_eq!(a.ranges().len(), n as usize);
    assert_eq!(b.ranges().len(), n as usize);

    let inter = intersection(&a, &b).unwrap();
    assert_eq!(inter.ranges().len(), n as usize);
    assert_eq!(inter.len(), n);
    assert_eq!(inter.ranges()[0], (1, 2));
    assert_eq!(inter.ranges()[n as usize - 1], (4 * (n - 1) + 1, 4 * (n - 1) + 2));

    // O(ranges) memory: stored_bytes is exact capacity = n * 16 + sizeof(IndexSet).
    let expected_bytes = core::mem::size_of::<IndexSet>() + (n as usize) * core::mem::size_of::<(u64, u64)>();
    assert_eq!(inter.stored_bytes(), expected_bytes);

    let u = union(&a, &b).unwrap();
    assert_eq!(u.ranges().len(), n as usize);
    assert_eq!(u.len(), 3 * n);

    let diff = difference(&a, &b).unwrap();
    assert_eq!(diff.ranges().len(), n as usize);
    assert_eq!(diff.len(), n);
}

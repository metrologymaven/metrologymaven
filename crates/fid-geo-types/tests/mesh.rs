//! Acceptance criteria of spec node GEO.TYPES.MESH (AC-1 .. AC-5), plus requirement and
//! validation checks. Expected facts come from `fixtures/geo/types/*.facts.json`, written
//! by the independent Python reference `make_mesh_fixtures.py`, or are hand-labelled from
//! the spec rules in the comments next to them; none come from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_types::mesh::{HALF_EDGE_COST, PARENT_COST, U32_TRIANGLE_COST};
use fid_geo_types::{
    AttributeSet, FrameRef, GeoTypeError, GeometryElement, IndexWidth, Mesh, MeshOptions, PointBuffer, TopologyFacts,
    TriangleAttrs,
};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/types/");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap()
}

fn frame_hex(s: &str) -> FrameRef {
    assert_eq!(s.len(), 32, "frame ID must be 32 hex digits");
    let mut b = [0u8; 16];
    for (i, o) in b.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
    }
    FrameRef::from_bytes(b)
}

struct Fixture {
    frame: FrameRef,
    v: Vec<[f64; 3]>,
    t: Vec<[u64; 3]>,
}

fn mesh_fixture(name: &str) -> Fixture {
    let text = read(&format!("{name}.txt"));
    let (mut frame, mut v, mut t) = (None, Vec::new(), Vec::new());
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f[0] {
            "frame" => frame = Some(frame_hex(f[1])),
            "v" => v.push([f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap()]),
            "t" => t.push([f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap()]),
            other => panic!("unknown fixture line kind {other}"),
        }
    }
    Fixture { frame: frame.unwrap(), v, t }
}

/// Reads a flat JSON object of integers and booleans (the sidecar format), rejecting
/// duplicate and unknown keys.
fn facts_fixture(name: &str) -> TopologyFacts {
    let text = read(&format!("{name}.facts.json"));
    let body = text.trim().strip_prefix('{').unwrap().strip_suffix('}').unwrap();
    let mut seen: Vec<String> = Vec::new();
    let mut nums = [None::<u64>; 5];
    let mut flags = [None::<bool>; 2];
    for item in body.split(',') {
        let (k, val) = item.split_once(':').unwrap();
        let k = k.trim().trim_matches('"').to_string();
        let val = val.trim();
        assert!(!seen.contains(&k), "duplicate key {k}");
        seen.push(k.clone());
        let num_keys = ["degenerate", "boundary_edges", "nonmanifold_edges", "misoriented_pairs", "components"];
        if let Some(i) = num_keys.iter().position(|n| *n == k) {
            nums[i] = Some(val.parse().unwrap());
        } else if k == "closed" {
            flags[0] = Some(val.parse().unwrap());
        } else if k == "oriented" {
            flags[1] = Some(val.parse().unwrap());
        } else {
            panic!("unknown key {k}");
        }
    }
    TopologyFacts {
        degenerate: nums[0].unwrap(),
        boundary_edges: nums[1].unwrap(),
        nonmanifold_edges: nums[2].unwrap(),
        misoriented_pairs: nums[3].unwrap(),
        components: nums[4].unwrap(),
        closed: flags[0].unwrap(),
        oriented: flags[1].unwrap(),
    }
}

fn build(f: &Fixture) -> Mesh {
    Mesh::from_points(f.frame, &f.v, &f.t).unwrap()
}

fn forced_u64(f: &Fixture) -> Mesh {
    let opts = MeshOptions { force_u64_indexes: true, ..MeshOptions::default() };
    Mesh::with_options(f.frame, PointBuffer::from_points(&f.v), AttributeSet::default(), f.t.clone(), opts).unwrap()
}

fn facts(
    degenerate: u64,
    boundary_edges: u64,
    nonmanifold_edges: u64,
    misoriented_pairs: u64,
    components: u64,
    closed: bool,
    oriented: bool,
) -> TopologyFacts {
    TopologyFacts { degenerate, boundary_edges, nonmanifold_edges, misoriented_pairs, components, closed, oriented }
}

const FIXTURES: [&str; 6] =
    ["mesh_cube", "mesh_open_flipped", "mesh_nonmanifold", "mesh_mobius", "mesh_bowtie", "mesh_degenerate_dup"];

/// Every fixture's facts equal the Python reference's sidecar.
#[test]
fn fixtures_match_reference_facts() {
    for name in FIXTURES {
        let m = build(&mesh_fixture(name));
        assert_eq!(m.facts(), facts_fixture(name), "{name}");
    }
}

/// AC-1: the closed cube is all zero, one component, closed and oriented.
#[test]
fn ac1_cube_closed_and_oriented() {
    let m = build(&mesh_fixture("mesh_cube"));
    assert_eq!(m.facts(), facts(0, 0, 0, 0, 1, true, true));
}

/// AC-2: one triangle removed and one flipped gives 3 boundary edges and 3 misoriented
/// pairs, and the vertex and triangle arrays are unchanged bitwise.
#[test]
fn ac2_open_flipped() {
    let f = mesh_fixture("mesh_open_flipped");
    assert_eq!(f.t.len(), 11);
    let m = build(&f);
    let x = m.facts();
    assert_eq!((x.boundary_edges, x.misoriented_pairs, x.closed, x.oriented), (3, 3, false, false));
    assert_eq!(m.vertex_count(), f.v.len() as u64);
    for (i, p) in f.v.iter().enumerate() {
        let got = m.vertex(i as u64).unwrap();
        assert_eq!(got.map(f64::to_bits), p.map(f64::to_bits), "vertex {i}");
    }
    assert_eq!(m.triangles().collect::<Vec<_>>(), f.t);
}

/// AC-3: three triangles on one edge make one non-manifold edge.
#[test]
fn ac3_nonmanifold() {
    let m = build(&mesh_fixture("mesh_nonmanifold"));
    assert_eq!(m.facts().nonmanifold_edges, 1);
}

/// A six-triangle Moebius strip (hand-labelled, also asserted by the Python reference):
/// the six rail edges are boundary, the twisted rung is the one misoriented pair, one
/// component, not oriented.
#[test]
fn mobius_strip() {
    let m = build(&mesh_fixture("mesh_mobius"));
    assert_eq!(m.facts(), facts(0, 6, 0, 1, 1, false, false));
}

/// Two triangles sharing only a vertex (hand-labelled, also asserted by the Python
/// reference): two components; the non-manifold vertex is not a reported fact.
#[test]
fn bowtie_two_components() {
    let m = build(&mesh_fixture("mesh_bowtie"));
    assert_eq!(m.facts(), facts(0, 6, 0, 0, 2, false, true));
}

/// AC-4: mesh_cube with triangle 5 referring to vertex 8 fails with that triangle and
/// index; a later offender does not mask it.
#[test]
fn ac4_index_out_of_range() {
    let mut f = mesh_fixture("mesh_cube");
    f.t[5][1] = 8;
    assert_eq!(Mesh::from_points(f.frame, &f.v, &f.t), Err(GeoTypeError::IndexOutOfRange { triangle: 5, index: 8 }));
    f.t[9][0] = 100;
    assert_eq!(Mesh::from_points(f.frame, &f.v, &f.t), Err(GeoTypeError::IndexOutOfRange { triangle: 5, index: 8 }));
    // The first offending corner within a triangle wins.
    let mut g = mesh_fixture("mesh_cube");
    g.t[2] = [4, 9, 8];
    assert_eq!(Mesh::from_points(g.frame, &g.v, &g.t), Err(GeoTypeError::IndexOutOfRange { triangle: 2, index: 9 }));
}

/// Indexes far beyond `usize` on 32-bit targets are compared as `u64`, not converted.
#[test]
fn huge_index_is_out_of_range() {
    let f = mesh_fixture("mesh_cube");
    let t = [[0, 1, u64::MAX]];
    let want = GeoTypeError::IndexOutOfRange { triangle: 0, index: u64::MAX };
    assert_eq!(Mesh::from_points(f.frame, &f.v, &t), Err(want));
    let t = [[1u64 << 32, 0, 1]];
    let want = GeoTypeError::IndexOutOfRange { triangle: 0, index: 1 << 32 };
    assert_eq!(Mesh::from_points(f.frame, &f.v, &t), Err(want));
}

/// AC-5 and REQ-002: u32 and forced-u64 storage give identical facts and triangles.
#[test]
fn ac5_u32_and_u64_storage_agree() {
    for name in FIXTURES {
        let f = mesh_fixture(name);
        let a = build(&f);
        let b = forced_u64(&f);
        assert_eq!(a.index_width(), IndexWidth::U32, "{name}");
        assert_eq!(b.index_width(), IndexWidth::U64, "{name}");
        assert_eq!(a.facts(), b.facts(), "{name}");
        assert_eq!(a.triangle_count(), b.triangle_count());
        for i in 0..a.triangle_count() {
            assert_eq!(a.triangle(i), b.triangle(i), "{name} triangle {i}");
            assert_eq!(a.triangle(i), Some(f.t[i as usize]));
        }
        assert_eq!(a.triangle(a.triangle_count()), None);
        assert_eq!(b.triangle(u64::MAX), None);
    }
}

/// Facts are counts over the edge table, so permuting the triangles leaves them unchanged.
#[test]
fn facts_independent_of_triangle_order() {
    for name in FIXTURES {
        let mut f = mesh_fixture(name);
        let want = build(&f).facts();
        f.t.reverse();
        assert_eq!(build(&f).facts(), want, "{name} reversed");
        let k = 3 % f.t.len();
        f.t.rotate_left(k);
        assert_eq!(build(&f).facts(), want, "{name} rotated");
    }
}

fn tri_vertices() -> Vec<[f64; 3]> {
    vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [5.0, 0.0, 0.0], [6.0, 0.0, 0.0], [5.0, 1.0, 0.0]]
}

/// Hand-labelled small cases from the spec rules (definitions in the module docs).
#[test]
fn hand_labelled_cases() {
    let fr = FrameRef::from_bytes([1; 16]);
    let v = tri_vertices();
    let cases: [(&str, Vec<[u64; 3]>, TopologyFacts); 8] = [
        // No triangles: nothing counted; not closed (no triangle), vacuously oriented.
        ("empty", vec![], facts(0, 0, 0, 0, 0, false, true)),
        // One triangle: its 3 edges are boundary, one component.
        ("single", vec![[0, 1, 2]], facts(0, 3, 0, 0, 1, false, true)),
        // Two disjoint triangles: 6 boundary edges, 2 components.
        ("two parts", vec![[0, 1, 2], [3, 4, 5]], facts(0, 6, 0, 0, 2, false, true)),
        // The same triangle twice: each edge used twice in the same direction.
        ("duplicate", vec![[0, 1, 2], [0, 1, 2]], facts(0, 0, 0, 3, 1, true, false)),
        // A triangle and its reverse: a closed, consistently oriented two-sided sheet.
        ("two-sided", vec![[0, 1, 2], [0, 2, 1]], facts(0, 0, 0, 0, 1, true, true)),
        // A degenerate triangle alone: counted, no edges, no component.
        ("degenerate", vec![[0, 0, 1]], facts(1, 0, 0, 0, 0, false, true)),
        // Fully collapsed triangle next to a real one.
        ("collapsed", vec![[2, 2, 2], [0, 1, 2]], facts(1, 3, 0, 0, 1, false, true)),
        // Duplicate plus its reverse: the three edges each used three times.
        ("triple", vec![[0, 1, 2], [0, 1, 2], [0, 2, 1]], facts(0, 0, 3, 0, 1, false, false)),
    ];
    for (name, t, want) in cases {
        let m = Mesh::from_points(fr, &v, &t).unwrap();
        assert_eq!(m.facts(), want, "{name}");
        let opts = MeshOptions { force_u64_indexes: true, ..MeshOptions::default() };
        let b = Mesh::with_options(fr, PointBuffer::from_points(&v), AttributeSet::default(), t.clone(), opts).unwrap();
        assert_eq!(b.facts(), want, "{name} (u64)");
        assert_eq!(m.triangles().collect::<Vec<_>>(), t, "{name}: triangles kept as given");
    }
}

/// REQ-GEO.TYPES-003: non-finite vertices and mismatched lengths are typed errors, in the
/// documented order (vertices, vertex attributes, indexes, triangle attributes).
#[test]
fn validation_errors_and_order() {
    let fr = FrameRef::from_bytes([2; 16]);
    let v = tri_vertices();
    let mut bad = v.clone();
    bad[4][2] = f64::NAN;
    // Non-finite vertex wins over a bad index.
    assert_eq!(Mesh::from_points(fr, &bad, &[[0, 1, 99]]), Err(GeoTypeError::NonFinite { index: 4 }));
    bad[4][2] = f64::INFINITY;
    assert_eq!(Mesh::from_points(fr, &bad, &[]), Err(GeoTypeError::NonFinite { index: 4 }));
    // Column length mismatch.
    let cols = PointBuffer::new(vec![0.0, 1.0, 2.0], vec![0.0, 1.0, 2.0], vec![0.0, 1.0]);
    assert_eq!(
        Mesh::new(fr, cols, AttributeSet::default(), vec![]),
        Err(GeoTypeError::AttributeLength { name: "z".to_string(), expected: 3, found: 2 })
    );
    // Vertex attribute length, before the index check.
    let attrs = AttributeSet { intensity: Some(vec![1.0; 5]), ..AttributeSet::default() };
    assert_eq!(
        Mesh::new(fr, PointBuffer::from_points(&v), attrs, vec![[0, 1, 99]]),
        Err(GeoTypeError::AttributeLength { name: "intensity".to_string(), expected: 6, found: 5 })
    );
    // Bad normal on a vertex.
    let mut n = [vec![0.0; 6], vec![0.0; 6], vec![1.0; 6]];
    n[2][3] = 2.0;
    let attrs = AttributeSet { normal: Some(n), ..AttributeSet::default() };
    assert_eq!(Mesh::new(fr, PointBuffer::from_points(&v), attrs, vec![]), Err(GeoTypeError::BadNormal { index: 3 }));
    // Index check before triangle attributes.
    let opts = MeshOptions {
        triangle_attrs: TriangleAttrs { scanner_id: Some(vec![1]), quality: None },
        ..MeshOptions::default()
    };
    assert_eq!(
        Mesh::with_options(fr, PointBuffer::from_points(&v), AttributeSet::default(), vec![[0, 1, 6]], opts.clone()),
        Err(GeoTypeError::IndexOutOfRange { triangle: 0, index: 6 })
    );
    // Triangle attribute lengths.
    let two = vec![[0, 1, 2], [3, 4, 5]];
    assert_eq!(
        Mesh::with_options(fr, PointBuffer::from_points(&v), AttributeSet::default(), two.clone(), opts),
        Err(GeoTypeError::AttributeLength { name: "triangle_scanner_id".to_string(), expected: 2, found: 1 })
    );
    let opts = MeshOptions {
        triangle_attrs: TriangleAttrs { scanner_id: Some(vec![1, 2]), quality: Some(vec![0, 1, 2]) },
        ..MeshOptions::default()
    };
    assert_eq!(
        Mesh::with_options(fr, PointBuffer::from_points(&v), AttributeSet::default(), two, opts),
        Err(GeoTypeError::AttributeLength { name: "triangle_quality".to_string(), expected: 2, found: 3 })
    );
}

/// Attributes are kept as given; the frame is the one passed in; vertices_to_point_set
/// carries the vertex attributes into the new point set.
#[test]
fn attributes_and_frame_kept() {
    let fr = FrameRef::from_bytes([5; 16]);
    let v = tri_vertices();
    let attrs = AttributeSet { quality: Some(vec![1, 2, 3, 4, 5, 6]), ..AttributeSet::default() };
    let tattrs = TriangleAttrs { scanner_id: Some(vec![7, 8]), quality: Some(vec![9, 10]) };
    let opts = MeshOptions { triangle_attrs: tattrs.clone(), ..MeshOptions::default() };
    let m =
        Mesh::with_options(fr, PointBuffer::from_points(&v), attrs.clone(), vec![[0, 1, 2], [3, 4, 5]], opts).unwrap();
    assert_eq!(m.frame(), fr);
    assert_eq!(m.vertex_attrs(), &attrs);
    assert_eq!(m.triangle_attrs(), &tattrs);
    assert_eq!(m.vertices(), &PointBuffer::from_points(&v));
    let p = m.vertices_to_point_set();
    assert_eq!(p.frame(), fr);
    assert_eq!(p.attributes(), &attrs);
    let e = GeometryElement::Mesh(m);
    assert_eq!(e.frame(), fr);
}

/// Bounded allocation: the working size is checked against the budget before anything
/// is allocated, and an overflowing size is refused.
#[test]
fn allocation_budget() {
    let f = mesh_fixture("mesh_cube");
    // Hand count for the cube: 12 triangles, all non-degenerate, u32 storage.
    let need = 12 * U32_TRIANGLE_COST + 12 * 3 * HALF_EDGE_COST + 12 * PARENT_COST;
    assert_eq!(Mesh::work_bytes(12, 12, true), Some(need));
    assert_eq!(Mesh::work_bytes(12, 12, false), Some(need - 12 * U32_TRIANGLE_COST));
    assert_eq!(Mesh::work_bytes(u64::MAX, u64::MAX, true), None);
    assert_eq!(Mesh::work_bytes(1 << 62, 0, false), None);
    let with = |limit: u64| {
        let opts = MeshOptions { work_budget_bytes: Some(limit), ..MeshOptions::default() };
        Mesh::with_options(f.frame, PointBuffer::from_points(&f.v), AttributeSet::default(), f.t.clone(), opts)
    };
    assert_eq!(with(need).unwrap().facts(), build(&f).facts());
    assert_eq!(with(need - 1), Err(GeoTypeError::AllocationBudget { needed: need, limit: need - 1 }));
    assert_eq!(with(0), Err(GeoTypeError::AllocationBudget { needed: need, limit: 0 }));
    // Degenerate triangles take no half-edges.
    let mut g = mesh_fixture("mesh_cube");
    g.t.push([1, 1, 2]);
    let need13 = 13 * U32_TRIANGLE_COST + 12 * 3 * HALF_EDGE_COST + 13 * PARENT_COST;
    let opts = MeshOptions { work_budget_bytes: Some(need13), ..MeshOptions::default() };
    let m = Mesh::with_options(g.frame, PointBuffer::from_points(&g.v), AttributeSet::default(), g.t.clone(), opts);
    assert_eq!(m.unwrap().facts().degenerate, 1);
}

/// Stable error codes for the allocation errors.
#[test]
fn allocation_error_codes() {
    assert_eq!(GeoTypeError::AllocationBudget { needed: 2, limit: 1 }.code(), "allocation_budget");
    assert_eq!(GeoTypeError::AllocationFailed { needed: 2 }.code(), "allocation_failed");
    assert_eq!(
        GeoTypeError::AllocationBudget { needed: 2, limit: 1 }.to_string(),
        "mesh construction needs 2 working bytes, budget is 1"
    );
}

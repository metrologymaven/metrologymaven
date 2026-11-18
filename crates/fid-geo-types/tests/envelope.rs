//! Acceptance criteria of spec node GEO.TYPES.ENV (AC-1 .. AC-4, AC-6; AC-5 is the
//! `compile_fail` doc-tests in `src/lib.rs`), plus requirement checks.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use core::num::NonZeroUsize;
use fid_geo_types::{
    Aabb, AttributeSet, CadBodyRef, ElementKind, FrameRef, GeoTypeError, GeometryElement, Mesh, PointBlock, PointBuffer,
    PointCloud, PointSet, PointSource,
};
use fid_types::Digest;

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

fn f64_hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn bits3(v: [f64; 3]) -> [u64; 3] {
    [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()]
}

/// The pointset_small fixture: frame, points in index order, expected box (as bits).
struct Small {
    frame: FrameRef,
    points: Vec<[f64; 3]>,
    min: [u64; 3],
    max: [u64; 3],
}

fn small() -> Small {
    let text = read("pointset_small.txt");
    let (mut frame, mut count, mut min, mut max) = (None, None, None, None);
    let mut points = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        let three = |s: &[&str]| [f64_hex(s[0]).to_bits(), f64_hex(s[1]).to_bits(), f64_hex(s[2]).to_bits()];
        match f[0] {
            "frame" => frame = Some(frame_hex(f[1])),
            "count" => count = Some(f[1].parse::<usize>().unwrap()),
            "aabb_min" => min = Some(three(&f[1..4])),
            "aabb_max" => max = Some(three(&f[1..4])),
            "p" => {
                assert_eq!(f[1].parse::<usize>().unwrap(), points.len(), "points must be in index order");
                points.push([f64_hex(f[2]), f64_hex(f[3]), f64_hex(f[4])]);
            }
            other => panic!("unknown fixture line kind {other}"),
        }
    }
    assert_eq!(count, Some(points.len()));
    assert_eq!(points.len(), 1000);
    Small { frame: frame.unwrap(), points, min: min.unwrap(), max: max.unwrap() }
}

/// The mesh_cube fixture: frame, vertices, triangles.
fn cube() -> (FrameRef, Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let text = read("mesh_cube.txt");
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
    (frame.unwrap(), v, t)
}

fn cube_mesh() -> Mesh {
    let (frame, v, t) = cube();
    Mesh::new(frame, &v, &t).unwrap()
}

/// The fixture really is the seeded generator's output (seed 1, stream 0, * 0.1 m), so
/// its provenance holds; the expected box comes from the Python reference alone.
#[test]
fn fixture_pointset_small_matches_seeded_generator() {
    use fid_num::rng::{Address, Cursor};
    let s = small();
    let mut cur = Cursor::new(Address { seed: 1, stream: 0 }, 0);
    for p in &s.points {
        let g = [cur.uniform_f64() * 0.1, cur.uniform_f64() * 0.1, cur.uniform_f64() * 0.1];
        assert_eq!(bits3(g), bits3(*p));
    }
}

/// AC-1: kind, frame and bit-exact bounding box of pointset_small in the envelope.
#[test]
fn ac1_pointset_small_envelope() {
    let s = small();
    let e = GeometryElement::PointSet(PointSet::from_points(s.frame, &s.points).unwrap());
    assert_eq!(e.kind(), ElementKind::PointSet);
    assert_eq!(e.frame(), s.frame);
    let b = e.aabb().unwrap();
    assert_eq!(bits3(b.min()), s.min);
    assert_eq!(bits3(b.max()), s.max);
    for p in &s.points {
        assert!(b.contains(*p));
    }
}

/// AC-1 also holds for the same points held as a point cloud.
#[test]
fn ac1_pointset_small_as_cloud() {
    let s = small();
    let e = GeometryElement::PointCloud(PointCloud::from_points(s.frame, &s.points).unwrap());
    assert_eq!(e.kind(), ElementKind::PointCloud);
    assert_eq!(e.frame(), s.frame);
    let b = e.aabb().unwrap();
    assert_eq!((bits3(b.min()), bits3(b.max())), (s.min, s.max));
}

/// AC-2: a mesh given to an operation that needs a point set is refused by the kind guard.
#[test]
fn ac2_wrong_kind_mesh_for_point_set() {
    let e = GeometryElement::Mesh(cube_mesh());
    let want = GeoTypeError::WrongKind { expected: ElementKind::PointSet, found: ElementKind::Mesh };
    assert_eq!(e.require(ElementKind::PointSet), Err(want.clone()));
    assert_eq!(e.as_point_set(), Err(want));
    assert_eq!(e.require(ElementKind::Mesh), Ok(()));
    assert!(e.as_mesh().is_ok());
}

/// REQ-003: every guard returns WrongKind { expected, found } for every other kind.
#[test]
fn req3_kind_guard_matrix() {
    let frame = FrameRef::from_bytes([1; 16]);
    let pts = [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]];
    let aabb = Aabb::new([0.0; 3], [1.0; 3]).unwrap();
    let elements = [
        GeometryElement::PointSet(PointSet::from_points(frame, &pts).unwrap()),
        GeometryElement::PointCloud(PointCloud::from_points(frame, &pts).unwrap()),
        GeometryElement::Mesh(Mesh::new(frame, &pts, &[]).unwrap()),
        GeometryElement::CadBody(CadBodyRef::new(frame, [2; 16], Digest::from_bytes([3; 32]), aabb)),
    ];
    for (e, kind) in elements.iter().zip(ElementKind::ALL) {
        assert_eq!(e.kind(), kind);
        assert_eq!(e.frame(), frame);
        for expected in ElementKind::ALL {
            let got = e.require(expected);
            if expected == kind {
                assert_eq!(got, Ok(()));
            } else {
                assert_eq!(got, Err(GeoTypeError::WrongKind { expected, found: kind }));
            }
        }
        let wk = |expected| GeoTypeError::WrongKind { expected, found: kind };
        assert_eq!(e.as_point_set().err(), (kind != ElementKind::PointSet).then(|| wk(ElementKind::PointSet)));
        assert_eq!(e.as_point_cloud().err(), (kind != ElementKind::PointCloud).then(|| wk(ElementKind::PointCloud)));
        assert_eq!(e.as_mesh().err(), (kind != ElementKind::Mesh).then(|| wk(ElementKind::Mesh)));
        assert_eq!(e.as_cad_body().err(), (kind != ElementKind::CadBodyRef).then(|| wk(ElementKind::CadBodyRef)));
    }
}

/// AC-3: an empty point set has no box.
#[test]
fn ac3_empty_point_set_aabb() {
    let p = PointSet::new(FrameRef::from_bytes([0; 16]), PointBuffer::default(), AttributeSet::default()).unwrap();
    assert!(p.is_empty());
    assert_eq!(p.aabb(), Err(GeoTypeError::Empty));
    assert_eq!(GeometryElement::PointSet(p).aabb(), Err(GeoTypeError::Empty));
}

/// REQ-005 for the other point-bearing kinds: empty cloud and vertex-less mesh.
#[test]
fn req5_empty_cloud_and_mesh_aabb() {
    let frame = FrameRef::from_bytes([9; 16]);
    let c = PointCloud::from_points(frame, &[]).unwrap();
    assert_eq!(GeometryElement::PointCloud(c).aabb(), Err(GeoTypeError::Empty));
    let m = Mesh::new(frame, &[], &[]).unwrap();
    assert_eq!(GeometryElement::Mesh(m).aabb(), Err(GeoTypeError::Empty));
}

/// AC-4: mesh_cube vertices become 8 points, same frame, same coordinates in vertex order.
#[test]
fn ac4_mesh_cube_vertices_to_point_set() {
    let (frame, v, t) = cube();
    assert_eq!((v.len(), t.len()), (8, 12));
    let m = Mesh::new(frame, &v, &t).unwrap();
    let p = m.vertices_to_point_set();
    assert_eq!(p.len(), 8);
    assert_eq!(p.frame(), frame);
    for (i, want) in v.iter().enumerate() {
        assert_eq!(bits3(p.point(i as u64).unwrap()), bits3(*want), "vertex {i}");
    }
    assert_eq!(p.point(8), None);
    // The source mesh is unchanged and the result is a new element.
    assert_eq!(m.vertex_count(), 8);
    assert_eq!(m.triangles().len(), 12);
}

/// REQ-004: PointCloud::to_point_set keeps frame and points.
#[test]
fn req4_cloud_to_point_set() {
    let s = small();
    let c = PointCloud::from_points(s.frame, &s.points).unwrap();
    let p = c.to_point_set();
    assert_eq!(p.frame(), s.frame);
    assert_eq!(p.x(), c.x());
    assert_eq!(p.y(), c.y());
    assert_eq!(p.z(), c.z());
}

fn drain(src: &mut impl PointSource) -> (Vec<PointBlock>, PointBlock) {
    let mut blocks = Vec::new();
    let mut all = PointBlock::default();
    while let Some(b) = src.next_block() {
        let b = b.unwrap();
        assert!(b.channels.is_empty());
        all.original_index.extend_from_slice(&b.original_index);
        all.x.extend_from_slice(&b.x);
        all.y.extend_from_slice(&b.y);
        all.z.extend_from_slice(&b.z);
        blocks.push(b);
    }
    assert!(src.next_block().is_none(), "a drained source stays drained");
    (blocks, all)
}

/// AC-6: pointset_small through PointSource in blocks of 128 concatenates to the buffer
/// in index order with original indexes 0..999.
#[test]
fn ac6_point_source_blocks_of_128() {
    let s = small();
    let p = PointSet::from_points(s.frame, &s.points).unwrap();
    let mut src = p.source(NonZeroUsize::new(128).unwrap());
    assert_eq!(src.len(), 1000);
    assert!(!src.is_empty());
    assert_eq!(src.frame(), s.frame);
    let (blocks, all) = drain(&mut src);
    // 7 full blocks of 128 and a last block of 104.
    let sizes: Vec<usize> = blocks.iter().map(|b| b.original_index.len()).collect();
    assert_eq!(sizes, [128, 128, 128, 128, 128, 128, 128, 104]);
    for b in &blocks {
        let n = b.original_index.len();
        assert_eq!((b.x.len(), b.y.len(), b.z.len()), (n, n, n));
    }
    assert_eq!(all.original_index, (0..1000u64).collect::<Vec<_>>());
    for (i, want) in s.points.iter().enumerate() {
        assert_eq!(bits3([all.x[i], all.y[i], all.z[i]]), bits3(*want), "point {i}");
    }
}

/// AC-6 for PointCloud, with other block sizes (1, exact divisor, larger than the set).
#[test]
fn ac6_point_source_cloud_block_sizes() {
    let s = small();
    let c = PointCloud::from_points(s.frame, &s.points).unwrap();
    for bs in [1usize, 250, 1000, 4096] {
        let mut src = c.source(NonZeroUsize::new(bs).unwrap());
        assert_eq!(src.frame(), s.frame);
        let (blocks, all) = drain(&mut src);
        assert_eq!(blocks.len(), 1000usize.div_ceil(bs));
        assert_eq!(all.original_index, (0..1000u64).collect::<Vec<_>>());
        assert_eq!(all.x, c.x());
        assert_eq!(all.y, c.y());
        assert_eq!(all.z, c.z());
    }
    let empty = PointSet::from_points(s.frame, &[]).unwrap();
    let mut src = empty.source(NonZeroUsize::new(128).unwrap());
    assert!(src.is_empty());
    assert!(src.next_block().is_none());
}

/// REQ-003 of the parent (used by every constructor): non-finite coordinates, unequal
/// arrays and out-of-range triangle indexes are typed errors.
#[test]
fn constructors_reject_bad_input() {
    let f = FrameRef::from_bytes([4; 16]);
    assert_eq!(
        PointSet::from_points(f, &[[0.0, 0.0, 0.0], [0.0, f64::NAN, 0.0]]),
        Err(GeoTypeError::NonFinite { index: 1 })
    );
    assert_eq!(PointCloud::from_points(f, &[[f64::INFINITY, 0.0, 0.0]]), Err(GeoTypeError::NonFinite { index: 0 }));
    assert_eq!(
        PointSet::new(f, PointBuffer::new(vec![0.0, 1.0], vec![0.0], vec![0.0, 1.0]), AttributeSet::default()),
        Err(GeoTypeError::AttributeLength { name: "y".to_string(), expected: 2, found: 1 })
    );
    let (_, v, _) = cube();
    assert_eq!(Mesh::new(f, &v, &[[0, 1, 2], [0, 8, 1]]), Err(GeoTypeError::IndexOutOfRange { triangle: 1, index: 8 }));
    assert_eq!(Mesh::new(f, &[[0.0, f64::NEG_INFINITY, 0.0]], &[]), Err(GeoTypeError::NonFinite { index: 0 }));
}

/// Aabb: validation, union, containment; the CAD body returns its stored box.
#[test]
fn aabb_helpers_and_cad_body() {
    assert_eq!(Aabb::new([0.0, 0.0, f64::NAN], [1.0; 3]), Err(GeoTypeError::NonFinite { index: 2 }));
    assert_eq!(Aabb::new([0.0; 3], [1.0, 1.0, f64::INFINITY]), Err(GeoTypeError::NonFinite { index: 5 }));
    assert_eq!(Aabb::new([0.0, 2.0, 0.0], [1.0; 3]), Err(GeoTypeError::InvertedAabb { axis: 1 }));
    // Signed zeros follow totalOrder (-0.0 < +0.0), as in of_points and union.
    assert_eq!(Aabb::new([0.0, 0.0, 0.0], [1.0, 1.0, -0.0]), Err(GeoTypeError::InvertedAabb { axis: 2 }));
    let z = Aabb::new([-0.0; 3], [0.0; 3]).unwrap();
    assert_eq!(bits3(z.min()), [(-0.0f64).to_bits(); 3]);
    assert!(Aabb::new([0.0; 3], [0.0; 3]).is_ok());
    let a = Aabb::new([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let b = Aabb::new([-1.0, 0.5, 0.25], [0.5, 2.0, 0.75]).unwrap();
    let u = a.union(&b);
    assert_eq!((u.min(), u.max()), ([-1.0, 0.0, 0.0], [1.0, 2.0, 1.0]));
    assert_eq!(u, b.union(&a));
    assert!(u.contains_aabb(&a) && u.contains_aabb(&b));
    assert!(!a.contains_aabb(&b));
    assert!(a.contains([1.0, 0.0, 0.5]));
    assert!(!a.contains([1.0, 0.0, 1.5]));
    assert!(!a.contains([f64::NAN, 0.5, 0.5]));

    let frame = FrameRef::from_bytes([5; 16]);
    let d = Digest::from_bytes([6; 32]);
    let c = CadBodyRef::new(frame, [7; 16], d, b);
    assert_eq!((c.body(), c.source_digest(), c.frame()), ([7; 16], d, frame));
    let e = GeometryElement::CadBody(c);
    assert_eq!(e.kind(), ElementKind::CadBodyRef);
    assert_eq!(e.aabb(), Ok(b));
}

/// Error codes and kind names are stable strings.
#[test]
fn stable_names() {
    let names: Vec<&str> = ElementKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(names, ["point_set", "point_cloud", "mesh", "cad_body_ref"]);
    assert_eq!(GeoTypeError::Empty.code(), "empty");
    assert_eq!(
        GeoTypeError::WrongKind { expected: ElementKind::PointSet, found: ElementKind::Mesh }.to_string(),
        "wrong element kind: expected point_set, found mesh"
    );
}

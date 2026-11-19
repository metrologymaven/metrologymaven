//! Acceptance criteria of spec node GEO.TYPES.DIGEST (AC-1 .. AC-5) plus requirement
//! checks. Expected digests and layout lengths come from
//! `fixtures/geo/types/digest_vectors.json`, written by the independent Python reference
//! `tools/fixtures/geo_digest_vectors.py` (stdlib `hashlib`); none come from this crate.
//! The elements are built here from the same fixture files and the same rules as listed
//! in that script's docstring.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_types::digest::{canonical_f64_bits, BLOCK_BYTES, LAYOUT_TAG, LAYOUT_VERSION};
use fid_geo_types::{
    digest, digest_cad_body, digest_mesh, digest_point_cloud, digest_point_set, write_layout, Aabb, AttributeSet,
    CadBodyRef, ElementDigest, FrameRef, GeometryElement, IndexWidth, Mesh, MeshOptions, PointBuffer, PointCloud,
    PointSet, TriangleAttrs,
};
use fid_types::Digest;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/types/");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap()
}

fn lines(text: &str) -> impl Iterator<Item = Vec<&str>> + '_ {
    text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).map(|l| l.split_whitespace().collect())
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
    assert_eq!(s.len(), 16);
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn f32_hex(s: &str) -> f32 {
    assert_eq!(s.len(), 8);
    f32::from_bits(u32::from_str_radix(s, 16).unwrap())
}

// ------------------------------------------------------------------ vectors file

struct Vector {
    name: String,
    layout_len: usize,
    sha256: String,
}

/// The value of a `"key": value` line, without quotes and trailing comma.
fn value_of<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.trim().strip_prefix(&format!("\"{key}\": "))?;
    Some(rest.trim_end_matches(',').trim_matches('"'))
}

/// The vectors, read line by line (the generator writes sorted keys, one per line).
fn vectors() -> Vec<Vector> {
    let text = read("digest_vectors.json");
    let mut out = Vec::new();
    let (mut len, mut name) = (None, None);
    for line in text.lines() {
        if let Some(v) = value_of(line, "layout_len") {
            len = Some(v.parse::<usize>().unwrap());
        } else if let Some(v) = value_of(line, "name") {
            name = Some(v.to_string());
        } else if let Some(v) = value_of(line, "sha256") {
            assert_eq!(v.len(), 64);
            out.push(Vector { name: name.take().unwrap(), layout_len: len.take().unwrap(), sha256: v.to_string() });
        }
    }
    assert_eq!(out.len(), 8, "vector count");
    out
}

fn source_digest() -> Digest {
    let text = read("digest_vectors.json");
    let line = text.lines().find_map(|l| value_of(l, "cad_body_source_digest")).unwrap();
    Digest::parse(line).unwrap()
}

// ------------------------------------------------------------------ fixtures

fn small_points() -> (FrameRef, PointBuffer) {
    let mut frame = None;
    let mut buf = PointBuffer::default();
    for f in lines(&read("pointset_small.txt")) {
        match f[0] {
            "frame" => frame = Some(frame_hex(f[1])),
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

fn small_attrs() -> AttributeSet {
    let (mut n, mut inten) = ([Vec::new(), Vec::new(), Vec::new()], Vec::new());
    for f in lines(&read("pointset_small_attrs.txt")) {
        match f[0] {
            "frame" | "count" => {}
            "a" => {
                assert_eq!(f[1].parse::<usize>().unwrap(), inten.len());
                for (k, col) in n.iter_mut().enumerate() {
                    col.push(f64_hex(f[2 + k]));
                }
                inten.push(f32_hex(f[5]));
            }
            other => panic!("unknown line kind {other}"),
        }
    }
    AttributeSet { normal: Some(n), intensity: Some(inten), ..AttributeSet::default() }
}

fn pointset_small() -> PointSet {
    let (frame, buf) = small_points();
    PointSet::new(frame, buf, AttributeSet::default()).unwrap()
}

struct Cube {
    frame: FrameRef,
    v: Vec<[f64; 3]>,
    t: Vec<[u64; 3]>,
}

fn cube() -> Cube {
    let (mut frame, mut v, mut t) = (None, Vec::new(), Vec::new());
    for f in lines(&read("mesh_cube.txt")) {
        match f[0] {
            "frame" => frame = Some(frame_hex(f[1])),
            "v" => v.push([f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap()]),
            "t" => t.push([f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap()]),
            other => panic!("unknown line kind {other}"),
        }
    }
    Cube { frame: frame.unwrap(), v, t }
}

fn mesh_cube(force_u64: bool) -> Mesh {
    let c = cube();
    let options = MeshOptions { force_u64_indexes: force_u64, ..MeshOptions::default() };
    let buf = PointBuffer::from_points(&c.v);
    Mesh::with_options(c.frame, buf, AttributeSet::default(), c.t, options).unwrap()
}

/// mesh_cube plus vertex colour (i*30, 255-i, 7), triangle_scanner_id t%2+1 and
/// triangle_quality 3*t.
fn mesh_cube_attrs() -> Mesh {
    let c = cube();
    let colour = (0..8u8).map(|i| [i * 30, 255 - i, 7]).collect();
    let attrs = AttributeSet { colour: Some(colour), ..AttributeSet::default() };
    let triangle_attrs = TriangleAttrs {
        scanner_id: Some((0..12u32).map(|t| t % 2 + 1).collect()),
        quality: Some((0..12u16).map(|t| 3 * t).collect()),
    };
    let options = MeshOptions { triangle_attrs, ..MeshOptions::default() };
    Mesh::with_options(c.frame, PointBuffer::from_points(&c.v), attrs, c.t, options).unwrap()
}

fn cloud_grid() -> PointCloud {
    let (mut frame, mut shape) = (None, None);
    let mut origins = Vec::new();
    let mut buf = PointBuffer::default();
    let (mut sid, mut inten, mut ts, mut q) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for f in lines(&read("cloud_grid_2scans.txt")) {
        let three = |s: &[&str]| [f64_hex(s[0]), f64_hex(s[1]), f64_hex(s[2])];
        match f[0] {
            "frame" => frame = Some(frame_hex(f[1])),
            "scan_grid" => shape = Some((f[1].parse::<u64>().unwrap(), f[2].parse::<u64>().unwrap())),
            "origin" => origins.push((f[1].parse::<u32>().unwrap(), three(&f[2..5]))),
            "p" => {
                assert_eq!(f[1].parse::<usize>().unwrap(), buf.x.len());
                sid.push(f[2].parse::<u32>().unwrap());
                let p = three(&f[3..6]);
                buf.x.push(p[0]);
                buf.y.push(p[1]);
                buf.z.push(p[2]);
                inten.push(f32_hex(f[6]));
                ts.push(f[7].parse::<i64>().unwrap());
                q.push(f[8].parse::<u16>().unwrap());
            }
            "count" | "aabb_min" | "aabb_max" => {}
            other => panic!("unknown line kind {other}"),
        }
    }
    let (rows, cols) = shape.unwrap();
    let attrs = AttributeSet {
        intensity: Some(inten),
        scanner_id: Some(sid),
        timestamp_ns: Some(ts),
        quality: Some(q),
        scan_origins: origins,
        grid: Some((2 * rows, cols)),
        ..AttributeSet::default()
    };
    PointCloud::new(frame.unwrap(), buf, attrs).unwrap()
}

fn cad_body() -> CadBodyRef {
    let frame = FrameRef::from_bytes(core::array::from_fn(|i| i as u8));
    let body = core::array::from_fn(|i| 16 + i as u8);
    let aabb = Aabb::new([0.0; 3], [1.0; 3]).unwrap();
    CadBodyRef::new(frame, body, source_digest(), aabb)
}

fn element(name: &str) -> GeometryElement {
    match name {
        "pointset_small" => GeometryElement::PointSet(pointset_small()),
        "mesh_cube" => GeometryElement::Mesh(mesh_cube(false)),
        "cloud_grid_2scans" => GeometryElement::PointCloud(cloud_grid()),
        "pointset_small_attrs" => {
            let (frame, buf) = small_points();
            GeometryElement::PointSet(PointSet::new(frame, buf, small_attrs()).unwrap())
        }
        "pointset_small_cloud" => {
            let (frame, buf) = small_points();
            GeometryElement::PointCloud(PointCloud::new(frame, buf, AttributeSet::default()).unwrap())
        }
        "pointset_empty" => {
            let (frame, _) = small_points();
            GeometryElement::PointSet(PointSet::new(frame, PointBuffer::default(), AttributeSet::default()).unwrap())
        }
        "mesh_cube_attrs" => GeometryElement::Mesh(mesh_cube_attrs()),
        "cad_body" => GeometryElement::CadBody(cad_body()),
        other => panic!("no builder for vector {other}"),
    }
}

/// The layout as delivered to the sink, block by block.
fn blocks(e: &GeometryElement) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    write_layout(e, &mut |b: &[u8]| out.push(b.to_vec()));
    out
}

fn kind_digest(e: &GeometryElement) -> ElementDigest {
    match e {
        GeometryElement::PointSet(p) => digest_point_set(p),
        GeometryElement::PointCloud(p) => digest_point_cloud(p),
        GeometryElement::Mesh(m) => digest_mesh(m),
        GeometryElement::CadBody(c) => digest_cad_body(c),
    }
}

// ------------------------------------------------------------------ acceptance

/// AC-1: every vector's digest and layout length match the independent Python reference
/// (run on Linux and Windows x86-64, macOS ARM64 and wasm32 in CI).
#[test]
fn ac1_vectors_match() {
    for v in vectors() {
        let e = element(&v.name);
        let d = digest(&e);
        assert_eq!(d.to_string(), v.sha256, "digest of {}", v.name);
        assert_eq!(kind_digest(&e), d, "per-kind digest of {}", v.name);
        assert_eq!(d.to_digest().hex(), v.sha256);
        let bl = blocks(&e);
        let total: usize = bl.iter().map(Vec::len).sum();
        assert_eq!(total, v.layout_len, "layout length of {}", v.name);
        let layout: Vec<u8> = bl.concat();
        assert_eq!(&layout[..6], &LAYOUT_TAG);
        assert_eq!(layout[6], LAYOUT_VERSION);
        assert_eq!(&layout[8..24], &e.frame().to_bytes());
    }
}

/// REQ-GEO.TYPES.DIGEST-005: the layout streams in full blocks of at most 64 KiB (only
/// the last may be short); cloud_grid_2scans spans several blocks.
#[test]
fn req5_streams_in_bounded_blocks() {
    let e = GeometryElement::PointCloud(cloud_grid());
    let bl = blocks(&e);
    assert!(bl.len() >= 3, "cloud_grid_2scans should span several blocks");
    let (last, full) = bl.split_last().unwrap();
    assert!(full.iter().all(|b| b.len() == BLOCK_BYTES));
    assert!(!last.is_empty() && last.len() <= BLOCK_BYTES);
    assert_eq!(BLOCK_BYTES, 64 * 1024);
}

/// AC-2: a one-ULP change of any coordinate changes the digest.
#[test]
fn ac2_one_ulp_changes_digest() {
    let (frame, buf) = small_points();
    let base = digest_point_set(&PointSet::new(frame, buf.clone(), AttributeSet::default()).unwrap());
    for (axis, i) in [(0usize, 0usize), (1, 517), (2, 999)] {
        let mut b = buf.clone();
        let col = match axis {
            0 => &mut b.x,
            1 => &mut b.y,
            _ => &mut b.z,
        };
        col[i] = f64::from_bits(col[i].to_bits() + 1);
        let d = digest_point_set(&PointSet::new(frame, b, AttributeSet::default()).unwrap());
        assert_ne!(d, base, "axis {axis} point {i}");
    }
}

/// AC-3: point 0's x as -0.0 and +0.0 give equal digests; so do signed zeros in normals,
/// scan origins and intensity.
#[test]
fn ac3_signed_zero_is_normalised() {
    let (frame, buf) = small_points();
    let with_x0 = |x: f64| {
        let mut b = buf.clone();
        b.x[0] = x;
        digest_point_set(&PointSet::new(frame, b, AttributeSet::default()).unwrap())
    };
    assert_eq!(with_x0(-0.0), with_x0(0.0));
    assert_ne!(with_x0(0.0), with_x0(f64::from_bits(1)), "the smallest subnormal is not zero");
    assert_eq!(canonical_f64_bits(-0.0), 0);

    let pts = PointBuffer::from_points(&[[0.0, 0.0, 0.0]]);
    let attrs = |nz: f64, ox: f64, i: f32| AttributeSet {
        normal: Some([vec![nz], vec![0.0], vec![1.0]]),
        intensity: Some(vec![i]),
        scan_origins: vec![(1, [ox, 0.0, 0.0])],
        ..AttributeSet::default()
    };
    let d = |a: AttributeSet| digest_point_set(&PointSet::new(frame, pts.clone(), a).unwrap());
    assert_eq!(d(attrs(-0.0, -0.0, -0.0)), d(attrs(0.0, 0.0, 0.0)));
}

/// f32 intensity is not validated, so it may hold NaN: every NaN payload digests alike,
/// and a NaN differs from every number.
#[test]
fn nan_intensity_is_canonical() {
    let frame = FrameRef::from_bytes([5; 16]);
    let pts = PointBuffer::from_points(&[[1.0, 2.0, 3.0]]);
    let d = |i: f32| {
        let a = AttributeSet { intensity: Some(vec![i]), ..AttributeSet::default() };
        digest_point_set(&PointSet::new(frame, pts.clone(), a).unwrap())
    };
    let quiet = d(f32::NAN);
    assert_eq!(d(f32::from_bits(0xffc0_0001)), quiet);
    assert_eq!(d(f32::from_bits(0x7f80_0001)), quiet);
    assert_ne!(d(f32::INFINITY), quiet);
    assert_ne!(d(f32::INFINITY), d(f32::NEG_INFINITY));
    assert_ne!(d(1.0), quiet);
}

/// AC-4: mesh_cube with u32 and u64 index storage digests alike (and matches AC-1).
#[test]
fn ac4_index_width_does_not_matter() {
    let (a, b) = (mesh_cube(false), mesh_cube(true));
    assert_eq!((a.index_width(), b.index_width()), (IndexWidth::U32, IndexWidth::U64));
    assert_eq!(digest_mesh(&a), digest_mesh(&b));
    let want = vectors().into_iter().find(|v| v.name == "mesh_cube").unwrap();
    assert_eq!(digest_mesh(&b).to_string(), want.sha256);
}

/// AC-5: pointset_small in two frames gives two digests.
#[test]
fn ac5_frame_changes_digest() {
    let (frame, buf) = small_points();
    let other = FrameRef::from_bytes([0xab; 16]);
    assert_ne!(frame, other);
    let a = digest_point_set(&PointSet::new(frame, buf.clone(), AttributeSet::default()).unwrap());
    let b = digest_point_set(&PointSet::new(other, buf, AttributeSet::default()).unwrap());
    assert_ne!(a, b);
}

// ------------------------------------------------------------------ requirements

/// REQ-GEO.TYPES.DIGEST-004: a change to any index, attribute value, channel presence or
/// kind changes the digest.
#[test]
fn req4_every_field_counts() {
    let base = mesh_cube_attrs();
    let d0 = digest_mesh(&base);
    let c = cube();
    let rebuild = |v: &[[f64; 3]], t: Vec<[u64; 3]>, a: AttributeSet, ta: TriangleAttrs| {
        let options = MeshOptions { triangle_attrs: ta, ..MeshOptions::default() };
        digest_mesh(&Mesh::with_options(c.frame, PointBuffer::from_points(v), a, t, options).unwrap())
    };
    let (a0, ta0) = (base.vertex_attrs().clone(), base.triangle_attrs().clone());
    assert_eq!(rebuild(&c.v, c.t.clone(), a0.clone(), ta0.clone()), d0, "rebuild is stable");

    // A triangle index (the same triangle rotated has other index positions).
    let mut t = c.t.clone();
    t[3] = [t[3][1], t[3][2], t[3][0]];
    assert_ne!(rebuild(&c.v, t, a0.clone(), ta0.clone()), d0);
    // Triangle order.
    let mut t = c.t.clone();
    t.swap(0, 1);
    assert_ne!(rebuild(&c.v, t, a0.clone(), ta0.clone()), d0);
    // A vertex colour byte.
    let mut a = a0.clone();
    a.colour.as_mut().unwrap()[4][2] ^= 1;
    assert_ne!(rebuild(&c.v, c.t.clone(), a, ta0.clone()), d0);
    // A per-triangle value, in each channel.
    let mut ta = ta0.clone();
    ta.scanner_id.as_mut().unwrap()[11] += 1;
    assert_ne!(rebuild(&c.v, c.t.clone(), a0.clone(), ta), d0);
    let mut ta = ta0.clone();
    ta.quality.as_mut().unwrap()[0] = 1;
    assert_ne!(rebuild(&c.v, c.t.clone(), a0.clone(), ta), d0);
    // Dropping a channel.
    let ta = TriangleAttrs { quality: None, ..ta0.clone() };
    assert_ne!(rebuild(&c.v, c.t.clone(), a0.clone(), ta), d0);
    // Topology facts are not hashed, but the plain cube differs from the attributed one.
    assert_ne!(digest_mesh(&mesh_cube(false)), d0);

    // Point attributes of the cloud: each channel and the grid and origins.
    let cloud = cloud_grid();
    let g0 = digest_point_cloud(&cloud);
    let (frame, buf, attrs) = cloud.clone().into_parts();
    let redo = |a: AttributeSet| digest_point_cloud(&PointCloud::new(frame, buf.clone(), a).unwrap());
    let mut a = attrs.clone();
    a.intensity.as_mut().unwrap()[100] = f32::from_bits(a.intensity.as_ref().unwrap()[100].to_bits() ^ 1);
    assert_ne!(redo(a), g0, "intensity");
    let mut a = attrs.clone();
    a.scanner_id.as_mut().unwrap()[0] = 2;
    assert_ne!(redo(a), g0, "scanner_id");
    let mut a = attrs.clone();
    a.timestamp_ns.as_mut().unwrap()[6143] -= 1;
    assert_ne!(redo(a), g0, "timestamp_ns");
    let mut a = attrs.clone();
    a.quality.as_mut().unwrap()[7] ^= 0x8000;
    assert_ne!(redo(a), g0, "quality");
    let mut a = attrs.clone();
    a.grid = Some((48, 128));
    assert_ne!(redo(a), g0, "grid shape");
    let mut a = attrs.clone();
    a.grid = None;
    assert_ne!(redo(a), g0, "grid absent");
    let mut a = attrs.clone();
    a.scan_origins[1].1[2] = f64::from_bits(a.scan_origins[1].1[2].to_bits() + 1);
    assert_ne!(redo(a), g0, "scan origin");
    let mut a = attrs.clone();
    a.scan_origins[1].0 = 3;
    assert_ne!(redo(a), g0, "scan origin ID");
    assert_ne!(digest_point_set(&cloud.to_point_set()), g0, "kind byte");

    // A normal component, by one ULP.
    let (sframe, sbuf) = small_points();
    let sa = small_attrs();
    let s0 = digest_point_set(&PointSet::new(sframe, sbuf.clone(), sa.clone()).unwrap());
    let mut a = sa.clone();
    let n = &mut a.normal.as_mut().unwrap()[1][3];
    *n = f64::from_bits(n.to_bits() + 1);
    assert_ne!(digest_point_set(&PointSet::new(sframe, sbuf, a).unwrap()), s0, "normal");
}

/// A present channel with zero records differs from an absent one, and channel values
/// cannot slide between channels (each carries its own name, type and count).
#[test]
fn req4_empty_and_absent_channels_differ() {
    let frame = FrameRef::from_bytes([9; 16]);
    let d = |a: AttributeSet| digest_point_set(&PointSet::new(frame, PointBuffer::default(), a).unwrap());
    let none = d(AttributeSet::default());
    let empty_q = d(AttributeSet { quality: Some(Vec::new()), ..AttributeSet::default() });
    let empty_s = d(AttributeSet { scanner_id: Some(Vec::new()), ..AttributeSet::default() });
    assert_ne!(none, empty_q);
    assert_ne!(empty_q, empty_s);
    // An empty origin list is the same as none (there is no Option around it).
    assert_eq!(d(AttributeSet { scan_origins: Vec::new(), ..AttributeSet::default() }), none);
}

/// CAD body handles hash frame, body ID and source digest; the import-time box is not
/// part of the layout (the spec names body and source digest only).
#[test]
fn cad_body_fields() {
    let c = cad_body();
    let d0 = digest_cad_body(&c);
    let box2 = Aabb::new([-1.0; 3], [2.0; 3]).unwrap();
    assert_eq!(digest_cad_body(&CadBodyRef::new(c.frame(), c.body(), c.source_digest(), box2)), d0);
    let mut body = c.body();
    body[15] ^= 1;
    assert_ne!(digest_cad_body(&CadBodyRef::new(c.frame(), body, c.source_digest(), c.aabb())), d0);
    let mut src = c.source_digest().to_bytes();
    src[0] ^= 0x80;
    assert_ne!(digest_cad_body(&CadBodyRef::new(c.frame(), c.body(), Digest::from_bytes(src), c.aabb())), d0);
    let other = FrameRef::from_bytes([0; 16]);
    assert_ne!(digest_cad_body(&CadBodyRef::new(other, c.body(), c.source_digest(), c.aabb())), d0);
}

#[test]
fn display_and_shared_digest_form() {
    let d = ElementDigest([0xa5; 32]);
    assert_eq!(d.to_string(), "a5".repeat(32));
    assert_eq!(format!("{d:?}"), format!("ElementDigest({})", "a5".repeat(32)));
    let shared: Digest = d.into();
    assert_eq!(shared.as_str(), format!("sha256:{}", "a5".repeat(32)));
    assert_eq!(shared.to_bytes(), d.to_bytes());
}

/// Digesting twice gives the same result, and digests of clones agree (no hidden state).
#[test]
fn digest_is_deterministic() {
    let e = GeometryElement::PointCloud(cloud_grid());
    assert_eq!(digest(&e), digest(&e.clone()));
}

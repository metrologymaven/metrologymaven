//! Acceptance criteria of spec node GEO.FRAME.APPLY (AC-1 to AC-4) plus requirement checks.
//! Expected bits come from `fixtures/geo/frame/apply_expected.bin`, written by the independent
//! Python reference `tools/fixtures/geo_apply_cases.py` (stdlib; bit-exact float model of the
//! pinned expressions plus an exact `fractions` check); none come from this crate. Inputs are
//! the existing GEO.TYPES fixtures and transform 17 of `xform_cases.json`. AC-3 and AC-4 are
//! hand-labelled from the spec. Run natively on x86-64 and ARM64 and on wasm32-wasip1 by CI.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_frame::apply::{
    require_same_frame, require_same_frame_ref, transform_element, transform_element_with_provenance, GeoError,
};
use fid_geo_frame::xform::{Rigid, Similarity, Transform, IDENTITY};
use fid_geo_types::{
    Aabb, AttributeSet, CadBodyRef, ElementKind, FrameRef, GeoTypeError, GeometryElement, Mesh, MeshOptions,
    PointBuffer, PointCloud, PointSet, TriangleAttrs,
};
use fid_types::Digest;

const TYPES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/types/");
const FRAME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/frame/");

/// The target frame of AC-1 (any frame other than the fixtures' own).
const TARGET: FrameRef = FrameRef::from_bytes(*b"GEO.FRAME.APPLY!");

fn read(path: String) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn lines(text: &str) -> impl Iterator<Item = Vec<&str>> + '_ {
    text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).map(|l| l.split_whitespace().collect())
}

fn frame_hex(s: &str) -> FrameRef {
    let mut b = [0u8; 16];
    for (i, o) in b.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
    }
    FrameRef::from_bytes(b)
}

fn f64_hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn three(s: &[&str]) -> [f64; 3] {
    [f64_hex(s[0]), f64_hex(s[1]), f64_hex(s[2])]
}

fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|x| x.to_bits()).collect()
}

// ------------------------------------------------------------------ fixtures

/// The value of `"key":"..."` in one case line of `xform_cases.json`.
fn field<'a>(line: &'a str, key: &str) -> &'a str {
    let pat = format!("\"{key}\":\"");
    let start = line.find(&pat).unwrap() + pat.len();
    let rest = &line[start..];
    &rest[..rest.find('"').unwrap()]
}

fn hexes(s: &str) -> Vec<f64> {
    s.split(' ').map(f64_hex).collect()
}

fn xform_transforms() -> Vec<Transform> {
    let text = read(format!("{FRAME}xform_cases.json"));
    let cases: Vec<&str> = text.lines().map(str::trim).filter(|l| l.starts_with("{\"")).collect();
    assert_eq!(cases.len(), 200);
    cases
        .iter()
        .map(|l| {
            let r = hexes(field(l, "r"));
            let t = hexes(field(l, "t"));
            let rigid =
                Rigid::new([[r[0], r[1], r[2]], [r[3], r[4], r[5]], [r[6], r[7], r[8]]], [t[0], t[1], t[2]]).unwrap();
            match field(l, "kind") {
                "rigid" => Transform::Rigid(rigid),
                _ => Transform::Similarity(Similarity::new(rigid, hexes(field(l, "s"))[0]).unwrap()),
            }
        })
        .collect()
}

/// Transform 17 ("random_11", a similarity), the AC-1 transform.
fn t17() -> Transform {
    let t = xform_transforms()[17];
    assert!(!t.is_rigid());
    t
}

fn cloud() -> PointCloud {
    let text = read(format!("{TYPES}cloud_grid_2scans.txt"));
    let (mut frame, mut shape) = (None, None);
    let mut origins = Vec::new();
    let mut buf = PointBuffer::default();
    let (mut sid, mut inten, mut ts, mut q) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for f in lines(&text) {
        match f[0] {
            "frame" => frame = Some(frame_hex(f[1])),
            "scan_grid" => shape = Some((f[1].parse::<u64>().unwrap(), f[2].parse::<u64>().unwrap())),
            "origin" => origins.push((f[1].parse::<u32>().unwrap(), three(&f[2..5]))),
            "p" => {
                sid.push(f[2].parse::<u32>().unwrap());
                let p = three(&f[3..6]);
                buf.x.push(p[0]);
                buf.y.push(p[1]);
                buf.z.push(p[2]);
                inten.push(f32::from_bits(u32::from_str_radix(f[6], 16).unwrap()));
                ts.push(f[7].parse::<i64>().unwrap());
                q.push(f[8].parse::<u16>().unwrap());
            }
            _ => {}
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

fn pointset_small() -> PointSet {
    let text = read(format!("{TYPES}pointset_small.txt"));
    let mut frame = None;
    let mut buf = PointBuffer::default();
    for f in lines(&text) {
        match f[0] {
            "frame" => frame = Some(frame_hex(f[1])),
            "p" => {
                let p = three(&f[2..5]);
                buf.x.push(p[0]);
                buf.y.push(p[1]);
                buf.z.push(p[2]);
            }
            _ => {}
        }
    }
    let text = read(format!("{TYPES}pointset_small_attrs.txt"));
    let mut n: [Vec<f64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for f in lines(&text) {
        if f[0] == "a" {
            let v = three(&f[2..5]);
            for (col, x) in n.iter_mut().zip(v) {
                col.push(x);
            }
        }
    }
    let attrs = AttributeSet { normal: Some(n), ..AttributeSet::default() };
    PointSet::new(frame.unwrap(), buf, attrs).unwrap()
}

fn mesh_cube() -> Mesh {
    let text = read(format!("{TYPES}mesh_cube.txt"));
    let mut frame = None;
    let (mut v, mut t) = (Vec::new(), Vec::new());
    for f in lines(&text) {
        match f[0] {
            "frame" => frame = Some(frame_hex(f[1])),
            "v" => v.push([f[1].parse::<f64>().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap()]),
            "t" => t.push([f[1].parse::<u64>().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap()]),
            _ => {}
        }
    }
    Mesh::from_points(frame.unwrap(), &v, &t).unwrap()
}

/// `apply_expected.bin`, layout as documented in `tools/fixtures/geo_apply_cases.py`.
struct Expected {
    cloud: [Vec<f64>; 3],
    origins: Vec<(u32, [f64; 3])>,
    pset: [Vec<f64>; 3],
    normals: [Vec<f64>; 3],
    cube: [Vec<f64>; 3],
}

struct Cursor<'a>(&'a [u8]);

impl Cursor<'_> {
    fn u64(&mut self) -> u64 {
        let (h, rest) = self.0.split_at(8);
        self.0 = rest;
        u64::from_le_bytes(h.try_into().unwrap())
    }
    fn f64(&mut self) -> f64 {
        f64::from_bits(self.u64())
    }
    fn cols(&mut self, n: usize) -> [Vec<f64>; 3] {
        [0, 1, 2].map(|_| (0..n).map(|_| self.f64()).collect())
    }
}

fn expected() -> Expected {
    let data = std::fs::read(format!("{FRAME}apply_expected.bin")).unwrap();
    assert_eq!(&data[..8], b"FGFAPPL1");
    let mut c = Cursor(&data[8..]);
    assert_eq!(c.u64(), 17);
    let n = c.u64() as usize;
    let cloud = c.cols(n);
    let m = c.u64() as usize;
    let origins = (0..m).map(|_| (c.u64() as u32, [c.f64(), c.f64(), c.f64()])).collect();
    let n = c.u64() as usize;
    let pset = c.cols(n);
    let normals = c.cols(n);
    let n = c.u64() as usize;
    let cube = c.cols(n);
    assert!(c.0.is_empty(), "trailing bytes");
    Expected { cloud, origins, pset, normals, cube }
}

fn assert_cols(what: &str, got: &PointBuffer, want: &[Vec<f64>; 3]) {
    assert_eq!(bits(&got.x), bits(&want[0]), "{what} x");
    assert_eq!(bits(&got.y), bits(&want[1]), "{what} y");
    assert_eq!(bits(&got.z), bits(&want[2]), "{what} z");
}

fn norm(n: [f64; 3]) -> f64 {
    ((n[0] * n[0] + n[1] * n[1]) + n[2] * n[2]).sqrt()
}

fn cad_body(frame: FrameRef) -> CadBodyRef {
    let aabb = Aabb::new([0.0; 3], [1.0; 3]).unwrap();
    CadBodyRef::new(frame, [5; 16], Digest::from_bytes([9; 32]), aabb)
}

// ------------------------------------------------------------------ acceptance

/// AC-1: the two-scan cloud through transform 17 matches the oracle bit for bit; grid shape,
/// scanner IDs and every other channel are unchanged; the frame is the target; the input is
/// untouched. The cloud fixture has no normals, so the normal half of AC-1 runs on
/// `pointset_small` with its normals (bits from the same oracle, |n| within 1e-12 of 1); the
/// cube mesh is checked too.
#[test]
fn ac1_cloud_bit_exact_channels_kept() {
    let t = t17();
    let want = expected();

    let src = cloud();
    let before = src.clone();
    let e = GeometryElement::PointCloud(src);
    let out = transform_element(&e, &t, TARGET).unwrap();
    assert_eq!(e, GeometryElement::PointCloud(before.clone()), "input untouched");
    let out = out.as_point_cloud().unwrap();
    assert_eq!(out.frame(), TARGET);
    assert_cols("cloud", out.buffer(), &want.cloud);
    let (a, b) = (out.attributes(), before.attributes());
    assert_eq!(a.grid, Some((128, 48)));
    assert_eq!(a.grid, b.grid);
    assert_eq!(a.scanner_id, b.scanner_id);
    assert_eq!(
        a.intensity.as_ref().map(|v| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>()),
        b.intensity.as_ref().map(|v| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>())
    );
    assert_eq!(a.timestamp_ns, b.timestamp_ns);
    assert_eq!(a.quality, b.quality);
    assert_eq!(a.colour, b.colour);
    assert!(a.normal.is_none());
    assert_eq!(a.scan_origins.len(), want.origins.len());
    for ((gid, go), (wid, wo)) in a.scan_origins.iter().zip(&want.origins) {
        assert_eq!(gid, wid);
        assert_eq!(bits(go), bits(wo), "origin {gid}");
    }

    let ps = pointset_small();
    let out = transform_element(&GeometryElement::PointSet(ps.clone()), &t, TARGET).unwrap();
    let out = out.as_point_set().unwrap();
    assert_eq!(out.frame(), TARGET);
    assert_cols("pointset", out.buffer(), &want.pset);
    let n = out.attributes().normal.as_ref().unwrap();
    for (k, (got, w)) in n.iter().zip(&want.normals).enumerate() {
        assert_eq!(bits(got), bits(w), "normal column {k}");
    }
    for i in 0..ps.len() {
        let m = out.attributes().normal_at(i).unwrap();
        assert!((norm(m) - 1.0).abs() <= 1e-12, "normal {i} length {}", norm(m));
    }

    let cube = mesh_cube();
    let out = transform_element(&GeometryElement::Mesh(cube.clone()), &t, TARGET).unwrap();
    let out = out.as_mesh().unwrap();
    assert_eq!(out.frame(), TARGET);
    assert_cols("mesh", out.vertices(), &want.cube);
    assert_eq!(out.triangles().collect::<Vec<_>>(), cube.triangles().collect::<Vec<_>>());
    assert_eq!(out.facts(), cube.facts());
    assert_eq!(out.index_width(), cube.index_width());
}

/// AC-2: the AC-1 output mapped by the inverse transform returns within 1e-12 m.
#[test]
fn ac2_inverse_round_trip() {
    let t = t17();
    let inv = t.inverse().unwrap();
    let src = cloud();
    let e = GeometryElement::PointCloud(src.clone());
    let fwd = transform_element(&e, &t, TARGET).unwrap();
    let back = transform_element(&fwd, &inv, src.frame()).unwrap();
    let back = back.as_point_cloud().unwrap();
    assert_eq!(back.frame(), src.frame());
    let mut worst = 0.0f64;
    let (g, w) = (back.buffer(), src.buffer());
    for (gc, wc) in [(&g.x, &w.x), (&g.y, &w.y), (&g.z, &w.z)] {
        for (a, b) in gc.iter().zip(wc) {
            let d = (a - b).abs();
            if d > worst {
                worst = d;
            }
        }
    }
    assert!(worst <= 1e-12, "max round-trip error {worst:e} m");
    for ((_, a), (_, b)) in back.attributes().scan_origins.iter().zip(&src.attributes().scan_origins) {
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() <= 1e-12);
        }
    }
}

/// AC-3: point set in frame A and mesh in frame B give `FrameMismatch { left: A, right: B }`.
#[test]
fn ac3_frame_mismatch() {
    let a = GeometryElement::PointSet(pointset_small());
    let b = GeometryElement::Mesh(mesh_cube());
    let (fa, fb) = (a.frame(), b.frame());
    assert_ne!(fa, fb);
    let err = require_same_frame(&a, &b).unwrap_err();
    assert_eq!(err, GeoError::FrameMismatch { left: fa, right: fb });
    assert_eq!(err.code(), "frame_mismatch");
    assert_eq!(require_same_frame(&b, &a), Err(GeoError::FrameMismatch { left: fb, right: fa }));
    // Same frame: the shared frame, nothing converted.
    assert_eq!(require_same_frame(&a, &a), Ok(fa));
    let moved = transform_element(&b, &Transform::IDENTITY, fa).unwrap();
    assert_eq!(require_same_frame(&a, &moved), Ok(fa));
    assert_eq!(require_same_frame_ref(fa, fb), Err(GeoError::FrameMismatch { left: fa, right: fb }));
    // CAD body handles are compared too.
    let c = GeometryElement::CadBody(cad_body(fb));
    assert_eq!(require_same_frame(&c, &b), Ok(fb));
    assert_eq!(require_same_frame(&a, &c), Err(GeoError::FrameMismatch { left: fa, right: fb }));
}

/// AC-4: a CAD body handle is refused with `Unsupported { kind: CadBodyRef }`.
#[test]
fn ac4_cad_body_refused() {
    let c = GeometryElement::CadBody(cad_body(TARGET));
    let want = GeoError::Unsupported { kind: ElementKind::CadBodyRef };
    assert_eq!(transform_element(&c, &t17(), TARGET), Err(want.clone()));
    assert_eq!(transform_element_with_provenance(&c, &Transform::IDENTITY, TARGET).unwrap_err(), want);
    assert_eq!(want.code(), "unsupported");
}

// ------------------------------------------------------------------ requirements

/// REQ-002: under every rigid and similarity transform of `xform_cases.json`, normals are
/// rotated only (no scale) and keep their length within 1e-12; points use the full transform.
#[test]
fn req2_normals_rotation_only_all_transforms() {
    let ps = pointset_small();
    let e = GeometryElement::PointSet(ps.clone());
    for (k, t) in xform_transforms().iter().enumerate() {
        let out = transform_element(&e, t, TARGET).unwrap();
        let out = out.as_point_set().unwrap();
        for i in 0..ps.len() {
            let n = ps.attributes().normal_at(i).unwrap();
            let m = out.attributes().normal_at(i).unwrap();
            assert_eq!(bits(&m), bits(&t.apply_dir(&n).unwrap()), "transform {k} normal {i}");
            assert!((norm(m) - norm(n)).abs() <= 1e-12, "transform {k} normal {i}");
            assert!((norm(m) - 1.0).abs() <= 1e-12, "transform {k} normal {i}");
            let p = ps.point(i).unwrap();
            assert_eq!(bits(&out.point(i).unwrap()), bits(&t.apply_point(&p).unwrap()));
        }
    }
}

/// REQ-001: mesh vertex attributes, per-triangle attributes and forced u64 indexes are kept.
#[test]
fn req1_mesh_channels_kept() {
    let frame = FrameRef::from_bytes([1; 16]);
    let v = PointBuffer::from_points(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    let attrs = AttributeSet {
        normal: Some([vec![0.0; 3], vec![0.0; 3], vec![1.0; 3]]),
        colour: Some(vec![[1, 2, 3], [4, 5, 6], [7, 8, 9]]),
        ..AttributeSet::default()
    };
    let opts = MeshOptions {
        triangle_attrs: TriangleAttrs { scanner_id: Some(vec![7]), quality: Some(vec![3]) },
        force_u64_indexes: true,
        work_budget_bytes: None,
    };
    let m = Mesh::with_options(frame, v, attrs, vec![[0, 1, 2]], opts).unwrap();
    // A quarter turn about x with a translation and a scale of 2.
    let r = [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]];
    let t = Transform::Similarity(Similarity::new(Rigid::new(r, [10.0, 20.0, 30.0]).unwrap(), 2.0).unwrap());
    let out = transform_element(&GeometryElement::Mesh(m.clone()), &t, TARGET).unwrap();
    let out = out.as_mesh().unwrap();
    assert_eq!(out.vertex(1), Some([12.0, 20.0, 30.0]));
    assert_eq!(out.vertex(2), Some([10.0, 20.0, 32.0]));
    // The +z normal turns to -y and is not scaled.
    assert_eq!(out.vertex_attrs().normal_at(0), Some([0.0, -1.0, 0.0]));
    assert_eq!(out.vertex_attrs().colour, m.vertex_attrs().colour);
    assert_eq!(out.triangle_attrs(), m.triangle_attrs());
    assert_eq!(out.index_width(), m.index_width());
    assert_eq!(out.facts(), m.facts());
}

/// Provenance: the source frame, target frame and transform travel with the result.
#[test]
fn provenance_recorded() {
    let ps = pointset_small();
    let t = t17();
    let e = GeometryElement::PointSet(ps.clone());
    let got = transform_element_with_provenance(&e, &t, TARGET).unwrap();
    assert_eq!(got.provenance.source, ps.frame());
    assert_eq!(got.provenance.target, TARGET);
    assert_eq!(got.provenance.transform, t);
    assert_eq!(got.element, transform_element(&e, &t, TARGET).unwrap());
}

/// An overflowing result is a typed error naming the first offending point, not a panic.
#[test]
fn overflow_is_typed_error() {
    let frame = FrameRef::from_bytes([2; 16]);
    let ps = PointSet::from_points(frame, &[[1.0, 0.0, 0.0], [1e308, 0.0, 0.0]]).unwrap();
    let t = Transform::Similarity(Similarity::new(Rigid::new(IDENTITY, [0.0; 3]).unwrap(), 4.0).unwrap());
    assert_eq!(
        transform_element(&GeometryElement::PointSet(ps), &t, TARGET),
        Err(GeoError::Element(GeoTypeError::NonFinite { index: 1 }))
    );
    let attrs = AttributeSet { scan_origins: vec![(3, [0.0; 3]), (5, [-1e308, 0.0, 0.0])], ..AttributeSet::default() };
    let pc = PointCloud::new(frame, PointBuffer::from_points(&[[0.0; 3]]), attrs).unwrap();
    assert_eq!(
        transform_element(&GeometryElement::PointCloud(pc), &t, TARGET),
        Err(GeoError::Element(GeoTypeError::ScanOrigin { scanner_id: 5 }))
    );
}

/// An empty element transforms to an empty element in the target frame.
#[test]
fn empty_element() {
    let ps = PointSet::from_points(FrameRef::from_bytes([4; 16]), &[]).unwrap();
    let out = transform_element(&GeometryElement::PointSet(ps), &t17(), TARGET).unwrap();
    assert_eq!(out.frame(), TARGET);
    assert!(out.as_point_set().unwrap().is_empty());
}

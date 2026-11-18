//! Acceptance criteria of spec node GEO.TYPES.POINTS (AC-1 .. AC-6) plus requirement
//! checks. Expected values come from `fixtures/geo/types/make_points_fixtures.py` and
//! `make_pointset_small.py` (stdlib Python), never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_types::{
    is_unit_normal, AttributeSet, ElementKind, FrameRef, GeoTypeError, GeometryElement, PointBuffer, PointCloud,
    PointSet, NORMAL_TOLERANCE,
};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/types/");

// AC-6: counts and indexes are u64, checked at compile time.
const _: fn(&PointSet) -> u64 = PointSet::len;
const _: fn(&PointCloud) -> u64 = PointCloud::len;
const _: fn(&PointSet, u64) -> Option<[f64; 3]> = PointSet::point;
const _: fn(&PointCloud, u64) -> Option<[f64; 3]> = PointCloud::point;
const _: fn(&PointBuffer) -> u64 = PointBuffer::len;
const _: fn(&AttributeSet, u64) -> Option<[f64; 3]> = AttributeSet::normal_at;

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

fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|x| x.to_bits()).collect()
}

fn bits3(v: [f64; 3]) -> [u64; 3] {
    [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()]
}

/// pointset_small (GEO.TYPES.ENV fixture) plus its normals and intensity.
struct Small {
    frame: FrameRef,
    buf: PointBuffer,
    attrs: AttributeSet,
}

fn small() -> Small {
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
    let (mut n, mut inten) = ([Vec::new(), Vec::new(), Vec::new()], Vec::new());
    let mut count = None;
    for f in lines(&read("pointset_small_attrs.txt")) {
        match f[0] {
            "frame" => assert_eq!(Some(frame_hex(f[1])), frame, "attribute file is for pointset_small"),
            "count" => count = Some(f[1].parse::<usize>().unwrap()),
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
    assert_eq!(count, Some(1000));
    assert_eq!((buf.x.len(), inten.len()), (1000, 1000));
    let attrs = AttributeSet { normal: Some(n), intensity: Some(inten), ..AttributeSet::default() };
    Small { frame: frame.unwrap(), buf, attrs }
}

/// AC-1: pointset_small with normals and intensity builds, has 1 000 points and reads
/// back bit-identically (coordinates and attributes).
#[test]
fn ac1_pointset_small_with_attributes() {
    let s = small();
    let p = PointSet::new(s.frame, s.buf.clone(), s.attrs.clone()).unwrap();
    assert_eq!(p.len(), 1000);
    assert!(!p.is_empty());
    assert_eq!(p.frame(), s.frame);
    assert_eq!(bits(p.x()), bits(&s.buf.x));
    assert_eq!(bits(p.y()), bits(&s.buf.y));
    assert_eq!(bits(p.z()), bits(&s.buf.z));
    for i in 0..1000u64 {
        let k = i as usize;
        assert_eq!(bits3(p.point(i).unwrap()), bits3([s.buf.x[k], s.buf.y[k], s.buf.z[k]]));
    }
    assert_eq!(p.point(1000), None);
    assert_eq!(p.point(u64::MAX), None);
    let a = p.attributes();
    let (want_n, got_n) = (s.attrs.normal.as_ref().unwrap(), a.normal.as_ref().unwrap());
    for (k, (got, want)) in got_n.iter().zip(want_n).enumerate() {
        assert_eq!(bits(got), bits(want), "normal column {k} unchanged");
    }
    let ib = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    assert_eq!(ib(a.intensity.as_ref().unwrap()), ib(s.attrs.intensity.as_ref().unwrap()));
    assert_eq!(a.channel_names(), ["normal", "intensity"]);
    assert_eq!(a.normal_at(3).map(bits3), Some(bits3([want_n[0][3], want_n[1][3], want_n[2][3]])));
    assert_eq!(a.normal_at(1000), None);
    // The same input as a cloud, and through the envelope.
    let c = PointCloud::new(s.frame, s.buf.clone(), s.attrs.clone()).unwrap();
    assert_eq!(c.len(), 1000);
    let e = GeometryElement::PointSet(p.clone());
    assert_eq!(e.kind(), ElementKind::PointSet);
    assert_eq!(e.aabb(), p.aabb());
    let (frame, buf, attrs) = p.into_parts();
    assert_eq!((frame, buf, attrs), (s.frame, s.buf, s.attrs));
}

/// AC-2: point 517's y set to NaN fails with NonFinite { index: 517 }; no element exists.
#[test]
fn ac2_nan_at_517() {
    let s = small();
    let mut buf = s.buf.clone();
    buf.y[517] = f64::NAN;
    assert_eq!(PointSet::new(s.frame, buf.clone(), s.attrs.clone()), Err(GeoTypeError::NonFinite { index: 517 }));
    assert_eq!(PointCloud::new(s.frame, buf, s.attrs.clone()).err(), Some(GeoTypeError::NonFinite { index: 517 }));
    // Infinities too, and the first offending point is named.
    let mut buf = s.buf;
    buf.z[900] = f64::NEG_INFINITY;
    buf.x[42] = f64::INFINITY;
    assert_eq!(PointSet::new(s.frame, buf, AttributeSet::default()), Err(GeoTypeError::NonFinite { index: 42 }));
}

/// AC-3: 999 intensity values for 1 000 points.
#[test]
fn ac3_intensity_length() {
    let s = small();
    let mut attrs = s.attrs.clone();
    attrs.intensity.as_mut().unwrap().pop();
    let want = GeoTypeError::AttributeLength { name: "intensity".to_string(), expected: 1000, found: 999 };
    assert_eq!(PointSet::new(s.frame, s.buf.clone(), attrs.clone()), Err(want.clone()));
    assert_eq!(PointCloud::new(s.frame, s.buf, attrs).err(), Some(want));
}

/// Every channel reports its own name on a length mismatch.
#[test]
fn req3_every_channel_length_is_checked() {
    let f = FrameRef::from_bytes([1; 16]);
    let buf = PointBuffer::from_points(&[[0.0; 3], [1.0; 3]]);
    let e = |name: &str, found| GeoTypeError::AttributeLength { name: name.to_string(), expected: 2, found };
    let cases: Vec<(AttributeSet, GeoTypeError)> = vec![
        (
            AttributeSet { normal: Some([vec![1.0; 2], vec![0.0; 3], vec![0.0; 2]]), ..AttributeSet::default() },
            e("normal", 3),
        ),
        (AttributeSet { intensity: Some(vec![]), ..AttributeSet::default() }, e("intensity", 0)),
        (AttributeSet { colour: Some(vec![[1, 2, 3]]), ..AttributeSet::default() }, e("colour", 1)),
        (AttributeSet { scanner_id: Some(vec![7; 3]), ..AttributeSet::default() }, e("scanner_id", 3)),
        (AttributeSet { timestamp_ns: Some(vec![-5]), ..AttributeSet::default() }, e("timestamp_ns", 1)),
        (AttributeSet { quality: Some(vec![0; 4]), ..AttributeSet::default() }, e("quality", 4)),
    ];
    for (attrs, want) in cases {
        assert_eq!(PointSet::new(f, buf.clone(), attrs), Err(want));
    }
    // Unequal coordinate columns are reported as y / z first.
    let bad = PointBuffer::new(vec![0.0, 1.0], vec![0.0, 1.0], vec![0.0]);
    assert_eq!(
        PointCloud::new(f, bad, AttributeSet::default()),
        Err(GeoTypeError::AttributeLength { name: "z".to_string(), expected: 2, found: 1 })
    );
}

/// All channels present, of the right length: everything is kept bit-identically.
#[test]
fn all_channels_preserved() {
    let f = FrameRef::from_bytes([2; 16]);
    let buf = PointBuffer::from_points(&[[0.0, 0.0, 0.0], [-1.5, 2.0, 1e-3], [3.0, -0.0, 7.25]]);
    let attrs = AttributeSet {
        normal: Some([vec![1.0, 0.0, 0.6], vec![0.0, -1.0, 0.8], vec![0.0, 0.0, 0.0]]),
        intensity: Some(vec![0.25, f32::MAX, 0.0]),
        colour: Some(vec![[255, 0, 0], [0, 255, 0], [1, 2, 3]]),
        scanner_id: Some(vec![9, 9, 4]),
        timestamp_ns: Some(vec![i64::MIN, 0, i64::MAX]),
        quality: Some(vec![0, 0xFFFF, 0x8001]),
        scan_origins: vec![(4, [0.0, 0.0, 1.0]), (9, [-2.0, 3.0, 1.5])],
        grid: Some((3, 1)),
    };
    let p = PointSet::new(f, buf.clone(), attrs.clone()).unwrap();
    assert_eq!(p.attributes(), &attrs);
    assert_eq!(bits(p.z()), bits(&buf.z), "signed zero kept");
    assert_eq!(
        p.attributes().channel_names(),
        ["normal", "intensity", "colour", "scanner_id", "timestamp_ns", "quality"]
    );
    assert_eq!(p.attributes().scan_origin(9), Some([-2.0, 3.0, 1.5]));
    assert_eq!(p.attributes().scan_origin(5), None);
    let c = PointCloud::new(f, buf, attrs.clone()).unwrap();
    let q = c.to_point_set();
    assert_eq!(q.attributes(), &attrs, "to_point_set keeps the attributes");
    assert!(AttributeSet::default().channel_names().is_empty());
}

/// The normal cases of normal_cases.txt, with verdicts from the Python reference.
fn normal_cases() -> Vec<(String, [f64; 3], bool)> {
    lines(&read("normal_cases.txt"))
        .map(|f| {
            assert_eq!(f[0], "n");
            let accept = match f[5] {
                "1" => true,
                "0" => false,
                other => panic!("bad verdict {other}"),
            };
            (f[1].to_string(), [f64_hex(f[2]), f64_hex(f[3]), f64_hex(f[4])], accept)
        })
        .collect()
}

/// AC-4: a normal of length 1 + 1e-6 is rejected with BadNormal; 1 + 1e-10 is accepted
/// unchanged. All fixture cases follow the Python verdicts, as element and as predicate.
#[test]
fn ac4_normal_tolerance() {
    assert_eq!(NORMAL_TOLERANCE.to_bits(), 1e-9f64.to_bits());
    let cases = normal_cases();
    assert!(cases.len() >= 19);
    let labels: Vec<&str> = cases.iter().map(|c| c.0.as_str()).collect();
    for want in [
        "plus_1e-6",
        "plus_1e-10",
        "upper_last_accepted",
        "upper_first_rejected",
        "lower_last_accepted",
        "lower_first_rejected",
    ] {
        assert!(labels.contains(&want), "fixture case {want} missing");
    }
    let f = FrameRef::from_bytes([3; 16]);
    // Point 0 has a good normal, point 1 the case under test.
    let buf = PointBuffer::from_points(&[[0.0; 3], [1.0, 2.0, 3.0]]);
    for (label, n, accept) in &cases {
        assert_eq!(is_unit_normal(*n), *accept, "{label}");
        let attrs = AttributeSet {
            normal: Some([vec![0.0, n[0]], vec![1.0, n[1]], vec![0.0, n[2]]]),
            ..AttributeSet::default()
        };
        let got = PointCloud::new(f, buf.clone(), attrs);
        if *accept {
            let c = got.unwrap();
            let kept = c.attributes().normal_at(1).unwrap();
            assert_eq!(bits3(kept), bits3(*n), "{label}: accepted unchanged, never normalised");
        } else {
            assert_eq!(got.err(), Some(GeoTypeError::BadNormal { index: 1 }), "{label}");
        }
    }
    // The spec's two named values, written out.
    let one = |x: f64| AttributeSet { normal: Some([vec![x], vec![0.0], vec![0.0]]), ..AttributeSet::default() };
    let b1 = PointBuffer::from_points(&[[0.0; 3]]);
    assert_eq!(PointSet::new(f, b1.clone(), one(1.0 + 1e-6)), Err(GeoTypeError::BadNormal { index: 0 }));
    let ok = PointSet::new(f, b1, one(1.0 + 1e-10)).unwrap();
    assert_eq!(ok.attributes().normal_at(0).unwrap()[0].to_bits(), (1.0f64 + 1e-10).to_bits());
}

/// The cloud_grid_2scans fixture.
struct Grid {
    frame: FrameRef,
    rows: u64,
    cols: u64,
    origins: Vec<(u32, [f64; 3])>,
    min: [u64; 3],
    max: [u64; 3],
    buf: PointBuffer,
    attrs: AttributeSet,
}

fn grid() -> Grid {
    let text = read("cloud_grid_2scans.txt");
    let (mut frame, mut count, mut shape, mut min, mut max) = (None, None, None, None, None);
    let mut origins = Vec::new();
    let mut buf = PointBuffer::default();
    let (mut sid, mut inten, mut ts, mut q) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for f in lines(&text) {
        let three = |s: &[&str]| [f64_hex(s[0]), f64_hex(s[1]), f64_hex(s[2])];
        match f[0] {
            "frame" => frame = Some(frame_hex(f[1])),
            "count" => count = Some(f[1].parse::<usize>().unwrap()),
            "scan_grid" => shape = Some((f[1].parse::<u64>().unwrap(), f[2].parse::<u64>().unwrap())),
            "origin" => origins.push((f[1].parse::<u32>().unwrap(), three(&f[2..5]))),
            "aabb_min" => min = Some(bits3(three(&f[1..4]))),
            "aabb_max" => max = Some(bits3(three(&f[1..4]))),
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
            other => panic!("unknown line kind {other}"),
        }
    }
    let (rows, cols) = shape.unwrap();
    assert_eq!((rows, cols), (64, 48));
    assert_eq!(count, Some(buf.x.len()));
    assert_eq!(buf.x.len(), 2 * 64 * 48);
    let attrs = AttributeSet {
        intensity: Some(inten),
        scanner_id: Some(sid),
        timestamp_ns: Some(ts),
        quality: Some(q),
        scan_origins: origins.clone(),
        grid: Some((2 * rows, cols)),
        ..AttributeSet::default()
    };
    Grid { frame: frame.unwrap(), rows, cols, origins, min: min.unwrap(), max: max.unwrap(), buf, attrs }
}

/// The points of one scan (scanner `id`) as their own organised cloud.
fn one_scan(g: &Grid, id: u32, grid: (u64, u64)) -> (PointBuffer, AttributeSet) {
    let sid = g.attrs.scanner_id.as_ref().unwrap();
    let keep: Vec<usize> = (0..sid.len()).filter(|&i| sid[i] == id).collect();
    let pick = |v: &[f64]| keep.iter().map(|&i| v[i]).collect::<Vec<f64>>();
    let buf = PointBuffer::new(pick(&g.buf.x), pick(&g.buf.y), pick(&g.buf.z));
    let origin = g.origins.iter().find(|o| o.0 == id).unwrap().1;
    let attrs = AttributeSet {
        scanner_id: Some(vec![id; keep.len()]),
        scan_origins: vec![(id, origin)],
        grid: Some(grid),
        ..AttributeSet::default()
    };
    (buf, attrs)
}

/// AC-5: the two-scan cloud keeps its grid shape, scanner IDs and both origins; a
/// 64 x 47 grid declaration fails.
#[test]
fn ac5_cloud_grid_two_scans() {
    let g = grid();
    let c = PointCloud::new(g.frame, g.buf.clone(), g.attrs.clone()).unwrap();
    assert_eq!(c.len(), 6144);
    let a = c.attributes();
    assert_eq!(a.grid, Some((128, 48)));
    assert_eq!(a.scanner_id, g.attrs.scanner_id);
    let ids = a.scanner_id.as_ref().unwrap();
    assert!(ids[..3072].iter().all(|&s| s == 1) && ids[3072..].iter().all(|&s| s == 2));
    assert_eq!(a.scan_origins.len(), 2);
    for (want, got) in g.origins.iter().zip(&a.scan_origins) {
        assert_eq!((want.0, bits3(want.1)), (got.0, bits3(got.1)));
        assert_eq!(a.scan_origin(want.0).map(bits3), Some(bits3(want.1)));
    }
    assert_eq!(a.timestamp_ns, g.attrs.timestamp_ns);
    assert_eq!(a.quality, g.attrs.quality);
    assert_eq!(bits(c.x()), bits(&g.buf.x));
    // Basic statistics: the box from the Python reference.
    let b = c.aabb().unwrap();
    assert_eq!((bits3(b.min()), bits3(b.max())), (g.min, g.max));
    // Scanner structure is attributes on PointCloud, not a separate kind (REQ-006).
    assert_eq!(GeometryElement::PointCloud(c).kind(), ElementKind::PointCloud);

    // A wrong grid declaration fails on the combined cloud ...
    let mut bad = g.attrs.clone();
    bad.grid = Some((64, 47));
    assert_eq!(
        PointCloud::new(g.frame, g.buf.clone(), bad),
        Err(GeoTypeError::GridShape { rows: 64, cols: 47, count: 6144 })
    );
    // ... and each scan alone is a 64 x 48 organised cloud that rejects 64 x 47.
    for id in [1u32, 2] {
        let (buf, attrs) = one_scan(&g, id, (g.rows, g.cols));
        let one = PointCloud::new(g.frame, buf.clone(), attrs.clone()).unwrap();
        assert_eq!(one.attributes().grid, Some((64, 48)));
        assert_eq!(one.attributes().scan_origins, attrs.scan_origins);
        let (_, bad) = one_scan(&g, id, (64, 47));
        assert_eq!(
            PointCloud::new(g.frame, buf, bad),
            Err(GeoTypeError::GridShape { rows: 64, cols: 47, count: 3072 })
        );
    }
}

/// REQ-005 edge cases: overflowing and empty grids.
#[test]
fn req5_grid_shapes() {
    let f = FrameRef::from_bytes([5; 16]);
    let buf = PointBuffer::from_points(&[[0.0; 3]; 6]);
    let with = |g| AttributeSet { grid: Some(g), ..AttributeSet::default() };
    for ok in [(2, 3), (3, 2), (1, 6), (6, 1)] {
        assert!(PointCloud::new(f, buf.clone(), with(ok)).is_ok(), "{ok:?}");
    }
    for (rows, cols) in [(u64::MAX, 2), (1u64 << 32, 1u64 << 32), (0, 6), (6, 0), (4, 2)] {
        assert_eq!(
            PointCloud::new(f, buf.clone(), with((rows, cols))),
            Err(GeoTypeError::GridShape { rows, cols, count: 6 })
        );
    }
    assert!(PointCloud::new(f, PointBuffer::default(), with((0, 5))).is_ok());
}

/// Scan origins must be finite, unique and in ascending scanner-ID order.
#[test]
fn scan_origins_validated() {
    let f = FrameRef::from_bytes([6; 16]);
    let buf = PointBuffer::from_points(&[[0.0; 3]]);
    let with = |o: Vec<(u32, [f64; 3])>| AttributeSet { scan_origins: o, ..AttributeSet::default() };
    assert!(PointCloud::new(f, buf.clone(), with(vec![(0, [0.0; 3]), (u32::MAX, [1.0; 3])])).is_ok());
    let cases = [
        (vec![(2, [0.0; 3]), (1, [0.0; 3])], 1),
        (vec![(3, [0.0; 3]), (3, [1.0; 3])], 3),
        (vec![(1, [0.0; 3]), (4, [0.0, f64::NAN, 0.0])], 4),
        (vec![(8, [f64::INFINITY, 0.0, 0.0])], 8),
    ];
    for (o, id) in cases {
        assert_eq!(PointCloud::new(f, buf.clone(), with(o)), Err(GeoTypeError::ScanOrigin { scanner_id: id }));
    }
}

/// Validation order: coordinates, channel lengths, normals, scan origins, grid.
#[test]
fn validation_order_reports_first_failure() {
    let f = FrameRef::from_bytes([7; 16]);
    let mut buf = PointBuffer::from_points(&[[0.0; 3], [1.0; 3]]);
    let mut a = AttributeSet {
        normal: Some([vec![2.0, 2.0], vec![0.0; 2], vec![0.0; 2]]),
        intensity: Some(vec![1.0]),
        scan_origins: vec![(1, [f64::NAN; 3])],
        grid: Some((3, 3)),
        ..AttributeSet::default()
    };
    buf.y[1] = f64::NAN;
    assert_eq!(PointSet::new(f, buf.clone(), a.clone()), Err(GeoTypeError::NonFinite { index: 1 }));
    buf.y[1] = 1.0;
    let len_err = GeoTypeError::AttributeLength { name: "intensity".to_string(), expected: 2, found: 1 };
    assert_eq!(PointSet::new(f, buf.clone(), a.clone()), Err(len_err));
    a.intensity = None;
    assert_eq!(PointSet::new(f, buf.clone(), a.clone()), Err(GeoTypeError::BadNormal { index: 0 }));
    a.normal = None;
    assert_eq!(PointSet::new(f, buf.clone(), a.clone()), Err(GeoTypeError::ScanOrigin { scanner_id: 1 }));
    a.scan_origins.clear();
    assert_eq!(PointSet::new(f, buf.clone(), a.clone()), Err(GeoTypeError::GridShape { rows: 3, cols: 3, count: 2 }));
    a.grid = None;
    assert!(PointSet::new(f, buf, a).is_ok());
}

/// Error codes and messages of the variants this leaf adds.
#[test]
fn stable_error_codes() {
    assert_eq!(GeoTypeError::BadNormal { index: 4 }.code(), "bad_normal");
    let g = GeoTypeError::GridShape { rows: 64, cols: 47, count: 6144 };
    assert_eq!(g.code(), "grid_shape");
    assert_eq!(g.to_string(), "grid 64 x 47 does not match 6144 points");
    assert_eq!(GeoTypeError::ScanOrigin { scanner_id: 2 }.code(), "scan_origin");
}

/// AC-6: 2^31 + 1 points (about 51.5 GB of coordinates). Needs a runner with >= 64 GiB
/// of RAM [unverified runner availability]; run with `cargo test -- --ignored`.
#[test]
#[ignore = "needs >= 64 GiB RAM"]
fn ac6_more_than_2_pow_31_points() {
    let n: u64 = (1u64 << 31) + 1;
    let Ok(len) = usize::try_from(n) else {
        return; // 32-bit targets cannot address this many points.
    };
    let buf = PointBuffer::new(vec![0.0; len], vec![0.0; len], vec![0.0; len]);
    let c = PointCloud::new(FrameRef::from_bytes([8; 16]), buf, AttributeSet::default()).unwrap();
    assert_eq!(c.len(), n);
    assert_eq!(c.point(n - 1), Some([0.0; 3]));
    assert_eq!(c.point(n), None);
    let (frame, mut buf, mut attrs) = c.into_parts();
    buf.x[len - 1] = f64::NAN;
    attrs.grid = Some((n, 1));
    assert_eq!(PointCloud::new(frame, buf, attrs).err(), Some(GeoTypeError::NonFinite { index: n - 1 }));
}

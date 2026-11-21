//! Shared test helpers: fixture JSON access, cloud construction from the vectors, the
//! seeded cloud_5e6 generator, and the store used by the tests (the CORE.STO `BlobStore`
//! in a temporary directory with the default `store` feature, else `MemStore`).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_geo_ooc::{blob_address, ChunkStore, OocError};
use fid_geo_types::{AttributeSet, FrameRef, PointBuffer, PointCloud};
use fid_num::rng::{Address, Cursor};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

pub const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/geo/ooc/");

pub fn json(name: &str) -> CValue {
    let bytes = std::fs::read(format!("{FIX}{name}")).unwrap();
    fid_canon::parse_with_limits(&bytes, 64 * 1024 * 1024, 64).unwrap()
}

pub fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => m,
        other => panic!("not an object: {other:?}"),
    }
}

pub fn get<'a>(v: &'a CValue, key: &str) -> &'a CValue {
    obj(v).get(key).unwrap_or_else(|| panic!("missing key {key}"))
}

pub fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("not an array: {other:?}"),
    }
}

pub fn s(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("not a string: {other:?}"),
    }
}

pub fn int(v: &CValue) -> u64 {
    match v {
        CValue::Num(x) => {
            assert!(*x >= 0.0 && *x <= 9_007_199_254_740_992.0 && x.fract() == 0.0);
            *x as u64
        }
        CValue::Str(s) => s.parse().unwrap(),
        CValue::U64(u) => *u,
        other => panic!("not an integer: {other:?}"),
    }
}

pub fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(x) => *x,
        other => panic!("not a number: {other:?}"),
    }
}

pub fn is_null(v: &CValue) -> bool {
    matches!(v, CValue::Null)
}

pub fn f64_hex(v: &CValue) -> f64 {
    let h = s(v);
    assert_eq!(h.len(), 16);
    f64::from_bits(u64::from_str_radix(h, 16).unwrap())
}

pub fn f32_hex(v: &CValue) -> f32 {
    let h = s(v);
    assert_eq!(h.len(), 8);
    f32::from_bits(u32::from_str_radix(h, 16).unwrap())
}

pub fn bytes_hex(h: &str) -> Vec<u8> {
    assert_eq!(h.len() % 2, 0);
    (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect()
}

pub fn d32(v: &CValue) -> [u8; 32] {
    bytes_hex(s(v)).try_into().unwrap()
}

pub fn frame(v: &CValue) -> FrameRef {
    FrameRef::from_bytes(bytes_hex(s(v)).try_into().unwrap())
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn f3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    [f64_hex(&a[0]), f64_hex(&a[1]), f64_hex(&a[2])]
}

/// The cloud of a vector case: embedded points and channels, or a generator.
pub fn case_cloud(case: &CValue) -> PointCloud {
    let m = obj(case);
    if let Some(cloud) = m.get("cloud") {
        let fr = frame(get(cloud, "frame"));
        let pts: Vec<[f64; 3]> = arr(get(cloud, "points")).iter().map(f3).collect();
        let mut attrs = AttributeSet::default();
        for (name, col) in obj(get(cloud, "channels")) {
            let col = arr(col);
            match name.as_str() {
                "normal" => {
                    let n: Vec<[f64; 3]> = col.iter().map(f3).collect();
                    attrs.normal = Some([
                        n.iter().map(|v| v[0]).collect(),
                        n.iter().map(|v| v[1]).collect(),
                        n.iter().map(|v| v[2]).collect(),
                    ]);
                }
                "intensity" => attrs.intensity = Some(col.iter().map(f32_hex).collect()),
                "colour" => {
                    attrs.colour = Some(
                        col.iter()
                            .map(|c| {
                                let c = arr(c);
                                [int(&c[0]) as u8, int(&c[1]) as u8, int(&c[2]) as u8]
                            })
                            .collect(),
                    )
                }
                "quality" => attrs.quality = Some(col.iter().map(|v| int(v) as u16).collect()),
                "scanner_id" => attrs.scanner_id = Some(col.iter().map(|v| int(v) as u32).collect()),
                "timestamp_ns" => attrs.timestamp_ns = Some(col.iter().map(|v| s(v).parse::<i64>().unwrap()).collect()),
                other => panic!("unknown channel {other}"),
            }
        }
        attrs.scan_origins = arr(get(cloud, "scan_origins"))
            .iter()
            .map(|o| {
                let o = arr(o);
                (int(&o[0]) as u32, [f64_hex(&o[1]), f64_hex(&o[2]), f64_hex(&o[3])])
            })
            .collect();
        let g = get(cloud, "grid");
        if !is_null(g) {
            let g = arr(g);
            attrs.grid = Some((int(&g[0]), int(&g[1])));
        }
        return PointCloud::new(fr, PointBuffer::from_points(&pts), attrs).unwrap();
    }
    let gen = get(case, "generator");
    let fr = frame(get(case, "frame"));
    let n = int(get(gen, "n")) as usize;
    let extra: Vec<[f64; 3]> =
        arr(get(gen, "extra")).iter().map(|p| arr(p).iter().map(num).collect::<Vec<_>>().try_into().unwrap()).collect();
    let mut pts = Vec::with_capacity(n + extra.len());
    match s(get(gen, "kind")) {
        "uniform" => {
            let seed = int(get(gen, "seed"));
            let lo: Vec<f64> = arr(get(gen, "lo")).iter().map(num).collect();
            let span: Vec<f64> = arr(get(gen, "span")).iter().map(num).collect();
            let mut c = Cursor::new(Address { seed, stream: 0 }, 0);
            for _ in 0..n {
                let p: [f64; 3] = core::array::from_fn(|k| lo[k] + c.uniform_f64() * span[k]);
                pts.push(p);
            }
        }
        "duplicates" => {
            let p: Vec<f64> = arr(get(gen, "point")).iter().map(num).collect();
            pts.resize(n, [p[0], p[1], p[2]]);
        }
        other => panic!("unknown generator {other}"),
    }
    pts.extend(extra);
    PointCloud::from_points(fr, &pts).unwrap()
}

// ------------------------------------------------------------------ cloud_5e6 generator

/// One point of fixture cloud_5e6 (see `fixtures/geo/ooc/make_layout_fixtures.py`):
/// Philox seed 61 stream 0, draws 5i .. 5i+4.
pub struct ScanGen {
    facets: Vec<[[f64; 3]; 4]>,
    noise: f64,
    seed: u64,
}

impl ScanGen {
    pub fn from_fixture(e: &CValue) -> ScanGen {
        let facets = arr(get(e, "facets"))
            .iter()
            .map(|f| {
                let f = arr(f);
                let v = |i: usize| -> [f64; 3] {
                    let a = arr(&f[i]);
                    [num(&a[0]), num(&a[1]), num(&a[2])]
                };
                [v(0), v(1), v(2), v(3)]
            })
            .collect();
        ScanGen { facets, noise: num(get(e, "noise")), seed: int(get(e, "seed")) }
    }

    /// Point `i`: coordinates, normal, intensity.
    pub fn point(&self, i: u64) -> ([f64; 3], [f64; 3], f32) {
        let mut c = Cursor::new(Address { seed: self.seed, stream: 0 }, 5 * i);
        let d0 = c.next_u64();
        let [o, a, b, n] = self.facets[(d0 % 6) as usize];
        let u = c.uniform_f64();
        let v = c.uniform_f64();
        let w = c.uniform_f64();
        let noise = (w - 0.5) * self.noise;
        let p: [f64; 3] = core::array::from_fn(|k| ((o[k] + u * a[k]) + v * b[k]) + noise * n[k]);
        let d4 = c.next_u64();
        let inten = ((d4 >> 40) as f64 * (1.0 / 16_777_216.0)) as f32;
        (p, n, inten)
    }
}

// ------------------------------------------------------------------ stores and temp dirs

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh, not yet existing path under the test temp directory. Named by the clock and
/// a counter (`std::process::id` is unsupported on wasm32-wasip1).
pub fn temp_path(tag: &str) -> PathBuf {
    let k = COUNTER.fetch_add(1, Ordering::SeqCst);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("fid-geo-ooc-{tag}-{t}-{k}"))
}

/// The store the tests use: the CORE.STO blob store (feature `store`), else `MemStore`.
pub struct TestStore {
    #[cfg(feature = "store")]
    inner: fid_store::BlobStore,
    #[cfg(feature = "store")]
    dir: PathBuf,
    #[cfg(not(feature = "store"))]
    inner: fid_geo_ooc::MemStore,
}

impl TestStore {
    pub fn new(tag: &str) -> TestStore {
        #[cfg(feature = "store")]
        {
            let dir = temp_path(tag);
            std::fs::create_dir_all(&dir).unwrap();
            TestStore { inner: fid_store::BlobStore::open(&dir).unwrap(), dir }
        }
        #[cfg(not(feature = "store"))]
        {
            let _ = tag;
            TestStore { inner: fid_geo_ooc::MemStore::new() }
        }
    }

    pub fn store(&self) -> &dyn ChunkStore {
        &self.inner
    }
}

#[cfg(feature = "store")]
impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A store that keeps nothing (memory tests without the CORE.STO store).
pub struct NullStore;

impl ChunkStore for NullStore {
    fn put_blob(&self, bytes: &[u8]) -> Result<[u8; 32], OocError> {
        Ok(blob_address(bytes))
    }

    fn get_blob(&self, _: &[u8; 32], _: u64) -> Result<Vec<u8>, OocError> {
        Err(OocError::Store { code: "not_found", detail: String::new() })
    }
}

// ------------------------------------------------------------------ stream sequence digest

/// Adds the points of `b` to a GEO.OOC.STREAM sequence digest v1 (see
/// `fixtures/geo/ooc/make_stream_fixtures.py`): per point, index u64 | x y z raw f64 bits |
/// the block's channels in name order (colour 3 bytes, intensity stored f32 bits, normal
/// raw f64 x y z, quality u16, scanner_id u32, timestamp_ns i64), little-endian.
pub fn seq_update(h: &mut fid_kci::kci::Sha256, b: &fid_geo_ooc::StreamBlock) {
    let a = &b.attrs;
    for i in 0..b.len() {
        h.update(&b.original_index[i].to_le_bytes());
        for col in [&b.x, &b.y, &b.z] {
            h.update(&col[i].to_bits().to_le_bytes());
        }
        if let Some(c) = &a.colour {
            h.update(&c[i]);
        }
        if let Some(v) = &a.intensity {
            h.update(&fid_geo_ooc::intensity_bits(v[i]).to_le_bytes());
        }
        if let Some(n) = &a.normal {
            for col in n {
                h.update(&col[i].to_bits().to_le_bytes());
            }
        }
        if let Some(v) = &a.quality {
            h.update(&v[i].to_le_bytes());
        }
        if let Some(v) = &a.scanner_id {
            h.update(&v[i].to_le_bytes());
        }
        if let Some(v) = &a.timestamp_ns {
            h.update(&v[i].to_le_bytes());
        }
    }
}

/// Streams `m` with `cfg` to the end: the index sequence, the sequence digest (hex) and
/// the block lengths.
pub fn stream_all(
    m: &fid_geo_ooc::ChunkManifest,
    store: &dyn ChunkStore,
    cache: &fid_geo_ooc::ChunkCache,
    cfg: fid_geo_ooc::StreamConfig,
) -> (Vec<u64>, String, Vec<usize>) {
    let mut s =
        fid_geo_ooc::ChunkStream::new(m, store, cache, None, cfg, fid_geo_ooc::Provenance::TrustedWriter).unwrap();
    let mut h = fid_kci::kci::Sha256::new();
    let (mut idx, mut lens) = (Vec::new(), Vec::new());
    while let Some(b) = s.next_stream_block() {
        let b = b.unwrap();
        seq_update(&mut h, &b);
        idx.extend_from_slice(&b.original_index);
        lens.push(b.len());
    }
    assert!(s.next_stream_block().is_none(), "the stream stays ended");
    (idx, hex(&h.finish()), lens)
}

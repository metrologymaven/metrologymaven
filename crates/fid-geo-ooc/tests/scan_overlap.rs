//! SCAN.POLY.MULTI.OVLP AC-3: overlap detection gives identical results for chunked and
//! in-memory inputs. It lives here, not in `fid-scan-poly`, because that kernel crate may not
//! depend on GEO.OOC (layer model); this crate's tests may depend on the kernel crate.
//!
//! The scans are the SCAN.POLY.MULTI.OVLP fixtures (`fixtures/scan/poly/*.ovlp.txt`, written
//! by the independent reference `fixtures/scan/poly/make_ovlp_fixtures.py`). Each non-empty
//! scan is chunked with `to_chunks` at several chunk sizes and viewed through a test adapter
//! over the GEO.OOC `ChunkedIndex` (radius queries across chunks) and the chunk cache (point
//! attributes by original index). The validity flags of SCAN.CLOUD.NORM stay an in-memory
//! column. `to_chunks` refuses an empty cloud, so an empty scan stays in memory. Results are
//! compared with the in-memory views' result as whole values, for all-chunked and for mixed
//! inputs, and with the reference's expected per-point sets and statuses.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_index::Neighbour;
use fid_geo_ooc::{
    to_chunks, ChunkCache, ChunkManifest, ChunkedIndex, LayoutParams, MemStore, Provenance, QueryConfig, QueryError,
    DEFAULT_CACHE_BUDGET,
};
use fid_geo_types::{AttributeSet, FrameRef, PointBuffer, PointCloud};
use fid_scan_poly::{detect_overlap, MemoryScan, OverlapMap, PointStatus, PolyError, ScanPoint, ScanView};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/scan/poly/");

struct Case {
    d: f64,
    /// Per scan: (position, normal, valid, expected status, expected set).
    scans: Vec<Vec<([f64; 3], [f64; 3], bool, String, String)>>,
}

fn load(case: &str) -> Case {
    let text = std::fs::read_to_string(format!("{FIX}{case}.ovlp.txt")).unwrap();
    let mut lines = text.lines().filter(|l| !l.starts_with('#'));
    let mut next = || lines.next().unwrap().to_string();
    assert_eq!(next(), format!("case {case}"));
    let d: f64 = next().strip_prefix("d ").unwrap().parse().unwrap();
    let s: usize = next().strip_prefix("scans ").unwrap().parse().unwrap();
    let mut scans = Vec::new();
    for i in 0..s {
        let n: usize = next().strip_prefix(&format!("scan {i} points ")).unwrap().parse().unwrap();
        let mut pts = Vec::new();
        for _ in 0..n {
            let l = next();
            let t: Vec<&str> = l.split_whitespace().collect();
            let v = |k: usize| -> f64 { t[k].parse().unwrap() };
            pts.push(([v(0), v(1), v(2)], [v(3), v(4), v(5)], t[6] == "1", t[7].to_string(), t[8].to_string()));
        }
        scans.push(pts);
    }
    Case { d, scans }
}

fn frame() -> FrameRef {
    FrameRef::from_bytes([7u8; 16])
}

fn cloud(pts: &[([f64; 3], [f64; 3], bool, String, String)]) -> (PointCloud, Vec<bool>) {
    let col = |k: usize| pts.iter().map(|p| p.0[k]).collect::<Vec<f64>>();
    let ncol = |k: usize| pts.iter().map(|p| p.1[k]).collect::<Vec<f64>>();
    let attrs = AttributeSet { normal: Some([ncol(0), ncol(1), ncol(2)]), ..AttributeSet::default() };
    let c = PointCloud::new(frame(), PointBuffer::new(col(0), col(1), col(2)), attrs).unwrap();
    (c, pts.iter().map(|p| p.2).collect())
}

/// A chunked scan: radius queries through `ChunkedIndex`, attributes through the chunk cache.
struct ChunkedScan<'a> {
    manifest: &'a ChunkManifest,
    store: &'a MemStore,
    cache: &'a ChunkCache,
    index: ChunkedIndex<'a, MemStore>,
    valid: &'a [bool],
    /// Original index to (chunk position, index inside the chunk).
    locate: Vec<(usize, usize)>,
}

fn view_err(e: QueryError) -> PolyError {
    match e {
        QueryError::Index(e) => PolyError::Index(e),
        e => PolyError::View { code: e.code() },
    }
}

impl<'a> ChunkedScan<'a> {
    fn new(
        manifest: &'a ChunkManifest,
        store: &'a MemStore,
        cache: &'a ChunkCache,
        valid: &'a [bool],
        provenance: Provenance,
    ) -> ChunkedScan<'a> {
        let index = ChunkedIndex::new(manifest, store, cache, QueryConfig::default(), provenance).unwrap();
        let mut locate = vec![(usize::MAX, usize::MAX); manifest.n_points as usize];
        for pos in 0..manifest.chunks.len() {
            let h = cache.get(store, manifest, pos).unwrap();
            for (l, &i) in h.index.iter().enumerate() {
                locate[i as usize] = (pos, l);
            }
        }
        assert!(locate.iter().all(|e| e.0 != usize::MAX), "every original index is in a chunk");
        ChunkedScan { manifest, store, cache, index, valid, locate }
    }
}

impl ScanView for ChunkedScan<'_> {
    fn frame(&self) -> FrameRef {
        self.manifest.frame
    }

    fn point_count(&self) -> u64 {
        self.manifest.n_points
    }

    fn point(&self, i: u64) -> Result<ScanPoint, PolyError> {
        let &(pos, l) = self.locate.get(i as usize).ok_or(PolyError::Inconsistent)?;
        let h = self.cache.get(self.store, self.manifest, pos).map_err(|e| PolyError::View { code: e.code() })?;
        let position = h.points.point(l as u64).ok_or(PolyError::Inconsistent)?;
        let normal = if self.valid[i as usize] {
            Some(h.attrs.normal_at(l as u64).ok_or(PolyError::Inconsistent)?)
        } else {
            None
        };
        Ok(ScanPoint { position, normal })
    }

    fn radius(&self, x: [f64; 3], r: f64) -> Result<Vec<Neighbour>, PolyError> {
        self.index.radius(x, r).map_err(view_err)
    }
}

fn in_memory(clouds: &[(PointCloud, Vec<bool>)], d: f64) -> OverlapMap {
    let views: Vec<MemoryScan<'_>> = clouds.iter().map(|(c, v)| MemoryScan::new(c, v).unwrap()).collect();
    let dyns: Vec<&dyn ScanView> = views.iter().map(|v| v as &dyn ScanView).collect();
    detect_overlap(&dyns, d).unwrap()
}

fn expected_matches(case: &str, c: &Case, m: &OverlapMap) {
    for (i, scan) in c.scans.iter().enumerate() {
        for (k, p) in scan.iter().enumerate() {
            let set = m.scans[i].sets.scans_of(k).unwrap();
            let got = if set.is_empty() {
                "-".to_string()
            } else {
                set.iter().map(|j| j.to_string()).collect::<Vec<_>>().join(",")
            };
            assert_eq!(got, p.4, "{case} scan {i} point {k}");
            let status = match m.scans[i].status[k] {
                PointStatus::Overlapping => "overlapping",
                PointStatus::NothingWithinD => "nothing_within_d",
                PointStatus::NormalsIncompatible => "normals_incompatible",
            };
            assert_eq!(status, p.3, "{case} scan {i} point {k}");
        }
    }
}

#[test]
fn ac3_chunked_and_in_memory_results_are_identical() {
    for case in ["two_sheets_overlap", "thin_wall_two_scans", "normals_mix", "many_scans"] {
        let c = load(case);
        let clouds: Vec<(PointCloud, Vec<bool>)> = c.scans.iter().map(Vec::as_slice).map(cloud).collect();
        let reference = in_memory(&clouds, c.d);
        expected_matches(case, &c, &reference);

        for max in [8u64, 64, 1 << 20] {
            let stores: Vec<MemStore> = clouds.iter().map(|_| MemStore::new()).collect();
            let manifests: Vec<Option<ChunkManifest>> = clouds
                .iter()
                .zip(&stores)
                .map(|((cl, _), st)| {
                    (!cl.is_empty()).then(|| to_chunks(cl, st, &LayoutParams { max_chunk_points: max }).unwrap())
                })
                .collect();
            if max == 8 && case == "two_sheets_overlap" {
                assert!(manifests.iter().flatten().all(|m| m.chunks.len() > 16), "{case}: the scans are split");
            }
            let cache = ChunkCache::new(DEFAULT_CACHE_BUDGET);
            let chunked: Vec<Option<ChunkedScan<'_>>> = manifests
                .iter()
                .zip(&stores)
                .zip(&clouds)
                .map(|((m, st), (_, v))| {
                    m.as_ref().map(|m| ChunkedScan::new(m, st, &cache, v, Provenance::TrustedWriter))
                })
                .collect();
            let memory: Vec<MemoryScan<'_>> = clouds.iter().map(|(cl, v)| MemoryScan::new(cl, v).unwrap()).collect();

            // Every non-empty scan chunked.
            let all: Vec<&dyn ScanView> = chunked
                .iter()
                .zip(&memory)
                .map(|(ch, me)| match ch {
                    Some(ch) => ch as &dyn ScanView,
                    None => me as &dyn ScanView,
                })
                .collect();
            assert_eq!(detect_overlap(&all, c.d).unwrap(), reference, "{case}, chunks of at most {max}");

            // Mixed: every other scan chunked.
            let mixed: Vec<&dyn ScanView> = chunked
                .iter()
                .zip(&memory)
                .enumerate()
                .map(|(i, (ch, me))| match ch {
                    Some(ch) if i % 2 == 1 => ch as &dyn ScanView,
                    _ => me as &dyn ScanView,
                })
                .collect();
            assert_eq!(detect_overlap(&mixed, c.d).unwrap(), reference, "{case} mixed, chunks of at most {max}");
        }
    }
}

#[test]
fn unverified_chunked_scan_fails_closed_with_its_code() {
    let c = load("normals_mix");
    let clouds: Vec<(PointCloud, Vec<bool>)> = c.scans.iter().take(2).map(Vec::as_slice).map(cloud).collect();
    let store = MemStore::new();
    let m = to_chunks(&clouds[1].0, &store, &LayoutParams { max_chunk_points: 8 }).unwrap();
    let cache = ChunkCache::new(DEFAULT_CACHE_BUDGET);
    let un = ChunkedScan::new(&m, &store, &cache, &clouds[1].1, Provenance::Untrusted);
    let mem = MemoryScan::new(&clouds[0].0, &clouds[0].1).unwrap();
    let e = detect_overlap(&[&mem, &un], c.d).unwrap_err();
    let direct = un.index.radius([0.0; 3], c.d).unwrap_err();
    assert_eq!(e, PolyError::View { code: direct.code() });
    assert_eq!(e.code(), "scan.poly.view");
}

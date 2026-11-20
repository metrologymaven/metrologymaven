//! GEO.OOC.CACHE: hand-labelled LRU sequences, pins (RAII, also across panics and early
//! returns), integrity failures (AC-2), budget refusals (AC-3), the cost rule, and the
//! 8-thread stress (AC-4). AC-1 (allocator peak on cloud_5e6) is `cache_budget.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use fid_geo_ooc::{
    load_cost, resident_cost, to_chunks, to_chunks_in, CacheConfig, ChunkCache, ChunkManifest, ChunkStore,
    LayoutParams, MemStore, OocError, ENTRY_OVERHEAD,
};
use fid_geo_types::{Aabb, AttributeSet, FrameRef, PointBuffer, PointCloud};
use fid_kci::kci::Sha256;
use std::sync::Mutex;

// ------------------------------------------------------------------ helpers

/// `k` points around each of the 8 corners of the unit cube, cluster `c` holding corner
/// `c` (x bit 1, y bit 2, z bit 4), only the clusters in `corners`. With
/// `max_chunk_points = k` every cluster is one chunk of exactly `k` points, in Morton
/// (corner) order.
fn corner_cloud(k: usize, corners: &[u8]) -> PointCloud {
    let mut pts = Vec::new();
    let mut inten = Vec::new();
    for &c in corners {
        let base = [(c & 1) as f64, ((c >> 1) & 1) as f64, ((c >> 2) & 1) as f64];
        for i in 0..k {
            let d = 0.001 * i as f64;
            pts.push([base[0] * 0.9 + 0.05 + d * 0.1, base[1] * 0.9 + 0.05, base[2] * 0.9 + 0.05 + d * 0.01]);
            inten.push(i as f32);
        }
    }
    let attrs = AttributeSet { intensity: Some(inten), ..AttributeSet::default() };
    PointCloud::new(FrameRef::from_bytes([7; 16]), PointBuffer::from_points(&pts), attrs).unwrap()
}

/// A deterministic cloud of `n` points, different for every `seed`.
fn varied_cloud(n: usize, seed: u64) -> PointCloud {
    let mut pts = Vec::with_capacity(n);
    let mut inten = Vec::with_capacity(n);
    let mut s = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    for i in 0..n {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        let f = |b: u64| ((s >> b) & 0xffff) as f64 / 65536.0;
        pts.push([f(0) * 3.0, f(16) * 2.0 + seed as f64, f(32)]);
        inten.push((i % 1000) as f32);
    }
    let attrs = AttributeSet { intensity: Some(inten), ..AttributeSet::default() };
    PointCloud::new(FrameRef::from_bytes([seed as u8; 16]), PointBuffer::from_points(&pts), attrs).unwrap()
}

fn chunked(cloud: &PointCloud, max: u64) -> (MemStore, ChunkManifest) {
    let store = MemStore::new();
    let m = to_chunks(cloud, &store, &LayoutParams { max_chunk_points: max }).unwrap();
    (store, m)
}

/// [`corner_cloud`] chunked over the unit cube, one chunk per cluster.
fn corners(k: usize, which: &[u8]) -> (PointCloud, MemStore, ChunkManifest) {
    let cloud = corner_cloud(k, which);
    let (store, m) = corner_chunks(&cloud, k);
    (cloud, store, m)
}

fn corner_chunks(cloud: &PointCloud, k: usize) -> (MemStore, ChunkManifest) {
    let store = MemStore::new();
    let unit = Aabb::new([0.0; 3], [1.0; 3]).unwrap();
    let m = to_chunks_in(cloud, &unit, &store, &LayoutParams { max_chunk_points: k as u64 }).unwrap();
    (store, m)
}

fn rcost(m: &ChunkManifest, i: usize) -> u64 {
    resident_cost(m.channels, m.chunks[i].count).unwrap()
}

fn lcost(m: &ChunkManifest, i: usize) -> u64 {
    load_cost(m.channels, &m.chunks[i]).unwrap()
}

/// Index of the chunk whose address is `d`.
fn which(m: &ChunkManifest, d: [u8; 32]) -> usize {
    m.chunks.iter().position(|e| e.blob == d).unwrap()
}

/// SHA-256 over a chunk's indexes, coordinates and intensities.
fn hash_chunk(h: &mut Sha256, c: &fid_geo_ooc::Chunk) {
    for &i in &c.index {
        h.update(&i.to_le_bytes());
    }
    for col in [&c.points.x, &c.points.y, &c.points.z] {
        for v in col {
            h.update(&v.to_bits().to_le_bytes());
        }
    }
    if let Some(it) = &c.attrs.intensity {
        for v in it {
            h.update(&v.to_bits().to_le_bytes());
        }
    }
}

/// A store returning the stored bytes with byte `at` flipped, without any check (the
/// fault-injecting store of AC-2).
struct FlipStore {
    inner: MemStore,
    at: usize,
}

impl ChunkStore for FlipStore {
    fn put_blob(&self, bytes: &[u8]) -> Result<[u8; 32], OocError> {
        self.inner.put_blob(bytes)
    }

    fn get_blob(&self, address: &[u8; 32], size: u64) -> Result<Vec<u8>, OocError> {
        let mut b = self.inner.get_blob(address, size)?;
        let at = self.at % b.len();
        b[at] ^= 0x01;
        Ok(b)
    }
}

/// A store returning one byte too many.
struct LongStore(MemStore);

impl ChunkStore for LongStore {
    fn put_blob(&self, bytes: &[u8]) -> Result<[u8; 32], OocError> {
        self.0.put_blob(bytes)
    }

    fn get_blob(&self, address: &[u8; 32], size: u64) -> Result<Vec<u8>, OocError> {
        let mut b = self.0.get_blob(address, size)?;
        b.push(0);
        Ok(b)
    }
}

/// A `MemStore` usable from several threads.
struct SharedStore(Mutex<MemStore>);

impl ChunkStore for SharedStore {
    fn put_blob(&self, bytes: &[u8]) -> Result<[u8; 32], OocError> {
        self.0.lock().unwrap().put_blob(bytes)
    }

    fn get_blob(&self, address: &[u8; 32], size: u64) -> Result<Vec<u8>, OocError> {
        self.0.lock().unwrap().get_blob(address, size)
    }
}

// ------------------------------------------------------------------ eviction order

/// Four equal chunks (corners 0, 1, 2, 3); budget 2R + L holds two resident chunks while
/// a third loads, three once loaded. Hand-labelled from the rules:
///
/// | access | result | stamps after                | evicted |
/// |--------|--------|-----------------------------|---------|
/// | 0      | miss   | 0:1                         |         |
/// | 1      | miss   | 0:1 1:2                     |         |
/// | 2      | miss   | 0:1 1:2 2:3                 |         |
/// | 0      | hit    | 1:2 2:3 0:4                 |         |
/// | 3      | miss   | 2:3 0:4 3:5                 | 1       |
/// | 1      | miss   | 0:4 3:5 1:6                 | 2       |
/// | 3      | hit    | 0:4 1:6 3:7                 |         |
/// | 2      | miss   | 1:6 3:7 2:8                 | 0       |
#[test]
fn lru_by_access_counter_hand_labelled() {
    let (_, store, m) = corners(10, &[0, 1, 2, 3]);
    assert_eq!(m.chunks.len(), 4);
    let (r, l) = (rcost(&m, 0), lcost(&m, 0));
    for i in 1..4 {
        assert_eq!((rcost(&m, i), lcost(&m, i)), (r, l), "equal chunks");
    }
    let cache = ChunkCache::with_config(CacheConfig { budget: 2 * r + l, eviction_log: 16 });
    for i in [0, 1, 2, 0, 3, 1, 3, 2] {
        let h = cache.get(&store, &m, i).unwrap();
        assert_eq!(h.cell, m.chunks[i].cell);
        assert_eq!(h.address(), m.chunks[i].blob);
        let s = cache.stats();
        assert!(s.resident_bytes + s.in_flight_bytes <= s.budget);
    }
    let log: Vec<usize> = cache.eviction_log().iter().map(|e| which(&m, e.digest)).collect();
    assert_eq!(log, vec![1, 2, 0]);
    let s = cache.stats();
    assert_eq!((s.hits, s.misses, s.evictions, s.resident_chunks), (2, 6, 3, 3));
    assert_eq!((s.resident_bytes, s.pinned_bytes, s.in_flight_bytes), (3 * r, 0, 0));
    assert_eq!(s.peak_bytes, 2 * r + l);
    assert_eq!(cache.eviction_log()[0].cell, m.chunks[1].cell);
}

/// The same access sequence gives the same eviction sequence on a fresh cache.
#[test]
fn eviction_sequence_is_reproducible() {
    let cloud = varied_cloud(6000, 3);
    let (store, m) = chunked(&cloud, 256);
    let n = m.chunks.len();
    assert!(n > 12);
    let budget = 5 * (0..n).map(|i| rcost(&m, i)).max().unwrap();
    let run = || {
        let cache = ChunkCache::with_config(CacheConfig { budget, eviction_log: 4096 });
        for k in 0..(4 * n) {
            let i = (k * 7 + k / n) % n;
            drop(cache.get(&store, &m, i).unwrap());
        }
        (cache.eviction_log(), cache.stats())
    };
    let (a, sa) = run();
    let (b, sb) = run();
    assert!(!a.is_empty());
    assert_eq!(a, b);
    assert_eq!(sa, sb);
}

// ------------------------------------------------------------------ pins

#[test]
fn pinned_chunks_are_never_evicted_and_exhaustion_is_typed() {
    let (_, store, m) = corners(10, &[0, 1, 2, 3]);
    let (r, l) = (rcost(&m, 0), lcost(&m, 0));
    let cache = ChunkCache::with_config(CacheConfig { budget: 2 * r + l, eviction_log: 16 });
    let h0 = cache.get(&store, &m, 0).unwrap();
    let h1 = cache.get(&store, &m, 1).unwrap();
    let h2 = cache.get(&store, &m, 2).unwrap();
    assert_eq!(cache.stats().pinned_bytes, 3 * r);
    // Pins fill the budget: no eviction possible, typed error, nothing changed.
    let e = cache.get(&store, &m, 3).unwrap_err();
    assert_eq!(e, OocError::CacheExhausted { needed: l, pinned: 3 * r });
    assert_eq!(e.code(), "cache_exhausted");
    let s = cache.stats();
    assert_eq!((s.resident_chunks, s.evictions, s.in_flight_bytes), (3, 0, 0));
    // Pinning a resident chunk twice is a hit and needs no room.
    let h0b = cache.get(&store, &m, 0).unwrap();
    drop(h0b);
    // Unpin 1: the next load evicts exactly 1, never the pinned 0 and 2.
    drop(h1);
    let h3 = cache.get(&store, &m, 3).unwrap();
    let log: Vec<usize> = cache.eviction_log().iter().map(|e| which(&m, e.digest)).collect();
    assert_eq!(log, vec![1]);
    assert_eq!(h0.cell, m.chunks[0].cell);
    assert_eq!(h2.cell, m.chunks[2].cell);
    assert_eq!(h3.cell, m.chunks[3].cell);
    assert_eq!(cache.stats().pinned_chunks, 3);
    drop((h0, h2, h3));
    let s = cache.stats();
    assert_eq!((s.pinned_bytes, s.pinned_chunks, s.resident_chunks), (0, 0, 3));
}

#[test]
fn pins_are_released_on_panic_and_early_return() {
    let (cloud, store, m) = corners(10, &[0, 1, 2, 3]);
    let (r, l) = (rcost(&m, 0), lcost(&m, 0));
    // One load fits; a load next to one pinned chunk does not.
    let cache = ChunkCache::new(r + l - 1);

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let h = cache.get(&store, &m, 0).unwrap();
        assert_eq!(cache.stats().pinned_bytes, r);
        if h.len() == 10 {
            panic!("caller panics while holding a pin");
        }
    }));
    assert!(caught.is_err());
    assert_eq!(cache.stats().pinned_bytes, 0, "the panic unpinned the chunk");

    fn early(cache: &ChunkCache, store: &MemStore, m: &ChunkManifest) -> Result<(), OocError> {
        let _h = cache.get(store, m, 1)?;
        // Chunk 2 does not fit next to pinned chunk 1: `?` returns early with the pin held.
        let _h2 = cache.get(store, m, 2)?;
        Ok(())
    }
    let e = early(&cache, &store, &m).unwrap_err();
    assert!(matches!(e, OocError::CacheExhausted { pinned, .. } if pinned == r));
    let s = cache.stats();
    assert_eq!((s.pinned_bytes, s.pinned_chunks), (0, 0), "the early return unpinned the chunk");
    // And the budget is usable again.
    drop(cache.get(&store, &m, 2).unwrap());
    // The cache still works after a panic in a thread holding a handle.
    std::thread::scope(|sc| {
        let j = sc.spawn(|| {
            let _h = cache.get(&store_clone(&cloud), &m, 3).unwrap();
            panic!("thread panics while holding a pin");
        });
        assert!(j.join().is_err());
    });
    assert_eq!(cache.stats().pinned_bytes, 0);
    drop(cache.get(&store, &m, 0).unwrap());
}

/// A fresh store holding the chunks of `cloud` (per thread: `MemStore` is not `Sync`).
fn store_clone(cloud: &PointCloud) -> MemStore {
    corner_chunks(cloud, 10).0
}

// ------------------------------------------------------------------ panicking store

/// A store that signals when a read starts, waits for a go, then panics.
struct PanicStore {
    entered: std::sync::mpsc::Sender<()>,
    go: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl ChunkStore for PanicStore {
    fn put_blob(&self, bytes: &[u8]) -> Result<[u8; 32], OocError> {
        Ok(fid_geo_ooc::blob_address(bytes))
    }

    fn get_blob(&self, _: &[u8; 32], _: u64) -> Result<Vec<u8>, OocError> {
        self.entered.send(()).unwrap();
        self.go.lock().unwrap().recv().unwrap();
        panic!("store panics during a load");
    }
}

/// A store that panics in the middle of a load releases the load's reservation: the
/// in-flight bytes return to 0 and a thread waiting for room completes.
#[test]
fn panicking_store_releases_the_reservation() {
    let (cloud, store, m) = corners(10, &[0, 1]);
    let l = lcost(&m, 0).max(lcost(&m, 1));
    // One load fits; a second load next to one in flight must wait.
    let cache = ChunkCache::new(2 * l - 1);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (go_tx, go_rx) = std::sync::mpsc::channel();
    let panicking = PanicStore { entered: entered_tx, go: Mutex::new(go_rx) };
    std::thread::scope(|sc| {
        let a = sc.spawn(|| cache.get(&panicking, &m, 0).map(|_| ()));
        entered_rx.recv().unwrap();
        assert_eq!(cache.stats().in_flight_bytes, lcost(&m, 0));
        let b = sc.spawn(|| {
            let own = corner_chunks(&cloud, 10).0;
            cache.get(&own, &m, 1).map(|h| h.len())
        });
        // Give B time to block on the reservation (it completes either way).
        std::thread::sleep(std::time::Duration::from_millis(100));
        go_tx.send(()).unwrap();
        assert!(a.join().is_err(), "the store's panic reaches the caller");
        assert_eq!(b.join().unwrap().unwrap(), 10);
    });
    let s = cache.stats();
    assert_eq!((s.in_flight_bytes, s.pinned_bytes, s.resident_chunks), (0, 0, 1));
    drop(cache.get(&store, &m, 0).unwrap());
}

/// A manifest entry whose count is too low for its byte length is refused before any
/// reservation is made.
#[test]
fn entry_length_must_match_its_count() {
    let (_, store, m) = corners(10, &[0, 1]);
    let mut bad = m.clone();
    bad.chunks[1].count = 1;
    let cache = ChunkCache::new(1 << 20);
    assert!(matches!(
        cache.get(&store, &bad, 1),
        Err(OocError::CorruptChunk { fault: fid_geo_ooc::Fault::WrongSize { .. } })
    ));
    let s = cache.stats();
    assert_eq!((s.misses, s.in_flight_bytes, s.peak_bytes), (0, 0, 0));
}

// ------------------------------------------------------------------ AC-2 integrity

#[test]
fn ac2_flipped_byte_is_chunk_integrity_and_nothing_is_cached() {
    let (cloud, mem, m) = corners(10, &[0, 1, 2, 3]);
    let cache = ChunkCache::new(1 << 20);
    for at in [0usize, 7, 100, 10_000] {
        let store = FlipStore { inner: corner_chunks(&cloud, 10).0, at };
        for i in 0..m.chunks.len() {
            let e = cache.get(&store, &m, i).unwrap_err();
            assert_eq!(
                e,
                OocError::ChunkIntegrity { cloud: m.manifest_digest, cell: m.chunks[i].cell, digest: m.chunks[i].blob }
            );
            assert_eq!(e.code(), "chunk_integrity");
            assert!(e.to_string().contains(&common::hex(&m.chunks[i].blob)));
            let s = cache.stats();
            assert_eq!((s.resident_chunks, s.resident_bytes, s.in_flight_bytes, s.pinned_bytes), (0, 0, 0, 0));
        }
    }
    assert_eq!(cache.stats().integrity_failures, 16);
    // Never retried silently: the same store fails the same way again.
    let flip = FlipStore { inner: corner_chunks(&cloud, 10).0, at: 3 };
    assert!(matches!(cache.get(&flip, &m, 0), Err(OocError::ChunkIntegrity { .. })));
    // A corrupted blob in a checking store (its own integrity error) maps to the same error.
    let mut bad = mem.get_blob(&m.chunks[2].blob, m.chunks[2].bytes).unwrap();
    bad[40] ^= 0x80;
    mem.replace(m.chunks[2].blob, bad);
    let e = cache.get(&mem, &m, 2).unwrap_err();
    assert_eq!(
        e,
        OocError::ChunkIntegrity { cloud: m.manifest_digest, cell: m.chunks[2].cell, digest: m.chunks[2].blob }
    );
    // Wrong length.
    let long = LongStore(corner_chunks(&cloud, 10).0);
    assert!(matches!(cache.get(&long, &m, 1), Err(OocError::ChunkIntegrity { .. })));
    assert_eq!(cache.stats().resident_chunks, 0);
    // The intact chunks still load.
    let h = cache.get(&mem, &m, 1).unwrap();
    assert_eq!(h.cell, m.chunks[1].cell);
}

// ------------------------------------------------------------------ AC-3 budget refusal

#[test]
fn ac3_budget_smaller_than_one_chunk_is_refused() {
    let (_, store, m) = corners(10, &[0, 1]);
    let l = lcost(&m, 0);
    for budget in [0, 1, rcost(&m, 0), l - 1] {
        let cache = ChunkCache::new(budget);
        let e = cache.get(&store, &m, 0).unwrap_err();
        assert_eq!(e, OocError::CacheExhausted { needed: l, pinned: 0 }, "budget {budget}");
        let s = cache.stats();
        assert_eq!((s.misses, s.resident_chunks, s.peak_bytes), (0, 0, 0));
    }
    let cache = ChunkCache::new(l);
    let h = cache.get(&store, &m, 0).unwrap();
    assert_eq!(cache.stats().peak_bytes, l);
    drop(h);
    // The second chunk needs the first evicted.
    drop(cache.get(&store, &m, 1).unwrap());
    assert_eq!(cache.stats().evictions, 1);
}

// ------------------------------------------------------------------ cost rule, misc

#[test]
fn resident_cost_is_the_decoded_size() {
    let cloud = varied_cloud(3000, 5);
    let (store, m) = chunked(&cloud, 512);
    let cache = ChunkCache::new(u64::MAX);
    let mut total = 0;
    for i in 0..m.chunks.len() {
        let h = cache.get(&store, &m, i).unwrap();
        let cols = h.len() * m.channels.record_bytes();
        assert_eq!(rcost(&m, i), cols + ENTRY_OVERHEAD);
        // index u64 + x y z f64 + intensity f32 = 36 bytes per point
        assert_eq!(m.channels.record_bytes(), 36);
        assert_eq!(h.index.capacity() as u64, h.len());
        assert_eq!(lcost(&m, i), rcost(&m, i) + m.chunks[i].bytes);
        total += rcost(&m, i);
        assert_eq!(cache.stats().resident_bytes, total);
    }
    let s = cache.stats();
    assert_eq!((s.hits, s.misses, s.evictions), (0, m.chunks.len() as u64, 0));
}

#[test]
fn invalid_position_and_mismatching_entries_are_errors() {
    let (_, store, m) = corners(10, &[0, 1, 2]);
    let cache = ChunkCache::new(1 << 20);
    assert_eq!(cache.get(&store, &m, 3).unwrap_err(), OocError::InvalidParams { what: "chunk" });
    drop(cache.get(&store, &m, 0).unwrap());
    // A manifest naming the resident blob with another count is refused on the hit path.
    let mut bad = m.clone();
    bad.chunks[0].count += 1;
    assert!(matches!(cache.get(&store, &bad, 0), Err(OocError::CorruptChunk { .. })));
    // And on the load path.
    let mut bad2 = m.clone();
    bad2.chunks[1].cell = m.chunks[2].cell;
    assert!(matches!(cache.get(&store, &bad2, 1), Err(OocError::CorruptChunk { .. })));
    let s = cache.stats();
    assert_eq!((s.resident_chunks, s.pinned_bytes, s.in_flight_bytes), (1, 0, 0));
    // An overflowing load cost is refused, not wrapped.
    let mut huge = m.clone();
    huge.chunks[2].count = u64::MAX / 2;
    assert_eq!(cache.get(&store, &huge, 2).unwrap_err(), OocError::CacheExhausted { needed: u64::MAX, pinned: 0 });
}

#[test]
fn two_manifests_naming_the_same_blob_share_one_entry() {
    let (_, store, m) = corners(10, &[0, 1]);
    let mut other = m.clone();
    other.frame = FrameRef::from_bytes([9; 16]);
    other.seal();
    assert_ne!(other.manifest_digest, m.manifest_digest);
    let cache = ChunkCache::new(1 << 20);
    let a = cache.get(&store, &m, 0).unwrap();
    let b = cache.get(&store, &other, 0).unwrap();
    assert_eq!(a.chunk(), b.chunk());
    let s = cache.stats();
    assert_eq!((s.resident_chunks, s.hits, s.misses, s.pinned_chunks), (1, 1, 1, 1));
}

#[cfg(feature = "store")]
#[test]
fn loads_from_the_core_blob_store() {
    let cloud = varied_cloud(4000, 11);
    let ts = common::TestStore::new("cache");
    let m = to_chunks(&cloud, ts.store(), &LayoutParams { max_chunk_points: 300 }).unwrap();
    let (mem, m2) = chunked(&cloud, 300);
    assert_eq!(m, m2);
    let budget = 3 * (0..m.chunks.len()).map(|i| lcost(&m, i)).max().unwrap();
    let cache = ChunkCache::new(budget);
    for _ in 0..2 {
        for i in 0..m.chunks.len() {
            let h = cache.get(ts.store(), &m, i).unwrap();
            let want =
                fid_geo_ooc::decode_chunk(&mem.get_blob(&m.chunks[i].blob, m.chunks[i].bytes).unwrap(), u64::MAX)
                    .unwrap();
            assert_eq!(*h.chunk(), want);
        }
    }
    assert!(cache.stats().evictions > 0);
    assert!(cache.stats().peak_bytes <= budget);
}

// ------------------------------------------------------------------ AC-4 threads

/// Streams every chunk of `m` forward then in reverse, holding the previous handle while
/// getting the next (two pins at a time), and returns the stream's digest. Checks the
/// budget invariant after every get.
fn stream(cache: &ChunkCache, store: &dyn ChunkStore, m: &ChunkManifest) -> [u8; 32] {
    let n = m.chunks.len();
    let mut h = Sha256::new();
    let mut prev = None;
    for i in (0..n).chain((0..n).rev()) {
        let cur = cache.get(store, m, i).unwrap();
        let s = cache.stats();
        assert!(s.resident_bytes + s.in_flight_bytes <= s.budget, "budget invariant: {s:?}");
        assert!(s.peak_bytes <= s.budget);
        hash_chunk(&mut h, cur.chunk());
        prev = Some(cur);
    }
    drop(prev);
    h.finish()
}

#[test]
fn ac4_eight_threads_different_clouds_match_single_thread() {
    let clouds: Vec<PointCloud> = (0..8).map(|k| varied_cloud(12_000 + 500 * k as usize, 100 + k)).collect();
    let parts: Vec<(MemStore, ChunkManifest)> = clouds.iter().map(|c| chunked(c, 600)).collect();
    let r = parts.iter().flat_map(|(_, m)| (0..m.chunks.len()).map(|i| rcost(m, i))).max().unwrap();
    let total: usize = parts.iter().map(|(_, m)| m.chunks.len()).sum();
    assert!(total > 150);
    // 8 threads x 2 pinned chunks + one load (about 2R) fit in 24R; far fewer chunks than
    // all of them fit, so the threads evict each other's chunks.
    let budget = 24 * r;
    let single: Vec<[u8; 32]> = parts
        .iter()
        .map(|(s, m)| {
            let cache = ChunkCache::new(budget);
            stream(&cache, s, m)
        })
        .collect();
    let cache = ChunkCache::new(budget);
    for round in 0..3 {
        let got: Vec<[u8; 32]> = std::thread::scope(|sc| {
            let joins: Vec<_> = clouds
                .iter()
                .zip(&parts)
                .map(|(c, (_, m))| {
                    let cache = &cache;
                    let store = chunked(c, 600).0;
                    sc.spawn(move || stream(cache, &store, m))
                })
                .collect();
            joins.into_iter().map(|j| j.join().unwrap()).collect()
        });
        assert_eq!(got, single, "round {round}");
        let s = cache.stats();
        assert!(s.peak_bytes <= budget);
        assert!(s.evictions > 0);
        assert_eq!((s.pinned_bytes, s.in_flight_bytes), (0, 0));
    }
}

/// Eight threads on the same cloud through one shared store, starting at different chunks:
/// concurrent misses on the same chunk race, the loser uses the winner's entry.
#[test]
fn eight_threads_same_cloud_race_on_loads() {
    let cloud = varied_cloud(16_000, 42);
    let (mem, m) = chunked(&cloud, 500);
    let n = m.chunks.len();
    let r = (0..n).map(|i| rcost(&m, i)).max().unwrap();
    // Worst case: 8 threads x 3 pins (two held plus the one being replaced) = 24R, minus
    // the getter's own third, plus one load (< 2R): 26R never runs out.
    let budget = 26 * r;
    let reference: Vec<[u8; 32]> = {
        let cache = ChunkCache::new(budget);
        (0..n)
            .map(|i| {
                let mut h = Sha256::new();
                hash_chunk(&mut h, cache.get(&mem, &m, i).unwrap().chunk());
                h.finish()
            })
            .collect()
    };
    let store = SharedStore(Mutex::new(mem));
    let cache = ChunkCache::new(budget);
    for _ in 0..3 {
        std::thread::scope(|sc| {
            for t in 0..8usize {
                let (cache, store, m, reference) = (&cache, &store, &m, &reference);
                sc.spawn(move || {
                    let mut held = Vec::new();
                    for k in 0..(3 * n) {
                        let i = (t * n / 8 + k) % n;
                        let h = cache.get(store, m, i).unwrap();
                        let mut d = Sha256::new();
                        hash_chunk(&mut d, h.chunk());
                        assert_eq!(d.finish(), reference[i]);
                        let s = cache.stats();
                        assert!(s.resident_bytes + s.in_flight_bytes <= s.budget);
                        held.push(h);
                        if held.len() > 2 {
                            held.remove(0);
                        }
                    }
                });
            }
        });
        let s = cache.stats();
        assert!(s.peak_bytes <= budget);
        assert_eq!((s.pinned_bytes, s.in_flight_bytes, s.pinned_chunks), (0, 0, 0));
    }
}

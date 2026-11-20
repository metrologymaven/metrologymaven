//! The chunk cache (spec node `GEO.OOC.CACHE`): loads chunks of chunked clouds on demand
//! from the CORE.STO store, verifies every load against the manifest's digest, keeps the
//! decoded chunks under a byte budget with deterministic eviction, and pins chunks while a
//! [`ChunkHandle`] to them lives.
//!
//! One [`ChunkCache`] is meant to be shared by every chunked cloud of a process (it is
//! `Sync`; share it by reference or in an `Arc`). Chunks are keyed by their store address,
//! so two manifests that name the same blob share one resident copy.
//!
//! # Accounting (pinned; `fixtures/geo/ooc/make_cache_fixtures.py` models it)
//!
//! * The **resident cost** of a chunk with `count` points is
//!   `count * record_bytes + ENTRY_OVERHEAD`: its decoded columns (`record_bytes` is
//!   [`ChannelSet::record_bytes`], 8 bytes of original index, 24 of coordinates and the
//!   channel widths) plus [`ENTRY_OVERHEAD`] for the decoded struct, its reference count and
//!   its share of the cache's map nodes. If the decoded columns ever hold more capacity
//!   than that, the larger, real size is charged.
//! * A **load in flight** costs the resident cost plus the blob's byte length: the fetched
//!   bytes are held while they are verified and decoded. If the buffer the store returns
//!   has a larger capacity, the reservation grows to that capacity before decoding (or the
//!   load fails with [`OocError::CacheExhausted`]). Before anything is reserved, the entry's
//!   byte length must equal the format size for its count and channels
//!   ([`crate::chunk_bytes`]), so a manifest cannot under-reserve with a low count.
//! * The cache keeps `resident + in flight <= budget` at all times (REQ-GEO.OOC.CACHE-001).
//!   The budget covers the decoded chunk data and the fetched bytes as the store returns
//!   them. The store's own internal buffers while it reads (for `fid_store::BlobStore`,
//!   its streaming reads and scratch buffer) are **outside** the budget.
//! * A reservation is an RAII guard: if the store panics during a load, unwinding releases
//!   the reservation and wakes waiting threads.
//!
//! # Loads and eviction (D0 for a given access order)
//!
//! * Every successful [`ChunkCache::get`] takes the next value of a monotonic access
//!   counter (never the clock) and stamps its entry with it: a hit when it is found, a miss
//!   when the loaded chunk is inserted.
//! * On a miss, if the pinned bytes plus the load cost exceed the budget, the load fails
//!   with [`OocError::CacheExhausted`] (REQ-GEO.OOC.CACHE-002) and nothing is evicted.
//!   Otherwise unpinned entries are evicted, smallest `(stamp, store address)` first
//!   (REQ-GEO.OOC.CACHE-004), until `resident + in flight + load cost <= budget`. Pinned
//!   entries are never evicted. If the budget is only short because of other threads'
//!   loads in flight, the call waits for them to finish (they never wait for anything,
//!   so this cannot deadlock) and tries again.
//! * The load runs outside the lock: the blob is fetched, its length and CORE.STO.CAS
//!   address ([`crate::blob_address`]) are compared with the manifest entry, then it is
//!   decoded and checked against the entry's cell, count and the manifest's channels. A
//!   mismatching digest, or a store integrity error, is
//!   [`OocError::ChunkIntegrity`] naming the cloud, the cell and the expected digest
//!   (REQ-GEO.OOC.CACHE-003); nothing is cached and nothing is retried.
//! * If two threads load the same chunk at once, the second to finish uses the first's
//!   entry and drops its own copy; contents never depend on which thread loaded a chunk
//!   (REQ-GEO.OOC.CACHE-005).
//!
//! Which chunks are resident can depend on thread scheduling when several threads share
//! the cache, and so can `CacheExhausted` when other threads' pins fill the budget; the
//! contents of every handle never do.

use crate::cell::CellCode;
use crate::error::{chunk_fault, Fault, OocError};
use crate::format::{chunk_bytes, decode_chunk, ChannelSet, Chunk, ChunkEntry, ChunkManifest};
use crate::store::{blob_address, ChunkStore};
use core::fmt;
use core::mem::size_of;
use core::ops::Deref;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

/// Default budget, 4 GiB (REQ-GEO.OOC-001) [unverified]; to become a user setting.
pub const DEFAULT_CACHE_BUDGET: u64 = 4 << 30;

/// Bytes charged per resident chunk on top of its decoded columns: the [`Chunk`] struct in
/// its reference-counted allocation, and the entry's share of the nodes of the cache's two
/// B-trees (at worst one whole leaf node each when the entry is alone in it).
pub const ENTRY_OVERHEAD: u64 = 2048;

/// Resident cost of a chunk of `count` points with `channels`, or `None` on overflow.
pub fn resident_cost(channels: ChannelSet, count: u64) -> Option<u64> {
    count.checked_mul(channels.record_bytes())?.checked_add(ENTRY_OVERHEAD)
}

/// Cost of loading the chunk of `entry`: its resident cost plus its blob length.
pub fn load_cost(channels: ChannelSet, entry: &ChunkEntry) -> Option<u64> {
    resident_cost(channels, entry.count)?.checked_add(entry.bytes)
}

/// Bytes the decoded columns of `c` really hold (their capacities).
fn decoded_bytes(c: &Chunk) -> u64 {
    fn cap<T>(v: &Vec<T>) -> u64 {
        (v.capacity() as u64).saturating_mul(size_of::<T>() as u64)
    }
    fn cap_opt<T>(v: &Option<Vec<T>>) -> u64 {
        v.as_ref().map_or(0, cap)
    }
    let a = &c.attrs;
    let normal = a.normal.as_ref().map_or(0, |[x, y, z]| cap(x).saturating_add(cap(y)).saturating_add(cap(z)));
    [
        cap(&c.index),
        cap(&c.points.x),
        cap(&c.points.y),
        cap(&c.points.z),
        normal,
        cap_opt(&a.intensity),
        cap_opt(&a.colour),
        cap_opt(&a.scanner_id),
        cap_opt(&a.timestamp_ns),
        cap_opt(&a.quality),
        cap(&a.scan_origins),
    ]
    .iter()
    .fold(0u64, |acc, v| acc.saturating_add(*v))
}

/// One eviction: the chunk's store address and cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Eviction {
    /// Store address (manifest entry `blob`) of the evicted chunk.
    pub digest: [u8; 32],
    /// Its octree cell.
    pub cell: CellCode,
}

/// Configuration of a [`ChunkCache`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheConfig {
    /// Byte budget for resident chunks plus loads in flight.
    pub budget: u64,
    /// How many evictions to record, from the first on (0: none). The log is allocated
    /// when the cache is created and never grows.
    pub eviction_log: usize,
}

impl Default for CacheConfig {
    fn default() -> CacheConfig {
        CacheConfig { budget: DEFAULT_CACHE_BUDGET, eviction_log: 0 }
    }
}

/// A snapshot of the cache's accounting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// The configured budget.
    pub budget: u64,
    /// Bytes of resident chunks, pinned or not.
    pub resident_bytes: u64,
    /// Bytes of pinned resident chunks.
    pub pinned_bytes: u64,
    /// Bytes reserved by loads in flight.
    pub in_flight_bytes: u64,
    /// Largest `resident_bytes + in_flight_bytes` so far.
    pub peak_bytes: u64,
    /// Number of resident chunks.
    pub resident_chunks: u64,
    /// Number of chunks pinned by at least one handle.
    pub pinned_chunks: u64,
    /// Gets served from a resident chunk.
    pub hits: u64,
    /// Gets that started a load.
    pub misses: u64,
    /// Chunks evicted.
    pub evictions: u64,
    /// Loads refused with [`OocError::ChunkIntegrity`].
    pub integrity_failures: u64,
}

struct Entry {
    chunk: Arc<Chunk>,
    cost: u64,
    pins: u64,
    stamp: u64,
}

#[derive(Default)]
struct State {
    resident: BTreeMap<[u8; 32], Entry>,
    /// Unpinned resident entries by `(stamp, address)`: the eviction order.
    lru: BTreeSet<(u64, [u8; 32])>,
    counter: u64,
    stats: CacheStats,
    log: Vec<Eviction>,
}

impl State {
    fn note_peak(&mut self) {
        let now = self.stats.resident_bytes.saturating_add(self.stats.in_flight_bytes);
        if now > self.stats.peak_bytes {
            self.stats.peak_bytes = now;
        }
    }

    /// Pins the resident entry `key` (a hit) after checking it against the manifest entry.
    /// `Ok(None)` when it is not resident.
    fn pin_resident(
        &mut self,
        key: &[u8; 32],
        e: &ChunkEntry,
        channels: ChannelSet,
    ) -> Result<Option<Arc<Chunk>>, OocError> {
        let State { resident, lru, counter, stats, .. } = self;
        let Some(ent) = resident.get_mut(key) else { return Ok(None) };
        check_against_entry(&ent.chunk, e, channels)?;
        *counter = counter.saturating_add(1);
        if ent.pins == 0 {
            lru.remove(&(ent.stamp, *key));
            stats.pinned_bytes = stats.pinned_bytes.saturating_add(ent.cost);
            stats.pinned_chunks = stats.pinned_chunks.saturating_add(1);
        }
        ent.pins = ent.pins.saturating_add(1);
        ent.stamp = *counter;
        Ok(Some(Arc::clone(&ent.chunk)))
    }

    /// Evicts unpinned entries, least recently used first, until
    /// `resident + in flight + extra <= budget` or nothing unpinned is left. Whether it
    /// succeeded.
    fn evict_for(&mut self, extra: u64) -> bool {
        loop {
            let total =
                self.stats.resident_bytes.checked_add(self.stats.in_flight_bytes).and_then(|t| t.checked_add(extra));
            if total.is_some_and(|t| t <= self.stats.budget) {
                return true;
            }
            let Some((_, key)) = self.lru.pop_first() else { return false };
            if let Some(ent) = self.resident.remove(&key) {
                self.stats.resident_bytes = self.stats.resident_bytes.saturating_sub(ent.cost);
                self.stats.resident_chunks = self.stats.resident_chunks.saturating_sub(1);
                self.stats.evictions = self.stats.evictions.saturating_add(1);
                if self.log.len() < self.log.capacity() {
                    self.log.push(Eviction { digest: key, cell: ent.chunk.cell });
                }
            }
        }
    }
}

fn check_against_entry(c: &Chunk, e: &ChunkEntry, channels: ChannelSet) -> Result<(), OocError> {
    if c.cell != e.cell {
        return Err(chunk_fault(Fault::WrongCell { expected: e.cell.raw(), found: c.cell.raw() }));
    }
    if c.len() != e.count {
        return Err(chunk_fault(Fault::WrongCount { expected: e.count, found: c.len() }));
    }
    if c.channels() != channels {
        return Err(chunk_fault(Fault::WrongChannels));
    }
    Ok(())
}

/// Bytes reserved in flight by one load. Dropping it (also while unwinding from a panic in
/// the store) releases what is still reserved and wakes threads waiting for room.
struct Reservation<'a> {
    cache: &'a ChunkCache,
    bytes: u64,
}

impl Reservation<'_> {
    /// Releases the reservation under a lock the caller already holds.
    fn release(&mut self, st: &mut State) {
        st.stats.in_flight_bytes = st.stats.in_flight_bytes.saturating_sub(self.bytes);
        self.bytes = 0;
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if self.bytes > 0 {
            let mut st = self.cache.lock();
            st.stats.in_flight_bytes = st.stats.in_flight_bytes.saturating_sub(self.bytes);
            self.bytes = 0;
        }
        self.cache.loaded.notify_all();
    }
}

/// A memory-bounded cache of decoded chunks, shared by every chunked cloud of a process.
/// See the [module documentation](self) for the pinned accounting and eviction rules.
pub struct ChunkCache {
    state: Mutex<State>,
    /// Signalled whenever a load in flight finishes.
    loaded: Condvar,
}

impl fmt::Debug for ChunkCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChunkCache").field("stats", &self.stats()).finish()
    }
}

impl ChunkCache {
    /// A cache with a byte budget and no eviction log.
    pub fn new(budget: u64) -> ChunkCache {
        ChunkCache::with_config(CacheConfig { budget, eviction_log: 0 })
    }

    /// A cache with the given configuration. Allocates the eviction log up front.
    pub fn with_config(config: CacheConfig) -> ChunkCache {
        let state = State {
            stats: CacheStats { budget: config.budget, ..CacheStats::default() },
            log: Vec::with_capacity(config.eviction_log),
            ..State::default()
        };
        ChunkCache { state: Mutex::new(state), loaded: Condvar::new() }
    }

    /// The configured budget in bytes.
    pub fn budget(&self) -> u64 {
        self.lock().stats.budget
    }

    /// A snapshot of the accounting.
    pub fn stats(&self) -> CacheStats {
        self.lock().stats
    }

    /// The recorded evictions, oldest first (at most the configured log length).
    pub fn eviction_log(&self) -> Vec<Eviction> {
        self.lock().log.clone()
    }

    // Nothing panics while the lock is held, so a poisoned lock still guards a consistent
    // state; recover it rather than propagate a panic from an unrelated thread.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The chunk at position `chunk` of `manifest`, pinned until the returned handle is
    /// dropped. Served from the cache, or loaded from `store`, verified and inserted.
    ///
    /// Errors: [`OocError::InvalidParams`] (`what = "chunk"`) for a position past the
    /// manifest's chunks; [`OocError::CacheExhausted`] when the pinned chunks leave no room
    /// for the load; [`OocError::ChunkIntegrity`] when the fetched bytes do not match the
    /// manifest entry; [`OocError::CorruptChunk`] or [`OocError::UnsupportedChunkVersion`]
    /// when verified bytes do not decode to the entry's chunk; store errors as they come.
    pub fn get(
        &self,
        store: &dyn ChunkStore,
        manifest: &ChunkManifest,
        chunk: usize,
    ) -> Result<ChunkHandle<'_>, OocError> {
        let e = *manifest.chunks.get(chunk).ok_or(OocError::InvalidParams { what: "chunk" })?;
        let key = e.blob;
        let channels = manifest.channels;
        let need = load_cost(channels, &e);
        let mut st = self.lock();
        let need = loop {
            if let Some(c) = st.pin_resident(&key, &e, channels)? {
                st.stats.hits = st.stats.hits.saturating_add(1);
                return Ok(ChunkHandle { cache: self, key, chunk: c });
            }
            // The entry's length must be the format size of its count, so the reservation
            // (resident cost from the count, plus the length) covers the decoded columns.
            if let Some(size) = chunk_bytes(channels, e.count) {
                if size != e.bytes {
                    return Err(chunk_fault(Fault::WrongSize { expected: size, found: e.bytes }));
                }
            }
            let pinned = st.stats.pinned_bytes;
            let need = match need {
                Some(n) if pinned.checked_add(n).is_some_and(|t| t <= st.stats.budget) => n,
                n => return Err(OocError::CacheExhausted { needed: n.unwrap_or(u64::MAX), pinned }),
            };
            let busy = pinned.checked_add(st.stats.in_flight_bytes).and_then(|t| t.checked_add(need));
            if st.stats.in_flight_bytes > 0 && busy.map_or(true, |t| t > st.stats.budget) {
                // Only other threads' loads in flight stand in the way; they finish
                // without waiting for anything.
                st = self.loaded.wait(st).unwrap_or_else(PoisonError::into_inner);
                continue;
            }
            if !st.evict_for(need) {
                // Unreachable: pinned + need <= budget and nothing is in flight.
                return Err(OocError::CacheExhausted { needed: need, pinned });
            }
            st.stats.in_flight_bytes = st.stats.in_flight_bytes.saturating_add(need);
            st.stats.misses = st.stats.misses.saturating_add(1);
            st.note_peak();
            break need;
        };
        drop(st);
        let mut res = Reservation { cache: self, bytes: need };

        let loaded = self.load_verified(&mut res, store, manifest, &e);

        let mut st = self.lock();
        // The reservation ends here; under the same lock the loaded chunk (at most the
        // resident part of the reservation) is inserted or dropped.
        res.release(&mut st);
        let result = self.insert(&mut st, key, &e, channels, loaded);
        drop(st);
        drop(res);
        result
    }

    /// Fetches, verifies and decodes the chunk of entry `e` of manifest `m`. Grows the
    /// reservation `res` if the fetched buffer's capacity exceeds the entry's length.
    fn load_verified(
        &self,
        res: &mut Reservation<'_>,
        store: &dyn ChunkStore,
        m: &ChunkManifest,
        e: &ChunkEntry,
    ) -> Result<Chunk, OocError> {
        let integrity = || OocError::ChunkIntegrity { cloud: m.manifest_digest, cell: e.cell, digest: e.blob };
        let bytes = match store.get_blob(&e.blob, e.bytes) {
            Ok(b) => b,
            Err(OocError::Store { code: "integrity" | "manifest_invalid", .. }) => return Err(integrity()),
            Err(other) => return Err(other),
        };
        let extra = (bytes.capacity() as u64).saturating_sub(e.bytes);
        if extra > 0 {
            let mut st = self.lock();
            let pinned = st.stats.pinned_bytes;
            let total = res.bytes.saturating_add(extra);
            let fits = pinned.checked_add(total).is_some_and(|t| t <= st.stats.budget) && st.evict_for(extra);
            if !fits {
                return Err(OocError::CacheExhausted { needed: total, pinned });
            }
            st.stats.in_flight_bytes = st.stats.in_flight_bytes.saturating_add(extra);
            res.bytes = total;
            st.note_peak();
        }
        if bytes.len() as u64 != e.bytes || blob_address(&bytes) != e.blob {
            return Err(integrity());
        }
        let c = decode_chunk(&bytes, e.bytes)?;
        drop(bytes);
        check_against_entry(&c, e, m.channels)?;
        Ok(c)
    }

    /// Inserts a finished load and pins it.
    fn insert(
        &self,
        st: &mut State,
        key: [u8; 32],
        e: &ChunkEntry,
        channels: ChannelSet,
        loaded: Result<Chunk, OocError>,
    ) -> Result<ChunkHandle<'_>, OocError> {
        let chunk = match loaded {
            Ok(c) => c,
            Err(err) => {
                if matches!(err, OocError::ChunkIntegrity { .. }) {
                    st.stats.integrity_failures = st.stats.integrity_failures.saturating_add(1);
                }
                return Err(err);
            }
        };
        // Another thread may have loaded the same chunk meanwhile: use its entry.
        if let Some(c) = st.pin_resident(&key, e, channels)? {
            return Ok(ChunkHandle { cache: self, key, chunk: c });
        }
        let rule = resident_cost(channels, e.count).unwrap_or(u64::MAX);
        let real = decoded_bytes(&chunk).saturating_add(ENTRY_OVERHEAD);
        let cost = rule.max(real);
        // cost <= need, so this evicts nothing unless the decoded columns came out larger
        // than the rule.
        let pinned = st.stats.pinned_bytes;
        let fits = pinned.checked_add(cost).is_some_and(|t| t <= st.stats.budget) && st.evict_for(cost);
        if !fits {
            return Err(OocError::CacheExhausted { needed: cost, pinned });
        }
        st.counter = st.counter.saturating_add(1);
        let chunk = Arc::new(chunk);
        st.resident.insert(key, Entry { chunk: Arc::clone(&chunk), cost, pins: 1, stamp: st.counter });
        st.stats.resident_bytes = st.stats.resident_bytes.saturating_add(cost);
        st.stats.resident_chunks = st.stats.resident_chunks.saturating_add(1);
        st.stats.pinned_bytes = st.stats.pinned_bytes.saturating_add(cost);
        st.stats.pinned_chunks = st.stats.pinned_chunks.saturating_add(1);
        st.note_peak();
        Ok(ChunkHandle { cache: self, key, chunk })
    }

    fn unpin(&self, key: &[u8; 32]) {
        let mut st = self.lock();
        let State { resident, lru, stats, .. } = &mut *st;
        if let Some(ent) = resident.get_mut(key) {
            if ent.pins > 0 {
                ent.pins -= 1;
                if ent.pins == 0 {
                    stats.pinned_bytes = stats.pinned_bytes.saturating_sub(ent.cost);
                    stats.pinned_chunks = stats.pinned_chunks.saturating_sub(1);
                    lru.insert((ent.stamp, *key));
                }
            }
        }
    }
}

/// A pinned, decoded chunk. While it lives its chunk stays resident; dropping it (also
/// on an early return or a panic in the caller) unpins it.
pub struct ChunkHandle<'a> {
    cache: &'a ChunkCache,
    key: [u8; 32],
    chunk: Arc<Chunk>,
}

impl ChunkHandle<'_> {
    /// The decoded chunk.
    pub fn chunk(&self) -> &Chunk {
        &self.chunk
    }

    /// The chunk's store address (its manifest entry's `blob`).
    pub fn address(&self) -> [u8; 32] {
        self.key
    }
}

impl Deref for ChunkHandle<'_> {
    type Target = Chunk;

    fn deref(&self) -> &Chunk {
        &self.chunk
    }
}

impl fmt::Debug for ChunkHandle<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChunkHandle").field("cell", &self.chunk.cell).field("count", &self.chunk.len()).finish()
    }
}

impl Drop for ChunkHandle<'_> {
    fn drop(&mut self) {
        self.cache.unpin(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A leaf node of a `BTreeMap<K, V>` holds 11 keys and values plus a small header; an
    /// internal node adds 12 child pointers. One entry may be alone in a leaf of each tree.
    #[test]
    fn entry_overhead_covers_real_sizes() {
        let header = 64;
        let map_leaf = 11 * (size_of::<[u8; 32]>() + size_of::<Entry>()) + 12 * size_of::<usize>() + header;
        let lru_leaf = 11 * size_of::<(u64, [u8; 32])>() + 12 * size_of::<usize>() + header;
        let arc = size_of::<Chunk>() + 2 * size_of::<usize>();
        assert!(ENTRY_OVERHEAD as usize >= map_leaf + lru_leaf + arc, "{map_leaf} + {lru_leaf} + {arc}");
    }

    #[test]
    fn cache_is_send_and_sync() {
        fn is<T: Send + Sync>() {}
        is::<ChunkCache>();
        is::<ChunkHandle<'static>>();
    }
}

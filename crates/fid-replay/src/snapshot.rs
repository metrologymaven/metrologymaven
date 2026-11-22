//! Snapshot policy, validation and fallback selection (spec node `CORE.RPL.RECON.SNAP`).
//!
//! Snapshots are purely derived caches (CONSTITUTION §6): losing or corrupting every cached
//! snapshot costs time, never state. The policy uses a deterministic per-branch entry counter
//! (no wall clock), writes snapshot bytes to the cache keyed by `(entry_hash, format_version)`
//! and appends a `SnapshotTaken` row (`SnapshotRecord`) to the project's `snapshots` table.
//!
//! On selection, only snapshots whose `entry_hash` is a logical ancestor of the requested head
//! are considered, newest first. Both the content digest (SHA-256 of cached bytes) and the
//! decoded `DocState::digest()` must match the recorded row; any corrupted candidate is evicted
//! from the cache with an integrity warning logged to the Activity Log before falling back to
//! older candidates or the journal start.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fid_canon::{digest_bytes, CValue};
use fid_journal::{
    AiAssistance, CommandDraft, DraftBody, EntryDraft, EntryTime, Manifests, OutputDigest, PrincipalDraft,
    PrincipalKind,
};
use fid_types::{Determinism, Digest, SideEffect};

/// Default snapshot cadence: take a snapshot every 500 committed entries on a branch
/// (REQ-CORE.RPL.RECON.SNAP-001).
pub const SNAPSHOT_INTERVAL: u64 = 500;

/// Maximum logical ancestor depth walked during snapshot selection (cycle/DoS guard).
pub const MAX_ANCESTOR_WALK: usize = 10_000_000;

/// Activity Log command name stamped on snapshot integrity/cache warnings.
pub const ACTIVITY_WARNING_COMMAND: &str = "replay.snapshot_warning";

/// Canonical JSON body of one journal event applied to a [`DocState`].
pub type EventBody = CValue;

/// Errors returned by [`DocState::apply`] / [`DocState::decode`] and downstream fold
/// (`CORE.RPL.RECON.FOLD`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoldError {
    /// A required journal entry payload is detached/missing.
    PayloadMissing { entry_hash: Digest },
    /// Applying an event body to the document state failed.
    ApplyFailed { detail: String },
    /// Decoding snapshot bytes failed.
    DecodeFailed { detail: String },
}

impl FoldError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            FoldError::PayloadMissing { .. } => "payload_missing",
            FoldError::ApplyFailed { .. } => "apply_failed",
            FoldError::DecodeFailed { .. } => "decode_failed",
        }
    }
}

impl fmt::Display for FoldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FoldError::PayloadMissing { entry_hash } => write!(f, "payload_missing ({entry_hash})"),
            FoldError::ApplyFailed { detail } => write!(f, "apply_failed: {detail}"),
            FoldError::DecodeFailed { detail } => write!(f, "decode_failed: {detail}"),
        }
    }
}

impl std::error::Error for FoldError {}

/// Document state that can be folded from journal events and materialised into cached snapshots
/// (`CORE.RPL.RECON.SNAP` Design).
pub trait DocState: Sized {
    /// Applies one recorded event body to the state.
    fn apply(&mut self, event: &EventBody) -> Result<(), FoldError>;

    /// Deterministic SHA-256 digest of the logical document state (`sha256:` + 64 lowercase hex).
    fn digest(&self) -> Digest;

    /// Encodes this state into snapshot bytes for [`Self::format_version`].
    fn encode(&self) -> Vec<u8>;

    /// Decodes snapshot bytes previously encoded with `format`.
    fn decode(bytes: &[u8], format: u32) -> Result<Self, FoldError>;

    /// Current snapshot encoding format version.
    fn format_version() -> u32;
}

/// One `SnapshotTaken` row recorded in the project's `snapshots` table (REQ-002, REQ-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRecord {
    /// Hash of the journal entry whose post-state was snapshotted.
    pub entry_hash: Digest,
    /// Snapshot encoding format version (`DocState::format_version()`).
    pub format_version: u32,
    /// `DocState::digest()` of the snapshotted state (`sha256:` + 64 lowercase hex).
    pub state_digest: Digest,
    /// SHA-256 of the encoded snapshot bytes (`sha256:` + 64 lowercase hex).
    pub content_digest: Digest,
    /// Encoded snapshot byte length.
    pub size: u64,
}

/// Non-fatal warnings produced when taking or validating snapshots (REQ-005, REQ-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotWarning {
    /// Writing snapshot bytes to the cache (or recording the snapshot row) failed; the commit
    /// succeeded and no `SnapshotTaken` row was written (REQ-006).
    CacheWriteFailed { entry_hash: Digest, detail: String },
    /// A candidate snapshot's cached bytes were damaged or failed content/state digest checks
    /// and were evicted from the cache (REQ-005).
    SnapshotInvalid { entry_hash: Digest, format_version: u32, detail: String },
}

impl SnapshotWarning {
    /// Stable warning code (`cache_write_failed` or `snapshot_invalid`).
    pub fn code(&self) -> &'static str {
        match self {
            SnapshotWarning::CacheWriteFailed { .. } => "cache_write_failed",
            SnapshotWarning::SnapshotInvalid { .. } => "snapshot_invalid",
        }
    }

    /// The journal entry hash named by this warning.
    pub fn entry_hash(&self) -> Digest {
        match self {
            SnapshotWarning::CacheWriteFailed { entry_hash, .. }
            | SnapshotWarning::SnapshotInvalid { entry_hash, .. } => *entry_hash,
        }
    }

    /// Builds an R0 Activity Log [`EntryDraft`] recording this warning and naming `entry_hash`.
    pub fn to_activity_draft(&self, manifests: Manifests, time: EntryTime) -> EntryDraft {
        let mut args = BTreeMap::new();
        args.insert("code".to_string(), CValue::Str(self.code().to_string()));
        args.insert("entry_hash".to_string(), CValue::Str(self.entry_hash().as_str().to_string()));
        match self {
            SnapshotWarning::CacheWriteFailed { detail, .. } => {
                args.insert("detail".to_string(), CValue::Str(detail.clone()));
            }
            SnapshotWarning::SnapshotInvalid { format_version, detail, .. } => {
                args.insert("format_version".to_string(), CValue::U64(u64::from(*format_version)));
                args.insert("detail".to_string(), CValue::Str(detail.clone()));
            }
        }
        let entry_hash = self.entry_hash();
        EntryDraft {
            stream: "activity".to_string(),
            branch: "system".to_string(),
            logical_parent: None,
            time,
            principal: PrincipalDraft {
                kind: PrincipalKind::System,
                identity: CValue::Str("replay-snapshot".to_string()),
                on_behalf_of: None,
            },
            ai_assistance: AiAssistance::None,
            manifests,
            reason: Some(self.code().to_string()),
            human_assertion: None,
            body: DraftBody::Command(CommandDraft {
                id: ACTIVITY_WARNING_COMMAND.to_string(),
                major: 1,
                class: SideEffect::R0,
                determinism: Determinism::N,
                seed: 0,
                args: CValue::Obj(args),
                events: Vec::new(),
                inputs: Vec::new(),
                consumed_r1: Vec::new(),
                outputs: vec![OutputDigest {
                    entity: Some(entry_hash.as_str().to_string()),
                    digest: fid_canon::Digest(entry_hash.to_bytes()),
                }],
                algorithms: Vec::new(),
            }),
        }
    }
}

impl fmt::Display for SnapshotWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotWarning::CacheWriteFailed { entry_hash, detail } => {
                write!(f, "cache_write_failed ({entry_hash}): {detail}")
            }
            SnapshotWarning::SnapshotInvalid { entry_hash, format_version, detail } => {
                write!(f, "snapshot_invalid ({entry_hash}, format {format_version}): {detail}")
            }
        }
    }
}

/// Errors returned while reading snapshot metadata or walking the journal DAG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// Underlying database or journal read/write failure.
    Storage { detail: String },
    /// A row in the `snapshots` table held malformed data.
    InvalidRow { detail: String },
    /// The logical-parent chain contained a cycle or exceeded [`MAX_ANCESTOR_WALK`].
    CycleDetected { entry_hash: Digest },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::Storage { detail } => write!(f, "storage: {detail}"),
            SnapshotError::InvalidRow { detail } => write!(f, "invalid_row: {detail}"),
            SnapshotError::CycleDetected { entry_hash } => write!(f, "cycle_detected ({entry_hash})"),
        }
    }
}

impl std::error::Error for SnapshotError {}

/// Outcome of reading cached snapshot bytes for `(entry_hash, format_version)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheRead {
    /// Bytes were found and read from the cache.
    Hit(Vec<u8>),
    /// No cached entry exists (normal miss / prior eviction).
    Absent,
    /// The cache detected damaged bytes on disk and evicted them.
    Corrupted(String),
}

/// Cache interface storing raw snapshot bytes keyed by `(entry_hash, format_version)`.
pub trait SnapshotCache {
    /// Stores `bytes` under `(entry_hash, format_version)`. Returns `Err(detail)` if the cache
    /// cannot store the item (e.g. read-only directory or disabled cache).
    fn put(&mut self, entry_hash: Digest, format_version: u32, bytes: &[u8]) -> Result<(), String>;

    /// Reads bytes stored under `(entry_hash, format_version)`.
    fn get(&mut self, entry_hash: Digest, format_version: u32) -> CacheRead;

    /// Evicts any cached bytes for `(entry_hash, format_version)`.
    fn evict(&mut self, entry_hash: Digest, format_version: u32);
}

/// Append-only snapshot metadata table (`snapshots` in `CORE.STO.SQL.SCHEMA`).
pub trait SnapshotTable {
    /// Appends a `SnapshotTaken` row.
    fn insert(&mut self, record: &SnapshotRecord) -> Result<(), SnapshotError>;

    /// Returns all snapshot rows for `entry_hash`, newest row first.
    fn for_entry(&self, entry_hash: &Digest) -> Result<Vec<SnapshotRecord>, SnapshotError>;

    /// Returns all recorded snapshot rows in insertion order.
    fn list_all(&self) -> Result<Vec<SnapshotRecord>, SnapshotError>;
}

/// Walks the logical DAG from a head back toward genesis (REQ-004).
pub trait LogicalAncestors {
    /// Returns the logical parent of `entry_hash`, or `Ok(None)` at the journal root.
    fn logical_parent(&self, entry_hash: &Digest) -> Result<Option<Digest>, SnapshotError>;
}

/// Sink receiving integrity and cache-write warnings (wired to the Activity Log in production).
pub trait WarningSink {
    fn warn(&mut self, warning: SnapshotWarning);
}

/// Computes the canonical `sha256:` content digest of raw snapshot bytes (REQ-003).
pub fn content_digest(bytes: &[u8]) -> Digest {
    Digest::from_bytes(digest_bytes(bytes).0)
}

/// Takes a snapshot of `state` at `entry_hash`: encodes bytes, stores them in `cache` keyed by
/// `(entry_hash, S::format_version())`, and appends a [`SnapshotRecord`] row to `table`
/// (REQ-002, REQ-003, REQ-006).
///
/// Never blocks a commit: if the cache write (or table insert) fails, logs a
/// [`SnapshotWarning::CacheWriteFailed`] warning, writes no `SnapshotTaken` row, and returns
/// `None`.
pub fn take<S: DocState>(
    state: &S,
    entry_hash: Digest,
    cache: &mut dyn SnapshotCache,
    table: &mut dyn SnapshotTable,
    warnings: &mut dyn WarningSink,
) -> Option<SnapshotRecord> {
    let bytes = state.encode();
    let format_version = S::format_version();
    let c_digest = content_digest(&bytes);
    let s_digest = state.digest();
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);

    if let Err(detail) = cache.put(entry_hash, format_version, &bytes) {
        warnings.warn(SnapshotWarning::CacheWriteFailed { entry_hash, detail });
        return None;
    }

    let record = SnapshotRecord { entry_hash, format_version, state_digest: s_digest, content_digest: c_digest, size };

    if let Err(e) = table.insert(&record) {
        warnings.warn(SnapshotWarning::CacheWriteFailed {
            entry_hash,
            detail: format!("snapshot table insert failed: {e}"),
        });
        return None;
    }

    Some(record)
}

/// Selects the newest valid snapshot on `head`'s logical ancestor chain (REQ-004, REQ-005).
///
/// Walks logical ancestors starting at `head` (inclusive) back to genesis. For each recorded
/// snapshot row on that path:
/// - If absent from cache, skips silently (cache eviction is normal).
/// - If damaged in cache, fails `content_digest`/`size`, fails `DocState::decode`, or fails
///   `decoded.digest() == record.state_digest`: evicts the bad bytes from `cache`, emits
///   [`SnapshotWarning::SnapshotInvalid`] naming `record.entry_hash`, and continues to the next
///   older candidate.
/// - Returns `Ok(None)` when no valid snapshot exists (caller folds from journal start).
pub fn select_valid<S: DocState>(
    head: &Digest,
    ancestors: &dyn LogicalAncestors,
    table: &dyn SnapshotTable,
    cache: &mut dyn SnapshotCache,
    warnings: &mut dyn WarningSink,
) -> Result<Option<(SnapshotRecord, S)>, SnapshotError> {
    let mut visited: BTreeSet<Digest> = BTreeSet::new();
    let mut cursor = Some(*head);
    let mut steps = 0usize;

    while let Some(entry_hash) = cursor {
        if steps >= MAX_ANCESTOR_WALK || !visited.insert(entry_hash) {
            return Err(SnapshotError::CycleDetected { entry_hash });
        }
        steps += 1;

        let candidates = table.for_entry(&entry_hash)?;
        for record in candidates {
            match cache.get(record.entry_hash, record.format_version) {
                CacheRead::Absent => continue,
                CacheRead::Corrupted(detail) => {
                    cache.evict(record.entry_hash, record.format_version);
                    warnings.warn(SnapshotWarning::SnapshotInvalid {
                        entry_hash: record.entry_hash,
                        format_version: record.format_version,
                        detail,
                    });
                    continue;
                }
                CacheRead::Hit(bytes) => {
                    let actual_size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
                    let actual_content = content_digest(&bytes);
                    if actual_size != record.size || actual_content != record.content_digest {
                        cache.evict(record.entry_hash, record.format_version);
                        warnings.warn(SnapshotWarning::SnapshotInvalid {
                            entry_hash: record.entry_hash,
                            format_version: record.format_version,
                            detail: format!(
                                "content digest/size mismatch: expected {} ({} B), got {} ({} B)",
                                record.content_digest, record.size, actual_content, actual_size
                            ),
                        });
                        continue;
                    }
                    let decoded = match S::decode(&bytes, record.format_version) {
                        Ok(s) => s,
                        Err(e) => {
                            cache.evict(record.entry_hash, record.format_version);
                            warnings.warn(SnapshotWarning::SnapshotInvalid {
                                entry_hash: record.entry_hash,
                                format_version: record.format_version,
                                detail: format!("decode failed: {e}"),
                            });
                            continue;
                        }
                    };
                    let actual_state = decoded.digest();
                    if actual_state != record.state_digest {
                        cache.evict(record.entry_hash, record.format_version);
                        warnings.warn(SnapshotWarning::SnapshotInvalid {
                            entry_hash: record.entry_hash,
                            format_version: record.format_version,
                            detail: format!(
                                "state digest mismatch: expected {}, got {}",
                                record.state_digest, actual_state
                            ),
                        });
                        continue;
                    }
                    return Ok(Some((record, decoded)));
                }
            }
        }

        cursor = ancestors.logical_parent(&entry_hash)?;
    }

    Ok(None)
}

/// Deterministic per-branch snapshot policy (REQ-001): snapshots every `interval` committed
/// entries on a branch (default [`SNAPSHOT_INTERVAL`] = 500) and on project close whenever
/// entries were committed on that branch since its last snapshot. Uses no wall clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPolicy {
    interval: u64,
    since_last: BTreeMap<String, u64>,
}

impl Default for SnapshotPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl SnapshotPolicy {
    /// Creates a policy with the pinned cadence of [`SNAPSHOT_INTERVAL`] (500) entries.
    pub fn new() -> Self {
        Self { interval: SNAPSHOT_INTERVAL, since_last: BTreeMap::new() }
    }

    /// Creates a policy with a custom cadence (clamped to at least 1).
    pub fn with_interval(interval: u64) -> Self {
        Self { interval: interval.max(1), since_last: BTreeMap::new() }
    }

    /// Configured cadence in committed entries per branch.
    pub fn interval(&self) -> u64 {
        self.interval
    }

    /// Committed entries on `branch` since its last snapshot.
    pub fn since_last(&self, branch: &str) -> u64 {
        self.since_last.get(branch).copied().unwrap_or(0)
    }

    /// Records one committed Record Journal entry on `branch` and returns `true` when the
    /// branch has reached the snapshot interval.
    pub fn record_commit(&mut self, branch: &str) -> bool {
        let count = self.since_last.entry(branch.to_string()).or_insert(0);
        *count = count.saturating_add(1);
        *count >= self.interval
    }

    /// Resets `branch`'s counter after a snapshot is taken.
    pub fn mark_snapshot_taken(&mut self, branch: &str) {
        self.since_last.insert(branch.to_string(), 0);
    }

    /// Whether closing the project should take a snapshot for `branch` (entries committed since
    /// the last snapshot).
    pub fn should_snapshot_on_close(&self, branch: &str) -> bool {
        self.since_last(branch) > 0
    }

    /// Records a commit on `branch` and, if the interval is reached, takes a snapshot via
    /// [`take`]. Always resets the counter when a snapshot attempt fires at the interval so a
    /// failing cache never blocks commits or retries on every subsequent entry (REQ-001, -006).
    pub fn on_commit<S: DocState>(
        &mut self,
        branch: &str,
        state: &S,
        entry_hash: Digest,
        cache: &mut dyn SnapshotCache,
        table: &mut dyn SnapshotTable,
        warnings: &mut dyn WarningSink,
    ) -> Option<SnapshotRecord> {
        if !self.record_commit(branch) {
            return None;
        }
        self.mark_snapshot_taken(branch);
        take(state, entry_hash, cache, table, warnings)
    }

    /// Project close hook: if entries have been committed on `branch` since the last snapshot,
    /// takes a final snapshot at `head` (REQ-001).
    pub fn on_close<S: DocState>(
        &mut self,
        branch: &str,
        state: &S,
        head: Digest,
        cache: &mut dyn SnapshotCache,
        table: &mut dyn SnapshotTable,
        warnings: &mut dyn WarningSink,
    ) -> Option<SnapshotRecord> {
        if !self.should_snapshot_on_close(branch) {
            return None;
        }
        self.mark_snapshot_taken(branch);
        take(state, head, cache, table, warnings)
    }
}

// ------------------------------------------------------------------ pure in-memory adapters

/// In-memory [`SnapshotCache`] (available on all targets including `wasm32`).
#[derive(Debug, Default, Clone)]
pub struct MemSnapshotCache {
    items: BTreeMap<(Digest, u32), Vec<u8>>,
    read_only: bool,
}

impl MemSnapshotCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggles read-only mode so `put` fails with a simulated I/O error (AC-4).
    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    /// Direct mutable access to stored bytes (for bit-flip corruption tests, AC-2).
    pub fn get_raw_mut(&mut self, entry_hash: Digest, format_version: u32) -> Option<&mut Vec<u8>> {
        self.items.get_mut(&(entry_hash, format_version))
    }

    /// True when cached bytes exist for `(entry_hash, format_version)`.
    pub fn contains(&self, entry_hash: Digest, format_version: u32) -> bool {
        self.items.contains_key(&(entry_hash, format_version))
    }

    /// Number of stored snapshot blobs.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl SnapshotCache for MemSnapshotCache {
    fn put(&mut self, entry_hash: Digest, format_version: u32, bytes: &[u8]) -> Result<(), String> {
        if self.read_only {
            return Err("cache is read-only".to_string());
        }
        self.items.insert((entry_hash, format_version), bytes.to_vec());
        Ok(())
    }

    fn get(&mut self, entry_hash: Digest, format_version: u32) -> CacheRead {
        match self.items.get(&(entry_hash, format_version)) {
            Some(b) => CacheRead::Hit(b.clone()),
            None => CacheRead::Absent,
        }
    }

    fn evict(&mut self, entry_hash: Digest, format_version: u32) {
        self.items.remove(&(entry_hash, format_version));
    }
}

/// In-memory append-only [`SnapshotTable`].
#[derive(Debug, Default, Clone)]
pub struct MemSnapshotTable {
    rows: Vec<SnapshotRecord>,
}

impl MemSnapshotTable {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SnapshotTable for MemSnapshotTable {
    fn insert(&mut self, record: &SnapshotRecord) -> Result<(), SnapshotError> {
        self.rows.push(record.clone());
        Ok(())
    }

    fn for_entry(&self, entry_hash: &Digest) -> Result<Vec<SnapshotRecord>, SnapshotError> {
        Ok(self.rows.iter().rev().filter(|r| &r.entry_hash == entry_hash).cloned().collect())
    }

    fn list_all(&self) -> Result<Vec<SnapshotRecord>, SnapshotError> {
        Ok(self.rows.clone())
    }
}

/// In-memory [`LogicalAncestors`] backed by a deterministic `BTreeMap`.
#[derive(Debug, Default, Clone)]
pub struct MemLogicalAncestors {
    parents: BTreeMap<Digest, Option<Digest>>,
}

impl MemLogicalAncestors {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, entry_hash: Digest, logical_parent: Option<Digest>) {
        self.parents.insert(entry_hash, logical_parent);
    }
}

impl LogicalAncestors for MemLogicalAncestors {
    fn logical_parent(&self, entry_hash: &Digest) -> Result<Option<Digest>, SnapshotError> {
        Ok(self.parents.get(entry_hash).copied().flatten())
    }
}

/// In-memory [`WarningSink`] collecting emitted [`SnapshotWarning`]s.
#[derive(Debug, Default, Clone)]
pub struct VecWarningSink {
    pub warnings: Vec<SnapshotWarning>,
}

impl VecWarningSink {
    pub fn new() -> Self {
        Self::default()
    }
}

impl WarningSink for VecWarningSink {
    fn warn(&mut self, warning: SnapshotWarning) {
        self.warnings.push(warning);
    }
}

// ------------------------------------------------------------------ fid-store & ActivityLog adapters

#[cfg(feature = "store")]
pub mod store_adapter {
    //! Adapters wiring [`super::SnapshotCache`], [`super::SnapshotTable`],
    //! [`super::LogicalAncestors`] and [`super::WarningSink`] to `fid-store` (`Cache` and SQLite
    //! `Store`) and `fid-journal`'s `ActivityLog`.

    use super::*;
    use fid_canon::parse;
    use fid_journal::stream::ActivityLog;
    use fid_store::{Cache, CacheKey, CacheWarning, Store, Value};

    /// Wraps a [`fid_store::Cache`] as a [`SnapshotCache`].
    pub struct StoreSnapshotCache<'a> {
        cache: &'a mut Cache,
    }

    impl<'a> StoreSnapshotCache<'a> {
        pub fn new(cache: &'a mut Cache) -> Self {
            Self { cache }
        }

        pub fn inner_mut(&mut self) -> &mut Cache {
            self.cache
        }
    }

    impl SnapshotCache for StoreSnapshotCache<'_> {
        fn put(&mut self, entry_hash: Digest, format_version: u32, bytes: &[u8]) -> Result<(), String> {
            let _ = self.cache.take_warnings();
            let key = CacheKey::Snapshot { entry_hash, format: format_version };
            if self.cache.put(&key, bytes) {
                Ok(())
            } else {
                let ws = self.cache.take_warnings();
                if ws.is_empty() {
                    Err("cache disabled or refused snapshot write".to_string())
                } else {
                    let joined: Vec<String> = ws.iter().map(ToString::to_string).collect();
                    Err(joined.join("; "))
                }
            }
        }

        fn get(&mut self, entry_hash: Digest, format_version: u32) -> CacheRead {
            let _ = self.cache.take_warnings();
            let key = CacheKey::Snapshot { entry_hash, format: format_version };
            if let Some(bytes) = self.cache.get(&key) {
                return CacheRead::Hit(bytes);
            }
            let ws = self.cache.take_warnings();
            for w in &ws {
                if matches!(w, CacheWarning::Integrity(_) | CacheWarning::Io(_)) {
                    return CacheRead::Corrupted(w.to_string());
                }
            }
            CacheRead::Absent
        }

        fn evict(&mut self, entry_hash: Digest, format_version: u32) {
            let key = CacheKey::Snapshot { entry_hash, format: format_version };
            self.cache.remove(&key);
        }
    }

    /// Wraps a [`fid_store::Store`] for reading and appending `snapshots` rows (`v1.sql`).
    pub struct StoreSnapshotTable<'a> {
        store: &'a Store,
    }

    impl<'a> StoreSnapshotTable<'a> {
        pub fn new(store: &'a Store) -> Self {
            Self { store }
        }
    }

    fn parse_record(
        entry_hash: String,
        format_version: i64,
        state_digest: String,
        content_digest: String,
        size: i64,
    ) -> Result<SnapshotRecord, SnapshotError> {
        let entry_hash = Digest::parse(&entry_hash)
            .map_err(|_| SnapshotError::InvalidRow { detail: format!("bad entry_hash: {entry_hash}") })?;
        let state_digest = Digest::parse(&state_digest)
            .map_err(|_| SnapshotError::InvalidRow { detail: format!("bad state_digest: {state_digest}") })?;
        let content_digest = Digest::parse(&content_digest)
            .map_err(|_| SnapshotError::InvalidRow { detail: format!("bad content_digest: {content_digest}") })?;
        let format_version = u32::try_from(format_version)
            .map_err(|_| SnapshotError::InvalidRow { detail: format!("bad format_version: {format_version}") })?;
        let size =
            u64::try_from(size).map_err(|_| SnapshotError::InvalidRow { detail: format!("bad size: {size}") })?;
        Ok(SnapshotRecord { entry_hash, format_version, state_digest, content_digest, size })
    }

    impl SnapshotTable for StoreSnapshotTable<'_> {
        fn insert(&mut self, record: &SnapshotRecord) -> Result<(), SnapshotError> {
            let size_i64 = i64::try_from(record.size).map_err(|_| SnapshotError::InvalidRow {
                detail: format!("snapshot size overflows i64: {}", record.size),
            })?;
            let mut txn = self.store.transaction();
            txn.insert(
                "snapshots",
                &[
                    ("entry_hash", Value::Text(record.entry_hash.as_str().to_string())),
                    ("format_version", Value::Integer(i64::from(record.format_version))),
                    ("state_digest", Value::Text(record.state_digest.as_str().to_string())),
                    ("content_digest", Value::Text(record.content_digest.as_str().to_string())),
                    ("size", Value::Integer(size_i64)),
                ],
            );
            txn.commit().map_err(|e| SnapshotError::Storage { detail: e.to_string() })?;
            Ok(())
        }

        fn for_entry(&self, entry_hash: &Digest) -> Result<Vec<SnapshotRecord>, SnapshotError> {
            let key = entry_hash.as_str().to_string();
            let raw = self
                .store
                .read(|conn| {
                    let mut stmt = conn.prepare(
                        "SELECT entry_hash, format_version, state_digest, content_digest, size \
                         FROM snapshots WHERE entry_hash = ?1 ORDER BY id DESC",
                    )?;
                    let rows = stmt.query_map([&key], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, i64>(4)?,
                        ))
                    })?;
                    let mut out = Vec::new();
                    for row in rows {
                        out.push(row?);
                    }
                    Ok(out)
                })
                .map_err(|e| SnapshotError::Storage { detail: e.to_string() })?;
            raw.into_iter().map(|(e, f, s, c, sz)| parse_record(e, f, s, c, sz)).collect()
        }

        fn list_all(&self) -> Result<Vec<SnapshotRecord>, SnapshotError> {
            let raw = self
                .store
                .read(|conn| {
                    let mut stmt = conn.prepare(
                        "SELECT entry_hash, format_version, state_digest, content_digest, size \
                         FROM snapshots ORDER BY id ASC",
                    )?;
                    let rows = stmt.query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, i64>(4)?,
                        ))
                    })?;
                    let mut out = Vec::new();
                    for row in rows {
                        out.push(row?);
                    }
                    Ok(out)
                })
                .map_err(|e| SnapshotError::Storage { detail: e.to_string() })?;
            raw.into_iter().map(|(e, f, s, c, sz)| parse_record(e, f, s, c, sz)).collect()
        }
    }

    /// Walks logical ancestors from `entries.header` JSON in a [`fid_store::Store`].
    pub struct StoreLogicalAncestors<'a> {
        store: &'a Store,
    }

    impl<'a> StoreLogicalAncestors<'a> {
        pub fn new(store: &'a Store) -> Self {
            Self { store }
        }
    }

    impl LogicalAncestors for StoreLogicalAncestors<'_> {
        fn logical_parent(&self, entry_hash: &Digest) -> Result<Option<Digest>, SnapshotError> {
            let key = entry_hash.as_str().to_string();
            let header_opt: Option<Vec<u8>> = self
                .store
                .read(|conn| {
                    let mut stmt = conn.prepare("SELECT header FROM entries WHERE hash = ?1")?;
                    let mut rows = stmt.query([&key])?;
                    match rows.next()? {
                        Some(row) => Ok(Some(row.get::<_, Vec<u8>>(0)?)),
                        None => Ok(None),
                    }
                })
                .map_err(|e| SnapshotError::Storage { detail: e.to_string() })?;
            let Some(header_bytes) = header_opt else {
                return Ok(None);
            };
            let val = parse(&header_bytes)
                .map_err(|e| SnapshotError::InvalidRow { detail: format!("entry {entry_hash} header json: {e}") })?;
            let CValue::Obj(map) = val else {
                return Err(SnapshotError::InvalidRow { detail: format!("entry {entry_hash} header not an object") });
            };
            match map.get("logical_parent") {
                None | Some(CValue::Null) => Ok(None),
                Some(CValue::Str(s)) => {
                    let d = Digest::parse(s).map_err(|_| SnapshotError::InvalidRow {
                        detail: format!("entry {entry_hash} bad logical_parent {s}"),
                    })?;
                    Ok(Some(d))
                }
                Some(_) => Err(SnapshotError::InvalidRow {
                    detail: format!("entry {entry_hash} logical_parent not a string or null"),
                }),
            }
        }
    }

    /// [`WarningSink`] that appends every [`SnapshotWarning`] to a [`fid_journal::stream::ActivityLog`]
    /// as an R0 `system` record naming `entry_hash` (REQ-005, REQ-006, AC-2, AC-4).
    pub struct ActivityWarningSink<'a> {
        log: &'a ActivityLog,
        manifests: Manifests,
        utc: String,
        mono_ns: u64,
        recorded: Vec<SnapshotWarning>,
    }

    impl<'a> ActivityWarningSink<'a> {
        pub fn new(log: &'a ActivityLog, manifests: Manifests, utc: impl Into<String>) -> Self {
            Self { log, manifests, utc: utc.into(), mono_ns: 0, recorded: Vec::new() }
        }

        /// Warnings emitted through this sink so far.
        pub fn warnings(&self) -> &[SnapshotWarning] {
            &self.recorded
        }
    }

    impl WarningSink for ActivityWarningSink<'_> {
        fn warn(&mut self, warning: SnapshotWarning) {
            self.mono_ns = self.mono_ns.saturating_add(1);
            let time = EntryTime { utc: self.utc.clone(), mono_ns: self.mono_ns, sync: "unknown".to_string() };
            let draft = warning.to_activity_draft(self.manifests, time);
            let _ = self.log.append(&draft);
            self.recorded.push(warning);
        }
    }
}

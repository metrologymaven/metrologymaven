//! Reconstruction, head moves, fold invariant and crash resume (spec node `CORE.RPL.RECON.FOLD`).
//!
//! Rebuilds document state at any committed head by selecting the newest valid logical-ancestor
//! snapshot via [`crate::snapshot::select_valid`] (or starting from `S::default()`), walking the
//! logical-parent path forward to `head`, verifying each entry's `events` commitment against its
//! durable payload, and applying the recorded event bodies in order (no command re-execution).
//!
//! Crash resume follows from durable-only journal reads: uncommitted writes are never visible, so
//! reopening after a crash folds every commit that returned success and nothing else.
//!
//! Determinism D0: ordered maps only, deterministic counter-based close sampler (no randomness or
//! wall clock).

use std::collections::{BTreeMap, BTreeSet};

use fid_canon::{digest_bytes, parse_with_limits, CValue, MAX_DEPTH};
use fid_journal::{
    commitment, AiAssistance, CommandDraft, DraftBody, EntryDraft, EntryTime, Manifests, OutputDigest, PrincipalDraft,
    PrincipalKind, Salt,
};
use fid_types::{Determinism, Digest, SideEffect};

use crate::snapshot::{
    select_valid, DocState, EventBody, FoldError, LogicalAncestors, SnapshotCache, SnapshotError, SnapshotTable,
    WarningSink, MAX_ANCESTOR_WALK,
};

/// Maximum accepted size of one entry header JSON blob, in bytes.
pub const MAX_HEADER_BYTES: usize = 256 * 1024;
/// Maximum accepted size of one entry payload JSON blob, in bytes.
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
/// Maximum close-check interval (at least 1 close in 20, REQ-CORE.RPL.RECON.FOLD-006).
pub const CLOSE_CHECK_INTERVAL: u64 = 20;
/// Activity Log command name stamped on fold-invariant mismatch incidents (REQ-005).
pub const ACTIVITY_INCIDENT_COMMAND: &str = "replay.fold_mismatch";

/// One durable journal entry as read by [`reconstruct`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableEntry {
    /// Committed entry hash (`sha256:` + 64 lowercase hex).
    pub entry_hash: Digest,
    /// Optional raw canonical header bytes; when `Some`, verified to hash to `entry_hash`.
    pub header_bytes: Option<Vec<u8>>,
    /// Logical parent hash in the project DAG (`None` at root).
    pub logical_parent: Option<Digest>,
    /// Salted commitment to `events` from the verified header (`None` when the entry carries no events).
    pub events_commitment: Option<Digest>,
    /// Raw payload JSON bytes (`{"salts":{"events":"..."},"events":[...]}`), or `None` if detached.
    pub payload_bytes: Option<Vec<u8>>,
}

/// Durable read-only source of committed journal entries for [`reconstruct`] (REQ-001, REQ-002).
pub trait JournalReader: LogicalAncestors {
    /// Returns the durable entry for `entry_hash`, or `Ok(None)` if not present / not durable.
    fn read_entry(&self, entry_hash: &Digest) -> Result<Option<DurableEntry>, FoldError>;
}

/// Result of [`reconstruct`] at a target head (Design: `Reconstructed<S>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reconstructed<S: DocState> {
    pub state: S,
    pub head: Digest,
    pub digest: Digest,
    pub from_snapshot: Option<Digest>,
    pub applied_entries: u64,
    pub applied_events: u64,
}

/// Injected reconstruction interface for project open, undo, redo and branch switch (Design).
pub trait Reconstructor<S: DocState> {
    fn reconstruct(&mut self, head: Digest) -> Result<Reconstructed<S>, FoldError>;
}

/// Holds the active in-memory document state and moves its head atomically: if reconstruction
/// fails (for example [`FoldError::PayloadMissing`]), the previous live state stays intact
/// (REQ-CORE.RPL.RECON.FOLD-003, REQ-004, AC-4).
#[derive(Debug, Clone)]
pub struct LiveState<S: DocState> {
    state: S,
    head: Option<Digest>,
    digest: Digest,
}

impl<S: DocState> LiveState<S> {
    pub fn new(state: S, head: Option<Digest>) -> Self {
        let digest = state.digest();
        Self { state, head, digest }
    }

    pub fn state(&self) -> &S {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    pub fn head(&self) -> Option<Digest> {
        self.head
    }

    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// Moves the head (open, undo, redo, or branch switch) by reconstructing `new_head`.
    /// On any error, `self` is left completely unchanged (REQ-004).
    pub fn move_head<R: Reconstructor<S>>(&mut self, new_head: Digest, recon: &mut R) -> Result<Digest, FoldError> {
        let rebuilt = recon.reconstruct(new_head)?;
        self.state = rebuilt.state;
        self.head = Some(rebuilt.head);
        self.digest = rebuilt.digest;
        Ok(self.digest)
    }
}

/// Details of a fold-invariant mismatch (REQ-CORE.RPL.RECON.FOLD-005, AC-5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldInvariantMismatch {
    pub head: Digest,
    pub live_digest: Digest,
    pub folded_digest: Digest,
}

impl FoldInvariantMismatch {
    /// Builds an R0 Activity Log [`EntryDraft`] recording this integrity incident naming `head`.
    pub fn to_activity_draft(&self, manifests: Manifests, time: EntryTime) -> EntryDraft {
        let mut args = BTreeMap::new();
        args.insert("code".to_string(), CValue::Str("fold_invariant_mismatch".to_string()));
        args.insert("folded_digest".to_string(), CValue::Str(self.folded_digest.as_str().to_string()));
        args.insert("head".to_string(), CValue::Str(self.head.as_str().to_string()));
        args.insert("live_digest".to_string(), CValue::Str(self.live_digest.as_str().to_string()));
        EntryDraft {
            stream: "activity".to_string(),
            branch: "system".to_string(),
            logical_parent: None,
            time,
            principal: PrincipalDraft {
                kind: PrincipalKind::System,
                identity: CValue::Str("fid-replay".to_string()),
                on_behalf_of: None,
            },
            ai_assistance: AiAssistance::None,
            manifests,
            reason: Some("fold_invariant_mismatch".to_string()),
            human_assertion: None,
            body: DraftBody::Command(CommandDraft {
                id: ACTIVITY_INCIDENT_COMMAND.to_string(),
                major: 1,
                class: SideEffect::R0,
                determinism: Determinism::D0,
                seed: 0,
                args: CValue::Obj(args),
                events: Vec::new(),
                inputs: Vec::new(),
                consumed_r1: Vec::new(),
                outputs: vec![OutputDigest {
                    entity: Some(self.head.as_str().to_string()),
                    digest: fid_canon::Digest(self.head.to_bytes()),
                }],
                algorithms: Vec::new(),
            }),
        }
    }
}

/// Outcome of [`check_fold_invariant`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoldInvariantOutcome {
    Match { head: Digest, digest: Digest },
    Mismatch(FoldInvariantMismatch),
}

/// Sink for integrity incidents logged when [`check_fold_invariant`] detects a mismatch (REQ-005).
pub trait IncidentSink {
    fn record_mismatch(&mut self, incident: &FoldInvariantMismatch);
}

/// In-memory [`IncidentSink`] collecting mismatch incidents.
#[derive(Debug, Default, Clone)]
pub struct VecIncidentSink {
    pub incidents: Vec<FoldInvariantMismatch>,
}

impl VecIncidentSink {
    pub fn new() -> Self {
        Self::default()
    }
}

impl IncidentSink for VecIncidentSink {
    fn record_mismatch(&mut self, incident: &FoldInvariantMismatch) {
        self.incidents.push(incident.clone());
    }
}

/// Deterministic counter-based close-time fold-invariant sampler (REQ-CORE.RPL.RECON.FOLD-006).
///
/// Runs on every `interval`-th project close (clamped to `1..=CLOSE_CHECK_INTERVAL`, so at least
/// 1 close in 20 is always verified). Uses no randomness and no clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseSampler {
    closes: u64,
    interval: u64,
}

impl Default for CloseSampler {
    fn default() -> Self {
        Self::new(CLOSE_CHECK_INTERVAL)
    }
}

impl CloseSampler {
    /// Creates a sampler checking once every `interval` closes, clamped to `1..=20`.
    pub fn new(interval: u64) -> Self {
        Self { closes: 0, interval: interval.clamp(1, CLOSE_CHECK_INTERVAL) }
    }

    pub fn closes(&self) -> u64 {
        self.closes
    }

    pub fn interval(&self) -> u64 {
        self.interval
    }

    /// Advances the close counter and returns `true` when this close must run the fold check.
    pub fn should_check_next(&mut self) -> bool {
        self.closes = self.closes.saturating_add(1);
        self.closes % self.interval == 0
    }

    /// Called on project close: runs [`check_fold_invariant`] whenever the deterministic counter
    /// hits the sample interval, returning `Ok(Some(outcome))` when checked or `Ok(None)` when
    /// skipped.
    #[allow(clippy::too_many_arguments)]
    pub fn on_close<S, J, T, C, W, I>(
        &mut self,
        live: &S,
        head: Digest,
        journal: &J,
        snapshots: &T,
        cache: &mut C,
        warnings: &mut W,
        incidents: &mut I,
    ) -> Result<Option<FoldInvariantOutcome>, FoldError>
    where
        S: DocState + Default,
        J: JournalReader,
        T: SnapshotTable,
        C: SnapshotCache,
        W: WarningSink,
        I: IncidentSink,
    {
        if !self.should_check_next() {
            return Ok(None);
        }
        let outcome = check_fold_invariant(live, head, journal, snapshots, cache, warnings, incidents)?;
        Ok(Some(outcome))
    }
}

/// Walks logical ancestors from `head` back to `stop_at` (exclusive) and returns the path in
/// forward logical order (oldest to newest, ending with `head`).
fn collect_forward_path<A: LogicalAncestors>(
    head: Digest,
    stop_at: Option<Digest>,
    ancestors: &A,
) -> Result<Vec<Digest>, FoldError> {
    if stop_at == Some(head) {
        return Ok(Vec::new());
    }
    let mut rev_path = Vec::new();
    let mut seen = BTreeSet::new();
    let mut cur = Some(head);
    while let Some(h) = cur {
        if stop_at == Some(h) {
            break;
        }
        if rev_path.len() >= MAX_ANCESTOR_WALK {
            return Err(FoldError::DecodeFailed {
                detail: format!("logical ancestor walk exceeded {MAX_ANCESTOR_WALK} entries"),
            });
        }
        if !seen.insert(h) {
            return Err(FoldError::DecodeFailed { detail: format!("cycle detected at entry {h}") });
        }
        rev_path.push(h);
        cur = ancestors.logical_parent(&h).map_err(snapshot_err_to_fold)?;
    }
    if let Some(target) = stop_at {
        if cur != Some(target) {
            return Err(FoldError::DecodeFailed {
                detail: format!("selected snapshot {target} is not on the logical path of {head}"),
            });
        }
    }
    rev_path.reverse();
    Ok(rev_path)
}

fn snapshot_err_to_fold(e: SnapshotError) -> FoldError {
    FoldError::DecodeFailed { detail: e.to_string() }
}

/// Verifies `entry`'s header hash (if header bytes present) and `events` salted commitment against
/// its payload, returning the ordered list of verified [`EventBody`] items to apply.
fn verified_events(entry: &DurableEntry) -> Result<Vec<EventBody>, FoldError> {
    if let Some(header_bytes) = &entry.header_bytes {
        if header_bytes.len() > MAX_HEADER_BYTES {
            return Err(FoldError::DecodeFailed {
                detail: format!("entry {} header exceeds {MAX_HEADER_BYTES} bytes", entry.entry_hash),
            });
        }
        let val = parse_with_limits(header_bytes, MAX_HEADER_BYTES, MAX_DEPTH)
            .map_err(|e| FoldError::DecodeFailed { detail: format!("entry {} header json: {e}", entry.entry_hash) })?;
        let has_embedded_hash = matches!(&val, CValue::Obj(m) if m.contains_key("hash"));
        if has_embedded_hash {
            let verified = fid_journal::verify_header_value(&val).map_err(|e| FoldError::DecodeFailed {
                detail: format!("entry {} header verification failed: {e}", entry.entry_hash),
            })?;
            if verified.0 != entry.entry_hash.to_bytes() {
                return Err(FoldError::DecodeFailed {
                    detail: format!("entry {} header hash mismatch (got {verified})", entry.entry_hash),
                });
            }
        } else {
            let actual = digest_bytes(header_bytes);
            if actual.0 != entry.entry_hash.to_bytes() {
                return Err(FoldError::DecodeFailed {
                    detail: format!("entry {} header hash mismatch (got {actual})", entry.entry_hash),
                });
            }
        }
    }

    let Some(expected_commit) = entry.events_commitment else {
        return Ok(Vec::new());
    };

    let Some(payload_bytes) = &entry.payload_bytes else {
        return Err(FoldError::PayloadMissing { entry_hash: entry.entry_hash });
    };
    if payload_bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(FoldError::DecodeFailed {
            detail: format!("entry {} payload exceeds {MAX_PAYLOAD_BYTES} bytes", entry.entry_hash),
        });
    }

    let payload_val = parse_with_limits(payload_bytes, MAX_PAYLOAD_BYTES, MAX_DEPTH)
        .map_err(|e| FoldError::DecodeFailed { detail: format!("entry {} payload json: {e}", entry.entry_hash) })?;
    let CValue::Obj(obj) = payload_val else {
        return Err(FoldError::DecodeFailed {
            detail: format!("entry {} payload must be a JSON object", entry.entry_hash),
        });
    };

    let salt_hex = match obj.get("salts") {
        Some(CValue::Obj(salts)) => match salts.get("events") {
            Some(CValue::Str(s)) => s.as_str(),
            None => return Err(FoldError::PayloadMissing { entry_hash: entry.entry_hash }),
            Some(_) => {
                return Err(FoldError::DecodeFailed {
                    detail: format!("entry {} salts.events must be a hex string", entry.entry_hash),
                });
            }
        },
        None => return Err(FoldError::PayloadMissing { entry_hash: entry.entry_hash }),
        Some(_) => {
            return Err(FoldError::DecodeFailed {
                detail: format!("entry {} payload salts must be an object", entry.entry_hash),
            });
        }
    };

    let Some(events_val) = obj.get("events") else {
        return Err(FoldError::PayloadMissing { entry_hash: entry.entry_hash });
    };

    let salt = Salt::from_hex(salt_hex)
        .map_err(|e| FoldError::DecodeFailed { detail: format!("entry {} bad events salt: {e}", entry.entry_hash) })?;
    let computed = commitment(&salt, events_val).map_err(|e| FoldError::DecodeFailed {
        detail: format!("entry {} commitment compute: {e}", entry.entry_hash),
    })?;
    if computed.0 != expected_commit.to_bytes() {
        return Err(FoldError::DecodeFailed {
            detail: format!(
                "entry {} events commitment mismatch: header={} computed={computed}",
                entry.entry_hash, expected_commit
            ),
        });
    }

    let CValue::Arr(events) = events_val else {
        return Err(FoldError::DecodeFailed {
            detail: format!("entry {} events field must be a JSON array", entry.entry_hash),
        });
    };
    Ok(events.clone())
}

/// Reconstructs document state at `head` by selecting the newest valid logical-ancestor snapshot
/// via [`select_valid`] (or starting from `S::default()`) and applying every later durable entry's
/// verified event bodies along the logical path up to and including `head`
/// (REQ-CORE.RPL.RECON.FOLD-001..004).
pub fn reconstruct<S, J, T, C, W>(
    head: Digest,
    journal: &J,
    snapshots: &T,
    cache: &mut C,
    warnings: &mut W,
) -> Result<Reconstructed<S>, FoldError>
where
    S: DocState + Default,
    J: JournalReader,
    T: SnapshotTable,
    C: SnapshotCache,
    W: WarningSink,
{
    let (mut state, from_snapshot) =
        match select_valid::<S>(&head, journal, snapshots, cache, warnings).map_err(snapshot_err_to_fold)? {
            Some((snap_rec, snap_state)) => (snap_state, Some(snap_rec.entry_hash)),
            None => (S::default(), None),
        };

    let forward_path = collect_forward_path(head, from_snapshot, journal)?;
    let mut applied_entries: u64 = 0;
    let mut applied_events: u64 = 0;

    for entry_hash in forward_path {
        let Some(entry) = journal.read_entry(&entry_hash)? else {
            return Err(FoldError::PayloadMissing { entry_hash });
        };
        if entry.entry_hash != entry_hash {
            return Err(FoldError::DecodeFailed {
                detail: format!("journal returned entry {} when asked for {entry_hash}", entry.entry_hash),
            });
        }
        let events = verified_events(&entry)?;
        for ev in &events {
            state.apply(ev)?;
            applied_events = applied_events.saturating_add(1);
        }
        applied_entries = applied_entries.saturating_add(1);
    }

    let digest = state.digest();
    Ok(Reconstructed { state, head, digest, from_snapshot, applied_entries, applied_events })
}

/// Compares `live.digest()` against a fresh reconstruction at `head`; on mismatch, records an
/// integrity incident naming `head` to `incidents` (REQ-CORE.RPL.RECON.FOLD-005, AC-5).
pub fn check_fold_invariant<S, J, T, C, W, I>(
    live: &S,
    head: Digest,
    journal: &J,
    snapshots: &T,
    cache: &mut C,
    warnings: &mut W,
    incidents: &mut I,
) -> Result<FoldInvariantOutcome, FoldError>
where
    S: DocState + Default,
    J: JournalReader,
    T: SnapshotTable,
    C: SnapshotCache,
    W: WarningSink,
    I: IncidentSink,
{
    let live_digest = live.digest();
    let folded = reconstruct::<S, _, _, _, _>(head, journal, snapshots, cache, warnings)?;
    if live_digest == folded.digest {
        Ok(FoldInvariantOutcome::Match { head, digest: live_digest })
    } else {
        let mismatch = FoldInvariantMismatch { head, live_digest, folded_digest: folded.digest };
        incidents.record_mismatch(&mismatch);
        Ok(FoldInvariantOutcome::Mismatch(mismatch))
    }
}

/// Concrete [`Reconstructor`] binding a journal, snapshot table, cache, and warning sink.
pub struct JournalReconstructor<'a, J, T, C, W> {
    pub journal: &'a J,
    pub snapshots: &'a T,
    pub cache: &'a mut C,
    pub warnings: &'a mut W,
}

impl<S, J, T, C, W> Reconstructor<S> for JournalReconstructor<'_, J, T, C, W>
where
    S: DocState + Default,
    J: JournalReader,
    T: SnapshotTable,
    C: SnapshotCache,
    W: WarningSink,
{
    fn reconstruct(&mut self, head: Digest) -> Result<Reconstructed<S>, FoldError> {
        reconstruct::<S, _, _, _, _>(head, self.journal, self.snapshots, self.cache, self.warnings)
    }
}

// ------------------------------------------------------------------ in-memory journal reader (WASM-safe)

/// Deterministic in-memory [`JournalReader`] backed by a `BTreeMap`.
#[derive(Debug, Default, Clone)]
pub struct MemJournalReader {
    entries: BTreeMap<Digest, DurableEntry>,
}

impl MemJournalReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, entry: DurableEntry) {
        self.entries.insert(entry.entry_hash, entry);
    }

    pub fn detach_payload(&mut self, entry_hash: &Digest) -> bool {
        if let Some(e) = self.entries.get_mut(entry_hash) {
            e.payload_bytes = None;
            true
        } else {
            false
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl LogicalAncestors for MemJournalReader {
    fn logical_parent(&self, entry_hash: &Digest) -> Result<Option<Digest>, SnapshotError> {
        Ok(self.entries.get(entry_hash).and_then(|e| e.logical_parent))
    }
}

impl JournalReader for MemJournalReader {
    fn read_entry(&self, entry_hash: &Digest) -> Result<Option<DurableEntry>, FoldError> {
        Ok(self.entries.get(entry_hash).cloned())
    }
}

// ------------------------------------------------------------------ fid-store & ActivityLog adapters

#[cfg(feature = "store")]
pub mod store_adapter {
    //! Adapters wiring [`super::JournalReader`] and [`super::IncidentSink`] to `fid-store`
    //! (`Store` + `BlobStore`) and `fid-journal`'s `ActivityLog`.

    use super::*;
    use fid_journal::stream::ActivityLog;
    use fid_store::{BlobStore, CasError, Store};

    /// Role name used by `fid-journal::append` in `entry_blobs` for detachable payloads.
    pub const ROLE_PAYLOAD: &str = "payload";

    /// Reads durable committed entries from a [`fid_store::Store`] and their detachable payloads
    /// from a [`fid_store::BlobStore`] (REQ-001, REQ-002, REQ-004).
    pub struct StoreJournalReader<'a> {
        store: &'a Store,
        blobs: &'a BlobStore,
    }

    impl<'a> StoreJournalReader<'a> {
        pub fn new(store: &'a Store, blobs: &'a BlobStore) -> Self {
            Self { store, blobs }
        }
    }

    fn parse_header_fields(
        entry_hash: Digest,
        header_bytes: &[u8],
    ) -> Result<(Option<Digest>, Option<Digest>), FoldError> {
        if header_bytes.len() > MAX_HEADER_BYTES {
            return Err(FoldError::DecodeFailed {
                detail: format!("entry {entry_hash} header exceeds {MAX_HEADER_BYTES} bytes"),
            });
        }
        let val = parse_with_limits(header_bytes, MAX_HEADER_BYTES, MAX_DEPTH)
            .map_err(|e| FoldError::DecodeFailed { detail: format!("entry {entry_hash} header json: {e}") })?;
        let CValue::Obj(map) = val else {
            return Err(FoldError::DecodeFailed { detail: format!("entry {entry_hash} header is not an object") });
        };

        let logical_parent = match map.get("logical_parent") {
            None | Some(CValue::Null) => None,
            Some(CValue::Str(s)) => {
                let d = Digest::parse(s).map_err(|_| FoldError::DecodeFailed {
                    detail: format!("entry {entry_hash} invalid logical_parent {s:?}"),
                })?;
                Some(d)
            }
            Some(_) => {
                return Err(FoldError::DecodeFailed {
                    detail: format!("entry {entry_hash} logical_parent must be string or null"),
                });
            }
        };

        let raw_events_commit = map
            .get("command")
            .and_then(|c| match c {
                CValue::Obj(co) => co.get("events_commit"),
                _ => None,
            })
            .or_else(|| map.get("events_commit"));

        let events_commitment = match raw_events_commit {
            None | Some(CValue::Null) => None,
            Some(CValue::Str(s)) => {
                let d = Digest::parse(s).map_err(|_| FoldError::DecodeFailed {
                    detail: format!("entry {entry_hash} invalid events_commit {s:?}"),
                })?;
                Some(d)
            }
            Some(_) => {
                return Err(FoldError::DecodeFailed {
                    detail: format!("entry {entry_hash} events_commit must be string or null"),
                });
            }
        };

        Ok((logical_parent, events_commitment))
    }

    impl LogicalAncestors for StoreJournalReader<'_> {
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
            let (parent, _) = parse_header_fields(*entry_hash, &header_bytes)
                .map_err(|e| SnapshotError::InvalidRow { detail: e.to_string() })?;
            Ok(parent)
        }
    }

    impl JournalReader for StoreJournalReader<'_> {
        fn read_entry(&self, entry_hash: &Digest) -> Result<Option<DurableEntry>, FoldError> {
            let key = entry_hash.as_str().to_string();
            let row_opt: Option<(Vec<u8>, Option<String>)> = self
                .store
                .read(|conn| {
                    let mut stmt = conn.prepare(
                        "SELECT e.header, b.digest \
                         FROM entries e \
                         LEFT JOIN entry_blobs b ON b.entry_hash = e.hash AND b.role = ?2 \
                         WHERE e.hash = ?1",
                    )?;
                    let mut rows = stmt.query([key.as_str(), ROLE_PAYLOAD])?;
                    match rows.next()? {
                        Some(row) => Ok(Some((row.get::<_, Vec<u8>>(0)?, row.get::<_, Option<String>>(1)?))),
                        None => Ok(None),
                    }
                })
                .map_err(|e| FoldError::DecodeFailed { detail: format!("store read {entry_hash}: {e}") })?;

            let Some((header_bytes, payload_digest_str)) = row_opt else {
                return Ok(None);
            };

            let (logical_parent, events_commitment) = parse_header_fields(*entry_hash, &header_bytes)?;
            let payload_bytes = match payload_digest_str {
                None => None,
                Some(ref raw_d) => {
                    let pd = Digest::parse(raw_d).map_err(|_| FoldError::DecodeFailed {
                        detail: format!("entry {entry_hash} bad payload digest {raw_d:?}"),
                    })?;
                    match self.blobs.read_object(&pd, MAX_PAYLOAD_BYTES as u64) {
                        Ok(bytes) => Some(bytes),
                        Err(CasError::NotFound(_)) => None,
                        Err(e) => {
                            return Err(FoldError::DecodeFailed {
                                detail: format!("entry {entry_hash} payload blob read: {e}"),
                            });
                        }
                    }
                }
            };

            Ok(Some(DurableEntry {
                entry_hash: *entry_hash,
                header_bytes: Some(header_bytes),
                logical_parent,
                events_commitment,
                payload_bytes,
            }))
        }
    }

    /// [`IncidentSink`] that appends every [`FoldInvariantMismatch`] to an [`ActivityLog`] as an
    /// R0 `system` integrity incident naming `head` (REQ-005, AC-5).
    pub struct ActivityIncidentSink<'a> {
        log: &'a ActivityLog,
        manifests: Manifests,
        utc: String,
        mono_ns: u64,
        recorded: Vec<FoldInvariantMismatch>,
    }

    impl<'a> ActivityIncidentSink<'a> {
        pub fn new(log: &'a ActivityLog, manifests: Manifests, utc: impl Into<String>) -> Self {
            Self { log, manifests, utc: utc.into(), mono_ns: 0, recorded: Vec::new() }
        }

        pub fn incidents(&self) -> &[FoldInvariantMismatch] {
            &self.recorded
        }
    }

    impl IncidentSink for ActivityIncidentSink<'_> {
        fn record_mismatch(&mut self, incident: &FoldInvariantMismatch) {
            self.mono_ns = self.mono_ns.saturating_add(1);
            let time = EntryTime { utc: self.utc.clone(), mono_ns: self.mono_ns, sync: "unknown".to_string() };
            let draft = incident.to_activity_draft(self.manifests, time);
            let _ = self.log.append(&draft);
            self.recorded.push(incident.clone());
        }
    }
}

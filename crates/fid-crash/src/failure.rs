//! What a failed check reports (REQ-CORE.STO.SQL.CRASH-003, -005).

use crate::plan::KillInstant;
use fid_bus::err::{BusError, Detail, InternalReason};
use std::fmt;

/// Kinds of failure. The first four are the spec's `CrashFailure` fields; `StoreRefused`
/// covers a store that cannot be opened or read back at all (a damaged database file, a
/// failed integrity check, a store over the check's size limits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// A row is malformed, not canonical, out of sequence, not linked to its predecessor, or
    /// not the row the chain defines at its position; or a companion row disagrees with it.
    ChainBroken,
    /// A sequence number the writer acknowledged is not in the reopened store.
    AckMissing,
    /// A row references a blob that is not stored.
    BlobMissing,
    /// A row references a blob whose stored bytes do not match its digest.
    BlobCorrupt,
    /// The store could not be opened or checked.
    StoreRefused,
}

impl FailureKind {
    /// Stable snake_case code.
    pub fn code(self) -> &'static str {
        match self {
            FailureKind::ChainBroken => "chain_broken",
            FailureKind::AckMissing => "ack_missing",
            FailureKind::BlobMissing => "blob_missing",
            FailureKind::BlobCorrupt => "blob_corrupt",
            FailureKind::StoreRefused => "store_refused",
        }
    }
}

/// A failed check, with the seed and kill instant that produced it when it came from a
/// harness run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashFailure {
    pub kind: FailureKind,
    pub detail: String,
    pub seed: Option<u64>,
    pub instant: Option<KillInstant>,
}

impl CrashFailure {
    pub fn new(kind: FailureKind, detail: impl Into<String>) -> CrashFailure {
        CrashFailure { kind, detail: detail.into(), seed: None, instant: None }
    }

    /// Adds the seed and instant of the kill after which the check failed.
    pub fn at(mut self, seed: u64, instant: KillInstant) -> CrashFailure {
        self.seed = Some(seed);
        self.instant = Some(instant);
        self
    }

    /// The failure as a bus error of the published catalogue (v3): a damaged or tampered
    /// store is `internal/storage`; an acknowledged commit that is gone is
    /// `internal/invariant_violated` (the store broke its own durability promise). Never a
    /// success value: a refused store has no "ok" form.
    pub fn to_bus_error(&self) -> BusError {
        let reason = match self.kind {
            FailureKind::AckMissing => InternalReason::InvariantViolated,
            _ => InternalReason::Storage,
        };
        BusError::Internal(reason, Detail::new(&self.to_string()))
    }
}

impl fmt::Display for CrashFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.code(), self.detail)?;
        if let (Some(seed), Some(instant)) = (self.seed, self.instant) {
            write!(f, " (seed {seed}, instant {instant})")?;
        }
        Ok(())
    }
}

impl std::error::Error for CrashFailure {}

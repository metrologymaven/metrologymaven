//! The derived head cache and the `checkpoints` table (feature `store`).
//!
//! # Pinned rules
//!
//! - **Branch head.** The head of a branch is the entry with the highest sequence whose
//!   `branch` is that branch (whatever its kind, `head_move` included; what a `head_move`
//!   means for the logical state belongs to `CORE.JRN.UNDO`). A branch with no entry has no
//!   head.
//! - **Derived cache (REQ-004).** The cache is the `heads` table of the project database
//!   (`CORE.STO.SQL.SCHEMA`), one `(branch, entry_hash)` row per branch. It can always be
//!   rebuilt from `entries` ([`Journal::rebuild_heads`]), and the journal always wins: every
//!   answer of [`Journal::head`] and [`Journal::heads`] is read from `entries` in the same
//!   read transaction as the cache row, and a missing row or one naming another entry
//!   triggers a rebuild of the whole cache before the journal's answer is returned. Schema v1
//!   has no `digest` column on `heads`, so the "stored digest" of the spec is this
//!   comparison of the cached entry hash with the journal's, which is at least as strong.
//!   `fid-store` offers no delete, so a stale row for a branch that has no entry cannot be
//!   removed; it is never answered (the journal says there is no head).
//! - **Rebuild.** One read of `entries` (the last entry of every branch, through the
//!   `entries_by_branch` index), then one transaction upserting every branch's row, both
//!   under the journal's store lock so no append interleaves. At most [`MAX_BRANCHES`]
//!   branches (`input_limit` beyond).
//! - **Checkpoint (REQ-001, REQ-002, REQ-005).** Under the store lock: the verified chain
//!   tail ([`crate::Journal::tail`]'s checks); `empty_stream` if there is none; the count of
//!   the stream's entries, which must equal the tail's sequence (`chain_invalid`
//!   otherwise); the time from the journal's clock ([`super::format_time`]); then one
//!   transaction inserting `(stream, entry_hash, body)` into `checkpoints`, where `body` is
//!   the canonical bytes. The ledger head is the caller's (`CORE.JRN.STREAM.LEDGER`), checked
//!   only for form. A failed COMMIT is `append_failed` and does not poison the journal: a
//!   checkpoint row is not part of the chain.
//! - **Poison.** While the journal is poisoned (an append COMMIT with unknown outcome), the
//!   head, checkpoint and verify calls refuse, like `tail`.

use super::{format_time, verify, Checkpoint, CheckpointError, Coverage, Format, LedgerHead, Link, Reason, Verified};
use crate::append::{failed, guard, read_tail, Core, Journal, JournalError};
use crate::entry::parse_digest;
use fid_canon::Digest;
use fid_store::Value;

/// Most branches the head cache holds.
pub const MAX_BRANCHES: usize = 65_536;

/// The head of one branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchHead {
    pub branch: String,
    pub seq: u64,
    pub hash: Digest,
}

/// What a rebuild of the head cache wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadsRebuilt {
    /// Branches written.
    pub branches: usize,
}

/// A checkpoint as stored in the `checkpoints` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCheckpoint {
    /// Row id (increasing in insertion order).
    pub id: i64,
    pub checkpoint: Checkpoint,
    /// SHA-256 of the stored body.
    pub digest: Digest,
}

/// One branch's journal head and its cached entry hash (if any).
type HeadRow = (String, i64, String, Option<String>);

/// The last entry of every branch, with the cache row joined.
const HEADS_SQL: &str = "SELECT e.branch, e.seq, e.hash, h.entry_hash FROM entries e \
     JOIN (SELECT max(seq) AS m FROM entries GROUP BY branch) t ON e.seq = t.m \
     LEFT JOIN heads h ON h.branch = e.branch ORDER BY e.branch LIMIT ?1";

fn bad_row(what: &str) -> JournalError {
    JournalError::ChainInvalid(format!("entries: {what}"))
}

fn branch_head(branch: String, seq: i64, hash: &str) -> Result<BranchHead, JournalError> {
    let seq = u64::try_from(seq).map_err(|_| bad_row("negative sequence"))?;
    let hash = parse_digest(hash).ok_or_else(|| bad_row("hash column is not a digest"))?;
    Ok(BranchHead { branch, seq, hash })
}

impl Core<'_> {
    /// The head of `branch` (module documentation): from the journal, after checking the
    /// cache row and rebuilding the cache if the row is missing or names another entry.
    /// `None` if the branch has no entry.
    pub(crate) fn head(&self, branch: &str) -> Result<Option<BranchHead>, JournalError> {
        self.check_poison()?;
        let (journal, cached) = self
            .store
            .read(|conn| {
                let mut st =
                    conn.prepare("SELECT seq, hash FROM entries WHERE branch = ?1 ORDER BY seq DESC LIMIT 1")?;
                let mut rows = st.query_map([branch], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
                let journal = rows.next().transpose()?;
                let mut st = conn.prepare("SELECT entry_hash FROM heads WHERE branch = ?1")?;
                let mut rows = st.query_map([branch], |r| r.get::<_, String>(0))?;
                let cached = rows.next().transpose()?;
                Ok((journal, cached))
            })
            .map_err(|e| failed("read head", e))?;
        let Some((seq, hash)) = journal else { return Ok(None) };
        if cached.as_deref() != Some(hash.as_str()) {
            self.rebuild_heads()?;
        }
        branch_head(branch.to_string(), seq, &hash).map(Some)
    }

    /// The head of every branch, in branch order, checked against the cache as
    /// `head` does (one rebuild if any row is missing or stale).
    pub(crate) fn heads(&self) -> Result<Vec<BranchHead>, JournalError> {
        self.check_poison()?;
        let rows = self.read_heads()?;
        let stale = rows.iter().any(|(_, _, hash, cached)| cached.as_deref() != Some(hash.as_str()));
        if stale {
            self.rebuild_heads()?;
        }
        rows.into_iter().map(|(branch, seq, hash, _)| branch_head(branch, seq, &hash)).collect()
    }

    fn read_heads(&self) -> Result<Vec<HeadRow>, JournalError> {
        let limit = i64::try_from(MAX_BRANCHES).unwrap_or(i64::MAX).saturating_add(1);
        let rows: Vec<HeadRow> = self
            .store
            .read(|conn| {
                let mut st = conn.prepare(HEADS_SQL)?;
                let rows = st.query_map([limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
                rows.collect()
            })
            .map_err(|e| failed("read heads", e))?;
        if rows.len() > MAX_BRANCHES {
            return Err(JournalError::InputLimit(format!("more than {MAX_BRANCHES} branches")));
        }
        Ok(rows)
    }

    /// Rebuilds the head cache from the entries: one row per branch, in one transaction.
    pub(crate) fn rebuild_heads(&self) -> Result<HeadsRebuilt, JournalError> {
        self.check_poison()?;
        let _gate = guard(self.gate);
        self.check_poison()?;
        let rows = self.read_heads()?;
        let mut txn = self.store.transaction();
        for (branch, seq, hash, _) in &rows {
            // Checked before writing: the cache only ever names well-formed rows.
            branch_head(branch.clone(), *seq, hash)?;
            txn.upsert(
                "heads",
                &[("branch", Value::Text(branch.clone()))],
                &[("entry_hash", Value::Text(hash.clone()))],
            );
        }
        if rows.is_empty() {
            txn.abort();
        } else {
            txn.commit().map_err(|e| failed("write head cache", e))?;
        }
        Ok(HeadsRebuilt { branches: rows.len() })
    }
}

impl Journal {
    /// The head of `branch` (module documentation): from the journal, after checking the
    /// cache row and rebuilding the cache if the row is missing or names another entry.
    /// `None` if the branch has no entry.
    pub fn head(&self, branch: &str) -> Result<Option<BranchHead>, JournalError> {
        self.core().head(branch)
    }

    /// The head of every branch, in branch order, checked against the cache as
    /// [`Journal::head`] does (one rebuild if any row is missing or stale).
    pub fn heads(&self) -> Result<Vec<BranchHead>, JournalError> {
        self.core().heads()
    }

    /// Rebuilds the head cache from the entries: one row per branch, in one transaction.
    pub fn rebuild_heads(&self) -> Result<HeadsRebuilt, JournalError> {
        self.core().rebuild_heads()
    }

    /// Makes, stores and returns a checkpoint of this journal's stream (REQ-001).
    pub fn checkpoint(&self, reason: Reason) -> Result<Checkpoint, JournalError> {
        self.checkpoint_with_ledger(reason, None)
    }

    /// [`Journal::checkpoint`] holding the Access Ledger's head too (REQ-002).
    pub fn checkpoint_with_ledger(
        &self,
        reason: Reason,
        ledger: Option<LedgerHead>,
    ) -> Result<Checkpoint, JournalError> {
        self.check_poison()?;
        let _gate = guard(&self.gate);
        self.check_poison()?;
        let tail =
            read_tail(&self.store, &self.stream)?.ok_or_else(|| JournalError::EmptyStream(self.stream.clone()))?;
        let count: i64 = self
            .store
            .read(|conn| conn.query_row("SELECT count(*) FROM entries WHERE stream = ?1", [&self.stream], |r| r.get(0)))
            .map_err(|e| failed("count entries", e))?;
        let count = u64::try_from(count).map_err(|_| bad_row("negative count"))?;
        if count != tail.seq {
            return Err(JournalError::ChainInvalid(format!(
                "stream holds {count} entries but its tail is at sequence {}",
                tail.seq
            )));
        }
        let now = self.clock.now().as_secs();
        let time = format_time(now)
            .ok_or_else(|| JournalError::Schema(format!("host clock (Unix {now}) is after 9999-12-31T23:59:59Z")))?;
        let cp = Checkpoint {
            format: Format::LinearV1,
            stream: self.stream.clone(),
            seq: tail.seq,
            head: tail.hash,
            count,
            ledger,
            reason,
            time,
        };
        let body = cp.canonical_bytes().map_err(|e| JournalError::Schema(e.to_string()))?;
        let mut txn = self.store.transaction();
        txn.insert(
            "checkpoints",
            &[
                ("stream", Value::Text(self.stream.clone())),
                ("entry_hash", Value::Text(tail.hash.to_string())),
                ("body", Value::Blob(body)),
            ],
        );
        txn.commit().map_err(|e| failed("store checkpoint", e))?;
        Ok(cp)
    }

    /// The most recent stored checkpoint of this stream, decoded and checked against its
    /// row; `None` if there is none.
    pub fn last_checkpoint(&self) -> Result<Option<StoredCheckpoint>, JournalError> {
        self.check_poison()?;
        let row: Option<(i64, String, Vec<u8>)> = self
            .store
            .read(|conn| {
                let mut st = conn.prepare(
                    "SELECT id, entry_hash, body FROM checkpoints WHERE stream = ?1 ORDER BY id DESC LIMIT 1",
                )?;
                let mut rows = st.query_map([&self.stream], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                rows.next().transpose()
            })
            .map_err(|e| failed("read checkpoint", e))?;
        let Some((id, entry_hash, body)) = row else { return Ok(None) };
        let bad = |why: String| JournalError::ChainInvalid(format!("checkpoint {id}: {why}"));
        let checkpoint = Checkpoint::from_bytes(&body).map_err(|e| bad(e.to_string()))?;
        if checkpoint.stream != self.stream || Some(checkpoint.head) != parse_digest(&entry_hash) {
            return Err(bad("row columns differ from the body".into()));
        }
        Ok(Some(StoredCheckpoint { id, digest: fid_canon::digest_bytes(&body), checkpoint }))
    }

    /// Checks `cp` against this journal's stored chain ([`super::verify`]), streaming the
    /// `seq`, `prev_hash` and `hash` columns in sequence order. It trusts the stored `hash`
    /// columns and does not recompute any hash from the entry headers; full verification of
    /// the chain is `CORE.JRN.CHAIN.VERIFY`'s job.
    pub fn verify_checkpoint(&self, cp: &Checkpoint, coverage: Coverage) -> Result<Verified, CheckpointError> {
        self.check_poison().map_err(|e| CheckpointError::Storage(e.to_string()))?;
        self.store
            .read(|conn| {
                let mut st = conn.prepare("SELECT seq, prev_hash, hash FROM entries ORDER BY seq")?;
                let rows = st.query_map([], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?))
                })?;
                let links = rows.map(|row| -> Result<Link, CheckpointError> {
                    let (seq, prev, hash) = row.map_err(|e| CheckpointError::Storage(e.to_string()))?;
                    let bad = |what: &str| CheckpointError::Storage(format!("entry {seq}: {what}"));
                    Ok(Link {
                        seq: u64::try_from(seq).map_err(|_| bad("negative sequence"))?,
                        prev: prev.as_deref().and_then(parse_digest).ok_or_else(|| bad("prev_hash is not a digest"))?,
                        hash: parse_digest(&hash).ok_or_else(|| bad("hash is not a digest"))?,
                    })
                });
                Ok(verify(cp, &self.stream, links, coverage))
            })
            .map_err(|e| CheckpointError::Storage(e.to_string()))?
    }
}

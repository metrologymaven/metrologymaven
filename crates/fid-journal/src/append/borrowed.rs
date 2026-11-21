//! Appending on a borrowed store (`CORE.JRN.CHAIN.APPEND`, REQ-008).
//!
//! [`Journal::on_store`] opens the chain of a stream in a [`Store`] the caller lends, such as
//! the temporary copy that [`Store::copy_to`] hands its `finish` callback, and returns a
//! [`JournalRef`]. It keeps every guarantee of the owned path:
//!
//! - **Single writer.** The lent store must be opened for writing and still hold its project
//!   write lock, with the lock file still the file at `lock` ([`Store::claim_append`]);
//!   otherwise `read_only`. The claim is exclusive in the process: while a [`JournalRef`]
//!   lives, a second [`Journal::on_store`] on the same store is refused (`append_failed`).
//!   The lock is proved again before every append, and [`fid_store::Txn::commit`] verifies it
//!   once more under the store's commit mutex. An owned [`Journal`] never lends its store, and
//!   the OS lock keeps a second process (or a second [`Store`] of the same project) out.
//! - **No second connection.** Every read and write goes through the lent store's one
//!   connection ([`Store::read`], [`Store::transaction`]), which only
//!   `fid_store::schema::open_write` (the hardened open) creates.
//! - **Same chain, hash, time and durability rules.** The open runs the owned open's chain
//!   checks (sequences 1..n, one stream, the full link walk, a verified tail), and `append`
//!   runs the owned path's transaction code unchanged (tail verification, `expected_prev`,
//!   time validity, order and ahead rules, blob checks, payload blob, `entries` and
//!   `entry_blobs` rows, `Txn::commit` with blobs durable before `BEGIN IMMEDIATE` and COMMIT
//!   under `synchronous=FULL`). The same draft and salts give byte-identical rows.
//! - **Serialised, no group commit.** Appends through one [`JournalRef`] run one at a time
//!   under its store lock, each in its own transaction. Group commit only groups
//!   transactions; it never changes an entry's bytes.
//! - **Poison.** A COMMIT whose outcome is unknown poisons the [`JournalRef`] and the store's
//!   append claim ([`fid_store::AppendClaim::poison`]): every later call, and every later
//!   [`Journal::on_store`] on the same store, fails until the project is reopened. Inside
//!   `copy_to`'s `finish`, return the error so the temporary copy is removed.
//! - **Head cache and checkpoints (`CORE.JRN.CHAIN.CKPT`).** [`JournalRef::head`] and
//!   [`JournalRef::heads`] run the owned path's cache rules (journal wins, rebuild on a missing
//!   or stale row). A [`JournalRef`] writes no checkpoint: checkpoints are taken for a
//!   signature, an export or a close by the owner of the project. Appends only extend the
//!   chain, so checkpoints already stored (in a copy, the source's) still verify as a prefix.

use super::{
    check_chain, check_stream_name, failed, guard, Append, AppendRequest, Clock, Core, EntryHash, JournalError,
    Pending, SystemClock, Tail,
};
use crate::checkpoint::BranchHead;
use crate::verify::{verify_store, ReadError, VerifyOptions, VerifyReport};
use crate::Journal;
use fid_store::cas::BlobStore;
use fid_store::{AppendClaim, Store, StoreError, TxnError};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The chained stream of a borrowed [`Store`] (module documentation). It holds the store's
/// append claim for its lifetime. `Send + Sync`.
pub struct JournalRef<'s> {
    claim: AppendClaim<'s>,
    stream: String,
    clock: Arc<dyn Clock>,
    /// Set after a COMMIT whose outcome is unknown; the store's claim is poisoned with it.
    poisoned: AtomicBool,
    /// The store lock: held by an append from its tail read to its COMMIT, by `tail`, and by
    /// the head-cache rebuild.
    gate: Mutex<()>,
}

impl fmt::Debug for JournalRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JournalRef")
            .field("stream", &self.stream)
            .field("root", &self.claim.store().location().root())
            .finish()
    }
}

/// The error of a refused claim.
fn claim_error(e: TxnError) -> JournalError {
    match e {
        TxnError::ReadOnly => JournalError::ReadOnly("the store is not opened for writing".into()),
        TxnError::Store(StoreError::LockLost) => {
            JournalError::ReadOnly("the store no longer holds its project write lock".into())
        }
        TxnError::Claimed(why) => JournalError::AppendFailed(format!("claim store: {why}")),
        other => failed("claim store", other),
    }
}

impl Journal {
    /// Opens the chain of `stream` on a store the caller lends (module documentation), with
    /// the system clock. Refuses a store not opened for writing, or without its write lock,
    /// with `read_only`, and a store whose append path is already claimed with
    /// `append_failed`; then runs [`Journal::open`]'s chain checks (`chain_invalid`).
    pub fn on_store<'s>(store: &'s Store, stream: &str) -> Result<JournalRef<'s>, JournalError> {
        Journal::on_store_with_clock(store, stream, Arc::new(SystemClock))
    }

    /// [`Journal::on_store`] with an injected host clock for the time-ahead rule.
    pub fn on_store_with_clock<'s>(
        store: &'s Store,
        stream: &str,
        clock: Arc<dyn Clock>,
    ) -> Result<JournalRef<'s>, JournalError> {
        check_stream_name(stream)?;
        let claim = store.claim_append().map_err(claim_error)?;
        check_chain(store, stream)?;
        Ok(JournalRef {
            claim,
            stream: stream.to_string(),
            clock,
            poisoned: AtomicBool::new(false),
            gate: Mutex::new(()),
        })
    }
}

impl<'s> JournalRef<'s> {
    fn core(&self) -> Core<'_> {
        Core {
            store: self.claim.store(),
            stream: &self.stream,
            clock: &*self.clock,
            poisoned: &self.poisoned,
            gate: &self.gate,
            #[cfg(feature = "test-hooks")]
            hook: None,
        }
    }

    /// The stream this journal appends to.
    pub fn stream(&self) -> &str {
        &self.stream
    }

    /// The lent store.
    pub fn store(&self) -> &'s Store {
        self.claim.store()
    }

    /// The store's blob store (callers put referenced blobs here before appending).
    pub fn blobs(&self) -> &'s BlobStore {
        self.claim.store().blobs()
    }

    /// True once a COMMIT's outcome is unknown; only reopening the project clears it.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned.load(Ordering::SeqCst)
    }

    /// Appends one entry under [`Journal::append`]'s rules, in its own transaction; returns
    /// its hash once the COMMIT returned. `head_moved` if `expected_prev` is set and is not
    /// the tail hash.
    pub fn append(&self, req: AppendRequest) -> Result<EntryHash, JournalError> {
        let core = self.core();
        core.precheck(&req)?;
        let _gate = guard(&self.gate);
        core.check_poison()?;
        self.claim.verify_lock().map_err(claim_error)?;
        let outcome = core.transaction(&[Pending { ticket: 0, req }]);
        if self.is_poisoned() {
            self.claim.poison();
        }
        match outcome {
            Ok((mut done, _)) => match done.pop() {
                Some((_, result)) => result,
                None => Err(JournalError::AppendFailed("the transaction took no request".into())),
            },
            Err(e) => Err(e),
        }
    }

    /// The current chain tail, verified; `None` for an empty chain ([`Journal::tail`]).
    pub fn tail(&self) -> Result<Option<Tail>, JournalError> {
        self.core().tail()
    }

    /// The head of `branch` under the head-cache rules of [`Journal::head`].
    pub fn head(&self, branch: &str) -> Result<Option<BranchHead>, JournalError> {
        self.core().head(branch)
    }

    /// The head of every branch, as [`Journal::heads`].
    pub fn heads(&self) -> Result<Vec<BranchHead>, JournalError> {
        self.core().heads()
    }

    /// `journal.verify_chain@1` over this stream of the lent store ([`Journal::verify_chain`]).
    pub fn verify(&self, opts: &VerifyOptions) -> Result<VerifyReport, ReadError> {
        self.core().check_poison().map_err(|e| ReadError::Storage(e.to_string()))?;
        verify_store(self.claim.store(), &self.stream, opts)
    }
}

impl Append for JournalRef<'_> {
    fn append(&self, req: AppendRequest) -> Result<EntryHash, JournalError> {
        JournalRef::append(self, req)
    }

    fn tail(&self) -> Result<Option<Tail>, JournalError> {
        JournalRef::tail(self)
    }
}

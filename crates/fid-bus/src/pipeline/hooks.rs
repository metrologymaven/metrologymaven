//! Commit hooks: the generic seam through which an authority-bearing fold checks an entry
//! atomically with its commit and then folds it (REQ-CORE.RPL.RECON.FOLD-008 (d), "Precommit
//! equals fold"). See the module docs of [`crate::pipeline`] for where they run.

use crate::error::{BusError, Detail, InternalReason};
use crate::handler::Event;
use crate::policy::projection::EntryOrigin;
use fid_canon::{CValue, Digest};
use fid_journal::branch::EntryOrigin as JournalOrigin;
use fid_journal::{EntryDraft, EntryHeader, EntryPayload, EntryTime};
use std::collections::BTreeSet;
use std::sync::Arc;

/// Which entries a [`CommitHook`] is called for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watch {
    /// Every entry the commit stage commits (a fold that must see every entry, e.g. to keep
    /// an index of released records).
    All,
    /// Only entries holding at least one event of these types.
    Effects(BTreeSet<String>),
}

impl Watch {
    /// Whether an entry with `events` is watched.
    pub fn matches(&self, events: &[Event]) -> bool {
        match self {
            Watch::All => true,
            Watch::Effects(types) => events.iter().any(|e| types.contains(e.type_id().as_str())),
        }
    }
}

/// What a [`CommitHook`] sees of the entry being committed. Built only by the commit stage,
/// from the entry it is about to append (already built and hashed with the salts it will be
/// appended with, so [`CommitRecord::hash`] is the hash the entry will have) and never from an
/// event body.
#[derive(Debug, Clone)]
pub struct CommitRecord<'a> {
    pub(crate) origin: Option<EntryOrigin>,
    pub(crate) journal: Option<JournalOrigin>,
    pub(crate) decided_at: Digest,
    pub(crate) header: &'a EntryHeader,
    pub(crate) payload: &'a EntryPayload,
    pub(crate) args: Option<&'a CValue>,
}

impl CommitRecord<'_> {
    /// The bus origin (principal, command `id@major`, parameters commitment) of a command
    /// entry; `None` for a session header.
    pub fn origin(&self) -> Option<&EntryOrigin> {
        self.origin.as_ref()
    }

    /// The journal origin of a command entry: principal kind, command, major, class, branch,
    /// the stage that added the events (always `handler`: this stage adds no events of its own
    /// except the `RequestApproved` of an approval, which `policy::projection` owns) and the
    /// header's `human_assertion`; `None` for a session header.
    pub fn journal(&self) -> Option<&JournalOrigin> {
        self.journal.as_ref()
    }

    /// The header's `promoted_from`: always `None`, because the commit stage writes no
    /// promoted copies (CORE.JRN.BR.PROMO's re-executed steps are not built).
    pub fn promoted_from(&self) -> Option<Digest> {
        None
    }

    /// The branch head the call was decided against: the head of the entry's branch before
    /// the entry ([`fid_journal::GENESIS_PREV`] for a branch with no entry). The commit stage
    /// has checked, under its commit lock, that the branch head is still this.
    pub fn decided_at(&self) -> Digest {
        self.decided_at
    }

    /// The entry's header, as it will be appended.
    pub fn header(&self) -> &EntryHeader {
        self.header
    }

    /// The entry's payload: salts and the committed values (principal identity, reason,
    /// arguments, events), so a hook can check an opening against its commitment.
    pub fn payload(&self) -> &EntryPayload {
        self.payload
    }

    /// The entry's hash.
    pub fn hash(&self) -> Digest {
        self.header.hash
    }

    /// The command's arguments as entered (the entry's `args`); `None` for a session header.
    pub fn args(&self) -> Option<&CValue> {
        self.args
    }
}

/// A fold whose state decides later commands (REQ-CORE.RPL.RECON.FOLD-008), registered with
/// the bus ([`crate::pipeline::Bus::register_hook`]).
///
/// For every entry it [watches](CommitHook::watch), the commit stage calls, under its commit
/// lock and in registration order:
///
/// 1. [`CommitHook::precommit`] before the append, with the entry already built and hashed.
///    Any error aborts the commit and is returned to the caller; nothing is appended. A hook
///    reports a stale decided-against state (head, sequence or prior digest) as
///    `conflict/head_moved`.
/// 2. [`CommitHook::fold`] after the append succeeded. Since precommit equals fold, a fold
///    refusal here is a bug: the bus is poisoned (every later commit is
///    `internal/invariant_violated` until it is rebuilt from the journal) and the call returns
///    `internal/invariant_violated` naming the entry.
///
/// Both take `&self`; a hook keeps its state behind its own lock. The commit lock serialises
/// every commit of one bus, so no two hooks calls of different commits interleave.
pub trait CommitHook: Send + Sync {
    /// A short name for error details.
    fn name(&self) -> &str;
    /// The entries this hook is called for.
    fn watch(&self) -> Watch;
    /// The fold's rules applied to the entry about to be committed.
    fn precommit(&self, record: &CommitRecord<'_>, events: &[Event]) -> Result<(), BusError>;
    /// Folds the committed entry.
    fn fold(&self, record: &CommitRecord<'_>, events: &[Event]) -> Result<(), BusError>;
}

/// Journal-infrastructure entries a command needs appended right before its own entry or
/// record, inside the same commit (CORE.PRJ.CONT.LIFE: the session header of create and open).
/// Registered per command ([`crate::pipeline::Bus::register_leading`]). Only
/// `session_header` drafts are taken; anything else is `internal/invariant_violated` and
/// nothing is appended, so this is not a side door for command entries.
pub trait LeadingEntries: Send + Sync {
    /// The drafts to append, in order, for a call of the registered command by the principal
    /// of `draft` (the command's own draft as the stage built it). `time` is the call's time.
    fn leading(&self, draft: &EntryDraft, time: &EntryTime) -> Result<Vec<EntryDraft>, BusError>;
}

/// The registered hooks, in registration order.
#[derive(Default, Clone)]
pub(crate) struct Hooks {
    pub(crate) list: Vec<Arc<dyn CommitHook>>,
}

impl Hooks {
    /// Runs every watching hook's precommit, in order; the first refusal is returned.
    pub(crate) fn precommit(&self, record: &CommitRecord<'_>, events: &[Event]) -> Result<(), BusError> {
        for h in self.list.iter().filter(|h| h.watch().matches(events)) {
            h.precommit(record, events)?;
        }
        Ok(())
    }

    /// Folds the committed entry into every watching hook; a refusal is an invariant
    /// violation naming the hook and the entry.
    pub(crate) fn fold(&self, record: &CommitRecord<'_>, events: &[Event]) -> Result<(), BusError> {
        for h in self.list.iter().filter(|h| h.watch().matches(events)) {
            h.fold(record, events).map_err(|e| {
                BusError::Internal(
                    InternalReason::InvariantViolated,
                    Detail::new(&format!(
                        "hook {} refused committed entry {} after its precommit accepted it: {}",
                        h.name(),
                        record.hash(),
                        e
                    )),
                )
            })?;
        }
        Ok(())
    }
}

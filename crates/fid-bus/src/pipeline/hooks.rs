//! Commit hooks: the generic seam through which an authority-bearing fold checks an entry
//! atomically with its commit and then folds it (REQ-CORE.RPL.RECON.FOLD-008 (d), "Precommit
//! equals fold"). See the module docs of [`crate::pipeline`] for where they run.

use crate::error::{BusError, Detail, InternalReason};
use crate::handler::Event;
use crate::policy::projection::EntryOrigin;
use fid_canon::{CValue, Digest};
use fid_journal::branch::EntryOrigin as JournalOrigin;
use fid_journal::{EntryHeader, EntryPayload};
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
    /// entry; `None` for an entry that is not a command entry.
    pub fn origin(&self) -> Option<&EntryOrigin> {
        self.origin.as_ref()
    }

    /// The journal origin of a command entry: principal kind, command, major, class, branch,
    /// the stage that added the events and the header's `human_assertion`; `None` for an entry
    /// that is not a command entry.
    ///
    /// Two fields are fixed until other leaves are built (NODE.md, "Deferred"): the source is
    /// always `handler`, also for the `RequestApproved` event the bus adds to an approval, and
    /// `human_assertion` is always `None`, because the stage writes none.
    pub fn journal(&self) -> Option<&JournalOrigin> {
        self.journal.as_ref()
    }

    /// The header's `promoted_from`: always `None`, because the commit stage writes no
    /// promoted copies. CORE.JRN.BR.PROMO must not route promoted copies through
    /// [`crate::pipeline::Bus::execute`] until `promoted_from` is threaded through.
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

    /// The command's arguments as entered (the entry's `args`); `None` for an entry that is
    /// not a command entry.
    pub fn args(&self) -> Option<&CValue> {
        self.args
    }
}

/// A fold whose state decides later commands (REQ-CORE.RPL.RECON.FOLD-008), registered with
/// the bus ([`crate::pipeline::Bus::register_hook`]).
///
/// A hook is two-phase. For every commit holding at least one entry it
/// [watches](CommitHook::watch), the commit stage, under its commit lock and in registration
/// order:
///
/// 1. opens a staged copy of the fold state with [`CommitHook::stage`];
/// 2. before the append, calls [`StagedFold::precommit`] for each watched entry of the commit
///    in order, with the entry already built and hashed. The staged copy folds each entry it
///    accepts, so the precommit of an entry sees the earlier entries of the same commit already
///    folded in. Any error aborts the commit and is returned to the caller; nothing is
///    appended and the staged copy is dropped. A hook reports a stale decided-against state
///    (head, sequence or prior digest) as `conflict/head_moved`;
/// 3. after the append succeeded, calls [`StagedFold::install`], which makes the staged copy
///    the hook's state. Since the staged copy already holds the fold, an install error is a
///    bug: the bus is poisoned (every later commit is `internal/invariant_violated` until the
///    project is reopened) and the call returns `internal/invariant_violated` naming the
///    entry.
///
/// A hook keeps its state behind its own lock. The commit lock serialises every commit of one
/// bus, so the installed state does not change between `stage` and `install`. A hook must not
/// recover a poisoned lock of its own (`PoisonError::into_inner`): it returns an error
/// instead. A panic inside a hook poisons the commit lock, and the bus then poisons itself.
pub trait CommitHook: Send + Sync {
    /// A short name for error details.
    fn name(&self) -> &str;
    /// The entries this hook is called for.
    fn watch(&self) -> Watch;
    /// A staged copy of the fold state (or a delta over it) for one commit. Called under the
    /// commit lock; an error aborts the commit and nothing is appended.
    fn stage(&self) -> Result<Box<dyn StagedFold + '_>, BusError>;
}

/// One commit's staged copy of a [`CommitHook`]'s fold state.
pub trait StagedFold {
    /// The fold's rules applied to the next watched entry of the commit, folding it into the
    /// staged copy on success.
    fn precommit(&mut self, record: &CommitRecord<'_>, events: &[Event]) -> Result<(), BusError>;
    /// Makes the staged copy the hook's state, after the append.
    fn install(self: Box<Self>) -> Result<(), BusError>;
}

/// The registered hooks, in registration order.
#[derive(Default, Clone)]
pub(crate) struct Hooks {
    pub(crate) list: Vec<Arc<dyn CommitHook>>,
}

/// The staged copies of one commit, ready to install.
pub(crate) struct Staged<'h> {
    list: Vec<(&'h dyn CommitHook, Box<dyn StagedFold + 'h>)>,
    hash: Digest,
}

impl Hooks {
    /// Stages every hook that watches at least one of `records` and runs its precommit on
    /// each watched record, in order; the first refusal is returned and nothing is staged.
    pub(crate) fn stage<'h>(&'h self, records: &[(CommitRecord<'_>, &[Event])]) -> Result<Staged<'h>, BusError> {
        let hash = records.last().map(|(r, _)| r.hash()).unwrap_or(fid_journal::GENESIS_PREV);
        let mut list = Vec::new();
        for h in &self.list {
            let watch = h.watch();
            if !records.iter().any(|(_, ev)| watch.matches(ev)) {
                continue;
            }
            let mut staged = h.stage()?;
            for (r, ev) in records.iter().filter(|(_, ev)| watch.matches(ev)) {
                staged.precommit(r, ev)?;
            }
            list.push((h.as_ref(), staged));
        }
        Ok(Staged { list, hash })
    }
}

impl Staged<'_> {
    /// Installs every staged copy; a refusal is an invariant violation naming the hook and the
    /// (last) entry of the commit.
    pub(crate) fn install(self) -> Result<(), BusError> {
        let hash = self.hash;
        for (h, staged) in self.list {
            staged.install().map_err(|e| {
                BusError::Internal(
                    InternalReason::InvariantViolated,
                    Detail::new(&format!(
                        "hook {} refused to install committed entry {hash} after its precommit accepted it: {e}",
                        h.name()
                    )),
                )
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    //! Finding 3 of the #416 review: when one commit carries several records, the precommit of
    //! each sees the earlier records of the same commit already folded in. The commit stage
    //! commits one entry per commit today (CORE.PRJ.CONT.LIFE's leading entries will make it
    //! several), so this drives `Hooks::stage` directly with two chained entries.

    use super::*;
    use crate::error::ConflictReason;
    use fid_journal::{
        build, AiAssistance, CommandDraft, DraftBody, Entry, EntryDraft, EntryTime, Manifests, OsSalts, PrincipalDraft,
        GENESIS_PREV,
    };
    use fid_types::{Determinism, SideEffect};
    use std::sync::Mutex;

    fn entry(seq: u64, prev: Digest) -> Entry {
        let draft = EntryDraft {
            stream: "record".into(),
            branch: "main".into(),
            logical_parent: None,
            time: EntryTime { utc: "2026-11-21T00:00:00Z".into(), mono_ns: seq, sync: "ntp".into() },
            principal: PrincipalDraft {
                kind: crate::principal::PrincipalKind::Human,
                identity: CValue::Str("alice".into()),
                on_behalf_of: None,
            },
            ai_assistance: AiAssistance::None,
            manifests: Manifests { registry: Digest([7; 32]), kernel: Digest([2; 32]) },
            reason: None,
            human_assertion: None,
            body: DraftBody::Command(CommandDraft {
                id: "test.write".into(),
                major: 1,
                class: SideEffect::W2,
                determinism: Determinism::D0,
                seed: 0,
                args: CValue::Null,
                events: Vec::new(),
                inputs: Vec::new(),
                consumed_r1: Vec::new(),
                outputs: Vec::new(),
                algorithms: Vec::new(),
            }),
        };
        build(&draft, seq, prev, &mut OsSalts).unwrap()
    }

    fn record(e: &Entry) -> CommitRecord<'_> {
        CommitRecord {
            origin: None,
            journal: None,
            decided_at: GENESIS_PREV,
            header: &e.header,
            payload: &e.payload,
            args: None,
        }
    }

    /// A chain fold: each entry's `prev` must be the last folded hash.
    struct Chain {
        folded: Mutex<Vec<Digest>>,
    }

    struct ChainStage<'a> {
        hook: &'a Chain,
        copy: Vec<Digest>,
    }

    impl CommitHook for Chain {
        fn name(&self) -> &str {
            "chain"
        }
        fn watch(&self) -> Watch {
            Watch::All
        }
        fn stage(&self) -> Result<Box<dyn StagedFold + '_>, BusError> {
            Ok(Box::new(ChainStage { hook: self, copy: self.folded.lock().unwrap().clone() }))
        }
    }

    impl StagedFold for ChainStage<'_> {
        fn precommit(&mut self, r: &CommitRecord<'_>, _events: &[Event]) -> Result<(), BusError> {
            let last = self.copy.last().copied().unwrap_or(GENESIS_PREV);
            if r.header().prev != last {
                return Err(BusError::Conflict(ConflictReason::HeadMoved, Detail::new("not on the folded tail")));
            }
            self.copy.push(r.hash());
            Ok(())
        }
        fn install(self: Box<Self>) -> Result<(), BusError> {
            *self.hook.folded.lock().unwrap() = self.copy;
            Ok(())
        }
    }

    #[test]
    fn each_precommit_sees_the_earlier_records_of_the_commit() {
        let chain = Arc::new(Chain { folded: Mutex::new(Vec::new()) });
        let hooks = Hooks { list: vec![chain.clone() as Arc<dyn CommitHook>] };
        let first = entry(1, GENESIS_PREV);
        let second = entry(2, first.header.hash);
        let none: &[Event] = &[];
        // The second record chains on the first, which is only in the staged copy: a precommit
        // against the installed state would refuse it.
        let staged = hooks.stage(&[(record(&first), none), (record(&second), none)]).unwrap();
        assert!(chain.folded.lock().unwrap().is_empty(), "nothing is installed before the append");
        staged.install().unwrap();
        assert_eq!(*chain.folded.lock().unwrap(), vec![first.header.hash, second.header.hash]);

        // A refusal of a later record drops the whole staged copy.
        let third = entry(3, second.header.hash);
        let stray = entry(4, first.header.hash);
        let e = hooks.stage(&[(record(&third), none), (record(&stray), none)]).err().unwrap();
        assert_eq!(e.reason(), "head_moved");
        assert_eq!(chain.folded.lock().unwrap().len(), 2);
    }
}

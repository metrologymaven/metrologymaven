//! The in-process bus port: sessions, human upgrade and calls. Spec node `CORE.BUS.PORT`.
//!
//! [`LocalPort`] implements [`fid_bus_api::BusPort`] over a composed [`Bus`]. One port is one
//! authenticated connection: it owns its [`SessionKey`] and, after a confirmed upgrade, its
//! [`UpgradeGrant`], and neither ever leaves this crate. No operation of a port returns,
//! clones or accepts the key, the grant or a pending upgrade (REQ-004); a surface holding a
//! port can only ask for one call at a time, read the principal kind the bus derives, and ask
//! for a human upgrade.
//!
//! Who confirms an upgrade, the OS user a port's channel carries and the clock of the upgrade
//! come from the [`SessionHost`], whose constructor takes only `fid-bus` types (REQ-003). Only
//! code that links `fid-bus` (the composition root, `AUTO.SURF.COMPOSE`) can build one, so a
//! script cannot pick its own presence provider and make itself `human` (REQ-CORE.BUS.PRIN-003,
//! -009, -010, -011).
//!
//! **Port kinds.** A port is opened as `cli`, `python`, `control` or `mcp` only
//! ([`PortKind`], REQ-001). There is no way to open a `gui`, `internal`, `device`, `service`
//! or server channel through a port: a `gui` channel derives `human` without an upgrade
//! (REQ-CORE.BUS.PRIN-005), and none of the four port kinds does.
//!
//! **Locks.** The [`SessionTable`] sits behind an `RwLock` in the host. [`SessionHost::open`],
//! the commit of an upgrade and the end of a session take the write lock;
//! [`BusPort::principal_kind`], the first upgrade step and [`Bus::execute`] take the read
//! lock. [`PendingUpgrade::confirm`](crate::principal::PendingUpgrade::confirm), which asks a
//! person, runs with no table lock held, so other ports of the same host keep calling and new
//! ports keep opening while a person is asked (AC-5). The host clock is locked only for each
//! reading, never across the prompt.
//!
//! **Upgrade order** (REQ-007 .. REQ-011, REQ-015). [`BusPort::confirm_human`] runs
//! `prepare_upgrade` first (a `control` or `mcp` port gets `upgrade_refused/channel` whatever
//! the host's provider), then refuses `upgrade_refused/declined` with detail
//! [`NO_PRESENCE_PROVIDER`] when the host has no provider (nobody is asked), then asks the
//! person, then commits the `principal_upgrade` entry on the bus's [`UpgradeJournal`]. A bus
//! with no upgrade journal (the no-project bus) cannot record it, so the port holds the
//! confirmation as pending and stays `script`; [`LocalPort::attach`] to a bus with an upgrade
//! journal commits it before switching. A port whose grant is recorded on one journal moves
//! only to a bus with the same [`JournalId`]: a grant is never carried to a journal that has
//! no `principal_upgrade` entry for it (REQ-CORE.BUS.PRIN-011).
//!
//! **Errors** (REQ-012). Every failure is an [`ApiError`] with the bus error's code, reason and
//! detail unchanged and the stage that raised it: the pipeline stage for a call, `session` for
//! the upgrade steps, the call's inputs, deriving the principal, opening and attaching. The
//! stage report does not tell a hidden command from an unknown one: both are refused byte for
//! byte alike at the `resolve` stage (D-035).
//!
//! The port reads no principal kind, upgrade state or identity from environment variables,
//! argv, files or call parameters (REQ-014): the identity is the host's OS user, and the call
//! parameters go to the bus as the command's parameters only. Determinism N (presence prompts,
//! clock reads, locks); a call keeps the determinism of its command.

#![deny(clippy::wildcard_enum_match_arm, clippy::match_wildcard_for_single_variants)]

use crate::error::{BusError, Code, Detail, InternalReason, PreconditionReason, UpgradeRefusedReason};
use crate::handler::{Event, StateView};
use crate::pipeline::{Bus, Call, CommandResult, Invocation, Stage};
use crate::principal::{
    Channel, ChannelKind, ConfirmedUpgrade, HumanPresence, Identity, JournalAppend, OsUser, PrincipalId, SessionId,
    SessionKey, SessionTable, UpgradeClock, UpgradeContext, UpgradeGrant,
};
use fid_bus_api::{ApiError, ApiReply, ApiStage, BusCall, BusPort, ChannelInfo, InvalidApiError, PortKind};
use fid_canon::CValue;
use fid_journal::EntryTime;
use fid_registry::PredContext;
use fid_types::PrincipalKind;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// The detail of the `upgrade_refused/declined` refusal of a host with no presence provider
/// (REQ-008).
pub const NO_PRESENCE_PROVIDER: &str = "no_presence_provider";
/// The detail of the `precondition/false` refusal of [`LocalPort::attach`] (REQ-015).
pub const OTHER_JOURNAL: &str = "upgrade recorded on another journal";

fn invariant(detail: &str) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, Detail::new(detail))
}

/// The channel kind of a port kind. Total, no wildcard; never `gui` (REQ-001).
pub const fn channel_kind(kind: PortKind) -> ChannelKind {
    match kind {
        PortKind::Cli => ChannelKind::Cli,
        PortKind::Python => ChannelKind::Python,
        PortKind::Control => ChannelKind::Control,
        PortKind::Mcp => ChannelKind::Mcp,
    }
}

/// The API stage of a pipeline stage; `None` (an error raised outside the stages) is
/// `session` (REQ-012).
fn api_stage(stage: Option<Stage>) -> ApiStage {
    match stage {
        None => ApiStage::Session,
        Some(Stage::Resolve) => ApiStage::Resolve,
        Some(Stage::Validate) => ApiStage::Validate,
        Some(Stage::Authorise) => ApiStage::Authorise,
        Some(Stage::Precondition) => ApiStage::Precondition,
        Some(Stage::Route) => ApiStage::Route,
        Some(Stage::Handler) => ApiStage::Handler,
        Some(Stage::Effects) => ApiStage::Effects,
        Some(Stage::Commit) => ApiStage::Commit,
    }
}

/// A bus error as an [`ApiError`] raised at `stage`, with its code, reason and detail
/// unchanged (REQ-012). Every code and reason of the catalogue and every [`Detail`] fit, so
/// this never fails for an error of this build ([`SessionHost::new`] checks the catalogue).
pub fn to_api_error(e: &BusError, stage: ApiStage) -> Result<ApiError, InvalidApiError> {
    ApiError::new(e.code().as_str(), e.reason(), e.detail().as_untrusted_str(), stage)
}

// ------------------------------------------------------------------ journals and call parts

/// Names one Record Journal for the upgrade rule of [`LocalPort::attach`] (REQ-015). Opaque:
/// issued only by [`JournalId::fresh`], from a process-wide counter that never repeats, so
/// two journals never share an ID by accident or by choice of a name. The composition root
/// issues one per open project's record stream and gives it to every [`LocalBus`] over that
/// stream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JournalId(u64);

static NEXT_JOURNAL: AtomicU64 = AtomicU64::new(1);

impl JournalId {
    /// A new ID, never issued before in this process. Fails only once the process has used
    /// up every number.
    pub fn fresh() -> Result<JournalId, BusError> {
        let mut current = NEXT_JOURNAL.load(Ordering::Relaxed);
        loop {
            let Some(next) = current.checked_add(1) else {
                return Err(invariant("journal ID numbers exhausted"));
            };
            match NEXT_JOURNAL.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => return Ok(JournalId(current)),
                Err(actual) => current = actual,
            }
        }
    }
}

impl fmt::Display for JournalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "journal#{}", self.0)
    }
}

/// Where a bus's `principal_upgrade` entries go: the Record Journal named by `id`, its append
/// and the stream, branch and manifests of the entry.
pub struct UpgradeJournal {
    id: JournalId,
    append: Mutex<Box<dyn JournalAppend + Send>>,
    ctx: UpgradeContext,
}

impl UpgradeJournal {
    /// The upgrade journal of the Record Journal `id`, appending through `append`.
    pub fn new(id: JournalId, append: Box<dyn JournalAppend + Send>, ctx: UpgradeContext) -> UpgradeJournal {
        UpgradeJournal { id, append: Mutex::new(append), ctx }
    }

    /// The Record Journal this appends to.
    pub fn id(&self) -> &JournalId {
        &self.id
    }
}

impl fmt::Debug for UpgradeJournal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpgradeJournal").field("id", &self.id).field("ctx", &self.ctx).finish_non_exhaustive()
    }
}

/// The per-call inputs of a [`LocalBus`]: the state a call runs against and its precondition
/// roots besides `params`. Implemented by the composition root; called once per call, before
/// the session table is locked.
pub trait CallParts: Send + Sync {
    /// The state view (at the head of the branch the call commits to) and the precondition
    /// roots of one call on `channel`.
    #[allow(clippy::type_complexity)]
    fn inputs(&self, channel: &dyn Channel) -> Result<(Box<dyn StateView>, Box<dyn PredContext>), BusError>;
}

/// One composed [`Bus`] as ports use it: the bus, its per-call parts and, for a bus over a
/// project, its upgrade journal.
pub struct LocalBus {
    bus: Bus,
    parts: Arc<dyn CallParts>,
    upgrades: Option<UpgradeJournal>,
}

impl LocalBus {
    /// A bus for ports. `upgrades` is `None` for a bus with no Record Journal to record a
    /// `principal_upgrade` on (the no-project bus).
    pub fn new(bus: Bus, parts: Arc<dyn CallParts>, upgrades: Option<UpgradeJournal>) -> LocalBus {
        LocalBus { bus, parts, upgrades }
    }

    /// The Record Journal this bus records upgrades on, if any.
    pub fn upgrade_journal(&self) -> Option<&JournalId> {
        self.upgrades.as_ref().map(UpgradeJournal::id)
    }
}

impl fmt::Debug for LocalBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalBus").field("bus", &self.bus).field("upgrades", &self.upgrades).finish_non_exhaustive()
    }
}

// ------------------------------------------------------------------ the session host

/// The host clock, locked for one reading at a time, so no lock is held while a person is
/// asked.
struct HostClock<'a>(&'a Mutex<Box<dyn UpgradeClock + Send>>);

impl UpgradeClock for HostClock<'_> {
    fn now(&mut self) -> EntryTime {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).now()
    }
}

/// The sessions of one process: the [`SessionTable`], the OS user every port's channel
/// carries, the presence provider (or none) and the upgrade clock.
pub struct SessionHost {
    sessions: RwLock<SessionTable>,
    os_user: OsUser,
    presence: Option<Arc<dyn HumanPresence + Send + Sync>>,
    clock: Mutex<Box<dyn UpgradeClock + Send>>,
    /// Returned for a bus error that does not fit an [`ApiError`], which [`SessionHost::new`]
    /// rules out for this build's catalogue.
    fallback: ApiError,
}

impl SessionHost {
    /// A host whose ports carry `os_user` as their authenticated identity, whose upgrades ask
    /// `presence` (none: every upgrade is refused) and read `clock`.
    ///
    /// Checks first that every code and reason of the bus error catalogue builds an
    /// [`ApiError`]; otherwise `internal/invariant_violated` and no host.
    pub fn new(
        os_user: OsUser,
        presence: Option<Arc<dyn HumanPresence + Send + Sync>>,
        clock: Box<dyn UpgradeClock + Send>,
    ) -> Result<SessionHost, BusError> {
        for code in Code::ALL {
            for reason in code.reasons() {
                if ApiError::new(code.as_str(), reason, "", ApiStage::Session).is_err() {
                    return Err(invariant(&format!("bus error {code}/{reason} does not fit the API error")));
                }
            }
        }
        let fallback = ApiError::new(
            "internal",
            "invariant_violated",
            "a bus error does not fit the API error",
            ApiStage::Session,
        )
        .map_err(|e| invariant(&e.to_string()))?;
        Ok(SessionHost {
            sessions: RwLock::new(SessionTable::new()),
            os_user,
            presence,
            clock: Mutex::new(clock),
            fallback,
        })
    }

    /// Opens a port of `kind` on `bus`: a new session from [`SessionTable::open_session`]
    /// labelled `label`, whose channel presents this host's OS user (REQ-002). The port starts
    /// as `script` (`cli`, `python`, `control`) or `agent` (`mcp`).
    pub fn open(self: &Arc<Self>, kind: PortKind, label: SessionId, bus: Arc<LocalBus>) -> Result<LocalPort, ApiError> {
        let key = self
            .write()
            .and_then(|mut table| table.open_session(label))
            .map_err(|e| self.api_error(&e, ApiStage::Session))?;
        let channel = LocalChannel { kind, os_user: self.os_user.clone(), key, grant: None };
        Ok(LocalPort { host: Arc::clone(self), bus, channel: Some(channel), pending: None, recorded_on: None })
    }

    /// The number of sessions of this host that hold a live upgrade grant.
    pub fn upgraded_sessions(&self) -> usize {
        self.sessions.read().unwrap_or_else(PoisonError::into_inner).len()
    }

    fn read(&self) -> Result<RwLockReadGuard<'_, SessionTable>, BusError> {
        self.sessions.read().map_err(|_| invariant("the session table lock is poisoned"))
    }

    fn write(&self) -> Result<RwLockWriteGuard<'_, SessionTable>, BusError> {
        self.sessions.write().map_err(|_| invariant("the session table lock is poisoned"))
    }

    fn api_error(&self, e: &BusError, stage: ApiStage) -> ApiError {
        to_api_error(e, stage).unwrap_or_else(|_| self.fallback.clone())
    }
}

impl fmt::Debug for SessionHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionHost")
            .field("os_user", &self.os_user)
            .field("presence", &self.presence.is_some())
            .finish_non_exhaustive()
    }
}

// ------------------------------------------------------------------ the port

/// A port's authenticated channel: its kind, the host's OS user, the bus-issued key and the
/// grant, if any. Private to this crate (REQ-004).
struct LocalChannel {
    kind: PortKind,
    os_user: OsUser,
    key: SessionKey,
    grant: Option<UpgradeGrant>,
}

impl Channel for LocalChannel {
    fn kind(&self) -> ChannelKind {
        channel_kind(self.kind)
    }

    fn authenticated_identity(&self) -> Option<Identity> {
        let mut identity = Identity::new(PrincipalId::from(&self.os_user));
        identity.os_user = Some(self.os_user.clone());
        Some(identity)
    }

    fn session(&self) -> &SessionKey {
        &self.key
    }

    fn upgrade_grant(&self) -> Option<&UpgradeGrant> {
        self.grant.as_ref()
    }
}

/// One authenticated connection to a [`LocalBus`] (module docs). Not `Clone`. Closing or
/// dropping it ends its session, which revokes its grant (REQ-013).
pub struct LocalPort {
    host: Arc<SessionHost>,
    bus: Arc<LocalBus>,
    /// `None` only while the port is being dropped.
    channel: Option<LocalChannel>,
    /// A confirmed upgrade the port's bus could not record (no upgrade journal, REQ-009).
    pending: Option<ConfirmedUpgrade>,
    /// The journal the port's grant is recorded on; `Some` exactly while it holds a grant.
    recorded_on: Option<JournalId>,
}

impl fmt::Debug for LocalPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalPort")
            .field("kind", &self.channel.as_ref().map(|c| c.kind))
            .field("session", &self.channel.as_ref().map(|c| c.key.journal_id()))
            .field("pending", &self.pending.is_some())
            .field("recorded_on", &self.recorded_on)
            .finish_non_exhaustive()
    }
}

/// The reply of a call's result. A dry run is always [`ApiReply::DryRun`], never a committed
/// or computed reply, so what a call would do never looks like what it did (D-045).
fn reply(result: CommandResult) -> ApiReply {
    match result {
        CommandResult::Done { result, entry: Some(hash), .. } => {
            ApiReply::Committed { entry: fid_types::Digest::from_bytes(hash.0), result }
        }
        CommandResult::Done { result, entry: None, .. } => ApiReply::Computed { result },
        CommandResult::Pending { request_id, .. } => ApiReply::RequestFiled { request_id: request_id.to_string() },
        CommandResult::DryRun { events } => {
            let events = CValue::Arr(events.iter().map(Event::to_value).collect());
            ApiReply::DryRun { result: CValue::Obj(BTreeMap::from([("events".to_string(), events)])) }
        }
        CommandResult::WouldFile { command } => ApiReply::DryRun {
            result: CValue::Obj(BTreeMap::from([("would_file".to_string(), CValue::Str(command.to_string()))])),
        },
    }
}

impl LocalPort {
    fn chan(&self) -> Result<&LocalChannel, BusError> {
        self.channel.as_ref().ok_or_else(|| invariant("the port is closed"))
    }

    fn kind(&self) -> PortKind {
        self.channel.as_ref().map_or(PortKind::Cli, |c| c.kind)
    }

    /// Moves the port to `bus` (REQ-010, REQ-011, REQ-015). A pending upgrade is committed
    /// on `bus`'s upgrade journal first, and the port switches only once that succeeded; if
    /// the commit fails, the pending upgrade is dropped, the port stays `script` on its
    /// current bus and the commit error is returned. A port holding a grant moves only to a
    /// bus whose upgrade journal has the same [`JournalId`], else `precondition/false` with
    /// detail [`OTHER_JOURNAL`], staying where it is. Errors report stage `session`.
    pub fn attach(&mut self, bus: Arc<LocalBus>) -> Result<(), ApiError> {
        self.attach_to(bus).map_err(|e| self.host.api_error(&e, ApiStage::Session))
    }

    fn attach_to(&mut self, bus: Arc<LocalBus>) -> Result<(), BusError> {
        if let Some(confirmed) = self.pending.take() {
            match &bus.upgrades {
                Some(journal) => self.commit(confirmed, journal)?,
                None => self.pending = Some(confirmed),
            }
        } else if let Some(recorded) = &self.recorded_on {
            if bus.upgrade_journal() != Some(recorded) {
                return Err(BusError::Precondition(PreconditionReason::False, Detail::new(OTHER_JOURNAL)));
            }
        }
        self.bus = bus;
        Ok(())
    }

    /// Whether the port holds a confirmed upgrade that no journal has recorded yet (read by
    /// the composition root's raw-path rule).
    pub fn has_pending_upgrade(&self) -> bool {
        self.pending.is_some()
    }

    /// Closes the port: ends its session, revoking its grant (REQ-013).
    pub fn close(self) {}

    /// The three upgrade steps (module docs).
    fn upgrade(&mut self) -> Result<(), BusError> {
        let pending = {
            let table = self.host.read()?;
            table.prepare_upgrade(self.chan()?)?
        };
        let Some(presence) = self.host.presence.clone() else {
            return Err(BusError::UpgradeRefused(UpgradeRefusedReason::Declined, Detail::new(NO_PRESENCE_PROVIDER)));
        };
        let confirmed = pending.confirm(presence.as_ref(), &mut HostClock(&self.host.clock))?;
        let bus = Arc::clone(&self.bus);
        match &bus.upgrades {
            Some(journal) => self.commit(confirmed, journal),
            None => {
                self.pending = Some(confirmed);
                Ok(())
            }
        }
    }

    /// Records `confirmed` on `journal` and keeps the grant (write lock, then the journal's
    /// append lock). On error nothing changes and the port stays as it was.
    fn commit(&mut self, confirmed: ConfirmedUpgrade, journal: &UpgradeJournal) -> Result<(), BusError> {
        let grant = {
            let mut table = self.host.write()?;
            let mut append = journal.append.lock().map_err(|_| invariant("the upgrade journal lock is poisoned"))?;
            table.commit_upgrade(confirmed, &mut **append, &journal.ctx)?
        };
        let channel = self.channel.as_mut().ok_or_else(|| invariant("the port is closed"))?;
        channel.grant = Some(grant);
        self.recorded_on = Some(journal.id.clone());
        Ok(())
    }

    /// One call (REQ-006): the bus's inputs, then one [`Bus::execute`] under the read lock.
    fn run(&self, call: &BusCall) -> Result<ApiReply, (ApiStage, BusError)> {
        let session = |e: BusError| (ApiStage::Session, e);
        let channel = self.chan().map_err(session)?;
        let (view, roots) = self.bus.parts.inputs(channel).map_err(session)?;
        let mut inv = Invocation::new(call.command.clone(), call.params.clone());
        inv.dry_run = call.dry_run;
        let table = self.host.read().map_err(session)?;
        let call = Call { sessions: &table, channel, view: &*view, roots: &*roots };
        let result = self.bus.bus.execute_staged(&call, &inv).map_err(|(stage, e)| (api_stage(stage), e))?;
        Ok(reply(result))
    }
}

impl BusPort for LocalPort {
    fn channel(&self) -> ChannelInfo {
        ChannelInfo {
            kind: self.kind(),
            identity: self.host.os_user.as_str().to_string(),
            session: self.channel.as_ref().map(|c| c.key.journal_id()).unwrap_or_default(),
        }
    }

    /// What [`SessionTable::derive_principal`] derives for the port's channel now (REQ-005).
    /// If it cannot derive (a poisoned lock), the kind before any upgrade, which is never
    /// `human` for a port.
    fn principal_kind(&self) -> PrincipalKind {
        let base = channel_kind(self.kind()).base_kind();
        let Some(channel) = &self.channel else { return base };
        match self.host.read() {
            Ok(table) => table.derive_principal(channel).map_or(base, |p| p.kind()),
            Err(_) => base,
        }
    }

    fn confirm_human(&mut self) -> Result<(), ApiError> {
        self.upgrade().map_err(|e| self.host.api_error(&e, ApiStage::Session))
    }

    fn call(&mut self, call: &BusCall) -> ApiReply {
        match self.run(call) {
            Ok(reply) => reply,
            Err((stage, e)) => ApiReply::Failed(self.host.api_error(&e, stage)),
        }
    }
}

impl Drop for LocalPort {
    fn drop(&mut self) {
        self.pending = None;
        if let Some(LocalChannel { key, .. }) = self.channel.take() {
            let mut table = self.host.sessions.write().unwrap_or_else(PoisonError::into_inner);
            table.end_session(key);
        }
    }
}

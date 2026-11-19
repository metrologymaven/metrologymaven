//! Principal model, channel binding and human session upgrade. Spec node `CORE.BUS.PRIN`.
//!
//! A [`Principal`] is derived only from the authenticated [`Channel`] a call arrives on,
//! never from the request parameters (REQ-002): [`SessionTable::derive_principal`] takes
//! no parameters at all. The mapping from [`ChannelKind`] to [`PrincipalKind`] is an
//! exhaustive `match` with no wildcard arm (REQ-013); the module denies
//! `clippy::wildcard_enum_match_arm`, so adding a channel kind fails to build until it is
//! mapped.
//!
//! A CLI or Python session becomes `human` only through [`SessionTable::upgrade_session`]:
//! a person confirms through a [`HumanPresence`] provider within [`UPGRADE_DEADLINE`], the
//! confirming OS user equals the session's OS user, and one `principal_upgrade` entry is
//! appended through [`JournalAppend`] before the session is inserted (REQ-009..011). The
//! table lives in memory only; a new session or a restart starts as `script`.

#![deny(clippy::wildcard_enum_match_arm, clippy::match_wildcard_for_single_variants)]

use crate::error::{BusError, Detail, InternalReason, UnauthenticatedReason, Untrusted, UpgradeRefusedReason};
use fid_canon::CValue;
use fid_journal::{AiAssistance, DraftBody, EntryDraft, EntryTime, Manifests, PrincipalDraft, UpgradeDraft};
use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

pub use fid_journal::{ConfirmMethod, PrincipalKind};

/// How long a person has to confirm an upgrade (REQ-010).
pub const UPGRADE_DEADLINE: Duration = Duration::from_secs(120);
/// Longest identifier ([`PrincipalId`], [`ClientId`], [`OsUser`], [`SessionId`]), in bytes.
pub const MAX_ID_BYTES: usize = 256;
/// Longest self-asserted client or model name, in characters; longer text is cut.
pub const MAX_SELF_ASSERTED_CHARS: usize = 256;
/// Most sessions one [`SessionTable`] holds as upgraded at the same time.
pub const MAX_UPGRADED_SESSIONS: usize = 4096;

// ------------------------------------------------------------------ identifiers

/// Why an identifier was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdError {
    /// The identifier is empty.
    Empty,
    /// The identifier is longer than [`MAX_ID_BYTES`] bytes.
    TooLong,
    /// The identifier holds a control character.
    ControlChar,
}

impl IdError {
    /// Stable machine-readable code.
    pub fn code(self) -> &'static str {
        match self {
            IdError::Empty => "empty",
            IdError::TooLong => "too_long",
            IdError::ControlChar => "control_char",
        }
    }
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for IdError {}

fn check_id(s: &str) -> Result<(), IdError> {
    if s.is_empty() {
        Err(IdError::Empty)
    } else if s.len() > MAX_ID_BYTES {
        Err(IdError::TooLong)
    } else if s.chars().any(char::is_control) {
        Err(IdError::ControlChar)
    } else {
        Ok(())
    }
}

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Checks and wraps `s`: 1 to [`MAX_ID_BYTES`] bytes, no control characters.
            /// Compared byte for byte; case is significant.
            pub fn new(s: &str) -> Result<Self, IdError> {
                check_id(s)?;
                Ok($name(s.to_string()))
            }

            /// The identifier text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(
    /// An authenticated principal identity, as the channel's authentication supplied it.
    PrincipalId
);
id_type!(
    /// An authenticated client identifier (for example a registered MCP client).
    ClientId
);
id_type!(
    /// An operating-system account name.
    OsUser
);
id_type!(
    /// A channel session, as the surface that owns the session names it.
    SessionId
);

impl From<&OsUser> for PrincipalId {
    /// An OS user as a principal identity: the same text (REQ-009).
    fn from(user: &OsUser) -> Self {
        PrincipalId(user.0.clone())
    }
}

// ------------------------------------------------------------------ self-asserted data

fn cut(text: &str) -> String {
    match text.char_indices().nth(MAX_SELF_ASSERTED_CHARS) {
        Some((at, _)) => text.get(..at).unwrap_or("").to_string(),
        None => text.to_string(),
    }
}

/// Client and model names a client states about itself (for example an MCP `clientInfo`).
/// Nothing checks them; they are kept for display and the journal only and are never used
/// in an authorisation decision (REQ-007, CONSTITUTION C6). Always wrapped in
/// [`Untrusted`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SelfAsserted {
    client_name: Option<String>,
    model_name: Option<String>,
}

impl SelfAsserted {
    /// Wraps the stated names as untrusted text; each is cut to
    /// [`MAX_SELF_ASSERTED_CHARS`] characters. Never fails.
    pub fn untrusted(client_name: Option<&str>, model_name: Option<&str>) -> Untrusted<SelfAsserted> {
        Untrusted::new(SelfAsserted { client_name: client_name.map(cut), model_name: model_name.map(cut) })
    }

    /// The stated client name, for display only.
    pub fn client_name(&self) -> Option<&str> {
        self.client_name.as_deref()
    }

    /// The stated model name, for display only.
    pub fn model_name(&self) -> Option<&str> {
        self.model_name.as_deref()
    }
}

// ------------------------------------------------------------------ channels

/// The kind of channel a call arrives on. Closed; every kind is mapped in
/// [`ChannelKind::base_kind`] and [`SessionTable::derive_principal`] without a wildcard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChannelKind {
    /// A GUI widget of a desktop app in an interactive OS session.
    Gui,
    /// The `fid` command line.
    Cli,
    /// The Python host.
    Python,
    /// The loopback control channel.
    Control,
    /// A local MCP server.
    Mcp,
    /// The self-hosted server's MCP endpoint.
    ServerMcp,
    /// A session on the self-hosted server (web client, API token of a signed-in user).
    ServerSession,
    /// A service credential on the self-hosted server.
    ServerService,
    /// A local service credential (a daemon or scheduled task with its own credential).
    Service,
    /// A device link.
    Device,
    /// A call that originates inside the core (recompute, replay, migration).
    Internal,
}

impl ChannelKind {
    /// Every channel kind, in declaration order.
    pub const ALL: &'static [ChannelKind] = &[
        ChannelKind::Gui,
        ChannelKind::Cli,
        ChannelKind::Python,
        ChannelKind::Control,
        ChannelKind::Mcp,
        ChannelKind::ServerMcp,
        ChannelKind::ServerSession,
        ChannelKind::ServerService,
        ChannelKind::Service,
        ChannelKind::Device,
        ChannelKind::Internal,
    ];

    /// The wire string.
    pub const fn as_str(self) -> &'static str {
        match self {
            ChannelKind::Gui => "gui",
            ChannelKind::Cli => "cli",
            ChannelKind::Python => "python",
            ChannelKind::Control => "control",
            ChannelKind::Mcp => "mcp",
            ChannelKind::ServerMcp => "server_mcp",
            ChannelKind::ServerSession => "server_session",
            ChannelKind::ServerService => "server_service",
            ChannelKind::Service => "service",
            ChannelKind::Device => "device",
            ChannelKind::Internal => "internal",
        }
    }

    /// Decodes a wire string; `None` if unknown.
    pub fn from_wire(s: &str) -> Option<ChannelKind> {
        ChannelKind::ALL.iter().copied().find(|k| k.as_str() == s)
    }

    /// The principal kind of a call on this channel before any session upgrade
    /// (CONSTITUTION §3, D-012 option C, D-030 option B). Total, no default case (REQ-013).
    pub const fn base_kind(self) -> PrincipalKind {
        match self {
            ChannelKind::Gui => PrincipalKind::Human,
            ChannelKind::Cli => PrincipalKind::Script,
            ChannelKind::Python => PrincipalKind::Script,
            ChannelKind::Control => PrincipalKind::Script,
            ChannelKind::Mcp => PrincipalKind::Agent,
            ChannelKind::ServerMcp => PrincipalKind::Agent,
            ChannelKind::ServerSession => PrincipalKind::Script,
            ChannelKind::ServerService => PrincipalKind::Service,
            ChannelKind::Service => PrincipalKind::Service,
            ChannelKind::Device => PrincipalKind::Device,
            ChannelKind::Internal => PrincipalKind::System,
        }
    }

    /// Whether a session on this channel may be upgraded to `human` (REQ-009, REQ-010).
    pub const fn upgradable(self) -> bool {
        match self {
            ChannelKind::Cli | ChannelKind::Python => true,
            ChannelKind::Gui
            | ChannelKind::Control
            | ChannelKind::Mcp
            | ChannelKind::ServerMcp
            | ChannelKind::ServerSession
            | ChannelKind::ServerService
            | ChannelKind::Service
            | ChannelKind::Device
            | ChannelKind::Internal => false,
        }
    }
}

impl fmt::Display for ChannelKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a channel's authentication established about the caller. Only the surface's
/// authentication fills it; request parameters never do (REQ-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The authenticated subject: the OS user (GUI, CLI, Python, control), the client
    /// credential (MCP), the server session (server session), the service credential, the
    /// device, or the core component (internal).
    pub subject: PrincipalId,
    /// The OS account that owns the local process, when the channel knows it. The session
    /// upgrade compares the confirming person against it.
    pub os_user: Option<OsUser>,
    /// Who the caller acts for. Read only on `server_session` (the authenticated server
    /// user) and `internal` (the triggering principal), where it is required; ignored on
    /// every other channel.
    pub acting_for: Option<PrincipalId>,
    /// The authenticated client identifier, if the channel authenticates clients.
    pub client_id: Option<ClientId>,
    /// What the client says about itself; untrusted.
    pub self_asserted: Option<Untrusted<SelfAsserted>>,
}

impl Identity {
    /// An identity with only a subject.
    pub fn new(subject: PrincipalId) -> Self {
        Identity { subject, os_user: None, acting_for: None, client_id: None, self_asserted: None }
    }
}

/// A surface's authenticated channel. Each surface implements it; the bus reads nothing else.
pub trait Channel {
    /// The channel kind.
    fn kind(&self) -> ChannelKind;
    /// The authenticated identity, or `None` if the channel could not authenticate the caller.
    fn authenticated_identity(&self) -> Option<Identity>;
    /// The channel's session.
    fn session(&self) -> SessionId;
}

// ------------------------------------------------------------------ principal

/// Who is acting on one call. Built only by [`SessionTable::derive_principal`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    kind: PrincipalKind,
    id: PrincipalId,
    on_behalf_of: Option<PrincipalId>,
    client_id: Option<ClientId>,
    self_asserted: Option<Untrusted<SelfAsserted>>,
    upgraded_session: Option<SessionId>,
}

impl Principal {
    /// The principal kind.
    pub fn kind(&self) -> PrincipalKind {
        self.kind
    }

    /// The authenticated identity.
    pub fn id(&self) -> &PrincipalId {
        &self.id
    }

    /// The principal this one acts for (server user, or the trigger of an internal call).
    pub fn on_behalf_of(&self) -> Option<&PrincipalId> {
        self.on_behalf_of.as_ref()
    }

    /// The authenticated client identifier.
    pub fn client_id(&self) -> Option<&ClientId> {
        self.client_id.as_ref()
    }

    /// What the client says about itself; display and journal only.
    pub fn self_asserted(&self) -> Option<&Untrusted<SelfAsserted>> {
        self.self_asserted.as_ref()
    }

    /// The upgraded session this `human` principal comes from, if any.
    pub fn upgraded_session(&self) -> Option<&SessionId> {
        self.upgraded_session.as_ref()
    }

    /// The principal as a journal draft: kind, identity and `on_behalf_of` as JSON strings.
    pub fn to_draft(&self) -> PrincipalDraft {
        PrincipalDraft {
            kind: self.kind,
            identity: CValue::Str(self.id.as_str().to_string()),
            on_behalf_of: self.on_behalf_of.as_ref().map(|p| CValue::Str(p.as_str().to_string())),
        }
    }
}

// ------------------------------------------------------------------ presence and journal

/// What the presence provider is asked to confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeRequest {
    pub session: SessionId,
    pub channel: ChannelKind,
    pub subject: PrincipalId,
    /// The session's OS user; the person confirming must be this user.
    pub os_user: OsUser,
}

/// The result of a presence check. An observed input: it is recorded in the journal
/// entry, so replay reads it and never prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresenceOutcome {
    /// A person confirmed, as `os_user`, through `method`.
    Confirmed { os_user: OsUser, method: ConfirmMethod },
    /// The person declined.
    Declined,
    /// Nobody answered before the deadline.
    TimedOut,
}

/// Asks a person to confirm a session upgrade: a prompt in the running desktop app, or an
/// OS user-presence check when headless. Implemented by the apps and `PLAT`.
pub trait HumanPresence {
    /// Asks for confirmation; must give up after `deadline`.
    fn confirm(&self, req: &UpgradeRequest, deadline: Duration) -> PresenceOutcome;
}

/// Appends one entry to the Record Journal (`CORE.JRN.CHAIN.APPEND` implements it).
pub trait JournalAppend {
    /// Appends `draft`; an error means nothing was appended.
    fn append(&mut self, draft: &EntryDraft) -> Result<(), BusError>;
}

/// The host clock used for the upgrade entry's time and for the deadline check.
pub trait UpgradeClock {
    /// The current time: UTC, monotonic nanoseconds and sync state.
    fn now(&mut self) -> EntryTime;
}

/// Where the `principal_upgrade` entry goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeContext {
    pub stream: String,
    pub branch: String,
    pub manifests: Manifests,
}

// ------------------------------------------------------------------ session table

#[derive(Debug, Clone, PartialEq, Eq)]
struct Upgraded {
    channel: ChannelKind,
    os_user: OsUser,
}

fn unauthenticated(detail: &str) -> BusError {
    BusError::Unauthenticated(UnauthenticatedReason::NoIdentity, Detail::new(detail))
}

fn refused(reason: UpgradeRefusedReason, detail: &str) -> BusError {
    BusError::UpgradeRefused(reason, Detail::new(detail))
}

/// Upgraded sessions, in memory only; never persisted, so a restart starts as `script`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionTable {
    rows: BTreeMap<SessionId, Upgraded>,
}

impl SessionTable {
    /// An empty table.
    pub fn new() -> Self {
        SessionTable::default()
    }

    /// Number of upgraded sessions.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no session is upgraded.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Whether `session` is upgraded.
    pub fn is_upgraded(&self, session: &SessionId) -> bool {
        self.rows.contains_key(session)
    }

    /// Ends a session (its channel closed): removes its row. Returns whether it was upgraded.
    pub fn end_session(&mut self, session: &SessionId) -> bool {
        self.rows.remove(session).is_some()
    }

    /// The confirmed OS user of an upgraded session, if the call comes on the same channel
    /// kind and from the same OS user as the confirmed one.
    fn upgraded_user(&self, kind: ChannelKind, session: &SessionId, identity: &Identity) -> Option<&OsUser> {
        let row = self.rows.get(session)?;
        (row.channel == kind && identity.os_user.as_ref() == Some(&row.os_user)).then_some(&row.os_user)
    }

    /// Derives the principal of a call from its channel alone (REQ-002..008, REQ-012).
    pub fn derive_principal(&self, channel: &dyn Channel) -> Result<Principal, BusError> {
        let Some(identity) = channel.authenticated_identity() else {
            return Err(unauthenticated("channel supplied no authenticated identity"));
        };
        let kind = channel.kind();
        let mut p = Principal {
            kind: kind.base_kind(),
            id: identity.subject.clone(),
            on_behalf_of: None,
            client_id: identity.client_id.clone(),
            self_asserted: identity.self_asserted.clone(),
            upgraded_session: None,
        };
        match kind {
            ChannelKind::Cli | ChannelKind::Python => {
                let session = channel.session();
                if let Some(user) = self.upgraded_user(kind, &session, &identity) {
                    p.kind = PrincipalKind::Human;
                    p.id = PrincipalId::from(user);
                    p.upgraded_session = Some(session);
                }
            }
            ChannelKind::ServerSession | ChannelKind::Internal => match identity.acting_for {
                Some(behalf) => p.on_behalf_of = Some(behalf),
                None => return Err(unauthenticated("channel supplied no principal it acts for")),
            },
            ChannelKind::Gui
            | ChannelKind::Control
            | ChannelKind::Mcp
            | ChannelKind::ServerMcp
            | ChannelKind::ServerService
            | ChannelKind::Service
            | ChannelKind::Device => {}
        }
        Ok(p)
    }

    /// Upgrades a CLI or Python session to `human` once a person confirms through
    /// `presence` (REQ-009..011). On success one `principal_upgrade` entry has been appended
    /// to `journal` and every later call of the session derives as `human` until
    /// [`SessionTable::end_session`]. On any error the table and the journal are unchanged.
    /// An already upgraded session returns `Ok` without prompting or appending.
    pub fn upgrade_session(
        &mut self,
        channel: &dyn Channel,
        presence: &dyn HumanPresence,
        journal: &mut dyn JournalAppend,
        clock: &mut dyn UpgradeClock,
        ctx: &UpgradeContext,
    ) -> Result<(), BusError> {
        // Authentication first: an incomplete identity is `unauthenticated` on every channel.
        let before = self.derive_principal(channel)?;
        let kind = channel.kind();
        if !kind.upgradable() {
            return Err(refused(UpgradeRefusedReason::Channel, "only cli and python sessions can be upgraded"));
        }
        if before.kind == PrincipalKind::Human {
            return Ok(());
        }
        let Some(identity) = channel.authenticated_identity() else {
            return Err(unauthenticated("channel supplied no authenticated identity"));
        };
        let Some(os_user) = identity.os_user.clone() else {
            return Err(refused(UpgradeRefusedReason::UserMismatch, "session has no OS user"));
        };
        let session = channel.session();
        if self.rows.len() >= MAX_UPGRADED_SESSIONS {
            return Err(BusError::Internal(InternalReason::Storage, Detail::new("upgraded-session table is full")));
        }
        let req = UpgradeRequest { session: session.clone(), channel: kind, subject: identity.subject, os_user };
        let start = clock.now().mono_ns;
        let outcome = presence.confirm(&req, UPGRADE_DEADLINE);
        let method = match outcome {
            PresenceOutcome::Declined => return Err(refused(UpgradeRefusedReason::Declined, "confirmation declined")),
            PresenceOutcome::TimedOut => return Err(refused(UpgradeRefusedReason::Timeout, "confirmation timed out")),
            PresenceOutcome::Confirmed { os_user: who, method } => {
                if who != req.os_user {
                    return Err(refused(UpgradeRefusedReason::UserMismatch, "confirming OS user differs"));
                }
                method
            }
        };
        let time = clock.now();
        let in_time = time.mono_ns.checked_sub(start).is_some_and(|ns| u128::from(ns) <= UPGRADE_DEADLINE.as_nanos());
        if !in_time {
            return Err(refused(UpgradeRefusedReason::Timeout, "confirmation arrived after the deadline"));
        }
        let draft = EntryDraft {
            stream: ctx.stream.clone(),
            branch: ctx.branch.clone(),
            logical_parent: None,
            principal: before.to_draft(),
            ai_assistance: AiAssistance::None,
            manifests: ctx.manifests,
            reason: None,
            human_assertion: None,
            body: DraftBody::PrincipalUpgrade(UpgradeDraft {
                session_id: session.as_str().to_string(),
                method,
                identity: CValue::Str(req.os_user.as_str().to_string()),
                confirmed_at: time.utc.clone(),
            }),
            time,
        };
        journal.append(&draft)?;
        self.rows.insert(session, Upgraded { channel: kind, os_user: req.os_user });
        Ok(())
    }
}

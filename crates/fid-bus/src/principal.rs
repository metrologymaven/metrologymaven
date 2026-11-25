//! Principal model, channel binding and human session upgrade. Spec node `CORE.BUS.PRIN`.
//!
//! A [`Principal`] is derived only from the authenticated [`Channel`] a call arrives on,
//! never from the request parameters (REQ-002): [`SessionTable::derive_principal`] takes
//! no parameters at all. The mapping from [`ChannelKind`] to [`PrincipalKind`] is an
//! exhaustive `match` with no wildcard arm (REQ-013); the module denies
//! `clippy::wildcard_enum_match_arm`, so adding a channel kind fails to build until it is
//! mapped.
//!
//! Sessions are named by the bus, not by the client: [`SessionTable::open_session`] issues
//! a [`SessionKey`] per authenticated connection, numbered by an internal monotonic `u64`
//! that is never reused. A CLI or Python session becomes `human` only through an upgrade
//! in three steps: [`SessionTable::prepare_upgrade`] (`&self`, no person asked) returns a
//! [`PendingUpgrade`] that borrows nothing; [`PendingUpgrade::confirm`] asks a person through
//! a [`HumanPresence`] provider within [`UPGRADE_DEADLINE`] while the table stays free; and
//! [`SessionTable::commit_upgrade`] (`&mut self`) re-checks the session row's generation,
//! appends one `principal_upgrade` entry through [`JournalAppend`] (REQ-009..011) and
//! issues the grant. The upgrade is returned as an [`UpgradeGrant`]: not `Clone`, private
//! fields, only built inside this crate. A call derives as `human` only when its channel
//! presents that grant together with the session key it was issued for. Dropping the
//! grant or [`SessionTable::end_session`] revokes it. The table lives in memory only; a new
//! session or a restart starts as `script`.

#![deny(clippy::wildcard_enum_match_arm, clippy::match_wildcard_for_single_variants)]

use crate::error::{
    BusError, ConflictReason, Detail, InternalReason, UnauthenticatedReason, Untrusted, UpgradeRefusedReason,
};
use fid_canon::CValue;
use fid_journal::{AiAssistance, DraftBody, EntryDraft, EntryTime, Manifests, PrincipalDraft, UpgradeDraft};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use unicode_normalization::is_nfc;

pub use fid_journal::ConfirmMethod;
pub use fid_types::PrincipalKind;

/// How long a person has to confirm an upgrade (REQ-010).
pub const UPGRADE_DEADLINE: Duration = Duration::from_secs(120);
/// Longest identifier ([`PrincipalId`], [`ClientId`], [`OsUser`], [`SessionId`]), in bytes.
pub const MAX_ID_BYTES: usize = 256;
/// Longest self-asserted client or model name, in characters; longer text is cut.
pub const MAX_SELF_ASSERTED_CHARS: usize = 256;
/// Most live upgrade grants one [`SessionTable`] holds at the same time.
pub const MAX_UPGRADED_SESSIONS: usize = 4096;

// ------------------------------------------------------------------ identifiers

/// Why an identifier was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdError {
    /// The identifier is empty.
    Empty,
    /// The identifier is longer than [`MAX_ID_BYTES`] bytes.
    TooLong,
    /// A control character (general category Cc).
    ControlChar,
    /// A format character (Cf): bidi overrides and isolates, zero-width characters, BOM, tags.
    FormatChar,
    /// A line or paragraph separator (Zl, Zp).
    Separator,
    /// A private-use character (Co). Surrogates (Cs) cannot occur in Rust strings.
    PrivateUse,
    /// A Default_Ignorable_Code_Point that is not Cf: variation selectors, the combining
    /// grapheme joiner, Hangul fillers and the reserved ignorable ranges.
    DefaultIgnorable,
    /// An unassigned code point (Cn, noncharacters included), Unicode 16.0.
    Unassigned,
    /// A blank other than U+0020 SPACE (see [`BLANK_CHARS`]).
    BlankChar,
    /// The identifier is not in Unicode Normalization Form C.
    NotNfc,
}

impl IdError {
    /// Stable machine-readable code.
    pub fn code(self) -> &'static str {
        match self {
            IdError::Empty => "empty",
            IdError::TooLong => "too_long",
            IdError::ControlChar => "control_char",
            IdError::FormatChar => "format_char",
            IdError::Separator => "separator",
            IdError::PrivateUse => "private_use",
            IdError::DefaultIgnorable => "default_ignorable",
            IdError::Unassigned => "unassigned",
            IdError::BlankChar => "blank_char",
            IdError::NotNfc => "not_nfc",
        }
    }
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for IdError {}

pub use crate::unicode16::{DEFAULT_IGNORABLE_RANGES, FORMAT_RANGES, UNASSIGNED_RANGES};

/// Code points of general category Co (private use), as inclusive ranges.
pub const PRIVATE_USE_RANGES: &[(u32, u32)] = &[(0xE000, 0xF8FF), (0xF0000, 0xFFFFD), (0x10_0000, 0x10_FFFD)];

/// Blanks that are not printable-distinct from U+0020 SPACE or from nothing: every Zs
/// character except U+0020 (Unicode 16.0: no-break space, Ogham space mark, the en/em and
/// other fixed-width spaces, narrow no-break space, medium mathematical space, ideographic
/// space), plus U+2800 BRAILLE PATTERN BLANK (So). Hangul fillers are default-ignorable.
pub const BLANK_CHARS: &[(u32, u32)] = &[
    (0x00A0, 0x00A0),
    (0x1680, 0x1680),
    (0x2000, 0x200A),
    (0x202F, 0x202F),
    (0x205F, 0x205F),
    (0x2800, 0x2800),
    (0x3000, 0x3000),
];

/// Whether `u` lies in one of the sorted, non-overlapping inclusive `ranges`.
fn in_ranges(ranges: &[(u32, u32)], u: u32) -> bool {
    ranges
        .binary_search_by(|&(lo, hi)| {
            if hi < u {
                std::cmp::Ordering::Less
            } else if lo > u {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

pub(crate) fn forbidden(c: char) -> Option<IdError> {
    let u = u32::from(c);
    if c.is_control() {
        Some(IdError::ControlChar)
    } else if u == 0x2028 || u == 0x2029 {
        Some(IdError::Separator)
    } else if in_ranges(PRIVATE_USE_RANGES, u) {
        Some(IdError::PrivateUse)
    } else if in_ranges(FORMAT_RANGES, u) {
        Some(IdError::FormatChar)
    } else if in_ranges(DEFAULT_IGNORABLE_RANGES, u) {
        Some(IdError::DefaultIgnorable)
    } else if in_ranges(UNASSIGNED_RANGES, u) {
        Some(IdError::Unassigned)
    } else if in_ranges(BLANK_CHARS, u) {
        Some(IdError::BlankChar)
    } else {
        None
    }
}

fn check_id(s: &str) -> Result<(), IdError> {
    if s.is_empty() {
        return Err(IdError::Empty);
    }
    if s.len() > MAX_ID_BYTES {
        return Err(IdError::TooLong);
    }
    if let Some(e) = s.chars().find_map(forbidden) {
        return Err(e);
    }
    if !is_nfc(s) {
        return Err(IdError::NotNfc);
    }
    Ok(())
}

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Checks and wraps `s`: 1 to [`MAX_ID_BYTES`] bytes, NFC, and no character of
            /// general category Cc, Zl, Zp, Co, Cf or Cn, no Default_Ignorable_Code_Point and
            /// no blank other than U+0020 (Unicode 16.0). Compared byte for byte.
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
    /// An operating-system account name, in the canonical spelling the platform reports.
    OsUser
);
id_type!(
    /// The surface's label for a session, for display and the journal. It grants nothing:
    /// the bus-issued [`SessionKey`] is the session's identity.
    SessionId
);

impl From<&OsUser> for PrincipalId {
    /// An OS user as a principal identity: the same text (REQ-009).
    fn from(user: &OsUser) -> Self {
        PrincipalId(user.0.clone())
    }
}

/// A principal named by kind and identity, so that `alice` as `script` and `alice` as
/// `human` stay distinguishable in `on_behalf_of`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrincipalRef {
    pub kind: PrincipalKind,
    pub id: PrincipalId,
}

impl PrincipalRef {
    /// `{"id": <id>, "kind": <kind>}`, the journal form.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("id".to_string(), CValue::Str(self.id.as_str().to_string()));
        o.insert("kind".to_string(), CValue::Str(self.kind.as_str().to_string()));
        CValue::Obj(o)
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
    /// Who the caller acts for, with kind. Read only on `server_session` (the authenticated
    /// server user) and `internal` (the triggering principal), where it is required;
    /// ignored on every other channel.
    pub acting_for: Option<PrincipalRef>,
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

/// The bus's name for one authenticated connection, issued by
/// [`SessionTable::open_session`]. Its number is an internal monotonic `u64`, never reused
/// and never taken from the client. Not `Clone`; private fields. [`SessionTable::end_session`]
/// consumes it.
#[derive(Debug, PartialEq, Eq)]
pub struct SessionKey {
    table: u64,
    number: u64,
    label: SessionId,
    /// The table holds a weak reference: dropping the key closes the session's row.
    open: Arc<()>,
}

impl SessionKey {
    /// The bus-issued session number.
    pub fn number(&self) -> u64 {
        self.number
    }

    /// The number of the [`SessionTable`] that issued this key; with [`SessionKey::number`]
    /// it names the session uniquely within the process (CORE.BUS.ROUTE keys open branches
    /// by both).
    pub(crate) fn table_id(&self) -> u64 {
        self.table
    }

    /// The surface's label, for display.
    pub fn label(&self) -> &SessionId {
        &self.label
    }

    /// The session ID written to the journal: `<label>#<number>`.
    pub fn journal_id(&self) -> String {
        format!("{}#{}", self.label, self.number)
    }
}

/// Proof that a person confirmed the upgrade of one session (REQ-009). Returned by
/// [`SessionTable::commit_upgrade`]; not `Clone`, private fields, and only this crate can
/// build one. The channel presents it through [`Channel::upgrade_grant`]. Dropping it, a
/// newer grant for the same session, or [`SessionTable::end_session`] revokes it.
#[derive(Debug)]
pub struct UpgradeGrant {
    table: u64,
    number: u64,
    alive: Arc<u64>,
}

impl UpgradeGrant {
    /// The number of the session this grant was issued for.
    pub fn session_number(&self) -> u64 {
        self.number
    }
}

/// A surface's authenticated channel. Each surface implements it; the bus reads nothing else.
pub trait Channel {
    /// The channel kind.
    fn kind(&self) -> ChannelKind;
    /// The authenticated identity, or `None` if the channel could not authenticate the caller.
    fn authenticated_identity(&self) -> Option<Identity>;
    /// The bus-issued key of this connection.
    fn session(&self) -> &SessionKey;
    /// The upgrade grant this connection holds, if any. Only CLI and Python channels ever
    /// hold one.
    fn upgrade_grant(&self) -> Option<&UpgradeGrant> {
        None
    }
}

// ------------------------------------------------------------------ principal

/// Who is acting on one call. Built only by [`SessionTable::derive_principal`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    kind: PrincipalKind,
    id: PrincipalId,
    on_behalf_of: Option<PrincipalRef>,
    client_id: Option<ClientId>,
    self_asserted: Option<Untrusted<SelfAsserted>>,
    upgraded_session: Option<u64>,
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

    /// Kind and identity, for naming this principal in another's `on_behalf_of`.
    pub fn reference(&self) -> PrincipalRef {
        PrincipalRef { kind: self.kind, id: self.id.clone() }
    }

    /// The principal this one acts for (server user, or the trigger of an internal call).
    pub fn on_behalf_of(&self) -> Option<&PrincipalRef> {
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

    /// The number of the upgraded session this `human` principal comes from, if any.
    pub fn upgraded_session(&self) -> Option<u64> {
        self.upgraded_session
    }

    /// The principal as a journal draft: identity as a JSON string, `on_behalf_of` as
    /// `{"id", "kind"}`.
    pub fn to_draft(&self) -> PrincipalDraft {
        PrincipalDraft {
            kind: self.kind,
            identity: CValue::Str(self.id.as_str().to_string()),
            on_behalf_of: self.on_behalf_of.as_ref().map(PrincipalRef::to_value),
        }
    }
}

// ------------------------------------------------------------------ presence and journal

/// What the presence provider is asked to confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeRequest {
    /// The surface's label of the session.
    pub session: SessionId,
    /// The bus-issued session number.
    pub session_number: u64,
    pub channel: ChannelKind,
    pub subject: PrincipalId,
    /// The session's OS user; the person confirming must be this user.
    pub os_user: OsUser,
}

/// The result of a presence check. An observed input: it is recorded in the journal
/// entry, so replay reads it and never prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresenceOutcome {
    /// A person confirmed, as `os_user` in the platform's canonical spelling, through `method`.
    Confirmed { os_user: OsUser, method: ConfirmMethod },
    /// The person declined.
    Declined,
    /// Nobody answered before the deadline.
    TimedOut,
}

/// Asks a person to confirm a session upgrade: a prompt in the running desktop app, or an
/// OS user-presence check when headless. Implemented by the apps and `PLAT`, which must
/// report OS users in the platform's canonical spelling (see [`OsUser`]).
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

/// A live upgrade grant of one session row.
#[derive(Debug, Clone)]
struct GrantRow {
    channel: ChannelKind,
    os_user: OsUser,
    alive: Weak<u64>,
}

impl GrantRow {
    fn live(&self) -> bool {
        self.alive.strong_count() > 0
    }
}

/// One open session. The row exists from [`SessionTable::open_session`] until
/// [`SessionTable::end_session`] (or until its key is dropped and the row is pruned).
#[derive(Debug, Clone)]
struct Row {
    /// Liveness of the session's key: dead once the key is dropped.
    open: Weak<()>,
    /// Bumped by every committed upgrade; a pending upgrade commits only if it is unchanged.
    generation: u64,
    grant: Option<GrantRow>,
}

impl Row {
    fn open(&self) -> bool {
        self.open.strong_count() > 0
    }

    fn granted(&self) -> bool {
        self.open() && self.grant.as_ref().is_some_and(GrantRow::live)
    }
}

fn unauthenticated(detail: &str) -> BusError {
    BusError::Unauthenticated(UnauthenticatedReason::NoIdentity, Detail::new(detail))
}

fn refused(reason: UpgradeRefusedReason, detail: &str) -> BusError {
    BusError::UpgradeRefused(reason, Detail::new(detail))
}

fn invariant(detail: &str) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, Detail::new(detail))
}

fn table_full() -> BusError {
    BusError::Internal(InternalReason::Storage, Detail::new("upgraded-session table is full"))
}

/// Process-wide table numbers, so a key or grant of one table never works in another.
static NEXT_TABLE: AtomicU64 = AtomicU64::new(1);

/// Rows are pruned of dropped keys once the table holds at least this many.
const PRUNE_FLOOR: usize = 64;

/// An upgrade that passed every check that needs no person, returned by
/// [`SessionTable::prepare_upgrade`]. It borrows nothing, so the table stays usable while
/// the person is asked through [`PendingUpgrade::confirm`]. Not `Clone`; private fields.
#[derive(Debug)]
pub struct PendingUpgrade {
    table: u64,
    number: u64,
    generation: u64,
    journal_id: String,
    channel: ChannelKind,
    request: UpgradeRequest,
    before: PrincipalDraft,
}

impl PendingUpgrade {
    /// What the person will be asked to confirm.
    pub fn request(&self) -> &UpgradeRequest {
        &self.request
    }

    /// Asks the person through `presence` with the [`UPGRADE_DEADLINE`]; holds no table
    /// borrow. Refuses a decline, a timeout, an answer later than the deadline by `clock`
    /// (or a clock running backwards) and a confirming OS user that differs from the
    /// session's. Nothing is recorded until [`SessionTable::commit_upgrade`].
    pub fn confirm(
        self,
        presence: &dyn HumanPresence,
        clock: &mut dyn UpgradeClock,
    ) -> Result<ConfirmedUpgrade, BusError> {
        let start = clock.now().mono_ns;
        let outcome = presence.confirm(&self.request, UPGRADE_DEADLINE);
        let method = match outcome {
            PresenceOutcome::Declined => return Err(refused(UpgradeRefusedReason::Declined, "confirmation declined")),
            PresenceOutcome::TimedOut => return Err(refused(UpgradeRefusedReason::Timeout, "confirmation timed out")),
            PresenceOutcome::Confirmed { os_user: who, method } => {
                if who != self.request.os_user {
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
        Ok(ConfirmedUpgrade { pending: self, method, time })
    }
}

/// A person's confirmation of a [`PendingUpgrade`], ready for
/// [`SessionTable::commit_upgrade`]. Not `Clone`; private fields; only this crate builds one.
#[derive(Debug)]
pub struct ConfirmedUpgrade {
    pending: PendingUpgrade,
    method: ConfirmMethod,
    time: EntryTime,
}

impl ConfirmedUpgrade {
    /// The number of the session this confirmation is for.
    pub fn session_number(&self) -> u64 {
        self.pending.number
    }
}

/// Session keys and upgrade grants, in memory only; never persisted, so a restart starts
/// every session as `script`.
#[derive(Debug)]
pub struct SessionTable {
    /// `None` once the process has used up every table number; such a table opens no session.
    id: Option<u64>,
    next: u64,
    rows: BTreeMap<u64, Row>,
    /// Row count at which `open_session` next prunes rows of dropped keys.
    prune_at: usize,
}

impl Default for SessionTable {
    fn default() -> Self {
        SessionTable::new()
    }
}

impl SessionTable {
    /// An empty table with a process-unique number.
    pub fn new() -> Self {
        // Compare-and-swap rather than `fetch_add`, so the counter never wraps and reuses a
        // number (`fetch_update` is deprecated on newer toolchains).
        let mut current = NEXT_TABLE.load(Ordering::Relaxed);
        let id = loop {
            let Some(next) = current.checked_add(1) else {
                break None;
            };
            match NEXT_TABLE.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break Some(current),
                Err(actual) => current = actual,
            }
        };
        SessionTable { id, next: 1, rows: BTreeMap::new(), prune_at: PRUNE_FLOOR }
    }

    /// Issues the key of a new authenticated connection. Numbers are monotonic and never
    /// reused; the surface's `label` is for display and the journal only.
    pub fn open_session(&mut self, label: SessionId) -> Result<SessionKey, BusError> {
        let Some(table) = self.id else {
            return Err(invariant("session table numbers exhausted"));
        };
        let number = self.next;
        let Some(next) = number.checked_add(1) else {
            return Err(invariant("session numbers exhausted"));
        };
        if self.rows.len() >= self.prune_at {
            self.rows.retain(|_, r| r.open());
            self.prune_at = self.rows.len().saturating_mul(2).max(PRUNE_FLOOR);
        }
        self.next = next;
        let open = Arc::new(());
        self.rows.insert(number, Row { open: Arc::downgrade(&open), generation: 0, grant: None });
        Ok(SessionKey { table, number, label, open })
    }

    /// Number of live upgrade grants.
    pub fn len(&self) -> usize {
        self.rows.values().filter(|r| r.granted()).count()
    }

    /// Whether no live upgrade grant exists.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether `session` holds a live upgrade grant.
    pub fn is_upgraded(&self, session: &SessionKey) -> bool {
        self.owns(session) && self.rows.get(&session.number).is_some_and(Row::granted)
    }

    /// Ends a session (its channel closed): removes its row, which revokes its grant and
    /// makes any pending upgrade of it fail. Takes the key by value, so an ended key cannot
    /// be presented again. Returns whether the session held a live grant.
    pub fn end_session(&mut self, session: SessionKey) -> bool {
        self.owns(&session) && self.rows.remove(&session.number).is_some_and(|r| r.granted())
    }

    fn owns(&self, session: &SessionKey) -> bool {
        self.id == Some(session.table)
    }

    /// Live grants of sessions other than `number`.
    fn other_grants(&self, number: u64) -> usize {
        self.rows.iter().filter(|(n, r)| **n != number && r.granted()).count()
    }

    /// The confirmed OS user when the channel presents a live grant issued by this table
    /// for this very session key, on the same channel kind, from the same OS user.
    fn granted_user(&self, kind: ChannelKind, channel: &dyn Channel, identity: &Identity) -> Option<&OsUser> {
        let grant = channel.upgrade_grant()?;
        let session = channel.session();
        if !self.owns(session) || grant.table != session.table || grant.number != session.number {
            return None;
        }
        let row = self.rows.get(&grant.number)?;
        let g = row.grant.as_ref()?;
        let live = g.alive.upgrade()?;
        let same =
            Arc::ptr_eq(&live, &grant.alive) && g.channel == kind && identity.os_user.as_ref() == Some(&g.os_user);
        same.then_some(&g.os_user)
    }

    /// Derives the principal of a call from its channel alone (REQ-002..008, REQ-012).
    pub fn derive_principal(&self, channel: &dyn Channel) -> Result<Principal, BusError> {
        let identity = channel.authenticated_identity();
        self.derive_with(channel, identity.as_ref())
    }

    fn derive_with(&self, channel: &dyn Channel, identity: Option<&Identity>) -> Result<Principal, BusError> {
        let Some(identity) = identity else {
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
                if let Some(user) = self.granted_user(kind, channel, identity) {
                    p.kind = PrincipalKind::Human;
                    p.id = PrincipalId::from(user);
                    p.upgraded_session = Some(channel.session().number);
                }
            }
            ChannelKind::ServerSession | ChannelKind::Internal => match &identity.acting_for {
                Some(behalf) => p.on_behalf_of = Some(behalf.clone()),
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

    /// First step of upgrading a CLI or Python session to `human` (REQ-009..011): every
    /// check that needs no person, in the pinned order. Takes `&self` and returns a token
    /// that borrows nothing, so no table borrow is held while the person is asked
    /// ([`PendingUpgrade::confirm`], up to [`UPGRADE_DEADLINE`]). Then
    /// [`SessionTable::commit_upgrade`] records it. Every request prompts.
    pub fn prepare_upgrade(&self, channel: &dyn Channel) -> Result<PendingUpgrade, BusError> {
        // One identity fetch; authentication first, so an incomplete identity is
        // `unauthenticated` on every channel.
        let fetched = channel.authenticated_identity();
        let current = self.derive_with(channel, fetched.as_ref())?;
        let Some(identity) = fetched else {
            return Err(unauthenticated("channel supplied no authenticated identity"));
        };
        let kind = channel.kind();
        if !kind.upgradable() {
            return Err(refused(UpgradeRefusedReason::Channel, "only cli and python sessions can be upgraded"));
        }
        let session = channel.session();
        if !self.owns(session) {
            return Err(invariant("session key was issued by another session table"));
        }
        let Some(row) = self.rows.get(&session.number).filter(|r| r.open()) else {
            return Err(invariant("session key has no open row"));
        };
        let Some(os_user) = identity.os_user.clone() else {
            return Err(refused(UpgradeRefusedReason::UserMismatch, "session has no OS user"));
        };
        if self.other_grants(session.number) >= MAX_UPGRADED_SESSIONS {
            return Err(table_full());
        }
        // The entry is written by the session's own, not-upgraded principal.
        let mut before = current;
        before.kind = kind.base_kind();
        before.id = identity.subject.clone();
        before.upgraded_session = None;
        Ok(PendingUpgrade {
            table: session.table,
            number: session.number,
            generation: row.generation,
            journal_id: session.journal_id(),
            channel: kind,
            request: UpgradeRequest {
                session: session.label.clone(),
                session_number: session.number,
                channel: kind,
                subject: identity.subject,
                os_user,
            },
            before: before.to_draft(),
        })
    }

    /// Last step of an upgrade: re-checks that the session is still open and its row is
    /// unchanged since [`SessionTable::prepare_upgrade`] (same generation; otherwise
    /// `conflict/head_moved`, for a session ended or upgraded by a competing request while
    /// the person was asked) and that the table has room, then appends one
    /// `principal_upgrade` entry to `journal` and issues the grant. The grant, presented by
    /// the same channel, makes its calls derive as `human`; it replaces (revokes) the
    /// session's earlier grant. On any error the table and the journal are unchanged.
    pub fn commit_upgrade(
        &mut self,
        confirmed: ConfirmedUpgrade,
        journal: &mut dyn JournalAppend,
        ctx: &UpgradeContext,
    ) -> Result<UpgradeGrant, BusError> {
        let ConfirmedUpgrade { pending, method, time } = confirmed;
        if self.id != Some(pending.table) {
            return Err(invariant("pending upgrade was prepared by another session table"));
        }
        let unchanged = self.rows.get(&pending.number).is_some_and(|r| r.open() && r.generation == pending.generation);
        if !unchanged {
            return Err(BusError::Conflict(
                ConflictReason::HeadMoved,
                Detail::new("session ended or changed while the person was asked"),
            ));
        }
        if self.other_grants(pending.number) >= MAX_UPGRADED_SESSIONS {
            return Err(table_full());
        }
        let Some(generation) = pending.generation.checked_add(1) else {
            return Err(invariant("session generation exhausted"));
        };
        let draft = EntryDraft {
            stream: ctx.stream.clone(),
            branch: ctx.branch.clone(),
            logical_parent: None,
            principal: pending.before,
            ai_assistance: AiAssistance::None,
            manifests: ctx.manifests,
            reason: None,
            human_assertion: None,
            agent_coedited: false,
            body: DraftBody::PrincipalUpgrade(UpgradeDraft {
                session_id: pending.journal_id,
                method,
                identity: CValue::Str(pending.request.os_user.as_str().to_string()),
                confirmed_at: time.utc.clone(),
            }),
            time,
        };
        journal.append(&draft)?;
        let Some(row) = self.rows.get_mut(&pending.number) else {
            return Err(invariant("session row vanished during commit"));
        };
        let alive = Arc::new(pending.number);
        row.generation = generation;
        let os_user = pending.request.os_user;
        row.grant = Some(GrantRow { channel: pending.channel, os_user, alive: Arc::downgrade(&alive) });
        Ok(UpgradeGrant { table: pending.table, number: pending.number, alive })
    }
}

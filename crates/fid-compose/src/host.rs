//! The in-process host: the process's OS user and session host, the presence providers, the
//! one port of the process, opening a project and handing out the transport
//! (REQ-AUTO.SURF.COMPOSE-007 .. -014, -016).
//!
//! **Identity.** The OS user of every session is the process's OS account, read from the OS
//! ([`process_os_user`]): the SID of the Windows process token user, or `uid:<n>` for the POSIX
//! real user ID. Nothing is read from environment variables, argv, files or parameters. A
//! lookup failure is `unauthenticated/no_identity` and no host is built.
//!
//! **Presence.** [`PRESENCE_PROVIDERS`] lists the shipped `HumanPresence` implementations by
//! platform. It is empty, so [`SessionHost::new`] gets no provider and every
//! [`InProcessHost::confirm_human`] fails with `upgrade_refused/declined`, detail
//! `no_presence_provider`, and the port stays `script` (fail closed; PRIN-003, -009 .. -011).
//!
//! **Opening a project** ([`InProcessHost::open_project`]):
//!
//! - a `root:` handle resolves through the approved roots of `CORE.BUS.POL.FS`
//!   (`resolve_folder`, as `project.open_project@1` declares its `source`), with the
//!   principal of a fresh `cli` session, which is never `human`; nothing opens if it does not
//!   resolve. A project in an import root opens read-only only;
//! - a raw path is refused with `policy/raw_path_refused`, before anything is opened or
//!   locked, unless the port is `human` or holds a person-confirmed upgrade still pending. For a
//!   pending upgrade the project is opened and its bus composed, and the port is attached, which
//!   records the upgrade on that project's Record Journal first (REQ-CORE.BUS.PORT-010). Only if
//!   the port is then `human`, that is its grant is journalled, is the project kept. Otherwise
//!   (a read-only project, which has no upgrade journal, or a failed append) the port goes
//!   back to the no-project bus, the project and its lock are released, and the open fails with
//!   `raw_path_refused`. So a port never calls a project opened by raw path unless a
//!   `principal_upgrade` entry for its session is on that project's journal;
//! - a port that holds a grant moves only to a bus with the same Record Journal
//!   (REQ-CORE.BUS.PORT-015), so a human upgrade is never carried to another project: the
//!   attach is refused and the project released. One host opens at most one project.
//!
//! **Transport.** [`InProcessHost::into_transport`] hands out a [`HostTransport`]: the
//! `AUTO.SURF.INPROC` transport over the port, then the project guard. Dropping it closes the
//! port first and then releases the project and its lock (REQ-013).

use crate::compose::{Composition, OpenedProject, ProjectGuard, SystemUpgradeClock};
use fid_bus::error::{BusError, Detail, InternalReason, PolicyReason, PreconditionReason, UnauthenticatedReason};
use fid_bus::policy::fs::{resolve_folder, Roots, MAX_RAW_PATH_BYTES};
use fid_bus::port::{channel_kind, to_api_error, LocalBus, LocalPort, SessionHost};
use fid_bus::principal::{
    Channel, ChannelKind, HumanPresence, Identity, OsUser, Principal, PrincipalId, SessionId, SessionKey, SessionTable,
    UpgradeGrant,
};
use fid_bus_api::{ApiError, ApiReply, ApiStage, BusCall, BusPort, ChannelInfo, PortKind, PrincipalKind};
use fid_journal::JournalError;
use fid_store::settings::{machine_config_dir, user_config_dir, FileStore, Scope, Sources};
use fid_store::{LockInfo, OpenMode, ProjectLocation, Store, StoreError, TxnError, VerifiedFolder};
use fid_surface::inproc::InProcTransport;
use fid_surface::jobs::{BusEvent, BusRequest, BusTransport, SendError, SystemKeySource};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

// ------------------------------------------------------------------ errors

/// Why the host could not be built or a project not opened.
#[derive(Debug, Clone, PartialEq)]
pub enum HostError {
    /// A bus error, unchanged: `policy/raw_path_refused`, `policy/bad_handle` and the other
    /// file-policy refusals, `unauthenticated/no_identity`, attach and upgrade errors.
    Bus(ApiError),
    /// The store or the journal refused to open the project: its code (for example `locked`,
    /// with the holder when the lock record could be read) and detail.
    Store { code: String, detail: String, holder: Option<LockInfo> },
    /// A bus error that does not fit an [`ApiError`] (never for this build's catalogue).
    Internal(String),
}

impl HostError {
    /// The bus error code, or the storage code.
    pub fn code(&self) -> &str {
        match self {
            HostError::Bus(e) => e.code(),
            HostError::Store { code, .. } => code,
            HostError::Internal(_) => "internal",
        }
    }

    /// The bus error reason, or the storage code.
    pub fn reason(&self) -> &str {
        match self {
            HostError::Bus(e) => e.reason(),
            HostError::Store { code, .. } => code,
            HostError::Internal(_) => "invariant_violated",
        }
    }

    pub(crate) fn bus(e: &BusError) -> HostError {
        match to_api_error(e, ApiStage::Session) {
            Ok(api) => HostError::Bus(api),
            Err(x) => HostError::Internal(format!("{}/{}: {x}", e.code(), e.reason())),
        }
    }

    pub(crate) fn from_store(e: &StoreError) -> HostError {
        let holder = match e {
            StoreError::Locked(h) => h.clone(),
            _ => None,
        };
        HostError::Store { code: e.code().to_string(), detail: e.to_string(), holder }
    }

    pub(crate) fn from_txn(e: &TxnError) -> HostError {
        match e {
            TxnError::Store(s) => HostError::from_store(s),
            other => HostError::Store { code: other.code().to_string(), detail: other.to_string(), holder: None },
        }
    }

    pub(crate) fn from_journal(e: &JournalError) -> HostError {
        HostError::Store { code: e.code().to_string(), detail: e.to_string(), holder: None }
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::Bus(e) => write!(f, "{}/{}: {}", e.code(), e.reason(), e.detail()),
            HostError::Store { code, detail, .. } => write!(f, "{code}: {detail}"),
            HostError::Internal(d) => write!(f, "internal: {d}"),
        }
    }
}

impl std::error::Error for HostError {}

fn policy(reason: PolicyReason, detail: &str) -> HostError {
    HostError::bus(&BusError::Policy(reason, Detail::new(detail)))
}

fn invariant(detail: &str) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, Detail::new(detail))
}

/// The refusal of a raw path (REQ-011).
fn raw_path_refused() -> HostError {
    policy(
        PolicyReason::RawPathRefused,
        "a raw path needs a human principal, or a person-confirmed upgrade recorded on the project's journal",
    )
}

/// The refusal of a raw path whose pending upgrade could not be recorded on the project's
/// journal: `raw_path_refused`, with the cause in the detail.
fn raw_path_refused_by(cause: &ApiError) -> HostError {
    policy(
        PolicyReason::RawPathRefused,
        &format!(
            "the person-confirmed upgrade could not be recorded on the project's journal ({}/{}: {})",
            cause.code(),
            cause.reason(),
            cause.detail()
        ),
    )
}

// ------------------------------------------------------------------ OS user and presence

/// A platform of [`PRESENCE_PROVIDERS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Platform {
    Linux,
    Macos,
    Windows,
    /// Any other target: no provider is ever listed for it.
    Other,
}

impl Platform {
    /// The platform this build runs on (a compile-time constant, never the environment).
    pub fn current() -> Platform {
        match std::env::consts::OS {
            "linux" => Platform::Linux,
            "macos" => Platform::Macos,
            "windows" => Platform::Windows,
            _ => Platform::Other,
        }
    }
}

/// The shipped `HumanPresence` implementations, by platform and type name (REQ-008, REQ-009).
/// Empty until an OS presence provider (TRUST.SIGN.UV or a PLAT presence check) is built, so a
/// confirmation fails closed everywhere.
pub const PRESENCE_PROVIDERS: &[(Platform, &str)] = &[];

/// The provider named `name` in [`PRESENCE_PROVIDERS`]. No provider is shipped.
fn provider_named(_name: &str) -> Option<Arc<dyn HumanPresence + Send + Sync>> {
    None
}

/// The presence provider [`PRESENCE_PROVIDERS`] lists for `platform`, or none (REQ-008).
pub fn presence_provider(platform: Platform) -> Option<Arc<dyn HumanPresence + Send + Sync>> {
    let (_, name) = PRESENCE_PROVIDERS.iter().find(|(p, _)| *p == platform)?;
    provider_named(name)
}

fn unauthenticated(detail: &str) -> BusError {
    BusError::Unauthenticated(UnauthenticatedReason::NoIdentity, Detail::new(detail))
}

#[cfg(unix)]
fn os_account() -> Result<String, BusError> {
    Ok(format!("uid:{}", rustix::process::getuid().as_raw()))
}

#[cfg(windows)]
fn os_account() -> Result<String, BusError> {
    fid_ctrl_acl::current_user_sid_string().map_err(|e| unauthenticated(&format!("process token user: {e}")))
}

#[cfg(not(any(unix, windows)))]
fn os_account() -> Result<String, BusError> {
    Err(unauthenticated("this platform has no OS account lookup"))
}

/// The OS user of the process (REQ-007): the SID of the Windows process token user, or
/// `uid:<n>` for the POSIX real user ID. A lookup failure is `unauthenticated/no_identity`.
pub fn process_os_user() -> Result<OsUser, BusError> {
    let account = os_account()?;
    OsUser::new(&account).map_err(|e| unauthenticated(&format!("OS account: {e:?}")))
}

/// The label of the process's port: `fid/<process id>` (REQ-016).
pub fn process_label() -> String {
    format!("fid/{}", std::process::id())
}

/// The port kind of a `fid` process with these arguments (REQ-016): `mcp` for `fid mcp`,
/// `cli` otherwise.
pub fn port_kind_for(args: &[String]) -> PortKind {
    if args.first().map(String::as_str) == Some("mcp") {
        PortKind::Mcp
    } else {
        PortKind::Cli
    }
}

/// The approved roots of the machine settings (an A0 admin setting, `CORE.BUS.POL.FS`). An
/// unreadable setting gives no roots, so every handle is refused (fail closed).
pub fn roots_from_settings() -> Roots {
    let env = |k: &str| std::env::var_os(k);
    let os = std::env::consts::OS;
    let (Some(user), Some(machine)) = (user_config_dir(os, env), machine_config_dir(os, env)) else {
        return Roots::default();
    };
    let (Ok(user), Ok(machine)) = (FileStore::new(Scope::User, user), FileStore::new(Scope::Machine, machine)) else {
        return Roots::default();
    };
    Roots::from_settings(&Sources { project: None, user: &user, machine: &machine })
        .map_or_else(|_| Roots::default(), |(r, _)| r)
}

// ------------------------------------------------------------------ handles

/// The channel of the principal that resolves `root:` handles: a fresh `cli` session of the
/// host's OS user, never upgraded.
struct ResolveChannel {
    identity: Identity,
    key: SessionKey,
}

impl Channel for ResolveChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Cli
    }

    fn authenticated_identity(&self) -> Option<Identity> {
        Some(self.identity.clone())
    }

    fn session(&self) -> &SessionKey {
        &self.key
    }

    fn upgrade_grant(&self) -> Option<&UpgradeGrant> {
        None
    }
}

/// A `script` principal of `os_user`, for handle resolution only.
fn resolve_principal(os_user: &OsUser) -> Result<Principal, BusError> {
    let mut table = SessionTable::new();
    let label = SessionId::new("fid-compose/resolve").map_err(|e| invariant(&format!("session label: {e:?}")))?;
    let key = table.open_session(label)?;
    let mut identity = Identity::new(PrincipalId::from(os_user));
    identity.os_user = Some(os_user.clone());
    table.derive_principal(&ResolveChannel { identity, key })
}

// ------------------------------------------------------------------ the host

/// How [`InProcessHost::open_project`] names a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectRef {
    /// A `root:<root id>/<relative path>` handle below an approved root.
    Handle(String),
    /// A raw file-system path (REQ-011).
    RawPath(PathBuf),
}

/// The session host, the one port and, once opened, the project of a process (module docs).
pub struct InProcessHost {
    /// `None` once the port was closed because it could not leave a released project's bus.
    port: Option<LocalPort>,
    kind: PortKind,
    /// The refusal of every request once the port is closed.
    closed: ApiError,
    os_user: OsUser,
    no_project: Arc<LocalBus>,
    project: Option<ProjectGuard>,
    composition: Composition,
    roots: Roots,
}

impl fmt::Debug for InProcessHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InProcessHost")
            .field("port", &self.port)
            .field("project", &self.project)
            .field("roots", &self.roots.len())
            .finish_non_exhaustive()
    }
}

impl InProcessHost {
    /// The host of this process (REQ-007, REQ-008, REQ-016): the OS user of the process, the
    /// provider [`PRESENCE_PROVIDERS`] lists for the platform (none today), the shipped
    /// [`Composition`], the approved roots of the settings, and one port of `kind` on the
    /// no-project bus, labelled `fid/<process id>`.
    pub fn for_process(kind: PortKind) -> Result<InProcessHost, HostError> {
        let os_user = process_os_user().map_err(|e| HostError::bus(&e))?;
        let presence = presence_provider(Platform::current());
        let host = SessionHost::new(os_user, presence, Box::new(SystemUpgradeClock)).map_err(|e| HostError::bus(&e))?;
        let composition = Composition::workspace().map_err(|e| HostError::bus(&e))?;
        InProcessHost::with_host(Arc::new(host), kind, roots_from_settings(), composition)
    }

    /// A host over a given session host, roots and composition, with one port of `kind` on the
    /// no-project bus labelled `fid/<process id>`. The desktop app passes its own session host
    /// (with its listed provider); tests pass fakes.
    pub fn with_host(
        host: Arc<SessionHost>,
        kind: PortKind,
        roots: Roots,
        composition: Composition,
    ) -> Result<InProcessHost, HostError> {
        let no_project = composition.no_project_bus().map_err(|e| HostError::bus(&e))?;
        let label =
            SessionId::new(&process_label()).map_err(|e| HostError::bus(&invariant(&format!("port label: {e:?}"))))?;
        let port = host.open(kind, label, Arc::clone(&no_project)).map_err(HostError::Bus)?;
        let os_user = OsUser::new(&port.channel().identity)
            .map_err(|e| HostError::bus(&invariant(&format!("port identity: {e:?}"))))?;
        let closed = to_api_error(&invariant(PORT_CLOSED), ApiStage::Session)
            .map_err(|e| HostError::Internal(format!("closed-port error: {e}")))?;
        Ok(InProcessHost { port: Some(port), kind, closed, os_user, no_project, project: None, composition, roots })
    }

    /// The principal kind the bus derives for the port now; for a closed port, the kind before
    /// any upgrade (never `human`).
    pub fn principal_kind(&self) -> PrincipalKind {
        self.port.as_ref().map_or_else(|| channel_kind(self.kind).base_kind(), BusPort::principal_kind)
    }

    /// The port's channel (for a closed port: its kind and OS user, no session).
    pub fn channel(&self) -> ChannelInfo {
        self.port.as_ref().map_or_else(
            || ChannelInfo { kind: self.kind, identity: self.os_user.as_str().to_string(), session: String::new() },
            BusPort::channel,
        )
    }

    /// Whether the port is still open.
    pub fn is_port_open(&self) -> bool {
        self.port.is_some()
    }

    /// Whether a project is open.
    pub fn has_project(&self) -> bool {
        self.project.is_some()
    }

    /// Asks the session host to confirm a person for the port (CORE.BUS.PORT). With no project
    /// open the confirmation stays pending; opening a project records it (REQ-014).
    pub fn confirm_human(&mut self) -> Result<(), ApiError> {
        match self.port.as_mut() {
            Some(port) => port.confirm_human(),
            None => Err(self.closed.clone()),
        }
    }

    /// Opens `project` in `mode` and moves the port to its bus (module docs; REQ-010 ..
    /// REQ-012, REQ-014).
    pub fn open_project(&mut self, project: ProjectRef, mode: OpenMode) -> Result<(), HostError> {
        if self.project.is_some() {
            return Err(HostError::bus(&BusError::Precondition(
                PreconditionReason::False,
                Detail::new("a project is already open"),
            )));
        }
        let Some(port) = self.port.as_ref() else { return Err(HostError::Bus(self.closed.clone())) };
        let raw = matches!(project, ProjectRef::RawPath(_));
        if raw && port.principal_kind() != PrincipalKind::Human && !port.has_pending_upgrade() {
            return Err(raw_path_refused());
        }
        let opened = match &project {
            ProjectRef::Handle(handle) => self.open_handle(handle, mode)?,
            ProjectRef::RawPath(path) => {
                let text = path.as_os_str();
                if !path.is_absolute() || text.len() > MAX_RAW_PATH_BYTES {
                    return Err(policy(
                        PolicyReason::BadHandle,
                        "a raw path must be absolute and at most 32 767 bytes",
                    ));
                }
                OpenedProject::open(path, mode)?
            }
        };
        let (bus, guard) = self.composition.compose_project(opened).map_err(|e| HostError::bus(&e))?.into_parts();
        let Some(port) = self.port.as_mut() else { return Err(HostError::Bus(self.closed.clone())) };
        // Attach before the transport is handed out: a pending upgrade is recorded first. A
        // failed attach leaves the port on the no-project bus; the project is released.
        if let Err(e) = port.attach(bus) {
            drop(guard);
            return Err(if raw { raw_path_refused_by(&e) } else { HostError::Bus(e) });
        }
        if raw && port.principal_kind() != PrincipalKind::Human {
            // The confirmation could not be recorded on this project's journal: give nothing.
            let back = port.attach(Arc::clone(&self.no_project));
            if back.is_err() {
                // The port cannot leave the project's bus: close it, so that it neither calls
                // the project nor keeps it (and its lock) open.
                if let Some(port) = self.port.take() {
                    port.close();
                }
            }
            drop(guard);
            return Err(raw_path_refused());
        }
        self.project = Some(guard);
        Ok(())
    }

    /// Resolves `handle` through the approved roots and opens the project folder in `mode`.
    fn open_handle(&self, handle: &str, mode: OpenMode) -> Result<OpenedProject, HostError> {
        let def = fid_project_cmd::lifecycle::open_project_def()
            .map_err(|e| HostError::bus(&invariant(&format!("project.open_project@1: {e}"))))?;
        let principal = resolve_principal(&self.os_user).map_err(|e| HostError::bus(&e))?;
        let folder = resolve_folder(&principal, &def, "source", handle, &self.roots)
            .and_then(|t| t.open_project())
            .map_err(|e| HostError::bus(&e))?;
        match mode {
            OpenMode::Read => {
                let store = Store::open_at(&folder, OpenMode::Read).map_err(|e| HostError::from_txn(&e))?;
                Ok(OpenedProject::read_only(store))
            }
            OpenMode::Write => {
                if folder.read_only() {
                    return Err(policy(PolicyReason::BadRoot, "the project is in an import root and opens read-only"));
                }
                let (location, lock) =
                    ProjectLocation::open_at(&folder, OpenMode::Write).map_err(|e| HostError::from_store(&e))?;
                let lock = lock.ok_or_else(|| HostError::from_store(&StoreError::LockLost))?;
                OpenedProject::writable(location, lock, folder.folder_path())
            }
        }
    }

    /// Hands out the transport (REQ-013, REQ-014): the in-process transport over the port,
    /// then the project guard.
    pub fn into_transport(self) -> HostTransport {
        let InProcessHost { port, kind, closed, os_user, project, .. } = self;
        let port: Box<dyn BusPort + Send> = match port {
            Some(port) => Box::new(port),
            None => Box::new(ClosedPort { kind, identity: os_user.as_str().to_string(), error: closed }),
        };
        let transport = InProcTransport::new(port, Box::new(SystemKeySource::new()));
        HostTransport { transport, project }
    }
}

/// The detail of every request on a closed port.
const PORT_CLOSED: &str = "the port was closed: it could not leave a released project's bus";

/// What a closed port's transport calls: it runs nothing, is never `human`, and refuses every
/// call and upgrade.
struct ClosedPort {
    kind: PortKind,
    identity: String,
    error: ApiError,
}

impl BusPort for ClosedPort {
    fn channel(&self) -> ChannelInfo {
        ChannelInfo { kind: self.kind, identity: self.identity.clone(), session: String::new() }
    }

    fn principal_kind(&self) -> PrincipalKind {
        channel_kind(self.kind).base_kind()
    }

    fn confirm_human(&mut self) -> Result<(), ApiError> {
        Err(self.error.clone())
    }

    fn call(&mut self, _call: &BusCall) -> ApiReply {
        ApiReply::Failed(self.error.clone())
    }
}

/// The transport a surface gets (REQ-013): the `AUTO.SURF.INPROC` transport over the port and
/// the project it holds. Dropping it closes the port first and then releases the project and
/// its lock: fields drop in declaration order.
pub struct HostTransport {
    transport: InProcTransport,
    project: Option<ProjectGuard>,
}

impl fmt::Debug for HostTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostTransport").field("transport", &self.transport).field("project", &self.project).finish()
    }
}

impl HostTransport {
    /// The principal kind the bus derives for the port now (never set by the transport).
    pub fn principal_kind(&self) -> PrincipalKind {
        self.transport.principal_kind()
    }

    /// The port's channel.
    pub fn channel(&self) -> ChannelInfo {
        self.transport.channel()
    }

    /// Forwards a confirmation request to the port.
    pub fn confirm_human(&mut self) -> Result<(), ApiError> {
        self.transport.confirm_human()
    }

    /// Whether a project is open.
    pub fn has_project(&self) -> bool {
        self.project.is_some()
    }
}

impl BusTransport for HostTransport {
    fn send(&mut self, request: &BusRequest) -> Result<(), SendError> {
        self.transport.send(request)
    }

    fn recv(&mut self) -> Option<BusEvent> {
        self.transport.recv()
    }
}

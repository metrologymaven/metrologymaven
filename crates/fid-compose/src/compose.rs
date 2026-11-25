//! Composing the project bus and the no-project bus (REQ-AUTO.SURF.COMPOSE-002, -003, -005,
//! -006, -012, -014).
//!
//! A [`Composition`] holds the command table of the manifest in force, its definitions, the
//! handler sources and the unwired list. From it:
//!
//! - [`Composition::no_project_bus`] builds the bus of a process with no project open: the
//!   handlers the sources register without a project, a [`NoProject`] stub for every other
//!   major, a Record Journal that refuses every append with `precondition/false` and detail
//!   `no project open`, and no upgrade journal, so nothing commits and no upgrade is recorded;
//! - [`Composition::compose_project`] builds the bus of one [`OpenedProject`]: every handler the
//!   sources register for it, an [`Unwired`] stub for every [`crate::UNWIRED`] major, the
//!   project's Record Journal and, for a writable project, its upgrade journal on the working
//!   branch.
//!
//! **Project state.** The state view of a call holds the container entities committed on the
//! working branch (folded from the journal when the project is opened, then by a commit hook),
//! an element tree with no elements, and the branch head read from the journal. Opening a
//! project whose working branch holds element-tree or stage events is refused: replaying them is
//! not built here, and a view without them would give wrong answers. No wired handler emits
//! such events, so a project opened here never gains them.
//!
//! Determinism N (clock, OS randomness, locks); a composition from the same workspace and
//! project is deterministic in content.

use crate::sources::{
    coverage, unwired_list, FreeParts, HandlerSource, NoProject, ProjectParts, Unwired, HANDLER_SOURCES,
    NO_PROJECT_OPEN,
};
use fid_bus::error::{BusError, ConflictReason, Detail, InternalReason, PreconditionReason};
use fid_bus::handler::{EntityId, EntityRef, Event, HandlerSet, SettingKey, StateView};
use fid_bus::pipeline::{
    journal_error, AppendFailure, AppendOutcome, Bus, BusEnv, BusParts, CommitHook, CommitRecord, RecordJournal,
    StagedFold, Watch,
};
use fid_bus::policy::projection::RequestId;
use fid_bus::port::{CallParts, JournalId, LocalBus, UpgradeJournal};
use fid_bus::principal::{Channel, JournalAppend, UpgradeClock, UpgradeContext};
use fid_bus::route::SessionRouter;
use fid_bus::validate::{CommandTable, NoRoots};
use fid_canon::{parse_with_limits, CValue, Digest, MAX_DEPTH};
use fid_journal::branch::BranchName;
use fid_journal::stream::{format_utc, ActivityLog, ActivityLogConfig, Streams};
use fid_journal::{parse_digest, AppendRequest, EntryDraft, EntryHash, EntryTime, Journal, Manifests, GENESIS_PREV};
use fid_project::id::{uuid_v7, ObservationSource, SystemSource};
use fid_project::lifecycle::PROJECT_IDENTIFIED;
use fid_project::{ElementTypeRegistry, Tree, Uuid, CONTAINER_CREATED};
use fid_project_cmd::tree::TreeState;
use fid_registry::{Catalogue, CommandDef, ContextTypes, PredContext};
use fid_store::{OpenMode, ProjectLocation, Store};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The Record Journal stream every bus commits to.
pub const RECORD_STREAM: &str = "record";
/// The working branch of every session composed here.
pub const WORKING_BRANCH: &str = "main";
/// Largest entry payload read while folding a project.
const MAX_PAYLOAD_BYTES: u64 = 16 * 1024 * 1024;

/// Event types that change the element tree or the stages. A project whose working branch
/// holds one is refused at open (module docs).
pub const UNFOLDED_EVENTS: [&str; 12] = [
    "ActiveStageSet",
    "ElementCreated",
    "ElementSlotSet",
    "ElementsMoved",
    "GroupCreated",
    "MetadataSet",
    "Renamed",
    "StageAdded",
    "StageRemoved",
    "StagesReordered",
    "TagsSet",
    "VisibilitySet",
];

fn invariant(detail: &str) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, Detail::new(detail))
}

fn storage(e: impl fmt::Display) -> BusError {
    BusError::Internal(InternalReason::Storage, Detail::new(&e.to_string()))
}

/// Every command definition of `fid_registry_build::SOURCES`, in source order: the commands of
/// the manifest in force.
pub fn workspace_defs() -> Result<Vec<CommandDef>, BusError> {
    let mut defs = Vec::new();
    for source in &fid_registry_build::SOURCES {
        defs.extend((source.defs)().map_err(|e| invariant(&format!("{}: {e}", source.krate)))?);
    }
    Ok(defs)
}

/// The kernel manifest digest every entry records until a kernel manifest is in force: the
/// SHA-256 of the empty string, a value no kernel manifest can have.
pub fn no_kernel_manifest() -> Digest {
    fid_canon::digest_bytes(b"")
}

// ------------------------------------------------------------------ host time and randomness

/// The process start, for monotonic readings.
fn base() -> Instant {
    static BASE: OnceLock<Instant> = OnceLock::new();
    *BASE.get_or_init(Instant::now)
}

/// The last monotonic reading handed out, so readings strictly increase across every bus and
/// clock of the process.
static LAST_MONO: AtomicU64 = AtomicU64::new(0);

/// The host time now: UTC from the system clock, a strictly increasing monotonic reading and
/// sync state `unknown` (no sync probe yet).
pub fn host_time() -> EntryTime {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    let elapsed = u64::try_from(base().elapsed().as_nanos()).unwrap_or(u64::MAX);
    let mut last = LAST_MONO.load(Ordering::SeqCst);
    let mono_ns = loop {
        let next = elapsed.max(last.saturating_add(1));
        match LAST_MONO.compare_exchange_weak(last, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => break next,
            Err(actual) => last = actual,
        }
    };
    EntryTime { utc: format_utc(since), mono_ns, sync: "unknown".to_string() }
}

/// The upgrade clock of a process: [`host_time`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemUpgradeClock;

impl UpgradeClock for SystemUpgradeClock {
    fn now(&mut self) -> EntryTime {
        host_time()
    }
}

/// The host inputs of every call: [`host_time`], a seed and UUIDv7 request IDs from the OS
/// CSPRNG.
#[derive(Debug, Default)]
struct SystemEnv;

impl BusEnv for SystemEnv {
    fn now(&self) -> EntryTime {
        host_time()
    }

    fn seed(&self) -> u64 {
        // A seed is recorded in the entry, so a fallback only loses unpredictability.
        match SystemSource.random_bits(64) {
            Ok(bits) => u64::try_from(bits.value() & u128::from(u64::MAX)).unwrap_or(0),
            Err(_) => host_time().mono_ns,
        }
    }

    fn request_id(&self) -> Result<RequestId, BusError> {
        let (ms, bits) = SystemSource.draw_v7().map_err(|e| invariant(&format!("request id: {e}")))?;
        let id = uuid_v7(ms, &bits).map_err(|e| invariant(&format!("request id: {e}")))?;
        RequestId::from_uuid(id)
    }
}

// ------------------------------------------------------------------ the composition

/// The command table and handler sources every bus of a process is built from.
pub struct Composition {
    table: Arc<CommandTable>,
    defs: BTreeMap<String, CommandDef>,
    manifests: Manifests,
    sources: Vec<HandlerSource>,
    unwired: Vec<(String, String)>,
}

impl fmt::Debug for Composition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Composition")
            .field("manifest", &self.manifests.registry)
            .field("majors", &self.defs.len())
            .field("sources", &self.sources.len())
            .field("unwired", &self.unwired.len())
            .finish()
    }
}

impl Composition {
    /// The shipped composition: the manifest in force over `fid_registry_build::SOURCES` and
    /// its merged quantity catalogue, [`HANDLER_SOURCES`] and [`crate::UNWIRED`].
    pub fn workspace() -> Result<Composition, BusError> {
        let build = |e: fid_registry_build::BuildError| invariant(&format!("registry build: {e}"));
        let catalogue = fid_registry_build::workspace_catalogue().map_err(build)?;
        let manifest = fid_registry_build::manifest_in_force(catalogue.clone()).map_err(build)?;
        let defs = workspace_defs()?;
        Composition::new(
            Digest(manifest.digest().to_bytes()),
            defs,
            catalogue,
            HANDLER_SOURCES.to_vec(),
            unwired_list(),
        )
    }

    /// A composition of `defs` (the manifest with digest `manifest_digest` and quantity
    /// catalogue `catalogue`), `sources` and `unwired`. Tests use it with extra sources.
    pub fn new(
        manifest_digest: Digest,
        defs: Vec<CommandDef>,
        catalogue: Catalogue,
        sources: Vec<HandlerSource>,
        unwired: Vec<(String, String)>,
    ) -> Result<Composition, BusError> {
        let table = CommandTable::build(manifest_digest, defs.clone(), catalogue, &ContextTypes::new())
            .map_err(|e| invariant(&format!("command table: {e}")))?;
        let defs = defs.into_iter().map(|d| (format!("{}@{}", d.id, d.major), d)).collect();
        let manifests = Manifests { registry: manifest_digest, kernel: no_kernel_manifest() };
        Ok(Composition { table: Arc::new(table), defs, manifests, sources, unwired })
    }

    /// Every major of the manifest, as `name@major`.
    pub fn majors(&self) -> BTreeSet<String> {
        self.defs.keys().cloned().collect()
    }

    /// The definition of `id` (`name@major`).
    pub fn def(&self, id: &str) -> Option<&CommandDef> {
        self.defs.get(id)
    }

    /// The manifest digests every entry of these buses records.
    pub fn manifests(&self) -> Manifests {
        self.manifests
    }

    /// The command table.
    pub fn table(&self) -> &CommandTable {
        &self.table
    }

    /// The majors the sources register for an open project (no stubs).
    pub fn wired(&self) -> Result<BTreeSet<String>, BusError> {
        let parts = ProjectParts {
            namespace: Uuid::from_u128(0x0190_0000_0000_4000_8000_0000_0000_0001),
            ids: Arc::new(Mutex::new(SystemSource)),
            branch: WORKING_BRANCH.to_string(),
        };
        let mut set = HandlerSet::new();
        for s in &self.sources {
            (s.project)(&mut set, &parts)?;
        }
        Ok(self.defs.keys().filter(|id| set.get(id).is_ok()).cloned().collect())
    }

    /// The majors the sources register without a project.
    pub fn wired_free(&self) -> Result<BTreeSet<String>, BusError> {
        let set = self.free_sources()?;
        Ok(self.defs.keys().filter(|id| set.get(id).is_ok()).cloned().collect())
    }

    /// The coverage findings of this composition ([`coverage`]); empty when covered.
    pub fn coverage(&self) -> Result<Vec<String>, BusError> {
        Ok(coverage(&self.majors(), &self.wired()?, &self.unwired))
    }

    fn free_sources(&self) -> Result<HandlerSet, BusError> {
        let mut set = HandlerSet::new();
        for s in &self.sources {
            if let Some(free) = s.free {
                free(&mut set, &FreeParts)?;
            }
        }
        Ok(set)
    }

    /// The handlers of a project bus: the sources' registrations for `parts`, then one
    /// [`Unwired`] stub per unwired major (REQ-002, REQ-003). An unwired entry that is not a
    /// major of the manifest, or that has a handler, is `internal/invariant_violated`.
    pub fn project_handlers(&self, parts: &ProjectParts) -> Result<HandlerSet, BusError> {
        let mut set = HandlerSet::new();
        for s in &self.sources {
            (s.project)(&mut set, parts)?;
        }
        for (id, _) in &self.unwired {
            let def =
                self.defs.get(id).ok_or_else(|| invariant(&format!("UNWIRED names {id}, not in the manifest")))?;
            set.register(def.clone(), Box::new(Unwired(id.clone())))?;
        }
        Ok(set)
    }

    /// The handlers of the no-project bus: the sources' registrations without a project, then
    /// a [`NoProject`] stub for every other major (REQ-005).
    pub fn free_handlers(&self) -> Result<HandlerSet, BusError> {
        let mut set = self.free_sources()?;
        for (id, def) in &self.defs {
            if set.get(id).is_err() {
                set.register(def.clone(), Box::new(NoProject))?;
            }
        }
        Ok(set)
    }

    fn bus(&self, handlers: HandlerSet, journal: Arc<dyn RecordJournal>, activity: PathBuf) -> Result<Bus, BusError> {
        let branch = BranchName::parse(WORKING_BRANCH).map_err(|e| invariant(&format!("working branch: {e:?}")))?;
        // Recording is never switched on by this composition, so the Activity Log writes
        // nothing and its folder is never created.
        let activity = Arc::new(ActivityLog::new(activity, ActivityLogConfig::default(), self.manifests));
        Ok(Bus::new(BusParts {
            table: Arc::clone(&self.table),
            handlers,
            router: Arc::new(SessionRouter::new(branch)),
            journal,
            streams: Streams::new(activity),
            env: Arc::new(SystemEnv),
            stream: RECORD_STREAM.to_string(),
            manifests: self.manifests,
        }))
    }

    /// The bus of a process with no project open (REQ-005, REQ-006): free handlers and
    /// no-project stubs, a Record Journal that refuses every append, no upgrade journal.
    pub fn no_project_bus(&self) -> Result<Arc<LocalBus>, BusError> {
        let activity = std::env::temp_dir().join("fid-no-project-activity");
        let bus = self.bus(self.free_handlers()?, Arc::new(NoProjectJournal), activity)?;
        Ok(Arc::new(LocalBus::new(bus, Arc::new(NoProjectParts), None)))
    }

    /// The bus of `project` (REQ-002, REQ-003, REQ-012; the algorithm of the leaf's Design,
    /// steps 1 to 4; the caller attaches the port, step 5). A writable project gets an upgrade
    /// journal on the working branch; a read-only one gets none, so nothing can be recorded on
    /// it. Refused when the working branch holds events this composition cannot fold.
    pub fn compose_project(&self, project: OpenedProject) -> Result<ComposedProject, BusError> {
        let scan = project.scan_store();
        let (containers, identified) = fold_containers(scan, WORKING_BRANCH)?;
        let namespace = namespace_of(scan, identified)?;
        let root = scan.location().root().to_path_buf();
        let parts =
            ProjectParts { namespace, ids: Arc::new(Mutex::new(SystemSource)), branch: WORKING_BRANCH.to_string() };
        let handlers = self.project_handlers(&parts)?;
        let tree = Tree::new(namespace, Arc::new(ElementTypeRegistry::new()))
            .map_err(|e| invariant(&format!("element tree: {e}")))?;
        let tree = TreeState::new(tree, GENESIS_PREV)?;
        let (record, upgrades, guard): (Arc<dyn RecordJournal>, Option<UpgradeJournal>, ProjectHold) =
            match project.inner {
                Inner::Writable { journal, .. } => {
                    let ctx = UpgradeContext {
                        stream: RECORD_STREAM.to_string(),
                        branch: WORKING_BRANCH.to_string(),
                        manifests: self.manifests,
                    };
                    let upgrades =
                        UpgradeJournal::new(JournalId::fresh()?, Box::new(UpgradeAppend(Arc::clone(&journal))), ctx);
                    let record: Arc<dyn RecordJournal> = journal.clone();
                    (record, Some(upgrades), ProjectHold::Writable(journal))
                }
                Inner::ReadOnly { store } => {
                    let record: Arc<dyn RecordJournal> = Arc::new(ReadOnlyJournal(Arc::clone(&store)));
                    (record, None, ProjectHold::ReadOnly(store))
                }
            };
        let state = Arc::new(ProjectState {
            cell: Mutex::new(Cell { containers: Arc::new(containers), pending: 0 }),
            tree: Arc::new(tree),
            journal: Arc::clone(&record),
            branch: WORKING_BRANCH.to_string(),
        });
        let mut bus = self.bus(handlers, record, root.join("tmp").join("activity"))?;
        bus.register_hook(Arc::new(ContainerHook(Arc::clone(&state))));
        let parts: Arc<dyn CallParts> = state;
        let bus = Arc::new(LocalBus::new(bus, parts, upgrades));
        Ok(ComposedProject { bus: Arc::clone(&bus), guard: ProjectGuard { bus: Some(bus), hold: Some(guard) } })
    }
}

// ------------------------------------------------------------------ opened projects

enum Inner {
    /// Opened with the write lock; the journal owns it. `scan` is a read-only store of the
    /// same folder, used once to fold the project.
    Writable { journal: Arc<Journal>, scan: Box<Store> },
    /// Opened read-only, without the lock.
    ReadOnly { store: Arc<Store> },
}

/// A project opened through `CORE.STO.LAYOUT`, before its bus is composed. It applies no
/// principal rule: [`crate::InProcessHost::open_project`] applies the raw-path and handle
/// rules before it opens one.
pub struct OpenedProject {
    inner: Inner,
}

impl fmt::Debug for OpenedProject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (mode, root) = match &self.inner {
            Inner::Writable { scan, .. } => ("write", scan.location().root()),
            Inner::ReadOnly { store } => ("read", store.location().root()),
        };
        f.debug_struct("OpenedProject").field("mode", &mode).field("root", &root).finish()
    }
}

fn store_error(e: fid_store::TxnError) -> crate::HostError {
    crate::HostError::from_txn(&e)
}

impl OpenedProject {
    /// Opens the project in `path` in `mode` (REQ-012): read-only without the write lock, or
    /// writable with it, its Record Journal owning the lock.
    pub fn open(path: &Path, mode: OpenMode) -> Result<OpenedProject, crate::HostError> {
        match mode {
            OpenMode::Read => {
                let store = Store::open(path, OpenMode::Read).map_err(store_error)?;
                Ok(OpenedProject { inner: Inner::ReadOnly { store: Arc::new(store) } })
            }
            OpenMode::Write => {
                let (location, lock) =
                    ProjectLocation::open(path, OpenMode::Write).map_err(|e| crate::HostError::from_store(&e))?;
                let Some(lock) = lock else {
                    return Err(crate::HostError::from_store(&fid_store::StoreError::LockLost));
                };
                OpenedProject::writable(location, lock, path)
            }
        }
    }

    /// The writable form of a project whose location and lock are held.
    pub(crate) fn writable(
        location: ProjectLocation,
        lock: fid_store::Locked,
        path: &Path,
    ) -> Result<OpenedProject, crate::HostError> {
        let id = location.folder_id().map_err(|e| crate::HostError::from_store(&e))?;
        let journal = Journal::open(location, lock, RECORD_STREAM).map_err(|e| crate::HostError::from_journal(&e))?;
        journal.location().check_identity(id).map_err(|e| crate::HostError::from_store(&e))?;
        let scan = Store::open(path, OpenMode::Read).map_err(store_error)?;
        scan.location().check_identity(id).map_err(|e| crate::HostError::from_store(&e))?;
        Ok(OpenedProject { inner: Inner::Writable { journal: Arc::new(journal), scan: Box::new(scan) } })
    }

    /// The read-only form of an already opened store.
    pub(crate) fn read_only(store: Store) -> OpenedProject {
        OpenedProject { inner: Inner::ReadOnly { store: Arc::new(store) } }
    }

    /// Whether the project was opened with the write lock.
    pub fn is_writable(&self) -> bool {
        matches!(self.inner, Inner::Writable { .. })
    }

    fn scan_store(&self) -> &Store {
        match &self.inner {
            Inner::Writable { scan, .. } => scan,
            Inner::ReadOnly { store } => store,
        }
    }
}

/// What keeps a project open: its journal (and write lock) or its read-only store.
enum ProjectHold {
    Writable(Arc<Journal>),
    ReadOnly(Arc<Store>),
}

impl ProjectHold {
    /// The project root.
    fn root(&self) -> &Path {
        match self {
            ProjectHold::Writable(journal) => journal.location().root(),
            ProjectHold::ReadOnly(store) => store.location().root(),
        }
    }
}

/// Holds an open project and its bus. Dropping it releases the project and, for a writable
/// project, its write lock, once no port is attached to the bus any more (REQ-013).
pub struct ProjectGuard {
    bus: Option<Arc<LocalBus>>,
    hold: Option<ProjectHold>,
}

impl fmt::Debug for ProjectGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mode = match &self.hold {
            Some(ProjectHold::Writable(_)) => "write",
            Some(ProjectHold::ReadOnly(_)) => "read",
            None => "closed",
        };
        f.debug_struct("ProjectGuard")
            .field("mode", &mode)
            .field("root", &self.hold.as_ref().map(ProjectHold::root))
            .finish_non_exhaustive()
    }
}

impl ProjectGuard {
    /// Whether the project is open with the write lock.
    pub fn is_writable(&self) -> bool {
        matches!(self.hold, Some(ProjectHold::Writable(_)))
    }
}

impl Drop for ProjectGuard {
    fn drop(&mut self) {
        // The bus first (it holds the journal), then the project.
        self.bus = None;
        self.hold = None;
    }
}

/// A composed project bus and the guard that keeps its project open.
#[derive(Debug)]
pub struct ComposedProject {
    bus: Arc<LocalBus>,
    guard: ProjectGuard,
}

impl ComposedProject {
    /// The project bus, for [`fid_bus::port::LocalPort::attach`] or
    /// [`fid_bus::port::SessionHost::open`].
    pub fn bus(&self) -> &Arc<LocalBus> {
        &self.bus
    }

    /// The bus and the guard.
    pub fn into_parts(self) -> (Arc<LocalBus>, ProjectGuard) {
        (self.bus, self.guard)
    }
}

// ------------------------------------------------------------------ reading a project

/// The project namespace: the namespace of its identity record when the working branch holds a
/// `ProjectIdentified` event (a record that does not verify is refused), else the layout UUID
/// of `CORE.STO.LAYOUT`.
fn namespace_of(store: &Store, identified: bool) -> Result<Uuid, BusError> {
    if identified {
        return Ok(fid_project_cmd::lifecycle::read_store_identity(store)?.namespace());
    }
    Uuid::parse(store.location().uuid()).map_err(|e| invariant(&format!("layout uuid: {e:?}")))
}

/// The payload digests of the command entries of `branch`, in sequence order.
fn command_payloads(store: &Store, branch: &str) -> Result<Vec<String>, BusError> {
    store
        .read(|conn| {
            let mut st = conn.prepare(
                "SELECT eb.digest FROM entries e JOIN entry_blobs eb ON eb.entry_hash = e.hash \
                 WHERE e.stream = ?1 AND e.branch = ?2 AND e.kind = 'command' AND eb.role = 'payload' \
                 ORDER BY e.seq ASC",
            )?;
            let rows = st.query_map([RECORD_STREAM, branch], |r| r.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .map_err(storage)
}

type Containers = BTreeMap<Uuid, (EntityId, String, CValue)>;

/// One `ContainerCreated` body as a container entity.
fn container_of(body: &CValue) -> Result<(Uuid, (EntityId, String, CValue)), BusError> {
    let CValue::Obj(o) = body else { return Err(invariant("ContainerCreated body is not an object")) };
    let (Some(CValue::Str(id)), Some(CValue::Str(kind))) = (o.get("id"), o.get("kind")) else {
        return Err(invariant("ContainerCreated body has no id or kind"));
    };
    let uuid = Uuid::parse(id).map_err(|e| invariant(&format!("ContainerCreated id: {e:?}")))?;
    Ok((uuid, (EntityId::new(&uuid.to_string())?, kind.clone(), body.clone())))
}

/// Folds the containers committed on `branch`, and tells whether it holds a
/// `ProjectIdentified` event; refuses a branch holding an event of [`UNFOLDED_EVENTS`] or a
/// container created twice.
fn fold_containers(store: &Store, branch: &str) -> Result<(Containers, bool), BusError> {
    let mut out = Containers::new();
    let mut identified = false;
    for digest in command_payloads(store, branch)? {
        let d = parse_digest(&digest).ok_or_else(|| invariant("malformed payload digest"))?;
        let bytes = store
            .blobs()
            .read_object(&fid_types::Digest::from_bytes(d.0), MAX_PAYLOAD_BYTES)
            .map_err(|e| BusError::Internal(InternalReason::Storage, Detail::new(&format!("payload {digest}: {e}"))))?;
        let payload = parse_with_limits(&bytes, MAX_PAYLOAD_BYTES as usize, MAX_DEPTH)
            .map_err(|e| invariant(&format!("payload {digest}: {e}")))?;
        let CValue::Obj(p) = payload else { return Err(invariant("payload is not an object")) };
        let Some(CValue::Arr(events)) = p.get("events") else { continue };
        for event in events {
            let CValue::Obj(e) = event else { return Err(invariant("event is not an object")) };
            let Some(CValue::Str(kind)) = e.get("type") else { return Err(invariant("event has no type")) };
            if kind == CONTAINER_CREATED {
                let body = e.get("body").ok_or_else(|| invariant("ContainerCreated has no body"))?;
                let (id, entity) = container_of(body)?;
                if out.insert(id, entity).is_some() {
                    return Err(invariant(&format!("container {id} is created twice on {branch}")));
                }
            } else if kind == PROJECT_IDENTIFIED {
                identified = true;
            } else if UNFOLDED_EVENTS.contains(&kind.as_str()) {
                return Err(invariant(&format!(
                    "project state replay is not built: branch {branch} holds {kind} events"
                )));
            }
        }
    }
    Ok((out, identified))
}

// ------------------------------------------------------------------ project state and view

struct Cell {
    containers: Arc<Containers>,
    /// Container commits between their precommit and the end of their staged fold: while one
    /// runs, the journal head may already include its entry and the containers not yet.
    pending: usize,
}

/// The state behind every call of one project bus.
struct ProjectState {
    cell: Mutex<Cell>,
    tree: Arc<TreeState>,
    journal: Arc<dyn RecordJournal>,
    branch: String,
}

fn poisoned() -> BusError {
    invariant("the project state lock is poisoned")
}

impl CallParts for ProjectState {
    fn inputs(&self, _channel: &dyn Channel) -> Result<(Box<dyn StateView>, Box<dyn PredContext>), BusError> {
        let cell = self.cell.lock().map_err(|_| poisoned())?;
        if cell.pending > 0 {
            return Err(BusError::Conflict(
                ConflictReason::HeadMoved,
                Detail::new("a commit is being folded into the project state; retry"),
            ));
        }
        let head = self.journal.branch_head(&self.branch)?.unwrap_or(GENESIS_PREV);
        let view = ProjectView { containers: Arc::clone(&cell.containers), tree: Arc::clone(&self.tree), head };
        Ok((Box::new(view), Box::new(NoRoots)))
    }
}

/// One call's view: the containers, the element tree and the branch head.
struct ProjectView {
    containers: Arc<Containers>,
    tree: Arc<TreeState>,
    head: Digest,
}

impl StateView for ProjectView {
    fn entity(&self, id: &EntityId) -> Option<EntityRef<'_>> {
        if let Some((eid, kind, body)) = Uuid::parse(id.as_str()).ok().and_then(|u| self.containers.get(&u)) {
            if eid.as_str() == id.as_str() {
                return Some(EntityRef { id: eid, kind: kind.as_str(), body });
            }
        }
        self.tree.entity(id)
    }

    fn head(&self) -> Digest {
        self.head
    }

    fn setting(&self, _key: &SettingKey) -> Option<CValue> {
        None
    }
}

/// Folds every committed `ContainerCreated` into the project state.
struct ContainerHook(Arc<ProjectState>);

impl CommitHook for ContainerHook {
    fn name(&self) -> &str {
        "fid-compose containers"
    }

    fn watch(&self) -> Watch {
        Watch::Effects(BTreeSet::from([CONTAINER_CREATED.to_string()]))
    }

    fn stage(&self) -> Result<Box<dyn StagedFold + '_>, BusError> {
        let mut cell = self.0.cell.lock().map_err(|_| poisoned())?;
        cell.pending = cell.pending.checked_add(1).ok_or_else(|| invariant("too many pending commits"))?;
        let known = Arc::clone(&cell.containers);
        Ok(Box::new(StagedContainers { state: &self.0, known, added: Vec::new() }))
    }
}

struct StagedContainers<'a> {
    state: &'a ProjectState,
    known: Arc<Containers>,
    added: Vec<(Uuid, (EntityId, String, CValue))>,
}

impl StagedFold for StagedContainers<'_> {
    fn precommit(&mut self, _record: &CommitRecord<'_>, events: &[Event]) -> Result<(), BusError> {
        for e in events.iter().filter(|e| e.type_id().as_str() == CONTAINER_CREATED) {
            let (id, entity) = container_of(e.body())?;
            if self.known.contains_key(&id) || self.added.iter().any(|(a, _)| *a == id) {
                return Err(invariant(&format!("container {id} already exists")));
            }
            self.added.push((id, entity));
        }
        Ok(())
    }

    fn install(mut self: Box<Self>) -> Result<(), BusError> {
        let mut cell = self.state.cell.lock().map_err(|_| poisoned())?;
        let mut next = (*cell.containers).clone();
        for (id, entity) in std::mem::take(&mut self.added) {
            next.insert(id, entity);
        }
        cell.containers = Arc::new(next);
        Ok(())
    }
}

impl Drop for StagedContainers<'_> {
    fn drop(&mut self) {
        // Installed or refused, the commit is over. A poisoned lock is left as it is: the
        // count then stays raised and every later call is refused (fail closed).
        if let Ok(mut cell) = self.state.cell.lock() {
            cell.pending = cell.pending.saturating_sub(1);
        }
    }
}

// ------------------------------------------------------------------ journals

/// Appends `principal_upgrade` entries to a writable project's Record Journal.
struct UpgradeAppend(Arc<Journal>);

impl JournalAppend for UpgradeAppend {
    fn append(&mut self, draft: &EntryDraft) -> Result<(), BusError> {
        self.0.append(AppendRequest::new(draft.clone())).map(|_| ()).map_err(|e| journal_error(&e))
    }
}

/// The Record Journal of a project opened read-only: reads its tail and heads, refuses every
/// append with `precondition/project_locked`.
struct ReadOnlyJournal(Arc<Store>);

/// The last entry `(seq, hash)` of the record stream, or of `branch` on it.
fn last_row(store: &Store, branch: Option<&str>) -> Result<Option<(i64, String)>, BusError> {
    store
        .read(|conn| match branch {
            None => {
                let mut st =
                    conn.prepare("SELECT seq, hash FROM entries WHERE stream = ?1 ORDER BY seq DESC LIMIT 1")?;
                let mut rows = st.query_map([RECORD_STREAM], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
                rows.next().transpose()
            }
            Some(b) => {
                let mut st = conn.prepare(
                    "SELECT seq, hash FROM entries WHERE stream = ?1 AND branch = ?2 ORDER BY seq DESC LIMIT 1",
                )?;
                let mut rows =
                    st.query_map([RECORD_STREAM, b], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
                rows.next().transpose()
            }
        })
        .map_err(storage)
}

fn entry_hash(hash: &str) -> Result<Digest, BusError> {
    parse_digest(hash).ok_or_else(|| invariant("malformed entry hash"))
}

impl RecordJournal for ReadOnlyJournal {
    fn tail(&self) -> Result<Option<(u64, Digest)>, BusError> {
        match last_row(&self.0, None)? {
            None => Ok(None),
            Some((seq, hash)) => {
                let seq = u64::try_from(seq).map_err(|_| invariant("negative sequence"))?;
                Ok(Some((seq, entry_hash(&hash)?)))
            }
        }
    }

    fn branch_head(&self, branch: &str) -> Result<Option<Digest>, BusError> {
        match last_row(&self.0, Some(branch))? {
            None => Ok(None),
            Some((_, hash)) => Ok(Some(entry_hash(&hash)?)),
        }
    }

    fn append(&self, _req: AppendRequest) -> Result<EntryHash, AppendFailure> {
        Err(AppendFailure {
            error: BusError::Precondition(
                PreconditionReason::ProjectLocked,
                Detail::new("the project is open read-only"),
            ),
            outcome: AppendOutcome::NotAppended,
        })
    }
}

/// The Record Journal of the no-project bus: empty, and every append is refused with
/// `precondition/false` and detail [`NO_PROJECT_OPEN`] (REQ-006).
struct NoProjectJournal;

impl RecordJournal for NoProjectJournal {
    fn tail(&self) -> Result<Option<(u64, Digest)>, BusError> {
        Ok(None)
    }

    fn branch_head(&self, _branch: &str) -> Result<Option<Digest>, BusError> {
        Ok(None)
    }

    fn append(&self, _req: AppendRequest) -> Result<EntryHash, AppendFailure> {
        Err(AppendFailure {
            error: BusError::Precondition(PreconditionReason::False, Detail::new(NO_PROJECT_OPEN)),
            outcome: AppendOutcome::NotAppended,
        })
    }
}

/// The call inputs of the no-project bus: no entities, the empty head, no settings.
struct NoProjectParts;

struct EmptyView;

impl StateView for EmptyView {
    fn entity(&self, _id: &EntityId) -> Option<EntityRef<'_>> {
        None
    }

    fn head(&self) -> Digest {
        GENESIS_PREV
    }

    fn setting(&self, _key: &SettingKey) -> Option<CValue> {
        None
    }
}

impl CallParts for NoProjectParts {
    fn inputs(&self, _channel: &dyn Channel) -> Result<(Box<dyn StateView>, Box<dyn PredContext>), BusError> {
        Ok((Box::new(EmptyView), Box::new(NoRoots)))
    }
}

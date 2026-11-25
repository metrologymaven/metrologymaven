//! The handler sources, the pinned list of unwired command majors and their stubs
//! (REQ-AUTO.SURF.COMPOSE-001 .. -005).
//!
//! [`HANDLER_SOURCES`] has one entry per `fid_registry_build::SOURCES` entry, naming the same
//! crate in the same order. An entry registers its crate's handlers for an open project
//! ([`ProjectParts`]) and, optionally, for no project ([`FreeParts`]). Every manifest major no
//! entry registers is listed in [`UNWIRED`] with a reason, and gets a stub that fails with
//! `internal/invariant_violated` and detail `handler not wired: <name@major>`
//! ([`not_wired_detail`]). [`coverage`] is the check behind the coverage test: every major has
//! exactly one handler or one `UNWIRED` entry.
//!
//! On the no-project bus every major that no entry registers without a project gets a stub
//! that fails with `precondition/false` and detail [`NO_PROJECT_OPEN`].

use fid_bus::error::{BusError, Detail, InternalReason, PreconditionReason};
use fid_bus::handler::{Event, Handler, HandlerCtx, HandlerError, HandlerSet, StateView, ValidParams};
use fid_project::Uuid;
use fid_project_cmd::SharedSource;
use std::collections::BTreeSet;
use std::fmt;

/// The detail of every no-project stub (REQ-005).
pub const NO_PROJECT_OPEN: &str = "no project open";

/// The detail of the stub of an unwired major (REQ-003).
pub fn not_wired_detail(id: &str) -> String {
    format!("handler not wired: {id}")
}

/// What an entry's `project` registration gets from one open project.
pub struct ProjectParts {
    /// The project namespace (the identity record's, or the layout UUID of a project whose
    /// Record Journal is empty).
    pub namespace: Uuid,
    /// The observation source of new IDs (the system clock and the OS CSPRNG).
    pub ids: SharedSource,
    /// The working branch calls commit to.
    pub branch: String,
}

impl fmt::Debug for ProjectParts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProjectParts")
            .field("namespace", &self.namespace)
            .field("branch", &self.branch)
            .finish_non_exhaustive()
    }
}

/// What an entry's `free` registration gets when no project is open. Empty: no crate
/// registers a handler without a project yet.
#[derive(Debug, Clone, Copy, Default)]
pub struct FreeParts;

/// Registers a crate's handlers for an open project.
pub type ProjectRegister = fn(&mut HandlerSet, &ProjectParts) -> Result<(), BusError>;
/// Registers a crate's handlers for the no-project bus.
pub type FreeRegister = fn(&mut HandlerSet, &FreeParts) -> Result<(), BusError>;

/// One crate of `fid_registry_build::SOURCES` and its handler registrations.
#[derive(Clone, Copy)]
pub struct HandlerSource {
    /// The crate name, as in `fid_registry_build::SOURCES`.
    pub krate: &'static str,
    /// Its handlers for an open project.
    pub project: ProjectRegister,
    /// Its handlers for the no-project bus, if any.
    pub free: Option<FreeRegister>,
}

impl fmt::Debug for HandlerSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HandlerSource").field("krate", &self.krate).field("free", &self.free.is_some()).finish()
    }
}

/// A crate none of whose majors is wired yet: every one is in [`UNWIRED`].
fn none(_set: &mut HandlerSet, _parts: &ProjectParts) -> Result<(), BusError> {
    Ok(())
}

/// `fid-project-cmd`: the two element-tree queries (`CORE.PRJ.TREE.QUERY`, R0) and the four
/// container commands (`CORE.PRJ.CONT.HIER`, W2). The stage and tree-group commands change the
/// element tree, which this composition does not fold yet, so they stay in [`UNWIRED`].
fn project_cmd(set: &mut HandlerSet, parts: &ProjectParts) -> Result<(), BusError> {
    fid_project_cmd::query::register(set, parts.namespace)?;
    fid_project_cmd::register_containers(set, parts.namespace, &parts.ids)
}

const fn src(krate: &'static str, project: ProjectRegister) -> HandlerSource {
    HandlerSource { krate, project, free: None }
}

/// One entry per `fid_registry_build::SOURCES` entry, same crates, same order (REQ-001).
pub const HANDLER_SOURCES: [HandlerSource; 20] = [
    src("fid-align", none),
    src("fid-bus", none),
    src("fid-dep", none),
    src("fid-esig", none),
    src("fid-geo-cmd", none),
    src("fid-hal-runtime", none),
    src("fid-jobs", none),
    src("fid-journal", none),
    src("fid-mig", none),
    src("fid-ors", none),
    src("fid-profile-cmd", none),
    src("fid-program-rec", none),
    src("fid-project-cmd", project_cmd),
    src("fid-qual", none),
    src("fid-registry", none),
    src("fid-report", none),
    src("fid-scan-mesh-cmd", none),
    src("fid-store", none),
    src("fid-unc", none),
    src("fid-unc-inst", none),
];

/// The manifest majors with no handler yet, as `(name@major, reason)`, sorted by command
/// (REQ-003). Removing an entry is part of the change that wires its handler.
pub const UNWIRED: &[(&str, &str)] = &[];

/// The stub of an unwired major (REQ-003).
pub(crate) struct Unwired(pub(crate) String);

impl Handler for Unwired {
    fn run(&self, _view: &dyn StateView, _params: &ValidParams, _ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        Err(HandlerError::Refused(BusError::Internal(
            InternalReason::InvariantViolated,
            Detail::new(&not_wired_detail(&self.0)),
        )))
    }
}

/// The stub of a major on the no-project bus (REQ-005).
pub(crate) struct NoProject;

impl Handler for NoProject {
    fn run(&self, _view: &dyn StateView, _params: &ValidParams, _ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        Err(HandlerError::Refused(BusError::Precondition(PreconditionReason::False, Detail::new(NO_PROJECT_OPEN))))
    }
}

/// The coverage check (REQ-004): every major of `majors` has exactly one handler in `handled`
/// or one entry in `unwired`, and every `unwired` entry names a major of `majors` with no
/// handler, once, in sorted order. Returns one finding per problem, each naming the major;
/// empty when the composition is covered.
pub fn coverage(majors: &BTreeSet<String>, handled: &BTreeSet<String>, unwired: &[(String, String)]) -> Vec<String> {
    let mut findings = Vec::new();
    let mut listed = BTreeSet::new();
    let mut previous: Option<&str> = None;
    for (id, reason) in unwired {
        if previous.is_some_and(|p| p >= id.as_str()) {
            findings.push(format!("{id}: UNWIRED is not sorted or lists it twice"));
        }
        previous = Some(id);
        if reason.is_empty() {
            findings.push(format!("{id}: UNWIRED entry has no reason"));
        }
        if !majors.contains(id) {
            findings.push(format!("{id}: UNWIRED entry is not a major of the manifest in force"));
        }
        if handled.contains(id) {
            findings.push(format!("{id}: UNWIRED entry has a handler"));
        }
        listed.insert(id.clone());
    }
    for id in majors {
        if !handled.contains(id) && !listed.contains(id) {
            findings.push(format!("{id}: no handler and no UNWIRED entry"));
        }
    }
    for id in handled {
        if !majors.contains(id) {
            findings.push(format!("{id}: a handler is registered for a major the manifest does not hold"));
        }
    }
    findings
}

/// [`UNWIRED`] as owned pairs.
pub fn unwired_list() -> Vec<(String, String)> {
    UNWIRED.iter().map(|(id, reason)| ((*id).to_string(), (*reason).to_string())).collect()
}

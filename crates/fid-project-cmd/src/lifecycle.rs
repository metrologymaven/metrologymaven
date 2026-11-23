//! Project lifecycle commands on disk and on the bus (`CORE.PRJ.CONT.LIFE`, Part 2):
//! `project.create_project@1` (W2, `write_root`), `project.open_project@1` (R0, `read_root`),
//! `project.close_project@1` (R0), `project.save_project_as@1` (W1, `write_root`, long-running)
//! and `project.list_projects@1` (R0).
//!
//! Store-free identity records and the fail-closed genesis verifier live in
//! `fid_project::lifecycle`. This module wires them to `fid_bus::policy::fs::resolve_folder`,
//! `fid_store::Store` (`create_at`, `open_at`, `copy_to`), `fid_journal` (`Journal::on_store`,
//! `session_start`, `Checkpoint`, `verify_store`) and the recent-projects user setting.

use crate::{created_by, created_event, BusView};
use fid_bus::error::{
    BusError, ConflictReason, Detail, InternalReason, NotFoundReason, PolicyReason, PreconditionReason,
    ValidationReason,
};
use fid_bus::handler::{EntityId, EntityRef, Event, SettingKey, StateView};
use fid_bus::policy::fs::{resolve_folder, Roots};
use fid_bus::principal::{Principal, PrincipalRef};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, Digest as CanonDigest, MAX_DEPTH};
use fid_journal::append::{AppendRequest, Clock, Journal, JournalError};
use fid_journal::checkpoint::{Checkpoint, Format as CheckpointFormat, Reason as CheckpointReason};
use fid_journal::entry::{
    parse_digest, session_start, AiAssistance, CommandDraft, DraftBody, EntryDraft, EntryTime, Field, Manifests,
    OsSalts, OutputDigest, PrincipalDraft, RecordedSalts, SaltSource, SessionInfo, GENESIS_PREV,
};
use fid_journal::verify::{verify_store, VerifyOptions, VerifyReport};
use fid_project::container::ID_PATTERN;
use fid_project::id::{uuid_v7, ObservationSource};
use fid_project::lifecycle::{
    read_identity, Identity, IdentityError, IdentityRecord, JournalEntry, RecordBlobs, SavedAsRecord, SourceRef,
    CLOSE_PROJECT, CREATE_PROJECT, LIST_PROJECTS, MAIN, OPEN_PROJECT, PROJECT_COPIED_FROM, PROJECT_IDENTIFIED,
    PROJECT_SAVED_AS, RECORD_STREAM, SAVE_PROJECT_AS,
};
use fid_project::{ContainerKind, Uuid};
use fid_registry::{CommandDef, DefError};
use fid_store::cas::{BlobStore, CasError, PutOpts};
use fid_store::{CopyTarget, OpenMode, Store, StoreError, TxnError, Value};
use fid_types::{Determinism, Digest as StoreDigest, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

const LIFECYCLE_SINCE: &str = "2026-11-23";
const MAX_LABEL_LEN: usize = 256;
const MAX_RECENT_PROJECTS: usize = 100;
const MAX_PAYLOAD_BYTES: u64 = 16 * 1024 * 1024;
const RECENT_FORMAT: &str = "fid-recent-projects/1";

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

fn obj<const N: usize>(items: [(&str, CValue); N]) -> CValue {
    CValue::Obj(items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn strs(xs: &[&str]) -> CValue {
    CValue::Arr(xs.iter().map(|s| text(s)).collect())
}

fn d(msg: impl fmt::Display) -> Detail {
    Detail::new(&msg.to_string())
}

fn store_digest(dig: CanonDigest) -> StoreDigest {
    StoreDigest::from_bytes(dig.0)
}

fn validate_label(label: &str) -> Result<(), BusError> {
    if label.is_empty() || label.len() > MAX_LABEL_LEN || label.chars().any(|c| c.is_control()) {
        return Err(BusError::Validation(ValidationReason::Label, d("invalid project label")));
    }
    Ok(())
}

/// Formats a Unix timestamp (seconds) as RFC 3339 UTC `YYYY-MM-DDTHH:MM:SSZ`.
pub fn format_utc_rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3_600;
    let min = (rem % 3_600) / 60;
    let sec = rem % 60;
    // Howard Hinnant's civil-from-days algorithm (exact, no external chrono dep).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

// ------------------------------------------------------------------ command definitions

fn path_like_prop(desc: &str) -> CValue {
    obj([
        ("description", text(desc)),
        ("maxLength", CValue::Num(1024.0)),
        ("minLength", CValue::Num(1.0)),
        ("path_like", CValue::Bool(true)),
        ("type", text("string")),
    ])
}

fn label_prop(desc: &str) -> CValue {
    obj([
        ("description", text(desc)),
        ("maxLength", CValue::Num(MAX_LABEL_LEN as f64)),
        ("minLength", CValue::Num(1.0)),
        ("type", text("string")),
    ])
}

fn id_prop(desc: &str) -> CValue {
    obj([("description", text(desc)), ("pattern", text(ID_PATTERN)), ("type", text("string"))])
}

#[allow(clippy::too_many_arguments)]
fn base_def(
    id: &str,
    title: &str,
    description: &str,
    side_effect: &str,
    fs_effect: &str,
    undo: &str,
    effects: &[&str],
    params_schema: CValue,
    result_schema: CValue,
) -> Result<CommandDef, DefError> {
    CommandDef::from_value(obj([
        ("algorithms", CValue::Arr(Vec::new())),
        ("cli_bare", CValue::Bool(false)),
        ("description", text(description)),
        ("determinism", text("D0")),
        ("effects", strs(effects)),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text(fs_effect)),
        ("human_only", CValue::Bool(false)),
        ("id", text(id)),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", text(side_effect)),
        ("title", text(title)),
        ("undo", text(undo)),
        ("web", text("available")),
    ]))
}

/// Registry definition of `project.create_project@1` (W2, D0, `write_root`).
pub fn create_project_def() -> Result<CommandDef, DefError> {
    let params = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([("label", label_prop("Project label.")), ("target", path_like_prop("Target folder handle."))]),
        ),
        ("required", strs(&["label", "target"])),
        ("type", text("object")),
    ]);
    let result = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("head", obj([("type", text("string"))])),
                ("namespace", id_prop("Project namespace UUID.")),
                ("project", id_prop("New UUIDv7 project ID.")),
            ]),
        ),
        ("required", strs(&["head", "namespace", "project"])),
        ("type", text("object")),
    ]);
    base_def(
        CREATE_PROJECT,
        "Create project",
        "Creates an empty project in an approved root with a new UUIDv7 project ID and random namespace UUID.",
        "W2",
        "write_root",
        "head_move",
        &[PROJECT_IDENTIFIED],
        params,
        result,
    )
}

/// Registry definition of `project.open_project@1` (R0, D0, `read_root`).
pub fn open_project_def() -> Result<CommandDef, DefError> {
    let params = obj([
        ("additionalProperties", CValue::Bool(false)),
        ("properties", obj([("source", path_like_prop("Project folder handle."))])),
        ("required", strs(&["source"])),
        ("type", text("object")),
    ]);
    let result = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("branches", obj([("items", obj([("type", text("string"))])), ("type", text("array"))])),
                ("head", obj([("type", text("string"))])),
                ("project", id_prop("Project UUIDv7 ID.")),
                ("read_only", obj([("type", text("boolean"))])),
            ]),
        ),
        ("required", strs(&["branches", "head", "project", "read_only"])),
        ("type", text("object")),
    ]);
    base_def(
        OPEN_PROJECT,
        "Open project",
        "Opens a project folder, acquiring its lock and starting a journal session (or opening read-only in an import root).",
        "R0",
        "read_root",
        "not_undoable",
        &[],
        params,
        result,
    )
}

/// Registry definition of `project.close_project@1` (R0, D0).
pub fn close_project_def() -> Result<CommandDef, DefError> {
    let params = obj([
        ("additionalProperties", CValue::Bool(false)),
        ("properties", CValue::Obj(BTreeMap::new())),
        ("type", text("object")),
    ]);
    let result = obj([
        ("additionalProperties", CValue::Bool(false)),
        ("properties", obj([("checkpointed", obj([("type", text("boolean"))]))])),
        ("required", strs(&["checkpointed"])),
        ("type", text("object")),
    ]);
    base_def(
        CLOSE_PROJECT,
        "Close project",
        "Writes a close checkpoint (when writable), releases the project lock and ends the session.",
        "R0",
        "none",
        "not_undoable",
        &[],
        params,
        result,
    )
}

/// Registry definition of `project.save_project_as@1` (W1, D0, `write_root`).
pub fn save_project_as_def() -> Result<CommandDef, DefError> {
    let params = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("label", label_prop("Optional new label for the copy.")),
                ("target", path_like_prop("Destination folder handle in an approved export root.")),
            ]),
        ),
        ("required", strs(&["target"])),
        ("type", text("object")),
    ]);
    let result = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("namespace", id_prop("Preserved namespace UUID.")),
                ("project", id_prop("New UUIDv7 project ID of the copy.")),
                ("source_head", obj([("type", text("string"))])),
                ("source_project", id_prop("Source project UUIDv7 ID.")),
            ]),
        ),
        ("required", strs(&["namespace", "project", "source_head", "source_project"])),
        ("type", text("object")),
    ]);
    base_def(
        SAVE_PROJECT_AS,
        "Save project as",
        "Atomically copies the open project into an approved export root with a new project ID and identical entity IDs.",
        "W1",
        "write_root",
        "not_undoable",
        &[PROJECT_COPIED_FROM, PROJECT_SAVED_AS],
        params,
        result,
    )
}

/// Registry definition of `project.list_projects@1` (R0, D0).
pub fn list_projects_def() -> Result<CommandDef, DefError> {
    let params = obj([
        ("additionalProperties", CValue::Bool(false)),
        ("properties", CValue::Obj(BTreeMap::new())),
        ("type", text("object")),
    ]);
    let item = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("handle", obj([("type", text("string"))])),
                ("label", obj([("type", text("string"))])),
                ("last_opened", obj([("type", text("string"))])),
                ("project", id_prop("Project UUIDv7 ID.")),
            ]),
        ),
        ("required", strs(&["handle", "label", "last_opened", "project"])),
        ("type", text("object")),
    ]);
    let result = obj([
        ("additionalProperties", CValue::Bool(false)),
        ("properties", obj([("projects", obj([("items", item), ("type", text("array"))]))])),
        ("required", strs(&["projects"])),
        ("type", text("object")),
    ]);
    base_def(
        LIST_PROJECTS,
        "List projects",
        "Lists the user's recent projects sorted by last opened, newest first, at most 100.",
        "R0",
        "none",
        "not_undoable",
        &[],
        params,
        result,
    )
}

/// All five lifecycle command definitions of `CORE.PRJ.CONT.LIFE`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(vec![
        create_project_def()?,
        open_project_def()?,
        close_project_def()?,
        save_project_as_def()?,
        list_projects_def()?,
    ])
}

// ------------------------------------------------------------------ recent projects (REQ-006)

/// One entry in the user's recent-projects setting (`REQ-CORE.PRJ.CONT.LIFE-006`, `AC-5`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentProject {
    pub handle: String,
    pub project: Uuid,
    pub label: String,
    pub last_opened: String,
    /// Monotonic sequence tie-breaker when two projects are opened within the same second.
    order: u64,
}

impl RecentProject {
    pub fn to_value(&self) -> CValue {
        obj([
            ("handle", text(&self.handle)),
            ("label", text(&self.label)),
            ("last_opened", text(&self.last_opened)),
            ("project", text(&self.project.to_string())),
        ])
    }
}

/// Bounded (<= 100) recent-projects list stored as a user setting, sorted newest first.
#[derive(Debug, Clone, Default)]
pub struct RecentProjects {
    entries: Vec<RecentProject>,
    counter: u64,
}

impl RecentProjects {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an open/create/save-as touch of `project`, keeping newest first and at most 100.
    pub fn record(&mut self, handle: &str, project: Uuid, label: &str, last_opened_utc: String) {
        self.counter = self.counter.saturating_add(1);
        self.entries.retain(|e| e.project != project);
        self.entries.push(RecentProject {
            handle: handle.to_owned(),
            project,
            label: label.to_owned(),
            last_opened: last_opened_utc,
            order: self.counter,
        });
        self.entries.sort_by(|a, b| {
            b.last_opened
                .cmp(&a.last_opened)
                .then_with(|| b.order.cmp(&a.order))
                .then_with(|| a.project.cmp(&b.project))
        });
        if self.entries.len() > MAX_RECENT_PROJECTS {
            self.entries.truncate(MAX_RECENT_PROJECTS);
        }
    }

    /// Returns the recent projects sorted newest first, at most 100 (`REQ-006`).
    pub fn list(&self) -> &[RecentProject] {
        &self.entries
    }

    /// Canonical JSON wire form of the user setting.
    pub fn to_value(&self) -> CValue {
        obj([
            ("format", text(RECENT_FORMAT)),
            ("projects", CValue::Arr(self.entries.iter().map(RecentProject::to_value).collect())),
        ])
    }
}

/// Runs `project.list_projects@1` (`REQ-006`, `AC-5`).
pub fn list_projects(recent: &RecentProjects) -> CValue {
    obj([("projects", CValue::Arr(recent.list().iter().map(RecentProject::to_value).collect()))])
}

// ------------------------------------------------------------------ store identity read

struct StoreRecordBlobs<'a>(&'a BlobStore);

impl RecordBlobs for StoreRecordBlobs<'_> {
    fn object(&self, digest: &CanonDigest, max_bytes: usize) -> Result<Option<Vec<u8>>, String> {
        match self.0.read_object(&store_digest(*digest), max_bytes as u64) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(CasError::NotFound(_) | CasError::OutOfRange { .. }) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
}

fn identity_bus_error(e: &IdentityError) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, d(e))
}

fn store_error_to_bus(e: StoreError) -> BusError {
    match e {
        StoreError::Locked(_) => BusError::Precondition(PreconditionReason::ProjectLocked, d(e)),
        StoreError::NotEmpty => BusError::Policy(PolicyReason::WouldOverwrite, d(e)),
        StoreError::FolderSwapped | StoreError::SymlinkRefused => BusError::Policy(PolicyReason::OutsideRoot, d(e)),
        StoreError::NotAProject(_) => BusError::NotFound(NotFoundReason::UnknownEntity, d(e)),
        StoreError::Io(ref io) if io.kind() == std::io::ErrorKind::NotFound => {
            BusError::NotFound(NotFoundReason::UnknownEntity, d(e))
        }
        other => BusError::Internal(InternalReason::Storage, d(other)),
    }
}

fn txn_error_to_bus(e: TxnError) -> BusError {
    match e {
        TxnError::Store(se) => store_error_to_bus(se),
        TxnError::ReadOnly => BusError::Policy(PolicyReason::BadRoot, d("read-only store")),
        other => BusError::Internal(InternalReason::Storage, d(other)),
    }
}

fn journal_error_to_bus(e: JournalError) -> BusError {
    match e {
        JournalError::HeadMoved { .. } => BusError::Conflict(ConflictReason::HeadMoved, d(e)),
        JournalError::ReadOnly(_) => BusError::Policy(PolicyReason::BadRoot, d(e)),
        other => BusError::Internal(InternalReason::Storage, d(other)),
    }
}

/// Reads and verifies the project's identity from `store` using `fid_project::lifecycle::read_identity`
/// (`REQ-008`, `REQ-009`, AC-6, AC-7), supplying `promoted_from` from each verified entry header
/// and loading payloads and record blobs through the safe CAS object reader.
pub fn read_store_identity(store: &Store) -> Result<Identity, BusError> {
    let rows: Vec<(String, Vec<u8>)> = store
        .read(|conn| {
            let mut st = conn.prepare("SELECT hash, header FROM entries WHERE stream = ?1 ORDER BY seq ASC")?;
            let iter = st.query_map([RECORD_STREAM], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
            let mut out = Vec::new();
            for row in iter {
                out.push(row?);
            }
            Ok(out)
        })
        .map_err(txn_error_to_bus)?;

    let payload_map: BTreeMap<String, String> = store
        .read(|conn| {
            let mut st = conn.prepare("SELECT entry_hash, digest FROM entry_blobs WHERE role = 'payload'")?;
            let iter = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            let mut map = BTreeMap::new();
            for row in iter {
                let (h, dig) = row?;
                map.insert(h, dig);
            }
            Ok(map)
        })
        .map_err(txn_error_to_bus)?;

    let blobs = store.blobs();
    let mut entries = Vec::with_capacity(rows.len());
    for (hash, hdr_bytes) in rows {
        let header = parse_with_limits(&hdr_bytes, 1024 * 1024, MAX_DEPTH)
            .map_err(|e| BusError::Internal(InternalReason::InvariantViolated, d(e)))?;
        let promoted_from = match &header {
            CValue::Obj(o) => match o.get("promoted_from") {
                Some(CValue::Str(s)) => parse_digest(s),
                _ => None,
            },
            _ => None,
        };
        let payload = if let Some(pdig_str) = payload_map.get(&hash) {
            let pdig = parse_digest(pdig_str)
                .ok_or_else(|| BusError::Internal(InternalReason::InvariantViolated, d("bad payload digest")))?;
            match blobs.read_object(&store_digest(pdig), MAX_PAYLOAD_BYTES) {
                Ok(bytes) => {
                    let pv = parse_with_limits(&bytes, MAX_PAYLOAD_BYTES as usize, MAX_DEPTH)
                        .map_err(|e| BusError::Internal(InternalReason::InvariantViolated, d(e)))?;
                    Some(pv)
                }
                Err(CasError::NotFound(_)) => None,
                Err(e) => return Err(BusError::Internal(InternalReason::Storage, d(e))),
            }
        } else {
            None
        };
        entries.push(JournalEntry { header, payload, promoted_from });
    }

    read_identity(entries, StoreRecordBlobs(blobs)).map_err(|e| identity_bus_error(&e))
}

fn read_branches_and_head(store: &Store) -> Result<(Vec<String>, CanonDigest), BusError> {
    let rows: Vec<(String, String)> = store
        .read(|conn| {
            let mut st = conn.prepare("SELECT branch, hash FROM entries WHERE stream = ?1 ORDER BY seq ASC")?;
            let iter = st.query_map([RECORD_STREAM], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            let mut out = Vec::new();
            for row in iter {
                out.push(row?);
            }
            Ok(out)
        })
        .map_err(txn_error_to_bus)?;

    let mut branches = BTreeSet::new();
    let mut last_hash = GENESIS_PREV;
    for (br, h) in rows {
        branches.insert(br);
        if let Some(dig) = parse_digest(&h) {
            last_hash = dig;
        }
    }
    Ok((branches.into_iter().collect(), last_hash))
}

fn read_project_label(store: &Store) -> String {
    // Recover label from the latest genesis payload's `args.label` (if present).
    let rows: Vec<String> = store
        .read(|conn| {
            let mut st = conn.prepare(
                "SELECT eb.digest FROM entries e JOIN entry_blobs eb ON eb.entry_hash = e.hash \
                 WHERE e.stream = ?1 AND e.kind = 'command' AND eb.role = 'payload' ORDER BY e.seq DESC",
            )?;
            let iter = st.query_map([RECORD_STREAM], |r| r.get::<_, String>(0))?;
            let mut out = Vec::new();
            for r in iter {
                out.push(r?);
            }
            Ok(out)
        })
        .unwrap_or_default();

    for dig_s in rows {
        if let Some(dig) = parse_digest(&dig_s) {
            if let Ok(bytes) = store.blobs().read_object(&store_digest(dig), 64 * 1024) {
                if let Ok(CValue::Obj(o)) = parse_with_limits(&bytes, 64 * 1024, MAX_DEPTH) {
                    if let Some(CValue::Obj(args)) = o.get("args") {
                        if let Some(CValue::Str(lbl)) = args.get("label") {
                            return lbl.clone();
                        }
                    }
                }
            }
        }
    }
    "Untitled Project".to_owned()
}

fn to_journal_principal(p: &Principal) -> PrincipalDraft {
    PrincipalDraft {
        kind: p.kind(),
        identity: text(p.id().as_str()),
        on_behalf_of: p.on_behalf_of().map(PrincipalRef::to_value),
    }
}

fn make_time(clock: &dyn Clock, mono_bump: u64) -> EntryTime {
    let now = clock.now();
    EntryTime {
        utc: format_utc_rfc3339(now.as_secs()),
        mono_ns: (now.as_nanos() as u64).saturating_add(mono_bump),
        sync: "synced".to_owned(),
    }
}

fn draw_uuid_v4(source: &mut dyn ObservationSource) -> Result<Uuid, BusError> {
    let bits = source.random_bits(128).map_err(|e| BusError::Internal(InternalReason::HandlerFailed, d(e)))?;
    let mut b = bits.value().to_be_bytes();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    Ok(Uuid::from_bytes(b))
}

fn draw_uuid_v7(source: &mut dyn ObservationSource) -> Result<Uuid, BusError> {
    let (ms, bits) = source.draw_v7().map_err(|e| BusError::Internal(InternalReason::HandlerFailed, d(e)))?;
    uuid_v7(ms, &bits).map_err(|e| BusError::Internal(InternalReason::HandlerFailed, d(e)))
}

fn draw_shared_salts(has_obo: bool) -> Result<(RecordedSalts, RecordedSalts), BusError> {
    let mut os = OsSalts;
    let id_salt = os.salt(Field::Identity).map_err(|e| BusError::Internal(InternalReason::HandlerFailed, d(e)))?;
    let obo_salt = if has_obo {
        Some(os.salt(Field::OnBehalfOf).map_err(|e| BusError::Internal(InternalReason::HandlerFailed, d(e)))?)
    } else {
        None
    };
    let args_salt = os.salt(Field::Args).map_err(|e| BusError::Internal(InternalReason::HandlerFailed, d(e)))?;
    let events_salt = os.salt(Field::Events).map_err(|e| BusError::Internal(InternalReason::HandlerFailed, d(e)))?;

    let mut s_map = BTreeMap::new();
    s_map.insert(Field::Identity, id_salt);
    if let Some(s) = obo_salt {
        s_map.insert(Field::OnBehalfOf, s);
    }
    let mut g_map = s_map.clone();
    g_map.insert(Field::Args, args_salt);
    g_map.insert(Field::Events, events_salt);
    Ok((RecordedSalts(s_map), RecordedSalts(g_map)))
}

fn event_envelope(ev_type: &str, body: CValue) -> CValue {
    obj([("body", body), ("type", text(ev_type))])
}

// ------------------------------------------------------------------ session & results

/// Result of `project.create_project@1` (`REQ-001`, `AC-1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateResult {
    pub project: Uuid,
    pub namespace: Uuid,
    pub head: CanonDigest,
    pub label: String,
    pub handle: String,
}

impl CreateResult {
    pub fn to_value(&self) -> CValue {
        obj([
            ("head", text(&self.head.to_string())),
            ("namespace", text(&self.namespace.to_string())),
            ("project", text(&self.project.to_string())),
        ])
    }
}

/// Result of `project.open_project@1` (`REQ-002`, `REQ-012`, `AC-2`, `AC-8`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenResult {
    pub project: Uuid,
    pub namespace: Uuid,
    pub branches: Vec<String>,
    pub head: CanonDigest,
    pub read_only: bool,
}

impl OpenResult {
    pub fn to_value(&self) -> CValue {
        obj([
            ("branches", CValue::Arr(self.branches.iter().map(|b| text(b)).collect())),
            ("head", text(&self.head.to_string())),
            ("project", text(&self.project.to_string())),
            ("read_only", CValue::Bool(self.read_only)),
        ])
    }
}

/// Result of `project.close_project@1` (`REQ-003`, `REQ-012`, `AC-2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseResult {
    pub checkpoint: Option<Checkpoint>,
}

impl CloseResult {
    pub fn to_value(&self) -> CValue {
        obj([("checkpointed", CValue::Bool(self.checkpoint.is_some()))])
    }
}

/// Result of `project.save_project_as@1` (`REQ-004`, `REQ-011`, `AC-3`, `AC-6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveAsResult {
    pub project: Uuid,
    pub namespace: Uuid,
    pub source_project: Uuid,
    pub source_head: CanonDigest,
    pub target_handle: String,
}

impl SaveAsResult {
    pub fn to_value(&self) -> CValue {
        obj([
            ("namespace", text(&self.namespace.to_string())),
            ("project", text(&self.project.to_string())),
            ("source_head", text(&self.source_head.to_string())),
            ("source_project", text(&self.source_project.to_string())),
        ])
    }
}

/// An open project session holding the store (and its OS write lock when writable).
pub struct ProjectSession {
    store: Store,
    identity: Identity,
    label: String,
    handle: String,
    read_only: bool,
    branch: String,
    commit_lock: Mutex<()>,
    clock: Arc<dyn Clock>,
    manifests: Manifests,
}

impl fmt::Debug for ProjectSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProjectSession")
            .field("project", &self.identity.project())
            .field("handle", &self.handle)
            .field("read_only", &self.read_only)
            .field("branch", &self.branch)
            .finish_non_exhaustive()
    }
}

impl ProjectSession {
    pub fn project_id(&self) -> Uuid {
        self.identity.project()
    }

    pub fn namespace(&self) -> Uuid {
        self.identity.namespace()
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn branch(&self) -> &str {
        &self.branch
    }

    pub fn set_branch(&mut self, branch: &str) {
        self.branch = branch.to_owned();
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Refuses any write command when the session was opened read-only from an import root
    /// (`REQ-CORE.PRJ.CONT.LIFE-012`, `AC-8`).
    pub fn require_writable(&self) -> Result<(), BusError> {
        if self.read_only {
            return Err(BusError::Policy(
                PolicyReason::BadRoot,
                d("session is read-only (opened from an import root)"),
            ));
        }
        Ok(())
    }

    /// Runs `fid journal verify-chain` over this session's store (`AC-3`).
    pub fn verify_chain(&self) -> Result<VerifyReport, BusError> {
        verify_store(&self.store, RECORD_STREAM, &VerifyOptions::default())
            .map_err(|e| BusError::Internal(InternalReason::InvariantViolated, d(e)))
    }

    /// Reads the latest verified chain head on stream `record`.
    pub fn head(&self) -> Result<CanonDigest, BusError> {
        let (_, head) = read_branches_and_head(&self.store)?;
        Ok(head)
    }

    /// Reconstructs all container entities committed in this project's Record Journal
    /// (`REQ-007`, `AC-2`, `AC-3`).
    pub fn load_containers(&self) -> Result<BTreeMap<Uuid, (String, CValue)>, BusError> {
        let rows: Vec<String> = self
            .store
            .read(|conn| {
                let mut st = conn.prepare(
                    "SELECT eb.digest FROM entries e JOIN entry_blobs eb ON eb.entry_hash = e.hash \
                     WHERE e.stream = ?1 AND e.kind = 'command' AND eb.role = 'payload' ORDER BY e.seq ASC",
                )?;
                let iter = st.query_map([RECORD_STREAM], |r| r.get::<_, String>(0))?;
                let mut out = Vec::new();
                for row in iter {
                    out.push(row?);
                }
                Ok(out)
            })
            .map_err(txn_error_to_bus)?;

        let mut entities: BTreeMap<Uuid, (String, CValue)> = BTreeMap::new();
        for dig_s in rows {
            let Some(dig) = parse_digest(&dig_s) else { continue };
            let bytes = self
                .store
                .blobs()
                .read_object(&store_digest(dig), MAX_PAYLOAD_BYTES)
                .map_err(|e| BusError::Internal(InternalReason::Storage, d(e)))?;
            let val = parse_with_limits(&bytes, MAX_PAYLOAD_BYTES as usize, MAX_DEPTH)
                .map_err(|e| BusError::Internal(InternalReason::InvariantViolated, d(e)))?;
            if let CValue::Obj(o) = val {
                if let Some(CValue::Arr(evs)) = o.get("events") {
                    for ev in evs {
                        if let CValue::Obj(eo) = ev {
                            if eo.get("type") == Some(&text("ContainerCreated")) {
                                if let Some(body @ CValue::Obj(bo)) = eo.get("body") {
                                    if let (Some(CValue::Str(id_s)), Some(CValue::Str(kind_s))) =
                                        (bo.get("id"), bo.get("kind"))
                                    {
                                        if let Ok(uid) = Uuid::parse(id_s) {
                                            entities.insert(uid, (kind_s.clone(), body.clone()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(entities)
    }

    /// Commits a container create command (`create-part`, `create-revision`, `create-plan`, `create-run`)
    /// onto this session's journal, enforcing `REQ-012` (read-only refusal) and preserving stable
    /// entity IDs (`AC-2`, `AC-3`, `AC-8`).
    pub fn commit_container(
        &self,
        kind: ContainerKind,
        principal: &Principal,
        params: &CValue,
        source: &mut dyn ObservationSource,
    ) -> Result<(Uuid, CanonDigest), BusError> {
        self.require_writable()?;
        let _guard = self
            .commit_lock
            .lock()
            .map_err(|_| BusError::Internal(InternalReason::InvariantViolated, d("commit lock poisoned")))?;
        let existing = self.load_containers()?;
        let jr = Journal::on_store_with_clock(&self.store, RECORD_STREAM, Arc::clone(&self.clock))
            .map_err(journal_error_to_bus)?;
        let tail = jr.tail().map_err(journal_error_to_bus)?;
        let expected_prev = tail.map(|t| t.hash).unwrap_or(GENESIS_PREV);

        let mut indexed: BTreeMap<Uuid, (EntityId, String, CValue)> = BTreeMap::new();
        for (uid, (k, body)) in existing {
            let eid = EntityId::new(&uid.to_string())?;
            indexed.insert(uid, (eid, k, body));
        }
        let view = ContainerMapView { entities: &indexed, head: expected_prev };
        let who = created_by(principal).map_err(|e| crate::to_bus_error(&e))?;
        let c = fid_project::container::create(kind, &BusView(&view), params, &who, self.identity.namespace(), source)
            .map_err(|e| crate::to_bus_error(&e))?;
        let ev: Event = created_event(&c)?;
        let ev_env = event_envelope(ev.type_id().as_str(), ev.body().clone());
        let body_bytes =
            to_canonical_bytes(ev.body()).map_err(|e| BusError::Internal(InternalReason::HandlerFailed, d(e)))?;
        let out_digest = fid_canon::digest_bytes(&body_bytes);
        let draft = EntryDraft {
            stream: RECORD_STREAM.to_owned(),
            branch: self.branch.clone(),
            logical_parent: None,
            time: make_time(&*self.clock, 0),
            principal: to_journal_principal(principal),
            ai_assistance: AiAssistance::None,
            manifests: self.manifests,
            reason: None,
            human_assertion: None,
            agent_coedited: false,
            body: DraftBody::Command(CommandDraft {
                id: kind.command().to_owned(),
                major: 1,
                class: SideEffect::W2,
                determinism: Determinism::D0,
                seed: 0,
                args: params.clone(),
                events: vec![ev_env],
                inputs: Vec::new(),
                consumed_r1: Vec::new(),
                outputs: vec![OutputDigest { entity: Some(c.id().to_string()), digest: out_digest }],
                algorithms: Vec::new(),
                event_sources: Vec::new(),
            }),
        };
        let mut req = AppendRequest::new(draft);
        req.expected_prev = Some(expected_prev);
        let h = jr.append(req).map_err(journal_error_to_bus)?;
        Ok((c.id(), h))
    }

    /// Reads the latest `close` checkpoint stored in this project (for `AC-2` verification).
    pub fn latest_close_checkpoint(&self) -> Result<Option<Checkpoint>, BusError> {
        let rows: Vec<Vec<u8>> = self
            .store
            .read(|conn| {
                let mut st = conn.prepare("SELECT body FROM checkpoints WHERE stream = ?1 ORDER BY id DESC")?;
                let iter = st.query_map([RECORD_STREAM], |r| r.get::<_, Vec<u8>>(0))?;
                let mut out = Vec::new();
                for r in iter {
                    out.push(r?);
                }
                Ok(out)
            })
            .map_err(txn_error_to_bus)?;
        for body in rows {
            let cp = Checkpoint::from_bytes(&body)
                .map_err(|e| BusError::Internal(InternalReason::InvariantViolated, d(e)))?;
            if cp.reason == CheckpointReason::Close {
                return Ok(Some(cp));
            }
        }
        Ok(None)
    }
}

struct ContainerMapView<'a> {
    entities: &'a BTreeMap<Uuid, (EntityId, String, CValue)>,
    head: CanonDigest,
}

impl StateView for ContainerMapView<'_> {
    fn entity(&self, id: &EntityId) -> Option<EntityRef<'_>> {
        let uid = Uuid::parse(id.as_str()).ok()?;
        let (eid, kind, body) = self.entities.get(&uid)?;
        Some(EntityRef { id: eid, kind: kind.as_str(), body })
    }

    fn head(&self) -> CanonDigest {
        self.head
    }

    fn setting(&self, _key: &SettingKey) -> Option<CValue> {
        None
    }
}

// ------------------------------------------------------------------ the five lifecycle operations

fn req_str_param<'a>(params: &'a CValue, key: &str) -> Result<&'a str, BusError> {
    match params {
        CValue::Obj(o) => match o.get(key) {
            Some(CValue::Str(s)) => Ok(s.as_str()),
            _ => Err(BusError::Validation(ValidationReason::Schema, d(format!("missing or invalid string {key:?}")))),
        },
        _ => Err(BusError::Validation(ValidationReason::Schema, d("params must be an object"))),
    }
}

fn opt_str_param<'a>(params: &'a CValue, key: &str) -> Result<Option<&'a str>, BusError> {
    match params {
        CValue::Obj(o) => match o.get(key) {
            None | Some(CValue::Null) => Ok(None),
            Some(CValue::Str(s)) => Ok(Some(s.as_str())),
            _ => Err(BusError::Validation(ValidationReason::Schema, d(format!("invalid string {key:?}")))),
        },
        _ => Err(BusError::Validation(ValidationReason::Schema, d("params must be an object"))),
    }
}

/// Runs `project.create_project@1` (`REQ-001`, `REQ-008`, `REQ-011`, `AC-1`).
pub fn create_project(
    principal: &Principal,
    roots: &Roots,
    recent: &mut RecentProjects,
    params: &CValue,
    source: &mut dyn ObservationSource,
    clock: Arc<dyn Clock>,
    manifests: Manifests,
) -> Result<(ProjectSession, CreateResult), BusError> {
    let target_handle = req_str_param(params, "target")?;
    let label = req_str_param(params, "label")?;
    validate_label(label)?;

    let def = create_project_def().map_err(|e| BusError::Internal(InternalReason::InvariantViolated, d(e)))?;
    let ft = resolve_folder(principal, &def, "target", target_handle, roots)?;
    let created = ft.create_new()?;
    let store = Store::create_at(&created).map_err(txn_error_to_bus)?;

    let project_id = draw_uuid_v7(source)?;
    let namespace = draw_uuid_v4(source)?;
    let record = IdentityRecord::create(project_id, namespace)
        .map_err(|e| BusError::Internal(InternalReason::HandlerFailed, d(e)))?;
    let record_bytes = record.to_bytes();
    let record_digest = record.digest();
    store
        .blobs()
        .put_bytes(&record_bytes, PutOpts::default())
        .map_err(|e| BusError::Internal(InternalReason::Storage, d(e)))?;

    let j_prin = to_journal_principal(principal);
    let (s_salts, g_salts) = draw_shared_salts(j_prin.on_behalf_of.is_some())?;
    let sess_id = draw_uuid_v7(source)?.to_string();

    let head = {
        let jr =
            Journal::on_store_with_clock(&store, RECORD_STREAM, Arc::clone(&clock)).map_err(journal_error_to_bus)?;
        let s_draft = session_start(
            RECORD_STREAM,
            MAIN,
            make_time(&*clock, 0),
            j_prin.clone(),
            AiAssistance::None,
            manifests,
            SessionInfo { session_id: sess_id, app_version: env!("CARGO_PKG_VERSION").to_owned() },
        );
        let mut s_req = AppendRequest::new(s_draft);
        s_req.expected_prev = Some(GENESIS_PREV);
        s_req.salts = Some(s_salts);
        let s_hash = jr.append(s_req).map_err(journal_error_to_bus)?;

        let g_args = obj([("label", text(label)), ("target", text(target_handle))]);
        let g_draft = EntryDraft {
            stream: RECORD_STREAM.to_owned(),
            branch: MAIN.to_owned(),
            logical_parent: None,
            time: make_time(&*clock, 1),
            principal: j_prin,
            ai_assistance: AiAssistance::None,
            manifests,
            reason: None,
            human_assertion: None,
            agent_coedited: false,
            body: DraftBody::Command(CommandDraft {
                id: CREATE_PROJECT.to_owned(),
                major: 1,
                class: SideEffect::W2,
                determinism: Determinism::D0,
                seed: 0,
                args: g_args,
                events: vec![event_envelope(PROJECT_IDENTIFIED, record.to_value())],
                inputs: Vec::new(),
                consumed_r1: Vec::new(),
                outputs: vec![OutputDigest { entity: Some(project_id.to_string()), digest: record_digest }],
                algorithms: Vec::new(),
                event_sources: Vec::new(),
            }),
        };
        let mut g_req = AppendRequest::new(g_draft);
        g_req.blobs = vec![record_digest];
        g_req.expected_prev = Some(s_hash);
        g_req.salts = Some(g_salts);
        jr.append(g_req).map_err(journal_error_to_bus)?
    };

    let identity = read_store_identity(&store)?;
    recent.record(target_handle, project_id, label, format_utc_rfc3339(clock.now().as_secs()));

    let res = CreateResult {
        project: project_id,
        namespace,
        head,
        label: label.to_owned(),
        handle: target_handle.to_owned(),
    };
    let session = ProjectSession {
        store,
        identity,
        label: label.to_owned(),
        handle: target_handle.to_owned(),
        read_only: false,
        branch: MAIN.to_owned(),
        commit_lock: Mutex::new(()),
        clock,
        manifests,
    };
    Ok((session, res))
}

/// Runs `project.open_project@1` (`REQ-002`, `REQ-012`, `AC-2`, `AC-8`).
pub fn open_project(
    principal: &Principal,
    roots: &Roots,
    recent: &mut RecentProjects,
    params: &CValue,
    source: &mut dyn ObservationSource,
    clock: Arc<dyn Clock>,
    manifests: Manifests,
) -> Result<(ProjectSession, OpenResult), BusError> {
    let source_handle = req_str_param(params, "source")?;
    let def = open_project_def().map_err(|e| BusError::Internal(InternalReason::InvariantViolated, d(e)))?;
    let ft = resolve_folder(principal, &def, "source", source_handle, roots)?;
    let folder = ft.open_project()?;
    let read_only = folder.is_read_only();

    let mode = if read_only { OpenMode::Read } else { OpenMode::Write };
    let store = Store::open_at(&folder, mode).map_err(txn_error_to_bus)?;

    // Fail closed on any corrupt chain or forged identity before accepting the session.
    verify_store(&store, RECORD_STREAM, &VerifyOptions::default())
        .map_err(|e| BusError::Internal(InternalReason::InvariantViolated, d(e)))?;
    let identity = read_store_identity(&store)?;
    let label = read_project_label(&store);

    if !read_only {
        let jr =
            Journal::on_store_with_clock(&store, RECORD_STREAM, Arc::clone(&clock)).map_err(journal_error_to_bus)?;
        let sess_id = draw_uuid_v7(source)?.to_string();
        let s_draft = session_start(
            RECORD_STREAM,
            MAIN,
            make_time(&*clock, 0),
            to_journal_principal(principal),
            AiAssistance::None,
            manifests,
            SessionInfo { session_id: sess_id, app_version: env!("CARGO_PKG_VERSION").to_owned() },
        );
        jr.append(AppendRequest::new(s_draft)).map_err(journal_error_to_bus)?;
    }

    let (branches, head) = read_branches_and_head(&store)?;
    recent.record(source_handle, identity.project(), &label, format_utc_rfc3339(clock.now().as_secs()));

    let res = OpenResult { project: identity.project(), namespace: identity.namespace(), branches, head, read_only };
    let session = ProjectSession {
        store,
        identity,
        label,
        handle: source_handle.to_owned(),
        read_only,
        branch: MAIN.to_owned(),
        commit_lock: Mutex::new(()),
        clock,
        manifests,
    };
    Ok((session, res))
}

/// Runs `project.close_project@1` (`REQ-003`, `REQ-012`, `AC-2`): writes a `close` checkpoint
/// (skipped on read-only sessions) and drops the store to release the OS lock.
pub fn close_project(session: ProjectSession) -> Result<CloseResult, BusError> {
    let checkpoint = if session.read_only {
        None
    } else {
        let tail_row: Option<(i64, String)> = session
            .store
            .read(|conn| {
                let mut st =
                    conn.prepare("SELECT seq, hash FROM entries WHERE stream = ?1 ORDER BY seq DESC LIMIT 1")?;
                let mut rows = st.query_map([RECORD_STREAM], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
                rows.next().transpose()
            })
            .map_err(txn_error_to_bus)?;
        let Some((seq_i, hash_s)) = tail_row else {
            return Err(BusError::Internal(InternalReason::InvariantViolated, d("cannot checkpoint empty journal")));
        };
        let seq = u64::try_from(seq_i)
            .map_err(|_| BusError::Internal(InternalReason::InvariantViolated, d("negative sequence")))?;
        let head = parse_digest(&hash_s)
            .ok_or_else(|| BusError::Internal(InternalReason::InvariantViolated, d("invalid tail hash")))?;
        let count_i: i64 = session
            .store
            .read(|conn| {
                conn.query_row("SELECT count(*) FROM entries WHERE stream = ?1", [RECORD_STREAM], |r| r.get(0))
            })
            .map_err(txn_error_to_bus)?;
        let count = u64::try_from(count_i)
            .map_err(|_| BusError::Internal(InternalReason::InvariantViolated, d("negative count")))?;
        let cp = Checkpoint {
            format: CheckpointFormat::LinearV1,
            stream: RECORD_STREAM.to_owned(),
            seq,
            head,
            count,
            ledger: None,
            reason: CheckpointReason::Close,
            time: format_utc_rfc3339(session.clock.now().as_secs()),
        };
        let body = cp.canonical_bytes().map_err(|e| BusError::Internal(InternalReason::HandlerFailed, d(e)))?;
        let mut txn = session.store.transaction();
        txn.insert(
            "checkpoints",
            &[
                ("stream", Value::Text(RECORD_STREAM.to_owned())),
                ("entry_hash", Value::Text(head.to_string())),
                ("body", Value::Blob(body)),
            ],
        );
        txn.commit().map_err(txn_error_to_bus)?;
        Some(cp)
    };
    drop(session);
    Ok(CloseResult { checkpoint })
}

/// Runs `project.save_project_as@1` (`REQ-004`, `REQ-005`, `REQ-008`..`REQ-011`, `AC-3`, `AC-4`, `AC-6`, `AC-7`).
pub fn save_project_as(
    session: &ProjectSession,
    principal: &Principal,
    roots: &Roots,
    recent: &mut RecentProjects,
    params: &CValue,
    source: &mut dyn ObservationSource,
) -> Result<SaveAsResult, BusError> {
    save_project_as_with_hook(session, principal, roots, recent, params, source, None)
}

/// [`save_project_as`] with an optional hook executed between copy finalisation and the
/// source compare-and-swap publish step, enabling deterministic testing of `conflict/head_moved`
/// (`REQ-011`, `AC-7`).
pub fn save_project_as_with_hook(
    session: &ProjectSession,
    principal: &Principal,
    roots: &Roots,
    recent: &mut RecentProjects,
    params: &CValue,
    source: &mut dyn ObservationSource,
    before_publish: Option<&dyn Fn(&ProjectSession)>,
) -> Result<SaveAsResult, BusError> {
    session.require_writable()?;
    // Review of #414: refuse save-as off branch `main` before anything is copied or written.
    if session.branch != MAIN {
        return Err(BusError::Policy(
            PolicyReason::BadRoot,
            d("project.save_project_as@1 is only allowed on branch main"),
        ));
    }

    let target_handle = req_str_param(params, "target")?;
    let label_override = opt_str_param(params, "label")?;
    if let Some(lbl) = label_override {
        validate_label(lbl)?;
    }

    let def = save_project_as_def().map_err(|e| BusError::Internal(InternalReason::InvariantViolated, d(e)))?;
    let ft = resolve_folder(principal, &def, "target", target_handle, roots)?;
    let copy_target = ft.into_copy_target()?;

    // Step 1 (Precommit): read source identity and source head H0.
    let source_project = session.identity.project();
    let namespace = session.identity.namespace();
    let (_, h0) = read_branches_and_head(&session.store)?;

    let mut copy_id = draw_uuid_v7(source)?;
    if copy_id == source_project {
        copy_id = draw_uuid_v7(source)?;
    }
    let copy_record = IdentityRecord::copy(copy_id, namespace, SourceRef { project: source_project, head: h0 })
        .map_err(|e| BusError::Internal(InternalReason::HandlerFailed, d(e)))?;
    let saved_as_record = SavedAsRecord::new(source_project, copy_id, h0)
        .map_err(|e| BusError::Internal(InternalReason::HandlerFailed, d(e)))?;

    let j_prin = to_journal_principal(principal);
    let (s_salts, g_salts) = draw_shared_salts(j_prin.on_behalf_of.is_some())?;
    let copy_sess_id = draw_uuid_v7(source)?.to_string();

    let mut args_map = BTreeMap::new();
    args_map.insert("target".to_owned(), text(target_handle));
    if let Some(lbl) = label_override {
        args_map.insert("label".to_owned(), text(lbl));
    }
    let cmd_args = CValue::Obj(args_map);

    let clock = Arc::clone(&session.clock);
    let manifests = session.manifests;
    let bus_err: Mutex<Option<BusError>> = Mutex::new(None);
    let published_path = copy_target.parent_path().join(copy_target.target_name());

    let copy_res = session.store.copy_to(
        &copy_target,
        // Step 2: finish callback on the temporary copy store (appends S and G with source.head = H0).
        |copy_store| {
            let jr = Journal::on_store_with_clock(copy_store, RECORD_STREAM, Arc::clone(&clock))
                .map_err(|e| TxnError::Store(StoreError::Io(std::io::Error::other(e.to_string()))))?;
            let snap_tail = jr
                .tail()
                .map_err(|e| TxnError::Store(StoreError::Io(std::io::Error::other(e.to_string()))))?
                .map(|t| t.hash)
                .unwrap_or(GENESIS_PREV);
            if snap_tail != h0 {
                if let Ok(mut slot) = bus_err.lock() {
                    *slot = Some(BusError::Conflict(ConflictReason::HeadMoved, d("source head moved during snapshot")));
                }
                return Err(TxnError::Store(StoreError::Io(std::io::Error::other("head_moved"))));
            }
            let copy_rec_bytes = copy_record.to_bytes();
            let copy_rec_digest = copy_record.digest();
            copy_store
                .blobs()
                .put_bytes(&copy_rec_bytes, PutOpts::default())
                .map_err(|e| TxnError::Store(StoreError::Io(std::io::Error::other(e.to_string()))))?;

            let s_draft = session_start(
                RECORD_STREAM,
                MAIN,
                make_time(&*clock, 1),
                j_prin.clone(),
                AiAssistance::None,
                manifests,
                SessionInfo { session_id: copy_sess_id.clone(), app_version: env!("CARGO_PKG_VERSION").to_owned() },
            );
            let mut s_req = AppendRequest::new(s_draft);
            s_req.expected_prev = Some(h0);
            s_req.salts = Some(s_salts.clone());
            let s_hash =
                jr.append(s_req).map_err(|e| TxnError::Store(StoreError::Io(std::io::Error::other(e.to_string()))))?;

            let g_draft = EntryDraft {
                stream: RECORD_STREAM.to_owned(),
                branch: MAIN.to_owned(),
                logical_parent: None,
                time: make_time(&*clock, 2),
                principal: j_prin.clone(),
                ai_assistance: AiAssistance::None,
                manifests,
                reason: None,
                human_assertion: None,
                agent_coedited: false,
                body: DraftBody::Command(CommandDraft {
                    id: SAVE_PROJECT_AS.to_owned(),
                    major: 1,
                    class: SideEffect::W1,
                    determinism: Determinism::D0,
                    seed: 0,
                    args: cmd_args.clone(),
                    events: vec![event_envelope(PROJECT_COPIED_FROM, copy_record.to_value())],
                    inputs: Vec::new(),
                    consumed_r1: Vec::new(),
                    outputs: vec![OutputDigest { entity: Some(copy_id.to_string()), digest: copy_rec_digest }],
                    algorithms: Vec::new(),
                    event_sources: Vec::new(),
                }),
            };
            let mut g_req = AppendRequest::new(g_draft);
            g_req.blobs = vec![copy_rec_digest];
            g_req.expected_prev = Some(s_hash);
            g_req.salts = Some(g_salts.clone());
            jr.append(g_req).map_err(|e| TxnError::Store(StoreError::Io(std::io::Error::other(e.to_string()))))?;
            Ok(())
        },
        // Steps 3-5: lock source, CAS check H == H0, atomic no-replace rename, append source ProjectSavedAs.
        |rename_fn| {
            if let Some(hook) = before_publish {
                hook(session);
            }
            let _lock = session
                .commit_lock
                .lock()
                .map_err(|_| TxnError::Store(StoreError::Io(std::io::Error::other("commit lock poisoned"))))?;
            let (_, cur_head) = read_branches_and_head(&session.store)
                .map_err(|e| TxnError::Store(StoreError::Io(std::io::Error::other(e.to_string()))))?;
            if cur_head != h0 {
                if let Ok(mut slot) = bus_err.lock() {
                    *slot = Some(BusError::Conflict(ConflictReason::HeadMoved, d("source head moved before publish")));
                }
                return Err(TxnError::Store(StoreError::Io(std::io::Error::other("head_moved"))));
            }

            // Step 4: atomic no-replace rename.
            if let Err(se) = rename_fn() {
                if let Ok(mut slot) = bus_err.lock() {
                    *slot = Some(store_error_to_bus(se));
                }
                return Err(TxnError::Store(StoreError::Io(std::io::Error::other("rename_failed"))));
            }

            // Step 5: append source ProjectSavedAs decided against H0.
            let append_source = || -> Result<(), BusError> {
                let sa_bytes = saved_as_record.to_bytes();
                let sa_digest = saved_as_record.digest();
                session
                    .store
                    .blobs()
                    .put_bytes(&sa_bytes, PutOpts::default())
                    .map_err(|e| BusError::Internal(InternalReason::Storage, d(e)))?;
                let jr = Journal::on_store_with_clock(&session.store, RECORD_STREAM, Arc::clone(&clock))
                    .map_err(journal_error_to_bus)?;
                let sa_draft = EntryDraft {
                    stream: RECORD_STREAM.to_owned(),
                    branch: MAIN.to_owned(),
                    logical_parent: None,
                    time: make_time(&*clock, 3),
                    principal: j_prin.clone(),
                    ai_assistance: AiAssistance::None,
                    manifests,
                    reason: None,
                    human_assertion: None,
                    agent_coedited: false,
                    body: DraftBody::Command(CommandDraft {
                        id: SAVE_PROJECT_AS.to_owned(),
                        major: 1,
                        class: SideEffect::W1,
                        determinism: Determinism::D0,
                        seed: 0,
                        args: cmd_args.clone(),
                        events: vec![event_envelope(PROJECT_SAVED_AS, saved_as_record.to_value())],
                        inputs: Vec::new(),
                        consumed_r1: Vec::new(),
                        outputs: vec![OutputDigest { entity: Some(source_project.to_string()), digest: sa_digest }],
                        algorithms: Vec::new(),
                        event_sources: Vec::new(),
                    }),
                };
                let mut sa_req = AppendRequest::new(sa_draft);
                sa_req.blobs = vec![sa_digest];
                sa_req.expected_prev = Some(h0);
                jr.append(sa_req).map_err(journal_error_to_bus)?;
                Ok(())
            };

            if let Err(err) = append_source() {
                let rb_msg = rollback_published(&published_path, target_handle);
                if let Ok(mut slot) = bus_err.lock() {
                    *slot = Some(match rb_msg {
                        Ok(()) => err,
                        Err(why) => BusError::Internal(
                            InternalReason::Storage,
                            d(format!("source append failed ({err}) and rollback of {target_handle} failed: {why}")),
                        ),
                    });
                }
                return Err(TxnError::Store(StoreError::Io(std::io::Error::other("source_append_failed"))));
            }
            Ok(())
        },
    );

    if let Err(te) = copy_res {
        if let Ok(mut slot) = bus_err.lock() {
            if let Some(captured) = slot.take() {
                return Err(captured);
            }
        }
        return Err(txn_error_to_bus(te));
    }

    let copy_label = label_override.unwrap_or(&session.label);
    recent.record(target_handle, copy_id, copy_label, format_utc_rfc3339(session.clock.now().as_secs()));

    Ok(SaveAsResult {
        project: copy_id,
        namespace,
        source_project,
        source_head: h0,
        target_handle: target_handle.to_owned(),
    })
}

fn rollback_published(published_path: &Path, target_handle: &str) -> Result<(), String> {
    let Some(parent) = published_path.parent() else {
        return Err(format!("no parent directory for {target_handle}"));
    };
    let rb_path = parent.join(".fid-copy-rollback");
    fs::rename(published_path, &rb_path).map_err(|e| e.to_string())?;
    fs::remove_dir_all(&rb_path).map_err(|e| e.to_string())?;
    Ok(())
}

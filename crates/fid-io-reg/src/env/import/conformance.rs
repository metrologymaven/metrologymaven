//! The shared conformance kit for commands built from the import skeleton
//! (REQ-IO.REG.ENV.IMPORT-006; feature `test-hooks`, as the faults it injects). A format leaf
//! calls [`check`] with the command under test (`import.file@1` or its `import.<format>@1`, as
//! an [`ImportRun`]), that command's registry definition and a [`Fixture`]: a project tree
//! with at least two live stages, a source file in an import root, the entity count its
//! leaf's independent reference expects, and a failure point. The kit runs the real command
//! path as a human and as an agent; only the stage sink is faulted ([`Fault`]). It fails
//! unless:
//!
//! 1. **definition**: names the command, W2, `fs_effect: read_root`, D0, undo `head_move`,
//!    effects `ElementCreated`, `ElementSlotSet` and the command's own record, a `path_like`
//!    `source` whose `x-extensions` are the command's formats' (only its own for a per-format
//!    command), `stage` and `dry_run`, and `format` exactly when the command takes one;
//! 2. **dry run**: `dry_run: true`, nothing to commit, no blob stored, the tree unchanged;
//! 3. **import**: the report equals the dry run's but for `dry_run`, the expected entity
//!    count; the commit is, per entity, an `ElementCreated` (the record's entity ID, under the
//!    root, the provenance marker in `meta`) then an `ElementSlotSet` of the same element
//!    (target stage, state `current`, a payload in the store), then the one record, and no
//!    other event kind; the record reads back from its canonical JSON, its blob manifest is
//!    stored and lists the payloads in order; the provenance names the file's real SHA-256,
//!    size and name, the command's format and how it was chosen, the caller's principal
//!    commitment and the effective options, and matches its digest; every stage but the
//!    target keeps its per-stage digest;
//! 4. **named stage**: with `stage` set to the other stage's label, only that stage changes;
//! 5. **unknown and ambiguous stage**: `stage_not_found`, and `stage_ambiguous` once a second
//!    stage carries the other stage's label, even for a source that does not exist (the
//!    source is never opened);
//! 6. **failure** after [`Fixture::fail_after`] entities, **cancel** before the start and
//!    **mid-read**: each run goes through prepare and (had it succeeded) commit on a working
//!    copy of the project; that copy keeps its digest and references none of the blobs the
//!    failed run stored. A commit refused by the tree (the same import into a project that
//!    already holds it) leaves that project unchanged;
//! 7. **principals**: an agent's raw path is `raw_path_forbidden`; a human's raw path (when
//!    the fixture gives one) imports (dry run);
//! 8. **replay** of the committed record against the state before it is `IDENTICAL`;
//! 9. **no double import**: the same import into the same stage again is refused
//!    (`conflict/id_collision`) and changes nothing.

use super::{
    blob_manifest, provenance_meta, BlobSink, Fault, ImportEnv, ImportError, ImportEvent, ImportParams, ImportRun,
    MemoryBlobs, PreparedImport, IMPORT_FILE,
};
use crate::detect::{OriginFilter, Registry};
use crate::limits::Principal;
use crate::prov::{PrincipalCommit, SCHEMA};
use crate::roots::{Direction, RootSet};
use fid_canon::CValue;
use fid_project::{Tree, TreeEvent, Uuid, ELEMENT_CREATED, ELEMENT_SLOT_SET, STAGE_ADDED};
use fid_registry::CommandDef;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::sync::atomic::AtomicBool;

/// What the kit runs against.
pub struct Fixture<'a> {
    pub registry: &'a Registry,
    pub roots: &'a RootSet,
    /// The project: at least two live stages; the active one is the default target.
    pub tree: &'a Tree,
    pub namespace: Uuid,
    pub principal_commit: PrincipalCommit,
    /// The source, as a `root://` handle.
    pub source: String,
    /// The same file as a raw path a human may give, if the platform allows one.
    pub raw_path: Option<String>,
    /// The label of a live stage other than the active one.
    pub other_stage_label: String,
    /// Entities the leaf's reference expects.
    pub expected_entities: u64,
    /// Entities after which the faulted sink fails (fewer than `expected_entities`).
    pub fail_after: u64,
}

fn env<'a>(fx: &'a Fixture<'a>, principal: Principal) -> ImportEnv<'a> {
    ImportEnv::new(fx.registry, fx.roots, principal, fx.principal_commit, fx.tree, fx.namespace)
}

fn fail(step: &str, m: impl std::fmt::Display) -> String {
    format!("conformance {step}: {m}")
}

fn digest(t: &Tree) -> Result<String, String> {
    t.state_digest().map(|d| d.as_str().to_string()).map_err(|e| fail("digest", e))
}

fn stage_digests(t: &Tree) -> Result<Vec<(Uuid, String)>, String> {
    t.live_stages()
        .into_iter()
        .map(|s| t.stage_digest(&s).map(|d| (s, d.as_str().to_string())).map_err(|e| fail("stage digest", e)))
        .collect()
}

fn obj<'v>(step: &str, v: &'v CValue) -> Result<&'v BTreeMap<String, CValue>, String> {
    match v {
        CValue::Obj(o) => Ok(o),
        _ => Err(fail(step, "not an object")),
    }
}

fn text<'v>(o: &'v BTreeMap<String, CValue>, k: &str) -> &'v str {
    match o.get(k) {
        Some(CValue::Str(s)) => s.as_str(),
        _ => "",
    }
}

fn str_list(v: Option<&CValue>) -> BTreeSet<String> {
    match v {
        Some(CValue::Arr(a)) => a
            .iter()
            .filter_map(|x| match x {
                CValue::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => BTreeSet::new(),
    }
}

fn check_definition(cmd: &dyn ImportRun, def: &CommandDef, reg: &Registry) -> Result<(), String> {
    let id = cmd.command_id().map_err(|e| fail("definition", e))?;
    let v = def.to_value();
    let o = obj("definition", &v)?;
    if format!("{}@{}", def.id, def.major) != id.as_str() {
        return Err(fail("definition", "names another command"));
    }
    if (text(o, "side_effect"), text(o, "fs_effect"), text(o, "determinism"), text(o, "undo"))
        != ("W2", "read_root", "D0", "head_move")
    {
        return Err(fail("definition", "must be W2, read_root, D0, head_move"));
    }
    for e in [ELEMENT_CREATED, ELEMENT_SLOT_SET, id.as_str()] {
        if !def.effects.iter().any(|x| x == e) {
            return Err(fail("definition", format!("effect {e} not declared")));
        }
    }
    let ps = obj("definition", &def.params_schema)?;
    let Some(CValue::Obj(props)) = ps.get("properties") else { return Err(fail("definition", "no properties")) };
    let Some(CValue::Obj(source)) = props.get("source") else { return Err(fail("definition", "no source")) };
    if source.get("path_like") != Some(&CValue::Bool(true))
        || !props.contains_key("stage")
        || !props.contains_key("dry_run")
    {
        return Err(fail("definition", "source (path_like), stage and dry_run are required properties"));
    }
    let want: BTreeSet<String> = reg
        .list(Direction::Import, OriginFilter::Any)
        .into_iter()
        .filter(|d| cmd.fixed_format().map_or(true, |f| d.id() == f))
        .flat_map(|d| d.extensions().iter().cloned())
        .collect();
    if str_list(source.get("x-extensions")) != want {
        return Err(fail("definition", "x-extensions are not exactly the command's formats' extensions"));
    }
    if props.contains_key("format") != cmd.takes_format() {
        return Err(fail("definition", "format present exactly when the command takes one"));
    }
    if cmd.takes_format() != (id.as_str() == IMPORT_FILE) || cmd.takes_format() == cmd.fixed_format().is_some() {
        return Err(fail("definition", "only import.file@1 takes a format"));
    }
    Ok(())
}

fn expect_err(step: &str, r: Result<PreparedImport, ImportError>, code: &str) -> Result<ImportError, String> {
    match r {
        Ok(_) => Err(fail(step, format!("accepted; expected {code}"))),
        Err(e) if e.code() == code => Ok(e),
        Err(e) => Err(fail(step, format!("{e}; expected {code}"))),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let d: [u8; 32] = Sha256::digest(bytes).into();
    Digest::from_bytes(d).as_str().to_string()
}

/// Step 3's checks of the commit's structure against the record and the store.
fn check_commit(
    step: &str,
    events: &[TreeEvent],
    rec: &ImportEvent,
    root: Uuid,
    blobs: &MemoryBlobs,
    evidence: &str,
) -> Result<(), String> {
    let n = rec.entities.len();
    if events.len() != 2 * n + 1 || rec.blob_count != n as u64 {
        return Err(fail(step, "the commit is not two events per entity then one record"));
    }
    let meta = provenance_meta(evidence, &rec.provenance_sha256);
    let mut payloads = Vec::with_capacity(n);
    for (i, id) in rec.entities.iter().enumerate() {
        let (c, s) = (&events[2 * i], &events[2 * i + 1]);
        if c.event_type != ELEMENT_CREATED || s.event_type != ELEMENT_SLOT_SET {
            return Err(fail(step, format!("entity {i}: not ElementCreated then ElementSlotSet")));
        }
        let co = obj(step, &c.body)?;
        let id = id.to_string();
        if text(co, "id") != id
            || text(co, "parent") != root.to_string()
            || co.get("meta") != Some(&meta)
            || co.get("visible") != Some(&CValue::Bool(true))
        {
            return Err(fail(step, format!("entity {i}: ElementCreated (id, parent, provenance marker, visible)")));
        }
        let so = obj(step, &s.body)?;
        if text(so, "element") != id || text(so, "stage") != rec.stage.to_string() || text(so, "state") != "current" {
            return Err(fail(step, format!("entity {i}: ElementSlotSet (element, target stage, current)")));
        }
        let d = Digest::parse(text(so, "digest")).map_err(|_| fail(step, format!("entity {i}: slot digest")))?;
        if !blobs.blobs.contains_key(&d) {
            return Err(fail(step, format!("entity {i}: the payload is not in the store")));
        }
        payloads.push(d);
    }
    let last = &events[2 * n];
    if last.event_type != rec.command.as_str() || last.body != rec.to_value() {
        return Err(fail(step, "the last event is not the record"));
    }
    let manifest = blob_manifest(&payloads).map_err(|e| fail(step, e))?;
    if blobs.blobs.get(&rec.blob_manifest) != Some(&manifest) {
        return Err(fail(step, "the blob manifest is not stored or does not list the payloads in order"));
    }
    let json = rec.to_json().map_err(|e| fail(step, e))?;
    if ImportEvent::from_json(&json).map_err(|e| fail(step, e))? != *rec {
        return Err(fail(step, "the record does not read back"));
    }
    Ok(())
}

/// Step 3's checks of the provenance record against the real file.
fn check_provenance(cmd: &dyn ImportRun, fx: &Fixture<'_>, rec: &ImportEvent) -> Result<String, String> {
    let step = "provenance";
    let mut f = fx.roots.resolve_import(Principal::Human, &fx.source).map_err(|e| fail(step, e))?;
    let mut bytes = Vec::new();
    f.file.read_to_end(&mut bytes).map_err(|e| fail(step, e))?;
    let p = obj(step, &rec.provenance)?;
    let canon = fid_canon::to_canonical_bytes(&rec.provenance).map_err(|e| fail(step, e))?;
    if sha256_hex(&canon) != rec.provenance_sha256.as_str() {
        return Err(fail(step, "does not match provenance_sha256"));
    }
    let name = f.path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let selected = if cmd.takes_format() { "content_signature" } else { "caller" };
    let size = bytes.len().to_string();
    let checks = [
        ("schema", SCHEMA.to_string()),
        ("sha256", sha256_hex(&bytes)),
        ("bytes", size),
        ("source_name", name),
        ("format", rec.format.as_str().to_string()),
        ("format_selected_by", selected.to_string()),
    ];
    for (k, want) in checks {
        if text(p, k) != want {
            return Err(fail(step, format!("{k} is not {want}")));
        }
    }
    if cmd.fixed_format().is_some_and(|f| *f != rec.format) {
        return Err(fail(step, "another format than the command's"));
    }
    let principal = obj(step, p.get("principal").unwrap_or(&CValue::Null))?;
    if text(principal, "kind") != fx.principal_commit.kind.as_str()
        || text(principal, "commit") != fx.principal_commit.commit.to_string()
    {
        return Err(fail(step, "not the caller's principal commitment"));
    }
    if p.get("options") != Some(&rec.options) {
        return Err(fail(step, "not the effective options"));
    }
    let evidence = text(p, "evidence");
    if !["uncharacterized", "claimed"].contains(&evidence) {
        return Err(fail(step, "evidence must be uncharacterized or claimed"));
    }
    Ok(evidence.to_string())
}

/// A copy of `t` with one more live stage labelled `label`.
fn with_second_label(t: &Tree, ns: Uuid, label: &str) -> Result<Tree, String> {
    let step = "ambiguous stage";
    let run = t.stage(&t.active_stage()).map(|s| s.run()).ok_or_else(|| fail(step, "no active stage record"))?;
    let id = fid_project::uuid_v5(&ns, b"io.reg.env.import conformance: second label");
    let mut body = BTreeMap::new();
    body.insert("adopts".to_string(), CValue::Null);
    body.insert("id".to_string(), CValue::Str(id.to_string()));
    body.insert("label".to_string(), CValue::Str(label.to_string()));
    body.insert("metadata".to_string(), CValue::Obj(BTreeMap::new()));
    body.insert("run".to_string(), CValue::Str(run.to_string()));
    body.insert("serial".to_string(), CValue::Null);
    let mut out = t.clone();
    out.apply(STAGE_ADDED, &CValue::Obj(body)).map_err(|e| fail(step, e))?;
    Ok(out)
}

/// Step 6: a failing run, through prepare and commit, on a working copy of the project.
fn failing_run(
    cmd: &dyn ImportRun,
    fx: &Fixture<'_>,
    step: &str,
    (cancel, fault): (Option<&AtomicBool>, Option<Fault>),
    code: Option<&str>,
    before: &str,
) -> Result<(), String> {
    let mut work = fx.tree.clone();
    let mut stored = MemoryBlobs::default();
    let r = {
        let mut e = ImportEnv { tree: &work, ..env(fx, Principal::Agent) };
        e.cancel = cancel;
        e.set_fault(fault);
        cmd.prepare(&e, &ImportParams::new(&fx.source), Some(&mut stored as &mut dyn BlobSink))
    };
    let outcome = r.and_then(|p| p.commit(&mut work));
    match (outcome, code) {
        (Ok(_), _) => return Err(fail(step, "the import did not fail")),
        (Err(e), Some(c)) if e.code() != c => return Err(fail(step, format!("{e}; expected {c}"))),
        (Err(_), _) => {}
    }
    if digest(&work)? != before {
        return Err(fail(step, "the project changed"));
    }
    for e in work.elements() {
        for s in e.slots().values() {
            if s.digest.is_some_and(|d| stored.blobs.contains_key(&d)) {
                return Err(fail(step, "a failed run's blob is referenced"));
            }
        }
    }
    Ok(())
}

/// Runs the kit (see the module docs).
pub fn check(cmd: &dyn ImportRun, def: &CommandDef, fx: &Fixture<'_>) -> Result<(), String> {
    check_definition(cmd, def, fx.registry)?;
    if fx.tree.live_stages().len() < 2 || fx.fail_after >= fx.expected_entities {
        return Err(fail("fixture", "needs two live stages and fail_after < expected_entities"));
    }
    let before = digest(fx.tree)?;
    let stages_before = stage_digests(fx.tree)?;
    let human = env(fx, Principal::Human);
    let agent = env(fx, Principal::Agent);

    // 2. Dry run.
    let mut blobs = MemoryBlobs::default();
    let mut p = ImportParams::new(&fx.source);
    p.dry_run = true;
    let dry = cmd.prepare(&agent, &p, Some(&mut blobs as &mut dyn BlobSink)).map_err(|e| fail("dry run", e))?;
    if !dry.report().dry_run() || !dry.events().is_empty() || !blobs.blobs.is_empty() {
        return Err(fail("dry run", "committed something, stored a blob or did not say dry_run"));
    }
    let mut t = fx.tree.clone();
    let dry_out = dry.commit(&mut t).map_err(|e| fail("dry run", e))?;
    if digest(&t)? != before {
        return Err(fail("dry run", "the tree changed"));
    }

    // 3. Import.
    let p = ImportParams::new(&fx.source);
    let mut blobs = MemoryBlobs::default();
    let prep = cmd.prepare(&agent, &p, Some(&mut blobs as &mut dyn BlobSink)).map_err(|e| fail("import", e))?;
    let events = prep.events().to_vec();
    let mut committed = fx.tree.clone();
    let out = prep.commit(&mut committed).map_err(|e| fail("import", e))?;
    let total: u64 = out.report.entity_counts().values().sum();
    if total != fx.expected_entities {
        return Err(fail("import", format!("{total} entities, expected {}", fx.expected_entities)));
    }
    if out.event != dry_out.event || out.report.to_cvalue() == dry_out.report.to_cvalue() {
        return Err(fail("import", "the dry run's record or report differs beyond dry_run"));
    }
    let evidence = check_provenance(cmd, fx, &out.event)?;
    check_commit("import", &events, &out.event, fx.tree.root(), &blobs, &evidence)?;
    let target = out.event.stage;
    if target != fx.tree.active_stage() {
        return Err(fail("import", "not into the active stage"));
    }
    for (s, d) in &stages_before {
        let now = committed.stage_digest(s).map_err(|e| fail("import", e))?;
        if *s != target && now.as_str() != d {
            return Err(fail("import", "another stage's data changed"));
        }
    }
    for id in &out.event.entities {
        let e = committed.element(id).ok_or_else(|| fail("import", "an entity is not in the project"))?;
        if e.slots().get(&target).and_then(|s| s.digest).is_none() {
            return Err(fail("import", "an entity holds no data in the target stage"));
        }
    }

    // 4. Named stage.
    let mut p = ImportParams::new(&fx.source);
    p.stage = Some(fx.other_stage_label.clone());
    let prep = cmd.prepare(&agent, &p, None).map_err(|e| fail("named stage", e))?;
    let named_stage = prep.event().stage;
    let mut t = fx.tree.clone();
    prep.commit(&mut t).map_err(|e| fail("named stage", e))?;
    if named_stage == fx.tree.active_stage() {
        return Err(fail("named stage", "imported into the active stage"));
    }
    for (s, d) in &stages_before {
        let now = t.stage_digest(s).map_err(|e| fail("named stage", e))?;
        if *s != named_stage && now.as_str() != d {
            return Err(fail("named stage", "another stage's data changed"));
        }
    }

    // 5. Unknown and ambiguous stage: refused before the (missing) source is opened.
    let mut p = ImportParams::new("root://kit-missing-root/does/not/exist.bin");
    p.stage = Some("NOPE".to_string());
    expect_err("unknown stage", cmd.prepare(&agent, &p, None), "stage_not_found")?;
    let twice = with_second_label(fx.tree, fx.namespace, &fx.other_stage_label)?;
    let mut p = ImportParams::new("root://kit-missing-root/does/not/exist.bin");
    p.stage = Some(fx.other_stage_label.clone());
    let amb = ImportEnv { tree: &twice, ..env(fx, Principal::Agent) };
    let e = expect_err("ambiguous stage", cmd.prepare(&amb, &p, None), "stage_ambiguous")?;
    if e.bus_code() != ("validation", "bad_stage") {
        return Err(fail("ambiguous stage", "not validation/bad_stage"));
    }

    // 6. Failure and cancels, against the real state after the attempt.
    let fail_after = fx.fail_after;
    failing_run(cmd, fx, "failure", (None, Some(Fault::FailAfter(fail_after))), None, &before)?;
    let flag = AtomicBool::new(true);
    failing_run(cmd, fx, "cancel before", (Some(&flag), None), Some("cancelled"), &before)?;
    failing_run(cmd, fx, "cancel mid-read", (None, Some(Fault::CancelAfter(1))), Some("cancelled"), &before)?;
    // A commit the tree refuses leaves that project unchanged.
    let again = cmd.prepare(&agent, &ImportParams::new(&fx.source), None).map_err(|e| fail("refused commit", e))?;
    let mut refused = committed.clone();
    let committed_digest = digest(&committed)?;
    if again.commit(&mut refused).is_ok() || digest(&refused)? != committed_digest {
        return Err(fail("refused commit", "a colliding commit was applied or changed the project"));
    }

    // 7. Principals.
    if let Some(raw) = &fx.raw_path {
        expect_err("agent raw path", cmd.prepare(&agent, &ImportParams::new(raw), None), "raw_path_forbidden")?;
        let mut p = ImportParams::new(raw);
        p.dry_run = true;
        cmd.prepare(&human, &p, None).map_err(|e| fail("human raw path", e))?;
    }

    // 8. Replay.
    match cmd.replay(&agent, &out.event, &events).map_err(|e| fail("replay", e))? {
        ReplayOutcome::Identical => {}
        other => return Err(fail("replay", format!("{other:?}"))),
    }

    // 9. No double import.
    let again = ImportEnv { tree: &committed, ..env(fx, Principal::Agent) };
    let e = expect_err("double import", cmd.prepare(&again, &ImportParams::new(&fx.source), None), "id_collision")?;
    if e.bus_code() != ("conflict", "id_collision") {
        return Err(fail("double import", "not conflict/id_collision"));
    }
    if digest(&committed)? != committed_digest {
        return Err(fail("double import", "the project changed"));
    }
    Ok(())
}

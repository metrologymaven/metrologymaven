//! The shared conformance kit for commands built from the import skeleton
//! (REQ-IO.REG.ENV.IMPORT-006). A format leaf calls [`check`] with the command under test
//! (`import.file@1` or its `import.<format>@1`, as an [`ImportRun`]), that command's registry
//! definition and a [`Fixture`]: a project tree with at least two live stages, a source
//! file in an import root, the entity count its leaf's independent reference expects, and a
//! failure point. The kit runs the real command path as a human and as an agent; only the
//! stage sink is faulted ([`Fault`]). It fails unless:
//!
//! 1. **definition**: names the command, W2, `fs_effect: read_root`, D0, undo `head_move`,
//!    effects `ElementCreated`, `ElementSlotSet` and the command's own record, a `path_like`
//!    `source`, `stage` and `dry_run`, and `format` exactly when the command takes one;
//! 2. **dry run**: `dry_run: true`, nothing to commit, no blob stored, the tree unchanged;
//! 3. **import**: the report equals the dry run's but for `dry_run`, the expected entity
//!    count, one commit whose last event is the record (it reads back from its canonical
//!    JSON), every payload in the store, and every stage but the target with an unchanged
//!    per-stage digest;
//! 4. **named stage**: with `stage` set to the other stage's label, only that stage changes;
//! 5. **unknown stage**: `stage_not_found` even for a source that does not exist (the source
//!    is never opened);
//! 6. **failure** after [`Fixture::fail_after`] entities, **cancel** before the start and
//!    **mid-read**: the import fails, nothing to commit, the tree unchanged, and the blobs
//!    the failed run stored are referenced by nothing;
//! 7. **principals**: an agent's raw path is `raw_path_forbidden`; a human's raw path (when
//!    the fixture gives one) imports (dry run);
//! 8. **replay** of the committed record against the state before it is `IDENTICAL`;
//! 9. **no double import**: the same import into the same stage again is refused
//!    (`conflict/id_collision`) and changes nothing.

use super::{BlobSink, Fault, ImportEnv, ImportError, ImportEvent, ImportParams, ImportRun, MemoryBlobs, IMPORT_FILE};
use crate::detect::Registry;
use crate::limits::{LimitRequest, Principal};
use crate::prov::PrincipalCommit;
use crate::roots::RootSet;
use fid_canon::CValue;
use fid_project::{Tree, Uuid, ELEMENT_CREATED, ELEMENT_SLOT_SET};
use fid_registry::CommandDef;
use fid_replay::ReplayOutcome;
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
    ImportEnv {
        registry: fx.registry,
        roots: fx.roots,
        principal,
        principal_commit: fx.principal_commit,
        limits: LimitRequest::default(),
        tree: fx.tree,
        namespace: fx.namespace,
        cancel: None,
        fault: None,
    }
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

fn check_definition(cmd: &dyn ImportRun, def: &CommandDef) -> Result<(), String> {
    let id = cmd.command_id().map_err(|e| fail("definition", e))?;
    let v = def.to_value();
    let CValue::Obj(o) = &v else { return Err(fail("definition", "not an object")) };
    let get = |k: &str| match o.get(k) {
        Some(CValue::Str(s)) => s.as_str(),
        _ => "",
    };
    if format!("{}@{}", def.id, def.major) != id.as_str() {
        return Err(fail("definition", "names another command"));
    }
    if (get("side_effect"), get("fs_effect"), get("determinism"), get("undo")) != ("W2", "read_root", "D0", "head_move")
    {
        return Err(fail("definition", "must be W2, read_root, D0, head_move"));
    }
    for e in [ELEMENT_CREATED, ELEMENT_SLOT_SET, id.as_str()] {
        if !def.effects.iter().any(|x| x == e) {
            return Err(fail("definition", format!("effect {e} not declared")));
        }
    }
    let CValue::Obj(ps) = &def.params_schema else { return Err(fail("definition", "params schema")) };
    let Some(CValue::Obj(props)) = ps.get("properties") else { return Err(fail("definition", "no properties")) };
    let path_like =
        matches!(props.get("source"), Some(CValue::Obj(s)) if s.get("path_like") == Some(&CValue::Bool(true)));
    if !path_like || !props.contains_key("stage") || !props.contains_key("dry_run") {
        return Err(fail("definition", "source (path_like), stage and dry_run are required properties"));
    }
    if props.contains_key("format") != cmd.takes_format() {
        return Err(fail("definition", "format present exactly when the command takes one"));
    }
    if cmd.takes_format() != (id.as_str() == IMPORT_FILE) {
        return Err(fail("definition", "only import.file@1 takes a format"));
    }
    Ok(())
}

fn expect_err(step: &str, r: Result<super::PreparedImport, ImportError>, code: &str) -> Result<ImportError, String> {
    match r {
        Ok(_) => Err(fail(step, format!("accepted; expected {code}"))),
        Err(e) if e.code() == code => Ok(e),
        Err(e) => Err(fail(step, format!("{e}; expected {code}"))),
    }
}

/// Runs the kit (see the module docs).
pub fn check(cmd: &dyn ImportRun, def: &CommandDef, fx: &Fixture<'_>) -> Result<(), String> {
    check_definition(cmd, def)?;
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
    let last = events.last().ok_or_else(|| fail("import", "no events"))?;
    if last.event_type != out.event.command.as_str() || events.len() as u64 != 2 * total + 1 {
        return Err(fail("import", "the commit is not the entity events then one record"));
    }
    let json = out.event.to_json().map_err(|e| fail("import", e))?;
    if ImportEvent::from_json(&json).map_err(|e| fail("import", e))? != out.event {
        return Err(fail("import", "the record does not read back"));
    }
    if out.event.blobs.iter().any(|d| !blobs.blobs.contains_key(d)) {
        return Err(fail("import", "a payload is not in the store"));
    }
    let target = out.event.stage;
    for (s, d) in &stages_before {
        let now = committed.stage_digest(s).map_err(|e| fail("import", e))?;
        if *s != target && now.as_str() != d {
            return Err(fail("import", "another stage's data changed"));
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

    // 5. Unknown stage: refused before the (missing) source is opened.
    let mut p = ImportParams::new("root://kit-missing-root/does/not/exist.bin");
    p.stage = Some("NOPE".to_string());
    expect_err("unknown stage", cmd.prepare(&agent, &p, None), "stage_not_found")?;

    // 6. Failure and cancels.
    let mut faulted = env(fx, Principal::Agent);
    faulted.fault = Some(Fault::FailAfter(fx.fail_after));
    let mut failed_blobs = MemoryBlobs::default();
    let r = cmd.prepare(&faulted, &ImportParams::new(&fx.source), Some(&mut failed_blobs as &mut dyn BlobSink));
    if r.is_ok() {
        return Err(fail("failure", "a failing sink did not fail the import"));
    }
    let flag = AtomicBool::new(true);
    let mut cancelled = env(fx, Principal::Agent);
    cancelled.cancel = Some(&flag);
    expect_err("cancel before", cmd.prepare(&cancelled, &ImportParams::new(&fx.source), None), "cancelled")?;
    let mut mid = env(fx, Principal::Agent);
    mid.fault = Some(Fault::CancelAfter(1));
    expect_err("cancel mid-read", cmd.prepare(&mid, &ImportParams::new(&fx.source), None), "cancelled")?;
    if digest(fx.tree)? != before {
        return Err(fail("failure", "the tree changed"));
    }
    for e in fx.tree.elements() {
        for s in e.slots().values() {
            if s.digest.is_some_and(|d| failed_blobs.blobs.contains_key(&d)) {
                return Err(fail("failure", "a failed run's blob is referenced"));
            }
        }
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
    Ok(())
}

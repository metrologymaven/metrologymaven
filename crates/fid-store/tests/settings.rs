//! Spec node CORE.STO.SET: AC-1 .. AC-7, the declaration checks (REQ-001), the fail-closed
//! rules of the change pipeline (REQ-005, -006, -009, -010) and the command schemas.
//!
//! Expected values come from `fixtures/core/sto/set/make_fixtures.py` (a reference resolver
//! and hand-written schemas, independent of this crate), which also validates every expected
//! result against the golden result schemas with `jsonschema`. These tests show that the
//! crate's schemas equal the golden ones and its results equal the expected ones, so the real
//! results validate against the published schemas. The project-database store is tested in
//! `settings_table.rs`; here project values come from an in-memory [`ProjectValues`].
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_registry::{check_schema, validate, Catalogue, Projection, SideEffect, Surface};
use fid_store::fid_settings;
use fid_store::settings::{
    declarations, encode_record, invoke, is_agent_branch, list_to_value, list_values, machine_config_dir,
    replay_outcome, reset_value, set_admin_value, set_value, user_config_dir, validate_value, ActivityLog, Caller,
    ChangeEntry, FileStore, Invocation, JournalTarget, ListValuesParams, PrincipalKind, ProjectScope, ProjectValues,
    ReplayOutcome, ResetValueParams, Scope, SetValueParams, SettingValue, SettingWarning, SettingsCtx, SettingsError,
    SettingsJournal, SettingsRegistry, WarningReason,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/sto/set");
const LENGTH: &str = "probe.approach_distance";
const TIPS: &str = "ui.show_tips";
const ROOTS: &str = "net.allowed_roots";

// ------------------------------------------------------------------ helpers

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-set-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        let p = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(FIXTURES).join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// An expected file: canonical JSON plus one newline (make_fixtures.py).
fn expected(name: &str) -> CValue {
    parse(fixture(name).strip_suffix(b"\n").expect("trailing newline")).expect("expected file parses")
}

fn canonical(v: &CValue) -> String {
    String::from_utf8(to_canonical_bytes(v).unwrap()).unwrap()
}

fn registry() -> SettingsRegistry {
    SettingsRegistry::from_manifest(&fixture("decls.json"), &Catalogue::geo_frame().unwrap()).unwrap()
}

/// Project values in memory; the journal below stages rows into it as the real journal
/// would into `settings_current`, in the commit.
#[derive(Default)]
struct MemProject(RefCell<BTreeMap<String, Vec<u8>>>);

impl ProjectValues for MemProject {
    fn record(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self.0.borrow().get(key).cloned())
    }
}

/// Records every commit: (target, canonical entry bytes, entry).
#[derive(Default)]
struct RecJournal {
    entries: RefCell<Vec<(JournalTarget, Vec<u8>, ChangeEntry)>>,
    fail: bool,
}

struct Journal<'a> {
    rec: &'a RecJournal,
    project: &'a MemProject,
}

impl SettingsJournal for Journal<'_> {
    fn commit(&self, target: &JournalTarget, entry: &ChangeEntry) -> Result<(), String> {
        if self.rec.fail {
            return Err("journal offline".into());
        }
        if let JournalTarget::Project { .. } = target {
            self.project.0.borrow_mut().insert(entry.key().to_string(), encode_record(entry.value()).unwrap());
        }
        let bytes = to_canonical_bytes(&entry.to_value()).unwrap();
        self.rec.entries.borrow_mut().push((target.clone(), bytes, entry.clone()));
        Ok(())
    }
}

#[derive(Default)]
struct Log(RefCell<Vec<SettingWarning>>);

impl ActivityLog for Log {
    fn warn(&self, w: &SettingWarning) {
        self.0.borrow_mut().push(w.clone());
    }
}

/// One install: user and machine files in a temp dir, an in-memory project, a journal.
struct Env {
    _dir: TempDir,
    registry: SettingsRegistry,
    user: FileStore,
    machine: FileStore,
    project: MemProject,
    rec: RecJournal,
    log: Log,
}

/// The project a call sees: `None` outside any project, else (branch, writable).
type Proj<'a> = Option<(&'a str, bool)>;

impl Env {
    fn new(tag: &str) -> Env {
        let dir = TempDir::new(tag);
        let user = FileStore::new(Scope::User, dir.0.join("user").join("fiducial").join("settings.json")).unwrap();
        let machine =
            FileStore::new(Scope::Machine, dir.0.join("machine").join("fiducial").join("settings.json")).unwrap();
        Env {
            _dir: dir,
            registry: registry(),
            user,
            machine,
            project: MemProject::default(),
            rec: RecJournal::default(),
            log: Log::default(),
        }
    }

    fn run<T>(&self, caller: Caller, proj: Proj<'_>, f: impl FnOnce(&SettingsCtx<'_>) -> T) -> T {
        let journal = Journal { rec: &self.rec, project: &self.project };
        let ctx = SettingsCtx {
            registry: &self.registry,
            caller,
            project: proj.map(|(branch, writable)| ProjectScope {
                values: &self.project,
                branch: branch.to_string(),
                writable,
            }),
            user: &self.user,
            machine: &self.machine,
            journal: &journal,
            log: &self.log,
        };
        f(&ctx)
    }

    fn list(&self, caller: Caller, proj: Proj<'_>) -> CValue {
        self.run(caller, proj, |ctx| list_to_value(&list_values(ctx, ListValuesParams::default()).unwrap()))
    }

    fn set(&self, caller: Caller, proj: Proj<'_>, key: &str, scope: Scope, v: CValue) -> Result<CValue, SettingsError> {
        let p = SetValueParams { key: key.into(), scope, value: SettingValue(v) };
        self.run(caller, proj, |ctx| set_value(ctx, p).map(|r| r.to_value()))
    }

    fn reset(&self, caller: Caller, proj: Proj<'_>, key: &str, scope: Scope) -> Result<CValue, SettingsError> {
        let p = ResetValueParams { key: key.into(), scope };
        self.run(caller, proj, |ctx| reset_value(ctx, p).map(|r| r.to_value()))
    }

    fn set_admin(
        &self,
        caller: Caller,
        proj: Proj<'_>,
        key: &str,
        scope: Scope,
        v: CValue,
    ) -> Result<CValue, SettingsError> {
        let p = SetValueParams { key: key.into(), scope, value: SettingValue(v) };
        self.run(caller, proj, |ctx| set_admin_value(ctx, p).map(|r| r.to_value()))
    }

    fn journaled(&self) -> usize {
        self.rec.entries.borrow().len()
    }
}

const fn caller(kind: PrincipalKind, surface: Surface) -> Caller {
    Caller { kind, surface }
}

const HUMAN: Caller = caller(PrincipalKind::Human, Surface::Cli);
const SCRIPT: Caller = caller(PrincipalKind::Script, Surface::Cli);
const AGENT: Caller = caller(PrincipalKind::Agent, Surface::Mcp);
const MAIN: Proj<'static> = Some(("main", true));
const AGENT_BRANCH: Proj<'static> = Some(("agent/claude/1", true));

fn num(n: f64) -> CValue {
    CValue::Num(n)
}

fn roots() -> CValue {
    CValue::Arr(vec![CValue::Str("/data/exports".into())])
}

fn every_caller() -> Vec<Caller> {
    let kinds = [PrincipalKind::Human, PrincipalKind::Agent, PrincipalKind::Script];
    let surfaces = [Surface::Cli, Surface::Mcp, Surface::Python, Surface::Gui, Surface::Control];
    kinds.iter().flat_map(|k| surfaces.iter().map(move |s| caller(*k, *s))).collect()
}

// ------------------------------------------------------------------ acceptance criteria

#[test]
fn ac1_fresh_install_lists_every_default() {
    let env = Env::new("ac1");
    assert_eq!(canonical(&env.list(HUMAN, None)), canonical(&expected("expected/list_fresh.json")));
    assert_eq!(canonical(&env.list(AGENT, MAIN)), canonical(&expected("expected/list_fresh.json")));
    assert!(env.log.0.borrow().is_empty());
}

#[test]
fn ac2_project_value_wins_inside_the_project_user_value_outside() {
    let env = Env::new("ac2");
    std::fs::create_dir_all(env.user.path().parent().unwrap()).unwrap();
    std::fs::write(env.user.path(), fixture("user_ac2.json")).unwrap();
    env.project.0.borrow_mut().insert(LENGTH.into(), encode_record(Some(&num(0.2))).unwrap());
    assert_eq!(canonical(&env.list(HUMAN, MAIN)), canonical(&expected("expected/list_ac2_project.json")));
    assert_eq!(canonical(&env.list(HUMAN, None)), canonical(&expected("expected/list_ac2_outside.json")));
    assert!(env.log.0.borrow().is_empty());
}

#[test]
fn ac3_invalid_user_value_falls_back_with_one_warning() {
    let env = Env::new("ac3");
    std::fs::create_dir_all(env.user.path().parent().unwrap()).unwrap();
    std::fs::write(env.user.path(), fixture("user_bad_length.json")).unwrap();
    assert_eq!(canonical(&env.list(HUMAN, None)), canonical(&expected("expected/list_ac3.json")));
    let warnings: Vec<CValue> = env
        .log
        .0
        .borrow()
        .iter()
        .map(|w| {
            CValue::Obj(BTreeMap::from([
                ("key".to_string(), CValue::Str(w.key.clone().unwrap())),
                ("reason".to_string(), CValue::Str(w.reason.code().into())),
                ("scope".to_string(), CValue::Str(w.scope.as_str().into())),
            ]))
        })
        .collect();
    assert_eq!(canonical(&CValue::Arr(warnings)), canonical(&expected("expected/warnings_ac3.json")));
    // The invalid value is never the setting's value, also with a project open.
    let rows = env.list(HUMAN, MAIN);
    let CValue::Arr(rows) = rows else { panic!() };
    let CValue::Obj(row) = &rows[1] else { panic!() };
    assert_eq!(row.get("value"), Some(&num(2.0)));
    assert_eq!(row.get("source_scope"), Some(&CValue::Str("default".into())));
}

#[test]
fn ac4_admin_key_is_refused_by_the_w2_commands_on_every_path() {
    let env = Env::new("ac4w2");
    for c in every_caller() {
        for proj in [None, MAIN, AGENT_BRANCH, Some(("main", false))] {
            for scope in Scope::ALL {
                let e = env.set(c, proj, ROOTS, scope, roots()).unwrap_err();
                assert_eq!(e.code(), "admin_setting", "{c:?} {proj:?} {scope}");
                let e = env.reset(c, proj, ROOTS, scope).unwrap_err();
                assert_eq!(e.code(), "admin_setting", "{c:?} {proj:?} {scope}");
            }
        }
    }
    assert_eq!(env.journaled(), 0);
    assert!(!env.machine.path().exists());
    assert!(env.project.0.borrow().is_empty());
}

#[test]
fn ac4_set_admin_value_by_a_human_cli_session_commits_one_a0_entry() {
    let env = Env::new("ac4a0");
    let r = env.set_admin(HUMAN, None, ROOTS, Scope::Machine, roots()).unwrap();
    assert_eq!(canonical(&r), canonical(&expected("expected/set_admin_result.json")));
    let entries = env.rec.entries.borrow();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, JournalTarget::UserSettings);
    assert_eq!(String::from_utf8(entries[0].1.clone()).unwrap(), canonical(&expected("expected/entry_admin.json")));
    drop(entries);
    // The machine file holds it and the read resolves it from `machine`.
    let rows = env.list(SCRIPT, None);
    let CValue::Arr(rows) = rows else { panic!() };
    let CValue::Obj(row) = &rows[0] else { panic!() };
    assert_eq!((row.get("value"), row.get("source_scope")), (Some(&roots()), Some(&CValue::Str("machine".into()))));
}

#[test]
fn ac4_set_admin_value_is_human_only_and_never_from_mcp() {
    let env = Env::new("ac4pol");
    for c in every_caller() {
        let allowed = c.kind == PrincipalKind::Human && c.surface != Surface::Mcp;
        if allowed {
            continue;
        }
        let e = env.set_admin(c, MAIN, ROOTS, Scope::Machine, roots()).unwrap_err();
        assert_eq!(e.code(), "policy", "{c:?}");
    }
    // The A0 command never changes an ordinary key.
    assert_eq!(env.set_admin(HUMAN, MAIN, LENGTH, Scope::User, num(1.0)).unwrap_err().code(), "validation");
    // Scope rules hold for admin keys too.
    assert_eq!(env.set_admin(HUMAN, MAIN, ROOTS, Scope::User, roots()).unwrap_err().code(), "scope_not_allowed");
    assert_eq!(env.journaled(), 0);
    assert!(!env.machine.path().exists());
}

#[test]
fn ac5_no_admin_tool_on_mcp() {
    let decls = declarations();
    let mut tools: Vec<String> = decls
        .iter()
        .filter(|d| d.def.mcp_projection != Projection::Never && d.def.exposure.contains(&Surface::Mcp))
        .map(|d| fid_registry::tool_name(&d.def.id, d.def.major))
        .collect();
    tools.sort();
    assert_eq!(tools, ["settings_list_values", "settings_reset_value", "settings_set_value"]);
    let admin = decls.iter().find(|d| d.def.id == "settings.set_admin_value").unwrap();
    assert_eq!((admin.def.side_effect, admin.def.mcp_projection), (SideEffect::A0, Projection::Never));
    assert!(admin.def.human_only && !admin.def.exposure.contains(&Surface::Mcp));
}

#[test]
fn ac5_agent_changes_project_values_only_on_its_branch() {
    let env = Env::new("ac5");
    // User- and machine-scope changes: policy, whatever the branch.
    for proj in [None, MAIN, AGENT_BRANCH] {
        assert_eq!(env.set(AGENT, proj, LENGTH, Scope::User, num(1.0)).unwrap_err().code(), "policy");
        assert_eq!(env.set(AGENT, proj, TIPS, Scope::User, CValue::Bool(false)).unwrap_err().code(), "policy");
        assert_eq!(env.reset(AGENT, proj, TIPS, Scope::User).unwrap_err().code(), "policy");
    }
    // A human or script arriving over MCP is held to the agent rules.
    for c in [caller(PrincipalKind::Human, Surface::Mcp), caller(PrincipalKind::Script, Surface::Mcp)] {
        assert_eq!(env.set(c, MAIN, LENGTH, Scope::User, num(1.0)).unwrap_err().code(), "policy");
    }
    // Project scope off the agent branch: policy.
    for proj in [MAIN, Some(("agent/claude", true)), Some(("agent/Claude/1", true)), Some(("agent/c/01", true))] {
        assert_eq!(env.set(AGENT, proj, LENGTH, Scope::Project, num(1.0)).unwrap_err().code(), "policy");
    }
    assert_eq!(env.journaled(), 0);
    // On the agent branch it commits there.
    let agent_cli = caller(PrincipalKind::Agent, Surface::Cli);
    env.set(agent_cli, AGENT_BRANCH, LENGTH, Scope::Project, num(1.0)).unwrap();
    let entries = env.rec.entries.borrow();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, JournalTarget::Project { branch: "agent/claude/1".into() });
    assert_eq!(entries[0].2.principal(), PrincipalKind::Agent);
    assert!(!env.user.path().exists());
}

#[test]
fn ac6_project_change_goes_to_the_project_journal_user_change_to_the_user_journal() {
    let env = Env::new("ac6");
    let r = env.set(HUMAN, MAIN, LENGTH, Scope::Project, num(0.2)).unwrap();
    assert_eq!(canonical(&r), canonical(&expected("expected/set_project_result.json")));
    let r = env.set(SCRIPT, MAIN, LENGTH, Scope::User, num(0.5)).unwrap();
    assert_eq!(canonical(&r), canonical(&expected("expected/set_user_result.json")));
    let entries = env.rec.entries.borrow();
    let project: Vec<_> = entries.iter().filter(|e| matches!(e.0, JournalTarget::Project { .. })).collect();
    let user: Vec<_> = entries.iter().filter(|e| e.0 == JournalTarget::UserSettings).collect();
    assert_eq!((project.len(), user.len()), (1, 1));
    assert_eq!(project[0].0, JournalTarget::Project { branch: "main".into() });
    assert_eq!(String::from_utf8(project[0].1.clone()).unwrap(), canonical(&expected("expected/entry_project.json")));
    assert_eq!(String::from_utf8(user[0].1.clone()).unwrap(), canonical(&expected("expected/entry_user.json")));
    drop(entries);
    let file = std::fs::read(env.user.path()).unwrap();
    assert_eq!(String::from_utf8(file).unwrap(), canonical(&expected("expected/user_after_set.json")));
    // A reset reports the previous value and removes it from the file.
    let r = env.reset(SCRIPT, MAIN, LENGTH, Scope::User).unwrap();
    assert_eq!(canonical(&r), canonical(&expected("expected/reset_user_result.json")));
    assert_eq!(std::fs::read_to_string(env.user.path()).unwrap(), r#"{"format":"fid-settings/1","values":{}}"#);
    assert_eq!(env.journaled(), 3);
}

/// The settings journal of a session: project and user changes, a reset and an admin change.
fn record_session(env: &Env) {
    env.set(HUMAN, MAIN, LENGTH, Scope::Project, num(0.2)).unwrap();
    env.set(SCRIPT, MAIN, LENGTH, Scope::User, num(0.5)).unwrap();
    env.set(SCRIPT, None, TIPS, Scope::User, CValue::Bool(false)).unwrap();
    env.reset(SCRIPT, None, LENGTH, Scope::User).unwrap();
    env.set(caller(PrincipalKind::Agent, Surface::Cli), AGENT_BRANCH, LENGTH, Scope::Project, num(3.0)).unwrap();
    env.set_admin(HUMAN, None, ROOTS, Scope::Machine, roots()).unwrap();
}

fn replay(recorded: &[Vec<u8>], tag: &str) -> (Vec<Vec<u8>>, Env) {
    let fresh = Env::new(tag);
    for bytes in recorded {
        let inv = Invocation::from_entry(&parse(bytes).unwrap()).unwrap();
        let c = caller(inv.principal, Surface::Cli);
        let proj = inv.branch.as_deref().map(|b| (b, true));
        fresh.run(c, proj, |ctx| invoke(ctx, &inv)).unwrap();
    }
    let replayed = fresh.rec.entries.borrow().iter().map(|e| e.1.clone()).collect();
    (replayed, fresh)
}

#[test]
fn ac7_exact_replay_of_the_settings_journal_is_identical() {
    let env = Env::new("ac7");
    record_session(&env);
    let recorded: Vec<Vec<u8>> = env.rec.entries.borrow().iter().map(|e| e.1.clone()).collect();
    assert_eq!(recorded.len(), 6);
    let (replayed, fresh) = replay(&recorded, "ac7r");
    assert_eq!(replay_outcome(&recorded, &replayed), ReplayOutcome::Identical);
    // Same stores after replay.
    for (a, b) in [(&env.user, &fresh.user), (&env.machine, &fresh.machine)] {
        assert_eq!(std::fs::read(a.path()).unwrap(), std::fs::read(b.path()).unwrap());
    }
    assert_eq!(*env.project.0.borrow(), *fresh.project.0.borrow());
    // A tampered entry (another value) replays to different entries.
    let mut tampered = recorded.clone();
    tampered[1] = String::from_utf8(tampered[1].clone()).unwrap().replace("0.5", "0.7").into_bytes();
    let (replayed, _) = replay(&tampered, "ac7t");
    assert_eq!(replay_outcome(&recorded, &replayed), ReplayOutcome::Divergent);
}

// ------------------------------------------------------------------ fail-closed rules

#[test]
fn req010_scope_not_allowed_and_unknown_keys_change_nothing() {
    let env = Env::new("scope");
    assert_eq!(
        env.set(HUMAN, MAIN, TIPS, Scope::Project, CValue::Bool(false)).unwrap_err().code(),
        "scope_not_allowed"
    );
    assert_eq!(
        env.set(HUMAN, MAIN, TIPS, Scope::Machine, CValue::Bool(false)).unwrap_err().code(),
        "scope_not_allowed"
    );
    assert_eq!(env.set(HUMAN, MAIN, LENGTH, Scope::Machine, num(1.0)).unwrap_err().code(), "scope_not_allowed");
    assert_eq!(env.reset(HUMAN, MAIN, LENGTH, Scope::Machine).unwrap_err().code(), "scope_not_allowed");
    assert_eq!(env.set(HUMAN, MAIN, "probe.unknown", Scope::User, num(1.0)).unwrap_err().code(), "unknown_key");
    assert_eq!(env.reset(HUMAN, MAIN, "probe.unknown", Scope::User).unwrap_err().code(), "unknown_key");
    assert_eq!(env.set_admin(HUMAN, MAIN, "net.unknown", Scope::Machine, roots()).unwrap_err().code(), "unknown_key");
    assert_eq!(env.journaled(), 0);
    assert!(!env.user.path().exists() && !env.machine.path().exists() && env.project.0.borrow().is_empty());
}

#[test]
fn invalid_values_are_refused_and_change_nothing() {
    let env = Env::new("invalid");
    for v in [
        CValue::Str("0.5 mm".into()),
        num(150.0),
        num(-1.0),
        CValue::Null,
        CValue::Bool(true),
        CValue::Arr(vec![num(1.0)]),
    ] {
        let e = env.set(HUMAN, MAIN, LENGTH, Scope::User, v).unwrap_err();
        assert_eq!(e.code(), "validation");
    }
    let dup = CValue::Arr(vec![CValue::Str("/a".into()), CValue::Str("/a".into())]);
    assert_eq!(env.set_admin(HUMAN, None, ROOTS, Scope::Machine, dup).unwrap_err().code(), "validation");
    let empty = CValue::Arr(vec![CValue::Str(String::new())]);
    assert_eq!(env.set_admin(HUMAN, None, ROOTS, Scope::Machine, empty).unwrap_err().code(), "validation");
    let big = CValue::Arr((0..65).map(|i| CValue::Str(format!("/r{i}"))).collect());
    assert_eq!(env.set_admin(HUMAN, None, ROOTS, Scope::Machine, big).unwrap_err().code(), "validation");
    assert_eq!(env.journaled(), 0);
    assert!(!env.user.path().exists() && !env.machine.path().exists());
}

#[test]
fn req010_unwritable_stores_are_refused_before_the_journal() {
    let env = Env::new("unwritable");
    assert_eq!(env.set(HUMAN, None, LENGTH, Scope::Project, num(1.0)).unwrap_err().code(), "scope_not_writable");
    let read_only = Some(("main", false));
    assert_eq!(env.set(HUMAN, read_only, LENGTH, Scope::Project, num(1.0)).unwrap_err().code(), "scope_not_writable");
    // A user file that is unreadable as a whole is never overwritten.
    std::fs::create_dir_all(env.user.path().parent().unwrap()).unwrap();
    std::fs::write(env.user.path(), b"{not json").unwrap();
    assert_eq!(env.set(HUMAN, None, TIPS, Scope::User, CValue::Bool(false)).unwrap_err().code(), "scope_not_writable");
    assert_eq!(std::fs::read(env.user.path()).unwrap(), b"{not json");
    // A machine directory that cannot be created (a file is in the way).
    let blocker = env.machine.path().parent().unwrap();
    std::fs::create_dir_all(blocker.parent().unwrap()).unwrap();
    std::fs::write(blocker, b"file").unwrap();
    assert_eq!(env.set_admin(HUMAN, None, ROOTS, Scope::Machine, roots()).unwrap_err().code(), "scope_not_writable");
    assert_eq!(env.journaled(), 0);
}

#[test]
fn a_failed_journal_commit_changes_nothing() {
    let mut env = Env::new("jfail");
    env.rec.fail = true;
    assert_eq!(env.set(HUMAN, MAIN, LENGTH, Scope::User, num(1.0)).unwrap_err().code(), "journal_unavailable");
    assert_eq!(env.set(HUMAN, MAIN, LENGTH, Scope::Project, num(1.0)).unwrap_err().code(), "journal_unavailable");
    assert!(!env.user.path().exists());
    assert!(env.project.0.borrow().is_empty());
}

#[test]
fn stored_values_are_checked_on_every_read() {
    let env = Env::new("reads");
    std::fs::create_dir_all(env.user.path().parent().unwrap()).unwrap();
    std::fs::create_dir_all(env.machine.path().parent().unwrap()).unwrap();
    // A user value out of range, a machine value in a scope the key does not allow, an
    // undeclared key and a malformed key.
    std::fs::write(
        env.user.path(),
        br#"{"format":"fid-settings/1","values":{"Bad Key":1,"probe.approach_distance":101,"probe.other":1}}"#,
    )
    .unwrap();
    std::fs::write(env.machine.path(), br#"{"format":"fid-settings/1","values":{"ui.show_tips":false}}"#).unwrap();
    // A project row that is not a record.
    env.project.0.borrow_mut().insert(LENGTH.into(), b"0.2".to_vec());
    assert_eq!(canonical(&env.list(HUMAN, MAIN)), canonical(&expected("expected/list_fresh.json")));
    let w = |key: Option<&str>, scope, reason| SettingWarning { key: key.map(str::to_string), scope, reason };
    let mut want = vec![
        w(None, Scope::User, WarningReason::UnknownKey),
        w(Some("probe.other"), Scope::User, WarningReason::UnknownKey),
        w(Some(LENGTH), Scope::Project, WarningReason::Unreadable),
        w(Some(LENGTH), Scope::User, WarningReason::InvalidValue),
        w(Some(TIPS), Scope::Machine, WarningReason::ScopeNotAllowed),
    ];
    want.sort();
    assert_eq!(*env.log.0.borrow(), want);
    // A garbled file: one warning, defaults.
    env.log.0.borrow_mut().clear();
    std::fs::write(env.user.path(), b"\xff\xfe").unwrap();
    std::fs::write(env.machine.path(), br#"{"format":"fid-settings/2","values":{}}"#).unwrap();
    assert_eq!(canonical(&env.list(HUMAN, None)), canonical(&expected("expected/list_fresh.json")));
    let mut want =
        vec![w(None, Scope::User, WarningReason::Unreadable), w(None, Scope::Machine, WarningReason::Unreadable)];
    want.sort();
    assert_eq!(*env.log.0.borrow(), want);
}

#[test]
fn a_reset_project_row_falls_through_to_user() {
    let env = Env::new("resetrow");
    env.set(HUMAN, MAIN, LENGTH, Scope::Project, num(0.2)).unwrap();
    env.set(HUMAN, MAIN, LENGTH, Scope::User, num(0.5)).unwrap();
    let r = env.reset(HUMAN, MAIN, LENGTH, Scope::Project).unwrap();
    assert_eq!(r, parse(br#"{"key":"probe.approach_distance","previous":0.2,"scope":"project"}"#).unwrap());
    assert_eq!(env.project.0.borrow().get(LENGTH).unwrap(), b"{}");
    assert_eq!(canonical(&env.list(HUMAN, MAIN)), canonical(&expected("expected/list_ac2_outside.json")));
}

#[test]
fn list_filters_by_prefix_and_scope() {
    let env = Env::new("filter");
    let run = |p: ListValuesParams| -> Vec<String> {
        env.run(HUMAN, None, |ctx| list_values(ctx, p).unwrap()).into_iter().map(|r| r.key).collect()
    };
    assert_eq!(run(ListValuesParams { prefix: Some("probe.".into()), scope: None }), [LENGTH]);
    assert_eq!(run(ListValuesParams { prefix: None, scope: Some(Scope::User) }), [LENGTH, TIPS]);
    assert_eq!(run(ListValuesParams { prefix: None, scope: Some(Scope::Machine) }), [ROOTS]);
    let bad = ListValuesParams { prefix: Some("Probe".into()), scope: None };
    assert_eq!(env.run(HUMAN, None, |ctx| list_values(ctx, bad)).unwrap_err().code(), "validation");
}

#[test]
fn parameters_are_decoded_strictly() {
    let ok = SetValueParams::from_json(br#"{"key":"ui.show_tips","scope":"user","value":false}"#).unwrap();
    assert_eq!((ok.key.as_str(), ok.scope, ok.value.0), (TIPS, Scope::User, CValue::Bool(false)));
    for bad in [
        &br#"{"key":"ui.show_tips","scope":"user"}"#[..],
        br#"{"key":"ui.show_tips","scope":"site","value":1}"#,
        br#"{"key":"UI.tips","scope":"user","value":1}"#,
        br#"{"key":"ui.show_tips","scope":"user","value":1,"principal":"human"}"#,
        br#"{"key":"ui.show_tips","key":"ui.show_tips","scope":"user","value":1}"#,
        br#"[]"#,
    ] {
        assert_eq!(SetValueParams::from_json(bad).unwrap_err().code(), "validation");
    }
    assert!(ResetValueParams::from_json(br#"{"key":"ui.show_tips","scope":"user","value":1}"#).is_err());
    assert_eq!(ListValuesParams::from_json(b"{}").unwrap(), ListValuesParams::default());
    assert!(ListValuesParams::from_json(br#"{"scope":"site"}"#).is_err());
}

#[test]
fn agent_branch_names() {
    for ok in ["agent/claude/1", "agent/a-b/42", "agent/x/10"] {
        assert!(is_agent_branch(ok), "{ok}");
    }
    for bad in
        ["main", "agent/claude", "agent//1", "agent/claude/0", "agent/claude/01", "agent/Claude/1", "agent/c/1/x"]
    {
        assert!(!is_agent_branch(bad), "{bad}");
    }
}

// ------------------------------------------------------------------ schemas and declarations

#[test]
fn command_schemas_equal_the_golden_files_and_pass_the_registry_checks() {
    let decls = declarations();
    let ids: Vec<String> = decls.iter().map(|d| format!("{}@{}", d.def.id, d.def.major)).collect();
    assert_eq!(
        ids,
        ["settings.list_values@1", "settings.set_value@1", "settings.reset_value@1", "settings.set_admin_value@1"]
    );
    let catalogue = Catalogue::geo_frame().unwrap();
    for d in &decls {
        let id = format!("{}@{}", d.def.id, d.def.major);
        for (kind, schema) in [("params", &d.def.params_schema), ("result", &d.def.result_schema)] {
            assert_eq!(canonical(schema), canonical(&expected(&format!("golden/{id}.{kind}.json"))), "{id} {kind}");
            assert_eq!(check_schema(schema, &catalogue), Vec::new(), "{id} {kind}");
        }
    }
    let raw: Vec<_> = decls.iter().map(|d| d.to_validation()).collect();
    assert_eq!(validate(&raw), Vec::new());
    let classes: Vec<SideEffect> = decls.iter().map(|d| d.def.side_effect).collect();
    assert_eq!(classes, [SideEffect::R0, SideEffect::W2, SideEffect::W2, SideEffect::A0]);
    // The handler pointers are the typed handlers.
    type Set = fn(&SettingsCtx<'_>, SetValueParams) -> Result<fid_store::settings::SetValueResult, SettingsError>;
    assert!(decls[1].handler.get::<Set>().is_some() && decls[3].handler.get::<Set>().is_some());
    assert!(decls[2].handler.get::<Set>().is_none());
}

#[test]
fn manifest_equals_the_reference() {
    let reg = registry();
    let want = fixture("expected/manifest.json");
    assert_eq!(reg.manifest().unwrap(), want.strip_suffix(b"\n").unwrap());
    assert_eq!(reg.len(), 3);
}

fid_settings! {
    fn fixture_settings;
    {
        key: "probe.approach_distance",
        owner: "probe",
        schema: r#"{"type": "number", "x-quantity": "length", "x-unit": "mm", "minimum": 0, "maximum": 100}"#,
        default: "2",
        scopes: [Project, User],
        admin: false,
    }
    {
        key: "ui.show_tips",
        owner: "ui",
        schema: r#"{"type": "boolean"}"#,
        default: "true",
        scopes: [User],
        admin: false,
    }
    {
        key: "net.allowed_roots",
        owner: "net",
        schema: r#"{"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 1024}, "maxItems": 64, "uniqueItems": true}"#,
        default: "[]",
        scopes: [Machine],
        admin: true,
    }
}

#[test]
fn fid_settings_macro_declares_the_same_settings() {
    let sources = fixture_settings();
    assert_eq!(sources.iter().map(|s| s.key).collect::<Vec<_>>(), [LENGTH, TIPS, ROOTS]);
    assert_eq!(SettingsRegistry::build(&sources).unwrap(), registry());
}

fn decl_error(schema: &str, default: &str, key: &str, owner: &str, scopes: &'static [Scope]) -> &'static str {
    let src = fid_store::settings::SettingSource {
        key: Box::leak(key.to_string().into_boxed_str()),
        owner: Box::leak(owner.to_string().into_boxed_str()),
        schema: Box::leak(schema.to_string().into_boxed_str()),
        default: Box::leak(default.to_string().into_boxed_str()),
        scopes,
        admin: false,
    };
    let errs = SettingsRegistry::build(&[src]).unwrap_err();
    assert_eq!(errs.len(), 1);
    errs[0].code
}

#[test]
fn req001_declarations_are_checked() {
    let b = r#"{"type": "boolean"}"#;
    let user: &'static [Scope] = &[Scope::User];
    assert_eq!(decl_error(b, "true", "Ui.tips", "ui", user), "bad_key");
    assert_eq!(decl_error(b, "true", "tips", "tips", user), "bad_key");
    assert_eq!(decl_error(b, "true", "ui.tips.", "ui", user), "bad_key");
    assert_eq!(decl_error(b, "true", "ui.tips", "probe", user), "owner_mismatch");
    assert_eq!(decl_error(b, "true", "ui.tips", "Ui", user), "bad_owner");
    assert_eq!(decl_error(b, "1", "ui.tips", "ui", user), "bad_default");
    assert_eq!(decl_error(b, "tru", "ui.tips", "ui", user), "bad_default");
    assert_eq!(decl_error(b, "true", "ui.tips", "ui", &[]), "no_scopes");
    assert_eq!(decl_error(b, "true", "ui.tips", "ui", &[Scope::User, Scope::User]), "duplicate_scope");
    assert_eq!(decl_error(r#"{"type": "number"}"#, "1", "ui.size", "ui", user), "unsupported_schema");
    assert_eq!(decl_error(r#"{"type": "number", "x-quantity": "lenght"}"#, "1", "ui.size", "ui", user), "bad_schema");
    assert_eq!(
        decl_error(r#"{"type": "string", "pattern": "^a"}"#, r#""a""#, "ui.name", "ui", user),
        "unsupported_schema"
    );
    assert_eq!(decl_error(r#"{"anyOf": [{"type": "string"}]}"#, r#""a""#, "ui.name", "ui", user), "unsupported_schema");
    assert_eq!(decl_error(r#"{"type": "string", "x-foo": 1}"#, r#""a""#, "ui.name", "ui", user), "bad_schema");
    assert_eq!(decl_error(r#"{"type": "string"#, r#""a""#, "ui.name", "ui", user), "bad_schema");
    // Duplicates across declarations.
    let s = fixture_settings();
    let errs = SettingsRegistry::build(&[s[1], s[1]]).unwrap_err();
    assert_eq!((errs.len(), errs[0].code), (1, "duplicate_key"));
}

/// Hand-labelled cases of the value validator (JSON Schema 2020-12 semantics).
#[test]
fn value_validator_cases() {
    let cases: &[(&str, &str, bool)] = &[
        (r#"{"type": "integer"}"#, "3", true),
        (r#"{"type": "integer"}"#, "3.5", false),
        (r#"{"type": ["string", "null"]}"#, "null", true),
        (r#"{"type": ["string", "null"]}"#, "1", false),
        (r#"{"enum": ["a", "b"]}"#, r#""b""#, true),
        (r#"{"enum": ["a", "b"]}"#, r#""c""#, false),
        (r#"{"const": 1}"#, "1", true),
        (r#"{"exclusiveMinimum": 0, "x-quantity": "length"}"#, "0", false),
        (r#"{"exclusiveMaximum": 1}"#, "0.5", true),
        (r#"{"minimum": 0}"#, r#""text""#, true),
        (r#"{"maxLength": 2}"#, r#""\u00e9\u00e9""#, true),
        (r#"{"maxLength": 2}"#, r#""abc""#, false),
        (r#"{"minItems": 1}"#, "[]", false),
        (r#"{"type": "object", "properties": {"a": {"type": "boolean"}}, "required": ["a"]}"#, r#"{"a": true}"#, true),
        (r#"{"type": "object", "required": ["a"]}"#, "{}", false),
        (r#"{"type": "object", "properties": {"a": {"type": "boolean"}}}"#, r#"{"a": 1}"#, false),
        (r#"{"type": "object", "additionalProperties": false}"#, r#"{"a": 1}"#, false),
        (r#"{"type": "object", "additionalProperties": {"type": "integer"}}"#, r#"{"a": 1, "b": 2}"#, true),
        (r#"{"type": "object", "additionalProperties": {"type": "integer"}}"#, r#"{"a": "x"}"#, false),
        (r#"{"maxProperties": 1}"#, r#"{"a": 1, "b": 2}"#, false),
        (r#"{"items": {"type": "string"}, "uniqueItems": true}"#, r#"["a", "b"]"#, true),
        (r#"{"uniqueItems": true}"#, r#"[{"a": 1}, {"a": 1}]"#, false),
    ];
    for (schema, value, ok) in cases {
        let schema = parse(schema.as_bytes()).unwrap();
        fid_store::settings::check_setting_schema(&schema).unwrap();
        let value = parse(value.as_bytes()).unwrap();
        assert_eq!(validate_value(&schema, &value).is_ok(), *ok, "{schema:?} {value:?}");
    }
    // Deep values cannot deepen the walk: only the schema's depth counts.
    let mut deep = CValue::Null;
    for _ in 0..100 {
        deep = CValue::Arr(vec![deep]);
    }
    assert!(validate_value(&parse(br#"{"type": "array"}"#).unwrap(), &deep).is_ok());
}

#[test]
fn config_locations() {
    use std::ffi::OsString;
    let env = |k: &str| -> Option<OsString> {
        match k {
            "APPDATA" => Some(r"C:\Users\u\AppData\Roaming".into()),
            "ProgramData" => Some(r"C:\ProgramData".into()),
            "HOME" => Some("/home/u".into()),
            _ => None,
        }
    };
    let s = |p: Option<PathBuf>| p.map(|p| p.to_string_lossy().replace('\\', "/"));
    assert_eq!(s(user_config_dir("linux", env)).as_deref(), Some("/home/u/.config/fiducial/settings.json"));
    assert_eq!(
        s(user_config_dir("macos", env)).as_deref(),
        Some("/home/u/Library/Application Support/fiducial/settings.json")
    );
    assert_eq!(
        s(user_config_dir("windows", env)).as_deref(),
        Some("C:/Users/u/AppData/Roaming/fiducial/settings.json")
    );
    assert_eq!(s(machine_config_dir("linux", env)).as_deref(), Some("/etc/fiducial/settings.json"));
    assert_eq!(s(machine_config_dir("windows", env)).as_deref(), Some("C:/ProgramData/fiducial/settings.json"));
    // A relative XDG_CONFIG_HOME is ignored.
    let rel = |k: &str| -> Option<OsString> { (k == "XDG_CONFIG_HOME").then(|| "rel".into()) };
    assert_eq!(user_config_dir("linux", rel), None);
    assert!(FileStore::new(Scope::Project, PathBuf::from("/x")).is_err());
}

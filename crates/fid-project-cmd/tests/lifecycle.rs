//! Integration tests for `CORE.PRJ.CONT.LIFE` Part 2 (`AC-1`..`AC-8`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, ConflictReason, PolicyReason, PreconditionReason};
use fid_bus::handler::{CancelToken, HandlerCtx};
use fid_bus::policy::fs::{Roots, ROOTS_KEY};
use fid_bus::principal::{
    Channel, ChannelKind, Identity as BusIdentity, Principal, PrincipalId, SessionId, SessionKey, SessionTable,
};
use fid_canon::{digest_bytes, parse, to_canonical_bytes, CValue, Digest};
use fid_journal::append::Clock;
use fid_journal::checkpoint::Reason as CheckpointReason;
use fid_journal::entry::Manifests;
use fid_project::id::{Bits, IdError, ObservationSource};
use fid_project::lifecycle::RECORD_STREAM;
use fid_project::{ContainerKind, Uuid};
use fid_project_cmd::lifecycle::{
    close_project, command_defs, create_project, list_projects, open_project, read_store_identity, save_project_as,
    save_project_as_with_hook, ProjectSession, RecentProjects,
};
use fid_registry::schema::check_schema;
use fid_registry::validate::{validate, CommandDecl};
use fid_registry::Catalogue;
use fid_store::settings::{FileStore, Scope, Sources};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_dir(tag: &str) -> PathBuf {
    let n = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("fid-life-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

struct StepClock {
    secs: AtomicU64,
}

impl StepClock {
    fn shared(start_secs: u64) -> Arc<dyn Clock> {
        Arc::new(Self { secs: AtomicU64::new(start_secs) })
    }
}

impl Clock for StepClock {
    fn now(&self) -> Duration {
        let s = self.secs.fetch_add(1, Ordering::Relaxed);
        Duration::from_secs(s)
    }
}

struct SeededSource {
    ms: u64,
    state: u64,
}

impl SeededSource {
    fn new(seed: u64) -> Self {
        Self { ms: 1_760_000_000_000, state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

impl ObservationSource for SeededSource {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        let out = self.ms;
        self.ms = self.ms.saturating_add(1000);
        Ok(out)
    }

    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        let hi = u128::from(self.next_u64());
        let lo = u128::from(self.next_u64());
        let full = (hi << 64) | lo;
        let v = if n == 128 { full } else { full & ((1u128 << n) - 1) };
        Bits::new(n, v)
    }
}

struct TestChan {
    kind: ChannelKind,
    who: String,
    session: SessionKey,
}

impl Channel for TestChan {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    fn authenticated_identity(&self) -> Option<BusIdentity> {
        Some(BusIdentity::new(PrincipalId::new(&self.who).unwrap()))
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
}

fn human_principal(name: &str) -> Principal {
    let mut table = SessionTable::new();
    let session = table.open_session(SessionId::new("desk-1").unwrap()).unwrap();
    let ch = TestChan { kind: ChannelKind::Gui, who: name.to_owned(), session };
    HandlerCtx::derive(&table, &ch, 0, CancelToken::never()).unwrap().principal().clone()
}

fn agent_principal(name: &str) -> Principal {
    let mut table = SessionTable::new();
    let session = table.open_session(SessionId::new("mcp-1").unwrap()).unwrap();
    let ch = TestChan { kind: ChannelKind::Mcp, who: name.to_owned(), session };
    HandlerCtx::derive(&table, &ch, 0, CancelToken::never()).unwrap().principal().clone()
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert(k.to_owned(), v);
    }
    CValue::Obj(m)
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

fn test_manifests() -> Manifests {
    let d = digest_bytes(b"fid-project-cmd-lifecycle-test");
    Manifests { kernel: d, registry: d }
}

struct TestEnv {
    base: PathBuf,
    work_dir: PathBuf,
    exports_dir: PathBuf,
    imports_dir: PathBuf,
    roots: Roots,
}

impl TestEnv {
    fn new(tag: &str) -> Self {
        let base = temp_dir(tag);
        let work_dir = base.join("work");
        let exports_dir = base.join("exports");
        let imports_dir = base.join("imports");
        fs::create_dir_all(&work_dir).unwrap();
        fs::create_dir_all(&exports_dir).unwrap();
        fs::create_dir_all(&imports_dir).unwrap();

        let roots_val = CValue::Arr(vec![
            obj([("id", text("work")), ("kind", text("export")), ("path", text(work_dir.to_str().unwrap()))]),
            obj([("id", text("exports")), ("kind", text("export")), ("path", text(exports_dir.to_str().unwrap()))]),
            obj([("id", text("imports")), ("kind", text("import")), ("path", text(imports_dir.to_str().unwrap()))]),
        ]);
        let mut values = BTreeMap::new();
        values.insert(ROOTS_KEY.to_owned(), roots_val);
        let doc = obj([("format", text("fid-settings/1")), ("values", CValue::Obj(values))]);
        let machine_path = base.join("cfg/machine/settings.json");
        let user_path = base.join("cfg/user/settings.json");
        fs::create_dir_all(machine_path.parent().unwrap()).unwrap();
        fs::write(&machine_path, to_canonical_bytes(&doc).unwrap()).unwrap();
        let machine = FileStore::new(Scope::Machine, machine_path).unwrap();
        let user = FileStore::new(Scope::User, user_path).unwrap();
        let sources = Sources { project: None, user: &user, machine: &machine };
        let (roots, warnings) = Roots::from_settings(&sources).unwrap();
        assert!(warnings.is_empty(), "unexpected root warnings: {warnings:?}");

        Self { base, work_dir, exports_dir, imports_dir, roots }
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

/// Loads `fixtures/core/prj/cont/life/p_small/manifest.json` and materializes all 20
/// containers (1 Part, 2 Revisions, 4 Plans, 13 Runs) inside `session`.
fn populate_p_small(
    session: &ProjectSession,
    principal: &Principal,
    source: &mut dyn ObservationSource,
) -> BTreeSet<Uuid> {
    let raw = include_str!("../../../fixtures/core/prj/cont/life/p_small/manifest.json");
    let doc = parse(raw.as_bytes()).unwrap();
    let CValue::Obj(root) = doc else { panic!("manifest root must be object") };
    let CValue::Arr(entities) = root.get("entities").unwrap() else { panic!("entities must be array") };
    assert_eq!(entities.len(), 20, "p_small fixture must define 20 entities");

    let mut id_map: BTreeMap<String, Uuid> = BTreeMap::new();
    let mut created_ids: BTreeSet<Uuid> = BTreeSet::new();

    for item in entities {
        let CValue::Obj(e) = item else { panic!("entity must be object") };
        let CValue::Str(fix_id) = e.get("id").unwrap() else { panic!("missing id") };
        let CValue::Str(kind_s) = e.get("kind").unwrap() else { panic!("missing kind") };
        let CValue::Str(label) = e.get("label").unwrap() else { panic!("missing label") };

        let (kind, params) = match kind_s.as_str() {
            "Part" => (ContainerKind::Part, obj([("label", text(label))])),
            "Revision" => {
                let CValue::Str(part_fix) = e.get("part").unwrap() else { panic!("missing part") };
                let part_id = id_map.get(part_fix).unwrap();
                (ContainerKind::Revision, obj([("label", text(label)), ("part", text(&part_id.to_string()))]))
            }
            "Plan" => {
                let CValue::Str(rev_fix) = e.get("revision").unwrap() else { panic!("missing revision") };
                let rev_id = id_map.get(rev_fix).unwrap();
                (ContainerKind::Plan, obj([("label", text(label)), ("revision", text(&rev_id.to_string()))]))
            }
            "Run" => {
                let CValue::Str(rev_fix) = e.get("revision").unwrap() else { panic!("missing revision") };
                let CValue::Str(plan_fix) = e.get("plan").unwrap() else { panic!("missing plan") };
                let rev_id = id_map.get(rev_fix).unwrap();
                let plan_id = id_map.get(plan_fix).unwrap();
                (
                    ContainerKind::Run,
                    obj([
                        ("label", text(label)),
                        ("plan", text(&plan_id.to_string())),
                        ("revision", text(&rev_id.to_string())),
                    ]),
                )
            }
            other => panic!("unexpected kind {other}"),
        };

        let (uid, _head) = session.commit_container(kind, principal, &params, source).unwrap();
        id_map.insert(fix_id.clone(), uid);
        assert!(created_ids.insert(uid), "duplicate entity UUID");
    }
    assert_eq!(created_ids.len(), 20);
    created_ids
}

fn dir_snapshot(root: &Path) -> BTreeMap<PathBuf, Digest> {
    fn walk(base: &Path, cur: &Path, out: &mut BTreeMap<PathBuf, Digest>) {
        for entry in fs::read_dir(cur).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap().to_path_buf();
            let meta = fs::symlink_metadata(&path).unwrap();
            if meta.is_dir() {
                walk(base, &path, out);
            } else if meta.is_file() {
                let bytes = fs::read(&path).unwrap();
                out.insert(rel, digest_bytes(&bytes));
            }
        }
    }
    let mut map = BTreeMap::new();
    walk(root, root, &mut map);
    map
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let sp = entry.path();
        let dp = dst.join(entry.file_name());
        if sp.is_dir() {
            copy_dir_recursive(&sp, &dp);
        } else {
            fs::copy(&sp, &dp).unwrap();
        }
    }
}

/// Validates all 5 lifecycle command definitions against `fid-registry` (including reserved effects).
#[test]
fn lifecycle_command_defs_register_cleanly() {
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 5);
    let catalogue = Catalogue::geo_frame().unwrap();
    for d in &defs {
        assert_eq!(check_schema(&d.params_schema, &catalogue), vec![], "params schema failed for {}", d.id);
        assert_eq!(check_schema(&d.result_schema, &catalogue), vec![], "result schema failed for {}", d.id);
    }
    let decls: Vec<CommandDecl> = defs.iter().map(CommandDecl::from_def).collect();
    assert_eq!(validate(&decls), vec![]);
    let ids: Vec<&str> = defs.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "project.create_project",
            "project.open_project",
            "project.close_project",
            "project.save_project_as",
            "project.list_projects",
        ]
    );
}

/// AC-1: `project.create_project@1` creates the folder, writes seq 1 `session_header` and
/// seq 2 `project.create_project@1`, and allocates a UUIDv7 project ID.
#[test]
fn ac1_create_project_journal_and_uuidv7() {
    let env = TestEnv::new("ac1");
    let alice = human_principal("alice");
    let mut recent = RecentProjects::new();
    let mut src = SeededSource::new(101);
    let clock = StepClock::shared(1_760_000_100);

    let (session, res) = create_project(
        &alice,
        &env.roots,
        &mut recent,
        &obj([("label", text("P1")), ("target", text("root:work/p1"))]),
        &mut src,
        clock,
        test_manifests(),
    )
    .unwrap();

    assert!(env.work_dir.join("p1").is_dir(), "project folder must exist");
    assert_eq!(res.project.version(), 7, "project ID must be UUIDv7");
    assert_eq!(res.namespace.version(), 4, "namespace must be random UUIDv4");

    // Verify seq 1 is session_header and seq 2 is command `project.create_project@1`.
    let seqs: Vec<(i64, String, Vec<u8>)> = session
        .store()
        .read(|conn| {
            let mut st = conn.prepare("SELECT seq, kind, header FROM entries WHERE stream = ?1 ORDER BY seq ASC")?;
            let iter = st.query_map([RECORD_STREAM], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            let mut out = Vec::new();
            for row in iter {
                out.push(row?);
            }
            Ok(out)
        })
        .unwrap();
    assert_eq!(seqs.len(), 2);
    assert_eq!(seqs[0].0, 1);
    assert_eq!(seqs[0].1, "session_header");
    assert_eq!(seqs[1].0, 2);
    assert_eq!(seqs[1].1, "command");
    let CValue::Obj(hdr2) = parse(&seqs[1].2).unwrap() else { panic!("header 2 object") };
    let CValue::Obj(cmd2) = hdr2.get("command").unwrap() else { panic!("command object") };
    assert_eq!(cmd2.get("id"), Some(&text("project.create_project")));

    let vr = session.verify_chain().unwrap();
    assert_eq!(vr.checked.entries, 2);
    close_project(session).unwrap();
}

/// AC-2: Given `p_small` (20 entities), open, close (`close` checkpoint exists), reopen
/// preserves every entity ID identically, and a concurrent second open returns `project_locked`.
#[test]
fn ac2_p_small_close_reopen_and_lock_exclusion() {
    let env = TestEnv::new("ac2");
    let alice = human_principal("alice");
    let mut recent = RecentProjects::new();
    let mut src = SeededSource::new(202);
    let clock = StepClock::shared(1_760_000_200);

    let (session, created) = create_project(
        &alice,
        &env.roots,
        &mut recent,
        &obj([("label", text("Small Benchmark Project")), ("target", text("root:work/p_small"))]),
        &mut src,
        Arc::clone(&clock),
        test_manifests(),
    )
    .unwrap();
    let expected_ids = populate_p_small(&session, &alice, &mut src);

    // Concurrent second open while lock is held must fail with `precondition/project_locked`.
    let lock_err = open_project(
        &alice,
        &env.roots,
        &mut recent,
        &obj([("source", text("root:work/p_small"))]),
        &mut src,
        Arc::clone(&clock),
        test_manifests(),
    )
    .unwrap_err();
    assert!(
        matches!(lock_err, BusError::Precondition(PreconditionReason::ProjectLocked, _)),
        "expected precondition/project_locked, got {lock_err:?}"
    );

    // Close writes a `close` checkpoint and releases the lock.
    let closed = close_project(session).unwrap();
    let cp = closed.checkpoint.expect("close checkpoint must exist");
    assert_eq!(cp.reason, CheckpointReason::Close);

    // Reopen succeeds, sees the stored `close` checkpoint, and preserves all 20 entity IDs.
    let (reopened, open_res) = open_project(
        &alice,
        &env.roots,
        &mut recent,
        &obj([("source", text("root:work/p_small"))]),
        &mut src,
        Arc::clone(&clock),
        test_manifests(),
    )
    .unwrap();
    assert_eq!(open_res.project, created.project);
    assert!(!open_res.read_only);
    assert!(open_res.branches.contains(&"main".to_owned()));
    let stored_cp = reopened.latest_close_checkpoint().unwrap().expect("close checkpoint stored");
    assert_eq!(stored_cp.reason, CheckpointReason::Close);

    let loaded_ids: BTreeSet<Uuid> = reopened.load_containers().unwrap().keys().copied().collect();
    assert_eq!(loaded_ids, expected_ids);
    reopened.verify_chain().unwrap();
    close_project(reopened).unwrap();
}

/// AC-3 & AC-6: `save_project_as` writes a complete copy with a new UUIDv7 project ID,
/// identical namespace and all 20 entity IDs, `verify_chain` passes in both, and identity reads
/// satisfy AC-6 (source genesis + `ProjectSavedAs` naming copy; copy genesis naming source ID & head).
#[test]
fn ac3_and_ac6_save_as_identity_and_verify_chain() {
    let env = TestEnv::new("ac3_ac6");
    let alice = human_principal("alice");
    let mut recent = RecentProjects::new();
    let mut src = SeededSource::new(303);
    let clock = StepClock::shared(1_760_000_300);

    let (source_sess, created) = create_project(
        &alice,
        &env.roots,
        &mut recent,
        &obj([("label", text("P Small")), ("target", text("root:work/p_small"))]),
        &mut src,
        Arc::clone(&clock),
        test_manifests(),
    )
    .unwrap();
    let expected_ids = populate_p_small(&source_sess, &alice, &mut src);
    let head_before_save = source_sess.head().unwrap();

    let saved = save_project_as(
        &source_sess,
        &alice,
        &env.roots,
        &mut recent,
        &obj([("target", text("root:exports/p_copy"))]),
        &mut src,
    )
    .unwrap();

    assert_ne!(saved.project, created.project, "copy must have a new project ID");
    assert_eq!(saved.project.version(), 7, "copy project ID must be UUIDv7");
    assert_eq!(saved.namespace, created.namespace, "namespace must be preserved");
    assert_eq!(saved.source_project, created.project);
    assert_eq!(saved.source_head, head_before_save);

    // AC-3: verify_chain passes in source.
    source_sess.verify_chain().unwrap();
    // AC-6: source identity still reads its create genesis and records ProjectSavedAs naming copy.
    let src_id = read_store_identity(source_sess.store()).unwrap();
    assert_eq!(src_id.project(), created.project);
    assert_eq!(src_id.namespace(), created.namespace);
    assert!(src_id.genesis.record.source.is_none(), "source genesis must be a create genesis");
    assert_eq!(src_id.saved_as.len(), 1);
    assert_eq!(src_id.saved_as[0].record.copy, saved.project);
    assert_eq!(src_id.saved_as[0].record.head, head_before_save);
    assert_eq!(src_id.saved_as[0].record.project, created.project);

    // Open copy, verify chain, verify all 20 entity IDs, verify AC-6 copy genesis.
    let (copy_sess, copy_open) = open_project(
        &alice,
        &env.roots,
        &mut recent,
        &obj([("source", text("root:exports/p_copy"))]),
        &mut src,
        Arc::clone(&clock),
        test_manifests(),
    )
    .unwrap();
    assert_eq!(copy_open.project, saved.project);
    copy_sess.verify_chain().unwrap();
    let copy_ids: BTreeSet<Uuid> = copy_sess.load_containers().unwrap().keys().copied().collect();
    assert_eq!(copy_ids, expected_ids, "all 20 entity IDs must be preserved across save-as");

    let copy_id = read_store_identity(copy_sess.store()).unwrap();
    assert_eq!(copy_id.project(), saved.project);
    assert_eq!(copy_id.namespace(), created.namespace);
    let src_ref = copy_id.genesis.record.source.expect("copy genesis must carry source ref");
    assert_eq!(src_ref.project, created.project);
    assert_eq!(src_ref.head, head_before_save);
    assert_eq!(copy_id.lineage.len(), 1, "copy lineage holds the earlier source create genesis");
    assert_eq!(copy_id.lineage[0].record.project, created.project);

    close_project(copy_sess).unwrap();
    close_project(source_sess).unwrap();
}

/// AC-4: existing target folder returns `policy/would_overwrite` (writing nothing in source);
/// agent passing a raw path returns `policy/raw_path_refused`; save-as off branch `main` is refused.
#[test]
fn ac4_overwrite_raw_path_and_non_main_branch_refusals() {
    let env = TestEnv::new("ac4");
    let alice = human_principal("alice");
    let agent = agent_principal("bot-1");
    let mut recent = RecentProjects::new();
    let mut src = SeededSource::new(404);
    let clock = StepClock::shared(1_760_000_400);

    let (mut sess, _) = create_project(
        &alice,
        &env.roots,
        &mut recent,
        &obj([("label", text("Src")), ("target", text("root:work/src_proj"))]),
        &mut src,
        Arc::clone(&clock),
        test_manifests(),
    )
    .unwrap();
    let head_before = sess.head().unwrap();

    // Existing folder -> policy/would_overwrite and source unchanged.
    fs::create_dir_all(env.exports_dir.join("taken")).unwrap();
    let ow_err = save_project_as(
        &sess,
        &alice,
        &env.roots,
        &mut recent,
        &obj([("target", text("root:exports/taken"))]),
        &mut src,
    )
    .unwrap_err();
    assert!(
        matches!(ow_err, BusError::Policy(PolicyReason::WouldOverwrite, _)),
        "expected policy/would_overwrite, got {ow_err:?}"
    );
    assert_eq!(sess.head().unwrap(), head_before, "refused save-as must not touch source journal");

    // Agent passing raw path -> policy/raw_path_refused.
    let raw_target = env.exports_dir.join("raw_copy").to_string_lossy().into_owned();
    let raw_err =
        save_project_as(&sess, &agent, &env.roots, &mut recent, &obj([("target", text(&raw_target))]), &mut src)
            .unwrap_err();
    assert!(
        matches!(raw_err, BusError::Policy(PolicyReason::RawPathRefused, _)),
        "expected policy/raw_path_refused, got {raw_err:?}"
    );
    assert!(!env.exports_dir.join("raw_copy").exists());

    // Agent passing raw path to create_project -> policy/raw_path_refused.
    let raw_create = env.work_dir.join("raw_new").to_string_lossy().into_owned();
    let raw_create_err = create_project(
        &agent,
        &env.roots,
        &mut recent,
        &obj([("label", text("Raw")), ("target", text(&raw_create))]),
        &mut src,
        Arc::clone(&clock),
        test_manifests(),
    )
    .unwrap_err();
    assert!(matches!(raw_create_err, BusError::Policy(PolicyReason::RawPathRefused, _)));

    // Save-as off branch `main` is refused before copying (review of #414).
    sess.set_branch("experiment");
    let br_err = save_project_as(
        &sess,
        &alice,
        &env.roots,
        &mut recent,
        &obj([("target", text("root:exports/off_main"))]),
        &mut src,
    )
    .unwrap_err();
    assert!(matches!(br_err, BusError::Policy(PolicyReason::BadRoot, _)));
    assert!(!env.exports_dir.join("off_main").exists());
    assert_eq!(sess.head().unwrap(), head_before);

    close_project(sess).unwrap();
}

/// AC-5: Three projects opened in order are returned newest first, and list caps at 100.
#[test]
fn ac5_list_projects_newest_first_and_cap_100() {
    let env = TestEnv::new("ac5");
    let alice = human_principal("alice");
    let mut recent = RecentProjects::new();
    let mut src = SeededSource::new(505);
    let clock = StepClock::shared(1_760_000_500);

    let mut ids = Vec::new();
    for (slug, lbl) in [("p1", "First"), ("p2", "Second"), ("p3", "Third")] {
        let (s, r) = create_project(
            &alice,
            &env.roots,
            &mut recent,
            &obj([("label", text(lbl)), ("target", text(&format!("root:work/{slug}")))]),
            &mut src,
            Arc::clone(&clock),
            test_manifests(),
        )
        .unwrap();
        ids.push(r.project);
        close_project(s).unwrap();
    }

    // Reopen p1 -> now p1 is newest, followed by p3, then p2.
    let (s1, _) = open_project(
        &alice,
        &env.roots,
        &mut recent,
        &obj([("source", text("root:work/p1"))]),
        &mut src,
        Arc::clone(&clock),
        test_manifests(),
    )
    .unwrap();
    close_project(s1).unwrap();

    let listed = list_projects(&recent);
    let CValue::Obj(o) = listed else { panic!("list_projects result object") };
    let CValue::Arr(arr) = o.get("projects").unwrap() else { panic!("projects array") };
    let returned: Vec<String> = arr
        .iter()
        .map(|item| {
            let CValue::Obj(m) = item else { panic!("item object") };
            let CValue::Str(s) = m.get("project").unwrap() else { panic!("project id") };
            s.clone()
        })
        .collect();
    assert_eq!(returned, vec![ids[0].to_string(), ids[2].to_string(), ids[1].to_string()]);

    // Cap at 100 entries.
    let mut capped = RecentProjects::new();
    for i in 0u64..120 {
        let u = Uuid::from_bytes({
            let mut b = [0u8; 16];
            b[8..16].copy_from_slice(&i.to_be_bytes());
            b[6] = 0x70;
            b[8] = (b[8] & 0x3f) | 0x80;
            b
        });
        capped.record(
            &format!("root:work/proj_{i}"),
            u,
            &format!("Proj {i}"),
            format!("2026-11-23T00:00:{:02}Z", i % 60),
        );
    }
    assert_eq!(capped.list().len(), 100);
}

/// AC-7 (CAS head_moved): When the source head moves before save-as publishes, save-as fails
/// with `conflict/head_moved`, writes no save-as entry in the source, and leaves nothing at target.
#[test]
fn ac7_save_as_head_moved_writes_nothing() {
    let env = TestEnv::new("ac7");
    let alice = human_principal("alice");
    let mut recent = RecentProjects::new();
    let mut src = SeededSource::new(707);
    let clock = StepClock::shared(1_760_000_700);

    let (sess, _) = create_project(
        &alice,
        &env.roots,
        &mut recent,
        &obj([("label", text("CAS Source")), ("target", text("root:work/cas_src"))]),
        &mut src,
        Arc::clone(&clock),
        test_manifests(),
    )
    .unwrap();

    // Hook commits a new Part on the source between snapshot finish and publish CAS.
    let hook_principal = alice.clone();
    let mut hook_src = SeededSource::new(7070);
    let hook = |s: &ProjectSession| {
        let mut local_src = SeededSource::new(7071);
        s.commit_container(ContainerKind::Part, &hook_principal, &obj([("label", text("Concurrent"))]), &mut local_src)
            .unwrap();
    };
    let _ = &mut hook_src;

    let err = save_project_as_with_hook(
        &sess,
        &alice,
        &env.roots,
        &mut recent,
        &obj([("target", text("root:exports/cas_copy"))]),
        &mut src,
        Some(&hook),
    )
    .unwrap_err();
    assert!(
        matches!(err, BusError::Conflict(ConflictReason::HeadMoved, _)),
        "expected conflict/head_moved, got {err:?}"
    );
    assert!(!env.exports_dir.join("cas_copy").exists(), "nothing must exist at target after head_moved");
    // Ensure no `.fid-copy-*` temp folder was leaked either.
    for entry in fs::read_dir(&env.exports_dir).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(!name.starts_with(".fid-copy-"), "leaked temp copy folder {name}");
    }
    // Source identity has zero ProjectSavedAs entries and chain still verifies.
    let id = read_store_identity(sess.store()).unwrap();
    assert!(id.saved_as.is_empty(), "refused save-as must leave no ProjectSavedAs entry");
    sess.verify_chain().unwrap();
    close_project(sess).unwrap();
}

/// AC-8: Opening `p_small` in an import root succeeds read-only, leaves the project folder
/// byte-for-byte unchanged, takes no lock (a second read-only open also succeeds), and refuses
/// `create-part` with `policy/bad_root`.
#[test]
fn ac8_import_root_read_only_open_and_write_refusal() {
    let env = TestEnv::new("ac8");
    let alice = human_principal("alice");
    let agent = agent_principal("inspector-agent");
    let mut recent = RecentProjects::new();
    let mut src = SeededSource::new(808);
    let clock = StepClock::shared(1_760_000_800);

    // Create & populate p_small in work root, close it cleanly, then stage a copy into imports root.
    let (sess, created) = create_project(
        &alice,
        &env.roots,
        &mut recent,
        &obj([("label", text("Import Fixture")), ("target", text("root:work/p_small"))]),
        &mut src,
        Arc::clone(&clock),
        test_manifests(),
    )
    .unwrap();
    let expected_ids = populate_p_small(&sess, &alice, &mut src);
    close_project(sess).unwrap();

    let import_proj = env.imports_dir.join("p_small");
    copy_dir_recursive(&env.work_dir.join("p_small"), &import_proj);
    let before = dir_snapshot(&import_proj);

    // Agent opens read-only from import root.
    let (ro_sess, open_res) = open_project(
        &agent,
        &env.roots,
        &mut recent,
        &obj([("source", text("root:imports/p_small"))]),
        &mut src,
        Arc::clone(&clock),
        test_manifests(),
    )
    .unwrap();
    assert!(open_res.read_only);
    assert!(ro_sess.is_read_only());
    assert_eq!(open_res.project, created.project);
    let loaded_ids: BTreeSet<Uuid> = ro_sess.load_containers().unwrap().keys().copied().collect();
    assert_eq!(loaded_ids, expected_ids);

    // No lock is held: a second concurrent open of the same import-root project succeeds too.
    let (ro_sess2, _) = open_project(
        &agent,
        &env.roots,
        &mut recent,
        &obj([("source", text("root:imports/p_small"))]),
        &mut src,
        Arc::clone(&clock),
        test_manifests(),
    )
    .unwrap();

    // Every write command (create-part, save-project-as) is refused with policy/bad_root.
    let w_err = ro_sess
        .commit_container(ContainerKind::Part, &agent, &obj([("label", text("Forbidden"))]), &mut src)
        .unwrap_err();
    assert!(
        matches!(w_err, BusError::Policy(PolicyReason::BadRoot, _)),
        "expected policy/bad_root on read-only create-part, got {w_err:?}"
    );
    let sa_err = save_project_as(
        &ro_sess,
        &agent,
        &env.roots,
        &mut recent,
        &obj([("target", text("root:exports/ro_copy"))]),
        &mut src,
    )
    .unwrap_err();
    assert!(matches!(sa_err, BusError::Policy(PolicyReason::BadRoot, _)));

    // Close writes no checkpoint on read-only sessions.
    assert!(close_project(ro_sess2).unwrap().checkpoint.is_none());
    assert!(close_project(ro_sess).unwrap().checkpoint.is_none());

    // Project folder in import root is byte-for-byte unchanged.
    let after = dir_snapshot(&import_proj);
    assert_eq!(before, after, "import-root project folder must be byte-for-byte unchanged");
}

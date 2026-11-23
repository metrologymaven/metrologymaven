//! Integration tests for `CORE.DEP.REPL` (`AC-1`..`AC-6`, `REQ-001`..`REQ-007`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::handler::{CancelToken as BusCancelToken, HandlerCtx};
use fid_bus::principal::{
    Channel, ChannelKind, Identity as BusIdentity, Principal, PrincipalId, SessionId, SessionKey, SessionTable,
};
use fid_canon::{digest_bytes, parse, to_canonical_bytes, CValue};
use fid_dep::recompute::bus::commit_recompute_entry;
use fid_dep::recompute::{
    execute_recompute, replay_exact, ElementRunOutput, NeverCancel, NullProgress, OverlayEvent, OverlayView,
    RecomputeRunner, RecomputeScope, RecordedCommand, ReplayOutcome,
};
use fid_dep::replace::bus::commit_replace_entry;
use fid_dep::replace::{
    apply_recompute_to_elements, command_def, overlay_from_elements, replace_input, ReplaceError, ReplaceInputParams,
    TreeElement,
};
use fid_dep::{EdgeIndex, EdgesSet, ElementState, EntityId, RecomputeMode};
use fid_journal::append::Journal;
use fid_journal::entry::{EntryTime, Manifests, GENESIS_PREV};
use fid_registry::schema::check_schema;
use fid_registry::validate::{validate, CommandDecl};
use fid_registry::Catalogue;
use fid_store::Store;
use fid_uuid::Uuid;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_dir(tag: &str) -> PathBuf {
    let n = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("fid-repl-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

fn num(n: i64) -> CValue {
    CValue::Num(n as f64)
}

fn test_manifests() -> Manifests {
    let d = digest_bytes(b"fid-dep-repl-test");
    Manifests { kernel: d, registry: d }
}

fn test_time(sec: u64) -> EntryTime {
    EntryTime { utc: format!("2026-11-23T00:00:{:02}Z", sec % 60), mono_ns: sec * 1_000_000_000, sync: "synced".into() }
}

struct TestGuiChannel {
    who: String,
    session: SessionKey,
}
impl Channel for TestGuiChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Gui
    }
    fn authenticated_identity(&self) -> Option<BusIdentity> {
        Some(BusIdentity::new(PrincipalId::new(&self.who).unwrap()))
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
}

fn alice_principal() -> Principal {
    let mut table = SessionTable::new();
    let session = table.open_session(SessionId::new("gui-repl").unwrap()).unwrap();
    let ch = TestGuiChannel { who: "alice".to_owned(), session };
    HandlerCtx::derive(&table, &ch, 0, BusCancelToken::never()).unwrap().principal().clone()
}

struct ReplaceFixture {
    elements: BTreeMap<EntityId, TreeElement>,
    graph: EdgeIndex,
    commands: BTreeMap<EntityId, RecordedCommand>,
    ids: BTreeMap<String, EntityId>,
}

fn load_replace_chain() -> ReplaceFixture {
    let raw = include_str!("../../../fixtures/core/dep/replace_chain/replace_chain.json");
    let doc = parse(raw.as_bytes()).unwrap();
    let CValue::Obj(root) = doc else { panic!("fixture object") };
    let CValue::Arr(elems) = root.get("elements").unwrap() else { panic!("elements array") };

    let mut elements = BTreeMap::new();
    let mut graph = EdgeIndex::new();
    let mut commands = BTreeMap::new();

    for item in elems {
        let CValue::Obj(e) = item else { panic!("elem obj") };
        let CValue::Str(id_s) = e.get("id").unwrap() else { panic!("id") };
        let CValue::Str(name) = e.get("name").unwrap() else { panic!("name") };
        let CValue::Str(kind) = e.get("kind").unwrap() else { panic!("kind") };
        let CValue::Str(group) = e.get("group").unwrap() else { panic!("group") };
        let CValue::Arr(tags_arr) = e.get("tags").unwrap() else { panic!("tags") };
        let tags: Vec<String> = tags_arr
            .iter()
            .map(|t| match t {
                CValue::Str(s) => s.clone(),
                _ => panic!("tag str"),
            })
            .collect();
        let CValue::Obj(metadata) = e.get("metadata").unwrap() else { panic!("metadata") };
        let content = e.get("content").unwrap().clone();
        let id = Uuid::parse(id_s).unwrap();
        elements.insert(
            id,
            TreeElement {
                id,
                kind: kind.clone(),
                name: name.clone(),
                group: group.clone(),
                tags,
                metadata: metadata.clone(),
                state: ElementState::Current,
                content,
            },
        );
        let CValue::Arr(inps) = e.get("inputs").unwrap() else { panic!("inputs") };
        let mut inp_ids: Vec<EntityId> = inps
            .iter()
            .map(|v| match v {
                CValue::Str(s) => Uuid::parse(s).unwrap(),
                _ => panic!("input uuid"),
            })
            .collect();
        inp_ids.sort();
        graph.apply(&[EdgesSet::new(id, inp_ids).unwrap()]).unwrap();
        if let Some(CValue::Str(cmd)) = e.get("command") {
            commands.insert(id, RecordedCommand { command: cmd.clone(), args: e.get("args").unwrap().clone() });
        }
    }

    let CValue::Obj(ids_obj) = root.get("ids").unwrap() else { panic!("ids") };
    let mut ids = BTreeMap::new();
    for (k, v) in ids_obj {
        let CValue::Str(s) = v else { panic!("id str") };
        ids.insert(k.clone(), Uuid::parse(s).unwrap());
    }

    ReplaceFixture { elements, graph, commands, ids }
}

fn elem_val(el: &TreeElement) -> i64 {
    let CValue::Obj(m) = &el.content else { panic!("content obj") };
    let CValue::Num(v) = m.get("value").unwrap() else { panic!("value num") };
    *v as i64
}

struct ChainTestRunner;

impl RecomputeRunner for ChainTestRunner {
    fn run_element(
        &self,
        id: EntityId,
        recorded: &RecordedCommand,
        overlay: &OverlayView,
    ) -> Result<ElementRunOutput, String> {
        let CValue::Obj(args) = &recorded.args else { return Err("bad_args".into()) };
        let CValue::Str(inp_s) = args.get("input").unwrap() else { return Err("missing_input".into()) };
        let inp_id = Uuid::parse(inp_s).unwrap();
        let inp_ent = overlay.get(&inp_id).ok_or_else(|| "missing_input".to_owned())?;
        let CValue::Obj(im) = &inp_ent.body else { return Err("bad_body".into()) };
        let CValue::Num(v_in) = im.get("value").unwrap() else { return Err("missing_value".into()) };
        let v_out = match recorded.command.as_str() {
            "test.add_const@1" => {
                let CValue::Num(d) = args.get("delta").unwrap() else { return Err("missing_delta".into()) };
                (*v_in as i64) + (*d as i64)
            }
            "test.mul_const@1" => {
                let CValue::Num(f) = args.get("factor").unwrap() else { return Err("missing_factor".into()) };
                (*v_in as i64) * (*f as i64)
            }
            other => return Err(format!("unknown_cmd:{other}")),
        };
        let body = obj([("id", text(&id.to_string())), ("value", num(v_out))]);
        let bytes = to_canonical_bytes(&body).unwrap();
        let digest = digest_bytes(&bytes);
        Ok(ElementRunOutput {
            output_digest: digest,
            events: vec![OverlayEvent { event_type: "TestElementComputed".into(), body: body.clone() }],
            updated_body: body,
        })
    }
}

/// Validates `project.replace_input@1` command definition against registry schema and rules.
#[test]
fn replace_command_def_validates_cleanly() {
    let def = command_def().unwrap();
    assert_eq!(def.id, "project.replace_input");
    assert_eq!(def.major, 1);
    let catalogue = Catalogue::geo_frame().unwrap();
    assert_eq!(check_schema(&def.params_schema, &catalogue), vec![]);
    assert_eq!(check_schema(&def.result_schema, &catalogue), vec![]);
    let decls = vec![CommandDecl::from_def(&def)];
    assert_eq!(validate(&decls), vec![]);
}

/// AC-1 & AC-2: Given `replace_chain` in `auto` mode (`m=10`, `s1=m+1=11`, `s2=s1*2=22`, staged
/// `m'=20`), `project.replace_input@1` keeps `m`'s ID, name, group, tags and metadata while giving
/// `m` value 20, removes `m'`, marks `s1` and `s2` outdated in the same commit, and in auto mode
/// submits one recompute entry bringing `s1=21` and `s2=42` with unchanged IDs. The journal holds
/// exactly one `project.replace_input@1` entry followed by one `project.recompute_dependents@1`
/// entry listing `s1` and `s2`.
#[test]
fn ac1_and_ac2_replace_input_auto_recompute_and_two_journal_entries() {
    let dir = temp_dir("ac1_ac2");
    let store = Store::create(&dir.join("proj")).unwrap();
    let jr = Journal::on_store(&store, "record").unwrap();
    let alice = alice_principal();

    let mut fx = load_replace_chain();
    let id_m = fx.ids["m"];
    let id_s1 = fx.ids["s1"];
    let id_s2 = fx.ids["s2"];
    let id_mp = fx.ids["m_prime"];
    let orig_m = fx.elements[&id_m].clone();

    let params = ReplaceInputParams { target: id_m, replacement: id_mp };
    let outcome = replace_input(&mut fx.elements, &mut fx.graph, RecomputeMode::Auto, params).unwrap();

    // REQ-001: target keeps ID, name, group, tags, metadata; content is now 20.
    let new_m = &fx.elements[&id_m];
    assert_eq!(new_m.id, id_m);
    assert_eq!(new_m.name, orig_m.name);
    assert_eq!(new_m.group, orig_m.group);
    assert_eq!(new_m.tags, orig_m.tags);
    assert_eq!(new_m.metadata, orig_m.metadata);
    assert_eq!(elem_val(new_m), 20);

    // REQ-003: replacement `m'` is removed from both tree and graph.
    assert!(!fx.elements.contains_key(&id_mp));
    assert!(!fx.graph.contains(&id_mp));

    // REQ-004: s1 and s2 keep their IDs and are marked `outdated` in the same commit.
    assert_eq!(fx.elements[&id_s1].state, ElementState::Outdated);
    assert_eq!(fx.elements[&id_s2].state, ElementState::Outdated);
    let marked = outcome.elements_marked.as_ref().expect("dependents must be marked");
    assert_eq!(marked.elements(), &[id_s1, id_s2]);

    // Commit entry 1: `project.replace_input@1`.
    let head1 =
        commit_replace_entry(&jr, "main", &alice, test_time(1), test_manifests(), params, &outcome, Some(GENESIS_PREV))
            .unwrap();

    // REQ-005: auto mode submits one recompute of `[s1, s2]`.
    let sub = outcome.submission.as_ref().expect("auto mode must submit recompute");
    assert_eq!(sub.elements(), &[id_s1, id_s2]);
    let scope = RecomputeScope::Elements(sub.elements().to_vec());
    let overlay = overlay_from_elements(head1, &fx.elements).unwrap();
    let exec =
        execute_recompute(&fx.graph, &overlay, &fx.commands, &scope, &ChainTestRunner, &NeverCancel, &NullProgress)
            .unwrap();
    let _head2 = commit_recompute_entry(&jr, "main", &alice, test_time(2), test_manifests(), &scope, &exec).unwrap();
    apply_recompute_to_elements(&mut fx.elements, &exec);

    assert_eq!(elem_val(&fx.elements[&id_m]), 20);
    assert_eq!(elem_val(&fx.elements[&id_s1]), 21);
    assert_eq!(elem_val(&fx.elements[&id_s2]), 42);
    assert_eq!(fx.elements[&id_s1].state, ElementState::Current);
    assert_eq!(fx.elements[&id_s2].state, ElementState::Current);

    // AC-2: journal holds exactly 2 entries: replace_input@1 then recompute_dependents@1 listing s1, s2.
    let rows: Vec<(i64, Vec<u8>)> = store
        .read(|conn| {
            let mut st = conn.prepare("SELECT seq, header FROM entries WHERE stream = 'record' ORDER BY seq ASC")?;
            let iter = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let mut out = Vec::new();
            for row in iter {
                out.push(row?);
            }
            Ok(out)
        })
        .unwrap();
    assert_eq!(rows.len(), 2);
    let CValue::Obj(h1) = parse(&rows[0].1).unwrap() else { panic!("h1") };
    let CValue::Obj(c1) = h1.get("command").unwrap() else { panic!("c1") };
    assert_eq!(c1.get("id"), Some(&text("project.replace_input")));
    let CValue::Obj(h2) = parse(&rows[1].1).unwrap() else { panic!("h2") };
    let CValue::Obj(c2) = h2.get("command").unwrap() else { panic!("c2") };
    assert_eq!(c2.get("id"), Some(&text("project.recompute_dependents")));
    let rec_ids: Vec<EntityId> = exec.committed.recomputed.iter().map(|r| r.id).collect();
    assert_eq!(rec_ids, vec![id_s1, id_s2]);

    let _ = fs::remove_dir_all(&dir);
}

/// AC-3: Replacement of a different registered type fails with `type_mismatch` and commits nothing.
#[test]
fn ac3_type_mismatch_fails_and_commits_nothing() {
    let mut fx = load_replace_chain();
    let before_elements = fx.elements.clone();
    let before_graph = fx.graph.clone();

    let params = ReplaceInputParams { target: fx.ids["m"], replacement: fx.ids["cad_other"] };
    let err = replace_input(&mut fx.elements, &mut fx.graph, RecomputeMode::Auto, params).unwrap_err();
    assert!(
        matches!(err, ReplaceError::TypeMismatch { ref target_kind, ref replacement_kind, .. }
            if target_kind == "test.mesh" && replacement_kind == "test.cad"),
        "expected TypeMismatch, got {err:?}"
    );
    assert_eq!(err.code(), "type_mismatch");
    assert_eq!(fx.elements, before_elements, "state must be unchanged after refusal");
    assert_eq!(fx.graph, before_graph, "graph must be unchanged after refusal");
}

/// AC-4 & AC-5: Undoing replace + recompute (by `head_move` replay to pre-replace head and
/// recomputing) restores `m=10`, `s1=11`, `s2=22`; replaying the AC-1 journal sequence in exact
/// mode produces `ReplayOutcome::Identical` with byte-identical digests.
#[test]
fn ac4_undo_restores_previous_values_and_ac5_exact_replay_identical() {
    let fx0 = load_replace_chain();
    let id_m = fx0.ids["m"];
    let id_s1 = fx0.ids["s1"];
    let id_s2 = fx0.ids["s2"];
    let id_mp = fx0.ids["m_prime"];

    // Run replace + auto recompute.
    let mut fx_live = load_replace_chain();
    let params = ReplaceInputParams { target: id_m, replacement: id_mp };
    let outcome1 = replace_input(&mut fx_live.elements, &mut fx_live.graph, RecomputeMode::Auto, params).unwrap();
    let scope = RecomputeScope::Elements(outcome1.submission.as_ref().unwrap().elements().to_vec());
    let ov1 = overlay_from_elements(GENESIS_PREV, &fx_live.elements).unwrap();
    let exec1 = execute_recompute(
        &fx_live.graph,
        &ov1,
        &fx_live.commands,
        &scope,
        &ChainTestRunner,
        &NeverCancel,
        &NullProgress,
    )
    .unwrap();
    apply_recompute_to_elements(&mut fx_live.elements, &exec1);
    assert_eq!(elem_val(&fx_live.elements[&id_m]), 20);
    assert_eq!(elem_val(&fx_live.elements[&id_s1]), 21);
    assert_eq!(elem_val(&fx_live.elements[&id_s2]), 42);

    // AC-4: Undo twice (head_move back over recompute and replace -> replay to head 0):
    // target regains value 10; marking dependents outdated and recomputing returns s1=11, s2=22.
    let mut fx_undone = load_replace_chain();
    assert_eq!(elem_val(&fx_undone.elements[&id_m]), 10);
    fx_undone.elements.get_mut(&id_s1).unwrap().state = ElementState::Outdated;
    fx_undone.elements.get_mut(&id_s2).unwrap().state = ElementState::Outdated;
    let ov_undo = overlay_from_elements(GENESIS_PREV, &fx_undone.elements).unwrap();
    let exec_undo = execute_recompute(
        &fx_undone.graph,
        &ov_undo,
        &fx_undone.commands,
        &RecomputeScope::AllOutdated,
        &ChainTestRunner,
        &NeverCancel,
        &NullProgress,
    )
    .unwrap();
    apply_recompute_to_elements(&mut fx_undone.elements, &exec_undo);
    assert_eq!(elem_val(&fx_undone.elements[&id_m]), 10);
    assert_eq!(elem_val(&fx_undone.elements[&id_s1]), 11);
    assert_eq!(elem_val(&fx_undone.elements[&id_s2]), 22);
    assert_eq!(fx_undone.elements[&id_m].content_digest().unwrap(), fx0.elements[&id_m].content_digest().unwrap());

    // AC-5: Exact replay of replace_input + recompute from initial state -> IDENTICAL.
    let mut fx_replay = load_replace_chain();
    let outcome_r = replace_input(&mut fx_replay.elements, &mut fx_replay.graph, RecomputeMode::Auto, params).unwrap();
    assert_eq!(outcome_r, outcome1, "replace_input events must replay byte-identically");
    let ov_r = overlay_from_elements(GENESIS_PREV, &fx_replay.elements).unwrap();
    let (rep_outcome, rep_exec) =
        replay_exact(&fx_replay.graph, &ov_r, &fx_replay.commands, &scope, &exec1.committed, &ChainTestRunner).unwrap();
    assert_eq!(rep_outcome, ReplayOutcome::Identical);
    assert_eq!(rep_exec.canonical_events_bytes().unwrap(), exec1.canonical_events_bytes().unwrap());
}

/// AC-6: Replacement element that already has dependents fails with `replacement_in_use` and
/// leaves state and graph unchanged.
#[test]
fn ac6_replacement_with_dependents_fails_with_replacement_in_use() {
    let mut fx = load_replace_chain();
    let before_elements = fx.elements.clone();
    let before_graph = fx.graph.clone();

    let params = ReplaceInputParams { target: fx.ids["m"], replacement: fx.ids["m_used"] };
    let err = replace_input(&mut fx.elements, &mut fx.graph, RecomputeMode::Auto, params).unwrap_err();
    assert!(
        matches!(err, ReplaceError::ReplacementInUse { replacement, ref dependents }
            if replacement == fx.ids["m_used"] && dependents == &[fx.ids["u1"]]),
        "expected ReplacementInUse, got {err:?}"
    );
    assert_eq!(err.code(), "replacement_in_use");
    assert_eq!(fx.elements, before_elements);
    assert_eq!(fx.graph, before_graph);
}

//! Bus handler and error mapping tests for `CORE.JRN.UNDO.MOVE` (`journal.undo@1`,
//! `journal.redo@1`, `journal.show_undo_tree@1`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::handler::{
    CancelToken, EntityId, EntityRef, Handler, HandlerCtx, HandlerSet, SettingKey, StateView, ValidParams,
};
use fid_bus::principal::{Channel, ChannelKind, Identity, PrincipalId, SessionId, SessionKey, SessionTable};
use fid_canon::{parse, CValue};
use fid_journal::undo::{UndoEntry, UndoEntryKind, UndoTree, HEAD_MOVED, REDO, SHOW_UNDO_TREE, UNDO};
use fid_journal_cmd::{register_undo, run_undo, UndoHandler, UndoOp, UndoSource};
use fid_types::{Digest, SideEffect};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/jrn/undo/move");

struct DummyView;
impl StateView for DummyView {
    fn entity(&self, _id: &EntityId) -> Option<EntityRef<'_>> {
        None
    }
    fn head(&self) -> fid_canon::Digest {
        fid_canon::Digest([0; 32])
    }
    fn setting(&self, _k: &SettingKey) -> Option<CValue> {
        None
    }
}

struct TestChannel {
    kind: ChannelKind,
    session: SessionKey,
}

impl Channel for TestChannel {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    fn authenticated_identity(&self) -> Option<Identity> {
        Some(Identity::new(PrincipalId::new("alice").unwrap()))
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
}

struct MemUndoSource {
    tree: RwLock<UndoTree>,
}

impl UndoSource for MemUndoSource {
    fn tree(&self) -> Result<UndoTree, String> {
        Ok(self.tree.read().unwrap().clone())
    }
    fn default_branch(&self, _ctx: &HandlerCtx) -> Result<String, String> {
        Ok("main".to_string())
    }
}

fn human_ctx() -> HandlerCtx {
    let mut table = SessionTable::new();
    let session = table.open_session(SessionId::new("sess-1").unwrap()).unwrap();
    HandlerCtx::derive(&table, &TestChannel { kind: ChannelKind::Gui, session }, 0, CancelToken::never()).unwrap()
}

fn load_tree(name: &str, take_n: Option<usize>) -> (UndoTree, BTreeMap<String, Digest>) {
    let raw = std::fs::read(format!("{FIXTURES}/{name}")).unwrap();
    let CValue::Obj(root) = parse(&raw).unwrap() else { panic!() };
    let CValue::Arr(entries) = root.get("entries").unwrap() else { panic!() };
    let slice = match take_n {
        Some(n) => &entries[..n],
        None => entries.as_slice(),
    };
    let mut tree = UndoTree::new();
    let mut map = BTreeMap::new();
    for v in slice {
        let CValue::Obj(o) = v else { panic!() };
        let CValue::Str(n) = o.get("name").unwrap() else { panic!() };
        let CValue::Num(seq) = o.get("seq").unwrap() else { panic!() };
        let CValue::Str(hash) = o.get("hash").unwrap() else { panic!() };
        let CValue::Str(branch) = o.get("branch").unwrap() else { panic!() };
        let lp = match o.get("logical_parent").unwrap() {
            CValue::Null => None,
            CValue::Str(s) => Some(Digest::parse(s).unwrap()),
            _ => panic!(),
        };
        let CValue::Str(cmd) = o.get("command").unwrap() else { panic!() };
        let CValue::Num(major) = o.get("major").unwrap() else { panic!() };
        let CValue::Str(cls) = o.get("class").unwrap() else { panic!() };
        let CValue::Bool(undoable) = o.get("undoable").unwrap() else { panic!() };
        let d = Digest::parse(hash).unwrap();
        map.insert(n.clone(), d);
        tree.apply(&UndoEntry {
            seq: *seq as u64,
            hash: d,
            branch: branch.clone(),
            logical_parent: lp,
            kind: UndoEntryKind::Command {
                command: cmd.clone(),
                major: *major as u32,
                class: SideEffect::parse(cls).unwrap(),
                undoable: *undoable,
            },
        })
        .unwrap();
    }
    (tree, map)
}

#[test]
fn register_and_run_undo_redo_and_show_tree_on_bus() {
    let (tree, map) = load_tree("linear.json", None);
    let src = Arc::new(MemUndoSource { tree: RwLock::new(tree) });
    let dyn_src: Arc<dyn UndoSource> = src.clone();

    let mut set = HandlerSet::new();
    register_undo(&mut set, &dyn_src).unwrap();
    assert!(set.get(&format!("{UNDO}@1")).is_ok());
    assert!(set.get(&format!("{REDO}@1")).is_ok());
    assert!(set.get(&format!("{SHOW_UNDO_TREE}@1")).is_ok());

    let ctx = human_ctx();
    let params = CValue::Obj(BTreeMap::from([("steps".to_string(), CValue::Num(2.0))]));
    let (events, result) = run_undo(UndoOp::Undo, dyn_src.as_ref(), &params, &ctx).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].type_id().as_str(), HEAD_MOVED);
    let CValue::Obj(ro) = result else { panic!() };
    assert_eq!(ro.get("from"), Some(&CValue::Str(map["s5"].as_str().to_string())));
    assert_eq!(ro.get("to"), Some(&CValue::Str(map["s3"].as_str().to_string())));

    let handler = UndoHandler::new(UndoOp::Undo, dyn_src.clone());
    let vp = ValidParams::for_tests(params);
    let evs = handler.run(&DummyView, &vp, &ctx).unwrap();
    assert_eq!(evs.len(), 1);

    let (tree_evs, tree_res) =
        run_undo(UndoOp::ShowUndoTree, dyn_src.as_ref(), &CValue::Obj(BTreeMap::new()), &ctx).unwrap();
    assert!(tree_evs.is_empty());
    let CValue::Obj(to) = tree_res else { panic!() };
    let CValue::Arr(nodes) = to.get("nodes").unwrap() else { panic!() };
    assert_eq!(nodes.len(), 6);

    let err = run_undo(UndoOp::Redo, dyn_src.as_ref(), &CValue::Obj(BTreeMap::new()), &ctx).unwrap_err();
    assert_eq!(err.code().as_str(), "precondition");
    assert_eq!(err.reason(), "nothing_to_redo");
}

#[test]
fn released_and_not_undoable_barriers_map_to_v3_bus_errors() {
    let (tree, _) = load_tree("released.json", None);
    let src = Arc::new(MemUndoSource { tree: RwLock::new(tree) });
    let ctx = human_ctx();

    let err = run_undo(UndoOp::Undo, src.as_ref(), &CValue::Obj(BTreeMap::new()), &ctx).unwrap_err();
    assert_eq!(err.code().as_str(), "precondition");
    assert_eq!(err.reason(), "not_undoable");

    let (t3, _) = load_tree("released.json", Some(4));
    let src3 = Arc::new(MemUndoSource { tree: RwLock::new(t3) });
    let params = CValue::Obj(BTreeMap::from([("steps".to_string(), CValue::Num(2.0))]));
    let err_w3 = run_undo(UndoOp::Undo, src3.as_ref(), &params, &ctx).unwrap_err();
    assert_eq!(err_w3.code().as_str(), "precondition");
    assert_eq!(err_w3.reason(), "released_record");
}

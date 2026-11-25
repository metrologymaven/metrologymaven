//! Acceptance tests for `CORE.JRN.UNDO.MOVE` (`AC-1`..`AC-5`) and `REQ-001`..`REQ-008`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue, Digest as CanonDigest};
use fid_journal::undo::{
    command_defs, UndoEntry, UndoEntryKind, UndoError, UndoTree, HEAD_MOVED, MAX_STEPS, MAX_TREE_NODES, REDO,
    SHOW_UNDO_TREE, UNDO,
};
use fid_journal::{
    AiAssistance, AppendRequest, Clock, CommandDraft, DraftBody, EntryDraft, EntryTime, Journal, Manifests,
    PrincipalDraft, PrincipalKind, VerifyOptions,
};
use fid_store::{OpenMode, ProjectLocation};
use fid_types::{Determinism, Digest, SideEffect};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/jrn/undo/move");
const STREAM: &str = "record";
const TEST_NOW: u64 = 1_893_456_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("fid-jrn-undo-{tag}-{id}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct FixedClock(u64);

impl Clock for FixedClock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0)
    }
}

fn new_journal(tag: &str) -> (TempDir, PathBuf, Journal) {
    let dir = TempDir::new(tag);
    let root = dir.0.join("proj");
    let (loc, lock) = ProjectLocation::create(&root).unwrap();
    let j = Journal::open_with_clock(loc, lock, STREAM, Arc::new(FixedClock(TEST_NOW))).unwrap();
    (dir, root, j)
}

fn reopen(root: &Path) -> Journal {
    let (loc, lock) = ProjectLocation::open(root, OpenMode::Write).unwrap();
    Journal::open_with_clock(loc, lock.unwrap(), STREAM, Arc::new(FixedClock(TEST_NOW))).unwrap()
}

fn load_fixture(name: &str) -> Vec<(String, UndoEntry)> {
    let raw = std::fs::read(format!("{FIXTURES}/{name}")).unwrap();
    let val = parse(&raw).unwrap();
    let CValue::Obj(root) = val else { panic!("fixture root must be object") };
    let CValue::Arr(entries) = root.get("entries").unwrap() else { panic!("entries must be array") };
    entries
        .iter()
        .map(|v| {
            let CValue::Obj(o) = v else { panic!("entry must be object") };
            let CValue::Str(name) = o.get("name").unwrap() else { panic!() };
            let CValue::Num(seq) = o.get("seq").unwrap() else { panic!() };
            let CValue::Str(hash) = o.get("hash").unwrap() else { panic!() };
            let CValue::Str(branch) = o.get("branch").unwrap() else { panic!() };
            let lp = match o.get("logical_parent").unwrap() {
                CValue::Null => None,
                CValue::Str(s) => Some(Digest::parse(s).unwrap()),
                other => panic!("unexpected logical_parent {other:?}"),
            };
            let CValue::Str(command) = o.get("command").unwrap() else { panic!() };
            let CValue::Num(major) = o.get("major").unwrap() else { panic!() };
            let CValue::Str(cls) = o.get("class").unwrap() else { panic!() };
            let CValue::Bool(undoable) = o.get("undoable").unwrap() else { panic!() };
            (
                name.clone(),
                UndoEntry {
                    seq: *seq as u64,
                    hash: Digest::parse(hash).unwrap(),
                    branch: branch.clone(),
                    logical_parent: lp,
                    kind: UndoEntryKind::Command {
                        command: command.clone(),
                        major: *major as u32,
                        class: SideEffect::parse(cls).unwrap(),
                        undoable: *undoable,
                    },
                },
            )
        })
        .collect()
}

fn zero_digest() -> CanonDigest {
    CanonDigest([0u8; 32])
}

fn cmd_draft(
    branch: &str,
    logical_parent: Option<CanonDigest>,
    cmd: &str,
    class: SideEffect,
    seq_hint: u64,
) -> EntryDraft {
    EntryDraft {
        stream: STREAM.to_string(),
        branch: branch.to_string(),
        logical_parent,
        time: EntryTime {
            utc: format!("2026-11-23T00:{:02}:{:02}Z", seq_hint / 60, seq_hint % 60),
            mono_ns: seq_hint * 1_000_000,
            sync: "synced".to_string(),
        },
        principal: PrincipalDraft {
            kind: PrincipalKind::Human,
            identity: CValue::Str("alice".to_string()),
            on_behalf_of: None,
        },
        ai_assistance: AiAssistance::None,
        manifests: Manifests { registry: zero_digest(), kernel: zero_digest() },
        reason: None,
        human_assertion: None,
        agent_coedited: false,
        body: DraftBody::Command(CommandDraft {
            id: cmd.to_string(),
            major: 1,
            class,
            determinism: Determinism::D0,
            seed: 0,
            args: CValue::Obj(BTreeMap::new()),
            events: Vec::new(),
            inputs: Vec::new(),
            consumed_r1: Vec::new(),
            outputs: Vec::new(),
            algorithms: Vec::new(),
            event_sources: Vec::new(),
        }),
    }
}

fn head_move_draft(branch: &str, from: Digest, to: Digest, seq_hint: u64) -> EntryDraft {
    let to_c = CanonDigest(to.to_bytes());
    let from_c = CanonDigest(from.to_bytes());
    EntryDraft {
        stream: STREAM.to_string(),
        branch: branch.to_string(),
        logical_parent: Some(to_c),
        time: EntryTime {
            utc: format!("2026-11-23T00:{:02}:{:02}Z", seq_hint / 60, seq_hint % 60),
            mono_ns: seq_hint * 1_000_000,
            sync: "synced".to_string(),
        },
        principal: PrincipalDraft {
            kind: PrincipalKind::Human,
            identity: CValue::Str("alice".to_string()),
            on_behalf_of: None,
        },
        ai_assistance: AiAssistance::None,
        manifests: Manifests { registry: zero_digest(), kernel: zero_digest() },
        reason: None,
        human_assertion: None,
        agent_coedited: false,
        body: DraftBody::HeadMove(fid_journal::HeadMove { from: from_c, to: to_c }),
    }
}

fn is_undoable_policy(cmd: &str, _major: u32) -> bool {
    !matches!(cmd, "project.create_project" | "journal.lock_step" | "report.release_record")
}

/// AC-1, AC-2, AC-5: real SQLite `Journal` append of `linear` states s0..s5, two undos to s3,
/// sibling command after undo, `show_undo_tree` showing both siblings, `redo --child s4`, and
/// reopen in a new session yielding an identical undo tree, head, and `verify_chain` passing.
#[test]
fn ac1_ac2_ac5_linear_undo_sibling_redo_and_reopen_replay() {
    let fixture = load_fixture("linear.json");
    assert_eq!(fixture.len(), 6);

    let (_dir, root, j) = new_journal("ac1-ac2-ac5");
    let mut hashes: BTreeMap<String, Digest> = BTreeMap::new();
    let mut prev_state: Option<CanonDigest> = None;

    for (i, (name, entry)) in fixture.iter().enumerate() {
        let UndoEntryKind::Command { command, class, .. } = &entry.kind else { panic!() };
        let draft = cmd_draft("main", prev_state, command, *class, i as u64 + 1);
        let app = j.append(AppendRequest { draft, expected_prev: None, blobs: Vec::new(), salts: None }).unwrap();
        let d = Digest::from_bytes(app.0);
        hashes.insert(name.clone(), d);
        prev_state = Some(app);
    }

    // AC-1: Undo twice -> head is s3, two head_move entries appended, all 6 command entries intact.
    for step in 0..2u64 {
        let tree = j.undo_tree(is_undoable_policy).unwrap();
        let moved = tree.plan_undo("main", 1).unwrap();
        assert_eq!(moved.type_id(), HEAD_MOVED);
        let draft = head_move_draft("main", moved.from, moved.to, 10 + step);
        j.append(AppendRequest { draft, expected_prev: None, blobs: Vec::new(), salts: None }).unwrap();
    }
    let tree = j.undo_tree(is_undoable_policy).unwrap();
    assert_eq!(tree.head("main"), Some(hashes["s3"]));
    assert_eq!(tree.state_count(), 6);

    // AC-2: Commit a new command after undo -> logical_parent is s3, sibling of s4.
    let lp = tree.next_logical_parent("main").unwrap();
    assert_eq!(lp, hashes["s3"]);
    let new_app = j
        .append(AppendRequest {
            draft: cmd_draft("main", Some(CanonDigest(lp.to_bytes())), "feature.fit_sphere", SideEffect::W2, 20),
            expected_prev: None,
            blobs: Vec::new(),
            salts: None,
        })
        .unwrap();
    let s_new = Digest::from_bytes(new_app.0);

    let tree = j.undo_tree(is_undoable_policy).unwrap();
    assert_eq!(tree.head("main"), Some(s_new));
    assert_eq!(tree.node(&s_new).unwrap().parent, Some(hashes["s3"]));
    assert_eq!(tree.node(&hashes["s3"]).unwrap().children, vec![hashes["s4"], s_new]);

    let shown = tree.show_tree("main", None, None).unwrap();
    let s3_view = shown.nodes.iter().find(|n| n.state == hashes["s3"]).unwrap();
    assert_eq!(s3_view.children, vec![hashes["s4"], s_new]);

    // Undo back to s3, then redo --child s4 -> moves to s4.
    let undo_to_s3 = tree.plan_undo("main", 1).unwrap();
    assert_eq!(undo_to_s3.to, hashes["s3"]);
    j.append(AppendRequest {
        draft: head_move_draft("main", undo_to_s3.from, undo_to_s3.to, 21),
        expected_prev: None,
        blobs: Vec::new(),
        salts: None,
    })
    .unwrap();

    let tree = j.undo_tree(is_undoable_policy).unwrap();
    let redo_s4 = tree.plan_redo("main", Some(hashes["s4"]), 1).unwrap();
    assert_eq!(redo_s4.from, hashes["s3"]);
    assert_eq!(redo_s4.to, hashes["s4"]);
    j.append(AppendRequest {
        draft: head_move_draft("main", redo_s4.from, redo_s4.to, 22),
        expected_prev: None,
        blobs: Vec::new(),
        salts: None,
    })
    .unwrap();

    let before_close = j.undo_tree(is_undoable_policy).unwrap();
    assert_eq!(before_close.head("main"), Some(hashes["s4"]));
    drop(j);

    // AC-5: Reopen in a new session -> identical undo tree, head, and verify_chain passes.
    let j2 = reopen(&root);
    let report = j2.verify_chain(&VerifyOptions::default()).unwrap();
    assert!(report.ok(), "verify_chain failed: {:?}", report.failure);
    let after_reopen = j2.undo_tree(is_undoable_policy).unwrap();
    assert_eq!(after_reopen, before_close);
    assert_eq!(after_reopen.head("main"), Some(hashes["s4"]));
}

/// AC-3: Head with children visited in order `a` then `b` -> redo without child moves to `b`;
/// visiting `a` again makes `a` most recently visited.
#[test]
fn ac3_redo_selects_most_recently_visited_child_and_nothing_to_redo() {
    let fixture = load_fixture("linear.json");
    let mut tree = UndoTree::new();
    let mut by_name = BTreeMap::new();
    for (name, entry) in &fixture[..4] {
        tree.apply(entry).unwrap();
        by_name.insert(name.clone(), entry.hash);
    }
    let s3 = by_name["s3"];
    // Nothing to redo at leaf s3.
    let err = tree.plan_redo("main", None, 1).unwrap_err();
    assert_eq!(err.bus_code(), ("precondition", "nothing_to_redo"));

    // Create child a, undo to s3, create child b, undo to s3 -> b was visited more recently.
    let a_hash = Digest::parse(&format!("sha256:{:064x}", 0xA1u64)).unwrap();
    let b_hash = Digest::parse(&format!("sha256:{:064x}", 0xB2u64)).unwrap();
    let hm1 = Digest::parse(&format!("sha256:{:064x}", 0xF1u64)).unwrap();
    let hm2 = Digest::parse(&format!("sha256:{:064x}", 0xF2u64)).unwrap();

    tree.apply(&UndoEntry {
        seq: 5,
        hash: a_hash,
        branch: "main".into(),
        logical_parent: Some(s3),
        kind: UndoEntryKind::Command {
            command: "sketch.add_line".into(),
            major: 1,
            class: SideEffect::W2,
            undoable: true,
        },
    })
    .unwrap();
    tree.apply(&UndoEntry {
        seq: 6,
        hash: hm1,
        branch: "main".into(),
        logical_parent: Some(s3),
        kind: UndoEntryKind::HeadMove { from: a_hash, to: s3 },
    })
    .unwrap();
    tree.apply(&UndoEntry {
        seq: 7,
        hash: b_hash,
        branch: "main".into(),
        logical_parent: Some(s3),
        kind: UndoEntryKind::Command {
            command: "sketch.add_circle".into(),
            major: 1,
            class: SideEffect::W2,
            undoable: true,
        },
    })
    .unwrap();
    tree.apply(&UndoEntry {
        seq: 8,
        hash: hm2,
        branch: "main".into(),
        logical_parent: Some(s3),
        kind: UndoEntryKind::HeadMove { from: b_hash, to: s3 },
    })
    .unwrap();

    // Redo without child -> b.
    let redo_b = tree.plan_redo("main", None, 1).unwrap();
    assert_eq!(redo_b.to, b_hash);

    // Visit a explicitly, then undo back to s3 -> now a has the highest visited_at.
    let hm3 = Digest::parse(&format!("sha256:{:064x}", 0xF3u64)).unwrap();
    let hm4 = Digest::parse(&format!("sha256:{:064x}", 0xF4u64)).unwrap();
    tree.apply(&UndoEntry {
        seq: 9,
        hash: hm3,
        branch: "main".into(),
        logical_parent: Some(a_hash),
        kind: UndoEntryKind::HeadMove { from: s3, to: a_hash },
    })
    .unwrap();
    tree.apply(&UndoEntry {
        seq: 10,
        hash: hm4,
        branch: "main".into(),
        logical_parent: Some(s3),
        kind: UndoEntryKind::HeadMove { from: a_hash, to: s3 },
    })
    .unwrap();
    let redo_a = tree.plan_redo("main", None, 1).unwrap();
    assert_eq!(redo_a.to, a_hash);
}

/// AC-4: W3 released record barrier (`precondition/released_record`), `not_undoable` barrier
/// (`precondition/not_undoable`), multi-step all-or-nothing refusal, and step/depth bounds.
#[test]
fn ac4_barriers_and_bounds() {
    let fixture = load_fixture("released.json");
    let mut tree = UndoTree::new();
    for (_, entry) in &fixture {
        tree.apply(entry).unwrap();
    }
    // Head is s4 (not_undoable W2 command).
    let err_nu = tree.plan_undo("main", 1).unwrap_err();
    assert_eq!(err_nu.bus_code(), ("precondition", "not_undoable"));
    assert!(matches!(err_nu, UndoError::NotUndoable { .. }));

    // Build tree up to s3 (s0 -> s1 -> s2[W3] -> s3[W2 undoable]).
    let mut tree3 = UndoTree::new();
    for (_, entry) in &fixture[..4] {
        tree3.apply(entry).unwrap();
    }
    // 1 step from s3 -> s2 succeeds (does not cross s2).
    let one = tree3.plan_undo("main", 1).unwrap();
    assert_eq!(one.to, fixture[2].1.hash);
    // 2 steps from s3 would cross W3 s2 -> all-or-nothing refusal with released_record!
    let err_w3 = tree3.plan_undo("main", 2).unwrap_err();
    assert_eq!(err_w3.bus_code(), ("precondition", "released_record"));
    assert!(matches!(err_w3, UndoError::ReleasedRecord { .. }));

    // Step and depth bounds (REQ-007, REQ-008).
    assert_eq!(tree3.plan_undo("main", 0).unwrap_err().bus_code(), ("validation", "bad_range"));
    assert_eq!(tree3.plan_undo("main", MAX_STEPS + 1).unwrap_err().bus_code(), ("validation", "bad_range"));
    assert_eq!(tree3.show_tree("main", None, Some(0)).unwrap_err().bus_code(), ("validation", "bad_range"));
    assert_eq!(
        tree3.show_tree("main", None, Some(MAX_TREE_NODES as u32 + 1)).unwrap_err().bus_code(),
        ("validation", "bad_range")
    );
}

#[test]
fn command_defs_match_spec_contract() {
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 3);
    assert_eq!(defs[0].id, UNDO);
    assert_eq!(defs[0].side_effect, SideEffect::W2);
    assert_eq!(defs[1].id, REDO);
    assert_eq!(defs[1].side_effect, SideEffect::W2);
    assert_eq!(defs[2].id, SHOW_UNDO_TREE);
    assert_eq!(defs[2].side_effect, SideEffect::R0);
}

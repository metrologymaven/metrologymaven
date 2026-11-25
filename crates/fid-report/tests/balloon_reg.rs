//! Acceptance tests for `RPT.BALLOON.REG` (`fid_report::balloon::register`, AC-1..AC-7).
//!
//! Expected outcomes come from `fixtures/rpt/balloon/two_sheet_drawing/two_sheet_drawing.json`,
//! written by the independent Python oracle `make_fixture.py` next to it (never by this
//! code). The forged-event, multi-revision and overflow cases below are hand-labelled from
//! the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_gdt::chars::dprof::CharId;
use fid_project::Uuid;
use fid_registry::CommandDef;
use fid_replay::ReplayOutcome;
use fid_report::balloon::register::{
    command_defs, execute, list_value, replay_exact, AddBalloonParams, AddEvent, Balloon, BalloonEvent,
    BalloonRegister, MoveBalloonParams, RegisterEnv, RegisterError, RenumberMode, SheetPoint, ADD_BALLOON,
    LIST_BALLOON, MOVE_BALLOON, RELINK_BALLOON, REMOVE_BALLOON, RENUMBER_BALLOON,
};
use fid_report::balloon::sheet::{
    digest_bytes, BalloonId, DeterministicV7Gen, DrawingRevId, SheetError, SheetId, SheetInfo, UuidV7Gen,
};
use fid_store::cas::BlobRef;

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

// ------------------------------------------------------------------ stubs

fn v7(tag: u8, seq: u8) -> Uuid {
    Uuid::from_bytes([0x01, 0x93, 0x00, 0x00, 0x00, tag, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, seq])
}

fn rev_id(seq: u8) -> DrawingRevId {
    DrawingRevId::new(v7(0xd0, seq)).unwrap()
}

fn sheet_id(seq: u8) -> SheetId {
    SheetId::new(v7(0x5e, seq)).unwrap()
}

/// Stub of `RPT.BALLOON.SHEET`'s `SheetInfo` (allowed by the leaf's Design until SHEET is
/// fully built): sheet -> (revision, width, height, ready).
#[derive(Default)]
struct StubSheets(BTreeMap<SheetId, (DrawingRevId, i64, i64, bool)>);

impl SheetInfo for StubSheets {
    fn bounds(&self, s: SheetId) -> Option<(i64, i64)> {
        self.0.get(&s).and_then(|&(_, w, h, ready)| ready.then_some((w, h)))
    }
    fn revision(&self, s: SheetId) -> Option<DrawingRevId> {
        self.0.get(&s).map(|e| e.0)
    }
    fn image(&self, _: SheetId) -> Option<BlobRef> {
        None
    }
    fn ready(&self, s: SheetId) -> bool {
        self.0.get(&s).is_some_and(|e| e.3)
    }
}

// ------------------------------------------------------------------ fixture access

fn fixture() -> CValue {
    let path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/rpt/balloon/two_sheet_drawing/two_sheet_drawing.json");
    parse(&std::fs::read(path).expect("read fixture")).expect("parse fixture")
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(k).unwrap_or_else(|| panic!("missing key {k}")),
        _ => panic!("not an object looking for {k}"),
    }
}

fn opt<'a>(v: &'a CValue, k: &str) -> Option<&'a CValue> {
    match v {
        CValue::Obj(m) => m.get(k).filter(|x| **x != CValue::Null),
        _ => None,
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("not a string: {other:?}"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("not an array: {other:?}"),
    }
}

fn o<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn s(x: &str) -> CValue {
    CValue::Str(x.to_string())
}

fn pt(v: &CValue) -> CValue {
    let a = arr(v);
    o([("x", a[0].clone()), ("y", a[1].clone())])
}

/// The fixture's drawing revision: sheets S1, S2 and characteristics C1..C10.
struct World {
    rev: DrawingRevId,
    sheet_keys: BTreeMap<String, SheetId>,
    sheets: StubSheets,
    chars: BTreeSet<CharId>,
}

impl World {
    fn from_fixture(fx: &CValue) -> World {
        let rev = rev_id(1);
        let mut sheets = StubSheets::default();
        let mut sheet_keys = BTreeMap::new();
        for (i, sh) in arr(get(fx, "sheets")).iter().enumerate() {
            let id = sheet_id(u8::try_from(i + 1).unwrap());
            let w: i64 = st(get(sh, "width_cmm")).parse().unwrap();
            let h: i64 = st(get(sh, "height_cmm")).parse().unwrap();
            sheets.0.insert(id, (rev, w, h, true));
            sheet_keys.insert(st(get(sh, "key")).to_string(), id);
        }
        let chars = arr(get(fx, "characteristics")).iter().map(|c| CharId::parse(st(c)).unwrap()).collect();
        World { rev, sheet_keys, sheets, chars }
    }

    fn env(&self) -> RegisterEnv<'_> {
        RegisterEnv { sheets: &self.sheets, chars: &self.chars }
    }
}

fn defs() -> BTreeMap<String, CommandDef> {
    command_defs().unwrap().into_iter().map(|d| (format!("{}@{}", d.id.as_str(), d.major), d)).collect()
}

fn command_of(op: &str) -> &'static str {
    match op {
        "add" => ADD_BALLOON,
        "move" => MOVE_BALLOON,
        "renumber" => RENUMBER_BALLOON,
        "relink" => RELINK_BALLOON,
        "remove" => REMOVE_BALLOON,
        other => panic!("unknown op {other}"),
    }
}

/// Runs one oracle scenario through the JSON dispatcher, checking every step; returns the
/// register and the handle -> balloon ID map.
fn run_scenario(
    world: &World,
    steps: &[CValue],
    ids: &mut dyn UuidV7Gen,
) -> (BalloonRegister, BTreeMap<String, BalloonId>) {
    let defs = defs();
    let mut reg = BalloonRegister::new();
    let mut handles: BTreeMap<String, BalloonId> = BTreeMap::new();
    let mut seen_ids: BTreeSet<BalloonId> = BTreeSet::new();
    let env = world.env();
    for (i, step) in steps.iter().enumerate() {
        let op = st(get(step, "op"));
        let p = get(step, "params");
        let mut params = BTreeMap::new();
        params.insert("drawing_rev".to_string(), s(&world.rev.to_string()));
        if let Some(h) = opt(p, "balloon") {
            params.insert("balloon".to_string(), hid(&handles, h));
        }
        if let Some(c) = opt(p, "char") {
            params.insert("characteristic".to_string(), c.clone());
        }
        if let Some(k) = opt(p, "sheet") {
            params.insert("sheet".to_string(), s(&world.sheet_keys[st(k)].to_string()));
        }
        for k in ["anchor", "label"] {
            if let Some(v) = opt(p, k) {
                params.insert(k.to_string(), pt(v));
            }
        }
        for k in ["number", "mode", "zone"] {
            if let Some(v) = opt(p, k) {
                params.insert(k.to_string(), v.clone());
            }
        }
        if matches!(op, "move" | "relink" | "remove") {
            params.remove("drawing_rev");
        }
        let params = CValue::Obj(params);
        let cmd = command_of(op);
        let def = &defs[cmd];
        let journal_before = reg.journal().len();
        let got = execute(cmd, &params, &mut reg, &env, ids);
        let expect = get(step, "expect");
        match (opt(expect, "error"), got) {
            (Some(code), Err(e)) => {
                assert_eq!(e.code(), st(code), "step {i} {op}: {e}");
                assert_eq!(reg.journal().len(), journal_before, "step {i}: a failure journals nothing");
            }
            (Some(code), Ok(v)) => panic!("step {i} {op}: expected {}, got {v:?}", st(code)),
            (None, Err(e)) => panic!("step {i} {op}: unexpected error {e}"),
            (None, Ok(v)) => {
                let ok = get(expect, "ok");
                let params_schema = Compiled::compile(&def.params_schema).unwrap();
                let result_schema = Compiled::compile(&def.result_schema).unwrap();
                assert!(params_schema.evaluate(&params, &NoHooks).is_ok(), "step {i}: params schema");
                assert!(result_schema.evaluate(&v, &NoHooks).is_ok(), "step {i}: result schema {v:?}");
                assert_eq!(reg.journal().len(), journal_before + 1, "step {i}: one event");
                assert_eq!(reg.journal()[journal_before].kind(), cmd);
                match op {
                    "add" => {
                        assert_eq!(get(&v, "number"), get(ok, "number"), "step {i}");
                        let id = BalloonId::parse(st(get(&v, "id"))).expect("UUIDv7 balloon ID");
                        assert!(seen_ids.insert(id), "step {i}: balloon ID reused");
                        handles.insert(st(get(ok, "handle")).to_string(), id);
                    }
                    "renumber" => {
                        let want: Vec<CValue> = arr(get(ok, "changes"))
                            .iter()
                            .map(|c| {
                                let c = arr(c);
                                o([("balloon", hid(&handles, &c[0])), ("new", c[2].clone()), ("old", c[1].clone())])
                            })
                            .collect();
                        assert_eq!(get(&v, "changes"), &CValue::Arr(want), "step {i}");
                    }
                    "remove" => assert_eq!(v, o([])),
                    _ => assert_eq!(get(&v, "number"), get(ok, "number"), "step {i}"),
                }
            }
        }
        // Full list after every step (REQ-011: ascending number, IDs and links as the oracle).
        let list =
            execute(LIST_BALLOON, &o([("drawing_rev", s(&world.rev.to_string()))]), &mut reg, &env, ids).expect("list");
        let want: Vec<CValue> = arr(get(step, "list"))
            .iter()
            .map(|r| {
                let zone = opt(r, "zone")
                    .map_or(CValue::Null, |z| o([("text", z.clone()), ("untrusted", CValue::Bool(true))]));
                o([
                    ("anchor", pt(get(r, "anchor"))),
                    ("characteristic", get(r, "characteristic").clone()),
                    ("drawing_rev", s(&world.rev.to_string())),
                    ("id", hid(&handles, get(r, "balloon"))),
                    ("label", pt(get(r, "label"))),
                    ("number", get(r, "number").clone()),
                    ("sheet", s(&world.sheet_keys[st(get(r, "sheet"))].to_string())),
                    ("zone", zone),
                ])
            })
            .collect();
        assert_eq!(list, o([("balloons", CValue::Arr(want))]), "step {i} {op}: list");
        assert_eq!(reg.journal().len().to_string(), st(get(step, "journal_len")), "step {i}: journal length");
        let list_schema = Compiled::compile(&defs[LIST_BALLOON].result_schema).unwrap();
        assert!(list_schema.evaluate(&list, &NoHooks).is_ok());
    }
    (reg, handles)
}

/// The balloon ID of an oracle handle, as a JSON string.
fn hid(handles: &BTreeMap<String, BalloonId>, h: &CValue) -> CValue {
    s(&handles.get(st(h)).unwrap_or_else(|| panic!("unknown handle {}", st(h))).to_string())
}

fn scenario<'a>(fx: &'a CValue, name: &str) -> &'a [CValue] {
    let sc = arr(get(fx, "scenarios")).iter().find(|sc| st(get(sc, "name")) == name).expect("scenario");
    arr(get(sc, "steps"))
}

fn list_bytes(reg: &BalloonRegister, rev: DrawingRevId) -> Vec<u8> {
    to_canonical_bytes(&list_value(&reg.list(rev))).unwrap()
}

// ------------------------------------------------------------------ AC-1 .. AC-5, AC-7

/// AC-1 (ten adds, numbers 1..10, distinct UUIDv7 IDs, ten add events), AC-2 (both error
/// paths journal nothing), AC-3 (remove 7, set 9 -> 7), AC-4 (insert C7 at 2), AC-5
/// (compact), then AC-7 (exact replay IDENTICAL, list byte-identical).
#[test]
fn ac1_to_ac5_and_ac7_two_sheet_drawing() {
    let fx = fixture();
    let world = World::from_fixture(&fx);
    let mut ids = DeterministicV7Gen::new(1_764_000_000_000);
    let (reg, handles) = run_scenario(&world, scenario(&fx, "acceptance"), &mut ids);

    // AC-1: the first ten events are report.add_balloon@1.
    assert!(reg.journal()[..10].iter().all(|e| e.kind() == ADD_BALLOON));
    assert_eq!(handles.len(), 11);
    // AC-3/AC-4: every surviving original balloon keeps its ID and link.
    let listed = reg.list(world.rev);
    for b in &listed {
        let handle = handles.iter().find(|(_, id)| **id == b.id).map(|(h, _)| h.clone()).unwrap();
        let expected_char = if handle == "B11" { "C7".to_string() } else { format!("C{}", &handle[1..]) };
        assert_eq!(b.characteristic.as_str(), expected_char);
    }
    assert!(reg.is_retired(handles["B7"]));
    assert!(reg.get(handles["B7"]).is_none());

    // AC-7: the journal survives canonical bytes, replays IDENTICAL, and the list is
    // byte-identical.
    let stored: Vec<Vec<u8>> = reg.journal().iter().map(|e| to_canonical_bytes(&e.to_value()).unwrap()).collect();
    let events: Vec<BalloonEvent> =
        stored.iter().map(|b| BalloonEvent::from_value(&parse(b).unwrap()).unwrap()).collect();
    assert_eq!(events, reg.journal());
    let run = replay_exact(&events, &world.env());
    assert_eq!(run.outcome, ReplayOutcome::Identical);
    assert_eq!(run.outcome.as_str(), "IDENTICAL");
    assert_eq!(run.first_mismatch, None);
    assert_eq!(list_bytes(&run.register, world.rev), list_bytes(&reg, world.rev));
    assert_eq!(run.register, reg);
}

/// AC-6 and the bounds edges of REQ-008 (oracle scenario `off_sheet`).
#[test]
fn ac6_off_sheet_positions_fail_and_journal_nothing() {
    let fx = fixture();
    let world = World::from_fixture(&fx);
    let mut ids = DeterministicV7Gen::new(1_764_000_100_000);
    let steps = scenario(&fx, "off_sheet");
    assert_eq!(st(get(get(&steps[0], "expect"), "error")), "report.balloon_off_sheet");
    let (reg, _) = run_scenario(&world, steps, &mut ids);
    assert_eq!(run_replay(&reg, &world), ReplayOutcome::Identical);
}

/// REQ-001..REQ-010 edge cases from the oracle (relink, retired IDs, set/insert, zone,
/// parameter errors), then exact replay.
#[test]
fn oracle_extras_relink_remove_renumber_zone() {
    let fx = fixture();
    let world = World::from_fixture(&fx);
    let mut ids = DeterministicV7Gen::new(1_764_000_200_000);
    let (reg, handles) = run_scenario(&world, scenario(&fx, "extras"), &mut ids);
    assert!(reg.is_retired(handles["B2"]));
    assert_eq!(run_replay(&reg, &world), ReplayOutcome::Identical);
}

/// Round-trips the journal through canonical bytes (as stored), then replays it exactly.
fn run_replay(reg: &BalloonRegister, world: &World) -> ReplayOutcome {
    let events: Vec<BalloonEvent> = reg
        .journal()
        .iter()
        .map(|e| BalloonEvent::from_value(&parse(&to_canonical_bytes(&e.to_value()).unwrap()).unwrap()).unwrap())
        .collect();
    assert_eq!(events, reg.journal());
    let run = replay_exact(&events, &world.env());
    if run.outcome == ReplayOutcome::Identical {
        assert_eq!(list_bytes(&run.register, world.rev), list_bytes(reg, world.rev));
    }
    run.outcome
}

// ------------------------------------------------------------------ hand-labelled cases

fn small_world() -> World {
    let mut sheets = StubSheets::default();
    sheets.0.insert(sheet_id(1), (rev_id(1), 42000, 29700, true));
    sheets.0.insert(sheet_id(2), (rev_id(2), 29700, 21000, true));
    sheets.0.insert(sheet_id(3), (rev_id(2), 10000, 10000, false));
    let chars = ["C1", "C2", "C3"].iter().map(|c| CharId::parse(c).unwrap()).collect();
    World { rev: rev_id(1), sheet_keys: BTreeMap::new(), sheets, chars }
}

fn add(rev: DrawingRevId, sheet: SheetId, c: &str, number: Option<u32>, insert: bool) -> AddBalloonParams {
    AddBalloonParams {
        drawing_rev: rev,
        sheet,
        characteristic: CharId::parse(c).unwrap(),
        anchor: SheetPoint::new(100, 100),
        label: SheetPoint::new(200, 200),
        number,
        insert,
        zone: None,
    }
}

/// Numbers and links are per drawing revision; a sheet of another revision is not found;
/// a sheet without a known size is not ready.
#[test]
fn numbers_and_links_are_per_drawing_revision() {
    let w = small_world();
    let env = w.env();
    let mut ids = DeterministicV7Gen::new(1_764_000_300_000);
    let mut reg = BalloonRegister::new();
    let a = reg.add_balloon(&env, &mut ids, &add(rev_id(1), sheet_id(1), "C1", None, false)).unwrap();
    let b = reg.add_balloon(&env, &mut ids, &add(rev_id(2), sheet_id(2), "C1", None, false)).unwrap();
    assert_eq!((a.number, b.number), (1, 1));
    let e = reg.add_balloon(&env, &mut ids, &add(rev_id(1), sheet_id(2), "C2", None, false)).unwrap_err();
    assert_eq!(e.code(), "report.not_found");
    let e = reg.add_balloon(&env, &mut ids, &add(rev_id(2), sheet_id(3), "C2", None, false)).unwrap_err();
    assert_eq!(e, RegisterError::SheetNotReady);
    // Moving to a sheet of another revision is refused.
    let mv = MoveBalloonParams { balloon: a.id, anchor: None, label: None, sheet: Some(sheet_id(2)) };
    assert_eq!(reg.move_balloon(&env, &mv).unwrap_err().code(), "report.not_found");
    // Renumbering one revision with a balloon of another is refused.
    let e = reg.renumber_balloon(rev_id(2), RenumberMode::Set { balloon: a.id, number: 5 }).unwrap_err();
    assert_eq!(e.code(), "report.not_found");
    assert_eq!(reg.journal().len(), 2);
}

/// An ID generator that hands out a live or retired ID makes the add fail and journal
/// nothing (IDs are never reused, REQ-009).
#[test]
fn reused_generator_id_is_refused() {
    struct Fixed(Uuid);
    impl UuidV7Gen for Fixed {
        fn next_v7(&mut self) -> Result<Uuid, SheetError> {
            Ok(self.0)
        }
    }
    let w = small_world();
    let env = w.env();
    let mut fixed = Fixed(v7(0xba, 1));
    let mut reg = BalloonRegister::new();
    let first = reg.add_balloon(&env, &mut fixed, &add(rev_id(1), sheet_id(1), "C1", None, false)).unwrap();
    let e = reg.add_balloon(&env, &mut fixed, &add(rev_id(1), sheet_id(1), "C2", None, false)).unwrap_err();
    assert_eq!(e.code(), "report.store_invalid");
    reg.remove_balloon(first.id).unwrap();
    let e = reg.add_balloon(&env, &mut fixed, &add(rev_id(1), sheet_id(1), "C2", None, false)).unwrap_err();
    assert_eq!(e.code(), "report.store_invalid");
    assert_eq!(reg.journal().len(), 2);
}

/// Numbers near `u32::MAX`: the next default number and an insert that would move
/// `u32::MAX` fail with `report.bad_params` and change nothing.
#[test]
fn number_overflow_is_refused() {
    let w = small_world();
    let env = w.env();
    let mut ids = DeterministicV7Gen::new(1_764_000_400_000);
    let mut reg = BalloonRegister::new();
    let top = reg.add_balloon(&env, &mut ids, &add(rev_id(1), sheet_id(1), "C1", Some(u32::MAX), false)).unwrap();
    let before = reg.clone();
    let e = reg.add_balloon(&env, &mut ids, &add(rev_id(1), sheet_id(1), "C2", None, false)).unwrap_err();
    assert_eq!(e.code(), "report.bad_params");
    let e = reg.add_balloon(&env, &mut ids, &add(rev_id(1), sheet_id(1), "C2", Some(1), true)).unwrap_err();
    assert_eq!(e.code(), "report.bad_params");
    let second = reg.add_balloon(&env, &mut ids, &add(rev_id(1), sheet_id(1), "C2", Some(1), false)).unwrap();
    let e = reg.renumber_balloon(rev_id(1), RenumberMode::Insert { balloon: second.id, number: 1 }).unwrap_err();
    assert_eq!(e.code(), "report.bad_params");
    assert_eq!(reg.get(top.id).unwrap().number, u32::MAX);
    assert_eq!(reg.journal().len(), before.journal().len() + 1);
}

/// Forged or altered journals do not replay: a reused (live or retired) ID, a changed
/// result digest, a changed assigned number, and events with extra keys.
#[test]
fn forged_events_do_not_replay() {
    let fx = fixture();
    let world = World::from_fixture(&fx);
    let mut ids = DeterministicV7Gen::new(1_764_000_500_000);
    let (reg, handles) = run_scenario(&world, scenario(&fx, "acceptance"), &mut ids);
    let journal = reg.journal().to_vec();
    let env = world.env();

    let first_add = |events: &[BalloonEvent]| -> AddEvent {
        match &events[0] {
            BalloonEvent::Add(a) => (**a).clone(),
            other => panic!("not an add: {other:?}"),
        }
    };
    // Event 10 removes B7 (C7); a well-formed add of C7 right after it, with a correct
    // number and digest, replays unless its ID is live or retired.
    let forged_add = |id: BalloonId| -> BalloonEvent {
        let mut a = first_add(&journal);
        a.id = id;
        a.characteristic = CharId::parse("C7").unwrap();
        a.number = 11;
        let b = Balloon {
            id,
            number: 11,
            drawing_rev: a.drawing_rev,
            sheet: a.sheet,
            characteristic: a.characteristic.clone(),
            anchor: a.anchor,
            label: a.label,
            zone: None,
        };
        a.result_digest = digest_bytes(&to_canonical_bytes(&b.to_value()).unwrap());
        BalloonEvent::Add(Box::new(a))
    };
    assert!(matches!(journal[10], BalloonEvent::Remove { .. }));
    let with = |ev: BalloonEvent| -> Vec<BalloonEvent> {
        let mut v = journal[..11].to_vec();
        v.push(ev);
        v
    };
    let fresh = BalloonId::new(v7(0xfe, 1)).unwrap();
    assert_eq!(replay_exact(&with(forged_add(fresh)), &env).outcome, ReplayOutcome::Identical);
    let live = replay_exact(&with(forged_add(handles["B1"])), &env);
    assert_eq!((live.outcome, live.first_mismatch), (ReplayOutcome::Divergent, Some(11)));
    let retired = replay_exact(&with(forged_add(handles["B7"])), &env);
    assert_eq!((retired.outcome, retired.first_mismatch), (ReplayOutcome::Divergent, Some(11)));

    // A changed result digest.
    let mut forged = journal.clone();
    if let BalloonEvent::Renumber { result_digest, .. } = &mut forged[13] {
        *result_digest = digest_bytes(b"forged");
    } else {
        panic!("event 13 is the AC-5 compact");
    }
    assert_eq!(replay_exact(&forged, &env).first_mismatch, Some(13));

    // A changed assigned number on an add.
    let mut forged = journal.clone();
    let mut a = first_add(&journal);
    a.number = 2;
    forged[0] = BalloonEvent::Add(Box::new(a));
    assert_eq!(replay_exact(&forged, &env).first_mismatch, Some(0));

    // Extra keys and unknown kinds are store_invalid.
    let mut v = journal[0].to_value();
    if let CValue::Obj(m) = &mut v {
        m.insert("principal".to_string(), s("agent"));
    }
    assert_eq!(BalloonEvent::from_value(&v).unwrap_err().code(), "report.store_invalid");
    let bogus = o([("kind", s("report.add_balloons@1"))]);
    assert_eq!(BalloonEvent::from_value(&bogus).unwrap_err().code(), "report.store_invalid");
}

/// The six registry definitions validate, are W2 except list (R0), and their parameter
/// schemas compile for the bus.
#[test]
fn command_defs_validate_and_compile() {
    let defs = command_defs().unwrap();
    let names: Vec<&str> = defs.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(
        names,
        [
            "report.add_balloon",
            "report.move_balloon",
            "report.renumber_balloon",
            "report.relink_balloon",
            "report.remove_balloon",
            "report.list_balloon"
        ]
    );
    for d in &defs {
        Compiled::compile(&d.params_schema).unwrap();
        Compiled::compile(&d.result_schema).unwrap();
    }
    let p = o([("drawing_rev", s(&rev_id(1).to_string())), ("mode", s("compact")), ("bogus", s("1"))]);
    let w = small_world();
    let mut reg = BalloonRegister::new();
    let mut ids = DeterministicV7Gen::new(1);
    let e = execute(RENUMBER_BALLOON, &p, &mut reg, &w.env(), &mut ids).unwrap_err();
    assert_eq!(e.code(), "report.bad_params");
}

/// Performance budget (Design: add, move or renumber in a register of 2000 balloons under
/// 10 ms on reference hardware, [unverified]); CI only checks a loose 2 s bound per command
/// in debug builds.
#[test]
fn two_thousand_balloons_stay_fast() {
    let mut w = small_world();
    w.chars = (1..=2001).map(|i| CharId::parse(&format!("P{i}")).unwrap()).collect();
    let env = w.env();
    let mut ids = DeterministicV7Gen::new(1_764_000_600_000);
    let mut reg = BalloonRegister::new();
    for i in 1..=2000 {
        reg.add_balloon(&env, &mut ids, &add(rev_id(1), sheet_id(1), &format!("P{i}"), None, false)).unwrap();
    }
    let t = Instant::now();
    let b = reg.add_balloon(&env, &mut ids, &add(rev_id(1), sheet_id(1), "P2001", Some(1), true)).unwrap();
    assert!(t.elapsed().as_millis() < 2000);
    let t = Instant::now();
    let mv = MoveBalloonParams { balloon: b.id, anchor: Some(SheetPoint::new(5, 5)), label: None, sheet: None };
    reg.move_balloon(&env, &mv).unwrap();
    assert!(t.elapsed().as_millis() < 2000);
    let t = Instant::now();
    let changes = reg.renumber_balloon(rev_id(1), RenumberMode::Insert { balloon: b.id, number: 1000 }).unwrap();
    assert_eq!(changes.len(), 1003);
    assert!(t.elapsed().as_millis() < 2000);
    let t = Instant::now();
    let changes = reg.renumber_balloon(rev_id(1), RenumberMode::Compact).unwrap();
    assert_eq!(changes.len(), 2001);
    assert!(t.elapsed().as_millis() < 2000);
    let numbers: Vec<u32> = reg.list(rev_id(1)).iter().map(|b| b.number).collect();
    assert_eq!(numbers, (1..=2001).collect::<Vec<u32>>());
}

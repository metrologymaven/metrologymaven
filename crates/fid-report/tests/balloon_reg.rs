//! Acceptance and performance tests for `RPT.BALLOON.REG` (`fid_report::balloon::register`, AC-1..AC-7).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::time::Instant;

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{to_canonical_bytes, CValue};
use fid_gdt::chars::dprof::CharId;
use fid_project::Uuid;
use fid_replay::ReplayOutcome;
use fid_report::balloon::register::{
    command_defs, execute, replay_exact, AddBalloonParams, BalloonEvent, BalloonRegister, MoveBalloonParams,
    RelinkBalloonParams, RenumberBalloonParams, RenumberMode, SheetPoint, ADD_BALLOON, LIST_BALLOON, MOVE_BALLOON,
    RELINK_BALLOON, REMOVE_BALLOON, RENUMBER_BALLOON,
};
use fid_report::balloon::sheet::{DeterministicV7Gen, DrawingRevId, SheetId, SheetInfo};
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

/// Synthetic `two_sheet_drawing` fixture builder: one drawing revision, two A3 landscape sheets
/// (`42000 x 29700 cmm` each) implementing `SheetInfo`.
struct TwoSheetDrawing {
    drawing_rev: DrawingRevId,
    sheets: [SheetId; 2],
    bounds: BTreeMap<SheetId, (i64, i64)>,
}

impl TwoSheetDrawing {
    fn new() -> Self {
        let rev = DrawingRevId::new(Uuid::from_bytes([
            0x01, 0x90, 0x00, 0x00, 0x00, 0x10, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, 1,
        ]))
        .expect("v7");
        let s1 = SheetId::new(Uuid::from_bytes([
            0x01, 0x90, 0x00, 0x00, 0x00, 0x10, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, 2,
        ]))
        .expect("v7");
        let s2 = SheetId::new(Uuid::from_bytes([
            0x01, 0x90, 0x00, 0x00, 0x00, 0x10, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, 3,
        ]))
        .expect("v7");
        let mut bounds = BTreeMap::new();
        bounds.insert(s1, (42_000, 29_700));
        bounds.insert(s2, (42_000, 29_700));
        TwoSheetDrawing { drawing_rev: rev, sheets: [s1, s2], bounds }
    }
}

impl SheetInfo for TwoSheetDrawing {
    fn bounds(&self, s: SheetId) -> Option<(i64, i64)> {
        self.bounds.get(&s).copied()
    }
    fn revision(&self, s: SheetId) -> Option<DrawingRevId> {
        self.bounds.contains_key(&s).then_some(self.drawing_rev)
    }
    fn image(&self, _s: SheetId) -> Option<BlobRef> {
        None
    }
    fn ready(&self, s: SheetId) -> bool {
        self.bounds.contains_key(&s)
    }
}

fn char_id(n: u32) -> CharId {
    CharId::parse(&format!("C{n}")).expect("valid char id")
}

// ------------------------------------------------------------------ AC-1 .. AC-7

#[test]
fn ac1_to_ac7_full_register_lifecycle_renumber_bounds_and_exact_replay() {
    let fixture = TwoSheetDrawing::new();
    let mut reg = BalloonRegister::new();
    let mut ids = DeterministicV7Gen::new(1_760_000_500_000);

    // AC-1: Add 10 balloons for C1..C10 across the two A3 sheets -> numbered 1..=10, 10 journal events.
    let mut initial_ids = Vec::new();
    for i in 1..=10u32 {
        let sheet = if i <= 5 { fixture.sheets[0] } else { fixture.sheets[1] };
        let b = reg
            .add_balloon(
                &fixture,
                &mut ids,
                &AddBalloonParams {
                    drawing_rev: fixture.drawing_rev,
                    sheet,
                    characteristic: char_id(i),
                    anchor: SheetPoint::new(i64::from(i) * 2000, 5000),
                    label: SheetPoint::new(i64::from(i) * 2000 + 500, 4000),
                    number: None,
                    insert: false,
                    zone: Some(format!("B{i}")),
                },
            )
            .expect("add balloon");
        assert_eq!(b.number, i);
        initial_ids.push(b.id);
    }
    assert_eq!(reg.journal().len(), 10);

    // AC-2: Duplicate char C3 -> report.char_already_ballooned (journal unchanged);
    // explicit taken number 4 -> report.balloon_number_taken (journal unchanged).
    let err_dup = reg
        .add_balloon(
            &fixture,
            &mut ids,
            &AddBalloonParams {
                drawing_rev: fixture.drawing_rev,
                sheet: fixture.sheets[0],
                characteristic: char_id(3),
                anchor: SheetPoint::new(1000, 1000),
                label: SheetPoint::new(1500, 1000),
                number: None,
                insert: false,
                zone: None,
            },
        )
        .expect_err("duplicate char must fail");
    assert_eq!(err_dup.code(), "report.char_already_ballooned");
    assert_eq!(reg.journal().len(), 10);

    let err_num = reg
        .add_balloon(
            &fixture,
            &mut ids,
            &AddBalloonParams {
                drawing_rev: fixture.drawing_rev,
                sheet: fixture.sheets[0],
                characteristic: char_id(99),
                anchor: SheetPoint::new(1000, 1000),
                label: SheetPoint::new(1500, 1000),
                number: Some(4),
                insert: false,
                zone: None,
            },
        )
        .expect_err("taken number 4 must fail");
    assert_eq!(err_num.code(), "report.balloon_number_taken");
    assert_eq!(reg.journal().len(), 10);

    // AC-3: Remove balloon 7 (permanently retires ID) and renumber balloon 9 with mode Set -> 7.
    let id7 = initial_ids[6];
    let id9 = initial_ids[8];
    reg.remove_balloon(id7).expect("remove balloon 7");
    assert!(reg.is_retired(id7));

    let ch = reg
        .renumber_balloon(&RenumberBalloonParams {
            drawing_rev: fixture.drawing_rev,
            mode: RenumberMode::Set { balloon: id9, number: 7 },
        })
        .expect("set balloon 9 -> 7");
    assert_eq!(ch.len(), 1);
    assert_eq!((ch[0].balloon, ch[0].old, ch[0].new), (id9, 9, 7));

    let listed = reg.list(fixture.drawing_rev);
    let nums: Vec<u32> = listed.iter().map(|b| b.number).collect();
    assert_eq!(nums, vec![1, 2, 3, 4, 5, 6, 7, 8, 10]);
    let b7 = listed.iter().find(|b| b.number == 7).expect("number 7");
    assert_eq!(b7.id, id9);
    assert_eq!(b7.characteristic, char_id(9));

    // AC-4: Add new balloon for C7 with insert mode at number 2 -> old 2..=10 shift up by 1; IDs/links unchanged.
    let new_c7 = reg
        .add_balloon(
            &fixture,
            &mut ids,
            &AddBalloonParams {
                drawing_rev: fixture.drawing_rev,
                sheet: fixture.sheets[0],
                characteristic: char_id(7),
                anchor: SheetPoint::new(3000, 8000),
                label: SheetPoint::new(3500, 7500),
                number: Some(2),
                insert: true,
                zone: Some("C2".into()),
            },
        )
        .expect("insert C7 at 2");
    assert_eq!(new_c7.number, 2);
    assert_ne!(new_c7.id, id7);
    let listed4 = reg.list(fixture.drawing_rev);
    let nums4: Vec<u32> = listed4.iter().map(|b| b.number).collect();
    assert_eq!(nums4, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 11]);
    // Old balloon 9 (which was at 7) shifted to 8 with unchanged ID & characteristic C9!
    assert_eq!(reg.get(id9).expect("id9").number, 8);
    assert_eq!(reg.get(id9).expect("id9").characteristic, char_id(9));

    // AC-5: Compact mode -> 1..=10 contiguous preserving relative order.
    let comp = reg
        .renumber_balloon(&RenumberBalloonParams { drawing_rev: fixture.drawing_rev, mode: RenumberMode::Compact })
        .expect("compact");
    assert_eq!(comp.len(), 1); // only 11 -> 10 shifted
    let listed5 = reg.list(fixture.drawing_rev);
    let nums5: Vec<u32> = listed5.iter().map(|b| b.number).collect();
    assert_eq!(nums5, (1..=10).collect::<Vec<u32>>());

    // AC-6: Position (42001, 100) outside 42000-wide sheet -> report.balloon_off_sheet.
    let err_off = reg
        .add_balloon(
            &fixture,
            &mut ids,
            &AddBalloonParams {
                drawing_rev: fixture.drawing_rev,
                sheet: fixture.sheets[0],
                characteristic: char_id(50),
                anchor: SheetPoint::new(42_001, 100),
                label: SheetPoint::new(41_000, 100),
                number: None,
                insert: false,
                zone: None,
            },
        )
        .expect_err("off-sheet must fail");
    assert_eq!(err_off.code(), "report.balloon_off_sheet");

    // Zone > 16 bytes rejected with report.text_invalid (REQ-010).
    let err_zone = reg
        .add_balloon(
            &fixture,
            &mut ids,
            &AddBalloonParams {
                drawing_rev: fixture.drawing_rev,
                sheet: fixture.sheets[0],
                characteristic: char_id(51),
                anchor: SheetPoint::new(1000, 1000),
                label: SheetPoint::new(1500, 1000),
                number: None,
                insert: false,
                zone: Some("ZONE-TOO-LONG-17X".into()),
            },
        )
        .expect_err("zone > 16 bytes must fail");
    assert_eq!(err_zone.code(), "report.text_invalid");

    // Also exercise move_balloon, relink_balloon, and RenumberMode::Insert on existing balloon.
    reg.move_balloon(
        &fixture,
        &MoveBalloonParams {
            balloon: new_c7.id,
            anchor: Some(SheetPoint::new(4000, 9000)),
            label: None,
            sheet: Some(fixture.sheets[1]),
        },
    )
    .expect("move ok");
    reg.relink_balloon(&RelinkBalloonParams { balloon: new_c7.id, characteristic: char_id(70) }).expect("relink ok");
    reg.renumber_balloon(&RenumberBalloonParams {
        drawing_rev: fixture.drawing_rev,
        mode: RenumberMode::Insert { balloon: id9, number: 1 },
    })
    .expect("insert existing balloon at 1");

    // AC-7: Canonical event round-trip & exact replay -> IDENTICAL & byte-identical list output.
    let round_tripped: Vec<BalloonEvent> =
        reg.journal().iter().map(|ev| BalloonEvent::from_value(&ev.to_value()).expect("event round-trip")).collect();
    assert_eq!(round_tripped, reg.journal());

    let replayed = replay_exact(&round_tripped, &fixture);
    assert_eq!(replayed.outcome, ReplayOutcome::Identical);
    assert_eq!(replayed.first_mismatch, None);
    let list_orig = CValue::Arr(reg.list(fixture.drawing_rev).iter().map(|b| b.to_value()).collect());
    let list_rep = CValue::Arr(replayed.register.list(fixture.drawing_rev).iter().map(|b| b.to_value()).collect());
    assert_eq!(to_canonical_bytes(&list_orig).expect("c1"), to_canonical_bytes(&list_rep).expect("c2"));
}

// ------------------------------------------------------------------ Schema validation & 2000-balloon perf

#[test]
fn command_defs_validate_and_2000_balloons_stay_fast() {
    let defs = command_defs().expect("command_defs valid");
    assert_eq!(defs.len(), 6);
    let schemas: BTreeMap<String, (Compiled, Compiled)> = defs
        .iter()
        .map(|d| {
            (
                format!("{}@{}", d.id, d.major),
                (
                    Compiled::compile(&d.params_schema).expect("params schema"),
                    Compiled::compile(&d.result_schema).expect("result schema"),
                ),
            )
        })
        .collect();

    let fixture = TwoSheetDrawing::new();
    let mut reg = BalloonRegister::new();
    let mut ids = DeterministicV7Gen::new(1_760_000_600_000);

    // Run every command via JSON dispatcher and validate against its registered schema.
    let add_json = format!(
        r#"{{"drawing_rev":"{}","sheet":"{}","characteristic":"C1","anchor":{{"x":"1000","y":"2000"}},"label":{{"x":"1500","y":"2000"}},"zone":"A1"}}"#,
        fixture.drawing_rev, fixture.sheets[0]
    );
    let add_p = fid_canon::parse(add_json.as_bytes()).expect("json");
    assert!(schemas[ADD_BALLOON].0.evaluate(&add_p, &NoHooks).is_ok());
    let add_r = execute(ADD_BALLOON, &add_p, &mut reg, &fixture, &mut ids).expect("add ok");
    assert!(schemas[ADD_BALLOON].1.evaluate(&add_r, &NoHooks).is_ok());

    let bid = match &add_r {
        CValue::Obj(m) => match m.get("id") {
            Some(CValue::Str(s)) => s.clone(),
            _ => panic!("id"),
        },
        _ => panic!("obj"),
    };

    let mv_json = format!(r#"{{"balloon":"{bid}","label":{{"x":"2000","y":"2500"}}}}"#);
    let mv_p = fid_canon::parse(mv_json.as_bytes()).expect("json");
    assert!(schemas[MOVE_BALLOON].0.evaluate(&mv_p, &NoHooks).is_ok());
    let mv_r = execute(MOVE_BALLOON, &mv_p, &mut reg, &fixture, &mut ids).expect("move ok");
    assert!(schemas[MOVE_BALLOON].1.evaluate(&mv_r, &NoHooks).is_ok());

    let rel_json = format!(r#"{{"balloon":"{bid}","characteristic":"C2"}}"#);
    let rel_p = fid_canon::parse(rel_json.as_bytes()).expect("json");
    assert!(schemas[RELINK_BALLOON].0.evaluate(&rel_p, &NoHooks).is_ok());
    let rel_r = execute(RELINK_BALLOON, &rel_p, &mut reg, &fixture, &mut ids).expect("relink ok");
    assert!(schemas[RELINK_BALLOON].1.evaluate(&rel_r, &NoHooks).is_ok());

    let ren_json = format!(r#"{{"drawing_rev":"{}","mode":{{"mode":"compact"}}}}"#, fixture.drawing_rev);
    let ren_p = fid_canon::parse(ren_json.as_bytes()).expect("json");
    assert!(schemas[RENUMBER_BALLOON].0.evaluate(&ren_p, &NoHooks).is_ok());
    let ren_r = execute(RENUMBER_BALLOON, &ren_p, &mut reg, &fixture, &mut ids).expect("renumber ok");
    assert!(schemas[RENUMBER_BALLOON].1.evaluate(&ren_r, &NoHooks).is_ok());

    let list_json = format!(r#"{{"drawing_rev":"{}"}}"#, fixture.drawing_rev);
    let list_p = fid_canon::parse(list_json.as_bytes()).expect("json");
    assert!(schemas[LIST_BALLOON].0.evaluate(&list_p, &NoHooks).is_ok());
    let list_r = execute(LIST_BALLOON, &list_p, &mut reg, &fixture, &mut ids).expect("list ok");
    assert!(schemas[LIST_BALLOON].1.evaluate(&list_r, &NoHooks).is_ok());

    let rem_json = format!(r#"{{"balloon":"{bid}"}}"#);
    let rem_p = fid_canon::parse(rem_json.as_bytes()).expect("json");
    assert!(schemas[REMOVE_BALLOON].0.evaluate(&rem_p, &NoHooks).is_ok());
    let rem_r = execute(REMOVE_BALLOON, &rem_p, &mut reg, &fixture, &mut ids).expect("remove ok");
    assert!(schemas[REMOVE_BALLOON].1.evaluate(&rem_r, &NoHooks).is_ok());

    // Performance budget: 2000 balloons -> add/move/renumber under budget.
    let mut big = BalloonRegister::new();
    for i in 1..=2000u32 {
        big.add_balloon(
            &fixture,
            &mut ids,
            &AddBalloonParams {
                drawing_rev: fixture.drawing_rev,
                sheet: fixture.sheets[0],
                characteristic: char_id(i),
                anchor: SheetPoint::new(1000, 1000),
                label: SheetPoint::new(1500, 1000),
                number: None,
                insert: false,
                zone: None,
            },
        )
        .expect("add 2000");
    }
    let last_id = big.list(fixture.drawing_rev).last().expect("last").id;
    let t0 = Instant::now();
    big.renumber_balloon(&RenumberBalloonParams {
        drawing_rev: fixture.drawing_rev,
        mode: RenumberMode::Insert { balloon: last_id, number: 1 },
    })
    .expect("renumber 2000");
    let dt = t0.elapsed();
    assert!(dt.as_millis() < 200, "2000-balloon renumber took {} ms", dt.as_millis());
}

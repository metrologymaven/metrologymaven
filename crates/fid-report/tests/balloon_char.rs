//! Acceptance tests for `RPT.BALLOON.CHAR` (`fid_report::balloon::char`, AC-1..AC-5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use fid_gdt::chars::dprof::DprofState;
use fid_gdt::chars::spec::{PlanId, SpecState};
use fid_io_reg::Principal;
use fid_project::Uuid;
use fid_replay::ReplayOutcome;
use fid_report::balloon::char::{
    command_def, make_char_input_from_literals, replay_exact, AddCharBalloonParams, CharBalloonStore, Dialect,
    DrawingCharKind, ADD_CHAR_BALLOON,
};
use fid_report::balloon::register::{BalloonRegister, SheetPoint};
use fid_report::balloon::sheet::{DeterministicV7Gen, DrawingRevId, SheetId, SheetInfo};
use fid_store::cas::BlobRef;

struct TwoSheetFixture {
    rev: DrawingRevId,
    s1: SheetId,
    bounds: BTreeMap<SheetId, (i64, i64)>,
}

impl TwoSheetFixture {
    fn new() -> Self {
        let rev = DrawingRevId::new(Uuid::from_bytes([
            0x01, 0x90, 0x00, 0x00, 0x00, 0x20, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, 1,
        ]))
        .expect("v7");
        let s1 = SheetId::new(Uuid::from_bytes([
            0x01, 0x90, 0x00, 0x00, 0x00, 0x20, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, 2,
        ]))
        .expect("v7");
        let mut bounds = BTreeMap::new();
        bounds.insert(s1, (42_000, 29_700));
        TwoSheetFixture { rev, s1, bounds }
    }
}

impl SheetInfo for TwoSheetFixture {
    fn bounds(&self, s: SheetId) -> Option<(i64, i64)> {
        self.bounds.get(&s).copied()
    }
    fn revision(&self, s: SheetId) -> Option<DrawingRevId> {
        self.bounds.contains_key(&s).then_some(self.rev)
    }
    fn image(&self, _s: SheetId) -> Option<BlobRef> {
        None
    }
    fn ready(&self, s: SheetId) -> bool {
        self.bounds.contains_key(&s)
    }
}

// ------------------------------------------------------------------ AC-1 .. AC-5

#[test]
fn ac1_to_ac5_atomic_add_char_balloon_authored_literals_rollback_and_replay() {
    let fixture = TwoSheetFixture::new();
    let plan = PlanId::parse("PLAN-01").expect("plan id");
    let mut spec = SpecState::new();
    let mut dprof = DprofState::new();
    let mut register = BalloonRegister::new();
    let mut store = CharBalloonStore::new();
    let mut ids = DeterministicV7Gen::new(1_760_000_700_000);

    // AC-1 & AC-2: diameter 12.000mm +0.018mm / 0mm ASME -> 1 char + balloon 1 + 1 journal event,
    // preserving exact verbatim authored literals ("12.000 mm", "+0.018 mm", "0 mm").
    let input = make_char_input_from_literals(
        "diameter",
        "12.000mm",
        "+0.018mm",
        "0mm",
        Some("ASME"),
        &["A", "B"],
        Some("Bore Dia"),
    )
    .expect("valid input");

    let res = store
        .add_char_balloon(
            &mut spec,
            &mut dprof,
            &mut register,
            &fixture,
            &mut ids,
            Principal::Human,
            &AddCharBalloonParams {
                plan: plan.clone(),
                drawing_rev: fixture.rev,
                sheet: fixture.s1,
                characteristic: input,
                anchor: SheetPoint::new(1000, 2000),
                label: SheetPoint::new(1500, 2000),
                number: None,
                zone: Some("B2".into()),
                dry_run: false,
            },
        )
        .expect("AC-1 add_char_balloon ok");

    assert!(res.committed);
    assert_eq!(res.balloon.number, 1);
    assert_eq!(res.balloon.characteristic, res.characteristic.id);
    assert_eq!(res.characteristic.kind, DrawingCharKind::Diameter);
    assert_eq!(res.characteristic.dialect, Dialect::Asme);
    assert_eq!(res.characteristic.nominal.authored_literal(), "12.000 mm");
    assert_eq!(res.characteristic.upper.authored_literal(), "+0.018 mm");
    assert_eq!(res.characteristic.lower.authored_literal(), "0 mm");
    assert_eq!(store.journal().len(), 1);
    assert_eq!(store.journal()[0].kind(), ADD_CHAR_BALLOON);

    // AC-3: Lower limit exceeds upper limit -> GDT validation error returned unchanged; zero mutation/journaling.
    let bad_limits =
        make_char_input_from_literals("diameter", "12.000mm", "+0.010mm", "+0.020mm", Some("ASME"), &[], None)
            .expect("parses literals");
    let err_lim = store
        .add_char_balloon(
            &mut spec,
            &mut dprof,
            &mut register,
            &fixture,
            &mut ids,
            Principal::Human,
            &AddCharBalloonParams {
                plan: plan.clone(),
                drawing_rev: fixture.rev,
                sheet: fixture.s1,
                characteristic: bad_limits,
                anchor: SheetPoint::new(2000, 2000),
                label: SheetPoint::new(2500, 2000),
                number: None,
                zone: None,
                dry_run: false,
            },
        )
        .expect_err("lower > upper must fail");
    assert_eq!(err_lim.code(), "invalid_limits");
    assert_eq!(store.journal().len(), 1);
    assert_eq!(register.list(fixture.rev).len(), 1);

    // AC-4 (part 1): Missing dialect -> report.dialect_missing, zero mutation.
    let no_dialect = make_char_input_from_literals("linear", "25.00mm", "+0.05mm", "-0.05mm", None, &[], None)
        .expect("parses without dialect");
    let err_dia = store
        .add_char_balloon(
            &mut spec,
            &mut dprof,
            &mut register,
            &fixture,
            &mut ids,
            Principal::Human,
            &AddCharBalloonParams {
                plan: plan.clone(),
                drawing_rev: fixture.rev,
                sheet: fixture.s1,
                characteristic: no_dialect,
                anchor: SheetPoint::new(3000, 2000),
                label: SheetPoint::new(3500, 2000),
                number: None,
                zone: None,
                dry_run: false,
            },
        )
        .expect_err("missing dialect must fail");
    assert_eq!(err_dia.code(), "report.dialect_missing");
    assert_eq!(store.journal().len(), 1);

    // AC-4 (part 2): Balloon number 1 already taken -> report.balloon_number_taken AND NO characteristic created!
    let valid_again = make_char_input_from_literals("linear", "45.00mm", "+0.02mm", "-0.01mm", Some("ISO"), &[], None)
        .expect("valid input");
    let err_taken = store
        .add_char_balloon(
            &mut spec,
            &mut dprof,
            &mut register,
            &fixture,
            &mut ids,
            Principal::Human,
            &AddCharBalloonParams {
                plan,
                drawing_rev: fixture.rev,
                sheet: fixture.s1,
                characteristic: valid_again,
                anchor: SheetPoint::new(3000, 2000),
                label: SheetPoint::new(3500, 2000),
                number: Some(1),
                zone: None,
                dry_run: false,
            },
        )
        .expect_err("number 1 taken must fail and roll back GDT characteristic");
    assert_eq!(err_taken.code(), "report.balloon_number_taken");
    assert_eq!(store.journal().len(), 1);
    assert_eq!(store.all().count(), 1);
    assert_eq!(register.list(fixture.rev).len(), 1);

    // Unsupported kind -> report.char_type_unsupported (REQ-005).
    let err_kind = make_char_input_from_literals("attribute_pass_fail", "1mm", "0mm", "0mm", Some("ASME"), &[], None)
        .expect_err("unsupported kind must fail");
    assert_eq!(err_kind.code(), "report.char_type_unsupported");

    // Registry CommandDef validates cleanly.
    let def = command_def().expect("command_def valid");
    assert_eq!(def.id.as_str(), "report.add_char_balloon");

    // AC-5: Exact replay produces IDENTICAL with unchanged characteristic and balloon IDs.
    let replayed = replay_exact(store.journal(), &fixture);
    assert_eq!(replayed.outcome, ReplayOutcome::Identical);
    assert_eq!(replayed.first_mismatch, None);
    let rep_char = replayed.store.get(&res.characteristic.id).expect("replayed char");
    assert_eq!(rep_char, &res.characteristic);
    let rep_balloon = replayed.register.get(res.balloon.id).expect("replayed balloon");
    assert_eq!(rep_balloon, &res.balloon);
}

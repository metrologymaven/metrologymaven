//! Acceptance tests of `RPT.DESIGN.PLACE.FRAME` (`fid_report::layout::frame`, AC-1..AC-5).
//!
//! Expected outcomes come from `fixtures/rpt/design/frame/cases.json`, written by the
//! independent Python oracle `make_cases.py` next to it (never by this code). The reports are
//! the shared fixtures `fixtures/rpt/design/one_page_report` and `three_page_report`, built
//! here with the `RPT.MODEL.DOC` commands; each scenario's blocks (text stub blocks) are added
//! with `report.add_block@1`. The forged-event, bad-parameter and settings cases below are
//! hand-labelled from the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, CValue};
use fid_project::{Bits, IdError, ObservationSource, Uuid};
use fid_registry::CommandDef;
use fid_replay::ReplayOutcome;
use fid_report::doc::stub::{ImageStub, TextStub};
use fid_report::doc::{
    execute as doc_execute, BlockId, KindRegistry, Report, ReportBackend, ReportId, ADD_BLOCK, ADD_PAGE, CREATE_REPORT,
    REMOVE_BLOCK,
};
use fid_report::layout::frame::{
    command_defs, execute, is_frame_event, payloads_digest, preview_move_block_layout, replay_exact, report_settings,
    snap_edge, BlockFrame, FrameEvent, GridPitch, MoveBlockLayout, PitchSetting, ReportFrames, DEFAULT_PITCH,
    GRID_PITCH_KEY, MOVE_BLOCK_LAYOUT,
};
use fid_report::layout::page::{execute as page_execute, LayoutError, MemoryProject, SET_PAGE_LAYOUT};
use fid_store::settings::{validate_value, Resolved, Scope, SettingWarning, SettingsRegistry, Source, WarningReason};

const BASE_MS: u64 = 1_763_510_400_000;

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

/// Deterministic ID source: draw k gives ms BASE_MS + k and random bits k + 1.
struct Counter {
    next: u64,
}

impl ObservationSource for Counter {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        Ok(BASE_MS + self.next)
    }
    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        let b = Bits::new(n, u128::from(self.next) + 1)?;
        self.next += 1;
        Ok(b)
    }
}

// ------------------------------------------------------------------ JSON helpers

fn s(x: &str) -> CValue {
    CValue::Str(x.to_string())
}

fn o<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(k).unwrap_or_else(|| panic!("missing key {k}")),
        _ => panic!("not an object looking for {k}"),
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

fn int(v: &CValue) -> i64 {
    st(v).parse().unwrap()
}

fn fixture(rel: &str) -> CValue {
    let path = format!("{}/../../fixtures/rpt/design/{rel}", env!("CARGO_MANIFEST_DIR"));
    parse(&std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))).expect("parse fixture")
}

fn cases() -> CValue {
    fixture("frame/cases.json")
}

fn defs() -> BTreeMap<String, CommandDef> {
    command_defs().unwrap().into_iter().map(|d| (format!("{}@{}", d.id, d.major), d)).collect()
}

fn valid(schema: &CValue, v: &CValue) -> bool {
    Compiled::compile(schema).unwrap().evaluate(v, &NoHooks).is_ok()
}

fn with_key(v: &CValue, k: &str, x: CValue) -> CValue {
    let CValue::Obj(m) = v else { panic!("not an object") };
    let mut m = m.clone();
    m.insert(k.to_string(), x);
    CValue::Obj(m)
}

fn pitch(units: i64) -> PitchSetting {
    GridPitch::new(units)
}

// ------------------------------------------------------------------ world

fn project_id() -> Uuid {
    Uuid::parse("019a9a9a-0000-7000-8000-00000000c0de").unwrap()
}

/// A UUIDv7 that no report, page, block or project of these tests uses.
fn stranger(seq: u8) -> Uuid {
    Uuid::from_bytes([0x01, 0x93, 0, 0, 0, 0xee, 0x70, 0, 0x80, 0, 0, 0, 0, 0, 0, seq])
}

fn kinds() -> KindRegistry {
    let mut k = KindRegistry::new();
    k.register(Box::new(TextStub)).unwrap();
    k.register(Box::new(ImageStub)).unwrap();
    k
}

struct World {
    project: MemoryProject,
    kinds: KindRegistry,
    ids: Counter,
    report: Uuid,
    /// Page handles `P1`, `P2`, ... in page order.
    pages: BTreeMap<String, Uuid>,
    /// Block handles `B1`, `B2`, ...
    blocks: BTreeMap<String, Uuid>,
}

impl World {
    /// Builds the report of fixture `name` with the `RPT.MODEL.DOC` commands (as the
    /// `RPT.DESIGN.PAGE` tests do), then adds `blocks` (`{handle, page, text}`) as text blocks.
    fn build(name: &str, blocks: &[CValue]) -> World {
        let fx = fixture(&format!("{name}/report.json"));
        assert_eq!(st(get(&fx, "fixture")), name);
        let n: usize = st(get(&fx, "pages")).parse().unwrap();
        let mut w = World {
            project: MemoryProject::new(project_id()),
            kinds: kinds(),
            ids: Counter { next: 0 },
            report: Uuid::NIL,
            pages: BTreeMap::new(),
            blocks: BTreeMap::new(),
        };
        let summary =
            w.doc(CREATE_REPORT, o([("title", get(&fx, "title").clone()), ("project", s(&project_id().to_string()))]));
        w.report = Uuid::parse(st(get(&summary, "report_id"))).unwrap();
        let first = Uuid::parse(st(get(&arr(get(&summary, "pages"))[0], "page_id"))).unwrap();
        w.pages.insert("P1".to_string(), first);
        let mut last = first;
        for i in 2..=n {
            let r = w.doc(ADD_PAGE, o([("report", s(&w.report.to_string())), ("after", s(&last.to_string()))]));
            last = Uuid::parse(st(get(&r, "page_id"))).unwrap();
            w.pages.insert(format!("P{i}"), last);
        }
        for b in blocks {
            let page = w.pages[st(get(b, "page"))];
            let r = w.doc(
                ADD_BLOCK,
                o([
                    ("report", s(&w.report.to_string())),
                    ("page", s(&page.to_string())),
                    ("kind", s("text")),
                    ("payload", o([("text", get(b, "text").clone())])),
                ]),
            );
            w.blocks.insert(st(get(b, "handle")).to_string(), Uuid::parse(st(get(&r, "block_id"))).unwrap());
        }
        assert_eq!(w.project.journal().len(), n + blocks.len());
        w
    }

    fn doc(&mut self, cmd: &str, params: CValue) -> CValue {
        doc_execute(cmd, &params, &mut self.project, &self.kinds, &mut self.ids).unwrap()
    }

    fn mv(&mut self, params: &CValue, pitch: &PitchSetting) -> Result<CValue, LayoutError> {
        execute(MOVE_BLOCK_LAYOUT, params, &mut self.project, &self.kinds, pitch)
    }

    fn report_id(&self) -> ReportId {
        ReportId::new(self.report).unwrap()
    }

    fn stored_report(&self) -> Report {
        let rid = self.report_id();
        self.project.load(&rid).unwrap().unwrap().open(&rid, &self.kinds).unwrap()
    }

    fn report_digest(&self) -> String {
        self.project.load(&self.report_id()).unwrap().unwrap().digest.as_str().to_string()
    }

    fn payloads(&self) -> String {
        payloads_digest(&self.stored_report()).unwrap().as_str().to_string()
    }

    fn block_id(&self, h: &str) -> BlockId {
        BlockId::new(self.blocks[h]).unwrap()
    }

    /// Maps oracle handles to IDs: `R` the report, `R?` an unknown report, `P<n>` a page,
    /// `X` an unknown page, `B<n>` a block, `B?` an unknown block.
    fn handle(&self, h: &str) -> CValue {
        let id = match h {
            "R" => self.report,
            "R?" => stranger(1),
            "X" => stranger(2),
            "B?" => stranger(3),
            h if h.starts_with('P') => self.pages[h],
            h => *self.blocks.get(h).unwrap_or_else(|| panic!("unknown handle {h}")),
        };
        s(&id.to_string())
    }

    /// An oracle value with handles in `report`, `block` and `page` mapped to IDs.
    fn mapped(&self, v: &CValue) -> CValue {
        let CValue::Obj(m) = v else { panic!("not an object") };
        let mut out = m.clone();
        for k in ["report", "block", "page"] {
            if let Some(CValue::Str(h)) = m.get(k) {
                out.insert(k.to_string(), self.handle(h));
            }
        }
        CValue::Obj(out)
    }

    /// `{handle: {page, frame}}` from the stored report and frames, in the oracle's form.
    fn state(&self) -> CValue {
        let report = self.stored_report();
        let frames = self.project.frames(&self.report_id()).cloned().unwrap_or_default();
        let page_handle = |id: Uuid| self.pages.iter().find(|(_, p)| **p == id).map(|(h, _)| h.clone()).unwrap();
        let mut out = BTreeMap::new();
        for (h, id) in &self.blocks {
            let page = report.pages().iter().find(|p| p.blocks().iter().any(|b| b.id().uuid() == *id)).unwrap();
            let frame = frames.get(&BlockId::new(*id).unwrap()).map_or(CValue::Null, |f| {
                o([
                    ("x", s(&f.x.to_string())),
                    ("y", s(&f.y.to_string())),
                    ("w", s(&f.w.to_string())),
                    ("h", s(&f.h.to_string())),
                ])
            });
            out.insert(h.clone(), o([("page", s(&page_handle(page.id().uuid()))), ("frame", frame)]));
        }
        CValue::Obj(out)
    }
}

/// Runs one oracle scenario and checks every step; returns the world after it.
fn run_scenario(name: &str) -> World {
    let all = cases();
    let sc = arr(get(&all, "scenarios")).iter().find(|sc| st(get(sc, "name")) == name).expect("scenario");
    let mut w = World::build(st(get(sc, "fixture")), arr(get(sc, "blocks")));
    let defs = defs();
    let def = &defs[MOVE_BLOCK_LAYOUT];
    let payloads = w.payloads();
    for (i, step) in arr(get(sc, "steps")).iter().enumerate() {
        let params = w.mapped(get(step, "params"));
        let expect = get(step, "expect");
        let before = w.project.journal().len();
        let doc_before = w.report_digest();
        let p = pitch(int(get(step, "pitch")));
        assert!(valid(&def.params_schema, &params), "{name} step {i}: params schema");
        let got = w.mv(&params, &p);
        match (got, expect) {
            (Ok(v), CValue::Obj(e)) if e.contains_key("result") => {
                assert_eq!(v, w.mapped(get(expect, "result")), "{name} step {i}: result");
                assert!(valid(&def.result_schema, &v), "{name} step {i}: result schema");
            }
            (Err(e), CValue::Obj(x)) if x.contains_key("error") => {
                assert_eq!(e.code(), st(get(expect, "error")), "{name} step {i}: {e}");
                assert_eq!(e.field(), x.get("field").map(st), "{name} step {i}: field");
                if let (LayoutError::LayoutInvalid { reason, .. }, Some(r)) = (&e, x.get("reason")) {
                    assert_eq!(*reason, st(r), "{name} step {i}: reason");
                }
            }
            (got, _) => panic!("{name} step {i}: got {got:?}, expected {expect:?}"),
        }
        let journaled = matches!(get(step, "journaled"), CValue::Bool(true));
        assert_eq!(w.project.journal().len(), before + usize::from(journaled), "{name} step {i}: journal length");
        if journaled {
            assert!(is_frame_event(&w.project.journal()[before]), "{name} step {i}: event type");
        } else {
            assert_eq!(w.report_digest(), doc_before, "{name} step {i}: a failed command changed the report");
        }
        // REQ-005: no payload, binding, decision or formatter choice changes.
        assert_eq!(w.payloads(), payloads, "{name} step {i}: block payloads changed");
        assert_eq!(w.state(), *get(step, "state"), "{name} step {i}: state");
    }
    w
}

fn ac_world() -> World {
    run_scenario("ac1_ac2_ac3")
}

fn stored_frame(w: &World, h: &str) -> BlockFrame {
    w.project.frames(&w.report_id()).unwrap().get(&w.block_id(h)).unwrap()
}

// ------------------------------------------------------------------ acceptance criteria

/// AC-1: `--x 10.04mm --y 20mm --w 80mm --h 40mm --snap` with pitch 1 mm stores
/// (1000, 2000, 8000, 4000).
#[test]
fn ac1_absolute_frame_snapped_to_1mm() {
    let mut w = World::build("one_page_report", &[o([("handle", s("B1")), ("page", s("P1")), ("text", s("B"))])]);
    let p = o([
        ("report", w.handle("R")),
        ("block", w.handle("B1")),
        ("x", s("10.04mm")),
        ("y", s("20mm")),
        ("w", s("80mm")),
        ("h", s("40mm")),
        ("snap", CValue::Bool(true)),
    ]);
    let r = w.mv(&p, &pitch(DEFAULT_PITCH)).unwrap();
    assert_eq!((int(get(&r, "x")), int(get(&r, "y")), int(get(&r, "w")), int(get(&r, "h"))), (1000, 2000, 8000, 4000));
    assert_eq!(stored_frame(&w, "B1"), BlockFrame { x: 1000, y: 2000, w: 8000, h: 4000 });
    assert_eq!(GridPitch::default().units(), 100);
}

/// AC-2: `--dx 0.35mm` with snapping off moves x to 1035.
#[test]
fn ac2_delta_without_snapping() {
    let w = ac_world();
    // The state after the scenario's AC-2 step (AC-3 refused, so it is still AC-2's).
    assert_eq!(stored_frame(&w, "B1"), BlockFrame { x: 1035, y: 2000, w: 8000, h: 4000 });
}

/// AC-3: `--w 0.5mm` fails with `report.layout_invalid` and journals nothing; a grid pitch
/// of 0.05 mm is refused too.
#[test]
fn ac3_too_small_and_pitch_below_minimum() {
    let mut w = ac_world();
    let len = w.project.journal().len();
    let e =
        w.mv(&o([("report", w.handle("R")), ("block", w.handle("B1")), ("w", s("0.5mm"))]), &pitch(100)).unwrap_err();
    assert_eq!((e.code(), e.field()), ("report.layout_invalid", Some("w")));
    assert_eq!(w.project.journal().len(), len);

    // Typed pitch text (the value a designer or CLI turns into the setting).
    let e = GridPitch::parse("0.05mm").unwrap_err();
    assert_eq!(e, LayoutError::LayoutInvalid { field: "grid_pitch", reason: "below_minimum" });
    assert_eq!(e.code(), "report.layout_invalid");
    // The setting value 5 (0.05 mm) itself.
    assert_eq!(
        GridPitch::new(5).unwrap_err(),
        LayoutError::LayoutInvalid { field: "grid_pitch", reason: "below_minimum" }
    );
    // CORE.STO.SET refuses to store it: the declared schema's minimum is 10.
    let registry = SettingsRegistry::build(&report_settings()).unwrap();
    let decl = registry.get(GRID_PITCH_KEY).unwrap();
    assert!(validate_value(decl.schema(), &parse(b"5").unwrap()).is_err());
    assert!(validate_value(decl.schema(), &parse(b"10").unwrap()).is_ok());
    assert!(validate_value(decl.schema(), &GridPitch::new(10).unwrap().setting_value()).is_ok());
    // A stored 5 that reaches the snapshot anyway (hand-edited file) is refused, and a move
    // that snaps with it fails and journals nothing.
    let five = Resolved { key: GRID_PITCH_KEY.to_string(), value: parse(b"5").unwrap(), source: Source::User };
    let setting = GridPitch::from_snapshot(&[five], &[]);
    assert_eq!(setting, Err(LayoutError::LayoutInvalid { field: "grid_pitch", reason: "below_minimum" }));
    let p = o([("report", w.handle("R")), ("block", w.handle("B1")), ("dx", s("1mm")), ("snap", CValue::Bool(true))]);
    let e = w.mv(&p, &setting).unwrap_err();
    assert_eq!((e.code(), e.field()), ("report.layout_invalid", Some("grid_pitch")));
    assert_eq!(w.project.journal().len(), len);
    // Without snapping the pitch is not read.
    let p = with_key(&p, "snap", CValue::Bool(false));
    assert!(w.mv(&p, &setting).is_ok());
}

/// AC-4: the payload digest is the same before AC-1 and after AC-1 and AC-2 (and so is the
/// report document: no page moved).
#[test]
fn ac4_payload_digest_unchanged() {
    let all = cases();
    let sc = arr(get(&all, "scenarios")).iter().find(|sc| st(get(sc, "name")) == "ac1_ac2_ac3").unwrap();
    let w0 = World::build("one_page_report", arr(get(sc, "blocks")));
    let (payloads, doc) = (w0.payloads(), w0.report_digest());
    let w = ac_world();
    assert_eq!(w.payloads(), payloads);
    assert_eq!(w.report_digest(), doc);
}

/// AC-5: the journal after AC-2 replays in exact mode to `IDENTICAL`, rebuilding the frames.
#[test]
fn ac5_exact_replay_is_identical() {
    for name in ["ac1_ac2_ac3", "snap_edges", "outside_and_bounds", "no_frame", "lengths", "not_found", "move_page"] {
        let w = run_scenario(name);
        let run = replay_exact(w.project.journal(), &w.kinds, project_id());
        assert_eq!((run.outcome, run.first_mismatch), (ReplayOutcome::Identical, None), "{name}");
        let rid = w.report_id();
        assert_eq!(run.frames.get(&rid), w.project.frames(&rid), "{name}: frames");
        let stored = w.project.load(&rid).unwrap().unwrap();
        assert_eq!(run.reports[&rid].1, stored.digest, "{name}: report");
    }
}

// ------------------------------------------------------------------ more rules

#[test]
fn oracle_scenarios() {
    let all = cases();
    for sc in arr(get(&all, "scenarios")) {
        run_scenario(st(get(sc, "name")));
    }
}

/// REQ-002: every snapping case of the oracle (ties away from zero).
#[test]
fn snapping_matches_the_oracle() {
    let all = cases();
    for c in arr(get(&all, "snaps")) {
        let (v, p) = (int(get(c, "value")), int(get(c, "pitch")));
        assert_eq!(snap_edge(v, p), Some(int(get(c, "snapped"))), "{v} on {p}");
    }
}

/// REQ-002: pitch texts and setting values of the oracle.
#[test]
fn pitches_match_the_oracle() {
    let all = cases();
    assert_eq!(int(get(&all, "default_pitch")), DEFAULT_PITCH);
    for c in arr(get(&all, "pitch_texts")) {
        let text = st(get(c, "text"));
        match (GridPitch::parse(text), c) {
            (Ok(g), CValue::Obj(m)) if m.contains_key("units") => assert_eq!(g.units(), int(get(c, "units")), "{text}"),
            (Err(e), CValue::Obj(m)) if m.contains_key("error") => assert_pitch_error(&e, st(get(c, "error"))),
            (got, _) => panic!("{text:?}: got {got:?}, expected {c:?}"),
        }
    }
    for c in arr(get(&all, "pitch_units")) {
        let units = int(get(c, "units"));
        match (GridPitch::new(units), c) {
            (Ok(g), CValue::Obj(m)) if m.contains_key("pitch") => assert_eq!(g.units(), int(get(c, "pitch"))),
            (Err(e), CValue::Obj(m)) if m.contains_key("error") => assert_pitch_error(&e, st(get(c, "error"))),
            (got, _) => panic!("{units}: got {got:?}, expected {c:?}"),
        }
    }
}

fn assert_pitch_error(e: &LayoutError, want: &str) {
    match e {
        LayoutError::LayoutInvalid { field, reason } => assert_eq!((*field, *reason), ("grid_pitch", want)),
        other => panic!("not layout_invalid: {other:?}"),
    }
}

/// The setting declaration: user scope only, not admin, default 100 (1 mm); the resolved
/// snapshot is read fail-closed.
#[test]
fn grid_pitch_setting() {
    let registry = SettingsRegistry::build(&report_settings()).unwrap();
    let decl = registry.get(GRID_PITCH_KEY).unwrap();
    assert_eq!(decl.scopes().collect::<Vec<_>>(), [Scope::User]);
    assert!(!decl.admin());
    assert_eq!(decl.owner(), "report");
    assert_eq!(GridPitch::from_snapshot(&[resolved(decl.default_value().clone(), Source::Default)], &[]), pitch(100));
    assert_eq!(GridPitch::from_snapshot(&[resolved(parse(b"25").unwrap(), Source::User)], &[]), pitch(25));
    let unusable = Err(LayoutError::LayoutInvalid { field: "grid_pitch", reason: "unusable_setting" });
    // A warning about the key: the resolver fell back to the default; we do not.
    let warn = SettingWarning {
        key: Some(GRID_PITCH_KEY.to_string()),
        scope: Scope::User,
        reason: WarningReason::InvalidValue,
    };
    assert_eq!(GridPitch::from_snapshot(&[resolved(parse(b"100").unwrap(), Source::Default)], &[warn]), unusable);
    let file = SettingWarning { key: None, scope: Scope::User, reason: WarningReason::Unreadable };
    assert_eq!(GridPitch::from_snapshot(&[resolved(parse(b"100").unwrap(), Source::Default)], &[file]), unusable);
    // A warning about another key is not ours.
    let other = SettingWarning {
        key: Some("stats.catalog_path".to_string()),
        scope: Scope::User,
        reason: WarningReason::UnknownKey,
    };
    assert_eq!(GridPitch::from_snapshot(&[resolved(parse(b"100").unwrap(), Source::Default)], &[other]), pitch(100));
    // Missing, twice, or not an integer.
    assert_eq!(GridPitch::from_snapshot(&[], &[]), unusable);
    let twice = [resolved(parse(b"100").unwrap(), Source::User), resolved(parse(b"100").unwrap(), Source::Default)];
    assert_eq!(GridPitch::from_snapshot(&twice, &[]), unusable);
    assert_eq!(GridPitch::from_snapshot(&[resolved(s("1mm"), Source::User)], &[]), unusable);
    assert_eq!(GridPitch::from_snapshot(&[resolved(parse(b"10.5").unwrap(), Source::User)], &[]), unusable);
    assert_eq!(
        GridPitch::from_snapshot(&[resolved(parse(b"200001").unwrap(), Source::User)], &[]),
        Err(LayoutError::LayoutInvalid { field: "grid_pitch", reason: "too_large" })
    );
}

fn resolved(value: CValue, source: Source) -> Resolved {
    Resolved { key: GRID_PITCH_KEY.to_string(), value, source }
}

/// REQ-005: a page move changes page membership (the report digest) but never a payload;
/// the frame is kept relative to the new page's body origin.
#[test]
fn page_move_changes_membership_only() {
    let all = cases();
    let sc = arr(get(&all, "scenarios")).iter().find(|sc| st(get(sc, "name")) == "move_page").unwrap();
    let w0 = World::build("three_page_report", arr(get(sc, "blocks")));
    let w = run_scenario("move_page");
    assert_eq!(w.payloads(), w0.payloads());
    assert_ne!(w.report_digest(), w0.report_digest());
    let report = w.stored_report();
    let order: Vec<Vec<Uuid>> =
        report.pages().iter().map(|p| p.blocks().iter().map(|b| b.id().uuid()).collect()).collect();
    assert_eq!(order, vec![vec![w.blocks["B2"]], vec![w.blocks["B3"]], vec![w.blocks["B1"]]]);
}

/// Page setup and frames share one journal, and replay reads all three kinds of events.
#[test]
fn page_setup_and_frames_replay_together() {
    let mut w = run_scenario("move_page");
    let r = w.handle("R");
    page_execute(SET_PAGE_LAYOUT, &o([("report", r.clone()), ("size", s("A3"))]), &mut w.project, &w.kinds).unwrap();
    let p = o([("report", r.clone()), ("block", w.handle("B3")), ("dw", s("1mm"))]);
    w.mv(&p, &pitch(100)).unwrap();
    // A removed block's frame stays in the store but the block can no longer be moved.
    w.doc(REMOVE_BLOCK, o([("report", r.clone()), ("block", w.handle("B3"))]));
    let e = w.mv(&p, &pitch(100)).unwrap_err();
    assert_eq!(e.code(), "report.not_found");
    let run = replay_exact(w.project.journal(), &w.kinds, project_id());
    assert_eq!(run.outcome, ReplayOutcome::Identical);
    let rid = w.report_id();
    assert_eq!(run.layouts.get(&rid), w.project.layout(&rid));
    assert_eq!(run.frames.get(&rid), w.project.frames(&rid));
}

/// `--dry-run`: the same checks and result, nothing committed.
#[test]
fn preview_commits_nothing() {
    let mut w = ac_world();
    let len = w.project.journal().len();
    let params =
        o([("report", w.handle("R")), ("block", w.handle("B1")), ("dx", s("1mm")), ("snap", CValue::Bool(true))]);
    let p = MoveBlockLayout::from_value(&params).unwrap();
    let preview = preview_move_block_layout(&w.project, &w.kinds, &pitch(100), &p).unwrap();
    assert_eq!(preview.frame, BlockFrame { x: 1100, y: 2000, w: 8000, h: 4000 });
    assert_eq!(w.project.journal().len(), len);
    assert_eq!(stored_frame(&w, "B1").x, 1035);
    let shrink = o([("report", w.handle("R")), ("block", w.handle("B1")), ("dw", s("-80mm"))]);
    let e =
        preview_move_block_layout(&w.project, &w.kinds, &pitch(100), &MoveBlockLayout::from_value(&shrink).unwrap())
            .unwrap_err();
    assert_eq!((e.code(), e.field()), ("report.layout_invalid", Some("w")));
    assert_eq!(w.project.journal().len(), len);
    let done = w.mv(&params, &pitch(100)).unwrap();
    assert_eq!(done, preview.to_value());
}

fn ac_journal() -> (World, usize) {
    let w = ac_world();
    let idx = w.project.journal().iter().position(is_frame_event).unwrap();
    (w, idx)
}

/// Forged and broken events (hand-labelled): replay must not accept them.
#[test]
fn forged_frame_events_do_not_replay() {
    let (w, idx) = ac_journal();
    let journal = w.project.journal().to_vec();
    let replay = |j: &[CValue]| replay_exact(j, &w.kinds, project_id());
    let zero = s(&format!("sha256:{}", "0".repeat(64)));

    // Tampered digests.
    for k in ["frames_digest", "report_digest"] {
        let mut j = journal.clone();
        j[idx] = with_key(&j[idx], k, zero.clone());
        let run = replay(&j);
        assert_eq!((run.outcome, run.first_mismatch), (ReplayOutcome::Divergent, Some(idx)), "{k}");
    }
    // A frame changed without its digest.
    let mut j = journal.clone();
    j[idx] = with_key(&j[idx], "frame", o([("h", s("4000")), ("w", s("8000")), ("x", s("999")), ("y", s("2000"))]));
    assert_eq!(replay(&j).outcome, ReplayOutcome::Divergent);
    // A block or page the report never had, or the wrong source page.
    for (k, v) in [("block", stranger(4)), ("page", stranger(5)), ("from_page", stranger(6))] {
        let mut j = journal.clone();
        j[idx] = with_key(&j[idx], k, s(&v.to_string()));
        assert_eq!(replay(&j).outcome, ReplayOutcome::Divergent, "{k}");
    }
    // A frame event before its report exists.
    let j = vec![journal[idx].clone()];
    assert_eq!((replay(&j).outcome, replay(&j).first_mismatch), (ReplayOutcome::Divergent, Some(0)));
    // An unknown key or a frame that does not read back: not replayable.
    let mut j = journal.clone();
    j[idx] = with_key(&j[idx], "extra", s("1"));
    assert_eq!(replay(&j).outcome, ReplayOutcome::NotReplayable);
    for bad in ["-0", "01", "+5", "1.5", "-5"] {
        let mut j = journal.clone();
        let frame = with_key(get(&journal[idx], "frame"), "w", s(bad));
        j[idx] = with_key(&j[idx], "frame", frame);
        assert_eq!(replay(&j).outcome, ReplayOutcome::NotReplayable, "w = {bad}");
    }
    // A frame the handler would refuse (0.5 mm wide), with self-consistent digests.
    let ev = FrameEvent::from_value(&journal[idx]).unwrap();
    let narrow = BlockFrame { w: 50, ..ev.frame };
    let mut frames = ReportFrames::default();
    frames.frames.insert(ev.block, narrow);
    let forged = FrameEvent { frame: narrow, frames_digest: frames.digest().unwrap(), ..ev.clone() };
    let mut j = journal[..idx].to_vec();
    j.push(forged.to_value());
    assert_eq!(replay(&j).outcome, ReplayOutcome::Divergent);
}

/// Page moves (hand-labelled): the recorded report digest must match the replayed move.
#[test]
fn forged_page_moves_do_not_replay() {
    let w = run_scenario("move_page");
    let journal = w.project.journal().to_vec();
    let replay = |j: &[CValue]| replay_exact(j, &w.kinds, project_id());
    let idx = journal.iter().position(is_frame_event).unwrap();
    let ev = FrameEvent::from_value(&journal[idx]).unwrap();
    assert_ne!(ev.page, ev.from_page, "the first move of the scenario changes page");
    // The move hidden (page == from_page) while keeping the moved report's digest.
    let hidden = FrameEvent { page: ev.from_page, ..ev.clone() };
    let mut j = journal.clone();
    j[idx] = hidden.to_value();
    assert_eq!((replay(&j).outcome, replay(&j).first_mismatch), (ReplayOutcome::Divergent, Some(idx)));
    // A different target page with the original digest.
    let p2 = fid_report::doc::PageId::new(w.pages["P2"]).unwrap();
    let elsewhere = FrameEvent { page: p2, ..ev.clone() };
    let mut j = journal.clone();
    j[idx] = elsewhere.to_value();
    assert_eq!(replay(&j).outcome, ReplayOutcome::Divergent);
    // The unchanged journal replays.
    assert_eq!(replay(&journal).outcome, ReplayOutcome::Identical);
}

#[test]
fn event_round_trip() {
    let (w, idx) = ac_journal();
    let v = &w.project.journal()[idx];
    let ev = FrameEvent::from_value(v).unwrap();
    assert_eq!(&ev.to_value(), v);
    assert_eq!(st(get(v, "type")), MOVE_BLOCK_LAYOUT);
    assert_eq!(ev.block, w.block_id("B1"));
    assert_eq!(ev.page, ev.from_page);
    assert_eq!(ev.frame, BlockFrame { x: 1000, y: 2000, w: 8000, h: 4000 });
}

/// Parameters of the wrong shape are `report.bad_params` and journal nothing.
#[test]
fn bad_params() {
    let mut w = World::build("one_page_report", &[o([("handle", s("B1")), ("page", s("P1")), ("text", s("B"))])]);
    let (r, b) = (w.handle("R"), w.handle("B1"));
    let cases = [
        o([("report", r.clone()), ("block", b.clone())]),
        o([("report", r.clone()), ("block", b.clone()), ("x", s("1mm")), ("dx", s("1mm"))]),
        o([("report", r.clone()), ("block", b.clone()), ("w", s("1mm")), ("dh", s("1mm"))]),
        o([("report", r.clone()), ("block", b.clone()), ("x", s("1mm")), ("bogus", s("1"))]),
        o([("report", r.clone()), ("block", b.clone()), ("x", s("1mm")), ("snap", s("true"))]),
        o([("report", r.clone()), ("block", b.clone()), ("x", CValue::Bool(true))]),
        o([("report", r.clone()), ("x", s("1mm"))]),
        o([("block", b.clone()), ("x", s("1mm"))]),
        o([("report", r.clone()), ("block", s(&w.blocks["B1"].to_string().to_uppercase())), ("x", s("1mm"))]),
        o([("report", r.clone()), ("block", b.clone()), ("x", s("1mm")), ("page", s("P1"))]),
        CValue::Arr(Vec::new()),
    ];
    for (i, p) in cases.iter().enumerate() {
        let e = w.mv(p, &pitch(100)).unwrap_err();
        assert_eq!(e.code(), "report.bad_params", "case {i}: {e}");
    }
    let ok = o([
        ("report", r.clone()),
        ("block", b.clone()),
        ("x", s("1mm")),
        ("y", s("1mm")),
        ("w", s("1mm")),
        ("h", s("1mm")),
    ]);
    let e = execute("report.move_block_layout@2", &ok, &mut w.project, &w.kinds, &pitch(100)).unwrap_err();
    assert_eq!(e.code(), "report.bad_params");
    // JSON null is the same as leaving a coordinate out.
    let with_null = with_key(&ok, "dx", CValue::Null);
    assert!(w.mv(&with_null, &pitch(100)).is_ok());
    assert_eq!(w.project.journal().len(), 3);
}

/// The registry definition builds, compiles and carries the leaf's classes.
#[test]
fn command_definition() {
    let defs = defs();
    assert_eq!(defs.keys().map(String::as_str).collect::<Vec<_>>(), [MOVE_BLOCK_LAYOUT]);
    let d = &defs[MOVE_BLOCK_LAYOUT];
    Compiled::compile(&d.params_schema).unwrap();
    Compiled::compile(&d.result_schema).unwrap();
    assert_eq!(d.determinism.as_str(), "D0");
    assert_eq!(d.side_effect.as_str(), "W2");
    let p = o([
        ("report", s(&stranger(1).to_string())),
        ("block", s(&stranger(2).to_string())),
        ("x", s("-10.04mm")),
        ("y", s("20mm")),
        ("w", s("80mm")),
        ("h", s("0.5in")),
        ("snap", CValue::Bool(true)),
    ]);
    assert!(valid(&d.params_schema, &p));
    assert!(!valid(&d.params_schema, &with_key(&p, "w", s("-80mm"))));
    assert!(!valid(&d.params_schema, &with_key(&p, "x", s("10 mm"))));
    assert!(!valid(&d.params_schema, &with_key(&p, "snap", s("yes"))));
}

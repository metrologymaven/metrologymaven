//! Acceptance tests of `RPT.DESIGN.PAGE` (`fid_report::layout::page`, AC-1..AC-6).
//!
//! Expected outcomes come from `fixtures/rpt/design/page/cases.json`, written by the
//! independent Python oracle `make_cases.py` next to it (never by this code); the reports of
//! `fixtures/rpt/design/one_page_report` and `three_page_report` are built here with the
//! `RPT.MODEL.DOC` commands. The forged-event and bad-parameter cases below are hand-labelled
//! from the spec's rules.
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
    execute as doc_execute, KindRegistry, ReportBackend, ReportId, ADD_PAGE, CREATE_REPORT, REMOVE_PAGE,
};
use fid_report::layout::page::{
    command_defs, execute, is_layout_event, parse_length, replay_exact, LayoutError, LayoutEvent, Margins,
    MemoryProject, Orientation, PageLayout, PageSize, ReportLayout, DEFAULT_LAYOUT, SET_PAGE_LAYOUT, SHOW_PAGE_LAYOUT,
};

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

fn fixture(rel: &str) -> CValue {
    let path = format!("{}/../../fixtures/rpt/design/{rel}", env!("CARGO_MANIFEST_DIR"));
    parse(&std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))).expect("parse fixture")
}

fn cases() -> CValue {
    fixture("page/cases.json")
}

fn defs() -> BTreeMap<String, CommandDef> {
    command_defs().unwrap().into_iter().map(|d| (format!("{}@{}", d.id, d.major), d)).collect()
}

fn valid(schema: &CValue, v: &CValue) -> bool {
    Compiled::compile(schema).unwrap().evaluate(v, &NoHooks).is_ok()
}

// ------------------------------------------------------------------ world

fn project_id() -> Uuid {
    Uuid::parse("019a9a9a-0000-7000-8000-00000000c0de").unwrap()
}

/// A UUIDv7 that no report, page or project of these tests uses.
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
}

impl World {
    /// Builds the report of fixture `name` (`one_page_report`, `three_page_report`) with the
    /// `RPT.MODEL.DOC` commands: `create_report` (one page), then `add_page` after the last.
    fn build(name: &str) -> World {
        let fx = fixture(&format!("{name}/report.json"));
        assert_eq!(st(get(&fx, "fixture")), name);
        let n: usize = st(get(&fx, "pages")).parse().unwrap();
        let mut w = World {
            project: MemoryProject::new(project_id()),
            kinds: kinds(),
            ids: Counter { next: 0 },
            report: Uuid::NIL,
            pages: BTreeMap::new(),
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
        assert_eq!(w.project.journal().len(), n);
        w
    }

    fn doc(&mut self, cmd: &str, params: CValue) -> CValue {
        doc_execute(cmd, &params, &mut self.project, &self.kinds, &mut self.ids).unwrap()
    }

    fn layout(&mut self, cmd: &str, params: &CValue) -> Result<CValue, LayoutError> {
        execute(cmd, params, &mut self.project, &self.kinds)
    }

    fn report_id(&self) -> ReportId {
        ReportId::new(self.report).unwrap()
    }

    fn report_digest(&self) -> String {
        self.project.load(&self.report_id()).unwrap().unwrap().digest.as_str().to_string()
    }

    /// Maps oracle handles to IDs: `R` the report, `R?` an unknown report, `P<n>` a page,
    /// `X` an unknown page.
    fn handle(&self, h: &str) -> CValue {
        match h {
            "R" => s(&self.report.to_string()),
            "R?" => s(&stranger(1).to_string()),
            "X" => s(&stranger(2).to_string()),
            p => s(&self.pages.get(p).unwrap_or_else(|| panic!("unknown handle {p}")).to_string()),
        }
    }

    /// An oracle value with handles in `report`, `page` and `page_id` mapped to IDs.
    fn mapped(&self, v: &CValue) -> CValue {
        let CValue::Obj(m) = v else { panic!("not an object") };
        let mut out = m.clone();
        for k in ["report", "page", "page_id"] {
            if let Some(CValue::Str(h)) = m.get(k) {
                out.insert(k.to_string(), self.handle(h));
            }
        }
        CValue::Obj(out)
    }

    fn show(&mut self) -> CValue {
        self.layout(SHOW_PAGE_LAYOUT, &o([("report", s(&self.report.to_string()))])).unwrap()
    }
}

/// Runs one oracle scenario and checks every step; returns the world after it.
fn run_scenario(name: &str) -> World {
    let all = cases();
    let sc = arr(get(&all, "scenarios")).iter().find(|sc| st(get(sc, "name")) == name).expect("scenario");
    let mut w = World::build(st(get(sc, "fixture")));
    let defs = defs();
    let (set_def, show_def) = (&defs[SET_PAGE_LAYOUT], &defs[SHOW_PAGE_LAYOUT]);
    let doc_digest = w.report_digest();
    for (i, step) in arr(get(sc, "steps")).iter().enumerate() {
        let params = w.mapped(get(step, "params"));
        let expect = get(step, "expect");
        let before = w.project.journal().len();
        let got = w.layout(SET_PAGE_LAYOUT, &params);
        match (got, expect) {
            (Ok(v), CValue::Obj(e)) if e.contains_key("result") => {
                assert_eq!(v, w.mapped(get(expect, "result")), "{name} step {i}: result");
                assert!(valid(&set_def.params_schema, &params), "{name} step {i}: params schema");
                assert!(valid(&set_def.result_schema, &v), "{name} step {i}: result schema");
            }
            (Err(e), CValue::Obj(x)) if x.contains_key("error") => {
                assert_eq!(e.code(), st(get(expect, "error")), "{name} step {i}: {e}");
                let field = x.get("field").map(st);
                assert_eq!(e.field(), field, "{name} step {i}: field");
                if let (LayoutError::LayoutInvalid { reason, .. }, Some(r)) = (&e, x.get("reason")) {
                    assert_eq!(*reason, st(r), "{name} step {i}: reason");
                }
            }
            (got, _) => panic!("{name} step {i}: got {got:?}, expected {expect:?}"),
        }
        let journaled = matches!(get(step, "journaled"), CValue::Bool(true));
        assert_eq!(w.project.journal().len(), before + usize::from(journaled), "{name} step {i}: journal length");
        if journaled {
            assert!(is_layout_event(&w.project.journal()[before]), "{name} step {i}: event type");
        }
        // REQ-RPT.DESIGN-007: the report document (blocks, bindings, values) is untouched.
        assert_eq!(w.report_digest(), doc_digest, "{name} step {i}: report document changed");
        let show = w.show();
        assert!(valid(&show_def.result_schema, &show), "{name} step {i}: show schema");
        assert_eq!(get(&show, "report"), &s(&w.report.to_string()));
        let want: Vec<CValue> = arr(get(step, "show")).iter().map(|p| w.mapped(p)).collect();
        assert_eq!(arr(get(&show, "pages")), want.as_slice(), "{name} step {i}: show");
    }
    w
}

// ------------------------------------------------------------------ acceptance criteria

fn effective_page<'a>(show: &'a CValue, idx: usize) -> &'a CValue {
    &arr(get(show, "pages"))[idx]
}

/// AC-1: A4 landscape, 15 mm margins, 12 mm header, 10 mm footer on `one_page_report`.
#[test]
fn ac1_a4_landscape_body() {
    let mut w = run_scenario("ac1_a4_landscape");
    let show = w.show();
    let p = effective_page(&show, 0);
    assert_eq!((st(get(p, "page_width")), st(get(p, "page_height"))), ("29700", "21000"));
    let body = get(p, "body");
    assert_eq!(
        (st(get(body, "width")), st(get(body, "height")), st(get(body, "x")), st(get(body, "y"))),
        ("26700", "15800", "1500", "2700")
    );
}

/// AC-2: `--size custom --width 8.5in --height 11in` stores exactly 21590 x 27940.
#[test]
fn ac2_custom_size_in_inches_is_exact() {
    for name in ["ac2_custom_inches", "ac1_then_ac2"] {
        let w = run_scenario(name);
        let setup = w.project.layout(&w.report_id()).unwrap();
        assert_eq!(setup.default_layout.size, PageSize::Custom { width: 21_590, height: 27_940 }, "{name}");
    }
}

/// AC-3: an inexact margin fails with `report.layout_invalid` naming `margin_top`; the
/// journal length is unchanged.
#[test]
fn ac3_inexact_margin_names_the_field_and_journals_nothing() {
    let w = run_scenario("ac3_inexact_margin");
    assert_eq!(w.project.journal().len(), 1);
    assert!(w.project.layout(&w.report_id()).is_none());
}

/// AC-4: 100 mm left and right margins on A4 portrait leave a body narrower than 20 mm.
#[test]
fn ac4_body_narrower_than_20mm_is_refused() {
    let w = run_scenario("ac4_body_too_narrow");
    assert_eq!(w.project.journal().len(), 1);
}

/// AC-5: a landscape override on page 2 of `three_page_report`.
#[test]
fn ac5_page_override() {
    let mut w = run_scenario("ac5_page2_landscape");
    let show = w.show();
    let orient: Vec<&str> = arr(get(&show, "pages")).iter().map(|p| st(get(p, "orientation"))).collect();
    assert_eq!(orient, ["portrait", "landscape", "portrait"]);
}

/// AC-6: the journal of AC-5 replays in exact mode to `IDENTICAL`, rebuilding the same
/// reports and page setups.
#[test]
fn ac6_exact_replay_is_identical() {
    for name in ["ac5_page2_landscape", "default_and_overrides", "presets", "boundaries", "refusals"] {
        let w = run_scenario(name);
        let run = replay_exact(w.project.journal(), &w.kinds, project_id());
        assert_eq!(run.outcome, ReplayOutcome::Identical, "{name}");
        assert_eq!(run.first_mismatch, None);
        let rid = w.report_id();
        assert_eq!(run.layouts.get(&rid), w.project.layout(&rid), "{name}: page setup");
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

/// REQ-003, REQ-004: every length case of the oracle.
#[test]
fn length_conversions_match_the_oracle() {
    let all = cases();
    for c in arr(get(&all, "lengths")) {
        let text = st(get(c, "text"));
        match (parse_length(text), c) {
            (Ok(n), CValue::Obj(m)) if m.contains_key("units") => {
                assert_eq!(n.to_string(), st(get(c, "units")), "{text:?}")
            }
            (Err(f), CValue::Obj(m)) if m.contains_key("error") => {
                assert_eq!(f.reason(), st(get(c, "error")), "{text:?}")
            }
            (got, _) => panic!("{text:?}: got {got:?}, expected {c:?}"),
        }
    }
}

/// REQ-001: the presets, portrait and landscape.
#[test]
fn presets_have_the_spec_dimensions() {
    assert_eq!(PageSize::A4.dims(), (21_000, 29_700));
    assert_eq!(PageSize::A3.dims(), (29_700, 42_000));
    assert_eq!(PageSize::Letter.dims(), (21_590, 27_940));
    let mut l = DEFAULT_LAYOUT;
    l.orientation = Orientation::Landscape;
    assert_eq!(l.page_dims(), (29_700, 21_000));
}

/// REQ-006 and replay order: an override on a page that is removed later still replays,
/// because replay checks each event against the report as it was then; show lists live
/// pages only.
#[test]
fn override_then_page_removed_replays_identical() {
    let mut w = World::build("three_page_report");
    let (r, p3) = (w.handle("R"), w.handle("P3"));
    w.layout(SET_PAGE_LAYOUT, &o([("report", r.clone()), ("page", p3.clone()), ("size", s("A3"))])).unwrap();
    w.doc(REMOVE_PAGE, o([("report", r.clone()), ("page", p3.clone())]));
    let show = w.show();
    assert_eq!(arr(get(&show, "pages")).len(), 2);
    let err = w.layout(SET_PAGE_LAYOUT, &o([("report", r), ("page", p3), ("size", s("A4"))])).unwrap_err();
    assert_eq!(err.code(), "report.not_found");
    let run = replay_exact(w.project.journal(), &w.kinds, project_id());
    assert_eq!(run.outcome, ReplayOutcome::Identical);
}

fn ac5_world() -> (World, usize) {
    let w = run_scenario("ac5_page2_landscape");
    let idx = w.project.journal().iter().position(is_layout_event).unwrap();
    (w, idx)
}

fn with_key(v: &CValue, k: &str, x: CValue) -> CValue {
    let CValue::Obj(m) = v else { panic!("not an object") };
    let mut m = m.clone();
    m.insert(k.to_string(), x);
    CValue::Obj(m)
}

/// Forged and broken events (hand-labelled): replay must not accept them.
#[test]
fn forged_layout_events_do_not_replay() {
    let (w, idx) = ac5_world();
    let journal = w.project.journal().to_vec();
    let replay = |j: &[CValue]| replay_exact(j, &w.kinds, project_id());

    // A tampered digest.
    let mut j = journal.clone();
    j[idx] = with_key(&j[idx], "layout_digest", s(&format!("sha256:{}", "0".repeat(64))));
    let run = replay(&j);
    assert_eq!((run.outcome, run.first_mismatch), (ReplayOutcome::Divergent, Some(idx)));

    // A page the report never had.
    let mut j = journal.clone();
    j[idx] = with_key(&j[idx], "page", s(&stranger(3).to_string()));
    assert_eq!(replay(&j).outcome, ReplayOutcome::Divergent);

    // A layout event before its report exists.
    let mut j = vec![journal[idx].clone()];
    j.extend(journal.iter().take(idx).cloned());
    let run = replay(&j);
    assert_eq!((run.outcome, run.first_mismatch), (ReplayOutcome::Divergent, Some(0)));

    // An unknown key, or a layout that does not read back: not replayable.
    let mut j = journal.clone();
    j[idx] = with_key(&j[idx], "extra", s("1"));
    assert_eq!(replay(&j).outcome, ReplayOutcome::NotReplayable);
    let mut j = journal.clone();
    let bad_layout = with_key(get(&journal[idx], "layout"), "header", s("-5"));
    j[idx] = with_key(&j[idx], "layout", bad_layout);
    assert_eq!(replay(&j).outcome, ReplayOutcome::NotReplayable);

    // A layout the handler would refuse (body 10 mm wide), with a self-consistent digest.
    let narrow = PageLayout {
        size: PageSize::A4,
        orientation: Orientation::Portrait,
        margins: Margins { top: 1_000, right: 10_000, bottom: 1_000, left: 10_000 },
        header: 0,
        footer: 0,
    };
    assert!(narrow.check().is_err());
    let report = w.report_id();
    let setup = ReportLayout { default_layout: narrow, overrides: BTreeMap::new() };
    let forged = LayoutEvent { report, page: None, layout: narrow, layout_digest: setup.digest().unwrap() };
    let mut j = journal[..idx].to_vec();
    j.push(forged.to_value());
    assert_eq!(replay(&j).outcome, ReplayOutcome::Divergent);
}

#[test]
fn event_round_trip() {
    let (w, idx) = ac5_world();
    let v = &w.project.journal()[idx];
    let ev = LayoutEvent::from_value(v).unwrap();
    assert_eq!(&ev.to_value(), v);
    assert_eq!(st(get(v, "type")), SET_PAGE_LAYOUT);
    assert_eq!(ev.page.map(|p| p.uuid().to_string()), Some(st(&w.handle("P2")).to_string()));
}

/// Parameters of the wrong shape are `report.bad_params` and journal nothing.
#[test]
fn bad_params() {
    let mut w = World::build("one_page_report");
    let r = w.handle("R");
    let cases = [
        o([("report", r.clone()), ("size", s("A4")), ("bogus", s("1"))]),
        o([("report", r.clone()), ("size", s("B5"))]),
        o([("report", r.clone()), ("size", s("a4"))]),
        o([("report", r.clone())]),
        o([("size", s("A4"))]),
        o([("report", r.clone()), ("size", s("A4")), ("orientation", s("sideways"))]),
        o([("report", s(&w.report.to_string().to_uppercase())), ("size", s("A4"))]),
        o([("report", r.clone()), ("size", s("A4")), ("header", CValue::Bool(true))]),
        CValue::Arr(Vec::new()),
    ];
    for (i, p) in cases.iter().enumerate() {
        let e = w.layout(SET_PAGE_LAYOUT, p).unwrap_err();
        assert_eq!(e.code(), "report.bad_params", "case {i}: {e}");
    }
    let e = w.layout(SHOW_PAGE_LAYOUT, &o([("report", r.clone()), ("page", r.clone())])).unwrap_err();
    assert_eq!(e.code(), "report.bad_params");
    let e = w.layout("report.set_page_layout@2", &o([("report", r)])).unwrap_err();
    assert_eq!(e.code(), "report.bad_params");
    let e = w.layout(SHOW_PAGE_LAYOUT, &o([("report", w.handle("R?"))])).unwrap_err();
    assert_eq!(e.code(), "report.not_found");
    assert_eq!(w.project.journal().len(), 1);
}

/// The registry definitions build, compile and carry the leaf's classes.
#[test]
fn command_definitions() {
    let defs = defs();
    assert_eq!(defs.keys().map(String::as_str).collect::<Vec<_>>(), [SET_PAGE_LAYOUT, SHOW_PAGE_LAYOUT]);
    for d in defs.values() {
        Compiled::compile(&d.params_schema).unwrap();
        Compiled::compile(&d.result_schema).unwrap();
        assert_eq!(d.determinism.as_str(), "D0");
    }
    assert_eq!(defs[SET_PAGE_LAYOUT].side_effect.as_str(), "W2");
    assert_eq!(defs[SHOW_PAGE_LAYOUT].side_effect.as_str(), "R0");
    let p = o([
        ("report", s(&stranger(1).to_string())),
        ("size", s("A4")),
        ("orientation", s("landscape")),
        ("margins", s("15mm,15mm,15mm,15mm")),
        ("header", s("12mm")),
        ("footer", s("10mm")),
    ]);
    assert!(valid(&defs[SET_PAGE_LAYOUT].params_schema, &p));
    assert!(!valid(&defs[SET_PAGE_LAYOUT].params_schema, &with_key(&p, "margins", s("15mm,15mm"))));
    assert!(!valid(&defs[SET_PAGE_LAYOUT].params_schema, &with_key(&p, "header", s("12 mm"))));
}

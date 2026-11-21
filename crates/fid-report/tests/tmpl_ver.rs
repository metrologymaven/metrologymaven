#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Acceptance tests of RPT.MODEL.TMPL.VER (AC-1..AC-6) and the pinned Design rules. Every
//! expected document, digest and journal event comes from the independent Python reference
//! fixtures/rpt/model/tmpl/ver/make_cases.py, never from this crate. Command parameters and
//! results are checked against the registered schemas with `fid_bus::validate::Compiled`.

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_io_reg::{Principal, RootAccess, RootId, RootSet};
use fid_project::{Bits, IdError, ObservationSource, Uuid};
use fid_registry::CommandDef;
use fid_replay::ReplayOutcome;
use fid_report::bind::stub::TableStub;
use fid_report::doc::stub::{ImageStub, TextStub};
use fid_report::doc::{KindRegistry, ReportBackend, ReportError, ReportEvent, ReportId, StoredReport};
use fid_report::tmpl::format::parse_template;
use fid_report::tmpl::ver::*;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::PathBuf;

const BASE_MS: u64 = 1_763_510_400_000;

/// The deterministic ID source of the fixtures: draw k gives ms BASE_MS + k, R = k + 1.
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

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/model/tmpl/ver")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap()
}

fn cases() -> CValue {
    parse(&read("cases.json")).unwrap()
}

fn get<'a>(v: &'a CValue, path: &str) -> &'a CValue {
    let mut at = v;
    for k in path.split('.') {
        at = match at {
            CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("missing {k} in {path}")),
            CValue::Arr(a) => a.get(k.parse::<usize>().unwrap()).unwrap(),
            _ => panic!("not a container at {k}"),
        };
    }
    at
}

fn st<'a>(v: &'a CValue, path: &str) -> &'a str {
    match get(v, path) {
        CValue::Str(s) => s,
        other => panic!("{path} is not a string: {other:?}"),
    }
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn uuid(text: &str) -> Uuid {
    Uuid::parse(text).unwrap()
}

/// The wire form of a value: canonical bytes, parsed back.
fn wire(v: &CValue) -> CValue {
    parse(&to_canonical_bytes(v).unwrap()).unwrap()
}

fn text(v: &CValue) -> String {
    String::from_utf8(to_canonical_bytes(v).unwrap()).unwrap()
}

fn kinds() -> KindRegistry {
    let mut k = KindRegistry::new();
    k.register(Box::new(TextStub)).unwrap();
    k.register(Box::new(ImageStub)).unwrap();
    k.register(Box::new(TableStub)).unwrap();
    k
}

fn builtins() -> BuiltinRegistry {
    let mut b = BuiltinRegistry::new();
    b.register(parse_template(&read("starter.json")).unwrap()).unwrap();
    b.register(parse_template(&read("fai_form3.json")).unwrap()).unwrap();
    b
}

fn roots() -> RootSet {
    let mut r = RootSet::new();
    r.insert(RootId::new("imports").unwrap(), RootAccess::Import, dir());
    r
}

fn defs() -> BTreeMap<String, CommandDef> {
    command_defs().unwrap().into_iter().map(|d| (format!("{}@{}", d.id, d.major), d)).collect()
}

fn assert_valid(schema: &CValue, v: &CValue, what: &str) {
    let c = Compiled::compile(schema).unwrap_or_else(|e| panic!("{what}: schema does not compile: {e}"));
    if let Err(f) = c.evaluate(&wire(v), &NoHooks) {
        panic!("{what}: {} at {} ({})", f.keyword, f.pointer, f.message);
    }
}

/// The project, its ID source and the context of the scenario.
struct World {
    backend: MemoryTemplateBackend,
    ids: Counter,
    kinds: KindRegistry,
    builtins: BuiltinRegistry,
    roots: RootSet,
}

impl World {
    fn new() -> World {
        let c = cases();
        World {
            backend: MemoryTemplateBackend::new(uuid(st(&c, "project"))),
            ids: Counter { next: 0 },
            kinds: kinds(),
            builtins: builtins(),
            roots: roots(),
        }
    }

    /// Runs a template command as `principal`; params and result are checked against the
    /// registered schemas.
    fn tmpl_as(&mut self, principal: Principal, command: &str, params: CValue) -> Result<CValue, VerError> {
        let d = defs();
        let def = d.get(command).unwrap();
        assert_valid(&def.params_schema, &params, &format!("{command} params"));
        let env = Env { kinds: &self.kinds, builtins: &self.builtins, roots: &self.roots, principal };
        let out = execute(command, &params, &mut self.backend, &env, &mut self.ids);
        if let Ok(v) = &out {
            assert_valid(&def.result_schema, v, &format!("{command} result"));
        }
        out
    }

    fn tmpl(&mut self, command: &str, params: CValue) -> Result<CValue, VerError> {
        self.tmpl_as(Principal::Agent, command, params)
    }

    fn doc(&mut self, command: &str, params: CValue) -> CValue {
        fid_report::doc::execute(command, &params, &mut self.backend, &self.kinds, &mut self.ids).unwrap()
    }

    fn journal_len(&self) -> usize {
        self.backend.journal().len()
    }
}

/// AC-1 and the whole oracle scenario: report R, save v1, edit, save v2, a third save
/// without edits, apply acme@1, apply the FAI form, edit and save it as a derived template,
/// save a file through a handle, and save over the unlocked starter (version 2 after the
/// built-in's version 1).
fn scenario(w: &mut World) {
    let c = cases();
    let r = st(&c, "report_r").to_string();
    let page = st(&c, "page_r").to_string();
    let acme = st(&c, "ids.acme").to_string();
    w.doc("report.create_report@1", obj([("title", s(st(&c, "title"))), ("project", s(st(&c, "project")))]));
    w.doc(
        "report.add_block@1",
        obj([
            ("report", s(&r)),
            ("page", s(&page)),
            ("kind", s("table")),
            ("payload", obj([("rows", CValue::Arr(Vec::new()))])),
        ]),
    );
    let save = |id: &str, report: &str| obj([("template", s(id)), ("from_report", s(report))]);
    let v1 = w.tmpl(SAVE_TEMPLATE, save(&acme, &r)).unwrap();
    assert_eq!(get(&v1, "created"), &CValue::Bool(true));
    assert_eq!(st(&v1, "template.digest"), st(&c, "acme_v1.digest"));
    assert_eq!(text(get(&v1, "template.version")), "\"1\"");
    w.doc(
        "report.add_block@1",
        obj([
            ("report", s(&r)),
            ("page", s(&page)),
            ("kind", s("text")),
            ("payload", obj([("text", s(st(&c, "edit_text")))])),
        ]),
    );
    let v2 = w.tmpl(SAVE_TEMPLATE, save(&acme, &r)).unwrap();
    assert_eq!(get(&v2, "created"), &CValue::Bool(true));
    assert_eq!(st(&v2, "template.digest"), st(&c, "acme_v2.digest"));
    assert_ne!(st(&v1, "template.digest"), st(&v2, "template.digest"));
    let before = w.journal_len();
    let v3 = w.tmpl(SAVE_TEMPLATE, save(&acme, &r)).unwrap();
    assert_eq!(get(&v3, "created"), &CValue::Bool(false));
    assert_eq!(text(get(&v3, "template")), text(get(&v2, "template")));
    assert_eq!(w.journal_len(), before, "an identical save journals nothing (REQ-005)");

    let a = w
        .tmpl(
            APPLY_TEMPLATE,
            obj([("template", s(&acme)), ("version", s("1")), ("title", s(st(&c, "report_a.title")))]),
        )
        .unwrap();
    assert_eq!(st(&a, "report_id"), st(&c, "report_a.id"));
    let f = w
        .tmpl(APPLY_TEMPLATE, obj([("template", s(st(&c, "ids.fai"))), ("title", s(st(&c, "report_f.title")))]))
        .unwrap();
    assert_eq!(st(&f, "template.digest"), st(&c, "fai_v1.digest"));
    let f_page = match get(&f, "pages") {
        CValue::Arr(p) => st(&p[0], "page_id").to_string(),
        _ => panic!(),
    };
    w.doc(
        "report.add_block@1",
        obj([
            ("report", s(st(&c, "report_f.id"))),
            ("page", s(&f_page)),
            ("kind", s("text")),
            ("payload", obj([("text", s(st(&c, "edit_text")))])),
        ]),
    );
    let d = w.tmpl(SAVE_TEMPLATE, save(st(&c, "ids.derived"), st(&c, "report_f.id"))).unwrap();
    assert_eq!(st(&d, "template.digest"), st(&c, "derived_v1.digest"));
    let file =
        w.tmpl(SAVE_TEMPLATE, obj([("template", s(st(&c, "ids.file"))), ("file", s(st(&c, "handle")))])).unwrap();
    assert_eq!(st(&file, "template.digest"), st(&c, "file_v1.digest"));
    let starter = w.tmpl(SAVE_TEMPLATE, save(st(&c, "ids.starter"), st(&c, "report_a.id"))).unwrap();
    assert_eq!(text(get(&starter, "template")), text(get(&c, "starter_v2")));
}

// ------------------------------------------------------------------ AC-1, AC-2

#[test]
fn ac1_two_saves_two_versions_and_the_journal_matches_the_oracle() {
    let mut w = World::new();
    scenario(&mut w);
    let c = cases();
    let want: Vec<String> = match get(&c, "journal_texts") {
        CValue::Arr(a) => a
            .iter()
            .map(|v| match v {
                CValue::Str(t) => t.clone(),
                _ => panic!(),
            })
            .collect(),
        _ => panic!(),
    };
    let got: Vec<String> = w.backend.journal_values().iter().map(text).collect();
    assert_eq!(got, want);
    // Every event reads back strictly, and the save events name their source.
    for v in w.backend.journal_values() {
        let t = match &v {
            CValue::Obj(o) => match o.get("type") {
                Some(CValue::Str(t)) => t.clone(),
                _ => panic!(),
            },
            _ => panic!(),
        };
        if TemplateEvent::is_template_type(&t) {
            let e = TemplateEvent::from_json(&to_canonical_bytes(&v).unwrap()).unwrap();
            assert_eq!(e.to_value(), v);
        } else {
            ReportEvent::from_value(&v).unwrap();
        }
    }
}

#[test]
fn ac2_applied_report_records_id_version_and_digest() {
    let mut w = World::new();
    scenario(&mut w);
    let c = cases();
    let a = ReportId::new(uuid(st(&c, "report_a.id"))).unwrap();
    let stored = w.backend.stored(&a).unwrap();
    assert_eq!(String::from_utf8(stored.canonical.clone()).unwrap(), st(&c, "report_a.doc_text"));
    let report = stored.open(&a, &w.kinds).unwrap();
    let t = report.template_ref().unwrap();
    assert_eq!(t.id.to_string(), st(&c, "ids.acme"));
    assert_eq!(t.version, 1);
    assert_eq!(t.digest.as_str(), st(&c, "acme_v1.digest"));
    // REQ-004: the blocks keep the template's block IDs, so their slots are found in the
    // recorded version; the derived template carried the FAI table's slot over.
    let derived = w
        .tmpl(SHOW_TEMPLATE, obj([("template", s(st(&c, "ids.derived")))]))
        .map(|v| text(get(&v, "document")))
        .unwrap();
    assert_eq!(derived, st(&c, "derived_v1.doc_text"));
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_locked_builtin_refuses_saves_and_applies() {
    let mut w = World::new();
    scenario(&mut w);
    let c = cases();
    let before = w.journal_len();
    let objects = w.backend.objects().len();
    let fai = st(&c, "ids.fai").to_string();
    let e = w.tmpl(SAVE_TEMPLATE, obj([("template", s(&fai)), ("from_report", s(st(&c, "report_r")))])).unwrap_err();
    assert_eq!(e.code(), "report.template_locked");
    let e = w.tmpl(SAVE_TEMPLATE, obj([("template", s(&fai)), ("file", s(st(&c, "handle")))])).unwrap_err();
    assert_eq!(e.code(), "report.template_locked");
    assert_eq!(w.journal_len(), before);
    assert_eq!(w.backend.objects().len(), objects);
    let ok = w.tmpl(APPLY_TEMPLATE, obj([("template", s(&fai)), ("title", s("again"))]));
    assert!(ok.is_ok());
    assert_eq!(w.journal_len(), before + 1);
}

#[test]
fn a_project_template_saved_locked_is_locked_too() {
    let mut w = World::new();
    let c = cases();
    // The FAI form file, saved under its own ID into a project without built-ins.
    w.builtins = BuiltinRegistry::new();
    let p = obj([("template", s(st(&c, "ids.fai"))), ("file", s("root://imports/fai_form3.json"))]);
    let first = w.tmpl(SAVE_TEMPLATE, p.clone()).unwrap();
    assert_eq!(text(get(&first, "template")), text(get(&c, "fai_v1")));
    let before = w.journal_len();
    assert_eq!(w.tmpl(SAVE_TEMPLATE, p).unwrap_err().code(), "report.template_locked");
    assert_eq!(w.journal_len(), before);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_list_is_sorted_with_locked_flags_and_show_hashes_to_the_digest() {
    let mut w = World::new();
    scenario(&mut w);
    let c = cases();
    let list = w.tmpl(LIST_TEMPLATE, obj([])).unwrap();
    assert_eq!(text(&list), text(get(&c, "list")));
    let shown = w.tmpl(SHOW_TEMPLATE, obj([("template", s(st(&c, "ids.acme"))), ("version", s("1"))])).unwrap();
    let bytes = to_canonical_bytes(get(&shown, "document")).unwrap();
    assert_eq!(String::from_utf8(bytes.clone()).unwrap(), st(&c, "acme_v1.doc_text"));
    assert_eq!(fid_canon::digest_bytes(&bytes).to_string(), st(&c, "acme_v1.digest"));
    // Latest by default.
    let latest = w.tmpl(SHOW_TEMPLATE, obj([("template", s(st(&c, "ids.acme")))])).unwrap();
    assert_eq!(text(get(&latest, "template")), text(get(&c, "acme_v2")));
    // An unknown version or ID is report.not_found.
    let e = w.tmpl(SHOW_TEMPLATE, obj([("template", s(st(&c, "ids.acme"))), ("version", s("9"))])).unwrap_err();
    assert_eq!(e.code(), "report.not_found");
    let e = w.tmpl(SHOW_TEMPLATE, obj([("template", s("019a9ec0-7000-7000-8000-000000000000"))])).unwrap_err();
    assert_eq!(e.code(), "report.not_found");
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_raw_paths_from_agents_and_invalid_files_fail_and_journal_nothing() {
    let mut w = World::new();
    let c = cases();
    let file = st(&c, "ids.file").to_string();
    let raw = dir().join("file_ok.json").to_string_lossy().into_owned();
    for p in [Principal::Agent, Principal::Script] {
        let e = w.tmpl_as(p, SAVE_TEMPLATE, obj([("template", s(&file)), ("file", s(&raw))])).unwrap_err();
        assert_eq!(e.code(), "raw_path_forbidden");
    }
    assert_eq!(w.roots.fs_calls(), 0, "refused before any file-system call");
    let e = w
        .tmpl(SAVE_TEMPLATE, obj([("template", s(&file)), ("file", s("root://imports/../ver/file_ok.json"))]))
        .unwrap_err();
    assert_eq!(e.code(), "path_outside_root");
    for (name, reason) in [
        ("file_invalid.json", "unknown_field"),
        ("label_switch.json", "label_control"),
        ("label_text.json", "label_control"),
    ] {
        let h = format!("root://imports/{name}");
        let e = w.tmpl(SAVE_TEMPLATE, obj([("template", s(&file)), ("file", s(&h))])).unwrap_err();
        assert_eq!(e.code(), "report.template_invalid", "{name}");
        match &e {
            VerError::Template(t) => assert_eq!(t.reason(), Some(reason), "{name}"),
            other => panic!("{name}: {other:?}"),
        }
    }
    // A file whose template_id is not the template parameter.
    let other = st(&c, "ids.acme");
    let e = w.tmpl(SAVE_TEMPLATE, obj([("template", s(other)), ("file", s(st(&c, "handle")))])).unwrap_err();
    match &e {
        VerError::Template(t) => {
            assert_eq!((t.pointer(), t.reason()), (Some("/template_id"), Some("template_id_mismatch")))
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(w.journal_len(), 0);
    assert!(w.backend.objects().is_empty());
    // A human may pass a raw path.
    let ok = w.tmpl_as(Principal::Human, SAVE_TEMPLATE, obj([("template", s(&file)), ("file", s(&raw))])).unwrap();
    assert_eq!(st(&ok, "template.digest"), st(&c, "file_v1.digest"));
}

// ------------------------------------------------------------------ REQ-008

#[test]
fn req008_unknown_kinds_are_refused_on_save_and_apply() {
    let mut w = World::new();
    let c = cases();
    w.kinds = KindRegistry::new();
    w.kinds.register(Box::new(TableStub)).unwrap();
    let e =
        w.tmpl(SAVE_TEMPLATE, obj([("template", s(st(&c, "ids.file"))), ("file", s(st(&c, "handle")))])).unwrap_err();
    assert_eq!(e.code(), "report.unknown_block_kind");
    let e = w.tmpl(APPLY_TEMPLATE, obj([("template", s(st(&c, "ids.starter"))), ("title", s("x"))])).unwrap_err();
    assert_eq!(e.code(), "report.unknown_block_kind");
    assert_eq!(w.journal_len(), 0);
    assert!(w.backend.objects().is_empty());
}

#[test]
fn a_bound_report_cannot_become_a_template() {
    let mut w = World::new();
    let c = cases();
    w.doc("report.create_report@1", obj([("title", s("t")), ("project", s(st(&c, "project")))]));
    let r = match w.backend.journal_values().first() {
        Some(CValue::Obj(o)) => match (o.get("report"), o.get("page")) {
            (Some(CValue::Str(r)), Some(CValue::Str(p))) => (r.clone(), p.clone()),
            _ => panic!(),
        },
        _ => panic!(),
    };
    let binding = obj([
        ("field", s("/value")),
        ("record", s("019a9a9a-0004-7000-8000-000000000004")),
        ("record_digest", s(&format!("sha256:{}", "11".repeat(32)))),
        ("schema", s("value_binding@1")),
        ("statement_digest", s(&format!("sha256:{}", "22".repeat(32)))),
    ]);
    w.doc(
        "report.add_block@1",
        obj([
            ("report", s(&r.0)),
            ("page", s(&r.1)),
            ("kind", s("table")),
            ("payload", obj([("rows", CValue::Arr(vec![obj([("value", binding)])]))])),
        ]),
    );
    let before = w.journal_len();
    let e = w.tmpl(SAVE_TEMPLATE, obj([("template", s(st(&c, "ids.acme"))), ("from_report", s(&r.0))])).unwrap_err();
    match &e {
        VerError::Template(t) => assert_eq!(t.reason(), Some("bound_value")),
        other => panic!("{other:?}"),
    }
    assert_eq!(w.journal_len(), before);
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_exact_replay_is_identical_with_the_same_digests() {
    let mut w = World::new();
    scenario(&mut w);
    let run =
        replay_exact(&w.backend.journal_values(), w.backend.objects(), &w.kinds, &w.builtins, w.backend.project());
    assert_eq!(run.outcome, ReplayOutcome::Identical);
    assert_eq!(run.first_mismatch, None);
    for (id, (_, d)) in &run.reports {
        assert_eq!(w.backend.stored(id).unwrap().digest, *d);
    }
    let c = cases();
    let want: Vec<&str> = ["acme_v1", "acme_v2", "derived_v1", "file_v1", "starter_v2"]
        .iter()
        .map(|k| st(&c, &format!("{k}.digest")))
        .collect();
    let got: Vec<&str> = run.templates.iter().map(|t| t.digest.as_str()).collect();
    assert_eq!(got, want);
}

fn first_index(values: &[CValue], ty: &str) -> usize {
    values.iter().position(|v| matches!(v, CValue::Obj(o) if o.get("type") == Some(&s(ty)))).unwrap()
}

#[test]
fn a_changed_template_never_replays_silently() {
    let mut w = World::new();
    scenario(&mut w);
    let c = cases();
    let journal = w.backend.journal_values();
    // 1. Other bytes under a stored digest: caught at the first event that reads them.
    let d1 = Digest::parse(st(&c, "acme_v1.digest")).unwrap();
    let mut store = w.backend.objects().clone();
    let mut bytes = store.get(&d1).unwrap().clone();
    let at = bytes.iter().position(|b| *b == b'B').unwrap();
    bytes[at] = b'C';
    store.insert(d1, bytes);
    let run = replay_exact(&journal, &store, &w.kinds, &w.builtins, w.backend.project());
    assert_eq!(run.outcome, ReplayOutcome::Divergent);
    assert_eq!(run.first_mismatch, Some(first_index(&journal, SAVE_TEMPLATE)));
    // 2. A missing object.
    let mut store = w.backend.objects().clone();
    store.remove(&d1);
    let run = replay_exact(&journal, &store, &w.kinds, &w.builtins, w.backend.project());
    assert_eq!(run.outcome, ReplayOutcome::Divergent);
    // 3. A built-in that changed under the same version: the apply event names the old
    //    digest for fai@1, which now resolves to another digest.
    let mut changed = BuiltinRegistry::new();
    changed.register(parse_template(&read("starter.json")).unwrap()).unwrap();
    let fai = String::from_utf8(read("fai_form3.json")).unwrap().replace("FAI form 3", "FAI form 3 rev");
    changed.register(parse_template(fai.as_bytes()).unwrap()).unwrap();
    let run = replay_exact(&journal, w.backend.objects(), &w.kinds, &changed, w.backend.project());
    assert_eq!(run.outcome, ReplayOutcome::Divergent);
    let fai_apply = journal
        .iter()
        .position(|v| text(v).contains(st(&c, "fai_v1.digest")) && text(v).contains(APPLY_TEMPLATE))
        .unwrap();
    assert_eq!(run.first_mismatch, Some(fai_apply));
    // 4. A save event whose recorded version skips one.
    let mut forged = journal.clone();
    let i = first_index(&forged, SAVE_TEMPLATE);
    let t = text(&forged[i]).replace("\"version\":\"1\"", "\"version\":\"2\"");
    forged[i] = parse(t.as_bytes()).unwrap();
    let run = replay_exact(&forged, w.backend.objects(), &w.kinds, &w.builtins, w.backend.project());
    assert_eq!((run.outcome, run.first_mismatch), (ReplayOutcome::Divergent, Some(i)));
    // 5. A save from a report whose report changed since: the rebuilt template differs.
    let mut forged = journal.clone();
    let i = first_index(&forged, SAVE_TEMPLATE);
    forged.swap(i, i + 1);
    let run = replay_exact(&forged, w.backend.objects(), &w.kinds, &w.builtins, w.backend.project());
    assert_eq!(run.outcome, ReplayOutcome::Divergent);
    // 6. A missing block kind is not replayable.
    let mut k = KindRegistry::new();
    k.register(Box::new(TableStub)).unwrap();
    let run = replay_exact(&journal, w.backend.objects(), &k, &w.builtins, w.backend.project());
    assert_eq!(run.outcome, ReplayOutcome::NotReplayable);
}

// ------------------------------------------------------------------ REQ-002

/// A backend whose store serves other bytes under a digest.
struct Tampered(MemoryTemplateBackend);

impl ReportBackend for Tampered {
    fn project(&self) -> Uuid {
        self.0.project()
    }
    fn load(&self, id: &ReportId) -> Result<Option<StoredReport>, ReportError> {
        self.0.load(id)
    }
    fn commit(&mut self, id: &ReportId, stored: StoredReport, event: ReportEvent) -> Result<(), ReportError> {
        self.0.commit(id, stored, event)
    }
}

impl ObjectRead for Tampered {
    fn object(&self, digest: &Digest) -> Result<Option<Vec<u8>>, ReportError> {
        Ok(self.0.object(digest)?.map(|mut b| {
            b.push(b' ');
            b
        }))
    }
}

impl TemplateBackend for Tampered {
    fn template_events(&self) -> Result<Vec<TemplateEvent>, ReportError> {
        self.0.template_events()
    }
    fn commit_save(&mut self, object: Vec<u8>, event: TemplateEvent) -> Result<(), ReportError> {
        self.0.commit_save(object, event)
    }
    fn commit_apply(
        &mut self,
        object: Vec<u8>,
        id: &ReportId,
        stored: StoredReport,
        event: TemplateEvent,
    ) -> Result<(), ReportError> {
        self.0.commit_apply(object, id, stored, event)
    }
}

#[test]
fn req002_stored_versions_are_read_only_through_their_digest() {
    let mut w = World::new();
    scenario(&mut w);
    let c = cases();
    let mut t = Tampered(w.backend.clone());
    let env = Env { kinds: &w.kinds, builtins: &w.builtins, roots: &w.roots, principal: Principal::Agent };
    let p = obj([("template", s(st(&c, "ids.acme"))), ("version", s("1"))]);
    let e = execute(SHOW_TEMPLATE, &p, &mut t, &env, &mut w.ids).unwrap_err();
    assert_eq!(e.code(), "report.store_invalid");
    let p = obj([("template", s(st(&c, "ids.acme"))), ("title", s("x"))]);
    let before = t.0.journal().len();
    let e = execute(APPLY_TEMPLATE, &p, &mut t, &env, &mut w.ids).unwrap_err();
    assert_eq!(e.code(), "report.store_invalid");
    assert_eq!(t.0.journal().len(), before);
    // The memory store refuses a commit whose object does not match its event.
    let ev = TemplateEvent::Save {
        template: fid_report::doc::TemplateRef {
            id: uuid(st(&c, "ids.acme")),
            version: 3,
            digest: Digest::parse(st(&c, "acme_v1.digest")).unwrap(),
        },
        locked: false,
        source: SaveSource::File { handle: fid_report::doc::Untrusted::text("x").unwrap() },
    };
    assert!(w.backend.commit_save(b"other".to_vec(), ev).is_err());
}

// ------------------------------------------------------------------ hostile params

#[test]
fn params_are_strict() {
    let mut w = World::new();
    let c = cases();
    let env_run = |w: &mut World, cmd: &str, p: CValue| {
        let env = Env { kinds: &w.kinds, builtins: &w.builtins, roots: &w.roots, principal: Principal::Agent };
        execute(cmd, &p, &mut w.backend, &env, &mut w.ids)
    };
    let acme = st(&c, "ids.acme");
    for (cmd, p) in [
        (SAVE_TEMPLATE, obj([("template", s(acme))])),
        (SAVE_TEMPLATE, obj([("template", s(acme)), ("from_report", s(acme)), ("file", s("root://imports/x.json"))])),
        (SAVE_TEMPLATE, obj([("template", s(acme)), ("file", s(""))])),
        (SAVE_TEMPLATE, obj([("template", s(&acme.to_uppercase())), ("from_report", s(acme))])),
        (SAVE_TEMPLATE, obj([("template", s(acme)), ("from_report", s(acme)), ("extra", CValue::Null)])),
        (SHOW_TEMPLATE, obj([("template", s(acme)), ("version", s("0"))])),
        (SHOW_TEMPLATE, obj([("template", s(acme)), ("version", s("01"))])),
        (SHOW_TEMPLATE, obj([("template", s(acme)), ("version", s("4294967296"))])),
        (SHOW_TEMPLATE, obj([("template", s(acme)), ("version", CValue::Bool(true))])),
        (APPLY_TEMPLATE, obj([("template", s(acme))])),
        (APPLY_TEMPLATE, obj([("template", s(acme)), ("title", s("a\u{0}b"))])),
        (LIST_TEMPLATE, obj([("x", CValue::Null)])),
        ("report.delete_template@1", obj([])),
    ] {
        let e = env_run(&mut w, cmd, p).unwrap_err();
        assert!(matches!(e.code(), "report.bad_params" | "report.text_invalid"), "{cmd}: {}", e.code());
    }
    // Duplicate keys never reach a handler: the bounded parser refuses them.
    assert!(parse(br#"{"template":"a","template":"b"}"#).is_err());
    assert_eq!(
        TemplateEvent::from_json(br#"{"type":"report.save_template@1","type":"x"}"#).unwrap_err().code(),
        "report.store_invalid"
    );
    assert_eq!(w.journal_len(), 0);
}

#[test]
fn events_read_back_strictly() {
    let mut w = World::new();
    scenario(&mut w);
    for v in w.backend.journal_values() {
        let t = text(&v);
        if !t.contains("_template@1") {
            continue;
        }
        // An extra key, a missing key, a bad digest or a zero version are all refused.
        let extra = t.replacen('{', "{\"extra\":null,", 1);
        assert!(TemplateEvent::from_json(extra.as_bytes()).is_err());
        let missing = t.replacen("\"type\":", "\"typ\":", 1);
        assert!(TemplateEvent::from_json(missing.as_bytes()).is_err());
        let digest = t.replacen("sha256:", "sha256:zz", 1);
        assert!(TemplateEvent::from_json(digest.as_bytes()).is_err());
        let zero = t.replacen("\"version\":\"", "\"version\":\"0", 1);
        assert!(TemplateEvent::from_json(zero.as_bytes()).is_err());
    }
}

#[test]
fn selectors_and_registration() {
    let c = cases();
    let acme = st(&c, "ids.acme");
    assert_eq!(parse_selector(acme).unwrap().1, None);
    assert_eq!(parse_selector(&format!("{acme}@2")).unwrap().1, Some(2));
    let bad: Vec<String> =
        vec!["acme-char@1".into(), format!("{acme}@0"), format!("{acme}@x"), format!("{acme}@01"), String::new()];
    for b in &bad {
        assert!(parse_selector(b).is_err(), "{b}");
    }
    // Registering identical bytes twice returns the same version; a change adds the next.
    let mut b = BuiltinRegistry::new();
    let one = b.register(parse_template(&read("starter.json")).unwrap()).unwrap();
    assert_eq!(b.register(parse_template(&read("starter.json")).unwrap()).unwrap(), one);
    let changed = String::from_utf8(read("starter.json")).unwrap().replace("\"Starter\"", "\"Starter 2\"");
    let two = b.register(parse_template(changed.as_bytes()).unwrap()).unwrap();
    assert_eq!((one.version, two.version), (1, 2));
    assert_eq!(text(&one.to_value()), text(get(&c, "starter_v1")));
}

#[test]
fn label_control_keys_are_pinned() {
    // D-045: the list a template payload may never hold (hand-labelled from the leaf's
    // Design note in the PR).
    assert_eq!(
        LABEL_CONTROL_KEYS,
        [
            "hide_labels",
            "interpretation",
            "label_mode",
            "labels",
            "provisional",
            "show_labels",
            "suppress_labels",
            "value_labels"
        ]
    );
}

/// Mutated template files (bytes flipped, truncated, duplicated keys) never panic and never
/// journal anything unless they still parse as a valid template.
#[test]
fn mutated_files_never_panic() {
    let base = read("file_ok.json");
    let tmp = std::env::temp_dir().join(format!("fid-tmpl-ver-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut roots = RootSet::new();
    roots.insert(RootId::new("imports").unwrap(), RootAccess::Import, &tmp);
    let c = cases();
    let mut w = World::new();
    w.roots = roots;
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    for i in 0..300 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let mut b = base.clone();
        match i % 4 {
            0 => {
                let at = (seed as usize) % b.len();
                b[at] = (seed >> 32) as u8;
            }
            1 => b.truncate((seed as usize) % b.len()),
            2 => b = [b"{\"locked\":true,".as_slice(), &b[1..]].concat(),
            _ => {
                let at = (seed as usize) % b.len();
                b.insert(at, b'{');
            }
        }
        std::fs::write(tmp.join("m.json"), &b).unwrap();
        let before = w.journal_len();
        let r = w.tmpl(SAVE_TEMPLATE, obj([("template", s(st(&c, "ids.file"))), ("file", s("root://imports/m.json"))]));
        if r.is_err() {
            assert_eq!(w.journal_len(), before);
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

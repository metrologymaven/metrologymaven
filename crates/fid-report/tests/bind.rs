#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::disallowed_types)]
//! Acceptance tests of RPT.MODEL.BIND.RESOLVE (AC-1..AC-8) and the pinned rules of
//! `fid_report::bind`. Journals, digests and expected bytes come from
//! fixtures/rpt/model/bracket_3stage/make_fixtures.py, never from this crate.

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_project::{Bits, IdError, ObservationSource, Uuid};
use fid_registry::{check_schema, validate, Catalogue, SideEffect, Surface};
use fid_replay::ReplayOutcome;
use fid_report::bind::stub::{TableResolver, TableStub};
use fid_report::bind::*;
use fid_report::doc::stub::TextStub;
use fid_report::doc::{
    add_block, create_report, replay_exact, AddBlock, BlockId, BlockKindId, CreateReport, KindRegistry, MemoryBackend,
    Report, ReportId, Untrusted, BIND_BLOCK,
};
use fid_report::fmt::SiValue;
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

fn fixture(rel: &str) -> CValue {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/model").join(rel);
    parse(&std::fs::read(p).unwrap()).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("no key {k}")),
        _ => panic!("not an object"),
    }
}

fn st<'a>(v: &'a CValue, k: &str) -> &'a str {
    match get(v, k) {
        CValue::Str(s) => s,
        _ => panic!("{k} is not a string"),
    }
}

fn arr(v: &CValue) -> &Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("not an array"),
    }
}

fn uuid(s: &str) -> Uuid {
    Uuid::parse(s).unwrap()
}

fn canon(v: &CValue) -> String {
    String::from_utf8(to_canonical_bytes(v).unwrap()).unwrap()
}

fn digest(s: &str) -> Digest {
    Digest::parse(s).unwrap()
}

/// Loads a fixture journal into a [`MemoryJournal`].
fn journal(rel: &str) -> MemoryJournal {
    let mut j = MemoryJournal::new();
    for e in arr(get(&fixture(rel), "entries")) {
        let kind = match st(e, "kind") {
            "result" => EntityKind::Result,
            "characteristic" => EntityKind::Characteristic,
            "named_view" => EntityKind::NamedView,
            "series" => EntityKind::Series,
            "statement" => EntityKind::Statement,
            k => panic!("kind {k}"),
        };
        let id = match get(e, "id") {
            CValue::Str(s) => Some(uuid(s)),
            _ => None,
        };
        let head = JournalHead::parse(st(e, "head")).unwrap();
        j.append(head, uuid(st(e, "project")), kind, id, st(e, "text").as_bytes().to_vec()).unwrap();
    }
    j
}

fn kinds() -> KindRegistry {
    let mut k = KindRegistry::new();
    k.register(Box::new(TableStub)).unwrap();
    k.register(Box::new(TextStub)).unwrap();
    k
}

fn resolvers() -> ResolverRegistry {
    let mut r = ResolverRegistry::new();
    r.register(Box::new(TableResolver)).unwrap();
    r
}

const HUMAN: Caller = Caller { principal: Principal::Human, surface: Surface::Cli };
const MAIN: &str = "main";

/// A form data source with one field.
struct Form;

impl FieldSource for Form {
    fn field(&self, id: &FieldId) -> Option<FieldValue> {
        (id.as_str() == "fai.part_number").then(|| FieldValue::Text(Untrusted::text("BRK-3").unwrap()))
    }

    fn rows(&self, _table: &FieldId) -> Vec<RowSource> {
        Vec::new()
    }
}

struct World {
    backend: MemoryBackend,
    kinds: KindRegistry,
    resolvers: ResolverRegistry,
    journal: MemoryJournal,
    ids: Counter,
    project: Uuid,
    report: ReportId,
    page: Uuid,
}

impl World {
    /// A project with one report whose first page holds one `table` block `{"rows": []}`.
    fn new(project: Uuid, journal_rel: &str) -> (World, BlockId) {
        let kinds = kinds();
        let mut backend = MemoryBackend::new(project);
        let mut ids = Counter { next: 0 };
        let title = Untrusted::text("Bracket 3-stage FAI").unwrap();
        let r = create_report(&mut backend, &kinds, &mut ids, &CreateReport { title, project }).unwrap();
        let page = r.pages()[0].id().uuid();
        let mut w = World {
            backend,
            kinds,
            resolvers: resolvers(),
            journal: journal(journal_rel),
            ids,
            project,
            report: r.id(),
            page,
        };
        let b = w.add(&parse(br#"{"rows":[]}"#).unwrap(), "table");
        (w, b)
    }

    fn add(&mut self, payload: &CValue, kind: &str) -> BlockId {
        let p = AddBlock {
            report: self.report.uuid(),
            page: self.page,
            after: None,
            kind: BlockKindId::new(kind),
            payload: payload.clone(),
        };
        add_block(&mut self.backend, &self.kinds, &mut self.ids, &p).unwrap()
    }

    fn bind_as(&mut self, caller: Caller, branch: &str, args: &[&str]) -> Result<CValue, BindError> {
        let sources: [&dyn FieldSource; 1] = [&Form];
        let mut ctx = BindCtx {
            backend: &mut self.backend,
            kinds: &self.kinds,
            resolvers: &self.resolvers,
            journal: &self.journal,
            sources: &sources,
            caller,
            branch,
        };
        invoke_cli(&mut ctx, args)
    }

    fn bind(&mut self, block: BlockId, flag: &str, ids: &str) -> Result<CValue, BindError> {
        let (r, b) = (self.report.uuid().to_string(), block.uuid().to_string());
        self.bind_as(HUMAN, MAIN, &["bind-block", "--report", &r, "--block", &b, flag, ids, "--json"])
    }

    fn stored(&self) -> Report {
        let s = self.backend.stored(&self.report).unwrap();
        s.open(&self.report, &self.kinds).unwrap()
    }

    fn resolve(&self, block: BlockId, head: &str) -> Result<ResolvedBlock, BindError> {
        let r = self.stored();
        let b = r.pages().iter().flat_map(|p| p.blocks()).find(|b| b.id() == block).unwrap().clone();
        let sources: Vec<&dyn FieldSource> = vec![&Form];
        let ctx = ResolveContext {
            project: self.project,
            head: JournalHead::parse(head).unwrap(),
            stages: Vec::new(),
            runs: Vec::new(),
            sources,
        };
        resolve_block(&b, &ctx, &self.kinds, &self.resolvers, &self.journal)
    }

    fn payload_of(&self, block: BlockId) -> CValue {
        let r = self.stored();
        let b = r.pages().iter().flat_map(|p| p.blocks()).find(|b| b.id() == block).unwrap();
        b.payload().value().clone()
    }

    fn journaled(&self) -> usize {
        self.backend.journal().len()
    }
}

fn bracket() -> (World, BlockId, CValue) {
    let cases = fixture("bracket_3stage/cases.json");
    let (w, b) = World::new(uuid(st(&cases, "project")), "bracket_3stage/journal.json");
    (w, b, cases)
}

/// AC-1 done: r1 and r2 bound to the table block.
fn bound() -> (World, BlockId, CValue) {
    let (mut w, b, cases) = bracket();
    let ids = format!("{},{}", st(&cases, "r1"), st(&cases, "r2"));
    w.bind(b, "--results", &ids).unwrap();
    (w, b, cases)
}

/// Replaces the value at `pointer` (all but the last token are object keys or indices).
fn set_at(v: &mut CValue, pointer: &str, new: CValue) {
    let mut at = v;
    let tokens: Vec<&str> = pointer.trim_start_matches('/').split('/').collect();
    let (last, path) = tokens.split_last().unwrap();
    for t in path {
        at = match at {
            CValue::Obj(o) => o.get_mut(*t).unwrap(),
            CValue::Arr(a) => &mut a[t.parse::<usize>().unwrap()],
            _ => panic!("bad pointer"),
        };
    }
    match at {
        CValue::Obj(o) => {
            o.insert((*last).to_string(), new);
        }
        _ => panic!("bad pointer"),
    }
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_bind_stores_only_ids_and_journals_one_event() {
    let (mut w, b, cases) = bracket();
    let ac1 = get(&cases, "ac1");
    assert_eq!(b.uuid().to_string(), st(ac1, "block"));
    assert_eq!(w.report.uuid().to_string(), st(ac1, "report"));
    assert_eq!(canon(&w.payload_of(b)), st(ac1, "initial_payload_text"));
    let before = w.journaled();
    let ids = format!("{},{}", st(&cases, "r1"), st(&cases, "r2"));
    let result = w.bind(b, "--results", &ids).unwrap();
    assert_eq!(canon(&result), st(ac1, "result_text"));
    assert_eq!(w.journaled(), before + 1);
    let event = w.backend.journal().last().unwrap();
    assert_eq!(event.event_type(), BIND_BLOCK);
    assert_eq!(canon(&event.to_value()), st(ac1, "event_text"));
    // The stored payload: every declared value field a value_binding@1, no number, no value
    // text; DOC re-reads it (stored() re-checks every block).
    let payload = w.payload_of(b);
    assert_eq!(canon(&payload), st(ac1, "payload_text"));
    let text = canon(&payload);
    assert!(!text.contains("value_text@1") && !text.contains("0x"));
    for row in arr(get(&payload, "rows")) {
        assert_eq!(st(get(row, "value"), "schema"), "value_binding@1");
    }
    // The params and the result validate against the declared schemas.
    let decl = &declarations()[0];
    let params = CValue::Obj(BTreeMap::from([
        ("report".to_string(), CValue::Str(w.report.uuid().to_string())),
        ("block".to_string(), CValue::Str(b.uuid().to_string())),
        ("refs".to_string(), get(&result, "refs").clone()),
    ]));
    conforms(&decl.def.params_schema, &params, "").unwrap();
    conforms(&decl.def.result_schema, &result, "").unwrap();
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_a_reference_or_payload_carrying_a_value_is_refused() {
    let (mut w, b, cases) = bracket();
    let ac2 = get(&cases, "ac2");
    let r#ref = parse(st(ac2, "ref_text").as_bytes()).unwrap();
    let params = CValue::Obj(BTreeMap::from([
        ("report".to_string(), CValue::Str(w.report.uuid().to_string())),
        ("block".to_string(), CValue::Str(b.uuid().to_string())),
        ("refs".to_string(), CValue::Arr(vec![r#ref])),
    ]));
    let before = w.journaled();
    let sources: [&dyn FieldSource; 0] = [];
    let mut ctx = BindCtx {
        backend: &mut w.backend,
        kinds: &w.kinds,
        resolvers: &w.resolvers,
        journal: &w.journal,
        sources: &sources,
        caller: HUMAN,
        branch: MAIN,
    };
    let e = execute_bind(&mut ctx, &params).unwrap_err();
    assert_eq!(e.code(), st(ac2, "code"));
    assert_eq!(st(&e.to_value(), "field"), st(ac2, "field"));
    assert_eq!(w.journaled(), before);
    // The AC-2 params also fail the declared params schema (a ref is exactly {kind, id}).
    conforms(&declarations()[0].def.params_schema, &params, "").unwrap_err();
    // A payload holding a number next to the bound record (in an undeclared place DOC
    // accepts) is refused when bound, and nothing is journaled.
    let noted = w.add(&parse(br#"{"note":{"value":12.003},"rows":[]}"#).unwrap(), "table");
    let before = w.journaled();
    let e = w.bind(noted, "--results", st(&cases, "r1")).unwrap_err();
    assert_eq!(e.code(), "report.binding_has_value");
    assert_eq!(st(&e.to_value(), "field"), "/note/value");
    assert_eq!(w.journaled(), before);
    // Resolving that payload is refused too.
    let e = w.resolve(noted, st(&cases, "current_head")).unwrap_err();
    assert_eq!(e.code(), "report.binding_has_value");
    assert_eq!(st(&e.to_value(), "reason"), "number");
}

#[test]
fn value_text_and_bit_patterns_are_refused_at_bind_and_at_resolve() {
    let (mut w, _, cases) = bracket();
    for x in arr(get(&cases, "value_strings")) {
        let (text, reason) = (st(x, "text"), st(x, "reason"));
        let note = CValue::Obj(BTreeMap::from([
            ("note".to_string(), CValue::Arr(vec![CValue::Str("C01".to_string()), CValue::Str(text.to_string())])),
            ("rows".to_string(), CValue::Arr(Vec::new())),
        ]));
        let blk = w.add(&note, "table");
        let before = w.journaled();
        let e = w.bind(blk, "--results", st(&cases, "r1")).unwrap_err();
        assert_eq!(e.code(), "report.binding_has_value", "{text}");
        assert_eq!((st(&e.to_value(), "field"), st(&e.to_value(), "reason")), ("/note/1", reason), "{text}");
        assert_eq!(w.journaled(), before);
        let e = w.resolve(blk, st(&cases, "current_head")).unwrap_err();
        assert_eq!(st(&e.to_value(), "reason"), reason, "{text}");
        assert!(is_number_text(text) || is_bit_pattern(text));
    }
    for x in arr(get(&cases, "not_value_strings")) {
        let CValue::Str(text) = x else { panic!() };
        assert!(!is_number_text(text) && !is_bit_pattern(text), "{text}");
        let note = CValue::Obj(BTreeMap::from([
            ("note".to_string(), CValue::Str(text.clone())),
            ("rows".to_string(), CValue::Arr(Vec::new())),
        ]));
        let blk = w.add(&note, "table");
        w.bind(blk, "--results", st(&cases, "r1")).unwrap();
        w.resolve(blk, st(&cases, "current_head")).unwrap();
    }
}

#[test]
fn a_uec0_result_cannot_be_bound() {
    let (mut w, b, cases) = bracket();
    let before = w.journaled();
    let e = w.bind(b, "--results", st(&cases, "uec0_record")).unwrap_err();
    assert_eq!(e.code(), "report.value_binding_invalid");
    assert_eq!(st(&e.to_value(), "reason"), "no_statement");
    assert_eq!(w.journaled(), before);
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_resolution_is_as_of_the_given_head() {
    let (w, b, cases) = bound();
    let before = w.journaled();
    let e = w.resolve(b, st(&cases, "h0")).unwrap_err();
    assert_eq!(e, BindError::UnjournaledRecord { block: b, record: st(&cases, "r2").to_string() });
    assert_eq!(e.code(), "report.unjournaled_record");
    let ok = w.resolve(b, st(&cases, "current_head")).unwrap();
    let view = ok.view();
    assert_eq!(view.block(), b);
    assert_eq!(view.refs().len(), 2);
    let expected = arr(get(&cases, "ac3_values"));
    assert_eq!(view.values().len(), expected.len());
    for (v, x) in view.values().iter().zip(expected) {
        assert_eq!(v.pointer(), st(x, "pointer"));
        assert_eq!(v.binding().record, uuid(st(x, "record")));
        assert_eq!(v.quantity().unit().as_str(), st(x, "unit"));
        let bits = u64::from_str_radix(st(x, "bits").trim_start_matches("0x"), 16).unwrap();
        assert_eq!(v.quantity().value(), SiValue::from_bits(bits));
        assert_eq!(v.statement().digest(), digest(st(x, "statement_digest")));
        assert_eq!(
            Digest::from_bytes(fid_canon::digest_bytes(v.statement().as_bytes()).0),
            digest(st(x, "statement_digest"))
        );
        // The evidence facts come from the journaled record (hand-labelled in the fixture).
        let ev = get(x, "evidence");
        let e = v.evidence();
        assert_eq!(e.basis().wire(), st(ev, "basis"));
        assert_eq!(e.class().as_str(), st(ev, "class"));
        assert_eq!(e.method(), Some(st(ev, "method")));
        assert_eq!(CValue::Bool(e.is_claimed()), *get(ev, "claimed"));
        assert_eq!(e.outcome().as_str(), st(ev, "outcome"));
        let rule = match get(ev, "rule") {
            CValue::Str(r) => Some(r.as_str()),
            _ => None,
        };
        assert_eq!(e.rule(), rule);
        assert_eq!(CValue::Bool(e.looks_measured()), *get(ev, "looks_measured"));
        assert!(!e.rests_on_declared_input() && !e.rests_on_uncharacterized_input());
    }
    // The layout holds pointers only; resolution journals nothing.
    assert_eq!(canon(ok.layout()), r#"{"rows":["/rows/0/value","/rows/1/value"]}"#);
    assert_eq!(w.journaled(), before);
    // A head that is no entry of the journal: never a fallback to "latest".
    let e = w.resolve(b, &format!("sha256:{}", "7".repeat(64))).unwrap_err();
    assert_eq!(e.code(), "report.not_found");
}

#[test]
fn ac3_every_reference_kind_is_looked_up_at_the_head() {
    let (mut w, b, cases) = bracket();
    let (r, bs) = (w.report.uuid().to_string(), b.uuid().to_string());
    let args = [
        "bind-block",
        "--report",
        &r,
        "--block",
        &bs,
        "--characteristics",
        st(&cases, "characteristic_1"),
        "--views",
        st(&cases, "view"),
        "--series",
        st(&cases, "series"),
        "--fields",
        "fai.part_number",
        "--results",
        st(&cases, "r1"),
        "--json",
    ];
    w.bind_as(HUMAN, MAIN, &args).unwrap();
    let ok = w.resolve(b, st(&cases, "current_head")).unwrap();
    let kinds: Vec<&str> = ok
        .view()
        .refs()
        .iter()
        .map(|r| match r {
            ResolvedRef::Characteristic(_) => "characteristic",
            ResolvedRef::NamedView(_) => "named_view",
            ResolvedRef::Series(_) => "series",
            ResolvedRef::Field { .. } => "field",
            ResolvedRef::Result { .. } => "result",
        })
        .collect();
    assert_eq!(kinds, ["characteristic", "named_view", "series", "field", "result"]);
    // At the head of the second entry (the series), the view and the series are journaled
    // but the characteristic is not: the first reference fails.
    let journal = fixture("bracket_3stage/journal.json");
    let entries = arr(get(&journal, "entries"));
    let e = w.resolve(b, st(&entries[1], "head")).unwrap_err();
    assert_eq!(e, BindError::UnjournaledRecord { block: b, record: st(&cases, "characteristic_1").to_string() });
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_cross_project_ids_are_refused_and_leak_nothing() {
    let cases = fixture("two_projects/cases.json");
    let (mut w, b) = World::new(uuid(st(&cases, "project_a")), "two_projects/journal.json");
    let x = get(&cases, "expected");
    let foreign = uuid(st(x, "project"));
    let keys: Vec<&str> = arr(get(x, "error_keys"))
        .iter()
        .map(|k| match k {
            CValue::Str(s) => s.as_str(),
            _ => panic!(),
        })
        .collect();
    let rb = st(get(&cases, "b"), "record");
    // Binding it is refused.
    let before = w.journaled();
    let e = w.bind(b, "--results", rb).unwrap_err();
    assert_eq!(e.code(), st(x, "code"));
    assert_eq!(e, BindError::CrossProjectBinding { block: b, project: foreign });
    assert_eq!(w.journaled(), before);
    let e = w.bind(b, "--characteristics", st(get(&cases, "b"), "characteristic")).unwrap_err();
    assert_eq!(e, BindError::CrossProjectBinding { block: b, project: foreign });
    // A block that holds B's record (written without the command) fails resolution, and the
    // error holds exactly the block and the foreign project.
    let payload = CValue::Obj(BTreeMap::from([
        (
            "binding".to_string(),
            parse(format!(r#"{{"refs":[{{"id":"{rb}","kind":"result"}}],"schema":"binding@1"}}"#).as_bytes()).unwrap(),
        ),
        (
            "rows".to_string(),
            CValue::Arr(vec![CValue::Obj(BTreeMap::from([(
                "value".to_string(),
                get(get(&cases, "b"), "binding").clone(),
            )]))]),
        ),
    ]));
    let held = w.add(&payload, "table");
    let e = w.resolve(held, st(&cases, "current_head")).unwrap_err();
    assert_eq!(e, BindError::CrossProjectBinding { block: held, project: foreign });
    let CValue::Obj(o) = e.to_value() else { panic!() };
    assert_eq!(o.keys().map(String::as_str).collect::<Vec<_>>(), keys);
    // Saved views and series are not owner-checked (cross-project data enters reports as
    // statistics series): a series journaled by B binds in A.
    let series = uuid("019a9a9b-0007-7000-8000-000000000001");
    let head = JournalHead::parse(&format!("sha256:{}", "8".repeat(64))).unwrap();
    w.journal.append(head, foreign, EntityKind::Series, Some(series), br#"{"name":"B trend"}"#.to_vec()).unwrap();
    w.bind(b, "--series", &series.to_string()).unwrap();
    w.resolve(b, head.digest().as_str()).unwrap();
    // A's own record binds and resolves.
    w.bind(b, "--results", st(get(&cases, "a"), "record")).unwrap();
    w.resolve(b, head.digest().as_str()).unwrap();
    // Both projects store byte-identical statements; each record resolves its own.
    let tj = fixture("two_projects/journal.json");
    let stmts: Vec<&CValue> = arr(get(&tj, "entries")).iter().filter(|e| st(e, "kind") == "statement").collect();
    assert_eq!(st(stmts[0], "text"), st(stmts[1], "text"));
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_a_kind_without_resolver_is_refused() {
    let (mut w, _, cases) = bracket();
    let text = w.add(&parse(br#"{"text":"notes"}"#).unwrap(), "text");
    let e = w.resolve(text, st(&cases, "current_head")).unwrap_err();
    assert_eq!(e, BindError::NoResolver(BlockKindId::new("text").unwrap()));
    assert_eq!(e.code(), "report.no_resolver");
    let before = w.journaled();
    assert_eq!(w.bind(text, "--results", st(&cases, "r1")).unwrap_err().code(), "report.no_resolver");
    assert_eq!(w.journaled(), before);
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_exact_replay_is_identical() {
    let (w, b, _) = bound();
    let run = replay_exact(&w.backend.journal_values(), &w.kinds, w.project);
    assert_eq!(run.outcome, ReplayOutcome::Identical);
    let (replayed, d) = run.reports.get(&w.report).unwrap();
    assert_eq!(*d, w.backend.stored(&w.report).unwrap().digest);
    let blk = replayed.pages()[0].blocks().iter().find(|x| x.id() == b).unwrap();
    assert_eq!(blk.payload().value(), &w.payload_of(b));
}

// ------------------------------------------------------------------ AC-7 and AC-8

/// Adds a copy of the AC-1 block whose payload has `pointer` replaced by `new`.
fn tampered(w: &mut World, b: BlockId, pointer: &str, new: CValue) -> BlockId {
    let mut p = w.payload_of(b);
    set_at(&mut p, pointer, new);
    w.add(&p, "table")
}

#[test]
fn ac7_a_replaced_digest_is_refused_without_value() {
    let (mut w, b, cases) = bound();
    let other = CValue::Str(st(&cases, "ac7_other_digest").to_string());
    for which in ["record_digest", "statement_digest"] {
        let t = tampered(&mut w, b, &format!("/rows/0/value/{which}"), other.clone());
        let e = w.resolve(t, st(&cases, "current_head")).unwrap_err();
        assert_eq!(
            e,
            BindError::BindingDigestMismatch { block: t, binding: "/rows/0/value".to_string(), digest: which }
        );
        assert_eq!(e.code(), "report.binding_digest_mismatch");
    }
}

#[test]
fn ac8_a_field_that_is_not_a_value_is_refused() {
    let (mut w, b, cases) = bound();
    for f in arr(get(&cases, "ac8_fields")) {
        let CValue::Str(f) = f else { panic!() };
        let t = tampered(&mut w, b, "/rows/0/value/field", CValue::Str(f.clone()));
        let e = w.resolve(t, st(&cases, "current_head")).unwrap_err();
        assert_eq!(e, BindError::BindingFieldNotValue { block: t, binding: "/rows/0/value".to_string() }, "{f}");
        assert_eq!(e.code(), "report.binding_field_not_value");
    }
}

#[test]
fn a_value_binding_to_a_record_the_block_does_not_reference_is_refused() {
    let (mut w, b, cases) = bound();
    // Drop r2 from the binding's references: row 1 then names a record the block does not
    // reference.
    let mut p = w.payload_of(b);
    let refs = format!(r#"{{"refs":[{{"id":"{}","kind":"result"}}],"schema":"binding@1"}}"#, st(&cases, "r1"));
    set_at(&mut p, "/binding", parse(refs.as_bytes()).unwrap());
    let t = w.add(&p, "table");
    let e = w.resolve(t, st(&cases, "current_head")).unwrap_err();
    assert_eq!(e.code(), "report.value_binding_invalid");
    assert_eq!(st(&e.to_value(), "reason"), "record_not_bound");
}

// ------------------------------------------------------------------ REQ-006 and params

#[test]
fn unknown_references_fail_as_not_found_and_journal_nothing() {
    let (mut w, b, cases) = bracket();
    let before = w.journaled();
    let random = "0190aaaa-bbbb-7ccc-8ddd-eeeeeeeeeeee";
    assert_eq!(w.bind(b, "--results", random).unwrap_err(), BindError::NotFound(random.to_string()));
    // An ID of another kind is not found as this kind.
    let c1 = st(&cases, "characteristic_1");
    assert_eq!(w.bind(b, "--results", c1).unwrap_err(), BindError::NotFound(c1.to_string()));
    assert_eq!(w.bind(b, "--fields", "fai.missing").unwrap_err().code(), "report.not_found");
    // Unknown report or block.
    let (r, rand) = (w.report.uuid().to_string(), random.to_string());
    let e = w.bind_as(HUMAN, MAIN, &["bind-block", "--report", &r, "--block", &rand, "--results", c1]).unwrap_err();
    assert_eq!(e.code(), "report.not_found");
    // Malformed parameters.
    let e = w.bind(b, "--results", &format!("{},{}", st(&cases, "r1"), st(&cases, "r1"))).unwrap_err();
    assert_eq!(e, BindError::BadParams("duplicate ref"));
    assert_eq!(w.bind(b, "--results", "R1").unwrap_err().code(), "report.bad_params");
    assert_eq!(w.bind_as(HUMAN, MAIN, &["bind-block", "--block", &rand]).unwrap_err().code(), "report.bad_params");
    assert_eq!(w.journaled(), before);
}

// ------------------------------------------------------------------ branches

#[test]
fn agents_bind_only_on_their_agent_branch_and_people_never_there() {
    let (mut w, b, cases) = bracket();
    let (r, bs) = (w.report.uuid().to_string(), b.uuid().to_string());
    let args = ["bind-block", "--report", &r, "--block", &bs, "--results", st(&cases, "r1"), "--json"];
    let agent = Caller { principal: Principal::Agent, surface: Surface::Cli };
    let mcp_human = Caller { principal: Principal::Human, surface: Surface::Mcp };
    let before = w.journaled();
    for (caller, branch) in [(agent, MAIN), (mcp_human, MAIN), (HUMAN, "agent/claude/1"), (agent, "agent/Claude/1")] {
        assert_eq!(w.bind_as(caller, branch, &args).unwrap_err().code(), "report.policy", "{caller:?} {branch}");
    }
    assert_eq!(w.journaled(), before);
    w.bind_as(agent, "agent/claude/1", &args).unwrap();
    w.bind_as(mcp_human, "agent/claude-code/12", &args).unwrap();
    assert_eq!(w.journaled(), before + 2);
    for (b, ok) in [
        ("agent/a/1", true),
        ("agent/a-1/20", true),
        ("agent/a/0", false),
        ("agent/a/01", false),
        ("agent//1", false),
        ("agent/a/1/x", false),
        ("main", false),
    ] {
        assert_eq!(is_agent_branch(b), ok, "{b}");
    }
}

// ------------------------------------------------------------------ declaration

#[test]
fn the_command_is_declared_with_the_registry_macros() {
    let decls = declarations();
    assert_eq!(decls.len(), 1);
    let d = &decls[0].def;
    assert_eq!((d.id.as_str(), d.major), ("report.bind_block", 1));
    assert_eq!(d.side_effect, SideEffect::W2);
    assert_eq!(d.effects, [BIND_BLOCK]);
    assert_eq!(fid_registry::tool_name(&d.id, d.major), MCP_BIND);
    let catalogue = Catalogue::geo_frame().unwrap();
    for schema in [&d.params_schema, &d.result_schema] {
        assert_eq!(check_schema(schema, &catalogue), Vec::new());
    }
    let raw: Vec<_> = decls.iter().map(|d| d.to_validation()).collect();
    assert_eq!(validate(&raw), Vec::new());
    // The parameter schema has no place for a value: a reference is exactly {kind, id}.
    let params = canon(&d.params_schema);
    assert!(params.contains(r#""additionalProperties":false"#));
    let r#ref = get(get(get(get(&d.params_schema, "properties"), "refs"), "items"), "properties");
    let CValue::Obj(o) = r#ref else { panic!() };
    assert_eq!(o.keys().map(String::as_str).collect::<Vec<_>>(), ["id", "kind"]);
}

/// A hostile `text` resolver: it tries to bring a value in through its layout, or to read
/// a record that is not at the head through the lookup it is given.
struct Evil {
    record: Uuid,
    layout: &'static str,
}

impl BlockResolver for Evil {
    fn kind(&self) -> BlockKindId {
        BlockKindId::new("text").unwrap()
    }

    fn place(
        &self,
        _p: &fid_report::doc::CanonicalJson,
        _b: &Binding,
        _v: &[ValueBinding],
    ) -> Result<CValue, fid_report::doc::PayloadError> {
        Ok(CValue::Obj(BTreeMap::new()))
    }

    fn resolve(&self, _view: &Resolved, _ctx: &ResolveContext<'_>, lookup: &dyn Lookup) -> Result<CValue, BindError> {
        match lookup.result(&self.record) {
            Err(LookupFault::Unjournaled) => Err(BindError::BadParams("lookup refused the record")),
            Err(_) => Err(BindError::BadParams("other fault")),
            Ok(_) => Ok(parse(self.layout.as_bytes()).unwrap()),
        }
    }
}

#[test]
fn a_resolver_cannot_bring_in_a_value_or_a_record_not_at_the_head() {
    let (mut w, _, cases) = bracket();
    let text = w.add(&parse(br#"{"text":"notes"}"#).unwrap(), "text");
    let r2 = uuid(st(&cases, "r2"));
    for (layout, reason) in [
        (r#"{"leak":12.003}"#, "number"),
        (r#"{"leak":"0x3f847ae147ae147b"}"#, "bit_pattern"),
        (r#"{"leak":["ok","12.003 mm"]}"#, "number_text"),
    ] {
        let mut reg = ResolverRegistry::new();
        reg.register(Box::new(Evil { record: r2, layout })).unwrap();
        w.resolvers = reg;
        // Before r2 is journaled, the lookup the resolver gets refuses it.
        let e = w.resolve(text, st(&cases, "h0")).unwrap_err();
        assert_eq!(e, BindError::BadParams("lookup refused the record"));
        // At the head, whatever the resolver puts in its layout is checked by the engine.
        let e = w.resolve(text, st(&cases, "current_head")).unwrap_err();
        assert_eq!(e.code(), "report.binding_has_value");
        assert_eq!(st(&e.to_value(), "reason"), reason, "{layout}");
    }
    // A layout without values passes; the view is still the engine's (no values here).
    let mut reg = ResolverRegistry::new();
    reg.register(Box::new(Evil { record: r2, layout: r#"{"caption":"C01"}"# })).unwrap();
    w.resolvers = reg;
    let ok = w.resolve(text, st(&cases, "current_head")).unwrap();
    assert!(ok.view().values().is_empty());
}

#[test]
fn resolution_reads_the_record_bytes_at_the_head() {
    let (mut w, b, cases) = bound();
    let at_bind = st(&cases, "current_head").to_string();
    // A later version of r1 (another measurand) is committed after the bind.
    let journal = fixture("bracket_3stage/journal.json");
    let r1 = st(&cases, "r1");
    let old = arr(get(&journal, "entries")).iter().find(|e| matches!(get(e, "id"), CValue::Str(s) if s == r1)).unwrap();
    let new_text = st(old, "text").replace("two-point size", "two-point size, re-evaluated");
    let later = format!("sha256:{}", "9".repeat(64));
    let project = uuid(st(&cases, "project"));
    w.journal
        .append(JournalHead::parse(&later).unwrap(), project, EntityKind::Result, Some(uuid(r1)), new_text.into_bytes())
        .unwrap();
    // At the bind head the binding resolves; at the later head r1's stored bytes differ.
    w.resolve(b, &at_bind).unwrap();
    let e = w.resolve(b, &later).unwrap_err();
    assert_eq!(
        e,
        BindError::BindingDigestMismatch { block: b, binding: "/rows/0/value".to_string(), digest: "record_digest" }
    );
}

// ------------------------------------------------------------------ schema checker

/// A checker for the JSON Schema subset the declared result schema uses. An unknown keyword
/// or pattern fails the test, so a schema change must extend this checker on purpose.
fn conforms(schema: &CValue, v: &CValue, at: &str) -> Result<(), String> {
    let CValue::Obj(s) = schema else { return Err(format!("{at}: schema is not an object")) };
    for (k, kv) in s {
        let ok = match (k.as_str(), kv) {
            ("description" | "title" | "$schema", _) => true,
            ("type", CValue::Str(t)) => matches!(
                (t.as_str(), v),
                ("object", CValue::Obj(_)) | ("array", CValue::Arr(_)) | ("string", CValue::Str(_))
            ),
            ("enum", CValue::Arr(e)) => e.contains(v),
            ("pattern", CValue::Str(p)) => {
                let CValue::Str(x) = v else { return Err(format!("{at}: pattern on a non-string")) };
                matches_pattern(p, x)?
            }
            ("maxLength" | "maxItems", _) => true,
            ("required", CValue::Arr(r)) => {
                let CValue::Obj(o) = v else { return Err(format!("{at}: required on a non-object")) };
                r.iter().all(|k| matches!(k, CValue::Str(k) if o.contains_key(k)))
            }
            ("additionalProperties", CValue::Bool(false)) => {
                let (CValue::Obj(o), Some(CValue::Obj(props))) = (v, s.get("properties")) else {
                    return Err(format!("{at}: additionalProperties without properties"));
                };
                o.keys().all(|k| props.contains_key(k))
            }
            ("properties", CValue::Obj(props)) => {
                if let CValue::Obj(o) = v {
                    for (pk, ps) in props {
                        if let Some(pv) = o.get(pk) {
                            conforms(ps, pv, &format!("{at}/{pk}"))?;
                        }
                    }
                }
                true
            }
            ("items", item) => {
                if let CValue::Arr(a) = v {
                    for (i, x) in a.iter().enumerate() {
                        conforms(item, x, &format!("{at}/{i}"))?;
                    }
                }
                true
            }
            (other, _) => return Err(format!("{at}: unknown keyword {other}")),
        };
        if !ok {
            return Err(format!("{at}: fails {k}"));
        }
    }
    Ok(())
}

/// The three patterns the schemas use, checked by hand.
fn matches_pattern(p: &str, x: &str) -> Result<bool, String> {
    let hex = |s: &str, n: usize| s.len() == n && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c));
    let is_uuid = |s: &str| {
        let parts: Vec<&str> = s.split('-').collect();
        parts.len() == 5 && [8, 4, 4, 4, 12].iter().zip(&parts).all(|(n, p)| hex(p, *n))
    };
    let is_field = |s: &str| {
        !s.is_empty()
            && s.len() <= 64
            && s.as_bytes()[0].is_ascii_lowercase()
            && s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'.')
    };
    match p {
        "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$" => Ok(is_uuid(x)),
        "^sha256:[0-9a-f]{64}$" => Ok(x.strip_prefix("sha256:").is_some_and(|h| hex(h, 64))),
        "^([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|[a-z][a-z0-9_.]{0,63})$" => {
            Ok(is_uuid(x) || is_field(x))
        }
        other => Err(format!("unknown pattern {other}")),
    }
}

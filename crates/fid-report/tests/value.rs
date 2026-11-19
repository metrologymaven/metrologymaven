#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The value rule of RPT.MODEL.DOC (reviews of #232): declared value fields hold
//! `value_binding@1` references only, never literal values; escaping covers every Cf and
//! default-ignorable character; duplicate keys are refused; replay checks the project.
//! Expected outcomes and bytes are hand-labelled by fixtures/rpt/model/doc/make_fixtures.py
//! (binding_cases.json, binding-report-text.json).

use fid_canon::{digest_bytes, parse, to_canonical_bytes, CValue};
use fid_project::{Bits, IdError, ObservationSource, Uuid};
use fid_replay::ReplayOutcome;
use fid_report::doc::stub::{ImageStub, TextStub};
use fid_report::doc::*;
use std::path::PathBuf;

const PROJECT: &str = "019a9a9a-0000-7000-8000-00000000c0de";

/// The deterministic ID source of make_fixtures.py: draw k gives ms BASE + k, R = k + 1.
struct Counter(u64);

impl ObservationSource for Counter {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        Ok(1_763_510_400_000 + self.0)
    }

    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        let b = Bits::new(n, u128::from(self.0) + 1)?;
        self.0 += 1;
        Ok(b)
    }
}

fn repo(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(repo(rel)).unwrap()
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap(),
        _ => panic!("not an object"),
    }
}

fn str_of(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("not a string"),
    }
}

fn arr(v: &CValue) -> &Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("not an array"),
    }
}

/// A permissive value kind: accepts any payload and declares `/U` as its value field. The
/// value rule alone must keep literal values out.
struct Measurement;

impl BlockKind for Measurement {
    fn id(&self) -> BlockKindId {
        BlockKindId::new("measurement").unwrap()
    }

    fn version(&self) -> u32 {
        1
    }

    fn validate(&self, _payload: &CanonicalJson) -> Result<(), PayloadError> {
        Ok(())
    }

    fn value_fields(&self, _payload: &CanonicalJson) -> Result<Vec<String>, PayloadError> {
        Ok(vec!["/U".to_string()])
    }
}

/// A kind that declares a malformed value-field pointer.
struct BadPointer;

impl BlockKind for BadPointer {
    fn id(&self) -> BlockKindId {
        BlockKindId::new("bad_pointer").unwrap()
    }

    fn version(&self) -> u32 {
        1
    }

    fn validate(&self, _payload: &CanonicalJson) -> Result<(), PayloadError> {
        Ok(())
    }

    fn value_fields(&self, _payload: &CanonicalJson) -> Result<Vec<String>, PayloadError> {
        Ok(vec!["U".to_string()])
    }
}

fn kinds() -> KindRegistry {
    let mut k = KindRegistry::new();
    k.register(Box::new(TextStub)).unwrap();
    k.register(Box::new(ImageStub)).unwrap();
    k.register(Box::new(Measurement)).unwrap();
    k.register(Box::new(BadPointer)).unwrap();
    k
}

fn binding() -> CValue {
    let cases = parse(&read("fixtures/rpt/model/doc/binding_cases.json")).unwrap();
    get(&arr(get(&cases, "cases"))[0], "value").clone()
}

struct World {
    backend: MemoryBackend,
    kinds: KindRegistry,
    ids: Counter,
    report: String,
    page: String,
}

impl World {
    fn new() -> World {
        let mut backend = MemoryBackend::new(Uuid::parse(PROJECT).unwrap());
        let kinds = kinds();
        let mut ids = Counter(0);
        let p = obj([("title", s("values")), ("project", s(PROJECT))]);
        let out = execute(CREATE_REPORT, &p, &mut backend, &kinds, &mut ids).unwrap();
        let report = str_of(get(&out, "report_id")).to_string();
        let page = str_of(get(&arr(get(&out, "pages"))[0], "page_id")).to_string();
        World { backend, kinds, ids, report, page }
    }

    fn add(&mut self, kind: &str, payload: CValue) -> Result<CValue, ReportError> {
        let p = obj([("report", s(&self.report)), ("page", s(&self.page)), ("kind", s(kind)), ("payload", payload)]);
        execute(ADD_BLOCK, &p, &mut self.backend, &self.kinds, &mut self.ids)
    }

    fn rid(&self) -> ReportId {
        ReportId::new(Uuid::parse(&self.report).unwrap()).unwrap()
    }
}

#[test]
fn binding_cases_match_hand_labels_in_add_block() {
    let cases = parse(&read("fixtures/rpt/model/doc/binding_cases.json")).unwrap();
    let mut n = 0;
    for case in arr(get(&cases, "cases")) {
        let name = str_of(get(case, "name"));
        let expect = str_of(get(case, "expect"));
        let mut w = World::new();
        let before = w.backend.clone();
        match w.add("measurement", obj([("U", get(case, "value").clone())])) {
            Ok(_) => {
                assert_eq!(expect, "ok", "{name}");
                assert_eq!(w.backend.journal().len(), 2, "{name}");
            }
            Err(e) => {
                assert_eq!(e.code(), expect, "{name}");
                assert_eq!(get(&e.to_value(), "field"), &s("/U"), "{name}");
                assert_eq!(get(&e.to_value(), "reason"), &s(str_of(get(case, "reason"))), "{name}");
                assert_eq!(w.backend, before, "{name}: journal and store unchanged");
            }
        }
        n += 1;
    }
    assert_eq!(n, 26);
}

#[test]
fn literal_value_text_and_hand_typed_numbers_are_refused() {
    let mut w = World::new();
    let text = obj([
        ("schema", s("value_text@1")),
        ("canonical", s("0.002")),
        ("digits_added", s("0")),
        ("formatter", s("fmt.length@1")),
        ("unit", s("mm")),
        ("markers", CValue::Arr(Vec::new())),
        ("labels", CValue::Arr(Vec::new())),
    ]);
    for literal in [text, s("0.002"), CValue::Num(0.002), CValue::U64(2), s("-1.5e-3")] {
        let e = w.add("measurement", obj([("U", literal)])).unwrap_err();
        assert_eq!(e.code(), "report.value_literal_refused");
    }
    // A missing value field and a kind with a malformed pointer.
    let e = w.add("measurement", obj([("V", binding())])).unwrap_err();
    assert_eq!((e.code(), get(&e.to_value(), "reason")), ("report.value_binding_invalid", &s("missing")));
    let e = w.add("bad_pointer", obj([("U", binding())])).unwrap_err();
    assert_eq!(e.code(), "report.invalid_block_payload");
    assert_eq!(w.backend.journal().len(), 1);
}

#[test]
fn well_formed_binding_is_stored_canonically() {
    let mut w = World::new();
    w.add("measurement", obj([("U", binding())])).unwrap();
    let stored = w.backend.stored(&w.rid()).unwrap().clone();
    assert_eq!(stored.canonical, read("fixtures/rpt/model/doc/binding-report-text.json"));
    // Read back and replayed unchanged.
    let shown = execute(SHOW_REPORT, &obj([("report", s(&w.report))]), &mut w.backend, &w.kinds, &mut w.ids).unwrap();
    assert_eq!(to_canonical_bytes(&shown).unwrap(), stored.canonical);
    let run = replay_exact(&w.backend.journal_values(), &w.kinds, Uuid::parse(PROJECT).unwrap());
    assert_eq!(run.outcome, ReplayOutcome::Identical);
}

#[test]
fn stored_reports_and_replay_refuse_a_literal_value() {
    let mut w = World::new();
    w.add("measurement", obj([("U", binding())])).unwrap();
    let id = w.rid();
    let stored = w.backend.stored(&id).unwrap().clone();

    // Replace the binding with a literal in the stored bytes and fix up the digest.
    let text = String::from_utf8(stored.canonical.clone()).unwrap();
    let b = String::from_utf8(to_canonical_bytes(&binding()).unwrap()).unwrap();
    let forged = text.replace(&b, "\"0.002\"");
    assert_ne!(forged, text);
    let bytes = forged.into_bytes();
    let digest = fid_types::Digest::from_bytes(digest_bytes(&bytes).0);
    let tampered = StoredReport { canonical: bytes.clone(), digest };
    assert_eq!(tampered.open(&id, &w.kinds).unwrap_err().code(), "report.value_literal_refused");
    assert_eq!(Report::from_canonical(&bytes, &w.kinds).unwrap_err().code(), "report.value_literal_refused");

    // Without the kind registered the stored report is not read at all.
    let mut text_only = KindRegistry::new();
    text_only.register(Box::new(TextStub)).unwrap();
    assert_eq!(stored.open(&id, &text_only).unwrap_err().code(), "report.unknown_block_kind");

    // The same forgery in the journal: exact replay is DIVERGENT at that event.
    let mut events = w.backend.journal_values();
    if let CValue::Obj(o) = &mut events[1] {
        o.insert("payload".into(), obj([("untrusted", CValue::Bool(true)), ("value", obj([("U", s("0.002"))]))]));
    }
    let run = replay_exact(&events, &w.kinds, Uuid::parse(PROJECT).unwrap());
    assert_eq!((run.outcome, run.first_mismatch), (ReplayOutcome::Divergent, Some(1)));
}

#[test]
fn replay_checks_the_project_of_create_report() {
    let w = World::new();
    let events = w.backend.journal_values();
    let ours = replay_exact(&events, &w.kinds, Uuid::parse(PROJECT).unwrap());
    assert_eq!(ours.outcome, ReplayOutcome::Identical);
    let other = Uuid::parse("019a9a9a-0000-7000-8000-0000000000ff").unwrap();
    let run = replay_exact(&events, &w.kinds, other);
    assert_eq!((run.outcome, run.first_mismatch), (ReplayOutcome::Divergent, Some(0)));
    assert!(run.reports.is_empty());
}

#[test]
fn duplicate_keys_are_store_invalid() {
    let w = World::new();
    let id = w.rid();
    let good = w.backend.stored(&id).unwrap().clone();
    // A stored document with `"schema_version"` twice.
    let text = String::from_utf8(good.canonical.clone()).unwrap();
    let dup = text.replacen(r#""schema_version":"1""#, r#""schema_version":"1","schema_version":"1""#, 1);
    assert_ne!(dup, text);
    let bytes = dup.into_bytes();
    let digest = fid_types::Digest::from_bytes(digest_bytes(&bytes).0);
    let e = StoredReport { canonical: bytes.clone(), digest }.open(&id, &w.kinds).unwrap_err();
    assert_eq!(e.code(), "report.store_invalid");
    assert_eq!(Report::from_canonical(&bytes, &w.kinds).unwrap_err().code(), "report.store_invalid");
    // A journal event with `"page"` twice.
    let ev = to_canonical_bytes(&w.backend.journal_values()[0]).unwrap();
    assert!(ReportEvent::from_json(&ev).is_ok());
    let ev = String::from_utf8(ev).unwrap();
    let page = format!(r#""page":"{}""#, w.page);
    let dup = ev.replacen(&page, &format!("{page},{page}"), 1);
    assert_ne!(dup, ev);
    assert_eq!(ReportEvent::from_json(dup.as_bytes()).unwrap_err().code(), "report.store_invalid");
}

#[test]
fn must_escape_covers_every_cf_and_default_ignorable_code_point() {
    // Characters the first version missed (review of #232), and a sample of the rest.
    let missed = [
        0x034F, 0x115F, 0x1160, 0x17B4, 0x180B, 0x180E, 0x206A, 0x206F, 0x3164, 0xFE00, 0xFE0F, 0xFFA0, 0xFFF0, 0x0600,
        0x06DD, 0x070F, 0x0890, 0x08E2, 0x110BD, 0x13430, 0x1BCA0, 0x1D173, 0xE0001, 0xE0020, 0xE007F, 0xE0100,
        0xE0FFF,
    ];
    for u in missed {
        let c = char::from_u32(u).unwrap();
        assert!(must_escape(c), "U+{u:04X}");
        let t = Untrusted::text(&format!("a{c}b")).unwrap();
        assert_eq!(t.escaped(), format!("a\\u{{{u:04X}}}b"));
    }
    for c in ['a', 'Z', '0', ' ', '\u{e9}', '\u{3b1}', '\u{4e2d}', '\u{1f600}', '\u{fffd}'] {
        assert!(!must_escape(c), "{c:?}");
    }
}

/// The copied tables must stay byte-identical to fid-bus's generated ones.
#[test]
fn unicode_tables_equal_the_fid_bus_copy() {
    let bus = String::from_utf8(read("crates/fid-bus/src/unicode16.rs")).unwrap();
    let ours = String::from_utf8(read("crates/fid-report/src/doc/unicode16.rs")).unwrap();
    for name in ["FORMAT_RANGES", "DEFAULT_IGNORABLE_RANGES"] {
        let table = |src: &str| {
            let start = src.find(&format!("pub const {name}")).unwrap();
            let end = start + src[start..].find("];").unwrap();
            src[start..end].to_string()
        };
        assert_eq!(table(&bus), table(&ours), "{name}");
    }
}

#[test]
fn pointers_resolve_per_rfc_6901() {
    let v = obj([("a/b", obj([("~c", CValue::Arr(vec![s("x"), s("y")]))]))]);
    assert_eq!(resolve_pointer(&v, "/a~1b/~0c/1"), Some(&s("y")));
    assert_eq!(resolve_pointer(&v, ""), Some(&v));
    for bad in ["a", "/a~1b/~0c/01", "/a~1b/~0c/2", "/a~2b", "/a~1b/~0c/-"] {
        assert_eq!(resolve_pointer(&v, bad), None, "{bad}");
    }
}

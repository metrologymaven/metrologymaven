#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The value rule of RPT.MODEL.DOC (review of #232): claimed values carry labels, value
//! fields hold FMT's `value_text@1`, escaping covers every Cf and default-ignorable
//! character, duplicate keys are refused, and replay checks the project.
//! Expected outcomes are hand-labelled in fixtures/rpt/model/doc/value_cases.json and
//! fixtures/unc/stmt/valid (UNC.UEC.STMT).

use fid_canon::{digest_bytes, parse, to_canonical_bytes, CValue};
use fid_kunc::UncertaintyStatement;
use fid_project::{Bits, IdError, ObservationSource, Uuid};
use fid_replay::ReplayOutcome;
use fid_report::doc::stub::{ImageStub, TextStub};
use fid_report::doc::*;
use std::collections::BTreeSet;
use std::path::PathBuf;

const PROJECT: &str = "019a9a9a-0000-7000-8000-00000000c0de";

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

fn statement(name: &str) -> String {
    String::from_utf8(read(&format!("fixtures/unc/stmt/valid/{name}.json"))).unwrap()
}

/// A permissive value kind: accepts any object payload, declares `/U` as its value field
/// and computes required labels from the uncertainty statement held in `/statement`
/// (canonical statement JSON, as text).
struct Permissive;

impl BlockKind for Permissive {
    fn id(&self) -> BlockKindId {
        BlockKindId::new("measurement").unwrap()
    }

    fn version(&self) -> u32 {
        1
    }

    fn validate(&self, _payload: &CanonicalJson) -> Result<(), PayloadError> {
        Ok(())
    }

    fn required_labels(&self, payload: &CanonicalJson) -> Result<BTreeSet<ValueLabel>, PayloadError> {
        let Some(CValue::Str(stmt)) = resolve_pointer(payload.value(), "/statement") else {
            return Err(PayloadError::new("/statement", "missing"));
        };
        let stmt = UncertaintyStatement::parse_canonical(stmt.as_bytes())
            .map_err(|_| PayloadError::new("/statement", "not_statement"))?;
        Ok(ValueLabel::required_by(&stmt))
    }

    fn value_fields(&self, _payload: &CanonicalJson) -> Result<Vec<String>, PayloadError> {
        Ok(vec!["/U".to_string()])
    }
}

fn kinds() -> KindRegistry {
    let mut k = KindRegistry::new();
    k.register(Box::new(TextStub)).unwrap();
    k.register(Box::new(ImageStub)).unwrap();
    k.register(Box::new(Permissive)).unwrap();
    k
}

fn value_text(labels: &[CValue]) -> CValue {
    obj([
        ("schema", s("value_text@1")),
        ("canonical", s("0.002")),
        ("digits_added", s("0")),
        ("formatter", s("fmt.length@1")),
        ("unit", s("mm")),
        ("markers", CValue::Arr(Vec::new())),
        ("labels", CValue::Arr(labels.to_vec())),
    ])
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

    fn add(&mut self, payload: CValue, labels: &[&str]) -> Result<CValue, ReportError> {
        let p = obj([
            ("report", s(&self.report)),
            ("page", s(&self.page)),
            ("kind", s("measurement")),
            ("labels", CValue::Arr(labels.iter().map(|l| s(l)).collect())),
            ("payload", payload),
        ]);
        execute(ADD_BLOCK, &p, &mut self.backend, &self.kinds, &mut self.ids)
    }
}

fn labels(names: &[&str]) -> BTreeSet<ValueLabel> {
    names.iter().map(|n| ValueLabel::parse(n).unwrap()).collect()
}

#[test]
fn required_by_follows_fid_kunc() {
    let cases = [
        ("uec1_declared", &["claimed", "declared_input", "uncharacterized_input"][..]),
        ("uec1_spec_derived", &["claimed", "uncharacterized_input"][..]),
        ("uec1_mcm", &["declared_input", "uncharacterized_input"][..]),
        ("uec2_lpu", &[][..]),
        ("uec3_iso15530_3", &[][..]),
    ];
    for (name, want) in cases {
        let stmt = UncertaintyStatement::parse_canonical(statement(name).as_bytes()).unwrap();
        assert_eq!(ValueLabel::required_by(&stmt), labels(want), "{name}");
    }
}

#[test]
fn permissive_value_kind_cannot_store_an_unlabelled_claimed_value() {
    let mut w = World::new();
    let declared = statement("uec1_declared");
    let before = w.backend.clone();

    // The review's case: a bare `{"U": "0.002"}` from a claimed statement.
    let e = w.add(obj([("U", s("0.002")), ("statement", s(&declared))]), &[]).unwrap_err();
    assert_eq!(e.code(), "report.value_text_invalid");
    assert_eq!(get(&e.to_value(), "field"), &s("/U"));
    assert_eq!(get(&e.to_value(), "reason"), &s("not_value_text"));

    // FMT-shaped, but unlabelled: refused, naming every missing label.
    let e = w.add(obj([("U", value_text(&[])), ("statement", s(&declared))]), &[]).unwrap_err();
    assert_eq!(
        e,
        ReportError::ValueLabelMismatch {
            missing: labels(&["claimed", "declared_input", "uncharacterized_input"]),
            extra: BTreeSet::new()
        }
    );
    assert_eq!(e.code(), "report.value_label_mismatch");
    assert_eq!(
        get(&e.to_value(), "missing"),
        &CValue::Arr(vec![s("claimed"), s("declared_input"), s("uncharacterized_input")])
    );

    // Partly labelled: still refused.
    let e = w.add(obj([("U", value_text(&[])), ("statement", s(&declared))]), &["claimed"]).unwrap_err();
    assert_eq!(e.code(), "report.value_label_mismatch");
    assert_eq!(w.backend, before);

    // Fully labelled: stored, and shown with its labels.
    let all = ["claimed", "declared_input", "uncharacterized_input"];
    w.add(obj([("U", value_text(&[])), ("statement", s(&declared))]), &all).unwrap();
    let shown = execute(SHOW_REPORT, &obj([("report", s(&w.report))]), &mut w.backend, &w.kinds, &mut w.ids).unwrap();
    let block = &arr(get(&arr(get(&shown, "pages"))[0], "blocks"))[0];
    assert_eq!(get(block, "labels"), &CValue::Arr(all.iter().map(|l| s(l)).collect()));
}

#[test]
fn labels_must_be_exact_and_value_text_labels_count() {
    let mut w = World::new();
    let evaluated = statement("uec2_lpu");
    // A measured value may not be labelled claimed.
    let e = w.add(obj([("U", value_text(&[])), ("statement", s(&evaluated))]), &["claimed"]).unwrap_err();
    assert_eq!(e, ReportError::ValueLabelMismatch { missing: BTreeSet::new(), extra: labels(&["claimed"]) });
    // A value text that says claimed makes the block require the label.
    let claimed_text = value_text(&[obj([("kind", s("claimed"))])]);
    let e = w.add(obj([("U", claimed_text.clone()), ("statement", s(&evaluated))]), &[]).unwrap_err();
    assert_eq!(e, ReportError::ValueLabelMismatch { missing: labels(&["claimed"]), extra: BTreeSet::new() });
    w.add(obj([("U", claimed_text), ("statement", s(&evaluated))]), &["claimed"]).unwrap();
    // Missing value field, unknown or unsorted label names.
    let e = w.add(obj([("statement", s(&evaluated))]), &[]).unwrap_err();
    assert_eq!((e.code(), get(&e.to_value(), "reason")), ("report.value_text_invalid", &s("missing")));
    assert_eq!(w.add(obj([("U", value_text(&[]))]), &["measured"]).unwrap_err().code(), "report.bad_params");
    let unsorted = w.add(obj([("U", value_text(&[]))]), &["declared_input", "claimed"]);
    assert_eq!(unsorted.unwrap_err().code(), "report.bad_params");
}

#[test]
fn value_text_cases_match_hand_labels() {
    let cases = parse(&read("fixtures/rpt/model/doc/value_cases.json")).unwrap();
    let mut n = 0;
    for case in arr(get(&cases, "cases")) {
        let name = str_of(get(case, "name"));
        let expect = str_of(get(case, "expect"));
        let want: Vec<&str> = arr(get(case, "labels")).iter().map(str_of).collect();
        match check_value_text(get(case, "value")) {
            Ok(found) => {
                assert_eq!(expect, "ok", "{name}");
                assert_eq!(found, labels(&want), "{name}");
            }
            Err(e) => assert_eq!(e.reason(), expect, "{name}"),
        }
        n += 1;
    }
    assert_eq!(n, 27);
}

#[test]
fn stored_reports_and_replay_refuse_broken_labels() {
    let mut w = World::new();
    let declared = statement("uec1_declared");
    let all = ["claimed", "declared_input", "uncharacterized_input"];
    w.add(obj([("U", value_text(&[])), ("statement", s(&declared))]), &all).unwrap();
    let id = ReportId::new(Uuid::parse(&w.report).unwrap()).unwrap();
    let stored = w.backend.stored(&id).unwrap().clone();

    // Strip the labels from the stored bytes and fix up the digest: the reader refuses it.
    let text = String::from_utf8(stored.canonical.clone()).unwrap();
    let stripped = text.replace(r#""labels":["claimed","declared_input","uncharacterized_input"]"#, r#""labels":[]"#);
    assert_ne!(stripped, text);
    let bytes = stripped.into_bytes();
    let digest = fid_types::Digest::from_bytes(digest_bytes(&bytes).0);
    let tampered = StoredReport { canonical: bytes.clone(), digest };
    assert_eq!(tampered.open(&id, &w.kinds).unwrap_err().code(), "report.value_label_mismatch");
    assert_eq!(Report::from_canonical(&bytes, &w.kinds).unwrap_err().code(), "report.value_label_mismatch");

    // Without the kind registered the stored report is not read at all.
    let mut text_only = KindRegistry::new();
    text_only.register(Box::new(TextStub)).unwrap();
    assert_eq!(stored.open(&id, &text_only).unwrap_err().code(), "report.unknown_block_kind");

    // The same edit in the journal: exact replay is DIVERGENT at that event.
    let mut events = w.backend.journal_values();
    assert_eq!(replay_exact(&events, &w.kinds, Uuid::parse(PROJECT).unwrap()).outcome, ReplayOutcome::Identical);
    if let CValue::Obj(o) = &mut events[1] {
        o.insert("labels".into(), CValue::Arr(Vec::new()));
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
    let id = ReportId::new(Uuid::parse(&w.report).unwrap()).unwrap();
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

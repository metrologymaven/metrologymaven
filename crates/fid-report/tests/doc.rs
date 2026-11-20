#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::disallowed_types)]
//! Acceptance tests of RPT.MODEL.DOC (AC-1..AC-7) and the pinned Design rules. Expected
//! bytes and digests come from fixtures/rpt/model/doc/make_fixtures.py, never from this crate.

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_project::{Bits, IdError, ObservationSource, Uuid, V7_RANDOM_BITS};
use fid_replay::ReplayOutcome;
use fid_report::doc::stub::{ImageStub, TextStub};
use fid_report::doc::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

const BASE_MS: u64 = 1_763_510_400_000;
const IMAGE_BLOB: &str = "sha256:abababababababababababababababababababababababababababababababab";

/// The deterministic ID source of make_fixtures.py: draw k gives ms BASE_MS + k, R = k + 1.
struct Counter {
    next: u64,
}

impl Counter {
    fn new() -> Counter {
        Counter { next: 0 }
    }
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

/// Always returns the same observation: every UUIDv7 it yields is the same ID.
struct Stuck;

impl ObservationSource for Stuck {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        Ok(BASE_MS)
    }

    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        Bits::new(n, 7)
    }
}

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/model").join(rel)
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(fixture(rel)).unwrap()
}

fn json(rel: &str) -> CValue {
    parse(&read(rel)).unwrap()
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

fn arr(v: &CValue) -> &Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("not an array"),
    }
}

fn str_of(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("not a string"),
    }
}

fn uuid_of(v: &CValue) -> Uuid {
    Uuid::parse(str_of(v)).unwrap()
}

fn kinds() -> KindRegistry {
    let mut k = KindRegistry::new();
    k.register(Box::new(TextStub)).unwrap();
    k.register(Box::new(ImageStub)).unwrap();
    k
}

fn project_id() -> Uuid {
    uuid_of(get(&json("empty_project/project.json"), "project"))
}

struct World {
    backend: MemoryBackend,
    kinds: KindRegistry,
    ids: Counter,
}

impl World {
    /// Fixture `empty_project`: one part, revision and plan, no results, no reports.
    fn empty() -> World {
        let p = json("empty_project/project.json");
        assert!(arr(get(&p, "results")).is_empty());
        assert!(arr(get(&p, "reports")).is_empty());
        World { backend: MemoryBackend::new(project_id()), kinds: kinds(), ids: Counter::new() }
    }

    fn run(&mut self, command: &str, params: CValue) -> Result<CValue, ReportError> {
        execute(command, &params, &mut self.backend, &self.kinds, &mut self.ids)
    }

    fn create(&mut self, title: &str) -> Result<CValue, ReportError> {
        self.run(CREATE_REPORT, obj([("title", s(title)), ("project", s(&project_id().to_string()))]))
    }

    fn add_text(&mut self, report: &str, page: &str, text: &str) -> Result<CValue, ReportError> {
        self.run(
            ADD_BLOCK,
            obj([("report", s(report)), ("page", s(page)), ("kind", s("text")), ("payload", obj([("text", s(text))]))]),
        )
    }

    fn show(&mut self, report: &str) -> CValue {
        self.run(SHOW_REPORT, obj([("report", s(report))])).unwrap()
    }
}

/// AC-1 then AC-2. Returns the world, the report ID and the two page IDs.
fn ac2_world() -> (World, String, String, String) {
    let mut w = World::empty();
    let summary = w.create("Bracket FAI").unwrap();
    let rid = str_of(get(&summary, "report_id")).to_string();
    let p1 = str_of(get(&arr(get(&summary, "pages"))[0], "page_id")).to_string();
    for t in ["Block 1", "Block 2", "Block 3"] {
        w.add_text(&rid, &p1, t).unwrap();
    }
    let p2 = w.run(ADD_PAGE, obj([("report", s(&rid))])).unwrap();
    let p2 = str_of(get(&p2, "page_id")).to_string();
    w.run(
        ADD_BLOCK,
        obj([("report", s(&rid)), ("page", s(&p2)), ("kind", s("image")), ("payload", obj([("blob", s(IMAGE_BLOB))]))]),
    )
    .unwrap();
    (w, rid, p1, p2)
}

fn rid_of(text: &str) -> ReportId {
    ReportId::new(Uuid::parse(text).unwrap()).unwrap()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_create_report_gives_uuidv7_ids_one_empty_page_and_one_event() {
    let mut w = World::empty();
    let out = w.create("Bracket FAI").unwrap();
    assert!(is_v7(&uuid_of(get(&out, "report_id"))));
    let pages = arr(get(&out, "pages"));
    assert_eq!(pages.len(), 1);
    assert!(is_v7(&uuid_of(get(&pages[0], "page_id"))));
    assert_eq!(get(&pages[0], "block_count"), &CValue::U64(0));
    assert_eq!(w.backend.journal().len(), 1);
    assert_eq!(w.backend.journal()[0].event_type(), "report.create_report@1");
    // Hand-labelled summary bytes.
    assert_eq!(to_canonical_bytes(&out).unwrap(), read("doc/ac1-summary-text.json"));
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_pages_and_blocks_in_insertion_order_and_reload_is_identical() {
    let (w, rid, p1, p2) = ac2_world();
    let mut w = w;
    let shown = w.show(&rid);
    let pages = arr(get(&shown, "pages"));
    assert_eq!(pages.len(), 2);
    assert_eq!(str_of(get(&pages[0], "id")), p1);
    assert_eq!(str_of(get(&pages[1], "id")), p2);
    let texts: Vec<&str> =
        arr(get(&pages[0], "blocks")).iter().map(|b| str_of(get(get(get(b, "payload"), "value"), "text"))).collect();
    assert_eq!(texts, ["Block 1", "Block 2", "Block 3"]);
    assert_eq!(arr(get(&pages[1], "blocks")).len(), 1);
    assert_eq!(w.backend.journal().len(), 6);

    // Stored bytes and digest against the hand-labelled fixture.
    let id = rid_of(&rid);
    let stored = w.backend.stored(&id).unwrap().clone();
    assert_eq!(stored.canonical, read("doc/ac2-report-text.json"));
    let digests = String::from_utf8(read("doc/digests.txt")).unwrap();
    assert!(digests.contains(&format!("ac2_report {}", stored.digest)));

    // Close and reopen: same IDs, order and stored digest (REQ-008).
    let before = w.show(&rid);
    let (docs, journal) = w.backend.clone().into_parts();
    let mut reopened =
        World { backend: MemoryBackend::reopen(project_id(), docs, journal), kinds: kinds(), ids: Counter::new() };
    assert_eq!(reopened.show(&rid), before);
    assert_eq!(reopened.backend.stored(&id).unwrap().digest, stored.digest);
    let report = StoredReport::of(&stored.open(&id, &kinds()).unwrap()).unwrap();
    assert_eq!(report, stored);
}

#[test]
fn ac2_journal_events_match_hand_labelled_text() {
    let (w, ..) = ac2_world();
    let events = CValue::Arr(w.backend.journal_values());
    assert_eq!(to_canonical_bytes(&events).unwrap(), read("doc/ac2-events-text.json"));
    let digests = String::from_utf8(read("doc/digests.txt")).unwrap();
    for (i, e) in w.backend.journal().iter().enumerate() {
        assert!(digests.contains(&format!("event_{i} {}", e.report_digest())));
    }
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_remove_non_empty_page_without_force_fails_and_journals_nothing() {
    let (mut w, rid, p1, _) = ac2_world();
    let before = w.backend.clone();
    let e = w.run(REMOVE_PAGE, obj([("report", s(&rid)), ("page", s(&p1))])).unwrap_err();
    assert_eq!(e.code(), "report.page_not_empty");
    assert_eq!(w.backend, before);
    // `force: false` explicitly is the same.
    let e = w.run(REMOVE_PAGE, obj([("report", s(&rid)), ("page", s(&p1)), ("force", CValue::Bool(false))]));
    assert_eq!(e.unwrap_err().code(), "report.page_not_empty");
    assert_eq!(w.backend.journal().len(), 6);
}

#[test]
fn remove_page_with_force_removes_its_blocks_and_retires_every_id() {
    let (mut w, rid, p1, _) = ac2_world();
    let shown = w.show(&rid);
    let blocks: Vec<String> =
        arr(get(&arr(get(&shown, "pages"))[0], "blocks")).iter().map(|b| str_of(get(b, "id")).to_string()).collect();
    w.run(REMOVE_PAGE, obj([("report", s(&rid)), ("page", s(&p1)), ("force", CValue::Bool(true))])).unwrap();
    let shown = w.show(&rid);
    assert_eq!(arr(get(&shown, "pages")).len(), 1);
    let retired: BTreeSet<&str> = arr(get(&shown, "retired_ids")).iter().map(str_of).collect();
    assert!(retired.contains(p1.as_str()));
    assert!(blocks.iter().all(|b| retired.contains(b.as_str())));
    assert_eq!(retired.len(), 4);
    assert_eq!(w.backend.journal().len(), 7);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_unknown_kind_and_unknown_ids_fail_and_journal_nothing() {
    let (mut w, rid, p1, _) = ac2_world();
    let before = w.backend.clone();
    let e = w
        .run(ADD_BLOCK, obj([("report", s(&rid)), ("page", s(&p1)), ("kind", s("no_such_kind")), ("payload", obj([]))]))
        .unwrap_err();
    assert_eq!(e.code(), "report.unknown_block_kind");
    assert_eq!(e.to_value(), obj([("code", s("report.unknown_block_kind")), ("kind", s("no_such_kind"))]));
    // A malformed kind is unknown too, and is not echoed.
    let e = w
        .run(ADD_BLOCK, obj([("report", s(&rid)), ("page", s(&p1)), ("kind", s("Evil\u{202e}")), ("payload", obj([]))]))
        .unwrap_err();
    assert_eq!(e.to_value(), obj([("code", s("report.unknown_block_kind"))]));
    // remove-block with a random (version 4) UUID names it.
    let random = "3f2b8c1e-9a4d-4e6f-8b2a-1c3d5e7f9a0b";
    let e = w.run(REMOVE_BLOCK, obj([("report", s(&rid)), ("block", s(random))])).unwrap_err();
    assert_eq!(e.code(), "report.not_found");
    assert_eq!(e.to_value(), obj([("code", s("report.not_found")), ("id", s(random))]));
    // Unknown report, page and sibling (REQ-006).
    let e = w.run(SHOW_REPORT, obj([("report", s(random))])).unwrap_err();
    assert_eq!(e, ReportError::NotFound(Uuid::parse(random).unwrap()));
    let e = w.add_text(&rid, random, "x").unwrap_err();
    assert_eq!(e.code(), "report.not_found");
    let e = w.run(ADD_PAGE, obj([("report", s(&rid)), ("after", s(random))])).unwrap_err();
    assert_eq!(e.code(), "report.not_found");
    assert_eq!(w.backend, before);
}

#[test]
fn invalid_payload_fails_with_field_and_journals_nothing() {
    let (mut w, rid, p1, _) = ac2_world();
    let before = w.backend.clone();
    let e = w
        .run(
            ADD_BLOCK,
            obj([
                ("report", s(&rid)),
                ("page", s(&p1)),
                ("kind", s("text")),
                ("payload", obj([("text", CValue::Bool(true))])),
            ]),
        )
        .unwrap_err();
    assert_eq!(e.code(), "report.invalid_block_payload");
    assert_eq!(get(&e.to_value(), "field"), &s("/text"));
    // A payload nested deeper than MAX_PAYLOAD_DEPTH is refused before the kind sees it.
    let mut deep = obj([]);
    for _ in 0..=MAX_PAYLOAD_DEPTH {
        deep = CValue::Arr(vec![deep]);
    }
    let e = w.run(ADD_BLOCK, obj([("report", s(&rid)), ("page", s(&p1)), ("kind", s("text")), ("payload", deep)]));
    assert_eq!(e.unwrap_err().code(), "report.invalid_block_payload");
    // A payload above MAX_PAYLOAD_BYTES likewise.
    let big = obj([("text", s(&"a".repeat(MAX_PAYLOAD_BYTES)))]);
    let e = w.run(ADD_BLOCK, obj([("report", s(&rid)), ("page", s(&p1)), ("kind", s("text")), ("payload", big)]));
    assert_eq!(e.unwrap_err().code(), "report.invalid_block_payload");
    assert_eq!(w.backend, before);
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_hostile_titles_are_stored_untrusted_or_refused() {
    let cases = json("hostile_strings/strings.json");
    assert_eq!(get(&cases, "licence"), &s("CC-BY-4.0"));
    let mut seen = 0;
    for case in arr(get(&cases, "cases")) {
        let text = str_of(get(case, "text"));
        let expect = str_of(get(case, "expect"));
        let mut w = World::empty();
        match w.create(text) {
            Ok(summary) => {
                assert_eq!(expect, "stored", "{}", str_of(get(case, "name")));
                let want = obj([("text", s(text)), ("untrusted", CValue::Bool(true))]);
                assert_eq!(get(&summary, "title"), &want);
                let rid = str_of(get(&summary, "report_id")).to_string();
                assert_eq!(get(&w.show(&rid), "title"), &want);
                assert_eq!(w.backend.journal().len(), 1);
            }
            Err(e) => {
                assert_eq!(e.code(), expect, "{}", str_of(get(case, "name")));
                assert!(w.backend.journal().is_empty());
            }
        }
        seen += 1;
    }
    assert_eq!(seen, 5);
}

#[test]
fn untrusted_text_escapes_controls_and_bidi_for_humans() {
    let t = Untrusted::text("a\u{202e}b\u{2066}c\u{200f}\\d\ne\u{7f}").unwrap();
    assert_eq!(t.escaped(), "a\\u{202E}b\\u{2066}c\\u{200F}\\\\d\\u{000A}e\\u{007F}");
    assert!(t.escaped().chars().all(|c| !must_escape(c)));
    assert_eq!(Text::new(&"x".repeat(MAX_TEXT_BYTES)).unwrap().len(), MAX_TEXT_BYTES);
    assert_eq!(Text::new(&"x".repeat(MAX_TEXT_BYTES + 1)), Err(ReportError::TextInvalid(TextFault::TooLong)));
    assert_eq!(Text::new("a\0b"), Err(ReportError::TextInvalid(TextFault::Nul)));
}

// ------------------------------------------------------------------ AC-6

/// splitmix64, seeded in the test (AC-6: seed 42).
struct SplitMix(u64);

impl SplitMix {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[test]
fn ac6_thousand_blocks_added_and_removed_never_reuse_an_id() {
    let mut w = World::empty();
    let summary = w.create("ids").unwrap();
    let rid = str_of(get(&summary, "report_id")).to_string();
    let page = str_of(get(&arr(get(&summary, "pages"))[0], "page_id")).to_string();
    let mut rng = SplitMix(42);
    let mut issued: BTreeSet<String> = BTreeSet::from([rid.clone(), page.clone()]);
    let mut live: Vec<String> = Vec::new();
    let mut added = 0;
    while added < 1000 || !live.is_empty() {
        let add = added < 1000 && (live.is_empty() || rng.next_u64() % 2 == 0);
        if add {
            let out = w.add_text(&rid, &page, &format!("b{added}")).unwrap();
            let id = str_of(get(&out, "block_id")).to_string();
            assert!(issued.insert(id.clone()), "block id issued twice: {id}");
            live.push(id);
            added += 1;
        } else {
            let at = (rng.next_u64() % live.len() as u64) as usize;
            let id = live.swap_remove(at);
            w.run(REMOVE_BLOCK, obj([("report", s(&rid)), ("block", s(&id))])).unwrap();
        }
    }
    assert_eq!(issued.len(), 1002);
    let shown = w.show(&rid);
    assert_eq!(arr(get(&shown, "retired_ids")).len(), 1000);
    assert_eq!(w.backend.journal().len(), 1 + 2000);
}

#[test]
fn a_source_that_repeats_a_retired_id_fails_closed() {
    // Stuck yields the same UUIDv7 every time: the report gets it, the page collides.
    let mut w = World::empty();
    let e = execute(
        CREATE_REPORT,
        &obj([("title", s("t")), ("project", s(&project_id().to_string()))]),
        &mut w.backend,
        &w.kinds,
        &mut Stuck,
    )
    .unwrap_err();
    assert_eq!(e.code(), "report.id_collision");
    assert!(w.backend.journal().is_empty());

    // A block ID that was removed is never issued again, even when the source repeats it.
    let summary = w.create("t").unwrap();
    let rid = str_of(get(&summary, "report_id")).to_string();
    let page = str_of(get(&arr(get(&summary, "pages"))[0], "page_id")).to_string();
    let p = obj([("report", s(&rid)), ("page", s(&page)), ("kind", s("text")), ("payload", obj([("text", s("x"))]))]);
    let b = execute(ADD_BLOCK, &p, &mut w.backend, &w.kinds, &mut Stuck).unwrap();
    w.run(REMOVE_BLOCK, obj([("report", s(&rid)), ("block", get(&b, "block_id").clone())])).unwrap();
    let n = w.backend.journal().len();
    let e = execute(ADD_BLOCK, &p, &mut w.backend, &w.kinds, &mut Stuck).unwrap_err();
    assert_eq!(e, ReportError::IdCollision(uuid_of(get(&b, "block_id"))));
    assert_eq!(w.backend.journal().len(), n);
}

// ------------------------------------------------------------------ AC-7

#[test]
fn ac7_exact_replay_is_identical_and_reuses_journaled_ids() {
    let (w, rid, ..) = ac2_world();
    let run = replay_exact(&w.backend.journal_values(), &kinds(), project_id());
    assert_eq!(run.outcome, ReplayOutcome::Identical);
    assert_eq!(run.outcome.as_str(), "IDENTICAL");
    let id = rid_of(&rid);
    let (report, digest) = run.reports.get(&id).unwrap();
    assert_eq!(*digest, w.backend.stored(&id).unwrap().digest);
    assert_eq!(StoredReport::of(report).unwrap(), *w.backend.stored(&id).unwrap());
}

#[test]
fn replay_detects_tampering_and_missing_kinds() {
    let (w, ..) = ac2_world();
    let events = w.backend.journal_values();
    // A changed payload changes the replayed digest: DIVERGENT at that event.
    let mut tampered = events.clone();
    if let CValue::Obj(o) = &mut tampered[2] {
        o.insert("payload".into(), obj([("untrusted", CValue::Bool(true)), ("value", obj([("text", s("forged"))]))]));
    }
    let run = replay_exact(&tampered, &kinds(), project_id());
    assert_eq!((run.outcome, run.first_mismatch), (ReplayOutcome::Divergent, Some(2)));
    // Without the image kind the last event cannot be replayed.
    let mut only_text = KindRegistry::new();
    only_text.register(Box::new(TextStub)).unwrap();
    let run = replay_exact(&events, &only_text, project_id());
    assert_eq!((run.outcome, run.first_mismatch), (ReplayOutcome::NotReplayable, Some(5)));
    // An unreadable event is NOT_REPLAYABLE.
    let run = replay_exact(&[obj([("type", s("report.nope@1"))])], &kinds(), project_id());
    assert_eq!(run.outcome, ReplayOutcome::NotReplayable);
    // Events round-trip through their journal form.
    for e in w.backend.journal() {
        assert_eq!(&ReportEvent::from_value(&e.to_value()).unwrap(), e);
    }
}

// ------------------------------------------------------------------ Design rules

#[test]
fn insert_after_sibling_and_after_page() {
    let (mut w, rid, p1, p2) = ac2_world();
    let shown = w.show(&rid);
    let first = str_of(get(&arr(get(&arr(get(&shown, "pages"))[0], "blocks"))[0], "id")).to_string();
    w.run(
        ADD_BLOCK,
        obj([
            ("report", s(&rid)),
            ("page", s(&p1)),
            ("after", s(&first)),
            ("kind", s("text")),
            ("payload", obj([("text", s("Block 1.5"))])),
        ]),
    )
    .unwrap();
    let mid = w.run(ADD_PAGE, obj([("report", s(&rid)), ("after", s(&p1))])).unwrap();
    let shown = w.show(&rid);
    let pages = arr(get(&shown, "pages"));
    let ids: Vec<&str> = pages.iter().map(|p| str_of(get(p, "id"))).collect();
    assert_eq!(ids, [p1.as_str(), str_of(get(&mid, "page_id")), p2.as_str()]);
    let texts: Vec<&str> =
        arr(get(&pages[0], "blocks")).iter().map(|b| str_of(get(get(get(b, "payload"), "value"), "text"))).collect();
    assert_eq!(texts, ["Block 1", "Block 1.5", "Block 2", "Block 3"]);
    // A sibling on another page is not found on this page.
    let img = str_of(get(&arr(get(&pages[2], "blocks"))[0], "id")).to_string();
    let e = w.run(
        ADD_BLOCK,
        obj([
            ("report", s(&rid)),
            ("page", s(&p1)),
            ("after", s(&img)),
            ("kind", s("text")),
            ("payload", obj([("text", s("x"))])),
        ]),
    );
    assert_eq!(e.unwrap_err(), ReportError::NotFound(Uuid::parse(&img).unwrap()));
}

#[test]
fn bad_params_are_refused_before_anything_runs() {
    let (mut w, rid, ..) = ac2_world();
    let before = w.backend.clone();
    let cases = [
        (SHOW_REPORT, obj([("report", s(&rid)), ("extra", CValue::Null)])),
        (SHOW_REPORT, obj([("report", s(&rid.to_uppercase()))])),
        (SHOW_REPORT, obj([])),
        (REMOVE_PAGE, obj([("report", s(&rid)), ("page", s(&rid)), ("force", s("yes"))])),
        (CREATE_REPORT, obj([("title", CValue::Num(1.0)), ("project", s(&project_id().to_string()))])),
        ("report.no_such_command@1", obj([])),
        (ADD_PAGE, CValue::Arr(Vec::new())),
    ];
    for (cmd, p) in cases {
        assert_eq!(w.run(cmd, p).unwrap_err().code(), "report.bad_params", "{cmd}");
    }
    // Another project is not found.
    let other = "019a9a9a-0000-7000-8000-0000000000ff";
    let e = w.run(CREATE_REPORT, obj([("title", s("t")), ("project", s(other))])).unwrap_err();
    assert_eq!(e.code(), "report.not_found");
    assert_eq!(w.backend, before);
}

#[test]
fn stored_documents_are_read_strictly() {
    let (w, rid, ..) = ac2_world();
    let id = rid_of(&rid);
    let good = w.backend.stored(&id).unwrap().clone();
    // Wrong digest.
    let bad = StoredReport { canonical: good.canonical.clone(), digest: fid_types::Digest::TOMBSTONE };
    assert_eq!(bad.open(&id, &kinds()).unwrap_err().code(), "report.store_invalid");
    // Non-canonical bytes (a space) with a matching digest.
    let mut spaced = good.canonical.clone();
    spaced.insert(1, b' ');
    let d = fid_types::Digest::from_bytes(fid_canon::digest_bytes(&spaced).0);
    assert_eq!(
        StoredReport { canonical: spaced, digest: d }.open(&id, &kinds()).unwrap_err().code(),
        "report.store_invalid"
    );
    // Unknown field, a repeated ID, an unmarked title.
    let v = parse(&good.canonical).unwrap();
    let mut extra = v.clone();
    if let CValue::Obj(o) = &mut extra {
        o.insert("note".into(), s("x"));
    }
    assert_eq!(Report::from_value(&extra, &kinds()).unwrap_err().code(), "report.store_invalid");
    let mut repeated = v.clone();
    if let CValue::Obj(o) = &mut repeated {
        o.insert("retired_ids".into(), CValue::Arr(vec![s(&rid)]));
    }
    assert_eq!(Report::from_value(&repeated, &kinds()).unwrap_err().code(), "report.store_invalid");
    let mut plain = v.clone();
    if let CValue::Obj(o) = &mut plain {
        o.insert("title".into(), s("Bracket FAI"));
    }
    assert_eq!(Report::from_value(&plain, &kinds()).unwrap_err().code(), "report.store_invalid");
    // Stored under another ID.
    let other = rid_of("019a9969-3401-7000-8000-000000000002");
    assert_eq!(good.open(&other, &kinds()).unwrap_err().code(), "report.store_invalid");
}

#[test]
fn registry_definitions_build_and_mark_untrusted_text() {
    let defs = command_defs().unwrap();
    let names: Vec<String> = defs.iter().map(|d| format!("{}@{}", d.id, d.major)).collect();
    assert_eq!(names, [CREATE_REPORT, ADD_PAGE, REMOVE_PAGE, ADD_BLOCK, REMOVE_BLOCK, SHOW_REPORT]);
    for d in &defs {
        let write = d.id != "report.show_report";
        assert_eq!(d.side_effect.as_str(), if write { "W2" } else { "R0" }, "{}", d.id);
        assert_eq!(d.effects, if write { vec![format!("{}@1", d.id)] } else { Vec::new() });
    }
    let create = to_canonical_bytes(&defs[0].params_schema).unwrap();
    assert!(String::from_utf8(create).unwrap().contains("\"x-untrusted\":true"));
    let show = to_canonical_bytes(&defs[5].result_schema).unwrap();
    assert!(String::from_utf8(show).unwrap().contains("\"x-untrusted\":true"));
}

#[test]
fn kind_registry_rules() {
    let mut k = kinds();
    assert_eq!(k.register(Box::new(TextStub)).unwrap_err().code(), "duplicate_kind");
    let ids: Vec<&str> = k.ids().map(BlockKindId::as_str).collect();
    assert_eq!(ids, ["image", "text"]);
    // Every kind-ID literal in the crate matches the pattern.
    for kind in [TextStub.id(), ImageStub.id()] {
        assert_eq!(BlockKindId::new(kind.as_str()), Some(kind.clone()));
    }
    for good in ["text", "form_field", "a", "a234567890123456789012345678901"] {
        assert!(BlockKindId::new(good).is_some(), "{good}");
    }
    for bad in ["", "Text", "1a", "a-b", "a234567890123456789012345678901_x", "t\u{202e}"] {
        assert!(BlockKindId::new(bad).is_none(), "{bad}");
    }
    let p = PayloadError::new("/a\u{202e}\nb", "r");
    assert_eq!(p.field(), "/a??b");
    assert_eq!(PayloadError::new(&"x".repeat(500), "r").field().len(), 128);
}

#[test]
fn every_id_is_unique_within_the_report() {
    let (mut w, rid, ..) = ac2_world();
    let shown = w.show(&rid);
    let mut all = vec![str_of(get(&shown, "id")).to_string()];
    for p in arr(get(&shown, "pages")) {
        all.push(str_of(get(p, "id")).to_string());
        all.extend(arr(get(p, "blocks")).iter().map(|b| str_of(get(b, "id")).to_string()));
    }
    let set: BTreeSet<&String> = all.iter().collect();
    assert_eq!(set.len(), all.len());
    let counts: BTreeMap<u8, usize> = all.iter().fold(BTreeMap::new(), |mut m, i| {
        *m.entry(Uuid::parse(i).unwrap().version()).or_default() += 1;
        m
    });
    assert_eq!(counts, BTreeMap::from([(7u8, 7usize)]));
    assert_eq!(V7_RANDOM_BITS, 74);
}

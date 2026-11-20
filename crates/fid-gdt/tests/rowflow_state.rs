//! Acceptance criteria of spec node GDT.RES.ROWFLOW.STATE (AC-1 .. AC-7) plus the D-045
//! content-digest binding. Every case file under `fixtures/gdt/rowflow/` (`happy.json`,
//! `self_review.json`, ...) holds an event list and, per event, the outcome labelled by the
//! independent stdlib-Python reference `fixtures/gdt/rowflow/make_state_cases.py`; the Rust
//! crate never produced an expected value. The format checks at the end are hand-labelled
//! from the leaf's rules. Rows, keys, designators, editions, clause numbers, reviewer IDs and
//! identities are all invented; no standards text. Principals are derived by the bus from
//! channels, the only way to build one.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::principal::{
    Channel, ChannelKind, Identity, OsUser, Principal, PrincipalId, PrincipalKind, SessionId, SessionKey, SessionTable,
};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_gdt::dmt_flow::roster::{Date, Roster, RosterView};
use fid_gdt::dmt_flow::state::{
    propose, transition, Attestation, Decision, FlowError, FlowState, IdrDraft, Provenance, ReviewEvent, RowRevision,
    MAX_ATTESTATIONS, MAX_IDR_POSITIONS,
};
use fid_gps::dmt::row::row_from_value;
use fid_gps::dmt::{row_content_digest, DmtRow, ReviewState, Reviewers};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ fixtures

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/rowflow")
}

fn doc(name: &str) -> CValue {
    parse(&std::fs::read(fixtures().join(name)).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => m,
        other => panic!("not an object: {other:?}"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("not an array: {other:?}"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("not a string: {other:?}"),
    }
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    obj(v).get(k).unwrap_or_else(|| panic!("missing {k}"))
}

fn roster() -> Roster {
    Roster::load(&std::fs::read(fixtures().join("state_roster.json")).unwrap()).unwrap()
}

fn rows() -> BTreeMap<String, DmtRow> {
    let d = doc("state_rows.json");
    obj(get(&d, "rows")).iter().map(|(k, v)| (k.clone(), row_from_value(v).unwrap())).collect()
}

fn date(s: &str) -> Date {
    Date::parse(s).unwrap()
}

fn day() -> Date {
    date("2026-11-20")
}

// ------------------------------------------------------------------ principals

struct TestChannel {
    kind: ChannelKind,
    identity: Identity,
    session: SessionKey,
}

impl Channel for TestChannel {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    fn authenticated_identity(&self) -> Option<Identity> {
        Some(self.identity.clone())
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
}

/// A principal of `kind` as the bus derives it: GUI is `human`, MCP `agent`, a CLI session
/// that was never upgraded `script`.
fn principal(table: &mut SessionTable, kind: &str, subject: &str) -> Principal {
    let ch = match kind {
        "human" => ChannelKind::Gui,
        "agent" => ChannelKind::Mcp,
        "script" => ChannelKind::Cli,
        other => panic!("unknown kind {other}"),
    };
    let mut identity = Identity::new(PrincipalId::new(subject).unwrap());
    identity.os_user = Some(OsUser::new(&subject.to_ascii_lowercase()).unwrap());
    let session = table.open_session(SessionId::new(&format!("{kind}-{subject}")).unwrap()).unwrap();
    let c = TestChannel { kind: ch, identity, session };
    let p = table.derive_principal(&c).unwrap();
    assert_eq!(p.kind().as_str(), kind);
    p
}

fn principal_of(table: &mut SessionTable, v: &CValue) -> Principal {
    principal(table, st(get(v, "kind")), st(get(v, "id")))
}

// ------------------------------------------------------------------ the runner

fn num(n: usize) -> CValue {
    CValue::Num(f64::from(u32::try_from(n).unwrap()))
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn o<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// The outcome summary in the reference's shape.
fn summary(rev: &RowRevision) -> CValue {
    let approvals: Vec<CValue> = rev.live_approvals().iter().map(|r| s(r.as_str())).collect();
    let mapped = match rev.review_state().unwrap() {
        ReviewState::Draft => o([("state", s("draft"))]),
        ReviewState::UnderReview => o([("state", s("under_review"))]),
        ReviewState::SignedOff(r) => {
            o([("reviewers", CValue::Arr(r.as_slice().iter().map(|x| s(x)).collect())), ("state", s("signed_off"))])
        }
    };
    let derived = match rev.derived_from() {
        None => CValue::Null,
        Some(d) => o([("content_digest", s(d.content_digest.as_str())), ("revision", num(d.revision as usize))]),
    };
    let idrs = rev
        .idrs()
        .iter()
        .map(|i| o([("id", s(i.id())), ("positions", num(i.positions().len())), ("status", s(i.status().as_str()))]))
        .collect();
    o([
        ("approvals", CValue::Arr(approvals)),
        ("author_kind", s(rev.author_kind().as_str())),
        ("content_digest", s(rev.content_digest().as_str())),
        ("derived_from", derived),
        ("idrs", CValue::Arr(idrs)),
        ("key", s(rev.key().as_str())),
        ("provenance", s(rev.provenance().as_str())),
        ("review_state", mapped),
        ("reviews", num(rev.reviews().len())),
        ("revision", num(rev.revision() as usize)),
        ("state", s(rev.state().as_str())),
    ])
}

fn error_value(e: &FlowError) -> CValue {
    let detail = match e {
        FlowError::AttestationMissing { designator, edition } => {
            o([("designator", s(designator)), ("edition", s(edition))])
        }
        FlowError::BadTransition { state, event } => o([("event", s(event)), ("state", s(state.as_str()))]),
        FlowError::FieldTooLong { field } => o([("field", s(field))]),
        FlowError::BadIdr { reason } => o([("reason", s(reason))]),
        _ => CValue::Obj(BTreeMap::new()),
    };
    o([("error", o([("code", s(e.code())), ("detail", detail)]))])
}

fn attestations(spec: &CValue, row: &DmtRow) -> Vec<Attestation> {
    match spec {
        CValue::Str(t) if t == "cited" => row
            .clauses()
            .iter()
            .map(|c| Attestation::EditionHeld {
                designator: c.designator().to_owned(),
                edition: c.edition().to_owned(),
            })
            .collect(),
        CValue::Str(t) if t == "placeholder" => vec![Attestation::NotRequiredPlaceholder],
        CValue::Str(t) if t == "none" => Vec::new(),
        CValue::Arr(pairs) => pairs
            .iter()
            .map(|p| {
                let p = arr(p);
                Attestation::EditionHeld { designator: st(&p[0]).to_owned(), edition: st(&p[1]).to_owned() }
            })
            .collect(),
        other => panic!("bad attest {other:?}"),
    }
}

struct Run {
    outcomes: Vec<CValue>,
    revisions: Vec<RowRevision>,
    records: Vec<Vec<u8>>,
}

fn run_case(name: &str) -> (Run, Vec<CValue>) {
    let d = doc(&format!("{name}.json"));
    let roster = roster();
    let rows = rows();
    let mut table = SessionTable::new();
    let mut rev: Option<RowRevision> = None;
    let mut run = Run { outcomes: Vec::new(), revisions: Vec::new(), records: Vec::new() };
    for ev in arr(get(&d, "events")) {
        let at = date(st(get(ev, "at")));
        let before = rev.clone();
        let result = match st(get(ev, "op")) {
            "propose" => {
                let author = principal_of(&mut table, get(ev, "author"));
                propose(rev.as_ref(), rows[st(get(ev, "row"))].clone(), &author, at)
            }
            "review" => {
                let current = rev.as_ref().unwrap();
                let reviewer = principal_of(&mut table, get(ev, "reviewer"));
                let decision = match st(get(ev, "decision")) {
                    "approve" => Decision::Approve,
                    "request_changes" => Decision::RequestChanges,
                    other => panic!("bad decision {other}"),
                };
                let reviewed_digest = match st(get(ev, "reviewed")) {
                    "current" => current.content_digest(),
                    other => row_content_digest(&rows[other]).unwrap(),
                };
                let idr = obj(ev).get("idr").map(|i| IdrDraft {
                    question_code: st(get(i, "question_code")).to_owned(),
                    positions: arr(get(i, "positions")).iter().map(|p| st(p).to_owned()).collect(),
                });
                let event = ReviewEvent {
                    reviewer: &reviewer,
                    decision,
                    attestations: attestations(get(ev, "attest"), current.row()),
                    reviewed_digest,
                    idr,
                    at,
                };
                transition(current, &event, &roster)
            }
            other => panic!("bad op {other}"),
        };
        match result {
            Ok(step) => {
                run.outcomes.push(o([("ok", summary(&step.revision))]));
                run.records.push(step.record.canonical_bytes().unwrap());
                run.revisions.push(step.revision.clone());
                rev = Some(step.revision);
            }
            Err(e) => {
                // The input revision is unchanged and nothing is returned to journal.
                assert_eq!(rev, before, "{name}: a failed event changed the revision");
                run.outcomes.push(error_value(&e));
            }
        }
    }
    let expected = arr(get(&d, "expected")).to_vec();
    (run, expected)
}

fn check_case(name: &str) -> Run {
    let (run, expected) = run_case(name);
    assert_eq!(run.outcomes.len(), expected.len(), "{name}");
    for (i, (got, want)) in run.outcomes.iter().zip(&expected).enumerate() {
        assert_eq!(
            String::from_utf8(to_canonical_bytes(got).unwrap()).unwrap(),
            String::from_utf8(to_canonical_bytes(want).unwrap()).unwrap(),
            "{name}: event {i}"
        );
    }
    run
}

// ------------------------------------------------------------------ every labelled case

const CASES: [&str; 14] = [
    "agent_authored",
    "agent_reviewer",
    "dup_reviewer",
    "happy",
    "idr",
    "key_change",
    "no_attestation",
    "not_in_roster",
    "placeholder",
    "placeholder_on_supported",
    "reset",
    "same_person_two_principals",
    "self_review",
    "synthetic_flip",
];

#[test]
fn every_case_matches_the_reference() {
    for name in CASES {
        check_case(name);
    }
    // Every case file the reference wrote is exercised.
    let mut on_disk: Vec<String> = std::fs::read_dir(fixtures())
        .unwrap()
        .filter_map(|e| {
            let n = e.unwrap().file_name().into_string().unwrap();
            let stem = n.strip_suffix(".json")?.to_owned();
            // Roster error fixtures may not even parse; only case files hold `events`.
            let d = parse(&std::fs::read(fixtures().join(&n)).ok()?).ok()?;
            matches!(&d, CValue::Obj(m) if m.contains_key("events")).then_some(stem)
        })
        .collect();
    on_disk.sort();
    let mut listed: Vec<String> = CASES.iter().map(|s| (*s).to_owned()).collect();
    listed.sort();
    assert_eq!(on_disk, listed);
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_happy_path_signs_off_with_sorted_reviewers() {
    let run = check_case("happy");
    let states: Vec<FlowState> = run.revisions.iter().map(RowRevision::state).collect();
    assert_eq!(states, [FlowState::Draft, FlowState::UnderReview, FlowState::SignedOff]);
    let last = run.revisions.last().unwrap();
    let want = Reviewers::new(vec!["r1".to_owned(), "r2".to_owned()]).unwrap();
    assert_eq!(last.review_state().unwrap(), ReviewState::SignedOff(want));
    // Every review record carries the roster digest it was checked against.
    let roster = roster();
    assert!(last.reviews().iter().all(|r| r.roster_digest() == roster.roster_digest()));
    assert!(last.reviews().iter().all(|r| r.reviewed_digest() == last.content_digest()));
}

#[test]
fn transitions_are_journal_records() {
    let (run, _) = run_case("happy");
    let events: Vec<(String, Option<String>, String)> = run
        .records
        .iter()
        .map(|b| {
            let v = parse(b).unwrap();
            let from = match get(&v, "from") {
                CValue::Null => None,
                f => Some(st(f).to_owned()),
            };
            (st(get(&v, "event")).to_owned(), from, st(get(&v, "to")).to_owned())
        })
        .collect();
    assert_eq!(
        events,
        [
            ("propose".to_owned(), None, "draft".to_owned()),
            ("approve".to_owned(), Some("draft".to_owned()), "under_review".to_owned()),
            ("approve".to_owned(), Some("under_review".to_owned()), "signed_off".to_owned()),
        ]
    );
    let review = parse(&run.records[1]).unwrap();
    assert_eq!(st(get(&review, "reviewer")), "r1");
    assert_eq!(st(get(&review, "roster_digest")), roster().roster_digest().as_str());
    assert_eq!(st(get(&review, "content_digest")), run.revisions[0].content_digest().as_str());
}

#[test]
fn reviewers_are_pseudonymous() {
    let run = check_case("happy");
    let bytes = run.revisions.last().unwrap().canonical_bytes().unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let mut all = text.clone();
    for r in &run.records {
        all.push_str(std::str::from_utf8(r).unwrap());
    }
    // Roster IDs only: never a reviewer's principal identity or display name.
    for leak in ["rev-one", "rev-two", "Reviewer One", "Reviewer Two", "fictional"] {
        assert!(!all.contains(leak), "{leak} leaked into a record");
    }
    assert!(text.contains("\"reviewer\":\"r1\"") && text.contains("\"reviewer\":\"r2\""));
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_error_paths() {
    for (name, code) in [
        ("self_review", "reviewer_is_author"),
        ("dup_reviewer", "duplicate_reviewer"),
        ("not_in_roster", "reviewer_not_in_roster"),
        ("agent_reviewer", "reviewer_not_human"),
        ("no_attestation", "attestation_missing"),
    ] {
        let (run, _) = run_case(name);
        check_case(name);
        assert!(
            run.outcomes.iter().any(|v| obj(v).get("error").is_some_and(|e| st(get(e, "code")) == code)),
            "{name} never failed with {code}"
        );
    }
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_agent_revision_needs_a_human_reproposal() {
    let run = check_case("agent_authored");
    let agent_rev = &run.revisions[0];
    assert_eq!(agent_rev.author_kind(), PrincipalKind::Agent);
    assert_eq!(agent_rev.provenance(), Provenance::Agent);
    let human_rev = &run.revisions[1];
    assert_eq!(human_rev.provenance(), Provenance::Human);
    let d = human_rev.derived_from().unwrap();
    assert_eq!((d.revision, d.content_digest), (1, agent_rev.content_digest()));
    assert_eq!(run.revisions.last().unwrap().state(), FlowState::SignedOff);
}

#[test]
fn every_non_human_author_is_refused_like_an_agent() {
    let rows = rows();
    let roster = roster();
    let mut table = SessionTable::new();
    let script = principal(&mut table, "script", "batch-job");
    let r1 = principal(&mut table, "human", "rev-one");
    let rev = propose(None, rows["supported"].clone(), &script, day()).unwrap().revision;
    assert_eq!(rev.provenance(), Provenance::Agent);
    let ev = approve(&r1, &rev);
    assert_eq!(transition(&rev, &ev, &roster).unwrap_err(), FlowError::AgentAuthoredRevision);
}

// ------------------------------------------------------------------ AC-4 and D-045

#[test]
fn ac4_content_change_resets_a_signed_off_row() {
    let run = check_case("reset");
    let signed = &run.revisions[2];
    assert_eq!(signed.state(), FlowState::SignedOff);
    let new = &run.revisions[3];
    assert_eq!((new.state(), new.reviews().len(), new.revision()), (FlowState::Draft, 0, 2));
    assert_ne!(new.content_digest(), signed.content_digest());
    assert_eq!(new.review_state().unwrap(), ReviewState::Draft);
}

#[test]
fn synthetic_flip_changes_the_digest_and_resets() {
    let run = check_case("synthetic_flip");
    let signed = &run.revisions[2];
    let flipped = &run.revisions[3];
    assert_eq!(signed.state(), FlowState::SignedOff);
    assert_eq!(signed.row().synthetic(), !flipped.row().synthetic());
    assert_ne!(signed.content_digest(), flipped.content_digest());
    assert_eq!((flipped.state(), flipped.reviews().len()), (FlowState::Draft, 0));
    assert_eq!(flipped.derived_from().unwrap().content_digest, signed.content_digest());
    // The digest of the row actually held, recomputed, equals the reference's.
    let d = doc("state_rows.json");
    let want = st(get(get(&d, "content_digests"), "supported_synthetic_flip"));
    assert_eq!(flipped.content_digest().as_str(), want);
    assert_eq!(row_content_digest(flipped.row()).unwrap(), flipped.content_digest());
}

#[test]
fn review_state_in_the_row_file_never_changes_the_content_digest() {
    let rows = rows();
    let mut table = SessionTable::new();
    let author = principal(&mut table, "human", "author-a");
    let a = propose(None, rows["supported"].clone(), &author, day()).unwrap().revision;
    let b = propose(None, rows["supported_review_only"].clone(), &author, day()).unwrap().revision;
    // A row file that claims `signed_off` still starts as a draft with no reviews.
    assert_eq!(a.content_digest(), b.content_digest());
    assert_eq!((b.state(), b.reviews().len()), (FlowState::Draft, 0));
}

// ------------------------------------------------------------------ AC-5, AC-6

#[test]
fn ac5_placeholder_attestation() {
    let run = check_case("placeholder");
    assert_eq!(run.revisions.last().unwrap().state(), FlowState::SignedOff);
    let (run, _) = run_case("placeholder_on_supported");
    check_case("placeholder_on_supported");
    assert_eq!(st(get(get(&run.outcomes[1], "error"), "code")), "attestation_missing");
}

#[test]
fn ac6_idr_position_limit() {
    let run = check_case("idr");
    let opened = &run.revisions[2];
    assert_eq!(opened.state(), FlowState::Draft);
    assert!(opened.live_approvals().is_empty());
    assert_eq!(opened.idrs().len(), 1);
    assert_eq!(opened.idrs()[0].positions()[0].chars().count(), 280);
    assert_eq!(opened.reviews().last().unwrap().idr(), Some(opened.idrs()[0].id()));
}

// ------------------------------------------------------------------ AC-7

#[test]
fn ac7_same_events_give_identical_bytes() {
    for name in CASES {
        let (a, _) = run_case(name);
        let (b, _) = run_case(name);
        assert_eq!(a.records, b.records, "{name}");
        let bytes = |r: &Run| r.revisions.iter().map(|v| v.canonical_bytes().unwrap()).collect::<Vec<_>>();
        assert_eq!(bytes(&a), bytes(&b), "{name}");
    }
}

// ------------------------------------------------------------------ hand-labelled format checks

fn approve<'a>(who: &'a Principal, rev: &RowRevision) -> ReviewEvent<'a> {
    ReviewEvent {
        reviewer: who,
        decision: Decision::Approve,
        attestations: rev
            .row()
            .clauses()
            .iter()
            .map(|c| Attestation::EditionHeld {
                designator: c.designator().to_owned(),
                edition: c.edition().to_owned(),
            })
            .collect(),
        reviewed_digest: rev.content_digest(),
        idr: None,
        at: day(),
    }
}

#[test]
fn idr_and_attestation_formats() {
    let rows = rows();
    let roster = roster();
    let mut table = SessionTable::new();
    let author = principal(&mut table, "human", "author-a");
    let r1 = principal(&mut table, "human", "rev-one");
    let rev = propose(None, rows["supported"].clone(), &author, day()).unwrap().revision;
    let idr = |code: &str, positions: Vec<String>| Some(IdrDraft { question_code: code.to_owned(), positions });

    let mut ev = approve(&r1, &rev);
    ev.idr = idr("zone_shape", vec!["A reading.".to_owned()]);
    assert_eq!(transition(&rev, &ev, &roster).unwrap_err(), FlowError::BadIdr { reason: "idr_on_approve" });

    ev.decision = Decision::RequestChanges;
    for (code, positions, want) in [
        ("Zone", vec!["a".to_owned()], FlowError::BadIdr { reason: "bad_question_code" }),
        ("", vec!["a".to_owned()], FlowError::BadIdr { reason: "bad_question_code" }),
        ("zone", Vec::new(), FlowError::BadIdr { reason: "bad_position_count" }),
        ("zone", vec!["a".to_owned(); MAX_IDR_POSITIONS + 1], FlowError::BadIdr { reason: "bad_position_count" }),
        ("zone", vec![String::new()], FlowError::BadIdr { reason: "bad_position" }),
        ("zone", vec!["line\nbreak".to_owned()], FlowError::BadIdr { reason: "bad_position" }),
        ("zone", vec!["\u{e9}".repeat(281)], FlowError::FieldTooLong { field: "position" }),
    ] {
        ev.idr = idr(code, positions);
        assert_eq!(transition(&rev, &ev, &roster).unwrap_err(), want, "{code}");
    }
    // 280 non-ASCII characters is within the limit: the limit counts characters, not bytes.
    ev.idr = idr("zone", vec!["\u{e9}".repeat(280)]);
    assert_eq!(transition(&rev, &ev, &roster).unwrap().revision.idrs()[0].id(), "idr-1-1");

    let mut ev = approve(&r1, &rev);
    ev.attestations = vec![Attestation::NotRequiredPlaceholder; MAX_ATTESTATIONS + 1];
    assert_eq!(transition(&rev, &ev, &roster).unwrap_err(), FlowError::FieldTooLong { field: "attestations" });
    let mut ev = approve(&r1, &rev);
    ev.attestations.push(Attestation::EditionHeld { designator: "d".repeat(33), edition: "1".to_owned() });
    assert_eq!(transition(&rev, &ev, &roster).unwrap_err(), FlowError::FieldTooLong { field: "attestation" });
    // Duplicate and extra attestations are harmless; the record keeps them sorted and unique.
    let mut ev = approve(&r1, &rev);
    let first = ev.attestations[0].clone();
    ev.attestations.insert(0, first);
    ev.attestations.push(Attestation::EditionHeld { designator: "syn-std-z".to_owned(), edition: "1999".to_owned() });
    let next = transition(&rev, &ev, &roster).unwrap().revision;
    let rec = next.reviews()[0].edition_attestations();
    assert_eq!(rec.len(), 3);
    assert!(rec.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn stale_review_names_a_digest_other_than_the_revisions() {
    let rows = rows();
    let roster = roster();
    let mut table = SessionTable::new();
    let author = principal(&mut table, "human", "author-a");
    let r1 = principal(&mut table, "human", "rev-one");
    let rev = propose(None, rows["supported"].clone(), &author, day()).unwrap().revision;
    let mut ev = approve(&r1, &rev);
    ev.reviewed_digest = Digest::TOMBSTONE;
    assert_eq!(transition(&rev, &ev, &roster).unwrap_err(), FlowError::StaleReview);
}

#[test]
fn roster_role_is_checked_on_the_review_date() {
    let rows = rows();
    let roster = roster();
    let mut table = SessionTable::new();
    let author = principal(&mut table, "human", "author-a");
    // r5 held the role until 2026-11-01 (exclusive).
    let r5 = principal(&mut table, "human", "rev-five");
    let rev = propose(None, rows["supported"].clone(), &author, day()).unwrap().revision;
    let mut ev = approve(&r5, &rev);
    ev.at = date("2026-10-31");
    assert_eq!(transition(&rev, &ev, &roster).unwrap().revision.live_approvals()[0].as_str(), "r5");
    ev.at = date("2026-11-01");
    assert_eq!(transition(&rev, &ev, &roster).unwrap_err(), FlowError::ReviewerNotInRoster);
}

#[test]
fn request_changes_from_an_approver_clears_approvals_and_allows_reapproval() {
    let rows = rows();
    let roster = roster();
    let mut table = SessionTable::new();
    let author = principal(&mut table, "human", "author-a");
    let r1 = principal(&mut table, "human", "rev-one");
    let rev = propose(None, rows["supported"].clone(), &author, day()).unwrap().revision;
    let rev = transition(&rev, &approve(&r1, &rev), &roster).unwrap().revision;
    let mut rc = approve(&r1, &rev);
    rc.decision = Decision::RequestChanges;
    let step = transition(&rev, &rc, &roster).unwrap();
    assert_eq!((step.record.from(), step.record.to()), (Some(FlowState::UnderReview), FlowState::Draft));
    let rev = step.revision;
    assert!(rev.live_approvals().is_empty());
    assert_eq!(rev.reviews().len(), 2, "history is kept");
    let rev = transition(&rev, &approve(&r1, &rev), &roster).unwrap().revision;
    assert_eq!(rev.state(), FlowState::UnderReview);
}

#[test]
fn error_codes_and_display() {
    let cases = [
        (FlowError::AgentAuthoredRevision, "agent_authored_revision"),
        (
            FlowError::AttestationMissing { designator: "syn-std-a".into(), edition: "2001".into() },
            "attestation_missing{syn-std-a, 2001}",
        ),
        (FlowError::ReviewerIsAuthor, "reviewer_is_author"),
        (FlowError::DuplicateReviewer, "duplicate_reviewer"),
        (FlowError::ReviewerNotInRoster, "reviewer_not_in_roster"),
        (FlowError::ReviewerNotHuman, "reviewer_not_human"),
        (FlowError::FieldTooLong { field: "position" }, "field_too_long{position}"),
        (
            FlowError::BadTransition { state: FlowState::SignedOff, event: "approve" },
            "bad_transition{signed_off, approve}",
        ),
        (FlowError::StaleReview, "stale_review"),
        (FlowError::KeyMismatch, "key_mismatch"),
        (FlowError::BadIdr { reason: "bad_position" }, "bad_idr{bad_position}"),
    ];
    for (e, text) in cases {
        assert_eq!(e.to_string(), text);
        assert!(text.starts_with(e.code()));
    }
}

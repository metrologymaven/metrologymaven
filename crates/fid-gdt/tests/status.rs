//! Acceptance criteria of spec node GDT.RES.STATUS (AC-1 .. AC-12). Every case folder under
//! `fixtures/gdt/status/` holds a journal extract, the expected register file(s), a
//! `gdt-published/v1` list and `expected.json` (verification outcomes per purpose and
//! `status_of` answers), all written and labelled by the independent stdlib-Python reference
//! `fixtures/gdt/status/make_cases.py`; the Rust crate never produced an expected value. The
//! loader and command checks at the end are hand-labelled from the leaf's rules. Keys,
//! designators, editions, clause numbers, reviewer and organisation IDs, principals and
//! digests are invented; no standards text, no real names.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_gdt::dmt_flow::roster::Roster;
use fid_gdt::dmt_flow::status::{
    build_register, command_def, export_rule_status, invoke_cli, invoke_mcp, replay_export, row_content_digest,
    status_of, verify_register, CoiInputs, ExportContext, ExportEvent, JournalExtract, Published, Purpose, Reason,
    Status, StatusError, TrustedRosters, Utc, EXPORT_COMMAND, EXPORT_MCP,
};
use fid_gps::dmt::row::row_from_value;
use fid_io_reg::{Principal, RootAccess, RootId, RootSet};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const CASES: [&str; 18] = [
    "agent_approval",
    "agent_draft",
    "alternating",
    "agent_rewritten",
    "changed",
    "currency",
    "foreign_roster",
    "forged",
    "happy",
    "one_approval",
    "recused",
    "request_changes",
    "request_changes_resign",
    "same_org",
    "same_org_no_coi",
    "synthetic_flip",
    "unregistered",
    "wrong_digest",
];

// ------------------------------------------------------------------ fixtures

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/status")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(dir().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn json(rel: &str) -> CValue {
    parse(&read(rel)).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(k).unwrap_or_else(|| panic!("missing {k}")),
        other => panic!("not an object: {other:?}"),
    }
}

fn st<'a>(v: &'a CValue, k: &str) -> &'a str {
    match get(v, k) {
        CValue::Str(s) => s,
        other => panic!("{k} not a string: {other:?}"),
    }
}

fn opt(v: &CValue, k: &str) -> Option<String> {
    match get(v, k) {
        CValue::Null => None,
        CValue::Str(s) => Some(s.clone()),
        other => panic!("{k}: {other:?}"),
    }
}

fn arr<'a>(v: &'a CValue, k: &str) -> &'a [CValue] {
    match get(v, k) {
        CValue::Arr(a) => a,
        other => panic!("{k} not an array: {other:?}"),
    }
}

fn digest(s: &str) -> Digest {
    Digest::parse(s).unwrap()
}

fn roster(name: &str) -> Roster {
    Roster::load(&read(name)).unwrap()
}

fn coi(kind: &str) -> CoiInputs {
    match kind {
        "loaded" => CoiInputs::load(&read("reviewers.toml"), &read("profiles.toml")).unwrap(),
        "none" => CoiInputs::none(),
        other => panic!("coi {other}"),
    }
}

struct Case {
    journal: JournalExtract,
    rosters: Vec<Roster>,
    coi: CoiInputs,
    published: Published,
    expected: CValue,
}

fn case(name: &str) -> Case {
    let expected = json(&format!("{name}/expected.json"));
    Case {
        journal: JournalExtract::load(&read(&format!("{name}/journal.json"))).unwrap(),
        rosters: arr(&expected, "rosters")
            .iter()
            .map(|r| match r {
                CValue::Str(f) => roster(f),
                other => panic!("{other:?}"),
            })
            .collect(),
        coi: coi(st(&expected, "coi")),
        published: Published::load(&read(&format!("{name}/published.json"))).unwrap(),
        expected,
    }
}

/// The rosters of a case that `gdt-published/v1` lists, through the verified loader.
fn trusted(c: &Case) -> TrustedRosters {
    let listed = c.rosters.iter().filter(|r| c.published.lists_roster(r.digest())).cloned().collect();
    c.published.trusted_rosters(listed).unwrap()
}

fn status_str(s: Status) -> &'static str {
    s.as_str()
}

// ------------------------------------------------------------------ every case against the reference

#[test]
fn every_case_matches_the_reference() {
    for name in CASES {
        let c = case(name);
        for item in arr(&c.expected, "registers") {
            let rel = format!("{name}/{}", st(item, "file"));
            let file = read(&rel);
            let current = roster(st(item, "current"));
            if get(item, "genuine") == &CValue::Bool(true) {
                // REQ-002: the register built here is byte-identical to the reference's.
                let built =
                    build_register(&c.journal, Some(digest(st(item, "head"))), &current, &trusted(&c), &c.coi).unwrap();
                assert_eq!(
                    String::from_utf8(built.canonical_bytes().unwrap()).unwrap(),
                    String::from_utf8(file.clone()).unwrap(),
                    "{rel}"
                );
                assert_eq!(built.digest().unwrap().as_str(), st(item, "digest"), "{rel}");
            }
            // REQ-007, REQ-010: verification per purpose.
            for purpose in [Purpose::Evaluate, Purpose::Replay] {
                let checked = verify_register(&file, &c.journal, &c.rosters, &c.coi, &c.published, purpose);
                assert_eq!(
                    checked.verification().code(),
                    st(get(item, "verify"), purpose.as_str()),
                    "{rel} {}",
                    purpose.as_str()
                );
                assert_eq!(checked.register().is_some(), checked.verification().code() == "verified");
            }
            // REQ-008: status_of on the register as checked for evaluation.
            let checked = verify_register(&file, &c.journal, &c.rosters, &c.coi, &c.published, Purpose::Evaluate);
            for a in arr(item, "answers") {
                let ans = status_of(Some(&checked), st(a, "rule"), digest(st(a, "digest")));
                let want = (st(a, "status").to_owned(), opt(a, "reason"));
                let got = (status_str(ans.status).to_owned(), ans.reason.map(|r| r.as_str().to_owned()));
                assert_eq!(got, want, "{rel} {}", st(a, "rule"));
                if ans.status == Status::SignedOff {
                    let e = ans.entry.unwrap();
                    assert_eq!(e.rule, st(a, "rule"));
                    assert!(checked.register().is_some());
                }
            }
        }
    }
}

// ------------------------------------------------------------------ AC-1, AC-11: the command

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-gdt-status-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn roots(d: &Path) -> RootSet {
    let mut r = RootSet::new();
    r.insert(RootId::new("exports").unwrap(), RootAccess::Export, d);
    r
}

#[test]
fn ac1_cli_export_writes_the_signed_off_register_and_one_event() {
    let c = case("happy");
    let d = scratch("ac1");
    let roots = roots(&d);
    let main = roster("roster.json");
    let trusted = trusted(&c);
    let ctx = ExportContext { roots: &roots, journal: &c.journal, current: &main, trusted: &trusted, coi: &c.coi };
    let args = ["export-rule-status", "--out", "root://exports/status.json", "--json"];
    let ex = invoke_cli(&ctx, Principal::Agent, &args).unwrap();

    let want = read("happy/register.json");
    assert_eq!(std::fs::read(d.join("status.json")).unwrap(), want);
    assert_eq!(ex.bytes, want);
    let item = &arr(&c.expected, "registers")[0];
    let reg_digest = st(item, "digest");
    assert_eq!(std::fs::read_to_string(d.join("status.json.sha256")).unwrap(), format!("{reg_digest}\n"));

    // The row entry is signed off with both reviewer IDs, journal hashes and revised_at.
    let row = &ex.register.entries()[0];
    assert_eq!(row.status, Status::SignedOff);
    assert_eq!(row.reason, None);
    let reviewers: Vec<&str> = row.approvals.iter().map(|a| a.reviewer.as_str()).collect();
    assert_eq!(reviewers, ["r1", "r2"]);
    let hashes: Vec<Digest> = c.journal.entries().iter().map(|e| e.hash()).collect();
    assert_eq!(row.approvals[0].journal_entry, hashes[1]);
    assert_eq!(row.approvals[1].journal_entry, hashes[2]);
    assert_eq!(row.revised_at, *c.journal.entries()[0].utc());

    // One event with the register digest, the roster digest and the journal head.
    assert_eq!(ex.event.register_digest.as_str(), reg_digest);
    assert_eq!(ex.event.roster_digest, main.digest());
    assert_eq!(ex.event.journal_head, c.journal.head().unwrap());
    let ev = parse(&ex.event.canonical_bytes().unwrap()).unwrap();
    assert_eq!(st(&ev, "command"), EXPORT_COMMAND);
    assert_eq!(ExportEvent::from_canonical_bytes(&ex.event.canonical_bytes().unwrap()), Some(ex.event.clone()));

    // The --json result.
    let res = parse(&ex.result_bytes().unwrap()).unwrap();
    assert_eq!(st(&res, "register_digest"), reg_digest);
    assert_eq!(st(&res, "roster_digest"), main.digest().as_str());
    assert_eq!(st(&res, "path"), "root://exports/status.json");
    assert_eq!(get(&res, "entries"), &CValue::Num(2.0));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn ac11_export_is_deterministic_and_replays_identical() {
    let c = case("happy");
    let d = scratch("ac11");
    let roots = roots(&d);
    let main = roster("roster.json");
    let trusted = trusted(&c);
    let ctx = ExportContext { roots: &roots, journal: &c.journal, current: &main, trusted: &trusted, coi: &c.coi };
    let a = export_rule_status(&ctx, Principal::Human, "root://exports/a.json", None).unwrap();
    let b = invoke_mcp(&ctx, Principal::Agent, EXPORT_MCP, br#"{"out":"root://exports/b.json"}"#).unwrap();
    assert_eq!(std::fs::read(d.join("a.json")).unwrap(), std::fs::read(d.join("b.json")).unwrap());
    assert_eq!(a.event.register_digest, b.event.register_digest);

    let recorded = a.event.canonical_bytes().unwrap();
    assert_eq!(replay_export(&recorded, &c.journal, &main, &trusted, &c.coi), ReplayOutcome::Identical);
    // A changed digest diverges; an unreadable event or unknown head is not replayable.
    let mut forged = a.event.clone();
    forged.register_digest = digest(&format!("sha256:{}", "2".repeat(64)));
    let forged = forged.canonical_bytes().unwrap();
    assert_eq!(replay_export(&forged, &c.journal, &main, &trusted, &c.coi), ReplayOutcome::Divergent);
    assert_eq!(replay_export(b"{}", &c.journal, &main, &trusted, &c.coi), ReplayOutcome::NotReplayable);
    let mut lost = a.event.clone();
    lost.journal_head = digest(&format!("sha256:{}", "3".repeat(64)));
    let lost = lost.canonical_bytes().unwrap();
    assert_eq!(replay_export(&lost, &c.journal, &main, &trusted, &c.coi), ReplayOutcome::NotReplayable);

    // An export at an earlier journal entry is AC-12's R1.
    let currency = case("currency");
    let r1 = &arr(&currency.expected, "registers")[0];
    let at = digest(st(r1, "head"));
    let early = export_rule_status(&ctx, Principal::Human, "root://exports/r1.json", Some(at)).unwrap();
    assert_eq!(early.bytes, read("currency/register_r1.json"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn export_refusals() {
    let c = case("happy");
    let d = scratch("refuse");
    let roots = roots(&d);
    let main = roster("roster.json");
    let trusted = trusted(&c);
    let ctx = ExportContext { roots: &roots, journal: &c.journal, current: &main, trusted: &trusted, coi: &c.coi };
    let code = |r: Result<_, StatusError>| r.map(|_: fid_gdt::dmt_flow::status::Exported| ()).unwrap_err();
    // An agent may not name a raw path; a handle outside a configured root is refused.
    let raw = d.join("raw.json");
    let e = code(export_rule_status(&ctx, Principal::Agent, raw.to_str().unwrap(), None));
    assert_eq!(e.code(), "export_root_denied");
    let e = code(export_rule_status(&ctx, Principal::Agent, "root://nowhere/x.json", None));
    assert_eq!(e.code(), "export_root_denied");
    // An existing file is never overwritten.
    export_rule_status(&ctx, Principal::Agent, "root://exports/once.json", None).unwrap();
    let e = code(export_rule_status(&ctx, Principal::Agent, "root://exports/once.json", None));
    assert_eq!(e, StatusError::ExportRootDenied { reason: "target_exists" });
    // Only .json files.
    let e = code(export_rule_status(&ctx, Principal::Agent, "root://exports/x.txt", None));
    assert_eq!(e, StatusError::BadParams { reason: "bad_out" });
    // An unknown journal head.
    let unknown = digest(&format!("sha256:{}", "4".repeat(64)));
    let e = code(export_rule_status(&ctx, Principal::Agent, "root://exports/y.json", Some(unknown)));
    assert_eq!(e, StatusError::JournalUnreadable { reason: "unknown_head" });
    // Parameter parsing on both surfaces.
    for args in [
        &["export-rule-status"][..],
        &["export-rule-status", "--out"],
        &["export-rule-status", "--out", "root://exports/z.json", "--out", "root://exports/z2.json"],
        &["export-rule-status", "--out", "root://exports/z.json", "--json", "--json"],
        &["export-rule-status", "--bogus", "x"],
        &["status", "--out", "root://exports/z.json"],
        &["export-rule-status", "--out", "root://exports/z.json", "--at", "sha256:xyz"],
    ] {
        assert_eq!(code(invoke_cli(&ctx, Principal::Human, args)).code(), "bad_params", "{args:?}");
    }
    for raw in [
        &br#"{"out":"root://exports/z.json","out":"root://exports/q.json"}"#[..],
        br#"{"out":1}"#,
        br#"{"x":"y"}"#,
        b"[]",
    ] {
        assert_eq!(code(invoke_mcp(&ctx, Principal::Agent, EXPORT_MCP, raw)).code(), "bad_params");
    }
    assert_eq!(
        code(invoke_mcp(&ctx, Principal::Agent, "gdt_other", br#"{"out":"root://exports/z.json"}"#)).code(),
        "bad_params"
    );
    assert!(!d.join("z.json").exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn command_definition_is_w1_d0_write_root_on_cli_and_mcp() {
    let d = command_def().unwrap();
    assert_eq!(format!("{}@{}", d.id, d.major), EXPORT_COMMAND);
    assert_eq!(d.side_effect.as_str(), "W1");
    assert_eq!(d.determinism.as_str(), "D0");
    assert_eq!(d.fs_effect.as_str(), "write_root");
    assert!(!d.human_only);
    assert_eq!(d.effects, [EXPORT_COMMAND]);
    let geo = fid_registry::Catalogue::geo_frame().unwrap();
    assert_eq!(fid_registry::check_schema(&d.params_schema, &geo), vec![]);
    assert_eq!(fid_registry::check_schema(&d.result_schema, &geo), vec![]);
    // The parameter and result shapes the command uses.
    let CValue::Obj(params) = get(&d.params_schema, "properties") else { panic!() };
    assert_eq!(params.keys().map(String::as_str).collect::<Vec<_>>(), ["at", "out"]);
    assert_eq!(get(get(get(&d.params_schema, "properties"), "out"), "path_like"), &CValue::Bool(true));
    let CValue::Obj(result) = get(&d.result_schema, "properties") else { panic!() };
    assert_eq!(
        result.keys().map(String::as_str).collect::<Vec<_>>(),
        ["entries", "path", "register_digest", "roster_digest"]
    );
}

// ------------------------------------------------------------------ AC-7, AC-9, AC-12 details

#[test]
fn ac7_forged_register_answers_unverified_for_every_rule() {
    let c = case("forged");
    let file = read("forged/register.json");
    let checked = verify_register(&file, &c.journal, &c.rosters, &c.coi, &c.published, Purpose::Replay);
    assert_eq!(checked.verification().code(), "register_mismatch");
    assert!(checked.register().is_none());
    let genuine = case("happy");
    let reg = verify_register(
        &read("happy/register.json"),
        &genuine.journal,
        &genuine.rosters,
        &genuine.coi,
        &genuine.published,
        Purpose::Evaluate,
    );
    for e in reg.register().unwrap().entries() {
        let a = status_of(Some(&checked), &e.rule, e.content_digest);
        assert_eq!((a.status, a.reason), (Status::Provisional, Some(Reason::UnverifiedRegister)));
        assert!(a.entry.is_none());
    }
}

#[test]
fn ac9_unknown_rule_unknown_digest_and_no_register() {
    let c = case("happy");
    let checked =
        verify_register(&read("happy/register.json"), &c.journal, &c.rosters, &c.coi, &c.published, Purpose::Evaluate);
    let known = &checked.register().unwrap().entries()[0];
    let a = status_of(Some(&checked), "dmt:asme/not_a_rule/none@1", known.content_digest);
    assert_eq!((a.status, a.reason), (Status::Provisional, Some(Reason::NotInRegister)));
    let a = status_of(Some(&checked), &known.rule, digest(&format!("sha256:{}", "5".repeat(64))));
    assert_eq!((a.status, a.reason), (Status::Provisional, Some(Reason::NotInRegister)));
    let a = status_of(None, &known.rule, known.content_digest);
    assert_eq!((a.status, a.reason), (Status::Provisional, Some(Reason::NoRegister)));
}

#[test]
fn register_not_published_and_foreign_pairing() {
    let c = case("happy");
    let file = read("happy/register.json");
    let main = roster("roster.json");
    let list = |reg: &str, ros: &str| {
        Published::load(
            format!(
                r#"{{"entries":[{{"register_digest":"{reg}","roster_digest":"{ros}"}}],"schema":"gdt-published/v1"}}"#
            )
            .as_bytes(),
        )
        .unwrap()
    };
    let other = format!("sha256:{}", "6".repeat(64));
    let p = list(&other, main.digest().as_str());
    let v = verify_register(&file, &c.journal, &c.rosters, &c.coi, &p, Purpose::Replay);
    assert_eq!(v.verification().code(), "register_not_published");
    // A listed register paired with another roster digest; the roster itself listed elsewhere.
    let reg_digest = st(&arr(&c.expected, "registers")[0], "digest").to_owned();
    let p = Published::load(
        format!(
            r#"{{"entries":[{{"register_digest":"{other}","roster_digest":"{}"}},{{"register_digest":"{reg_digest}","roster_digest":"{other}"}}],"schema":"gdt-published/v1"}}"#,
            main.digest().as_str()
        )
        .as_bytes(),
    )
    .unwrap();
    let v = verify_register(&file, &c.journal, &c.rosters, &c.coi, &p, Purpose::Replay);
    assert_eq!(v.verification().code(), "foreign_roster");
    // An unlisted extra roster handed to the verifier is refused too.
    let mut rosters = c.rosters.clone();
    rosters.push(roster("roster_foreign.json"));
    let v = verify_register(&file, &c.journal, &rosters, &c.coi, &c.published, Purpose::Replay);
    assert_eq!(v.verification().code(), "foreign_roster");
    // Garbage is a mismatch, not an error.
    let v = verify_register(b"not json", &c.journal, &c.rosters, &c.coi, &c.published, Purpose::Replay);
    assert_eq!(v.verification().code(), "register_mismatch");
    // Other conflict inputs give other bytes.
    let v = verify_register(&file, &c.journal, &c.rosters, &CoiInputs::none(), &c.published, Purpose::Replay);
    assert_eq!(v.verification().code(), "register_mismatch");
}

// ------------------------------------------------------------------ AC-10: the content digest

#[test]
fn ac10_content_digest_covers_every_field_but_review() {
    let doc = json("rows.json");
    let rows = get(&doc, "rows");
    let want = get(&doc, "content_digests");
    let d = |k: &str| row_content_digest(&row_from_value(get(rows, k)).unwrap()).unwrap();
    for k in ["a", "a_notes", "a_review", "a_flip"] {
        assert_eq!(d(k).as_str(), st(want, k), "{k}");
    }
    assert_ne!(d("a"), d("a_notes"));
    assert_eq!(d("a"), d("a_review"));
    assert_ne!(d("a"), d("a_flip"));
    assert_eq!(roster("roster.json").digest().as_str(), st(&doc, "roster_digest"));
    assert_eq!(roster("roster_foreign.json").digest().as_str(), st(&doc, "foreign_roster_digest"));
    assert_eq!(coi("loaded").digest().unwrap().as_str(), st(&doc, "coi_digest"));
    assert_eq!(coi("none").digest().unwrap().as_str(), st(&doc, "coi_digest_none"));
}

// ------------------------------------------------------------------ loaders (hand-labelled)

fn journal_err(text: &str) -> &'static str {
    JournalExtract::load(text.as_bytes()).unwrap_err().reason().unwrap()
}

#[test]
fn journal_extract_is_strict() {
    let h = |n: char| format!("sha256:{}", n.to_string().repeat(64));
    let entry = |hash: &str, utc: &str, extra: &str| {
        format!(
            r#"{{"command":"gdt.review_dmt_row@1","hash":"{hash}","principal":{{"id":"rev-one","kind":"human"}},"record":{{}},"utc":"{utc}"{extra}}}"#
        )
    };
    let doc = |es: &[String]| format!(r#"{{"entries":[{}],"schema":"gdt-rowflow-journal/v1"}}"#, es.join(","));
    let ok = doc(&[entry(&h('1'), "2026-11-20T09:00:00Z", "")]);
    assert_eq!(JournalExtract::load(ok.as_bytes()).unwrap().entries().len(), 1);
    assert_eq!(
        journal_err(&doc(&[entry(&h('1'), "2026-11-20T09:00:00Z", ""), entry(&h('1'), "2026-11-20T09:01:00Z", "")])),
        "duplicate_hash"
    );
    assert_eq!(
        journal_err(&doc(&[entry(&h('1'), "2026-11-20T09:01:00Z", ""), entry(&h('2'), "2026-11-20T09:00:00Z", "")])),
        "time_goes_back"
    );
    assert_eq!(journal_err(&doc(&[entry(&h('1'), "2026-11-20T09:00:00Z", r#","row":{}"#)])), "row_only_on_propose");
    assert_eq!(journal_err(&doc(&[entry(&h('1'), "2026-11-20T09:00:00Z", r#","extra":1"#)])), "unknown_field");
    assert_eq!(journal_err(&doc(&[entry(&h('1'), "2026-02-30T09:00:00Z", "")])), "bad_utc");
    assert_eq!(journal_err(&doc(&[entry("sha256:00", "2026-11-20T09:00:00Z", "")])), "bad_digest");
    assert_eq!(journal_err(r#"{"entries":[],"schema":"other"}"#), "wrong_schema");
    assert_eq!(journal_err(r#"{"entries":[],"entries":[],"schema":"gdt-rowflow-journal/v1"}"#), "duplicate_key");
    // An empty journal builds no register.
    let empty = JournalExtract::load(br#"{"entries":[],"schema":"gdt-rowflow-journal/v1"}"#).unwrap();
    let c = case("happy");
    let e = build_register(&empty, None, &roster("roster.json"), &trusted(&c), &c.coi).unwrap_err();
    assert_eq!(e, StatusError::JournalUnreadable { reason: "empty" });
    // Timestamps.
    assert!(Utc::parse("2024-02-29T23:59:59Z").is_some());
    for bad in [
        "2023-02-29T00:00:00Z",
        "2026-11-20 09:00:00Z",
        "2026-11-20T24:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-11-20T09:00:00",
    ] {
        assert!(Utc::parse(bad).is_none(), "{bad}");
    }
}

#[test]
fn published_list_is_strict_and_append_only() {
    let h = |n: char| format!("sha256:{}", n.to_string().repeat(64));
    let list = |pairs: &[(String, String)]| {
        let es: Vec<String> =
            pairs.iter().map(|(r, o)| format!(r#"{{"register_digest":"{r}","roster_digest":"{o}"}}"#)).collect();
        format!(r#"{{"entries":[{}],"schema":"gdt-published/v1"}}"#, es.join(","))
    };
    let one = Published::load(list(&[(h('1'), h('a'))]).as_bytes()).unwrap();
    let two = Published::load(list(&[(h('1'), h('a')), (h('2'), h('a'))]).as_bytes()).unwrap();
    let swapped = Published::load(list(&[(h('2'), h('a')), (h('1'), h('a'))]).as_bytes()).unwrap();
    assert!(two.check_extends(&one).is_ok());
    assert!(two.check_extends(&two).is_ok());
    assert_eq!(one.check_extends(&two).unwrap_err(), StatusError::PublishedUnreadable { reason: "not_append_only" });
    assert_eq!(swapped.check_extends(&one).unwrap_err().reason(), Some("not_append_only"));
    assert_eq!(two.latest().unwrap().register_digest, digest(&h('2')));
    let err = |t: &str| Published::load(t.as_bytes()).unwrap_err().reason().unwrap();
    assert_eq!(err(&list(&[(h('1'), h('a')), (h('1'), h('b'))])), "duplicate_register");
    assert_eq!(err(r#"{"entries":[],"schema":"gdt-published/v2"}"#), "wrong_schema");
    assert_eq!(
        err(r#"{"entries":[{"register_digest":"x","roster_digest":"y"}],"schema":"gdt-published/v1"}"#),
        "bad_digest"
    );
    assert_eq!(err(r#"{"entries":[],"extra":1,"schema":"gdt-published/v1"}"#), "unknown_field");
}

#[test]
fn trusted_rosters_admit_only_published_rosters() {
    let c = case("happy");
    let main = roster("roster.json");
    let foreign = roster("roster_foreign.json");
    assert_eq!(c.published.trusted_rosters(vec![foreign.clone()]).unwrap_err(), StatusError::ForeignRoster);
    assert_eq!(c.published.trusted_rosters(vec![main.clone(), foreign]).unwrap_err(), StatusError::ForeignRoster);
    let t = c.published.trusted_rosters(vec![main.clone()]).unwrap();
    assert_eq!(t.current().unwrap().digest(), main.digest());
    assert_eq!(t.get(main.digest()).unwrap().digest(), main.digest());
    // Without the latest entry's roster there is no current roster: transition then fails
    // closed (`unknown_roster`), and a review naming a missing roster is not applied.
    let none = c.published.trusted_rosters(Vec::new()).unwrap();
    assert!(none.current().is_none());
    let reg = build_register(&c.journal, None, &main, &none, &c.coi).unwrap();
    let row = &reg.entries()[0];
    assert_eq!((row.status, row.reason), (Status::Provisional, Some(Reason::AwaitingReview)));
    assert!(row.approvals.is_empty());
}

#[test]
fn conflict_inputs_are_strict() {
    let profiles = read("profiles.toml");
    let reviewers = |body: &str| CoiInputs::load(body.as_bytes(), &profiles);
    let ok = "[[reviewer]]\nid = \"r1\"\norg_id = \"org-1\"\nrecusals = []\n";
    assert!(reviewers(ok).is_ok());
    let err = |body: &str| reviewers(body).unwrap_err().reason().unwrap();
    // Duplicate IDs, also when they differ only in case; duplicate keys.
    assert_eq!(err(&format!("{ok}{ok}")), "duplicate_reviewer");
    assert_eq!(
        err(&format!("{ok}[[reviewer]]\nid = \"R1\"\norg_id = \"org-2\"\nrecusals = []\n")),
        "duplicate_reviewer"
    );
    assert_eq!(err("[[reviewer]]\nid = \"r1\"\nid = \"r2\"\norg_id = \"org-1\"\nrecusals = []\n"), "bad_toml");
    // Organisation IDs and recusals.
    assert_eq!(err("[[reviewer]]\nid = \"r1\"\norg_id = \"acme\"\nrecusals = []\n"), "bad_org_id");
    assert_eq!(err("[[reviewer]]\nid = \"r1\"\norg_id = \"org-1\"\n"), "missing_field");
    assert_eq!(err("[[reviewer]]\nid = \"r1\"\norg_id = \"org-1\"\nrecusals = [1]\n"), "bad_recusal");
    assert_eq!(err("schema = \"other/v1\"\n"), "wrong_schema");
    assert_eq!(err("other = 1\n"), "unknown_field");
    // Profiles: full dp: references, unique by profile ID.
    let p = |body: &str| CoiInputs::load(ok.as_bytes(), body.as_bytes());
    assert_eq!(
        p("[[profile]]\nid = \"dp:nope\"\nsponsor_org = \"org-1\"\n").unwrap_err().reason(),
        Some("bad_profile_id")
    );
    let loaded = coi("loaded");
    assert_eq!(loaded.sponsor_of("dp:asme-syn-2001@1.0.0"), Some("org-9"));
    assert_eq!(loaded.reviewer("R3").unwrap().unwrap().org_id, "org-1");
    assert!(loaded.reviewer("r5").unwrap().is_none());
    assert!(CoiInputs::none().reviewer("r1").is_none());
}

#[test]
fn public_register_files_hold_no_identities() {
    // Reviewers appear by roster ID only: no principal identity, display name or
    // organisation ID from the inputs reaches a register file.
    let mut seen = BTreeMap::new();
    for name in CASES {
        let c = case(name);
        for item in arr(&c.expected, "registers") {
            let text = String::from_utf8(read(&format!("{name}/{}", st(item, "file")))).unwrap();
            for needle in ["rev-", "author-one", "agent-one", "Fictional", "org-", "display_name"] {
                assert!(!text.contains(needle), "{name}: {needle}");
            }
            seen.insert(name, ());
        }
    }
    assert_eq!(seen.len(), CASES.len());
}

// ------------------------------------------------------------------ review follow-ups (hand-labelled)

#[test]
fn export_is_atomic_and_leaves_no_orphan() {
    let c = case("happy");
    let d = scratch("atomic");
    let roots = roots(&d);
    let main = roster("roster.json");
    let trusted = trusted(&c);
    let ctx = ExportContext { roots: &roots, journal: &c.journal, current: &main, trusted: &trusted, coi: &c.coi };
    // The digest target exists: nothing is written, not even the register.
    std::fs::write(d.join("a.json.sha256"), b"left over").unwrap();
    let e = export_rule_status(&ctx, Principal::Human, "root://exports/a.json", None).unwrap_err();
    assert_eq!(e, StatusError::ExportRootDenied { reason: "target_exists" });
    assert!(!d.join("a.json").exists());
    assert_eq!(std::fs::read(d.join("a.json.sha256")).unwrap(), b"left over");
    // The digest target is a folder: the second writer fails, the first leaves nothing.
    std::fs::create_dir(d.join("b.json.sha256")).unwrap();
    assert_eq!(
        export_rule_status(&ctx, Principal::Human, "root://exports/b.json", None).unwrap_err().code(),
        "export_root_denied"
    );
    assert!(!d.join("b.json").exists());
    // The register target exists: the digest file is not written either.
    std::fs::write(d.join("c.json"), b"old").unwrap();
    assert_eq!(
        export_rule_status(&ctx, Principal::Human, "root://exports/c.json", None).unwrap_err(),
        StatusError::ExportRootDenied { reason: "target_exists" }
    );
    assert!(!d.join("c.json.sha256").exists());
    // No temp files are left behind.
    let names: Vec<String> =
        std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().all(|n| !n.starts_with(".fid-tmp-")), "{names:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn recusals_must_be_rule_ids() {
    let profiles = read("profiles.toml");
    let with = |recusal: &str| {
        let body = format!("[[reviewer]]\nid = \"r1\"\norg_id = \"org-1\"\nrecusals = [\"{recusal}\"]\n");
        CoiInputs::load(body.as_bytes(), &profiles)
    };
    assert!(with("dmt:asme/flatness/none@1").is_ok());
    assert!(with("dp:asme-syn-2001@1.0.0").is_ok());
    for bad in [
        "DMT:asme/flatness/none@1",
        "dmt:ASME/flatness/none@1",
        "dmt:asme/flatness/none",
        "dp:asme-syn-2001@1.0",
        "dp:asme-syn-2001@1.0.0#sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "flatness",
        "",
    ] {
        assert_eq!(with(bad).unwrap_err(), StatusError::CoiInputsUnreadable { reason: "bad_recusal" }, "{bad}");
    }
}

#[test]
fn published_list_loads_only_as_an_extension_of_a_pinned_prior() {
    let c = case("currency");
    let bytes = read("currency/published.json");
    let full = Published::load(&bytes).unwrap();
    let first_only = Published::load(
        format!(
            r#"{{"entries":[{{"register_digest":"{}","roster_digest":"{}"}}],"schema":"gdt-published/v1"}}"#,
            full.entries()[0].register_digest.as_str(),
            full.entries()[0].roster_digest.as_str()
        )
        .as_bytes(),
    )
    .unwrap();
    assert_eq!(Published::load_extending(&bytes, &first_only).unwrap(), c.published);
    assert_eq!(Published::load_extending(&bytes, &full).unwrap(), full);
    // A list that drops its latest entry no longer extends the pinned copy.
    let truncated = format!(
        r#"{{"entries":[{{"register_digest":"{}","roster_digest":"{}"}}],"schema":"gdt-published/v1"}}"#,
        full.entries()[0].register_digest.as_str(),
        full.entries()[0].roster_digest.as_str()
    );
    assert_eq!(
        Published::load_extending(truncated.as_bytes(), &full).unwrap_err(),
        StatusError::PublishedUnreadable { reason: "not_append_only" }
    );
}

#[test]
fn register_records_its_trusted_rosters() {
    let c = case("happy");
    let checked =
        verify_register(&read("happy/register.json"), &c.journal, &c.rosters, &c.coi, &c.published, Purpose::Evaluate);
    let built = checked.register().unwrap().built_from();
    assert_eq!(built.trusted_rosters, [roster("roster.json").digest()]);
}

/// A checker for the JSON Schema subset the command definition uses (as in `datum.rs`). An
/// unknown keyword fails the test, so a schema change must extend this checker on purpose.
fn conforms(schema: &CValue, v: &CValue, at: &str) -> Result<(), String> {
    let CValue::Obj(s) = schema else { return Err(format!("{at}: schema is not an object")) };
    for (k, kv) in s {
        let ok = match (k.as_str(), kv) {
            ("description", _) | ("path_like", CValue::Bool(true)) => true,
            ("type", CValue::Str(t)) => match (t.as_str(), v) {
                ("object", CValue::Obj(_)) | ("string", CValue::Str(_)) => true,
                ("integer", CValue::Num(n)) => n.fract() == 0.0,
                _ => false,
            },
            ("pattern", CValue::Str(p)) => {
                if p != Digest::PATTERN {
                    return Err(format!("{at}: pattern {p} not supported by the test checker"));
                }
                matches!(v, CValue::Str(x) if Digest::parse(x).is_ok())
            }
            ("x-extensions", CValue::Arr(exts)) => {
                let CValue::Str(x) = v else { return Err(format!("{at}: x-extensions on a non-string")) };
                exts.iter().any(|e| matches!(e, CValue::Str(e) if x.to_ascii_lowercase().ends_with(&format!(".{e}"))))
            }
            ("minimum", CValue::Num(m)) => matches!(v, CValue::Num(n) if n >= m),
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
            (other, _) => return Err(format!("{at}: keyword {other} not supported by the test checker")),
        };
        if !ok {
            return Err(format!("{at}: fails {k}"));
        }
    }
    Ok(())
}

#[test]
fn params_and_results_conform_to_the_published_schemas() {
    let def = command_def().unwrap();
    let c = case("happy");
    let head = c.journal.head().unwrap();
    // Real parameters, as both surfaces send them.
    for raw in [
        r#"{"out":"root://exports/a.json"}"#.to_owned(),
        format!(r#"{{"at":"{}","out":"root://exports/b.json"}}"#, head.as_str()),
    ] {
        conforms(&def.params_schema, &parse(raw.as_bytes()).unwrap(), "params").unwrap();
    }
    // A real result.
    let d = scratch("schema");
    let roots = roots(&d);
    let main = roster("roster.json");
    let trusted = trusted(&c);
    let ctx = ExportContext { roots: &roots, journal: &c.journal, current: &main, trusted: &trusted, coi: &c.coi };
    let ex = export_rule_status(&ctx, Principal::Agent, "root://exports/r.json", None).unwrap();
    conforms(&def.result_schema, &parse(&ex.result_bytes().unwrap()).unwrap(), "result").unwrap();
    let _ = std::fs::remove_dir_all(&d);
    // Negative cases.
    for raw in [
        r#"{}"#,
        r#"{"out":"root://exports/a.txt"}"#,
        r#"{"out":"root://exports/a.json","at":"sha256:12"}"#,
        r#"{"out":"root://exports/a.json","extra":"x"}"#,
        r#"{"out":7}"#,
    ] {
        assert!(conforms(&def.params_schema, &parse(raw.as_bytes()).unwrap(), "params").is_err(), "{raw}");
    }
    for raw in [
        r#"{"entries":2,"path":"p","register_digest":"sha256:12","roster_digest":"sha256:12"}"#.to_owned(),
        format!(r#"{{"entries":-1,"path":"p","register_digest":"{h}","roster_digest":"{h}"}}"#, h = head.as_str()),
        format!(r#"{{"entries":2,"path":"p","register_digest":"{h}"}}"#, h = head.as_str()),
        format!(r#"{{"entries":1.5,"path":"p","register_digest":"{h}","roster_digest":"{h}"}}"#, h = head.as_str()),
    ] {
        assert!(conforms(&def.result_schema, &parse(raw.as_bytes()).unwrap(), "result").is_err(), "{raw}");
    }
}

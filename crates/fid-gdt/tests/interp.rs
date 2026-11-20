//! Acceptance criteria of spec node GDT.CHAR.INTERP (AC-1 .. AC-13).
//!
//! Every case under `fixtures/gdt/interp/` (rows, journals, registers, `gdt-published/v1`
//! lists and the expected interpretation block, error or replay outcome in `cases.json`) is
//! written and labelled by the independent stdlib-Python reference
//! `fixtures/gdt/interp/make_cases.py`, which builds the registers with the status reference
//! of `GDT.RES.STATUS`; the Rust crate never produced an expected value. The other fields of
//! each record come from `fixtures/gdt/record/flatness_inputs.json`; rosters and conflict
//! inputs from `fixtures/gdt/status/`. CLI, MCP, replay and branch checks are hand-labelled
//! from the leaf's rules. Synthetic data only; no standards text.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_gdt::chars::dprof::{parse_profile, CharId, KeyVocabulary, ProfileDoc};
use fid_gdt::chars::interp::{
    replay_interpretation, Interpretation, InterpretationPin, RegisterSources, RegisterStore, RegisterView, RuleStatus,
    MAX_RULE_ROWS, PROVISIONAL_LABEL_KEY, PROVISIONAL_LABEL_TEXT, REGISTER_UNAVAILABLE,
};
use fid_gdt::chars::record::{
    command_def, invoke_cli, invoke_mcp, invoke_mcp_tool, AlgorithmRef, EvaluationBasis, InputDigests, KernelVersion,
    Measurand, Quantity, RecordError, ResultRecord, ResultRecordBuilder, ResultStore, RunId, Uncertainty,
    GET_RESULT_CLI, GET_RESULT_MCP,
};
use fid_gdt::chars::spec::{GoverningSpec, SpecError};
use fid_gdt::dmt_flow::roster::Roster;
use fid_gdt::dmt_flow::status::{row_content_digest, CoiInputs, JournalExtract, Published, Purpose, RuleKind};
use fid_gdt::{requires_provisional_label, requires_provisional_label_bytes};
use fid_gps::dmt::row::row_from_value;
use fid_gps::dmt::{DmtKey, DmtRow};
use fid_io_reg::Principal;
use fid_kunc::Uec0Reason;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::path::{Path, PathBuf};

const H: Principal = Principal::Human;
const A: Principal = Principal::Agent;

// ------------------------------------------------------------------ fixtures

fn gdt() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(gdt().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
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

fn strs(v: &CValue, k: &str) -> Vec<String> {
    match get(v, k) {
        CValue::Arr(a) => a
            .iter()
            .map(|x| match x {
                CValue::Str(s) => s.clone(),
                other => panic!("{other:?}"),
            })
            .collect(),
        other => panic!("{k}: {other:?}"),
    }
}

fn has(v: &CValue, k: &str) -> bool {
    matches!(v, CValue::Obj(m) if m.contains_key(k))
}

fn cases() -> CValue {
    json("interp/cases.json")
}

fn case(name: &str) -> CValue {
    get(get(&cases(), "cases"), name).clone()
}

fn row(name: &str) -> DmtRow {
    row_from_value(get(get(&json("interp/rows.json"), "rows"), name)).unwrap()
}

fn vocab() -> KeyVocabulary {
    let v = json("dprof/vocabulary.json");
    let CValue::Arr(keys) = get(&v, "keys") else { panic!("keys") };
    KeyVocabulary::new(keys.iter().map(|k| match k {
        CValue::Str(s) => s.as_str(),
        _ => panic!("key"),
    }))
    .unwrap()
}

fn profile() -> ProfileDoc {
    parse_profile(&read("dprof/asme_placeholder_1.0.0.json"), &vocab()).unwrap()
}

fn coi() -> CoiInputs {
    CoiInputs::load(&read("status/reviewers.toml"), &read("status/profiles.toml")).unwrap()
}

/// Everything a register is checked against, owned.
struct Inputs {
    journal: JournalExtract,
    rosters: Vec<Roster>,
    coi: CoiInputs,
    published: Vec<u8>,
    prior: Published,
}

impl Inputs {
    fn of(c: &CValue) -> Inputs {
        Inputs::with_lists(c, st(c, "published"), st(c, "prior"))
    }

    fn with_lists(c: &CValue, published: &str, prior: &str) -> Inputs {
        Inputs {
            journal: JournalExtract::load(&read(&format!("interp/{}", st(c, "journal")))).unwrap(),
            rosters: strs(c, "rosters").iter().map(|r| Roster::load(&read(&format!("status/{r}"))).unwrap()).collect(),
            coi: coi(),
            published: read(&format!("interp/{published}")),
            // The pinned prior is the consumer's own trusted copy.
            prior: Published::load(&read(&format!("interp/{prior}"))).unwrap(),
        }
    }

    fn sources(&self) -> RegisterSources<'_> {
        RegisterSources {
            journal: &self.journal,
            rosters: &self.rosters,
            coi: &self.coi,
            published: &self.published,
            pinned_prior: &self.prior,
        }
    }
}

fn register_file(c: &CValue) -> Option<Vec<u8>> {
    match get(c, "register") {
        CValue::Null => None,
        CValue::Str(f) => Some(read(&format!("interp/{f}"))),
        other => panic!("{other:?}"),
    }
}

fn journal_of(c: &CValue) -> JournalExtract {
    JournalExtract::load(&read(&format!("interp/{}", st(c, "journal")))).unwrap()
}

fn view(c: &CValue, purpose: Purpose) -> RegisterView {
    match register_file(c) {
        None => RegisterView::none(&journal_of(c)),
        Some(file) => RegisterView::load(&file, &Inputs::of(c).sources(), purpose),
    }
}

fn rows_of(c: &CValue) -> Vec<DmtRow> {
    strs(c, "rows").iter().map(|n| row(n)).collect()
}

fn sha(bytes: &[u8]) -> Digest {
    Digest::from_bytes(fid_canon::digest_bytes(bytes).0)
}

/// The evaluated chain of the record inputs: the flatness row's default variant (the
/// synthetic position, datum-frame and size rows share it).
fn flat_chain() -> String {
    st(&json("record/flatness_inputs.json"), "operator_chain_digest").to_owned()
}

/// The non-interpretation fields of `fixtures/gdt/record/flatness_inputs.json`, with UEC-0
/// and no bound rule, for the characteristic `char`, primary row `primary` and evaluated
/// chain `chain`.
fn base(char: &str, primary: &str, chain: &str) -> ResultRecordBuilder {
    let o = json("record/flatness_inputs.json");
    fn structural(reason: &'static str) -> SpecError {
        SpecError::BadRequest { reason }
    }
    let bits = u64::from_str_radix(st(&o, "value").trim_start_matches("0x"), 16).unwrap();
    let digests = strs(&o, "input_digests").iter().map(|d| Digest::parse(d).unwrap()).collect();
    ResultRecordBuilder::new(RunId::parse(st(&o, "run")).unwrap(), CharId::parse(char).unwrap())
        .measurand(Measurand::parse(st(&o, "measurand")).unwrap())
        .evaluation_basis(EvaluationBasis::from_wire(st(&o, "evaluation_basis")).unwrap())
        .value(Quantity::new(f64::from_bits(bits), st(&o, "unit")).unwrap())
        .uncertainty(Uncertainty::uec0(Uec0Reason::UMissing))
        .bind_decision(None, None)
        .spec(GoverningSpec::from_cvalue(get(&o, "spec"), structural).unwrap())
        .algorithm(AlgorithmRef::parse(st(&o, "algorithm")).unwrap())
        .kernel_version(KernelVersion::parse(st(&o, "kernel_version")).unwrap())
        .input_digests(InputDigests::new(digests).unwrap())
        .journal_entry(Digest::parse(st(&o, "journal_entry")).unwrap())
        .dmt_row(DmtKey::parse(primary).unwrap())
        .operator_chain_digest(Digest::parse(chain).unwrap())
        .dmt_digest(Digest::parse(st(&o, "dmt_digest")).unwrap())
}

/// Builds the record of a case with the register view `v`.
fn build_with(c: &CValue, v: &RegisterView) -> Result<ResultRecord, RecordError> {
    let rows = rows_of(c);
    let refs: Vec<&DmtRow> = rows.iter().collect();
    let p = profile();
    let profile = if get(c, "profile") == &CValue::Bool(true) { Some(&p) } else { None };
    base("flat-1", st(c, "primary"), st(c, "operator_chain_digest")).interpretation(&refs, profile, v).finish()
}

fn build(name: &str) -> ResultRecord {
    let c = case(name);
    build_with(&c, &view(&c, Purpose::Evaluate)).unwrap()
}

fn record_json(r: &ResultRecord) -> CValue {
    parse(r.canonical_json()).unwrap()
}

// ------------------------------------------------------------------ every case against the reference

#[test]
fn every_case_matches_the_reference() {
    let all = cases();
    let CValue::Obj(cs) = get(&all, "cases") else { panic!("cases") };
    assert_eq!(cs.len(), 19);
    for (name, c) in cs {
        let want = get(c, "expect");
        let v = view(c, Purpose::Evaluate);
        let got = build_with(c, &v);
        if has(want, "error") {
            assert_eq!(got.map(|_| ()).unwrap_err().to_string(), st(want, "error"), "{name}");
            continue;
        }
        let r = got.unwrap_or_else(|e| panic!("{name}: {e}"));
        let j = record_json(&r);
        for k in ["interpretation", "interpretation_register", "interpretation_rules"] {
            assert_eq!(get(&j, k), get(want, k), "{name} {k}");
        }
        assert_eq!(v.code(), st(want, "verification"), "{name}");
        assert_eq!(r.interpretation_pin().verification(), st(want, "verification"), "{name}");
        assert_eq!(r.interpretation_pin().register(), r.interpretation_register(), "{name}");
        // Fail closed: signed_off only from a verified register.
        if !v.is_verified() {
            assert!(r.interpretation_rules().iter().all(|u| u.status() == RuleStatus::Provisional), "{name}");
        }
    }
}

// ------------------------------------------------------------------ AC-1 .. AC-7

#[test]
fn ac1_flatness_row_signed_off_with_recomputed_digest_and_register_digest() {
    let c = case("flatness_signed");
    let r = build("flatness_signed");
    let rules = r.interpretation_rules();
    assert_eq!(rules.len(), 2);
    let flat = row("flat");
    assert_eq!(rules[0].rule(), flat.key().as_str());
    assert_eq!(rules[0].kind(), RuleKind::DmtRow);
    assert_eq!(rules[0].revision(), Some(1));
    assert_eq!(rules[0].content_digest(), row_content_digest(&flat).unwrap());
    assert_eq!(rules[0].status(), RuleStatus::SignedOff);
    assert_eq!(rules[0].reason(), None);
    assert_eq!(rules[1].kind(), RuleKind::Profile);
    assert_eq!(rules[1].revision(), Some(1));
    assert_eq!(rules[1].content_digest(), *profile().digest());
    // Deviation from AC-1, stated in the PR: GDT.RES.STATUS REQ-006 gives every profile
    // `provisional`/`no_signoff_path`, so no verified register makes this record signed_off.
    assert_eq!(rules[1].status(), RuleStatus::Provisional);
    assert_eq!(r.interpretation(), Interpretation::Provisional);
    assert_eq!(r.interpretation_register(), Some(sha(&register_file(&c).unwrap())));
}

#[test]
fn ac2_position_mmc_lists_all_four_rules_with_their_own_statuses() {
    let r = build("position_mmc_mixed");
    let got: Vec<(&str, RuleStatus)> = r.interpretation_rules().iter().map(|u| (u.rule(), u.status())).collect();
    assert_eq!(
        got,
        [
            ("dmt:asme/datum_frame/rmb@1", RuleStatus::SignedOff),
            ("dmt:asme/position/mmc@1", RuleStatus::SignedOff),
            ("dmt:asme/size/none@1", RuleStatus::SignedOff),
            ("dp:asme-y14.5-2018@1.0.0", RuleStatus::Provisional),
        ]
    );
    assert_eq!(r.interpretation(), Interpretation::Provisional);
}

#[test]
fn ac3_tampered_row_is_recomputed_and_not_in_register() {
    let r = build("tampered_row");
    let tampered = row("flat_tampered");
    // The row's review metadata still claims the signed-off revision; the builder ignores it.
    assert!(format!("{:?}", tampered.review()).contains("SignedOff"));
    let u = &r.interpretation_rules()[0];
    assert_eq!(u.content_digest(), row_content_digest(&tampered).unwrap());
    assert_ne!(u.content_digest(), row_content_digest(&row("flat")).unwrap());
    assert_eq!((u.status(), u.reason().map(|x| x.as_str())), (RuleStatus::Provisional, Some("not_in_register")));
    assert_eq!(u.revision(), None);
    assert_eq!(r.interpretation(), Interpretation::Provisional);
}

#[test]
fn ac4_forged_register_and_foreign_roster_label_every_rule_unverified() {
    for name in ["forged_register", "foreign_roster"] {
        let r = build(name);
        assert!(r
            .interpretation_rules()
            .iter()
            .all(|u| u.status() == RuleStatus::Provisional
                && u.reason().map(|x| x.as_str()) == Some("unverified_register")));
        assert_eq!(r.interpretation(), Interpretation::Provisional);
    }
}

#[test]
fn ac5_draft_rule_is_not_admitted_and_nothing_is_emitted() {
    // The position row's latest revision is agent-authored (the person/agent rule of
    // ROWFLOW: an agent's row stays draft until a person re-proposes it).
    let c = case("draft_row");
    let e = build_with(&c, &view(&c, Purpose::Evaluate)).unwrap_err();
    assert_eq!(e.code(), "rule_not_admitted");
    assert_eq!(e.rule(), Some("dmt:asme/position/mmc@1"));
    // Without the agent's row the same evaluation builds.
    let flat = row("flat");
    let p = profile();
    let ok = base("flat-1", flat.key().as_str(), &flat_chain()).interpretation(
        &[&flat],
        Some(&p),
        &view(&c, Purpose::Evaluate),
    );
    assert!(ok.finish().is_ok());
}

#[test]
fn an_agent_draft_is_refused_with_no_register_or_an_unverified_one() {
    // Decision on #332: an agent-authored draft row is refused whatever the register state;
    // it is never emitted, not even as provisional.
    for (name, code) in [("draft_row_no_register", "none"), ("draft_row_unverified", "register_not_published")] {
        let c = case(name);
        let v = view(&c, Purpose::Evaluate);
        assert_eq!(v.code(), code, "{name}");
        let e = build_with(&c, &v).unwrap_err();
        assert_eq!(e.rule(), Some("dmt:asme/position/mmc@1"), "{name}");
    }
}

#[test]
fn the_primary_row_must_contain_the_evaluated_chain() {
    // The tampered chain evaluated with only the signed-off revision passed: refused.
    let c = case("chain_not_in_primary");
    let e = build_with(&c, &view(&c, Purpose::Evaluate)).unwrap_err();
    assert_eq!(e, RecordError::IncompleteResult { field: "interpretation_rules" });
    // The tampered chain passed alongside the signed-off revision of the same key: both are
    // listed, the tampered one not in the register, so the record is provisional.
    let r = build("tampered_with_signed");
    let flat: Vec<(RuleStatus, Option<&str>)> = r
        .interpretation_rules()
        .iter()
        .filter(|u| u.rule() == "dmt:asme/flatness/none@1")
        .map(|u| (u.status(), u.reason().map(|x| x.as_str())))
        .collect();
    assert_eq!(flat, [(RuleStatus::SignedOff, None), (RuleStatus::Provisional, Some("not_in_register"))]);
    assert!(requires_provisional_label(&r));
}

#[test]
fn an_empty_pinned_prior_is_refused() {
    let c = case("empty_prior");
    assert_eq!(view(&c, Purpose::Evaluate).code(), "empty_pinned_prior");
    let r = build("empty_prior");
    assert!(r.interpretation_rules().iter().all(|u| u.reason().map(|x| x.as_str()) == Some("unverified_register")));
}

#[test]
fn ac6_missing_rows_primary_or_profile_is_incomplete() {
    let want = RecordError::IncompleteResult { field: "interpretation_rules" };
    for name in ["no_rule_list", "primary_absent", "missing_profile"] {
        let c = case(name);
        assert_eq!(build_with(&c, &view(&c, Purpose::Evaluate)).unwrap_err(), want, "{name}");
    }
    // A profile other than the spec's defaults profile is not the characteristic's profile.
    let other = parse_profile(&read("dprof/asme_placeholder_1.1.0.json"), &vocab()).unwrap();
    let flat = row("flat");
    let b = base("flat-1", flat.key().as_str(), &flat_chain()).interpretation(
        &[&flat],
        Some(&other),
        &view(&case("no_register"), Purpose::Evaluate),
    );
    assert_eq!(b.finish().unwrap_err(), want);
}

#[test]
fn ac7_no_register_labels_every_rule_and_pins_none() {
    let r = build("no_register");
    assert!(r
        .interpretation_rules()
        .iter()
        .all(|u| u.status() == RuleStatus::Provisional && u.reason().map(|x| x.as_str()) == Some("no_register")));
    assert_eq!(r.interpretation_register(), None);
    assert_eq!(get(&record_json(&r), "interpretation_register"), &CValue::Str("none".to_owned()));
    assert_eq!(r.interpretation(), Interpretation::Provisional);
}

#[test]
fn a_register_checked_for_replay_is_not_used_by_a_new_evaluation() {
    let c = case("flatness_signed");
    let v = view(&c, Purpose::Replay);
    assert!(v.is_verified());
    assert_eq!(v.purpose(), Some(Purpose::Replay));
    let r = build_with(&c, &v).unwrap();
    assert!(r.interpretation_rules().iter().all(|u| u.reason().map(|x| x.as_str()) == Some("unverified_register")));
}

#[test]
fn rows_are_deduplicated_and_bounded() {
    let c = case("flatness_signed");
    let v = view(&c, Purpose::Evaluate);
    let flat = row("flat");
    let p = profile();
    let once =
        base("flat-1", flat.key().as_str(), &flat_chain()).interpretation(&[&flat], Some(&p), &v).finish().unwrap();
    let twice = base("flat-1", flat.key().as_str(), &flat_chain())
        .interpretation(&[&flat, &flat], Some(&p), &v)
        .finish()
        .unwrap();
    assert_eq!(once.canonical_json(), twice.canonical_json());
    let many: Vec<&DmtRow> = vec![&flat; MAX_RULE_ROWS + 1];
    let e =
        base("flat-1", flat.key().as_str(), &flat_chain()).interpretation(&many, Some(&p), &v).finish().unwrap_err();
    assert_eq!(e, RecordError::InvalidField { field: "interpretation_rules" });
}

// ------------------------------------------------------------------ AC-8, AC-13: replay

fn replay_with(
    r: &ResultRecord,
    c: &CValue,
    inputs: &Inputs,
    store: &RegisterStore,
) -> fid_gdt::chars::interp::InterpretationReplay {
    let rows = rows_of(c);
    let refs: Vec<&DmtRow> = rows.iter().collect();
    let p = profile();
    let pin = r.interpretation_pin().canonical_bytes().unwrap();
    replay_interpretation(r.canonical_json(), &pin, &refs, Some(&p), store, &inputs.sources())
}

#[test]
fn ac8_replay_with_the_pinned_register_is_identical_and_without_it_not_replayable() {
    let c = case("position_mmc_mixed");
    let r = build("position_mmc_mixed");
    let inputs = Inputs::of(&c);
    let mut store = RegisterStore::new();
    let d = store.put(&register_file(&c).unwrap()).unwrap();
    assert_eq!(Some(d), r.interpretation_register());
    let pin = parse(&r.interpretation_pin().canonical_bytes().unwrap()).unwrap();
    assert_eq!(st(&pin, "interpretation_register"), d.as_str());
    assert_eq!(st(&pin, "register_verification"), "verified");

    let ok = replay_with(&r, &c, &inputs, &store);
    assert_eq!((ok.outcome, ok.reason), (ReplayOutcome::Identical, None));

    store.remove(&d).unwrap();
    let gone = replay_with(&r, &c, &inputs, &store);
    assert_eq!((gone.outcome, gone.reason), (ReplayOutcome::NotReplayable, Some(REGISTER_UNAVAILABLE)));

    // A register that no longer verifies (another journal) is unavailable, never relabelled.
    let mut store = RegisterStore::new();
    store.put(&register_file(&c).unwrap()).unwrap();
    let other = Inputs::of(&case("flatness_signed"));
    let bad = replay_with(&r, &c, &other, &store);
    assert_eq!((bad.outcome, bad.reason), (ReplayOutcome::NotReplayable, Some(REGISTER_UNAVAILABLE)));

    // Different rows replay as divergent; a bad pin is not replayable.
    let flat = row("flat");
    let p = profile();
    let pin = r.interpretation_pin().canonical_bytes().unwrap();
    let div = replay_interpretation(r.canonical_json(), &pin, &[&flat], Some(&p), &store, &inputs.sources());
    assert_eq!(div.outcome, ReplayOutcome::Divergent);
    let junk = replay_interpretation(r.canonical_json(), b"{}", &[&flat], Some(&p), &store, &inputs.sources());
    assert_eq!((junk.outcome, junk.reason), (ReplayOutcome::NotReplayable, Some("bad_pin")));
}

#[test]
fn unverified_and_no_register_records_replay_their_fail_safe_labels() {
    let empty = RegisterStore::new();
    // No register pinned: nothing to load, the no_register labels replay.
    let c = case("no_register");
    let out = replay_with(&build("no_register"), &c, &Inputs::of(&c), &empty);
    assert_eq!(out.outcome, ReplayOutcome::Identical);
    // A pinned register must be in the store even when it did not verify; the fail-safe
    // unverified_register labels then replay.
    for name in ["forged_register", "currency_r1_stale"] {
        let c = case(name);
        let r = build(name);
        let out = replay_with(&r, &c, &Inputs::of(&c), &empty);
        assert_eq!((out.outcome, out.reason), (ReplayOutcome::NotReplayable, Some(REGISTER_UNAVAILABLE)), "{name}");
        let mut store = RegisterStore::new();
        store.put(&register_file(&c).unwrap()).unwrap();
        let out = replay_with(&r, &c, &Inputs::of(&c), &store);
        assert_eq!(out.outcome, ReplayOutcome::Identical, "{name}");
    }
}

#[test]
fn ac13_stale_register_is_unverified_for_evaluation_and_verifies_for_replay() {
    // R1 was the latest when the record was built: verified, pinned.
    let c = case("currency_r1_when_latest");
    let r = build("currency_r1_when_latest");
    assert_eq!(r.interpretation_pin().verification(), "verified");
    // R2 is published after it: a new evaluation with R1 is unverified (stale_register) ...
    let stale = case("currency_r1_stale");
    let v = view(&stale, Purpose::Evaluate);
    assert_eq!(v.code(), "stale_register");
    // ... while the journal of the record that pinned R1 replays IDENTICAL against the later
    // list, loaded with load_extending against the pinned prior.
    let later = get(&c, "replay_later_list");
    assert_eq!(st(later, "outcome"), "IDENTICAL");
    let inputs = Inputs::with_lists(&c, st(later, "published"), st(later, "prior"));
    let mut store = RegisterStore::new();
    store.put(&register_file(&c).unwrap()).unwrap();
    let out = replay_with(&r, &c, &inputs, &store);
    assert_eq!(out.outcome, ReplayOutcome::Identical);
    // A list that rewrites the pinned prior verifies nothing, not even the latest register.
    assert_eq!(view(&case("currency_rewritten_list"), Purpose::Evaluate).code(), "published_unreadable");
    let rewritten = Inputs::with_lists(&c, "currency/published_rewritten.json", st(later, "prior"));
    let out = replay_with(&r, &c, &rewritten, &store);
    assert_eq!((out.outcome, out.reason), (ReplayOutcome::NotReplayable, Some(REGISTER_UNAVAILABLE)));
}

// ------------------------------------------------------------------ AC-9 .. AC-12

fn lines(b: &[u8]) -> Vec<String> {
    String::from_utf8(b.to_vec()).unwrap().lines().map(str::to_owned).collect()
}

#[test]
fn ac9_cli_shows_the_label_in_text_and_json() {
    let r = build("position_mmc_mixed");
    let (run, ch) = (r.run().as_str().to_owned(), r.characteristic().as_str().to_owned());
    let mut store = ResultStore::new();
    store.commit(H, r).unwrap();
    let text = invoke_cli(&store, H, &[GET_RESULT_CLI, "--run", &run, "--char", &ch]).unwrap();
    assert!(lines(&text).iter().any(|l| l == PROVISIONAL_LABEL_TEXT));
    assert!(lines(&text).iter().any(|l| l == "interpretation: provisional"));
    assert_eq!(lines(&text).iter().filter(|l| l.starts_with("rule: ")).count(), 4);
    let json = invoke_cli(&store, H, &[GET_RESULT_CLI, "--run", &run, "--char", &ch, "--json"]).unwrap();
    assert!(String::from_utf8(json).unwrap().contains(r#""interpretation":"provisional""#));
    assert_eq!(PROVISIONAL_LABEL_KEY, "gdt.interpretation.provisional");
}

#[test]
fn ac10_mcp_structured_and_text_content_carry_the_label() {
    let r = build("position_mmc_mixed");
    let params = format!(r#"{{"run":"{}","char":"{}"}}"#, r.run().as_str(), r.characteristic().as_str());
    let mut store = ResultStore::new();
    store.commit(H, r).unwrap();
    let out = invoke_mcp_tool(&store, A, GET_RESULT_MCP, params.as_bytes()).unwrap();
    let s = parse(&out.structured).unwrap();
    assert_eq!(st(&s, "interpretation"), "provisional");
    assert!(matches!(get(&s, "interpretation_rules"), CValue::Arr(a) if a.len() == 4));
    assert!(out.text.lines().any(|l| l == "interpretation: provisional"));
    assert_eq!(out.structured, invoke_mcp(&store, A, GET_RESULT_MCP, params.as_bytes()).unwrap());
    // The structured result validates against the published result schema ...
    let schema = command_def().unwrap().result_schema;
    conforms(&schema, &s, "result").unwrap();
    // ... which requires the three fields, the enum and the record schema tag.
    for k in ["interpretation", "interpretation_rules", "interpretation_register", "schema"] {
        let CValue::Obj(mut m) = s.clone() else { panic!("object") };
        m.remove(k);
        assert!(conforms(&schema, &CValue::Obj(m), "result").is_err(), "{k}");
    }
    for (k, bad) in [("interpretation", "maybe"), ("schema", "fid.gdt-result/1")] {
        let CValue::Obj(mut m) = s.clone() else { panic!("object") };
        m.insert(k.to_owned(), CValue::Str(bad.to_owned()));
        assert!(conforms(&schema, &CValue::Obj(m), "result").is_err(), "{k}");
    }
}

/// A checker for the JSON Schema subset the result schema uses. An unknown keyword fails the
/// test, so a schema change must extend this checker on purpose.
fn conforms(schema: &CValue, v: &CValue, at: &str) -> Result<(), String> {
    let CValue::Obj(s) = schema else { return Err(format!("{at}: schema is not an object")) };
    for (k, kv) in s {
        let ok = match (k.as_str(), kv) {
            ("description", _) => true,
            ("type", CValue::Str(t)) => matches!(
                (t.as_str(), v),
                ("object", CValue::Obj(_)) | ("string", CValue::Str(_)) | ("array", CValue::Arr(_))
            ),
            ("enum", CValue::Arr(e)) => e.contains(v),
            ("required", CValue::Arr(r)) => {
                let CValue::Obj(o) = v else { return Err(format!("{at}: required on a non-object")) };
                r.iter().all(|k| matches!(k, CValue::Str(k) if o.contains_key(k)))
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
            (other, _) => return Err(format!("{at}: keyword {other} not supported by the test checker")),
        };
        if !ok {
            return Err(format!("{at}: fails {k}"));
        }
    }
    Ok(())
}

#[test]
fn ac11_requires_provisional_label_follows_interpretation() {
    let r = build("position_mmc_mixed");
    assert!(requires_provisional_label(&r));
    for name in ["flatness_signed", "no_register", "forged_register"] {
        let r = build(name);
        assert_eq!(requires_provisional_label(&r), r.interpretation() == Interpretation::Provisional);
    }
}

#[test]
fn the_bytes_predicate_is_provisional_unless_a_v2_record_is_signed_off() {
    let r = build("position_mmc_mixed");
    assert!(requires_provisional_label_bytes(r.canonical_json()));
    let canon = |v: &CValue| fid_canon::to_canonical_bytes(v).unwrap();
    // The same record with every rule and the label signed off (hand-edited bytes): false.
    let CValue::Obj(mut m) = record_json(&r) else { panic!("object") };
    m.insert("interpretation".to_owned(), CValue::Str("signed_off".to_owned()));
    let Some(CValue::Arr(rules)) = m.get_mut("interpretation_rules") else { panic!("rules") };
    for rule in rules.iter_mut() {
        let CValue::Obj(o) = rule else { panic!("rule") };
        o.insert("status".to_owned(), CValue::Str("signed_off".to_owned()));
    }
    let signed = CValue::Obj(m.clone());
    assert!(!requires_provisional_label_bytes(&canon(&signed)));
    // Anything else is provisional: a /1 record, a missing or other label, a provisional rule
    // under a signed-off label, an empty rule list, unreadable bytes.
    let edit = |k: &str, v: Option<CValue>| {
        let mut x = m.clone();
        match v {
            Some(v) => x.insert(k.to_owned(), v),
            None => x.remove(k),
        };
        canon(&CValue::Obj(x))
    };
    assert!(requires_provisional_label_bytes(&edit("schema", Some(CValue::Str("fid.gdt-result/1".to_owned())))));
    assert!(requires_provisional_label_bytes(&edit("interpretation", None)));
    assert!(requires_provisional_label_bytes(&edit("interpretation", Some(CValue::Str("SIGNED_OFF".to_owned())))));
    assert!(requires_provisional_label_bytes(&edit("interpretation", Some(CValue::Bool(true)))));
    assert!(requires_provisional_label_bytes(&edit("interpretation_rules", Some(CValue::Arr(Vec::new())))));
    assert!(requires_provisional_label_bytes(&edit("interpretation_rules", None)));
    let mut mixed = m.clone();
    let Some(CValue::Arr(rules)) = mixed.get_mut("interpretation_rules") else { panic!("rules") };
    let CValue::Obj(o) = &mut rules[0] else { panic!("rule") };
    o.insert("status".to_owned(), CValue::Str("provisional".to_owned()));
    assert!(requires_provisional_label_bytes(&canon(&CValue::Obj(mixed))));
    assert!(requires_provisional_label_bytes(b"not json"));
    assert!(requires_provisional_label_bytes(b"[]"));
    assert!(requires_provisional_label_bytes(b""));
}

#[test]
fn ac12_control_differs_only_in_the_type_state() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/compile_fail/");
    let fail = std::fs::read_to_string(format!("{dir}record_interp.rs")).unwrap();
    let ok = std::fs::read_to_string(format!("{dir}record_interp_control.rs")).unwrap();
    let a: Vec<&str> = fail.lines().collect();
    let b: Vec<&str> = ok.lines().collect();
    assert_eq!(a.len(), b.len());
    let diff: Vec<(&str, &str)> = a.iter().zip(&b).filter(|(x, y)| x != y).map(|(x, y)| (*x, *y)).collect();
    assert_eq!(
        diff,
        vec![(
            "fn serialise(b: ResultRecordBuilder<Uninterpreted>) -> Result<Vec<u8>, RecordError> {",
            "fn serialise(b: ResultRecordBuilder<Interpreted>) -> Result<Vec<u8>, RecordError> {"
        )]
    );
}

#[test]
fn the_canonical_json_always_holds_interpretation() {
    for name in ["flatness_signed", "no_register", "tampered_row", "currency_r2_latest"] {
        let j = record_json(&build(name));
        assert!(has(&j, "interpretation") && has(&j, "interpretation_rules") && has(&j, "interpretation_register"));
    }
}

// ------------------------------------------------------------------ person / agent branches

#[test]
fn agent_records_keep_the_label_and_never_reach_the_persons_branch() {
    let r = build("position_mmc_mixed");
    let (run, ch) = (r.run().as_str().to_owned(), r.characteristic().as_str().to_owned());
    let mut store = ResultStore::new();
    store.commit(A, r).unwrap();
    let agent = invoke_cli(&store, A, &[GET_RESULT_CLI, "--run", &run, "--char", &ch]).unwrap();
    assert!(lines(&agent).iter().any(|l| l == PROVISIONAL_LABEL_TEXT));
    // A person does not see the agent's record ...
    let e = invoke_cli(&store, H, &[GET_RESULT_CLI, "--run", &run, "--char", &ch]).unwrap_err();
    assert_eq!(e, RecordError::NotFound);
    // ... and their own record, on the working branch, carries its own label.
    store.commit(H, build("no_register")).unwrap();
    let person = invoke_cli(&store, H, &[GET_RESULT_CLI, "--run", &run, "--char", &ch]).unwrap();
    assert!(lines(&person).iter().any(|l| l == PROVISIONAL_LABEL_TEXT));
    assert!(lines(&person).iter().any(|l| l == "interpretation_register: none"));
    // The agent still reads its own record first.
    let again = invoke_cli(&store, A, &[GET_RESULT_CLI, "--run", &run, "--char", &ch, "--json"]).unwrap();
    assert!(String::from_utf8(again).unwrap().contains("position/mmc"));
}

#[test]
fn pins_round_trip() {
    for name in ["flatness_signed", "no_register", "forged_register"] {
        let r = build(name);
        let b = r.interpretation_pin().canonical_bytes().unwrap();
        assert_eq!(InterpretationPin::from_canonical_bytes(&b).as_ref(), Some(r.interpretation_pin()));
    }
}

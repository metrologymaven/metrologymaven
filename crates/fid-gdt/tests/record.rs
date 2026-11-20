//! Acceptance criteria of spec node GDT.CHAR.RECORD (AC-1 .. AC-7) plus requirement checks.
//!
//! Inputs and expected record bytes come from the independent stdlib-Python reference
//! `fixtures/gdt/record/make_cases.py`; error paths are hand-labelled from the leaf's rules.
//! The uncertainty statements are the `UNC.UEC.STMT` fixtures. The "UNC rule stub" of AC-1 is
//! this file: it plays the UNC decision rule and returns a `fid_kdec::Decision`. AC-7 itself
//! is the `compile_fail` doctest on `fid_gdt::chars::record`. Synthetic data only. Every
//! record carries the interpretation block of `GDT.CHAR.INTERP`, built here with no status
//! register (the block itself is tested in `tests/interp.rs`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::dprof::{parse_profile, CharId, KeyVocabulary, ProfileDoc};
use fid_gdt::chars::interp::RegisterView;
use fid_gdt::chars::record::{
    command_def, get_result, invoke_cli, invoke_mcp, AlgorithmRef, EvaluationBasis, InputDigests, Interpreted,
    KernelVersion, Measurand, NotDecidableReason, Quantity, RecordError, ResultRecord, ResultRecordBuilder,
    ResultStore, RunId, Uncertainty, GET_RESULT_CLI, GET_RESULT_COMMAND, GET_RESULT_MCP,
};
use fid_gdt::chars::spec::{GoverningSpec, SpecError};
use fid_gps::dmt::row::row_from_value;
use fid_gps::dmt::{DmtKey, DmtRow};
use fid_io_reg::Principal;
use fid_kdec::{Catalog, Decision, EvidenceBasis, NotDecidableReason as UncReason, Outcome, RuleRef};
use fid_kunc::{Uec0Reason, UncertaintyStatement};
use fid_types::Digest;
use std::collections::BTreeMap;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/gdt/record/");
const STMT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/stmt/valid/");
const H: Principal = Principal::Human;
const A: Principal = Principal::Agent;

type Obj = BTreeMap<String, CValue>;

fn inputs() -> Obj {
    let bytes = std::fs::read(format!("{FIX}flatness_inputs.json")).unwrap();
    match fid_canon::parse(&bytes).unwrap() {
        CValue::Obj(o) => o,
        _ => panic!("inputs"),
    }
}

fn s<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        _ => panic!("{k}"),
    }
}

fn expected(case: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}expected/{case}.json")).unwrap()
}

fn statement(name: &str) -> UncertaintyStatement {
    UncertaintyStatement::parse_canonical(&std::fs::read(format!("{STMT}{name}")).unwrap()).unwrap()
}

fn digest(s: &str) -> Digest {
    Digest::parse(s).unwrap()
}

fn spec(o: &Obj) -> GoverningSpec {
    fn structural(reason: &'static str) -> SpecError {
        SpecError::BadRequest { reason }
    }
    GoverningSpec::from_cvalue(o.get("spec").unwrap(), structural).unwrap()
}

fn stub_rule(o: &Obj) -> RuleRef {
    let CValue::Obj(stub) = o.get("rule_stub").unwrap() else { panic!("rule_stub") };
    RuleRef::parse(s(stub, "rule")).unwrap()
}

/// The UNC decision-rule stub: evaluates nothing, returns the outcome the fixture names,
/// built through `fid-kdec`'s checked constructor on the statement's evidence basis.
fn unc_stub(rule: &RuleRef, outcome: Outcome, stmt: &UncertaintyStatement) -> Decision {
    let rule = Catalog::builtin().unwrap().resolve(rule).unwrap();
    Decision::new(Some(&rule), outcome, Some(EvidenceBasis::of(stmt).unwrap())).unwrap()
}

const GDT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/gdt/");

/// The synthetic flatness row `dmt_row` names (`fixtures/gdt/status/rows.json`, row `a`).
fn flatness_row() -> DmtRow {
    let v = fid_canon::parse(&std::fs::read(format!("{GDT}status/rows.json")).unwrap()).unwrap();
    let CValue::Obj(o) = v else { panic!("rows") };
    let Some(CValue::Obj(rows)) = o.get("rows") else { panic!("rows") };
    row_from_value(rows.get("a").unwrap()).unwrap()
}

/// The defaults profile the spec names (`fixtures/gdt/dprof/asme_placeholder_1.0.0.json`).
fn profile() -> ProfileDoc {
    let v = fid_canon::parse(&std::fs::read(format!("{GDT}dprof/vocabulary.json")).unwrap()).unwrap();
    let CValue::Obj(o) = v else { panic!("vocabulary") };
    let Some(CValue::Arr(keys)) = o.get("keys") else { panic!("keys") };
    let keys: Vec<&str> = keys
        .iter()
        .map(|k| match k {
            CValue::Str(s) => s.as_str(),
            _ => panic!("key"),
        })
        .collect();
    let vocab = KeyVocabulary::new(keys).unwrap();
    parse_profile(&std::fs::read(format!("{GDT}dprof/asme_placeholder_1.0.0.json")).unwrap(), &vocab).unwrap()
}

/// Every field from the inputs except `skip`; uncertainty and decision as given; the
/// interpretation block with no register.
fn builder_with(
    o: &Obj,
    skip: &str,
    u: Option<Uncertainty>,
    bound: Option<&RuleRef>,
    d: Option<Decision>,
) -> ResultRecordBuilder<Interpreted> {
    let mut b =
        ResultRecordBuilder::new(RunId::parse(s(o, "run")).unwrap(), CharId::parse(s(o, "characteristic")).unwrap());
    let bits = u64::from_str_radix(s(o, "value").trim_start_matches("0x"), 16).unwrap();
    if skip != "measurand" {
        b = b.measurand(Measurand::parse(s(o, "measurand")).unwrap());
    }
    if skip != "evaluation_basis" {
        b = b.evaluation_basis(EvaluationBasis::from_wire(s(o, "evaluation_basis")).unwrap());
    }
    if skip != "value" {
        b = b.value(Quantity::new(f64::from_bits(bits), s(o, "unit")).unwrap());
    }
    if skip != "uncertainty" {
        if let Some(u) = u {
            b = b.uncertainty(u);
        }
    }
    if skip != "decision" {
        b = b.bind_decision(bound, d);
    }
    if skip != "spec" {
        b = b.spec(spec(o));
    }
    if skip != "algorithm" {
        b = b.algorithm(AlgorithmRef::parse(s(o, "algorithm")).unwrap());
    }
    if skip != "kernel_version" {
        b = b.kernel_version(KernelVersion::parse(s(o, "kernel_version")).unwrap());
    }
    if skip != "input_digests" {
        let Some(CValue::Arr(a)) = o.get("input_digests") else { panic!("input_digests") };
        let ds = a
            .iter()
            .map(|v| match v {
                CValue::Str(x) => digest(x),
                _ => panic!("digest"),
            })
            .collect();
        b = b.input_digests(InputDigests::new(ds).unwrap());
    }
    if skip != "journal_entry" {
        b = b.journal_entry(digest(s(o, "journal_entry")));
    }
    if skip != "dmt_row" {
        b = b.dmt_row(DmtKey::parse(s(o, "dmt_row")).unwrap());
    }
    if skip != "operator_chain_digest" {
        b = b.operator_chain_digest(digest(s(o, "operator_chain_digest")));
    }
    if skip != "dmt_digest" {
        b = b.dmt_digest(digest(s(o, "dmt_digest")));
    }
    let row = flatness_row();
    b.interpretation(&[&row], Some(&profile()), &RegisterView::none())
}

/// The AC-1 builder: UEC-2 statement, stub rule returning CONFORMS.
fn ac1_builder(skip: &str) -> ResultRecordBuilder<Interpreted> {
    let o = inputs();
    let stmt = statement(s(&o, "uncertainty_statement"));
    let rule = stub_rule(&o);
    let d = unc_stub(&rule, Outcome::Conforms, &stmt);
    builder_with(&o, skip, Some(Uncertainty::of(&stmt)), Some(&rule), Some(d))
}

fn ac1_record() -> ResultRecord {
    ac1_builder("").finish().unwrap()
}

#[test]
fn ac1_record_has_every_field_and_the_stub_rule() {
    let o = inputs();
    let r = ac1_record();
    assert_eq!(r.canonical_json(), expected("flatness_conforms").as_slice());
    assert_eq!(r.decision().rule(), Some(&stub_rule(&o)));
    assert_eq!(r.decision().outcome(), "CONFORMS");
    assert_eq!(r.dmt_row().as_str(), s(&o, "dmt_row"));
    assert_eq!(r.operator_chain_digest(), digest(s(&o, "operator_chain_digest")));
    assert_eq!(r.dmt_digest(), digest(s(&o, "dmt_digest")));
    assert_eq!(r.digest(), Digest::from_bytes(fid_canon::digest_bytes(r.canonical_json()).0));
    // Deterministic: the same inputs give the same bytes.
    assert_eq!(ac1_record().canonical_json(), r.canonical_json());
}

#[test]
fn ac2_missing_operator_chain_digest_is_incomplete() {
    assert_eq!(
        ac1_builder("operator_chain_digest").finish().unwrap_err(),
        RecordError::IncompleteResult { field: "operator_chain_digest" }
    );
    // Every other required field, the same way.
    for f in [
        "measurand",
        "evaluation_basis",
        "value",
        "uncertainty",
        "decision",
        "spec",
        "algorithm",
        "kernel_version",
        "input_digests",
        "journal_entry",
        "dmt_row",
        "dmt_digest",
    ] {
        let e = ac1_builder(f).finish().unwrap_err();
        assert_eq!(e, RecordError::IncompleteResult { field: f }, "{f}");
        assert_eq!(e.to_string(), format!("incomplete_result{{{f}}}"));
    }
}

#[test]
fn ac3_no_bound_rule_is_not_decidable() {
    let o = inputs();
    let stmt = statement(s(&o, "uncertainty_statement"));
    let r = builder_with(&o, "", Some(Uncertainty::of(&stmt)), None, None).finish().unwrap();
    assert_eq!(r.canonical_json(), expected("no_rule_bound").as_slice());
    assert_eq!(r.decision().outcome(), "NOT_DECIDABLE");
    assert_eq!(r.decision().reason(), Some("no_rule_bound"));
    assert_eq!(r.decision().gdt_reason(), Some(NotDecidableReason::NoRuleBound));
    assert!(r.decision().rule().is_none());
}

#[test]
fn bound_rule_without_unc_decision_is_unc_no_decision() {
    let o = inputs();
    let stmt = statement(s(&o, "uncertainty_statement"));
    let rule = stub_rule(&o);
    let r = builder_with(&o, "", Some(Uncertainty::of(&stmt)), Some(&rule), None).finish().unwrap();
    assert_eq!(r.decision().outcome(), "NOT_DECIDABLE");
    assert_eq!(r.decision().reason(), Some("unc_no_decision"));
    assert_eq!(r.decision().rule(), Some(&rule));
    assert!(r.decision().unc().is_none());
}

#[test]
fn ac4_uec0_with_a_value_is_refused() {
    let o = inputs();
    let valued = Uncertainty::of(&statement("uec2_lpu.json"));
    for (first, second) in [
        (Uncertainty::uec0(Uec0Reason::UMissing), valued.clone()),
        (valued.clone(), Uncertainty::uec0(Uec0Reason::UMissing)),
        (Uncertainty::of(&statement("uec0_u_missing.json")), valued.clone()),
    ] {
        let b = builder_with(&o, "", Some(first), None, None).uncertainty(second);
        assert_eq!(b.finish().unwrap_err(), RecordError::Uec0WithValue);
    }
    // Two different evaluated statements: changed, not uec0_with_value.
    let b =
        builder_with(&o, "", Some(valued), None, None).uncertainty(Uncertainty::of(&statement("uec3_iso15530_3.json")));
    assert_eq!(b.finish().unwrap_err(), RecordError::UncertaintyChanged);
}

#[test]
fn uec0_record_has_no_value_and_takes_uncs_not_decidable() {
    let o = inputs();
    let stmt = statement("uec0_u_missing.json");
    let rule = stub_rule(&o);
    let catalog_rule = Catalog::builtin().unwrap().resolve(&rule).unwrap();
    let d = Decision::new(
        Some(&catalog_rule),
        Outcome::NotDecidable(UncReason::UMissing),
        Some(EvidenceBasis::of(&stmt).unwrap()),
    )
    .unwrap();
    let r = builder_with(&o, "", Some(Uncertainty::of(&stmt)), Some(&rule), Some(d)).finish().unwrap();
    assert!(!r.uncertainty().has_value());
    assert_eq!(r.decision().outcome(), "NOT_DECIDABLE");
    assert_eq!(r.decision().reason(), Some("u_missing"));
    let text = String::from_utf8(r.canonical_json().to_vec()).unwrap();
    assert!(text.contains(r#""expanded":null"#) && text.contains(r#""u_std":null"#), "{text}");
    assert!(text.contains(r#""interval":null"#));
}

#[test]
fn basis_passes_through_and_cannot_change() {
    let o = inputs();
    let stmt = statement("uec2_lpu.json");
    for basis in EvaluationBasis::ALL {
        let r = builder_with(&o, "evaluation_basis", Some(Uncertainty::of(&stmt)), None, None)
            .evaluation_basis(basis)
            .finish()
            .unwrap();
        assert_eq!(r.evaluation_basis(), basis);
        let text = String::from_utf8(r.canonical_json().to_vec()).unwrap();
        assert!(text.contains(&format!(r#""evaluation_basis":"{}""#, basis.wire())));
    }
    // The same basis twice is fine; an upgrade (or any change) is refused.
    let same = ac1_builder("").evaluation_basis(EvaluationBasis::Points);
    assert!(same.finish().is_ok());
    let up = ac1_builder("").evaluation_basis(EvaluationBasis::Features);
    assert_eq!(up.finish().unwrap_err(), RecordError::BasisChanged);
    // The first error is kept.
    let both =
        ac1_builder("").evaluation_basis(EvaluationBasis::ResultsOnly).value(Quantity::new(1.0, "si:m").unwrap());
    assert_eq!(both.finish().unwrap_err(), RecordError::BasisChanged);
}

#[test]
fn ac5_second_record_for_same_run_and_char_is_duplicate() {
    let mut store = ResultStore::new();
    store.commit(H, ac1_record()).unwrap();
    assert_eq!(store.commit(H, ac1_record()).unwrap_err(), RecordError::DuplicateResult);
    // Different content, same (run, characteristic): still a duplicate.
    let o = inputs();
    let other = builder_with(&o, "", Some(Uncertainty::of(&statement("uec2_lpu.json"))), None, None).finish().unwrap();
    assert_eq!(store.commit(H, other).unwrap_err(), RecordError::DuplicateResult);
    assert_eq!(store.len(), 1);
}

#[test]
fn agent_records_never_reach_the_working_branch() {
    let o = inputs();
    let mut store = ResultStore::new();
    store.commit(A, ac1_record()).unwrap();
    let (run, ch) = (s(&o, "run"), s(&o, "characteristic"));
    assert_eq!(get_result(&store, H, run, ch).unwrap_err(), RecordError::NotFound);
    assert_eq!(get_result(&store, Principal::Script, run, ch).unwrap(), ac1_record().canonical_json());
    assert_eq!(store.commit(Principal::Script, ac1_record()).unwrap_err(), RecordError::DuplicateResult);
    // The person's own record is separate.
    store.commit(H, ac1_record()).unwrap();
    assert_eq!(store.len(), 2);
}

#[test]
fn ac6_get_result_prints_the_stored_canonical_json() {
    assert_eq!(GET_RESULT_COMMAND, "gdt.get_result@1");
    let o = inputs();
    let mut store = ResultStore::new();
    let record = ac1_record();
    let stored = record.canonical_json().to_vec();
    store.commit(H, record).unwrap();
    let out = get_result(&store, H, s(&o, "run"), s(&o, "characteristic")).unwrap();
    assert_eq!(out, stored.as_slice());
    assert_eq!(out, expected("flatness_conforms").as_slice());
    assert_eq!(get_result(&store, H, "run-other", "flat-1").unwrap_err(), RecordError::NotFound);
    assert_eq!(get_result(&store, H, s(&o, "run"), "flat-2").unwrap_err(), RecordError::NotFound);
    assert_eq!(get_result(&store, H, "", "flat-1").unwrap_err(), RecordError::InvalidField { field: "run" });
    assert_eq!(get_result(&store, H, "r", "-x").unwrap_err(), RecordError::InvalidField { field: "char" });
}

#[test]
fn ac7_control_differs_only_in_the_binding_line() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/compile_fail/");
    let fail = std::fs::read_to_string(format!("{dir}record_outcome.rs")).unwrap();
    let ok = std::fs::read_to_string(format!("{dir}record_outcome_control.rs")).unwrap();
    let a: Vec<&str> = fail.lines().collect();
    let b: Vec<&str> = ok.lines().collect();
    assert_eq!(a.len(), b.len());
    let diff: Vec<(&str, &str)> = a.iter().zip(&b).filter(|(x, y)| x != y).map(|(x, y)| (*x, *y)).collect();
    assert_eq!(diff, vec![("    b.outcome(fid_kdec::Outcome::Conforms)", "    b.bind_decision(None, None)")]);
}

#[test]
fn claimed_value_never_looks_measured() {
    let o = inputs();
    let stmt = statement("uec1_declared.json");
    let rule = stub_rule(&o);
    let d = unc_stub(&rule, Outcome::Conforms, &stmt);
    let r = builder_with(&o, "", Some(Uncertainty::of(&stmt)), Some(&rule), Some(d)).finish().unwrap();
    assert_eq!(r.canonical_json(), expected("claimed_conforms").as_slice());
    assert!(r.uncertainty().is_claimed());
    assert!(r.uncertainty().rests_on_declared_input() && r.uncertainty().rests_on_uncharacterized_input());
    let basis = r.decision().basis().unwrap();
    assert!(basis.is_claimed() && basis.rests_on_declared_input() && basis.rests_on_uncharacterized_input());
}

#[test]
fn nonconforms_and_indeterminate_pass_through_unchanged() {
    let o = inputs();
    let stmt = statement("uec2_lpu.json");
    let rule = stub_rule(&o);
    for (outcome, wire) in [(Outcome::Nonconforms, "NONCONFORMS"), (Outcome::Indeterminate, "INDETERMINATE")] {
        let d = unc_stub(&rule, outcome, &stmt);
        let r = builder_with(&o, "", Some(Uncertainty::of(&stmt)), Some(&rule), Some(d)).finish().unwrap();
        assert_eq!(r.decision().outcome(), wire);
        assert!(r.decision().reason().is_none());
    }
}

#[test]
fn decision_must_name_the_bound_rule() {
    let o = inputs();
    let stmt = statement("uec2_lpu.json");
    let rule = stub_rule(&o);
    let d = unc_stub(&rule, Outcome::Conforms, &stmt);
    // A decision with no bound rule.
    let b = builder_with(&o, "", Some(Uncertainty::of(&stmt)), None, Some(d.clone()));
    assert_eq!(b.finish().unwrap_err(), RecordError::DecisionRuleMismatch);
    // A decision of another rule.
    let iso = RuleRef::parse("dr:iso14253-1:2017/default@1").unwrap();
    let b = builder_with(&o, "", Some(Uncertainty::of(&stmt)), Some(&iso), Some(d.clone()));
    assert_eq!(b.finish().unwrap_err(), RecordError::DecisionRuleMismatch);
    // A rule-less UNC decision (kdec's rule_missing) under a bound rule.
    let none = Decision::new(None, Outcome::NotDecidable(UncReason::RuleMissing), None).unwrap();
    let b = builder_with(&o, "", Some(Uncertainty::of(&stmt)), Some(&rule), Some(none));
    assert_eq!(b.finish().unwrap_err(), RecordError::DecisionRuleMismatch);
    // Binding twice, differently.
    let b = ac1_builder("").bind_decision(None, None);
    assert_eq!(b.finish().unwrap_err(), RecordError::FieldChanged { field: "decision" });
}

#[test]
fn decision_must_rest_on_the_records_statement() {
    let o = inputs();
    let rule = stub_rule(&o);
    // CONFORMS on an evaluated statement, but the record says UEC-0.
    let d = unc_stub(&rule, Outcome::Conforms, &statement("uec2_lpu.json"));
    let b = builder_with(&o, "", Some(Uncertainty::uec0(Uec0Reason::UMissing)), Some(&rule), Some(d.clone()));
    assert_eq!(b.finish().unwrap_err(), RecordError::DecisionBasisMismatch);
    // CONFORMS on a claimed statement, the record names an evaluated one.
    let claimed = unc_stub(&rule, Outcome::Conforms, &statement("uec1_declared.json"));
    let stmt = statement("uec2_lpu.json");
    let b = builder_with(&o, "", Some(Uncertainty::of(&stmt)), Some(&rule), Some(claimed));
    assert_eq!(b.finish().unwrap_err(), RecordError::DecisionBasisMismatch);
}

#[test]
fn unbound_guard_band_never_decides() {
    let o = inputs();
    let stmt = statement("uec2_lpu.json");
    let template = RuleRef::parse("dr:guard-band@1").unwrap();
    let rule = Catalog::builtin().unwrap().get(&template.id, 1).unwrap().clone();
    // fid-kdec refuses every side under the unbound template ...
    for outcome in [Outcome::Conforms, Outcome::Nonconforms, Outcome::Indeterminate] {
        assert!(Decision::new(Some(&rule), outcome, Some(EvidenceBasis::of(&stmt).unwrap())).is_err());
    }
    // ... so the record can only say it was not decided.
    let r = builder_with(&o, "", Some(Uncertainty::of(&stmt)), Some(&template), None).finish().unwrap();
    assert_eq!(r.decision().outcome(), "NOT_DECIDABLE");
    assert_eq!(r.decision().reason(), Some("unc_no_decision"));
}

#[test]
fn unit_must_match_the_statement() {
    let b = ac1_builder("value").value(Quantity::new(0.001, "si:rad").unwrap());
    assert_eq!(b.finish().unwrap_err(), RecordError::UnitMismatch);
}

#[test]
fn field_grammars_fail_closed() {
    let inv = |field| RecordError::InvalidField { field };
    assert_eq!(RunId::parse("").unwrap_err(), inv("run"));
    assert_eq!(RunId::parse(&"r".repeat(129)).unwrap_err(), inv("run"));
    assert_eq!(RunId::parse("run 1").unwrap_err(), inv("run"));
    assert!(RunId::parse(&"r".repeat(128)).is_ok());
    assert_eq!(Measurand::parse("").unwrap_err(), inv("measurand"));
    assert_eq!(Measurand::parse(" lead").unwrap_err(), inv("measurand"));
    assert_eq!(Measurand::parse("tab\there").unwrap_err(), inv("measurand"));
    assert_eq!(Measurand::parse(&"m".repeat(257)).unwrap_err(), inv("measurand"));
    for bad in ["gdt.form@1.0.0", "gdt.form.flat@1.0", "gdt.form.flat@01.0.0", "Gdt.form.flat@1.0.0", "gdt.form.flat"] {
        assert_eq!(AlgorithmRef::parse(bad).unwrap_err(), inv("algorithm"), "{bad}");
    }
    let a = AlgorithmRef::parse("gdt.form.flatness.minzone@1.20.3").unwrap();
    assert_eq!((a.id(), a.version()), ("gdt.form.flatness.minzone", "1.20.3"));
    for bad in ["", "1", "1.0", "1.0.0.0", "1.00.0", "a.b.c", "1234567890.0.0"] {
        assert_eq!(KernelVersion::parse(bad).unwrap_err(), inv("kernel_version"), "{bad}");
    }
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, f64::from_bits(1)] {
        assert_eq!(Quantity::new(v, "si:m").unwrap_err(), inv("value"), "{v}");
    }
    assert_eq!(Quantity::new(1.0, "mm").unwrap_err(), inv("unit"));
    assert!(Quantity::new(-0.5, "si:m").is_ok() && Quantity::new(0.0, "si:1").is_ok());
    assert_eq!(InputDigests::new(Vec::new()).unwrap_err(), inv("input_digests"));
    let d = Digest::from_bytes([7; 32]);
    assert_eq!(InputDigests::new(vec![d, d]).unwrap_err(), RecordError::DuplicateInputDigest);
    assert_eq!(InputDigests::new(vec![d; 1025]).unwrap_err(), inv("input_digests"));
    assert_eq!(EvaluationBasis::from_wire("points"), None);
}

#[test]
fn other_fields_cannot_change_either() {
    let b = ac1_builder("").measurand(Measurand::parse("something else").unwrap());
    assert_eq!(b.finish().unwrap_err(), RecordError::FieldChanged { field: "measurand" });
    let b = ac1_builder("").operator_chain_digest(Digest::from_bytes([1; 32]));
    assert_eq!(b.finish().unwrap_err(), RecordError::FieldChanged { field: "operator_chain_digest" });
}

// ------------------------------------------------------------------ review of #256

#[test]
fn person_then_agent_is_a_duplicate_and_the_agent_reads_the_persons_result() {
    let o = inputs();
    let (run, ch) = (s(&o, "run"), s(&o, "characteristic"));
    let mut store = ResultStore::new();
    store.commit(H, ac1_record()).unwrap();
    // The person's result is visible to agents and scripts ...
    assert_eq!(get_result(&store, A, run, ch).unwrap(), ac1_record().canonical_json());
    assert_eq!(get_result(&store, Principal::Script, run, ch).unwrap(), ac1_record().canonical_json());
    // ... so neither may add a second record for the same (run, characteristic).
    assert_eq!(store.commit(A, ac1_record()).unwrap_err(), RecordError::DuplicateResult);
    assert_eq!(store.commit(Principal::Script, ac1_record()).unwrap_err(), RecordError::DuplicateResult);
    assert_eq!(store.len(), 1);
}

#[test]
fn agent_then_person_is_accepted_and_the_agent_record_is_shadowed() {
    let o = inputs();
    let (run, ch) = (s(&o, "run"), s(&o, "characteristic"));
    let mut store = ResultStore::new();
    // The agent's proposal: same (run, characteristic), different content.
    let proposal =
        builder_with(&o, "", Some(Uncertainty::of(&statement("uec2_lpu.json"))), None, None).finish().unwrap();
    store.commit(A, proposal.clone()).unwrap();
    assert_eq!(get_result(&store, H, run, ch).unwrap_err(), RecordError::NotFound);
    store.commit(H, ac1_record()).unwrap();
    // The person sees only the working record; the agent still sees its own first.
    assert_eq!(get_result(&store, H, run, ch).unwrap(), ac1_record().canonical_json());
    assert_eq!(get_result(&store, A, run, ch).unwrap(), proposal.canonical_json());
    // And the person cannot commit twice.
    assert_eq!(store.commit(H, ac1_record()).unwrap_err(), RecordError::DuplicateResult);
    assert_eq!(store.len(), 2);
}

/// GDT never decides conformance: no source file of this crate builds a UNC decision or
/// writes an outcome that states a side or INDETERMINATE.
#[test]
fn sources_never_construct_a_decision_or_an_outcome() {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    assert!(files.len() > 10, "scan found too few files");
    let banned = [
        "Decision::new(",
        "Outcome::Conforms",
        "Outcome::Nonconforms",
        "Outcome::Indeterminate",
        "Outcome::from_wire(",
    ];
    let mut hits = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        for (n, line) in text.lines().enumerate() {
            let flat: String = line.chars().filter(|c| !c.is_whitespace()).collect();
            for b in banned {
                if flat.contains(b) {
                    hits.push(format!("{}:{}: {b}", f.display(), n + 1));
                }
            }
        }
    }
    assert!(hits.is_empty(), "{hits:#?}");
}

#[test]
fn a_unc_decision_without_basis_cannot_ignore_the_records_statement() {
    let o = inputs();
    let rule = stub_rule(&o);
    let catalog = Catalog::builtin().unwrap();
    let simple = catalog.resolve(&rule).unwrap();
    let no_basis = Decision::new(Some(&simple), Outcome::NotDecidable(UncReason::UMissing), None).unwrap();
    // The record carries an evaluated statement, the decision ignores it.
    let b =
        builder_with(&o, "", Some(Uncertainty::of(&statement("uec2_lpu.json"))), Some(&rule), Some(no_basis.clone()));
    assert_eq!(b.finish().unwrap_err(), RecordError::DecisionBasisMismatch);
    // Without a statement in the record, the same decision is fine.
    let r = builder_with(&o, "", Some(Uncertainty::uec0(Uec0Reason::UMissing)), Some(&rule), Some(no_basis))
        .finish()
        .unwrap();
    assert_eq!(r.decision().reason(), Some("u_missing"));
    // label_only needs no uncertainty: allowed with a statement in the record.
    let label = RuleRef::parse("dr:as-reported-by-source@1").unwrap();
    let label_rule = catalog.resolve(&label).unwrap();
    let d = Decision::new(Some(&label_rule), Outcome::NotDecidable(UncReason::LabelOnly), None).unwrap();
    let r = builder_with(&o, "", Some(Uncertainty::of(&statement("uec2_lpu.json"))), Some(&label), Some(d))
        .finish()
        .unwrap();
    assert_eq!(r.decision().reason(), Some("label_only"));
    // GDT's own reasons carry no UNC decision and stay allowed (AC-3, unc_no_decision).
    let stmt = Some(Uncertainty::of(&statement("uec2_lpu.json")));
    assert!(builder_with(&o, "", stmt.clone(), None, None).finish().is_ok());
    assert!(builder_with(&o, "", stmt, Some(&rule), None).finish().is_ok());
}

#[test]
fn cli_and_mcp_return_the_stored_bytes() {
    let o = inputs();
    let (run, ch) = (s(&o, "run"), s(&o, "characteristic"));
    let mut store = ResultStore::new();
    store.commit(H, ac1_record()).unwrap();
    let want = expected("flatness_conforms");
    let cli = invoke_cli(&store, H, &[GET_RESULT_CLI, "--run", run, "--char", ch, "--json"]).unwrap();
    assert_eq!(cli, want);
    let params = format!(r#"{{"run":"{run}","char":"{ch}"}}"#);
    assert_eq!(invoke_mcp(&store, H, GET_RESULT_MCP, params.as_bytes()).unwrap(), want);
    // Agents read the person's result through MCP too.
    assert_eq!(invoke_mcp(&store, A, GET_RESULT_MCP, params.as_bytes()).unwrap(), want);
    let code = |r: Result<Vec<u8>, RecordError>| r.unwrap_err().to_string();
    assert_eq!(code(invoke_cli(&store, H, &[GET_RESULT_CLI, "--run", "nope", "--char", ch])), "not_found");
    assert_eq!(code(invoke_cli(&store, H, &[])), "bad_params{missing subcommand}");
    assert_eq!(code(invoke_cli(&store, H, &["get-results"])), "bad_params{unknown subcommand}");
    assert_eq!(code(invoke_cli(&store, H, &[GET_RESULT_CLI, "--run", run])), "bad_params{missing parameter}");
    assert_eq!(code(invoke_cli(&store, H, &[GET_RESULT_CLI, "--run"])), "bad_params{option needs a value}");
    assert_eq!(
        code(invoke_cli(&store, H, &[GET_RESULT_CLI, "--run", run, "--run", run, "--char", ch])),
        "bad_params{repeated option}"
    );
    assert_eq!(code(invoke_cli(&store, H, &[GET_RESULT_CLI, "--json", "--json"])), "bad_params{repeated option}");
    assert_eq!(code(invoke_cli(&store, H, &[GET_RESULT_CLI, "--x", "1"])), "bad_params{unknown option}");
    assert_eq!(code(invoke_cli(&store, H, &[GET_RESULT_CLI, "stray"])), "bad_params{unexpected argument}");
    assert_eq!(code(invoke_cli(&store, H, &["--json"; 9])), "bad_params{too many arguments}");
    assert_eq!(code(invoke_mcp(&store, H, "gdt_get_results", params.as_bytes())), "bad_params{unknown tool}");
    assert_eq!(code(invoke_mcp(&store, H, GET_RESULT_MCP, b"[]")), "bad_params{params is not an object}");
    assert_eq!(code(invoke_mcp(&store, H, GET_RESULT_MCP, br#"{"run":"a","run":"b"}"#)), "bad_params{duplicate key}");
    assert_eq!(
        code(invoke_mcp(&store, H, GET_RESULT_MCP, br#"{"run":"a","char":1}"#)),
        "bad_params{parameter is not a string}"
    );
    assert_eq!(
        code(invoke_mcp(&store, H, GET_RESULT_MCP, br#"{"run":"a","char":"c","x":"y"}"#)),
        "bad_params{unknown parameter}"
    );
    assert_eq!(
        code(invoke_mcp(&store, H, GET_RESULT_MCP, br#"{"run":"a","char":{"x":1}}"#)),
        "bad_params{params too large or too deep}"
    );
    let big = vec![b' '; 5000];
    assert_eq!(code(invoke_mcp(&store, H, GET_RESULT_MCP, &big)), "bad_params{params too large or too deep}");
}

#[test]
fn command_definition_is_r0_d0_on_cli_and_mcp() {
    let d = command_def().unwrap();
    assert_eq!(format!("{}@{}", d.id, d.major), GET_RESULT_COMMAND);
    assert_eq!(d.side_effect.as_str(), "R0");
    assert_eq!(d.determinism.as_str(), "D0");
}

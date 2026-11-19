//! Acceptance criteria of spec node UNC.UEC.CLASS (AC-1 .. AC-5) plus requirement and
//! maintainer-rule checks.
//!
//! Expected classes, reasons, labels and errors come from the independent Python reference
//! `fixtures/unc/class/make_table.py` (stdlib only) and its hand labels; none come from this
//! crate. The test only decodes each row into an input and a record store.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kunc::uec::{
    assign, AssignError, AssignInput, AssignOutput, Basis, ContributorSource, DowngradeReason, Origin, RecordFacts,
    RecordRef, RecordTable, SourceClaim, SourceRecords, ALGORITHM_ID, MAX_CONTRIBUTORS, RULE_SET,
};
use fid_kunc::{
    Contributor, EvidenceClass, Expanded, Method, ModelRef, PrincipalKind, SourceKind, SourceRef, StatementParts,
    StmtError, Uec0Reason, UncertaintyStatement,
};
use fid_types::Digest;

const TABLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/class/table.csv");

fn digest_of(label: &str) -> Digest {
    fid_kci::kci::sha256_digest(label.as_bytes())
}

fn facts(word: &str) -> (SourceKind, Option<Origin>) {
    match word {
        "dcc" => (SourceKind::Dcc, None),
        "qualification" => (SourceKind::Qualification, None),
        "type_a" => (SourceKind::TypeA, None),
        "declared" => (SourceKind::Declared, None),
        "im_iemps" => (SourceKind::InstrumentModel, Some(Origin::Iemps)),
        "im_spec" => (SourceKind::InstrumentModel, Some(Origin::Specification)),
        "im" => (SourceKind::InstrumentModel, None),
        "dcc_iemps" => (SourceKind::Dcc, Some(Origin::Iemps)),
        other => panic!("unknown claim word {other}"),
    }
}

fn flag(s: &str) -> bool {
    match s {
        "true" => true,
        "false" => false,
        other => panic!("bad flag {other}"),
    }
}

struct Row {
    case: String,
    basis: Result<Basis, AssignError>,
    input: Option<AssignInput>,
    records: RecordTable,
    expect: Vec<String>,
}

fn decode(cols: &[&str]) -> Row {
    assert_eq!(cols.len(), 14, "{cols:?}");
    let case = cols[0].to_owned();
    let basis = Basis::parse(cols[1]);
    let mut entries = Vec::new();
    let mut contributor_sources = Vec::new();
    if cols[4] != "-" {
        for item in cols[4].split(';') {
            let parts: Vec<&str> = item.split(':').collect();
            // Optional fourth field: the record label, so two contributors can share one record.
            let (id, claim, record, label) = match *parts.as_slice() {
                [id, claim, record] => (id, claim, record, id),
                [id, claim, record, label] => (id, claim, record, label),
                _ => panic!("{item}"),
            };
            let digest = digest_of(&format!("source:{label}"));
            let source = if claim == "none" {
                None
            } else {
                let (kind, origin) = facts(claim);
                Some(SourceClaim { kind, origin, digest })
            };
            if claim != "none" && record != "absent" {
                let (kind, origin) = facts(if record == "same" { claim } else { record });
                let f = RecordFacts::Source { kind, origin };
                match entries.iter().find(|e: &&(Digest, RecordFacts)| e.0 == digest) {
                    Some(e) => assert_eq!(e.1, f, "{item}: a shared record has one kind"),
                    None => entries.push((digest, f)),
                }
            }
            contributor_sources.push(ContributorSource { input_id: id.to_owned(), source });
        }
    }
    let vdigest = digest_of(&format!("validation:{case}"));
    let validation_record = match cols[5] {
        "-" => None,
        "ok" => {
            entries.push((vdigest, RecordFacts::Validation));
            Some(RecordRef { digest: vdigest })
        }
        "absent" => Some(RecordRef { digest: vdigest }),
        "wrong" => {
            entries.push((vdigest, RecordFacts::Source { kind: SourceKind::Dcc, origin: None }));
            Some(RecordRef { digest: vdigest })
        }
        other => panic!("bad validation {other}"),
    };
    let input = basis.ok().map(|basis| AssignInput {
        basis,
        contributor_sources,
        validation_record,
        model_bound: flag(cols[2]),
        instruments_bound: flag(cols[3]),
    });
    let records = RecordTable::new(entries).unwrap();
    Row { case, basis, input, records, expect: cols[6..].iter().map(|s| (*s).to_owned()).collect() }
}

fn rows() -> Vec<Row> {
    let text = std::fs::read_to_string(TABLE).unwrap();
    let mut lines = text.lines();
    assert!(lines.next().unwrap().starts_with("case,basis,"));
    lines.filter(|l| !l.is_empty()).map(|l| decode(&l.split(',').collect::<Vec<_>>())).collect()
}

fn row(name: &str) -> Row {
    rows().into_iter().find(|r| r.case == name).unwrap_or_else(|| panic!("no row {name}"))
}

fn err_text(e: AssignError) -> String {
    match e.detail() {
        Some(d) => format!("{}:{d}", e.code()),
        None => e.code().to_owned(),
    }
}

fn tf(b: bool) -> String {
    b.to_string()
}

fn render(r: &Result<AssignOutput, AssignError>) -> Vec<String> {
    match r {
        Ok(o) => vec![
            o.class().wire().to_owned(),
            o.uec0_reason().map_or("-", Uec0Reason::wire).to_owned(),
            o.downgrade().map_or("-", |d| d.reason.code()).to_owned(),
            o.downgrade().and_then(|d| d.input_id.clone()).unwrap_or_else(|| "-".to_owned()),
            tf(o.is_claimed()),
            tf(o.rests_on_declared_input()),
            tf(o.rests_on_uncharacterized_input()),
            "-".to_owned(),
        ],
        Err(e) => {
            let mut v = vec!["-".to_owned(); 7];
            v.push(err_text(*e));
            v
        }
    }
}

fn run(r: &Row) -> Result<AssignOutput, AssignError> {
    match (&r.basis, &r.input) {
        (Err(e), _) => Err(*e),
        (Ok(_), Some(input)) => assign(input, &r.records),
        (Ok(_), None) => unreachable!(),
    }
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_decision_table_matches_reference() {
    let rows = rows();
    assert!(rows.len() >= 60, "table has {} rows", rows.len());
    for r in &rows {
        assert_eq!(render(&run(r)), r.expect, "row {}", r.case);
    }
    // Every basis, class, UEC-0 reason and downgrade reason occurs in the table.
    let all: Vec<Vec<String>> = rows.iter().map(|r| r.expect.clone()).collect();
    for b in Basis::ALL {
        assert!(rows.iter().any(|r| r.basis == Ok(*b)), "basis {b} missing");
    }
    for c in EvidenceClass::ALL {
        assert!(all.iter().any(|e| e[0] == c.wire()), "class {c} missing");
    }
    for u in Uec0Reason::ALL {
        assert!(all.iter().any(|e| e[1] == u.wire()), "reason {u} missing");
    }
    for d in DowngradeReason::ALL {
        assert!(all.iter().any(|e| e[2] == d.code()), "downgrade {d} missing");
    }
}

// ------------------------------------------------------------------ AC-2 .. AC-4

#[test]
fn ac2_mcm_one_declared_is_uec1_naming_that_contributor() {
    let r = row("mcm_one_declared");
    assert_eq!(r.input.as_ref().unwrap().contributor_sources.len(), 5);
    let o = run(&r).unwrap();
    assert_eq!(o.class(), EvidenceClass::Uec1);
    let d = o.downgrade().unwrap();
    assert_eq!(d.reason, DowngradeReason::UnsourcedContributor);
    assert_eq!(d.input_id.as_deref(), Some("c3"));
    assert!(o.rests_on_declared_input() && o.rests_on_uncharacterized_input() && !o.is_claimed());
}

#[test]
fn ac3_substitution_without_validation_is_uec2_not_validated() {
    let o = run(&row("subst_no_validation")).unwrap();
    assert_eq!(o.class(), EvidenceClass::Uec2);
    let d = o.downgrade().unwrap();
    assert_eq!((d.reason, d.input_id.as_deref()), (DowngradeReason::NotValidated, None));
}

#[test]
fn ac4_modelled_without_contributors_fails_empty_model() {
    for case in ["modelled_empty", "modelled_lpu_empty", "modelled_mcm_empty", "iso15530_3_empty"] {
        assert_eq!(run(&row(case)), Err(AssignError::EmptyModel), "{case}");
    }
}

// ------------------------------------------------------------------ AC-5

fn parts_for(o: &AssignOutput, sources: Vec<SourceRef>) -> StatementParts {
    let mut p = StatementParts::new(EvidenceClass::Uec0);
    p.method = o.basis().method();
    if p.method.is_some() {
        p.unit = Some("si:m".to_owned());
        p.u_std = Some(1.0e-6);
        p.expanded = Some(Expanded { value: 2.0e-6, k: 2.0 });
        p.model = Some(ModelRef {
            id: "um:bore-diameter".to_owned(),
            semver: "1.2.0".to_owned(),
            digest: digest_of("um:bore-diameter@1.2.0"),
        });
        p.sources = sources;
        p.contributors = o
            .contributor_ids()
            .iter()
            .map(|id| Contributor {
                input_id: id.clone(),
                sensitivity: Some(1.0),
                u_contrib: 1.0e-7,
                variance_share: 0.5,
            })
            .collect();
    }
    if p.method == Some(Method::Declared) {
        p.provenance.principal = Some("person:alice".to_owned());
        p.provenance.principal_kind = Some(PrincipalKind::Human);
    }
    p
}

fn src(kind: SourceKind, id: &str) -> SourceRef {
    SourceRef { kind, id: id.to_owned(), digest: digest_of(&format!("source:{id}")), unverified: false }
}

#[test]
fn ac5_same_input_gives_identical_output_and_provenance() {
    for r in rows() {
        assert_eq!(run(&r), run(&r), "row {}", r.case);
        // Input order does not matter: contributors are scanned in input_id order.
        if let Some(input) = &r.input {
            let mut rev = input.clone();
            rev.contributor_sources.reverse();
            assert_eq!(assign(&rev, &r.records), run(&r), "row {} reversed", r.case);
        }
    }
    let r = row("modelled_lpu_all_dcc");
    let o = run(&r).unwrap();
    let sources = vec![src(SourceKind::Dcc, "a"), src(SourceKind::Dcc, "b"), src(SourceKind::Dcc, "c")];
    let s1 = o.statement(parts_for(&o, sources.clone())).unwrap();
    let s2 = run(&r).unwrap().statement(parts_for(&o, sources)).unwrap();
    assert_eq!(s1.canonical_json(), s2.canonical_json());
    assert_eq!(s1.digest(), s2.digest());
    assert_eq!(s1.class(), EvidenceClass::Uec2);
}

// ------------------------------------------------------------------ requirements

#[test]
fn req006_statement_records_rule_set() {
    assert_eq!((RULE_SET, ALGORITHM_ID), ("uec-assign@1", "unc.uec.assign@1.0.0"));
    for (case, sources) in [
        ("none_u_missing", vec![]),
        ("declared_bare", vec![]),
        ("iso15530_4_all_dcc", vec![src(SourceKind::Dcc, "a"), src(SourceKind::Dcc, "b"), src(SourceKind::Dcc, "c")]),
    ] {
        let o = run(&row(case)).unwrap();
        assert_eq!((o.rule(), o.algorithm()), (RULE_SET, ALGORITHM_ID));
        let s = o.statement(parts_for(&o, sources)).unwrap();
        assert_eq!(s.provenance().rule_sets, vec![RULE_SET.to_owned()], "{case}");
        assert_eq!(s.class(), o.class());
        assert_eq!(s.uec0_reason(), o.uec0_reason());
    }
}

#[test]
fn claims_are_never_promoted() {
    // A declared value with a genuine DCC behind it stays UEC-1 and claimed.
    let o = run(&row("declared_dcc_source")).unwrap();
    assert_eq!(o.class(), EvidenceClass::Uec1);
    assert!(o.is_claimed());
    let s = o.statement(parts_for(&o, vec![])).unwrap();
    assert!(s.is_claimed());
    // A statement whose method differs from the basis is refused, so a claim cannot be
    // re-labelled as an evaluation (or the reverse).
    let mut p = parts_for(&o, vec![src(SourceKind::Dcc, "a")]);
    p.method = Some(Method::Lpu);
    assert_eq!(o.statement(p), Err(AssignError::Inconsistent { field: "method" }));
}

#[test]
fn records_never_upgrade_a_claim() {
    // Declared claim, record says dcc: still unsourced; dcc claim, record says declared: unverified.
    let o = run(&row("modelled_mcm_declared_claim_dcc_record")).unwrap();
    assert_eq!(o.class(), EvidenceClass::Uec1);
    assert_eq!(o.downgrade().unwrap().reason, DowngradeReason::UnsourcedContributor);
    let o = run(&row("modelled_mcm_dcc_claim_declared_record")).unwrap();
    assert_eq!(o.downgrade().unwrap().reason, DowngradeReason::UnverifiedSource);
    assert!(o.rests_on_declared_input());
}

#[test]
fn unverified_source_cannot_be_written_as_characterized() {
    // The claim says dcc but no record confirms it: UEC-1, and a statement that lists the
    // source as dcc would hide the uncharacterized input, so it is refused.
    let o = run(&row("modelled_lpu_one_record_absent")).unwrap();
    assert_eq!(o.class(), EvidenceClass::Uec1);
    let sources = vec![src(SourceKind::Dcc, "a"), src(SourceKind::Dcc, "b"), src(SourceKind::Dcc, "c")];
    assert_eq!(o.statement(parts_for(&o, sources)), Err(AssignError::SourceNotVerified));
    // Leaving the unverified input out hides it, so that is refused too.
    let sources = vec![src(SourceKind::Dcc, "a"), src(SourceKind::Dcc, "c")];
    assert_eq!(
        o.statement(parts_for(&o, sources)),
        Err(AssignError::LabelDropped { field: "rests_on_uncharacterized_input" })
    );
    // The UEC-2 attempt: an unverified mark never rides on a UEC-2/3 statement, neither
    // through CLASS (an assignment above UEC-1 cites no unverified claim) nor through the
    // public constructor or the parser (class_mismatch{sources.unverified}).
    let o2 = run(&row("modelled_lpu_all_dcc")).unwrap();
    assert_eq!(o2.class(), EvidenceClass::Uec2);
    assert!(o2.unverified_claims().is_empty());
    let sources = vec![src(SourceKind::Dcc, "a"), unverified(src(SourceKind::Dcc, "b")), src(SourceKind::Dcc, "c")];
    assert_eq!(o2.statement(parts_for(&o2, sources.clone())), Err(AssignError::SourceNotVerified));
    let mut p = parts_for(&o2, sources);
    p.class = EvidenceClass::Uec2;
    p.provenance.rule_sets.push(RULE_SET.to_owned());
    assert_eq!(UncertaintyStatement::new(p), Err(StmtError::ClassMismatch { field: "sources.unverified" }));
    let good = o2.statement(parts_for(&o2, vec![src(SourceKind::Dcc, "a"), src(SourceKind::Dcc, "c")]));
    let text = String::from_utf8(good.unwrap().canonical_json().to_vec()).unwrap();
    let forged = text.replacen("\"kind\":\"dcc\"}", "\"kind\":\"dcc\",\"unverified\":\"true\"}", 1);
    assert_ne!(forged, text);
    assert_eq!(
        UncertaintyStatement::parse_canonical(forged.as_bytes()),
        Err(StmtError::ClassMismatch { field: "sources.unverified" })
    );
}

fn unverified(s: SourceRef) -> SourceRef {
    SourceRef { unverified: true, ..s }
}

#[test]
fn unverified_source_is_written_as_an_honest_uec1_statement() {
    // The same downgrade, written honestly: the unverified claim is cited with the mark, and
    // the statement carries the uncharacterized label and uec-assign@1 provenance.
    let o = run(&row("modelled_lpu_one_record_absent")).unwrap();
    assert_eq!(o.class(), EvidenceClass::Uec1);
    assert_eq!(o.unverified_claims(), [(SourceKind::Dcc, digest_of("source:b"))]);
    let sources = vec![src(SourceKind::Dcc, "a"), unverified(src(SourceKind::Dcc, "b")), src(SourceKind::Dcc, "c")];
    let s = o.statement(parts_for(&o, sources)).unwrap();
    assert_eq!(s.class(), EvidenceClass::Uec1);
    assert!(s.rests_on_uncharacterized_input() && !s.rests_on_declared_input());
    assert_eq!(s.provenance().rule_sets, vec![RULE_SET.to_owned()]);
    let back = UncertaintyStatement::parse_canonical(s.canonical_json()).unwrap();
    assert_eq!(back, s);
    // Only the offending contributor may be marked: a verified claim marked unverified, or
    // a mark on a digest nobody claimed, is refused.
    let sources = vec![unverified(src(SourceKind::Dcc, "a")), unverified(src(SourceKind::Dcc, "b"))];
    assert_eq!(o.statement(parts_for(&o, sources)), Err(AssignError::SourceNotVerified));
    let sources = vec![unverified(src(SourceKind::Dcc, "b")), unverified(src(SourceKind::Dcc, "z"))];
    assert_eq!(o.statement(parts_for(&o, sources)), Err(AssignError::SourceNotVerified));
    // ... and under its claimed kind (or its record's kind), not any other.
    let sources = vec![unverified(src(SourceKind::TypeA, "b"))];
    assert_eq!(o.statement(parts_for(&o, sources)), Err(AssignError::SourceNotVerified));

    // A dcc claim whose record says declared: cited under the record's kind it carries the
    // declared label too; under the claimed kind it would drop that label.
    let o = run(&row("modelled_lpu_dcc_claim_declared_record")).unwrap();
    assert!(o.rests_on_declared_input());
    let sources = vec![src(SourceKind::Dcc, "a"), unverified(src(SourceKind::Dcc, "b")), src(SourceKind::Dcc, "c")];
    assert_eq!(
        o.statement(parts_for(&o, sources)),
        Err(AssignError::LabelDropped { field: "rests_on_declared_input" })
    );
    let sources =
        vec![src(SourceKind::Dcc, "a"), unverified(src(SourceKind::Declared, "b")), src(SourceKind::Dcc, "c")];
    let s = o.statement(parts_for(&o, sources)).unwrap();
    assert!(s.rests_on_declared_input() && s.rests_on_uncharacterized_input());
    assert_eq!(s.class(), EvidenceClass::Uec1);

    // A declared input must stay visible as declared: re-labelled it is not a verified
    // claim, left out it drops the label.
    let o = run(&row("modelled_lpu_one_declared")).unwrap();
    let sources = vec![src(SourceKind::Dcc, "a"), src(SourceKind::InstrumentModel, "b"), src(SourceKind::Dcc, "c")];
    assert_eq!(o.statement(parts_for(&o, sources)), Err(AssignError::SourceNotVerified));
    let sources = vec![src(SourceKind::Dcc, "a"), src(SourceKind::Dcc, "c")];
    assert_eq!(
        o.statement(parts_for(&o, sources)),
        Err(AssignError::LabelDropped { field: "rests_on_declared_input" })
    );
    let sources = vec![src(SourceKind::Dcc, "a"), src(SourceKind::Declared, "b"), src(SourceKind::Dcc, "c")];
    let s = o.statement(parts_for(&o, sources)).unwrap();
    assert!(s.rests_on_declared_input() && s.rests_on_uncharacterized_input());
    assert_eq!(s.class(), EvidenceClass::Uec1);
}

#[test]
fn verified_parse_rechecks_uec2_and_uec3_sources_against_records() {
    assert_eq!(StmtError::SourceNotVerified.code(), "source_not_verified");
    let r = row("iso15530_4_all_dcc");
    let o = run(&r).unwrap();
    assert_eq!(o.class(), EvidenceClass::Uec3);
    let sources = vec![src(SourceKind::Dcc, "a"), src(SourceKind::Dcc, "b"), src(SourceKind::Dcc, "c")];
    let s = o.statement(parts_for(&o, sources)).unwrap();
    let bytes = s.canonical_json();
    assert_eq!(UncertaintyStatement::parse_canonical_verified(bytes, &r.records).unwrap(), s);
    // The same bytes against a store that lacks one record, or holds it under another
    // kind, are refused even though they name uec-assign@1.
    let d_b = digest_of("source:b");
    let dcc = RecordFacts::Source { kind: SourceKind::Dcc, origin: None };
    let missing = RecordTable::new(vec![(digest_of("source:a"), dcc), (digest_of("source:c"), dcc)]).unwrap();
    assert_eq!(UncertaintyStatement::parse_canonical_verified(bytes, &missing), Err(StmtError::SourceNotVerified));
    let other_kind = RecordTable::new(vec![
        (digest_of("source:a"), dcc),
        (d_b, RecordFacts::Source { kind: SourceKind::TypeA, origin: None }),
        (digest_of("source:c"), dcc),
    ])
    .unwrap();
    assert_eq!(UncertaintyStatement::parse_canonical_verified(bytes, &other_kind), Err(StmtError::SourceNotVerified));
    let with_origin = RecordTable::new(vec![
        (digest_of("source:a"), dcc),
        (d_b, RecordFacts::Source { kind: SourceKind::Dcc, origin: Some(Origin::Iemps) }),
        (digest_of("source:c"), dcc),
    ])
    .unwrap();
    assert_eq!(UncertaintyStatement::parse_canonical_verified(bytes, &with_origin), Err(StmtError::SourceNotVerified));
    // Parse errors come first and are unchanged.
    assert_eq!(UncertaintyStatement::parse_canonical_verified(b"{}", &r.records).unwrap_err().code(), "malformed");
    // UEC-0/1 statements are returned as parsed, whatever the records say.
    let o = run(&row("modelled_lpu_one_record_absent")).unwrap();
    let sources = vec![src(SourceKind::Dcc, "a"), unverified(src(SourceKind::Dcc, "b")), src(SourceKind::Dcc, "c")];
    let s = o.statement(parts_for(&o, sources)).unwrap();
    let empty = RecordTable::new(vec![]).unwrap();
    assert_eq!(UncertaintyStatement::parse_canonical_verified(s.canonical_json(), &empty).unwrap(), s);
}

#[test]
fn statement_errors_are_passed_through() {
    let o = run(&row("modelled_lpu_all_dcc")).unwrap();
    let mut p = parts_for(&o, vec![src(SourceKind::Dcc, "a")]);
    p.unit = None;
    let e = o.statement(p).unwrap_err();
    assert_eq!((e.code(), e.detail()), ("statement_refused", Some("statement_incomplete")));
}

#[test]
fn record_table_refuses_duplicate_digests() {
    let d = digest_of("source:a");
    let f = RecordFacts::Source { kind: SourceKind::Dcc, origin: None };
    assert_eq!(RecordTable::new(vec![(d, f), (d, f)]), Err(AssignError::DuplicateRecord));
    let t = RecordTable::new(vec![(d, f)]).unwrap();
    assert_eq!((t.len(), t.lookup(&d), t.lookup(&digest_of("x"))), (1, Some(f), None));
}

#[test]
fn custom_record_store_is_consulted_and_bounded() {
    struct Nothing;
    impl SourceRecords for Nothing {
        fn lookup(&self, _: &Digest) -> Option<RecordFacts> {
            None
        }
    }
    let mut input = row("modelled_lpu_all_dcc").input.unwrap();
    let o = assign(&input, &Nothing).unwrap();
    assert_eq!(o.class(), EvidenceClass::Uec1);
    assert_eq!(o.downgrade().unwrap().reason, DowngradeReason::UnverifiedSource);
    assert_eq!(o.downgrade().unwrap().input_id.as_deref(), Some("a"));

    input.contributor_sources =
        (0..=MAX_CONTRIBUTORS).map(|i| ContributorSource { input_id: format!("x{i}"), source: None }).collect();
    assert_eq!(assign(&input, &Nothing), Err(AssignError::InvalidInput { field: "contributors" }));
    input.contributor_sources.truncate(MAX_CONTRIBUTORS);
    let o = assign(&input, &Nothing).unwrap();
    assert_eq!(o.downgrade().unwrap().input_id.as_deref(), Some("x0"));
}

#[test]
fn error_codes_are_stable() {
    let cases = [
        (AssignError::EmptyModel, "empty_model"),
        (AssignError::UnknownBasis, "unknown_basis"),
        (AssignError::DuplicateInput, "duplicate_input"),
        (AssignError::DuplicateRecord, "duplicate_record"),
        (AssignError::InvalidInput { field: "x" }, "invalid_input"),
        (AssignError::Inconsistent { field: "x" }, "inconsistent_input"),
        (AssignError::LabelDropped { field: "x" }, "label_dropped"),
        (AssignError::SourceNotVerified, "source_not_verified"),
        (AssignError::ContributorMismatch, "contributor_mismatch"),
    ];
    for (e, code) in cases {
        assert_eq!(e.code(), code);
        assert!(e.to_string().starts_with(code));
    }
}

#[test]
fn forged_source_is_refused_even_when_every_claim_is_verified() {
    let o = run(&row("modelled_lpu_all_dcc")).unwrap();
    assert_eq!(o.class(), EvidenceClass::Uec2);
    let ok = vec![src(SourceKind::Dcc, "a"), src(SourceKind::Dcc, "b"), src(SourceKind::Dcc, "c")];
    o.statement(parts_for(&o, ok.clone())).unwrap();
    // An extra source nobody claimed.
    let mut forged = ok.clone();
    forged.push(src(SourceKind::Dcc, "z"));
    assert_eq!(o.statement(parts_for(&o, forged)), Err(AssignError::SourceNotVerified));
    // A claimed digest under another kind.
    let mut forged = ok.clone();
    forged[1] = SourceRef { kind: SourceKind::TypeA, ..forged[1].clone() };
    assert_eq!(o.statement(parts_for(&o, forged)), Err(AssignError::SourceNotVerified));
    // A citable kind with a digest that was never claimed.
    let mut forged = ok;
    forged[2].digest = digest_of("source:forged");
    assert_eq!(o.statement(parts_for(&o, forged)), Err(AssignError::SourceNotVerified));
}

#[test]
fn contributors_must_be_exactly_the_assigned_ones() {
    let o = run(&row("modelled_lpu_all_dcc")).unwrap();
    assert_eq!(o.contributor_ids(), ["a", "b", "c"]);
    let ok = vec![src(SourceKind::Dcc, "a"), src(SourceKind::Dcc, "b"), src(SourceKind::Dcc, "c")];
    let extra = Contributor { input_id: "d".to_owned(), sensitivity: None, u_contrib: 0.0, variance_share: 0.0 };
    let mut p = parts_for(&o, ok.clone());
    p.contributors.pop();
    assert_eq!(o.statement(p), Err(AssignError::ContributorMismatch));
    let mut p = parts_for(&o, ok.clone());
    p.contributors.push(extra.clone());
    assert_eq!(o.statement(p), Err(AssignError::ContributorMismatch));
    let mut p = parts_for(&o, ok.clone());
    p.contributors[0].input_id = "d".to_owned();
    assert_eq!(o.statement(p), Err(AssignError::ContributorMismatch));
    // Order does not matter.
    let mut p = parts_for(&o, ok);
    p.contributors.reverse();
    o.statement(p).unwrap();
    // A claimed basis carries no contributors.
    let o = run(&row("declared_dcc_source")).unwrap();
    assert!(o.contributor_ids().is_empty());
    let mut p = parts_for(&o, vec![]);
    p.contributors.push(extra);
    assert_eq!(o.statement(p), Err(AssignError::ContributorMismatch));
}

#[test]
fn shared_records() {
    // Two contributors citing one record of the claimed kind: both verified, one citation.
    let o = run(&row("modelled_lpu_shared_record_same_kind")).unwrap();
    assert_eq!(o.class(), EvidenceClass::Uec2);
    assert_eq!(o.citable_sources().len(), 1);
    let shared =
        SourceRef { kind: SourceKind::Dcc, id: "s".to_owned(), digest: digest_of("source:s"), unverified: false };
    o.statement(parts_for(&o, vec![shared.clone()])).unwrap();
    // One record, two claims of different kinds: only the matching claim is citable.
    let o = run(&row("modelled_lpu_shared_record_other_kind")).unwrap();
    assert_eq!(o.class(), EvidenceClass::Uec1);
    assert_eq!(o.downgrade().unwrap().reason, DowngradeReason::UnverifiedSource);
    let typed = SourceRef { kind: SourceKind::TypeA, ..shared };
    assert_eq!(o.statement(parts_for(&o, vec![typed])), Err(AssignError::SourceNotVerified));
}

#[test]
fn uec2_and_uec3_statements_are_sealed_to_class() {
    let o = run(&row("iso15530_4_all_dcc")).unwrap();
    assert_eq!(o.class(), EvidenceClass::Uec3);
    let sources = vec![src(SourceKind::Dcc, "a"), src(SourceKind::Dcc, "b"), src(SourceKind::Dcc, "c")];
    let mut p = parts_for(&o, sources.clone());
    p.class = EvidenceClass::Uec3;
    assert_eq!(UncertaintyStatement::new(p.clone()), Err(StmtError::ClassRequiresAssignment));
    p.class = EvidenceClass::Uec2;
    assert_eq!(UncertaintyStatement::new(p), Err(StmtError::ClassRequiresAssignment));
    // Built through CLASS, the statement re-reads from its canonical bytes.
    let s = o.statement(parts_for(&o, sources)).unwrap();
    let back = UncertaintyStatement::parse_canonical(s.canonical_json()).unwrap();
    assert_eq!(back, s);
    // UEC-0 and UEC-1 stay publicly constructible.
    let o = run(&row("modelled_lpu_one_declared")).unwrap();
    let mut p = parts_for(&o, vec![src(SourceKind::Dcc, "a"), src(SourceKind::Declared, "b")]);
    p.class = EvidenceClass::Uec1;
    UncertaintyStatement::new(p).unwrap();
}

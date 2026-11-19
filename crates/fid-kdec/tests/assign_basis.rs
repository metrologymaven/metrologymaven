//! UNC.UEC.CLASS into UNC.DEC.TYPE: the class and the labels assigned by `uec-assign@1`
//! reach every decision through the statement, so a claimed or partly unsourced value never
//! looks measured downstream. Expected classes come from the reference table
//! `fixtures/unc/class/table.csv` (written by `make_table.py`), not from this code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kdec::{Decision, EvidenceBasis, NotDecidableReason, Outcome};
use fid_kunc::uec::{assign, AssignInput, Basis, ContributorSource, RecordFacts, RecordTable, SourceClaim};
use fid_kunc::{
    Contributor, EvidenceClass, Expanded, Method, ModelRef, PrincipalKind, SourceKind, SourceRef, StatementParts,
};
use fid_types::Digest;

fn digest_of(label: &str) -> Digest {
    fid_kci::kci::sha256_digest(label.as_bytes())
}

fn claim(id: &str, kind: SourceKind) -> ContributorSource {
    ContributorSource {
        input_id: id.to_owned(),
        source: Some(SourceClaim { kind, origin: None, digest: digest_of(&format!("source:{id}")) }),
    }
}

fn record(id: &str, kind: SourceKind) -> (Digest, RecordFacts) {
    (digest_of(&format!("source:{id}")), RecordFacts::Source { kind, origin: None })
}

fn parts(method: Method, sources: &[(&str, SourceKind)]) -> StatementParts {
    let mut p = StatementParts::new(EvidenceClass::Uec0);
    p.method = Some(method);
    p.unit = Some("si:m".to_owned());
    p.u_std = Some(1.0e-6);
    p.expanded = Some(Expanded { value: 2.0e-6, k: 2.0 });
    p.model = Some(ModelRef { id: "um:m".to_owned(), semver: "1.0.0".to_owned(), digest: digest_of("um:m") });
    p.sources = sources
        .iter()
        .map(|(id, kind)| SourceRef {
            kind: *kind,
            id: (*id).to_owned(),
            digest: digest_of(&format!("source:{id}")),
            unverified: false,
        })
        .collect();
    if method == Method::Declared {
        p.provenance.principal = Some("person:alice".to_owned());
        p.provenance.principal_kind = Some(PrincipalKind::Human);
    }
    p
}

fn decision_basis(input: &AssignInput, records: &RecordTable, mut p: StatementParts) -> (EvidenceClass, EvidenceBasis) {
    let o = assign(input, records).unwrap();
    // The statement carries exactly the assigned contributors.
    p.contributors = o
        .contributor_ids()
        .iter()
        .map(|id| Contributor { input_id: id.clone(), sensitivity: None, u_contrib: 1.0e-7, variance_share: 0.5 })
        .collect();
    let stmt = o.statement(p).unwrap();
    let basis = EvidenceBasis::of(&stmt).unwrap();
    assert_eq!(basis.class(), o.class());
    assert_eq!(basis.is_claimed(), o.is_claimed());
    assert_eq!(basis.rests_on_declared_input(), o.rests_on_declared_input());
    assert_eq!(basis.rests_on_uncharacterized_input(), o.rests_on_uncharacterized_input());
    let d = Decision::new(None, Outcome::NotDecidable(NotDecidableReason::RuleMissing), Some(basis.clone())).unwrap();
    assert_eq!(d.is_claimed(), o.is_claimed());
    assert_eq!(d.rests_on_declared_input(), o.rests_on_declared_input());
    assert_eq!(d.rests_on_uncharacterized_input(), o.rests_on_uncharacterized_input());
    (o.class(), basis)
}

#[test]
fn labels_reach_the_decision() {
    // Table row modelled_mcm_one_declared: UEC-1, rests on a declared input.
    let input = AssignInput {
        basis: Basis::ModelledMcm,
        contributor_sources: vec![
            claim("a", SourceKind::Dcc),
            claim("b", SourceKind::Declared),
            claim("c", SourceKind::Dcc),
        ],
        validation_record: None,
        model_bound: true,
        instruments_bound: true,
    };
    let records = RecordTable::new(vec![
        record("a", SourceKind::Dcc),
        record("b", SourceKind::Declared),
        record("c", SourceKind::Dcc),
    ])
    .unwrap();
    let p = parts(Method::Mcm, &[("a", SourceKind::Dcc), ("b", SourceKind::Declared), ("c", SourceKind::Dcc)]);
    let (class, basis) = decision_basis(&input, &records, p);
    assert_eq!(class, EvidenceClass::Uec1);
    assert!(!basis.is_claimed() && basis.rests_on_declared_input() && basis.rests_on_uncharacterized_input());

    // Table row declared_dcc_source: a claim stays a claim even with a genuine DCC.
    let input = AssignInput {
        basis: Basis::Declared,
        contributor_sources: vec![claim("a", SourceKind::Dcc)],
        validation_record: None,
        model_bound: true,
        instruments_bound: true,
    };
    let records = RecordTable::new(vec![record("a", SourceKind::Dcc)]).unwrap();
    let (class, basis) = decision_basis(&input, &records, parts(Method::Declared, &[]));
    assert_eq!(class, EvidenceClass::Uec1);
    assert!(basis.is_claimed() && !basis.rests_on_uncharacterized_input());

    // Table row modelled_lpu_all_dcc: UEC-2 with no labels.
    let input = AssignInput {
        basis: Basis::ModelledLpu,
        contributor_sources: vec![claim("a", SourceKind::Dcc), claim("b", SourceKind::TypeA)],
        validation_record: None,
        model_bound: true,
        instruments_bound: true,
    };
    let records = RecordTable::new(vec![record("a", SourceKind::Dcc), record("b", SourceKind::TypeA)]).unwrap();
    let p = parts(Method::Lpu, &[("a", SourceKind::Dcc), ("b", SourceKind::TypeA)]);
    let (class, basis) = decision_basis(&input, &records, p);
    assert_eq!(class, EvidenceClass::Uec2);
    assert!(!basis.is_claimed() && !basis.rests_on_declared_input() && !basis.rests_on_uncharacterized_input());
}

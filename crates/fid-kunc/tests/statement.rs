//! Acceptance criteria of spec node UNC.UEC.STMT (AC-1 .. AC-6) plus requirement checks.
//!
//! Fixture bytes, digests and expected errors come from the independent Python reference
//! `fixtures/unc/stmt/make_fixtures.py` (stdlib `json`, `struct`, `hashlib`) and its
//! hand-labelled error table; none come from this crate. AC-5 is this same file run by CI
//! on wasm32-wasip1 (wasmtime), x86-64 (Linux, Windows) and ARM64 (macOS).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kunc::statement::{MAX_INPUT_BYTES, SCHEMA};
use fid_kunc::{
    Contributor, CoverageInterval, EvidenceClass, Expanded, IntervalKind, Method, ModelRef, NlinVerdict, Nonlinearity,
    PrincipalKind, Provenance, SourceKind, SourceRef, StatementParts, StmtError, Uec0Reason, UncertaintyStatement,
};
use fid_num::rng::{Address, Cursor};
use fid_types::Digest;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/stmt/");

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}{rel}")).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn table(rel: &str) -> Vec<Vec<String>> {
    String::from_utf8(read(rel))
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(str::to_owned).collect())
        .collect()
}

fn digest_of(label: &str) -> Digest {
    fid_kci::kci::sha256_digest(label.as_bytes())
}

// ------------------------------------------------------------------ AC-1, AC-5

#[test]
fn ac1_ac5_valid_fixtures_round_trip_byte_exact_with_reference_digests() {
    let rows = table("valid/digests.txt");
    assert!(rows.len() >= 10);
    let mut classes = Vec::new();
    let mut methods = Vec::new();
    for row in &rows {
        let [file, digest] = row.as_slice() else { panic!("bad row {row:?}") };
        let bytes = read(&format!("valid/{file}"));
        let stmt = UncertaintyStatement::parse_canonical(&bytes).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(stmt.canonical_json(), bytes.as_slice(), "{file}: re-serialized bytes differ");
        assert_eq!(stmt.digest().as_str(), digest, "{file}: digest");
        // Rebuilding from the parts gives the same bytes again.
        let again = UncertaintyStatement::new(stmt.to_parts()).unwrap();
        assert_eq!(again, stmt);
        classes.push(stmt.class());
        methods.extend(stmt.method());
    }
    for c in EvidenceClass::ALL {
        assert!(classes.contains(c), "no valid fixture of class {c}");
    }
    for m in Method::ALL {
        assert!(methods.contains(m), "no valid fixture with method {m}");
    }
    // Every file in valid/ is listed.
    let listed = std::fs::read_dir(format!("{FIX}valid")).unwrap().count();
    assert_eq!(listed, rows.len() + 1);
}

// ------------------------------------------------------------------ AC-2, AC-3, AC-4

fn check_invalid(file: &str, code: &str, field: &str) {
    let bytes = read(&format!("invalid/{file}"));
    let err = UncertaintyStatement::parse_canonical(&bytes).expect_err(file);
    assert_eq!(err.code(), code, "{file}: {err}");
    assert_eq!(err.field().unwrap_or("-"), field, "{file}: {err}");
}

#[test]
fn ac2_ac3_ac4_and_every_labelled_invalid_fixture_fails_as_labelled() {
    let rows = table("invalid/expected.txt");
    for row in &rows {
        let [file, code, field] = row.as_slice() else { panic!("bad row {row:?}") };
        check_invalid(file, code, field);
    }
    let listed = std::fs::read_dir(format!("{FIX}invalid")).unwrap().count();
    assert_eq!(listed, rows.len() + 1);
}

#[test]
fn ac2_uec0_with_u() {
    check_invalid("uec0_with_U.json", "uec0_has_value", "expanded");
}

#[test]
fn ac3_incomplete_names_the_file_suffix() {
    let mut seen = 0;
    for row in table("invalid/expected.txt") {
        if let Some(suffix) = row[0].strip_prefix("incomplete_").and_then(|s| s.strip_suffix(".json")) {
            check_invalid(&row[0], "statement_incomplete", suffix);
            seen += 1;
        }
    }
    assert!(seen >= 4, "method, unit, coverage and model at least");
}

#[test]
fn ac4_nan_negative_k_and_p_one_are_invalid_numbers() {
    check_invalid("nan_U.json", "invalid_number", "expanded.U");
    check_invalid("neg_k.json", "invalid_number", "expanded.k");
    check_invalid("p_1.json", "invalid_number", "interval.p");
}

// ------------------------------------------------------------------ constructor

fn model() -> ModelRef {
    ModelRef { id: "um:m".into(), semver: "1.0.0".into(), digest: digest_of("m") }
}

fn complete(class: EvidenceClass, method: Method) -> StatementParts {
    let mut p = StatementParts::new(class);
    p.method = Some(method);
    p.unit = Some("unit:m".into());
    p.expanded = Some(Expanded { value: 2e-6, k: 2.0 });
    p.model = Some(model());
    if class >= EvidenceClass::Uec2 {
        p.sources.push(SourceRef { kind: SourceKind::Dcc, id: "dcc:1".into(), digest: digest_of("dcc:1") });
    }
    p
}

fn err(p: StatementParts) -> StmtError {
    UncertaintyStatement::new(p).unwrap_err()
}

#[test]
fn req001_class_is_always_present_and_uec0_has_no_number() {
    let mut p = StatementParts::new(EvidenceClass::Uec0);
    assert_eq!(err(p.clone()), StmtError::StatementIncomplete { field: "uec0_reason" });
    p.uec0_reason = Some(Uec0Reason::UMissing);
    let s = UncertaintyStatement::new(p.clone()).unwrap();
    assert_eq!(s.class(), EvidenceClass::Uec0);
    assert!(s.u_std().is_none() && s.expanded().is_none() && s.interval().is_none() && !s.is_claimed());

    // REQ-002: any numeric field (or evaluation output) on UEC-0 fails, also a zero.
    let mut q = p.clone();
    q.u_std = Some(0.0);
    assert_eq!(err(q), StmtError::Uec0HasValue { field: "u_std" });
    let mut q = p.clone();
    q.interval = Some(CoverageInterval { low: -1.0, high: 1.0, p: 0.95, kind: IntervalKind::Shortest });
    assert_eq!(err(q), StmtError::Uec0HasValue { field: "interval" });
    let mut q = p.clone();
    q.contributors.push(Contributor { input_id: "a".into(), sensitivity: None, u_contrib: 0.0, variance_share: 0.0 });
    assert_eq!(err(q), StmtError::Uec0HasValue { field: "contributors" });
    // The class check comes before the number check.
    let mut q = p;
    q.expanded = Some(Expanded { value: f64::NAN, k: 2.0 });
    assert_eq!(err(q), StmtError::Uec0HasValue { field: "expanded" });
}

#[test]
fn req003_first_missing_field_in_declaration_order() {
    let mut p = StatementParts::new(EvidenceClass::Uec2);
    assert_eq!(err(p.clone()), StmtError::StatementIncomplete { field: "method" });
    p.method = Some(Method::Lpu);
    assert_eq!(err(p.clone()), StmtError::StatementIncomplete { field: "unit" });
    p.unit = Some("unit:m".into());
    p.u_std = Some(1e-6); // a standard uncertainty alone is not a coverage statement
    assert_eq!(err(p.clone()), StmtError::StatementIncomplete { field: "coverage" });
    p.interval = Some(CoverageInterval { low: 1.0, high: 2.0, p: 0.95, kind: IntervalKind::ProbSymmetric });
    assert_eq!(err(p.clone()), StmtError::StatementIncomplete { field: "model" });
    p.model = Some(model());
    // Numbers are checked before completeness.
    let mut q = StatementParts::new(EvidenceClass::Uec1);
    q.u_std = Some(-1.0);
    assert_eq!(err(q), StmtError::InvalidNumber { field: "u_std" });
    // UEC-2 also needs characterized sources.
    assert_eq!(err(p.clone()), StmtError::ClassMismatch { field: "sources" });
    p.sources.push(SourceRef { kind: SourceKind::TypeA, id: "ta".into(), digest: digest_of("ta") });
    UncertaintyStatement::new(p).unwrap();
}

#[test]
fn req006_numbers_are_checked_on_bits() {
    let cases: [(fn(&mut StatementParts), &str); 9] = [
        (|p| p.expanded = Some(Expanded { value: f64::INFINITY, k: 2.0 }), "expanded.U"),
        (|p| p.expanded = Some(Expanded { value: -0.0, k: 2.0 }), "expanded.U"),
        (|p| p.expanded = Some(Expanded { value: 1e-6, k: 0.0 }), "expanded.k"),
        (|p| p.expanded = Some(Expanded { value: 1e-6, k: f64::NAN }), "expanded.k"),
        (|p| p.u_std = Some(f64::NEG_INFINITY), "u_std"),
        (|p| p.interval = Some(CoverageInterval { low: -0.0, high: 1.0, p: 0.5, kind: IntervalKind::Shortest }), "interval.low"),
        (|p| p.interval = Some(CoverageInterval { low: 1.0, high: 1.0, p: 0.5, kind: IntervalKind::Shortest }), "interval.high"),
        (
            |p| p.interval = Some(CoverageInterval { low: 0.0, high: 1.0, p: f64::from_bits(0x3ff0_0000_0000_0000), kind: IntervalKind::Shortest }),
            "interval.p",
        ),
        (
            |p| {
                p.contributors.push(Contributor { input_id: "x".into(), sensitivity: Some(-0.0), u_contrib: 0.0, variance_share: 0.0 })
            },
            "contributors.sensitivity",
        ),
    ];
    for (mutate, field) in cases {
        let mut p = complete(EvidenceClass::Uec1, Method::Lpu);
        mutate(&mut p);
        assert_eq!(err(p), StmtError::InvalidNumber { field });
    }
    // The smallest positive values are accepted: positivity is a sign and zero test only.
    let mut p = complete(EvidenceClass::Uec1, Method::Lpu);
    p.expanded = Some(Expanded { value: f64::from_bits(1), k: f64::from_bits(1) });
    p.interval =
        Some(CoverageInterval { low: -f64::MAX, high: f64::MAX, p: f64::from_bits(0x3fef_ffff_ffff_ffff), kind: IntervalKind::Shortest });
    UncertaintyStatement::new(p).unwrap();
}

#[test]
fn claimed_values_never_look_measured() {
    for m in [Method::Declared, Method::SpecDerived] {
        let s = UncertaintyStatement::new(complete(EvidenceClass::Uec1, m)).unwrap();
        assert!(s.is_claimed());
        for c in [EvidenceClass::Uec2, EvidenceClass::Uec3] {
            assert_eq!(err(complete(c, m)), StmtError::ClassMismatch { field: "method" }, "{m} {c}");
        }
    }
    for m in [Method::Lpu, Method::Mcm] {
        assert!(!UncertaintyStatement::new(complete(EvidenceClass::Uec2, m)).unwrap().is_claimed());
        assert_eq!(err(complete(EvidenceClass::Uec3, m)), StmtError::ClassMismatch { field: "method" });
    }
    for m in [Method::Iso15530_3, Method::Iso15530_4] {
        UncertaintyStatement::new(complete(EvidenceClass::Uec3, m)).unwrap();
    }
    // A declared or instrument-model source keeps a statement at UEC-1.
    for kind in [SourceKind::Declared, SourceKind::InstrumentModel] {
        let mut p = complete(EvidenceClass::Uec2, Method::Mcm);
        p.sources.push(SourceRef { kind, id: "z".into(), digest: digest_of("z") });
        assert_eq!(err(p.clone()), StmtError::ClassMismatch { field: "sources" });
        p.class = EvidenceClass::Uec1;
        UncertaintyStatement::new(p).unwrap();
    }
}

#[test]
fn agent_statements_are_proposals() {
    let mut p = complete(EvidenceClass::Uec1, Method::Declared);
    p.provenance.principal = Some("agent:a".into());
    assert_eq!(err(p.clone()), StmtError::ClassMismatch { field: "provenance.principal_kind" });
    p.provenance.principal_kind = Some(PrincipalKind::Agent);
    assert_eq!(err(p.clone()), StmtError::ClassMismatch { field: "provenance.proposal" });
    p.provenance.proposal = true;
    UncertaintyStatement::new(p.clone()).unwrap();
    let mut q = complete(EvidenceClass::Uec1, Method::Declared);
    q.provenance.adopted_by = Some("person:b".into());
    assert_eq!(err(q), StmtError::ClassMismatch { field: "provenance.proposal" });
}

#[test]
fn vectors_are_sorted_and_ids_unique() {
    let mut p = complete(EvidenceClass::Uec2, Method::Lpu);
    for id in ["z", "a", "m"] {
        p.contributors.push(Contributor { input_id: id.into(), sensitivity: None, u_contrib: 1e-7, variance_share: 0.25 });
    }
    p.provenance.rule_sets = vec!["uec-assign@1".into(), "est-policy@1".into()];
    let s = UncertaintyStatement::new(p.clone()).unwrap();
    let ids: Vec<&str> = s.contributors().iter().map(|c| c.input_id.as_str()).collect();
    assert_eq!(ids, ["a", "m", "z"]);
    assert_eq!(s.provenance().rule_sets, ["est-policy@1", "uec-assign@1"]);
    p.contributors.push(Contributor { input_id: "m".into(), sensitivity: None, u_contrib: 0.0, variance_share: 0.0 });
    assert_eq!(err(p), StmtError::InvalidField { field: "contributors.input_id" });
}

#[test]
fn every_field_changes_the_digest() {
    let mut base = complete(EvidenceClass::Uec1, Method::Lpu);
    base.contributors.push(Contributor { input_id: "a".into(), sensitivity: Some(1.0), u_contrib: 1e-6, variance_share: 1.0 });
    let d0 = UncertaintyStatement::new(base.clone()).unwrap().digest();
    let edits: [fn(&mut StatementParts); 8] = [
        |p| p.method = Some(Method::Mcm),
        |p| p.unit = Some("unit:mm".into()),
        |p| p.u_std = Some(1e-6),
        |p| p.expanded = Some(Expanded { value: f64::from_bits(2e-6f64.to_bits() + 1), k: 2.0 }),
        |p| p.contributors[0].sensitivity = None,
        |p| p.lpu_validated_by_mc = Some(false),
        |p| p.provenance.proposal = true,
        |p| p.selection_rule = Some("unc-select@1".into()),
    ];
    let mut seen = vec![d0];
    for e in edits {
        let mut p = base.clone();
        e(&mut p);
        let d = UncertaintyStatement::new(p).unwrap().digest();
        assert!(!seen.contains(&d));
        seen.push(d);
    }
}

#[test]
fn nonlinearity_lpu_reportable_follows_the_outcome() {
    let mut n = Nonlinearity {
        version: "nlin@1".into(),
        verdict: NlinVerdict::LinearAdequate,
        v1: 1.0,
        v2: 0.0,
        ratio: Some(0.0),
        probed_inputs: 0,
        probed_pairs: 0,
        cross_terms_truncated: false,
        reason: None,
    };
    assert!(n.lpu_reportable());
    n.verdict = NlinVerdict::SecondOrderNeeded;
    assert!(!n.lpu_reportable());
}

#[test]
fn parser_fails_closed_on_hostile_input() {
    assert_eq!(
        UncertaintyStatement::parse_canonical(&vec![b' '; MAX_INPUT_BYTES + 1]),
        Err(StmtError::Malformed { detail: "too_large" })
    );
    let bad_inputs: [&[u8]; 7] =
        [b"", b"null", b"[]", b"{}", b"{\"schema\":\"x\"}", b"\xff", b"{\"schema\":\"uncertainty_statement@1\"}"];
    for bad in bad_inputs {
        assert!(UncertaintyStatement::parse_canonical(bad).is_err(), "{bad:?}");
    }
    // Deep nesting inside a member is bounded.
    let deep = format!("{{\"schema\":\"{SCHEMA}\",\"class\":{}{}}}", "[".repeat(100_000), "]".repeat(100_000));
    assert_eq!(UncertaintyStatement::parse_canonical(deep.as_bytes()), Err(StmtError::Malformed { detail: "too_deep" }));
}

// ------------------------------------------------------------------ AC-6

struct Gen(Cursor);

const SIGN: u64 = 1 << 63;
const EXP: u64 = 0x7ff0_0000_0000_0000;
const ONE: u64 = 0x3ff0_0000_0000_0000;

impl Gen {
    fn u(&mut self) -> u64 {
        self.0.next_u64()
    }
    fn below(&mut self, n: u64) -> u64 {
        self.u() % n
    }
    fn coin(&mut self) -> bool {
        self.u() & 1 == 1
    }
    /// Finite, not -0, any sign; raw bit patterns so subnormals and extremes occur.
    fn finite(&mut self) -> f64 {
        loop {
            let b = self.u();
            if b & EXP != EXP && b != SIGN {
                return f64::from_bits(b);
            }
        }
    }
    fn non_negative(&mut self) -> f64 {
        f64::from_bits(self.finite().to_bits() & !SIGN)
    }
    fn positive(&mut self) -> f64 {
        loop {
            let x = self.non_negative();
            if x.to_bits() != 0 {
                return x;
            }
        }
    }
    fn open_unit(&mut self) -> f64 {
        f64::from_bits(1 + self.below(ONE - 1))
    }
    fn closed_unit(&mut self) -> f64 {
        f64::from_bits(self.below(ONE + 1))
    }
    fn token(&mut self, prefix: &str) -> String {
        const CH: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.:@/!#$%&'()*+,;<=>?[]^`{|}~";
        let n = 1 + self.below(12) as usize;
        let mut s = prefix.to_owned();
        for _ in 0..n {
            s.push(char::from(CH[self.below(CH.len() as u64) as usize]));
        }
        s
    }
    fn digest(&mut self) -> Digest {
        let mut b = [0u8; 32];
        for chunk in b.chunks_mut(8) {
            chunk.copy_from_slice(&self.u().to_le_bytes());
        }
        Digest::from_bytes(b)
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize]
    }

    fn parts(&mut self) -> StatementParts {
        let class = self.pick(EvidenceClass::ALL);
        let mut p = StatementParts::new(class);
        let methods: &[Method] = match class {
            EvidenceClass::Uec0 => &[],
            EvidenceClass::Uec1 => Method::ALL,
            EvidenceClass::Uec2 => &[Method::Lpu, Method::Mcm, Method::Iso15530_3, Method::Iso15530_4],
            EvidenceClass::Uec3 => &[Method::Iso15530_3, Method::Iso15530_4],
        };
        let source_kinds: &[SourceKind] = if class >= EvidenceClass::Uec2 {
            &[SourceKind::Dcc, SourceKind::Qualification, SourceKind::TypeA]
        } else {
            SourceKind::ALL
        };
        let n_sources = if class >= EvidenceClass::Uec2 { 1 + self.below(4) } else { self.below(4) };
        for i in 0..n_sources {
            let kind = self.pick(source_kinds);
            let id = self.token(&format!("{i}"));
            let digest = self.digest();
            p.sources.push(SourceRef { kind, id, digest });
        }
        if class == EvidenceClass::Uec0 || self.coin() {
            if self.coin() {
                p.unit = Some(self.token("unit:"));
            }
            if self.coin() {
                p.model = Some(ModelRef { id: self.token("um:"), semver: "0.1.12".into(), digest: self.digest() });
            }
        }
        if class == EvidenceClass::Uec0 {
            p.uec0_reason = Some(self.pick(Uec0Reason::ALL));
        } else {
            p.method = Some(self.pick(methods));
            p.unit = Some(self.token("x-y.z+1:"));
            p.model = Some(ModelRef {
                id: self.token("um:"),
                semver: format!("{}.{}.{}", self.below(1000), self.below(10), self.below(999_999_999)),
                digest: self.digest(),
            });
            if self.coin() {
                p.u_std = Some(self.positive());
            }
            let with_interval = self.coin();
            if !with_interval || self.coin() {
                p.expanded = Some(Expanded { value: self.positive(), k: self.positive() });
            }
            if with_interval {
                let (mut a, mut b) = (self.finite(), self.finite());
                while a == b {
                    b = self.finite();
                }
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                let kind = self.pick(IntervalKind::ALL);
                p.interval = Some(CoverageInterval { low: a, high: b, p: self.open_unit(), kind });
            }
            for i in 0..self.below(5) {
                let sensitivity = if self.coin() { Some(self.finite()) } else { None };
                p.contributors.push(Contributor {
                    input_id: self.token(&format!("{i}")),
                    sensitivity,
                    u_contrib: self.non_negative(),
                    variance_share: self.closed_unit(),
                });
            }
            if self.coin() {
                p.lpu_validated_by_mc = Some(self.coin());
            }
            if self.coin() {
                p.nonlinearity = Some(Nonlinearity {
                    version: self.token("nlin@"),
                    verdict: self.pick(NlinVerdict::ALL),
                    v1: self.non_negative(),
                    v2: self.non_negative(),
                    ratio: if self.coin() { Some(self.non_negative()) } else { None },
                    probed_inputs: self.u() as u32,
                    probed_pairs: self.below(3) as u32,
                    cross_terms_truncated: self.coin(),
                    reason: if self.coin() { Some(self.token("r")) } else { None },
                });
            }
        }
        if self.coin() {
            p.selection_rule = Some(self.token("unc-select@"));
        }
        if self.coin() {
            p.estimate_convention = Some(self.token("est-policy@"));
        }
        let mut pv = Provenance::default();
        for i in 0..self.below(3) {
            pv.rule_sets.push(self.token(&format!("{i}")));
        }
        if self.coin() {
            pv.principal = Some(self.token("p:"));
            pv.principal_kind = Some(self.pick(PrincipalKind::ALL));
        }
        if self.coin() {
            pv.adopted_by = Some(self.token("person:"));
        }
        pv.proposal = pv.principal_kind == Some(PrincipalKind::Agent) || pv.adopted_by.is_some() || self.coin();
        if self.coin() {
            pv.supersedes = Some(self.digest());
        }
        p.provenance = pv;
        p
    }
}

#[test]
fn ac6_ten_thousand_seeded_statements_round_trip_losslessly() {
    let mut g = Gen(Cursor::new(Address { seed: 0x5EED_0001, stream: 0 }, 0));
    for n in 0..10_000 {
        let parts = g.parts();
        let stmt = UncertaintyStatement::new(parts.clone()).unwrap_or_else(|e| panic!("case {n}: {e}: {parts:?}"));
        let back = UncertaintyStatement::parse_canonical(stmt.canonical_json()).unwrap_or_else(|e| panic!("case {n}: {e}"));
        assert_eq!(back, stmt, "case {n}");
        assert_eq!(back.digest(), stmt.digest());
        // Bit-level equality of every float (PartialEq alone would equate +0 and -0).
        let (a, b) = (format!("{:?}", back.to_parts()), format!("{:?}", stmt.to_parts()));
        assert_eq!(a, b, "case {n}");
    }
}

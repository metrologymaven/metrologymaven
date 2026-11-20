//! Acceptance criteria of spec node UNC.DEC.TYPE (AC-1 .. AC-5) plus requirement and
//! decision-record checks.
//!
//! Fixture bytes, digests, ID classifications and expected errors come from the
//! independent Python reference `fixtures/unc/dec/type/make_fixtures.py` (stdlib `json`,
//! `re`, `fractions`, `hashlib`) and its hand labels; none come from this crate. The
//! uncertainty statements are the `UNC.UEC.STMT` fixtures. AC-5 is this same file run by
//! CI on wasm32-wasip1 (wasmtime), x86-64 (Linux, Windows) and ARM64 (macOS).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kdec::rule::json::{self, Json};
use fid_kdec::rule::{BUILTIN_CATALOG_DIGEST, BUILTIN_CATALOG_JSON, CATALOG_TAG};
use fid_kdec::{
    Catalog, DecError, Decision, DecisionRule, EvidenceBasis, NotDecidableReason, Outcome, ParamDecimal, PdfSource,
    RiskReporting, RuleId, RuleKind, RuleParams, RuleParts, RuleRef, StandardsRef,
};
use fid_kunc::{EvidenceClass, UncertaintyStatement};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/dec/type/");
const STMT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/stmt/valid/");

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}{rel}")).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn dec(s: &str) -> ParamDecimal {
    ParamDecimal::parse(s).unwrap_or_else(|| panic!("{s}"))
}

fn statement(name: &str) -> UncertaintyStatement {
    let bytes = std::fs::read(format!("{STMT}{name}.json")).unwrap();
    UncertaintyStatement::parse_canonical(&bytes).unwrap()
}

fn customer(params: RuleParams) -> RuleParts {
    RuleParts {
        id: RuleId::parse("dr:customer/acme/bore").unwrap(),
        version: 1,
        standards_ref: None,
        params,
        pdf_source: PdfSource::McOrValidatedNormal,
        risk_reporting: RiskReporting::Specific,
    }
}

fn err(code: &str, detail: Option<&str>) -> (String, Option<String>) {
    (code.to_owned(), detail.map(str::to_owned))
}

fn of(e: DecError) -> (String, Option<String>) {
    err(e.code(), e.detail())
}

// ------------------------------------------------------------------ AC-1, AC-5

#[test]
fn ac1_ac5_catalog_loads_with_reference_digest() {
    let bytes = read("catalog_v1.json");
    let digest = String::from_utf8(read("catalog_v1.sha256")).unwrap();
    assert_eq!(BUILTIN_CATALOG_JSON.as_bytes(), bytes.as_slice(), "embedded catalog differs from the fixture");
    assert_eq!(BUILTIN_CATALOG_DIGEST, digest.trim_end());

    let parsed = Catalog::parse_canonical(&bytes).unwrap();
    assert_eq!(parsed.canonical_json(), bytes.as_slice());
    assert_eq!(parsed.digest().as_str(), digest.trim_end());
    let builtin = Catalog::builtin().unwrap();
    assert_eq!(builtin, parsed);
    assert_eq!(builtin.digest().as_str(), digest.trim_end());

    // Rebuilding from the rules gives the same bytes and digest.
    let again = Catalog::new(builtin.rules().to_vec()).unwrap();
    assert_eq!(again, builtin);
}

#[test]
fn ac1_catalog_holds_the_four_builtin_rules() {
    let c = Catalog::builtin().unwrap();
    assert_eq!(c.rules().len(), 4);

    let iso = c.get(&RuleId::Iso14253Default, 1).unwrap();
    assert_eq!(iso.kind(), RuleKind::ConformanceProbability);
    assert_eq!(iso.params(), &RuleParams::ConformanceProbability { p_min: dec("0.95") });
    let sr = iso.standards_ref().unwrap();
    assert_eq!((sr.designator(), sr.edition(), sr.clause()), ("ISO 14253-1", "2017", "5"));

    let simple = c.get(&RuleId::SimpleAcceptance, 1).unwrap();
    assert_eq!(simple.kind(), RuleKind::SimpleAcceptance);
    assert_eq!(simple.params(), &RuleParams::SimpleAcceptance);

    let guard = c.get(&RuleId::GuardBand { r: None }, 1).unwrap();
    assert_eq!(guard.kind(), RuleKind::GuardBand);
    assert_eq!(guard.params(), &RuleParams::GuardBand { r: None });

    let label = c.get(&RuleId::AsReportedBySource, 1).unwrap();
    assert_eq!(label.kind(), RuleKind::LabelOnly);
    assert_eq!(label.standards_ref(), None);
    assert_eq!(label.pdf_source(), PdfSource::NotUsed);
    assert_eq!(label.risk_reporting(), RiskReporting::NotReported);

    for r in c.rules() {
        assert_eq!(r.version(), 1);
        assert!(!r.id().is_customer());
        // REQ-005: designator, edition and clause only.
        if let Some(s) = r.standards_ref() {
            assert_eq!(StandardsRef::new(s.designator(), s.edition(), s.clause()).as_ref(), Ok(s));
        }
    }
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_rule_id_cases_match_the_reference() {
    let text = String::from_utf8(read("ids.csv")).unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("input,expect,kind,params,version"));
    let mut n = 0;
    let mut ok = 0;
    for line in lines {
        let cols: Vec<&str> = line.split(',').collect();
        let [input, expect, kind, params, version] = cols.as_slice() else { panic!("bad row {line:?}") };
        n += 1;
        match RuleRef::parse(input) {
            Ok(r) => {
                assert_eq!(*expect, "ok", "{input:?} parsed as {r}");
                ok += 1;
                let got_kind = r.id.kind().map_or("customer", RuleKind::wire);
                let got_params = match &r.id {
                    RuleId::GuardBand { r: Some(x) } => format!("r={x}"),
                    RuleId::Customer { tenant, name } => format!("tenant={tenant};id={name}"),
                    _ => String::new(),
                };
                assert_eq!((got_kind, got_params.as_str(), r.version.to_string().as_str()), (*kind, *params, *version));
                // Canonical: the string is the one form of this reference.
                assert_eq!(r.to_string(), *input);
                assert_eq!(RuleId::parse(&r.id.to_string()), Ok(r.id.clone()));
            }
            Err(e) => {
                let got = match e.detail() {
                    Some(d) => format!("{}:{d}", e.code()),
                    None => e.code().to_owned(),
                };
                assert_eq!(got, *expect, "{input:?}");
            }
        }
    }
    assert!(n >= 60 && ok >= 15, "{n} cases, {ok} valid");
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_bad_customer_rules_fail_with_invalid_rule() {
    let Json::Obj(top) = json::read(&read("bad_customer.json")).unwrap() else { panic!("not an object") };
    let Some((_, Json::Arr(cases))) = top.iter().find(|(k, _)| k == "cases") else { panic!("no cases") };
    let field = |m: &[(String, Json)], k: &str| -> String {
        match m.iter().find(|(key, _)| key == k) {
            Some((_, Json::Str(s))) => s.clone(),
            _ => panic!("missing {k}"),
        }
    };
    let mut names = Vec::new();
    for case in cases {
        let Json::Obj(m) = case else { panic!("case is not an object") };
        let name = field(m, "name");
        let rule = &m.iter().find(|(k, _)| k == "rule").unwrap().1;
        let param = field(m, "param");
        let want = err(&field(m, "expect"), (param != "-").then_some(param.as_str()));
        let got = DecisionRule::customer_from_json(rule).map(|r| r.to_string_for_test());
        assert_eq!(got.map_err(of), Err(want), "{name}");
        names.push(name);
    }
    for required in ["p_min_0_4", "r_minus_1", "kind_label_only"] {
        assert!(names.iter().any(|n| n == required), "AC-3 case {required} missing");
    }
}

/// Test-only helper so a successful parse shows up readably in a failed assertion.
trait ShowRule {
    fn to_string_for_test(&self) -> String;
}

impl ShowRule for DecisionRule {
    fn to_string_for_test(&self) -> String {
        String::from_utf8(self.canonical_json().unwrap()).unwrap()
    }
}

#[test]
fn valid_customer_rules_round_trip_through_their_json() {
    let rules = [
        customer(RuleParams::ConformanceProbability { p_min: dec("0.99") }),
        customer(RuleParams::GuardBand { r: Some(dec("0")) }),
        customer(RuleParams::GuardBand { r: Some(dec("1.5")) }),
        RuleParts {
            standards_ref: Some(StandardsRef::new("ASME B89.7.3.1", "2001", "4.2").unwrap()),
            risk_reporting: RiskReporting::SpecificAndGlobal,
            pdf_source: PdfSource::McOnly,
            ..customer(RuleParams::SimpleAcceptance)
        },
    ];
    for parts in rules {
        let rule = DecisionRule::customer(parts.clone()).unwrap();
        assert_eq!(rule.to_parts(), parts);
        let bytes = rule.canonical_json().unwrap();
        let back = DecisionRule::customer_from_json(&json::read(&bytes).unwrap()).unwrap();
        assert_eq!(back, rule);
    }
}

#[test]
fn typed_customer_rules_are_checked_like_json_ones() {
    let bad = |parts: RuleParts| DecisionRule::customer(parts).map_err(of).err();
    assert_eq!(
        bad(customer(RuleParams::ConformanceProbability { p_min: dec("0.4") })),
        Some(err("invalid_rule", Some("p_min")))
    );
    assert_eq!(
        bad(customer(RuleParams::ConformanceProbability { p_min: dec("1.5") })),
        Some(err("invalid_rule", Some("p_min")))
    );
    assert_eq!(bad(customer(RuleParams::GuardBand { r: Some(dec("-1")) })), Some(err("invalid_rule", Some("r"))));
    assert_eq!(bad(customer(RuleParams::GuardBand { r: None })), Some(err("invalid_rule", Some("r"))));
    assert_eq!(bad(customer(RuleParams::LabelOnly)), Some(err("invalid_rule", Some("kind"))));
    assert_eq!(
        bad(RuleParts { version: 0, ..customer(RuleParams::SimpleAcceptance) }),
        Some(err("invalid_rule", Some("version")))
    );
    assert_eq!(
        bad(RuleParts { id: RuleId::SimpleAcceptance, ..customer(RuleParams::SimpleAcceptance) }),
        Some(err("invalid_rule", Some("id")))
    );
    assert_eq!(
        bad(RuleParts { pdf_source: PdfSource::NotUsed, ..customer(RuleParams::SimpleAcceptance) }),
        Some(err("invalid_rule", Some("pdf_source")))
    );
    // A hand-built ID outside the grammar is refused, never trusted.
    let hand = RuleId::Customer { tenant: "Acme".into(), name: "x".into() };
    assert_eq!(
        bad(RuleParts { id: hand, ..customer(RuleParams::SimpleAcceptance) }),
        Some(err("invalid_rule_id", None))
    );
    let slash = RuleId::Customer { tenant: "a/b".into(), name: "x".into() };
    assert_eq!(
        bad(RuleParts { id: slash, ..customer(RuleParams::SimpleAcceptance) }),
        Some(err("invalid_rule_id", None))
    );
    // Standards references hold no text.
    assert_eq!(
        StandardsRef::new("ISO 14253-1", "2017", "the value shall").map_err(of),
        Err(err("invalid_rule", Some("standards_ref")))
    );
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_not_decidable_requires_a_reason_at_the_parse_boundary() {
    // The type-level half is the `compile_fail` doctest on `Outcome`.
    assert_eq!(Outcome::from_wire("NOT_DECIDABLE", None), Err(DecError::ReasonRequired));
    assert_eq!(DecError::ReasonRequired.code(), "reason_required");
    assert_eq!(
        Outcome::from_wire("NOT_DECIDABLE", Some("u_missing")),
        Ok(Outcome::NotDecidable(NotDecidableReason::UMissing))
    );
    assert_eq!(Outcome::from_wire("NOT_DECIDABLE", Some("unknown")), Err(DecError::UnknownReason));
    assert_eq!(Outcome::from_wire("CONFORMS", Some("u_missing")), Err(DecError::UnexpectedReason));
    assert_eq!(Outcome::from_wire("conforms", None), Err(DecError::UnknownOutcome));
    assert_eq!(Outcome::from_wire("PASS", None), Err(DecError::UnknownOutcome));
    // REQ-002: exactly four values and four reasons.
    assert_eq!(Outcome::WIRE, ["CONFORMS", "NONCONFORMS", "INDETERMINATE", "NOT_DECIDABLE"]);
    let reasons: Vec<&str> = NotDecidableReason::ALL.iter().map(|r| r.wire()).collect();
    assert_eq!(reasons, ["u_missing", "rule_missing", "pdf_unavailable", "label_only"]);
    assert_eq!(Outcome::NotDecidable(NotDecidableReason::LabelOnly).to_string(), "NOT_DECIDABLE(label_only)");
    assert!(Outcome::Conforms.is_decided() && Outcome::Nonconforms.is_decided());
    assert!(!Outcome::Indeterminate.is_decided());
}

// ------------------------------------------------------------------ catalog errors

fn catalog_with(rules: &[&str]) -> Vec<u8> {
    format!("{{\"catalog\":\"{CATALOG_TAG}\",\"rules\":[{}]}}", rules.join(",")).into_bytes()
}

const SIMPLE: &str = r#"{"id":"dr:simple-acceptance","kind":"simple_acceptance","params":{},"pdf_source":"mc_or_validated_normal","risk_reporting":"specific","standards_ref":"none","version":"1"}"#;

#[test]
fn catalog_errors() {
    let code = |b: &[u8]| Catalog::parse_canonical(b).map_err(of).err();
    assert_eq!(code(&catalog_with(&[SIMPLE])), None);
    assert_eq!(code(&catalog_with(&[SIMPLE, SIMPLE])), Some(err("duplicate_rule", None)));
    let v2 = SIMPLE.replace("\"version\":\"1\"", "\"version\":\"2\"");
    assert_eq!(code(&catalog_with(&[SIMPLE, &v2])), None);
    // Out of order: valid but not canonical.
    assert_eq!(code(&catalog_with(&[&v2, SIMPLE])), Some(err("non_canonical", None)));
    let spaced = String::from_utf8(catalog_with(&[SIMPLE])).unwrap().replace(",\"rules\"", ", \"rules\"");
    assert_eq!(code(spaced.as_bytes()), Some(err("non_canonical", None)));
    let cust = SIMPLE.replace("dr:simple-acceptance", "dr:customer/a/b");
    assert_eq!(code(&catalog_with(&[&cust])), Some(err("invalid_rule", Some("id"))));
    let inst = SIMPLE.replace("dr:simple-acceptance", "dr:guard-band(r=1)").replace("simple_acceptance", "guard_band");
    assert_eq!(code(&catalog_with(&[&inst])), Some(err("invalid_rule", Some("id"))));
    let wrong_kind = SIMPLE.replace("\"simple_acceptance\"", "\"guard_band\"");
    assert_eq!(code(&catalog_with(&[&wrong_kind])), Some(err("invalid_rule", Some("kind"))));
    let tag = String::from_utf8(catalog_with(&[SIMPLE])).unwrap().replace("dr-catalog@1", "dr-catalog@2");
    assert_eq!(code(tag.as_bytes()), Some(err("malformed", Some("catalog_tag"))));
    let extra = String::from_utf8(catalog_with(&[SIMPLE])).unwrap().replace("{\"catalog\"", "{\"x\":\"1\",\"catalog\"");
    assert_eq!(code(extra.as_bytes()), Some(err("malformed", Some("unknown_member"))));
    let number = SIMPLE.replace("\"version\":\"1\"", "\"version\":1");
    assert_eq!(code(&catalog_with(&[&number])), Some(err("malformed", Some("unsupported_value"))));
    let big = vec![b' '; fid_kdec::rule::MAX_CATALOG_BYTES + 1];
    assert_eq!(code(&big), Some(err("malformed", Some("too_large"))));
    let many: Vec<String> =
        (1..=257).map(|v| SIMPLE.replace("\"version\":\"1\"", &format!("\"version\":\"{v}\""))).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    assert_eq!(code(&catalog_with(&many)), Some(err("malformed", Some("too_many_rules"))));
}

#[test]
fn resolve_binds_guard_band_instances_and_refuses_the_rest() {
    let c = Catalog::builtin().unwrap();
    let inst = c.resolve(&RuleRef::parse("dr:guard-band(r=0.5)@1").unwrap()).unwrap();
    assert_eq!(inst.params(), &RuleParams::GuardBand { r: Some(dec("0.5")) });
    assert_eq!(inst.rule_ref().to_string(), "dr:guard-band(r=0.5)@1");
    assert_eq!(inst.standards_ref(), c.get(&RuleId::GuardBand { r: None }, 1).unwrap().standards_ref());
    let RuleParams::GuardBand { r: Some(r) } = inst.params() else { panic!() };
    assert_eq!(r.to_f64("r"), Ok(0.5));

    let iso = c.resolve(&RuleRef::parse("dr:iso14253-1:2017/default@1").unwrap()).unwrap();
    let RuleParams::ConformanceProbability { p_min } = iso.params() else { panic!() };
    assert_eq!(p_min.to_f64("p_min"), Ok(0.95));

    let refuse = |r: RuleRef| c.resolve(&r).map_err(of).err();
    assert_eq!(refuse(RuleRef::parse("dr:simple-acceptance@2").unwrap()), Some(err("not_in_catalog", None)));
    assert_eq!(refuse(RuleRef::parse("dr:guard-band(r=1)@2").unwrap()), Some(err("not_in_catalog", None)));
    assert_eq!(refuse(RuleRef::parse("dr:customer/acme/bore@1").unwrap()), Some(err("not_in_catalog", None)));
    // Hand-built references outside the grammar.
    let neg = RuleRef { id: RuleId::GuardBand { r: Some(dec("-1")) }, version: 1 };
    assert_eq!(refuse(neg), Some(err("invalid_rule", Some("r"))));
    assert_eq!(refuse(RuleRef { id: RuleId::SimpleAcceptance, version: 0 }), Some(err("invalid_rule_id", None)));
    let long = RuleRef { id: RuleId::Customer { tenant: "a".repeat(65), name: "b".into() }, version: 1 };
    assert_eq!(refuse(long), Some(err("invalid_rule_id", None)));
}

// ------------------------------------------------------------------ decisions and evidence

#[test]
fn evidence_facts_travel_with_every_decision() {
    let c = Catalog::builtin().unwrap();
    let iso = c.get(&RuleId::Iso14253Default, 1).unwrap();
    // (statement, claimed, declared input, uncharacterized input)
    let cases = [
        ("uec1_declared", true, true, true),
        ("uec1_spec_derived", true, false, true),
        ("uec1_lpu", false, false, true),
        ("uec2_lpu", false, false, false),
    ];
    for (name, claimed, declared, uncharacterized) in cases {
        let stmt = statement(name);
        let basis = EvidenceBasis::of(&stmt).unwrap();
        assert_eq!(basis.statement(), stmt.digest());
        for outcome in [Outcome::Conforms, Outcome::Nonconforms, Outcome::Indeterminate] {
            let d = Decision::new(Some(iso), outcome, Some(basis.clone())).unwrap();
            assert_eq!(
                (d.is_claimed(), d.rests_on_declared_input(), d.rests_on_uncharacterized_input()),
                (claimed, declared, uncharacterized),
                "{name} {outcome}"
            );
            assert_eq!(d.rule().map(ToString::to_string).as_deref(), Some("dr:iso14253-1:2017/default@1"));
        }
    }
}

#[test]
fn decision_outcomes_must_fit_rule_and_basis() {
    let c = Catalog::builtin().unwrap();
    let iso = c.get(&RuleId::Iso14253Default, 1).unwrap();
    let simple = c.get(&RuleId::SimpleAcceptance, 1).unwrap();
    let label = c.get(&RuleId::AsReportedBySource, 1).unwrap();
    let u = EvidenceBasis::of(&statement("uec2_lpu")).unwrap();
    let u0 = EvidenceBasis::of(&statement("uec0_u_missing")).unwrap();
    assert_eq!(u0.class(), EvidenceClass::Uec0);
    let nd = Outcome::NotDecidable;
    let check = |rule: Option<&DecisionRule>, o: Outcome, b: Option<&EvidenceBasis>| {
        Decision::new(rule, o, b.cloned()).map(|_| ()).map_err(of).err()
    };
    let mismatch = |f: &str| Some(err("outcome_mismatch", Some(f)));

    assert_eq!(check(None, nd(NotDecidableReason::RuleMissing), Some(&u)), None);
    assert_eq!(check(None, Outcome::Conforms, Some(&u)), mismatch("rule_missing"));
    assert_eq!(check(Some(iso), nd(NotDecidableReason::RuleMissing), Some(&u)), mismatch("rule_missing"));
    assert_eq!(check(Some(label), nd(NotDecidableReason::LabelOnly), Some(&u)), None);
    assert_eq!(check(Some(label), Outcome::Conforms, Some(&u)), mismatch("label_only"));
    assert_eq!(check(Some(iso), nd(NotDecidableReason::LabelOnly), Some(&u)), mismatch("label_only"));
    assert_eq!(check(Some(iso), nd(NotDecidableReason::UMissing), Some(&u0)), None);
    assert_eq!(check(Some(iso), nd(NotDecidableReason::UMissing), None), None);
    assert_eq!(check(Some(iso), nd(NotDecidableReason::UMissing), Some(&u)), mismatch("u_missing"));
    for o in [Outcome::Conforms, Outcome::Nonconforms, Outcome::Indeterminate] {
        assert_eq!(check(Some(simple), o, Some(&u)), None);
        assert_eq!(check(Some(simple), o, Some(&u0)), mismatch("basis"));
        assert_eq!(check(Some(simple), o, None), mismatch("basis"));
    }
    assert_eq!(check(Some(iso), nd(NotDecidableReason::PdfUnavailable), Some(&u)), None);
    assert_eq!(check(Some(simple), nd(NotDecidableReason::PdfUnavailable), Some(&u)), mismatch("pdf_unavailable"));
    // No basis means no facts, and no measured-looking decision.
    let d = Decision::new(None, nd(NotDecidableReason::RuleMissing), None).unwrap();
    assert!(!d.is_claimed() && d.basis().is_none() && d.rule().is_none());
}

#[test]
fn subnormal_statement_numbers_are_refused() {
    // UEC-2/3 statements are built only by UNC.UEC.CLASS, so the edited copies are UEC-1;
    // the subnormal check does not depend on the class.
    let mut base = statement("uec2_lpu").to_parts();
    base.class = EvidenceClass::Uec1;
    let sub = f64::from_bits(1);
    let mut cases: Vec<(&str, fid_kunc::StatementParts)> = Vec::new();
    let mut p = base.clone();
    p.u_std = Some(sub);
    cases.push(("u_std", p));
    let mut p = base.clone();
    if let Some(e) = p.expanded.as_mut() {
        e.value = sub;
    }
    cases.push(("expanded.U", p));
    let mut p = base;
    if let Some(e) = p.expanded.as_mut() {
        e.k = f64::from_bits(0x0008_0000_0000_0000);
    }
    cases.push(("expanded.k", p));
    let mut p = statement("uec1_spec_derived").to_parts();
    if let Some(i) = p.interval.as_mut() {
        i.low = -sub;
    }
    cases.push(("interval.low", p));
    for (field, parts) in cases {
        // fid-kunc accepts these (finite, positive); the decision boundary does not.
        let stmt = UncertaintyStatement::new(parts).unwrap();
        assert_eq!(EvidenceBasis::of(&stmt).map_err(of), Err(err("invalid_number", Some(field))), "{field}");
    }
}

#[test]
fn param_conversion_refuses_nothing_valid_and_is_exact() {
    // Expected values from the platform's correctly rounded decimal parser.
    for s in ["0.95", "0", "2", "0.1", "12345678901234567890", "0.00000000000000000001"] {
        let want: f64 = s.parse().unwrap();
        assert_eq!(dec(s).to_f64("r").map(f64::to_bits), Ok(want.to_bits()), "{s}");
    }
}

#[test]
fn unbound_guard_band_template_never_carries_an_outcome() {
    let c = Catalog::builtin().unwrap();
    let template = c.get(&RuleId::GuardBand { r: None }, 1).unwrap();
    let u = EvidenceBasis::of(&statement("uec2_lpu")).unwrap();
    let u0 = EvidenceBasis::of(&statement("uec0_u_missing")).unwrap();
    let all = [
        Outcome::Conforms,
        Outcome::Nonconforms,
        Outcome::Indeterminate,
        Outcome::NotDecidable(NotDecidableReason::UMissing),
        Outcome::NotDecidable(NotDecidableReason::PdfUnavailable),
    ];
    for o in all {
        for b in [Some(&u), Some(&u0), None] {
            let got = Decision::new(Some(template), o, b.cloned()).map_err(of).err();
            assert_eq!(got, Some(err("invalid_rule", Some("r"))), "{o}");
        }
    }
    // The two exempt reasons are refused earlier, by their own checks.
    let rm = Decision::new(Some(template), Outcome::NotDecidable(NotDecidableReason::RuleMissing), Some(u.clone()));
    assert_eq!(rm.map_err(of).err(), Some(err("outcome_mismatch", Some("rule_missing"))));
    let lo = Decision::new(Some(template), Outcome::NotDecidable(NotDecidableReason::LabelOnly), Some(u.clone()));
    assert_eq!(lo.map_err(of).err(), Some(err("outcome_mismatch", Some("label_only"))));
    // A resolved instance decides.
    let inst = c.resolve(&RuleRef::parse("dr:guard-band(r=0.5)@1").unwrap()).unwrap();
    for o in [Outcome::Conforms, Outcome::Nonconforms, Outcome::Indeterminate] {
        let d = Decision::new(Some(&inst), o, Some(u.clone())).unwrap();
        assert_eq!(d.rule().map(ToString::to_string).as_deref(), Some("dr:guard-band(r=0.5)@1"));
    }
}

#[test]
fn catalog_new_writes_only_bytes_that_parse_back() {
    let c = Catalog::builtin().unwrap();
    // Round trip: every catalog `new` accepts parses back to itself.
    let rebuilt = Catalog::new(c.rules().to_vec()).unwrap();
    assert_eq!(Catalog::parse_canonical(rebuilt.canonical_json()).unwrap(), rebuilt);
    let subset = Catalog::new(c.rules().iter().take(2).cloned().collect()).unwrap();
    assert_eq!(Catalog::parse_canonical(subset.canonical_json()).unwrap(), subset);
    // A resolved guard-band instance is not a template: `new` refuses it like the parser.
    let inst = c.resolve(&RuleRef::parse("dr:guard-band(r=0.5)@1").unwrap()).unwrap();
    let mut rules = c.rules().to_vec();
    rules.push(inst.clone());
    assert_eq!(Catalog::new(rules).map_err(of).err(), Some(err("invalid_rule", Some("id"))));
    let bytes = DecisionRule::canonical_json(&inst).unwrap();
    let doc = format!("{{\"catalog\":\"{CATALOG_TAG}\",\"rules\":[{}]}}", String::from_utf8(bytes).unwrap());
    assert_eq!(Catalog::parse_canonical(doc.as_bytes()).map_err(of).err(), Some(err("invalid_rule", Some("id"))));
    // Customer rules likewise.
    let cust = DecisionRule::customer(customer(RuleParams::SimpleAcceptance)).unwrap();
    assert_eq!(Catalog::new(vec![cust]).map_err(of).err(), Some(err("invalid_rule", Some("id"))));
}

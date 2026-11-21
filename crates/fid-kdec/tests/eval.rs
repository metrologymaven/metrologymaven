//! UNC.DEC.EVAL acceptance tests. Expected outcomes, probabilities, flags and errors come
//! from the independent stdlib-Python reference `fixtures/unc/dec/eval/make_cases.py`
//! (exact rational comparisons, 100-digit normal CDF), never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kdec::eval::{ALGORITHM_ID, DEFAULT_WARN_RATIO, OUTCOME_REPLAY_KIND};
use fid_kdec::{
    evaluate, Catalog, DecisionRecord, DecisionRule, EvalError, EvalInput, Flag, Outcome, ParamDecimal, PdfSource,
    RiskReporting, RuleParams, RuleParts, RuleRef, Samples, Tolerance,
};
use fid_kunc::prop::mcm::samples::{decode, SampleSet, DEFAULT_CAP};
use fid_kunc::{
    CoverageInterval, EvidenceClass, Expanded, IntervalKind, Method, ModelRef, NlinVerdict, Nonlinearity,
    PrincipalKind, SourceKind, SourceRef, StatementParts, Uec0Reason, UncertaintyStatement,
};
use fid_num::kdec::Decimal;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/dec/eval/");

fn read(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

const HEADER: &str = "id,rule,lsl,usl,unit,value,class,method,src,u_std,U,lpu_validated,nlin,selection,samples,\
output,warn,outcome,reason,p,U_out,flags,error";

struct Row {
    cols: Vec<String>,
}

impl Row {
    fn get(&self, name: &str) -> &str {
        let i = HEADER.split(',').position(|h| h == name).unwrap_or_else(|| panic!("column {name}"));
        &self.cols[i]
    }
    fn f64(&self, name: &str) -> Option<f64> {
        let s = self.get(name);
        if s.is_empty() {
            None
        } else {
            Some(s.parse::<f64>().unwrap_or_else(|e| panic!("{name}={s}: {e}")))
        }
    }
    fn dec(&self, name: &str) -> Option<Decimal> {
        let s = self.get(name);
        if s.is_empty() {
            None
        } else {
            Some(Decimal::parse(s).unwrap())
        }
    }
}

fn rows() -> Vec<Row> {
    let text = String::from_utf8(read("cases.csv")).unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some(HEADER));
    let n = HEADER.split(',').count();
    lines
        .map(|l| {
            let cols: Vec<String> = l.split(',').map(str::to_owned).collect();
            assert_eq!(cols.len(), n, "{l}");
            Row { cols }
        })
        .collect()
}

fn digest(label: &str) -> fid_types::Digest {
    fid_kci::kci::sha256_digest(label.as_bytes())
}

fn statement(r: &Row) -> UncertaintyStatement {
    if r.get("class") == "UEC-0" {
        let mut p = StatementParts::new(EvidenceClass::Uec0);
        p.uec0_reason = Some(Uec0Reason::UMissing);
        return UncertaintyStatement::new(p).unwrap();
    }
    assert_eq!(r.get("class"), "UEC-1");
    let mut p = StatementParts::new(EvidenceClass::Uec1);
    let method = Method::from_wire(r.get("method")).unwrap();
    p.method = Some(method);
    p.unit = Some("si:m".to_owned());
    p.u_std = r.f64("u_std");
    match r.f64("U") {
        Some(u) => p.expanded = Some(Expanded { value: u, k: 2.0 }),
        None => p.interval = Some(CoverageInterval { low: -1.0, high: 1.0, p: 0.95, kind: IntervalKind::Shortest }),
    }
    p.model = Some(ModelRef { id: "um:eval".to_owned(), semver: "1.0.0".to_owned(), digest: digest("um:eval") });
    for s in r.get("src").split(';').filter(|s| !s.is_empty()) {
        let (kind, unverified) = match s.strip_suffix('!') {
            Some(k) => (k, true),
            None => (s, false),
        };
        let id = format!("src:{kind}");
        p.sources.push(SourceRef { kind: SourceKind::from_wire(kind).unwrap(), digest: digest(&id), id, unverified });
    }
    p.sources.sort_by(|a, b| a.id.cmp(&b.id));
    p.lpu_validated_by_mc = match r.get("lpu_validated") {
        "" => None,
        "true" => Some(true),
        "false" => Some(false),
        other => panic!("{other}"),
    };
    let nlin = r.get("nlin");
    if !nlin.is_empty() {
        p.nonlinearity = Some(Nonlinearity {
            version: "nlin@1".to_owned(),
            verdict: NlinVerdict::from_wire(nlin).unwrap(),
            v1: 1.0e-12,
            v2: 0.0,
            ratio: None,
            probed_inputs: 1,
            probed_pairs: 0,
            cross_terms_truncated: false,
            reason: None,
        });
    }
    let sel = r.get("selection");
    p.selection_rule = if sel.is_empty() { None } else { Some(sel.to_owned()) };
    if method == Method::Declared {
        p.provenance.principal = Some("person:alice".to_owned());
        p.provenance.principal_kind = Some(PrincipalKind::Human);
    }
    UncertaintyStatement::new(p).unwrap_or_else(|e| panic!("{}: {e:?}", r.get("id")))
}

fn rule(text: &str) -> DecisionRule {
    if let Some((reference, rest)) = text.split_once('|') {
        let parts: Vec<&str> = rest.split('|').collect();
        assert_eq!(parts[0], "cp");
        let rr = RuleRef::parse(reference).unwrap();
        return DecisionRule::customer(RuleParts {
            id: rr.id,
            version: rr.version,
            standards_ref: None,
            params: RuleParams::ConformanceProbability { p_min: ParamDecimal::parse(parts[1]).unwrap() },
            pdf_source: PdfSource::from_wire(parts[2]).unwrap(),
            risk_reporting: RiskReporting::Specific,
        })
        .unwrap();
    }
    Catalog::builtin().unwrap().resolve(&RuleRef::parse(text).unwrap()).unwrap()
}

fn sample_set(name: &str) -> SampleSet {
    decode(&read(&format!("samples/{name}.bin")), DEFAULT_CAP).unwrap()
}

fn run(r: &Row) -> Result<DecisionRecord, EvalError> {
    let stmt = statement(r);
    let rule = rule(r.get("rule"));
    let tol = Tolerance { lsl: r.dec("lsl"), usl: r.dec("usl"), unit: r.get("unit").to_owned() };
    let set = if r.get("samples").is_empty() { None } else { Some(sample_set(r.get("samples"))) };
    let warn = Decimal::parse(if r.get("warn").is_empty() { DEFAULT_WARN_RATIO } else { r.get("warn") }).unwrap();
    let input = EvalInput {
        value: r.f64("value").unwrap(),
        statement: &stmt,
        samples: set.as_ref().map(|set| Samples { set, output: r.get("output") }),
        tolerance: &tol,
        rule: &rule,
        warn_ratio: warn,
    };
    evaluate(&input)
}

fn flags_text(rec: &DecisionRecord) -> String {
    rec.flags().iter().map(|f| f.wire()).collect::<Vec<_>>().join(";")
}

/// AC-1: every row of `cases.csv` (outcome, reason, p within 1e-12, U, flags, error).
#[test]
fn ac1_cases() {
    let rows = rows();
    assert!(rows.len() >= 80, "{}", rows.len());
    for r in &rows {
        let id = r.get("id");
        let got = run(r);
        let err = r.get("error");
        if !err.is_empty() {
            let e = got.err().unwrap_or_else(|| panic!("{id}: expected error {err}"));
            assert_eq!(e.code(), err, "{id}");
            continue;
        }
        let rec = got.unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(rec.outcome().wire(), r.get("outcome"), "{id}");
        assert_eq!(rec.outcome().reason().map(|x| x.wire()).unwrap_or(""), r.get("reason"), "{id}");
        match (rec.p_conformance(), r.f64("p")) {
            (None, None) => {}
            (Some(a), Some(b)) => assert!((a - b).abs() <= 1e-12, "{id}: p {a} vs {b}"),
            (a, b) => panic!("{id}: p {a:?} vs {b:?}"),
        }
        assert_eq!(rec.expanded_u().map(f64::to_bits), r.f64("U_out").map(f64::to_bits), "{id}");
        assert_eq!(flags_text(&rec), r.get("flags"), "{id}");
        // REQ-UNC.DEC.EVAL-001: NOT_DECIDABLE always carries its reason; the rule is named.
        assert_eq!(rec.decision().rule().map(|x| x.to_string()), Some(rule(r.get("rule")).rule_ref().to_string()));
        // D-045: provisional labels are the statement's, and survive CONFORMS.
        let basis = rec.decision().basis().unwrap();
        assert_eq!(rec.has_flag(Flag::ClaimedUncertainty), basis.is_claimed(), "{id}");
        assert_eq!(rec.is_provisional(), basis.class() == EvidenceClass::Uec1 || basis.is_claimed(), "{id}");
    }
}

fn row(id: &str) -> Row {
    rows().into_iter().find(|r| r.get("id") == id).unwrap_or_else(|| panic!("{id}"))
}

/// AC-2: one ulp past an exact decimal limit is outside under simple acceptance.
#[test]
fn ac2_simple_just_over() {
    let r = row("simple_just_over");
    assert_eq!(r.f64("value"), Some(0.010_050_000_4));
    assert_eq!(run(&r).unwrap().outcome(), Outcome::Nonconforms);
    // The nearest binary64 of a limit is put on its true side, both ways.
    assert_eq!(run(&row("simple_at_usl_up")).unwrap().outcome(), Outcome::Nonconforms);
    assert_eq!(run(&row("simple_at_lsl_down")).unwrap().outcome(), Outcome::Nonconforms);
}

/// AC-3: UEC-0 is `NOT_DECIDABLE(u_missing)` under every kernel rule kind. Deviation: under
/// simple acceptance too (fail closed; see NODE.md).
#[test]
fn ac3_uec0() {
    for id in ["uec0_iso", "uec0_simple", "uec0_gb"] {
        let rec = run(&row(id)).unwrap();
        assert_eq!(rec.outcome().to_string(), "NOT_DECIDABLE(u_missing)", "{id}");
        assert_eq!(rec.p_conformance(), None);
        assert_eq!(rec.expanded_u(), None);
    }
    assert_eq!(run(&row("uec0_label")).unwrap().outcome().to_string(), "NOT_DECIDABLE(label_only)");
}

/// AC-4: MCM-only statement without samples (no LPU validation) is `pdf_unavailable`.
#[test]
fn ac4_pdf_unavailable() {
    for id in ["cp_mcm_no_samples", "cp_lpu_not_validated", "cp_mc_only_lpu", "declared_cp"] {
        assert_eq!(run(&row(id)).unwrap().outcome().to_string(), "NOT_DECIDABLE(pdf_unavailable)", "{id}");
    }
}

/// AC-5: r = 1 and U >= T / 2 gives NONCONFORMS with `empty_acceptance_zone`.
#[test]
fn ac5_empty_acceptance_zone() {
    for id in ["gb_empty_exact", "gb_empty_half", "gb_empty_over"] {
        let rec = run(&row(id)).unwrap();
        assert_eq!(rec.outcome(), Outcome::Nonconforms, "{id}");
        assert!(rec.has_flag(Flag::EmptyAcceptanceZone), "{id}");
    }
    assert!(!run(&row("gb_half_r")).unwrap().has_flag(Flag::EmptyAcceptanceZone));
}

/// AC-6: two evaluations whose p differs by about 1e-7 across p_min flip the outcome, and
/// the REP-1 classifier calls the outcome field DIVERGENT.
#[test]
fn ac6_outcome_flip_is_divergent() {
    let a = run(&row("rep_flip_a")).unwrap();
    let b = run(&row("rep_flip_b")).unwrap();
    assert_eq!((a.outcome(), b.outcome()), (Outcome::Conforms, Outcome::Indeterminate));
    let (pa, pb) = (a.p_conformance().unwrap(), b.p_conformance().unwrap());
    assert!(pa > 0.95 && pb < 0.95 && (pa - pb) < 2e-7 && (pa - pb) > 5e-8, "{pa} {pb}");
    let kind = fid_rep::FieldKind::parse(OUTCOME_REPLAY_KIND).unwrap();
    assert_eq!(kind, fid_rep::FieldKind::Exact);
    let o = fid_rep::classify_exact_bytes(a.outcome().to_string().as_bytes(), b.outcome().to_string().as_bytes());
    assert_eq!(o.outcome, fid_rep::Outcome::Divergent);
    // The same outcome replays IDENTICAL.
    let same = fid_rep::classify_exact_bytes(a.outcome().to_string().as_bytes(), b"CONFORMS");
    assert_eq!(same.outcome, fid_rep::Outcome::Identical);
}

/// REQ-UNC.DEC.EVAL-008: the spec card declares the algorithm and the outcome field `exact`.
#[test]
fn spec_card_declares_outcome_exact() {
    let text =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/cards/dec.rule.evaluate@1.0.0.toml")).unwrap();
    let card = fid_kci::parse_card(&text).unwrap();
    assert_eq!(card.id.to_string(), ALGORITHM_ID);
    assert_eq!(card.determinism, "D0");
    assert!(card.validation.contains("outcome field: replay equivalence class exact"), "{}", card.validation);
}

/// AC-7: outputs of AC-1 are bit-identical on every target. `crate_bits.txt` pins the
/// crate's own output (not an oracle; AC-1 checks the values against the oracle); CI runs
/// this test on wasm32-wasip1, x86-64 (Linux, Windows) and ARM64 (macOS). Regenerate with
/// `UNC_DEC_EVAL_PRINT_BITS=1`.
#[test]
fn ac7_bits_are_pinned() {
    let mut lines = vec!["# UNC.DEC.EVAL bit pins: crate output, regenerate with UNC_DEC_EVAL_PRINT_BITS=1".to_owned()];
    for r in rows() {
        let line = match run(&r) {
            Err(e) => format!("{} error {}", r.get("id"), e.code()),
            Ok(rec) => format!(
                "{} {} p={} U={} flags={}",
                r.get("id"),
                rec.outcome(),
                rec.p_conformance().map_or("none".to_owned(), |p| format!("{:016x}", p.to_bits())),
                rec.expanded_u().map_or("none".to_owned(), |u| format!("{:016x}", u.to_bits())),
                flags_text(&rec)
            ),
        };
        lines.push(line);
    }
    let text = lines.join("\n") + "\n";
    if std::env::var_os("UNC_DEC_EVAL_PRINT_BITS").is_some() {
        std::fs::write(format!("{FIX}crate_bits.txt"), &text).unwrap();
    }
    assert_eq!(text, String::from_utf8(read("crate_bits.txt")).unwrap());
}

/// The guard-band template (r unbound) never evaluates; a hand-built rule is still checked.
#[test]
fn template_refused() {
    let e = run(&row("err_template")).unwrap_err();
    assert_eq!(e.code(), "invalid_rule");
    assert_eq!(e.to_string(), "invalid_rule: r");
}

/// Samples are compared exactly at the binary64 neighbours of the decimal limits.
#[test]
fn boundary_samples_counted_exactly() {
    let rec = run(&row("sa_mc_boundary")).unwrap();
    let set = sample_set("boundary");
    let lsl = Decimal::parse("0.00999").unwrap();
    let usl = Decimal::parse("0.01001").unwrap();
    let k = set.outputs[0]
        .iter()
        .filter(|&&x| {
            fid_num::kdec::cmp(x, &lsl).unwrap() != std::cmp::Ordering::Less
                && fid_num::kdec::cmp(x, &usl).unwrap() != std::cmp::Ordering::Greater
        })
        .count();
    assert_eq!(rec.p_conformance(), Some(k as f64 / set.outputs[0].len() as f64));
}

/// Performance budget probe (not a gate): 10^6 samples are counted with one binary64
/// comparison per limit and sample.
#[test]
fn million_samples_count() {
    let mut set = sample_set("center");
    let col: Vec<f64> = (0..1_000_000u32).map(|i| 0.00999 + f64::from(i) * 2.0e-11).collect();
    set.header.trials = col.len() as u64;
    set.header.failed = 0;
    set.failed.clear();
    set.outputs = vec![col];
    let stmt = statement(&row("cp_mc_center"));
    let rule = rule("dr:iso14253-1:2017/default@1");
    let tol = Tolerance {
        lsl: Some(Decimal::parse("0.00999").unwrap()),
        usl: Some(Decimal::parse("0.01001").unwrap()),
        unit: "si:m".to_owned(),
    };
    let input = EvalInput {
        value: 0.01,
        statement: &stmt,
        samples: Some(Samples { set: &set, output: "y" }),
        tolerance: &tol,
        rule: &rule,
        warn_ratio: Decimal::parse(DEFAULT_WARN_RATIO).unwrap(),
    };
    let rec = evaluate(&input).unwrap();
    assert!(rec.p_conformance().unwrap() > 0.99);
}

/// erfc against the 100-digit reference values of `erfc.csv`: absolute error <= 2e-15,
/// relative error <= 2e-13 where the value is normal.
#[test]
fn erfc_matches_reference() {
    let text = String::from_utf8(read("erfc.csv")).unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("x,erfc"));
    let mut n = 0;
    for l in lines {
        let (x, v) = l.split_once(',').unwrap();
        let x: f64 = x.parse().unwrap();
        let v: f64 = v.parse().unwrap();
        let got = fid_kdec::eval::normal::erfc(x).unwrap();
        let abs = (got - v).abs();
        assert!(abs <= 2e-15, "x={x}: {got} vs {v} (abs {abs})");
        if v > 1e-300 {
            let rel = abs / v;
            assert!(rel <= 2e-13, "x={x}: {got} vs {v} (rel {rel})");
        }
        n += 1;
    }
    assert!(n > 300);
}

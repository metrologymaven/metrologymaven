//! Acceptance criteria of spec node RPT.MODEL.FMT (AC-1 .. AC-6 and AC-8 .. AC-11; AC-7 is
//! the lint, tested in `tools/tests/test_rpt_float_lint.py`).
//!
//! Expected strings, digit counts, sides and tie flags come from the independent reference
//! `fixtures/rpt/model/fmt/make_cases.py` (exact rationals, stdlib Python). Labels, markers,
//! warnings and error reasons below are hand-labelled from the leaf's requirements.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::disallowed_types)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fid_canon::{parse, CValue};
use fid_gdt::chars::record::EvaluationBasis;
use fid_io_res::trans::model::{load_rows, transcribe, Transcription};
use fid_kdec::{Catalog as RuleCatalog, Decision, EvidenceBasis, NotDecidableReason, Outcome, RuleRef};
use fid_kunc::{EvidenceClass, UncertaintyStatement};
use fid_num::fmt::ang::{format_with_ladder, PiEnclosure};
use fid_num::fmt::{Conversion, FormatSpec, FormatterId, Reported, SideOutcome, Unit};
use fid_num::kdec::Decimal;
use fid_project::{IdService, ReplaySource, Uuid};
use fid_report::fmt::{
    fmt_outcome, fmt_requirement, fmt_value, fmt_value_with, labels, Catalog, CatalogError, Characteristic,
    DefaultDecimals, FmtContext, FmtError, GaugeId, Label, LimitSet, LimitUnit, LocaleError, LocaleRules, Marker,
    SiValue, Side, UnitOverride, ValueKind, ValueRecord, ValueText, WARN_DISPLAY_TIE, WARN_DISPLAY_UNDECIDABLE,
};

// ------------------------------------------------------------------ helpers

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap()
}

fn json(rel: &str) -> CValue {
    parse(&read(rel)).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(k).unwrap_or_else(|| panic!("missing {k}")),
        _ => panic!("not an object"),
    }
}

fn s(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("not a string: {other:?}"),
    }
}

fn opt_s(v: &CValue) -> Option<&str> {
    match v {
        CValue::Null => None,
        other => Some(s(other)),
    }
}

fn int(v: &CValue) -> u64 {
    match v {
        CValue::I64(i) => u64::try_from(*i).unwrap(),
        CValue::U64(u) => *u,
        CValue::Num(n) => {
            assert!(n.fract() == 0.0 && *n >= 0.0);
            *n as u64
        }
        other => panic!("not an integer: {other:?}"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("not an array"),
    }
}

fn stmt(name: &str) -> UncertaintyStatement {
    UncertaintyStatement::parse_canonical(&read(&format!("fixtures/unc/stmt/valid/{name}"))).unwrap()
}

fn decision(rule: Option<&str>, outcome: Outcome, st: &UncertaintyStatement) -> Decision {
    let cat = RuleCatalog::builtin().unwrap();
    let rule = rule.map(|r| cat.resolve(&RuleRef::parse(r).unwrap()).unwrap());
    Decision::new(rule.as_ref(), outcome, Some(EvidenceBasis::of(st).unwrap())).unwrap()
}

fn dec(s: &str) -> Decimal {
    Decimal::parse(s).unwrap()
}

fn limits(lower: Option<&str>, upper: Option<&str>, unit: LimitUnit) -> LimitSet {
    LimitSet { lower: lower.map(dec), upper: upper.map(dec), unit }
}

fn en() -> LocaleRules {
    LocaleRules::new("en", "test-1", '.', Some(','), 3, 3, "-").unwrap()
}

fn de() -> LocaleRules {
    LocaleRules::new("de", "test-1", ',', Some('.'), 3, 3, "-").unwrap()
}

fn ctx(locale: LocaleRules, catalog: &Catalog) -> FmtContext<'_> {
    FmtContext { locale, units: UnitOverride::NONE, default_decimals: DefaultDecimals::initial(), catalog }
}

fn unit_of(sym: &str) -> Unit {
    [Unit::M, Unit::Mm, Unit::Um, Unit::In, Unit::One, Unit::Rad, Unit::Deg, Unit::Dms]
        .into_iter()
        .find(|u| u.symbol() == sym)
        .unwrap()
}

fn kind_of(k: &str) -> ValueKind {
    match k {
        "length" => ValueKind::Length,
        "angle" => ValueKind::Angle,
        "scalar" => ValueKind::Scalar,
        _ => panic!("kind {k}"),
    }
}

fn limit_set(v: &CValue) -> Option<LimitSet> {
    if *v == CValue::Null {
        return None;
    }
    let unit = match s(get(v, "unit")) {
        "si" => LimitUnit::Si,
        "deg" => LimitUnit::Deg,
        u => panic!("limit unit {u}"),
    };
    Some(limits(opt_s(get(v, "lower")), opt_s(get(v, "upper")), unit))
}

fn ch(id: &str, kind: ValueKind, unit: Unit, decimals: Option<u8>, l: Option<LimitSet>) -> Characteristic {
    Characteristic { id: id.to_owned(), kind: Some(kind), unit, decimals, limits: l, nominal: None }
}

fn rec<'a>(id: &str, value: f64, d: &'a Decision, st: &'a UncertaintyStatement) -> ValueRecord<'a> {
    ValueRecord {
        id: id.to_owned(),
        value: SiValue::new(value),
        basis: EvaluationBasis::Points,
        decision: d,
        statement: st,
        acceptance: None,
        transcription: None,
        gauge: None,
    }
}

fn unformattable(r: Result<ValueText, FmtError>) -> &'static str {
    match r {
        Err(FmtError::Unformattable { reason, .. }) => reason,
        other => panic!("expected report.unformattable, got {other:?}"),
    }
}

fn markers(v: &ValueText) -> Vec<Marker> {
    v.markers().iter().map(|m| m.marker).collect()
}

fn label_set(v: &ValueText) -> Vec<Label> {
    v.labels().iter().map(|l| l.label.clone()).collect()
}

fn claim_labels(v: &ValueText) -> BTreeSet<&'static str> {
    v.labels()
        .iter()
        .filter_map(|l| match l.label {
            Label::Claimed => Some("Claimed"),
            Label::DeclaredInput => Some("DeclaredInput"),
            Label::UncharacterizedInput => Some("UncharacterizedInput"),
            _ => None,
        })
        .collect()
}

/// One fixture case: its characteristic, value and decision, ready to format.
struct Case {
    name: String,
    ch: Characteristic,
    value: f64,
    rule: Option<String>,
    outcome: Outcome,
    acceptance: Option<LimitSet>,
    statement: String,
}

fn cases() -> Vec<Case> {
    let doc = json("fixtures/rpt/model/fmt/fmt_cases.json");
    assert_eq!(s(get(&doc, "schema")), "rpt-fmt-cases@1");
    arr(get(&doc, "cases"))
        .iter()
        .map(|c| {
            let rule = opt_s(get(c, "rule")).map(str::to_owned);
            let outcome = match s(get(c, "outcome")) {
                "NOT_DECIDABLE" => Outcome::NotDecidable(NotDecidableReason::RuleMissing),
                o => Outcome::from_wire(o, None).unwrap(),
            };
            let bits = u64::from_str_radix(s(get(c, "value_bits")).trim_start_matches("0x"), 16).unwrap();
            let decimals = match get(c, "decimals") {
                CValue::Null => None,
                v => Some(u8::try_from(int(v)).unwrap()),
            };
            Case {
                name: s(get(c, "name")).to_owned(),
                ch: Characteristic {
                    id: format!("ch-{}", s(get(c, "name"))),
                    kind: Some(kind_of(s(get(c, "kind")))),
                    unit: unit_of(s(get(c, "unit"))),
                    decimals,
                    limits: limit_set(get(c, "limits")),
                    nominal: None,
                },
                value: f64::from_bits(bits),
                rule,
                outcome,
                acceptance: limit_set(get(c, "acceptance")),
                statement: s(get(c, "statement")).to_owned(),
            }
        })
        .collect()
}

fn format_case(c: &Case, locale: LocaleRules) -> ValueText {
    let cat = Catalog::english();
    let st = stmt(&c.statement);
    let d = decision(c.rule.as_deref(), c.outcome, &st);
    let mut r = rec(&c.name, c.value, &d, &st);
    r.acceptance = c.acceptance;
    fmt_value(&r, &c.ch, &ctx(locale, &cat)).unwrap_or_else(|e| panic!("{}: {e}", c.name))
}

// ------------------------------------------------------------------ AC-1, AC-2, AC-4, AC-10

#[test]
fn ac1_fixture_cases_match_the_golden_file() {
    let golden = json("fixtures/rpt/model/fmt/fmt_cases.golden.json");
    let all = cases();
    assert_eq!(all.len(), 50);
    let mut units = BTreeSet::new();
    let mut authored = [false, false];
    for c in &all {
        let g = get(&golden, &c.name);
        let v = format_case(c, en());
        let t = v.text();
        assert_eq!(t.canonical, s(get(g, "canonical")), "{}", c.name);
        assert_eq!(t.display, s(get(g, "display_en")), "{}", c.name);
        assert_eq!(v.formatter().to_string(), s(get(g, "formatter")), "{}", c.name);
        assert_eq!(v.unit().symbol(), s(get(g, "unit")), "{}", c.name);
        assert_eq!(u64::from(v.digits_added()), int(get(g, "digits_added")), "{}", c.name);
        assert_eq!(v.side().name(), s(get(g, "side")), "{}", c.name);
        let tie = matches!(get(g, "tie"), CValue::Bool(true));
        assert_eq!(v.has_marker(Marker::Tie), tie, "{}", c.name);
        assert_eq!(v.exact_conversion().to, v.unit(), "{}", c.name);
        // Every value names its formatter ID and version (AC-1).
        assert!(v.formatter().major >= 1);
        units.insert(v.unit().symbol());
        authored[usize::from(c.ch.decimals.is_some())] = true;
        // AC-10: no case of AC-1..AC-4 carries the undecidable-side marker.
        assert!(!v.has_marker(Marker::UndecidableSide), "{}", c.name);
        assert_ne!(v.side(), Side::Undecidable, "{}", c.name);
    }
    for u in ["mm", "in", "um", "m", "rad", "deg", "dms", "1"] {
        assert!(units.contains(u), "fixture lacks unit {u}");
    }
    assert_eq!(authored, [true, true], "records with and without authored decimals");
}

#[test]
fn ac2_side_rule_never_prints_the_limit() {
    let c = cases().into_iter().find(|c| c.name == "ac2_side_rule").unwrap();
    assert_eq!(c.value, f64::from_bits(5e-5_f64.to_bits() + 1));
    let v = format_case(&c, en());
    assert_ne!(v.text().canonical, "0.050");
    assert!(v.digits_added() >= 1);
    assert!(v.markers().is_empty(), "no marker: {:?}", v.markers());
    assert!(v.warnings().is_empty());
    // AC-10: above, and the printed text is strictly greater than the limit.
    assert_eq!(v.side(), Side::Above);
    assert!(dec(v.text().canonical) > dec("0.050"));
}

#[test]
fn ac4_guard_band_uses_the_acceptance_limit() {
    let c = cases().into_iter().find(|c| c.name == "ac4_guard_band").unwrap();
    assert_eq!(c.acceptance.unwrap().upper, Some(dec("0.000048")));
    let v = format_case(&c, en());
    assert!(dec(v.text().canonical) > dec("0.048"), "{}", v.text().canonical);
    assert_eq!(v.side(), Side::Above);
    assert!(v.digits_added() >= 1);
    // Against the spec limit (0.050 mm) alone it would print 0.048 and look conforming.
    let mut spec_only = c;
    spec_only.rule = Some("dr:simple-acceptance@1".to_owned());
    spec_only.acceptance = None;
    let cat = Catalog::english();
    let st = stmt("uec1_lpu.json");
    let d = decision(spec_only.rule.as_deref(), Outcome::Nonconforms, &st);
    let r = rec("ac4", spec_only.value, &d, &st);
    assert_eq!(unformattable(fmt_value(&r, &spec_only.ch, &ctx(en(), &cat))), "nonconforms_not_beyond_limit");
}

#[test]
fn ac10_sides_of_conforming_values_and_requirements() {
    let all = cases();
    let conforming = all.iter().find(|c| c.name == "len_mm_auth3_conforms").unwrap();
    assert_eq!(format_case(conforming, en()).side(), Side::Within);
    let below = all.iter().find(|c| c.name == "len_mm_negative_below").unwrap();
    assert_eq!(format_case(below, en()).side(), Side::Below);

    let cat = Catalog::english();
    let mut req =
        ch("ch-req", ValueKind::Length, Unit::Mm, Some(3), Some(limits(None, Some("0.00005"), LimitUnit::Si)));
    req.nominal = Some(SiValue::new(0.025));
    let v = fmt_requirement(&req, &ctx(en(), &cat)).unwrap();
    assert_eq!(v.side(), Side::NotApplicable);
    assert_eq!(v.text().canonical, "25.000");
    assert!(v.labels().is_empty() && v.markers().is_empty());
    req.nominal = None;
    match fmt_requirement(&req, &ctx(en(), &cat)) {
        Err(FmtError::Unformattable { record, reason }) => {
            assert_eq!((record.as_str(), reason), ("ch-req", "no_nominal"))
        }
        other => panic!("{other:?}"),
    }
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_tie_from_a_stub_formatter_is_marked() {
    let cat = Catalog::english();
    let st = stmt("uec1_lpu.json");
    let d = decision(Some("dr:simple-acceptance@1"), Outcome::Nonconforms, &st);
    let r = rec("rec-tie", 5.1e-5, &d, &st);
    let c = ch("ch-tie", ValueKind::Length, Unit::Mm, Some(3), Some(limits(None, Some("0.00005"), LimitUnit::Si)));
    let stub = |_x: f64, spec: &FormatSpec| -> Result<Reported, fid_num::NumError> {
        Ok(Reported {
            value: "0.051".to_owned(),
            unit: spec.unit,
            exact_conversion: Conversion { from: Unit::M, to: Unit::Mm, factor: "1000", pi_enclosure: None },
            formatter: spec.formatter,
            digits_added: 0,
            tie_at_max_precision: true,
            side: SideOutcome::AboveUpper,
        })
    };
    let v = fmt_value_with(&r, &c, &ctx(en(), &cat), &stub).unwrap();
    assert_eq!(markers(&v), [Marker::Nonconformance, Marker::Tie]);
    assert_eq!(v.warnings().len(), 1);
    assert_eq!(v.warnings()[0].code, WARN_DISPLAY_TIE);
    assert_eq!(v.warnings()[0].record, "rec-tie");
    assert_eq!(v.side(), Side::Above);

    // The real kernel flags the 17-digit tie of the fixture the same way.
    let tie = cases().into_iter().find(|c| c.name == "scalar_tie_at_cap").unwrap();
    let v = format_case(&tie, en());
    assert_eq!(markers(&v), [Marker::Nonconformance, Marker::Tie]);
    assert_eq!(v.warnings()[0].code, WARN_DISPLAY_TIE);
}

/// A deliberately coarse ladder (pi in [3.141, 3.142]) on which `fmt.angle@1` cannot place
/// 1 rad (57.2958 deg) against an upper limit of 57.29 deg, but still rounds it to 57.3.
const COARSE: [PiEnclosure; 1] = [PiEnclosure { lo: "3.141", hi: "3.142", digits: 3 }];

fn undecidable_angle(outcome: Outcome) -> ValueText {
    let cat = Catalog::english();
    let st = stmt("uec1_lpu.json");
    let d = decision(Some("dr:simple-acceptance@1"), outcome, &st);
    let r = rec("rec-angle", 1.0, &d, &st);
    let c = ch("ch-angle", ValueKind::Angle, Unit::Deg, Some(1), Some(limits(None, Some("57.29"), LimitUnit::Deg)));
    let coarse = |x: f64, spec: &FormatSpec| format_with_ladder(x, spec, &COARSE);
    fmt_value_with(&r, &c, &ctx(en(), &cat), &coarse).unwrap()
}

#[test]
fn ac3_ac10_undecidable_side_is_shown_never_guessed() {
    let v = undecidable_angle(Outcome::Nonconforms);
    assert_eq!(v.text().canonical, "57.3");
    assert_eq!(v.formatter(), FormatterId::ANGLE_V1);
    assert_eq!(v.side(), Side::Undecidable);
    assert_eq!(markers(&v), [Marker::Nonconformance, Marker::UndecidableSide]);
    assert_eq!(v.warnings().len(), 1);
    assert_eq!(v.warnings()[0].code, WARN_DISPLAY_UNDECIDABLE);
    assert_eq!(v.warnings()[0].record, "rec-angle");
    assert_eq!(v.text().markers[1].text, "side of the limit undecidable");
    // Not NONCONFORMS: the side marker and warning stay, without the nonconformance marker.
    let v = undecidable_angle(Outcome::Indeterminate);
    assert_eq!(markers(&v), [Marker::UndecidableSide]);
    assert_eq!(v.side(), Side::Undecidable);
    // The full ladder decides it: above.
    let cat = Catalog::english();
    let st = stmt("uec1_lpu.json");
    let d = decision(Some("dr:simple-acceptance@1"), Outcome::Nonconforms, &st);
    let c = ch("ch-angle", ValueKind::Angle, Unit::Deg, Some(1), Some(limits(None, Some("57.29"), LimitUnit::Deg)));
    let v = fmt_value(&rec("rec-angle", 1.0, &d, &st), &c, &ctx(en(), &cat)).unwrap();
    assert_eq!(v.side(), Side::Above);
    assert!(v.markers().is_empty());
    assert!(dec(v.text().canonical) > dec("57.29"));
}

// ------------------------------------------------------------------ AC-5, AC-9

fn transcription() -> Transcription {
    let (source, rows) = load_rows(&read("fixtures/io/res/trans/model/rows_basic.json")).unwrap();
    let mut replay = ReplaySource::new(Vec::new());
    let existing: Vec<Uuid> = Vec::new();
    let ids = IdService::new(Uuid::parse("0b6f8c1e-3d2a-4f5b-9c7e-a1b2c3d4e5f6").unwrap(), &mut replay, &existing);
    transcribe(&rows, &source, &ids).unwrap()
}

#[test]
fn ac5_transcribed_gauge_and_uec0_labels() {
    let cat = Catalog::english();
    let t = transcription();
    let first = &t.results()[0];
    let st = first.uncertainty().clone();
    assert_eq!(st.class(), EvidenceClass::Uec0);
    let cat_rules = RuleCatalog::builtin().unwrap();
    let as_reported = cat_rules.resolve(&RuleRef::parse("dr:as-reported-by-source@1").unwrap()).unwrap();
    let d = Decision::new(
        Some(&as_reported),
        Outcome::NotDecidable(NotDecidableReason::LabelOnly),
        Some(EvidenceBasis::of(&st).unwrap()),
    )
    .unwrap();
    let mut r = rec(&first.id().to_string(), 0.010012, &d, &st);
    r.basis = EvaluationBasis::ResultsOnly;
    r.transcription = Some(t.record());
    let c = ch("C1", ValueKind::Length, Unit::Mm, Some(3), None);
    let v = fmt_value(&r, &c, &ctx(en(), &cat)).unwrap();
    assert_eq!(v.text().canonical, "10.012");
    let texts: Vec<&str> = v.labels().iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        texts,
        ["not decidable (label_only)", "dr:as-reported-by-source@1", "not evaluated", "transcribed: VendorX unknown"]
    );
    assert_eq!(label_set(&v)[3], Label::Transcribed { software: "VendorX".to_owned(), version: "unknown".to_owned() });
    // The UEC-0 text is the catalog's, never a number; a translation replaces it.
    let de_cat = Catalog::new(vec![("report.uec.not_evaluated".to_owned(), "nicht bewertet".to_owned())]).unwrap();
    let v = fmt_value(&r, &c, &ctx(de(), &de_cat)).unwrap();
    assert_eq!(v.labels()[2].text, "nicht bewertet");
    assert_eq!(v.text().display, "10,012");

    // DIRECT_READING citing gauge G-17.
    let st = stmt("uec1_spec_derived.json");
    let d = decision(Some("dr:simple-acceptance@1"), Outcome::Conforms, &st);
    let mut r = rec("rec-g17", 0.0251, &d, &st);
    r.basis = EvaluationBasis::DirectReading;
    r.gauge = GaugeId::new("G-17");
    let c = ch("C9", ValueKind::Length, Unit::Mm, Some(2), Some(limits(Some("0.025"), Some("0.026"), LimitUnit::Si)));
    let v = fmt_value(&r, &c, &ctx(en(), &cat)).unwrap();
    assert!(label_set(&v).contains(&Label::Gauge(GaugeId::new("G-17").unwrap())));
    assert!(v.labels().iter().any(|l| l.text == "gauge G-17"));
    assert_eq!(v.labels()[2].text, "UEC-1");
    // The label sources must match the basis.
    r.gauge = None;
    assert_eq!(unformattable(fmt_value(&r, &c, &ctx(en(), &cat))), "gauge_missing");
    r.basis = EvaluationBasis::ResultsOnly;
    assert_eq!(unformattable(fmt_value(&r, &c, &ctx(en(), &cat))), "transcription_missing");
    r.basis = EvaluationBasis::Points;
    r.gauge = GaugeId::new("G-17");
    assert_eq!(unformattable(fmt_value(&r, &c, &ctx(en(), &cat))), "gauge_unexpected");
    r.gauge = None;
    r.transcription = Some(t.record());
    assert_eq!(unformattable(fmt_value(&r, &c, &ctx(en(), &cat))), "transcription_unexpected");
    assert!(GaugeId::new("").is_none() && GaugeId::new(" G").is_none() && GaugeId::new("G\n1").is_none());
}

#[test]
fn ac9_each_value_keeps_its_own_claim_labels() {
    let cat = Catalog::english();
    let c = ch("C1", ValueKind::Length, Unit::Mm, Some(3), None);
    let expected: [(&str, &[&str]); 3] = [
        ("uec1_declared.json", &["Claimed", "DeclaredInput", "UncharacterizedInput"]),
        ("uec1_spec_derived.json", &["Claimed", "UncharacterizedInput"]),
        ("uec2_lpu.json", &[]),
    ];
    let stmts: Vec<UncertaintyStatement> = expected.iter().map(|(f, _)| stmt(f)).collect();
    let decisions: Vec<Decision> =
        stmts.iter().map(|st| decision(None, Outcome::NotDecidable(NotDecidableReason::RuleMissing), st)).collect();
    // One table: all three formatted together, each with its own labels.
    let table: Vec<ValueText> = stmts
        .iter()
        .zip(&decisions)
        .enumerate()
        .map(|(i, (st, d))| {
            fmt_value(&rec(&format!("row-{i}"), 0.001 * (i as f64 + 1.0), d, st), &c, &ctx(en(), &cat)).unwrap()
        })
        .collect();
    for ((file, want), v) in expected.iter().zip(&table) {
        let want: BTreeSet<&str> = want.iter().copied().collect();
        assert_eq!(claim_labels(v), want, "{file}");
    }
    // `labels` gives the same list as the formatted value.
    let r = rec("row-0", 0.001, &decisions[0], &stmts[0]);
    assert_eq!(labels(&r).unwrap(), label_set(&table[0]));
    // A decision that does not rest on the stored statement is refused.
    let r = rec("row-x", 0.001, &decisions[0], &stmts[1]);
    assert_eq!(unformattable(fmt_value(&r, &c, &ctx(en(), &cat))), "statement_mismatch");
}

// ------------------------------------------------------------------ AC-6

fn rules_of(id: &str, v: &CValue) -> LocaleRules {
    let one = |k: &str| -> char {
        let t = s(get(v, k));
        assert_eq!(t.chars().count(), 1);
        t.chars().next().unwrap()
    };
    LocaleRules::new(
        id,
        "test-1",
        one("decimal"),
        Some(one("group")),
        u8::try_from(int(get(v, "primary"))).unwrap(),
        u8::try_from(int(get(v, "secondary"))).unwrap(),
        s(get(v, "minus")),
    )
    .unwrap()
}

#[test]
fn ac6_locale_changes_only_separators_and_stays_injective() {
    let doc = json("fixtures/rpt/model/fmt/locale_corpus.json");
    let golden = json("fixtures/rpt/model/fmt/locale_corpus.golden.json");
    let corpus: Vec<&str> = arr(get(&doc, "canonical")).iter().map(s).collect();
    let CValue::Obj(locales) = get(&doc, "locales") else { panic!() };
    assert!(locales.contains_key("de") && locales.contains_key("ar"));
    for (id, v) in locales {
        let rules = rules_of(id, v);
        let want: Vec<&str> = arr(get(&golden, id)).iter().map(s).collect();
        let mut seen = BTreeSet::new();
        for (canonical, want) in corpus.iter().zip(&want) {
            let shown = rules.render(canonical).unwrap();
            assert_eq!(&shown, want, "{id}: {canonical}");
            // Byte-identical digits (and dms colons) once the locale's separators and minus
            // sign, and the canonical `.` and `-`, are stripped: nothing re-rounded, every
            // trailing zero kept.
            let strip = |t: &str, drop: &[char]| t.chars().filter(|c| !drop.contains(c)).collect::<String>();
            let mut seps: Vec<char> = s(get(v, "minus")).chars().collect();
            seps.extend(s(get(v, "decimal")).chars());
            seps.extend(s(get(v, "group")).chars());
            assert_eq!(strip(&shown, &seps).as_bytes(), strip(canonical, &['-', '.']).as_bytes(), "{id}: {canonical}");
            assert!(seen.insert(shown), "{id}: {canonical} renders like another string");
        }
        // No two distinct canonical strings of the corpus (distinct under kdec or not) render alike.
        assert_eq!(seen.len(), corpus.len());
    }
    // AC-1 values under `de` (golden display_de).
    let golden = json("fixtures/rpt/model/fmt/fmt_cases.golden.json");
    for c in cases() {
        let v = format_case(&c, de());
        assert_eq!(v.text().display, s(get(get(&golden, &c.name), "display_de")), "{}", c.name);
    }
}

#[test]
fn locale_rules_that_could_collide_are_refused() {
    let bad = |d: char, g: Option<char>, p: u8, q: u8, m: &str| LocaleRules::new("x", "r", d, g, p, q, m).unwrap_err();
    assert_eq!(bad('.', Some('.'), 3, 3, "-"), LocaleError::InvalidRules("group"));
    assert_eq!(bad('1', Some(','), 3, 3, "-"), LocaleError::InvalidRules("decimal"));
    assert_eq!(bad('.', Some(','), 0, 3, "-"), LocaleError::InvalidRules("grouping"));
    assert_eq!(bad('.', Some(','), 3, 3, ""), LocaleError::InvalidRules("minus"));
    assert_eq!(bad('.', Some(','), 3, 3, ","), LocaleError::InvalidRules("minus"));
    assert_eq!(bad('.', Some(','), 3, 3, "\u{2212}0"), LocaleError::InvalidRules("minus"));
    assert_eq!(bad('-', Some(','), 3, 3, "\u{2212}"), LocaleError::InvalidRules("decimal"));
    assert_eq!(LocaleRules::new("", "r", '.', None, 3, 3, "-").unwrap_err(), LocaleError::InvalidRules("id"));
    // Arabic-style: U+066B decimal, U+066C group, U+061C letter mark before the minus.
    let ar = LocaleRules::new("ar", "r", '\u{066b}', Some('\u{066c}'), 3, 3, "\u{061c}-").unwrap();
    assert_eq!(ar.render("-1234.50").unwrap(), "\u{061c}-1\u{066c}234\u{066b}50");
    // A Unicode minus and narrow no-break space grouping are fine.
    let fr = LocaleRules::new("fr", "r", ',', Some('\u{202f}'), 3, 3, "\u{2212}").unwrap();
    assert_eq!(fr.render("-1234567.50").unwrap(), "\u{2212}1\u{202f}234\u{202f}567,50");
    assert_eq!(fr.id(), "fr");
    assert_eq!(fr.release(), "r");
    let no_group = LocaleRules::new("xx", "r", '.', None, 3, 3, "-").unwrap();
    assert_eq!(no_group.render("1234567.0").unwrap(), "1234567.0");
    for not_canonical in ["", "-", "1.", ".5", "1e3", "+1", "1,5", "1.2.3", " 1", "1:2:3", "--1"] {
        assert_eq!(en().render(not_canonical), Err(LocaleError::NotCanonical), "{not_canonical:?}");
    }
}

// ------------------------------------------------------------------ AC-8 and other refusals

#[test]
fn ac8_non_finite_values_are_unformattable() {
    let cat = Catalog::english();
    let st = stmt("uec1_lpu.json");
    let d = decision(None, Outcome::NotDecidable(NotDecidableReason::RuleMissing), &st);
    let c = ch("C1", ValueKind::Length, Unit::Mm, Some(3), None);
    for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        match fmt_value(&rec("rec-nan", x, &d, &st), &c, &ctx(en(), &cat)) {
            Err(e @ FmtError::Unformattable { .. }) => {
                assert_eq!(e.code(), "report.unformattable");
                assert_eq!(e.record(), "rec-nan");
                assert_eq!(e.to_string(), "report.unformattable: rec-nan: non_finite");
            }
            other => panic!("{other:?}"),
        }
    }
    // A characteristic type without a formatter kind.
    let mut none = c.clone();
    none.kind = None;
    assert_eq!(unformattable(fmt_value(&rec("rec", 0.001, &d, &st), &none, &ctx(en(), &cat))), "no_formatter_kind");
}

#[test]
fn inputs_that_disagree_are_refused() {
    let cat = Catalog::english();
    let st = stmt("uec1_lpu.json");
    let x = ctx(en(), &cat);
    let lims = Some(limits(None, Some("0.00005"), LimitUnit::Si));

    // Guard-band decision without its acceptance limits.
    let gb = decision(Some("dr:guard-band(r=1)@1"), Outcome::Nonconforms, &st);
    let c = ch("C1", ValueKind::Length, Unit::Mm, Some(3), lims);
    assert_eq!(unformattable(fmt_value(&rec("r", 4.9e-5, &gb, &st), &c, &x)), "acceptance_limits_missing");
    // Acceptance limits under a rule without guard bands.
    let sa = decision(Some("dr:simple-acceptance@1"), Outcome::Nonconforms, &st);
    let mut r = rec("r", 5.1e-5, &sa, &st);
    r.acceptance = Some(limits(None, Some("0.000048"), LimitUnit::Si));
    assert_eq!(unformattable(fmt_value(&r, &c, &x)), "acceptance_limits_unexpected");
    // NONCONFORMS without any limit, or inside its limits, never prints as conforming.
    let open = ch("C2", ValueKind::Length, Unit::Mm, Some(3), None);
    assert_eq!(unformattable(fmt_value(&rec("r", 5.1e-5, &sa, &st), &open, &x)), "nonconforms_not_beyond_limit");
    assert_eq!(unformattable(fmt_value(&rec("r", 4.0e-5, &sa, &st), &c, &x)), "nonconforms_not_beyond_limit");
    // CONFORMS printed beyond its own limit: the inputs disagree, so no text.
    let ok = decision(Some("dr:simple-acceptance@1"), Outcome::Conforms, &st);
    assert_eq!(unformattable(fmt_value(&rec("r", 5.1e-5, &ok, &st), &c, &x)), "conforms_beyond_limit");
    let low = ch("C1", ValueKind::Length, Unit::Mm, Some(3), Some(limits(Some("0.00001"), None, LimitUnit::Si)));
    assert_eq!(unformattable(fmt_value(&rec("r", 0.9e-5, &ok, &st), &low, &x)), "conforms_beyond_limit");
    assert_eq!(fmt_value(&rec("r", 4.9e-5, &ok, &st), &c, &x).unwrap().side(), Side::Within);
    // SiValue shows bits, never decimal text.
    assert_eq!(format!("{:?}", SiValue::new(1.0)), "SiValue(0x3ff0000000000000)");
    // Units of another kind, and angle limits in the wrong unit.
    let nd = decision(None, Outcome::NotDecidable(NotDecidableReason::RuleMissing), &st);
    let wrong = ch("C3", ValueKind::Length, Unit::Deg, Some(3), None);
    assert_eq!(unformattable(fmt_value(&rec("r", 1.0, &nd, &st), &wrong, &x)), "unit_kind_mismatch");
    let rad_with_deg = ch("C4", ValueKind::Angle, Unit::Rad, Some(3), Some(limits(None, Some("10"), LimitUnit::Deg)));
    assert_eq!(unformattable(fmt_value(&rec("r", 0.1, &nd, &st), &rad_with_deg, &x)), "limit_unit_mismatch");
    // No authored decimals and no default for the unit (m has none initially).
    let m = ch("C5", ValueKind::Length, Unit::M, None, None);
    assert_eq!(unformattable(fmt_value(&rec("r", 1.0, &nd, &st), &m, &x)), "no_default_precision");
    let mut with_m = ctx(en(), &cat);
    with_m.default_decimals = DefaultDecimals::initial().set(Unit::M, 2).unwrap();
    assert_eq!(fmt_value(&rec("r", 1.0, &nd, &st), &m, &with_m).unwrap().text().canonical, "1.00");
    assert!(DefaultDecimals::initial().set(Unit::M, 21).is_none());
    // A kernel refusal is wrapped, never swallowed (precision above 20 decimals).
    let fine = ch("C6", ValueKind::Length, Unit::Mm, Some(21), None);
    match fmt_value(&rec("r", 1.0, &nd, &st), &fine, &x) {
        Err(e @ FmtError::Formatter { .. }) => assert_eq!(e.code(), "report.formatter_failed"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn unit_override_uses_the_default_precision_and_the_exact_conversion() {
    let cat = Catalog::english();
    let st = stmt("uec1_lpu.json");
    let nd = decision(None, Outcome::NotDecidable(NotDecidableReason::RuleMissing), &st);
    let c = ch("C1", ValueKind::Length, Unit::Mm, Some(3), None);
    let mut x = ctx(en(), &cat);
    x.units = UnitOverride::new(Some(Unit::In), None).unwrap();
    let v = fmt_value(&rec("r", 0.0254, &nd, &st), &c, &x).unwrap();
    // Authored decimals belong to mm; inches use report.default_decimals (4).
    assert_eq!(v.text().canonical, "1.0000");
    assert_eq!(v.unit(), Unit::In);
    let conv = v.exact_conversion();
    assert_eq!((conv.from, conv.to, conv.factor), (Unit::M, Unit::In, "1/0.0254"));
    assert!(UnitOverride::new(Some(Unit::Deg), None).is_none());
    assert!(UnitOverride::new(None, Some(Unit::Mm)).is_none());
}

#[test]
fn outcome_text_and_catalog() {
    let cat = Catalog::english();
    let st = stmt("uec1_lpu.json");
    let d = decision(Some("dr:guard-band(r=1)@1"), Outcome::Nonconforms, &st);
    let o = fmt_outcome(&d, &cat);
    assert_eq!(o.outcome, Outcome::Nonconforms);
    assert_eq!(o.text, "nonconforms");
    assert_eq!(o.rule_text.as_deref(), Some("dr:guard-band(r=1)@1"));
    let nd = decision(None, Outcome::NotDecidable(NotDecidableReason::RuleMissing), &st);
    let o = fmt_outcome(&nd, &cat);
    assert_eq!(o.text, "not decidable (rule_missing)");
    assert!(o.rule.is_none());
    assert!(matches!(Catalog::new(vec![("report.nope".into(), "x".into())]), Err(CatalogError::UnknownKey(_))));
    assert!(matches!(
        Catalog::new(vec![("report.label.claimed".into(), "a".into()), ("report.label.claimed".into(), "b".into())]),
        Err(CatalogError::DuplicateKey(_))
    ));
    assert!(matches!(
        Catalog::new(vec![("report.label.claimed".into(), String::new())]),
        Err(CatalogError::InvalidText(_))
    ));
    assert_eq!(cat.text("report.label.claimed"), "claimed");
    assert_eq!(cat.text("report.unknown"), "report.unknown");
}

// ------------------------------------------------------------------ AC-11

#[test]
fn ac11_the_only_text_accessor_carries_labels_and_markers() {
    let v = undecidable_angle(Outcome::Nonconforms);
    let t = v.text();
    assert_eq!(t.display, "57.3");
    assert_eq!(t.canonical, "57.3");
    assert_eq!(t.unit, Unit::Deg);
    assert_eq!(t.markers.len(), 2);
    assert_eq!(t.labels, v.labels());
    assert!(!t.labels.is_empty());
    // The compile-fail half is the set of `compile_fail` doctests on `ValueText`
    // (crates/fid-report/src/fmt/model.rs), run by `cargo test`.
}

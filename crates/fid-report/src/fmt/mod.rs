//! Value formatting, decision display and source labels. Spec node `RPT.MODEL.FMT`.
//!
//! The single place in RPT where a value of record becomes text. It holds no float-to-text
//! code of its own: every numeric string is the `Reported.value` of a kernel formatter call
//! through [`fid_num::fmt::format`] (`fmt.length@1`, `fmt.angle@1`, `fmt.scalar@1`). Outside
//! this module (and the JSON-number boundary `json_num`) no `fid-report*` code may name a
//! float type (clippy `disallowed_types`, `crates/fid-report*/clippy.toml`) or hold a float
//! literal; values travel as [`SiValue`], which has no `Display`. `tools/rpt_float_lint.py`
//! adds a token scan for float formatting and proves the clippy rule in CI
//! (REQ-RPT.MODEL.FMT-002; see the leaf's Open questions for what neither layer sees).
//!
//! # [`fmt_value`]
//!
//! Checks, in this order (each failure is `report.unformattable` naming the record, with the
//! reason in parentheses, and no text):
//!
//! 1. the characteristic's type has a formatter kind (`no_formatter_kind`);
//! 2. the value is finite (`non_finite`);
//! 3. the decision's evidence basis, when it has one, names the stored statement
//!    (`statement_mismatch`);
//! 4. a `RESULTS_ONLY` value has its transcription record and only it has one
//!    (`transcription_missing`, `transcription_unexpected`); a `DIRECT_READING` value cites
//!    a gauge and only it does (`gauge_missing`, `gauge_unexpected`);
//! 5. the authored unit and any override are units of the kind (`unit_kind_mismatch`);
//! 6. the precision: the authored decimals when the value is shown in the authored unit,
//!    else `report.default_decimals` for the display unit (`no_default_precision`);
//! 7. the limits the decision was evaluated against: the acceptance limits when the rule is
//!    a guard band (required for `CONFORMS` and `NONCONFORMS`: `acceptance_limits_missing`;
//!    with another outcome the specification limits are used when they are absent), else the
//!    specification limits (acceptance limits with any other rule:
//!    `acceptance_limits_unexpected`); their unit must be the one the formatter takes for
//!    the display unit (`limit_unit_mismatch`).
//!
//! It then calls the formatter with the display unit, `Decimals(p)` and those limits, so the
//! kernel's side-preserving rule applies: a `NONCONFORMS` value is never rounded to look
//! conforming (#200). The kernel's error is wrapped as `report.formatter_failed`. The result
//! must come from the formatter asked for, in the unit asked for (`formatter_mismatch`).
//! The [`Side`] is the formatter's; a `NONCONFORMS` value whose side is not `above`,
//! `below` or `undecidable` is refused (`nonconforms_not_beyond_limit`), so it can never
//! print inside its limits; a `CONFORMS` value whose side is `above` or `below` is refused
//! too (`conforms_beyond_limit`): the inputs disagree and no text could be honest.
//!
//! Markers: [`Marker::Tie`] with warning `report.display_tie` when the formatter set
//! `tie_at_max_precision`; [`Marker::UndecidableSide`] with warning
//! `report.display_undecidable` exactly when the side is undecidable; and, for a
//! `NONCONFORMS` value with either, [`Marker::Nonconformance`] first (REQ-004).
//!
//! The display string is the canonical string rendered with the pinned locale rules
//! ([`LocaleRules::render`]): separators and minus sign only, never re-rounded.
//!
//! Labels ([`labels`]), in this order: `ProvisionalInterpretation` when the record's GD&T
//! interpretation is provisional ([`InterpretationStatus`], read only from the stored
//! record; D-045, REQ-015; also on the [`OutcomeText`]), the outcome, the rule (if any), the evidence class
//! (UEC-0 as the catalog text "not evaluated", never a number), `transcribed` with source
//! software and version (`RESULTS_ONLY`) or the gauge (`DIRECT_READING`), then `Claimed`,
//! `DeclaredInput` and `UncharacterizedInput` exactly when the stored statement's
//! `is_claimed()`, `rests_on_declared_input()` and `rests_on_uncharacterized_input()` are
//! true (REQ-012). A claimed or transcribed value therefore never looks measured.

// The one module of fid-report (with the JSON boundary `json_num`) where a float may exist;
// see crates/fid-report/clippy.toml.
#![allow(clippy::disallowed_types)]

mod catalog;
mod locale;
mod model;

pub use catalog::{Catalog, CatalogError, MAX_TEXT_BYTES, MESSAGES};
pub use locale::{LocaleError, LocaleRules, MAX_LOCALE_ID_BYTES, MAX_MINUS_CHARS};
pub use model::{
    Characteristic, DefaultDecimals, FmtContext, FmtError, GaugeId, InterpretationStatus, Label, LabelText, LimitSet,
    LimitUnit, Marker, MarkerText, OutcomeText, Shown, SiValue, Side, UnitOverride, ValueKind, ValueRecord, ValueText,
    Warning, PROVISIONAL_TEXT, WARN_DISPLAY_TIE, WARN_DISPLAY_UNDECIDABLE,
};

use fid_gdt::chars::record::EvaluationBasis;
use fid_kdec::{Decision, Outcome, RuleId};
use fid_num::err::NumError;
use fid_num::fmt::{self as kfmt, FormatSpec, Limits, Precision, Reported, SideOutcome, Unit};

/// A kernel formatter call: `fid_num::fmt::format` in every shipped build.
type Formatter<'f> = &'f dyn Fn(f64, &FormatSpec) -> Result<Reported, NumError>;

fn unformattable(record: &str, reason: &'static str) -> FmtError {
    FmtError::Unformattable { record: record.to_owned(), reason }
}

/// Formats a value of record (see the module documentation).
pub fn fmt_value(record: &ValueRecord<'_>, ch: &Characteristic, ctx: &FmtContext<'_>) -> Result<ValueText, FmtError> {
    fmt_value_inner(record, ch, ctx, &kfmt::format)
}

/// Test hook: [`fmt_value`] with another formatter (for example a stub that reports a tie,
/// or `fmt::ang::format_with_ladder` with a coarse ladder). Only in builds with the
/// `test-formatter` feature, which shipped builds never enable.
#[cfg(feature = "test-formatter")]
#[doc(hidden)]
pub fn fmt_value_with(
    record: &ValueRecord<'_>,
    ch: &Characteristic,
    ctx: &FmtContext<'_>,
    formatter: &dyn Fn(f64, &FormatSpec) -> Result<Reported, NumError>,
) -> Result<ValueText, FmtError> {
    fmt_value_inner(record, ch, ctx, formatter)
}

fn fmt_value_inner(
    record: &ValueRecord<'_>,
    ch: &Characteristic,
    ctx: &FmtContext<'_>,
    formatter: Formatter<'_>,
) -> Result<ValueText, FmtError> {
    let rid = record.id.as_str();
    let kind = ch.kind.ok_or_else(|| unformattable(rid, "no_formatter_kind"))?;
    let value = record.value.get();
    if !value.is_finite() {
        return Err(unformattable(rid, "non_finite"));
    }
    let labels = labels(record)?;
    let unit = display_unit(rid, kind, ch, ctx)?;
    let decimals = precision(rid, ch, unit, ctx)?;
    let outcome = record.decision.outcome();
    let limits = decision_limits(record, ch)?.map(|l| to_kernel(rid, kind, unit, l)).transpose()?;
    let spec = FormatSpec { formatter: kind.formatter(), unit, precision: Precision::Decimals(decimals), limits };
    let rep = call(rid, formatter, value, &spec)?;
    let side = side_of(rep.side);
    if outcome == Outcome::Nonconforms && !matches!(side, Side::Above | Side::Below | Side::Undecidable) {
        return Err(unformattable(rid, "nonconforms_not_beyond_limit"));
    }
    if outcome == Outcome::Conforms && matches!(side, Side::Above | Side::Below) {
        return Err(unformattable(rid, "conforms_beyond_limit"));
    }
    let mut markers = Vec::new();
    let mut warnings = Vec::new();
    let flagged = rep.tie_at_max_precision || side == Side::Undecidable;
    if outcome == Outcome::Nonconforms && flagged {
        markers.push(Marker::Nonconformance);
    }
    if rep.tie_at_max_precision {
        markers.push(Marker::Tie);
        warnings.push(Warning { code: WARN_DISPLAY_TIE, record: rid.to_owned() });
    }
    if side == Side::Undecidable {
        markers.push(Marker::UndecidableSide);
        warnings.push(Warning { code: WARN_DISPLAY_UNDECIDABLE, record: rid.to_owned() });
    }
    finish(rid, rep, side, markers, labels, warnings, ctx)
}

/// Formats a characteristic's nominal value (a requirement): no limits, so the side is
/// `not_applicable`; no labels. Fails with `report.unformattable` naming the characteristic
/// when it has no formatter kind or no finite nominal value (`no_nominal`, `non_finite`).
pub fn fmt_requirement(ch: &Characteristic, ctx: &FmtContext<'_>) -> Result<ValueText, FmtError> {
    let cid = ch.id.as_str();
    let kind = ch.kind.ok_or_else(|| unformattable(cid, "no_formatter_kind"))?;
    let nominal = ch.nominal.ok_or_else(|| unformattable(cid, "no_nominal"))?.get();
    if !nominal.is_finite() {
        return Err(unformattable(cid, "non_finite"));
    }
    let unit = display_unit(cid, kind, ch, ctx)?;
    let decimals = precision(cid, ch, unit, ctx)?;
    let spec = FormatSpec { formatter: kind.formatter(), unit, precision: Precision::Decimals(decimals), limits: None };
    let rep = call(cid, &kfmt::format, nominal, &spec)?;
    let side = side_of(rep.side);
    finish(cid, rep, side, Vec::new(), Vec::new(), Vec::new(), ctx)
}

/// The decision outcome and rule ID as text (REQ-RPT.MODEL.FMT-007), with the provisional
/// label when the record's interpretation is provisional (REQ-RPT.MODEL.FMT-015).
pub fn fmt_outcome(decision: &Decision, interpretation: InterpretationStatus, catalog: &Catalog) -> OutcomeText {
    let outcome = decision.outcome();
    let rule = decision.rule().cloned();
    let labels = if interpretation.is_provisional() {
        let l = Label::ProvisionalInterpretation;
        vec![LabelText { text: l.text(catalog), label: l }]
    } else {
        Vec::new()
    };
    OutcomeText {
        outcome,
        text: model::Label::Outcome(outcome).text(catalog),
        rule_text: rule.as_ref().map(ToString::to_string),
        rule,
        labels,
    }
}

/// The labels of a value of record, in display order (see the module documentation).
/// Fails when the evaluation basis and its label source disagree or the decision does not
/// rest on the stored statement.
pub fn labels(record: &ValueRecord<'_>) -> Result<Vec<Label>, FmtError> {
    let rid = record.id.as_str();
    let stmt = record.statement;
    if record.decision.basis().is_some_and(|b| b.statement() != stmt.digest()) {
        return Err(unformattable(rid, "statement_mismatch"));
    }
    let mut out = Vec::new();
    if record.interpretation.is_provisional() {
        out.push(Label::ProvisionalInterpretation);
    }
    out.push(Label::Outcome(record.decision.outcome()));
    if let Some(r) = record.decision.rule() {
        out.push(Label::Rule(r.clone()));
    }
    out.push(Label::Uec(stmt.class()));
    match (record.basis, record.transcription) {
        (EvaluationBasis::ResultsOnly, Some(t)) => out.push(Label::Transcribed {
            software: t.source_software().escaped(),
            version: t.source_version().escaped(),
        }),
        (EvaluationBasis::ResultsOnly, None) => return Err(unformattable(rid, "transcription_missing")),
        (_, Some(_)) => return Err(unformattable(rid, "transcription_unexpected")),
        (_, None) => {}
    }
    match (record.basis, &record.gauge) {
        (EvaluationBasis::DirectReading, Some(g)) => out.push(Label::Gauge(g.clone())),
        (EvaluationBasis::DirectReading, None) => return Err(unformattable(rid, "gauge_missing")),
        (_, Some(_)) => return Err(unformattable(rid, "gauge_unexpected")),
        (_, None) => {}
    }
    if stmt.is_claimed() {
        out.push(Label::Claimed);
    }
    if stmt.rests_on_declared_input() {
        out.push(Label::DeclaredInput);
    }
    if stmt.rests_on_uncharacterized_input() {
        out.push(Label::UncharacterizedInput);
    }
    Ok(out)
}

fn display_unit(id: &str, kind: ValueKind, ch: &Characteristic, ctx: &FmtContext<'_>) -> Result<Unit, FmtError> {
    if !kind.accepts(ch.unit) {
        return Err(unformattable(id, "unit_kind_mismatch"));
    }
    let unit = ctx.units.for_kind(kind).unwrap_or(ch.unit);
    if !kind.accepts(unit) {
        return Err(unformattable(id, "unit_kind_mismatch"));
    }
    Ok(unit)
}

fn precision(id: &str, ch: &Characteristic, unit: Unit, ctx: &FmtContext<'_>) -> Result<u8, FmtError> {
    match ch.decimals {
        Some(d) if unit == ch.unit => Ok(d),
        _ => ctx.default_decimals.get(unit).ok_or_else(|| unformattable(id, "no_default_precision")),
    }
}

fn decision_limits(record: &ValueRecord<'_>, ch: &Characteristic) -> Result<Option<LimitSet>, FmtError> {
    let rid = record.id.as_str();
    let guard_band = record.decision.rule().is_some_and(|r| matches!(r.id, RuleId::GuardBand { .. }));
    let decided = matches!(record.decision.outcome(), Outcome::Conforms | Outcome::Nonconforms);
    match (guard_band, record.acceptance) {
        (true, Some(a)) => Ok(Some(a)),
        (true, None) if decided => Err(unformattable(rid, "acceptance_limits_missing")),
        (true, None) => Ok(ch.limits),
        (false, Some(_)) => Err(unformattable(rid, "acceptance_limits_unexpected")),
        (false, None) => Ok(ch.limits),
    }
}

fn to_kernel(id: &str, kind: ValueKind, unit: Unit, l: LimitSet) -> Result<Limits, FmtError> {
    let want = match (kind, unit) {
        (ValueKind::Angle, Unit::Deg | Unit::Dms) => LimitUnit::Deg,
        _ => LimitUnit::Si,
    };
    if l.unit != want {
        return Err(unformattable(id, "limit_unit_mismatch"));
    }
    Ok(Limits { lower: l.lower, upper: l.upper })
}

fn call(id: &str, formatter: Formatter<'_>, x: f64, spec: &FormatSpec) -> Result<Reported, FmtError> {
    let rep = formatter(x, spec).map_err(|error| FmtError::Formatter { record: id.to_owned(), error })?;
    if rep.formatter != spec.formatter || rep.unit != spec.unit {
        return Err(unformattable(id, "formatter_mismatch"));
    }
    Ok(rep)
}

fn side_of(s: SideOutcome) -> Side {
    match s {
        SideOutcome::NoLimits => Side::NotApplicable,
        SideOutcome::Within => Side::Within,
        SideOutcome::AboveUpper => Side::Above,
        SideOutcome::BelowLower => Side::Below,
        SideOutcome::Undecidable => Side::Undecidable,
    }
}

fn finish(
    id: &str,
    rep: Reported,
    side: Side,
    markers: Vec<Marker>,
    labels: Vec<Label>,
    warnings: Vec<Warning>,
    ctx: &FmtContext<'_>,
) -> Result<ValueText, FmtError> {
    let display = ctx.locale.render(&rep.value).map_err(|_| unformattable(id, "canonical_malformed"))?;
    let cat = ctx.catalog;
    Ok(ValueText {
        record: id.to_owned(),
        canonical: rep.value,
        display,
        unit: rep.unit,
        formatter: rep.formatter,
        exact_conversion: rep.exact_conversion,
        digits_added: rep.digits_added,
        side,
        markers: markers.into_iter().map(|m| MarkerText { marker: m, text: cat.text(m.key()).to_owned() }).collect(),
        labels: labels.into_iter().map(|l| LabelText { text: l.text(cat), label: l }).collect(),
        warnings,
    })
}

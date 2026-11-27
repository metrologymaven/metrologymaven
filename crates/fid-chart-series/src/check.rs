//! Validation (REQ-001 to REQ-004) and the consumer's version check (REQ-005).

use crate::{
    echo_kind, AxisKind, ChartSeries, ChartSeriesError, InvalidReason as R, Marker, Point, Quantity, RefLine,
    ReviewStatus, SeriesKind, SeriesStatus, UnsupportedCause, UnverifiedRef, CONTRACT_MAJOR, MAX_ALGORITHM_BYTES,
    MAX_CONSTANTS, MAX_CONSTANTS_REF_BYTES, MAX_COUNT, MAX_EXCLUDED, MAX_ID_BYTES, MAX_LABEL_CHARS,
    MAX_LABEL_KEY_BYTES, MAX_MARKERS, MAX_POINTS, MAX_REF_LINES, MAX_SET_VERSION, MAX_UNVERIFIED, MAX_VERSION_PART,
};
use std::cmp::Ordering;
use std::collections::BTreeMap;

type Res = Result<(), ChartSeriesError>;

fn bad(field: impl Into<String>, reason: R) -> Res {
    Err(ChartSeriesError::invalid(field, reason))
}

/// Checks every rule of the contract (crate docs, "Rules beyond the field formats").
///
/// A major other than [`CONTRACT_MAJOR`] is `report.chart_series_unsupported`; everything
/// else is `report.chart_series_invalid` naming the first offending field.
pub fn validate(series: &ChartSeries) -> Res {
    let c = series.contract;
    if c.major != CONTRACT_MAJOR {
        return Err(ChartSeriesError::Unsupported {
            major: c.major,
            minor: c.minor,
            kind: echo_kind(series.kind.as_str()),
            cause: UnsupportedCause::Major,
        });
    }
    if c.minor > MAX_VERSION_PART {
        return bad("contract.minor", R::OutOfRange);
    }
    axis(&series.x, "x")?;
    axis(&series.y, "y")?;
    let histogram = series.kind == SeriesKind::Histogram;
    if histogram {
        if series.x.kind == AxisKind::Ordinal {
            return bad("x.kind", R::AxisKind);
        }
        if series.y.kind != AxisKind::Ordinal {
            return bad("y.kind", R::AxisKind);
        }
    }
    if series.points.len() > MAX_POINTS {
        return bad("points", R::TooMany);
    }
    for (i, p) in series.points.iter().enumerate() {
        point(p, &format!("points[{i}]"), series)?;
    }
    let insufficient = series.status == SeriesStatus::InsufficientData;
    if insufficient && !series.kind.is_control() && !series.points.is_empty() {
        return bad("points", R::StatusConflict);
    }
    if !insufficient && series.points.is_empty() {
        return bad("status", R::StatusConflict);
    }
    let value_axis = if histogram { series.x.kind } else { series.y.kind };
    ref_lines(&series.ref_lines, value_axis, series)?;
    provenance(series)?;
    reviews_against_unverified(series)
}

/// Whether a consumer that reads contract major `supported_major` and draws the kinds
/// `supported_kinds` can take `series` (REQ-005). An empty `supported_kinds` supports nothing.
pub fn check_supported(series: &ChartSeries, supported_major: u32, supported_kinds: &[SeriesKind]) -> Res {
    let c = series.contract;
    let cause = if c.major != supported_major {
        UnsupportedCause::Major
    } else if !supported_kinds.contains(&series.kind) {
        UnsupportedCause::Kind
    } else {
        return Ok(());
    };
    Err(ChartSeriesError::Unsupported { major: c.major, minor: c.minor, kind: echo_kind(series.kind.as_str()), cause })
}

fn axis(a: &crate::Axis, at: &str) -> Res {
    if !label_key_ok(&a.label_key) {
        return bad(format!("{at}.label_key"), R::BadFormat);
    }
    if let AxisKind::Quantity { quantity, unit } = a.kind {
        if Quantity::of(unit) != quantity {
            return bad(format!("{at}.unit"), R::UnitMismatch);
        }
    }
    Ok(())
}

fn finite(v: f64, at: impl Into<String>) -> Res {
    if v.is_finite() {
        Ok(())
    } else {
        bad(at, R::NonFinite)
    }
}

fn point(p: &Point, at: &str, series: &ChartSeries) -> Res {
    let histogram = series.kind == SeriesKind::Histogram;
    finite(p.x, format!("{at}.x"))?;
    finite(p.y, format!("{at}.y"))?;
    if let Some(u) = p.x_upper {
        finite(u, format!("{at}.x_upper"))?;
    }
    if let Some(l) = &p.x_label {
        if !x_label_ok(l) {
            return bad(format!("{at}.x_label"), R::BadFormat);
        }
    }
    if p.record.as_deref().is_some_and(|s| !id_ok(s)) {
        return bad(format!("{at}.record"), R::BadFormat);
    }
    if p.piece.as_deref().is_some_and(|s| !id_ok(s)) {
        return bad(format!("{at}.piece"), R::BadFormat);
    }
    markers(&p.markers, at)?;
    for (j, m) in p.markers.iter().enumerate() {
        if matches!(m, Marker::RuleViolation { .. }) {
            if !series.kind.is_control() {
                return bad(format!("{at}.markers[{j}]"), R::NotApplicable);
            }
            if series.status == SeriesStatus::InsufficientData {
                return bad(format!("{at}.markers[{j}]"), R::StatusConflict);
            }
        }
    }
    if !histogram {
        if p.x_upper.is_some() {
            return bad(format!("{at}.x_upper"), R::NotApplicable);
        }
        return Ok(());
    }
    // A count: integral, from +0 to 2^53, never -0.0 (one spelling per count).
    let count_ok = p.y.is_sign_positive() && p.y.trunc() == p.y && p.y <= MAX_COUNT;
    if !count_ok {
        return bad(format!("{at}.y"), R::BadCount);
    }
    if p.x_upper.is_some_and(|u| u.total_cmp(&p.x) == Ordering::Less) {
        return bad(format!("{at}.x_upper"), R::UpperBelowLower);
    }
    Ok(())
}

fn markers(ms: &[Marker], at: &str) -> Res {
    if ms.len() > MAX_MARKERS {
        return bad(format!("{at}.markers"), R::TooMany);
    }
    for (j, m) in ms.iter().enumerate() {
        if let Marker::RuleViolation { set_id, set_version, rule_id, .. } = m {
            let mat = format!("{at}.markers[{j}]");
            if !set_id_ok(set_id) {
                return bad(format!("{mat}.set_id"), R::BadFormat);
            }
            if !(1..=MAX_SET_VERSION).contains(set_version) {
                return bad(format!("{mat}.set_version"), R::OutOfRange);
            }
            if !rule_id_ok(rule_id) {
                return bad(format!("{mat}.rule_id"), R::BadFormat);
            }
        }
    }
    for (j, w) in ms.windows(2).enumerate() {
        if w[0].canonical_cmp(&w[1]) != Ordering::Less {
            return bad(format!("{at}.markers[{}]", j + 1), R::NotCanonicalOrder);
        }
    }
    let oot = ms.contains(&Marker::OutOfTolerance);
    if oot && ms.contains(&Marker::Indeterminate) {
        return bad(format!("{at}.markers"), R::ConflictingMarkers);
    }
    Ok(())
}

fn ref_lines(rs: &[RefLine], value_axis: AxisKind, series: &ChartSeries) -> Res {
    if rs.len() > MAX_REF_LINES {
        return bad("ref_lines", R::TooMany);
    }
    for (i, r) in rs.iter().enumerate() {
        finite(r.value, format!("ref_lines[{i}].value"))?;
        if r.value.to_bits() == (-0.0f64).to_bits() {
            return bad(format!("ref_lines[{i}].value"), R::NegativeZero);
        }
        match value_axis {
            AxisKind::Ordinal => return bad(format!("ref_lines[{i}].unit"), R::NoValueUnit),
            AxisKind::Quantity { unit, .. } if unit != r.unit => {
                return bad(format!("ref_lines[{i}].unit"), R::UnitMismatch)
            }
            AxisKind::Quantity { .. } => {}
        }
        if r.kind.is_limit() {
            if !series.kind.is_control() {
                return bad(format!("ref_lines[{i}].kind"), R::NotApplicable);
            }
            if series.status == SeriesStatus::InsufficientData {
                return bad(format!("ref_lines[{i}].kind"), R::StatusConflict);
            }
        }
    }
    for (i, w) in rs.windows(2).enumerate() {
        // Finite and never -0.0 here, so IEEE equality and totalOrder agree.
        if w[0].kind == w[1].kind && w[0].value.total_cmp(&w[1].value) == Ordering::Equal {
            return bad(format!("ref_lines[{}]", i + 1), R::Duplicate);
        }
        let order = w[0].kind.cmp(&w[1].kind).then_with(|| w[0].value.total_cmp(&w[1].value));
        if order != Ordering::Less {
            return bad(format!("ref_lines[{}]", i + 1), R::NotCanonicalOrder);
        }
    }
    Ok(())
}

fn provenance(series: &ChartSeries) -> Res {
    let p = &series.provenance;
    if p.dataset_digest.is_tombstone() {
        return bad("provenance.dataset_digest", R::Tombstone);
    }
    if !algorithm_ok(&p.algorithm) {
        return bad("provenance.algorithm", R::BadFormat);
    }
    if !semver_ok(&p.kernel_version) {
        return bad("provenance.kernel_version", R::BadFormat);
    }
    if let Some(f) = &p.frozen_limits {
        if !id_ok(f) {
            return bad("provenance.frozen_limits", R::BadFormat);
        }
        if !series.kind.is_control() {
            return bad("provenance.frozen_limits", R::NotApplicable);
        }
    }
    match &p.binning_rule {
        Some(b) if !binning_rule_ok(b) => return bad("provenance.binning_rule", R::BadFormat),
        Some(_) if series.kind != SeriesKind::Histogram => return bad("provenance.binning_rule", R::NotApplicable),
        None if series.kind == SeriesKind::Histogram => return bad("provenance.binning_rule", R::RequiredForKind),
        _ => {}
    }
    constants(series)?;
    unverified(series)?;
    excluded(series)
}

fn constants(series: &ChartSeries) -> Res {
    let cs = &series.provenance.constants;
    if cs.len() > MAX_CONSTANTS {
        return bad("provenance.constants", R::TooMany);
    }
    for (k, c) in cs.iter().enumerate() {
        if !constants_ref_ok(c) {
            return bad(format!("provenance.constants[{k}]"), R::BadFormat);
        }
    }
    for (k, w) in cs.windows(2).enumerate() {
        if w[0].as_bytes() >= w[1].as_bytes() {
            return bad(format!("provenance.constants[{}]", k + 1), R::NotCanonicalOrder);
        }
    }
    match (series.kind.is_control(), cs.is_empty()) {
        (true, true) => bad("provenance.constants", R::RequiredForKind),
        (false, false) => bad("provenance.constants", R::NotApplicable),
        _ => Ok(()),
    }
}

fn unverified(series: &ChartSeries) -> Res {
    let us = &series.provenance.unverified;
    if us.len() > MAX_UNVERIFIED {
        return bad("provenance.unverified", R::TooMany);
    }
    for (k, u) in us.iter().enumerate() {
        let at = format!("provenance.unverified[{k}]");
        match u {
            UnverifiedRef::Constants(r) if !constants_ref_ok(r) => return bad(format!("{at}.ref"), R::BadFormat),
            UnverifiedRef::RuleSet { set_id, .. } if !set_id_ok(set_id) => {
                return bad(format!("{at}.set_id"), R::BadFormat)
            }
            UnverifiedRef::RuleSet { set_version, .. } if !(1..=MAX_SET_VERSION).contains(set_version) => {
                return bad(format!("{at}.set_version"), R::OutOfRange)
            }
            _ => {}
        }
    }
    for (k, w) in us.windows(2).enumerate() {
        if w[0].canonical_cmp(&w[1]) != Ordering::Less {
            return bad(format!("provenance.unverified[{}]", k + 1), R::NotCanonicalOrder);
        }
    }
    for (k, u) in us.iter().enumerate() {
        match u {
            UnverifiedRef::Constants(r) if !series.provenance.constants.contains(r) => {
                return bad(format!("provenance.unverified[{k}]"), R::NotReferenced)
            }
            UnverifiedRef::RuleSet { .. } if !series.kind.is_control() => {
                return bad(format!("provenance.unverified[{k}]"), R::NotApplicable)
            }
            _ => {}
        }
    }
    Ok(())
}

fn excluded(series: &ChartSeries) -> Res {
    let es = &series.provenance.excluded;
    if es.len() > MAX_EXCLUDED {
        return bad("provenance.excluded", R::TooMany);
    }
    for (k, e) in es.iter().enumerate() {
        if !exclusion_reason_ok(&e.reason) {
            return bad(format!("provenance.excluded[{k}].reason"), R::BadFormat);
        }
    }
    for (k, w) in es.windows(2).enumerate() {
        if w[0].source_index >= w[1].source_index {
            return bad(format!("provenance.excluded[{}]", k + 1), R::NotCanonicalOrder);
        }
    }
    Ok(())
}

/// Every rule-violation marker's review agrees with the other markers of its set and with
/// `provenance.unverified`.
fn reviews_against_unverified(series: &ChartSeries) -> Res {
    let listed =
        |id: &str, v: u32| {
            series.provenance.unverified.iter().any(
                |u| matches!(u, UnverifiedRef::RuleSet { set_id, set_version } if set_id == id && *set_version == v),
            )
        };
    // The review of the first marker of each (set ID, version): a map, so hostile input with
    // many distinct sets stays O(n log n).
    let mut seen: BTreeMap<(&str, u32), ReviewStatus> = BTreeMap::new();
    for (i, p) in series.points.iter().enumerate() {
        for (j, m) in p.markers.iter().enumerate() {
            let Marker::RuleViolation { set_id, set_version, review, .. } = m else {
                continue;
            };
            let at = format!("points[{i}].markers[{j}].review");
            let first = *seen.entry((set_id.as_str(), *set_version)).or_insert(*review);
            if first != *review {
                return bad(at, R::ConflictingReview);
            }
            let is_listed = listed(set_id, *set_version);
            if *review == ReviewStatus::ExpertReviewed && is_listed {
                return bad(at, R::ConflictingReview);
            }
            if *review != ReviewStatus::ExpertReviewed && !is_listed {
                return bad(at, R::NotInUnverified);
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ grammars

fn lower(c: u8) -> bool {
    c.is_ascii_lowercase()
}

fn word(s: &str, first: fn(u8) -> bool, rest: fn(u8) -> bool, max: usize) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= max && first(b[0]) && b[1..].iter().all(|&c| rest(c))
}

fn lower_digit_us(c: u8) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'
}

/// `[a-z][a-z0-9_]*` segments joined by `.`, at most [`MAX_LABEL_KEY_BYTES`] bytes.
pub(crate) fn label_key_ok(s: &str) -> bool {
    s.len() <= MAX_LABEL_KEY_BYTES && s.split('.').all(|seg| word(seg, lower, lower_digit_us, usize::MAX))
}

/// `[A-Za-z0-9][A-Za-z0-9._:-]*`, 1 to [`MAX_ID_BYTES`] bytes.
pub(crate) fn id_ok(s: &str) -> bool {
    word(
        s,
        |c| c.is_ascii_alphanumeric(),
        |c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'-'),
        MAX_ID_BYTES,
    )
}

/// `[a-z][a-z0-9_]*`, at most 64 bytes (RPT.STATS.SPC.RULES).
fn set_id_ok(s: &str) -> bool {
    word(s, lower, lower_digit_us, 64)
}

/// `[a-z0-9][a-z0-9_]*`, at most 32 bytes (RPT.STATS.SPC.RULES).
fn rule_id_ok(s: &str) -> bool {
    word(s, |c| c.is_ascii_lowercase() || c.is_ascii_digit(), lower_digit_us, 32)
}

/// `0` or `[1-9][0-9]{0,8}`.
fn semver_part_ok(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= 9 && b.iter().all(u8::is_ascii_digit) && (b.len() == 1 || b[0] != b'0')
}

/// `MAJOR.MINOR.PATCH`.
pub(crate) fn semver_ok(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| semver_part_ok(p))
}

/// At least three `.`-joined segments `[a-z][a-z0-9_-]*`, `@`, a semver; at most
/// [`MAX_ALGORITHM_BYTES`] bytes.
pub(crate) fn algorithm_ok(s: &str) -> bool {
    let Some((name, version)) = s.split_once('@') else {
        return false;
    };
    let segments: Vec<&str> = name.split('.').collect();
    s.len() <= MAX_ALGORITHM_BYTES
        && segments.len() >= 3
        && segments.iter().all(|g| word(g, lower, |c| lower_digit_us(c) || c == b'-', usize::MAX))
        && semver_ok(version)
}

/// `[a-z][a-z0-9_]{0,55}@[1-9][0-9]{0,5}`.
pub(crate) fn binning_rule_ok(s: &str) -> bool {
    let Some((name, major)) = s.split_once('@') else {
        return false;
    };
    let m = major.as_bytes();
    word(name, lower, lower_digit_us, 56)
        && !m.is_empty()
        && m.len() <= 6
        && m[0] != b'0'
        && m.iter().all(u8::is_ascii_digit)
}

/// `[a-z][a-z0-9_]*` segments joined by `.`, `@`, a semver; at most
/// [`MAX_CONSTANTS_REF_BYTES`] bytes.
pub(crate) fn constants_ref_ok(s: &str) -> bool {
    let Some((name, version)) = s.split_once('@') else {
        return false;
    };
    s.len() <= MAX_CONSTANTS_REF_BYTES
        && name.split('.').all(|seg| word(seg, lower, lower_digit_us, usize::MAX))
        && semver_ok(version)
}

/// `[A-Z][A-Z0-9_]*`, at most 64 bytes.
fn exclusion_reason_ok(s: &str) -> bool {
    word(s, |c| c.is_ascii_uppercase(), |c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_', 64)
}

/// 1 to [`MAX_LABEL_CHARS`] characters, none forbidden by `fid-types`.
fn x_label_ok(s: &str) -> bool {
    let n = s.chars().count();
    (1..=MAX_LABEL_CHARS).contains(&n) && !s.chars().any(fid_types::text::is_forbidden_char)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammars() {
        assert!(label_key_ok("stats.axis.value") && !label_key_ok("stats..axis") && !label_key_ok(""));
        assert!(!label_key_ok("stats.Axis") && !label_key_ok("1stats") && !label_key_ok("stats.axis."));
        assert!(id_ok("rec-0001") && id_ok("part:1") && !id_ok("-rec") && !id_ok("rec 1") && !id_ok(""));
        assert!(id_ok(&"a".repeat(128)) && !id_ok(&"a".repeat(129)));
        assert!(semver_ok("0.1.0") && semver_ok("10.0.999999999") && !semver_ok("01.0.0") && !semver_ok("1.0"));
        assert!(!semver_ok("1.0.1234567890") && !semver_ok("1.0.+1"));
        assert!(algorithm_ok("stats.trend.build@1.0.0") && algorithm_ok("lvm.plot.vector-group.v2@1.2.3"));
        assert!(!algorithm_ok("stats.trend@1.0.0") && !algorithm_ok("stats.trend.build") && !algorithm_ok("@1.0.0"));
        assert!(!algorithm_ok("Stats.trend.build@1.0.0") && !algorithm_ok("stats.trend.build@1.0.0@1.0.0"));
        assert!(binning_rule_ok("sturges@1") && binning_rule_ok("fixed_width@999999"));
        assert!(!binning_rule_ok("sturges@0") && !binning_rule_ok("sturges@1000000") && !binning_rule_ok("sturges"));
        assert!(!binning_rule_ok("sturges@01") && !binning_rule_ok("Sturges@1") && !binning_rule_ok("@1"));
        assert!(set_id_ok("nelson") && !set_id_ok("1nelson") && rule_id_ok("1") && !rule_id_ok("R1"));
        assert!(constants_ref_ok("spc.constants.normal@1.0.0") && constants_ref_ok("t@0.0.0"));
        assert!(!constants_ref_ok("spc.constants.normal") && !constants_ref_ok("spc..normal@1.0.0"));
        assert!(!constants_ref_ok("Spc@1.0.0") && !constants_ref_ok("spc@1.0") && !constants_ref_ok("@1.0.0"));
        assert!(exclusion_reason_ok("NOT_DECIDABLE") && exclusion_reason_ok("USER") && !exclusion_reason_ok("user"));
        assert!(!exclusion_reason_ok("_USER") && !exclusion_reason_ok("") && !exclusion_reason_ok(&"A".repeat(65)));
        assert!(x_label_ok("Teil 1 \u{2013} Gr\u{f6}\u{df}e") && !x_label_ok("a\u{202e}b") && !x_label_ok(""));
        assert!(x_label_ok(&"\u{e9}".repeat(256)) && !x_label_ok(&"\u{e9}".repeat(257)));
    }
}

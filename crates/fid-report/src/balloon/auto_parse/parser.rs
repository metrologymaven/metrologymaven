//! Deterministic recursive-descent / rule-based engineering callout grammar parser
//! (`RPT.BALLOON.AUTO.PARSE`, `REQ-001..005`, `AC-1..AC-4`).
//!
//! Pure D0 string & integer grammar: never uses floating-point math, never panics on arbitrary
//! Unicode, preserves exact authored decimal strings verbatim (with trailing zeros).

use super::cluster::{cluster_runs, RunCluster};
use super::model::{
    CalloutKind, ConfidenceWord, DecimalString, GdtSymbol, Modifier, ParseDrawingResult, ParsedCallout, ReasonCode,
    TitleBlockHints, UnitHint,
};
use crate::balloon::auto_text::{DrawingTextMap, SheetRect};
use crate::doc::text::{Text, Untrusted};

/// Clusters spatially adjacent text runs in `map` and parses all engineering callouts while
/// excluding title-block metadata and general notes (`REQ-001..005`).
pub fn parse_drawing_text_map(map: &DrawingTextMap) -> ParseDrawingResult {
    let clusters = cluster_runs(&map.runs, &map.frame_boxes);
    let mut callouts = Vec::new();
    let mut excluded_run_ids = Vec::new();
    let mut hints = TitleBlockHints::default();

    for cluster in clusters {
        let joined = join_cluster_text(&cluster);
        let run_ids: Vec<u32> = cluster.runs.iter().map(|r| r.id).collect();

        // Check title-block metadata / general notes first (REQ-004, AC-4).
        if let Some(hint_unit) = detect_title_block_or_note(&joined, &mut hints) {
            if hints.unit_hint.is_none() {
                hints.unit_hint = hint_unit;
            }
            excluded_run_ids.extend(run_ids);
            continue;
        }

        if let Some(mut parsed) = parse_callout_text(&joined, cluster.bbox, run_ids.clone()) {
            if cluster.stacked_tolerance && !parsed.reasons.contains(&ReasonCode::StackedClusterMerged) {
                parsed.reasons.push(ReasonCode::StackedClusterMerged);
            }
            callouts.push(parsed);
        } else {
            excluded_run_ids.extend(run_ids);
        }
    }

    // Apply drawing-wide unit hint from title block to linear/radial callouts lacking explicit unit.
    if let Some(doc_unit) = hints.unit_hint {
        for c in &mut callouts {
            if c.unit_hint.is_none() && !matches!(c.kind, CalloutKind::Angle) {
                c.unit_hint = Some(doc_unit);
            }
        }
    }

    // Deterministic reading order: top-to-bottom, left-to-right, then raw text.
    callouts.sort_by(|a, b| {
        a.bbox
            .y0
            .cmp(&b.bbox.y0)
            .then_with(|| a.bbox.x0.cmp(&b.bbox.x0))
            .then_with(|| a.raw_text.untrusted_value().as_str().cmp(b.raw_text.untrusted_value().as_str()))
    });
    excluded_run_ids.sort();

    ParseDrawingResult { callouts, excluded_run_ids, title_block_hints: hints }
}

/// Parses a single standalone callout text string (useful for unit/corpus testing and OCR boxes).
pub fn parse_callout_text(raw: &str, bbox: SheetRect, run_ids: Vec<u32>) -> Option<ParsedCallout> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut dummy_hints = TitleBlockHints::default();
    if detect_title_block_or_note(trimmed, &mut dummy_hints).is_some() {
        return None;
    }

    let raw_untrusted = Untrusted::text(trimmed).ok()?;

    // 1. GD&T Feature Control Frame (starts with a GD&T symbol).
    if let Some((sym, rest)) = strip_gdt_symbol(trimmed) {
        return Some(parse_gdt_fcf(sym, rest, raw_untrusted, bbox, run_ids));
    }

    // 2. Basic dimension `[25.00]`.
    if let Some(inner) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        if let Some(nom) = DecimalString::parse(inner.trim()) {
            return Some(ParsedCallout {
                kind: CalloutKind::BasicDimension,
                repeat_count: 1,
                nominal: Some(nom),
                upper: None,
                lower: None,
                unit_hint: None,
                gdt_zone_diameter: false,
                datum_refs: Vec::new(),
                modifiers: Vec::new(),
                raw_text: raw_untrusted,
                bbox,
                run_ids,
                confidence: ConfidenceWord::High,
                reasons: vec![ReasonCode::ExactMatch],
            });
        }
    }

    // 3. Reference dimension `(10.0)`.
    if let Some(inner) = trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        if let Some(nom) = DecimalString::parse(inner.trim()) {
            return Some(ParsedCallout {
                kind: CalloutKind::ReferenceDimension,
                repeat_count: 1,
                nominal: Some(nom),
                upper: None,
                lower: None,
                unit_hint: None,
                gdt_zone_diameter: false,
                datum_refs: Vec::new(),
                modifiers: Vec::new(),
                raw_text: raw_untrusted,
                bbox,
                run_ids,
                confidence: ConfidenceWord::High,
                reasons: vec![ReasonCode::ExactMatch],
            });
        }
    }

    // 4. Optional repeat count prefix (`4X `, `2x `).
    let (repeat_count, after_rep) = strip_repeat_prefix(trimmed);
    let s = after_rep.trim();

    // 5. Surface finish (`Ra 1.6`, `Rz 6.3`).
    if let Some(rest) = s.strip_prefix("Ra ").or_else(|| s.strip_prefix("RA ")).or_else(|| s.strip_prefix("Rz ")) {
        if let Some(nom) = DecimalString::parse(rest.split_whitespace().next().unwrap_or("")) {
            return Some(ParsedCallout {
                kind: CalloutKind::SurfaceFinish,
                repeat_count,
                nominal: Some(nom),
                upper: None,
                lower: None,
                unit_hint: None,
                gdt_zone_diameter: false,
                datum_refs: Vec::new(),
                modifiers: Vec::new(),
                raw_text: raw_untrusted,
                bbox,
                run_ids,
                confidence: ConfidenceWord::High,
                reasons: vec![ReasonCode::ExactMatch],
            });
        }
    }

    // 6. Metric thread (`M6x1.0-6H`, `M8x1.25`).
    if let Some(rest) = s.strip_prefix('M') {
        if let Some((maj, tail)) = split_thread_major(rest) {
            if tail.starts_with('x') || tail.starts_with('X') || tail.starts_with('×') {
                if let Some(nom) = DecimalString::parse(maj) {
                    return Some(ParsedCallout {
                        kind: CalloutKind::Thread,
                        repeat_count,
                        nominal: Some(nom),
                        upper: None,
                        lower: None,
                        unit_hint: Some(UnitHint::Mm),
                        gdt_zone_diameter: false,
                        datum_refs: Vec::new(),
                        modifiers: Vec::new(),
                        raw_text: raw_untrusted,
                        bbox,
                        run_ids,
                        confidence: ConfidenceWord::High,
                        reasons: vec![ReasonCode::ExactMatch],
                    });
                }
            }
        }
    }

    // 7. Chamfer (`C2.0`, `2x45°`, `1.5×45°`).
    if let Some(rest) = s.strip_prefix('C') {
        if let Some(nom) = DecimalString::parse(rest.split_whitespace().next().unwrap_or("")) {
            return Some(ParsedCallout {
                kind: CalloutKind::Chamfer,
                repeat_count,
                nominal: Some(nom),
                upper: None,
                lower: None,
                unit_hint: None,
                gdt_zone_diameter: false,
                datum_refs: Vec::new(),
                modifiers: Vec::new(),
                raw_text: raw_untrusted,
                bbox,
                run_ids,
                confidence: ConfidenceWord::High,
                reasons: vec![ReasonCode::ExactMatch],
            });
        }
    }
    if let Some(nom) = parse_chamfer_x_angle(s) {
        return Some(ParsedCallout {
            kind: CalloutKind::Chamfer,
            repeat_count,
            nominal: Some(nom),
            upper: None,
            lower: None,
            unit_hint: None,
            gdt_zone_diameter: false,
            datum_refs: Vec::new(),
            modifiers: Vec::new(),
            raw_text: raw_untrusted,
            bbox,
            run_ids,
            confidence: ConfidenceWord::High,
            reasons: vec![ReasonCode::ExactMatch],
        });
    }

    // 8. Dimension prefix: SØ, Ø/⌀, SR, R, or plain linear/angle.
    let (mut kind, body) = strip_dim_prefix(s);
    let body = body.trim();
    if body.is_empty() {
        return None;
    }

    // Detect explicit unit suffix (`mm`, `in`, `°`).
    let has_degree = body.contains('°');
    let mut unit_hint = if has_degree {
        kind = CalloutKind::Angle;
        Some(UnitHint::Deg)
    } else if body.ends_with(" mm") || body.ends_with("mm") {
        Some(UnitHint::Mm)
    } else if body.ends_with(" in") || body.ends_with('"') {
        Some(UnitHint::Inch)
    } else {
        None
    };

    let clean_body =
        body.trim_end_matches(" mm").trim_end_matches("mm").trim_end_matches(" in").trim_end_matches('"').trim();

    // Try tolerance forms in order of specificity:
    // (a) Symmetric `±` or `+/-`
    if let Some((nom_str, tol_str)) = split_symmetric(clean_body) {
        let nom = DecimalString::parse(strip_deg(nom_str))?;
        let tol_clean = strip_deg(tol_str).trim();
        let Some(tol_mag) = DecimalString::parse(tol_clean.trim_start_matches('+').trim_start_matches('-')) else {
            return Some(low_conf_ambiguous(kind, repeat_count, Some(nom), unit_hint, raw_untrusted, bbox, run_ids));
        };
        let upper = DecimalString::parse(&format!("+{}", tol_mag.as_str()));
        let lower = DecimalString::parse(&format!("-{}", tol_mag.as_str()));
        return Some(ParsedCallout {
            kind,
            repeat_count,
            nominal: Some(nom),
            upper,
            lower,
            unit_hint,
            gdt_zone_diameter: false,
            datum_refs: Vec::new(),
            modifiers: Vec::new(),
            raw_text: raw_untrusted,
            bbox,
            run_ids,
            confidence: ConfidenceWord::High,
            reasons: vec![ReasonCode::ExactMatch],
        });
    }

    // (b) Bilateral `<nom> +<a> / -<b>` or `<nom> +<a> -<b>` (including merged stacked tolerances).
    if let Some((nom, up, lo)) = parse_bilateral(clean_body) {
        return Some(ParsedCallout {
            kind,
            repeat_count,
            nominal: Some(nom),
            upper: Some(up),
            lower: Some(lo),
            unit_hint,
            gdt_zone_diameter: false,
            datum_refs: Vec::new(),
            modifiers: Vec::new(),
            raw_text: raw_untrusted,
            bbox,
            run_ids,
            confidence: ConfidenceWord::High,
            reasons: vec![ReasonCode::ExactMatch],
        });
    }

    // (c) Limit dimension `<lo> - <hi>` (two positive numbers separated by ` - `).
    if let Some((lo, hi)) = parse_limit_dim(clean_body) {
        return Some(ParsedCallout {
            kind,
            repeat_count,
            nominal: None,
            upper: Some(hi),
            lower: Some(lo),
            unit_hint,
            gdt_zone_diameter: false,
            datum_refs: Vec::new(),
            modifiers: Vec::new(),
            raw_text: raw_untrusted,
            bbox,
            run_ids,
            confidence: ConfidenceWord::High,
            reasons: vec![ReasonCode::ExactMatch],
        });
    }

    // (d) Nominal with ISO fit class (`6.50 H7`, `12.00 g6`).
    let mut parts = clean_body.split_whitespace();
    let first = parts.next()?;
    let second = parts.next();
    let third = parts.next();

    if let Some(nom) = DecimalString::parse(strip_deg(first)) {
        match (second, third) {
            (None, _) => {
                // Plain dimension with no explicit tolerance (never fabricate one!).
                if has_degree {
                    unit_hint = Some(UnitHint::Deg);
                }
                let mut reasons = vec![ReasonCode::MissingTolerance];
                if unit_hint.is_none() {
                    reasons.push(ReasonCode::MissingUnit);
                }
                let confidence = if matches!(kind, CalloutKind::Linear) && !first.contains('.') {
                    // Bare integer without prefix or decimal point is low-confidence.
                    ConfidenceWord::Low
                } else {
                    ConfidenceWord::Medium
                };
                return Some(ParsedCallout {
                    kind,
                    repeat_count,
                    nominal: Some(nom),
                    upper: None,
                    lower: None,
                    unit_hint,
                    gdt_zone_diameter: false,
                    datum_refs: Vec::new(),
                    modifiers: Vec::new(),
                    raw_text: raw_untrusted,
                    bbox,
                    run_ids,
                    confidence,
                    reasons,
                });
            }
            (Some(fit), None) if is_iso_fit_class(fit) => {
                return Some(ParsedCallout {
                    kind,
                    repeat_count,
                    nominal: Some(nom),
                    upper: None,
                    lower: None,
                    unit_hint,
                    gdt_zone_diameter: false,
                    datum_refs: Vec::new(),
                    modifiers: Vec::new(),
                    raw_text: raw_untrusted,
                    bbox,
                    run_ids,
                    confidence: ConfidenceWord::High,
                    reasons: vec![ReasonCode::ExactMatch, ReasonCode::IsoFitClass],
                });
            }
            (Some(_), _) => {
                // Trailing ambiguous tokens (e.g. occluded/garbled tolerance `12.00 ±???` -> AC-3).
                return Some(low_conf_ambiguous(
                    kind,
                    repeat_count,
                    Some(nom),
                    unit_hint,
                    raw_untrusted,
                    bbox,
                    run_ids,
                ));
            }
        }
    }

    None
}

fn low_conf_ambiguous(
    kind: CalloutKind,
    repeat_count: u32,
    nominal: Option<DecimalString>,
    unit_hint: Option<UnitHint>,
    raw_text: Untrusted<Text>,
    bbox: SheetRect,
    run_ids: Vec<u32>,
) -> ParsedCallout {
    let mut reasons = vec![ReasonCode::AmbiguousTolerance];
    if unit_hint.is_none() {
        reasons.push(ReasonCode::MissingUnit);
    }
    ParsedCallout {
        kind,
        repeat_count,
        nominal,
        upper: None,
        lower: None,
        unit_hint,
        gdt_zone_diameter: false,
        datum_refs: Vec::new(),
        modifiers: Vec::new(),
        raw_text,
        bbox,
        run_ids,
        confidence: ConfidenceWord::Low,
        reasons,
    }
}

fn join_cluster_text(cluster: &RunCluster) -> String {
    if cluster.runs.len() == 1 {
        return cluster.runs[0].text.untrusted_value().as_str().trim().to_string();
    }
    let parts: Vec<&str> =
        cluster.runs.iter().map(|r| r.text.untrusted_value().as_str().trim()).filter(|s| !s.is_empty()).collect();
    if cluster.stacked_tolerance && parts.len() == 3 {
        // E.g. ["12.000", "+0.018", "-0.000"] -> "12.000 +0.018/-0.000"
        return format!("{} {}/{}", parts[0], parts[1], parts[2]);
    }
    if cluster.in_frame_box {
        return parts.join(" | ");
    }
    parts.join(" ")
}

/// Detects title-block metadata and general-note distractors (`REQ-004`, `AC-4`).
/// Returns `Some(maybe_unit_hint)` when the text should be excluded from characteristic candidates.
fn detect_title_block_or_note(s: &str, hints: &mut TitleBlockHints) -> Option<Option<UnitHint>> {
    let upper = s.to_ascii_uppercase();

    // Detect unit declarations in title-block notes.
    if upper.contains("ALL DIMENSIONS IN") || upper.contains("DIMENSIONS ARE IN") || upper.contains("UNITS:") {
        let unit = if upper.contains("MILLIMETER") || upper.contains("MM") {
            Some(UnitHint::Mm)
        } else if upper.contains("INCH") || upper.contains(" IN") {
            Some(UnitHint::Inch)
        } else {
            None
        };
        if let Ok(ut) = Untrusted::text(s) {
            hints.default_tolerance_notes.push(ut);
        }
        return Some(unit);
    }
    if upper.contains("UNLESS OTHERWISE SPECIFIED") || upper.contains("GENERAL TOLERANCE") {
        if let Ok(ut) = Untrusted::text(s) {
            hints.default_tolerance_notes.push(ut);
        }
        return Some(None);
    }

    const EXCLUDE_PREFIXES: &[&str] = &[
        "DRAWN BY",
        "CHECKED BY",
        "APPROVED BY",
        "SCALE",
        "SHEET ",
        "REV ",
        "REV:",
        "REVISION",
        "PART NO",
        "PART NUMBER",
        "DWG NO",
        "DRAWING NO",
        "DATE:",
        "DATE ",
        "MATERIAL",
        "FINISH:",
        "TITLE:",
        "THIRD ANGLE",
        "FIRST ANGLE",
        "PROPRIETARY",
        "CONFIDENTIAL",
        "DO NOT SCALE",
        "CAD FILE",
        "WEIGHT:",
        "MASS:",
        "TOLERANCES:",
        "NOTES:",
    ];
    for p in EXCLUDE_PREFIXES {
        if upper.starts_with(p) {
            return Some(None);
        }
    }
    // Single bare letters (zone labels "A", "B", "1", "2") or pure alphabetic words with no digits/symbols.
    if !s.chars().any(|c| c.is_ascii_digit()) && strip_gdt_symbol(s).is_none() {
        return Some(None);
    }
    None
}

fn strip_gdt_symbol(s: &str) -> Option<(GdtSymbol, &str)> {
    let mut chars = s.chars();
    let first = chars.next()?;
    let sym = match first {
        '\u{2316}' => GdtSymbol::Position,                      // ⌖
        '\u{23E5}' => GdtSymbol::Flatness,                      // ⏥
        '\u{25CB}' => GdtSymbol::Circularity,                   // ○
        '\u{232D}' => GdtSymbol::Cylindricity,                  // ⌭
        '\u{27C2}' | '\u{22A5}' => GdtSymbol::Perpendicularity, // ⟂ / ⊥
        '\u{2225}' => GdtSymbol::Parallelism,                   // ∥
        '\u{2220}' => GdtSymbol::Angularity,                    // ∠
        '\u{2313}' => GdtSymbol::ProfileSurface,                // ⌓
        '\u{2312}' => GdtSymbol::ProfileLine,                   // ⌒
        '\u{2197}' => GdtSymbol::CircularRunout,                // ↗
        '\u{2330}' => GdtSymbol::TotalRunout,                   // ⌰
        '\u{25CE}' => GdtSymbol::Concentricity,                 // ◎
        '\u{232F}' => GdtSymbol::Symmetry,                      // ⌯
        '\u{23E4}' => GdtSymbol::Straightness,                  // ⏤
        _ => return None,
    };
    Some((sym, chars.as_str()))
}

fn parse_gdt_fcf(
    sym: GdtSymbol,
    rest: &str,
    raw_text: Untrusted<Text>,
    bbox: SheetRect,
    run_ids: Vec<u32>,
) -> ParsedCallout {
    let compartments: Vec<&str> = rest.split('|').map(str::trim).filter(|c| !c.is_empty()).collect();

    let mut nominal: Option<DecimalString> = None;
    let mut gdt_zone_diameter = false;
    let mut modifiers = Vec::new();
    let mut datum_refs = Vec::new();

    if let Some(&zone_comp) = compartments.first() {
        for tok in zone_comp.split_whitespace() {
            if let Some(d) = tok.strip_prefix('\u{00D8}').or_else(|| tok.strip_prefix('\u{2300}')) {
                gdt_zone_diameter = true;
                if nominal.is_none() {
                    nominal = DecimalString::parse(d);
                }
            } else if let Some(m) = parse_modifier(tok) {
                modifiers.push(m);
            } else if nominal.is_none() {
                if let Some(d) = DecimalString::parse(tok) {
                    nominal = Some(d);
                }
            }
        }
    }

    for &comp in compartments.iter().skip(1) {
        let mut datum_label = String::new();
        for tok in comp.split_whitespace() {
            if let Some(m) = parse_modifier(tok) {
                if !modifiers.contains(&m) {
                    modifiers.push(m);
                }
            } else if is_datum_token(tok) {
                if !datum_label.is_empty() {
                    datum_label.push(' ');
                }
                datum_label.push_str(tok);
            }
        }
        if !datum_label.is_empty() {
            if let Ok(ut) = Untrusted::text(&datum_label) {
                datum_refs.push(ut);
            }
        }
    }

    let (confidence, reasons) = if nominal.is_some() {
        (ConfidenceWord::High, vec![ReasonCode::ExactMatch])
    } else {
        (ConfidenceWord::Low, vec![ReasonCode::PartialGdtFrame])
    };

    ParsedCallout {
        kind: CalloutKind::GdtFcf(sym),
        repeat_count: 1,
        nominal,
        upper: None,
        lower: None,
        unit_hint: None,
        gdt_zone_diameter,
        datum_refs,
        modifiers,
        raw_text,
        bbox,
        run_ids,
        confidence,
        reasons,
    }
}

fn parse_modifier(tok: &str) -> Option<Modifier> {
    match tok {
        "M" | "(M)" | "\u{24C2}" => Some(Modifier::Mmc),
        "L" | "(L)" | "\u{24C1}" => Some(Modifier::Lmc),
        "P" | "(P)" | "\u{24C5}" => Some(Modifier::Projected),
        "F" | "(F)" | "\u{24BB}" => Some(Modifier::FreeState),
        "T" | "(T)" | "\u{24C9}" => Some(Modifier::TangentPlane),
        _ => None,
    }
}

fn is_datum_token(tok: &str) -> bool {
    !tok.is_empty()
        && tok.len() <= 5
        && tok.bytes().all(|b| b.is_ascii_uppercase() || b == b'-')
        && tok.bytes().any(|b| b.is_ascii_uppercase())
}

fn strip_repeat_prefix(s: &str) -> (u32, &str) {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i < bytes.len() && matches!(bytes[i], b'X' | b'x') {
        let rest = &s[i + 1..];
        // Distinguish repeat count `"4X Ø6.50"` (followed by whitespace) from chamfer `"2x45°"`.
        if rest.starts_with(' ') || rest.starts_with('\t') {
            if let Ok(n) = s[..i].parse::<u32>() {
                if n >= 1 {
                    return (n, rest.trim_start());
                }
            }
        }
    }
    (1, s)
}

fn strip_dim_prefix(s: &str) -> (CalloutKind, &str) {
    if let Some(r) = s.strip_prefix("S\u{00D8}").or_else(|| s.strip_prefix("S\u{2300}")) {
        return (CalloutKind::SphericalDiameter, r);
    }
    if let Some(r) =
        s.strip_prefix('\u{00D8}').or_else(|| s.strip_prefix('\u{2300}')).or_else(|| s.strip_prefix('\u{2205}'))
    {
        return (CalloutKind::Diameter, r);
    }
    if let Some(r) = s.strip_prefix("SR") {
        if r.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return (CalloutKind::SphericalRadius, r);
        }
    }
    if let Some(r) = s.strip_prefix('R') {
        if r.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return (CalloutKind::Radius, r);
        }
    }
    (CalloutKind::Linear, s)
}

fn split_thread_major(rest: &str) -> Option<(&str, &str)> {
    let mut i = 0usize;
    let bytes = rest.as_bytes();
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    Some((&rest[..i], &rest[i..]))
}

fn parse_chamfer_x_angle(s: &str) -> Option<DecimalString> {
    let (lhs, rhs) = s.split_once('x').or_else(|| s.split_once('X')).or_else(|| s.split_once('×'))?;
    if !rhs.contains('°') {
        return None;
    }
    let _ang = DecimalString::parse(strip_deg(rhs.trim()))?;
    DecimalString::parse(lhs.trim())
}

fn split_symmetric(s: &str) -> Option<(&str, &str)> {
    if let Some((a, b)) = s.split_once('\u{00B1}') {
        return Some((a.trim(), b.trim()));
    }
    if let Some((a, b)) = s.split_once("+/-") {
        return Some((a.trim(), b.trim()));
    }
    None
}

fn parse_bilateral(s: &str) -> Option<(DecimalString, DecimalString, DecimalString)> {
    // Handles: "45.00 +0.02/-0.01", "12.000 +0.018 / -0.000", "30° +0.5°/-0.0°"
    let (before_slash, after_slash) = s.split_once('/')?;
    let before_slash = before_slash.trim();
    let after_slash = strip_deg(after_slash.trim()).trim();

    // Split `before_slash` into `<nominal>` and `<signed_upper>`.
    let split_idx = before_slash.rfind(|c: char| c.is_whitespace())?;
    let nom_str = strip_deg(before_slash[..split_idx].trim());
    let up_str = strip_deg(before_slash[split_idx..].trim());
    if !(up_str.starts_with('+') || up_str.starts_with('-')) {
        return None;
    }
    if !(after_slash.starts_with('+') || after_slash.starts_with('-')) {
        return None;
    }
    let nom = DecimalString::parse(nom_str)?;
    let up = DecimalString::parse(up_str)?;
    let lo = DecimalString::parse(after_slash)?;
    Some((nom, up, lo))
}

fn parse_limit_dim(s: &str) -> Option<(DecimalString, DecimalString)> {
    let (lhs, rhs) = s.split_once(" - ")?;
    let lo = DecimalString::parse(strip_deg(lhs.trim()))?;
    let hi = DecimalString::parse(strip_deg(rhs.trim()))?;
    Some((lo, hi))
}

fn strip_deg(s: &str) -> &str {
    s.trim().trim_end_matches('°').trim()
}

fn is_iso_fit_class(tok: &str) -> bool {
    let b = tok.as_bytes();
    if b.len() < 2 || b.len() > 4 {
        return false;
    }
    let mut i = 0usize;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    (1..=2).contains(&i) && i < b.len() && b[i..].iter().all(u8::is_ascii_digit)
}

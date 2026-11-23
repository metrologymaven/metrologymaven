//! Deterministic AS9102 Form 3 starter CSV generator with strict CSV formula-injection
//! neutralisation (`RPT.BALLOON.AUTO.EXPORT`, `REQ-001`, `REQ-003`, `REQ-005`, `AC-1`, `AC-4`).

use super::pdf_overlay::BalloonOutcome;
use crate::balloon::char::DrawingCharacteristic;
use crate::balloon::register::Balloon;
use fid_gdt::chars::dprof::CharId;
use std::collections::BTreeMap;

/// Optional evaluated result text + outcome for one characteristic (empty/not_evaluated when unmeasured).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluatedRowValue {
    /// Kernel-formatted measurement text (from `fid_report::fmt`), if evaluated.
    pub formatted_result: String,
    pub outcome: BalloonOutcome,
}

/// Renders deterministic RFC 4180 CRLF AS9102 Form 3 starter CSV ordered by ascending balloon number.
pub fn render_form3_starter_csv(
    balloons: &[Balloon],
    characteristics: &BTreeMap<CharId, DrawingCharacteristic>,
    evaluated: &BTreeMap<CharId, EvaluatedRowValue>,
    include_unmeasured: bool,
) -> Vec<u8> {
    let mut ordered: Vec<&Balloon> = balloons.iter().collect();
    ordered.sort_by_key(|b| b.number);

    let mut out = String::with_capacity(ordered.len() * 128 + 128);
    out.push_str(
        "CharNo,ReferenceLocation,CharacteristicDesignator,Requirement,Nominal,UpperTol,LowerTol,Unit,Dialect,Results,Status\r\n",
    );

    for b in ordered {
        let eval = evaluated.get(&b.characteristic);
        if eval.is_none() && !include_unmeasured {
            continue;
        }
        let zone_raw = b.zone.as_ref().map_or("", |z| z.untrusted_value().as_str());
        let zone_safe = csv_cell_untrusted(zone_raw);

        let (designator, req_text, nominal, upper, lower, unit, dialect) =
            if let Some(ch) = characteristics.get(&b.characteristic) {
                let req = ch.name.as_ref().map(|n| n.untrusted_value().as_str().to_string()).unwrap_or_else(|| {
                    format!(
                        "{} {} ({}/{})",
                        ch.kind.as_str(),
                        ch.nominal.authored_literal(),
                        ch.upper.raw_decimal.as_str(),
                        ch.lower.raw_decimal.as_str()
                    )
                });
                (
                    ch.kind.as_str().to_string(),
                    req,
                    ch.nominal.raw_decimal.as_str().to_string(),
                    ch.upper.raw_decimal.as_str().to_string(),
                    ch.lower.raw_decimal.as_str().to_string(),
                    ch.nominal.unit.as_str().to_string(),
                    ch.dialect.as_str().to_string(),
                )
            } else {
                (
                    "unlinked".to_string(),
                    b.characteristic.as_str().to_string(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                )
            };

        let (result_text, status_text) = match eval {
            Some(ev) => (ev.formatted_result.as_str(), ev.outcome.as_str()),
            None => ("", "not_evaluated"),
        };

        out.push_str(&b.number.to_string());
        out.push(',');
        out.push_str(&zone_safe);
        out.push(',');
        out.push_str(&csv_cell_untrusted(&designator));
        out.push(',');
        out.push_str(&csv_cell_untrusted(&req_text));
        out.push(',');
        out.push_str(&csv_cell_literal(&nominal));
        out.push(',');
        out.push_str(&csv_cell_literal(&upper));
        out.push(',');
        out.push_str(&csv_cell_literal(&lower));
        out.push(',');
        out.push_str(&csv_cell_literal(&unit));
        out.push(',');
        out.push_str(&csv_cell_literal(&dialect));
        out.push(',');
        out.push_str(&csv_cell_untrusted(result_text));
        out.push(',');
        out.push_str(&csv_cell_literal(status_text));
        out.push_str("\r\n");
    }

    out.into_bytes()
}

/// Escapes an untrusted free-text CSV cell, neutralising spreadsheet formula-injection prefixes
/// (`=`, `+`, `-`, `@`, `\t`, `\r` after optional leading whitespace) per `REQ-003` and `AC-4`.
pub fn csv_cell_untrusted(raw: &str) -> String {
    let trimmed_start = raw.trim_start_matches(' ');
    let dangerous =
        trimmed_start.as_bytes().first().is_some_and(|b| matches!(b, b'=' | b'+' | b'-' | b'@' | b'\t' | b'\r'));
    let neutralised = if dangerous { format!("'{raw}") } else { raw.to_string() };
    quote_rfc4180(&neutralised)
}

/// Escapes a trusted/validated schema literal (e.g. exact `DecimalString` like `"+0.018"`, `"-0.000"`,
/// unit `"mm"`, status `"not_evaluated"`) using RFC 4180 quoting without corrupting numeric signs.
fn csv_cell_literal(raw: &str) -> String {
    quote_rfc4180(raw)
}

fn quote_rfc4180(s: &str) -> String {
    let needs_quote = s.bytes().any(|b| matches!(b, b',' | b'"' | b'\r' | b'\n' | b'\''));
    if !needs_quote {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        if ch == '"' {
            out.push_str("\"\"");
        } else {
            out.push(ch);
        }
    }
    out.push('"');
    out
}

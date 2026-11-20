//! `fid-design color-check --root <design dir> [--out <file>] [--json]` (spec node
//! `UX.TOKENS.CHECK`): the release gate of the design-system token values. One run executes
//! `tokens-check` ([`crate::tokens`]), `status-check` ([`crate::status`]) and `palettes-check`
//! ([`crate::palettes`]) over the same tree, adds the measured release rules below, and writes
//! the `color-report@1` release report. A release is refused while any finding remains.
//!
//! Measured rules (findings, exit 1):
//! - `CONTRAST <theme> <fg>/<bg> <ratio>`: a declared pair of `tokens/pairs.json` below its WCAG 2.2
//!   minimum in a theme other than `hc`: `text` 4.5:1, `large-text` 3:1 (SC 1.4.3), `non-text` 3:1
//!   (SC 1.4.11); also a `non-text` pair below 3:1 in `hc` (REQ-UX.TOKENS.CHECK-001).
//! - `CONTRAST-HC hc <fg>/<bg> <ratio>`: a `text` or `large-text` pair below 7:1 in `hc`
//!   (REQ-UX.TOKENS-004).
//! - `STATUS-DISTANCE <vision> <theme> <state>`: a nonconforming-set colour of
//!   `tokens/status.json` closer than the file's `min_delta_e2000` (CIEDE2000) to a
//!   conforming-set colour of the same theme under that vision condition
//!   (REQ-UX.TOKENS.CHECK-004, REQ-UX.TOKENS-009). (`status-check` uses the same code for its
//!   neutral-state rule, with a `<theme> <state> <other>` subject.)
//! - `CHECK-INPUT <what>`: a pair side that does not resolve to a colour in a theme, a colour
//!   with alpha over a background with alpha, or a status file without a readable threshold or
//!   sets.
//! - Every finding of `tokens-check`, `status-check` and `palettes-check` (codes `TOK-*`,
//!   `STATUS-*`, `PAL-*`), among them `PAL-LIGHTNESS` and `PAL-DELTA` (REQ-UX.TOKENS.CHECK-003).
//!
//! The report lists every declared pair in every theme with its ratio, every nonconforming state
//! against every conforming state per theme under the four vision conditions, and every stepped
//! palette's CIELAB L* per step and adjacent-step CIEDE2000 under the four vision conditions.
//! Every measured value is rounded to 0.01 (correctly rounded decimal of the binary value). It
//! records the WCAG version, the colour-difference formula and the CVD model with the digest of
//! its parameter table (REQ-UX.TOKENS.CHECK-002, REQ-UX.TOKENS-008).
//!
//! Semi-transparent colours (`#rrggbbaa`) are composited over the pair's background in 8-bit
//! sRGB before measuring; a background with alpha is `CHECK-INPUT`.
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable tree or report path. Determinism D1:
//! `BTreeMap` only, no clock; f64 transcendental functions may differ in the last bit between
//! platforms, so every reported value is rounded to 0.01 and the shipped values clear every
//! threshold by more than that.

use crate::finding::{findings_text, findings_value, Finding};
use crate::palettes::{lstar, Kind};
use crate::status::cvd::{delta_e2000_under, Vision};
use crate::tokens::wcag::{contrast_ratio, parse_hex, Rgb};
use crate::tokens::THEMES;
use fid_canon::CValue;
use std::collections::BTreeMap;
use std::path::Path;

/// Report format ID.
pub const REPORT_FORMAT: &str = "color-report@1";
/// The WCAG version whose definitions are used.
pub const WCAG: &str = "2.2";
/// The colour-difference formula.
pub const FORMULA: &str = "ciede2000 (Sharma, Wu and Dalal 2005)";
/// The colour-vision-deficiency model.
pub const CVD_MODEL: &str = "Machado, Oliveira and Fernandes (2009)";
/// The parameter table used: the published severity-1.0 matrices on linear sRGB.
pub const CVD_TABLE: &str = "machado-2009-severity-1.0@1";
/// The release report file name the spec gives.
pub const REPORT_FILE: &str = "color-report.json";
/// Least adjacent-step CIEDE2000 of a stepped palette (REQ-UX.TOKENS-007).
pub const MIN_STEP_DELTA_E2000: f64 = crate::palettes::MIN_STEP_DELTA_E2000;
/// Least contrast of a text pair in the high-contrast theme (REQ-UX.TOKENS-004).
pub const HC_TEXT: f64 = 7.0;

pub const CONTRAST: &str = "CONTRAST";
pub const CONTRAST_HC: &str = "CONTRAST-HC";
pub const STATUS_DISTANCE: &str = "STATUS-DISTANCE";
pub const CHECK_INPUT: &str = "CHECK-INPUT";

/// One pair in one theme.
#[derive(Debug, Clone, PartialEq)]
pub struct PairResult {
    pub theme: String,
    pub fg: String,
    pub bg: String,
    pub tag: String,
    pub ratio: f64,
    pub required: f64,
}

/// One nonconforming state against one conforming state in one theme under one vision.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusResult {
    pub theme: String,
    pub state: String,
    pub other: String,
    pub vision: Vision,
    pub de2000: f64,
    pub required: f64,
}

/// One stepped palette.
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteResult {
    pub reference: String,
    pub default: bool,
    pub lstar: Vec<f64>,
    pub monotonic: bool,
    /// `(step, vision, CIEDE2000)` between step and step + 1.
    pub adjacent: Vec<(usize, Vision, f64)>,
}

/// Result of `color-check`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub pairs: Vec<PairResult>,
    pub status: Vec<StatusResult>,
    pub palettes: Vec<PaletteResult>,
    /// Findings per sub-check: `tokens-check`, `status-check`, `palettes-check`.
    pub gates: BTreeMap<&'static str, usize>,
    /// Sorted, de-duplicated; sub-check findings included.
    pub findings: Vec<Finding>,
}

/// A value rounded to 0.01: the correctly rounded two-decimal form of the binary value, read back.
pub fn round2(x: f64) -> f64 {
    format!("{x:.2}").parse().unwrap_or(x)
}

/// `#rrggbb` or `#rrggbbaa` (any case): the colour and its alpha byte.
pub fn parse_rgba(s: &str) -> Option<(Rgb, u8)> {
    if s.len() == 9 {
        let rgb = parse_hex(s.get(..7)?)?;
        let a = u8::from_str_radix(s.get(7..)?, 16).ok()?;
        return Some((rgb, a));
    }
    parse_hex(s).map(|c| (c, 255))
}

/// `fg` composited over the opaque `bg` in 8-bit sRGB (`c = (fa·f + (255 − fa)·b) / 255`, rounded).
pub fn composite(fg: Rgb, alpha: u8, bg: Rgb) -> Rgb {
    let mut out = [0u8; 3];
    for (i, o) in out.iter_mut().enumerate() {
        let v = u32::from(alpha) * u32::from(fg[i]) + (255 - u32::from(alpha)) * u32::from(bg[i]);
        *o = ((v + 127) / 255) as u8;
    }
    out
}

/// The canonical JSON form of the CVD parameter table and its `sha256:` digest.
pub fn cvd_table_digest() -> Result<String, fid_canon::CanonError> {
    let mut m = BTreeMap::new();
    for v in Vision::ALL {
        if let Some(rows) = v.matrix() {
            let rows =
                rows.iter().map(|r| CValue::Arr(r.iter().map(|x| CValue::Num(*x)).collect())).collect::<Vec<_>>();
            m.insert(v.as_str().to_string(), CValue::Arr(rows));
        }
    }
    fid_canon::digest(&CValue::Obj(m)).map(|d| d.to_string())
}

/// Runs the gate on `<root>`. `Err` (exit 2): a sub-check cannot read the tree.
pub fn check(root: &Path) -> Result<Report, String> {
    let tokens = crate::tokens::check(root)?;
    let status = crate::status::check(root)?;
    let palettes = crate::palettes::check(root, None)?;
    let mut f = Vec::new();
    let mut r = Report::default();
    r.gates.insert("tokens-check", tokens.findings.len());
    r.gates.insert("status-check", status.findings.len());
    r.gates.insert("palettes-check", palettes.findings.len());
    f.extend(tokens.findings.iter().cloned());
    f.extend(status.findings.iter().cloned());
    f.extend(palettes.findings.iter().cloned());

    // REQ-UX.TOKENS.CHECK-001: every declared pair in every theme.
    for theme in THEMES {
        let values = tokens.themes.get(theme).map(|t| &t.values);
        let get = |n: &str| values.and_then(|v| v.get(n)).and_then(|v| parse_rgba(&v.value));
        for p in &tokens.pairs {
            let Some(required) = crate::tokens::wcag::required_ratio(&p.tag, theme) else {
                continue; // an unknown tag is TOK-SCHEMA
            };
            let subject = format!("{theme} {}/{}", p.fg, p.bg);
            let (fg, bg) = match (get(&p.fg), get(&p.bg)) {
                (Some((fg, fa)), Some((bg, 255))) => (composite(fg, fa, bg), bg),
                (Some(_), Some(_)) => {
                    f.push(Finding::new(
                        CHECK_INPUT,
                        &subject,
                        "the background has alpha; nothing to composite it over",
                    ));
                    continue;
                }
                _ => {
                    f.push(Finding::new(CHECK_INPUT, &subject, "a side does not resolve to a colour in this theme"));
                    continue;
                }
            };
            let ratio = contrast_ratio(fg, bg);
            if ratio < required {
                let hc_text = theme == "hc" && p.tag != "non-text";
                let code = if hc_text { CONTRAST_HC } else { CONTRAST };
                f.push(Finding::new(
                    code,
                    format!("{subject} {ratio:.2}"),
                    format!("{} pair below {required}:1 (WCAG 2.2)", p.tag),
                ));
            }
            r.pairs.push(PairResult {
                theme: theme.to_string(),
                fg: p.fg.clone(),
                bg: p.bg.clone(),
                tag: p.tag.clone(),
                ratio,
                required,
            });
        }
    }

    // REQ-UX.TOKENS.CHECK-004: the status palette's set distance.
    match status_sets(root) {
        Some((min, conforming, nonconforming)) => {
            for theme in THEMES {
                let Some(row) = status.themes.get(theme) else {
                    continue; // STATUS-MISSING
                };
                for s in &nonconforming {
                    let Some(a) = row.get(s).and_then(|h| parse_hex(h)) else {
                        continue; // out-of-tolerance: UX.TOKENS.DATAPAL's PAL-OOT
                    };
                    for c in &conforming {
                        let Some(b) = row.get(c).and_then(|h| parse_hex(h)) else {
                            continue;
                        };
                        for v in Vision::ALL {
                            let d = delta_e2000_under(v, a, b);
                            if d < min {
                                f.push(Finding::new(
                                    STATUS_DISTANCE,
                                    format!("{} {theme} {s}", v.as_str()),
                                    format!("CIEDE2000 {d:.2} from {c}; at least {min} (min_delta_e2000)"),
                                ));
                            }
                            r.status.push(StatusResult {
                                theme: theme.to_string(),
                                state: s.clone(),
                                other: c.clone(),
                                vision: v,
                                de2000: d,
                                required: min,
                            });
                        }
                    }
                }
            }
        }
        None => f.push(Finding::new(
            CHECK_INPUT,
            crate::status::PALETTE_FILE,
            "no readable min_delta_e2000 and sets; the status distance cannot be checked",
        )),
    }

    // REQ-UX.TOKENS.CHECK-003: stepped palettes (the rules are palettes-check's PAL-LIGHTNESS and
    // PAL-DELTA; the values are reported here).
    for (reference, p) in palettes.colours.iter().filter(|(_, p)| p.kind == Kind::Stepped) {
        let ls: Vec<f64> = p.colors.iter().map(|c| lstar(*c)).collect();
        let n = ls.len();
        let monotonic = if n % 2 == 1 {
            let c = (n - 1) / 2;
            let strict = |xs: &[f64]| xs.windows(2).all(|w| w[1] > w[0]) || xs.windows(2).all(|w| w[1] < w[0]);
            let lower: Vec<f64> = ls[..=c].iter().rev().copied().collect();
            strict(&lower) && strict(&ls[c..])
        } else {
            false
        };
        let mut adjacent = Vec::new();
        for i in 0..n.saturating_sub(1) {
            for v in Vision::ALL {
                adjacent.push((i, v, delta_e2000_under(v, p.colors[i], p.colors[i + 1])));
            }
        }
        r.palettes.push(PaletteResult {
            reference: reference.clone(),
            default: palettes.default.as_deref() == Some(reference.as_str()),
            lstar: ls,
            monotonic,
            adjacent,
        });
    }

    f.sort();
    f.dedup();
    r.findings = f;
    Ok(r)
}

/// `min_delta_e2000` and the conforming and nonconforming sets of `tokens/status.json`.
fn status_sets(root: &Path) -> Option<(f64, Vec<String>, Vec<String>)> {
    let text = crate::finding::read_text(&root.join(crate::status::PALETTE_FILE), 1024 * 1024).ok()?;
    let v = fid_canon::parse_with_limits(text.as_bytes(), 1024 * 1024, fid_design_pkg::MAX_JSON_DEPTH).ok()?;
    let CValue::Obj(m) = v else {
        return None;
    };
    let Some(CValue::Num(min)) = m.get("min_delta_e2000") else {
        return None;
    };
    let Some(CValue::Obj(sets)) = m.get("sets") else {
        return None;
    };
    let list = |k: &str| -> Option<Vec<String>> {
        match sets.get(k) {
            Some(CValue::Arr(xs)) => {
                xs.iter().map(|x| if let CValue::Str(s) = x { Some(s.clone()) } else { None }).collect()
            }
            _ => None,
        }
    };
    if !min.is_finite() || *min <= 0.0 {
        return None;
    }
    Some((*min, list("conforming")?, list("nonconforming")?))
}

// ------------------------------------------------------------------ report

fn num(x: f64) -> CValue {
    CValue::Num(round2(x))
}

fn s(x: &str) -> CValue {
    CValue::Str(x.to_string())
}

fn obj(items: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(items.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let pairs = self
            .pairs
            .iter()
            .map(|p| {
                obj(vec![
                    ("theme", s(&p.theme)),
                    ("fg", s(&p.fg)),
                    ("bg", s(&p.bg)),
                    ("tag", s(&p.tag)),
                    ("ratio", num(p.ratio)),
                    ("required", CValue::Num(p.required)),
                    ("pass", CValue::Bool(p.ratio >= p.required)),
                ])
            })
            .collect();
        let status = self
            .status
            .iter()
            .map(|x| {
                obj(vec![
                    ("theme", s(&x.theme)),
                    ("state", s(&x.state)),
                    ("other", s(&x.other)),
                    ("vision", s(x.vision.as_str())),
                    ("de2000", num(x.de2000)),
                    ("required", CValue::Num(x.required)),
                    ("pass", CValue::Bool(x.de2000 >= x.required)),
                ])
            })
            .collect();
        let palettes = self
            .palettes
            .iter()
            .map(|p| {
                let adjacent = p
                    .adjacent
                    .iter()
                    .map(|(i, v, d)| {
                        obj(vec![
                            ("step", CValue::Num(*i as f64)),
                            ("vision", s(v.as_str())),
                            ("de2000", num(*d)),
                            ("pass", CValue::Bool(*d >= MIN_STEP_DELTA_E2000)),
                        ])
                    })
                    .collect();
                obj(vec![
                    ("ref", s(&p.reference)),
                    ("default", CValue::Bool(p.default)),
                    ("lstar", CValue::Arr(p.lstar.iter().map(|x| num(*x)).collect())),
                    ("lstar_monotonic", CValue::Bool(p.monotonic)),
                    ("adjacent", CValue::Arr(adjacent)),
                    ("required", CValue::Num(MIN_STEP_DELTA_E2000)),
                ])
            })
            .collect();
        let cvd = obj(vec![
            ("model", s(CVD_MODEL)),
            ("table", s(CVD_TABLE)),
            ("table_digest", cvd_table_digest().map_or(CValue::Null, CValue::Str)),
            ("severity", CValue::Num(1.0)),
            ("visions", CValue::Arr(Vision::ALL.iter().map(|v| s(v.as_str())).collect())),
        ]);
        let gates = CValue::Obj(self.gates.iter().map(|(k, n)| ((*k).to_string(), CValue::Num(*n as f64))).collect());
        obj(vec![
            ("format", s(REPORT_FORMAT)),
            ("wcag", s(WCAG)),
            ("formula", s(FORMULA)),
            ("cvd", cvd),
            ("pairs", CValue::Arr(pairs)),
            ("status", CValue::Arr(status)),
            ("palettes", CValue::Arr(palettes)),
            ("gates", gates),
            ("findings", findings_value(&self.findings)),
            ("ok", CValue::Bool(self.findings.is_empty())),
        ])
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let failing_pairs = self.pairs.iter().filter(|p| p.ratio < p.required).count();
        let mut out = format!(
            "fid-design color-check: {} pair results ({} failing), {} status distances, {} stepped palettes; gates: {}\n",
            self.pairs.len(),
            failing_pairs,
            self.status.len(),
            self.palettes.len(),
            self.gates.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ")
        );
        findings_text(&mut out, &self.findings);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_and_alpha() {
        assert_eq!(round2(4.2049), 4.2);
        assert_eq!(round2(21.0), 21.0);
        assert_eq!(parse_rgba("#11223380"), Some(([0x11, 0x22, 0x33], 0x80)));
        assert_eq!(parse_rgba("#112233"), Some(([0x11, 0x22, 0x33], 255)));
        assert_eq!(parse_rgba("#1122338"), None);
        assert_eq!(composite([0, 0, 0], 255, [255, 255, 255]), [0, 0, 0]);
        assert_eq!(composite([0, 0, 0], 0, [255, 255, 255]), [255, 255, 255]);
        assert_eq!(composite([0, 0, 0], 128, [255, 255, 255]), [127, 127, 127]);
        assert!(cvd_table_digest().is_ok_and(|d| d.starts_with("sha256:")));
    }
}

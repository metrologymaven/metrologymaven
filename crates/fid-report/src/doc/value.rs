//! Values inside block payloads: provenance labels and the FMT value-text shape.
//!
//! The document model formats no numbers, but it refuses to store a value that could pass
//! for measured when it is not:
//!
//! - Every block carries a set of [`ValueLabel`]s. A kind computes the labels its payload
//!   requires ([`super::BlockKind::required_labels`], from fid-kunc through
//!   [`ValueLabel::required_by`]) and declares where its values sit
//!   ([`super::BlockKind::value_fields`], RFC 6901 JSON pointers).
//! - Each declared value field must hold a value text in the pinned `value_text@1` shape
//!   that `RPT.MODEL.FMT` produces ([`check_value_text`]); a bare string or number such as
//!   `"0.002"` is refused with `report.value_text_invalid`.
//! - The block's labels must equal the required labels plus every value label found in
//!   its value texts; otherwise `report.value_label_mismatch`. This runs on `add_block`,
//!   on every replayed event and on every stored report read back.
//!
//! `value_text@1` (exact keys; every array strictly ascending, so it is canonical):
//!
//! ```text
//! { "schema": "value_text@1",
//!   "canonical": "-?(0|[1-9][0-9]{0,39})(\.[0-9]{1,40})?",   kernel formatter output, verbatim
//!   "digits_added": "0".."255",                               decimal string
//!   "formatter": "fmt.<name>@<major>",                        e.g. "fmt.length@1"
//!   "unit": [A-Za-z0-9_.:/^-]{1,32},
//!   "markers": [ "nonconformance" | "tie_at_max_precision" | "undecidable_side" ],
//!   "labels": [ {"kind": "claimed" | "declared_input" | "uncharacterized_input"}
//!             | {"kind": "transcribed" | "gauge" | "rule" | "uec", "value": <text>} ] }
//! ```
//!
//! `tie_at_max_precision` and `undecidable_side` require `nonconformance` (FMT-004: when
//! the formatter cannot place a nonconforming value strictly beyond its limit, the side is
//! read from the formatter's result, never from the printed string, and the value is
//! marked). Label `value`s are 1..=256 bytes with no character [`must_escape`] flags;
//! `labels` is ordered by the canonical bytes of each label object.

use super::error::ReportError;
use super::kind::{BlockKind, PayloadError};
use super::model::CanonicalJson;
use super::text::must_escape;
use fid_canon::{to_canonical_bytes, CValue};
use fid_kunc::UncertaintyStatement;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Schema tag of the value-text shape.
pub const VALUE_TEXT_SCHEMA: &str = "value_text@1";
/// Most value fields one block may declare.
pub const MAX_VALUE_FIELDS: usize = 4096;
/// Longest value-field pointer, in bytes.
pub const MAX_POINTER_BYTES: usize = 256;
/// Longest label `value`, in bytes.
pub const MAX_LABEL_VALUE_BYTES: usize = 256;
/// Most labels or markers in one value text.
pub const MAX_VALUE_TEXT_ITEMS: usize = 16;

/// A provenance label a value must carry so it never looks measured when it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ValueLabel {
    /// The value was declared by a person or derived from a specification (fid-kunc
    /// `is_claimed()`).
    Claimed,
    /// The value rests on at least one declared input (`rests_on_declared_input()`).
    DeclaredInput,
    /// The value rests on at least one uncharacterized input
    /// (`rests_on_uncharacterized_input()`).
    UncharacterizedInput,
}

impl ValueLabel {
    pub const ALL: [ValueLabel; 3] = [ValueLabel::Claimed, ValueLabel::DeclaredInput, ValueLabel::UncharacterizedInput];

    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            ValueLabel::Claimed => "claimed",
            ValueLabel::DeclaredInput => "declared_input",
            ValueLabel::UncharacterizedInput => "uncharacterized_input",
        }
    }

    pub fn parse(s: &str) -> Option<ValueLabel> {
        ValueLabel::ALL.into_iter().find(|l| l.as_str() == s)
    }

    /// The labels an uncertainty statement requires of every value that rests on it.
    pub fn required_by(stmt: &UncertaintyStatement) -> BTreeSet<ValueLabel> {
        let mut out = BTreeSet::new();
        if stmt.is_claimed() {
            out.insert(ValueLabel::Claimed);
        }
        if stmt.rests_on_declared_input() {
            out.insert(ValueLabel::DeclaredInput);
        }
        if stmt.rests_on_uncharacterized_input() {
            out.insert(ValueLabel::UncharacterizedInput);
        }
        out
    }
}

impl fmt::Display for ValueLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A label set as a JSON array of wire names, ascending.
pub(crate) fn labels_value(labels: &BTreeSet<ValueLabel>) -> CValue {
    CValue::Arr(labels.iter().map(|l| CValue::Str(l.as_str().to_string())).collect())
}

/// Reads a label array: known names, strictly ascending (so no repeats). `None` otherwise.
pub(crate) fn labels_from_value(v: &CValue) -> Option<BTreeSet<ValueLabel>> {
    let CValue::Arr(items) = v else { return None };
    if items.len() > ValueLabel::ALL.len() {
        return None;
    }
    let mut out = BTreeSet::new();
    let mut last: Option<ValueLabel> = None;
    for item in items {
        let CValue::Str(s) = item else { return None };
        let l = ValueLabel::parse(s)?;
        if last.is_some_and(|p| p >= l) {
            return None;
        }
        last = Some(l);
        out.insert(l);
    }
    Some(out)
}

// ------------------------------------------------------------------ value text

fn is_canonical_decimal(s: &str) -> bool {
    let body = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit());
    let int_ok = digits(int) && int.len() <= 40 && (int == "0" || !int.starts_with('0'));
    let frac_ok = frac.is_none_or(|f| digits(f) && f.len() <= 40);
    int_ok && frac_ok
}

fn is_formatter_id(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("fmt.") else { return false };
    let Some((name, major)) = rest.split_once('@') else { return false };
    let nb = name.as_bytes();
    let name_ok = !nb.is_empty()
        && nb.len() <= 32
        && nb.first().is_some_and(u8::is_ascii_lowercase)
        && nb.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_');
    let major_ok =
        !major.is_empty() && major.len() <= 9 && major.bytes().all(|c| c.is_ascii_digit()) && !major.starts_with('0');
    name_ok && major_ok
}

fn is_unit(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b':' | b'/' | b'^' | b'-'))
}

fn is_digits_added(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 3
        && s.bytes().all(|c| c.is_ascii_digit())
        && (s == "0" || !s.starts_with('0'))
        && s.parse::<u16>().is_ok_and(|n| n <= 255)
}

const MARKERS: [&str; 3] = ["nonconformance", "tie_at_max_precision", "undecidable_side"];
const VALUED_LABELS: [&str; 4] = ["gauge", "rule", "transcribed", "uec"];

fn str_field<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Option<&'a str> {
    match o.get(k) {
        Some(CValue::Str(s)) => Some(s),
        _ => None,
    }
}

/// Checks one `value_text@1` value and returns the [`ValueLabel`]s it carries. The error's
/// reason names the first rule broken; its field is set by the caller.
pub fn check_value_text(v: &CValue) -> Result<BTreeSet<ValueLabel>, PayloadError> {
    let bad = |reason: &'static str| PayloadError::new("", reason);
    let CValue::Obj(o) = v else { return Err(bad("not_value_text")) };
    const KEYS: [&str; 7] = ["canonical", "digits_added", "formatter", "labels", "markers", "schema", "unit"];
    if o.len() != KEYS.len() || KEYS.iter().any(|k| !o.contains_key(*k)) {
        return Err(bad("value_text_keys"));
    }
    if str_field(o, "schema") != Some(VALUE_TEXT_SCHEMA) {
        return Err(bad("value_text_schema"));
    }
    if !str_field(o, "canonical").is_some_and(is_canonical_decimal) {
        return Err(bad("canonical"));
    }
    if !str_field(o, "digits_added").is_some_and(is_digits_added) {
        return Err(bad("digits_added"));
    }
    if !str_field(o, "formatter").is_some_and(is_formatter_id) {
        return Err(bad("formatter"));
    }
    if !str_field(o, "unit").is_some_and(is_unit) {
        return Err(bad("unit"));
    }
    // Markers: known, strictly ascending; a tie or undecidable side needs `nonconformance`.
    let Some(CValue::Arr(markers)) = o.get("markers") else { return Err(bad("markers")) };
    if markers.len() > MAX_VALUE_TEXT_ITEMS {
        return Err(bad("markers"));
    }
    let mut seen: Vec<&str> = Vec::new();
    for m in markers {
        let CValue::Str(s) = m else { return Err(bad("markers")) };
        if !MARKERS.contains(&s.as_str()) || seen.last().is_some_and(|p| *p >= s.as_str()) {
            return Err(bad("markers"));
        }
        seen.push(s);
    }
    if seen.iter().any(|m| *m != "nonconformance") && !seen.contains(&"nonconformance") {
        return Err(bad("marker_without_nonconformance"));
    }
    // Labels: closed kinds, ordered by canonical bytes.
    let Some(CValue::Arr(labels)) = o.get("labels") else { return Err(bad("labels")) };
    if labels.len() > MAX_VALUE_TEXT_ITEMS {
        return Err(bad("labels"));
    }
    let mut out = BTreeSet::new();
    let mut last: Option<Vec<u8>> = None;
    for l in labels {
        let CValue::Obj(lo) = l else { return Err(bad("labels")) };
        let kind = str_field(lo, "kind").ok_or_else(|| bad("labels"))?;
        if let Some(vl) = ValueLabel::parse(kind) {
            if lo.len() != 1 {
                return Err(bad("labels"));
            }
            out.insert(vl);
        } else if VALUED_LABELS.contains(&kind) {
            let ok = lo.len() == 2
                && str_field(lo, "value")
                    .is_some_and(|t| !t.is_empty() && t.len() <= MAX_LABEL_VALUE_BYTES && !t.chars().any(must_escape));
            if !ok {
                return Err(bad("labels"));
            }
        } else {
            return Err(bad("labels"));
        }
        let bytes = to_canonical_bytes(l).map_err(|_| bad("labels"))?;
        if last.as_ref().is_some_and(|p| *p >= bytes) {
            return Err(bad("labels"));
        }
        last = Some(bytes);
    }
    Ok(out)
}

/// RFC 6901 JSON pointer lookup (`""` is the whole value; `~0` is `~`, `~1` is `/`).
pub fn resolve_pointer<'a>(root: &'a CValue, pointer: &str) -> Option<&'a CValue> {
    if pointer.is_empty() {
        return Some(root);
    }
    let rest = pointer.strip_prefix('/')?;
    let mut at = root;
    for raw in rest.split('/') {
        if raw.contains('~') {
            let mut chars = raw.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '~' && !matches!(chars.peek(), Some('0' | '1')) {
                    return None;
                }
            }
        }
        let token = raw.replace("~1", "/").replace("~0", "~");
        at = match at {
            CValue::Obj(o) => o.get(&token)?,
            CValue::Arr(a) => {
                let canonical = token == "0" || (!token.starts_with('0') && token.bytes().all(|c| c.is_ascii_digit()));
                if !canonical || token.is_empty() {
                    return None;
                }
                a.get(token.parse::<usize>().ok()?)?
            }
            _ => return None,
        };
    }
    Some(at)
}

/// The value rule for one block: every declared value field holds a `value_text@1`, and
/// `labels` equals the kind's required labels plus every value label in those texts.
pub(crate) fn check_block_values(
    kind: &dyn BlockKind,
    payload: &CanonicalJson,
    labels: &BTreeSet<ValueLabel>,
) -> Result<(), ReportError> {
    let fields = kind.value_fields(payload).map_err(ReportError::InvalidBlockPayload)?;
    if fields.len() > MAX_VALUE_FIELDS {
        return Err(ReportError::InvalidBlockPayload(PayloadError::new("", "too_many_value_fields")));
    }
    let mut want = kind.required_labels(payload).map_err(ReportError::InvalidBlockPayload)?;
    for f in &fields {
        if f.len() > MAX_POINTER_BYTES {
            return Err(ReportError::ValueTextInvalid(PayloadError::new(f, "pointer_too_long")));
        }
        let v = resolve_pointer(payload.value(), f)
            .ok_or_else(|| ReportError::ValueTextInvalid(PayloadError::new(f, "missing")))?;
        let found = check_value_text(v).map_err(|e| ReportError::ValueTextInvalid(PayloadError::new(f, e.reason())))?;
        want.extend(found);
    }
    if want != *labels {
        let missing = want.difference(labels).copied().collect();
        let extra = labels.difference(&want).copied().collect();
        return Err(ReportError::ValueLabelMismatch { missing, extra });
    }
    Ok(())
}

//! What the scalar reading packages `Gauge@1` ([`crate::caps::gauge`]) and `Environment@1`
//! ([`crate::caps::environment`]) share (spec node `HAL.IDL.SENS.GAUENV`): the error type with
//! the leaf's codes, the [`Reading`] trait, the reading batch wire form and its series rules.
//!
//! **Wire forms.** RFC 8785 canonical JSON through `fid-canon`, with closed key sets: quantities
//! as `{"unit", "value"}` with the value's IEEE 754 bits (`HAL.IDL.UNITS`), timestamps as
//! decimal `i64` strings in ns and sequence numbers as decimal `u64` strings. A reading or batch
//! whose bytes are not the canonical form of what they say is refused (`reading_invalid`,
//! `not_canonical`), so the bytes that were digested or sized are the bytes [`reading_to_json`]
//! and [`ReadingBatch::to_json`] write.
//!
//! ```text
//! {"schema": "<batch tag>", "first_seq": "<u64>", "last_seq": "<u64>", "readings": [<reading>, ...]}
//! ```
//!
//! One sequence number per reading, so `last_seq = first_seq + readings - 1`.
//!
//! **No origin.** Neither a reading nor a batch has an origin field: provenance is the
//! acquisition-batch envelope's job (`HAL.IDL.STAMP`), and a file cannot show that a device
//! produced it (D-045). An `origin` key is `field_unknown`. Stream records built from batch
//! summaries go through [`crate::caps::stream::BatchRecord::new`], which refuses `device`.

use crate::base::config::ConfigError;
use crate::caps::stream::{BatchSummary, StreamError};
use crate::units::{kept, Quantity, SiUnit, Timestamp, UnitsError};
use crate::UntrustedText;
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Longest gauge or sensor identity, in bytes; an empty identity is refused too.
pub const MAX_IDENTITY_BYTES: usize = 256;
/// Largest accepted reading JSON document, in bytes.
pub const MAX_READING_BYTES: usize = 4096;
/// Deepest accepted nesting of a reading (the reading and one quantity).
const MAX_READING_DEPTH: usize = 3;
/// Most JSON elements (values and keys) in one reading.
const MAX_READING_ELEMENTS: usize = 64;
/// Most readings in one batch.
pub const MAX_BATCH_READINGS: usize = 50_000;
/// Largest accepted batch JSON document (16 MiB).
pub const MAX_BATCH_BYTES: usize = 16 * 1024 * 1024;
/// Deepest accepted nesting of a batch (batch, readings, reading, quantity, one spare).
const MAX_BATCH_DEPTH: usize = 5;
/// Most JSON elements in one batch.
const MAX_BATCH_ELEMENTS: usize = 16 + MAX_READING_ELEMENTS * MAX_BATCH_READINGS;

// ------------------------------------------------------------------ errors

/// A failure of a reading, a batch, a setting or the package (stable codes from
/// [`Self::code`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadingError {
    /// `reading_not_quantised`: a gauge value that is not an integer multiple of its
    /// resolution within 1e-9 relative.
    NotQuantised,
    /// `value_out_of_range`: a quantity outside its domain (a humidity outside [0, 1], a
    /// non-positive resolution, temperature, uncertainty, pressure or sampling time, a
    /// non-finite value, a value outside the gauge's declared range, or more than 2^53
    /// resolution steps).
    OutOfRange { field: &'static str, reason: &'static str },
    /// `unit_unsupported`: a unit that is not SI, or not the unit the field requires.
    UnitUnsupported { field: &'static str, unit: String },
    /// `timestamps_not_monotonic`: `t_host` decreases, or `t_dev` does not increase, at
    /// reading `index`.
    NotMonotonic { field: &'static str, index: usize },
    /// `reading_duplicate`: reading `index` repeats an earlier reading of the series exactly,
    /// host time included.
    Duplicate { index: usize },
    /// `reading_inconsistent`: reading `index` differs from the first reading of its stream
    /// (or from the gauge's declared info, or from the configured sampling time) in a field
    /// that cannot change within a stream: identity, unit, resolution, calibration reference,
    /// the presence of `t_dev`, humidity or pressure, or the sampling time.
    Inconsistent { field: &'static str, index: usize },
    /// `reading_invalid`: a reading or batch that is not the closed wire shape, or not its
    /// canonical bytes; `pointer` is an RFC 6901 JSON pointer.
    Invalid { pointer: String, reason: &'static str },
    /// `settings_invalid`: a core setting of the wrong type or not digested, a value the
    /// package forbids, or a description that is not the expected package.
    Settings { setting: String, reason: &'static str },
    /// The configuration does not validate against its schema (`CONFIG_INVALID`,
    /// `OUT_OF_RANGE`).
    Config(ConfigError),
    /// The streaming template or the vendor-extension rule failed.
    Stream(StreamError),
}

impl ReadingError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            ReadingError::NotQuantised => "reading_not_quantised",
            ReadingError::OutOfRange { .. } => "value_out_of_range",
            ReadingError::UnitUnsupported { .. } => "unit_unsupported",
            ReadingError::NotMonotonic { .. } => "timestamps_not_monotonic",
            ReadingError::Duplicate { .. } => "reading_duplicate",
            ReadingError::Inconsistent { .. } => "reading_inconsistent",
            ReadingError::Invalid { .. } => "reading_invalid",
            ReadingError::Settings { .. } => "settings_invalid",
            ReadingError::Config(e) => e.code(),
            ReadingError::Stream(e) => e.code(),
        }
    }

    pub(crate) fn invalid(pointer: impl Into<String>, reason: &'static str) -> ReadingError {
        ReadingError::Invalid { pointer: pointer.into(), reason }
    }

    pub(crate) fn settings(setting: &str, reason: &'static str) -> ReadingError {
        ReadingError::Settings { setting: kept(setting), reason }
    }

    pub(crate) fn range(field: &'static str, reason: &'static str) -> ReadingError {
        ReadingError::OutOfRange { field, reason }
    }

    pub(crate) fn unit(field: &'static str, unit: SiUnit) -> ReadingError {
        ReadingError::UnitUnsupported { field, unit: unit.as_str().to_string() }
    }
}

impl fmt::Display for ReadingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadingError::NotQuantised => f.write_str("reading_not_quantised"),
            ReadingError::OutOfRange { field, reason } => write!(f, "value_out_of_range: {field}: {reason}"),
            ReadingError::UnitUnsupported { field, unit } => write!(f, "unit_unsupported: {field}: {unit:?}"),
            ReadingError::NotMonotonic { field, index } => {
                write!(f, "timestamps_not_monotonic: {field} at reading {index}")
            }
            ReadingError::Duplicate { index } => write!(f, "reading_duplicate: reading {index}"),
            ReadingError::Inconsistent { field, index } => {
                write!(f, "reading_inconsistent: {field} at reading {index}")
            }
            ReadingError::Invalid { pointer, reason } => write!(f, "reading_invalid{{{pointer}}}: {reason}"),
            ReadingError::Settings { setting, reason } => write!(f, "settings_invalid: {setting:?}: {reason}"),
            ReadingError::Config(e) => write!(f, "{e}"),
            ReadingError::Stream(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ReadingError {}

// ------------------------------------------------------------------ the trait

/// One scalar reading type: a `GaugeReading` or an `EnvReading`.
pub trait Reading: Sized + Clone + PartialEq {
    /// The `schema` tag of one reading, e.g. `gauge-reading/1`.
    const TAG: &'static str;
    /// The `schema` tag of a batch of readings, e.g. `gauge-batch/1`.
    const BATCH_TAG: &'static str;

    /// The wire value (unchecked; [`reading_to_json`] validates first).
    fn to_value(&self) -> CValue;

    /// Reads the wire shape at JSON pointer `ptr`; the package rules are
    /// [`Reading::validate`]'s.
    fn from_value(v: &CValue, ptr: &str) -> Result<Self, ReadingError>;

    /// The package rules on one reading.
    fn validate(&self) -> Result<(), ReadingError>;

    /// The reading's timestamps.
    fn timestamp(&self) -> Timestamp;

    /// The fields that cannot change within a stream, compared with the stream's `first`
    /// reading (`reading_inconsistent` at `index`).
    fn check_same_stream(&self, first: &Self, index: usize) -> Result<(), ReadingError>;
}

/// The RFC 8785 bytes of a reading (validates it first).
pub fn reading_to_json<R: Reading>(r: &R) -> Result<Vec<u8>, ReadingError> {
    r.validate()?;
    canonical(&r.to_value())
}

/// Reads and checks a reading from its wire bytes, in order: the JSON within
/// [`MAX_READING_BYTES`] and a fixed depth and element budget; the wire shape; the package
/// rules; and last, the bytes equal to [`reading_to_json`] of the result (`not_canonical`).
pub fn reading_from_json<R: Reading>(bytes: &[u8]) -> Result<R, ReadingError> {
    let v = fid_canon::parse_with_budget(bytes, MAX_READING_BYTES, MAX_READING_DEPTH, MAX_READING_ELEMENTS)
        .map_err(|_| ReadingError::invalid("", "json"))?;
    let r = R::from_value(&v, "")?;
    if reading_to_json(&r)? != bytes {
        return Err(ReadingError::invalid("", "not_canonical"));
    }
    Ok(r)
}

/// The series rules over readings in arrival order, after each reading passed
/// [`Reading::validate`]. For every reading after the first, in order: `t_dev` present in
/// all readings or in none (`reading_inconsistent`, field `t_dev`); `t_host` never decreasing
/// and `t_dev` strictly increasing (`timestamps_not_monotonic`); the fields of
/// [`Reading::check_same_stream`] unchanged (`reading_inconsistent`); and no exact repeat of
/// an earlier reading (`reading_duplicate`). Readings received together may share a host time;
/// a reading equal in every field, host time included, to one before it is a repeat.
pub fn check_series<R: Reading>(readings: &[R]) -> Result<(), ReadingError> {
    let Some(first) = readings.first() else {
        return Ok(());
    };
    let has_dev = first.timestamp().t_dev.is_some();
    // Readings sharing the host time of the current run, by canonical bytes. A repeat has the
    // same host time, and host time never decreases, so only the current run is searched.
    let mut run: BTreeSet<Vec<u8>> = BTreeSet::new();
    run.insert(canonical(&first.to_value())?);
    for (i, pair) in readings.windows(2).enumerate() {
        let index = i + 1;
        let (prev, r) = (&pair[0], &pair[1]);
        let (tp, t) = (prev.timestamp(), r.timestamp());
        if t.t_dev.is_some() != has_dev {
            return Err(ReadingError::Inconsistent { field: "t_dev", index });
        }
        if t.t_host < tp.t_host {
            return Err(ReadingError::NotMonotonic { field: "t_host", index });
        }
        if let (Some(a), Some(b)) = (tp.t_dev, t.t_dev) {
            if b <= a {
                return Err(ReadingError::NotMonotonic { field: "t_dev", index });
            }
        }
        r.check_same_stream(first, index)?;
        if t.t_host != tp.t_host {
            run.clear();
        }
        if !run.insert(canonical(&r.to_value())?) {
            return Err(ReadingError::Duplicate { index });
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ the batch

/// One batch of readings from one stream: one sequence number per reading.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadingBatch<R> {
    /// Sequence number of the first reading; reading `k` has `first_seq + k`.
    pub first_seq: u64,
    /// The readings, in arrival order.
    pub readings: Vec<R>,
}

const BATCH_FIELDS: [&str; 4] = ["first_seq", "last_seq", "readings", "schema"];

impl<R: Reading> ReadingBatch<R> {
    /// Sequence number of the last reading.
    pub fn last_seq(&self) -> Option<u64> {
        let n = u64::try_from(self.readings.len()).ok()?;
        self.first_seq.checked_add(n.checked_sub(1)?)
    }

    /// Checks the batch, in order: 1 to [`MAX_BATCH_READINGS`] readings and a sequence range
    /// that fits in `u64` (`reading_invalid`); every reading's package rules; then
    /// [`check_series`].
    pub fn validate(&self) -> Result<(), ReadingError> {
        if self.readings.is_empty() || self.readings.len() > MAX_BATCH_READINGS {
            return Err(ReadingError::invalid("/readings", "count_out_of_range"));
        }
        if self.last_seq().is_none() {
            return Err(ReadingError::invalid("/last_seq", "seq_overflow"));
        }
        for r in &self.readings {
            r.validate()?;
        }
        check_series(&self.readings)
    }

    /// The RFC 8785 wire bytes (validates the batch first).
    pub fn to_json(&self) -> Result<Vec<u8>, ReadingError> {
        self.validate()?;
        self.canonical_bytes()
    }

    fn canonical_bytes(&self) -> Result<Vec<u8>, ReadingError> {
        let v = obj(vec![
            ("first_seq", dec(self.first_seq)),
            ("last_seq", dec(self.last_seq().unwrap_or(u64::MAX))),
            ("readings", CValue::Arr(self.readings.iter().map(Reading::to_value).collect())),
            ("schema", CValue::Str(R::BATCH_TAG.to_string())),
        ]);
        canonical(&v)
    }

    /// Reads and checks a batch from its wire bytes, in order: the JSON within
    /// [`MAX_BATCH_BYTES`] and a fixed depth and element budget; the closed batch shape, the
    /// tag and the sequence numbers; 1 to [`MAX_BATCH_READINGS`] readings; the sequence range
    /// (`range_reversed`, then `seq_range_mismatch` unless it holds one number per reading);
    /// each reading's wire shape; [`ReadingBatch::validate`]; and last, the bytes equal to
    /// [`ReadingBatch::to_json`] of the result (`not_canonical`).
    pub fn from_json(bytes: &[u8]) -> Result<ReadingBatch<R>, ReadingError> {
        let v = fid_canon::parse_with_budget(bytes, MAX_BATCH_BYTES, MAX_BATCH_DEPTH, MAX_BATCH_ELEMENTS)
            .map_err(|_| ReadingError::invalid("", "json"))?;
        let o = Fields::new(&v, "", &BATCH_FIELDS)?;
        if o.str("schema")? != R::BATCH_TAG {
            return Err(ReadingError::invalid("/schema", "schema_unknown"));
        }
        let first_seq = o.u64("first_seq")?;
        let last_seq = o.u64("last_seq")?;
        let items = match o.get("readings") {
            Some(CValue::Arr(items)) => items,
            Some(_) => return Err(ReadingError::invalid("/readings", "wrong_type")),
            None => return Err(ReadingError::invalid("/readings", "field_missing")),
        };
        if items.is_empty() || items.len() > MAX_BATCH_READINGS {
            return Err(ReadingError::invalid("/readings", "count_out_of_range"));
        }
        let span =
            last_seq.checked_sub(first_seq).ok_or_else(|| ReadingError::invalid("/last_seq", "range_reversed"))?;
        if u64::try_from(items.len() - 1).ok() != Some(span) {
            return Err(ReadingError::invalid("/last_seq", "seq_range_mismatch"));
        }
        let mut readings = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            readings.push(R::from_value(item, &format!("/readings/{i}"))?);
        }
        let batch = ReadingBatch { first_seq, readings };
        batch.validate()?;
        if batch.canonical_bytes()? != bytes {
            return Err(ReadingError::invalid("", "not_canonical"));
        }
        Ok(batch)
    }

    /// The `Stream-common@1` summary of this batch: its sequence range, its serialised size
    /// and, with device time, the span from its first to its last reading.
    pub fn summary(&self) -> Result<BatchSummary, ReadingError> {
        let bytes = self.to_json()?;
        let span_ns = match (self.readings.first(), self.readings.last()) {
            (Some(a), Some(b)) => match (a.timestamp().t_dev, b.timestamp().t_dev) {
                (Some(x), Some(y)) => u64::try_from(i128::from(y) - i128::from(x)).ok(),
                _ => None,
            },
            _ => None,
        };
        Ok(BatchSummary {
            first_seq: self.first_seq,
            last_seq: self.last_seq().unwrap_or(u64::MAX),
            bytes: bytes.len() as u64,
            span_ns,
        })
    }
}

// ------------------------------------------------------------------ wire helpers

pub(crate) fn canonical(v: &CValue) -> Result<Vec<u8>, ReadingError> {
    fid_canon::to_canonical_bytes(v).map_err(|_| ReadingError::invalid("", "json"))
}

pub(crate) fn dec(n: u64) -> CValue {
    CValue::Str(n.to_string())
}

pub(crate) fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

pub(crate) fn obj(pairs: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn escape(seg: &str) -> String {
    kept(seg).replace('~', "~0").replace('/', "~1")
}

/// Checks an identity: 1 to [`MAX_IDENTITY_BYTES`] bytes.
pub(crate) fn check_identity(field: &'static str, t: &UntrustedText) -> Result<(), ReadingError> {
    if t.len() == 0 || t.len() > MAX_IDENTITY_BYTES {
        return Err(ReadingError::invalid(format!("/{field}"), "identity_invalid"));
    }
    Ok(())
}

/// A closed wire object being read, with the JSON pointer of the object.
pub(crate) struct Fields<'a> {
    o: &'a BTreeMap<String, CValue>,
    ptr: &'a str,
}

impl<'a> Fields<'a> {
    pub(crate) fn new(v: &'a CValue, ptr: &'a str, allowed: &[&str]) -> Result<Fields<'a>, ReadingError> {
        let CValue::Obj(o) = v else {
            return Err(ReadingError::invalid(ptr, "not_object"));
        };
        if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(ReadingError::invalid(format!("{ptr}/{}", escape(k)), "field_unknown"));
        }
        Ok(Fields { o, ptr })
    }

    fn at(&self, k: &str) -> String {
        format!("{}/{k}", self.ptr)
    }

    pub(crate) fn get(&self, k: &str) -> Option<&'a CValue> {
        self.o.get(k)
    }

    pub(crate) fn str(&self, k: &str) -> Result<&'a str, ReadingError> {
        match self.o.get(k) {
            Some(CValue::Str(s)) => Ok(s),
            Some(_) => Err(ReadingError::invalid(self.at(k), "wrong_type")),
            None => Err(ReadingError::invalid(self.at(k), "field_missing")),
        }
    }

    /// Checks the `schema` tag.
    pub(crate) fn tag(&self, tag: &str) -> Result<(), ReadingError> {
        if self.str("schema")? != tag {
            return Err(ReadingError::invalid(self.at("schema"), "schema_unknown"));
        }
        Ok(())
    }

    /// A `u64` as a canonical decimal string: digits only, no leading zero, at most 20 digits.
    pub(crate) fn u64(&self, k: &str) -> Result<u64, ReadingError> {
        let s = self.str(k)?;
        let canonical = !s.is_empty()
            && s.len() <= 20
            && s.bytes().all(|c| c.is_ascii_digit())
            && (s == "0" || !s.starts_with('0'));
        if !canonical {
            return Err(ReadingError::invalid(self.at(k), "not_u64"));
        }
        s.parse::<u64>().map_err(|_| ReadingError::invalid(self.at(k), "not_u64"))
    }

    /// An `i64` as a canonical decimal string (no `+`, no `-0`, no leading zero).
    fn i64_at(&self, k: &str, s: &str) -> Result<i64, ReadingError> {
        let digits = s.strip_prefix('-').unwrap_or(s);
        let canonical = !digits.is_empty()
            && digits.bytes().all(|c| c.is_ascii_digit())
            && (digits == "0" || !digits.starts_with('0'))
            && s != "-0";
        if !canonical {
            return Err(ReadingError::invalid(self.at(k), "not_i64"));
        }
        s.parse::<i64>().map_err(|_| ReadingError::invalid(self.at(k), "not_i64"))
    }

    pub(crate) fn i64(&self, k: &str) -> Result<i64, ReadingError> {
        let s = self.str(k)?;
        self.i64_at(k, s)
    }

    pub(crate) fn opt_i64(&self, k: &str) -> Result<Option<i64>, ReadingError> {
        if self.o.contains_key(k) {
            self.i64(k).map(Some)
        } else {
            Ok(None)
        }
    }

    /// A quantity: a unit that is not SI is `unit_unsupported`, a NaN or infinite value
    /// `value_out_of_range`, anything else malformed `reading_invalid`.
    pub(crate) fn quantity(&self, k: &'static str) -> Result<Quantity, ReadingError> {
        let v = self.o.get(k).ok_or_else(|| ReadingError::invalid(self.at(k), "field_missing"))?;
        Quantity::from_value(v.clone()).map_err(|e| match e {
            UnitsError::UnitUnsupported(u) => ReadingError::UnitUnsupported { field: k, unit: u },
            UnitsError::QuantityNotFinite => ReadingError::range(k, "not_finite"),
            _ => ReadingError::invalid(self.at(k), "quantity_invalid"),
        })
    }

    pub(crate) fn opt_quantity(&self, k: &'static str) -> Result<Option<Quantity>, ReadingError> {
        if self.o.contains_key(k) {
            self.quantity(k).map(Some)
        } else {
            Ok(None)
        }
    }

    /// An identity string, 1 to [`MAX_IDENTITY_BYTES`] bytes (`identity_invalid`).
    pub(crate) fn identity(&self, k: &'static str) -> Result<UntrustedText, ReadingError> {
        let s = self.str(k)?;
        if s.is_empty() || s.len() > MAX_IDENTITY_BYTES {
            return Err(ReadingError::invalid(self.at(k), "identity_invalid"));
        }
        Ok(UntrustedText::new(s))
    }

    /// An optional `sha256:` digest (`digest_invalid`).
    pub(crate) fn opt_digest(&self, k: &str) -> Result<Option<Digest>, ReadingError> {
        if !self.o.contains_key(k) {
            return Ok(None);
        }
        let s = self.str(k)?;
        Digest::parse(s).map(Some).map_err(|_| ReadingError::invalid(self.at(k), "digest_invalid"))
    }
}

/// The wire value of a timestamp's fields, merged into a reading's key list.
pub(crate) fn time_pairs(t: Timestamp) -> Vec<(&'static str, CValue)> {
    let mut out = vec![("t_host", CValue::I64(t.t_host))];
    if let Some(d) = t.t_dev {
        out.push(("t_dev", CValue::I64(d)));
    }
    out
}

/// The wire value of an untrusted text (for writing only).
pub(crate) fn untrusted(t: &UntrustedText) -> CValue {
    CValue::Str(t.raw().to_string())
}

// ------------------------------------------------------------------ type-schema helpers

pub(crate) fn string_schema(max_len: usize) -> CValue {
    // Small exact integers only; f64 holds them exactly.
    obj(vec![("maxLength", CValue::Num(max_len as f64)), ("type", text("string"))])
}

pub(crate) fn def_ref(name: &str) -> CValue {
    obj(vec![("$ref", CValue::Str(format!("#/$defs/{name}")))])
}

pub(crate) fn enumeration(values: &[&str]) -> CValue {
    obj(vec![("enum", CValue::Arr(values.iter().map(|v| text(v)).collect())), ("type", text("string"))])
}

/// A closed object schema.
pub(crate) fn closed(props: Vec<(&str, CValue)>, required: &[&str]) -> CValue {
    obj(vec![
        ("additionalProperties", CValue::Bool(false)),
        ("properties", obj(props)),
        ("required", CValue::Arr(required.iter().map(|s| text(s)).collect())),
        ("type", text("object")),
    ])
}

/// The `Quantity` and `SiUnit` type schemas: `{"unit", "value"}` with the value's IEEE 754
/// bits as `"0x"` + 16 hex digits.
pub(crate) fn quantity_defs() -> Vec<(String, CValue)> {
    let units: Vec<&str> = SiUnit::ALL.iter().map(|u| u.as_str()).collect();
    vec![
        (
            "Quantity".to_string(),
            closed(vec![("unit", def_ref("SiUnit")), ("value", string_schema(18))], &["unit", "value"]),
        ),
        ("SiUnit".to_string(), enumeration(&units)),
    ]
}

/// Decimal `i64` and `u64` strings are at most 20 characters.
pub(crate) const INT_STRING_LEN: usize = 20;
/// A `sha256:` digest is 71 characters.
pub(crate) const DIGEST_STRING_LEN: usize = 71;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn fields(pairs: &[(&str, &str)]) -> CValue {
        CValue::Obj(pairs.iter().map(|(k, v)| (k.to_string(), CValue::Str(v.to_string()))).collect())
    }

    #[test]
    fn integers_are_canonical() {
        let v = fields(&[("a", "0"), ("b", "-9223372036854775808"), ("c", "18446744073709551615")]);
        let f = Fields::new(&v, "", &["a", "b", "c"]).unwrap();
        assert_eq!(f.i64("a"), Ok(0));
        assert_eq!(f.i64("b"), Ok(i64::MIN));
        assert_eq!(f.u64("c"), Ok(u64::MAX));
        for bad in ["", "-0", "01", "+1", "1.0", " 1", "9223372036854775808"] {
            let v = fields(&[("t", bad)]);
            assert_eq!(Fields::new(&v, "", &["t"]).unwrap().i64("t").unwrap_err().code(), "reading_invalid", "{bad}");
        }
        for bad in ["", "-1", "01", "18446744073709551616"] {
            let v = fields(&[("t", bad)]);
            assert_eq!(Fields::new(&v, "", &["t"]).unwrap().u64("t").unwrap_err().code(), "reading_invalid", "{bad}");
        }
    }

    #[test]
    fn unknown_keys_are_refused() {
        let v = fields(&[("origin", "device")]);
        assert_eq!(
            Fields::new(&v, "/readings/0", &["a"]).err(),
            Some(ReadingError::invalid("/readings/0/origin", "field_unknown"))
        );
    }
}

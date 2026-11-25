//! The `Gauge@1` package for digital hand gauges and indicators (spec node
//! `HAL.IDL.SENS.GAUENV`): `read` and `zero`, the [`GaugeReading`] streamed through the shared
//! template, the [`GaugeInfo`] a driver reports in describe, and the `Gauge-core@1` profile.
//!
//! - **REQ-HAL.IDL.SENS.GAUENV-001:** [`GaugeReading`] carries the value as an SI
//!   [`Quantity`], the resolution as a quantity of the same unit, the gauge identity as
//!   [`UntrustedText`], an optional calibration reference ([`Digest`]), an optional `t_dev` and
//!   a required `t_host` (both i64 ns).
//! - **REQ-HAL.IDL.SENS.GAUENV-002:** [`GaugeReading::validate`] fails with
//!   `reading_not_quantised` unless `|v/r - round(v/r)| <= 1e-9 * max(1, |v/r|)` for value `v`
//!   and resolution `r` (one division, one rounding, no FMA).
//! - **REQ-HAL.IDL.SENS.GAUENV-004:** [`GaugeOperation`] holds `read` and `zero`, both D1; the
//!   built package adds only the template's stream operations, so no operation writes
//!   calibration. A zero that changes the calibration reference fails `GA-03`.
//! - **REQ-HAL.IDL.SENS.GAUENV-005:** [`GAUGE_CORE`] lists `GA-01` (quantisation), `GA-02`
//!   (unit consistency), `GA-03` (zero behaviour) and `GA-04` (timestamp monotonicity); the
//!   built package adds the `Stream-common@1` items. [`check_readings`] evaluates `GA-01`,
//!   `GA-02` and `GA-04` on a recorded stream and [`check_zero`] evaluates `GA-03`.
//!
//! **Wire form** of a reading (RFC 8785, closed key set, see [`crate::caps::reading`]):
//!
//! ```text
//! {"schema": "gauge-reading/1", "value": <Quantity>, "resolution": <Quantity>,
//!  "gauge_identity": "<1..256 bytes>", "calibration_ref"?: "sha256:<hex>",
//!  "t_dev"?: "<i64 ns>", "t_host": "<i64 ns>"}
//! ```
//!
//! Batches are `gauge-batch/1` [`ReadingBatch`]es. Values are m (lengths) or rad (angles).

use crate::base::config::{ConfigSchema, SettingSchema};
use crate::base::device::{ConformanceProfile, ProfileItem};
use crate::base::error::ErrorCode;
use crate::caps::reading::{
    check_identity, check_series, closed, def_ref, enumeration, obj, quantity_defs, string_schema, text, time_pairs,
    untrusted, Fields, Reading, ReadingBatch, ReadingError, DIGEST_STRING_LEN, INT_STRING_LEN, MAX_IDENTITY_BYTES,
};
use crate::caps::stream::{ItemFailure, StreamError, StreamingDescription, StreamingPackage};
use crate::package::{BatchColumnDesc, BatchSchemaDesc, CapabilityPackage, OperationDesc, PackageDescription};
use crate::package::{PackageManifest, SemanticsNote};
use crate::units::{EncodingDescriptor, NumericType, Quantity, SiUnit, Timestamp};
use crate::UntrustedText;
use fid_canon::CValue;
use fid_types::{Digest, SideEffect};
use std::collections::BTreeMap;

/// Package name.
pub const PACKAGE_NAME: &str = "Gauge";
/// Package version.
pub const PACKAGE_VERSION: &str = "1.0.0";
/// ID of the conformance profile.
pub const PROFILE_ID: &str = "Gauge-core@1";
/// ID of the package's batch schema.
pub const BATCH_SCHEMA_ID: &str = "gauge.readings";
/// Wire tag of one reading.
pub const READING_TAG: &str = "gauge-reading/1";
/// Wire tag of a batch of readings.
pub const BATCH_TAG: &str = "gauge-batch/1";

/// Item `GA-01`: quantisation.
pub const ITEM_QUANTISATION: &str = "GA-01";
/// Item `GA-02`: unit consistency.
pub const ITEM_UNITS: &str = "GA-02";
/// Item `GA-03`: zero behaviour.
pub const ITEM_ZERO: &str = "GA-03";
/// Item `GA-04`: timestamp monotonicity.
pub const ITEM_TIMESTAMPS: &str = "GA-04";

/// Relative quantisation tolerance (REQ-002).
pub const QUANTISATION_TOLERANCE: f64 = 1e-9;
/// Most resolution steps a value may hold (2^53): beyond it every value would look quantised.
pub const MAX_STEPS: f64 = 9_007_199_254_740_992.0;

/// A batch of gauge readings.
pub type GaugeBatch = ReadingBatch<GaugeReading>;

// ------------------------------------------------------------------ the reading

/// One gauge reading (REQ-001).
#[derive(Debug, Clone, PartialEq)]
pub struct GaugeReading {
    /// The value, in m or rad.
    pub value: Quantity,
    /// The display resolution, in the value's unit.
    pub resolution: Quantity,
    /// The gauge's identity as it reports it (make, model, serial); data, never instructions.
    pub gauge_identity: UntrustedText,
    /// Digest of the gauge's calibration record, when the device or driver knows it.
    pub calibration_ref: Option<Digest>,
    /// Device time, in ns, when the gauge has a clock.
    pub t_dev: Option<i64>,
    /// Host time, in ns.
    pub t_host: i64,
}

const READING_FIELDS: [&str; 7] =
    ["calibration_ref", "gauge_identity", "resolution", "schema", "t_dev", "t_host", "value"];

impl GaugeReading {
    /// The number of resolution steps the value holds, rounded to the nearest integer.
    pub fn steps(&self) -> f64 {
        (self.value.value() / self.resolution.value()).round()
    }

    /// Checks the reading, in order: the identity is 1 to 256 bytes (`reading_invalid`); the
    /// value is in m or rad and the resolution in the same unit (`unit_unsupported`); the
    /// resolution is positive and the value holds at most [`MAX_STEPS`] steps
    /// (`value_out_of_range`); the value is quantised (`reading_not_quantised`).
    pub fn validate(&self) -> Result<(), ReadingError> {
        check_identity("gauge_identity", &self.gauge_identity)?;
        let unit = self.value.unit();
        if !matches!(unit, SiUnit::Metre | SiUnit::Radian) {
            return Err(ReadingError::unit("value", unit));
        }
        if self.resolution.unit() != unit {
            return Err(ReadingError::unit("resolution", self.resolution.unit()));
        }
        check_quantised(self.value.value(), self.resolution.value())
    }

    /// The RFC 8785 wire bytes (validates first).
    pub fn to_json(&self) -> Result<Vec<u8>, ReadingError> {
        crate::caps::reading::reading_to_json(self)
    }

    /// Reads and checks a reading from its canonical wire bytes.
    pub fn from_json(bytes: &[u8]) -> Result<GaugeReading, ReadingError> {
        crate::caps::reading::reading_from_json(bytes)
    }
}

/// REQ-002 with a positive resolution: [`QUANTISATION_TOLERANCE`] relative to
/// `max(1, |value / resolution|)`.
fn check_quantised(value: f64, resolution: f64) -> Result<(), ReadingError> {
    if resolution <= 0.0 {
        return Err(ReadingError::range("resolution", "not_positive"));
    }
    let r = value / resolution;
    if !r.is_finite() || r.abs() > MAX_STEPS {
        return Err(ReadingError::range("value", "too_many_steps"));
    }
    let scale = if r.abs() > 1.0 { r.abs() } else { 1.0 };
    if (r - r.round()).abs() <= QUANTISATION_TOLERANCE * scale {
        Ok(())
    } else {
        Err(ReadingError::NotQuantised)
    }
}

impl Reading for GaugeReading {
    const TAG: &'static str = READING_TAG;
    const BATCH_TAG: &'static str = BATCH_TAG;

    fn to_value(&self) -> CValue {
        let mut pairs = vec![
            ("gauge_identity", untrusted(&self.gauge_identity)),
            ("resolution", self.resolution.to_value()),
            ("schema", text(READING_TAG)),
            ("value", self.value.to_value()),
        ];
        if let Some(d) = &self.calibration_ref {
            pairs.push(("calibration_ref", text(d.as_str())));
        }
        pairs.extend(time_pairs(self.timestamp()));
        obj(pairs)
    }

    fn from_value(v: &CValue, ptr: &str) -> Result<GaugeReading, ReadingError> {
        let o = Fields::new(v, ptr, &READING_FIELDS)?;
        o.tag(READING_TAG)?;
        Ok(GaugeReading {
            value: o.quantity("value")?,
            resolution: o.quantity("resolution")?,
            gauge_identity: o.identity("gauge_identity")?,
            calibration_ref: o.opt_digest("calibration_ref")?,
            t_dev: o.opt_i64("t_dev")?,
            t_host: o.i64("t_host")?,
        })
    }

    fn validate(&self) -> Result<(), ReadingError> {
        GaugeReading::validate(self)
    }

    fn timestamp(&self) -> Timestamp {
        Timestamp { t_dev: self.t_dev, t_host: self.t_host }
    }

    /// In order: the gauge identity, the unit, the resolution and the calibration reference.
    fn check_same_stream(&self, first: &GaugeReading, index: usize) -> Result<(), ReadingError> {
        let field = if self.gauge_identity != first.gauge_identity {
            "gauge_identity"
        } else if self.value.unit() != first.value.unit() {
            "unit"
        } else if self.resolution != first.resolution {
            "resolution"
        } else if self.calibration_ref != first.calibration_ref {
            "calibration_ref"
        } else {
            return Ok(());
        };
        Err(ReadingError::Inconsistent { field, index })
    }
}

// ------------------------------------------------------------------ describe info

/// What kind of gauge a device is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GaugeKind {
    Caliper,
    Micrometer,
    Indicator,
    HeightGauge,
    BoreGauge,
    DepthGauge,
    Protractor,
    Other,
}

impl GaugeKind {
    /// Every kind, in wire order.
    pub const ALL: [GaugeKind; 8] = [
        GaugeKind::Caliper,
        GaugeKind::Micrometer,
        GaugeKind::Indicator,
        GaugeKind::HeightGauge,
        GaugeKind::BoreGauge,
        GaugeKind::DepthGauge,
        GaugeKind::Protractor,
        GaugeKind::Other,
    ];

    /// The wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            GaugeKind::Caliper => "caliper",
            GaugeKind::Micrometer => "micrometer",
            GaugeKind::Indicator => "indicator",
            GaugeKind::HeightGauge => "height_gauge",
            GaugeKind::BoreGauge => "bore_gauge",
            GaugeKind::DepthGauge => "depth_gauge",
            GaugeKind::Protractor => "protractor",
            GaugeKind::Other => "other",
        }
    }

    /// Parses a wire name.
    pub fn parse(s: &str) -> Option<GaugeKind> {
        GaugeKind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// What a gauge driver reports about the gauge in describe: its kind, its measuring range
/// and its resolution. Wire form: `{"kind", "range": {"max": <Quantity>, "min": <Quantity>},
/// "resolution": <Quantity>}`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GaugeInfo {
    pub kind: GaugeKind,
    /// Smallest value the gauge reads.
    pub min: Quantity,
    /// Largest value the gauge reads.
    pub max: Quantity,
    pub resolution: Quantity,
}

impl GaugeInfo {
    /// Checks the info, in order: the resolution in m or rad and the range in the same unit
    /// (`unit_unsupported`); a positive resolution and `min < max` (`value_out_of_range`).
    pub fn validate(&self) -> Result<(), ReadingError> {
        let unit = self.resolution.unit();
        if !matches!(unit, SiUnit::Metre | SiUnit::Radian) {
            return Err(ReadingError::unit("resolution", unit));
        }
        for q in [self.min, self.max] {
            if q.unit() != unit {
                return Err(ReadingError::unit("range", q.unit()));
            }
        }
        if self.resolution.value() <= 0.0 {
            return Err(ReadingError::range("resolution", "not_positive"));
        }
        if self.min.value() >= self.max.value() {
            return Err(ReadingError::range("range", "empty"));
        }
        Ok(())
    }

    /// Checks one reading against the info: both valid; the reading's unit and resolution
    /// those of the info (`reading_inconsistent` at index 0); the value within the range
    /// (`value_out_of_range`, `outside_range`).
    pub fn check_reading(&self, r: &GaugeReading) -> Result<(), ReadingError> {
        self.validate()?;
        r.validate()?;
        if r.value.unit() != self.resolution.unit() {
            return Err(ReadingError::Inconsistent { field: "unit", index: 0 });
        }
        if r.resolution != self.resolution {
            return Err(ReadingError::Inconsistent { field: "resolution", index: 0 });
        }
        let v = r.value.value();
        if v < self.min.value() || v > self.max.value() {
            return Err(ReadingError::range("value", "outside_range"));
        }
        Ok(())
    }

    /// The wire value.
    pub fn to_value(&self) -> CValue {
        obj(vec![
            ("kind", text(self.kind.as_str())),
            ("range", obj(vec![("max", self.max.to_value()), ("min", self.min.to_value())])),
            ("resolution", self.resolution.to_value()),
        ])
    }

    /// Reads and checks the wire value.
    pub fn from_value(v: &CValue) -> Result<GaugeInfo, ReadingError> {
        let o = Fields::new(v, "", &["kind", "range", "resolution"])?;
        let kind = GaugeKind::parse(o.str("kind")?).ok_or_else(|| ReadingError::invalid("/kind", "kind_unknown"))?;
        let range_v = o.get("range").ok_or_else(|| ReadingError::invalid("/range", "field_missing"))?;
        let range = Fields::new(range_v, "/range", &["max", "min"])?;
        let info = GaugeInfo {
            kind,
            min: range.quantity("min")?,
            max: range.quantity("max")?,
            resolution: o.quantity("resolution")?,
        };
        info.validate()?;
        Ok(info)
    }
}

// ------------------------------------------------------------------ the evaluators

/// The `Gauge-core@1` item a reading failure belongs to: quantisation and the resolution's
/// domain are `GA-01`; units, ranges and a changed unit or resolution `GA-02`; timestamps,
/// repeats and a changed identity or calibration reference `GA-04`.
pub fn item_of(e: &ReadingError) -> &'static str {
    match e {
        ReadingError::NotQuantised
        | ReadingError::OutOfRange { field: "resolution", .. }
        | ReadingError::OutOfRange { reason: "too_many_steps", .. } => ITEM_QUANTISATION,
        ReadingError::UnitUnsupported { .. }
        | ReadingError::OutOfRange { .. }
        | ReadingError::Inconsistent { field: "unit" | "resolution", .. } => ITEM_UNITS,
        _ => ITEM_TIMESTAMPS,
    }
}

fn failure(e: &ReadingError, index: Option<usize>) -> ItemFailure {
    ItemFailure { item: item_of(e), reason: e.code(), index }
}

/// Items `GA-01`, `GA-02` and `GA-04` over the readings of one recorded stream, in arrival
/// order (all batches joined). With the gauge's describe `info`, the info must be valid
/// (`GA-02`, `info_invalid`) and every reading must match it ([`GaugeInfo::check_reading`]).
/// Every reading is checked with [`GaugeReading::validate`], then the series with
/// [`check_series`]. A failure names its item ([`item_of`]), the error code as the reason and
/// the reading's index. An empty record is `GA-01`, `no_readings`.
pub fn check_readings(info: Option<&GaugeInfo>, readings: &[GaugeReading]) -> Result<(), ItemFailure> {
    if readings.is_empty() {
        return Err(ItemFailure { item: ITEM_QUANTISATION, reason: "no_readings", index: None });
    }
    if let Some(i) = info {
        if i.validate().is_err() {
            return Err(ItemFailure { item: ITEM_UNITS, reason: "info_invalid", index: None });
        }
    }
    for (k, r) in readings.iter().enumerate() {
        r.validate().map_err(|e| failure(&e, Some(k)))?;
        if let Some(i) = info {
            i.check_reading(r).map_err(|e| failure(&e, Some(k)))?;
        }
    }
    check_series(readings).map_err(|e| {
        let index = match e {
            ReadingError::NotMonotonic { index, .. }
            | ReadingError::Duplicate { index }
            | ReadingError::Inconsistent { index, .. } => Some(index),
            _ => None,
        };
        failure(&e, index)
    })
}

/// One recorded zero: a reading, a `zero` call and the next reading, with the gauge held
/// still throughout.
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroCheck {
    pub before: GaugeReading,
    pub outcome: Result<(), ErrorCode>,
    pub after: GaugeReading,
}

/// Item `GA-03` over recorded zeros. For each, in order: both readings valid (the failing
/// reading's item, [`item_of`]); the call succeeded (`operation_failed`); the second reading
/// later than the first on host time and, where both carry it, device time (`not_after`);
/// the gauge identity, unit, resolution, calibration reference and device-time presence
/// unchanged (`identity_changed`, `unit_changed`, `resolution_changed`,
/// `calibration_changed`, `t_dev_changed`), since zero writes no calibration; and the second
/// reading zero steps (`not_zero`). At least one zero must start from a non-zero reading
/// (`not_exercised`); an empty record is `no_steps`.
pub fn check_zero(steps: &[ZeroCheck]) -> Result<(), ItemFailure> {
    let fail = |reason, index| Err(ItemFailure { item: ITEM_ZERO, reason, index });
    if steps.is_empty() {
        return fail("no_steps", None);
    }
    let mut exercised = false;
    for (k, s) in steps.iter().enumerate() {
        for r in [&s.before, &s.after] {
            r.validate().map_err(|e| failure(&e, Some(k)))?;
        }
        if s.outcome.is_err() {
            return fail("operation_failed", Some(k));
        }
        let later_dev = match (s.before.t_dev, s.after.t_dev) {
            (Some(a), Some(b)) => b > a,
            _ => true,
        };
        if s.after.t_host <= s.before.t_host || !later_dev {
            return fail("not_after", Some(k));
        }
        if let Err(ReadingError::Inconsistent { field, .. }) = s.after.check_same_stream(&s.before, 1) {
            let reason = match field {
                "gauge_identity" => "identity_changed",
                "unit" => "unit_changed",
                "resolution" => "resolution_changed",
                _ => "calibration_changed",
            };
            return fail(reason, Some(k));
        }
        if s.after.t_dev.is_some() != s.before.t_dev.is_some() {
            return fail("t_dev_changed", Some(k));
        }
        if s.after.steps().abs() > 0.0 {
            return fail("not_zero", Some(k));
        }
        exercised |= s.before.steps().abs() > 0.0;
    }
    if !exercised {
        return fail("not_exercised", None);
    }
    Ok(())
}

// ------------------------------------------------------------------ settings

/// The package's core setting names: none. Every device setting is a vendor extension under a
/// namespace listed in describe (`HAL.IDL.SENS.STREAM`).
pub const CORE_SETTINGS: [&str; 0] = [];

/// Checks a device's `config-schema/0` against a `Gauge@1` package, failing closed: the
/// description must be this package with no core settings (`settings_invalid`, `not_gauge`),
/// then every setting must be a declared extension under a listed namespace
/// (`extension_undeclared`).
pub fn check_device_schema(package: &StreamingDescription, schema: &ConfigSchema) -> Result<(), ReadingError> {
    if package.description().manifest.name != PACKAGE_NAME || !package.core_settings().is_empty() {
        return Err(ReadingError::settings("", "not_gauge"));
    }
    package.check_config_schema(schema).map_err(ReadingError::Stream)
}

// ------------------------------------------------------------------ operations

/// One operation of the package's own (REQ-004). Streaming comes from the template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GaugeOperation {
    /// `{session_id}` -> `{reading}`: one reading now.
    Read,
    /// `{session_id}` -> `{}`: makes the present position read zero. Writes no calibration.
    Zero,
}

use ErrorCode::{CalibrationExpired, CommLost, Internal, Limit, NotQualified, Transient, Unsupported};

impl GaugeOperation {
    /// Every operation.
    pub const ALL: [GaugeOperation; 2] = [GaugeOperation::Read, GaugeOperation::Zero];

    /// The wire name.
    pub const fn name(self) -> &'static str {
        match self {
            GaugeOperation::Read => "read",
            GaugeOperation::Zero => "zero",
        }
    }

    /// The side-effect class: D1 for both (REQ-004).
    pub const fn class(self) -> SideEffect {
        SideEffect::D1
    }

    /// The `Device@1` error codes the operation may return, in error-table order.
    pub const fn errors(self) -> &'static [ErrorCode] {
        match self {
            GaugeOperation::Read => {
                &[Transient, CommLost, NotQualified, CalibrationExpired, Limit, Unsupported, Internal]
            }
            GaugeOperation::Zero => &[Transient, CommLost, NotQualified, Limit, Unsupported, Internal],
        }
    }

    /// The operation's `capability-package/v1` description.
    pub fn description(self) -> OperationDesc {
        let request = closed(vec![("session_id", def_ref("SessionId"))], &["session_id"]);
        let response = match self {
            GaugeOperation::Read => closed(vec![("reading", def_ref("GaugeReading"))], &["reading"]),
            GaugeOperation::Zero => closed(Vec::new(), &[]),
        };
        OperationDesc {
            name: self.name().to_string(),
            class: self.class(),
            request,
            response,
            errors: self.errors().to_vec(),
            live_settings: Vec::new(),
        }
    }
}

/// The package's own type schemas.
fn defs() -> BTreeMap<String, CValue> {
    let kinds: Vec<&str> = GaugeKind::ALL.iter().map(|k| k.as_str()).collect();
    let mut d: BTreeMap<String, CValue> = quantity_defs().into_iter().collect();
    d.insert("GaugeKind".to_string(), enumeration(&kinds));
    d.insert(
        "GaugeInfo".to_string(),
        closed(
            vec![
                ("kind", def_ref("GaugeKind")),
                ("range", closed(vec![("max", def_ref("Quantity")), ("min", def_ref("Quantity"))], &["max", "min"])),
                ("resolution", def_ref("Quantity")),
            ],
            &["kind", "range", "resolution"],
        ),
    );
    d.insert(
        "GaugeReading".to_string(),
        closed(
            vec![
                ("calibration_ref", string_schema(DIGEST_STRING_LEN)),
                ("gauge_identity", string_schema(MAX_IDENTITY_BYTES)),
                ("resolution", def_ref("Quantity")),
                ("schema", enumeration(&[READING_TAG])),
                ("t_dev", string_schema(INT_STRING_LEN)),
                ("t_host", string_schema(INT_STRING_LEN)),
                ("value", def_ref("Quantity")),
            ],
            &["gauge_identity", "resolution", "schema", "t_host", "value"],
        ),
    );
    d
}

// ------------------------------------------------------------------ conformance profile

/// The `Gauge-core@1` items (REQ-005). Item IDs are stable: an item is never renumbered, and a
/// withdrawn ID is not reused. The built package appends the `Stream-common@1` items. Editing
/// the item text changes the package digest and needs a package version bump.
pub const GAUGE_CORE: ConformanceProfile = ConformanceProfile {
    id: PROFILE_ID,
    items: &[
        ProfileItem {
            id: ITEM_QUANTISATION,
            title: "Quantisation",
            checks: &[
                "every reading has a positive resolution",
                "every value is an integer multiple of its resolution within 1e-9 relative",
            ],
        },
        ProfileItem {
            id: ITEM_UNITS,
            title: "Unit consistency",
            checks: &[
                "value and resolution share one unit, m or rad",
                "every reading of a stream has the unit and resolution of the first and of the gauge info in describe",
                "every value lies within the range the gauge info declares",
            ],
        },
        ProfileItem {
            id: ITEM_ZERO,
            title: "Zero behaviour",
            checks: &[
                "after a successful zero with the gauge held still the next reading is zero",
                "zero changes neither the gauge identity, unit and resolution nor the calibration reference",
            ],
        },
        ProfileItem {
            id: ITEM_TIMESTAMPS,
            title: "Timestamp monotonicity",
            checks: &[
                "t_host never decreases from one reading to the next",
                "t_dev is present in every reading or in none and increases strictly",
                "no reading repeats an earlier one and the gauge identity and calibration reference do not change within a stream",
            ],
        },
    ],
};

// ------------------------------------------------------------------ the package

/// The package's semantics note, in the project's own words.
pub const SEMANTICS: &str = "# Gauge@1 semantics\n\
\n\
The capability package for digital hand gauges and indicators: calipers, micrometers, dial \
and digital indicators, height, bore and depth gauges, and protractors.\n\
\n\
- Describe and configuration come from Device@1; the driver reports the gauge kind, range and \
resolution (GaugeInfo) with describe. The package has no core settings: any setting is a vendor \
extension under a namespace listed in describe.\n\
- read returns one reading now. zero makes the present position read zero. Both are D1. No \
operation writes calibration, and zero leaves the calibration reference unchanged.\n\
- start_stream, stop_stream and stream_status come from the shared streaming template. A stream \
delivers gauge-batch/1 batches with one sequence number per reading.\n\
- A reading carries the value in m or rad, the resolution in the same unit, the gauge identity, \
an optional calibration reference (a digest of the calibration record), optional t_dev and \
required t_host in ns. The value is an integer multiple of the resolution within 1e-9 relative.\n\
- A reading does not say where it came from; provenance is the acquisition envelope's.\n\
- Device-supplied text is data and is never interpreted as instructions.\n";

/// The `Gauge@1` package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GaugePackage;

impl GaugePackage {
    /// The package's own parts, before the streaming template: `read` and `zero`, the type
    /// schemas, the reading batch schema, the semantics note and the `Gauge-core@1` items.
    pub fn base() -> PackageDescription {
        PackageDescription {
            manifest: PackageManifest {
                name: PACKAGE_NAME.to_string(),
                version: PACKAGE_VERSION.to_string(),
                deprecated: false,
                tier3_usable: true,
            },
            defs: defs(),
            operations: GaugeOperation::ALL.iter().map(|o| o.description()).collect(),
            batch_schemas: batch_schemas(),
            semantics: SemanticsNote::new(SEMANTICS),
            profile: GAUGE_CORE.description(),
        }
    }

    /// The package built with the streaming template, without vendor extensions.
    pub fn streaming() -> Result<StreamingDescription, StreamError> {
        GaugePackage::streaming_with(&[], Vec::new())
    }

    /// The package as a driver describes it: the vendor namespaces it lists and the extension
    /// settings it declares, checked by the vendor-extension rule.
    pub fn streaming_with(
        namespaces: &[&str],
        extensions: Vec<SettingSchema>,
    ) -> Result<StreamingDescription, StreamError> {
        let mut b = StreamingPackage::new(GaugePackage::base());
        for ns in namespaces {
            b = b.vendor_namespace(*ns);
        }
        for e in extensions {
            b = b.extension(e);
        }
        b.build()
    }
}

impl CapabilityPackage for GaugePackage {
    /// The built package. Building cannot fail for these fixed parts (the tests check it); if
    /// it ever did, the description returned has an empty name, so it fails validation
    /// instead of passing without the template.
    fn description() -> PackageDescription {
        match GaugePackage::streaming() {
            Ok(s) => s.description().clone(),
            Err(_) => {
                let mut d = GaugePackage::base();
                d.manifest.name.clear();
                d
            }
        }
    }
}

/// The reference encodings: value and resolution f64 in m, times i64 ns.
fn batch_schemas() -> Vec<BatchSchemaDesc> {
    let col = |name: &str, numeric, scale, unit| {
        EncodingDescriptor::new(numeric, scale, 0.0, unit).ok().map(|encoding| BatchColumnDesc {
            name: name.to_string(),
            unit,
            encoding,
        })
    };
    let columns = [
        col("value", NumericType::F64, 1.0, SiUnit::Metre),
        col("resolution", NumericType::F64, 1.0, SiUnit::Metre),
        col("t_host", NumericType::I64, 1e-9, SiUnit::Second),
        col("t_dev", NumericType::I64, 1e-9, SiUnit::Second),
    ];
    vec![BatchSchemaDesc {
        id: BATCH_SCHEMA_ID.to_string(),
        columns: columns.into_iter().flatten().collect(),
        validity_mask: false,
        seq_range: true,
    }]
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn kinds_round_trip() {
        for k in GaugeKind::ALL {
            assert_eq!(GaugeKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(GaugeKind::parse("Caliper"), None);
    }

    #[test]
    fn quantisation_uses_a_relative_tolerance() {
        assert_eq!(check_quantised(0.01234, 1e-5), Ok(()));
        assert_eq!(check_quantised(0.012345, 1e-5), Err(ReadingError::NotQuantised));
        assert_eq!(check_quantised(0.0, 1e-5), Ok(()));
        assert_eq!(check_quantised(-0.01234, 1e-5), Ok(()));
        assert_eq!(check_quantised(1.0, 0.0).unwrap_err().code(), "value_out_of_range");
        assert_eq!(check_quantised(1.0, 1e-300).unwrap_err().code(), "value_out_of_range");
    }
}

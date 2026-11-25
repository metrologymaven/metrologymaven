//! The `Environment@1` package for temperature, humidity and pressure sensors (spec node
//! `HAL.IDL.SENS.GAUENV`): subscription through the shared streaming template, the
//! [`EnvReading`] it delivers, and the `Environment-core@1` profile.
//!
//! - **REQ-HAL.IDL.SENS.GAUENV-003:** [`EnvReading`] carries the temperature in K with its
//!   stated standard uncertainty in K, the sampling time in s, and optional humidity (a
//!   dimensionless fraction in [0, 1]) and pressure (Pa), plus the sensor identity and the
//!   timestamps. [`EnvReading::validate`] fails with `unit_unsupported` or
//!   `value_out_of_range`.
//! - **REQ-HAL.IDL.SENS.GAUENV-005:** [`ENVIRONMENT_CORE`] lists `EN-01` (units), `EN-02`
//!   (uncertainty presence), `EN-03` (sampling-time consistency) and `EN-04` (timestamp
//!   monotonicity); the built package adds the `Stream-common@1` items. [`check_readings`]
//!   evaluates all four on a recorded stream.
//!
//! The package defines no operations of its own: subscribing is the template's
//! `start_stream`, and readings arrive as `env-batch/1` [`ReadingBatch`]es.
//!
//! **Sampling time** is the time over which the sensor sampled one reading (for example its
//! averaging window). It is fixed by the sensor or by the optional core setting
//! `sampling_time`, so it is the same in every reading of a stream, and equal to the setting
//! when the device has one (`EN-03`).
//!
//! **Wire form** of a reading (RFC 8785, closed key set, see [`crate::caps::reading`]):
//!
//! ```text
//! {"schema": "env-reading/1", "temperature": <Quantity K>, "temperature_u": <Quantity K>,
//!  "humidity"?: <Quantity 1>, "pressure"?: <Quantity Pa>, "sampling_time": <Quantity s>,
//!  "sensor_identity": "<1..256 bytes>", "t_dev"?: "<i64 ns>", "t_host": "<i64 ns>"}
//! ```

use crate::base::config::{validate as validate_config, ConfigSchema, Configuration, SettingSchema};
use crate::base::config::{SettingType, SettingValue};
use crate::base::device::{ConformanceProfile, ProfileItem};
use crate::caps::reading::{
    check_identity, check_series, closed, def_ref, enumeration, obj, quantity_defs, string_schema, text, time_pairs,
    untrusted, Fields, Reading, ReadingBatch, ReadingError, INT_STRING_LEN, MAX_IDENTITY_BYTES,
};
use crate::caps::stream::{ItemFailure, StreamError, StreamingDescription, StreamingPackage};
use crate::package::{BatchColumnDesc, BatchSchemaDesc, CapabilityPackage, PackageDescription};
use crate::package::{PackageManifest, SemanticsNote};
use crate::units::{EncodingDescriptor, NumericType, Quantity, SiUnit, Timestamp};
use crate::UntrustedText;
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};

/// Package name.
pub const PACKAGE_NAME: &str = "Environment";
/// Package version.
pub const PACKAGE_VERSION: &str = "1.0.0";
/// ID of the conformance profile.
pub const PROFILE_ID: &str = "Environment-core@1";
/// ID of the package's batch schema.
pub const BATCH_SCHEMA_ID: &str = "environment.readings";
/// Wire tag of one reading.
pub const READING_TAG: &str = "env-reading/1";
/// Wire tag of a batch of readings.
pub const BATCH_TAG: &str = "env-batch/1";

/// Item `EN-01`: units.
pub const ITEM_UNITS: &str = "EN-01";
/// Item `EN-02`: uncertainty presence.
pub const ITEM_UNCERTAINTY: &str = "EN-02";
/// Item `EN-03`: sampling-time consistency.
pub const ITEM_SAMPLING: &str = "EN-03";
/// Item `EN-04`: timestamp monotonicity.
pub const ITEM_TIMESTAMPS: &str = "EN-04";

/// A batch of environment readings.
pub type EnvBatch = ReadingBatch<EnvReading>;

// ------------------------------------------------------------------ the reading

/// One environment reading (REQ-003).
#[derive(Debug, Clone, PartialEq)]
pub struct EnvReading {
    /// Air or part temperature, in K.
    pub temperature: Quantity,
    /// The sensor's stated standard uncertainty of the temperature, in K.
    pub temperature_u: Quantity,
    /// Relative humidity as a fraction in [0, 1], when the sensor measures it.
    pub humidity: Option<Quantity>,
    /// Air pressure, in Pa, when the sensor measures it.
    pub pressure: Option<Quantity>,
    /// The time over which the sensor sampled this reading, in s.
    pub sampling_time: Quantity,
    /// The sensor's identity as it reports it; data, never instructions.
    pub sensor_identity: UntrustedText,
    /// Device time, in ns, when the sensor has a clock.
    pub t_dev: Option<i64>,
    /// Host time, in ns.
    pub t_host: i64,
}

const READING_FIELDS: [&str; 9] = [
    "humidity",
    "pressure",
    "sampling_time",
    "schema",
    "sensor_identity",
    "t_dev",
    "t_host",
    "temperature",
    "temperature_u",
];

impl EnvReading {
    /// Checks the reading, in order: the identity is 1 to 256 bytes (`reading_invalid`); every
    /// unit, temperature and uncertainty in K, humidity `1`, pressure Pa, sampling time s
    /// (`unit_unsupported`); then positive temperature, uncertainty, pressure and sampling
    /// time and a humidity in [0, 1] (`value_out_of_range`).
    pub fn validate(&self) -> Result<(), ReadingError> {
        check_identity("sensor_identity", &self.sensor_identity)?;
        let mut units = vec![
            ("temperature", self.temperature, SiUnit::Kelvin),
            ("temperature_u", self.temperature_u, SiUnit::Kelvin),
        ];
        if let Some(h) = self.humidity {
            units.push(("humidity", h, SiUnit::One));
        }
        if let Some(p) = self.pressure {
            units.push(("pressure", p, SiUnit::Pascal));
        }
        units.push(("sampling_time", self.sampling_time, SiUnit::Second));
        for (field, q, unit) in &units {
            if q.unit() != *unit {
                return Err(ReadingError::unit(field, q.unit()));
            }
        }
        let positive = |field, q: Quantity| {
            if q.value() > 0.0 {
                Ok(())
            } else {
                Err(ReadingError::range(field, "not_positive"))
            }
        };
        positive("temperature", self.temperature)?;
        positive("temperature_u", self.temperature_u)?;
        if let Some(h) = self.humidity {
            if !(0.0..=1.0).contains(&h.value()) {
                return Err(ReadingError::range("humidity", "outside_0_1"));
            }
        }
        if let Some(p) = self.pressure {
            positive("pressure", p)?;
        }
        positive("sampling_time", self.sampling_time)
    }

    /// The RFC 8785 wire bytes (validates first).
    pub fn to_json(&self) -> Result<Vec<u8>, ReadingError> {
        crate::caps::reading::reading_to_json(self)
    }

    /// Reads and checks a reading from its canonical wire bytes.
    pub fn from_json(bytes: &[u8]) -> Result<EnvReading, ReadingError> {
        crate::caps::reading::reading_from_json(bytes)
    }
}

impl Reading for EnvReading {
    const TAG: &'static str = READING_TAG;
    const BATCH_TAG: &'static str = BATCH_TAG;

    fn to_value(&self) -> CValue {
        let mut pairs = vec![
            ("sampling_time", self.sampling_time.to_value()),
            ("schema", text(READING_TAG)),
            ("sensor_identity", untrusted(&self.sensor_identity)),
            ("temperature", self.temperature.to_value()),
            ("temperature_u", self.temperature_u.to_value()),
        ];
        if let Some(h) = self.humidity {
            pairs.push(("humidity", h.to_value()));
        }
        if let Some(p) = self.pressure {
            pairs.push(("pressure", p.to_value()));
        }
        pairs.extend(time_pairs(self.timestamp()));
        obj(pairs)
    }

    fn from_value(v: &CValue, ptr: &str) -> Result<EnvReading, ReadingError> {
        let o = Fields::new(v, ptr, &READING_FIELDS)?;
        o.tag(READING_TAG)?;
        Ok(EnvReading {
            temperature: o.quantity("temperature")?,
            temperature_u: o.quantity("temperature_u")?,
            humidity: o.opt_quantity("humidity")?,
            pressure: o.opt_quantity("pressure")?,
            sampling_time: o.quantity("sampling_time")?,
            sensor_identity: o.identity("sensor_identity")?,
            t_dev: o.opt_i64("t_dev")?,
            t_host: o.i64("t_host")?,
        })
    }

    fn validate(&self) -> Result<(), ReadingError> {
        EnvReading::validate(self)
    }

    fn timestamp(&self) -> Timestamp {
        Timestamp { t_dev: self.t_dev, t_host: self.t_host }
    }

    /// In order: the sensor identity, the presence of humidity and of pressure, and the
    /// sampling time.
    fn check_same_stream(&self, first: &EnvReading, index: usize) -> Result<(), ReadingError> {
        let field = if self.sensor_identity != first.sensor_identity {
            "sensor_identity"
        } else if self.humidity.is_some() != first.humidity.is_some() {
            "humidity"
        } else if self.pressure.is_some() != first.pressure.is_some() {
            "pressure"
        } else if self.sampling_time != first.sampling_time {
            "sampling_time"
        } else {
            return Ok(());
        };
        Err(ReadingError::Inconsistent { field, index })
    }
}

// ------------------------------------------------------------------ settings

/// The sampling time (s), for sensors whose sampling time is configurable.
pub const SETTING_SAMPLING_TIME: &str = "sampling_time";

/// The package's core setting names. A device setting without a dot outside this list is an
/// undeclared extension (`HAL.IDL.SENS.STREAM`). A sensor with a fixed sampling time need not
/// declare it.
pub const CORE_SETTINGS: [&str; 1] = [SETTING_SAMPLING_TIME];

/// The reference declaration of the core setting: acquisition-relevant, not live.
pub fn core_setting_schemas() -> Vec<SettingSchema> {
    vec![SettingSchema {
        name: SETTING_SAMPLING_TIME.to_string(),
        ty: SettingType::Quantity(SiUnit::Second),
        range: None,
        acquisition_relevant: true,
        live_changeable: false,
    }]
}

/// Checks a device's `config-schema/0` against an `Environment@1` package, failing closed:
/// the description must be this package with its core settings (`settings_invalid`,
/// `not_environment`); every setting must be a core setting or a declared extension under a
/// listed namespace (`extension_undeclared`); and a declared `sampling_time` must be a
/// quantity in s (`type_wrong`) and acquisition-relevant (`not_digested`).
pub fn check_device_schema(package: &StreamingDescription, schema: &ConfigSchema) -> Result<(), ReadingError> {
    let core: BTreeSet<String> = CORE_SETTINGS.iter().map(|s| s.to_string()).collect();
    if package.description().manifest.name != PACKAGE_NAME || *package.core_settings() != core {
        return Err(ReadingError::settings("", "not_environment"));
    }
    package.check_config_schema(schema).map_err(ReadingError::Stream)?;
    if let Some(s) = schema.get(SETTING_SAMPLING_TIME) {
        if s.ty != SettingType::Quantity(SiUnit::Second) {
            return Err(ReadingError::settings(SETTING_SAMPLING_TIME, "type_wrong"));
        }
        if !s.acquisition_relevant {
            return Err(ReadingError::settings(SETTING_SAMPLING_TIME, "not_digested"));
        }
    }
    Ok(())
}

/// The typed core settings of an `Environment@1` device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvironmentSettings {
    /// The configured sampling time in s; `None` when the device has no such setting.
    pub sampling_time_s: Option<f64>,
}

impl EnvironmentSettings {
    /// Reads the core settings of a configuration, in order: [`check_device_schema`]; the
    /// configuration against the schema (`CONFIG_INVALID`, `OUT_OF_RANGE`); a positive
    /// sampling time (`settings_invalid`, `not_positive`).
    pub fn from_configuration(
        package: &StreamingDescription,
        schema: &ConfigSchema,
        cfg: &Configuration,
    ) -> Result<EnvironmentSettings, ReadingError> {
        check_device_schema(package, schema)?;
        validate_config(schema, cfg).map_err(ReadingError::Config)?;
        let sampling_time_s = match cfg.values.get(SETTING_SAMPLING_TIME) {
            None => None,
            Some(SettingValue::Quantity(q)) if q.value() > 0.0 => Some(q.value()),
            Some(SettingValue::Quantity(_)) => {
                return Err(ReadingError::settings(SETTING_SAMPLING_TIME, "not_positive"));
            }
            Some(_) => return Err(ReadingError::settings(SETTING_SAMPLING_TIME, "type_wrong")),
        };
        Ok(EnvironmentSettings { sampling_time_s })
    }
}

// ------------------------------------------------------------------ the evaluator

/// The `Environment-core@1` item a reading failure belongs to: the uncertainty's domain is
/// `EN-02`; the sampling time's domain or a changed or unconfigured sampling time `EN-03`;
/// other units, domains, a changed humidity or pressure presence and a malformed reading (an
/// empty or over-long identity) `EN-01`; timestamps, repeats and a changed identity `EN-04`.
pub fn item_of(e: &ReadingError) -> &'static str {
    match e {
        ReadingError::OutOfRange { field: "temperature_u", .. } => ITEM_UNCERTAINTY,
        ReadingError::OutOfRange { field: "sampling_time", .. }
        | ReadingError::UnitUnsupported { field: "sampling_time", .. }
        | ReadingError::Inconsistent { field: "sampling_time", .. } => ITEM_SAMPLING,
        ReadingError::UnitUnsupported { .. }
        | ReadingError::OutOfRange { .. }
        | ReadingError::Invalid { .. }
        | ReadingError::Inconsistent { field: "humidity" | "pressure", .. } => ITEM_UNITS,
        _ => ITEM_TIMESTAMPS,
    }
}

fn failure(e: &ReadingError, index: Option<usize>) -> ItemFailure {
    ItemFailure { item: item_of(e), reason: e.code(), index }
}

/// Items `EN-01` .. `EN-04` over the readings of one recorded stream, in arrival order (all
/// batches joined). Every reading is checked with [`EnvReading::validate`]; with `settings`
/// from [`EnvironmentSettings::from_configuration`] whose sampling time is set, every
/// reading's sampling time must equal it (`EN-03`, `reading_inconsistent`); then the series
/// with [`check_series`]. A failure names its item ([`item_of`]), the error code as the
/// reason and the reading's index. An empty record is `EN-01`, `no_readings`.
pub fn check_readings(settings: Option<&EnvironmentSettings>, readings: &[EnvReading]) -> Result<(), ItemFailure> {
    if readings.is_empty() {
        return Err(ItemFailure { item: ITEM_UNITS, reason: "no_readings", index: None });
    }
    let configured = settings.and_then(|s| s.sampling_time_s);
    for (k, r) in readings.iter().enumerate() {
        r.validate().map_err(|e| failure(&e, Some(k)))?;
        if let Some(t) = configured {
            if r.sampling_time.value().to_bits() != t.to_bits() {
                return Err(failure(&ReadingError::Inconsistent { field: "sampling_time", index: k }, Some(k)));
            }
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

// ------------------------------------------------------------------ conformance profile

/// The `Environment-core@1` items (REQ-005). Item IDs are stable: an item is never
/// renumbered, and a withdrawn ID is not reused. The built package appends the
/// `Stream-common@1` items. Editing the item text changes the package digest and needs a
/// package version bump.
pub const ENVIRONMENT_CORE: ConformanceProfile = ConformanceProfile {
    id: PROFILE_ID,
    items: &[
        ProfileItem {
            id: ITEM_UNITS,
            title: "Units",
            checks: &[
                "temperature is in K and positive, humidity a fraction in [0, 1] and pressure in Pa and positive",
                "every reading of a stream carries the same optional quantities",
            ],
        },
        ProfileItem {
            id: ITEM_UNCERTAINTY,
            title: "Uncertainty presence",
            checks: &["every reading states a positive standard uncertainty of its temperature in K"],
        },
        ProfileItem {
            id: ITEM_SAMPLING,
            title: "Sampling-time consistency",
            checks: &[
                "every reading states a positive sampling time in s",
                "the sampling time is the same in every reading of a stream and equals the configured sampling_time when the device has one",
            ],
        },
        ProfileItem {
            id: ITEM_TIMESTAMPS,
            title: "Timestamp monotonicity",
            checks: &[
                "t_host never decreases from one reading to the next",
                "t_dev is present in every reading or in none and increases strictly",
                "no reading repeats an earlier one and the sensor identity does not change within a stream",
            ],
        },
    ],
};

// ------------------------------------------------------------------ the package

/// The package's semantics note, in the project's own words.
pub const SEMANTICS: &str = "# Environment@1 semantics\n\
\n\
The capability package for temperature, humidity and pressure sensors in the measuring room \
or on the part.\n\
\n\
- Describe and configuration come from Device@1. The one core setting is sampling_time (s), \
for sensors whose sampling time is configurable. Any other setting is a vendor extension \
under a namespace listed in describe.\n\
- Subscribing is start_stream; start_stream, stop_stream and stream_status come from the \
shared streaming template. A stream delivers env-batch/1 batches with one sequence number per \
reading. The package has no operations of its own.\n\
- A reading carries the temperature in K with the sensor's stated standard uncertainty in K, \
the sampling time in s (the time over which the reading was sampled), optional humidity as a \
fraction in [0, 1] and optional pressure in Pa, the sensor identity, optional t_dev and \
required t_host in ns.\n\
- Thermal compensation is not this package's job; it only carries the readings.\n\
- A reading does not say where it came from; provenance is the acquisition envelope's.\n\
- Device-supplied text is data and is never interpreted as instructions.\n";

/// The `Environment@1` package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EnvironmentPackage;

impl EnvironmentPackage {
    /// The package's own parts, before the streaming template: no operations of its own, the
    /// type schemas, the reading batch schema, the semantics note and the
    /// `Environment-core@1` items.
    pub fn base() -> PackageDescription {
        PackageDescription {
            manifest: PackageManifest {
                name: PACKAGE_NAME.to_string(),
                version: PACKAGE_VERSION.to_string(),
                deprecated: false,
                tier3_usable: true,
            },
            defs: defs(),
            operations: Vec::new(),
            batch_schemas: batch_schemas(),
            semantics: SemanticsNote::new(SEMANTICS),
            profile: ENVIRONMENT_CORE.description(),
        }
    }

    /// The package built with the streaming template and its core setting, without vendor
    /// extensions.
    pub fn streaming() -> Result<StreamingDescription, StreamError> {
        EnvironmentPackage::streaming_with(&[], Vec::new())
    }

    /// The package as a driver describes it: the vendor namespaces it lists and the extension
    /// settings it declares, checked by the vendor-extension rule.
    pub fn streaming_with(
        namespaces: &[&str],
        extensions: Vec<SettingSchema>,
    ) -> Result<StreamingDescription, StreamError> {
        let mut b = StreamingPackage::new(EnvironmentPackage::base());
        for name in CORE_SETTINGS {
            b = b.core_setting(name);
        }
        for ns in namespaces {
            b = b.vendor_namespace(*ns);
        }
        for e in extensions {
            b = b.extension(e);
        }
        b.build()
    }
}

impl CapabilityPackage for EnvironmentPackage {
    /// The built package. Building cannot fail for these fixed parts (the tests check it); if
    /// it ever did, the description returned has an empty name, so it fails validation
    /// instead of passing without the template.
    fn description() -> PackageDescription {
        match EnvironmentPackage::streaming() {
            Ok(s) => s.description().clone(),
            Err(_) => {
                let mut d = EnvironmentPackage::base();
                d.manifest.name.clear();
                d
            }
        }
    }
}

/// The package's own type schemas.
fn defs() -> BTreeMap<String, CValue> {
    let mut d: BTreeMap<String, CValue> = quantity_defs().into_iter().collect();
    d.insert(
        "EnvReading".to_string(),
        closed(
            vec![
                ("humidity", def_ref("Quantity")),
                ("pressure", def_ref("Quantity")),
                ("sampling_time", def_ref("Quantity")),
                ("schema", enumeration(&[READING_TAG])),
                ("sensor_identity", string_schema(MAX_IDENTITY_BYTES)),
                ("t_dev", string_schema(INT_STRING_LEN)),
                ("t_host", string_schema(INT_STRING_LEN)),
                ("temperature", def_ref("Quantity")),
                ("temperature_u", def_ref("Quantity")),
            ],
            &["sampling_time", "schema", "sensor_identity", "t_host", "temperature", "temperature_u"],
        ),
    );
    d
}

/// The reference encodings: every quantity f64 in its unit, times i64 ns.
fn batch_schemas() -> Vec<BatchSchemaDesc> {
    let col = |name: &str, numeric, scale, unit| {
        EncodingDescriptor::new(numeric, scale, 0.0, unit).ok().map(|encoding| BatchColumnDesc {
            name: name.to_string(),
            unit,
            encoding,
        })
    };
    let columns = [
        col("temperature", NumericType::F64, 1.0, SiUnit::Kelvin),
        col("temperature_u", NumericType::F64, 1.0, SiUnit::Kelvin),
        col("humidity", NumericType::F64, 1.0, SiUnit::One),
        col("pressure", NumericType::F64, 1.0, SiUnit::Pascal),
        col("sampling_time", NumericType::F64, 1.0, SiUnit::Second),
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

    fn q(v: f64, u: SiUnit) -> Quantity {
        Quantity::new(v, u).unwrap()
    }

    #[test]
    fn humidity_bounds_are_inclusive() {
        let mut r = EnvReading {
            temperature: q(293.15, SiUnit::Kelvin),
            temperature_u: q(0.05, SiUnit::Kelvin),
            humidity: Some(q(0.0, SiUnit::One)),
            pressure: None,
            sampling_time: q(1.0, SiUnit::Second),
            sensor_identity: UntrustedText::new("x"),
            t_dev: None,
            t_host: 0,
        };
        r.validate().unwrap();
        r.humidity = Some(q(1.0, SiUnit::One));
        r.validate().unwrap();
        r.humidity = Some(q(1.0000000000000002, SiUnit::One));
        assert_eq!(r.validate(), Err(ReadingError::range("humidity", "outside_0_1")));
    }
}

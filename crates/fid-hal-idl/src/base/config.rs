//! Typed configuration, the acquisition-relevant subset and the configuration digest. Spec
//! node `HAL.IDL.BASE.CFG` (REQ-HAL.IDL.BASE-007).
//!
//! A capability package declares a [`ConfigSchema`]: one [`SettingSchema`] per setting, with
//! its type, its SI unit if it is a quantity, an optional inclusive range, and the flags
//! `acquisition_relevant` and `live_changeable`. Vendor extensions are settings named
//! `<vendor>.<name>`; they are declared in describe like any other setting and always enter
//! the digest. A [`Configuration`] holds one value for every declared setting.
//!
//! [`config_digest`] is the SHA-256 of the RFC 8785 canonical JSON (through `fid-canon`) of
//! the object that maps each acquisition-relevant or extension setting name to its wire
//! value. Settings that are neither leave the digest unchanged (REQ-003).
//!
//! Wire forms (JSON through `fid-canon`; every field required, unknown fields refused):
//!
//! - schema: `{"schema": "config-schema/0", "settings": [setting, ...]}`, where a setting is
//!   `{"name", "type", "unit", "values", "range", "acquisition_relevant", "live_changeable"}`;
//!   `type` is `bool`, `int`, `quantity`, `enum` or `text`; `unit` is an SI symbol of
//!   `HAL.IDL.UNITS` for a quantity and `null` otherwise; `values` lists an enum's values and
//!   is `null` otherwise; `range` is `null` or `{"min", "max"}` (inclusive) for an int
//!   (decimal strings) or a quantity (`"0x"` + 16 hex digits, in the setting's unit);
//! - configuration: `{"schema": "configuration/0", "values": {name: value, ...}}`; a bool is
//!   a JSON bool, an int a canonical decimal string, a quantity the `HAL.IDL.UNITS` form
//!   `{"unit", "value"}`, an enum or text value a string.
//!
//! Errors carry one of the two leaf codes, `CONFIG_INVALID` or `OUT_OF_RANGE`, the name of
//! the offending setting (empty for the document as a whole) and a finer [`ConfigReason`].
//! Settings are checked in name order (a schema: in list order), so the first failure is
//! well defined. Every list and string is bounded and the input is limited to
//! [`MAX_INPUT_BYTES`] bytes.
//!
//! A quantity value of `-0.0` is normalised to `+0.0` when it is read, range-checked,
//! written and digested: both spell the same setpoint, so they give the same digest
//! (maintainer decision on PR 162).
//!
//! [`validate`], [`digest_canonical_bytes`] and [`config_digest`] are reached through
//! `fid_hal_idl::base::config::`; only [`config_digest`] and the types are re-exported at the
//! crate root, because `validate` there is the stamp validator (`stamp::validate`).

use crate::stamp::digest_of;
use crate::units::{boolean, f64_bits, i64_dec, kept, object, string, take, Obj, Quantity, SiUnit, UnitsError};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Largest accepted schema or configuration document, in bytes.
pub const MAX_INPUT_BYTES: usize = 256 * 1024;
/// Deepest accepted nesting of a schema or configuration document.
pub const MAX_DEPTH: usize = 8;
/// Most settings in one schema (and so in one configuration).
pub const MAX_SETTINGS: usize = 256;
/// Longest setting name, in bytes.
pub const MAX_NAME_BYTES: usize = 64;
/// Most values of one enum setting.
pub const MAX_ENUM_VALUES: usize = 64;
/// Longest enum value, in bytes.
pub const MAX_ENUM_VALUE_BYTES: usize = 64;
/// Longest text value, in Unicode scalar values.
pub const MAX_TEXT_CHARS: usize = 256;

/// Wire tag of a schema document.
pub const SCHEMA_TAG: &str = "config-schema/0";
/// Wire tag of a configuration document.
pub const CONFIGURATION_TAG: &str = "configuration/0";

const SETTING_FIELDS: [&str; 7] =
    ["acquisition_relevant", "live_changeable", "name", "range", "type", "unit", "values"];

// ------------------------------------------------------------------ errors

/// The two error codes this leaf uses, from the `HAL.IDL.BASE.ERR` code set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConfigErrorCode {
    /// A schema or configuration that breaks a typing rule (REQ-004).
    ConfigInvalid,
    /// A well-typed value outside its declared range (REQ-004).
    OutOfRange,
}

impl From<ConfigErrorCode> for super::error::ErrorCode {
    fn from(c: ConfigErrorCode) -> super::error::ErrorCode {
        match c {
            ConfigErrorCode::ConfigInvalid => super::error::ErrorCode::ConfigInvalid,
            ConfigErrorCode::OutOfRange => super::error::ErrorCode::OutOfRange,
        }
    }
}

impl ConfigErrorCode {
    /// The wire code: `CONFIG_INVALID` or `OUT_OF_RANGE`.
    pub fn as_str(self) -> &'static str {
        match self {
            ConfigErrorCode::ConfigInvalid => "CONFIG_INVALID",
            ConfigErrorCode::OutOfRange => "OUT_OF_RANGE",
        }
    }
}

/// Why a check failed, finer than the code; [`ConfigReason::as_str`] is stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConfigReason {
    /// A configuration value for a setting the schema does not declare, including an
    /// extension not declared under its vendor namespace.
    Undeclared,
    /// A declared setting without a value.
    Missing,
    /// A value of the wrong JSON or Rust type for its setting.
    WrongType,
    /// A quantity whose unit is not the setting's unit (or no SI unit at all).
    WrongUnit,
    /// A quantity value that is NaN or infinite.
    NotFinite,
    /// A value of the right type with a malformed spelling (an int with a leading zero, a
    /// quantity with an extra field, text with a control character).
    BadValue,
    /// An enum value outside the declared values.
    Enum,
    /// A text value longer than [`MAX_TEXT_CHARS`].
    TooLong,
    /// A value outside its range (code `OUT_OF_RANGE`), or a malformed range in a schema.
    Range,
    /// A document or setting object of the wrong shape: unknown or missing field, wrong
    /// tag, wrong JSON type.
    Schema,
    /// A setting name that is not `name` or `vendor.name` (lowercase, digits, `_`).
    BadName,
    /// Two settings with the same name.
    Duplicate,
    /// A quantity without an SI unit, or a unit on a setting that is not a quantity.
    Unit,
    /// An enum without values, with a repeated or malformed value, or values on a setting
    /// that is not an enum.
    Values,
    /// A setting type outside `bool`, `int`, `quantity`, `enum` and `text`.
    Type,
    /// An extension declared as not acquisition-relevant; extensions are always digested.
    ExtensionFlag,
    /// More than [`MAX_SETTINGS`] settings or values.
    TooMany,
    /// Bytes that are not JSON `fid-canon` accepts (syntax, duplicate key, size or depth).
    Json,
}

impl ConfigReason {
    /// Stable machine-readable reason.
    pub fn as_str(self) -> &'static str {
        match self {
            ConfigReason::Undeclared => "undeclared",
            ConfigReason::Missing => "missing",
            ConfigReason::WrongType => "wrong_type",
            ConfigReason::WrongUnit => "wrong_unit",
            ConfigReason::NotFinite => "not_finite",
            ConfigReason::BadValue => "bad_value",
            ConfigReason::Enum => "enum",
            ConfigReason::TooLong => "too_long",
            ConfigReason::Range => "range",
            ConfigReason::Schema => "schema",
            ConfigReason::BadName => "bad_name",
            ConfigReason::Duplicate => "duplicate",
            ConfigReason::Unit => "unit",
            ConfigReason::Values => "values",
            ConfigReason::Type => "type",
            ConfigReason::ExtensionFlag => "extension_flag",
            ConfigReason::TooMany => "too_many",
            ConfigReason::Json => "json",
        }
    }
}

/// A validation failure naming the offending setting (REQ-004). `setting` is empty when the
/// document as a whole is wrong, and is cut to 64 bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfigError {
    pub code: ConfigErrorCode,
    pub setting: String,
    pub reason: ConfigReason,
}

impl ConfigError {
    fn invalid(setting: &str, reason: ConfigReason) -> ConfigError {
        ConfigError { code: ConfigErrorCode::ConfigInvalid, setting: kept(setting), reason }
    }

    fn out_of_range(setting: &str) -> ConfigError {
        ConfigError { code: ConfigErrorCode::OutOfRange, setting: kept(setting), reason: ConfigReason::Range }
    }

    /// The wire code: `CONFIG_INVALID` or `OUT_OF_RANGE`.
    pub fn code(&self) -> &'static str {
        self.code.as_str()
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}) for setting {:?}", self.code(), self.reason.as_str(), self.setting)
    }
}

impl std::error::Error for ConfigError {}

// ------------------------------------------------------------------ schema

/// The type of a setting. A quantity carries its SI unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingType {
    Bool,
    Int,
    Quantity(SiUnit),
    /// The allowed values, distinct and non-empty.
    Enum(Vec<String>),
    Text,
}

impl SettingType {
    /// The wire name: `bool`, `int`, `quantity`, `enum` or `text`.
    pub fn as_str(&self) -> &'static str {
        match self {
            SettingType::Bool => "bool",
            SettingType::Int => "int",
            SettingType::Quantity(_) => "quantity",
            SettingType::Enum(_) => "enum",
            SettingType::Text => "text",
        }
    }
}

/// An inclusive range. A quantity range is in the setting's unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingRange {
    Int { min: i64, max: i64 },
    Quantity { min: f64, max: f64 },
}

/// One declared setting (REQ-001).
#[derive(Debug, Clone, PartialEq)]
pub struct SettingSchema {
    /// `name`, or `vendor.name` for a vendor extension.
    pub name: String,
    pub ty: SettingType,
    pub range: Option<SettingRange>,
    /// Whether the setting enters the configuration digest.
    pub acquisition_relevant: bool,
    /// Whether the setting may change while streaming.
    pub live_changeable: bool,
}

impl SettingSchema {
    /// The SI unit of a quantity setting; `None` for every other type.
    pub fn unit(&self) -> Option<SiUnit> {
        match self.ty {
            SettingType::Quantity(u) => Some(u),
            _ => None,
        }
    }

    /// Whether this is a vendor extension (`vendor.name`).
    pub fn is_extension(&self) -> bool {
        self.name.contains('.')
    }

    /// Whether the value of this setting enters the digest (REQ-002).
    pub fn is_digested(&self) -> bool {
        self.acquisition_relevant || self.is_extension()
    }

    /// The rules a declared setting must meet beyond its wire shape.
    fn check(&self) -> Result<(), ConfigError> {
        let bad = |reason| Err(ConfigError::invalid(&self.name, reason));
        if !valid_name(&self.name) {
            return bad(ConfigReason::BadName);
        }
        if let SettingType::Enum(values) = &self.ty {
            let well_formed = values.iter().all(|v| !v.is_empty() && v.len() <= MAX_ENUM_VALUE_BYTES)
                && values.iter().all(|v| !v.chars().any(char::is_control));
            let mut sorted: Vec<&String> = values.iter().collect();
            sorted.sort();
            sorted.dedup();
            if values.is_empty() || values.len() > MAX_ENUM_VALUES || !well_formed || sorted.len() != values.len() {
                return bad(ConfigReason::Values);
            }
        }
        let range_ok = match (&self.ty, self.range) {
            (_, None) => true,
            (SettingType::Int, Some(SettingRange::Int { min, max })) => min <= max,
            (SettingType::Quantity(_), Some(SettingRange::Quantity { min, max })) => {
                min.is_finite() && max.is_finite() && min <= max
            }
            _ => false,
        };
        if !range_ok {
            return bad(ConfigReason::Range);
        }
        if self.is_extension() && !self.acquisition_relevant {
            return bad(ConfigReason::ExtensionFlag);
        }
        Ok(())
    }

    /// Reads one setting of the wire form, then checks it.
    fn from_value(v: CValue) -> Result<SettingSchema, ConfigError> {
        let named = match &v {
            CValue::Obj(o) => match o.get("name") {
                Some(CValue::Str(s)) => s.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        let shape = |_| ConfigError::invalid(&named, ConfigReason::Schema);
        let mut o = object(v, "setting", &SETTING_FIELDS).map_err(shape)?;
        let name = string("name", take(&mut o, "name").map_err(shape)?).map_err(shape)?;
        let ty_name = take(&mut o, "type").map_err(shape)?;
        let unit = take(&mut o, "unit").map_err(shape)?;
        let values = take(&mut o, "values").map_err(shape)?;
        let range = take(&mut o, "range").map_err(shape)?;
        let acquisition_relevant =
            boolean("acquisition_relevant", take(&mut o, "acquisition_relevant").map_err(shape)?).map_err(shape)?;
        let live_changeable =
            boolean("live_changeable", take(&mut o, "live_changeable").map_err(shape)?).map_err(shape)?;

        let err = |reason| ConfigError::invalid(&name, reason);
        let ty_name = match ty_name {
            CValue::Str(s) => s,
            _ => return Err(err(ConfigReason::Type)),
        };
        let unit = match unit {
            CValue::Null => None,
            CValue::Str(s) => Some(SiUnit::parse(&s).map_err(|_| err(ConfigReason::Unit))?),
            _ => return Err(err(ConfigReason::Unit)),
        };
        let ty = match (ty_name.as_str(), unit) {
            ("quantity", Some(u)) => SettingType::Quantity(u),
            ("quantity", None) => return Err(err(ConfigReason::Unit)),
            ("bool" | "int" | "enum" | "text", Some(_)) => return Err(err(ConfigReason::Unit)),
            ("bool", None) => SettingType::Bool,
            ("int", None) => SettingType::Int,
            ("enum", None) => SettingType::Enum(Vec::new()),
            ("text", None) => SettingType::Text,
            _ => return Err(err(ConfigReason::Type)),
        };
        let ty = match (ty, values) {
            (SettingType::Enum(_), CValue::Arr(items)) => {
                if items.len() > MAX_ENUM_VALUES {
                    return Err(err(ConfigReason::Values));
                }
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    match item {
                        CValue::Str(s) => out.push(s),
                        _ => return Err(err(ConfigReason::Values)),
                    }
                }
                SettingType::Enum(out)
            }
            (SettingType::Enum(_), _) => return Err(err(ConfigReason::Values)),
            (ty, CValue::Null) => ty,
            (_, _) => return Err(err(ConfigReason::Values)),
        };
        let range = match range {
            CValue::Null => None,
            r => Some(read_range(&ty, r).ok_or_else(|| err(ConfigReason::Range))?),
        };
        let s = SettingSchema { name, ty, range, acquisition_relevant, live_changeable };
        s.check()?;
        Ok(s)
    }
}

fn read_range(ty: &SettingType, v: CValue) -> Option<SettingRange> {
    let mut o = object(v, "range", &["max", "min"]).ok()?;
    let min = take(&mut o, "min").ok()?;
    let max = take(&mut o, "max").ok()?;
    match ty {
        SettingType::Int => Some(SettingRange::Int { min: i64_dec("min", min).ok()?, max: i64_dec("max", max).ok()? }),
        SettingType::Quantity(_) => {
            Some(SettingRange::Quantity { min: f64_bits("min", min).ok()?, max: f64_bits("max", max).ok()? })
        }
        _ => None,
    }
}

/// `[a-z][a-z0-9_]*`, optionally followed by one `.` and a second such segment.
fn valid_name(name: &str) -> bool {
    fn segment(s: &str) -> bool {
        let mut b = s.bytes();
        matches!(b.next(), Some(b'a'..=b'z')) && b.all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'_'))
    }
    if name.len() > MAX_NAME_BYTES {
        return false;
    }
    match name.split_once('.') {
        None => segment(name),
        Some((vendor, rest)) => segment(vendor) && segment(rest),
    }
}

/// The declared settings of a device, base and vendor extensions alike, keyed by name.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigSchema {
    settings: BTreeMap<String, SettingSchema>,
}

impl ConfigSchema {
    /// A schema from declared settings, checked in order: names, enum values, ranges and
    /// the extension flag of each, then uniqueness.
    pub fn new(settings: Vec<SettingSchema>) -> Result<ConfigSchema, ConfigError> {
        if settings.len() > MAX_SETTINGS {
            return Err(ConfigError::invalid("", ConfigReason::TooMany));
        }
        let mut map = BTreeMap::new();
        for s in settings {
            s.check()?;
            if map.contains_key(&s.name) {
                return Err(ConfigError::invalid(&s.name, ConfigReason::Duplicate));
            }
            map.insert(s.name.clone(), s);
        }
        Ok(ConfigSchema { settings: map })
    }

    /// Parses and checks a `config-schema/0` document.
    pub fn from_json(input: &[u8]) -> Result<ConfigSchema, ConfigError> {
        ConfigSchema::from_value(parse(input)?)
    }

    /// [`ConfigSchema::from_json`] for an already parsed value.
    pub fn from_value(v: CValue) -> Result<ConfigSchema, ConfigError> {
        let doc = || ConfigError::invalid("", ConfigReason::Schema);
        let mut o = object(v, "config schema", &["schema", "settings"]).map_err(|_| doc())?;
        if take(&mut o, "schema").map_err(|_| doc())? != CValue::Str(SCHEMA_TAG.to_string()) {
            return Err(doc());
        }
        let CValue::Arr(items) = take(&mut o, "settings").map_err(|_| doc())? else {
            return Err(doc());
        };
        if items.len() > MAX_SETTINGS {
            return Err(ConfigError::invalid("", ConfigReason::TooMany));
        }
        let mut map = BTreeMap::new();
        for item in items {
            let s = SettingSchema::from_value(item)?;
            if map.contains_key(&s.name) {
                return Err(ConfigError::invalid(&s.name, ConfigReason::Duplicate));
            }
            map.insert(s.name.clone(), s);
        }
        Ok(ConfigSchema { settings: map })
    }

    /// The declared setting with this name.
    pub fn get(&self, name: &str) -> Option<&SettingSchema> {
        self.settings.get(name)
    }

    /// Every declared setting, in name order.
    pub fn settings(&self) -> impl Iterator<Item = &SettingSchema> {
        self.settings.values()
    }
}

// ------------------------------------------------------------------ configuration

/// The value of one setting.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
    Quantity(Quantity),
    Enum(String),
    Text(String),
}

impl SettingValue {
    /// The wire value.
    pub fn to_value(&self) -> CValue {
        match self {
            SettingValue::Bool(b) => CValue::Bool(*b),
            SettingValue::Int(i) => CValue::I64(*i),
            SettingValue::Quantity(q) => plus_zero(*q).to_value(),
            SettingValue::Enum(s) | SettingValue::Text(s) => CValue::Str(s.clone()),
        }
    }

    /// Reads the wire value of a setting of type `ty`; range, enum and text rules are
    /// checked separately by [`check_value`].
    fn from_value(setting: &SettingSchema, v: CValue) -> Result<SettingValue, ConfigError> {
        let name = setting.name.as_str();
        let wrong_type = || ConfigError::invalid(name, ConfigReason::WrongType);
        match &setting.ty {
            SettingType::Bool => match v {
                CValue::Bool(b) => Ok(SettingValue::Bool(b)),
                _ => Err(wrong_type()),
            },
            SettingType::Int => i64_dec("value", v).map(SettingValue::Int).map_err(|e| units_error(name, &e)),
            SettingType::Quantity(_) => {
                let q = Quantity::from_value(v).map_err(|e| units_error(name, &e))?;
                Ok(SettingValue::Quantity(plus_zero(q)))
            }
            SettingType::Enum(_) => match v {
                CValue::Str(s) => Ok(SettingValue::Enum(s)),
                _ => Err(wrong_type()),
            },
            SettingType::Text => match v {
                CValue::Str(s) => Ok(SettingValue::Text(s)),
                _ => Err(wrong_type()),
            },
        }
    }
}

/// `-0.0` as `+0.0`; every other value unchanged.
fn plus_zero(q: Quantity) -> Quantity {
    if q.value() == 0.0 {
        Quantity::new(0.0, q.unit()).unwrap_or(q)
    } else {
        q
    }
}

fn units_error(setting: &str, e: &UnitsError) -> ConfigError {
    let reason = match e {
        UnitsError::UnitUnsupported(_) => ConfigReason::WrongUnit,
        UnitsError::QuantityNotFinite => ConfigReason::NotFinite,
        UnitsError::BadType { .. } => ConfigReason::WrongType,
        _ => ConfigReason::BadValue,
    };
    ConfigError::invalid(setting, reason)
}

/// Checks a typed value against its declared setting: type, unit, enum values, text bounds
/// and range (REQ-004).
fn check_value(setting: &SettingSchema, value: &SettingValue) -> Result<(), ConfigError> {
    let name = setting.name.as_str();
    let bad = |reason| Err(ConfigError::invalid(name, reason));
    match (&setting.ty, value) {
        (SettingType::Bool, SettingValue::Bool(_)) => Ok(()),
        (SettingType::Int, SettingValue::Int(i)) => match setting.range {
            Some(SettingRange::Int { min, max }) if !(min..=max).contains(i) => Err(ConfigError::out_of_range(name)),
            _ => Ok(()),
        },
        (SettingType::Quantity(unit), SettingValue::Quantity(q)) => {
            let q = plus_zero(*q);
            if q.unit() != *unit {
                return bad(ConfigReason::WrongUnit);
            }
            if !q.value().is_finite() {
                return bad(ConfigReason::NotFinite);
            }
            match setting.range {
                Some(SettingRange::Quantity { min, max }) if !(min..=max).contains(&q.value()) => {
                    Err(ConfigError::out_of_range(name))
                }
                _ => Ok(()),
            }
        }
        (SettingType::Enum(values), SettingValue::Enum(v)) => {
            if values.contains(v) {
                Ok(())
            } else {
                bad(ConfigReason::Enum)
            }
        }
        (SettingType::Text, SettingValue::Text(t)) => {
            if t.chars().count() > MAX_TEXT_CHARS {
                bad(ConfigReason::TooLong)
            } else if t.chars().any(char::is_control) {
                bad(ConfigReason::BadValue)
            } else {
                Ok(())
            }
        }
        _ => bad(ConfigReason::WrongType),
    }
}

/// One value for every declared setting, keyed by name.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Configuration {
    pub values: BTreeMap<String, SettingValue>,
}

impl Configuration {
    /// Parses a `configuration/0` document against `schema` and validates it.
    pub fn from_json(schema: &ConfigSchema, input: &[u8]) -> Result<Configuration, ConfigError> {
        Configuration::from_value(schema, parse(input)?)
    }

    /// [`Configuration::from_json`] for an already parsed value.
    pub fn from_value(schema: &ConfigSchema, v: CValue) -> Result<Configuration, ConfigError> {
        let doc = || ConfigError::invalid("", ConfigReason::Schema);
        let mut o = object(v, "configuration", &["schema", "values"]).map_err(|_| doc())?;
        if take(&mut o, "schema").map_err(|_| doc())? != CValue::Str(CONFIGURATION_TAG.to_string()) {
            return Err(doc());
        }
        let CValue::Obj(raw) = take(&mut o, "values").map_err(|_| doc())? else {
            return Err(doc());
        };
        if raw.len() > MAX_SETTINGS {
            return Err(ConfigError::invalid("", ConfigReason::TooMany));
        }
        // One walk over declared and given names in name order, so the first failure is the
        // same one `validate` reports for the typed configuration.
        let mut values = BTreeMap::new();
        for (setting, v) in in_name_order(schema, raw)? {
            let value = SettingValue::from_value(setting, v)?;
            check_value(setting, &value)?;
            values.insert(setting.name.clone(), value);
        }
        Ok(Configuration { values })
    }

    /// The wire value.
    pub fn to_value(&self) -> CValue {
        let values: Obj = self.values.iter().map(|(k, v)| (k.clone(), v.to_value())).collect();
        CValue::Obj(BTreeMap::from([
            ("schema".to_string(), CValue::Str(CONFIGURATION_TAG.to_string())),
            ("values".to_string(), CValue::Obj(values)),
        ]))
    }
}

/// Pairs each raw value with its declared setting, failing on the first undeclared or
/// missing name in name order.
fn in_name_order(schema: &ConfigSchema, mut raw: Obj) -> Result<Vec<(&SettingSchema, CValue)>, ConfigError> {
    let mut names: Vec<String> = raw.keys().chain(schema.settings.keys()).cloned().collect();
    names.sort();
    names.dedup();
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let Some(setting) = schema.get(&name) else {
            return Err(ConfigError::invalid(&name, ConfigReason::Undeclared));
        };
        let Some(v) = raw.remove(&name) else {
            return Err(ConfigError::invalid(&name, ConfigReason::Missing));
        };
        out.push((setting, v));
    }
    Ok(out)
}

fn parse(input: &[u8]) -> Result<CValue, ConfigError> {
    fid_canon::parse_with_limits(input, MAX_INPUT_BYTES, MAX_DEPTH)
        .map_err(|_| ConfigError::invalid("", ConfigReason::Json))
}

/// Validates a configuration against its schema (REQ-004): every declared setting has a
/// value of its type, unit, enum values and range; no undeclared setting, including an
/// extension outside its vendor's declared names, is present. The first failing setting in
/// name order is reported.
pub fn validate(schema: &ConfigSchema, cfg: &Configuration) -> Result<(), ConfigError> {
    if cfg.values.len() > MAX_SETTINGS {
        return Err(ConfigError::invalid("", ConfigReason::TooMany));
    }
    let mut names: Vec<&String> = cfg.values.keys().chain(schema.settings.keys()).collect();
    names.sort();
    names.dedup();
    for name in names {
        let Some(setting) = schema.get(name) else {
            return Err(ConfigError::invalid(name, ConfigReason::Undeclared));
        };
        let Some(value) = cfg.values.get(name) else {
            return Err(ConfigError::invalid(name, ConfigReason::Missing));
        };
        check_value(setting, value)?;
    }
    Ok(())
}

// ------------------------------------------------------------------ digest

/// The configuration digest a stamp carries in `config_digest` (REQ-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfigDigest(Digest);

impl ConfigDigest {
    /// The shared `sha256:` digest.
    pub fn digest(&self) -> Digest {
        self.0
    }

    /// The wire form, `sha256:` followed by 64 lowercase hex digits.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for ConfigDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The RFC 8785 canonical bytes the digest is taken over: the JSON object mapping every
/// acquisition-relevant or extension setting name to its wire value (keys sorted by RFC
/// 8785). Validates first.
pub fn digest_canonical_bytes(schema: &ConfigSchema, cfg: &Configuration) -> Result<Vec<u8>, ConfigError> {
    validate(schema, cfg)?;
    let subset: Obj = cfg
        .values
        .iter()
        .filter(|(name, _)| schema.get(name).is_some_and(SettingSchema::is_digested))
        .map(|(name, v)| (name.clone(), v.to_value()))
        .collect();
    fid_canon::to_canonical_bytes(&CValue::Obj(subset)).map_err(|_| ConfigError::invalid("", ConfigReason::Json))
}

/// SHA-256 over [`digest_canonical_bytes`] (REQ-002, REQ-003). Validates first.
pub fn config_digest(schema: &ConfigSchema, cfg: &Configuration) -> Result<ConfigDigest, ConfigError> {
    let bytes = digest_canonical_bytes(schema, cfg)?;
    Ok(ConfigDigest(digest_of(&bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for ok in ["exposure", "a", "a_1", "acme.gain", "x9.y_"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["", "Exposure", "_a", "1a", "a.", ".a", "a.b.c", "a-b", "a b", "acme.Gain"] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert!(!valid_name(&"a".repeat(MAX_NAME_BYTES + 1)));
        assert!(valid_name(&"a".repeat(MAX_NAME_BYTES)));
    }

    #[test]
    fn codes_and_reasons_are_stable() {
        assert_eq!(ConfigErrorCode::ConfigInvalid.as_str(), "CONFIG_INVALID");
        assert_eq!(ConfigErrorCode::OutOfRange.as_str(), "OUT_OF_RANGE");
        assert_eq!(ConfigReason::ExtensionFlag.as_str(), "extension_flag");
        assert_eq!(ConfigReason::Undeclared.as_str(), "undeclared");
    }

    #[test]
    fn error_names_are_bounded() {
        let long = "z".repeat(500);
        let e = ConfigError::invalid(&long, ConfigReason::Undeclared);
        assert_eq!(e.setting.len(), 64);
    }

    #[test]
    fn oversized_input_is_refused() {
        let big = vec![b' '; MAX_INPUT_BYTES + 1];
        assert_eq!(ConfigSchema::from_json(&big).err().map(|e| e.reason), Some(ConfigReason::Json));
    }
}

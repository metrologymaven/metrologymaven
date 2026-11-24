//! The `Device@1` capability package (spec node `HAL.IDL.BASE.DESC`): its seven operations
//! with typed requests, responses, allowed error codes and side-effect classes; the
//! [`DeviceInstance`] descriptor; read-only [`CalibrationState`] references; and the
//! `Device-core@1` conformance profile (the list of items, not their execution).
//!
//! - **REQ-HAL.IDL.BASE.DESC-001:** [`Operation::ALL`] holds exactly `describe`,
//!   `open_session`, `close_session`, `get_health`, `get_configuration`, `set_configuration`
//!   and `get_calibration_state`. `describe` and the three `get_*` operations are R0; the
//!   others are D1 (CONSTITUTION §3).
//! - **REQ-HAL.IDL.BASE.DESC-002:** every device-supplied string of [`DeviceInstance`] and
//!   [`CalibrationState`] is a [`DeviceText`]: an [`crate::UntrustedText`] cut at
//!   [`MAX_MESSAGE_BYTES`] bytes on a UTF-8 boundary and marked `truncated`. No field carries
//!   operator guidance as free text.
//! - **REQ-HAL.IDL.BASE.DESC-003:** no operation writes calibration, qualification or
//!   compensation data; `get_calibration_state` returns references and validity dates only.
//! - **REQ-HAL.IDL.BASE.DESC-004:** [`DEVICE_CORE`] lists the six `Device-core@1` items, each
//!   with a stable ID (`DC-01` .. `DC-06`).
//! - **REQ-HAL.IDL.BASE.DESC-005:** [`DevicePackage`] implements [`CapabilityPackage`], so the
//!   package exports as `capability-package/v1` with a platform-independent digest.
//!
//! **Wire forms.** Every record is a JSON object with a closed key set; unknown or duplicate
//! keys are refused. An absent optional field is an absent key, never `null`, because the
//! `capability-package/v1` schema allowlist has no union types. Parsing is bounded: at most
//! [`MAX_RECORD_BYTES`] bytes, depth [`MAX_RECORD_DEPTH`] and [`MAX_RECORD_ELEMENTS`] JSON
//! elements, checked before any value is built.

use crate::base::config::{ConfigDigest, Configuration};
use crate::base::error::{DeviceText, ErrorCode, MAX_MESSAGE_BYTES};
use crate::base::health::HealthState;
use crate::package::{
    CapabilityPackage, ConformanceItemDesc, ConformanceProfileDesc, OperationDesc, PackageDescription, PackageManifest,
    SemanticsNote,
};
use fid_canon::CValue;
use fid_types::{Digest, SideEffect};
use fid_uuid::Uuid;
use std::collections::BTreeMap;
use std::fmt;

/// Package name in the manifest.
pub const PACKAGE_NAME: &str = "Device";

/// Package version in the manifest (major 1).
pub const PACKAGE_VERSION: &str = "1.0.0";

/// ID of the conformance profile.
pub const PROFILE_ID: &str = "Device-core@1";

/// Largest accepted [`DeviceInstance`] or [`CalibrationState`] JSON document (64 KiB). It is
/// larger than the kept text so an over-long device string reaches the truncation rule.
pub const MAX_RECORD_BYTES: usize = 64 * 1024;

/// Deepest accepted JSON nesting of a record.
pub const MAX_RECORD_DEPTH: usize = 4;

/// Most JSON elements (values plus object keys) in one record.
pub const MAX_RECORD_ELEMENTS: usize = 512;

/// Longest accepted driver ID, in bytes.
pub const MAX_DRIVER_ID_BYTES: usize = 128;

/// Most DCC digests in one [`CalibrationState`].
pub const MAX_DCC_DIGESTS: usize = 64;

// ------------------------------------------------------------------ errors

/// A [`DeviceInstance`] or [`CalibrationState`] wire value that is not valid
/// (`device_record_invalid`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRecordError {
    /// RFC 6901 JSON pointer to the offending value (`""` for the whole document).
    pub pointer: String,
    /// Which rule failed: `json`, `not_object`, `field_missing`, `field_unknown`,
    /// `wrong_type`, `driver_id_invalid`, `digest_invalid`, `digests_not_ascending`,
    /// `too_many_digests` or `date_invalid`.
    pub reason: &'static str,
}

impl DeviceRecordError {
    fn new(pointer: impl Into<String>, reason: &'static str) -> DeviceRecordError {
        DeviceRecordError { pointer: pointer.into(), reason }
    }

    /// Stable machine-readable error code.
    pub const fn code(&self) -> &'static str {
        "device_record_invalid"
    }
}

impl fmt::Display for DeviceRecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "device_record_invalid{{{}}}: {}", self.pointer, self.reason)
    }
}

impl std::error::Error for DeviceRecordError {}

// ------------------------------------------------------------------ small value types

/// The ID of the driver serving a device: 1 to [`MAX_DRIVER_ID_BYTES`] bytes of
/// `[a-z0-9._-]`, starting with a lowercase letter or digit, without `..`. It is assigned by
/// the driver package, not by the device, so it is checked rather than truncated.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DriverId(String);

impl DriverId {
    /// Checks a driver ID.
    pub fn parse(s: &str) -> Result<DriverId, DeviceRecordError> {
        let ok = !s.is_empty()
            && s.len() <= MAX_DRIVER_ID_BYTES
            && !s.contains("..")
            && s.as_bytes().first().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'));
        if ok {
            Ok(DriverId(s.to_string()))
        } else {
            Err(DeviceRecordError::new("/driver_id", "driver_id_invalid"))
        }
    }

    /// The ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A calendar date `YYYY-MM-DD` (proleptic Gregorian, years 0001 to 9999). Ordering is
/// chronological.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CalDate(String);

impl CalDate {
    /// Checks a date in the exact form `YYYY-MM-DD`.
    pub fn parse(s: &str) -> Option<CalDate> {
        let b = s.as_bytes();
        if b.len() != 10 || b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
            return None;
        }
        let num = |from: usize, to: usize| -> Option<u32> {
            let part = b.get(from..to)?;
            if !part.iter().all(u8::is_ascii_digit) {
                return None;
            }
            Some(part.iter().fold(0u32, |acc, d| acc * 10 + u32::from(d - b'0')))
        };
        let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        if year == 0 || day == 0 || day > days {
            return None;
        }
        Some(CalDate(s.to_string()))
    }

    /// The date text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ------------------------------------------------------------------ DeviceInstance

/// The device as it describes itself, plus the driver serving it (REQ-002). `make`, `model`,
/// `serial` and `firmware` are device-supplied and untrusted; each is bounded by
/// [`DeviceText::new`].
///
/// Wire form: `{"driver_id": <string>, "make": <DeviceText>, "model": <DeviceText>,
/// "serial"?: <DeviceText>, "firmware"?: <DeviceText>}` where a `DeviceText` is
/// `{"bytes": <string>, "truncated": <bool>}`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceInstance {
    pub make: DeviceText,
    pub model: DeviceText,
    pub serial: Option<DeviceText>,
    pub firmware: Option<DeviceText>,
    pub driver_id: DriverId,
}

/// Keys of a `DeviceText` wire object.
const TEXT_FIELDS: [&str; 2] = ["bytes", "truncated"];

const INSTANCE_FIELDS: [&str; 5] = ["driver_id", "firmware", "make", "model", "serial"];

impl DeviceInstance {
    /// Builds an instance from raw device strings, bounding each by [`DeviceText::new`].
    pub fn new(make: &str, model: &str, serial: Option<&str>, firmware: Option<&str>, driver_id: DriverId) -> Self {
        DeviceInstance {
            make: DeviceText::new(make),
            model: DeviceText::new(model),
            serial: serial.map(DeviceText::new),
            firmware: firmware.map(DeviceText::new),
            driver_id,
        }
    }

    /// The wire value.
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("driver_id".to_string(), CValue::Str(self.driver_id.as_str().to_string()));
        m.insert("make".to_string(), self.make.to_value());
        m.insert("model".to_string(), self.model.to_value());
        if let Some(t) = &self.serial {
            m.insert("serial".to_string(), t.to_value());
        }
        if let Some(t) = &self.firmware {
            m.insert("firmware".to_string(), t.to_value());
        }
        CValue::Obj(m)
    }

    /// Parses and checks the wire value from JSON bytes, within the record bounds.
    pub fn from_json(bytes: &[u8]) -> Result<DeviceInstance, DeviceRecordError> {
        DeviceInstance::from_value(parse_record(bytes)?)
    }

    /// Checks the wire value. A text longer than [`MAX_MESSAGE_BYTES`] bytes is cut and
    /// marked `truncated`, not refused.
    pub fn from_value(v: CValue) -> Result<DeviceInstance, DeviceRecordError> {
        let mut o = Fields::new(v, "", &INSTANCE_FIELDS)?;
        let driver_id = match o.take("driver_id")? {
            CValue::Str(s) => DriverId::parse(&s)?,
            _ => return Err(DeviceRecordError::new("/driver_id", "wrong_type")),
        };
        Ok(DeviceInstance {
            make: o.text("make")?,
            model: o.text("model")?,
            serial: o.opt_text("serial")?,
            firmware: o.opt_text("firmware")?,
            driver_id,
        })
    }
}

// ------------------------------------------------------------------ CalibrationState

/// The device's calibration state, as references and validity dates only (REQ-003). Nothing
/// in this package writes it.
///
/// Wire form: `{"dcc_digests": [<sha256:...>, ...], "qualification_ref"?: <DeviceText>,
/// "compensation_id"?: <DeviceText>, "last_interim_check"?: <YYYY-MM-DD>,
/// "valid_until"?: <YYYY-MM-DD>}`. `dcc_digests` is strictly ascending and holds at most
/// [`MAX_DCC_DIGESTS`] digests of Digital Calibration Certificates.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CalibrationState {
    pub dcc_digests: Vec<Digest>,
    pub qualification_ref: Option<DeviceText>,
    pub compensation_id: Option<DeviceText>,
    pub last_interim_check: Option<CalDate>,
    pub valid_until: Option<CalDate>,
}

const CALIBRATION_FIELDS: [&str; 5] =
    ["compensation_id", "dcc_digests", "last_interim_check", "qualification_ref", "valid_until"];

impl CalibrationState {
    /// The wire value.
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert(
            "dcc_digests".to_string(),
            CValue::Arr(self.dcc_digests.iter().map(|d| CValue::Str(d.as_str().to_string())).collect()),
        );
        if let Some(t) = &self.qualification_ref {
            m.insert("qualification_ref".to_string(), t.to_value());
        }
        if let Some(t) = &self.compensation_id {
            m.insert("compensation_id".to_string(), t.to_value());
        }
        if let Some(d) = &self.last_interim_check {
            m.insert("last_interim_check".to_string(), CValue::Str(d.as_str().to_string()));
        }
        if let Some(d) = &self.valid_until {
            m.insert("valid_until".to_string(), CValue::Str(d.as_str().to_string()));
        }
        CValue::Obj(m)
    }

    /// Parses and checks the wire value from JSON bytes, within the record bounds.
    pub fn from_json(bytes: &[u8]) -> Result<CalibrationState, DeviceRecordError> {
        CalibrationState::from_value(parse_record(bytes)?)
    }

    /// Checks the wire value.
    pub fn from_value(v: CValue) -> Result<CalibrationState, DeviceRecordError> {
        let mut o = Fields::new(v, "", &CALIBRATION_FIELDS)?;
        let CValue::Arr(items) = o.take("dcc_digests")? else {
            return Err(DeviceRecordError::new("/dcc_digests", "wrong_type"));
        };
        if items.len() > MAX_DCC_DIGESTS {
            return Err(DeviceRecordError::new("/dcc_digests", "too_many_digests"));
        }
        let mut dcc_digests: Vec<Digest> = Vec::with_capacity(items.len());
        for (i, item) in items.into_iter().enumerate() {
            let ptr = format!("/dcc_digests/{i}");
            let CValue::Str(s) = item else {
                return Err(DeviceRecordError::new(ptr, "wrong_type"));
            };
            let d = Digest::parse(&s).map_err(|_| DeviceRecordError::new(ptr.clone(), "digest_invalid"))?;
            if dcc_digests.last().is_some_and(|prev| *prev >= d) {
                return Err(DeviceRecordError::new(ptr, "digests_not_ascending"));
            }
            dcc_digests.push(d);
        }
        Ok(CalibrationState {
            dcc_digests,
            qualification_ref: o.opt_text("qualification_ref")?,
            compensation_id: o.opt_text("compensation_id")?,
            last_interim_check: o.opt_date("last_interim_check")?,
            valid_until: o.opt_date("valid_until")?,
        })
    }
}

// ------------------------------------------------------------------ operations

/// One operation of the `Device@1` package (REQ-001). The typed request and response of each
/// are the structs named in its documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Operation {
    /// [`DescribeRequest`] -> [`DescribeResponse`].
    Describe,
    /// [`OpenSessionRequest`] -> [`OpenSessionResponse`].
    OpenSession,
    /// [`CloseSessionRequest`] -> [`CloseSessionResponse`].
    CloseSession,
    /// [`GetHealthRequest`] -> [`GetHealthResponse`].
    GetHealth,
    /// [`GetConfigurationRequest`] -> [`GetConfigurationResponse`].
    GetConfiguration,
    /// [`SetConfigurationRequest`] -> [`SetConfigurationResponse`].
    SetConfiguration,
    /// [`GetCalibrationStateRequest`] -> [`GetCalibrationStateResponse`].
    GetCalibrationState,
}

use ErrorCode::{
    CalibrationExpired, CommLost, ConfigInvalid, Internal, Limit, NotQualified, OutOfRange, SafetyStopObserved,
    Transient, Unsupported, VendorLicense,
};

impl Operation {
    /// Every operation, in package order.
    pub const ALL: [Operation; 7] = [
        Operation::Describe,
        Operation::OpenSession,
        Operation::CloseSession,
        Operation::GetHealth,
        Operation::GetConfiguration,
        Operation::SetConfiguration,
        Operation::GetCalibrationState,
    ];

    /// The wire name.
    pub const fn name(self) -> &'static str {
        match self {
            Operation::Describe => "describe",
            Operation::OpenSession => "open_session",
            Operation::CloseSession => "close_session",
            Operation::GetHealth => "get_health",
            Operation::GetConfiguration => "get_configuration",
            Operation::SetConfiguration => "set_configuration",
            Operation::GetCalibrationState => "get_calibration_state",
        }
    }

    /// The side-effect class (CONSTITUTION §3): R0 for `describe` and the three reads, D1
    /// for the operations that change device or session state.
    pub const fn class(self) -> SideEffect {
        match self {
            Operation::Describe
            | Operation::GetHealth
            | Operation::GetConfiguration
            | Operation::GetCalibrationState => SideEffect::R0,
            Operation::OpenSession | Operation::CloseSession | Operation::SetConfiguration => SideEffect::D1,
        }
    }

    /// The `Device@1` error codes the operation may return, in error-table order.
    pub const fn errors(self) -> &'static [ErrorCode] {
        match self {
            Operation::Describe | Operation::CloseSession | Operation::GetHealth | Operation::GetConfiguration => {
                &[Transient, CommLost, Internal]
            }
            Operation::OpenSession => &[
                Transient,
                CommLost,
                NotQualified,
                CalibrationExpired,
                SafetyStopObserved,
                VendorLicense,
                Unsupported,
                Internal,
            ],
            Operation::SetConfiguration => {
                &[Transient, CommLost, ConfigInvalid, OutOfRange, Limit, Unsupported, Internal]
            }
            Operation::GetCalibrationState => &[Transient, CommLost, Unsupported, Internal],
        }
    }

    /// The request type schema (a `capability-package/v1` fragment).
    pub fn request_schema(self) -> CValue {
        match self {
            Operation::Describe => object(&[], &[]),
            Operation::SetConfiguration => object(
                &[("configuration", def_ref("Configuration")), ("session_id", def_ref("SessionId"))],
                &["configuration", "session_id"],
            ),
            _ => object(&[("session_id", def_ref("SessionId"))], &["session_id"]),
        }
    }

    /// The response type schema (a `capability-package/v1` fragment).
    pub fn response_schema(self) -> CValue {
        match self {
            Operation::Describe => object(&[("instance", def_ref("DeviceInstance"))], &["instance"]),
            Operation::OpenSession => object(
                &[("health", def_ref("HealthState")), ("session_id", def_ref("SessionId"))],
                &["health", "session_id"],
            ),
            Operation::CloseSession | Operation::GetHealth => {
                object(&[("health", def_ref("HealthState"))], &["health"])
            }
            Operation::GetConfiguration => object(
                &[("config_digest", def_ref("Digest")), ("configuration", def_ref("Configuration"))],
                &["config_digest", "configuration"],
            ),
            Operation::SetConfiguration => object(&[("config_digest", def_ref("Digest"))], &["config_digest"]),
            Operation::GetCalibrationState => object(&[("calibration", def_ref("CalibrationState"))], &["calibration"]),
        }
    }

    /// The operation's `capability-package/v1` description.
    pub fn description(self) -> OperationDesc {
        OperationDesc {
            name: self.name().to_string(),
            class: self.class(),
            request: self.request_schema(),
            response: self.response_schema(),
            errors: self.errors().to_vec(),
            live_settings: Vec::new(),
        }
    }
}

/// Request of `describe`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DescribeRequest;

/// Response of `describe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescribeResponse {
    pub instance: DeviceInstance,
}

/// Request of `open_session`; the runtime assigns the session ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenSessionRequest {
    pub session_id: Uuid,
}

/// Response of `open_session`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenSessionResponse {
    pub session_id: Uuid,
    pub health: HealthState,
}

/// Request of `close_session`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseSessionRequest {
    pub session_id: Uuid,
}

/// Response of `close_session`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseSessionResponse {
    pub health: HealthState,
}

/// Request of `get_health`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetHealthRequest {
    pub session_id: Uuid,
}

/// Response of `get_health`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetHealthResponse {
    pub health: HealthState,
}

/// Request of `get_configuration`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetConfigurationRequest {
    pub session_id: Uuid,
}

/// Response of `get_configuration`: the `configuration/0` document and its digest.
#[derive(Debug, Clone, PartialEq)]
pub struct GetConfigurationResponse {
    pub configuration: Configuration,
    pub config_digest: ConfigDigest,
}

/// Request of `set_configuration`.
#[derive(Debug, Clone, PartialEq)]
pub struct SetConfigurationRequest {
    pub session_id: Uuid,
    pub configuration: Configuration,
}

/// Response of `set_configuration`: the digest of the configuration now in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetConfigurationResponse {
    pub config_digest: ConfigDigest,
}

/// Request of `get_calibration_state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetCalibrationStateRequest {
    pub session_id: Uuid,
}

/// Response of `get_calibration_state`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetCalibrationStateResponse {
    pub calibration: CalibrationState,
}

/// The wire value of a typed request or response, matching its operation's schema.
pub trait WireValue {
    /// The operation this type belongs to.
    const OPERATION: Operation;
    /// The wire value.
    fn to_value(&self) -> CValue;
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn uuid(u: &Uuid) -> CValue {
    CValue::Str(u.to_string())
}

fn health(h: HealthState) -> CValue {
    CValue::Str(h.as_str().to_string())
}

fn cfg_digest(d: &ConfigDigest) -> CValue {
    CValue::Str(d.as_str().to_string())
}

impl WireValue for DescribeRequest {
    const OPERATION: Operation = Operation::Describe;
    fn to_value(&self) -> CValue {
        obj([])
    }
}

impl WireValue for DescribeResponse {
    const OPERATION: Operation = Operation::Describe;
    fn to_value(&self) -> CValue {
        obj([("instance", self.instance.to_value())])
    }
}

impl WireValue for OpenSessionRequest {
    const OPERATION: Operation = Operation::OpenSession;
    fn to_value(&self) -> CValue {
        obj([("session_id", uuid(&self.session_id))])
    }
}

impl WireValue for OpenSessionResponse {
    const OPERATION: Operation = Operation::OpenSession;
    fn to_value(&self) -> CValue {
        obj([("health", health(self.health)), ("session_id", uuid(&self.session_id))])
    }
}

impl WireValue for CloseSessionRequest {
    const OPERATION: Operation = Operation::CloseSession;
    fn to_value(&self) -> CValue {
        obj([("session_id", uuid(&self.session_id))])
    }
}

impl WireValue for CloseSessionResponse {
    const OPERATION: Operation = Operation::CloseSession;
    fn to_value(&self) -> CValue {
        obj([("health", health(self.health))])
    }
}

impl WireValue for GetHealthRequest {
    const OPERATION: Operation = Operation::GetHealth;
    fn to_value(&self) -> CValue {
        obj([("session_id", uuid(&self.session_id))])
    }
}

impl WireValue for GetHealthResponse {
    const OPERATION: Operation = Operation::GetHealth;
    fn to_value(&self) -> CValue {
        obj([("health", health(self.health))])
    }
}

impl WireValue for GetConfigurationRequest {
    const OPERATION: Operation = Operation::GetConfiguration;
    fn to_value(&self) -> CValue {
        obj([("session_id", uuid(&self.session_id))])
    }
}

impl WireValue for GetConfigurationResponse {
    const OPERATION: Operation = Operation::GetConfiguration;
    fn to_value(&self) -> CValue {
        obj([("config_digest", cfg_digest(&self.config_digest)), ("configuration", self.configuration.to_value())])
    }
}

impl WireValue for SetConfigurationRequest {
    const OPERATION: Operation = Operation::SetConfiguration;
    fn to_value(&self) -> CValue {
        obj([("configuration", self.configuration.to_value()), ("session_id", uuid(&self.session_id))])
    }
}

impl WireValue for SetConfigurationResponse {
    const OPERATION: Operation = Operation::SetConfiguration;
    fn to_value(&self) -> CValue {
        obj([("config_digest", cfg_digest(&self.config_digest))])
    }
}

impl WireValue for GetCalibrationStateRequest {
    const OPERATION: Operation = Operation::GetCalibrationState;
    fn to_value(&self) -> CValue {
        obj([("session_id", uuid(&self.session_id))])
    }
}

impl WireValue for GetCalibrationStateResponse {
    const OPERATION: Operation = Operation::GetCalibrationState;
    fn to_value(&self) -> CValue {
        obj([("calibration", self.calibration.to_value())])
    }
}

// ------------------------------------------------------------------ conformance profile

/// One item of a conformance profile: a stable ID, a title and the checks it makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileItem {
    pub id: &'static str,
    pub title: &'static str,
    pub checks: &'static [&'static str],
}

impl ProfileItem {
    /// The one-line summary used in the package description: the title, a colon, then the
    /// checks joined by `"; "` and a final full stop.
    pub fn summary(&self) -> String {
        format!("{}: {}.", self.title, self.checks.join("; "))
    }
}

/// A conformance profile: an ID and its numbered items. Running it is `HAL.SIM.CONF`'s job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConformanceProfile {
    pub id: &'static str,
    pub items: &'static [ProfileItem],
}

impl ConformanceProfile {
    /// The profile's `capability-package/v1` description.
    pub fn description(&self) -> ConformanceProfileDesc {
        ConformanceProfileDesc {
            id: self.id.to_string(),
            items: self
                .items
                .iter()
                .map(|it| ConformanceItemDesc { id: it.id.to_string(), summary: it.summary(), fixture_digest: None })
                .collect(),
        }
    }
}

/// The `Device-core@1` conformance profile (REQ-004). Item IDs are stable: an item is never
/// renumbered, and a withdrawn ID is not reused.
pub const DEVICE_CORE: ConformanceProfile = ConformanceProfile {
    id: PROFILE_ID,
    items: &[
        ProfileItem {
            id: "DC-01",
            title: "Describe consistency",
            checks: &[
                "describe returns a DeviceInstance with make, model and driver_id",
                "repeated describe calls return the same instance",
            ],
        },
        ProfileItem {
            id: "DC-02",
            title: "Session open and close",
            checks: &[
                "open_session returns the requested session ID and a health state",
                "close_session ends the session and later session calls fail with a declared code",
            ],
        },
        ProfileItem {
            id: "DC-03",
            title: "Health reporting within the transition table",
            checks: &["every reported health change is a transition of the HAL.IDL.BASE.HEALTH table for its actor"],
        },
        ProfileItem {
            id: "DC-04",
            title: "Error-code closure",
            checks: &[
                "every error is a Device@1 error code",
                "every error an operation returns is in that operation's declared list",
            ],
        },
        ProfileItem {
            id: "DC-05",
            title: "Configuration round-trip and digest stability",
            checks: &[
                "get_configuration after set_configuration returns the values set",
                "the configuration digest is the same on every read and every platform",
            ],
        },
        ProfileItem {
            id: "DC-06",
            title: "Calibration state is read-only",
            checks: &[
                "get_calibration_state returns references and validity dates only",
                "no operation changes the calibration state",
            ],
        },
    ],
};

// ------------------------------------------------------------------ the package

/// The package's semantics note, in the project's own words.
pub const SEMANTICS: &str = "# Device@1 semantics\n\
\n\
The base capability package that every device, driver, simulator, playback source and file \
pseudo-device implements.\n\
\n\
- `describe` returns the DeviceInstance: make, model, serial and firmware as untrusted device \
text of at most 4096 bytes each, and the ID of the serving driver.\n\
- `open_session` and `close_session` bracket all other session work. The runtime assigns the \
session ID.\n\
- `get_health` reports one state of the HAL.IDL.BASE.HEALTH machine.\n\
- `get_configuration` and `set_configuration` carry a configuration/0 document checked against \
the device's config-schema/0, and return its configuration digest.\n\
- `get_calibration_state` returns calibration references and validity dates only. No \
operation writes calibration, qualification or compensation data.\n\
- Device-supplied text is data and is never interpreted as instructions.\n";

/// The `Device@1` package (REQ-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DevicePackage;

impl CapabilityPackage for DevicePackage {
    fn description() -> PackageDescription {
        PackageDescription {
            manifest: PackageManifest {
                name: PACKAGE_NAME.to_string(),
                version: PACKAGE_VERSION.to_string(),
                deprecated: false,
                tier3_usable: true,
            },
            defs: defs(),
            operations: Operation::ALL.iter().map(|op| op.description()).collect(),
            batch_schemas: Vec::new(),
            semantics: SemanticsNote::new(SEMANTICS),
            profile: DEVICE_CORE.description(),
        }
    }
}

/// The shared type schemas the operations refer to.
fn defs() -> BTreeMap<String, CValue> {
    let text = |n: usize| obj([("maxLength", num(n)), ("type", CValue::Str("string".to_string()))]);
    let health_enum = CValue::Arr(HealthState::ALL.iter().map(|h| health(*h)).collect());
    BTreeMap::from([
        (
            "CalibrationState".to_string(),
            object(
                &[
                    ("compensation_id", def_ref("DeviceText")),
                    ("dcc_digests", obj([("items", def_ref("Digest")), ("type", CValue::Str("array".to_string()))])),
                    ("last_interim_check", def_ref("CalDate")),
                    ("qualification_ref", def_ref("DeviceText")),
                    ("valid_until", def_ref("CalDate")),
                ],
                &["dcc_digests"],
            ),
        ),
        ("CalDate".to_string(), text(10)),
        ("Configuration".to_string(), obj([("type", CValue::Str("object".to_string()))])),
        (
            "DeviceInstance".to_string(),
            object(
                &[
                    ("driver_id", text(MAX_DRIVER_ID_BYTES)),
                    ("firmware", def_ref("DeviceText")),
                    ("make", def_ref("DeviceText")),
                    ("model", def_ref("DeviceText")),
                    ("serial", def_ref("DeviceText")),
                ],
                &["driver_id", "make", "model"],
            ),
        ),
        (
            "DeviceText".to_string(),
            object(
                &[
                    ("bytes", text(MAX_MESSAGE_BYTES)),
                    ("truncated", obj([("type", CValue::Str("boolean".to_string()))])),
                ],
                &["bytes", "truncated"],
            ),
        ),
        ("Digest".to_string(), text(71)),
        ("HealthState".to_string(), obj([("enum", health_enum), ("type", CValue::Str("string".to_string()))])),
        ("SessionId".to_string(), text(36)),
    ])
}

fn num(n: usize) -> CValue {
    // Small exact integers only (lengths); f64 holds them exactly.
    CValue::Num(n as f64)
}

fn def_ref(name: &str) -> CValue {
    obj([("$ref", CValue::Str(format!("#/$defs/{name}")))])
}

/// A closed object schema; `properties` and `required` are left out when empty.
fn object(props: &[(&str, CValue)], required: &[&str]) -> CValue {
    let mut m = BTreeMap::new();
    m.insert("additionalProperties".to_string(), CValue::Bool(false));
    m.insert("type".to_string(), CValue::Str("object".to_string()));
    if !props.is_empty() {
        m.insert(
            "properties".to_string(),
            CValue::Obj(props.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()),
        );
    }
    if !required.is_empty() {
        m.insert("required".to_string(), CValue::Arr(required.iter().map(|s| CValue::Str(s.to_string())).collect()));
    }
    CValue::Obj(m)
}

// ------------------------------------------------------------------ record parsing helpers

fn parse_record(bytes: &[u8]) -> Result<CValue, DeviceRecordError> {
    fid_canon::parse_with_budget(bytes, MAX_RECORD_BYTES, MAX_RECORD_DEPTH, MAX_RECORD_ELEMENTS)
        .map_err(|_| DeviceRecordError::new("", "json"))
}

/// The fields of one wire object, taken one by one; unknown keys are refused up front.
struct Fields {
    map: BTreeMap<String, CValue>,
    ptr: String,
}

impl Fields {
    fn new(v: CValue, ptr: &str, allowed: &[&str]) -> Result<Fields, DeviceRecordError> {
        let CValue::Obj(map) = v else {
            return Err(DeviceRecordError::new(ptr, "not_object"));
        };
        if let Some(k) = map.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(DeviceRecordError::new(format!("{ptr}/{}", escape(k)), "field_unknown"));
        }
        Ok(Fields { map, ptr: ptr.to_string() })
    }

    fn p(&self, k: &str) -> String {
        format!("{}/{k}", self.ptr)
    }

    fn take(&mut self, k: &str) -> Result<CValue, DeviceRecordError> {
        self.map.remove(k).ok_or_else(|| DeviceRecordError::new(self.p(k), "field_missing"))
    }

    /// A `DeviceText` wire object; errors inside it carry the inner pointer
    /// (`/<k>/bytes`, `/<k>/truncated` or `/<k>/<unknown key>`).
    fn text(&mut self, k: &str) -> Result<DeviceText, DeviceRecordError> {
        let ptr = self.p(k);
        let mut inner = Fields::new(self.take(k)?, &ptr, &TEXT_FIELDS)?;
        let bytes = inner.take("bytes")?;
        if !matches!(bytes, CValue::Str(_)) {
            return Err(DeviceRecordError::new(inner.p("bytes"), "wrong_type"));
        }
        let truncated = inner.take("truncated")?;
        if !matches!(truncated, CValue::Bool(_)) {
            return Err(DeviceRecordError::new(inner.p("truncated"), "wrong_type"));
        }
        let checked = CValue::Obj(BTreeMap::from([("bytes".to_string(), bytes), ("truncated".to_string(), truncated)]));
        DeviceText::from_value(checked).map_err(|_| DeviceRecordError::new(ptr, "wrong_type"))
    }

    fn opt_text(&mut self, k: &str) -> Result<Option<DeviceText>, DeviceRecordError> {
        if self.map.contains_key(k) {
            self.text(k).map(Some)
        } else {
            Ok(None)
        }
    }

    fn opt_date(&mut self, k: &str) -> Result<Option<CalDate>, DeviceRecordError> {
        match self.map.remove(k) {
            None => Ok(None),
            Some(CValue::Str(s)) => {
                CalDate::parse(&s).map(Some).ok_or_else(|| DeviceRecordError::new(self.p(k), "date_invalid"))
            }
            Some(_) => Err(DeviceRecordError::new(self.p(k), "wrong_type")),
        }
    }
}

fn escape(seg: &str) -> String {
    seg.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_follow_the_gregorian_calendar() {
        for ok in ["2026-02-28", "2024-02-29", "2000-02-29", "0001-01-01", "9999-12-31"] {
            assert!(CalDate::parse(ok).is_some(), "{ok}");
        }
        for bad in ["2026-02-29", "1900-02-29", "0000-01-01", "2026-13-01", "2026-04-31", "2026-1-01", "2026/01/01", ""]
        {
            assert!(CalDate::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn driver_ids_are_checked() {
        for ok in ["drv.example.linescan", "0driver", "a-b_c.d"] {
            assert!(DriverId::parse(ok).is_ok(), "{ok}");
        }
        let long = "a".repeat(MAX_DRIVER_ID_BYTES + 1);
        for bad in ["", "Drv", ".drv", "a..b", "a b", "drv/x", long.as_str()] {
            assert!(DriverId::parse(bad).is_err(), "{bad}");
        }
    }
}

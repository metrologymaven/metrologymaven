//! The shared streaming template, the vendor-extension rule and the `Stream-common@1`
//! conformance profile (spec node `HAL.IDL.SENS.STREAM`).
//!
//! Every sensor package (`Gauge@1`, `Environment@1`, `LineScan@1`, ...) is built with
//! [`StreamingPackage`]: the package defines its payloads, and the builder adds the parts all
//! of them share. Describe and configuration come from the `Device@1` base package that every
//! device implements (`HAL.IDL.BASE.DESC`), so a built package has describe, configure and
//! acquisition operations.
//!
//! - **REQ-HAL.IDL.SENS.STREAM-001:** [`StreamingPackage::build`] adds `start_stream` and
//!   `stop_stream` (D1) and `stream_status` (R0), the stream event, state and status type
//!   schemas, and the `Stream-common@1` items, to a `capability-package/v1` description.
//!   [`StreamingDescription::validate`] refuses a description that lacks any of them.
//! - **REQ-HAL.IDL.SENS.STREAM-002:** [`validate_extensions`] fails with
//!   `extension_undeclared` when a declared vendor-extension setting is not named
//!   `<namespace>.<name>` with `<namespace>` among the vendor namespaces the package lists
//!   in describe. [`check_config_schema`] applies the same rule to a device's
//!   `config-schema/0`.
//! - **REQ-HAL.IDL.SENS.STREAM-003:** [`check_batch_size`] (item `SC-01`) passes only if the
//!   recorded test stream lasts at least [`TEST_STREAM_NS`] and at least
//!   [`BATCH_PASS_PERCENT`] % of its batches hold [`BATCH_MIN_BYTES`] to [`BATCH_MAX_BYTES`]
//!   bytes or span at most [`BATCH_MAX_SPAN_NS`] of device time. The thresholds are
//!   proposals [unverified]; `MB` is read as 10^6 bytes.
//! - **REQ-HAL.IDL.SENS.STREAM-004:** [`check_start_stop_idempotence`] (`SC-02`),
//!   [`check_sequence_contiguity`] (`SC-03`) and [`check_extensions_digested`] (`SC-04`).
//!   [`STREAM_COMMON`] lists the four items with stable IDs.
//!
//! The evaluators are pure functions over recorded summaries; running a device to record them
//! is the conformance suite's job (`HAL.SIM.CONF`). Each fails closed: an empty recording, a
//! recording that does not exercise the rule, or anything it cannot read is a failure, never
//! a pass.
//!
//! **Where extensions live.** `capability-package/v1` (`HAL.IDL.GEN.FMT`) has no field for
//! vendor namespaces or extension settings, so a [`StreamingDescription`] carries them next to
//! the exported description. A dotted `live_settings` name in an operation is an extension
//! name and must be declared.
//!
//! **Wire forms** (JSON through `fid-canon`; closed key sets; `u64` values as canonical
//! decimal strings):
//!
//! - [`StreamStatus`]: `{"next_seq": "<u64>", "state": "idle" | "streaming"}`;
//! - [`StreamEvent`]: `{"kind": "started"}`, `{"kind": "stopped", "reason": <StopReason>}` or
//!   `{"kind": "gap", "from": "<u64>", "to": "<u64>"}` with `from <= to`;
//! - [`BatchRecord`]: `{"schema": "stream-batches/0", "origin": "synthetic" | "simulated" |
//!   "device", "duration_ns": "<u64>", "batches": [{"first_seq", "last_seq", "bytes",
//!   "span_ns"?}, ...]}`; `span_ns` is absent when the device gives no device time.

use crate::base::config::{config_digest, digest_canonical_bytes, ConfigDigest, ConfigSchema, Configuration};
use crate::base::config::{ConfigError, SettingSchema};
use crate::base::device::{ConformanceProfile, ProfileItem};
use crate::base::error::ErrorCode;
use crate::package::{OperationDesc, PackageDescription, PackageError};
use crate::units::kept;
use fid_canon::CValue;
use fid_types::SideEffect;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// ID of the conformance profile.
pub const PROFILE_ID: &str = "Stream-common@1";

/// Item `SC-01`: batch size within the recommendation (REQ-003).
pub const ITEM_BATCH_SIZE: &str = "SC-01";
/// Item `SC-02`: start and stop idempotence (REQ-004).
pub const ITEM_IDEMPOTENCE: &str = "SC-02";
/// Item `SC-03`: sequence contiguity (REQ-004).
pub const ITEM_CONTIGUITY: &str = "SC-03";
/// Item `SC-04`: extension declaration and digesting (REQ-004).
pub const ITEM_EXTENSIONS: &str = "SC-04";

/// Smallest recommended batch, in bytes (1 MB, 10^6 bytes) [unverified].
pub const BATCH_MIN_BYTES: u64 = 1_000_000;
/// Largest recommended batch, in bytes (8 MB) [unverified].
pub const BATCH_MAX_BYTES: u64 = 8_000_000;
/// Longest recommended batch span, in nanoseconds of device time (50 ms) [unverified].
pub const BATCH_MAX_SPAN_NS: u64 = 50_000_000;
/// Share of batches, in percent, that must be within the recommendation (proposal) [unverified].
pub const BATCH_PASS_PERCENT: u64 = 95;
/// Shortest test stream `SC-01` accepts, in nanoseconds (60 s).
pub const TEST_STREAM_NS: u64 = 60_000_000_000;

/// Most vendor namespaces one package lists.
pub const MAX_NAMESPACES: usize = 16;
/// Longest vendor namespace, in bytes.
pub const MAX_NAMESPACE_BYTES: usize = 31;

/// Wire tag of a [`BatchRecord`].
pub const BATCH_RECORD_TAG: &str = "stream-batches/0";
/// Largest accepted [`BatchRecord`] JSON document (16 MiB).
pub const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;
/// Most batches in one [`BatchRecord`].
pub const MAX_RECORD_BATCHES: usize = 100_000;
/// Deepest accepted JSON nesting of a record.
const MAX_RECORD_DEPTH: usize = 4;
/// Most JSON elements in one record: nine per batch (object, four keys, four values) plus
/// the envelope.
const MAX_RECORD_ELEMENTS: usize = 16 + 9 * MAX_RECORD_BATCHES;

// ------------------------------------------------------------------ errors

/// A failure of the streaming template, the extension rule or a stream wire value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamError {
    /// `extension_undeclared`: a vendor-extension setting not named under a vendor namespace
    /// the package lists in describe (REQ-002), or a dotted live setting no extension declares.
    ExtensionUndeclared { setting: String },
    /// `namespace_invalid`: a vendor namespace that is not `[a-z][a-z0-9_]*` of at most
    /// [`MAX_NAMESPACE_BYTES`] bytes, or more than [`MAX_NAMESPACES`] of them.
    NamespaceInvalid { namespace: String },
    /// `stream_conflict`: the base description already holds an operation, type schema,
    /// profile item or namespace of the same name with other content.
    Conflict { what: &'static str, name: String },
    /// `stream_template_missing`: a description without one of the template's operations,
    /// type schemas or profile items (REQ-001).
    Missing { what: &'static str, name: String },
    /// A declared extension setting that breaks a `HAL.IDL.BASE.CFG` rule
    /// (`CONFIG_INVALID`).
    Extension(ConfigError),
    /// The built description is not valid `capability-package/v1` (`package_invalid`).
    Package(PackageError),
    /// `stream_record_invalid`: a [`StreamStatus`], [`StreamEvent`] or [`BatchRecord`] wire
    /// value that is not valid; `pointer` is an RFC 6901 JSON pointer.
    Record { pointer: String, reason: &'static str },
}

impl StreamError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            StreamError::ExtensionUndeclared { .. } => "extension_undeclared",
            StreamError::NamespaceInvalid { .. } => "namespace_invalid",
            StreamError::Conflict { .. } => "stream_conflict",
            StreamError::Missing { .. } => "stream_template_missing",
            StreamError::Extension(e) => e.code(),
            StreamError::Package(e) => e.code(),
            StreamError::Record { .. } => "stream_record_invalid",
        }
    }

    fn record(pointer: impl Into<String>, reason: &'static str) -> StreamError {
        StreamError::Record { pointer: pointer.into(), reason }
    }
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamError::ExtensionUndeclared { setting } => write!(f, "extension_undeclared: {setting:?}"),
            StreamError::NamespaceInvalid { namespace } => write!(f, "namespace_invalid: {namespace:?}"),
            StreamError::Conflict { what, name } => write!(f, "stream_conflict: {what} {name:?}"),
            StreamError::Missing { what, name } => write!(f, "stream_template_missing: {what} {name:?}"),
            StreamError::Extension(e) => write!(f, "extension: {e}"),
            StreamError::Package(e) => write!(f, "{e}"),
            StreamError::Record { pointer, reason } => write!(f, "stream_record_invalid{{{pointer}}}: {reason}"),
        }
    }
}

impl std::error::Error for StreamError {}

/// A conformance item that did not pass: the item ID, a stable reason and, where one batch
/// or step is at fault, its index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemFailure {
    pub item: &'static str,
    pub reason: &'static str,
    pub index: Option<usize>,
}

impl ItemFailure {
    fn new(item: &'static str, reason: &'static str, index: Option<usize>) -> ItemFailure {
        ItemFailure { item, reason, index }
    }

    /// Stable machine-readable error code.
    pub const fn code(&self) -> &'static str {
        "conformance_item_failed"
    }
}

impl fmt::Display for ItemFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.index {
            Some(i) => write!(f, "{} failed: {} at {i}", self.item, self.reason),
            None => write!(f, "{} failed: {}", self.item, self.reason),
        }
    }
}

impl std::error::Error for ItemFailure {}

// ------------------------------------------------------------------ small closed enums

/// Whether a stream is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StreamState {
    Idle,
    Streaming,
}

impl StreamState {
    /// Every state, in wire-enum order.
    pub const ALL: [StreamState; 2] = [StreamState::Idle, StreamState::Streaming];

    /// The wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            StreamState::Idle => "idle",
            StreamState::Streaming => "streaming",
        }
    }

    /// Parses a wire name.
    pub fn parse(s: &str) -> Option<StreamState> {
        StreamState::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// Why a stream stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StopReason {
    /// `stop_stream` was called.
    Requested,
    /// A device error ended the stream.
    DeviceError,
    /// A limit (`LIMIT`) ended the stream.
    Limit,
    /// An observed safety stop ended the stream.
    SafetyStop,
    /// A finite source (file or playback) has no more data.
    EndOfData,
}

impl StopReason {
    /// Every reason, in wire-enum order.
    pub const ALL: [StopReason; 5] = [
        StopReason::Requested,
        StopReason::DeviceError,
        StopReason::Limit,
        StopReason::SafetyStop,
        StopReason::EndOfData,
    ];

    /// The wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            StopReason::Requested => "requested",
            StopReason::DeviceError => "device_error",
            StopReason::Limit => "limit",
            StopReason::SafetyStop => "safety_stop",
            StopReason::EndOfData => "end_of_data",
        }
    }

    /// Parses a wire name.
    pub fn parse(s: &str) -> Option<StopReason> {
        StopReason::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// Where a [`BatchRecord`] came from. A pass on a synthetic or simulated record is never a
/// statement about a real device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordOrigin {
    /// Hand-written or generated test data.
    Synthetic,
    /// Recorded from a simulator (`HAL.SIM`).
    Simulated,
    /// Recorded from a device.
    Device,
}

impl RecordOrigin {
    /// Every origin, in wire order.
    pub const ALL: [RecordOrigin; 3] = [RecordOrigin::Synthetic, RecordOrigin::Simulated, RecordOrigin::Device];

    /// The wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            RecordOrigin::Synthetic => "synthetic",
            RecordOrigin::Simulated => "simulated",
            RecordOrigin::Device => "device",
        }
    }

    /// Parses a wire name.
    pub fn parse(s: &str) -> Option<RecordOrigin> {
        RecordOrigin::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

// ------------------------------------------------------------------ status and events

/// The response of every stream operation: the state after the call and the sequence number
/// the next batch will start at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamStatus {
    pub state: StreamState,
    pub next_seq: u64,
}

impl StreamStatus {
    /// The wire value.
    pub fn to_value(&self) -> CValue {
        obj(vec![("next_seq", dec(self.next_seq)), ("state", text(self.state.as_str()))])
    }

    /// Reads and checks the wire value.
    pub fn from_value(v: &CValue) -> Result<StreamStatus, StreamError> {
        let o = fields(v, "", &["next_seq", "state"])?;
        let state = StreamState::parse(req_str(o, "", "state")?)
            .ok_or_else(|| StreamError::record("/state", "state_unknown"))?;
        Ok(StreamStatus { state, next_seq: req_u64(o, "", "next_seq")? })
    }
}

/// A standard stream event (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StreamEvent {
    /// The stream started.
    Started,
    /// The stream stopped.
    Stopped { reason: StopReason },
    /// The device reports that the sequence numbers `from..=to` will never arrive.
    Gap { from: u64, to: u64 },
}

impl StreamEvent {
    /// The wire `kind`.
    pub const fn kind(&self) -> &'static str {
        match self {
            StreamEvent::Started => "started",
            StreamEvent::Stopped { .. } => "stopped",
            StreamEvent::Gap { .. } => "gap",
        }
    }

    /// The wire value.
    pub fn to_value(&self) -> CValue {
        match self {
            StreamEvent::Started => obj(vec![("kind", text("started"))]),
            StreamEvent::Stopped { reason } => obj(vec![("kind", text("stopped")), ("reason", text(reason.as_str()))]),
            StreamEvent::Gap { from, to } => obj(vec![("from", dec(*from)), ("kind", text("gap")), ("to", dec(*to))]),
        }
    }

    /// Reads and checks the wire value: the key set must be exactly the one of its kind.
    pub fn from_value(v: &CValue) -> Result<StreamEvent, StreamError> {
        let o = fields(v, "", &["from", "kind", "reason", "to"])?;
        let kind = req_str(o, "", "kind")?;
        let allowed: &[&str] = match kind {
            "started" => &["kind"],
            "stopped" => &["kind", "reason"],
            "gap" => &["from", "kind", "to"],
            _ => return Err(StreamError::record("/kind", "kind_unknown")),
        };
        if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(StreamError::record(format!("/{}", escape(k)), "field_unknown"));
        }
        match kind {
            "started" => Ok(StreamEvent::Started),
            "stopped" => {
                let reason = StopReason::parse(req_str(o, "", "reason")?)
                    .ok_or_else(|| StreamError::record("/reason", "reason_unknown"))?;
                Ok(StreamEvent::Stopped { reason })
            }
            _ => {
                let from = req_u64(o, "", "from")?;
                let to = req_u64(o, "", "to")?;
                if from > to {
                    return Err(StreamError::record("/to", "range_reversed"));
                }
                Ok(StreamEvent::Gap { from, to })
            }
        }
    }
}

// ------------------------------------------------------------------ operations

/// One operation the template adds (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StreamOperation {
    /// `{session_id}` -> `{status}`. Starting a running stream changes nothing.
    StartStream,
    /// `{session_id}` -> `{status}`. Stopping a stopped stream changes nothing.
    StopStream,
    /// `{session_id}` -> `{status}`.
    StreamStatus,
}

use ErrorCode::{
    CalibrationExpired, CommLost, ConfigInvalid, Internal, Limit, NotQualified, SafetyStopObserved, Transient,
    Unsupported, VendorLicense,
};

impl StreamOperation {
    /// Every operation, in template order.
    pub const ALL: [StreamOperation; 3] =
        [StreamOperation::StartStream, StreamOperation::StopStream, StreamOperation::StreamStatus];

    /// The wire name.
    pub const fn name(self) -> &'static str {
        match self {
            StreamOperation::StartStream => "start_stream",
            StreamOperation::StopStream => "stop_stream",
            StreamOperation::StreamStatus => "stream_status",
        }
    }

    /// The side-effect class (CONSTITUTION §3): D1 for start and stop, R0 for the status read.
    pub const fn class(self) -> SideEffect {
        match self {
            StreamOperation::StartStream | StreamOperation::StopStream => SideEffect::D1,
            StreamOperation::StreamStatus => SideEffect::R0,
        }
    }

    /// The `Device@1` error codes the operation may return, in error-table order.
    pub const fn errors(self) -> &'static [ErrorCode] {
        match self {
            StreamOperation::StartStream => &[
                Transient,
                CommLost,
                ConfigInvalid,
                NotQualified,
                CalibrationExpired,
                Limit,
                SafetyStopObserved,
                VendorLicense,
                Unsupported,
                Internal,
            ],
            StreamOperation::StopStream | StreamOperation::StreamStatus => &[Transient, CommLost, Internal],
        }
    }

    /// The request type schema (a `capability-package/v1` fragment).
    pub fn request_schema(self) -> CValue {
        closed(vec![("session_id", def_ref("SessionId"))], &["session_id"])
    }

    /// The response type schema (a `capability-package/v1` fragment).
    pub fn response_schema(self) -> CValue {
        closed(vec![("status", def_ref("StreamStatus"))], &["status"])
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

/// The type schemas the template adds to `$defs`. `SessionId` is the same schema as in
/// `Device@1`, so a package that already defines it keeps it.
pub fn stream_defs() -> BTreeMap<String, CValue> {
    let enumeration = |values: Vec<&str>| {
        obj(vec![("enum", CValue::Arr(values.into_iter().map(text).collect())), ("type", text("string"))])
    };
    BTreeMap::from([
        ("SessionId".to_string(), string_schema(36)),
        ("StopReason".to_string(), enumeration(StopReason::ALL.iter().map(|r| r.as_str()).collect())),
        (
            "StreamEvent".to_string(),
            closed(
                vec![
                    ("from", string_schema(20)),
                    ("kind", enumeration(vec!["started", "stopped", "gap"])),
                    ("reason", def_ref("StopReason")),
                    ("to", string_schema(20)),
                ],
                &["kind"],
            ),
        ),
        ("StreamState".to_string(), enumeration(StreamState::ALL.iter().map(|s| s.as_str()).collect())),
        (
            "StreamStatus".to_string(),
            closed(vec![("next_seq", string_schema(20)), ("state", def_ref("StreamState"))], &["next_seq", "state"]),
        ),
    ])
}

// ------------------------------------------------------------------ conformance profile

/// The `Stream-common@1` profile (REQ-003, REQ-004). Item IDs are stable: an item is never
/// renumbered, and a withdrawn ID is not reused.
pub const STREAM_COMMON: ConformanceProfile = ConformanceProfile {
    id: PROFILE_ID,
    items: &[
        ProfileItem {
            id: ITEM_BATCH_SIZE,
            title: "Batch size within the recommendation",
            checks: &["over a test stream of at least 60 s, at least 95 % of batches hold 1 MB to 8 MB or span at most 50 ms of device time"],
        },
        ProfileItem {
            id: ITEM_IDEMPOTENCE,
            title: "Start and stop idempotence",
            checks: &[
                "start_stream on a running stream changes nothing and emits no second started event",
                "stop_stream on a stopped stream changes nothing and emits no second stopped event",
            ],
        },
        ProfileItem {
            id: ITEM_CONTIGUITY,
            title: "Sequence contiguity",
            checks: &[
                "each batch starts one after the last sequence number of the batch before it",
                "no batch range is reversed, overlapping or missing",
            ],
        },
        ProfileItem {
            id: ITEM_EXTENSIONS,
            title: "Extension declaration and digesting",
            checks: &[
                "every vendor-extension setting is named under a vendor namespace listed in describe",
                "every extension value is in the configuration digest the device reports",
            ],
        },
    ],
};

// ------------------------------------------------------------------ the builder

/// Builds a streaming sensor package from a package's own description (REQ-001).
///
/// ```text
/// StreamingPackage::new(base).vendor_namespace("acme").extension(setting).build()
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingPackage {
    base: PackageDescription,
    namespaces: Vec<String>,
    extensions: Vec<SettingSchema>,
}

impl StreamingPackage {
    /// Starts from the package's own operations, payload batch schemas, semantics and profile.
    pub fn new(base: PackageDescription) -> StreamingPackage {
        StreamingPackage { base, namespaces: Vec::new(), extensions: Vec::new() }
    }

    /// Lists a vendor namespace in describe.
    pub fn vendor_namespace(mut self, namespace: impl Into<String>) -> StreamingPackage {
        self.namespaces.push(namespace.into());
        self
    }

    /// Declares a vendor-extension setting; its name must be `<namespace>.<name>` with a
    /// listed namespace.
    pub fn extension(mut self, setting: SettingSchema) -> StreamingPackage {
        self.extensions.push(setting);
        self
    }

    /// Adds the template's operations, type schemas and profile items, then validates the
    /// whole package. An operation or profile item of the same name already in the base, or a
    /// type schema of the same name with other content, is `stream_conflict`.
    pub fn build(self) -> Result<StreamingDescription, StreamError> {
        let mut desc = self.base;
        for op in StreamOperation::ALL {
            if desc.operations.iter().any(|o| o.name == op.name()) {
                return Err(StreamError::Conflict { what: "operation", name: op.name().to_string() });
            }
            desc.operations.push(op.description());
        }
        for (name, schema) in stream_defs() {
            match desc.defs.get(&name) {
                Some(existing) if *existing != schema => {
                    return Err(StreamError::Conflict { what: "type schema", name });
                }
                Some(_) => {}
                None => {
                    desc.defs.insert(name, schema);
                }
            }
        }
        for item in STREAM_COMMON.description().items {
            if desc.profile.items.iter().any(|i| i.id == item.id) {
                return Err(StreamError::Conflict { what: "profile item", name: item.id });
            }
            desc.profile.items.push(item);
        }
        let mut namespaces = BTreeSet::new();
        for ns in self.namespaces {
            if namespaces.contains(&ns) {
                return Err(StreamError::Conflict { what: "namespace", name: kept(&ns) });
            }
            namespaces.insert(ns);
        }
        let built =
            StreamingDescription { description: desc, vendor_namespaces: namespaces, extensions: self.extensions };
        built.validate()?;
        Ok(built)
    }
}

/// A validated streaming package: its `capability-package/v1` description plus the vendor
/// namespaces it lists in describe and the extension settings it declares.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingDescription {
    description: PackageDescription,
    vendor_namespaces: BTreeSet<String>,
    extensions: Vec<SettingSchema>,
}

impl StreamingDescription {
    /// A streaming package from parts read elsewhere (for example a description parsed with
    /// [`PackageDescription::from_json`] and the namespaces a driver lists in describe),
    /// checked with [`StreamingDescription::validate`].
    pub fn from_parts(
        description: PackageDescription,
        vendor_namespaces: BTreeSet<String>,
        extensions: Vec<SettingSchema>,
    ) -> Result<StreamingDescription, StreamError> {
        let out = StreamingDescription { description, vendor_namespaces, extensions };
        out.validate()?;
        Ok(out)
    }

    /// The `capability-package/v1` description, for export and digest.
    pub fn description(&self) -> &PackageDescription {
        &self.description
    }

    /// The vendor namespaces listed in describe.
    pub fn vendor_namespaces(&self) -> &BTreeSet<String> {
        &self.vendor_namespaces
    }

    /// The declared vendor-extension settings, in declaration order.
    pub fn extensions(&self) -> &[SettingSchema] {
        &self.extensions
    }

    /// Checks, in order: the description is valid `capability-package/v1`; it holds the
    /// template's operations, type schemas and profile items unchanged; and the extension
    /// rule ([`validate_extensions`]) holds.
    pub fn validate(&self) -> Result<(), StreamError> {
        let desc = &self.description;
        desc.validate().map_err(StreamError::Package)?;
        for op in StreamOperation::ALL {
            match desc.operations.iter().find(|o| o.name == op.name()) {
                None => return Err(StreamError::Missing { what: "operation", name: op.name().to_string() }),
                Some(o) if *o != op.description() => {
                    return Err(StreamError::Conflict { what: "operation", name: op.name().to_string() });
                }
                Some(_) => {}
            }
        }
        for (name, schema) in stream_defs() {
            match desc.defs.get(&name) {
                None => return Err(StreamError::Missing { what: "type schema", name }),
                Some(s) if *s != schema => return Err(StreamError::Conflict { what: "type schema", name }),
                Some(_) => {}
            }
        }
        for item in STREAM_COMMON.description().items {
            match desc.profile.items.iter().find(|i| i.id == item.id) {
                None => return Err(StreamError::Missing { what: "profile item", name: item.id }),
                Some(i) if *i != item => return Err(StreamError::Conflict { what: "profile item", name: item.id }),
                Some(_) => {}
            }
        }
        validate_extensions(desc, &self.vendor_namespaces, &self.extensions)
    }
}

// ------------------------------------------------------------------ the extension rule

/// The package-level vendor-extension rule (REQ-002), checked in order:
///
/// 1. at most [`MAX_NAMESPACES`] namespaces, each `[a-z][a-z0-9_]*` of at most
///    [`MAX_NAMESPACE_BYTES`] bytes (`namespace_invalid`);
/// 2. every declared extension, in declaration order, is named `<namespace>.<name>` with a
///    listed namespace (`extension_undeclared`);
/// 3. the extensions form a valid `HAL.IDL.BASE.CFG` schema: distinct names, well-formed
///    types and ranges, all acquisition-relevant (`CONFIG_INVALID`);
/// 4. every dotted `live_settings` name of every operation is a declared extension
///    (`extension_undeclared`).
pub fn validate_extensions(
    desc: &PackageDescription,
    namespaces: &BTreeSet<String>,
    extensions: &[SettingSchema],
) -> Result<(), StreamError> {
    check_namespaces(namespaces)?;
    for ext in extensions {
        if !under_listed_namespace(namespaces, &ext.name) {
            return Err(StreamError::ExtensionUndeclared { setting: kept(&ext.name) });
        }
    }
    ConfigSchema::new(extensions.to_vec()).map_err(StreamError::Extension)?;
    let declared: BTreeSet<&str> = extensions.iter().map(|e| e.name.as_str()).collect();
    for op in &desc.operations {
        for s in &op.live_settings {
            if s.contains('.') && !declared.contains(s.as_str()) {
                return Err(StreamError::ExtensionUndeclared { setting: kept(s) });
            }
        }
    }
    Ok(())
}

/// The runtime form of the rule (REQ-HAL.IDL.SENS-006): every extension setting (a dotted
/// name) of a device's `config-schema/0` is under a namespace the driver listed in describe.
/// The first offending setting in name order is reported.
pub fn check_config_schema(namespaces: &BTreeSet<String>, schema: &ConfigSchema) -> Result<(), StreamError> {
    check_namespaces(namespaces)?;
    for s in schema.settings() {
        if s.is_extension() && !under_listed_namespace(namespaces, &s.name) {
            return Err(StreamError::ExtensionUndeclared { setting: kept(&s.name) });
        }
    }
    Ok(())
}

fn check_namespaces(namespaces: &BTreeSet<String>) -> Result<(), StreamError> {
    if namespaces.len() > MAX_NAMESPACES {
        return Err(StreamError::NamespaceInvalid { namespace: String::new() });
    }
    for ns in namespaces {
        if !is_namespace(ns) {
            return Err(StreamError::NamespaceInvalid { namespace: kept(ns) });
        }
    }
    Ok(())
}

fn is_namespace(s: &str) -> bool {
    let mut b = s.bytes();
    s.len() <= MAX_NAMESPACE_BYTES
        && matches!(b.next(), Some(b'a'..=b'z'))
        && b.all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

fn under_listed_namespace(namespaces: &BTreeSet<String>, name: &str) -> bool {
    match name.split_once('.') {
        Some((ns, rest)) => !rest.is_empty() && namespaces.contains(ns),
        None => false,
    }
}

// ------------------------------------------------------------------ recorded summaries

/// The summary of one recorded batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BatchSummary {
    /// First sequence number in the batch.
    pub first_seq: u64,
    /// Last sequence number in the batch (inclusive).
    pub last_seq: u64,
    /// Payload size in bytes.
    pub bytes: u64,
    /// Device time from the first to the last sample, in nanoseconds; `None` when the device
    /// gives no device time.
    pub span_ns: Option<u64>,
}

impl BatchSummary {
    /// Whether the batch is within the recommendation (REQ-003): [`BATCH_MIN_BYTES`] to
    /// [`BATCH_MAX_BYTES`] bytes inclusive, or a device-time span of at most
    /// [`BATCH_MAX_SPAN_NS`]. A batch without device time qualifies by size only.
    pub fn within_recommendation(&self) -> bool {
        (BATCH_MIN_BYTES..=BATCH_MAX_BYTES).contains(&self.bytes)
            || self.span_ns.is_some_and(|s| s <= BATCH_MAX_SPAN_NS)
    }

    fn to_value(self) -> CValue {
        let mut pairs =
            vec![("bytes", dec(self.bytes)), ("first_seq", dec(self.first_seq)), ("last_seq", dec(self.last_seq))];
        if let Some(s) = self.span_ns {
            pairs.push(("span_ns", dec(s)));
        }
        obj(pairs)
    }

    fn from_value(v: &CValue, ptr: &str) -> Result<BatchSummary, StreamError> {
        let o = fields(v, ptr, &["bytes", "first_seq", "last_seq", "span_ns"])?;
        let span_ns = match o.get("span_ns") {
            None => None,
            Some(_) => Some(req_u64(o, ptr, "span_ns")?),
        };
        Ok(BatchSummary {
            first_seq: req_u64(o, ptr, "first_seq")?,
            last_seq: req_u64(o, ptr, "last_seq")?,
            bytes: req_u64(o, ptr, "bytes")?,
            span_ns,
        })
    }
}

/// A recorded test stream: its origin, its duration and its batches in arrival order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchRecord {
    pub origin: RecordOrigin,
    /// Duration of the test stream, in nanoseconds.
    pub duration_ns: u64,
    pub batches: Vec<BatchSummary>,
}

impl BatchRecord {
    /// The wire value.
    pub fn to_value(&self) -> CValue {
        obj(vec![
            ("batches", CValue::Arr(self.batches.iter().map(|b| b.to_value()).collect())),
            ("duration_ns", dec(self.duration_ns)),
            ("origin", text(self.origin.as_str())),
            ("schema", text(BATCH_RECORD_TAG)),
        ])
    }

    /// Parses and checks a record from JSON bytes, within [`MAX_RECORD_BYTES`],
    /// [`MAX_RECORD_BATCHES`] and a fixed depth and element budget.
    pub fn from_json(bytes: &[u8]) -> Result<BatchRecord, StreamError> {
        let v = fid_canon::parse_with_budget(bytes, MAX_RECORD_BYTES, MAX_RECORD_DEPTH, MAX_RECORD_ELEMENTS)
            .map_err(|_| StreamError::record("", "json"))?;
        BatchRecord::from_value(&v)
    }

    /// Checks a parsed record.
    pub fn from_value(v: &CValue) -> Result<BatchRecord, StreamError> {
        let o = fields(v, "", &["batches", "duration_ns", "origin", "schema"])?;
        if req_str(o, "", "schema")? != BATCH_RECORD_TAG {
            return Err(StreamError::record("/schema", "schema_unknown"));
        }
        let origin = RecordOrigin::parse(req_str(o, "", "origin")?)
            .ok_or_else(|| StreamError::record("/origin", "origin_unknown"))?;
        let duration_ns = req_u64(o, "", "duration_ns")?;
        let Some(CValue::Arr(items)) = o.get("batches") else {
            return Err(StreamError::record(
                "/batches",
                if o.contains_key("batches") { "wrong_type" } else { "field_missing" },
            ));
        };
        if items.len() > MAX_RECORD_BATCHES {
            return Err(StreamError::record("/batches", "too_many_batches"));
        }
        let mut batches = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            batches.push(BatchSummary::from_value(item, &format!("/batches/{i}"))?);
        }
        Ok(BatchRecord { origin, duration_ns, batches })
    }
}

// ------------------------------------------------------------------ the evaluators

/// The outcome of a passing `SC-01`: how many batches were within the recommendation, out of
/// how many, over how long, and where the record came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchSizeTally {
    pub origin: RecordOrigin,
    pub total: usize,
    pub within: usize,
    pub duration_ns: u64,
}

/// Item `SC-01` (REQ-003). Fails with `no_batches` on an empty record, `stream_too_short`
/// when the stream lasted less than [`TEST_STREAM_NS`], and `below_threshold` when fewer than
/// [`BATCH_PASS_PERCENT`] % of the batches are within the recommendation. Exact integer
/// arithmetic.
pub fn check_batch_size(record: &BatchRecord) -> Result<BatchSizeTally, ItemFailure> {
    let fail = |reason| ItemFailure::new(ITEM_BATCH_SIZE, reason, None);
    let total = record.batches.len();
    if total == 0 {
        return Err(fail("no_batches"));
    }
    if record.duration_ns < TEST_STREAM_NS {
        return Err(fail("stream_too_short"));
    }
    let within = record.batches.iter().filter(|b| b.within_recommendation()).count();
    if (within as u128) * 100 < (BATCH_PASS_PERCENT as u128) * (total as u128) {
        return Err(fail("below_threshold"));
    }
    Ok(BatchSizeTally { origin: record.origin, total, within, duration_ns: record.duration_ns })
}

/// Item `SC-03` (REQ-004). Fails with `no_batches` on an empty list, `range_reversed` when a
/// batch's last sequence number is below its first, `gap` when a batch starts after the
/// previous batch's successor, and `overlap` when it starts at or before the previous batch's
/// last number. The index is the offending batch.
pub fn check_sequence_contiguity(batches: &[BatchSummary]) -> Result<(), ItemFailure> {
    let fail = |reason, i| Err(ItemFailure::new(ITEM_CONTIGUITY, reason, Some(i)));
    if batches.is_empty() {
        return Err(ItemFailure::new(ITEM_CONTIGUITY, "no_batches", None));
    }
    let mut prev_last: Option<u64> = None;
    for (i, b) in batches.iter().enumerate() {
        if b.first_seq > b.last_seq {
            return fail("range_reversed", i);
        }
        if let Some(last) = prev_last {
            // A previous batch ending at u64::MAX has no successor: anything after it overlaps.
            match last.checked_add(1) {
                Some(next) if b.first_seq == next => {}
                Some(next) if b.first_seq > next => return fail("gap", i),
                _ => return fail("overlap", i),
            }
        }
        prev_last = Some(b.last_seq);
    }
    Ok(())
}

/// One recorded control call: the operation, its outcome, and the stream events seen between
/// this call and the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlStep {
    pub op: StreamOperation,
    pub outcome: Result<StreamStatus, ErrorCode>,
    pub events: Vec<StreamEvent>,
}

/// Item `SC-02` (REQ-004) over a recorded call sequence that starts with the stream idle.
///
/// Every call must succeed (`operation_failed`) and report the state it leaves the stream in
/// (`state_wrong`). Starting an idle stream emits exactly one `started` event and stopping a
/// running one exactly one `stopped` event with reason `requested` (`events_wrong`).
/// Starting a running stream, stopping an idle one, or reading the status emits neither
/// event, and a call on an idle stream leaves `next_seq` unchanged (`not_idempotent`; for the
/// status read, `events_wrong`). Gap events are ignored here. The record must contain at
/// least one start of a running stream and one stop of an idle stream (`not_exercised`).
pub fn check_start_stop_idempotence(steps: &[ControlStep]) -> Result<(), ItemFailure> {
    let fail = |reason, i| Err(ItemFailure::new(ITEM_IDEMPOTENCE, reason, Some(i)));
    let mut state = StreamState::Idle;
    let mut idle_next_seq: Option<u64> = None;
    let (mut restarted, mut restopped) = (false, false);
    for (i, step) in steps.iter().enumerate() {
        let Ok(status) = step.outcome else {
            return fail("operation_failed", i);
        };
        let started = step.events.iter().filter(|e| matches!(e, StreamEvent::Started)).count();
        let stopped: Vec<StopReason> = step
            .events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::Stopped { reason } => Some(*reason),
                _ => None,
            })
            .collect();
        let quiet = started == 0 && stopped.is_empty();
        let expected = match step.op {
            StreamOperation::StartStream => StreamState::Streaming,
            StreamOperation::StopStream => StreamState::Idle,
            StreamOperation::StreamStatus => state,
        };
        if status.state != expected {
            return fail("state_wrong", i);
        }
        match (step.op, state) {
            (StreamOperation::StartStream, StreamState::Idle) => {
                if started != 1 || !stopped.is_empty() {
                    return fail("events_wrong", i);
                }
            }
            (StreamOperation::StopStream, StreamState::Streaming) => {
                if started != 0 || stopped != [StopReason::Requested] {
                    return fail("events_wrong", i);
                }
            }
            (StreamOperation::StartStream, StreamState::Streaming) => {
                restarted = true;
                if !quiet {
                    return fail("not_idempotent", i);
                }
            }
            (StreamOperation::StopStream, StreamState::Idle) => {
                restopped = true;
                if !quiet || idle_next_seq.is_some_and(|n| n != status.next_seq) {
                    return fail("not_idempotent", i);
                }
            }
            (StreamOperation::StreamStatus, _) => {
                if !quiet {
                    return fail("events_wrong", i);
                }
                if state == StreamState::Idle && idle_next_seq.is_some_and(|n| n != status.next_seq) {
                    return fail("not_idempotent", i);
                }
            }
        }
        state = status.state;
        idle_next_seq = match state {
            StreamState::Idle => Some(status.next_seq),
            StreamState::Streaming => None,
        };
    }
    if !restarted || !restopped {
        return Err(ItemFailure::new(ITEM_IDEMPOTENCE, "not_exercised", None));
    }
    Ok(())
}

/// Item `SC-04` (REQ-004, REQ-HAL.IDL.SENS-006). Fails with `extension_undeclared` when an
/// extension setting of the device's schema is not under a listed namespace,
/// `config_invalid` when the configuration does not validate against the schema,
/// `extension_not_digested` when an extension value is missing from the digested object, and
/// `digest_mismatch` when the digest the device reported is not the configuration digest.
/// A schema without extensions passes the first three checks trivially.
pub fn check_extensions_digested(
    namespaces: &BTreeSet<String>,
    schema: &ConfigSchema,
    cfg: &Configuration,
    reported: &ConfigDigest,
) -> Result<(), ItemFailure> {
    let fail = |reason| Err(ItemFailure::new(ITEM_EXTENSIONS, reason, None));
    if check_config_schema(namespaces, schema).is_err() {
        return fail("extension_undeclared");
    }
    let Ok(bytes) = digest_canonical_bytes(schema, cfg) else {
        return fail("config_invalid");
    };
    // Read the digested object back rather than trusting the digest rule's flags.
    let Ok(CValue::Obj(digested)) = fid_canon::parse(&bytes) else {
        return fail("config_invalid");
    };
    for s in schema.settings().filter(|s| s.is_extension()) {
        if !digested.contains_key(&s.name) {
            return fail("extension_not_digested");
        }
    }
    match config_digest(schema, cfg) {
        Ok(d) if d == *reported => Ok(()),
        Ok(_) => fail("digest_mismatch"),
        Err(_) => fail("config_invalid"),
    }
}

// ------------------------------------------------------------------ wire helpers

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn dec(n: u64) -> CValue {
    CValue::Str(n.to_string())
}

fn obj(pairs: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn string_schema(max_len: usize) -> CValue {
    // Small exact integers only; f64 holds them exactly.
    obj(vec![("maxLength", CValue::Num(max_len as f64)), ("type", text("string"))])
}

fn def_ref(name: &str) -> CValue {
    obj(vec![("$ref", CValue::Str(format!("#/$defs/{name}")))])
}

/// A closed object schema.
fn closed(props: Vec<(&str, CValue)>, required: &[&str]) -> CValue {
    obj(vec![
        ("additionalProperties", CValue::Bool(false)),
        ("properties", obj(props)),
        ("required", CValue::Arr(required.iter().map(|s| text(s)).collect())),
        ("type", text("object")),
    ])
}

fn escape(seg: &str) -> String {
    kept(seg).replace('~', "~0").replace('/', "~1")
}

fn fields<'a>(v: &'a CValue, ptr: &str, allowed: &[&str]) -> Result<&'a BTreeMap<String, CValue>, StreamError> {
    let CValue::Obj(o) = v else {
        return Err(StreamError::record(ptr, "not_object"));
    };
    if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(StreamError::record(format!("{ptr}/{}", escape(k)), "field_unknown"));
    }
    Ok(o)
}

fn req_str<'a>(o: &'a BTreeMap<String, CValue>, ptr: &str, k: &str) -> Result<&'a str, StreamError> {
    match o.get(k) {
        Some(CValue::Str(s)) => Ok(s),
        Some(_) => Err(StreamError::record(format!("{ptr}/{k}"), "wrong_type")),
        None => Err(StreamError::record(format!("{ptr}/{k}"), "field_missing")),
    }
}

/// A `u64` as a canonical decimal string: digits only, no leading zero, at most 20 digits.
fn req_u64(o: &BTreeMap<String, CValue>, ptr: &str, k: &str) -> Result<u64, StreamError> {
    let s = req_str(o, ptr, k)?;
    let canonical =
        !s.is_empty() && s.len() <= 20 && s.bytes().all(|c| c.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    if !canonical {
        return Err(StreamError::record(format!("{ptr}/{k}"), "not_u64"));
    }
    s.parse::<u64>().map_err(|_| StreamError::record(format!("{ptr}/{k}"), "not_u64"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_are_checked() {
        for ok in ["acme", "a", "x9_y", "a234567890123456789012345678901"] {
            assert!(is_namespace(ok), "{ok}");
        }
        for bad in ["", "Acme", "9acme", "ac.me", "ac-me", "a2345678901234567890123456789012"] {
            assert!(!is_namespace(bad), "{bad}");
        }
    }

    #[test]
    fn u64_strings_are_canonical() {
        let get = |s: &str| {
            let o = BTreeMap::from([("n".to_string(), CValue::Str(s.to_string()))]);
            req_u64(&o, "", "n")
        };
        assert_eq!(get("0"), Ok(0));
        assert_eq!(get("18446744073709551615"), Ok(u64::MAX));
        for bad in ["", "01", "-1", "+1", "1.0", "18446744073709551616", " 1"] {
            assert!(get(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn closed_enums_round_trip() {
        for s in StreamState::ALL {
            assert_eq!(StreamState::parse(s.as_str()), Some(s));
        }
        for r in StopReason::ALL {
            assert_eq!(StopReason::parse(r.as_str()), Some(r));
        }
        for o in RecordOrigin::ALL {
            assert_eq!(RecordOrigin::parse(o.as_str()), Some(o));
        }
        assert_eq!(StreamState::parse("Idle"), None);
    }
}

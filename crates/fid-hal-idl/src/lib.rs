//! Device interface definitions of Metrology Maven (spec node `HAL.IDL`). Every capability
//! package, driver and simulator builds against these types. Modules land one spec leaf at a
//! time; see `spec/hal/idl/`.
//!
//! - [`units`]: SI quantities, declared exact encodings, frames and timestamp types
//!   (`HAL.IDL.UNITS`).
//! - [`stamp`]: the frozen v0 provenance stamp, acquisition-batch envelope and external
//!   claim schemas, their validator and canonical digests (`HAL.IDL.STAMP`).
//! - [`base`]: the `Device@1` base package; so far [`base::config`], typed configuration and
//!   the configuration digest (`HAL.IDL.BASE.CFG`), [`base::error`], the closed device
//!   error model (`HAL.IDL.BASE.ERR`), and [`base::device`], the package's operations,
//!   `DeviceInstance`, calibration-state references and `Device-core@1` (`HAL.IDL.BASE.DESC`).
//! - [`package`]: the `capability-package/v1` language-neutral capability-package description
//!   format, [`CapabilityPackage`] trait, validator and RFC 8785 canonical digest
//!   (`HAL.IDL.GEN.FMT`).
//! - [`profile`]: the `instrument-profile/v1` descriptor, its validator, the spec-sheet UEC
//!   cap and the canonical digest that excludes the signature block (`HAL.IDL.PROFILE`).
//! - [`negotiate`]: capability declarations, the runtime-support table, the fail-closed
//!   negotiation function and the capability-package semver rules (`HAL.IDL.NEG`).
//! - [`caps`]: the sensor capability packages; so far [`caps::stream`], the shared streaming
//!   template, the vendor-extension rule and the `Stream-common@1` profile
//!   (`HAL.IDL.SENS.STREAM`).

pub mod base;
pub mod caps;
pub mod negotiate;
pub mod package;
pub mod profile;
pub mod stamp;
pub mod units;

pub use units::{
    EncodingDescriptor, Frame, FrameEdge, FrameGraph, FrameId, FrameKind, NumericType, Quantity, RawValue, SiUnit,
    TimeSource, Timestamp, UnitVector3, UnitsError,
};

pub use stamp::{
    validate, AcquisitionBatchEnvelope, ExternalClaim, ProvenanceStamp, Record, RecordKind, StampError, StampErrorKind,
    UntrustedText,
};

pub use base::config::{
    config_digest, ConfigDigest, ConfigError, ConfigErrorCode, ConfigReason, ConfigSchema, Configuration, SettingRange,
    SettingSchema, SettingType, SettingValue,
};

pub use base::device::{
    CalDate, CalibrationState, DeviceInstance, DevicePackage, DeviceRecordError, DriverId,
    Operation as DeviceOperation, DEVICE_CORE,
};

pub use base::error::{
    Behaviour, DeviceError, DeviceErrorParseError, DeviceText, ErrorCode, OperatorHintId, ParseErrorKind, Severity,
};

pub use base::health::{
    acquisition_allowed, acquisition_allowed_default, check as check_health_transition, warmup_state, Actor,
    HealthState, IllegalTransition, Transition, TRANSITIONS as HEALTH_TRANSITIONS,
};

pub use package::{
    is_semver, validate_and_digest_json, validate_package, BatchColumnDesc, BatchSchemaDesc, CapabilityPackage,
    ConformanceItemDesc, ConformanceProfileDesc, OperationDesc, PackageDescription, PackageError, PackageManifest,
    SemanticsNote, ALLOWED_SCHEMA_KEYWORDS, ALLOWED_SCHEMA_TYPES, MAX_PACKAGE_BYTES, MAX_PACKAGE_DEPTH,
    MAX_SEMANTICS_BYTES, PACKAGE_SCHEMA_TAG,
};

pub use profile::{InstrumentProfile, ProfileError, ProfileErrorKind, SourceKind as ProfileSourceKind};

pub use negotiate::{
    negotiate, negotiate_json, negotiate_with_profile, CapabilityDeclaration, CapabilityOffer, NegError, NegErrorKind,
    NegotiationResult, Outcome, RuntimeSupport, VersionRange,
};

pub use caps::stream::{
    BatchRecord, BatchSummary, StreamError, StreamEvent, StreamOperation, StreamState, StreamStatus,
    StreamingDescription, StreamingPackage, STREAM_COMMON,
};

//! Device interface definitions of Metrology Maven (spec node `HAL.IDL`). Every capability
//! package, driver and simulator builds against these types. Modules land one spec leaf at a
//! time; see `spec/hal/idl/`.
//!
//! - [`units`]: SI quantities, declared exact encodings, frames and timestamp types
//!   (`HAL.IDL.UNITS`).
//! - [`stamp`]: the frozen v0 provenance stamp, acquisition-batch envelope and external
//!   claim schemas, their validator and canonical digests (`HAL.IDL.STAMP`).
//! - [`base`]: the `Device@1` base package; so far [`base::config`], typed configuration and
//!   the configuration digest (`HAL.IDL.BASE.CFG`), and [`base::error`], the closed device
//!   error model (`HAL.IDL.BASE.ERR`).
//! - [`package`]: the `capability-package/v1` language-neutral capability-package description
//!   format, [`CapabilityPackage`] trait, validator and RFC 8785 canonical digest
//!   (`HAL.IDL.GEN.FMT`).

pub mod base;
pub mod package;
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

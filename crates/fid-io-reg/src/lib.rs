//! The safe-read framework of Metrology Maven (spec node `IO.REG`). This crate grows one
//! leaf at a time; today it holds `IO.REG.LIMITS`, the harness every format reader runs
//! inside, `IO.REG.ROOTS`, the approved import and export root handles ([`roots`]),
//! `IO.REG.ENV.REPORT`, the import and export report types ([`report`]),
//! `IO.REG.WRITE`, the atomic export write path ([`write`], [`AtomicWriter`]), and
//! `IO.REG.DETECT`, the format registry, its listing commands and the content-signature
//! detector ([`detect`], [`Registry`]), and `IO.REG.PROV`, source provenance with unit
//! and frame capture ([`prov`], [`SourceProvenance`], [`HashingSource`], [`resolve_unit`]).
//!
//! - [`Limits`] with documented defaults; [`LimitRequest`] overrides may only lower a
//!   limit unless the [`Principal`] is a human (`limit_raise_forbidden` otherwise);
//! - [`Budget`]: per-invocation counters for bytes, elements, nesting depth (through the
//!   [`DepthGuard`] scope token), tracked allocation and wall time;
//! - [`BoundedSource`]: a [`std::io::Read`] wrapper that charges every byte;
//! - [`IoError`]: the closed error taxonomy shared by all readers, with a [`Position`]
//!   (byte offset, optional 1-based line and column) on every malformed-input variant;
//! - [`UntrustedText`]: every string taken from file content (CONSTITUTION C6).
//!
//! The harness never relies on unwinding: every outcome is a returned `Result`.
//! [`toy`] is a small recursive-descent reader built on the harness, used by the tests
//! and the `limits_toy_reader` fuzz target.

mod budget;
pub mod detect;
mod error;
mod limits;
pub mod prov;
pub mod report;
pub mod roots;
mod source;
pub mod toy;
mod untrusted;
pub mod write;

pub use budget::{Budget, Clock, DepthGuard, MonotonicClock, WALL_CHECK_EVERY};
pub use detect::{
    read_prefix, DetectError, FormatDescriptor, Matcher, Origin, OriginFilter, Phase, Predicate, Registry, Signature,
    VersionRange, DETECT_PREFIX_LEN,
};
pub use error::{IoError, Position, CODES};
pub use limits::{
    LimitKind, LimitRequest, Limits, Principal, DEFAULT_ALLOC, DEFAULT_BYTES, DEFAULT_DEPTH, DEFAULT_ELEMENTS,
    DEFAULT_WALL_MS,
};
pub use prov::{
    resolve_unit, to_metres, Detection, EffectiveOptions, Evidence, FormatSelection, FrameCapture, HashingSource,
    NonFiniteLength, ProvError, ProvenanceBuilder, ReaderId, ReaderInfo, SourceDigest, SourceProvenance,
    UnitResolution, UnitSource, VersionToken,
};
pub use report::{
    Code, Dropped, ExportReport, FormatId, ImportReport, Location, ReportBuilder, ReportError, ReportPos, ReportText,
    TypeName, Warning, MAX_DETAIL_CHARS, MAX_LISTED,
};
pub use roots::{Direction, ExportTarget, ImportFile, RelPath, RootAccess, RootHandle, RootId, RootSet, RootWarning};
pub use source::BoundedSource;
pub use untrusted::{UntrustedDisplay, UntrustedText};
pub use write::{AtomicWriter, WriteOptions, WriteReceipt};

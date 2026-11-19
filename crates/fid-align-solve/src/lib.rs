//! Alignment kernel of Metrology Maven. Pure Rust, `f64` metres, single-threaded, no I/O, no
//! `unsafe`; floating-point primitives only through `fid-num` (KNP-1, CONSTITUTION §4).
//!
//! Modules land one spec leaf at a time; see `spec/aln/`. Implemented leaves:
//!
//! - `ALN.QA.REPORT`: [`report`], the one [`ResidualReport`] every alignment solver writes,
//!   its [`ReportBuilder`], the fixed-order statistics (RMS, weighted RMS, maximum absolute
//!   residual, used and rejected counts, all with units) and the report digest. The read
//!   command `align.get_residuals@1` lives in `fid-align` (engine layer), which stores
//!   reports next to the alignments that reference them.

pub mod report;

pub use report::{
    Disposition, Entry, EntryKind, EntryRef, EntryView, Length, LengthVec, RejectReason, ReportBuilder, ReportError,
    ResidualReport, ResidualStats, Unit, LAYOUT_TAG, LAYOUT_VERSION, MAX_ENTRIES, MAX_ITEM, MAX_REASONS,
    MAX_REASON_BYTES,
};

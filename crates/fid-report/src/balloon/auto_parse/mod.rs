//! Deterministic dimension and GD&T callout parser (`RPT.BALLOON.AUTO.PARSE`).
//!
//! Pure `R0` / `D0` library with zero I/O, zero floating-point math, and `BTreeMap`-only maps:
//! - Spatially clusters adjacent [`crate::balloon::auto_text::TextRun`]s (stacked bilateral
//!   tolerances, GD&T feature-control-frame compartments, same-line fragments).
//! - Parses linear, diameter, spherical diameter, radius, spherical radius, angle, chamfer,
//!   thread, surface finish, basic/reference dimensions, repeat counts (`NX`), symmetric (`±`),
//!   bilateral (`+a/-b`), limit (`lo - hi`), and GD&T FCF callouts.
//! - Preserves every authored decimal string verbatim (validated via `fid_num::kdec::Decimal::parse`).
//! - Excludes title-block noise and emits qualitative confidence words (`high`/`medium`/`low`)
//!   with typed reason codes.

pub mod cluster;
pub mod model;
pub mod parser;

pub use cluster::{cluster_runs, RunCluster};
pub use model::{
    CalloutKind, ConfidenceWord, DecimalString, GdtSymbol, Modifier, ParseDrawingResult, ParsedCallout, ReasonCode,
    TitleBlockHints, UnitHint,
};
pub use parser::{parse_callout_text, parse_drawing_text_map};

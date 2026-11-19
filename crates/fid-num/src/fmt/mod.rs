//! Versioned kernel number formatters `fmt.<kind>@<major>`. Spec node `KRN.NUM.FMT`.
//!
//! - [`types`]: the shared formatter types (`KRN.NUM.FMT.LEN`).
//! - [`len`]: `fmt.length@1` and `fmt.scalar@1` with the side-preserving rule
//!   (`KRN.NUM.FMT.LEN`).
//! - [`ang`]: `fmt.angle@1` in rad, deg and dms with a ladder of rational enclosures of pi
//!   (`KRN.NUM.FMT.ANG`).
//!
//! The registry with `fmt::format` dispatch (`KRN.NUM.FMT.REG`) lands in its own leaf.

pub mod ang;
pub mod len;
pub mod types;

pub use types::{
    Conversion, FmtError, FormatSpec, FormatterId, Limits, Precision, Reported, SideOutcome, Unit, MAX_DECIMALS,
    MAX_SIG_DIGITS,
};

//! Versioned kernel number formatters `fmt.<kind>@<major>`. Spec node `KRN.NUM.FMT`.
//!
//! - [`types`]: the shared formatter types (`KRN.NUM.FMT.LEN`).
//! - [`len`]: `fmt.length@1` and `fmt.scalar@1` with the side-preserving rule
//!   (`KRN.NUM.FMT.LEN`).
//!
//! Angle formatters (`KRN.NUM.FMT.ANG`) and the registry with `fmt::format` dispatch
//! (`KRN.NUM.FMT.REG`) land in their own leaves.

pub mod len;
pub mod types;

pub use types::{
    Conversion, FmtError, FormatSpec, FormatterId, Limits, Precision, Reported, Unit, MAX_DECIMALS, MAX_SIG_DIGITS,
};

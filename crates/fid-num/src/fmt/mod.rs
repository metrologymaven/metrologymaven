//! Versioned kernel number formatters `fmt.<kind>@<major>`. Spec node `KRN.NUM.FMT`.
//!
//! - [`types`]: the shared formatter types (`KRN.NUM.FMT.LEN`).
//! - [`len`]: `fmt.length@1` and `fmt.scalar@1` with the side-preserving rule
//!   (`KRN.NUM.FMT.LEN`).
//! - [`ang`]: `fmt.angle@1` in rad, deg and dms with a ladder of rational enclosures of pi
//!   (`KRN.NUM.FMT.ANG`).
//! - [`registry`](mod@registry): the formatter registry, [`format`] dispatch by versioned ID, textual
//!   [`lookup`], and `fmt.hexbits@1` ([`hexbits`]) (`KRN.NUM.FMT.REG`).
//!
//! Consumers format through [`format`], which returns the implementation's [`Reported`]
//! (with `side`, `digits_added`, `tie_at_max_precision` and `exact_conversion`) unchanged.

pub mod ang;
pub mod len;
pub mod registry;
pub mod types;

pub use registry::{
    descriptor, format, hexbits, lookup, registry, FormatterDescriptor, IdText, LookupError, MAX_ID_LEN, MAX_KIND_LEN,
};

pub use types::{
    Conversion, FmtError, FormatSpec, FormatterId, Limits, Precision, Reported, SideOutcome, Unit, MAX_DECIMALS,
    MAX_SIG_DIGITS,
};

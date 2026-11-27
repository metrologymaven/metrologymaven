//! The `ChartSeries` contract, major 1, minor 0. Spec node `RPT.STATS.TREND.CONTRACT`.
//!
//! A [`ChartSeries`] is the one data shape every chart source (trend and histogram series,
//! control charts, vector group plots) hands to report chart blocks and viewers. This crate
//! holds the type, its JSON Schema ([`SCHEMA_JSON`], `schema/chart-series.schema.json`), the
//! validation ([`validate`]), the canonical JSON round trip ([`parse`], [`to_canonical_bytes`],
//! [`digest`]) and the consumer's version check ([`check_supported`]). It computes nothing and
//! does no I/O.
//!
//! # Wire form (contract 1.0)
//!
//! RFC 8785 canonical JSON (`fid-canon`), every key required unless marked optional:
//!
//! ```text
//! { "contract":   { "major": 1, "minor": 0 },
//!   "kind":       "TREND" | "HISTOGRAM" | "CONTROL_XBAR" | "CONTROL_R" | "CONTROL_S"
//!                 | "CONTROL_I" | "CONTROL_MR" | "VECTOR_GROUP",
//!   "x", "y":     { "kind": "ORDINAL", "label_key": K }
//!               | { "kind": "QUANTITY", "quantity": "LENGTH" | "ANGLE", "unit": U, "label_key": K },
//!   "points":     [ { "x": F, "y": F, "x_upper"?: F, "x_label"?: text, "record"?: ID,
//!                     "piece"?: ID, "markers": [ M, ... ] } ],
//!   "ref_lines":  [ { "kind": "NOMINAL" | "LSL" | "USL" | "CENTRE" | "LCL" | "UCL",
//!                     "value": F, "unit": U } ],
//!   "provenance": { "dataset_digest": "sha256:...", "algorithm": A, "kernel_version": V,
//!                   "frozen_limits"?: ID, "binning_rule"?: R } }
//! M = { "kind": "OUT_OF_TOLERANCE" } | { "kind": "INDETERMINATE" }
//!   | { "kind": "RULE_VIOLATION", "set_id": S, "set_version": N, "rule_id": RI }
//! ```
//!
//! - `F`: an f64 as `"0x"` and 16 lowercase hex digits of its IEEE 754 bits, so every value,
//!   `-0.0` and subnormals included, survives the round trip bit for bit (REQ-007). NaN and
//!   infinities are refused. Integers (`major`, `minor`, `set_version`) are JSON numbers.
//! - `U`: a symbol of the `GEO.FRAME.UNITS` catalogue (`m`, `cm`, `mm`, `um`, `nm`, `in`, `ft`,
//!   `thou`; `rad`, `deg`, `arcmin`, `arcsec`, `gon`), of the axis's quantity kind.
//! - `K` (label key): `[a-z][a-z0-9_]*` segments joined by `.`, at most 128 bytes; a key into
//!   the label catalogue, not display text.
//! - `ID` (record, piece and frozen-limits record IDs): `[A-Za-z0-9][A-Za-z0-9._:-]*`, 1 to 128
//!   bytes, the ResultRecord ID grammar.
//! - `A` (algorithm): `<module>.<feature>.<method>[.<more>]@<semver>` (CONSTITUTION §3), each
//!   segment `[a-z][a-z0-9_-]*`, semver parts without leading zeros and at most 9 digits, at
//!   most 128 bytes. `V` (kernel version): such a semver.
//! - `R` (binning rule): `[a-z][a-z0-9_]*@<major>`, at most 56 + 7 bytes (`sturges@1`).
//! - `S`, `RI`, `N`: the run-rule set ID (`[a-z][a-z0-9_]*`, at most 64 bytes), rule ID
//!   (`[a-z0-9][a-z0-9_]*`, at most 32 bytes) and set version (1 to 999 999) of `RPT.STATS.SPC.RULES`.
//! - `x_label`: display text, 1 to 256 characters, none of the `fid-types` forbidden code
//!   points (controls, bidi overrides, invisible format characters).
//!
//! # Rules beyond the field formats
//!
//! [`validate`] (which [`parse`] and [`to_canonical_bytes`] both run) also requires:
//!
//! - **Values are in the axis unit.** Point and reference-line values are expressed in the unit
//!   their axis names, not in SI. A reference line lies on the value axis (`x` for
//!   `HISTOGRAM`, `y` for every other kind): that axis must be a quantity axis
//!   (`no_value_unit`) and the line's unit must be its unit (`unit_mismatch`).
//! - **Histograms**: `x` is a quantity axis and `y` is `ORDINAL` (a count has no physical
//!   unit; `axis_kind`). Each point is one bin: `x` the lower edge, `y` the count (an
//!   integral value from `+0` to 2^53, not `-0.0`; `bad_count`) and the optional `x_upper` not
//!   below `x` in IEEE 754 totalOrder (`upper_below_lower`; equal edges are the single
//!   degenerate bin of `RPT.STATS.TREND.HIST`). `provenance.binning_rule` is required
//!   (REQ-RPT.STATS.TREND-003; `required_for_kind`). Only histograms may carry `x_upper` or a
//!   binning rule, and only control charts a frozen-limits record (`not_applicable`).
//! - **One spelling per value**: markers in a point and the reference lines are each strictly
//!   ascending (markers by kind in the order above, then set ID, set version, rule ID;
//!   reference lines by kind in the order above, then value in totalOrder), so a duplicate or
//!   a reordering is refused (`not_canonical_order`) instead of giving a second encoding of
//!   the same series. Points keep their order: it is the series order.
//! - **No contradictory markers**: `OUT_OF_TOLERANCE` and `INDETERMINATE` on one point are
//!   refused (`conflicting_markers`); a decision is one or the other.
//! - **No tombstone data set**: the all-zero digest is refused (`tombstone`).
//! - Bounds: at most [`MAX_POINTS`] points, [`MAX_MARKERS`] markers per point and
//!   [`MAX_REF_LINES`] reference lines (`too_many`); inputs over [`MAX_SERIES_BYTES`] or deeper
//!   than [`MAX_DEPTH`] are refused by the parser (`input_limit`).
//!
//! # Versions (REQ-005, REQ-006)
//!
//! [`parse`] reads `contract` first. A major other than [`CONTRACT_MAJOR`] is
//! `report.chart_series_unsupported` naming the major, the minor and (when it is a plain
//! upper-case name) the kind; the rest of such a document is not read, since its shape is not
//! this contract's. A minor up to [`CONTRACT_MINOR`] is read strictly: an unknown field is
//! `unknown_field`. A later minor of the same major may only have added optional fields, so
//! unknown fields anywhere are ignored and every one is listed in [`Parsed::ignored_fields`];
//! unknown enum values and missing required fields are still refused. Such a series is held
//! with its minor as read, and [`to_canonical_bytes`] refuses it (`not_encodable`): this crate
//! cannot write the fields of a minor it does not know, and writing the series under minor
//! 0 would change what it claims.
//!
//! The provenance block and the markers are the producer's statements. Validation checks
//! their form only; it does not verify the data-set digest, the algorithm, the frozen-limits
//! record or a run-rule set's review status (look the set up by ID and version for that).
//!
//! # Errors
//!
//! Every refusal is a [`ChartSeriesError`]: `report.chart_series_invalid` with the field path
//! (`provenance.dataset_digest`, `points[3].markers[0].set_id`, `$` for the whole document)
//! and an [`InvalidReason`], or `report.chart_series_unsupported`. Nothing is returned with
//! an error.

mod check;
mod wire;

pub use check::{check_supported, validate};
pub use wire::{digest, parse, to_canonical_bytes, Parsed};

pub use fid_geo_frame::units::Unit;
pub use fid_types::Digest;

use std::fmt;

/// The contract major this crate reads and writes.
pub const CONTRACT_MAJOR: u32 = 1;
/// The highest contract minor this crate knows (it writes only this one).
pub const CONTRACT_MINOR: u32 = 0;
/// Largest major or minor a document may state.
pub const MAX_VERSION_PART: u32 = 999_999;
/// Most points in one series.
pub const MAX_POINTS: usize = 1_000_000;
/// Most markers on one point.
pub const MAX_MARKERS: usize = 64;
/// Most reference lines in one series.
pub const MAX_REF_LINES: usize = 64;
/// Largest run-rule set version (that of `RPT.STATS.SPC.RULES`).
pub const MAX_SET_VERSION: u32 = 999_999;
/// Most characters of a point's display label.
pub const MAX_LABEL_CHARS: usize = 256;
/// Most bytes of a label key.
pub const MAX_LABEL_KEY_BYTES: usize = 128;
/// Most bytes of a record, piece or frozen-limits ID.
pub const MAX_ID_BYTES: usize = 128;
/// Most bytes of an algorithm ID with its version.
pub const MAX_ALGORITHM_BYTES: usize = 128;
/// Largest histogram count, 2^53 (every integer up to it is exact in f64).
pub const MAX_COUNT: f64 = 9_007_199_254_740_992.0;
/// Largest accepted document, in bytes (the `fid-canon` limit).
pub const MAX_SERIES_BYTES: usize = fid_canon::MAX_INPUT_BYTES;
/// Deepest accepted nesting (the format itself needs 5).
pub const MAX_DEPTH: usize = 8;
/// Most parsed JSON elements (values and keys) in one document.
pub const MAX_ELEMENTS: usize = 1 << 25;

/// The JSON Schema 2020-12 of contract 1 (`schema/chart-series.schema.json`), for consumers
/// that validate without linking this crate. The schema is weaker than [`validate`]: see its
/// `description`.
pub const SCHEMA_JSON: &str = include_str!("../schema/chart-series.schema.json");

/// A contract version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContractVersion {
    /// Incompatible changes.
    pub major: u32,
    /// Optional additions only (REQ-006).
    pub minor: u32,
}

impl ContractVersion {
    /// Contract 1.0, the version this crate writes.
    pub const CURRENT: ContractVersion = ContractVersion { major: CONTRACT_MAJOR, minor: CONTRACT_MINOR };
}

macro_rules! wire_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $variant:ident = $wire:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($(#[$vm])* $variant),+
        }

        impl $name {
            /// Every value, in canonical order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The wire name.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $wire),+
                }
            }

            /// The value with this exact wire name.
            pub fn from_wire(s: &str) -> Option<$name> {
                Self::ALL.iter().copied().find(|v| v.as_str() == s)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

wire_enum! {
    /// What a series shows (REQ-003).
    SeriesKind {
        /// Value per characteristic over stages, runs or parts.
        Trend = "TREND",
        /// One point per bin.
        Histogram = "HISTOGRAM",
        /// Subgroup means.
        ControlXbar = "CONTROL_XBAR",
        /// Subgroup ranges.
        ControlR = "CONTROL_R",
        /// Subgroup standard deviations.
        ControlS = "CONTROL_S",
        /// Individual values.
        ControlI = "CONTROL_I",
        /// Moving ranges.
        ControlMr = "CONTROL_MR",
        /// Reserved for the LVM vector group plot.
        VectorGroup = "VECTOR_GROUP",
    }
}

impl SeriesKind {
    /// Whether this is a control chart (the only kinds that may name frozen limits).
    pub const fn is_control(self) -> bool {
        matches!(
            self,
            SeriesKind::ControlXbar
                | SeriesKind::ControlR
                | SeriesKind::ControlS
                | SeriesKind::ControlI
                | SeriesKind::ControlMr
        )
    }
}

wire_enum! {
    /// Reference-line kinds (REQ-003), in canonical order.
    RefKind {
        /// Nominal value.
        Nominal = "NOMINAL",
        /// Lower specification limit.
        Lsl = "LSL",
        /// Upper specification limit.
        Usl = "USL",
        /// Control-chart centre line.
        Centre = "CENTRE",
        /// Lower control limit.
        Lcl = "LCL",
        /// Upper control limit.
        Ucl = "UCL",
    }
}

wire_enum! {
    /// Quantity kinds of the unit catalogue.
    Quantity {
        /// Lengths.
        Length = "LENGTH",
        /// Angles.
        Angle = "ANGLE",
    }
}

impl Quantity {
    /// The quantity kind of a catalogue unit.
    pub const fn of(unit: Unit) -> Quantity {
        match unit {
            Unit::Length(_) => Quantity::Length,
            Unit::Angle(_) => Quantity::Angle,
        }
    }
}

/// What an axis measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxisKind {
    /// No physical quantity: a sequence position (part, run, subgroup) or a count.
    Ordinal,
    /// A quantity with its display unit; values on this axis are in `unit`.
    Quantity {
        /// The quantity kind; must be that of `unit`.
        quantity: Quantity,
        /// The unit values are expressed in.
        unit: Unit,
    },
}

/// One axis.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Axis {
    /// Ordinal or quantity.
    pub kind: AxisKind,
    /// Label-catalogue key of the axis title.
    pub label_key: String,
}

/// A per-point marker (REQ-003, REQ-004).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Marker {
    /// The point's journaled decision is out of tolerance.
    OutOfTolerance,
    /// The point's journaled decision is indeterminate.
    Indeterminate,
    /// The point breaks a run rule.
    RuleViolation {
        /// The run-rule set ID.
        set_id: String,
        /// The run-rule set version.
        set_version: u32,
        /// The rule ID within the set.
        rule_id: String,
    },
}

impl Marker {
    /// The wire kind name.
    pub const fn kind_str(&self) -> &'static str {
        match self {
            Marker::OutOfTolerance => "OUT_OF_TOLERANCE",
            Marker::Indeterminate => "INDETERMINATE",
            Marker::RuleViolation { .. } => "RULE_VIOLATION",
        }
    }

    const fn rank(&self) -> u8 {
        match self {
            Marker::OutOfTolerance => 0,
            Marker::Indeterminate => 1,
            Marker::RuleViolation { .. } => 2,
        }
    }

    /// The canonical order of markers in a point: kind, then set ID, set version and rule ID
    /// (strings by bytes).
    pub fn canonical_cmp(&self, other: &Marker) -> std::cmp::Ordering {
        match (self, other) {
            (
                Marker::RuleViolation { set_id: a, set_version: av, rule_id: ar },
                Marker::RuleViolation { set_id: b, set_version: bv, rule_id: br },
            ) => (a.as_bytes(), av, ar.as_bytes()).cmp(&(b.as_bytes(), bv, br.as_bytes())),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

/// One point. Values are in the units of their axes.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    /// Position on the x axis (for a histogram, the bin's lower edge).
    pub x: f64,
    /// Value on the y axis (for a histogram, the bin's count).
    pub y: f64,
    /// Histogram only: the bin's upper edge.
    pub x_upper: Option<f64>,
    /// Display text for the x position.
    pub x_label: Option<String>,
    /// The ResultRecord the point shows.
    pub record: Option<String>,
    /// The piece of a multi-piece set.
    pub piece: Option<String>,
    /// Markers, in canonical order.
    pub markers: Vec<Marker>,
}

/// One reference line on the value axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RefLine {
    /// What the line is.
    pub kind: RefKind,
    /// Its value, in `unit`.
    pub value: f64,
    /// Must be the value axis's unit.
    pub unit: Unit,
}

/// Where a series came from (REQ-002). The producer's statements, checked for form only.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Provenance {
    /// Digest of the data set the series was built from (required).
    pub dataset_digest: Digest,
    /// Algorithm ID and version that built the series (required).
    pub algorithm: String,
    /// Kernel version.
    pub kernel_version: String,
    /// Control charts only: the frozen-limits record the limits came from.
    pub frozen_limits: Option<String>,
    /// Histograms only (and required there): the binning rule ID.
    pub binning_rule: Option<String>,
}

/// A chart data series (REQ-001). `PartialEq` is IEEE equality (so `-0.0 == 0.0`); compare
/// [`to_canonical_bytes`] output for bit identity.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSeries {
    /// The contract version the series follows.
    pub contract: ContractVersion,
    /// What it shows.
    pub kind: SeriesKind,
    /// The x axis.
    pub x: Axis,
    /// The y axis.
    pub y: Axis,
    /// Points, in series order.
    pub points: Vec<Point>,
    /// Reference lines, in canonical order.
    pub ref_lines: Vec<RefLine>,
    /// Provenance.
    pub provenance: Provenance,
}

/// Why a consumer cannot take a series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnsupportedCause {
    /// The contract major is not the consumer's.
    Major,
    /// The consumer does not draw this kind.
    Kind,
}

/// Why a series is invalid. Each has a stable snake-case name ([`InvalidReason::as_str`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvalidReason {
    /// Not JSON (`fid-canon` `syntax`).
    Syntax,
    /// A key repeated in one object.
    DuplicateKey,
    /// Over [`MAX_SERIES_BYTES`], [`MAX_DEPTH`] or [`MAX_ELEMENTS`].
    InputLimit,
    /// Not UTF-8.
    InvalidUtf8,
    /// A JSON number too large for f64.
    NonFiniteNumber,
    /// The wrong JSON type (`null` included).
    WrongType,
    /// A required field is absent.
    Missing,
    /// A field this contract minor does not define.
    UnknownField,
    /// A number that is not an integer.
    NotInteger,
    /// An integer outside its range.
    OutOfRange,
    /// An f64 not spelled `0x` and 16 lowercase hex digits.
    BadHex,
    /// NaN or an infinity.
    NonFinite,
    /// A string outside its grammar or length.
    BadFormat,
    /// A name that is not one of the defined values.
    UnknownValue,
    /// More items than the bound.
    TooMany,
    /// A unit not of the axis's quantity, or a reference-line unit not the value axis's.
    UnitMismatch,
    /// A reference line on an `ORDINAL` value axis.
    NoValueUnit,
    /// A histogram axis of the wrong kind.
    AxisKind,
    /// Items not strictly ascending in canonical order (a duplicate included).
    NotCanonicalOrder,
    /// `OUT_OF_TOLERANCE` and `INDETERMINATE` on one point.
    ConflictingMarkers,
    /// A field the series kind does not allow.
    NotApplicable,
    /// A field the series kind requires.
    RequiredForKind,
    /// A histogram count that is not an integral value from `+0` to 2^53.
    BadCount,
    /// A histogram upper edge below the lower edge.
    UpperBelowLower,
    /// The all-zero tombstone digest.
    Tombstone,
    /// A later minor than this crate writes.
    NotEncodable,
}

impl InvalidReason {
    /// The stable name.
    pub const fn as_str(self) -> &'static str {
        match self {
            InvalidReason::Syntax => "syntax",
            InvalidReason::DuplicateKey => "duplicate_key",
            InvalidReason::InputLimit => "input_limit",
            InvalidReason::InvalidUtf8 => "invalid_utf8",
            InvalidReason::NonFiniteNumber => "non_finite_number",
            InvalidReason::WrongType => "wrong_type",
            InvalidReason::Missing => "missing",
            InvalidReason::UnknownField => "unknown_field",
            InvalidReason::NotInteger => "not_integer",
            InvalidReason::OutOfRange => "out_of_range",
            InvalidReason::BadHex => "bad_hex",
            InvalidReason::NonFinite => "non_finite",
            InvalidReason::BadFormat => "bad_format",
            InvalidReason::UnknownValue => "unknown_value",
            InvalidReason::TooMany => "too_many",
            InvalidReason::UnitMismatch => "unit_mismatch",
            InvalidReason::NoValueUnit => "no_value_unit",
            InvalidReason::AxisKind => "axis_kind",
            InvalidReason::NotCanonicalOrder => "not_canonical_order",
            InvalidReason::ConflictingMarkers => "conflicting_markers",
            InvalidReason::NotApplicable => "not_applicable",
            InvalidReason::RequiredForKind => "required_for_kind",
            InvalidReason::BadCount => "bad_count",
            InvalidReason::UpperBelowLower => "upper_below_lower",
            InvalidReason::Tombstone => "tombstone",
            InvalidReason::NotEncodable => "not_encodable",
        }
    }
}

/// A refusal (REQ-002, REQ-005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChartSeriesError {
    /// `report.chart_series_unsupported`.
    Unsupported {
        /// The series' contract major.
        major: u32,
        /// The series' contract minor.
        minor: u32,
        /// The series' kind; `None` when the document's kind is absent or not a plain
        /// upper-case name of at most 32 bytes (`[A-Z][A-Z0-9_]*`), so no untrusted text is
        /// echoed.
        kind: Option<String>,
        /// Whether the major or the kind is not supported.
        cause: UnsupportedCause,
    },
    /// `report.chart_series_invalid`.
    Invalid {
        /// The field path, `$` for the whole document.
        field: String,
        /// Why.
        reason: InvalidReason,
    },
}

impl ChartSeriesError {
    /// The stable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            ChartSeriesError::Unsupported { .. } => "report.chart_series_unsupported",
            ChartSeriesError::Invalid { .. } => "report.chart_series_invalid",
        }
    }

    pub(crate) fn invalid(field: impl Into<String>, reason: InvalidReason) -> ChartSeriesError {
        ChartSeriesError::Invalid { field: field.into(), reason }
    }
}

impl fmt::Display for ChartSeriesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChartSeriesError::Unsupported { major, minor, kind, cause } => {
                let what = match cause {
                    UnsupportedCause::Major => "major",
                    UnsupportedCause::Kind => "kind",
                };
                write!(f, "{}: contract {major}.{minor}, kind {}", self.code(), kind.as_deref().unwrap_or("?"))?;
                write!(f, " ({what} not supported)")
            }
            ChartSeriesError::Invalid { field, reason } => {
                write!(f, "{}: {field}: {}", self.code(), reason.as_str())
            }
        }
    }
}

impl std::error::Error for ChartSeriesError {}

/// `kind` as echoed in an unsupported error: only a plain upper-case name.
pub(crate) fn echo_kind(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let ok = !b.is_empty()
        && b.len() <= 32
        && b[0].is_ascii_uppercase()
        && b.iter().all(|&c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_');
    ok.then(|| s.to_owned())
}

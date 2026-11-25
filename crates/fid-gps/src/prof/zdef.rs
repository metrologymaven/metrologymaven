//! Constant-width profile zone definitions and their dispatch (spec node `GDT.PROF.ZDEF.UNIF`,
//! REQ-001..007, AC-1..AC-6).
//!
//! A profile characteristic states its zone as one [`ZoneDefinition`]. This module turns the
//! constant-width definitions into one allowed interval `[lower, upper]` on the signed
//! deviation of `GDT.PROF.DEV` ([`super::dev`]), plus the interval's width and its
//! disposition shift `c`, the mid-surface offset that the offset shape of `GDT.ZONE.ZTYPE`
//! ([`crate::zone::shape::ZoneShape::Offset`]) takes:
//!
//! | definition                         | lower          | upper          |
//! |------------------------------------|----------------|----------------|
//! | `Bilateral{t}`                     | `-(t * 0.5)`   | `t * 0.5`      |
//! | `Unilateral{t, Outward}`           | `0`            | `t`            |
//! | `Unilateral{t, Inward}`            | `-t`           | `0`            |
//! | `Unequal{t, share}`, side outward  | `-(t - share)` | `share`        |
//! | `Unequal{t, share}`, side inward   | `-share`       | `t - share`    |
//!
//! then `width = upper - lower` and `shift = (lower + upper) * 0.5`, and nothing else (REQ-004,
//! Design). Each line is at most one rounded IEEE-754 operation plus exact negations and
//! halvings, so the results are bit-identical on every platform (D0).
//!
//! # Sign convention
//!
//! The interval is on the signed deviation exactly as `GDT.PROF.DEV` defines it: positive on
//! the outward side of the nominal, away from the material. `Side::Outward` is therefore the
//! positive side. A deviation whose `signed_dev` is `None` (no sign of record, including a
//! foot on the edge of the toleranced set, D-045) has no membership:
//! [`ConstantZone::contains_deviation`] returns `None` for it, never `Some(true)`.
//!
//! The interval is taken on the deviation from the *represented* nominal. A face's
//! caller-declared `approx_error` (D-045) is not folded into the interval here; it travels
//! with the deviations to UNC.
//!
//! # Where the share side comes from
//!
//! For an unequally disposed zone the side the stated share lies on is interpretation, so it
//! is read from the resolved operator chain, never hard-coded: the chain must carry the token
//! parameter [`SHARE_SIDE_PARAM`] (`outward` or `inward`) in at least one step, with the same
//! value wherever it appears.
//!
//! # Dispatch
//!
//! A definition kind is `UNSUPPORTED` (no interval) when the resolved row carries the
//! limitation `zone_def_unsupported:<kind>` ([`ZoneDefKind::code`]; `nonuniform` is also
//! read as `non_uniform`, the spelling the synthetic table uses), and a `NonUniform`
//! definition is always `UNSUPPORTED` in this build (owned by `GDT.PROF.ZDEF.NONUNIF`). The
//! dispatch runs before parameter validation: an unsupported kind is refused whatever its
//! parameters.
//!
//! Pure KNP-1 style: no I/O, no `unsafe`, no panics, no FMA, no `f64::min`/`f64::max`, NaN
//! is an error.

use crate::dmt::{OperatorChain, Param};
use crate::prof::dev::Deviation;
use crate::zone::shape::ZoneShape;
use core::fmt;
use fid_canon::CValue;
use std::collections::BTreeMap;

/// The chain parameter naming the side an unequally disposed share lies on.
pub const SHARE_SIDE_PARAM: &str = "share_side";

/// The limitation code prefix that marks a zone definition kind unsupported by a row.
pub const UNSUPPORTED_LIMITATION_PREFIX: &str = "zone_def_unsupported:";

/// A side of the nominal, under the signed-deviation convention of `GDT.PROF.DEV`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    /// The positive side: outward from the nominal, away from the material.
    Outward,
    /// The negative side: into the material.
    Inward,
}

impl Side {
    /// The token: `outward` or `inward`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Side::Outward => "outward",
            Side::Inward => "inward",
        }
    }

    /// Parses the token; anything else is `None`.
    pub fn parse(s: &str) -> Option<Side> {
        match s {
            "outward" => Some(Side::Outward),
            "inward" => Some(Side::Inward),
            _ => None,
        }
    }
}

/// The parameters of a non-uniform zone. Interval construction for it is owned by
/// `GDT.PROF.ZDEF.NONUNIF`; this build only dispatches it to `UNSUPPORTED`.
#[derive(Debug, Clone, PartialEq)]
pub struct NonUniformParams {
    /// The stated zone widths, metres, in the order stated on the characteristic. Their
    /// placement along the feature is defined by `GDT.PROF.ZDEF.NONUNIF`.
    pub widths_m: Vec<f64>,
}

/// The zone definition a profile characteristic states. All lengths in metres.
#[derive(Debug, Clone, PartialEq)]
pub enum ZoneDefinition {
    /// Uniform zone of total width `t`, centred on the nominal.
    Bilateral {
        /// Total zone width.
        t: f64,
    },
    /// Uniform zone of total width `t`, entirely on `side` of the nominal.
    Unilateral {
        /// Total zone width.
        t: f64,
        /// The side the zone lies on.
        side: Side,
    },
    /// Unequally disposed zone of total width `t` with `share` of it on the side the
    /// resolved chain's [`SHARE_SIDE_PARAM`] names.
    Unequal {
        /// Total zone width.
        t: f64,
        /// The stated share, `0 <= share <= t`.
        share: f64,
    },
    /// Non-uniform zone (always `UNSUPPORTED` in this build).
    NonUniform(NonUniformParams),
}

impl ZoneDefinition {
    /// The definition kind.
    pub fn kind(&self) -> ZoneDefKind {
        match self {
            ZoneDefinition::Bilateral { .. } => ZoneDefKind::Bilateral,
            ZoneDefinition::Unilateral { .. } => ZoneDefKind::Unilateral,
            ZoneDefinition::Unequal { .. } => ZoneDefKind::Unequal,
            ZoneDefinition::NonUniform(_) => ZoneDefKind::NonUniform,
        }
    }
}

/// A zone definition kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ZoneDefKind {
    /// `bilateral`.
    Bilateral,
    /// `unilateral`.
    Unilateral,
    /// `unequal`.
    Unequal,
    /// `non_uniform`.
    NonUniform,
}

impl ZoneDefKind {
    /// The stable code, also the argument of the `zone_def_unsupported:<kind>` limitation.
    pub const fn code(self) -> &'static str {
        match self {
            ZoneDefKind::Bilateral => "bilateral",
            ZoneDefKind::Unilateral => "unilateral",
            ZoneDefKind::Unequal => "unequal",
            ZoneDefKind::NonUniform => "non_uniform",
        }
    }

    /// Whether a limitation argument names this kind (`nonuniform` also names `non_uniform`).
    fn named_by(self, arg: &str) -> bool {
        arg == self.code() || (self == ZoneDefKind::NonUniform && arg == "nonuniform")
    }
}

/// A closed interval on the signed deviation, metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    /// Lower bound.
    pub lower_m: f64,
    /// Upper bound.
    pub upper_m: f64,
}

/// A constant-width zone: its interval, width `upper - lower` and disposition shift
/// `(lower + upper) * 0.5` (REQ-004).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConstantZone {
    /// The allowed interval.
    pub interval: Interval,
    /// `upper - lower`, metres.
    pub width_m: f64,
    /// `(lower + upper) * 0.5`, metres, signed like the deviations.
    pub shift_m: f64,
}

impl ConstantZone {
    /// Whether the deviation `d` lies in the interval, `lower <= d <= upper` (no
    /// arithmetic); `None` for NaN.
    pub fn contains(&self, d: f64) -> Option<bool> {
        if d.is_nan() {
            return None;
        }
        Some(self.interval.lower_m <= d && d <= self.interval.upper_m)
    }

    /// Membership of one `GDT.PROF.DEV` deviation; `None` when it has no projection or no
    /// signed deviation (no sign of record, or a foot on the edge of the toleranced set).
    pub fn contains_deviation(&self, dev: &Deviation) -> Option<bool> {
        let d = dev.projected()?.signed_dev?;
        self.contains(d)
    }

    /// The `GDT.ZONE.ZTYPE` offset shape of this zone; its size to compare against is
    /// [`ConstantZone::width_m`].
    pub fn offset_shape(&self) -> ZoneShape {
        ZoneShape::Offset { disposition_shift: self.shift_m }
    }
}

/// The intervals of a zone definition.
#[derive(Debug, Clone, PartialEq)]
pub enum ZoneIntervals {
    /// One interval for every point (the constant-width definitions).
    Constant(ConstantZone),
    /// One interval per point, in deviation order (filled by `GDT.PROF.ZDEF.NONUNIF`).
    PerPoint(Vec<Interval>),
}

/// The `zone_definition` fragment of the ResultRecord payload (REQ-007). All lengths in
/// metres; written as IEEE-754 hex bits by [`ZoneDefRecord::to_cvalue`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoneDefRecord {
    /// The definition kind.
    pub kind: ZoneDefKind,
    /// The stated total width.
    pub t: f64,
    /// The stated share (`Unequal` only).
    pub share: Option<f64>,
    /// The side the share lies on, as read from the chain (`Unequal` only).
    pub share_side: Option<Side>,
    /// The side of a unilateral zone (`Unilateral` only).
    pub side: Option<Side>,
    /// Interval lower bound.
    pub lower: f64,
    /// Interval upper bound.
    pub upper: f64,
    /// Disposition shift.
    pub shift: f64,
}

impl ZoneDefRecord {
    /// `{kind, lower, share, share_side, shift, side, t, upper}` with every length as
    /// `"0x"` + 16 lower-case hex digits of its bits and absent values as `null`.
    pub fn to_cvalue(&self) -> CValue {
        let hex = |x: f64| CValue::Str(format!("0x{:016x}", x.to_bits()));
        let side = |s: Option<Side>| s.map_or(CValue::Null, |s| CValue::Str(s.as_str().to_owned()));
        let mut m: BTreeMap<String, CValue> = BTreeMap::new();
        m.insert("kind".to_owned(), CValue::Str(self.kind.code().to_owned()));
        m.insert("lower".to_owned(), hex(self.lower));
        m.insert("share".to_owned(), self.share.map_or(CValue::Null, hex));
        m.insert("share_side".to_owned(), side(self.share_side));
        m.insert("shift".to_owned(), hex(self.shift));
        m.insert("side".to_owned(), side(self.side));
        m.insert("t".to_owned(), hex(self.t));
        m.insert("upper".to_owned(), hex(self.upper));
        CValue::Obj(m)
    }
}

/// An applied zone definition: its intervals and the record of what was applied.
#[derive(Debug, Clone, PartialEq)]
pub struct AppliedZone {
    /// The intervals.
    pub intervals: ZoneIntervals,
    /// The ResultRecord fragment.
    pub record: ZoneDefRecord,
}

/// A parameter named by [`ZdefError::BadZoneParameter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ZoneParam {
    /// The total width `t`.
    T,
    /// The stated share.
    Share,
    /// The chain's [`SHARE_SIDE_PARAM`].
    ShareSide,
}

impl ZoneParam {
    /// The parameter name.
    pub const fn name(self) -> &'static str {
        match self {
            ZoneParam::T => "t",
            ZoneParam::Share => "share",
            ZoneParam::ShareSide => "share_side",
        }
    }
}

/// Why no interval was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZdefError {
    /// `bad_zone_parameter{name}` (REQ-005): `t` NaN, not finite, `<= 0` or subnormal;
    /// `share` NaN, `< 0` or `> t`; the chain's `share_side` missing, not an `outward` /
    /// `inward` token, or stated twice with different values.
    BadZoneParameter {
        /// The parameter.
        name: ZoneParam,
    },
    /// `UNSUPPORTED{kind}` (REQ-006): the row lists the kind as unsupported, or the kind has
    /// no implementation in this build.
    Unsupported {
        /// The definition kind.
        kind: ZoneDefKind,
    },
}

impl ZdefError {
    /// The stable error code: `bad_zone_parameter` or `UNSUPPORTED`.
    pub fn code(&self) -> &'static str {
        match self {
            ZdefError::BadZoneParameter { .. } => "bad_zone_parameter",
            ZdefError::Unsupported { .. } => "UNSUPPORTED",
        }
    }
}

impl fmt::Display for ZdefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZdefError::BadZoneParameter { name } => write!(f, "bad_zone_parameter{{{}}}", name.name()),
            ZdefError::Unsupported { kind } => write!(f, "UNSUPPORTED{{{}}}", kind.code()),
        }
    }
}

impl std::error::Error for ZdefError {}

/// The intervals of `def` under the resolved `chain` and the row's `limitations`
/// (Build prompt: `zone_intervals(def, chain_params, row_limitations)`).
pub fn zone_intervals(
    def: &ZoneDefinition,
    chain: &OperatorChain,
    limitations: &[String],
) -> Result<ZoneIntervals, ZdefError> {
    apply(def, chain, limitations).map(|a| a.intervals)
}

/// [`zone_intervals`] together with the ResultRecord fragment.
pub fn apply(def: &ZoneDefinition, chain: &OperatorChain, limitations: &[String]) -> Result<AppliedZone, ZdefError> {
    let kind = def.kind();
    let listed =
        limitations.iter().filter_map(|l| l.strip_prefix(UNSUPPORTED_LIMITATION_PREFIX)).any(|arg| kind.named_by(arg));
    if listed {
        return Err(ZdefError::Unsupported { kind });
    }
    let (t, share, share_side, side, lower, upper) = match def {
        ZoneDefinition::NonUniform(_) => return Err(ZdefError::Unsupported { kind }),
        ZoneDefinition::Bilateral { t } => {
            let t = check_t(*t)?;
            let h = t * 0.5;
            (t, None, None, None, -h, h)
        }
        ZoneDefinition::Unilateral { t, side } => {
            let t = check_t(*t)?;
            match side {
                Side::Outward => (t, None, None, Some(*side), 0.0, t),
                Side::Inward => (t, None, None, Some(*side), -t, 0.0),
            }
        }
        ZoneDefinition::Unequal { t, share } => {
            let t = check_t(*t)?;
            let share = check_share(*share, t)?;
            let s = chain_share_side(chain)?;
            let rest = t - share;
            match s {
                Side::Outward => (t, Some(share), Some(s), None, -rest, share),
                Side::Inward => (t, Some(share), Some(s), None, -share, rest),
            }
        }
    };
    let width_m = upper - lower;
    let shift_m = (lower + upper) * 0.5;
    let zone = ConstantZone { interval: Interval { lower_m: lower, upper_m: upper }, width_m, shift_m };
    let record = ZoneDefRecord { kind, t, share, share_side, side, lower, upper, shift: shift_m };
    Ok(AppliedZone { intervals: ZoneIntervals::Constant(zone), record })
}

/// `t` must be a finite, normal, positive number (a subnormal width cannot be halved exactly).
fn check_t(t: f64) -> Result<f64, ZdefError> {
    if t.is_finite() && t >= f64::MIN_POSITIVE {
        Ok(t)
    } else {
        Err(ZdefError::BadZoneParameter { name: ZoneParam::T })
    }
}

/// `share` must lie in `[0, t]` (NaN fails both comparisons).
fn check_share(share: f64, t: f64) -> Result<f64, ZdefError> {
    if (0.0..=t).contains(&share) {
        Ok(share)
    } else {
        Err(ZdefError::BadZoneParameter { name: ZoneParam::Share })
    }
}

/// The chain's `share_side` token: present in at least one step, the same in every step.
fn chain_share_side(chain: &OperatorChain) -> Result<Side, ZdefError> {
    let bad = ZdefError::BadZoneParameter { name: ZoneParam::ShareSide };
    let mut found: Option<Side> = None;
    for step in chain.steps() {
        let Some(p) = step.params().get(SHARE_SIDE_PARAM) else { continue };
        let side = match p {
            Param::Token(s) => Side::parse(s).ok_or(bad)?,
            _ => return Err(bad),
        };
        match found {
            Some(prev) if prev != side => return Err(bad),
            _ => found = Some(side),
        }
    }
    found.ok_or(bad)
}

//! The typed error of section definitions (REQ-SCAN.SECT.DEF-004). Errors are values; nothing
//! in this crate panics (CONSTITUTION §8).

use core::fmt;

use fid_geo_frame::FrameId;
use fid_num::err::NumError;
use fid_types::Digest;

/// Why a definition is refused. Every refusal names its reason; nothing is dropped or
/// substituted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// A coordinate or scalar is NaN or infinite; `field` names it (for example
    /// `parallel.spacing`).
    NonFinite { field: &'static str },
    /// A normal or direction is not unit length within [`super::UNIT_TOL`].
    NotUnit { field: &'static str },
    /// The radial reference direction is not perpendicular to the axis within
    /// [`super::UNIT_TOL`] (`|d . r| > 1e-12`).
    NotPerpendicular,
    /// The parallel spacing is zero or negative.
    SpacingNotPositive,
    /// The radial angular step is zero or negative.
    StepNotPositive,
    /// The radial angular step is positive but at most [`super::ANGLE_SLACK`] (`1e-12` rad),
    /// the resolution of the sweep and full-turn checks.
    StepTooSmall,
    /// A count of zero.
    CountZero,
    /// A count above [`super::MAX_PLANES`], given or derived from an extent or a sweep.
    CountTooLarge,
    /// A negative parallel extent.
    ExtentNegative,
    /// A radial sweep that is zero or negative.
    SweepNotPositive,
    /// A radial series whose last plane reaches a full turn (`(k - 1) step >= 2 pi - 1e-12`),
    /// so it would repeat the first plane.
    RadialFullTurn,
    /// The expanded plane `index` has a non-finite point (the offset overflowed).
    PlaneNotFinite { index: u32 },
    /// The expanded plane `index` is bit-identical (point and normal) to plane `index - 1`: the
    /// offset or rotation step is below the resolution of the coordinates.
    CoincidentPlanes { index: u32 },
    /// The definition's frame and the frame of its referenced element differ.
    FrameMismatch { definition: FrameId, element: FrameId },
    /// The expanded radial normal `index` has zero length (cannot happen for a validated unit
    /// axis and perpendicular unit reference direction; refused rather than divided by zero).
    DegenerateNormal { index: u32 },
    /// A `fid-num` operation refused its input.
    Numeric(NumError),
    /// The resolver does not know the referenced element.
    ElementNotFound,
    /// The referenced CAD entity exists but is not a face; `kind` is its entity kind.
    ElementNotAFace { kind: &'static str },
    /// The referenced element is not planar; `kind` is what it is (`cylinder`, `nurbs`,
    /// `sphere`, a constructed `line3`, ...).
    ElementNotPlanar { kind: &'static str },
    /// The referenced element changed since the definition recorded it: the section must be
    /// recomputed (re-record the definition) before it is expanded again.
    ElementStale { recorded: Digest, current: Digest },
    /// The element's source refused to give its plane; `code` is the upstream error code (for
    /// example GEO.CAD.NOMINAL.DIRECT `NoOuterLoop`).
    ElementUpstream { code: &'static str },
}

impl Reason {
    /// A stable machine-readable reason code.
    pub fn code(&self) -> &'static str {
        match self {
            Reason::NonFinite { .. } => "non_finite",
            Reason::NotUnit { .. } => "not_unit",
            Reason::NotPerpendicular => "not_perpendicular",
            Reason::SpacingNotPositive => "spacing_not_positive",
            Reason::StepNotPositive => "step_not_positive",
            Reason::StepTooSmall => "step_too_small",
            Reason::CountZero => "count_zero",
            Reason::CountTooLarge => "count_too_large",
            Reason::ExtentNegative => "extent_negative",
            Reason::SweepNotPositive => "sweep_not_positive",
            Reason::RadialFullTurn => "radial_full_turn",
            Reason::PlaneNotFinite { .. } => "plane_not_finite",
            Reason::CoincidentPlanes { .. } => "coincident_planes",
            Reason::FrameMismatch { .. } => "frame_mismatch",
            Reason::DegenerateNormal { .. } => "degenerate_normal",
            Reason::Numeric(_) => "numeric",
            Reason::ElementNotFound => "element_not_found",
            Reason::ElementNotAFace { .. } => "element_not_a_face",
            Reason::ElementNotPlanar { .. } => "element_not_planar",
            Reason::ElementStale { .. } => "element_stale",
            Reason::ElementUpstream { .. } => "element_upstream",
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::NonFinite { field } => write!(f, "{field} is not finite"),
            Reason::NotUnit { field } => write!(f, "{field} is not unit length within 1e-12"),
            Reason::NotPerpendicular => {
                f.write_str("radial.ref_dir is not perpendicular to radial.axis_dir within 1e-12")
            }
            Reason::SpacingNotPositive => f.write_str("parallel spacing is not positive"),
            Reason::StepNotPositive => f.write_str("radial step is not positive"),
            Reason::StepTooSmall => f.write_str("radial step is at most 1e-12 rad"),
            Reason::CountZero => f.write_str("count is zero"),
            Reason::CountTooLarge => write!(f, "more than {} planes", super::MAX_PLANES),
            Reason::ExtentNegative => f.write_str("parallel extent is negative"),
            Reason::SweepNotPositive => f.write_str("radial sweep is not positive"),
            Reason::RadialFullTurn => f.write_str("radial series reaches a full turn and would repeat its first plane"),
            Reason::PlaneNotFinite { index } => write!(f, "plane {index} is not finite"),
            Reason::CoincidentPlanes { index } => write!(f, "plane {index} coincides with the plane before it"),
            Reason::FrameMismatch { definition, element } => {
                write!(f, "definition frame {definition} differs from element frame {element}")
            }
            Reason::DegenerateNormal { index } => write!(f, "radial normal {index} has zero length"),
            Reason::Numeric(e) => write!(f, "numeric: {e}"),
            Reason::ElementNotFound => f.write_str("referenced element not found"),
            Reason::ElementNotAFace { kind } => write!(f, "referenced CAD entity is a {kind}, not a face"),
            Reason::ElementNotPlanar { kind } => write!(f, "referenced element is not planar ({kind})"),
            Reason::ElementStale { recorded, current } => {
                write!(f, "referenced element changed (recorded {recorded}, current {current})")
            }
            Reason::ElementUpstream { code } => write!(f, "referenced element refused by its source: {code}"),
        }
    }
}

/// `scan.sect.bad_definition`: the one error of this module, with its [`Reason`] (boxed, so a
/// `Result` stays small: a stale-element reason carries two digests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectError {
    reason: Box<Reason>,
}

impl SectError {
    /// The stable error code of every refusal.
    pub const CODE: &'static str = "scan.sect.bad_definition";

    pub(crate) fn new(reason: Reason) -> SectError {
        SectError { reason: Box::new(reason) }
    }

    /// Always [`SectError::CODE`].
    pub fn code(&self) -> &'static str {
        Self::CODE
    }

    /// Why the definition was refused.
    pub fn reason(&self) -> &Reason {
        &self.reason
    }
}

impl From<Reason> for SectError {
    fn from(reason: Reason) -> Self {
        SectError::new(reason)
    }
}

impl From<NumError> for SectError {
    fn from(e: NumError) -> Self {
        SectError::new(Reason::Numeric(e))
    }
}

impl fmt::Display for SectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", Self::CODE, self.reason.code(), self.reason)
    }
}

impl core::error::Error for SectError {}

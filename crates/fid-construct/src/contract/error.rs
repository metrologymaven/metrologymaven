//! The construction error model (REQ-KRN.CONS.DECL-003, REQ-KRN.CONS-002).
//!
//! # Pinned semantics: undefined versus ill-conditioned
//!
//! - [`ConstructError::Undefined`]: the result does not exist, or is not unique, for the
//!   exact inputs, whatever the thresholds. The decision uses exact tests only (a product,
//!   determinant or separation that is exactly zero; bitwise-equal inputs; a configuration
//!   with no real solution) and names the condition with an [`UndefinedReason`].
//! - [`ConstructError::IllConditioned`]: the result exists for the exact inputs, but the
//!   construction's condition measure (an angle or a distance separating the inputs from
//!   the degenerate configuration, or a dimensionless ratio) is below the threshold the
//!   caller set in `ConstructParams`. It carries the [`Condition`]: the measure, the
//!   measured value and the threshold, with `0 <= value < threshold`, both finite.
//!
//! A construction tests `Undefined` first: when both apply it returns `Undefined`. The only
//! way to build an `IllConditioned` error is [`Condition::check`], which applies the pinned
//! comparison `value < threshold` (strict: a value equal to the threshold passes). A
//! threshold of zero therefore refuses nothing beyond the exact `Undefined` cases.
//!
//! Every error is a whole result: [`super::Construction::evaluate`] returns
//! `Result<ConstructedElement, ConstructError>`, so no partial element travels with an
//! error.

use core::fmt;

use fid_num::NumError;

use super::geom::InputKind;
use super::id::Role;

macro_rules! code_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($(#[$vm])* $variant,)+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The stable wire string.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $s,)+
                }
            }
        }
    };
}

code_enum!(
    /// Why a construction is geometrically undefined (exact tests only).
    UndefinedReason {
        /// Directions are exactly parallel (for example parallel planes, a line parallel to a
        /// plane).
        Parallel => "parallel",
        /// Inputs that must be distinct coincide exactly.
        Coincident => "coincident",
        /// Inputs that must span a plane are exactly collinear.
        Collinear => "collinear",
        /// A direction or extent the construction needs has zero length.
        ZeroLength => "zero_length",
        /// The inputs have no common point (for example disjoint circles or spheres).
        NoIntersection => "no_intersection",
        /// Lines are skew and the caller selected the strict rule.
        Skew => "skew",
        /// The result would be of a kind the construction does not produce (for example a
        /// conic section other than a circle or ellipse).
        NotSupported => "not_supported",
    }
);

code_enum!(
    /// What a condition measure measures, with its SI unit.
    Measure {
        /// An angle in radians.
        Angle => "angle",
        /// A distance in metres.
        Distance => "distance",
        /// A dimensionless ratio.
        Ratio => "ratio",
    }
);

impl Measure {
    /// The SI unit of the measure: `rad`, `m` or `1`.
    pub const fn unit(self) -> &'static str {
        match self {
            Measure::Angle => "rad",
            Measure::Distance => "m",
            Measure::Ratio => "1",
        }
    }
}

/// An ill-conditioning report: `measure` took `value`, below `threshold`. Built only by
/// [`Condition::check`], so `0 <= value < threshold` and both are finite; a zero value is
/// stored as `+0.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Condition {
    measure: Measure,
    value: f64,
    threshold: f64,
}

impl Condition {
    /// The pinned condition test. `value` is the construction's measured separation from
    /// the degenerate configuration, `threshold` the value from `ConstructParams`.
    ///
    /// 1. `value` or `threshold` NaN: [`ConstructError::Num`] (`NotANumber`); infinite:
    ///    [`ConstructError::Num`] (`Infinite`); negative: [`ConstructError::Num`] (`Domain`).
    /// 2. `value < threshold`: [`ConstructError::IllConditioned`].
    /// 3. Otherwise `Ok(())`.
    pub fn check(measure: Measure, value: f64, threshold: f64) -> Result<(), ConstructError> {
        const OP: &str = "construct.condition";
        for x in [value, threshold] {
            if x.is_nan() {
                return Err(ConstructError::Num(NumError::NotANumber { op: OP, index: None }));
            }
            if x.is_infinite() {
                return Err(ConstructError::Num(NumError::Infinite { op: OP, index: None }));
            }
            if x < 0.0 {
                return Err(ConstructError::Num(NumError::Domain { op: OP, detail: "negative_measure" }));
            }
        }
        if value < threshold {
            // `+ 0.0` turns a `-0.0` into `+0.0` and leaves every other value unchanged.
            return Err(ConstructError::IllConditioned(Condition { measure, value: value + 0.0, threshold }));
        }
        Ok(())
    }

    /// What was measured.
    pub fn measure(&self) -> Measure {
        self.measure
    }

    /// The measured value, in the measure's SI unit.
    pub fn value(&self) -> f64 {
        self.value
    }

    /// The threshold it fell below, in the measure's SI unit.
    pub fn threshold(&self) -> f64 {
        self.threshold
    }
}

/// Why a construction, a declaration or a contract operation returned no result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConstructError {
    /// The result does not exist or is not unique for the exact inputs (module
    /// documentation).
    Undefined { reason: UndefinedReason },
    /// The result exists but its condition measure is below the configured threshold
    /// (module documentation).
    IllConditioned(Condition),
    /// Input `index` (declared order) has kind `got` where the construction needs
    /// `expected`.
    WrongInputKind { index: usize, expected: InputKind, got: InputKind },
    /// `got` inputs were supplied where the declaration lists `expected`.
    InputCount { expected: usize, got: usize },
    /// The construction asked for an element reference that its declaration does not list.
    UndeclaredInput { role: Role },
    /// A declaration is malformed; `field` names the rule (for example `inputs.role`).
    InvalidDecl { field: &'static str },
    /// A parameter is malformed or out of range; `field` names it.
    InvalidParams { field: &'static str },
    /// The construction asked for a named parameter that `ConstructParams` does not hold, or
    /// holds with another quantity kind.
    MissingParam { name: &'static str },
    /// An input or output feature is not a valid fitted feature (`code` is the
    /// `fid_fit::contract::FitError` code, for example `non_finite` or `invalid_direction`).
    InvalidFeature { index: Option<usize>, code: &'static str },
    /// A numeric kernel error (non-finite value, domain error, ...).
    Num(NumError),
}

impl ConstructError {
    /// A stable machine-readable code.
    pub const fn code(&self) -> &'static str {
        match self {
            ConstructError::Undefined { .. } => "undefined",
            ConstructError::IllConditioned(_) => "ill_conditioned",
            ConstructError::WrongInputKind { .. } => "wrong_input_kind",
            ConstructError::InputCount { .. } => "input_count",
            ConstructError::UndeclaredInput { .. } => "undeclared_input",
            ConstructError::InvalidDecl { .. } => "invalid_decl",
            ConstructError::InvalidParams { .. } => "invalid_params",
            ConstructError::MissingParam { .. } => "missing_param",
            ConstructError::InvalidFeature { .. } => "invalid_feature",
            ConstructError::Num(_) => "numeric",
        }
    }
}

impl fmt::Display for ConstructError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConstructError::Undefined { reason } => write!(f, "construct: undefined ({})", reason.as_str()),
            ConstructError::IllConditioned(c) => write!(
                f,
                "construct: ill-conditioned ({} {:e} {} below threshold {:e})",
                c.measure.as_str(),
                c.value,
                c.measure.unit(),
                c.threshold
            ),
            ConstructError::WrongInputKind { index, expected, got } => {
                write!(f, "construct: input {index} is {got}, expected {expected}")
            }
            ConstructError::InputCount { expected, got } => {
                write!(f, "construct: {got} inputs supplied, {expected} declared")
            }
            ConstructError::UndeclaredInput { role } => {
                write!(f, "construct: input with role {} is not declared", role.as_str())
            }
            ConstructError::InvalidDecl { field } => write!(f, "construct: invalid declaration ({field})"),
            ConstructError::InvalidParams { field } => write!(f, "construct: invalid parameter {field}"),
            ConstructError::MissingParam { name } => write!(f, "construct: missing parameter {name}"),
            ConstructError::InvalidFeature { index: Some(i), code } => {
                write!(f, "construct: input {i} is not a valid feature ({code})")
            }
            ConstructError::InvalidFeature { index: None, code } => {
                write!(f, "construct: output is not a valid feature ({code})")
            }
            ConstructError::Num(e) => write!(f, "construct: {e}"),
        }
    }
}

impl core::error::Error for ConstructError {}

impl From<NumError> for ConstructError {
    fn from(e: NumError) -> Self {
        ConstructError::Num(e)
    }
}

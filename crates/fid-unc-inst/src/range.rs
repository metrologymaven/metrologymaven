//! The range check producers run before they evaluate a model (REQ-UNC.INST.SCHEMA-005,
//! REQ-UNC.INST-004).
//!
//! [`check_range`] compares an [`ObservationContext`] with every declared axis of the model,
//! in the model's canonical (sorted) axis order, against **closed** bounds: `min <= x <= max`
//! is inside. The verdict is:
//!
//! - [`RangeVerdict::OutOfModelRange`] for the first observed axis whose value lies below
//!   `min` (side `lower`, bound `min`) or above `max` (side `upper`, bound `max`). An axis
//!   out of range is reported before any axis that could not be checked.
//! - otherwise [`RangeVerdict::NotChecked`] for the first axis the context does not hold
//!   (`not_observed`) or holds in another SI unit (`unit_mismatch`). The model's validity was
//!   not established, so this is a flag too: producers treat every verdict other than
//!   [`RangeVerdict::InRange`] as "do not evaluate the model by extrapolation; attach the
//!   flag to the result" ([`RangeVerdict::must_flag`]).
//! - otherwise [`RangeVerdict::InRange`].
//!
//! Context axes the model does not declare are ignored: the model makes no claim on them.
//! Context values are SI doubles; a value that is not finite is refused when the context is
//! built, so NaN never reaches a comparison.

use crate::model::{hex, number_to_si, unit_of, InstrumentModel, SiUnit};
use fid_canon::CValue;
use std::collections::BTreeMap;

/// One observed axis value in SI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Observed {
    /// The value, finite.
    pub value: f64,
    /// Its SI unit.
    pub unit: SiUnit,
}

/// Why a context value was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextError {
    /// The value is not finite, or not a number in a known unit.
    BadValue,
    /// The axis is given twice.
    Duplicate,
}

/// The observation's values on named axes (`range`, `temperature`, ...).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObservationContext {
    axes: BTreeMap<String, Observed>,
}

impl ObservationContext {
    /// An empty context.
    pub fn new() -> ObservationContext {
        ObservationContext::default()
    }

    /// Adds an SI value. Not finite: `BadValue`; an axis given twice: `Duplicate`.
    pub fn with(mut self, axis: &str, value: f64, unit: SiUnit) -> Result<ObservationContext, ContextError> {
        if !value.is_finite() {
            return Err(ContextError::BadValue);
        }
        if self.axes.insert(axis.to_owned(), Observed { value, unit }).is_some() {
            return Err(ContextError::Duplicate);
        }
        Ok(self)
    }

    /// Adds a number text in a unit symbol, converted to SI as the model reader does.
    pub fn with_text(self, axis: &str, value: &str, unit: &str) -> Result<ObservationContext, ContextError> {
        let (si, shift) = unit_of(unit).ok_or(ContextError::BadValue)?;
        let x = number_to_si(value, shift).ok_or(ContextError::BadValue)?;
        self.with(axis, x, si)
    }

    /// The value on `axis`.
    pub fn get(&self, axis: &str) -> Option<Observed> {
        self.axes.get(axis).copied()
    }
}

/// Which bound was violated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundSide {
    /// Below `min`.
    Lower,
    /// Above `max`.
    Upper,
}

impl BoundSide {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            BoundSide::Lower => "lower",
            BoundSide::Upper => "upper",
        }
    }
}

/// The violated bound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangeBound {
    /// Lower or upper.
    pub side: BoundSide,
    /// The bound value.
    pub value: f64,
    /// Its SI unit.
    pub unit: SiUnit,
}

/// Why an axis could not be checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotCheckedReason {
    /// The context holds no value on the axis.
    NotObserved,
    /// The context's value is in another SI unit.
    UnitMismatch,
}

impl NotCheckedReason {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            NotCheckedReason::NotObserved => "not_observed",
            NotCheckedReason::UnitMismatch => "unit_mismatch",
        }
    }
}

/// The result of [`check_range`].
#[derive(Debug, Clone, PartialEq)]
pub enum RangeVerdict {
    /// Every declared axis is observed and inside its closed bounds.
    InRange,
    /// `out_of_model_range`: the observation lies outside the axis's bound.
    OutOfModelRange { axis: String, bound: RangeBound },
    /// `not_checked`: the axis could not be compared.
    NotChecked { axis: String, reason: NotCheckedReason },
}

impl RangeVerdict {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            RangeVerdict::InRange => "in_range",
            RangeVerdict::OutOfModelRange { .. } => "out_of_model_range",
            RangeVerdict::NotChecked { .. } => "not_checked",
        }
    }

    /// Whether a producer must attach this verdict to its result and not evaluate the model
    /// by extrapolation: every verdict but `InRange`.
    pub fn must_flag(&self) -> bool {
        !matches!(self, RangeVerdict::InRange)
    }

    /// The JSON value: `{"verdict"}`, plus `axis`, `side`, `bound` (hex) and `unit` for
    /// `out_of_model_range`, `axis` and `reason` for `not_checked`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("verdict".to_owned(), CValue::Str(self.code().to_owned()));
        match self {
            RangeVerdict::InRange => {}
            RangeVerdict::OutOfModelRange { axis, bound } => {
                o.insert("axis".to_owned(), CValue::Str(axis.clone()));
                o.insert("bound".to_owned(), CValue::Str(hex(bound.value)));
                o.insert("side".to_owned(), CValue::Str(bound.side.as_str().to_owned()));
                o.insert("unit".to_owned(), CValue::Str(bound.unit.as_str().to_owned()));
            }
            RangeVerdict::NotChecked { axis, reason } => {
                o.insert("axis".to_owned(), CValue::Str(axis.clone()));
                o.insert("reason".to_owned(), CValue::Str(reason.as_str().to_owned()));
            }
        }
        CValue::Obj(o)
    }
}

/// Checks `ctx` against every declared range axis of `model` (see the module docs).
pub fn check_range(model: &InstrumentModel, ctx: &ObservationContext) -> RangeVerdict {
    let mut pending: Option<RangeVerdict> = None;
    let mut defer = |axis: &str, reason: NotCheckedReason| {
        if pending.is_none() {
            pending = Some(RangeVerdict::NotChecked { axis: axis.to_owned(), reason });
        }
    };
    for a in model.range() {
        let Some(o) = ctx.get(&a.axis) else {
            defer(&a.axis, NotCheckedReason::NotObserved);
            continue;
        };
        if o.unit != a.unit {
            defer(&a.axis, NotCheckedReason::UnitMismatch);
            continue;
        }
        let side = if o.value < a.min {
            Some((BoundSide::Lower, a.min))
        } else if o.value > a.max {
            Some((BoundSide::Upper, a.max))
        } else {
            None
        };
        if let Some((side, value)) = side {
            return RangeVerdict::OutOfModelRange {
                axis: a.axis.clone(),
                bound: RangeBound { side, value, unit: a.unit },
            };
        }
    }
    pending.unwrap_or(RangeVerdict::InRange)
}

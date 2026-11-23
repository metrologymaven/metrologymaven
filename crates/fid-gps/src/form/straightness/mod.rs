//! Straightness evaluation and chain-driven dispatch (`GDT.FORM.STRAIGHT`).
//!
//! - [`line_elements`] (`GDT.FORM.STRAIGHT.LINEEL`): line-element extraction from cutting planes
//!   and per-element 2D straightness evaluation through `GDT.FORM.FCORE`.
//! - [`derived`] (`GDT.FORM.STRAIGHT.DERIVED`): derived median-line straightness with optional MMC.
//! - Dispatch (`GDT.FORM.STRAIGHT.CMD`): inspects the resolved chain for a median-line reduction
//!   operator and dispatches to [`derived::evaluate_derived_straightness`] (`D3`) or
//!   [`line_elements::evaluate_line_elements`] (`D0`).

pub mod derived;
pub mod line_elements;

use crate::dmt::OperatorChain;
use crate::form::{FitProvider, FormError, FormFlag};
use core::fmt;
use derived::{
    chain_has_median_line_reduction, evaluate_derived_straightness, DerivedStraightnessError,
    DerivedStraightnessPayload, DerivedToleranceSpec, MaterialConditionProvider, MedianLineDeriver,
};
use fid_canon::CValue;
use line_elements::{evaluate_line_elements, LineElementError, LineElementParams, LineElementPayload, ALG_LINE2D_MZ};
use std::collections::{BTreeMap, BTreeSet};

/// Straightness evaluation path selected from the resolved DMT operator chain
/// (`REQ-GDT.FORM.STRAIGHT.CMD-001`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StraightnessPath {
    LineElements,
    DerivedMedianLine,
}

impl StraightnessPath {
    pub const fn as_str(self) -> &'static str {
        match self {
            StraightnessPath::LineElements => "line_elements",
            StraightnessPath::DerivedMedianLine => "derived_median_line",
        }
    }

    pub const fn determinism(self) -> &'static str {
        match self {
            StraightnessPath::LineElements => "D0",
            StraightnessPath::DerivedMedianLine => "D3",
        }
    }
}

/// Errors produced by [`evaluate_straightness_dispatch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StraightnessError {
    LineElements(LineElementError),
    Derived(DerivedStraightnessError),
}

impl StraightnessError {
    pub const fn code(&self) -> &'static str {
        match self {
            StraightnessError::LineElements(e) => e.code(),
            StraightnessError::Derived(e) => e.code(),
        }
    }
}

impl fmt::Display for StraightnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StraightnessError::LineElements(e) => write!(f, "{e}"),
            StraightnessError::Derived(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StraightnessError {}

/// Unified payload emitted by `check.eval_straightness@1` (`GDT.FORM.STRAIGHT.CMD` Design):
/// `{ path, value_si, value_si_hex, algorithm_id, determinism, flags, line_elements?, derived? }`.
#[derive(Debug, Clone, PartialEq)]
pub struct StraightnessPayload {
    pub path: StraightnessPath,
    pub value_si: f64,
    pub algorithm_id: String,
    pub determinism: &'static str,
    pub flags: BTreeSet<FormFlag>,
    pub line_elements: Option<LineElementPayload>,
    pub derived: Option<DerivedStraightnessPayload>,
}

impl StraightnessPayload {
    pub fn to_cvalue(&self) -> Result<CValue, FormError> {
        let mut m = BTreeMap::new();
        m.insert("algorithm_id".to_owned(), CValue::Str(self.algorithm_id.clone()));
        m.insert("determinism".to_owned(), CValue::Str(self.determinism.to_owned()));
        m.insert(
            "flags".to_owned(),
            CValue::Arr(self.flags.iter().map(|f| CValue::Str(f.as_str().to_owned())).collect()),
        );
        m.insert("path".to_owned(), CValue::Str(self.path.as_str().to_owned()));
        m.insert("value_si".to_owned(), CValue::Num(self.value_si));
        m.insert("value_si_hex".to_owned(), CValue::Str(format!("{:016x}", self.value_si.to_bits())));
        if let Some(le) = &self.line_elements {
            m.insert("line_elements".to_owned(), le.to_cvalue()?);
        }
        if let Some(dv) = &self.derived {
            m.insert("derived".to_owned(), dv.to_cvalue());
        }
        Ok(CValue::Obj(m))
    }
}

/// Dispatches straightness evaluation by inspecting `chain` (`REQ-GDT.FORM.STRAIGHT.CMD-001`):
/// if `chain` contains a median-line reduction operator, evaluates the derived median-line path;
/// otherwise evaluates the line-element path.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_straightness_dispatch(
    points: &[[f64; 3]],
    chain: &OperatorChain,
    deriver: &dyn MedianLineDeriver,
    krn_line_el: &dyn FitProvider,
    krn_derived: &dyn FitProvider,
    tol: &DerivedToleranceSpec,
    matl: &dyn MaterialConditionProvider,
) -> Result<StraightnessPayload, StraightnessError> {
    if chain_has_median_line_reduction(chain) {
        let derived = evaluate_derived_straightness(points, chain, deriver, krn_derived, tol, matl)
            .map_err(StraightnessError::Derived)?;
        let value_si = derived.value_si;
        let algorithm_id = derived.algorithm_id.clone();
        let flags = derived.flags.clone();
        Ok(StraightnessPayload {
            path: StraightnessPath::DerivedMedianLine,
            value_si,
            algorithm_id,
            determinism: StraightnessPath::DerivedMedianLine.determinism(),
            flags,
            line_elements: None,
            derived: Some(derived),
        })
    } else {
        let params = LineElementParams::from_chain(chain).map_err(StraightnessError::LineElements)?;
        let le =
            evaluate_line_elements(points, &params, chain, krn_line_el).map_err(StraightnessError::LineElements)?;
        let value_si = le.max_value_si;
        let mut flags = BTreeSet::new();
        for el in &le.per_element {
            for &fl in &el.flags {
                flags.insert(fl);
            }
        }
        let algorithm_id = le
            .per_element
            .iter()
            .find(|e| e.index == le.attaining_element)
            .map(|e| e.payload.algorithm_id.clone())
            .unwrap_or_else(|| ALG_LINE2D_MZ.to_owned());
        Ok(StraightnessPayload {
            path: StraightnessPath::LineElements,
            value_si,
            algorithm_id,
            determinism: StraightnessPath::LineElements.determinism(),
            flags,
            line_elements: Some(le),
            derived: None,
        })
    }
}

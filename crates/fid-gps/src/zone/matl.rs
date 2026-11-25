//! Bonus tolerance arithmetic from actual size (spec node `GDT.ZONE.MATL.BONUS`,
//! `REQ-001`..`REQ-006`).
//!
//! Pure KNP-1 kernel arithmetic: SI metres, no I/O, no `unsafe`, no panics, no
//! `HashMap`/`HashSet`, no FMA, total-order comparisons, NaN is an error.

use crate::dmt::{OperatorChain, Param};
use core::cmp::Ordering;
use core::fmt;

/// Material-condition modifier on a toleranced feature of size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MaterialCondition {
    /// Maximum material condition (Ⓜ).
    Mmc,
    /// Least material condition (Ⓛ).
    Lmc,
}

impl MaterialCondition {
    pub const fn as_str(self) -> &'static str {
        match self {
            MaterialCondition::Mmc => "mmc",
            MaterialCondition::Lmc => "lmc",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "mmc" | "MMC" | "M" => Some(MaterialCondition::Mmc),
            "lmc" | "LMC" | "L" => Some(MaterialCondition::Lmc),
            _ => None,
        }
    }
}

/// Whether the toleranced feature of size is internal (hole/slot) or external (pin/tab).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeatureKind {
    Internal,
    External,
}

impl FeatureKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            FeatureKind::Internal => "internal",
            FeatureKind::External => "external",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "internal" | "hole" => Some(FeatureKind::Internal),
            "external" | "pin" | "shaft" => Some(FeatureKind::External),
            _ => None,
        }
    }
}

/// Actual size supplied by the size evaluation together with its solver provenance flags.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActualSize {
    pub size_m: f64,
    pub local_optimum_only: bool,
}

impl ActualSize {
    pub const fn new(size_m: f64) -> Self {
        Self { size_m, local_optimum_only: false }
    }

    pub const fn with_local_optimum_only(mut self, flag: bool) -> Self {
        self.local_optimum_only = flag;
        self
    }
}

/// Diagnostic flags carried on [`BonusRecord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BonusFlag {
    /// Actual size violated the material condition limit (`δ < 0`, `REQ-002`).
    ActualBeyondMaterialLimit,
    /// Bonus was capped at `|MMC - LMC|` per `bonus_cap: opposite_limit` (`REQ-003`).
    BonusCapped,
    /// Passed through from [`ActualSize::local_optimum_only`] (`REQ-005`).
    LocalOptimumOnly,
}

impl BonusFlag {
    pub const fn as_str(self) -> &'static str {
        match self {
            BonusFlag::ActualBeyondMaterialLimit => "actual_beyond_material_limit",
            BonusFlag::BonusCapped => "bonus_capped",
            BonusFlag::LocalOptimumOnly => "local_optimum_only",
        }
    }
}

/// Output of [`bonus`] (`REQ-004`).
#[derive(Debug, Clone, PartialEq)]
pub struct BonusRecord {
    pub stated_m: f64,
    pub bonus_m: f64,
    pub total_m: f64,
    pub departure_m: f64,
    pub actual_m: f64,
    pub actual_def: String,
    pub flags: Vec<BonusFlag>,
}

/// Errors returned by [`bonus`] (`REQ-006`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatlError {
    BadMaterialInput { detail: &'static str },
    ActualSizeUndefined,
}

impl MatlError {
    pub const fn code(&self) -> &'static str {
        match self {
            MatlError::BadMaterialInput { .. } => "bad_material_input",
            MatlError::ActualSizeUndefined => "actual_size_undefined",
        }
    }
}

impl fmt::Display for MatlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MatlError::BadMaterialInput { detail } => write!(f, "bad_material_input{{{detail}}}"),
            MatlError::ActualSizeUndefined => f.write_str("actual_size_undefined"),
        }
    }
}

impl std::error::Error for MatlError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BonusCap {
    None,
    OppositeLimit,
}

/// Computes material-condition bonus tolerance from actual size (`REQ-001`..`REQ-006`).
///
/// The resolved `chain` must declare `actual_size_def` (or `actual_size`) as a token parameter
/// on one of its steps (`actual_size_undefined` otherwise), and may declare
/// `bonus_cap = "opposite_limit"` to cap the bonus at `|mmc_size_m - lmc_size_m|`.
pub fn bonus(
    stated_m: f64,
    mmc_size_m: f64,
    lmc_size_m: f64,
    kind: FeatureKind,
    cond: MaterialCondition,
    actual: ActualSize,
    chain: &OperatorChain,
) -> Result<BonusRecord, MatlError> {
    if !stated_m.is_finite() || !mmc_size_m.is_finite() || !lmc_size_m.is_finite() || !actual.size_m.is_finite() {
        return Err(MatlError::BadMaterialInput { detail: "non_finite" });
    }
    if stated_m.total_cmp(&0.0) == Ordering::Less {
        return Err(MatlError::BadMaterialInput { detail: "negative_stated_tolerance" });
    }
    match kind {
        FeatureKind::Internal => {
            if mmc_size_m.total_cmp(&lmc_size_m) == Ordering::Greater {
                return Err(MatlError::BadMaterialInput { detail: "internal_mmc_exceeds_lmc" });
            }
        }
        FeatureKind::External => {
            if mmc_size_m.total_cmp(&lmc_size_m) == Ordering::Less {
                return Err(MatlError::BadMaterialInput { detail: "external_mmc_below_lmc" });
            }
        }
    }

    let mut actual_def: Option<String> = None;
    let mut cap = BonusCap::None;
    for step in chain.steps() {
        let params = step.params();
        if let Some(p) = params.get("actual_size_def").or_else(|| params.get("actual_size")) {
            match p {
                Param::Token(tok) if !tok.is_empty() => {
                    actual_def = Some(tok.clone());
                }
                _ => return Err(MatlError::BadMaterialInput { detail: "invalid_actual_size_def" }),
            }
        }
        if let Some(p) = params.get("bonus_cap") {
            match p {
                Param::Token(tok) if tok == "opposite_limit" => cap = BonusCap::OppositeLimit,
                Param::Token(tok) if tok == "none" => cap = BonusCap::None,
                _ => return Err(MatlError::BadMaterialInput { detail: "invalid_bonus_cap" }),
            }
        }
    }

    let Some(actual_def) = actual_def else {
        return Err(MatlError::ActualSizeUndefined);
    };

    // REQ-001: signed departure δ away from the specified material condition.
    let delta_raw = match (kind, cond) {
        (FeatureKind::Internal, MaterialCondition::Mmc) => actual.size_m - mmc_size_m,
        (FeatureKind::External, MaterialCondition::Mmc) => mmc_size_m - actual.size_m,
        (FeatureKind::Internal, MaterialCondition::Lmc) => lmc_size_m - actual.size_m,
        (FeatureKind::External, MaterialCondition::Lmc) => actual.size_m - lmc_size_m,
    };
    if !delta_raw.is_finite() {
        return Err(MatlError::BadMaterialInput { detail: "departure_overflow" });
    }
    let departure_m = canonical_zero(delta_raw);

    let mut flags = Vec::new();
    let mut bonus_m = if departure_m.total_cmp(&0.0) == Ordering::Less {
        flags.push(BonusFlag::ActualBeyondMaterialLimit);
        0.0
    } else {
        departure_m
    };

    // REQ-003: optional opposite-limit cap.
    if cap == BonusCap::OppositeLimit {
        let limit_span = (mmc_size_m - lmc_size_m).abs();
        if !limit_span.is_finite() {
            return Err(MatlError::BadMaterialInput { detail: "cap_overflow" });
        }
        if bonus_m.total_cmp(&limit_span) == Ordering::Greater {
            bonus_m = canonical_zero(limit_span);
            flags.push(BonusFlag::BonusCapped);
        }
    }

    let total_raw = stated_m + bonus_m;
    if !total_raw.is_finite() {
        return Err(MatlError::BadMaterialInput { detail: "total_overflow" });
    }
    let total_m = canonical_zero(total_raw);

    if actual.local_optimum_only {
        flags.push(BonusFlag::LocalOptimumOnly);
    }

    Ok(BonusRecord {
        stated_m: canonical_zero(stated_m),
        bonus_m: canonical_zero(bonus_m),
        total_m,
        departure_m,
        actual_m: canonical_zero(actual.size_m),
        actual_def,
        flags,
    })
}

fn canonical_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

//! Pure KNP-1 scalar property registry and min/max/range checks (spec node `GDT.DIM.SCALAR`,
//! `REQ-001`..`REQ-005`).
//!
//! Pure Rust, SI units (`si:m`, `si:rad`, `1`), no I/O, no `unsafe`, no panics, no
//! `HashMap`/`HashSet`, no FMA, fixed-order reductions, total-order min/max, NaN is an error.

use core::cmp::Ordering;
use core::fmt;

/// Algorithm ID for scalar property checks (`GDT.DIM.SCALAR`).
pub const SCALAR_ALGORITHM_ID: &str = "gdt.dim.scalar@1.0.0";
/// Algorithm ID for min/max/range checks (`GDT.DIM.SCALAR`).
pub const MINMAX_ALGORITHM_ID: &str = "gdt.dim.minmax@1.0.0";

/// Quantity kind of a registered property or value set (`REQ-002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QuantityKind {
    Length,
    Angle,
    Dimensionless,
}

impl QuantityKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            QuantityKind::Length => "length",
            QuantityKind::Angle => "angle",
            QuantityKind::Dimensionless => "dimensionless",
        }
    }

    pub const fn si_unit(self) -> &'static str {
        match self {
            QuantityKind::Length => "si:m",
            QuantityKind::Angle => "si:rad",
            QuantityKind::Dimensionless => "si:1",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "length" => Some(QuantityKind::Length),
            "angle" => Some(QuantityKind::Angle),
            "dimensionless" => Some(QuantityKind::Dimensionless),
            _ => None,
        }
    }

    /// Infers the quantity kind of a tolerance/unit designator (`REQ-002`, `AC-3`).
    pub fn from_unit(unit: &str) -> Option<Self> {
        match unit {
            "m" | "si:m" | "mm" | "um" | "µm" | "in" => Some(QuantityKind::Length),
            "rad" | "si:rad" | "deg" | "dms" => Some(QuantityKind::Angle),
            "1" | "si:1" | "dimensionless" | "ratio" => Some(QuantityKind::Dimensionless),
            _ => None,
        }
    }
}

/// Element carrying typed numeric properties exposed through [`PROPERTY_REGISTRY`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScalarElement {
    Circle { diameter: f64, radius: f64 },
    Cone { apex_angle_rad: f64, half_angle_rad: f64, length: f64 },
    Cylinder { diameter: f64, length: f64, radius: f64 },
    Line { length: f64 },
    Plane { area: f64 },
    Point { x: f64, y: f64, z: f64 },
    Slot { length: f64, width: f64 },
    Sphere { diameter: f64, radius: f64 },
}

impl ScalarElement {
    pub const fn kind_str(&self) -> &'static str {
        match self {
            ScalarElement::Circle { .. } => "circle",
            ScalarElement::Cone { .. } => "cone",
            ScalarElement::Cylinder { .. } => "cylinder",
            ScalarElement::Line { .. } => "line",
            ScalarElement::Plane { .. } => "plane",
            ScalarElement::Point { .. } => "point",
            ScalarElement::Slot { .. } => "slot",
            ScalarElement::Sphere { .. } => "sphere",
        }
    }
}

/// One entry in the static, sorted property registry (`REQ-001`, Design).
#[derive(Clone, Copy)]
pub struct PropertyEntry {
    pub element_kind: &'static str,
    pub property: &'static str,
    pub quantity_kind: QuantityKind,
    pub accessor: fn(&ScalarElement) -> Option<f64>,
}

fn acc_circle_diameter(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Circle { diameter, .. } => Some(diameter),
        _ => None,
    }
}
fn acc_circle_radius(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Circle { radius, .. } => Some(radius),
        _ => None,
    }
}
fn acc_cone_apex_angle(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Cone { apex_angle_rad, .. } => Some(apex_angle_rad),
        _ => None,
    }
}
fn acc_cone_half_angle(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Cone { half_angle_rad, .. } => Some(half_angle_rad),
        _ => None,
    }
}
fn acc_cone_length(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Cone { length, .. } => Some(length),
        _ => None,
    }
}
fn acc_cylinder_diameter(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Cylinder { diameter, .. } => Some(diameter),
        _ => None,
    }
}
fn acc_cylinder_length(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Cylinder { length, .. } => Some(length),
        _ => None,
    }
}
fn acc_cylinder_radius(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Cylinder { radius, .. } => Some(radius),
        _ => None,
    }
}
fn acc_line_length(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Line { length } => Some(length),
        _ => None,
    }
}
fn acc_plane_area(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Plane { area } => Some(area),
        _ => None,
    }
}
fn acc_point_x(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Point { x, .. } => Some(x),
        _ => None,
    }
}
fn acc_point_y(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Point { y, .. } => Some(y),
        _ => None,
    }
}
fn acc_point_z(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Point { z, .. } => Some(z),
        _ => None,
    }
}
fn acc_slot_length(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Slot { length, .. } => Some(length),
        _ => None,
    }
}
fn acc_slot_width(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Slot { width, .. } => Some(width),
        _ => None,
    }
}
fn acc_sphere_diameter(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Sphere { diameter, .. } => Some(diameter),
        _ => None,
    }
}
fn acc_sphere_radius(e: &ScalarElement) -> Option<f64> {
    match *e {
        ScalarElement::Sphere { radius, .. } => Some(radius),
        _ => None,
    }
}

/// Static sorted property registry `(element_kind, property) -> (quantity_kind, accessor)`.
/// Entries MUST stay sorted lexicographically by `(element_kind, property)`.
pub const PROPERTY_REGISTRY: &[PropertyEntry] = &[
    PropertyEntry {
        element_kind: "circle",
        property: "diameter",
        quantity_kind: QuantityKind::Length,
        accessor: acc_circle_diameter,
    },
    PropertyEntry {
        element_kind: "circle",
        property: "radius",
        quantity_kind: QuantityKind::Length,
        accessor: acc_circle_radius,
    },
    PropertyEntry {
        element_kind: "cone",
        property: "apex_angle",
        quantity_kind: QuantityKind::Angle,
        accessor: acc_cone_apex_angle,
    },
    PropertyEntry {
        element_kind: "cone",
        property: "half_angle",
        quantity_kind: QuantityKind::Angle,
        accessor: acc_cone_half_angle,
    },
    PropertyEntry {
        element_kind: "cone",
        property: "length",
        quantity_kind: QuantityKind::Length,
        accessor: acc_cone_length,
    },
    PropertyEntry {
        element_kind: "cylinder",
        property: "diameter",
        quantity_kind: QuantityKind::Length,
        accessor: acc_cylinder_diameter,
    },
    PropertyEntry {
        element_kind: "cylinder",
        property: "length",
        quantity_kind: QuantityKind::Length,
        accessor: acc_cylinder_length,
    },
    PropertyEntry {
        element_kind: "cylinder",
        property: "radius",
        quantity_kind: QuantityKind::Length,
        accessor: acc_cylinder_radius,
    },
    PropertyEntry {
        element_kind: "line",
        property: "length",
        quantity_kind: QuantityKind::Length,
        accessor: acc_line_length,
    },
    PropertyEntry {
        element_kind: "plane",
        property: "area",
        quantity_kind: QuantityKind::Dimensionless,
        accessor: acc_plane_area,
    },
    PropertyEntry { element_kind: "point", property: "x", quantity_kind: QuantityKind::Length, accessor: acc_point_x },
    PropertyEntry { element_kind: "point", property: "y", quantity_kind: QuantityKind::Length, accessor: acc_point_y },
    PropertyEntry { element_kind: "point", property: "z", quantity_kind: QuantityKind::Length, accessor: acc_point_z },
    PropertyEntry {
        element_kind: "slot",
        property: "length",
        quantity_kind: QuantityKind::Length,
        accessor: acc_slot_length,
    },
    PropertyEntry {
        element_kind: "slot",
        property: "width",
        quantity_kind: QuantityKind::Length,
        accessor: acc_slot_width,
    },
    PropertyEntry {
        element_kind: "sphere",
        property: "diameter",
        quantity_kind: QuantityKind::Length,
        accessor: acc_sphere_diameter,
    },
    PropertyEntry {
        element_kind: "sphere",
        property: "radius",
        quantity_kind: QuantityKind::Length,
        accessor: acc_sphere_radius,
    },
];

/// Binary-search lookup in [`PROPERTY_REGISTRY`].
pub fn lookup_property(element_kind: &str, property: &str) -> Option<&'static PropertyEntry> {
    let idx = PROPERTY_REGISTRY
        .binary_search_by(|entry| entry.element_kind.cmp(element_kind).then_with(|| entry.property.cmp(property)))
        .ok()?;
    PROPERTY_REGISTRY.get(idx)
}

/// Statistic requested by `check.eval_minmax@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MinMaxStat {
    Min,
    Max,
    Range,
}

impl MinMaxStat {
    pub const fn as_str(self) -> &'static str {
        match self {
            MinMaxStat::Min => "min",
            MinMaxStat::Max => "max",
            MinMaxStat::Range => "range",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "min" => Some(MinMaxStat::Min),
            "max" => Some(MinMaxStat::Max),
            "range" => Some(MinMaxStat::Range),
            _ => None,
        }
    }
}

/// Result of [`evaluate_scalar`].
#[derive(Debug, Clone, PartialEq)]
pub struct ScalarValue {
    pub element_kind: &'static str,
    pub property: &'static str,
    pub quantity_kind: QuantityKind,
    pub si_unit: &'static str,
    pub value_si: f64,
}

impl ScalarValue {
    pub const fn algorithm(&self) -> &'static str {
        SCALAR_ALGORITHM_ID
    }
}

/// Result of [`evaluate_minmax`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinMaxValue {
    pub statistic: MinMaxStat,
    pub quantity_kind: QuantityKind,
    pub si_unit: &'static str,
    pub value_si: f64,
    pub min_si: f64,
    pub max_si: f64,
    pub range_si: f64,
    pub min_index: u64,
    pub max_index: u64,
}

impl MinMaxValue {
    pub const fn algorithm(&self) -> &'static str {
        MINMAX_ALGORITHM_ID
    }
}

/// Errors of [`evaluate_scalar`] and [`evaluate_minmax`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScalarError {
    UnknownProperty { property: String },
    QuantityKindMismatch { expected: QuantityKind, got_unit: String },
    EmptyValueSet,
    NanValue { index: Option<u64> },
}

impl ScalarError {
    pub const fn code(&self) -> &'static str {
        match self {
            ScalarError::UnknownProperty { .. } => "unknown_property",
            ScalarError::QuantityKindMismatch { .. } => "quantity_kind_mismatch",
            ScalarError::EmptyValueSet => "empty_value_set",
            ScalarError::NanValue { .. } => "nan_value",
        }
    }
}

impl fmt::Display for ScalarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScalarError::UnknownProperty { property } => write!(f, "unknown_property{{{property}}}"),
            ScalarError::QuantityKindMismatch { expected, got_unit } => {
                write!(f, "quantity_kind_mismatch{{expected={}, got_unit={got_unit}}}", expected.as_str())
            }
            ScalarError::EmptyValueSet => f.write_str("empty_value_set"),
            ScalarError::NanValue { index: Some(i) } => write!(f, "nan_value{{index={i}}}"),
            ScalarError::NanValue { index: None } => f.write_str("nan_value"),
        }
    }
}

impl std::error::Error for ScalarError {}

/// Evaluates a registered scalar property of `element` (`REQ-001`, `REQ-002`, `REQ-005`).
///
/// `property` may be either bare (`"length"`) or qualified (`"cylinder.length"`). When
/// `tolerance_unit` is `Some(u)`, its quantity kind must match the registered property's
/// quantity kind or `quantity_kind_mismatch` is returned.
pub fn evaluate_scalar(
    element: &ScalarElement,
    property: &str,
    tolerance_unit: Option<&str>,
) -> Result<ScalarValue, ScalarError> {
    let kind = element.kind_str();
    let prop_name = match property.split_once('.') {
        Some((prefix, rest)) => {
            if prefix != kind {
                return Err(ScalarError::UnknownProperty { property: property.to_owned() });
            }
            rest
        }
        None => property,
    };
    let Some(entry) = lookup_property(kind, prop_name) else {
        return Err(ScalarError::UnknownProperty { property: property.to_owned() });
    };
    if let Some(u) = tolerance_unit {
        let got = QuantityKind::from_unit(u);
        if got != Some(entry.quantity_kind) {
            return Err(ScalarError::QuantityKindMismatch { expected: entry.quantity_kind, got_unit: u.to_owned() });
        }
    }
    let Some(raw) = (entry.accessor)(element) else {
        return Err(ScalarError::UnknownProperty { property: property.to_owned() });
    };
    if !raw.is_finite() {
        return Err(ScalarError::NanValue { index: None });
    }
    Ok(ScalarValue {
        element_kind: entry.element_kind,
        property: entry.property,
        quantity_kind: entry.quantity_kind,
        si_unit: entry.quantity_kind.si_unit(),
        value_si: canonical_zero(raw),
    })
}

/// Evaluates min, max, and range in one fixed-order pass over `values` using total-order
/// comparisons and lowest-index tie-breaking (`REQ-003`, `REQ-004`, `REQ-005`).
pub fn evaluate_minmax(
    values: &[f64],
    statistic: MinMaxStat,
    quantity_kind: QuantityKind,
) -> Result<MinMaxValue, ScalarError> {
    let Some(&first) = values.first() else {
        return Err(ScalarError::EmptyValueSet);
    };
    if !first.is_finite() {
        return Err(ScalarError::NanValue { index: Some(0) });
    }
    let mut min_val = first;
    let mut max_val = first;
    let mut min_idx = 0u64;
    let mut max_idx = 0u64;

    for (i, &v) in values.iter().enumerate().skip(1) {
        let idx = u64::try_from(i).unwrap_or(u64::MAX);
        if !v.is_finite() {
            return Err(ScalarError::NanValue { index: Some(idx) });
        }
        if v.total_cmp(&min_val) == Ordering::Less {
            min_val = v;
            min_idx = idx;
        }
        if v.total_cmp(&max_val) == Ordering::Greater {
            max_val = v;
            max_idx = idx;
        }
    }

    let min_si = canonical_zero(min_val);
    let max_si = canonical_zero(max_val);
    let range_si = canonical_zero(max_si - min_si);
    if !range_si.is_finite() {
        return Err(ScalarError::NanValue { index: None });
    }
    let value_si = match statistic {
        MinMaxStat::Min => min_si,
        MinMaxStat::Max => max_si,
        MinMaxStat::Range => range_si,
    };
    Ok(MinMaxValue {
        statistic,
        quantity_kind,
        si_unit: quantity_kind.si_unit(),
        value_si,
        min_si,
        max_si,
        range_si,
        min_index: min_idx,
        max_index: max_idx,
    })
}

fn canonical_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

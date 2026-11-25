//! AP242 PMI entity graph extraction and classification (spec node `IO.CAD.PMI.READ`).
//!
//! [`read_pmi`] walks a parsed AP242 file ([`P21File`]) together with its resolved unit
//! contexts ([`StepHeaderInfo`]) and returns a [`PmiGraph`]: every PMI-related instance
//! exactly once, keyed by entity number, classified as `semantic`, `graphic_only` or
//! `unsupported` before any QIF mapping happens, so nothing can be dropped silently later.
//!
//! - Semantic items (tolerances, dimensions, datum features, datums, datum targets, datum
//!   systems and references, shape aspects, plus-minus tolerances and their values) list
//!   the faces and edges their shape aspects resolve to through the usage entities
//!   (`links`), the PMI items they refer to (`refs`), the presentations linked to them
//!   (`presented_by`), and their numeric values as authored lexemes with the unit of their
//!   representation context (`values`).
//! - A reference to an entity number the file does not define is kept as
//!   `link_unresolved`, never dropped.
//! - Presentation items (annotation occurrences and callouts) are `semantic` when an
//!   association links them to a PMI item and `graphic_only` otherwise; a graphic-only
//!   item carries no characteristic data.
//! - A semantic instance whose type or complex-entity combination is not in the supported
//!   table ([`names::SUPPORTED`]) is `unsupported`, with its full entity-type list.
//!
//! The rules are pinned in the leaf's "Pinned at build" section. The walk is iterative and
//! bounded ([`PmiOptions`]): item count, tracked allocation and work steps. Every
//! structurally impossible reference is a typed [`PmiError::Malformed`]; nothing panics.
//! Output is deterministic (D0): items in ascending entity number, lists sorted.

pub mod names;
mod read;

pub use read::{read_pmi, read_pmi_with};

use fid_io_step::{AngleUnit, Ident, Param, StepLengthUnit};
use std::collections::BTreeMap;
use std::fmt;

/// Default limit on the number of graph items.
pub const DEFAULT_MAX_ITEMS: usize = 1_000_000;
/// Default limit on the tracked allocation of the graph and its indexes, in bytes.
pub const DEFAULT_MAX_ALLOC_BYTES: u64 = 1 << 30;
/// Default limit on work steps (parameters visited, index entries, closure steps).
pub const DEFAULT_MAX_WORK: u64 = 200_000_000;

/// Nominal bytes charged per graph item: the item, its map entry and node share.
pub const ITEM_COST: u64 = 512;
/// Nominal bytes charged per raw parameter kept (a `Param` in a growing `Vec`).
pub const PARAM_COST: u64 = 64;
/// Nominal bytes charged per entity-type name kept in `types` or `raw`.
pub const IDENT_COST: u64 = 32;
/// Nominal bytes charged per entity number kept in a list or an index.
pub const LINK_COST: u64 = 16;
/// Nominal bytes charged per index or visited-set entry (map node share included).
pub const INDEX_COST: u64 = 64;
/// Nominal bytes charged per value, on top of its lexeme length.
pub const VALUE_COST: u64 = 160;

/// The three-way classification (REQ-IO.CAD.PMI.READ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PmiClass {
    /// A supported semantic construct, or a presentation linked to a PMI item.
    Semantic,
    /// A presentation with no association to a PMI item.
    GraphicOnly,
    /// A semantic construct the reader does not model.
    Unsupported,
}

impl PmiClass {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            PmiClass::Semantic => "semantic",
            PmiClass::GraphicOnly => "graphic_only",
            PmiClass::Unsupported => "unsupported",
        }
    }
}

/// What an item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PmiKind {
    /// A geometric tolerance (any supported type and complex form).
    Tolerance,
    /// A size or location dimension.
    Dimension,
    /// The representation carrying a dimension's values.
    DimensionRepresentation,
    /// A plus-minus tolerance on a dimension.
    PlusMinus,
    /// A tolerance value (lower and upper bound).
    ToleranceValue,
    /// A limits-and-fits designation.
    LimitsAndFits,
    /// A shape aspect (plain or composite).
    ShapeAspect,
    /// A datum feature.
    DatumFeature,
    /// A datum.
    Datum,
    /// A datum target.
    DatumTarget,
    /// A datum system.
    DatumSystem,
    /// A datum reference compartment or element.
    DatumReference,
    /// A presentation item (annotation occurrence or callout).
    Presentation,
    /// An unsupported item whose names do not agree on one kind.
    Unknown,
}

impl PmiKind {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            PmiKind::Tolerance => "tolerance",
            PmiKind::Dimension => "dimension",
            PmiKind::DimensionRepresentation => "dimension_representation",
            PmiKind::PlusMinus => "plus_minus",
            PmiKind::ToleranceValue => "tolerance_value",
            PmiKind::LimitsAndFits => "limits_and_fits",
            PmiKind::ShapeAspect => "shape_aspect",
            PmiKind::DatumFeature => "datum_feature",
            PmiKind::Datum => "datum",
            PmiKind::DatumTarget => "datum_target",
            PmiKind::DatumSystem => "datum_system",
            PmiKind::DatumReference => "datum_reference",
            PmiKind::Presentation => "presentation",
            PmiKind::Unknown => "unknown",
        }
    }

    /// True for the kinds whose shape aspects resolve to faces and edges.
    pub fn has_geometry(self) -> bool {
        matches!(
            self,
            PmiKind::Tolerance
                | PmiKind::Dimension
                | PmiKind::ShapeAspect
                | PmiKind::DatumFeature
                | PmiKind::Datum
                | PmiKind::DatumTarget
        )
    }
}

/// Why an item is `unsupported`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnsupportedReason {
    /// A part name is not in the supported table.
    TypeNotSupported,
    /// The supported names do not agree on one kind.
    MixedKinds,
    /// More than one geometric tolerance type in one instance.
    TwoToleranceTypes,
    /// Presentation and semantic names in one instance.
    MixedPresentation,
}

impl UnsupportedReason {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            UnsupportedReason::TypeNotSupported => "type_not_supported",
            UnsupportedReason::MixedKinds => "mixed_kinds",
            UnsupportedReason::TwoToleranceTypes => "two_tolerance_types",
            UnsupportedReason::MixedPresentation => "mixed_presentation",
        }
    }
}

/// The unit of a value, read from its unit context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ValueUnit {
    /// The context's effective length unit.
    Length(StepLengthUnit),
    /// The context's plane-angle unit.
    Angle(AngleUnit),
    /// No context assigns the measure's unit, or the unit is neither a length nor a
    /// plane angle, or it is not defined.
    Unresolved,
}

impl ValueUnit {
    /// `mm`, `in`, ..., `radian`, `degree` or `unresolved`.
    pub fn as_str(self) -> &'static str {
        match self {
            ValueUnit::Length(u) => u.symbol(),
            ValueUnit::Angle(a) => a.as_str(),
            ValueUnit::Unresolved => "unresolved",
        }
    }
}

/// One numeric value as authored (REQ-IO.CAD.PMI.READ-005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PmiValue {
    /// The measure-with-unit instance carrying the value.
    pub measure: u64,
    /// The measure type name (`LENGTH_MEASURE`, ...), when the value is typed.
    pub measure_type: Option<Ident>,
    /// For a real, its authored lexeme as the file spells it. For an integer, its decimal
    /// form: the parser keeps no integer spelling, so `+05` reads as `5`. `None` when the
    /// value is neither, or is a real without a lexeme (only in files built in memory).
    pub lexeme: Option<Box<str>>,
    /// The unit entity the measure names.
    pub unit_entity: Option<u64>,
    /// The representation context the unit is read from.
    pub unit_ctx: Option<u64>,
    /// The unit.
    pub unit: ValueUnit,
}

/// One graph item.
#[derive(Debug, Clone, PartialEq)]
pub struct PmiItem {
    /// The entity number.
    pub entity: u64,
    /// The full entity-type list, in file order.
    pub types: Vec<Ident>,
    /// The classification.
    pub class: PmiClass,
    /// The kind.
    pub kind: PmiKind,
    /// Why the item is unsupported (`None` otherwise).
    pub unsupported: Option<UnsupportedReason>,
    /// Entity numbers of the faces and edges the item's shape aspects resolve to, sorted.
    pub links: Vec<u64>,
    /// Entity numbers the item refers to (directly or through its aspects, usages,
    /// measures or associations) that the file does not define, sorted.
    pub link_unresolved: Vec<u64>,
    /// The PMI items this item refers to (for a linked presentation: the semantic items
    /// it presents), sorted.
    pub refs: Vec<u64>,
    /// The presentation items linked to this item, sorted.
    pub presented_by: Vec<u64>,
    /// Numeric values in parameter order.
    pub values: Vec<PmiValue>,
    /// The instance's parts as parsed.
    pub raw: Vec<(Ident, Vec<Param>)>,
}

/// The PMI graph of one file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PmiGraph {
    /// Every PMI-related instance, by entity number.
    pub items: BTreeMap<u64, PmiItem>,
}

impl PmiGraph {
    /// The number of items with this class.
    pub fn count(&self, class: PmiClass) -> usize {
        self.items.values().filter(|i| i.class == class).count()
    }

    /// Items with this class, ascending.
    pub fn with_class(&self, class: PmiClass) -> impl Iterator<Item = &PmiItem> {
        self.items.values().filter(move |i| i.class == class)
    }
}

/// Bounds of one read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PmiOptions {
    /// Maximum number of graph items.
    pub max_items: usize,
    /// Maximum tracked allocation, in bytes (graph, indexes and transient sets).
    pub max_alloc_bytes: u64,
    /// Maximum work steps.
    pub max_work: u64,
}

impl Default for PmiOptions {
    fn default() -> Self {
        PmiOptions {
            max_items: DEFAULT_MAX_ITEMS,
            max_alloc_bytes: DEFAULT_MAX_ALLOC_BYTES,
            max_work: DEFAULT_MAX_WORK,
        }
    }
}

/// Which bound a [`PmiError::LimitExceeded`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PmiBound {
    /// [`PmiOptions::max_items`].
    Items,
    /// [`PmiOptions::max_alloc_bytes`].
    Alloc,
    /// [`PmiOptions::max_work`].
    Work,
}

impl PmiBound {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            PmiBound::Items => "items",
            PmiBound::Alloc => "alloc",
            PmiBound::Work => "work",
        }
    }
}

/// Every outcome other than a graph. Reasons are static strings chosen by the reader,
/// never text copied from the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PmiError {
    /// A structurally impossible reference or attribute.
    Malformed {
        /// The instance concerned.
        entity: u64,
        /// Static reason code.
        reason: &'static str,
    },
    /// A bound was exceeded.
    LimitExceeded {
        /// The bound.
        bound: PmiBound,
        /// Its configured value.
        value: u64,
    },
}

impl PmiError {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            PmiError::Malformed { .. } => "malformed",
            PmiError::LimitExceeded { .. } => "limit_exceeded",
        }
    }
}

impl fmt::Display for PmiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PmiError::Malformed { entity, reason } => write!(f, "malformed: {reason} (entity #{entity})"),
            PmiError::LimitExceeded { bound, value } => {
                write!(f, "limit_exceeded: bound {} = {value}", bound.as_str())
            }
        }
    }
}

impl std::error::Error for PmiError {}

//! The entity-name tables and the classification of one instance by its part names.
//!
//! Names only; the tables carry no standards or recommended-practice text. Which names
//! count as PMI-related, and which of those the reader models, is pinned in the leaf's
//! "Pinned at build" section (spec/io/cad/pmi/read/NODE.md) [unverified: the complete
//! entity list is still to be confirmed against the CAx-IF recommended practices].

use crate::{PmiKind, UnsupportedReason};
use fid_io_step::Ident;

/// The geometric tolerance types (at most one per instance).
pub const TOLERANCE_TYPES: &[&str] = &[
    "ANGULARITY_TOLERANCE",
    "CIRCULAR_RUNOUT_TOLERANCE",
    "COAXIALITY_TOLERANCE",
    "CONCENTRICITY_TOLERANCE",
    "CYLINDRICITY_TOLERANCE",
    "FLATNESS_TOLERANCE",
    "LINE_PROFILE_TOLERANCE",
    "PARALLELISM_TOLERANCE",
    "PERPENDICULARITY_TOLERANCE",
    "POSITION_TOLERANCE",
    "ROUNDNESS_TOLERANCE",
    "STRAIGHTNESS_TOLERANCE",
    "SURFACE_PROFILE_TOLERANCE",
    "SYMMETRY_TOLERANCE",
    "TOTAL_RUNOUT_TOLERANCE",
];

/// The supported semantic entity names and their kinds. A semantic instance is supported
/// only when every part name is in this table.
pub const SUPPORTED: &[(&str, PmiKind)] = &[
    ("ALL_AROUND_SHAPE_ASPECT", PmiKind::ShapeAspect),
    ("ANGULARITY_TOLERANCE", PmiKind::Tolerance),
    ("CIRCULAR_RUNOUT_TOLERANCE", PmiKind::Tolerance),
    ("COAXIALITY_TOLERANCE", PmiKind::Tolerance),
    ("COMPOSITE_GROUP_SHAPE_ASPECT", PmiKind::ShapeAspect),
    ("COMPOSITE_SHAPE_ASPECT", PmiKind::ShapeAspect),
    ("CONCENTRICITY_TOLERANCE", PmiKind::Tolerance),
    ("CYLINDRICITY_TOLERANCE", PmiKind::Tolerance),
    ("DATUM", PmiKind::Datum),
    ("DATUM_FEATURE", PmiKind::DatumFeature),
    ("DATUM_REFERENCE_COMPARTMENT", PmiKind::DatumReference),
    ("DATUM_REFERENCE_ELEMENT", PmiKind::DatumReference),
    ("DATUM_SYSTEM", PmiKind::DatumSystem),
    ("DATUM_TARGET", PmiKind::DatumTarget),
    ("DIMENSIONAL_CHARACTERISTIC_REPRESENTATION", PmiKind::DimensionRepresentation),
    ("DIMENSIONAL_LOCATION", PmiKind::Dimension),
    ("DIMENSIONAL_SIZE", PmiKind::Dimension),
    ("FLATNESS_TOLERANCE", PmiKind::Tolerance),
    ("GENERAL_DATUM_REFERENCE", PmiKind::DatumReference),
    ("GEOMETRIC_TOLERANCE", PmiKind::Tolerance),
    ("GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE", PmiKind::Tolerance),
    ("GEOMETRIC_TOLERANCE_WITH_DEFINED_AREA_UNIT", PmiKind::Tolerance),
    ("GEOMETRIC_TOLERANCE_WITH_DEFINED_UNIT", PmiKind::Tolerance),
    ("GEOMETRIC_TOLERANCE_WITH_MODIFIERS", PmiKind::Tolerance),
    ("LIMITS_AND_FITS", PmiKind::LimitsAndFits),
    ("LINE_PROFILE_TOLERANCE", PmiKind::Tolerance),
    ("PARALLELISM_TOLERANCE", PmiKind::Tolerance),
    ("PERPENDICULARITY_TOLERANCE", PmiKind::Tolerance),
    ("PLUS_MINUS_TOLERANCE", PmiKind::PlusMinus),
    ("POSITION_TOLERANCE", PmiKind::Tolerance),
    ("ROUNDNESS_TOLERANCE", PmiKind::Tolerance),
    ("SHAPE_ASPECT", PmiKind::ShapeAspect),
    ("STRAIGHTNESS_TOLERANCE", PmiKind::Tolerance),
    ("SURFACE_PROFILE_TOLERANCE", PmiKind::Tolerance),
    ("SYMMETRY_TOLERANCE", PmiKind::Tolerance),
    ("TOLERANCE_VALUE", PmiKind::ToleranceValue),
    ("TOTAL_RUNOUT_TOLERANCE", PmiKind::Tolerance),
];

/// Further PMI-related names that the word rules below do not catch (all unsupported).
pub const EXTRA_RELATED: &[&str] = &[
    "ANGULAR_LOCATION",
    "ANGULAR_SIZE",
    "APEX",
    "CENTRE_OF_SYMMETRY",
    "EXTENSION",
    "GEOMETRIC_ALIGNMENT",
    "GEOMETRIC_INTERSECTION",
    "PARALLEL_OFFSET",
    "PERPENDICULAR_TO",
    "TANGENT",
];

/// Shape-aspect names whose members (related aspects of a `SHAPE_ASPECT_RELATIONSHIP`)
/// contribute their geometry.
pub const COMPOSITE_ASPECTS: &[&str] =
    &["ALL_AROUND_SHAPE_ASPECT", "COMPOSITE_GROUP_SHAPE_ASPECT", "COMPOSITE_SHAPE_ASPECT"];

/// Usage entities that tie a shape aspect to geometry (or, for the associations, a
/// semantic item to a presentation). The attributes sit in the first part named here.
pub const USAGE_ROOT: &str = "ITEM_IDENTIFIED_REPRESENTATION_USAGE";
/// Usage entity names.
pub const USAGE_NAMES: &[&str] = &[
    "DRAUGHTING_MODEL_ITEM_ASSOCIATION",
    "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER",
    "GEOMETRIC_ITEM_SPECIFIC_USAGE",
    "ITEM_IDENTIFIED_REPRESENTATION_USAGE",
];
/// The usage names that link a semantic item to a presentation.
pub const ASSOCIATION_NAMES: &[&str] =
    &["DRAUGHTING_MODEL_ITEM_ASSOCIATION", "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER"];

/// The kind of a supported semantic name.
pub fn supported_kind(name: &str) -> Option<PmiKind> {
    SUPPORTED.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
}

/// A presentation name: a callout, or an annotation occurrence.
pub fn is_presentation_name(name: &str) -> bool {
    is_callout_name(name)
        || name.ends_with("ANNOTATION_OCCURRENCE")
        || (name.starts_with("ANNOTATION_") && name.ends_with("_OCCURRENCE"))
}

/// A callout name (`DRAUGHTING_CALLOUT` or any `..._CALLOUT`).
pub fn is_callout_name(name: &str) -> bool {
    name == "DRAUGHTING_CALLOUT" || name.ends_with("_CALLOUT")
}

/// A semantic PMI-related name (supported or not).
pub fn is_semantic_name(name: &str) -> bool {
    if is_presentation_name(name) {
        return false;
    }
    if supported_kind(name).is_some() || EXTRA_RELATED.contains(&name) {
        return true;
    }
    if name == "DIMENSIONAL_EXPONENTS" {
        return false;
    }
    name.ends_with("SHAPE_ASPECT") || name.split('_').any(|w| w == "TOLERANCE" || w == "DATUM" || w == "DIMENSIONAL")
}

/// How the reader treats one instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    /// Not PMI-related.
    NotPmi,
    /// A presentation item (linked or graphic-only, decided later).
    Presentation,
    /// A supported semantic item.
    Supported(PmiKind),
    /// A PMI-related item the reader does not model.
    Unsupported(PmiKind, UnsupportedReason),
}

/// The single kind the supported names of an instance agree on (`ShapeAspect` yields to
/// any other kind), or `Unknown`.
fn kind_of(names: &[&Ident]) -> PmiKind {
    let mut kinds: Vec<PmiKind> = Vec::new();
    for n in names {
        if let Some(k) = supported_kind(n.as_str()) {
            if !kinds.contains(&k) {
                kinds.push(k);
            }
        }
    }
    if kinds.len() > 1 {
        kinds.retain(|k| *k != PmiKind::ShapeAspect);
    }
    match kinds.as_slice() {
        [k] => *k,
        _ => PmiKind::Unknown,
    }
}

/// Classifies an instance by its part names.
pub(crate) fn classify(names: &[&Ident]) -> Class {
    let has_p = names.iter().any(|n| is_presentation_name(n.as_str()));
    let has_s = names.iter().any(|n| is_semantic_name(n.as_str()));
    match (has_p, has_s) {
        (false, false) => return Class::NotPmi,
        (true, true) => return Class::Unsupported(kind_of(names), UnsupportedReason::MixedPresentation),
        (true, false) => return Class::Presentation,
        (false, true) => {}
    }
    if names.iter().any(|n| supported_kind(n.as_str()).is_none()) {
        return Class::Unsupported(kind_of(names), UnsupportedReason::TypeNotSupported);
    }
    let kind = kind_of(names);
    if kind == PmiKind::Unknown {
        return Class::Unsupported(kind, UnsupportedReason::MixedKinds);
    }
    let types = names.iter().filter(|n| TOLERANCE_TYPES.contains(&n.as_str())).count();
    if types > 1 {
        return Class::Unsupported(kind, UnsupportedReason::TwoToleranceTypes);
    }
    Class::Supported(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_table_is_sorted_and_unique() {
        assert!(SUPPORTED.windows(2).all(|w| matches!(w, [a, b] if a.0 < b.0)));
        assert!(TOLERANCE_TYPES.iter().all(|t| supported_kind(t) == Some(PmiKind::Tolerance)));
    }

    #[test]
    fn name_rules() {
        assert!(is_semantic_name("TOLERANCE_ZONE"));
        assert!(is_semantic_name("CENTRE_OF_SYMMETRY"));
        assert!(is_semantic_name("DERIVED_SHAPE_ASPECT"));
        assert!(!is_semantic_name("DIMENSIONAL_EXPONENTS"));
        assert!(!is_semantic_name("SHAPE_ASPECT_RELATIONSHIP"));
        assert!(!is_semantic_name("LENGTH_MEASURE_WITH_UNIT"));
        assert!(!is_semantic_name("DATUM_FEATURE_CALLOUT"));
        assert!(is_presentation_name("DATUM_FEATURE_CALLOUT"));
        assert!(is_presentation_name("TESSELLATED_ANNOTATION_OCCURRENCE"));
        assert!(is_presentation_name("ANNOTATION_FILL_AREA_OCCURRENCE"));
        assert!(!is_presentation_name("STYLED_ITEM"));
    }
}

//! `LabelSet` document entity, templates, placements and the three W2 label commands
//! (`view.add_label@1`, `view.edit_label@1`, `view.remove_label@1`; spec node
//! `VIZ.OVERLAY.LABEL.MODEL`, REQ-VIZ.OVERLAY.LABEL.MODEL-001 .. -005).
//!
//! # Presentation-only boundary (REQ-001)
//!
//! A [`Label`] holds only presentation state: its source element reference, ordered template
//! fields, pinned flag and per-view placements in view-relative units (`0.0..=1.0`). It holds
//! no numeric measurement value of its source; SCAN holds deviation-label and comparison-point
//! values and GDT holds characteristic and dimension results.
//!
//! # Mutation and replay (REQ-002 .. REQ-005)
//!
//! [`LabelSet`] exposes no direct public mutators for adding, editing or removing labels:
//! state changes go only through [`add_label`], [`edit_label`] and [`remove_label`] (each W2,
//! D0), producing a [`LabelEvent`] that records the post-change RFC 8785 SHA-256 [`LabelSet`]
//! digest, applied through [`LabelSet::apply_event`]. Replaying recorded commands with a
//! `fid_project::ReplaySource` reproduces identical label UUIDv7s and byte-identical digests.

#![deny(clippy::wildcard_enum_match_arm)]

use fid_canon::{digest as canon_digest, to_canonical_bytes, CValue};
use fid_project::container::ID_PATTERN;
use fid_project::{IdError, IdIndex, IdService, ObservationSource, Tree, Uuid};
use fid_registry::{CommandDef, DefError};
use fid_types::text::is_forbidden_char;
use fid_types::{Digest, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Canonical schema version of [`LabelSet`].
pub const LABEL_SET_SCHEMA: u32 = 1;
/// Element type ID for registering `LabelSet` in `CORE.PRJ.TREE.MODEL`.
pub const LABEL_SET_ELEMENT_TYPE: &str = "viz.label_set";

/// Registry command names (major 1).
pub const ADD_LABEL: &str = "view.add_label";
pub const EDIT_LABEL: &str = "view.edit_label";
pub const REMOVE_LABEL: &str = "view.remove_label";

/// Journal event types emitted by the three W2 commands.
pub const LABEL_ADDED: &str = "LabelAdded";
pub const LABEL_EDITED: &str = "LabelEdited";
pub const LABEL_REMOVED: &str = "LabelRemoved";

/// Bounded limits.
pub const MAX_LABELS: usize = 10_000;
pub const MAX_PLACEMENTS_PER_LABEL: usize = 256;
pub const MAX_VIEW_KEY_CHARS: usize = 128;

const LIFECYCLE_SINCE: &str = "2026-11-22";

/// Stable element type IDs that may be targeted by a label (REQ-003).
pub const TYPE_DEVIATION_LABEL: &str = "scan.deviation_label";
pub const TYPE_COMPARISON_POINT: &str = "scan.comparison_point";
pub const TYPE_CHARACTERISTIC: &str = "gdt.characteristic";
pub const TYPE_DIMENSION: &str = "gdt.dimension";

/// Stable ID of a [`LabelSet`] document entity (UUID).
pub type LabelSetId = Uuid;
/// Stable ID of a [`Label`] (UUIDv7).
pub type LabelId = Uuid;
/// Stable reference to a source element in the project tree (UUID).
pub type ElementRef = Uuid;

// ------------------------------------------------------------------ errors

/// Typed errors of `VIZ.OVERLAY.LABEL.MODEL`.
#[derive(Debug, Clone, PartialEq)]
pub enum LabelError {
    /// Source does not exist or is not a deviation label, comparison point, characteristic or
    /// dimension (REQ-003).
    NotLabelable { source: String },
    /// Label ID does not exist in the target `LabelSet`.
    UnknownLabel { label: LabelId },
    /// Template is empty, has duplicate fields or names an unknown field.
    BadTemplate { reason: String },
    /// Manual placement coordinate is non-finite or outside `0.0..=1.0`.
    PlacementOutOfRange { x: f64, y: f64 },
    /// View key is empty, too long or holds forbidden control/bidi characters.
    BadViewKey { reason: String },
    /// Malformed parameter or canonical document.
    Schema { reason: String },
    /// Capacity bound exceeded.
    LimitExceeded { what: &'static str, max: usize },
    /// Attempted automatic layout update on a pinned placement (REQ-004).
    PlacementPinned { label: LabelId, view: String },
    /// Digest mismatch during event replay.
    DigestMismatch { expected: String, actual: String },
    /// ID generation or import error.
    Id(IdError),
}

impl LabelError {
    /// Stable error code matching the spec node's Errors section.
    pub fn code(&self) -> &'static str {
        match self {
            LabelError::NotLabelable { .. } => "NotLabelable",
            LabelError::UnknownLabel { .. } => "UnknownLabel",
            LabelError::BadTemplate { .. } => "BadTemplate",
            LabelError::PlacementOutOfRange { .. } => "PlacementOutOfRange",
            LabelError::BadViewKey { .. } => "BadViewKey",
            LabelError::Schema { .. } => "Schema",
            LabelError::LimitExceeded { .. } => "LimitExceeded",
            LabelError::PlacementPinned { .. } => "PlacementPinned",
            LabelError::DigestMismatch { .. } => "DigestMismatch",
            LabelError::Id(_) => "IdError",
        }
    }
}

impl fmt::Display for LabelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LabelError::NotLabelable { source } => {
                write!(f, "NotLabelable: source {source:?} does not exist or is not a labelable element")
            }
            LabelError::UnknownLabel { label } => write!(f, "UnknownLabel: no label {label}"),
            LabelError::BadTemplate { reason } => write!(f, "BadTemplate: {reason}"),
            LabelError::PlacementOutOfRange { x, y } => {
                write!(f, "PlacementOutOfRange: ({x}, {y}) must be finite in 0.0..=1.0")
            }
            LabelError::BadViewKey { reason } => write!(f, "BadViewKey: {reason}"),
            LabelError::Schema { reason } => write!(f, "Schema: {reason}"),
            LabelError::LimitExceeded { what, max } => write!(f, "LimitExceeded: {what} exceeds {max}"),
            LabelError::PlacementPinned { label, view } => {
                write!(f, "PlacementPinned: label {label} is pinned in view {view:?}")
            }
            LabelError::DigestMismatch { expected, actual } => {
                write!(f, "DigestMismatch: expected {expected}, got {actual}")
            }
            LabelError::Id(e) => write!(f, "IdError: {e}"),
        }
    }
}

impl std::error::Error for LabelError {}

impl From<IdError> for LabelError {
    fn from(e: IdError) -> Self {
        LabelError::Id(e)
    }
}

// ------------------------------------------------------------------ domain enums & value types

/// The four element kinds that a [`Label`] may target (REQ-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LabelableKind {
    DeviationLabel,
    ComparisonPoint,
    Characteristic,
    Dimension,
}

impl LabelableKind {
    /// All four labelable kinds.
    pub const ALL: [LabelableKind; 4] = [
        LabelableKind::DeviationLabel,
        LabelableKind::ComparisonPoint,
        LabelableKind::Characteristic,
        LabelableKind::Dimension,
    ];

    /// Canonical project-tree element type ID.
    pub fn as_str(self) -> &'static str {
        match self {
            LabelableKind::DeviationLabel => TYPE_DEVIATION_LABEL,
            LabelableKind::ComparisonPoint => TYPE_COMPARISON_POINT,
            LabelableKind::Characteristic => TYPE_CHARACTERISTIC,
            LabelableKind::Dimension => TYPE_DIMENSION,
        }
    }

    /// Maps a project-tree element `type_id` to its [`LabelableKind`], or `None` if not labelable.
    pub fn from_element_type(type_id: &str) -> Option<LabelableKind> {
        match type_id {
            TYPE_DEVIATION_LABEL => Some(LabelableKind::DeviationLabel),
            TYPE_COMPARISON_POINT => Some(LabelableKind::ComparisonPoint),
            TYPE_CHARACTERISTIC => Some(LabelableKind::Characteristic),
            TYPE_DIMENSION => Some(LabelableKind::Dimension),
            _ => None,
        }
    }
}

/// One field a label template may display (Scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field {
    Name,
    Nominal,
    Actual,
    Deviation,
    Tolerance,
    Uncertainty,
    Status,
}

impl Field {
    /// All seven template fields in canonical schema order.
    pub const ALL: [Field; 7] = [
        Field::Name,
        Field::Nominal,
        Field::Actual,
        Field::Deviation,
        Field::Tolerance,
        Field::Uncertainty,
        Field::Status,
    ];

    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Field::Name => "name",
            Field::Nominal => "nominal",
            Field::Actual => "actual",
            Field::Deviation => "deviation",
            Field::Tolerance => "tolerance",
            Field::Uncertainty => "uncertainty",
            Field::Status => "status",
        }
    }

    /// Parses one wire field name.
    pub fn from_wire(s: &str) -> Option<Field> {
        match s {
            "name" => Some(Field::Name),
            "nominal" => Some(Field::Nominal),
            "actual" => Some(Field::Actual),
            "deviation" => Some(Field::Deviation),
            "tolerance" => Some(Field::Tolerance),
            "uncertainty" => Some(Field::Uncertainty),
            "status" => Some(Field::Status),
            _ => None,
        }
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Ordered non-empty subset of [`Field`]s without duplicates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template(Vec<Field>);

impl Template {
    /// Validates and constructs an ordered, duplicate-free, non-empty template.
    pub fn new(fields: Vec<Field>) -> Result<Self, LabelError> {
        if fields.is_empty() {
            return Err(LabelError::BadTemplate { reason: "template must contain at least one field".into() });
        }
        let mut seen = BTreeSet::new();
        for &fld in &fields {
            if !seen.insert(fld) {
                return Err(LabelError::BadTemplate { reason: format!("duplicate template field `{}`", fld.as_str()) });
            }
        }
        Ok(Self(fields))
    }

    /// Parses a comma-separated CLI template string such as `"name,deviation,status"`.
    pub fn parse_csv(s: &str) -> Result<Self, LabelError> {
        if s.trim().is_empty() {
            return Err(LabelError::BadTemplate { reason: "template must not be empty".into() });
        }
        let mut fields = Vec::new();
        for part in s.split(',') {
            let trimmed = part.trim();
            let Some(f) = Field::from_wire(trimmed) else {
                return Err(LabelError::BadTemplate { reason: format!("unknown template field {trimmed:?}") });
            };
            fields.push(f);
        }
        Self::new(fields)
    }

    /// The ordered fields.
    pub fn fields(&self) -> &[Field] {
        &self.0
    }

    fn to_value(&self) -> CValue {
        CValue::Arr(self.0.iter().map(|f| CValue::Str(f.as_str().to_string())).collect())
    }

    fn from_value(v: &CValue) -> Result<Self, LabelError> {
        let CValue::Arr(arr) = v else {
            return Err(LabelError::BadTemplate { reason: "template must be an array of field names".into() });
        };
        if arr.len() > Field::ALL.len() {
            return Err(LabelError::BadTemplate { reason: "template has too many fields".into() });
        }
        let mut fields = Vec::with_capacity(arr.len());
        for item in arr {
            let CValue::Str(s) = item else {
                return Err(LabelError::BadTemplate { reason: "template entries must be strings".into() });
            };
            let Some(fld) = Field::from_wire(s) else {
                return Err(LabelError::BadTemplate { reason: format!("unknown template field {s:?}") });
            };
            fields.push(fld);
        }
        Self::new(fields)
    }
}

/// View identifier (viewport ID such as `"main"` or a NamedView UUID).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewKey(String);

impl ViewKey {
    /// Validates a non-empty, bounded view key with no control or forbidden characters.
    pub fn new(raw: impl Into<String>) -> Result<Self, LabelError> {
        let s = raw.into();
        let n = s.chars().count();
        if n == 0 || s.trim().is_empty() {
            return Err(LabelError::BadViewKey { reason: "view key must not be empty".into() });
        }
        if n > MAX_VIEW_KEY_CHARS {
            return Err(LabelError::BadViewKey {
                reason: format!("view key has {n} characters, at most {MAX_VIEW_KEY_CHARS} allowed"),
            });
        }
        if s != s.trim() {
            return Err(LabelError::BadViewKey { reason: "view key must not have leading/trailing whitespace".into() });
        }
        if s.chars().any(is_forbidden_char) {
            return Err(LabelError::BadViewKey { reason: "view key contains forbidden control characters".into() });
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ViewKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn norm_coord(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

/// Placement of a label inside one view, in view-relative units `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    at: (f64, f64),
    pinned: bool,
}

impl Placement {
    /// Validates `(x, y)` in `0.0..=1.0` (finite, `-0.0` normalised to `0.0`).
    pub fn new(x: f64, y: f64, pinned: bool) -> Result<Self, LabelError> {
        if !x.is_finite() || !y.is_finite() || !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
            return Err(LabelError::PlacementOutOfRange { x, y });
        }
        Ok(Self { at: (norm_coord(x), norm_coord(y)), pinned })
    }

    /// View-relative coordinates `(x, y)` in `0.0..=1.0`.
    pub fn at(&self) -> (f64, f64) {
        self.at
    }

    /// Whether this placement is pinned (REQ-004).
    pub fn pinned(&self) -> bool {
        self.pinned
    }

    fn to_value(self) -> CValue {
        obj([
            ("at", CValue::Arr(vec![CValue::Num(self.at.0), CValue::Num(self.at.1)])),
            ("pinned", CValue::Bool(self.pinned)),
        ])
    }

    fn from_value(v: &CValue) -> Result<Self, LabelError> {
        let CValue::Obj(m) = v else {
            return Err(LabelError::Schema { reason: "placement must be an object".into() });
        };
        if m.len() != 2 || !m.contains_key("at") || !m.contains_key("pinned") {
            return Err(LabelError::Schema { reason: "placement must have exactly keys `at`, `pinned`".into() });
        }
        let Some(CValue::Arr(coords)) = m.get("at") else {
            return Err(LabelError::Schema { reason: "placement `at` must be [x, y]".into() });
        };
        if coords.len() != 2 {
            return Err(LabelError::Schema { reason: "placement `at` must have 2 coordinates".into() });
        }
        let (CValue::Num(x), CValue::Num(y)) = (&coords[0], &coords[1]) else {
            return Err(LabelError::Schema { reason: "placement coordinates must be numbers".into() });
        };
        let Some(CValue::Bool(pinned)) = m.get("pinned") else {
            return Err(LabelError::Schema { reason: "placement `pinned` must be a boolean".into() });
        };
        Placement::new(*x, *y, *pinned)
    }
}

/// One presentation label inside a [`LabelSet`] (REQ-001: holds no numeric value of its source).
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    id: LabelId,
    source: ElementRef,
    template: Template,
    pinned: bool,
    placements: BTreeMap<ViewKey, Placement>,
}

impl Label {
    /// Stable UUIDv7 of this label.
    pub fn id(&self) -> LabelId {
        self.id
    }

    /// Source element UUID (a deviation label, comparison point, characteristic or dimension).
    pub fn source(&self) -> ElementRef {
        self.source
    }

    /// Ordered template fields to display.
    pub fn template(&self) -> &Template {
        &self.template
    }

    /// Label-level pinned flag.
    pub fn pinned(&self) -> bool {
        self.pinned
    }

    /// Placements keyed by view key, in deterministic sorted order.
    pub fn placements(&self) -> &BTreeMap<ViewKey, Placement> {
        &self.placements
    }

    /// Placement for `view`, if any.
    pub fn placement(&self, view: &ViewKey) -> Option<&Placement> {
        self.placements.get(view)
    }

    /// Whether `view` is pinned (either on the view placement itself or at label level).
    pub fn is_pinned_in(&self, view: &ViewKey) -> bool {
        self.pinned || self.placements.get(view).is_some_and(Placement::pinned)
    }

    /// Canonical `CValue` representation (sorted keys, no numeric values of the source).
    pub fn to_value(&self) -> CValue {
        let mut pmap = BTreeMap::new();
        for (vk, pl) in &self.placements {
            pmap.insert(vk.as_str().to_string(), pl.to_value());
        }
        obj([
            ("id", text(&self.id.to_string())),
            ("pinned", CValue::Bool(self.pinned)),
            ("placements", CValue::Obj(pmap)),
            ("source", text(&self.source.to_string())),
            ("template", self.template.to_value()),
        ])
    }

    /// Parses a [`Label`] from canonical `CValue`, failing closed.
    pub fn from_value(v: &CValue) -> Result<Self, LabelError> {
        let CValue::Obj(m) = v else {
            return Err(LabelError::Schema { reason: "label must be an object".into() });
        };
        let keys: Vec<&str> = m.keys().map(String::as_str).collect();
        if keys != ["id", "pinned", "placements", "source", "template"] {
            return Err(LabelError::Schema {
                reason: "label must have exactly keys `id`, `pinned`, `placements`, `source`, `template`".into(),
            });
        }
        let id = parse_uuid_field(m.get("id"), "id")?;
        let source = parse_uuid_field(m.get("source"), "source")?;
        let Some(CValue::Bool(pinned)) = m.get("pinned") else {
            return Err(LabelError::Schema { reason: "`pinned` must be a boolean".into() });
        };
        let Some(tmpl_val) = m.get("template") else {
            return Err(LabelError::Schema { reason: "`template` is missing".into() });
        };
        let template = Template::from_value(tmpl_val)?;
        let Some(CValue::Obj(pm)) = m.get("placements") else {
            return Err(LabelError::Schema { reason: "`placements` must be an object".into() });
        };
        if pm.len() > MAX_PLACEMENTS_PER_LABEL {
            return Err(LabelError::LimitExceeded { what: "placements", max: MAX_PLACEMENTS_PER_LABEL });
        }
        let mut placements = BTreeMap::new();
        for (k, pv) in pm {
            let vk = ViewKey::new(k.clone())?;
            let pl = Placement::from_value(pv)?;
            placements.insert(vk, pl);
        }
        Ok(Label { id, source, template, pinned: *pinned, placements })
    }
}

/// The `LabelSet` document entity: holds presentation labels with canonical form and digest.
///
/// Mutated only through [`LabelSet::apply_event`] driven by [`add_label`], [`edit_label`] and
/// [`remove_label`] (REQ-002).
#[derive(Debug, Clone, PartialEq)]
pub struct LabelSet {
    id: LabelSetId,
    labels: Vec<Label>,
}

impl LabelSet {
    /// Creates an empty `LabelSet` with the given stable ID.
    pub fn empty(id: LabelSetId) -> Self {
        Self { id, labels: Vec::new() }
    }

    pub fn id(&self) -> LabelSetId {
        self.id
    }

    pub fn labels(&self) -> &[Label] {
        &self.labels
    }

    pub fn len(&self) -> usize {
        self.labels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    pub fn get(&self, id: LabelId) -> Option<&Label> {
        self.labels.iter().find(|l| l.id == id)
    }

    /// Canonical `CValue` representation: `{"id", "labels", "schema": 1}`.
    pub fn to_value(&self) -> CValue {
        obj([
            ("id", text(&self.id.to_string())),
            ("labels", CValue::Arr(self.labels.iter().map(Label::to_value).collect())),
            ("schema", CValue::Num(f64::from(LABEL_SET_SCHEMA))),
        ])
    }

    /// RFC 8785 canonical JSON bytes.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, LabelError> {
        to_canonical_bytes(&self.to_value())
            .map_err(|e| LabelError::Schema { reason: format!("canonicalise: {}", e.code()) })
    }

    /// SHA-256 of the RFC 8785 canonical JSON bytes.
    pub fn digest(&self) -> Result<Digest, LabelError> {
        let d = canon_digest(&self.to_value())
            .map_err(|e| LabelError::Schema { reason: format!("digest: {}", e.code()) })?;
        Digest::parse(&d.to_string()).map_err(|e| LabelError::Schema { reason: format!("digest format: {e}") })
    }

    /// Parses a [`LabelSet`] from canonical `CValue`, failing closed.
    pub fn from_value(v: &CValue) -> Result<Self, LabelError> {
        let CValue::Obj(m) = v else {
            return Err(LabelError::Schema { reason: "LabelSet must be an object".into() });
        };
        let keys: Vec<&str> = m.keys().map(String::as_str).collect();
        if keys != ["id", "labels", "schema"] {
            return Err(LabelError::Schema {
                reason: "LabelSet must have exactly keys `id`, `labels`, `schema`".into(),
            });
        }
        match m.get("schema") {
            Some(CValue::Num(n)) if *n == f64::from(LABEL_SET_SCHEMA) => {}
            _ => {
                return Err(LabelError::Schema { reason: format!("`schema` must be {LABEL_SET_SCHEMA}") });
            }
        }
        let id = parse_uuid_field(m.get("id"), "id")?;
        let Some(CValue::Arr(arr)) = m.get("labels") else {
            return Err(LabelError::Schema { reason: "`labels` must be an array".into() });
        };
        if arr.len() > MAX_LABELS {
            return Err(LabelError::LimitExceeded { what: "labels", max: MAX_LABELS });
        }
        let mut labels = Vec::with_capacity(arr.len());
        let mut seen = BTreeSet::new();
        for item in arr {
            let lbl = Label::from_value(item)?;
            if !seen.insert(lbl.id) {
                return Err(LabelError::Schema { reason: format!("duplicate label id {}", lbl.id) });
            }
            labels.push(lbl);
        }
        Ok(Self { id, labels })
    }

    /// Updates a placement from automatic layout (`VIZ.OVERLAY.LABEL.LAYOUT`), refusing to
    /// move any pinned placement (REQ-004: a pinned placement changes only through
    /// `view.edit_label@1`).
    pub fn set_unpinned_placement(&mut self, label: LabelId, view: ViewKey, x: f64, y: f64) -> Result<(), LabelError> {
        let Some(lbl) = self.labels.iter_mut().find(|l| l.id == label) else {
            return Err(LabelError::UnknownLabel { label });
        };
        if lbl.is_pinned_in(&view) {
            return Err(LabelError::PlacementPinned { label, view: view.as_str().to_string() });
        }
        let pl = Placement::new(x, y, false)?;
        if !lbl.placements.contains_key(&view) && lbl.placements.len() >= MAX_PLACEMENTS_PER_LABEL {
            return Err(LabelError::LimitExceeded { what: "placements", max: MAX_PLACEMENTS_PER_LABEL });
        }
        lbl.placements.insert(view, pl);
        Ok(())
    }

    /// Applies one committed [`LabelEvent`] produced by [`add_label`], [`edit_label`] or
    /// [`remove_label`], verifying the resulting [`LabelSet::digest`] matches the event's
    /// recorded `label_set_digest` (REQ-002, REQ-005). All-or-nothing: on error `self` is unchanged.
    pub fn apply_event(&mut self, event: &LabelEvent) -> Result<(), LabelError> {
        if event.label_set_id != self.id {
            return Err(LabelError::Schema {
                reason: format!("event targets LabelSet {}, expected {}", event.label_set_id, self.id),
            });
        }
        let mut next = self.clone();
        match &event.kind {
            LabelEventKind::Added { label } => {
                if next.labels.len() >= MAX_LABELS {
                    return Err(LabelError::LimitExceeded { what: "labels", max: MAX_LABELS });
                }
                if next.labels.iter().any(|l| l.id == label.id) {
                    return Err(LabelError::Id(IdError::IdCollision(label.id)));
                }
                next.labels.push(label.clone());
            }
            LabelEventKind::Edited { label, template, pinned, view, at } => {
                let Some(existing) = next.labels.iter_mut().find(|l| l.id == *label) else {
                    return Err(LabelError::UnknownLabel { label: *label });
                };
                if let Some(t) = template {
                    existing.template = t.clone();
                }
                match (view, at, pinned) {
                    (Some(vk), Some((x, y)), _) => {
                        let pl = Placement::new(*x, *y, true)?;
                        if !existing.placements.contains_key(vk)
                            && existing.placements.len() >= MAX_PLACEMENTS_PER_LABEL
                        {
                            return Err(LabelError::LimitExceeded {
                                what: "placements",
                                max: MAX_PLACEMENTS_PER_LABEL,
                            });
                        }
                        existing.placements.insert(vk.clone(), pl);
                    }
                    (Some(vk), None, Some(pin)) => {
                        let Some(pl) = existing.placements.get_mut(vk) else {
                            return Err(LabelError::Schema {
                                reason: format!("label {label} has no placement for view {vk:?}"),
                            });
                        };
                        pl.pinned = *pin;
                    }
                    (None, None, Some(pin)) => {
                        existing.pinned = *pin;
                    }
                    (None, None, None) => {}
                    (Some(_), None, None) | (None, Some(_), _) => {
                        return Err(LabelError::Schema {
                            reason: "`at` requires `view`, and `view` without `at` requires `pinned`".into(),
                        });
                    }
                }
            }
            LabelEventKind::Removed { label } => {
                let before = next.labels.len();
                next.labels.retain(|l| l.id != *label);
                if next.labels.len() == before {
                    return Err(LabelError::UnknownLabel { label: *label });
                }
            }
        }

        let actual = next.digest()?;
        if actual != event.label_set_digest {
            return Err(LabelError::DigestMismatch {
                expected: event.label_set_digest.as_str().to_string(),
                actual: actual.as_str().to_string(),
            });
        }
        *self = next;
        Ok(())
    }
}

impl IdIndex for LabelSet {
    fn contains_id(&self, id: &Uuid) -> bool {
        self.id == *id || self.labels.iter().any(|l| l.id == *id)
    }
}

// ------------------------------------------------------------------ source lookup

/// Reference to a source element in `view.add_label@1`: by UUID or by element name (e.g. `cp-07`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    Id(ElementRef),
    Name(String),
}

impl SourceRef {
    /// Parses a UUID string as `SourceRef::Id`, otherwise treats non-empty text as `SourceRef::Name`.
    pub fn parse(s: &str) -> Result<Self, LabelError> {
        if s.trim().is_empty() {
            return Err(LabelError::NotLabelable { source: s.to_string() });
        }
        if let Ok(id) = Uuid::parse(s) {
            Ok(SourceRef::Id(id))
        } else {
            Ok(SourceRef::Name(s.to_string()))
        }
    }

    pub fn display_text(&self) -> String {
        match self {
            SourceRef::Id(id) => id.to_string(),
            SourceRef::Name(name) => name.clone(),
        }
    }
}

/// Read-only view of project elements used to validate `view.add_label@1` sources (REQ-003).
pub trait SourceCatalog {
    /// Resolves `source` to `(element_id, element_type_id)`. Returns `None` if absent or ambiguous.
    fn resolve(&self, source: &SourceRef) -> Option<(ElementRef, String)>;
}

impl SourceCatalog for Tree {
    fn resolve(&self, source: &SourceRef) -> Option<(ElementRef, String)> {
        match source {
            SourceRef::Id(id) => self.element(id).map(|e| (e.id(), e.type_id().to_string())),
            SourceRef::Name(name) => {
                let mut matches = self.elements().filter(|e| e.name().as_untrusted_str() == name.as_str());
                let first = matches.next()?;
                if matches.next().is_some() {
                    return None;
                }
                Some((first.id(), first.type_id().to_string()))
            }
        }
    }
}

// ------------------------------------------------------------------ commands & events

/// Parameters of `view.add_label@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddLabelParams {
    pub set: Option<LabelSetId>,
    pub source: SourceRef,
    pub template: Template,
}

/// Parameters of `view.edit_label@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct EditLabelParams {
    pub label: LabelId,
    pub template: Option<Template>,
    pub pinned: Option<bool>,
    pub view: Option<ViewKey>,
    pub at: Option<(f64, f64)>,
}

/// Parameters of `view.remove_label@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveLabelParams {
    pub label: LabelId,
}

/// Typed event payload for the three W2 label commands.
#[derive(Debug, Clone, PartialEq)]
pub enum LabelEventKind {
    Added {
        label: Label,
    },
    Edited {
        label: LabelId,
        template: Option<Template>,
        pinned: Option<bool>,
        view: Option<ViewKey>,
        at: Option<(f64, f64)>,
    },
    Removed {
        label: LabelId,
    },
}

/// Journaled event produced by `view.add_label@1`, `view.edit_label@1` or `view.remove_label@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelEvent {
    pub command: &'static str,
    pub major: u32,
    pub side_effect: SideEffect,
    pub event_type: &'static str,
    pub label_set_id: LabelSetId,
    pub label_id: LabelId,
    pub label_set_digest: Digest,
    pub kind: LabelEventKind,
}

impl LabelEvent {
    /// Canonical event body `CValue`, carrying the post-change `label_set_digest`.
    pub fn body(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("label".to_string(), text(&self.label_id.to_string()));
        m.insert("label_set_digest".to_string(), text(self.label_set_digest.as_str()));
        m.insert("set".to_string(), text(&self.label_set_id.to_string()));
        match &self.kind {
            LabelEventKind::Added { label } => {
                m.insert("pinned".to_string(), CValue::Bool(label.pinned));
                m.insert("source".to_string(), text(&label.source.to_string()));
                m.insert("template".to_string(), label.template.to_value());
            }
            LabelEventKind::Edited { template, pinned, view, at, .. } => {
                if let Some(t) = template {
                    m.insert("template".to_string(), t.to_value());
                }
                if let Some(p) = pinned {
                    m.insert("pinned".to_string(), CValue::Bool(*p));
                }
                if let Some(vk) = view {
                    m.insert("view".to_string(), text(vk.as_str()));
                }
                if let Some((x, y)) = at {
                    m.insert("at".to_string(), CValue::Arr(vec![CValue::Num(*x), CValue::Num(*y)]));
                }
            }
            LabelEventKind::Removed { .. } => {}
        }
        CValue::Obj(m)
    }
}

/// Command result returned by `view.add_label@1`, `view.edit_label@1` and `view.remove_label@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelCommandResult {
    pub label: LabelId,
    pub set: LabelSetId,
    pub label_set_digest: Digest,
}

impl LabelCommandResult {
    pub fn to_value(&self) -> CValue {
        obj([
            ("label", text(&self.label.to_string())),
            ("label_set_digest", text(self.label_set_digest.as_str())),
            ("set", text(&self.set.to_string())),
        ])
    }
}

/// Combined event + result returned on a successful W2 label command.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelCommandOutput {
    pub event: LabelEvent,
    pub result: LabelCommandResult,
}

/// Plans `view.add_label@1` (W2, D0): validates source is one of the four labelable kinds
/// (else `NotLabelable` and nothing journaled), allocates a UUIDv7 through `source`, computes
/// the post-change [`LabelSet`] digest, and returns the event and result.
pub fn add_label(
    label_set: &LabelSet,
    catalog: &dyn SourceCatalog,
    params: &AddLabelParams,
    namespace: Uuid,
    obs: &mut dyn ObservationSource,
) -> Result<LabelCommandOutput, LabelError> {
    if let Some(target_set) = params.set {
        if target_set != label_set.id {
            return Err(LabelError::Schema {
                reason: format!("set {target_set} does not match target LabelSet {}", label_set.id),
            });
        }
    }
    if label_set.labels.len() >= MAX_LABELS {
        return Err(LabelError::LimitExceeded { what: "labels", max: MAX_LABELS });
    }
    let Some((element_id, type_id)) = catalog.resolve(&params.source) else {
        return Err(LabelError::NotLabelable { source: params.source.display_text() });
    };
    if LabelableKind::from_element_type(&type_id).is_none() {
        return Err(LabelError::NotLabelable { source: params.source.display_text() });
    }

    let mut ids = IdService::new(namespace, obs, label_set);
    let label_id = ids.new_v7()?;
    let label = Label {
        id: label_id,
        source: element_id,
        template: params.template.clone(),
        pinned: false,
        placements: BTreeMap::new(),
    };

    let mut preview = label_set.clone();
    preview.labels.push(label.clone());
    let digest = preview.digest()?;

    let event = LabelEvent {
        command: ADD_LABEL,
        major: 1,
        side_effect: SideEffect::W2,
        event_type: LABEL_ADDED,
        label_set_id: label_set.id,
        label_id,
        label_set_digest: digest,
        kind: LabelEventKind::Added { label },
    };
    let result = LabelCommandResult { label: label_id, set: label_set.id, label_set_digest: digest };
    Ok(LabelCommandOutput { event, result })
}

/// Plans `view.edit_label@1` (W2, D0): edits template, label-level pinned flag, or per-view
/// manual placement (which always sets `pinned = true` for that view key, REQ-004).
pub fn edit_label(label_set: &LabelSet, params: &EditLabelParams) -> Result<LabelCommandOutput, LabelError> {
    if params.template.is_none() && params.pinned.is_none() && params.at.is_none() {
        return Err(LabelError::Schema {
            reason: "view.edit_label@1 requires at least one of `template`, `pinned`, or `at`".into(),
        });
    }
    if params.at.is_some() && params.view.is_none() {
        return Err(LabelError::Schema { reason: "`at` requires `view`".into() });
    }
    if params.at.is_some() && params.pinned == Some(false) {
        return Err(LabelError::Schema {
            reason: "manual placement always pins the view key (`pinned` cannot be false with `at`)".into(),
        });
    }
    if params.view.is_some() && params.at.is_none() && params.pinned.is_none() {
        return Err(LabelError::Schema { reason: "`view` requires `at` or `pinned`".into() });
    }

    let normalized_at = match params.at {
        Some((x, y)) => {
            let pl = Placement::new(x, y, true)?;
            Some(pl.at())
        }
        None => None,
    };
    let effective_pinned = if normalized_at.is_some() { Some(true) } else { params.pinned };

    let mut preview = label_set.clone();
    let Some(lbl) = preview.labels.iter_mut().find(|l| l.id == params.label) else {
        return Err(LabelError::UnknownLabel { label: params.label });
    };
    if let Some(ref t) = params.template {
        lbl.template = t.clone();
    }
    match (&params.view, normalized_at, effective_pinned) {
        (Some(vk), Some((x, y)), _) => {
            let pl = Placement::new(x, y, true)?;
            if !lbl.placements.contains_key(vk) && lbl.placements.len() >= MAX_PLACEMENTS_PER_LABEL {
                return Err(LabelError::LimitExceeded { what: "placements", max: MAX_PLACEMENTS_PER_LABEL });
            }
            lbl.placements.insert(vk.clone(), pl);
        }
        (Some(vk), None, Some(pin)) => {
            let Some(pl) = lbl.placements.get_mut(vk) else {
                return Err(LabelError::Schema {
                    reason: format!("label {} has no placement for view {:?}", params.label, vk.as_str()),
                });
            };
            pl.pinned = pin;
        }
        (None, None, Some(pin)) => {
            lbl.pinned = pin;
        }
        (None, None, None) => {}
        (Some(_), None, None) | (None, Some(_), _) => {
            return Err(LabelError::Schema { reason: "invalid edit_label parameter combination".into() });
        }
    }

    let digest = preview.digest()?;
    let event = LabelEvent {
        command: EDIT_LABEL,
        major: 1,
        side_effect: SideEffect::W2,
        event_type: LABEL_EDITED,
        label_set_id: label_set.id,
        label_id: params.label,
        label_set_digest: digest,
        kind: LabelEventKind::Edited {
            label: params.label,
            template: params.template.clone(),
            pinned: effective_pinned,
            view: params.view.clone(),
            at: normalized_at,
        },
    };
    let result = LabelCommandResult { label: params.label, set: label_set.id, label_set_digest: digest };
    Ok(LabelCommandOutput { event, result })
}

/// Plans `view.remove_label@1` (W2, D0): removes `params.label` and journals the new digest.
pub fn remove_label(label_set: &LabelSet, params: &RemoveLabelParams) -> Result<LabelCommandOutput, LabelError> {
    let mut preview = label_set.clone();
    let before = preview.labels.len();
    preview.labels.retain(|l| l.id != params.label);
    if preview.labels.len() == before {
        return Err(LabelError::UnknownLabel { label: params.label });
    }
    let digest = preview.digest()?;
    let event = LabelEvent {
        command: REMOVE_LABEL,
        major: 1,
        side_effect: SideEffect::W2,
        event_type: LABEL_REMOVED,
        label_set_id: label_set.id,
        label_id: params.label,
        label_set_digest: digest,
        kind: LabelEventKind::Removed { label: params.label },
    };
    let result = LabelCommandResult { label: params.label, set: label_set.id, label_set_digest: digest };
    Ok(LabelCommandOutput { event, result })
}

// ------------------------------------------------------------------ registry definitions

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn id_schema(description: &str) -> CValue {
    obj([("type", text("string")), ("pattern", text(ID_PATTERN)), ("description", text(description))])
}

fn digest_schema(description: &str) -> CValue {
    obj([("type", text("string")), ("pattern", text("^sha256:[0-9a-f]{64}$")), ("description", text(description))])
}

fn object_schema(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

fn parse_uuid_field(v: Option<&CValue>, key: &str) -> Result<Uuid, LabelError> {
    let Some(CValue::Str(s)) = v else {
        return Err(LabelError::Schema { reason: format!("`{key}` must be a UUID string") });
    };
    Uuid::parse(s).map_err(|_| LabelError::Schema { reason: format!("`{key}` is not a valid UUID") })
}

/// Declares the three W2 registry commands `view.add_label@1`, `view.edit_label@1` and
/// `view.remove_label@1` (CLI `fid view add-label|edit-label|remove-label`, MCP
/// `view_add_label|view_edit_label|view_remove_label`).
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let field_enum = strs(&["name", "nominal", "actual", "deviation", "tolerance", "uncertainty", "status"]);
    let template_schema = obj([
        ("type", text("array")),
        ("minItems", CValue::Num(1.0)),
        ("maxItems", CValue::Num(7.0)),
        ("uniqueItems", CValue::Bool(true)),
        ("items", obj([("type", text("string")), ("enum", field_enum)])),
        ("description", text("Ordered subset of label template fields to display.")),
    ]);
    let at_schema = obj([
        ("type", text("array")),
        ("minItems", CValue::Num(2.0)),
        ("maxItems", CValue::Num(2.0)),
        (
            "items",
            obj([
                ("type", text("number")),
                ("x-quantity", text("dimensionless")),
                ("minimum", CValue::Num(0.0)),
                ("maximum", CValue::Num(1.0)),
            ]),
        ),
        ("description", text("Manual placement [x, y] in 0..1 view-relative units; pins the view key.")),
    ]);
    let result_schema = object_schema(
        &["label", "label_set_digest", "set"],
        obj([
            ("label", id_schema("The label UUIDv7.")),
            ("label_set_digest", digest_schema("RFC 8785 SHA-256 digest of the LabelSet after the change.")),
            ("set", id_schema("The LabelSet UUID.")),
        ]),
    );

    let add_params = object_schema(
        &["source", "template"],
        obj([
            ("set", id_schema("Optional target LabelSet UUID; defaults to the active LabelSet.")),
            (
                "source",
                obj([
                    ("type", text("string")),
                    ("x-untrusted", CValue::Bool(true)),
                    (
                        "description",
                        text("Source element UUID or name (deviation label, comparison point, characteristic or dimension)."),
                    ),
                ]),
            ),
            ("template", template_schema.clone()),
        ]),
    );

    let edit_params = object_schema(
        &["label"],
        obj([
            ("at", at_schema),
            ("label", id_schema("Label UUIDv7 to edit.")),
            ("pinned", obj([("type", text("boolean")), ("description", text("Pinned state."))])),
            ("template", template_schema),
            (
                "view",
                obj([
                    ("type", text("string")),
                    ("x-untrusted", CValue::Bool(true)),
                    ("description", text("View key (viewport ID or NamedView ID) for placement or pin.")),
                ]),
            ),
        ]),
    );

    let remove_params = object_schema(&["label"], obj([("label", id_schema("Label UUIDv7 to remove."))]));

    let specs = [
        (
            ADD_LABEL,
            "Add label",
            "Adds a presentation label targeting a deviation label, comparison point, characteristic or dimension; fails with NotLabelable otherwise.",
            add_params,
            LABEL_ADDED,
        ),
        (
            EDIT_LABEL,
            "Edit label",
            "Edits a label's template, pinned flag, or manual placement in view-relative 0..1 units (manual placement pins that view key).",
            edit_params,
            LABEL_EDITED,
        ),
        (
            REMOVE_LABEL,
            "Remove label",
            "Removes a label from the LabelSet and records the updated LabelSet digest.",
            remove_params,
            LABEL_REMOVED,
        ),
    ];

    specs
        .into_iter()
        .map(|(id, title, description, params, effect)| {
            CommandDef::from_value(obj([
                ("id", text(id)),
                ("major", CValue::Num(1.0)),
                ("title", text(title)),
                ("description", text(description)),
                ("params_schema", params),
                ("result_schema", result_schema.clone()),
                ("side_effect", text("W2")),
                ("mcp_projection", text("exposed")),
                ("determinism", text("D0")),
                ("idempotency", text("none")),
                ("fs_effect", text("none")),
                ("undo", text("head_move")),
                ("preconditions", CValue::Arr(Vec::new())),
                ("effects", strs(&[effect])),
                ("algorithms", CValue::Arr(Vec::new())),
                ("exposure", strs(&["cli", "mcp"])),
                ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
                ("human_only", CValue::Bool(false)),
                ("network", text("none")),
                ("web", text("available")),
                ("cli_bare", CValue::Bool(false)),
            ]))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_validation() {
        let t = Template::parse_csv("name,deviation,status");
        assert!(t.is_ok());
        assert!(matches!(Template::parse_csv(""), Err(LabelError::BadTemplate { .. })));
        assert!(matches!(Template::parse_csv("name,name"), Err(LabelError::BadTemplate { .. })));
        assert!(matches!(Template::parse_csv("name,bogus"), Err(LabelError::BadTemplate { .. })));
    }

    #[test]
    fn placement_range_and_neg_zero() {
        assert!(Placement::new(0.0, 1.0, true).is_ok());
        assert!(matches!(Placement::new(-0.01, 0.5, true), Err(LabelError::PlacementOutOfRange { .. })));
        assert!(matches!(Placement::new(0.5, 1.01, true), Err(LabelError::PlacementOutOfRange { .. })));
        assert!(matches!(Placement::new(f64::NAN, 0.5, true), Err(LabelError::PlacementOutOfRange { .. })));
        assert!(matches!(Placement::new(f64::INFINITY, 0.5, true), Err(LabelError::PlacementOutOfRange { .. })));
        let p = Placement::new(-0.0, 0.5, false);
        assert!(p.is_ok());
        if let Ok(pl) = p {
            assert_eq!(pl.at().0.to_bits(), 0.0_f64.to_bits());
        }
    }
}

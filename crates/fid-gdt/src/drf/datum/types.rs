//! The value types of a datum declaration. Every field is private; every constructor checks
//! the same rules the project loader and the event reader apply.

use super::{echo, DatumError};
use crate::chars::dprof::CharId;
use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;

/// Longest datum label, in characters.
pub const MAX_LABEL_LEN: usize = 3;
/// Most nominal faces one datum may name.
pub const MAX_DATUM_FACES: usize = 256;
/// Most datum targets one datum may carry.
pub const MAX_TARGETS: usize = 64;
/// Largest datum-target number (the `1` of `A1`).
pub const MAX_TARGET_NUMBER: u32 = 99;
/// Longest label pattern, in bytes.
pub const MAX_PATTERN_LEN: usize = 64;

// ------------------------------------------------------------------ IDs

/// A reference to a measured feature, a nominal CAD face or a target geometry: 1 to 128
/// bytes, first byte `[A-Za-z0-9]`, the rest `[A-Za-z0-9._:-]` (the ID grammar of
/// `GDT.CHAR`; for example `f1`, `face:12`, `pt:3`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RefId(String);

impl RefId {
    /// Parses a reference ID.
    pub fn parse(s: &str) -> Option<RefId> {
        CharId::parse(s).map(|_| RefId(s.to_owned()))
    }

    /// The ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RefId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ------------------------------------------------------------------ labels

/// A datum label: one to three ASCII capital letters (`A`, `B`, `AA`). A label is only
/// accepted by a command once it also matches the active [`LabelPattern`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DatumLabel(String);

impl DatumLabel {
    /// Checks the base rule every dialect shares: `bad_label{empty}`, `bad_label{too_long}`
    /// (more than three characters, counted as Unicode scalar values), and
    /// `bad_label{not_ascii_capital}` for anything but `A`..`Z` (lower case, digits, and
    /// non-ASCII look-alikes such as full-width or Cyrillic capitals).
    pub fn parse(s: &str) -> Result<DatumLabel, DatumError> {
        if s.is_empty() {
            return Err(DatumError::BadLabel { reason: "empty" });
        }
        if s.chars().count() > MAX_LABEL_LEN {
            return Err(DatumError::BadLabel { reason: "too_long" });
        }
        if !s.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(DatumError::BadLabel { reason: "not_ascii_capital" });
        }
        Ok(DatumLabel(s.to_owned()))
    }

    /// The label text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DatumLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The label pattern a defaults profile sets under `gdt.datum.label_pattern`. Grammar (a
/// small, closed subset of a regular expression, so no regex engine is needed):
///
/// ```text
/// pattern = "[" item+ "]" "{" n "}"  |  "[" item+ "]" "{" n "," n "}"
/// item    = CAP | CAP "-" CAP         (CAP is A..Z; a range runs low to high)
/// n       = "1" | "2" | "3"           (minimum not above maximum)
/// ```
///
/// A label matches when its length lies in the bounds and every letter is in the class.
/// Example: `[A-HJ-NPR-Z]{1,2}` (a synthetic pattern, not taken from any standard).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelPattern {
    text: String,
    allowed: [bool; 26],
    min: usize,
    max: usize,
}

fn pat_bad(reason: &'static str) -> DatumError {
    DatumError::LabelPatternInvalid { reason }
}

fn bound(b: u8) -> Option<usize> {
    match b {
        b'1'..=b'3' => Some(usize::from(b - b'0')),
        _ => None,
    }
}

impl LabelPattern {
    /// Parses a pattern (`label_pattern_invalid{reason}`).
    pub fn parse(s: &str) -> Result<LabelPattern, DatumError> {
        if s.len() > MAX_PATTERN_LEN {
            return Err(pat_bad("too_long"));
        }
        let b = s.as_bytes();
        let close = b.iter().position(|&c| c == b']').ok_or_else(|| pat_bad("no_class"))?;
        if b.first() != Some(&b'[') || close < 2 {
            return Err(pat_bad("no_class"));
        }
        let mut allowed = [false; 26];
        let class = &b[1..close];
        let mut i = 0;
        while i < class.len() {
            let lo = class[i];
            if !lo.is_ascii_uppercase() {
                return Err(pat_bad("bad_class"));
            }
            let hi = if class.get(i + 1) == Some(&b'-') {
                let hi = *class.get(i + 2).ok_or_else(|| pat_bad("bad_range"))?;
                if !hi.is_ascii_uppercase() || hi < lo {
                    return Err(pat_bad("bad_range"));
                }
                i += 3;
                hi
            } else {
                i += 1;
                lo
            };
            for c in lo..=hi {
                allowed[usize::from(c - b'A')] = true;
            }
        }
        let (min, max) = match &b[close + 1..] {
            [b'{', n, b'}'] => {
                let n = bound(*n).ok_or_else(|| pat_bad("bad_bounds"))?;
                (n, n)
            }
            [b'{', lo, b',', hi, b'}'] => {
                let lo = bound(*lo).ok_or_else(|| pat_bad("bad_bounds"))?;
                let hi = bound(*hi).ok_or_else(|| pat_bad("bad_bounds"))?;
                if lo > hi {
                    return Err(pat_bad("bad_bounds"));
                }
                (lo, hi)
            }
            _ => return Err(pat_bad("bad_bounds")),
        };
        Ok(LabelPattern { text: s.to_owned(), allowed, min, max })
    }

    /// The pattern text as authored.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether `label` matches.
    pub fn matches(&self, label: &DatumLabel) -> bool {
        let b = label.as_str().as_bytes();
        (self.min..=self.max).contains(&b.len())
            && b.iter().all(|&c| c.is_ascii_uppercase() && self.allowed[usize::from(c - b'A')])
    }

    /// Parses a label under the base rule ([`DatumLabel::parse`]) and this pattern
    /// (`bad_label{pattern}`).
    pub fn label(&self, s: &str) -> Result<DatumLabel, DatumError> {
        let l = DatumLabel::parse(s)?;
        if !self.matches(&l) {
            return Err(DatumError::BadLabel { reason: "pattern" });
        }
        Ok(l)
    }
}

// ------------------------------------------------------------------ feature types

/// The feature types a measured feature can have. Only the datum-capable ones
/// ([`FeatureType::is_datum_capable`]) may be declared as datum features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeatureType {
    /// A point.
    Point,
    /// A straight line.
    Line,
    /// A plane.
    Plane,
    /// A circle.
    Circle,
    /// A cylinder.
    Cylinder,
    /// A cone.
    Cone,
    /// A sphere.
    Sphere,
    /// A torus.
    Torus,
    /// A width: two opposed parallel planes.
    Width,
    /// A general (free-form) surface.
    Surface,
}

/// Every feature type with its wire name, in wire-name order.
const FEATURE_TYPES: [(FeatureType, &str); 10] = [
    (FeatureType::Circle, "circle"),
    (FeatureType::Cone, "cone"),
    (FeatureType::Cylinder, "cylinder"),
    (FeatureType::Line, "line"),
    (FeatureType::Plane, "plane"),
    (FeatureType::Point, "point"),
    (FeatureType::Sphere, "sphere"),
    (FeatureType::Surface, "surface"),
    (FeatureType::Torus, "torus"),
    (FeatureType::Width, "width"),
];

/// The datum-capable feature types: those a DRF can be built from in phase P1a (planar and
/// cylindrical datum features, `GDT.DRF` scope). The DMT schema (`GDT.RES.SCHEMA`) does not
/// yet list datum-capable types; when it does, this list must come from it.
pub const DATUM_CAPABLE: [FeatureType; 2] = [FeatureType::Plane, FeatureType::Cylinder];

impl FeatureType {
    /// Wire name (lower case).
    pub fn as_str(self) -> &'static str {
        FEATURE_TYPES.iter().find(|(t, _)| *t == self).map_or("", |(_, n)| n)
    }

    /// Parses a wire name (exact, lower case).
    pub fn parse(s: &str) -> Option<FeatureType> {
        FEATURE_TYPES.iter().find(|(_, n)| *n == s).map(|(t, _)| *t)
    }

    /// Whether a datum may be declared on a feature of this type ([`DATUM_CAPABLE`]).
    pub fn is_datum_capable(self) -> bool {
        DATUM_CAPABLE.contains(&self)
    }

    /// Parses a datum feature type: unknown or not datum-capable is
    /// `unsupported_datum_feature_type{type}`.
    pub fn parse_datum(s: &str) -> Result<FeatureType, DatumError> {
        match FeatureType::parse(s) {
            Some(t) if t.is_datum_capable() => Ok(t),
            _ => Err(DatumError::UnsupportedDatumFeatureType { feature_type: echo(s) }),
        }
    }
}

// ------------------------------------------------------------------ datum targets

/// The kind of a datum target: a point, a line or an area on the datum feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TargetKind {
    /// A point target.
    Point,
    /// A line target.
    Line,
    /// An area target.
    Area,
}

impl TargetKind {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            TargetKind::Point => "point",
            TargetKind::Line => "line",
            TargetKind::Area => "area",
        }
    }

    /// Parses a wire name.
    pub fn parse(s: &str) -> Option<TargetKind> {
        match s {
            "point" => Some(TargetKind::Point),
            "line" => Some(TargetKind::Line),
            "area" => Some(TargetKind::Area),
            _ => None,
        }
    }

    /// Whether a target of this kind can lie on a datum feature of type `t`. A target is a
    /// portion of the feature's surface, so only surface types carry targets; a straight line
    /// target cannot lie on a doubly curved surface (sphere, torus, free-form). For the
    /// P1a datum-capable types (plane, cylinder) every kind is allowed.
    pub fn allowed_on(self, t: FeatureType) -> bool {
        match t {
            FeatureType::Plane | FeatureType::Cylinder | FeatureType::Cone | FeatureType::Width => true,
            FeatureType::Sphere | FeatureType::Torus | FeatureType::Surface => self != TargetKind::Line,
            FeatureType::Point | FeatureType::Line | FeatureType::Circle => false,
        }
    }
}

/// One datum target, as a geometry reference only: its ID (the datum label followed by a
/// number 1..99 without leading zero, such as `A1`), its kind, and the plan geometry it
/// refers to. The geometry itself is never copied here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumTargetRef {
    id: String,
    kind: TargetKind,
    geometry: RefId,
}

impl DatumTargetRef {
    /// Builds a target reference, checking its parts alone: `datum_target_invalid{target}`
    /// with reason `bad_id` (not 1..3 capitals then 1..99 without leading zero) or
    /// `bad_kind`, and `invalid_field{targets:bad_geometry_id}`. That the ID starts with
    /// the datum's own label is checked by [`DatumDefinition::new`].
    pub fn new(id: &str, kind: &str, geometry: &str) -> Result<DatumTargetRef, DatumError> {
        if target_number(id).is_none() {
            return Err(DatumError::target(id, "bad_id"));
        }
        let kind = TargetKind::parse(kind).ok_or_else(|| DatumError::target(id, "bad_kind"))?;
        let geometry = RefId::parse(geometry).ok_or_else(|| DatumError::invalid("targets", "bad_geometry_id"))?;
        Ok(DatumTargetRef { id: id.to_owned(), kind, geometry })
    }

    /// The target ID (`A1`).
    pub fn id(&self) -> &str {
        &self.id
    }
    /// The kind.
    pub fn kind(&self) -> TargetKind {
        self.kind
    }
    /// The referenced geometry.
    pub fn geometry(&self) -> &RefId {
        &self.geometry
    }
    /// The label part of the ID.
    fn label_part(&self) -> &str {
        let n = self.id.bytes().take_while(u8::is_ascii_uppercase).count();
        &self.id[..n]
    }

    fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("id".to_owned(), CValue::Str(self.id.clone()));
        m.insert("kind".to_owned(), CValue::Str(self.kind.as_str().to_owned()));
        m.insert("geometry".to_owned(), CValue::Str(self.geometry.as_str().to_owned()));
        CValue::Obj(m)
    }
}

/// Splits `A12` into its number when the ID is 1..3 capitals then 1..99 without leading
/// zero.
fn target_number(id: &str) -> Option<u32> {
    let b = id.as_bytes();
    let n = b.iter().take_while(|c| c.is_ascii_uppercase()).count();
    let digits = &b[n..];
    if n == 0 || n > MAX_LABEL_LEN || digits.is_empty() || digits.len() > 2 || digits[0] == b'0' {
        return None;
    }
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let v = digits.iter().fold(0u32, |a, d| a * 10 + u32::from(d - b'0'));
    (1..=MAX_TARGET_NUMBER).contains(&v).then_some(v)
}

// ------------------------------------------------------------------ definition

/// A declared datum (REQ-GDT.DRF.DATUM-001): label, bound measured feature, the nominal CAD
/// faces of that feature it covers, the feature type, and the datum targets. Faces are
/// held ascending and targets ascending by ID, so the canonical form does not depend on the
/// order given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumDefinition {
    label: DatumLabel,
    feature_ref: RefId,
    nominal_faces: Vec<RefId>,
    feature_type: FeatureType,
    targets: Vec<DatumTargetRef>,
}

impl DatumDefinition {
    /// Builds a definition, checking the rules that need no plan: the type is datum-capable
    /// (`unsupported_datum_feature_type`); one to [`MAX_DATUM_FACES`] faces
    /// (`invalid_field{faces:none|too_many|duplicate}`); at most [`MAX_TARGETS`] targets
    /// (`invalid_field{targets:too_many}`); each target's ID starts with this label
    /// (`datum_target_invalid{target:wrong_label}`), is unique
    /// (`datum_target_invalid{target:duplicate_id}`), refers to a geometry no other target
    /// uses (`datum_target_invalid{target:duplicate_geometry}`), and its kind can lie on the
    /// type (`datum_target_invalid{target:kind_not_allowed}`). The plan-relative checks
    /// (references exist, type matches the feature, targets lie on the datum's faces) are in
    /// [`super::DatumModel`].
    pub fn new(
        label: DatumLabel,
        feature_ref: RefId,
        mut nominal_faces: Vec<RefId>,
        feature_type: FeatureType,
        mut targets: Vec<DatumTargetRef>,
    ) -> Result<DatumDefinition, DatumError> {
        if !feature_type.is_datum_capable() {
            return Err(DatumError::UnsupportedDatumFeatureType { feature_type: feature_type.as_str().to_owned() });
        }
        if nominal_faces.is_empty() {
            return Err(DatumError::invalid("faces", "none"));
        }
        if nominal_faces.len() > MAX_DATUM_FACES {
            return Err(DatumError::invalid("faces", "too_many"));
        }
        nominal_faces.sort();
        if nominal_faces.windows(2).any(|w| w[0] == w[1]) {
            return Err(DatumError::invalid("faces", "duplicate"));
        }
        if targets.len() > MAX_TARGETS {
            return Err(DatumError::invalid("targets", "too_many"));
        }
        for t in &targets {
            if t.label_part() != label.as_str() {
                return Err(DatumError::target(&t.id, "wrong_label"));
            }
            if !t.kind.allowed_on(feature_type) {
                return Err(DatumError::target(&t.id, "kind_not_allowed"));
            }
        }
        targets.sort_by(|a, b| a.id.cmp(&b.id));
        if let Some(w) = targets.windows(2).find(|w| w[0].id == w[1].id) {
            return Err(DatumError::target(&w[1].id, "duplicate_id"));
        }
        let mut geoms: Vec<(&RefId, &str)> = targets.iter().map(|t| (&t.geometry, t.id.as_str())).collect();
        geoms.sort();
        if let Some(w) = geoms.windows(2).find(|w| w[0].0 == w[1].0) {
            return Err(DatumError::target(w[1].1, "duplicate_geometry"));
        }
        Ok(DatumDefinition { label, feature_ref, nominal_faces, feature_type, targets })
    }

    /// The label.
    pub fn label(&self) -> &DatumLabel {
        &self.label
    }
    /// The bound measured feature.
    pub fn feature_ref(&self) -> &RefId {
        &self.feature_ref
    }
    /// The nominal CAD faces, ascending.
    pub fn nominal_faces(&self) -> &[RefId] {
        &self.nominal_faces
    }
    /// The feature type.
    pub fn feature_type(&self) -> FeatureType {
        self.feature_type
    }
    /// The datum targets, ascending by ID.
    pub fn targets(&self) -> &[DatumTargetRef] {
        &self.targets
    }

    /// The canonical-JSON value:
    /// `{"faces":[..],"feature":"f1","feature_type":"plane","label":"A","targets":[{"geometry":..,"id":"A1","kind":"point"}]}`.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("label".to_owned(), CValue::Str(self.label.as_str().to_owned()));
        m.insert("feature".to_owned(), CValue::Str(self.feature_ref.as_str().to_owned()));
        m.insert(
            "faces".to_owned(),
            CValue::Arr(self.nominal_faces.iter().map(|f| CValue::Str(f.as_str().to_owned())).collect()),
        );
        m.insert("feature_type".to_owned(), CValue::Str(self.feature_type.as_str().to_owned()));
        m.insert("targets".to_owned(), CValue::Arr(self.targets.iter().map(DatumTargetRef::to_cvalue).collect()));
        CValue::Obj(m)
    }

    /// Reads the value written by [`Self::to_cvalue`] strictly: exactly the five fields, all
    /// strings or arrays of the right shape (else `bad(reason)`), faces and targets already
    /// in canonical order (`bad("not_canonical")`), then [`Self::new`]'s rules.
    pub fn from_cvalue(v: &CValue, bad: fn(&'static str) -> DatumError) -> Result<DatumDefinition, DatumError> {
        const KEYS: [&str; 5] = ["faces", "feature", "feature_type", "label", "targets"];
        let o = match v {
            CValue::Obj(o) if o.len() == KEYS.len() && KEYS.iter().all(|k| o.contains_key(*k)) => o,
            CValue::Obj(_) => return Err(bad("bad_keys")),
            _ => return Err(bad("wrong_type")),
        };
        let s = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Ok(s.as_str()),
            _ => Err(bad("wrong_type")),
        };
        let arr = |k: &str| match o.get(k) {
            Some(CValue::Arr(a)) => Ok(a),
            _ => Err(bad("wrong_type")),
        };
        let label = DatumLabel::parse(s("label")?)?;
        let feature = RefId::parse(s("feature")?).ok_or_else(|| DatumError::invalid("feature", "bad_id"))?;
        let feature_type = FeatureType::parse_datum(s("feature_type")?)?;
        let faces = arr("faces")?
            .iter()
            .map(|f| match f {
                CValue::Str(f) => RefId::parse(f).ok_or_else(|| DatumError::invalid("faces", "bad_id")),
                _ => Err(bad("wrong_type")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut targets = Vec::new();
        for t in arr("targets")? {
            const TKEYS: [&str; 3] = ["geometry", "id", "kind"];
            let to = match t {
                CValue::Obj(to) if to.len() == TKEYS.len() && TKEYS.iter().all(|k| to.contains_key(*k)) => to,
                CValue::Obj(_) => return Err(bad("bad_keys")),
                _ => return Err(bad("wrong_type")),
            };
            let ts = |k: &str| match to.get(k) {
                Some(CValue::Str(s)) => Ok(s.as_str()),
                _ => Err(bad("wrong_type")),
            };
            targets.push(DatumTargetRef::new(ts("id")?, ts("kind")?, ts("geometry")?)?);
        }
        let def = DatumDefinition::new(label, feature, faces.clone(), feature_type, targets.clone())?;
        let same_targets = def.targets.iter().map(|t| &t.id).eq(targets.iter().map(|t| &t.id));
        if def.nominal_faces != faces || !same_targets {
            return Err(bad("not_canonical"));
        }
        Ok(def)
    }
}

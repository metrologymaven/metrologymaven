//! The ViewState model and its canonical form (spec leaf `VIZ.VIEW.STATE`).
//!
//! A [`ViewState`] is everything a viewport shows: the f64 camera (eye, target, up, projection,
//! near and far distances, in metres and radians), the viewport size, clip planes, visibility
//! rules, transparency entries, and references to the active ColorMapSpec, legend layout and
//! LabelSet. Its canonical bytes ([`ViewState::to_canonical_bytes`]) are RFC 8785 JSON written by
//! the journal's canonicaliser (`fid-canon`), and its digest ([`ViewState::digest`]) is the
//! SHA-256 of those bytes. NamedViews store them, reports reference them and replay compares them.
//!
//! # Canonical form (schema 1)
//!
//! ```json
//! {"camera":{"eye_m":[x,y,z],"far_m":f,"near_m":n,
//!            "projection":{"fov_y_rad":a,"kind":"perspective"},   // or {"height_m":h,"kind":"ortho"}
//!            "target_m":[x,y,z],"up":[x,y,z]},
//!  "clips":[{"id":u32,"normal":[x,y,z],"offset_m":d}, ...],          // sorted by id
//!  "colormap":"sha256:..."|null,"labels":"<uuid>"|null,"legend":"sha256:..."|null,
//!  "schema":1,
//!  "transparency":[{"alpha":a,"entity":"<uuid>"}, ...],              // sorted by entity
//!  "viewport":{"height_px":u32,"width_px":u32},
//!  "visibility":[{"entity":"<uuid>","visible":bool}, ...]}           // sorted by entity
//! ```
//!
//! (Shown wrapped; the canonical bytes have no whitespace.) Every key is always present; an
//! absent reference is `null`. Unknown keys are refused.
//!
//! # Numbers
//!
//! - Every f64 is a plain JSON number in the RFC 8785 / ECMAScript `Number::toString` form of its
//!   shortest round-trip digits (`fid_canon::format_number`), so parsing returns the identical bit
//!   pattern. Subnormals are kept exactly (`5e-324`).
//! - `-0.0` is written as `0` and therefore parses as `+0.0`: the two zeros describe the same view
//!   and have the same digest. This is the only value whose bits do not survive a round trip.
//! - Integers (`schema`, clip `id`, viewport size) are JSON numbers without fraction or exponent.
//! - NaN and infinities are refused everywhere ([`ViewStateError::NonFiniteNumber`]).
//!
//! UUIDs are written lowercase; digests are `sha256:` plus 64 lowercase hex digits.

mod error;
mod json;

pub use error::ViewStateError;
pub use json::{parse, parse_canonical, MAX_SEPARATORS};

use fid_canon::CValue;
use fid_types::Digest;
use std::collections::BTreeMap;

/// The schema version this build reads and writes.
pub const SCHEMA_VERSION: u16 = 1;
/// Most clip planes in one ViewState (REQ-VIZ.VIEW.STATE-005).
pub const MAX_CLIP_PLANES: usize = 64;
/// Most entries in each of `visibility` and `transparency` (REQ-VIZ.VIEW.STATE-005).
pub const MAX_ENTRIES: usize = 100_000;
/// Largest accepted serialised ViewState, in bytes. A canonical ViewState at both entry limits
/// is under 15 MiB.
pub const MAX_INPUT_BYTES: usize = 24 * 1024 * 1024;
/// Deepest JSON nesting a ViewState uses (root, `clips`, a clip, its `normal`).
pub const MAX_DEPTH: usize = 4;
/// Largest viewport width or height, in pixels.
pub const MAX_VIEWPORT_PX: u32 = 32_768;
/// A clip-plane normal is unit if `|n·n - 1| <= UNIT_TOLERANCE`.
pub const UNIT_TOLERANCE: f64 = 1e-12;
/// The up vector is parallel to the view direction if, after scaling each by its largest
/// absolute component, `|u×d|² <= PARALLEL_TOLERANCE² · |u|²·|d|²` (sine of the angle at most
/// `PARALLEL_TOLERANCE`).
pub const PARALLEL_TOLERANCE: f64 = 1e-12;

/// Entity and LabelSet IDs are workspace UUIDs.
pub type EntityId = fid_uuid::Uuid;

/// Camera projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// Vertical field of view in radians, `0 < fov_y_rad < π`.
    Perspective { fov_y_rad: f64 },
    /// Height of the view volume in metres, `> 0`.
    Ortho { height_m: f64 },
}

/// Camera pose and projection, in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub eye_m: [f64; 3],
    pub target_m: [f64; 3],
    /// Need not be unit length; must be non-zero and not parallel to `target_m - eye_m`.
    pub up: [f64; 3],
    pub projection: Projection,
    /// Near clipping distance; `> 0` for perspective.
    pub near_m: f64,
    /// Far clipping distance; `> near_m`.
    pub far_m: f64,
}

/// The viewport this state is laid out for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub width_px: u32,
    pub height_px: u32,
}

/// A clip plane `{x : normal·x = offset_m}` with a unit normal. Which side is kept is
/// `VIZ.VIEW.CLIP`'s rule; this model only stores the plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipPlane {
    /// Stable plane ID, unique within the ViewState; the sort key.
    pub id: u32,
    pub normal: [f64; 3],
    pub offset_m: f64,
}

/// Shows or hides one entity. Unique per entity; sorted by entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibilityRule {
    pub entity: EntityId,
    pub visible: bool,
}

/// Opacity of one entity, `0 <= alpha <= 1` (1 is opaque). Unique per entity; sorted by entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransparencyEntry {
    pub entity: EntityId,
    pub alpha: f64,
}

/// A legend layout, referenced by the digest of its canonical form (`VIZ.OVERLAY.LEGEND`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LegendLayoutRef(pub Digest);

/// What one viewport shows. Collections may be held in any order; the canonical form sorts them.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewState {
    pub schema: u16,
    pub camera: Camera,
    pub viewport: Viewport,
    pub clips: Vec<ClipPlane>,
    pub visibility: Vec<VisibilityRule>,
    pub transparency: Vec<TransparencyEntry>,
    /// Digest of the active ColorMapSpec (`VIZ.OVERLAY.CMAP.SPEC`).
    pub colormap: Option<Digest>,
    pub legend: Option<LegendLayoutRef>,
    /// ID of the active LabelSet (`VIZ.OVERLAY.LABEL.MODEL`).
    pub labels: Option<EntityId>,
}

/// What the document holds, for [`ViewState::check_references`]. Implemented by the document
/// model; this crate does not look anything up itself.
pub trait References {
    fn has_entity(&self, id: &EntityId) -> bool;
    fn has_colormap(&self, digest: &Digest) -> bool;
    fn has_legend_layout(&self, layout: &LegendLayoutRef) -> bool;
    fn has_label_set(&self, id: &EntityId) -> bool;
}

fn err_nonfinite(field: String) -> ViewStateError {
    ViewStateError::NonFiniteNumber { field }
}

fn check_finite(v: f64, field: &str) -> Result<(), ViewStateError> {
    if v.is_finite() {
        Ok(())
    } else {
        Err(err_nonfinite(field.to_string()))
    }
}

fn check_vec3(v: &[f64; 3], field: &str) -> Result<(), ViewStateError> {
    for (i, c) in v.iter().enumerate() {
        if !c.is_finite() {
            return Err(err_nonfinite(format!("{field}[{i}]")));
        }
    }
    Ok(())
}

fn abs_max(v: &[f64; 3]) -> f64 {
    let mut m = 0.0_f64;
    for c in v {
        let a = c.abs();
        if a > m {
            m = a;
        }
    }
    m
}

/// `v` divided by its largest absolute component, or `None` for the zero vector.
fn scaled(v: &[f64; 3]) -> Option<[f64; 3]> {
    let m = abs_max(v);
    if m == 0.0 {
        return None;
    }
    Some([v[0] / m, v[1] / m, v[2] / m])
}

fn dot(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: &[f64; 3], b: &[f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn degenerate(field: &str, reason: &'static str) -> ViewStateError {
    ViewStateError::DegenerateCamera { field: field.to_string(), reason }
}

impl Camera {
    fn validate(&self) -> Result<(), ViewStateError> {
        check_vec3(&self.eye_m, "camera.eye_m")?;
        check_vec3(&self.target_m, "camera.target_m")?;
        check_vec3(&self.up, "camera.up")?;
        match self.projection {
            Projection::Perspective { fov_y_rad } => check_finite(fov_y_rad, "camera.projection.fov_y_rad")?,
            Projection::Ortho { height_m } => check_finite(height_m, "camera.projection.height_m")?,
        }
        check_finite(self.near_m, "camera.near_m")?;
        check_finite(self.far_m, "camera.far_m")?;

        let up = scaled(&self.up).ok_or_else(|| degenerate("camera.up", "zero up vector"))?;
        let d = [self.target_m[0] - self.eye_m[0], self.target_m[1] - self.eye_m[1], self.target_m[2] - self.eye_m[2]];
        if !d.iter().all(|c| c.is_finite()) {
            return Err(degenerate("camera.target_m", "view direction overflows"));
        }
        let d = scaled(&d).ok_or_else(|| degenerate("camera.target_m", "eye equals target"))?;
        let c = cross(&up, &d);
        let tol2 = PARALLEL_TOLERANCE * PARALLEL_TOLERANCE;
        if dot(&c, &c) <= tol2 * dot(&up, &up) * dot(&d, &d) {
            return Err(degenerate("camera.up", "up parallel to view direction"));
        }
        match self.projection {
            Projection::Perspective { fov_y_rad } => {
                if fov_y_rad <= 0.0 || fov_y_rad >= std::f64::consts::PI {
                    return Err(degenerate("camera.projection.fov_y_rad", "field of view outside (0, pi)"));
                }
                if self.near_m <= 0.0 {
                    return Err(degenerate("camera.near_m", "perspective near distance not positive"));
                }
            }
            Projection::Ortho { height_m } => {
                if height_m <= 0.0 {
                    return Err(degenerate("camera.projection.height_m", "orthographic height not positive"));
                }
            }
        }
        if self.near_m >= self.far_m {
            return Err(degenerate("camera.far_m", "near not below far"));
        }
        Ok(())
    }
}

impl ClipPlane {
    fn validate(&self, i: usize) -> Result<(), ViewStateError> {
        let field = format!("clips[{i}].normal");
        check_vec3(&self.normal, &field)?;
        check_finite(self.offset_m, &format!("clips[{i}].offset_m"))?;
        if self.normal.iter().all(|c| *c == 0.0) {
            return Err(ViewStateError::DegenerateNormal { field });
        }
        let n2 = dot(&self.normal, &self.normal);
        if (n2 - 1.0).abs() > UNIT_TOLERANCE {
            return Err(ViewStateError::NonUnitNormal { field });
        }
        Ok(())
    }
}

/// Index of the first entry whose key repeats an earlier one (in sorted order), if any.
fn first_duplicate<K: Ord + Copy>(keys: &[K]) -> Option<usize> {
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by_key(|&i| (keys[i], i));
    order.windows(2).find(|w| keys[w[0]] == keys[w[1]]).map(|w| w[1])
}

impl ViewState {
    /// Checks every rule of the model and fails on the first violation, in this order: schema,
    /// camera, viewport, clip planes, visibility, transparency. Does not check references
    /// against a document; see [`ViewState::check_references`].
    pub fn validate(&self) -> Result<(), ViewStateError> {
        if self.schema != SCHEMA_VERSION {
            return Err(ViewStateError::UnsupportedSchema { found: u64::from(self.schema) });
        }
        self.camera.validate()?;
        for (v, name) in
            [(self.viewport.width_px, "viewport.width_px"), (self.viewport.height_px, "viewport.height_px")]
        {
            if v == 0 || v > MAX_VIEWPORT_PX {
                return Err(ViewStateError::OutOfRange {
                    field: name.to_string(),
                    reason: "viewport size outside 1..=32768 px",
                });
            }
        }
        if self.clips.len() > MAX_CLIP_PLANES {
            return Err(ViewStateError::LimitExceeded { field: "clips".to_string(), limit: MAX_CLIP_PLANES });
        }
        if self.visibility.len() > MAX_ENTRIES {
            return Err(ViewStateError::LimitExceeded { field: "visibility".to_string(), limit: MAX_ENTRIES });
        }
        if self.transparency.len() > MAX_ENTRIES {
            return Err(ViewStateError::LimitExceeded { field: "transparency".to_string(), limit: MAX_ENTRIES });
        }
        for (i, c) in self.clips.iter().enumerate() {
            c.validate(i)?;
        }
        let ids: Vec<u32> = self.clips.iter().map(|c| c.id).collect();
        if let Some(i) = first_duplicate(&ids) {
            return Err(ViewStateError::DuplicateKey { field: format!("clips[{i}].id") });
        }
        let ents: Vec<EntityId> = self.visibility.iter().map(|v| v.entity).collect();
        if let Some(i) = first_duplicate(&ents) {
            return Err(ViewStateError::DuplicateKey { field: format!("visibility[{i}].entity") });
        }
        for (i, t) in self.transparency.iter().enumerate() {
            let field = format!("transparency[{i}].alpha");
            check_finite(t.alpha, &field)?;
            if t.alpha < 0.0 || t.alpha > 1.0 {
                return Err(ViewStateError::OutOfRange { field, reason: "alpha outside [0, 1]" });
            }
        }
        let ents: Vec<EntityId> = self.transparency.iter().map(|t| t.entity).collect();
        if let Some(i) = first_duplicate(&ents) {
            return Err(ViewStateError::DuplicateKey { field: format!("transparency[{i}].entity") });
        }
        Ok(())
    }

    /// Sorts the collections into canonical order: clips by ID, visibility and transparency by
    /// entity. Serialisation does not need this (it sorts its own view); it makes two equal
    /// states compare equal with `==`.
    pub fn sort_collections(&mut self) {
        self.clips.sort_by_key(|c| c.id);
        self.visibility.sort_by_key(|v| v.entity);
        self.transparency.sort_by_key(|t| t.entity);
    }

    /// Every reference the document does not hold, as field paths, in this order: colormap,
    /// legend, labels, visibility entries, transparency entries (each in `Vec` order).
    pub fn unknown_references(&self, refs: &dyn References) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(d) = &self.colormap {
            if !refs.has_colormap(d) {
                out.push("colormap".to_string());
            }
        }
        if let Some(l) = &self.legend {
            if !refs.has_legend_layout(l) {
                out.push("legend".to_string());
            }
        }
        if let Some(id) = &self.labels {
            if !refs.has_label_set(id) {
                out.push("labels".to_string());
            }
        }
        for (i, v) in self.visibility.iter().enumerate() {
            if !refs.has_entity(&v.entity) {
                out.push(format!("visibility[{i}].entity"));
            }
        }
        for (i, t) in self.transparency.iter().enumerate() {
            if !refs.has_entity(&t.entity) {
                out.push(format!("transparency[{i}].entity"));
            }
        }
        out
    }

    /// Fails closed with [`ViewStateError::UnknownReference`] naming the first reference (in the
    /// order of [`ViewState::unknown_references`]) the document does not hold.
    pub fn check_references(&self, refs: &dyn References) -> Result<(), ViewStateError> {
        match self.unknown_references(refs).into_iter().next() {
            Some(field) => Err(ViewStateError::UnknownReference { field }),
            None => Ok(()),
        }
    }

    /// The canonical value tree (validated, collections sorted).
    fn to_cvalue(&self) -> Result<CValue, ViewStateError> {
        self.validate()?;
        let num = CValue::Num;
        let vec3 = |v: &[f64; 3]| CValue::Arr(v.iter().map(|c| CValue::Num(*c)).collect());
        let obj =
            |pairs: Vec<(&str, CValue)>| CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect());

        let projection = match self.camera.projection {
            Projection::Perspective { fov_y_rad } => {
                obj(vec![("fov_y_rad", num(fov_y_rad)), ("kind", CValue::Str("perspective".to_string()))])
            }
            Projection::Ortho { height_m } => {
                obj(vec![("height_m", num(height_m)), ("kind", CValue::Str("ortho".to_string()))])
            }
        };
        let c = &self.camera;
        let camera = obj(vec![
            ("eye_m", vec3(&c.eye_m)),
            ("far_m", num(c.far_m)),
            ("near_m", num(c.near_m)),
            ("projection", projection),
            ("target_m", vec3(&c.target_m)),
            ("up", vec3(&c.up)),
        ]);

        let mut clips: Vec<&ClipPlane> = self.clips.iter().collect();
        clips.sort_by_key(|c| c.id);
        let clips = clips
            .into_iter()
            .map(|p| {
                obj(vec![("id", num(f64::from(p.id))), ("normal", vec3(&p.normal)), ("offset_m", num(p.offset_m))])
            })
            .collect();

        let mut vis: Vec<&VisibilityRule> = self.visibility.iter().collect();
        vis.sort_by_key(|v| v.entity);
        let vis = vis
            .into_iter()
            .map(|v| obj(vec![("entity", CValue::Str(v.entity.to_string())), ("visible", CValue::Bool(v.visible))]))
            .collect();

        let mut tr: Vec<&TransparencyEntry> = self.transparency.iter().collect();
        tr.sort_by_key(|t| t.entity);
        let tr = tr
            .into_iter()
            .map(|t| obj(vec![("alpha", num(t.alpha)), ("entity", CValue::Str(t.entity.to_string()))]))
            .collect();

        let opt_str = |s: Option<String>| s.map_or(CValue::Null, CValue::Str);
        let mut root: BTreeMap<String, CValue> = BTreeMap::new();
        root.insert("camera".to_string(), camera);
        root.insert("clips".to_string(), CValue::Arr(clips));
        root.insert("colormap".to_string(), opt_str(self.colormap.map(|d| d.as_str().to_string())));
        root.insert("labels".to_string(), opt_str(self.labels.map(|u| u.to_string())));
        root.insert("legend".to_string(), opt_str(self.legend.map(|l| l.0.as_str().to_string())));
        root.insert("schema".to_string(), num(f64::from(self.schema)));
        root.insert("transparency".to_string(), CValue::Arr(tr));
        root.insert(
            "viewport".to_string(),
            obj(vec![
                ("height_px", num(f64::from(self.viewport.height_px))),
                ("width_px", num(f64::from(self.viewport.width_px))),
            ]),
        );
        root.insert("visibility".to_string(), CValue::Arr(vis));
        Ok(CValue::Obj(root))
    }

    /// The RFC 8785 canonical bytes (REQ-VIZ.VIEW.STATE-002..004). Validates first; produces
    /// nothing on error.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, ViewStateError> {
        let v = self.to_cvalue()?;
        // Validation has refused every non-finite number, the only error the writer has.
        fid_canon::to_canonical_bytes(&v).map_err(|_| err_nonfinite("$".to_string()))
    }

    /// SHA-256 of the canonical bytes, as `sha256:<hex>` (REQ-VIZ.VIEW.STATE-006).
    pub fn digest(&self) -> Result<Digest, ViewStateError> {
        let bytes = self.to_canonical_bytes()?;
        Ok(Digest::from_bytes(fid_canon::digest_bytes(&bytes).0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_duplicate_reports_the_later_index() {
        assert_eq!(first_duplicate::<u32>(&[]), None);
        assert_eq!(first_duplicate(&[3u32, 1, 2]), None);
        assert_eq!(first_duplicate(&[3u32, 1, 3, 1]), Some(3));
        assert_eq!(first_duplicate(&[5u32, 7, 5]), Some(2));
    }

    #[test]
    fn scaled_handles_subnormals_and_zero() {
        assert_eq!(scaled(&[0.0, -0.0, 0.0]), None);
        let tiny = f64::from_bits(1);
        assert_eq!(scaled(&[tiny, 0.0, -tiny]), Some([1.0, 0.0, -1.0]));
    }
}

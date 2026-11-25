//! Index build and neighbour query commands (`GEO.INDEX.CMD`, `spec/geo/index/cmd/NODE.md`).
//!
//! Four R1, D0 commands exposed on CLI and MCP (`mcp_projection: "exposed"`), written only
//! to the Activity Log (never the Record Journal):
//!
//! - `geo.build_index@1` (`fid geo build-index` / `geo_build_index`): builds or warms the
//!   cached spatial index (`point` k-d tree or `primitive` triangle BVH) for a geometric
//!   element under an optional memory budget, returning the build report or `budget_exceeded`
//!   with the needed byte count (REQ-001, REQ-005).
//! - `geo.query_knn@1` (`fid geo query-knn` / `geo_query_knn`): k-nearest neighbours in
//!   metres in `(dist2, index)` order from `GEO.INDEX.KDTREE` (REQ-001..REQ-004).
//! - `geo.query_radius@1` (`fid geo query-radius` / `geo_query_radius`): neighbours within an
//!   authored length converted by `fid-geo-frame::units` (REQ-001..REQ-004).
//! - `geo.query_box@1` (`fid geo query-box` / `geo_query_box`): points inside an axis-aligned
//!   box `[min, max]` in ascending index order (REQ-001..REQ-004).
//!
//! Query points may be given inline (at most 10 000 per call, else `too_many_queries`) or as
//! a selection handle. Frame mismatches fail with `frame_mismatch`. Total returned neighbour
//! entries across all query points are capped at 1 000 000 in query order with
//! `truncated: true`. Nothing in this module panics or uses `unsafe`.

use std::collections::BTreeMap;
use std::sync::Arc;

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_geo_frame::units::{AuthoredQuantity, LengthUnit, Unit};
use fid_geo_index::{
    build_bvh, build_point_tree, estimate_bytes, BudgetError, BuildParams, CachedIndex, IndexCache, IndexKey,
    IndexKind, MemoryBudget, PointTree, MAX_K,
};
use fid_geo_types::{Aabb, FrameRef, GeometryElement, PointBuffer};
use fid_registry::{CommandDef, DefError};
use fid_uuid::Uuid;

use crate::describe::{ActivityEntry, Surface};
use crate::error::{kept, GeoCmdError};
use crate::register::{ElementTree, EntityId, GeoEntry, TreeEntry};

/// Command ID for `geo.build_index@1`.
pub const BUILD_INDEX: &str = "geo.build_index@1";
/// Command ID for `geo.query_knn@1`.
pub const QUERY_KNN: &str = "geo.query_knn@1";
/// Command ID for `geo.query_radius@1`.
pub const QUERY_RADIUS: &str = "geo.query_radius@1";
/// Command ID for `geo.query_box@1`.
pub const QUERY_BOX: &str = "geo.query_box@1";

/// CLI subcommand for `geo.build_index@1`.
pub const CLI_BUILD_INDEX: &str = "build-index";
/// CLI subcommand for `geo.query_knn@1`.
pub const CLI_QUERY_KNN: &str = "query-knn";
/// CLI subcommand for `geo.query_radius@1`.
pub const CLI_QUERY_RADIUS: &str = "query-radius";
/// CLI subcommand for `geo.query_box@1`.
pub const CLI_QUERY_BOX: &str = "query-box";

/// MCP tool name for `geo.build_index@1`.
pub const MCP_BUILD_INDEX: &str = "geo_build_index";
/// MCP tool name for `geo.query_knn@1`.
pub const MCP_QUERY_KNN: &str = "geo_query_knn";
/// MCP tool name for `geo.query_radius@1`.
pub const MCP_QUERY_RADIUS: &str = "geo_query_radius";
/// MCP tool name for `geo.query_box@1`.
pub const MCP_QUERY_BOX: &str = "geo_query_box";

/// Maximum inline query points per call (REQ-GEO.INDEX.CMD-004).
pub const MAX_INLINE_QUERIES: usize = 10_000;
/// Maximum total neighbour entries returned across all query points (REQ-GEO.INDEX.CMD-003).
pub const MAX_RESULT_ENTRIES: usize = 1_000_000;
/// Maximum JSON parameter payload for query calls (covers 10 000 inline 3D points).
pub const MAX_QUERY_PARAMS_BYTES: usize = 4 * 1024 * 1024;
/// Maximum JSON parameter nesting depth for index commands.
pub const MAX_QUERY_PARAMS_DEPTH: usize = 8;
/// Maximum CLI arguments after `fid geo`.
const MAX_CLI_ARGS: usize = 32;
const LIFECYCLE_SINCE: &str = "2026-11-22";
const ID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";

/// Index kind accepted by `geo.build_index@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BuildKind {
    /// Point k-d tree (`point`).
    Point,
    /// Triangle bounding volume hierarchy (`primitive`).
    Primitive,
}

impl BuildKind {
    /// Wire string: `point` or `primitive`.
    pub const fn as_str(self) -> &'static str {
        match self {
            BuildKind::Point => "point",
            BuildKind::Primitive => "primitive",
        }
    }

    /// Parses `point` or `primitive`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "point" => Some(BuildKind::Point),
            "primitive" => Some(BuildKind::Primitive),
            _ => None,
        }
    }
}

/// Points resolved from a selection handle, tagged with their coordinate frame.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionPoints {
    /// Coordinate frame of the selected points.
    pub frame: FrameRef,
    /// Query point coordinates in metres.
    pub points: Vec<[f64; 3]>,
}

/// Read-only provider of selection handles for `points | selection` query inputs.
pub trait SelectionStore {
    /// Returns the points and frame for `id`, or `None` if no such selection exists.
    fn get_selection(&self, id: &EntityId) -> Option<SelectionPoints>;
}

impl SelectionStore for () {
    fn get_selection(&self, _id: &EntityId) -> Option<SelectionPoints> {
        None
    }
}

impl SelectionStore for BTreeMap<EntityId, SelectionPoints> {
    fn get_selection(&self, id: &EntityId) -> Option<SelectionPoints> {
        self.get(id).cloned()
    }
}

/// Query point source for kNN and radius queries: inline coordinates or a selection handle.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryPoints {
    /// Inline query points in metres, with an optional frame tag verified against the element.
    Inline { points: Vec<[f64; 3]>, frame: Option<FrameRef> },
    /// A selection handle resolved via [`SelectionStore`].
    Selection(EntityId),
}

/// An authored non-negative length parsed through `fid-geo-frame::units` (GEO.FRAME.UNITS).
#[derive(Debug, Clone, PartialEq)]
pub struct AuthoredLength {
    /// Authored decimal text.
    pub text: String,
    /// Authored length unit.
    pub unit: LengthUnit,
    /// Single-rounded SI value in metres.
    pub metres: f64,
}

impl AuthoredLength {
    /// Parses a compact authored length string such as `"5mm"`, `"0.01m"`, `"250um"`,
    /// `"0.5in"`, or `"1.5e-2 m"` using `fid-geo-frame::units::AuthoredQuantity::parse`.
    pub fn parse_str(input: &str) -> Result<Self, GeoCmdError> {
        let trimmed = input.trim();
        if trimmed.is_empty() || trimmed.len() > 128 {
            return Err(GeoCmdError::BadLength("empty or overly long authored length".to_string()));
        }
        // Split trailing ASCII alphabetic unit suffix (e.g. "mm", "um", "thou", "in", "ft", "m").
        let bytes = trimmed.as_bytes();
        let mut split = bytes.len();
        while split > 0 && bytes[split - 1].is_ascii_alphabetic() {
            split -= 1;
        }
        let unit_str = trimmed.get(split..).unwrap_or("");
        let num_str = trimmed.get(..split).unwrap_or("").trim();
        if num_str.is_empty() || unit_str.is_empty() {
            return Err(GeoCmdError::BadLength(
                "authored length must include decimal value and unit symbol (e.g. '5mm' or '0.01m')".to_string(),
            ));
        }
        Self::parse_parts(num_str, unit_str)
    }

    /// Parses decimal `text` and length unit symbol `unit_symbol` via `fid-geo-frame::units`.
    pub fn parse_parts(text: &str, unit_symbol: &str) -> Result<Self, GeoCmdError> {
        let unit = match Unit::from_symbol(unit_symbol) {
            Some(Unit::Length(u)) => u,
            _ => return Err(GeoCmdError::BadLength(format!("unknown length unit {}", kept(unit_symbol)))),
        };
        let qty = AuthoredQuantity::parse(text, Unit::Length(unit))
            .map_err(|e| GeoCmdError::BadLength(format!("invalid length: {}", e.code())))?;
        let metres = qty.si();
        if !metres.is_finite() || metres < 0.0 {
            return Err(GeoCmdError::BadLength("length must be finite and non-negative".to_string()));
        }
        Ok(Self { text: qty.text().to_string(), unit, metres })
    }

    /// Parses an authored length from a canonical JSON value: either `"5mm"` or
    /// `{"text": "5", "unit": "mm"}`.
    pub fn from_value(v: &CValue) -> Result<Self, GeoCmdError> {
        match v {
            CValue::Str(s) => Self::parse_str(s),
            CValue::Obj(o) => {
                if o.keys().any(|k| k != "text" && k != "unit") {
                    return Err(GeoCmdError::BadParams("unknown field in authored length"));
                }
                let Some(CValue::Str(t)) = o.get("text") else {
                    return Err(GeoCmdError::BadParams("authored length missing string 'text'"));
                };
                let Some(CValue::Str(u)) = o.get("unit") else {
                    return Err(GeoCmdError::BadParams("authored length missing string 'unit'"));
                };
                Self::parse_parts(t, u)
            }
            _ => Err(GeoCmdError::BadParams("radius must be an authored length string or {text, unit} object")),
        }
    }
}

/// Parameters for `geo.build_index@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildIndexParams {
    pub element: EntityId,
    pub kind: BuildKind,
    pub budget_bytes: Option<u64>,
}

/// Result of `geo.build_index@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildIndexResult {
    pub kind: BuildKind,
    pub bytes: u64,
    pub elapsed_ns: u64,
    pub hit: bool,
}

/// Parameters for `geo.query_knn@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryKnnParams {
    pub element: EntityId,
    pub query: QueryPoints,
    pub k: usize,
}

/// Parameters for `geo.query_radius@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryRadiusParams {
    pub element: EntityId,
    pub query: QueryPoints,
    pub radius: AuthoredLength,
}

/// Parameters for `geo.query_box@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryBoxParams {
    pub element: EntityId,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub frame: Option<FrameRef>,
}

/// One neighbour hit returned by query commands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QueryHit {
    pub index: u64,
    pub distance_m: f64,
}

/// Result of `geo.query_knn@1`, `geo.query_radius@1`, and `geo.query_box@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    pub results: Vec<Vec<QueryHit>>,
    pub truncated: bool,
}

// ------------------------------------------------------------------ parameter parsing

fn parse_id(text: &str) -> Option<Uuid> {
    let id = Uuid::parse(text).ok()?;
    (id.to_string() == text && id != Uuid::NIL && id != Uuid::MAX).then_some(id)
}

fn parse_frame_id(text: &str) -> Option<FrameRef> {
    let id = Uuid::parse(text).ok()?;
    if id.to_string() != text {
        return None;
    }
    Some(FrameRef::from_bytes(*id.as_bytes()))
}

fn parse_u64_field(v: &CValue, msg: &'static str) -> Result<u64, GeoCmdError> {
    let CValue::Num(n) = v else { return Err(GeoCmdError::BadParams(msg)) };
    if !n.is_finite() || *n < 0.0 || n.fract() != 0.0 || *n > (1u64 << 53) as f64 {
        return Err(GeoCmdError::BadParams(msg));
    }
    Ok(*n as u64)
}

fn parse_point3(v: &CValue, msg: &'static str) -> Result<[f64; 3], GeoCmdError> {
    let CValue::Arr(a) = v else { return Err(GeoCmdError::BadParams(msg)) };
    if a.len() != 3 {
        return Err(GeoCmdError::BadParams(msg));
    }
    let mut p = [0.0f64; 3];
    for (slot, item) in p.iter_mut().zip(a.iter()) {
        let CValue::Num(x) = item else { return Err(GeoCmdError::BadParams(msg)) };
        if !x.is_finite() {
            return Err(GeoCmdError::BadParams(msg));
        }
        *slot = *x;
    }
    Ok(p)
}

fn parse_optional_frame(o: &BTreeMap<String, CValue>) -> Result<Option<FrameRef>, GeoCmdError> {
    match o.get("frame") {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => {
            parse_frame_id(s).map(Some).ok_or(GeoCmdError::BadParams("frame is not a canonical uuid"))
        }
        Some(_) => Err(GeoCmdError::BadParams("frame must be a uuid string")),
    }
}

fn parse_query_points(o: &BTreeMap<String, CValue>) -> Result<QueryPoints, GeoCmdError> {
    let has_points = o.get("points").is_some_and(|v| !matches!(v, CValue::Null));
    let has_sel = o.get("selection").is_some_and(|v| !matches!(v, CValue::Null));
    match (has_points, has_sel) {
        (true, true) => Err(GeoCmdError::BadParams("provide either points or selection, not both")),
        (false, false) => Err(GeoCmdError::BadParams("missing points or selection")),
        (true, false) => {
            let Some(CValue::Arr(arr)) = o.get("points") else {
                return Err(GeoCmdError::BadParams("points must be an array of [x, y, z]"));
            };
            if arr.len() > MAX_INLINE_QUERIES {
                return Err(GeoCmdError::TooManyQueries { count: arr.len(), max: MAX_INLINE_QUERIES });
            }
            let mut pts = Vec::with_capacity(arr.len());
            for item in arr {
                pts.push(parse_point3(item, "each query point must be [x, y, z] finite numbers")?);
            }
            let frame = parse_optional_frame(o)?;
            Ok(QueryPoints::Inline { points: pts, frame })
        }
        (false, true) => {
            if o.get("frame").is_some_and(|v| !matches!(v, CValue::Null)) {
                return Err(GeoCmdError::BadParams("frame may not be combined with selection"));
            }
            let Some(CValue::Str(s)) = o.get("selection") else {
                return Err(GeoCmdError::BadParams("selection must be a canonical uuid string"));
            };
            let id = parse_id(s).ok_or(GeoCmdError::BadParams("selection is not a canonical uuid"))?;
            Ok(QueryPoints::Selection(id))
        }
    }
}

impl BuildIndexParams {
    /// Parses `{"element": "<uuid>", "kind": "point"|"primitive", "budget_bytes"?: <int>}`.
    pub fn from_value(v: &CValue) -> Result<Self, GeoCmdError> {
        let CValue::Obj(o) = v else { return Err(GeoCmdError::BadParams("params is not an object")) };
        if o.keys().any(|k| !matches!(k.as_str(), "element" | "kind" | "budget_bytes")) {
            return Err(GeoCmdError::BadParams("unknown parameter"));
        }
        let element = match o.get("element") {
            Some(CValue::Str(s)) => parse_id(s).ok_or(GeoCmdError::BadParams("element is not a canonical id"))?,
            None => return Err(GeoCmdError::BadParams("missing element")),
            Some(_) => return Err(GeoCmdError::BadParams("element is not a string")),
        };
        let kind = match o.get("kind") {
            Some(CValue::Str(s)) => {
                BuildKind::parse(s).ok_or(GeoCmdError::BadParams("kind must be point or primitive"))?
            }
            None => return Err(GeoCmdError::BadParams("missing kind")),
            Some(_) => return Err(GeoCmdError::BadParams("kind is not a string")),
        };
        let budget_bytes = match o.get("budget_bytes") {
            None | Some(CValue::Null) => None,
            Some(val) => Some(parse_u64_field(val, "budget_bytes must be a non-negative integer")?),
        };
        Ok(Self { element, kind, budget_bytes })
    }
}

impl QueryKnnParams {
    /// Parses `{"element": "<uuid>", "k": <1..=1024>, "points"|"selection": ..., "frame"?: "<uuid>"}`.
    pub fn from_value(v: &CValue) -> Result<Self, GeoCmdError> {
        let CValue::Obj(o) = v else { return Err(GeoCmdError::BadParams("params is not an object")) };
        if o.keys().any(|k| !matches!(k.as_str(), "element" | "k" | "points" | "selection" | "frame")) {
            return Err(GeoCmdError::BadParams("unknown parameter"));
        }
        let element = match o.get("element") {
            Some(CValue::Str(s)) => parse_id(s).ok_or(GeoCmdError::BadParams("element is not a canonical id"))?,
            None => return Err(GeoCmdError::BadParams("missing element")),
            Some(_) => return Err(GeoCmdError::BadParams("element is not a string")),
        };
        let k_u64 = match o.get("k") {
            Some(val) => parse_u64_field(val, "k must be an integer in 1..=1024")?,
            None => return Err(GeoCmdError::BadParams("missing k")),
        };
        let k = usize::try_from(k_u64).unwrap_or(usize::MAX);
        if k == 0 || k > MAX_K {
            return Err(GeoCmdError::BadParams("k must be between 1 and 1024"));
        }
        let query = parse_query_points(o)?;
        Ok(Self { element, query, k })
    }
}

impl QueryRadiusParams {
    /// Parses `{"element": "<uuid>", "radius": <AuthoredLength>, "points"|"selection": ..., "frame"?: "<uuid>"}`.
    pub fn from_value(v: &CValue) -> Result<Self, GeoCmdError> {
        let CValue::Obj(o) = v else { return Err(GeoCmdError::BadParams("params is not an object")) };
        if o.keys().any(|k| !matches!(k.as_str(), "element" | "radius" | "points" | "selection" | "frame")) {
            return Err(GeoCmdError::BadParams("unknown parameter"));
        }
        let element = match o.get("element") {
            Some(CValue::Str(s)) => parse_id(s).ok_or(GeoCmdError::BadParams("element is not a canonical id"))?,
            None => return Err(GeoCmdError::BadParams("missing element")),
            Some(_) => return Err(GeoCmdError::BadParams("element is not a string")),
        };
        let radius = match o.get("radius") {
            Some(val) => AuthoredLength::from_value(val)?,
            None => return Err(GeoCmdError::BadParams("missing radius")),
        };
        let query = parse_query_points(o)?;
        Ok(Self { element, query, radius })
    }
}

impl QueryBoxParams {
    /// Parses `{"element": "<uuid>", "min": [x,y,z], "max": [x,y,z], "frame"?: "<uuid>"}`.
    pub fn from_value(v: &CValue) -> Result<Self, GeoCmdError> {
        let CValue::Obj(o) = v else { return Err(GeoCmdError::BadParams("params is not an object")) };
        if o.keys().any(|k| !matches!(k.as_str(), "element" | "min" | "max" | "frame")) {
            return Err(GeoCmdError::BadParams("unknown parameter"));
        }
        let element = match o.get("element") {
            Some(CValue::Str(s)) => parse_id(s).ok_or(GeoCmdError::BadParams("element is not a canonical id"))?,
            None => return Err(GeoCmdError::BadParams("missing element")),
            Some(_) => return Err(GeoCmdError::BadParams("element is not a string")),
        };
        let min = match o.get("min") {
            Some(val) => parse_point3(val, "min must be [x, y, z] finite numbers")?,
            None => return Err(GeoCmdError::BadParams("missing min")),
        };
        let max = match o.get("max") {
            Some(val) => parse_point3(val, "max must be [x, y, z] finite numbers")?,
            None => return Err(GeoCmdError::BadParams("missing max")),
        };
        if min[0] > max[0] || min[1] > max[1] || min[2] > max[2] {
            return Err(GeoCmdError::BadParams("box min must be <= max on every axis"));
        }
        let frame = parse_optional_frame(o)?;
        Ok(Self { element, min, max, frame })
    }
}

// ------------------------------------------------------------------ core execution

fn lookup_geo<'a>(tree: &'a dyn ElementTree, id: &EntityId) -> Result<&'a GeoEntry, GeoCmdError> {
    match tree.get(id) {
        None => Err(GeoCmdError::NotFound(*id)),
        Some(TreeEntry::Other { .. }) => Err(GeoCmdError::NotGeometry(*id)),
        Some(TreeEntry::Geometry(g)) => Ok(g),
    }
}

fn element_points(entry: &GeoEntry) -> Result<&PointBuffer, GeoCmdError> {
    match &entry.element {
        GeometryElement::PointSet(p) => Ok(p.buffer()),
        GeometryElement::PointCloud(p) => Ok(p.buffer()),
        GeometryElement::Mesh(m) => Ok(m.vertices()),
        GeometryElement::CadBody(_) => Err(GeoCmdError::Index("CAD body has no point buffer".to_string())),
    }
}

fn map_budget_error(err: BudgetError) -> GeoCmdError {
    match err {
        BudgetError::BudgetExceeded { needed, budget } => GeoCmdError::BudgetExceeded { needed, budget },
        other => GeoCmdError::Index(other.to_string()),
    }
}

fn get_or_build_point_tree(entry: &GeoEntry, cache: &mut IndexCache) -> Result<Arc<PointTree>, GeoCmdError> {
    let pts = element_points(entry)?;
    let count = u64::try_from(pts.x.len()).unwrap_or(u64::MAX);
    let key = IndexKey::new(entry.facts.digest, IndexKind::KdTree, BuildParams::kdtree());
    let lookup = cache
        .get_or_build(key, count, || {
            let (tree, _) = build_point_tree(pts, BuildParams::kdtree(), MemoryBudget::UNLIMITED)?;
            Ok(CachedIndex::Point(Arc::new(tree)))
        })
        .map_err(map_budget_error)?;
    lookup.index.as_point().cloned().ok_or(GeoCmdError::Internal("cached index kind mismatch"))
}

fn resolve_query_points(
    entry: &GeoEntry,
    selections: &dyn SelectionStore,
    query: &QueryPoints,
) -> Result<Vec<[f64; 3]>, GeoCmdError> {
    let expected = entry.facts.frame;
    match query {
        QueryPoints::Inline { points, frame } => {
            if points.len() > MAX_INLINE_QUERIES {
                return Err(GeoCmdError::TooManyQueries { count: points.len(), max: MAX_INLINE_QUERIES });
            }
            if let Some(found) = frame {
                if *found != expected {
                    return Err(GeoCmdError::FrameMismatch { expected, found: *found });
                }
            }
            Ok(points.clone())
        }
        QueryPoints::Selection(sel_id) => {
            let sel = selections.get_selection(sel_id).ok_or(GeoCmdError::NotFound(*sel_id))?;
            if sel.frame != expected {
                return Err(GeoCmdError::FrameMismatch { expected, found: sel.frame });
            }
            Ok(sel.points)
        }
    }
}

/// Runs `geo.build_index@1`: builds or warms the cached index for `params.element` under the
/// optional memory budget, returning `budget_exceeded` with `needed` bytes before allocating
/// if the pre-allocation estimate exceeds the budget (REQ-GEO.INDEX.CMD-001, -005).
pub fn build_index(
    tree: &dyn ElementTree,
    cache: &mut IndexCache,
    params: &BuildIndexParams,
) -> Result<BuildIndexResult, GeoCmdError> {
    let entry = lookup_geo(tree, &params.element)?;
    let (index_kind, build_params, count) = match params.kind {
        BuildKind::Point => {
            let pts = element_points(entry)?;
            (IndexKind::KdTree, BuildParams::kdtree(), u64::try_from(pts.x.len()).unwrap_or(u64::MAX))
        }
        BuildKind::Primitive => match &entry.element {
            GeometryElement::Mesh(m) => (IndexKind::Bvh, BuildParams::bvh(), m.triangle_count()),
            _ => return Err(GeoCmdError::Index("primitive index requires a Mesh element".to_string())),
        },
    };

    // REQ-005: explicit per-call budget check fails closed before any build allocation.
    if let Some(budget_bytes) = params.budget_bytes {
        let needed = estimate_bytes(index_kind, count, build_params);
        if needed > budget_bytes {
            return Err(GeoCmdError::BudgetExceeded { needed, budget: budget_bytes });
        }
    }

    let key = IndexKey::new(entry.facts.digest, index_kind, build_params);
    let call_budget = params.budget_bytes.map(MemoryBudget::new).unwrap_or(MemoryBudget::UNLIMITED);
    let lookup = cache
        .get_or_build(key, count, || match params.kind {
            BuildKind::Point => {
                let pts = match &entry.element {
                    GeometryElement::PointSet(p) => p.buffer(),
                    GeometryElement::PointCloud(p) => p.buffer(),
                    GeometryElement::Mesh(m) => m.vertices(),
                    GeometryElement::CadBody(_) => {
                        return Err(BudgetError::PointIndex(fid_geo_index::PointIndexError::Inconsistent))
                    }
                };
                let (pt_tree, _) = build_point_tree(pts, build_params, call_budget)?;
                Ok(CachedIndex::Point(Arc::new(pt_tree)))
            }
            BuildKind::Primitive => match &entry.element {
                GeometryElement::Mesh(m) => {
                    let payload: Vec<u64> = (0..m.triangle_count()).collect();
                    let (bvh, _) = build_bvh(m, &payload, build_params, call_budget)?;
                    Ok(CachedIndex::Primitive(Arc::new(bvh)))
                }
                _ => Err(BudgetError::Bvh(fid_geo_index::BvhError::Inconsistent)),
            },
        })
        .map_err(map_budget_error)?;

    Ok(BuildIndexResult {
        kind: params.kind,
        bytes: lookup.report.bytes,
        elapsed_ns: lookup.report.elapsed_ns,
        hit: lookup.hit,
    })
}

/// Runs `geo.query_knn@1`: returns for each query point the k nearest neighbours in metres,
/// ordered by `(dist2, index)` as defined by `GEO.INDEX.KDTREE`, capped at `10^6` total
/// entries in query order with `truncated: true` (REQ-GEO.INDEX.CMD-001..004).
pub fn query_knn(
    tree: &dyn ElementTree,
    selections: &dyn SelectionStore,
    cache: &mut IndexCache,
    params: &QueryKnnParams,
) -> Result<QueryResult, GeoCmdError> {
    if params.k == 0 || params.k > MAX_K {
        return Err(GeoCmdError::BadParams("k must be between 1 and 1024"));
    }
    let entry = lookup_geo(tree, &params.element)?;
    let query_pts = resolve_query_points(entry, selections, &params.query)?;
    let pts = element_points(entry)?;
    let pt_tree = get_or_build_point_tree(entry, cache)?;

    let mut results = Vec::with_capacity(query_pts.len());
    let mut total = 0usize;
    let mut truncated = false;

    for q in query_pts {
        let hits = pt_tree.knn(pts, q, params.k).map_err(|e| GeoCmdError::Index(e.to_string()))?;
        let remaining = MAX_RESULT_ENTRIES.saturating_sub(total);
        if hits.len() > remaining {
            let take: Vec<QueryHit> = hits
                .into_iter()
                .take(remaining)
                .map(|n| QueryHit { index: n.index, distance_m: n.dist2.sqrt() })
                .collect();
            results.push(take);
            truncated = true;
            break;
        }
        let list: Vec<QueryHit> =
            hits.into_iter().map(|n| QueryHit { index: n.index, distance_m: n.dist2.sqrt() }).collect();
        total += list.len();
        results.push(list);
    }

    Ok(QueryResult { results, truncated })
}

/// Runs `geo.query_radius@1`: returns for each query point every neighbour within the
/// authored radius (converted to SI metres by `fid-geo-frame::units`), ordered by
/// `(dist2, index)`, capped at `10^6` total entries in query order with `truncated: true`
/// (REQ-GEO.INDEX.CMD-001..004).
pub fn query_radius(
    tree: &dyn ElementTree,
    selections: &dyn SelectionStore,
    cache: &mut IndexCache,
    params: &QueryRadiusParams,
) -> Result<QueryResult, GeoCmdError> {
    let entry = lookup_geo(tree, &params.element)?;
    let query_pts = resolve_query_points(entry, selections, &params.query)?;
    let pts = element_points(entry)?;
    let pt_tree = get_or_build_point_tree(entry, cache)?;

    let mut results = Vec::with_capacity(query_pts.len());
    let mut total = 0usize;
    let mut truncated = false;

    for q in query_pts {
        let hits = pt_tree.radius(pts, q, params.radius.metres).map_err(|e| GeoCmdError::Index(e.to_string()))?;
        let remaining = MAX_RESULT_ENTRIES.saturating_sub(total);
        if hits.len() > remaining {
            let take: Vec<QueryHit> = hits
                .into_iter()
                .take(remaining)
                .map(|n| QueryHit { index: n.index, distance_m: n.dist2.sqrt() })
                .collect();
            results.push(take);
            truncated = true;
            break;
        }
        let list: Vec<QueryHit> =
            hits.into_iter().map(|n| QueryHit { index: n.index, distance_m: n.dist2.sqrt() }).collect();
        total += list.len();
        results.push(list);
    }

    Ok(QueryResult { results, truncated })
}

/// Runs `geo.query_box@1`: returns every point inside `[min, max]` in ascending index order
/// (`distance_m: 0.0`), capped at `10^6` entries with `truncated: true`
/// (REQ-GEO.INDEX.CMD-001..004).
pub fn query_box(
    tree: &dyn ElementTree,
    cache: &mut IndexCache,
    params: &QueryBoxParams,
) -> Result<QueryResult, GeoCmdError> {
    let entry = lookup_geo(tree, &params.element)?;
    if let Some(found) = params.frame {
        if found != entry.facts.frame {
            return Err(GeoCmdError::FrameMismatch { expected: entry.facts.frame, found });
        }
    }
    let pts = element_points(entry)?;
    let pt_tree = get_or_build_point_tree(entry, cache)?;
    let aabb = Aabb::new(params.min, params.max).map_err(GeoCmdError::Geometry)?;
    let indices = pt_tree.in_box(pts, &aabb).map_err(|e| GeoCmdError::Index(e.to_string()))?;

    let truncated = indices.len() > MAX_RESULT_ENTRIES;
    let list: Vec<QueryHit> =
        indices.into_iter().take(MAX_RESULT_ENTRIES).map(|index| QueryHit { index, distance_m: 0.0 }).collect();

    Ok(QueryResult { results: vec![list], truncated })
}

// ------------------------------------------------------------------ canonical serialization

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn build_result_to_value(r: &BuildIndexResult) -> CValue {
    obj([
        ("bytes", CValue::Num(r.bytes as f64)),
        ("elapsed_ns", CValue::Num(r.elapsed_ns as f64)),
        ("hit", CValue::Bool(r.hit)),
        ("kind", text(r.kind.as_str())),
    ])
}

fn query_result_to_value(r: &QueryResult) -> CValue {
    let outer = r
        .results
        .iter()
        .map(|list| {
            CValue::Arr(
                list.iter()
                    .map(|h| obj([("distance_m", CValue::Num(h.distance_m)), ("index", CValue::Num(h.index as f64))]))
                    .collect(),
            )
        })
        .collect();
    obj([("results", CValue::Arr(outer)), ("truncated", CValue::Bool(r.truncated))])
}

fn logged(
    log: &mut Vec<ActivityEntry>,
    command: &'static str,
    surface: Surface,
    out: Result<Vec<u8>, GeoCmdError>,
) -> Result<Vec<u8>, GeoCmdError> {
    let outcome = match &out {
        Ok(_) => "ok",
        Err(e) => e.code(),
    };
    log.push(ActivityEntry { command, surface, outcome });
    out
}

fn encode_value(v: &CValue) -> Result<Vec<u8>, GeoCmdError> {
    to_canonical_bytes(v).map_err(|_| GeoCmdError::Internal("canonical json"))
}

// ------------------------------------------------------------------ MCP dispatch

/// Runs one of the four `GEO.INDEX.CMD` MCP tools (`geo_build_index`, `geo_query_knn`,
/// `geo_query_radius`, `geo_query_box`) on raw JSON parameters and returns canonical JSON
/// bytes. Appends exactly one entry to `log` and never touches the Record Journal.
pub fn invoke_index_mcp(
    tree: &dyn ElementTree,
    selections: &dyn SelectionStore,
    cache: &mut IndexCache,
    log: &mut Vec<ActivityEntry>,
    tool: &str,
    params: &[u8],
) -> Result<Vec<u8>, GeoCmdError> {
    let command = match tool {
        MCP_BUILD_INDEX => BUILD_INDEX,
        MCP_QUERY_KNN => QUERY_KNN,
        MCP_QUERY_RADIUS => QUERY_RADIUS,
        MCP_QUERY_BOX => QUERY_BOX,
        _ => {
            return logged(log, BUILD_INDEX, Surface::Mcp, Err(GeoCmdError::UnknownCommand(kept(tool))));
        }
    };
    let parsed = parse_with_limits(params, MAX_QUERY_PARAMS_BYTES, MAX_QUERY_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => GeoCmdError::InputLimit("params"),
        CanonError::DuplicateKey(_) => GeoCmdError::BadParams("duplicate key"),
        _ => GeoCmdError::BadParams("params are not valid JSON"),
    });
    let out = parsed.and_then(|v| dispatch_value(tree, selections, cache, command, &v));
    logged(log, command, Surface::Mcp, out)
}

fn dispatch_value(
    tree: &dyn ElementTree,
    selections: &dyn SelectionStore,
    cache: &mut IndexCache,
    command: &'static str,
    v: &CValue,
) -> Result<Vec<u8>, GeoCmdError> {
    match command {
        BUILD_INDEX => {
            let p = BuildIndexParams::from_value(v)?;
            let res = build_index(tree, cache, &p)?;
            encode_value(&build_result_to_value(&res))
        }
        QUERY_KNN => {
            let p = QueryKnnParams::from_value(v)?;
            let res = query_knn(tree, selections, cache, &p)?;
            encode_value(&query_result_to_value(&res))
        }
        QUERY_RADIUS => {
            let p = QueryRadiusParams::from_value(v)?;
            let res = query_radius(tree, selections, cache, &p)?;
            encode_value(&query_result_to_value(&res))
        }
        QUERY_BOX => {
            let p = QueryBoxParams::from_value(v)?;
            let res = query_box(tree, cache, &p)?;
            encode_value(&query_result_to_value(&res))
        }
        _ => Err(GeoCmdError::UnknownCommand(kept(command))),
    }
}

// ------------------------------------------------------------------ CLI dispatch

fn parse_csv_point3(s: &str) -> Result<CValue, GeoCmdError> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 3 {
        return Err(GeoCmdError::BadParams("point must be x,y,z"));
    }
    let mut vals = Vec::with_capacity(3);
    for p in parts {
        let n: f64 = p.trim().parse().map_err(|_| GeoCmdError::BadParams("invalid coordinate number"))?;
        if !n.is_finite() {
            return Err(GeoCmdError::BadParams("coordinate must be finite"));
        }
        vals.push(CValue::Num(n));
    }
    Ok(CValue::Arr(vals))
}

fn load_points_file(path: &str) -> Result<(CValue, Option<CValue>), GeoCmdError> {
    let bytes = std::fs::read(path).map_err(|_| GeoCmdError::BadParams("cannot read points file"))?;
    if bytes.len() > MAX_QUERY_PARAMS_BYTES {
        return Err(GeoCmdError::InputLimit("points file"));
    }
    let val = parse_with_limits(&bytes, MAX_QUERY_PARAMS_BYTES, MAX_QUERY_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => GeoCmdError::InputLimit("points file"),
        CanonError::DuplicateKey(_) => GeoCmdError::BadParams("duplicate key in points file"),
        _ => GeoCmdError::BadParams("points file is not valid JSON"),
    })?;
    match val {
        CValue::Arr(arr) => Ok((CValue::Arr(arr), None)),
        CValue::Obj(mut o) => {
            if o.keys().any(|k| k != "points" && k != "frame") {
                return Err(GeoCmdError::BadParams("unknown key in points file"));
            }
            let pts = o.remove("points").ok_or(GeoCmdError::BadParams("points file missing points array"))?;
            let frame = o.remove("frame");
            Ok((pts, frame))
        }
        _ => Err(GeoCmdError::BadParams("points file must be an array or {points, frame?} object")),
    }
}

fn next_val<'a>(rest: &'a [&'a str], i: &mut usize, flag: &'static str) -> Result<&'a str, GeoCmdError> {
    *i += 1;
    rest.get(*i).copied().ok_or(GeoCmdError::BadParams(flag))
}

/// Runs `fid geo <args>` for `build-index`, `query-knn`, `query-radius`, or `query-box`.
/// Output is RFC 8785 canonical JSON. Appends exactly one entry to `log` and never touches
/// the Record Journal.
pub fn invoke_index_cli(
    tree: &dyn ElementTree,
    selections: &dyn SelectionStore,
    cache: &mut IndexCache,
    log: &mut Vec<ActivityEntry>,
    args: &[&str],
) -> Result<Vec<u8>, GeoCmdError> {
    if args.len() > MAX_CLI_ARGS {
        return logged(log, BUILD_INDEX, Surface::Cli, Err(GeoCmdError::InputLimit("args")));
    }
    let Some((sub, rest)) = args.split_first() else {
        return logged(log, BUILD_INDEX, Surface::Cli, Err(GeoCmdError::BadParams("missing subcommand")));
    };
    let command = match *sub {
        CLI_BUILD_INDEX => BUILD_INDEX,
        CLI_QUERY_KNN => QUERY_KNN,
        CLI_QUERY_RADIUS => QUERY_RADIUS,
        CLI_QUERY_BOX => QUERY_BOX,
        _ => {
            return logged(log, BUILD_INDEX, Surface::Cli, Err(GeoCmdError::UnknownCommand(kept(sub))));
        }
    };
    let out = cli_to_value(command, rest).and_then(|v| dispatch_value(tree, selections, cache, command, &v));
    logged(log, command, Surface::Cli, out)
}

fn cli_to_value(command: &'static str, rest: &[&str]) -> Result<CValue, GeoCmdError> {
    let mut map = BTreeMap::new();
    let mut element: Option<&str> = None;
    let mut json_flag = false;
    let mut i = 0usize;

    while i < rest.len() {
        let arg = rest[i];
        match arg {
            "--json" => {
                if json_flag {
                    return Err(GeoCmdError::BadParams("repeated --json"));
                }
                json_flag = true;
            }
            "--kind" if command == BUILD_INDEX => {
                let v = next_val(rest, &mut i, "missing value for --kind")?;
                if map.insert("kind".to_string(), text(v)).is_some() {
                    return Err(GeoCmdError::BadParams("repeated --kind"));
                }
            }
            "--budget-bytes" if command == BUILD_INDEX => {
                let v = next_val(rest, &mut i, "missing value for --budget-bytes")?;
                let n: u64 = v.parse().map_err(|_| GeoCmdError::BadParams("invalid --budget-bytes"))?;
                if map.insert("budget_bytes".to_string(), CValue::Num(n as f64)).is_some() {
                    return Err(GeoCmdError::BadParams("repeated --budget-bytes"));
                }
            }
            "--k" if command == QUERY_KNN => {
                let v = next_val(rest, &mut i, "missing value for --k")?;
                let n: u64 = v.parse().map_err(|_| GeoCmdError::BadParams("invalid --k"))?;
                if map.insert("k".to_string(), CValue::Num(n as f64)).is_some() {
                    return Err(GeoCmdError::BadParams("repeated --k"));
                }
            }
            "--radius" if command == QUERY_RADIUS => {
                let v = next_val(rest, &mut i, "missing value for --radius")?;
                if map.insert("radius".to_string(), text(v)).is_some() {
                    return Err(GeoCmdError::BadParams("repeated --radius"));
                }
            }
            "--points" if command == QUERY_KNN || command == QUERY_RADIUS => {
                let path = next_val(rest, &mut i, "missing value for --points")?;
                let (pts, file_frame) = load_points_file(path)?;
                if map.insert("points".to_string(), pts).is_some() {
                    return Err(GeoCmdError::BadParams("repeated --points"));
                }
                if let Some(f) = file_frame {
                    map.entry("frame".to_string()).or_insert(f);
                }
            }
            "--points-json" if command == QUERY_KNN || command == QUERY_RADIUS => {
                let raw = next_val(rest, &mut i, "missing value for --points-json")?;
                let pts = parse_with_limits(raw.as_bytes(), MAX_QUERY_PARAMS_BYTES, MAX_QUERY_PARAMS_DEPTH)
                    .map_err(|_| GeoCmdError::BadParams("invalid --points-json"))?;
                if map.insert("points".to_string(), pts).is_some() {
                    return Err(GeoCmdError::BadParams("repeated --points"));
                }
            }
            "--selection" if command == QUERY_KNN || command == QUERY_RADIUS => {
                let v = next_val(rest, &mut i, "missing value for --selection")?;
                if map.insert("selection".to_string(), text(v)).is_some() {
                    return Err(GeoCmdError::BadParams("repeated --selection"));
                }
            }
            "--frame" if command != BUILD_INDEX => {
                let v = next_val(rest, &mut i, "missing value for --frame")?;
                if map.insert("frame".to_string(), text(v)).is_some() {
                    return Err(GeoCmdError::BadParams("repeated --frame"));
                }
            }
            "--min" if command == QUERY_BOX => {
                let v = next_val(rest, &mut i, "missing value for --min")?;
                if map.insert("min".to_string(), parse_csv_point3(v)?).is_some() {
                    return Err(GeoCmdError::BadParams("repeated --min"));
                }
            }
            "--max" if command == QUERY_BOX => {
                let v = next_val(rest, &mut i, "missing value for --max")?;
                if map.insert("max".to_string(), parse_csv_point3(v)?).is_some() {
                    return Err(GeoCmdError::BadParams("repeated --max"));
                }
            }
            s if s.starts_with("--") => return Err(GeoCmdError::BadParams("unknown option")),
            s if element.is_none() => element = Some(s),
            _ => return Err(GeoCmdError::BadParams("unexpected positional argument")),
        }
        i += 1;
    }

    if let Some(e) = element {
        map.insert("element".to_string(), text(e));
    }
    if command == BUILD_INDEX && !map.contains_key("kind") {
        map.insert("kind".to_string(), text("point"));
    }
    Ok(CValue::Obj(map))
}

// ------------------------------------------------------------------ registry definitions

fn length_number_schema() -> CValue {
    obj([("type", text("number")), ("x-quantity", text("length")), ("x-unit", text("m"))])
}

fn point3_schema() -> CValue {
    obj([
        ("type", text("array")),
        ("items", length_number_schema()),
        ("minItems", CValue::Num(3.0)),
        ("maxItems", CValue::Num(3.0)),
    ])
}

fn uuid_schema(desc: &str) -> CValue {
    obj([("type", text("string")), ("pattern", text(ID_PATTERN)), ("description", text(desc))])
}

fn element_ref_schema(desc: &str) -> CValue {
    obj([
        ("type", text("string")),
        ("x-ref", text("element")),
        ("pattern", text(ID_PATTERN)),
        ("description", text(desc)),
    ])
}

fn query_result_schema() -> CValue {
    let hit = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["distance_m", "index"])),
        (
            "properties",
            obj([
                ("distance_m", length_number_schema()),
                ("index", obj([("type", text("integer")), ("minimum", CValue::Num(0.0))])),
            ]),
        ),
    ]);
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["results", "truncated"])),
        (
            "properties",
            obj([
                ("results", obj([("type", text("array")), ("items", obj([("type", text("array")), ("items", hit)]))])),
                ("truncated", obj([("type", text("boolean"))])),
            ]),
        ),
    ])
}

fn build_r1_command(
    id: &str,
    title: &str,
    description: &str,
    params_schema: CValue,
    result_schema: CValue,
) -> Result<CommandDef, DefError> {
    CommandDef::from_value(obj([
        ("id", text(id)),
        ("major", CValue::Num(1.0)),
        ("title", text(title)),
        ("description", text(description)),
        ("params_schema", params_schema),
        ("result_schema", result_schema),
        ("side_effect", text("R1")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("natural")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", CValue::Arr(Vec::new())),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}

/// The registry definitions of the four `GEO.INDEX.CMD` commands (`geo.build_index@1`,
/// `geo.query_knn@1`, `geo.query_radius@1`, `geo.query_box@1`): class R1, determinism D0,
/// exposed on CLI and MCP with `mcp_projection: "exposed"`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let build_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["element", "kind"])),
        (
            "properties",
            obj([
                ("budget_bytes", obj([("type", text("integer")), ("minimum", CValue::Num(0.0))])),
                ("element", element_ref_schema("The geometric element to index.")),
                ("kind", obj([("type", text("string")), ("enum", strs(&["point", "primitive"]))])),
            ]),
        ),
    ]);
    let build_result = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["bytes", "elapsed_ns", "hit", "kind"])),
        (
            "properties",
            obj([
                ("bytes", obj([("type", text("integer")), ("minimum", CValue::Num(0.0))])),
                ("elapsed_ns", obj([("type", text("integer")), ("minimum", CValue::Num(0.0))])),
                ("hit", obj([("type", text("boolean"))])),
                ("kind", obj([("type", text("string")), ("enum", strs(&["point", "primitive"]))])),
            ]),
        ),
    ]);

    let points_array = obj([
        ("type", text("array")),
        ("items", point3_schema()),
        ("maxItems", CValue::Num(MAX_INLINE_QUERIES as f64)),
    ]);

    let knn_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["element", "k"])),
        (
            "properties",
            obj([
                ("element", element_ref_schema("The geometric element to query.")),
                ("frame", uuid_schema("Optional frame UUID of inline query points.")),
                (
                    "k",
                    obj([
                        ("type", text("integer")),
                        ("minimum", CValue::Num(1.0)),
                        ("maximum", CValue::Num(MAX_K as f64)),
                    ]),
                ),
                ("points", points_array.clone()),
                ("selection", uuid_schema("Optional selection handle providing query points.")),
            ]),
        ),
    ]);

    let radius_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["element", "radius"])),
        (
            "properties",
            obj([
                ("element", element_ref_schema("The geometric element to query.")),
                ("frame", uuid_schema("Optional frame UUID of inline query points.")),
                ("points", points_array),
                (
                    "radius",
                    obj([
                        ("type", text("string")),
                        ("description", text("Authored non-negative length with unit symbol (e.g. '5mm', '0.01m').")),
                    ]),
                ),
                ("selection", uuid_schema("Optional selection handle providing query points.")),
            ]),
        ),
    ]);

    let box_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["element", "max", "min"])),
        (
            "properties",
            obj([
                ("element", element_ref_schema("The geometric element to query.")),
                ("frame", uuid_schema("Optional frame UUID of the query box.")),
                ("max", point3_schema()),
                ("min", point3_schema()),
            ]),
        ),
    ]);

    let q_res = query_result_schema();
    Ok(vec![
        build_r1_command(
            "geo.build_index",
            "Build spatial index",
            "Builds or warms the cached spatial index (point k-d tree or primitive triangle BVH) for an element under an optional memory budget.",
            build_params,
            build_result,
        )?,
        build_r1_command(
            "geo.query_knn",
            "Query k-nearest neighbours",
            "Returns the k nearest neighbours in metres for each query point in deterministic (distance, index) order, capped at 1,000,000 entries.",
            knn_params,
            q_res.clone(),
        )?,
        build_r1_command(
            "geo.query_radius",
            "Query neighbours within radius",
            "Returns every neighbour within an authored length radius in deterministic (distance, index) order, capped at 1,000,000 entries.",
            radius_params,
            q_res.clone(),
        )?,
        build_r1_command(
            "geo.query_box",
            "Query points inside axis-aligned box",
            "Returns every point inside an axis-aligned bounding box in ascending index order, capped at 1,000,000 entries.",
            box_params,
            q_res,
        )?,
    ])
}

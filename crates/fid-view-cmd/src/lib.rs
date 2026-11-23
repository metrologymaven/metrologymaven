//! View and render registry commands (spec leaf `VIZ.RENDER.FRAME.TIER`).
//!
//! Declares `view.get_render_info@1` (`R0`, determinism `D1`, MCP `view_get_render_info`,
//! CLI `fid view get-render-info`): returns [`RenderInfo`] (tier, backend, adapter, driver,
//! point-raster path, transparency path, frame-time and GPU memory budgets, and displayed/total
//! point and triangle counts of the last presented frame) as RFC 8785 canonical JSON matching
//! `schemas/render-info-1.json`.
//!
//! Mutates no document, view or render state (REQ-VIZ.RENDER.FRAME.TIER-004). Under D-031,
//! because its side-effect class is `R0`, the journal router writes it to the Activity Log only
//! while a recording session is active; outside a recording session both the journal and
//! Activity Log remain untouched.

#![deny(clippy::todo, clippy::unimplemented, clippy::unreachable)]

use fid_bus::error::{BusError, Detail, InternalReason, NotFoundReason, PreconditionReason, ValidationReason};
use fid_bus::handler::{Event, Handler, HandlerCtx, HandlerError, StateView, ValidParams};
use fid_canon::{to_canonical_bytes, CValue};
use fid_registry::{CommandDef, DefError};
use fid_render::frame::graph::{Backend, FrameGraph};
use fid_render::frame::tier::{CapabilityTier, PointRasterPath, RenderInfo, TierError, TransparencyPath};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// Command ID name (`view.get_render_info`, major 1).
pub const GET_RENDER_INFO: &str = "view.get_render_info";
/// Default viewport ID used when `params.viewport` is omitted.
pub const DEFAULT_VIEWPORT: &str = "main";
/// Maximum length of a `ViewportId`, in bytes.
pub const MAX_VIEWPORT_ID_LEN: usize = 64;

const LIFECYCLE_SINCE: &str = "2026-11-22";
const VIEWPORT_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$";

// ------------------------------------------------------------------ errors & viewport IDs

/// Domain errors for `view.get_render_info@1` (`VIZ.RENDER.FRAME.TIER`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderInfoError {
    /// No renderer device is ready yet (AC-4).
    RendererNotReady,
    /// The requested viewport ID does not exist.
    UnknownViewport(String),
    /// Invalid command parameters.
    InvalidParams(String),
    /// Failed to encode `RenderInfo` to canonical JSON.
    Encode(TierError),
}

impl RenderInfoError {
    /// Stable error code (`RendererNotReady`, `UnknownViewport`, ...).
    pub fn code(&self) -> &'static str {
        match self {
            RenderInfoError::RendererNotReady => "RendererNotReady",
            RenderInfoError::UnknownViewport(_) => "UnknownViewport",
            RenderInfoError::InvalidParams(_) => "InvalidParams",
            RenderInfoError::Encode(_) => "EncodeError",
        }
    }

    /// Maps this domain error onto the frozen `fid-bus` error catalogue.
    pub fn to_bus_error(&self) -> BusError {
        match self {
            RenderInfoError::RendererNotReady => {
                BusError::Precondition(PreconditionReason::False, Detail::new("RendererNotReady"))
            }
            RenderInfoError::UnknownViewport(id) => {
                BusError::NotFound(NotFoundReason::UnknownEntity, Detail::new(&format!("UnknownViewport: {id}")))
            }
            RenderInfoError::InvalidParams(msg) => BusError::Validation(ValidationReason::Schema, Detail::new(msg)),
            RenderInfoError::Encode(e) => {
                BusError::Internal(InternalReason::HandlerFailed, Detail::new(&e.to_string()))
            }
        }
    }
}

impl fmt::Display for RenderInfoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderInfoError::RendererNotReady => f.write_str("RendererNotReady"),
            RenderInfoError::UnknownViewport(id) => write!(f, "UnknownViewport: {id}"),
            RenderInfoError::InvalidParams(msg) => write!(f, "InvalidParams: {msg}"),
            RenderInfoError::Encode(e) => write!(f, "EncodeError: {e}"),
        }
    }
}

impl std::error::Error for RenderInfoError {}

impl From<RenderInfoError> for BusError {
    fn from(e: RenderInfoError) -> BusError {
        e.to_bus_error()
    }
}

/// Validates a `ViewportId`: 1..=64 ASCII chars `[A-Za-z0-9][A-Za-z0-9_.-]*`.
pub fn valid_viewport_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_VIEWPORT_ID_LEN
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'))
}

// ------------------------------------------------------------------ state source

/// Read-only source of [`RenderInfo`] snapshots per viewport.
pub trait RenderInfoSource {
    /// Returns the latest [`RenderInfo`] snapshot for `viewport` (or the default viewport when
    /// `None`). Must mutate no document, view or render state.
    fn render_info(&self, viewport: Option<&str>) -> Result<RenderInfo, RenderInfoError>;
}

/// Pure in-memory table mapping viewport IDs to their latest [`RenderInfo`] snapshot (or `None`
/// when a viewport exists without an initialised GPU device yet).
#[derive(Debug, Clone, Default)]
pub struct ViewportRegistry {
    viewports: BTreeMap<String, Option<RenderInfo>>,
}

impl ViewportRegistry {
    /// Creates an empty registry (no viewports, no device ready -> `RendererNotReady`).
    pub fn new() -> ViewportRegistry {
        ViewportRegistry::default()
    }

    /// Registers a viewport with no ready device yet (`RendererNotReady` until populated).
    pub fn register_viewport(&mut self, id: impl Into<String>) -> Result<(), RenderInfoError> {
        let id = id.into();
        if !valid_viewport_id(&id) {
            return Err(RenderInfoError::InvalidParams(format!("invalid viewport id `{id}`")));
        }
        self.viewports.entry(id).or_insert(None);
        Ok(())
    }

    /// Stores a ready [`RenderInfo`] snapshot for `id` (creating the viewport entry if needed).
    pub fn set_ready(&mut self, id: impl Into<String>, info: RenderInfo) -> Result<(), RenderInfoError> {
        let id = id.into();
        if !valid_viewport_id(&id) {
            return Err(RenderInfoError::InvalidParams(format!("invalid viewport id `{id}`")));
        }
        self.viewports.insert(id, Some(info));
        Ok(())
    }

    /// Updates `id` with a fresh snapshot read from `graph`.
    pub fn update_from_graph(&mut self, id: impl Into<String>, graph: &FrameGraph) -> Result<(), RenderInfoError> {
        self.set_ready(id, graph.render_info())
    }

    /// Marks `id` as having no ready device (e.g. after device loss or before initialisation).
    pub fn clear_device(&mut self, id: &str) {
        if let Some(slot) = self.viewports.get_mut(id) {
            *slot = None;
        }
    }
}

impl RenderInfoSource for ViewportRegistry {
    fn render_info(&self, viewport: Option<&str>) -> Result<RenderInfo, RenderInfoError> {
        match viewport {
            None => match self.viewports.get(DEFAULT_VIEWPORT) {
                Some(Some(info)) => Ok(info.clone()),
                _ => Err(RenderInfoError::RendererNotReady),
            },
            Some(id) => {
                if !valid_viewport_id(id) {
                    return Err(RenderInfoError::InvalidParams(format!("invalid viewport id `{id}`")));
                }
                match self.viewports.get(id) {
                    None => Err(RenderInfoError::UnknownViewport(id.to_string())),
                    Some(None) => Err(RenderInfoError::RendererNotReady),
                    Some(Some(info)) => Ok(info.clone()),
                }
            }
        }
    }
}

impl RenderInfoSource for Option<RenderInfo> {
    fn render_info(&self, viewport: Option<&str>) -> Result<RenderInfo, RenderInfoError> {
        if let Some(id) = viewport {
            if !valid_viewport_id(id) {
                return Err(RenderInfoError::InvalidParams(format!("invalid viewport id `{id}`")));
            }
            if id != DEFAULT_VIEWPORT {
                return Err(RenderInfoError::UnknownViewport(id.to_string()));
            }
        }
        self.clone().ok_or(RenderInfoError::RendererNotReady)
    }
}

// ------------------------------------------------------------------ command execution & handler

/// Parses `{ viewport?: ViewportId }` strictly (rejecting unknown keys or invalid viewport IDs).
pub fn parse_viewport_param(params: &CValue) -> Result<Option<String>, RenderInfoError> {
    let CValue::Obj(map) = params else {
        return Err(RenderInfoError::InvalidParams("params must be a JSON object".into()));
    };
    for key in map.keys() {
        if key != "viewport" {
            return Err(RenderInfoError::InvalidParams(format!("unexpected parameter `{key}`")));
        }
    }
    match map.get("viewport") {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(id)) => {
            if !valid_viewport_id(id) {
                return Err(RenderInfoError::InvalidParams(format!("invalid viewport id `{id}`")));
            }
            Ok(Some(id.clone()))
        }
        Some(_) => Err(RenderInfoError::InvalidParams("`viewport` must be a string".into())),
    }
}

/// Typed read entrypoint for `view.get_render_info@1`. Mutates nothing.
pub fn get_render_info(source: &dyn RenderInfoSource, viewport: Option<&str>) -> Result<RenderInfo, RenderInfoError> {
    source.render_info(viewport)
}

/// Executes `view.get_render_info@1` over canonical JSON params and returns canonical [`CValue`].
pub fn run_get_render_info(source: &dyn RenderInfoSource, params: &CValue) -> Result<CValue, BusError> {
    let viewport = parse_viewport_param(params)?;
    let info = get_render_info(source, viewport.as_deref())?;
    info.to_cvalue().map_err(|e| RenderInfoError::Encode(e).to_bus_error())
}

/// Executes `view.get_render_info@1` and returns RFC 8785 canonical JSON bytes matching
/// `schemas/render-info-1.json`.
pub fn run_get_render_info_json(source: &dyn RenderInfoSource, params: &CValue) -> Result<Vec<u8>, BusError> {
    let v = run_get_render_info(source, params)?;
    to_canonical_bytes(&v).map_err(|e| BusError::Internal(InternalReason::HandlerFailed, Detail::new(&e.to_string())))
}

/// Bus [`Handler`] for `view.get_render_info@1` (R0: validates params and device readiness,
/// emits zero events, mutates nothing).
pub struct GetRenderInfoHandler {
    source: Arc<dyn RenderInfoSource + Send + Sync>,
}

impl GetRenderInfoHandler {
    pub fn new(source: Arc<dyn RenderInfoSource + Send + Sync>) -> GetRenderInfoHandler {
        GetRenderInfoHandler { source }
    }
}

impl Handler for GetRenderInfoHandler {
    fn run(&self, _state: &dyn StateView, params: &ValidParams, _ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        let _ = run_get_render_info(self.source.as_ref(), params.value()).map_err(HandlerError::from)?;
        Ok(Vec::new())
    }
}

// ------------------------------------------------------------------ registry declaration

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn obj(fields: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn counts_schema(description: &str) -> CValue {
    obj(vec![
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["displayed", "total"])),
        (
            "properties",
            obj(vec![
                ("displayed", obj(vec![("type", text("integer")), ("minimum", CValue::Num(0.0))])),
                ("total", obj(vec![("type", text("integer")), ("minimum", CValue::Num(0.0))])),
            ]),
        ),
        ("description", text(description)),
    ])
}

/// Command declarations exported by `fid-view-cmd`: `view.get_render_info@1`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let params_schema = obj(vec![
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj(vec![(
                "viewport",
                obj(vec![
                    ("type", text("string")),
                    ("minLength", CValue::Num(1.0)),
                    ("maxLength", CValue::Num(MAX_VIEWPORT_ID_LEN as f64)),
                    ("pattern", text(VIEWPORT_PATTERN)),
                    ("description", text("Optional viewport identifier; defaults to the primary viewport.")),
                ]),
            )]),
        ),
    ]);

    let tiers: Vec<&str> = CapabilityTier::ALL.iter().map(|t| t.as_str()).collect();
    let backends: Vec<&str> = Backend::ALL.iter().map(|b| b.as_str()).collect();
    let point_rasters: Vec<&str> = PointRasterPath::ALL.iter().map(|p| p.as_str()).collect();
    let transparencies: Vec<&str> = TransparencyPath::ALL.iter().map(|t| t.as_str()).collect();

    let result_schema = obj(vec![
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        (
            "required",
            strs(&[
                "adapter",
                "backend",
                "driver",
                "frame_budget_ms",
                "mem_budget_bytes",
                "point_raster",
                "points",
                "tier",
                "transparency",
                "triangles",
            ]),
        ),
        (
            "properties",
            obj(vec![
                ("tier", obj(vec![("type", text("string")), ("enum", strs(&tiers))])),
                ("backend", obj(vec![("type", text("string")), ("enum", strs(&backends))])),
                ("adapter", obj(vec![("type", text("string")), ("minLength", CValue::Num(1.0))])),
                ("driver", obj(vec![("type", text("string"))])),
                ("point_raster", obj(vec![("type", text("string")), ("enum", strs(&point_rasters))])),
                ("transparency", obj(vec![("type", text("string")), ("enum", strs(&transparencies))])),
                ("frame_budget_ms", obj(vec![("type", text("integer")), ("minimum", CValue::Num(1.0))])),
                ("mem_budget_bytes", obj(vec![("type", text("integer")), ("minimum", CValue::Num(0.0))])),
                ("points", counts_schema("Displayed and total point counts of the last presented frame.")),
                ("triangles", counts_schema("Displayed and total triangle counts of the last presented frame.")),
            ]),
        ),
    ]);

    let def = CommandDef::from_value(obj(vec![
        ("id", text(GET_RENDER_INFO)),
        ("major", CValue::Num(1.0)),
        ("title", text("Get render info")),
        (
            "description",
            text(
                "Returns the active renderer's capability tier, backend, adapter, driver, point-raster \
                 and transparency paths, frame-time and GPU memory budgets, and last-frame primitive \
                 counts as canonical JSON.",
            ),
        ),
        ("params_schema", params_schema),
        ("result_schema", result_schema),
        ("side_effect", text("R0")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D1")),
        ("idempotency", text("natural")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", CValue::Arr(Vec::new())),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp", "gui"])),
        ("lifecycle", obj(vec![("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))?;

    Ok(vec![def])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_def_parses_and_exposes_expected_surfaces() {
        let defs = command_defs();
        assert!(defs.is_ok(), "{defs:?}");
        if let Ok(defs) = defs {
            assert_eq!(defs.len(), 1);
            assert_eq!(defs[0].id, "view.get_render_info");
            assert_eq!(defs[0].major, 1);
        }
    }
}

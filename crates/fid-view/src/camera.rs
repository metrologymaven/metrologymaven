//! Set-camera (`view.set_camera@1`, W2) and fly-to (`view.fly_to@1`, R1) registry commands
//! (spec leaf `VIZ.VIEW.CAMERA.SET`).
//!
//! - `view.set_camera@1` (W2, D0): writes exact `f64` eye, target, up (metres) and projection
//!   into the target viewport's [`ViewState`], rejects non-finite or degenerate cameras without
//!   journaling (`NonFiniteNumber`, `DegenerateCamera`), journals the canonical event carrying
//!   the resulting `ViewState` SHA-256 digest, and replays identically (`REQ-001..003`).
//! - `view.fly_to@1` (R1, D0): computes the end camera deterministically from the target
//!   entity's `f64` bounds and the live camera's current view direction using
//!   [`crate::nav::fit_camera`], applies it only to the live [`CameraController`] (jumping
//!   immediately under `reduced_motion`, `REQ-006`), never modifies the document [`ViewState`]
//!   or journal, logs an Activity Log record only during recording (`D-031`), and attaches a
//!   `"proposal"` tag when called by an agent principal until the next human camera change
//!   (`REQ-004`, `REQ-005`).

use crate::nav::{fit_camera, BoundingSphere, CameraController};
use crate::state::{Camera, EntityId, Projection, ViewState, ViewStateError, Viewport};
use fid_canon::CValue;
use fid_registry::{fid_command, fid_commands, CommandDef, DefError, FidSchema};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Registry / journal event ID for `view.set_camera@1` (W2).
pub const SET_CAMERA_EVENT: &str = "view.set_camera@1";
/// Activity Log entry ID for `view.fly_to@1` (R1, recorded only when recording is active).
pub const FLY_TO_EVENT: &str = "view.fly_to@1";
/// Maximum byte length of a viewport identifier.
pub const MAX_VIEWPORT_ID_BYTES: usize = 64;
/// Canonical UUID regex pattern for schema declarations.
const UUID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";
/// Viewport identifier pattern (1..=64 safe ASCII identifier chars).
const VIEWPORT_PATTERN: &str = "^[A-Za-z0-9._-]{1,64}$";
/// Exact conversion factor `pi / 180` in f64 (used once when converting authored degree literals).
const DEG_TO_RAD: f64 = std::f64::consts::PI / 180.0;

// ------------------------------------------------------------------ errors

/// Typed errors of `view.set_camera@1` and `view.fly_to@1` (`VIZ.VIEW.CAMERA.SET`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CameraError {
    /// Eye equals target, up is zero or parallel to view direction, or projection/clip bounds invalid.
    DegenerateCamera { field: &'static str, reason: &'static str },
    /// Any numeric input is NaN or infinite.
    NonFiniteNumber { field: &'static str },
    /// Named viewport does not exist in the workspace.
    UnknownViewport(String),
    /// Named entity does not exist in the scene.
    UnknownEntity(EntityId),
    /// Named entity exists in the scene but is not currently displayed in the viewport.
    EntityNotDisplayed(EntityId),
    /// Malformed CLI or JSON command parameters.
    InvalidParam(&'static str),
    /// Underlying [`ViewState`] validation or canonicalization error.
    State(ViewStateError),
}

impl CameraError {
    /// Stable PascalCase / spec variant tag matching `VIZ.VIEW.CAMERA.SET` Design & AC-3.
    pub fn variant_name(&self) -> &'static str {
        match self {
            CameraError::DegenerateCamera { .. } => "DegenerateCamera",
            CameraError::NonFiniteNumber { .. } => "NonFiniteNumber",
            CameraError::UnknownViewport(_) => "UnknownViewport",
            CameraError::UnknownEntity(_) => "UnknownEntity",
            CameraError::EntityNotDisplayed(_) => "EntityNotDisplayed",
            CameraError::InvalidParam(_) => "InvalidParam",
            CameraError::State(ViewStateError::DegenerateCamera { .. }) => "DegenerateCamera",
            CameraError::State(ViewStateError::NonFiniteNumber { .. }) => "NonFiniteNumber",
            CameraError::State(_) => "ViewStateError",
        }
    }

    /// Stable snake_case machine code.
    pub fn code(&self) -> &'static str {
        match self {
            CameraError::DegenerateCamera { .. } => "view.degenerate_camera",
            CameraError::NonFiniteNumber { .. } => "view.non_finite_number",
            CameraError::UnknownViewport(_) => "view.unknown_viewport",
            CameraError::UnknownEntity(_) => "view.unknown_entity",
            CameraError::EntityNotDisplayed(_) => "view.entity_not_displayed",
            CameraError::InvalidParam(_) => "view.invalid_param",
            CameraError::State(e) => e.code(),
        }
    }

    /// Canonical `--json` / MCP error object.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code().to_string()));
        o.insert("error".to_string(), CValue::Str(self.variant_name().to_string()));
        match self {
            CameraError::DegenerateCamera { field, reason } => {
                o.insert("field".to_string(), CValue::Str((*field).to_string()));
                o.insert("reason".to_string(), CValue::Str((*reason).to_string()));
            }
            CameraError::NonFiniteNumber { field } => {
                o.insert("field".to_string(), CValue::Str((*field).to_string()));
            }
            CameraError::UnknownViewport(vp) => {
                o.insert("viewport".to_string(), CValue::Str(vp.clone()));
            }
            CameraError::UnknownEntity(id) | CameraError::EntityNotDisplayed(id) => {
                o.insert("entity".to_string(), CValue::Str(id.to_string()));
            }
            CameraError::InvalidParam(reason) => {
                o.insert("reason".to_string(), CValue::Str((*reason).to_string()));
            }
            CameraError::State(e) => {
                o.insert("detail".to_string(), CValue::Str(e.to_string()));
            }
        }
        CValue::Obj(o)
    }
}

impl fmt::Display for CameraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CameraError::DegenerateCamera { field, reason } => {
                write!(f, "DegenerateCamera ({field}): {reason}")
            }
            CameraError::NonFiniteNumber { field } => write!(f, "NonFiniteNumber ({field})"),
            CameraError::UnknownViewport(vp) => write!(f, "UnknownViewport: {vp}"),
            CameraError::UnknownEntity(id) => write!(f, "UnknownEntity: {id}"),
            CameraError::EntityNotDisplayed(id) => write!(f, "EntityNotDisplayed: {id}"),
            CameraError::InvalidParam(r) => write!(f, "InvalidParam: {r}"),
            CameraError::State(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CameraError {}

impl From<ViewStateError> for CameraError {
    fn from(e: ViewStateError) -> Self {
        match e {
            ViewStateError::NonFiniteNumber { .. } => CameraError::NonFiniteNumber { field: "camera" },
            ViewStateError::DegenerateCamera { reason, .. } => {
                CameraError::DegenerateCamera { field: "camera", reason }
            }
            other => CameraError::State(other),
        }
    }
}

// ------------------------------------------------------------------ principals & proposal tag

/// Calling principal class (`CORE.BUS.PRIN`, kept pure-Rust for wasm32 compatibility).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PrincipalKind {
    Human,
    Agent,
    Script,
}

impl PrincipalKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            PrincipalKind::Human => "human",
            PrincipalKind::Agent => "agent",
            PrincipalKind::Script => "script",
        }
    }

    pub fn parse(s: &str) -> Option<PrincipalKind> {
        match s {
            "human" => Some(PrincipalKind::Human),
            "agent" => Some(PrincipalKind::Agent),
            "script" => Some(PrincipalKind::Script),
            _ => None,
        }
    }
}

/// Proposal badge shown on a viewport when an agent calls `view.fly_to@1` (`REQ-005`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProposalTag {
    /// Wire tag label (`"proposal"`).
    pub label: &'static str,
    /// Principal that proposed the framing (`PrincipalKind::Agent`).
    pub principal: PrincipalKind,
    /// Target entity framed by the proposed fly-to.
    pub target_entity: EntityId,
}

impl ProposalTag {
    /// Creates the `"proposal"` tag for an agent-initiated fly-to.
    pub fn agent_proposal(target_entity: EntityId) -> ProposalTag {
        ProposalTag { label: "proposal", principal: PrincipalKind::Agent, target_entity }
    }
}

// ------------------------------------------------------------------ scene bounds seam

/// Result of querying a scene for an entity's bounding sphere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EntityBoundsLookup {
    /// Entity exists and is displayed with this `f64` world-space bounding sphere.
    Displayed(BoundingSphere),
    /// Entity exists in the scene but is currently hidden / not displayed.
    Hidden,
    /// Entity does not exist in the scene.
    Unknown,
}

/// Read-only scene-graph bounds provider used by `view.fly_to@1` (C2: no GPU dependency).
pub trait SceneBounds {
    fn lookup_entity(&self, entity: &EntityId) -> EntityBoundsLookup;
}

/// In-memory [`SceneBounds`] map for deterministic headless execution and tests.
#[derive(Debug, Clone, Default)]
pub struct MemorySceneBounds {
    entries: BTreeMap<EntityId, Option<BoundingSphere>>,
}

impl MemorySceneBounds {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a displayed entity with its world-space `f64` bounding sphere.
    pub fn insert_displayed(&mut self, entity: EntityId, bounds: BoundingSphere) {
        self.entries.insert(entity, Some(bounds));
    }

    /// Registers an existing entity that is currently hidden / not displayed.
    pub fn insert_hidden(&mut self, entity: EntityId) {
        self.entries.insert(entity, None);
    }
}

impl SceneBounds for MemorySceneBounds {
    fn lookup_entity(&self, entity: &EntityId) -> EntityBoundsLookup {
        match self.entries.get(entity) {
            Some(Some(sphere)) => EntityBoundsLookup::Displayed(*sphere),
            Some(None) => EntityBoundsLookup::Hidden,
            None => EntityBoundsLookup::Unknown,
        }
    }
}

// ------------------------------------------------------------------ viewport state & workspace

/// Validated viewport identifier (1..=64 ASCII `[A-Za-z0-9._-]`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewportId(String);

impl ViewportId {
    pub fn new(raw: &str) -> Result<ViewportId, CameraError> {
        if raw.is_empty()
            || raw.len() > MAX_VIEWPORT_ID_BYTES
            || !raw.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return Err(CameraError::InvalidParam("invalid viewport id"));
        }
        Ok(ViewportId(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ViewportId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One live viewport: journaled working-document [`ViewState`] + transient [`CameraController`]
/// + reduced-motion toggle + agent proposal badge.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportSlot {
    /// Journaled document state (changed ONLY by W2 commands such as `view.set_camera@1`).
    pub doc_state: ViewState,
    /// Live presentation camera (changed by gestures and R1 `view.fly_to@1`).
    pub controller: CameraController,
    /// When `true`, `view.fly_to@1` jumps to the end camera with no animation (`REQ-006`).
    pub reduced_motion: bool,
    /// Present when an agent called `view.fly_to@1`; cleared on the next human camera change (`REQ-005`).
    pub proposal_tag: Option<ProposalTag>,
}

impl ViewportSlot {
    /// Creates a viewport slot initialized with `doc_state`.
    pub fn new(doc_state: ViewState) -> Result<Self, CameraError> {
        doc_state.validate()?;
        let live = doc_state.camera;
        Ok(ViewportSlot {
            doc_state,
            controller: CameraController::new(live),
            reduced_motion: false,
            proposal_tag: None,
        })
    }

    /// Current working-document ViewState digest.
    pub fn doc_digest(&self) -> Result<Digest, CameraError> {
        Ok(self.doc_state.digest()?)
    }

    /// Clears the agent proposal tag when a person moves the camera (`REQ-005`).
    pub fn on_human_camera_change(&mut self) {
        self.proposal_tag = None;
    }

    /// Performs a human orbit gesture on the live camera and clears any proposal tag (`AC-5`).
    pub fn human_orbit(&mut self, yaw_rad: f64, pitch_rad: f64) {
        self.controller.orbit(yaw_rad, pitch_rad);
        self.on_human_camera_change();
    }

    /// Performs a human pan gesture on the live camera and clears any proposal tag (`REQ-005`).
    pub fn human_pan(&mut self, dx_px: f64, dy_px: f64) {
        let vp = self.doc_state.viewport;
        self.controller.pan(dx_px, dy_px, vp);
        self.on_human_camera_change();
    }

    /// Performs a human zoom gesture on the live camera and clears any proposal tag (`REQ-005`).
    pub fn human_zoom(&mut self, factor: f64, cursor_ndc: Option<[f64; 2]>) {
        let vp = self.doc_state.viewport;
        self.controller.zoom(factor, cursor_ndc, vp);
        self.on_human_camera_change();
    }

    /// Performs a human roll gesture on the live camera and clears any proposal tag (`REQ-005`).
    pub fn human_roll(&mut self, delta_rad: f64) {
        self.controller.roll(delta_rad);
        self.on_human_camera_change();
    }

    /// Returns `true` if `doc_state.visibility` explicitly hides `entity`.
    fn is_hidden_in_view_state(&self, entity: &EntityId) -> bool {
        self.doc_state.visibility.iter().any(|r| r.entity == *entity && !r.visible)
    }
}

/// Journal entry emitted by a successful `view.set_camera@1` (W2, `REQ-002`).
#[derive(Debug, Clone, PartialEq)]
pub struct SetCameraEvent {
    pub viewport: ViewportId,
    pub camera: Camera,
    pub authored_fov_deg: Option<String>,
    pub view_state_digest: Digest,
}

impl SetCameraEvent {
    /// Canonical JSON representation for journaling and replay.
    pub fn to_value(&self) -> Result<CValue, CameraError> {
        let mut o = BTreeMap::new();
        o.insert("type".to_string(), CValue::Str(SET_CAMERA_EVENT.to_string()));
        o.insert("viewport".to_string(), CValue::Str(self.viewport.as_str().to_string()));
        o.insert("camera".to_string(), camera_to_cvalue(&self.camera)?);
        o.insert(
            "authored_fov_deg".to_string(),
            self.authored_fov_deg.as_ref().map_or(CValue::Null, |s| CValue::Str(s.clone())),
        );
        o.insert("view_state_digest".to_string(), CValue::Str(self.view_state_digest.as_str().to_string()));
        Ok(CValue::Obj(o))
    }

    /// Strict deserialiser for exact replay (`AC-2`).
    pub fn from_value(v: &CValue) -> Result<SetCameraEvent, CameraError> {
        let CValue::Obj(o) = v else {
            return Err(CameraError::InvalidParam("event is not an object"));
        };
        let keys = ["type", "viewport", "camera", "authored_fov_deg", "view_state_digest"];
        if o.len() != keys.len() || o.keys().any(|k| !keys.contains(&k.as_str())) {
            return Err(CameraError::InvalidParam("unexpected event keys"));
        }
        match o.get("type") {
            Some(CValue::Str(s)) if s == SET_CAMERA_EVENT => {}
            _ => return Err(CameraError::InvalidParam("wrong event type")),
        }
        let viewport = match o.get("viewport") {
            Some(CValue::Str(s)) => ViewportId::new(s)?,
            _ => return Err(CameraError::InvalidParam("invalid viewport")),
        };
        let camera = match o.get("camera") {
            Some(cv) => camera_from_cvalue(cv)?,
            None => return Err(CameraError::InvalidParam("missing camera")),
        };
        let authored_fov_deg = match o.get("authored_fov_deg") {
            Some(CValue::Null) => None,
            Some(CValue::Str(s)) => Some(s.clone()),
            _ => return Err(CameraError::InvalidParam("invalid authored_fov_deg")),
        };
        let view_state_digest = match o.get("view_state_digest") {
            Some(CValue::Str(s)) => Digest::parse(s).map_err(|_| CameraError::InvalidParam("invalid digest"))?,
            _ => return Err(CameraError::InvalidParam("missing digest")),
        };
        Ok(SetCameraEvent { viewport, camera, authored_fov_deg, view_state_digest })
    }
}

/// Activity Log record emitted by `view.fly_to@1` ONLY while recording is enabled (`D-031`).
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityLogEntry {
    pub command: &'static str,
    pub viewport: ViewportId,
    pub entity: EntityId,
    pub principal: PrincipalKind,
    pub end_camera: Camera,
}

/// Multi-viewport workspace holding journaled `ViewState`s, live `CameraController`s,
/// document journal events (`view.set_camera@1`) and optional Activity Log (`view.fly_to@1`).
#[derive(Debug, Clone, Default)]
pub struct CameraWorkspace {
    viewports: BTreeMap<ViewportId, ViewportSlot>,
    journal: Vec<SetCameraEvent>,
    activity_log: Vec<ActivityLogEntry>,
    recording: bool,
}

impl CameraWorkspace {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces a viewport slot.
    pub fn insert_viewport(&mut self, id: ViewportId, slot: ViewportSlot) {
        self.viewports.insert(id, slot);
    }

    /// Borrows a viewport slot by ID.
    pub fn viewport(&self, id: &ViewportId) -> Option<&ViewportSlot> {
        self.viewports.get(id)
    }

    /// Mutably borrows a viewport slot by ID.
    pub fn viewport_mut(&mut self, id: &ViewportId) -> Option<&mut ViewportSlot> {
        self.viewports.get_mut(id)
    }

    /// Enables or disables D-031 Activity Log recording for R1 `view.fly_to@1` calls.
    pub fn set_recording(&mut self, recording: bool) {
        self.recording = recording;
    }

    /// Document journal entries emitted by `view.set_camera@1`.
    pub fn journal(&self) -> &[SetCameraEvent] {
        &self.journal
    }

    /// Journal head count (unchanged by R1 `view.fly_to@1`, `AC-4`).
    pub fn journal_len(&self) -> usize {
        self.journal.len()
    }

    /// Canonical journal JSON values.
    pub fn journal_values(&self) -> Result<Vec<CValue>, CameraError> {
        self.journal.iter().map(SetCameraEvent::to_value).collect()
    }

    /// Activity Log entries recorded for R1 `view.fly_to@1` while recording was on.
    pub fn activity_log(&self) -> &[ActivityLogEntry] {
        &self.activity_log
    }
}

// ------------------------------------------------------------------ camera JSON helpers

fn f64_cval(x: f64, field: &'static str) -> Result<CValue, CameraError> {
    if !x.is_finite() {
        return Err(CameraError::NonFiniteNumber { field });
    }
    let clean = if x == 0.0 { 0.0 } else { x };
    let text = fid_canon::format_number(clean).map_err(|_| CameraError::NonFiniteNumber { field })?;
    fid_canon::parse(text.as_bytes()).map_err(|_| CameraError::InvalidParam("cannot format number"))
}

fn vec3_to_cvalue(v: &[f64; 3], field: &'static str) -> Result<CValue, CameraError> {
    Ok(CValue::Arr(vec![f64_cval(v[0], field)?, f64_cval(v[1], field)?, f64_cval(v[2], field)?]))
}

fn cvalue_to_f64(v: &CValue, field: &'static str) -> Result<f64, CameraError> {
    let x = match v {
        CValue::Num(n) => *n,
        CValue::I64(n) => *n as f64,
        CValue::U64(n) => *n as f64,
        CValue::F64Bits(b) => *b,
        _ => return Err(CameraError::InvalidParam("expected finite number")),
    };
    if !x.is_finite() {
        return Err(CameraError::NonFiniteNumber { field });
    }
    Ok(x)
}

fn cvalue_to_vec3(v: &CValue, field: &'static str) -> Result<[f64; 3], CameraError> {
    let CValue::Arr(a) = v else {
        return Err(CameraError::InvalidParam("expected 3-element array"));
    };
    let [x, y, z] = a.as_slice() else {
        return Err(CameraError::InvalidParam("expected 3-element array"));
    };
    Ok([cvalue_to_f64(x, field)?, cvalue_to_f64(y, field)?, cvalue_to_f64(z, field)?])
}

/// Serialises a [`Camera`] in canonical RFC 8785 value form.
pub fn camera_to_cvalue(cam: &Camera) -> Result<CValue, CameraError> {
    let proj = match cam.projection {
        Projection::Perspective { fov_y_rad } => {
            let mut po = BTreeMap::new();
            po.insert("fov_y_rad".to_string(), f64_cval(fov_y_rad, "projection.fov_y_rad")?);
            po.insert("kind".to_string(), CValue::Str("perspective".to_string()));
            CValue::Obj(po)
        }
        Projection::Ortho { height_m } => {
            let mut po = BTreeMap::new();
            po.insert("height_m".to_string(), f64_cval(height_m, "projection.height_m")?);
            po.insert("kind".to_string(), CValue::Str("ortho".to_string()));
            CValue::Obj(po)
        }
    };
    let mut o = BTreeMap::new();
    o.insert("eye_m".to_string(), vec3_to_cvalue(&cam.eye_m, "eye_m")?);
    o.insert("far_m".to_string(), f64_cval(cam.far_m, "far_m")?);
    o.insert("near_m".to_string(), f64_cval(cam.near_m, "near_m")?);
    o.insert("projection".to_string(), proj);
    o.insert("target_m".to_string(), vec3_to_cvalue(&cam.target_m, "target_m")?);
    o.insert("up".to_string(), vec3_to_cvalue(&cam.up, "up")?);
    Ok(CValue::Obj(o))
}

/// Deserialises a [`Camera`] from canonical JSON value form.
pub fn camera_from_cvalue(v: &CValue) -> Result<Camera, CameraError> {
    let CValue::Obj(o) = v else {
        return Err(CameraError::InvalidParam("camera must be an object"));
    };
    let eye_m = cvalue_to_vec3(o.get("eye_m").ok_or(CameraError::InvalidParam("missing eye_m"))?, "eye_m")?;
    let target_m = cvalue_to_vec3(o.get("target_m").ok_or(CameraError::InvalidParam("missing target_m"))?, "target_m")?;
    let up = cvalue_to_vec3(o.get("up").ok_or(CameraError::InvalidParam("missing up"))?, "up")?;
    let near_m = cvalue_to_f64(o.get("near_m").ok_or(CameraError::InvalidParam("missing near_m"))?, "near_m")?;
    let far_m = cvalue_to_f64(o.get("far_m").ok_or(CameraError::InvalidParam("missing far_m"))?, "far_m")?;
    let Some(CValue::Obj(po)) = o.get("projection") else {
        return Err(CameraError::InvalidParam("missing projection"));
    };
    let projection = match po.get("kind") {
        Some(CValue::Str(k)) if k == "perspective" => {
            let fov = cvalue_to_f64(
                po.get("fov_y_rad").ok_or(CameraError::InvalidParam("missing fov_y_rad"))?,
                "projection.fov_y_rad",
            )?;
            Projection::Perspective { fov_y_rad: fov }
        }
        Some(CValue::Str(k)) if k == "ortho" => {
            let h = cvalue_to_f64(
                po.get("height_m").ok_or(CameraError::InvalidParam("missing height_m"))?,
                "projection.height_m",
            )?;
            Projection::Ortho { height_m: h }
        }
        _ => return Err(CameraError::InvalidParam("unknown projection kind")),
    };
    Ok(Camera { eye_m, target_m, up, projection, near_m, far_m })
}

// ------------------------------------------------------------------ validation

fn check_vec3_finite(v: [f64; 3], field: &'static str) -> Result<(), CameraError> {
    if v[0].is_finite() && v[1].is_finite() && v[2].is_finite() {
        Ok(())
    } else {
        Err(CameraError::NonFiniteNumber { field })
    }
}

fn validate_camera_inputs(cam: &Camera) -> Result<(), CameraError> {
    check_vec3_finite(cam.eye_m, "eye_m")?;
    check_vec3_finite(cam.target_m, "target_m")?;
    check_vec3_finite(cam.up, "up")?;
    if !cam.near_m.is_finite() {
        return Err(CameraError::NonFiniteNumber { field: "near_m" });
    }
    if !cam.far_m.is_finite() {
        return Err(CameraError::NonFiniteNumber { field: "far_m" });
    }
    match cam.projection {
        Projection::Perspective { fov_y_rad } => {
            if !fov_y_rad.is_finite() {
                return Err(CameraError::NonFiniteNumber { field: "projection.fov_y_rad" });
            }
            if fov_y_rad <= 0.0 || fov_y_rad >= std::f64::consts::PI {
                return Err(CameraError::DegenerateCamera {
                    field: "projection.fov_y_rad",
                    reason: "fov_y_rad outside (0, pi)",
                });
            }
            if cam.near_m <= 0.0 {
                return Err(CameraError::DegenerateCamera {
                    field: "near_m",
                    reason: "perspective near_m must be positive",
                });
            }
        }
        Projection::Ortho { height_m } => {
            if !height_m.is_finite() {
                return Err(CameraError::NonFiniteNumber { field: "projection.height_m" });
            }
            if height_m <= 0.0 {
                return Err(CameraError::DegenerateCamera {
                    field: "projection.height_m",
                    reason: "ortho height_m must be positive",
                });
            }
        }
    }
    if cam.near_m >= cam.far_m {
        return Err(CameraError::DegenerateCamera { field: "far_m", reason: "near_m must be below far_m" });
    }

    let d = [cam.target_m[0] - cam.eye_m[0], cam.target_m[1] - cam.eye_m[1], cam.target_m[2] - cam.eye_m[2]];
    if !(d[0].is_finite() && d[1].is_finite() && d[2].is_finite()) {
        return Err(CameraError::DegenerateCamera { field: "target_m", reason: "view direction overflows" });
    }
    let dm = d[0].abs().max(d[1].abs()).max(d[2].abs());
    if dm == 0.0 {
        return Err(CameraError::DegenerateCamera { field: "target_m", reason: "eye equals target" });
    }
    let um = cam.up[0].abs().max(cam.up[1].abs()).max(cam.up[2].abs());
    if um == 0.0 {
        return Err(CameraError::DegenerateCamera { field: "up", reason: "zero up vector" });
    }
    let ds = [d[0] / dm, d[1] / dm, d[2] / dm];
    let us = [cam.up[0] / um, cam.up[1] / um, cam.up[2] / um];
    let cx = us[1] * ds[2] - us[2] * ds[1];
    let cy = us[2] * ds[0] - us[0] * ds[2];
    let cz = us[0] * ds[1] - us[1] * ds[0];
    let c2 = cx * cx + cy * cy + cz * cz;
    let d2 = ds[0] * ds[0] + ds[1] * ds[1] + ds[2] * ds[2];
    let u2 = us[0] * us[0] + us[1] * us[1] + us[2] * us[2];
    let tol2 = crate::state::PARALLEL_TOLERANCE * crate::state::PARALLEL_TOLERANCE;
    if c2 <= tol2 * u2 * d2 {
        return Err(CameraError::DegenerateCamera { field: "up", reason: "up parallel to view direction" });
    }
    Ok(())
}

/// Converts an authored degree literal (e.g. `"45"`) to radians once in `f64`.
pub fn parse_fov_deg_to_rad(raw: &str) -> Result<f64, CameraError> {
    let deg = raw.trim().parse::<f64>().map_err(|_| CameraError::InvalidParam("invalid fov_deg number"))?;
    if !deg.is_finite() {
        return Err(CameraError::NonFiniteNumber { field: "projection.fov_y_deg" });
    }
    let rad = deg * DEG_TO_RAD;
    if !rad.is_finite() {
        return Err(CameraError::NonFiniteNumber { field: "projection.fov_y_rad" });
    }
    Ok(rad)
}

// ------------------------------------------------------------------ command view.set_camera@1 (W2)

/// Execution context for `view.set_camera@1` and `view.fly_to@1`.
pub struct CameraCmdCtx<'a> {
    pub workspace: &'a mut CameraWorkspace,
    pub scene: &'a dyn SceneBounds,
    pub principal: PrincipalKind,
}

/// Wire projection specification for `view.set_camera@1`.
#[derive(FidSchema, Debug, Clone, PartialEq)]
pub struct ProjectionParam {
    #[fid(description = "Projection kind: perspective or ortho.")]
    pub kind: String,
    #[fid(description = "Perspective vertical field of view in radians, (0, pi).", quantity = "angle", unit = "rad")]
    pub fov_y_rad: Option<f64>,
    #[fid(
        description = "Optional authored perspective vertical field of view in degrees (converted once to radians)."
    )]
    pub fov_y_deg: Option<String>,
    #[fid(description = "Orthographic view volume height in metres (> 0).", quantity = "length", unit = "m")]
    pub height_m: Option<f64>,
}

/// Parameters of `view.set_camera@1` (W2).
#[derive(FidSchema, Debug, Clone, PartialEq)]
pub struct SetCameraParams {
    #[fid(description = "Target viewport identifier (e.g. main).", pattern = VIEWPORT_PATTERN)]
    pub viewport: String,
    #[fid(
        description = "Camera eye position [x, y, z] in metres (f64).",
        max_items = 3,
        quantity = "length",
        unit = "m"
    )]
    pub eye_m: Vec<f64>,
    #[fid(
        description = "Camera look-at target [x, y, z] in metres (f64).",
        max_items = 3,
        quantity = "length",
        unit = "m"
    )]
    pub target_m: Vec<f64>,
    #[fid(
        description = "Camera up direction [x, y, z] in world metres (non-zero, not parallel to target - eye).",
        max_items = 3,
        quantity = "length",
        unit = "m"
    )]
    pub up: Vec<f64>,
    #[fid(description = "Camera projection (perspective or ortho).")]
    pub projection: ProjectionParam,
    #[fid(
        description = "Optional near clipping distance in metres (defaults to current viewport near_m).",
        quantity = "length",
        unit = "m"
    )]
    pub near_m: Option<f64>,
    #[fid(
        description = "Optional far clipping distance in metres (defaults to current viewport far_m).",
        quantity = "length",
        unit = "m"
    )]
    pub far_m: Option<f64>,
}

impl SetCameraParams {
    /// Strict `--json` / MCP parameter parser.
    pub fn from_value(v: &CValue) -> Result<SetCameraParams, CameraError> {
        let CValue::Obj(o) = v else {
            return Err(CameraError::InvalidParam("params must be an object"));
        };
        let allowed = ["viewport", "eye_m", "target_m", "up", "projection", "near_m", "far_m"];
        if o.keys().any(|k| !allowed.contains(&k.as_str())) {
            return Err(CameraError::InvalidParam("unknown parameter"));
        }
        let viewport = match o.get("viewport") {
            Some(CValue::Str(s)) => s.clone(),
            _ => return Err(CameraError::InvalidParam("viewport must be a string")),
        };
        let eye_m =
            cvalue_to_vec3(o.get("eye_m").ok_or(CameraError::InvalidParam("missing eye_m"))?, "eye_m")?.to_vec();
        let target_m =
            cvalue_to_vec3(o.get("target_m").ok_or(CameraError::InvalidParam("missing target_m"))?, "target_m")?
                .to_vec();
        let up = cvalue_to_vec3(o.get("up").ok_or(CameraError::InvalidParam("missing up"))?, "up")?.to_vec();
        let Some(CValue::Obj(po)) = o.get("projection") else {
            return Err(CameraError::InvalidParam("projection must be an object"));
        };
        let p_allowed = ["kind", "fov_y_rad", "fov_y_deg", "height_m"];
        if po.keys().any(|k| !p_allowed.contains(&k.as_str())) {
            return Err(CameraError::InvalidParam("unknown projection key"));
        }
        let kind = match po.get("kind") {
            Some(CValue::Str(s)) => s.clone(),
            _ => return Err(CameraError::InvalidParam("projection.kind must be a string")),
        };
        let fov_y_rad = match po.get("fov_y_rad") {
            None | Some(CValue::Null) => None,
            Some(x) => Some(cvalue_to_f64(x, "projection.fov_y_rad")?),
        };
        let fov_y_deg = match po.get("fov_y_deg") {
            None | Some(CValue::Null) => None,
            Some(CValue::Str(s)) => Some(s.clone()),
            Some(x) => {
                let n = cvalue_to_f64(x, "projection.fov_y_deg")?;
                Some(
                    fid_canon::format_number(n)
                        .map_err(|_| CameraError::NonFiniteNumber { field: "projection.fov_y_deg" })?,
                )
            }
        };
        let height_m = match po.get("height_m") {
            None | Some(CValue::Null) => None,
            Some(x) => Some(cvalue_to_f64(x, "projection.height_m")?),
        };
        let near_m = match o.get("near_m") {
            None | Some(CValue::Null) => None,
            Some(x) => Some(cvalue_to_f64(x, "near_m")?),
        };
        let far_m = match o.get("far_m") {
            None | Some(CValue::Null) => None,
            Some(x) => Some(cvalue_to_f64(x, "far_m")?),
        };
        Ok(SetCameraParams {
            viewport,
            eye_m,
            target_m,
            up,
            projection: ProjectionParam { kind, fov_y_rad, fov_y_deg, height_m },
            near_m,
            far_m,
        })
    }
}

/// Result of `view.set_camera@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct SetCameraResult {
    #[fid(description = "SHA-256 digest of the updated canonical ViewState.")]
    pub view_state_digest: String,
}

impl SetCameraResult {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("view_state_digest".to_string(), CValue::Str(self.view_state_digest.clone()));
        CValue::Obj(o)
    }
}

fn slice_to_arr3(v: &[f64], field: &'static str) -> Result<[f64; 3], CameraError> {
    let [x, y, z] = *v else {
        return Err(CameraError::InvalidParam("vector must have 3 coordinates"));
    };
    check_vec3_finite([x, y, z], field)?;
    Ok([x, y, z])
}

#[fid_command(
    id = "view.set_camera",
    major = 1,
    title = "Set viewport camera",
    description = "Writes exact f64 eye, target, up and projection into a viewport's ViewState and journals the resulting ViewState digest.",
    side_effect = W2,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = HeadMove,
    preconditions = [],
    effects = [SET_CAMERA_EVENT],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-22"),
    web = Available,
)]
pub fn set_camera(ctx: &mut CameraCmdCtx<'_>, params: SetCameraParams) -> Result<SetCameraResult, CameraError> {
    let vp_id = ViewportId::new(&params.viewport)?;
    let eye_m = slice_to_arr3(&params.eye_m, "eye_m")?;
    let target_m = slice_to_arr3(&params.target_m, "target_m")?;
    let up = slice_to_arr3(&params.up, "up")?;

    let (projection, authored_fov_deg) = match params.projection.kind.as_str() {
        "perspective" => match (params.projection.fov_y_rad, &params.projection.fov_y_deg) {
            (Some(rad), None) => (Projection::Perspective { fov_y_rad: rad }, None),
            (None, Some(deg_s)) => {
                (Projection::Perspective { fov_y_rad: parse_fov_deg_to_rad(deg_s)? }, Some(deg_s.clone()))
            }
            (Some(_), Some(_)) => return Err(CameraError::InvalidParam("specify only one of fov_y_rad or fov_y_deg")),
            (None, None) => {
                return Err(CameraError::InvalidParam("perspective projection requires fov_y_rad or fov_y_deg"))
            }
        },
        "ortho" => {
            let h =
                params.projection.height_m.ok_or(CameraError::InvalidParam("ortho projection requires height_m"))?;
            (Projection::Ortho { height_m: h }, None)
        }
        _ => return Err(CameraError::InvalidParam("projection.kind must be perspective or ortho")),
    };

    let Some(slot) = ctx.workspace.viewport(&vp_id) else {
        return Err(CameraError::UnknownViewport(vp_id.as_str().to_string()));
    };
    let near_m = params.near_m.unwrap_or(slot.doc_state.camera.near_m);
    let far_m = params.far_m.unwrap_or(slot.doc_state.camera.far_m);
    let new_cam = Camera { eye_m, target_m, up, projection, near_m, far_m };
    validate_camera_inputs(&new_cam)?;

    // Build and validate a candidate ViewState before mutating anything (REQ-003).
    let mut candidate = slot.doc_state.clone();
    candidate.camera = new_cam;
    candidate.validate()?;
    let digest = candidate.digest()?;

    // Commit to viewport slot and journal one event (REQ-001, REQ-002).
    let Some(slot_mut) = ctx.workspace.viewport_mut(&vp_id) else {
        return Err(CameraError::UnknownViewport(vp_id.as_str().to_string()));
    };
    slot_mut.doc_state = candidate;
    slot_mut.controller.live = new_cam;
    slot_mut.controller.pivot_m = new_cam.target_m;
    slot_mut.controller.cancel_animation();
    if ctx.principal == PrincipalKind::Human {
        slot_mut.on_human_camera_change();
    }

    ctx.workspace.journal.push(SetCameraEvent {
        viewport: vp_id,
        camera: new_cam,
        authored_fov_deg,
        view_state_digest: digest,
    });

    Ok(SetCameraResult { view_state_digest: digest.as_str().to_string() })
}

// ------------------------------------------------------------------ command view.fly_to@1 (R1)

/// Parameters of `view.fly_to@1` (R1).
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct FlyToParams {
    #[fid(description = "Target viewport identifier (e.g. main).", pattern = VIEWPORT_PATTERN)]
    pub viewport: String,
    #[fid(description = "Target entity UUID to frame in the viewport.", pattern = UUID_PATTERN)]
    pub entity: String,
}

impl FlyToParams {
    /// Strict `--json` / MCP parameter parser.
    pub fn from_value(v: &CValue) -> Result<FlyToParams, CameraError> {
        let CValue::Obj(o) = v else {
            return Err(CameraError::InvalidParam("params must be an object"));
        };
        if o.keys().any(|k| !matches!(k.as_str(), "viewport" | "entity")) {
            return Err(CameraError::InvalidParam("unknown parameter"));
        }
        let viewport = match o.get("viewport") {
            None | Some(CValue::Null) => "main".to_string(),
            Some(CValue::Str(s)) => s.clone(),
            Some(_) => return Err(CameraError::InvalidParam("viewport must be a string")),
        };
        let entity = match o.get("entity") {
            Some(CValue::Str(s)) => s.clone(),
            _ => return Err(CameraError::InvalidParam("entity must be a uuid string")),
        };
        Ok(FlyToParams { viewport, entity })
    }
}

/// Wire camera value returned by `view.fly_to@1`.
#[derive(FidSchema, Debug, Clone, PartialEq)]
pub struct CameraOut {
    #[fid(description = "Eye position [x, y, z] in metres.", max_items = 3, quantity = "length", unit = "m")]
    pub eye_m: Vec<f64>,
    #[fid(description = "Target position [x, y, z] in metres.", max_items = 3, quantity = "length", unit = "m")]
    pub target_m: Vec<f64>,
    #[fid(description = "Up direction [x, y, z] in world metres.", max_items = 3, quantity = "length", unit = "m")]
    pub up: Vec<f64>,
    #[fid(description = "Projection kind (perspective or ortho).")]
    pub projection_kind: String,
    #[fid(
        description = "Perspective vertical field of view in radians (when perspective).",
        quantity = "angle",
        unit = "rad"
    )]
    pub fov_y_rad: Option<f64>,
    #[fid(description = "Orthographic height in metres (when ortho).", quantity = "length", unit = "m")]
    pub height_m: Option<f64>,
    #[fid(description = "Near clipping distance in metres.", quantity = "length", unit = "m")]
    pub near_m: f64,
    #[fid(description = "Far clipping distance in metres.", quantity = "length", unit = "m")]
    pub far_m: f64,
}

impl CameraOut {
    pub fn from_camera(cam: Camera) -> Self {
        let (projection_kind, fov_y_rad, height_m) = match cam.projection {
            Projection::Perspective { fov_y_rad } => ("perspective".to_string(), Some(fov_y_rad), None),
            Projection::Ortho { height_m } => ("ortho".to_string(), None, Some(height_m)),
        };
        CameraOut {
            eye_m: cam.eye_m.to_vec(),
            target_m: cam.target_m.to_vec(),
            up: cam.up.to_vec(),
            projection_kind,
            fov_y_rad,
            height_m,
            near_m: cam.near_m,
            far_m: cam.far_m,
        }
    }

    pub fn to_camera(&self) -> Result<Camera, CameraError> {
        let eye_m = slice_to_arr3(&self.eye_m, "eye_m")?;
        let target_m = slice_to_arr3(&self.target_m, "target_m")?;
        let up = slice_to_arr3(&self.up, "up")?;
        let projection = match self.projection_kind.as_str() {
            "perspective" => Projection::Perspective {
                fov_y_rad: self.fov_y_rad.ok_or(CameraError::InvalidParam("missing fov_y_rad"))?,
            },
            "ortho" => {
                Projection::Ortho { height_m: self.height_m.ok_or(CameraError::InvalidParam("missing height_m"))? }
            }
            _ => return Err(CameraError::InvalidParam("unknown projection kind")),
        };
        Ok(Camera { eye_m, target_m, up, projection, near_m: self.near_m, far_m: self.far_m })
    }
}

/// Result of `view.fly_to@1` (R1).
#[derive(FidSchema, Debug, Clone, PartialEq)]
pub struct FlyToResult {
    #[fid(description = "Deterministic end camera computed from the target entity bounds.")]
    pub camera: CameraOut,
    #[fid(description = "Present (`\"proposal\"`) when called by an agent principal.")]
    pub proposal_tag: Option<String>,
    #[fid(description = "True when reduced-motion mode jumped straight to the end camera without animation.")]
    pub reduced_motion_jump: bool,
}

impl FlyToResult {
    pub fn to_value(&self) -> Result<CValue, CameraError> {
        let cam = self.camera.to_camera()?;
        let mut o = BTreeMap::new();
        o.insert("camera".to_string(), camera_to_cvalue(&cam)?);
        o.insert(
            "proposal_tag".to_string(),
            self.proposal_tag.as_ref().map_or(CValue::Null, |s| CValue::Str(s.clone())),
        );
        o.insert("reduced_motion_jump".to_string(), CValue::Bool(self.reduced_motion_jump));
        Ok(CValue::Obj(o))
    }
}

#[fid_command(
    id = "view.fly_to",
    major = 1,
    title = "Fly viewport camera to entity",
    description = "Frames an entity's f64 bounds in the live viewport camera using the NAV fit rule without changing document state; shows a proposal tag when called by an agent.",
    side_effect = R1,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = [],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-22"),
    web = Available,
)]
pub fn fly_to(ctx: &mut CameraCmdCtx<'_>, params: FlyToParams) -> Result<FlyToResult, CameraError> {
    let vp_id = ViewportId::new(&params.viewport)?;
    let entity = EntityId::parse(&params.entity).map_err(|_| CameraError::InvalidParam("invalid entity uuid"))?;

    let (viewport_size, live_cam, reduced_motion, hidden_in_vs) = {
        let Some(slot) = ctx.workspace.viewport(&vp_id) else {
            return Err(CameraError::UnknownViewport(vp_id.as_str().to_string()));
        };
        (slot.doc_state.viewport, slot.controller.live, slot.reduced_motion, slot.is_hidden_in_view_state(&entity))
    };

    let bounds = match ctx.scene.lookup_entity(&entity) {
        EntityBoundsLookup::Unknown => return Err(CameraError::UnknownEntity(entity)),
        EntityBoundsLookup::Hidden => return Err(CameraError::EntityNotDisplayed(entity)),
        EntityBoundsLookup::Displayed(_) if hidden_in_vs => return Err(CameraError::EntityNotDisplayed(entity)),
        EntityBoundsLookup::Displayed(b) => b,
    };

    // Deterministic f64 framing using the NAV fit rule from the current view direction (REQ-004).
    let end_cam = fit_camera(&live_cam, bounds, viewport_size);

    // Apply ONLY to the live presentation camera; document ViewState and journal are untouched.
    let Some(slot_mut) = ctx.workspace.viewport_mut(&vp_id) else {
        return Err(CameraError::UnknownViewport(vp_id.as_str().to_string()));
    };
    let returned = slot_mut.controller.fit_bounds(bounds, viewport_size, reduced_motion);
    if reduced_motion {
        // REQ-006: jump immediately with no animation frames.
        slot_mut.controller.live = returned;
        slot_mut.controller.anim = None;
    }
    let proposal_tag = if ctx.principal == PrincipalKind::Agent {
        slot_mut.proposal_tag = Some(ProposalTag::agent_proposal(entity));
        Some("proposal".to_string())
    } else {
        if ctx.principal == PrincipalKind::Human {
            slot_mut.on_human_camera_change();
        }
        None
    };

    // D-031: R1 command emits an Activity Log record ONLY during active recording.
    if ctx.workspace.recording {
        ctx.workspace.activity_log.push(ActivityLogEntry {
            command: FLY_TO_EVENT,
            viewport: vp_id,
            entity,
            principal: ctx.principal,
            end_camera: end_cam,
        });
    }

    Ok(FlyToResult { camera: CameraOut::from_camera(end_cam), proposal_tag, reduced_motion_jump: reduced_motion })
}

fid_commands![set_camera, fly_to];

/// Registry command definitions exported by `VIZ.VIEW.CAMERA.SET`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(declarations().into_iter().map(|d| d.def).collect())
}

// ------------------------------------------------------------------ replay (REQ-002, AC-2)

/// Outcome of replaying `view.set_camera@1` journal events in exact mode.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraReplayRun {
    pub outcome: ReplayOutcome,
    pub replayed_events: usize,
    pub final_digests: BTreeMap<ViewportId, Digest>,
}

/// Replays serialized `view.set_camera@1` journal events against `initial_viewports` and
/// compares every step's resulting [`ViewState`] digest against the journaled digest (`AC-2`).
pub fn replay_camera_journal(
    events: &[CValue],
    initial_viewports: &BTreeMap<ViewportId, ViewState>,
) -> Result<CameraReplayRun, CameraError> {
    let mut states = initial_viewports.clone();
    for ev_val in events {
        let ev = SetCameraEvent::from_value(ev_val)?;
        let Some(vs) = states.get_mut(&ev.viewport) else {
            return Ok(CameraReplayRun {
                outcome: ReplayOutcome::NotReplayable,
                replayed_events: 0,
                final_digests: BTreeMap::new(),
            });
        };
        vs.camera = ev.camera;
        if vs.validate().is_err() {
            return Ok(CameraReplayRun {
                outcome: ReplayOutcome::Divergent,
                replayed_events: 0,
                final_digests: BTreeMap::new(),
            });
        }
        let digest = vs.digest()?;
        if digest != ev.view_state_digest {
            return Ok(CameraReplayRun {
                outcome: ReplayOutcome::Divergent,
                replayed_events: 0,
                final_digests: BTreeMap::new(),
            });
        }
    }
    let mut final_digests = BTreeMap::new();
    for (id, vs) in &states {
        final_digests.insert(id.clone(), vs.digest()?);
    }
    Ok(CameraReplayRun { outcome: ReplayOutcome::Identical, replayed_events: events.len(), final_digests })
}

// ------------------------------------------------------------------ JSON & CLI dispatchers

/// Executes `view.set_camera@1` from canonical JSON parameters (`--json` / MCP).
pub fn execute_set_camera(ctx: &mut CameraCmdCtx<'_>, params: &CValue) -> Result<CValue, CameraError> {
    let parsed = SetCameraParams::from_value(params)?;
    let res = set_camera(ctx, parsed)?;
    Ok(res.to_value())
}

/// Executes `view.fly_to@1` from canonical JSON parameters (`--json` / MCP).
pub fn execute_fly_to(ctx: &mut CameraCmdCtx<'_>, params: &CValue) -> Result<CValue, CameraError> {
    let parsed = FlyToParams::from_value(params)?;
    let res = fly_to(ctx, parsed)?;
    res.to_value()
}

fn parse_csv_vec3(s: &str, field: &'static str) -> Result<Vec<f64>, CameraError> {
    let parts: Vec<&str> = s.split(',').collect();
    let [sx, sy, sz] = parts.as_slice() else {
        return Err(CameraError::InvalidParam("expected x,y,z triple"));
    };
    let parse_one = |tok: &str| -> Result<f64, CameraError> {
        let v = tok.trim().parse::<f64>().map_err(|_| CameraError::InvalidParam("invalid float in triple"))?;
        if !v.is_finite() {
            return Err(CameraError::NonFiniteNumber { field });
        }
        Ok(v)
    };
    Ok(vec![parse_one(sx)?, parse_one(sy)?, parse_one(sz)?])
}

/// CLI entry point for `fid view set-camera --viewport <vp> --eye x,y,z --target x,y,z --up x,y,z (--perspective-fov-deg <d> | --perspective-fov-rad <r> | --ortho-height <h>) --json`.
pub fn invoke_cli_set_camera(ctx: &mut CameraCmdCtx<'_>, args: &[&str]) -> Result<CValue, CameraError> {
    let mut viewport: Option<String> = None;
    let mut eye: Option<Vec<f64>> = None;
    let mut target: Option<Vec<f64>> = None;
    let mut up: Option<Vec<f64>> = None;
    let mut fov_deg: Option<String> = None;
    let mut fov_rad: Option<f64> = None;
    let mut ortho_h: Option<f64> = None;
    let mut near_m: Option<f64> = None;
    let mut far_m: Option<f64> = None;
    let mut json_flag = false;

    let mut i = 0usize;
    while i < args.len() {
        let Some(&flag) = args.get(i) else {
            break;
        };
        match flag {
            "--json" => {
                json_flag = true;
                i += 1;
            }
            "--viewport" => {
                viewport = args.get(i + 1).map(|s| (*s).to_string());
                i += 2;
            }
            "--eye" => {
                let s = args.get(i + 1).ok_or(CameraError::InvalidParam("missing --eye value"))?;
                eye = Some(parse_csv_vec3(s, "eye_m")?);
                i += 2;
            }
            "--target" => {
                let s = args.get(i + 1).ok_or(CameraError::InvalidParam("missing --target value"))?;
                target = Some(parse_csv_vec3(s, "target_m")?);
                i += 2;
            }
            "--up" => {
                let s = args.get(i + 1).ok_or(CameraError::InvalidParam("missing --up value"))?;
                up = Some(parse_csv_vec3(s, "up")?);
                i += 2;
            }
            "--perspective-fov-deg" => {
                let s = args.get(i + 1).ok_or(CameraError::InvalidParam("missing --perspective-fov-deg value"))?;
                fov_deg = Some((*s).to_string());
                i += 2;
            }
            "--perspective-fov-rad" => {
                let s = args.get(i + 1).ok_or(CameraError::InvalidParam("missing --perspective-fov-rad value"))?;
                let v = s.parse::<f64>().map_err(|_| CameraError::InvalidParam("invalid fov_rad"))?;
                if !v.is_finite() {
                    return Err(CameraError::NonFiniteNumber { field: "projection.fov_y_rad" });
                }
                fov_rad = Some(v);
                i += 2;
            }
            "--ortho-height" => {
                let s = args.get(i + 1).ok_or(CameraError::InvalidParam("missing --ortho-height value"))?;
                let v = s.parse::<f64>().map_err(|_| CameraError::InvalidParam("invalid ortho height"))?;
                if !v.is_finite() {
                    return Err(CameraError::NonFiniteNumber { field: "projection.height_m" });
                }
                ortho_h = Some(v);
                i += 2;
            }
            "--near" => {
                let s = args.get(i + 1).ok_or(CameraError::InvalidParam("missing --near value"))?;
                let v = s.parse::<f64>().map_err(|_| CameraError::InvalidParam("invalid near"))?;
                if !v.is_finite() {
                    return Err(CameraError::NonFiniteNumber { field: "near_m" });
                }
                near_m = Some(v);
                i += 2;
            }
            "--far" => {
                let s = args.get(i + 1).ok_or(CameraError::InvalidParam("missing --far value"))?;
                let v = s.parse::<f64>().map_err(|_| CameraError::InvalidParam("invalid far"))?;
                if !v.is_finite() {
                    return Err(CameraError::NonFiniteNumber { field: "far_m" });
                }
                far_m = Some(v);
                i += 2;
            }
            _ => return Err(CameraError::InvalidParam("unknown cli flag")),
        }
    }
    if !json_flag {
        return Err(CameraError::InvalidParam("--json flag required"));
    }
    let projection = match (fov_deg, fov_rad, ortho_h) {
        (Some(d), None, None) => {
            ProjectionParam { kind: "perspective".to_string(), fov_y_rad: None, fov_y_deg: Some(d), height_m: None }
        }
        (None, Some(r), None) => {
            ProjectionParam { kind: "perspective".to_string(), fov_y_rad: Some(r), fov_y_deg: None, height_m: None }
        }
        (None, None, Some(h)) => {
            ProjectionParam { kind: "ortho".to_string(), fov_y_rad: None, fov_y_deg: None, height_m: Some(h) }
        }
        _ => return Err(CameraError::InvalidParam("specify exactly one projection flag")),
    };
    let params = SetCameraParams {
        viewport: viewport.unwrap_or_else(|| "main".to_string()),
        eye_m: eye.ok_or(CameraError::InvalidParam("missing --eye"))?,
        target_m: target.ok_or(CameraError::InvalidParam("missing --target"))?,
        up: up.ok_or(CameraError::InvalidParam("missing --up"))?,
        projection,
        near_m,
        far_m,
    };
    let res = set_camera(ctx, params)?;
    Ok(res.to_value())
}

/// CLI entry point for `fid view fly-to [--viewport <vp>] --entity <uuid> --json`.
pub fn invoke_cli_fly_to(ctx: &mut CameraCmdCtx<'_>, args: &[&str]) -> Result<CValue, CameraError> {
    let mut viewport: Option<String> = None;
    let mut entity: Option<String> = None;
    let mut json_flag = false;

    let mut i = 0usize;
    while i < args.len() {
        let Some(&flag) = args.get(i) else {
            break;
        };
        match flag {
            "--json" => {
                json_flag = true;
                i += 1;
            }
            "--viewport" => {
                viewport = args.get(i + 1).map(|s| (*s).to_string());
                i += 2;
            }
            "--entity" => {
                entity = args.get(i + 1).map(|s| (*s).to_string());
                i += 2;
            }
            _ => return Err(CameraError::InvalidParam("unknown cli flag")),
        }
    }
    if !json_flag {
        return Err(CameraError::InvalidParam("--json flag required"));
    }
    let params = FlyToParams {
        viewport: viewport.unwrap_or_else(|| "main".to_string()),
        entity: entity.ok_or(CameraError::InvalidParam("missing --entity"))?,
    };
    let res = fly_to(ctx, params)?;
    res.to_value()
}

/// Helper to build a golden-cube `ViewState` with sensible defaults for a `width_px x height_px` viewport.
pub fn default_view_state(width_px: u32, height_px: u32) -> ViewState {
    ViewState {
        schema: crate::state::SCHEMA_VERSION,
        camera: Camera {
            eye_m: [2.0, -3.0, 2.0],
            target_m: [0.0, 0.0, 0.0],
            up: [0.0, 0.0, 1.0],
            projection: Projection::Perspective { fov_y_rad: 45.0 * DEG_TO_RAD },
            near_m: 0.01,
            far_m: 100.0,
        },
        viewport: Viewport { width_px, height_px },
        clips: Vec::new(),
        visibility: Vec::new(),
        transparency: Vec::new(),
        colormap: None,
        legend: None,
        labels: None,
    }
}

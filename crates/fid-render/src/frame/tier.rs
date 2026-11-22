//! Capability tiers and render-info snapshots (spec leaf `VIZ.RENDER.FRAME.TIER`).
//!
//! Classifies a renderer device into [`CapabilityTier`] (`T0`..`T3`) from adapter features and
//! limits (REQ-001), records the backend, adapter name, driver info, [`PointRasterPath`] and
//! [`TransparencyPath`] (REQ-002), and produces a deterministic RFC 8785 canonical JSON
//! [`RenderInfo`] snapshot matching `schemas/render-info-1.json` (REQ-003) without mutating any
//! document, view or render state (REQ-004).
//!
//! Determinism D1: depends on the adapter and last rendered frame; identical calls between
//! frames yield byte-identical canonical JSON.

#![deny(clippy::todo, clippy::unimplemented, clippy::unreachable)]

use crate::frame::graph::Backend;
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Interactive frame-time budget in milliseconds (60 fps baseline).
pub const DEFAULT_INTERACTIVE_FRAME_BUDGET_MS: u64 = 16;
/// Headless server still frame-time budget in milliseconds (T3).
pub const DEFAULT_HEADLESS_FRAME_BUDGET_MS: u64 = 1000;
/// Default GPU memory budget cap in bytes (512 MiB).
pub const DEFAULT_MEM_BUDGET_BYTES: u64 = 512 * 1024 * 1024;
/// Largest accepted JSON payload parsed by this module, in bytes.
pub const MAX_JSON_BYTES: usize = 1024 * 1024;
/// Deepest accepted JSON nesting parsed by this module.
pub const MAX_JSON_DEPTH: usize = 16;
/// Largest exact integer representable in IEEE 754 binary64 (2^53 - 1).
const MAX_SAFE_INT: u64 = (1u64 << 53) - 1;

const RENDER_INFO_KEYS: [&str; 10] = [
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
];

// ------------------------------------------------------------------ core enums

/// Renderer capability tier (REQ-VIZ.RENDER.FRAME.TIER-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapabilityTier {
    /// Native adapter with compute shaders and 64-bit storage atomics.
    T0,
    /// WebGPU-class adapter with compute shaders and no 64-bit storage atomics.
    T1,
    /// WebGL2 / GL-class adapter lacking compute shaders.
    T2,
    /// Headless renderer for server-side still frames.
    T3,
}

impl CapabilityTier {
    pub const ALL: [CapabilityTier; 4] =
        [CapabilityTier::T0, CapabilityTier::T1, CapabilityTier::T2, CapabilityTier::T3];

    pub fn as_str(self) -> &'static str {
        match self {
            CapabilityTier::T0 => "T0",
            CapabilityTier::T1 => "T1",
            CapabilityTier::T2 => "T2",
            CapabilityTier::T3 => "T3",
        }
    }

    pub fn parse(s: &str) -> Option<CapabilityTier> {
        CapabilityTier::ALL.into_iter().find(|t| t.as_str() == s)
    }
}

impl fmt::Display for CapabilityTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Active point-cloud rasterisation path (REQ-VIZ.RENDER.FRAME.TIER-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PointRasterPath {
    /// Single-pass 64-bit depth+index atomic min compute raster (T0).
    Compute64,
    /// Two-pass 32-bit compute raster (T1).
    TwoPass32,
    /// Instanced quad sprites with depth test (T2 / T3 baseline).
    Sprites,
}

impl PointRasterPath {
    pub const ALL: [PointRasterPath; 3] =
        [PointRasterPath::Compute64, PointRasterPath::TwoPass32, PointRasterPath::Sprites];

    pub fn as_str(self) -> &'static str {
        match self {
            PointRasterPath::Compute64 => "compute64",
            PointRasterPath::TwoPass32 => "twopass32",
            PointRasterPath::Sprites => "sprites",
        }
    }

    pub fn parse(s: &str) -> Option<PointRasterPath> {
        PointRasterPath::ALL.into_iter().find(|p| p.as_str() == s)
    }
}

impl fmt::Display for PointRasterPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Active transparency compositing path (REQ-VIZ.RENDER.FRAME.TIER-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TransparencyPath {
    /// Weighted blended order-independent transparency (interactive T0/T1).
    Wboit,
    /// Multi-pass depth peeling (report-quality / T3 headless stills).
    Peel,
    /// Back-to-front sorted alpha blending (T2 fallback).
    Sorted,
}

impl TransparencyPath {
    pub const ALL: [TransparencyPath; 3] = [TransparencyPath::Wboit, TransparencyPath::Peel, TransparencyPath::Sorted];

    pub fn as_str(self) -> &'static str {
        match self {
            TransparencyPath::Wboit => "wboit",
            TransparencyPath::Peel => "peel",
            TransparencyPath::Sorted => "sorted",
        }
    }

    pub fn parse(s: &str) -> Option<TransparencyPath> {
        TransparencyPath::ALL.into_iter().find(|p| p.as_str() == s)
    }
}

impl fmt::Display for TransparencyPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ------------------------------------------------------------------ counts, budgets, capabilities

/// Displayed vs total primitive counts for one frame (REQ-VIZ.RENDER.FRAME.TIER-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrimitiveCounts {
    pub displayed: u64,
    pub total: u64,
}

impl PrimitiveCounts {
    pub fn add(&mut self, displayed: u64, total: u64) {
        self.displayed = self.displayed.saturating_add(displayed);
        self.total = self.total.saturating_add(total);
    }
}

/// Primitive counts published by passes during a rendered frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameStats {
    pub points: PrimitiveCounts,
    pub triangles: PrimitiveCounts,
}

impl FrameStats {
    pub fn add_points(&mut self, displayed: u64, total: u64) {
        self.points.add(displayed, total);
    }

    pub fn add_triangles(&mut self, displayed: u64, total: u64) {
        self.triangles.add(displayed, total);
    }
}

/// Frame-time and GPU memory budgets reported by [`RenderInfo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderBudgets {
    pub frame_budget_ms: u64,
    pub mem_budget_bytes: u64,
}

impl RenderBudgets {
    /// Deterministic default budgets for `tier` clamped by the adapter's `max_buffer_size`.
    pub fn for_tier(tier: CapabilityTier, max_buffer_size: u64) -> RenderBudgets {
        let frame_budget_ms = match tier {
            CapabilityTier::T3 => DEFAULT_HEADLESS_FRAME_BUDGET_MS,
            CapabilityTier::T0 | CapabilityTier::T1 | CapabilityTier::T2 => DEFAULT_INTERACTIVE_FRAME_BUDGET_MS,
        };
        let mem_budget_bytes =
            if max_buffer_size > 0 { DEFAULT_MEM_BUDGET_BYTES.min(max_buffer_size) } else { DEFAULT_MEM_BUDGET_BYTES };
        RenderBudgets { frame_budget_ms, mem_budget_bytes }
    }
}

/// Pure-data summary of adapter features and limits used for tier classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterCapabilities {
    pub backend: Backend,
    pub adapter: String,
    pub driver: String,
    pub shader_int64_atomic_min_max: bool,
    pub shader_int64_atomic_all_ops: bool,
    pub compute_shaders: bool,
    pub max_compute_workgroups_per_dimension: u32,
    pub max_storage_buffers_per_shader_stage: u32,
    pub max_buffer_size: u64,
    pub headless_server: bool,
}

impl AdapterCapabilities {
    /// Builds [`AdapterCapabilities`] from live wgpu adapter inspection at device creation.
    pub fn from_wgpu(
        backend: Backend,
        info: &wgpu::AdapterInfo,
        features: wgpu::Features,
        downlevel: wgpu::DownlevelFlags,
        limits: &wgpu::Limits,
        headless_server: bool,
    ) -> AdapterCapabilities {
        let adapter = if info.name.trim().is_empty() { backend.as_str().to_string() } else { info.name.clone() };
        let driver = format_driver(&info.driver, &info.driver_info);
        AdapterCapabilities {
            backend,
            adapter,
            driver,
            shader_int64_atomic_min_max: features.contains(wgpu::Features::SHADER_INT64_ATOMIC_MIN_MAX),
            shader_int64_atomic_all_ops: features.contains(wgpu::Features::SHADER_INT64_ATOMIC_ALL_OPS),
            compute_shaders: downlevel.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS),
            max_compute_workgroups_per_dimension: limits.max_compute_workgroups_per_dimension,
            max_storage_buffers_per_shader_stage: limits.max_storage_buffers_per_shader_stage,
            max_buffer_size: limits.max_buffer_size,
            headless_server,
        }
    }

    /// True when the adapter supports compute shaders with usable workgroup and storage limits.
    pub fn supports_compute(&self) -> bool {
        self.compute_shaders
            && self.max_compute_workgroups_per_dimension > 0
            && self.max_storage_buffers_per_shader_stage > 0
    }

    /// True when the adapter supports 64-bit storage atomics (`SHADER_INT64_ATOMIC_MIN_MAX` or
    /// `SHADER_INT64_ATOMIC_ALL_OPS`).
    pub fn supports_atomic64(&self) -> bool {
        self.shader_int64_atomic_min_max || self.shader_int64_atomic_all_ops
    }
}

fn format_driver(driver: &str, driver_info: &str) -> String {
    match (driver.trim(), driver_info.trim()) {
        ("", "") => String::new(),
        (d, "") => d.to_string(),
        ("", i) => i.to_string(),
        (d, i) if i.starts_with(d) => i.to_string(),
        (d, i) => format!("{d} ({i})"),
    }
}

/// Classifies an adapter into [`CapabilityTier`] (REQ-VIZ.RENDER.FRAME.TIER-001):
/// - `T3`: headless server stills (`headless_server == true`);
/// - `T0`: compute shaders AND 64-bit storage atomics;
/// - `T1`: compute shaders without 64-bit storage atomics;
/// - `T2`: lacks compute shaders.
pub fn classify_tier(caps: &AdapterCapabilities) -> CapabilityTier {
    if caps.headless_server {
        CapabilityTier::T3
    } else if caps.supports_compute() && caps.supports_atomic64() {
        CapabilityTier::T0
    } else if caps.supports_compute() {
        CapabilityTier::T1
    } else {
        CapabilityTier::T2
    }
}

/// Default point-raster path for `tier` (REQ-VIZ.RENDER.FRAME.TIER-002, REQ-VIZ.RENDER.LOD.RASTER-001).
pub fn default_point_raster(tier: CapabilityTier) -> PointRasterPath {
    match tier {
        CapabilityTier::T0 => PointRasterPath::Compute64,
        CapabilityTier::T1 => PointRasterPath::TwoPass32,
        CapabilityTier::T2 | CapabilityTier::T3 => PointRasterPath::Sprites,
    }
}

/// Default transparency path for `tier` (REQ-VIZ.RENDER.FRAME.TIER-002, REQ-VIZ.RENDER.SHADE.OIT).
pub fn default_transparency(tier: CapabilityTier) -> TransparencyPath {
    match tier {
        CapabilityTier::T0 | CapabilityTier::T1 => TransparencyPath::Wboit,
        CapabilityTier::T2 => TransparencyPath::Sorted,
        CapabilityTier::T3 => TransparencyPath::Peel,
    }
}

/// Static renderer classification recorded at device creation (REQ-VIZ.RENDER.FRAME.TIER-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderProfile {
    pub tier: CapabilityTier,
    pub backend: Backend,
    pub adapter: String,
    pub driver: String,
    pub point_raster: PointRasterPath,
    pub transparency: TransparencyPath,
}

impl RenderProfile {
    /// Classifies `caps` into a [`RenderProfile`] with default raster and transparency paths.
    pub fn from_capabilities(caps: &AdapterCapabilities) -> RenderProfile {
        let tier = classify_tier(caps);
        RenderProfile {
            tier,
            backend: caps.backend,
            adapter: caps.adapter.clone(),
            driver: caps.driver.clone(),
            point_raster: default_point_raster(tier),
            transparency: default_transparency(tier),
        }
    }
}

// ------------------------------------------------------------------ RenderInfo & canonical JSON

/// Snapshot returned by `view.get_render_info@1` (REQ-VIZ.RENDER.FRAME.TIER-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderInfo {
    pub tier: CapabilityTier,
    pub backend: Backend,
    pub adapter: String,
    pub driver: String,
    pub point_raster: PointRasterPath,
    pub transparency: TransparencyPath,
    pub frame_budget_ms: u64,
    pub mem_budget_bytes: u64,
    pub points: PrimitiveCounts,
    pub triangles: PrimitiveCounts,
}

/// Errors raised while encoding, decoding or validating [`RenderInfo`] or adapter fixtures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TierError {
    InvalidField { field: &'static str, detail: String },
    Parse(String),
}

impl fmt::Display for TierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TierError::InvalidField { field, detail } => write!(f, "invalid field `{field}`: {detail}"),
            TierError::Parse(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for TierError {}

impl RenderInfo {
    /// Combines a [`RenderProfile`], [`RenderBudgets`] and last-frame [`FrameStats`].
    pub fn new(profile: &RenderProfile, budgets: RenderBudgets, stats: FrameStats) -> RenderInfo {
        RenderInfo {
            tier: profile.tier,
            backend: profile.backend,
            adapter: profile.adapter.clone(),
            driver: profile.driver.clone(),
            point_raster: profile.point_raster,
            transparency: profile.transparency,
            frame_budget_ms: budgets.frame_budget_ms,
            mem_budget_bytes: budgets.mem_budget_bytes,
            points: stats.points,
            triangles: stats.triangles,
        }
    }

    /// Converts to a [`CValue`] matching `schemas/render-info-1.json`.
    pub fn to_cvalue(&self) -> Result<CValue, TierError> {
        if self.adapter.is_empty() {
            return Err(TierError::InvalidField { field: "adapter", detail: "must not be empty".into() });
        }
        if self.frame_budget_ms == 0 {
            return Err(TierError::InvalidField { field: "frame_budget_ms", detail: "must be >= 1".into() });
        }
        let mut map = BTreeMap::new();
        map.insert("adapter".into(), CValue::Str(self.adapter.clone()));
        map.insert("backend".into(), CValue::Str(self.backend.as_str().into()));
        map.insert("driver".into(), CValue::Str(self.driver.clone()));
        map.insert("frame_budget_ms".into(), u64_json(self.frame_budget_ms, "frame_budget_ms")?);
        map.insert("mem_budget_bytes".into(), u64_json(self.mem_budget_bytes, "mem_budget_bytes")?);
        map.insert("point_raster".into(), CValue::Str(self.point_raster.as_str().into()));
        map.insert("points".into(), counts_to_cvalue(self.points, "points")?);
        map.insert("tier".into(), CValue::Str(self.tier.as_str().into()));
        map.insert("transparency".into(), CValue::Str(self.transparency.as_str().into()));
        map.insert("triangles".into(), counts_to_cvalue(self.triangles, "triangles")?);
        Ok(CValue::Obj(map))
    }

    /// Serialises to RFC 8785 canonical JSON bytes.
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, TierError> {
        let v = self.to_cvalue()?;
        to_canonical_bytes(&v).map_err(|e| TierError::Parse(e.to_string()))
    }

    /// Strictly validates and decodes a [`CValue`] against `schemas/render-info-1.json`.
    pub fn from_cvalue(value: &CValue) -> Result<RenderInfo, TierError> {
        let CValue::Obj(map) = value else {
            return Err(TierError::Parse("expected JSON object".into()));
        };
        let allowed: BTreeSet<&str> = RENDER_INFO_KEYS.into_iter().collect();
        for k in map.keys() {
            if !allowed.contains(k.as_str()) {
                return Err(TierError::Parse(format!("unexpected property `{k}`")));
            }
        }
        let tier_s = req_str(map, "tier")?;
        let tier = CapabilityTier::parse(tier_s)
            .ok_or_else(|| TierError::InvalidField { field: "tier", detail: format!("unknown tier `{tier_s}`") })?;
        let backend_s = req_str(map, "backend")?;
        let backend = Backend::parse(backend_s).ok_or_else(|| TierError::InvalidField {
            field: "backend",
            detail: format!("unknown backend `{backend_s}`"),
        })?;
        let adapter = req_str(map, "adapter")?.to_string();
        if adapter.is_empty() {
            return Err(TierError::InvalidField { field: "adapter", detail: "must not be empty".into() });
        }
        let driver = req_str(map, "driver")?.to_string();
        let pr_s = req_str(map, "point_raster")?;
        let point_raster = PointRasterPath::parse(pr_s).ok_or_else(|| TierError::InvalidField {
            field: "point_raster",
            detail: format!("unknown point_raster `{pr_s}`"),
        })?;
        let tr_s = req_str(map, "transparency")?;
        let transparency = TransparencyPath::parse(tr_s).ok_or_else(|| TierError::InvalidField {
            field: "transparency",
            detail: format!("unknown transparency `{tr_s}`"),
        })?;
        let frame_budget_ms = req_u64(map, "frame_budget_ms")?;
        if frame_budget_ms == 0 {
            return Err(TierError::InvalidField { field: "frame_budget_ms", detail: "must be >= 1".into() });
        }
        let mem_budget_bytes = req_u64(map, "mem_budget_bytes")?;
        let points = req_counts(map, "points")?;
        let triangles = req_counts(map, "triangles")?;
        Ok(RenderInfo {
            tier,
            backend,
            adapter,
            driver,
            point_raster,
            transparency,
            frame_budget_ms,
            mem_budget_bytes,
            points,
            triangles,
        })
    }

    /// Parses and validates canonical JSON bytes into [`RenderInfo`].
    pub fn from_json(bytes: &[u8]) -> Result<RenderInfo, TierError> {
        let v =
            parse_with_limits(bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH).map_err(|e| TierError::Parse(e.to_string()))?;
        RenderInfo::from_cvalue(&v)
    }
}

fn u64_json(v: u64, field: &'static str) -> Result<CValue, TierError> {
    if v > MAX_SAFE_INT {
        return Err(TierError::InvalidField { field, detail: format!("exceeds 2^53 - 1 ({v})") });
    }
    Ok(CValue::Num(v as f64))
}

fn counts_to_cvalue(c: PrimitiveCounts, field: &'static str) -> Result<CValue, TierError> {
    let mut m = BTreeMap::new();
    m.insert("displayed".into(), u64_json(c.displayed, field)?);
    m.insert("total".into(), u64_json(c.total, field)?);
    Ok(CValue::Obj(m))
}

fn req_str<'a>(map: &'a BTreeMap<String, CValue>, field: &'static str) -> Result<&'a str, TierError> {
    match map.get(field) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(TierError::InvalidField { field, detail: "missing or not a string".into() }),
    }
}

fn req_u64(map: &BTreeMap<String, CValue>, field: &'static str) -> Result<u64, TierError> {
    match map.get(field) {
        Some(CValue::Num(n)) if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n <= MAX_SAFE_INT as f64 => {
            Ok(*n as u64)
        }
        _ => Err(TierError::InvalidField { field, detail: "missing or not a non-negative integer".into() }),
    }
}

fn req_bool(map: &BTreeMap<String, CValue>, field: &'static str) -> Result<bool, TierError> {
    match map.get(field) {
        Some(CValue::Bool(b)) => Ok(*b),
        _ => Err(TierError::InvalidField { field, detail: "missing or not a boolean".into() }),
    }
}

fn req_counts(map: &BTreeMap<String, CValue>, field: &'static str) -> Result<PrimitiveCounts, TierError> {
    let Some(CValue::Obj(sub)) = map.get(field) else {
        return Err(TierError::InvalidField { field, detail: "missing or not an object".into() });
    };
    if sub.len() != 2 || !sub.contains_key("displayed") || !sub.contains_key("total") {
        return Err(TierError::InvalidField { field, detail: "must have exactly displayed and total".into() });
    }
    let displayed = req_u64(sub, "displayed")?;
    let total = req_u64(sub, "total")?;
    Ok(PrimitiveCounts { displayed, total })
}

// ------------------------------------------------------------------ recorded adapter fixtures (AC-2)

/// One recorded adapter test case from `fixtures/viz/adapters.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterFixtureCase {
    pub id: String,
    pub capabilities: AdapterCapabilities,
    pub expected_tier: CapabilityTier,
    pub expected_point_raster: PointRasterPath,
    pub expected_transparency: TransparencyPath,
}

/// Parses `fixtures/viz/adapters.json` (AC-2).
pub fn parse_adapter_fixtures(bytes: &[u8]) -> Result<Vec<AdapterFixtureCase>, TierError> {
    let root = parse_with_limits(bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH).map_err(|e| TierError::Parse(e.to_string()))?;
    let CValue::Obj(map) = root else {
        return Err(TierError::Parse("expected top-level object".into()));
    };
    let Some(CValue::Arr(items)) = map.get("adapters") else {
        return Err(TierError::InvalidField { field: "adapters", detail: "missing array".into() });
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let CValue::Obj(obj) = item else {
            return Err(TierError::Parse("adapter entry must be an object".into()));
        };
        let id = req_str(obj, "id")?.to_string();
        let backend_s = req_str(obj, "backend")?;
        let backend = Backend::parse(backend_s).ok_or_else(|| TierError::InvalidField {
            field: "backend",
            detail: format!("unknown backend `{backend_s}`"),
        })?;
        let adapter = req_str(obj, "adapter")?.to_string();
        let driver = req_str(obj, "driver")?.to_string();
        let Some(CValue::Obj(feats)) = obj.get("features") else {
            return Err(TierError::InvalidField { field: "features", detail: "missing object".into() });
        };
        let Some(CValue::Obj(limits)) = obj.get("limits") else {
            return Err(TierError::InvalidField { field: "limits", detail: "missing object".into() });
        };
        let caps = AdapterCapabilities {
            backend,
            adapter,
            driver,
            shader_int64_atomic_min_max: req_bool(feats, "shader_int64_atomic_min_max")?,
            shader_int64_atomic_all_ops: req_bool(feats, "shader_int64_atomic_all_ops")?,
            compute_shaders: req_bool(feats, "compute_shaders")?,
            max_compute_workgroups_per_dimension: u32::try_from(req_u64(
                limits,
                "max_compute_workgroups_per_dimension",
            )?)
            .map_err(|_| TierError::InvalidField {
                field: "max_compute_workgroups_per_dimension",
                detail: "overflows u32".into(),
            })?,
            max_storage_buffers_per_shader_stage: u32::try_from(req_u64(
                limits,
                "max_storage_buffers_per_shader_stage",
            )?)
            .map_err(|_| TierError::InvalidField {
                field: "max_storage_buffers_per_shader_stage",
                detail: "overflows u32".into(),
            })?,
            max_buffer_size: req_u64(limits, "max_buffer_size")?,
            headless_server: req_bool(obj, "headless_server")?,
        };
        let exp_tier_s = req_str(obj, "expected_tier")?;
        let expected_tier = CapabilityTier::parse(exp_tier_s).ok_or_else(|| TierError::InvalidField {
            field: "expected_tier",
            detail: format!("unknown tier `{exp_tier_s}`"),
        })?;
        let exp_pr_s = req_str(obj, "expected_point_raster")?;
        let expected_point_raster = PointRasterPath::parse(exp_pr_s).ok_or_else(|| TierError::InvalidField {
            field: "expected_point_raster",
            detail: format!("unknown point_raster `{exp_pr_s}`"),
        })?;
        let exp_tr_s = req_str(obj, "expected_transparency")?;
        let expected_transparency = TransparencyPath::parse(exp_tr_s).ok_or_else(|| TierError::InvalidField {
            field: "expected_transparency",
            detail: format!("unknown transparency `{exp_tr_s}`"),
        })?;
        out.push(AdapterFixtureCase {
            id,
            capabilities: caps,
            expected_tier,
            expected_point_raster,
            expected_transparency,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(compute: bool, atomic64: bool, headless: bool) -> AdapterCapabilities {
        AdapterCapabilities {
            backend: Backend::Vulkan,
            adapter: "Test Adapter".into(),
            driver: "1.0".into(),
            shader_int64_atomic_min_max: atomic64,
            shader_int64_atomic_all_ops: false,
            compute_shaders: compute,
            max_compute_workgroups_per_dimension: if compute { 65535 } else { 0 },
            max_storage_buffers_per_shader_stage: if compute { 8 } else { 0 },
            max_buffer_size: 256 * 1024 * 1024,
            headless_server: headless,
        }
    }

    #[test]
    fn classification_rules() {
        assert_eq!(classify_tier(&caps(true, true, false)), CapabilityTier::T0);
        assert_eq!(classify_tier(&caps(true, false, false)), CapabilityTier::T1);
        assert_eq!(classify_tier(&caps(false, false, false)), CapabilityTier::T2);
        assert_eq!(classify_tier(&caps(false, true, false)), CapabilityTier::T2);
        assert_eq!(classify_tier(&caps(true, true, true)), CapabilityTier::T3);
        assert_eq!(classify_tier(&caps(false, false, true)), CapabilityTier::T3);
    }

    #[test]
    fn render_info_canonical_round_trip() {
        let profile = RenderProfile::from_capabilities(&caps(true, true, false));
        let budgets = RenderBudgets::for_tier(profile.tier, 256 * 1024 * 1024);
        let stats = FrameStats {
            points: PrimitiveCounts { displayed: 100, total: 500 },
            triangles: PrimitiveCounts { displayed: 12, total: 12 },
        };
        let info = RenderInfo::new(&profile, budgets, stats);
        let bytes = info.to_canonical_json();
        assert!(bytes.is_ok());
        if let Ok(b) = bytes {
            assert_eq!(RenderInfo::from_json(&b), Ok(info));
        }
    }
}

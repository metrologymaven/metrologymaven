//! `fid-gpu-select`: GPU adapter probing and backend selection (spec leaf `PLAT.GPU.DETECT`).
//! Layer `viewer`, on `fid-gpu-policy` (the policy schema and known-bad lookup, below the tool
//! layer) and wgpu; never a dependency of a kernel crate (REQ-006, CONSTITUTION C2: this data
//! never changes a value of record).
//!
//! - [`select`] is pure (D0): adapter facts, the policy row of the host, the known-bad list and
//!   an optional override in; a [`GpuSelection`] out. Candidates are the row's hardware
//!   backends in policy order (an allowed override first), then by device type (discrete,
//!   integrated, virtual, other), then by input order (stable). The first candidate that meets
//!   the row's minimum and is not excluded by a known-bad entry wins:
//!   - `avoid_backend` skips the adapter on that backend and records the entry ID;
//!   - `force_software` selects `software` at once and records the entry ID;
//!   - `workaround:<name>` keeps the adapter and lists the workaround and its entry ID.
//!
//!   No survivor gives `software` with `fallback = true` and a reason listing every rejected
//!   adapter with its cause.
//! - An allowed `software` override selects `software` at once (`fallback_cause` and reason
//!   `user_override`), so a
//!   user whose GPU driver crashes can escape it; [`Probe::run_with_override`] then probes
//!   nothing. An override the row does not list is ignored and noted in `unverified`.
//! - [`Probe`] lists adapters per backend through a [`Prober`] (the real one is
//!   [`WgpuProber`], no surface, shared as [`Probe::wgpu`]), each backend on a worker thread
//!   with a deadline; a backend that fails or does not answer in time becomes a [`Rejection`]
//!   (`probe_failed`, `probe_timeout`), never an error or a panic. A backend still hung from
//!   an earlier probe is `probe_timeout` at once, without a new thread. The result lists the
//!   backends that timed out or failed ([`GpuSelection::probe_timeout`],
//!   [`GpuSelection::probe_failed`]).
//! - [`GpuSelection::to_json`] is the canonical JSON of REQ-007; [`GpuSelection::adapter_choice`]
//!   is the plain-data choice `VIZ.RENDER.FRAME.GRAPH` takes (no code edge either way).
//!
//! Choices made at build time (see the leaf's Open questions):
//! - A CPU adapter on a hardware backend (lavapipe on Vulkan, WARP on DX12) is never selected
//!   as that backend: it is rejected as `not_hardware`, because REQ-002 makes `software` the
//!   answer when no hardware adapter qualifies.
//! - wgpu reports neither the API version nor a numeric driver version. [`WgpuProber`] reads
//!   the first dotted numeric token of the driver string ([`driver_version_of`]) and records
//!   only a guaranteed API floor ([`AdapterInfo::api_floor`]: DX12 11.0, or 11.1 from wgpu's
//!   features; Vulkan 1.0). A minimum at or below the floor is met; above it, the API minimum
//!   is unverified, listed in [`GpuSelection::unverified`], and does not reject the adapter.
//!   An unknown driver version is handled the same way. A known-bad entry with a driver range
//!   applies to an adapter whose driver version is unknown (the safe side), also listed there.

mod probe;

pub use probe::{driver_version_of, Probe, ProbeReport, Prober, WgpuProber, DEFAULT_PROBE_TIMEOUT};

use fid_canon::CValue;
use fid_gpu_policy::policy::{Backend, BadAction, BadEntry, GpuOs, KnownBad, PolicyRow};
use fid_gpu_policy::Version;
use std::collections::BTreeMap;
use std::fmt;

/// Report format ID of [`GpuSelection::to_json`].
pub const SELECTION_FORMAT: &str = "gpu-selection@2";

/// The kind of adapter, as the graphics API reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeviceType {
    Discrete,
    Integrated,
    Virtual,
    Other,
    Cpu,
}

impl DeviceType {
    /// Hardware preference: discrete, integrated, virtual, other; `None` for a CPU adapter.
    pub fn hardware_rank(self) -> Option<u8> {
        match self {
            DeviceType::Discrete => Some(0),
            DeviceType::Integrated => Some(1),
            DeviceType::Virtual => Some(2),
            DeviceType::Other => Some(3),
            DeviceType::Cpu => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DeviceType::Discrete => "discrete",
            DeviceType::Integrated => "integrated",
            DeviceType::Virtual => "virtual",
            DeviceType::Other => "other",
            DeviceType::Cpu => "cpu",
        }
    }

    pub fn parse(s: &str) -> Option<DeviceType> {
        [DeviceType::Discrete, DeviceType::Integrated, DeviceType::Virtual, DeviceType::Other, DeviceType::Cpu]
            .into_iter()
            .find(|d| d.as_str() == s)
    }
}

/// One adapter on one backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterInfo {
    pub backend: Backend,
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_type: DeviceType,
    /// `None` when the driver reports no version this crate can read.
    pub driver_version: Option<Version>,
    /// `None` when the graphics layer does not report it (wgpu never does).
    pub api_version: Option<Version>,
    /// The lowest API version the graphics layer guarantees for this adapter, a proxy read
    /// when `api_version` is unknown (see [`WgpuProber`]). It can show a minimum is met, never
    /// that it is missed.
    pub api_floor: Option<Version>,
}

/// Why an adapter or a backend was not selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cause {
    /// Below the row's minimum API or driver version.
    BelowMinimum,
    /// Skipped by a known-bad `avoid_backend` entry.
    KnownBad(String),
    /// A CPU adapter on a hardware backend (software is the fallback, not a candidate).
    NotHardware,
    /// The probe of the backend failed.
    ProbeFailed(String),
    /// The probe of the backend did not answer before the deadline.
    ProbeTimeout,
}

impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cause::BelowMinimum => f.write_str("below_minimum"),
            Cause::KnownBad(id) => write!(f, "known_bad({id})"),
            Cause::NotHardware => f.write_str("not_hardware"),
            Cause::ProbeFailed(m) => write!(f, "probe_failed({m})"),
            Cause::ProbeTimeout => f.write_str("probe_timeout"),
        }
    }
}

/// Why the selection is or is not a fallback: the structured form of `fallback`, so a reader
/// (`PLAT.DOCTOR`, the notice) never has to parse `reason`. `fallback` is true exactly when
/// this is not [`FallbackCause::None`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackCause {
    /// A hardware adapter was selected.
    None,
    /// No hardware adapter survived the minimums, the known-bad `avoid_backend` entries and
    /// the probe (every rejection is listed).
    NoQualifyingAdapter,
    /// A known-bad `force_software` entry matched (its ID is in `known_bad`).
    KnownBad,
    /// The caller's allowed `software` override (the user's setting): the hardware was not
    /// evaluated, so this says nothing about the machine's GPU class.
    UserOverride,
}

impl FallbackCause {
    pub fn as_str(self) -> &'static str {
        match self {
            FallbackCause::None => "none",
            FallbackCause::NoQualifyingAdapter => "no_qualifying_adapter",
            FallbackCause::KnownBad => "known_bad",
            FallbackCause::UserOverride => USER_OVERRIDE,
        }
    }

    pub fn parse(s: &str) -> Option<FallbackCause> {
        [FallbackCause::None, FallbackCause::NoQualifyingAdapter, FallbackCause::KnownBad, FallbackCause::UserOverride]
            .into_iter()
            .find(|c| c.as_str() == s)
    }
}

impl fmt::Display for FallbackCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A rejected adapter (`adapter` is its name), or a whole backend (`adapter` is `None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub adapter: Option<String>,
    pub backend: Backend,
    pub cause: Cause,
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.adapter {
            Some(a) => write!(f, "{a} on {}: {}", self.backend, self.cause),
            None => write!(f, "{}: {}", self.backend, self.cause),
        }
    }
}

/// The selected adapter, as REQ-007 reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedAdapter {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_type: DeviceType,
    pub driver_version: Option<Version>,
}

/// The plain-data adapter choice `VIZ.RENDER.FRAME.GRAPH` takes (`AdapterChoice` there):
/// the backend by its policy name and the adapter's vendor ID, device ID and name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterChoice {
    pub backend: String,
    pub vendor: Option<u32>,
    pub device: Option<u32>,
    pub name: Option<String>,
}

/// The result of [`select`] (REQ-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuSelection {
    pub backend: Backend,
    /// `None` for `software`.
    pub adapter: Option<SelectedAdapter>,
    /// True when no hardware adapter was selected (no survivor, or `force_software`).
    pub fallback: bool,
    /// Why `fallback` is set ([`FallbackCause::None`] when it is not). Classify by this, never
    /// by `fallback` alone: a user override is a fallback on any hardware.
    pub fallback_cause: FallbackCause,
    /// One line per rejection, then why software was chosen when it was.
    pub reason: String,
    pub rejections: Vec<Rejection>,
    /// IDs of the known-bad entries that changed the result, in the order they applied.
    pub known_bad: Vec<String>,
    pub workarounds: Vec<String>,
    /// Checks that could not be made (an unknown API or driver version), and the override
    /// when the row does not allow it.
    pub unverified: Vec<String>,
    /// Backends whose probe timed out (or was still hung from an earlier probe), in the order
    /// of the probe rejections. The frame graph must not touch them.
    pub probe_timeout: Vec<Backend>,
    /// Backends whose probe failed.
    pub probe_failed: Vec<Backend>,
}

/// The backends of the backend-level rejections (`adapter` is `None`) whose cause matches.
pub(crate) fn probe_backends(rejections: &[Rejection], cause: impl Fn(&Cause) -> bool) -> Vec<Backend> {
    let mut out = Vec::new();
    for r in rejections.iter().filter(|r| r.adapter.is_none() && cause(&r.cause)) {
        if !out.contains(&r.backend) {
            out.push(r.backend);
        }
    }
    out
}

/// The reason given when the caller's override selects `software`.
pub const USER_OVERRIDE: &str = "user_override";

/// A known-bad entry applies to `a` on `os`. With an unknown driver version, an entry with a
/// driver range applies (the safe side) and `unknown` is set.
fn entry_applies(e: &BadEntry, os: GpuOs, a: &AdapterInfo, unknown: &mut bool) -> bool {
    if e.os != os || e.backend != a.backend || e.vendor_id != a.vendor_id || !e.device_ids.contains(a.device_id) {
        return false;
    }
    match &a.driver_version {
        Some(d) => e.driver.contains(d),
        None => {
            if e.driver.min.is_some() || e.driver.max.is_some() {
                *unknown = true;
            }
            true
        }
    }
}

/// Selects the backend and adapter (see the crate documentation). `probe_rejections` are the
/// backend-level failures of [`Probe::run`]; they are carried into the result.
/// `override_backend` is tried first when the row lists it: a hardware backend moves to the
/// front of the order (a known-bad `force_software` still wins); `software` is selected at
/// once with reason `user_override` and `fallback = true`, without evaluating any adapter. An
/// override the row does not list is ignored and noted in `unverified`.
pub fn select(
    adapters: &[AdapterInfo],
    probe_rejections: &[Rejection],
    row: &PolicyRow,
    known_bad: &KnownBad,
    override_backend: Option<Backend>,
) -> GpuSelection {
    let mut order: Vec<Backend> = row.backends.iter().copied().filter(|b| *b != Backend::Software).collect();
    let mut unverified = Vec::new();
    match override_backend {
        Some(Backend::Software) if row.backends.contains(&Backend::Software) => {
            let why = format!("{USER_OVERRIDE}: software requested by the caller");
            return software(probe_rejections.to_vec(), Vec::new(), unverified, FallbackCause::UserOverride, Some(why));
        }
        Some(o) => match order.iter().position(|b| *b == o) {
            Some(i) => {
                let b = order.remove(i);
                order.insert(0, b);
            }
            None => unverified.push(format!("override {o} is not a backend of the policy row; ignored")),
        },
        None => {}
    }
    let mut candidates: Vec<(usize, u8, usize, &AdapterInfo)> = Vec::new();
    let mut rejections: Vec<Rejection> = probe_rejections.to_vec();
    for (i, a) in adapters.iter().enumerate() {
        let Some(bi) = order.iter().position(|b| *b == a.backend) else { continue };
        match a.device_type.hardware_rank() {
            Some(r) => candidates.push((bi, r, i, a)),
            None => rejections.push(Rejection {
                adapter: Some(a.name.clone()),
                backend: a.backend,
                cause: Cause::NotHardware,
            }),
        }
    }
    candidates.sort_by_key(|c| (c.0, c.1, c.2));
    let mut applied = Vec::new();
    for (_, _, _, a) in candidates {
        if !meets_minimum(row, a, &mut unverified) {
            rejections.push(Rejection {
                adapter: Some(a.name.clone()),
                backend: a.backend,
                cause: Cause::BelowMinimum,
            });
            continue;
        }
        let mut unknown = false;
        let matching: Vec<&BadEntry> =
            known_bad.entries.iter().filter(|e| entry_applies(e, row.os, a, &mut unknown)).collect();
        if unknown {
            unverified
                .push(format!("{} on {}: driver version unknown; known-bad driver ranges applied", a.name, a.backend));
        }
        let winning = matching.iter().copied().fold(None::<&BadEntry>, |best, e| match best {
            Some(b) if b.action.precedence() <= e.action.precedence() => Some(b),
            _ => Some(e),
        });
        match winning.map(|e| (&e.action, e)) {
            Some((BadAction::ForceSoftware, e)) => {
                applied.push(e.id.clone());
                let why = format!("known-bad entry {} forces software for {} on {}", e.id, a.name, a.backend);
                return software(rejections, applied, unverified, FallbackCause::KnownBad, Some(why));
            }
            Some((BadAction::AvoidBackend, e)) => {
                applied.push(e.id.clone());
                rejections.push(Rejection {
                    adapter: Some(a.name.clone()),
                    backend: a.backend,
                    cause: Cause::KnownBad(e.id.clone()),
                });
                continue;
            }
            _ => {}
        }
        let mut workarounds = Vec::new();
        for e in &matching {
            if let BadAction::Workaround(w) = &e.action {
                applied.push(e.id.clone());
                if !workarounds.contains(w) {
                    workarounds.push(w.clone());
                }
            }
        }
        let reason = rejections.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ");
        let probe_timeout = probe_backends(&rejections, |c| matches!(c, Cause::ProbeTimeout));
        let probe_failed = probe_backends(&rejections, |c| matches!(c, Cause::ProbeFailed(_)));
        return GpuSelection {
            backend: a.backend,
            adapter: Some(SelectedAdapter {
                name: a.name.clone(),
                vendor_id: a.vendor_id,
                device_id: a.device_id,
                device_type: a.device_type,
                driver_version: a.driver_version,
            }),
            fallback: false,
            fallback_cause: FallbackCause::None,
            reason,
            rejections,
            known_bad: applied,
            workarounds,
            unverified,
            probe_timeout,
            probe_failed,
        };
    }
    software(rejections, applied, unverified, FallbackCause::NoQualifyingAdapter, None)
}

fn meets_minimum(row: &PolicyRow, a: &AdapterInfo, unverified: &mut Vec<String>) -> bool {
    let Some(min) = row.min.get(&a.backend) else {
        return true;
    };
    match (&a.api_version, &a.api_floor) {
        (Some(v), _) if *v < min.api => return false,
        (Some(_), _) => {}
        // The guaranteed floor meets the minimum: verified without the exact version.
        (None, Some(f)) if *f >= min.api => {}
        (None, Some(f)) => unverified.push(format!(
            "{} on {}: API version unknown (at least {f}); minimum {} unverified",
            a.name, a.backend, min.api
        )),
        (None, None) => {
            unverified.push(format!("{} on {}: API version unknown; minimum {} unverified", a.name, a.backend, min.api))
        }
    }
    if let Some(dmin) = &min.driver {
        match &a.driver_version {
            Some(v) if v < dmin => return false,
            Some(_) => {}
            None => unverified
                .push(format!("{} on {}: driver version unknown; minimum driver {dmin} unverified", a.name, a.backend)),
        }
    }
    true
}

fn software(
    rejections: Vec<Rejection>,
    known_bad: Vec<String>,
    unverified: Vec<String>,
    cause: FallbackCause,
    forced: Option<String>,
) -> GpuSelection {
    let mut lines: Vec<String> = rejections.iter().map(ToString::to_string).collect();
    lines.push(forced.unwrap_or_else(|| "no hardware adapter qualifies; software".to_string()));
    let probe_timeout = probe_backends(&rejections, |c| matches!(c, Cause::ProbeTimeout));
    let probe_failed = probe_backends(&rejections, |c| matches!(c, Cause::ProbeFailed(_)));
    GpuSelection {
        backend: Backend::Software,
        adapter: None,
        fallback: true,
        fallback_cause: cause,
        reason: lines.join("; "),
        rejections,
        known_bad,
        workarounds: Vec::new(),
        unverified,
        probe_timeout,
        probe_failed,
    }
}

fn backends(v: &[Backend]) -> CValue {
    CValue::Arr(v.iter().map(|b| s(b.as_str())).collect())
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn strs(v: &[String]) -> CValue {
    CValue::Arr(v.iter().map(|x| s(x)).collect())
}

fn hex(n: u32) -> CValue {
    s(&format!("0x{n:08x}"))
}

impl GpuSelection {
    /// REQ-007 as a value: `backend`, `adapter` (`name`, `vendor_id`, `device_id` as `0x`
    /// hex, `device_type`, `driver_version`, or `null` for software), `fallback`,
    /// `fallback_cause` (`none`, `no_qualifying_adapter`, `known_bad`, `user_override`), `reason`,
    /// `rejections`, `known_bad`, `workarounds`, `unverified`, `probe_timeout`, `probe_failed`.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), s(SELECTION_FORMAT));
        m.insert("backend".to_string(), s(self.backend.as_str()));
        m.insert(
            "adapter".to_string(),
            match &self.adapter {
                None => CValue::Null,
                Some(a) => CValue::Obj(BTreeMap::from([
                    ("name".to_string(), s(&a.name)),
                    ("vendor_id".to_string(), hex(a.vendor_id)),
                    ("device_id".to_string(), hex(a.device_id)),
                    ("device_type".to_string(), s(a.device_type.as_str())),
                    ("driver_version".to_string(), a.driver_version.map_or(CValue::Null, |v| s(&v.to_string()))),
                ])),
            },
        );
        m.insert("fallback".to_string(), CValue::Bool(self.fallback));
        m.insert("fallback_cause".to_string(), s(self.fallback_cause.as_str()));
        m.insert("reason".to_string(), s(&self.reason));
        m.insert(
            "rejections".to_string(),
            CValue::Arr(
                self.rejections
                    .iter()
                    .map(|r| {
                        CValue::Obj(BTreeMap::from([
                            ("adapter".to_string(), r.adapter.as_deref().map_or(CValue::Null, s)),
                            ("backend".to_string(), s(r.backend.as_str())),
                            ("cause".to_string(), s(&r.cause.to_string())),
                        ]))
                    })
                    .collect(),
            ),
        );
        m.insert("known_bad".to_string(), strs(&self.known_bad));
        m.insert("workarounds".to_string(), strs(&self.workarounds));
        m.insert("unverified".to_string(), strs(&self.unverified));
        m.insert("probe_timeout".to_string(), backends(&self.probe_timeout));
        m.insert("probe_failed".to_string(), backends(&self.probe_failed));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON of [`GpuSelection::to_cvalue`] (byte-identical for equal inputs).
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    /// The choice to hand `VIZ.RENDER.FRAME.GRAPH`: the selected adapter on its backend, or
    /// `software` with no narrowing.
    pub fn adapter_choice(&self) -> AdapterChoice {
        match &self.adapter {
            Some(a) => AdapterChoice {
                backend: self.backend.as_str().to_string(),
                vendor: Some(a.vendor_id),
                device: Some(a.device_id),
                name: Some(a.name.clone()),
            },
            None => AdapterChoice {
                backend: Backend::Software.as_str().to_string(),
                vendor: None,
                device: None,
                name: None,
            },
        }
    }
}

/// Parses a recorded adapter list (`fixtures/plat/gpu/adapters-*.json`): a JSON array of
/// `{ backend, name, vendor_id, device_id, device_type, driver_version, api_version }` and an
/// optional `api_floor`, IDs as `"0x..."` strings, versions as dotted strings or `null`.
/// Duplicate and unknown keys are refused.
pub fn parse_adapters(bytes: &[u8]) -> Result<Vec<AdapterInfo>, String> {
    let v = fid_canon::parse_with_limits(bytes, 1024 * 1024, 8).map_err(|e| format!("not valid JSON: {}", e.code()))?;
    let CValue::Arr(items) = v else {
        return Err("an adapter list is a JSON array".to_string());
    };
    let mut out = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let at = format!("adapter {i}");
        let CValue::Obj(m) = it else { return Err(format!("{at} is not an object")) };
        let keys: Vec<&str> = m.keys().map(String::as_str).filter(|k| *k != "api_floor").collect();
        let want = ["api_version", "backend", "device_id", "device_type", "driver_version", "name", "vendor_id"];
        if keys != want {
            return Err(format!("{at} must have exactly the keys {} (and optionally api_floor)", want.join(", ")));
        }
        let text = |k: &str| match m.get(k) {
            Some(CValue::Str(x)) => Ok(x.as_str()),
            _ => Err(format!("{at}: `{k}` is not a string")),
        };
        let id = |k: &str| -> Result<u32, String> {
            let t = text(k)?;
            t.strip_prefix("0x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .ok_or_else(|| format!("{at}: `{k}` is not a 0x hex u32"))
        };
        let ver = |k: &str| -> Result<Option<Version>, String> {
            match m.get(k) {
                Some(CValue::Null) => Ok(None),
                Some(CValue::Str(x)) => {
                    Version::parse(x).map(Some).ok_or_else(|| format!("{at}: `{k}` is not a version"))
                }
                _ => Err(format!("{at}: `{k}` is not a version or null")),
            }
        };
        out.push(AdapterInfo {
            backend: Backend::from_name(text("backend")?).ok_or_else(|| format!("{at}: unknown backend"))?,
            name: text("name")?.to_string(),
            vendor_id: id("vendor_id")?,
            device_id: id("device_id")?,
            device_type: DeviceType::parse(text("device_type")?).ok_or_else(|| format!("{at}: unknown device_type"))?,
            driver_version: ver("driver_version")?,
            api_version: ver("api_version")?,
            api_floor: if m.contains_key("api_floor") { ver("api_floor")? } else { None },
        });
    }
    Ok(out)
}

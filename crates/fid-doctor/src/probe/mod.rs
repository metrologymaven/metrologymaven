//! Environment probes for `fid doctor` (spec node `PLAT.DOCTOR.PROBE`).
//!
//! Gathers OS family and version, architecture, CPU model/physical cores/instruction-set level,
//! total and available RAM, free/total storage on install and data volumes (without paths),
//! primary display resolution, and GPU selection.
//!
//! Every probe sits behind the [`Env`] trait:
//! - [`RealEnv`] probes the local machine on Windows, Linux and macOS (zero network, no user name,
//!   host name, serial numbers or MAC addresses).
//! - [`FakeEnv`] replays a recorded [`EnvSnapshot`] loaded from JSON so downstream tests run
//!   deterministically (D0) without hardware.
//!
//! [`probe_all`] runs every probe on its own worker thread under a per-probe deadline (default 2 s),
//! turning errors, timeouts, unsupported probes and worker panics into [`Probed::Unknown`] in a
//! fixed field order without ever panicking.

mod real;

pub use real::RealEnv;

use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// Snapshot schema version.
pub const SCHEMA_VERSION: u32 = 1;
/// Default per-probe deadline (REQ-PLAT.DOCTOR.PROBE-002: 2 s).
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(2);
/// Maximum length of [`Probed::Unknown`] `detail` strings, in Unicode scalar values.
pub const MAX_DETAIL_CHARS: usize = 200;
/// Largest snapshot JSON accepted by [`EnvSnapshot::from_json`] and [`FakeEnv::from_file`] (1 MiB).
pub const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;
/// Deepest JSON nesting accepted when parsing a snapshot.
const MAX_JSON_DEPTH: usize = 16;
/// Maximum number of volume entries accepted in `storage`.
const MAX_VOLUMES: usize = 16;

/// Why a probed item is [`Probed::Unknown`] (REQ-PLAT.DOCTOR.PROBE-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnknownReason {
    Error,
    Timeout,
    Unsupported,
    NoDisplay,
    NotBuilt,
}

impl UnknownReason {
    pub const ALL: [UnknownReason; 5] = [
        UnknownReason::Error,
        UnknownReason::Timeout,
        UnknownReason::Unsupported,
        UnknownReason::NoDisplay,
        UnknownReason::NotBuilt,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            UnknownReason::Error => "error",
            UnknownReason::Timeout => "timeout",
            UnknownReason::Unsupported => "unsupported",
            UnknownReason::NoDisplay => "no_display",
            UnknownReason::NotBuilt => "not_built",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == s)
    }
}

impl fmt::Display for UnknownReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Sanitises and truncates an unknown-probe detail string to at most [`MAX_DETAIL_CHARS`] chars.
pub fn bound_detail(detail: &str) -> String {
    let cleaned: String = detail.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for c in collapsed.chars().take(MAX_DETAIL_CHARS) {
        out.push(c);
    }
    out
}

/// Either a known probed value or `unknown` with a structured reason and bounded detail.
#[derive(Debug, Clone, PartialEq)]
pub enum Probed<T> {
    Known(T),
    Unknown { reason: UnknownReason, detail: String },
}

impl<T> Probed<T> {
    /// Constructs [`Probed::Unknown`] with `detail` sanitised and bounded to [`MAX_DETAIL_CHARS`].
    pub fn unknown(reason: UnknownReason, detail: impl AsRef<str>) -> Self {
        Probed::Unknown { reason, detail: bound_detail(detail.as_ref()) }
    }

    pub fn is_known(&self) -> bool {
        matches!(self, Probed::Known(_))
    }

    pub fn known(&self) -> Option<&T> {
        match self {
            Probed::Known(v) => Some(v),
            Probed::Unknown { .. } => None,
        }
    }

    pub fn unknown_reason(&self) -> Option<UnknownReason> {
        match self {
            Probed::Known(_) => None,
            Probed::Unknown { reason, .. } => Some(*reason),
        }
    }
}

/// Operating system family and version (no hostname or username).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsInfo {
    /// Lowercase OS family (e.g. `"windows"`, `"ubuntu"`, `"macos"`, `"freebsd"`).
    pub family: String,
    /// Version string (e.g. `"10.0.22631"`, `"24.04"`, `"15.1"`).
    pub version: String,
}

/// CPU architecture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
    Other(String),
}

impl Arch {
    pub fn as_str(&self) -> &str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
            Arch::Other(s) => s.as_str(),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim();
        if t.is_empty() || t.len() > 64 || t.chars().any(char::is_control) {
            return None;
        }
        Some(match t {
            "x86_64" => Arch::X86_64,
            "aarch64" => Arch::Aarch64,
            other => Arch::Other(other.to_string()),
        })
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// CPU model, physical core count and instruction-set level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuInfo {
    pub model: String,
    pub physical_cores: u32,
    /// Instruction-set level (e.g. `"x86-64-v2"`, `"x86-64-v3"`, `"armv8.0"`, `"armv8.2"`).
    pub instruction_set_level: String,
}

/// Total and available RAM in GiB.
#[derive(Debug, Clone, PartialEq)]
pub struct RamInfo {
    pub total_gib: f64,
    pub available_gib: f64,
}

/// Role of a probed storage volume (no filesystem paths are stored).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VolumeRole {
    Install,
    Data,
}

impl VolumeRole {
    pub fn as_str(self) -> &'static str {
        match self {
            VolumeRole::Install => "install",
            VolumeRole::Data => "data",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "install" => Some(VolumeRole::Install),
            "data" => Some(VolumeRole::Data),
            _ => None,
        }
    }
}

/// Free and total storage on one volume role, in GiB (no paths).
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeInfo {
    pub role: VolumeRole,
    pub free_gib: f64,
    pub total_gib: f64,
}

/// GPU backend selection reported in [`EnvSnapshot`].
///
/// Mirrors the fields `fid doctor` and `PLAT.DOCTOR.LOOKUP` inspect from
/// `fid-gpu-select::GpuSelection`, plus optional dedicated VRAM (`vram_gib`) for profile
/// classification. When built with the `gpu` feature, converts directly from
/// `&fid_gpu_select::GpuSelection`.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuSelection {
    pub backend: String,
    pub adapter: Option<String>,
    pub driver_version: Option<String>,
    pub device_type: Option<String>,
    pub vram_gib: Option<f64>,
    pub fallback: bool,
    pub fallback_cause: String,
    pub reason: String,
}

#[cfg(feature = "gpu")]
impl From<&fid_gpu_select::GpuSelection> for GpuSelection {
    fn from(sel: &fid_gpu_select::GpuSelection) -> Self {
        let (adapter, driver_version, device_type) = match &sel.adapter {
            Some(a) => (
                Some(a.name.clone()),
                a.driver_version.map(|v| v.to_string()),
                Some(a.device_type.as_str().to_string()),
            ),
            None => (None, None, None),
        };
        GpuSelection {
            backend: sel.backend.as_str().to_string(),
            adapter,
            driver_version,
            device_type,
            vram_gib: None,
            fallback: sel.fallback,
            fallback_cause: sel.fallback_cause.as_str().to_string(),
            reason: bound_detail(&sel.reason),
        }
    }
}

/// Complete environment snapshot produced by [`probe_all`].
#[derive(Debug, Clone, PartialEq)]
pub struct EnvSnapshot {
    pub schema_version: u32,
    pub os: Probed<OsInfo>,
    pub arch: Probed<Arch>,
    pub cpu: Probed<CpuInfo>,
    pub ram: Probed<RamInfo>,
    pub storage: Probed<Vec<VolumeInfo>>,
    pub display: Probed<(u32, u32)>,
    pub gpu: Probed<GpuSelection>,
}

/// Error parsing or serialising an [`EnvSnapshot`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotError {
    pub message: String,
}

impl SnapshotError {
    fn new(msg: impl Into<String>) -> Self {
        SnapshotError { message: msg.into() }
    }
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SnapshotError {}

/// Trait implemented by [`RealEnv`] and [`FakeEnv`] (one method per probe).
pub trait Env: Send + Sync + 'static {
    fn probe_os(&self) -> Probed<OsInfo>;
    fn probe_arch(&self) -> Probed<Arch>;
    fn probe_cpu(&self) -> Probed<CpuInfo>;
    fn probe_ram(&self) -> Probed<RamInfo>;
    fn probe_storage(&self) -> Probed<Vec<VolumeInfo>>;
    fn probe_display(&self) -> Probed<(u32, u32)>;
    fn probe_gpu(&self, deadline: Duration) -> Probed<GpuSelection>;
}

/// Fake [`Env`] fed from a recorded [`EnvSnapshot`] (REQ-PLAT.DOCTOR.PROBE-005).
#[derive(Debug, Clone, PartialEq)]
pub struct FakeEnv {
    snapshot: EnvSnapshot,
}

impl FakeEnv {
    pub fn from_snapshot(snapshot: EnvSnapshot) -> Self {
        FakeEnv { snapshot }
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, SnapshotError> {
        Ok(FakeEnv { snapshot: EnvSnapshot::from_json(bytes)? })
    }

    pub fn from_file(path: &Path) -> Result<Self, SnapshotError> {
        let meta = std::fs::symlink_metadata(path)
            .map_err(|e| SnapshotError::new(format!("cannot stat the snapshot file: {}", e.kind())))?;
        if meta.file_type().is_symlink() {
            return Err(SnapshotError::new("snapshot file: symbolic links are not allowed"));
        }
        if meta.len() > MAX_SNAPSHOT_BYTES as u64 {
            return Err(SnapshotError::new(format!("snapshot file: exceeds {MAX_SNAPSHOT_BYTES} byte limit")));
        }
        let file = std::fs::File::open(path)
            .map_err(|e| SnapshotError::new(format!("cannot open the snapshot file: {}", e.kind())))?;
        let mut buf = Vec::new();
        file.take(MAX_SNAPSHOT_BYTES as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| SnapshotError::new(format!("cannot read the snapshot file: {}", e.kind())))?;
        if buf.len() > MAX_SNAPSHOT_BYTES {
            return Err(SnapshotError::new(format!("snapshot file: exceeds {MAX_SNAPSHOT_BYTES} byte limit")));
        }
        Self::from_json(&buf)
    }
}

impl Env for FakeEnv {
    fn probe_os(&self) -> Probed<OsInfo> {
        self.snapshot.os.clone()
    }

    fn probe_arch(&self) -> Probed<Arch> {
        self.snapshot.arch.clone()
    }

    fn probe_cpu(&self) -> Probed<CpuInfo> {
        self.snapshot.cpu.clone()
    }

    fn probe_ram(&self) -> Probed<RamInfo> {
        self.snapshot.ram.clone()
    }

    fn probe_storage(&self) -> Probed<Vec<VolumeInfo>> {
        self.snapshot.storage.clone()
    }

    fn probe_display(&self) -> Probed<(u32, u32)> {
        self.snapshot.display.clone()
    }

    fn probe_gpu(&self, _deadline: Duration) -> Probed<GpuSelection> {
        self.snapshot.gpu.clone()
    }
}

enum ProbeRx<T> {
    Ready(mpsc::Receiver<Probed<T>>),
    NotStarted(String),
}

fn spawn_probe<T, F>(name: &str, f: F) -> ProbeRx<T>
where
    T: Send + 'static,
    F: FnOnce() -> Probed<T> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    match std::thread::Builder::new().name(name.to_string()).spawn(move || {
        let val = f();
        let _ = tx.send(val);
    }) {
        Ok(_) => ProbeRx::Ready(rx),
        Err(e) => ProbeRx::NotStarted(e.to_string()),
    }
}

fn collect_probe<T>(rx: ProbeRx<T>, deadline: Option<Instant>) -> Probed<T> {
    let rx = match rx {
        ProbeRx::Ready(r) => r,
        ProbeRx::NotStarted(e) => {
            return Probed::unknown(UnknownReason::Error, format!("failed to start probe thread: {e}"));
        }
    };
    let res = match deadline {
        Some(d) => rx.recv_timeout(d.saturating_duration_since(Instant::now())),
        None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
    };
    match res {
        Ok(Probed::Known(v)) => Probed::Known(v),
        Ok(Probed::Unknown { reason, detail }) => Probed::unknown(reason, detail),
        Err(mpsc::RecvTimeoutError::Timeout) => Probed::unknown(UnknownReason::Timeout, "probe exceeded deadline"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Probed::unknown(UnknownReason::Error, "probe thread terminated unexpectedly")
        }
    }
}

/// Runs all environment probes concurrently under `per_probe_deadline` and collects the
/// [`EnvSnapshot`] in fixed field order (`os`, `arch`, `cpu`, `ram`, `storage`, `display`, `gpu`).
/// Never panics; every error, timeout or worker panic becomes [`Probed::Unknown`].
pub fn probe_all<E: Env>(env: E, per_probe_deadline: Duration) -> EnvSnapshot {
    let env = Arc::new(env);
    let deadline = Instant::now().checked_add(per_probe_deadline);

    let os_rx = {
        let e = Arc::clone(&env);
        spawn_probe("doctor-probe-os", move || e.probe_os())
    };
    let arch_rx = {
        let e = Arc::clone(&env);
        spawn_probe("doctor-probe-arch", move || e.probe_arch())
    };
    let cpu_rx = {
        let e = Arc::clone(&env);
        spawn_probe("doctor-probe-cpu", move || e.probe_cpu())
    };
    let ram_rx = {
        let e = Arc::clone(&env);
        spawn_probe("doctor-probe-ram", move || e.probe_ram())
    };
    let storage_rx = {
        let e = Arc::clone(&env);
        spawn_probe("doctor-probe-storage", move || e.probe_storage())
    };
    let display_rx = {
        let e = Arc::clone(&env);
        spawn_probe("doctor-probe-display", move || e.probe_display())
    };
    let gpu_rx = {
        let e = Arc::clone(&env);
        spawn_probe("doctor-probe-gpu", move || e.probe_gpu(per_probe_deadline))
    };

    EnvSnapshot {
        schema_version: SCHEMA_VERSION,
        os: collect_probe(os_rx, deadline),
        arch: collect_probe(arch_rx, deadline),
        cpu: collect_probe(cpu_rx, deadline),
        ram: collect_probe(ram_rx, deadline),
        storage: collect_probe(storage_rx, deadline),
        display: collect_probe(display_rx, deadline),
        gpu: collect_probe(gpu_rx, deadline),
    }
}

// ------------------------------------------------------------------ canonical JSON & schema validation

fn cstr(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn probed_to_cvalue<T>(p: &Probed<T>, encode: impl Fn(&T) -> CValue) -> CValue {
    let mut m = BTreeMap::new();
    match p {
        Probed::Known(v) => {
            m.insert("status".to_string(), cstr("known"));
            m.insert("value".to_string(), encode(v));
        }
        Probed::Unknown { reason, detail } => {
            m.insert("detail".to_string(), cstr(&bound_detail(detail)));
            m.insert("reason".to_string(), cstr(reason.as_str()));
            m.insert("status".to_string(), cstr("unknown"));
        }
    }
    CValue::Obj(m)
}

impl EnvSnapshot {
    /// Converts the snapshot into a canonical JSON value tree (`BTreeMap`-ordered keys).
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("schema_version".to_string(), CValue::Num(f64::from(self.schema_version)));
        m.insert(
            "os".to_string(),
            probed_to_cvalue(&self.os, |os| {
                CValue::Obj(BTreeMap::from([
                    ("family".to_string(), cstr(&os.family)),
                    ("version".to_string(), cstr(&os.version)),
                ]))
            }),
        );
        m.insert("arch".to_string(), probed_to_cvalue(&self.arch, |a| cstr(a.as_str())));
        m.insert(
            "cpu".to_string(),
            probed_to_cvalue(&self.cpu, |c| {
                CValue::Obj(BTreeMap::from([
                    ("instruction_set_level".to_string(), cstr(&c.instruction_set_level)),
                    ("model".to_string(), cstr(&c.model)),
                    ("physical_cores".to_string(), CValue::Num(f64::from(c.physical_cores))),
                ]))
            }),
        );
        m.insert(
            "ram".to_string(),
            probed_to_cvalue(&self.ram, |r| {
                CValue::Obj(BTreeMap::from([
                    ("available_gib".to_string(), CValue::Num(r.available_gib)),
                    ("total_gib".to_string(), CValue::Num(r.total_gib)),
                ]))
            }),
        );
        m.insert(
            "storage".to_string(),
            probed_to_cvalue(&self.storage, |vols| {
                CValue::Arr(
                    vols.iter()
                        .map(|v| {
                            CValue::Obj(BTreeMap::from([
                                ("free_gib".to_string(), CValue::Num(v.free_gib)),
                                ("role".to_string(), cstr(v.role.as_str())),
                                ("total_gib".to_string(), CValue::Num(v.total_gib)),
                            ]))
                        })
                        .collect(),
                )
            }),
        );
        m.insert(
            "display".to_string(),
            probed_to_cvalue(&self.display, |(w, h)| {
                CValue::Arr(vec![CValue::Num(f64::from(*w)), CValue::Num(f64::from(*h))])
            }),
        );
        m.insert(
            "gpu".to_string(),
            probed_to_cvalue(&self.gpu, |g| {
                CValue::Obj(BTreeMap::from([
                    ("adapter".to_string(), g.adapter.as_deref().map_or(CValue::Null, cstr)),
                    ("backend".to_string(), cstr(&g.backend)),
                    ("device_type".to_string(), g.device_type.as_deref().map_or(CValue::Null, cstr)),
                    ("driver_version".to_string(), g.driver_version.as_deref().map_or(CValue::Null, cstr)),
                    ("fallback".to_string(), CValue::Bool(g.fallback)),
                    ("fallback_cause".to_string(), cstr(&g.fallback_cause)),
                    ("reason".to_string(), cstr(&g.reason)),
                    ("vram_gib".to_string(), g.vram_gib.map_or(CValue::Null, CValue::Num)),
                ]))
            }),
        );
        CValue::Obj(m)
    }

    /// Serialises to RFC 8785 canonical JSON bytes.
    pub fn to_json(&self) -> Result<Vec<u8>, SnapshotError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|e| SnapshotError::new(e.to_string()))
    }

    /// Parses and validates an [`EnvSnapshot`] from JSON bytes against the v1 snapshot schema.
    pub fn from_json(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let v = fid_canon::parse_with_limits(bytes, MAX_SNAPSHOT_BYTES, MAX_JSON_DEPTH)
            .map_err(|e| SnapshotError::new(format!("invalid JSON: {}", e.code())))?;
        Self::from_cvalue(&v)
    }

    /// Validates an already parsed JSON value against the v1 snapshot schema (used by
    /// [`EnvSnapshot::from_json`] and by the doctor report parser, which nests a snapshot).
    pub fn from_cvalue(v: &CValue) -> Result<Self, SnapshotError> {
        let CValue::Obj(root) = v else {
            return Err(SnapshotError::new("snapshot root must be a JSON object"));
        };
        check_exact_keys(
            root,
            &["arch", "cpu", "display", "gpu", "os", "ram", "schema_version", "storage"],
            "snapshot",
        )?;
        let schema_version = req_u32(root, "schema_version", "snapshot")?;
        if schema_version != SCHEMA_VERSION {
            return Err(SnapshotError::new(format!("unsupported schema_version (expected {SCHEMA_VERSION})")));
        }
        let os = parse_probed(req_val(root, "os", "snapshot")?, "os", parse_os_info)?;
        let arch = parse_probed(req_val(root, "arch", "snapshot")?, "arch", parse_arch)?;
        let cpu = parse_probed(req_val(root, "cpu", "snapshot")?, "cpu", parse_cpu_info)?;
        let ram = parse_probed(req_val(root, "ram", "snapshot")?, "ram", parse_ram_info)?;
        let storage = parse_probed(req_val(root, "storage", "snapshot")?, "storage", parse_volumes)?;
        let display = parse_probed(req_val(root, "display", "snapshot")?, "display", parse_display)?;
        let gpu = parse_probed(req_val(root, "gpu", "snapshot")?, "gpu", parse_gpu)?;

        Ok(EnvSnapshot { schema_version, os, arch, cpu, ram, storage, display, gpu })
    }
}

fn check_exact_keys(m: &BTreeMap<String, CValue>, expected: &[&str], ctx: &str) -> Result<(), SnapshotError> {
    let keys: Vec<&str> = m.keys().map(String::as_str).collect();
    if keys != expected {
        // The keys found are input: name only the expected ones.
        return Err(SnapshotError::new(format!("{ctx}: expected exactly the keys [{}]", expected.join(", "))));
    }
    Ok(())
}

fn req_val<'a>(m: &'a BTreeMap<String, CValue>, key: &str, ctx: &str) -> Result<&'a CValue, SnapshotError> {
    m.get(key).ok_or_else(|| SnapshotError::new(format!("{ctx}: missing `{key}`")))
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, key: &str, ctx: &str) -> Result<&'a str, SnapshotError> {
    match m.get(key) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(SnapshotError::new(format!("{ctx}: `{key}` must be a string"))),
    }
}

fn req_non_empty_str(m: &BTreeMap<String, CValue>, key: &str, ctx: &str) -> Result<String, SnapshotError> {
    let s = req_str(m, key, ctx)?;
    if s.trim().is_empty() {
        return Err(SnapshotError::new(format!("{ctx}: `{key}` must be non-empty")));
    }
    Ok(s.to_string())
}

fn req_opt_str(m: &BTreeMap<String, CValue>, key: &str, ctx: &str) -> Result<Option<String>, SnapshotError> {
    match m.get(key) {
        Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        _ => Err(SnapshotError::new(format!("{ctx}: `{key}` must be a string or null"))),
    }
}

fn req_bool(m: &BTreeMap<String, CValue>, key: &str, ctx: &str) -> Result<bool, SnapshotError> {
    match m.get(key) {
        Some(CValue::Bool(b)) => Ok(*b),
        _ => Err(SnapshotError::new(format!("{ctx}: `{key}` must be a boolean"))),
    }
}

fn req_u32(m: &BTreeMap<String, CValue>, key: &str, ctx: &str) -> Result<u32, SnapshotError> {
    match m.get(key) {
        Some(CValue::Num(n)) => num_to_u32(*n, &format!("{ctx}.{key}")),
        _ => Err(SnapshotError::new(format!("{ctx}: `{key}` must be a non-negative integer"))),
    }
}

fn num_to_u32(n: f64, ctx: &str) -> Result<u32, SnapshotError> {
    if !n.is_finite() || n < 0.0 || n > f64::from(u32::MAX) || n.fract() != 0.0 {
        return Err(SnapshotError::new(format!("{ctx}: must be a u32 integer")));
    }
    Ok(n as u32)
}

fn req_nonneg_f64(m: &BTreeMap<String, CValue>, key: &str, ctx: &str) -> Result<f64, SnapshotError> {
    match m.get(key) {
        Some(CValue::Num(n)) if n.is_finite() && *n >= 0.0 => Ok(*n),
        _ => Err(SnapshotError::new(format!("{ctx}: `{key}` must be a finite non-negative number"))),
    }
}

fn req_opt_nonneg_f64(m: &BTreeMap<String, CValue>, key: &str, ctx: &str) -> Result<Option<f64>, SnapshotError> {
    match m.get(key) {
        Some(CValue::Null) => Ok(None),
        Some(CValue::Num(n)) if n.is_finite() && *n >= 0.0 => Ok(Some(*n)),
        _ => Err(SnapshotError::new(format!("{ctx}: `{key}` must be a finite non-negative number or null"))),
    }
}

fn parse_probed<T>(
    v: &CValue,
    ctx: &str,
    parse_val: impl Fn(&CValue, &str) -> Result<T, SnapshotError>,
) -> Result<Probed<T>, SnapshotError> {
    let CValue::Obj(m) = v else {
        return Err(SnapshotError::new(format!("{ctx}: must be a JSON object")));
    };
    let status = req_str(m, "status", ctx)?;
    match status {
        "known" => {
            check_exact_keys(m, &["status", "value"], ctx)?;
            let inner = req_val(m, "value", ctx)?;
            Ok(Probed::Known(parse_val(inner, &format!("{ctx}.value"))?))
        }
        "unknown" => {
            check_exact_keys(m, &["detail", "reason", "status"], ctx)?;
            let reason_s = req_str(m, "reason", ctx)?;
            let reason = UnknownReason::parse(reason_s)
                .ok_or_else(|| SnapshotError::new(format!("{ctx}.reason: unknown reason")))?;
            let detail = req_str(m, "detail", ctx)?;
            if detail.chars().count() > MAX_DETAIL_CHARS {
                return Err(SnapshotError::new(format!("{ctx}.detail: exceeds {MAX_DETAIL_CHARS} character limit")));
            }
            Ok(Probed::Unknown { reason, detail: detail.to_string() })
        }
        _ => Err(SnapshotError::new(format!("{ctx}.status: must be `known` or `unknown`"))),
    }
}

fn parse_os_info(v: &CValue, ctx: &str) -> Result<OsInfo, SnapshotError> {
    let CValue::Obj(m) = v else { return Err(SnapshotError::new(format!("{ctx}: must be an object"))) };
    check_exact_keys(m, &["family", "version"], ctx)?;
    Ok(OsInfo { family: req_non_empty_str(m, "family", ctx)?, version: req_non_empty_str(m, "version", ctx)? })
}

fn parse_arch(v: &CValue, ctx: &str) -> Result<Arch, SnapshotError> {
    let CValue::Str(s) = v else { return Err(SnapshotError::new(format!("{ctx}: must be a string"))) };
    Arch::parse(s).ok_or_else(|| SnapshotError::new(format!("{ctx}: invalid arch")))
}

fn parse_cpu_info(v: &CValue, ctx: &str) -> Result<CpuInfo, SnapshotError> {
    let CValue::Obj(m) = v else { return Err(SnapshotError::new(format!("{ctx}: must be an object"))) };
    check_exact_keys(m, &["instruction_set_level", "model", "physical_cores"], ctx)?;
    let physical_cores = req_u32(m, "physical_cores", ctx)?;
    if physical_cores == 0 {
        return Err(SnapshotError::new(format!("{ctx}.physical_cores: must be >= 1")));
    }
    Ok(CpuInfo {
        model: req_non_empty_str(m, "model", ctx)?,
        physical_cores,
        instruction_set_level: req_non_empty_str(m, "instruction_set_level", ctx)?,
    })
}

fn parse_ram_info(v: &CValue, ctx: &str) -> Result<RamInfo, SnapshotError> {
    let CValue::Obj(m) = v else { return Err(SnapshotError::new(format!("{ctx}: must be an object"))) };
    check_exact_keys(m, &["available_gib", "total_gib"], ctx)?;
    Ok(RamInfo {
        total_gib: req_nonneg_f64(m, "total_gib", ctx)?,
        available_gib: req_nonneg_f64(m, "available_gib", ctx)?,
    })
}

fn parse_volumes(v: &CValue, ctx: &str) -> Result<Vec<VolumeInfo>, SnapshotError> {
    let CValue::Arr(arr) = v else { return Err(SnapshotError::new(format!("{ctx}: must be an array"))) };
    if arr.len() > MAX_VOLUMES {
        return Err(SnapshotError::new(format!("{ctx}: more than {MAX_VOLUMES} volume entries")));
    }
    let mut out = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let ic = format!("{ctx}[{i}]");
        let CValue::Obj(m) = item else { return Err(SnapshotError::new(format!("{ic}: must be an object"))) };
        check_exact_keys(m, &["free_gib", "role", "total_gib"], &ic)?;
        let role_s = req_str(m, "role", &ic)?;
        let role = VolumeRole::parse(role_s)
            .ok_or_else(|| SnapshotError::new(format!("{ic}.role: expected `install` or `data`")))?;
        out.push(VolumeInfo {
            role,
            free_gib: req_nonneg_f64(m, "free_gib", &ic)?,
            total_gib: req_nonneg_f64(m, "total_gib", &ic)?,
        });
    }
    Ok(out)
}

fn parse_display(v: &CValue, ctx: &str) -> Result<(u32, u32), SnapshotError> {
    let CValue::Arr(arr) = v else { return Err(SnapshotError::new(format!("{ctx}: must be a [w, h] array"))) };
    if arr.len() != 2 {
        return Err(SnapshotError::new(format!("{ctx}: expected [width, height]")));
    }
    let (CValue::Num(wn), CValue::Num(hn)) = (&arr[0], &arr[1]) else {
        return Err(SnapshotError::new(format!("{ctx}: width and height must be integers")));
    };
    let w = num_to_u32(*wn, &format!("{ctx}[0]"))?;
    let h = num_to_u32(*hn, &format!("{ctx}[1]"))?;
    if w == 0 || h == 0 {
        return Err(SnapshotError::new(format!("{ctx}: display dimensions must be >= 1")));
    }
    Ok((w, h))
}

fn parse_gpu(v: &CValue, ctx: &str) -> Result<GpuSelection, SnapshotError> {
    let CValue::Obj(m) = v else { return Err(SnapshotError::new(format!("{ctx}: must be an object"))) };
    check_exact_keys(
        m,
        &["adapter", "backend", "device_type", "driver_version", "fallback", "fallback_cause", "reason", "vram_gib"],
        ctx,
    )?;
    Ok(GpuSelection {
        backend: req_non_empty_str(m, "backend", ctx)?,
        adapter: req_opt_str(m, "adapter", ctx)?,
        driver_version: req_opt_str(m, "driver_version", ctx)?,
        device_type: req_opt_str(m, "device_type", ctx)?,
        vram_gib: req_opt_nonneg_f64(m, "vram_gib", ctx)?,
        fallback: req_bool(m, "fallback", ctx)?,
        fallback_cause: req_non_empty_str(m, "fallback_cause", ctx)?,
        reason: req_str(m, "reason", ctx)?.to_string(),
    })
}

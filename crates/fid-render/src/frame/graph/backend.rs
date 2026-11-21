//! Backend order and adapter selection (REQ-VIZ.RENDER.FRAME.GRAPH-001, -002).
//!
//! The order comes from the backend policy file `platform/gpu-policy.toml` (`PLAT.GPU.POLICY`),
//! read here as an artifact (no code edge to `fid-platform`, which is a tool-layer crate). Only
//! the fields the frame graph needs are read: `schema_version`, and each `[[row]]`'s `os`,
//! `arch` and `backends`; `min` and anything else in a row belong to `PLAT.GPU.DETECT`.
//!
//! Precedence: an injected [`AdapterChoice`] is tried first, then the backend named by the
//! setting [`BACKEND_SETTING`], then the policy order (each backend once). Every backend that
//! yields no adapter is recorded as an [`Attempt`] with its reason; if none yields one, the
//! result is [`GraphError::NoAdapter`] listing every attempt in order.

use super::GraphError;
use std::fmt;

/// The setting that overrides the policy order (`CORE.STO.SET`). `auto` (or no value) keeps
/// the policy order.
pub const BACKEND_SETTING: &str = "viz.render.backend";
/// Largest policy file read, in bytes (the same bound as `PLAT.GPU.POLICY`).
pub const MAX_POLICY_BYTES: usize = 1024 * 1024;
/// Most `[[row]]` items read.
pub const MAX_POLICY_ROWS: usize = 10_000;
/// The only policy `schema_version` read.
pub const POLICY_SCHEMA: i64 = 1;

/// A graphics backend as named in the policy file and the setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Backend {
    Vulkan,
    Metal,
    Dx12,
    Gl,
    WebGpu,
    /// A CPU adapter on any compiled backend (WARP, lavapipe, llvmpipe).
    Software,
}

impl Backend {
    /// Every backend, in declaration order.
    pub const ALL: [Backend; 6] =
        [Backend::Vulkan, Backend::Metal, Backend::Dx12, Backend::Gl, Backend::WebGpu, Backend::Software];

    /// Parses the policy and setting spelling (`vulkan`, `metal`, `dx12`, `gl`, `webgpu`,
    /// `software`).
    pub fn parse(s: &str) -> Option<Backend> {
        Backend::ALL.into_iter().find(|b| b.as_str() == s)
    }

    /// The policy and setting spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Vulkan => "vulkan",
            Backend::Metal => "metal",
            Backend::Dx12 => "dx12",
            Backend::Gl => "gl",
            Backend::WebGpu => "webgpu",
            Backend::Software => "software",
        }
    }

    /// The wgpu backends searched for this backend (`Software`: all of them).
    pub fn wgpu_backends(self) -> wgpu::Backends {
        match self {
            Backend::Vulkan => wgpu::Backends::VULKAN,
            Backend::Metal => wgpu::Backends::METAL,
            Backend::Dx12 => wgpu::Backends::DX12,
            Backend::Gl => wgpu::Backends::GL,
            Backend::WebGpu => wgpu::Backends::BROWSER_WEBGPU,
            Backend::Software => wgpu::Backends::all(),
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The host a policy row is chosen for: `os` is `windows`, `linux` or `macos` and `arch`
/// `x86_64` or `aarch64`, as in the policy file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    pub os: String,
    pub arch: String,
}

impl Host {
    /// The host this process runs on (`std::env::consts`).
    pub fn current() -> Host {
        Host { os: std::env::consts::OS.to_string(), arch: std::env::consts::ARCH.to_string() }
    }
}

/// Why the policy file gave no order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    /// Longer than [`MAX_POLICY_BYTES`], or more than [`MAX_POLICY_ROWS`] rows.
    TooLarge,
    /// Not UTF-8 or not TOML (duplicate keys included).
    Parse(String),
    /// Well-formed TOML that is not a policy of [`POLICY_SCHEMA`].
    Schema(String),
    /// A row of the host names a backend this reader does not know.
    UnknownBackend(String),
    /// No row for the host.
    NoRow { os: String, arch: String },
    /// More than one row for the host.
    DuplicateRow { os: String, arch: String },
    /// The file could not be read.
    Io(String),
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolicyError::TooLarge => write!(f, "policy file over {MAX_POLICY_BYTES} bytes or {MAX_POLICY_ROWS} rows"),
            PolicyError::Parse(e) => write!(f, "policy file is not valid TOML: {e}"),
            PolicyError::Schema(e) => write!(f, "policy file schema: {e}"),
            PolicyError::UnknownBackend(b) => write!(f, "policy names unknown backend `{b}`"),
            PolicyError::NoRow { os, arch } => write!(f, "policy has no row for {os}/{arch}"),
            PolicyError::DuplicateRow { os, arch } => write!(f, "policy has more than one row for {os}/{arch}"),
            PolicyError::Io(e) => write!(f, "policy file cannot be read: {e}"),
        }
    }
}

/// The backend order of the policy row for `host`.
pub fn policy_order(bytes: &[u8], host: &Host) -> Result<Vec<Backend>, PolicyError> {
    if bytes.len() > MAX_POLICY_BYTES {
        return Err(PolicyError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|e| PolicyError::Parse(e.to_string()))?;
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| PolicyError::Parse(e.message().to_string()))?;
    match table.get("schema_version") {
        Some(toml::Value::Integer(v)) if *v == POLICY_SCHEMA => {}
        Some(toml::Value::Integer(v)) => return Err(PolicyError::Schema(format!("schema_version {v} is not 1"))),
        _ => return Err(PolicyError::Schema("schema_version missing or not an integer".into())),
    }
    let rows = match table.get("row") {
        Some(toml::Value::Array(rows)) => rows,
        _ => return Err(PolicyError::Schema("no [[row]] array".into())),
    };
    if rows.len() > MAX_POLICY_ROWS {
        return Err(PolicyError::TooLarge);
    }
    // Every row is checked (names known, each backend once, `software` last, as
    // REQ-PLAT.GPU.POLICY-004 requires), so a broken row elsewhere is not silently accepted.
    let mut found: Option<Vec<Backend>> = None;
    for (i, row) in rows.iter().enumerate() {
        let toml::Value::Table(row) = row else {
            return Err(PolicyError::Schema(format!("row {} is not a table", i + 1)));
        };
        let field = |k: &str| match row.get(k) {
            Some(toml::Value::String(s)) => Ok(s.as_str()),
            _ => Err(PolicyError::Schema(format!("row {}: `{k}` missing or not a string", i + 1))),
        };
        let (os, arch) = (field("os")?, field("arch")?);
        let Some(toml::Value::Array(list)) = row.get("backends") else {
            return Err(PolicyError::Schema(format!("row {}: `backends` missing or not an array", i + 1)));
        };
        let mut order = Vec::new();
        for v in list {
            let toml::Value::String(s) = v else {
                return Err(PolicyError::Schema(format!("row {}: a backend is not a string", i + 1)));
            };
            let b = Backend::parse(s).ok_or_else(|| PolicyError::UnknownBackend(s.clone()))?;
            if order.contains(&b) {
                return Err(PolicyError::Schema(format!("row {}: backend `{b}` is listed twice", i + 1)));
            }
            order.push(b);
        }
        if order.last() != Some(&Backend::Software) {
            return Err(PolicyError::Schema(format!("row {}: `backends` must end with `software`", i + 1)));
        }
        if os == host.os && arch == host.arch {
            if found.is_some() {
                return Err(PolicyError::DuplicateRow { os: host.os.clone(), arch: host.arch.clone() });
            }
            found = Some(order);
        }
    }
    let Some(order) = found else {
        return Err(PolicyError::NoRow { os: host.os.clone(), arch: host.arch.clone() });
    };
    Ok(order)
}

/// Reads the policy file at `path` (at most [`MAX_POLICY_BYTES`]) and returns the host's
/// order.
pub fn policy_order_from_file(path: &std::path::Path, host: &Host) -> Result<Vec<Backend>, PolicyError> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).map_err(|e| PolicyError::Io(e.to_string()))?;
    let mut bytes = Vec::new();
    file.take(MAX_POLICY_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| PolicyError::Io(e.to_string()))?;
    policy_order(&bytes, host)
}

/// The safe default of `PLAT.GPU.POLICY`: the policy order, or `[software]` with the error
/// (to be reported as a warning) when the policy gave none.
pub fn order_or_software(r: Result<Vec<Backend>, PolicyError>) -> (Vec<Backend>, Option<PolicyError>) {
    match r {
        Ok(order) => (order, None),
        Err(e) => (vec![Backend::Software], Some(e)),
    }
}

/// An adapter picked by the caller (for example from a `PLAT.GPU.DETECT` selection), passed
/// as plain data. `vendor`, `device` and `name` narrow the backend's adapters when given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterChoice {
    pub backend: Backend,
    pub vendor: Option<u32>,
    pub device: Option<u32>,
    pub name: Option<String>,
}

impl AdapterChoice {
    /// True when `info` is the chosen adapter.
    pub fn matches(&self, info: &wgpu::AdapterInfo) -> bool {
        self.vendor.is_none_or(|v| v == info.vendor)
            && self.device.is_none_or(|d| d == info.device)
            && self.name.as_ref().is_none_or(|n| *n == info.name)
    }
}

/// The order to try: the choice's backend, then the setting's, then the policy order, each
/// backend once. A setting that is not `auto` or a backend name is [`GraphError::BadSetting`].
pub fn resolve_order(
    policy: &[Backend],
    setting: Option<&str>,
    choice: Option<&AdapterChoice>,
) -> Result<Vec<Backend>, GraphError> {
    let mut order = Vec::new();
    if let Some(c) = choice {
        order.push(c.backend);
    }
    match setting {
        None | Some("auto") => {}
        Some(s) => {
            let b = Backend::parse(s).ok_or_else(|| GraphError::BadSetting(s.to_string()))?;
            if !order.contains(&b) {
                order.push(b);
            }
        }
    }
    for b in policy {
        if !order.contains(b) {
            order.push(*b);
        }
    }
    Ok(order)
}

/// A backend that yielded no adapter, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub backend: Backend,
    pub reason: String,
}

/// Where adapters come from: the real wgpu instance ([`super::WgpuSource`]) or a scripted fake
/// in tests.
pub trait AdapterSource {
    /// What a successful request gives (for wgpu: the adapter with its device and queue).
    type Adapter;
    /// An adapter on `backend` (narrowed by `choice` when given), or the reason there is none.
    fn request(&self, backend: Backend, choice: Option<&AdapterChoice>) -> Result<Self::Adapter, String>;
}

/// The selected adapter and every attempt that failed before it.
#[derive(Debug)]
pub struct Selected<A> {
    pub backend: Backend,
    pub adapter: A,
    pub tried: Vec<Attempt>,
}

/// Tries `order` first to last and returns the first adapter. `choice` applies only to its
/// own backend. [`GraphError::NoAdapter`] lists every attempt when none succeeds.
pub fn select<S: AdapterSource>(
    source: &S,
    order: &[Backend],
    choice: Option<&AdapterChoice>,
) -> Result<Selected<S::Adapter>, GraphError> {
    let mut tried = Vec::new();
    for &backend in order {
        let c = choice.filter(|c| c.backend == backend);
        match source.request(backend, c) {
            Ok(adapter) => return Ok(Selected { backend, adapter, tried }),
            Err(reason) => tried.push(Attempt { backend, reason }),
        }
    }
    Err(GraphError::NoAdapter { tried })
}

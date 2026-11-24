//! WASM kernel sandbox with digest check and limits (spec node `TRUST.REPLAY.SANDBOX`).
//!
//! The only place the verifier runs code from outside its own binary. [`Sandbox::load`]:
//!
//! 1. hashes the artifact and refuses it with `sandbox.artifact_mismatch` unless the SHA-256
//!    equals the kernel manifest's `wasm32-wasip1` entry (REQ-001), before parsing a byte;
//! 2. parses the module with a bounded, fail-closed reader and refuses every import outside
//!    [`IMPORT_ALLOWLIST`] and [`TRAPPED_IMPORTS`] with `sandbox.forbidden_import` (REQ-002);
//! 3. validates and compiles every function body.
//!
//! [`Sandbox::call`] then runs the kernel's single entry point, the KRN.MAN.WASM command
//! protocol: a fresh instance per call reads the canonical `KernelCall` JSON on standard input
//! and writes the result envelope on standard output. The host side is five deterministic
//! WASI stubs (stdin from the call bytes, stdout captured, stderr counted, an empty
//! environment, `proc_exit`); there is no host function for files, sockets, clocks,
//! randomness or environment variables (REQ-003), and the sealed clock imports trap.
//!
//! Limits (REQ-004/005): linear memory capped at [`Limits::memory_bytes`] (default 1 GiB,
//! KRN.MAN.WASM's `max_memory_size`; `sandbox.memory_limit`), fuel per call and a wall-clock
//! budget per sandbox, i.e. per package (`sandbox.budget_exhausted`, no partial result).
//! Loading is bounded too: every parse and validation step is charged to a work budget of 64
//! units per artifact byte plus 2^20, beyond which the module is `sandbox.malformed_module`. A trap is `sandbox.trap` with its
//! [`TrapKind`] and never aborts the process (REQ-006). The interpreter is deterministic, so
//! the same call gives byte-identical envelopes (REQ-007); the watchdog only ever stops a call.
//!
//! Runtime: a pure-Rust interpreter in this crate (no third-party crate, no `unsafe`, builds
//! for wasm32), configured like the pinned KRN.MAN.WASM runtime settings: NaN
//! canonicalisation on, no SIMD (relaxed or not), no threads, `memory.grow` failure traps,
//! the clock traps, fuel metering.

mod exec;
mod module;

use crate::kart::KernelManifest;
use fid_canon::digest_bytes;
use fid_types::Digest;
use std::fmt;
use std::time::Duration;

/// A monotonic clock: the time since a fixed origin of the caller's choosing. The sandbox
/// reads it only when [`Limits::wall_clock`] is set, and only to stop a call.
pub type Clock = fn() -> Duration;

/// The default clock: [`std::time::Instant`] since the first reading in this process.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn std_clock() -> Duration {
    static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    ORIGIN.get_or_init(std::time::Instant::now).elapsed()
}

/// The clock used when the caller set none. `wasm32-unknown-unknown` has no clock
/// (`Instant::now` panics there), so a wall-clock budget needs [`Sandbox::with_clock`].
fn default_clock() -> Option<Clock> {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        Some(std_clock)
    }
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        None
    }
}

/// Function imports a kernel may have, as allowlisted by KRN.MAN.WASM
/// (`crates/fid-kernel-wasm/wasm-imports.allow`), each bound to a deterministic stub.
pub const IMPORT_ALLOWLIST: &[(&str, &str)] = &[
    ("wasi_snapshot_preview1", "fd_read"),
    ("wasi_snapshot_preview1", "fd_write"),
    ("wasi_snapshot_preview1", "proc_exit"),
    ("wasi_snapshot_preview1", "environ_get"),
    ("wasi_snapshot_preview1", "environ_sizes_get"),
];

/// Clock imports renamed to module `fid_trap` by `fid-wasm-check seal`: they link, and any
/// call to them is `sandbox.trap` (`host_trap`), as KRN.MAN.WASM's `clock = "trap"` requires.
pub const TRAPPED_IMPORTS: &[(&str, &str)] = &[("fid_trap", "clock_time_get"), ("fid_trap", "clock_res_get")];

/// Largest canonical call accepted on standard input (KRN.MAN.WASM `MAX_CALL_BYTES`).
pub const MAX_CALL_BYTES: usize = 64 * 1024 * 1024;
/// Largest number of inputs in one call (KRN.MAN.WASM pinned choice).
pub const MAX_CALL_INPUTS: usize = 4096;
/// Largest algorithm ID in bytes (KRN.MAN.WASM pinned choice).
pub const MAX_ALGORITHM_BYTES: usize = 128;
/// Entry point of a WASI command module.
pub const ENTRY_EXPORT: &str = "_start";

/// Stable error codes (spec node Design, plus two for inputs that are not a kernel at all).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// `sandbox.artifact_mismatch`: SHA-256 differs from the manifest (or no wasm entry).
    ArtifactMismatch,
    /// `sandbox.forbidden_import`: an import outside the allowlist.
    ForbiddenImport,
    /// `sandbox.memory_limit`: linear memory or output beyond the caps.
    MemoryLimit,
    /// `sandbox.budget_exhausted`: fuel per call or wall clock per package ran out.
    BudgetExhausted,
    /// `sandbox.trap`: the module trapped; see [`SandboxError::trap`].
    Trap,
    /// `sandbox.malformed_module`: not a valid module, or uses a refused feature.
    MalformedModule,
    /// `sandbox.bad_request`: the call cannot be encoded in the pinned call format.
    BadRequest,
}

impl ErrorCode {
    /// Wire form, e.g. `"sandbox.trap"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::ArtifactMismatch => "sandbox.artifact_mismatch",
            ErrorCode::ForbiddenImport => "sandbox.forbidden_import",
            ErrorCode::MemoryLimit => "sandbox.memory_limit",
            ErrorCode::BudgetExhausted => "sandbox.budget_exhausted",
            ErrorCode::Trap => "sandbox.trap",
            ErrorCode::MalformedModule => "sandbox.malformed_module",
            ErrorCode::BadRequest => "sandbox.bad_request",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Kind of a `sandbox.trap` (REQ-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrapKind {
    Unreachable,
    IntegerDivideByZero,
    IntegerOverflow,
    BadConversionToInteger,
    OutOfBoundsMemoryAccess,
    CallStackExhausted,
    IndirectCallToNull,
    BadSignature,
    TableOutOfBounds,
    /// A call to a sealed (`fid_trap`) import.
    HostTrap,
    /// The command ended in a way the KRN.MAN.WASM protocol does not allow (exit code, output
    /// shape or stderr output).
    UnexpectedExit,
    /// An internal invariant of the interpreter failed; reported, never a panic.
    EngineFault,
}

impl TrapKind {
    /// Wire form, e.g. `"out_of_bounds_memory_access"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            TrapKind::Unreachable => "unreachable",
            TrapKind::IntegerDivideByZero => "integer_divide_by_zero",
            TrapKind::IntegerOverflow => "integer_overflow",
            TrapKind::BadConversionToInteger => "bad_conversion_to_integer",
            TrapKind::OutOfBoundsMemoryAccess => "out_of_bounds_memory_access",
            TrapKind::CallStackExhausted => "call_stack_exhausted",
            TrapKind::IndirectCallToNull => "indirect_call_to_null",
            TrapKind::BadSignature => "bad_signature",
            TrapKind::TableOutOfBounds => "table_out_of_bounds",
            TrapKind::HostTrap => "host_trap",
            TrapKind::UnexpectedExit => "unexpected_exit",
            TrapKind::EngineFault => "engine_fault",
        }
    }
}

/// A refusal or failure of the sandbox. Never carries a partial result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxError {
    pub code: ErrorCode,
    /// Set exactly when `code` is [`ErrorCode::Trap`].
    pub trap: Option<TrapKind>,
    pub detail: String,
}

impl SandboxError {
    pub(crate) fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        SandboxError { code, trap: None, detail: detail.into() }
    }

    pub(crate) fn trap(kind: TrapKind, detail: impl Into<String>) -> Self {
        SandboxError { code: ErrorCode::Trap, trap: Some(kind), detail: detail.into() }
    }
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.trap {
            Some(k) => write!(f, "{} ({}): {}", self.code, k.as_str(), self.detail),
            None => write!(f, "{}: {}", self.code, self.detail),
        }
    }
}

impl std::error::Error for SandboxError {}

/// Caller-set limits (REQ-004, REQ-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Linear-memory cap in bytes, rounded down to whole 64 KiB pages. Default
    /// [`DEFAULT_MEMORY_BYTES`], 1 GiB: KRN.MAN.WASM's pinned `max_memory_size`.
    pub memory_bytes: u64,
    /// Fuel per call: one unit per executed operation, plus one per WASI iovec, per KiB
    /// moved by `memory.copy`, `memory.fill` and WASI reads and writes, and per KiB of locals
    /// zeroed when a function is entered. Default
    /// 4 000 000 000, the KRN.MAN.WASM pinned `fuel`.
    pub fuel_per_call: u64,
    /// Wall-clock budget of the sandbox, counted from its first call over all its calls (one
    /// sandbox per package). `None` disables the watchdog and the clock is never read. On
    /// `wasm32-unknown-unknown` a budget needs a caller-provided [`Clock`]
    /// ([`Sandbox::with_clock`]); without one every call is refused with
    /// `sandbox.bad_request`. Default 120 s \[unverified\].
    pub wall_clock: Option<Duration>,
    /// Maximum call depth. Default 10 000.
    pub max_call_depth: u32,
    /// Maximum value-stack slots (8 bytes each) over all frames. Default 4 Mi (32 MiB).
    pub max_stack_slots: u64,
    /// Maximum bytes the kernel may write to standard output. Default 64 MiB.
    pub max_output_bytes: u64,
    /// Maximum artifact size in bytes. Default 64 MiB (the KRN.MAN.WASM reader's cap).
    pub max_module_bytes: u64,
}

/// Default linear-memory cap: 1 GiB, KRN.MAN.WASM's pinned `max_memory_size`
/// (`crates/fid-kernel-wasm/wasm-runtime.toml`; maintainer decision 2026-11-23).
pub const DEFAULT_MEMORY_BYTES: u64 = 1_073_741_824;

impl Default for Limits {
    fn default() -> Self {
        Limits {
            memory_bytes: DEFAULT_MEMORY_BYTES,
            fuel_per_call: 4_000_000_000,
            wall_clock: Some(Duration::from_secs(120)),
            max_call_depth: 10_000,
            max_stack_slots: 4 * 1024 * 1024,
            max_output_bytes: 64 * 1024 * 1024,
            max_module_bytes: 64 * 1024 * 1024,
        }
    }
}

/// One input of a kernel call: its bytes and the digest the package claims for them. The
/// kernel checks the claim (KRN.MAN.KCI); the sandbox passes it through unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallInput {
    pub bytes: Vec<u8>,
    pub sha256: Digest,
}

impl CallInput {
    /// An input whose claimed digest is the SHA-256 of `bytes`.
    pub fn new(bytes: Vec<u8>) -> Self {
        let sha256 = Digest::from_bytes(digest_bytes(&bytes).0);
        CallInput { bytes, sha256 }
    }
}

/// A kernel call (REQ-KRN.MAN-001): algorithm ID, parameter bytes, optional seed, inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRequest {
    pub algorithm: String,
    pub params: Vec<u8>,
    pub seed: Option<u64>,
    pub inputs: Vec<CallInput>,
}

fn push_hex(out: &mut Vec<u8>, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &b in bytes {
        out.push(HEX[usize::from(b >> 4)]);
        out.push(HEX[usize::from(b & 0x0f)]);
    }
}

impl CallRequest {
    /// The canonical call bytes of the KRN.MAN.WASM protocol:
    /// `{"algorithm":"<id>","inputs":[{"bytes":"<hex>","sha256":"sha256:<hex>"},...],"params":"<hex>","seed":"<u64>"}`
    /// with `seed` absent when there is none. Refuses (`sandbox.bad_request`) an ID that is
    /// empty, longer than 128 bytes or not printable ASCII without `"` and `\`, more than
    /// 4096 inputs, or a call longer than 64 MiB.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, SandboxError> {
        let id = self.algorithm.as_bytes();
        let id_ok = !id.is_empty()
            && id.len() <= MAX_ALGORITHM_BYTES
            && id.iter().all(|&c| (0x21..=0x7e).contains(&c) && c != b'"' && c != b'\\');
        if !id_ok {
            return Err(SandboxError::new(ErrorCode::BadRequest, "algorithm ID is not a valid AlgorithmId string"));
        }
        if self.inputs.len() > MAX_CALL_INPUTS {
            return Err(SandboxError::new(ErrorCode::BadRequest, "more than 4096 inputs"));
        }
        let mut size: usize = 64 + id.len() + 2 * self.params.len();
        for i in &self.inputs {
            size = size.saturating_add(2 * i.bytes.len()).saturating_add(100);
        }
        if size > MAX_CALL_BYTES + 64 {
            return Err(SandboxError::new(ErrorCode::BadRequest, "call larger than 64 MiB"));
        }
        let mut out = Vec::with_capacity(size);
        out.extend_from_slice(b"{\"algorithm\":\"");
        out.extend_from_slice(id);
        out.extend_from_slice(b"\",\"inputs\":[");
        for (k, i) in self.inputs.iter().enumerate() {
            if k > 0 {
                out.push(b',');
            }
            out.extend_from_slice(b"{\"bytes\":\"");
            push_hex(&mut out, &i.bytes);
            out.extend_from_slice(b"\",\"sha256\":\"");
            out.extend_from_slice(i.sha256.as_str().as_bytes());
            out.extend_from_slice(b"\"}");
        }
        out.extend_from_slice(b"],\"params\":\"");
        push_hex(&mut out, &self.params);
        out.push(b'"');
        if let Some(seed) = self.seed {
            out.extend_from_slice(b",\"seed\":\"");
            out.extend_from_slice(seed.to_string().as_bytes());
            out.push(b'"');
        }
        out.push(b'}');
        if out.len() > MAX_CALL_BYTES {
            return Err(SandboxError::new(ErrorCode::BadRequest, "call larger than 64 MiB"));
        }
        Ok(out)
    }
}

/// What the kernel answered, by the KRN.MAN.WASM protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallResult {
    /// Exit 0, stdout starting `{"algorithm":"`: the result envelope bytes.
    Envelope(Vec<u8>),
    /// Exit 1 or 2, stdout starting `{"error":"`: the kernel's typed refusal, verbatim.
    KernelError { exit_code: u32, bytes: Vec<u8> },
}

impl CallResult {
    /// The stdout bytes either way.
    pub fn bytes(&self) -> &[u8] {
        match self {
            CallResult::Envelope(b) | CallResult::KernelError { bytes: b, .. } => b,
        }
    }
}

/// A loaded, digest-checked, import-checked and validated kernel with its limits.
#[derive(Debug, Clone)]
pub struct Sandbox {
    engine: module::Module,
    limits: Limits,
    digest: Digest,
    /// Caller-provided clock; `None` uses [`default_clock`].
    clock: Option<Clock>,
    /// Clock reading at the first call that had a wall-clock budget.
    started: Option<Duration>,
}

impl Sandbox {
    /// Loads `artifact` for the `wasm32-wasip1` entry of `manifest` (REQ-001, REQ-002).
    pub fn load(artifact: &[u8], manifest: &KernelManifest, limits: Limits) -> Result<Sandbox, SandboxError> {
        let expected = manifest.wasm32_wasip1_digest.ok_or_else(|| {
            SandboxError::new(ErrorCode::ArtifactMismatch, "kernel manifest lists no wasm32-wasip1 artifact")
        })?;
        Sandbox::load_with_digest(artifact, &expected, limits)
    }

    /// Loads `artifact` whose SHA-256 must equal `expected`.
    pub fn load_with_digest(artifact: &[u8], expected: &Digest, limits: Limits) -> Result<Sandbox, SandboxError> {
        let actual = Digest::from_bytes(digest_bytes(artifact).0);
        if actual != *expected {
            return Err(SandboxError::new(
                ErrorCode::ArtifactMismatch,
                format!("artifact SHA-256 {actual} differs from the kernel manifest entry {expected}"),
            ));
        }
        if artifact.len() as u64 > limits.max_module_bytes {
            return Err(SandboxError::new(ErrorCode::MalformedModule, "too_large: artifact exceeds max_module_bytes"));
        }
        let engine = module::parse(artifact)?;
        let entry = engine.export_func(ENTRY_EXPORT).and_then(|f| engine.func_type(f));
        if !entry.is_some_and(|t| t.params.is_empty() && t.results.is_empty()) {
            return Err(SandboxError::new(ErrorCode::MalformedModule, "no `_start` export of type [] -> []"));
        }
        Ok(Sandbox { engine, limits, digest: actual, clock: None, started: None })
    }

    /// Uses `clock` for the wall-clock budget instead of the default monotonic clock. Needed
    /// on `wasm32-unknown-unknown`, which has none.
    pub fn with_clock(mut self, clock: Clock) -> Sandbox {
        self.clock = Some(clock);
        self
    }

    /// The verified artifact digest.
    pub fn artifact_digest(&self) -> Digest {
        self.digest
    }

    /// The limits in force.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Runs one kernel call in a fresh instance (REQ-005..REQ-007).
    pub fn call(&mut self, request: &CallRequest) -> Result<CallResult, SandboxError> {
        let stdin = request.canonical_bytes()?;
        self.run_command(&stdin)
    }

    /// Runs the command with `stdin` as its standard input and maps the exit by the
    /// KRN.MAN.WASM runner rules.
    pub fn run_command(&mut self, stdin: &[u8]) -> Result<CallResult, SandboxError> {
        let (code, stdout, stderr) = self.run_raw(stdin)?;
        if stderr > 0 {
            return Err(SandboxError::trap(TrapKind::UnexpectedExit, format!("kernel wrote {stderr} bytes to stderr")));
        }
        if code == 0 && stdout.starts_with(b"{\"algorithm\":\"") {
            return Ok(CallResult::Envelope(stdout));
        }
        if (code == 1 || code == 2) && stdout.starts_with(b"{\"error\":\"") {
            return Ok(CallResult::KernelError { exit_code: code, bytes: stdout });
        }
        Err(SandboxError::trap(
            TrapKind::UnexpectedExit,
            format!("exit code {code} with {} stdout bytes is not a kernel answer", stdout.len()),
        ))
    }

    /// The limits of the next run. Reads the clock only when a wall-clock budget is set.
    fn run_limits(&mut self) -> Result<exec::RunLimits, SandboxError> {
        let deadline = match self.limits.wall_clock {
            None => None,
            Some(budget) => {
                let clock = self.clock.or_else(default_clock).ok_or_else(|| {
                    SandboxError::new(
                        ErrorCode::BadRequest,
                        "a wall-clock budget needs a caller-provided clock on this target (Sandbox::with_clock)",
                    )
                })?;
                let now = clock();
                let started = *self.started.get_or_insert(now);
                match started.checked_add(budget) {
                    Some(at) if now >= at => {
                        return Err(SandboxError::new(ErrorCode::BudgetExhausted, "wall-clock budget exhausted"));
                    }
                    Some(at) => Some(exec::Deadline { clock, at }),
                    None => None,
                }
            }
        };
        Ok(exec::RunLimits { max_depth: self.limits.max_call_depth, max_slots: self.limits.max_stack_slots, deadline })
    }

    fn cap_pages(&self) -> u32 {
        (self.limits.memory_bytes / module::PAGE).min(u64::from(module::MAX_PAGES)) as u32
    }

    /// Instantiates and runs the start function and `_start`: (exit code, stdout, stderr bytes).
    fn run_raw(&mut self, stdin: &[u8]) -> Result<(u32, Vec<u8>, u64), SandboxError> {
        let lim = self.run_limits()?;
        let mut inst = exec::Instance::new(&self.engine, self.cap_pages(), self.limits.fuel_per_call)?;
        let mut host = exec::Host {
            stdin,
            cursor: 0,
            stdout: Vec::new(),
            stderr_bytes: 0,
            max_output: self.limits.max_output_bytes,
        };
        let mut entries = Vec::new();
        entries.extend(self.engine.start);
        entries.extend(self.engine.export_func(ENTRY_EXPORT));
        for f in entries {
            if let exec::End::Exit(code) = inst.invoke(f, &mut host, &lim)? {
                return Ok((code, host.stdout, host.stderr_bytes));
            }
        }
        Ok((0, host.stdout, host.stderr_bytes))
    }
}

#[cfg(test)]
mod tests;

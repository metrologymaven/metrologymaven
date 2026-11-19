//! The pinned runtime settings file (`wasm-runtime.toml`, REQ-KRN.MAN.WASM-004).
//!
//! The file is a small TOML subset read strictly, and must be in canonical form: exactly these
//! ten keys, sorted by byte value, one `key = value` per line with single spaces around `=`,
//! LF line endings including after the last line, no comments, blank lines or other bytes:
//!
//! ```toml
//! clock = "trap"
//! fuel = 10000000000
//! grow_failure = "trap"
//! max_memory_size = 1073741824
//! max_wasm_stack = 1048576
//! nan_canonicalization = true
//! relaxed_simd = false
//! schema = "1"
//! threads = false
//! wasmtime_version = "49.0.6"
//! ```
//!
//! Integers are decimal without leading zeros; strings have no escapes. Because only the
//! canonical form is accepted, the SHA-256 of the file bytes is the SHA-256 of the settings'
//! canonical bytes; it is the kernel manifest's `replay_runtime.config_digest`.
//!
//! Policy (CONSTITUTION §4, the leaf's Scope) is enforced, never configurable: NaN
//! canonicalisation on, relaxed SIMD off, threads off, schema `"1"`, `clock = "trap"` (clock
//! imports are sealed to trap, see [`crate::seal`]) and `grow_failure = "trap"` (a failed
//! `memory.grow` traps in the runtime instead of returning -1 to the guest). The limits must
//! lie in [`STACK_RANGE`], [`MEMORY_RANGE`] and [`FUEL_RANGE`], and the version must be
//! `MAJOR.MINOR.PATCH`.

use std::fmt;

use fid_kci::kci::{sha256_digest, Digest};

/// Largest accepted file, in bytes.
pub const MAX_RUNTIME_BYTES: usize = 4096;
/// Accepted `max_wasm_stack`, in bytes (64 KiB to 16 MiB).
pub const STACK_RANGE: (u64, u64) = (64 * 1024, 16 * 1024 * 1024);
/// Accepted `max_memory_size`, in bytes (one 64 KiB page to the 4 GiB wasm32 address space).
pub const MEMORY_RANGE: (u64, u64) = (64 * 1024, 4 * 1024 * 1024 * 1024);
/// Accepted `fuel` (wasmtime fuel units, about one per wasm instruction).
pub const FUEL_RANGE: (u64, u64) = (1_000_000, 1_000_000_000_000);
/// The only schema this module reads.
pub const SCHEMA: &str = "1";

const KEYS: [&str; 10] = [
    "clock",
    "fuel",
    "grow_failure",
    "max_memory_size",
    "max_wasm_stack",
    "nan_canonicalization",
    "relaxed_simd",
    "schema",
    "threads",
    "wasmtime_version",
];

/// The validated settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    /// Exact wasmtime version, `MAJOR.MINOR.PATCH`.
    pub wasmtime_version: String,
    /// Native stack available to wasm code, in bytes.
    pub max_wasm_stack: u64,
    /// Largest linear memory, in bytes.
    pub max_memory_size: u64,
    /// Fuel per run; running out is a resource limit.
    pub fuel: u64,
    canonical: Vec<u8>,
}

/// Settings errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    TooLarge,
    /// Not UTF-8, a line that is not `key = value`, or a malformed value; `line` is 1-based.
    Syntax {
        line: usize,
    },
    UnknownKey(String),
    DuplicateKey(String),
    MissingKey(&'static str),
    /// A value of the wrong type or outside its range.
    BadValue(&'static str),
    /// A value the policy forbids (for example `relaxed_simd = true`).
    Policy(&'static str),
    /// Valid settings, but not in canonical form; `offset` is the first differing byte.
    NonCanonical {
        offset: usize,
    },
}

impl RuntimeError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            RuntimeError::TooLarge => "too_large",
            RuntimeError::Syntax { .. } => "syntax",
            RuntimeError::UnknownKey(_) => "unknown_key",
            RuntimeError::DuplicateKey(_) => "duplicate_key",
            RuntimeError::MissingKey(_) => "missing_key",
            RuntimeError::BadValue(_) => "bad_value",
            RuntimeError::Policy(_) => "policy",
            RuntimeError::NonCanonical { .. } => "non_canonical",
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            RuntimeError::TooLarge => f.write_str(code),
            RuntimeError::Syntax { line } => write!(f, "{code}: line {line}"),
            RuntimeError::UnknownKey(k) | RuntimeError::DuplicateKey(k) => write!(f, "{code}: {k}"),
            RuntimeError::MissingKey(k) | RuntimeError::BadValue(k) | RuntimeError::Policy(k) => {
                write!(f, "{code}: {k}")
            }
            RuntimeError::NonCanonical { offset } => write!(f, "{code}: at byte {offset}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Int(u64),
    Bool(bool),
    Str(String),
}

fn parse_value(v: &str) -> Option<Value> {
    match v {
        "true" => return Some(Value::Bool(true)),
        "false" => return Some(Value::Bool(false)),
        _ => {}
    }
    if let Some(inner) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        if inner.bytes().all(|c| (0x20..=0x7e).contains(&c) && c != b'"' && c != b'\\') {
            return Some(Value::Str(inner.to_string()));
        }
        return None;
    }
    if v.is_empty() || v.len() > 20 || !v.bytes().all(|c| c.is_ascii_digit()) || (v.len() > 1 && v.starts_with('0')) {
        return None;
    }
    v.parse::<u64>().ok().map(Value::Int)
}

fn is_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 9
                && p.bytes().all(|c| c.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
}

fn get<'a>(vals: &'a [(String, Value)], key: &'static str) -> Result<&'a Value, RuntimeError> {
    vals.iter().find(|(k, _)| k == key).map(|(_, v)| v).ok_or(RuntimeError::MissingKey(key))
}

fn boolean(vals: &[(String, Value)], key: &'static str, required: bool) -> Result<(), RuntimeError> {
    match get(vals, key)? {
        Value::Bool(b) if *b == required => Ok(()),
        Value::Bool(_) => Err(RuntimeError::Policy(key)),
        _ => Err(RuntimeError::BadValue(key)),
    }
}

fn int(vals: &[(String, Value)], key: &'static str, range: (u64, u64)) -> Result<u64, RuntimeError> {
    match get(vals, key)? {
        Value::Int(n) if *n >= range.0 && *n <= range.1 => Ok(*n),
        _ => Err(RuntimeError::BadValue(key)),
    }
}

impl RuntimeConfig {
    /// The canonical bytes of these settings.
    fn render(wasmtime_version: &str, max_wasm_stack: u64, max_memory_size: u64, fuel: u64) -> Vec<u8> {
        format!(
            "clock = \"trap\"\nfuel = {fuel}\ngrow_failure = \"trap\"\n\
             max_memory_size = {max_memory_size}\nmax_wasm_stack = {max_wasm_stack}\nnan_canonicalization = true\n\
             relaxed_simd = false\nschema = \"{SCHEMA}\"\nthreads = false\nwasmtime_version = \"{wasmtime_version}\"\n"
        )
        .into_bytes()
    }

    /// Reads the file and requires it to be in canonical form (see the module docs).
    pub fn parse_canonical(input: &[u8]) -> Result<RuntimeConfig, RuntimeError> {
        if input.len() > MAX_RUNTIME_BYTES {
            return Err(RuntimeError::TooLarge);
        }
        let text = std::str::from_utf8(input).map_err(|_| RuntimeError::Syntax { line: 1 })?;
        let mut vals: Vec<(String, Value)> = Vec::new();
        for (i, raw) in text.split('\n').enumerate() {
            let line = i + 1;
            if raw.is_empty() {
                continue;
            }
            let (k, v) = raw.split_once(" = ").ok_or(RuntimeError::Syntax { line })?;
            if k.is_empty() || !k.bytes().all(|c| c.is_ascii_lowercase() || c == b'_') {
                return Err(RuntimeError::Syntax { line });
            }
            let value = parse_value(v).ok_or(RuntimeError::Syntax { line })?;
            if !KEYS.contains(&k) {
                return Err(RuntimeError::UnknownKey(k.to_string()));
            }
            if vals.iter().any(|(seen, _)| seen == k) {
                return Err(RuntimeError::DuplicateKey(k.to_string()));
            }
            vals.push((k.to_string(), value));
        }
        match get(&vals, "schema")? {
            Value::Str(s) if s == SCHEMA => {}
            Value::Str(_) => return Err(RuntimeError::Policy("schema")),
            _ => return Err(RuntimeError::BadValue("schema")),
        }
        boolean(&vals, "nan_canonicalization", true)?;
        boolean(&vals, "relaxed_simd", false)?;
        boolean(&vals, "threads", false)?;
        for key in ["clock", "grow_failure"] {
            match get(&vals, key)? {
                Value::Str(s) if s == "trap" => {}
                Value::Str(_) => return Err(RuntimeError::Policy(key)),
                _ => return Err(RuntimeError::BadValue(key)),
            }
        }
        let fuel = int(&vals, "fuel", FUEL_RANGE)?;
        let max_wasm_stack = int(&vals, "max_wasm_stack", STACK_RANGE)?;
        let max_memory_size = int(&vals, "max_memory_size", MEMORY_RANGE)?;
        let wasmtime_version = match get(&vals, "wasmtime_version")? {
            Value::Str(s) if is_version(s) => s.clone(),
            _ => return Err(RuntimeError::BadValue("wasmtime_version")),
        };
        let canonical = Self::render(&wasmtime_version, max_wasm_stack, max_memory_size, fuel);
        if canonical != input {
            let shorter = if input.len() < canonical.len() { input.len() } else { canonical.len() };
            let offset = canonical.iter().zip(input).position(|(a, b)| a != b).unwrap_or(shorter);
            return Err(RuntimeError::NonCanonical { offset });
        }
        Ok(RuntimeConfig { wasmtime_version, max_wasm_stack, max_memory_size, fuel, canonical })
    }

    /// The canonical bytes (equal to the file bytes).
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    /// SHA-256 of the canonical bytes: the manifest's `replay_runtime.config_digest`.
    pub fn config_digest(&self) -> Digest {
        sha256_digest(&self.canonical)
    }

    /// The `wasmtime run` arguments that apply these settings to `module` (see the leaf's
    /// Design). The module path must not start with `-`, so it cannot be read as an option.
    pub fn wasmtime_args(&self, module: &str) -> Option<Vec<String>> {
        if module.is_empty() || module.starts_with('-') {
            return None;
        }
        Some(vec![
            "run".to_string(),
            "-C".to_string(),
            "cache=n".to_string(),
            "-W".to_string(),
            "nan-canonicalization=y".to_string(),
            "-W".to_string(),
            "relaxed-simd=n".to_string(),
            "-W".to_string(),
            "threads=n".to_string(),
            "-W".to_string(),
            format!("max-wasm-stack={}", self.max_wasm_stack),
            "-W".to_string(),
            format!("max-memory-size={}", self.max_memory_size),
            "-W".to_string(),
            "trap-on-grow-failure=y".to_string(),
            "-W".to_string(),
            format!("fuel={}", self.fuel),
            "-W".to_string(),
            "unknown-imports-trap=y".to_string(),
            module.to_string(),
        ])
    }
}

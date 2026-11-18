//! Metrology Maven GPU backend policy and known-bad driver list (spec node `PLAT.GPU.POLICY`):
//! the typed schemas of `platform/gpu-policy.toml` and `platform/gpu-known-bad.toml`, their
//! strict, bounded validators, the cross-check of the policy against the support matrix, and
//! the deterministic known-bad lookup that `PLAT.GPU.DETECT` and `fid doctor` use.
//!
//! This data never influences a value of record (CONSTITUTION C2, REQ-PLAT.GPU-008).
//!
//! Determinism D0: pure functions over bytes; tables are `BTreeMap`-backed (the `toml` crate
//! without `preserve_order`), and diagnostics have a fixed order (see [`validate_policy`] and
//! [`validate_known_bad`]).
//!
//! # Matching rules (known-bad list)
//!
//! An entry matches a probed adapter ([`Probe`]) when **all** of these hold:
//! - `os` and `backend` are equal;
//! - `vendor_id` is equal (an exact `u32`; PCI vendor IDs are 16-bit, Vulkan also reports
//!   Khronos vendor IDs above `0xFFFF`, so the full `u32` range is accepted);
//! - the device ID is in `device_ids`: `"any"`, or the **inclusive** range `min..=max`;
//! - the driver version is in `driver`: `"any"`, or the **half-open** range `[min, max)` where
//!   a missing `min` means "from the lowest version" and a missing `max` means "open-ended".
//!
//! Driver versions are dotted numeric [`Version`]s of 1 to 4 `u32` components (the same type
//! as the support matrix); missing trailing components compare as 0, so `560` equals `560.0`.
//! Turning a vendor's own driver string into this form is the caller's job (`PLAT.GPU.DETECT`).
//! [`KnownBad::matching`] returns every matching entry in file order, so the result is the same
//! on every platform and run.

use crate::matrix::{Arch, Matrix, OsFamily, Phase, Tier, Version};
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The only `schema_version` this reader supports (both files).
pub const SUPPORTED_SCHEMA: u32 = 1;
/// Largest accepted file, in bytes (REQ-PLAT.GPU.POLICY-007).
pub const MAX_BYTES: usize = 1024 * 1024;
/// Largest accepted number of `[[row]]` or `[[entry]]` items (REQ-PLAT.GPU.POLICY-007).
pub const MAX_ENTRIES: usize = 10_000;
/// Validation stops collecting diagnostics after this many and appends `TooManyErrors`.
pub const MAX_ERRORS: usize = 1000;
/// Longest accepted known-bad `id`, in bytes.
pub const MAX_ID: usize = 64;
/// Longest accepted `workaround:<name>` name, in bytes.
pub const MAX_WORKAROUND: usize = 64;
/// Longest accepted `reason`, in bytes.
pub const MAX_REASON: usize = 1024;
/// Longest accepted `reference` URL, in bytes.
pub const MAX_REFERENCE: usize = 2048;
/// Path of the shipped policy file, relative to the repository root.
pub const POLICY_PATH: &str = "platform/gpu-policy.toml";
/// Path of the shipped known-bad list, relative to the repository root.
pub const KNOWN_BAD_PATH: &str = "platform/gpu-known-bad.toml";
/// Path of the shipped support matrix, relative to the repository root.
pub const MATRIX_PATH: &str = "platform/support-matrix.toml";
/// Phase used by `plat-gpu validate` when `--phase` is not given (the node's phase).
pub const DEFAULT_PHASE: Phase = Phase::P1a;

const POLICY_TOP: [&str; 2] = ["row", "schema_version"];
const ROW_KEYS: [&str; 4] = ["arch", "backends", "min", "os"];
const MIN_KEYS: [&str; 2] = ["api", "driver"];
const BAD_TOP: [&str; 2] = ["entry", "schema_version"];
const ENTRY_KEYS: [&str; 9] =
    ["action", "backend", "device_ids", "driver", "id", "os", "reason", "reference", "vendor_id"];
const RANGE_KEYS: [&str; 2] = ["max", "min"];

macro_rules! named_enum {
    ($(#[$m:meta])* $name:ident { $($var:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name { $($var),+ }
        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$var),+];
            /// The name used in the files.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$var => $s),+ }
            }
            /// Parses the name used in the files.
            pub fn from_name(s: &str) -> Option<Self> {
                match s { $($s => Some($name::$var),)+ _ => None }
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

named_enum!(
    /// Operating system as the GPU policy sees it. Every Linux family of the support matrix
    /// (`ubuntu`, `debian`, `fedora`, `rhel`) maps to `linux`; see [`GpuOs::from_family`].
    GpuOs { Windows = "windows", Linux = "linux", Macos = "macos" }
);
named_enum!(
    /// A GPU backend (REQ-PLAT.GPU.POLICY-001).
    Backend { Vulkan = "vulkan", Metal = "metal", Dx12 = "dx12", Software = "software" }
);

impl GpuOs {
    /// The policy OS of a support-matrix OS family.
    pub fn from_family(f: OsFamily) -> GpuOs {
        match f {
            OsFamily::Windows => GpuOs::Windows,
            OsFamily::Ubuntu | OsFamily::Debian | OsFamily::Fedora | OsFamily::Rhel => GpuOs::Linux,
            OsFamily::Macos => GpuOs::Macos,
        }
    }

    /// Backends every row of this OS must list (REQ-PLAT.GPU-002, REQ-PLAT.GPU.POLICY-003),
    /// in [`Backend::ALL`] order.
    pub fn mandatory(self) -> &'static [Backend] {
        match self {
            GpuOs::Windows => &[Backend::Vulkan, Backend::Dx12],
            GpuOs::Linux => &[Backend::Vulkan],
            GpuOs::Macos => &[Backend::Metal],
        }
    }
}

/// Minimum API version of a backend (for DX12 the feature level, `11_0` written `11.0`) and an
/// optional minimum driver version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Minimum {
    pub api: Version,
    pub driver: Option<Version>,
}

/// One policy row: the backends tried, in order, on one OS and architecture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRow {
    pub os: GpuOs,
    pub arch: Arch,
    /// Tried first to last; distinct, ends with `software`, contains the OS's mandatory backends.
    pub backends: Vec<Backend>,
    /// One entry per hardware backend in `backends` (optional for `software`).
    pub min: BTreeMap<Backend, Minimum>,
}

/// A validated policy file: no row errors and at most one row per (os, arch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuPolicy {
    pub schema_version: u32,
    pub rows: Vec<PolicyRow>,
}

impl GpuPolicy {
    /// The row for `os` on `arch`, if any.
    pub fn row(&self, os: GpuOs, arch: Arch) -> Option<&PolicyRow> {
        self.rows.iter().find(|r| r.os == os && r.arch == arch)
    }
}

/// Device IDs a known-bad entry covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdRange {
    Any,
    /// Inclusive: `min..=max`, `min <= max`.
    Range {
        min: u32,
        max: u32,
    },
}

impl IdRange {
    /// Whether `id` is covered.
    pub fn contains(&self, id: u32) -> bool {
        match self {
            IdRange::Any => true,
            IdRange::Range { min, max } => *min <= id && id <= *max,
        }
    }
}

/// Driver versions a known-bad entry covers: half-open `[min, max)`; `None` bounds are open.
/// Both `None` is `"any"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionRange {
    pub min: Option<Version>,
    pub max: Option<Version>,
}

impl VersionRange {
    /// Whether `v` is covered.
    pub fn contains(&self, v: &Version) -> bool {
        self.min.is_none_or(|m| *v >= m) && self.max.is_none_or(|m| *v < m)
    }
}

/// What to do when an entry matches (REQ-PLAT.GPU.POLICY-005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadAction {
    /// Skip this backend and try the next one in the policy row.
    AvoidBackend,
    /// Go straight to the `software` backend.
    ForceSoftware,
    /// Keep the backend and apply the named workaround (`[a-z][a-z0-9_]*`, at most 64 bytes).
    Workaround(String),
}

impl BadAction {
    /// Parses `avoid_backend`, `force_software` or `workaround:<name>`.
    pub fn parse(s: &str) -> Option<BadAction> {
        match s {
            "avoid_backend" => Some(BadAction::AvoidBackend),
            "force_software" => Some(BadAction::ForceSoftware),
            _ => {
                let name = s.strip_prefix("workaround:")?;
                let mut bytes = name.bytes();
                let first_ok = bytes.next().is_some_and(|b| b.is_ascii_lowercase());
                let rest_ok = bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
                (first_ok && rest_ok && name.len() <= MAX_WORKAROUND).then(|| BadAction::Workaround(name.to_string()))
            }
        }
    }
}

impl fmt::Display for BadAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BadAction::AvoidBackend => f.write_str("avoid_backend"),
            BadAction::ForceSoftware => f.write_str("force_software"),
            BadAction::Workaround(n) => write!(f, "workaround:{n}"),
        }
    }
}

/// One known-bad entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadEntry {
    /// Unique within the file; `[A-Za-z0-9._-]`, 1 to 64 bytes.
    pub id: String,
    pub os: GpuOs,
    pub backend: Backend,
    pub vendor_id: u32,
    pub device_ids: IdRange,
    pub driver: VersionRange,
    pub action: BadAction,
    /// Non-empty, at most 1024 bytes.
    pub reason: String,
    /// An `https://` URL, at most 2048 bytes.
    pub reference: String,
}

/// A probed adapter, as `PLAT.GPU.DETECT` reports it, to look up in the known-bad list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    pub os: GpuOs,
    pub backend: Backend,
    pub vendor_id: u32,
    pub device_id: u32,
    pub driver: Version,
}

impl BadEntry {
    /// Whether this entry applies to `p` (see the module documentation for the rules).
    pub fn matches(&self, p: &Probe) -> bool {
        self.os == p.os
            && self.backend == p.backend
            && self.vendor_id == p.vendor_id
            && self.device_ids.contains(p.device_id)
            && self.driver.contains(&p.driver)
    }
}

/// A validated known-bad list: no entry errors and unique IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownBad {
    pub schema_version: u32,
    pub entries: Vec<BadEntry>,
}

impl KnownBad {
    /// Every entry that applies to `p`, in file order.
    pub fn matching(&self, p: &Probe) -> Vec<&BadEntry> {
        self.entries.iter().filter(|e| e.matches(p)).collect()
    }
}

/// Which bound `TooLarge` hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    Bytes,
    Entries,
}

/// A typed policy or known-bad error. Row and entry numbers are 1-based positions among the
/// `[[row]]` or `[[entry]]` items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuPolicyError {
    /// Fatal: the file exceeds [`MAX_BYTES`] or [`MAX_ENTRIES`].
    TooLarge { limit: Limit, found: usize, max: usize },
    /// Fatal: `schema_version` is above [`SUPPORTED_SCHEMA`].
    UnsupportedSchema { found: u64 },
    /// Fatal: not UTF-8 or not TOML. `line` is 1-based when the parser reports a position.
    Parse { line: Option<usize>, msg: String },
    /// A structural problem: an unknown key, a wrong type or value, or a missing required field
    /// that has no more specific error. `item` is `row N` or `entry N`, `None` for top-level keys.
    BadField { item: Option<String>, field: String, msg: String },
    /// Rows `a` < `b` are both for `os` on `arch`.
    DuplicateRow { a: usize, b: usize, os: GpuOs, arch: Arch },
    /// An effective Tier 1 or Tier 2 combination of the support matrix has no policy row.
    NoPolicyRow { os: GpuOs, arch: Arch },
    /// A row omits a backend its OS requires (REQ-PLAT.GPU.POLICY-003).
    MissingMandatoryBackend { row: usize, os: GpuOs, arch: Arch, backend: Backend },
    /// A row does not end with `software` (REQ-PLAT.GPU.POLICY-004).
    NoSoftwareFallback { row: usize, os: Option<GpuOs>, arch: Option<Arch> },
    /// A row lists `backend` more than once.
    DuplicateBackend { row: usize, backend: Backend },
    /// A known-bad entry has no `reference` `https://` URL. `id` is the entry's `id`, or
    /// `entry N` when the entry has no usable `id`.
    MissingReference { id: String },
    /// A known-bad entry has no non-empty `reason`.
    MissingReason { id: String },
    /// A second entry uses an `id` already taken.
    DuplicateId { id: String },
    /// `device_ids` has `min > max`, or `driver` has `max <= min`.
    BadRange { id: String, field: &'static str },
    /// Validation stopped after [`MAX_ERRORS`] diagnostics.
    TooManyErrors { limit: usize },
}

fn opt<T: fmt::Display>(v: &Option<T>) -> String {
    v.as_ref().map_or_else(|| "?".to_string(), ToString::to_string)
}

impl fmt::Display for GpuPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuPolicyError::TooLarge { limit, found, max } => {
                let unit = match limit {
                    Limit::Bytes => "bytes",
                    Limit::Entries => "entries",
                };
                write!(f, "TooLarge: {found} {unit} exceeds the limit of {max}")
            }
            GpuPolicyError::UnsupportedSchema { found } => {
                write!(f, "UnsupportedSchema: schema_version {found} is above the supported version {SUPPORTED_SCHEMA}")
            }
            GpuPolicyError::Parse { line: Some(l), msg } => write!(f, "Parse: line {l}: {msg}"),
            GpuPolicyError::Parse { line: None, msg } => write!(f, "Parse: {msg}"),
            GpuPolicyError::BadField { item: Some(i), field, msg } => write!(f, "BadField: {i}: {field} {msg}"),
            GpuPolicyError::BadField { item: None, field, msg } => write!(f, "BadField: {field} {msg}"),
            GpuPolicyError::DuplicateRow { a, b, os, arch } => {
                write!(f, "DuplicateRow {{ os: {os}, arch: {arch} }}: rows {a} and {b}")
            }
            GpuPolicyError::NoPolicyRow { os, arch } => write!(f, "NoPolicyRow {{ os: {os}, arch: {arch} }}"),
            GpuPolicyError::MissingMandatoryBackend { row, os, arch, backend } => {
                write!(f, "MissingMandatoryBackend {{ os: {os}, backend: {backend} }}: row {row} ({os} {arch})")
            }
            GpuPolicyError::NoSoftwareFallback { row, os, arch } => {
                write!(f, "NoSoftwareFallback: row {row} ({} {}) does not end with software", opt(os), opt(arch))
            }
            GpuPolicyError::DuplicateBackend { row, backend } => {
                write!(f, "DuplicateBackend {{ backend: {backend} }}: row {row}")
            }
            GpuPolicyError::MissingReference { id } => write!(f, "MissingReference {{ id: {id} }}"),
            GpuPolicyError::MissingReason { id } => write!(f, "MissingReason {{ id: {id} }}"),
            GpuPolicyError::DuplicateId { id } => write!(f, "DuplicateId {{ id: {id} }}"),
            GpuPolicyError::BadRange { id, field } => write!(f, "BadRange {{ id: {id}, field: {field} }}"),
            GpuPolicyError::TooManyErrors { limit } => write!(f, "TooManyErrors: stopped after {limit} errors"),
        }
    }
}

impl std::error::Error for GpuPolicyError {}

/// The result of validating one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validation<T> {
    /// Number of `[[row]]` or `[[entry]]` items (0 when a fatal error stopped parsing).
    pub items: usize,
    /// Diagnostics in the documented order. A fatal error is the only entry.
    pub errors: Vec<GpuPolicyError>,
    /// The validated file, present only when `errors` is empty.
    pub value: Option<T>,
}

impl<T> Validation<T> {
    /// One diagnostic per line, then `<label>: <items> <noun>, <errors> errors`.
    /// Byte-identical for identical input.
    pub fn render(&self, label: &str, noun: &str) -> String {
        let mut out = String::new();
        for e in &self.errors {
            out.push_str(&e.to_string());
            out.push('\n');
        }
        out.push_str(&format!("{label}: {} {noun}, {} errors\n", self.items, self.errors.len()));
        out
    }

    fn fatal(e: GpuPolicyError) -> Validation<T> {
        Validation { items: 0, errors: vec![e], value: None }
    }
}

/// Counts `[[<name>]]` headers without parsing, so oversized files fail before a full parse.
/// Lines inside multi-line strings can only make the count larger, which fails closed.
fn prescan(text: &str, name: &str) -> usize {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            match t.strip_prefix("[[").map(str::trim_start).and_then(|r| r.strip_prefix(name)) {
                Some(r) => r.trim_start().starts_with("]]"),
                None => false,
            }
        })
        .count()
}

fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes().iter().take(offset).filter(|&&b| b == b'\n').count() + 1
}

fn bad(item: Option<&str>, field: &str, msg: impl Into<String>) -> GpuPolicyError {
    GpuPolicyError::BadField { item: item.map(str::to_string), field: field.to_string(), msg: msg.into() }
}

/// The shared front end: size guard, UTF-8, entry prescan, TOML parse, `schema_version`,
/// unknown top-level keys and the item array. `Err` is a fatal error.
fn load(
    bytes: &[u8],
    array: &str,
    top_keys: &[&str],
    errors: &mut Vec<GpuPolicyError>,
) -> Result<Vec<toml::Value>, GpuPolicyError> {
    if bytes.len() > MAX_BYTES {
        return Err(GpuPolicyError::TooLarge { limit: Limit::Bytes, found: bytes.len(), max: MAX_BYTES });
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Err(GpuPolicyError::Parse { line: None, msg: "file is not UTF-8".into() });
    };
    let pre = prescan(text, array);
    if pre > MAX_ENTRIES {
        return Err(GpuPolicyError::TooLarge { limit: Limit::Entries, found: pre, max: MAX_ENTRIES });
    }
    let mut table = match text.parse::<toml::Table>() {
        Ok(t) => t,
        Err(e) => {
            let line = e.span().map(|s| line_of(text, s.start));
            return Err(GpuPolicyError::Parse { line, msg: e.message().trim_end().to_string() });
        }
    };
    match table.get("schema_version") {
        None => errors.push(bad(None, "schema_version", "is missing")),
        Some(toml::Value::Integer(n)) if *n >= 1 => {
            let n = n.unsigned_abs();
            if n > u64::from(SUPPORTED_SCHEMA) {
                return Err(GpuPolicyError::UnsupportedSchema { found: n });
            }
        }
        Some(_) => errors.push(bad(None, "schema_version", "is not a positive integer")),
    }
    for k in table.keys() {
        if !top_keys.contains(&k.as_str()) {
            errors.push(bad(None, k, "is not a known key"));
        }
    }
    let items = match table.remove(array) {
        None => Vec::new(),
        Some(toml::Value::Array(a)) => a,
        Some(_) => {
            errors.push(bad(None, array, "is not an array of tables"));
            Vec::new()
        }
    };
    if items.len() > MAX_ENTRIES {
        return Err(GpuPolicyError::TooLarge { limit: Limit::Entries, found: items.len(), max: MAX_ENTRIES });
    }
    Ok(items)
}

fn cap(errors: &mut Vec<GpuPolicyError>) {
    if errors.len() > MAX_ERRORS {
        errors.truncate(MAX_ERRORS);
        errors.push(GpuPolicyError::TooManyErrors { limit: MAX_ERRORS });
    }
}

fn unknown_keys(t: &toml::Table, allowed: &[&str], item: &str, prefix: &str, errors: &mut Vec<GpuPolicyError>) {
    for k in t.keys() {
        if !allowed.contains(&k.as_str()) {
            errors.push(bad(Some(item), &format!("{prefix}{k}"), "is not a known key"));
        }
    }
}

/// A required string field parsed by `parse`; pushes `BadField` and returns `None` on failure.
fn named<T>(
    t: &toml::Table,
    key: &str,
    item: &str,
    expected: &str,
    parse: impl Fn(&str) -> Option<T>,
    errors: &mut Vec<GpuPolicyError>,
) -> Option<T> {
    match t.get(key) {
        None => {
            errors.push(bad(Some(item), key, "is missing"));
            None
        }
        Some(toml::Value::String(s)) => {
            let v = parse(s);
            if v.is_none() {
                errors.push(bad(Some(item), key, format!("is not {expected}")));
            }
            v
        }
        Some(_) => {
            errors.push(bad(Some(item), key, format!("is not {expected}")));
            None
        }
    }
}

const OSES: &str = "one of windows, linux, macos";
const ARCHES: &str = "one of x86_64, aarch64";
const BACKENDS: &str = "one of vulkan, metal, dx12, software";
const VERSIONS: &str = "a dotted numeric version of 1 to 4 u32 components";
const IDS: &str = "an id of 1 to 64 characters from A-Z a-z 0-9 . _ -";
const ACTIONS: &str = "avoid_backend, force_software or workaround:<name> with name [a-z][a-z0-9_]*";
const NOT_U32: &str = "is not an integer in 0..=0xFFFFFFFF";

/// Validates a policy file (REQ-PLAT.GPU.POLICY-001, -003, -004, -007). Fatal errors
/// (`TooLarge`, `UnsupportedSchema`, `Parse`) stop at once with no partial result. The
/// support-matrix cross-check (REQ-PLAT.GPU.POLICY-002) is [`check_coverage`].
///
/// Order: top-level field errors; then per row, in row order: unknown keys, `os`, `arch`,
/// `backends` (type, then `DuplicateBackend` in list order), `min` (in backend-name order),
/// `min` against `backends` (missing minimums in list order, then extra minimums),
/// `MissingMandatoryBackend` (in [`Backend::ALL`] order), `NoSoftwareFallback`; then
/// `DuplicateRow` in order of the later row.
pub fn validate_policy(bytes: &[u8]) -> Validation<GpuPolicy> {
    let mut errors = Vec::new();
    let items = match load(bytes, "row", &POLICY_TOP, &mut errors) {
        Ok(i) => i,
        Err(e) => return Validation::fatal(e),
    };
    let mut good = Vec::new();
    let mut first: BTreeMap<RowKey, usize> = BTreeMap::new();
    let mut dups = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let n = i + 1;
        let toml::Value::Table(t) = item else {
            errors.push(bad(Some(&format!("row {n}")), "row", "is not a table"));
            continue;
        };
        let (key, row) = check_row(n, t, &mut errors);
        if let Some(k) = key {
            match first.entry(k) {
                Entry::Occupied(o) => dups.push(GpuPolicyError::DuplicateRow { a: *o.get(), b: n, os: k.0, arch: k.1 }),
                Entry::Vacant(v) => {
                    v.insert(n);
                }
            }
        }
        if let Some(r) = row {
            good.push(r);
        }
        if errors.len() > MAX_ERRORS {
            break;
        }
    }
    errors.extend(dups);
    cap(&mut errors);
    let value = errors.is_empty().then_some(GpuPolicy { schema_version: SUPPORTED_SCHEMA, rows: good });
    Validation { items: items.len(), errors, value }
}

type RowKey = (GpuOs, Arch);

fn check_row(n: usize, t: &toml::Table, errors: &mut Vec<GpuPolicyError>) -> (Option<RowKey>, Option<PolicyRow>) {
    let item = format!("row {n}");
    let before = errors.len();
    unknown_keys(t, &ROW_KEYS, &item, "", errors);
    let os = named(t, "os", &item, OSES, GpuOs::from_name, errors);
    let arch = named(t, "arch", &item, ARCHES, Arch::from_name, errors);

    let backends: Option<Vec<Backend>> = match t.get("backends") {
        None => {
            errors.push(bad(Some(&item), "backends", "is missing"));
            None
        }
        Some(toml::Value::Array(a)) => {
            let parsed: Option<Vec<Backend>> = a.iter().map(|v| v.as_str().and_then(Backend::from_name)).collect();
            if parsed.is_none() {
                errors.push(bad(Some(&item), "backends", format!("has an element that is not {BACKENDS}")));
            }
            parsed
        }
        Some(_) => {
            errors.push(bad(Some(&item), "backends", "is not an array of backend names"));
            None
        }
    };
    if let Some(list) = &backends {
        let mut seen = BTreeSet::new();
        let mut reported = BTreeSet::new();
        for b in list {
            if !seen.insert(*b) && reported.insert(*b) {
                errors.push(GpuPolicyError::DuplicateBackend { row: n, backend: *b });
            }
        }
    }

    let mut min = BTreeMap::new();
    let mut min_ok = true;
    match t.get("min") {
        None => {}
        Some(toml::Value::Table(m)) => {
            for (k, v) in m {
                let field = format!("min.{k}");
                let Some(b) = Backend::from_name(k) else {
                    errors.push(bad(Some(&item), &field, format!("is not {BACKENDS}")));
                    min_ok = false;
                    continue;
                };
                match check_minimum(v, &item, &field, errors) {
                    Some(m) => {
                        min.insert(b, m);
                    }
                    None => min_ok = false,
                }
            }
        }
        Some(_) => {
            errors.push(bad(Some(&item), "min", "is not a table of backend minimums"));
            min_ok = false;
        }
    }

    if let Some(list) = &backends {
        if min_ok {
            let mut done = BTreeSet::new();
            for b in list {
                if *b != Backend::Software && done.insert(*b) && !min.contains_key(b) {
                    errors.push(bad(Some(&item), &format!("min.{b}"), "is missing"));
                }
            }
            for b in min.keys() {
                if !list.contains(b) {
                    errors.push(bad(Some(&item), &format!("min.{b}"), "names a backend that is not in backends"));
                }
            }
        }
        if let (Some(os), Some(arch)) = (os, arch) {
            for b in os.mandatory() {
                if !list.contains(b) {
                    errors.push(GpuPolicyError::MissingMandatoryBackend { row: n, os, arch, backend: *b });
                }
            }
        }
        if list.last() != Some(&Backend::Software) {
            errors.push(GpuPolicyError::NoSoftwareFallback { row: n, os, arch });
        }
    }

    let key = os.zip(arch);
    if errors.len() != before {
        return (key, None);
    }
    match (os, arch, backends) {
        (Some(os), Some(arch), Some(backends)) => (key, Some(PolicyRow { os, arch, backends, min })),
        _ => (key, None),
    }
}

/// An optional version field: `Ok(None)` when absent, `Err(())` (after pushing `BadField`) when
/// unusable.
fn version_field(
    t: &toml::Table,
    key: &str,
    item: &str,
    field: &str,
    errors: &mut Vec<GpuPolicyError>,
) -> Result<Option<Version>, ()> {
    match t.get(key) {
        None => Ok(None),
        Some(toml::Value::String(s)) => match Version::parse(s) {
            Some(v) => Ok(Some(v)),
            None => {
                errors.push(bad(Some(item), field, format!("is not {VERSIONS}")));
                Err(())
            }
        },
        Some(_) => {
            errors.push(bad(Some(item), field, format!("is not {VERSIONS}")));
            Err(())
        }
    }
}

fn check_minimum(v: &toml::Value, item: &str, field: &str, errors: &mut Vec<GpuPolicyError>) -> Option<Minimum> {
    let toml::Value::Table(t) = v else {
        errors.push(bad(Some(item), field, "is not a table with api and optional driver"));
        return None;
    };
    let before = errors.len();
    unknown_keys(t, &MIN_KEYS, item, &format!("{field}."), errors);
    let api = version_field(t, "api", item, &format!("{field}.api"), errors);
    if let Ok(None) = api {
        errors.push(bad(Some(item), &format!("{field}.api"), "is missing"));
    }
    let driver = version_field(t, "driver", item, &format!("{field}.driver"), errors);
    if errors.len() != before {
        return None;
    }
    match (api, driver) {
        (Ok(Some(api)), Ok(driver)) => Some(Minimum { api, driver }),
        _ => None,
    }
}

/// REQ-PLAT.GPU.POLICY-002: every support-matrix row whose tier is Tier 1 or Tier 2 and
/// effective at `phase` (`phase >= since_phase`) needs a policy row for its OS (via
/// [`GpuOs::from_family`]) and architecture. Every OS family of matrix schema 1 is a desktop
/// OS. Returns one `NoPolicyRow` per missing (os, arch), sorted by ([`GpuOs`], [`Arch`]).
pub fn check_coverage(policy: &GpuPolicy, matrix: &Matrix, phase: Phase) -> Vec<GpuPolicyError> {
    let mut missing = BTreeSet::new();
    for r in &matrix.rows {
        let effective = matches!(r.tier, Tier::Tier1 | Tier::Tier2) && phase >= r.since_phase;
        let os = GpuOs::from_family(r.os);
        if effective && policy.row(os, r.arch).is_none() {
            missing.insert((os, r.arch));
        }
    }
    missing.into_iter().map(|(os, arch)| GpuPolicyError::NoPolicyRow { os, arch }).collect()
}

/// Validates a known-bad list (REQ-PLAT.GPU.POLICY-005, -006, -007). Fatal errors stop at once.
///
/// Order: top-level field errors; then per entry, in entry order: unknown keys, `id`, `os`,
/// `backend`, `vendor_id`, `device_ids`, `driver`, `action`, `reason` (`MissingReason`),
/// `reference` (`MissingReference`), `DuplicateId`.
pub fn validate_known_bad(bytes: &[u8]) -> Validation<KnownBad> {
    let mut errors = Vec::new();
    let items = match load(bytes, "entry", &BAD_TOP, &mut errors) {
        Ok(i) => i,
        Err(e) => return Validation::fatal(e),
    };
    let mut good = Vec::new();
    let mut ids = BTreeSet::new();
    for (i, item) in items.iter().enumerate() {
        let n = i + 1;
        let toml::Value::Table(t) = item else {
            errors.push(bad(Some(&format!("entry {n}")), "entry", "is not a table"));
            continue;
        };
        if let Some(e) = check_entry(n, t, &mut ids, &mut errors) {
            good.push(e);
        }
        if errors.len() > MAX_ERRORS {
            break;
        }
    }
    cap(&mut errors);
    let value = errors.is_empty().then_some(KnownBad { schema_version: SUPPORTED_SCHEMA, entries: good });
    Validation { items: items.len(), errors, value }
}

fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_ID
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

/// An `https://` URL with a non-empty host and no whitespace or control characters.
fn valid_reference(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    s.len() <= MAX_REFERENCE && !host.is_empty() && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn u32_of(v: &toml::Value) -> Option<u32> {
    match v {
        toml::Value::Integer(n) => u32::try_from(*n).ok(),
        _ => None,
    }
}

fn check_entry(
    n: usize,
    t: &toml::Table,
    ids: &mut BTreeSet<String>,
    errors: &mut Vec<GpuPolicyError>,
) -> Option<BadEntry> {
    let item = format!("entry {n}");
    let before = errors.len();
    unknown_keys(t, &ENTRY_KEYS, &item, "", errors);
    let id = named(t, "id", &item, IDS, |s| valid_id(s).then(|| s.to_string()), errors);
    let label = id.clone().unwrap_or_else(|| item.clone());
    let os = named(t, "os", &item, OSES, GpuOs::from_name, errors);
    let backend = named(t, "backend", &item, BACKENDS, Backend::from_name, errors);
    let vendor_id = match t.get("vendor_id") {
        None => {
            errors.push(bad(Some(&item), "vendor_id", "is missing"));
            None
        }
        Some(v) => {
            let id = u32_of(v);
            if id.is_none() {
                errors.push(bad(Some(&item), "vendor_id", NOT_U32));
            }
            id
        }
    };
    let device_ids = match t.get("device_ids") {
        None => {
            errors.push(bad(Some(&item), "device_ids", "is missing (write \"any\" for every device)"));
            None
        }
        Some(toml::Value::String(s)) if s == "any" => Some(IdRange::Any),
        Some(toml::Value::Table(r)) => {
            let b = errors.len();
            unknown_keys(r, &RANGE_KEYS, &item, "device_ids.", errors);
            let mut bound = |k: &str| match r.get(k) {
                None => {
                    errors.push(bad(Some(&item), &format!("device_ids.{k}"), "is missing"));
                    None
                }
                Some(v) => {
                    let x = u32_of(v);
                    if x.is_none() {
                        errors.push(bad(Some(&item), &format!("device_ids.{k}"), NOT_U32));
                    }
                    x
                }
            };
            let lo = bound("min");
            let hi = bound("max");
            match (lo, hi) {
                (Some(min), Some(max)) if errors.len() == b => {
                    if min > max {
                        errors.push(GpuPolicyError::BadRange { id: label.clone(), field: "device_ids" });
                        None
                    } else {
                        Some(IdRange::Range { min, max })
                    }
                }
                _ => None,
            }
        }
        Some(_) => {
            errors.push(bad(Some(&item), "device_ids", "is not \"any\" or a table { min, max }"));
            None
        }
    };
    let driver = match t.get("driver") {
        None => {
            errors.push(bad(Some(&item), "driver", "is missing (write \"any\" for every version)"));
            None
        }
        Some(toml::Value::String(s)) if s == "any" => Some(VersionRange { min: None, max: None }),
        Some(toml::Value::Table(r)) => {
            let b = errors.len();
            unknown_keys(r, &RANGE_KEYS, &item, "driver.", errors);
            let lo = version_field(r, "min", &item, "driver.min", errors);
            let hi = version_field(r, "max", &item, "driver.max", errors);
            match (lo, hi) {
                (Ok(None), Ok(None)) => {
                    errors.push(bad(Some(&item), "driver", "has neither min nor max (write \"any\" instead)"));
                    None
                }
                (Ok(min), Ok(max)) if errors.len() == b => {
                    if matches!((min, max), (Some(lo), Some(hi)) if hi <= lo) {
                        errors.push(GpuPolicyError::BadRange { id: label.clone(), field: "driver" });
                        None
                    } else {
                        Some(VersionRange { min, max })
                    }
                }
                _ => None,
            }
        }
        Some(_) => {
            errors.push(bad(Some(&item), "driver", "is not \"any\" or a table { min, max }"));
            None
        }
    };
    let action = named(t, "action", &item, ACTIONS, BadAction::parse, errors);
    let reason = match t.get("reason") {
        Some(toml::Value::String(s)) if s.len() > MAX_REASON => {
            errors.push(bad(Some(&item), "reason", "is longer than 1024 bytes"));
            None
        }
        Some(toml::Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
        _ => {
            errors.push(GpuPolicyError::MissingReason { id: label.clone() });
            None
        }
    };
    let reference = match t.get("reference") {
        Some(toml::Value::String(s)) if valid_reference(s) => Some(s.clone()),
        _ => {
            errors.push(GpuPolicyError::MissingReference { id: label.clone() });
            None
        }
    };
    if let Some(i) = &id {
        if !ids.insert(i.clone()) {
            errors.push(GpuPolicyError::DuplicateId { id: i.clone() });
        }
    }
    let entry = match (id, os, backend, vendor_id, device_ids, driver, action, reason, reference) {
        (
            Some(id),
            Some(os),
            Some(backend),
            Some(vendor_id),
            Some(device_ids),
            Some(driver),
            Some(action),
            Some(reason),
            Some(reference),
        ) => Some(BadEntry { id, os, backend, vendor_id, device_ids, driver, action, reason, reference }),
        _ => None,
    };
    finish(errors, before, entry)
}

fn finish<T>(errors: &[GpuPolicyError], before: usize, v: Option<T>) -> Option<T> {
    if errors.len() == before {
        v
    } else {
        None
    }
}

/// Parses and validates a policy file without the matrix cross-check, returning the first error.
pub fn parse_policy(bytes: &[u8]) -> Result<GpuPolicy, GpuPolicyError> {
    let v = validate_policy(bytes);
    match (v.value, v.errors.into_iter().next()) {
        (Some(p), None) => Ok(p),
        (_, Some(e)) => Err(e),
        (None, None) => Err(GpuPolicyError::Parse { line: None, msg: "no policy".into() }),
    }
}

/// Parses and validates a known-bad list, returning the first error.
pub fn parse_known_bad(bytes: &[u8]) -> Result<KnownBad, GpuPolicyError> {
    let v = validate_known_bad(bytes);
    match (v.value, v.errors.into_iter().next()) {
        (Some(k), None) => Ok(k),
        (_, Some(e)) => Err(e),
        (None, None) => Err(GpuPolicyError::Parse { line: None, msg: "no known-bad list".into() }),
    }
}

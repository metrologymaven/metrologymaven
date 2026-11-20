//! Kernel manifest: schema, canonical encoding and digests. Spec node `KRN.MAN.MANI.SCHEMA`.
//!
//! A [`KernelManifest`] is the kernel's identity: a [`ManifestCore`] (every field except the
//! artifact hashes), the SHA-256 digest of the core's canonical JSON, and the artifacts with
//! their hashes. Its digest, [`KernelManifest::manifest_digest`], is SHA-256 over the RFC 8785
//! canonical JSON of the whole manifest, written by the kernel's canonical JSON writer
//! ([`crate::kci::json`]); the core digest is the same over the core alone.
//!
//! Conventions (REQ-KRN.MAN.MANI.SCHEMA-001): every digest is the shared `fid_types::Digest`
//! wire form, `sha256:` followed by 64 lowercase hex digits; every number is a string
//! (`schema_version` is `"1"`); `native_gate` is `measured-only` or `gating`. Algorithms are
//! sorted by the [`AlgorithmId`] total order (module, feature, method, then numeric version) and
//! artifacts by `(role, target)`, both strictly, so a repeated entry is also `Unsorted`.
//!
//! [`KernelManifest::parse`] is a hand-written reader for this schema only. It is bounded
//! (4 MiB input, fixed nesting, capped list lengths) and fails closed: unknown fields,
//! duplicate keys, non-string values, malformed digests, unsorted lists and a `core_digest`
//! that does not match the core are each a typed [`ManifestError`]. Whitespace and member
//! order in the input are free; parsing canonical bytes and writing them again reproduces them
//! exactly (REQ-KRN.MAN.MANI.SCHEMA-004).
//!
//! Two reading modes. [`KernelManifest::parse`] is lenient about formatting and every digest
//! it reports is over the canonical re-encoding, never over the input bytes.
//! [`KernelManifest::parse_canonical`] additionally requires the input bytes to be exactly
//! that re-encoding (`NonCanonical` otherwise), so the digest of the file and the digest of the
//! manifest coincide; verifiers and `TRUST` use the strict mode.
//!
//! `schemas/kernel-manifest.schema.json` states the same rules as a JSON Schema; the
//! hand-labelled fixtures under `fixtures/man/manifest/` are checked against both.

use std::cmp::Ordering;
use std::fmt;

use crate::cards::AlgorithmId;
use crate::kci::json::{to_canonical, Json};
use crate::kci::{sha256_digest, Digest, RESERVED_MODULE};

/// The only schema version this module reads and writes.
pub const SCHEMA_VERSION: &str = "1";
/// Largest accepted input, in bytes (REQ-KRN.MAN.MANI.SCHEMA-003).
pub const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;
/// Most algorithms one manifest may list.
pub const MAX_ALGORITHMS: usize = 4096;
/// Most artifacts one manifest may list.
pub const MAX_ARTIFACTS: usize = 256;
/// Longest `name`, `numeric_profile`, `role`, `target` and semver string, in bytes.
pub const MAX_TOKEN_LEN: usize = 64;
/// Longest algorithm ID, in bytes.
pub const MAX_ID_LEN: usize = 128;
/// Longest key kept in an `UnknownField` or `DuplicateKey` error, in characters.
const MAX_KEPT_CHARS: usize = 64;

/// Whether the native artifact gates results or is only measured against the WASM reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NativeGate {
    /// `measured-only`
    MeasuredOnly,
    /// `gating`
    Gating,
}

impl NativeGate {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            NativeGate::MeasuredOnly => "measured-only",
            NativeGate::Gating => "gating",
        }
    }

    /// Parses the wire form exactly.
    pub fn parse(s: &str) -> Option<NativeGate> {
        match s {
            "measured-only" => Some(NativeGate::MeasuredOnly),
            "gating" => Some(NativeGate::Gating),
            _ => None,
        }
    }
}

/// The replay runtime the kernel was built with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayRuntime {
    /// Semver of the replay runtime.
    pub version: String,
    /// Digest of its configuration.
    pub config_digest: Digest,
}

/// One algorithm in the kernel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlgorithmEntry {
    /// Algorithm ID with exact semver.
    pub id: AlgorithmId,
    /// Digest of its spec card (`KRN.MAN.CARD`).
    pub card_digest: Digest,
    /// Digest of its implementation closure (`KRN.MAN.MANI.IMPL`).
    pub impl_digest: Digest,
}

/// Every manifest field except the artifact hashes. `schema_version` is not stored: it is
/// always [`SCHEMA_VERSION`], written by the encoder and checked by the reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestCore {
    pub name: String,
    pub semver: String,
    pub numeric_profile: String,
    pub source_tree_digest: Digest,
    pub lockfile_digest: Digest,
    pub toolchain_digest: Digest,
    pub replay_runtime: ReplayRuntime,
    /// Strictly increasing by [`AlgorithmId`].
    pub algorithms: Vec<AlgorithmEntry>,
    pub facade_selftest_digest: Digest,
    pub solver_limits_digest: Digest,
    pub reference_suite_digest: Digest,
    pub reference_results_digest: Digest,
    pub native_gate: NativeGate,
}

/// One built artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// What the artifact is (for example `wasm` or `native`).
    pub role: String,
    /// The target it was built for (for example `wasm32-unknown-unknown`).
    pub target: String,
    /// Digest of the artifact's bytes.
    pub sha256: Digest,
}

/// A validated kernel manifest. Only [`KernelManifest::new`] and [`KernelManifest::parse`]
/// build one, so its core digest always matches its core and its canonical bytes are fixed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelManifest {
    core: ManifestCore,
    core_digest: Digest,
    artifacts: Vec<Artifact>,
    canonical: Vec<u8>,
}

/// Manifest errors, each with a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// The input, or a list, is larger than its bound.
    TooLarge { what: &'static str, limit: usize },
    /// The input is not well-formed JSON (or not UTF-8); `offset` is a byte offset.
    Syntax { offset: usize },
    /// Strict mode only: the input is a valid manifest but not its canonical bytes; `offset`
    /// is the first byte that differs (the input length when one is a prefix of the other).
    NonCanonical { offset: usize },
    /// An object has a member the schema does not define (the key, truncated).
    UnknownField(String),
    /// An object has the same key twice (the key, truncated).
    DuplicateKey(String),
    /// A required member is absent.
    MissingField(&'static str),
    /// A member has the wrong JSON type or a value outside its allowed set.
    BadValue(&'static str),
    /// A digest is not `sha256:` followed by exactly 64 lowercase hex digits.
    BadHex(&'static str),
    /// A list is not strictly increasing; `index` is the first entry out of order.
    Unsorted { list: &'static str, index: usize },
    /// The claimed `core_digest` differs from the digest of the core (boxed to keep the
    /// error small).
    CoreDigestMismatch { claimed: Box<Digest>, computed: Box<Digest> },
}

impl ManifestError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            ManifestError::TooLarge { .. } => "too_large",
            ManifestError::Syntax { .. } => "syntax",
            ManifestError::NonCanonical { .. } => "non_canonical",
            ManifestError::UnknownField(_) => "unknown_field",
            ManifestError::DuplicateKey(_) => "duplicate_key",
            ManifestError::MissingField(_) => "missing_field",
            ManifestError::BadValue(_) => "bad_value",
            ManifestError::BadHex(_) => "bad_hex",
            ManifestError::Unsorted { .. } => "unsorted",
            ManifestError::CoreDigestMismatch { .. } => "core_digest_mismatch",
        }
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            ManifestError::TooLarge { what, limit } => write!(f, "{code}: {what} exceeds {limit}"),
            ManifestError::Syntax { offset } | ManifestError::NonCanonical { offset } => {
                write!(f, "{code}: at byte {offset}")
            }
            ManifestError::UnknownField(k) | ManifestError::DuplicateKey(k) => write!(f, "{code}: {k:?}"),
            ManifestError::MissingField(p) | ManifestError::BadValue(p) | ManifestError::BadHex(p) => {
                write!(f, "{code}: {p}")
            }
            ManifestError::Unsorted { list, index } => write!(f, "{code}: {list}[{index}]"),
            ManifestError::CoreDigestMismatch { claimed, computed } => {
                write!(f, "{code}: claimed {claimed}, computed {computed}")
            }
        }
    }
}

impl std::error::Error for ManifestError {}

// ------------------------------------------------------------------------------ value rules

/// `^[a-z][a-z0-9_-]{0,63}$`: `name`, `numeric_profile` and artifact `role`.
fn is_token(s: &str) -> bool {
    let b = s.as_bytes();
    match b.split_first() {
        Some((first, rest)) => {
            b.len() <= MAX_TOKEN_LEN
                && first.is_ascii_lowercase()
                && rest.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
        }
        None => false,
    }
}

/// `^[a-z0-9][a-z0-9_.-]{0,63}$`: artifact `target`.
fn is_target(s: &str) -> bool {
    let b = s.as_bytes();
    match b.split_first() {
        Some((first, rest)) => {
            b.len() <= MAX_TOKEN_LEN
                && (first.is_ascii_lowercase() || first.is_ascii_digit())
                && rest.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.' | b'-'))
        }
        None => false,
    }
}

/// A decimal with no leading zero (`0|[1-9][0-9]*`).
fn is_numeric_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) && (s.len() == 1 || !s.starts_with('0'))
}

/// SemVer 2.0.0 `MAJOR.MINOR.PATCH` with an optional pre-release and no build metadata, at most
/// [`MAX_TOKEN_LEN`] bytes. Checked lexically: numbers are not parsed, so no size limit beyond
/// the length applies.
fn is_semver(s: &str) -> bool {
    if s.len() > MAX_TOKEN_LEN {
        return false;
    }
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (s, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 || !parts.iter().all(|p| is_numeric_id(p)) {
        return false;
    }
    match pre {
        None => true,
        Some(p) => p.split('.').all(|id| {
            !id.is_empty()
                && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
                && (!id.bytes().all(|c| c.is_ascii_digit()) || is_numeric_id(id))
        }),
    }
}

/// Index of the first entry that is not strictly greater than its predecessor.
fn first_unsorted<T>(items: &[T], cmp: impl Fn(&T, &T) -> Ordering) -> Option<usize> {
    items.windows(2).position(|w| cmp(&w[0], &w[1]) != Ordering::Less).map(|i| i + 1)
}

fn kept(s: &str) -> String {
    s.chars().take(MAX_KEPT_CHARS).collect()
}

// ------------------------------------------------------------------------------ writing

fn s(x: &str) -> Json {
    Json::Str(x.to_string())
}

fn d(x: &Digest) -> Json {
    s(x.as_str())
}

fn obj(members: Vec<(&str, Json)>) -> Json {
    Json::Obj(members.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

impl ManifestCore {
    /// Checks every value rule that the types do not already guarantee: token, target and
    /// semver formats, list lengths and strict ordering.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if !is_token(&self.name) {
            return Err(ManifestError::BadValue("core.name"));
        }
        if !is_semver(&self.semver) {
            return Err(ManifestError::BadValue("core.semver"));
        }
        if !is_token(&self.numeric_profile) {
            return Err(ManifestError::BadValue("core.numeric_profile"));
        }
        if !is_semver(&self.replay_runtime.version) {
            return Err(ManifestError::BadValue("core.replay_runtime.version"));
        }
        if self.algorithms.len() > MAX_ALGORITHMS {
            return Err(ManifestError::TooLarge { what: "core.algorithms", limit: MAX_ALGORITHMS });
        }
        // The reserved module belongs to the kernel's own queries, never to a listed algorithm.
        if self.algorithms.iter().any(|a| a.id.to_string().len() > MAX_ID_LEN || a.id.module == RESERVED_MODULE) {
            return Err(ManifestError::BadValue("core.algorithms[].id"));
        }
        if let Some(index) = first_unsorted(&self.algorithms, |a, b| a.id.cmp(&b.id)) {
            return Err(ManifestError::Unsorted { list: "core.algorithms", index });
        }
        Ok(())
    }

    fn to_json(&self) -> Json {
        let algorithms = self
            .algorithms
            .iter()
            .map(|a| {
                obj(vec![
                    ("card_digest", d(&a.card_digest)),
                    ("id", s(&a.id.to_string())),
                    ("impl_digest", d(&a.impl_digest)),
                ])
            })
            .collect();
        obj(vec![
            ("algorithms", Json::Arr(algorithms)),
            ("facade_selftest_digest", d(&self.facade_selftest_digest)),
            ("lockfile_digest", d(&self.lockfile_digest)),
            ("name", s(&self.name)),
            ("native_gate", s(self.native_gate.as_str())),
            ("numeric_profile", s(&self.numeric_profile)),
            ("reference_results_digest", d(&self.reference_results_digest)),
            ("reference_suite_digest", d(&self.reference_suite_digest)),
            (
                "replay_runtime",
                obj(vec![
                    ("config_digest", d(&self.replay_runtime.config_digest)),
                    ("version", s(&self.replay_runtime.version)),
                ]),
            ),
            ("schema_version", s(SCHEMA_VERSION)),
            ("semver", s(&self.semver)),
            ("solver_limits_digest", d(&self.solver_limits_digest)),
            ("source_tree_digest", d(&self.source_tree_digest)),
            ("toolchain_digest", d(&self.toolchain_digest)),
        ])
    }

    /// RFC 8785 canonical JSON of the core (validated first).
    pub fn canonical_json(&self) -> Result<Vec<u8>, ManifestError> {
        self.validate()?;
        canonical(&self.to_json())
    }

    /// SHA-256 over [`ManifestCore::canonical_json`] (REQ-KRN.MAN.MANI.SCHEMA-002).
    pub fn core_digest(&self) -> Result<Digest, ManifestError> {
        Ok(sha256_digest(&self.canonical_json()?))
    }
}

/// The writer fails only on a duplicate key or nesting deeper than 128; the manifest's keys
/// are fixed and unique and its depth is 4, so an error here is reported, never ignored.
fn canonical(v: &Json) -> Result<Vec<u8>, ManifestError> {
    to_canonical(v).map_err(|_| ManifestError::BadValue("canonical_json"))
}

fn validate_artifacts(artifacts: &[Artifact]) -> Result<(), ManifestError> {
    if artifacts.len() > MAX_ARTIFACTS {
        return Err(ManifestError::TooLarge { what: "artifacts", limit: MAX_ARTIFACTS });
    }
    for a in artifacts {
        if !is_token(&a.role) {
            return Err(ManifestError::BadValue("artifacts[].role"));
        }
        if !is_target(&a.target) {
            return Err(ManifestError::BadValue("artifacts[].target"));
        }
    }
    // Roles and targets are ASCII, so byte order equals RFC 8785's UTF-16 order.
    if let Some(index) = first_unsorted(artifacts, |a, b| (&a.role, &a.target).cmp(&(&b.role, &b.target))) {
        return Err(ManifestError::Unsorted { list: "artifacts", index });
    }
    Ok(())
}

impl KernelManifest {
    /// Builds a manifest from a core and its artifacts, computing the core digest.
    pub fn new(core: ManifestCore, artifacts: Vec<Artifact>) -> Result<KernelManifest, ManifestError> {
        let core_digest = core.core_digest()?;
        Self::assemble(core, core_digest, artifacts)
    }

    fn assemble(core: ManifestCore, core_digest: Digest, artifacts: Vec<Artifact>) -> Result<Self, ManifestError> {
        validate_artifacts(&artifacts)?;
        let arts = artifacts
            .iter()
            .map(|a| obj(vec![("role", s(&a.role)), ("sha256", d(&a.sha256)), ("target", s(&a.target))]))
            .collect();
        let canonical = canonical(&obj(vec![
            ("artifacts", Json::Arr(arts)),
            ("core", core.to_json()),
            ("core_digest", d(&core_digest)),
        ]))?;
        Ok(KernelManifest { core, core_digest, artifacts, canonical })
    }

    /// Strict parse for verifiers: [`KernelManifest::parse`], then the input bytes must equal
    /// [`KernelManifest::canonical_json`] exactly, else `NonCanonical`.
    pub fn parse_canonical(input: &[u8]) -> Result<KernelManifest, ManifestError> {
        let m = Self::parse(input)?;
        if m.canonical.as_slice() != input {
            // Where the bytes first differ, else the shorter length (one is a prefix of the other).
            let shorter = if input.len() < m.canonical.len() { input.len() } else { m.canonical.len() };
            let offset = m.canonical.iter().zip(input).position(|(a, b)| a != b).unwrap_or(shorter);
            return Err(ManifestError::NonCanonical { offset });
        }
        Ok(m)
    }

    /// Parses a manifest (REQ-KRN.MAN.MANI.SCHEMA-003). Bounded and fail-closed. Formatting is
    /// free (whitespace, member order, escapes); the digests are over the canonical
    /// re-encoding, not over `input`. Use [`KernelManifest::parse_canonical`] to verify a file.
    pub fn parse(input: &[u8]) -> Result<KernelManifest, ManifestError> {
        if input.len() > MAX_INPUT_BYTES {
            return Err(ManifestError::TooLarge { what: "input", limit: MAX_INPUT_BYTES });
        }
        let text = std::str::from_utf8(input).map_err(|e| ManifestError::Syntax { offset: e.valid_up_to() })?;
        let mut r = Reader { s: text, pos: 0 };
        r.ws();
        let (core, claimed, artifacts) = r.manifest()?;
        r.ws();
        if r.pos != text.len() {
            return Err(ManifestError::Syntax { offset: r.pos });
        }
        core.validate()?;
        validate_artifacts(&artifacts)?;
        let computed = core.core_digest()?;
        if computed != claimed {
            return Err(ManifestError::CoreDigestMismatch { claimed: Box::new(claimed), computed: Box::new(computed) });
        }
        Self::assemble(core, computed, artifacts)
    }

    /// The core.
    pub fn core(&self) -> &ManifestCore {
        &self.core
    }

    /// SHA-256 over the core's canonical JSON.
    pub fn core_digest(&self) -> &Digest {
        &self.core_digest
    }

    /// The artifacts, sorted by `(role, target)`.
    pub fn artifacts(&self) -> &[Artifact] {
        &self.artifacts
    }

    /// RFC 8785 canonical JSON of the whole manifest.
    pub fn canonical_json(&self) -> &[u8] {
        &self.canonical
    }

    /// SHA-256 over [`KernelManifest::canonical_json`]: the `kernel_manifest_digest`
    /// (REQ-KRN.MAN.MANI.SCHEMA-002).
    pub fn manifest_digest(&self) -> Digest {
        sha256_digest(&self.canonical)
    }
}

// ------------------------------------------------------------------------------ reading

/// Member keys of each object, so a key's position doubles as its "seen" slot.
const TOP_KEYS: [&str; 3] = ["artifacts", "core", "core_digest"];
const CORE_KEYS: [&str; 14] = [
    "algorithms",
    "facade_selftest_digest",
    "lockfile_digest",
    "name",
    "native_gate",
    "numeric_profile",
    "reference_results_digest",
    "reference_suite_digest",
    "replay_runtime",
    "schema_version",
    "semver",
    "solver_limits_digest",
    "source_tree_digest",
    "toolchain_digest",
];
const RUNTIME_KEYS: [&str; 2] = ["config_digest", "version"];
const ALGORITHM_KEYS: [&str; 3] = ["card_digest", "id", "impl_digest"];
const ARTIFACT_KEYS: [&str; 3] = ["role", "sha256", "target"];

/// Tracks which members of one object have been read.
struct Members<'k> {
    keys: &'k [&'static str],
    paths: &'k [&'static str],
    seen: Vec<bool>,
}

impl<'k> Members<'k> {
    fn new(keys: &'k [&'static str], paths: &'k [&'static str]) -> Self {
        Members { keys, paths, seen: vec![false; keys.len()] }
    }

    /// The index of `key`, refusing unknown and repeated keys.
    fn claim(&mut self, key: &str) -> Result<usize, ManifestError> {
        let i = self.keys.iter().position(|k| *k == key).ok_or_else(|| ManifestError::UnknownField(kept(key)))?;
        match self.seen.get_mut(i) {
            Some(slot) if !*slot => {
                *slot = true;
                Ok(i)
            }
            _ => Err(ManifestError::DuplicateKey(kept(key))),
        }
    }

    /// The first absent member, in key order.
    fn check_complete(&self) -> Result<(), ManifestError> {
        match self.seen.iter().position(|s| !s) {
            Some(i) => Err(ManifestError::MissingField(self.paths.get(i).copied().unwrap_or("?"))),
            None => Ok(()),
        }
    }
}

fn need<T>(v: Option<T>, field_path: &'static str) -> Result<T, ManifestError> {
    v.ok_or(ManifestError::MissingField(field_path))
}

struct Reader<'a> {
    s: &'a str,
    pos: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.as_bytes().get(self.pos).copied()
    }

    fn syntax(&self) -> ManifestError {
        ManifestError::Syntax { offset: self.pos }
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Result<(), ManifestError> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.syntax())
        }
    }

    fn hex4(&mut self) -> Result<u32, ManifestError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let digit = self.peek().and_then(|c| char::from(c).to_digit(16)).ok_or_else(|| self.syntax())?;
            v = v * 16 + digit;
            self.pos += 1;
        }
        Ok(v)
    }

    /// A JSON string (the opening quote is next). Raw control characters, bad escapes and
    /// unpaired surrogates are syntax errors.
    fn string(&mut self) -> Result<String, ManifestError> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let start = self.pos;
            while matches!(self.peek(), Some(c) if c != b'"' && c != b'\\' && c >= 0x20) {
                self.pos += 1;
            }
            // `start` and `pos` sit next to ASCII bytes (or the ends), so both are char boundaries.
            out.push_str(self.s.get(start..self.pos).ok_or_else(|| self.syntax())?);
            match self.peek() {
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let c = self.peek().ok_or_else(|| self.syntax())?;
                    self.pos += 1;
                    let ch = match c {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode_escape()?,
                        _ => return Err(ManifestError::Syntax { offset: self.pos - 1 }),
                    };
                    out.push(ch);
                }
                _ => return Err(self.syntax()),
            }
        }
    }

    /// The code point of a `\u` escape whose `\u` was just read.
    fn unicode_escape(&mut self) -> Result<char, ManifestError> {
        let hi = self.hex4()?;
        let cp = if (0xD800..0xDC00).contains(&hi) {
            self.eat(b'\\')?;
            self.eat(b'u')?;
            let lo = self.hex4()?;
            if !(0xDC00..0xE000).contains(&lo) {
                return Err(self.syntax());
            }
            0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
        } else {
            hi
        };
        char::from_u32(cp).ok_or_else(|| self.syntax())
    }

    /// The error for an unexpected value: `BadValue(field_path)` when a JSON value of another type
    /// starts here, a syntax error when no JSON value does (end of input, a stray comma, a BOM).
    fn wrong_type(&self, field_path: &'static str) -> ManifestError {
        match self.peek() {
            Some(b'{' | b'[' | b'"' | b'-' | b'0'..=b'9' | b't' | b'f' | b'n') => ManifestError::BadValue(field_path),
            _ => self.syntax(),
        }
    }

    /// A member value that must be a string; any other JSON type is `BadValue(field_path)` (see [`Self::wrong_type`]).
    fn str_value(&mut self, field_path: &'static str) -> Result<String, ManifestError> {
        if self.peek() != Some(b'"') {
            return Err(self.wrong_type(field_path));
        }
        self.string()
    }

    fn digest_value(&mut self, field_path: &'static str) -> Result<Digest, ManifestError> {
        let text = self.str_value(field_path)?;
        Digest::parse(&text).map_err(|_| ManifestError::BadHex(field_path))
    }

    /// Opens a container: `open` must be next, else the value has the wrong type.
    fn open(&mut self, open: u8, field_path: &'static str) -> Result<(), ManifestError> {
        if self.peek() != Some(open) {
            return Err(self.wrong_type(field_path));
        }
        self.pos += 1;
        self.ws();
        Ok(())
    }

    /// Iterates an object's members, calling `member` with each key's slot; `member` reads the
    /// value. A trailing comma, a missing colon or anything else out of place is a syntax error.
    fn object(
        &mut self,
        field_path: &'static str,
        members: &mut Members<'_>,
        mut member: impl FnMut(&mut Self, usize) -> Result<(), ManifestError>,
    ) -> Result<(), ManifestError> {
        self.open(b'{', field_path)?;
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return members.check_complete();
        }
        loop {
            self.ws();
            let key = self.string()?;
            self.ws();
            self.eat(b':')?;
            self.ws();
            let i = members.claim(&key)?;
            member(self, i)?;
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return members.check_complete();
                }
                _ => return Err(self.syntax()),
            }
        }
    }

    /// Iterates an array of at most `limit` items, calling `item` for each.
    fn array(
        &mut self,
        field_path: &'static str,
        limit: usize,
        mut item: impl FnMut(&mut Self) -> Result<(), ManifestError>,
    ) -> Result<(), ManifestError> {
        self.open(b'[', field_path)?;
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(());
        }
        let mut n = 0usize;
        loop {
            n += 1;
            if n > limit {
                return Err(ManifestError::TooLarge { what: field_path, limit });
            }
            self.ws();
            item(self)?;
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return Err(self.syntax()),
            }
        }
    }

    fn manifest(&mut self) -> Result<(ManifestCore, Digest, Vec<Artifact>), ManifestError> {
        const PATHS: [&str; 3] = ["artifacts", "core", "core_digest"];
        let (mut core, mut claimed, mut artifacts) = (None, None, None);
        let mut m = Members::new(&TOP_KEYS, &PATHS);
        self.object("manifest", &mut m, |r, i| {
            match i {
                0 => artifacts = Some(r.artifacts()?),
                1 => core = Some(r.core()?),
                _ => claimed = Some(r.digest_value("core_digest")?),
            }
            Ok(())
        })?;
        Ok((need(core, "core")?, need(claimed, "core_digest")?, need(artifacts, "artifacts")?))
    }

    fn artifacts(&mut self) -> Result<Vec<Artifact>, ManifestError> {
        const PATHS: [&str; 3] = ["artifacts[].role", "artifacts[].sha256", "artifacts[].target"];
        let mut out = Vec::new();
        self.array("artifacts", MAX_ARTIFACTS, |r| {
            let (mut role, mut sha, mut target) = (None, None, None);
            let mut m = Members::new(&ARTIFACT_KEYS, &PATHS);
            r.object("artifacts[]", &mut m, |r, i| {
                match i {
                    0 => role = Some(r.str_value(PATHS[0])?),
                    1 => sha = Some(r.digest_value(PATHS[1])?),
                    _ => target = Some(r.str_value(PATHS[2])?),
                }
                Ok(())
            })?;
            out.push(Artifact {
                role: need(role, PATHS[0])?,
                target: need(target, PATHS[2])?,
                sha256: need(sha, PATHS[1])?,
            });
            Ok(())
        })?;
        Ok(out)
    }

    fn algorithms(&mut self) -> Result<Vec<AlgorithmEntry>, ManifestError> {
        const PATHS: [&str; 3] =
            ["core.algorithms[].card_digest", "core.algorithms[].id", "core.algorithms[].impl_digest"];
        let mut out = Vec::new();
        self.array("core.algorithms", MAX_ALGORITHMS, |r| {
            let (mut card, mut id, mut imp) = (None, None, None);
            let mut m = Members::new(&ALGORITHM_KEYS, &PATHS);
            r.object("core.algorithms[]", &mut m, |r, i| {
                match i {
                    0 => card = Some(r.digest_value(PATHS[0])?),
                    1 => {
                        let text = r.str_value(PATHS[1])?;
                        if text.len() > MAX_ID_LEN {
                            return Err(ManifestError::BadValue(PATHS[1]));
                        }
                        id = Some(AlgorithmId::parse(&text).map_err(|_| ManifestError::BadValue(PATHS[1]))?);
                    }
                    _ => imp = Some(r.digest_value(PATHS[2])?),
                }
                Ok(())
            })?;
            out.push(AlgorithmEntry {
                id: need(id, PATHS[1])?,
                card_digest: need(card, PATHS[0])?,
                impl_digest: need(imp, PATHS[2])?,
            });
            Ok(())
        })?;
        Ok(out)
    }

    fn replay_runtime(&mut self) -> Result<ReplayRuntime, ManifestError> {
        const PATHS: [&str; 2] = ["core.replay_runtime.config_digest", "core.replay_runtime.version"];
        let (mut cfg, mut version) = (None, None);
        let mut m = Members::new(&RUNTIME_KEYS, &PATHS);
        self.object("core.replay_runtime", &mut m, |r, i| {
            match i {
                0 => cfg = Some(r.digest_value(PATHS[0])?),
                _ => version = Some(r.str_value(PATHS[1])?),
            }
            Ok(())
        })?;
        Ok(ReplayRuntime { version: need(version, PATHS[1])?, config_digest: need(cfg, PATHS[0])? })
    }

    fn core(&mut self) -> Result<ManifestCore, ManifestError> {
        const PATHS: [&str; 14] = [
            "core.algorithms",
            "core.facade_selftest_digest",
            "core.lockfile_digest",
            "core.name",
            "core.native_gate",
            "core.numeric_profile",
            "core.reference_results_digest",
            "core.reference_suite_digest",
            "core.replay_runtime",
            "core.schema_version",
            "core.semver",
            "core.solver_limits_digest",
            "core.source_tree_digest",
            "core.toolchain_digest",
        ];
        let mut algorithms = None;
        let mut strings: [Option<String>; 3] = [None, None, None]; // name, numeric_profile, semver
        let mut digests: [Option<Digest>; 7] = [None; 7];
        let mut runtime = None;
        let mut gate = None;
        let mut m = Members::new(&CORE_KEYS, &PATHS);
        self.object("core", &mut m, |r, i| {
            let field_path = PATHS.get(i).copied().unwrap_or("core");
            // Slot of each digest member in `digests`, in key order.
            let digest_slot = match i {
                1 => Some(0),
                2 => Some(1),
                6 => Some(2),
                7 => Some(3),
                11 => Some(4),
                12 => Some(5),
                13 => Some(6),
                _ => None,
            };
            if let Some(slot) = digest_slot.and_then(|j| digests.get_mut(j)) {
                *slot = Some(r.digest_value(field_path)?);
                return Ok(());
            }
            match i {
                0 => algorithms = Some(r.algorithms()?),
                3 | 5 | 10 => {
                    let j = match i {
                        3 => 0,
                        5 => 1,
                        _ => 2,
                    };
                    if let Some(slot) = strings.get_mut(j) {
                        *slot = Some(r.str_value(field_path)?);
                    }
                }
                4 => {
                    let text = r.str_value(field_path)?;
                    gate = Some(NativeGate::parse(&text).ok_or(ManifestError::BadValue(field_path))?);
                }
                8 => runtime = Some(r.replay_runtime()?),
                9 => {
                    if r.str_value(field_path)? != SCHEMA_VERSION {
                        return Err(ManifestError::BadValue(field_path));
                    }
                }
                _ => return Err(ManifestError::BadValue(field_path)),
            }
            Ok(())
        })?;
        let [name, numeric_profile, semver] = strings;
        let [facade, lockfile, results, suite, limits, source, toolchain] = digests;
        Ok(ManifestCore {
            name: need(name, PATHS[3])?,
            semver: need(semver, PATHS[10])?,
            numeric_profile: need(numeric_profile, PATHS[5])?,
            source_tree_digest: need(source, PATHS[12])?,
            lockfile_digest: need(lockfile, PATHS[2])?,
            toolchain_digest: need(toolchain, PATHS[13])?,
            replay_runtime: need(runtime, PATHS[8])?,
            algorithms: need(algorithms, PATHS[0])?,
            facade_selftest_digest: need(facade, PATHS[1])?,
            solver_limits_digest: need(limits, PATHS[11])?,
            reference_suite_digest: need(suite, PATHS[7])?,
            reference_results_digest: need(results, PATHS[6])?,
            native_gate: need(gate, PATHS[4])?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kci::json::cmp_utf16;

    fn strictly_sorted(keys: &[&str]) -> bool {
        keys.windows(2).all(|p| cmp_utf16(p[0], p[1]) == Ordering::Less)
    }

    /// Reader key tables are in canonical order, so `Members` slots line up with the writer.
    #[test]
    fn key_tables_are_canonical() {
        for keys in [&TOP_KEYS[..], &CORE_KEYS[..], &RUNTIME_KEYS[..], &ALGORITHM_KEYS[..], &ARTIFACT_KEYS[..]] {
            assert!(strictly_sorted(keys), "{keys:?}");
        }
    }

    #[test]
    fn value_rules() {
        assert!(is_semver("0.1.0") && is_semver("1.0.0-rc.1") && is_semver("1.0.0-0a.x-y"));
        for bad in ["1.0", "01.0.0", "1.0.0-", "1.0.0-01", "1.0.0+b", "1.0.0-a..b", "v1.0.0", ""] {
            assert!(!is_semver(bad), "{bad}");
        }
        assert!(is_token("fid-kernel") && is_token("knp-1") && !is_token("1x") && !is_token("A"));
        assert!(is_token(&"a".repeat(64)) && !is_token(&"a".repeat(65)));
        assert!(is_target("x86_64-unknown-linux-gnu") && is_target("wasm32-wasip1") && !is_target("-x"));
    }
}

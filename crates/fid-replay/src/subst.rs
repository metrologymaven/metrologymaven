//! Observation substitution and missing-item detection. Spec node `CORE.RPL.EXEC.SUBST`.
//!
//! A replay must see exactly what the recording saw. [`ObservationSource`] serves each
//! step's class-N observations (clock, salt, device, human text, LLM output, external file)
//! from the recorded span, in recorded order, and never calls a clock, random generator,
//! device, LLM or file system (REQ-001). Each served value is checked against its recorded
//! SHA-256 (REQ-003). Commitment salts come from the entry payload through
//! [`fid_journal::SaltSource`], so re-computed header commitments equal the recorded ones
//! (REQ-004).
//!
//! Fail closed: an extra request, a request of another kind, a value left unconsumed at
//! [`StepObservations::finish`] or a salt asked twice is `observation_mismatch` (the step is
//! `DIVERGENT`, cause `input`, REQ-002). An absent or corrupt value is `missing_item` (the step
//! is `NOT_REPLAYABLE`, naming the digest). After the first fault every request of the step
//! fails with that fault, so a handler that swallows an error cannot continue on other inputs.
//!
//! [`preflight`] walks a span once before the first step and lists every missing blob,
//! unreachable command major, detached payload and unavailable kernel with the steps that
//! need it (REQ-005, REQ-006).
//!
//! Everything here is D0: no clocks, no randomness, no hash-ordered collections.

use crate::bisect::Cause;
use crate::classify::ReplayOutcome;
use fid_canon::{digest_bytes, CValue};
use fid_journal::{EntryError, Field, Salt, SaltSource};
use fid_registry::{check_migrators, retained, LifecycleDecl, MigratorRecord};
use fid_types::{CommandId, Digest};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Most steps in one span.
pub const MAX_STEPS: usize = 1 << 20;
/// Most recorded observations in one step.
pub const MAX_OBSERVATIONS_PER_STEP: usize = 4096;
/// Largest inline observation value, in bytes. Larger values belong in the blob store.
pub const MAX_INLINE_BYTES: usize = 1 << 20;
/// Longest name of an [`ObservationKind::Other`] kind.
pub const MAX_KIND_LEN: usize = 64;

// ------------------------------------------------------------------ errors

/// Kind of a missing item (REQ-006), in report order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MissingKind {
    Blob,
    Command,
    Payload,
    Kernel,
}

impl MissingKind {
    /// Wire name, e.g. `blob`.
    pub fn as_str(self) -> &'static str {
        match self {
            MissingKind::Blob => "blob",
            MissingKind::Command => "command",
            MissingKind::Payload => "payload",
            MissingKind::Kernel => "kernel",
        }
    }
}

impl fmt::Display for MissingKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Something a replay needs and cannot have (REQ-006).
///
/// `id` is the blob or kernel digest (`sha256:...`), the command ID with its major
/// (`name@major`), or for a payload the hash of the entry whose payload is detached or damaged.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MissingItem {
    pub kind: MissingKind,
    pub id: String,
    /// Sequence numbers of the steps that need the item, ascending, without duplicates.
    pub steps: Vec<u64>,
}

impl MissingItem {
    /// The item as a JSON value: `{"kind", "id", "steps"}` (steps as decimal strings).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("kind".to_string(), CValue::Str(self.kind.as_str().to_string()));
        o.insert("id".to_string(), CValue::Str(self.id.clone()));
        o.insert("steps".to_string(), CValue::Arr(self.steps.iter().map(|s| CValue::U64(*s)).collect()));
        CValue::Obj(o)
    }
}

impl fmt::Display for MissingItem {
    /// `kind id`, the form `StepComparison::missing` takes in `CORE.RPL.CLASS`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.kind, self.id)
    }
}

/// Typed errors with stable codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubstError {
    /// The replayed handler asked for more observations, or another kind, than the recording
    /// holds for the step, or left recorded ones unconsumed (REQ-002).
    ObservationMismatch { seq: u64, detail: String },
    /// A recorded value is absent or fails its digest check (REQ-003).
    MissingItem(MissingItem),
    /// The recorded span is malformed or over a bound.
    SpanInvalid(String),
    /// Lifecycle or migrator records fail the `CORE.REG.VER.LIFE` checks.
    CatalogInvalid(String),
    /// The span holds no step with this sequence number.
    UnknownStep(u64),
}

impl SubstError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            SubstError::ObservationMismatch { .. } => "observation_mismatch",
            SubstError::MissingItem(_) => "missing_item",
            SubstError::SpanInvalid(_) => "span_invalid",
            SubstError::CatalogInvalid(_) => "catalog_invalid",
            SubstError::UnknownStep(_) => "unknown_step",
        }
    }

    /// The step outcome this error forces: `DIVERGENT` for a mismatch, `NOT_REPLAYABLE` for a
    /// missing item, `None` for errors that are not about one step.
    pub fn outcome(&self) -> Option<ReplayOutcome> {
        match self {
            SubstError::ObservationMismatch { .. } => Some(ReplayOutcome::Divergent),
            SubstError::MissingItem(_) => Some(ReplayOutcome::NotReplayable),
            _ => None,
        }
    }

    /// The divergence cause: `input` for a mismatch (REQ-002), otherwise `None`.
    pub fn cause(&self) -> Option<Cause> {
        match self {
            SubstError::ObservationMismatch { .. } => Some(Cause::Input),
            _ => None,
        }
    }
}

impl fmt::Display for SubstError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubstError::ObservationMismatch { seq, detail } => write!(f, "observation_mismatch: step {seq}: {detail}"),
            SubstError::MissingItem(m) => write!(f, "missing_item: {m}"),
            SubstError::SpanInvalid(why) => write!(f, "span_invalid: {why}"),
            SubstError::CatalogInvalid(why) => write!(f, "catalog_invalid: {why}"),
            SubstError::UnknownStep(seq) => write!(f, "unknown_step: {seq}"),
        }
    }
}

impl std::error::Error for SubstError {}

fn invalid(why: impl Into<String>) -> SubstError {
    SubstError::SpanInvalid(why.into())
}

// ------------------------------------------------------------------ recorded span

/// Kind of a class-N observation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObservationKind {
    Clock,
    Salt,
    Device,
    HumanText,
    LlmOutput,
    ExternalFile,
    /// Any other class-N source; the name matches `^[a-z][a-z0-9_]{0,63}$` and is not a
    /// built-in kind's name.
    Other(String),
}

const BUILTIN: [(&str, ObservationKind); 6] = [
    ("clock", ObservationKind::Clock),
    ("salt", ObservationKind::Salt),
    ("device", ObservationKind::Device),
    ("human_text", ObservationKind::HumanText),
    ("llm_output", ObservationKind::LlmOutput),
    ("external_file", ObservationKind::ExternalFile),
];

impl ObservationKind {
    /// Wire name, e.g. `human_text`; an `Other` kind is its name.
    pub fn as_str(&self) -> &str {
        match self {
            ObservationKind::Clock => "clock",
            ObservationKind::Salt => "salt",
            ObservationKind::Device => "device",
            ObservationKind::HumanText => "human_text",
            ObservationKind::LlmOutput => "llm_output",
            ObservationKind::ExternalFile => "external_file",
            ObservationKind::Other(name) => name,
        }
    }

    /// Parses a wire name: a built-in name gives its variant, any other valid name `Other`.
    pub fn parse(s: &str) -> Result<ObservationKind, SubstError> {
        match BUILTIN.iter().find(|(n, _)| *n == s) {
            Some((_, k)) => Ok(k.clone()),
            None => ObservationKind::other(s),
        }
    }

    /// An `Other` kind; a built-in name or a name outside the pattern is refused, so every
    /// kind has exactly one representation.
    pub fn other(name: &str) -> Result<ObservationKind, SubstError> {
        if BUILTIN.iter().any(|(n, _)| *n == name) {
            return Err(invalid(format!("observation kind {name:?} is built in, not other")));
        }
        if !valid_other_name(name) {
            return Err(invalid(format!("observation kind {name:?} must match ^[a-z][a-z0-9_]{{0,63}}$")));
        }
        Ok(ObservationKind::Other(name.to_string()))
    }

    /// Whether this value is in its one valid representation.
    fn is_valid(&self) -> bool {
        match self {
            ObservationKind::Other(name) => valid_other_name(name) && !BUILTIN.iter().any(|(n, _)| n == name),
            _ => true,
        }
    }
}

fn valid_other_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_KIND_LEN
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

impl fmt::Display for ObservationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a recorded observation's bytes live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Storage {
    /// In the entry payload ([`StepPayload::inline`]).
    Inline,
    /// In the content-addressed blob store, by digest.
    Blob,
}

/// One recorded observation: its kind and the SHA-256 of its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedObservation {
    pub kind: ObservationKind,
    pub digest: Digest,
    pub storage: Storage,
}

/// The detachable part of a step: commitment salts and inline observation values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepPayload {
    pub salts: BTreeMap<Field, Salt>,
    pub inline: BTreeMap<Digest, Vec<u8>>,
}

/// One recorded journal entry, as replay sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedStep {
    pub seq: u64,
    /// The entry hash.
    pub entry: Digest,
    /// Command `name@major`, or `None` for entries that run no command.
    pub command: Option<CommandId>,
    /// Kernel manifest digest the step ran under, if it needs a kernel.
    pub kernel: Option<Digest>,
    /// Class-N observations in the order the handler read them.
    pub observations: Vec<RecordedObservation>,
    /// `None` when the payload is detached.
    pub payload: Option<StepPayload>,
}

/// A validated recorded span: steps in strictly increasing sequence order, within bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedSpan {
    steps: Vec<RecordedStep>,
}

impl RecordedSpan {
    /// Checks the bounds, that sequence numbers are at least 1 and strictly increase, that
    /// every kind is valid and that inline values stay under [`MAX_INLINE_BYTES`].
    pub fn new(steps: Vec<RecordedStep>) -> Result<RecordedSpan, SubstError> {
        if steps.len() > MAX_STEPS {
            return Err(invalid(format!("more than {MAX_STEPS} steps")));
        }
        let mut prev = 0u64;
        for s in &steps {
            if s.seq <= prev {
                return Err(invalid(format!("seq {} is not greater than {prev} (seq starts at 1)", s.seq)));
            }
            prev = s.seq;
            if s.observations.len() > MAX_OBSERVATIONS_PER_STEP {
                return Err(invalid(format!("step {}: more than {MAX_OBSERVATIONS_PER_STEP} observations", s.seq)));
            }
            if let Some(o) = s.observations.iter().find(|o| !o.kind.is_valid()) {
                return Err(invalid(format!("step {}: invalid observation kind {:?}", s.seq, o.kind)));
            }
            if let Some(p) = &s.payload {
                if p.inline.values().any(|v| v.len() > MAX_INLINE_BYTES) {
                    return Err(invalid(format!("step {}: inline value over {MAX_INLINE_BYTES} bytes", s.seq)));
                }
            }
        }
        Ok(RecordedSpan { steps })
    }

    /// Reads a span from JSON:
    /// `{"steps": [{"seq", "entry", "command", "kernel", "observations", "payload"}]}`, where
    /// `seq` is an integer, `command` is `name@major` or null, `kernel` a digest or null, each
    /// observation is `{"kind", "digest", "storage": "inline" | "blob"}` and `payload` is null
    /// (detached) or `{"salts": {field: 32 hex}, "inline": {digest: lowercase hex bytes}}`.
    /// Unknown fields are refused.
    pub fn from_value(v: &CValue) -> Result<RecordedSpan, SubstError> {
        let root = object(v, "span", &["steps"])?;
        let CValue::Arr(raw) = need(root, "steps", "span")? else {
            return Err(invalid("span.steps must be an array"));
        };
        if raw.len() > MAX_STEPS {
            return Err(invalid(format!("more than {MAX_STEPS} steps")));
        }
        let steps = raw.iter().map(step_from_value).collect::<Result<Vec<_>, _>>()?;
        RecordedSpan::new(steps)
    }

    /// The steps, in sequence order.
    pub fn steps(&self) -> &[RecordedStep] {
        &self.steps
    }

    /// The step with sequence number `seq`.
    pub fn step(&self, seq: u64) -> Option<&RecordedStep> {
        self.steps.binary_search_by_key(&seq, |s| s.seq).ok().and_then(|i| self.steps.get(i))
    }
}

fn object<'a>(v: &'a CValue, what: &str, keys: &[&str]) -> Result<&'a BTreeMap<String, CValue>, SubstError> {
    let CValue::Obj(o) = v else { return Err(invalid(format!("{what} must be an object"))) };
    if let Some(k) = o.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(invalid(format!("{what}: unknown field {k:?}")));
    }
    Ok(o)
}

fn need<'a>(o: &'a BTreeMap<String, CValue>, k: &str, what: &str) -> Result<&'a CValue, SubstError> {
    o.get(k).ok_or_else(|| invalid(format!("{what}.{k} is required")))
}

fn text<'a>(v: &'a CValue, what: &str) -> Result<&'a str, SubstError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(invalid(format!("{what} must be a string"))),
    }
}

fn digest(v: &CValue, what: &str) -> Result<Digest, SubstError> {
    Digest::parse(text(v, what)?).map_err(|_| invalid(format!("{what} must be sha256: and 64 lowercase hex digits")))
}

/// Integers above 2^53 cannot be told apart from their neighbours in a JSON number.
const MAX_JSON_INT: f64 = 9_007_199_254_740_991.0;

fn step_from_value(v: &CValue) -> Result<RecordedStep, SubstError> {
    let o = object(v, "step", &["seq", "entry", "command", "kernel", "observations", "payload"])?;
    let seq = match need(o, "seq", "step")? {
        CValue::Num(n) if n.fract() == 0.0 && *n >= 1.0 && *n <= MAX_JSON_INT => *n as u64,
        _ => return Err(invalid("step.seq must be an integer from 1 to 2^53 - 1")),
    };
    let at = format!("step {seq}");
    let entry = digest(need(o, "entry", &at)?, &format!("{at}.entry"))?;
    let command = match need(o, "command", &at)? {
        CValue::Null => None,
        c => {
            let s = text(c, &format!("{at}.command"))?;
            Some(CommandId::parse(s).map_err(|_| invalid(format!("{at}.command is not name@major: {s:?}")))?)
        }
    };
    let kernel = match need(o, "kernel", &at)? {
        CValue::Null => None,
        k => Some(digest(k, &format!("{at}.kernel"))?),
    };
    let CValue::Arr(raw) = need(o, "observations", &at)? else {
        return Err(invalid(format!("{at}.observations must be an array")));
    };
    if raw.len() > MAX_OBSERVATIONS_PER_STEP {
        return Err(invalid(format!("{at}: more than {MAX_OBSERVATIONS_PER_STEP} observations")));
    }
    let mut observations = Vec::with_capacity(raw.len());
    for (i, ov) in raw.iter().enumerate() {
        let w = format!("{at}.observations[{i}]");
        let ob = object(ov, &w, &["kind", "digest", "storage"])?;
        let kind = ObservationKind::parse(text(need(ob, "kind", &w)?, &format!("{w}.kind"))?)?;
        let digest = digest(need(ob, "digest", &w)?, &format!("{w}.digest"))?;
        let storage = match text(need(ob, "storage", &w)?, &format!("{w}.storage"))? {
            "inline" => Storage::Inline,
            "blob" => Storage::Blob,
            other => return Err(invalid(format!("{w}.storage must be inline or blob, got {other:?}"))),
        };
        observations.push(RecordedObservation { kind, digest, storage });
    }
    let payload = match need(o, "payload", &at)? {
        CValue::Null => None,
        p => Some(payload_from_value(p, &format!("{at}.payload"))?),
    };
    Ok(RecordedStep { seq, entry, command, kernel, observations, payload })
}

fn payload_from_value(v: &CValue, at: &str) -> Result<StepPayload, SubstError> {
    let o = object(v, at, &["salts", "inline"])?;
    let mut payload = StepPayload::default();
    let CValue::Obj(salts) = need(o, "salts", at)? else {
        return Err(invalid(format!("{at}.salts must be an object")));
    };
    for (name, hex) in salts {
        let field = Field::parse(name).map_err(|_| invalid(format!("{at}.salts: unknown field {name:?}")))?;
        let salt = Salt::from_hex(text(hex, &format!("{at}.salts.{name}"))?)
            .map_err(|_| invalid(format!("{at}.salts.{name} must be 32 lowercase hex digits")))?;
        payload.salts.insert(field, salt);
    }
    let CValue::Obj(inline) = need(o, "inline", at)? else {
        return Err(invalid(format!("{at}.inline must be an object")));
    };
    for (d, hex) in inline {
        let key = Digest::parse(d).map_err(|_| invalid(format!("{at}.inline: key {d:?} is not a digest")))?;
        let bytes = from_hex(text(hex, &format!("{at}.inline.{d}"))?).ok_or_else(|| {
            invalid(format!("{at}.inline.{d} must be lowercase hex of at most {MAX_INLINE_BYTES} bytes"))
        })?;
        payload.inline.insert(key, bytes);
    }
    Ok(payload)
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if b.len() % 2 != 0 || b.len() / 2 > MAX_INLINE_BYTES {
        return None;
    }
    let nib = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    b.chunks(2).map(|p| Some((nib(*p.first()?)? << 4) | nib(*p.get(1)?)?)).collect()
}

// ------------------------------------------------------------------ sources

/// Read access to the content-addressed blob store. Values are always re-checked against
/// their digest by the caller, so a store that returns wrong bytes cannot cause a silent replay.
pub trait BlobSource {
    /// The bytes stored under `digest`, or `None` when absent.
    fn get(&self, digest: &Digest) -> Option<Vec<u8>>;
    /// Whether a blob is stored under `digest` (used by [`preflight`]).
    fn has(&self, digest: &Digest) -> bool {
        self.get(digest).is_some()
    }
}

impl BlobSource for BTreeMap<Digest, Vec<u8>> {
    fn get(&self, digest: &Digest) -> Option<Vec<u8>> {
        BTreeMap::get(self, digest).cloned()
    }
    fn has(&self, digest: &Digest) -> bool {
        self.contains_key(digest)
    }
}

/// Kernel artifacts available to the replay; `CORE.RPL.EXEC.VERIFY` supplies the real one.
pub trait KernelSource {
    /// Whether the kernel with manifest digest `digest` can be supplied.
    fn has(&self, digest: &Digest) -> bool;
}

impl KernelSource for BTreeSet<Digest> {
    fn has(&self, digest: &Digest) -> bool {
        self.contains(digest)
    }
}

/// A served observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub kind: ObservationKind,
    /// The recorded digest; `bytes` has been checked to hash to it.
    pub digest: Digest,
    pub bytes: Vec<u8>,
}

/// Serves each step's recorded class-N observations (REQ-001).
#[derive(Clone, Copy)]
pub struct ObservationSource<'a> {
    span: &'a RecordedSpan,
    blobs: &'a dyn BlobSource,
}

impl<'a> ObservationSource<'a> {
    pub fn new(span: &'a RecordedSpan, blobs: &'a dyn BlobSource) -> ObservationSource<'a> {
        ObservationSource { span, blobs }
    }

    /// The observation queue of step `seq`, positioned at its first observation.
    pub fn step(&self, seq: u64) -> Result<StepObservations<'a>, SubstError> {
        let step = self.span.step(seq).ok_or(SubstError::UnknownStep(seq))?;
        Ok(StepObservations { step, blobs: self.blobs, next: 0, salts_served: BTreeSet::new(), fault: None })
    }
}

/// The recorded observations of one step, consumed in order.
pub struct StepObservations<'a> {
    step: &'a RecordedStep,
    blobs: &'a dyn BlobSource,
    next: usize,
    salts_served: BTreeSet<Field>,
    fault: Option<SubstError>,
}

impl StepObservations<'_> {
    /// The step's sequence number.
    pub fn seq(&self) -> u64 {
        self.step.seq
    }

    fn fail(&mut self, e: SubstError) -> SubstError {
        if self.fault.is_none() {
            self.fault = Some(e.clone());
        }
        e
    }

    fn mismatch(&mut self, detail: String) -> SubstError {
        let e = SubstError::ObservationMismatch { seq: self.step.seq, detail };
        self.fail(e)
    }

    fn missing(&mut self, kind: MissingKind, id: String) -> SubstError {
        let e = SubstError::MissingItem(MissingItem { kind, id, steps: vec![self.step.seq] });
        self.fail(e)
    }

    /// Serves the next recorded observation, which must be of kind `kind` (REQ-001..003).
    pub fn observe(&mut self, kind: &ObservationKind) -> Result<Observation, SubstError> {
        if let Some(f) = &self.fault {
            return Err(f.clone());
        }
        let step = self.step;
        let n = step.observations.len();
        let Some(rec) = step.observations.get(self.next) else {
            return Err(
                self.mismatch(format!("observation {} of kind {kind} requested, the recording holds {n}", n + 1))
            );
        };
        if !kind.is_valid() || rec.kind != *kind {
            let at = self.next + 1;
            return Err(self.mismatch(format!("observation {at} requested as {kind}, recorded as {}", rec.kind)));
        }
        self.next += 1;
        let digest = rec.digest;
        let bytes = match rec.storage {
            Storage::Blob => match self.blobs.get(&digest) {
                Some(b) if Digest::from_bytes(digest_bytes(&b).0) == digest => b,
                _ => return Err(self.missing(MissingKind::Blob, digest.to_string())),
            },
            Storage::Inline => match step.payload.as_ref().and_then(|p| p.inline.get(&digest)) {
                Some(b) if Digest::from_bytes(digest_bytes(b).0) == digest => b.clone(),
                _ => return Err(self.missing(MissingKind::Payload, step.entry.to_string())),
            },
        };
        Ok(Observation { kind: rec.kind.clone(), digest, bytes })
    }

    /// Serves the recorded commitment salt for `field` from the entry payload (REQ-004).
    /// Each field is served at most once per step.
    pub fn commitment_salt(&mut self, field: Field) -> Result<Salt, SubstError> {
        if let Some(f) = &self.fault {
            return Err(f.clone());
        }
        let step = self.step;
        let Some(payload) = step.payload.as_ref() else {
            return Err(self.missing(MissingKind::Payload, step.entry.to_string()));
        };
        let Some(salt) = payload.salts.get(&field).copied() else {
            return Err(self.mismatch(format!("commitment salt for {field} requested, none recorded")));
        };
        if !self.salts_served.insert(field) {
            return Err(self.mismatch(format!("commitment salt for {field} requested twice")));
        }
        Ok(salt)
    }

    /// Ends the step. Fails with the step's first fault, or with `observation_mismatch` when a
    /// recorded observation or commitment salt was not consumed.
    pub fn finish(mut self) -> Result<(), SubstError> {
        if let Some(f) = self.fault.take() {
            return Err(f);
        }
        let step = self.step;
        let n = step.observations.len();
        if self.next < n {
            return Err(self.mismatch(format!("{} of {n} recorded observations were not consumed", n - self.next)));
        }
        if let Some(p) = &step.payload {
            let left: Vec<&str> =
                p.salts.keys().filter(|f| !self.salts_served.contains(f)).map(|f| f.as_str()).collect();
            if !left.is_empty() {
                return Err(self.mismatch(format!("recorded commitment salts not consumed: {}", left.join(", "))));
            }
        }
        Ok(())
    }
}

/// Lets `fid_journal::build` draw its salts from the recording. The typed error stays in the
/// step and is returned by [`StepObservations::finish`].
impl SaltSource for StepObservations<'_> {
    fn salt(&mut self, field: Field) -> Result<Salt, EntryError> {
        self.commitment_salt(field).map_err(|_| EntryError::MissingSalt(field))
    }
}

// ------------------------------------------------------------------ pre-flight

/// Which command majors a replay can dispatch, from the registry's lifecycle and migrator
/// records (`CORE.REG.VER.LIFE`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCatalog {
    retained: BTreeSet<(String, u32)>,
    /// (name, from_major) -> to_majors.
    migrators: BTreeMap<(String, u32), BTreeSet<u32>>,
}

impl CommandCatalog {
    /// Builds the catalog. The records must pass `fid_registry::retained` and
    /// `fid_registry::check_migrators`; otherwise `catalog_invalid` (fail closed).
    pub fn new(decls: &[LifecycleDecl], migrators: &[MigratorRecord]) -> Result<CommandCatalog, SubstError> {
        let kept = retained(decls).map_err(|e| SubstError::CatalogInvalid(e.to_string()))?;
        check_migrators(decls, migrators).map_err(|e| SubstError::CatalogInvalid(e.to_string()))?;
        let retained = kept.iter().map(|d| (d.command.name().to_string(), d.command.major())).collect();
        let mut edges: BTreeMap<(String, u32), BTreeSet<u32>> = BTreeMap::new();
        for m in migrators {
            edges.entry((m.command.as_str().to_string(), m.from_major)).or_default().insert(m.to_major);
        }
        Ok(CommandCatalog { retained, migrators: edges })
    }

    /// Whether `command` is retained (any state but `removed`, `hidden` included) or a chain
    /// of migrators leads from it to a retained major.
    pub fn reachable(&self, command: &CommandId) -> bool {
        let name = command.name().to_string();
        let mut seen = BTreeSet::new();
        let mut todo = vec![command.major()];
        while let Some(major) = todo.pop() {
            if !seen.insert(major) {
                continue;
            }
            let key = (name.clone(), major);
            if self.retained.contains(&key) {
                return true;
            }
            if let Some(next) = self.migrators.get(&key) {
                todo.extend(next.iter().copied());
            }
        }
        false
    }
}

/// Lists everything the span needs and cannot have, before the first step (REQ-005, REQ-006):
/// blob digests the store lacks, command majors neither retained nor reachable by migrators,
/// detached or damaged payloads (named by entry hash), and kernel digests `kernels` cannot
/// supply. Items are ordered by kind (`blob`, `command`, `payload`, `kernel`) then ID; each
/// lists the steps that need it in ascending order. Each blob, command and kernel is looked
/// up once.
pub fn preflight(
    span: &RecordedSpan,
    blobs: &dyn BlobSource,
    commands: &CommandCatalog,
    kernels: &dyn KernelSource,
) -> Vec<MissingItem> {
    let mut missing: BTreeMap<(MissingKind, String), BTreeSet<u64>> = BTreeMap::new();
    let mut blob_ok: BTreeMap<Digest, bool> = BTreeMap::new();
    let mut command_ok: BTreeMap<CommandId, bool> = BTreeMap::new();
    let mut kernel_ok: BTreeMap<Digest, bool> = BTreeMap::new();
    for step in span.steps() {
        let mut need = |kind: MissingKind, id: String| {
            missing.entry((kind, id)).or_default().insert(step.seq);
        };
        let mut payload_ok = step.payload.is_some();
        for o in &step.observations {
            match o.storage {
                Storage::Blob => {
                    if !*blob_ok.entry(o.digest).or_insert_with(|| blobs.has(&o.digest)) {
                        need(MissingKind::Blob, o.digest.to_string());
                    }
                }
                Storage::Inline => {
                    let good = step
                        .payload
                        .as_ref()
                        .and_then(|p| p.inline.get(&o.digest))
                        .is_some_and(|b| Digest::from_bytes(digest_bytes(b).0) == o.digest);
                    payload_ok &= good;
                }
            }
        }
        if !payload_ok {
            need(MissingKind::Payload, step.entry.to_string());
        }
        if let Some(c) = &step.command {
            if !*command_ok.entry(c.clone()).or_insert_with(|| commands.reachable(c)) {
                need(MissingKind::Command, c.as_str().to_string());
            }
        }
        if let Some(k) = &step.kernel {
            if !*kernel_ok.entry(*k).or_insert_with(|| kernels.has(k)) {
                need(MissingKind::Kernel, k.to_string());
            }
        }
    }
    missing
        .into_iter()
        .map(|((kind, id), steps)| MissingItem { kind, id, steps: steps.into_iter().collect() })
        .collect()
}

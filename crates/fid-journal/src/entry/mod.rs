//! Entry model, salted commitments and session entries. Spec node `CORE.JRN.ENTRY`.
//!
//! An entry is a hashed [`EntryHeader`] plus a detachable [`EntryPayload`]. Every
//! detachable field (arguments, event bodies, reason, identities) appears in the header
//! only as a commitment: SHA-256 over a 128-bit salt followed by the field's RFC 8785
//! bytes. The salts and values live in the payload, so deleting the payload leaves the
//! header hash and every commitment verifiable (REQ-010). The entry hash is SHA-256 over
//! the RFC 8785 header with `hash` omitted (REQ-002); headers are checked against the
//! rules of `schema/entry.schema.json` before they are hashed (REQ-011).
//!
//! Salt generation is class N; the salts are recorded in the payload, and replay passes
//! them back through [`RecordedSalts`], which makes building D0 given the salts.

use fid_canon::{digest_bytes, to_canonical_bytes, CValue, CanonError, Digest};
use fid_kci::AlgorithmId;
use fid_types::{Determinism, SideEffect};
use std::collections::BTreeMap;
use std::fmt;

mod json;

pub use json::{
    validate_header_value, verify_header_value, ALGORITHM_PATTERN, BRANCH_PATTERN, COMMAND_ID_PATTERN,
    COMMAND_REQUIRED, DIGEST_PATTERN, HEADER_OPTIONAL, HEADER_REQUIRED, IDENT_PATTERN, SEQ_PATTERN, U64_PATTERN,
    UTC_PATTERN,
};

/// Header schema version written by this module.
pub const SCHEMA_VERSION: u32 = 1;
/// Reserved output digest of a deleted entity: `sha256:` followed by 64 zeros (REQ-012).
/// Only an output that names its entity may carry it.
pub const TOMBSTONE: Digest = Digest([0; 32]);
/// `prev` of sequence 1, the first entry of a stream: also `sha256:` followed by 64 zeros
/// (`CORE.JRN.CHAIN.APPEND` REQ-002). `prev` is always a digest, never null.
pub const GENESIS_PREV: Digest = Digest([0; 32]);
/// The first sequence number of a stream.
pub const FIRST_SEQ: u64 = 1;
/// Input-digest tag of a consumed R1 result (REQ-008, D-031).
pub const R1_RESULT_TAG: &str = "r1_result";
/// Salt length in bytes (128 bits, REQ-003).
pub const SALT_BYTES: usize = 16;

// ------------------------------------------------------------------ errors

/// Typed errors with stable codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryError {
    /// The header or draft does not match `entry.schema.json`.
    Schema(String),
    /// A committed or hashed value holds NaN or infinity (from `fid-canon`).
    NonFiniteNumber,
    /// Replay was given no recorded salt for this field.
    MissingSalt(Field),
    /// The operating system's random source failed.
    RngUnavailable,
    /// A header's `hash` field does not equal the recomputed entry hash.
    HashMismatch,
}

impl EntryError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            EntryError::Schema(_) => "schema",
            EntryError::NonFiniteNumber => "non_finite_number",
            EntryError::MissingSalt(_) => "missing_salt",
            EntryError::RngUnavailable => "rng_unavailable",
            EntryError::HashMismatch => "hash_mismatch",
        }
    }
}

impl fmt::Display for EntryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EntryError::Schema(m) => write!(f, "schema: {m}"),
            EntryError::MissingSalt(field) => write!(f, "missing_salt: {field}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for EntryError {}

impl From<CanonError> for EntryError {
    fn from(e: CanonError) -> Self {
        match e {
            CanonError::NonFiniteNumber => EntryError::NonFiniteNumber,
            other => EntryError::Schema(other.to_string()),
        }
    }
}

/// Why [`verify_commitment`] failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitError {
    /// The header holds no commitment for this field.
    NotCommitted,
    /// The salt or the value is gone (the payload was removed).
    PayloadMissing,
    /// Salt and value are present but do not reproduce the commitment.
    Mismatch,
    /// The value holds NaN or infinity.
    NonFiniteNumber,
}

impl CommitError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            CommitError::NotCommitted => "not_committed",
            CommitError::PayloadMissing => "payload_missing",
            CommitError::Mismatch => "commitment_mismatch",
            CommitError::NonFiniteNumber => "non_finite_number",
        }
    }
}

impl fmt::Display for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for CommitError {}

// ------------------------------------------------------------------ enums

macro_rules! wire_enum {
    ($(#[$m:meta])* $name:ident, $field:literal { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($variant,)+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The wire string.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $s,)+
                }
            }

            /// Parses the wire string; anything else is a `schema` error.
            pub fn parse(s: &str) -> Result<Self, EntryError> {
                match s {
                    $($s => Ok($name::$variant),)+
                    other => Err(EntryError::Schema(format!("{}: unknown value {other:?}", $field))),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

wire_enum!(
    /// Entry kind.
    EntryKind, "kind" {
        Command => "command", SessionHeader => "session_header",
        PrincipalUpgrade => "principal_upgrade", HeadMove => "head_move",
    }
);
wire_enum!(
    /// AI-assistance level; mandatory on every entry (REQ-004).
    AiAssistance, "ai_assistance" { None => "none", Suggested => "suggested", Authored => "authored", Executed => "executed" }
);
wire_enum!(
    /// Principal kind (CONSTITUTION §3, D-012).
    PrincipalKind, "principal.kind" {
        Human => "human", Agent => "agent", Script => "script",
        Service => "service", Device => "device", System => "system",
    }
);
wire_enum!(
    /// How a person confirmed a session upgrade (REQ-007).
    ConfirmMethod, "upgrade.method" { GuiPrompt => "gui_prompt", OsPresence => "os_presence" }
);
wire_enum!(
    /// A detachable field. Its name keys the salt and the value in the payload.
    Field, "field" {
        Args => "args", Events => "events", Identity => "identity",
        OnBehalfOf => "on_behalf_of", Reason => "reason", UpgradeIdentity => "upgrade_identity",
    }
);

// ------------------------------------------------------------------ digests and salts

/// Parses `sha256:` + 64 lowercase hex digits.
pub fn parse_digest(s: &str) -> Option<Digest> {
    let hex = s.strip_prefix("sha256:")?;
    if hex.len() != 64 || !hex.bytes().all(is_lower_hex) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(Digest(out))
}

fn is_lower_hex(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
}

/// A 128-bit commitment salt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Salt(pub [u8; SALT_BYTES]);

impl Salt {
    /// 32 lowercase hex digits.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Parses exactly 32 lowercase hex digits.
    pub fn from_hex(s: &str) -> Result<Salt, EntryError> {
        let bad = || EntryError::Schema(format!("salt: expected 32 lowercase hex digits, got {s:?}"));
        if s.len() != 2 * SALT_BYTES || !s.bytes().all(is_lower_hex) {
            return Err(bad());
        }
        let mut out = [0u8; SALT_BYTES];
        for (i, o) in out.iter_mut().enumerate() {
            let pair = s.get(2 * i..2 * i + 2).ok_or_else(bad)?;
            *o = u8::from_str_radix(pair, 16).map_err(|_| bad())?;
        }
        Ok(Salt(out))
    }
}

/// SHA-256 over the salt followed by the value's RFC 8785 bytes (REQ-003).
pub fn commitment(salt: &Salt, value: &CValue) -> Result<Digest, EntryError> {
    let canonical = to_canonical_bytes(value)?;
    let mut bytes = Vec::with_capacity(SALT_BYTES + canonical.len());
    bytes.extend_from_slice(&salt.0);
    bytes.extend_from_slice(&canonical);
    Ok(digest_bytes(&bytes))
}

/// Where commitment salts come from: fresh ones when recording, recorded ones on replay.
pub trait SaltSource {
    /// The salt for `field`.
    fn salt(&mut self, field: Field) -> Result<Salt, EntryError>;
}

/// Fresh salts from the operating system's CSPRNG (class N).
#[derive(Debug, Clone, Copy, Default)]
pub struct OsSalts;

impl SaltSource for OsSalts {
    fn salt(&mut self, _field: Field) -> Result<Salt, EntryError> {
        let mut b = [0u8; SALT_BYTES];
        getrandom::getrandom(&mut b).map_err(|_| EntryError::RngUnavailable)?;
        Ok(Salt(b))
    }
}

/// Salts recorded in a payload, for replay (AC-7).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordedSalts(pub BTreeMap<Field, Salt>);

impl SaltSource for RecordedSalts {
    fn salt(&mut self, field: Field) -> Result<Salt, EntryError> {
        self.0.get(&field).copied().ok_or(EntryError::MissingSalt(field))
    }
}

// ------------------------------------------------------------------ shared parts

/// Entry time as recorded by the host (REQ-005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryTime {
    /// RFC 3339 UTC instant ending in `Z`.
    pub utc: String,
    /// Monotonic timestamp in nanoseconds (a decimal string on the wire).
    pub mono_ns: u64,
    /// Clock-sync state as the host reports it.
    pub sync: String,
}

/// Registry and kernel manifest digests in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Manifests {
    pub registry: Digest,
    pub kernel: Digest,
}

/// An input digest with its role tag, e.g. `param`, `entity` or `r1_result`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedDigest {
    pub tag: String,
    pub digest: Digest,
}

/// An output digest, with the stable ID of the entity it belongs to when there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDigest {
    pub entity: Option<String>,
    pub digest: Digest,
}

impl OutputDigest {
    /// The output recording that `entity` was deleted: the tombstone digest (REQ-012).
    pub fn deleted(entity: impl Into<String>) -> Self {
        OutputDigest { entity: Some(entity.into()), digest: TOMBSTONE }
    }
}

/// Session facts written by a `session_header` entry (REQ-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub session_id: String,
    pub app_version: String,
}

/// A head move between two logical states (the rules belong to `CORE.JRN.UNDO`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadMove {
    pub from: Digest,
    pub to: Digest,
}

// ------------------------------------------------------------------ drafts

/// The acting principal as the bus supplies it; the identities are committed, never stored.
#[derive(Debug, Clone, PartialEq)]
pub struct PrincipalDraft {
    pub kind: PrincipalKind,
    pub identity: CValue,
    pub on_behalf_of: Option<CValue>,
}

/// The command part of a draft.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandDraft {
    pub id: String,
    pub major: u32,
    pub class: SideEffect,
    pub determinism: Determinism,
    pub seed: u64,
    pub args: CValue,
    pub events: Vec<CValue>,
    pub inputs: Vec<TaggedDigest>,
    /// Digests of R1 results the command consumed; appended to the inputs tagged `r1_result`.
    pub consumed_r1: Vec<Digest>,
    pub outputs: Vec<OutputDigest>,
    pub algorithms: Vec<AlgorithmId>,
}

/// The `principal_upgrade` part of a draft (REQ-007, D-012).
#[derive(Debug, Clone, PartialEq)]
pub struct UpgradeDraft {
    pub session_id: String,
    pub method: ConfirmMethod,
    /// The confirming person's identity; committed, never stored in the header.
    pub identity: CValue,
    /// RFC 3339 UTC instant of the confirmation.
    pub confirmed_at: String,
}

/// The kind-specific part of a draft.
#[derive(Debug, Clone, PartialEq)]
pub enum DraftBody {
    Command(CommandDraft),
    SessionHeader(SessionInfo),
    PrincipalUpgrade(UpgradeDraft),
    HeadMove(HeadMove),
}

/// What the bus supplies: an entry without sequence, previous hash and hash.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryDraft {
    pub stream: String,
    pub branch: String,
    pub logical_parent: Option<Digest>,
    pub time: EntryTime,
    pub principal: PrincipalDraft,
    pub ai_assistance: AiAssistance,
    pub manifests: Manifests,
    pub reason: Option<String>,
    pub human_assertion: Option<String>,
    pub body: DraftBody,
}

impl EntryDraft {
    /// The draft's entry kind.
    pub fn kind(&self) -> EntryKind {
        match self.body {
            DraftBody::Command(_) => EntryKind::Command,
            DraftBody::SessionHeader(_) => EntryKind::SessionHeader,
            DraftBody::PrincipalUpgrade(_) => EntryKind::PrincipalUpgrade,
            DraftBody::HeadMove(_) => EntryKind::HeadMove,
        }
    }

    /// Reads a draft from JSON (the shape used by the fixture). A missing field, an
    /// unknown field or a value outside its enum is a `schema` error.
    pub fn from_value(v: &CValue) -> Result<Self, EntryError> {
        json::draft_from_value(v)
    }
}

/// The draft of the `session_header` entry that starts a session (REQ-006, AC-5).
pub fn session_start(
    stream: &str,
    branch: &str,
    time: EntryTime,
    principal: PrincipalDraft,
    ai_assistance: AiAssistance,
    manifests: Manifests,
    session: SessionInfo,
) -> EntryDraft {
    EntryDraft {
        stream: stream.to_string(),
        branch: branch.to_string(),
        logical_parent: None,
        time,
        principal,
        ai_assistance,
        manifests,
        reason: None,
        human_assertion: None,
        body: DraftBody::SessionHeader(session),
    }
}

// ------------------------------------------------------------------ header and payload

/// The principal in the header: kind plus identity commitments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrincipalCommit {
    pub kind: PrincipalKind,
    pub commit: Digest,
    pub on_behalf_of_commit: Option<Digest>,
}

/// The command part of a header (REQ-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandHeader {
    pub id: String,
    pub major: u32,
    pub class: SideEffect,
    pub determinism: Determinism,
    pub seed: u64,
    pub args_commit: Digest,
    pub events_commit: Digest,
    pub inputs: Vec<TaggedDigest>,
    pub outputs: Vec<OutputDigest>,
    pub algorithms: Vec<AlgorithmId>,
}

/// The `principal_upgrade` part of a header (REQ-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeHeader {
    pub session_id: String,
    pub method: ConfirmMethod,
    pub identity_commit: Digest,
    pub confirmed_at: String,
}

/// The kind-specific part of a header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderBody {
    Command(CommandHeader),
    SessionHeader(SessionInfo),
    PrincipalUpgrade(UpgradeHeader),
    HeadMove(HeadMove),
}

/// The hashed header of one entry (REQ-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryHeader {
    pub stream: String,
    pub seq: u64,
    /// Hash of the previous entry of the stream; [`GENESIS_PREV`] for sequence 1.
    pub prev: Digest,
    pub branch: String,
    pub logical_parent: Option<Digest>,
    pub time: EntryTime,
    pub principal: PrincipalCommit,
    pub ai_assistance: AiAssistance,
    pub manifests: Manifests,
    pub reason_commit: Option<Digest>,
    pub human_assertion: Option<String>,
    pub body: HeaderBody,
    pub hash: Digest,
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn dg(d: &Digest) -> CValue {
    CValue::Str(d.to_string())
}

fn opt_dg(d: Option<Digest>) -> CValue {
    d.as_ref().map_or(CValue::Null, dg)
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> BTreeMap<String, CValue> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

impl EntryHeader {
    /// The header's entry kind.
    pub fn kind(&self) -> EntryKind {
        match self.body {
            HeaderBody::Command(_) => EntryKind::Command,
            HeaderBody::SessionHeader(_) => EntryKind::SessionHeader,
            HeaderBody::PrincipalUpgrade(_) => EntryKind::PrincipalUpgrade,
            HeaderBody::HeadMove(_) => EntryKind::HeadMove,
        }
    }

    /// The header as JSON without the `hash` field: the entry-hash pre-image.
    pub fn unhashed_value(&self) -> CValue {
        let mut principal = obj([("kind", s(self.principal.kind.as_str())), ("commit", dg(&self.principal.commit))]);
        if let Some(c) = &self.principal.on_behalf_of_commit {
            principal.insert("on_behalf_of_commit".into(), dg(c));
        }
        let mut o = obj([
            ("schema_version", CValue::Num(f64::from(SCHEMA_VERSION))),
            ("kind", s(self.kind().as_str())),
            ("stream", s(&self.stream)),
            ("seq", CValue::U64(self.seq)),
            ("prev", dg(&self.prev)),
            ("branch", s(&self.branch)),
            ("logical_parent", opt_dg(self.logical_parent)),
            (
                "time",
                CValue::Obj(obj([
                    ("utc", s(&self.time.utc)),
                    ("mono_ns", CValue::U64(self.time.mono_ns)),
                    ("sync", s(&self.time.sync)),
                ])),
            ),
            ("principal", CValue::Obj(principal)),
            ("ai_assistance", s(self.ai_assistance.as_str())),
            (
                "manifests",
                CValue::Obj(obj([("registry", dg(&self.manifests.registry)), ("kernel", dg(&self.manifests.kernel))])),
            ),
        ]);
        if let Some(c) = &self.reason_commit {
            o.insert("reason_commit".into(), dg(c));
        }
        if let Some(a) = &self.human_assertion {
            o.insert("human_assertion".into(), s(a));
        }
        let (key, body) = match &self.body {
            HeaderBody::Command(c) => ("command", command_value(c)),
            HeaderBody::SessionHeader(si) => {
                ("session", obj([("session_id", s(&si.session_id)), ("app_version", s(&si.app_version))]))
            }
            HeaderBody::PrincipalUpgrade(u) => (
                "upgrade",
                obj([
                    ("session_id", s(&u.session_id)),
                    ("method", s(u.method.as_str())),
                    ("identity_commit", dg(&u.identity_commit)),
                    ("confirmed_at", s(&u.confirmed_at)),
                ]),
            ),
            HeaderBody::HeadMove(h) => ("head_move", obj([("from", dg(&h.from)), ("to", dg(&h.to))])),
        };
        o.insert(key.into(), CValue::Obj(body));
        CValue::Obj(o)
    }

    /// The full header as JSON, including `hash`.
    pub fn to_value(&self) -> CValue {
        match self.unhashed_value() {
            CValue::Obj(mut o) => {
                o.insert("hash".into(), dg(&self.hash));
                CValue::Obj(o)
            }
            other => other,
        }
    }

    /// SHA-256 of the RFC 8785 header with `hash` omitted (REQ-002).
    pub fn compute_hash(&self) -> Result<Digest, EntryError> {
        Ok(digest_bytes(&to_canonical_bytes(&self.unhashed_value())?))
    }

    /// Checks that `hash` equals the recomputed entry hash.
    pub fn verify_hash(&self) -> Result<(), EntryError> {
        if self.compute_hash()? == self.hash {
            Ok(())
        } else {
            Err(EntryError::HashMismatch)
        }
    }

    /// The digest a human assertion signs: the entry hash of this header with the
    /// `human_assertion` field omitted (REQ-009). It covers `seq` and `prev`, so an
    /// assertion can only be made once the entry's place in the chain is fixed.
    pub fn assertion_preimage(&self) -> Result<Digest, EntryError> {
        let mut bare = self.clone();
        bare.human_assertion = None;
        bare.compute_hash()
    }

    /// The header's commitment for a detachable field, if it has one.
    pub fn commitment(&self, field: Field) -> Option<Digest> {
        match (field, &self.body) {
            (Field::Identity, _) => Some(self.principal.commit),
            (Field::OnBehalfOf, _) => self.principal.on_behalf_of_commit,
            (Field::Reason, _) => self.reason_commit,
            (Field::Args, HeaderBody::Command(c)) => Some(c.args_commit),
            (Field::Events, HeaderBody::Command(c)) => Some(c.events_commit),
            (Field::UpgradeIdentity, HeaderBody::PrincipalUpgrade(u)) => Some(u.identity_commit),
            _ => None,
        }
    }
}

fn command_value(c: &CommandHeader) -> BTreeMap<String, CValue> {
    let inputs = c.inputs.iter().map(|t| CValue::Obj(obj([("tag", s(&t.tag)), ("digest", dg(&t.digest))]))).collect();
    let outputs = c
        .outputs
        .iter()
        .map(|out| {
            let mut o = obj([("digest", dg(&out.digest))]);
            if let Some(e) = &out.entity {
                o.insert("entity".into(), s(e));
            }
            CValue::Obj(o)
        })
        .collect();
    obj([
        ("id", s(&c.id)),
        ("major", CValue::Num(f64::from(c.major))),
        ("class", s(c.class.as_str())),
        ("determinism", s(c.determinism.as_str())),
        ("seed", CValue::U64(c.seed)),
        ("args_commit", dg(&c.args_commit)),
        ("events_commit", dg(&c.events_commit)),
        ("inputs", CValue::Arr(inputs)),
        ("outputs", CValue::Arr(outputs)),
        ("algorithms", CValue::Arr(c.algorithms.iter().map(|a| CValue::Str(a.to_string())).collect())),
    ])
}

/// The detachable payload: each committed value with its salt.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntryPayload {
    pub salts: BTreeMap<Field, Salt>,
    pub values: BTreeMap<Field, CValue>,
}

impl EntryPayload {
    /// The payload as JSON: `{"salts": {field: hex}, field: value, ...}`.
    pub fn to_value(&self) -> CValue {
        let mut o: BTreeMap<String, CValue> =
            self.values.iter().map(|(f, v)| (f.as_str().to_string(), v.clone())).collect();
        let salts = self.salts.iter().map(|(f, salt)| (f.as_str().to_string(), CValue::Str(salt.to_hex()))).collect();
        o.insert("salts".into(), CValue::Obj(salts));
        CValue::Obj(o)
    }

    /// The recorded salts, for replay.
    pub fn recorded_salts(&self) -> RecordedSalts {
        RecordedSalts(self.salts.clone())
    }
}

/// A built entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub header: EntryHeader,
    pub payload: EntryPayload,
}

/// Builds an entry from a draft: commits every detachable field with a salt from
/// `salts`, checks the header against the schema rules and hashes it. `seq` starts at
/// [`FIRST_SEQ`] and `prev` of the first entry of a stream is [`GENESIS_PREV`].
pub fn build(
    draft: &EntryDraft,
    seq: u64,
    prev: Digest,
    salts: &mut dyn SaltSource,
) -> Result<Entry, EntryError> {
    let mut payload = EntryPayload::default();
    let mut commit = |field: Field, value: CValue| -> Result<Digest, EntryError> {
        let salt = salts.salt(field)?;
        let c = commitment(&salt, &value)?;
        payload.salts.insert(field, salt);
        payload.values.insert(field, value);
        Ok(c)
    };
    let principal = PrincipalCommit {
        kind: draft.principal.kind,
        commit: commit(Field::Identity, draft.principal.identity.clone())?,
        on_behalf_of_commit: match &draft.principal.on_behalf_of {
            Some(v) => Some(commit(Field::OnBehalfOf, v.clone())?),
            None => None,
        },
    };
    let reason_commit = match &draft.reason {
        Some(r) => Some(commit(Field::Reason, CValue::Str(r.clone()))?),
        None => None,
    };
    let body = match &draft.body {
        DraftBody::Command(c) => {
            if c.inputs.iter().any(|t| t.tag == R1_RESULT_TAG) {
                return Err(EntryError::Schema(format!(
                    "draft.command.inputs: tag {R1_RESULT_TAG} comes only from consumed_r1"
                )));
            }
            let mut inputs = c.inputs.clone();
            inputs.extend(c.consumed_r1.iter().map(|d| TaggedDigest { tag: R1_RESULT_TAG.to_string(), digest: *d }));
            HeaderBody::Command(CommandHeader {
                id: c.id.clone(),
                major: c.major,
                class: c.class,
                determinism: c.determinism,
                seed: c.seed,
                args_commit: commit(Field::Args, c.args.clone())?,
                events_commit: commit(Field::Events, CValue::Arr(c.events.clone()))?,
                inputs,
                outputs: c.outputs.clone(),
                algorithms: c.algorithms.clone(),
            })
        }
        DraftBody::SessionHeader(si) => HeaderBody::SessionHeader(si.clone()),
        DraftBody::PrincipalUpgrade(u) => HeaderBody::PrincipalUpgrade(UpgradeHeader {
            session_id: u.session_id.clone(),
            method: u.method,
            identity_commit: commit(Field::UpgradeIdentity, u.identity.clone())?,
            confirmed_at: u.confirmed_at.clone(),
        }),
        DraftBody::HeadMove(h) => HeaderBody::HeadMove(*h),
    };
    let mut header = EntryHeader {
        stream: draft.stream.clone(),
        seq,
        prev,
        branch: draft.branch.clone(),
        logical_parent: draft.logical_parent,
        time: draft.time.clone(),
        principal,
        ai_assistance: draft.ai_assistance,
        manifests: draft.manifests,
        reason_commit,
        human_assertion: draft.human_assertion.clone(),
        body,
        hash: TOMBSTONE,
    };
    header.hash = header.compute_hash()?;
    validate_header_value(&header.to_value())?;
    Ok(Entry { header, payload })
}

/// Checks one detachable field against its header commitment. With the payload
/// removed (`salt` or `value` is `None`) this fails with [`CommitError::PayloadMissing`]
/// while the header itself still verifies (REQ-010, AC-2).
pub fn verify_commitment(
    header: &EntryHeader,
    field: Field,
    salt: Option<&Salt>,
    value: Option<&CValue>,
) -> Result<(), CommitError> {
    let want = header.commitment(field).ok_or(CommitError::NotCommitted)?;
    let (Some(salt), Some(value)) = (salt, value) else {
        return Err(CommitError::PayloadMissing);
    };
    match commitment(salt, value) {
        Ok(got) if got == want => Ok(()),
        Ok(_) => Err(CommitError::Mismatch),
        Err(_) => Err(CommitError::NonFiniteNumber),
    }
}

//! Proposal brief schema and `agent.attach_brief@1` command (spec node `AGENT.PROPOSE.BRIEF`).
//!
//! Everything an agent supplies in a [`ProposalBrief`] is untrusted text (C5/C6); every
//! free-text field is declared `"x-untrusted": true` in [`brief_schema`] and
//! [`attach_brief_command_def`] so MCP surfaces wrap it as `{"untrusted": true, "text": ...}`
//! through `AUTO.MCP.TAINT`.
//!
//! - Zero `unsafe`, zero panicking unwraps/expects, bounded iterative JSON parsing, no
//!   `HashMap`/`HashSet`, and root-handle-only CLI file reading (never raw paths).

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_journal::branch::{BranchName, Owner, NAME_PATTERN};
use fid_registry::{CommandDef, DefError};
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Registry command ID and journal event type of `agent.attach_brief@1`.
pub const ATTACH_BRIEF: &str = "agent.attach_brief@1";
const ATTACH_BRIEF_NAME: &str = "agent.attach_brief";
/// CLI subcommand under `fid agent`.
pub const CLI_ATTACH_BRIEF: &str = "attach-brief";
/// MCP tool name.
pub const MCP_ATTACH_BRIEF: &str = "agent_attach_brief";

pub const MIN_GOAL_CHARS: usize = 1;
pub const MAX_GOAL_CHARS: usize = 2_000;
pub const MIN_RATIONALE_STEPS: usize = 1;
pub const MAX_RATIONALE_STEPS: usize = 20;
pub const MIN_RATIONALE_STEP_CHARS: usize = 1;
pub const MAX_RATIONALE_STEP_CHARS: usize = 500;
pub const MAX_ASSUMPTIONS: usize = 20;
pub const MAX_ASSUMPTION_CHARS: usize = 500;
pub const MIN_CONFIDENCE_REASONS: usize = 1;
pub const MAX_CONFIDENCE_REASONS: usize = 5;
pub const MAX_CONFIDENCE_REASON_CHARS: usize = 500;
pub const MAX_CLAIMED_CHARACTERISTICS: usize = 500;
pub const MIN_CLIENT_MODEL_CHARS: usize = 1;
pub const MAX_CLIENT_MODEL_CHARS: usize = 200;
pub const MAX_SKILLS_USED: usize = 50;
pub const MAX_SKILL_REF_CHARS: usize = 128;

/// Largest JSON brief / parameter payload accepted (256 KiB).
pub const MAX_BRIEF_BYTES: usize = 256 * 1024;
/// Deepest JSON nesting accepted for brief payloads.
pub const MAX_BRIEF_DEPTH: usize = 8;
/// Most CLI arguments after `fid agent attach-brief`.
const MAX_CLI_ARGS: usize = 16;

const UUID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";
const DIGEST_PATTERN: &str = "^sha256:[0-9a-f]{64}$";
const SKILL_REF_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._:/@-]{0,127}$";
const LIFECYCLE_SINCE: &str = "2026-11-22";

const BRIEF_KEYS: [&str; 9] = [
    "assumptions",
    "claimed_characteristics",
    "client",
    "confidence",
    "confidence_reasons",
    "goal",
    "model",
    "rationale",
    "skills_used",
];

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn parse_canonical_uuid(s: &str) -> Option<Uuid> {
    let id = Uuid::parse(s).ok()?;
    (id.to_string() == s && id != Uuid::NIL && id != Uuid::MAX).then_some(id)
}

// ------------------------------------------------------------------ errors

/// One invalid field reported inside [`BriefError::InvalidBrief`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldError {
    pub field: String,
    pub reason: String,
}

/// Errors returned by proposal brief validation, root-handle loading, attachment and replay.
/// On any error nothing is appended or changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BriefError {
    /// Target branch exists but is not an agent branch (REQ-001, AC-2).
    NotAgentBranch { branch: String },
    /// Target branch does not exist in the project.
    UnknownBranch { branch: String },
    /// Brief failed validation; `fields` names every offending field at once (REQ-004, REQ-006, AC-3).
    InvalidBrief { fields: Vec<FieldError> },
    /// Malformed CLI/MCP parameters or invalid root handle (C5/C6).
    BadParams(String),
    /// Payload exceeded size or depth budget.
    InputLimit(&'static str),
    /// Replay mismatch or tampered entry digest / bound_head.
    ReplayMismatch(String),
    /// Internal encoding error.
    Internal(&'static str),
}

impl BriefError {
    pub const fn code(&self) -> &'static str {
        match self {
            BriefError::NotAgentBranch { .. } => "not_agent_branch",
            BriefError::UnknownBranch { .. } => "unknown_branch",
            BriefError::InvalidBrief { .. } => "invalid_brief",
            BriefError::BadParams(_) => "bad_params",
            BriefError::InputLimit(_) => "input_limit",
            BriefError::ReplayMismatch(_) => "replay_mismatch",
            BriefError::Internal(_) => "internal",
        }
    }

    /// Names of all offending fields when `self` is [`BriefError::InvalidBrief`].
    pub fn offending_fields(&self) -> Vec<&str> {
        match self {
            BriefError::InvalidBrief { fields } => fields.iter().map(|f| f.field.as_str()).collect(),
            _ => Vec::new(),
        }
    }
}

impl fmt::Display for BriefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BriefError::NotAgentBranch { branch } => write!(f, "not_agent_branch: {branch:?}"),
            BriefError::UnknownBranch { branch } => write!(f, "unknown_branch: {branch:?}"),
            BriefError::InvalidBrief { fields } => {
                f.write_str("invalid_brief: [")?;
                for (i, fe) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}: {}", fe.field, fe.reason)?;
                }
                f.write_str("]")
            }
            BriefError::BadParams(s) => write!(f, "bad_params: {s}"),
            BriefError::InputLimit(s) => write!(f, "input_limit: {s}"),
            BriefError::ReplayMismatch(s) => write!(f, "replay_mismatch: {s}"),
            BriefError::Internal(s) => write!(f, "internal: {s}"),
        }
    }
}

impl std::error::Error for BriefError {}

// ------------------------------------------------------------------ data types

/// Closed qualitative confidence enum (REQ-003, REQ-006: never numeric).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub const ALL: [Confidence; 3] = [Confidence::Low, Confidence::Medium, Confidence::High];

    pub const fn as_str(self) -> &'static str {
        match self {
            Confidence::Low => "low",
            Confidence::Medium => "medium",
            Confidence::High => "high",
        }
    }

    pub fn parse(s: &str) -> Option<Confidence> {
        match s {
            "low" => Some(Confidence::Low),
            "medium" => Some(Confidence::Medium),
            "high" => Some(Confidence::High),
            _ => None,
        }
    }
}

/// A validated Skill or recipe identifier (1..=128 ASCII chars matching [`SKILL_REF_PATTERN`]).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SkillRef(String);

impl SkillRef {
    pub fn parse(s: &str) -> Option<SkillRef> {
        let b = s.as_bytes();
        if b.is_empty() || b.len() > MAX_SKILL_REF_CHARS {
            return None;
        }
        let first = *b.first()?;
        if !first.is_ascii_alphanumeric() {
            return None;
        }
        if !b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'/' | b'@' | b'-')) {
            return None;
        }
        Some(SkillRef(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The agent-authored proposal brief attached to an agent branch (REQ-003..006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalBrief {
    pub goal: String,
    pub rationale: Vec<String>,
    pub assumptions: Vec<String>,
    pub confidence: Confidence,
    pub confidence_reasons: Vec<String>,
    pub claimed_characteristics: Vec<Uuid>,
    pub client: String,
    pub model: String,
    pub skills_used: Vec<SkillRef>,
}

fn push_err(errors: &mut Vec<FieldError>, field: impl Into<String>, reason: impl Into<String>) {
    errors.push(FieldError { field: field.into(), reason: reason.into() });
}

fn validate_str_field(
    map: &BTreeMap<String, CValue>,
    key: &str,
    min_chars: usize,
    max_chars: usize,
    errors: &mut Vec<FieldError>,
) -> Option<String> {
    match map.get(key) {
        None => {
            push_err(errors, key, "missing required field");
            None
        }
        Some(CValue::Str(s)) => {
            let n = s.chars().count();
            if n < min_chars || n > max_chars {
                push_err(errors, key, format!("length {n} outside {min_chars}..={max_chars} characters"));
                None
            } else {
                Some(s.clone())
            }
        }
        Some(_) => {
            push_err(errors, key, "expected string");
            None
        }
    }
}

fn validate_str_list(
    map: &BTreeMap<String, CValue>,
    key: &str,
    min_items: usize,
    max_items: usize,
    min_chars: usize,
    max_chars: usize,
    errors: &mut Vec<FieldError>,
) -> Option<Vec<String>> {
    match map.get(key) {
        None => {
            push_err(errors, key, "missing required field");
            None
        }
        Some(CValue::Arr(arr)) => {
            let mut ok = true;
            if arr.len() < min_items || arr.len() > max_items {
                push_err(errors, key, format!("item count {} outside {min_items}..={max_items}", arr.len()));
                ok = false;
            }
            let mut out = Vec::with_capacity(arr.len().min(max_items));
            for (i, item) in arr.iter().enumerate() {
                match item {
                    CValue::Str(s) => {
                        let n = s.chars().count();
                        if n < min_chars || n > max_chars {
                            push_err(
                                errors,
                                key,
                                format!("item {i} length {n} outside {min_chars}..={max_chars} characters"),
                            );
                            ok = false;
                        } else if ok {
                            out.push(s.clone());
                        }
                    }
                    _ => {
                        push_err(errors, key, format!("item {i} must be a string"));
                        ok = false;
                    }
                }
            }
            ok.then_some(out)
        }
        Some(_) => {
            push_err(errors, key, "expected array of strings");
            None
        }
    }
}

impl ProposalBrief {
    /// Validates the entire brief and collects every offending field into
    /// [`BriefError::InvalidBrief`] before returning (REQ-003, REQ-004, REQ-006).
    pub fn from_value(v: &CValue) -> Result<ProposalBrief, BriefError> {
        let CValue::Obj(o) = v else {
            return Err(BriefError::InvalidBrief {
                fields: vec![FieldError { field: "brief".into(), reason: "must be a JSON object".into() }],
            });
        };

        let mut errors: Vec<FieldError> = Vec::new();

        // REQ-006: reject every unknown field.
        for k in o.keys() {
            if !BRIEF_KEYS.contains(&k.as_str()) {
                push_err(&mut errors, k.clone(), "unknown field not in ProposalBrief schema");
            }
        }

        let goal = validate_str_field(o, "goal", MIN_GOAL_CHARS, MAX_GOAL_CHARS, &mut errors);
        let rationale = validate_str_list(
            o,
            "rationale",
            MIN_RATIONALE_STEPS,
            MAX_RATIONALE_STEPS,
            MIN_RATIONALE_STEP_CHARS,
            MAX_RATIONALE_STEP_CHARS,
            &mut errors,
        );
        let assumptions = validate_str_list(o, "assumptions", 0, MAX_ASSUMPTIONS, 0, MAX_ASSUMPTION_CHARS, &mut errors);

        let confidence = match o.get("confidence") {
            None => {
                push_err(&mut errors, "confidence", "missing required field");
                None
            }
            Some(CValue::Str(s)) => match Confidence::parse(s) {
                Some(c) => Some(c),
                None => {
                    push_err(&mut errors, "confidence", "must be one of 'low', 'medium' or 'high'");
                    None
                }
            },
            Some(_) => {
                push_err(&mut errors, "confidence", "must be a string ('low', 'medium' or 'high'), never a number");
                None
            }
        };

        let confidence_reasons = validate_str_list(
            o,
            "confidence_reasons",
            MIN_CONFIDENCE_REASONS,
            MAX_CONFIDENCE_REASONS,
            0,
            MAX_CONFIDENCE_REASON_CHARS,
            &mut errors,
        );

        let claimed_characteristics = match o.get("claimed_characteristics") {
            None => {
                push_err(&mut errors, "claimed_characteristics", "missing required field");
                None
            }
            Some(CValue::Arr(arr)) => {
                let mut ok = true;
                if arr.len() > MAX_CLAIMED_CHARACTERISTICS {
                    push_err(
                        &mut errors,
                        "claimed_characteristics",
                        format!("count {} exceeds {MAX_CLAIMED_CHARACTERISTICS}", arr.len()),
                    );
                    ok = false;
                }
                let mut out = Vec::with_capacity(arr.len().min(MAX_CLAIMED_CHARACTERISTICS));
                for (i, item) in arr.iter().enumerate() {
                    match item {
                        CValue::Str(s) => match parse_canonical_uuid(s) {
                            Some(id) if ok => out.push(id),
                            Some(_) => {}
                            None => {
                                push_err(
                                    &mut errors,
                                    "claimed_characteristics",
                                    format!("item {i} is not a canonical entity UUID"),
                                );
                                ok = false;
                            }
                        },
                        _ => {
                            push_err(&mut errors, "claimed_characteristics", format!("item {i} must be a UUID string"));
                            ok = false;
                        }
                    }
                }
                ok.then_some(out)
            }
            Some(_) => {
                push_err(&mut errors, "claimed_characteristics", "expected array of entity UUIDs");
                None
            }
        };

        let client = validate_str_field(o, "client", MIN_CLIENT_MODEL_CHARS, MAX_CLIENT_MODEL_CHARS, &mut errors);
        let model = validate_str_field(o, "model", MIN_CLIENT_MODEL_CHARS, MAX_CLIENT_MODEL_CHARS, &mut errors);

        let skills_used = match o.get("skills_used") {
            None => {
                push_err(&mut errors, "skills_used", "missing required field");
                None
            }
            Some(CValue::Arr(arr)) => {
                let mut ok = true;
                if arr.len() > MAX_SKILLS_USED {
                    push_err(&mut errors, "skills_used", format!("count {} exceeds {MAX_SKILLS_USED}", arr.len()));
                    ok = false;
                }
                let mut out = Vec::with_capacity(arr.len().min(MAX_SKILLS_USED));
                for (i, item) in arr.iter().enumerate() {
                    match item {
                        CValue::Str(s) => match SkillRef::parse(s) {
                            Some(sr) if ok => out.push(sr),
                            Some(_) => {}
                            None => {
                                push_err(&mut errors, "skills_used", format!("item {i} is not a valid skill ref"));
                                ok = false;
                            }
                        },
                        _ => {
                            push_err(&mut errors, "skills_used", format!("item {i} must be a string"));
                            ok = false;
                        }
                    }
                }
                ok.then_some(out)
            }
            Some(_) => {
                push_err(&mut errors, "skills_used", "expected array of skill references");
                None
            }
        };

        if !errors.is_empty() {
            errors.sort();
            return Err(BriefError::InvalidBrief { fields: errors });
        }

        match (
            goal,
            rationale,
            assumptions,
            confidence,
            confidence_reasons,
            claimed_characteristics,
            client,
            model,
            skills_used,
        ) {
            (
                Some(goal),
                Some(rationale),
                Some(assumptions),
                Some(confidence),
                Some(confidence_reasons),
                Some(claimed_characteristics),
                Some(client),
                Some(model),
                Some(skills_used),
            ) => Ok(ProposalBrief {
                goal,
                rationale,
                assumptions,
                confidence,
                confidence_reasons,
                claimed_characteristics,
                client,
                model,
                skills_used,
            }),
            _ => Err(BriefError::Internal("validated fields unexpectedly absent")),
        }
    }

    /// Parses bounded JSON bytes into a validated [`ProposalBrief`].
    pub fn from_json(bytes: &[u8]) -> Result<ProposalBrief, BriefError> {
        let v = parse_with_limits(bytes, MAX_BRIEF_BYTES, MAX_BRIEF_DEPTH).map_err(|e| match e {
            CanonError::InputLimit => BriefError::InputLimit("brief"),
            CanonError::DuplicateKey(k) => {
                BriefError::InvalidBrief { fields: vec![FieldError { field: k, reason: "duplicate key".into() }] }
            }
            _ => BriefError::BadParams("brief is not valid JSON".into()),
        })?;
        ProposalBrief::from_value(&v)
    }

    /// Canonical [`CValue`] representation (exact 9 schema fields).
    pub fn to_value(&self) -> CValue {
        obj([
            ("assumptions", CValue::Arr(self.assumptions.iter().map(|s| text(s)).collect())),
            (
                "claimed_characteristics",
                CValue::Arr(self.claimed_characteristics.iter().map(|id| text(&id.to_string())).collect()),
            ),
            ("client", text(&self.client)),
            ("confidence", text(self.confidence.as_str())),
            ("confidence_reasons", CValue::Arr(self.confidence_reasons.iter().map(|s| text(s)).collect())),
            ("goal", text(&self.goal)),
            ("model", text(&self.model)),
            ("rationale", CValue::Arr(self.rationale.iter().map(|s| text(s)).collect())),
            ("skills_used", CValue::Arr(self.skills_used.iter().map(|s| text(s.as_str())).collect())),
        ])
    }

    pub fn to_json(&self) -> Result<Vec<u8>, BriefError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| BriefError::Internal("canonical json"))
    }
}

// ------------------------------------------------------------------ branch heads & brief journal

/// Minimal branch-head view used by `agent.attach_brief@1`, keyed by canonical [`BranchName`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchHeads {
    heads: BTreeMap<BranchName, Digest>,
}

impl BranchHeads {
    pub fn new() -> BranchHeads {
        BranchHeads::default()
    }

    pub fn insert(&mut self, name: BranchName, head: Digest) {
        self.heads.insert(name, head);
    }

    pub fn get(&self, name: &BranchName) -> Option<Digest> {
        self.heads.get(name).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&BranchName, &Digest)> {
        self.heads.iter()
    }

    /// Deterministic digest of the entire branch-head map (for replay equivalence checks).
    pub fn state_digest(&self) -> Result<Digest, BriefError> {
        let mut map = BTreeMap::new();
        for (name, head) in &self.heads {
            map.insert(name.as_str().to_string(), text(head.as_str()));
        }
        let d = fid_canon::digest(&CValue::Obj(map)).map_err(|_| BriefError::Internal("canonical json"))?;
        Ok(Digest::from_bytes(d.0))
    }
}

/// One journaled `agent.attach_brief@1` entry on an agent branch (REQ-002, REQ-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefEntry {
    pub entry_id: Uuid,
    pub seq: u64,
    pub branch: BranchName,
    pub bound_head: Digest,
    pub entry_digest: Digest,
    pub brief: ProposalBrief,
}

impl BriefEntry {
    fn compute_digest(
        entry_id: Uuid,
        seq: u64,
        branch: &BranchName,
        bound_head: Digest,
        brief: &ProposalBrief,
    ) -> Result<Digest, BriefError> {
        let payload = obj([
            ("bound_head", text(bound_head.as_str())),
            ("branch", text(branch.as_str())),
            ("brief", brief.to_value()),
            ("command", text(ATTACH_BRIEF)),
            ("entry_id", text(&entry_id.to_string())),
            ("seq", CValue::U64(seq)),
        ]);
        let d = fid_canon::digest(&payload).map_err(|_| BriefError::Internal("canonical json"))?;
        Ok(Digest::from_bytes(d.0))
    }

    pub fn to_value(&self) -> CValue {
        obj([
            ("bound_head", text(self.bound_head.as_str())),
            ("branch", text(self.branch.as_str())),
            ("brief", self.brief.to_value()),
            ("command", text(ATTACH_BRIEF)),
            ("entry_digest", text(self.entry_digest.as_str())),
            ("entry_id", text(&self.entry_id.to_string())),
            ("seq", CValue::U64(self.seq)),
        ])
    }

    pub fn to_json(&self) -> Result<Vec<u8>, BriefError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| BriefError::Internal("canonical json"))
    }

    pub fn from_value(v: &CValue) -> Result<BriefEntry, BriefError> {
        let CValue::Obj(o) = v else { return Err(BriefError::ReplayMismatch("entry is not an object".into())) };
        const KEYS: [&str; 7] = ["bound_head", "branch", "brief", "command", "entry_digest", "entry_id", "seq"];
        if o.len() != KEYS.len() || !KEYS.iter().all(|k| o.contains_key(*k)) {
            return Err(BriefError::ReplayMismatch("unexpected entry keys".into()));
        }
        match o.get("command") {
            Some(CValue::Str(s)) if s == ATTACH_BRIEF => {}
            _ => return Err(BriefError::ReplayMismatch("unexpected entry command".into())),
        }
        let entry_id = match o.get("entry_id") {
            Some(CValue::Str(s)) => {
                parse_canonical_uuid(s).ok_or_else(|| BriefError::ReplayMismatch("bad entry_id".into()))?
            }
            _ => return Err(BriefError::ReplayMismatch("bad entry_id".into())),
        };
        let seq = match o.get("seq") {
            Some(CValue::Str(s)) => s.parse::<u64>().map_err(|_| BriefError::ReplayMismatch("bad seq".into()))?,
            _ => return Err(BriefError::ReplayMismatch("bad seq".into())),
        };
        let branch = match o.get("branch") {
            Some(CValue::Str(s)) => {
                BranchName::parse(s).map_err(|_| BriefError::ReplayMismatch("bad branch name".into()))?
            }
            _ => return Err(BriefError::ReplayMismatch("bad branch".into())),
        };
        let bound_head = match o.get("bound_head") {
            Some(CValue::Str(s)) => {
                Digest::parse(s).map_err(|_| BriefError::ReplayMismatch("bad bound_head".into()))?
            }
            _ => return Err(BriefError::ReplayMismatch("bad bound_head".into())),
        };
        let entry_digest = match o.get("entry_digest") {
            Some(CValue::Str(s)) => {
                Digest::parse(s).map_err(|_| BriefError::ReplayMismatch("bad entry_digest".into()))?
            }
            _ => return Err(BriefError::ReplayMismatch("bad entry_digest".into())),
        };
        let brief = match o.get("brief") {
            Some(bv) => ProposalBrief::from_value(bv)?,
            None => return Err(BriefError::ReplayMismatch("missing brief".into())),
        };
        let recomputed = BriefEntry::compute_digest(entry_id, seq, &branch, bound_head, &brief)?;
        if recomputed != entry_digest {
            return Err(BriefError::ReplayMismatch("entry_digest mismatch".into()));
        }
        Ok(BriefEntry { entry_id, seq, branch, bound_head, entry_digest, brief })
    }

    pub fn from_json(bytes: &[u8]) -> Result<BriefEntry, BriefError> {
        let v = parse_with_limits(bytes, MAX_BRIEF_BYTES, MAX_BRIEF_DEPTH).map_err(|e| match e {
            CanonError::InputLimit => BriefError::InputLimit("entry"),
            _ => BriefError::ReplayMismatch("invalid entry json".into()),
        })?;
        BriefEntry::from_value(&v)
    }
}

/// Append-only journal of attached proposal briefs across branches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BriefJournal {
    entries: Vec<BriefEntry>,
}

impl BriefJournal {
    pub fn new() -> BriefJournal {
        BriefJournal::default()
    }

    pub fn entries(&self) -> &[BriefEntry] {
        &self.entries
    }

    /// All brief entries attached to `branch`, in append order.
    pub fn entries_on(&self, branch: &BranchName) -> Vec<&BriefEntry> {
        self.entries.iter().filter(|e| &e.branch == branch).collect()
    }

    /// The current brief on `branch`: the latest attached entry wins (REQ-007, AC-5).
    pub fn current_brief(&self, branch: &BranchName) -> Option<&BriefEntry> {
        self.entries.iter().rev().find(|e| &e.branch == branch)
    }
}

/// Command result of `agent.attach_brief@1`: `{ entry_id, bound_head }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachBriefResult {
    pub entry_id: Uuid,
    pub bound_head: Digest,
}

impl AttachBriefResult {
    pub fn to_value(&self) -> CValue {
        obj([("bound_head", text(self.bound_head.as_str())), ("entry_id", text(&self.entry_id.to_string()))])
    }

    pub fn to_json(&self) -> Result<Vec<u8>, BriefError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| BriefError::Internal("canonical json"))
    }
}

/// Executes `agent.attach_brief@1` atomically (REQ-001..007).
///
/// Order:
/// 1. Resolve `branch_name` in `branches` (else `unknown_branch`).
/// 2. Require `branch.owner() == Owner::Agent` (else `not_agent_branch`).
/// 3. Validate the entire `brief_value` (collecting every offending field into `invalid_brief`).
/// 4. Record `bound_head` = branch head immediately before append, compute deterministic
///    `entry_digest`, append to `journal` (on that branch only), and advance the branch head.
pub fn attach_brief(
    branches: &mut BranchHeads,
    journal: &mut BriefJournal,
    branch_name: &str,
    brief_value: &CValue,
    entry_id: Uuid,
) -> Result<(AttachBriefResult, BriefEntry), BriefError> {
    if entry_id == Uuid::NIL || entry_id == Uuid::MAX {
        return Err(BriefError::BadParams("entry_id must be a canonical non-nil non-max UUID".into()));
    }
    let branch =
        BranchName::parse(branch_name).map_err(|_| BriefError::UnknownBranch { branch: branch_name.to_string() })?;
    let bound_head =
        branches.get(&branch).ok_or_else(|| BriefError::UnknownBranch { branch: branch_name.to_string() })?;
    if branch.owner() != Owner::Agent {
        return Err(BriefError::NotAgentBranch { branch: branch_name.to_string() });
    }
    let brief = ProposalBrief::from_value(brief_value)?;
    let seq = (journal.entries.len() as u64).saturating_add(1);
    let entry_digest = BriefEntry::compute_digest(entry_id, seq, &branch, bound_head, &brief)?;
    let entry = BriefEntry { entry_id, seq, branch: branch.clone(), bound_head, entry_digest, brief };

    journal.entries.push(entry.clone());
    branches.insert(branch, entry_digest);
    Ok((AttachBriefResult { entry_id, bound_head }, entry))
}

/// Replays a list of [`BriefEntry`] records from `initial_branches` in exact mode (AC-6),
/// verifying every branch kind, `bound_head`, sequence number and `entry_digest`.
pub fn replay_brief_entries(
    initial_branches: &BranchHeads,
    entries: &[BriefEntry],
) -> Result<(BranchHeads, BriefJournal), BriefError> {
    let mut branches = initial_branches.clone();
    let mut journal = BriefJournal::new();
    for entry in entries {
        let Some(head) = branches.get(&entry.branch) else {
            return Err(BriefError::UnknownBranch { branch: entry.branch.as_str().to_string() });
        };
        if entry.branch.owner() != Owner::Agent {
            return Err(BriefError::NotAgentBranch { branch: entry.branch.as_str().to_string() });
        }
        if head != entry.bound_head {
            return Err(BriefError::ReplayMismatch(format!(
                "bound_head mismatch on {}: expected {head}, got {}",
                entry.branch.as_str(),
                entry.bound_head
            )));
        }
        let expected_seq = (journal.entries.len() as u64).saturating_add(1);
        if entry.seq != expected_seq {
            return Err(BriefError::ReplayMismatch(format!(
                "seq mismatch: expected {expected_seq}, got {}",
                entry.seq
            )));
        }
        let recomputed =
            BriefEntry::compute_digest(entry.entry_id, entry.seq, &entry.branch, entry.bound_head, &entry.brief)?;
        if recomputed != entry.entry_digest {
            return Err(BriefError::ReplayMismatch("entry_digest mismatch".into()));
        }
        journal.entries.push(entry.clone());
        branches.insert(entry.branch.clone(), entry.entry_digest);
    }
    Ok((branches, journal))
}

// ------------------------------------------------------------------ root-handle CLI & MCP entry points

/// Approved read-root handle `<alias>:<relative/path>` (C5/C6: agents never pass raw paths).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootHandle {
    pub root: String,
    pub rel_path: PathBuf,
}

impl RootHandle {
    /// Parses `<alias>:<rel_path>`, rejecting raw paths, absolute paths, backslashes, NUL
    /// bytes, empty aliases or `..` traversal components.
    pub fn parse(raw: &str) -> Result<RootHandle, BriefError> {
        if raw.contains('\0') || raw.contains('\\') {
            return Err(BriefError::BadParams("brief handle contains forbidden path characters".into()));
        }
        let Some((alias, rel)) = raw.split_once(':') else {
            return Err(BriefError::BadParams(
                "brief must be a root handle '<root>:<relative-path>', raw paths are forbidden".into(),
            ));
        };
        if alias.is_empty() || !alias.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')) {
            return Err(BriefError::BadParams("invalid root handle alias".into()));
        }
        if rel.is_empty() || rel.starts_with('/') {
            return Err(BriefError::BadParams("root handle path must be non-empty and relative".into()));
        }
        let rel_path = PathBuf::from(rel);
        for comp in rel_path.components() {
            match comp {
                Component::Normal(_) => {}
                _ => {
                    return Err(BriefError::BadParams(
                        "root handle path must contain only normal relative path segments".into(),
                    ))
                }
            }
        }
        Ok(RootHandle { root: alias.to_string(), rel_path })
    }

    /// Reads the brief JSON from `roots[self.root] / self.rel_path` with a hard byte bound
    /// (`MAX_BRIEF_BYTES + 1` read so oversized files fail closed with `input_limit`).
    pub fn read_bounded(&self, roots: &BTreeMap<String, PathBuf>) -> Result<Vec<u8>, BriefError> {
        let base = roots
            .get(&self.root)
            .ok_or_else(|| BriefError::BadParams(format!("unknown approved root {:?}", self.root)))?;
        let full: PathBuf = base.join(&self.rel_path);
        read_file_bounded(&full, MAX_BRIEF_BYTES)
    }
}

fn read_file_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, BriefError> {
    let f = File::open(path).map_err(|e| BriefError::BadParams(format!("cannot open brief handle target: {e}")))?;
    let mut buf = Vec::new();
    let mut limited = f.take((limit as u64).saturating_add(1));
    limited
        .read_to_end(&mut buf)
        .map_err(|e| BriefError::BadParams(format!("cannot read brief handle target: {e}")))?;
    if buf.len() > limit {
        return Err(BriefError::InputLimit("brief file exceeds 256 KiB"));
    }
    Ok(buf)
}

/// CLI handler for `fid agent attach-brief --branch <name> --brief <root:relpath> [--json]`.
/// Accepts only an approved root handle (never a raw path), reads bounded JSON, and returns
/// canonical JSON bytes plus the appended [`BriefEntry`].
pub fn invoke_attach_brief_cli(
    roots: &BTreeMap<String, PathBuf>,
    branches: &mut BranchHeads,
    journal: &mut BriefJournal,
    args: &[&str],
    entry_id: Uuid,
) -> Result<(Vec<u8>, BriefEntry), BriefError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(BriefError::InputLimit("cli args"));
    }
    let mut branch: Option<&str> = None;
    let mut brief_handle: Option<&str> = None;
    let mut json = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match *arg {
            "--json" if !json => json = true,
            "--json" => return Err(BriefError::BadParams("repeated --json".into())),
            "--branch" => {
                if branch.is_some() {
                    return Err(BriefError::BadParams("repeated --branch".into()));
                }
                branch = Some(*it.next().ok_or_else(|| BriefError::BadParams("--branch needs a value".into()))?);
            }
            "--brief" => {
                if brief_handle.is_some() {
                    return Err(BriefError::BadParams("repeated --brief".into()));
                }
                brief_handle = Some(*it.next().ok_or_else(|| BriefError::BadParams("--brief needs a value".into()))?);
            }
            other => return Err(BriefError::BadParams(format!("unknown option {other:?}"))),
        }
    }
    let branch_s = branch.ok_or_else(|| BriefError::BadParams("missing --branch".into()))?;
    let handle_s = brief_handle.ok_or_else(|| BriefError::BadParams("missing --brief".into()))?;
    let handle = RootHandle::parse(handle_s)?;
    let raw = handle.read_bounded(roots)?;
    let val = parse_with_limits(&raw, MAX_BRIEF_BYTES, MAX_BRIEF_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => BriefError::InputLimit("brief"),
        CanonError::DuplicateKey(k) => {
            BriefError::InvalidBrief { fields: vec![FieldError { field: k, reason: "duplicate key".into() }] }
        }
        _ => BriefError::BadParams("brief file is not valid JSON".into()),
    })?;
    let (res, entry) = attach_brief(branches, journal, branch_s, &val, entry_id)?;
    Ok((res.to_json()?, entry))
}

/// MCP handler for `agent_attach_brief` on bounded JSON `{"branch": "...", "brief": {...}}`.
/// Calls the exact same [`attach_brief`] logic and returns canonical JSON bytes.
pub fn invoke_attach_brief_mcp(
    branches: &mut BranchHeads,
    journal: &mut BriefJournal,
    params_json: &[u8],
    entry_id: Uuid,
) -> Result<(Vec<u8>, BriefEntry), BriefError> {
    let val = parse_with_limits(params_json, MAX_BRIEF_BYTES, MAX_BRIEF_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => BriefError::InputLimit("params"),
        CanonError::DuplicateKey(k) => {
            BriefError::InvalidBrief { fields: vec![FieldError { field: k, reason: "duplicate key".into() }] }
        }
        _ => BriefError::BadParams("params are not valid JSON".into()),
    })?;
    let CValue::Obj(o) = &val else {
        return Err(BriefError::BadParams("params must be a JSON object".into()));
    };
    if o.keys().any(|k| k != "branch" && k != "brief") {
        return Err(BriefError::BadParams("unknown parameter key".into()));
    }
    let branch_s = match o.get("branch") {
        Some(CValue::Str(s)) => s.as_str(),
        _ => return Err(BriefError::BadParams("missing or non-string branch".into())),
    };
    let brief_val = o.get("brief").ok_or_else(|| BriefError::BadParams("missing brief".into()))?;
    let (res, entry) = attach_brief(branches, journal, branch_s, brief_val, entry_id)?;
    Ok((res.to_json()?, entry))
}

// ------------------------------------------------------------------ registry schema & command def

fn untrusted_str_schema(min_len: usize, max_len: usize, description: &str) -> CValue {
    obj([
        ("type", text("string")),
        ("minLength", CValue::Num(min_len as f64)),
        ("maxLength", CValue::Num(max_len as f64)),
        ("x-untrusted", CValue::Bool(true)),
        ("description", text(description)),
    ])
}

/// JSON Schema for [`ProposalBrief`] with `"x-untrusted": true` on every free-text field
/// (`goal`, `rationale`, `assumptions`, `confidence_reasons`, `client`, `model`) (REQ-005).
pub fn brief_schema() -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&BRIEF_KEYS)),
        (
            "properties",
            obj([
                (
                    "assumptions",
                    obj([
                        ("type", text("array")),
                        ("minItems", CValue::Num(0.0)),
                        ("maxItems", CValue::Num(MAX_ASSUMPTIONS as f64)),
                        ("items", untrusted_str_schema(0, MAX_ASSUMPTION_CHARS, "One untrusted assumption.")),
                        ("description", text("Explicit assumptions made by the agent.")),
                    ]),
                ),
                (
                    "claimed_characteristics",
                    obj([
                        ("type", text("array")),
                        ("minItems", CValue::Num(0.0)),
                        ("maxItems", CValue::Num(MAX_CLAIMED_CHARACTERISTICS as f64)),
                        ("items", obj([("type", text("string")), ("pattern", text(UUID_PATTERN))])),
                        ("description", text("Stable entity IDs of characteristics claimed to be affected.")),
                    ]),
                ),
                ("client", untrusted_str_schema(MIN_CLIENT_MODEL_CHARS, MAX_CLIENT_MODEL_CHARS, "Agent client label.")),
                (
                    "confidence",
                    obj([
                        ("type", text("string")),
                        ("enum", strs(&["low", "medium", "high"])),
                        ("description", text("Qualitative confidence band (never numeric).")),
                    ]),
                ),
                (
                    "confidence_reasons",
                    obj([
                        ("type", text("array")),
                        ("minItems", CValue::Num(MIN_CONFIDENCE_REASONS as f64)),
                        ("maxItems", CValue::Num(MAX_CONFIDENCE_REASONS as f64)),
                        (
                            "items",
                            untrusted_str_schema(0, MAX_CONFIDENCE_REASON_CHARS, "One reason for the confidence band."),
                        ),
                        ("description", text("Reasons supporting the qualitative confidence band.")),
                    ]),
                ),
                ("goal", untrusted_str_schema(MIN_GOAL_CHARS, MAX_GOAL_CHARS, "Agent-authored goal statement.")),
                ("model", untrusted_str_schema(MIN_CLIENT_MODEL_CHARS, MAX_CLIENT_MODEL_CHARS, "Agent model label.")),
                (
                    "rationale",
                    obj([
                        ("type", text("array")),
                        ("minItems", CValue::Num(MIN_RATIONALE_STEPS as f64)),
                        ("maxItems", CValue::Num(MAX_RATIONALE_STEPS as f64)),
                        (
                            "items",
                            untrusted_str_schema(
                                MIN_RATIONALE_STEP_CHARS,
                                MAX_RATIONALE_STEP_CHARS,
                                "One rationale step.",
                            ),
                        ),
                        ("description", text("Step-by-step rationale written by the agent.")),
                    ]),
                ),
                (
                    "skills_used",
                    obj([
                        ("type", text("array")),
                        ("minItems", CValue::Num(0.0)),
                        ("maxItems", CValue::Num(MAX_SKILLS_USED as f64)),
                        ("items", obj([("type", text("string")), ("pattern", text(SKILL_REF_PATTERN))])),
                        ("description", text("Skill or recipe IDs used while building the proposal.")),
                    ]),
                ),
            ]),
        ),
    ])
}

/// The registry definition of `agent.attach_brief@1` (W1, D0, `fs_effect: none`, undo `head_move`).
pub fn attach_brief_command_def() -> Result<CommandDef, DefError> {
    let params_schema = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["branch", "brief"])),
        (
            "properties",
            obj([
                (
                    "branch",
                    obj([
                        ("type", text("string")),
                        ("pattern", text(NAME_PATTERN)),
                        ("description", text("The target agent branch name (must be under agent/).")),
                    ]),
                ),
                ("brief", brief_schema()),
            ]),
        ),
    ]);
    let result_schema = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["bound_head", "entry_id"])),
        (
            "properties",
            obj([
                (
                    "bound_head",
                    obj([
                        ("type", text("string")),
                        ("pattern", text(DIGEST_PATTERN)),
                        ("description", text("Digest of the branch head immediately before attaching the brief.")),
                    ]),
                ),
                (
                    "entry_id",
                    obj([
                        ("type", text("string")),
                        ("pattern", text(UUID_PATTERN)),
                        ("description", text("ID of the appended journal entry.")),
                    ]),
                ),
            ]),
        ),
    ]);
    CommandDef::from_value(obj([
        ("id", text(ATTACH_BRIEF_NAME)),
        ("major", CValue::Num(1.0)),
        ("title", text("Attach proposal brief")),
        (
            "description",
            text(
                "Attaches an untrusted agent-authored proposal brief to an agent branch, recording bound_head \
                 immediately before append. Refuses non-agent branches and reports every invalid field at once.",
            ),
        ),
        ("params_schema", params_schema),
        ("result_schema", result_schema),
        ("side_effect", text("W1")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("keyed")),
        ("fs_effect", text("none")),
        ("undo", text("head_move")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[ATTACH_BRIEF])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}

/// Registry command definitions exported by `fid-agent`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(vec![attach_brief_command_def()?])
}

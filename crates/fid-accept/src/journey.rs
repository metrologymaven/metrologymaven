//! Journey-map format `journey-map@1` and its structural validator (spec node
//! FLOW.PERSONA.SCHEMA).
//!
//! Every journey map in FLOW (persona maps, acceptance-job maps and the setup map) is one
//! JSON file in this format (REQ-FLOW.PERSONA.SCHEMA-001/002). The validator checks the rules
//! that need no registry or spec tree: the closed schema, step numbering, hand-off references,
//! the actor and human-only rule (REQ-003) and the size bounds (REQ-004). Cross-checks against
//! the registry manifest and the spec tree belong to FLOW.PERSONA.CHECK.
//!
//! The map digest is the SHA-256 of the RFC 8785 form of the whole map (REQ-005), computed
//! with the project's canonical-JSON crate.
//!
//! Output is a pure function of the input bytes: findings are sorted by code, map ID, step,
//! field and detail (determinism class D0).

use crate::package::{as_count, is_ident, is_job_id, is_text, shown};
use crate::persona::PHASES;
use fid_canon::CValue;
use fid_types::{CommandId, Digest};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Format tag of a journey map.
pub const FORMAT: &str = "journey-map@1";
/// Largest accepted map, in bytes (REQ-004).
pub const MAX_MAP_BYTES: usize = 1 << 20;
/// Deepest accepted JSON nesting (Design: depth 32). The format itself needs 3.
pub const MAX_DEPTH: usize = 32;
/// Most steps in one map (REQ-004).
pub const MAX_STEPS: usize = 500;
/// Most entries in `personas`.
pub const MAX_PERSONAS: usize = 32;
/// Most entries in `exercised_by`.
pub const MAX_EXERCISED_BY: usize = 64;
/// Longest spec node ID (`owner`) and screen ID, in bytes.
pub const MAX_ID_BYTES: usize = 128;

/// Top-level fields every map has (REQ-001).
pub const REQUIRED_FIELDS: [&str; 9] =
    ["format", "map_id", "subject", "kind", "revision", "app", "phase", "personas", "steps"];
/// Top-level field required for `kind: persona` and refused otherwise (FLOW.PERSONA.MAPS
/// Design: part of the schema for persona maps).
pub const PERSONA_ONLY_FIELD: &str = "exercised_by";
/// Fields every step has (REQ-002), besides exactly one of [`TARGET_FIELDS`].
pub const STEP_REQUIRED_FIELDS: [&str; 5] = ["n", "actor", "owner", "handoff", "human_only"];
/// Optional step fields (REQ-002).
pub const STEP_OPTIONAL_FIELDS: [&str; 2] = ["decision", "phase"];
/// Step target fields: exactly one per step (REQ-002).
pub const TARGET_FIELDS: [&str; 2] = ["command", "screen"];
/// Map kinds and the `JOB.*` group their subject must belong to.
pub const KINDS: [(&str, &str); 3] = [("persona", "PER"), ("job", "ACC"), ("setup", "USE")];
/// Step actors (REQ-002).
pub const ACTORS: [&str; 5] = ["human", "operator", "admin", "agent", "system"];

// ------------------------------------------------------------------ findings

/// Finding codes (spec Design: Errors). Declared in alphabetical order of their text so the
/// derived order sorts findings by code text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Code {
    /// A step is marked `human_only` and attributed to an agent (REQ-003).
    AgentHumanOnly,
    /// A field is missing, unknown or malformed, or the map is not valid JSON (REQ-004).
    Schema,
    /// The map is over 1 MiB, nested deeper than 32 or has more than 500 steps (REQ-004).
    Size,
    /// Step numbers are not 1, 2, 3, ... in list order, or a numeric hand-off does not name
    /// the next step (REQ-002, REQ-004).
    StepOrder,
}

impl Code {
    /// Stable text of the code.
    pub fn as_str(self) -> &'static str {
        match self {
            Code::AgentHumanOnly => "MAP-AGENT-HUMANONLY",
            Code::Schema => "MAP-SCHEMA",
            Code::Size => "MAP-SIZE",
            Code::StepOrder => "MAP-STEP-ORDER",
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One finding: `<CODE> <map_id> [step <k>] <field>: <detail>`.
///
/// `map_id` is the map's ID, or `-` when it is missing or malformed. `step` is the 1-based
/// position of the step in `steps` (equal to its `n` when numbering is correct), or `None`
/// for a map-level finding. `field` is the field concerned, `-` for the map or step as a
/// whole.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: Code,
    pub map_id: String,
    pub step: Option<u64>,
    pub field: String,
    pub detail: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.step {
            Some(k) => write!(f, "{} {} step {} {}: {}", self.code, self.map_id, k, self.field, self.detail),
            None => write!(f, "{} {} {}: {}", self.code, self.map_id, self.field, self.detail),
        }
    }
}

// ------------------------------------------------------------------ data structures

/// Map kind (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Persona,
    Job,
    Setup,
}

impl Kind {
    /// Stable text of the kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Persona => "persona",
            Kind::Job => "job",
            Kind::Setup => "setup",
        }
    }

    fn parse(s: &str) -> Option<Kind> {
        match s {
            "persona" => Some(Kind::Persona),
            "job" => Some(Kind::Job),
            "setup" => Some(Kind::Setup),
            _ => None,
        }
    }

    /// The `JOB.*` group the subject of a map of this kind belongs to.
    pub fn subject_group(self) -> &'static str {
        match self {
            Kind::Persona => "PER",
            Kind::Job => "ACC",
            Kind::Setup => "USE",
        }
    }
}

/// Who performs a step (REQ-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Human,
    Operator,
    Admin,
    Agent,
    System,
}

impl Actor {
    /// Stable text of the actor.
    pub fn as_str(self) -> &'static str {
        match self {
            Actor::Human => "human",
            Actor::Operator => "operator",
            Actor::Admin => "admin",
            Actor::Agent => "agent",
            Actor::System => "system",
        }
    }

    fn parse(s: &str) -> Option<Actor> {
        match s {
            "human" => Some(Actor::Human),
            "operator" => Some(Actor::Operator),
            "admin" => Some(Actor::Admin),
            "agent" => Some(Actor::Agent),
            "system" => Some(Actor::System),
            _ => None,
        }
    }
}

/// A screen ID. Free text until UX publishes a screen inventory (spec Open questions); the
/// validator only checks its syntax.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScreenId(String);

impl ScreenId {
    /// JSON Schema pattern of a screen ID.
    pub const PATTERN: &'static str = "^[A-Za-z][A-Za-z0-9._-]{0,127}$";

    /// Whether `s` is a valid screen ID: 1 to 128 bytes of ASCII letters, digits, `.`, `_`
    /// and `-`, starting with a letter.
    pub fn is_valid(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() <= MAX_ID_BYTES
            && b.first().is_some_and(u8::is_ascii_alphabetic)
            && b.iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
    }

    /// The ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a step does: exactly one registry command or one screen (REQ-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepTarget {
    Command(CommandId),
    Screen(ScreenId),
}

/// Where a step hands over (REQ-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handoff {
    /// The `n` of the next step.
    Step(u64),
    /// Free text, for example naming the persona that takes over.
    Text(String),
}

/// One step of a journey map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub n: u64,
    pub actor: Actor,
    pub target: StepTarget,
    pub owner: String,
    pub handoff: Handoff,
    pub decision: Option<String>,
    pub human_only: bool,
    pub phase: Option<String>,
}

/// A journey map (REQ-001). Steps keep their file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JourneyMap {
    pub format: String,
    pub map_id: String,
    pub subject: String,
    pub kind: Kind,
    pub revision: u64,
    pub app: String,
    pub phase: String,
    pub personas: Vec<String>,
    /// Acceptance jobs that exercise a persona map; `Some` exactly when `kind` is `persona`.
    pub exercised_by: Option<Vec<String>>,
    pub steps: Vec<Step>,
}

/// Result of validating one map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The parsed map. `None` when there is any `MAP-SCHEMA`, `MAP-SIZE` or `MAP-STEP-ORDER`
    /// finding (REQ-004); a map whose only findings are `MAP-AGENT-HUMANONLY` is returned so
    /// the coverage check can show it.
    pub map: Option<JourneyMap>,
    /// SHA-256 of the RFC 8785 form of the map (REQ-005), whenever the bytes parse as JSON
    /// within the bounds.
    pub digest: Option<Digest>,
    /// Findings, sorted.
    pub findings: Vec<Finding>,
}

impl Report {
    /// True when there are no findings.
    pub fn is_ok(&self) -> bool {
        self.findings.is_empty()
    }
}

// ------------------------------------------------------------------ syntax rules

/// Whether `s` is a spec node ID: dot-separated segments of uppercase ASCII letters and
/// digits, each starting with a letter, at most [`MAX_ID_BYTES`] bytes (e.g. `CORE.JRN.CANON`).
pub fn is_node_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_ID_BYTES
        && s.split('.').all(|seg| {
            let b = seg.as_bytes();
            b.first().is_some_and(u8::is_ascii_uppercase)
                && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        })
}

/// Whether `s` is a `JOB.<group>.<two digits>` seed ID of the given group.
pub fn is_job_id_in(s: &str, group: &str) -> bool {
    is_job_id(s) && s.split('.').nth(1) == Some(group)
}

/// Persona IDs (`JOB.PER.<two digits>`) named in free text, in order of appearance.
pub fn persona_mentions(text: &str) -> Vec<&str> {
    const PREFIX: &str = "JOB.PER.";
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = text.get(from..).and_then(|t| t.find(PREFIX)) {
        let start = from + i;
        let end = start + PREFIX.len() + 2;
        let before_ok = start == 0
            || text.as_bytes().get(start - 1).is_some_and(|c| !c.is_ascii_alphanumeric() && *c != b'.');
        let after_ok = text.as_bytes().get(end).is_none_or(|c| !c.is_ascii_alphanumeric());
        if let Some(id) = text.get(start..end) {
            if before_ok && after_ok && is_job_id_in(id, "PER") {
                out.push(id);
            }
        }
        from = start + PREFIX.len();
    }
    out
}

// ------------------------------------------------------------------ validator

type Obj = BTreeMap<String, CValue>;

struct Ctx {
    map_id: String,
    findings: Vec<Finding>,
}

impl Ctx {
    fn push(&mut self, code: Code, step: Option<u64>, field: impl Into<String>, detail: impl Into<String>) {
        self.findings.push(Finding {
            code,
            map_id: self.map_id.clone(),
            step,
            field: field.into(),
            detail: detail.into(),
        });
    }

    fn schema(&mut self, step: Option<u64>, field: &str, detail: impl Into<String>) {
        self.push(Code::Schema, step, field, detail);
    }

    fn text(&mut self, o: &Obj, key: &str, step: Option<u64>, ok: fn(&str) -> bool, what: &str) -> Option<String> {
        match o.get(key) {
            None => {
                self.schema(step, key, "missing");
                None
            }
            Some(CValue::Str(s)) if ok(s) => Some(s.clone()),
            Some(_) => {
                self.schema(step, key, format!("must be {what}"));
                None
            }
        }
    }

    /// A list of unique IDs accepted by `ok`, at most `max` long.
    fn id_list(&mut self, o: &Obj, key: &str, max: usize, ok: impl Fn(&str) -> bool, what: &str) -> Option<Vec<String>> {
        let arr = match o.get(key) {
            None => {
                self.schema(None, key, "missing");
                return None;
            }
            Some(CValue::Arr(a)) if a.len() <= max => a,
            Some(CValue::Arr(_)) => {
                self.schema(None, key, format!("must have at most {max} items"));
                return None;
            }
            Some(_) => {
                self.schema(None, key, "must be a list");
                return None;
            }
        };
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        let mut good = true;
        for (i, v) in arr.iter().enumerate() {
            match v {
                CValue::Str(s) if ok(s) => {
                    if !seen.insert(s.as_str()) {
                        self.schema(None, &format!("{key}[{i}]"), format!("repeats `{s}`"));
                        good = false;
                    }
                    out.push(s.clone());
                }
                _ => {
                    self.schema(None, &format!("{key}[{i}]"), format!("must be {what}"));
                    good = false;
                }
            }
        }
        good.then_some(out)
    }
}

fn phase_ok(s: &str) -> bool {
    PHASES.contains(&s)
}

fn int_field(v: Option<&CValue>) -> Option<u64> {
    v.and_then(as_count)
}

/// Validates one map (REQ-001..REQ-005).
pub fn validate(input: &[u8]) -> Report {
    let mut ctx = Ctx { map_id: "-".to_string(), findings: Vec::new() };
    if input.len() > MAX_MAP_BYTES {
        ctx.push(Code::Size, None, "-", format!("map is {} bytes, over the {MAX_MAP_BYTES}-byte limit", input.len()));
        return Report { map: None, digest: None, findings: ctx.findings };
    }
    let value = match fid_canon::parse_with_limits(input, MAX_MAP_BYTES, MAX_DEPTH) {
        Ok(v) => v,
        Err(fid_canon::CanonError::InputLimit) => {
            ctx.push(Code::Size, None, "-", format!("nested deeper than {MAX_DEPTH}"));
            return Report { map: None, digest: None, findings: ctx.findings };
        }
        Err(e) => {
            ctx.schema(None, "-", format!("not valid JSON ({e})"));
            return Report { map: None, digest: None, findings: ctx.findings };
        }
    };
    let digest = fid_canon::digest(&value).ok().map(|d| Digest::from_bytes(d.0));
    let map = check_value(&value, &mut ctx);
    let mut findings = ctx.findings;
    findings.sort();
    findings.dedup();
    let blocking = findings.iter().any(|f| f.code != Code::AgentHumanOnly);
    Report { map: if blocking { None } else { map }, digest, findings }
}

/// The map digest alone: SHA-256 of the RFC 8785 form of the map (REQ-005). `None` when the
/// bytes are not JSON within the bounds.
pub fn map_digest(input: &[u8]) -> Option<Digest> {
    if input.len() > MAX_MAP_BYTES {
        return None;
    }
    let value = fid_canon::parse_with_limits(input, MAX_MAP_BYTES, MAX_DEPTH).ok()?;
    fid_canon::digest(&value).ok().map(|d| Digest::from_bytes(d.0))
}

fn check_value(value: &CValue, ctx: &mut Ctx) -> Option<JourneyMap> {
    let CValue::Obj(top) = value else {
        ctx.schema(None, "-", "top level is not an object");
        return None;
    };
    if let Some(CValue::Str(id)) = top.get("map_id") {
        if is_ident(id) {
            ctx.map_id = id.clone();
        }
    }
    for key in top.keys() {
        if !REQUIRED_FIELDS.contains(&key.as_str()) && key != PERSONA_ONLY_FIELD {
            ctx.schema(None, &shown(key), "unknown field");
        }
    }
    let format = ctx.text(top, "format", None, |s| s == FORMAT, "`journey-map@1`");
    let map_id = ctx.text(top, "map_id", None, is_ident, "a lowercase identifier (a-z, 0-9, inner `-`, at most 64)");
    let kind = match top.get("kind") {
        None => {
            ctx.schema(None, "kind", "missing");
            None
        }
        Some(CValue::Str(s)) => {
            let k = Kind::parse(s);
            if k.is_none() {
                ctx.schema(None, "kind", "must be one of `persona`, `job`, `setup`");
            }
            k
        }
        Some(_) => {
            ctx.schema(None, "kind", "must be one of `persona`, `job`, `setup`");
            None
        }
    };
    let subject = ctx.text(top, "subject", None, is_job_id, "a `JOB.*` seed ID");
    if let (Some(k), Some(s)) = (kind, subject.as_deref()) {
        if !is_job_id_in(s, k.subject_group()) {
            ctx.schema(None, "subject", format!("a `{}` map's subject must be a `JOB.{}.*` ID", k.as_str(), k.subject_group()));
        }
    }
    let revision = match top.get("revision") {
        None => {
            ctx.schema(None, "revision", "missing");
            None
        }
        Some(v) => match as_count(v) {
            Some(r) if r >= 1 => Some(r),
            _ => {
                ctx.schema(None, "revision", "must be an integer of at least 1");
                None
            }
        },
    };
    let app = ctx.text(top, "app", None, is_ident, "a lowercase identifier (a-z, 0-9, inner `-`, at most 64)");
    let phase = ctx.text(top, "phase", None, phase_ok, "a phase (P0, P1a, P1b, P2, P3, P4, later)");
    let personas =
        ctx.id_list(top, "personas", MAX_PERSONAS, |s| is_job_id_in(s, "PER"), "a `JOB.PER.*` persona ID");
    if let (Some(Kind::Persona), Some(s), Some(p)) = (kind, subject.as_deref(), personas.as_ref()) {
        if !p.iter().any(|x| x == s) {
            ctx.schema(None, "personas", format!("must include the subject `{s}`"));
        }
    }
    let exercised_by = match kind {
        Some(Kind::Persona) => ctx
            .id_list(top, PERSONA_ONLY_FIELD, MAX_EXERCISED_BY, |s| is_job_id_in(s, "ACC"), "a `JOB.ACC.*` job ID")
            .map(Some),
        Some(k) => {
            if top.contains_key(PERSONA_ONLY_FIELD) {
                ctx.schema(None, PERSONA_ONLY_FIELD, format!("only allowed on `persona` maps, not `{}`", k.as_str()));
            }
            Some(None)
        }
        None => Some(None),
    };
    let steps = check_steps(top, personas.as_deref(), ctx);
    Some(JourneyMap {
        format: format?,
        map_id: map_id?,
        subject: subject?,
        kind: kind?,
        revision: revision?,
        app: app?,
        phase: phase?,
        personas: personas?,
        exercised_by: exercised_by?,
        steps: steps?,
    })
}

fn check_steps(top: &Obj, personas: Option<&[String]>, ctx: &mut Ctx) -> Option<Vec<Step>> {
    let arr = match top.get("steps") {
        None => {
            ctx.schema(None, "steps", "missing");
            return None;
        }
        Some(CValue::Arr(a)) => a,
        Some(_) => {
            ctx.schema(None, "steps", "must be a list");
            return None;
        }
    };
    if arr.is_empty() {
        ctx.schema(None, "steps", "must not be empty");
        return None;
    }
    if arr.len() > MAX_STEPS {
        ctx.push(Code::Size, None, "steps", format!("{} steps, over the limit of {MAX_STEPS}", arr.len()));
        return None;
    }
    // Declared step numbers by position, read first so hand-offs can name the next one.
    let declared: Vec<Option<u64>> = arr
        .iter()
        .map(|s| match s {
            CValue::Obj(o) => int_field(o.get("n")),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    let mut good = true;
    for (i, v) in arr.iter().enumerate() {
        let pos = i as u64 + 1;
        let next = declared.get(i + 1).copied();
        match check_step(v, pos, next, personas, ctx) {
            Some(s) => out.push(s),
            None => good = false,
        }
    }
    good.then_some(out)
}

fn check_step(v: &CValue, pos: u64, next: Option<Option<u64>>, personas: Option<&[String]>, ctx: &mut Ctx) -> Option<Step> {
    let at = Some(pos);
    let CValue::Obj(o) = v else {
        ctx.schema(at, "-", "step is not an object");
        return None;
    };
    for key in o.keys() {
        let k = key.as_str();
        if !STEP_REQUIRED_FIELDS.contains(&k) && !STEP_OPTIONAL_FIELDS.contains(&k) && !TARGET_FIELDS.contains(&k) {
            ctx.schema(at, &shown(key), "unknown field");
        }
    }
    let n = match o.get("n") {
        None => {
            ctx.schema(at, "n", "missing");
            None
        }
        Some(v) => match as_count(v) {
            Some(n) if n >= 1 => {
                if n != pos {
                    ctx.push(Code::StepOrder, at, "n", format!("declares n = {n} at position {pos}; steps are numbered 1, 2, 3, ... in order"));
                }
                Some(n)
            }
            _ => {
                ctx.schema(at, "n", "must be an integer of at least 1");
                None
            }
        },
    };
    let actor = match o.get("actor") {
        None => {
            ctx.schema(at, "actor", "missing");
            None
        }
        Some(v) => {
            let a = match v {
                CValue::Str(s) => Actor::parse(s),
                _ => None,
            };
            if a.is_none() {
                ctx.schema(at, "actor", "must be one of `human`, `operator`, `admin`, `agent`, `system`");
            }
            a
        }
    };
    let target = match (o.get("command"), o.get("screen")) {
        (Some(_), Some(_)) => {
            ctx.schema(at, "-", "has both `command` and `screen`; exactly one is allowed");
            None
        }
        (None, None) => {
            ctx.schema(at, "-", "has neither `command` nor `screen`; exactly one is required");
            None
        }
        (Some(c), None) => match c {
            CValue::Str(s) => match CommandId::parse(s) {
                Ok(id) => Some(StepTarget::Command(id)),
                Err(_) => {
                    ctx.schema(at, "command", "must be a command ID `domain.verb_object@major`");
                    None
                }
            },
            _ => {
                ctx.schema(at, "command", "must be a command ID `domain.verb_object@major`");
                None
            }
        },
        (None, Some(sc)) => match sc {
            CValue::Str(s) if ScreenId::is_valid(s) => Some(StepTarget::Screen(ScreenId(s.clone()))),
            _ => {
                ctx.schema(at, "screen", "must be a screen ID (a letter, then letters, digits, `.`, `_`, `-`; at most 128)");
                None
            }
        },
    };
    let owner = ctx.text(o, "owner", at, is_node_id, "a spec node ID such as `CORE.JRN.CANON`");
    let handoff = match o.get("handoff") {
        None => {
            ctx.schema(at, "handoff", "missing");
            None
        }
        Some(CValue::Str(s)) if is_text(s) => Some(Handoff::Text(s.clone())),
        Some(v @ CValue::Num(_)) => match as_count(v) {
            Some(h) => {
                match next {
                    None => ctx.push(Code::StepOrder, at, "handoff", format!("names step {h}, but this is the last step")),
                    Some(Some(nx)) if nx != h => {
                        ctx.push(Code::StepOrder, at, "handoff", format!("names step {h}, but the next step is {nx}"));
                    }
                    _ => {}
                }
                Some(Handoff::Step(h))
            }
            None => {
                ctx.schema(at, "handoff", "must be text or the next step's number");
                None
            }
        },
        Some(_) => {
            ctx.schema(at, "handoff", "must be text (1 to 256 bytes, no control characters) or the next step's number");
            None
        }
    };
    let decision = match o.get("decision") {
        None => Some(None),
        Some(CValue::Str(s)) if is_text(s) => Some(Some(s.clone())),
        Some(_) => {
            ctx.schema(at, "decision", "must be text (1 to 256 bytes, no control characters)");
            None
        }
    };
    let human_only = match o.get("human_only") {
        None => {
            ctx.schema(at, "human_only", "missing");
            None
        }
        Some(CValue::Bool(b)) => Some(*b),
        Some(_) => {
            ctx.schema(at, "human_only", "must be true or false");
            None
        }
    };
    let phase = match o.get("phase") {
        None => Some(None),
        Some(CValue::Str(s)) if phase_ok(s) => Some(Some(s.clone())),
        Some(_) => {
            ctx.schema(at, "phase", "must be a phase (P0, P1a, P1b, P2, P3, P4, later)");
            None
        }
    };
    if let Some(list) = personas {
        for (field, text) in [("handoff", o.get("handoff")), ("decision", o.get("decision"))] {
            if let Some(CValue::Str(t)) = text {
                for id in persona_mentions(t) {
                    if !list.iter().any(|p| p == id) {
                        ctx.schema(at, field, format!("names `{id}`, which is not in `personas`"));
                    }
                }
            }
        }
    }
    if human_only == Some(true) && actor == Some(Actor::Agent) {
        ctx.push(Code::AgentHumanOnly, at, "actor", "step is human-only but attributed to an agent");
    }
    Some(Step {
        n: n?,
        actor: actor?,
        target: target?,
        owner: owner?,
        handoff: handoff?,
        decision: decision?,
        human_only: human_only?,
        phase: phase?,
    })
}

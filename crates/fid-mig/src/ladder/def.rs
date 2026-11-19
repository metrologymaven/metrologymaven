//! The migration ladder data file `mig-ladder@1` and its stage-independence check (spec node
//! `MIG.LADDER.DEF`).
//!
//! The ladder names five stages (`coexist`, `templates`, `re_evaluate`, `history`,
//! `programs`), what each needs and produces, the registry commands behind it (with the spec
//! node that owns each command and whether it is `built` or `planned`), the evaluation basis
//! its results carry, its prerequisites and its completion evidence.
//!
//! [`load`] parses and schema-checks a ladder file (bounded parse; findings `LADDER-SCHEMA`)
//! and exposes its version and the SHA-256 of its RFC 8785 canonical JSON
//! (REQ-MIG.LADDER.DEF-005). [`check`] runs the whole build check in the order of the leaf's
//! algorithm: schema, independence (`LADDER-FORWARD`), registry manifest (`LADDER-MISSING-CMD`)
//! and basis rule (`LADDER-BASIS`). Findings are reported in that order, stages in ladder
//! order, so the output is identical across runs (D0).

use fid_canon::CValue;
use fid_types::{CommandId, Digest};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The schema tag of the ladder file.
pub const SCHEMA: &str = "mig-ladder@1";
/// Largest accepted ladder file, in bytes.
pub const MAX_LADDER_BYTES: usize = 1024 * 1024;
/// Deepest accepted nesting in a ladder file.
pub const MAX_LADDER_DEPTH: usize = 16;
/// Largest accepted registry manifest, in bytes (the `fid-canon` input limit).
pub const MAX_MANIFEST_BYTES: usize = fid_canon::MAX_INPUT_BYTES;
/// Most entries in any one list of a stage.
pub const MAX_ITEMS: usize = 256;
/// Longest accepted identifier (input or output kind, spec node ID, phase).
pub const MAX_IDENT: usize = 64;
/// The stage IDs, in ladder order (stage `n` is at index `n - 1`).
pub const STAGE_IDS: [&str; 5] = ["coexist", "templates", "re_evaluate", "history", "programs"];

const TOP_KEYS: [&str; 3] = ["schema", "stages", "version"];
const STAGE_KEYS: [&str; 9] =
    ["basis", "commands", "completion_evidence", "id", "inputs", "order", "outputs", "phase", "prerequisites"];
const COMMAND_KEYS: [&str; 3] = ["availability", "command_id", "spec_node"];

/// A `MAJOR.MINOR.PATCH` version (no pre-release or build suffix, no leading zeros).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl SemVer {
    /// Parses `MAJOR.MINOR.PATCH`; `None` when the text is not exactly that.
    pub fn parse(s: &str) -> Option<SemVer> {
        let mut parts = s.split('.');
        let mut next = || -> Option<u64> {
            let p = parts.next()?;
            if p.is_empty() || p.len() > 19 || !p.bytes().all(|b| b.is_ascii_digit()) || (p.len() > 1 && p.starts_with('0'))
            {
                return None;
            }
            p.parse().ok()
        };
        let v = SemVer { major: next()?, minor: next()?, patch: next()? };
        if parts.next().is_some() {
            return None;
        }
        Some(v)
    }
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The evaluation basis a stage's results carry (CONSTITUTION data model `evaluation_basis`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvaluationBasis {
    Points,
    Features,
    ResultsOnly,
    DirectReading,
}

impl EvaluationBasis {
    /// Every basis, in declaration order.
    pub const ALL: [EvaluationBasis; 4] =
        [EvaluationBasis::Points, EvaluationBasis::Features, EvaluationBasis::ResultsOnly, EvaluationBasis::DirectReading];

    /// The wire form, e.g. `RESULTS_ONLY`.
    pub fn as_str(self) -> &'static str {
        match self {
            EvaluationBasis::Points => "POINTS",
            EvaluationBasis::Features => "FEATURES",
            EvaluationBasis::ResultsOnly => "RESULTS_ONLY",
            EvaluationBasis::DirectReading => "DIRECT_READING",
        }
    }

    /// Parses the wire form.
    pub fn parse(s: &str) -> Option<EvaluationBasis> {
        EvaluationBasis::ALL.into_iter().find(|b| b.as_str() == s)
    }
}

/// Whether a stage command exists in this build (`built`) or is still to come (`planned`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Availability {
    Built,
    Planned,
}

impl Availability {
    /// The wire form, `built` or `planned`.
    pub fn as_str(self) -> &'static str {
        match self {
            Availability::Built => "built",
            Availability::Planned => "planned",
        }
    }
}

/// One registry command that implements a stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageCommand {
    pub command_id: CommandId,
    /// The spec node that owns the command, e.g. `MIG.TEMPL.MAP`.
    pub spec_node: String,
    pub availability: Availability,
}

/// A stage prerequisite: an input kind or a command that must already be usable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Prereq {
    Input(String),
    Command(CommandId),
}

impl fmt::Display for Prereq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Prereq::Input(k) => write!(f, "input:{k}"),
            Prereq::Command(c) => write!(f, "command:{}", c.as_str()),
        }
    }
}

/// One ladder stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage {
    pub id: String,
    /// 1 to 5.
    pub order: u8,
    /// Spec phase tag, e.g. `P1a`.
    pub phase: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub commands: Vec<StageCommand>,
    pub basis: Vec<EvaluationBasis>,
    pub prerequisites: Vec<Prereq>,
    /// Commands whose committed events in a project show the stage was used.
    pub completion_evidence: Vec<CommandId>,
}

/// A loaded, schema-valid `mig-ladder@1` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationLadder {
    version: SemVer,
    stages: Vec<Stage>,
    digest: Digest,
}

impl MigrationLadder {
    /// The schema tag, always [`SCHEMA`].
    pub fn schema(&self) -> &'static str {
        SCHEMA
    }

    /// The ladder version (REQ-MIG.LADDER.DEF-005).
    pub fn version(&self) -> SemVer {
        self.version
    }

    /// SHA-256 of the file's RFC 8785 canonical JSON (REQ-MIG.LADDER.DEF-005).
    pub fn digest(&self) -> &Digest {
        &self.digest
    }

    /// The five stages, in ladder order.
    pub fn stages(&self) -> &[Stage] {
        &self.stages
    }

    /// The stage with this ID.
    pub fn stage(&self, id: &str) -> Option<&Stage> {
        self.stages.iter().find(|s| s.id == id)
    }
}

/// A ladder-check finding. Displayed as one line starting with its code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// `LADDER-SCHEMA <detail>`: the ladder or manifest file is not well formed.
    Schema(String),
    /// `LADDER-FORWARD <stage> <later stage> <prerequisite>` (REQ-MIG.LADDER.DEF-002).
    Forward { stage: String, later: String, prerequisite: Prereq },
    /// `LADDER-MISSING-CMD <command id> <stage>` (REQ-MIG.LADDER.DEF-003).
    MissingCmd { command_id: String, stage: String },
    /// `LADDER-BASIS <stage> <basis>` (REQ-MIG.LADDER.DEF-004).
    Basis { stage: String, basis: EvaluationBasis },
}

impl Finding {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            Finding::Schema(_) => "LADDER-SCHEMA",
            Finding::Forward { .. } => "LADDER-FORWARD",
            Finding::MissingCmd { .. } => "LADDER-MISSING-CMD",
            Finding::Basis { .. } => "LADDER-BASIS",
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            Finding::Schema(d) => write!(f, "{code} {d}"),
            Finding::Forward { stage, later, prerequisite } => write!(f, "{code} {stage} {later} {prerequisite}"),
            Finding::MissingCmd { command_id, stage } => write!(f, "{code} {command_id} {stage}"),
            Finding::Basis { stage, basis } => write!(f, "{code} {stage} {}", basis.as_str()),
        }
    }
}

/// The command set of a registry manifest (`CORE.REG.MAN`): `(id, major)` pairs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestCommands {
    commands: BTreeSet<(String, u32)>,
}

impl ManifestCommands {
    /// Whether the manifest lists this command (same name and major).
    pub fn contains(&self, id: &CommandId) -> bool {
        self.commands.contains(&(id.name().to_string(), id.major()))
    }

    /// Number of listed commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether the manifest lists no command.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// The result of [`check`]: the ladder when it loaded, and every finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    pub ladder: Option<MigrationLadder>,
    pub findings: Vec<Finding>,
}

impl CheckReport {
    /// True when the ladder loaded and there are no findings.
    pub fn is_ok(&self) -> bool {
        self.ladder.is_some() && self.findings.is_empty()
    }
}

// ------------------------------------------------------------------ loading

fn schema(msg: impl Into<String>) -> Finding {
    Finding::Schema(msg.into())
}

fn is_ident(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_IDENT
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

fn is_spec_node(s: &str) -> bool {
    s.len() <= MAX_IDENT
        && s.split('.').count() >= 2
        && s.split('.').all(|seg| {
            let b = seg.as_bytes();
            !b.is_empty() && b[0].is_ascii_uppercase() && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        })
}

fn is_phase(s: &str) -> bool {
    let b = s.as_bytes();
    let digits = b.iter().skip(1).take_while(|c| c.is_ascii_digit()).count();
    let rest = b.len().saturating_sub(1 + digits);
    b.first() == Some(&b'P') && (1..=2).contains(&digits) && rest <= 1 && b[1 + digits..].iter().all(u8::is_ascii_lowercase)
}

fn exact_keys(obj: &BTreeMap<String, CValue>, keys: &[&str], at: &str, out: &mut Vec<Finding>) -> bool {
    let mut ok = true;
    for k in keys {
        if !obj.contains_key(*k) {
            out.push(schema(format!("{at}: missing field {k}")));
            ok = false;
        }
    }
    for k in obj.keys() {
        if !keys.contains(&k.as_str()) {
            out.push(schema(format!("{at}: unknown field {k}")));
            ok = false;
        }
    }
    ok
}

fn list<'a>(v: Option<&'a CValue>, at: &str, nonempty: bool, out: &mut Vec<Finding>) -> Option<&'a [CValue]> {
    match v {
        Some(CValue::Arr(a)) if a.len() > MAX_ITEMS => {
            out.push(schema(format!("{at}: more than {MAX_ITEMS} entries")));
            None
        }
        Some(CValue::Arr(a)) if nonempty && a.is_empty() => {
            out.push(schema(format!("{at}: must not be empty")));
            None
        }
        Some(CValue::Arr(a)) => Some(a),
        _ => {
            out.push(schema(format!("{at}: must be an array")));
            None
        }
    }
}

fn kinds(v: Option<&CValue>, at: &str, out: &mut Vec<Finding>) -> Vec<String> {
    let mut got = Vec::new();
    let mut seen = BTreeSet::new();
    for (i, item) in list(v, at, true, out).unwrap_or_default().iter().enumerate() {
        match item {
            CValue::Str(s) if is_ident(s) => {
                if seen.insert(s.as_str()) {
                    got.push(s.clone());
                } else {
                    out.push(schema(format!("{at}[{i}]: duplicate {s}")));
                }
            }
            _ => out.push(schema(format!("{at}[{i}]: must be a lowercase snake_case identifier"))),
        }
    }
    got
}

fn command_id(v: Option<&CValue>, at: &str, out: &mut Vec<Finding>) -> Option<CommandId> {
    match v {
        Some(CValue::Str(s)) => match CommandId::parse(s) {
            Ok(id) => Some(id),
            Err(_) => {
                out.push(schema(format!("{at}: not a command ID (name@major)")));
                None
            }
        },
        _ => {
            out.push(schema(format!("{at}: must be a string")));
            None
        }
    }
}

fn stage_commands(v: Option<&CValue>, at: &str, out: &mut Vec<Finding>) -> Vec<StageCommand> {
    let mut got: Vec<StageCommand> = Vec::new();
    for (i, item) in list(v, at, true, out).unwrap_or_default().iter().enumerate() {
        let here = format!("{at}[{i}]");
        let CValue::Obj(o) = item else {
            out.push(schema(format!("{here}: must be an object")));
            continue;
        };
        if !exact_keys(o, &COMMAND_KEYS, &here, out) {
            continue;
        }
        let id = command_id(o.get("command_id"), &format!("{here}.command_id"), out);
        let spec_node = match o.get("spec_node") {
            Some(CValue::Str(s)) if is_spec_node(s) => Some(s.clone()),
            _ => {
                out.push(schema(format!("{here}.spec_node: must be a spec node ID such as MIG.TEMPL.MAP")));
                None
            }
        };
        let availability = match o.get("availability") {
            Some(CValue::Str(s)) if s == "built" => Some(Availability::Built),
            Some(CValue::Str(s)) if s == "planned" => Some(Availability::Planned),
            _ => {
                out.push(schema(format!("{here}.availability: must be built or planned")));
                None
            }
        };
        if let (Some(command_id), Some(spec_node), Some(availability)) = (id, spec_node, availability) {
            if got.iter().any(|c| c.command_id == command_id) {
                out.push(schema(format!("{here}: duplicate command {}", command_id.as_str())));
            } else {
                got.push(StageCommand { command_id, spec_node, availability });
            }
        }
    }
    got
}

fn bases(v: Option<&CValue>, at: &str, out: &mut Vec<Finding>) -> Vec<EvaluationBasis> {
    let mut got = Vec::new();
    for (i, item) in list(v, at, true, out).unwrap_or_default().iter().enumerate() {
        match item {
            CValue::Str(s) => match EvaluationBasis::parse(s) {
                Some(b) if got.contains(&b) => out.push(schema(format!("{at}[{i}]: duplicate {s}"))),
                Some(b) => got.push(b),
                None => out.push(schema(format!(
                    "{at}[{i}]: must be one of POINTS, FEATURES, RESULTS_ONLY, DIRECT_READING"
                ))),
            },
            _ => out.push(schema(format!("{at}[{i}]: must be a string"))),
        }
    }
    got
}

fn prerequisites(v: Option<&CValue>, at: &str, out: &mut Vec<Finding>) -> Vec<Prereq> {
    let mut got = Vec::new();
    for (i, item) in list(v, at, false, out).unwrap_or_default().iter().enumerate() {
        let here = format!("{at}[{i}]");
        let p = match item {
            CValue::Obj(o) if o.len() == 1 => match o.iter().next() {
                Some((k, CValue::Str(s))) if k == "input" && is_ident(s) => Some(Prereq::Input(s.clone())),
                Some((k, v)) if k == "command" => command_id(Some(v), &format!("{here}.command"), out).map(Prereq::Command),
                _ => {
                    out.push(schema(format!("{here}: must be {{\"input\": <kind>}} or {{\"command\": <id>}}")));
                    None
                }
            },
            _ => {
                out.push(schema(format!("{here}: must be {{\"input\": <kind>}} or {{\"command\": <id>}}")));
                None
            }
        };
        if let Some(p) = p {
            if got.contains(&p) {
                out.push(schema(format!("{here}: duplicate {p}")));
            } else {
                got.push(p);
            }
        }
    }
    got
}

fn stage(v: &CValue, index: usize, out: &mut Vec<Finding>) -> Option<Stage> {
    let at = format!("stages[{index}]");
    let CValue::Obj(o) = v else {
        out.push(schema(format!("{at}: must be an object")));
        return None;
    };
    if !exact_keys(o, &STAGE_KEYS, &at, out) {
        return None;
    }
    let before = out.len();
    let expected_order = index + 1;
    let order = match o.get("order") {
        Some(CValue::Num(n)) if (1.0..=5.0).contains(n) && n.fract() == 0.0 => Some(*n as u8),
        _ => None,
    };
    if order.map(usize::from) != Some(expected_order) {
        out.push(schema(format!("{at}.order: must be {expected_order} (stages are listed in order 1 to 5)")));
    }
    let expected_id = STAGE_IDS.get(index).copied().unwrap_or_default();
    let id = match o.get("id") {
        Some(CValue::Str(s)) if s == expected_id => s.clone(),
        _ => {
            out.push(schema(format!("{at}.id: must be {expected_id}")));
            String::new()
        }
    };
    let phase = match o.get("phase") {
        Some(CValue::Str(s)) if is_phase(s) => s.clone(),
        _ => {
            out.push(schema(format!("{at}.phase: must be a phase tag such as P1a")));
            String::new()
        }
    };
    let inputs = kinds(o.get("inputs"), &format!("{at}.inputs"), out);
    let outputs = kinds(o.get("outputs"), &format!("{at}.outputs"), out);
    let commands = stage_commands(o.get("commands"), &format!("{at}.commands"), out);
    let basis = bases(o.get("basis"), &format!("{at}.basis"), out);
    let prerequisites = prerequisites(o.get("prerequisites"), &format!("{at}.prerequisites"), out);
    let mut completion_evidence: Vec<CommandId> = Vec::new();
    let ev_at = format!("{at}.completion_evidence");
    for (i, item) in list(o.get("completion_evidence"), &ev_at, true, out).unwrap_or_default().iter().enumerate() {
        let Some(id) = command_id(Some(item), &format!("{ev_at}[{i}]"), out) else { continue };
        if completion_evidence.contains(&id) {
            out.push(schema(format!("{ev_at}[{i}]: duplicate {}", id.as_str())));
        } else if !commands.iter().any(|c| c.command_id == id) {
            out.push(schema(format!("{ev_at}[{i}]: {} is not one of this stage's commands", id.as_str())));
        } else {
            completion_evidence.push(id);
        }
    }
    if out.len() != before {
        return None;
    }
    Some(Stage {
        id,
        order: order.unwrap_or_default(),
        phase,
        inputs,
        outputs,
        commands,
        basis,
        prerequisites,
        completion_evidence,
    })
}

/// Parses and schema-checks a `mig-ladder@1` file (REQ-MIG.LADDER.DEF-001). On failure every
/// `LADDER-SCHEMA` finding is returned.
pub fn load(bytes: &[u8]) -> Result<MigrationLadder, Vec<Finding>> {
    let value = fid_canon::parse_with_limits(bytes, MAX_LADDER_BYTES, MAX_LADDER_DEPTH)
        .map_err(|e| vec![schema(format!("ladder: not valid JSON within limits ({e})"))])?;
    let canonical =
        fid_canon::to_canonical_bytes(&value).map_err(|e| vec![schema(format!("ladder: no canonical form ({e})"))])?;
    let digest = Digest::from_bytes(fid_canon::digest_bytes(&canonical).0);
    let mut out = Vec::new();
    let CValue::Obj(top) = &value else {
        return Err(vec![schema("ladder: top level must be an object")]);
    };
    if !exact_keys(top, &TOP_KEYS, "ladder", &mut out) {
        return Err(out);
    }
    if top.get("schema") != Some(&CValue::Str(SCHEMA.to_string())) {
        out.push(schema(format!("schema: must be {SCHEMA}")));
    }
    let version = match top.get("version") {
        Some(CValue::Str(s)) => SemVer::parse(s),
        _ => None,
    };
    if version.is_none() {
        out.push(schema("version: must be MAJOR.MINOR.PATCH"));
    }
    let mut stages = Vec::new();
    match top.get("stages") {
        Some(CValue::Arr(a)) if a.len() == STAGE_IDS.len() => {
            for (i, v) in a.iter().enumerate() {
                if let Some(s) = stage(v, i, &mut out) {
                    stages.push(s);
                }
            }
        }
        _ => out.push(schema("stages: must be an array of exactly 5 stages")),
    }
    match version {
        Some(version) if out.is_empty() => Ok(MigrationLadder { version, stages, digest }),
        _ => Err(out),
    }
}

/// Reads the command set of a registry manifest: an object whose `commands` array holds
/// objects with `id` (command name) and `major` (an integer, or its decimal string form).
/// Other fields are ignored.
pub fn parse_manifest(bytes: &[u8]) -> Result<ManifestCommands, Finding> {
    let value = fid_canon::parse_with_limits(bytes, MAX_MANIFEST_BYTES, fid_canon::MAX_DEPTH)
        .map_err(|e| schema(format!("manifest: not valid JSON within limits ({e})")))?;
    let Some(CValue::Arr(commands)) = (match &value {
        CValue::Obj(o) => o.get("commands"),
        _ => None,
    }) else {
        return Err(schema("manifest: must be an object with a commands array"));
    };
    let mut got = BTreeSet::new();
    for (i, c) in commands.iter().enumerate() {
        let CValue::Obj(o) = c else {
            return Err(schema(format!("manifest.commands[{i}]: must be an object")));
        };
        let major = match o.get("major") {
            Some(CValue::Num(n)) if (0.0..=f64::from(u32::MAX)).contains(n) && n.fract() == 0.0 => Some(*n as u32),
            Some(CValue::Str(s)) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => s.parse().ok(),
            _ => None,
        };
        match (o.get("id"), major) {
            (Some(CValue::Str(id)), Some(major)) => {
                got.insert((id.clone(), major));
            }
            _ => return Err(schema(format!("manifest.commands[{i}]: needs a string id and an integer major"))),
        }
    }
    Ok(ManifestCommands { commands: got })
}

// ------------------------------------------------------------------ checks

/// REQ-MIG.LADDER.DEF-002: a prerequisite fails when every stage that lists it (as input,
/// output or command) comes later than the stage that requires it. A prerequisite that no
/// stage lists is outside the ladder and not checked here.
pub fn check_independence(ladder: &MigrationLadder) -> Vec<Finding> {
    // Owning stage orders per prerequisite (input and output kinds share one namespace).
    let mut owners: BTreeMap<Prereq, BTreeSet<u8>> = BTreeMap::new();
    for s in ladder.stages() {
        for k in s.inputs.iter().chain(&s.outputs) {
            owners.entry(Prereq::Input(k.clone())).or_default().insert(s.order);
        }
        for c in &s.commands {
            owners.entry(Prereq::Command(c.command_id.clone())).or_default().insert(s.order);
        }
    }
    let mut out = Vec::new();
    for s in ladder.stages() {
        for p in &s.prerequisites {
            let Some(earliest) = owners.get(p).and_then(|o| o.first()) else { continue };
            if *earliest > s.order {
                let later = ladder.stages().iter().find(|t| t.order == *earliest).map(|t| t.id.clone());
                out.push(Finding::Forward {
                    stage: s.id.clone(),
                    later: later.unwrap_or_default(),
                    prerequisite: p.clone(),
                });
            }
        }
    }
    out
}

/// REQ-MIG.LADDER.DEF-003: every `built` command must be in the manifest; `planned` commands
/// are skipped.
pub fn check_manifest(ladder: &MigrationLadder, manifest: &ManifestCommands) -> Vec<Finding> {
    let mut out = Vec::new();
    for s in ladder.stages() {
        for c in &s.commands {
            if c.availability == Availability::Built && !manifest.contains(&c.command_id) {
                out.push(Finding::MissingCmd { command_id: c.command_id.as_str().to_string(), stage: s.id.clone() });
            }
        }
    }
    out
}

/// REQ-MIG.LADDER.DEF-004: stages 1 and 4 carry only `RESULTS_ONLY`; stage 3 only `POINTS`
/// or `FEATURES`. Other stages are not constrained.
pub fn check_basis(ladder: &MigrationLadder) -> Vec<Finding> {
    let mut out = Vec::new();
    for s in ladder.stages() {
        let allowed: &[EvaluationBasis] = match s.order {
            1 | 4 => &[EvaluationBasis::ResultsOnly],
            3 => &[EvaluationBasis::Points, EvaluationBasis::Features],
            _ => continue,
        };
        for b in &s.basis {
            if !allowed.contains(b) {
                out.push(Finding::Basis { stage: s.id.clone(), basis: *b });
            }
        }
    }
    out
}

/// The whole ladder check (`cargo xtask mig ladder-check`): schema, independence, manifest
/// and basis findings, in that order. A ladder that fails the schema is not checked further.
pub fn check(ladder: &[u8], manifest: &[u8]) -> CheckReport {
    let ladder = match load(ladder) {
        Ok(l) => l,
        Err(findings) => return CheckReport { ladder: None, findings },
    };
    let mut findings = check_independence(&ladder);
    match parse_manifest(manifest) {
        Ok(m) => findings.extend(check_manifest(&ladder, &m)),
        Err(f) => findings.push(f),
    }
    findings.extend(check_basis(&ladder));
    CheckReport { ladder: Some(ladder), findings }
}

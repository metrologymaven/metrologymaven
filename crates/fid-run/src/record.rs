//! The run record (REQ-AUTO.RUN.BIND-004): the fields of the project's Run entity, its
//! canonical JSON (RFC 8785, `fid-canon`), its strict reader and its JSON Schema.
//!
//! Wire form, every member always present:
//!
//! ```text
//! {"binding": {<param>: <value>, ...},      the binding as authored (a variant's own binding)
//!  "branch": "<branch>" | null,             the run's journal branch; null until AUTO.RUN.EXEC names it
//!  "inputs": {<param>: {"handle": "<data handle>", "sha256": "sha256:<hex>"}, ...},
//!  "lifecycle_state": "draft" | "registered" | "released" | "deprecated",
//!  "outcome": "pending" | "succeeded" | "failed" | "cancelled",
//!  "override_deprecated": <bool>,           the deprecation override, as requested
//!  "principal": {"id": "<id>", "kind": "<kind>"},
//!  "program_digest": "sha256:<hex>",
//!  "program_id": "<uuidv7>",
//!  "program_version": <n> | null,           null for a draft (no version)
//!  "run_id": "<uuidv7>",
//!  "trial": <bool>,                         as requested
//!  "variant": "<name>" | null}
//! ```
//!
//! `program_version` and `lifecycle_state` are the lifecycle the run was decided against
//! (REQ-CORE.RPL.RECON.FOLD-008 (d)); see [`crate::ledger`].
//!
//! **The `program.run@1` parameters** of a run are a function of its record ([`run_params`]):
//!
//! ```text
//! {"binding": {..}, "inputs": {<param>: "<handle>", ..}, "override_deprecated": <bool>,
//!  "program": "<uuidv7>", "program_digest": "sha256:..", "trial": <bool>, "variant": "<name>" | null}
//! ```
//!
//! `binding` is the binding the run uses (for a variant run, the variant's binding, which the
//! front end resolves and the handler re-checks), `inputs` every data handle the run reads.
//! `AUTO.RUN.EXEC.CMD` takes exactly this object as its parameters, so the bus's parameters
//! commitment of the entry binds the run to the program, binding, inputs and flags asked for.

use fid_bus::principal::{PrincipalId, PrincipalKind, PrincipalRef};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_program::{binding_from_value, binding_to_value, Binding, Uuid7};
use fid_program_rec::life::LifeState;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Members of a record, sorted.
pub const RECORD_KEYS: [&str; 13] = [
    "binding",
    "branch",
    "inputs",
    "lifecycle_state",
    "outcome",
    "override_deprecated",
    "principal",
    "program_digest",
    "program_id",
    "program_version",
    "run_id",
    "trial",
    "variant",
];

/// Most bound inputs one record may name (the family's parameter limit).
pub const MAX_INPUTS: usize = fid_program::family::MAX_PARAMS;

/// A run's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Outcome {
    /// Prepared, not finished (every record `prepare_run` produces).
    Pending,
    /// Finished without error.
    Succeeded,
    /// Stopped by an error.
    Failed,
    /// Cancelled.
    Cancelled,
}

impl Outcome {
    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Pending => "pending",
            Outcome::Succeeded => "succeeded",
            Outcome::Failed => "failed",
            Outcome::Cancelled => "cancelled",
        }
    }

    /// Reads a wire name.
    pub fn parse(s: &str) -> Option<Outcome> {
        [Outcome::Pending, Outcome::Succeeded, Outcome::Failed, Outcome::Cancelled]
            .into_iter()
            .find(|o| o.as_str() == s)
    }
}

/// The digest of one bound input dataset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDigest {
    /// The data handle as bound.
    pub handle: String,
    /// SHA-256 of the bytes read through it.
    pub sha256: Digest,
}

/// The record of one run (module docs). Fields are private: a record comes from
/// [`crate::prepare_run`] or from [`RunRecord::from_value`], and the only change after
/// preparation is naming its branch ([`RunRecord::with_branch`]).
#[derive(Debug, Clone, PartialEq)]
pub struct RunRecord {
    pub(crate) run_id: Uuid7,
    pub(crate) program_id: Uuid7,
    pub(crate) program_version: Option<u32>,
    pub(crate) program_digest: Digest,
    pub(crate) lifecycle_state: LifeState,
    pub(crate) binding: Binding,
    pub(crate) variant: Option<String>,
    pub(crate) inputs: BTreeMap<String, InputDigest>,
    pub(crate) principal: PrincipalRef,
    pub(crate) trial: bool,
    pub(crate) override_deprecated: bool,
    pub(crate) branch: Option<String>,
    pub(crate) outcome: Outcome,
}

/// Why a record document was refused (static reason).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordError(pub &'static str);

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "run record: {}", self.0)
    }
}

impl std::error::Error for RecordError {}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn opt_str(v: &Option<String>) -> CValue {
    v.as_deref().map_or(CValue::Null, s)
}

impl RunRecord {
    /// The run ID (UUIDv7).
    pub fn run_id(&self) -> Uuid7 {
        self.run_id
    }
    /// The program ID.
    pub fn program_id(&self) -> Uuid7 {
        self.program_id
    }
    /// The version number, `None` for a draft.
    pub fn program_version(&self) -> Option<u32> {
        self.program_version
    }
    /// SHA-256 of the program's canonical bytes.
    pub fn program_digest(&self) -> Digest {
        self.program_digest
    }
    /// The lifecycle state the run was decided against.
    pub fn lifecycle_state(&self) -> LifeState {
        self.lifecycle_state
    }
    /// The binding as authored (a variant's own binding for a variant run).
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    /// The variant name, if the run bound a variant.
    pub fn variant(&self) -> Option<&str> {
        self.variant.as_deref()
    }
    /// Digests of the bound input datasets, by parameter name.
    pub fn inputs(&self) -> &BTreeMap<String, InputDigest> {
        &self.inputs
    }
    /// The principal that prepared the run.
    pub fn principal(&self) -> &PrincipalRef {
        &self.principal
    }
    /// Whether this is a trial run (REQ-005); results of a trial run carry the trial mark.
    pub fn trial(&self) -> bool {
        self.trial
    }
    /// Whether a person's deprecation override was applied (REQ-002).
    pub fn override_deprecated(&self) -> bool {
        self.override_deprecated
    }
    /// The run's journal branch, once named.
    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }
    /// The outcome.
    pub fn outcome(&self) -> Outcome {
        self.outcome
    }

    /// The same record on journal branch `branch` (named by `AUTO.RUN.EXEC`).
    pub fn with_branch(mut self, branch: &str) -> RunRecord {
        self.branch = Some(branch.to_owned());
        self
    }

    /// The JSON value (module docs).
    pub fn to_value(&self) -> CValue {
        let inputs = self
            .inputs
            .iter()
            .map(|(k, d)| {
                let mut o = BTreeMap::new();
                o.insert("handle".to_owned(), s(&d.handle));
                o.insert("sha256".to_owned(), s(d.sha256.as_str()));
                (k.clone(), CValue::Obj(o))
            })
            .collect();
        let pairs: [(&str, CValue); 13] = [
            ("binding", binding_to_value(&self.binding)),
            ("branch", opt_str(&self.branch)),
            ("inputs", CValue::Obj(inputs)),
            ("lifecycle_state", s(self.lifecycle_state.as_str())),
            ("outcome", s(self.outcome.as_str())),
            ("override_deprecated", CValue::Bool(self.override_deprecated)),
            ("principal", self.principal.to_value()),
            ("program_digest", s(self.program_digest.as_str())),
            ("program_id", s(&self.program_id.to_string())),
            ("program_version", self.program_version.map_or(CValue::Null, |v| CValue::Num(f64::from(v)))),
            ("run_id", s(&self.run_id.to_string())),
            ("trial", CValue::Bool(self.trial)),
            ("variant", opt_str(&self.variant)),
        ];
        CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }

    /// The canonical bytes (RFC 8785).
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, RecordError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| RecordError("no canonical form"))
    }

    /// Reads canonical record bytes (strict, as [`RunRecord::from_value`]).
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<RunRecord, RecordError> {
        let v = parse(bytes).map_err(|_| RecordError("not JSON"))?;
        let r = RunRecord::from_value(&v)?;
        if r.to_canonical_bytes()? != bytes {
            return Err(RecordError("not canonical"));
        }
        Ok(r)
    }

    /// Reads a record value strictly: exactly the members of [`RECORD_KEYS`], each of its
    /// type; a draft has `program_version: null` and nothing else does.
    pub fn from_value(v: &CValue) -> Result<RunRecord, RecordError> {
        let CValue::Obj(o) = v else { return Err(RecordError("not an object")) };
        if o.len() != RECORD_KEYS.len() || !RECORD_KEYS.iter().all(|k| o.contains_key(*k)) {
            return Err(RecordError("members are not exactly the record's"));
        }
        let get = |k: &str| o.get(k).ok_or(RecordError("missing member"));
        let text = |k: &'static str| match get(k)? {
            CValue::Str(t) => Ok(t.as_str()),
            _ => Err(RecordError(k)),
        };
        let flag = |k: &'static str| match get(k)? {
            CValue::Bool(b) => Ok(*b),
            _ => Err(RecordError(k)),
        };
        let opt_text = |k: &'static str| match get(k)? {
            CValue::Null => Ok(None),
            CValue::Str(t) => Ok(Some(t.clone())),
            _ => Err(RecordError(k)),
        };
        let uuid = |k: &'static str| text(k).and_then(|t| Uuid7::parse(t).ok_or(RecordError(k)));
        let lifecycle_state = LifeState::parse(text("lifecycle_state")?).ok_or(RecordError("lifecycle_state"))?;
        let program_version = match get("program_version")? {
            CValue::Null => None,
            CValue::Num(n) if *n >= 1.0 && *n <= f64::from(u32::MAX) && n.fract() == 0.0 => {
                Some(u32::try_from(*n as u64).map_err(|_| RecordError("program_version"))?)
            }
            _ => return Err(RecordError("program_version")),
        };
        if program_version.is_none() != (lifecycle_state == LifeState::Draft) {
            return Err(RecordError("program_version is null exactly for a draft"));
        }
        let binding = binding_from_value(get("binding")?.clone()).map_err(|_| RecordError("binding"))?;
        let CValue::Obj(ins) = get("inputs")? else { return Err(RecordError("inputs")) };
        if ins.len() > MAX_INPUTS {
            return Err(RecordError("inputs"));
        }
        let mut inputs = BTreeMap::new();
        for (k, d) in ins {
            let CValue::Obj(d) = d else { return Err(RecordError("inputs")) };
            match (d.len(), d.get("handle"), d.get("sha256")) {
                (2, Some(CValue::Str(h)), Some(CValue::Str(sum))) => {
                    let sha256 = Digest::parse(sum).map_err(|_| RecordError("inputs"))?;
                    inputs.insert(k.clone(), InputDigest { handle: h.clone(), sha256 });
                }
                _ => return Err(RecordError("inputs")),
            }
        }
        let CValue::Obj(p) = get("principal")? else { return Err(RecordError("principal")) };
        let principal = match (p.len(), p.get("id"), p.get("kind")) {
            (2, Some(CValue::Str(id)), Some(CValue::Str(kind))) => PrincipalRef {
                kind: PrincipalKind::parse(kind).map_err(|_| RecordError("principal"))?,
                id: PrincipalId::new(id).map_err(|_| RecordError("principal"))?,
            },
            _ => return Err(RecordError("principal")),
        };
        Ok(RunRecord {
            run_id: uuid("run_id")?,
            program_id: uuid("program_id")?,
            program_version,
            program_digest: Digest::parse(text("program_digest")?).map_err(|_| RecordError("program_digest"))?,
            lifecycle_state,
            binding,
            variant: opt_text("variant")?,
            inputs,
            principal,
            trial: flag("trial")?,
            override_deprecated: flag("override_deprecated")?,
            branch: opt_text("branch")?,
            outcome: Outcome::parse(text("outcome")?).ok_or(RecordError("outcome"))?,
        })
    }
}

/// The `program.run@1` parameters of `record` (module docs).
pub fn run_params(record: &RunRecord) -> CValue {
    let inputs = record.inputs.iter().map(|(k, d)| (k.clone(), s(&d.handle))).collect();
    let pairs: [(&str, CValue); 7] = [
        ("binding", binding_to_value(&record.binding)),
        ("inputs", CValue::Obj(inputs)),
        ("override_deprecated", CValue::Bool(record.override_deprecated)),
        ("program", s(&record.program_id.to_string())),
        ("program_digest", s(record.program_digest.as_str())),
        ("trial", CValue::Bool(record.trial)),
        ("variant", opt_str(&record.variant)),
    ];
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// The journal branch of run `run_id`: exactly `run/<run_id>`.
pub fn run_branch(run_id: Uuid7) -> String {
    format!("run/{run_id}")
}

/// The JSON Schema (2020-12) of a record: the Run entity's fields (module docs).
pub const RECORD_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Run record (AUTO.RUN.BIND)",
  "type": "object",
  "additionalProperties": false,
  "required": ["binding", "branch", "inputs", "lifecycle_state", "outcome", "override_deprecated", "principal",
               "program_digest", "program_id", "program_version", "run_id", "trial", "variant"],
  "properties": {
    "binding": {"type": "object"},
    "branch": {"type": ["string", "null"], "minLength": 1},
    "inputs": {
      "type": "object",
      "additionalProperties": {
        "type": "object",
        "additionalProperties": false,
        "required": ["handle", "sha256"],
        "properties": {
          "handle": {"type": "string", "minLength": 1, "maxLength": 4096},
          "sha256": {"type": "string", "pattern": "^sha256:[0-9a-f]{64}$"}
        }
      }
    },
    "lifecycle_state": {"enum": ["draft", "registered", "released", "deprecated"]},
    "outcome": {"enum": ["pending", "succeeded", "failed", "cancelled"]},
    "override_deprecated": {"type": "boolean"},
    "principal": {
      "type": "object",
      "additionalProperties": false,
      "required": ["id", "kind"],
      "properties": {
        "id": {"type": "string", "minLength": 1},
        "kind": {"enum": ["human", "agent", "script", "service", "device", "system"]}
      }
    },
    "program_digest": {"type": "string", "pattern": "^sha256:[0-9a-f]{64}$"},
    "program_id": {"type": "string", "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"},
    "program_version": {"type": ["integer", "null"], "minimum": 1, "maximum": 4294967295},
    "run_id": {"type": "string", "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"},
    "trial": {"type": "boolean"},
    "variant": {"type": ["string", "null"], "minLength": 1, "maxLength": 64}
  }
}"#;

/// [`RECORD_SCHEMA`] as a value.
pub fn record_schema() -> Result<CValue, RecordError> {
    parse(RECORD_SCHEMA.as_bytes()).map_err(|_| RecordError("schema is not JSON"))
}

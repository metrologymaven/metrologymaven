//! `prepare_run`: everything that must hold before a run starts (REQ-AUTO.RUN.BIND-001..005).
//!
//! Check order, the first failure returned before anything later runs:
//!
//! 1. **Lifecycle query.** The program's state comes from the project's lifecycle store, that
//!    of `main` ([`LifeStore::run_gate`], `AUTO.REC.LIFE`), never from the caller: the same
//!    store, as of the commit, that the commit stage and the run ledger judge the run against
//!    ([`crate::ledger`]).
//!    An identified program ([`ProgramRef::Identified`]) is read from that store's frozen
//!    bytes; a draft ([`ProgramRef::Draft`]) is the caller's in-memory program, and if its
//!    digest identifies a registered, released or deprecated version, that version's state
//!    applies (so a deprecated program cannot be passed off as a draft).
//! 2. **Principal rules**, on the bus-derived [`Principal`] (no parameter names or changes
//!    its kind): released → any principal; draft or registered → only a `human` with
//!    [`RunFlags::trial`] (`not_released` otherwise); deprecated → only a `human` with
//!    [`RunFlags::override_deprecated`] (`deprecated` otherwise). The override flag on a
//!    program that is not deprecated, or from anyone but a person, is refused
//!    (`override_refused`, or `deprecated` on a deprecated program), so no record carries an
//!    override that was not a person's decision on a deprecated program. The trial flag only
//!    lowers a run's standing, so any principal may set it on a released program. Both flags
//!    are recorded as requested (they are part of the `program.run@1` parameters the ledger
//!    checks, [`crate::record::run_params`]).
//! 3. **Binding check**: the program's family (`AUTO.PROG.FAMILY`), the variant's binding or
//!    the caller's binding, then `check_binding`; any error is `binding_invalid` with the full
//!    list.
//! 4. **Inputs**: every `data_handle` parameter, bound or by default, is opened through the
//!    host's [`InputSource`] (the bus file policy and root handles) and streamed through
//!    SHA-256 (`input_unreadable` on failure). The default of every `data_handle` parameter
//!    that declares one (the recorded input) is hashed the same way into
//!    [`RunPlan::default_digests`] for the re-target guard (`AUTO.RUN.EXEC.RETGT`); a default
//!    that cannot be read is left out, never an error, so binding that parameter re-targets
//!    the run (fail closed).
//! 5. **Record**: a new run ID from [`RunIds`], outcome `pending`, branch `null`.
//!
//! D0 apart from the run ID: the same inputs give the same record but for `run_id`.

use crate::record::{InputDigest, Outcome, RunRecord};
use fid_bus::principal::{Principal, PrincipalKind};
use fid_program::{Binding, BindingError, BoundValue, Family, ParamType, Program, Uuid7};
use fid_program_rec::life::{ArtifactId, LifeState, LifeStore, RunGate};
use fid_registry::QuantityCatalogue;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;

/// Bytes read per chunk while hashing an input.
pub const HASH_CHUNK: usize = 64 * 1024;

/// The program a run is for.
#[derive(Debug, Clone, Copy)]
pub enum ProgramRef<'a> {
    /// A registered, released or deprecated version, by program ID and digest; its bytes are
    /// the lifecycle store's frozen bytes.
    Identified {
        /// The program ID.
        program_id: Uuid7,
        /// SHA-256 of the version's canonical bytes.
        digest: Digest,
    },
    /// The project's current draft (trial runs only).
    Draft(&'a Program),
}

/// What the run binds.
#[derive(Debug, Clone, PartialEq)]
pub enum BindingSource {
    /// A binding as authored.
    Binding(Binding),
    /// A named variant of the program's family.
    Variant(String),
}

/// The flags of a run request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunFlags {
    /// A trial run (REQ-001, REQ-005).
    pub trial: bool,
    /// A person's override of the deprecation refusal (REQ-002).
    pub override_deprecated: bool,
}

/// Opens bound input datasets. The host wires it to the bus file policy and the approved
/// root handles (`CORE.BUS.POL.FS`, `IO.REG.ROOTS`), so an agent or a script never reads a
/// raw path. `Err` holds a display-safe reason.
pub trait InputSource {
    /// Opens the dataset `handle` bound to parameter `param`, for `principal`.
    fn open(&self, principal: &Principal, param: &str, handle: &str) -> Result<Box<dyn Read + '_>, String>;
}

/// Mints run IDs (UUIDv7; the host wires the project's ID service, `CORE.PRJ.CONT.ID`).
pub trait RunIds {
    /// A new run ID; `Err` holds a display-safe reason.
    fn next_run_id(&mut self) -> Result<Uuid7, String>;
}

/// What `prepare_run` reads.
pub struct BindCtx<'a> {
    /// The project's lifecycle state (that of `main`), the store the ledger also judges by.
    pub lifecycle: &'a LifeStore,
    /// The registry's quantity catalogue (for the binding check).
    pub catalogue: &'a dyn QuantityCatalogue,
    /// The input datasets.
    pub inputs: &'a dyn InputSource,
    /// The run-ID source.
    pub run_ids: &'a mut dyn RunIds,
}

/// A run that may start: its record and the re-targetable flag of every step.
#[derive(Debug, Clone, PartialEq)]
pub struct RunPlan {
    /// The run record (outcome `pending`, branch `null`).
    pub record: RunRecord,
    /// Every step ID in document order with whether it may run on re-targeted inputs.
    pub steps_retargetable: Vec<(Uuid7, bool)>,
    /// SHA-256 of the recorded default dataset of every `data_handle` parameter whose default
    /// could be read, by parameter name (`AUTO.RUN.EXEC.RETGT`). A program stores a default's
    /// handle only, so this is the dataset that handle names when the run is prepared.
    pub default_digests: BTreeMap<String, Digest>,
}

/// Errors of `prepare_run`. [`RunError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    /// `not_released{state}`: a draft or registered program, and not a person's trial run.
    NotReleased {
        /// The program's state.
        state: LifeState,
    },
    /// `deprecated{version}`: a deprecated program without a person's override.
    Deprecated {
        /// The deprecated version.
        version: u32,
    },
    /// `binding_invalid{errors}`: every error of the family and binding checks.
    BindingInvalid {
        /// The errors, in the checker's order.
        errors: Vec<BindingError>,
    },
    /// `input_unreadable{param, reason}`.
    InputUnreadable {
        /// The parameter.
        param: String,
        /// Display-safe reason.
        reason: String,
    },
    /// `override_refused{state}`: the override flag on a program that is not deprecated.
    OverrideRefused {
        /// The program's state.
        state: LifeState,
    },
    /// `program_not_found`: no version of the program has this digest.
    ProgramNotFound,
    /// `internal{reason}`.
    Internal {
        /// Reason.
        reason: String,
    },
}

impl RunError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            RunError::NotReleased { .. } => "not_released",
            RunError::Deprecated { .. } => "deprecated",
            RunError::BindingInvalid { .. } => "binding_invalid",
            RunError::InputUnreadable { .. } => "input_unreadable",
            RunError::OverrideRefused { .. } => "override_refused",
            RunError::ProgramNotFound => "program_not_found",
            RunError::Internal { .. } => "internal",
        }
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunError::NotReleased { state } => write!(f, "not_released: the program is {}", state.as_str()),
            RunError::Deprecated { version } => write!(f, "deprecated: version {version} is deprecated"),
            RunError::BindingInvalid { errors } => {
                let codes: Vec<String> =
                    errors.iter().map(|e| format!("{}({})", e.code(), e.param().unwrap_or(""))).collect();
                write!(f, "binding_invalid: {}", codes.join(", "))
            }
            RunError::InputUnreadable { param, reason } => write!(f, "input_unreadable: {param}: {reason}"),
            RunError::OverrideRefused { state } => {
                write!(f, "override_refused: the program is {}, not deprecated", state.as_str())
            }
            RunError::ProgramNotFound => f.write_str("program_not_found"),
            RunError::Internal { reason } => write!(f, "internal: {reason}"),
        }
    }
}

impl std::error::Error for RunError {}

fn internal(reason: &str) -> RunError {
    RunError::Internal { reason: reason.to_owned() }
}

/// The lifecycle a run is decided against: (version, state). A digest that identifies no
/// registered, released or deprecated version is a draft (no version).
pub fn decided_lifecycle(store: &LifeStore, program_id: Uuid7, digest: &Digest) -> (Option<u32>, LifeState) {
    match store.run_gate(&ArtifactId::Program(program_id), digest) {
        RunGate::Runnable { version, state } => (Some(version), state),
        RunGate::Deprecated { version } => (Some(version), LifeState::Deprecated),
        RunGate::NotIdentified => (None, LifeState::Draft),
    }
}

/// REQ-001/002 on a principal kind and the flags as requested (and recorded).
pub(crate) fn principal_rule(
    state: LifeState,
    version: Option<u32>,
    kind: PrincipalKind,
    flags: RunFlags,
) -> Result<(), RunError> {
    let person = kind == PrincipalKind::Human;
    match state {
        LifeState::Deprecated if person && flags.override_deprecated => Ok(()),
        LifeState::Deprecated => Err(RunError::Deprecated { version: version.unwrap_or(0) }),
        // The override exists only for a person, on a deprecated program: anywhere else it is
        // refused, so no record ever carries an override that a later preparation could apply.
        _ if flags.override_deprecated => Err(RunError::OverrideRefused { state }),
        LifeState::Released => Ok(()),
        LifeState::Draft | LifeState::Registered if person && flags.trial => Ok(()),
        LifeState::Draft | LifeState::Registered => Err(RunError::NotReleased { state }),
    }
}

fn sha256_stream(mut r: impl Read) -> Result<Digest, String> {
    let mut h = Sha256::new();
    let mut buf = vec![0u8; HASH_CHUNK];
    loop {
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => h.update(buf.get(..n).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("read failed: {}", e.kind())),
        }
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(h.finalize().as_slice());
    Ok(Digest::from_bytes(out))
}

/// Prepares a run (module docs).
pub fn prepare_run(
    ctx: &mut BindCtx<'_>,
    program: ProgramRef<'_>,
    binding: BindingSource,
    principal: &Principal,
    flags: RunFlags,
) -> Result<RunPlan, RunError> {
    // 1. Lifecycle.
    let (program, digest) = match program {
        ProgramRef::Identified { program_id, digest } => {
            let (version, _) = decided_lifecycle(ctx.lifecycle, program_id, &digest);
            let version = version.ok_or(RunError::ProgramNotFound)?;
            let bytes = ctx
                .lifecycle
                .frozen_bytes(&ArtifactId::Program(program_id), version)
                .ok_or_else(|| internal("frozen bytes missing"))?;
            let p = Program::parse(bytes).map_err(|e| internal(&format!("frozen program: {}", e.code())))?;
            (p, digest)
        }
        ProgramRef::Draft(p) => {
            let d = p.digest().map_err(|e| internal(&format!("draft program: {}", e.code())))?;
            (p.clone(), d)
        }
    };
    if program.digest().ok() != Some(digest) {
        return Err(internal("program bytes do not have the requested digest"));
    }
    let (version, state) = decided_lifecycle(ctx.lifecycle, program.program_id, &digest);

    // 2. Principal rules.
    principal_rule(state, version, principal.kind(), flags)?;

    // 3. Binding.
    let family = Family::from_program(&program, ctx.catalogue).map_err(|errors| RunError::BindingInvalid { errors })?;
    let (bound, variant) = match binding {
        BindingSource::Binding(b) => (b, None),
        BindingSource::Variant(name) => match family.variant(&name) {
            Ok(b) => (b.clone(), Some(name)),
            Err(e) => return Err(RunError::BindingInvalid { errors: vec![e] }),
        },
    };
    let errors = family.check(&bound, ctx.catalogue);
    if !errors.is_empty() {
        return Err(RunError::BindingInvalid { errors });
    }

    // 4. Inputs: every data handle, bound or by default.
    let mut inputs = BTreeMap::new();
    let mut default_digests = BTreeMap::new();
    for d in family.params().iter().filter(|d| d.ty == ParamType::DataHandle) {
        let default = match d.default.as_ref() {
            Some(BoundValue::Text(h)) => Some(h.as_str()),
            Some(_) => return Err(internal("data handle default is not text after the binding check")),
            None => None,
        };
        let handle = match bound.get(&d.name) {
            Some(BoundValue::Text(h)) => Some(h.clone()),
            Some(_) => return Err(internal("data handle is not text after the binding check")),
            None => default.map(str::to_owned),
        };
        if let Some(handle) = handle {
            let reader = ctx
                .inputs
                .open(principal, &d.name, &handle)
                .map_err(|reason| RunError::InputUnreadable { param: d.name.clone(), reason })?;
            let sha256 =
                sha256_stream(reader).map_err(|reason| RunError::InputUnreadable { param: d.name.clone(), reason })?;
            if default == Some(handle.as_str()) {
                default_digests.insert(d.name.clone(), sha256);
            }
            inputs.insert(d.name.clone(), InputDigest { handle, sha256 });
        }
        // The recorded default, when the run binds another dataset to the parameter.
        if let Some(def) = default {
            if !default_digests.contains_key(&d.name) {
                if let Ok(sha256) = ctx.inputs.open(principal, &d.name, def).and_then(sha256_stream) {
                    default_digests.insert(d.name.clone(), sha256);
                }
            }
        }
    }

    // 5. Record.
    let run_id = ctx.run_ids.next_run_id().map_err(|r| internal(&format!("run id: {r}")))?;
    let record = RunRecord {
        run_id,
        program_id: program.program_id,
        program_version: version,
        program_digest: digest,
        lifecycle_state: state,
        binding: bound,
        variant,
        inputs,
        principal: principal.reference(),
        trial: flags.trial,
        override_deprecated: flags.override_deprecated,
        branch: None,
        outcome: Outcome::Pending,
    };
    // AUTO.PROG.FMT has no per-step `retargetable` field yet (AUTO.REC.LIFT open question),
    // so no stored program can mark a step `false`: every step is re-targetable.
    let steps_retargetable = program.step_ids().into_iter().map(|id| (id, true)).collect();
    Ok(RunPlan { record, steps_retargetable, default_digests })
}

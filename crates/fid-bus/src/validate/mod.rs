//! Resolve, validate and precondition stages of `Bus::execute`. Spec node
//! `CORE.BUS.PIPE.VALID`.
//!
//! The pipeline (`CORE.BUS.PIPE.STAGE`) holds one [`CommandTable`] per Registry Manifest
//! digest (a [`TableCache`]), built once from the manifest's command definitions: every
//! parameter schema is compiled and every precondition parsed and type-checked then. A call
//! then runs, in this order, each step returning the first refusal:
//!
//! 1. [`CommandTable::resolve`]: exact `(id, major)` lookup. Unknown ID
//!    `not_found/unknown_command`, unknown major `not_found/unknown_major`, `removed`
//!    `not_found/removed_major` (REQ-001); `hidden` gives `hidden_command/hidden_major`
//!    unless the principal is the replay engine's `system` principal (REQ-002). `active`
//!    and `deprecated` majors resolve; [`Resolved::deprecation`] carries the
//!    `replaced_by` and `remove_after` of a deprecated one for the surface to show.
//! 2. [`Resolved::validate_json`] (or [`Resolved::validate_value`] for a parsed document):
//!    a document over [`MAX_PARAMS_BYTES`] bytes, [`MAX_PARAMS_DEPTH`] levels or
//!    [`MAX_PARAMS_ELEMENTS`] values and keys is `validation/input_limit` before it is
//!    parsed further (REQ-008); then the compiled JSON Schema ([`schema`]) with the
//!    project keywords. The refusal names the JSON pointer of the first failing value in
//!    document order and the failed keyword (REQ-003): a [`ValidationError`], which
//!    converts into the [`BusError`] the pipeline returns. `x-ref` and `x-selector`
//!    values must name entities of the state view of the declared kind, else
//!    `validation/bad_reference` (REQ-004). An authored literal (`{"value", "unit"}`) where
//!    the schema says `x-quantity` is normalised by [`crate::unit::normalise_value`]; its
//!    refusals keep their own reasons (`literal_syntax`, `unit_mismatch`,
//!    `literal_out_of_range`). The handler gets the SI values ([`Validated::params`]); the
//!    entry keeps the document as entered ([`Validated::entry_params`]) plus each
//!    literal and its SI bits ([`Validated::literals`]) (REQ-005).
//! 3. [`Resolved::check_preconditions`]: each declared precondition in order, with the
//!    registry's FPL evaluator and its step bound; false (or an evaluation error) is
//!    `precondition/false` naming the message key and the expression (REQ-006), a stopped
//!    evaluation is `precondition/step_bound_exceeded` (REQ-007).
//!
//! The text entry point also refuses an integer above 2^53 that no binary64 number equals
//! (`validation/schema`, keyword `json`; see `exact`), since parsing would round it. Validation
//! recurses once per nested schema application (at most [`MAX_EVAL_DEPTH`]); run it on a thread
//! with at least 1 MiB of stack.
//!
//! Fail closed: a schema that does not compile, a precondition that does not compile, or a
//! lifecycle the table cannot read never lets a call through; nothing here returns
//! `internal` for a caller's mistake. Every error is from the frozen bus error catalogue;
//! this module adds no code or reason. Determinism D0: document-order traversal, no clock.

mod ecma;
mod exact;
mod format;
mod pointer;
pub mod schema;

pub use ecma::{Pattern, MAX_PATTERN_BYTES, REGEX_DFA_LIMIT, REGEX_NEST_LIMIT, REGEX_SIZE_LIMIT};
pub use exact::MAX_SCAN_WORK;
pub use format::Format;
pub use pointer::{cmp_utf16, JsonPointer, Token};
pub use schema::{
    CompileError, Compiled, Fail, Hooks, RefLookup, DIALECT, MAX_EVAL_DEPTH, MAX_EVAL_STEPS, MAX_SCHEMA_DEPTH,
    MAX_SCHEMA_NODES, RANK,
};

use crate::error::{BusError, Detail, HiddenCommandReason, NotFoundReason, PreconditionReason, ValidationReason};
use crate::handler::{canonical_len, EntityId, StateView, ValidParams};
use crate::principal::{Principal, PrincipalKind};
use crate::unit::{algorithm_id, normalise_value, Normalised};
use fid_canon::{parse_with_budget, CValue, CanonError, Digest};
use fid_kci::AlgorithmId;
use fid_registry::{
    compile_command, evaluate, Catalogue, CommandDef, ContextTypes, FieldPath, Lifecycle, ParamSchema, PredContext,
    PredError, Predicate, State, Type, Value, Verdict,
};
use fid_types::{CommandId, CommandName};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

/// Largest parameter document, in bytes of JSON text or of RFC 8785 canonical form (REQ-008).
pub const MAX_PARAMS_BYTES: usize = 16 * 1024 * 1024;
/// Deepest parameter document: the root container is level 1 (REQ-008).
pub const MAX_PARAMS_DEPTH: usize = 64;
/// Most values and object keys in one parameter document (each counts one), so a small
/// document of tiny values cannot build a value tree far larger than its text.
pub const MAX_PARAMS_ELEMENTS: usize = 1 << 20;
/// Principal ID of the replay engine's `system` principal: the only caller that may invoke
/// a `hidden` major (REQ-002). A `system` principal comes only from the internal channel.
pub const REPLAY_PRINCIPAL_ID: &str = "replay";
/// Most command tables a [`TableCache`] keeps.
pub const TABLE_CACHE_CAPACITY: usize = 4;

// ------------------------------------------------------------------ errors

/// A parameter refusal: the JSON pointer of the first failing value in document order,
/// the failed keyword and the reason (REQ-003). Pseudo-keywords: `json` (the text is not
/// acceptable JSON), `input_limit` (a size, depth, element or evaluation limit), `schema`
/// (the command's schema does not compile).
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationError {
    pub pointer: JsonPointer,
    pub keyword: &'static str,
    pub reason: ValidationReason,
    /// Fixed explanatory text.
    pub message: String,
    bus: Option<BusError>,
}

impl ValidationError {
    fn new(pointer: JsonPointer, keyword: &'static str, reason: ValidationReason, message: &str) -> Self {
        ValidationError { pointer, keyword, reason, message: message.to_string(), bus: None }
    }

    fn limit(message: &str) -> Self {
        ValidationError::new(JsonPointer::root(), "input_limit", ValidationReason::InputLimit, message)
    }

    /// The bus error the pipeline returns: `validation/<reason>` with the keyword and the
    /// pointer in the detail, or the hook's own error for a broken invariant.
    pub fn to_bus_error(&self) -> BusError {
        if let Some(e) = &self.bus {
            return e.clone();
        }
        let ptr: String = self.pointer.to_string().chars().take(512).collect();
        BusError::Validation(self.reason, Detail::new(&format!("{} at {ptr:?}: {}", self.keyword, self.message)))
    }
}

impl From<Fail> for ValidationError {
    fn from(f: Fail) -> Self {
        ValidationError { pointer: f.pointer, keyword: f.keyword, reason: f.reason, message: f.message, bus: f.bus }
    }
}

impl From<ValidationError> for BusError {
    fn from(e: ValidationError) -> Self {
        e.to_bus_error()
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "validation/{}: {} at {:?}", self.reason, self.keyword, self.pointer.to_string())
    }
}

/// Why a [`CommandTable`] could not be built. Never a call-time error: the registry build
/// already refuses such definitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableError {
    /// A definition's lifecycle does not read.
    Lifecycle { command: String, message: String },
    /// Two definitions share an `(id, major)`.
    Duplicate(String),
    /// A cached build returned a table for another manifest.
    DigestMismatch,
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TableError::Lifecycle { command, message } => write!(f, "{command}: lifecycle: {message}"),
            TableError::Duplicate(c) => write!(f, "{c} is defined more than once"),
            TableError::DigestMismatch => f.write_str("table was built for another manifest digest"),
        }
    }
}

impl std::error::Error for TableError {}

// ------------------------------------------------------------------ table

struct Command {
    def: CommandDef,
    lifecycle: Lifecycle,
    schema: Result<Compiled, CompileError>,
    preds: Result<Vec<Predicate>, String>,
    param_types: ParamSchema,
}

/// The commands of one Registry Manifest, with their schemas compiled and preconditions
/// type-checked once.
pub struct CommandTable {
    digest: Digest,
    catalogue: Catalogue,
    commands: BTreeMap<(String, u32), Command>,
    names: BTreeSet<String>,
}

impl fmt::Debug for CommandTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandTable").field("digest", &self.digest).field("commands", &self.commands.len()).finish()
    }
}

impl CommandTable {
    /// Builds the table of the manifest with digest `manifest_digest` from its definitions
    /// (removed majors may be included; they resolve to `not_found/removed_major`), the
    /// manifest's quantity catalogue and the types of the precondition context roots other
    /// than `params`. A schema or precondition that does not compile is kept as such and
    /// refuses every call of its command ([`CommandTable::compile_errors`] lists them).
    pub fn build(
        manifest_digest: Digest,
        defs: Vec<CommandDef>,
        catalogue: Catalogue,
        context: &ContextTypes,
    ) -> Result<CommandTable, TableError> {
        let mut commands = BTreeMap::new();
        let mut names = BTreeSet::new();
        for def in defs {
            let name = format!("{}@{}", def.id, def.major);
            let lifecycle = Lifecycle::from_map(&def.lifecycle)
                .map_err(|e| TableError::Lifecycle { command: name.clone(), message: e.to_string() })?;
            let schema = Compiled::compile(&def.params_schema);
            let preds = compile_command(&def, context).map_err(|e| e.to_string());
            let param_types = ParamSchema::from_json_schema(&def.params_schema);
            let key = (def.id.clone(), def.major);
            names.insert(def.id.clone());
            let cmd = Command { def, lifecycle, schema, preds, param_types };
            if commands.insert(key, cmd).is_some() {
                return Err(TableError::Duplicate(name));
            }
        }
        Ok(CommandTable { digest: manifest_digest, catalogue, commands, names })
    }

    /// The manifest digest this table was built for.
    pub fn manifest_digest(&self) -> Digest {
        self.digest
    }

    /// Commands whose schema or preconditions do not compile, as `(id@major, why)`.
    pub fn compile_errors(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for ((id, major), c) in &self.commands {
            if let Err(e) = &c.schema {
                out.push((format!("{id}@{major}"), format!("params_schema {e}")));
            }
            if let Err(e) = &c.preds {
                out.push((format!("{id}@{major}"), e.clone()));
            }
        }
        out
    }

    /// Resolves `id@major` for `principal` (REQ-001, REQ-002).
    pub fn resolve(&self, id: &str, major: u32, principal: &Principal) -> Result<Resolved<'_>, BusError> {
        let shown: String = id.chars().take(128).collect();
        let not_found =
            |r: NotFoundReason, what: &str| BusError::NotFound(r, Detail::new(&format!("{shown:?}@{major}: {what}")));
        if !CommandName::is_valid(id) || !self.names.contains(id) {
            return Err(unknown_command(id, major));
        }
        let Some(cmd) = self.commands.get(&(id.to_string(), major)) else {
            return Err(not_found(NotFoundReason::UnknownMajor, "unknown major"));
        };
        match cmd.lifecycle.state {
            State::Removed => Err(not_found(NotFoundReason::RemovedMajor, "major was removed")),
            State::Hidden if !is_replay_engine(principal) => Err(BusError::HiddenCommand(
                HiddenCommandReason::HiddenMajor,
                Detail::new(&format!("{shown}@{major}: hidden major; only replay may invoke it")),
            )),
            State::Active | State::Deprecated | State::Hidden => Ok(Resolved { table: self, cmd }),
        }
    }

    /// [`CommandTable::resolve`] for a typed command ID.
    pub fn resolve_id(&self, id: &CommandId, principal: &Principal) -> Result<Resolved<'_>, BusError> {
        self.resolve(id.name(), id.major(), principal)
    }
}

/// The refusal of a command `id@major` the table does not hold: `not_found/unknown_command`
/// with detail `"<id>"@<major>: unknown command` (the ID cut to 128 characters). The MCP
/// projection check returns exactly this for a `never`-projected command, so an agent cannot
/// tell a hidden command from one that does not exist (D-035, CORE.BUS.POL.PROJ).
pub fn unknown_command(id: &str, major: u32) -> BusError {
    let shown: String = id.chars().take(128).collect();
    BusError::NotFound(NotFoundReason::UnknownCommand, Detail::new(&format!("{shown:?}@{major}: unknown command")))
}

/// Whether `p` is the replay engine's `system` principal.
pub fn is_replay_engine(p: &Principal) -> bool {
    p.kind() == PrincipalKind::System && p.id().as_str() == REPLAY_PRINCIPAL_ID
}

/// The command tables of recent manifests, keyed by manifest digest, at most
/// [`TABLE_CACHE_CAPACITY`] (the oldest is dropped first).
#[derive(Debug, Default)]
pub struct TableCache {
    tables: Vec<Arc<CommandTable>>,
}

impl TableCache {
    pub fn new() -> Self {
        TableCache::default()
    }

    /// The table of `digest`, if cached.
    pub fn get(&self, digest: &Digest) -> Option<Arc<CommandTable>> {
        self.tables.iter().find(|t| t.digest == *digest).cloned()
    }

    /// The table of `digest`, building it with `build` on a miss.
    pub fn get_or_build(
        &mut self,
        digest: Digest,
        build: impl FnOnce() -> Result<CommandTable, TableError>,
    ) -> Result<Arc<CommandTable>, TableError> {
        if let Some(t) = self.get(&digest) {
            return Ok(t);
        }
        let t = build()?;
        if t.digest != digest {
            return Err(TableError::DigestMismatch);
        }
        if self.tables.len() >= TABLE_CACHE_CAPACITY {
            self.tables.remove(0);
        }
        let t = Arc::new(t);
        self.tables.push(Arc::clone(&t));
        Ok(t)
    }
}

// ------------------------------------------------------------------ resolved command

/// A resolved command: its definition and lifecycle. Validation and preconditions run
/// through it, so they always use the schema compiled for this manifest.
#[derive(Clone, Copy)]
pub struct Resolved<'a> {
    table: &'a CommandTable,
    cmd: &'a Command,
}

impl fmt::Debug for Resolved<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Resolved({}@{}, {})", self.cmd.def.id, self.cmd.def.major, self.cmd.lifecycle.state)
    }
}

/// What a surface shows for a deprecated major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deprecation {
    pub replaced_by: Option<CommandId>,
    pub remove_after: Option<fid_registry::Date>,
}

impl<'a> Resolved<'a> {
    /// The command definition.
    pub fn def(&self) -> &'a CommandDef {
        &self.cmd.def
    }

    /// The lifecycle of this major.
    pub fn lifecycle(&self) -> &'a Lifecycle {
        &self.cmd.lifecycle
    }

    /// `id@major`.
    pub fn command_id(&self) -> String {
        format!("{}@{}", self.cmd.def.id, self.cmd.def.major)
    }

    /// For a deprecated major, its successor and removal date, if declared.
    pub fn deprecation(&self) -> Option<Deprecation> {
        let l = &self.cmd.lifecycle;
        (l.state == State::Deprecated)
            .then(|| Deprecation { replaced_by: l.replaced_by.clone(), remove_after: l.remove_after })
    }

    /// Validates a parameter document given as JSON text (REQ-003..005, REQ-008).
    pub fn validate_json(&self, bytes: &[u8], view: &dyn StateView) -> Result<Validated, ValidationError> {
        if bytes.len() > MAX_PARAMS_BYTES {
            return Err(ValidationError::limit("parameter document is larger than 16 MiB"));
        }
        let value =
            parse_with_budget(bytes, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH, MAX_PARAMS_ELEMENTS).map_err(|e| match e {
                CanonError::InputLimit => {
                    ValidationError::limit("parameter document exceeds the size, depth or element limit")
                }
                other => ValidationError::new(
                    JsonPointer::root(),
                    "json",
                    ValidationReason::Schema,
                    &format!("parameter document is not acceptable JSON ({})", other.code()),
                ),
            })?;
        match exact::first_inexact_integer(bytes) {
            exact::Scan::Clean => {}
            exact::Scan::Inexact(p) => {
                return Err(ValidationError::new(
                    p,
                    "json",
                    ValidationReason::Schema,
                    "integer above 2^53 that no binary64 number equals",
                ))
            }
            exact::Scan::TooMuchWork => {
                return Err(ValidationError::limit("deciding integer exactness exceeds its work budget"))
            }
        }
        self.validate_checked(value, view)
    }

    /// Validates a parsed parameter document (REQ-003..005, REQ-008). Its limits are checked
    /// first, without walking past them; only the number form JSON text produces is
    /// accepted (no `I64`, `U64` or `F64Bits`).
    ///
    /// **Warning:** integers above 2^53 that no binary64 number equals are refused only by
    /// [`Resolved::validate_json`], which sees the JSON text. A parsed value has already been
    /// rounded (`9007199254740993` became `9007199254740992`), so this entry point cannot refuse
    /// it. Callers holding JSON text, the pipeline included, must use `validate_json`.
    pub fn validate_value(&self, params: &CValue, view: &dyn StateView) -> Result<Validated, ValidationError> {
        prescan(params)?;
        self.validate_checked(params.clone(), view)
    }

    fn validate_checked(&self, value: CValue, view: &dyn StateView) -> Result<Validated, ValidationError> {
        let schema = self.cmd.schema.as_ref().map_err(|e| {
            ValidationError::new(
                JsonPointer::root(),
                "schema",
                ValidationReason::Schema,
                &format!("the parameter schema of {} does not compile: {e}", self.command_id()),
            )
        })?;
        let hooks = BusHooks { catalogue: &self.table.catalogue, view };
        let lits = schema.evaluate(&value, &hooks)?;
        let mut normalised = value.clone();
        for (p, n) in &lits {
            if let Some(slot) = get_mut(&mut normalised, p) {
                *slot = n.si_value();
            }
        }
        let literals = lits.into_iter().map(|(pointer, normalised)| NormalisedParam { pointer, normalised }).collect();
        Ok(Validated { params: ValidParams::from_validated(normalised), entry_params: value, literals })
    }

    /// Evaluates the command's preconditions in declared order against the validated
    /// parameters (root `params`) and `roots` (every other context root) (REQ-006, REQ-007).
    pub fn check_preconditions(&self, params: &ValidParams, roots: &dyn PredContext) -> Result<(), BusError> {
        let id = self.command_id();
        let preds = self.cmd.preds.as_ref().map_err(|e| {
            BusError::Precondition(
                PreconditionReason::False,
                Detail::new(&format!("{id}: a precondition does not compile: {e}")),
            )
        })?;
        let ctx = ParamsContext { params: params.value(), types: &self.cmd.param_types, roots };
        for p in preds {
            let eval = evaluate(p, &ctx);
            let named = format!("{id}: precondition {} ({})", p.message_key, p.src);
            match eval.result {
                Ok(Verdict::True) => {}
                Ok(Verdict::False { .. }) => {
                    return Err(BusError::Precondition(PreconditionReason::False, Detail::new(&named)));
                }
                Err(PredError::Budget { steps }) => {
                    return Err(BusError::Precondition(
                        PreconditionReason::StepBoundExceeded,
                        Detail::new(&format!("{named} stopped at its step bound of {steps}")),
                    ));
                }
                Err(e) => {
                    return Err(BusError::Precondition(
                        PreconditionReason::False,
                        Detail::new(&format!("{named} could not be evaluated: {}", e.code())),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// `table.resolve(id, major, principal)` (the build prompt's free-function form).
pub fn resolve<'a>(table: &'a CommandTable, id: &str, major: u32, p: &Principal) -> Result<Resolved<'a>, BusError> {
    table.resolve(id, major, p)
}

/// [`Resolved::validate_value`] with the refusal as a [`BusError`].
///
/// **Warning:** like [`Resolved::validate_value`], it cannot see integers above 2^53 that
/// parsing rounded; callers holding JSON text must use [`validate_params_json`].
pub fn validate_params(cmd: &Resolved<'_>, params: &CValue, view: &dyn StateView) -> Result<Validated, BusError> {
    cmd.validate_value(params, view).map_err(BusError::from)
}

/// [`Resolved::validate_json`] with the refusal as a [`BusError`].
pub fn validate_params_json(cmd: &Resolved<'_>, json: &[u8], view: &dyn StateView) -> Result<Validated, BusError> {
    cmd.validate_json(json, view).map_err(BusError::from)
}

/// [`Resolved::check_preconditions`].
pub fn check_preconditions(cmd: &Resolved<'_>, params: &ValidParams, roots: &dyn PredContext) -> Result<(), BusError> {
    cmd.check_preconditions(params, roots)
}

// ------------------------------------------------------------------ results

/// One authored literal and its normalisation (REQ-005).
#[derive(Debug, Clone, PartialEq)]
pub struct NormalisedParam {
    /// Where the literal was in the parameter document.
    pub pointer: JsonPointer,
    /// The SI value, the literal as entered and the algorithm ID.
    pub normalised: Normalised,
}

/// Parameters that passed validation.
#[derive(Debug, Clone, PartialEq)]
pub struct Validated {
    params: ValidParams,
    entry_params: CValue,
    literals: Vec<NormalisedParam>,
}

impl Validated {
    /// What the handler gets: literals replaced by their SI values.
    pub fn params(&self) -> &ValidParams {
        &self.params
    }

    /// The handler parameters, by value.
    pub fn into_params(self) -> ValidParams {
        self.params
    }

    /// The document as entered, literals included, for the journal entry's arguments.
    pub fn entry_params(&self) -> &CValue {
        &self.entry_params
    }

    /// Each normalised literal, in document order.
    pub fn literals(&self) -> &[NormalisedParam] {
        &self.literals
    }

    /// The algorithm IDs the entry records (`core.unit.normalise@1.0.0` when a literal was
    /// normalised; CORE.BUS.UNIT REQ-005).
    pub fn algorithms(&self) -> Vec<AlgorithmId> {
        if self.literals.is_empty() {
            Vec::new()
        } else {
            vec![algorithm_id()]
        }
    }
}

// ------------------------------------------------------------------ internals

struct BusHooks<'a> {
    catalogue: &'a Catalogue,
    view: &'a dyn StateView,
}

impl Hooks for BusHooks<'_> {
    fn literal(&self, kind: &str, value: &CValue) -> Result<Normalised, BusError> {
        normalise_value(value, kind, self.catalogue)
    }

    fn entity(&self, id: &str) -> RefLookup {
        let Ok(id) = EntityId::new(id) else { return RefLookup::BadId };
        match self.view.entity(&id) {
            None => RefLookup::Missing,
            Some(e) => RefLookup::Kind(e.kind.to_string()),
        }
    }
}

/// Checks the limits of a parsed document iteratively, stopping at the first one exceeded,
/// then refuses number forms JSON text does not produce.
fn prescan(root: &CValue) -> Result<(), ValidationError> {
    let mut elements = 0usize;
    let mut stack: Vec<(&CValue, usize)> = vec![(root, 0)];
    while let Some((v, depth)) = stack.pop() {
        elements += 1;
        if elements > MAX_PARAMS_ELEMENTS {
            return Err(ValidationError::limit("parameter document has more than 1048576 values and keys"));
        }
        match v {
            CValue::Arr(items) => {
                if depth + 1 > MAX_PARAMS_DEPTH {
                    return Err(ValidationError::limit("parameter document nests deeper than 64 levels"));
                }
                stack.extend(items.iter().map(|i| (i, depth + 1)));
            }
            CValue::Obj(m) => {
                if depth + 1 > MAX_PARAMS_DEPTH {
                    return Err(ValidationError::limit("parameter document nests deeper than 64 levels"));
                }
                elements = elements.saturating_add(m.len());
                stack.extend(m.values().map(|i| (i, depth + 1)));
            }
            _ => {}
        }
    }
    match canonical_len(root, MAX_PARAMS_BYTES as u64) {
        Ok(n) if n <= MAX_PARAMS_BYTES as u64 => {}
        Ok(_) => return Err(ValidationError::limit("parameter document is larger than 16 MiB")),
        Err(e) => {
            return Err(ValidationError::new(
                JsonPointer::root(),
                "json",
                ValidationReason::Schema,
                &format!("parameter document has no canonical form ({})", e.code()),
            ))
        }
    }
    let mut stack: Vec<(&CValue, JsonPointer)> = vec![(root, JsonPointer::root())];
    let mut first: Option<JsonPointer> = None;
    while let Some((v, p)) = stack.pop() {
        match v {
            CValue::I64(_) | CValue::U64(_) | CValue::F64Bits(_) => {
                if first.as_ref().map_or(true, |f| p.doc_cmp(f).is_lt()) {
                    first = Some(p);
                }
            }
            CValue::Arr(items) => stack.extend(items.iter().enumerate().map(|(i, x)| (x, p.index(i)))),
            CValue::Obj(m) => stack.extend(m.iter().map(|(k, x)| (x, p.key(k)))),
            _ => {}
        }
    }
    match first {
        Some(p) => Err(ValidationError::new(p, "type", ValidationReason::Schema, "not a JSON number form")),
        None => Ok(()),
    }
}

fn get_mut<'v>(root: &'v mut CValue, p: &JsonPointer) -> Option<&'v mut CValue> {
    let mut cur = root;
    for t in p.tokens() {
        cur = match (cur, t) {
            (CValue::Arr(a), Token::Index(i)) => a.get_mut(*i)?,
            (CValue::Obj(m), Token::Key(k)) => m.get_mut(k)?,
            _ => return None,
        };
    }
    Some(cur)
}

/// The precondition context: `params.*` from the validated parameters, typed by the
/// parameter schema; every other root from the caller's `roots`.
struct ParamsContext<'a> {
    params: &'a CValue,
    types: &'a ParamSchema,
    roots: &'a dyn PredContext,
}

fn untyped(v: &CValue) -> Option<Value> {
    Some(match v {
        CValue::Bool(b) => Value::Bool(*b),
        CValue::Num(x) => Value::Float(*x),
        CValue::Str(s) => Value::Str(s.clone()),
        CValue::Arr(items) => Value::List(items.iter().filter_map(untyped).collect()),
        CValue::Obj(m) => Value::Map(m.iter().filter_map(|(k, x)| Some((k.clone(), untyped(x)?))).collect()),
        _ => return None,
    })
}

fn typed(v: &CValue, t: &Type) -> Option<Value> {
    Some(match (t, v) {
        (Type::Bool, CValue::Bool(b)) => Value::Bool(*b),
        (Type::Int, CValue::Num(x)) if x.fract() == 0.0 && x.abs() <= 9_007_199_254_740_992.0 => Value::Int(*x as i64),
        (Type::Float, CValue::Num(x)) => Value::Float(*x),
        (Type::String, CValue::Str(s)) => Value::Str(s.clone()),
        (Type::EntityRef, CValue::Str(s)) => Value::Ref(s.clone()),
        (Type::List(inner), CValue::Arr(items)) => {
            Value::List(items.iter().map(|i| typed(i, inner)).collect::<Option<Vec<_>>>()?)
        }
        (Type::Map, CValue::Obj(_)) => untyped(v)?,
        _ => return None,
    })
}

impl PredContext for ParamsContext<'_> {
    fn get(&self, path: &FieldPath) -> Result<Value, PredError> {
        let missing = || PredError::MissingField { path: path.to_string() };
        if path.0.first().map(String::as_str) != Some("params") {
            return self.roots.get(path);
        }
        let mut cur = self.params;
        for seg in path.0.iter().skip(1) {
            cur = match cur {
                CValue::Obj(m) => m.get(seg).ok_or_else(missing)?,
                _ => return Err(missing()),
            };
        }
        let key = path.0.join(".");
        let t = self.types.0 .0.get(&key).ok_or_else(missing)?;
        typed(cur, t).ok_or_else(|| PredError::TypeError { pos: 0, detail: format!("{key} is not a {t}") })
    }
}

/// A [`PredContext`] with no roots besides `params`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRoots;

impl PredContext for NoRoots {
    fn get(&self, path: &FieldPath) -> Result<Value, PredError> {
        Err(PredError::MissingField { path: path.to_string() })
    }
}

//! Diameter and radius checks (spec node `GDT.DIM.RADIAL`): the commands
//! `check.eval_diameter@1` and `check.eval_radius@1` (W2, dry run R1; CLI
//! `fid check eval-diameter` / `fid check eval-radius`, MCP `check_eval_diameter` /
//! `check_eval_radius`), their journal event and exact replay.
//!
//! **Built today.** The least-squares circle (`circle2`, `circle3`), cylinder and sphere fits of
//! `fid-fit`. A cone is checked for its gauge position (`gauge_position_missing`, AC-3) and then
//! refused with `fit_unavailable{fit.cone.ls@1.0.0}` until the cone fit (`KRN.LSQ.AXIS.CONE`)
//! is on main; no cone value is ever made up (AC-2 open).
//!
//! **Pipeline of [`eval_radial`]** (every check runs before anything is returned; a refusal and
//! a dry run journal nothing and commit nothing):
//!
//! 1. the request: run ID, characteristic ID (`bad_params{bad_char_id}`), feature, optional
//!    DMT variant and optional gauge position;
//! 2. the measured points of the feature ([`RadialFeatures`]; `measurement_missing{feature}`),
//!    with the fit algorithm the feature is defined with;
//! 3. the interpretation chain: `gdt.resolve_chain@1`'s resolver for the characteristic type
//!    `diameter` or `radius` with no modifier and the requested variant, behind the
//!    governing-spec gate (its errors pass through: `governing_spec_missing`, ...); an outcome
//!    other than `RESOLVED` is `chain_not_resolved{outcome}`;
//! 4. the pure evaluation `fid_gps::dim::radial::evaluate` (its refusals pass through:
//!    `not_radial_feature`, `gauge_position_missing`, `gauge_not_applicable`,
//!    `fit_method_mismatch`, `ambiguous_association`, `fit_unavailable`, `fit_refused{code}`;
//!    a fit flagged `local_optimum_only` or `suspect_axis`, or ill-conditioned, is never a
//!    value);
//! 5. one record per (run, characteristic) visible to the principal (`duplicate_result`);
//! 6. the ResultRecord through `GDT.CHAR.RECORD`'s builder: measurand `<measure> of
//!    <feature>`, basis `POINTS`, the value in `si:m`, UEC-0 (`u_missing`), no bound decision
//!    rule (`NOT_DECIDABLE`, `no_rule_bound`), the governing spec, the fit's algorithm ID as the
//!    record's algorithm (REQ-003), the points digest as its input, the resolved row, chain and
//!    DMT digests and the interpretation block of D-045 (`rule_not_admitted{rule}` for a draft
//!    row). The record is checked in full here (with a placeholder journal entry) so every
//!    builder refusal comes before the event.
//!
//! **Journal entry.** A record names the journal entry that holds its event, which does not
//! exist until the event is committed, so the record is finished in a second step:
//! [`Evaluated::record`] with the entry's hash, then [`commit`] into the [`ResultStore`]. The
//! event carries everything the record holds except that hash; replay rebuilds the record from
//! the entry it reads.
//!
//! **Values.** The only value source is measured points fitted here by `fid-fit`. There is no
//! input for a fitted, nominal or claimed radius, and the uncertainty is UEC-0, so nothing
//! claimed can enter a record as if measured (D-045). The gauge distance is a declared
//! parameter and is recorded as such in the event.
//!
//! Floats in events and results are `0x` + 16 lowercase hex digits of their bits (D0).

pub mod command;
pub mod event;

pub use command::{
    diameter_command_def, invoke_cli, invoke_mcp, radius_command_def, EVAL_DIAMETER_CLI, EVAL_DIAMETER_MCP,
    EVAL_RADIUS_CLI, EVAL_RADIUS_MCP,
};
pub use event::{replay, RadialEvent, RadialReplayed};

use crate::chars::dprof::{Branch, CharId, DprofState};
use crate::chars::interp::{InterpretationPin, Interpreted, RegisterView, PROVISIONAL_LABEL_TEXT, SIGNED_OFF_LINE};
use crate::chars::record::{
    AlgorithmRef, EvaluationBasis, InputDigests, KernelVersion, Measurand, Quantity, RecordError, ResultRecord,
    ResultRecordBuilder, ResultStore, RunId, Uncertainty,
};
use crate::chars::spec::{SpecError, SpecState};
use crate::dmt_flow::status::Purpose;
use crate::resolve::{resolve_chain, CharacteristicRef, CommandError, CHAR_SCHEMA};
use fid_canon::CValue;
use fid_gps::dim::radial::{evaluate, Gauge, Measure, RadialError, RadialInput, RadialValue};
use fid_gps::dmt::{AlgId, DmtTable, OperatorKind};
use fid_gps::resolve::Resolution;
use fid_io_reg::Principal;
use fid_kunc::Uec0Reason;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Command ID of the diameter check.
pub const EVAL_DIAMETER_COMMAND: &str = "check.eval_diameter@1";
/// Command ID of the radius check.
pub const EVAL_RADIUS_COMMAND: &str = "check.eval_radius@1";
/// `schema` of the command result.
pub const RESULT_SCHEMA: &str = "fid.check-radial-result/1";
/// The record unit: metres.
pub const UNIT: &str = "si:m";
/// Most measured points of one feature.
pub const MAX_FEATURE_POINTS: usize = 1 << 20;
/// Most measured features in one [`RadialFeatures`].
pub const MAX_FEATURES: usize = 4096;
/// Longest feature ID or variant name, in bytes.
pub const MAX_ID_LEN: usize = 128;
/// Longest text echoed back inside an error, in bytes.
const MAX_ECHO: usize = 160;

pub(crate) fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

/// The command ID of a measure.
pub fn command_of(measure: Measure) -> &'static str {
    match measure {
        Measure::Diameter => EVAL_DIAMETER_COMMAND,
        Measure::Radius => EVAL_RADIUS_COMMAND,
    }
}

/// Errors of the two commands. [`EvalError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    /// `bad_params{reason}`: CLI arguments or MCP parameters that are not a valid call.
    BadParams {
        /// Static reason.
        reason: &'static str,
    },
    /// `measurement_missing{feature}`: no measured points for the feature.
    MeasurementMissing {
        /// The feature (truncated).
        feature: String,
    },
    /// `measurement_invalid{feature:reason}`: points that cannot be stored.
    MeasurementInvalid {
        /// The feature (truncated).
        feature: String,
        /// Static reason.
        reason: &'static str,
    },
    /// An error of the chain resolution (the governing-spec gate, `dmt_not_loaded`, the
    /// resolver), passed through.
    Chain(CommandError),
    /// `chain_not_resolved{outcome}`: the resolver said `UNRESOLVED` or `UNSUPPORTED`.
    ChainNotResolved {
        /// The outcome.
        outcome: &'static str,
    },
    /// An error of the pure evaluation (`fid_gps::dim::radial`), passed through.
    Radial(RadialError),
    /// An error of the ResultRecord builder or store, passed through.
    Record(RecordError),
    /// `encode_failed`: the canonical writer failed (a defect).
    Encode,
    /// `event_invalid{reason}`: a journal event could not be read.
    Event {
        /// Static reason.
        reason: &'static str,
    },
}

impl EvalError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            EvalError::BadParams { .. } => "bad_params",
            EvalError::MeasurementMissing { .. } => "measurement_missing",
            EvalError::MeasurementInvalid { .. } => "measurement_invalid",
            EvalError::Chain(e) => e.code(),
            EvalError::ChainNotResolved { .. } => "chain_not_resolved",
            EvalError::Radial(e) => e.code(),
            EvalError::Record(e) => e.code(),
            EvalError::Encode => "encode_failed",
            EvalError::Event { .. } => "event_invalid",
        }
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.code();
        match self {
            EvalError::BadParams { reason } | EvalError::Event { reason } => write!(f, "{c}{{{reason}}}"),
            EvalError::ChainNotResolved { outcome } => write!(f, "{c}{{{outcome}}}"),
            EvalError::MeasurementMissing { feature } => write!(f, "{c}{{{}}}", feature.escape_debug()),
            EvalError::MeasurementInvalid { feature, reason } => {
                write!(f, "{c}{{{}:{reason}}}", feature.escape_debug())
            }
            EvalError::Chain(e) => e.fmt(f),
            EvalError::Radial(e) => e.fmt(f),
            EvalError::Record(e) => e.fmt(f),
            EvalError::Encode => f.write_str(c),
        }
    }
}

impl std::error::Error for EvalError {}

impl From<RecordError> for EvalError {
    fn from(e: RecordError) -> Self {
        EvalError::Record(e)
    }
}

/// A feature ID or variant name: `[A-Za-z0-9][A-Za-z0-9._:-]*`, at most [`MAX_ID_LEN`] bytes.
pub(crate) fn is_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_ID_LEN
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'-'))
}

#[derive(Debug, Clone, PartialEq)]
struct Measured {
    fit: AlgId,
    xyz: Vec<f64>,
    digest: Digest,
}

/// SHA-256 of `fid.gdt-radial-points/1\n`, the fit algorithm ID, `\n`, then the coordinates as
/// little-endian `f64` bits.
fn points_digest(fit: &AlgId, xyz: &[f64]) -> Digest {
    let mut b = Vec::with_capacity(64 + 8 * xyz.len());
    b.extend_from_slice(b"fid.gdt-radial-points/1\n");
    b.extend_from_slice(fit.as_str().as_bytes());
    b.push(b'\n');
    for x in xyz {
        b.extend_from_slice(&x.to_bits().to_le_bytes());
    }
    Digest::from_bytes(fid_canon::digest_bytes(&b).0)
}

/// The measured points of the plan's radial features, each with the fit algorithm the feature
/// is defined with (`fit.<primitive>.<method>@<x.y.z>`), content-addressed by
/// [`RadialFeatures::digest`]. This is the narrow input the commands read measurements from
/// until a measurement store exists; the caller vouches that the points are measured.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RadialFeatures {
    map: BTreeMap<String, Measured>,
}

impl RadialFeatures {
    /// An empty set.
    pub fn new() -> RadialFeatures {
        RadialFeatures::default()
    }

    /// Stores (or replaces) the points of `feature`, defined with fit algorithm `fit`, and
    /// returns their digest. Refused with `measurement_invalid{feature:reason}`: `bad_id`,
    /// `bad_fit` (not an algorithm ID), `bad_length` (empty or not whole triples),
    /// `too_many_points`, `non_finite`, `too_many_features`.
    pub fn insert(&mut self, feature: &str, fit: &str, xyz: Vec<f64>) -> Result<Digest, EvalError> {
        let bad = |reason| EvalError::MeasurementInvalid { feature: echo(feature), reason };
        if !is_id(feature) {
            return Err(bad("bad_id"));
        }
        let fit = AlgId::parse(fit).map_err(|_| bad("bad_fit"))?;
        if xyz.is_empty() || xyz.len() % 3 != 0 {
            return Err(bad("bad_length"));
        }
        if xyz.len() / 3 > MAX_FEATURE_POINTS {
            return Err(bad("too_many_points"));
        }
        if xyz.iter().any(|x| !x.is_finite()) {
            return Err(bad("non_finite"));
        }
        if !self.map.contains_key(feature) && self.map.len() >= MAX_FEATURES {
            return Err(bad("too_many_features"));
        }
        let digest = points_digest(&fit, &xyz);
        self.map.insert(feature.to_owned(), Measured { fit, xyz, digest });
        Ok(digest)
    }

    /// The digest of `feature`'s points, if stored.
    pub fn digest(&self, feature: &str) -> Option<Digest> {
        self.map.get(feature).map(|m| m.digest)
    }

    /// The fit `feature` is defined with, if stored.
    pub fn fit(&self, feature: &str) -> Option<&AlgId> {
        self.map.get(feature).map(|m| &m.fit)
    }
}

/// What the commands read: governing specs, defaults profiles, the loaded DMT table, the
/// measured features and the status register view of the interpretation block (D-045).
#[derive(Debug, Clone, Copy)]
pub struct EvalInputs<'a> {
    /// Governing specs (`GDT.CHAR.SPEC`).
    pub spec: &'a SpecState,
    /// Defaults profiles (`GDT.CHAR.DPROF`).
    pub dprof: &'a DprofState,
    /// The loaded DMT table (`None`: `dmt_not_loaded`).
    pub table: Option<&'a DmtTable>,
    /// The measured features.
    pub features: &'a RadialFeatures,
    /// The status register view.
    pub register: &'a RegisterView,
}

/// The parameters of `check.eval_diameter@1` / `check.eval_radius@1`.
#[derive(Debug, Clone, Copy)]
pub struct EvalRequest<'a> {
    /// `run`: the run the record belongs to.
    pub run: &'a str,
    /// `char`: the characteristic (its governing spec and defaults profile apply).
    pub char: &'a str,
    /// `feature`: the measured feature.
    pub feature: &'a str,
    /// `variant`: the DMT variant to use instead of the profile's or the row's default.
    pub variant: Option<&'a str>,
    /// `gauge`: the gauge position of a cone.
    pub gauge: Option<Gauge>,
}

/// The result of an evaluation: the value with its fit and chain, the label, and (unless a dry
/// run) the journal event. [`Evaluated::record`] finishes the ResultRecord once the event's
/// journal entry is known.
#[derive(Debug, Clone)]
pub struct Evaluated {
    command: &'static str,
    principal: Principal,
    branch: Branch,
    run: RunId,
    char: CharId,
    feature: String,
    variant: Option<String>,
    gauge: Option<Gauge>,
    value: RadialValue,
    points_digest: Digest,
    row: String,
    chain_variant: String,
    chain_digest: Digest,
    dmt_digest: Digest,
    pin: InterpretationPin,
    interpretation: &'static str,
    label: &'static str,
    dry_run: bool,
    builder: ResultRecordBuilder<Interpreted>,
    event: Option<RadialEvent>,
}

pub(crate) fn hex(x: f64) -> CValue {
    CValue::F64Bits(x)
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

pub(crate) fn obj<const N: usize>(items: [(&str, CValue); N]) -> CValue {
    CValue::Obj(items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

pub(crate) fn gauge_cvalue(g: Option<Gauge>) -> CValue {
    match g {
        None => CValue::Null,
        Some(Gauge::FromApex(d)) => obj([("from_apex", hex(d))]),
    }
}

impl Evaluated {
    /// The command run.
    pub fn command(&self) -> &'static str {
        self.command
    }
    /// The branch the record goes to.
    pub fn branch(&self) -> Branch {
        self.branch
    }
    /// The evaluated value and its fit.
    pub fn value(&self) -> &RadialValue {
        &self.value
    }
    /// The digest of the measured points read.
    pub fn points_digest(&self) -> Digest {
        self.points_digest
    }
    /// The interpretation pin the record carries.
    pub fn pin(&self) -> &InterpretationPin {
        &self.pin
    }
    /// `provisional` or `signed_off`.
    pub fn interpretation(&self) -> &'static str {
        self.interpretation
    }
    /// The display line of the interpretation label (D-045).
    pub fn label(&self) -> &'static str {
        self.label
    }
    /// Whether this was a dry run.
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }
    /// The journal event; `None` for a dry run.
    pub fn event(&self) -> Option<&RadialEvent> {
        self.event.as_ref()
    }

    /// The ResultRecord, naming `journal_entry`, the hash of the journal entry that holds the
    /// event. Refused for a dry run (`bad_params{dry_run}`).
    pub fn record(&self, journal_entry: Digest) -> Result<ResultRecord, EvalError> {
        if self.dry_run {
            return Err(EvalError::BadParams { reason: "dry_run" });
        }
        Ok(self.builder.clone().journal_entry(journal_entry).finish()?)
    }

    /// The `--json` / MCP result: `{branch, chain, char, command, dry_run, feature, fit,
    /// gauge, interpretation, label, measure, points, run, schema, unit, value}`. `value` is the
    /// value in metres as `0x` + its bits; `fit` holds the algorithm ID of the fit that
    /// produced the feature and the digest of its canonical encoding.
    pub fn to_cvalue(&self) -> CValue {
        obj([
            ("branch", s(self.branch.as_str())),
            (
                "chain",
                obj([
                    ("chain_digest", s(self.chain_digest.as_str())),
                    ("dmt_digest", s(self.dmt_digest.as_str())),
                    ("row", s(&self.row)),
                    ("variant", s(&self.chain_variant)),
                ]),
            ),
            ("char", s(self.char.as_str())),
            ("command", s(self.command)),
            ("dry_run", CValue::Bool(self.dry_run)),
            ("feature", s(&self.feature)),
            (
                "fit",
                obj([("algorithm", s(self.value.algorithm())), ("output_digest", s(self.value.fit_digest().as_str()))]),
            ),
            ("gauge", gauge_cvalue(self.gauge)),
            ("interpretation", s(self.interpretation)),
            ("label", s(self.label)),
            ("measure", s(self.value.measure().as_str())),
            ("points", CValue::Num(self.value.points() as f64)),
            ("run", s(self.run.as_str())),
            ("schema", s(RESULT_SCHEMA)),
            ("unit", s(UNIT)),
            ("value", hex(self.value.value())),
        ])
    }

    /// The result as RFC 8785 canonical JSON bytes.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, EvalError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| EvalError::Encode)
    }
}

/// The inline characteristic of the chain resolution: type `diameter` or `radius`, no
/// modifier, the requested variant as override.
fn chain_characteristic(
    char: &CharId,
    measure: Measure,
    variant: Option<&str>,
) -> Result<CharacteristicRef, EvalError> {
    let doc = obj([
        ("char", s(char.as_str())),
        ("characteristic", s(measure.as_str())),
        ("modifiers", CValue::Arr(Vec::new())),
        ("override", variant.map_or(CValue::Null, s)),
        ("schema", s(CHAR_SCHEMA)),
    ]);
    let bytes = fid_canon::to_canonical_bytes(&doc).map_err(|_| EvalError::Encode)?;
    CharacteristicRef::parse(&bytes).map_err(EvalError::Chain)
}

/// `check.eval_diameter@1` (`measure` diameter) or `check.eval_radius@1` (radius); W2, dry
/// run R1: the pipeline of the module documentation. `store` is read only (the duplicate
/// check); the record is committed by [`commit`] once the event is journaled.
pub fn eval_radial(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    measure: Measure,
    req: &EvalRequest<'_>,
    dry_run: bool,
) -> Result<Evaluated, EvalError> {
    eval_for(store, inp, principal, measure, req, dry_run, Purpose::Evaluate)
}

/// [`eval_radial`] for an evaluation needing a register verified for `purpose` (replay passes
/// [`Purpose::Replay`] with the pinned register).
pub(crate) fn eval_for(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    measure: Measure,
    req: &EvalRequest<'_>,
    dry_run: bool,
    purpose: Purpose,
) -> Result<Evaluated, EvalError> {
    // 1. The request.
    let run = RunId::parse(req.run)?;
    let char = CharId::parse(req.char).ok_or(EvalError::BadParams { reason: "bad_char_id" })?;
    if !is_id(req.feature) {
        return Err(EvalError::BadParams { reason: "bad_feature_id" });
    }
    if req.variant.is_some_and(|v| !is_id(v)) {
        return Err(EvalError::BadParams { reason: "bad_variant" });
    }
    // 2. The measured feature.
    let m = inp
        .features
        .map
        .get(req.feature)
        .ok_or_else(|| EvalError::MeasurementMissing { feature: echo(req.feature) })?;

    // 3. The interpretation chain, behind the governing-spec gate.
    let ch = chain_characteristic(&char, measure, req.variant)?;
    let resolution = resolve_chain(inp.spec, inp.dprof, principal, &ch, inp.table).map_err(EvalError::Chain)?;
    let res = match resolution {
        Resolution::Resolved(r) => r,
        other => return Err(EvalError::ChainNotResolved { outcome: other.outcome() }),
    };
    let table = inp.table.ok_or(EvalError::Chain(CommandError::DmtNotLoaded))?;
    let row = table.get(res.row().as_str()).ok_or(EvalError::ChainNotResolved { outcome: "UNRESOLVED" })?;
    if row.characteristic() != measure.as_str() || !row.modifiers().is_empty() {
        return Err(EvalError::ChainNotResolved { outcome: "UNRESOLVED" });
    }
    let association: Vec<&AlgId> =
        res.chain().steps().iter().filter(|st| st.kind() == OperatorKind::Association).map(|st| st.alg()).collect();

    // 4. The value.
    let input = RadialInput { measure, feature_fit: &m.fit, association: &association, xyz: &m.xyz, gauge: req.gauge };
    let value = evaluate(&input).map_err(EvalError::Radial)?;

    // 5. One record per (run, characteristic).
    match store.get(principal, &run, &char) {
        Ok(_) => return Err(EvalError::Record(RecordError::DuplicateResult)),
        Err(RecordError::NotFound) => {}
        Err(e) => return Err(EvalError::Record(e)),
    }

    // 6. The record, checked in full before anything is returned.
    let spec_err = |e: SpecError| EvalError::Chain(CommandError::Spec(e));
    let gs = inp.spec.require_governing_spec(principal, char.as_str(), inp.dprof).map_err(spec_err)?;
    let stored =
        inp.dprof.resolve(Branch::for_principal(principal), gs.defaults_profile()).map_err(|e| spec_err(e.into()))?;
    let measurand = Measurand::parse(&format!("{} of {}", measure.as_str(), req.feature))?;
    let builder = ResultRecordBuilder::new(run.clone(), char.clone())
        .measurand(measurand)
        .evaluation_basis(EvaluationBasis::Points)
        .value(Quantity::new(value.value(), UNIT)?)
        .uncertainty(Uncertainty::uec0(Uec0Reason::UMissing))
        .bind_decision(None, None)
        .spec(gs.clone())
        .algorithm(AlgorithmRef::parse(value.algorithm())?)
        .kernel_version(KernelVersion::parse(env!("CARGO_PKG_VERSION"))?)
        .input_digests(InputDigests::new(vec![m.digest])?)
        .resolved(&res)
        .interpretation_for(&[row], Some(stored.doc()), inp.register, purpose);
    let trial = builder.clone().journal_entry(Digest::TOMBSTONE).finish()?;
    let pin = *trial.interpretation_pin();
    let interpretation = trial.interpretation().as_str();
    let label = if crate::requires_provisional_label(&trial) { PROVISIONAL_LABEL_TEXT } else { SIGNED_OFF_LINE };

    let mut ev = Evaluated {
        command: command_of(measure),
        principal,
        branch: Branch::for_principal(principal),
        run,
        char,
        feature: req.feature.to_owned(),
        variant: req.variant.map(str::to_owned),
        gauge: req.gauge,
        value,
        points_digest: m.digest,
        row: res.row().as_str().to_owned(),
        chain_variant: res.variant().to_owned(),
        chain_digest: res.chain_digest(),
        dmt_digest: res.dmt_digest(),
        pin,
        interpretation,
        label,
        dry_run,
        builder,
        event: None,
    };
    if !dry_run {
        ev.event = Some(RadialEvent::of(&ev));
    }
    Ok(ev)
}

/// Commits the record of a non-dry-run evaluation, naming `journal_entry` (the hash of the
/// journal entry that holds its event), on `principal`'s branch, and returns its digest
/// (`duplicate_result`, `store_full`, and the builder's refusals pass through).
pub fn commit(
    store: &mut ResultStore,
    principal: Principal,
    ev: &Evaluated,
    journal_entry: Digest,
) -> Result<Digest, EvalError> {
    if principal != ev.principal {
        return Err(EvalError::BadParams { reason: "principal_mismatch" });
    }
    let record = ev.record(journal_entry)?;
    Ok(store.commit(principal, record)?)
}

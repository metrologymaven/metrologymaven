//! Rigid point-pair alignment command `align.run_point_pairs@1` (W2, D0; spec node
//! `ALN.XFM.PAIRS`, `REQ-001`..`REQ-006`, `AC-1`..`AC-6`).

use crate::command::{ID_PATTERN, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH};
use crate::element::{
    id_value, obj, parse_id, Alignment, AlignmentStatus, ElementRef, ParamSet, ParentRef, PrincipalKind, Provenance,
};
use crate::error::AlignError;
use crate::model::{AlignmentModel, MAX_ALIGNMENTS};
use crate::residuals::ResidualStore;
use fid_align_solve::constr::{
    validate as validate_constraints, ConstrError, Dof, DofLocks, FrameContext, LockFrameRef, WeightSpec,
};
use fid_align_solve::pairs::{point_pairs_rigid, PairSolution, PairsError, PAIRS_ALGORITHM_ID};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_dep::{EdgeError, EdgeIndex, EdgesSet};
use fid_geo_frame::{FrameTree, Rigid, Transform};
use fid_kci::AlgorithmId;
use fid_project::IdService;
use fid_registry::{CommandDef, DefError};
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Registry command ID and journal event type of `align.run_point_pairs@1`.
pub const RUN_POINT_PAIRS: &str = "align.run_point_pairs@1";
/// CLI subcommand name (`fid align run-point-pairs`).
pub const CLI_RUN_POINT_PAIRS: &str = "run-point-pairs";
/// MCP tool name (`align_run_point_pairs`).
pub const MCP_RUN_POINT_PAIRS: &str = "align_run_point_pairs";
/// Maximum inline point pairs accepted in one call.
pub const MAX_PAIRS: usize = 100_000;
const MAX_CLI_ARGS: usize = 32;
/// Largest serialised event accepted by [`RunPointPairsEvent::from_canonical_bytes`].
pub const MAX_PAIRS_EVENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_EVENT_DEPTH: usize = 12;
const LIFECYCLE_SINCE: &str = "2026-11-23";
const REPORT_NS: Uuid =
    Uuid::from_bytes([0x7b, 0x11, 0x44, 0x92, 0x20, 0x3e, 0x4d, 0x12, 0x9b, 0x55, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]);

/// One measured/reference point pair with optional non-negative weight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointPairInput {
    pub measured: [f64; 3],
    pub reference: [f64; 3],
    pub weight: Option<f64>,
}

/// Parameters of `align.run_point_pairs@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct RunPointPairsParams {
    pub pairs: Vec<PointPairInput>,
    pub dof_locks: Vec<Dof>,
    pub lock_frame: Option<String>,
    pub start_pose: Option<Rigid>,
    pub parent: Option<Uuid>,
    pub principal: String,
    pub principal_kind: PrincipalKind,
}

/// Errors of `align.run_point_pairs@1`. [`RunPointPairsError::code`] is stable.
#[derive(Debug, Clone, PartialEq)]
pub enum RunPointPairsError {
    Pairs(PairsError),
    Constraint(ConstrError),
    BadParams(&'static str),
    Align(AlignError),
    Edge(EdgeError),
}

impl RunPointPairsError {
    pub fn code(&self) -> &str {
        match self {
            RunPointPairsError::Pairs(e) => e.code(),
            RunPointPairsError::Constraint(e) => e.code(),
            RunPointPairsError::BadParams(_) => "bad_params",
            RunPointPairsError::Align(e) => e.code(),
            RunPointPairsError::Edge(e) => e.code(),
        }
    }

    pub fn to_value(&self) -> CValue {
        obj([("code", CValue::Str(self.code().to_owned())), ("message", CValue::Str(self.to_string()))])
    }
}

impl fmt::Display for RunPointPairsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunPointPairsError::Pairs(e) => e.fmt(f),
            RunPointPairsError::Constraint(e) => e.fmt(f),
            RunPointPairsError::BadParams(s) => write!(f, "bad_params{{{s}}}"),
            RunPointPairsError::Align(e) => e.fmt(f),
            RunPointPairsError::Edge(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for RunPointPairsError {}

impl From<PairsError> for RunPointPairsError {
    fn from(e: PairsError) -> Self {
        RunPointPairsError::Pairs(e)
    }
}

impl From<ConstrError> for RunPointPairsError {
    fn from(e: ConstrError) -> Self {
        RunPointPairsError::Constraint(e)
    }
}

impl From<AlignError> for RunPointPairsError {
    fn from(e: AlignError) -> Self {
        RunPointPairsError::Align(e)
    }
}

/// Committed journal event for `align.run_point_pairs@1` (`AC-6`).
#[derive(Debug, Clone, PartialEq)]
pub struct RunPointPairsEvent {
    pub id: Uuid,
    pub parent: Option<Uuid>,
    pub principal: String,
    pub principal_kind: PrincipalKind,
    pub pairs: Vec<PointPairInput>,
    pub dof_locks: Vec<Dof>,
    pub lock_frame: Option<String>,
    pub alignment_digest: Digest,
    pub report_digest: Digest,
    pub weighted_rms_bits: u64,
    pub max_len_bits: u64,
}

impl RunPointPairsEvent {
    pub fn to_value(&self) -> CValue {
        let pairs_arr = CValue::Arr(
            self.pairs
                .iter()
                .map(|p| {
                    obj([
                        ("measured", vec3_hex(p.measured)),
                        ("reference", vec3_hex(p.reference)),
                        ("weight", p.weight.map_or(CValue::Null, |w| CValue::Str(hex(w)))),
                    ])
                })
                .collect(),
        );
        let locks_arr = CValue::Arr(self.dof_locks.iter().map(|d| CValue::Str(dof_str(*d).to_owned())).collect());
        obj([
            ("alignment_digest", CValue::Str(self.alignment_digest.as_str().to_owned())),
            ("command", CValue::Str(RUN_POINT_PAIRS.to_owned())),
            ("dof_locks", locks_arr),
            ("id", id_value(&self.id)),
            ("lock_frame", self.lock_frame.as_ref().map_or(CValue::Null, |s| CValue::Str(s.clone()))),
            ("max_len_m", CValue::Str(format!("0x{:016x}", self.max_len_bits))),
            ("pairs", pairs_arr),
            ("parent", self.parent.as_ref().map_or(CValue::Null, id_value)),
            ("principal", CValue::Str(self.principal.clone())),
            ("principal_kind", CValue::Str(self.principal_kind.as_str().to_owned())),
            ("report_digest", CValue::Str(self.report_digest.as_str().to_owned())),
            ("weighted_rms_m", CValue::Str(format!("0x{:016x}", self.weighted_rms_bits))),
        ])
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, RunPointPairsError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| RunPointPairsError::BadParams("canonical_encode_failed"))
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<RunPointPairsEvent, RunPointPairsError> {
        let v = parse_with_limits(bytes, MAX_PAIRS_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|e| match e {
            CanonError::InputLimit => RunPointPairsError::BadParams("event too large or deep"),
            _ => RunPointPairsError::BadParams("malformed event"),
        })?;
        let CValue::Obj(o) = &v else { return Err(RunPointPairsError::BadParams("event not object")) };
        if req_str(o, "command")? != RUN_POINT_PAIRS {
            return Err(RunPointPairsError::BadParams("wrong command"));
        }
        let id = parse_id(req_str(o, "id")?).ok_or(RunPointPairsError::BadParams("bad id"))?;
        let parent = match o.get("parent") {
            Some(CValue::Null) => None,
            Some(CValue::Str(s)) => Some(parse_id(s).ok_or(RunPointPairsError::BadParams("bad parent"))?),
            _ => return Err(RunPointPairsError::BadParams("bad parent")),
        };
        let principal = req_str(o, "principal")?.to_owned();
        let principal_kind = PrincipalKind::parse(req_str(o, "principal_kind")?)?;
        let lock_frame = opt_str(o, "lock_frame")?.map(str::to_owned);
        let alignment_digest = Digest::parse(req_str(o, "alignment_digest")?)
            .map_err(|_| RunPointPairsError::BadParams("bad alignment_digest"))?;
        let report_digest = Digest::parse(req_str(o, "report_digest")?)
            .map_err(|_| RunPointPairsError::BadParams("bad report_digest"))?;
        let weighted_rms_bits = parse_hex_bits(req_str(o, "weighted_rms_m")?)?;
        let max_len_bits = parse_hex_bits(req_str(o, "max_len_m")?)?;
        let Some(CValue::Arr(raw_locks)) = o.get("dof_locks") else {
            return Err(RunPointPairsError::BadParams("bad dof_locks"));
        };
        let mut dof_locks = Vec::with_capacity(raw_locks.len());
        for l in raw_locks {
            let CValue::Str(ls) = l else { return Err(RunPointPairsError::BadParams("bad dof_lock")) };
            dof_locks.push(parse_dof(ls).ok_or(RunPointPairsError::BadParams("unknown dof"))?);
        }
        let Some(CValue::Arr(raw_pairs)) = o.get("pairs") else {
            return Err(RunPointPairsError::BadParams("bad pairs"));
        };
        let mut pairs = Vec::with_capacity(raw_pairs.len());
        for p in raw_pairs {
            let CValue::Obj(po) = p else { return Err(RunPointPairsError::BadParams("bad pair")) };
            let measured = parse_hex_vec3(po.get("measured"))?;
            let reference = parse_hex_vec3(po.get("reference"))?;
            let weight = match po.get("weight") {
                Some(CValue::Null) => None,
                Some(CValue::Str(ws)) => Some(f64::from_bits(parse_hex_bits(ws)?)),
                _ => return Err(RunPointPairsError::BadParams("bad weight")),
            };
            pairs.push(PointPairInput { measured, reference, weight });
        }
        Ok(RunPointPairsEvent {
            id,
            parent,
            principal,
            principal_kind,
            pairs,
            dof_locks,
            lock_frame,
            alignment_digest,
            report_digest,
            weighted_rms_bits,
            max_len_bits,
        })
    }
}

/// Output of committing `align.run_point_pairs@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct RunPointPairsCommit {
    pub alignment: Alignment,
    pub edges: EdgesSet,
    pub event: RunPointPairsEvent,
    pub solution: PairSolution,
}

impl RunPointPairsCommit {
    pub fn to_value(&self) -> CValue {
        let r = self.solution.transform.r();
        let t = self.solution.transform.t();
        let row = |x: [f64; 3]| CValue::Arr(vec![CValue::F64Bits(x[0]), CValue::F64Bits(x[1]), CValue::F64Bits(x[2])]);
        let res_arr = CValue::Arr(
            self.solution
                .residuals
                .iter()
                .map(|pr| {
                    obj([
                        ("index", CValue::Num(pr.index as f64)),
                        ("length_m", CValue::Str(hex(pr.length_m))),
                        ("vector_m", vec3_hex(pr.vector_m)),
                        ("weight", CValue::Str(hex(pr.weight))),
                    ])
                })
                .collect(),
        );
        obj([
            ("alignment_digest", CValue::Str(self.alignment.digest().as_str().to_owned())),
            ("command", CValue::Str(RUN_POINT_PAIRS.to_owned())),
            ("id", id_value(&self.alignment.id())),
            ("iterations", CValue::Num(self.solution.iterations as f64)),
            ("max_len_m", CValue::Str(hex(self.solution.max_len_m))),
            ("report_digest", CValue::Str(self.solution.report.digest().as_str().to_owned())),
            ("residuals", res_arr),
            (
                "transform",
                obj([
                    ("kind", CValue::Str("rigid".to_owned())),
                    ("r", CValue::Arr(vec![row(r[0]), row(r[1]), row(r[2])])),
                    ("t", row(t)),
                ]),
            ),
            ("weighted_rms_m", CValue::Str(hex(self.solution.weighted_rms_m))),
        ])
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, RunPointPairsError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| RunPointPairsError::BadParams("canonical_encode_failed"))
    }
}

/// Executes `align.run_point_pairs@1` atomically against `model`, `edges`, `reports`, and `tree`.
/// On any error nothing is mutated and no UUID is drawn.
pub fn run_point_pairs(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    tree: &FrameTree,
    ids: &mut IdService<'_>,
    params: &RunPointPairsParams,
) -> Result<RunPointPairsCommit, RunPointPairsError> {
    let prep = prepare(model, edges, tree, params)?;
    let id = ids.new_v7().map_err(|e| RunPointPairsError::Align(AlignError::Id(e)))?;
    commit_prepared(model, edges, reports, id, params, prep)
}

/// Replays a committed `RunPointPairsEvent` in exact mode (`AC-6`).
pub fn replay_point_pairs(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    tree: &FrameTree,
    raw_event: &[u8],
) -> Result<RunPointPairsCommit, RunPointPairsError> {
    let recorded = RunPointPairsEvent::from_canonical_bytes(raw_event)?;
    let params = RunPointPairsParams {
        pairs: recorded.pairs.clone(),
        dof_locks: recorded.dof_locks.clone(),
        lock_frame: recorded.lock_frame.clone(),
        start_pose: None,
        parent: recorded.parent,
        principal: recorded.principal.clone(),
        principal_kind: recorded.principal_kind,
    };
    let prep = prepare(model, edges, tree, &params)?;
    let committed = commit_prepared(model, edges, reports, recorded.id, &params, prep)?;
    let rebuilt = committed.event.to_canonical_bytes()?;
    if rebuilt != raw_event || committed.alignment.digest() != recorded.alignment_digest {
        return Err(RunPointPairsError::BadParams("replay_divergent"));
    }
    Ok(committed)
}

struct Prepared {
    provenance: Provenance,
    parent_ref: Option<ParentRef>,
    solution: PairSolution,
}

fn prepare(
    model: &AlignmentModel,
    edges: &EdgeIndex,
    tree: &FrameTree,
    params: &RunPointPairsParams,
) -> Result<Prepared, RunPointPairsError> {
    if model.len() >= MAX_ALIGNMENTS {
        return Err(RunPointPairsError::Align(AlignError::InputLimit("alignments")));
    }
    if params.pairs.len() > MAX_PAIRS {
        return Err(RunPointPairsError::BadParams("too many pairs"));
    }
    let provenance = Provenance::for_creation(&params.principal, params.principal_kind)?;

    let parent_ref = match params.parent {
        None => None,
        Some(pid) => {
            let p = model.get(&pid).ok_or(RunPointPairsError::Align(AlignError::NotFound(pid)))?;
            if !edges.contains(&pid) {
                return Err(RunPointPairsError::Align(AlignError::NotFound(pid)));
            }
            if p.status() != AlignmentStatus::Ok {
                return Err(RunPointPairsError::Align(AlignError::ParentUnusable(pid)));
            }
            let next_depth = crate::hier::chain_depth(model, pid)
                .map_err(|_| RunPointPairsError::Align(AlignError::Internal("parent hierarchy invalid")))?
                + 1;
            if next_depth > crate::hier::MAX_HIERARCHY_DEPTH {
                return Err(RunPointPairsError::Align(AlignError::HierarchyTooDeep(next_depth)));
            }
            Some(ParentRef { id: pid, digest: p.digest() })
        }
    };

    let mut measured = Vec::with_capacity(params.pairs.len());
    let mut reference = Vec::with_capacity(params.pairs.len());
    let mut weights = Vec::with_capacity(params.pairs.len());
    let mut any_weight = false;
    for p in &params.pairs {
        measured.push(p.measured);
        reference.push(p.reference);
        if let Some(w) = p.weight {
            any_weight = true;
            weights.push(w);
        } else {
            weights.push(1.0);
        }
    }
    let weight_slice = if any_weight { Some(weights.as_slice()) } else { None };

    let solution = if params.dof_locks.is_empty() {
        point_pairs_rigid(&measured, &reference, weight_slice, None)?
    } else {
        let mut bits = 0u8;
        for &d in &params.dof_locks {
            bits |= dof_bit(d);
        }
        let locks = DofLocks::from_bits(bits)?;
        let ctx = FrameContext { tree, parent: tree.world() };
        let lframe = match params.lock_frame.as_deref() {
            None => LockFrameRef::Parent,
            Some(name) => LockFrameRef::Name(name),
        };
        let start = params.start_pose.unwrap_or(Rigid::IDENTITY);
        let wspec = WeightSpec { per_point: None, per_region: Vec::new() };
        let cs = validate_constraints(locks, lframe, &ctx, &start, &wspec, measured.len())?;
        point_pairs_rigid(&measured, &reference, weight_slice, Some(&cs))?
    };

    Ok(Prepared { provenance, parent_ref, solution })
}

fn commit_prepared(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    id: Uuid,
    params: &RunPointPairsParams,
    prep: Prepared,
) -> Result<RunPointPairsCommit, RunPointPairsError> {
    if parse_id(&id.to_string()).is_none() || model.get(&id).is_some() || edges.contains(&id) {
        return Err(RunPointPairsError::Align(AlignError::IdCollision(id)));
    }
    let alg = AlgorithmId::parse(PAIRS_ALGORITHM_ID)
        .map_err(|_| RunPointPairsError::Align(AlignError::Internal("bad pairs algorithm id")))?;
    let report_id = fid_uuid::uuid_v5(&REPORT_NS, format!("pairs-report:{id}").as_bytes());
    let report_digest = prep.solution.report.digest();
    let report_ref = ElementRef { id: report_id, digest: report_digest };

    let alignment = Alignment::assemble(
        id,
        alg,
        Vec::new(),
        ParamSet::new(),
        Transform::Rigid(prep.solution.transform),
        prep.parent_ref,
        report_ref,
        prep.provenance,
        AlignmentStatus::Ok,
        None,
    )?;

    let mut from: BTreeSet<Uuid> = BTreeSet::new();
    if let Some(p) = &prep.parent_ref {
        from.insert(p.id);
    }
    let set = EdgesSet::from_set(id, &from).map_err(RunPointPairsError::Edge)?;
    edges.apply(std::slice::from_ref(&set)).map_err(RunPointPairsError::Edge)?;
    reports.insert(report_id, prep.solution.report.clone())?;
    model.insert_stored(alignment.clone())?;

    let event = RunPointPairsEvent {
        id,
        parent: params.parent,
        principal: params.principal.clone(),
        principal_kind: params.principal_kind,
        pairs: params.pairs.clone(),
        dof_locks: params.dof_locks.clone(),
        lock_frame: params.lock_frame.clone(),
        alignment_digest: alignment.digest(),
        report_digest,
        weighted_rms_bits: prep.solution.weighted_rms_m.to_bits(),
        max_len_bits: prep.solution.max_len_m.to_bits(),
    };
    Ok(RunPointPairsCommit { alignment, edges: set, event, solution: prep.solution })
}

/// Runs MCP tool `align_run_point_pairs` on raw JSON parameters.
pub fn invoke_point_pairs_mcp(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    tree: &FrameTree,
    ids: &mut IdService<'_>,
    caller: (&str, PrincipalKind),
    raw: &[u8],
) -> Result<RunPointPairsCommit, RunPointPairsError> {
    let v = parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => RunPointPairsError::BadParams("params too large or deep"),
        CanonError::DuplicateKey(_) => RunPointPairsError::BadParams("duplicate key"),
        _ => RunPointPairsError::BadParams("params not valid JSON"),
    })?;
    let params = parse_params_value(&v, caller.0, caller.1)?;
    run_point_pairs(model, edges, reports, tree, ids, &params)
}

/// Runs CLI `fid align run-point-pairs --params-json <JSON> [--json]`.
pub fn invoke_point_pairs_cli(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    tree: &FrameTree,
    ids: &mut IdService<'_>,
    caller: (&str, PrincipalKind),
    args: &[&str],
) -> Result<RunPointPairsCommit, RunPointPairsError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(RunPointPairsError::BadParams("too many arguments"));
    }
    let (sub, rest) = args.split_first().ok_or(RunPointPairsError::BadParams("missing subcommand"))?;
    if *sub != CLI_RUN_POINT_PAIRS {
        return Err(RunPointPairsError::BadParams("unknown subcommand"));
    }
    let mut raw_json: Option<&str> = None;
    let mut json = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        match *arg {
            "--json" if !json => json = true,
            "--json" => return Err(RunPointPairsError::BadParams("repeated --json")),
            "--params-json" => {
                if raw_json.is_some() {
                    return Err(RunPointPairsError::BadParams("repeated --params-json"));
                }
                raw_json = Some(*it.next().ok_or(RunPointPairsError::BadParams("missing value"))?);
            }
            s if s.starts_with('-') => return Err(RunPointPairsError::BadParams("unknown option")),
            _ => return Err(RunPointPairsError::BadParams("unexpected argument")),
        }
    }
    let raw = raw_json.ok_or(RunPointPairsError::BadParams("missing --params-json"))?;
    invoke_point_pairs_mcp(model, edges, reports, tree, ids, caller, raw.as_bytes())
}

/// Returns the validated [`CommandDef`] for `align.run_point_pairs@1`.
pub fn run_point_pairs_command_def() -> Result<CommandDef, DefError> {
    let text = |s: &str| CValue::Str(s.to_owned());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("dof_locks", obj([("type", text("array"))])),
                ("lock_frame", obj([("type", text("string"))])),
                ("pairs", obj([("type", text("array"))])),
                ("parent", obj([("pattern", text(ID_PATTERN)), ("type", text("string"))])),
            ]),
        ),
        ("required", strs(&["pairs"])),
        ("type", text("object")),
    ]);
    let result_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("alignment_digest", obj([("type", text("string"))])),
                ("command", obj([("enum", strs(&[RUN_POINT_PAIRS])), ("type", text("string"))])),
                ("id", obj([("pattern", text(ID_PATTERN)), ("type", text("string"))])),
                ("iterations", obj([("type", text("integer"))])),
                ("max_len_m", obj([("pattern", text("^0x[0-9a-f]{16}$")), ("type", text("string"))])),
                ("report_digest", obj([("type", text("string"))])),
                ("residuals", obj([("type", text("array"))])),
                ("transform", obj([("type", text("object"))])),
                ("weighted_rms_m", obj([("pattern", text("^0x[0-9a-f]{16}$")), ("type", text("string"))])),
            ]),
        ),
        (
            "required",
            strs(&[
                "alignment_digest",
                "command",
                "id",
                "iterations",
                "max_len_m",
                "report_digest",
                "residuals",
                "transform",
                "weighted_rms_m",
            ]),
        ),
        ("type", text("object")),
    ]);
    CommandDef::from_value(obj([
        ("algorithms", strs(&[PAIRS_ALGORITHM_ID])),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            text("Solves for the rigid transform mapping measured points onto reference points in weighted least squares, with optional per-axis DoF locks."),
        ),
        ("determinism", text("D0")),
        ("effects", strs(&[RUN_POINT_PAIRS])),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text("align.run_point_pairs")),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", text("W2")),
        ("title", text("Solve rigid point-pair alignment")),
        ("undo", text("head_move")),
        ("web", text("available")),
    ]))
}

// ------------------------------------------------------------------ helpers

fn parse_params_value(
    v: &CValue,
    principal: &str,
    principal_kind: PrincipalKind,
) -> Result<RunPointPairsParams, RunPointPairsError> {
    let CValue::Obj(o) = v else { return Err(RunPointPairsError::BadParams("params not object")) };
    for k in o.keys() {
        if !matches!(k.as_str(), "pairs" | "dof_locks" | "lock_frame" | "parent") {
            return Err(RunPointPairsError::BadParams("unknown parameter"));
        }
    }
    let Some(CValue::Arr(raw_pairs)) = o.get("pairs") else {
        return Err(RunPointPairsError::BadParams("missing pairs"));
    };
    let mut pairs = Vec::with_capacity(raw_pairs.len());
    for item in raw_pairs {
        let CValue::Obj(po) = item else { return Err(RunPointPairsError::BadParams("pair not object")) };
        let measured = parse_num_vec3(po.get("measured"))?;
        let reference = parse_num_vec3(po.get("reference"))?;
        let weight = match po.get("weight") {
            None | Some(CValue::Null) => None,
            Some(CValue::Num(n)) => Some(*n),
            _ => return Err(RunPointPairsError::BadParams("weight not number")),
        };
        pairs.push(PointPairInput { measured, reference, weight });
    }
    let mut dof_locks = Vec::new();
    if let Some(lv) = o.get("dof_locks") {
        let CValue::Arr(arr) = lv else { return Err(RunPointPairsError::BadParams("dof_locks not array")) };
        for d in arr {
            let CValue::Str(ds) = d else { return Err(RunPointPairsError::BadParams("dof_lock not string")) };
            dof_locks.push(parse_dof(ds).ok_or(RunPointPairsError::BadParams("unknown dof"))?);
        }
    }
    let lock_frame = opt_str(o, "lock_frame")?.map(str::to_owned);
    let parent = match opt_str(o, "parent")? {
        None => None,
        Some(s) => Some(parse_id(s).ok_or(RunPointPairsError::BadParams("bad parent id"))?),
    };
    Ok(RunPointPairsParams {
        pairs,
        dof_locks,
        lock_frame,
        start_pose: None,
        parent,
        principal: principal.to_owned(),
        principal_kind,
    })
}

fn parse_num_vec3(v: Option<&CValue>) -> Result<[f64; 3], RunPointPairsError> {
    let Some(CValue::Arr(a)) = v else { return Err(RunPointPairsError::BadParams("vec3 not array")) };
    if a.len() != 3 {
        return Err(RunPointPairsError::BadParams("vec3 length != 3"));
    }
    let mut out = [0.0; 3];
    for (i, item) in a.iter().enumerate() {
        match item {
            CValue::Num(n) => out[i] = *n,
            _ => return Err(RunPointPairsError::BadParams("vec3 entry not number")),
        }
    }
    Ok(out)
}

fn parse_hex_vec3(v: Option<&CValue>) -> Result<[f64; 3], RunPointPairsError> {
    let Some(CValue::Arr(a)) = v else { return Err(RunPointPairsError::BadParams("hex vec3 not array")) };
    if a.len() != 3 {
        return Err(RunPointPairsError::BadParams("hex vec3 length != 3"));
    }
    let mut out = [0.0; 3];
    for (i, item) in a.iter().enumerate() {
        let CValue::Str(s) = item else { return Err(RunPointPairsError::BadParams("hex entry not string")) };
        out[i] = f64::from_bits(parse_hex_bits(s)?);
    }
    Ok(out)
}

fn hex(v: f64) -> String {
    format!("0x{:016x}", v.to_bits())
}

fn vec3_hex(v: [f64; 3]) -> CValue {
    CValue::Arr(vec![CValue::Str(hex(v[0])), CValue::Str(hex(v[1])), CValue::Str(hex(v[2]))])
}

fn parse_hex_bits(s: &str) -> Result<u64, RunPointPairsError> {
    let rest = s.strip_prefix("0x").ok_or(RunPointPairsError::BadParams("bad hex"))?;
    if rest.len() != 16 {
        return Err(RunPointPairsError::BadParams("bad hex len"));
    }
    u64::from_str_radix(rest, 16).map_err(|_| RunPointPairsError::BadParams("bad hex digits"))
}

fn req_str<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, RunPointPairsError> {
    match o.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(RunPointPairsError::BadParams("missing string field")),
    }
}

fn opt_str<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<Option<&'a str>, RunPointPairsError> {
    match o.get(k) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.as_str())),
        _ => Err(RunPointPairsError::BadParams("non-string field")),
    }
}

fn dof_str(d: Dof) -> &'static str {
    match d {
        Dof::Tx => "tx",
        Dof::Ty => "ty",
        Dof::Tz => "tz",
        Dof::Rx => "rx",
        Dof::Ry => "ry",
        Dof::Rz => "rz",
    }
}

fn parse_dof(s: &str) -> Option<Dof> {
    match s {
        "tx" => Some(Dof::Tx),
        "ty" => Some(Dof::Ty),
        "tz" => Some(Dof::Tz),
        "rx" => Some(Dof::Rx),
        "ry" => Some(Dof::Ry),
        "rz" => Some(Dof::Rz),
        _ => None,
    }
}

fn dof_bit(d: Dof) -> u8 {
    match d {
        Dof::Tx => 1 << 0,
        Dof::Ty => 1 << 1,
        Dof::Tz => 1 << 2,
        Dof::Rx => 1 << 3,
        Dof::Ry => 1 << 4,
        Dof::Rz => 1 << 5,
    }
}

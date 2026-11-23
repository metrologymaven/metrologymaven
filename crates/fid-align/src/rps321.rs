//! Classic 3-2-1 alignment command `align.run_321@1` (W2, D0; spec node `ALN.RPS.CLASSIC`,
//! `REQ-001`..`REQ-007`, `AC-1`..`AC-6`).

use crate::command::{ID_PATTERN, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH};
use crate::element::{
    id_value, obj, parse_id, Alignment, AlignmentStatus, ElementRef, ParamSet, ParentRef, PrincipalKind, Provenance,
};
use crate::error::AlignError;
use crate::model::{AlignmentModel, MAX_ALIGNMENTS};
use crate::residuals::ResidualStore;
use fid_align_solve::rps321::{
    solve_321_elements, solve_321_three_points, Rps321Error, Rps321Solution, Rps321Tolerances, RpsElement, SignedAxis,
    RPS321_ALGORITHM_ID,
};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_dep::{EdgeError, EdgeIndex, EdgesSet};
use fid_geo_frame::Transform;
use fid_kci::AlgorithmId;
use fid_project::IdService;
use fid_registry::{CommandDef, DefError};
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Registry command ID and journal event type of `align.run_321@1`.
pub const RUN_321: &str = "align.run_321@1";
/// CLI subcommand name (`fid align run-321`).
pub const CLI_RUN_321: &str = "run-321";
/// MCP tool name (`align_run_321`).
pub const MCP_RUN_321: &str = "align_run_321";
/// Maximum stored elements in [`Rps321Elements`].
pub const MAX_RPS_ELEMENTS: usize = 4096;
const MAX_CLI_ARGS: usize = 32;
pub const MAX_321_EVENT_BYTES: usize = 64 * 1024;
const MAX_EVENT_DEPTH: usize = 12;
const LIFECYCLE_SINCE: &str = "2026-11-23";
const REPORT_NS: Uuid =
    Uuid::from_bytes([0x8c, 0x22, 0x55, 0xa3, 0x30, 0x3e, 0x4d, 0x12, 0x9b, 0x55, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);

/// Named element store passed to `align.run_321@1` in `elements` mode.
#[derive(Debug, Clone, Default)]
pub struct Rps321Elements {
    map: BTreeMap<String, RpsElement>,
}

impl Rps321Elements {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: &str, element: RpsElement) -> Result<(), Run321Error> {
        if name.is_empty() || name.len() > 128 {
            return Err(Run321Error::BadParams("bad element name"));
        }
        if !self.map.contains_key(name) && self.map.len() >= MAX_RPS_ELEMENTS {
            return Err(Run321Error::BadParams("too many elements"));
        }
        self.map.insert(name.to_owned(), element);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&RpsElement> {
        self.map.get(name)
    }
}

/// Mode of `align.run_321@1`.
#[derive(Debug, Clone, PartialEq)]
pub enum Run321Mode {
    Elements { primary: String, secondary: String, tertiary: String },
    ThreePoints { points: [[f64; 3]; 3] },
}

/// Parameters of `align.run_321@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Run321Params {
    pub mode: Run321Mode,
    pub primary_axis: SignedAxis,
    pub secondary_axis: SignedAxis,
    pub tolerances: Rps321Tolerances,
    pub parent: Option<Uuid>,
    pub principal: String,
    pub principal_kind: PrincipalKind,
}

/// Errors of `align.run_321@1`. [`Run321Error::code`] is stable.
#[derive(Debug, Clone, PartialEq)]
pub enum Run321Error {
    Solver(Rps321Error),
    ElementMissing(String),
    BadParams(&'static str),
    Align(AlignError),
    Edge(EdgeError),
}

impl Run321Error {
    pub fn code(&self) -> &str {
        match self {
            Run321Error::Solver(e) => e.code(),
            Run321Error::ElementMissing(_) => "element_missing",
            Run321Error::BadParams(_) => "bad_params",
            Run321Error::Align(e) => e.code(),
            Run321Error::Edge(e) => e.code(),
        }
    }

    pub fn to_value(&self) -> CValue {
        obj([("code", CValue::Str(self.code().to_owned())), ("message", CValue::Str(self.to_string()))])
    }
}

impl fmt::Display for Run321Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Run321Error::Solver(e) => e.fmt(f),
            Run321Error::ElementMissing(el) => write!(f, "element_missing{{{el}}}"),
            Run321Error::BadParams(s) => write!(f, "bad_params{{{s}}}"),
            Run321Error::Align(e) => e.fmt(f),
            Run321Error::Edge(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Run321Error {}

impl From<Rps321Error> for Run321Error {
    fn from(e: Rps321Error) -> Self {
        Run321Error::Solver(e)
    }
}

impl From<AlignError> for Run321Error {
    fn from(e: AlignError) -> Self {
        Run321Error::Align(e)
    }
}

/// Committed journal event for `align.run_321@1` (`AC-6`).
#[derive(Debug, Clone, PartialEq)]
pub struct Run321Event {
    pub id: Uuid,
    pub parent: Option<Uuid>,
    pub principal: String,
    pub principal_kind: PrincipalKind,
    pub mode: Run321Mode,
    pub primary_axis: SignedAxis,
    pub secondary_axis: SignedAxis,
    pub alignment_digest: Digest,
    pub report_digest: Digest,
}

impl Run321Event {
    pub fn to_value(&self) -> CValue {
        let mode_val = match &self.mode {
            Run321Mode::Elements { primary, secondary, tertiary } => obj([
                ("kind", CValue::Str("elements".to_owned())),
                ("primary", CValue::Str(primary.clone())),
                ("secondary", CValue::Str(secondary.clone())),
                ("tertiary", CValue::Str(tertiary.clone())),
            ]),
            Run321Mode::ThreePoints { points } => obj([
                ("kind", CValue::Str("three_points".to_owned())),
                ("points", CValue::Arr(vec![vec3_hex(points[0]), vec3_hex(points[1]), vec3_hex(points[2])])),
            ]),
        };
        obj([
            ("alignment_digest", CValue::Str(self.alignment_digest.as_str().to_owned())),
            ("command", CValue::Str(RUN_321.to_owned())),
            ("id", id_value(&self.id)),
            ("mode", mode_val),
            ("parent", self.parent.as_ref().map_or(CValue::Null, id_value)),
            ("primary_axis", CValue::Str(self.primary_axis.as_str().to_owned())),
            ("principal", CValue::Str(self.principal.clone())),
            ("principal_kind", CValue::Str(self.principal_kind.as_str().to_owned())),
            ("report_digest", CValue::Str(self.report_digest.as_str().to_owned())),
            ("secondary_axis", CValue::Str(self.secondary_axis.as_str().to_owned())),
        ])
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, Run321Error> {
        to_canonical_bytes(&self.to_value()).map_err(|_| Run321Error::BadParams("canonical_encode_failed"))
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Run321Event, Run321Error> {
        let v = parse_with_limits(bytes, MAX_321_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|e| match e {
            CanonError::InputLimit => Run321Error::BadParams("event too large or deep"),
            _ => Run321Error::BadParams("malformed event"),
        })?;
        let CValue::Obj(o) = &v else { return Err(Run321Error::BadParams("event not object")) };
        if req_str(o, "command")? != RUN_321 {
            return Err(Run321Error::BadParams("wrong command"));
        }
        let id = parse_id(req_str(o, "id")?).ok_or(Run321Error::BadParams("bad id"))?;
        let parent = match o.get("parent") {
            Some(CValue::Null) => None,
            Some(CValue::Str(s)) => Some(parse_id(s).ok_or(Run321Error::BadParams("bad parent"))?),
            _ => return Err(Run321Error::BadParams("bad parent")),
        };
        let principal = req_str(o, "principal")?.to_owned();
        let principal_kind = PrincipalKind::parse(req_str(o, "principal_kind")?)?;
        let primary_axis = SignedAxis::parse(req_str(o, "primary_axis")?)
            .ok_or(Run321Error::Solver(Rps321Error::InvalidAxisAssignment))?;
        let secondary_axis = SignedAxis::parse(req_str(o, "secondary_axis")?)
            .ok_or(Run321Error::Solver(Rps321Error::InvalidAxisAssignment))?;
        let alignment_digest = Digest::parse(req_str(o, "alignment_digest")?)
            .map_err(|_| Run321Error::BadParams("bad alignment_digest"))?;
        let report_digest =
            Digest::parse(req_str(o, "report_digest")?).map_err(|_| Run321Error::BadParams("bad report_digest"))?;
        let Some(CValue::Obj(mo)) = o.get("mode") else {
            return Err(Run321Error::BadParams("bad mode"));
        };
        let mode = match req_str(mo, "kind")? {
            "elements" => Run321Mode::Elements {
                primary: req_str(mo, "primary")?.to_owned(),
                secondary: req_str(mo, "secondary")?.to_owned(),
                tertiary: req_str(mo, "tertiary")?.to_owned(),
            },
            "three_points" => {
                let Some(CValue::Arr(pts)) = mo.get("points") else {
                    return Err(Run321Error::BadParams("bad points"));
                };
                if pts.len() != 3 {
                    return Err(Run321Error::BadParams("need 3 points"));
                }
                Run321Mode::ThreePoints {
                    points: [parse_hex_vec3(&pts[0])?, parse_hex_vec3(&pts[1])?, parse_hex_vec3(&pts[2])?],
                }
            }
            _ => return Err(Run321Error::BadParams("unknown mode")),
        };
        Ok(Run321Event {
            id,
            parent,
            principal,
            principal_kind,
            mode,
            primary_axis,
            secondary_axis,
            alignment_digest,
            report_digest,
        })
    }
}

/// Output of committing `align.run_321@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Run321Commit {
    pub alignment: Alignment,
    pub edges: EdgesSet,
    pub event: Run321Event,
    pub solution: Rps321Solution,
}

impl Run321Commit {
    pub fn to_value(&self) -> CValue {
        let r = self.solution.transform.r();
        let t = self.solution.transform.t();
        let row = |x: [f64; 3]| CValue::Arr(vec![CValue::F64Bits(x[0]), CValue::F64Bits(x[1]), CValue::F64Bits(x[2])]);
        obj([
            ("alignment_digest", CValue::Str(self.alignment.digest().as_str().to_owned())),
            ("command", CValue::Str(RUN_321.to_owned())),
            ("id", id_value(&self.alignment.id())),
            ("origin_world", vec3_hex(self.solution.origin_world)),
            ("primary_axis", CValue::Str(self.event.primary_axis.as_str().to_owned())),
            ("report_digest", CValue::Str(self.event.report_digest.as_str().to_owned())),
            ("secondary_axis", CValue::Str(self.event.secondary_axis.as_str().to_owned())),
            (
                "transform",
                obj([
                    ("kind", CValue::Str("rigid".to_owned())),
                    ("r", CValue::Arr(vec![row(r[0]), row(r[1]), row(r[2])])),
                    ("t", row(t)),
                ]),
            ),
        ])
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, Run321Error> {
        to_canonical_bytes(&self.to_value()).map_err(|_| Run321Error::BadParams("canonical_encode_failed"))
    }
}

/// Executes `align.run_321@1` atomically. Errors mutate nothing and draw no UUID.
pub fn run_321(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    elements: &Rps321Elements,
    ids: &mut IdService<'_>,
    params: &Run321Params,
) -> Result<Run321Commit, Run321Error> {
    let prep = prepare(model, edges, elements, params)?;
    let id = ids.new_v7().map_err(|e| Run321Error::Align(AlignError::Id(e)))?;
    commit_prepared(model, edges, reports, id, params, prep)
}

/// Replays a committed `Run321Event` in exact mode (`AC-6`).
pub fn replay_321(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    elements: &Rps321Elements,
    raw_event: &[u8],
) -> Result<Run321Commit, Run321Error> {
    let recorded = Run321Event::from_canonical_bytes(raw_event)?;
    let params = Run321Params {
        mode: recorded.mode.clone(),
        primary_axis: recorded.primary_axis,
        secondary_axis: recorded.secondary_axis,
        tolerances: Rps321Tolerances::default(),
        parent: recorded.parent,
        principal: recorded.principal.clone(),
        principal_kind: recorded.principal_kind,
    };
    let prep = prepare(model, edges, elements, &params)?;
    let committed = commit_prepared(model, edges, reports, recorded.id, &params, prep)?;
    let rebuilt = committed.event.to_canonical_bytes()?;
    if rebuilt != raw_event || committed.alignment.digest() != recorded.alignment_digest {
        return Err(Run321Error::BadParams("replay_divergent"));
    }
    Ok(committed)
}

struct Prepared {
    provenance: Provenance,
    parent_ref: Option<ParentRef>,
    solution: Rps321Solution,
}

fn prepare(
    model: &AlignmentModel,
    edges: &EdgeIndex,
    elements: &Rps321Elements,
    params: &Run321Params,
) -> Result<Prepared, Run321Error> {
    if model.len() >= MAX_ALIGNMENTS {
        return Err(Run321Error::Align(AlignError::InputLimit("alignments")));
    }
    let provenance = Provenance::for_creation(&params.principal, params.principal_kind)?;
    let parent_ref = match params.parent {
        None => None,
        Some(pid) => {
            let p = model.get(&pid).ok_or(Run321Error::Align(AlignError::NotFound(pid)))?;
            if !edges.contains(&pid) {
                return Err(Run321Error::Align(AlignError::NotFound(pid)));
            }
            if p.status() != AlignmentStatus::Ok {
                return Err(Run321Error::Align(AlignError::ParentUnusable(pid)));
            }
            Some(ParentRef { id: pid, digest: p.digest() })
        }
    };

    let solution = match &params.mode {
        Run321Mode::Elements { primary, secondary, tertiary } => {
            let e1 = elements.get(primary).ok_or_else(|| Run321Error::ElementMissing(primary.clone()))?;
            let e2 = elements.get(secondary).ok_or_else(|| Run321Error::ElementMissing(secondary.clone()))?;
            let e3 = elements.get(tertiary).ok_or_else(|| Run321Error::ElementMissing(tertiary.clone()))?;
            solve_321_elements(e1, e2, e3, params.primary_axis, params.secondary_axis, params.tolerances)?
        }
        Run321Mode::ThreePoints { points } => {
            solve_321_three_points(*points, params.primary_axis, params.secondary_axis, params.tolerances)?
        }
    };
    Ok(Prepared { provenance, parent_ref, solution })
}

fn commit_prepared(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    id: Uuid,
    params: &Run321Params,
    prep: Prepared,
) -> Result<Run321Commit, Run321Error> {
    if parse_id(&id.to_string()).is_none() || model.get(&id).is_some() || edges.contains(&id) {
        return Err(Run321Error::Align(AlignError::IdCollision(id)));
    }
    let alg = AlgorithmId::parse(RPS321_ALGORITHM_ID)
        .map_err(|_| Run321Error::Align(AlignError::Internal("bad 321 algorithm id")))?;
    let report_id = fid_uuid::uuid_v5(&REPORT_NS, format!("321-report:{id}").as_bytes());
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
    let set = EdgesSet::from_set(id, &from).map_err(Run321Error::Edge)?;
    edges.apply(std::slice::from_ref(&set)).map_err(Run321Error::Edge)?;
    reports.insert(report_id, prep.solution.report.clone())?;
    model.insert_stored(alignment.clone())?;

    let event = Run321Event {
        id,
        parent: params.parent,
        principal: params.principal.clone(),
        principal_kind: params.principal_kind,
        mode: params.mode.clone(),
        primary_axis: params.primary_axis,
        secondary_axis: params.secondary_axis,
        alignment_digest: alignment.digest(),
        report_digest,
    };
    Ok(Run321Commit { alignment, edges: set, event, solution: prep.solution })
}

/// Runs MCP tool `align_run_321` on raw JSON parameters.
pub fn invoke_321_mcp(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    elements: &Rps321Elements,
    ids: &mut IdService<'_>,
    caller: (&str, PrincipalKind),
    raw: &[u8],
) -> Result<Run321Commit, Run321Error> {
    let v = parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => Run321Error::BadParams("params too large or deep"),
        CanonError::DuplicateKey(_) => Run321Error::BadParams("duplicate key"),
        _ => Run321Error::BadParams("params not valid JSON"),
    })?;
    let params = parse_params_value(&v, caller.0, caller.1)?;
    run_321(model, edges, reports, elements, ids, &params)
}

/// Runs CLI `fid align run-321 --primary <P> --secondary <S> --tertiary <T> [--primary-axis +Z] [--secondary-axis +X] [--json]`
/// or `fid align run-321 --mode three_points --points x0,y0,z0;x1,y1,z1;x2,y2,z2 [--primary-axis +X] [--secondary-axis +Y] [--json]`.
pub fn invoke_321_cli(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    elements: &Rps321Elements,
    ids: &mut IdService<'_>,
    caller: (&str, PrincipalKind),
    args: &[&str],
) -> Result<Run321Commit, Run321Error> {
    if args.len() > MAX_CLI_ARGS {
        return Err(Run321Error::BadParams("too many arguments"));
    }
    let (sub, rest) = args.split_first().ok_or(Run321Error::BadParams("missing subcommand"))?;
    if *sub != CLI_RUN_321 {
        return Err(Run321Error::BadParams("unknown subcommand"));
    }
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    let mut json = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        match *arg {
            "--json" if !json => json = true,
            "--json" => return Err(Run321Error::BadParams("repeated --json")),
            "--mode" | "--primary" | "--secondary" | "--tertiary" | "--points" | "--primary-axis"
            | "--secondary-axis" | "--parent" => {
                let val = it.next().ok_or(Run321Error::BadParams("option needs value"))?;
                if map.insert((*arg).to_owned(), (*val).to_owned()).is_some() {
                    return Err(Run321Error::BadParams("repeated option"));
                }
            }
            s if s.starts_with('-') => return Err(Run321Error::BadParams("unknown option")),
            _ => return Err(Run321Error::BadParams("unexpected argument")),
        }
    }

    let mode_str = map.get("--mode").map(String::as_str).unwrap_or("elements");
    let (mode, def_p, def_s) = match mode_str {
        "elements" => {
            let primary = map.get("--primary").cloned().ok_or(Run321Error::BadParams("missing --primary"))?;
            let secondary = map.get("--secondary").cloned().ok_or(Run321Error::BadParams("missing --secondary"))?;
            let tertiary = map.get("--tertiary").cloned().ok_or(Run321Error::BadParams("missing --tertiary"))?;
            (Run321Mode::Elements { primary, secondary, tertiary }, SignedAxis::PosZ, SignedAxis::PosX)
        }
        "three_points" => {
            let pts_s = map.get("--points").ok_or(Run321Error::BadParams("missing --points"))?;
            let pts = parse_cli_points3(pts_s)?;
            (Run321Mode::ThreePoints { points: pts }, SignedAxis::PosX, SignedAxis::PosY)
        }
        _ => return Err(Run321Error::BadParams("unknown --mode")),
    };
    let primary_axis = match map.get("--primary-axis") {
        None => def_p,
        Some(s) => SignedAxis::parse(s).ok_or(Run321Error::Solver(Rps321Error::InvalidAxisAssignment))?,
    };
    let secondary_axis = match map.get("--secondary-axis") {
        None => def_s,
        Some(s) => SignedAxis::parse(s).ok_or(Run321Error::Solver(Rps321Error::InvalidAxisAssignment))?,
    };
    let parent = match map.get("--parent") {
        None => None,
        Some(s) => Some(parse_id(s).ok_or(Run321Error::BadParams("bad parent id"))?),
    };
    let params = Run321Params {
        mode,
        primary_axis,
        secondary_axis,
        tolerances: Rps321Tolerances::default(),
        parent,
        principal: caller.0.to_owned(),
        principal_kind: caller.1,
    };
    run_321(model, edges, reports, elements, ids, &params)
}

/// Returns the validated [`CommandDef`] for `align.run_321@1`.
pub fn run_321_command_def() -> Result<CommandDef, DefError> {
    let text = |s: &str| CValue::Str(s.to_owned());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let axes = &["+X", "-X", "+Y", "-Y", "+Z", "-Z"];
    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("mode", obj([("enum", strs(&["elements", "three_points"])), ("type", text("string"))])),
                ("parent", obj([("pattern", text(ID_PATTERN)), ("type", text("string"))])),
                ("points", obj([("type", text("array"))])),
                ("primary", obj([("type", text("string"))])),
                ("primary_axis", obj([("enum", strs(axes)), ("type", text("string"))])),
                ("secondary", obj([("type", text("string"))])),
                ("secondary_axis", obj([("enum", strs(axes)), ("type", text("string"))])),
                ("tertiary", obj([("type", text("string"))])),
            ]),
        ),
        ("required", CValue::Arr(Vec::new())),
        ("type", text("object")),
    ]);
    let result_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("alignment_digest", obj([("type", text("string"))])),
                ("command", obj([("enum", strs(&[RUN_321])), ("type", text("string"))])),
                ("id", obj([("pattern", text(ID_PATTERN)), ("type", text("string"))])),
                ("origin_world", obj([("type", text("array"))])),
                ("primary_axis", obj([("enum", strs(axes)), ("type", text("string"))])),
                ("report_digest", obj([("type", text("string"))])),
                ("secondary_axis", obj([("enum", strs(axes)), ("type", text("string"))])),
                ("transform", obj([("type", text("object"))])),
            ]),
        ),
        (
            "required",
            strs(&[
                "alignment_digest",
                "command",
                "id",
                "origin_world",
                "primary_axis",
                "report_digest",
                "secondary_axis",
                "transform",
            ]),
        ),
        ("type", text("object")),
    ]);
    CommandDef::from_value(obj([
        ("algorithms", strs(&[RPS321_ALGORITHM_ID])),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            text("Computes a closed-form right-handed classic 3-2-1 alignment from primary/secondary/tertiary elements or three points."),
        ),
        ("determinism", text("D0")),
        ("effects", strs(&[RUN_321])),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text("align.run_321")),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", text("W2")),
        ("title", text("Compute classic 3-2-1 or three-point alignment")),
        ("undo", text("head_move")),
        ("web", text("available")),
    ]))
}

// ------------------------------------------------------------------ helpers

fn parse_params_value(v: &CValue, principal: &str, principal_kind: PrincipalKind) -> Result<Run321Params, Run321Error> {
    let CValue::Obj(o) = v else { return Err(Run321Error::BadParams("params not object")) };
    for k in o.keys() {
        if !matches!(
            k.as_str(),
            "mode" | "primary" | "secondary" | "tertiary" | "points" | "primary_axis" | "secondary_axis" | "parent"
        ) {
            return Err(Run321Error::BadParams("unknown parameter"));
        }
    }
    let mode_s = opt_str(o, "mode")?.unwrap_or("elements");
    let (mode, def_p, def_s) = match mode_s {
        "elements" => (
            Run321Mode::Elements {
                primary: req_str(o, "primary")?.to_owned(),
                secondary: req_str(o, "secondary")?.to_owned(),
                tertiary: req_str(o, "tertiary")?.to_owned(),
            },
            SignedAxis::PosZ,
            SignedAxis::PosX,
        ),
        "three_points" => {
            let Some(CValue::Arr(pts)) = o.get("points") else {
                return Err(Run321Error::BadParams("missing points"));
            };
            if pts.len() != 3 {
                return Err(Run321Error::BadParams("points must have length 3"));
            }
            (
                Run321Mode::ThreePoints {
                    points: [parse_num_vec3(&pts[0])?, parse_num_vec3(&pts[1])?, parse_num_vec3(&pts[2])?],
                },
                SignedAxis::PosX,
                SignedAxis::PosY,
            )
        }
        _ => return Err(Run321Error::BadParams("unknown mode")),
    };
    let primary_axis = match opt_str(o, "primary_axis")? {
        None => def_p,
        Some(s) => SignedAxis::parse(s).ok_or(Run321Error::Solver(Rps321Error::InvalidAxisAssignment))?,
    };
    let secondary_axis = match opt_str(o, "secondary_axis")? {
        None => def_s,
        Some(s) => SignedAxis::parse(s).ok_or(Run321Error::Solver(Rps321Error::InvalidAxisAssignment))?,
    };
    let parent = match opt_str(o, "parent")? {
        None => None,
        Some(s) => Some(parse_id(s).ok_or(Run321Error::BadParams("bad parent id"))?),
    };
    Ok(Run321Params {
        mode,
        primary_axis,
        secondary_axis,
        tolerances: Rps321Tolerances::default(),
        parent,
        principal: principal.to_owned(),
        principal_kind,
    })
}

fn parse_cli_points3(s: &str) -> Result<[[f64; 3]; 3], Run321Error> {
    let groups: Vec<&str> = s.split(';').collect();
    if groups.len() != 3 {
        return Err(Run321Error::BadParams("points must be 3 semicolon-separated triples"));
    }
    let mut out = [[0.0_f64; 3]; 3];
    for (i, g) in groups.iter().enumerate() {
        let coords: Vec<&str> = g.split(',').collect();
        if coords.len() != 3 {
            return Err(Run321Error::BadParams("each point must have 3 coords"));
        }
        for (j, c) in coords.iter().enumerate() {
            out[i][j] = c.trim().parse().map_err(|_| Run321Error::BadParams("invalid float"))?;
        }
    }
    Ok(out)
}

fn parse_num_vec3(v: &CValue) -> Result<[f64; 3], Run321Error> {
    let CValue::Arr(a) = v else { return Err(Run321Error::BadParams("point not array")) };
    if a.len() != 3 {
        return Err(Run321Error::BadParams("point length != 3"));
    }
    let mut out = [0.0; 3];
    for (i, item) in a.iter().enumerate() {
        match item {
            CValue::Num(n) => out[i] = *n,
            _ => return Err(Run321Error::BadParams("point coord not number")),
        }
    }
    Ok(out)
}

fn parse_hex_vec3(v: &CValue) -> Result<[f64; 3], Run321Error> {
    let CValue::Arr(a) = v else { return Err(Run321Error::BadParams("hex vec3 not array")) };
    if a.len() != 3 {
        return Err(Run321Error::BadParams("hex vec3 length != 3"));
    }
    let mut out = [0.0; 3];
    for (i, item) in a.iter().enumerate() {
        let CValue::Str(s) = item else { return Err(Run321Error::BadParams("hex coord not string")) };
        let rest = s.strip_prefix("0x").ok_or(Run321Error::BadParams("bad hex"))?;
        if rest.len() != 16 {
            return Err(Run321Error::BadParams("bad hex len"));
        }
        let bits = u64::from_str_radix(rest, 16).map_err(|_| Run321Error::BadParams("bad hex digits"))?;
        out[i] = f64::from_bits(bits);
    }
    Ok(out)
}

fn hex(v: f64) -> String {
    format!("0x{:016x}", v.to_bits())
}

fn vec3_hex(v: [f64; 3]) -> CValue {
    CValue::Arr(vec![CValue::Str(hex(v[0])), CValue::Str(hex(v[1])), CValue::Str(hex(v[2]))])
}

fn req_str<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, Run321Error> {
    match o.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(Run321Error::BadParams("missing string field")),
    }
}

fn opt_str<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<Option<&'a str>, Run321Error> {
    match o.get(k) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.as_str())),
        _ => Err(Run321Error::BadParams("non-string field")),
    }
}

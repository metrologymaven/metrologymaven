//! `geo.describe_element@1` (R0, D0; REQ-GEO.TYPES.REG-004 to -006): what an element is.
//!
//! The command reads the facts cached at registration (it never rescans the element) and
//! returns them as RFC 8785 canonical JSON:
//!
//! ```text
//! {"aabb": {"max": [x, y, z], "min": [x, y, z]} | null,   metres; null with no points
//!  "channels": ["normal", ...],                           per-point channels present
//!  "digest": "sha256:<64 hex>",                           GEO.TYPES.DIGEST content digest
//!  "frame": "<uuid>",                                     the 16 frame-ID bytes
//!  "id": "<uuid>",                                        the entity ID
//!  "kind": "PointSet" | "PointCloud" | "Mesh" | "CadBodyRef",
//!  "points": n | null,                                    vertices for a mesh; null for a CAD body
//!  "source_id": "<text>" | null,                          the imported source ID, verbatim
//!  "topology": {...} | null,                              mesh topology facts
//!  "triangles": n | null}                                 meshes only
//! ```
//!
//! Both surfaces (`fid geo describe-element <id> [--json]` and the MCP tool
//! `geo_describe_element`) build the same parameters and call the same handler, so they
//! return identical bytes. Every call, failed or not, appends exactly one line to the
//! caller's Activity Log; an R0 command never reaches the Record Journal, and the handler
//! only gets read access to the tree, so it cannot change anything.

use crate::error::{kept, GeoCmdError};
use crate::register::{kind_name, ElementFacts, ElementTree, EntityId, TreeEntry};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_geo_types::TopologyFacts;
use fid_registry::{CommandDef, DefError};
use fid_uuid::Uuid;
use std::collections::BTreeMap;

/// Command ID.
pub const DESCRIBE_ELEMENT: &str = "geo.describe_element@1";
/// CLI subcommand of `fid geo`.
pub const CLI_DESCRIBE: &str = "describe-element";
/// MCP tool name.
pub const MCP_DESCRIBE: &str = "geo_describe_element";
/// Largest MCP parameter payload, in bytes.
pub const MAX_PARAMS_BYTES: usize = 4 * 1024;
/// Deepest MCP parameter nesting.
pub const MAX_PARAMS_DEPTH: usize = 4;
/// Most CLI arguments after `fid geo`.
const MAX_CLI_ARGS: usize = 8;
const LIFECYCLE_SINCE: &str = "2026-11-19";
const ID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";

/// The surface a call came through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Surface {
    Cli,
    Mcp,
}

/// One Activity Log line of an R0 call: the command, the surface and the outcome (`ok` or
/// the error code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityEntry {
    pub command: &'static str,
    pub surface: Surface,
    pub outcome: &'static str,
}

/// Parameters of `geo.describe_element@1`: `element`, an entity ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescribeParams {
    pub element: EntityId,
}

/// A canonical entity ID: lowercase hyphenated, not nil, not max.
fn parse_id(text: &str) -> Option<Uuid> {
    let id = Uuid::parse(text).ok()?;
    (id.to_string() == text && id != Uuid::NIL && id != Uuid::MAX).then_some(id)
}

impl DescribeParams {
    /// Reads the parameter object `{"element": "<uuid>"}`. Refuses an unknown key, a
    /// missing element and a malformed ID (`bad_params`).
    pub fn from_value(v: &CValue) -> Result<DescribeParams, GeoCmdError> {
        let CValue::Obj(o) = v else { return Err(GeoCmdError::BadParams("params is not an object")) };
        if o.keys().any(|k| k != "element") {
            return Err(GeoCmdError::BadParams("unknown parameter"));
        }
        match o.get("element") {
            None => Err(GeoCmdError::BadParams("missing element")),
            Some(CValue::Str(s)) => parse_id(s)
                .map(|element| DescribeParams { element })
                .ok_or(GeoCmdError::BadParams("element is not a canonical id")),
            Some(_) => Err(GeoCmdError::BadParams("element is not a string")),
        }
    }
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// A count as a JSON number; facts never hold more than `MAX_COUNT` (2^53), which is exact.
fn num(n: u64) -> CValue {
    CValue::Num(n as f64)
}

fn xyz(p: [f64; 3]) -> CValue {
    CValue::Arr(p.iter().map(|c| CValue::Num(*c)).collect())
}

fn topology_value(t: &TopologyFacts) -> CValue {
    obj([
        ("boundary_edges", num(t.boundary_edges)),
        ("closed", CValue::Bool(t.closed)),
        ("components", num(t.components)),
        ("degenerate", num(t.degenerate)),
        ("misoriented_pairs", num(t.misoriented_pairs)),
        ("nonmanifold_edges", num(t.nonmanifold_edges)),
        ("oriented", CValue::Bool(t.oriented)),
    ])
}

/// The describe result of element `id` with cached `facts`.
fn description(id: &EntityId, facts: &ElementFacts, source_id: Option<&str>) -> CValue {
    let opt = |v: Option<CValue>| v.unwrap_or(CValue::Null);
    obj([
        ("aabb", opt(facts.aabb.map(|b| obj([("max", xyz(b.max())), ("min", xyz(b.min()))])))),
        ("channels", CValue::Arr(facts.channels.iter().map(|c| text(c)).collect())),
        ("digest", text(facts.digest.to_digest().as_str())),
        ("frame", text(&Uuid::from_bytes(facts.frame.to_bytes()).to_string())),
        ("id", text(&id.to_string())),
        ("kind", text(kind_name(facts.kind))),
        ("points", opt(facts.points.map(num))),
        ("source_id", opt(source_id.map(text))),
        ("topology", opt(facts.topology.as_ref().map(topology_value))),
        ("triangles", opt(facts.triangles.map(num))),
    ])
}

/// The description of one element: `not_found` if the tree has no such ID, `not_geometry`
/// if it is not a geometric element (REQ-004, REQ-005).
pub fn describe_element(tree: &dyn ElementTree, params: &DescribeParams) -> Result<CValue, GeoCmdError> {
    match tree.get(&params.element) {
        None => Err(GeoCmdError::NotFound(params.element)),
        Some(TreeEntry::Other { .. }) => Err(GeoCmdError::NotGeometry(params.element)),
        Some(TreeEntry::Geometry(g)) => Ok(description(&params.element, &g.facts, g.source_id.as_deref())),
    }
}

fn run(tree: &dyn ElementTree, params: &CValue) -> Result<Vec<u8>, GeoCmdError> {
    let result = describe_element(tree, &DescribeParams::from_value(params)?)?;
    to_canonical_bytes(&result).map_err(|_| GeoCmdError::Internal("canonical json"))
}

fn logged(
    log: &mut Vec<ActivityEntry>,
    surface: Surface,
    out: Result<Vec<u8>, GeoCmdError>,
) -> Result<Vec<u8>, GeoCmdError> {
    let outcome = match &out {
        Ok(_) => "ok",
        Err(e) => e.code(),
    };
    log.push(ActivityEntry { command: DESCRIBE_ELEMENT, surface, outcome });
    out
}

/// Runs the MCP tool `geo_describe_element` on raw JSON parameters (at most
/// [`MAX_PARAMS_BYTES`] bytes and [`MAX_PARAMS_DEPTH`] levels; duplicate keys refused) and
/// returns the canonical result bytes. One line is appended to `log`.
pub fn invoke_mcp(
    tree: &dyn ElementTree,
    log: &mut Vec<ActivityEntry>,
    tool: &str,
    params: &[u8],
) -> Result<Vec<u8>, GeoCmdError> {
    let out = if tool != MCP_DESCRIBE {
        Err(GeoCmdError::UnknownCommand(kept(tool)))
    } else {
        parse_with_limits(params, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
            .map_err(|e| match e {
                CanonError::InputLimit => GeoCmdError::InputLimit("params"),
                CanonError::DuplicateKey(_) => GeoCmdError::BadParams("duplicate key"),
                _ => GeoCmdError::BadParams("params are not valid JSON"),
            })
            .and_then(|v| run(tree, &v))
    };
    logged(log, Surface::Mcp, out)
}

/// Runs `fid geo <args>`: `describe-element <id> [--json]`. Output is canonical JSON either
/// way. Unknown or repeated options are refused. One line is appended to `log`.
pub fn invoke_cli(tree: &dyn ElementTree, log: &mut Vec<ActivityEntry>, args: &[&str]) -> Result<Vec<u8>, GeoCmdError> {
    let out = cli_params(args).and_then(|v| run(tree, &v));
    logged(log, Surface::Cli, out)
}

fn cli_params(args: &[&str]) -> Result<CValue, GeoCmdError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(GeoCmdError::InputLimit("args"));
    }
    let (sub, rest) = args.split_first().ok_or(GeoCmdError::BadParams("missing subcommand"))?;
    if *sub != CLI_DESCRIBE {
        return Err(GeoCmdError::UnknownCommand(kept(sub)));
    }
    let mut json = false;
    let mut element: Option<&str> = None;
    for arg in rest {
        match *arg {
            "--json" if !json => json = true,
            "--json" => return Err(GeoCmdError::BadParams("repeated option")),
            s if s.starts_with("--") => return Err(GeoCmdError::BadParams("unknown option")),
            s if element.is_none() => element = Some(s),
            _ => return Err(GeoCmdError::BadParams("unexpected argument")),
        }
    }
    let mut o = BTreeMap::new();
    if let Some(e) = element {
        o.insert("element".to_string(), text(e));
    }
    Ok(CValue::Obj(o))
}

// ------------------------------------------------------------------ registry definition

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn nullable(types: &str) -> CValue {
    strs(&[types, "null"])
}

fn result_schema() -> CValue {
    let metres = obj([("type", text("number")), ("x-quantity", text("length")), ("x-unit", text("m"))]);
    let corner = obj([
        ("type", text("array")),
        ("items", metres),
        ("minItems", CValue::Num(3.0)),
        ("maxItems", CValue::Num(3.0)),
    ]);
    let count = obj([("type", text("integer")), ("minimum", CValue::Num(0.0))]);
    let topology = obj([
        ("type", nullable("object")),
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("boundary_edges", count.clone()),
                ("closed", obj([("type", text("boolean"))])),
                ("components", count.clone()),
                ("degenerate", count.clone()),
                ("misoriented_pairs", count.clone()),
                ("nonmanifold_edges", count.clone()),
                ("oriented", obj([("type", text("boolean"))])),
            ]),
        ),
    ]);
    let nullable_count = obj([("type", nullable("integer")), ("minimum", CValue::Num(0.0))]);
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        (
            "required",
            strs(&[
                "aabb",
                "channels",
                "digest",
                "frame",
                "id",
                "kind",
                "points",
                "source_id",
                "topology",
                "triangles",
            ]),
        ),
        (
            "properties",
            obj([
                (
                    "aabb",
                    obj([
                        ("type", nullable("object")),
                        ("additionalProperties", CValue::Bool(false)),
                        ("properties", obj([("max", corner.clone()), ("min", corner)])),
                    ]),
                ),
                ("channels", obj([("type", text("array")), ("items", obj([("type", text("string"))]))])),
                ("digest", obj([("type", text("string")), ("pattern", text("^sha256:[0-9a-f]{64}$"))])),
                ("frame", obj([("type", text("string")), ("pattern", text(ID_PATTERN))])),
                ("id", obj([("type", text("string")), ("pattern", text(ID_PATTERN))])),
                (
                    "kind",
                    obj([("type", text("string")), ("enum", strs(&["CadBodyRef", "Mesh", "PointCloud", "PointSet"]))]),
                ),
                ("points", nullable_count.clone()),
                ("source_id", obj([("type", nullable("string"))])),
                ("topology", topology),
                ("triangles", nullable_count),
            ]),
        ),
    ])
}

/// The registry definition of `geo.describe_element@1`, built through
/// [`CommandDef::from_value`]: class R0, determinism D0, exposed on CLI and MCP with
/// projection `exposed`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["element"])),
        (
            "properties",
            obj([(
                "element",
                obj([
                    ("type", text("string")),
                    ("x-ref", text("element")),
                    ("pattern", text(ID_PATTERN)),
                    ("description", text("The element to describe.")),
                ]),
            )]),
        ),
    ]);
    let def = CommandDef::from_value(obj([
        ("id", text("geo.describe_element")),
        ("major", CValue::Num(1.0)),
        ("title", text("Describe element")),
        (
            "description",
            text(
                "Returns what a geometric element is: kind, point and triangle counts, bounding box in metres, \
                 frame ID, content digest, attribute channels and, for meshes, topology facts.",
            ),
        ),
        ("params_schema", params),
        ("result_schema", result_schema()),
        ("side_effect", text("R0")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("natural")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", CValue::Arr(Vec::new())),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))?;
    Ok(vec![def])
}

//! The MCP emitter: tool definitions from the surface projection (REQ-001 to REQ-004,
//! REQ-006).
//!
//! Pinned rules (stated in the PR, mirrored by `fixtures/mcp/make_fixtures.py`):
//!
//! - **Which commands get a tool** ([`decide`]): the stricter of the declared projection
//!   (`human_only` clamps it to `request_only`, D-031) and the ceiling of the class (R0, R1,
//!   I1, W1, W2 `exposed`; W3, X1, D1 `request_only`; M1, S1, A0 `never`), and `never` when
//!   `mcp` is not in the declared exposure. `never` lists no tool; `request_only` lists a
//!   request-only tool; `exposed` lists a direct tool. This is the bus's own rule
//!   (`fid_bus::policy::projection::mcp_projection`), and the tests check the two agree.
//! - **Name** is the projection's `mcp_name` (`AUTO.SURF.PROJ`); there is no other source.
//! - **Title** is the registry `title`; **description** is the registry `description`,
//!   followed, for a request-only tool, by one space and [`REQUEST_ONLY_SENTENCE`] (or the
//!   sentence alone when the description is empty). No other text is added.
//! - **Input schema**: an object with `additionalProperties: false`, one property per
//!   parameter and `required` in byte order. A parameter with `x-unit` but no `x-quantity` is
//!   `schema_unrepresentable`. A parameter with a quantity kind is
//!   `{value: string, unit: string}` (both required, no other member) with `x-quantity` and
//!   `x-unit` copied from the registry when declared, and no default; it must be declared
//!   `number` (type `object`) or `number` or `null` (type `[null, object]`), else
//!   `schema_unrepresentable`. Any other parameter is `{type: T}` for a JSON
//!   type, `{type: [T, ...]}` for a union of JSON types, and `{}` for `any` or a `#...`
//!   reference into the definition (the bus validates against the full `params_schema`),
//!   with its `default` when declared. Any other type is `schema_unrepresentable`.
//! - **Output schema**: the result envelope schema (`schema/envelope-v1.json`) without `$id`
//!   and `$schema`, with `properties.result` set to the result type mapped as above.
//! - **Annotations**: `read_only_hint` for R0 and R1; `request_only` for request-only tools;
//!   `destructive_hint` for every direct tool that is not read-only (a request-only tool
//!   performs nothing).
//! - **Motion (D-008)**: a tool whose command declares an effect `run_ticket.create`,
//!   `run_ticket.approve` or `run_ticket.release` (ASCII case-insensitive, with or without an
//!   `@major` suffix) fails generation with `motion_tool_forbidden`, unless the command is in
//!   [`MOTION_ALLOWLIST`] or is R0.
//! - **Text**: every key and string of a tool must be free of [`crate::text::forbidden`]
//!   characters, else `forbidden_character`.
//! - **Order**: the projection's (name bytes, major) order. The first error in that order is
//!   reported and nothing is emitted.

use crate::text::first_forbidden;
use fid_canon::{to_canonical_bytes, CValue};
use fid_surface::{envelope_schema, DeclaredProjection, ParamSpec, Projection, SurfaceProjection};
use fid_types::{CommandId, SideEffect};
use std::collections::BTreeMap;
use std::fmt;

/// The surface name, which is also its directory under the generator's output.
pub const SURFACE: &str = "mcp";
/// The tool list file of the surface.
pub const TOOLS_FILE: &str = "mcp-tools.json";
/// The fixed sentence every request-only tool's description ends with (REQ-004).
pub const REQUEST_ONLY_SENTENCE: &str =
    "Calling this tool only files a request for a human to review; it performs nothing itself.";
/// Declared effects that create, approve or release a motion run ticket (D-008).
pub const MOTION_EFFECTS: [&str; 3] = ["run_ticket.create", "run_ticket.approve", "run_ticket.release"];
/// Commands that may declare a [`MOTION_EFFECTS`] effect and still be listed (D-008: the
/// hold request). R0 status queries are allowed too.
pub const MOTION_ALLOWLIST: [&str; 1] = ["cmm.request_hold@1"];

const JSON_TYPES: [&str; 7] = ["array", "boolean", "integer", "null", "number", "object", "string"];

/// How a listed tool behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolKind {
    /// The bus runs the command (W1 and W2 on the agent's branch).
    Direct,
    /// The bus only files a pending request for a human (W3, X1, D1, `human_only`).
    RequestOnly,
}

/// The tool an MCP caller gets for a command, or `None` for no tool. Total over its inputs:
/// the stricter of `declared` (clamped to `request_only` when `human_only`) and the class
/// ceiling, and `None` unless the command is exposed on `mcp`. Pure.
pub fn decide(class: SideEffect, declared: DeclaredProjection, human_only: bool, on_mcp: bool) -> Option<ToolKind> {
    let declared = if human_only && declared > DeclaredProjection::RequestOnly {
        DeclaredProjection::RequestOnly
    } else {
        declared
    };
    let ceiling = match class {
        SideEffect::R0 | SideEffect::R1 | SideEffect::I1 | SideEffect::W1 | SideEffect::W2 => {
            DeclaredProjection::Exposed
        }
        SideEffect::W3 | SideEffect::X1 | SideEffect::D1 => DeclaredProjection::RequestOnly,
        SideEffect::M1 | SideEffect::S1 | SideEffect::A0 => DeclaredProjection::Never,
    };
    if !on_mcp {
        return None;
    }
    match declared.min(ceiling) {
        DeclaredProjection::Never => None,
        DeclaredProjection::RequestOnly => Some(ToolKind::RequestOnly),
        DeclaredProjection::Exposed => Some(ToolKind::Direct),
    }
}

/// Why the MCP surface could not be generated. Codes are stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpError {
    /// A listed tool would create, approve or release a motion run ticket (D-008).
    MotionToolForbidden { command: String, effect: String },
    /// A parameter or result type has no JSON Schema form here.
    SchemaUnrepresentable { command: String, detail: String },
    /// A tool text holds a character of [`crate::text::FORBIDDEN_RANGES`].
    ForbiddenCharacter { command: String, code_point: u32 },
    /// The tool list has no canonical JSON form.
    Serialise { detail: String },
}

impl McpError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            McpError::MotionToolForbidden { .. } => "motion_tool_forbidden",
            McpError::SchemaUnrepresentable { .. } => "schema_unrepresentable",
            McpError::ForbiddenCharacter { .. } => "forbidden_character",
            McpError::Serialise { .. } => "serialise_failed",
        }
    }

    /// The command this error names, if any.
    pub fn command(&self) -> Option<&str> {
        match self {
            McpError::MotionToolForbidden { command, .. }
            | McpError::SchemaUnrepresentable { command, .. }
            | McpError::ForbiddenCharacter { command, .. } => Some(command),
            McpError::Serialise { .. } => None,
        }
    }
}

impl fmt::Display for McpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            McpError::MotionToolForbidden { command, effect } => {
                write!(
                    f,
                    "{code}: {command} declares {effect:?}; agents may not create, approve or release run tickets"
                )
            }
            McpError::SchemaUnrepresentable { command, detail } => write!(f, "{code}: {command}: {detail}"),
            McpError::ForbiddenCharacter { command, code_point } => {
                write!(f, "{code}: {command}: tool text holds U+{code_point:04X}")
            }
            McpError::Serialise { detail } => write!(f, "{code}: {detail}"),
        }
    }
}

impl std::error::Error for McpError {}

/// Hints for MCP clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Annotations {
    pub read_only_hint: bool,
    pub destructive_hint: bool,
    pub request_only: bool,
}

/// One MCP tool definition.
#[derive(Debug, Clone, PartialEq)]
pub struct McpTool {
    pub name: String,
    pub title: String,
    pub description: String,
    pub input_schema: CValue,
    pub output_schema: CValue,
    pub annotations: Annotations,
    pub command: CommandId,
    pub kind: ToolKind,
}

fn s(x: &str) -> CValue {
    CValue::Str(x.to_string())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

impl McpTool {
    /// The JSON value of this tool in `mcp-tools.json`.
    pub fn to_value(&self) -> CValue {
        obj([
            (
                "annotations",
                obj([
                    ("destructive_hint", CValue::Bool(self.annotations.destructive_hint)),
                    ("read_only_hint", CValue::Bool(self.annotations.read_only_hint)),
                    ("request_only", CValue::Bool(self.annotations.request_only)),
                ]),
            ),
            (
                "command",
                obj([
                    ("command_id", s(self.command.as_str())),
                    ("major", CValue::Num(f64::from(self.command.major()))),
                ]),
            ),
            ("description", s(&self.description)),
            ("input_schema", self.input_schema.clone()),
            ("name", s(&self.name)),
            ("output_schema", self.output_schema.clone()),
            ("title", s(&self.title)),
        ])
    }
}

fn unrepresentable(sp: &SurfaceProjection, detail: String) -> McpError {
    McpError::SchemaUnrepresentable { command: sp.command_id.as_str().to_string(), detail }
}

/// JSON Schema of a projected type string (see the module rules), or `None`.
fn type_schema(ty: &str) -> Option<CValue> {
    if ty == "any" || ty.starts_with('#') {
        return Some(CValue::Obj(BTreeMap::new()));
    }
    let parts: Vec<&str> = ty.split('|').collect();
    if !parts.iter().all(|p| JSON_TYPES.contains(p)) {
        return None;
    }
    match parts.as_slice() {
        [one] => Some(obj([("type", s(one))])),
        many => Some(obj([("type", CValue::Arr(many.iter().map(|p| s(p)).collect()))])),
    }
}

fn param_schema(sp: &SurfaceProjection, p: &ParamSpec) -> Result<CValue, McpError> {
    if p.quantity_kind.is_none() && p.unit.is_some() {
        return Err(unrepresentable(sp, format!("parameter {:?} declares x-unit without x-quantity", p.name)));
    }
    if p.quantity_kind.is_some() {
        let ty = match p.ty.as_str() {
            "number" => s("object"),
            "null|number" => CValue::Arr(vec![s("null"), s("object")]),
            other => {
                return Err(unrepresentable(
                    sp,
                    format!("quantity parameter {:?} has type {other:?}, not number", p.name),
                ))
            }
        };
        let mut o = BTreeMap::new();
        o.insert("type".to_string(), ty);
        o.insert("additionalProperties".to_string(), CValue::Bool(false));
        o.insert("required".to_string(), CValue::Arr(vec![s("unit"), s("value")]));
        o.insert(
            "properties".to_string(),
            obj([("unit", obj([("type", s("string"))])), ("value", obj([("type", s("string"))]))]),
        );
        if let Some(k) = &p.quantity_kind {
            o.insert("x-quantity".to_string(), s(k));
        }
        if let Some(u) = &p.unit {
            o.insert("x-unit".to_string(), s(u));
        }
        return Ok(CValue::Obj(o));
    }
    let schema =
        type_schema(&p.ty).ok_or_else(|| unrepresentable(sp, format!("parameter {:?} has type {:?}", p.name, p.ty)))?;
    let CValue::Obj(mut o) = schema else {
        return Err(unrepresentable(sp, format!("parameter {:?}", p.name)));
    };
    if let Some(d) = &p.default {
        o.insert("default".to_string(), d.clone());
    }
    Ok(CValue::Obj(o))
}

fn input_schema(sp: &SurfaceProjection) -> Result<CValue, McpError> {
    let mut props = BTreeMap::new();
    let mut required = Vec::new();
    for p in &sp.params {
        props.insert(p.name.clone(), param_schema(sp, p)?);
        if p.required {
            required.push(p.name.as_str());
        }
    }
    required.sort_unstable();
    Ok(obj([
        ("additionalProperties", CValue::Bool(false)),
        ("properties", CValue::Obj(props)),
        ("required", CValue::Arr(required.into_iter().map(s).collect())),
        ("type", s("object")),
    ]))
}

fn output_schema(sp: &SurfaceProjection) -> Result<CValue, McpError> {
    let result =
        type_schema(&sp.result).ok_or_else(|| unrepresentable(sp, format!("result has type {:?}", sp.result)))?;
    let CValue::Obj(mut env) = envelope_schema() else {
        return Err(unrepresentable(sp, "the envelope schema is not an object".to_string()));
    };
    env.remove("$id");
    env.remove("$schema");
    match env.get_mut("properties") {
        Some(CValue::Obj(props)) => {
            props.insert("result".to_string(), result);
        }
        _ => return Err(unrepresentable(sp, "the envelope schema has no properties".to_string())),
    }
    Ok(CValue::Obj(env))
}

/// The motion effect `sp` declares, if any (see the module rules).
fn motion_effect(sp: &SurfaceProjection) -> Option<&str> {
    sp.effects.iter().map(String::as_str).find(|e| {
        let base = e.split('@').next().unwrap_or(e);
        MOTION_EFFECTS.iter().any(|m| m.eq_ignore_ascii_case(base))
    })
}

/// The first forbidden character in any key or string of `v`.
fn forbidden_in(v: &CValue) -> Option<char> {
    match v {
        CValue::Str(x) => first_forbidden(x),
        CValue::Arr(items) => items.iter().find_map(forbidden_in),
        CValue::Obj(o) => o.iter().find_map(|(k, x)| first_forbidden(k).or_else(|| forbidden_in(x))),
        _ => None,
    }
}

/// The tool for one command, or `None` when MCP callers get none (REQ-001 to REQ-004).
pub fn tool_for(sp: &SurfaceProjection) -> Result<Option<McpTool>, McpError> {
    let on_mcp = sp.exposure.iter().any(|x| x == SURFACE);
    let Some(kind) = decide(sp.side_effect, sp.mcp_projection.declared_form(), sp.human_only, on_mcp) else {
        return Ok(None);
    };
    let command = sp.command_id.as_str();
    if let Some(effect) = motion_effect(sp) {
        if sp.side_effect != SideEffect::R0 && !MOTION_ALLOWLIST.contains(&command) {
            return Err(McpError::MotionToolForbidden { command: command.to_string(), effect: effect.to_string() });
        }
    }
    let read_only = matches!(sp.side_effect, SideEffect::R0 | SideEffect::R1);
    let description = match kind {
        ToolKind::Direct => sp.help.description.clone(),
        ToolKind::RequestOnly if sp.help.description.is_empty() => REQUEST_ONLY_SENTENCE.to_string(),
        ToolKind::RequestOnly => format!("{} {REQUEST_ONLY_SENTENCE}", sp.help.description),
    };
    let tool = McpTool {
        name: sp.mcp_name.clone(),
        title: sp.help.title.clone(),
        description,
        input_schema: input_schema(sp)?,
        output_schema: output_schema(sp)?,
        annotations: Annotations {
            read_only_hint: read_only,
            destructive_hint: kind == ToolKind::Direct && !read_only,
            request_only: kind == ToolKind::RequestOnly,
        },
        command: sp.command_id.clone(),
        kind,
    };
    if let Some(c) = forbidden_in(&tool.to_value()) {
        return Err(McpError::ForbiddenCharacter { command: command.to_string(), code_point: u32::from(c) });
    }
    Ok(Some(tool))
}

/// Every tool of a projection, in projection order; the first error stops generation.
pub fn tools(p: &Projection) -> Result<Vec<McpTool>, McpError> {
    let mut out = Vec::new();
    for sp in &p.commands {
        if let Some(t) = tool_for(sp)? {
            out.push(t);
        }
    }
    Ok(out)
}

/// The emitted surface: the bytes of [`TOOLS_FILE`] (RFC 8785, without the generator's
/// header member) and the commands it covers.
#[derive(Debug, Clone, PartialEq)]
pub struct Emitted {
    pub tools: Vec<McpTool>,
    pub bytes: Vec<u8>,
    pub commands: Vec<CommandId>,
}

/// Generates the MCP surface of a projection: `{manifest_digest, surface, tools}`.
pub fn emit(p: &Projection) -> Result<Emitted, McpError> {
    let tools = tools(p)?;
    let value = obj([
        ("manifest_digest", s(p.manifest_digest.as_str())),
        ("surface", s(SURFACE)),
        ("tools", CValue::Arr(tools.iter().map(McpTool::to_value).collect())),
    ]);
    let bytes = to_canonical_bytes(&value).map_err(|e| McpError::Serialise { detail: e.code().to_string() })?;
    let commands = tools.iter().map(|t| t.command.clone()).collect();
    Ok(Emitted { tools, bytes, commands })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_table() {
        use DeclaredProjection as D;
        use ToolKind as K;
        for class in SideEffect::ALL.iter().copied() {
            let want = match class {
                SideEffect::R0 | SideEffect::R1 | SideEffect::I1 | SideEffect::W1 | SideEffect::W2 => Some(K::Direct),
                SideEffect::W3 | SideEffect::X1 | SideEffect::D1 => Some(K::RequestOnly),
                SideEffect::M1 | SideEffect::S1 | SideEffect::A0 => None,
            };
            assert_eq!(decide(class, D::Exposed, false, true), want, "{class:?}");
            assert_eq!(decide(class, D::Exposed, false, false), None);
            assert_eq!(decide(class, D::Never, false, true), None);
            let ho = decide(class, D::Exposed, true, true);
            assert_eq!(ho, want.map(|_| K::RequestOnly), "{class:?} human_only");
        }
    }

    #[test]
    fn types() {
        assert_eq!(type_schema("any"), Some(CValue::Obj(BTreeMap::new())));
        assert_eq!(type_schema("#/$defs/frame"), Some(CValue::Obj(BTreeMap::new())));
        assert_eq!(type_schema("string"), Some(obj([("type", s("string"))])));
        assert_eq!(type_schema("null|string"), Some(obj([("type", CValue::Arr(vec![s("null"), s("string")]))])));
        assert_eq!(type_schema("decimal"), None);
        assert_eq!(type_schema("string|decimal"), None);
        assert_eq!(type_schema("https://x/y.json"), None);
    }
}

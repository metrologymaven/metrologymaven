//! Surface projection model. Spec node `AUTO.SURF.PROJ`.
//!
//! [`project`] is a pure function from the registry manifest bytes to one [`SurfaceProjection`]
//! per command major: CLI path, MCP tool name, Python name, MCP projection, parameters with
//! quantity kinds and units, result type, side-effect and determinism class. Every surface
//! emitter and the completeness check consume this one model.
//!
//! The input is refused, never repaired: bytes that are not RFC 8785 canonical, unknown keys,
//! a `definition_digest` that does not match its definition, a name outside the surface grammar,
//! a declared MCP projection looser than the class table, and any two commands deriving the
//! same name are all typed errors, and no partial projection is returned. A declaration
//! stricter than the class table projects to itself, as the bus decides it
//! ([`McpProjection::for_declared`]). Commands are
//! processed in (name bytes, major) order and the first error in that order is reported, so the
//! error is as deterministic as the output.

use crate::names::{derive, DerivedNames};
use fid_canon::{digest as canon_digest, digest_bytes, parse_with_limits, to_canonical_bytes, CValue};
use fid_types::{CommandId, CommandName, Determinism, Digest, SideEffect};
use std::collections::BTreeMap;
use std::fmt;

/// Largest accepted manifest, in bytes.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024 * 1024;
/// Deepest accepted nesting of the manifest.
pub const MAX_MANIFEST_DEPTH: usize = 64;
/// The only manifest `format` this build reads (CORE.REG.MAN).
pub const MANIFEST_FORMAT: f64 = 1.0;

/// How a command appears to MCP clients (REQ-005). Ordered from most to least visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum McpProjection {
    /// Listed and callable.
    Exposed,
    /// Callable as a named tool; agents write to an agent branch.
    Named,
    /// Agents may only file a request for a human.
    RequestOnly,
    /// Never on MCP.
    None,
}

impl McpProjection {
    /// Every value, most visible first.
    pub const ALL: [McpProjection; 4] =
        [McpProjection::Exposed, McpProjection::Named, McpProjection::RequestOnly, McpProjection::None];

    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            McpProjection::Exposed => "exposed",
            McpProjection::Named => "named",
            McpProjection::RequestOnly => "request_only",
            McpProjection::None => "none",
        }
    }

    /// The class table of REQ-005 (CONSTITUTION §3).
    pub fn from_class(class: SideEffect) -> McpProjection {
        match class {
            SideEffect::R0 | SideEffect::R1 | SideEffect::I1 => McpProjection::Exposed,
            SideEffect::W1 | SideEffect::W2 => McpProjection::Named,
            SideEffect::W3 | SideEffect::X1 | SideEffect::D1 => McpProjection::RequestOnly,
            SideEffect::M1 | SideEffect::S1 | SideEffect::A0 => McpProjection::None,
        }
    }

    /// The projection of a command: the class table, tightened to `request_only` when the
    /// command is `human_only` (D-031, CONSTITUTION §3).
    pub fn for_command(class: SideEffect, human_only: bool) -> McpProjection {
        let p = McpProjection::from_class(class);
        if human_only && p < McpProjection::RequestOnly {
            McpProjection::RequestOnly
        } else {
            p
        }
    }

    /// The projection of a command that declares `declared` (already clamped by `human_only`)
    /// under the class ceiling `ceiling` ([`McpProjection::for_command`]): the stricter of the
    /// two, the rule of the bus (`CORE.BUS.POL.PROJ` `mcp_projection`). A declaration at the
    /// ceiling projects to the ceiling (`exposed` on a W1 or W2 command is `named`); a stricter
    /// one projects to itself (`request_only`, or `never` as `none`). `None` when the
    /// declaration is looser than the ceiling: the registry refuses that, and the projection
    /// fails closed with `projection_mismatch` rather than clamp it (REQ-006).
    pub fn for_declared(declared: DeclaredProjection, ceiling: McpProjection) -> Option<McpProjection> {
        match declared.cmp(&ceiling.declared_form()) {
            std::cmp::Ordering::Greater => None,
            std::cmp::Ordering::Equal => Some(ceiling),
            std::cmp::Ordering::Less if declared == DeclaredProjection::Never => Some(McpProjection::None),
            // Stricter than a ceiling that is at most `exposed`, and not `never`: `request_only`.
            std::cmp::Ordering::Less => Some(McpProjection::RequestOnly),
        }
    }

    /// The registry's three-valued declaration this projection corresponds to. The registry
    /// (CORE.REG.DEF.FIELD) has no `named` value: a W1 or W2 command declares `exposed`.
    pub fn declared_form(self) -> DeclaredProjection {
        match self {
            McpProjection::Exposed | McpProjection::Named => DeclaredProjection::Exposed,
            McpProjection::RequestOnly => DeclaredProjection::RequestOnly,
            McpProjection::None => DeclaredProjection::Never,
        }
    }
}

/// The registry's declared `mcp_projection` (CORE.REG.DEF.FIELD wire values).
///
/// Read here rather than through `fid-registry`: surface crates link `fid-surface` and may not
/// reach a CORE crate (`lint-allowlist.toml`, `[surfaces]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeclaredProjection {
    Never,
    RequestOnly,
    Exposed,
}

impl DeclaredProjection {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            DeclaredProjection::Never => "never",
            DeclaredProjection::RequestOnly => "request_only",
            DeclaredProjection::Exposed => "exposed",
        }
    }

    /// Parses the wire string.
    pub fn parse(s: &str) -> Option<DeclaredProjection> {
        match s {
            "never" => Some(DeclaredProjection::Never),
            "request_only" => Some(DeclaredProjection::RequestOnly),
            "exposed" => Some(DeclaredProjection::Exposed),
            _ => None,
        }
    }
}

impl fmt::Display for McpProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One parameter of a command, read from `params_schema.properties`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamSpec {
    pub name: String,
    /// See [`type_of`].
    pub ty: String,
    /// `x-quantity`, when declared.
    pub quantity_kind: Option<String>,
    /// `x-unit`, when declared.
    pub unit: Option<String>,
    /// `default`, when declared (kept as the typed JSON value).
    pub default: Option<CValue>,
    /// Listed in the schema's `required`.
    pub required: bool,
    /// `"path_like": true` (convention shared with AGENT.REDTEAM.SCRIPT; read by AUTO.SURF.ROOTS).
    pub path_like: bool,
}

/// Help text of a command, from the definition's `title` and `description`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocText {
    pub title: String,
    pub description: String,
}

/// The derived description of one command major.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceProjection {
    pub command_id: CommandId,
    pub major: u32,
    pub cli_path: Vec<String>,
    pub mcp_name: String,
    pub py_module: String,
    pub py_name: String,
    /// The MCP projection from the class table and the declaration (REQ-005, REQ-006). It does
    /// not account for `exposure`: the bus treats a command without `mcp` among its surfaces
    /// as `never` whatever this says, so a consumer must also check that `mcp` is in the
    /// command's exposure (as the MCP emitter's `tool_for` does, AUTO.MCP.PROJ).
    pub mcp_projection: McpProjection,
    pub side_effect: SideEffect,
    pub determinism: Determinism,
    pub human_only: bool,
    pub params: Vec<ParamSpec>,
    /// Result type, see [`type_of`].
    pub result: String,
    /// The registry does not declare this yet; always `false` (stated in the PR).
    pub long_running: bool,
    /// The registry does not declare this yet; always `false`, so surfaces refuse `--dry-run`.
    pub dry_run: bool,
    pub help: DocText,
    /// Declared `effects` (journal event types), in manifest order. Registry metadata read by
    /// [`crate::roots::is_root_config`] (AUTO.SURF.ROOTS); not part of [`Self::to_value`].
    pub effects: Vec<String>,
    /// The parameter schema declares `"path_like": true` somewhere other than on a top-level
    /// property. Such a file parameter cannot be checked, so every surface refuses the command
    /// ([`crate::roots::gate_raw_paths`]); not part of [`Self::to_value`].
    pub nested_path_like: bool,
}

/// The projection of one manifest (REQ-001).
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    /// SHA-256 of the exact manifest bytes.
    pub manifest_digest: Digest,
    /// Ordered by command name bytes, then major.
    pub commands: Vec<SurfaceProjection>,
}

/// Which surface a colliding name belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NameKind {
    Cli,
    Mcp,
    Python,
}

impl NameKind {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            NameKind::Cli => "cli",
            NameKind::Mcp => "mcp",
            NameKind::Python => "python",
        }
    }
}

/// Why a manifest could not be projected. Codes are stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjError {
    /// The bytes are not a well-formed canonical manifest.
    ManifestParse { detail: String },
    /// A command ID outside the surface grammar.
    BadCommandId { command: String, detail: String },
    /// The declared MCP projection is looser than the class table allows.
    ProjectionMismatch { command: String, declared: String, expected: McpProjection },
    /// Two commands derive the same name. `first` sorts before `second`.
    NameCollision { kind: NameKind, name: String, first: String, second: String },
    /// The MCP tool name is longer than 64 characters.
    NameTooLong { command: String, name: String, len: usize },
    /// A derived name contains characters (or is a word) its surface cannot carry.
    BadNameChars { command: String, name: String, detail: String },
    /// Memory for the output could not be reserved.
    AllocFailed,
}

impl ProjError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            ProjError::ManifestParse { .. } => "manifest_parse",
            ProjError::BadCommandId { .. } => "bad_command_id",
            ProjError::ProjectionMismatch { .. } => "projection_mismatch",
            ProjError::NameCollision { .. } => "name_collision",
            ProjError::NameTooLong { .. } => "name_too_long",
            ProjError::BadNameChars { .. } => "bad_name_chars",
            ProjError::AllocFailed => "alloc_failed",
        }
    }

    /// The command IDs this error names, in sort order.
    pub fn commands(&self) -> Vec<&str> {
        match self {
            ProjError::ManifestParse { .. } | ProjError::AllocFailed => Vec::new(),
            ProjError::BadCommandId { command, .. }
            | ProjError::ProjectionMismatch { command, .. }
            | ProjError::NameTooLong { command, .. }
            | ProjError::BadNameChars { command, .. } => vec![command.as_str()],
            ProjError::NameCollision { first, second, .. } => vec![first.as_str(), second.as_str()],
        }
    }
}

impl fmt::Display for ProjError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            ProjError::ManifestParse { detail } => write!(f, "{code}: {detail}"),
            ProjError::BadCommandId { command, detail } => write!(f, "{code}: {command:?}: {detail}"),
            ProjError::ProjectionMismatch { command, declared, expected } => {
                write!(f, "{code}: {command} declares {declared:?}, looser than its class allows ({expected})")
            }
            ProjError::NameCollision { kind, name, first, second } => {
                write!(f, "{code}: {} name {name:?} is derived by both {first} and {second}", kind.as_str())
            }
            ProjError::NameTooLong { command, name, len } => {
                write!(f, "{code}: {command}: MCP name {name:?} has {len} characters (at most 64)")
            }
            ProjError::BadNameChars { command, name, detail } => write!(f, "{code}: {command}: {name:?}: {detail}"),
            ProjError::AllocFailed => f.write_str(code),
        }
    }
}

impl std::error::Error for ProjError {}

fn parse_err(detail: impl Into<String>) -> ProjError {
    ProjError::ManifestParse { detail: detail.into() }
}

/// Type of a schema: its `type` when a string, the `|`-joined sorted list when an array of
/// strings, else its `$ref`, else `any`. Any other shape of `type` or `$ref` is refused.
pub fn type_of(schema: &BTreeMap<String, CValue>, at: &str) -> Result<String, ProjError> {
    match schema.get("type") {
        Some(CValue::Str(s)) => return Ok(s.clone()),
        Some(CValue::Arr(items)) => {
            let mut names = Vec::new();
            names.try_reserve_exact(items.len()).map_err(|_| ProjError::AllocFailed)?;
            for item in items {
                match item {
                    CValue::Str(s) => names.push(s.as_str()),
                    _ => return Err(parse_err(format!("{at}: type must be a string or an array of strings"))),
                }
            }
            if names.is_empty() {
                return Err(parse_err(format!("{at}: type must not be an empty array")));
            }
            names.sort_unstable();
            names.dedup();
            return Ok(names.join("|"));
        }
        Some(_) => return Err(parse_err(format!("{at}: type must be a string or an array of strings"))),
        None => {}
    }
    match schema.get("$ref") {
        Some(CValue::Str(s)) => Ok(s.clone()),
        Some(_) => Err(parse_err(format!("{at}: $ref must be a string"))),
        None => Ok("any".to_string()),
    }
}

fn opt_str(o: &BTreeMap<String, CValue>, key: &str, at: &str) -> Result<Option<String>, ProjError> {
    match o.get(key) {
        None => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(parse_err(format!("{at}: {key} must be a string"))),
    }
}

/// Reads the parameters of `params_schema`, in property-name byte order.
fn params_of(schema: &CValue, command: &str) -> Result<Vec<ParamSpec>, ProjError> {
    let at = format!("{command}: params_schema");
    let CValue::Obj(root) = schema else {
        return Err(parse_err(format!("{at} must be an object")));
    };
    let props = match root.get("properties") {
        None => return Ok(Vec::new()),
        Some(CValue::Obj(p)) => p,
        Some(_) => return Err(parse_err(format!("{at}.properties must be an object"))),
    };
    let required: Vec<&str> = match root.get("required") {
        None => Vec::new(),
        Some(CValue::Arr(items)) => {
            let mut r = Vec::new();
            r.try_reserve_exact(items.len()).map_err(|_| ProjError::AllocFailed)?;
            for item in items {
                match item {
                    CValue::Str(s) if props.contains_key(s) => r.push(s.as_str()),
                    CValue::Str(s) => return Err(parse_err(format!("{at}.required names unknown property {s:?}"))),
                    _ => return Err(parse_err(format!("{at}.required must be an array of strings"))),
                }
            }
            r
        }
        Some(_) => return Err(parse_err(format!("{at}.required must be an array of strings"))),
    };
    let mut out = Vec::new();
    out.try_reserve_exact(props.len()).map_err(|_| ProjError::AllocFailed)?;
    // BTreeMap iteration is key byte order.
    for (name, p) in props {
        let pat = format!("{at}.properties.{name}");
        let CValue::Obj(p) = p else {
            return Err(parse_err(format!("{pat} must be an object")));
        };
        let path_like = match p.get("path_like") {
            None => false,
            Some(CValue::Bool(b)) => *b,
            Some(_) => return Err(parse_err(format!("{pat}.path_like must be a boolean"))),
        };
        out.push(ParamSpec {
            name: name.clone(),
            ty: type_of(p, &pat)?,
            quantity_kind: opt_str(p, "x-quantity", &pat)?,
            unit: opt_str(p, "x-unit", &pat)?,
            default: p.get("default").cloned(),
            required: required.contains(&name.as_str()),
            path_like,
        });
    }
    Ok(out)
}

/// Number of `"path_like": true` keywords anywhere in `v` (the same count CORE.BUS.POL.FS
/// takes, so the surface and the bus agree on which commands declare a nested file parameter).
fn count_path_like(v: &CValue) -> usize {
    match v {
        CValue::Obj(o) => {
            let here = usize::from(o.get("path_like") == Some(&CValue::Bool(true)));
            o.values().fold(here, |n, x| n.saturating_add(count_path_like(x)))
        }
        CValue::Arr(a) => a.iter().fold(0usize, |n, x| n.saturating_add(count_path_like(x))),
        _ => 0,
    }
}

/// Every field a CORE.REG.DEF.FIELD definition may carry.
const DEF_FIELDS: [&str; 22] = [
    "id",
    "major",
    "title",
    "description",
    "params_schema",
    "result_schema",
    "side_effect",
    "mcp_projection",
    "determinism",
    "idempotency",
    "fs_effect",
    "undo",
    "preconditions",
    "effects",
    "algorithms",
    "exposure",
    "lifecycle",
    "human_only",
    "network",
    "web",
    "web_note",
    "cli_bare",
];

/// The fields CORE.REG.DEF.FIELD requires (the rest have defaults or are conditional).
const DEF_REQUIRED: [&str; 18] = [
    "id",
    "major",
    "title",
    "description",
    "params_schema",
    "result_schema",
    "side_effect",
    "mcp_projection",
    "determinism",
    "idempotency",
    "fs_effect",
    "undo",
    "preconditions",
    "effects",
    "algorithms",
    "exposure",
    "lifecycle",
    "web",
];

/// The part of a definition the projection reads. Full declaration validation is CORE.REG's;
/// this reader checks the field set and the type of every field it uses, and refuses the rest.
struct Def {
    title: String,
    description: String,
    params_schema: CValue,
    result_schema: CValue,
    side_effect: SideEffect,
    mcp_projection: DeclaredProjection,
    determinism: Determinism,
    human_only: bool,
    effects: Vec<String>,
}

impl Def {
    /// The projection MCP clients get: `human_only` caps it at `request_only` (D-031).
    fn effective_projection(&self) -> DeclaredProjection {
        if self.human_only && self.mcp_projection > DeclaredProjection::RequestOnly {
            DeclaredProjection::RequestOnly
        } else {
            self.mcp_projection
        }
    }
}

/// One parsed manifest entry, before projection.
struct Entry {
    id: CommandId,
    def: Def,
}

/// Reads one definition (without `definition_digest`) into its command ID and [`Def`].
fn read_def(index: usize, mut o: BTreeMap<String, CValue>) -> Result<Entry, ProjError> {
    let at = |m: &str| parse_err(format!("commands[{index}]: {m}"));
    if let Some(k) = o.keys().find(|k| !DEF_FIELDS.contains(&k.as_str())) {
        return Err(at(&format!("unknown field {k:?}")));
    }
    if let Some(k) = DEF_REQUIRED.iter().find(|k| !o.contains_key(**k)) {
        return Err(at(&format!("missing field {k:?}")));
    }
    let mut text = |k: &str| match o.remove(k) {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(at(&format!("{k} must be a string"))),
    };
    let name = text("id")?;
    let title = text("title")?;
    let description = text("description")?;
    let side_effect = text("side_effect")?;
    let mcp_projection = text("mcp_projection")?;
    let determinism = text("determinism")?;
    if !CommandName::is_valid(&name) {
        return Err(ProjError::BadCommandId { command: name, detail: "not a command name".to_string() });
    }
    let major = match o.remove("major") {
        Some(CValue::Num(n)) if n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&n) => n as u32,
        _ => return Err(at("major must be an integer 0..2^32")),
    };
    let id = CommandId::parse(&format!("{name}@{major}")).map_err(|_| ProjError::BadCommandId {
        command: format!("{name}@{major}"),
        detail: "major has more than 5 digits".to_string(),
    })?;
    let side_effect = SideEffect::parse(&side_effect).map_err(|_| at("bad side_effect"))?;
    let mcp_projection = DeclaredProjection::parse(&mcp_projection).ok_or_else(|| at("bad mcp_projection"))?;
    let determinism = Determinism::parse(&determinism).map_err(|_| at("bad determinism"))?;
    let human_only = match o.remove("human_only") {
        None => false,
        Some(CValue::Bool(b)) => b,
        Some(_) => return Err(at("human_only must be a boolean")),
    };
    let (Some(params_schema), Some(result_schema)) = (o.remove("params_schema"), o.remove("result_schema")) else {
        return Err(at("missing schema"));
    };
    let effects = match o.remove("effects") {
        Some(CValue::Arr(items)) => {
            let mut out = Vec::new();
            out.try_reserve_exact(items.len()).map_err(|_| ProjError::AllocFailed)?;
            for item in items {
                match item {
                    CValue::Str(s) => out.push(s),
                    _ => return Err(at("effects must be an array of strings")),
                }
            }
            out
        }
        _ => return Err(at("effects must be an array of strings")),
    };
    let def = Def {
        title,
        description,
        params_schema,
        result_schema,
        side_effect,
        mcp_projection,
        determinism,
        human_only,
        effects,
    };
    Ok(Entry { id, def })
}

/// Parses and checks the manifest envelope and every definition.
fn read_manifest(manifest: &[u8]) -> Result<Vec<Entry>, ProjError> {
    let value = parse_with_limits(manifest, MAX_MANIFEST_BYTES, MAX_MANIFEST_DEPTH)
        .map_err(|e| parse_err(format!("json: {}", e.code())))?;
    let canonical = to_canonical_bytes(&value).map_err(|e| parse_err(format!("json: {}", e.code())))?;
    if canonical != manifest {
        return Err(parse_err("manifest is not RFC 8785 canonical JSON"));
    }
    let CValue::Obj(mut top) = value else {
        return Err(parse_err("manifest must be an object"));
    };
    let format = top.remove("format");
    let qcd = top.remove("quantity_catalogue_digest");
    let commands = top.remove("commands");
    if let Some(k) = top.keys().next() {
        return Err(parse_err(format!("unknown manifest field {k:?}")));
    }
    match format {
        // Bit comparison: exactly 1, and no float equality lint.
        Some(CValue::Num(n)) if n.to_bits() == MANIFEST_FORMAT.to_bits() => {}
        Some(_) => return Err(parse_err("unsupported manifest format (expected 1)")),
        None => return Err(parse_err("missing manifest field \"format\"")),
    }
    match qcd {
        Some(CValue::Str(s)) if Digest::is_valid(&s) => {}
        Some(_) => return Err(parse_err("quantity_catalogue_digest must be a sha256: digest")),
        None => return Err(parse_err("missing manifest field \"quantity_catalogue_digest\"")),
    }
    let items = match commands {
        Some(CValue::Arr(items)) => items,
        Some(_) => return Err(parse_err("commands must be an array")),
        None => return Err(parse_err("missing manifest field \"commands\"")),
    };
    let mut out = Vec::new();
    out.try_reserve_exact(items.len()).map_err(|_| ProjError::AllocFailed)?;
    for (index, item) in items.into_iter().enumerate() {
        let CValue::Obj(mut obj) = item else {
            return Err(parse_err(format!("commands[{index}] must be an object")));
        };
        let stated = match obj.remove("definition_digest") {
            Some(CValue::Str(s)) => Digest::parse(&s)
                .map_err(|_| parse_err(format!("commands[{index}].definition_digest is not a sha256: digest")))?,
            Some(_) => return Err(parse_err(format!("commands[{index}].definition_digest must be a string"))),
            None => return Err(parse_err(format!("commands[{index}] has no definition_digest"))),
        };
        let rest = CValue::Obj(obj);
        let actual = canon_digest(&rest).map_err(|e| parse_err(format!("commands[{index}]: {}", e.code())))?;
        if actual.0 != stated.to_bytes() {
            return Err(parse_err(format!("commands[{index}].definition_digest does not match the definition")));
        }
        let CValue::Obj(obj) = rest else {
            return Err(parse_err(format!("commands[{index}] must be an object")));
        };
        out.push(read_def(index, obj)?);
    }
    // Byte order of the name, then numeric major (REQ-008).
    out.sort_by(|a, b| a.id.name().as_bytes().cmp(b.id.name().as_bytes()).then(a.id.major().cmp(&b.id.major())));
    Ok(out)
}

/// Records `name` for `id` in a collision map, failing on a repeat.
fn claim(map: &mut BTreeMap<String, String>, kind: NameKind, name: String, id: &str) -> Result<(), ProjError> {
    if let Some(first) = map.get(&name) {
        return Err(ProjError::NameCollision { kind, name, first: first.clone(), second: id.to_string() });
    }
    map.insert(name, id.to_string());
    Ok(())
}

/// Projects a registry manifest (REQ-001 to REQ-008).
///
/// Takes only the manifest bytes. Returns every command's projection, or the first error in
/// command order and no projection at all.
pub fn project(manifest: &[u8]) -> Result<Projection, ProjError> {
    let manifest_digest = Digest::from_bytes(digest_bytes(manifest).0);
    let entries = read_manifest(manifest)?;
    let mut commands = Vec::new();
    commands.try_reserve_exact(entries.len()).map_err(|_| ProjError::AllocFailed)?;
    let mut cli: BTreeMap<Vec<String>, (String, String)> = BTreeMap::new();
    let mut mcp = BTreeMap::new();
    let mut py = BTreeMap::new();
    for Entry { id, def } in entries {
        let command = id.as_str();
        let DerivedNames { cli_path, mcp_name, py_module, py_name } = derive(command, id.name(), id.major())?;
        let ceiling = McpProjection::for_command(def.side_effect, def.human_only);
        let expected = McpProjection::for_declared(def.effective_projection(), ceiling).ok_or_else(|| {
            ProjError::ProjectionMismatch {
                command: command.to_string(),
                declared: def.mcp_projection.as_str().to_string(),
                expected: ceiling,
            }
        })?;
        // Majors of one command share their CLI path by design (AUTO.CLI.TREE groups them), so
        // a CLI path collides only between different command names.
        match cli.get(&cli_path) {
            Some((name, first)) if name != id.name() => {
                return Err(ProjError::NameCollision {
                    kind: NameKind::Cli,
                    name: cli_path.join(" "),
                    first: String::clone(first),
                    second: command.to_string(),
                });
            }
            Some(_) => {}
            None => {
                cli.insert(cli_path.clone(), (id.name().to_string(), command.to_string()));
            }
        }
        claim(&mut mcp, NameKind::Mcp, mcp_name.clone(), command)?;
        claim(&mut py, NameKind::Python, format!("{py_module}.{py_name}"), command)?;
        let params = params_of(&def.params_schema, command)?;
        let nested_path_like = count_path_like(&def.params_schema) != params.iter().filter(|p| p.path_like).count();
        let result = match &def.result_schema {
            CValue::Obj(o) => type_of(o, &format!("{command}: result_schema"))?,
            _ => return Err(parse_err(format!("{command}: result_schema must be an object"))),
        };
        commands.push(SurfaceProjection {
            major: id.major(),
            cli_path,
            mcp_name,
            py_module,
            py_name,
            mcp_projection: expected,
            side_effect: def.side_effect,
            determinism: def.determinism,
            human_only: def.human_only,
            params,
            result,
            long_running: false,
            dry_run: false,
            help: DocText { title: def.title, description: def.description },
            effects: def.effects,
            nested_path_like,
            command_id: id,
        });
    }
    Ok(Projection { manifest_digest, commands })
}

fn s(x: &str) -> CValue {
    CValue::Str(x.to_string())
}

fn opt(x: &Option<String>) -> CValue {
    x.as_deref().map_or(CValue::Null, s)
}

impl ParamSpec {
    /// The JSON value; `default` is omitted when not declared (a declared `null` is kept).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("name".into(), s(&self.name));
        o.insert("type".into(), s(&self.ty));
        o.insert("quantity_kind".into(), opt(&self.quantity_kind));
        o.insert("unit".into(), opt(&self.unit));
        if let Some(d) = &self.default {
            o.insert("default".into(), d.clone());
        }
        o.insert("required".into(), CValue::Bool(self.required));
        o.insert("path_like".into(), CValue::Bool(self.path_like));
        CValue::Obj(o)
    }
}

impl SurfaceProjection {
    /// The JSON value.
    pub fn to_value(&self) -> CValue {
        let mut help = BTreeMap::new();
        help.insert("title".into(), s(&self.help.title));
        help.insert("description".into(), s(&self.help.description));
        let mut result = BTreeMap::new();
        result.insert("type".into(), s(&self.result));
        let mut o = BTreeMap::new();
        o.insert("command_id".into(), s(self.command_id.as_str()));
        o.insert("major".into(), CValue::Num(f64::from(self.major)));
        o.insert("cli_path".into(), CValue::Arr(self.cli_path.iter().map(String::as_str).map(s).collect()));
        o.insert("mcp_name".into(), s(&self.mcp_name));
        o.insert("py_module".into(), s(&self.py_module));
        o.insert("py_name".into(), s(&self.py_name));
        o.insert("mcp_projection".into(), s(self.mcp_projection.as_str()));
        o.insert("side_effect".into(), s(self.side_effect.as_str()));
        o.insert("determinism".into(), s(self.determinism.as_str()));
        o.insert("human_only".into(), CValue::Bool(self.human_only));
        o.insert("params".into(), CValue::Arr(self.params.iter().map(ParamSpec::to_value).collect()));
        o.insert("result".into(), CValue::Obj(result));
        o.insert("long_running".into(), CValue::Bool(self.long_running));
        o.insert("dry_run".into(), CValue::Bool(self.dry_run));
        o.insert("help".into(), CValue::Obj(help));
        CValue::Obj(o)
    }
}

impl Projection {
    /// The JSON value: `{ "commands": [...], "manifest_digest": "sha256:..." }`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("manifest_digest".into(), s(self.manifest_digest.as_str()));
        o.insert("commands".into(), CValue::Arr(self.commands.iter().map(SurfaceProjection::to_value).collect()));
        CValue::Obj(o)
    }

    /// RFC 8785 canonical bytes of [`Projection::to_value`] (REQ-008).
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, ProjError> {
        to_canonical_bytes(&self.to_value()).map_err(|e| parse_err(format!("serialise: {}", e.code())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_table_req005() {
        use McpProjection as P;
        let want = [
            ("R0", P::Exposed),
            ("R1", P::Exposed),
            ("W1", P::Named),
            ("W2", P::Named),
            ("W3", P::RequestOnly),
            ("X1", P::RequestOnly),
            ("I1", P::Exposed),
            ("D1", P::RequestOnly),
            ("M1", P::None),
            ("S1", P::None),
            ("A0", P::None),
        ];
        assert_eq!(want.len(), SideEffect::ALL.len());
        for (class, p) in want {
            let c = SideEffect::parse(class);
            assert_eq!(c.map(McpProjection::from_class), Ok(p), "{class}");
        }
    }

    #[test]
    fn human_only_tightens_never_loosens() {
        assert_eq!(McpProjection::for_command(SideEffect::W2, true), McpProjection::RequestOnly);
        assert_eq!(McpProjection::for_command(SideEffect::R0, true), McpProjection::RequestOnly);
        assert_eq!(McpProjection::for_command(SideEffect::W3, true), McpProjection::RequestOnly);
        assert_eq!(McpProjection::for_command(SideEffect::M1, true), McpProjection::None);
        assert_eq!(McpProjection::for_command(SideEffect::W2, false), McpProjection::Named);
    }

    #[test]
    fn type_of_shapes() {
        let mut o = BTreeMap::new();
        assert_eq!(type_of(&o, "x"), Ok("any".to_string()));
        o.insert("$ref".to_string(), s("#/defs/frame"));
        assert_eq!(type_of(&o, "x"), Ok("#/defs/frame".to_string()));
        o.insert("type".to_string(), CValue::Arr(vec![s("string"), s("null"), s("string")]));
        assert_eq!(type_of(&o, "x"), Ok("null|string".to_string()));
        o.insert("type".to_string(), CValue::Num(1.0));
        assert_eq!(type_of(&o, "x").map_err(|e| e.code()), Err("manifest_parse"));
        o.insert("type".to_string(), CValue::Arr(Vec::new()));
        assert_eq!(type_of(&o, "x").map_err(|e| e.code()), Err("manifest_parse"));
    }

    #[test]
    fn garbage_is_refused() {
        for bad in [&b""[..], b"[]", b"{}", b"null", b"{\"commands\":[],\"format\":1}", b"\xff"] {
            assert_eq!(project(bad).map(|_| ()).map_err(|e| e.code()), Err("manifest_parse"), "{bad:?}");
        }
        let empty = br#"{"commands":[],"format":1,"quantity_catalogue_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}"#;
        assert_eq!(project(empty).map(|p| p.commands.len()), Ok(0));
        // Not canonical: whitespace.
        let spaced = br#"{"commands":[], "format":1,"quantity_catalogue_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}"#;
        assert_eq!(project(spaced).map(|_| ()).map_err(|e| e.code()), Err("manifest_parse"));
        // Format 2.
        let f2 = br#"{"commands":[],"format":2,"quantity_catalogue_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}"#;
        assert_eq!(project(f2).map(|_| ()).map_err(|e| e.code()), Err("manifest_parse"));
    }
}

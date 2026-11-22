//! Generated command reference records (`docs-ref-v1`). Spec node `AUTO.SURF.DOCS`.
//!
//! Pure `SurfaceEmitter` for surface `docs`: emits one canonical JSON reference record
//! `ref/<command_id>@<major>.json` per command major in the projection, one per-domain index
//! `index/<domain>.json`, and the side-effect class legend `classes.json`.
//!
//! No prose is added beyond projection values and the fixed field names and closed reason codes
//! of `docs-ref-v1` (REQ-AUTO.SURF.DOCS-004). Help text (`summary` from `DocText.title` and
//! `help` from `DocText.description`) is copied verbatim from the projection (REQ-003).
//!
//! Determinism D0: pure function of the [`Projection`], ordered maps only, RFC 8785 JSON.

use std::collections::BTreeMap;
use std::fmt;

use fid_canon::{to_canonical_bytes, CValue};
use fid_surface::{McpProjection, ParamSpec, Projection, SurfaceProjection};
use fid_types::{Determinism, Digest, SideEffect};

use crate::{CommentSyntax, EmitError, GeneratedFile, SurfaceEmitter};

/// Surface name owned by [`DocsEmitter`].
pub const DOCS_SURFACE: &str = "docs";
/// Schema identifier carried in every reference record (`RefRecord.schema`).
pub const DOCS_SCHEMA: &str = "docs-ref-v1";
/// Relative directory for per-command reference records.
pub const REF_DIR: &str = "ref";
/// Relative directory for per-domain command index records.
pub const INDEX_DIR: &str = "index";
/// Relative path for the side-effect class legend record.
pub const CLASSES_FILE: &str = "classes.json";

/// Closed set of MCP reachability reason codes (REQ-AUTO.SURF.DOCS-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReasonCode {
    /// Side-effect class (M1, S1, A0) or declaration is never exposed on MCP.
    ClassNeverOnMcp,
    /// Agents may only file a request for a human (W3, X1, D1, or stricter declaration).
    RequestOnly,
    /// Command is marked `human_only: true` (D-031), overriding `exposed` or `named`.
    HumanOnly,
}

impl ReasonCode {
    /// Every reason code, in spec order.
    pub const ALL: [ReasonCode; 3] = [ReasonCode::ClassNeverOnMcp, ReasonCode::RequestOnly, ReasonCode::HumanOnly];

    /// Wire string in `docs-ref-v1`.
    pub fn as_str(self) -> &'static str {
        match self {
            ReasonCode::ClassNeverOnMcp => "class_never_on_mcp",
            ReasonCode::RequestOnly => "request_only",
            ReasonCode::HumanOnly => "human_only",
        }
    }
}

impl fmt::Display for ReasonCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Reachability entry for one surface (`cli`, `mcp`, or `python`) inside [`RefRecord`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceDoc {
    pub available: bool,
    pub name: Option<String>,
    pub projection: Option<McpProjection>,
    pub reason: Option<ReasonCode>,
}

impl SurfaceDoc {
    /// Canonical JSON object representation.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("available".to_string(), CValue::Bool(self.available));
        if let Some(name) = &self.name {
            o.insert("name".to_string(), CValue::Str(name.clone()));
        }
        if let Some(proj) = self.projection {
            o.insert("projection".to_string(), CValue::Str(proj.as_str().to_string()));
        }
        if let Some(reason) = self.reason {
            o.insert("reason".to_string(), CValue::Str(reason.as_str().to_string()));
        }
        CValue::Obj(o)
    }
}

/// One parameter entry in [`RefRecord`].
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDoc {
    pub name: String,
    pub ty: String,
    pub quantity_kind: Option<String>,
    pub unit: Option<String>,
    pub default: Option<CValue>,
}

impl ParamDoc {
    fn from_spec(spec: &ParamSpec) -> Self {
        Self {
            name: spec.name.clone(),
            ty: spec.ty.clone(),
            quantity_kind: spec.quantity_kind.clone(),
            unit: spec.unit.clone(),
            default: spec.default.clone(),
        }
    }

    /// Canonical JSON object representation.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("default".to_string(), self.default.clone().unwrap_or(CValue::Null));
        o.insert("name".to_string(), CValue::Str(self.name.clone()));
        o.insert("quantity_kind".to_string(), opt_str(&self.quantity_kind));
        o.insert("ty".to_string(), CValue::Str(self.ty.clone()));
        o.insert("unit".to_string(), opt_str(&self.unit));
        CValue::Obj(o)
    }
}

/// Structured `docs-ref-v1` reference record for one command major.
#[derive(Debug, Clone, PartialEq)]
pub struct RefRecord {
    pub schema: &'static str,
    pub manifest_digest: Digest,
    pub command_digest: Digest,
    /// Bare command name without `@major` (e.g. `align.run_bestfit`).
    pub command_id: String,
    pub major: u32,
    /// Verbatim copy of `DocText.title`.
    pub summary: String,
    /// Verbatim copy of `DocText.description` (REQ-AUTO.SURF.DOCS-003).
    pub help: String,
    pub params: Vec<ParamDoc>,
    pub result: String,
    pub side_effect: SideEffect,
    pub determinism: Determinism,
    pub human_only: bool,
    pub long_running: bool,
    pub dry_run: bool,
    pub cli: SurfaceDoc,
    pub mcp: SurfaceDoc,
    pub python: SurfaceDoc,
}

impl RefRecord {
    /// Relative path under the `docs` surface directory (`ref/<command_id>@<major>.json`).
    pub fn rel_path(&self) -> String {
        format!("{REF_DIR}/{}@{}.json", self.command_id, self.major)
    }

    /// Canonical JSON object representation.
    pub fn to_value(&self) -> CValue {
        let mut surfaces = BTreeMap::new();
        surfaces.insert("cli".to_string(), self.cli.to_value());
        surfaces.insert("mcp".to_string(), self.mcp.to_value());
        surfaces.insert("python".to_string(), self.python.to_value());

        let mut o = BTreeMap::new();
        o.insert("command_digest".to_string(), CValue::Str(self.command_digest.as_str().to_string()));
        o.insert("command_id".to_string(), CValue::Str(self.command_id.clone()));
        o.insert("determinism".to_string(), CValue::Str(self.determinism.as_str().to_string()));
        o.insert("dry_run".to_string(), CValue::Bool(self.dry_run));
        o.insert("help".to_string(), CValue::Str(self.help.clone()));
        o.insert("human_only".to_string(), CValue::Bool(self.human_only));
        o.insert("long_running".to_string(), CValue::Bool(self.long_running));
        o.insert("major".to_string(), CValue::Num(f64::from(self.major)));
        o.insert("manifest_digest".to_string(), CValue::Str(self.manifest_digest.as_str().to_string()));
        o.insert("params".to_string(), CValue::Arr(self.params.iter().map(ParamDoc::to_value).collect()));
        o.insert("result".to_string(), CValue::Str(self.result.clone()));
        o.insert("schema".to_string(), CValue::Str(self.schema.to_string()));
        o.insert("side_effect".to_string(), CValue::Str(self.side_effect.as_str().to_string()));
        o.insert("summary".to_string(), CValue::Str(self.summary.clone()));
        o.insert("surfaces".to_string(), CValue::Obj(surfaces));
        CValue::Obj(o)
    }
}

/// One entry inside [`DomainIndex`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRefEntry {
    pub command_id: String,
    pub major: u32,
    pub ref_path: String,
    pub summary: String,
}

impl CommandRefEntry {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("command_id".to_string(), CValue::Str(self.command_id.clone()));
        o.insert("major".to_string(), CValue::Num(f64::from(self.major)));
        o.insert("ref_path".to_string(), CValue::Str(self.ref_path.clone()));
        o.insert("summary".to_string(), CValue::Str(self.summary.clone()));
        CValue::Obj(o)
    }
}

/// Per-domain index record (`index/<domain>.json`, REQ-AUTO.SURF.DOCS-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainIndex {
    pub domain: String,
    pub manifest_digest: Digest,
    pub commands: Vec<CommandRefEntry>,
}

impl DomainIndex {
    /// Relative path under the `docs` surface directory (`index/<domain>.json`).
    pub fn rel_path(&self) -> String {
        format!("{INDEX_DIR}/{}.json", self.domain)
    }

    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("commands".to_string(), CValue::Arr(self.commands.iter().map(CommandRefEntry::to_value).collect()));
        o.insert("domain".to_string(), CValue::Str(self.domain.clone()));
        o.insert("manifest_digest".to_string(), CValue::Str(self.manifest_digest.as_str().to_string()));
        CValue::Obj(o)
    }
}

/// One entry in [`ClassLegend`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassLegendEntry {
    pub code: SideEffect,
    pub mcp_projection: McpProjection,
}

impl ClassLegendEntry {
    pub fn to_value(self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code.as_str().to_string()));
        o.insert("mcp_projection".to_string(), CValue::Str(self.mcp_projection.as_str().to_string()));
        CValue::Obj(o)
    }
}

/// Side-effect class legend record (`classes.json`, REQ-AUTO.SURF.DOCS-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassLegend {
    pub classes: Vec<ClassLegendEntry>,
}

impl ClassLegend {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert(
            "classes".to_string(),
            CValue::Arr(self.classes.iter().copied().map(ClassLegendEntry::to_value).collect()),
        );
        CValue::Obj(o)
    }
}

/// Errors produced by [`DocsEmitter`] (stable codes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocsError {
    /// A command's `DocText` (`title` or `description`) is empty (AC-6).
    MissingHelp { command: String },
    /// Canonical JSON serialisation failed.
    CanonicalEncode { detail: String },
}

impl DocsError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            DocsError::MissingHelp { .. } => "missing_help",
            DocsError::CanonicalEncode { .. } => "emit_failed",
        }
    }
}

impl fmt::Display for DocsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocsError::MissingHelp { command } => write!(f, "missing_help: {command}"),
            DocsError::CanonicalEncode { detail } => write!(f, "emit_failed: {detail}"),
        }
    }
}

impl std::error::Error for DocsError {}

fn opt_str(v: &Option<String>) -> CValue {
    match v {
        Some(s) => CValue::Str(s.clone()),
        None => CValue::Null,
    }
}

fn has_exposure(cmd: &SurfaceProjection, surface: &str) -> bool {
    cmd.exposure.iter().any(|s| s == surface)
}

fn mcp_surface_doc(cmd: &SurfaceProjection) -> SurfaceDoc {
    if cmd.mcp_projection == McpProjection::None || !has_exposure(cmd, "mcp") {
        return SurfaceDoc {
            available: false,
            name: None,
            projection: Some(McpProjection::None),
            reason: Some(ReasonCode::ClassNeverOnMcp),
        };
    }
    if cmd.human_only {
        return SurfaceDoc {
            available: true,
            name: Some(cmd.mcp_name.clone()),
            projection: Some(cmd.mcp_projection),
            reason: Some(ReasonCode::HumanOnly),
        };
    }
    if cmd.mcp_projection == McpProjection::RequestOnly {
        return SurfaceDoc {
            available: true,
            name: Some(cmd.mcp_name.clone()),
            projection: Some(McpProjection::RequestOnly),
            reason: Some(ReasonCode::RequestOnly),
        };
    }
    SurfaceDoc { available: true, name: Some(cmd.mcp_name.clone()), projection: Some(cmd.mcp_projection), reason: None }
}

/// Builds a single `docs-ref-v1` [`RefRecord`] from a [`SurfaceProjection`].
///
/// Fails with [`DocsError::MissingHelp`] if either `DocText.title` or `DocText.description` is
/// empty or whitespace-only (AC-6).
pub fn build_ref_record(p: &Projection, cmd: &SurfaceProjection) -> Result<RefRecord, DocsError> {
    if cmd.help.title.trim().is_empty() || cmd.help.description.trim().is_empty() {
        return Err(DocsError::MissingHelp { command: cmd.command_id.as_str().to_string() });
    }
    let cli = if has_exposure(cmd, "cli") {
        SurfaceDoc { available: true, name: Some(cmd.cli_path.join(" ")), projection: None, reason: None }
    } else {
        SurfaceDoc { available: false, name: None, projection: None, reason: None }
    };
    let python = if has_exposure(cmd, "python") {
        SurfaceDoc {
            available: true,
            name: Some(format!("{}.{}", cmd.py_module, cmd.py_name)),
            projection: None,
            reason: None,
        }
    } else {
        SurfaceDoc { available: false, name: None, projection: None, reason: None }
    };
    let mcp = mcp_surface_doc(cmd);

    Ok(RefRecord {
        schema: DOCS_SCHEMA,
        manifest_digest: p.manifest_digest,
        command_digest: cmd.command_digest,
        command_id: cmd.command_id.name().to_string(),
        major: cmd.major,
        summary: cmd.help.title.clone(),
        help: cmd.help.description.clone(),
        params: cmd.params.iter().map(ParamDoc::from_spec).collect(),
        result: cmd.result.clone(),
        side_effect: cmd.side_effect,
        determinism: cmd.determinism,
        human_only: cmd.human_only,
        long_running: cmd.long_running,
        dry_run: cmd.dry_run,
        cli,
        mcp,
        python,
    })
}

/// Builds per-domain index records in domain byte order, with each domain's commands in
/// `(command_id bytes, major)` order (REQ-AUTO.SURF.DOCS-006).
pub fn build_domain_indexes(manifest_digest: Digest, records: &[RefRecord]) -> Vec<DomainIndex> {
    let mut by_domain: BTreeMap<String, Vec<CommandRefEntry>> = BTreeMap::new();
    for rec in records {
        let domain = rec.command_id.split('.').next().unwrap_or(&rec.command_id).to_string();
        by_domain.entry(domain).or_default().push(CommandRefEntry {
            command_id: rec.command_id.clone(),
            major: rec.major,
            ref_path: rec.rel_path(),
            summary: rec.summary.clone(),
        });
    }
    by_domain
        .into_iter()
        .map(|(domain, mut commands)| {
            commands.sort_by(|a, b| a.command_id.as_bytes().cmp(b.command_id.as_bytes()).then(a.major.cmp(&b.major)));
            DomainIndex { domain, manifest_digest, commands }
        })
        .collect()
}

/// Builds the fixed CONSTITUTION §3 side-effect class legend (REQ-AUTO.SURF.DOCS-006).
pub fn build_class_legend() -> ClassLegend {
    let classes = SideEffect::ALL
        .iter()
        .copied()
        .map(|code| ClassLegendEntry { code, mcp_projection: McpProjection::from_class(code) })
        .collect();
    ClassLegend { classes }
}

fn str_arr(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| CValue::Str((*s).to_string())).collect())
}

fn obj(entries: Vec<(&str, CValue)>) -> CValue {
    let mut m = BTreeMap::new();
    for (k, v) in entries {
        m.insert(k.to_string(), v);
    }
    CValue::Obj(m)
}

/// Generates the JSON Schema Draft 2020-12 definition for `docs-ref-v1` directly from the Rust
/// types and closed enums (`SideEffect`, `Determinism`, `McpProjection`, `ReasonCode`).
pub fn docs_ref_v1_schema() -> CValue {
    let side_effects: Vec<&str> = SideEffect::ALL.iter().map(|c| c.as_str()).collect();
    let determinisms: Vec<&str> = Determinism::ALL.iter().map(|d| d.as_str()).collect();
    let projections: Vec<&str> = McpProjection::ALL.iter().map(|p| p.as_str()).collect();
    let reasons: Vec<&str> = ReasonCode::ALL.iter().map(|r| r.as_str()).collect();

    let digest_def = obj(vec![
        ("pattern", CValue::Str("^sha256:[0-9a-f]{64}$".to_string())),
        ("type", CValue::Str("string".to_string())),
    ]);

    let surface_def = obj(vec![
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj(vec![
                ("available", obj(vec![("type", CValue::Str("boolean".to_string()))])),
                ("name", obj(vec![("minLength", CValue::Num(1.0)), ("type", CValue::Str("string".to_string()))])),
                ("projection", obj(vec![("enum", str_arr(&projections))])),
                ("reason", obj(vec![("enum", str_arr(&reasons))])),
            ]),
        ),
        ("required", str_arr(&["available"])),
        ("type", CValue::Str("object".to_string())),
    ]);

    let param_def = obj(vec![
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj(vec![
                ("default", obj(vec![])),
                ("name", obj(vec![("minLength", CValue::Num(1.0)), ("type", CValue::Str("string".to_string()))])),
                ("quantity_kind", obj(vec![("type", str_arr(&["string", "null"]))])),
                ("ty", obj(vec![("minLength", CValue::Num(1.0)), ("type", CValue::Str("string".to_string()))])),
                ("unit", obj(vec![("type", str_arr(&["string", "null"]))])),
            ]),
        ),
        ("required", str_arr(&["default", "name", "quantity_kind", "ty", "unit"])),
        ("type", CValue::Str("object".to_string())),
    ]);

    let props = obj(vec![
        ("_header", obj(vec![("minLength", CValue::Num(1.0)), ("type", CValue::Str("string".to_string()))])),
        ("command_digest", obj(vec![("$ref", CValue::Str("#/$defs/digest".to_string()))])),
        (
            "command_id",
            obj(vec![
                ("pattern", CValue::Str("^[a-z0-9]([a-z0-9_.]*[a-z0-9])?$".to_string())),
                ("type", CValue::Str("string".to_string())),
            ]),
        ),
        ("determinism", obj(vec![("enum", str_arr(&determinisms))])),
        ("dry_run", obj(vec![("type", CValue::Str("boolean".to_string()))])),
        ("help", obj(vec![("minLength", CValue::Num(1.0)), ("type", CValue::Str("string".to_string()))])),
        ("human_only", obj(vec![("type", CValue::Str("boolean".to_string()))])),
        ("long_running", obj(vec![("type", CValue::Str("boolean".to_string()))])),
        (
            "major",
            obj(vec![
                ("maximum", CValue::Num(99_999.0)),
                ("minimum", CValue::Num(0.0)),
                ("type", CValue::Str("integer".to_string())),
            ]),
        ),
        ("manifest_digest", obj(vec![("$ref", CValue::Str("#/$defs/digest".to_string()))])),
        (
            "params",
            obj(vec![
                ("items", obj(vec![("$ref", CValue::Str("#/$defs/param".to_string()))])),
                ("type", CValue::Str("array".to_string())),
            ]),
        ),
        ("result", obj(vec![("type", str_arr(&["string", "object"]))])),
        ("schema", obj(vec![("const", CValue::Str(DOCS_SCHEMA.to_string()))])),
        ("side_effect", obj(vec![("enum", str_arr(&side_effects))])),
        ("summary", obj(vec![("minLength", CValue::Num(1.0)), ("type", CValue::Str("string".to_string()))])),
        (
            "surfaces",
            obj(vec![
                ("additionalProperties", CValue::Bool(false)),
                (
                    "properties",
                    obj(vec![
                        ("cli", obj(vec![("$ref", CValue::Str("#/$defs/surface".to_string()))])),
                        ("mcp", obj(vec![("$ref", CValue::Str("#/$defs/surface".to_string()))])),
                        ("python", obj(vec![("$ref", CValue::Str("#/$defs/surface".to_string()))])),
                    ]),
                ),
                ("required", str_arr(&["cli", "mcp", "python"])),
                ("type", CValue::Str("object".to_string())),
            ]),
        ),
    ]);

    obj(vec![
        ("$defs", obj(vec![("digest", digest_def), ("param", param_def), ("surface", surface_def)])),
        ("$id", CValue::Str("https://metrologymaven.io/schema/docs-ref-v1.json".to_string())),
        ("$schema", CValue::Str("https://json-schema.org/draft/2020-12/schema".to_string())),
        ("additionalProperties", CValue::Bool(false)),
        ("properties", props),
        (
            "required",
            str_arr(&[
                "command_digest",
                "command_id",
                "determinism",
                "dry_run",
                "help",
                "human_only",
                "long_running",
                "major",
                "manifest_digest",
                "params",
                "result",
                "schema",
                "side_effect",
                "summary",
                "surfaces",
            ]),
        ),
        ("title", CValue::Str("docs-ref-v1 command reference record".to_string())),
        ("type", CValue::Str("object".to_string())),
    ])
}

/// Canonical RFC 8785 JSON bytes (with trailing LF) of `schema/docs-ref-v1.json`.
pub fn docs_ref_v1_schema_bytes() -> Result<Vec<u8>, DocsError> {
    let mut bytes = to_canonical_bytes(&docs_ref_v1_schema())
        .map_err(|e| DocsError::CanonicalEncode { detail: e.code().to_string() })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn encode_canonical(v: &CValue) -> Result<Vec<u8>, EmitError> {
    to_canonical_bytes(v).map_err(|e| EmitError { detail: format!("emit_failed: {}", e.code()) })
}

/// The `docs` surface emitter (`AUTO.SURF.DOCS`).
#[derive(Debug, Clone, Copy, Default)]
pub struct DocsEmitter;

impl SurfaceEmitter for DocsEmitter {
    fn surface(&self) -> &'static str {
        DOCS_SURFACE
    }

    fn emit(&self, p: &Projection) -> Result<Vec<GeneratedFile>, EmitError> {
        let mut records = Vec::with_capacity(p.commands.len());
        for cmd in &p.commands {
            let rec = build_ref_record(p, cmd).map_err(|e| EmitError { detail: e.to_string() })?;
            records.push((rec, cmd.command_id.clone()));
        }

        let plain_records: Vec<RefRecord> = records.iter().map(|(r, _)| r.clone()).collect();
        let indexes = build_domain_indexes(p.manifest_digest, &plain_records);
        let legend = build_class_legend();

        let mut files = Vec::with_capacity(records.len() + indexes.len() + 1);
        for (rec, cmd_id) in records {
            files.push(GeneratedFile {
                rel_path: rec.rel_path(),
                bytes: encode_canonical(&rec.to_value())?,
                covers: vec![cmd_id],
            });
        }
        for idx in indexes {
            files.push(GeneratedFile {
                rel_path: idx.rel_path(),
                bytes: encode_canonical(&idx.to_value())?,
                covers: Vec::new(),
            });
        }
        files.push(GeneratedFile {
            rel_path: CLASSES_FILE.to_string(),
            bytes: encode_canonical(&legend.to_value())?,
            covers: Vec::new(),
        });
        Ok(files)
    }

    fn comment_syntax(&self, _path: &str) -> CommentSyntax {
        CommentSyntax::JsonMember
    }
}

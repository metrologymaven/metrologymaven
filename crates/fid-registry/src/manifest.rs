//! The canonical Registry Manifest (spec node `CORE.REG.MAN`, REQ-CORE.REG-005..007).
//!
//! [`export_with`] turns command declarations into one RFC 8785 document
//! `{"format": 1, "quantity_catalogue_digest": "sha256:..", "commands": [..]}` and its
//! digest. The steps are fixed:
//!
//! 1. at most [`MAX_COMMANDS`] declarations, else `too_many_commands`;
//! 2. every declaration is validated by `CORE.REG.VAL` ([`validate_with`]) against the
//!    quantity catalogue that is passed in, with the default [`Limits`]. There is no way to
//!    skip this: any error (a duplicate (ID, major), a tool-name collision, an unknown
//!    quantity kind, ...) fails the export as `declarations_invalid` with VAL's sorted error
//!    list;
//! 3. each declaration is parsed as a [`CommandDef`] and written in its normal form
//!    ([`CommandDef::to_value`]: optional fields written out, `exposure` in surface order),
//!    plus `definition_digest` = SHA-256 of the RFC 8785 bytes of that normal form
//!    (REQ-004);
//! 4. definitions sorted by ID (byte order), then major;
//! 5. the document is serialised with `fid-canon` and the manifest digest is the SHA-256 of
//!    exactly those bytes (REQ-002). Nothing from the environment goes in (REQ-003).
//!
//! The registry defines no quantity kind: the catalogue is an input, and its digest is the
//! `quantity_catalogue_digest` field. [`export`] uses `Catalogue::geo_frame()`.
//!
//! [`check_manifest`] verifies a supplied manifest (`manifest_invalid`), and
//! [`command_def`] / [`show_manifest`] / [`invoke_cli`] / [`invoke_mcp`] are the R0 command
//! `registry.show_manifest@1` (REQ-006).

use crate::catalogue::{Catalogue, QuantityCatalogue};
use crate::def::CommandDef;
use crate::validate::{validate_with, CommandDecl, Limits, ValidationError};
use fid_canon::{digest_bytes, parse, parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// The manifest format version written in `format`.
pub const MANIFEST_FORMAT: u32 = 1;
/// Most declarations one export takes.
pub const MAX_COMMANDS: usize = 50_000;
/// Largest manifest, exported or checked, in bytes (`fid-canon`'s parse limit, so every
/// exported manifest can be read back).
pub const MAX_MANIFEST_BYTES: usize = fid_canon::MAX_INPUT_BYTES;
/// Command ID of the show command.
pub const SHOW_MANIFEST: &str = "registry.show_manifest@1";
/// CLI form: `fid registry show-manifest`.
pub const CLI_SHOW: &str = "show-manifest";
/// MCP tool name.
pub const MCP_SHOW: &str = "registry_show_manifest";
/// Largest MCP parameter payload, in bytes (room for a `check` manifest).
pub const MAX_PARAMS_BYTES: usize = MAX_MANIFEST_BYTES;
/// Deepest MCP parameter nesting.
pub const MAX_PARAMS_DEPTH: usize = 4;
/// Most CLI arguments after `fid registry`.
const MAX_CLI_ARGS: usize = 8;
const LIFECYCLE_SINCE: &str = "2026-11-19";
const DIGEST_PATTERN: &str = "^sha256:[0-9a-f]{64}$";
const TOP_FIELDS: [&str; 3] = ["commands", "format", "quantity_catalogue_digest"];

/// Why an export, a check or a show call failed. Messages never echo input text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// The declarations failed `CORE.REG.VAL`; VAL's sorted errors (`declarations_invalid`).
    DeclarationsInvalid(Vec<ValidationError>),
    /// More than [`MAX_COMMANDS`] declarations (`too_many_commands`).
    TooManyCommands(usize),
    /// The exported manifest would exceed [`MAX_MANIFEST_BYTES`] (`manifest_too_large`).
    TooLarge(usize),
    /// The built-in catalogue could not be built; the catalogue's code (`catalogue_invalid`).
    Catalogue(&'static str),
    /// A value could not be canonicalised; the `fid-canon` code (`canonical_json`).
    Canonical(&'static str),
    /// A supplied manifest is not a valid manifest (`manifest_invalid`).
    Invalid(String),
    /// Command parameters or CLI arguments are malformed (`bad_params`).
    BadParams(&'static str),
    /// An unknown CLI subcommand or MCP tool (`unknown_command`).
    UnknownCommand,
}

impl ManifestError {
    /// Stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            ManifestError::DeclarationsInvalid(_) => "declarations_invalid",
            ManifestError::TooManyCommands(_) => "too_many_commands",
            ManifestError::TooLarge(_) => "manifest_too_large",
            ManifestError::Catalogue(_) => "catalogue_invalid",
            ManifestError::Canonical(_) => "canonical_json",
            ManifestError::Invalid(_) => "manifest_invalid",
            ManifestError::BadParams(_) => "bad_params",
            ManifestError::UnknownCommand => "unknown_command",
        }
    }

    /// JSON form `{"code", "message"}`, plus `"errors"` (VAL's list) for
    /// `declarations_invalid`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code().to_string()));
        o.insert("message".to_string(), CValue::Str(self.to_string()));
        if let ManifestError::DeclarationsInvalid(errors) = self {
            o.insert("errors".to_string(), crate::validate::errors_to_value(errors));
        }
        CValue::Obj(o)
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            ManifestError::DeclarationsInvalid(e) => write!(f, "{code}: {} validation error(s)", e.len()),
            ManifestError::TooManyCommands(n) => write!(f, "{code}: {n} declarations, at most {MAX_COMMANDS}"),
            ManifestError::TooLarge(n) => write!(f, "{code}: {n} bytes, at most {MAX_MANIFEST_BYTES}"),
            ManifestError::Catalogue(c) | ManifestError::Canonical(c) => write!(f, "{code}: {c}"),
            ManifestError::Invalid(m) => write!(f, "{code}: {m}"),
            ManifestError::BadParams(m) => write!(f, "{code}: {m}"),
            ManifestError::UnknownCommand => write!(f, "{code}"),
        }
    }
}

impl std::error::Error for ManifestError {}

fn canon(e: CanonError) -> ManifestError {
    ManifestError::Canonical(e.code())
}

fn sha(bytes: &[u8]) -> Digest {
    Digest::from_bytes(digest_bytes(bytes).0)
}

// ------------------------------------------------------------------ export

/// One exported manifest: its bytes, digest, and the digest of each definition by (ID, major).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub bytes: Vec<u8>,
    pub digest: Digest,
    pub definitions: BTreeMap<(String, u32), Digest>,
}

/// [`export_with`] over the `GEO.FRAME.UNITS` catalogue ([`Catalogue::geo_frame`]).
pub fn export(decls: &[CommandDecl]) -> Result<(Vec<u8>, Digest), ManifestError> {
    let catalogue = Catalogue::geo_frame().map_err(|e| ManifestError::Catalogue(e.code()))?;
    export_with(decls, &catalogue).map(|e| (e.bytes, e.digest))
}

/// Validates `decls` against `catalogue` and exports the manifest (see the module docs).
pub fn export_with(decls: &[CommandDecl], catalogue: &Catalogue) -> Result<Export, ManifestError> {
    if decls.len() > MAX_COMMANDS {
        return Err(ManifestError::TooManyCommands(decls.len()));
    }
    let errors = validate_with(decls, &Limits::default(), catalogue);
    if !errors.is_empty() {
        return Err(ManifestError::DeclarationsInvalid(errors));
    }
    let mut commands: BTreeMap<(String, u32), CValue> = BTreeMap::new();
    let mut definitions = BTreeMap::new();
    for decl in decls {
        // VAL's backstop guarantees that a declaration without errors parses; this only
        // keeps the function total.
        let def = CommandDef::from_value(decl.0.clone())
            .map_err(|_| ManifestError::Invalid("a validated declaration did not parse".into()))?;
        let key = (def.id.clone(), def.major);
        let normal = def.to_value();
        let digest = sha(&to_canonical_bytes(&normal).map_err(canon)?);
        let CValue::Obj(mut body) = normal else {
            return Err(ManifestError::Invalid("definition is not an object".into()));
        };
        body.insert("definition_digest".to_string(), CValue::Str(digest.to_string()));
        definitions.insert(key.clone(), digest);
        if commands.insert(key, CValue::Obj(body)).is_some() {
            // VAL reports duplicate (ID, major) first; kept as a guard.
            return Err(ManifestError::Invalid("duplicate command".into()));
        }
    }
    let mut doc = BTreeMap::new();
    doc.insert("format".to_string(), CValue::Num(f64::from(MANIFEST_FORMAT)));
    doc.insert("quantity_catalogue_digest".to_string(), CValue::Str(catalogue.digest().to_string()));
    doc.insert("commands".to_string(), CValue::Arr(commands.into_values().collect()));
    let bytes = to_canonical_bytes(&CValue::Obj(doc)).map_err(canon)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ManifestError::TooLarge(bytes.len()));
    }
    let digest = sha(&bytes);
    Ok(Export { bytes, digest, definitions })
}

// ------------------------------------------------------------------ check

/// Checks a supplied manifest against `catalogue` and returns its digest. Refused as
/// `manifest_invalid`: more than [`MAX_MANIFEST_BYTES`] bytes; not JSON (or a duplicate key);
/// not in RFC 8785 canonical form; a structure outside `manifest.schema.json` (exact top-level
/// fields, `format` 1, a digest string, a `commands` array of objects); a
/// `quantity_catalogue_digest` other than `catalogue`'s; a `definition_digest` that is missing,
/// malformed or does not match; a definition that fails `CORE.REG.VAL`; definitions not
/// strictly sorted by (ID, major); and, as a backstop, any byte difference from re-exporting
/// the same definitions.
pub fn check_manifest(bytes: &[u8], catalogue: &Catalogue) -> Result<Digest, ManifestError> {
    let bad = |m: &str| ManifestError::Invalid(m.to_string());
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(bad("larger than the manifest limit"));
    }
    let value = parse(bytes).map_err(|e| ManifestError::Invalid(format!("json: {}", e.code())))?;
    if to_canonical_bytes(&value).map_err(canon)? != bytes {
        return Err(bad("not RFC 8785 canonical JSON"));
    }
    let CValue::Obj(top) = value else { return Err(bad("root must be an object")) };
    if top.len() != TOP_FIELDS.len() || !TOP_FIELDS.iter().all(|k| top.contains_key(*k)) {
        return Err(bad("root must have exactly commands, format and quantity_catalogue_digest"));
    }
    if top.get("format") != Some(&CValue::Num(f64::from(MANIFEST_FORMAT))) {
        return Err(bad("format must be 1"));
    }
    match top.get("quantity_catalogue_digest") {
        Some(CValue::Str(s)) if Digest::is_valid(s) => {
            if *s != catalogue.digest().to_string() {
                return Err(bad("quantity_catalogue_digest is not the digest of the catalogue in force"));
            }
        }
        _ => return Err(bad("quantity_catalogue_digest must be a sha256 digest")),
    }
    let Some(CValue::Arr(items)) = top.get("commands") else { return Err(bad("commands must be an array")) };
    if items.len() > MAX_COMMANDS {
        return Err(bad("too many commands"));
    }
    let mut decls = Vec::with_capacity(items.len());
    let mut last: Option<(String, u32)> = None;
    for (i, item) in items.iter().enumerate() {
        let CValue::Obj(def) = item else { return Err(ManifestError::Invalid(format!("commands[{i}] not an object"))) };
        let mut body = def.clone();
        let stated = match body.remove("definition_digest") {
            Some(CValue::Str(s)) if Digest::is_valid(&s) => s,
            _ => return Err(ManifestError::Invalid(format!("commands[{i}]: definition_digest missing or malformed"))),
        };
        let body = CValue::Obj(body);
        if sha(&to_canonical_bytes(&body).map_err(canon)?).to_string() != stated {
            return Err(ManifestError::Invalid(format!("commands[{i}]: definition_digest mismatch")));
        }
        let key = match (body_get(&body, "id"), body_get(&body, "major")) {
            (Some(CValue::Str(id)), Some(CValue::Num(m)))
                if m.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(m) =>
            {
                (id.clone(), *m as u32)
            }
            _ => return Err(ManifestError::Invalid(format!("commands[{i}]: id or major malformed"))),
        };
        if last.as_ref().is_some_and(|l| *l >= key) {
            return Err(ManifestError::Invalid(format!("commands[{i}]: not sorted by (id, major) or repeated")));
        }
        last = Some(key);
        decls.push(CommandDecl(body));
    }
    let errors = validate_with(&decls, &Limits::default(), catalogue);
    if let Some(first) = errors.first() {
        return Err(ManifestError::Invalid(format!(
            "{} definition error(s), first {} on a command",
            errors.len(),
            first.rule.as_str()
        )));
    }
    let again =
        export_with(&decls, catalogue).map_err(|e| ManifestError::Invalid(format!("re-export: {}", e.code())))?;
    if again.bytes != bytes {
        return Err(bad("a definition is not in normal form"));
    }
    Ok(again.digest)
}

fn body_get<'v>(v: &'v CValue, key: &str) -> Option<&'v CValue> {
    match v {
        CValue::Obj(o) => o.get(key),
        _ => None,
    }
}

// ------------------------------------------------------------------ schema

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

/// JSON Schema 2020-12 of the manifest; published as `schema/manifest.schema.json` (REQ-005).
/// A command is the `command-def.schema.json` record in normal form (the optional
/// `human_only`, `network`, `cli_bare` written out) plus `definition_digest`. Sorting,
/// uniqueness, the digests themselves and VAL's rules are checked by [`check_manifest`].
pub fn manifest_schema() -> CValue {
    let mut command = match crate::def::command_def_schema() {
        CValue::Obj(o) => o,
        _ => BTreeMap::new(),
    };
    for k in ["$schema", "$id", "title"] {
        command.remove(k);
    }
    if let Some(CValue::Obj(props)) = command.get_mut("properties") {
        props.insert(
            "definition_digest".to_string(),
            obj([("type", text("string")), ("pattern", text(DIGEST_PATTERN))]),
        );
    }
    if let Some(CValue::Arr(req)) = command.get_mut("required") {
        for k in ["cli_bare", "definition_digest", "human_only", "network"] {
            req.push(text(k));
        }
    }
    obj([
        ("$schema", text("https://json-schema.org/draft/2020-12/schema")),
        ("$id", text("https://metrologymaven.io/schema/manifest.schema.json")),
        ("title", text("Metrology Maven registry manifest")),
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&TOP_FIELDS)),
        (
            "properties",
            obj([
                ("format", obj([("const", CValue::Num(f64::from(MANIFEST_FORMAT)))])),
                ("quantity_catalogue_digest", obj([("type", text("string")), ("pattern", text(DIGEST_PATTERN))])),
                ("commands", obj([("type", text("array")), ("items", obj([("$ref", text("#/$defs/command"))]))])),
            ]),
        ),
        ("$defs", obj([("command", CValue::Obj(command))])),
    ])
}

// ------------------------------------------------------------------ registry.show_manifest@1

/// The manifest in force: exported once, shown many times.
#[derive(Debug, Clone)]
pub struct ManifestInForce {
    export: Export,
    catalogue: Catalogue,
}

impl ManifestInForce {
    /// Exports `decls` against `catalogue` ([`export_with`]).
    pub fn new(decls: &[CommandDecl], catalogue: Catalogue) -> Result<ManifestInForce, ManifestError> {
        let export = export_with(decls, &catalogue)?;
        Ok(ManifestInForce { export, catalogue })
    }

    /// The exported bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.export.bytes
    }

    /// The manifest digest.
    pub fn digest(&self) -> Digest {
        self.export.digest
    }

    /// The whole export, with the per-definition digests.
    pub fn export(&self) -> &Export {
        &self.export
    }

    /// The catalogue the manifest was validated against.
    pub fn catalogue(&self) -> &Catalogue {
        &self.catalogue
    }
}

/// Parameters of `registry.show_manifest@1`: `full` (default false) and optional `check`,
/// the text of a manifest to verify instead of showing the one in force.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShowParams {
    pub full: bool,
    pub check: Option<String>,
}

impl ShowParams {
    /// Reads the parameter object; unknown keys and wrong types are `bad_params`.
    pub fn from_value(v: &CValue) -> Result<ShowParams, ManifestError> {
        let CValue::Obj(o) = v else { return Err(ManifestError::BadParams("params is not an object")) };
        if o.keys().any(|k| k != "full" && k != "check") {
            return Err(ManifestError::BadParams("unknown parameter"));
        }
        let full = match o.get("full") {
            None => false,
            Some(CValue::Bool(b)) => *b,
            Some(_) => return Err(ManifestError::BadParams("full is not a boolean")),
        };
        let check = match o.get("check") {
            None => None,
            Some(CValue::Str(s)) if s.len() <= MAX_MANIFEST_BYTES => Some(s.clone()),
            Some(CValue::Str(_)) => return Err(ManifestError::BadParams("check is too long")),
            Some(_) => return Err(ManifestError::BadParams("check is not a string")),
        };
        Ok(ShowParams { full, check })
    }
}

/// The result `{digest, bytes?}`: the digest of the manifest in force (or of the checked
/// manifest, which must pass [`check_manifest`]), and with `full` its text.
pub fn show_manifest(m: &ManifestInForce, p: &ShowParams) -> Result<CValue, ManifestError> {
    let (digest, bytes) = match &p.check {
        Some(text) => (check_manifest(text.as_bytes(), &m.catalogue)?, text.as_str()),
        None => {
            let s = std::str::from_utf8(m.bytes()).map_err(|_| ManifestError::Canonical("invalid_utf8"))?;
            (m.digest(), s)
        }
    };
    let mut o = BTreeMap::new();
    o.insert("digest".to_string(), CValue::Str(digest.to_string()));
    if p.full {
        o.insert("bytes".to_string(), CValue::Str(bytes.to_string()));
    }
    Ok(CValue::Obj(o))
}

/// The surface a call came through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Surface {
    Cli,
    Mcp,
}

/// One Activity Log record of an R0 call (R0 calls never reach the Record Journal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityEntry {
    pub command: &'static str,
    pub surface: Surface,
}

fn run(m: &ManifestInForce, params: &CValue) -> Result<Vec<u8>, ManifestError> {
    let result = show_manifest(m, &ShowParams::from_value(params)?)?;
    to_canonical_bytes(&result).map_err(canon)
}

/// Runs the MCP tool `registry_show_manifest` on raw JSON parameters (at most
/// [`MAX_PARAMS_BYTES`] bytes, [`MAX_PARAMS_DEPTH`] levels, duplicate keys refused) and returns
/// the canonical result bytes. The call is logged to `log`.
pub fn invoke_mcp(
    m: &ManifestInForce,
    log: &mut Vec<ActivityEntry>,
    tool: &str,
    params: &[u8],
) -> Result<Vec<u8>, ManifestError> {
    if tool != MCP_SHOW {
        return Err(ManifestError::UnknownCommand);
    }
    log.push(ActivityEntry { command: SHOW_MANIFEST, surface: Surface::Mcp });
    let value = parse_with_limits(params, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
        .map_err(|_| ManifestError::BadParams("params are not valid JSON within the limits"))?;
    run(m, &value)
}

/// Runs `fid registry show-manifest [--full] [--check <file>] [--json]`. `read` loads the
/// `--check` file (the caller bounds the read; more than [`MAX_MANIFEST_BYTES`] is refused
/// here too). Output is canonical JSON either way. Unknown or repeated options are refused.
/// The call is logged to `log`.
pub fn invoke_cli(
    m: &ManifestInForce,
    log: &mut Vec<ActivityEntry>,
    args: &[&str],
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, ManifestError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(ManifestError::BadParams("too many arguments"));
    }
    let (sub, rest) = args.split_first().ok_or(ManifestError::BadParams("missing subcommand"))?;
    if *sub != CLI_SHOW {
        return Err(ManifestError::UnknownCommand);
    }
    log.push(ActivityEntry { command: SHOW_MANIFEST, surface: Surface::Cli });
    let (mut json, mut full, mut check) = (false, false, None);
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        match *arg {
            "--json" if !json => json = true,
            "--full" if !full => full = true,
            "--check" if check.is_none() => {
                let path = it.next().ok_or(ManifestError::BadParams("--check needs a file"))?;
                let bytes = read(path).ok_or(ManifestError::BadParams("--check file cannot be read"))?;
                if bytes.len() > MAX_MANIFEST_BYTES {
                    return Err(ManifestError::Invalid("larger than the manifest limit".into()));
                }
                let text = String::from_utf8(bytes).map_err(|_| ManifestError::Invalid("json: invalid_utf8".into()))?;
                check = Some(text);
            }
            "--json" | "--full" | "--check" => return Err(ManifestError::BadParams("repeated option")),
            _ => return Err(ManifestError::BadParams("unknown argument")),
        }
    }
    let mut params = BTreeMap::new();
    if full {
        params.insert("full".to_string(), CValue::Bool(true));
    }
    if let Some(text) = check {
        params.insert("check".to_string(), CValue::Str(text));
    }
    run(m, &CValue::Obj(params))
}

/// The registry definition of `registry.show_manifest@1` (R0, MCP exposed, D0).
pub fn command_def() -> Result<CommandDef, crate::def::DefError> {
    let params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                (
                    "full",
                    obj([
                        ("type", text("boolean")),
                        ("default", CValue::Bool(false)),
                        ("description", text("Also return the manifest bytes.")),
                    ]),
                ),
                (
                    "check",
                    obj([
                        ("type", text("string")),
                        ("maxLength", CValue::Num(MAX_MANIFEST_BYTES as f64)),
                        ("description", text("A manifest to verify instead of showing the one in force.")),
                    ]),
                ),
            ]),
        ),
    ]);
    let result = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["digest"])),
        (
            "properties",
            obj([
                ("digest", obj([("type", text("string")), ("pattern", text(DIGEST_PATTERN))])),
                ("bytes", obj([("type", text("string"))])),
            ]),
        ),
    ]);
    CommandDef::from_value(obj([
        ("id", text("registry.show_manifest")),
        ("major", CValue::Num(1.0)),
        ("title", text("Show manifest")),
        ("description", text("Returns the digest of the registry manifest in force and, with full, its canonical bytes; with check, verifies a supplied manifest instead.")),
        ("params_schema", params),
        ("result_schema", result),
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
    ]))
}

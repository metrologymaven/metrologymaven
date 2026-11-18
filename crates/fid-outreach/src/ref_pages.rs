//! Generated command reference pages (spec node `OUTR.DOCS.REF`).
//!
//! Renders the CLI, MCP and Python reference page of every command major from the `docs-ref-v1`
//! records that the registry docs emitter (`AUTO.SURF.DOCS`) writes for one registry manifest,
//! and checks a published page tree against a fresh rendering.
//!
//! Inputs: from the manifest only its SHA-256 digest (of the exact bytes) and `commands[].id` /
//! `major`; from each record the fields named in the spec's input contract; from the surfaces
//! file the list of surfaces the release ships (`surfaces = ["cli", "mcp"]`).
//!
//! Page layout (HTML fragment, `\n` line ends; `{x}` is a record value, HTML-escaped):
//!
//! ```text
//! <article class="ref-page ref-{surface}">
//! <h1><code>{command_id}@{major}</code></h1>
//! <p class="summary">{summary}</p>
//! <dl class="ref-facts">
//! <dt>Command</dt><dd><code>{command_id}@{major}</code></dd>
//! <dt>Side-effect class</dt><dd>{side_effect}</dd>
//! <dt>Determinism</dt><dd>{determinism}</dd>
//! <dt>Human only</dt><dd>{true|false}</dd>
//! <dt>MCP</dt><dd>{surfaces.mcp.projection}</dd>
//! </dl>
//! <pre class="help">{help}</pre>
//! <h2>{CLI|MCP tool|Python}</h2>
//! then one of:
//!   Python page, release without python:
//!     <p class="unavailable">Python bindings are not part of this release.</p>
//!   surface available:
//!     <p><code>{surfaces.<s>.name}</code></p>
//!     [<p class="reason">Reason: {reason in words}</p>]
//!     <h2>Parameters</h2> table (Name, Type, Quantity kind, Unit, Default) and <h2>Result</h2>
//!   surface not available:
//!     <p class="unavailable">This command is not available on {the CLI|MCP|Python}.</p>
//!     [<p class="reason">Reason: {reason in words}</p>]
//! </article>
//! ```
//!
//! Reason words: `class_never_on_mcp` "never on MCP for this class", `request_only`
//! "request-only", `human_only` "human-only". A table cell or result that is not a string is
//! written as its RFC 8785 JSON text; `null` or an absent field is an empty cell. No other text is
//! added (REQ-OUTR.DOCS.REF-008). Pages are written at
//! `<out>/<release>/ref/<cli|mcp|python>/<command_id>@<major>.html` (REQ-OUTR.DOCS.REF-006).
//!
//! Determinism D0: a pure function of the input bytes; ordered maps only.

use fid_canon::CValue;
use fid_types::{CommandName, Determinism, Digest, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::Path;

/// Largest record file, in bytes (records over 1 MB are refused).
pub const MAX_RECORD_BYTES: usize = 1024 * 1024;
/// Largest manifest file, in bytes.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024 * 1024;
/// Largest surfaces file, in bytes.
pub const MAX_SURFACES_BYTES: usize = 64 * 1024;
/// Largest published page read by `ref-check`, in bytes; a larger page is reported as drift.
pub const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024;
/// Most directory entries read from the records directory or the page tree.
pub const MAX_ENTRIES: usize = 100_000;
/// Deepest JSON nesting accepted in a record.
const MAX_RECORD_DEPTH: usize = 64;
/// Longest release tag.
const MAX_RELEASE_LEN: usize = 128;

/// A command major: (command ID without the major, major).
pub type Key = (String, u32);

fn key_text(k: &Key) -> String {
    format!("{}@{}", k.0, k.1)
}

/// The three reference surfaces, in page order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Surface {
    Cli,
    Mcp,
    Python,
}

impl Surface {
    pub const ALL: [Surface; 3] = [Surface::Cli, Surface::Mcp, Surface::Python];

    /// Directory name and `surfaces.toml` value.
    pub fn as_str(self) -> &'static str {
        match self {
            Surface::Cli => "cli",
            Surface::Mcp => "mcp",
            Surface::Python => "python",
        }
    }

    pub fn parse(s: &str) -> Option<Surface> {
        Surface::ALL.into_iter().find(|v| v.as_str() == s)
    }

    fn heading(self) -> &'static str {
        match self {
            Surface::Cli => "CLI",
            Surface::Mcp => "MCP tool",
            Surface::Python => "Python",
        }
    }

    fn unavailable_on(self) -> &'static str {
        match self {
            Surface::Cli => "the CLI",
            Surface::Mcp => "MCP",
            Surface::Python => "Python",
        }
    }

    fn index(self) -> usize {
        match self {
            Surface::Cli => 0,
            Surface::Mcp => 1,
            Surface::Python => 2,
        }
    }
}

/// Reason codes of `docs-ref-v1` (closed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    ClassNeverOnMcp,
    RequestOnly,
    HumanOnly,
}

impl Reason {
    pub fn parse(s: &str) -> Option<Reason> {
        match s {
            "class_never_on_mcp" => Some(Reason::ClassNeverOnMcp),
            "request_only" => Some(Reason::RequestOnly),
            "human_only" => Some(Reason::HumanOnly),
            _ => None,
        }
    }

    /// The reason in words (REQ-OUTR.DOCS.REF-005).
    pub fn words(self) -> &'static str {
        match self {
            Reason::ClassNeverOnMcp => "never on MCP for this class",
            Reason::RequestOnly => "request-only",
            Reason::HumanOnly => "human-only",
        }
    }
}

/// One surface entry of a record.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceDoc {
    pub available: bool,
    pub name: Option<String>,
    pub projection: Option<String>,
    pub reason: Option<Reason>,
}

/// One parameter of a record.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: String,
    pub quantity_kind: CValue,
    pub unit: CValue,
    pub default: CValue,
}

/// One `docs-ref-v1` record, reduced to the fields this leaf reads.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// File name inside the records directory.
    pub file: String,
    pub manifest_digest: String,
    pub command_id: String,
    pub major: u32,
    pub summary: String,
    pub help: String,
    pub params: Vec<Param>,
    pub result: CValue,
    pub side_effect: SideEffect,
    pub determinism: Determinism,
    pub human_only: bool,
    /// Indexed by [`Surface::index`].
    pub surfaces: [SurfaceDoc; 3],
}

impl Record {
    pub fn key(&self) -> Key {
        (self.command_id.clone(), self.major)
    }

    pub fn surface(&self, s: Surface) -> &SurfaceDoc {
        &self.surfaces[s.index()]
    }
}

/// The registry manifest as this leaf reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// `sha256:` digest of the exact file bytes.
    pub digest: String,
    pub commands: BTreeSet<Key>,
}

/// Finding codes. Order is the report order for equal paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    ManifestDigestMismatch,
    RecordMissing,
    PageMissing,
    PageWithoutCommand,
    PageDrift,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::ManifestDigestMismatch => "manifest_digest_mismatch",
            Code::RecordMissing => "record_missing",
            Code::PageMissing => "page_missing",
            Code::PageWithoutCommand => "page_without_command",
            Code::PageDrift => "page_drift",
        }
    }
}

/// One finding. `path` is a record file name (`<id>@<major>.json`) for the record codes, and a
/// page path relative to the page tree (`<surface>/<id>@<major>.html`) for the page codes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub path: String,
    pub code: Code,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code.as_str(), self.path)
    }
}

/// Errors that stop a command (exit 2), plus the digest mismatch, which is fatal for
/// `ref-check` and a refusal (exit 1) for `ref-render`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefError {
    ManifestUnreadable { path: String, reason: String },
    RecordParse { file: String, reason: String },
    SurfacesUnreadable { path: String, reason: String },
    Unreadable { path: String, reason: String },
    Unwritable { path: String, reason: String },
    DigestMismatch { files: Vec<String> },
}

impl RefError {
    pub fn code(&self) -> &'static str {
        match self {
            RefError::ManifestUnreadable { .. } => "manifest_unreadable",
            RefError::RecordParse { .. } => "record_parse",
            RefError::SurfacesUnreadable { .. } => "surfaces_unreadable",
            RefError::Unreadable { .. } => "input_unreadable",
            RefError::Unwritable { .. } => "output_unwritable",
            RefError::DigestMismatch { .. } => "manifest_digest_mismatch",
        }
    }

    fn parts(&self) -> (String, String) {
        match self {
            RefError::ManifestUnreadable { path, reason }
            | RefError::SurfacesUnreadable { path, reason }
            | RefError::Unreadable { path, reason }
            | RefError::Unwritable { path, reason } => (path.clone(), reason.clone()),
            RefError::RecordParse { file, reason } => (file.clone(), reason.clone()),
            RefError::DigestMismatch { files } => {
                (files.join(" "), "record manifest_digest differs from the manifest digest".to_string())
            }
        }
    }

    /// Canonical JSON for `--json` on exit 2.
    pub fn to_json(&self) -> Vec<u8> {
        let (path, detail) = self.parts();
        let mut e = BTreeMap::new();
        e.insert("code".to_string(), CValue::Str(self.code().to_string()));
        e.insert("detail".to_string(), CValue::Str(detail));
        e.insert("path".to_string(), CValue::Str(path));
        let mut o = BTreeMap::new();
        o.insert("error".to_string(), CValue::Obj(e));
        o.insert("ok".to_string(), CValue::Bool(false));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }
}

impl fmt::Display for RefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (path, reason) = self.parts();
        write!(f, "{} {path}: {reason}", self.code())
    }
}

impl std::error::Error for RefError {}

fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    f.take(max as u64 + 1).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() > max {
        return Err(format!("larger than {max} bytes"));
    }
    Ok(buf)
}

fn major_of(v: Option<&CValue>) -> Option<u32> {
    match v {
        Some(CValue::Num(n)) if n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(n) => Some(*n as u32),
        _ => None,
    }
}

/// Parses the manifest bytes: digest of the exact bytes, `commands[].id` and `major`.
pub fn parse_manifest(bytes: &[u8], path: &str) -> Result<Manifest, RefError> {
    let err = |reason: String| RefError::ManifestUnreadable { path: path.to_string(), reason };
    let root = fid_canon::parse(bytes).map_err(|e| err(e.to_string()))?;
    let Some(CValue::Arr(cmds)) = (match &root {
        CValue::Obj(o) => o.get("commands"),
        _ => None,
    }) else {
        return Err(err("no `commands` array".into()));
    };
    let mut commands = BTreeSet::new();
    for (i, c) in cmds.iter().enumerate() {
        let CValue::Obj(o) = c else { return Err(err(format!("commands[{i}] is not an object"))) };
        let id = match o.get("id") {
            Some(CValue::Str(s)) if CommandName::is_valid(s) => s.clone(),
            _ => return Err(err(format!("commands[{i}].id is not a command name"))),
        };
        let Some(major) = major_of(o.get("major")) else {
            return Err(err(format!("commands[{i}].major is not an integer 0..2^32")));
        };
        if !commands.insert((id.clone(), major)) {
            return Err(err(format!("duplicate command {id}@{major}")));
        }
    }
    Ok(Manifest { digest: fid_canon::digest_bytes(bytes).to_string(), commands })
}

/// Parses `surfaces.toml`; returns the shipped surfaces.
pub fn parse_surfaces(text: &str, path: &str) -> Result<BTreeSet<Surface>, RefError> {
    let err = |reason: String| RefError::SurfacesUnreadable { path: path.to_string(), reason };
    let table = text.parse::<toml::Table>().map_err(|e| err(e.message().to_string()))?;
    if let Some(k) = table.keys().find(|k| k.as_str() != "surfaces") {
        return Err(err(format!("unknown key `{k}`")));
    }
    let Some(toml::Value::Array(items)) = table.get("surfaces") else {
        return Err(err("missing `surfaces` array".into()));
    };
    let mut out = BTreeSet::new();
    for item in items {
        let Some(s) = item.as_str().and_then(Surface::parse) else {
            return Err(err("`surfaces` holds only \"cli\", \"mcp\" and \"python\"".into()));
        };
        if !out.insert(s) {
            return Err(err(format!("duplicate surface `{}`", s.as_str())));
        }
    }
    Ok(out)
}

fn opt_str(o: &BTreeMap<String, CValue>, key: &str) -> Result<Option<String>, String> {
    match o.get(key) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("`{key}` is not a string")),
    }
}

fn req_str(o: &BTreeMap<String, CValue>, key: &str) -> Result<String, String> {
    opt_str(o, key)?.ok_or_else(|| format!("missing `{key}`"))
}

fn req_bool(o: &BTreeMap<String, CValue>, key: &str) -> Result<bool, String> {
    match o.get(key) {
        Some(CValue::Bool(b)) => Ok(*b),
        _ => Err(format!("`{key}` is not a boolean")),
    }
}

fn surface_doc(o: &BTreeMap<String, CValue>, s: Surface) -> Result<SurfaceDoc, String> {
    let at = format!("surfaces.{}", s.as_str());
    let Some(CValue::Obj(d)) = o.get(s.as_str()) else { return Err(format!("`{at}` is not an object")) };
    let available = req_bool(d, "available").map_err(|e| format!("{at}: {e}"))?;
    let name = opt_str(d, "name").map_err(|e| format!("{at}: {e}"))?;
    let projection = opt_str(d, "projection").map_err(|e| format!("{at}: {e}"))?;
    let reason = match opt_str(d, "reason").map_err(|e| format!("{at}: {e}"))? {
        None => None,
        Some(r) => Some(Reason::parse(&r).ok_or_else(|| format!("{at}.reason `{r}` is not a known reason code"))?),
    };
    if available && name.is_none() {
        return Err(format!("{at} is available but has no `name`"));
    }
    Ok(SurfaceDoc { available, name, projection, reason })
}

/// Parses one record file's bytes. `file` is its name inside the records directory and must be
/// `<command_id>@<major>.json`.
pub fn parse_record(bytes: &[u8], file: &str) -> Result<Record, RefError> {
    let err = |reason: String| RefError::RecordParse { file: file.to_string(), reason };
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(err(format!("larger than {MAX_RECORD_BYTES} bytes")));
    }
    let root = fid_canon::parse_with_limits(bytes, MAX_RECORD_BYTES, MAX_RECORD_DEPTH).map_err(|e| err(e.to_string()))?;
    let CValue::Obj(o) = root else { return Err(err("not a JSON object".into())) };
    if let Some(schema) = opt_str(&o, "schema").map_err(err)? {
        if schema != "docs-ref-v1" {
            return Err(err(format!("schema `{schema}` is not docs-ref-v1")));
        }
    }
    let manifest_digest = req_str(&o, "manifest_digest").map_err(err)?;
    if !Digest::is_valid(&manifest_digest) {
        return Err(err("`manifest_digest` is not a sha256: digest".into()));
    }
    let command_id = req_str(&o, "command_id").map_err(err)?;
    if !CommandName::is_valid(&command_id) {
        return Err(err("`command_id` is not a command name".into()));
    }
    let major = major_of(o.get("major")).ok_or_else(|| err("`major` is not an integer 0..2^32".into()))?;
    if file != format!("{command_id}@{major}.json") {
        return Err(err(format!("file name does not match {command_id}@{major}.json")));
    }
    let summary = req_str(&o, "summary").map_err(err)?;
    let help = req_str(&o, "help").map_err(err)?;
    let Some(CValue::Arr(raw_params)) = o.get("params") else { return Err(err("`params` is not an array".into())) };
    let mut params = Vec::with_capacity(raw_params.len());
    for (i, p) in raw_params.iter().enumerate() {
        let CValue::Obj(p) = p else { return Err(err(format!("params[{i}] is not an object"))) };
        let field = |k: &str| p.get(k).cloned().unwrap_or(CValue::Null);
        params.push(Param {
            name: req_str(p, "name").map_err(|e| err(format!("params[{i}]: {e}")))?,
            ty: req_str(p, "ty").map_err(|e| err(format!("params[{i}]: {e}")))?,
            quantity_kind: field("quantity_kind"),
            unit: field("unit"),
            default: field("default"),
        });
    }
    let result = o.get("result").cloned().ok_or_else(|| err("missing `result`".into()))?;
    let side_effect = SideEffect::parse(&req_str(&o, "side_effect").map_err(err)?).map_err(|e| err(e.to_string()))?;
    let determinism = Determinism::parse(&req_str(&o, "determinism").map_err(err)?).map_err(|e| err(e.to_string()))?;
    let human_only = req_bool(&o, "human_only").map_err(err)?;
    let Some(CValue::Obj(s)) = o.get("surfaces") else { return Err(err("`surfaces` is not an object".into())) };
    let surfaces = [
        surface_doc(s, Surface::Cli).map_err(err)?,
        surface_doc(s, Surface::Mcp).map_err(err)?,
        surface_doc(s, Surface::Python).map_err(err)?,
    ];
    Ok(Record {
        file: file.to_string(),
        manifest_digest,
        command_id,
        major,
        summary,
        help,
        params,
        result,
        side_effect,
        determinism,
        human_only,
        surfaces,
    })
}

/// HTML-escapes `& < > " '`.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Cell text of a record value: a string as is, `null` empty, anything else its RFC 8785 text.
fn value_text(v: &CValue) -> String {
    match v {
        CValue::Null => String::new(),
        CValue::Str(s) => s.clone(),
        other => fid_canon::to_canonical_bytes(other).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default(),
    }
}

fn push_reason(out: &mut String, reason: Option<Reason>) {
    if let Some(r) = reason {
        out.push_str("<p class=\"reason\">Reason: ");
        out.push_str(r.words());
        out.push_str("</p>\n");
    }
}

/// Renders one page (see the module docs for the layout).
pub fn render_page(rec: &Record, surface: Surface, ships_python: bool) -> String {
    let cmd = escape(&format!("{}@{}", rec.command_id, rec.major));
    let mcp = rec.surface(Surface::Mcp);
    let mut out = String::new();
    out.push_str(&format!("<article class=\"ref-page ref-{}\">\n", surface.as_str()));
    out.push_str(&format!("<h1><code>{cmd}</code></h1>\n"));
    out.push_str(&format!("<p class=\"summary\">{}</p>\n", escape(&rec.summary)));
    out.push_str("<dl class=\"ref-facts\">\n");
    out.push_str(&format!("<dt>Command</dt><dd><code>{cmd}</code></dd>\n"));
    out.push_str(&format!("<dt>Side-effect class</dt><dd>{}</dd>\n", rec.side_effect.as_str()));
    out.push_str(&format!("<dt>Determinism</dt><dd>{}</dd>\n", rec.determinism.as_str()));
    out.push_str(&format!("<dt>Human only</dt><dd>{}</dd>\n", rec.human_only));
    out.push_str(&format!("<dt>MCP</dt><dd>{}</dd>\n", escape(mcp.projection.as_deref().unwrap_or(""))));
    out.push_str("</dl>\n");
    out.push_str(&format!("<pre class=\"help\">{}</pre>\n", escape(&rec.help)));
    out.push_str(&format!("<h2>{}</h2>\n", surface.heading()));
    let doc = rec.surface(surface);
    match (&doc.name, doc.available) {
        _ if surface == Surface::Python && !ships_python => {
            out.push_str("<p class=\"unavailable\">Python bindings are not part of this release.</p>\n");
        }
        (Some(name), true) => {
            out.push_str(&format!("<p><code>{}</code></p>\n", escape(name)));
            push_reason(&mut out, doc.reason);
            out.push_str("<h2>Parameters</h2>\n<table class=\"params\">\n");
            out.push_str(
                "<thead><tr><th>Name</th><th>Type</th><th>Quantity kind</th><th>Unit</th><th>Default</th></tr></thead>\n",
            );
            out.push_str("<tbody>\n");
            for p in &rec.params {
                out.push_str(&format!(
                    "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                    escape(&p.name),
                    escape(&p.ty),
                    escape(&value_text(&p.quantity_kind)),
                    escape(&value_text(&p.unit)),
                    escape(&value_text(&p.default)),
                ));
            }
            out.push_str("</tbody>\n</table>\n");
            out.push_str(&format!("<h2>Result</h2>\n<p><code>{}</code></p>\n", escape(&value_text(&rec.result))));
        }
        _ => {
            out.push_str(&format!(
                "<p class=\"unavailable\">This command is not available on {}.</p>\n",
                surface.unavailable_on()
            ));
            push_reason(&mut out, doc.reason);
        }
    }
    out.push_str("</article>\n");
    out
}

/// Page path relative to the `ref/` directory of one release.
pub fn page_path(surface: Surface, key: &Key) -> String {
    format!("{}/{}.html", surface.as_str(), key_text(key))
}

/// Whether `tag` is a usable release tag: 1 to 128 of `[A-Za-z0-9._+-]`, not starting with `.`.
pub fn valid_release(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= MAX_RELEASE_LEN
        && !tag.starts_with('.')
        && tag.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
}

/// The input files of both subcommands.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub records: &'a Path,
    pub manifest: &'a Path,
    pub surfaces: &'a Path,
}

/// Parsed inputs.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub manifest: Manifest,
    pub records: BTreeMap<Key, Record>,
    pub ships: BTreeSet<Surface>,
}

/// Reads and parses the manifest, the surfaces file and every record.
pub fn load(inputs: &Inputs<'_>) -> Result<Loaded, RefError> {
    let mpath = inputs.manifest.display().to_string();
    let bytes = read_bounded(inputs.manifest, MAX_MANIFEST_BYTES)
        .map_err(|reason| RefError::ManifestUnreadable { path: mpath.clone(), reason })?;
    let manifest = parse_manifest(&bytes, &mpath)?;

    let spath = inputs.surfaces.display().to_string();
    let sbytes = read_bounded(inputs.surfaces, MAX_SURFACES_BYTES)
        .map_err(|reason| RefError::SurfacesUnreadable { path: spath.clone(), reason })?;
    let stext = String::from_utf8(sbytes)
        .map_err(|_| RefError::SurfacesUnreadable { path: spath.clone(), reason: "not UTF-8".into() })?;
    let ships = parse_surfaces(&stext, &spath)?;

    let rdir = inputs.records.display().to_string();
    let unreadable = |reason: String| RefError::Unreadable { path: rdir.clone(), reason };
    let mut names = BTreeSet::new();
    for entry in std::fs::read_dir(inputs.records).map_err(|e| unreadable(e.to_string()))? {
        let entry = entry.map_err(|e| unreadable(e.to_string()))?;
        if names.len() >= MAX_ENTRIES {
            return Err(unreadable(format!("more than {MAX_ENTRIES} entries")));
        }
        names.insert(entry.file_name().to_string_lossy().into_owned());
    }
    let mut records = BTreeMap::new();
    for name in names {
        let path = inputs.records.join(&name);
        if !name.ends_with(".json") || !path.is_file() {
            return Err(RefError::RecordParse { file: name, reason: "not a .json record file".into() });
        }
        let bytes = read_bounded(&path, MAX_RECORD_BYTES)
            .map_err(|reason| RefError::RecordParse { file: name.clone(), reason })?;
        let rec = parse_record(&bytes, &name)?;
        records.insert(rec.key(), rec);
    }
    Ok(Loaded { manifest, records, ships })
}

impl Loaded {
    /// Records whose `manifest_digest` is not the manifest's digest, as findings.
    pub fn digest_mismatches(&self) -> Vec<Finding> {
        self.records
            .values()
            .filter(|r| r.manifest_digest != self.manifest.digest)
            .map(|r| Finding { path: r.file.clone(), code: Code::ManifestDigestMismatch })
            .collect()
    }

    /// Manifest commands with no record, as `record_missing` findings.
    pub fn missing_records(&self) -> Vec<Finding> {
        self.manifest
            .commands
            .iter()
            .filter(|k| !self.records.contains_key(*k))
            .map(|k| Finding { path: format!("{}.json", key_text(k)), code: Code::RecordMissing })
            .collect()
    }

    /// Fails with `record_parse` for a record whose command is not in the manifest (its digest
    /// matches, so the inputs contradict each other).
    fn check_no_orphans(&self) -> Result<(), RefError> {
        match self.records.iter().find(|(k, _)| !self.manifest.commands.contains(*k)) {
            Some((_, r)) => {
                Err(RefError::RecordParse { file: r.file.clone(), reason: "command is not in the manifest".into() })
            }
            None => Ok(()),
        }
    }

    /// Every page, keyed by its path relative to the release's `ref/` directory.
    pub fn render(&self) -> BTreeMap<String, Vec<u8>> {
        let python = self.ships.contains(&Surface::Python);
        let mut pages = BTreeMap::new();
        for (key, rec) in &self.records {
            for s in Surface::ALL {
                pages.insert(page_path(s, key), render_page(rec, s, python).into_bytes());
            }
        }
        pages
    }
}

/// Result of `ref-render`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderOutcome {
    /// Number of pages written.
    Written(usize),
    /// Nothing written: `manifest_digest_mismatch` or `record_missing` findings.
    Refused(Vec<Finding>),
}

/// `ref-render`: writes every page under `<out>/<release>/ref/`, or nothing when refused.
pub fn render_tree(inputs: &Inputs<'_>, release: &str, out: &Path) -> Result<RenderOutcome, RefError> {
    let loaded = load(inputs)?;
    let mismatches = loaded.digest_mismatches();
    if !mismatches.is_empty() {
        return Ok(RenderOutcome::Refused(mismatches));
    }
    loaded.check_no_orphans()?;
    let missing = loaded.missing_records();
    if !missing.is_empty() {
        return Ok(RenderOutcome::Refused(missing));
    }
    let pages = loaded.render();
    let base = out.join(release).join("ref");
    for (rel, bytes) in &pages {
        let path = base.join(rel);
        let unwritable = |e: std::io::Error| RefError::Unwritable { path: path.display().to_string(), reason: e.to_string() };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(unwritable)?;
        }
        std::fs::write(&path, bytes).map_err(unwritable)?;
    }
    Ok(RenderOutcome::Written(pages.len()))
}

/// Result of `ref-check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Sorted by path, then code.
    pub findings: Vec<Finding>,
    pub pages_checked: usize,
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    /// Text output: one line per finding, then a summary.
    pub fn render(&self) -> String {
        let mut s = String::new();
        for f in &self.findings {
            s.push_str(&format!("{f}\n"));
        }
        s.push_str(&format!("ref-check: {} page(s), {} finding(s)\n", self.pages_checked, self.findings.len()));
        s
    }

    /// RFC 8785 JSON report, findings sorted by path.
    pub fn to_json(&self) -> Vec<u8> {
        let findings = self
            .findings
            .iter()
            .map(|f| {
                let mut o = BTreeMap::new();
                o.insert("code".to_string(), CValue::Str(f.code.as_str().to_string()));
                o.insert("path".to_string(), CValue::Str(f.path.clone()));
                CValue::Obj(o)
            })
            .collect();
        let mut o = BTreeMap::new();
        o.insert("findings".to_string(), CValue::Arr(findings));
        o.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }
}

/// Reads the page tree: files keyed `<surface>/<name>`, plus the paths of entries that cannot
/// be pages (anything outside the three surface directories, and subdirectories in them).
fn read_tree(dir: &Path) -> Result<(BTreeMap<String, Vec<u8>>, BTreeSet<String>), RefError> {
    let unreadable = |p: &Path, e: String| RefError::Unreadable { path: p.display().to_string(), reason: e };
    let mut files = BTreeMap::new();
    let mut strays = BTreeSet::new();
    let mut seen = 0usize;
    let mut list = |d: &Path| -> Result<Vec<(String, std::path::PathBuf)>, RefError> {
        let mut v = Vec::new();
        for entry in std::fs::read_dir(d).map_err(|e| unreadable(d, e.to_string()))? {
            let entry = entry.map_err(|e| unreadable(d, e.to_string()))?;
            seen += 1;
            if seen > MAX_ENTRIES {
                return Err(unreadable(d, format!("more than {MAX_ENTRIES} entries")));
            }
            v.push((entry.file_name().to_string_lossy().into_owned(), entry.path()));
        }
        v.sort();
        Ok(v)
    };
    for (top, top_path) in list(dir)? {
        if Surface::parse(&top).is_none() || !top_path.is_dir() {
            strays.insert(top);
            continue;
        }
        for (name, path) in list(&top_path)? {
            let rel = format!("{top}/{name}");
            if !path.is_file() {
                strays.insert(rel);
                continue;
            }
            // A page over the bound cannot equal a rendering; keep a marker that never matches.
            let bytes = read_bounded(&path, MAX_PAGE_BYTES).unwrap_or_else(|_| vec![0xff]);
            files.insert(rel, bytes);
        }
    }
    Ok((files, strays))
}

/// `ref-check`: renders in memory and compares with the page tree at `ref_dir`
/// (the `<release>/ref` directory).
pub fn check(inputs: &Inputs<'_>, ref_dir: &Path) -> Result<Report, RefError> {
    let loaded = load(inputs)?;
    let mismatches = loaded.digest_mismatches();
    if !mismatches.is_empty() {
        return Err(RefError::DigestMismatch { files: mismatches.into_iter().map(|f| f.path).collect() });
    }
    loaded.check_no_orphans()?;
    if !ref_dir.is_dir() {
        return Err(RefError::Unreadable { path: ref_dir.display().to_string(), reason: "not a directory".into() });
    }
    let mut findings = loaded.missing_records();
    // Pages of a manifest command without a record are covered by `record_missing`.
    let skipped: BTreeSet<String> = loaded
        .manifest
        .commands
        .iter()
        .filter(|k| !loaded.records.contains_key(*k))
        .flat_map(|k| Surface::ALL.map(|s| page_path(s, k)))
        .collect();
    let expected = loaded.render();
    let (tree, strays) = read_tree(ref_dir)?;
    for (path, bytes) in &expected {
        match tree.get(path) {
            None => findings.push(Finding { path: path.clone(), code: Code::PageMissing }),
            Some(b) if b != bytes => findings.push(Finding { path: path.clone(), code: Code::PageDrift }),
            Some(_) => {}
        }
    }
    for path in tree.keys().chain(strays.iter()) {
        if !expected.contains_key(path) && !skipped.contains(path) {
            findings.push(Finding { path: path.clone(), code: Code::PageWithoutCommand });
        }
    }
    findings.sort();
    Ok(Report { findings, pages_checked: expected.len() })
}

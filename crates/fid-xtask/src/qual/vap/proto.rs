//! `qual vap check-protocols`: the IQ and OQ protocol templates and their command check (spec
//! node `QUAL.VAP.PROTO`).
//!
//! # The step table
//! A protocol is a `vap-template/1` template (`iq-protocol` or `oq-protocol`) with exactly one
//! table field `steps` whose columns are, in this order, [`STEP_COLUMNS`]: five static columns
//! (`step`, `purpose`, `command`, `record-field`, `acceptance`) and two customer columns
//! (`result`, `signature`). Each row is one step; the `step` cell must equal the row ID, so the
//! ID a reader sees is the ID the check names. The template itself must pass the
//! `vap-template/1` check of `fid_qual::vap::tpl` first.
//!
//! # Guarantees
//! - Every step names a registry command ID (`name@major`) and a record field path
//!   (REQ-QUAL.VAP.PROTO-001); a step whose command is not a command ID is a
//!   `PROTO-COMMAND` finding, one whose field is not a dotted path a `PROTO-FIELD` finding.
//! - A command ID absent from the given Registry Manifest is a `PROTO-COMMAND` finding and a
//!   field path that does not resolve in the given `qual-record/1` JSON Schema a `PROTO-FIELD`
//!   finding, both naming the protocol and the step (REQ-QUAL.VAP.PROTO-002).
//! - The IQ protocol must have steps for `validate.run_iq@1` and `validate.export_bundle@1`,
//!   and the OQ protocol a step for `validate.run_oq@1`; when any OQ step names a field under
//!   `goldens`, it must also have a step for `validate.register_golden@1`. A missing one is a
//!   `PROTO-REQUIRED` finding naming the command (REQ-QUAL.VAP.PROTO-003).
//! - A protocol file that is absent, fails `vap-template/1` or has no well-formed step table
//!   is a `PROTO-TEMPLATE` finding and is not checked further; nothing is skipped silently.
//! - The manifest and schema are inputs, read with `fid-canon`'s bounded parser (duplicate
//!   keys refused). A manifest that is not `format` 1 with a `commands` array of
//!   `{id, major}` objects, or that lists one `(id, major)` twice, is refused as a whole, as is
//!   a schema whose root is not an object schema for `qual-record/1`. The check never treats
//!   an unreadable input as an empty one.
//! - Output depends only on the inputs (D0): protocols in [`PROTOCOLS`] order, steps in row
//!   order, command finding before field finding, then the required-command findings.

use fid_canon::{parse_with_budget, parse_with_limits, CValue};
use fid_qual::vap::tpl::{check_template, Catalogue, FieldBody, Fill, Template};
use fid_types::{CommandId, CommandName};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Largest accepted Registry Manifest (the registry's own limit).
pub const MAX_MANIFEST_BYTES: usize = fid_canon::MAX_INPUT_BYTES;
/// Largest accepted record schema.
pub const MAX_SCHEMA_BYTES: usize = 1024 * 1024;
/// Deepest accepted record schema nesting.
pub const MAX_SCHEMA_DEPTH: usize = 32;
/// Element budget of the record schema parser.
pub const MAX_SCHEMA_ELEMENTS: usize = 100_000;
/// Most `$ref` hops followed while resolving one path segment.
pub const MAX_REF_HOPS: usize = 16;
/// Most segments in a record field path.
pub const MAX_PATH_SEGMENTS: usize = 16;
/// The record schema the field paths resolve against.
pub const RECORD_SCHEMA: &str = "qual-record/1";
/// The table field holding a protocol's steps.
pub const STEPS_FIELD: &str = "steps";
/// The step table's columns, in order, with their fill.
pub const STEP_COLUMNS: [(&str, Fill); 7] = [
    ("step", Fill::Static),
    ("purpose", Fill::Static),
    ("command", Fill::Static),
    ("record-field", Fill::Static),
    ("acceptance", Fill::Static),
    ("result", Fill::Customer),
    ("signature", Fill::Customer),
];

/// A protocol this check covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Protocol {
    Iq,
    Oq,
}

impl Protocol {
    /// Template ID (and file stem): `iq-protocol` or `oq-protocol`.
    pub fn id(self) -> &'static str {
        match self {
            Protocol::Iq => "iq-protocol",
            Protocol::Oq => "oq-protocol",
        }
    }

    /// Commands the protocol must always have a step for (REQ-QUAL.VAP.PROTO-003).
    pub fn required(self) -> &'static [&'static str] {
        match self {
            Protocol::Iq => &[RUN_IQ, EXPORT_BUNDLE],
            Protocol::Oq => &[RUN_OQ],
        }
    }
}

/// The protocols, in check and report order.
pub const PROTOCOLS: [Protocol; 2] = [Protocol::Iq, Protocol::Oq];
/// `validate.run_iq@1`.
pub const RUN_IQ: &str = "validate.run_iq@1";
/// `validate.export_bundle@1`.
pub const EXPORT_BUNDLE: &str = "validate.export_bundle@1";
/// `validate.run_oq@1`.
pub const RUN_OQ: &str = "validate.run_oq@1";
/// `validate.register_golden@1`, required in OQ when goldens are used.
pub const REGISTER_GOLDEN: &str = "validate.register_golden@1";
/// The record field whose use in an OQ step means goldens are used.
pub const GOLDENS_FIELD: &str = "goldens";

// ------------------------------------------------------------------ findings

/// The kind of a protocol-check finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FindingCode {
    /// The protocol file is absent, fails `vap-template/1`, or has no well-formed step table.
    Template,
    /// A step's command is not a command ID or is absent from the Registry Manifest.
    Command,
    /// A step's record field is not a path or does not resolve in the record schema.
    Field,
    /// A required command has no step.
    Required,
}

impl FindingCode {
    /// Stable code: `PROTO-TEMPLATE`, `PROTO-COMMAND`, `PROTO-FIELD` or `PROTO-REQUIRED`.
    pub fn code(self) -> &'static str {
        match self {
            FindingCode::Template => "PROTO-TEMPLATE",
            FindingCode::Command => "PROTO-COMMAND",
            FindingCode::Field => "PROTO-FIELD",
            FindingCode::Required => "PROTO-REQUIRED",
        }
    }
}

/// One finding. `subject` is `<protocol>/<step>` for command and field findings, the protocol
/// ID for template findings and the command ID for required-command findings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub code: FindingCode,
    pub subject: String,
    pub detail: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code.code(), self.subject, self.detail)
    }
}

// ------------------------------------------------------------------ inputs

/// The `(name, major)` command IDs a Registry Manifest lists, as `name@major` text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ManifestCommands {
    ids: BTreeSet<String>,
}

impl ManifestCommands {
    /// Reads a Registry Manifest: an object with `format` 1 and a `commands` array of objects,
    /// each with a command-name `id` and an integer `major` (0 to 99999). Other fields are not
    /// interpreted here: definitions and digests are the registry's own check
    /// (`registry.show_manifest@1` with `check`). A repeated `(id, major)` is refused.
    pub fn from_json(bytes: &[u8]) -> Result<ManifestCommands, String> {
        let v = parse_with_limits(bytes, MAX_MANIFEST_BYTES, fid_canon::MAX_DEPTH)
            .map_err(|e| format!("not valid JSON within limits ({e})"))?;
        let CValue::Obj(root) = &v else {
            return Err("must be a JSON object".to_string());
        };
        match root.get("format") {
            Some(CValue::Num(n)) if *n == 1.0 => {}
            _ => return Err("format must be 1".to_string()),
        }
        let Some(CValue::Arr(commands)) = root.get("commands") else {
            return Err("commands must be an array".to_string());
        };
        let mut ids = BTreeSet::new();
        for (i, c) in commands.iter().enumerate() {
            let CValue::Obj(o) = c else {
                return Err(format!("commands[{i}] must be an object"));
            };
            let name = match o.get("id") {
                Some(CValue::Str(s)) if CommandName::is_valid(s) => s,
                _ => return Err(format!("commands[{i}].id must be a command name")),
            };
            let major = match o.get("major") {
                Some(CValue::Num(n)) if n.fract() == 0.0 && (0.0..=99_999.0).contains(n) => *n as u32,
                _ => return Err(format!("commands[{i}].major must be an integer from 0 to 99999")),
            };
            if !ids.insert(format!("{name}@{major}")) {
                return Err(format!("commands[{i}] lists {name}@{major} a second time"));
            }
        }
        Ok(ManifestCommands { ids })
    }

    /// True when the manifest lists exactly this command ID text (`name@major`, no leading
    /// zeros in the major).
    pub fn contains(&self, id: &str) -> bool {
        self.ids.contains(id)
    }

    /// Number of command IDs listed.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// True when the manifest lists no command.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

type Map = BTreeMap<String, CValue>;

/// A `qual-record/1` JSON Schema, used only to resolve record field paths.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordSchema {
    root: Map,
}

impl RecordSchema {
    /// Reads the schema. Its root must be an object schema (`"type": "object"` with
    /// `properties`) whose `schema` property is `{"const": "qual-record/1"}`, so a schema
    /// for some other document cannot stand in for it.
    pub fn from_json(bytes: &[u8]) -> Result<RecordSchema, String> {
        let v = parse_with_budget(bytes, MAX_SCHEMA_BYTES, MAX_SCHEMA_DEPTH, MAX_SCHEMA_ELEMENTS)
            .map_err(|e| format!("not valid JSON within limits ({e})"))?;
        let CValue::Obj(root) = v else {
            return Err("must be a JSON object".to_string());
        };
        if !matches!(root.get("type"), Some(CValue::Str(t)) if t == "object") {
            return Err("root must have \"type\": \"object\"".to_string());
        }
        let tag = match root.get("properties") {
            Some(CValue::Obj(p)) => p.get("schema"),
            _ => return Err("root must have properties".to_string()),
        };
        let is_record = match tag {
            Some(CValue::Obj(s)) => matches!(s.get("const"), Some(CValue::Str(c)) if c == RECORD_SCHEMA),
            _ => false,
        };
        if !is_record {
            return Err(format!("properties.schema must be {{\"const\": {RECORD_SCHEMA:?}}}"));
        }
        Ok(RecordSchema { root })
    }

    /// Follows local `$ref`s (`#/$defs/<name>` only) from `node`, at most [`MAX_REF_HOPS`].
    fn follow_refs<'a>(&'a self, mut node: &'a Map) -> Option<&'a Map> {
        for _ in 0..MAX_REF_HOPS {
            let Some(r) = node.get("$ref") else {
                return Some(node);
            };
            let CValue::Str(r) = r else { return None };
            let name = r.strip_prefix("#/$defs/")?;
            let Some(CValue::Obj(defs)) = self.root.get("$defs") else { return None };
            let Some(CValue::Obj(next)) = defs.get(name) else { return None };
            node = next;
        }
        None
    }

    /// True when the dotted `field` path resolves: each segment is a property of the current
    /// object schema, where an array schema stands for its `items`. A `$ref` to `#/$defs/...`
    /// is followed; anything else (another `$ref`, a combinator without `properties`) does not
    /// resolve, so the check fails closed.
    pub fn resolves(&self, field: &str) -> bool {
        if !is_field_path(field) {
            return false;
        }
        let mut node: &Map = &self.root;
        for seg in field.split('.') {
            let mut cur = match self.follow_refs(node) {
                Some(n) => n,
                None => return false,
            };
            // An array of objects (possibly nested): descend through `items`.
            let mut hops = 0;
            while matches!(cur.get("type"), Some(CValue::Str(t)) if t == "array") {
                hops += 1;
                let next = match cur.get("items") {
                    Some(CValue::Obj(items)) if hops <= MAX_REF_HOPS => self.follow_refs(items),
                    _ => None,
                };
                match next {
                    Some(n) => cur = n,
                    None => return false,
                }
            }
            let Some(CValue::Obj(props)) = cur.get("properties") else { return false };
            match props.get(seg) {
                Some(CValue::Obj(child)) => node = child,
                _ => return false,
            }
        }
        true
    }

    /// Every field path that [`RecordSchema::resolves`], sorted, down to
    /// [`MAX_PATH_SEGMENTS`] segments (so a `$ref` cycle ends).
    pub fn field_paths(&self) -> Vec<String> {
        let mut out = BTreeSet::new();
        self.collect_paths(&self.root, "", 1, &mut out);
        out.into_iter().filter(|p| self.resolves(p)).collect()
    }

    fn collect_paths(&self, node: &Map, prefix: &str, depth: usize, out: &mut BTreeSet<String>) {
        let Some(mut cur) = self.follow_refs(node) else { return };
        let mut hops = 0;
        while matches!(cur.get("type"), Some(CValue::Str(t)) if t == "array") {
            hops += 1;
            match cur.get("items") {
                Some(CValue::Obj(items)) if hops <= MAX_REF_HOPS => match self.follow_refs(items) {
                    Some(n) => cur = n,
                    None => return,
                },
                _ => return,
            }
        }
        let Some(CValue::Obj(props)) = cur.get("properties") else { return };
        for (k, child) in props {
            let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
            if !is_field_path(&p) {
                continue;
            }
            out.insert(p.clone());
            if let (CValue::Obj(c), true) = (child, depth < MAX_PATH_SEGMENTS) {
                self.collect_paths(c, &p, depth + 1, out);
            }
        }
    }
}

/// A record field path: 1 to [`MAX_PATH_SEGMENTS`] segments of `[a-z0-9_]`, joined by `.`.
pub fn is_field_path(s: &str) -> bool {
    let segs: Vec<&str> = s.split('.').collect();
    segs.len() <= MAX_PATH_SEGMENTS
        && segs
            .iter()
            .all(|g| !g.is_empty() && g.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'))
}

// ------------------------------------------------------------------ steps

/// One protocol step, read from a row of the step table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub id: String,
    pub purpose: String,
    pub command: String,
    pub record_field: String,
    pub acceptance: String,
}

/// Reads the step table of a checked template: exactly one field `steps`, a table with
/// [`STEP_COLUMNS`] in order, at least one row, and each row's `step` cell equal to its ID.
pub fn steps(template: &Template) -> Result<Vec<Step>, String> {
    let mut tables = Vec::new();
    for s in template.sections() {
        for f in &s.fields {
            if f.id == STEPS_FIELD {
                tables.push(&f.body);
            }
        }
    }
    let body = match tables.as_slice() {
        [one] => *one,
        [] => return Err(format!("has no field {STEPS_FIELD:?}")),
        _ => return Err(format!("has more than one field {STEPS_FIELD:?}")),
    };
    let FieldBody::Table { columns, rows } = body else {
        return Err(format!("field {STEPS_FIELD:?} is not a table"));
    };
    let got: Vec<(&str, Fill)> = columns.iter().map(|c| (c.id.as_str(), c.fill)).collect();
    if got != STEP_COLUMNS {
        let want: Vec<String> = STEP_COLUMNS.iter().map(|(id, fill)| format!("{id} ({})", fill_name(*fill))).collect();
        return Err(format!("step table columns must be, in order: {}", want.join(", ")));
    }
    if rows.is_empty() {
        return Err("step table has no steps".to_string());
    }
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let cell = |k: &str| r.cells.get(k).cloned().ok_or_else(|| format!("step {} lacks cell {k:?}", r.id));
        let step = cell("step")?;
        if step != r.id {
            return Err(format!("step {}: the step cell {step:?} differs from the row ID", r.id));
        }
        out.push(Step {
            id: r.id.clone(),
            purpose: cell("purpose")?,
            command: cell("command")?,
            record_field: cell("record-field")?,
            acceptance: cell("acceptance")?,
        });
    }
    Ok(out)
}

fn fill_name(f: Fill) -> &'static str {
    match f {
        Fill::Static => "static",
        Fill::Prefill => "prefill",
        Fill::Customer => "customer",
    }
}

// ------------------------------------------------------------------ check

/// The result of a protocol check.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// Steps read per protocol that had a well-formed step table, in [`PROTOCOLS`] order.
    pub steps: Vec<(Protocol, Vec<Step>)>,
    /// Every finding, in report order.
    pub findings: Vec<Finding>,
}

impl Report {
    /// True when there are no findings.
    pub fn is_ok(&self) -> bool {
        self.findings.is_empty()
    }

    /// Total number of steps read.
    pub fn step_count(&self) -> usize {
        self.steps.iter().map(|(_, s)| s.len()).sum()
    }
}

/// Checks the IQ and OQ protocols. `files` are (file stem, bytes) pairs of a template
/// directory (other templates in it are ignored); a protocol missing from it is a
/// `PROTO-TEMPLATE` finding.
pub fn check(
    files: &[(String, Vec<u8>)],
    catalogue: &Catalogue,
    manifest: &ManifestCommands,
    schema: &RecordSchema,
) -> Report {
    let mut report = Report::default();
    for p in PROTOCOLS {
        let template_finding =
            |detail: String| Finding { code: FindingCode::Template, subject: p.id().to_string(), detail };
        let matching: Vec<&(String, Vec<u8>)> = files.iter().filter(|(stem, _)| stem == p.id()).collect();
        let bytes = match matching.as_slice() {
            [(_, b)] => b,
            [] => {
                report.findings.push(template_finding(format!("{}.json is missing", p.id())));
                continue;
            }
            _ => {
                report.findings.push(template_finding(format!("{}.json appears more than once", p.id())));
                continue;
            }
        };
        let (template, tpl_findings) = check_template(p.id(), bytes, catalogue);
        let template = match template {
            Some(t) if tpl_findings.is_empty() => t,
            _ => {
                let detail = match tpl_findings.first() {
                    Some(f) => format!("fails vap-template/1 ({f})"),
                    None => "fails vap-template/1".to_string(),
                };
                report.findings.push(template_finding(detail));
                continue;
            }
        };
        let list = match steps(&template) {
            Ok(s) => s,
            Err(d) => {
                report.findings.push(template_finding(d));
                continue;
            }
        };
        let mut named: BTreeSet<&str> = BTreeSet::new();
        let mut goldens_used = false;
        for s in &list {
            let subject = format!("{}/{}", p.id(), s.id);
            if !CommandId::is_valid(&s.command) {
                report.findings.push(Finding {
                    code: FindingCode::Command,
                    subject: subject.clone(),
                    detail: format!("{:?} is not a command ID (name@major)", s.command),
                });
            } else if !manifest.contains(&s.command) {
                report.findings.push(Finding {
                    code: FindingCode::Command,
                    subject: subject.clone(),
                    detail: format!("{} is not in the registry manifest", s.command),
                });
            }
            named.insert(s.command.as_str());
            if !is_field_path(&s.record_field) {
                report.findings.push(Finding {
                    code: FindingCode::Field,
                    subject,
                    detail: format!("{:?} is not a record field path", s.record_field),
                });
            } else if !schema.resolves(&s.record_field) {
                report.findings.push(Finding {
                    code: FindingCode::Field,
                    subject,
                    detail: format!("{} is not a field of {RECORD_SCHEMA}", s.record_field),
                });
            }
            let first = s.record_field.split('.').next().unwrap_or_default();
            if first == GOLDENS_FIELD {
                goldens_used = true;
            }
        }
        let mut required: Vec<&str> = p.required().to_vec();
        if p == Protocol::Oq && goldens_used {
            required.push(REGISTER_GOLDEN);
        }
        for cmd in required {
            if !named.contains(cmd) {
                let detail = if cmd == REGISTER_GOLDEN {
                    format!("{} uses goldens but has no step that runs it", p.id())
                } else {
                    format!("{} has no step that runs it", p.id())
                };
                report.findings.push(Finding { code: FindingCode::Required, subject: cmd.to_string(), detail });
            }
        }
        report.steps.push((p, list));
    }
    report
}

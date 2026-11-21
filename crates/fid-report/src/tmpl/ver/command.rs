//! The four template commands: parameters, handlers, results and the `--json` dispatcher.
//!
//! Every handler checks everything that can fail before it draws an ID or touches the
//! store, and hands its writes to the backend in one commit, so a failed command stores
//! and journals nothing.

use super::convert::{
    check_template, open_stored, report_from_template, source_template, template_from_report, version_bytes,
};
use super::error::VerError;
use super::event::{SaveSource, TemplateEvent, APPLY_TEMPLATE, LIST_TEMPLATE, SAVE_TEMPLATE, SHOW_TEMPLATE};
use super::store::{digest_of, BuiltinRegistry, Origin, TemplateBackend, TemplateIndex};
use crate::doc::model::{obj, parse_canonical_uuid, u32_value};
use crate::doc::{
    summary_value, KindRegistry, Report, ReportError, ReportId, StoredReport, TemplateRef, Text, Untrusted,
};
use crate::tmpl::format::{parse_template, serialise, Template, TemplateError, TemplateId, MAX_TEMPLATE_BYTES};
use fid_canon::{parse_with_limits, CValue};
use fid_io_reg::{Principal, RootSet};
use fid_project::{uuid_v7, ObservationSource, Uuid};
use std::collections::BTreeMap;
use std::io::Read;

/// Longest `file` parameter, in bytes.
pub const MAX_FILE_PARAM_BYTES: usize = 4096;

// ------------------------------------------------------------------ params

/// The source of `report.save_template@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// `from_report`: a stored report.
    Report(Uuid),
    /// `file`: an import-root handle (`root://<root>/<path>`), or for a human a raw path.
    File(String),
}

/// `report.save_template@1` parameters: `{template, from_report}` or `{template, file}`
/// (exactly one source).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveTemplate {
    pub template: TemplateId,
    pub source: Source,
}

/// `report.apply_template@1` parameters: `{template, version?: "<u32>" | null, title}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyTemplate {
    pub template: TemplateId,
    pub version: Option<u32>,
    pub title: Untrusted<Text>,
}

/// `report.show_template@1` parameters: `{template, version?: "<u32>" | null}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShowTemplate {
    pub template: TemplateId,
    pub version: Option<u32>,
}

fn param_obj<'a>(v: &'a CValue, allowed: &[&str]) -> Result<&'a BTreeMap<String, CValue>, VerError> {
    let CValue::Obj(o) = v else { return Err(ReportError::BadParams("params is not an object").into()) };
    if o.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(ReportError::BadParams("unknown parameter").into());
    }
    Ok(o)
}

fn uuid_param(o: &BTreeMap<String, CValue>, key: &str) -> Result<Uuid, VerError> {
    match o.get(key) {
        Some(CValue::Str(s)) => {
            parse_canonical_uuid(s).ok_or_else(|| ReportError::BadParams("id is not a canonical uuid").into())
        }
        Some(_) => Err(ReportError::BadParams("id is not a string").into()),
        None => Err(ReportError::BadParams("missing id parameter").into()),
    }
}

fn template_param(o: &BTreeMap<String, CValue>) -> Result<TemplateId, VerError> {
    let u = uuid_param(o, "template")?;
    TemplateId::new(u).ok_or(VerError::NotFound { id: u, version: None })
}

fn version_param(o: &BTreeMap<String, CValue>) -> Result<Option<u32>, VerError> {
    match o.get("version") {
        None | Some(CValue::Null) => Ok(None),
        Some(v @ CValue::Str(_)) => match u32_value(v) {
            Ok(n) if n >= 1 => Ok(Some(n)),
            _ => Err(ReportError::BadParams("version is not a decimal string of at least 1").into()),
        },
        Some(_) => Err(ReportError::BadParams("version is not a string").into()),
    }
}

impl SaveTemplate {
    pub fn from_value(v: &CValue) -> Result<SaveTemplate, VerError> {
        let o = param_obj(v, &["template", "from_report", "file"])?;
        let template = template_param(o)?;
        let source = match (o.get("from_report"), o.get("file")) {
            (Some(_), None) => Source::Report(uuid_param(o, "from_report")?),
            (None, Some(CValue::Str(f))) if !f.is_empty() && f.len() <= MAX_FILE_PARAM_BYTES => Source::File(f.clone()),
            (None, Some(_)) => return Err(ReportError::BadParams("file is not a string of 1 to 4096 bytes").into()),
            _ => return Err(ReportError::BadParams("exactly one of from_report and file").into()),
        };
        Ok(SaveTemplate { template, source })
    }
}

impl ApplyTemplate {
    pub fn from_value(v: &CValue) -> Result<ApplyTemplate, VerError> {
        let o = param_obj(v, &["template", "version", "title"])?;
        let title = match o.get("title") {
            Some(CValue::Str(s)) => Untrusted::text(s)?,
            Some(_) => return Err(ReportError::BadParams("title is not a string").into()),
            None => return Err(ReportError::BadParams("missing title").into()),
        };
        Ok(ApplyTemplate { template: template_param(o)?, version: version_param(o)?, title })
    }
}

impl ShowTemplate {
    pub fn from_value(v: &CValue) -> Result<ShowTemplate, VerError> {
        let o = param_obj(v, &["template", "version"])?;
        Ok(ShowTemplate { template: template_param(o)?, version: version_param(o)? })
    }
}

/// Splits a CLI template selector `<uuid>` or `<uuid>@<version>` into the `template` and
/// `version` parameters. `report.bad_params` on anything else.
pub fn parse_selector(text: &str) -> Result<(TemplateId, Option<u32>), VerError> {
    let (id, version) = match text.split_once('@') {
        None => (text, None),
        Some((id, v)) => {
            let n = u32_value(&CValue::Str(v.to_string()))
                .ok()
                .filter(|n| *n >= 1)
                .ok_or(ReportError::BadParams("version is not a decimal of at least 1"))?;
            (id, Some(n))
        }
    };
    let u = parse_canonical_uuid(id).ok_or(ReportError::BadParams("template is not a canonical uuid"))?;
    let t = TemplateId::new(u).ok_or(VerError::NotFound { id: u, version })?;
    Ok((t, version))
}

// ------------------------------------------------------------------ context

/// What the template commands need besides the project.
pub struct Env<'a> {
    /// The block-kind registry (REQ-008).
    pub kinds: &'a KindRegistry,
    /// The built-in templates.
    pub builtins: &'a BuiltinRegistry,
    /// The approved import roots, for `file` sources.
    pub roots: &'a RootSet,
    /// Who runs the command: agents and scripts must pass import-root handles (REQ-007).
    pub principal: Principal,
}

fn index(backend: &dyn TemplateBackend, builtins: &BuiltinRegistry) -> Result<TemplateIndex, VerError> {
    Ok(TemplateIndex::fold(&backend.template_events()?, builtins)?)
}

fn not_found(id: TemplateId, version: Option<u32>) -> VerError {
    VerError::NotFound { id: id.uuid(), version }
}

/// A fresh UUIDv7 from the observation source.
fn draw(ids: &mut dyn ObservationSource) -> Result<Uuid, ReportError> {
    let (ms, bits) = ids.draw_v7().map_err(|e| ReportError::IdUnavailable(e.code()))?;
    let id = uuid_v7(ms, &bits).map_err(|e| ReportError::IdUnavailable(e.code()))?;
    if !crate::doc::is_v7(&id) || id == Uuid::NIL || id == Uuid::MAX {
        return Err(ReportError::IdUnavailable("bad_id"));
    }
    Ok(id)
}

/// Reads a template file through the import roots: `raw_path_forbidden` for a raw path
/// from an agent or a script (before any file-system call), the resolver's other codes,
/// and at most [`MAX_TEMPLATE_BYTES`] + 1 bytes (the parser refuses the extra byte).
fn read_file(roots: &RootSet, principal: Principal, file: &str) -> Result<Template, VerError> {
    let opened = roots.resolve_import(principal, file).map_err(|e| VerError::File(e.code()))?;
    let limit = MAX_TEMPLATE_BYTES as u64 + 1;
    let mut bytes = Vec::new();
    opened.file.take(limit).read_to_end(&mut bytes).map_err(|_| VerError::File("unreadable"))?;
    Ok(parse_template(&bytes)?)
}

// ------------------------------------------------------------------ handlers

/// The result of a save: the version and whether this call created it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Saved {
    pub template: TemplateRef,
    pub created: bool,
}

/// `report.save_template@1` (REQ-001, REQ-003, REQ-005, REQ-007, REQ-008). Order: params,
/// lock, source (report or file), template ID of a file, slot / kind / payload / value /
/// label checks, then either the existing latest version (identical bytes, nothing
/// journaled) or version n+1 committed with one event.
pub fn save_template(backend: &mut dyn TemplateBackend, env: &Env<'_>, p: &SaveTemplate) -> Result<Saved, VerError> {
    let ix = index(backend, env.builtins)?;
    let latest = ix.latest(&p.template, env.builtins);
    let builtin_locked = env.builtins.versions(&p.template).iter().any(|b| b.locked);
    if builtin_locked || latest.as_ref().is_some_and(Origin::locked) {
        return Err(VerError::Locked(p.template));
    }
    let (template, source) = match &p.source {
        Source::Report(r) => {
            let rid = ReportId::new(*r).ok_or(VerError::NotFound { id: *r, version: None })?;
            let stored = backend.load(&rid)?.ok_or(VerError::NotFound { id: *r, version: None })?;
            let report = stored.open(&rid, env.kinds)?;
            let from = source_template(&report, &*backend)?;
            let t = template_from_report(&report, p.template, from.as_ref())?;
            (t, SaveSource::Report { report: rid, report_digest: stored.digest })
        }
        Source::File(f) => {
            let t = read_file(env.roots, env.principal, f)?;
            if t.template_id() != p.template {
                return Err(TemplateError::invalid("/template_id", "template_id_mismatch").into());
            }
            (t, SaveSource::File { handle: Untrusted::text(f)? })
        }
    };
    check_template(&template, env.kinds)?;
    let bytes = serialise(&template)?.into_vec();
    let digest = digest_of(&bytes);
    if let Some(l) = &latest {
        if l.r().digest == digest {
            return Ok(Saved { template: l.r(), created: false });
        }
    }
    let version = match &latest {
        None => 1,
        Some(l) => l.r().version.checked_add(1).ok_or(ReportError::LimitExceeded("template versions"))?,
    };
    let r = TemplateRef { id: p.template.uuid(), version, digest };
    // The fold's own rules (increasing versions, bounds) are checked before the commit.
    ix.clone().push(r, template.locked(), env.builtins)?;
    backend.commit_save(bytes, TemplateEvent::Save { template: r, locked: template.locked(), source })?;
    Ok(Saved { template: r, created: true })
}

/// Resolves `id@version` (latest for `None`) and returns its reference and parsed,
/// verified template.
fn resolve(
    backend: &dyn TemplateBackend,
    builtins: &BuiltinRegistry,
    id: TemplateId,
    version: Option<u32>,
) -> Result<(TemplateRef, Vec<u8>, Template), VerError> {
    let ix = index(backend, builtins)?;
    let origin = ix.get(&id, version, builtins).ok_or_else(|| not_found(id, version))?;
    let r = origin.r();
    let bytes = version_bytes(&origin, backend)?;
    let t = open_stored(&bytes, &r)?;
    Ok((r, bytes, t))
}

/// `report.apply_template@1` (REQ-004, REQ-008): a new report from a template version,
/// recording its ID, version and digest. The template's bytes are stored in the
/// content-addressed store with the report (a built-in's too), so the report replays even
/// after the built-in changes.
pub fn apply_template(
    backend: &mut dyn TemplateBackend,
    env: &Env<'_>,
    ids: &mut dyn ObservationSource,
    p: &ApplyTemplate,
) -> Result<Report, VerError> {
    let (r, bytes, t) = resolve(backend, env.builtins, p.template, p.version)?;
    check_template(&t, env.kinds)?;
    let report = ReportId::new(draw(ids)?).ok_or(ReportError::IdUnavailable("bad_id"))?;
    if backend.load(&report)?.is_some() || report.uuid() == backend.project() {
        return Err(ReportError::IdCollision(report.uuid()).into());
    }
    let doc = report_from_template(&t, r, report, &p.title, env.kinds)?;
    let stored = StoredReport::of(&doc)?;
    let event = TemplateEvent::Apply { report, title: p.title.clone(), template: r, report_digest: stored.digest };
    backend.commit_apply(bytes, &report, stored, event)?;
    Ok(doc)
}

/// `report.show_template@1` (REQ-006): one version's canonical document.
pub fn show_template(
    backend: &dyn TemplateBackend,
    builtins: &BuiltinRegistry,
    p: &ShowTemplate,
) -> Result<(TemplateRef, CValue), VerError> {
    let (r, bytes, _) = resolve(backend, builtins, p.template, p.version)?;
    let v = parse_with_limits(&bytes, MAX_TEMPLATE_BYTES, fid_canon::MAX_DEPTH)
        .map_err(|_| ReportError::StoreInvalid("stored template does not parse"))?;
    Ok((r, v))
}

/// One row of `report.list_template@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateInfo {
    pub template: TemplateRef,
    pub locked: bool,
    pub builtin: bool,
}

/// `report.list_template@1` (REQ-006): built-in and project versions, sorted by template
/// ID, then version, with their locked flags.
pub fn list_template(backend: &dyn TemplateBackend, builtins: &BuiltinRegistry) -> Result<Vec<TemplateInfo>, VerError> {
    let ix = index(backend, builtins)?;
    let mut ids: Vec<TemplateId> = builtins.iter().filter_map(|b| TemplateId::new(b.r.id)).collect();
    ids.extend(ix.project_ids().copied());
    ids.sort();
    ids.dedup();
    let mut out = Vec::new();
    for id in ids {
        for o in ix.all(&id, builtins) {
            out.push(TemplateInfo { template: o.r(), locked: o.locked(), builtin: matches!(o, Origin::Builtin(_)) });
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ results

/// `{"created": bool, "template": {digest, id, version}}`.
pub fn saved_value(s: &Saved) -> CValue {
    obj([("created", CValue::Bool(s.created)), ("template", s.template.to_value())])
}

/// The `report.apply_template@1` result: the report summary of `RPT.MODEL.DOC` plus
/// `template` (`{digest, id, version}`).
pub fn applied_value(r: &Report) -> Result<CValue, VerError> {
    let mut v = summary_value(r)?;
    if let (CValue::Obj(o), Some(t)) = (&mut v, r.template_ref()) {
        o.insert("template".to_string(), t.to_value());
    }
    Ok(v)
}

/// `{"template": {digest, id, version}, "document": <canonical template document>}`.
pub fn shown_value(r: &TemplateRef, doc: CValue) -> CValue {
    obj([("document", doc), ("template", r.to_value())])
}

/// `{"templates": [{"digest", "id", "locked", "origin": "builtin" | "project", "version"}]}`.
pub fn list_value(rows: &[TemplateInfo]) -> CValue {
    let items = rows
        .iter()
        .map(|i| {
            let mut o = match i.template.to_value() {
                CValue::Obj(o) => o,
                _ => BTreeMap::new(),
            };
            o.insert("locked".to_string(), CValue::Bool(i.locked));
            o.insert("origin".to_string(), CValue::Str(if i.builtin { "builtin" } else { "project" }.to_string()));
            CValue::Obj(o)
        })
        .collect();
    obj([("templates", CValue::Arr(items))])
}

/// Runs one template command from its ID and `--json` parameters and returns its `--json`
/// result. An unknown command is `report.bad_params`.
pub fn execute(
    command: &str,
    params: &CValue,
    backend: &mut dyn TemplateBackend,
    env: &Env<'_>,
    ids: &mut dyn ObservationSource,
) -> Result<CValue, VerError> {
    match command {
        SAVE_TEMPLATE => save_template(backend, env, &SaveTemplate::from_value(params)?).map(|s| saved_value(&s)),
        APPLY_TEMPLATE => applied_value(&apply_template(backend, env, ids, &ApplyTemplate::from_value(params)?)?),
        SHOW_TEMPLATE => {
            show_template(backend, env.builtins, &ShowTemplate::from_value(params)?).map(|(r, d)| shown_value(&r, d))
        }
        LIST_TEMPLATE => {
            param_obj(params, &[])?;
            list_template(backend, env.builtins).map(|l| list_value(&l))
        }
        _ => Err(ReportError::BadParams("unknown command").into()),
    }
}

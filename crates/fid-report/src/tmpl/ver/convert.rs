//! Template checks and the two conversions: report to template (save from a report) and
//! template to report (apply).

use super::error::VerError;
use super::store::{digest_of, read_object, ObjectRead, Origin};
use crate::doc::model::{check_block, obj};
use crate::doc::{
    CanonicalJson, KindRegistry, PayloadError, Report, ReportError, ReportId, TemplateRef, Text, Untrusted,
};
use crate::tmpl::format::{parse_template, serialise, Template, TemplateError, TemplateId, MEDIA_TYPE};
use crate::tmpl::slot::validate_template;
use fid_canon::{to_canonical_bytes, CValue};
use fid_gdt::chars::interp::{PROVISIONAL_LABEL_KEY, PROVISIONAL_LABEL_TEXT};
use std::collections::{BTreeMap, BTreeSet};

/// Object keys a template payload may never hold anywhere (D-045): a template has no say
/// over which labels a value carries, so nothing in it may even look like a label switch.
/// Labels are added by `RPT.MODEL.FMT` from the stored record only.
pub const LABEL_CONTROL_KEYS: [&str; 8] = [
    "hide_labels",
    "interpretation",
    "label_mode",
    "labels",
    "provisional",
    "show_labels",
    "suppress_labels",
    "value_labels",
];

fn pointer_push(out: &mut String, token: &str) {
    out.push('/');
    for c in token.chars() {
        match c {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            c => out.push(c),
        }
    }
}

/// The first label-control offence in `v` (depth first, keys in order): an object key in
/// [`LABEL_CONTROL_KEYS`], or a string equal to the provisional label's catalog key or
/// English text. Iterative; `v` is a bounded payload.
fn label_offence(v: &CValue, at: &str) -> Option<String> {
    let mut stack: Vec<(String, &CValue)> = vec![(at.to_string(), v)];
    while let Some((p, node)) = stack.pop() {
        match node {
            CValue::Str(s) if s == PROVISIONAL_LABEL_KEY || s == PROVISIONAL_LABEL_TEXT => return Some(p),
            CValue::Obj(o) => {
                for k in o.keys() {
                    if LABEL_CONTROL_KEYS.contains(&k.as_str()) {
                        let mut q = p.clone();
                        pointer_push(&mut q, k);
                        return Some(q);
                    }
                }
                for (k, c) in o.iter().rev() {
                    let mut q = p.clone();
                    pointer_push(&mut q, k);
                    stack.push((q, c));
                }
            }
            CValue::Arr(a) => {
                for (i, c) in a.iter().enumerate().rev() {
                    let mut q = p.clone();
                    pointer_push(&mut q, &i.to_string());
                    stack.push((q, c));
                }
            }
            _ => {}
        }
    }
    None
}

/// Every check a template must pass before it is saved or applied, in this order: slots
/// (`RPT.MODEL.TMPL.SLOT`, `report.slot_invalid`); per block in document order the kind
/// registry, the payload check and the value-field rule (REQ-008); then the label rule
/// (D-045, `report.template_invalid` reason `label_control`).
pub(crate) fn check_template(t: &Template, kinds: &KindRegistry) -> Result<(), VerError> {
    validate_template(t)?;
    for page in t.pages() {
        for b in page.blocks() {
            let payload = CanonicalJson::new(b.payload().value())
                .ok_or_else(|| ReportError::InvalidBlockPayload(PayloadError::new("", "payload_bounds")))?;
            check_block(kinds, b.kind(), b.kind_version(), &payload)?;
        }
    }
    for (pi, page) in t.pages().iter().enumerate() {
        for (bi, b) in page.blocks().iter().enumerate() {
            let at = format!("/pages/{pi}/blocks/{bi}/payload/value");
            if let Some(p) = label_offence(b.payload().value(), &at) {
                return Err(TemplateError::invalid(&p, "label_control").into());
            }
        }
    }
    Ok(())
}

/// Parses stored template bytes and checks they are exactly the canonical form of a
/// template with the expected ID. Anything else is `report.store_invalid`: stored bytes are
/// trusted only after this check.
pub(crate) fn open_stored(bytes: &[u8], r: &TemplateRef) -> Result<Template, ReportError> {
    if digest_of(bytes) != r.digest {
        return Err(ReportError::StoreInvalid("template bytes do not hash to the digest"));
    }
    let t = parse_template(bytes).map_err(|_| ReportError::StoreInvalid("stored template does not parse"))?;
    if t.template_id().uuid() != r.id {
        return Err(ReportError::StoreInvalid("stored template has another id"));
    }
    match serialise(&t) {
        Ok(c) if c.as_bytes() == bytes => Ok(t),
        _ => Err(ReportError::StoreInvalid("stored template is not canonical")),
    }
}

/// The bytes of a resolved version: a built-in's own bytes, or the object from the store.
pub(crate) fn version_bytes<S: ObjectRead + ?Sized>(origin: &Origin<'_>, store: &S) -> Result<Vec<u8>, ReportError> {
    match origin {
        Origin::Builtin(b) => Ok(b.bytes.clone()),
        Origin::Project(p) => {
            read_object(store, &p.r.digest)?.ok_or(ReportError::StoreInvalid("template object missing"))
        }
    }
}

/// The template a report was applied from, read from the store by its recorded digest.
/// `None` when the report has no template reference.
pub(crate) fn source_template<S: ObjectRead + ?Sized>(
    report: &Report,
    store: &S,
) -> Result<Option<Template>, ReportError> {
    let Some(r) = report.template_ref() else { return Ok(None) };
    let bytes = read_object(store, &r.digest)?.ok_or(ReportError::StoreInvalid("template object missing"))?;
    open_stored(&bytes, &r).map(Some)
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

/// The template saved from `report` under `id` (save from a report). Pages, blocks, their
/// IDs, kinds, kind versions and payloads are the report's; the title is the report's.
/// When the report was applied from `source`, a block with the same ID, kind and kind
/// version keeps that template block's slot and `repeat_header_rows`, and its
/// `continuation_block` when the target is still on the same page; every other block gets
/// no slot, `"0"` and `null`. The result is not locked and has no clearance. It goes
/// through the format parser, so a payload holding a bound value or a number fails as
/// `report.template_invalid`.
pub(crate) fn template_from_report(
    report: &Report,
    id: TemplateId,
    source: Option<&Template>,
) -> Result<Template, VerError> {
    let mut from: BTreeMap<_, _> = BTreeMap::new();
    if let Some(t) = source {
        for b in t.pages().iter().flat_map(|p| p.blocks()) {
            from.insert(b.id(), b);
        }
    }
    let mut pages = Vec::with_capacity(report.pages().len());
    for p in report.pages() {
        let on_page: BTreeSet<_> = p.blocks().iter().map(|b| b.id()).collect();
        let mut blocks = Vec::with_capacity(p.blocks().len());
        for b in p.blocks() {
            let kept = from.get(&b.id()).filter(|t| t.kind() == b.kind() && t.kind_version() == b.kind_version());
            let slot = kept
                .and_then(|t| t.slot())
                .map_or(CValue::Null, |sl| obj([("kind", s(sl.kind().as_str())), ("spec", sl.spec().to_value())]));
            let rows = kept.map_or(0, |t| t.repeat_header_rows());
            let cont = kept
                .and_then(|t| t.continuation_block())
                .filter(|c| on_page.contains(c))
                .map_or(CValue::Null, |c| s(&c.uuid().to_string()));
            blocks.push(obj([
                ("continuation_block", cont),
                ("id", s(&b.id().uuid().to_string())),
                ("kind", s(b.kind().as_str())),
                ("kind_version", s(&b.kind_version().to_string())),
                ("payload", obj([("untrusted", CValue::Bool(true)), ("value", b.payload().value().clone())])),
                ("repeat_header_rows", s(&rows.to_string())),
                ("slot", slot),
            ]));
        }
        pages.push(obj([("blocks", CValue::Arr(blocks)), ("id", s(&p.id().uuid().to_string()))]));
    }
    let v = obj([
        ("clearance", CValue::Null),
        ("locked", CValue::Bool(false)),
        ("media_type", s(MEDIA_TYPE)),
        ("pages", CValue::Arr(pages)),
        ("schema_version", s("1")),
        ("template_id", s(&id.uuid().to_string())),
        ("title", report.title().to_value()),
    ]);
    let bytes = to_canonical_bytes(&v).map_err(|_| TemplateError::invalid("", "non_finite_number"))?;
    Ok(parse_template(&bytes)?)
}

/// The report applied from template `t` (version `r`): ID `report`, title `title`, the
/// template's pages and blocks with their IDs, kinds, kind versions and payloads, no
/// retired IDs, and `template_ref = r`. A block's slot stays in the template: the block
/// keeps the template block's ID, so `RPT.MODEL.BIND` finds it through the recorded
/// digest. Read back through the strict report reader, which re-checks every block
/// against the kind registry.
pub(crate) fn report_from_template(
    t: &Template,
    r: TemplateRef,
    report: ReportId,
    title: &Untrusted<Text>,
    kinds: &KindRegistry,
) -> Result<Report, VerError> {
    let taken = report.uuid() == t.template_id().uuid()
        || t.pages()
            .iter()
            .any(|p| p.id().uuid() == report.uuid() || p.blocks().iter().any(|b| b.id().uuid() == report.uuid()));
    if taken {
        return Err(ReportError::IdCollision(report.uuid()).into());
    }
    let pages = t
        .pages()
        .iter()
        .map(|p| {
            let blocks = p
                .blocks()
                .iter()
                .map(|b| {
                    obj([
                        ("id", s(&b.id().uuid().to_string())),
                        ("kind", s(b.kind().as_str())),
                        ("kind_version", CValue::U64(u64::from(b.kind_version()))),
                        ("payload", obj([("untrusted", CValue::Bool(true)), ("value", b.payload().value().clone())])),
                    ])
                })
                .collect();
            obj([("blocks", CValue::Arr(blocks)), ("id", s(&p.id().uuid().to_string()))])
        })
        .collect();
    let v = obj([
        ("id", s(&report.uuid().to_string())),
        ("pages", CValue::Arr(pages)),
        ("retired_ids", CValue::Arr(Vec::new())),
        ("schema_version", CValue::U64(u64::from(crate::doc::SCHEMA_VERSION))),
        ("template_ref", r.to_value()),
        ("title", title.to_value()),
    ]);
    Ok(Report::from_value(&v, kinds)?)
}

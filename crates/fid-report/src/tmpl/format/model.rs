//! The template types and their canonical JSON form (schema version 1).
//!
//! A template can never hold a bound or measured value: a block's payload and a slot's
//! spec are [`TemplateJson`], whose only constructor refuses every JSON number and every
//! object tagged `value_binding@...` or `value_text@...`, wherever it sits. No type here
//! has a field for a record, a digest of a record or a value; templates name QIF and
//! project entities only by stable ID inside slots.

use super::error::{push_token, TemplateError};
use super::MEDIA_TYPE;
use crate::doc::{is_v7, BlockId, BlockKindId, CanonicalJson, PageId, Text, Untrusted};
use fid_canon::CValue;
use fid_project::Uuid;
use std::collections::BTreeMap;

/// A template ID (UUIDv7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TemplateId(Uuid);

impl TemplateId {
    /// Wraps a UUIDv7; any other UUID is refused.
    pub fn new(id: Uuid) -> Option<TemplateId> {
        is_v7(&id).then_some(TemplateId(id))
    }

    pub fn uuid(&self) -> Uuid {
        self.0
    }
}

/// Template-level clearance state (D-015 counsel review of built-in form templates; the
/// gate itself belongs to `RPT.FORMS.LAYOUT.MAP`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Clearance {
    Pending,
    /// Cleared, with a reference to the counsel review record.
    Cleared {
        review_ref: Untrusted<Text>,
    },
}

/// The binding-rule kind of a slot (the closed list of REQ-RPT.MODEL.TMPL.SLOT-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SlotKind {
    Characteristics,
    Elements,
    Tags,
    Stages,
    Runs,
    SavedQuery,
    FieldTable,
}

impl SlotKind {
    /// Every slot kind, in the order of the spec list.
    pub const ALL: [SlotKind; 7] = [
        SlotKind::Characteristics,
        SlotKind::Elements,
        SlotKind::Tags,
        SlotKind::Stages,
        SlotKind::Runs,
        SlotKind::SavedQuery,
        SlotKind::FieldTable,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SlotKind::Characteristics => "characteristics",
            SlotKind::Elements => "elements",
            SlotKind::Tags => "tags",
            SlotKind::Stages => "stages",
            SlotKind::Runs => "runs",
            SlotKind::SavedQuery => "saved_query",
            SlotKind::FieldTable => "field_table",
        }
    }

    pub fn parse(s: &str) -> Option<SlotKind> {
        SlotKind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// Bounded canonical JSON that holds no value: no JSON number anywhere and no object whose
/// `schema` member is a string starting `value_binding@` or `value_text@`. Integers in
/// templates are decimal strings, as everywhere in the report model.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateJson(CanonicalJson);

/// Prefixes of the `schema` tags a template may never hold.
const BOUND_VALUE_PREFIX: &str = "value_binding@";
const VALUE_TEXT_PREFIX: &str = "value_text@";

impl TemplateJson {
    /// Checks and wraps `v`. Errors name `at` plus the pointer inside `v`:
    /// `value_too_large` (over the payload bounds of `RPT.MODEL.DOC`), then the first
    /// offence in canonical order (`number`, `bound_value`, `value_text`).
    pub fn new(v: &CValue, at: &str) -> Result<TemplateJson, TemplateError> {
        let c = CanonicalJson::new(v).ok_or_else(|| TemplateError::invalid(at, "value_too_large"))?;
        if let Some((pointer, reason)) = first_offence(c.value()) {
            let mut p = at.to_string();
            p.push_str(&pointer);
            return Err(TemplateError::invalid(&p, reason));
        }
        Ok(TemplateJson(c))
    }

    pub fn value(&self) -> &CValue {
        self.0.value()
    }

    /// Machine form: `{"untrusted": true, "value": <json>}`.
    pub(crate) fn to_value(&self) -> CValue {
        obj([("untrusted", CValue::Bool(true)), ("value", self.0.value().clone())])
    }
}

#[derive(Clone, Copy)]
enum Step<'a> {
    Root,
    Key(&'a str),
    Index(usize),
}

/// Depth-first, objects in key order and arrays in index order; memory is one borrowed
/// trail entry per visited node (a bounded payload has at most as many nodes as bytes).
fn first_offence(root: &CValue) -> Option<(String, &'static str)> {
    let mut trail: Vec<(usize, Step<'_>)> = Vec::new();
    let mut stack: Vec<(usize, &CValue)> = Vec::new();
    trail.push((usize::MAX, Step::Root));
    stack.push((0, root));
    while let Some((i, node)) = stack.pop() {
        let reason = match node {
            CValue::Num(_) | CValue::I64(_) | CValue::U64(_) | CValue::F64Bits(_) => Some("number"),
            CValue::Obj(o) => match o.get("schema") {
                Some(CValue::Str(s)) if s.starts_with(BOUND_VALUE_PREFIX) => Some("bound_value"),
                Some(CValue::Str(s)) if s.starts_with(VALUE_TEXT_PREFIX) => Some("value_text"),
                _ => None,
            },
            _ => None,
        };
        if let Some(r) = reason {
            return Some((trail_pointer(&trail, i), r));
        }
        match node {
            CValue::Obj(o) => {
                for (k, c) in o.iter().rev() {
                    stack.push((trail.len(), c));
                    trail.push((i, Step::Key(k)));
                }
            }
            CValue::Arr(a) => {
                for (j, c) in a.iter().enumerate().rev() {
                    stack.push((trail.len(), c));
                    trail.push((i, Step::Index(j)));
                }
            }
            _ => {}
        }
    }
    None
}

fn trail_pointer(trail: &[(usize, Step<'_>)], i: usize) -> String {
    let mut steps = Vec::new();
    let mut at = i;
    while let Some((parent, step)) = trail.get(at) {
        steps.push(*step);
        at = *parent;
    }
    let mut out = String::new();
    for s in steps.iter().rev() {
        match s {
            Step::Root => {}
            Step::Key(k) => push_token(&mut out, k),
            Step::Index(j) => push_token(&mut out, &j.to_string()),
        }
    }
    out
}

/// A slot: a binding-rule kind and its spec, which `RPT.MODEL.TMPL.SLOT` validates.
/// Carried here as checked [`TemplateJson`] holding an object.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub(crate) kind: SlotKind,
    pub(crate) spec: TemplateJson,
}

impl Slot {
    pub fn kind(&self) -> SlotKind {
        self.kind
    }

    pub fn spec(&self) -> &TemplateJson {
        &self.spec
    }
}

/// One template block: a report block whose payload is [`TemplateJson`], an optional
/// slot, and the layout flags.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateBlock {
    pub(crate) id: BlockId,
    pub(crate) kind: BlockKindId,
    pub(crate) kind_version: u32,
    pub(crate) payload: TemplateJson,
    pub(crate) slot: Option<Slot>,
    pub(crate) repeat_header_rows: u8,
    pub(crate) continuation_block: Option<BlockId>,
}

impl TemplateBlock {
    pub fn id(&self) -> BlockId {
        self.id
    }

    pub fn kind(&self) -> &BlockKindId {
        &self.kind
    }

    pub fn kind_version(&self) -> u32 {
        self.kind_version
    }

    pub fn payload(&self) -> &TemplateJson {
        &self.payload
    }

    pub fn slot(&self) -> Option<&Slot> {
        self.slot.as_ref()
    }

    /// Header rows a table repeats on each continuation page, 0 to 8.
    pub fn repeat_header_rows(&self) -> u8 {
        self.repeat_header_rows
    }

    /// The block on the same page that continues this one.
    pub fn continuation_block(&self) -> Option<BlockId> {
        self.continuation_block
    }

    fn to_value(&self) -> CValue {
        let slot = match &self.slot {
            None => CValue::Null,
            Some(s) => obj([("kind", CValue::Str(s.kind.as_str().to_string())), ("spec", s.spec.to_value())]),
        };
        obj([
            ("continuation_block", self.continuation_block.map_or(CValue::Null, |b| uuid_value(b.uuid()))),
            ("id", uuid_value(self.id.uuid())),
            ("kind", CValue::Str(self.kind.as_str().to_string())),
            ("kind_version", CValue::Str(self.kind_version.to_string())),
            ("payload", self.payload.to_value()),
            ("repeat_header_rows", CValue::Str(self.repeat_header_rows.to_string())),
            ("slot", slot),
        ])
    }
}

/// One template page.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplatePage {
    pub(crate) id: PageId,
    pub(crate) blocks: Vec<TemplateBlock>,
}

impl TemplatePage {
    pub fn id(&self) -> PageId {
        self.id
    }

    pub fn blocks(&self) -> &[TemplateBlock] {
        &self.blocks
    }
}

/// A report template (schema version 1). Built only by [`super::parse_template`], so every
/// instance passed the budgets and the schema check.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub(crate) schema_version: u32,
    pub(crate) template_id: TemplateId,
    pub(crate) title: Untrusted<Text>,
    pub(crate) locked: bool,
    pub(crate) clearance: Option<Clearance>,
    pub(crate) pages: Vec<TemplatePage>,
}

impl Template {
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn template_id(&self) -> TemplateId {
        self.template_id
    }

    pub fn title(&self) -> &Untrusted<Text> {
        &self.title
    }

    /// A locked template may not be edited or saved over (enforced by `RPT.MODEL.TMPL.VER`).
    pub fn locked(&self) -> bool {
        self.locked
    }

    pub fn clearance(&self) -> Option<&Clearance> {
        self.clearance.as_ref()
    }

    pub fn pages(&self) -> &[TemplatePage] {
        &self.pages
    }

    /// Number of blocks over all pages.
    pub fn block_count(&self) -> usize {
        self.pages.iter().map(|p| p.blocks.len()).sum()
    }

    /// The canonical JSON value (schema version 1).
    pub fn to_value(&self) -> CValue {
        let clearance = match &self.clearance {
            None => CValue::Null,
            Some(Clearance::Pending) => obj([("state", CValue::Str("pending".to_string()))]),
            Some(Clearance::Cleared { review_ref }) => {
                obj([("review_ref", review_ref.to_value()), ("state", CValue::Str("cleared".to_string()))])
            }
        };
        let pages = self
            .pages
            .iter()
            .map(|p| {
                obj([
                    ("blocks", CValue::Arr(p.blocks.iter().map(TemplateBlock::to_value).collect())),
                    ("id", uuid_value(p.id.uuid())),
                ])
            })
            .collect();
        obj([
            ("clearance", clearance),
            ("locked", CValue::Bool(self.locked)),
            ("media_type", CValue::Str(MEDIA_TYPE.to_string())),
            ("pages", CValue::Arr(pages)),
            ("schema_version", CValue::Str(self.schema_version.to_string())),
            ("template_id", uuid_value(self.template_id.uuid())),
            ("title", self.title.to_value()),
        ])
    }
}

fn uuid_value(u: Uuid) -> CValue {
    CValue::Str(u.to_string())
}

pub(crate) fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>())
}

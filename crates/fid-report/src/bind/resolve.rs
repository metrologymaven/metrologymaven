//! Resolution of a block's binding against an explicit journal head (REQ-002..005, -007).
//!
//! [`resolve_block`] runs these steps and returns the first failure; it reads only, so
//! nothing is ever journaled:
//!
//! 1. the block's kind has a registered [`BlockResolver`] (`report.no_resolver`);
//! 2. the head is an entry of the journal (`report.not_found`);
//! 3. the stored `binding@1` reads back (`report.store_invalid`);
//! 4. every reference, in binding order, as of the head: an entity of another project is
//!    `report.cross_project_binding` (block and foreign project only); one committed after
//!    the head is `report.unjournaled_record` (block and ID); an absent one, a field no
//!    [`FieldSource`] supplies, or an entity of another kind is `report.not_found`;
//! 5. every declared value field, in the kind's order: its `value_binding@1` reads back
//!    (DOC's codes), names a record the binding references (`report.value_binding_invalid`,
//!    `record_not_bound`); the SHA-256 of the stored record equals `record_digest`
//!    (`report.binding_digest_mismatch`, `record_digest`); `field` is one of the record's
//!    value fields (`report.binding_field_not_value`); the record references exactly
//!    `statement_digest` for that field and the stored statement hashes to it
//!    (`report.binding_digest_mismatch`, `statement_digest`);
//! 6. the kind's resolver builds the [`ResolvedBlock`] from the typed [`Resolved`] view.
//!
//! Values reach the kind only as [`Quantity`] (an [`crate::fmt::SiValue`] and a unit), and
//! never from the block or the caller: from the stored record whose digest was just
//! recomputed. A [`ResolvedBlock`]'s layout may hold no JSON number.

use super::error::BindError;
use super::journal::{EntityView, HeadLookup, JournalReads, Lookup, LookupFault, StatementBytes};
use super::model::{Binding, FieldId, JournalHead, Quantity, Ref, ValueBinding};
use crate::doc::kind::PayloadError;
use crate::doc::value::resolve_pointer;
use crate::doc::{Block, BlockId, BlockKindId, CanonicalJson, KindRegistry, ReportError, Text, Untrusted};
use fid_canon::CValue;
use fid_project::Uuid;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Most resolvers one registry holds.
pub const MAX_RESOLVERS: usize = 256;

// ------------------------------------------------------------------ field sources

/// A form field's value: untrusted text, or a ResultRecord the field shows (resolved by
/// the block kind through [`Lookup`]). Never a number.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Text(Untrusted<Text>),
    Result(Uuid),
}

/// One table row a form supplies, keyed by column field ID.
pub type RowSource = BTreeMap<FieldId, FieldValue>;

/// Form data (FAI, PPAP) that supplies field values and table rows (`RPT.FORMS`).
pub trait FieldSource {
    fn field(&self, id: &FieldId) -> Option<FieldValue>;
    fn rows(&self, table: &FieldId) -> Vec<RowSource>;
}

/// The value of `id` from the first source that has it.
pub fn find_field(sources: &[&dyn FieldSource], id: &FieldId) -> Option<FieldValue> {
    sources.iter().find_map(|s| s.field(id))
}

// ------------------------------------------------------------------ context and view

/// What a resolution runs against: the project, an explicit journal head (never "latest"),
/// the stage and run selection (interpreted by `RPT.MODEL.BIND.EXPAND` and the kinds) and
/// the form data sources.
pub struct ResolveContext<'a> {
    pub project: Uuid,
    pub head: JournalHead,
    pub stages: Vec<Uuid>,
    pub runs: Vec<Uuid>,
    pub sources: Vec<&'a dyn FieldSource>,
}

/// One resolved reference.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedRef {
    /// A ResultRecord: its ID and stored digest (its values are in [`Resolved::values`]).
    Result {
        id: Uuid,
        digest: Digest,
    },
    Characteristic(EntityView),
    NamedView(EntityView),
    Series(EntityView),
    Field {
        id: FieldId,
        value: FieldValue,
    },
}

/// One resolved value field: the payload pointer, the binding, the typed quantity from the
/// stored record and the stored statement it references.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedValue {
    pub pointer: String,
    pub binding: ValueBinding,
    pub quantity: Quantity,
    pub statement: StatementBytes,
}

/// The typed view a [`BlockResolver`] gets: the block, its references in binding order and
/// its value fields in the kind's order, all resolved as of the context's head.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub block: BlockId,
    pub kind: BlockKindId,
    pub refs: Vec<ResolvedRef>,
    pub values: Vec<ResolvedValue>,
}

/// A resolved block: the typed view plus the kind's layout (structure without values).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedBlock {
    view: Resolved,
    layout: CValue,
}

impl ResolvedBlock {
    /// Refuses a layout holding a JSON number anywhere (`report.binding_has_value`): values
    /// travel in [`Resolved::values`] only.
    pub fn new(view: Resolved, layout: CValue) -> Result<ResolvedBlock, BindError> {
        find_number(&layout).map_or(Ok(()), |p| Err(BindError::BindingHasValue(PayloadError::new(&p, "number"))))?;
        Ok(ResolvedBlock { view, layout })
    }

    pub fn view(&self) -> &Resolved {
        &self.view
    }

    pub fn layout(&self) -> &CValue {
        &self.layout
    }
}

/// The pointer of the first JSON number in `v` (depth first, objects in key order, arrays
/// in index order), if any. Iterative; memory is one borrowed entry per visited node, and
/// pointer text is built only for the node reported (cut at 128 bytes by
/// [`PayloadError::new`]).
pub(crate) fn find_number(v: &CValue) -> Option<String> {
    enum Step<'a> {
        Root,
        Key(&'a str),
        Index(usize),
    }
    let mut trail: Vec<(usize, Step<'_>)> = vec![(usize::MAX, Step::Root)];
    let mut stack: Vec<(usize, &CValue)> = vec![(0, v)];
    let mut found = None;
    while let Some((i, v)) = stack.pop() {
        match v {
            CValue::Num(_) | CValue::I64(_) | CValue::U64(_) | CValue::F64Bits(_) => {
                found = Some(i);
                break;
            }
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
    let mut steps = Vec::new();
    let mut at = found?;
    while let Some((parent, step)) = trail.get(at) {
        steps.push(step);
        at = *parent;
    }
    let mut out = String::new();
    for step in steps.iter().rev() {
        if out.len() > 128 {
            break;
        }
        match step {
            Step::Root => {}
            Step::Key(k) => {
                out.push('/');
                out.push_str(&k.replace('~', "~0").replace('/', "~1"));
            }
            Step::Index(j) => {
                out.push('/');
                out.push_str(&j.to_string());
            }
        }
    }
    Some(out)
}

// ------------------------------------------------------------------ resolvers

/// A block kind's binding and resolution logic, registered by the crate that owns the
/// kind's payload (`RPT.MODEL.BLOCKS`, `RPT.FORMS`, `RPT.BALLOON`).
pub trait BlockResolver {
    /// The kind it resolves.
    fn kind(&self) -> BlockKindId;
    /// Places the value bindings `report.bind_block@1` made (one per value field of each
    /// bound record, in reference order) into the kind's payload and returns the new
    /// payload object, without the `binding` key (the command writes it). Must be bounded
    /// and must not panic.
    fn place(
        &self,
        payload: &CanonicalJson,
        binding: &Binding,
        values: &[ValueBinding],
    ) -> Result<CValue, PayloadError>;
    /// Builds the resolved block from the typed view.
    fn resolve(
        &self,
        view: Resolved,
        ctx: &ResolveContext<'_>,
        lookup: &dyn Lookup,
    ) -> Result<ResolvedBlock, BindError>;
}

/// Registered resolvers, keyed by kind ID.
#[derive(Default)]
pub struct ResolverRegistry {
    resolvers: BTreeMap<BlockKindId, Box<dyn BlockResolver>>,
}

impl ResolverRegistry {
    pub fn new() -> ResolverRegistry {
        ResolverRegistry::default()
    }

    /// Registers a resolver; a second one for the same kind, or more than
    /// [`MAX_RESOLVERS`], is `report.bad_params`.
    pub fn register(&mut self, r: Box<dyn BlockResolver>) -> Result<(), BindError> {
        let kind = r.kind();
        if self.resolvers.contains_key(&kind) {
            return Err(BindError::BadParams("duplicate resolver"));
        }
        if self.resolvers.len() >= MAX_RESOLVERS {
            return Err(BindError::BadParams("too many resolvers"));
        }
        self.resolvers.insert(kind, r);
        Ok(())
    }

    /// The resolver of `kind` (`report.no_resolver`).
    pub fn get(&self, kind: &BlockKindId) -> Result<&dyn BlockResolver, BindError> {
        self.resolvers.get(kind).map(|r| r.as_ref()).ok_or_else(|| BindError::NoResolver(kind.clone()))
    }
}

impl fmt::Debug for ResolverRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.resolvers.keys().map(BlockKindId::as_str)).finish()
    }
}

// ------------------------------------------------------------------ resolution

/// Maps a lookup failure of `id` in `block` to its error.
pub(crate) fn fault(block: BlockId, id: &str, f: LookupFault) -> BindError {
    match f {
        LookupFault::NotFound => BindError::NotFound(id.to_string()),
        LookupFault::Unjournaled => BindError::UnjournaledRecord { block, record: id.to_string() },
        LookupFault::CrossProject(project) => BindError::CrossProjectBinding { block, project },
        LookupFault::Invalid(r) => BindError::StoreInvalid(r),
    }
}

/// Resolves one reference as of the lookup's head.
pub(crate) fn resolve_ref(
    block: BlockId,
    r: &Ref,
    lookup: &dyn Lookup,
    sources: &[&dyn FieldSource],
) -> Result<ResolvedRef, BindError> {
    let f = |u: &Uuid, e| fault(block, &u.to_string(), e);
    Ok(match r {
        Ref::Result(u) => {
            let v = lookup.result(u).map_err(|e| f(u, e))?;
            ResolvedRef::Result { id: v.id, digest: v.digest }
        }
        Ref::Characteristic(u) => ResolvedRef::Characteristic(lookup.characteristic(u).map_err(|e| f(u, e))?),
        Ref::NamedView(u) => ResolvedRef::NamedView(lookup.named_view(u).map_err(|e| f(u, e))?),
        Ref::Series(u) => ResolvedRef::Series(lookup.series(u).map_err(|e| f(u, e))?),
        Ref::Field(id) => {
            let value = find_field(sources, id).ok_or_else(|| BindError::NotFound(id.as_str().to_string()))?;
            ResolvedRef::Field { id: id.clone(), value }
        }
    })
}

/// Resolves one value field at `pointer` (step 5 of the module documentation).
fn resolve_value(
    block: BlockId,
    pointer: &str,
    payload: &CValue,
    binding: &Binding,
    lookup: &dyn Lookup,
) -> Result<ResolvedValue, BindError> {
    let v = resolve_pointer(payload, pointer)
        .ok_or_else(|| ReportError::ValueBindingInvalid(PayloadError::new(pointer, "missing")))?;
    let vb = ValueBinding::from_value(v, pointer)?;
    if !binding.refs().contains(&Ref::Result(vb.record)) {
        return Err(ReportError::ValueBindingInvalid(PayloadError::new(pointer, "record_not_bound")).into());
    }
    let mismatch = |digest| BindError::BindingDigestMismatch { block, binding: pointer.to_string(), digest };
    let record = lookup.result(&vb.record).map_err(|e| fault(block, &vb.record.to_string(), e))?;
    if record.digest != vb.record_digest {
        return Err(mismatch("record_digest"));
    }
    let field = record
        .value_field(&vb.field)
        .ok_or_else(|| BindError::BindingFieldNotValue { block, binding: pointer.to_string() })?;
    if field.statement != Some(vb.statement_digest) {
        return Err(mismatch("statement_digest"));
    }
    let statement = lookup.statement(&vb.statement_digest).map_err(|e| match e {
        LookupFault::Invalid(_) => mismatch("statement_digest"),
        other => fault(block, &vb.record.to_string(), other),
    })?;
    Ok(ResolvedValue { pointer: pointer.to_string(), quantity: field.quantity, binding: vb, statement })
}

/// Resolves `block` as of `ctx.head` (see the module documentation for the order of the
/// checks). `kinds` supplies the kind's declared value fields; `journal` the store.
pub fn resolve_block(
    block: &Block,
    ctx: &ResolveContext<'_>,
    kinds: &KindRegistry,
    resolvers: &ResolverRegistry,
    journal: &dyn JournalReads,
) -> Result<ResolvedBlock, BindError> {
    let resolver = resolvers.get(block.kind())?;
    let lookup = HeadLookup::new(journal, ctx.project, &ctx.head)?;
    let payload = block.payload().value();
    let binding = Binding::of_payload(payload)?;
    let id = block.id();
    let refs =
        binding.refs().iter().map(|r| resolve_ref(id, r, &lookup, &ctx.sources)).collect::<Result<Vec<_>, _>>()?;
    let (_, kind) = kinds.get(block.kind()).ok_or_else(|| ReportError::UnknownBlockKind(Some(block.kind().clone())))?;
    let fields = kind.value_fields(block.payload()).map_err(ReportError::InvalidBlockPayload)?;
    let values =
        fields.iter().map(|p| resolve_value(id, p, payload, &binding, &lookup)).collect::<Result<Vec<_>, _>>()?;
    let view = Resolved { block: id, kind: block.kind().clone(), refs, values };
    resolver.resolve(view, ctx, &lookup)
}

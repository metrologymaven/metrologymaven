//! Resolution of a block's binding against an explicit journal head (REQ-002..005, -007).
//!
//! [`resolve_block`] runs these steps and returns the first failure; it reads only, so
//! nothing is ever journaled:
//!
//! 1. the block's kind has a registered [`BlockResolver`] (`report.no_resolver`);
//! 2. the payload holds no value ([`find_value`]: no JSON number, no number text, no bit
//!    pattern outside its ID fields; `report.binding_has_value`);
//! 3. the head is an entry of the journal (`report.not_found`);
//! 4. the stored `binding@1` reads back (`report.store_invalid`);
//! 5. every reference, in binding order, as of the head: a ResultRecord or characteristic
//!    of another project is `report.cross_project_binding` (block and foreign project
//!    only); an entity committed only after the head is `report.unjournaled_record` (block
//!    and ID); an absent one, a field no [`FieldSource`] supplies, or an entity of another
//!    kind is `report.not_found`;
//! 6. every declared value field, in the kind's order: its `value_binding@1` reads back
//!    (DOC's codes), names a record the binding references (`report.value_binding_invalid`,
//!    `record_not_bound`); the SHA-256 of the record stored at the head equals
//!    `record_digest` (`report.binding_digest_mismatch`, `record_digest`); `field` is one
//!    of the record's value fields (`report.binding_field_not_value`); the record references
//!    exactly `statement_digest` for that field and the project's stored statement hashes to
//!    it (`report.binding_digest_mismatch`, `statement_digest`);
//! 7. the kind's resolver returns a layout from the typed [`Resolved`] view; the engine
//!    refuses a layout holding a value ([`find_value`]) and builds the [`ResolvedBlock`].
//!
//! The view is the engine's: [`Resolved`], [`ResolvedValue`] and [`crate::bind::Quantity`]
//! have private fields and no public constructor, and a resolver returns only a layout, so
//! a resolver cannot add, change or swap a value, and cannot bring in one that is not at the
//! head. Values reach the kind only as a [`Quantity`] with its record's [`Evidence`]:
//!
//! ```compile_fail
//! fn forge(view: &fid_report::bind::Resolved) -> fid_report::bind::ResolvedBlock {
//!     fid_report::bind::ResolvedBlock::new(view.clone(), fid_canon::CValue::Null).unwrap()
//! }
//! ```

use super::error::BindError;
use super::journal::{EntityView, Evidence, HeadLookup, JournalReads, Lookup, LookupFault, StatementBytes};
use super::model::{Binding, FieldId, JournalHead, Quantity, Ref, ValueBinding, BINDING_KEY, BINDING_SCHEMA};
use crate::doc::kind::PayloadError;
use crate::doc::value::{resolve_pointer, VALUE_BINDING_SCHEMA};
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

/// One resolved value field: the payload pointer, the binding, the typed quantity and the
/// evidence facts from the record stored at the head, and the stored statement it
/// references.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedValue {
    pointer: String,
    binding: ValueBinding,
    quantity: Quantity,
    evidence: Evidence,
    statement: StatementBytes,
}

impl ResolvedValue {
    /// The value field's RFC 6901 pointer in the block payload.
    pub fn pointer(&self) -> &str {
        &self.pointer
    }
    pub fn binding(&self) -> &ValueBinding {
        &self.binding
    }
    pub fn quantity(&self) -> Quantity {
        self.quantity
    }
    /// Evaluation basis, decision and claim facts of the record: a claimed, declared-input,
    /// UEC-0, `RESULTS_ONLY` or `DIRECT_READING` value never looks measured.
    pub fn evidence(&self) -> &Evidence {
        &self.evidence
    }
    pub fn statement(&self) -> &StatementBytes {
        &self.statement
    }
}

/// The typed view a [`BlockResolver`] gets: the block, its references in binding order and
/// its value fields in the kind's order, all resolved as of the context's head. Built only
/// by [`resolve_block`].
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    block: BlockId,
    kind: BlockKindId,
    refs: Vec<ResolvedRef>,
    values: Vec<ResolvedValue>,
}

impl Resolved {
    pub fn block(&self) -> BlockId {
        self.block
    }
    pub fn kind(&self) -> &BlockKindId {
        &self.kind
    }
    pub fn refs(&self) -> &[ResolvedRef] {
        &self.refs
    }
    pub fn values(&self) -> &[ResolvedValue] {
        &self.values
    }
}

/// A resolved block: the engine's typed view plus the kind's layout (structure without
/// values). Built only by [`resolve_block`].
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedBlock {
    view: Resolved,
    layout: CValue,
}

impl ResolvedBlock {
    /// Refuses a layout holding a value ([`find_value`], `report.binding_has_value`):
    /// values travel in [`Resolved::values`] only.
    pub(crate) fn new(view: Resolved, layout: CValue) -> Result<ResolvedBlock, BindError> {
        refuse_value(&layout)?;
        Ok(ResolvedBlock { view, layout })
    }

    pub fn view(&self) -> &Resolved {
        &self.view
    }

    pub fn layout(&self) -> &CValue {
        &self.layout
    }
}

// ------------------------------------------------------------------ the no-value rule

/// Whether `s` is value text (REQ-001): optional blanks, an optional sign (`+`, `-`,
/// U+2212), a decimal mantissa (`d+`, `d+.`, `d+.d+` or `.d+`, with `.` or `,` as the
/// separator, ASCII digits), an optional exponent (`e`/`E`, optional sign, `d+`), then
/// optionally blanks and a unit (a first character that is an ASCII letter or one of
/// `µ μ ° % ' " ′ ″`, then any of those, ASCII digits, `/ ^ · . ² ³`), then optional
/// blanks. Blanks are U+0020, U+0009, U+00A0 and U+202F.
pub fn is_number_text(s: &str) -> bool {
    let blank = |c: char| matches!(c, ' ' | '\t' | '\u{a0}' | '\u{202f}');
    let unit_start = |c: char| c.is_ascii_alphabetic() || matches!(c, 'µ' | 'μ' | '°' | '%' | '\'' | '"' | '′' | '″');
    let unit_char = |c: char| unit_start(c) || c.is_ascii_digit() || matches!(c, '/' | '^' | '·' | '.' | '²' | '³');
    let c: Vec<char> = s.trim_matches(blank).chars().collect();
    let mut i = 0;
    if matches!(c.first(), Some('+' | '-' | '\u{2212}')) {
        i += 1;
    }
    let digits = |i: &mut usize| {
        let start = *i;
        while c.get(*i).is_some_and(char::is_ascii_digit) {
            *i += 1;
        }
        *i - start
    };
    let int = digits(&mut i);
    let mut frac = 0;
    if matches!(c.get(i), Some('.' | ',')) {
        i += 1;
        frac = digits(&mut i);
    }
    if int == 0 && frac == 0 {
        return false;
    }
    if matches!(c.get(i), Some('e' | 'E')) {
        let mut j = i + 1;
        if matches!(c.get(j), Some('+' | '-' | '\u{2212}')) {
            j += 1;
        }
        if digits(&mut j) > 0 {
            i = j;
        }
    }
    while c.get(i).is_some_and(|x| blank(*x)) {
        i += 1;
    }
    match c.get(i) {
        None => true,
        Some(&x) if unit_start(x) => c.get(i + 1..).is_some_and(|rest| rest.iter().all(|x| unit_char(*x))),
        Some(_) => false,
    }
}

/// Whether `s` is an IEEE-754 bit pattern: `0x` or `0X` and at least one hex digit.
pub fn is_bit_pattern(s: &str) -> bool {
    let h = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"));
    h.is_some_and(|h| !h.is_empty() && h.bytes().all(|c| c.is_ascii_hexdigit()))
}

/// The pointer and reason of the first value in `v` (depth first, objects in key order,
/// arrays in index order): a JSON number (`number`), a bit pattern (`bit_pattern`) or value
/// text (`number_text`) in any string. ID fields are skipped: the payload's top-level
/// `binding` and every object tagged `binding@1` or `value_binding@1`. Iterative; memory
/// is one borrowed entry per visited node, and pointer text is built only for the node
/// reported (cut at 128 bytes).
pub fn find_value(v: &CValue) -> Option<(String, &'static str)> {
    enum Step<'a> {
        Root,
        Key(&'a str),
        Index(usize),
    }
    let mut trail: Vec<(usize, Step<'_>)> = vec![(usize::MAX, Step::Root)];
    let mut stack: Vec<(usize, &CValue)> = vec![(0, v)];
    let mut found = None;
    while let Some((i, v)) = stack.pop() {
        let reason = match v {
            CValue::Num(_) | CValue::I64(_) | CValue::U64(_) | CValue::F64Bits(_) => Some("number"),
            CValue::Str(s) if is_bit_pattern(s) => Some("bit_pattern"),
            CValue::Str(s) if is_number_text(s) => Some("number_text"),
            CValue::Obj(o) => {
                let tagged =
                    matches!(o.get("schema"), Some(CValue::Str(t)) if t == VALUE_BINDING_SCHEMA || t == BINDING_SCHEMA);
                if !tagged {
                    for (k, c) in o.iter().rev() {
                        if i == 0 && k == BINDING_KEY {
                            continue;
                        }
                        stack.push((trail.len(), c));
                        trail.push((i, Step::Key(k)));
                    }
                }
                None
            }
            CValue::Arr(a) => {
                for (j, c) in a.iter().enumerate().rev() {
                    stack.push((trail.len(), c));
                    trail.push((i, Step::Index(j)));
                }
                None
            }
            _ => None,
        };
        if let Some(r) = reason {
            found = Some((i, r));
            break;
        }
    }
    let (mut at, reason) = found?;
    let mut steps = Vec::new();
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
    Some((out, reason))
}

/// [`find_value`] as an error (`report.binding_has_value`).
pub(crate) fn refuse_value(v: &CValue) -> Result<(), BindError> {
    match find_value(v) {
        Some((at, reason)) => Err(BindError::BindingHasValue(PayloadError::new(&at, reason))),
        None => Ok(()),
    }
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
    /// The kind's layout for the engine's typed view: structure only (pointers, IDs, labels
    /// keys), never a value. The engine builds the [`ResolvedBlock`].
    fn resolve(&self, view: &Resolved, ctx: &ResolveContext<'_>, lookup: &dyn Lookup) -> Result<CValue, BindError>;
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
            ResolvedRef::Result { id: v.id(), digest: v.digest() }
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

/// Resolves one value field at `pointer` (step 6 of the module documentation).
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
    if record.digest() != vb.record_digest {
        return Err(mismatch("record_digest"));
    }
    let field = record
        .value_field(&vb.field)
        .ok_or_else(|| BindError::BindingFieldNotValue { block, binding: pointer.to_string() })?;
    if field.statement() != Some(vb.statement_digest) {
        return Err(mismatch("statement_digest"));
    }
    let statement = lookup.statement(&vb.statement_digest).map_err(|e| match e {
        LookupFault::Invalid(_) => mismatch("statement_digest"),
        other => fault(block, &vb.record.to_string(), other),
    })?;
    Ok(ResolvedValue {
        pointer: pointer.to_string(),
        quantity: field.quantity(),
        evidence: record.evidence().clone(),
        binding: vb,
        statement,
    })
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
    let payload = block.payload().value();
    refuse_value(payload)?;
    let lookup = HeadLookup::new(journal, ctx.project, &ctx.head)?;
    let binding = Binding::of_payload(payload)?;
    let id = block.id();
    let refs =
        binding.refs().iter().map(|r| resolve_ref(id, r, &lookup, &ctx.sources)).collect::<Result<Vec<_>, _>>()?;
    let (_, kind) = kinds.get(block.kind()).ok_or_else(|| ReportError::UnknownBlockKind(Some(block.kind().clone())))?;
    let fields = kind.value_fields(block.payload()).map_err(ReportError::InvalidBlockPayload)?;
    let values =
        fields.iter().map(|p| resolve_value(id, p, payload, &binding, &lookup)).collect::<Result<Vec<_>, _>>()?;
    let view = Resolved { block: id, kind: block.kind().clone(), refs, values };
    let layout = resolver.resolve(&view, ctx, &lookup)?;
    ResolvedBlock::new(view, layout)
}

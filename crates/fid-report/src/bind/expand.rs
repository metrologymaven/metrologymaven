//! Slot, repetition and multi-stage expansion at a journal head. Spec node
//! `RPT.MODEL.BIND.EXPAND`.
//!
//! [`expand`] turns a template ([`Template`], its slots checked by
//! [`crate::tmpl::slot::validate_template`]) into an ordered list of [`ExpandedBlock`]s with
//! plain bindings that the resolver (`RPT.MODEL.BIND.RESOLVE`) can look up. It copies no
//! value: an expanded block holds stable-ID references only ([`Binding`]), never a number,
//! text of a value or a label, so whether a value looks measured or carries the provisional
//! label is decided later, from the stored record, by resolution and `RPT.MODEL.FMT`.
//!
//! Pinned rules (each has a test in `crates/fid-report/tests/bind_expand.rs`; expected bytes
//! come from the independent Python reference `fixtures/rpt/model/bind/expand/make_fixtures.py`):
//!
//! - **Project model.** Rules are evaluated against the project as of `ctx.head` through
//!   [`ProjectModel`] (characteristics, elements, stages in project order, runs, results),
//!   implemented by the store adapter of the bus like [`JournalReads`]; [`MemoryModel`] in
//!   memory. Every list holds at most [`MAX_MODEL_ITEMS`] entries with distinct IDs
//!   (`report.store_invalid`). Every selected characteristic is also looked up in the
//!   journal at the head ([`HeadLookup`]: `report.cross_project_binding`,
//!   `report.unjournaled_record`, `report.not_found`), and the outcome filter reads each
//!   result's decision from the journaled record, never from the model.
//! - **Selectors.** `characteristics` all, by ID or by type (byte-equal type token);
//!   `tags` selects characteristics by [`TagSelector::matches`]; `elements`, `stages` (live
//!   stages only) and `runs` all or by ID; a listed ID the model does not hold is
//!   `report.not_found`. `saved_query` needs a [`QueryProvider`] (`report.query_unavailable`
//!   naming the block, REQ-005) whose answer may hold characteristic references only
//!   (`report.query_result_invalid` otherwise). `field_table` takes the rows of the first
//!   source in `ctx.sources` that has any; row `i` has the stable ID `<table>/<i, ten
//!   digits>`.
//! - **Outcome filter.** An item is kept when one of its results in the stage columns (for
//!   a stage, its own results; for a run, its stages' results; for an element, its
//!   characteristics' results) has a decision outcome in the filter. Field rows have none.
//! - **Order (REQ-001, REQ-003).** Items are sorted by [`order`] on the slot's sort key, then
//!   ascending stable ID. Values: characteristic ID, name and element ID as text, balloon
//!   number, stage position and run start time as integers, `Missing` where an item has
//!   none. A `stages` or `runs` slot whose sort is `null` uses the multi-stage order:
//!   stages by project position, then their run's start time, then run ID; runs by their
//!   first live stage's position, then start time, then ID.
//! - **Stage columns (REQ-003).** The stages whose results sit side by side: `ctx.stages`
//!   if given (each a live stage, else `report.not_found`), otherwise every live stage;
//!   then, if `ctx.runs` is given, only stages of those runs; in the multi-stage order.
//! - **Repetition (REQ-002).** Pages in document order. A page named by a `page_group` slot
//!   is emitted, with the other pages of its group in document order, once per item, at the
//!   place of the group's first page (no items: no copies). A page in two groups, or a
//!   `page_group` slot on a page of another group, is `report.slot_invalid`
//!   (`page_group_overlap`, `nested_page_group`): no recursion. On each page, blocks in
//!   order: a block without a slot (or with the `page_group` slot) is one [`Role::Copy`];
//!   a `row` slot is one [`Role::Table`] holding the stage columns, followed by one
//!   [`Role::Row`] per item whose binding is the characteristic and its result in each stage
//!   column (`cells`, aligned with the columns, `None` where a stage has no result); a
//!   `column` slot is one [`Role::ColumnGroup`] per run of at most `max_columns_per_group`
//!   items (one empty group for no items).
//! - **IDs (REQ-004, CONSTITUTION §5).** UUIDv5 with the UTF-8 of the item's stable ID as
//!   name: a page copy is `v5(template page ID, item)`; on a copy, a block's base ID is
//!   `v5(template block ID, page item)`, else the template block ID; a row is `v5(base,
//!   row item)` and a column group `v5(base, its first item)`.
//! - **Bounds (REQ-006).** At most [`MAX_ROWS`] rows, [`MAX_COLUMNS`] columns and
//!   [`MAX_PAGES`] pages per report, counted before any item is looked up or sorted;
//!   crossing one is `report.expansion_too_large` naming the block whose slot crossed it.
//! - **Canonical form.** [`to_canonical_bytes`] is the RFC 8785 form `{"blocks": [...],
//!   "schema": "expansion@1"}`.

use super::error::BindError;
use super::journal::{HeadLookup, JournalReads, Lookup, LookupFault, Outcome as RecordOutcome};
use super::model::{Binding, FieldId, JournalHead, Ref};
use super::resolve::ResolveContext;
use crate::doc::{BlockId, PageId};
use crate::tmpl::format::{Template, TemplateBlock};
use crate::tmpl::slot::{
    self, order, CharacteristicSelector, IdSelector, Item, Outcome, RepetitionRule, SavedQueryId, Selector, Slot,
    SlotError, SortField, SortKey, SortValue, StableId,
};
use fid_canon::CValue;
use fid_project::{uuid_v5, Uuid};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Most rows one report expands to (REQ-006) [unverified limit].
pub const MAX_ROWS: usize = 100_000;
/// Most pages one report expands to (REQ-006) [unverified limit].
pub const MAX_PAGES: usize = 5_000;
/// Most repeated columns one report expands to (pinned at build, as for rows).
pub const MAX_COLUMNS: usize = 100_000;
/// Most entries in one [`ProjectModel`] list.
pub const MAX_MODEL_ITEMS: usize = 1_000_000;
/// Most tags on one characteristic.
pub const MAX_TAGS: usize = 256;
/// Schema word of the canonical expansion.
pub const EXPANSION_SCHEMA: &str = "expansion@1";

// ------------------------------------------------------------------ the project model

/// A characteristic as the project holds it at a head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacteristicItem {
    pub id: Uuid,
    /// Untrusted name, compared by bytes only.
    pub name: String,
    /// QIF characteristic type token (for example `PositionCharacteristic`).
    pub char_type: String,
    pub balloon: Option<i64>,
    pub element: Option<Uuid>,
    /// Untrusted tag names, compared by bytes only.
    pub tags: Vec<String>,
}

/// A stage; its position is its index in [`ProjectModel::stages`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageItem {
    pub id: Uuid,
    pub run: Uuid,
    pub removed: bool,
}

/// A run and its start time (integer milliseconds on one time base).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunItem {
    pub id: Uuid,
    pub start_ms: i64,
}

/// A ResultRecord of one characteristic in one stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultItem {
    pub id: Uuid,
    pub characteristic: Uuid,
    pub stage: Uuid,
}

/// The project model as of a journal head (the store adapter of the bus; [`MemoryModel`]
/// in memory). Unknown heads are `report.not_found`.
pub trait ProjectModel {
    fn characteristics(&self, head: &JournalHead) -> Result<Vec<CharacteristicItem>, BindError>;
    fn elements(&self, head: &JournalHead) -> Result<Vec<Uuid>, BindError>;
    /// Every stage in project order, removed ones included.
    fn stages(&self, head: &JournalHead) -> Result<Vec<StageItem>, BindError>;
    fn runs(&self, head: &JournalHead) -> Result<Vec<RunItem>, BindError>;
    fn results(&self, head: &JournalHead) -> Result<Vec<ResultItem>, BindError>;
}

/// A project model valid at one head, for tests and tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryModel {
    pub head: JournalHead,
    pub characteristics: Vec<CharacteristicItem>,
    pub elements: Vec<Uuid>,
    pub stages: Vec<StageItem>,
    pub runs: Vec<RunItem>,
    pub results: Vec<ResultItem>,
}

impl MemoryModel {
    fn at<T: Clone>(&self, head: &JournalHead, v: &[T]) -> Result<Vec<T>, BindError> {
        if *head == self.head {
            Ok(v.to_vec())
        } else {
            Err(BindError::NotFound(head.digest().as_str().to_string()))
        }
    }
}

impl ProjectModel for MemoryModel {
    fn characteristics(&self, head: &JournalHead) -> Result<Vec<CharacteristicItem>, BindError> {
        self.at(head, &self.characteristics)
    }
    fn elements(&self, head: &JournalHead) -> Result<Vec<Uuid>, BindError> {
        self.at(head, &self.elements)
    }
    fn stages(&self, head: &JournalHead) -> Result<Vec<StageItem>, BindError> {
        self.at(head, &self.stages)
    }
    fn runs(&self, head: &JournalHead) -> Result<Vec<RunItem>, BindError> {
        self.at(head, &self.runs)
    }
    fn results(&self, head: &JournalHead) -> Result<Vec<ResultItem>, BindError> {
        self.at(head, &self.results)
    }
}

/// Runs saved catalog queries (`RPT.CATALOG.QUERY`, P1b).
pub trait QueryProvider {
    fn run(&self, query: SavedQueryId, head: &JournalHead) -> Result<Vec<Ref>, BindError>;
}

// ------------------------------------------------------------------ output

/// What an item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemKind {
    Characteristic,
    Element,
    Stage,
    Run,
    FieldRow,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Characteristic => "characteristic",
            ItemKind::Element => "element",
            ItemKind::Stage => "stage",
            ItemKind::Run => "run",
            ItemKind::FieldRow => "field_row",
        }
    }
}

/// One selected item: its kind and stable ID text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExpandItem {
    pub kind: ItemKind,
    pub id: String,
}

impl ExpandItem {
    fn uuid(kind: ItemKind, id: Uuid) -> ExpandItem {
        ExpandItem { kind, id: id.to_string() }
    }

    pub fn to_value(&self) -> CValue {
        obj([("id", CValue::Str(self.id.clone())), ("kind", CValue::Str(self.kind.as_str().to_string()))])
    }
}

/// The part of the template an expanded block stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A block without a repeating slot, copied.
    Copy,
    /// The table of a `row` slot; its rows follow it.
    Table,
    /// One row of a `row` slot.
    Row,
    /// One column group of a `column` slot.
    ColumnGroup,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Copy => "copy",
            Role::Table => "table",
            Role::Row => "row",
            Role::ColumnGroup => "column_group",
        }
    }
}

/// One expanded block (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandedBlock {
    /// Derived ID (UUIDv5), or the template block ID for a block outside page copies.
    pub id: Uuid,
    pub source_block: BlockId,
    /// The expanded page: the template page ID, or the page copy's derived ID.
    pub page: Uuid,
    pub source_page: PageId,
    /// The item of the page copy, if the page is a copy.
    pub page_item: Option<ExpandItem>,
    pub role: Role,
    /// The row's item ([`Role::Row`]).
    pub item: Option<ExpandItem>,
    /// [`Role::Table`]: the stage columns; [`Role::ColumnGroup`]: the group's items.
    pub columns: Vec<ExpandItem>,
    /// Plain stable-ID references for the resolver.
    pub binding: Binding,
    /// [`Role::Row`] of a characteristic: its result in each stage column.
    pub cells: Vec<Option<Uuid>>,
}

impl ExpandedBlock {
    pub fn to_value(&self) -> CValue {
        let item = |i: &Option<ExpandItem>| i.as_ref().map_or(CValue::Null, ExpandItem::to_value);
        obj([
            ("binding", self.binding.to_value()),
            (
                "cells",
                CValue::Arr(
                    self.cells.iter().map(|c| c.map_or(CValue::Null, |u| CValue::Str(u.to_string()))).collect(),
                ),
            ),
            ("columns", CValue::Arr(self.columns.iter().map(ExpandItem::to_value).collect())),
            ("id", CValue::Str(self.id.to_string())),
            ("item", item(&self.item)),
            ("page", CValue::Str(self.page.to_string())),
            ("page_item", item(&self.page_item)),
            ("role", CValue::Str(self.role.as_str().to_string())),
            ("source_block", CValue::Str(self.source_block.uuid().to_string())),
            ("source_page", CValue::Str(self.source_page.uuid().to_string())),
        ])
    }
}

/// The RFC 8785 bytes of an expansion: `{"blocks": [...], "schema": "expansion@1"}`.
pub fn to_canonical_bytes(blocks: &[ExpandedBlock]) -> Result<Vec<u8>, ExpandError> {
    let v = obj([
        ("blocks", CValue::Arr(blocks.iter().map(ExpandedBlock::to_value).collect())),
        ("schema", CValue::Str(EXPANSION_SCHEMA.to_string())),
    ]);
    fid_canon::to_canonical_bytes(&v).map_err(|_| ExpandError::Bind(BindError::StoreInvalid("canonical form")))
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

// ------------------------------------------------------------------ errors

/// Why expansion failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandError {
    /// A lookup or store error (`report.not_found`, `report.store_invalid`,
    /// `report.cross_project_binding`, `report.unjournaled_record`, ...).
    Bind(BindError),
    /// `report.slot_invalid`: a slot failed validation, or page groups overlap or nest.
    Slot(SlotError),
    /// `report.query_unavailable`: a `saved_query` slot and no provider (REQ-005).
    QueryUnavailable { block: BlockId },
    /// `report.query_result_invalid`: the provider answered with a non-characteristic
    /// reference.
    QueryResultInvalid { block: BlockId },
    /// `report.expansion_too_large`: past [`MAX_ROWS`], [`MAX_COLUMNS`] or [`MAX_PAGES`]
    /// (`limit` is `rows`, `columns` or `pages`; REQ-006).
    ExpansionTooLarge { block: BlockId, limit: &'static str },
}

impl From<BindError> for ExpandError {
    fn from(e: BindError) -> ExpandError {
        ExpandError::Bind(e)
    }
}

impl ExpandError {
    pub fn code(&self) -> &'static str {
        match self {
            ExpandError::Bind(e) => e.code(),
            ExpandError::Slot(e) => e.code(),
            ExpandError::QueryUnavailable { .. } => "report.query_unavailable",
            ExpandError::QueryResultInvalid { .. } => "report.query_result_invalid",
            ExpandError::ExpansionTooLarge { .. } => "report.expansion_too_large",
        }
    }

    /// The block (slot) the error names, if any.
    pub fn block(&self) -> Option<BlockId> {
        match self {
            ExpandError::Bind(_) => None,
            ExpandError::Slot(e) => Some(e.block()),
            ExpandError::QueryUnavailable { block }
            | ExpandError::QueryResultInvalid { block }
            | ExpandError::ExpansionTooLarge { block, .. } => Some(*block),
        }
    }

    /// The `--json` / MCP error object.
    pub fn to_value(&self) -> CValue {
        match self {
            ExpandError::Bind(e) => e.to_value(),
            ExpandError::Slot(e) => e.to_value(),
            ExpandError::QueryUnavailable { block } | ExpandError::QueryResultInvalid { block } => {
                obj([("block", CValue::Str(block.uuid().to_string())), ("code", CValue::Str(self.code().to_string()))])
            }
            ExpandError::ExpansionTooLarge { block, limit } => obj([
                ("block", CValue::Str(block.uuid().to_string())),
                ("code", CValue::Str(self.code().to_string())),
                ("limit", CValue::Str((*limit).to_string())),
            ]),
        }
    }
}

impl fmt::Display for ExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExpandError::Bind(e) => e.fmt(f),
            ExpandError::Slot(e) => e.fmt(f),
            ExpandError::QueryUnavailable { block } | ExpandError::QueryResultInvalid { block } => {
                write!(f, "{}: block {}", self.code(), block.uuid())
            }
            ExpandError::ExpansionTooLarge { block, limit } => {
                write!(f, "{}: block {} ({limit})", self.code(), block.uuid())
            }
        }
    }
}

impl std::error::Error for ExpandError {}

// ------------------------------------------------------------------ the snapshot

/// The model at the head, checked and indexed once.
struct Snapshot {
    chars: BTreeMap<Uuid, CharacteristicItem>,
    elements: BTreeSet<Uuid>,
    /// Live stages: ID -> (project position, run).
    stages: BTreeMap<Uuid, (i64, Uuid)>,
    runs: BTreeMap<Uuid, i64>,
    /// (characteristic, stage) -> result.
    results: BTreeMap<(Uuid, Uuid), Uuid>,
}

fn bounded<T>(v: Vec<T>) -> Result<Vec<T>, BindError> {
    if v.len() > MAX_MODEL_ITEMS {
        return Err(BindError::StoreInvalid("model list too long"));
    }
    Ok(v)
}

fn distinct<K: Ord, V>(pairs: impl Iterator<Item = (K, V)>) -> Result<BTreeMap<K, V>, BindError> {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        if m.insert(k, v).is_some() {
            return Err(BindError::StoreInvalid("repeated model ID"));
        }
    }
    Ok(m)
}

impl Snapshot {
    fn read(model: &dyn ProjectModel, head: &JournalHead) -> Result<Snapshot, BindError> {
        let chars = bounded(model.characteristics(head)?)?;
        if chars.iter().any(|c| c.tags.len() > MAX_TAGS) {
            return Err(BindError::StoreInvalid("too many tags"));
        }
        let chars = distinct(chars.into_iter().map(|c| (c.id, c)))?;
        let elements = distinct(bounded(model.elements(head)?)?.into_iter().map(|e| (e, ())))?;
        let runs = distinct(bounded(model.runs(head)?)?.into_iter().map(|r| (r.id, r.start_ms)))?;
        let all_stages = bounded(model.stages(head)?)?;
        let mut seen = BTreeSet::new();
        let mut stages = BTreeMap::new();
        for (pos, s) in all_stages.iter().enumerate() {
            if !seen.insert(s.id) {
                return Err(BindError::StoreInvalid("repeated model ID"));
            }
            if !s.removed {
                stages.insert(s.id, (pos as i64, s.run));
            }
        }
        let results =
            distinct(bounded(model.results(head)?)?.into_iter().map(|r| ((r.characteristic, r.stage), r.id)))?;
        Ok(Snapshot { chars, elements: elements.into_keys().collect(), stages, runs, results })
    }

    /// The multi-stage order key of a stage: position, run start, run ID.
    fn stage_key(&self, id: &Uuid) -> (i64, Option<i64>, Uuid) {
        let (pos, run) = self.stages.get(id).copied().unwrap_or((i64::MAX, Uuid::MAX));
        (pos, self.runs.get(&run).copied(), run)
    }

    /// The multi-stage order key of a run: first live stage's position, start time.
    fn run_key(&self, id: &Uuid) -> (Option<i64>, Option<i64>) {
        let first = self.stages.values().filter(|(_, r)| r == id).map(|(p, _)| *p).min();
        (first, self.runs.get(id).copied())
    }
}

/// `None` sorts after every value, as `Missing` does in the slot contract.
fn some_first(a: Option<i64>, b: Option<i64>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

// ------------------------------------------------------------------ evaluation

struct Expander<'a> {
    ctx: &'a ResolveContext<'a>,
    snap: Snapshot,
    lookup: HeadLookup<'a>,
    query: Option<&'a dyn QueryProvider>,
    /// The stage columns, in multi-stage order.
    columns: Vec<Uuid>,
    rows: usize,
    cols: usize,
    pages: usize,
}

fn uuid_of(s: &StableId) -> Option<Uuid> {
    Uuid::parse(s.untrusted_str()).ok().filter(|u| u.to_string() == s.untrusted_str())
}

fn not_found(s: &StableId) -> BindError {
    BindError::NotFound(s.untrusted_str().to_string())
}

fn fault(block: BlockId, id: &Uuid, f: LookupFault) -> BindError {
    match f {
        LookupFault::NotFound => BindError::NotFound(id.to_string()),
        LookupFault::Unjournaled => BindError::UnjournaledRecord { block, record: id.to_string() },
        LookupFault::CrossProject(project) => BindError::CrossProjectBinding { block, project },
        LookupFault::Invalid(r) => BindError::StoreInvalid(r),
    }
}

fn outcome_of(o: RecordOutcome) -> Outcome {
    match o {
        RecordOutcome::Conforms => Outcome::Conforms,
        RecordOutcome::Nonconforms => Outcome::Nonconforms,
        RecordOutcome::Indeterminate => Outcome::Indeterminate,
        RecordOutcome::NotDecidable => Outcome::NotDecidable,
    }
}

impl<'a> Expander<'a> {
    /// IDs listed by an `ids` selector, each held by `present` (`report.not_found`).
    fn listed(ids: &IdSelector, present: impl Fn(&Uuid) -> bool, all: Vec<Uuid>) -> Result<Vec<Uuid>, BindError> {
        match ids {
            IdSelector::All => Ok(all),
            IdSelector::Ids(list) => {
                list.as_slice().iter().map(|s| uuid_of(s).filter(|u| present(u)).ok_or_else(|| not_found(s))).collect()
            }
        }
    }

    /// The selected items, unordered, before the bounds are checked.
    fn select(&self, block: BlockId, slot: &Slot) -> Result<Vec<ExpandItem>, ExpandError> {
        let s = &self.snap;
        let chars = |f: &dyn Fn(&CharacteristicItem) -> bool| -> Vec<ExpandItem> {
            s.chars.values().filter(|c| f(c)).map(|c| ExpandItem::uuid(ItemKind::Characteristic, c.id)).collect()
        };
        let wrap = |kind, ids: Vec<Uuid>| ids.into_iter().map(|u| ExpandItem::uuid(kind, u)).collect::<Vec<_>>();
        Ok(match slot.rule().selector() {
            Selector::Characteristics(CharacteristicSelector::All) => chars(&|_| true),
            Selector::Characteristics(CharacteristicSelector::Ids(list)) => {
                let ids = Self::listed(&IdSelector::Ids(list.clone()), |u| s.chars.contains_key(u), Vec::new())?;
                wrap(ItemKind::Characteristic, ids)
            }
            Selector::Characteristics(CharacteristicSelector::Type(t)) => {
                chars(&|c| c.char_type.as_bytes() == t.as_str().as_bytes())
            }
            Selector::Tags(sel) => chars(&|c| sel.matches(&c.tags.iter().map(String::as_str).collect::<Vec<_>>())),
            Selector::Elements(sel) => {
                let ids = Self::listed(sel, |u| s.elements.contains(u), s.elements.iter().copied().collect())?;
                wrap(ItemKind::Element, ids)
            }
            Selector::Stages(sel) => {
                let ids = Self::listed(sel, |u| s.stages.contains_key(u), s.stages.keys().copied().collect())?;
                wrap(ItemKind::Stage, ids)
            }
            Selector::Runs(sel) => {
                let ids = Self::listed(sel, |u| s.runs.contains_key(u), s.runs.keys().copied().collect())?;
                wrap(ItemKind::Run, ids)
            }
            Selector::SavedQuery(q) => {
                let provider = self.query.ok_or(ExpandError::QueryUnavailable { block })?;
                let refs = provider.run(*q, &self.ctx.head)?;
                if refs.len() > MAX_MODEL_ITEMS {
                    return Err(ExpandError::Bind(BindError::StoreInvalid("query answer too long")));
                }
                let mut ids = BTreeSet::new();
                for r in refs {
                    match r {
                        Ref::Characteristic(u) if s.chars.contains_key(&u) => {
                            ids.insert(u);
                        }
                        Ref::Characteristic(u) => return Err(BindError::NotFound(u.to_string()).into()),
                        _ => return Err(ExpandError::QueryResultInvalid { block }),
                    }
                }
                wrap(ItemKind::Characteristic, ids.into_iter().collect())
            }
            Selector::FieldTable(t) => {
                let table = FieldId::new(t.as_str()).ok_or(BindError::BadParams("table id"))?;
                let rows = self.ctx.sources.iter().map(|src| src.rows(&table).len()).find(|n| *n > 0).unwrap_or(0);
                if rows > MAX_MODEL_ITEMS {
                    return Err(ExpandError::Bind(BindError::StoreInvalid("too many field rows")));
                }
                (0..rows)
                    .map(|i| ExpandItem { kind: ItemKind::FieldRow, id: format!("{}/{i:010}", t.as_str()) })
                    .collect()
            }
        })
    }

    /// Results of `item` that the outcome filter reads.
    fn results_of(&self, item: &ExpandItem) -> Vec<Uuid> {
        let s = &self.snap;
        let Ok(u) = Uuid::parse(&item.id) else { return Vec::new() };
        let in_columns = |c: &Uuid| -> Vec<Uuid> {
            self.columns.iter().filter_map(|st| s.results.get(&(*c, *st)).copied()).collect()
        };
        match item.kind {
            ItemKind::Characteristic => in_columns(&u),
            ItemKind::Element => {
                s.chars.values().filter(|c| c.element == Some(u)).flat_map(|c| in_columns(&c.id)).collect()
            }
            ItemKind::Stage => s.results.iter().filter(|((_, st), _)| *st == u).map(|(_, r)| *r).collect(),
            ItemKind::Run => s
                .results
                .iter()
                .filter(|((_, st), _)| s.stages.get(st).is_some_and(|(_, run)| *run == u))
                .map(|(_, r)| *r)
                .collect(),
            ItemKind::FieldRow => Vec::new(),
        }
    }

    /// Applies the outcome filter, reading each decision from the journaled record.
    fn filter(&self, block: BlockId, slot: &Slot, items: Vec<ExpandItem>) -> Result<Vec<ExpandItem>, ExpandError> {
        let Some(wanted) = slot.rule().outcome() else { return Ok(items) };
        let mut kept = Vec::with_capacity(items.len());
        for item in items {
            let mut keep = false;
            for r in self.results_of(&item) {
                let view = self.lookup.result(&r).map_err(|f| fault(block, &r, f))?;
                if wanted.contains(&outcome_of(view.evidence().outcome())) {
                    keep = true;
                    break;
                }
            }
            if keep {
                kept.push(item);
            }
        }
        Ok(kept)
    }

    /// The sort value of `item` for `field`.
    fn value(&self, item: &ExpandItem, field: SortField) -> SortValue {
        let s = &self.snap;
        let u = Uuid::parse(&item.id).ok();
        let ch = u.and_then(|u| s.chars.get(&u)).filter(|_| item.kind == ItemKind::Characteristic);
        match (field, item.kind) {
            (SortField::CharacteristicId, ItemKind::Characteristic) => SortValue::Text(item.id.clone()),
            (SortField::CharacteristicName, _) => ch.map_or(SortValue::Missing, |c| SortValue::Text(c.name.clone())),
            (SortField::BalloonNumber, _) => ch.and_then(|c| c.balloon).map_or(SortValue::Missing, SortValue::Int),
            (SortField::ElementId, ItemKind::Element) => SortValue::Text(item.id.clone()),
            (SortField::ElementId, _) => {
                ch.and_then(|c| c.element).map_or(SortValue::Missing, |e| SortValue::Text(e.to_string()))
            }
            (SortField::StageOrder, ItemKind::Stage) => {
                u.and_then(|u| s.stages.get(&u)).map_or(SortValue::Missing, |(p, _)| SortValue::Int(*p))
            }
            (SortField::RunStartTime, ItemKind::Run) => {
                u.and_then(|u| s.runs.get(&u)).map_or(SortValue::Missing, |t| SortValue::Int(*t))
            }
            (SortField::RunStartTime, ItemKind::Stage) => u
                .and_then(|u| s.stages.get(&u))
                .and_then(|(_, run)| s.runs.get(run))
                .map_or(SortValue::Missing, |t| SortValue::Int(*t)),
            _ => SortValue::Missing,
        }
    }

    /// The items in slot order (REQ-001, REQ-003).
    fn sort(&self, slot: &Slot, items: Vec<ExpandItem>) -> Result<Vec<ExpandItem>, ExpandError> {
        let multi = slot.sort_as_written().is_none()
            && matches!(slot.rule().selector(), Selector::Stages(_) | Selector::Runs(_));
        if multi {
            let mut keyed: Vec<(ExpandItem, Uuid)> =
                items.into_iter().filter_map(|i| Uuid::parse(&i.id).ok().map(|u| (i, u))).collect();
            let s = &self.snap;
            keyed.sort_by(|(a, ua), (b, ub)| {
                let by_key = if a.kind == ItemKind::Stage {
                    let (ka, kb) = (s.stage_key(ua), s.stage_key(ub));
                    ka.0.cmp(&kb.0).then(some_first(ka.1, kb.1)).then(ka.2.cmp(&kb.2))
                } else {
                    let (ka, kb) = (s.run_key(ua), s.run_key(ub));
                    some_first(ka.0, kb.0).then(some_first(ka.1, kb.1))
                };
                by_key.then_with(|| a.id.as_bytes().cmp(b.id.as_bytes()))
            });
            return Ok(keyed.into_iter().map(|(i, _)| i).collect());
        }
        let key: SortKey = slot.sort();
        let mut by_id: BTreeMap<String, ExpandItem> = BTreeMap::new();
        let mut keyed = Vec::with_capacity(items.len());
        for it in items {
            let id = StableId::new(&it.id).ok_or(BindError::StoreInvalid("item id"))?;
            keyed.push(Item { id, value: self.value(&it, key.field) });
            by_id.insert(it.id.clone(), it);
        }
        Ok(order(keyed, key).into_iter().filter_map(|i| by_id.remove(i.id.untrusted_str())).collect())
    }

    /// Selected, bounded, verified, filtered and ordered items of a slot. `limit` names the
    /// bound the items count towards; `per_item` is how many units each item adds.
    fn items(
        &mut self,
        block: BlockId,
        slot: &Slot,
        limit: &'static str,
        per_item: usize,
    ) -> Result<Vec<ExpandItem>, ExpandError> {
        let items = self.select(block, slot)?;
        let add = items.len().saturating_mul(per_item);
        let (used, max) = match limit {
            "rows" => (&mut self.rows, MAX_ROWS),
            "columns" => (&mut self.cols, MAX_COLUMNS),
            _ => (&mut self.pages, MAX_PAGES),
        };
        *used = used.saturating_add(add);
        if *used > max {
            return Err(ExpandError::ExpansionTooLarge { block, limit });
        }
        for it in &items {
            if it.kind == ItemKind::Characteristic {
                if let Ok(u) = Uuid::parse(&it.id) {
                    self.lookup.characteristic(&u).map_err(|f| fault(block, &u, f))?;
                }
            }
        }
        let items = self.filter(block, slot, items)?;
        self.sort(slot, items)
    }

    /// The stage columns (REQ-003).
    fn stage_columns(snap: &Snapshot, ctx: &ResolveContext<'_>) -> Result<Vec<Uuid>, BindError> {
        let mut cols: Vec<Uuid> = if ctx.stages.is_empty() {
            snap.stages.keys().copied().collect()
        } else {
            let mut v = Vec::with_capacity(ctx.stages.len());
            for st in &ctx.stages {
                if !snap.stages.contains_key(st) {
                    return Err(BindError::NotFound(st.to_string()));
                }
                if !v.contains(st) {
                    v.push(*st);
                }
            }
            v
        };
        if !ctx.runs.is_empty() {
            if let Some(r) = ctx.runs.iter().find(|r| !snap.runs.contains_key(r)) {
                return Err(BindError::NotFound(r.to_string()));
            }
            cols.retain(|st| snap.stages.get(st).is_some_and(|(_, run)| ctx.runs.contains(run)));
        }
        cols.sort_by(|a, b| {
            let (ka, kb) = (snap.stage_key(a), snap.stage_key(b));
            ka.0.cmp(&kb.0).then(some_first(ka.1, kb.1)).then(ka.2.cmp(&kb.2)).then(a.cmp(b))
        });
        Ok(cols)
    }

    fn empty() -> Result<Binding, ExpandError> {
        Ok(Binding::new(Vec::new())?)
    }

    /// The blocks of one (possibly copied) page.
    fn page(
        &mut self,
        out: &mut Vec<ExpandedBlock>,
        slots: &BTreeMap<BlockId, Slot>,
        blocks: &[TemplateBlock],
        page: (Uuid, PageId, Option<&ExpandItem>),
    ) -> Result<(), ExpandError> {
        let (page_id, source_page, page_item) = page;
        for b in blocks {
            let base = page_item.map_or(b.id().uuid(), |i| uuid_v5(&b.id().uuid(), i.id.as_bytes()));
            let block = |role, id, item, columns, binding, cells| ExpandedBlock {
                id,
                source_block: b.id(),
                page: page_id,
                source_page,
                page_item: page_item.cloned(),
                role,
                item,
                columns,
                binding,
                cells,
            };
            match slots.get(&b.id()).map(|s| (s, s.repeat())) {
                Some((slot, RepetitionRule::Row)) => {
                    let items = self.items(b.id(), slot, "rows", 1)?;
                    let cols: Vec<ExpandItem> =
                        self.columns.iter().map(|u| ExpandItem::uuid(ItemKind::Stage, *u)).collect();
                    out.push(block(Role::Table, base, None, cols, Self::empty()?, Vec::new()));
                    for it in items {
                        let (binding, cells) = self.row_binding(&it)?;
                        let id = uuid_v5(&base, it.id.as_bytes());
                        out.push(block(Role::Row, id, Some(it), Vec::new(), binding, cells));
                    }
                }
                Some((slot, RepetitionRule::Column(rule))) => {
                    let items = self.items(b.id(), slot, "columns", 1)?;
                    let per = usize::from(rule.max_columns_per_group()).max(1);
                    if items.is_empty() {
                        out.push(block(Role::ColumnGroup, base, None, Vec::new(), Self::empty()?, Vec::new()));
                    }
                    for group in items.chunks(per) {
                        let first = group.first().map_or(&[][..], |i| i.id.as_bytes());
                        let refs: Vec<Ref> = group.iter().filter_map(char_ref).collect();
                        let id = uuid_v5(&base, first);
                        out.push(block(Role::ColumnGroup, id, None, group.to_vec(), Binding::new(refs)?, Vec::new()));
                    }
                }
                _ => out.push(block(Role::Copy, base, None, Vec::new(), Self::empty()?, Vec::new())),
            }
        }
        Ok(())
    }

    /// A row's binding: the characteristic, then its result in each stage column.
    fn row_binding(&self, item: &ExpandItem) -> Result<(Binding, Vec<Option<Uuid>>), ExpandError> {
        let Some(Ref::Characteristic(c)) = char_ref(item) else {
            return Ok((Self::empty()?, Vec::new()));
        };
        let cells: Vec<Option<Uuid>> =
            self.columns.iter().map(|st| self.snap.results.get(&(c, *st)).copied()).collect();
        let mut refs = vec![Ref::Characteristic(c)];
        refs.extend(cells.iter().flatten().map(|r| Ref::Result(*r)));
        Ok((Binding::new(refs)?, cells))
    }
}

fn char_ref(item: &ExpandItem) -> Option<Ref> {
    if item.kind == ItemKind::Characteristic {
        Uuid::parse(&item.id).ok().map(Ref::Characteristic)
    } else {
        None
    }
}

/// Expands `template` at `ctx.head` (see the module docs). Reads only; journals nothing.
pub fn expand(
    template: &Template,
    ctx: &ResolveContext<'_>,
    model: &dyn ProjectModel,
    journal: &dyn JournalReads,
    query: Option<&dyn QueryProvider>,
) -> Result<Vec<ExpandedBlock>, ExpandError> {
    let slots: BTreeMap<BlockId, Slot> =
        slot::validate_template(template).map_err(ExpandError::Slot)?.into_iter().collect();
    let lookup = HeadLookup::new(journal, ctx.project, &ctx.head)?;
    let snap = Snapshot::read(model, &ctx.head)?;
    let columns = Expander::stage_columns(&snap, ctx)?;
    let mut ex = Expander { ctx, snap, lookup, query, columns, rows: 0, cols: 0, pages: 0 };

    // Page groups: page -> its group's slot block; no page in two groups, no group slot on
    // a page of another group.
    let mut group_of: BTreeMap<PageId, BlockId> = BTreeMap::new();
    for (block, slot) in &slots {
        if let RepetitionRule::PageGroup(rule) = slot.repeat() {
            for p in rule.pages() {
                if group_of.insert(*p, *block).is_some() {
                    return Err(ExpandError::Slot(SlotError::new(
                        *block,
                        "/spec/value/repeat/pages",
                        "page_group_overlap",
                    )));
                }
            }
        }
    }
    for page in template.pages() {
        for b in page.blocks() {
            let is_group = matches!(slots.get(&b.id()).map(Slot::repeat), Some(RepetitionRule::PageGroup(_)));
            if is_group && group_of.get(&page.id()).is_some_and(|g| *g != b.id()) {
                return Err(ExpandError::Slot(SlotError::new(b.id(), "/spec/value/repeat", "nested_page_group")));
            }
        }
    }

    // Pages outside every group count once each, before any group is evaluated.
    ex.pages = template.pages().iter().filter(|p| !group_of.contains_key(&p.id())).count();
    let mut out = Vec::new();
    let mut done: BTreeSet<BlockId> = BTreeSet::new();
    for page in template.pages() {
        let Some(group) = group_of.get(&page.id()).copied() else {
            ex.page(&mut out, &slots, page.blocks(), (page.id().uuid(), page.id(), None))?;
            continue;
        };
        if !done.insert(group) {
            continue;
        }
        let Some(slot) = slots.get(&group) else { continue };
        let members: Vec<_> = template.pages().iter().filter(|p| group_of.get(&p.id()) == Some(&group)).collect();
        let items = ex.items(group, slot, "pages", members.len())?;
        for it in &items {
            for p in &members {
                let id = uuid_v5(&p.id().uuid(), it.id.as_bytes());
                ex.page(&mut out, &slots, p.blocks(), (id, p.id(), Some(it)))?;
            }
        }
    }
    Ok(out)
}

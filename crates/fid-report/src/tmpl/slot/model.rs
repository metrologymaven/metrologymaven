//! The slot types and their JSON form (the `value` of a slot's `spec` wrapper).
//!
//! Every identifier and tag name here is content from the template file (CONSTITUTION C6):
//! it is stored as given, compared byte for byte, and never parsed, normalised, case-folded
//! or interpreted. There is no type for an expression; rules are closed enums and ID lists.

use crate::doc::{is_v7, PageId};
use crate::tmpl::format::{self, SlotKind, TemplateError, TemplateJson};
use fid_canon::CValue;
use fid_project::Uuid;
use std::collections::BTreeMap;

/// Most entries of an ID list, a tag list or a page list.
pub const MAX_LISTED_IDS: usize = 10_000;
/// Longest stable ID or tag name, in bytes of UTF-8.
pub const MAX_ID_BYTES: usize = 256;
/// Largest `max_columns_per_group`.
pub const MAX_COLUMNS_PER_GROUP: u8 = 64;
/// Most entries of `repeat_columns`.
pub const MAX_REPEAT_COLUMNS: usize = 64;
/// Longest field-table ID, column key or characteristic type name, in bytes.
pub const MAX_TOKEN_BYTES: usize = 64;
/// The block kind a `row` repetition needs.
pub const TABLE_KIND: &str = "table";

/// Why an untrusted ID or tag name is refused: `empty_id`, `id_too_long` or `id_nul`.
pub(crate) fn id_fault(s: &str) -> Option<&'static str> {
    if s.is_empty() {
        Some("empty_id")
    } else if s.len() > MAX_ID_BYTES {
        Some("id_too_long")
    } else if s.contains('\0') {
        Some("id_nul")
    } else {
        None
    }
}

macro_rules! untrusted_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        ///
        /// Untrusted content: 1 to [`MAX_ID_BYTES`] bytes of UTF-8, no NUL, otherwise any
        /// characters (bidi controls included), kept exactly as given. `Ord` is byte order of
        /// the UTF-8, which equals code-point order: no locale, no normalisation.
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Checks the bounds; `None` if empty, too long or holding NUL.
            pub fn new(s: &str) -> Option<$name> {
                id_fault(s).is_none().then(|| $name(s.to_string()))
            }

            /// The raw string. Callers must keep treating it as data: compare it, never
            /// interpret it.
            pub fn untrusted_str(&self) -> &str {
                &self.0
            }

            /// Byte-for-byte equality with `other`, the only matching there is.
            pub fn same_bytes(&self, other: &str) -> bool {
                self.0.as_bytes() == other.as_bytes()
            }
        }
    };
}

untrusted_id!(
    /// A stable record ID as a template names it (CONSTITUTION §5: imported IDs are kept as
    /// imported, so the form is open).
    StableId
);
untrusted_id!(
    /// A tag name.
    TagName
);

/// `^[a-z][a-z0-9_]{0,63}$`.
fn snake_token(s: &str) -> bool {
    let b = s.as_bytes();
    matches!(b.first(), Some(c) if c.is_ascii_lowercase())
        && b.len() <= MAX_TOKEN_BYTES
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

macro_rules! token_id {
    ($(#[$m:meta])* $name:ident, $check:expr) => {
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Checks the pattern; `None` if it does not match.
            pub fn new(s: &str) -> Option<$name> {
                let check: fn(&str) -> bool = $check;
                check(s).then(|| $name(s.to_string()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

token_id!(
    /// The ID of a table of a form data source (`FieldSource::rows`):
    /// `^[a-z][a-z0-9_]{0,63}$`.
    FieldTableId,
    snake_token
);
token_id!(
    /// A column key of the slot's table block: `^[a-z][a-z0-9_]{0,63}$`.
    ColumnKey,
    snake_token
);
token_id!(
    /// A QIF 3.0 characteristic type name in token form, `^[A-Z][A-Za-z0-9]{0,63}$` (for
    /// example `PositionCharacteristic`). Only the shape is checked here; the list of types
    /// belongs to `GDT.CHAR.MODEL.TYPES` and is applied at expansion.
    CharacteristicType,
    |s: &str| {
        let b = s.as_bytes();
        matches!(b.first(), Some(c) if c.is_ascii_uppercase())
            && b.len() <= MAX_TOKEN_BYTES
            && b.iter().all(|c| c.is_ascii_alphanumeric())
    }
);

/// A saved catalog query ID (UUIDv7, `RPT.CATALOG.QUERY.SAVE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SavedQueryId(Uuid);

impl SavedQueryId {
    /// Wraps a UUIDv7; any other UUID is refused.
    pub fn new(id: Uuid) -> Option<SavedQueryId> {
        is_v7(&id).then_some(SavedQueryId(id))
    }

    pub fn uuid(&self) -> Uuid {
        self.0
    }
}

/// A list of 1 to [`MAX_LISTED_IDS`] distinct entries in ascending byte order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortedList<T>(Vec<T>);

/// Why a checked constructor refused its input. The reason words are the ones
/// [`super::validate`] gives for the same fault in a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildError {
    reason: &'static str,
}

impl BuildError {
    pub(crate) fn new(reason: &'static str) -> BuildError {
        BuildError { reason }
    }

    /// The fixed reason word (`empty_list`, `too_many_ids`, `not_sorted`, `duplicate`,
    /// `out_of_range`, `too_many_columns`, `value_too_large`).
    pub fn reason(&self) -> &'static str {
        self.reason
    }
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "report.slot_invalid: {}", self.reason)
    }
}

impl std::error::Error for BuildError {}

/// `items` hold 1 to `max` entries in strictly ascending order, or the reason they do not
/// (`too_many` past `max`).
fn check_ascending<T: Ord>(items: &[T], max: usize, too_many: &'static str) -> Result<(), BuildError> {
    if items.is_empty() {
        return Err(BuildError::new("empty_list"));
    }
    if items.len() > max {
        return Err(BuildError::new(too_many));
    }
    for w in items.windows(2) {
        match w[0].cmp(&w[1]) {
            std::cmp::Ordering::Less => {}
            std::cmp::Ordering::Equal => return Err(BuildError::new("duplicate")),
            std::cmp::Ordering::Greater => return Err(BuildError::new("not_sorted")),
        }
    }
    Ok(())
}

impl<T: Ord> SortedList<T> {
    /// Checks and wraps `items`, which must already be distinct and in ascending byte
    /// order (as the template must spell them): `empty_list`, `too_many_ids`, `duplicate`
    /// or `not_sorted` otherwise.
    pub fn new(items: Vec<T>) -> Result<SortedList<T>, BuildError> {
        check_ascending(&items, MAX_LISTED_IDS, "too_many_ids")?;
        Ok(SortedList(items))
    }

    /// Wraps a list already checked by the reader.
    pub(crate) fn checked(items: Vec<T>) -> SortedList<T> {
        SortedList(items)
    }

    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Never true: a list holds at least one entry.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether the list holds `item` (binary search, byte comparison).
    pub fn contains(&self, item: &T) -> bool {
        self.0.binary_search(item).is_ok()
    }
}

/// A sorted list of stable IDs.
pub type IdList = SortedList<StableId>;
/// A sorted list of tag names.
pub type TagList = SortedList<TagName>;

/// Characteristic selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharacteristicSelector {
    All,
    Ids(IdList),
    Type(CharacteristicType),
}

/// Selector of elements, stages or runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdSelector {
    All,
    Ids(IdList),
}

/// Tag selector. Matching compares tag names byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagSelector {
    /// Items holding at least one of the tags.
    AnyOf(TagList),
    /// Items holding every one of the tags.
    AllOf(TagList),
}

impl TagSelector {
    /// Whether an item with `item_tags` is selected. Byte comparison only: no
    /// normalisation, no case folding, controls and bidi characters compared like any other.
    pub fn matches(&self, item_tags: &[&str]) -> bool {
        let has = |t: &TagName| item_tags.iter().any(|i| t.same_bytes(i));
        match self {
            TagSelector::AnyOf(l) => l.as_slice().iter().any(has),
            TagSelector::AllOf(l) => l.as_slice().iter().all(has),
        }
    }
}

/// What a binding rule selects (REQ-RPT.MODEL.TMPL.SLOT-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    Characteristics(CharacteristicSelector),
    Elements(IdSelector),
    Tags(TagSelector),
    Stages(IdSelector),
    Runs(IdSelector),
    SavedQuery(SavedQueryId),
    FieldTable(FieldTableId),
}

impl Selector {
    /// The slot kind this selector belongs to.
    pub fn kind(&self) -> SlotKind {
        match self {
            Selector::Characteristics(_) => SlotKind::Characteristics,
            Selector::Elements(_) => SlotKind::Elements,
            Selector::Tags(_) => SlotKind::Tags,
            Selector::Stages(_) => SlotKind::Stages,
            Selector::Runs(_) => SlotKind::Runs,
            Selector::SavedQuery(_) => SlotKind::SavedQuery,
            Selector::FieldTable(_) => SlotKind::FieldTable,
        }
    }
}

/// A decision outcome (CONSTITUTION §5), for the outcome filter. Declared in code-point
/// order of the JSON words, so `Ord` matches the stored order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Outcome {
    Conforms,
    Indeterminate,
    Nonconforms,
    NotDecidable,
}

impl Outcome {
    pub const ALL: [Outcome; 4] =
        [Outcome::Conforms, Outcome::Indeterminate, Outcome::Nonconforms, Outcome::NotDecidable];

    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Conforms => "conforms",
            Outcome::Indeterminate => "indeterminate",
            Outcome::Nonconforms => "nonconforms",
            Outcome::NotDecidable => "not_decidable",
        }
    }

    pub fn parse(s: &str) -> Option<Outcome> {
        Outcome::ALL.into_iter().find(|o| o.as_str() == s)
    }
}

/// A binding rule: a selector, optionally intersected with an outcome filter (the items
/// whose decision outcome is one of the listed ones).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingRule {
    pub(crate) selector: Selector,
    pub(crate) outcome: Option<Vec<Outcome>>,
}

impl BindingRule {
    /// A rule. The outcome list, if any, must be non-empty, distinct and ascending
    /// (`empty_list`, `duplicate`, `not_sorted`), as in a template.
    pub fn new(selector: Selector, outcome: Option<Vec<Outcome>>) -> Result<BindingRule, BuildError> {
        if let Some(o) = &outcome {
            check_ascending(o, usize::MAX, "duplicate")?;
        }
        Ok(BindingRule { selector, outcome })
    }

    pub fn selector(&self) -> &Selector {
        &self.selector
    }

    /// The outcome filter, ascending and distinct.
    pub fn outcome(&self) -> Option<&[Outcome]> {
        self.outcome.as_deref()
    }
}

/// The value a slot sorts by (REQ-RPT.MODEL.TMPL.SLOT-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SortField {
    CharacteristicId,
    CharacteristicName,
    BalloonNumber,
    ElementId,
    StageOrder,
    RunStartTime,
}

impl SortField {
    pub const ALL: [SortField; 6] = [
        SortField::CharacteristicId,
        SortField::CharacteristicName,
        SortField::BalloonNumber,
        SortField::ElementId,
        SortField::StageOrder,
        SortField::RunStartTime,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SortField::CharacteristicId => "characteristic_id",
            SortField::CharacteristicName => "characteristic_name",
            SortField::BalloonNumber => "balloon_number",
            SortField::ElementId => "element_id",
            SortField::StageOrder => "stage_order",
            SortField::RunStartTime => "run_start_time",
        }
    }

    pub fn parse(s: &str) -> Option<SortField> {
        SortField::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    Asc,
    Desc,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Asc => "asc",
            Direction::Desc => "desc",
        }
    }

    pub fn parse(s: &str) -> Option<Direction> {
        match s {
            "asc" => Some(Direction::Asc),
            "desc" => Some(Direction::Desc),
            _ => None,
        }
    }
}

/// A sort key: a field and a direction. The default is ascending characteristic ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SortKey {
    pub field: SortField,
    pub direction: Direction,
}

impl Default for SortKey {
    fn default() -> SortKey {
        SortKey { field: SortField::CharacteristicId, direction: Direction::Asc }
    }
}

/// How a slot repeats per selected item (REQ-RPT.MODEL.TMPL.SLOT-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepetitionRule {
    /// One table row per item; only on a `table` block.
    Row,
    /// One column (group) per item.
    Column(ColumnRule),
    /// One copy of a page group per item.
    PageGroup(PageGroupRule),
}

/// Column repetition: at most `max_columns_per_group` (1 to 64) item columns per group,
/// each group repeating `repeat_columns` (in the listed order) first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRule {
    pub(crate) max_columns_per_group: u8,
    pub(crate) repeat_columns: Option<Vec<ColumnKey>>,
}

impl ColumnRule {
    /// Checks the bounds: `out_of_range` unless 1 to 64 columns per group;
    /// `repeat_columns` 1 to 64 distinct keys (`empty_list`, `too_many_columns`,
    /// `duplicate`), order kept.
    pub fn new(max_columns_per_group: u8, repeat_columns: Option<Vec<ColumnKey>>) -> Result<ColumnRule, BuildError> {
        if !(1..=MAX_COLUMNS_PER_GROUP).contains(&max_columns_per_group) {
            return Err(BuildError::new("out_of_range"));
        }
        if let Some(c) = &repeat_columns {
            if c.is_empty() {
                return Err(BuildError::new("empty_list"));
            }
            if c.len() > MAX_REPEAT_COLUMNS {
                return Err(BuildError::new("too_many_columns"));
            }
            let mut seen = std::collections::BTreeSet::new();
            if !c.iter().all(|k| seen.insert(k)) {
                return Err(BuildError::new("duplicate"));
            }
        }
        Ok(ColumnRule { max_columns_per_group, repeat_columns })
    }

    pub fn max_columns_per_group(&self) -> u8 {
        self.max_columns_per_group
    }

    pub fn repeat_columns(&self) -> Option<&[ColumnKey]> {
        self.repeat_columns.as_deref()
    }
}

/// Page-group repetition: the pages to copy, in ascending ID order. Copies follow the
/// pages' document order at expansion. Whether each page exists in the template is checked
/// by [`super::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageGroupRule {
    pub(crate) pages: Vec<PageId>,
}

impl PageGroupRule {
    /// Checks the list: 1 to 10 000 distinct IDs in ascending order (`empty_list`,
    /// `too_many_ids`, `duplicate`, `not_sorted`).
    pub fn new(pages: Vec<PageId>) -> Result<PageGroupRule, BuildError> {
        check_ascending(&pages, MAX_LISTED_IDS, "too_many_ids")?;
        Ok(PageGroupRule { pages })
    }

    pub fn pages(&self) -> &[PageId] {
        &self.pages
    }
}

/// A validated slot: binding rule, sort key (`None` when the template leaves the default)
/// and repetition rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub(crate) rule: BindingRule,
    pub(crate) sort: Option<SortKey>,
    pub(crate) repeat: RepetitionRule,
}

impl Slot {
    /// A slot from checked parts. Refuses a slot whose spec value exceeds the format's
    /// payload bound (`value_too_large`), so [`Slot::to_format`] always succeeds. Rules that
    /// need the template (pages present, `row` only on a table block) are checked by
    /// [`super::validate`].
    pub fn new(rule: BindingRule, sort: Option<SortKey>, repeat: RepetitionRule) -> Result<Slot, BuildError> {
        let slot = Slot { rule, sort, repeat };
        TemplateJson::new(&slot.to_value(), "/spec/value").map_err(|_| BuildError::new("value_too_large"))?;
        Ok(slot)
    }

    /// The slot kind (the envelope's `kind`).
    pub fn kind(&self) -> SlotKind {
        self.rule.selector.kind()
    }

    pub fn rule(&self) -> &BindingRule {
        &self.rule
    }

    /// The sort key in effect: the stored one, or the default (ascending characteristic ID).
    pub fn sort(&self) -> SortKey {
        self.sort.unwrap_or_default()
    }

    /// The sort key as the template spells it (`None` for `"sort": null`).
    pub fn sort_as_written(&self) -> Option<SortKey> {
        self.sort
    }

    pub fn repeat(&self) -> &RepetitionRule {
        &self.repeat
    }

    /// The spec value: `{"repeat", "rule", "sort"}`.
    pub fn to_value(&self) -> CValue {
        obj(vec![
            ("repeat", repeat_value(&self.repeat)),
            ("rule", rule_value(&self.rule)),
            ("sort", sort_value(self.sort)),
        ])
    }

    /// The slot as `RPT.MODEL.TMPL.FORMAT` carries it. Every constructor keeps the spec
    /// value within the format's payload bound, so this does not fail in practice; it stays
    /// a `Result` because the format's checked constructor is fallible.
    pub fn to_format(&self) -> Result<format::Slot, TemplateError> {
        let spec = TemplateJson::new(&self.to_value(), "/spec/value")?;
        Ok(format::Slot { kind: self.kind(), spec })
    }

    /// The whole slot member as `RPT.MODEL.TMPL.FORMAT` stores it:
    /// `{"kind", "spec": {"untrusted": true, "value": <spec value>}}`.
    pub fn to_envelope_value(&self) -> CValue {
        obj(vec![
            ("kind", s(self.kind().as_str())),
            ("spec", obj(vec![("untrusted", CValue::Bool(true)), ("value", self.to_value())])),
        ])
    }
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn obj(pairs: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>())
}

fn strs<'a>(items: impl Iterator<Item = &'a str>) -> CValue {
    CValue::Arr(items.map(s).collect())
}

fn ids_value(l: &IdList) -> CValue {
    obj(vec![("ids", strs(l.as_slice().iter().map(StableId::untrusted_str)))])
}

fn id_selector_value(sel: &IdSelector) -> CValue {
    match sel {
        IdSelector::All => s("all"),
        IdSelector::Ids(l) => ids_value(l),
    }
}

fn rule_value(r: &BindingRule) -> CValue {
    let sel = match &r.selector {
        Selector::Characteristics(CharacteristicSelector::All) => s("all"),
        Selector::Characteristics(CharacteristicSelector::Ids(l)) => ids_value(l),
        Selector::Characteristics(CharacteristicSelector::Type(t)) => obj(vec![("type", s(t.as_str()))]),
        Selector::Elements(x) | Selector::Stages(x) | Selector::Runs(x) => id_selector_value(x),
        Selector::Tags(TagSelector::AnyOf(l)) => {
            obj(vec![("any_of", strs(l.as_slice().iter().map(TagName::untrusted_str)))])
        }
        Selector::Tags(TagSelector::AllOf(l)) => {
            obj(vec![("all_of", strs(l.as_slice().iter().map(TagName::untrusted_str)))])
        }
        Selector::SavedQuery(q) => obj(vec![("id", s(&q.uuid().to_string()))]),
        Selector::FieldTable(t) => obj(vec![("table", s(t.as_str()))]),
    };
    let mut pairs = vec![(r.selector.kind().as_str(), sel)];
    if let Some(o) = &r.outcome {
        pairs.push(("outcome", strs(o.iter().map(|x| x.as_str()))));
    }
    obj(pairs)
}

fn sort_value(k: Option<SortKey>) -> CValue {
    match k {
        None => CValue::Null,
        Some(k) => obj(vec![("direction", s(k.direction.as_str())), ("key", s(k.field.as_str()))]),
    }
}

fn repeat_value(r: &RepetitionRule) -> CValue {
    match r {
        RepetitionRule::Row => obj(vec![("kind", s("row"))]),
        RepetitionRule::Column(ColumnRule { max_columns_per_group, repeat_columns }) => obj(vec![
            ("kind", s("column")),
            ("max_columns_per_group", s(&max_columns_per_group.to_string())),
            (
                "repeat_columns",
                match repeat_columns {
                    None => CValue::Null,
                    Some(c) => strs(c.iter().map(ColumnKey::as_str)),
                },
            ),
        ]),
        RepetitionRule::PageGroup(PageGroupRule { pages }) => obj(vec![
            ("kind", s("page_group")),
            ("pages", CValue::Arr(pages.iter().map(|p| s(&p.uuid().to_string())).collect())),
        ]),
    }
}

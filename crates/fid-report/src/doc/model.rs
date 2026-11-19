//! `Report`, `Page`, `Block` and their canonical JSON form (REQ-001..004, REQ-008).
//!
//! Stored form (RFC 8785 canonical JSON, `schema_version` 1):
//!
//! ```text
//! { "id": <uuidv7>,
//!   "pages": [ { "blocks": [ { "id": <uuidv7>, "kind": <kind id>, "kind_version": "<u32>",
//!                              "payload": { "untrusted": true, "value": <json> } } ],
//!                "id": <uuidv7> } ],
//!   "retired_ids": [ <uuidv7>, ... ascending ],
//!   "schema_version": "1",
//!   "template_ref": null | { "id": <uuid>, "version": "<u32>" },
//!   "title": { "text": <string>, "untrusted": true } }
//! ```
//!
//! IDs are lowercase hyphenated text. Integers are decimal strings (the `fid-canon` 64-bit
//! integer rule), so the document holds no JSON numbers of its own. The reader is strict:
//! unknown or missing keys, a non-UUIDv7 ID, a repeated ID, an unsorted `retired_ids` and
//! any bound violation are `report.store_invalid`.

use super::error::ReportError;
use super::text::{Text, Untrusted};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_project::Uuid;
use std::collections::{BTreeMap, BTreeSet};

/// The only stored schema version this leaf reads and writes.
pub const SCHEMA_VERSION: u32 = 1;
/// Most pages in one report.
pub const MAX_PAGES: usize = 10_000;
/// Most blocks in one report, over all pages.
pub const MAX_BLOCKS: usize = 100_000;
/// Most IDs one report may ever issue (pages and blocks, live and retired). Bounds
/// `retired_ids`, which only grows (REQ-004).
pub const MAX_ISSUED_IDS: usize = 262_144;
/// Largest canonical payload of one block, in bytes.
pub const MAX_PAYLOAD_BYTES: usize = 65_536;
/// Deepest nesting of one payload (a scalar is depth 0, `{}` or `[]` depth 1).
pub const MAX_PAYLOAD_DEPTH: usize = 32;
/// Largest canonical report document, in bytes.
pub const MAX_REPORT_BYTES: usize = 32 * 1024 * 1024;

// ------------------------------------------------------------------ IDs

/// The canonical text of an ID: 36 lowercase hyphenated hex digits. Any other spelling is
/// refused, so one entity has exactly one text. Any UUID version is accepted here: params
/// may name a random UUID, which then fails as `report.not_found`.
pub(crate) fn parse_canonical_uuid(text: &str) -> Option<Uuid> {
    let id = Uuid::parse(text).ok()?;
    (id.to_string() == text).then_some(id)
}

/// Whether `id` is an RFC 9562 UUIDv7 (version 7, variant `0b10`).
pub fn is_v7(id: &Uuid) -> bool {
    id.version() == 7 && id.variant_bits() == 0b10
}

macro_rules! v7_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            /// Wraps a UUIDv7; any other UUID is refused.
            pub fn new(id: Uuid) -> Option<$name> {
                is_v7(&id).then_some($name(id))
            }

            pub fn uuid(&self) -> Uuid {
                self.0
            }

            pub(crate) fn to_value(self) -> CValue {
                CValue::Str(self.0.to_string())
            }

            /// Reads a stored or journaled ID: canonical text of a UUIDv7.
            pub(crate) fn from_value(v: &CValue) -> Result<$name, ReportError> {
                match v {
                    CValue::Str(s) => parse_canonical_uuid(s)
                        .and_then($name::new)
                        .ok_or(ReportError::StoreInvalid("id is not a canonical uuidv7")),
                    _ => Err(ReportError::StoreInvalid("id is not a string")),
                }
            }
        }
    };
}

v7_id!(
    /// A report ID (UUIDv7).
    ReportId
);
v7_id!(
    /// A page ID (UUIDv7).
    PageId
);
v7_id!(
    /// A block ID (UUIDv7).
    BlockId
);

/// A block kind ID: `^[a-z][a-z0-9_]{0,31}$`, e.g. `text`, `table`, `form_field`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockKindId(String);

impl BlockKindId {
    /// Longest kind ID, in bytes.
    pub const MAX_LEN: usize = 32;

    /// Parses a kind ID; `None` unless it matches `^[a-z][a-z0-9_]{0,31}$`.
    pub fn new(s: &str) -> Option<BlockKindId> {
        let b = s.as_bytes();
        let ok = !b.is_empty()
            && b.len() <= Self::MAX_LEN
            && b.first().is_some_and(u8::is_ascii_lowercase)
            && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_');
        ok.then(|| BlockKindId(s.to_string()))
    }

    /// A kind ID from a literal in kind code. The literal must match the pattern; tests
    /// assert it for every literal in this crate. Not for user input.
    #[cfg_attr(not(feature = "test-kinds"), allow(dead_code))]
    pub(crate) fn from_literal(s: &'static str) -> BlockKindId {
        BlockKindId(s.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ------------------------------------------------------------------ payload

/// A block payload in canonical form: re-parsed from its own RFC 8785 bytes, so two
/// payloads with the same canonical bytes are equal values. At most
/// [`MAX_PAYLOAD_BYTES`] canonical bytes and [`MAX_PAYLOAD_DEPTH`] levels deep.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalJson(CValue);

impl CanonicalJson {
    /// Canonicalises a value. `None` if it has no canonical form (non-finite number) or
    /// breaks a bound.
    pub fn new(v: &CValue) -> Option<CanonicalJson> {
        if depth_exceeds(v, MAX_PAYLOAD_DEPTH) {
            return None;
        }
        let bytes = to_canonical_bytes(v).ok()?;
        if bytes.len() > MAX_PAYLOAD_BYTES {
            return None;
        }
        parse_with_limits(&bytes, MAX_PAYLOAD_BYTES, MAX_PAYLOAD_DEPTH + 1).ok().map(CanonicalJson)
    }

    pub fn value(&self) -> &CValue {
        &self.0
    }
}

/// Whether `v` nests deeper than `max` (iterative, so hostile nesting cannot overflow).
fn depth_exceeds(v: &CValue, max: usize) -> bool {
    let mut stack = vec![(v, 0usize)];
    while let Some((v, d)) = stack.pop() {
        let children: Box<dyn Iterator<Item = &CValue>> = match v {
            CValue::Arr(a) => Box::new(a.iter()),
            CValue::Obj(o) => Box::new(o.values()),
            _ => continue,
        };
        if d + 1 > max {
            return true;
        }
        stack.extend(children.map(|c| (c, d + 1)));
    }
    false
}

// ------------------------------------------------------------------ types

/// The template a report was made from (set by `RPT.MODEL.TMPL`; never set by this leaf's
/// commands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateRef {
    pub id: Uuid,
    pub version: u32,
}

/// One content block. Its payload is opaque here; the kind's registered check validates
/// it (REQ-007).
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    id: BlockId,
    kind: BlockKindId,
    kind_version: u32,
    payload: CanonicalJson,
}

impl Block {
    pub(crate) fn new(id: BlockId, kind: BlockKindId, kind_version: u32, payload: CanonicalJson) -> Block {
        Block { id, kind, kind_version, payload }
    }

    pub fn id(&self) -> BlockId {
        self.id
    }

    pub fn kind(&self) -> &BlockKindId {
        &self.kind
    }

    pub fn kind_version(&self) -> u32 {
        self.kind_version
    }

    /// The payload. Its strings are content-derived: the machine form marks the whole
    /// payload `"untrusted": true`.
    pub fn payload(&self) -> &CanonicalJson {
        &self.payload
    }
}

/// One page: an ordered list of blocks.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    id: PageId,
    blocks: Vec<Block>,
}

impl Page {
    pub fn id(&self) -> PageId {
        self.id
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }
}

/// A report: an ordered list of pages. Invariants (kept by every constructor and
/// operation): every page and block ID and the report ID are distinct UUIDv7s, none of
/// them is in `retired_ids`, and the bounds above hold.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    id: ReportId,
    schema_version: u32,
    title: Untrusted<Text>,
    pages: Vec<Page>,
    template_ref: Option<TemplateRef>,
    retired_ids: BTreeSet<Uuid>,
}

impl Report {
    /// A new report with one empty page (REQ-001).
    pub(crate) fn new(id: ReportId, page: PageId, title: Untrusted<Text>) -> Result<Report, ReportError> {
        if id.uuid() == page.uuid() {
            return Err(ReportError::IdCollision(page.uuid()));
        }
        Ok(Report {
            id,
            schema_version: SCHEMA_VERSION,
            title,
            pages: vec![Page { id: page, blocks: Vec::new() }],
            template_ref: None,
            retired_ids: BTreeSet::new(),
        })
    }

    pub fn id(&self) -> ReportId {
        self.id
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn title(&self) -> &Untrusted<Text> {
        &self.title
    }

    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    pub fn template_ref(&self) -> Option<TemplateRef> {
        self.template_ref
    }

    /// IDs of removed pages and blocks; never issued again in this report (REQ-004).
    pub fn retired_ids(&self) -> &BTreeSet<Uuid> {
        &self.retired_ids
    }

    /// Number of blocks over all pages.
    pub fn block_count(&self) -> usize {
        self.pages.iter().map(|p| p.blocks.len()).sum()
    }

    /// Pages and blocks ever issued: live plus retired.
    fn issued_count(&self) -> usize {
        self.pages.len() + self.block_count() + self.retired_ids.len()
    }

    /// Whether `id` is the report ID, a live page or block ID, or retired.
    pub fn knows_id(&self, id: &Uuid) -> bool {
        *id == self.id.uuid()
            || self.retired_ids.contains(id)
            || self.pages.iter().any(|p| p.id.uuid() == *id || p.blocks.iter().any(|b| b.id.uuid() == *id))
    }

    fn page_index(&self, page: &Uuid) -> Option<usize> {
        self.pages.iter().position(|p| p.id.uuid() == *page)
    }

    /// `(page index, block index)` of a live block.
    fn block_position(&self, block: &Uuid) -> Option<(usize, usize)> {
        self.pages
            .iter()
            .enumerate()
            .find_map(|(pi, p)| p.blocks.iter().position(|b| b.id.uuid() == *block).map(|bi| (pi, bi)))
    }

    /// Whether `page` names a live page.
    pub(crate) fn has_page(&self, page: &Uuid) -> bool {
        self.page_index(page).is_some()
    }

    /// Whether `block` names a live block on `page`.
    pub(crate) fn page_has_block(&self, page: &Uuid, block: &Uuid) -> bool {
        self.page_index(page)
            .and_then(|pi| self.pages.get(pi))
            .is_some_and(|p| p.blocks.iter().any(|b| b.id.uuid() == *block))
    }

    fn fresh(&self, id: &Uuid) -> Result<(), ReportError> {
        if self.knows_id(id) {
            return Err(ReportError::IdCollision(*id));
        }
        if self.issued_count() >= MAX_ISSUED_IDS {
            return Err(ReportError::LimitExceeded("issued ids"));
        }
        Ok(())
    }

    /// Inserts an empty page after `after`, or at the end (REQ-003).
    pub(crate) fn insert_page(&mut self, after: Option<&Uuid>, page: PageId) -> Result<(), ReportError> {
        let at = match after {
            None => self.pages.len(),
            Some(a) => self.page_index(a).ok_or(ReportError::NotFound(*a))? + 1,
        };
        self.fresh(&page.uuid())?;
        if self.pages.len() >= MAX_PAGES {
            return Err(ReportError::LimitExceeded("pages"));
        }
        self.pages.insert(at, Page { id: page, blocks: Vec::new() });
        Ok(())
    }

    /// Removes a page; with `force` its blocks too. Every removed ID is retired (REQ-004,
    /// REQ-005).
    pub(crate) fn remove_page(&mut self, page: &Uuid, force: bool) -> Result<(), ReportError> {
        let at = self.page_index(page).ok_or(ReportError::NotFound(*page))?;
        let held = self.pages.get(at).map_or(0, |p| p.blocks.len());
        if held > 0 && !force {
            return Err(ReportError::PageNotEmpty(*page));
        }
        let removed = self.pages.remove(at);
        self.retired_ids.insert(removed.id.uuid());
        self.retired_ids.extend(removed.blocks.iter().map(|b| b.id.uuid()));
        Ok(())
    }

    /// Appends a block to `page`, or inserts it right after `after` on that page (REQ-002).
    pub(crate) fn insert_block(&mut self, page: &Uuid, after: Option<&Uuid>, block: Block) -> Result<(), ReportError> {
        let pi = self.page_index(page).ok_or(ReportError::NotFound(*page))?;
        self.fresh(&block.id.uuid())?;
        if self.block_count() >= MAX_BLOCKS {
            return Err(ReportError::LimitExceeded("blocks"));
        }
        let p = self.pages.get_mut(pi).ok_or(ReportError::NotFound(*page))?;
        let at = match after {
            None => p.blocks.len(),
            Some(a) => p.blocks.iter().position(|b| b.id.uuid() == *a).ok_or(ReportError::NotFound(*a))? + 1,
        };
        p.blocks.insert(at, block);
        Ok(())
    }

    /// Removes a block and retires its ID (REQ-004).
    pub(crate) fn remove_block(&mut self, block: &Uuid) -> Result<(), ReportError> {
        let (pi, bi) = self.block_position(block).ok_or(ReportError::NotFound(*block))?;
        let p = self.pages.get_mut(pi).ok_or(ReportError::NotFound(*block))?;
        if bi >= p.blocks.len() {
            return Err(ReportError::NotFound(*block));
        }
        let removed = p.blocks.remove(bi);
        self.retired_ids.insert(removed.id.uuid());
        Ok(())
    }

    // -------------------------------------------------------------- canonical form

    /// The canonical JSON value (the stored form and the `show_report` result).
    pub fn to_value(&self) -> CValue {
        let pages = self.pages.iter().map(page_value).collect();
        let retired = self.retired_ids.iter().map(|u| CValue::Str(u.to_string())).collect();
        let template = match &self.template_ref {
            None => CValue::Null,
            Some(t) => obj([("id", CValue::Str(t.id.to_string())), ("version", CValue::U64(u64::from(t.version)))]),
        };
        obj([
            ("id", self.id.to_value()),
            ("pages", CValue::Arr(pages)),
            ("retired_ids", CValue::Arr(retired)),
            ("schema_version", CValue::U64(u64::from(self.schema_version))),
            ("template_ref", template),
            ("title", self.title.to_value()),
        ])
    }

    /// RFC 8785 bytes of [`Report::to_value`]. `report.limit_exceeded` above
    /// [`MAX_REPORT_BYTES`].
    pub fn to_canonical(&self) -> Result<Vec<u8>, ReportError> {
        let bytes = to_canonical_bytes(&self.to_value()).map_err(|_| ReportError::StoreInvalid("not canonical"))?;
        if bytes.len() > MAX_REPORT_BYTES {
            return Err(ReportError::LimitExceeded("report bytes"));
        }
        Ok(bytes)
    }

    /// Reads canonical bytes back. The bytes must be exactly the canonical form of the
    /// report they describe; anything else is `report.store_invalid`.
    pub fn from_canonical(bytes: &[u8]) -> Result<Report, ReportError> {
        if bytes.len() > MAX_REPORT_BYTES {
            return Err(ReportError::LimitExceeded("report bytes"));
        }
        let v = parse_with_limits(bytes, MAX_REPORT_BYTES, fid_canon::MAX_DEPTH)
            .map_err(|_| ReportError::StoreInvalid("not json"))?;
        let r = Report::from_value(&v)?;
        if r.to_canonical()? != bytes {
            return Err(ReportError::StoreInvalid("bytes are not canonical"));
        }
        Ok(r)
    }

    /// Strict reader of [`Report::to_value`].
    pub fn from_value(v: &CValue) -> Result<Report, ReportError> {
        let o = exact_obj(v, &["id", "pages", "retired_ids", "schema_version", "template_ref", "title"])?;
        let id = ReportId::from_value(field(o, "id")?)?;
        let schema_version = u32_value(field(o, "schema_version")?)?;
        if schema_version != SCHEMA_VERSION {
            return Err(ReportError::StoreInvalid("unsupported schema_version"));
        }
        let title = Untrusted::from_value(field(o, "title")?)?;
        let template_ref = match field(o, "template_ref")? {
            CValue::Null => None,
            t => {
                let to = exact_obj(t, &["id", "version"])?;
                let tid = match field(to, "id")? {
                    CValue::Str(s) => parse_canonical_uuid(s).ok_or(ReportError::StoreInvalid("template id"))?,
                    _ => return Err(ReportError::StoreInvalid("template id")),
                };
                Some(TemplateRef { id: tid, version: u32_value(field(to, "version")?)? })
            }
        };
        let CValue::Arr(retired_in) = field(o, "retired_ids")? else {
            return Err(ReportError::StoreInvalid("retired_ids is not an array"));
        };
        if retired_in.len() > MAX_ISSUED_IDS {
            return Err(ReportError::LimitExceeded("issued ids"));
        }
        let mut seen: BTreeSet<Uuid> = BTreeSet::new();
        seen.insert(id.uuid());
        let mut retired_ids = BTreeSet::new();
        let mut last: Option<Uuid> = None;
        for r in retired_in {
            let u = match r {
                CValue::Str(s) => parse_canonical_uuid(s).filter(is_v7),
                _ => None,
            }
            .ok_or(ReportError::StoreInvalid("retired id is not a canonical uuidv7"))?;
            if last.is_some_and(|l| l >= u) {
                return Err(ReportError::StoreInvalid("retired_ids not strictly ascending"));
            }
            last = Some(u);
            if !seen.insert(u) {
                return Err(ReportError::StoreInvalid("repeated id"));
            }
            retired_ids.insert(u);
        }
        let CValue::Arr(pages_in) = field(o, "pages")? else {
            return Err(ReportError::StoreInvalid("pages is not an array"));
        };
        if pages_in.len() > MAX_PAGES {
            return Err(ReportError::LimitExceeded("pages"));
        }
        let mut pages = Vec::with_capacity(pages_in.len());
        let mut blocks_total = 0usize;
        for p in pages_in {
            let po = exact_obj(p, &["blocks", "id"])?;
            let pid = PageId::from_value(field(po, "id")?)?;
            if !seen.insert(pid.uuid()) {
                return Err(ReportError::StoreInvalid("repeated id"));
            }
            let CValue::Arr(blocks_in) = field(po, "blocks")? else {
                return Err(ReportError::StoreInvalid("blocks is not an array"));
            };
            blocks_total = blocks_total.saturating_add(blocks_in.len());
            if blocks_total > MAX_BLOCKS {
                return Err(ReportError::LimitExceeded("blocks"));
            }
            let mut blocks = Vec::with_capacity(blocks_in.len());
            for b in blocks_in {
                let block = block_from_value(b)?;
                if !seen.insert(block.id.uuid()) {
                    return Err(ReportError::StoreInvalid("repeated id"));
                }
                blocks.push(block);
            }
            pages.push(Page { id: pid, blocks });
        }
        if seen.len() - 1 > MAX_ISSUED_IDS {
            return Err(ReportError::LimitExceeded("issued ids"));
        }
        Ok(Report { id, schema_version, title, pages, template_ref, retired_ids })
    }
}

fn page_value(p: &Page) -> CValue {
    obj([("blocks", CValue::Arr(p.blocks.iter().map(block_value).collect())), ("id", p.id.to_value())])
}

fn block_value(b: &Block) -> CValue {
    obj([
        ("id", b.id.to_value()),
        ("kind", CValue::Str(b.kind.as_str().to_string())),
        ("kind_version", CValue::U64(u64::from(b.kind_version))),
        ("payload", payload_value(&b.payload)),
    ])
}

/// `{"untrusted": true, "value": <payload>}`: the payload is opaque, content-derived data.
pub(crate) fn payload_value(p: &CanonicalJson) -> CValue {
    obj([("untrusted", CValue::Bool(true)), ("value", p.0.clone())])
}

pub(crate) fn payload_from_value(v: &CValue) -> Result<CanonicalJson, ReportError> {
    let o = exact_obj(v, &["untrusted", "value"])?;
    if field(o, "untrusted")? != &CValue::Bool(true) {
        return Err(ReportError::StoreInvalid("payload marker"));
    }
    let p = CanonicalJson::new(field(o, "value")?).ok_or(ReportError::StoreInvalid("payload out of bounds"))?;
    Ok(p)
}

fn block_from_value(v: &CValue) -> Result<Block, ReportError> {
    let o = exact_obj(v, &["id", "kind", "kind_version", "payload"])?;
    let id = BlockId::from_value(field(o, "id")?)?;
    let kind = match field(o, "kind")? {
        CValue::Str(s) => BlockKindId::new(s).ok_or(ReportError::StoreInvalid("kind id"))?,
        _ => return Err(ReportError::StoreInvalid("kind id")),
    };
    let kind_version = u32_value(field(o, "kind_version")?)?;
    if kind_version == 0 {
        return Err(ReportError::StoreInvalid("kind_version is 0"));
    }
    let payload = payload_from_value(field(o, "payload")?)?;
    Ok(Block { id, kind, kind_version, payload })
}

pub(crate) fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// An object with exactly `keys`.
pub(crate) fn exact_obj<'a>(v: &'a CValue, keys: &[&str]) -> Result<&'a BTreeMap<String, CValue>, ReportError> {
    let CValue::Obj(o) = v else { return Err(ReportError::StoreInvalid("not an object")) };
    if o.len() != keys.len() || keys.iter().any(|k| !o.contains_key(*k)) {
        return Err(ReportError::StoreInvalid("unexpected keys"));
    }
    Ok(o)
}

pub(crate) fn field<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a CValue, ReportError> {
    o.get(k).ok_or(ReportError::StoreInvalid("missing key"))
}

/// A `u32` written as a canonical decimal string (or [`CValue::U64`] before writing).
pub(crate) fn u32_value(v: &CValue) -> Result<u32, ReportError> {
    let bad = ReportError::StoreInvalid("not a canonical u32 string");
    match v {
        CValue::U64(n) => u32::try_from(*n).map_err(|_| bad),
        CValue::Str(s)
            if !s.is_empty()
                && s.len() <= 10
                && s.bytes().all(|c| c.is_ascii_digit())
                && (s == "0" || !s.starts_with('0')) =>
        {
            s.parse::<u32>().map_err(|_| bad)
        }
        _ => Err(bad),
    }
}

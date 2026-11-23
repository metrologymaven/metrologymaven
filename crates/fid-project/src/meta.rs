//! Tags, keywords and key-value metadata for project elements and the project itself
//! (spec node `CORE.PRJ.META`, `REQ-001`..`REQ-008`, `AC-1`..`AC-7`).
//!
//! All tag and metadata values are untrusted user content (`Untrusted<String>`, CONSTITUTION C6)
//! and are never expanded, evaluated or templated. Every collection is ordered (`BTreeSet` /
//! `BTreeMap`) so results and canonical event bytes are D0 deterministic.

use crate::tree::Tree;
use crate::Uuid;
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use unicode_normalization::UnicodeNormalization;

/// Command ID for replacing a target's tag set (W2, D0, undo `head_move`).
pub const SET_TAGS: &str = "project.set_tags";
/// Command ID for updating/removing metadata keys in one commit (W2, D0, undo `head_move`).
pub const SET_METADATA: &str = "project.set_metadata";
/// Command ID for finding elements carrying a tag (R0, D0).
pub const FIND_ELEMENTS_BY_TAG: &str = "project.find_elements_by_tag";

/// Event emitted by `project.set_tags@1`.
pub const TAGS_SET: &str = "TagsSet";
/// Event emitted by `project.set_metadata@1`.
pub const METADATA_SET: &str = "MetadataSet";

/// Target sentinel naming the project itself (`REQ-001`).
pub const PROJECT_TARGET: &str = "project";

/// Minimum tag length in Unicode scalar characters after NFC normalisation (`REQ-002`).
pub const MIN_TAG_CHARS: usize = 1;
/// Maximum tag length in Unicode scalar characters after NFC normalisation (`REQ-002`).
pub const MAX_TAG_CHARS: usize = 64;
/// Maximum distinct tags allowed on one target (`REQ-002`).
pub const MAX_TAGS_PER_TARGET: usize = 64;
/// Maximum metadata key length in ASCII bytes (`REQ-004`).
pub const MAX_KEY_BYTES: usize = 64;
/// Maximum metadata value length in UTF-8 bytes (`REQ-004`).
pub const MAX_VALUE_BYTES: usize = 4_096;

/// Regex pattern for valid metadata keys (`REQ-004`).
pub const META_KEY_PATTERN: &str = "^[a-z0-9_.-]{1,64}$";

// ------------------------------------------------------------------ untrusted & validated types

/// Wrapper marking user-supplied text as untrusted content (CONSTITUTION C6).
/// The inner value is stored verbatim and never interpreted, expanded or templated (`REQ-005`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Untrusted<T>(T);

impl<T> Untrusted<T> {
    /// Wraps `value` as untrusted user content.
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// Borrows the raw untrusted value without interpreting it.
    pub const fn as_untrusted(&self) -> &T {
        &self.0
    }

    /// Consumes the wrapper and returns the raw untrusted value.
    pub fn into_untrusted(self) -> T {
        self.0
    }
}

impl Untrusted<String> {
    pub fn as_untrusted_str(&self) -> &str {
        self.0.as_str()
    }

    /// Wire envelope `{"text": "...", "untrusted": true}` matching `fid_project::query`.
    pub fn to_wire_value(&self) -> CValue {
        CValue::Obj(BTreeMap::from([
            ("text".to_string(), CValue::Str(self.0.clone())),
            ("untrusted".to_string(), CValue::Bool(true)),
        ]))
    }
}

/// Validated, NFC-normalised tag (`1..=64` Unicode scalar characters, untrusted; `REQ-002`, `REQ-005`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tag(Untrusted<String>);

impl Tag {
    /// NFC-normalises `raw` and enforces `1..=64` Unicode scalar characters (`REQ-002`).
    pub fn parse(raw: &str) -> Result<Self, MetaError> {
        let norm: String = raw.nfc().collect();
        let len = norm.chars().count();
        if !(MIN_TAG_CHARS..=MAX_TAG_CHARS).contains(&len) {
            return Err(MetaError::Validation(format!(
                "tag length {len} must be between {MIN_TAG_CHARS} and {MAX_TAG_CHARS} characters"
            )));
        }
        Ok(Self(Untrusted::new(norm)))
    }

    pub fn as_untrusted(&self) -> &Untrusted<String> {
        &self.0
    }

    pub fn as_untrusted_str(&self) -> &str {
        self.0.as_untrusted_str()
    }
}

/// Validated metadata key matching `^[a-z0-9_.-]{1,64}$` (`REQ-004`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MetaKey(String);

impl MetaKey {
    pub fn parse(raw: &str) -> Result<Self, MetaError> {
        if raw.is_empty() || raw.len() > MAX_KEY_BYTES {
            return Err(MetaError::Validation(format!(
                "metadata key length {} must be between 1 and {MAX_KEY_BYTES} bytes",
                raw.len()
            )));
        }
        if !raw.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'-')) {
            return Err(MetaError::Validation(format!(
                "metadata key {:?} must match {META_KEY_PATTERN}",
                raw.chars().take(64).collect::<String>()
            )));
        }
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Validates a metadata value (`<= 4,096` UTF-8 bytes) and wraps it as [`Untrusted<String>`] (`REQ-004`, `REQ-005`).
pub fn validate_meta_value(raw: &str) -> Result<Untrusted<String>, MetaError> {
    if raw.len() > MAX_VALUE_BYTES {
        return Err(MetaError::Validation(format!(
            "metadata value length {} exceeds {MAX_VALUE_BYTES} UTF-8 bytes",
            raw.len()
        )));
    }
    Ok(Untrusted::new(raw.to_string()))
}

/// Target of `project.set_tags@1` or `project.set_metadata@1`: either an element ID or `"project"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetaTarget {
    Project,
    Element(Uuid),
}

impl MetaTarget {
    pub fn parse(raw: &str) -> Result<Self, MetaError> {
        if raw == PROJECT_TARGET {
            return Ok(MetaTarget::Project);
        }
        Uuid::parse(raw).map(MetaTarget::Element).map_err(|_| {
            MetaError::Validation(format!("target must be \"project\" or a UUID, got {:?}", truncate(raw)))
        })
    }

    pub fn to_wire_string(self) -> String {
        match self {
            MetaTarget::Project => PROJECT_TARGET.to_string(),
            MetaTarget::Element(id) => id.to_string(),
        }
    }
}

// ------------------------------------------------------------------ errors

/// Typed error of `CORE.PRJ.META` operations (`REQ-002`, `REQ-004`, `REQ-006`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaError {
    /// Invalid target format, tag limit/length, key grammar or value byte bound (`validation/schema`).
    Validation(String),
    /// Target element ID does not exist in the project (`not_found/unknown_entity`).
    NotFound(String),
    /// Malformed event or internal store inconsistency (`internal/invariant_violated`).
    Invariant(String),
}

impl MetaError {
    pub fn bus_code(&self) -> (&'static str, &'static str) {
        match self {
            MetaError::Validation(_) => ("validation", "schema"),
            MetaError::NotFound(_) => ("not_found", "unknown_entity"),
            MetaError::Invariant(_) => ("internal", "invariant_violated"),
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            MetaError::Validation(s) | MetaError::NotFound(s) | MetaError::Invariant(s) => s.as_str(),
        }
    }
}

impl fmt::Display for MetaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (c, r) = self.bus_code();
        write!(f, "{c}/{r}: {}", self.detail())
    }
}

impl std::error::Error for MetaError {}

fn truncate(s: &str) -> String {
    s.chars().take(64).collect()
}

// ------------------------------------------------------------------ events

/// Committed event produced by `project.set_tags@1` or `project.set_metadata@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaEvent {
    TagsSet { target: MetaTarget, tags: BTreeSet<Tag> },
    MetadataSet { target: MetaTarget, metadata: BTreeMap<MetaKey, Untrusted<String>> },
}

impl MetaEvent {
    pub const fn type_id(&self) -> &'static str {
        match self {
            MetaEvent::TagsSet { .. } => TAGS_SET,
            MetaEvent::MetadataSet { .. } => METADATA_SET,
        }
    }

    /// Canonical event body (ordered keys, untrusted text preserved verbatim).
    pub fn body(&self) -> CValue {
        match self {
            MetaEvent::TagsSet { target, tags } => CValue::Obj(BTreeMap::from([
                (
                    "tags".to_string(),
                    CValue::Arr(tags.iter().map(|t| CValue::Str(t.as_untrusted_str().to_string())).collect()),
                ),
                ("target".to_string(), CValue::Str(target.to_wire_string())),
            ])),
            MetaEvent::MetadataSet { target, metadata } => {
                let m: BTreeMap<String, CValue> = metadata
                    .iter()
                    .map(|(k, v)| (k.as_str().to_string(), CValue::Str(v.as_untrusted_str().to_string())))
                    .collect();
                CValue::Obj(BTreeMap::from([
                    ("metadata".to_string(), CValue::Obj(m)),
                    ("target".to_string(), CValue::Str(target.to_wire_string())),
                ]))
            }
        }
    }

    /// Command result value carrying explicit untrusted wrappers (`REQ-005`, `AC-6`).
    pub fn result_value(&self) -> CValue {
        match self {
            MetaEvent::TagsSet { target, tags } => CValue::Obj(BTreeMap::from([
                ("tags".to_string(), CValue::Arr(tags.iter().map(|t| t.as_untrusted().to_wire_value()).collect())),
                ("target".to_string(), CValue::Str(target.to_wire_string())),
            ])),
            MetaEvent::MetadataSet { target, metadata } => {
                let m: BTreeMap<String, CValue> =
                    metadata.iter().map(|(k, v)| (k.as_str().to_string(), v.to_wire_value())).collect();
                CValue::Obj(BTreeMap::from([
                    ("metadata".to_string(), CValue::Obj(m)),
                    ("target".to_string(), CValue::Str(target.to_wire_string())),
                ]))
            }
        }
    }

    /// Parses a committed event back into [`MetaEvent`] for exact replay / reconstruction (`AC-7`).
    pub fn from_wire(event_type: &str, body: &CValue) -> Result<Self, MetaError> {
        let CValue::Obj(o) = body else {
            return Err(MetaError::Invariant("meta event body must be an object".into()));
        };
        let target_str = match o.get("target") {
            Some(CValue::Str(s)) => s.as_str(),
            _ => return Err(MetaError::Invariant("meta event missing string target".into())),
        };
        let target = MetaTarget::parse(target_str)?;
        match event_type {
            TAGS_SET => {
                let Some(CValue::Arr(arr)) = o.get("tags") else {
                    return Err(MetaError::Invariant("TagsSet missing tags array".into()));
                };
                let mut tags = BTreeSet::new();
                for item in arr {
                    let CValue::Str(s) = item else {
                        return Err(MetaError::Invariant("TagsSet tag must be a string".into()));
                    };
                    tags.insert(Tag::parse(s)?);
                }
                if tags.len() > MAX_TAGS_PER_TARGET {
                    return Err(MetaError::Validation(format!("target may hold at most {MAX_TAGS_PER_TARGET} tags")));
                }
                Ok(MetaEvent::TagsSet { target, tags })
            }
            METADATA_SET => {
                let Some(CValue::Obj(m)) = o.get("metadata") else {
                    return Err(MetaError::Invariant("MetadataSet missing metadata object".into()));
                };
                let mut metadata = BTreeMap::new();
                for (k, v) in m {
                    let key = MetaKey::parse(k)?;
                    let CValue::Str(s) = v else {
                        return Err(MetaError::Invariant("MetadataSet value must be a string".into()));
                    };
                    metadata.insert(key, validate_meta_value(s)?);
                }
                Ok(MetaEvent::MetadataSet { target, metadata })
            }
            other => Err(MetaError::Invariant(format!("unknown meta event type {:?}", truncate(other)))),
        }
    }
}

// ------------------------------------------------------------------ state store & index

/// Deterministic project-wide store of tags, metadata and the inverted `tag -> elements` index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetaStore {
    project_tags: BTreeSet<Tag>,
    project_metadata: BTreeMap<MetaKey, Untrusted<String>>,
    element_tags: BTreeMap<Uuid, BTreeSet<Tag>>,
    element_metadata: BTreeMap<Uuid, BTreeMap<MetaKey, Untrusted<String>>>,
    tag_index: BTreeMap<Tag, BTreeSet<Uuid>>,
}

impl MetaStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tags attached to the project itself.
    pub fn project_tags(&self) -> &BTreeSet<Tag> {
        &self.project_tags
    }

    /// Key-value metadata attached to the project itself.
    pub fn project_metadata(&self) -> &BTreeMap<MetaKey, Untrusted<String>> {
        &self.project_metadata
    }

    /// Tags attached to element `id`.
    pub fn element_tags(&self, id: &Uuid) -> Option<&BTreeSet<Tag>> {
        self.element_tags.get(id)
    }

    /// Key-value metadata attached to element `id`.
    pub fn element_metadata(&self, id: &Uuid) -> Option<&BTreeMap<MetaKey, Untrusted<String>>> {
        self.element_metadata.get(id)
    }

    /// Current metadata map for `target`.
    pub fn metadata_for(&self, target: MetaTarget) -> BTreeMap<MetaKey, Untrusted<String>> {
        match target {
            MetaTarget::Project => self.project_metadata.clone(),
            MetaTarget::Element(id) => self.element_metadata.get(&id).cloned().unwrap_or_default(),
        }
    }

    /// Applies one committed [`MetaEvent`], maintaining the inverted tag index and optionally
    /// updating the element's `meta` slot in [`Tree`] so `describe_element` sees the latest tags
    /// and metadata (`AC-1`).
    pub fn apply(&mut self, event: &MetaEvent, tree: Option<&mut Tree>) -> Result<(), MetaError> {
        match event {
            MetaEvent::TagsSet { target, tags } => {
                match target {
                    MetaTarget::Project => {
                        self.project_tags = tags.clone();
                    }
                    MetaTarget::Element(id) => {
                        // Remove old inverted-index entries for this element.
                        if let Some(old) = self.element_tags.get(id) {
                            for tag in old {
                                if let Some(ids) = self.tag_index.get_mut(tag) {
                                    ids.remove(id);
                                    if ids.is_empty() {
                                        self.tag_index.remove(tag);
                                    }
                                }
                            }
                        }
                        for tag in tags {
                            self.tag_index.entry(tag.clone()).or_default().insert(*id);
                        }
                        self.element_tags.insert(*id, tags.clone());
                        if let Some(t) = tree {
                            self.sync_element_tree_meta(t, *id);
                        }
                    }
                }
                Ok(())
            }
            MetaEvent::MetadataSet { target, metadata } => {
                match target {
                    MetaTarget::Project => {
                        self.project_metadata = metadata.clone();
                    }
                    MetaTarget::Element(id) => {
                        self.element_metadata.insert(*id, metadata.clone());
                        if let Some(t) = tree {
                            self.sync_element_tree_meta(t, *id);
                        }
                    }
                }
                Ok(())
            }
        }
    }

    /// Rebuilds a [`MetaStore`] from scratch by folding committed events in sequence order
    /// (also resetting and syncing element metadata in `tree` if supplied; `AC-7`).
    pub fn rebuild(events: &[MetaEvent], mut tree: Option<&mut Tree>) -> Result<Self, MetaError> {
        let mut store = Self::new();
        if let Some(t) = tree.as_deref_mut() {
            let ids: Vec<Uuid> = t.elements().map(|e| e.id()).collect();
            for id in ids {
                if let Some(elem) = t.element_mut(&id) {
                    elem.set_meta(CValue::Obj(BTreeMap::new()));
                }
            }
        }
        for ev in events {
            store.apply(ev, tree.as_deref_mut())?;
        }
        Ok(store)
    }

    fn sync_element_tree_meta(&self, tree: &mut Tree, id: Uuid) {
        let Some(elem) = tree.element_mut(&id) else {
            return;
        };
        let mut obj = BTreeMap::new();
        if let Some(meta) = self.element_metadata.get(&id) {
            for (k, v) in meta {
                obj.insert(k.as_str().to_string(), CValue::Str(v.as_untrusted_str().to_string()));
            }
        }
        if let Some(tags) = self.element_tags.get(&id) {
            obj.insert(
                "tags".to_string(),
                CValue::Arr(tags.iter().map(|t| CValue::Str(t.as_untrusted_str().to_string())).collect()),
            );
        }
        elem.set_meta(CValue::Obj(obj));
    }

    /// Finds all element IDs carrying `tag` (NFC-normalised), sorted ascending by element ID
    /// (`REQ-007`, `AC-3`).
    pub fn find_elements_by_tag(&self, raw_tag: &str) -> Result<Vec<Uuid>, MetaError> {
        let tag = Tag::parse(raw_tag)?;
        Ok(self.tag_index.get(&tag).map(|s| s.iter().copied().collect()).unwrap_or_default())
    }
}

// ------------------------------------------------------------------ command planners

/// Plans `project.set_tags@1` (`REQ-001`, `REQ-002`, `REQ-005`, `REQ-006`).
///
/// `element_exists` checks whether an element UUID exists in the current immutable state view.
pub fn plan_set_tags<F>(element_exists: F, params: &CValue) -> Result<(MetaEvent, CValue), MetaError>
where
    F: Fn(&Uuid) -> bool,
{
    let o = expect_obj(params, &["tags", "target"])?;
    let target = parse_target_checked(o, element_exists)?;
    let Some(CValue::Arr(raw_tags)) = o.get("tags") else {
        return Err(MetaError::Validation("missing tags array".into()));
    };
    let mut tags = BTreeSet::new();
    for item in raw_tags {
        let CValue::Str(s) = item else {
            return Err(MetaError::Validation("each tag must be a string".into()));
        };
        tags.insert(Tag::parse(s)?);
    }
    if tags.len() > MAX_TAGS_PER_TARGET {
        return Err(MetaError::Validation(format!("tag count {} exceeds maximum {MAX_TAGS_PER_TARGET}", tags.len())));
    }
    let ev = MetaEvent::TagsSet { target, tags };
    let res = ev.result_value();
    Ok((ev, res))
}

/// Plans `project.set_metadata@1` (`REQ-003`, `REQ-004`, `REQ-005`, `REQ-006`).
///
/// Supports both `unset: ["key"]` and `set: {"key": null}` to remove keys in one commit, while
/// setting every string entry in `set` verbatim as untrusted text.
pub fn plan_set_metadata<F>(
    element_exists: F,
    store: &MetaStore,
    params: &CValue,
) -> Result<(MetaEvent, CValue), MetaError>
where
    F: Fn(&Uuid) -> bool,
{
    let o = expect_obj(params, &["set", "target", "unset"])?;
    let target = parse_target_checked(o, element_exists)?;
    let mut next = store.metadata_for(target);

    if let Some(set_val) = o.get("set") {
        let CValue::Obj(set_map) = set_val else {
            return Err(MetaError::Validation("set must be an object".into()));
        };
        for (k, v) in set_map {
            let key = MetaKey::parse(k)?;
            match v {
                CValue::Str(s) => {
                    next.insert(key, validate_meta_value(s)?);
                }
                CValue::Null => {
                    next.remove(&key);
                }
                _ => {
                    return Err(MetaError::Validation(format!(
                        "metadata value for key {:?} must be a string or null",
                        key.as_str()
                    )));
                }
            }
        }
    }

    if let Some(unset_val) = o.get("unset") {
        let CValue::Arr(unset_arr) = unset_val else {
            return Err(MetaError::Validation("unset must be an array of keys".into()));
        };
        for item in unset_arr {
            let CValue::Str(k) = item else {
                return Err(MetaError::Validation("each unset entry must be a key string".into()));
            };
            let key = MetaKey::parse(k)?;
            next.remove(&key);
        }
    }

    let ev = MetaEvent::MetadataSet { target, metadata: next };
    let res = ev.result_value();
    Ok((ev, res))
}

/// Executes `project.find_elements_by_tag@1` (`REQ-007`, R0).
pub fn run_find_elements_by_tag(store: &MetaStore, params: &CValue) -> Result<CValue, MetaError> {
    let o = expect_obj(params, &["tag"])?;
    let Some(CValue::Str(raw_tag)) = o.get("tag") else {
        return Err(MetaError::Validation("missing string parameter tag".into()));
    };
    let ids = store.find_elements_by_tag(raw_tag)?;
    Ok(CValue::Obj(BTreeMap::from([(
        "elements".to_string(),
        CValue::Arr(ids.into_iter().map(|id| CValue::Str(id.to_string())).collect()),
    )])))
}

fn expect_obj<'a>(params: &'a CValue, allowed: &[&str]) -> Result<&'a BTreeMap<String, CValue>, MetaError> {
    let CValue::Obj(o) = params else {
        return Err(MetaError::Validation("parameters must be an object".into()));
    };
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(MetaError::Validation(format!("unknown parameter {:?}", truncate(k))));
        }
    }
    Ok(o)
}

fn parse_target_checked<F>(o: &BTreeMap<String, CValue>, element_exists: F) -> Result<MetaTarget, MetaError>
where
    F: Fn(&Uuid) -> bool,
{
    let Some(CValue::Str(raw_target)) = o.get("target") else {
        return Err(MetaError::Validation("missing string parameter target".into()));
    };
    let target = MetaTarget::parse(raw_target)?;
    if let MetaTarget::Element(id) = target {
        if !element_exists(&id) {
            return Err(MetaError::NotFound(format!("unknown element {id}")));
        }
    }
    Ok(target)
}

//! Schema keyword vocabulary, meta-schema and schema checker. Spec node
//! `CORE.REG.DEF.SCHEMA` (REQ-001 to REQ-003, REQ-006 to REQ-009).
//!
//! Parameter and result schemas are JSON Schema 2020-12 plus nine project keywords
//! ([`ProjectKeyword`]). [`check_schema`] refuses every keyword outside a closed, sorted table
//! ([`KEYWORDS`]) and checks every keyword value; [`registry_keywords_schema`] is the same
//! vocabulary as a meta-schema, checked in as `schemas/registry-keywords.schema.json`.
//!
//! The conventions are those of `CORE.REG.VAL` (`validate`): `$schema` only at the root and
//! only 2020-12, keywords of earlier drafts refused, `$ref` only `#` or `#/<pointer>` landing
//! on a position the walk checks as a schema, integers above 2^53 refused, property names
//! printable ASCII, error echoes escaped and bounded. Iterative walk with an explicit stack,
//! no panics. Every choice is pinned in `spec/core/reg/def/schema/NODE.md` (Design).

use crate::catalogue::QuantityCatalogue;
use crate::validate::{
    cut, number, pointer, prescan, quote, InputError, Seg, KEEP_MESSAGE, MAX_COUNT, OLD_DRAFT_KEYWORDS, SCHEMA_DIALECT,
    TYPES,
};
use fid_canon::CValue;
use std::collections::{btree_map, BTreeMap, BTreeSet, BinaryHeap};
use std::fmt;

/// Largest accepted schema document, in bytes (1 MiB): the raw bytes for
/// [`check_schema_json`], the RFC 8785 canonical form for [`check_schema`].
pub const SCHEMA_MAX_BYTES: usize = 1024 * 1024;
/// Deepest accepted nesting (the schema object is level 1; every object or array below adds 1).
pub const SCHEMA_MAX_DEPTH: usize = 64;
/// Most issues reported for one document; beyond it the list ends with `too_many_issues`.
pub const MAX_ISSUES: usize = 10_000;
/// Version of the project vocabulary; a change to a closed list is a new version.
pub const REGISTRY_KEYWORDS_VERSION: u32 = 2;
/// `$id` of the meta-schema.
pub const REGISTRY_KEYWORDS_ID: &str = "https://metrologymaven.io/schemas/registry-keywords.schema.json";
/// The `x-standards-ref` pattern (REQ-007): an identifier-like designator (a letter, then
/// letters, digits, `.` and `-`; at most 16 characters, no space or colon), `:`, a
/// four-digit edition year and an optional clause; at most 42 characters by construction.
pub const STANDARDS_REF_PATTERN: &str = "^[A-Za-z][A-Za-z0-9.-]{0,15}:[0-9]{4}( cl\\. [0-9.]{1,16})?$";
/// Longest designator of an `x-standards-ref`.
pub const MAX_DESIGNATOR: usize = 16;
/// Longest `x-standards-ref`.
pub const MAX_STANDARDS_REF: usize = 64;
/// The pattern of one `x-extensions` entry: a lowercase file extension without the dot.
pub const EXTENSION_PATTERN: &str = "^[a-z0-9]{1,16}$";
/// Longest `x-extensions` entry, in bytes (the guard's `MAX_EXT_BYTES`, CORE.BUS.POL.FS).
pub const MAX_EXTENSION_BYTES: usize = 16;
/// Most entries of one `x-extensions` list (the guard's `MAX_EXTENSIONS`, CORE.BUS.POL.FS).
pub const MAX_EXTENSIONS: usize = 64;

macro_rules! closed_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($variant,)+
        }

        impl $name {
            /// Every value, in wire-string order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The wire string.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $s,)+
                }
            }

            /// The value with this exact wire string.
            pub fn parse(s: &str) -> Option<$name> {
                Self::ALL.iter().copied().find(|v| v.as_str() == s)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

closed_enum! {
    /// The nine project keywords (REQ-001), in byte order.
    ProjectKeyword {
        PathLike => "path_like",
        XExtensions => "x-extensions",
        XQuantity => "x-quantity",
        XRef => "x-ref",
        XSelector => "x-selector",
        XStandardsRef => "x-standards-ref",
        XUi => "x-ui",
        XUnit => "x-unit",
        XUntrusted => "x-untrusted",
    }
}

closed_enum! {
    /// Entity types `x-ref` may name (REQ-008; version [`REGISTRY_KEYWORDS_VERSION`]).
    EntityType {
        Alignment => "alignment",
        Characteristic => "characteristic",
        Datum => "datum",
        Element => "element",
        Feature => "feature",
        Frame => "frame",
        Part => "part",
        Plan => "plan",
        Result => "result",
        Revision => "revision",
        Run => "run",
    }
}

closed_enum! {
    /// Selector forms `x-selector` may name (REQ-008; version [`REGISTRY_KEYWORDS_VERSION`]):
    /// one entity ID, or a list of entity IDs.
    SelectorForm {
        List => "list",
        Single => "single",
    }
}

closed_enum! {
    /// Issue codes, stable snake_case strings.
    SchemaCode {
        BadDialect => "bad_dialect",
        BadJson => "bad_json",
        BadKeywordValue => "bad_keyword_value",
        BadNumber => "bad_number",
        BadPropertyName => "bad_property_name",
        BadSchema => "bad_schema",
        BadStandardsRef => "bad_standards_ref",
        DuplicateKey => "duplicate_key",
        ExternalRef => "external_ref",
        MisplacedPathLike => "misplaced_path_like",
        SchemaTooDeep => "schema_too_deep",
        SchemaTooLarge => "schema_too_large",
        TooManyIssues => "too_many_issues",
        UnitNotAccepted => "unit_not_accepted",
        UnknownKeyword => "unknown_keyword",
        UnknownQuantity => "unknown_quantity",
        UntrustedOnNonString => "untrusted_on_non_string",
        UnwalkedRef => "unwalked_ref",
    }
}

/// How a keyword's value is checked (and walked).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A schema (object or boolean).
    Schema,
    /// An object of schemas keyed by property or definition names.
    SchemaMap,
    /// `patternProperties`: an object of schemas keyed by regexes.
    PatternMap,
    /// A non-empty array of schemas.
    SchemaList,
    /// `type`.
    Type,
    /// A number (integers at most 2^53 in magnitude).
    Number,
    /// `multipleOf`: a positive number.
    PositiveNumber,
    /// An integer `0..=2^53`.
    Count,
    /// A boolean.
    Bool,
    /// A string.
    Str,
    /// An array of anything (`enum`, `examples`).
    Array,
    /// Any JSON value, not walked as a schema (`const`, `default`).
    Any,
    /// `required`: an array of distinct property names.
    Required,
    /// `dependentRequired`: an object of arrays of distinct property names.
    DependentRequired,
    /// `$schema`.
    Dialect,
    /// `$id`.
    Id,
    /// `$anchor`, `$dynamicAnchor`.
    Anchor,
    /// `$ref`.
    Ref,
    /// `$dynamicRef` (refused: resolved at evaluation time).
    DynamicRef,
    /// `$vocabulary`: an object of booleans.
    Vocabulary,
    /// A project keyword.
    Project(ProjectKeyword),
}

/// A keyword of the closed vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyword {
    /// A keyword of the JSON Schema 2020-12 vocabularies (core, applicator, unevaluated,
    /// validation, meta-data, format-annotation, content).
    Standard(&'static str),
    /// A project keyword.
    Project(ProjectKeyword),
}

impl Keyword {
    /// The keyword as written.
    pub fn as_str(self) -> &'static str {
        match self {
            Keyword::Standard(s) => s,
            Keyword::Project(p) => p.as_str(),
        }
    }
}

/// The closed keyword vocabulary, sorted by byte order for binary search: the 2020-12
/// vocabularies (without the deprecated `definitions`, `dependencies`, `$recursiveAnchor`,
/// `$recursiveRef`) plus the nine project keywords.
pub const KEYWORDS: [(&str, Role); 66] = [
    ("$anchor", Role::Anchor),
    ("$comment", Role::Str),
    ("$defs", Role::SchemaMap),
    ("$dynamicAnchor", Role::Anchor),
    ("$dynamicRef", Role::DynamicRef),
    ("$id", Role::Id),
    ("$ref", Role::Ref),
    ("$schema", Role::Dialect),
    ("$vocabulary", Role::Vocabulary),
    ("additionalProperties", Role::Schema),
    ("allOf", Role::SchemaList),
    ("anyOf", Role::SchemaList),
    ("const", Role::Any),
    ("contains", Role::Schema),
    ("contentEncoding", Role::Str),
    ("contentMediaType", Role::Str),
    ("contentSchema", Role::Schema),
    ("default", Role::Any),
    ("dependentRequired", Role::DependentRequired),
    ("dependentSchemas", Role::SchemaMap),
    ("deprecated", Role::Bool),
    ("description", Role::Str),
    ("else", Role::Schema),
    ("enum", Role::Array),
    ("examples", Role::Array),
    ("exclusiveMaximum", Role::Number),
    ("exclusiveMinimum", Role::Number),
    ("format", Role::Str),
    ("if", Role::Schema),
    ("items", Role::Schema),
    ("maxContains", Role::Count),
    ("maxItems", Role::Count),
    ("maxLength", Role::Count),
    ("maxProperties", Role::Count),
    ("maximum", Role::Number),
    ("minContains", Role::Count),
    ("minItems", Role::Count),
    ("minLength", Role::Count),
    ("minProperties", Role::Count),
    ("minimum", Role::Number),
    ("multipleOf", Role::PositiveNumber),
    ("not", Role::Schema),
    ("oneOf", Role::SchemaList),
    ("path_like", Role::Project(ProjectKeyword::PathLike)),
    ("pattern", Role::Str),
    ("patternProperties", Role::PatternMap),
    ("prefixItems", Role::SchemaList),
    ("properties", Role::SchemaMap),
    ("propertyNames", Role::Schema),
    ("readOnly", Role::Bool),
    ("required", Role::Required),
    ("then", Role::Schema),
    ("title", Role::Str),
    ("type", Role::Type),
    ("unevaluatedItems", Role::Schema),
    ("unevaluatedProperties", Role::Schema),
    ("uniqueItems", Role::Bool),
    ("writeOnly", Role::Bool),
    ("x-extensions", Role::Project(ProjectKeyword::XExtensions)),
    ("x-quantity", Role::Project(ProjectKeyword::XQuantity)),
    ("x-ref", Role::Project(ProjectKeyword::XRef)),
    ("x-selector", Role::Project(ProjectKeyword::XSelector)),
    ("x-standards-ref", Role::Project(ProjectKeyword::XStandardsRef)),
    ("x-ui", Role::Project(ProjectKeyword::XUi)),
    ("x-unit", Role::Project(ProjectKeyword::XUnit)),
    ("x-untrusted", Role::Project(ProjectKeyword::XUntrusted)),
];

/// Every keyword of the vocabulary, sorted.
pub fn all_keywords() -> impl Iterator<Item = (&'static str, Role)> {
    KEYWORDS.into_iter()
}

/// The role of a keyword, or `None` when it is outside the vocabulary (case-sensitive).
pub fn lookup(name: &str) -> Option<Role> {
    KEYWORDS.binary_search_by(|(k, _)| (*k).cmp(name)).ok().and_then(|i| KEYWORDS.get(i)).map(|e| e.1)
}

/// The keyword with this exact name, or `None` outside the vocabulary.
pub fn keyword(name: &str) -> Option<Keyword> {
    let role = lookup(name)?;
    Some(match role {
        Role::Project(p) => Keyword::Project(p),
        _ => Keyword::Standard(all_keywords().find(|(k, _)| *k == name).map(|(k, _)| k)?),
    })
}

/// One finding of the checker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaIssue {
    /// Display-safe RFC 6901 pointer into the schema (`""` for the whole document), escaped
    /// and cut like `validate`'s `field`.
    pub pointer: String,
    pub code: SchemaCode,
    /// Printable ASCII, at most 512 bytes; echoed input is cut to 128 bytes and escaped.
    pub message: String,
}

impl SchemaIssue {
    /// The JSON form `{"code", "message", "pointer"}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code.as_str().to_string()));
        o.insert("message".to_string(), CValue::Str(self.message.clone()));
        o.insert("pointer".to_string(), CValue::Str(self.pointer.clone()));
        CValue::Obj(o)
    }
}

impl fmt::Display for SchemaIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {:?}: {}", self.code, self.pointer, self.message)
    }
}

impl std::error::Error for SchemaIssue {}

fn issue(pointer: String, code: SchemaCode, message: String) -> SchemaIssue {
    SchemaIssue { pointer, code, message: cut(&message, KEEP_MESSAGE) }
}

/// Checks a schema document given as bytes: larger than 1 MiB [`SchemaCode::SchemaTooLarge`];
/// then `validate`'s byte pre-scan (nesting above `fid-canon`'s 128 levels
/// [`SchemaCode::SchemaTooDeep`]; a number literal that is not finite, underflows, or exceeds
/// 2^53 [`SchemaCode::BadNumber`]); then `fid-canon` parsing ([`SchemaCode::DuplicateKey`] for
/// an exact duplicate key, else [`SchemaCode::BadJson`]); then [`check_schema`]. Each refusal
/// before [`check_schema`] is the only issue, at pointer `""`.
pub fn check_schema_json(bytes: &[u8], catalogue: &dyn QuantityCatalogue) -> Vec<SchemaIssue> {
    if bytes.len() > SCHEMA_MAX_BYTES {
        let msg = format!("the document has {} bytes, more than {SCHEMA_MAX_BYTES}", bytes.len());
        return vec![issue(String::new(), SchemaCode::SchemaTooLarge, msg)];
    }
    if let Err(e) = prescan(bytes, fid_canon::MAX_DEPTH) {
        let (code, msg) = match e {
            InputError::SchemaTooDeep => {
                (SchemaCode::SchemaTooDeep, format!("the document is nested deeper than {}", fid_canon::MAX_DEPTH))
            }
            InputError::BadNumber(lit) => (SchemaCode::BadNumber, format!("number literal {} is refused", quote(&lit))),
            other => (SchemaCode::BadJson, format!("the document is refused: {}", other.code())),
        };
        return vec![issue(String::new(), code, msg)];
    }
    match fid_canon::parse_with_limits(bytes, SCHEMA_MAX_BYTES, fid_canon::MAX_DEPTH) {
        Ok(v) => check_schema(&v, catalogue),
        Err(e) => {
            let code = if e.code() == "duplicate_key" { SchemaCode::DuplicateKey } else { SchemaCode::BadJson };
            vec![issue(String::new(), code, format!("the document is not accepted JSON ({})", e.code()))]
        }
    }
}

/// Checks one parameter or result schema against the vocabulary and the catalogue
/// (REQ-002, REQ-003, REQ-006 to REQ-009). Returns every issue, sorted by pointer, then
/// code, then message; an empty list means the schema is accepted.
pub fn check_schema(doc: &CValue, catalogue: &dyn QuantityCatalogue) -> Vec<SchemaIssue> {
    let size = canonical_len(doc, SCHEMA_MAX_BYTES);
    if size > SCHEMA_MAX_BYTES {
        let msg = format!("the canonical form is larger than {SCHEMA_MAX_BYTES} bytes");
        return vec![issue(String::new(), SchemaCode::SchemaTooLarge, msg)];
    }
    let CValue::Obj(root) = doc else {
        return vec![issue(String::new(), SchemaCode::BadSchema, "the schema root must be an object".to_string())];
    };
    let mut c = Checker { catalogue, issues: BinaryHeap::new(), overflow: false, refs: Vec::new(), deep: false };
    c.walk(root);
    for (target, at) in std::mem::take(&mut c.refs) {
        if resolve(doc, &target) != Some(Kind::Schema) {
            let msg = format!("$ref {} does not point at a schema position the checker walks", quote(&target));
            c.push(at, SchemaCode::UnwalkedRef, msg);
        }
    }
    c.finish()
}

/// The length of `fid-canon`'s RFC 8785 serialisation of `v`, counted iteratively without
/// building it; counting stops once it exceeds `limit`. A non-finite number counts 0 bytes
/// (the walk reports it).
pub fn canonical_len(v: &CValue, limit: usize) -> usize {
    let mut total = 0usize;
    let mut stack = vec![v];
    while let Some(v) = stack.pop() {
        if total > limit {
            break;
        }
        total = total.saturating_add(match v {
            CValue::Null => 4,
            CValue::Bool(b) => {
                if *b {
                    4
                } else {
                    5
                }
            }
            CValue::Str(s) => str_len(s),
            CValue::I64(i) => i.to_string().len() + 2,
            CValue::U64(u) => u.to_string().len() + 2,
            CValue::F64Bits(_) => 20,
            CValue::Num(n) => fid_canon::format_number(*n).map(|s| s.len()).unwrap_or(0),
            CValue::Arr(a) => {
                stack.extend(a.iter());
                2 + a.len().saturating_sub(1)
            }
            CValue::Obj(m) => {
                let mut n = 2 + m.len().saturating_sub(1);
                for (k, item) in m {
                    n = n.saturating_add(str_len(k) + 1);
                    stack.push(item);
                }
                n
            }
        });
    }
    total
}

/// Bytes of an RFC 8785 string, quotes included.
fn str_len(s: &str) -> usize {
    2 + s
        .chars()
        .map(|c| match c {
            '"' | '\\' | '\u{8}' | '\t' | '\n' | '\u{c}' | '\r' => 2,
            c if (c as u32) < 0x20 => 6,
            c => c.len_utf8(),
        })
        .sum::<usize>()
}

/// Whether `s` matches [`STANDARDS_REF_PATTERN`]: designator `[A-Za-z][A-Za-z0-9.-]{0,15}`,
/// `:`, a four-digit edition year, then optionally ` cl. ` and a clause `[0-9.]{1,16}`.
pub fn is_standards_ref(s: &str) -> bool {
    if s.len() > MAX_STANDARDS_REF || !s.is_ascii() {
        return false;
    }
    let Some((designator, rest)) = s.split_once(':') else { return false };
    let mut d = designator.bytes();
    let designator_ok = designator.len() <= MAX_DESIGNATOR
        && d.next().is_some_and(|c| c.is_ascii_alphabetic())
        && d.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'));
    let (edition, clause) = match rest.split_at_checked(4) {
        Some(parts) => parts,
        None => return false,
    };
    let edition_ok = edition.bytes().all(|c| c.is_ascii_digit());
    let clause_ok = clause.is_empty()
        || clause
            .strip_prefix(" cl. ")
            .is_some_and(|n| (1..=16).contains(&n.len()) && n.bytes().all(|c| c.is_ascii_digit() || c == b'.'));
    designator_ok && edition_ok && clause_ok
}

/// Whether `s` matches [`EXTENSION_PATTERN`]: 1 to 16 lowercase ASCII letters or digits.
pub fn is_extension(s: &str) -> bool {
    (1..=MAX_EXTENSION_BYTES).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// Whether `v` is a valid `x-extensions` value: a non-empty array of at most
/// [`MAX_EXTENSIONS`] strings matching [`EXTENSION_PATTERN`], strictly increasing in byte order
/// (so sorted and unique).
pub fn is_extension_list(v: &CValue) -> bool {
    let CValue::Arr(items) = v else { return false };
    if items.is_empty() || items.len() > MAX_EXTENSIONS {
        return false;
    }
    let mut prev: Option<&str> = None;
    for item in items {
        let CValue::Str(s) = item else { return false };
        if !is_extension(s) || prev.is_some_and(|p| p >= s.as_str()) {
            return false;
        }
        prev = Some(s);
    }
    true
}

/// Whether `path` (the position of a schema object) is a top-level property of the schema,
/// `/properties/<name>`: the only position `path_like` may take (CORE.BUS.POL.FS reads only
/// those).
fn top_level_property(path: &[Seg<'_>]) -> bool {
    matches!(path, [Seg::Key("properties"), Seg::Key(_)])
}

/// Whether `s` is an anchor name (`^[A-Za-z_][-A-Za-z0-9._]*$`, as the 2020-12 core
/// meta-schema), at most 128 bytes.
fn is_anchor(s: &str) -> bool {
    let mut b = s.bytes();
    s.len() <= 128
        && b.next().is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && b.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_'))
}

// ------------------------------------------------------------------ the walk

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Schema,
    SchemaMap,
    PatternMap,
    SchemaList,
    Data,
}

fn child_kind(parent: Kind, seg: Seg<'_>) -> Kind {
    match (parent, seg) {
        (Kind::Schema, Seg::Key(k)) => match lookup(k) {
            Some(Role::Schema) => Kind::Schema,
            Some(Role::SchemaMap) => Kind::SchemaMap,
            Some(Role::PatternMap) => Kind::PatternMap,
            Some(Role::SchemaList) => Kind::SchemaList,
            _ => Kind::Data,
        },
        (Kind::SchemaMap | Kind::PatternMap, Seg::Key(_)) | (Kind::SchemaList, Seg::Idx(_)) => Kind::Schema,
        _ => Kind::Data,
    }
}

/// Follows an RFC 6901 pointer (`/a/b`; `~1` is `/`, `~0` is `~`, any other `~` refuses)
/// from the schema root and returns the kind the walk gives the target, or `None` when it
/// does not exist. Array indices must be canonical decimals.
fn resolve(root: &CValue, target: &str) -> Option<Kind> {
    let (mut cur, mut kind) = (root, Kind::Schema);
    if target.is_empty() {
        return Some(kind);
    }
    let rest = target.strip_prefix('/')?;
    for raw in rest.split('/') {
        let mut key = String::with_capacity(raw.len());
        let mut chars = raw.chars();
        while let Some(c) = chars.next() {
            if c == '~' {
                match chars.next() {
                    Some('0') => key.push('~'),
                    Some('1') => key.push('/'),
                    _ => return None,
                }
            } else {
                key.push(c);
            }
        }
        let (seg, child) = match cur {
            CValue::Obj(m) => (Seg::Key(key.as_str()), m.get(&key)?),
            CValue::Arr(a) => {
                let canonical =
                    key == "0" || (!key.starts_with('0') && !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()));
                let i: usize = if canonical { key.parse().ok()? } else { return None };
                (Seg::Idx(i), a.get(i)?)
            }
            _ => return None,
        };
        kind = child_kind(kind, seg);
        cur = child;
    }
    Some(kind)
}

enum Kids<'a> {
    Obj(btree_map::Iter<'a, String, CValue>),
    Arr(std::iter::Enumerate<std::slice::Iter<'a, CValue>>),
}

struct Frame<'a> {
    kind: Kind,
    depth: usize,
    kids: Kids<'a>,
}

fn order(a: &SchemaIssue, b: &SchemaIssue) -> std::cmp::Ordering {
    (a.pointer.as_str(), a.code.as_str(), a.message.as_str()).cmp(&(
        b.pointer.as_str(),
        b.code.as_str(),
        b.message.as_str(),
    ))
}

/// An issue ordered by [`order`], so a max-heap keeps the largest on top.
struct Ranked(SchemaIssue);

impl PartialEq for Ranked {
    fn eq(&self, other: &Self) -> bool {
        order(&self.0, &other.0).is_eq()
    }
}

impl Eq for Ranked {}

impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        order(&self.0, &other.0)
    }
}

struct Checker<'c> {
    catalogue: &'c dyn QuantityCatalogue,
    /// The [`MAX_ISSUES`] smallest issues so far in [`order`] (a max-heap: the largest kept
    /// issue is on top and is evicted by any smaller one), so memory stays bounded and the
    /// kept set does not depend on the order issues are found in.
    issues: BinaryHeap<Ranked>,
    /// Whether an issue was dropped because [`MAX_ISSUES`] were already kept.
    overflow: bool,
    /// Local `$ref` pointers (after `#`) and where they are, resolved after the walk.
    refs: Vec<(String, String)>,
    /// Whether `schema_too_deep` was reported (once per document).
    deep: bool,
}

impl Checker<'_> {
    fn push(&mut self, pointer: String, code: SchemaCode, message: String) {
        let new = Ranked(issue(pointer, code, message));
        if self.issues.len() < MAX_ISSUES {
            self.issues.push(new);
            return;
        }
        self.overflow = true;
        if self.issues.peek().is_some_and(|top| new < *top) {
            self.issues.pop();
            self.issues.push(new);
        }
    }

    fn finish(self) -> Vec<SchemaIssue> {
        let mut out: Vec<SchemaIssue> = self.issues.into_sorted_vec().into_iter().map(|r| r.0).collect();
        if self.overflow {
            let msg = format!("more than {MAX_ISSUES} issues; the rest are not reported");
            out.push(issue(String::new(), SchemaCode::TooManyIssues, msg));
            out.sort_by(order);
        }
        out
    }

    /// Pre-order walk with an explicit stack; containers deeper than [`SCHEMA_MAX_DEPTH`] are
    /// reported once and not entered.
    fn walk(&mut self, root: &BTreeMap<String, CValue>) {
        let mut path: Vec<Seg<'_>> = Vec::new();
        self.case_check(root, &path);
        self.schema_object(root, &path);
        let mut stack: Vec<Frame<'_>> = vec![Frame { kind: Kind::Schema, depth: 1, kids: Kids::Obj(root.iter()) }];
        while let Some(top) = stack.last_mut() {
            let (parent, depth) = (top.kind, top.depth);
            let next = match &mut top.kids {
                Kids::Obj(it) => it.next().map(|(k, v)| (Seg::Key(k.as_str()), v)),
                Kids::Arr(it) => it.next().map(|(i, v)| (Seg::Idx(i), v)),
            };
            let Some((seg, child)) = next else {
                stack.pop();
                path.pop();
                continue;
            };
            let kind = child_kind(parent, seg);
            path.push(seg);
            let kids = match child {
                CValue::Obj(m) => Some(Kids::Obj(m.iter())),
                CValue::Arr(a) => Some(Kids::Arr(a.iter().enumerate())),
                _ => None,
            };
            if kids.is_some() && depth + 1 > SCHEMA_MAX_DEPTH {
                if !self.deep {
                    self.deep = true;
                    let msg = format!("nested deeper than {SCHEMA_MAX_DEPTH} levels; not checked further");
                    self.push(pointer(&path), SchemaCode::SchemaTooDeep, msg);
                }
                path.pop();
                continue;
            }
            self.visit(child, kind, &path);
            match kids {
                Some(kids) => stack.push(Frame { kind, depth: depth + 1, kids }),
                None => {
                    path.pop();
                }
            }
        }
    }

    fn visit(&mut self, v: &CValue, kind: Kind, path: &[Seg<'_>]) {
        if let CValue::Num(n) | CValue::F64Bits(n) = v {
            if !n.is_finite() {
                self.push(pointer(path), SchemaCode::BadNumber, "the number is not finite".to_string());
            } else if n.is_subnormal() {
                self.push(pointer(path), SchemaCode::BadNumber, format!("the number {n:e} is subnormal"));
            }
        }
        match (kind, v) {
            (Kind::Schema, CValue::Obj(m)) => {
                self.case_check(m, path);
                self.schema_object(m, path);
            }
            (Kind::Schema, CValue::Bool(_)) => {}
            (Kind::Schema, _) => {
                self.push(pointer(path), SchemaCode::BadSchema, "a schema must be an object or a boolean".to_string())
            }
            (Kind::SchemaMap, CValue::Obj(m)) => {
                self.case_check(m, path);
                self.stray_path_like(m, path);
                for k in m.keys() {
                    self.check_name(k, pointer(path));
                }
            }
            (Kind::PatternMap, CValue::Obj(m)) => self.stray_path_like(m, path),
            (Kind::SchemaMap | Kind::PatternMap, _) => {
                self.push(pointer(path), SchemaCode::BadSchema, "must be an object of schemas".to_string())
            }
            (Kind::SchemaList, CValue::Arr(a)) if !a.is_empty() => {}
            (Kind::SchemaList, _) => {
                self.push(pointer(path), SchemaCode::BadSchema, "must be a non-empty array of schemas".to_string())
            }
            (Kind::Data, CValue::Obj(m)) => {
                self.case_check(m, path);
                self.stray_path_like(m, path);
            }
            _ => {}
        }
    }

    /// A `"path_like": true` member of an object that is not a schema (a map of schemas, or
    /// data such as `const`, `default` or `x-ui`). CORE.BUS.POL.FS counts every such member
    /// and refuses a command with one anywhere but on a top-level property, so the checker
    /// refuses it too [`SchemaCode::MisplacedPathLike`].
    fn stray_path_like(&mut self, m: &BTreeMap<String, CValue>, path: &[Seg<'_>]) {
        if m.get("path_like") == Some(&CValue::Bool(true)) {
            let mut p = path.to_vec();
            p.push(Seg::Key("path_like"));
            let msg = "\"path_like\": true outside a schema position".to_string();
            self.push(pointer(&p), SchemaCode::MisplacedPathLike, msg);
        }
    }

    /// Keys of one object that are equal after ASCII lowercasing.
    fn case_check(&mut self, m: &BTreeMap<String, CValue>, path: &[Seg<'_>]) {
        let mut seen: BTreeMap<String, &str> = BTreeMap::new();
        for k in m.keys() {
            let low = k.to_ascii_lowercase();
            if let Some(prev) = seen.get(&low) {
                let msg = format!("keys {} and {} differ only in ASCII case", quote(prev), quote(k));
                self.push(pointer(path), SchemaCode::DuplicateKey, msg);
            } else {
                seen.insert(low, k);
            }
        }
    }

    /// A property or definition name: non-empty printable ASCII without space.
    fn check_name(&mut self, name: &str, at: String) {
        if name.is_empty() || !name.bytes().all(|b| (b'!'..=b'~').contains(&b)) {
            let msg = format!("name {} is not printable ASCII without space", quote(name));
            self.push(at, SchemaCode::BadPropertyName, msg);
        }
    }

    /// An array of distinct names.
    fn name_list(&mut self, v: &CValue, at: String, what: &str) {
        let mut seen = BTreeSet::new();
        let ok = matches!(v, CValue::Arr(items) if items.iter().all(|it| matches!(it, CValue::Str(s) if seen.insert(s.as_str()))));
        if !ok {
            self.push(at, SchemaCode::BadSchema, format!("{what} must be an array of distinct strings"));
            return;
        }
        for name in seen {
            self.check_name(name, at.clone());
        }
    }

    /// The keyword checks of one schema object.
    fn schema_object(&mut self, m: &BTreeMap<String, CValue>, path: &[Seg<'_>]) {
        let kp = |k: &str| {
            let mut p = path.to_vec();
            p.push(Seg::Key(k));
            pointer(&p)
        };
        for (k, v) in m {
            let Some(role) = lookup(k) else {
                if OLD_DRAFT_KEYWORDS.contains(&k.as_str()) || k == "definitions" {
                    let msg = format!("{} belongs to an earlier JSON Schema draft and is not supported", quote(k));
                    self.push(kp(k), SchemaCode::BadDialect, msg);
                } else {
                    let msg = format!("keyword {} is not in the registry vocabulary", quote(k));
                    self.push(kp(k), SchemaCode::UnknownKeyword, msg);
                }
                continue;
            };
            let bad = |c: &mut Self, why: String| c.push(kp(k), SchemaCode::BadSchema, why);
            match role {
                // Walked; their shape is checked when visited.
                Role::Schema | Role::SchemaMap | Role::PatternMap | Role::SchemaList | Role::Any => {}
                Role::Type => {
                    let mut seen = BTreeSet::new();
                    let ok = match v {
                        CValue::Str(s) => TYPES.contains(&s.as_str()),
                        CValue::Arr(items) => {
                            !items.is_empty()
                                && items.iter().all(
                                    |it| matches!(it, CValue::Str(s) if TYPES.contains(&s.as_str()) && seen.insert(s.as_str())),
                                )
                        }
                        _ => false,
                    };
                    if !ok {
                        bad(
                            self,
                            format!("type must be one of {} or a non-empty array of distinct ones", TYPES.join(", ")),
                        );
                    }
                }
                Role::Number => {
                    if number(v).is_none() {
                        bad(self, format!("{k} must be a number (integers at most 2^53 in magnitude)"));
                    }
                }
                Role::PositiveNumber => {
                    if !number(v).is_some_and(|n| n > 0.0) {
                        bad(self, format!("{k} must be a positive number"));
                    }
                }
                Role::Count => {
                    if !number(v).is_some_and(|n| n.fract() == 0.0 && (0.0..=MAX_COUNT).contains(&n)) {
                        bad(self, format!("{k} must be an integer 0..=2^53"));
                    }
                }
                Role::Bool => {
                    if !matches!(v, CValue::Bool(_)) {
                        bad(self, format!("{k} must be a boolean"));
                    }
                }
                Role::Str => {
                    if !matches!(v, CValue::Str(_)) {
                        bad(self, format!("{k} must be a string"));
                    }
                }
                Role::Array => {
                    if !matches!(v, CValue::Arr(_)) {
                        bad(self, format!("{k} must be an array"));
                    }
                }
                Role::Required => self.name_list(v, kp(k), "required"),
                Role::DependentRequired => match v {
                    CValue::Obj(dr) => {
                        for (name, list) in dr {
                            self.check_name(name, kp(k));
                            let mut p = path.to_vec();
                            p.push(Seg::Key(k));
                            p.push(Seg::Key(name));
                            self.name_list(list, pointer(&p), "a dependentRequired value");
                        }
                    }
                    _ => bad(self, "dependentRequired must be an object of name arrays".to_string()),
                },
                Role::Dialect => {
                    let msg = match v {
                        CValue::Str(u) if path.is_empty() && u == SCHEMA_DIALECT => None,
                        CValue::Str(u) if path.is_empty() => {
                            Some(format!("$schema {} is not {SCHEMA_DIALECT}", quote(u)))
                        }
                        CValue::Str(_) => Some("$schema is accepted only at the schema root".to_string()),
                        _ => Some("$schema must be a string".to_string()),
                    };
                    if let Some(msg) = msg {
                        self.push(kp(k), SchemaCode::BadDialect, msg);
                    }
                }
                Role::Id => match v {
                    _ if !path.is_empty() => {
                        let msg = "an embedded $id changes how references resolve".to_string();
                        self.push(kp(k), SchemaCode::UnwalkedRef, msg);
                    }
                    CValue::Str(s) if !s.contains('#') => {}
                    _ => bad(self, "$id must be a string without a fragment".to_string()),
                },
                Role::Anchor => {
                    if !matches!(v, CValue::Str(s) if is_anchor(s)) {
                        bad(self, format!("{k} must match ^[A-Za-z_][-A-Za-z0-9._]*$"));
                    }
                }
                Role::Vocabulary => {
                    if !matches!(v, CValue::Obj(o) if o.values().all(|b| matches!(b, CValue::Bool(_)))) {
                        bad(self, "$vocabulary must be an object of booleans".to_string());
                    }
                }
                Role::Ref => match v {
                    CValue::Str(r) if !r.starts_with('#') => {
                        let msg = format!("$ref {} leaves the schema", quote(r));
                        self.push(kp(k), SchemaCode::ExternalRef, msg);
                    }
                    CValue::Str(r) if r == "#" => {}
                    CValue::Str(r) if r.starts_with("#/") && !r.contains('%') => {
                        let target = r.get(1..).unwrap_or_default().to_string();
                        self.refs.push((target, kp(k)));
                    }
                    CValue::Str(r) => {
                        let msg = format!("$ref {} is a plain-name or percent-encoded reference", quote(r));
                        self.push(kp(k), SchemaCode::UnwalkedRef, msg);
                    }
                    _ => bad(self, "$ref must be a string".to_string()),
                },
                Role::DynamicRef => {
                    let msg = "$dynamicRef is not supported: its target is resolved at evaluation time".to_string();
                    self.push(kp(k), SchemaCode::UnwalkedRef, msg);
                }
                Role::Project(p) => self.project(p, v, m, path, kp(k)),
            }
        }
    }

    /// The project keywords (REQ-003, REQ-006 to REQ-008, REQ-010); `path` is the position of
    /// the schema object `m`.
    fn project(&mut self, p: ProjectKeyword, v: &CValue, m: &BTreeMap<String, CValue>, path: &[Seg<'_>], at: String) {
        let kv = SchemaCode::BadKeywordValue;
        match p {
            ProjectKeyword::PathLike => {
                if v != &CValue::Bool(true) {
                    self.push(at, kv, "path_like must be true".to_string());
                } else if !top_level_property(path) {
                    let msg = "path_like is allowed only on a top-level property (/properties/<name>)".to_string();
                    self.push(at, SchemaCode::MisplacedPathLike, msg);
                } else if m.get("type") != Some(&CValue::Str("string".to_string())) {
                    self.push(at, SchemaCode::MisplacedPathLike, "path_like needs type string".to_string());
                }
            }
            ProjectKeyword::XExtensions => {
                if !is_extension_list(v) {
                    let msg = format!(
                        "x-extensions must be a non-empty, sorted array of at most {MAX_EXTENSIONS} distinct strings \
                         matching {EXTENSION_PATTERN}"
                    );
                    self.push(at, kv, msg);
                } else if !m.contains_key("path_like") {
                    let msg = "x-extensions needs a sibling path_like: true".to_string();
                    self.push(at, SchemaCode::MisplacedPathLike, msg);
                }
            }
            ProjectKeyword::XQuantity => match v {
                CValue::Str(q) if self.catalogue.kind(q).is_some() => {}
                CValue::Str(q) => {
                    let msg = format!("x-quantity {} is not a kind of the unit catalogue", quote(q));
                    self.push(at, SchemaCode::UnknownQuantity, msg);
                }
                _ => self.push(at, kv, "x-quantity must be a string".to_string()),
            },
            ProjectKeyword::XUnit => match v {
                CValue::Str(u) => match m.get("x-quantity") {
                    Some(CValue::Str(q)) if self.catalogue.kind(q).is_some() => {
                        if !self.catalogue.accepts(q, u) {
                            let msg = format!("x-unit {} is not an accepted unit of {}", quote(u), quote(q));
                            self.push(at, SchemaCode::UnitNotAccepted, msg);
                        }
                    }
                    // An invalid x-quantity is already reported.
                    Some(_) => {}
                    None => self.push(at, SchemaCode::UnitNotAccepted, "x-unit needs a sibling x-quantity".to_string()),
                },
                _ => self.push(at, kv, "x-unit must be a string".to_string()),
            },
            ProjectKeyword::XRef => {
                if !matches!(v, CValue::Str(s) if EntityType::parse(s).is_some()) {
                    let all: Vec<&str> = EntityType::ALL.iter().map(|e| e.as_str()).collect();
                    self.push(at, kv, format!("x-ref must be one of {}", all.join(", ")));
                }
            }
            ProjectKeyword::XSelector => {
                if !matches!(v, CValue::Str(s) if SelectorForm::parse(s).is_some()) {
                    let all: Vec<&str> = SelectorForm::ALL.iter().map(|e| e.as_str()).collect();
                    self.push(at, kv, format!("x-selector must be one of {}", all.join(", ")));
                }
            }
            ProjectKeyword::XStandardsRef => {
                if !matches!(v, CValue::Str(s) if is_standards_ref(s)) {
                    let msg = format!("x-standards-ref must be a string matching {STANDARDS_REF_PATTERN}");
                    self.push(at, SchemaCode::BadStandardsRef, msg);
                }
            }
            ProjectKeyword::XUi => {
                if !matches!(v, CValue::Obj(_)) {
                    self.push(at, kv, "x-ui must be an object of presentation hints".to_string());
                }
            }
            ProjectKeyword::XUntrusted => {
                if !matches!(v, CValue::Bool(_)) {
                    self.push(at, kv, "x-untrusted must be a boolean".to_string());
                } else if !string_typed(m.get("type")) {
                    let msg = "x-untrusted needs type string (or an array of string and null)".to_string();
                    self.push(at, SchemaCode::UntrustedOnNonString, msg);
                }
            }
        }
    }
}

/// Whether a `type` value is `"string"`, or an array holding `"string"` and otherwise only
/// `"null"`.
fn string_typed(t: Option<&CValue>) -> bool {
    match t {
        Some(CValue::Str(s)) => s == "string",
        Some(CValue::Arr(items)) => {
            items.iter().any(|i| matches!(i, CValue::Str(s) if s == "string"))
                && items.iter().all(|i| matches!(i, CValue::Str(s) if s == "string" || s == "null"))
        }
        _ => false,
    }
}

// ------------------------------------------------------------------ meta-schema

fn s(x: &str) -> CValue {
    CValue::Str(x.to_string())
}

fn obj<const N: usize>(items: [(&str, CValue); N]) -> CValue {
    CValue::Obj(items.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|x| s(x)).collect())
}

/// The meta-schema `registry-keywords.schema.json` (REQ-001): the 2020-12 meta-schema plus
/// the nine project keywords, with `unevaluatedProperties: false` so that no other keyword
/// is admitted, the deprecated 2019-09 keywords and `$dynamicRef` refused, and `$ref`
/// limited to `#...` (REQ-002). Checked in as canonical JSON plus a newline.
pub fn registry_keywords_schema() -> CValue {
    let entity: Vec<&str> = EntityType::ALL.iter().map(|e| e.as_str()).collect();
    let selector: Vec<&str> = SelectorForm::ALL.iter().map(|e| e.as_str()).collect();
    let f = CValue::Bool(false);
    let string_types = obj([
        ("type", s("array")),
        ("items", obj([("enum", strs(&["string", "null"]))])),
        ("contains", obj([("const", s("string"))])),
        ("uniqueItems", CValue::Bool(true)),
    ]);
    obj([
        ("$schema", s(SCHEMA_DIALECT)),
        ("$id", s(REGISTRY_KEYWORDS_ID)),
        ("$dynamicAnchor", s("meta")),
        ("$comment", s(&format!("Metrology Maven registry keyword vocabulary, version {REGISTRY_KEYWORDS_VERSION} (CORE.REG.DEF.SCHEMA)"))),
        ("title", s("Metrology Maven parameter and result schema")),
        ("type", strs(&["object", "boolean"])),
        ("allOf", CValue::Arr(vec![obj([("$ref", s(SCHEMA_DIALECT))])])),
        (
            "properties",
            obj([
                ("$schema", obj([("const", s(SCHEMA_DIALECT))])),
                ("$ref", obj([("type", s("string")), ("pattern", s("^#"))])),
                ("$dynamicRef", f.clone()),
                ("$recursiveAnchor", f.clone()),
                ("$recursiveRef", f.clone()),
                ("definitions", f.clone()),
                ("dependencies", f),
                ("path_like", obj([("const", CValue::Bool(true))])),
                (
                    "x-extensions",
                    obj([
                        ("type", s("array")),
                        ("minItems", CValue::Num(1.0)),
                        ("maxItems", CValue::Num(64.0)),
                        ("uniqueItems", CValue::Bool(true)),
                        ("items", obj([("type", s("string")), ("pattern", s(EXTENSION_PATTERN))])),
                    ]),
                ),
                ("x-quantity", obj([("type", s("string")), ("pattern", s("^[a-z][a-z0-9_]{0,31}$"))])),
                ("x-unit", obj([("type", s("string")), ("pattern", s("^[!-~]{1,16}$"))])),
                ("x-ref", obj([("enum", strs(&entity))])),
                ("x-selector", obj([("enum", strs(&selector))])),
                (
                    "x-standards-ref",
                    obj([
                        ("type", s("string")),
                        ("maxLength", CValue::Num(64.0)),
                        ("pattern", s(STANDARDS_REF_PATTERN)),
                    ]),
                ),
                ("x-ui", obj([("type", s("object"))])),
                ("x-untrusted", obj([("type", s("boolean"))])),
            ]),
        ),
        (
            "dependentSchemas",
            obj([
                (
                    "path_like",
                    obj([("required", strs(&["type"])), ("properties", obj([("type", obj([("const", s("string"))]))]))]),
                ),
                ("x-extensions", obj([("required", strs(&["path_like"]))])),
                ("x-unit", obj([("required", strs(&["x-quantity"]))])),
                (
                    "x-untrusted",
                    obj([
                        ("required", strs(&["type"])),
                        (
                            "properties",
                            obj([(
                                "type",
                                obj([("anyOf", CValue::Arr(vec![obj([("const", s("string"))]), string_types]))]),
                            )]),
                        ),
                    ]),
                ),
            ]),
        ),
        ("unevaluatedProperties", CValue::Bool(false)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_table_is_sorted_and_unique() {
        let names: Vec<&str> = all_keywords().map(|(k, _)| k).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names, sorted);
        assert_eq!(names.len(), 66);
        for (k, _) in all_keywords() {
            assert!(lookup(k).is_some(), "{k}");
        }
        let project: Vec<&str> = all_keywords()
            .filter_map(|(_, r)| match r {
                Role::Project(p) => Some(p.as_str()),
                _ => None,
            })
            .collect();
        let want: Vec<&str> = ProjectKeyword::ALL.iter().map(|p| p.as_str()).collect();
        assert_eq!(project, want);
    }

    #[test]
    fn closed_lists_are_sorted() {
        for list in [
            SchemaCode::ALL.iter().map(|c| c.as_str()).collect::<Vec<_>>(),
            EntityType::ALL.iter().map(|c| c.as_str()).collect(),
            SelectorForm::ALL.iter().map(|c| c.as_str()).collect(),
            ProjectKeyword::ALL.iter().map(|c| c.as_str()).collect(),
        ] {
            let mut sorted = list.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(list, sorted);
        }
    }

    #[test]
    fn lookups() {
        assert_eq!(lookup("x-ui"), Some(Role::Project(ProjectKeyword::XUi)));
        assert_eq!(lookup("X-ui"), None);
        assert_eq!(lookup("definitions"), None);
        assert_eq!(lookup("additionalItems"), None);
        assert_eq!(keyword("type"), Some(Keyword::Standard("type")));
        assert_eq!(keyword("x-unit"), Some(Keyword::Project(ProjectKeyword::XUnit)));
        assert_eq!(keyword("x-unit").map(Keyword::as_str), Some("x-unit"));
    }

    #[test]
    fn standards_refs() {
        for ok in [
            "ISO1101:2017",
            "ISO-1101:2017 cl. 8.2",
            "ASME-Y14.5:2018",
            "iso.14405-1:2016",
            "V:2020",
            "ISO-GUM-98-3:2008 cl. 4.1.2",
        ] {
            assert!(is_standards_ref(ok), "{ok}");
        }
        for bad in [
            "",
            "ISO 1101:2017",
            "ISO/IEC-98:2008",
            "1SO-1101:2017",
            "-ISO:2017",
            ":2017",
            "ISO_1101:2017",
            "ISO-1101:17",
            "ISO-1101:2017 clause 8",
            "ISO-1101:2017 cl. ",
            "ISO-1101:2017 cl. 8.2 and some text",
            "ISO-1101:2017 cl. 12345678901234567",
            "ISO-1101",
            "ISO-1101:2017:2018",
            "ISO-1101:20\u{e1}7",
        ] {
            assert!(!is_standards_ref(bad), "{bad}");
        }
        assert!(is_standards_ref(&format!("A{}:2017 cl. {}", "B".repeat(15), "1".repeat(16))));
        assert!(!is_standards_ref(&format!("A{}:2017", "B".repeat(16))));
    }

    #[test]
    fn extensions() {
        for ok in ["stl", "step", "3mf", "a", "abcdefghijklmnop"] {
            assert!(is_extension(ok), "{ok}");
        }
        for bad in ["", "STL", ".stl", "st l", "st-l", "\u{17f}tl", "abcdefghijklmnopq"] {
            assert!(!is_extension(bad), "{bad}");
        }
        let list = |xs: &[&str]| CValue::Arr(xs.iter().map(|x| s(x)).collect());
        assert!(is_extension_list(&list(&["step", "stl"])));
        assert!(!is_extension_list(&list(&[])));
        assert!(!is_extension_list(&list(&["stl", "step"])));
        assert!(!is_extension_list(&list(&["stl", "stl"])));
        let max: Vec<String> = (0..MAX_EXTENSIONS).map(|i| format!("e{i:03}")).collect();
        let max: Vec<&str> = max.iter().map(String::as_str).collect();
        assert!(is_extension_list(&list(&max)));
        let over: Vec<String> = (0..=MAX_EXTENSIONS).map(|i| format!("e{i:03}")).collect();
        let over: Vec<&str> = over.iter().map(String::as_str).collect();
        assert!(!is_extension_list(&list(&over)));
        assert!(!is_extension_list(&s("stl")));
        // The meta-schema writes the limit as a literal.
        assert_eq!(MAX_EXTENSIONS, 64);
    }

    #[test]
    fn canonical_len_matches_fid_canon() {
        let doc = obj([
            ("a", s("q\"\\\u{1}\u{e9}\n")),
            ("b", CValue::Arr(vec![CValue::Num(1.5), CValue::Num(-0.0), CValue::Null, CValue::Bool(false)])),
            ("c", CValue::I64(-42)),
            ("d", CValue::U64(7)),
            ("e", CValue::F64Bits(2.0)),
            ("f", obj([])),
            ("g", CValue::Arr(vec![])),
            ("h", CValue::Bool(true)),
        ]);
        let want = fid_canon::to_canonical_bytes(&doc).map(|b| b.len()).unwrap_or(0);
        assert_eq!(canonical_len(&doc, usize::MAX), want);
    }
}

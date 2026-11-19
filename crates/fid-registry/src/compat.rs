//! Compatibility classifier and manifest diff. Spec node `CORE.REG.VER.COMPAT`.
//!
//! [`classify`] compares two versions of one command definition field by field and
//! returns one [`Change`] per changed JSON pointer. It fails closed: a change is
//! [`Kind::Additive`] only when a rule below proves it so (REQ-001); everything else,
//! including any field or schema keyword without a rule, is [`Kind::Breaking`]
//! (REQ-002, REQ-005).
//!
//! Additive changes:
//! - `title`, `description`, `web`, `web_note`, `cli_bare` of the definition;
//! - `title`, `description` and `x-ui` keywords anywhere in a parameter or result schema;
//! - a new result field (a new name under `properties` of a result schema object);
//! - a new optional parameter (a new name under `properties` of a parameter schema object)
//!   that is not `required`, declares a `default` and sits in an object with
//!   `additionalProperties: false`. In an open object an old caller may already have sent
//!   that name, so the addition is not provably additive (`param_added_to_open_object`).
//!
//! An additive verdict inside a schema object that uses a keyword without a rule (for
//! example `if`/`then`/`else`, `oneOf`, `$ref`, `patternProperties`), changed or not, is
//! turned into `unclassified_construct`: the unknown keyword may constrain the addition.
//!
//! Lifecycle metadata is not classified here: [`diff`] lists lifecycle changes separately,
//! and their legality is the job of `CORE.REG.VER.LIFE`.

use crate::def::{CommandDef, DefError};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Deepest schema nesting the classifier walks; deeper changes are `unclassified_construct`.
/// Matches the depth limit of `CORE.REG.DEF.SCHEMA`.
pub const MAX_SCHEMA_DEPTH: usize = 64;

/// Whether a change keeps old callers and readers working.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Additive,
    Breaking,
}

impl Kind {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Additive => "additive",
            Kind::Breaking => "breaking",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

macro_rules! reasons {
    ($($variant:ident => $s:literal, $kind:ident;)+) => {
        /// Why a change got its classification. Each reason has exactly one [`Kind`].
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Reason {
            $($variant,)+
        }

        impl Reason {
            /// Every reason, in declaration order.
            pub const ALL: &'static [Reason] = &[$(Reason::$variant),+];

            /// Stable code.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Reason::$variant => $s,)+
                }
            }

            /// The classification this reason implies.
            pub fn kind(self) -> Kind {
                match self {
                    $(Reason::$variant => Kind::$kind,)+
                }
            }
        }
    };
}

reasons! {
    DocChanged => "doc_changed", Additive;
    UiHintChanged => "ui_hint_changed", Additive;
    WebChanged => "web_changed", Additive;
    CliBareChanged => "cli_bare_changed", Additive;
    OptionalParamAdded => "optional_param_added", Additive;
    ResultFieldAdded => "result_field_added", Additive;
    IdentityChanged => "identity_changed", Breaking;
    ParamRemoved => "param_removed", Breaking;
    ResultFieldRemoved => "result_field_removed", Breaking;
    RequiredParamAdded => "required_param_added", Breaking;
    OptionalParamWithoutDefault => "optional_param_without_default", Breaking;
    ParamAddedToOpenObject => "param_added_to_open_object", Breaking;
    ParamMadeRequired => "param_made_required", Breaking;
    RequiredChanged => "required_changed", Breaking;
    TypeChanged => "type_changed", Breaking;
    QuantityChanged => "quantity_changed", Breaking;
    UnitChanged => "unit_changed", Breaking;
    RefChanged => "ref_changed", Breaking;
    EnumNarrowed => "enum_narrowed", Breaking;
    EnumWidened => "enum_widened", Breaking;
    EnumChanged => "enum_changed", Breaking;
    RangeNarrowed => "range_narrowed", Breaking;
    RangeWidened => "range_widened", Breaking;
    DefaultChanged => "default_changed", Breaking;
    ConstraintChanged => "constraint_changed", Breaking;
    PreconditionChanged => "precondition_changed", Breaking;
    SideEffectChanged => "side_effect_changed", Breaking;
    McpProjectionChanged => "mcp_projection_changed", Breaking;
    DeterminismChanged => "determinism_changed", Breaking;
    IdempotencyChanged => "idempotency_changed", Breaking;
    FsEffectChanged => "fs_effect_changed", Breaking;
    UndoChanged => "undo_changed", Breaking;
    NetworkChanged => "network_changed", Breaking;
    HumanOnlyChanged => "human_only_changed", Breaking;
    EffectsChanged => "effects_changed", Breaking;
    AlgorithmsChanged => "algorithms_changed", Breaking;
    ExposureChanged => "exposure_changed", Breaking;
    UnclassifiedConstruct => "unclassified_construct", Breaking;
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A definition key: command ID and major. Orders by ID, then major (numerically).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandRef {
    pub id: String,
    pub major: u32,
}

impl CommandRef {
    /// The key of a definition.
    pub fn of(def: &CommandDef) -> CommandRef {
        CommandRef { id: def.id.clone(), major: def.major }
    }

    fn put(&self, o: &mut BTreeMap<String, CValue>) {
        o.insert("command".into(), CValue::Str(self.id.clone()));
        o.insert("major".into(), CValue::Num(f64::from(self.major)));
    }

    /// `{"command": id, "major": n}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        self.put(&mut o);
        CValue::Obj(o)
    }
}

/// One classified change (REQ-003).
///
/// `pointer` is an RFC 6901 JSON pointer into the definition's JSON form
/// ([`CommandDef::to_value`]). A digest is the SHA-256 of the RFC 8785 form of the value
/// at that pointer; `None` when the value is absent on that side.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Change {
    pub command: CommandRef,
    pub pointer: String,
    pub old_digest: Option<Digest>,
    pub new_digest: Option<Digest>,
    pub kind: Kind,
    pub reason: Reason,
}

fn digest_value(d: Option<Digest>) -> CValue {
    match d {
        Some(d) => CValue::Str(d.as_str().to_string()),
        None => CValue::Null,
    }
}

impl Change {
    /// `{command, major, pointer, old_digest, new_digest, kind, reason}`; absent digests are `null`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        self.command.put(&mut o);
        o.insert("pointer".into(), CValue::Str(self.pointer.clone()));
        o.insert("old_digest".into(), digest_value(self.old_digest));
        o.insert("new_digest".into(), digest_value(self.new_digest));
        o.insert("kind".into(), CValue::Str(self.kind.as_str().into()));
        o.insert("reason".into(), CValue::Str(self.reason.as_str().into()));
        CValue::Obj(o)
    }
}

/// A change of the `lifecycle` object of one (ID, major).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LifecycleChange {
    pub command: CommandRef,
    /// The `state` member on each side, if it is a string.
    pub old_state: Option<String>,
    pub new_state: Option<String>,
    pub old_digest: Option<Digest>,
    pub new_digest: Option<Digest>,
}

impl LifecycleChange {
    /// `{command, major, pointer: "/lifecycle", old_state, new_state, old_digest, new_digest}`.
    pub fn to_value(&self) -> CValue {
        let state = |s: &Option<String>| s.as_ref().map_or(CValue::Null, |s| CValue::Str(s.clone()));
        let mut o = BTreeMap::new();
        self.command.put(&mut o);
        o.insert("pointer".into(), CValue::Str("/lifecycle".into()));
        o.insert("old_state".into(), state(&self.old_state));
        o.insert("new_state".into(), state(&self.new_state));
        o.insert("old_digest".into(), digest_value(self.old_digest));
        o.insert("new_digest".into(), digest_value(self.new_digest));
        CValue::Obj(o)
    }
}

/// The difference between two manifests (REQ-004). Every list is sorted by ID, then
/// major, then pointer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestDiff {
    pub added: Vec<CommandRef>,
    pub removed: Vec<CommandRef>,
    pub lifecycle_changed: Vec<LifecycleChange>,
    pub changed: Vec<Change>,
}

impl ManifestDiff {
    /// True when the manifests hold the same definitions (REQ-006).
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.lifecycle_changed.is_empty() && self.changed.is_empty()
    }

    /// Number of entries over all four lists.
    pub fn len(&self) -> usize {
        self.added.len() + self.removed.len() + self.lifecycle_changed.len() + self.changed.len()
    }

    /// True when any change is breaking. Removing a definition counts as breaking.
    pub fn is_breaking(&self) -> bool {
        !self.removed.is_empty() || self.changed.iter().any(|c| c.kind == Kind::Breaking)
    }

    /// `{added, removed, lifecycle_changed, changed}`; serialise with `fid-canon` for
    /// byte-identical output.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("added".into(), CValue::Arr(self.added.iter().map(CommandRef::to_value).collect()));
        o.insert("removed".into(), CValue::Arr(self.removed.iter().map(CommandRef::to_value).collect()));
        o.insert(
            "lifecycle_changed".into(),
            CValue::Arr(self.lifecycle_changed.iter().map(LifecycleChange::to_value).collect()),
        );
        o.insert("changed".into(), CValue::Arr(self.changed.iter().map(Change::to_value).collect()));
        CValue::Obj(o)
    }

    /// RFC 8785 bytes of [`ManifestDiff::to_value`].
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, ManifestError> {
        to_canonical_bytes(&self.to_value()).map_err(|e| ManifestError(format!("diff: {}", e.code())))
    }
}

// ------------------------------------------------------------------ classifier

fn digest_of(v: Option<&CValue>) -> Option<Digest> {
    // A value that cannot be canonicalised (a non-finite number, only possible in memory)
    // has no digest; the change itself is still reported.
    v.and_then(|v| fid_canon::digest(v).ok()).map(|d| Digest::from_bytes(d.0))
}

/// RFC 6901 escaping of one reference token.
fn token(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

struct Rec {
    pointer: String,
    old: Option<Digest>,
    new: Option<Digest>,
    reason: Reason,
}

fn push(out: &mut Vec<Rec>, pointer: String, old: Option<&CValue>, new: Option<&CValue>, reason: Reason) {
    out.push(Rec { pointer, old: digest_of(old), new: digest_of(new), reason });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    /// Parameter schema: the command reads it.
    Input,
    /// Result schema: callers read it.
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rule {
    Doc,
    Ui,
    Type,
    Quantity,
    Unit,
    Ref,
    Enum,
    Lower,
    Upper,
    Default,
    Properties,
    Required,
    Items,
    Constraint,
    Unknown,
}

/// The rule table for schema keywords. Keywords not listed have no rule.
fn keyword_rule(k: &str) -> Rule {
    match k {
        "title" | "description" => Rule::Doc,
        "x-ui" => Rule::Ui,
        "type" => Rule::Type,
        "x-quantity" => Rule::Quantity,
        "x-unit" => Rule::Unit,
        "x-ref" => Rule::Ref,
        "enum" => Rule::Enum,
        "minimum" | "exclusiveMinimum" | "minLength" | "minItems" | "minProperties" => Rule::Lower,
        "maximum" | "exclusiveMaximum" | "maxLength" | "maxItems" | "maxProperties" => Rule::Upper,
        "default" => Rule::Default,
        "properties" => Rule::Properties,
        "required" => Rule::Required,
        "items" => Rule::Items,
        "const"
        | "pattern"
        | "format"
        | "multipleOf"
        | "uniqueItems"
        | "additionalProperties"
        | "x-selector"
        | "x-standards-ref"
        | "x-untrusted"
        | "$schema"
        | "$comment"
        | "examples" => Rule::Constraint,
        _ => Rule::Unknown,
    }
}

fn number(v: &CValue) -> Option<f64> {
    match v {
        CValue::Num(n) => Some(*n),
        CValue::I64(i) => Some(*i as f64),
        CValue::U64(u) => Some(*u as f64),
        _ => None,
    }
}

/// Numeric bounds compared by direction. `None` means no effective change.
fn bound_reason(old: Option<&CValue>, new: Option<&CValue>, lower: bool) -> Option<Reason> {
    match (old.map(number), new.map(number)) {
        (Some(Some(a)), Some(Some(b))) => {
            let (tighter, looser) = if lower { (b > a, b < a) } else { (b < a, b > a) };
            if tighter {
                Some(Reason::RangeNarrowed)
            } else if looser {
                Some(Reason::RangeWidened)
            } else if a == b {
                None
            } else {
                Some(Reason::UnclassifiedConstruct)
            }
        }
        (None, Some(Some(_))) => Some(Reason::RangeNarrowed),
        (Some(Some(_)), None) => Some(Reason::RangeWidened),
        _ => Some(Reason::UnclassifiedConstruct),
    }
}

fn canon_set(items: &[CValue]) -> Option<BTreeSet<Vec<u8>>> {
    items.iter().map(|v| to_canonical_bytes(v).ok()).collect()
}

/// Enums compared as sets. `None` means no effective change (same set, other order).
fn enum_reason(old: Option<&CValue>, new: Option<&CValue>) -> Option<Reason> {
    match (old, new) {
        (Some(CValue::Arr(a)), Some(CValue::Arr(b))) => match (canon_set(a), canon_set(b)) {
            (Some(a), Some(b)) if a == b => None,
            (Some(a), Some(b)) if a.is_subset(&b) => Some(Reason::EnumWidened),
            (Some(a), Some(b)) if b.is_subset(&a) => Some(Reason::EnumNarrowed),
            (Some(_), Some(_)) => Some(Reason::EnumChanged),
            _ => Some(Reason::UnclassifiedConstruct),
        },
        (None, Some(CValue::Arr(_))) => Some(Reason::EnumNarrowed),
        (Some(CValue::Arr(_)), None) => Some(Reason::EnumWidened),
        _ => Some(Reason::UnclassifiedConstruct),
    }
}

/// `properties` as a map; absent is empty, anything else but an object is `None`.
fn props_of<'a>(v: Option<&'a CValue>, empty: &'a BTreeMap<String, CValue>) -> Option<&'a BTreeMap<String, CValue>> {
    match v {
        None => Some(empty),
        Some(CValue::Obj(m)) => Some(m),
        Some(_) => None,
    }
}

/// `required` as a set of names; absent is empty, anything else but an array of strings is `None`.
fn names_of(v: Option<&CValue>) -> Option<BTreeSet<&str>> {
    match v {
        None => Some(BTreeSet::new()),
        Some(CValue::Arr(items)) => items
            .iter()
            .map(|x| match x {
                CValue::Str(s) => Some(s.as_str()),
                _ => None,
            })
            .collect(),
        Some(_) => None,
    }
}

fn has_default(schema: &CValue) -> bool {
    matches!(schema, CValue::Obj(m) if m.contains_key("default"))
}

fn compare_schema(old: &CValue, new: &CValue, ptr: &str, dir: Dir, depth: usize, out: &mut Vec<Rec>) {
    if old == new {
        return;
    }
    let (CValue::Obj(o), CValue::Obj(n)) = (old, new) else {
        // Boolean schemas, or a schema replaced by a non-object: no rule.
        push(out, ptr.to_string(), Some(old), Some(new), Reason::UnclassifiedConstruct);
        return;
    };
    if depth >= MAX_SCHEMA_DEPTH {
        push(out, ptr.to_string(), Some(old), Some(new), Reason::UnclassifiedConstruct);
        return;
    }
    let start = out.len();
    let mut unknown = false;
    let keys: BTreeSet<&str> = o.keys().chain(n.keys()).map(String::as_str).collect();
    for k in keys {
        let rule = keyword_rule(k);
        if rule == Rule::Unknown {
            unknown = true;
        }
        let (ov, nv) = (o.get(k), n.get(k));
        if ov == nv {
            continue;
        }
        let kp = format!("{ptr}/{}", token(k));
        let reason = match rule {
            Rule::Doc => Some(Reason::DocChanged),
            Rule::Ui => Some(Reason::UiHintChanged),
            Rule::Type => Some(Reason::TypeChanged),
            Rule::Quantity => Some(Reason::QuantityChanged),
            Rule::Unit => Some(Reason::UnitChanged),
            Rule::Ref => Some(Reason::RefChanged),
            Rule::Enum => enum_reason(ov, nv),
            Rule::Lower => bound_reason(ov, nv, true),
            Rule::Upper => bound_reason(ov, nv, false),
            Rule::Default => Some(Reason::DefaultChanged),
            Rule::Constraint => Some(Reason::ConstraintChanged),
            Rule::Unknown => Some(Reason::UnclassifiedConstruct),
            Rule::Items => match (ov, nv) {
                (Some(a), Some(b)) => {
                    compare_schema(a, b, &kp, dir, depth + 1, out);
                    None
                }
                // Adding or dropping `items` changes what arrays are accepted.
                _ => Some(Reason::ConstraintChanged),
            },
            // Compared together below.
            Rule::Properties | Rule::Required => None,
        };
        if let Some(r) = reason {
            push(out, kp, ov, nv, r);
        }
    }
    compare_members(o, n, ptr, dir, depth, out);
    if unknown {
        // A keyword without a rule may constrain anything in this schema object.
        for r in out.iter_mut().skip(start) {
            if r.reason.kind() == Kind::Additive {
                r.reason = Reason::UnclassifiedConstruct;
            }
        }
    }
}

/// `properties` by name and `required` as sets.
fn compare_members(
    o: &BTreeMap<String, CValue>,
    n: &BTreeMap<String, CValue>,
    ptr: &str,
    dir: Dir,
    depth: usize,
    out: &mut Vec<Rec>,
) {
    let empty = BTreeMap::new();
    let mut added = BTreeSet::new();
    let mut removed = BTreeSet::new();
    let (op, np) = (o.get("properties"), n.get("properties"));
    match (props_of(op, &empty), props_of(np, &empty)) {
        (Some(op), Some(np)) => {
            let new_required = names_of(n.get("required")).unwrap_or_default();
            let closed = n.get("additionalProperties") == Some(&CValue::Bool(false));
            let names: BTreeSet<&str> = op.keys().chain(np.keys()).map(String::as_str).collect();
            for name in names {
                let pp = format!("{ptr}/properties/{}", token(name));
                match (op.get(name), np.get(name)) {
                    (Some(a), Some(b)) => compare_schema(a, b, &pp, dir, depth + 1, out),
                    (Some(a), None) => {
                        removed.insert(name);
                        let r = match dir {
                            Dir::Input => Reason::ParamRemoved,
                            Dir::Output => Reason::ResultFieldRemoved,
                        };
                        push(out, pp, Some(a), None, r);
                    }
                    (None, Some(b)) => {
                        added.insert(name);
                        let r = match dir {
                            Dir::Output => Reason::ResultFieldAdded,
                            Dir::Input if new_required.contains(name) => Reason::RequiredParamAdded,
                            Dir::Input if !has_default(b) => Reason::OptionalParamWithoutDefault,
                            Dir::Input if !closed => Reason::ParamAddedToOpenObject,
                            Dir::Input => Reason::OptionalParamAdded,
                        };
                        push(out, pp, None, Some(b), r);
                    }
                    (None, None) => {}
                }
            }
        }
        _ => {
            if op != np {
                push(out, format!("{ptr}/properties"), op, np, Reason::UnclassifiedConstruct);
            }
        }
    }
    let (or, nr) = (o.get("required"), n.get("required"));
    if or == nr {
        return;
    }
    let reason = match (names_of(or), names_of(nr)) {
        (Some(a), Some(b)) => {
            // Names added or removed as properties were reported at their own pointer.
            let skip = |x: &&&str| !added.contains(*x) && !removed.contains(*x);
            let gained = b.difference(&a).filter(skip).count();
            let lost = a.difference(&b).filter(skip).count();
            match (gained, lost) {
                (0, 0) => None,
                (_, 0) if dir == Dir::Input => Some(Reason::ParamMadeRequired),
                _ => Some(Reason::RequiredChanged),
            }
        }
        _ => Some(Reason::UnclassifiedConstruct),
    };
    if let Some(r) = reason {
        push(out, format!("{ptr}/required"), or, nr, r);
    }
}

/// The rule table for top-level definition fields. `None`: not classified here.
fn field_rule(k: &str) -> Option<Reason> {
    Some(match k {
        "lifecycle" => return None,
        "id" | "major" => Reason::IdentityChanged,
        "title" | "description" => Reason::DocChanged,
        "web" | "web_note" => Reason::WebChanged,
        "cli_bare" => Reason::CliBareChanged,
        "side_effect" => Reason::SideEffectChanged,
        "mcp_projection" => Reason::McpProjectionChanged,
        "determinism" => Reason::DeterminismChanged,
        "idempotency" => Reason::IdempotencyChanged,
        "fs_effect" => Reason::FsEffectChanged,
        "undo" => Reason::UndoChanged,
        "network" => Reason::NetworkChanged,
        "human_only" => Reason::HumanOnlyChanged,
        "preconditions" => Reason::PreconditionChanged,
        "effects" => Reason::EffectsChanged,
        "algorithms" => Reason::AlgorithmsChanged,
        "exposure" => Reason::ExposureChanged,
        // Schemas are compared structurally by the caller; any other field has no rule.
        _ => Reason::UnclassifiedConstruct,
    })
}

/// Classifies every difference between two versions of one definition (REQ-001..003, 005).
///
/// Records are sorted by pointer and carry `old`'s (ID, major). Equal definitions give an
/// empty list. Lifecycle changes are not reported here (see [`diff`]).
pub fn classify(old: &CommandDef, new: &CommandDef) -> Vec<Change> {
    let command = CommandRef::of(old);
    let mut out = Vec::new();
    match (old.to_value(), new.to_value()) {
        (CValue::Obj(o), CValue::Obj(n)) => {
            let keys: BTreeSet<&str> = o.keys().chain(n.keys()).map(String::as_str).collect();
            for k in keys {
                let (ov, nv) = (o.get(k), n.get(k));
                if ov == nv {
                    continue;
                }
                let ptr = format!("/{}", token(k));
                let dir = match k {
                    "params_schema" => Some(Dir::Input),
                    "result_schema" => Some(Dir::Output),
                    _ => None,
                };
                match (dir, ov, nv) {
                    (Some(dir), Some(a), Some(b)) => compare_schema(a, b, &ptr, dir, 0, &mut out),
                    (Some(_), _, _) => push(&mut out, ptr, ov, nv, Reason::UnclassifiedConstruct),
                    (None, _, _) => {
                        if let Some(r) = field_rule(k) {
                            push(&mut out, ptr, ov, nv, r);
                        }
                    }
                }
            }
        }
        (a, b) => push(&mut out, String::new(), Some(&a), Some(&b), Reason::UnclassifiedConstruct),
    }
    let mut changes: Vec<Change> = out
        .into_iter()
        .map(|r| Change {
            command: command.clone(),
            pointer: r.pointer,
            old_digest: r.old,
            new_digest: r.new,
            kind: r.reason.kind(),
            reason: r.reason,
        })
        .collect();
    changes.sort();
    changes
}

// ------------------------------------------------------------------ manifests

/// A supplied manifest is not a valid manifest. Code `manifest_invalid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError(pub String);

impl ManifestError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        "manifest_invalid"
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "manifest_invalid: {}", self.0)
    }
}

impl std::error::Error for ManifestError {}

/// A Registry Manifest as read for diffing: `{format, quantity_catalogue_digest, commands}`
/// (`CORE.REG.MAN` REQ-001). Each command is a definition plus its `definition_digest`.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub format: u32,
    pub quantity_catalogue_digest: Digest,
    /// Definitions keyed by (ID, major).
    pub commands: BTreeMap<CommandRef, CommandDef>,
}

impl Manifest {
    /// Parses manifest JSON bytes; any problem is `manifest_invalid`.
    pub fn from_json(bytes: &[u8]) -> Result<Manifest, ManifestError> {
        let v = parse(bytes).map_err(|e| ManifestError(format!("json: {}", e.code())))?;
        Manifest::from_value(v)
    }

    /// Checks the structure, every definition (`CORE.REG.DEF.FIELD`) and every
    /// `definition_digest` (SHA-256 of the RFC 8785 form of the definition without that
    /// field), and refuses a repeated (ID, major).
    pub fn from_value(v: CValue) -> Result<Manifest, ManifestError> {
        let bad = |s: String| ManifestError(s);
        let CValue::Obj(mut o) = v else { return Err(bad("root must be an object".into())) };
        if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "format" | "quantity_catalogue_digest" | "commands")) {
            return Err(bad(format!("unknown field {k}")));
        }
        let format = match o.remove("format") {
            Some(CValue::Num(n)) if n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&n) => n as u32,
            _ => return Err(bad("format must be an integer".into())),
        };
        let quantity_catalogue_digest = match o.remove("quantity_catalogue_digest") {
            Some(CValue::Str(s)) => Digest::parse(&s).map_err(|_| bad("bad quantity_catalogue_digest".into()))?,
            _ => return Err(bad("quantity_catalogue_digest must be a string".into())),
        };
        let Some(CValue::Arr(items)) = o.remove("commands") else {
            return Err(bad("commands must be an array".into()));
        };
        let mut commands = BTreeMap::new();
        for (i, item) in items.into_iter().enumerate() {
            let CValue::Obj(mut def) = item else { return Err(bad(format!("commands[{i}] must be an object"))) };
            let stated = match def.remove("definition_digest") {
                Some(CValue::Str(s)) => Digest::parse(&s).map_err(|_| bad(format!("commands[{i}]: bad digest")))?,
                _ => return Err(bad(format!("commands[{i}]: definition_digest missing"))),
            };
            let body = CValue::Obj(def);
            if digest_of(Some(&body)) != Some(stated) {
                return Err(bad(format!("commands[{i}]: definition_digest mismatch")));
            }
            let def = CommandDef::from_value(body).map_err(|e: DefError| bad(format!("commands[{i}]: {e}")))?;
            let key = CommandRef::of(&def);
            if commands.contains_key(&key) {
                return Err(bad(format!("duplicate command {}@{}", key.id, key.major)));
            }
            commands.insert(key, def);
        }
        Ok(Manifest { format, quantity_catalogue_digest, commands })
    }
}

fn state_of(def: &CommandDef) -> Option<String> {
    match def.lifecycle.get("state") {
        Some(CValue::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Compares two manifests (REQ-004, REQ-006). Definitions are matched by (ID, major);
/// a definition whose lifecycle and contract both changed appears in both
/// `lifecycle_changed` and `changed`.
pub fn diff(a: &Manifest, b: &Manifest) -> ManifestDiff {
    let mut d = ManifestDiff::default();
    for (key, old) in &a.commands {
        match b.commands.get(key) {
            None => d.removed.push(key.clone()),
            Some(new) => {
                if old.lifecycle != new.lifecycle {
                    let (ol, nl) = (CValue::Obj(old.lifecycle.clone()), CValue::Obj(new.lifecycle.clone()));
                    d.lifecycle_changed.push(LifecycleChange {
                        command: key.clone(),
                        old_state: state_of(old),
                        new_state: state_of(new),
                        old_digest: digest_of(Some(&ol)),
                        new_digest: digest_of(Some(&nl)),
                    });
                }
                d.changed.extend(classify(old, new));
            }
        }
    }
    d.added = b.commands.keys().filter(|k| !a.commands.contains_key(*k)).cloned().collect();
    d.changed.sort();
    d
}

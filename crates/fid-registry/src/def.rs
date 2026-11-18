//! Definition record, enums and policy fields. Spec node `CORE.REG.DEF.FIELD`.
//!
//! JSON is read and written through `fid-canon`'s typed value model, so definitions hash
//! canonically. Unknown top-level fields are refused, which also means the record can
//! never carry a licence tier, entitlement or edition (REQ-009, D-001).

use fid_canon::{parse, CValue};
use fid_kci::AlgorithmId;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

macro_rules! closed_enum {
    ($(#[$m:meta])* $name:ident, $field:literal { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($variant,)+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            /// The JSON field this enum is read from.
            pub const FIELD: &'static str = $field;

            /// The wire string.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $s,)+
                }
            }

            /// Parses the wire string.
            pub fn parse(s: &str) -> Result<Self, DefError> {
                match s {
                    $($s => Ok($name::$variant),)+
                    other => Err(DefError::BadEnum { field: $field, value: other.to_string() }),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

closed_enum!(
    /// Side-effect class (CONSTITUTION §3.1).
    SideEffect, "side_effect" {
        R0 => "R0", R1 => "R1", W1 => "W1", W2 => "W2", W3 => "W3", X1 => "X1",
        I1 => "I1", D1 => "D1", M1 => "M1", S1 => "S1", A0 => "A0",
    }
);
closed_enum!(
    /// How the command appears to MCP clients. Ordered `Never` < `RequestOnly` < `Exposed`.
    Projection, "mcp_projection" { Never => "never", RequestOnly => "request_only", Exposed => "exposed" }
);
closed_enum!(
    /// Determinism class (CONSTITUTION §4).
    Determinism, "determinism" { D0 => "D0", D1 => "D1", D2 => "D2", D3 => "D3", N => "N" }
);
closed_enum!(
    /// Whether repeating a call is safe.
    Idempotency, "idempotency" { Natural => "natural", Keyed => "keyed", None => "none" }
);
closed_enum!(
    /// File-system effect outside the project store.
    FsEffect, "fs_effect" { None => "none", ReadRoot => "read_root", WriteRoot => "write_root" }
);
closed_enum!(
    /// Undo behaviour.
    Undo, "undo" { HeadMove => "head_move", NotUndoable => "not_undoable" }
);
closed_enum!(
    /// Network use.
    Network, "network" { None => "none", Egress => "egress", Device => "device" }
);
closed_enum!(
    /// Availability in the web build.
    Web, "web" { Available => "available", Partial => "partial", Unavailable => "unavailable" }
);
closed_enum!(
    /// Surfaces a command is exposed on.
    Surface, "exposure" { Cli => "cli", Mcp => "mcp", Python => "python", Gui => "gui", Control => "control" }
);

/// One command definition (REQ-CORE.REG.DEF.FIELD-001).
#[derive(Debug, Clone, PartialEq)]
pub struct CommandDef {
    /// Dotted lowercase name without the major, e.g. `align.run_321`.
    pub id: String,
    pub major: u32,
    pub title: String,
    pub description: String,
    /// Opaque here; checked by CORE.REG.DEF.SCHEMA.
    pub params_schema: CValue,
    pub result_schema: CValue,
    pub side_effect: SideEffect,
    pub mcp_projection: Projection,
    pub determinism: Determinism,
    pub idempotency: Idempotency,
    pub fs_effect: FsEffect,
    pub undo: Undo,
    /// Precondition sources; checked by CORE.REG.DEF.PRED.
    pub preconditions: Vec<String>,
    /// Journal event types this command may emit, e.g. `align.run_321@1`.
    pub effects: Vec<String>,
    pub algorithms: Vec<AlgorithmId>,
    pub exposure: BTreeSet<Surface>,
    /// Lifecycle state and metadata; rules live in CORE.REG.VER.LIFE.
    pub lifecycle: BTreeMap<String, CValue>,
    pub human_only: bool,
    pub network: Network,
    pub web: Web,
    pub web_note: Option<String>,
    pub cli_bare: bool,
}

/// Definition errors with stable codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefError {
    UnknownField(String),
    MissingField(&'static str),
    BadType { field: String, expected: &'static str },
    BadEnum { field: &'static str, value: String },
    BadId(String),
    BadAlgorithm(String),
    WebMissing,
    WebNoteMissing,
    /// The JSON could not be parsed (`fid-canon` error code).
    Json(&'static str),
}

impl DefError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            DefError::UnknownField(_) => "unknown_field",
            DefError::MissingField(_) => "missing_field",
            DefError::BadType { .. } => "bad_type",
            DefError::BadEnum { .. } => "bad_enum",
            DefError::BadId(_) => "bad_id",
            DefError::BadAlgorithm(_) => "bad_algorithm",
            DefError::WebMissing => "web_missing",
            DefError::WebNoteMissing => "web_note_missing",
            DefError::Json(c) => c,
        }
    }
}

impl fmt::Display for DefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DefError::UnknownField(k) => write!(f, "unknown_field: {k}"),
            DefError::MissingField(k) => write!(f, "missing_field: {k}"),
            DefError::BadType { field, expected } => write!(f, "bad_type: {field} must be {expected}"),
            DefError::BadEnum { field, value } => write!(f, "bad_enum: {field} = {value:?}"),
            DefError::BadId(s) => write!(f, "bad_id: {s:?}"),
            DefError::BadAlgorithm(s) => write!(f, "bad_algorithm: {s:?}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for DefError {}

/// Every field name, in the order of REQ-001.
pub const FIELDS: [&str; 22] = [
    "id",
    "major",
    "title",
    "description",
    "params_schema",
    "result_schema",
    "side_effect",
    "mcp_projection",
    "determinism",
    "idempotency",
    "fs_effect",
    "undo",
    "preconditions",
    "effects",
    "algorithms",
    "exposure",
    "lifecycle",
    "human_only",
    "network",
    "web",
    "web_note",
    "cli_bare",
];

/// `[a-z0-9_.]`, starting and ending with an alphanumeric.
fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
        && s.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && s.bytes().last().is_some_and(|b| b.is_ascii_alphanumeric())
}

struct Reader(BTreeMap<String, CValue>);

impl Reader {
    fn take(&mut self, k: &'static str) -> Option<CValue> {
        self.0.remove(k)
    }
    fn req(&mut self, k: &'static str) -> Result<CValue, DefError> {
        self.take(k).ok_or(DefError::MissingField(k))
    }
    fn string(&mut self, k: &'static str) -> Result<String, DefError> {
        match self.req(k)? {
            CValue::Str(s) => Ok(s),
            _ => Err(DefError::BadType { field: k.into(), expected: "a string" }),
        }
    }
    fn strings(&mut self, k: &'static str) -> Result<Vec<String>, DefError> {
        match self.req(k)? {
            CValue::Arr(items) => items
                .into_iter()
                .map(|v| match v {
                    CValue::Str(s) => Ok(s),
                    _ => Err(DefError::BadType { field: k.into(), expected: "an array of strings" }),
                })
                .collect(),
            _ => Err(DefError::BadType { field: k.into(), expected: "an array of strings" }),
        }
    }
    fn flag(&mut self, k: &'static str) -> Result<bool, DefError> {
        match self.take(k) {
            None => Ok(false),
            Some(CValue::Bool(b)) => Ok(b),
            Some(_) => Err(DefError::BadType { field: k.into(), expected: "a boolean" }),
        }
    }
}

impl CommandDef {
    /// Parses one definition from JSON bytes.
    pub fn from_json(bytes: &[u8]) -> Result<Self, DefError> {
        Self::from_value(parse(bytes).map_err(|e| DefError::Json(e.code()))?)
    }

    /// Builds a definition from a typed JSON value, enforcing every field rule.
    pub fn from_value(v: CValue) -> Result<Self, DefError> {
        let CValue::Obj(map) = v else {
            return Err(DefError::BadType { field: "<root>".into(), expected: "an object" });
        };
        if let Some(k) = map.keys().find(|k| !FIELDS.contains(&k.as_str())) {
            return Err(DefError::UnknownField(k.clone()));
        }
        let mut r = Reader(map);
        let id = r.string("id")?;
        if !valid_id(&id) {
            return Err(DefError::BadId(id));
        }
        let major = match r.req("major")? {
            CValue::Num(n) if n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&n) => n as u32,
            _ => return Err(DefError::BadType { field: "major".into(), expected: "an integer 0..2^32" }),
        };
        let title = r.string("title")?;
        let description = r.string("description")?;
        let params_schema = r.req("params_schema")?;
        let result_schema = r.req("result_schema")?;
        let side_effect = SideEffect::parse(&r.string("side_effect")?)?;
        let mcp_projection = Projection::parse(&r.string("mcp_projection")?)?;
        let determinism = Determinism::parse(&r.string("determinism")?)?;
        let idempotency = Idempotency::parse(&r.string("idempotency")?)?;
        let fs_effect = FsEffect::parse(&r.string("fs_effect")?)?;
        let undo = Undo::parse(&r.string("undo")?)?;
        let preconditions = r.strings("preconditions")?;
        let effects = r.strings("effects")?;
        let algorithms = r
            .strings("algorithms")?
            .into_iter()
            .map(|s| AlgorithmId::parse(&s).map_err(|_| DefError::BadAlgorithm(s)))
            .collect::<Result<Vec<_>, _>>()?;
        let exposure = r.strings("exposure")?.iter().map(|s| Surface::parse(s)).collect::<Result<BTreeSet<_>, _>>()?;
        let lifecycle = match r.req("lifecycle")? {
            CValue::Obj(o) => o,
            _ => return Err(DefError::BadType { field: "lifecycle".into(), expected: "an object" }),
        };
        let human_only = r.flag("human_only")?;
        let network = match r.take("network") {
            None => Network::None,
            Some(CValue::Str(s)) => Network::parse(&s)?,
            Some(_) => return Err(DefError::BadType { field: "network".into(), expected: "a string" }),
        };
        let web = match r.take("web") {
            None => return Err(DefError::WebMissing),
            Some(CValue::Str(s)) => Web::parse(&s)?,
            Some(_) => return Err(DefError::BadType { field: "web".into(), expected: "a string" }),
        };
        let web_note = match r.take("web_note") {
            None => None,
            Some(CValue::Str(s)) => Some(s),
            Some(_) => return Err(DefError::BadType { field: "web_note".into(), expected: "a string" }),
        };
        if web != Web::Available && web_note.as_deref().map(str::trim).unwrap_or_default().is_empty() {
            return Err(DefError::WebNoteMissing);
        }
        let cli_bare = r.flag("cli_bare")?;
        Ok(CommandDef {
            id,
            major,
            title,
            description,
            params_schema,
            result_schema,
            side_effect,
            mcp_projection,
            determinism,
            idempotency,
            fs_effect,
            undo,
            preconditions,
            effects,
            algorithms,
            exposure,
            lifecycle,
            human_only,
            network,
            web,
            web_note,
            cli_bare,
        })
    }

    /// The JSON value of this definition; defaults are written out explicitly.
    pub fn to_value(&self) -> CValue {
        let strs = |v: &[String]| CValue::Arr(v.iter().map(|s| CValue::Str(s.clone())).collect());
        let s = |x: &str| CValue::Str(x.to_string());
        let mut o = BTreeMap::new();
        o.insert("id".into(), s(&self.id));
        o.insert("major".into(), CValue::Num(f64::from(self.major)));
        o.insert("title".into(), s(&self.title));
        o.insert("description".into(), s(&self.description));
        o.insert("params_schema".into(), self.params_schema.clone());
        o.insert("result_schema".into(), self.result_schema.clone());
        o.insert("side_effect".into(), s(self.side_effect.as_str()));
        o.insert("mcp_projection".into(), s(self.mcp_projection.as_str()));
        o.insert("determinism".into(), s(self.determinism.as_str()));
        o.insert("idempotency".into(), s(self.idempotency.as_str()));
        o.insert("fs_effect".into(), s(self.fs_effect.as_str()));
        o.insert("undo".into(), s(self.undo.as_str()));
        o.insert("preconditions".into(), strs(&self.preconditions));
        o.insert("effects".into(), strs(&self.effects));
        o.insert("algorithms".into(), CValue::Arr(self.algorithms.iter().map(|a| s(&a.to_string())).collect()));
        o.insert("exposure".into(), CValue::Arr(self.exposure.iter().map(|x| s(x.as_str())).collect()));
        o.insert("lifecycle".into(), CValue::Obj(self.lifecycle.clone()));
        o.insert("human_only".into(), CValue::Bool(self.human_only));
        o.insert("network".into(), s(self.network.as_str()));
        o.insert("web".into(), s(self.web.as_str()));
        if let Some(n) = &self.web_note {
            o.insert("web_note".into(), s(n));
        }
        o.insert("cli_bare".into(), CValue::Bool(self.cli_bare));
        CValue::Obj(o)
    }

    /// The projection MCP clients actually get (REQ-005): a `human_only` command is
    /// never more visible than `request_only`.
    pub fn effective_projection(&self) -> Projection {
        if self.human_only && self.mcp_projection > Projection::RequestOnly {
            Projection::RequestOnly
        } else {
            self.mcp_projection
        }
    }
}

// ------------------------------------------------------------------ schema

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|x| CValue::Str((*x).to_string())).collect())
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn enum_of(values: &[&str]) -> CValue {
    obj([("enum", strs(values))])
}

fn string_array() -> CValue {
    obj([("type", s("array")), ("items", obj([("type", s("string"))]))])
}

/// JSON Schema (2020-12) for definitions; checked in as `schema/command-def.schema.json` (REQ-010).
pub fn command_def_schema() -> CValue {
    let side: Vec<&str> = SideEffect::ALL.iter().map(|x| x.as_str()).collect();
    let proj: Vec<&str> = Projection::ALL.iter().map(|x| x.as_str()).collect();
    let det: Vec<&str> = Determinism::ALL.iter().map(|x| x.as_str()).collect();
    let idem: Vec<&str> = Idempotency::ALL.iter().map(|x| x.as_str()).collect();
    let fs: Vec<&str> = FsEffect::ALL.iter().map(|x| x.as_str()).collect();
    let undo: Vec<&str> = Undo::ALL.iter().map(|x| x.as_str()).collect();
    let net: Vec<&str> = Network::ALL.iter().map(|x| x.as_str()).collect();
    let web: Vec<&str> = Web::ALL.iter().map(|x| x.as_str()).collect();
    let surf: Vec<&str> = Surface::ALL.iter().map(|x| x.as_str()).collect();
    let optional = ["human_only", "network", "web_note", "cli_bare"];
    let required: Vec<&str> = FIELDS.iter().copied().filter(|f| !optional.contains(f)).collect();
    obj([
        ("$schema", s("https://json-schema.org/draft/2020-12/schema")),
        ("$id", s("https://metrologymaven.io/schema/command-def.schema.json")),
        ("title", s("Metrology Maven command definition")),
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&required)),
        (
            "properties",
            obj([
                ("id", obj([("type", s("string")), ("pattern", s("^[a-z0-9]([a-z0-9_.]*[a-z0-9])?$"))])),
                ("major", obj([("type", s("integer")), ("minimum", CValue::Num(0.0)), ("maximum", CValue::Num(f64::from(u32::MAX)))])),
                ("title", obj([("type", s("string"))])),
                ("description", obj([("type", s("string"))])),
                ("params_schema", obj([])),
                ("result_schema", obj([])),
                ("side_effect", enum_of(&side)),
                ("mcp_projection", enum_of(&proj)),
                ("determinism", enum_of(&det)),
                ("idempotency", enum_of(&idem)),
                ("fs_effect", enum_of(&fs)),
                ("undo", enum_of(&undo)),
                ("preconditions", string_array()),
                ("effects", string_array()),
                (
                    "algorithms",
                    obj([
                        ("type", s("array")),
                        (
                            "items",
                            obj([
                                ("type", s("string")),
                                (
                                    "pattern",
                                    s("^[a-z][a-z0-9_]*\\.[a-z][a-z0-9_]*\\.[a-z][a-z0-9_]*@(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$"),
                                ),
                            ]),
                        ),
                    ]),
                ),
                ("exposure", obj([("type", s("array")), ("uniqueItems", CValue::Bool(true)), ("items", enum_of(&surf))])),
                ("lifecycle", obj([("type", s("object"))])),
                ("human_only", obj([("type", s("boolean"))])),
                ("network", enum_of(&net)),
                ("web", enum_of(&web)),
                ("web_note", obj([("type", s("string"))])),
                ("cli_bare", obj([("type", s("boolean"))])),
            ]),
        ),
        (
            "if",
            obj([("properties", obj([("web", enum_of(&["partial", "unavailable"]))]))]),
        ),
        ("then", obj([("required", strs(&["web_note"])), ("properties", obj([("web_note", obj([("minLength", CValue::Num(1.0))]))]))])),
    ])
}

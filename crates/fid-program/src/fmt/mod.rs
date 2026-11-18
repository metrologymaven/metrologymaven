//! Command Program document format. Spec node `AUTO.PROG.FMT`.
//!
//! A program is stored as RFC 8785 canonical JSON (through `fid-canon`), so one program
//! has exactly one byte form and one digest. Parsing is the only way in, and it fails
//! closed: size, nesting, value and step counts are bounded before or while the tree is
//! built, unknown fields are refused, and every step carries a UUIDv7 that is unique
//! within the program. Expressions, parameter declarations and operator descriptors are
//! kept as source text or opaque objects for `AUTO.PROG.EXPR`, `AUTO.PROG.FAMILY` and
//! `AUTO.PROG.OPER`.

mod shape;
mod uuid;

pub use shape::{program_schema, COMMAND_PATTERN, IDENT_PATTERN, UUID7_PATTERN};
pub use uuid::Uuid7;

use fid_canon::{digest_bytes, format_number, parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_types::Digest;
use shape::{Shape, Ty};
use std::collections::{BTreeMap, BTreeSet};

/// The only `format` value.
pub const FORMAT: &str = "fid-program";
/// The only supported `format_version`.
pub const FORMAT_VERSION: u32 = 1;
/// Largest accepted document, in bytes (REQ-AUTO.PROG.FMT-006).
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
/// Deepest accepted nesting of JSON arrays and objects (REQ-AUTO.PROG.FMT-006).
pub const MAX_DEPTH: usize = 64;
/// Most steps in one program, counting nested and subroutine steps (REQ-AUTO.PROG.FMT-006).
pub const MAX_STEPS: usize = 100_000;
/// Most JSON structural tokens (`[`, `{`, `,`, `:`), checked before the tree is built so the
/// in-memory tree stays bounded.
pub const MAX_TOKENS: usize = 2_000_000;
/// Most JSON objects, checked before the tree is built (objects cost the most memory).
pub const MAX_OBJECTS: usize = 250_000;

/// Opaque JSON object (meta, parameter declaration, operator descriptor).
pub type Opaque = BTreeMap<String, CValue>;

/// Which bound a document exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    Bytes,
    Depth,
    Tokens,
    Objects,
    Steps,
}

impl Bound {
    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Bound::Bytes => "bytes",
            Bound::Depth => "depth",
            Bound::Tokens => "tokens",
            Bound::Objects => "objects",
            Bound::Steps => "steps",
        }
    }
}

/// Program format errors with stable codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramError {
    /// `format_version` is a number other than 1; holds its canonical text.
    FormatVersionUnsupported(String),
    /// The bytes are JSON but not the canonical form of themselves.
    NonCanonical,
    /// Two steps share this ID.
    DuplicateStepId(Uuid7),
    /// An object carries a field its shape does not declare. `path` is a JSON Pointer.
    UnknownField { path: String, field: String },
    /// Any other schema violation. `path` is a JSON Pointer.
    SchemaInvalid { path: String, reason: String },
    /// A bound of REQ-AUTO.PROG.FMT-006 was exceeded.
    ProgramTooLarge(Bound),
    /// Not JSON at all (invalid UTF-8, syntax, duplicate key); holds the `fid-canon` code.
    InvalidJson(&'static str),
}

impl ProgramError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            ProgramError::FormatVersionUnsupported(_) => "format_version_unsupported",
            ProgramError::NonCanonical => "non_canonical",
            ProgramError::DuplicateStepId(_) => "duplicate_step_id",
            ProgramError::UnknownField { .. } => "unknown_field",
            ProgramError::SchemaInvalid { .. } => "schema_invalid",
            ProgramError::ProgramTooLarge(_) => "program_too_large",
            ProgramError::InvalidJson(_) => "invalid_json",
        }
    }
}

impl std::fmt::Display for ProgramError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgramError::FormatVersionUnsupported(v) => write!(f, "format_version_unsupported: {v}"),
            ProgramError::DuplicateStepId(id) => write!(f, "duplicate_step_id: {id}"),
            ProgramError::UnknownField { path, field } => write!(f, "unknown_field: {field:?} at {path:?}"),
            ProgramError::SchemaInvalid { path, reason } => write!(f, "schema_invalid: {reason} at {path:?}"),
            ProgramError::ProgramTooLarge(b) => write!(f, "program_too_large: {}", b.as_str()),
            ProgramError::InvalidJson(c) => write!(f, "invalid_json: {c}"),
            ProgramError::NonCanonical => f.write_str("non_canonical"),
        }
    }
}

impl std::error::Error for ProgramError {}

// ------------------------------------------------------------------ model

/// A Command Program, format version 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub format_version: u32,
    pub program_id: Uuid7,
    /// Free metadata (title, author, notes); opaque here.
    pub meta: Opaque,
    /// Part-family parameter declarations; validated by `AUTO.PROG.FAMILY`.
    pub params: Vec<Opaque>,
    pub variables: Vec<VarDecl>,
    pub subroutines: Vec<Subroutine>,
    pub main: Vec<Step>,
}

/// A typed variable or subroutine parameter. `ty` is type source for `AUTO.PROG.EXPR`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarDecl {
    pub name: String,
    pub ty: String,
}

/// A subroutine with typed parameters and an optional return type.
#[derive(Debug, Clone, PartialEq)]
pub struct Subroutine {
    pub name: String,
    pub params: Vec<VarDecl>,
    pub returns: Option<String>,
    pub body: Vec<Step>,
}

/// One step: a stable ID and what it does.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub step_id: Uuid7,
    pub kind: StepKind,
}

/// A command or call argument.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    /// `{"literal": <JSON>}`: a recorded value, stored canonically.
    Literal(CValue),
    /// `{"expr": "<source>"}`: expression source for `AUTO.PROG.EXPR`.
    Expr(String),
}

/// One `if` / `else if` branch.
#[derive(Debug, Clone, PartialEq)]
pub struct Branch {
    pub cond: String,
    pub body: Vec<Step>,
}

/// Loop form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopMode {
    Counted { var: String, from: String, to: String, step: String },
    While { cond: String },
}

/// The closed set of step kinds. `Invoke` is the only one that names a state-changing
/// action (REQ-AUTO.PROG.FMT-004).
#[derive(Debug, Clone, PartialEq)]
pub enum StepKind {
    Invoke { command: String, major: u32, args: BTreeMap<String, Arg>, bind: Option<String> },
    Assign { var: String, expr: String },
    If { branches: Vec<Branch>, else_body: Vec<Step> },
    Loop { mode: LoopMode, max_iterations: u32, body: Vec<Step> },
    Call { subroutine: String, args: BTreeMap<String, Arg>, bind: Option<String> },
    Return { expr: Option<String> },
    Label { name: String },
    Jump { label: String, cond: Option<String> },
    Operator { descriptor: Opaque },
    Comment { text: String },
}

impl StepKind {
    /// The wire `kind`.
    pub fn as_str(&self) -> &'static str {
        match self {
            StepKind::Invoke { .. } => "invoke",
            StepKind::Assign { .. } => "assign",
            StepKind::If { .. } => "if",
            StepKind::Loop { .. } => "loop",
            StepKind::Call { .. } => "call",
            StepKind::Return { .. } => "return",
            StepKind::Label { .. } => "label",
            StepKind::Jump { .. } => "jump",
            StepKind::Operator { .. } => "operator",
            StepKind::Comment { .. } => "comment",
        }
    }
}

impl Program {
    /// Parses stored bytes. They must be the canonical form of a valid program
    /// (REQ-AUTO.PROG.FMT-001..006).
    pub fn parse(bytes: &[u8]) -> Result<Program, ProgramError> {
        if bytes.len() > MAX_BYTES {
            return Err(ProgramError::ProgramTooLarge(Bound::Bytes));
        }
        prescan(bytes)?;
        let value = parse_with_limits(bytes, MAX_BYTES, MAX_DEPTH).map_err(|e| match e {
            CanonError::InputLimit => ProgramError::ProgramTooLarge(Bound::Depth),
            other => ProgramError::InvalidJson(other.code()),
        })?;
        check_header(&value)?;
        let canonical = to_canonical_bytes(&value).map_err(|e| ProgramError::InvalidJson(e.code()))?;
        if canonical.as_slice() != bytes {
            return Err(ProgramError::NonCanonical);
        }
        drop(canonical);
        let mut dec = Decoder::default();
        let program = dec.program(value)?;
        // The decoder is lossless by construction; this guards the invariant of REQ-002.
        if program.encode_bytes()?.as_slice() != bytes {
            return Err(ProgramError::NonCanonical);
        }
        Ok(program)
    }

    /// The stored (canonical) bytes. Fails if the program would not parse back, so
    /// everything this returns is a valid stored program.
    pub fn to_canonical(&self) -> Result<Vec<u8>, ProgramError> {
        let bytes = self.encode_bytes()?;
        Program::parse(&bytes)?;
        Ok(bytes)
    }

    /// SHA-256 of the canonical bytes, as `sha256:<hex>` (REQ-AUTO.PROG.FMT-005).
    pub fn digest(&self) -> Result<Digest, ProgramError> {
        self.to_canonical().map(|b| Digest::from_bytes(digest_bytes(&b).0))
    }

    /// Every step ID in document order (main, then subroutines; depth first).
    pub fn step_ids(&self) -> Vec<Uuid7> {
        fn walk(steps: &[Step], out: &mut Vec<Uuid7>) {
            for s in steps {
                out.push(s.step_id);
                match &s.kind {
                    StepKind::If { branches, else_body } => {
                        for b in branches {
                            walk(&b.body, out);
                        }
                        walk(else_body, out);
                    }
                    StepKind::Loop { body, .. } => walk(body, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.main, &mut out);
        for sub in &self.subroutines {
            walk(&sub.body, &mut out);
        }
        out
    }

    fn encode_bytes(&self) -> Result<Vec<u8>, ProgramError> {
        to_canonical_bytes(&self.encode()?).map_err(|e| ProgramError::InvalidJson(e.code()))
    }
}

/// Counts structural tokens and objects outside strings, so hostile input cannot build a
/// huge tree (REQ-AUTO.PROG.FMT-006, AC-6).
fn prescan(bytes: &[u8]) -> Result<(), ProgramError> {
    let (mut tokens, mut objects) = (0usize, 0usize);
    let (mut in_str, mut escaped) = (false, false);
    for &c in bytes {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_str = false;
            }
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' => {
                objects += 1;
                tokens += 1;
                if objects > MAX_OBJECTS {
                    return Err(ProgramError::ProgramTooLarge(Bound::Objects));
                }
            }
            b'[' | b',' | b':' => tokens += 1,
            _ => continue,
        }
        if tokens > MAX_TOKENS {
            return Err(ProgramError::ProgramTooLarge(Bound::Tokens));
        }
    }
    Ok(())
}

/// `format` and `format_version` are checked before anything else, so a document of
/// another version is refused by version, not by whatever else changed.
fn check_header(v: &CValue) -> Result<(), ProgramError> {
    let CValue::Obj(map) = v else {
        return Err(invalid(&ROOT, "a program must be a JSON object"));
    };
    if !matches!(map.get("format"), Some(CValue::Str(f)) if f == FORMAT) {
        return Err(invalid(&ROOT.key("format"), format!("must be {FORMAT:?}")));
    }
    match map.get("format_version") {
        Some(n) if shape::as_u32(n) == Some(FORMAT_VERSION) => Ok(()),
        Some(CValue::Num(n)) => {
            Err(ProgramError::FormatVersionUnsupported(format_number(*n).unwrap_or_else(|_| String::from("?"))))
        }
        _ => Err(invalid(&ROOT.key("format_version"), "must be an integer")),
    }
}

// ------------------------------------------------------------------ paths

#[derive(Clone, Copy)]
enum Seg<'a> {
    Root,
    Key(&'a str),
    Idx(usize),
}

/// A JSON Pointer built lazily: rendered only when an error is reported.
#[derive(Clone, Copy)]
struct At<'a> {
    parent: Option<&'a At<'a>>,
    seg: Seg<'a>,
}

const ROOT: At<'static> = At { parent: None, seg: Seg::Root };

impl<'a> At<'a> {
    fn key(&'a self, k: &'a str) -> At<'a> {
        At { parent: Some(self), seg: Seg::Key(k) }
    }

    fn idx(&'a self, i: usize) -> At<'a> {
        At { parent: Some(self), seg: Seg::Idx(i) }
    }

    fn render(&self) -> String {
        let mut segs = Vec::new();
        let mut cur = Some(self);
        while let Some(a) = cur {
            segs.push(a.seg);
            cur = a.parent;
        }
        let mut out = String::new();
        for seg in segs.iter().rev() {
            match seg {
                Seg::Root => {}
                Seg::Key(k) => {
                    out.push('/');
                    out.push_str(&k.replace('~', "~0").replace('/', "~1"));
                }
                Seg::Idx(i) => {
                    out.push('/');
                    out.push_str(&i.to_string());
                }
            }
        }
        out
    }
}

fn invalid(at: &At<'_>, reason: impl Into<String>) -> ProgramError {
    ProgramError::SchemaInvalid { path: at.render(), reason: reason.into() }
}

// ------------------------------------------------------------------ decoding

/// An object checked against its shape: no unknown field, every required field present,
/// every present field of the declared type.
struct Fields<'a> {
    map: Opaque,
    at: &'a At<'a>,
}

fn open<'a>(v: CValue, shape: &Shape, at: &'a At<'a>) -> Result<Fields<'a>, ProgramError> {
    let CValue::Obj(map) = v else {
        return Err(invalid(at, "must be an object"));
    };
    if let Some(k) = map.keys().find(|k| !shape.fields.iter().any(|f| f.name == k.as_str())) {
        return Err(ProgramError::UnknownField { path: at.render(), field: k.clone() });
    }
    for f in shape.fields {
        match map.get(f.name) {
            None if f.required => return Err(invalid(at, format!("missing field {:?}", f.name))),
            Some(v) if !f.ty.admits(v) => return Err(invalid(&at.key(f.name), format!("must be {}", f.ty.expected()))),
            _ => {}
        }
    }
    Ok(Fields { map, at })
}

impl Fields<'_> {
    fn take(&mut self, k: &'static str) -> Result<CValue, ProgramError> {
        self.map.remove(k).ok_or_else(|| invalid(self.at, format!("missing field {k:?}")))
    }

    fn text(&mut self, k: &'static str) -> Result<String, ProgramError> {
        match self.take(k)? {
            CValue::Str(s) => Ok(s),
            _ => Err(invalid(&self.at.key(k), "must be a string")),
        }
    }

    fn opt_text(&mut self, k: &'static str) -> Result<Option<String>, ProgramError> {
        if self.map.contains_key(k) {
            self.text(k).map(Some)
        } else {
            Ok(None)
        }
    }

    fn u32(&mut self, k: &'static str) -> Result<u32, ProgramError> {
        let v = self.take(k)?;
        shape::as_u32(&v).ok_or_else(|| invalid(&self.at.key(k), "must be an integer"))
    }

    fn uuid(&mut self, k: &'static str) -> Result<Uuid7, ProgramError> {
        let s = self.text(k)?;
        Uuid7::parse(&s).ok_or_else(|| invalid(&self.at.key(k), "must be a lowercase hyphenated UUIDv7"))
    }

    fn object(&mut self, k: &'static str) -> Result<Opaque, ProgramError> {
        match self.take(k)? {
            CValue::Obj(m) => Ok(m),
            _ => Err(invalid(&self.at.key(k), "must be an object")),
        }
    }

    fn array(&mut self, k: &'static str) -> Result<Vec<CValue>, ProgramError> {
        match self.take(k)? {
            CValue::Arr(items) => Ok(items),
            _ => Err(invalid(&self.at.key(k), "must be an array")),
        }
    }
}

#[derive(Default)]
struct Decoder {
    count: usize,
    ids: BTreeSet<Uuid7>,
}

impl Decoder {
    fn program(&mut self, v: CValue) -> Result<Program, ProgramError> {
        let root = ROOT;
        let mut f = open(v, &shape::PROGRAM, &root)?;
        let format_version = f.u32("format_version")?;
        let program_id = f.uuid("program_id")?;
        let meta = f.object("meta")?;
        let at_p = root.key("params");
        let mut params = Vec::new();
        for (i, item) in f.array("params")?.into_iter().enumerate() {
            match item {
                CValue::Obj(m) => params.push(m),
                _ => return Err(invalid(&at_p.idx(i), "must be an object")),
            }
        }
        let at_v = root.key("variables");
        let mut variables = Vec::new();
        for (i, item) in f.array("variables")?.into_iter().enumerate() {
            variables.push(var_decl(item, &at_v.idx(i))?);
        }
        // Main first, then subroutines: the canonical byte order of the two keys.
        let main = self.steps(f.array("main")?, &root.key("main"))?;
        let at_s = root.key("subroutines");
        let mut subroutines = Vec::new();
        for (i, item) in f.array("subroutines")?.into_iter().enumerate() {
            subroutines.push(self.subroutine(item, &at_s.idx(i))?);
        }
        Ok(Program { format_version, program_id, meta, params, variables, subroutines, main })
    }

    fn subroutine(&mut self, v: CValue, at: &At<'_>) -> Result<Subroutine, ProgramError> {
        let mut f = open(v, &shape::SUBROUTINE, at)?;
        let name = f.text("name")?;
        let at_p = at.key("params");
        let mut params = Vec::new();
        for (i, item) in f.array("params")?.into_iter().enumerate() {
            params.push(var_decl(item, &at_p.idx(i))?);
        }
        let returns = f.opt_text("returns")?;
        let body = self.steps(f.array("body")?, &at.key("body"))?;
        Ok(Subroutine { name, params, returns, body })
    }

    fn steps(&mut self, items: Vec<CValue>, at: &At<'_>) -> Result<Vec<Step>, ProgramError> {
        let mut out = Vec::with_capacity(items.len());
        for (i, item) in items.into_iter().enumerate() {
            out.push(self.step(item, &at.idx(i))?);
        }
        Ok(out)
    }

    fn step(&mut self, v: CValue, at: &At<'_>) -> Result<Step, ProgramError> {
        self.count += 1;
        if self.count > MAX_STEPS {
            return Err(ProgramError::ProgramTooLarge(Bound::Steps));
        }
        let (kind, counted, shape) = {
            let CValue::Obj(map) = &v else {
                return Err(invalid(at, "a step must be an object"));
            };
            let Some(CValue::Str(kind)) = map.get("kind") else {
                return Err(invalid(&at.key("kind"), "must be a step kind"));
            };
            let mode = match map.get("mode") {
                Some(CValue::Str(m)) => Some(m.as_str()),
                _ => None,
            };
            let Some(shape) = shape::step_shape(kind, mode) else {
                return Err(invalid(at, format!("unknown step kind {kind:?} (or loop mode)")));
            };
            (kind.clone(), mode == Some("counted"), shape)
        };
        let mut f = open(v, shape, at)?;
        let step_id = f.uuid("step_id")?;
        if !self.ids.insert(step_id) {
            return Err(ProgramError::DuplicateStepId(step_id));
        }
        let kind = match kind.as_str() {
            "invoke" => StepKind::Invoke {
                command: f.text("command")?,
                major: f.u32("major")?,
                args: args(f.object("args")?, &at.key("args"))?,
                bind: f.opt_text("bind")?,
            },
            "assign" => StepKind::Assign { var: f.text("var")?, expr: f.text("expr")? },
            "if" => {
                let at_b = at.key("branches");
                let mut branches = Vec::new();
                for (i, item) in f.array("branches")?.into_iter().enumerate() {
                    let at_i = at_b.idx(i);
                    let mut b = open(item, &shape::BRANCH, &at_i)?;
                    let cond = b.text("cond")?;
                    let body = self.steps(b.array("body")?, &at_i.key("body"))?;
                    branches.push(Branch { cond, body });
                }
                let else_body = self.steps(f.array("else_body")?, &at.key("else_body"))?;
                StepKind::If { branches, else_body }
            }
            "loop" => {
                let mode = if counted {
                    LoopMode::Counted {
                        var: f.text("var")?,
                        from: f.text("from")?,
                        to: f.text("to")?,
                        step: f.text("step")?,
                    }
                } else {
                    LoopMode::While { cond: f.text("cond")? }
                };
                let max_iterations = f.u32("max_iterations")?;
                let body = self.steps(f.array("body")?, &at.key("body"))?;
                StepKind::Loop { mode, max_iterations, body }
            }
            "call" => StepKind::Call {
                subroutine: f.text("subroutine")?,
                args: args(f.object("args")?, &at.key("args"))?,
                bind: f.opt_text("bind")?,
            },
            "return" => StepKind::Return { expr: f.opt_text("expr")? },
            "label" => StepKind::Label { name: f.text("name")? },
            "jump" => StepKind::Jump { label: f.text("label")?, cond: f.opt_text("cond")? },
            "operator" => StepKind::Operator { descriptor: f.object("descriptor")? },
            "comment" => StepKind::Comment { text: f.text("text")? },
            other => return Err(invalid(at, format!("unknown step kind {other:?}"))),
        };
        Ok(Step { step_id, kind })
    }
}

fn var_decl(v: CValue, at: &At<'_>) -> Result<VarDecl, ProgramError> {
    let mut f = open(v, &shape::VAR_DECL, at)?;
    Ok(VarDecl { name: f.text("name")?, ty: f.text("type")? })
}

fn args(map: Opaque, at: &At<'_>) -> Result<BTreeMap<String, Arg>, ProgramError> {
    let mut out = BTreeMap::new();
    for (name, v) in map {
        let at_n = at.key(&name);
        let shape = match &v {
            CValue::Obj(m) if m.contains_key("literal") => &shape::ARG_LITERAL,
            CValue::Obj(m) if m.contains_key("expr") => &shape::ARG_EXPR,
            _ => return Err(invalid(&at_n, "an argument must be {\"literal\": <JSON>} or {\"expr\": \"<source>\"}")),
        };
        let mut f = open(v, shape, &at_n)?;
        let arg =
            if f.map.contains_key("literal") { Arg::Literal(f.take("literal")?) } else { Arg::Expr(f.text("expr")?) };
        out.insert(name, arg);
    }
    Ok(out)
}

// ------------------------------------------------------------------ encoding

/// Deepest step nesting the encoder walks; deeper programs cannot be stored anyway
/// (each level costs at least two JSON levels of [`MAX_DEPTH`]).
const MAX_STEP_NESTING: usize = MAX_DEPTH / 2;

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> BTreeMap<String, CValue> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn num(n: u32) -> CValue {
    CValue::Num(f64::from(n))
}

fn enc_var(v: &VarDecl) -> CValue {
    CValue::Obj(obj([("name", s(&v.name)), ("type", s(&v.ty))]))
}

fn enc_args(args: &BTreeMap<String, Arg>) -> CValue {
    CValue::Obj(
        args.iter()
            .map(|(k, a)| {
                let v = match a {
                    Arg::Literal(l) => obj([("literal", l.clone())]),
                    Arg::Expr(e) => obj([("expr", s(e))]),
                };
                (k.clone(), CValue::Obj(v))
            })
            .collect(),
    )
}

fn enc_steps(steps: &[Step], depth: usize) -> Result<CValue, ProgramError> {
    if depth > MAX_STEP_NESTING {
        return Err(ProgramError::ProgramTooLarge(Bound::Depth));
    }
    steps.iter().map(|st| enc_step(st, depth)).collect::<Result<Vec<_>, _>>().map(CValue::Arr)
}

fn enc_step(st: &Step, depth: usize) -> Result<CValue, ProgramError> {
    let mut m = obj([("step_id", s(&st.step_id.to_string())), ("kind", s(st.kind.as_str()))]);
    let mut put = |k: &str, v: CValue| {
        m.insert(k.to_string(), v);
    };
    match &st.kind {
        StepKind::Invoke { command, major, args, bind } => {
            put("command", s(command));
            put("major", num(*major));
            put("args", enc_args(args));
            if let Some(b) = bind {
                put("bind", s(b));
            }
        }
        StepKind::Assign { var, expr } => {
            put("var", s(var));
            put("expr", s(expr));
        }
        StepKind::If { branches, else_body } => {
            let mut bs = Vec::with_capacity(branches.len());
            for b in branches {
                bs.push(CValue::Obj(obj([("cond", s(&b.cond)), ("body", enc_steps(&b.body, depth + 1)?)])));
            }
            put("branches", CValue::Arr(bs));
            put("else_body", enc_steps(else_body, depth + 1)?);
        }
        StepKind::Loop { mode, max_iterations, body } => {
            match mode {
                LoopMode::Counted { var, from, to, step } => {
                    put("mode", s("counted"));
                    put("var", s(var));
                    put("from", s(from));
                    put("to", s(to));
                    put("step", s(step));
                }
                LoopMode::While { cond } => {
                    put("mode", s("while"));
                    put("cond", s(cond));
                }
            }
            put("max_iterations", num(*max_iterations));
            put("body", enc_steps(body, depth + 1)?);
        }
        StepKind::Call { subroutine, args, bind } => {
            put("subroutine", s(subroutine));
            put("args", enc_args(args));
            if let Some(b) = bind {
                put("bind", s(b));
            }
        }
        StepKind::Return { expr } => {
            if let Some(e) = expr {
                put("expr", s(e));
            }
        }
        StepKind::Label { name } => put("name", s(name)),
        StepKind::Jump { label, cond } => {
            put("label", s(label));
            if let Some(c) = cond {
                put("cond", s(c));
            }
        }
        StepKind::Operator { descriptor } => put("descriptor", CValue::Obj(descriptor.clone())),
        StepKind::Comment { text } => put("text", s(text)),
    }
    Ok(CValue::Obj(m))
}

impl Program {
    fn encode(&self) -> Result<CValue, ProgramError> {
        let mut subs = Vec::with_capacity(self.subroutines.len());
        for sub in &self.subroutines {
            let mut m = obj([
                ("name", s(&sub.name)),
                ("params", CValue::Arr(sub.params.iter().map(enc_var).collect())),
                ("body", enc_steps(&sub.body, 1)?),
            ]);
            if let Some(r) = &sub.returns {
                m.insert("returns".into(), s(r));
            }
            subs.push(CValue::Obj(m));
        }
        Ok(CValue::Obj(obj([
            ("format", s(FORMAT)),
            ("format_version", num(self.format_version)),
            ("program_id", s(&self.program_id.to_string())),
            ("meta", CValue::Obj(self.meta.clone())),
            ("params", CValue::Arr(self.params.iter().map(|p| CValue::Obj(p.clone())).collect())),
            ("variables", CValue::Arr(self.variables.iter().map(enc_var).collect())),
            ("subroutines", CValue::Arr(subs)),
            ("main", enc_steps(&self.main, 1)?),
        ])))
    }
}

/// Every step kind, in declaration order.
pub const STEP_KINDS: [&str; 10] =
    ["invoke", "assign", "if", "loop", "call", "return", "label", "jump", "operator", "comment"];

/// Every field name a step of this kind may carry, from the field tables. Lets callers
/// (and tests) confirm that only `invoke` has a `command` (REQ-AUTO.PROG.FMT-004).
pub fn step_fields(kind: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = shape::STEP_SHAPES
        .iter()
        .filter(|s| s.fields.iter().any(|f| f.name == "kind" && matches!(f.ty, Ty::Const(c) if c == kind)))
        .flat_map(|s| s.fields.iter().map(|f| f.name))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}
